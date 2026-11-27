import SwiftUI

/// Remote artwork with a designed fallback. Panels miss artwork often, so
/// the fallback is a title card in a colour derived from the name, not a
/// grey box: a shelf of them still reads as a shelf.
struct Artwork: View {
    let url: String?
    let title: String
    var contentMode: ContentMode = .fill
    /// Logos sit inset on a plate; posters fill the frame.
    var isLogo = false

    var body: some View {
        CachedImage(url: url) { image in
            if isLogo {
                image.resizable().aspectRatio(contentMode: .fit).padding(24)
            } else {
                image.resizable().aspectRatio(contentMode: contentMode)
            }
        } placeholder: {
            TitleCard(title: title, isLogo: isLogo)
        }
        .background(isLogo ? AnyShapeStyle(LogoPlate()) : AnyShapeStyle(Theme.raised))
    }
}

private struct TitleCard: View {
    let title: String
    let isLogo: Bool

    var body: some View {
        let hue = Double(stableHash(title) % 360) / 360
        ZStack {
            LinearGradient(
                colors: [
                    Color(hue: hue, saturation: 0.45, brightness: 0.32),
                    Color(hue: hue, saturation: 0.55, brightness: 0.14),
                ],
                startPoint: .topLeading, endPoint: .bottomTrailing
            )
            Text(title)
                .font(isLogo ? .headline : .title3)
                .fontWeight(.semibold)
                .multilineTextAlignment(.center)
                .foregroundStyle(Theme.ink.opacity(0.9))
                .lineLimit(3)
                .minimumScaleFactor(0.6)
                .padding(20)
        }
    }

    /// FNV-1a: the same title gets the same colour on every launch, which
    /// `String.hashValue` does not promise.
    private func stableHash(_ text: String) -> UInt32 {
        var hash: UInt32 = 2_166_136_261
        for byte in text.utf8 {
            hash = (hash ^ UInt32(byte)) &* 16_777_619
        }
        return hash
    }
}

/// Channel logos are drawn for light and dark backgrounds alike; a soft
/// graphite plate suits both.
private struct LogoPlate: ShapeStyle {
    func resolve(in environment: EnvironmentValues) -> some ShapeStyle {
        LinearGradient(
            colors: [Color(white: 0.24), Color(white: 0.13)],
            startPoint: .top, endPoint: .bottom
        )
    }
}

/// The thin bar under Continue Watching cards.
struct ProgressBar: View {
    let progress: Double
    var tint = Theme.ink

    var body: some View {
        GeometryReader { proxy in
            ZStack(alignment: .leading) {
                Capsule().fill(.white.opacity(0.25))
                Capsule().fill(tint).frame(width: proxy.size.width * min(max(progress, 0), 1))
            }
        }
        .frame(height: 6)
    }
}
