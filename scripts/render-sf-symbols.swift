// Renders the SF Symbols used by the UI into template PNGs (ui/sf/<name>.png).
// The UI uses them as CSS masks so they take the current text color.
// Run on macOS: swift scripts/render-sf-symbols.swift
import AppKit

let names = [
  "books.vertical", "archivebox", "ipad.and.iphone", "person.crop.circle",
  "arrow.clockwise", "arrow.down.circle", "square.and.arrow.up", "square.and.arrow.down",
  "folder", "magnifyingglass", "trash", "doc.on.doc", "shippingbox", "checkmark.seal.fill",
  "lock.fill", "lock.open.fill", "arrow.down.to.line", "eye", "cart", "iphone", "ipad",
  "cable.connector", "hourglass", "exclamationmark.triangle.fill", "app.badge", "key.fill",
  "rectangle.portrait.and.arrow.right", "square.and.arrow.down.on.square", "cpu", "calendar",
  "number", "tag", "plus.circle", "square.grid.2x2", "list.bullet", "lock.shield", "pencil", "network", "book", "appletv", "homepod", "pause.circle", "doc", "chevron.left", "globe", "chevron.right", "folder.badge.plus", "arrow.triangle.2.circlepath", "questionmark", "externaldrive", "clock", "checkmark.circle.fill", "xmark.circle.fill", "chevron.right.2", "photo", "music.note", "film", "doc.text", "doc.zipper", "folder.fill", "app.fill", "star.fill", "heart.fill", "arrow.up.doc", "pencil.line", "link", "square.grid.3x3", "info.circle", "gearshape", "sidebar.left", "person.text.rectangle", "tray.and.arrow.down", "desktopcomputer", "wrench.and.screwdriver", "checkmark.shield.fill", "text.alignleft", "sparkles", "character.bubble", "capslock.fill", "eye.slash", "circle.lefthalf.filled", "terminal", "photo.badge.arrow.down",
]
let out = URL(fileURLWithPath: CommandLine.arguments.count > 1 ? CommandLine.arguments[1] : "ui/sf")
try? FileManager.default.createDirectory(at: out, withIntermediateDirectories: true)
let px: CGFloat = 96
for n in names {
  guard let base = NSImage(systemSymbolName: n, accessibilityDescription: nil) else { print("missing", n); continue }
  let img = base.withSymbolConfiguration(.init(pointSize: 64, weight: .regular)) ?? base
  let rep = NSBitmapImageRep(bitmapDataPlanes: nil, pixelsWide: Int(px), pixelsHigh: Int(px), bitsPerSample: 8,
                             samplesPerPixel: 4, hasAlpha: true, isPlanar: false, colorSpaceName: .deviceRGB, bytesPerRow: 0, bitsPerPixel: 0)!
  NSGraphicsContext.saveGraphicsState()
  NSGraphicsContext.current = NSGraphicsContext(bitmapImageRep: rep)
  let s = img.size, k = min(px / s.width, px / s.height) * 0.92
  let r = NSRect(x: (px - s.width * k) / 2, y: (px - s.height * k) / 2, width: s.width * k, height: s.height * k)
  img.draw(in: r)
  NSColor.black.set()
  r.fill(using: .sourceAtop)
  NSGraphicsContext.restoreGraphicsState()
  try! rep.representation(using: .png, properties: [:])!.write(to: out.appendingPathComponent("\(n).png"))
}
print("rendered \(names.count) symbols to \(out.path)")
