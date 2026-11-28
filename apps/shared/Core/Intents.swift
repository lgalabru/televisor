import AppIntents
import Foundation
import SwiftUI

/// Requests from outside the app's own UI (Siri today) for the app to show
/// something. Views watch it and move there.
@MainActor
@Observable
final class AppRouter {
    static let shared = AppRouter()

    enum Destination: Hashable {
        case home, live, movies, series, search, settings
    }

    var tab: Destination = .home
    /// A query to run on the Search tab, consumed when Search shows it.
    var pendingSearch: String?
    /// The Search tab's navigation, so picking a result can be recorded
    /// before it opens (see SearchView): titles and their categories.
    var searchPath = NavigationPath()

    func search(_ query: String) {
        pendingSearch = query
        tab = .search
    }
}

/// "Search Televisor for Dune" from Siri: opens the app on Search with the
/// query already running. Apple's own TV app search only lists apps in
/// Apple's video partner programme, which a personal IPTV catalog can't
/// join; Siri's in-app search is the system search open to everyone.
struct SearchTelevisorIntent: ShowInAppSearchResultsIntent {
    static let title: LocalizedStringResource = "Search Televisor"
    static let description = IntentDescription("Find channels, movies and shows in Televisor.")
    static let searchScopes: [StringSearchScope] = [.general]

    @Parameter(title: "Search")
    var criteria: StringSearchCriteria

    @MainActor
    func perform() async throws -> some IntentResult {
        AppRouter.shared.search(criteria.term)
        return .result()
    }
}

struct TelevisorShortcuts: AppShortcutsProvider {
    static var appShortcuts: [AppShortcut] {
        AppShortcut(
            intent: SearchTelevisorIntent(),
            phrases: [
                "Search \(.applicationName)",
                "Search in \(.applicationName)",
                "Find something on \(.applicationName)",
            ],
            shortTitle: "Search",
            systemImageName: "magnifyingglass"
        )
    }
}
