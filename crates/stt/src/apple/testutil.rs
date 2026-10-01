//! Shared fixtures for the Apple engine's silence-gate tests.

use crate::vad::{SAMPLE_RATE, SpeechTimeline};

// --- the silence gate, without a sidecar ---
//
// The lines below are the sidecar's real output shape. The room-tone one is
// copied from `meet-stt room-tone-30s.wav --volatile` on macOS 27.0, run
// against the seeded fixture (`seed=1` in `generate.sh`). The
// detector is scripted, because what is under test is the wiring: that
// Apple's results are held against the detector at all, on both paths.

/// Scores 0.9 for frames inside `speech`, 0.0 elsewhere.
pub(super) struct SpeechAt {
    speech: std::ops::Range<usize>,
    next: usize,
}

impl crate::vad::Vad for SpeechAt {
    fn score(&mut self, _frame: &[i16]) -> f32 {
        let score = if self.speech.contains(&self.next) {
            0.9
        } else {
            0.0
        };
        self.next += 1;
        score
    }

    fn reset(&mut self) {
        self.next = 0;
    }
}

const FRAMES_PER_SEC: usize = SAMPLE_RATE as usize / crate::vad::FRAME_SAMPLES;

/// 30 s of audio, with speech only in the given whole seconds.
pub(super) fn heard(speech_secs: std::ops::Range<usize>) -> SpeechTimeline {
    let vad = SpeechAt {
        speech: speech_secs.start * FRAMES_PER_SEC..speech_secs.end * FRAMES_PER_SEC,
        next: 0,
    };
    let mut timeline = SpeechTimeline::new(crate::vad::SegmentConfig::default(), Box::new(vad));
    timeline.push(&vec![0; SAMPLE_RATE as usize * 30]);
    timeline
}

pub(super) const ROOM_TONE: &str = r#"{"type":"volatile","start_sec":0,"end_sec":30,"text":"I"}
{"type":"final","start_sec":0,"end_sec":3.84,"text":"I"}
{"type":"done","duration_sec":30}
"#;

pub(super) const ROOM_TONE_THEN_SPEECH: &str = r#"{"type":"final","start_sec":0,"end_sec":3.84,"text":"I"}
{"type":"volatile","start_sec":20.0,"end_sec":21.0,"text":"about two"}
{"type":"final","start_sec":20.0,"end_sec":23.4,"text":"About two days."}
{"type":"done","duration_sec":30}
"#;
