import SwiftUI
#if os(iOS)
import AVFoundation
#endif

@main
struct TelevisorApp: App {
    @State private var model = AppModel()

    init() {
        // Artwork has its own cache (ImagePipeline). URLCache is left to
        // what remains, which is little: 8 MB in memory, 32 MB on disk.
        URLCache.shared = URLCache(
            memoryCapacity: 8 * 1024 * 1024,
            diskCapacity: 32 * 1024 * 1024
        )
        #if os(iOS)
        // Playback keeps going with the screen locked, in Picture in
        // Picture, and over AirPlay; all three need the playback category.
        do {
            try AVAudioSession.sharedInstance().setCategory(.playback, mode: .moviePlayback)
        } catch {
            // Without it video still plays, just not in the background.
            print("televisor: could not set the audio session category: \(error)")
        }
        #endif
    }

    var body: some Scene {
        WindowGroup {
            RootView()
                .environment(model)
                .preferredColorScheme(.dark)
        }
    }
}
