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
//   {"type":"ready","locale":"en-US","sample_rate":16000,...}   (--stdin only)
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
    /// Read raw capture frames from stdin instead of opening a finished WAV.
    /// This is the live path (TUR-31): the recorder tees one channel's samples
    /// into this process as they are captured.
    var useStdin = false
}

let usage = """
    meet-stt — transcribe audio to JSON lines on stdout using Apple's on-device
    SpeechTranscriber (macOS 26+).

    USAGE:
        meet-stt <WAV> [--locale en-US] [--volatile]
        meet-stt --stdin [--locale en-US] [--volatile]
        meet-stt --probe [--locale en-US]
        meet-stt --install-locale [--locale en-US]

    OPTIONS:
        --stdin             Transcribe a live stream instead of a file. Reads
                            raw 16 kHz mono signed 16-bit little-endian frames
                            from stdin, with no WAV header, until EOF.
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
        case "--stdin":
            options.useStdin = true
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

/// The checks every transcribing command shares: engine present, locale
/// resolvable, model already on disk, locale reserved. Returns the resolved
/// locale; never returns if any of it fails.
///
/// Reserving is the step that is easy to miss: the analyzer will not use a
/// locale's model until it is reserved, and skipping it produces
/// `SFSpeechError.assetLocaleNotAllocated` rather than anything self-describing.
@available(macOS 26, *)
func preflight(_ options: Options, transcriber: SpeechTranscriber, locale: Locale) async {
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
    do {
        _ = try await AssetInventory.reserve(locale: locale)
    } catch {
        fail(code: "locale_reserve_failed", message: "\(error)")
    }
}

/// Resolve the engine and locale, or fail with a typed code.
@available(macOS 26, *)
func resolveEngine(_ options: Options) async -> Locale {
    guard SpeechTranscriber.isAvailable else {
        fail(code: "engine_unavailable", message: "SpeechTranscriber is not available on this Mac")
    }
    guard let locale = await resolveLocale(options.localeIdentifier) else {
        fail(
            code: "locale_unsupported",
            message: "no on-device model exists for \(options.localeIdentifier)")
    }
    return locale
}

/// Drain results concurrently with analysis, one JSON line each.
///
/// The sequence finishes when the analyzer finalizes, which is why callers
/// await this task afterwards rather than cancelling it — cancelling would
/// throw away results the analyzer has already committed to.
@available(macOS 26, *)
func startDraining(_ transcriber: SpeechTranscriber) -> Task<Void, Never> {
    Task {
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
}

@available(macOS 26, *)
func runTranscribe(_ options: Options) async {
    guard let path = options.wavPath else {
        fail(code: "bad_arguments", message: "no WAV path given")
    }
    let locale = await resolveEngine(options)

    let url = URL(fileURLWithPath: path)
    let audioFile: AVAudioFile
    do {
        audioFile = try AVAudioFile(forReading: url)
    } catch {
        fail(code: "unreadable_wav", message: "\(path): \(error)")
    }

    let transcriber = makeTranscriber(locale: locale, reportVolatile: options.reportVolatile)
    await preflight(options, transcriber: transcriber, locale: locale)

    let analyzer = SpeechAnalyzer(modules: [transcriber])
    let drain = startDraining(transcriber)

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

// MARK: - The live path: raw capture frames on stdin
//
// TUR-31. `meet-stt <wav>` needs a *finished* file, which is why the Apple
// engine had no streaming path. The recorder cannot hand over a finished file
// while the meeting is still happening, and it cannot hand over a growing one
// either: `AVAudioFile(forReading:)` takes its length from the WAV header at
// open, and SPEC §3.3 only rewrites that header every 5 seconds — so a reader
// that trusted it would stop at whatever the last checkpoint declared and call
// it end-of-meeting.
//
// So: a pipe. The recorder tees one channel's samples into this process as it
// captures them, and the wire format is the rawest thing that can carry them —
// 16 kHz mono signed 16-bit little-endian frames, no header, no framing, EOF
// means the meeting ended. That is exactly what the recorder already has in
// hand on its way to `hound`, and exactly what `SttSession::feed(&[i16])`
// already takes on the Rust side.

/// L4's capture rate. Both the tap and the mic are resampled to this before
/// anything downstream sees them, so the wire format has one rate, not a
/// negotiated one.
let liveSampleRate = 16_000.0

/// How much audio one read asks for: 100 ms, 3200 bytes.
///
/// Small enough that a volatile hypothesis is not held back by our own
/// buffering, large enough that a 45-minute meeting is ~27k reads rather than
/// millions.
let liveFramesPerChunk = 1_600

/// stdin, pulled by the analyzer.
///
/// This is deliberately **pull**-based: `next()` reads only when the analyzer
/// asks for more. That makes the 64 KB pipe itself the backpressure boundary,
/// which is the property the capture side needs. If the analyzer ever falls
/// behind real time, the pipe fills and the *recorder* finds out by its write
/// returning `EAGAIN` — a condition it can handle by dropping transcription
/// audio. The alternative, buffering everything we can read, would turn a slow
/// analyzer into unbounded memory growth inside a four-hour meeting and would
/// hide the problem from the only process able to do something about it.
@available(macOS 26, *)
final class StdinFrames: AsyncSequence, AsyncIteratorProtocol, @unchecked Sendable {
    typealias Element = AnalyzerInput

    private let inputFormat: AVAudioFormat
    private let analyzerFormat: AVAudioFormat
    private let converter: AVAudioConverter
    private let source = FileHandle.standardInput
    /// Blocking reads belong on a thread of their own, not on one of the
    /// cooperative pool's — starving that pool would stall the analyzer we are
    /// feeding.
    private let readQueue = DispatchQueue(label: "ing.meet-ai.meet-stt.stdin")

    /// Input frames consumed so far, which is the recording's own timeline.
    ///
    /// Every timestamp handed to the analyzer is derived from this rather than
    /// from a clock, so a live transcript lines up with the WAV the recorder is
    /// writing in parallel. Wall-clock timestamps would drift by however long
    /// the model took to load and by however far behind real time we are.
    private(set) var framesConsumed: Int64 = 0
    private var finished = false

    init(inputFormat: AVAudioFormat, analyzerFormat: AVAudioFormat, converter: AVAudioConverter) {
        self.inputFormat = inputFormat
        self.analyzerFormat = analyzerFormat
        self.converter = converter
    }

    var secondsConsumed: Double { Double(framesConsumed) / liveSampleRate }

    func makeAsyncIterator() -> StdinFrames { self }

    func next() async throws -> AnalyzerInput? {
        while true {
            if finished { return nil }
            guard let data = await readChunk() else {
                finished = true
                return nil
            }
            // A partial frame at EOF is a torn write, not audio. Half a sample
            // played as a sample is a click, and a click is the kind of thing a
            // recognizer will happily turn into a word.
            let frames = data.count / MemoryLayout<Int16>.size
            guard frames > 0 else {
                finished = true
                return nil
            }

            let startTime = CMTime(
                value: framesConsumed, timescale: CMTimeScale(liveSampleRate))
            framesConsumed += Int64(frames)

            guard let converted = try convert(data, frames: frames) else { continue }
            return AnalyzerInput(buffer: converted, bufferStartTime: startTime)
        }
    }

    /// One chunk, or `nil` at EOF. Short reads are normal on a pipe — a writer
    /// producing 100 ms of audio does not produce it in one `write(2)` — so
    /// this accumulates until the chunk is full or the stream ends.
    private func readChunk() async -> Data? {
        await withCheckedContinuation { continuation in
            readQueue.async {
                let want = liveFramesPerChunk * MemoryLayout<Int16>.size
                var buffer = Data()
                buffer.reserveCapacity(want)
                while buffer.count < want {
                    guard
                        let piece = try? self.source.read(upToCount: want - buffer.count),
                        !piece.isEmpty
                    else { break }
                    buffer.append(piece)
                }
                continuation.resume(returning: buffer.isEmpty ? nil : buffer)
            }
        }
    }

    /// Raw little-endian i16 bytes -> whatever format the analyzer asked for.
    ///
    /// `bestAvailableAudioFormat` is not promised to be 16 kHz mono float, so
    /// this goes through `AVAudioConverter` rather than assuming. When the two
    /// formats already match, the converter is a copy.
    private func convert(_ data: Data, frames: Int) throws -> AVAudioPCMBuffer? {
        guard
            let input = AVAudioPCMBuffer(
                pcmFormat: inputFormat, frameCapacity: AVAudioFrameCount(frames))
        else { throw LiveError.allocationFailed }
        input.frameLength = AVAudioFrameCount(frames)
        guard let destination = input.int16ChannelData?[0] else {
            throw LiveError.allocationFailed
        }
        data.withUnsafeBytes { raw in
            if let base = raw.baseAddress {
                memcpy(destination, base, frames * MemoryLayout<Int16>.size)
            }
        }

        let ratio = analyzerFormat.sampleRate / inputFormat.sampleRate
        // The slack covers a resampler's filter delay, which can emit more than
        // the ratio alone predicts on the first buffers.
        let capacity = AVAudioFrameCount(Double(frames) * ratio) + 1_024
        guard
            let output = AVAudioPCMBuffer(pcmFormat: analyzerFormat, frameCapacity: capacity)
        else { throw LiveError.allocationFailed }

        var supplied = false
        var conversionError: NSError?
        let status = converter.convert(to: output, error: &conversionError) { _, outStatus in
            if supplied {
                outStatus.pointee = .noDataNow
                return nil
            }
            supplied = true
            outStatus.pointee = .haveData
            return input
        }
        if status == .error {
            throw LiveError.conversionFailed(conversionError?.localizedDescription ?? "unknown")
        }
        // A resampler can swallow the first chunk entirely while it primes.
        // That is not an error and not end-of-stream; ask for more.
        return output.frameLength > 0 ? output : nil
    }

    enum LiveError: Error, CustomStringConvertible {
        case allocationFailed
        case conversionFailed(String)

        var description: String {
            switch self {
            case .allocationFailed: "could not allocate a PCM buffer"
            case .conversionFailed(let reason): "audio conversion failed: \(reason)"
            }
        }
    }
}

@available(macOS 26, *)
func runTranscribeStdin(_ options: Options) async {
    let locale = await resolveEngine(options)
    let transcriber = makeTranscriber(locale: locale, reportVolatile: options.reportVolatile)
    await preflight(options, transcriber: transcriber, locale: locale)

    guard
        let analyzerFormat = await SpeechAnalyzer.bestAvailableAudioFormat(
            compatibleWith: [transcriber])
    else {
        fail(
            code: "no_analyzer_format",
            message: "SpeechAnalyzer offered no audio format for \(locale.identifier(.bcp47))")
    }
    guard
        let inputFormat = AVAudioFormat(
            commonFormat: .pcmFormatInt16, sampleRate: liveSampleRate, channels: 1,
            interleaved: true),
        let converter = AVAudioConverter(from: inputFormat, to: analyzerFormat)
    else {
        fail(
            code: "unsupported_input_format",
            message: "cannot convert 16 kHz mono i16 to \(analyzerFormat)")
    }

    let frames = StdinFrames(
        inputFormat: inputFormat, analyzerFormat: analyzerFormat, converter: converter)
    let analyzer = SpeechAnalyzer(modules: [transcriber])
    let drain = startDraining(transcriber)

    // The model is loaded and the pipe is being read from here on. The recorder
    // is already capturing by now — it must never wait on us — but the driver
    // needs one unambiguous "streaming started" edge to report, and an error
    // before this line means transcription never began at all.
    emit([
        "type": "ready",
        "locale": locale.identifier(.bcp47),
        "sample_rate": liveSampleRate,
        "analyzer_sample_rate": analyzerFormat.sampleRate,
    ])

    do {
        _ = try await analyzer.analyzeSequence(frames)
        try await analyzer.finalizeAndFinishThroughEndOfInput()
    } catch {
        drain.cancel()
        _ = await AssetInventory.release(reservedLocale: locale)
        fail(code: "analysis_failed", message: "\(error)")
    }

    await drain.value
    _ = await AssetInventory.release(reservedLocale: locale)
    emit(["type": "done", "duration_sec": frames.secondsConsumed])
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
} else if options.useStdin {
    guard options.wavPath == nil else {
        fail(code: "bad_arguments", message: "--stdin and a WAV path are mutually exclusive")
    }
    await runTranscribeStdin(options)
} else {
    await runTranscribe(options)
}
