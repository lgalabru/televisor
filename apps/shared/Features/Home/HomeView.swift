import SwiftUI

/// The first screen: a rotating hero of new movies, then what the viewer
/// was watching, their channels, and what's new.
struct HomeView: View {
    @Environment(AppModel.self) private var model
    @State private var sports = SportsToday()

    var body: some View {
        ScrollView(.vertical) {
            LazyVStack(alignment: .leading, spacing: Theme.shelfSpacing) {
                Hero()
                sportsToday
                yourShows
                continueWatching
                favoriteChannels
                RegionChannelsShelf()
                favoriteMovies
                recentChannels
                CategoryShelf(library: .movies, category: nil, title: "New Movies", order: .recent)
                CategoryShelf(library: .series, category: nil, title: "New Series", order: .recent)
            }
            .padding(.bottom, 60)
        }
        .scrollClipDisabled()
        .background(Canvas())
        .task { await model.reloadPersonal() }
        .task(id: model.region) { await sports.keepCurrent(region: model.region) }
    }

    @ViewBuilder private var sportsToday: some View {
        if !sports.games.isEmpty {
            Shelf(title: "Sports Today") {
                ForEach(sports.games) { game in
                    GameCard(game: game)
                }
            }
        }
    }

    @ViewBuilder private var continueWatching: some View {
        let started = model.history.filter { entry in
            entry.kind != .live && model.positions.mark(entry.kind, entry.id) != nil
        }
        if !started.isEmpty {
            Shelf(title: "Continue Watching") {
                ForEach(started) { entry in
                    Button {
                        Task { await model.play(entry.playRequest) }
                    } label: {
                        WideLabel(
                            title: DisplayTitle(entry.name, library: .movies).text,
                            caption: remaining(entry),
                            image: entry.image,
                            progress: model.positions.mark(entry.kind, entry.id)?.progress
                        )
                    }
                    .cardButtonStyle()
                }
            }
        }
    }

    /// Bookmarked shows, the one watched most recently first. Each card
    /// picks up where the viewer left off.
    @ViewBuilder private var yourShows: some View {
        let shows = model.favorites
            .filter { $0.kind == .series }
            .sorted {
                (model.positions.lastWatched(series: $0.id) ?? .distantPast)
                    > (model.positions.lastWatched(series: $1.id) ?? .distantPast)
            }
        if !shows.isEmpty {
            Shelf(title: "Your Shows") {
                ForEach(shows) { show in
                    ShowCard(show: show)
                }
            }
        }
    }

    @ViewBuilder private var favoriteChannels: some View {
        let channels = model.favorites.filter { $0.kind == .live }
        if !channels.isEmpty {
            Shelf(title: "Your Channels") {
                ForEach(channels) { entry in
                    EntryChannelCard(entry: entry)
                }
            }
        }
    }

    @ViewBuilder private var recentChannels: some View {
        let channels = model.history.filter { $0.kind == .live }
        if !channels.isEmpty {
            Shelf(title: "Recently Watched Channels") {
                ForEach(channels) { entry in
                    EntryChannelCard(entry: entry)
                }
            }
        }
    }

    @ViewBuilder private var favoriteMovies: some View {
        let movies = model.favorites.filter { $0.kind == .movie }
        if !movies.isEmpty {
            Shelf(title: "Your Movies") {
                ForEach(movies) { entry in
                    let item = entry.movieItem
                    NavigationLink(value: item) {
                        PosterLabel(title: item.title.text, caption: item.title.year, image: item.image)
                    }
                    .cardButtonStyle()
                }
            }
        }
    }

    private func remaining(_ entry: Entry) -> String? {
        guard let mark = model.positions.mark(entry.kind, entry.id) else { return nil }
        return "\(Format.duration(UInt32(max(mark.duration - mark.seconds, 0)))) left"
    }
}

private struct EntryChannelCard: View {
    let entry: Entry
    @Environment(AppModel.self) private var model

    var body: some View {
        Button {
            Task { await model.play(entry.playRequest) }
        } label: {
            ChannelLabel(name: entry.name, image: entry.image)
        }
        .cardButtonStyle()
        .contextMenu {
            FavoriteButton(kind: entry.kind, id: entry.id, name: entry.name, image: entry.image)
        }
    }
}

