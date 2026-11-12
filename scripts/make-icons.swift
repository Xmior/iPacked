// Builds the app icons from the Icon Composer artwork (light/dark PNGs whose
// artwork already has rounded, transparent corners).
//
// 1. Full-bleed layers: fills the transparent corners with the artwork's own
//    gradient so macOS 26+ (Assets.car) masks a solid square and no transparent
//    sliver shows at the edge.
// 2. icon.icns / icon.png for macOS 12–15: the art is clipped to Apple's icon
//    grid (824px rounded square centered in 1024px, drop shadow), like every
//    other Mac app icon.
//
// Usage: swift scripts/make-icons.swift src-tauri/icons/source/light.png src-tauri/icons/source/dark.png <out-dir>
// then copy light/dark.png into AppIcon.icon/Assets, icon.icns + icon.png into icons/,
// and rebuild Assets.car: xcrun actool AppIcon.icon --compile <dir> --platform macosx --minimum-deployment-target 12.0 --app-icon AppIcon --output-partial-info-plist <dir>/p.plist
import AppKit

let a = CommandLine.arguments
guard a.count == 4 else { print("usage: make-icons <light.png> <dark.png> <out-dir>"); exit(2) }
let outDir = URL(fileURLWithPath: a[3])

func load(_ p: String) -> CGImage {
    guard let src = CGImageSourceCreateWithURL(URL(fileURLWithPath: p) as CFURL, nil),
          let img = CGImageSourceCreateImageAtIndex(src, 0, nil) else { fatalError("cannot read \(p)") }
    return img
}

func context(_ s: Int) -> CGContext {
    CGContext(data: nil, width: s, height: s, bitsPerComponent: 8, bytesPerRow: 0,
              space: CGColorSpace(name: CGColorSpace.sRGB)!,
              bitmapInfo: CGImageAlphaInfo.premultipliedLast.rawValue)!
}

func save(_ img: CGImage, _ url: URL) {
    let d = CGImageDestinationCreateWithURL(url as CFURL, "public.png" as CFString, 1, nil)!
    CGImageDestinationAddImage(d, img, nil)
    CGImageDestinationFinalize(d)
}

/// Colour of the opaque pixel at (x, y) (top-left origin).
func pixel(_ img: CGImage, _ x: Int, _ y: Int) -> CGColor {
    let c = context(1)
    c.draw(img, in: CGRect(x: -x, y: -(img.height - 1 - y), width: img.width, height: img.height))
    let p = c.data!.assumingMemoryBound(to: UInt8.self)
    let al = max(CGFloat(p[3]) / 255, 0.001)
    return CGColor(srgbRed: CGFloat(p[0]) / 255 / al, green: CGFloat(p[1]) / 255 / al,
                   blue: CGFloat(p[2]) / 255 / al, alpha: 1)
}

/// Artwork on top of a vertical gradient sampled from its own background.
func fullBleed(_ art: CGImage) -> CGImage {
    let s = art.width
    let c = context(s)
    // Sample just inside the rounded edge, along the left side (no glyph there).
    let x = s * 6 / 100
    let stops = stride(from: 0.0, through: 1.0, by: 0.125).map { t -> CGColor in
        let y = Int(Double(s) * (0.2 + 0.6 * t))
        return pixel(art, x, y)
    }
    // Extend the sampled 20%–80% band to the full height.
    let locs: [CGFloat] = stride(from: 0.0, through: 1.0, by: 0.125).map { 0.2 + 0.6 * CGFloat($0) }
    let g = CGGradient(colorsSpace: CGColorSpace(name: CGColorSpace.sRGB)!, colors: stops as CFArray,
                       locations: locs.map { 1 - $0 })!
    c.drawLinearGradient(g, start: CGPoint(x: 0, y: 0), end: CGPoint(x: 0, y: s),
                         options: [.drawsBeforeStartLocation, .drawsAfterEndLocation])
    // Scale up ~5% so the artwork's own rounded rim falls outside the system mask.
    let z = CGFloat(s) * 0.05
    c.interpolationQuality = .high
    c.draw(art, in: CGRect(x: -z / 2, y: -z / 2, width: CGFloat(s) + z, height: CGFloat(s) + z))
    return c.makeImage()!
}

/// Apple's macOS icon grid: 824/1024 rounded square, radius ~22.4%, soft shadow.
func gridIcon(_ bleed: CGImage, _ size: Int) -> CGImage {
    let c = context(size)
    c.interpolationQuality = .high
    let k = CGFloat(size) / 1024
    let rect = CGRect(x: 100 * k, y: 100 * k + 12 * k, width: 824 * k, height: 824 * k) // a bit up for the shadow
    let path = CGPath(roundedRect: rect, cornerWidth: 185.4 * k, cornerHeight: 185.4 * k, transform: nil)
    c.saveGState()
    c.setShadow(offset: CGSize(width: 0, height: -10 * k), blur: 20 * k, color: CGColor(gray: 0, alpha: 0.3))
    c.addPath(path); c.setFillColor(CGColor(gray: 0, alpha: 1)); c.fillPath()
    c.restoreGState()
    c.addPath(path); c.clip()
    c.draw(bleed, in: rect)
    return c.makeImage()!
}

let fm = FileManager.default
try? fm.createDirectory(at: outDir, withIntermediateDirectories: true)
let light = fullBleed(load(a[1])), dark = fullBleed(load(a[2]))
save(light, outDir.appendingPathComponent("light.png"))
save(dark, outDir.appendingPathComponent("dark.png"))

let set = outDir.appendingPathComponent("icon.iconset")
try? fm.createDirectory(at: set, withIntermediateDirectories: true)
for base in [16, 32, 128, 256, 512] {
    save(gridIcon(light, base), set.appendingPathComponent("icon_\(base)x\(base).png"))
    save(gridIcon(light, base * 2), set.appendingPathComponent("icon_\(base)x\(base)@2x.png"))
}
save(gridIcon(light, 512), outDir.appendingPathComponent("icon.png"))
print("ok")
