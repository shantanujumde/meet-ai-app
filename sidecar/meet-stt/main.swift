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
// ⚠️ SpeechAnalyzer / SpeechTranscriber shipped with macOS 26 and are barely
// present in any model's training data (SETUP.md §4). Everything below was
// written against the SDK's own interface file, not from memory:
//
//   $(xcrun --show-sdk-path)/System/Library/Frameworks/Speech.framework/
//     Modules/Speech.swiftmodule/arm64e-apple-macos.swiftinterface
//
// Re-read that file before changing an API call here. It is the ground truth on
// this machine and it costs nothing to check.

import AVFoundation
import Foundation
import Speech

// MARK: - JSON line protocol
//
// Every line on stdout is one JSON object with a `type`. The Rust driver in
// crates/stt/src/apple.rs parses exactly these shapes; adding a field is safe,
// renaming one is a breaking change to a tested contract.
//
//   {"type":"probe","available":true,"locale":"en-US","installed":true,...}
//   {"type":"final","start_sec":4.12,"end_sec":7.80,"text":"Morning everyone."}
//   {"type":"volatile","start_sec":7.80,"end_sec":8.10,"text":"and the API"}
//   {"type":"progress","fraction":0.42}
//   {"type":"done","duration_sec":63.4}
//   {"type":"error","code":"locale_not_installed","message":"..."}

/// stdout is block-buffered against a pipe by default, which would let the Rust
/// side block waiting for a line the sidecar has already produced. Writing
/// straight to the file handle keeps live transcription actually live.
func emit(_ object: [String: Any]) {
    guard let data = try? JSONSerialization.data(withJSONObject: object),
        var line = String(data: data, encoding: .utf8)
    else { return }
    line += "\n"
    FileHandle.standardOutput.write(Data(line.utf8))
}

func fail(code: String, message: String) -> Never {
    emit(["type": "error", "code": code, "message": message])
    exit(1)
}

/// CMTime -> seconds, defensively. A non-numeric CMTime would otherwise
/// serialize as NaN and make JSONSerialization throw, costing us the line.
func seconds(_ time: CMTime) -> Double {
    guard time.isValid, !time.isIndefinite, time.timescale != 0 else { return 0 }
    let value = CMTimeGetSeconds(time)
    return value.isFinite ? value : 0
}

// MARK: - Arguments

struct Options {
    var wavPath: String?
    var localeIdentifier = "en-US"
    var probe = false
    var installLocale = false
    /// Off by default. Batch transcription of a finished WAV only needs
    /// finalized text (SPEC §2.5 — only finalized text reaches disk); the live
    /// path in Phase 2 turns this on to feed the UI's volatile tail.
    var reportVolatile = false
}

let usage = """
    meet-stt — transcribe a WAV to JSON lines on stdout using Apple's on-device
    SpeechTranscriber (macOS 26+).

    USAGE:
        meet-stt <WAV> [--locale en-US] [--volatile]
        meet-stt --probe [--locale en-US]
        meet-stt --install-locale [--locale en-US]

    OPTIONS:
        --probe             Print engine availability as one JSON line and exit.
        --install-locale    Download and install the on-device model for the
                            locale. This is the ONLY command that touches the
                            network; transcription never does.
        --locale <ID>       BCP-47 locale. Default en-US.
        --volatile          Also emit volatile (non-final) results.
        -h, --help          Print this message.

    OUTPUT:
        One JSON object per line. See the protocol comment at the top of
        sidecar/meet-stt/main.swift.
    """

