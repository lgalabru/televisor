import SwiftUI

struct RootView: View {
    @Environment(AppModel.self) private var model

    var body: some View {
        @Bindable var model = model
        ZStack {
            Canvas()
            switch model.phase {
            case .launching:
                LaunchView()
            case .signedOut:
                SignInView()
            case .unreachable(let failure):
                MessageView(
                    title: failure.title,
                    detail: failure.explanation,
                    systemImage: "wifi.exclamationmark"
                ) {
                    Task { await model.start() }
                }
            case .signedIn:
                MainTabs()
                    .id(model.libraryGeneration)
            }
        }
        .animation(.easeInOut(duration: 0.4), value: model.phase)
        .task { await model.start() }
        .fullScreenCover(isPresented: $model.isPlayerFullScreen) {
            if let player = model.player {
                PlayerView(controller: player)
            }
        }
        .alert(
            model.alert?.title ?? "",
            isPresented: Binding(
                get: { model.alert != nil },
                set: { if !$0 { model.alert = nil } }
            ),
            presenting: model.alert
        ) { _ in
            Button("OK", role: .cancel) {}
        } message: { error in
            Text(error.explanation)
        }
    }
}

private struct MainTabs: View {
    @Bindable private var router = AppRouter.shared

    var body: some View {
        TabView(selection: $router.tab) {
            Tab("Home", systemImage: "house", value: AppRouter.Destination.home) {
                NavigationStack { HomeView().browseDestinations() }
            }
            Tab("Live TV", systemImage: "dot.radiowaves.left.and.right", value: AppRouter.Destination.live) {
                NavigationStack { LiveView() }
            }
            Tab("Movies", systemImage: "film", value: AppRouter.Destination.movies) {
                NavigationStack { LibraryView(library: .movies).browseDestinations() }
            }
            Tab("Series", systemImage: "tv", value: AppRouter.Destination.series) {
                NavigationStack { LibraryView(library: .series).browseDestinations() }
            }
            Tab(value: AppRouter.Destination.search, role: .search) {
                NavigationStack(path: $router.searchPath) { SearchView().browseDestinations() }
            }
            Tab("Settings", systemImage: "gearshape", value: AppRouter.Destination.settings) {
                SettingsView()
            }
        }
    }
}
