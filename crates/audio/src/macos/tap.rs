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
//! through the mismatched-encoding method. De-interleaving the tap's buffers
//! — the genuine logic that doesn't touch Core Audio — lives in
//! `super::tap_buffers` and is unit-tested against synthetic buffer lists.

use std::cell::RefCell;
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

use super::tap_buffers::{LayoutProbe, TapBuffers, gather_into};
use super::tap_pipeline::TapPipeline;
use super::tap_rate::{CallbackMeter, RateSources, RateState, RateWatch};
use super::tap_uuid::{format_uuid_bytes, locally_unique_uuid_bytes};
use crate::tee::Tee;
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
    /// Unregistered before the aggregate device is destroyed.
    rate_watch: RateWatch,
    /// Reported and measured rates, for [`AudioSource::rate_report`].
    rates: Arc<RateState>,
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
    /// The same `Arc` as `built.shared`, kept alive independently so
    /// [`AudioSource::position`] still reports the final frame count after
    /// [`AudioSource::stop`] has torn `built` down. Without this, the
    /// orchestrator's post-stop "read the exact final count" step (contract
    /// §7's "equality on graceful stop") would silently see `None` and fall
    /// back to zero, undoing everything the earlier checkpoints wrote.
    shared: Option<Arc<Mutex<Shared>>>,
    /// The live-transcription copy, if one was asked for ([`AudioSource::tee`]).
    tee: Option<Tee>,
}

impl Default for SystemSource {
    fn default() -> Self {
        Self::new()
    }
}

impl SystemSource {
    pub fn new() -> Self {
        Self {
            built: None,
            shared: None,
            tee: None,
        }
    }

    /// Raw IO-proc samples → [`TapPipeline`], at [`RateState::effective`].
    #[allow(clippy::too_many_arguments)]
    fn worker_loop(
        mut consumer: HeapCons<f32>,
        mut pipeline: TapPipeline,
        rates: Arc<RateState>,
        shared: Arc<Mutex<Shared>>,
        last_cb_host_ns: Arc<AtomicU64>,
        running: Arc<AtomicBool>,
        tee: Option<Tee>,
        probe: Arc<LayoutProbe>,
    ) {
        let mut sink = |frames: &[i16]| {
            let host_ns = last_cb_host_ns.load(Ordering::Relaxed);
            let Ok(mut guard) = shared.lock() else {
                tracing::warn!("system writer mutex poisoned; dropping this chunk");
                return;
            };
            if guard.writer.append(frames).is_err() {
                tracing::warn!("system wav writer append failed; dropping this chunk");
                return;
            }
            guard.frames += frames.len() as u64;
            guard.last_host_ns = host_ns;
            // Same as the mic: the writer lock is released first.
            drop(guard);
            if let Some(tee) = &tee {
                tee.offer(frames);
            }
        };
        // Sized once; `pop_slice` only refills it.
        let mut scratch = vec![0.0f32; 4096];
        loop {
            let popped = consumer.pop_slice(&mut scratch);
            if popped == 0 && !running.load(Ordering::Acquire) {
                break;
            } else if popped == 0 {
                std::thread::sleep(IDLE_POLL);
                continue;
            }
            if let Some(report) = probe.take_report() {
                tracing::info!("{report}");
            }
            pipeline.follow(&rates, &mut sink);
            pipeline.push(&scratch[..popped], &mut sink);
        }
    }