func parseArguments() -> Options {
    var options = Options()
    var arguments = Array(CommandLine.arguments.dropFirst())

    if arguments.isEmpty || arguments.contains("-h") || arguments.contains("--help") {
        print(usage)
        exit(0)
    }

    while !arguments.isEmpty {
        let argument = arguments.removeFirst()
        switch argument {
        case "--probe":
            options.probe = true
        case "--install-locale":
            options.installLocale = true
        case "--volatile":
            options.reportVolatile = true
        case "--locale":
            guard !arguments.isEmpty else {
                fail(code: "bad_arguments", message: "--locale needs a value")
            }
            options.localeIdentifier = arguments.removeFirst()
        default:
            guard !argument.hasPrefix("-") else {
                fail(code: "bad_arguments", message: "unknown option \(argument)")
            }
            options.wavPath = argument
        }
    }
    return options
}

// MARK: - Engine setup

/// Build the transcriber for a locale.
///
/// `.audioTimeRange` is what gives each result a usable `range`, which becomes
/// the `[HH:MM:SS]` prefix in transcript.md. Without it we would be back to
/// wall-clock timestamps, which SPEC §3.4 forbids.
@available(macOS 26, *)
func makeTranscriber(locale: Locale, reportVolatile: Bool) -> SpeechTranscriber {
    var reporting: Set<SpeechTranscriber.ReportingOption> = []
    if reportVolatile {
        reporting.insert(.volatileResults)
    }
    return SpeechTranscriber(
        locale: locale,
        transcriptionOptions: [],
        reportingOptions: reporting,
        attributeOptions: [.audioTimeRange]
    )
}

/// Resolve a requested locale to one an on-device model actually supports.
@available(macOS 26, *)
func resolveLocale(_ identifier: String) async -> Locale? {
    await SpeechTranscriber.supportedLocale(equivalentTo: Locale(identifier: identifier))
}

/// Is `locale`'s model already on disk? This is the offline-readiness question,
/// as opposed to `supportedLocales`, which only says the OS *could* fetch one.
@available(macOS 26, *)
func isInstalled(_ locale: Locale) async -> Bool {
    let installed = await SpeechTranscriber.installedLocales
    return installed.contains { $0.identifier(.bcp47) == locale.identifier(.bcp47) }
}

// MARK: - Commands

@available(macOS 26, *)
func runProbe(_ options: Options) async {
    let available = SpeechTranscriber.isAvailable
    let resolved = await resolveLocale(options.localeIdentifier)
    let installedLocales = await SpeechTranscriber.installedLocales

    var installed = false
    if let resolved { installed = await isInstalled(resolved) }

    emit([
        "type": "probe",
        "engine": "apple-speech",
        "available": available,
        "requested_locale": options.localeIdentifier,
        "locale": resolved?.identifier(.bcp47) ?? NSNull(),
        "installed": installed,
        "installed_locales": installedLocales.map { $0.identifier(.bcp47) },
        "os_version": ProcessInfo.processInfo.operatingSystemVersionString,
    ])
}

/// The one command that is allowed to use the network.
///
/// Apple's asset install is the moral equivalent of the whisper model download:
/// a one-time fetch that must happen before transcription, never during it.
@available(macOS 26, *)
func runInstallLocale(_ options: Options) async {
    guard SpeechTranscriber.isAvailable else {
        fail(code: "engine_unavailable", message: "SpeechTranscriber is not available on this Mac")
    }
    guard let locale = await resolveLocale(options.localeIdentifier) else {
        fail(
            code: "locale_unsupported",
            message: "no on-device model exists for \(options.localeIdentifier)")
    }

    let transcriber = makeTranscriber(locale: locale, reportVolatile: false)
    do {
        // nil means there is nothing to install — the model is already present.
        guard
            let request = try await AssetInventory.assetInstallationRequest(
                supporting: [transcriber])
        else {
            emit(["type": "done", "locale": locale.identifier(.bcp47), "installed": true])
            return
        }

        let observation = request.progress.observe(\.fractionCompleted) { progress, _ in
            emit(["type": "progress", "fraction": progress.fractionCompleted])
        }
        defer { observation.invalidate() }

        try await request.downloadAndInstall()
        emit(["type": "done", "locale": locale.identifier(.bcp47), "installed": true])
    } catch {
        fail(code: "install_failed", message: "\(error)")
    }
}

