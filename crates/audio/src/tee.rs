//! A second consumer of a channel's 16 kHz frames, fed alongside the WAV.
//!
//! TUR-31 settled how live transcription reaches a running recording: the
//! capture side hands out a *copy* of each channel's resampled frames, one
//! stream per channel, and the speech side decides what to do with them. This
//! is that copy. It is what `src-tauri`'s live transcript (TUR-96) reads from.
//!
//! The three rules TUR-31 set, and how this module holds them:
//!
//! * **Never on the Core Audio IO callback.** [`Tee::offer`] is called from each
//!   source's worker thread, after the WAV append and after its writer lock is
//!   released — the same place the samples come out of the resampler. The IO
//!   callback still only timestamps and pushes into its ring.
//! * **Never blocks; on overflow, transcription loses frames, the WAV never
//!   does.** The queue is bounded and [`Tee::offer`] uses `try_send`. A full
//!   queue (the speech engine fell behind) or a gone receiver (it died) costs
//!   the recording nothing.
//! * **The WAV write path does not change.** A source with no tee set writes
//!   byte-for-byte what it wrote before this module existed.
//!
//! One thing TUR-31 left implicit and this module makes exact: frames dropped
//! on overflow are **not** silently skipped. Skipping would slide every later
//! `start_sec` earlier than the audio it describes, and SPEC §3.4 makes those
//! timestamps permanent. The dropped count is carried forward and delivered as
//! that many zeros at the front of the next chunk that fits, so sample N of the
//! tee is always sample N of the WAV. The speech side hears a gap of silence
//! where it fell behind — which its VAD already treats as nothing — rather
//! than a timeline that has quietly drifted.

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, SyncSender, TryRecvError, TrySendError};
use std::time::Duration;

/// How many chunks the queue holds before it starts dropping.
///
/// A chunk is one resampler output — [`crate::resample`] takes 1024
/// device-rate frames at a time, so ~341 frames (~21 ms) at 48 kHz. 1024 of
/// them is ~20 s of audio: far more than a healthy engine ever lags (the Apple
/// sidecar measured ~24× real time on TUR-31), and small enough that a wedged
/// one costs a bounded ~11 MB rather than a four-hour meeting's worth of RAM.
pub const DEFAULT_CAPACITY_CHUNKS: usize = 1024;

/// What travels down the queue: the frames, plus how many frames of silence
/// are owed ahead of them. The gap is a count rather than a buffer of zeros so
/// that a sender whose receiver is wedged does not rebuild an ever-growing
/// buffer on every refused offer — the zeros are only materialised once, on
/// the receiving side, when there is finally somewhere for them to go.
#[derive(Debug)]
struct Chunk {
    gap: u64,
    frames: Vec<i16>,
}

impl Chunk {
    fn into_frames(self) -> Vec<i16> {
        if self.gap == 0 {
            return self.frames;
        }
        let mut out = vec![0; self.gap as usize];
        out.extend_from_slice(&self.frames);
        out
    }
}

/// The sending half, handed to an [`crate::AudioSource`] via
/// [`crate::AudioSource::tee`]. Cheap to clone; a segment reopen hands the
/// same tee to the rebuilt source.
#[derive(Debug, Clone)]
pub struct Tee {
    tx: SyncSender<Chunk>,
    /// Frames dropped since the last chunk that fit, owed as leading silence
    /// on the next one. Shared between clones so a reopen cannot lose it.
    owed: Arc<AtomicU64>,
    /// Every frame ever dropped, for the log line at the end of a meeting.
    dropped: Arc<AtomicU64>,
}

/// The receiving half. Owned by whatever feeds a speech session.
#[derive(Debug)]
pub struct TeeFeed {
    rx: Receiver<Chunk>,
    dropped: Arc<AtomicU64>,
}

/// A connected [`Tee`]/[`TeeFeed`] pair with [`DEFAULT_CAPACITY_CHUNKS`].
pub fn tee() -> (Tee, TeeFeed) {
    tee_with_capacity(DEFAULT_CAPACITY_CHUNKS)
}

/// A connected pair with an explicit queue depth, in chunks. Tests use a tiny
/// one to exercise overflow without pushing a thousand chunks.
pub fn tee_with_capacity(chunks: usize) -> (Tee, TeeFeed) {
    let (tx, rx) = mpsc::sync_channel(chunks.max(1));
    let dropped = Arc::new(AtomicU64::new(0));
    (
        Tee {
            tx,
            owed: Arc::new(AtomicU64::new(0)),
            dropped: Arc::clone(&dropped),
        },
        TeeFeed { rx, dropped },
    )
}

