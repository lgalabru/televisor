import SwiftUI

/// Today's games, refreshed every minute while shown: scores and states
/// move, and feeds appear as panels rename their event channels.
@MainActor
@Observable
final class SportsToday {
    private(set) var games: [Game] = []
    private(set) var hasLoaded = false

    /// ESPN updates live scores about every 30 seconds; once a minute is
    /// current enough for a shelf.
    private static let refreshInterval: Duration = .seconds(60)

    func keepCurrent(region: String) async {
        while !Task.isCancelled {
            // A failed refresh keeps the last games: stale beats empty.
            if let games = try? await Televisor.sportsToday(region: region) {
                self.games = games
            }
            hasLoaded = true
            try? await Task.sleep(for: Self.refreshInterval)
        }
    }
}

/// A game as a 16:9 card: both crests, the league, and the state.
struct GameLabel: View {
    let game: Game

    var body: some View {
        VStack(alignment: .leading, spacing: 18) {
            ZStack {
                LinearGradient(
                    colors: [Color(white: 0.2), Color(white: 0.09)],
                    startPoint: .topLeading, endPoint: .bottomTrailing
                )
                HStack(spacing: Theme.itemSpacing * 0.6) {
                    Crest(team: game.away)
                    VStack(spacing: 6) {
                        if let score = game.score {
                            Text(score)
                                .font(.system(size: Theme.scoreSize, weight: .bold, design: .rounded))
                                .monospacedDigit()
                        } else {
                            Text(game.startDate.formatted(date: .omitted, time: .shortened))
                                .font(.system(size: Theme.scoreSize * 0.85, weight: .semibold, design: .rounded))
                        }
                    }
                    .frame(minWidth: Theme.crestSize * 1.25)
                    Crest(team: game.home)
                }
                VStack {
                    HStack {
                        Text(game.leagueName.uppercased())
                            .font(.caption2).fontWeight(.bold).kerning(1.2)
                            .foregroundStyle(.secondary)
                        Spacer()
                        StateBadge(game: game)
                    }
                    Spacer()
                }
                .padding(Theme.itemSpacing * 0.4)
            }
            .frame(width: Theme.wideWidth, height: Theme.wideHeight)
            .cardShape()
            .hoverEffect(.highlight)
            VStack(alignment: .leading, spacing: 4) {
                Text(game.title).font(.callout).fontWeight(.medium).lineLimit(1)
                Text("\(game.channels.count) \(game.channels.count == 1 ? "feed" : "feeds")")
                    .font(.caption).foregroundStyle(.secondary)
            }
            .frame(width: Theme.wideWidth, alignment: .leading)
        }
    }
}

private struct Crest: View {
    let team: GameTeam

    var body: some View {
        VStack(spacing: 10) {
            CachedImage(url: team.logo) { image in
                image.resizable().aspectRatio(contentMode: .fit)
            } placeholder: {
                Text(team.abbreviation ?? String(team.name.prefix(3)))
                    .font(.title3).fontWeight(.heavy)
                    .foregroundStyle(.secondary)
            }
            .frame(width: Theme.crestSize, height: Theme.crestSize)
            Text(team.abbreviation ?? team.name)
                .font(.caption).fontWeight(.semibold)
                .lineLimit(1)
        }
        .frame(width: Theme.crestSize * 1.25)
    }
}

/// LIVE in red with a breathing dot, the kickoff time, or Final.
struct StateBadge: View {
    let game: Game
    @State private var isPulsing = false

    var body: some View {
        switch game.state {
        case .live:
            HStack(spacing: 8) {
                Circle()
                    .fill(.white)
                    .frame(width: 10, height: 10)
                    .opacity(isPulsing ? 0.35 : 1)
                    .animation(.easeInOut(duration: 0.9).repeatForever(), value: isPulsing)
                    .onAppear { isPulsing = true }
                Text(game.status.map { "LIVE · \($0)" } ?? "LIVE")
            }
            .font(.caption2).fontWeight(.heavy)
            .padding(.horizontal, 12).padding(.vertical, 6)
            .background(Theme.accent, in: Capsule())
        case .scheduled:
            Text(game.startDate.formatted(.relative(presentation: .named)))
                .font(.caption2).fontWeight(.semibold)
                .padding(.horizontal, 12).padding(.vertical, 6)
                .background(.white.opacity(0.15), in: Capsule())
        case .finished:
            Text("FINAL")
                .font(.caption2).fontWeight(.heavy)
                .padding(.horizontal, 12).padding(.vertical, 6)
                .background(.white.opacity(0.1), in: Capsule())
        }
    }
}

