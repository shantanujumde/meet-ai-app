//! [`LoopbackSource`] over a fake [`Backend`]: the keepalive's lifecycle,
//! and gap filling, silent buffers and the clock end to end, with no device.

use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use super::*;

const MS: u64 = 1_000_000;

/// What the fake backend and its streams did, in order.
#[derive(Clone, Default)]
struct Log(Arc<Mutex<Vec<String>>>);

impl Log {
    fn push(&self, event: impl Into<String>) {
        self.0.lock().unwrap().push(event.into());
    }

    fn events(&self) -> Vec<String> {
        self.0.lock().unwrap().clone()
    }

    fn count(&self, event: &str) -> usize {
        self.events().iter().filter(|e| *e == event).count()
    }
}

struct FakeStream {
    name: &'static str,
    log: Log,
}

impl LiveStream for FakeStream {
    fn pause(&mut self) {
        self.log.push(format!("{} paused", self.name));
    }
}

impl Drop for FakeStream {
    fn drop(&mut self) {
        self.log.push(format!("{} dropped", self.name));
    }
}

/// Where a test drives the capture callback from.
type Slot = Arc<Mutex<Option<Capture>>>;

struct FakeBackend {
    log: Log,
    format: Option<Format>,
    keepalive_fails: bool,
    capture_fails: bool,
    slot: Slot,
}

impl FakeBackend {
    fn new(rate: u32, channels: usize) -> (Self, Log, Slot) {
        let log = Log::default();
        let slot = Slot::default();
        let backend = Self {
            log: log.clone(),
            format: Some(Format {
                rate,
                channels,
                label: "Fake Speakers".into(),
            }),
            keepalive_fails: false,
            capture_fails: false,
            slot: Arc::clone(&slot),
        };
        (backend, log, slot)
    }
}

impl Backend for FakeBackend {
    fn format(&mut self) -> Result<Format, Error> {
        self.log.push("format");
        self.format
            .clone()
            .ok_or_else(|| Error::NoDevice("no default output device".into()))
    }

    fn start_keepalive(&mut self) -> Result<Box<dyn LiveStream>, Error> {
        if self.keepalive_fails {
            self.log.push("keepalive failed");
            return Err(Error::NoDevice("render stream refused".into()));
        }
        self.log.push("keepalive started");
        Ok(Box::new(FakeStream {
            name: "keepalive",
            log: self.log.clone(),
        }))
    }

    fn start_capture(
        &mut self,
        _format: &Format,
        capture: Capture,
    ) -> Result<Box<dyn LiveStream>, Error> {
        if self.capture_fails {
            self.log.push("capture failed");
            return Err(Error::NoDevice("loopback refused".into()));
        }
        self.log.push("capture started");
        *self.slot.lock().unwrap() = Some(capture);
        Ok(Box::new(FakeStream {
            name: "capture",
            log: self.log.clone(),
        }))
    }
}

/// One packet through the capture callback the backend was handed.
fn packet(slot: &Slot, samples: &[f32], capture_ns: Option<u64>, silent: bool) {
    let mut samples = samples.to_vec();
    slot.lock()
        .unwrap()
        .as_mut()
        .expect("capture started")
        .packet(&mut samples, capture_ns, silent);
}

fn read_wav(path: &std::path::Path) -> Vec<i16> {
    hound::WavReader::open(path)
        .unwrap()
        .into_samples::<i16>()
        .map(Result::unwrap)
        .collect()
}

fn tone(frames: usize, offset: usize) -> Vec<f32> {
    test_support::sine_f32(frames + offset, 16_000, 440.0, 0.5)[offset..].to_vec()
}

fn wait_for_position(source: &dyn AudioSource) -> (u64, u64) {
    let started = Instant::now();
    loop {
        if let Some(pos) = source.position() {
            return pos;
        }
        assert!(
            started.elapsed() < Duration::from_secs(5),
            "no position within 5 s"
        );
        std::thread::sleep(Duration::from_millis(1));
    }
}

#[test]
fn the_keepalive_starts_before_the_capture_and_stops_with_it() {
    let dir = tempfile::tempdir().unwrap();
    let (backend, log, _slot) = FakeBackend::new(48_000, 2);
    let mut source = LoopbackSource::new(backend);
    source.start(dir.path().join("system.wav")).unwrap();
    assert!(source.has_keepalive());
    assert_eq!(
        log.events(),
        vec!["format", "keepalive started", "capture started"]
    );

    source.stop().unwrap();
    assert!(!source.has_keepalive());
    assert_eq!(
        log.events()[3..],
        [
            "capture paused",
            "capture dropped",
            "keepalive paused",
            "keepalive dropped"
        ]
    );
}

