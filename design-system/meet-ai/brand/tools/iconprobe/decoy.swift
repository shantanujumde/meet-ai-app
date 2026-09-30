import AppKit

// A deliberately unmistakable stand-in icon: full-bleed magenta with a lime
// cross. Swapped into a lab copy in place of the real .icns, it answers "is the
// system drawing the .icns or the Assets.car?" at a glance and by the numbers —
// nothing in the meet-ai art is anywhere near either colour.
//
//   decoy <out.png> [px]      (default 1024)
let a = CommandLine.arguments
guard a.count >= 2 else { print("usage: decoy <out.png> [px]"); exit(2) }
let px = a.count >= 3 ? Int(a[2])! : 1024
let s = CGFloat(px)

let bmp = NSBitmapImageRep(bitmapDataPlanes: nil, pixelsWide: px, pixelsHigh: px,
                           bitsPerSample: 8, samplesPerPixel: 4, hasAlpha: true, isPlanar: false,
                           colorSpaceName: .deviceRGB, bytesPerRow: 0, bitsPerPixel: 0)!
NSGraphicsContext.saveGraphicsState()
NSGraphicsContext.current = NSGraphicsContext(bitmapImageRep: bmp)
NSColor(red: 1, green: 0, blue: 1, alpha: 1).setFill()
NSRect(x: 0, y: 0, width: s, height: s).fill()
let cross = NSBezierPath()
cross.move(to: NSPoint(x: 0, y: 0)); cross.line(to: NSPoint(x: s, y: s))
cross.move(to: NSPoint(x: 0, y: s)); cross.line(to: NSPoint(x: s, y: 0))
cross.lineWidth = s * 0.18
NSColor(red: 0, green: 1, blue: 0, alpha: 1).setStroke()
cross.stroke()
NSGraphicsContext.restoreGraphicsState()
try! bmp.representation(using: .png, properties: [:])!.write(to: URL(fileURLWithPath: a[1]))
