//! macOS: `NSWorkspaceWillSleepNotification` (TUR-145).
//!
//! Apple: "An observer of this message can delay sleep for up to 30 seconds
//! while handling this notification." It is posted on the main thread, and
//! with no queue given the block runs right there, so the stop runs inline
//! and the Mac sleeps once it has returned. Inline on purpose, not on a
//! worker the main thread waits for: the stop's own state events reach
//! listeners (the menu bar's label) that hop to the main thread and wait, so
//! a main thread blocked on a worker would hold both until a timeout.
//! The stop's own waits are bounded (a tick in flight, then up to
//! `live_transcript::STOP_TIMEOUT` for the transcript), so it stays well
//! inside Apple's 30 seconds. The notification is registered on
//! `NSWorkspace`'s own center, the only one it is posted on.

use std::ptr::NonNull;

use block2::RcBlock;
use objc2_app_kit::{NSWorkspace, NSWorkspaceWillSleepNotification};
use objc2_foundation::NSNotification;

use super::Handler;

pub(super) fn on_will_sleep(handler: Handler) -> Result<(), String> {
    let center = NSWorkspace::sharedWorkspace().notificationCenter();
    let block = RcBlock::new(move |_note: NonNull<NSNotification>| {
        tracing::info!("the Mac is going to sleep");
        handler();
    });
    // SAFETY: the name is AppKit's own constant, there is no object filter,
    // and with no queue the block runs on the posting thread (AppKit posts
    // this on the main thread). The block only calls a `Send + Sync` handler.
    let observer = unsafe {
        center.addObserverForName_object_queue_usingBlock(
            Some(NSWorkspaceWillSleepNotification),
            None,
            None,
            &block,
        )
    };
    // The center keeps the block for as long as the observer is registered,
    // which is the life of the app: never removed.
    std::mem::forget(observer);
    Ok(())
}
