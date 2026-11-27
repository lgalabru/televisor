import SwiftUI

// Card labels. The interactive wrapper (`ItemCard`, or a NavigationLink)
// gives them a borderless button style, and the artwork carries the
// highlight effect: tvOS lifts the art, tilts it under the remote's touch
// surface and adds the sheen, while the caption stays flat underneath, as
// on the Apple TV app.

/// A 2:3 poster for a movie or show.
struct PosterLabel: View {
    let title: String
    var caption: String?
    let image: String?
    var progress: Double?

    var body: some View {
        VStack(alignment: .leading, spacing: 18) {
            Artwork(url: image, title: title)
                .frame(width: Theme.posterWidth, height: Theme.posterHeight)
                .overlay(alignment: .bottom) { ProgressOverlay(progress: progress) }
                .cardShape()
                .hoverEffect(.highlight)
            Caption(title: title, detail: caption, width: Theme.posterWidth)
        }
    }
}

/// A 16:9 card: episode stills, Continue Watching.
struct WideLabel: View {
    let title: String
    var caption: String?
    let image: String?
    var progress: Double?
    var isLogo = false

    var body: some View {
        VStack(alignment: .leading, spacing: 18) {
            Artwork(url: image, title: title, isLogo: isLogo)
                .frame(width: Theme.wideWidth, height: Theme.wideHeight)
                .overlay(alignment: .bottom) { ProgressOverlay(progress: progress) }
                .cardShape()
                .hoverEffect(.highlight)
            Caption(title: title, detail: caption, width: Theme.wideWidth)
        }
    }
}

/// A channel: its logo on a plate.
struct ChannelLabel: View {
    let name: String
    let image: String?
    var detail: String?

    var body: some View {
        VStack(alignment: .leading, spacing: 18) {
            Artwork(url: image, title: name, isLogo: true)
                .frame(width: Theme.channelWidth, height: Theme.channelHeight)
                .cardShape()
                .hoverEffect(.highlight)
            Caption(title: name, detail: detail, width: Theme.channelWidth)
        }
    }
}

/// The tile closing a shelf that has more than it shows.
struct SeeAllLabel: View {
    let library: Library

    var body: some View {
        let isPoster = library != .live
        VStack(spacing: 16) {
            Image(systemName: "square.grid.2x2")
                .font(.system(size: Theme.iconSize * 0.65, weight: .light))
            Text("See All").font(.headline)
        }
        .foregroundStyle(Theme.ink.opacity(0.85))
        .frame(
            width: isPoster ? Theme.posterWidth : Theme.channelWidth,
            height: isPoster ? Theme.posterHeight : Theme.channelHeight
        )
        .background(Theme.raised)
        .cardShape()
        .hoverEffect(.highlight)
    }
}

private struct ProgressOverlay: View {
    let progress: Double?

    var body: some View {
        if let progress {
            ProgressBar(progress: progress)
                .padding(16)
                .background(
                    LinearGradient(colors: [.clear, .black.opacity(0.6)], startPoint: .top, endPoint: .bottom)
                )
        }
    }
}

private struct Caption: View {
    let title: String
    let detail: String?
    let width: CGFloat

    var body: some View {
        VStack(alignment: .leading, spacing: 4) {
            Text(title)
                .font(.callout)
                .fontWeight(.medium)
                .lineLimit(1)
            if let detail {
                Text(detail)
                    .font(.caption)
                    .foregroundStyle(.secondary)
                    .lineLimit(1)
            }
        }
        .frame(width: width, alignment: .leading)
    }
}

/// Any catalog item as a card: channels play on select, movies and shows
/// open their page. Long-press adds or removes a favorite.
struct ItemCard: View {
    let item: Item
    @Environment(AppModel.self) private var model

    var body: some View {
        switch item.library {
        case .live:
            Button {
                Task { await model.play(item.playRequest) }
            } label: {
                ChannelLabel(name: item.title.text, image: item.image)
            }
            .cardButtonStyle()
            .contextMenu { FavoriteButton(kind: .live, id: item.id, name: item.name, image: item.image) }
        case .movies:
            NavigationLink(value: item) {
                PosterLabel(
                    title: item.title.text, caption: item.caption, image: item.image,
                    progress: model.positions.mark(.movie, item.id)?.progress
                )
            }
            .cardButtonStyle()
            .contextMenu { FavoriteButton(kind: .movie, id: item.id, name: item.name, image: item.image) }
        case .series:
            NavigationLink(value: item) {
                PosterLabel(title: item.title.text, caption: item.caption, image: item.image)
            }
            .cardButtonStyle()
            .contextMenu { FavoriteButton(kind: .series, id: item.id, name: item.name, image: item.image) }
        }
    }
}

struct FavoriteButton: View {
    let kind: MediaKind
    let id: UInt64
    let name: String
    let image: String?
    @Environment(AppModel.self) private var model

    var body: some View {
        let wording = FavoriteWording(kind: kind, isFavorite: model.isFavorite(kind, id))
        Button {
            Task { await model.toggleFavorite(kind, id: id, name: name, image: image) }
        } label: {
            Label(wording.action, systemImage: wording.menuSymbol)
        }
    }
}

/// Shows are bookmarked to follow; channels and movies are favorites.
struct FavoriteWording {
    let kind: MediaKind
    let isFavorite: Bool

    private var isShow: Bool { kind == .series }

    var action: String {
        switch (isShow, isFavorite) {
        case (true, true): "Remove Bookmark"
        case (true, false): "Bookmark Show"
        case (false, true): "Remove from Favorites"
        case (false, false): "Add to Favorites"
        }
    }

    var symbol: String {
        let base = isShow ? "bookmark" : "heart"
        return isFavorite ? "\(base).fill" : base
    }

    var menuSymbol: String {
        let base = isShow ? "bookmark" : "heart"
        return isFavorite ? "\(base).slash" : base
    }
}

extension Item {
    var caption: String? {
        switch library {
        case .live: nil
        case .movies, .series: Format.joined([title.year, Format.rating(rating)])
        }
    }

    var playRequest: PlayRequest {
        switch library {
        case .live:
            PlayRequest(kind: .live, id: id, name: name, ext: nil, image: image)
        case .movies:
            PlayRequest(kind: .movie, id: id, name: name, ext: ext, image: image)
        case .series:
            preconditionFailure("a show is not playable; pick an episode")
        }
    }
}
