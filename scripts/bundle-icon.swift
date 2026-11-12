// Renders the icon macOS shows for an app bundle (including icons that live only
// in Assets.car, e.g. tvOS / visionOS apps) to a PNG.
// Usage: bundle-icon <App.app> <out.png> [size]
import AppKit

let args = CommandLine.arguments
guard args.count >= 3 else { exit(2) }
let size = args.count > 3 ? CGFloat(Double(args[3]) ?? 512) : 512
let icon = NSWorkspace.shared.icon(forFile: args[1])
icon.size = NSSize(width: size, height: size)
guard let rep = NSBitmapImageRep(bitmapDataPlanes: nil, pixelsWide: Int(size), pixelsHigh: Int(size), bitsPerSample: 8,
                                 samplesPerPixel: 4, hasAlpha: true, isPlanar: false, colorSpaceName: .deviceRGB, bytesPerRow: 0, bitsPerPixel: 0)
else { exit(1) }
NSGraphicsContext.saveGraphicsState()
NSGraphicsContext.current = NSGraphicsContext(bitmapImageRep: rep)
icon.draw(in: NSRect(x: 0, y: 0, width: size, height: size))
NSGraphicsContext.restoreGraphicsState()
guard let png = rep.representation(using: .png, properties: [:]) else { exit(1) }
try? png.write(to: URL(fileURLWithPath: args[2]))
