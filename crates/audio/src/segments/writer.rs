//! The write side of `segments.json`: [`SegmentsWriter`] and [`SegmentOpen`].

use std::io;
use std::path::Path;

use meeting_format::segments::LiveSegments;

use super::{Anchor, SCHEMA_VERSION, Segment, Segments};
#[allow(unused_imports)] // named by intra-doc links only
use super::{CLOSE_ANCHOR_SLACK_MS, SegmentsDrift};

/// Fields fixed when a segment opens. Frame counts and anchors accumulate
/// afterward through [`SegmentsWriter`].
pub struct SegmentOpen {
    pub start_host_ns: u64,
    pub start_continuous_ns: Option<u64>,
    pub start_unix_ns: Option<u64>,
    pub mic_rate: u32,
    pub sys_rate: u32,
    pub mic_device_rate: Option<u32>,
    pub sys_device_rate: Option<u32>,
    pub reason: String,
}

/// The write side of `segments.json` — the counterpart to [`Segments`], which
/// only reads. `meet-rec` is the only thing that constructs one.
///
/// Nothing touches disk until [`SegmentsWriter::write_atomic`], which is the
/// §11/§7 checkpoint write: temp file, `fsync`, `rename(2)`, directory
/// `fsync`. The caller
/// sequences that as the *second* of the checkpoint's three writes — after
/// `WavWriter::fsync_data` on both channels, before `WavWriter::patch_header`
/// on either (see the `wav_writer` module docs and
/// [`SegmentsDrift::check_wav_header`] for why that order is the safe direction).
pub struct SegmentsWriter {
    segments: Vec<Segment>,
    /// Where each version is also published in memory, for the live
    /// transcript (TUR-164). Empty for `meet-rec`.
    shared: Vec<LiveSegments>,
}

impl SegmentsWriter {
    /// Start a new recording with its first segment.
    pub fn new(open: SegmentOpen) -> Self {
        Self {
            segments: vec![Self::segment(0, open)],
            shared: Vec::new(),
        }
    }

    /// Also publish every version this writer writes to `shared`, starting
    /// now (TUR-164). The in-memory copy gets it even when the disk write
    /// then fails: it describes what was captured, not what reached the disk.
    pub fn share_with(&mut self, shared: LiveSegments) {
        shared.publish(self.as_segments());
        self.shared.push(shared);
    }

    fn segment(idx: u32, open: SegmentOpen) -> Segment {
        Segment {
            idx,
            start_host_ns: open.start_host_ns,
            mic_rate: open.mic_rate,
            sys_rate: open.sys_rate,
            mic_frames: 0,
            sys_frames: 0,
            reason: open.reason,
            start_continuous_ns: open.start_continuous_ns,
            start_unix_ns: open.start_unix_ns,
            mic_device_rate: open.mic_device_rate,
            sys_device_rate: open.sys_device_rate,
            anchors: Vec::new(),
        }
    }

    fn current_mut(&mut self) -> &mut Segment {
        self.segments
            .last_mut()
            // quality: allow-unwrap `open` pushes the first segment; none are ever removed
            .expect("a writer always has at least one segment")
    }

    /// Update the current segment's running frame counts. Call this whenever
    /// frames are appended to either WAV — [`SegmentsWriter::write_atomic`]
    /// just needs the totals to already be current when it runs.
    pub fn update_frames(&mut self, mic_frames: u64, sys_frames: u64) {
        let seg = self.current_mut();
        seg.mic_frames = mic_frames;
        seg.sys_frames = sys_frames;
    }

    /// Append an ordinary checkpoint anchor to the current segment (§11).
    /// `anchor`'s frame counts must be the WAV-domain frame index each
    /// channel's last buffer *ended at* — not "frames flushed so far" (see
    /// the module docs on [`Anchor`]); anchoring on flush position would
    /// measure this writer's own latency instead of the device clock.
    pub fn checkpoint_anchor(&mut self, anchor: Anchor) {
        self.current_mut().anchors.push(anchor);
    }

    /// The current segment's latest anchor, or its start (frame 0 on both
    /// channels at `start_host_ns`) before it has one: the point the next
    /// checkpoint's frame gains are measured from.
    pub fn last_anchor(&self) -> Anchor {
        let seg = self.segments.last();
        let start = seg.map_or(0, |s| s.start_host_ns);
        seg.and_then(|s| s.anchors.last().copied())
            .unwrap_or(Anchor {
                mic_host_ns: start,
                mic_frames: 0,
                sys_host_ns: start,
                sys_frames: 0,
            })
    }

