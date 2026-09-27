//! The Core Audio system-audio process tap — the `System` half of
//! [`AudioSource`], alongside [`crate::mic::MicSource`].
//!
//! Ports `spikes/phase0a-tcc/src/probe/main.swift`'s `SystemTapRecorder` to
//! Rust via `objc2-core-audio`. Same worker-thread → resampler → [`WavWriter`]
//! shape as [`crate::mic::MicSource`]; the genuinely new part is building the
//! tap and its private aggregate device through Core Audio's C API instead of
//! `cpal`, and de-interleaving the tap's `AudioBufferList` by hand.
//!
//! **Verified against real hardware (TUR-4).** The permission chime, played
//! through the default output device, is recovered from a real `system.wav`
//! recorded through this tap/aggregate-device pipeline —
//! `crates/audio/tests/system_closed_loop.rs`, `cargo test -p audio --test
//! system_closed_loop -- --ignored --nocapture`. Passed twice independently:
//! 60032 frames / RMS 0.184 / both chime notes detected, and 59690 frames /
//! RMS 0.136 / both notes detected on a second run. That first live run also
//! caught a real bug this module shipped with: `NSUUID::from_bytes` (via
//! `objc2-foundation` 0.3.2's `initWithUUIDBytes:`) panics at the
//! Objective-C message-send boundary on real hardware — the crate's own docs
//! say it needs `disable-encoding-assertions` to be safe to call, which
//! wasn't enabled. Fixed by building the UUID from a formatted string via
//! `initWithUUIDString:` instead ([`format_uuid_bytes`]), which never goes
//! through the mismatched-encoding method. `interleave_into` — the one piece
//! of genuine logic that doesn't touch Core Audio — is additionally
//! unit-tested against synthetic channel data.

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::Duration;

use block2::RcBlock;
use objc2::AnyThread;
use objc2::rc::Retained;
use objc2_core_audio::{
    self as ca, AudioDeviceIOProcID, AudioHardwareCreateAggregateDevice,
    AudioHardwareCreateProcessTap, AudioHardwareDestroyAggregateDevice,
    AudioHardwareDestroyProcessTap, AudioObjectID, AudioObjectPropertyAddress, CATapDescription,
    CATapMuteBehavior,
};
use objc2_core_audio_types::{AudioBufferList, AudioStreamBasicDescription, AudioTimeStamp};
use objc2_core_foundation::{CFArray, CFBoolean, CFDictionary, CFRetained, CFString, CFType};
use objc2_foundation::{NSArray, NSNumber, NSString, NSUUID};
use ringbuf::traits::{Consumer, Producer, Split};
use ringbuf::{HeapCons, HeapRb};

use crate::resample::{Resampler, downmix_to_mono};
use crate::wav_writer::WavWriter;
use crate::{AudioSource, Channel, Error};

/// Ring buffer capacity, in raw (tap-rate, interleaved) samples. Same 4 s
/// oversizing rationale as `crate::mic::RING_CAPACITY_SAMPLES`: generous
/// against a worker thread serviced every couple of milliseconds, so a
/// scheduling hiccup drops nothing rather than build steady-state backlog.
const RING_CAPACITY_SAMPLES: usize = 48_000 * 2 * 4;

/// How long the worker thread sleeps when the ring buffer is empty, between
/// polls. Matches `crate::mic::IDLE_POLL` for the same reason: short enough
/// not to become the dominant term against the 200 ms drift gate, long
/// enough not to spin a core.
const IDLE_POLL: Duration = Duration::from_millis(2);

fn f32_to_i16(sample: f32) -> i16 {
    (sample.clamp(-1.0, 1.0) * i16::MAX as f32).round() as i16
}