#[test]
fn a_failed_keepalive_does_not_fail_the_recording() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("system.wav");
    let (mut backend, log, slot) = FakeBackend::new(16_000, 1);
    backend.keepalive_fails = true;
    let mut source = LoopbackSource::new(backend);
    source.start(path.clone()).unwrap();
    assert!(!source.has_keepalive());
    assert_eq!(log.count("capture started"), 1);

    for n in 0..20 {
        packet(
            &slot,
            &tone(160, n * 160),
            Some(1_000 * MS + n as u64 * 10 * MS),
            false,
        );
    }
    wait_for_position(&source);
    source.stop().unwrap();
    assert!(!read_wav(&path).is_empty());
}

#[test]
fn a_failed_capture_stops_the_keepalive_and_fails_the_start() {
    let dir = tempfile::tempdir().unwrap();
    let (mut backend, log, _slot) = FakeBackend::new(48_000, 2);
    backend.capture_fails = true;
    let mut source = LoopbackSource::new(backend);
    let err = source.start(dir.path().join("system.wav")).unwrap_err();
    assert!(matches!(err, Error::NoDevice(_)), "{err:?}");
    assert_eq!(log.count("keepalive paused"), 1);
    assert_eq!(log.count("keepalive dropped"), 1);
    assert!(!source.has_keepalive());
    assert_eq!(source.position(), None);
    source.stop().unwrap();
}

#[test]
fn no_output_device_opens_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let (mut backend, log, _slot) = FakeBackend::new(48_000, 2);
    backend.format = None;
    let mut source = LoopbackSource::new(backend);
    let err = source.start(dir.path().join("system.wav")).unwrap_err();
    assert!(matches!(err, Error::NoDevice(_)), "{err:?}");
    assert_eq!(log.events(), vec!["format"]);
}

#[test]
fn a_device_with_no_channels_is_refused() {
    let dir = tempfile::tempdir().unwrap();
    let (mut backend, log, _slot) = FakeBackend::new(48_000, 0);
    backend.format.as_mut().unwrap().channels = 0;
    let mut source = LoopbackSource::new(backend);
    assert!(source.start(dir.path().join("system.wav")).is_err());
    assert_eq!(log.events(), vec!["format"]);
}

#[test]
fn every_start_gets_its_own_keepalive_and_drop_stops_them() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("system.wav");
    let (backend, log, _slot) = FakeBackend::new(48_000, 2);
    let mut source = LoopbackSource::new(backend);
    source.start(path.clone()).unwrap();
    source.stop().unwrap();
    source.start(path).unwrap();
    assert_eq!(log.count("keepalive started"), 2);
    drop(source);
    assert_eq!(log.count("keepalive dropped"), 2);
    assert_eq!(log.count("capture dropped"), 2);
}

#[test]
fn a_timestamp_gap_is_written_as_silence_and_keeps_wall_time() {
    // 16 kHz mono, 10 ms packets: 0.5 s of tone, 0.5 s with no packets (as
    // when nothing plays and the keepalive is not running), 0.5 s of tone.
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("system.wav");
    let (backend, _log, slot) = FakeBackend::new(16_000, 1);
    let mut source = LoopbackSource::new(backend);
    source.start(path.clone()).unwrap();

    let mut ns = 1_000 * MS;
    for n in 0..100 {
        if n == 50 {
            ns += 500 * MS;
        }
        packet(&slot, &tone(160, n * 160), Some(ns), false);
        ns += 10 * MS;
    }
    source.stop().unwrap();

    let wav = read_wav(&path);
    let expected = 100 * 160 + 8_000;
    // Only the resampler's last partial chunk is still unwritten at a stop.
    assert!(
        wav.len() <= expected && expected - wav.len() < 1_024,
        "{} frames, expected about {expected}",
        wav.len()
    );
    let peak = |range: std::ops::Range<usize>| {
        wav[range]
            .iter()
            .map(|s| s.unsigned_abs())
            .max()
            .unwrap_or(0)
    };
    assert!(peak(1_000..7_000) > 10_000, "tone before the gap");
    assert_eq!(peak(8_300..15_700), 0, "the gap is silence");
    assert!(peak(17_000..22_000) > 10_000, "tone after the gap");
}

