//! The pipeline whisper and Parakeet share (TUR-177): 16-bit samples to the
//! `f32` a model takes, a WAV's speech spans written to a sink, and a live
//! session's spans settled into lines. Each engine brings only its `decode`
//! closure: one span in, `(seconds on the recording, text)` lines out.

use std::path::Path;

use crate::session::{LiveEmitter, LiveListener, SessionOptions, SessionOutcome, SpanAssembler};
use crate::sink::TranscriptSink;
use crate::vad::{SAMPLE_RATE, SegmentConfig, SpeechSpan, Vad, detect_speech};
use crate::{Error, Speaker};

/// What a decode hands back: each line's start on the recording, and its text.
pub(crate) type Lines = Vec<(f64, String)>;

/// One span, ready for a model.
pub(crate) struct ModelSpan<'a> {
    /// The span as [`to_model_audio`] made it. A model that needs the
    /// samples by value may take them (`std::mem::take`); the next span
    /// fills the buffer again.
    pub audio: &'a mut Vec<f32>,
    /// The span's own length in seconds, before the padding.
    pub sec: f64,
    /// Seconds from the start of the recording to the span's first sample.
    pub start_sec: f64,
}

impl<'a> ModelSpan<'a> {
    /// Convert `samples` into `audio` and describe them.
    fn new(samples: &[i16], start_sec: f64, audio: &'a mut Vec<f32>) -> Self {
        to_model_audio(samples, audio);
        Self {
            audio,
            sec: samples.len() as f64 / SAMPLE_RATE as f64,
            start_sec,
        }
    }
}

/// Fill `audio` with `samples` as `f32` in `[-1, 1]`, replacing what was there.
///
/// Padded with silence to at least one second: whisper.cpp refuses anything
/// under about 1 s, and Parakeet's encoder subsamples 8 times, so a shorter
/// span would reach it as a handful of frames. Short real words would be
/// dropped otherwise. `audio` is the caller's scratch, so repeated calls
/// reuse its capacity.
pub(crate) fn to_model_audio(samples: &[i16], audio: &mut Vec<f32>) {
    audio.clear();
    audio.reserve(samples.len().max(SAMPLE_RATE as usize));
    audio.extend(
        samples
            .iter()
            .map(|sample| *sample as f32 / i16::MAX as f32),
    );
    if audio.len() < SAMPLE_RATE as usize {
        audio.resize(SAMPLE_RATE as usize, 0.0);
    }
}

/// The WAV at `wav` and the speech spans the detector found in it.
pub(crate) fn speech_spans(
    wav: &Path,
    vad: &mut dyn Vad,
    segmentation: &SegmentConfig,
) -> Result<(Vec<i16>, Vec<SpeechSpan>), Error> {
    let pcm = crate::read_wav_16k_mono(wav)?;
    let spans = detect_speech(&pcm, vad, segmentation);
    tracing::debug!(
        wav = %wav.display(),
        samples = pcm.len(),
        spans = spans.len(),
        "vad segmentation complete"
    );
    Ok((pcm, spans))
}

