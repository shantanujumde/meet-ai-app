import AppKit

// Draws what macOS itself hands back for a bundle — the same path Finder and
// the Dock use — at an exact pixel size, with no interpolation of our own.
let args = CommandLine.arguments
guard args.count >= 4 else {
    FileHandle.standardError.write("usage: sysicon <app> <px> <out.png> [--reps]\n".data(using: .utf8)!)
    exit(2)
}
let appPath = args[1]
let px = Int(args[2])!
let outPath = args[3]

let icon = NSWorkspace.shared.icon(forFile: appPath)

if args.contains("--reps") {
    let sizes = icon.representations.map { "\(Int($0.size.width))x\(Int($0.size.height))@\($0.pixelsWide)x\($0.pixelsHigh)" }
    print("reps: " + sizes.joined(separator: " "))
}

// Ask for the icon at the logical size, then rasterise at exactly px device pixels.
icon.size = NSSize(width: px, height: px)
guard let bmp = NSBitmapImageRep(bitmapDataPlanes: nil, pixelsWide: px, pixelsHigh: px,
                                 bitsPerSample: 8, samplesPerPixel: 4, hasAlpha: true,
                                 isPlanar: false, colorSpaceName: .deviceRGB,
                                 bytesPerRow: 0, bitsPerPixel: 0) else { exit(3) }
bmp.size = NSSize(width: px, height: px)
NSGraphicsContext.saveGraphicsState()
NSGraphicsContext.current = NSGraphicsContext(bitmapImageRep: bmp)
NSGraphicsContext.current?.imageInterpolation = .high
icon.draw(in: NSRect(x: 0, y: 0, width: px, height: px),
          from: .zero, operation: .sourceOver, fraction: 1.0)
NSGraphicsContext.restoreGraphicsState()
try! bmp.representation(using: .png, properties: [:])!.write(to: URL(fileURLWithPath: outPath))
