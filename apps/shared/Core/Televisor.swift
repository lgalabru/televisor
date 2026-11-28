import Foundation

/// Typed calls into the Rust core. Every method is one bridge round trip.
enum Televisor {
    struct Credentials: Codable, Sendable, Equatable {
        var accountName: String
        var host: String
        var username: String
        var password: String
    }

    static func open(_ credentials: Credentials) async throws -> AccountOverview {
        struct Params: Encodable, Sendable {
            let accountName, host, username, password, cacheDir, statePath: String
        }
        let storage = try Storage.directory()
        return try await Bridge.call(.open, Params(
            accountName: credentials.accountName,
            host: credentials.host,
            username: credentials.username,
            password: credentials.password,
            cacheDir: storage.appending(path: "catalog").path(percentEncoded: false),
            statePath: storage.appending(path: "state.toml").path(percentEncoded: false)
        ))
    }

    static func close() async throws {
        let _: Bridge.Empty = try await Bridge.call(.close)
    }

    static func status() async throws -> AccountOverview {
        try await Bridge.call(.status)
    }

    static func refresh() async throws {
        let _: Bridge.Empty = try await Bridge.call(.refresh)
    }

    static func categories(_ library: Library) async throws -> [Category] {
        struct Params: Encodable, Sendable { let library: Library }
        return try await Bridge.call(.categories, Params(library: library))
    }

    static func browse(
        _ library: Library,
        category: String? = nil,
        query: String = "",
        order: Order = .panel,
        offset: UInt32 = 0,
        limit: UInt32
    ) async throws -> Page {
        struct Params: Encodable, Sendable {
            let library: Library
            let categoryId: String?
            let query: String
            let order: Order
            let offset: UInt32
            let limit: UInt32
        }
        return try await Bridge.call(.browse, Params(
            library: library, categoryId: category, query: query,
            order: order, offset: offset, limit: limit
        ))
    }

    static func search(_ query: String, limit: UInt32) async throws -> SearchResults {
        struct Params: Encodable, Sendable { let query: String; let limit: UInt32 }
        return try await Bridge.call(.search, Params(query: query, limit: limit))
    }

    static func movie(_ id: UInt64) async throws -> MovieDetail {
        struct Params: Encodable, Sendable { let id: UInt64 }
        return try await Bridge.call(.movie, Params(id: id))
    }

    static func series(_ id: UInt64) async throws -> SeriesDetail {
        struct Params: Encodable, Sendable { let id: UInt64 }
        return try await Bridge.call(.series, Params(id: id))
    }

    /// Mirrors `EPG_BATCH_MAX` in `crates/ffi/src/protocol.rs`.
    static let epgBatchMax = 50

    static func nowNext(_ streamIds: [UInt64]) async throws -> [NowNext] {
        struct Params: Encodable, Sendable { let streamIds: [UInt64] }
        precondition(streamIds.count <= epgBatchMax, "callers chunk EPG batches")
        return try await Bridge.call(.epgNow, Params(streamIds: streamIds))
    }

    static func play(
        _ kind: MediaKind, id: UInt64, name: String, ext: String?, image: String?
    ) async throws -> Playback {
        struct Params: Encodable, Sendable {
            let kind: MediaKind
            let id: UInt64
            let name: String
            let ext: String?
            let image: String?
        }
        return try await Bridge.call(.play, Params(
            kind: kind, id: id, name: name, ext: ext, image: image
        ))
    }

    /// Serve a Matroska title (`url` from `play`) as HLS on loopback, for
    /// AVPlayer. Throws `.unsupported` when only VLC can play it.
    static func remuxOpen(_ url: URL) async throws -> RemuxedStream {
        struct Params: Encodable, Sendable { let url: String }
        return try await Bridge.call(.remuxOpen, Params(url: url.absoluteString))
    }

    /// Stop serving the remuxed title at `url`; a no-op if a newer title
    /// replaced it.
    static func remuxClose(_ url: URL) async throws {
        struct Params: Encodable, Sendable { let url: String }
        let _: Bridge.Empty = try await Bridge.call(.remuxClose, Params(url: url.absoluteString))
    }

    /// Big games live now or soon, each with the channels carrying it.
    /// Channels whose name starts with `region` (like "FR") come first.
    static func sportsToday(region: String?) async throws -> [Game] {
        struct Params: Encodable, Sendable { let region: String? }
        return try await Bridge.call(.sportsToday, Params(region: region))
    }

    static func history() async throws -> [Entry] {
        try await Bridge.call(.history)
    }

    static func favorites() async throws -> [Entry] {
        try await Bridge.call(.favorites)
    }

    static func setFavorite(
        _ kind: MediaKind, id: UInt64, name: String, image: String?, isFavorite: Bool
    ) async throws -> [Entry] {
        struct Params: Encodable, Sendable {
            let kind: MediaKind
            let id: UInt64
            let name: String
            let image: String?
            let isFavorite: Bool
        }
        return try await Bridge.call(.setFavorite, Params(
            kind: kind, id: id, name: name, image: image, isFavorite: isFavorite
        ))
    }
}

/// Where the core keeps its files. tvOS gives apps no durable local
/// storage beyond small preferences: Caches is the one writable place, and
/// the system may purge it under pressure. Catalogs refetch on their own;
/// favorites and history could be lost, which is acceptable for a cache of
/// what the panel already knows.
enum Storage {
    static func directory() throws -> URL {
        let caches = try FileManager.default.url(
            for: .cachesDirectory, in: .userDomainMask, appropriateFor: nil, create: true
        )
        let directory = caches.appending(path: "televisor", directoryHint: .isDirectory)
        try FileManager.default.createDirectory(at: directory, withIntermediateDirectories: true)
        return directory
    }
}
