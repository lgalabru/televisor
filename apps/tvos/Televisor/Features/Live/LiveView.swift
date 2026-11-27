import SwiftUI

/// The channel guide: sources on the left, channels with what is on now
/// and next on the right. Moving focus down the left column previews each
/// source after a short pause, like the Settings app, so flicking through
/// twenty categories doesn't fire twenty loads.
struct LiveView: View {
    @Environment(AppModel.self) private var model
    @State private var selection: GuideSource = .favorites
    @State private var pendingSelection: Task<Void, Never>?

    /// Long enough to tell a pause from a flick through the list.
    private static let selectionDelay: Duration = .milliseconds(350)

    var body: some View {
        HStack(alignment: .top, spacing: 60) {
            SourceList(selection: selection, choose: preview)
                .frame(width: 480)
                .focusSection()
            Group {
                if selection == .sports {
                    SportsGuide(onPlay: play)
                } else {
                    ChannelList(source: selection, onPlay: play)
                }
            }
            .id(selection)
            .focusSection()
        }
        .onAppear {
            if !model.favorites.contains(where: { $0.kind == .live }) {
                selection = .all
            }
        }
    }

    private func play(_ request: PlayRequest) {
        Task { await model.play(request) }
    }

    private func preview(_ source: GuideSource) {
        pendingSelection?.cancel()
        pendingSelection = Task {
            try? await Task.sleep(for: Self.selectionDelay)
            if !Task.isCancelled {
                selection = source
            }
        }
    }
}
