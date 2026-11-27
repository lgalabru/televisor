// Render the app icons and the launch-screen logo of both apps from
// `icon-source.png`.
//
//     swiftc -O apps/branding/make-icons.swift -o /tmp/make-icons
//     /tmp/make-icons apps
//
// The source is a screenshot of the icon on a blurred background, so it is
// not cropped as is: its corners show the background, and the systems
// apply their own rounded mask. Instead the pirate-and-"tv" logo is keyed
// off its dark plate and composited onto a fresh plate at every size. On
// tvOS the logo and the plate are separate layers, which gives the icon
// the system's parallax tilt under focus.

import CoreGraphics
import Foundation
import ImageIO
import UniformTypeIdentifiers

/// Where the logo sits in the source, in pixels from the top left, with a
/// margin so the glossy edges and glow are kept.
let logoBox = CGRect(x: 190, y: 368, width: 840, height: 440)
/// The source plate's colour, top and bottom, sampled at its flat areas.
let plateTop = (r: 30.0, g: 29.0, b: 32.0)
let plateBottom = (r: 13.0, g: 13.0, b: 16.0)
/// Brightness over the plate where the logo starts (fully transparent
/// below) and where it is fully opaque. The plate is about 16 to 30; its
/// soft shadow reaches about 40 and would ring the logo on pure black.
let keyLow = 44.0
let keyHigh = 84.0

let space = CGColorSpace(name: CGColorSpace.sRGB)!

func load(_ path: String) -> CGImage {
    guard let source = CGImageSourceCreateWithURL(URL(fileURLWithPath: path) as CFURL, nil),
          let image = CGImageSourceCreateImageAtIndex(source, 0, nil)
    else { fatalError("cannot read \(path)") }
    return image
}

func bitmap(_ width: Int, _ height: Int) -> CGContext {
    guard let context = CGContext(
        data: nil, width: width, height: height, bitsPerComponent: 8, bytesPerRow: width * 4,
        space: space, bitmapInfo: CGImageAlphaInfo.premultipliedLast.rawValue
    ) else { fatalError("cannot allocate \(width)x\(height)") }
    context.interpolationQuality = .high
    return context
}

/// The logo with the plate keyed out: alpha follows brightness, so dark
/// plate pixels vanish and the glossy letters and their glow stay.
func keyedLogo(from source: CGImage) -> CGImage {
    let width = Int(logoBox.width), height = Int(logoBox.height)
    guard let crop = source.cropping(to: logoBox) else { fatalError("logo box is outside the source") }
    let context = bitmap(width, height)
    context.draw(crop, in: CGRect(x: 0, y: 0, width: width, height: height))
    let pixels = context.data!.bindMemory(to: UInt8.self, capacity: width * height * 4)
    for index in 0..<(width * height) {
        let offset = index * 4
        let brightest = Double(max(pixels[offset], pixels[offset + 1], pixels[offset + 2]))
        let alpha = min(max((brightest - keyLow) / (keyHigh - keyLow), 0), 1)
        // Premultiplied storage: scale colour by the new alpha.
        for channel in 0..<3 {
            pixels[offset + channel] = UInt8(Double(pixels[offset + channel]) * alpha)
        }
        pixels[offset + 3] = UInt8(alpha * 255)
    }
    return context.makeImage()!
}

func drawPlate(_ context: CGContext, _ width: Int, _ height: Int) {
    let colors = [plateTop, plateBottom].map {
        CGColor(colorSpace: space, components: [$0.r / 255, $0.g / 255, $0.b / 255, 1])!
    }
    let gradient = CGGradient(colorsSpace: space, colors: colors as CFArray, locations: [0, 1])!
    // CoreGraphics' origin is bottom left: the lighter colour goes on top.
    context.drawLinearGradient(
        gradient, start: CGPoint(x: 0, y: height), end: .zero, options: []
    )
}

/// The logo centred, `share` of the canvas width wide (or `heightShare`
/// of its height, whichever is smaller).
func drawLogo(_ logo: CGImage, _ context: CGContext, _ width: Int, _ height: Int, share: Double, heightShare: Double) {
    let aspect = Double(logo.height) / Double(logo.width)
    var logoWidth = Double(width) * share
    if logoWidth * aspect > Double(height) * heightShare {
        logoWidth = Double(height) * heightShare / aspect
    }
    let logoHeight = logoWidth * aspect
    let rect = CGRect(
        x: (Double(width) - logoWidth) / 2, y: (Double(height) - logoHeight) / 2,
        width: logoWidth, height: logoHeight
    )
    context.draw(logo, in: rect)
}

func write(_ image: CGImage, _ path: String) {
    try? FileManager.default.createDirectory(
        atPath: (path as NSString).deletingLastPathComponent, withIntermediateDirectories: true
    )
    guard let destination = CGImageDestinationCreateWithURL(
        URL(fileURLWithPath: path) as CFURL, UTType.png.identifier as CFString, 1, nil
    ) else { fatalError("cannot write \(path)") }
    CGImageDestinationAddImage(destination, image, nil)
    guard CGImageDestinationFinalize(destination) else { fatalError("cannot write \(path)") }
}

/// Opaque: plate and logo. Used where there are no layers.
func flat(_ logo: CGImage, _ width: Int, _ height: Int, share: Double, heightShare: Double) -> CGImage {
    let context = bitmap(width, height)
    drawPlate(context, width, height)
    drawLogo(logo, context, width, height, share: share, heightShare: heightShare)
    return context.makeImage()!
}

