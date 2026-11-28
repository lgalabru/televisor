import Foundation

/// What the viewer asked to watch.
struct PlayRequest: Sendable, Hashable {
    let kind: MediaKind
    let id: UInt64
    let name: String
    let ext: String?
    let image: String?
    /// Shown under the title in the player's info panel.
    var subtitle: String?
    var summary: String?
    /// The show an episode belongs to, to remember where the viewer is.
    var seriesId: UInt64?
}

/// A resolved stream, ready for the player.
struct PlaybackSession {
    let url: URL
    let request: PlayRequest
    let isLive: Bool
    /// Where to resume, in seconds; nil starts at the beginning.
    let startSeconds: Double?
    /// `url` is the remuxer's loopback playlist, not the panel's file.
    var isRemuxed = false

    /// The same title, played from the remuxer at `url`.
    func remuxed(at url: URL) -> PlaybackSession {
        var session = PlaybackSession(url: url, request: request, isLive: isLive, startSeconds: startSeconds)
        session.isRemuxed = true
        return session
    }
}

/// App-wide state: who is signed in, their favorites and history, and
/// what is playing.
@MainActor
@Observable
final class AppModel {
    enum Phase: Equatable {
        case launching
        case signedOut
        /// A saved account exists but the panel could not be reached.
        /// The account is kept: asking to sign in again would be wrong.
        case unreachable(BridgeError)
        case signedIn(AccountOverview)
    }

    private(set) var phase: Phase = .launching
    private(set) var favorites: [Entry] = []
    private(set) var history: [Entry] = []
    private(set) var host: String?
    /// Bumped when the library is refreshed, so screens reload.
    private(set) var libraryGeneration = 0
    /// The stream playing now, in whichever presentation.
    private(set) var player: (any PlaybackController)?
    var presentation: PlayerPresentation = .fullScreen
    private var playerOutcomes: Task<Void, Never>?
    /// The remuxer's playlist while it serves the current title.
    private var remuxedURL: URL?
    /// Bumped whenever playback stops, so a title that finishes opening
    /// after the viewer moved on is dropped.
    private var playbackGeneration = 0
    /// The channel-name prefix ("FR", "US") pinned first in Live TV and
    /// ranked first among a game's feeds.
    var region: String = UserDefaults.standard.string(forKey: AppModel.regionKey) ?? AppModel.regionDefault {
        didSet { UserDefaults.standard.set(region, forKey: Self.regionKey) }
    }
    private static let regionKey = "preferences.region"
    /// The owner watches French television first.
    private static let regionDefault = "FR"
    var alert: BridgeError?
    let positions = PlaybackPositions()

    /// Sign in with the Keychain account, else the build's seed account,
    /// else ask.
    ///
    /// Only a rejected login leads to the sign-in form. A network or panel
    /// hiccup with a known account leads to a retry screen instead.
    func start() async {
        phase = .launching
        var lastFailure: BridgeError?
        let saved = CredentialStore.load()
        let seed = SeedAccount.load()
        for credentials in [saved, seed == saved ? nil : seed].compactMap({ $0 }) {
            do {
                try await signIn(credentials)
                return
            } catch let error as BridgeError {
                lastFailure = error
            } catch {
                lastFailure = BridgeError(kind: .storage, message: error.localizedDescription)
            }
        }
        switch lastFailure?.kind {
        case nil, .authFailed, .invalidHost, .invalidRequest, .notSignedIn:
            phase = .signedOut
        // `unsupported` only comes from remuxing; never from signing in.
        case .network, .server, .paymentRequired, .liveOnly, .notFound, .storage, .unsupported:
            phase = .unreachable(lastFailure!)
        }
    }

    /// Throws a `BridgeError` for the sign-in form to show.
    func signIn(_ credentials: Televisor.Credentials) async throws {
        let overview: AccountOverview
        do {
            overview = try await Televisor.open(credentials)
        } catch let error as BridgeError {
            throw error
        } catch {
            throw BridgeError(kind: .storage, message: error.localizedDescription)
        }
        CredentialStore.save(credentials)
        host = credentials.host
        phase = .signedIn(overview)
        await reloadPersonal()
        #if DEBUG
        await playFromLaunchArguments()
        #endif
    }

