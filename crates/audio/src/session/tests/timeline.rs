//! TUR-164: the live transcript places its lines by the same `segments.json`
//! the batch path reads, so each tee's feed must see every version the
//! session writes, from the moment the first segment opens.

use super::*;

#[test]
fn each_feed_sees_segments_json_as_the_session_writes_it() {
    let tmp = tempfile::tempdir().unwrap();
    let (mic_tee, mic_feed) = crate::tee::tee();
    let (sys_tee, sys_feed) = crate::tee::tee();
    let mic: Box<dyn AudioSource> = Box::new(StubSource::new(Channel::Mic));
    let sys: Box<dyn AudioSource> = Box::new(StubSource::new(Channel::System));

    let session = RecordingSession::start_with_tees(
        tmp.path().to_path_buf(),
        mic,
        Some(sys),
        Tees {
            mic: Some(mic_tee),
            sys: Some(sys_tee),
        },
    )
    .expect("session starts cleanly");

    // Published as soon as the first segment opens, before any checkpoint:
    // a line said in the first five seconds is placed by it too.
    let opened = mic_feed
        .timeline()
        .with(|segments| segments.clone())
        .expect("published at start");
    assert_eq!(opened.segments.len(), 1);
    assert_eq!(opened.segments[0].reason, segments::reason::START);

    let report = session.stop().expect("session stops cleanly");
    let on_disk = read_segments(&report.segments_path);
    for feed in [&mic_feed, &sys_feed] {
        let live = feed.timeline().with(|segments| segments.clone());
        assert_eq!(
            live.as_ref(),
            Some(&on_disk),
            "the feed missed the last write"
        );
    }
}

#[test]
fn a_session_without_tees_publishes_nowhere_and_records_the_same() {
    // `meet-rec` has no tees: the writer has nothing to share with, and a
    // feed that was never attached stays empty.
    let (_unused_tee, feed) = crate::tee::tee();
    let tmp = tempfile::tempdir().unwrap();
    let mic: Box<dyn AudioSource> = Box::new(StubSource::new(Channel::Mic));
    let session =
        RecordingSession::start(tmp.path().to_path_buf(), mic, None).expect("session starts");
    session.stop().expect("session stops");
    assert_eq!(feed.timeline().with(|_| ()), None);
}