/// De-interleaves a tap's planar `AudioBufferList` — one buffer per channel —
/// into a single interleaved buffer, matching the Swift probe's
/// `interleaveScratch` loop. Pure and independent of Core Audio, so it is the
/// one piece of this module's logic that can be verified without a live tap:
/// wrong channel order or off-by-one frame counts here would silently swap
/// or corrupt system audio on every real recording.
///
/// Writes into `out` (cleared and resized as needed) rather than returning a
/// fresh `Vec`, so the IO callback can call this every cycle without
/// allocating once `out`'s capacity has grown to steady state (SPEC §2.3).
///
/// `channels` must all report the same frame count; the shortest is used if
/// they don't, matching the tap's own guarantee that every buffer in one
/// `AudioBufferList` covers the same IO cycle.
fn interleave_into(channels: &[Vec<f32>], out: &mut Vec<f32>) {
    out.clear();
    if channels.is_empty() {
        return;
    }
    if channels.len() == 1 {
        out.extend_from_slice(&channels[0]);
        return;
    }
    let frames = channels.iter().map(|c| c.len()).min().unwrap_or(0);
    out.resize(frames * channels.len(), 0.0);
    for (c, channel) in channels.iter().enumerate() {
        for f in 0..frames {
            out[f * channels.len() + c] = channel[f];
        }
    }
}

/// Not cryptographically random — only needs to be unique among this
/// process's own private, per-recording taps, which a mix of wall-clock
/// nanoseconds and a process-wide counter comfortably provides. Avoids
/// pulling in a `uuid`/`rand` dependency for the one field
/// `CATapDescription` needs a fresh value in.
fn locally_unique_uuid_bytes() -> [u8; 16] {
    use std::sync::atomic::{AtomicU64 as Counter, Ordering as CounterOrdering};
    static COUNTER: Counter = Counter::new(0);
    let counter = COUNTER.fetch_add(1, CounterOrdering::Relaxed);
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let pid = std::process::id() as u128;
    let mixed = nanos ^ ((pid as u128) << 64) ^ (counter as u128);
    mixed.to_le_bytes()
}

/// Formats 16 bytes as a canonical `8-4-4-4-12` hex UUID string.
///
/// `objc2-foundation` 0.3.2's `NSUUID::from_bytes`/`initWithUUIDBytes:` is
/// documented by the crate itself as requiring the `disable-encoding-
/// assertions` feature to use at all: `__NSConcreteUUID`'s real method
/// signature takes a `char*`, not the inline 16-byte array the public
/// headers claim, so calling it with encoding assertions on panics at the
/// Objective-C message-send boundary — confirmed by reproducing it directly
/// against a live tap (TUR-4). Going through `initWithUUIDString:` instead
/// (via [`NSUUID::from_string`]) sidesteps that mismatched-encoding method
/// entirely rather than weakening encoding verification crate-wide for one
/// call site.
fn format_uuid_bytes(bytes: [u8; 16]) -> String {
    format!(
        "{:02X}{:02X}{:02X}{:02X}-{:02X}{:02X}-{:02X}{:02X}-{:02X}{:02X}-{:02X}{:02X}{:02X}{:02X}{:02X}{:02X}",
        bytes[0],
        bytes[1],
        bytes[2],
        bytes[3],
        bytes[4],
        bytes[5],
        bytes[6],
        bytes[7],
        bytes[8],
        bytes[9],
        bytes[10],
        bytes[11],
        bytes[12],
        bytes[13],
        bytes[14],
        bytes[15],
    )
}

/// Builds a `CFDictionary<CFString, CFType>` from `&str` keys (Core Audio's
/// aggregate-device keys are all `&'static CStr`) and already-boxed
/// `CFType`-compatible values.
fn cf_dict(
    pairs: &[(&std::ffi::CStr, CFRetained<CFType>)],
) -> CFRetained<CFDictionary<CFString, CFType>> {
    let keys: Vec<CFRetained<CFString>> = pairs
        .iter()
        .map(|(k, _)| CFString::from_str(k.to_str().expect("aggregate device keys are ASCII")))
        .collect();
    let key_refs: Vec<&CFString> = keys.iter().map(|k| &**k).collect();
    let val_refs: Vec<&CFType> = pairs.iter().map(|(_, v)| &**v).collect();
    CFDictionary::from_slices(&key_refs, &val_refs)
}

fn cf_string_type(s: &str) -> CFRetained<CFType> {
    unsafe { CFRetained::cast_unchecked(CFString::from_str(s)) }
}

fn cf_bool_type(b: bool) -> CFRetained<CFType> {
    unsafe {
        CFRetained::cast_unchecked(CFRetained::retain(std::ptr::NonNull::from(CFBoolean::new(
            b,
        ))))
    }
}