extension Entry {
    var playRequest: PlayRequest {
        PlayRequest(kind: kind, id: id, name: name, ext: ext, image: image)
    }

    var movieItem: Item {
        precondition(kind == .movie, "only movie favorites open a movie page")
        return Item(
            library: .movies, id: id, name: name, image: image, backdrop: nil,
            rating: nil, genre: nil, addedAt: nil, ext: ext, hasArchive: false
        )
    }
}

/// New movies with real backdrops, cross-fading every few seconds until
/// the viewer focuses on one.
private struct Hero: View {
    @Environment(AppModel.self) private var model
    @State private var slides: [Slide] = []
    @State private var index = 0
    @FocusState private var focusedButton: Int?

    private struct Slide: Identifiable {
        let item: Item
        let detail: MovieDetail
        var id: UInt64 { item.id }
    }

    /// Recent movies whose detail is fetched; panels give backdrops to
    /// only some, and 12 usually yields the 5 we show.
    private static let candidatesMax: UInt32 = 12
    private static let slidesMax = 5
    private static let slideDuration: Duration = .seconds(9)

    var body: some View {
        Group {
            if slides.isEmpty {
                Color.clear.frame(height: 1)
            } else {
                let slide = slides[index]
                caption(slide)
                    .padding(.horizontal, Theme.contentInset)
                    .padding(.bottom, 20)
                    .frame(maxWidth: .infinity, minHeight: Theme.heroMinHeight, alignment: .bottomLeading)
                    // The art is a background, so its bleed past the margins
                    // never widens the text's layout.
                    .background(alignment: .bottom) { art(slide) }
            }
        }
        .task { await load() }
        .task(id: slides.count) { await rotate() }
    }

    private func art(_ slide: Slide) -> some View {
        Artwork(url: slide.detail.backdrop, title: "")
            .frame(maxWidth: .infinity)
            .frame(height: Theme.heroArtHeight)
            .clipped()
            .overlay {
                LinearGradient(
                    stops: [.init(color: Theme.canvas.opacity(0.9), location: 0), .init(color: .clear, location: 0.6)],
                    startPoint: .leading, endPoint: .trailing
                )
            }
            .overlay {
                LinearGradient(
                    stops: [.init(color: .clear, location: 0.4), .init(color: Theme.canvas, location: 1)],
                    startPoint: .top, endPoint: .bottom
                )
            }
            .padding(.horizontal, -Theme.bleed)
            .ignoresSafeArea(edges: .top)
            .id(slide.id)
            .transition(.opacity)
    }

    private func caption(_ slide: Slide) -> some View {
        VStack(alignment: .leading, spacing: 20) {
            Text("NEW")
                .font(.caption).fontWeight(.heavy).kerning(2)
                .foregroundStyle(Theme.accent)
            Text(slide.item.title.text)
                .font(.system(size: Theme.heroTitleSize, weight: .bold))
                .lineLimit(2)
                .minimumScaleFactor(0.6)
                .frame(maxWidth: Theme.textMaxWidth, alignment: .leading)
            Text(Format.joined([
                slide.item.title.year,
                slide.detail.durationSecs.map(Format.duration),
                slide.detail.genre,
                Format.rating(slide.detail.rating),
            ]))
            .font(.callout)
            .foregroundStyle(.secondary)
            if let plot = slide.detail.plot {
                Text(plot).font(.callout).lineLimit(2).frame(maxWidth: Theme.textMaxWidth, alignment: .leading)
            }
            HStack(spacing: 30) {
                Button {
                    var request = slide.item.playRequest
                    request.summary = slide.detail.plot
                    Task { await model.play(request) }
                } label: {
                    Label("Play", systemImage: "play.fill")
                }
                .focused($focusedButton, equals: 0)
                NavigationLink(value: slide.item) {
                    Label("Details", systemImage: "info.circle")
                }
                .focused($focusedButton, equals: 1)
            }
            .padding(.top, 10)
            .buttonStyle(.glass)
            .focusGroup()
        }
        .id(slide.id)
        .transition(.opacity)
    }

