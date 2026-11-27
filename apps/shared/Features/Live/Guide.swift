import SwiftUI

/// What the guide lists.
enum GuideSource: Hashable {
    case favorites, recent, all, sports
    case category(Category)
}

/// Where the guide's channels come from: favorites, history, today's
/// games, or a panel category. Categories are grouped by region, the
/// viewer's own first.
struct SourceList: View {
    let selection: GuideSource
    let choose: (GuideSource) -> Void
    @Environment(AppModel.self) private var model
    @State private var categories: [Category]?
    @State private var failure: BridgeError?

    var body: some View {
        ScrollView(.vertical) {
            LazyVStack(alignment: .leading, spacing: 4) {
                SourceButton(title: "Favorites", systemImage: "heart.fill", source: .favorites, selection: selection, choose: choose)
                SourceButton(title: "Sports Today", systemImage: "sportscourt.fill", source: .sports, selection: selection, choose: choose)
                SourceButton(title: "Recently Watched", systemImage: "clock", source: .recent, selection: selection, choose: choose)
                SourceButton(title: "All Channels", systemImage: "square.grid.3x3", source: .all, selection: selection, choose: choose)
                if let categories {
                    ForEach(RegionGroups(categories, preferred: model.region).groups) { group in
                        Text("\(group.region.flag)  \(group.region.name)")
                            .font(.caption).fontWeight(.bold)
                            .foregroundStyle(.secondary)
                            .padding(.horizontal, Theme.itemSpacing * 0.6)
                            .padding(.top, Theme.itemSpacing * 0.8)
                            .padding(.bottom, 4)
                        ForEach(group.categories) { category in
                            SourceButton(
                                title: RegionGroups.shortName(category), systemImage: nil,
                                source: .category(category), selection: selection, choose: choose
                            )
                        }
                    }
                } else if let failure {
                    Text(failure.explanation).font(.caption).foregroundStyle(.secondary).padding()
                } else {
                    ProgressView().padding(.top, 30).frame(maxWidth: .infinity)
                }
            }
            .padding(.vertical, Theme.itemSpacing * 0.6)
        }
        .scrollClipDisabled()
        .task { await load() }
    }

    private func load() async {
        guard categories == nil else { return }
        do {
            categories = try await Televisor.categories(.live)
        } catch let error as BridgeError {
            failure = error
        } catch {
            model.present(error)
        }
    }
}

private struct SourceButton: View {
    let title: String
    let systemImage: String?
    let source: GuideSource
    let selection: GuideSource
    let choose: (GuideSource) -> Void
    @FocusState private var isFocused: Bool

    var body: some View {
        Button {
            choose(source)
        } label: {
            HStack(spacing: 20) {
                if let systemImage {
                    Image(systemName: systemImage).frame(width: Theme.iconSize * 0.45)
                }
                Text(title).lineLimit(1)
                Spacer(minLength: 0)
                if selection == source {
                    Circle().fill(Theme.accent).frame(width: 12, height: 12)
                }
            }
            .font(.callout)
            .fontWeight(selection == source ? .semibold : .regular)
            .padding(.horizontal, Theme.itemSpacing * 0.6)
            .padding(.vertical, Theme.itemSpacing * 0.4)
        }
        .buttonStyle(GuideButtonStyle())
        .focused($isFocused)
        .onChange(of: isFocused) { _, focused in
            // On tvOS, resting focus on a source previews it.
            if focused { choose(source) }
        }
    }
}

/// One row of the guide.
struct GuideChannel: Identifiable, Hashable {
    let id: UInt64
    let name: String
    let image: String?
}

/// One source's channels, with what is on now and next, paging in as the
/// list scrolls. `onPlay` decides where a chosen channel plays.
struct ChannelList: View {
    let source: GuideSource
    let onPlay: (PlayRequest) -> Void
    @Environment(AppModel.self) private var model
    @State private var channels: [GuideChannel] = []
    @State private var total: UInt32?
    @State private var guide: [UInt64: NowNext] = [:]
    @State private var isLoading = false

