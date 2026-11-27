import SwiftUI

/// Live TV on iPhone. Folded, it is a list of sources that drills into
/// channels, and a channel plays full screen. Unfolded, it is a guide with
/// the player docked beside it, so you can keep browsing while you watch:
/// on top in portrait, on the right in landscape.
struct LiveView: View {
    @Environment(AppModel.self) private var model
    @State private var selection: GuideSource = .favorites
    @State private var isShowingChannels = false

    /// Narrowest width that fits sources and channels side by side: the
    /// unfolded screen is about 670 points wide, the cover screen 466.
    private static let wideWidthMin: CGFloat = 600

    var body: some View {
        GeometryReader { proxy in
            let isWide = proxy.size.width >= Self.wideWidthMin
            Group {
                if isWide {
                    wide(isLandscape: proxy.size.width > proxy.size.height * 1.15)
                } else {
                    compact
                }
            }
            .onChange(of: isWide) { _, nowWide in
                // Folding the phone mid-stream carries the stream to full
                // screen instead of cutting it.
                if !nowWide && model.presentation == .docked {
                    model.presentation = .fullScreen
                }
            }
        }
        .background(Canvas())
        .animation(.snappy, value: model.player?.session.url)
        .onAppear {
            if !model.favorites.contains(where: { $0.kind == .live }) {
                selection = .all
            }
        }
    }

    // MARK: Folded

    private var compact: some View {
        SourceList(selection: selection) { source in
            selection = source
            isShowingChannels = true
        }
        .navigationTitle("Live TV")
        .navigationDestination(isPresented: $isShowingChannels) {
            channels(play: fullScreen)
                .background(Canvas())
                .navigationTitle(title(of: selection))
                .navigationBarTitleDisplayMode(.inline)
        }
    }

    // MARK: Unfolded

    @ViewBuilder
    private func wide(isLandscape: Bool) -> some View {
        if isLandscape {
            HStack(spacing: 0) {
                sources
                channels(play: docked).id(selection)
                VStack(spacing: 14) {
                    stage
                    Spacer(minLength: 0)
                }
                .frame(maxWidth: .infinity)
                .padding(16)
            }
            .toolbar(.hidden, for: .navigationBar)
        } else {
            VStack(spacing: 0) {
                stage.padding(16)
                HStack(spacing: 0) {
                    sources
                    channels(play: docked).id(selection)
                }
            }
            .toolbar(.hidden, for: .navigationBar)
        }
    }

    private var sources: some View {
        SourceList(selection: selection) { selection = $0 }
            .frame(width: 236)
            .background(.black.opacity(0.25))
    }

    /// The docked player and what's on it, or an invitation to pick.
    @ViewBuilder
    private var stage: some View {
        if let player = model.player, model.presentation == .docked {
            VStack(alignment: .leading, spacing: 10) {
                DockedPlayer(controller: player)
                    .onDisappear {
                        // Leaving the screen (another tab) keeps the stream,
                        // full screen, rather than cutting it.
                        if model.presentation == .docked {
                            model.presentation = .fullScreen
                        }
                    }
                NowPlayingBar(request: player.session.request)
            }
            .transition(.opacity.combined(with: .scale(scale: 0.98)))
        } else {
            RoundedRectangle(cornerRadius: Theme.cornerRadius, style: .continuous)
                .fill(Theme.raised)
                .aspectRatio(16 / 9, contentMode: .fit)
                .overlay {
                    VStack(spacing: 10) {
                        Image(systemName: "play.tv").font(.system(size: 40, weight: .light))
                        Text("Pick a channel to watch it here").font(.callout)
                    }
                    .foregroundStyle(.secondary)
                }
        }
    }

    @ViewBuilder
    private func channels(play: @escaping (PlayRequest) -> Void) -> some View {
        if selection == .sports {
            SportsGuide(onPlay: play)
        } else {
            ChannelList(source: selection, onPlay: play)
        }
    }

    private func fullScreen(_ request: PlayRequest) {
        Task { await model.play(request) }
    }

    private func docked(_ request: PlayRequest) {
        Task { await model.play(request, docked: true) }
    }

    private func title(of source: GuideSource) -> String {
        switch source {
        case .favorites: "Favorites"
        case .recent: "Recently Watched"
        case .all: "All Channels"
        case .sports: "Sports Today"
        case .category(let category): RegionGroups.shortName(category)
        }
    }
}

/// Under the docked player: what's on, and the two ways out.
private struct NowPlayingBar: View {
    let request: PlayRequest
    @Environment(AppModel.self) private var model

    var body: some View {
        HStack(spacing: 14) {
            Circle().fill(Theme.accent).frame(width: 8, height: 8)
            VStack(alignment: .leading, spacing: 2) {
                Text(request.name).font(.headline).lineLimit(1)
                if let subtitle = request.subtitle {
                    Text(subtitle).font(.caption).foregroundStyle(.secondary).lineLimit(1)
                }
            }
            Spacer(minLength: 8)
            Button {
                model.presentation = .fullScreen
            } label: {
                Image(systemName: "arrow.up.left.and.arrow.down.right")
            }
            .accessibilityLabel("Full Screen")
            Button {
                withAnimation(.snappy) { model.stopPlayback() }
            } label: {
                Image(systemName: "xmark")
            }
            .accessibilityLabel("Stop")
        }
        .buttonStyle(.glass)
    }
}