/// # Safety
/// `object_id` must name a live `AudioObject`, and `T` must match the
/// property's actual C layout (`AudioObjectID` for a device-id property,
/// `AudioStreamBasicDescription` for a format property, etc — exactly like
/// the Swift probe's generic `audioObjectProperty<T>`).
unsafe fn get_property<T: Copy>(object_id: AudioObjectID, selector: u32) -> Result<T, Error> {
    let mut address = AudioObjectPropertyAddress {
        mSelector: selector,
        mScope: ca::kAudioObjectPropertyScopeGlobal,
        mElement: ca::kAudioObjectPropertyElementMain,
    };
    let mut size = std::mem::size_of::<T>() as u32;
    let mut value = std::mem::MaybeUninit::<T>::uninit();
    let status = unsafe {
        ca::AudioObjectGetPropertyData(
            object_id,
            std::ptr::NonNull::from(&mut address),
            0,
            std::ptr::null(),
            std::ptr::NonNull::from(&mut size),
            std::ptr::NonNull::new(value.as_mut_ptr().cast()).expect("stack pointer is non-null"),
        )
    };
    if status != 0 {
        return Err(Error::NoDevice(format!(
            "AudioObjectGetPropertyData(0x{selector:08x}) on object {object_id} failed: OSStatus {status}"
        )));
    }
    // SAFETY: a `noErr` status guarantees Core Audio wrote a full `T`.
    Ok(unsafe { value.assume_init() })
}

fn output_device_uid(device_id: AudioObjectID) -> Result<String, Error> {
    // SAFETY: `kAudioDevicePropertyDeviceUID` returns a `CFStringRef` the
    // caller owns (the "Copy" naming convention) — read as a raw pointer,
    // then immediately wrapped so it releases on drop rather than leaking.
    let raw: *mut CFString = unsafe { get_property(device_id, ca::kAudioDevicePropertyDeviceUID)? };
    let ptr = std::ptr::NonNull::new(raw)
        .ok_or_else(|| Error::NoDevice("device has no UID".to_string()))?;
    let uid: CFRetained<CFString> = unsafe { CFRetained::from_raw(ptr) };
    Ok(uid.to_string())
}

struct Shared {
    writer: WavWriter,
    frames: u64,
    last_host_ns: u64,
}

/// The signature Core Audio's `AudioDeviceIOBlock` requires: `(inNow,
/// inInputData, inInputTime, outOutputData, outOutputTime)`, none of them
/// `Option` since the block form always receives live pointers.
type IoBlockFn = dyn Fn(
    std::ptr::NonNull<AudioTimeStamp>,
    std::ptr::NonNull<AudioBufferList>,
    std::ptr::NonNull<AudioTimeStamp>,
    std::ptr::NonNull<AudioBufferList>,
    std::ptr::NonNull<AudioTimeStamp>,
);

/// Everything alive between a successful `build()` and `stop()`. Torn down in
/// the mirror-image order of construction: IO proc → aggregate device → tap.
struct Built {
    tap_id: AudioObjectID,
    aggregate_id: AudioObjectID,
    io_proc_id: AudioDeviceIOProcID,
    /// Kept alive for the tap's lifetime even though Core Audio `Block_copy`s
    /// its own reference on `AudioDeviceCreateIOProcIDWithBlock` — belt and
    /// braces against relying on an internal-only guarantee.
    _io_block: RcBlock<IoBlockFn>,
    worker: JoinHandle<()>,
    running: Arc<AtomicBool>,
    shared: Arc<Mutex<Shared>>,
}

// SAFETY: `RcBlock` is only non-`Send` because it wraps a raw `NonNull`
// pointer to a heap-allocated Objective-C block; the block itself is never
// invoked concurrently by our own code (Core Audio calls it on its own
// internal IO thread, serialized per its own documented contract), and we
// only ever touch `_io_block` from whichever single thread happens to own
// `Built` at the time (construction, then `stop`'s drop) — never from two
// threads at once. `AudioSource: Send` requires this so `Built` can cross
// from the init thread (`SystemSource::start`) to the caller.
unsafe impl Send for Built {}

/// The system-audio capture channel: a private Core Audio process tap riding
/// an aggregate device built around the default output device, feeding a
/// resampler and a [`WavWriter`] through a lock-free ring buffer — same
/// real-time discipline as `MicSource`: the IO block only timestamps and
/// pushes samples, never locks, allocates, or touches disk.
pub struct SystemSource {
    built: Option<Built>,
}

impl Default for SystemSource {
    fn default() -> Self {
        Self::new()
    }
}

