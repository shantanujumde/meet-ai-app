//! The hardware tier whisper's default model is picked by (TUR-61): is there a
//! GPU whisper can use, and how much memory does this machine have.
//!
//! Only the reading of the machine lives here. What to do with the answer is
//! [`crate::model::recommended`], a pure function tested on every OS. Which
//! GPU check runs is the OS seam's call ([`crate::platform`]): Apple silicon
//! on macOS, ggml's own device list (Vulkan) on Windows and Linux.

use std::path::Path;

/// Whether whisper can run on a GPU here.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Gpu {
    /// There is one, and whisper's build can use it.
    Usable,
    /// None that whisper's build can use: no GPU, no driver, or a build
    /// without a GPU backend.
    None,
    /// There is one, but whisper crashed on it in an earlier run
    /// ([`crate::gpu_guard`]), so it runs on the CPU.
    CrashedBefore,
}

/// The machine, as far as picking a model goes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Hardware {
    pub gpu: Gpu,
    pub total_memory_bytes: u64,
}

impl Hardware {
    /// Read this machine. `app_dir` is the `.app/` folder that holds the GPU
    /// crash marker; `None` skips that check.
    ///
    /// Off macOS this asks ggml for its devices, which starts the Vulkan
    /// loader in this process: the same thing loading a whisper model does
    /// first. Call it once and keep the answer.
    pub fn detect(app_dir: Option<&Path>) -> Self {
        let present = crate::platform::gpu_present();
        let crashed = app_dir.is_some_and(crate::gpu_guard::crashed_before);
        let mut system = sysinfo::System::new();
        system.refresh_memory();
        let hardware = Self {
            gpu: gpu(present, crashed),
            total_memory_bytes: system.total_memory(),
        };
        tracing::info!(gpu = ?hardware.gpu, memory_bytes = hardware.total_memory_bytes, "hardware tier");
        hardware
    }
}

fn gpu(present: bool, crashed: bool) -> Gpu {
    match (present, crashed) {
        (false, _) => Gpu::None,
        (true, true) => Gpu::CrashedBefore,
        (true, false) => Gpu::Usable,
    }
}

/// The GPUs ggml registered in this build, by name. Empty for a CPU-only
/// build, and when the Vulkan loader finds no driver (ggml catches that).
pub fn ggml_gpu_devices() -> Vec<String> {
    use whisper_rs::whisper_rs_sys as sys;

    // ggml logs while it registers its backends; route that through tracing
    // first, as loading a model does.
    crate::whisper::install_logging_hooks();
    let mut names = Vec::new();
    // SAFETY: the ggml device registry is built on first use and lives for the
    // process; these calls only read it. A null device is skipped, and the
    // name is a NUL-terminated string owned by ggml, copied out at once.
    unsafe {
        for index in 0..sys::ggml_backend_dev_count() {
            let device = sys::ggml_backend_dev_get(index);
            if device.is_null() {
                continue;
            }
            let kind = sys::ggml_backend_dev_type(device);
            if kind != sys::ggml_backend_dev_type_GGML_BACKEND_DEVICE_TYPE_GPU
                && kind != sys::ggml_backend_dev_type_GGML_BACKEND_DEVICE_TYPE_IGPU
            {
                continue;
            }
            let name = sys::ggml_backend_dev_description(device);
            names.push(if name.is_null() {
                format!("GPU {index}")
            } else {
                std::ffi::CStr::from_ptr(name)
                    .to_string_lossy()
                    .into_owned()
            });
        }
    }
    names
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_crash_marker_only_matters_when_there_is_a_gpu() {
        assert_eq!(gpu(true, false), Gpu::Usable);
        assert_eq!(gpu(true, true), Gpu::CrashedBefore);
        assert_eq!(gpu(false, true), Gpu::None);
        assert_eq!(gpu(false, false), Gpu::None);
    }

    #[test]
    fn detecting_this_machine_reads_its_memory_and_honours_the_marker() {
        let dir = tempfile::tempdir().unwrap();
        let clean = Hardware::detect(Some(dir.path()));
        assert!(clean.total_memory_bytes > 0);
        assert_ne!(clean.gpu, Gpu::CrashedBefore);

        // A crash in this version: arm, then never pass or drop.
        std::mem::forget(crate::gpu_guard::arm(dir.path()).guard);
        let after = Hardware::detect(Some(dir.path()));
        let expected = match clean.gpu {
            Gpu::Usable => Gpu::CrashedBefore,
            other => other,
        };
        assert_eq!(after.gpu, expected);
    }

    #[test]
    fn listing_ggml_gpus_never_panics() {
        // On a runner with no GPU driver this is empty; either way it returns.
        for name in ggml_gpu_devices() {
            assert!(!name.is_empty());
        }
    }
}