    #if DEBUG
    /// `-televisorPlay <movie id> -televisorPlayExt <ext>` plays a movie at
    /// launch, and `-televisorTab search` opens on Search, so playback can be tested in a simulator, which has no taps
    /// or remote to reach it. Debug builds only.
    private func playFromLaunchArguments() async {
        let defaults = UserDefaults.standard
        if defaults.string(forKey: "televisorTab") == "search" {
            AppRouter.shared.tab = .search
        }
        guard let id = UInt64(defaults.string(forKey: "televisorPlay") ?? "") else { return }
        await play(PlayRequest(
            kind: .movie, id: id, name: "Launch argument \(id)",
            ext: defaults.string(forKey: "televisorPlayExt"), image: nil
        ))
    }
    #endif

    func signOut() async {
        // Closing only drops the in-memory session; it cannot fail usefully.
        try? await Televisor.close()
        CredentialStore.clear()
        favorites = []
        history = []
        host = nil
        phase = .signedOut
    }

    func refreshStatus() async {
        do {
            phase = .signedIn(try await Televisor.status())
        } catch {
            present(error)
        }
    }

    func refreshLibrary() async {
        do {
            try await Televisor.refresh()
            libraryGeneration += 1
        } catch {
            present(error)
        }
    }

    func reloadPersonal() async {
        do {
            async let favorites = Televisor.favorites()
            async let history = Televisor.history()
            (self.favorites, self.history) = try await (favorites, history)
        } catch {
            present(error)
        }
    }

    func isFavorite(_ kind: MediaKind, _ id: UInt64) -> Bool {
        favorites.contains { $0.kind == kind && $0.id == id }
    }

    func toggleFavorite(_ kind: MediaKind, id: UInt64, name: String, image: String?) async {
        let isFavorite = !isFavorite(kind, id)
        do {
            favorites = try await Televisor.setFavorite(
                kind, id: id, name: name, image: image, isFavorite: isFavorite
            )
        } catch {
            present(error)
        }
    }

    /// Whether the full-screen player is up; drives the cover.
    var isPlayerFullScreen: Bool {
        get { player != nil && presentation == .fullScreen }
        set {
            // The cover was dismissed by the system (swipe down, Menu).
            if !newValue && presentation == .fullScreen {
                stopPlayback()
            }
        }
    }

    /// Resolve and start a stream. `docked` keeps it inline in the
    /// current screen instead of taking it over.
    func play(_ request: PlayRequest, docked: Bool = false) async {
        do {
            let playback = try await Televisor.play(
                request.kind, id: request.id, name: request.name,
                ext: request.ext, image: request.image
            )
            guard let url = URL(string: playback.url) else {
                throw BridgeError(kind: .server, message: "the stream address is not a URL")
            }
            if let seriesId = request.seriesId {
                positions.setLastEpisode(request.id, ofSeries: seriesId)
            }
            let resume = playback.isLive ? nil : positions.mark(request.kind, request.id)?.seconds
            await start(
                PlaybackSession(url: url, request: request, isLive: playback.isLive, startSeconds: resume),
                presentation: docked ? .docked : .fullScreen
            )
            history = try await Televisor.history()
        } catch {
            present(error)
        }
    }

    func stopPlayback() {
        playbackGeneration += 1
        playerOutcomes?.cancel()
        playerOutcomes = nil
        player?.stop()
        player = nil
        presentation = .fullScreen
        if remuxedURL != nil {
            // Frees the panel connection. Closing by URL makes a late
            // close harmless to a newer title; a failed close leaves a
            // server idle until the next title replaces it.
            Task { await closeRemux() }
        }
    }

