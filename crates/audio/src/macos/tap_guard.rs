//! The Core Audio objects a process tap is built from, each destroyed when
//! its guard drops (TUR-162).
//!
//! `SystemSource::build` creates a process tap, then a private aggregate
//! device carrying it, then an IO proc on that aggregate. Before this, each
//! error path destroyed what came before by hand, and two did not (the tap
//! format read and the WAV open), leaking the tap and the aggregate; and a
//! build that finished after its start had timed out was dropped with
//! everything still running. Holding each object in a guard makes every
//! early return and every drop tear down what exists, in reverse order:
//! IO proc, then aggregate, then tap (locals drop in reverse, and `Built`
//! declares its fields in teardown order).

use objc2_core_audio::{
    self as ca, AudioDeviceIOProcID, AudioHardwareDestroyAggregateDevice,
    AudioHardwareDestroyProcessTap, AudioObjectID,
};

/// A process tap, destroyed on drop.
pub(super) struct TapObject(pub(super) AudioObjectID);

impl Drop for TapObject {
    fn drop(&mut self) {
        // SAFETY: `self.0` is a tap this guard's creator made and nothing
        // else destroys.
        let status = unsafe { AudioHardwareDestroyProcessTap(self.0) };
        if status != 0 {
            tracing::warn!(
                "destroying process tap {} failed: OSStatus {status}",
                self.0
            );
        }
    }
}

/// A private aggregate device, destroyed on drop.
pub(super) struct AggregateDevice(pub(super) AudioObjectID);

impl Drop for AggregateDevice {
    fn drop(&mut self) {
        // SAFETY: as for `TapObject`: ours, and destroyed only here.
        let status = unsafe { AudioHardwareDestroyAggregateDevice(self.0) };
        if status != 0 {
            tracing::warn!(
                "destroying aggregate device {} failed: OSStatus {status}",
                self.0
            );
        }
    }
}

/// An IO proc on an aggregate device: stopped (once started) and destroyed
/// on drop.
pub(super) struct IoProc {
    aggregate_id: AudioObjectID,
    id: AudioDeviceIOProcID,
    started: bool,
}

impl IoProc {
    /// Guard the IO proc `id`, created on `aggregate_id` and not started yet.
    pub(super) fn new(aggregate_id: AudioObjectID, id: AudioDeviceIOProcID) -> Self {
        Self {
            aggregate_id,
            id,
            started: false,
        }
    }

    /// `AudioDeviceStart`, returning its status; the drop stops it only if
    /// this succeeded.
    pub(super) fn start(&mut self) -> i32 {
        // SAFETY: the aggregate and the proc are live; the guards own both.
        let status = unsafe { ca::AudioDeviceStart(self.aggregate_id, self.id) };
        self.started = status == 0;
        status
    }
}

impl Drop for IoProc {
    fn drop(&mut self) {
        // SAFETY: as for `start`, and the aggregate guard outlives this one.
        unsafe {
            if self.started {
                ca::AudioDeviceStop(self.aggregate_id, self.id);
            }
            ca::AudioDeviceDestroyIOProcID(self.aggregate_id, self.id);
        }
    }
}
