import AVKit
import SwiftUI

/// Full-screen playback in the system player, so the remote and touch
/// gestures work the way they do everywhere else on the device.
struct PlayerView: View {
    let controller: any PlaybackController
    @Environment(AppModel.self) private var model

    var body: some View {
        ZStack {
            Color.black.ignoresSafeArea()
            if let system = controller as? PlayerController {
                SystemPlayer(controller: system)
                    .ignoresSafeArea()
            } else if let vlc = controller as? VLCPlayerController {
                VLCPlayerScreen(controller: vlc) { model.stopPlayback() }
            }
        }
        .onDisappear {
            // Leaving full screen ends playback, unless it moved to
            // Picture in Picture or the docked pane.
            if model.presentation == .fullScreen {
                model.stopPlayback()
            }
        }
    }
}

/// The player docked inside a screen (the unfolded iPhone's Live TV):
/// inline controls, plus AVKit's own full-screen, AirPlay and PiP buttons.
struct DockedPlayer: View {
    let controller: any PlaybackController
    @Environment(AppModel.self) private var model

    var body: some View {
        Group {
            if let system = controller as? PlayerController {
                SystemPlayer(controller: system)
            } else if let vlc = controller as? VLCPlayerController {
                VLCPlayerScreen(controller: vlc) { model.stopPlayback() }
            }
        }
            .aspectRatio(16 / 9, contentMode: .fit)
            .background(.black)
            .clipShape(RoundedRectangle(cornerRadius: Theme.cornerRadius, style: .continuous))
    }
}

/// How the current stream is shown.
enum PlayerPresentation: Equatable {
    case fullScreen
    /// Inline in a screen, next to the guide.
    case docked
    /// Floating over other apps; the app's own player UI is gone.
    case pictureInPicture
}

/// Owns everything for one stream: the AVPlayer, the system player view
/// controller (kept alive through Picture in Picture), resume, metadata,
/// and the two ways playback ends.
@MainActor
final class PlayerController: NSObject, PlaybackController {
    let player: AVPlayer
    let viewController = AVPlayerViewController()
    let session: PlaybackSession
    /// Picture in Picture started (true) or stopped (false).
    var onPictureInPicture: ((Bool) -> Void)?
    /// The viewer tapped "back to app" on the PiP window.
    var onRestore: (() -> Void)?

    private let positions: PlaybackPositions
    private var timeObserver: Any?

    /// How often the position is saved: often enough that a crash or a
    /// power cut loses little, rarely enough to cost nothing.
    private static let saveInterval = CMTime(seconds: 10, preferredTimescale: 1)

    init(session: PlaybackSession, positions: PlaybackPositions) {
        self.session = session
        self.positions = positions
        let item = AVPlayerItem(url: session.url)
        item.externalMetadata = Self.metadata(for: session.request)
        // No cap on the buffer: panels deliver in bursts, and a small fixed
        // cushion (6 s, before) ran dry between them and stalled. Zero lets
        // AVFoundation size it from the network, as Apple's apps do.
        item.preferredForwardBufferDuration = 0
        // Resuming a paused live channel starts from a full buffer.
        item.canUseNetworkResourcesForLiveStreamingWhilePaused = session.isLive
        player = AVPlayer(playerItem: item)
        // AirPlay hands the URL to the Apple TV, which streams it itself.
        player.allowsExternalPlayback = true
        super.init()
        viewController.player = player
        #if os(iOS)
        viewController.allowsPictureInPicturePlayback = true
        viewController.canStartPictureInPictureAutomaticallyFromInline = true
        viewController.delegate = self
        #endif
        if let start = session.startSeconds {
            player.seek(to: CMTime(seconds: start, preferredTimescale: 600))
        }
        if !session.isLive {
            timeObserver = player.addPeriodicTimeObserver(
                forInterval: Self.saveInterval, queue: .main
            ) { [weak self] _ in
                MainActor.assumeIsolated { self?.savePosition() }
            }
        }
        player.play()
        Task { await self.loadArtwork() }
    }

