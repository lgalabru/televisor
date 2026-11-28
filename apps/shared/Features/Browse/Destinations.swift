import SwiftUI

/// A shelf's "See All" destination.
struct CategoryRoute: Hashable {
    let library: Library
    let category: Category?
    let title: String
    var order: Order = .panel
}

extension View {
    /// Where cards lead: a movie or show page, or a full category grid.
    func browseDestinations() -> some View {
        navigationDestination(for: Item.self) { item in
            switch item.library {
            case .movies: MovieDetailView(item: item)
            case .series: SeriesDetailView(item: item)
            case .live: preconditionFailure("channels play instead of opening a page")
            }
        }
        .navigationDestination(for: CategoryRoute.self) { route in
            CategoryGridView(route: route)
        }
    }
}

extension CategoryRoute {
    /// The category listing `item`, named as the panel names it; nil when
    /// the item doesn't say, or the category is gone.
    static func parent(of item: Item) async -> CategoryRoute? {
        guard let id = item.categoryId,
              let categories = try? await Televisor.categories(item.library),
              let category = categories.first(where: { $0.id == id })
        else {
            return nil
        }
        return CategoryRoute(
            library: item.library, category: category,
            title: DisplayTitle(category.name, library: item.library).text
        )
    }
}

/// "More in <category>" on a movie or show page, so a title found by
/// search leads to what sits next to it. Shows nothing until the category
/// is known.
struct ParentCategoryLink: View {
    let item: Item
    @State private var route: CategoryRoute?

    var body: some View {
        Group {
            if let route {
                NavigationLink(value: route) {
                    Label("More in \(route.title)", systemImage: "square.grid.2x2")
                }
                .buttonStyle(.glass)
                .padding(.horizontal, Theme.contentInset)
            }
        }
        .task { route = await CategoryRoute.parent(of: item) }
    }
}