impl SystemSource {
    pub fn new() -> Self {
        Self { built: None }
    }

    fn worker_loop(
        mut consumer: HeapCons<f32>,
        channels: usize,
        device_rate: u32,
        shared: Arc<Mutex<Shared>>,
        last_cb_host_ns: Arc<AtomicU64>,
        running: Arc<AtomicBool>,
    ) {
        let mut resampler = Resampler::new(device_rate);
        let chunk_raw_len = resampler.input_chunk_frames() * channels.max(1);

        let mut pending: Vec<f32> = Vec::with_capacity(chunk_raw_len * 2);
        let mut scratch = vec![0.0f32; 4096];
        let mut mono = Vec::with_capacity(resampler.input_chunk_frames());
        let mut i16_buf: Vec<i16> = Vec::with_capacity(resampler.input_chunk_frames());

        loop {
            let popped = consumer.pop_slice(&mut scratch);
            if popped > 0 {
                pending.extend_from_slice(&scratch[..popped]);
            } else if !running.load(Ordering::Acquire) {
                break;
            } else {
                std::thread::sleep(IDLE_POLL);
                continue;
            }

            while pending.len() >= chunk_raw_len {
                let raw: Vec<f32> = pending.drain(..chunk_raw_len).collect();
                downmix_to_mono(&raw, channels.max(1), &mut mono);
                let out = resampler.process(&mono);
                if out.is_empty() {
                    continue;
                }
                i16_buf.clear();
                i16_buf.extend(out.iter().copied().map(f32_to_i16));
                let host_ns = last_cb_host_ns.load(Ordering::Relaxed);

                let mut guard = shared.lock().expect("system writer mutex poisoned");
                if guard.writer.append(&i16_buf).is_err() {
                    tracing::warn!("system wav writer append failed; dropping this chunk");
                    continue;
                }
                guard.frames += i16_buf.len() as u64;
                guard.last_host_ns = host_ns;
            }
        }
    }

