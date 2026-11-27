import SwiftUI
#if os(tvOS)
@preconcurrency import TVVLCKit
#else
@preconcurrency import MobileVLCKit
#endif

/// Plays a stream with VLC for what AVFoundation can't open. VLC draws
/// into a plain view, so this also provides the controls: play and pause,
/// skipping, scrubbing, and the audio and subtitle tracks Matroska files
/// usually carry.
@MainActor
@Observable
final class VLCPlayerController: NSObject, PlaybackController {
    let session: PlaybackSession
    /// VLC draws the video into this view.
    @ObservationIgnored let videoView = UIView()
    private(set) var isPlaying = false
    private(set) var isBuffering = true
    private(set) var seconds: Double = 0
    private(set) var duration: Double = 0
    private(set) var audioTracks: [Track] = []
    private(set) var subtitleTracks: [Track] = []
    private(set) var audioTrack: Int32 = -1
    private(set) var subtitleTrack: Int32 = -1

    struct Track: Identifiable, Hashable {
        let id: Int32
        let name: String
    }

    @ObservationIgnored private let player = VLCMediaPlayer()
    @ObservationIgnored private let positions: PlaybackPositions
    @ObservationIgnored private var continuation: AsyncStream<PlaybackOutcome>.Continuation?
    @ObservationIgnored private var lastSaved: Double = 0

    /// Milliseconds of network buffer. IPTV panels deliver in bursts, and
    /// 1.5 s (live) and 4 s (VOD) ran dry between them. Common IPTV
    /// players use 3 to 10 s; this costs a few seconds at start, once.
    private static let liveCachingMs = 5000
    private static let vodCachingMs = 10000
    /// VLC's id for "no track".
    private static let disabledTrack: Int32 = -1
    /// How often the position is saved, as in `PlayerController`.
    private static let saveIntervalSeconds: Double = 10

    init(session: PlaybackSession, positions: PlaybackPositions) {
        self.session = session
        self.positions = positions
        super.init()
        videoView.backgroundColor = .black
        let media = VLCMedia(url: session.url)
        let caching = session.isLive ? Self.liveCachingMs : Self.vodCachingMs
        media.addOption(":network-caching=\(caching)")
        media.addOption(":live-caching=\(caching)")
        // A dropped connection resumes where it was instead of stalling.
        media.addOption(":http-reconnect")
        if let start = session.startSeconds {
            media.addOption(":start-time=\(Int(start))")
        }
        player.media = media
        player.drawable = videoView
        player.delegate = self
        player.play()
    }

    func outcomes() -> AsyncStream<PlaybackOutcome> {
        AsyncStream { continuation in
            self.continuation = continuation
        }
    }

    func stop() {
        savePosition()
        player.delegate = nil
        player.stop()
        continuation?.finish()
        continuation = nil
    }

    func togglePlayPause() {
        if player.isPlaying { player.pause() } else { player.play() }
    }

    func skip(seconds delta: Int32) {
        guard !session.isLive else { return }
        if delta >= 0 { player.jumpForward(delta) } else { player.jumpBackward(-delta) }
    }

    /// `fraction` of the whole title, 0 to 1.
    func seek(to fraction: Double) {
        guard !session.isLive, player.isSeekable else { return }
        player.position = Float(min(max(fraction, 0), 1))
    }

    func selectAudio(_ track: Track) {
        player.currentAudioTrackIndex = track.id
        audioTrack = track.id
    }

    func selectSubtitle(_ track: Track) {
        player.currentVideoSubTitleIndex = track.id
        subtitleTrack = track.id
    }

    fileprivate func stateChanged() {
        isPlaying = player.isPlaying
        switch player.state {
        case .buffering, .opening:
            isBuffering = true
        case .playing:
            isBuffering = false
            refreshTracks()
        case .paused, .esAdded:
            isBuffering = false
            refreshTracks()
        case .ended:
            positions.record(session.request.kind, session.request.id, seconds: duration, duration: max(duration, 1))
            continuation?.yield(.finished)
            continuation?.finish()
        case .error:
            continuation?.yield(.failed("The stream didn’t start. It may be offline at your provider right now."))
            continuation?.finish()
        case .stopped:
            break
        @unknown default:
            break
        }
    }

