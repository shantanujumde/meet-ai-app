import AppKit

// Draws what macOS itself hands back for a bundle — the same path Finder and
// the Dock use — at an exact pixel size, with no interpolation of our own.
//
//   sysicon <app> <pt> <out.png> [--scale N] [--reps]
//
// Without --scale the output is <pt> pixels square, exactly as before (a 1x
// render). --scale 2 asks for the same point size on a 2x backing store, so
// "16 --scale 2" is the 16pt@2x rep a Retina Finder list view draws, not a 32pt
// icon; the system can pick a different rep for the two.
//
// There is deliberately no dark-mode switch. Drawing under NSAppearance
// .darkAqua returns the same pixels as .aqua, for our .icns and for Terminal's
// Assets.car alike (measured on TUR-86): the icon's dark/tinted/clear variant
// follows the user's system icon style, not the drawing appearance.
let args = CommandLine.arguments
func usage() -> Never {
    FileHandle.standardError.write("usage: sysicon <app> <pt> <out.png> [--scale N] [--reps]\n".data(using: .utf8)!)
    exit(2)
}
guard args.count >= 4, let pt = Int(args[2]) else { usage() }
func opt(_ name: String) -> String? {
    guard let i = args.firstIndex(of: name), i + 1 < args.count else { return nil }
    return args[i + 1]
}
let scale = Int(opt("--scale") ?? "1") ?? 1
let px = pt * scale
let outPath = args[3]

// Must be absolute. Handed a relative path, icon(forFile:) does not fail — it
// returns the generic folded-corner document icon, which looks exactly like
// LaunchServices giving up on the bundle. Found on TUR-86.
let appPath = URL(fileURLWithPath: args[1]).standardizedFileURL.path

let icon = NSWorkspace.shared.icon(forFile: appPath)

if args.contains("--reps") {
    let sizes = icon.representations.map { "\(Int($0.size.width))x\(Int($0.size.height))@\($0.pixelsWide)x\($0.pixelsHigh)" }
    print("reps: " + sizes.joined(separator: " "))
}

// Ask for the icon at the logical size, then rasterise at exactly px device pixels.
icon.size = NSSize(width: pt, height: pt)
guard let bmp = NSBitmapImageRep(bitmapDataPlanes: nil, pixelsWide: px, pixelsHigh: px,
                                 bitsPerSample: 8, samplesPerPixel: 4, hasAlpha: true,
                                 isPlanar: false, colorSpaceName: .deviceRGB,
                                 bytesPerRow: 0, bitsPerPixel: 0) else { exit(3) }
bmp.size = NSSize(width: pt, height: pt)
NSGraphicsContext.saveGraphicsState()
NSGraphicsContext.current = NSGraphicsContext(bitmapImageRep: bmp)
NSGraphicsContext.current?.imageInterpolation = .high
icon.draw(in: NSRect(x: 0, y: 0, width: pt, height: pt),
          from: .zero, operation: .sourceOver, fraction: 1.0)
NSGraphicsContext.restoreGraphicsState()
try! bmp.representation(using: .png, properties: [:])!.write(to: URL(fileURLWithPath: outPath))