    /// Everything that can block on the system-audio-recording TCC dialog,
    /// run on its own thread exactly like `MicSource::build`, so
    /// [`AudioSource::start`] can bound the wait with
    /// [`crate::AUDIO_PERMISSION_TIMEOUT`].
    fn build(dest: PathBuf) -> Result<Built, Error> {
        // 1. Default output device — the tap rides alongside it in an aggregate.
        let out_dev: AudioObjectID = unsafe {
            get_property(
                ca::kAudioObjectSystemObject as AudioObjectID,
                ca::kAudioHardwarePropertyDefaultOutputDevice,
            )?
        };
        let output_uid = output_device_uid(out_dev)?;

        // 2. Global stereo tap, excluding nothing == everything the machine plays.
        let empty_processes: Retained<NSArray<NSNumber>> = NSArray::from_slice(&[]);
        let desc: Retained<CATapDescription> = unsafe {
            CATapDescription::initStereoGlobalTapButExcludeProcesses(
                CATapDescription::alloc(),
                &empty_processes,
            )
        };
        unsafe { desc.setName(&NSString::from_str("meet-ai system tap")) };
        let uuid_string = format_uuid_bytes(locally_unique_uuid_bytes());
        let uuid = NSUUID::from_string(&NSString::from_str(&uuid_string))
            .expect("a freshly-formatted canonical UUID string always parses");
        unsafe { desc.setUUID(&uuid) };
        unsafe { desc.setPrivate(true) };
        unsafe { desc.setMuteBehavior(CATapMuteBehavior::Unmuted) };
        let tap_uuid_string = uuid_string.clone();

        // THE call under test — per FINDINGS §9.1/§10.1, a `noErr` here proves
        // nothing about capture; only the samples in `handle()` below do.
        let mut tap_id: AudioObjectID = ca::kAudioObjectUnknown;
        let status = unsafe { AudioHardwareCreateProcessTap(Some(&desc), &mut tap_id) };
        if status != 0 {
            return Err(Error::NoDevice(format!(
                "AudioHardwareCreateProcessTap failed: OSStatus {status}"
            )));
        }
        if tap_id == ca::kAudioObjectUnknown {
            return Err(Error::NoDevice(
                "AudioHardwareCreateProcessTap returned noErr but kAudioObjectUnknown".to_string(),
            ));
        }

        let format: AudioStreamBasicDescription =
            unsafe { get_property(tap_id, ca::kAudioTapPropertyFormat)? };
        if format.mChannelsPerFrame == 0 || format.mSampleRate <= 0.0 {
            unsafe { AudioHardwareDestroyProcessTap(tap_id) };
            return Err(Error::NoDevice(format!(
                "tap reports a degenerate format: {}Hz {}ch",
                format.mSampleRate, format.mChannelsPerFrame
            )));
        }
        let channels = format.mChannelsPerFrame as usize;
        let device_rate = format.mSampleRate as u32;

        // 3. Private aggregate device carrying the tap.
        let agg_uid = format!("meet-ai-agg-{uuid_string}");
        let sub_device = cf_dict(&[(ca::kAudioSubDeviceUIDKey, cf_string_type(&output_uid))]);
        let sub_device_list =
            CFArray::from_objects(&[&*sub_device as &CFDictionary<CFString, CFType>]);
        let sub_tap = cf_dict(&[
            (ca::kAudioSubTapDriftCompensationKey, cf_bool_type(true)),
            (ca::kAudioSubTapUIDKey, cf_string_type(&tap_uuid_string)),
        ]);
        let tap_list = CFArray::from_objects(&[&*sub_tap as &CFDictionary<CFString, CFType>]);

        let composition = cf_dict(&[
            (
                ca::kAudioAggregateDeviceNameKey,
                cf_string_type("meet-ai system aggregate"),
            ),
            (ca::kAudioAggregateDeviceUIDKey, cf_string_type(&agg_uid)),
            (
                ca::kAudioAggregateDeviceMainSubDeviceKey,
                cf_string_type(&output_uid),
            ),
            (ca::kAudioAggregateDeviceIsPrivateKey, cf_bool_type(true)),
            (ca::kAudioAggregateDeviceIsStackedKey, cf_bool_type(false)),
            // MUST be false — contract §10 / FINDINGS: `true` blocks
            // `AudioDeviceStart` until some tapped process produces audio,
            // silently corrupting `start_host_ns`.
            (
                ca::kAudioAggregateDeviceTapAutoStartKey,
                cf_bool_type(false),
            ),
            (ca::kAudioAggregateDeviceSubDeviceListKey, unsafe {
                CFRetained::cast_unchecked(sub_device_list)
            }),
            (ca::kAudioAggregateDeviceTapListKey, unsafe {
                CFRetained::cast_unchecked(tap_list)
            }),
        ]);

        let mut aggregate_id: AudioObjectID = ca::kAudioObjectUnknown;
        let status = unsafe {
            AudioHardwareCreateAggregateDevice(
                composition.as_ref(),
                std::ptr::NonNull::from(&mut aggregate_id),
            )
        };
        if status != 0 {
            unsafe { AudioHardwareDestroyProcessTap(tap_id) };
            return Err(Error::NoDevice(format!(
                "AudioHardwareCreateAggregateDevice failed: OSStatus {status}"
            )));
        }

        // 4. WAV + ring buffer, then the IO proc.
        let writer = WavWriter::create(&dest)?;
        let shared = Arc::new(Mutex::new(Shared {
            writer,
            frames: 0,
            last_host_ns: 0,
        }));
        let rb = HeapRb::<f32>::new(RING_CAPACITY_SAMPLES);
        let (producer, consumer) = rb.split();
        let last_cb_host_ns = Arc::new(AtomicU64::new(0));
        let running = Arc::new(AtomicBool::new(true));

        // Every callback needs to mutate its own scratch (de-interleave
        // buffers, the interleaved output, and the ring-buffer producer),
        // but the IO block itself must be `Fn` (Core Audio's `DynBlock`
        // requires it, since nothing prevents a re-entrant call). One
        // `Mutex` around all three gives interior mutability with a single
        // lock per callback rather than three.
        let callback_state = Mutex::new((producer, Vec::<Vec<f32>>::new(), Vec::<f32>::new()));
        let last_cb_host_ns_for_block = Arc::clone(&last_cb_host_ns);
        let io_block: RcBlock<IoBlockFn> = RcBlock::new(
            move |_now: std::ptr::NonNull<AudioTimeStamp>,
                  in_input_data: std::ptr::NonNull<AudioBufferList>,
                  in_input_time: std::ptr::NonNull<AudioTimeStamp>,
                  _out_output_data: std::ptr::NonNull<AudioBufferList>,
                  _out_output_time: std::ptr::NonNull<AudioTimeStamp>| {
                // SAFETY: Core Audio guarantees `in_input_data`/`in_input_time`
                // are valid for the duration of this call.
                let host_ns =
                    unsafe { ca::AudioConvertHostTimeToNanos(in_input_time.as_ref().mHostTime) };
                last_cb_host_ns_for_block.store(host_ns, Ordering::Relaxed);

                let abl = unsafe { in_input_data.as_ref() };
                let n = abl.mNumberBuffers as usize;
                if n == 0 {
                    return;
                }
                let Ok(mut state) = callback_state.lock() else {
                    return;
                };
                let (producer, channel_scratch, interleaved) = &mut *state;

                // `mBuffers` is declared `[AudioBuffer; 1]` but is really a
                // C flexible array member — buffer `i` lives at
                // `mBuffers.as_ptr().add(i)`, exactly like the Swift probe's
                // `UnsafeMutableAudioBufferListPointer`.
                let buffers_ptr = abl.mBuffers.as_ptr();
                if channel_scratch.len() < n {
                    channel_scratch.resize_with(n, Vec::new);
                }
                let mut any_frames = 0usize;
                for (i, dest) in channel_scratch.iter_mut().enumerate().take(n) {
                    // SAFETY: `i < mNumberBuffers`, and `mData` is valid for
                    // `mDataByteSize` bytes of `f32` per Core Audio's own
                    // documented layout for this (non-interleaved) tap format.
                    let buf = unsafe { &*buffers_ptr.add(i) };
                    let count = buf.mDataByteSize as usize / std::mem::size_of::<f32>();
                    any_frames = any_frames.max(count);
                    dest.clear();
                    if !buf.mData.is_null() {
                        let slice =
                            unsafe { std::slice::from_raw_parts(buf.mData.cast::<f32>(), count) };
                        dest.extend_from_slice(slice);
                    }
                }
                if any_frames == 0 {
                    return;
                }
                interleave_into(&channel_scratch[..n], interleaved);
                let _ = producer.push_slice(interleaved);
            },
        );

        let mut io_proc_id: AudioDeviceIOProcID = None;
        let status = unsafe {
            ca::AudioDeviceCreateIOProcIDWithBlock(
                std::ptr::NonNull::from(&mut io_proc_id),
                aggregate_id,
                None,
                RcBlock::as_ptr(&io_block),
            )
        };
        if status != 0 {
            unsafe { AudioHardwareDestroyAggregateDevice(aggregate_id) };
            unsafe { AudioHardwareDestroyProcessTap(tap_id) };
            return Err(Error::NoDevice(format!(
                "AudioDeviceCreateIOProcIDWithBlock failed: OSStatus {status}"
            )));
        }
        let Some(io_proc_id_value) = io_proc_id else {
            unsafe { AudioHardwareDestroyAggregateDevice(aggregate_id) };
            unsafe { AudioHardwareDestroyProcessTap(tap_id) };
            return Err(Error::NoDevice(
                "AudioDeviceCreateIOProcIDWithBlock returned noErr but no IOProcID".to_string(),
            ));
        };

        let status = unsafe { ca::AudioDeviceStart(aggregate_id, io_proc_id) };
        if status != 0 {
            unsafe { ca::AudioDeviceDestroyIOProcID(aggregate_id, io_proc_id) };
            unsafe { AudioHardwareDestroyAggregateDevice(aggregate_id) };
            unsafe { AudioHardwareDestroyProcessTap(tap_id) };
            return Err(Error::NoDevice(format!(
                "AudioDeviceStart failed: OSStatus {status}"
            )));
        }

        let worker = std::thread::Builder::new()
            .name("meet-rec-system-worker".to_string())
            .spawn({
                let shared = Arc::clone(&shared);
                let running = Arc::clone(&running);
                move || {
                    Self::worker_loop(
                        consumer,
                        channels,
                        device_rate,
                        shared,
                        last_cb_host_ns,
                        running,
                    )
                }
            })
            .expect("spawning the system worker thread");

        Ok(Built {
            tap_id,
            aggregate_id,
            io_proc_id: Some(io_proc_id_value),
            _io_block: io_block,
            worker,
            running,
            shared,
        })
    }
}