impl Tee {
    /// Hand over a copy of `frames`. Never blocks.
    ///
    /// Allocates one `Vec` per call, which is fine where this is called from —
    /// a worker thread that already allocates per chunk — and would not be on
    /// the IO callback, which is exactly why it is never called from there.
    pub fn offer(&self, frames: &[i16]) {
        if frames.is_empty() {
            return;
        }
        self.send(frames.to_vec(), 0);
    }

    /// Hand over `frames` of silence — the tee's half of
    /// [`crate::AudioSource::pad_leading_silence`], so the head-pad the WAV
    /// gets is on the tee's timeline too.
    ///
    /// The pad lands *after* whatever the source already offered, where the
    /// WAV puts it at the very front. That misplaces at most the first
    /// resampler chunk (~21 ms) by the pad length; every frame after it is at
    /// the same index in both, which is what timestamps depend on.
    pub fn offer_silence(&self, frames: u64) {
        if frames == 0 {
            return;
        }
        self.send(Vec::new(), frames);
    }

    /// `silence` is extra gap on top of whatever is already owed.
    fn send(&self, frames: Vec<i16>, silence: u64) {
        let owed = self.owed.swap(0, Ordering::AcqRel);
        let chunk = Chunk {
            gap: owed + silence,
            frames,
        };
        match self.tx.try_send(chunk) {
            Ok(()) => {}
            Err(TrySendError::Full(chunk)) => {
                // Previously-owed frames were counted as dropped when they
                // were first refused; only this call's frames are news.
                let fresh = chunk.frames.len() as u64 + silence;
                self.dropped.fetch_add(fresh, Ordering::Relaxed);
                self.owed
                    .fetch_add(chunk.gap + chunk.frames.len() as u64, Ordering::AcqRel);
            }
            // Nobody is listening any more — the speech side stopped or died.
            // That is its business; the recording carries on unchanged.
            Err(TrySendError::Disconnected(_)) => {}
        }
    }
}

impl TeeFeed {
    /// The next chunk, waiting at most `timeout`, with any owed silence
    /// already in front of it.
    ///
    /// `Disconnected` means every [`Tee`] is gone and everything they sent has
    /// been delivered: the recording stopped.
    pub fn recv_timeout(&self, timeout: Duration) -> Result<Vec<i16>, RecvTimeoutError> {
        self.rx.recv_timeout(timeout).map(Chunk::into_frames)
    }

    /// The next chunk if one is already queued.
    pub fn try_recv(&self) -> Result<Vec<i16>, TryRecvError> {
        self.rx.try_recv().map(Chunk::into_frames)
    }

