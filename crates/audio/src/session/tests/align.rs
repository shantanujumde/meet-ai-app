//! TUR-151 at the session level, against sources whose capture times are
//! set by the test: the start alignment uses each channel's frame-0 time,
//! and a reopened segment's head-pad lands at that segment's start.

use super::*;

const MS: u64 = 1_000_000;

/// A source that, the moment it starts, has already written `written`
/// frames of `value`, the first of them captured at `frame_zero_ns`: what a
/// real channel looks like by the time alignment polls it.
struct TimedStub {
    channel: Channel,
    value: i16,
    written: u64,
    frame_zero_ns: u64,
    writer: Option<WavWriter>,
    frames: u64,
    /// When frame `frames` was captured; fixed by `start`, not moved by a pad.
    next_ns: u64,
}

fn timed(channel: Channel, value: i16, written: u64, frame_zero_ns: u64) -> Box<dyn AudioSource> {
    Box::new(TimedStub {
        channel,
        value,
        written,
        frame_zero_ns,
        writer: None,
        frames: 0,
        next_ns: 0,
    })
}

impl AudioSource for TimedStub {
    fn start(&mut self, dest: PathBuf) -> Result<(), AudioError> {
        let mut writer = if dest.exists() {
            WavWriter::open_append(&dest)?
        } else {
            WavWriter::create(&dest)?
        };
        writer.append(&vec![self.value; self.written as usize])?;
        self.writer = Some(writer);
        self.frames = self.written;
        self.next_ns = self.frame_zero_ns + self.written * 1_000_000_000 / 16_000;
        Ok(())
    }

    fn stop(&mut self) -> Result<(), AudioError> {
        if let Some(writer) = self.writer.as_mut() {
            writer.fsync_data()?;
            writer.patch_header()?;
        }
        Ok(())
    }

    fn channel(&self) -> Channel {
        self.channel
    }

    fn position(&self) -> Option<(u64, u64)> {
        self.writer.as_ref().map(|_| (self.next_ns, self.frames))
    }

    fn fsync_data(&mut self) -> Result<(), AudioError> {
        if let Some(writer) = self.writer.as_mut() {
            writer.fsync_data()?;
        }
        Ok(())
    }

    fn patch_header(&mut self) -> Result<(), AudioError> {
        if let Some(writer) = self.writer.as_mut() {
            writer.patch_header()?;
        }
        Ok(())
    }

    fn pad_leading_silence(&mut self, frames: u64) -> Result<(), AudioError> {
        if let Some(writer) = self.writer.as_mut() {
            writer.pad_segment_head(frames)?;
            self.frames += frames;
        }
        Ok(())
    }
}

fn samples(path: &Path) -> Vec<i16> {
    hound::WavReader::open(path)
        .unwrap()
        .samples::<i16>()
        .map(Result::unwrap)
        .collect()
}

/// `(value, run length)` for each run of equal samples.
fn runs(samples: &[i16]) -> Vec<(i16, usize)> {
    let mut out: Vec<(i16, usize)> = Vec::new();
    for &s in samples {
        match out.last_mut() {
            Some((value, n)) if *value == s => *n += 1,
            _ => out.push((s, 1)),
        }
    }
    out
}

/// The microphone starts, then the tap's start blocks for 200 ms while the
/// microphone keeps writing; the tap's frame 0 is 150 ms after the
/// microphone's. Taking `host_ns` alone, the microphone looked 40 ms *later*
/// and was the one padded; by frame-0 times it is the system track that
/// starts 150 ms late and gets 2400 frames of pad.
#[test]
fn a_second_source_that_starts_late_is_padded_by_its_frame_zero() {
    let tmp = tempfile::tempdir().unwrap();
    let mic = timed(Channel::Mic, 1, 3_200, 1_000 * MS);
    let sys = timed(Channel::System, 2, 160, 1_150 * MS);
    let session = RecordingSession::start(tmp.path().to_path_buf(), mic, Some(sys)).unwrap();
    let report = session.stop().unwrap();

    let segments = read_segments(&report.segments_path);
    assert_eq!(segments.segments[0].start_host_ns, 1_000 * MS);
    assert_eq!(runs(&samples(&report.mic_path)), vec![(1, 3_200)]);
    assert_eq!(
        runs(&samples(&report.sys_path)),
        vec![(0, 2_400), (2, 160)],
        "150 ms of pad ahead of the system track's first frame"
    );
}

/// A device switch reopens both channels into the same files. The new
/// system track's frame 0 is 50 ms after the new microphone's, so it gets
/// 800 frames of pad, at the start of segment 1, after segment 0's audio,
/// which stays exactly where it was on both channels.
#[test]
fn a_reopened_segment_is_padded_at_its_own_start() {
    let tmp = tempfile::tempdir().unwrap();
    let mic = timed(Channel::Mic, 1, 160, 1_000 * MS);
    let sys = timed(Channel::System, 2, 160, 1_000 * MS);
    let mut session = RecordingSession::start(tmp.path().to_path_buf(), mic, Some(sys)).unwrap();

    session
        .reopen_with(
            segments::reason::DEFAULT_OUTPUT_DEVICE_CHANGED,
            || timed(Channel::Mic, 3, 320, 5_000 * MS),
            || Some(timed(Channel::System, 4, 160, 5_050 * MS)),
        )
        .unwrap();
    let report = session.stop().unwrap();

    assert_eq!(runs(&samples(&report.mic_path)), vec![(1, 160), (3, 320)]);
    assert_eq!(
        runs(&samples(&report.sys_path)),
        vec![(2, 160), (0, 800), (4, 160)],
        "the pad sits between the segments, not before segment 0"
    );
    let segments = read_segments(&report.segments_path);
    assert_eq!(segments.segments.len(), 2);
    assert_eq!(segments.segments[1].start_host_ns, 5_000 * MS);
    let close = segments.segments[0].anchors.last().unwrap();
    assert_eq!((close.mic_frames, close.sys_frames), (160, 160));
}
