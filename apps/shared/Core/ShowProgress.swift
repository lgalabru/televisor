import Foundation

/// Where a viewer stands in a show, shared by the show's page and the
/// "Your Shows" row on Home so both always agree.
struct UpNext: Hashable {
    let season: Season
    let episode: Episode
    /// The viewer stopped part-way through this episode.
    let isResume: Bool

    /// "S2 E3".
    var code: String { "S\(season.number) E\(episode.number ?? 1)" }
}

@MainActor
enum ShowProgress {
    /// The last episode played if unfinished, else the one after it, else
    /// the very first. A finished last episode of the show stays up next,
    /// to rewatch from the start of it.
    static func upNext(in seasons: [Season], seriesId: UInt64, positions: PlaybackPositions) -> UpNext? {
        let ordered = seasons.flatMap { season in season.episodes.map { (season, $0) } }
        guard let first = ordered.first else { return nil }
        guard let lastId = positions.lastEpisode(ofSeries: seriesId),
              let index = ordered.firstIndex(where: { $0.1.id == lastId })
        else {
            return UpNext(season: first.0, episode: first.1, isResume: false)
        }
        let isUnfinished = positions.mark(.episode, lastId) != nil
        let (season, episode) = isUnfinished || index + 1 == ordered.count ? ordered[index] : ordered[index + 1]
        return UpNext(
            season: season, episode: episode,
            isResume: positions.mark(.episode, episode.id) != nil
        )
    }

    /// Panels prefix episode titles with "S01E01 - ".
    static func cleanTitle(_ episode: Episode) -> String {
        guard let title = episode.title else { return "Episode \(episode.number ?? 0)" }
        let stripped = title.replacing(/^S\d+\s*E\d+\s*[-:]?\s*/.ignoresCase(), with: "")
        return stripped.isEmpty ? title : stripped
    }

    static func playRequest(
        showTitle: String, showImage: String?, seriesId: UInt64, season: Season, episode: Episode
    ) -> PlayRequest {
        PlayRequest(
            kind: .episode, id: episode.id,
            name: "\(showTitle) S\(season.number)E\(episode.number ?? 0)",
            ext: episode.ext,
            image: episode.image ?? showImage,
            subtitle: "\(season.label), Episode \(episode.number ?? 0) · \(cleanTitle(episode))",
            summary: episode.plot,
            seriesId: seriesId
        )
    }
}
