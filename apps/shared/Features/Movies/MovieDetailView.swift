import SwiftUI

/// A movie's page: full-bleed art, the facts on one line, and Play.
struct MovieDetailView: View {
    let item: Item
    @Environment(AppModel.self) private var model
    @State private var detail: MovieDetail?

    var body: some View {
        let mark = model.positions.mark(.movie, item.id)
        DetailPage(
            backdrop: detail?.backdrop ?? detail?.poster ?? item.image,
            title: item.title.text,
            facts: Format.joined([
                item.title.year ?? detail?.released.map { String($0.prefix(4)) },
                detail?.durationSecs.map(Format.duration),
                detail?.genre,
                Format.rating(detail?.rating ?? item.rating),
            ]),
            summary: detail?.plot,
            credits: [
                detail?.director.map { "Directed by \($0)" },
                detail?.cast.map { "Starring \($0)" },
            ].compactMap { $0 }
        ) {
            Button {
                Task { await model.play(request) }
            } label: {
                if let mark {
                    let left = UInt32(max(mark.duration - mark.seconds, 0))
                    Label("Resume · \(Format.duration(left)) left", systemImage: "play.fill")
                } else {
                    Label("Play", systemImage: "play.fill")
                }
            }
            if mark != nil {
                Button {
                    model.positions.record(.movie, item.id, seconds: 0, duration: 1)
                    Task { await model.play(request) }
                } label: {
                    Label("Start Over", systemImage: "gobackward")
                }
            }
            FavoriteToggle(kind: .movie, id: item.id, name: item.name, image: item.image)
        } below: {
            ParentCategoryLink(item: item)
        }
        .task {
            // The page works from the listing alone; detail only enriches it.
            detail = try? await Televisor.movie(item.id)
        }
    }

    private var request: PlayRequest {
        var request = item.playRequest
        request.subtitle = item.title.year
        request.summary = detail?.plot
        return request
    }
}

/// The heart button on detail pages.
struct FavoriteToggle: View {
    let kind: MediaKind
    let id: UInt64
    let name: String
    let image: String?
    @Environment(AppModel.self) private var model

    var body: some View {
        let isFavorite = model.isFavorite(kind, id)
        let wording = FavoriteWording(kind: kind, isFavorite: isFavorite)
        Button {
            Task { await model.toggleFavorite(kind, id: id, name: name, image: image) }
        } label: {
            Image(systemName: wording.symbol)
                .foregroundStyle(isFavorite ? Theme.accent : Theme.ink)
                .contentTransition(.symbolEffect(.replace))
        }
        .accessibilityLabel(wording.action)
    }
}

/// The shared layout of movie and show pages. Art fills the screen and
/// fades into the canvas at the bottom and the left, where the text sits,
/// so any image keeps the text legible.
struct DetailPage<Actions: View, Below: View>: View {
    let backdrop: String?
    let title: String
    let facts: String
    let summary: String?
    let credits: [String]
    @ViewBuilder let actions: Actions
    @ViewBuilder let below: Below

    var body: some View {
        ScrollView(.vertical) {
            VStack(alignment: .leading, spacing: Theme.shelfSpacing) {
                header.padding(.horizontal, Theme.contentInset)
                below
            }
            .padding(.bottom, 60)
        }
        .scrollClipDisabled()
        .background(alignment: .top) { art }
        .background(Canvas())
    }

    private var header: some View {
        VStack(alignment: .leading, spacing: Theme.itemSpacing * 0.5) {
            Spacer(minLength: 0)
            Text(title)
                .font(.system(size: Theme.displayTitleSize, weight: .bold))
                .lineLimit(2)
                .minimumScaleFactor(0.6)
                .shadow(color: .black.opacity(0.5), radius: 20)
            if !facts.isEmpty {
                Text(facts).font(.callout).foregroundStyle(.secondary)
            }
            if let summary {
                Text(summary)
                    .font(.callout)
                    .lineLimit(4)
                    .frame(maxWidth: Theme.textMaxWidth, alignment: .leading)
                    .foregroundStyle(Theme.ink.opacity(0.9))
            }
            HStack(spacing: Theme.itemSpacing * 0.7) { actions }
                .padding(.top, 16)
                .buttonStyle(.glass)
                .focusGroup()
            ForEach(credits, id: \.self) { line in
                Text(line)
                    .font(.caption)
                    .foregroundStyle(.secondary)
                    .lineLimit(1)
                    .frame(maxWidth: Theme.textMaxWidth, alignment: .leading)
            }
        }
        .frame(maxWidth: .infinity, minHeight: Theme.detailHeaderMinHeight, alignment: .bottomLeading)
    }

    private var art: some View {
        Artwork(url: backdrop, title: "")
            .frame(maxWidth: .infinity)
            .frame(height: Theme.detailHeaderMinHeight * 1.4)
            .clipped()
            .overlay {
                LinearGradient(
                    stops: [.init(color: Theme.canvas.opacity(0.95), location: 0), .init(color: .clear, location: 0.65)],
                    startPoint: .leading, endPoint: .trailing
                )
            }
            .overlay {
                LinearGradient(
                    stops: [.init(color: .clear, location: 0.35), .init(color: Theme.canvas, location: 1)],
                    startPoint: .top, endPoint: .bottom
                )
            }
            .ignoresSafeArea()
    }
}

extension DetailPage where Below == EmptyView {
    init(
        backdrop: String?, title: String, facts: String, summary: String?, credits: [String],
        @ViewBuilder actions: () -> Actions
    ) {
        self.init(
            backdrop: backdrop, title: title, facts: facts, summary: summary, credits: credits,
            actions: actions, below: { EmptyView() }
        )
    }
}