/// Decode every span of `pcm` and write its lines for `speaker`, then flush.
///
/// The hallucination guard's layer 1: no spans means `decode` is never
/// called, so the model never sees silence and nothing can be invented.
pub(crate) fn write_spans<D>(
    pcm: &[i16],
    spans: &[SpeechSpan],
    speaker: Speaker,
    sink: &mut dyn TranscriptSink,
    mut decode: D,
) -> Result<(), Error>
where
    D: FnMut(ModelSpan<'_>) -> Result<Lines, Error>,
{
    let mut audio = Vec::new();
    for &span in spans {
        let ready = ModelSpan::new(span.samples(pcm), span.start_sec(), &mut audio);
        for (start_sec, text) in decode(ready)? {
            sink.write_at(start_sec, speaker, text)?;
        }
    }
    sink.flush()
}

/// A live session's shared half: the spans the detector settles, the lines
/// they become, and the scratch every decode reuses.
pub(crate) struct LiveSpans {
    pub assembler: SpanAssembler,
    pub emitter: LiveEmitter,
    sink: Box<dyn TranscriptSink + Send>,
    /// Scratch for the `f32` copy a model takes, reused by every decode.
    pub audio: Vec<f32>,
}

impl LiveSpans {
    pub(crate) fn new(
        options: &SessionOptions,
        segmentation: SegmentConfig,
        sink: Box<dyn TranscriptSink + Send>,
        listener: Box<dyn LiveListener>,
    ) -> Self {
        Self {
            assembler: SpanAssembler::with_default_vad(segmentation),
            emitter: LiveEmitter::new(options, listener),
            sink,
            audio: Vec::new(),
        }
    }

    /// Settle every span `samples` completes into transcript lines. The
    /// caller polls the emitter afterwards ([`LiveEmitter::poll`]), once any
    /// guess of its own is in.
    pub(crate) fn feed<D>(&mut self, samples: &[i16], decode: &mut D) -> Result<(), Error>
    where
        D: FnMut(ModelSpan<'_>) -> Result<Lines, Error>,
    {
        for span in self.assembler.push(samples) {
            self.settle(&span.samples, span.start_sec, decode)?;
        }
        Ok(())
    }

    fn settle<D>(&mut self, samples: &[i16], start_sec: f64, decode: &mut D) -> Result<(), Error>
    where
        D: FnMut(ModelSpan<'_>) -> Result<Lines, Error>,
    {
        let ready = ModelSpan::new(samples, start_sec, &mut self.audio);
        for (start_sec, text) in decode(ready)? {
            self.emitter
                .finalize(start_sec, &text, self.sink.as_mut())?;
        }
        Ok(())
    }

    /// Settle the open span, then close the session.
    ///
    /// Whatever was still a guess stays a guess. `transcript.md` is
    /// append-only, so there is no version of promoting it that is not a
    /// line the user cannot get rid of.
    pub(crate) fn finish<D>(
        mut self,
        engine: &'static str,
        mut decode: D,
    ) -> Result<SessionOutcome, Error>
    where
        D: FnMut(ModelSpan<'_>) -> Result<Lines, Error>,
    {
        if let Some(span) = self.assembler.finish() {
            self.settle(&span.samples, span.start_sec, &mut decode)?;
        }
        let discarded_volatile = self.emitter.withdraw();
        self.sink.flush()?;
        Ok(SessionOutcome {
            speaker: self.emitter.speaker(),
            finalized: self.emitter.finalized(),
            discarded_volatile,
            audio_sec: self.assembler.fed_sec(),
            engine,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::CollectingSink;

    #[test]
    fn audio_conversion_is_pinned_and_padded_to_one_second() {
        let mut audio = Vec::new();
        to_model_audio(&[0, i16::MAX, i16::MIN, 16_384], &mut audio);
        assert_eq!(audio.len(), SAMPLE_RATE as usize);
        assert_eq!(audio[0], 0.0);
        assert_eq!(audio[1], 1.0);
        assert_eq!(audio[2], i16::MIN as f32 / i16::MAX as f32);
        assert_eq!(audio[3], 16_384.0 / i16::MAX as f32);
        assert!(audio[4..].iter().all(|s| *s == 0.0));

        let long = vec![100i16; 3 * SAMPLE_RATE as usize];
        to_model_audio(&long, &mut audio);
        assert_eq!(audio.len(), long.len(), "no padding past one second");
    }

    #[test]
    fn the_audio_scratch_is_reused_and_never_leaks_old_samples() {
        let mut audio = Vec::new();
        let long = vec![1_000i16; 5 * SAMPLE_RATE as usize];
        to_model_audio(&long, &mut audio);
        let capacity = audio.capacity();
        to_model_audio(&long, &mut audio);
        to_model_audio(&[7; 10], &mut audio);
        assert_eq!(audio.len(), SAMPLE_RATE as usize);
        assert!(audio[10..].iter().all(|s| *s == 0.0), "stale audio leaked");
        assert_eq!(audio.capacity(), capacity);
    }

    #[test]
    fn a_taken_scratch_is_filled_again() {
        let mut audio = Vec::new();
        to_model_audio(&[1; 10], &mut audio);
        let taken = std::mem::take(&mut audio);
        assert_eq!(taken.len(), SAMPLE_RATE as usize);
        to_model_audio(&[2; 10], &mut audio);
        assert_eq!(audio.len(), SAMPLE_RATE as usize);
        assert_eq!(audio[0], 2.0 / i16::MAX as f32);
    }

    #[test]
    fn each_span_is_decoded_once_and_its_lines_written_for_the_speaker() {
        let pcm = vec![0i16; 4 * SAMPLE_RATE as usize];
        let spans = [
            SpeechSpan {
                start_sample: 0,
                end_sample: 8_000,
            },
            SpeechSpan {
                start_sample: 32_000,
                end_sample: 64_000,
            },
        ];
        let mut sink = CollectingSink::default();
        let mut seen = Vec::new();
        write_spans(&pcm, &spans, Speaker::You, &mut sink, |span| {
            seen.push((span.start_sec, span.sec, span.audio.len()));
            Ok(vec![(span.start_sec, format!("at {}", span.start_sec))])
        })
        .unwrap();
        assert_eq!(seen, vec![(0.0, 0.5, 16_000), (2.0, 2.0, 32_000)]);
        let lines: Vec<(u64, Speaker, String)> = sink
            .utterances
            .iter()
            .map(|u| (u.start_sec, u.speaker, u.text.clone()))
            .collect();
        assert_eq!(
            lines,
            vec![
                (0, Speaker::You, "at 0".to_string()),
                (2, Speaker::You, "at 2".to_string())
            ]
        );
    }

    #[test]
    fn no_spans_never_calls_the_model() {
        let mut sink = CollectingSink::default();
        write_spans(&[0; 100], &[], Speaker::Others, &mut sink, |_| {
            panic!("decode ran on silence")
        })
        .unwrap();
        assert!(sink.utterances.is_empty());
    }
}
