import AppKit
import Foundation

// ---------------------------------------------------------------------------
// meet-ai (phase0a spike app) — Contents/MacOS/meet-ai
//
// Stands in for the Tauri shell. Its only jobs are to be the process that
// LaunchServices launches (so it is the TCC *responsible* process) and to spawn
// the capture helper as a child, exactly as SPEC §5 Phase 0a describes.
//
// It also plays a synthetic tone through the default output device, so the
// "is there non-silent system audio" question does not depend on a live Zoom
// call: if the tap works, the tone must show up in system.wav.
// ---------------------------------------------------------------------------

func stamp() -> String { ISO8601DateFormatter().string(from: Date()) }

struct AppLog {
    let handle: FileHandle
    func write(_ msg: String) {
        let line = "[\(stamp())] [app] \(msg)\n"
        handle.write(Data(line.utf8))
        FileHandle.standardError.write(Data(line.utf8))
    }
}

// --- args ---------------------------------------------------------------

var outDir = URL(fileURLWithPath: NSHomeDirectory()).appendingPathComponent("meet-ai-phase0a")
var seconds = 12.0
var wantMic = false
var playTone = true
var tapAutoStart = false
do {
    var it = CommandLine.arguments.dropFirst().makeIterator()
    while let a = it.next() {
        switch a {
        case "--out": if let v = it.next() { outDir = URL(fileURLWithPath: v) }
        case "--seconds": if let v = it.next(), let d = Double(v) { seconds = d }
        case "--mic": wantMic = true
        case "--no-tone": playTone = false
        case "--tap-autostart": tapAutoStart = true
        default: break
        }
    }
}

try? FileManager.default.createDirectory(at: outDir, withIntermediateDirectories: true)
let logURL = outDir.appendingPathComponent("run.log")
FileManager.default.createFile(atPath: logURL.path, contents: nil)
let logger = AppLog(handle: try FileHandle(forWritingTo: logURL))

// Accessory policy: no Dock icon, no window, but still a real NSApplication —
// the same shape the shipped app will have when it spawns the recorder.
NSApplication.shared.setActivationPolicy(.accessory)

let bundle = Bundle.main
logger.write("bundle path        = \(bundle.bundlePath)")
logger.write("bundle identifier  = \(bundle.bundleIdentifier ?? "<none>")")
logger.write("bundle name        = \(bundle.infoDictionary?["CFBundleName"] as? String ?? "<none>")")
logger.write("audio usage string = \(bundle.infoDictionary?["NSAudioCaptureUsageDescription"] as? String ?? "<MISSING>")")
logger.write("mic usage string   = \(bundle.infoDictionary?["NSMicrophoneUsageDescription"] as? String ?? "<MISSING>")")
logger.write("pid=\(getpid()) ppid=\(getppid())")
logger.write("out=\(outDir.path) seconds=\(seconds) mic=\(wantMic) tone=\(playTone)")

// --- synthetic tone -----------------------------------------------------

/// 48 kHz stereo, L=440 Hz, R=660 Hz, -6 dBFS, with a 50 ms fade in/out.
/// Synthetic by construction, so nothing private can ever end up in a fixture.
func makeTone(url: URL, seconds: Double) throws {
    let rate = 48_000.0
    let n = Int(rate * seconds)
    let w = try WavWriter(url: url, sampleRate: rate, channels: 2, flushInterval: 1.0)
    let amp: Float = 0.5
    let fade = Int(rate * 0.05)
    var chunk = [Float]()
    chunk.reserveCapacity(4096)
    for i in 0..<n {
        let t = Double(i) / rate
        var g: Float = 1
        if i < fade { g = Float(i) / Float(fade) }
        if i > n - fade { g = Float(n - i) / Float(fade) }
        chunk.append(amp * g * Float(sin(2 * .pi * 440 * t)))
        chunk.append(amp * g * Float(sin(2 * .pi * 660 * t)))
        if chunk.count >= 4096 {
            try chunk.withUnsafeBufferPointer { try w.append($0) }
            chunk.removeAll(keepingCapacity: true)
        }
    }
    try chunk.withUnsafeBufferPointer { try w.append($0) }
    try w.close()
}

var tonePlayer: Process?
if playTone {
    let toneURL = outDir.appendingPathComponent("tone.wav")
    do {
        // A little longer than the capture window, so the tap is never chasing silence.
        try makeTone(url: toneURL, seconds: seconds + 4)
        logger.write("tone written: \(toneURL.path)")
    } catch {
        logger.write("tone synthesis FAILED: \(error)")
    }
}

// --- spawn the helper ---------------------------------------------------

let helperURL = bundle.bundleURL.appendingPathComponent("Contents/MacOS/meet-tap-probe")
logger.write("helper = \(helperURL.path) exists=\(FileManager.default.fileExists(atPath: helperURL.path))")

let probeLogURL = outDir.appendingPathComponent("probe.log")
FileManager.default.createFile(atPath: probeLogURL.path, contents: nil)
let probeLog = try FileHandle(forWritingTo: probeLogURL)

let helper = Process()
helper.executableURL = helperURL
helper.arguments = ["--out", outDir.path, "--seconds", String(seconds)]
    + (wantMic ? ["--mic"] : [])
    + (tapAutoStart ? ["--tap-autostart"] : [])
helper.standardOutput = probeLog
helper.standardError = probeLog

var exitCode: Int32 = -1
do {
    try helper.run()
    logger.write("helper launched, pid=\(helper.processIdentifier)")

    // Start the tone only once the helper is up, so the capture window overlaps it.
    if playTone {
        Thread.sleep(forTimeInterval: 1.0)
        let p = Process()
        p.executableURL = URL(fileURLWithPath: "/usr/bin/afplay")
        p.arguments = [outDir.appendingPathComponent("tone.wav").path]
        do {
            try p.run()
            tonePlayer = p
            logger.write("afplay launched, pid=\(p.processIdentifier)")
        } catch {
            logger.write("afplay FAILED: \(error)")
        }
    }

    helper.waitUntilExit()
    exitCode = helper.terminationStatus
    logger.write("helper exited with \(exitCode)")
} catch {
    logger.write("helper launch FAILED: \(error)")
}

if let p = tonePlayer, p.isRunning { p.terminate() }
try? probeLog.close()

// Marker file so the runner script knows the app is done, distinct from the
// probe's own result (which is absent if the helper never got that far).
let done: [String: Any] = [
    "app_exit_code": Int(exitCode),
    "finished_at": stamp(),
    "bundle_identifier": bundle.bundleIdentifier ?? "",
    "bundle_path": bundle.bundlePath,
]
try? JSONSerialization.data(withJSONObject: done, options: [.prettyPrinted, .sortedKeys])
    .write(to: outDir.appendingPathComponent("app-done.json"))
logger.write("done")
exit(exitCode)
