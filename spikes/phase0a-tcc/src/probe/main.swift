import AVFoundation
import AudioToolbox
import CoreAudio
import Foundation

// ---------------------------------------------------------------------------
// meet-tap-probe — Phase 0a helper (SPEC §5, TUR-3)
//
// Answers exactly one question and refuses to guess at it: after a Core Audio
// process tap is created inside a signed bundle, *what is actually in the
// buffers*? An OSStatus of 0 proves nothing here — the documented TCC failure
// mode is a granted-looking tap that delivers digital silence. So every number
// this binary prints is measured from the samples: frame counts, RMS, peak, the
// count of bit-exact-zero samples, and host timestamps of the first and last
// buffer.
// ---------------------------------------------------------------------------

// MARK: - small helpers

func fourCC(_ status: OSStatus) -> String {
    let n = UInt32(bitPattern: status)
    let bytes = [UInt8((n >> 24) & 0xFF), UInt8((n >> 16) & 0xFF), UInt8((n >> 8) & 0xFF), UInt8(n & 0xFF)]
    let printable = bytes.allSatisfy { $0 >= 0x20 && $0 < 0x7F }
    return printable ? "'\(String(bytes: bytes, encoding: .ascii) ?? "")' (\(status))" : "\(status)"
}

func log(_ msg: String) {
    let ts = ISO8601DateFormatter().string(from: Date())
    FileHandle.standardError.write(Data("[\(ts)] \(msg)\n".utf8))
    print("[\(ts)] \(msg)")
    fflush(stdout)
}

struct ProbeError: Error, CustomStringConvertible {
    let description: String
    init(_ d: String) { description = d }
}

func check(_ status: OSStatus, _ what: String) throws {
    guard status == noErr else { throw ProbeError("\(what) failed: \(fourCC(status))") }
}

// MARK: - measurement

/// Accumulates the only evidence that counts: what the samples contain.
final class Meter: @unchecked Sendable {
    private let lock = NSLock()

    private(set) var frames: UInt64 = 0
    private(set) var callbacks: UInt64 = 0
    private(set) var sumSquares: Double = 0
    private(set) var peak: Float = 0
    private(set) var zeroSamples: UInt64 = 0
    private(set) var totalSamples: UInt64 = 0
    private(set) var firstHostNs: UInt64 = 0
    private(set) var lastHostNs: UInt64 = 0

    /// Per-second RMS so a partial signal (e.g. tap goes silent after 3s) is visible
    /// rather than averaged away.
    private(set) var perSecondRms: [Double] = []
    private var bucketSumSquares: Double = 0
    private var bucketSamples: UInt64 = 0
    private let samplesPerBucket: UInt64

    init(sampleRate: Double, channels: UInt32) {
        samplesPerBucket = UInt64(max(1, sampleRate)) * UInt64(max(1, channels))
    }

    func ingest(_ samples: UnsafeBufferPointer<Float>, frames n: UInt64, hostNs: UInt64) {
        lock.lock()
        defer { lock.unlock() }
        callbacks &+= 1
        frames &+= n
        if firstHostNs == 0 { firstHostNs = hostNs }
        lastHostNs = hostNs

        for s in samples {
            let d = Double(s)
            sumSquares += d * d
            bucketSumSquares += d * d
            if s == 0 { zeroSamples &+= 1 }
            let a = abs(s)
            if a > peak { peak = a }
        }
        totalSamples &+= UInt64(samples.count)
        bucketSamples &+= UInt64(samples.count)
        while bucketSamples >= samplesPerBucket {
            perSecondRms.append((bucketSumSquares / Double(samplesPerBucket)).squareRoot())
            bucketSumSquares = 0
            bucketSamples -= samplesPerBucket
        }
    }

    var rms: Double {
        lock.lock(); defer { lock.unlock() }
        return totalSamples == 0 ? 0 : (sumSquares / Double(totalSamples)).squareRoot()
    }