/// A game card that opens the list of its feeds.
struct GameCard: View {
    let game: Game
    @State private var isChoosing = false

    var body: some View {
        Button {
            isChoosing = true
        } label: {
            GameLabel(game: game)
        }
        .cardButtonStyle()
        #if os(tvOS)
        .fullScreenCover(isPresented: $isChoosing) {
            GameFeedsView(game: game)
        }
        #else
        .sheet(isPresented: $isChoosing) {
            GameFeedsView(game: game)
                .presentationDetents([.medium, .large])
                .presentationBackground(.ultraThinMaterial)
        }
        #endif
    }
}

/// Every feed of one game. The viewer's region comes first and wears its
/// flag, so the French commentary is one click away.
struct GameFeedsView: View {
    let game: Game
    @Environment(AppModel.self) private var model
    @Environment(\.dismiss) private var dismiss

    var body: some View {
        #if os(tvOS)
        ZStack {
            Canvas()
            HStack(alignment: .top, spacing: 80) {
                header.frame(width: 640, alignment: .leading)
                feeds
            }
            .padding(80)
        }
        #else
        ScrollView(.vertical) {
            VStack(alignment: .leading, spacing: 24) {
                header
                feeds
            }
            .padding(Theme.contentInset)
        }
        #endif
    }

    private var header: some View {
        VStack(alignment: .leading, spacing: 16) {
            Text(game.leagueName.uppercased())
                .font(.caption).fontWeight(.heavy).kerning(2)
                .foregroundStyle(Theme.accent)
            Text(game.title)
                .font(.system(size: Theme.pageTitleSize, weight: .bold))
                .lineLimit(2)
            HStack(spacing: 16) {
                StateBadge(game: game)
                if let score = game.score {
                    Text(score).font(.title2).fontWeight(.bold).monospacedDigit()
                }
            }
            Text(game.startDate.formatted(date: .abbreviated, time: .shortened))
                .font(.callout).foregroundStyle(.secondary)
        }
    }

    private var feeds: some View {
        LazyVStack(spacing: 6) {
            ForEach(game.channels) { channel in
                FeedRow(channel: channel) {
                    dismiss()
                    Task { await model.play(play(channel)) }
                }
            }
        }
    }

    private func play(_ channel: Item) -> PlayRequest {
        var request = channel.playRequest
        request.subtitle = game.title
        request.summary = game.leagueName
        return request
    }
}

/// One feed: region flag, a readable name, and the panel's raw name small.
struct FeedRow: View {
    let channel: Item
    let action: () -> Void

    var body: some View {
        Button(action: action) {
            HStack(spacing: 28) {
                Text(Region.code(ofChannel: channel.name).map { Region(code: $0).flag } ?? "📺")
                    .font(.system(size: Theme.iconSize * 0.55))
                    .frame(width: Theme.iconSize * 0.9)
                VStack(alignment: .leading, spacing: 6) {
                    Text(FeedName.readable(channel.name)).font(.headline).lineLimit(1)
                    Text(channel.name).font(.caption2).foregroundStyle(.tertiary).lineLimit(1)
                }
                Spacer(minLength: 0)
                Image(systemName: "play.fill").foregroundStyle(.secondary)
            }
            .padding(.horizontal, Theme.itemSpacing * 0.6)
            .padding(.vertical, Theme.itemSpacing * 0.45)
        }
        .buttonStyle(GuideButtonStyle())
    }
}

/// Event channel names pack status, title, quality and slot into one line:
/// "Live | Eagles vs. Cowboys | all | 8K EXCLUSIVE | FR: SOCCER PPV 28".
/// The viewer wants "FR · SOCCER PPV 28" and the 8K tag.
enum FeedName {
    static func readable(_ name: String) -> String {
        let parts = name.split(separator: "|").map { $0.trimmingCharacters(in: .whitespaces) }
        guard parts.count > 1 else { return name }
        let slot = parts.last ?? name
        let quality = parts.first { $0.contains("4K") || $0.contains("8K") }
        return [slot, quality].compactMap { $0 }.joined(separator: "  ·  ")
    }
}
