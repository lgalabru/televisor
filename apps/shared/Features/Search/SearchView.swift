import SwiftUI

/// One search across channels, movies and shows, as you type. With the
/// field empty, it shows what the viewer opened from past searches.
struct SearchView: View {
    @Environment(AppModel.self) private var model
    private let recent = RecentSearches.shared
    @State private var query = ""
    @State private var results: SearchResults?
    @State private var isSearching = false

    /// Typing on the on-screen keyboard is slow; this waits out a burst of
    /// letters without feeling laggy.
    private static let debounce: Duration = .milliseconds(400)
    private static let resultsPerLibrary: UInt32 = 30

    var body: some View {
        ScrollView(.vertical) {
            LazyVStack(alignment: .leading, spacing: Theme.shelfSpacing) {
                if let results {
                    if results.isEmpty {
                        MessageView(
                            title: "No Results",
                            detail: "Nothing matches “\(query)”.",
                            systemImage: "magnifyingglass"
                        )
                        .padding(.top, 80)
                    }
                    section("Channels", results.live)
                    section("Movies", results.movies)
                    section("Series", results.series)
                } else if isSearching {
                    LoadingView(label: "Searching your library").padding(.top, 120)
                } else if !recent.items.isEmpty {
                    recentSearches
                }
            }
            .padding(.vertical, 40)
        }
        .scrollClipDisabled()
        .searchable(text: $query, prompt: "Channels, Movies and Shows")
        .task(id: query) { await search() }
        // A query from Siri lands here (see AppRouter).
        .onAppear(perform: takePendingSearch)
        .onChange(of: AppRouter.shared.pendingSearch) { takePendingSearch() }
    }

    @ViewBuilder
    private func section(_ title: String, _ items: [Item]) -> some View {
        if !items.isEmpty {
            Shelf(title: title) {
                ForEach(items) { item in
                    resultCard(item)
                }
            }
        }
    }

    /// Past picks, grouped like results, then a way to forget them.
    @ViewBuilder private var recentSearches: some View {
        Text("Recently Searched")
            .font(.title2).fontWeight(.bold)
            .padding(.horizontal, Theme.contentInset)
        section("Channels", recent.items.filter { $0.library == .live })
        section("Movies", recent.items.filter { $0.library == .movies })
        section("Series", recent.items.filter { $0.library == .series })
        Button(role: .destructive) {
            withAnimation { recent.clear() }
        } label: {
            Label("Clear Search History", systemImage: "trash")
        }
        .padding(.horizontal, Theme.contentInset)
        .focusGroup()
    }

    /// A result that is remembered when opened: channels play, movies and
    /// shows open their page.
    private func resultCard(_ item: Item) -> some View {
        Button {
            recent.record(item)
            switch item.library {
            case .live:
                Task { await model.play(item.playRequest) }
            case .movies, .series:
                AppRouter.shared.searchPath.append(item)
            }
        } label: {
            switch item.library {
            case .live:
                ChannelLabel(name: item.title.text, image: item.image)
            case .movies:
                PosterLabel(
                    title: item.title.text, caption: item.caption, image: item.image,
                    progress: model.positions.mark(.movie, item.id)?.progress
                )
            case .series:
                PosterLabel(title: item.title.text, caption: item.caption, image: item.image)
            }
        }
        .cardButtonStyle()
        .contextMenu {
            switch item.library {
            case .live: FavoriteButton(kind: .live, id: item.id, name: item.name, image: item.image)
            case .movies: FavoriteButton(kind: .movie, id: item.id, name: item.name, image: item.image)
            case .series: FavoriteButton(kind: .series, id: item.id, name: item.name, image: item.image)
            }
        }
    }

    private func takePendingSearch() {
        guard let pending = AppRouter.shared.pendingSearch else { return }
        AppRouter.shared.pendingSearch = nil
        query = pending
    }

    private func search() async {
        let trimmed = query.trimmingCharacters(in: .whitespaces)
        guard trimmed.count >= 2 else {
            results = nil
            return
        }
        try? await Task.sleep(for: Self.debounce)
        guard !Task.isCancelled else { return }
        isSearching = true
        defer { isSearching = false }
        let found = try? await Televisor.search(trimmed, limit: Self.resultsPerLibrary)
        guard !Task.isCancelled else { return }
        results = found ?? SearchResults(live: [], movies: [], series: [])
    }
}