    func snapshot() -> [String: Any] {
        lock.lock(); defer { lock.unlock() }
        let r = totalSamples == 0 ? 0 : (sumSquares / Double(totalSamples)).squareRoot()
        let dbfs = r > 0 ? 20 * log10(r) : -Double.infinity
        let peakDb = peak > 0 ? 20 * log10(Double(peak)) : -Double.infinity
        return [
            "io_callbacks": callbacks,
            "frames": frames,
            "samples": totalSamples,
            "rms": r,
            "rms_dbfs": dbfs.isFinite ? dbfs : -999.0,
            "peak": Double(peak),
            "peak_dbfs": peakDb.isFinite ? peakDb : -999.0,
            "bit_exact_zero_samples": zeroSamples,
            "zero_sample_fraction": totalSamples == 0 ? 1.0 : Double(zeroSamples) / Double(totalSamples),
            "first_buffer_host_ns": firstHostNs,
            "last_buffer_host_ns": lastHostNs,
            "span_ms": firstHostNs == 0 ? 0 : Double(lastHostNs - firstHostNs) / 1e6,
            "per_second_rms": perSecondRms.map { $0 },
        ]
    }
}

// MARK: - Core Audio plumbing

func audioObjectProperty<T>(_ objectID: AudioObjectID, _ selector: AudioObjectPropertySelector,
                            scope: AudioObjectPropertyScope = kAudioObjectPropertyScopeGlobal,
                            _ initial: T) throws -> T {
    var addr = AudioObjectPropertyAddress(mSelector: selector, mScope: scope,
                                          mElement: kAudioObjectPropertyElementMain)
    var size = UInt32(MemoryLayout<T>.size)
    var value = initial
    // Explicit typed pointer rather than `&value`: passing an inout of a generic
    // to a raw-pointer parameter makes the compiler (rightly) warn that T could
    // hold an object reference.
    return try withUnsafeMutablePointer(to: &value) { ptr in
        try check(AudioObjectGetPropertyData(objectID, &addr, 0, nil, &size, ptr),
                  "AudioObjectGetPropertyData(\(fourCC(OSStatus(bitPattern: selector))))")
        return ptr.pointee
    }
}

func deviceUID(_ deviceID: AudioObjectID) throws -> String {
    var addr = AudioObjectPropertyAddress(mSelector: kAudioDevicePropertyDeviceUID,
                                          mScope: kAudioObjectPropertyScopeGlobal,
                                          mElement: kAudioObjectPropertyElementMain)
    var size = UInt32(MemoryLayout<CFString?>.size)
    var uid: CFString? = nil
    try withUnsafeMutablePointer(to: &uid) { ptr in
        try check(AudioObjectGetPropertyData(deviceID, &addr, 0, nil, &size, ptr),
                  "kAudioDevicePropertyDeviceUID")
    }
    guard let uid = uid as String? else { throw ProbeError("device \(deviceID) has no UID") }
    return uid
}

/// Everything the system tap needs, kept together so teardown is ordered.
final class SystemTapRecorder {
    private var tapID = AudioObjectID(kAudioObjectUnknown)
    private var aggregateID = AudioObjectID(kAudioObjectUnknown)
    private var ioProcID: AudioDeviceIOProcID?
    private var writer: WavWriter?
    private var meter: Meter?
    private var interleaveScratch = [Float]()

    private(set) var createTapStatus: OSStatus = -1
    private(set) var format = AudioStreamBasicDescription()
    private(set) var outputDeviceUID = ""
    private(set) var tapUUID = ""

    /// Wall-clock cost of each Core Audio step, in milliseconds.
    /// `AudioHardwareCreateProcessTap` is where a TCC prompt would block, so a
    /// multi-second value here is the signature of "a human clicked a dialog"
    /// and a sub-millisecond one is the signature of "no dialog was shown".
    private(set) var timings: [String: Double] = [:]
    private(set) var captureStartedAt: Date?

    private func timed<T>(_ label: String, _ body: () throws -> T) rethrows -> T {
        let t0 = DispatchTime.now().uptimeNanoseconds
        defer { timings[label] = Double(DispatchTime.now().uptimeNanoseconds - t0) / 1e6 }
        return try body()
    }