func back(_ width: Int, _ height: Int) -> CGImage {
    let context = bitmap(width, height)
    drawPlate(context, width, height)
    return context.makeImage()!
}

func front(_ logo: CGImage, _ width: Int, _ height: Int, share: Double) -> CGImage {
    let context = bitmap(width, height)
    drawLogo(logo, context, width, height, share: share, heightShare: 0.7)
    return context.makeImage()!
}

func json(_ object: Any, _ path: String) {
    let data = try! JSONSerialization.data(withJSONObject: object, options: [.prettyPrinted, .sortedKeys])
    try? FileManager.default.createDirectory(
        atPath: (path as NSString).deletingLastPathComponent, withIntermediateDirectories: true
    )
    FileManager.default.createFile(atPath: path, contents: data)
}

let info: [String: Any] = ["author": "xcode", "version": 1]
let apps = CommandLine.arguments.dropFirst().first ?? "apps"
let logo = keyedLogo(from: load("\(apps)/branding/icon-source.png"))

// iOS: one 1024 square; the system draws every smaller size from it.
let iosSet = "\(apps)/ios/Televisor/Assets.xcassets/AppIcon.appiconset"
write(flat(logo, 1024, 1024, share: 0.78, heightShare: 0.78), "\(iosSet)/AppIcon.png")
json([
    "images": [["filename": "AppIcon.png", "idiom": "universal", "platform": "ios", "size": "1024x1024"]],
    "info": info,
], "\(iosSet)/Contents.json")

// tvOS: layered icons for the Home Screen and the App Store, and the Top
// Shelf banners. The logo stays inside the middle 68 percent, the area
// tvOS never crops while tilting the icon.
let brand = "\(apps)/tvos/Televisor/Assets.xcassets/App Icon & Top Shelf Image.brandassets"
json([
    "assets": [
        ["filename": "App Icon - App Store.imagestack", "idiom": "tv", "role": "primary-app-icon", "size": "1280x768"],
        ["filename": "App Icon.imagestack", "idiom": "tv", "role": "primary-app-icon", "size": "400x240"],
        ["filename": "Top Shelf Image Wide.imageset", "idiom": "tv", "role": "top-shelf-image-wide", "size": "2320x720"],
        ["filename": "Top Shelf Image.imageset", "idiom": "tv", "role": "top-shelf-image", "size": "1920x720"],
    ],
    "info": info,
], "\(brand)/Contents.json")

func stack(_ name: String, idiom: String, sizes: [(scale: Int, width: Int, height: Int)]) {
    let root = "\(brand)/\(name).imagestack"
    json(["info": info, "layers": [["filename": "Front.imagestacklayer"], ["filename": "Back.imagestacklayer"]]], "\(root)/Contents.json")
    for layer in ["Front", "Back"] {
        let set = "\(root)/\(layer).imagestacklayer/Content.imageset"
        json(["info": info], "\(root)/\(layer).imagestacklayer/Contents.json")
        var images: [[String: Any]] = []
        for size in sizes {
            let file = "\(layer.lowercased())@\(size.scale)x.png"
            let image = layer == "Front"
                ? front(logo, size.width, size.height, share: 0.68)
                : back(size.width, size.height)
            write(image, "\(set)/\(file)")
            images.append(["filename": file, "idiom": idiom, "scale": "\(size.scale)x"])
        }
        json(["images": images, "info": info], "\(set)/Contents.json")
    }
}
stack("App Icon", idiom: "tv", sizes: [(1, 400, 240), (2, 800, 480)])
stack("App Icon - App Store", idiom: "tv-marketing", sizes: [(1, 1280, 768)])

func shelf(_ name: String, sizes: [(scale: Int, width: Int, height: Int)]) {
    let set = "\(brand)/\(name).imageset"
    var images: [[String: Any]] = []
    for size in sizes {
        let file = "shelf@\(size.scale)x.png"
        write(flat(logo, size.width, size.height, share: 0.5, heightShare: 0.42), "\(set)/\(file)")
        images.append(["filename": file, "idiom": "tv", "scale": "\(size.scale)x"])
    }
    json(["images": images, "info": info], "\(set)/Contents.json")
}
shelf("Top Shelf Image", sizes: [(1, 1920, 720), (2, 3840, 1440)])
shelf("Top Shelf Image Wide", sizes: [(1, 2320, 720), (2, 4640, 1440)])

// The launch screen: the keyed logo on the app's canvas colour, used by the
// system launch screen and by the app's own launch view, so the hand-over
// between them is invisible. Same pixels, declared at 3x on iPhone (280
// points wide) and 2x on Apple TV (420 points on a 1920-point screen).
for (app, scale) in [("ios", 3), ("tvos", 2)] {
    let catalog = "\(apps)/\(app)/Televisor/Assets.xcassets"
    write(logo, "\(catalog)/Logo.imageset/logo.png")
    json([
        "images": [["filename": "logo.png", "idiom": "universal", "scale": "\(scale)x"]],
        "info": info,
    ], "\(catalog)/Logo.imageset/Contents.json")
    // Theme.canvas in apps/shared/Design/Theme.swift.
    json([
        "colors": [[
            "color": ["color-space": "srgb", "components": [
                "red": "0.000", "green": "0.000", "blue": "0.000", "alpha": "1.000",
            ]],
            "idiom": "universal",
        ]],
        "info": info,
    ], "\(catalog)/LaunchBackground.colorset/Contents.json")
}

print("icons written")
