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
/// system player, Picture in Picture and AirPlay. Matroska goes to it
/// through the remuxer, which serves the title as HLS on loopback. VLC
/// plays what neither can (AVI and the like, Matroska with DTS or without
/// a keyframe index), and takes over when AVFoundation fails.
enum PlaybackEngine: Equatable {
    case system
    /// The system player, on the title remuxed to HLS.
    case remux
    case vlc

    /// Containers AVFoundation has no demuxer for.
    private static let vlcContainers: Set<String> = ["avi", "wmv", "flv", "webm", "divx", "ogm", "mpg", "mpeg"]

    static func preferred(for request: PlayRequest, resolvedURL: URL) -> PlaybackEngine {
        // Favorites and history written by older builds may not carry the
        // catalog extension. The bridge repairs their playback URL from the
        // catalog, so use that URL as the source of truth when needed.
        let supplied = request.ext?.trimmingCharacters(in: CharacterSet(charactersIn: ". "))
        let ext = (supplied?.isEmpty == false ? supplied : nil) ?? resolvedURL.pathExtension
        guard !ext.isEmpty else {
            return .system
        }
        let normalized = ext.lowercased()
        if normalized == "mkv" {
            return .remux
        }
        return vlcContainers.contains(normalized) ? .vlc : .system
    }
}