    /// Close the current segment and open the next one.
    ///
    /// `close_anchor` must be latched *after* the outgoing stream's ring
    /// buffer has been drained — [`CLOSE_ANCHOR_SLACK_MS`] is sized for
    /// exactly that drain. Latch it before draining (or skip it) and
    /// [`SegmentsDrift::drift`] will correctly refuse the segment later: from the
    /// reader's side, a missing close anchor is indistinguishable from a
    /// writer that never got the chance to run this method, which is exactly
    /// the failure F1 exists to catch.
    pub fn close_segment(&mut self, close_anchor: Anchor, next: SegmentOpen) {
        self.checkpoint_anchor(close_anchor);
        self.open_segment(next);
    }

    /// Open the next segment after the current one, whose last anchor is
    /// already its close anchor: the session latches that with
    /// [`SegmentsWriter::checkpoint_anchor`] and writes it to disk before its
    /// headers are patched, then opens the next segment once the new sources
    /// are aligned (TUR-162). Same result as [`SegmentsWriter::close_segment`].
    pub fn open_segment(&mut self, next: SegmentOpen) {
        let idx = self.segments.len() as u32;
        self.segments.push(Self::segment(idx, next));
    }

    /// A read-only snapshot of everything written so far — e.g. to run
    /// [`SegmentsDrift::check_wav_header`] mid-recording without a round trip
    /// through disk.
    pub fn as_segments(&self) -> Segments {
        Segments {
            version: SCHEMA_VERSION,
            segments: self.segments.clone(),
        }
    }

    /// Render the current state as `segments.json`.
    pub fn to_json(&self) -> serde_json::Result<String> {
        serde_json::to_string_pretty(&self.as_segments())
    }

