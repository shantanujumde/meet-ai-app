import Foundation

/// Minimal incremental 32-bit-float WAV writer.
///
/// Phase 0a only needs "produce a file something else can measure", but the
/// header-patching cadence is deliberate: it is the same crash-safety shape the
/// real `meet-rec` needs (SPEC §2.3, `hound`, header flushed every 5s). Sizes are
/// re-patched every `flushInterval` seconds, so a `kill -9` leaves a playable
/// file that is at most `flushInterval` short, never a truncated header.
final class WavWriter {
    private let handle: FileHandle
    private let channels: UInt32
    private let sampleRate: UInt32
    private var dataBytes: UInt32 = 0
    private var bytesSinceFlush: UInt32 = 0
    private let flushEveryBytes: UInt32

    let url: URL

    init(url: URL, sampleRate: Double, channels: UInt32, flushInterval: Double = 1.0) throws {
        self.url = url
        self.channels = channels
        self.sampleRate = UInt32(sampleRate.rounded())
        self.flushEveryBytes = UInt32(flushInterval * sampleRate * Double(channels) * 4)

        FileManager.default.createFile(atPath: url.path, contents: nil)
        self.handle = try FileHandle(forWritingTo: url)
        try handle.write(contentsOf: Self.header(sampleRate: self.sampleRate, channels: channels, dataBytes: 0))
    }

    /// Interleaved float32 frames.
    func append(_ samples: UnsafeBufferPointer<Float>) throws {
        guard !samples.isEmpty else { return }
        let bytes = Data(buffer: samples)
        try handle.write(contentsOf: bytes)
        dataBytes &+= UInt32(bytes.count)
        bytesSinceFlush &+= UInt32(bytes.count)
        if bytesSinceFlush >= flushEveryBytes {
            try patchSizes()
            bytesSinceFlush = 0
        }
    }

    func close() throws {
        try patchSizes()
        try handle.close()
    }

    /// Rewrites the two length fields in place, then seeks back to the tail.
    private func patchSizes() throws {
        let end = try handle.offset()
        try handle.seek(toOffset: 4)
        try handle.write(contentsOf: le32(36 &+ dataBytes))
        try handle.seek(toOffset: 40)
        try handle.write(contentsOf: le32(dataBytes))
        try handle.seek(toOffset: end)
        try handle.synchronize()
    }

    private func le32(_ v: UInt32) -> Data {
        var x = v.littleEndian
        return Data(bytes: &x, count: 4)
    }

    private static func header(sampleRate: UInt32, channels: UInt32, dataBytes: UInt32) -> Data {
        var d = Data()
        func u32(_ v: UInt32) { var x = v.littleEndian; d.append(Data(bytes: &x, count: 4)) }
        func u16(_ v: UInt16) { var x = v.littleEndian; d.append(Data(bytes: &x, count: 2)) }
        let bytesPerFrame = channels * 4

        d.append(contentsOf: Array("RIFF".utf8))
        u32(36 &+ dataBytes)
        d.append(contentsOf: Array("WAVE".utf8))
        d.append(contentsOf: Array("fmt ".utf8))
        u32(16)
        u16(3)                                  // WAVE_FORMAT_IEEE_FLOAT
        u16(UInt16(channels))
        u32(sampleRate)
        u32(sampleRate * bytesPerFrame)         // byte rate
        u16(UInt16(bytesPerFrame))              // block align
        u16(32)                                 // bits per sample
        d.append(contentsOf: Array("data".utf8))
        u32(dataBytes)
        return d
    }
}
