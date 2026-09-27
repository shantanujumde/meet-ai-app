// sidecar/meet-stt — the Swift speech helper.
//
// SPEC §2.6. A single CLI binary that lives inside the app bundle at
// Contents/MacOS/meet-stt. It reads WAV paths and writes JSON lines to stdout.
//
// Keep it dumb: WAV in, transcript lines out. No app logic belongs here. The
// process boundary and the JSON shape are the contract, and they are tested
// from Rust in crates/stt/tests/ rather than with XCTest — a bare swiftc build
// has no test target and adding SwiftPM just to get one is disproportionate.
//
// Phase 1 replaces the body of `transcribe` with SpeechAnalyzer /
// SpeechTranscriber. ⚠️ Those shipped with macOS 26 and are barely present in
// any model's training data — read Apple's docs and FluidInference/swift-scribe
// before writing against them, not from memory.

import Foundation

let usage = """
meet-stt — transcribe a 16 kHz mono WAV to JSON lines on stdout.

USAGE:
    meet-stt <WAV>
    meet-stt --probe

OPTIONS:
    --probe    Print engine availability as one JSON line and exit.
    -h         Print this message.

OUTPUT:
    One JSON object per line, e.g.
    {"type":"final","start_sec":4,"text":"Morning everyone."}
"""

/// Emit one JSON line. Anything that is not a JSON line is a protocol violation.
func emit(_ object: [String: Any]) {
    guard let data = try? JSONSerialization.data(withJSONObject: object),
          let line = String(data: data, encoding: .utf8)
    else { return }
    print(line)
}

let args = Array(CommandLine.arguments.dropFirst())

switch args.first {
case nil, "-h", "--help":
    print(usage)
    exit(0)

case "--probe":
    // Phase 1 replaces this with a real SpeechTranscriber availability check.
    emit([
        "type": "probe",
        "engine": "apple-speech",
        "available": false,
        "reason": "not implemented yet",
    ])
    exit(0)

default:
    FileHandle.standardError.write(
        Data("meet-stt: transcription lands in Phase 1.\n".utf8))
    exit(1)
}