    func start(outURL: URL, tapAutoStart: Bool) throws {
        // 1. Default output device — the tap rides alongside it in an aggregate.
        let outDev: AudioObjectID = try audioObjectProperty(AudioObjectID(kAudioObjectSystemObject),
                                                            kAudioHardwarePropertyDefaultOutputDevice,
                                                            AudioObjectID(0))
        outputDeviceUID = try deviceUID(outDev)
        log("default output device: id=\(outDev) uid=\(outputDeviceUID)")

        // 2. Global stereo tap, excluding nothing == everything the machine plays.
        let desc = CATapDescription(stereoGlobalTapButExcludeProcesses: [])
        desc.name = "meet-ai phase0a probe"
        desc.uuid = UUID()
        desc.isPrivate = true
        desc.muteBehavior = .unmuted
        tapUUID = desc.uuid.uuidString

        // THE call under test. Note: a noErr here is explicitly NOT treated as success.
        createTapStatus = timed("create_process_tap_ms") { AudioHardwareCreateProcessTap(desc, &tapID) }
        log("AudioHardwareCreateProcessTap -> \(fourCC(createTapStatus)), tapID=\(tapID), " +
            "took \(String(format: "%.3f", timings["create_process_tap_ms"] ?? -1))ms")
        try check(createTapStatus, "AudioHardwareCreateProcessTap")
        guard tapID != kAudioObjectUnknown else { throw ProbeError("tap object id is kAudioObjectUnknown") }

        format = try timed("read_tap_format_ms") {
            try audioObjectProperty(tapID, kAudioTapPropertyFormat, AudioStreamBasicDescription())
        }
        log("tap format: \(format.mSampleRate)Hz ch=\(format.mChannelsPerFrame) " +
            "bits=\(format.mBitsPerChannel) flags=\(format.mFormatFlags) bytesPerFrame=\(format.mBytesPerFrame)")
        guard format.mChannelsPerFrame > 0, format.mSampleRate > 0 else {
            throw ProbeError("tap reports a degenerate format")
        }

        // 3. Private aggregate device carrying the tap.
        let aggUID = UUID().uuidString
        let composition: [String: Any] = [
            kAudioAggregateDeviceNameKey: "meet-ai phase0a aggregate",
            kAudioAggregateDeviceUIDKey: aggUID,
            kAudioAggregateDeviceMainSubDeviceKey: outputDeviceUID,
            kAudioAggregateDeviceIsPrivateKey: true,
            kAudioAggregateDeviceIsStackedKey: false,
            // Default OFF. With this on, AudioDeviceStart blocks until some tapped
            // process actually produces audio — which for a meeting recorder means
            // "recording does not start until someone speaks", and a first-buffer
            // host time that no longer marks the true start of the segment.
            kAudioAggregateDeviceTapAutoStartKey: tapAutoStart,
            kAudioAggregateDeviceSubDeviceListKey: [[kAudioSubDeviceUIDKey: outputDeviceUID]],
            kAudioAggregateDeviceTapListKey: [[
                kAudioSubTapDriftCompensationKey: true,
                kAudioSubTapUIDKey: tapUUID,
            ]],
        ]
        try check(timed("create_aggregate_ms") {
            AudioHardwareCreateAggregateDevice(composition as CFDictionary, &aggregateID)
        }, "AudioHardwareCreateAggregateDevice")
        log("aggregate device id=\(aggregateID) uid=\(aggUID)")

        // 4. WAV + meter, then the IO proc.
        let channels = format.mChannelsPerFrame
        let meter = Meter(sampleRate: format.mSampleRate, channels: channels)
        let writer = try WavWriter(url: outURL, sampleRate: format.mSampleRate, channels: channels)
        self.meter = meter
        self.writer = writer
        interleaveScratch.reserveCapacity(8192 * Int(channels))

        var err: Error?
        let status = timed("create_ioproc_ms") {
            AudioDeviceCreateIOProcIDWithBlock(&ioProcID, aggregateID, nil) {
                [weak self] _, inInputData, inInputTime, _, _ in
                guard let self else { return }
                do {
                    try self.handle(inInputData, inInputTime, channels: channels, meter: meter, writer: writer)
                } catch {
                    if err == nil { err = error }
                }
            }
        }
        log("AudioDeviceCreateIOProcIDWithBlock -> \(fourCC(status)), took " +
            "\(String(format: "%.3f", timings["create_ioproc_ms"] ?? -1))ms")
        try check(status, "AudioDeviceCreateIOProcIDWithBlock")
        try check(timed("device_start_ms") { AudioDeviceStart(aggregateID, ioProcID) }, "AudioDeviceStart")
        captureStartedAt = Date()
        log("capture started (AudioDeviceStart took " +
            "\(String(format: "%.3f", timings["device_start_ms"] ?? -1))ms)")
    }

