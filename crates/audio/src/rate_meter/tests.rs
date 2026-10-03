use super::*;

/// `secs` of callbacks of `block` frames at a true `rate`, from `t0_ns`.
fn run(meter: &mut CallbackMeter<FixedRates>, rate: f64, block: usize, secs: f64, t0_ns: u64) {
    let callbacks = (secs * rate / block as f64) as u64;
    for n in 0..callbacks {
        let host_ns = t0_ns + (n as f64 * block as f64 * 1e9 / rate) as u64;
        meter.observe(host_ns, block * 2);
    }
}

#[test]
fn decide_keeps_the_reported_rate_within_tolerance_and_snaps_beyond_it() {
    assert_eq!(decide(48_000, None), 48_000);
    assert_eq!(decide(48_000, Some(47_700.0)), 48_000);
    assert_eq!(decide(48_000, Some(16_030.0)), 16_000);
    assert_eq!(decide(16_000, Some(44_000.0)), 44_100);
    assert_eq!(decide(48_000, Some(f64::NAN)), 48_000);
}

/// TUR-87 M1: `cpal` says 48 kHz, a headset mic in a call delivers 16 kHz.
#[test]
fn a_microphone_delivering_another_rate_than_it_reported_is_detected() {
    let rates = FixedRates::new("microphone", 48_000);
    let mut meter = CallbackMeter::new(Arc::clone(&rates), 2);
    assert_eq!(
        rates.effective(),
        48_000,
        "the reported rate until measured"
    );

    run(&mut meter, 16_000.0, 160, 1.0, 1_000);

    assert!(rates.cell().take_change(), "the worker is told");
    assert!(rates.cell().first());
    assert_eq!(rates.effective(), 16_000);
    let line = rates.describe();
    assert!(line.contains("microphone device 48000 Hz"), "{line}");
    assert!(line.contains("effective_rate 16000 Hz"), "{line}");
}

#[test]
fn a_microphone_at_its_reported_rate_keeps_it() {
    let rates = FixedRates::new("microphone", 48_000);
    let mut meter = CallbackMeter::new(Arc::clone(&rates), 2);
    run(&mut meter, 48_000.0, 512, 1.0, 0);
    assert_eq!(rates.effective(), 48_000);
}

#[test]
fn a_restart_forgets_the_measurement_and_measures_again() {
    let rates = FixedRates::new("microphone", 48_000);
    let mut meter = CallbackMeter::new(Arc::clone(&rates), 2);
    run(&mut meter, 16_000.0, 160, 1.0, 0);
    assert_eq!(rates.effective(), 16_000);

    rates.cell().restart();
    assert_eq!(rates.cell().get(), None);
    assert_eq!(rates.effective(), 48_000);
    run(&mut meter, 24_000.0, 240, 0.7, 5_000_000_000);
    assert_eq!(rates.effective(), 24_000);
    assert!(rates.cell().first(), "first again after the restart");
}
