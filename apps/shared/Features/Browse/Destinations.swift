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