#[test]
fn a_long_gap_is_written_without_resampling_it_all() {
    // A minute of nothing at 48 kHz stereo: the length must be exact to a
    // frame or two, and quick.
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("system.wav");
    let (backend, _log, slot) = FakeBackend::new(48_000, 2);
    let mut source = LoopbackSource::new(backend);
    source.start(path.clone()).unwrap();
    let block = vec![0.1f32; 960];
    packet(&slot, &block, Some(1_000 * MS), false);
    packet(&slot, &block, Some(1_010 * MS + 60_000 * MS), false);
    let started = Instant::now();
    source.stop().unwrap();
    assert!(started.elapsed() < Duration::from_secs(10));

    let frames = read_wav(&path).len() as u64;
    let expected = 60 * 16_000 + 2 * 160;
    assert!(
        frames <= expected && expected - frames < 1_024,
        "{frames} frames, expected about {expected}"
    );
}

#[test]
fn silent_and_non_finite_buffers_are_written_as_zeros() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("system.wav");
    let (backend, _log, slot) = FakeBackend::new(16_000, 1);
    let mut source = LoopbackSource::new(backend);
    source.start(path.clone()).unwrap();
    let mut ns = 1_000 * MS;
    let mut garbage = vec![0.8f32; 160];
    garbage[7] = f32::NAN;
    for n in 0..100 {
        // Every packet is flagged silent but holds a loud tone, or is NaN.
        if n % 2 == 0 {
            packet(&slot, &tone(160, n * 160), Some(ns), true);
        } else {
            packet(&slot, &garbage, Some(ns), true);
        }
        ns += 10 * MS;
    }
    source.stop().unwrap();
    let wav = read_wav(&path);
    assert!(wav.len() > 10_000);
    assert!(
        wav.iter().all(|&s| s == 0),
        "a silent buffer leaked through"
    );
}

#[test]
fn position_pairs_the_latest_capture_time_with_the_frames_written() {
    let dir = tempfile::tempdir().unwrap();
    let (backend, _log, slot) = FakeBackend::new(16_000, 1);
    let mut source = LoopbackSource::new(backend);
    assert_eq!(source.position(), None, "not started");
    source.start(dir.path().join("system.wav")).unwrap();
    assert_eq!(source.position(), None, "no packet yet");

    // No capture time on the first packet: no position from it.
    packet(&slot, &tone(1_600, 0), None, false);
    std::thread::sleep(Duration::from_millis(50));
    assert_eq!(source.position(), None);

    let last = 2_000 * MS;
    packet(&slot, &tone(1_600, 1_600), Some(last), false);
    let (host_ns, frames) = wait_for_position(&source);
    assert_eq!(host_ns, last);
    assert!(frames > 0);
    source.stop().unwrap();
    let (_, final_frames) = source.position().unwrap();
    assert_eq!(
        final_frames,
        hound::WavReader::open(dir.path().join("system.wav"))
            .unwrap()
            .duration() as u64,
        "position agrees with the header after stop"
    );
}

#[test]
fn the_tee_and_the_head_pad_see_the_same_frames_as_the_wav() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("system.wav");
    let (backend, _log, slot) = FakeBackend::new(16_000, 1);
    let mut source = LoopbackSource::new(backend);
    let (tee, feed) = crate::tee::tee();
    source.tee(tee);
    source.start(path.clone()).unwrap();
    let mut ns = 1_000 * MS;
    for n in 0..50 {
        if n == 25 {
            ns += 200 * MS;
        }
        packet(&slot, &tone(160, n * 160), Some(ns), false);
        ns += 10 * MS;
    }
    wait_for_position(&source);
    source.pad_leading_silence(320).unwrap();
    source.stop().unwrap();

    let mut teed = 0;
    while let Ok(chunk) = feed.try_recv() {
        teed += chunk.len();
    }
    assert_eq!(teed, read_wav(&path).len());
    assert_eq!(source.channel(), Channel::System);
    assert_eq!(source.device_rate(), Some(16_000));
    assert!(source.rate_report().unwrap().contains("16000 Hz"));
}

#[test]
fn a_reopened_segment_appends_to_the_same_file() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("system.wav");
    let mut total = 0;
    for _ in 0..2 {
        let (backend, _log, slot) = FakeBackend::new(16_000, 1);
        let mut source = LoopbackSource::new(backend);
        source.start(path.clone()).unwrap();
        for n in 0..30 {
            packet(
                &slot,
                &tone(160, n * 160),
                Some((1_000 + n as u64 * 10) * MS),
                false,
            );
        }
        source.stop().unwrap();
        total += source.position().unwrap().1;
    }
    assert_eq!(read_wav(&path).len() as u64, total);
}
