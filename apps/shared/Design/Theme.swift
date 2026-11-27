import SwiftUI

/// The palette and measurements every screen shares. tvOS sizes are
/// points on a 1920 × 1080 canvas, after Apple's shelf metrics: posters
/// about 250 wide, wide cards about 400, 40 to 50 between items. iPhone
/// sizes follow the TV app on iOS: a poster row shows about three.
enum Theme {
    /// The landing page's "on air" red.
    static let accent = Color(red: 0.945, green: 0.169, blue: 0.208)
    /// Pure black: what OLED TVs and phones show as "off", so artwork
    /// floats on nothing.
    static let canvas = Color.black
    static let raised = Color(white: 1, opacity: 0.07)
    static let ink = Color(red: 0.937, green: 0.929, blue: 0.902)

    #if os(tvOS)
    static let posterWidth: CGFloat = 250
    static let posterHeight: CGFloat = 375
    static let wideWidth: CGFloat = 420
    static let wideHeight: CGFloat = 236
    static let channelWidth: CGFloat = 320
    static let channelHeight: CGFloat = 180
    static let itemSpacing: CGFloat = 44
    static let shelfSpacing: CGFloat = 56
    static let cornerRadius: CGFloat = 14
    /// Horizontal margin the content keeps; tvOS applies its own 80.
    static let contentInset: CGFloat = 0
    /// How far full-bleed art reaches past the content margin.
    static let bleed: CGFloat = 80
    static let heroArtHeight: CGFloat = 820
    static let heroMinHeight: CGFloat = 700
    static let detailHeaderMinHeight: CGFloat = 760
    static let displayTitleSize: CGFloat = 76
    static let heroTitleSize: CGFloat = 68
    static let pageTitleSize: CGFloat = 60
    static let crestSize: CGFloat = 96
    static let scoreSize: CGFloat = 40
    static let iconSize: CGFloat = 80
    static let textMaxWidth: CGFloat = 1050
    #else
    static let posterWidth: CGFloat = 124
    static let posterHeight: CGFloat = 186
    static let wideWidth: CGFloat = 264
    static let wideHeight: CGFloat = 148
    static let channelWidth: CGFloat = 168
    static let channelHeight: CGFloat = 94
    static let itemSpacing: CGFloat = 14
    static let shelfSpacing: CGFloat = 30
    static let cornerRadius: CGFloat = 10
    static let contentInset: CGFloat = 20
    static let bleed: CGFloat = 20
    static let heroArtHeight: CGFloat = 470
    static let heroMinHeight: CGFloat = 420
    static let detailHeaderMinHeight: CGFloat = 440
    static let displayTitleSize: CGFloat = 34
    static let heroTitleSize: CGFloat = 32
    static let pageTitleSize: CGFloat = 30
    static let crestSize: CGFloat = 48
    static let scoreSize: CGFloat = 22
    static let iconSize: CGFloat = 44
    static let textMaxWidth: CGFloat = 640
    #endif
}

/// The app's background: plain black, edge to edge.
struct Canvas: View {
    var body: some View {
        Theme.canvas.ignoresSafeArea()
    }
}

extension View {
    func cardShape() -> some View {
        clipShape(RoundedRectangle(cornerRadius: Theme.cornerRadius, style: .continuous))
    }
}

enum Format {
    static func duration(_ seconds: UInt32) -> String {
        let minutes = (seconds + 30) / 60
        if minutes < 60 {
            return "\(minutes) min"
        }
        let rest = minutes % 60
        return rest == 0 ? "\(minutes / 60) hr" : "\(minutes / 60) hr \(rest) min"
    }

    static func rating(_ rating: Double?) -> String? {
        guard let rating, rating > 0 else { return nil }
        return String(format: "★ %.1f", min(rating, 10))
    }

    static func timeRange(_ programme: Programme) -> String {
        let style = Date.FormatStyle.dateTime.hour().minute()
        return "\(programme.startDate.formatted(style)) – \(programme.stopDate.formatted(style))"
    }

    /// "Dune · 2021 · 2 hr 35 min · Sci-Fi"
    static func joined(_ parts: [String?]) -> String {
        parts.compactMap { $0 }.filter { !$0.isEmpty }.joined(separator: "  ·  ")
    }
}

extension View {
    /// Cards: on tvOS a borderless button, whose artwork takes the focus
    /// highlight; on touch screens a plain one, so the label keeps its own
    /// colours instead of the tint.
    @ViewBuilder
    func cardButtonStyle() -> some View {
        #if os(tvOS)
        buttonStyle(.borderless)
        #else
        buttonStyle(.plain)
        #endif
    }

    /// tvOS moves focus between sections as a whole (see `Shelf`); touch
    /// screens have no focus to move.
    @ViewBuilder
    func focusGroup() -> some View {
        #if os(tvOS)
        focusSection()
        #else
        self
        #endif
    }
}

/// The name, with the "on air" light.
struct Wordmark: View {
    var size: CGFloat = Theme.displayTitleSize

    var body: some View {
        HStack(spacing: size * 0.24) {
            Circle()
                .fill(Theme.accent)
                .frame(width: size * 0.34, height: size * 0.34)
                .shadow(color: Theme.accent.opacity(0.8), radius: size * 0.24)
            Text("televisor")
                .font(.system(size: size, weight: .bold, design: .rounded))
                .foregroundStyle(Theme.ink)
        }
    }
}

/// A row that brightens and lifts slightly under focus (tvOS) or press
/// (touch). Lighter than the system's white platter, so a long list of
/// rows stays calm.
struct GuideButtonStyle: ButtonStyle {
    func makeBody(configuration: Configuration) -> some View {
        StyledRow(configuration: configuration)
    }

    private struct StyledRow: View {
        let configuration: Configuration
        @Environment(\.isFocused) private var isFocused

        var body: some View {
            let isLit = isFocused || configuration.isPressed
            configuration.label
                .foregroundStyle(isLit ? Theme.ink : Theme.ink.opacity(0.78))
                .background(
                    RoundedRectangle(cornerRadius: Theme.cornerRadius, style: .continuous)
                        .fill(isLit ? Color.white.opacity(0.17) : .clear)
                )
                .scaleEffect(configuration.isPressed ? 0.98 : isFocused ? 1.02 : 1)
                .shadow(color: .black.opacity(isFocused ? 0.45 : 0), radius: 24, y: 12)
                .animation(.easeOut(duration: 0.18), value: isFocused)
                .animation(.easeOut(duration: 0.1), value: configuration.isPressed)
                .contentShape(Rectangle())
        }
    }
}
