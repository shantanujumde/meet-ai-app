import AppKit

// Contact sheet: each input PNG magnified nearest-neighbour, shown on a dark
// row and a light row, with a label. Nearest-neighbour matters — any smoothing
// would hide exactly the pixel-level collapse we are hunting.
struct Cell { let label: String; let path: String }

let a = CommandLine.arguments
guard a.count >= 4 else { print("usage: sheet out.png <scale> label=path ..."); exit(2) }
let out = a[1]
let scale = CGFloat(Int(a[2])!)
let cells = a[3...].map { s -> Cell in
    let i = s.firstIndex(of: "=")!
    return Cell(label: String(s[s.startIndex..<i]), path: String(s[s.index(after: i)...]))
}

let pad: CGFloat = 16, labelH: CGFloat = 20
var cellW: CGFloat = 0, cellH: CGFloat = 0
var imgs: [NSBitmapImageRep] = []
for c in cells {
    let d = try! Data(contentsOf: URL(fileURLWithPath: c.path))
    let r = NSBitmapImageRep(data: d)!
    imgs.append(r)
    cellW = max(cellW, CGFloat(r.pixelsWide) * scale)
    cellH = max(cellH, CGFloat(r.pixelsHigh) * scale)
}
let W = pad + (cellW + pad) * CGFloat(cells.count)
let H = pad + labelH + (cellH + pad) * 2 + labelH

let bmp = NSBitmapImageRep(bitmapDataPlanes: nil, pixelsWide: Int(W), pixelsHigh: Int(H),
                           bitsPerSample: 8, samplesPerPixel: 4, hasAlpha: true, isPlanar: false,
                           colorSpaceName: .deviceRGB, bytesPerRow: 0, bitsPerPixel: 0)!
NSGraphicsContext.saveGraphicsState()
NSGraphicsContext.current = NSGraphicsContext(bitmapImageRep: bmp)
let ctx = NSGraphicsContext.current!
NSColor(white: 0.5, alpha: 1).setFill()
NSRect(x: 0, y: 0, width: W, height: H).fill()

func drawLabel(_ s: String, _ x: CGFloat, _ y: CGFloat, _ w: CGFloat, dark: Bool) {
    let p = NSMutableParagraphStyle(); p.alignment = .center
    let at: [NSAttributedString.Key: Any] = [
        .font: NSFont.monospacedSystemFont(ofSize: 11, weight: .medium),
        .foregroundColor: dark ? NSColor.white : NSColor.black,
        .paragraphStyle: p,
    ]
    NSAttributedString(string: s, attributes: at).draw(in: NSRect(x: x, y: y, width: w, height: labelH))
}

// Two rows: dark background on top, light below (dark-first, per the brief).
let rows: [(NSColor, Bool, CGFloat)] = [
    (NSColor(red: 0.11, green: 0.11, blue: 0.12, alpha: 1), true,  H - labelH - cellH - pad),
    (NSColor(red: 0.95, green: 0.95, blue: 0.96, alpha: 1), false, H - labelH - cellH * 2 - pad * 2 - labelH),
]
for (bg, isDark, rowY) in rows {
    for (i, _) in cells.enumerated() {
        let x = pad + (cellW + pad) * CGFloat(i)
        bg.setFill()
        NSRect(x: x, y: rowY, width: cellW, height: cellH).fill()
        let r = imgs[i]
        let w = CGFloat(r.pixelsWide) * scale, h = CGFloat(r.pixelsHigh) * scale
        ctx.imageInterpolation = .none
        r.draw(in: NSRect(x: x + (cellW - w) / 2, y: rowY + (cellH - h) / 2, width: w, height: h),
               from: .zero, operation: .sourceOver, fraction: 1, respectFlipped: true, hints: [.interpolation: NSImageInterpolation.none.rawValue])
        if isDark { drawLabel(cells[i].label, x, rowY + cellH + 2, cellW, dark: true) }
    }
}
NSGraphicsContext.restoreGraphicsState()
try! bmp.representation(using: .png, properties: [:])!.write(to: URL(fileURLWithPath: out))
print("wrote \(out) \(Int(W))x\(Int(H))")