    private func handle(_ bufferList: UnsafePointer<AudioBufferList>,
                        _ time: UnsafePointer<AudioTimeStamp>,
                        channels: UInt32, meter: Meter, writer: WavWriter) throws {
        let hostNs = AudioConvertHostTimeToNanos(time.pointee.mHostTime)
        let abl = UnsafeMutableAudioBufferListPointer(UnsafeMutablePointer(mutating: bufferList))
        guard abl.count > 0 else { return }

        if abl.count == 1 {
            // Already interleaved (or mono).
            let buf = abl[0]
            guard let data = buf.mData else { return }
            let count = Int(buf.mDataByteSize) / MemoryLayout<Float>.size
            let ptr = data.assumingMemoryBound(to: Float.self)
            let samples = UnsafeBufferPointer(start: ptr, count: count)
            let frames = UInt64(count / Int(max(1, buf.mNumberChannels)))
            meter.ingest(samples, frames: frames, hostNs: hostNs)
            try writer.append(samples)
            return
        }

        // Deinterleaved planar: one buffer per channel. Interleave for the WAV.
        let framesPerBuffer = Int(abl[0].mDataByteSize) / MemoryLayout<Float>.size
        let ch = min(Int(channels), abl.count)
        if interleaveScratch.count < framesPerBuffer * ch {
            interleaveScratch = [Float](repeating: 0, count: framesPerBuffer * ch)
        }
        interleaveScratch.withUnsafeMutableBufferPointer { out in
            for c in 0..<ch {
                guard let data = abl[c].mData else { continue }
                let src = data.assumingMemoryBound(to: Float.self)
                for f in 0..<framesPerBuffer { out[f * ch + c] = src[f] }
            }
        }
        try interleaveScratch.withUnsafeBufferPointer { full in
            let slice = UnsafeBufferPointer(start: full.baseAddress, count: framesPerBuffer * ch)
            meter.ingest(slice, frames: UInt64(framesPerBuffer), hostNs: hostNs)
            try writer.append(slice)
        }
    }

    func stop() {
        if aggregateID != kAudioObjectUnknown, let ioProcID {
            AudioDeviceStop(aggregateID, ioProcID)
            AudioDeviceDestroyIOProcID(aggregateID, ioProcID)
        }
        if aggregateID != kAudioObjectUnknown { AudioHardwareDestroyAggregateDevice(aggregateID) }
        if tapID != kAudioObjectUnknown { AudioHardwareDestroyProcessTap(tapID) }
        try? writer?.close()
    }

    func report() -> [String: Any] {
        var r: [String: Any] = [
            "create_tap_osstatus": Int(createTapStatus),
            "create_tap_osstatus_fourcc": fourCC(createTapStatus),
            "tap_uuid": tapUUID,
            "output_device_uid": outputDeviceUID,
            "format_sample_rate": format.mSampleRate,
            "format_channels": Int(format.mChannelsPerFrame),
            "format_bits_per_channel": Int(format.mBitsPerChannel),
            "timings_ms": timings,
            // Anchors the per-second RMS timeline to wall clock, so it can be
            // lined up against tccd's AUTHREQ_PROMPTING / TCCDEvent timestamps.
            "capture_started_at": captureStartedAt.map { ISO8601DateFormatter().string(from: $0) } ?? "",
        ]
        if let meter { r["measured"] = meter.snapshot() }
        return r
    }
}

// MARK: - microphone (second TCC service, same bundle)

final class MicRecorder {
    private let engine = AVAudioEngine()
    private var writer: WavWriter?
    private var meter: Meter?
    private(set) var authorization = "not-requested"
    private(set) var startError: String?

    func requestAccess(timeout: TimeInterval = 30) {
        let status = AVCaptureDevice.authorizationStatus(for: .audio)
        switch status {
        case .authorized: authorization = "authorized"; return
        case .restricted: authorization = "restricted"; return
        case .denied: authorization = "denied"; return
        case .notDetermined: authorization = "not-determined"
        @unknown default: authorization = "unknown"
        }
        let sem = DispatchSemaphore(value: 0)
        AVCaptureDevice.requestAccess(for: .audio) { granted in
            self.authorization = granted ? "authorized" : "denied"
            sem.signal()
        }
        _ = sem.wait(timeout: .now() + timeout)
    }