    private func load() async {
        guard slides.isEmpty,
              let page = try? await Televisor.browse(.movies, order: .recent, limit: Self.candidatesMax)
        else { return }
        let found = await withTaskGroup(of: Slide?.self) { group in
            for item in page.items {
                group.addTask {
                    guard let detail = try? await Televisor.movie(item.id), detail.backdrop != nil else {
                        return nil
                    }
                    return Slide(item: item, detail: detail)
                }
            }
            var slides: [Slide] = []
            for await slide in group {
                if let slide { slides.append(slide) }
            }
            return slides
        }
        // Keep the panel's newest-first order, not completion order.
        let order = page.items.map(\.id)
        slides = Array(
            found.sorted { order.firstIndex(of: $0.id)! < order.firstIndex(of: $1.id)! }
                .prefix(Self.slidesMax)
        )
    }

    private func rotate() async {
        guard slides.count > 1 else { return }
        while !Task.isCancelled {
            try? await Task.sleep(for: Self.slideDuration)
            // Never swap the movie out from under the viewer's focus.
            if focusedButton == nil {
                withAnimation(.easeInOut(duration: 0.8)) {
                    index = (index + 1) % slides.count
                }
            }
        }
    }
}

/// The preferred region's main channels ("FR| GÉNÉRAL HD/4K"), so the
/// channels the viewer grew up with are on the first screen.
private struct RegionChannelsShelf: View {
    @Environment(AppModel.self) private var model
    @State private var category: Category?

    var body: some View {
        Group {
            if let category {
                let region = Region(code: model.region)
                CategoryShelf(
                    library: .live, category: category,
                    title: "\(region.flag)  \(region.name) · \(RegionGroups.shortName(category).capitalized)"
                )
                .id(category.id)
            }
        }
        .task(id: model.region) {
            guard let categories = try? await Televisor.categories(.live) else { return }
            let mine = categories.filter { Region.code(ofCategory: $0.name) == model.region }
            // Panels call the main bouquet "GENERAL", "GÉNÉRAL" or "GENERALE".
            category = mine.first { $0.name.uppercased().contains("GÉNÉRAL") || $0.name.uppercased().contains("GENERAL") }
                ?? mine.first
        }
    }
}

/// A bookmarked show on Home: the next episode's still, where the viewer
/// stands, and one tap to carry on. Long-press opens the show or removes
/// the bookmark.
private struct ShowCard: View {
    let show: Entry
    @Environment(AppModel.self) private var model
    @State private var detail: SeriesDetail?
    @State private var isOpeningShow = false

    var body: some View {
        let next = detail.flatMap {
            ShowProgress.upNext(in: $0.seasons, seriesId: show.id, positions: model.positions)
        }
        Button {
            if let next { play(next) } else { isOpeningShow = true }
        } label: {
            WideLabel(
                title: DisplayTitle(show.name, library: .series).text,
                caption: caption(next),
                image: next?.episode.image ?? detail?.backdrop ?? show.image,
                progress: next.flatMap { model.positions.mark(.episode, $0.episode.id)?.progress }
            )
        }
        .cardButtonStyle()
        .contextMenu {
            Button {
                isOpeningShow = true
            } label: {
                Label("Go to Show", systemImage: "tv")
            }
            FavoriteButton(kind: .series, id: show.id, name: show.name, image: show.image)
        }
        .navigationDestination(isPresented: $isOpeningShow) {
            SeriesDetailView(item: item)
        }
        // A failed lookup leaves the card opening the show page instead.
        .task { detail = try? await Televisor.series(show.id) }
    }

    private var item: Item {
        Item(
            library: .series, id: show.id, name: show.name, image: show.image, backdrop: nil,
            rating: nil, genre: nil, addedAt: nil, ext: nil, hasArchive: false
        )
    }

    private func caption(_ next: UpNext?) -> String {
        guard let next else { return detail == nil ? "Loading…" : "No episodes yet" }
        guard next.isResume, let mark = model.positions.mark(.episode, next.episode.id) else {
            let verb = model.positions.lastEpisode(ofSeries: show.id) == nil ? "Start" : "Next"
            return "\(verb) · \(next.code)"
        }
        let left = Format.duration(UInt32(max(mark.duration - mark.seconds, 0)))
        return "Continue · \(next.code) · \(left) left"
    }

    private func play(_ next: UpNext) {
        let request = ShowProgress.playRequest(
            showTitle: DisplayTitle(show.name, library: .series).text, showImage: show.image,
            seriesId: show.id, season: next.season, episode: next.episode
        )
        Task { await model.play(request) }
    }
}