    fileprivate func timeChanged() {
        seconds = Double(player.time.intValue) / 1000
        if let length = player.media?.length.value?.doubleValue, length > 0 {
            duration = length / 1000
        }
        isBuffering = false
        if seconds - lastSaved >= Self.saveIntervalSeconds || seconds < lastSaved {
            savePosition()
        }
    }

    private func savePosition() {
        guard !session.isLive, duration > 0 else { return }
        lastSaved = seconds
        positions.record(session.request.kind, session.request.id, seconds: seconds, duration: duration)
    }

    /// VLC lists a fake "Disable" track (id -1) first in both lists. It is
    /// "Off" for subtitles, and dropped for audio: silence is the mute
    /// button's job.
    private func refreshTracks() {
        audioTracks = Self.tracks(names: player.audioTrackNames, indexes: player.audioTrackIndexes)
            .filter { $0.id != Self.disabledTrack }
        subtitleTracks = Self.tracks(names: player.videoSubTitlesNames, indexes: player.videoSubTitlesIndexes)
            .map { $0.id == Self.disabledTrack ? Track(id: $0.id, name: "Off") : $0 }
        audioTrack = player.currentAudioTrackIndex
        subtitleTrack = player.currentVideoSubTitleIndex
    }

    /// VLCKit 3 returns untyped parallel arrays; anything malformed is
    /// skipped rather than trusted.
    private static func tracks(names: [Any], indexes: [Any]) -> [Track] {
        zip(names, indexes).compactMap { name, index in
            guard let name = name as? String, let index = (index as? NSNumber)?.int32Value else { return nil }
            return Track(id: index, name: name)
        }
    }
}

extension VLCPlayerController: VLCMediaPlayerDelegate {
    // VLCKit 3 delivers its delegate calls on the main thread.
    nonisolated func mediaPlayerStateChanged(_ aNotification: Notification) {
        MainActor.assumeIsolated { stateChanged() }
    }

    nonisolated func mediaPlayerTimeChanged(_ aNotification: Notification) {
        MainActor.assumeIsolated { timeChanged() }
    }
}

/// VLC's video surface in SwiftUI.
private struct VLCVideoSurface: UIViewRepresentable {
    let controller: VLCPlayerController

    func makeUIView(context: Context) -> UIView { controller.videoView }
    func updateUIView(_ view: UIView, context: Context) {}
}

/// The VLC player with its controls. The controls show on any input and
/// fade after a few seconds, like the system player's.
struct VLCPlayerScreen: View {
    let controller: VLCPlayerController
    var onClose: () -> Void

    @State private var areControlsVisible = true
    @State private var hideTask: Task<Void, Never>?
    #if os(tvOS)
    /// The swipe-down panel with subtitles and audio, as in Apple's player.
    @State private var isTrackPanelOpen = false
    #endif

    /// How long the controls stay up without input.
    private static let controlsTimeout: Duration = .seconds(4)
    private static let skipSeconds: Int32 = 10