    func start(outURL: URL) {
        do {
            let input = engine.inputNode
            let fmt = input.outputFormat(forBus: 0)
            guard fmt.sampleRate > 0, fmt.channelCount > 0 else {
                startError = "input node reports \(fmt.sampleRate)Hz ch=\(fmt.channelCount)"
                return
            }
            let meter = Meter(sampleRate: fmt.sampleRate, channels: fmt.channelCount)
            let writer = try WavWriter(url: outURL, sampleRate: fmt.sampleRate, channels: fmt.channelCount)
            self.meter = meter
            self.writer = writer
            log("mic format: \(fmt.sampleRate)Hz ch=\(fmt.channelCount)")

            input.installTap(onBus: 0, bufferSize: 4096, format: fmt) { buffer, when in
                guard let chans = buffer.floatChannelData else { return }
                let frames = Int(buffer.frameLength)
                let ch = Int(buffer.format.channelCount)
                var interleaved = [Float](repeating: 0, count: frames * ch)
                for c in 0..<ch {
                    let src = chans[c]
                    for f in 0..<frames { interleaved[f * ch + c] = src[f] }
                }
                let hostNs = AudioConvertHostTimeToNanos(when.hostTime)
                interleaved.withUnsafeBufferPointer { buf in
                    meter.ingest(buf, frames: UInt64(frames), hostNs: hostNs)
                    try? writer.append(buf)
                }
            }
            try engine.start()
            log("mic capture started")
        } catch {
            startError = "\(error)"
            log("mic start failed: \(error)")
        }
    }

    func stop() {
        if engine.isRunning {
            engine.inputNode.removeTap(onBus: 0)
            engine.stop()
        }
        try? writer?.close()
    }

    func report() -> [String: Any] {
        var r: [String: Any] = ["authorization": authorization]
        if let startError { r["start_error"] = startError }
        if let meter { r["measured"] = meter.snapshot() }
        return r
    }
}

// MARK: - main

func parseArgs() -> (out: URL, seconds: Double, mic: Bool, tapAutoStart: Bool) {
    var out = FileManager.default.temporaryDirectory.appendingPathComponent("meet-ai-phase0a")
    var seconds = 12.0
    var mic = false
    var tapAutoStart = false
    var it = CommandLine.arguments.dropFirst().makeIterator()
    while let a = it.next() {
        switch a {
        case "--out": if let v = it.next() { out = URL(fileURLWithPath: v) }
        case "--seconds": if let v = it.next(), let d = Double(v) { seconds = d }
        case "--mic": mic = true
        case "--tap-autostart": tapAutoStart = true
        default: break
        }
    }
    return (out, seconds, mic, tapAutoStart)
}

let args = parseArgs()
try? FileManager.default.createDirectory(at: args.out, withIntermediateDirectories: true)

log("meet-tap-probe starting")
log("pid=\(getpid()) ppid=\(getppid()) exe=\(CommandLine.arguments[0])")
log("out=\(args.out.path) seconds=\(args.seconds) mic=\(args.mic) tapAutoStart=\(args.tapAutoStart)")

var result: [String: Any] = [
    "probe_version": 1,
    "pid": getpid(),
    "ppid": getppid(),
    "executable": CommandLine.arguments[0],
    "started_at": ISO8601DateFormatter().string(from: Date()),
    "requested_seconds": args.seconds,
    "tap_auto_start": args.tapAutoStart,
]

let mic = MicRecorder()
if args.mic {
    mic.requestAccess()
    log("microphone authorization: \(mic.authorization)")
    mic.start(outURL: args.out.appendingPathComponent("mic.wav"))
}

let tap = SystemTapRecorder()
var tapFailure: String?
do {
    try tap.start(outURL: args.out.appendingPathComponent("system.wav"), tapAutoStart: args.tapAutoStart)
} catch {
    tapFailure = "\(error)"
    log("system tap failed: \(error)")
}

Thread.sleep(forTimeInterval: args.seconds)

tap.stop()
if args.mic { mic.stop() }

result["system_tap"] = tap.report()
if let tapFailure { result["system_tap_error"] = tapFailure }
if args.mic { result["microphone"] = mic.report() }
result["finished_at"] = ISO8601DateFormatter().string(from: Date())

let resultURL = args.out.appendingPathComponent("probe-result.json")
let json = try JSONSerialization.data(withJSONObject: result, options: [.prettyPrinted, .sortedKeys])
try json.write(to: resultURL)
log("wrote \(resultURL.path)")
print(String(data: json, encoding: .utf8) ?? "{}")
fflush(stdout)
exit(tapFailure == nil ? 0 : 2)