    /// Ten rows fit a screen; a hundred is ten screens per request.
    private static let pageChannels: UInt32 = 100
    /// How often finished programmes are swapped for what follows.
    private static let guideRefresh: Duration = .seconds(60)

    var body: some View {
        Group {
            if channels.isEmpty && !isLoading && total != nil {
                MessageView(title: emptyTitle, detail: emptyDetail, systemImage: "tv")
            } else {
                ScrollView(.vertical) {
                    LazyVStack(spacing: Theme.itemSpacing * 0.15) {
                        ForEach(channels) { channel in
                            row(channel)
                                .onAppear {
                                    if channel.id == channels.last?.id {
                                        Task { await loadMore() }
                                    }
                                }
                        }
                        if isLoading {
                            ProgressView().padding(40)
                        }
                    }
                    .padding(.vertical, Theme.itemSpacing * 0.6)
                    .padding(.horizontal, Theme.contentInset * 0.5)
                }
                .scrollClipDisabled()
            }
        }
        .task { await loadMore() }
        .task { await keepGuideCurrent() }
    }

    private func row(_ channel: GuideChannel) -> some View {
        Button {
            onPlay(PlayRequest(
                kind: .live, id: channel.id, name: channel.name, ext: nil, image: channel.image,
                subtitle: guide[channel.id]?.now?.title,
                summary: guide[channel.id]?.now?.description
            ))
        } label: {
            GuideRow(
                channel: channel,
                guide: guide[channel.id],
                isFavorite: model.isFavorite(.live, channel.id)
            )
        }
        .buttonStyle(GuideButtonStyle())
        .contextMenu {
            FavoriteButton(kind: .live, id: channel.id, name: channel.name, image: channel.image)
        }
    }

    private var emptyTitle: String {
        switch source {
        case .favorites: "No Favorites Yet"
        case .recent: "Nothing Watched Yet"
        case .all, .category, .sports: "No Channels"
        }
    }

    private var emptyDetail: String {
        switch source {
        case .favorites: "Press and hold a channel, then choose Add to Favorites."
        case .recent: "Channels you watch appear here."
        case .all, .category, .sports: "This category is empty at your provider."
        }
    }

    private func loadMore() async {
        guard !isLoading else { return }
        if let total, UInt32(channels.count) >= total { return }
        isLoading = true
        defer { isLoading = false }
        let added: [GuideChannel]
        switch source {
        case .favorites:
            added = model.favorites.filter { $0.kind == .live }.map(GuideChannel.init)
            total = UInt32(added.count)
        case .recent:
            added = model.history.filter { $0.kind == .live }.map(GuideChannel.init)
            total = UInt32(added.count)
        case .sports:
            preconditionFailure("Sports Today has its own guide")
        case .all, .category:
            let category: String? = if case .category(let category) = source { category.id } else { nil }
            guard let page = try? await Televisor.browse(
                .live, category: category, offset: UInt32(channels.count), limit: Self.pageChannels
            ) else {
                total = UInt32(channels.count)
                return
            }
            added = page.items.map { GuideChannel(id: $0.id, name: $0.name, image: $0.image) }
            total = page.total
        }
        channels.append(contentsOf: added)
        await loadGuide(for: added.map(\.id))
    }

    private func loadGuide(for ids: [UInt64]) async {
        for start in stride(from: 0, to: ids.count, by: Televisor.epgBatchMax) {
            let batch = Array(ids[start..<min(start + Televisor.epgBatchMax, ids.count)])
            // Channels without a guide just show their name.
            guard let entries = try? await Televisor.nowNext(batch) else { continue }
            for entry in entries {
                guide[entry.streamId] = entry
            }
        }
    }

    private func keepGuideCurrent() async {
        while !Task.isCancelled {
            try? await Task.sleep(for: Self.guideRefresh)
            let now = Int64(Date.now.timeIntervalSince1970)
            let stale = guide.values
                .filter { ($0.now?.stop ?? .max) <= now || ($0.now == nil && ($0.next?.start ?? .max) <= now) }
                .map(\.streamId)
            if !stale.isEmpty {
                await loadGuide(for: stale)
            }
        }
    }
}

