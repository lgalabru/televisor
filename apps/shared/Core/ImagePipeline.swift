import CryptoKit
import ImageIO
import SwiftUI
import UIKit

/// Every poster, logo, still and crest the apps show, cached on disk and in
/// memory.
///
/// URLCache only keeps what the server's headers allow, and the artwork
/// hosts behind IPTV panels mostly send none, so shelves refetched the same
/// posters on every scroll. This keeps the original bytes on disk whatever
/// the headers say, and decodes them downsampled to the size they are drawn
/// at, which is what keeps a shelf of 4K posters cheap in memory.
actor ImagePipeline {
    static let shared = ImagePipeline()

    /// Artwork rarely changes behind a URL; a month keeps a library warm.
    private static let diskLifetime: TimeInterval = 30 * 24 * 60 * 60
    /// Room for about 6,000 posters. tvOS may purge Caches under pressure,
    /// which only costs refetching.
    private static let diskBytesMax = 600 * 1024 * 1024
    /// Trimmed back to this share of the limit, so trimming is rare.
    private static let diskTrimTarget = 0.8
    /// Writes between two checks of the disk size.
    private static let writesBetweenTrims = 200
    /// No artwork is larger; anything bigger is a mistake or an attack.
    private static let responseBytesMax = 15 * 1024 * 1024
    /// Downloads in flight. Enough to fill a screen of posters at once
    /// without starving the stream or the panel API.
    private static let downloadsMax = 6
    #if os(tvOS)
    private static let memoryBytesMax = 128 * 1024 * 1024
    #else
    private static let memoryBytesMax = 64 * 1024 * 1024
    #endif

    private let memory = NSCache<NSString, UIImage>()
    private let directory: URL?
    private let session: URLSession
    private var inFlight: [URL: Task<Data?, Never>] = [:]
    private var downloading = 0
    private var waiting: [CheckedContinuation<Void, Never>] = []
    private var writesSinceTrim = 0

    private init() {
        memory.totalCostLimit = Self.memoryBytesMax
        let configuration = URLSessionConfiguration.default
        // This pipeline is the cache; a second copy in URLCache is waste.
        configuration.urlCache = nil
        configuration.requestCachePolicy = .reloadIgnoringLocalCacheData
        configuration.timeoutIntervalForRequest = 20
        session = URLSession(configuration: configuration)
        directory = try? Storage.directory().appending(path: "images", directoryHint: .isDirectory)
        if let directory {
            // Without a directory the cache still works, in memory only.
            try? FileManager.default.createDirectory(at: directory, withIntermediateDirectories: true)
        }
    }

    /// The image at `url`, decoded to fit `pixels` on its longer side.
    /// Nil when it can't be fetched or isn't an image.
    func image(for url: URL, pixels: Int) async -> UIImage? {
        precondition(pixels > 0, "callers ask for a visible size")
        let key = "\(url.absoluteString)#\(pixels)" as NSString
        if let cached = memory.object(forKey: key) {
            return cached
        }
        guard let data = await data(for: url), let image = Self.decode(data, pixels: pixels) else {
            return nil
        }
        let cost = Int(image.size.width * image.size.height * image.scale * image.scale * 4)
        memory.setObject(image, forKey: key, cost: cost)
        return image
    }

    /// The original bytes, from disk or the network.
    func data(for url: URL) async -> Data? {
        if let data = readDisk(url) {
            return data
        }
        if let running = inFlight[url] {
            return await running.value
        }
        let task = Task { await self.download(url) }
        inFlight[url] = task
        let data = await task.value
        inFlight[url] = nil
        return data
    }

    private func download(_ url: URL) async -> Data? {
        await acquireSlot()
        defer { releaseSlot() }
        guard let (data, response) = try? await session.data(from: url),
              let http = response as? HTTPURLResponse, (200..<300).contains(http.statusCode),
              data.count <= Self.responseBytesMax
        else {
            // Missing artwork is common and harmless: the card shows its
            // title instead. Not cached, so it is retried next time.
            return nil
        }
        writeDisk(data, for: url)
        return data
    }

    private func acquireSlot() async {
        if downloading < Self.downloadsMax {
            downloading += 1
            return
        }
        await withCheckedContinuation { waiting.append($0) }
    }

    private func releaseSlot() {
        if waiting.isEmpty {
            downloading -= 1
        } else {
            // The slot passes straight to the next waiter.
            waiting.removeFirst().resume()
        }
        assert(downloading >= 0 && downloading <= Self.downloadsMax)
    }

    // MARK: Disk

    private func file(for url: URL) -> URL? {
        let digest = SHA256.hash(data: Data(url.absoluteString.utf8))
        let name = digest.map { String(format: "%02x", $0) }.joined()
        return directory?.appending(path: name)
    }

    private func readDisk(_ url: URL) -> Data? {
        guard let file = file(for: url),
              let attributes = try? FileManager.default.attributesOfItem(atPath: file.path),
              let modified = attributes[.modificationDate] as? Date
        else { return nil }
        guard Date.now.timeIntervalSince(modified) < Self.diskLifetime else {
            try? FileManager.default.removeItem(at: file)
            return nil
        }
        return try? Data(contentsOf: file)
    }

    private func writeDisk(_ data: Data, for url: URL) {
        guard let file = file(for: url) else { return }
        // A failed write only means a refetch later.
        try? data.write(to: file, options: .atomic)
        writesSinceTrim += 1
        if writesSinceTrim >= Self.writesBetweenTrims {
            writesSinceTrim = 0
            trimDisk()
        }
    }

    /// Oldest files go first until the cache is back under its target.
    private func trimDisk() {
        guard let directory,
              let files = try? FileManager.default.contentsOfDirectory(
                  at: directory, includingPropertiesForKeys: [.fileSizeKey, .contentModificationDateKey]
              )
        else { return }
        var entries = files.compactMap { file -> (URL, Int, Date)? in
            guard let values = try? file.resourceValues(forKeys: [.fileSizeKey, .contentModificationDateKey]),
                  let size = values.fileSize, let date = values.contentModificationDate
            else { return nil }
            return (file, size, date)
        }
        var total = entries.reduce(0) { $0 + $1.1 }
        guard total > Self.diskBytesMax else { return }
        entries.sort { $0.2 < $1.2 }
        let target = Int(Double(Self.diskBytesMax) * Self.diskTrimTarget)
        for (file, size, _) in entries where total > target {
            try? FileManager.default.removeItem(at: file)
            total -= size
        }
    }

    // MARK: Decoding

    /// Decode straight to the drawn size: ImageIO never inflates the full
    /// image, so a 3000-pixel poster costs a 600-pixel poster's memory.
    private static func decode(_ data: Data, pixels: Int) -> UIImage? {
        guard let source = CGImageSourceCreateWithData(data as CFData, nil) else { return nil }
        let options: [CFString: Any] = [
            kCGImageSourceCreateThumbnailFromImageAlways: true,
            kCGImageSourceCreateThumbnailWithTransform: true,
            kCGImageSourceShouldCacheImmediately: true,
            kCGImageSourceThumbnailMaxPixelSize: pixels,
        ]
        guard let image = CGImageSourceCreateThumbnailAtIndex(source, 0, options as CFDictionary) else {
            return nil
        }
        return UIImage(cgImage: image)
    }
}

