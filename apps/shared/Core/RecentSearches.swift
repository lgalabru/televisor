import Foundation

/// What the viewer opened from search results, newest first, shown when
/// the search field is empty so a second search is one click.
@MainActor
@Observable
final class RecentSearches {
    static let shared = RecentSearches()

    private(set) var items: [Item]

    /// Two shelves' worth; older picks matter less than a clean screen.
    private static let itemsMax = 24
    private static let defaultsKey = "search.recent"

    private init() {
        items = UserDefaults.standard.data(forKey: Self.defaultsKey)
            .flatMap { try? JSONDecoder().decode([Item].self, from: $0) } ?? []
    }

    func record(_ item: Item) {
        items.removeAll { $0.library == item.library && $0.id == item.id }
        items.insert(item, at: 0)
        if items.count > Self.itemsMax {
            items.removeLast(items.count - Self.itemsMax)
        }
        assert(items.count <= Self.itemsMax)
        persist()
    }

    func clear() {
        items = []
        persist()
    }

    private func persist() {
        guard let data = try? JSONEncoder().encode(items) else {
            preconditionFailure("items always encode")
        }
        UserDefaults.standard.set(data, forKey: Self.defaultsKey)
    }
}