    /// §7/§11's checkpoint write, through [`meeting_format::write_atomic`]: a
    /// temp file in the same directory, `fsync`, `rename(2)` over `path`, then
    /// `fsync` of the directory so the rename itself survives a power cut. The
    /// rename is atomic on APFS/HFS+, so a concurrent reader observes either
    /// the old file or the new one, never a torn one — that is what makes this
    /// safe to call on a live recording `crates/stt` might be tailing.
    pub fn write_atomic(&self, path: &Path) -> io::Result<()> {
        for shared in &self.shared {
            shared.publish(self.as_segments());
        }
        let json = self
            .to_json()
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
        meeting_format::write_atomic(path, json.as_bytes())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::segments::{DRIFT_GATE_MS, DriftError, SAMPLE_RATE_HZ, reason};

    fn temp_path(name: &str) -> (tempfile::TempDir, std::path::PathBuf) {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(name);
        (dir, path)
    }

    fn open(reason: &str, start_host_ns: u64) -> SegmentOpen {
        SegmentOpen {
            start_host_ns,
            start_continuous_ns: Some(start_host_ns),
            start_unix_ns: Some(1_790_000_000_000_000_000 + start_host_ns),
            mic_rate: SAMPLE_RATE_HZ,
            sys_rate: SAMPLE_RATE_HZ,
            mic_device_rate: Some(48_000),
            sys_device_rate: Some(48_000),
            reason: reason.into(),
        }
    }

    #[test]
    fn a_single_segment_written_to_disk_round_trips_and_measures_clean() {
        let mut writer = SegmentsWriter::new(open(reason::START, 1_000_000_000));
        writer.update_frames(80_000, 80_000);
        writer.checkpoint_anchor(Anchor {
            mic_host_ns: 6_000_000_000,
            mic_frames: 80_000,
            sys_host_ns: 6_000_000_000,
            sys_frames: 80_000,
        });

        let (_dir, path) = temp_path("single.json");
        writer.write_atomic(&path).unwrap();

        let on_disk = std::fs::read_to_string(&path).unwrap();
        let parsed = Segments::from_json(&on_disk).unwrap();
        assert_eq!(parsed, writer.as_segments());

        let report = parsed.drift().expect("a fully-anchored segment measures");
        assert!(report.passes(DRIFT_GATE_MS));
        assert_eq!(report.mic.anchors, 1);
    }

    #[test]
    fn last_anchor_is_the_segment_start_until_a_checkpoint_then_the_latest() {
        let mut writer = SegmentsWriter::new(open(reason::START, 7));
        let start = writer.last_anchor();
        assert_eq!((start.mic_host_ns, start.sys_host_ns), (7, 7));
        assert_eq!((start.mic_frames, start.sys_frames), (0, 0));

        let a = Anchor {
            mic_host_ns: 5_000_000_007,
            mic_frames: 80_000,
            sys_host_ns: 5_000_000_007,
            sys_frames: 79_990,
        };
        writer.checkpoint_anchor(a);
        assert_eq!(writer.last_anchor(), a);

        // A new segment measures from its own start again.
        writer.close_segment(a, open(reason::DEFAULT_OUTPUT_DEVICE_CHANGED, 9));
        assert_eq!(writer.last_anchor().mic_frames, 0);
        assert_eq!(writer.last_anchor().mic_host_ns, 9);
    }

    #[test]
    fn write_atomic_overwrites_the_previous_checkpoint_and_leaves_no_temp_file() {
        let mut writer = SegmentsWriter::new(open(reason::START, 0));
        writer.update_frames(10, 10);
        let (_dir, path) = temp_path("overwrite.json");
        writer.write_atomic(&path).unwrap();
        let first = std::fs::read_to_string(&path).unwrap();

        writer.update_frames(20, 20);
        writer.write_atomic(&path).unwrap();
        let second = std::fs::read_to_string(&path).unwrap();

        assert_ne!(first, second, "the checkpoint on disk did not advance");
        assert!(second.contains("\"mic_frames\": 20"));

        let leftovers: Vec<_> = std::fs::read_dir(path.parent().unwrap())
            .unwrap()
            .filter_map(|e| e.ok())
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .filter(|name| name.contains(".tmp."))
            .collect();
        assert!(
            leftovers.is_empty(),
            "temp checkpoint files were left behind: {leftovers:?}"
        );
    }

    #[test]
    fn closing_a_segment_latches_a_close_anchor_and_the_result_is_drift_measurable() {
        let mut writer = SegmentsWriter::new(open(reason::START, 1_000_000_000));
        // 60s of clean audio in segment 0, checkpointed once.
        writer.update_frames(960_000, 960_000);
        writer.checkpoint_anchor(Anchor {
            mic_host_ns: 1_000_000_000 + 60_000_000_000,
            mic_frames: 960_000,
            sys_host_ns: 1_000_000_000 + 60_000_000_000,
            sys_frames: 960_000,
        });

        // The device stopped delivering frames right after that last
        // checkpoint. The close anchor is latched at the drain — a few
        // hundred microseconds later, not after the gap — so it still reads
        // as ~0 drift; the 400ms unrecorded gap (SPEC A5 — never padded) is
        // what elapses *between* this anchor and the next segment's open.
        let last_frame_ns = 1_000_000_000 + 60_000_000_000;
        let close_anchor_ns = last_frame_ns + 500_000; // 0.5ms drain latency
        let next_segment_start_ns = close_anchor_ns + 400_000_000; // the gap
        writer.close_segment(
            Anchor {
                mic_host_ns: close_anchor_ns,
                mic_frames: 960_000,
                sys_host_ns: close_anchor_ns,
                sys_frames: 960_000,
            },
            open(reason::DEFAULT_OUTPUT_DEVICE_CHANGED, next_segment_start_ns),
        );
        writer.update_frames(160_000, 160_000);
        writer.checkpoint_anchor(Anchor {
            mic_host_ns: next_segment_start_ns + 10_000_000_000,
            mic_frames: 160_000,
            sys_host_ns: next_segment_start_ns + 10_000_000_000,
            sys_frames: 160_000,
        });

        let segments = writer.as_segments();
        assert_eq!(segments.segments.len(), 2);
        assert_eq!(segments.segments[0].anchors.len(), 2, "checkpoint + close");
        assert_eq!(segments.segments[1].idx, 1);

        let report = segments
            .drift()
            .expect("both segments are fully anchored, including the close anchor");
        assert!(report.passes(DRIFT_GATE_MS));

        let gaps = segments.boundary_gaps();
        assert_eq!(gaps.len(), 1);
        assert!(
            (gaps[0].mic_ms - 400.0).abs() < 1.0,
            "expected the unpadded 400ms gap, got {} ms",
            gaps[0].mic_ms
        );
    }

    #[test]
    fn a_segment_closed_without_latching_a_close_anchor_is_refused_like_a_dropped_marker() {
        // Regression for F1: skipping close_segment's anchor must not silently
        // pass drift() on the segment it abandoned.
        let mut writer = SegmentsWriter::new(open(reason::START, 0));
        writer.update_frames(80_000, 80_000);
        writer.checkpoint_anchor(Anchor {
            mic_host_ns: 5_000_000_000,
            mic_frames: 80_000,
            sys_host_ns: 5_000_000_000,
            sys_frames: 80_000,
        });
        // Segment kept recording another 10s past its last anchor before the
        // (hypothetical, buggy) writer tore it down without a close anchor.
        writer.update_frames(240_000, 240_000);

        let idx = 1;
        writer.segments.push(SegmentsWriter::segment(
            idx,
            open(reason::STREAM_RESTART, 10_000_000_000),
        ));

        let err = writer.as_segments().drift().unwrap_err();
        assert!(matches!(
            err,
            DriftError::AnchorCoverage {
                segment: 0,
                closed_deliberately: true,
                ..
            }
        ));
    }
}
