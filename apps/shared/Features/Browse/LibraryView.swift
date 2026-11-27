import SwiftUI

/// Movies or Series: a "Recently Added" row, then one row per panel
/// category. Rows load as they scroll into view, so a panel with 400
/// categories costs a request per row actually seen.
struct LibraryView: View {
    let library: Library
    @Environment(AppModel.self) private var model
    @State private var categories: [Category]?
    @State private var failure: BridgeError?

    var body: some View {
        Group {
            if let categories {
                ScrollView(.vertical) {
                    LazyVStack(alignment: .leading, spacing: Theme.shelfSpacing) {
                        CategoryShelf(
                            library: library, category: nil,
                            title: "Recently Added", order: .recent
                        )
                        ForEach(categories) { category in
                            CategoryShelf(
                                library: library, category: category,
                                title: DisplayTitle(category.name, library: library).text
                            )
                        }
                    }
                    .padding(.vertical, 40)
                }
                .scrollClipDisabled()
                .background(Canvas())
            } else if let failure {
                MessageView(title: failure.title, detail: failure.explanation, retry: reload)
            } else {
                LoadingView(label: library == .movies ? "Loading movies" : "Loading series")
            }
        }
        .task { await load() }
    }

    private func reload() {
        failure = nil
        Task { await load() }
    }

    private func load() async {
        guard categories == nil else { return }
        do {
            categories = try await Televisor.categories(library)
        } catch let error as BridgeError {
            failure = error
        } catch {
            model.present(error)
        }
    }
}

/// One row of a category. Hides itself when the category turns out empty
/// (panels list plenty of those, and adult ones are filtered out).
struct CategoryShelf: View {
    let library: Library
    let category: Category?
    let title: String
    var order: Order = .panel

    /// Enough to fill the row twice over at 7 posters per screen width.
    private static let rowItemsMax: UInt32 = 20

    @State private var page: Page?

    var body: some View {
        Group {
            if let page, !page.items.isEmpty {
                Shelf(title: title) {
                    ForEach(page.items) { item in
                        ItemCard(item: item)
                    }
                    if page.total > UInt32(page.items.count) {
                        NavigationLink(value: CategoryRoute(
                            library: library, category: category, title: title, order: order
                        )) {
                            SeeAllLabel(library: library)
                        }
                        .cardButtonStyle()
                    }
                }
            } else if page == nil {
                // Holds the row's height so the stack doesn't jump as rows land.
                Shelf(title: title) {
                    ForEach(0..<6, id: \.self) { _ in
                        RoundedRectangle(cornerRadius: Theme.cornerRadius)
                            .fill(Theme.raised)
                            .frame(
                                width: library == .live ? Theme.channelWidth : Theme.posterWidth,
                                height: library == .live ? Theme.channelHeight : Theme.posterHeight
                            )
                    }
                }
                .redacted(reason: .placeholder)
            }
        }
        .task {
            guard page == nil else { return }
            // A failed row stays hidden; the rest of the screen still works.
            page = (try? await Televisor.browse(
                library, category: category?.id, order: order, limit: Self.rowItemsMax
            )) ?? Page(total: 0, items: [])
        }
    }
}

/// Every item of one category, as a grid that pages in as you scroll.
struct CategoryGridView: View {
    let route: CategoryRoute
    @State private var items: [Item] = []
    @State private var total: UInt32?
    @State private var isLoading = false

    /// Six rows of posters per request.
    private static let pageItems: UInt32 = 48

    var body: some View {
        let isLive = route.library == .live
        let width = isLive ? Theme.channelWidth : Theme.posterWidth
        ScrollView(.vertical) {
            VStack(alignment: .leading, spacing: 40) {
                Text(route.title).font(.title2).fontWeight(.bold)
                    .padding(.horizontal, Theme.contentInset)
                LazyVGrid(
                    columns: [GridItem(.adaptive(minimum: width, maximum: width), spacing: Theme.itemSpacing)],
                    alignment: .leading,
                    spacing: Theme.shelfSpacing
                ) {
                    ForEach(items) { item in
                        ItemCard(item: item)
                            .onAppear {
                                if item.id == items.last?.id {
                                    Task { await loadMore() }
                                }
                            }
                    }
                }
                .padding(.horizontal, Theme.contentInset)
                if isLoading {
                    ProgressView().frame(maxWidth: .infinity)
                }
            }
            .padding(.vertical, 40)
        }
        .scrollClipDisabled()
        .background(Canvas())
        .navigationTitle(route.title)
        .task { await loadMore() }
    }

    private func loadMore() async {
        guard !isLoading else { return }
        if let total, UInt32(items.count) >= total { return }
        isLoading = true
        defer { isLoading = false }
        guard let page = try? await Televisor.browse(
            route.library, category: route.category?.id, order: route.order,
            offset: UInt32(items.count), limit: Self.pageItems
        ) else {
            // Stop paging; what loaded stays usable.
            total = UInt32(items.count)
            return
        }
        items.append(contentsOf: page.items)
        total = page.total
    }
}