/// An image from `ImagePipeline`, sized to where it is drawn.
struct CachedImage<Content: View, Placeholder: View>: View {
    let url: String?
    @ViewBuilder let content: (Image) -> Content
    @ViewBuilder let placeholder: () -> Placeholder

    @Environment(\.displayScale) private var displayScale
    @State private var image: UIImage?

    /// Sizes are rounded up to this many pixels, so a poster seen at
    /// nearby sizes decodes once.
    private static var pixelBucket: Int { 128 }

    var body: some View {
        GeometryReader { proxy in
            let pixels = bucket(max(proxy.size.width, proxy.size.height) * displayScale)
            Group {
                if let image {
                    content(Image(uiImage: image))
                        .transition(.opacity)
                } else {
                    placeholder()
                }
            }
            .frame(width: proxy.size.width, height: proxy.size.height)
            .task(id: "\(url ?? "")#\(pixels)") {
                guard let url = url.flatMap(URL.init(string:)) else {
                    image = nil
                    return
                }
                let loaded = await ImagePipeline.shared.image(for: url, pixels: pixels)
                withAnimation(.easeOut(duration: 0.2)) { image = loaded }
            }
        }
    }

    private func bucket(_ pixels: CGFloat) -> Int {
        let size = max(Int(pixels.rounded(.up)), 1)
        return (size + Self.pixelBucket - 1) / Self.pixelBucket * Self.pixelBucket
    }
}