impl AudioSource for SystemSource {
    fn start(&mut self, dest: PathBuf) -> Result<(), Error> {
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::Builder::new()
            .name("meet-rec-system-init".to_string())
            .spawn(move || {
                let _ = tx.send(Self::build(dest));
            })
            .expect("spawning the system tap init thread");

        let built = match rx.recv_timeout(crate::AUDIO_PERMISSION_TIMEOUT) {
            Ok(result) => result?,
            Err(_) => {
                // Same shape as `MicSource::start`: the init thread is still
                // blocked inside Core Audio with no way to be cancelled, and
                // is deliberately leaked rather than joined.
                return Err(Error::PermissionDenied);
            }
        };

        self.built = Some(built);
        Ok(())
    }

    fn stop(&mut self) -> Result<(), Error> {
        let Some(built) = self.built.take() else {
            return Ok(());
        };
        built.running.store(false, Ordering::Release);
        unsafe { ca::AudioDeviceStop(built.aggregate_id, built.io_proc_id) };
        unsafe { ca::AudioDeviceDestroyIOProcID(built.aggregate_id, built.io_proc_id) };
        unsafe { AudioHardwareDestroyAggregateDevice(built.aggregate_id) };
        unsafe { AudioHardwareDestroyProcessTap(built.tap_id) };
        let _ = built.worker.join();
        {
            let mut guard = built.shared.lock().expect("system writer mutex poisoned");
            guard.writer.fsync_data()?;
            guard.writer.patch_header()?;
        }
        Ok(())
    }