private extension GuideChannel {
    init(_ entry: Entry) {
        self.init(id: entry.id, name: entry.name, image: entry.image)
    }
}

private struct GuideRow: View {
    let channel: GuideChannel
    let guide: NowNext?
    let isFavorite: Bool

    var body: some View {
        HStack(spacing: Theme.itemSpacing * 0.8) {
            Artwork(url: channel.image, title: channel.name, isLogo: true)
                .frame(width: logoWidth, height: logoWidth * 9 / 16)
                .cardShape()
            VStack(alignment: .leading, spacing: 10) {
                HStack(spacing: 14) {
                    Text(channel.name).font(.headline).lineLimit(1)
                    if isFavorite {
                        Image(systemName: "heart.fill").font(.caption).foregroundStyle(Theme.accent)
                    }
                }
                if let now = guide?.now {
                    TimelineView(.everyMinute) { context in
                        VStack(alignment: .leading, spacing: 10) {
                            HStack {
                                Text(now.title ?? "On Now").font(.callout).lineLimit(1)
                                Spacer(minLength: 24)
                                Text(Format.timeRange(now)).font(.caption).foregroundStyle(.secondary)
                            }
                            ProgressBar(progress: now.progress(at: context.date), tint: Theme.accent)
                                .frame(maxWidth: Theme.textMaxWidth / 2)
                        }
                    }
                } else {
                    Text("No guide information").font(.callout).foregroundStyle(.tertiary)
                }
                if let next = guide?.next {
                    Text("Next  \(next.startDate.formatted(date: .omitted, time: .shortened))  \(next.title ?? "")")
                        .font(.caption)
                        .foregroundStyle(.secondary)
                        .lineLimit(1)
                }
            }
            Spacer(minLength: 0)
        }
        .padding(.horizontal, Theme.itemSpacing * 0.55)
        .padding(.vertical, Theme.itemSpacing * 0.4)
        .frame(maxWidth: .infinity, alignment: .leading)
    }

    #if os(tvOS)
    private let logoWidth: CGFloat = 200
    #else
    private let logoWidth: CGFloat = 88
    #endif
}

/// Sports Today as a guide: each game, then its feeds, playable in place.
struct SportsGuide: View {
    let onPlay: (PlayRequest) -> Void
    @Environment(AppModel.self) private var model
    @State private var sports = SportsToday()

    var body: some View {
        Group {
            if sports.games.isEmpty {
                if sports.hasLoaded {
                    MessageView(
                        title: "No Big Games Right Now",
                        detail: "Games from the NFL, NBA, NHL, MLB and Europe’s top football leagues show here when your provider carries them.",
                        systemImage: "sportscourt"
                    )
                } else {
                    LoadingView(label: "Finding today’s games")
                }
            } else {
                ScrollView(.vertical) {
                    LazyVStack(alignment: .leading, spacing: Theme.itemSpacing * 0.15) {
                        ForEach(sports.games) { game in
                            HStack(spacing: Theme.itemSpacing * 0.45) {
                                StateBadge(game: game)
                                Text(game.title).font(.title3).fontWeight(.semibold).lineLimit(1)
                                if let score = game.score {
                                    Text(score).font(.title3).monospacedDigit().foregroundStyle(.secondary)
                                }
                                Spacer()
                                Text(game.leagueName).font(.caption).foregroundStyle(.secondary)
                            }
                            .padding(.horizontal, Theme.itemSpacing * 0.6)
                            .padding(.top, Theme.itemSpacing * 0.8)
                            .padding(.bottom, 6)
                            ForEach(game.channels) { channel in
                                FeedRow(channel: channel) {
                                    var request = channel.playRequest
                                    request.subtitle = game.title
                                    request.summary = game.leagueName
                                    onPlay(request)
                                }
                            }
                        }
                    }
                    .padding(.vertical, 20)
                }
                .scrollClipDisabled()
            }
        }
        .task(id: model.region) { await sports.keepCurrent(region: model.region) }
    }
}