    /// `session` is always the panel's stream: a remuxed title that fails
    /// falls back to VLC on it.
    private func start(
        _ session: PlaybackSession,
        presentation: PlayerPresentation,
        engine requested: PlaybackEngine? = nil
    ) async {
        stopPlayback()
        let generation = playbackGeneration
        var engine = requested ?? PlaybackEngine.preferred(
            for: session.request, resolvedURL: session.url
        )
        var playing = session
        if engine == .remux {
            let remuxed = await openRemux(session)
            guard generation == playbackGeneration else {
                // The viewer stopped or picked another title meanwhile.
                if let remuxed {
                    try? await Televisor.remuxClose(remuxed.url)
                }
                return
            }
            if let remuxed {
                playing = remuxed
                remuxedURL = remuxed.url
            } else {
                engine = .vlc
            }
        }
        let controller = makeController(engine == .vlc ? session : playing, engine: engine)
        self.presentation = presentation
        player = controller
        watch(controller, session: session, fallsBackToVLC: engine != .vlc)
    }

    /// The title served as HLS by the remuxer, or nil when only VLC can
    /// play it (a codec AVFoundation lacks, no keyframe index).
    private func openRemux(_ session: PlaybackSession) async -> PlaybackSession? {
        guard let stream = try? await Televisor.remuxOpen(session.url) else {
            return nil
        }
        guard let url = URL(string: stream.url) else {
            preconditionFailure("the remuxer always returns a URL")
        }
        return session.remuxed(at: url)
    }

    private func makeController(_ session: PlaybackSession, engine: PlaybackEngine) -> any PlaybackController {
        switch engine {
        case .system, .remux:
            let system = PlayerController(session: session, positions: positions)
            system.onPictureInPicture = { [weak self] isActive in
                self?.pictureInPictureChanged(isActive)
            }
            system.onRestore = { [weak self] in
                self?.presentation = .fullScreen
            }
            return system
        case .vlc:
            return VLCPlayerController(session: session, positions: positions)
        }
    }

    private func watch(_ controller: any PlaybackController, session: PlaybackSession, fallsBackToVLC: Bool) {
        playerOutcomes = Task { [weak self] in
            for await outcome in controller.outcomes() {
                guard let self, self.player === controller else { return }
                switch outcome {
                case .finished:
                    self.stopPlayback()
                case .failed(let reason):
                    // AVFoundation refuses more than VLC does: give the
                    // stream a second chance before telling the viewer.
                    if fallsBackToVLC {
                        await self.closeRemux()
                        await self.start(session, presentation: self.presentation, engine: .vlc)
                    } else {
                        self.stopPlayback()
                        self.alert = BridgeError(kind: .server, message: await self.explainFailure(reason))
                    }
                }
            }
        }
    }

    /// Waits for the remuxer to let go of the panel connection, so VLC
    /// taking over doesn't open a second one the plan would refuse.
    private func closeRemux() async {
        guard let url = remuxedURL else { return }
        remuxedURL = nil
        try? await Televisor.remuxClose(url)
    }

    /// The usual reason a stream won't start is the plan's screen limit:
    /// another device, or a stream that ended badly a minute ago, still
    /// holds the connection. Ask the panel before blaming the stream.
    private func explainFailure(_ reason: String) async -> String {
        guard let overview = try? await Televisor.status() else { return reason }
        phase = .signedIn(overview)
        guard let max = overview.connectionsMax, let active = overview.connectionsActive, active >= max else {
            return reason
        }
        let screens = max == 1 ? "1 screen" : "\(max) screens"
        return "Your plan allows \(screens) at a time, and \(max == 1 ? "it is" : "they are") in use, maybe on another device. Stop playback there, or wait a minute for your provider to free it, then try again."
    }

    /// Full screen hands over to the floating window (and the cover goes
    /// away); a docked player stays docked underneath it. When the window
    /// closes without "back to app", playback ends.
    private func pictureInPictureChanged(_ isActive: Bool) {
        if isActive {
            if presentation == .fullScreen {
                presentation = .pictureInPicture
            }
        } else if presentation == .pictureInPicture {
            stopPlayback()
        }
    }

    func present(_ error: any Error) {
        if let error = error as? BridgeError {
            if error.kind == .notSignedIn {
                phase = .signedOut
                return
            }
            alert = error
        } else {
            alert = BridgeError(kind: .storage, message: error.localizedDescription)
        }
    }
}