    var body: some View {
        ZStack {
            Color.black.ignoresSafeArea()
            VLCVideoSurface(controller: controller).ignoresSafeArea()
            if controller.isBuffering {
                ProgressView().controlSize(.large).tint(.white)
            }
            if areControlsVisible {
                controls.transition(.opacity)
            }
            #if os(tvOS)
            if isTrackPanelOpen {
                VStack {
                    TrackPanel(controller: controller)
                    Spacer()
                }
                .transition(.move(edge: .top).combined(with: .opacity))
            }
            #endif
        }
        .animation(.easeOut(duration: 0.25), value: areControlsVisible)
        .onAppear(perform: showControls)
        #if os(tvOS)
        .animation(.snappy, value: isTrackPanelOpen)
        // The screen holds focus while the panel is shut, so the remote's
        // swipes and clicks reach the player; the panel takes it when open.
        .focusable(!isTrackPanelOpen)
        .onTapGesture {
            controller.togglePlayPause()
            showControls()
        }
        .onPlayPauseCommand {
            controller.togglePlayPause()
            showControls()
        }
        .onMoveCommand { direction in
            guard !isTrackPanelOpen else {
                if direction == .up { isTrackPanelOpen = false }
                return
            }
            switch direction {
            case .left: controller.skip(seconds: -Self.skipSeconds)
            case .right: controller.skip(seconds: Self.skipSeconds)
            case .down:
                if controller.audioTracks.count > 1 || controller.subtitleTracks.count > 1 {
                    isTrackPanelOpen = true
                }
            default: break
            }
            showControls()
        }
        .onExitCommand {
            if isTrackPanelOpen {
                isTrackPanelOpen = false
            } else if areControlsVisible {
                areControlsVisible = false
            } else {
                onClose()
            }
        }
        #else
        .onTapGesture { areControlsVisible ? (areControlsVisible = false) : showControls() }
        .persistentSystemOverlays(.hidden)
        #endif
    }

    private var controls: some View {
        VStack(spacing: Theme.itemSpacing * 0.5) {
            HStack {
                #if os(iOS)
                Button(action: onClose) { Image(systemName: "xmark") }
                    .buttonStyle(.glass)
                #endif
                VStack(alignment: .leading, spacing: 2) {
                    Text(DisplayTitle(controller.session.request.name, library: .movies).text)
                        .font(.headline).lineLimit(1)
                    if let subtitle = controller.session.request.subtitle {
                        Text(subtitle).font(.caption).foregroundStyle(.secondary).lineLimit(1)
                    }
                }
                Spacer()
                #if os(tvOS)
                if controller.audioTracks.count > 1 || controller.subtitleTracks.count > 1 {
                    Label("Swipe down for Subtitles & Audio", systemImage: "chevron.down")
                        .font(.caption)
                        .padding(.horizontal, 20).padding(.vertical, 10)
                        .glassEffect()
                }
                #else
                trackMenus
                #endif
            }
            Spacer()
            HStack(spacing: Theme.itemSpacing) {
                if !controller.session.isLive {
                    Button { controller.skip(seconds: -Self.skipSeconds); showControls() } label: {
                        Image(systemName: "gobackward.10")
                    }
                }
                Button { controller.togglePlayPause(); showControls() } label: {
                    Image(systemName: controller.isPlaying ? "pause.fill" : "play.fill")
                        .font(.system(size: Theme.iconSize * 0.6))
                }
                if !controller.session.isLive {
                    Button { controller.skip(seconds: Self.skipSeconds); showControls() } label: {
                        Image(systemName: "goforward.10")
                    }
                }
            }
            .font(.system(size: Theme.iconSize * 0.45))
            .buttonStyle(.glass)
            Spacer()
            if !controller.session.isLive && controller.duration > 0 {
                Scrubber(controller: controller, onChange: showControls)
            }
        }
        .padding(Theme.contentInset + 16)
        .background {
            LinearGradient(
                colors: [.black.opacity(0.7), .clear, .clear, .black.opacity(0.7)],
                startPoint: .top, endPoint: .bottom
            )
            .ignoresSafeArea()
            .allowsHitTesting(false)
        }
        .foregroundStyle(.white)
    }

    @ViewBuilder private var trackMenus: some View {
        HStack(spacing: 12) {
            if controller.audioTracks.count > 1 {
                Menu {
                    ForEach(controller.audioTracks) { track in
                        Button {
                            controller.selectAudio(track)
                        } label: {
                            if track.id == controller.audioTrack {
                                Label(track.name, systemImage: "checkmark")
                            } else {
                                Text(track.name)
                            }
                        }
                    }
                } label: {
                    Image(systemName: "waveform")
                }
                .accessibilityLabel("Audio")
            }
            if controller.subtitleTracks.count > 1 {
                Menu {
                    ForEach(controller.subtitleTracks) { track in
                        Button {
                            controller.selectSubtitle(track)
                        } label: {
                            if track.id == controller.subtitleTrack {
                                Label(track.name, systemImage: "checkmark")
                            } else {
                                Text(track.name)
                            }
                        }
                    }
                } label: {
                    Image(systemName: "captions.bubble")
                }
                .accessibilityLabel("Subtitles")
            }
        }
        .buttonStyle(.glass)
    }

