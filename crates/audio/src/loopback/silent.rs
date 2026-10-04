//! Making one capture packet safe to resample (TUR-37).
//!
//! WASAPI flags some capture buffers `AUDCLNT_BUFFERFLAGS_SILENT`: the bytes
//! in them are to be ignored and the buffer treated as silence. `cpal` 0.18.2
//! does not pass that flag on (`wasapi/stream.rs`, `process_input` reads the
//! flags only for `DATA_DISCONTINUITY`), so today's `cpal` backend never sets
//! `silent`; whether those bytes are ever not zeros in practice is TUR-43's
//! to measure. A backend that can see the flag (the `wasapi` crate, a later
//! `cpal`) passes it, and this zero-fills the packet.
//!
//! Independently of the flag, a non-finite sample (NaN or infinity, which
//! garbage float bytes can be) becomes 0: one NaN through the sinc resampler
//! would turn every later output sample into NaN, and so the rest of the
//! track into noise or silence.

/// What [`condition`] changed.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Conditioned {
    /// The packet was flagged silent and zero-filled.
    pub zero_filled: bool,
    /// Non-finite samples replaced with 0 (0 when zero-filled).
    pub non_finite: usize,
}

/// Zero-fill `samples` when the OS flagged the packet `silent`, and replace
/// any non-finite sample with 0 otherwise. No allocation, so it runs in the
/// capture callback.
pub fn condition(samples: &mut [f32], silent: bool) -> Conditioned {
    if silent {
        samples.fill(0.0);
        return Conditioned {
            zero_filled: true,
            non_finite: 0,
        };
    }
    let mut non_finite = 0;
    for sample in samples.iter_mut().filter(|s| !s.is_finite()) {
        *sample = 0.0;
        non_finite += 1;
    }
    Conditioned {
        zero_filled: false,
        non_finite,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_silent_packet_is_zero_filled_whatever_it_held() {
        let mut samples = vec![0.5, -0.25, f32::NAN, 1.0];
        let done = condition(&mut samples, true);
        assert_eq!(samples, vec![0.0; 4]);
        assert_eq!(
            done,
            Conditioned {
                zero_filled: true,
                non_finite: 0
            }
        );
    }

    #[test]
    fn a_normal_packet_is_left_as_is() {
        let mut samples = vec![0.5, -0.25, 0.0, 1.0];
        let done = condition(&mut samples, false);
        assert_eq!(samples, vec![0.5, -0.25, 0.0, 1.0]);
        assert_eq!(done, Conditioned::default());
    }

    #[test]
    fn non_finite_samples_become_zero() {
        let mut samples = vec![0.5, f32::NAN, f32::INFINITY, f32::NEG_INFINITY, -0.5];
        let done = condition(&mut samples, false);
        assert_eq!(samples, vec![0.5, 0.0, 0.0, 0.0, -0.5]);
        assert_eq!(done.non_finite, 3);
        assert!(!done.zero_filled);
    }

    #[test]
    fn an_empty_packet_is_fine() {
        assert_eq!(condition(&mut [], true).non_finite, 0);
        assert_eq!(condition(&mut [], false), Conditioned::default());
    }
}