    /// Frames refused because the queue was full, over this tee's whole life.
    /// Zero on any healthy run; logged when it is not.
    pub fn dropped_frames(&self) -> u64 {
        self.dropped.load(Ordering::Relaxed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn offered_frames_arrive_unchanged_and_in_order() {
        let (tee, feed) = tee();
        tee.offer(&[1, 2, 3]);
        tee.offer(&[4, 5]);
        assert_eq!(feed.try_recv().unwrap(), [1, 2, 3]);
        assert_eq!(feed.try_recv().unwrap(), [4, 5]);
        assert_eq!(feed.dropped_frames(), 0);
    }

    #[test]
    fn an_empty_offer_sends_nothing() {
        let (tee, feed) = tee();
        tee.offer(&[]);
        tee.offer_silence(0);
        assert!(matches!(feed.try_recv(), Err(TryRecvError::Empty)));
    }

    #[test]
    fn overflow_is_carried_forward_as_silence_so_the_timeline_never_slides() {
        let (tee, feed) = tee_with_capacity(1);
        tee.offer(&[1, 1]); // fits
        tee.offer(&[2, 2, 2]); // queue full: dropped, owed
        tee.offer(&[3]); // still full: dropped, owed

        assert_eq!(feed.try_recv().unwrap(), [1, 1]);
        assert_eq!(feed.dropped_frames(), 4);

        tee.offer(&[9, 9]);
        // Sample index in the feed still equals sample index in the WAV: the
        // four frames that never made it are four zeros, then the new ones.
        assert_eq!(feed.try_recv().unwrap(), [0, 0, 0, 0, 9, 9]);
        assert_eq!(feed.dropped_frames(), 4, "owed frames are not re-counted");
    }

    #[test]
    fn silence_is_delivered_as_zeros() {
        let (tee, feed) = tee();
        tee.offer(&[7]);
        tee.offer_silence(3);
        assert_eq!(feed.try_recv().unwrap(), [7]);
        assert_eq!(feed.try_recv().unwrap(), [0, 0, 0]);
    }

    #[test]
    fn a_gone_receiver_costs_the_sender_nothing() {
        // The independence property from TUR-31: the speech side dying must
        // never reach capture. No panic, no block, no error to handle.
        let (tee, feed) = tee_with_capacity(1);
        drop(feed);
        for _ in 0..10 {
            tee.offer(&[1, 2, 3]);
        }
        tee.offer_silence(100);
    }

    /// Overflow, as a property. A source numbers every sample it writes
    /// (never zero), offers chunks of varying size into a small queue, and
    /// the reader drains at random moments. However the two interleave:
    ///
    /// * every sample the reader sees is either the WAV's sample at that
    ///   exact index, or a zero standing in for one that was dropped — the
    ///   timeline never slides;
    /// * the reader's count plus what is still owed is exactly the WAV's
    ///   count;
    /// * `dropped_frames` counts each dropped sample once.
    #[test]
    fn under_any_overflow_the_tee_timeline_matches_the_wav_sample_for_sample() {
        // xorshift: deterministic, and no dev-dependency for one test.
        let mut state = 0x9E37_79B9_7F4A_7C15_u64;
        let mut next = move |below: u64| {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            state % below
        };

        for round in 0..200 {
            let (tee, feed) = tee_with_capacity(1 + next(4) as usize);
            let mut wav: Vec<i16> = Vec::new();
            let mut heard: Vec<i16> = Vec::new();

            for _ in 0..(20 + next(80)) {
                if next(5) == 0 {
                    let pad = next(50);
                    tee.offer_silence(pad);
                    wav.extend(std::iter::repeat_n(0, pad as usize));
                } else {
                    let len = 1 + next(40) as usize;
                    let chunk: Vec<i16> = (0..len)
                        .map(|i| ((wav.len() + i) % 30_000 + 1) as i16)
                        .collect();
                    tee.offer(&chunk);
                    wav.extend_from_slice(&chunk);
                }
                // The reader falls behind and catches up in bursts.
                if next(3) == 0 {
                    for _ in 0..next(4) {
                        match feed.try_recv() {
                            Ok(chunk) => heard.extend(chunk),
                            Err(_) => break,
                        }
                    }
                }
            }
            let owed = tee.owed.load(Ordering::Acquire);
            drop(tee);
            while let Ok(chunk) = feed.try_recv() {
                heard.extend(chunk);
            }

            assert_eq!(
                heard.len() as u64 + owed,
                wav.len() as u64,
                "round {round}: the tee and the WAV disagree on length"
            );
            let mut real_in_place = 0u64;
            for (index, (&got, &want)) in heard.iter().zip(&wav).enumerate() {
                if got == want {
                    real_in_place += u64::from(want != 0);
                } else {
                    assert_eq!(
                        got, 0,
                        "round {round}: sample {index} slid (got {got}, WAV has {want})"
                    );
                }
            }
            // Every real sample that did not arrive in place was dropped, and
            // nothing that did arrive in place was. (A dropped pad is zeros
            // either way, which is why these are bounds, not one count.)
            let wav_real = wav.iter().filter(|&&s| s != 0).count() as u64;
            let dropped = feed.dropped_frames();
            assert!(
                dropped >= wav_real - real_in_place,
                "round {round}: a dropped sample went uncounted ({dropped})"
            );
            assert!(
                dropped <= wav.len() as u64 - real_in_place,
                "round {round}: a sample was counted as dropped twice ({dropped})"
            );
        }
    }

    #[test]
    fn the_feed_disconnects_once_every_tee_is_dropped() {
        let (tee, feed) = tee();
        let reopened = tee.clone();
        tee.offer(&[1]);
        drop(tee);
        reopened.offer(&[2]);
        drop(reopened);

        assert_eq!(feed.recv_timeout(Duration::ZERO).unwrap(), [1]);
        assert_eq!(feed.recv_timeout(Duration::ZERO).unwrap(), [2]);
        assert!(matches!(
            feed.recv_timeout(Duration::ZERO),
            Err(RecvTimeoutError::Disconnected)
        ));
    }
}