    /// Everything that can block on the system-audio-recording TCC dialog,
    /// run on its own thread exactly like `MicSource::build`, so
    /// [`AudioSource::start`] can bound the wait with
    /// [`crate::AUDIO_PERMISSION_TIMEOUT`].
    fn build(dest: PathBuf, tee: Option<Tee>) -> Result<Built, Error> {
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
        let tap_format_rate = format.mSampleRate.round() as u32;

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

        // The IO proc runs at the aggregate's rate, not the tap format's
        // (TUR-80: 16 kHz on a Bluetooth headset in a call).
        let rate_sources = RateSources {
            tap_format_rate,
            aggregate_id,
            output_device_id: out_dev,
        };
        let rates = RateState::new(rate_sources);
        let input_rate = rates.effective();
        // Only the tap's own buffers, never the output device's mic (TUR-87).
        let probe = Arc::new(LayoutProbe::new(TapBuffers::read(aggregate_id, &format)));
        let probe_for_block = Arc::clone(&probe);

        // 4. WAV + ring buffer, then the IO proc.
        //
        // A segment reopen (default-output-device change) rebuilds the tap
        // and aggregate device from scratch against `dest`, but the archive
        // itself stays one continuous file across segments — see the
        // matching comment in `crate::mic::MicSource::build`.
        let writer = if dest.exists() {
            WavWriter::open_append(&dest)?
        } else {
            WavWriter::create(&dest)?
        };
        let shared = Arc::new(Mutex::new(Shared {
            writer,
            frames: 0,
            last_host_ns: 0,
        }));
        let rb = HeapRb::<f32>::new(RING_CAPACITY_SAMPLES);
        let (producer, consumer) = rb.split();
        let last_cb_host_ns = Arc::new(AtomicU64::new(0));
        let running = Arc::new(AtomicBool::new(true));

        // The IO block must be `Fn`, so the producer and interleave buffer sit
        // in a `RefCell`: no lock on the real-time thread, and a re-entrant
        // call skips its cycle. `interleaved` is sized once (2 x 16384 frames).
        // Plus the delivered-rate meter (TUR-84): integers only, real-time safe.
        let meter = CallbackMeter::new(Arc::clone(&rates), channels);
        let callback_state = RefCell::new((producer, Vec::<f32>::with_capacity(32_768), meter));
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
                let Ok(mut state) = callback_state.try_borrow_mut() else {
                    return;
                };
                let (producer, interleaved, meter) = &mut *state;

                // `mBuffers` is declared `[AudioBuffer; 1]` but is really a
                // C flexible array member — buffer `i` lives at
                // `mBuffers.as_ptr().add(i)`, exactly like the Swift probe's
                // `UnsafeMutableAudioBufferListPointer`.
                let buffers_ptr = abl.mBuffers.as_ptr();
                // SAFETY: `i < mNumberBuffers`, and `mData` is valid for
                // `mDataByteSize` bytes of `f32` per Core Audio's own
                // documented layout for this (non-interleaved) tap format;
                // both outlive this call. A null `mData` reads as empty.
                let channel = |i: usize| -> &[f32] {
                    let buf = unsafe { &*buffers_ptr.add(i) };
                    if buf.mData.is_null() {
                        return &[];
                    }
                    let count = buf.mDataByteSize as usize / std::mem::size_of::<f32>();
                    unsafe { std::slice::from_raw_parts(buf.mData.cast::<f32>(), count) }
                };
                probe_for_block.observe(n, |i| unsafe { (*buffers_ptr.add(i)).mNumberChannels });
                let tap = probe_for_block.layout().range(n);
                let any_frames = tap.clone().map(|i| channel(i).len()).max().unwrap_or(0);
                if any_frames == 0 {
                    return;
                }
                let buffer = |i: usize| {
                    // SAFETY: as for `channel`, `i < mNumberBuffers`.
                    (unsafe { (*buffers_ptr.add(i)).mNumberChannels }, channel(i))
                };
                gather_into(tap, buffer, channels, interleaved);
                let _ = producer.push_slice(interleaved);
                meter.observe(host_ns, interleaved.len());
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

        let rate_watch = RateWatch::install(&rates);
        let pipeline = TapPipeline::new(channels, input_rate);
        let worker = std::thread::Builder::new()
            .name("meet-rec-system-worker".to_string())
            .spawn({
                let shared = Arc::clone(&shared);
                let running = Arc::clone(&running);
                let rates = Arc::clone(&rates);
                move || {
                    Self::worker_loop(
                        consumer,
                        pipeline,
                        rates,
                        shared,
                        last_cb_host_ns,
                        running,
                        tee,
                        probe,
                    )
                }
            })
            .expect("spawning the system worker thread");

        Ok(Built {
            tap_id,
            aggregate_id,
            io_proc_id: Some(io_proc_id_value),
            _io_block: io_block,
            rate_watch,
            rates,
            worker,
            running,
            shared,
        })
    }
}

impl AudioSource for SystemSource {
    fn start(&mut self, dest: PathBuf) -> Result<(), Error> {
        let (tx, rx) = std::sync::mpsc::channel();
        let tee = self.tee.clone();
        std::thread::Builder::new()
            .name("meet-rec-system-init".to_string())
            .spawn(move || {
                let _ = tx.send(Self::build(dest, tee));
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

        self.shared = Some(Arc::clone(&built.shared));
        self.built = Some(built);
        Ok(())
    }

    fn stop(&mut self) -> Result<(), Error> {
        let Some(built) = self.built.take() else {
            return Ok(());
        };
        built.running.store(false, Ordering::Release);
        drop(built.rate_watch);
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
        let shared = self.shared.as_ref()?;
        let guard = shared.lock().expect("system writer mutex poisoned");
        if guard.last_host_ns == 0 {
            None
        } else {
            Some((guard.last_host_ns, guard.frames))
        }
    }

    fn fsync_data(&mut self) -> Result<(), Error> {
        let Some(built) = &self.built else {
            return Ok(());
        };
        let mut guard = built.shared.lock().expect("system writer mutex poisoned");
        guard.writer.fsync_data()?;
        Ok(())
    }

    fn patch_header(&mut self) -> Result<(), Error> {
        let Some(built) = &self.built else {
            return Ok(());
        };
        let mut guard = built.shared.lock().expect("system writer mutex poisoned");
        guard.writer.patch_header()?;
        Ok(())
    }

    fn pad_leading_silence(&mut self, frames: u64) -> Result<(), Error> {
        let Some(built) = &self.built else {
            return Ok(());
        };
        let mut guard = built.shared.lock().expect("system writer mutex poisoned");
        guard.writer.prepend_silence(frames)?;
        guard.frames += frames;
        drop(guard);
        if let Some(tee) = &self.tee {
            tee.offer_silence(frames);
        }
        Ok(())
    }

    fn tee(&mut self, tee: Tee) {
        self.tee = Some(tee);
    }

    fn rate_report(&self) -> Option<String> {
        self.built.as_ref().map(|built| built.rates.describe())
    }

    fn device_rate(&self) -> Option<u32> {
        self.built.as_ref().map(|built| built.rates.effective())
    }
}
