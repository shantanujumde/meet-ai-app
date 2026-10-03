use super::*;

/// A fake `AudioBufferList`: per buffer, its `mNumberChannels` and samples.
struct Abl(Vec<(u32, Vec<f32>)>);

impl Abl {
    fn gather(&self, layout: TapBuffers, width: usize) -> Vec<f32> {
        let mut out = Vec::new();
        let range = layout.range(self.0.len());
        gather_into(range, |i| (self.0[i].0, &self.0[i].1[..]), width, &mut out);
        out
    }
}

const NON_INTERLEAVED: u32 = kAudioFormatFlagIsNonInterleaved;

#[test]
fn a_non_interleaved_stereo_tap_alone_alternates_channels() {
    let abl = Abl(vec![(1, vec![1.0, 2.0, 3.0]), (1, vec![10.0, 20.0, 30.0])]);
    let layout = TapBuffers::for_format(2, NON_INTERLEAVED);
    assert_eq!(abl.gather(layout, 2), vec![1.0, 10.0, 2.0, 20.0, 3.0, 30.0]);
}

#[test]
fn a_headset_mic_buffer_before_the_tap_is_left_out() {
    // HFP headset: the output device's mono mic comes first, then the
    // tap's two planar buffers. Before TUR-87 all three were interleaved.
    let mic = (1, vec![0.9f32; 4]);
    let abl = Abl(vec![
        mic,
        (1, vec![1.0, 2.0, 3.0, 4.0]),
        (1, vec![5.0, 6.0, 7.0, 8.0]),
    ]);
    let layout = TapBuffers::for_format(2, NON_INTERLEAVED);
    let out = abl.gather(layout, 2);
    assert_eq!(out, vec![1.0, 5.0, 2.0, 6.0, 3.0, 7.0, 4.0, 8.0]);
    assert!(
        !out.contains(&0.9),
        "headset mic leaked into the system track"
    );
}

#[test]
fn a_mono_mic_next_to_an_interleaved_stereo_tap_keeps_the_tap_whole() {
    // The mis-pairing case: a 3-frame mono buffer and a 3-frame interleaved
    // stereo buffer (6 samples). Truncating to the shortest buffer used to
    // keep half the tap's frames and pair them with the mic.
    let abl = Abl(vec![
        (1, vec![0.9, 0.9, 0.9]),
        (2, vec![1.0, 10.0, 2.0, 20.0, 3.0, 30.0]),
    ]);
    let layout = TapBuffers::for_format(2, 0);
    assert_eq!(abl.gather(layout, 2), vec![1.0, 10.0, 2.0, 20.0, 3.0, 30.0]);
}

#[test]
fn a_tap_with_fewer_channels_than_expected_repeats_the_last_one() {
    let abl = Abl(vec![(1, vec![1.0, 2.0])]);
    assert_eq!(
        abl.gather(TapBuffers::for_format(1, 0), 2),
        vec![1.0, 1.0, 2.0, 2.0]
    );
}

#[test]
fn channels_past_the_expected_width_are_dropped() {
    let abl = Abl(vec![(3, vec![1.0, 2.0, 3.0, 4.0, 5.0, 6.0])]);
    assert_eq!(
        abl.gather(TapBuffers::for_format(3, 0), 2),
        vec![1.0, 2.0, 4.0, 5.0]
    );
}

#[test]
fn a_zero_channel_count_reads_as_mono() {
    let abl = Abl(vec![(0, vec![1.0, 2.0])]);
    assert_eq!(abl.gather(TapBuffers::for_format(1, 0), 1), vec![1.0, 2.0]);
}

#[test]
fn the_shortest_selected_buffer_sets_the_frame_count() {
    let abl = Abl(vec![(1, vec![1.0, 2.0, 3.0]), (1, vec![10.0, 20.0])]);
    let layout = TapBuffers::for_format(2, NON_INTERLEAVED);
    assert_eq!(abl.gather(layout, 2), vec![1.0, 10.0, 2.0, 20.0]);
}

#[test]
fn empty_input_is_empty() {
    assert!(
        Abl(vec![])
            .gather(TapBuffers::for_format(2, 0), 2)
            .is_empty()
    );
    let empty = Abl(vec![(1, vec![]), (1, vec![])]);
    assert!(
        empty
            .gather(TapBuffers::for_format(2, NON_INTERLEAVED), 2)
            .is_empty()
    );
}

#[test]
fn a_reused_buffer_does_not_leak_stale_tail_samples() {
    let mut out = vec![9.0f32; 32];
    let bufs = [vec![1.0f32, 2.0], vec![10.0, 20.0]];
    gather_into(0..2, |i| (1, &bufs[i][..]), 2, &mut out);
    assert_eq!(out, vec![1.0, 10.0, 2.0, 20.0]);
}

#[test]
fn a_shorter_list_than_the_tap_needs_uses_what_is_there() {
    let layout = TapBuffers::for_format(2, NON_INTERLEAVED);
    assert_eq!(layout.range(1), 0..1);
    assert_eq!(layout.range(3), 1..3);
}

#[test]
fn the_layout_probe_reports_the_first_callback_once() {
    let probe = LayoutProbe::new(TapBuffers::for_format(2, NON_INTERLEAVED));
    assert_eq!(probe.take_report(), None, "nothing seen yet");
    probe.observe(3, |i| [1, 1, 1][i]);
    probe.observe(5, |_| 7);
    let report = probe.take_report().expect("one report");
    assert!(report.contains("mNumberBuffers 3"), "{report}");
    assert!(report.contains("[1, 1, 1]"), "{report}");
    assert!(report.contains("buffers 1..3"), "{report}");
    assert_eq!(probe.take_report(), None, "only once");
}
