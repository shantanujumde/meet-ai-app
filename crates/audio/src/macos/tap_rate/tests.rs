use super::*;

fn reported(aggregate: Option<f64>, output: Option<f64>, stream: Option<f64>) -> ReportedRates {
    ReportedRates {
        tap_format: 48_000,
        aggregate_nominal: aggregate,
        output_nominal: output,
        stream_virtual: stream,
    }
}

/// IO callbacks of `block` frames at a true `rate`, starting at `t0_ns`;
/// returns every published measurement.
fn run_meter(meter: &mut RateMeter, rate: f64, block: u64, secs: f64, t0_ns: u64) -> Vec<f64> {
    let callbacks = (secs * rate / block as f64) as u64;
    (0..callbacks)
        .filter_map(|n| {
            let host_ns = t0_ns + (n as f64 * block as f64 * 1e9 / rate) as u64;
            meter.observe(host_ns, block).map(|m| m.hz)
        })
        .collect()
}

#[test]
fn the_aggregate_rate_wins_over_the_stream_format() {
    // TUR-84, the owner's log: stream 48 kHz, aggregate and output 16 kHz.
    let rates = reported(Some(16_000.0), Some(16_000.0), Some(48_000.0));
    assert_eq!(effective_rate(rates, None), 16_000);
    let rates = reported(Some(44_100.0), Some(44_100.0), Some(48_000.0));
    assert_eq!(effective_rate(rates, None), 44_100);
}

#[test]
fn the_output_rate_comes_next_then_the_stream_then_the_tap_format() {
    assert_eq!(
        effective_rate(reported(None, Some(16_000.0), Some(48_000.0)), None),
        16_000
    );
    assert_eq!(
        effective_rate(reported(None, None, Some(24_000.0)), None),
        24_000
    );
    assert_eq!(effective_rate(reported(None, None, None), None), 48_000);
}

#[test]
fn nonsense_reads_fall_through_to_the_next_source() {
    assert_eq!(
        effective_rate(reported(Some(0.0), Some(f64::NAN), None), None),
        48_000
    );
    assert_eq!(
        effective_rate(reported(Some(-1.0), Some(44_100.0), None), None),
        44_100
    );
    assert_eq!(
        effective_rate(reported(Some(f64::INFINITY), None, Some(1e9)), None),
        48_000
    );
}

#[test]
fn fractional_rates_round_to_the_nearest_hertz() {
    assert_eq!(
        effective_rate(reported(Some(15_999.6), None, None), None),
        16_000
    );
}

#[test]
fn a_measurement_within_two_percent_keeps_the_reported_rate() {
    let rates = reported(Some(44_100.0), None, None);
    assert_eq!(effective_rate(rates, Some(44_500.0)), 44_100);
    assert_eq!(effective_rate(rates, Some(43_300.0)), 44_100);
}

#[test]
fn a_measurement_further_off_wins_and_snaps_to_a_standard_rate() {
    // Everything claims 48 kHz, frames come at 16 kHz.
    let all_48 = reported(Some(48_000.0), Some(48_000.0), Some(48_000.0));
    assert_eq!(effective_rate(all_48, Some(16_040.0)), 16_000);
    assert_eq!(effective_rate(all_48, Some(44_010.0)), 44_100);
    assert_eq!(effective_rate(all_48, Some(23_700.0)), 24_000);
    // Nonsense measurements are ignored.
    assert_eq!(effective_rate(all_48, Some(f64::NAN)), 48_000);
    assert_eq!(effective_rate(all_48, Some(0.0)), 48_000);
}

#[test]
fn snap_picks_the_nearest_standard_rate() {
    assert_eq!(snap(7_990.0), Some(8_000));
    assert_eq!(snap(22_000.0), Some(22_050));
    assert_eq!(snap(46_500.0), Some(48_000));
    assert_eq!(snap(46_000.0), Some(44_100));
    assert_eq!(snap(200_000.0), Some(96_000));
    assert_eq!(snap(10.0), None);
}

#[test]
fn the_meter_measures_the_delivered_rate_after_half_a_second() {
    for (rate, block) in [(16_000.0, 160), (44_100.0, 512), (48_000.0, 512)] {
        let mut meter = RateMeter::default();
        let published = run_meter(&mut meter, rate, block, 0.45, 1_000);
        assert!(published.is_empty(), "{rate}: nothing before 0.5 s");
        let mut meter = RateMeter::default();
        let published = run_meter(&mut meter, rate, block, 0.6, 1_000);
        assert_eq!(published.len(), 1, "{rate}");
        assert!(
            (published[0] - rate).abs() / rate < 0.005,
            "{rate}: measured {}",
            published[0]
        );
    }
}

#[test]
fn the_meter_publishes_a_change_only_when_two_windows_agree() {
    let mut meter = RateMeter::default();
    let first = run_meter(&mut meter, 44_100.0, 512, 2.0, 0);
    assert_eq!(first.len(), 1, "a steady rate is published once");
    assert_eq!(snap(first[0]), Some(44_100));
    // One odd window (a dropout) does not flip it...
    let t = 3_000_000_000;
    assert_eq!(meter.observe(t, 512), None);
    assert_eq!(meter.observe(t + 600_000_000, 512), None);
    // ...a sustained change does, after the second agreeing window.
    let changed = run_meter(&mut meter, 16_000.0, 160, 2.0, t + 700_000_000);
    assert_eq!(changed.len(), 1, "{changed:?}");
    assert_eq!(snap(changed[0]), Some(16_000));
}

#[test]
fn a_reset_measures_again_from_scratch() {
    let mut meter = RateMeter::default();
    assert_eq!(run_meter(&mut meter, 44_100.0, 512, 1.0, 0).len(), 1);
    meter.reset();
    let again = run_meter(&mut meter, 16_000.0, 160, 0.7, 5_000_000_000);
    assert_eq!(again.len(), 1, "the first window after a reset publishes");
    assert_eq!(snap(again[0]), Some(16_000));
}

#[test]
fn a_host_clock_going_backwards_restarts_the_window() {
    let mut meter = RateMeter::default();
    assert_eq!(meter.observe(1_000_000_000, 512), None);
    assert_eq!(meter.observe(10, 512), None);
    assert_eq!(meter.observe(20, 512), None);
}

#[test]
fn only_the_first_measurement_after_a_reset_is_marked_first() {
    let mut meter = RateMeter::default();
    let mut all = Vec::new();
    for n in 0..200u64 {
        let host_ns = n * 512 * 1_000_000_000 / 44_100;
        all.extend(meter.observe(host_ns, 512));
    }
    let t = 200 * 512 * 1_000_000_000 / 44_100;
    for n in 0..200u64 {
        all.extend(meter.observe(t + n * 160 * 1_000_000_000 / 16_000, 160));
    }
    let firsts: Vec<bool> = all.iter().map(|m| m.first).collect();
    assert_eq!(firsts, [true, false], "{all:?}");
    meter.reset();
    assert_eq!(meter.observe(0, 24_000), None);
    assert!(
        meter
            .observe(500_000_000, 512)
            .is_some_and(|m| m.first && snap(m.hz) == Some(48_000))
    );
}
