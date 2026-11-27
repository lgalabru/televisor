import Foundation

/// Where each movie and episode was left, for Resume and the progress bars
/// on Continue Watching. Kept in preferences: small, and tvOS keeps them.
@MainActor
@Observable
final class PlaybackPositions {
    struct Mark: Codable, Sendable, Equatable {
        let seconds: Double
        let duration: Double
        let updatedAt: Date

        var progress: Double { min(max(seconds / duration, 0), 1) }
    }

    /// Titles remembered. A preference value is capped near 1 MB on tvOS;
    /// 300 marks are about 30 KB.
    private static let marksMax = 300
    /// Under this, a title counts as not started.
    private static let startedSecondsMin: Double = 30
    /// Past this share, a title counts as watched and starts over next time.
    private static let finishedProgressMin = 0.95

    private static let defaultsKey = "playback.marks"
    private static let lastEpisodeKey = "playback.lastEpisode"
    private static let lastWatchedKey = "playback.lastWatched"

    private(set) var marks: [String: Mark]
    private(set) var lastEpisodes: [String: UInt64]
    /// When each show was last played, as seconds since 1970, to order
    /// "Your Shows" by what the viewer is into right now.
    private(set) var lastWatched: [String: Double]

    init() {
        let defaults = UserDefaults.standard
        marks = defaults.data(forKey: Self.defaultsKey)
            .flatMap { try? JSONDecoder().decode([String: Mark].self, from: $0) } ?? [:]
        lastEpisodes = (defaults.dictionary(forKey: Self.lastEpisodeKey) as? [String: UInt64]) ?? [:]
        lastWatched = (defaults.dictionary(forKey: Self.lastWatchedKey) as? [String: Double]) ?? [:]
    }

    static func key(_ kind: MediaKind, _ id: UInt64) -> String {
        "\(kind.rawValue)-\(id)"
    }

    func mark(_ kind: MediaKind, _ id: UInt64) -> Mark? {
        marks[Self.key(kind, id)]
    }

    func record(_ kind: MediaKind, _ id: UInt64, seconds: Double, duration: Double) {
        precondition(kind != .live, "live streams have no position")
        guard duration.isFinite, duration > 0, seconds.isFinite else { return }
        let key = Self.key(kind, id)
        if seconds < Self.startedSecondsMin || seconds / duration > Self.finishedProgressMin {
            marks[key] = nil
        } else {
            marks[key] = Mark(seconds: seconds, duration: duration, updatedAt: .now)
        }
        trim()
        persist()
    }

    func lastEpisode(ofSeries seriesId: UInt64) -> UInt64? {
        lastEpisodes[String(seriesId)]
    }

    func lastWatched(series seriesId: UInt64) -> Date? {
        lastWatched[String(seriesId)].map(Date.init(timeIntervalSince1970:))
    }

    func setLastEpisode(_ episodeId: UInt64, ofSeries seriesId: UInt64) {
        let key = String(seriesId)
        lastEpisodes[key] = episodeId
        lastWatched[key] = Date.now.timeIntervalSince1970
        if lastEpisodes.count > Self.marksMax,
           let stalest = lastWatched.min(by: { $0.value < $1.value })?.key {
            lastEpisodes[stalest] = nil
            lastWatched[stalest] = nil
        }
        UserDefaults.standard.set(lastEpisodes, forKey: Self.lastEpisodeKey)
        UserDefaults.standard.set(lastWatched, forKey: Self.lastWatchedKey)
    }

    private func trim() {
        guard marks.count > Self.marksMax else { return }
        let oldest = marks.sorted { $0.value.updatedAt < $1.value.updatedAt }
        for (key, _) in oldest.prefix(marks.count - Self.marksMax) {
            marks[key] = nil
        }
        assert(marks.count == Self.marksMax)
    }

    private func persist() {
        guard let data = try? JSONEncoder().encode(marks) else {
            preconditionFailure("marks always encode")
        }
        UserDefaults.standard.set(data, forKey: Self.defaultsKey)
    }
}