    private func showControls() {
        #if os(tvOS)
        // The panel keeps the controls up: they say what is playing.
        guard !isTrackPanelOpen else { return }
        #endif
        areControlsVisible = true
        hideTask?.cancel()
        hideTask = Task {
            try? await Task.sleep(for: Self.controlsTimeout)
            if !Task.isCancelled && controller.isPlaying {
                areControlsVisible = false
            }
        }
    }
}

/// Elapsed and remaining time around a progress bar. Dragging it seeks on
/// touch screens; on tvOS, left and right skip instead.
private struct Scrubber: View {
    let controller: VLCPlayerController
    let onChange: () -> Void
    @State private var dragFraction: Double?

    var body: some View {
        let fraction = dragFraction ?? (controller.seconds / max(controller.duration, 1))
        HStack(spacing: 14) {
            Text(clock(controller.seconds)).monospacedDigit()
            GeometryReader { proxy in
                ProgressBar(progress: fraction)
                    .frame(maxHeight: .infinity)
                    .contentShape(Rectangle())
                    #if os(iOS)
                    .gesture(
                        DragGesture(minimumDistance: 0)
                            .onChanged { value in
                                dragFraction = min(max(value.location.x / proxy.size.width, 0), 1)
                                onChange()
                            }
                            .onEnded { _ in
                                if let dragFraction { controller.seek(to: dragFraction) }
                                dragFraction = nil
                            }
                    )
                    #endif
            }
            .frame(height: 28)
            Text("-" + clock(max(controller.duration - controller.seconds, 0))).monospacedDigit()
        }
        .font(.caption)
    }

    private func clock(_ seconds: Double) -> String {
        let total = Int(seconds.rounded(.down))
        let hours = total / 3600, minutes = (total % 3600) / 60, rest = total % 60
        return hours > 0
            ? String(format: "%d:%02d:%02d", hours, minutes, rest)
            : String(format: "%d:%02d", minutes, rest)
    }
}

#if os(tvOS)
/// Subtitles and audio, one row each, like the system player's panel.
private struct TrackPanel: View {
    let controller: VLCPlayerController

    var body: some View {
        VStack(alignment: .leading, spacing: 34) {
            if controller.subtitleTracks.count > 1 {
                row("Subtitles", tracks: controller.subtitleTracks, selected: controller.subtitleTrack) {
                    controller.selectSubtitle($0)
                }
            }
            if controller.audioTracks.count > 1 {
                row("Audio", tracks: controller.audioTracks, selected: controller.audioTrack) {
                    controller.selectAudio($0)
                }
            }
        }
        .padding(44)
        .frame(maxWidth: .infinity, alignment: .leading)
        .glassEffect(in: RoundedRectangle(cornerRadius: 36, style: .continuous))
        .padding(.horizontal, 80)
        .padding(.top, 50)
        .focusSection()
    }

    private func row(
        _ title: String,
        tracks: [VLCPlayerController.Track],
        selected: Int32,
        choose: @escaping (VLCPlayerController.Track) -> Void
    ) -> some View {
        VStack(alignment: .leading, spacing: 14) {
            Text(title).font(.headline).foregroundStyle(.secondary)
            ScrollView(.horizontal) {
                HStack(spacing: 18) {
                    ForEach(tracks) { track in
                        Button {
                            choose(track)
                        } label: {
                            HStack(spacing: 10) {
                                if track.id == selected {
                                    Image(systemName: "checkmark")
                                }
                                Text(track.name).lineLimit(1)
                            }
                        }
                    }
                }
                .padding(.vertical, 12)
            }
            .scrollClipDisabled()
        }
    }
}
#endif
