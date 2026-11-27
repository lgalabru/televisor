import SwiftUI

/// A show's page: art and facts, a Play button that knows where the viewer
/// left off, then seasons and their episodes.
struct SeriesDetailView: View {
    let item: Item
    @Environment(AppModel.self) private var model
    @State private var detail: SeriesDetail?
    @State private var failure: BridgeError?
    @State private var seasonNumber: UInt32?

    var body: some View {
        DetailPage(
            backdrop: detail?.backdrop ?? item.backdrop ?? detail?.poster ?? item.image,
            title: item.title.text,
            facts: Format.joined([
                item.title.year,
                detail.map { "\($0.seasons.count) \($0.seasons.count == 1 ? "Season" : "Seasons")" },
                detail?.genre ?? item.genre,
                Format.rating(detail?.rating ?? item.rating),
            ]),
            summary: detail?.plot,
            credits: [detail?.cast.map { "Starring \($0)" }].compactMap { $0 }
        ) {
            if let next = upNext {
                Button {
                    play(next.episode, in: next.season)
                } label: {
                    Label("\(next.isResume ? "Resume" : "Play") \(next.code)", systemImage: "play.fill")
                }
            } else if failure == nil {
                ProgressView()
            }
            FavoriteToggle(kind: .series, id: item.id, name: item.name, image: item.image)
        } below: {
            if let detail, !detail.seasons.isEmpty {
                seasons(detail.seasons)
            } else if let failure {
                Text(failure.explanation)
                    .foregroundStyle(.secondary)
                    .padding(.horizontal, Theme.contentInset)
            }
        }
        .task { await load() }
    }

    private func seasons(_ seasons: [Season]) -> some View {
        let current = seasons.first { $0.number == seasonNumber } ?? seasons[0]
        return VStack(alignment: .leading, spacing: Theme.itemSpacing * 0.6) {
            if seasons.count > 1 {
                ScrollView(.horizontal) {
                    HStack(spacing: Theme.itemSpacing * 0.45) {
                        ForEach(seasons) { season in
                            Button(season.label) { seasonNumber = season.number }
                                .buttonStyle(SeasonButtonStyle(isSelected: season.number == current.number))
                        }
                    }
                    .padding(.vertical, 12)
                    .padding(.horizontal, Theme.contentInset)
                }
                .scrollClipDisabled()
                .focusGroup()
            }
            Shelf(title: "\(current.label) · \(current.episodes.count) Episodes") {
                ForEach(current.episodes) { episode in
                    Button {
                        play(episode, in: current)
                    } label: {
                        WideLabel(
                            title: "\(episode.number.map { "\($0). " } ?? "")\(ShowProgress.cleanTitle(episode))",
                            caption: Format.joined([episode.durationSecs.map(Format.duration), episode.plot]),
                            image: episode.image ?? detail?.backdrop ?? item.image,
                            progress: model.positions.mark(.episode, episode.id)?.progress
                        )
                    }
                    .cardButtonStyle()
                }
            }
            .id(current.number)
        }
    }

    private var upNext: UpNext? {
        detail.flatMap {
            ShowProgress.upNext(in: $0.seasons, seriesId: item.id, positions: model.positions)
        }
    }

    private func play(_ episode: Episode, in season: Season) {
        let request = ShowProgress.playRequest(
            showTitle: item.title.text, showImage: item.image, seriesId: item.id,
            season: season, episode: episode
        )
        Task { await model.play(request) }
    }

    private func load() async {
        guard detail == nil else { return }
        do {
            let detail = try await Televisor.series(item.id)
            self.detail = detail
            seasonNumber = upNext?.season.number
        } catch let error as BridgeError {
            failure = error
        } catch {
            model.present(error)
        }
    }
}

private struct SeasonButtonStyle: ButtonStyle {
    let isSelected: Bool

    func makeBody(configuration: Configuration) -> some View {
        Pill(configuration: configuration, isSelected: isSelected)
    }

    private struct Pill: View {
        let configuration: Configuration
        let isSelected: Bool
        @Environment(\.isFocused) private var isFocused

        var body: some View {
            configuration.label
                .font(.callout)
                .fontWeight(isSelected ? .semibold : .regular)
                .padding(.horizontal, Theme.itemSpacing * 0.75)
                .padding(.vertical, Theme.itemSpacing * 0.35)
                .foregroundStyle(isFocused ? Color.black : Theme.ink)
                .background(
                    Capsule().fill(isFocused ? Theme.ink : isSelected ? Color.white.opacity(0.2) : Color.white.opacity(0.06))
                )
                .scaleEffect(isFocused ? 1.06 : 1)
                .animation(.easeOut(duration: 0.15), value: isFocused)
        }
    }
}
