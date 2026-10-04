//! Putting silence back where the capture callback found a gap (TUR-37).
//!
//! The callback cannot write silence into the sample ring itself: a gap of
//! minutes would not fit, and the callback must not block. It pushes a
//! [`GapMark`] instead, "this many frames of silence go before sample N",
//! into a second small ring, always *before* the samples that follow the gap.
//! The worker pops samples first and marks second, so every mark for the
//! samples it holds has arrived, and [`splice`] interleaves the two.

use std::collections::VecDeque;

/// Silence owed at one point of the sample stream.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct GapMark {
    /// Interleaved samples pushed before the silence goes in.
    pub at_sample: u64,
    /// Frames (not samples) of silence.
    pub frames: u64,
}

/// One run of output: samples as they came, or silence.
#[derive(Debug, PartialEq)]
pub(crate) enum Piece<'a> {
    Samples(&'a [f32]),
    Silence(u64),
}

/// Hand `samples`, whose first sample is number `start` of the stream, to
/// `emit` with every pending mark that falls inside it (or right at its end)
/// spliced in. Marks past its end stay in `pending` for the next call; an
/// empty `samples` emits the marks that are due at `start`.
pub(crate) fn splice<'a>(
    start: u64,
    samples: &'a [f32],
    pending: &mut VecDeque<GapMark>,
    mut emit: impl FnMut(Piece<'a>),
) {
    let end = start + samples.len() as u64;
    let mut offset = 0;
    while let Some(mark) = pending.front().copied() {
        if mark.at_sample > end {
            break;
        }
        let at = (mark.at_sample.saturating_sub(start) as usize).max(offset);
        if at > offset {
            emit(Piece::Samples(&samples[offset..at]));
            offset = at;
        }
        if mark.frames > 0 {
            emit(Piece::Silence(mark.frames));
        }
        pending.pop_front();
    }
    if offset < samples.len() {
        emit(Piece::Samples(&samples[offset..]));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(start: u64, samples: &[f32], pending: &mut VecDeque<GapMark>) -> Vec<String> {
        let mut out = Vec::new();
        splice(start, samples, pending, |piece| {
            out.push(match piece {
                Piece::Samples(s) => format!("s{:?}", s),
                Piece::Silence(f) => format!("z{f}"),
            })
        });
        out
    }

    fn marks(list: &[(u64, u64)]) -> VecDeque<GapMark> {
        list.iter()
            .map(|&(at_sample, frames)| GapMark { at_sample, frames })
            .collect()
    }

    #[test]
    fn no_marks_passes_samples_through() {
        let mut pending = VecDeque::new();
        assert_eq!(run(0, &[1.0, 2.0], &mut pending), vec!["s[1.0, 2.0]"]);
    }

    #[test]
    fn a_mark_inside_the_slice_splits_it() {
        let mut pending = marks(&[(12, 5)]);
        assert_eq!(
            run(10, &[1.0, 2.0, 3.0, 4.0], &mut pending),
            vec!["s[1.0, 2.0]", "z5", "s[3.0, 4.0]"]
        );
        assert!(pending.is_empty());
    }

    #[test]
    fn marks_at_the_start_and_the_end_are_emitted_in_place() {
        let mut pending = marks(&[(10, 1), (12, 2)]);
        assert_eq!(
            run(10, &[1.0, 2.0], &mut pending),
            vec!["z1", "s[1.0, 2.0]", "z2"]
        );
    }

    #[test]
    fn a_later_mark_waits_for_its_samples() {
        let mut pending = marks(&[(20, 3)]);
        assert_eq!(run(10, &[1.0, 2.0], &mut pending), vec!["s[1.0, 2.0]"]);
        assert_eq!(pending.len(), 1);
        // Nothing new popped yet: still not due.
        assert!(run(12, &[], &mut pending).is_empty());
        assert_eq!(run(20, &[], &mut pending), vec!["z3"]);
    }

    #[test]
    fn two_marks_at_one_point_are_both_emitted() {
        let mut pending = marks(&[(1, 2), (1, 3)]);
        assert_eq!(
            run(0, &[1.0, 2.0], &mut pending),
            vec!["s[1.0]", "z2", "z3", "s[2.0]"]
        );
    }

    #[test]
    fn a_mark_already_behind_goes_out_first() {
        // Cannot happen with marks pushed before their samples, but must not
        // panic or reorder samples if it did.
        let mut pending = marks(&[(5, 1)]);
        assert_eq!(run(10, &[1.0], &mut pending), vec!["z1", "s[1.0]"]);
    }
}
