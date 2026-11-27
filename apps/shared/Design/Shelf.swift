import SwiftUI

/// A titled horizontal row, the basic unit of every tvOS browse screen.
/// The row is a focus section, so swiping up or down from anywhere in it
/// lands on the nearest card of the next row instead of skipping rows.
struct Shelf<Content: View>: View {
    let title: String
    @ViewBuilder let content: Content

    var body: some View {
        VStack(alignment: .leading, spacing: shelfTitleSpacing) {
            Text(title)
                .font(.title3)
                .fontWeight(.semibold)
                .foregroundStyle(.secondary)
                .padding(.horizontal, Theme.contentInset)
            ScrollView(.horizontal) {
                LazyHStack(alignment: .top, spacing: Theme.itemSpacing) {
                    content
                }
                .padding(.horizontal, Theme.contentInset)
                // Room for the lifted card and its shadow.
                .padding(.vertical, liftRoom)
            }
            .scrollClipDisabled()
            .scrollIndicators(.hidden)
        }
        .focusGroup()
    }
}

#if os(tvOS)
private let shelfTitleSpacing: CGFloat = 20
private let liftRoom: CGFloat = 24
#else
private let shelfTitleSpacing: CGFloat = 8
private let liftRoom: CGFloat = 6
#endif

/// Shown while the first page of a screen loads.
struct LoadingView: View {
    var label = "Loading"

    var body: some View {
        VStack(spacing: 24) {
            ProgressView()
            Text(label).font(.callout).foregroundStyle(.secondary)
        }
        .frame(maxWidth: .infinity, maxHeight: .infinity)
    }
}

/// A full-screen message with an optional retry.
struct MessageView: View {
    let title: String
    let detail: String
    var systemImage = "tv"
    var retry: (() -> Void)?

    var body: some View {
        VStack(spacing: 28) {
            Image(systemName: systemImage)
                .font(.system(size: Theme.iconSize, weight: .light))
                .foregroundStyle(.secondary)
            Text(title).font(.title2).fontWeight(.semibold)
            Text(detail)
                .font(.callout)
                .foregroundStyle(.secondary)
                .multilineTextAlignment(.center)
                .frame(maxWidth: 900)
            if let retry {
                Button("Try Again", action: retry).padding(.top, 12)
            }
        }
        .frame(maxWidth: .infinity, maxHeight: .infinity)
    }
}
