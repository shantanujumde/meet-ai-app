import AppKit

// Compares two renders of the same size, pixel for pixel, in straight RGBA.
//
//   pixdiff <a.png> <b.png> [--threshold N]
//   -> "mad=<mean abs diff, 0-255> max=<worst channel diff> changed=<% of pixels>"
//
// A pixel counts as changed when any channel moves by more than the threshold
// (default 8, a little above the noise the compositor adds between two draws of
// an identical icon). Exit 0 whatever the numbers — the caller decides what
// "same" means. Exit 3 on a size mismatch, since a diff across sizes is
// meaningless and should not be scored as "different".
let a = CommandLine.arguments
guard a.count >= 3 else {
    FileHandle.standardError.write("usage: pixdiff <a.png> <b.png> [--threshold N]\n".data(using: .utf8)!)
    exit(2)
}
var threshold = 8
if let i = a.firstIndex(of: "--threshold"), i + 1 < a.count { threshold = Int(a[i + 1]) ?? 8 }

// Redraw each input into a known layout, so a PNG saved with a different
// bytes-per-row or channel order cannot read as a difference.
func load(_ path: String) -> (Int, Int, [UInt8]) {
    guard let d = FileManager.default.contents(atPath: path), let src = NSBitmapImageRep(data: d) else {
        FileHandle.standardError.write("pixdiff: cannot read \(path)\n".data(using: .utf8)!)
        exit(2)
    }
    let w = src.pixelsWide, h = src.pixelsHigh
    let dst = NSBitmapImageRep(bitmapDataPlanes: nil, pixelsWide: w, pixelsHigh: h,
                               bitsPerSample: 8, samplesPerPixel: 4, hasAlpha: true, isPlanar: false,
                               colorSpaceName: .deviceRGB, bytesPerRow: w * 4, bitsPerPixel: 32)!
    NSGraphicsContext.saveGraphicsState()
    NSGraphicsContext.current = NSGraphicsContext(bitmapImageRep: dst)
    src.draw(in: NSRect(x: 0, y: 0, width: w, height: h), from: .zero, operation: .copy,
             fraction: 1, respectFlipped: false, hints: nil)
    NSGraphicsContext.restoreGraphicsState()
    return (w, h, Array(UnsafeBufferPointer(start: dst.bitmapData!, count: w * h * 4)))
}

let (wa, ha, pa) = load(a[1])
let (wb, hb, pb) = load(a[2])
guard wa == wb, ha == hb else { print("size mismatch \(wa)x\(ha) vs \(wb)x\(hb)"); exit(3) }

var sum = 0, worst = 0, changed = 0
for p in stride(from: 0, to: pa.count, by: 4) {
    var moved = false
    for c in 0..<4 {
        let d = abs(Int(pa[p + c]) - Int(pb[p + c]))
        sum += d; worst = max(worst, d)
        if d > threshold { moved = true }
    }
    if moved { changed += 1 }
}
let n = wa * ha
print(String(format: "mad=%.2f max=%d changed=%.2f%%", Double(sum) / Double(n * 4), worst, 100 * Double(changed) / Double(n)))
