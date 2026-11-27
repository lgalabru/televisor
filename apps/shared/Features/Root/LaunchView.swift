import SwiftUI

/// Shown while the saved account signs in. It continues the system launch
/// screen exactly (the same `Logo` asset, centred, on the flat
/// `LaunchBackground` colour), so start-up reads as one screen and only
/// the spinner appears.
struct LaunchView: View {
    var body: some View {
        // Centred on the whole screen, like the system launch screen, not
        // inside the safe area: the status bar would shift it.
        Image("Logo")
            .overlay(alignment: .bottom) {
                // Below the logo, without moving it off centre.
                ProgressView()
                    .alignmentGuide(.bottom) { $0[.top] - spinnerGap }
            }
            .frame(maxWidth: .infinity, maxHeight: .infinity)
            .background(Color("LaunchBackground"))
            .ignoresSafeArea()
    }

    #if os(tvOS)
    private let spinnerGap: CGFloat = 80
    #else
    private let spinnerGap: CGFloat = 44
    #endif
}
