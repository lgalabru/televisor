import Foundation

// Decodable mirrors of the response types in `crates/ffi/src/protocol.rs`.
// Artwork stays a `String` on the wire: panels send URLs with spaces and
// worse, and one bad URL must not fail a whole page. `URL(string:)` runs
// where the image is drawn.

enum Library: String, Codable, Sendable, CaseIterable {
    case live, movies, series
}

enum MediaKind: String, Codable, Sendable {
    case live, movie, episode
    /// A bookmarked show; never played itself.
    case series
}

enum Order: String, Encodable, Sendable {
    case panel, recent
}

struct AccountOverview: Decodable, Sendable, Equatable {
    let status: String?
    let expiresAt: Int64?
    let isTrial: Bool
    let connectionsActive: Int64?
    let connectionsMax: Int64?
    let timezone: String?
}

struct Category: Decodable, Sendable, Hashable, Identifiable {
    let id: String
    let name: String
}

/// Codable so recent searches can be kept (see RecentSearches).
struct Item: Codable, Sendable, Hashable, Identifiable {
    let library: Library
    let id: UInt64
    let name: String
    let image: String?
    let backdrop: String?
    let rating: Double?
    let genre: String?
    let addedAt: Int64?
    let ext: String?
    let hasArchive: Bool

    var title: DisplayTitle { DisplayTitle(name, library: library) }
}

struct Page: Decodable, Sendable {
    let total: UInt32
    let items: [Item]
}

struct SearchResults: Decodable, Sendable {
    let live: [Item]
    let movies: [Item]
    let series: [Item]

    var isEmpty: Bool { live.isEmpty && movies.isEmpty && series.isEmpty }
}

struct MovieDetail: Decodable, Sendable {
    let plot: String?
    let genre: String?
    let cast: String?
    let director: String?
    let durationSecs: UInt32?
    let rating: Double?
    let released: String?
    let poster: String?
    let backdrop: String?
    let trailer: String?
}

struct SeriesDetail: Decodable, Sendable {
    let plot: String?
    let genre: String?
    let cast: String?
    let rating: Double?
    let poster: String?
    let backdrop: String?
    let seasons: [Season]
}

struct Season: Decodable, Sendable, Hashable, Identifiable {
    let number: UInt32
    let episodes: [Episode]

    var id: UInt32 { number }
    var label: String { number == 0 ? "Specials" : "Season \(number)" }
}

struct Episode: Decodable, Sendable, Hashable, Identifiable {
    let id: UInt64
    let number: UInt32?
    let title: String?
    let plot: String?
    let image: String?
    let durationSecs: UInt32?
    let ext: String?
}

struct Programme: Decodable, Sendable, Hashable {
    let title: String?
    let description: String?
    let start: Int64
    let stop: Int64

    var startDate: Date { Date(timeIntervalSince1970: TimeInterval(start)) }
    var stopDate: Date { Date(timeIntervalSince1970: TimeInterval(stop)) }

    func progress(at date: Date) -> Double {
        let span = Double(stop - start)
        precondition(span > 0, "the bridge drops programmes without a span")
        let elapsed = date.timeIntervalSince1970 - Double(start)
        return min(max(elapsed / span, 0), 1)
    }
}

struct NowNext: Decodable, Sendable {
    let streamId: UInt64
    let now: Programme?
    let next: Programme?
}

struct Playback: Decodable, Sendable {
    let url: String
    let isLive: Bool
}

/// A favorite or a history entry.
struct Entry: Decodable, Sendable, Hashable, Identifiable {
    let kind: MediaKind
    let id: UInt64
    let name: String
    let image: String?
    let ext: String?
    let playedAt: UInt64?
}

/// A panel name split into what a viewer reads. Panels prefix titles with
/// a language or quality tag and bury the year in parentheses:
/// "EN| The Matrix (1999)", "4K-NF - Dune (2021) (US)".
struct DisplayTitle: Hashable, Sendable {
    let text: String
    let year: String?

    init(_ raw: String, library: Library) {
        // Channel prefixes ("US: FOX SPORTS 1") carry the country, which
        // tells two "FOX SPORTS 1" apart. Keep them.
        guard library != .live else {
            text = raw.trimmingCharacters(in: .whitespaces)
            year = nil
            return
        }
        var title = Substring(raw)
        if let tag = title.firstMatch(of: /^[A-Z0-9][A-Z0-9+\-]{1,6}\s*(\||:|\s-)\s*/) {
            title = title[tag.range.upperBound...]
        }
        var found: String?
        if let match = title.firstMatch(of: /\s*[\(\[]((?:19|20)\d{2})[\)\]]/) {
            found = String(match.output.1)
            title = title[..<match.range.lowerBound]
        }
        let cleaned = title.trimmingCharacters(in: .whitespaces)
        text = cleaned.isEmpty ? raw : cleaned
        year = found
    }
}

enum GameState: String, Decodable, Sendable {
    case scheduled, live, finished
}

struct GameTeam: Decodable, Sendable, Hashable {
    let name: String
    let fullName: String
    let abbreviation: String?
    let logo: String?
    let score: String?
}

/// A game from `sports_today`, with the feeds of it in the catalog.
struct Game: Decodable, Sendable, Hashable, Identifiable {
    let id: String
    let league: String
    let leagueName: String
    let start: Int64
    let state: GameState
    let status: String?
    let home: GameTeam
    let away: GameTeam
    let channels: [Item]

    var startDate: Date { Date(timeIntervalSince1970: TimeInterval(start)) }
    var title: String { "\(away.name) at \(home.name)" }

    /// "21 – 17", away first, as the title reads.
    var score: String? {
        guard state != .scheduled, let away = away.score, let home = home.score else { return nil }
        return "\(away) – \(home)"
    }
}
