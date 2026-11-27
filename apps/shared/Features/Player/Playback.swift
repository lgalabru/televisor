import Foundation

/// How playback of one stream ended.
enum PlaybackOutcome {
    case finished
    case failed(String)
}

/// One stream being played, by either engine. The app holds exactly one.
@MainActor
protocol PlaybackController: AnyObject {
    var session: PlaybackSession { get }
    /// Yields once, when the stream finishes or fails, then ends.
    func outcomes() -> AsyncStream<PlaybackOutcome>
    func stop()
}

/// Which engine plays a stream. AVFoundation is preferred: it brings the
/// system player, Picture in Picture and AirPlay. VLC plays what
/// AVFoundation cannot open (Matroska, AVI and the like), and takes over
/// when AVFoundation fails on anything else.
enum PlaybackEngine: Equatable {
    case system
    case vlc

    /// Containers AVFoundation has no demuxer for.
    private static let vlcContainers: Set<String> = ["mkv", "avi", "wmv", "flv", "webm", "divx", "ogm", "mpg", "mpeg"]

    static func preferred(for request: PlayRequest) -> PlaybackEngine {
        guard let ext = request.ext?.lowercased().trimmingCharacters(in: CharacterSet(charactersIn: ". ")) else {
            return .system
        }
        return vlcContainers.contains(ext) ? .vlc : .system
    }
}