    fn channel(&self) -> Channel {
        Channel::System
    }

    fn position(&self) -> Option<(u64, u64)> {
        let built = self.built.as_ref()?;
        let guard = built.shared.lock().expect("system writer mutex poisoned");
        if guard.last_host_ns == 0 {
            None
        } else {
            Some((guard.last_host_ns, guard.frames))
        }
    }

    fn checkpoint(&mut self) -> Result<(), Error> {
        let Some(built) = &self.built else {
            return Ok(());
        };
        let mut guard = built.shared.lock().expect("system writer mutex poisoned");
        guard.writer.fsync_data()?;
        guard.writer.patch_header()?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn interleave(channels: &[Vec<f32>]) -> Vec<f32> {
        let mut out = Vec::new();
        interleave_into(channels, &mut out);
        out
    }

    #[test]
    fn interleave_of_a_single_channel_is_a_copy() {
        let ch0 = vec![1.0f32, 2.0, 3.0];
        assert_eq!(interleave(&[ch0]), vec![1.0, 2.0, 3.0]);
    }

    #[test]
    fn interleave_of_stereo_alternates_channels() {
        let left = vec![1.0f32, 2.0, 3.0];
        let right = vec![10.0f32, 20.0, 30.0];
        assert_eq!(
            interleave(&[left, right]),
            vec![1.0, 10.0, 2.0, 20.0, 3.0, 30.0]
        );
    }

    #[test]
    fn interleave_of_empty_input_is_empty() {
        let empty: Vec<f32> = Vec::new();
        assert_eq!(interleave(&[empty.clone(), empty]), Vec::<f32>::new());
        assert_eq!(interleave(&[]), Vec::<f32>::new());
    }

    #[test]
    fn interleave_truncates_to_the_shortest_channel() {
        let long = vec![1.0f32, 2.0, 3.0];
        let short = vec![10.0f32, 20.0];
        assert_eq!(interleave(&[long, short]), vec![1.0, 10.0, 2.0, 20.0]);
    }

    /// A buffer that already holds data from a previous, larger callback
    /// must not leak stale samples past the new, shorter length — this is
    /// exactly the "reuse capacity" property `interleave_into` trades for
    /// avoiding an allocation every callback.
    #[test]
    fn interleave_into_reuses_a_buffer_without_leaking_stale_tail_samples() {
        let mut out = vec![9.0f32; 32];
        interleave_into(&[vec![1.0, 2.0], vec![10.0, 20.0]], &mut out);
        assert_eq!(out, vec![1.0, 10.0, 2.0, 20.0]);
    }

    #[test]
    fn locally_unique_uuid_bytes_do_not_repeat_back_to_back() {
        let a = locally_unique_uuid_bytes();
        let b = locally_unique_uuid_bytes();
        assert_ne!(a, b);
    }
}
