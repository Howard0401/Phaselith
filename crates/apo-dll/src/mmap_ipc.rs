// Memory-mapped file IPC between Tauri control panel and APO DLL.
//
// SharedConfig: Tauri writes, APO reads (atomic loads)
// SharedStatus: APO writes, Tauri reads (atomic stores)

use std::sync::atomic::{AtomicBool, AtomicU8, AtomicU32, AtomicU64, Ordering};

#[cfg(windows)]
use windows::Win32::Foundation::{HANDLE, CloseHandle, INVALID_HANDLE_VALUE};
#[cfg(windows)]
use windows::Win32::System::Memory::*;
#[cfg(windows)]
use windows::core::PCWSTR;

/// Shared config written by Tauri, read by APO (lock-free via atomics)
#[repr(C)]
pub struct SharedConfig {
    pub version: AtomicU32,
    pub enabled: AtomicBool,
    pub compensation_strength_u32: AtomicU32,  // f32 * 10000 → u32
    pub hf_reconstruction_u32: AtomicU32,
    pub dynamics_restoration_u32: AtomicU32,
    pub transient_repair_u32: AtomicU32,
    pub phase_mode: AtomicU8,      // 0=Linear, 1=Minimum
    pub quality_preset: AtomicU8,  // 0=Light, 1=Standard, 2=Ultra
}

impl SharedConfig {
    pub fn compensation_strength(&self) -> f32 {
        self.compensation_strength_u32.load(Ordering::Relaxed) as f32 / 10000.0
    }

    pub fn hf_reconstruction(&self) -> f32 {
        self.hf_reconstruction_u32.load(Ordering::Relaxed) as f32 / 10000.0
    }

    pub fn dynamics_restoration(&self) -> f32 {
        self.dynamics_restoration_u32.load(Ordering::Relaxed) as f32 / 10000.0
    }

    pub fn transient_repair(&self) -> f32 {
        self.transient_repair_u32.load(Ordering::Relaxed) as f32 / 10000.0
    }

    pub fn is_enabled(&self) -> bool {
        self.enabled.load(Ordering::Relaxed)
    }
}

/// Shared status written by APO, read by Tauri
#[repr(C)]
pub struct SharedStatus {
    pub frame_count: AtomicU64,
    pub current_cutoff_u32: AtomicU32,       // f32 bits
    pub current_quality_tier: AtomicU8,
    pub current_clipping_u32: AtomicU32,     // f32 bits
    pub processing_load_u32: AtomicU32,      // f32 bits (percent)
}

impl SharedStatus {
    pub fn set_cutoff(&self, freq: Option<f32>) {
        let bits = freq.unwrap_or(0.0).to_bits();
        self.current_cutoff_u32.store(bits, Ordering::Relaxed);
    }

    pub fn set_clipping(&self, severity: f32) {
        self.current_clipping_u32.store(severity.to_bits(), Ordering::Relaxed);
    }

    pub fn set_processing_load(&self, percent: f32) {
        self.processing_load_u32.store(percent.to_bits(), Ordering::Relaxed);
    }

    pub fn increment_frames(&self) {
        self.frame_count.fetch_add(1, Ordering::Relaxed);
    }
}

const MMAP_CONFIG_NAME: &str = "Global\\ASCE_SharedConfig_v1";
const MMAP_STATUS_NAME: &str = "Global\\ASCE_SharedStatus_v1";

/// Manages memory-mapped file IPC
pub struct MmapIpc {
    #[cfg(windows)]
    config_handle: HANDLE,
    #[cfg(windows)]
    status_handle: HANDLE,
    config_ptr: *mut SharedConfig,
    status_ptr: *mut SharedStatus,
}

unsafe impl Send for MmapIpc {}
unsafe impl Sync for MmapIpc {}

#[cfg(windows)]
impl MmapIpc {
    /// Open or create the shared memory regions.
    /// Called during APO initialization (not on real-time thread).
    pub fn open_or_create() -> Result<Self, String> {
        unsafe {
            let config_name = to_wide(MMAP_CONFIG_NAME);
            let status_name = to_wide(MMAP_STATUS_NAME);

            let config_handle = CreateFileMappingW(
                INVALID_HANDLE_VALUE,
                None,
                PAGE_READWRITE,
                0,
                std::mem::size_of::<SharedConfig>() as u32,
                PCWSTR(config_name.as_ptr()),
            ).map_err(|e| format!("CreateFileMapping config: {e}"))?;

            let config_view = MapViewOfFile(
                config_handle,
                FILE_MAP_ALL_ACCESS,
                0,
                0,
                std::mem::size_of::<SharedConfig>(),
            );
            if config_view.Value.is_null() {
                let _ = CloseHandle(config_handle);
                return Err("MapViewOfFile config failed".into());
            }

            let status_handle = CreateFileMappingW(
                INVALID_HANDLE_VALUE,
                None,
                PAGE_READWRITE,
                0,
                std::mem::size_of::<SharedStatus>() as u32,
                PCWSTR(status_name.as_ptr()),
            ).map_err(|e| format!("CreateFileMapping status: {e}"))?;

            let status_view = MapViewOfFile(
                status_handle,
                FILE_MAP_ALL_ACCESS,
                0,
                0,
                std::mem::size_of::<SharedStatus>(),
            );
            if status_view.Value.is_null() {
                let _ = UnmapViewOfFile(config_view);
                let _ = CloseHandle(config_handle);
                let _ = CloseHandle(status_handle);
                return Err("MapViewOfFile status failed".into());
            }

            Ok(Self {
                config_handle,
                status_handle,
                config_ptr: config_view.Value as *mut SharedConfig,
                status_ptr: status_view.Value as *mut SharedStatus,
            })
        }
    }

    /// Get reference to shared config (lock-free atomic reads).
    /// Safe to call from real-time thread.
    pub fn config(&self) -> &SharedConfig {
        unsafe { &*self.config_ptr }
    }

    /// Get reference to shared status (lock-free atomic writes).
    /// Safe to call from real-time thread.
    pub fn status(&self) -> &SharedStatus {
        unsafe { &*self.status_ptr }
    }
}

#[cfg(windows)]
impl Drop for MmapIpc {
    fn drop(&mut self) {
        unsafe {
            let _ = UnmapViewOfFile(MEMORY_MAPPED_VIEW_ADDRESS {
                Value: self.config_ptr as *mut _,
            });
            let _ = UnmapViewOfFile(MEMORY_MAPPED_VIEW_ADDRESS {
                Value: self.status_ptr as *mut _,
            });
            let _ = CloseHandle(self.config_handle);
            let _ = CloseHandle(self.status_handle);
        }
    }
}

#[cfg(windows)]
fn to_wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}