    /// Ends when the title finishes or fails.
    func outcomes() -> AsyncStream<PlaybackOutcome> {
        AsyncStream { continuation in
            guard let item = player.currentItem else {
                continuation.finish()
                return
            }
            let ext = session.request.ext
            let ended = Task { @MainActor in
                let endings = NotificationCenter.default.notifications(
                    named: AVPlayerItem.didPlayToEndTimeNotification, object: item
                )
                for await _ in endings {
                    continuation.yield(.finished)
                    continuation.finish()
                    return
                }
            }
            let status = item.observe(\.status, options: [.new]) { item, _ in
                guard item.status == .failed else { return }
                let reason = PlayerController.describe(item.error, ext: ext)
                continuation.yield(.failed(reason))
                continuation.finish()
            }
            continuation.onTermination = { _ in
                ended.cancel()
                status.invalidate()
            }
        }
    }

    func stop() {
        savePosition()
        if let timeObserver {
            player.removeTimeObserver(timeObserver)
        }
        timeObserver = nil
        player.pause()
        player.replaceCurrentItem(with: nil)
    }

    private func savePosition() {
        guard !session.isLive, let item = player.currentItem else { return }
        let duration = item.duration.seconds
        let seconds = player.currentTime().seconds
        guard duration.isFinite, seconds.isFinite else { return }
        positions.record(session.request.kind, session.request.id, seconds: seconds, duration: duration)
    }

    private func loadArtwork() async {
        guard let image = session.request.image, let url = URL(string: image),
              let data = await ImagePipeline.shared.data(for: url)
        else {
            // Artwork is decoration for the info panel; none is fine.
            return
        }
        let artwork = AVMutableMetadataItem()
        artwork.identifier = .commonIdentifierArtwork
        artwork.value = data as NSData
        artwork.dataType = kCMMetadataBaseDataType_JPEG as String
        player.currentItem?.externalMetadata.append(artwork)
    }

    private static func metadata(for request: PlayRequest) -> [AVMetadataItem] {
        var items: [AVMetadataItem] = []
        func add(_ identifier: AVMetadataIdentifier, _ value: String?) {
            guard let value, !value.isEmpty else { return }
            let item = AVMutableMetadataItem()
            item.identifier = identifier
            item.value = value as NSString
            item.extendedLanguageTag = "und"
            items.append(item)
        }
        let library: Library = request.kind == .live ? .live : .movies
        add(.commonIdentifierTitle, DisplayTitle(request.name, library: library).text)
        add(.iTunesMetadataTrackSubTitle, request.subtitle)
        add(.commonIdentifierDescription, request.summary)
        return items
    }

    /// AVFoundation's errors name codecs and OSStatus codes; say what it
    /// means for the viewer instead.
    nonisolated static func describe(_ error: (any Error)?, ext: String?) -> String {
        // `ext` stays for callers' sake; containers AVFoundation can't open
        // never reach it: they go to VLC first (see PlaybackEngine).
        _ = ext
        if let error = error as? URLError {
            return error.code == .userAuthenticationRequired
                ? "Your provider refused the stream. Your plan may allow fewer screens than are in use."
                : "The stream stopped answering. Check your connection and try again."
        }
        return "The stream didn’t start. It may be offline at your provider right now."
    }
}

#if os(iOS)
extension PlayerController: AVPlayerViewControllerDelegate {
    // The app dismisses its own full-screen cover (see AppModel), so the
    // view controller must not try to dismiss itself.
    nonisolated func playerViewControllerShouldAutomaticallyDismissAtPictureInPictureStart(
        _ playerViewController: AVPlayerViewController
    ) -> Bool {
        false
    }

    nonisolated func playerViewControllerWillStartPictureInPicture(
        _ playerViewController: AVPlayerViewController
    ) {
        MainActor.assumeIsolated { onPictureInPicture?(true) }
    }

    nonisolated func playerViewControllerDidStopPictureInPicture(
        _ playerViewController: AVPlayerViewController
    ) {
        MainActor.assumeIsolated { onPictureInPicture?(false) }
    }

    nonisolated func playerViewController(
        _ playerViewController: AVPlayerViewController,
        restoreUserInterfaceForPictureInPictureStopWithCompletionHandler completionHandler: @escaping (Bool) -> Void
    ) {
        MainActor.assumeIsolated { onRestore?() }
        completionHandler(true)
    }
}
#endif

/// The controller's long-lived AVPlayerViewController, placed in SwiftUI.
/// Reusing one instance is what lets Picture in Picture outlive the screen
/// that started it.
private struct SystemPlayer: UIViewControllerRepresentable {
    let controller: PlayerController

    func makeUIViewController(context: Context) -> AVPlayerViewController {
        controller.viewController
    }

    func updateUIViewController(_ viewController: AVPlayerViewController, context: Context) {}
}