@available(macOS 26, *)
func runTranscribe(_ options: Options) async {
    guard let path = options.wavPath else {
        fail(code: "bad_arguments", message: "no WAV path given")
    }
    guard SpeechTranscriber.isAvailable else {
        fail(code: "engine_unavailable", message: "SpeechTranscriber is not available on this Mac")
    }
    guard let locale = await resolveLocale(options.localeIdentifier) else {
        fail(
            code: "locale_unsupported",
            message: "no on-device model exists for \(options.localeIdentifier)")
    }

    // Refuse to transcribe when the model is missing rather than letting the
    // framework quietly reach for the network. "Offline at transcription time"
    // is a product promise (PROBLEM.md), so it is enforced, not assumed.
    guard await isInstalled(locale) else {
        fail(
            code: "locale_not_installed",
            message:
                "the on-device model for \(locale.identifier(.bcp47)) is not installed; "
                + "run `meet-stt --install-locale --locale \(locale.identifier(.bcp47))` once, "
                + "while online")
    }

    let url = URL(fileURLWithPath: path)
    let audioFile: AVAudioFile
    do {
        audioFile = try AVAudioFile(forReading: url)
    } catch {
        fail(code: "unreadable_wav", message: "\(path): \(error)")
    }

    let transcriber = makeTranscriber(locale: locale, reportVolatile: options.reportVolatile)

    // A locale must be reserved before the analyzer will use its model;
    // skipping this is what produces SFSpeechError.assetLocaleNotAllocated.
    do {
        _ = try await AssetInventory.reserve(locale: locale)
    } catch {
        fail(code: "locale_reserve_failed", message: "\(error)")
    }

    let analyzer = SpeechAnalyzer(modules: [transcriber])

    // Drain results concurrently with analysis. The sequence finishes when the
    // analyzer finalizes, which is why this is awaited afterwards rather than
    // cancelled.
    let drain = Task {
        do {
            for try await result in transcriber.results {
                let text = String(result.text.characters)
                // SPEC §3.4: empty text is never written. Dropping it here as
                // well as in Rust costs nothing and keeps the protocol clean.
                guard !text.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty else {
                    continue
                }
                emit([
                    "type": result.isFinal ? "final" : "volatile",
                    "start_sec": seconds(result.range.start),
                    "end_sec": seconds(CMTimeRangeGetEnd(result.range)),
                    "text": text,
                ])
            }
        } catch {
            emit(["type": "error", "code": "results_failed", "message": "\(error)"])
        }
    }

    do {
        _ = try await analyzer.analyzeSequence(from: audioFile)
        try await analyzer.finalizeAndFinishThroughEndOfInput()
    } catch {
        drain.cancel()
        _ = await AssetInventory.release(reservedLocale: locale)
        fail(code: "analysis_failed", message: "\(error)")
    }

    await drain.value
    _ = await AssetInventory.release(reservedLocale: locale)

    let duration = Double(audioFile.length) / audioFile.fileFormat.sampleRate
    emit(["type": "done", "duration_sec": duration])
}

// MARK: - Entry point

let options = parseArguments()

// macOS 26 is the floor for this whole binary (L2). Below it the Rust registry
// must pick whisper instead, and it decides that by running `--probe`, so even
// the unavailable answer has to be a well-formed JSON line.
guard #available(macOS 26, *) else {
    if options.probe {
        emit([
            "type": "probe",
            "engine": "apple-speech",
            "available": false,
            "installed": false,
            "reason": "SpeechTranscriber needs macOS 26 or later",
            "os_version": ProcessInfo.processInfo.operatingSystemVersionString,
        ])
        exit(0)
    }
    fail(code: "engine_unavailable", message: "SpeechTranscriber needs macOS 26 or later")
}

if options.probe {
    await runProbe(options)
} else if options.installLocale {
    await runInstallLocale(options)
} else {
    await runTranscribe(options)
}
