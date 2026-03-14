// Memory-mapped file IPC bridge for Tauri ↔ APO communication.
//
// Tauri writes SharedConfig, reads SharedStatus.
// Mirror of the APO DLL's mmap_ipc.rs structures.

use std::sync::atomic::{AtomicBool, AtomicU8, AtomicU32, AtomicU64, Ordering};
use std::sync::OnceLock;

#[cfg(windows)]
use windows::Win32::Foundation::{HANDLE, CloseHandle, INVALID_HANDLE_VALUE};
#[cfg(windows)]
use windows::Win32::System::Memory::*;
#[cfg(windows)]
use windows::core::PCWSTR;

#[repr(C)]
pub struct SharedConfig {
    pub version: AtomicU32,
    pub enabled: AtomicBool,
    pub compensation_strength_u32: AtomicU32,
    pub hf_reconstruction_u32: AtomicU32,
    pub dynamics_restoration_u32: AtomicU32,
    pub transient_repair_u32: AtomicU32,
    pub phase_mode: AtomicU8,
    pub quality_preset: AtomicU8,
    pub synthesis_mode: AtomicU8,  // 0=LegacyAdditive, 1=FftOlaPilot, 2=FftOlaFull
}

#[repr(C)]
pub struct SharedStatus {
    pub frame_count: AtomicU64,
    pub current_cutoff_u32: AtomicU32,
    pub current_quality_tier: AtomicU8,
    pub current_clipping_u32: AtomicU32,
    pub processing_load_u32: AtomicU32,
}

struct Bridge {
    #[cfg(windows)]
    _config_handle: HANDLE,
    #[cfg(windows)]
    _status_handle: HANDLE,
    config_ptr: *mut SharedConfig,
    status_ptr: *mut SharedStatus,
}

unsafe impl Send for Bridge {}
unsafe impl Sync for Bridge {}

static BRIDGE: OnceLock<Bridge> = OnceLock::new();

const MMAP_CONFIG_NAME: &str = "Global\\ASCE_SharedConfig_v1";
const MMAP_STATUS_NAME: &str = "Global\\ASCE_SharedStatus_v1";

pub fn init() {
    #[cfg(windows)]
    {
        let _ = BRIDGE.get_or_init(|| {
            match create_bridge() {
                Ok(b) => b,
                Err(e) => {
                    eprintln!("ASCE IPC bridge init failed: {e}");
                    // Create a dummy bridge with null pointers
                    Bridge {
                        _config_handle: HANDLE::default(),
                        _status_handle: HANDLE::default(),
                        config_ptr: std::ptr::null_mut(),
                        status_ptr: std::ptr::null_mut(),
                    }
                }
            }
        });
    }
}

/// Write config values to shared memory (Tauri → APO)
pub fn write_config(
    enabled: bool,
    strength: f32,
    hf_reconstruction: f32,
    dynamics: f32,
    transient: f32,
    phase_mode: u8,
    quality_preset: u8,
    synthesis_mode: u8,
) {
    if let Some(bridge) = BRIDGE.get() {
        if bridge.config_ptr.is_null() { return; }
        let config = unsafe { &*bridge.config_ptr };
        config.enabled.store(enabled, Ordering::Relaxed);
        config.compensation_strength_u32.store((strength * 10000.0) as u32, Ordering::Relaxed);
        config.hf_reconstruction_u32.store((hf_reconstruction * 10000.0) as u32, Ordering::Relaxed);
        config.dynamics_restoration_u32.store((dynamics * 10000.0) as u32, Ordering::Relaxed);
        config.transient_repair_u32.store((transient * 10000.0) as u32, Ordering::Relaxed);
        config.phase_mode.store(phase_mode, Ordering::Relaxed);
        config.quality_preset.store(quality_preset, Ordering::Relaxed);
        config.synthesis_mode.store(synthesis_mode, Ordering::Relaxed);
        config.version.fetch_add(1, Ordering::Release);
    }
}

/// Read status from shared memory (APO → Tauri)
pub fn read_status() -> Option<StatusSnapshot> {
    let bridge = BRIDGE.get()?;
    if bridge.status_ptr.is_null() { return None; }
    let status = unsafe { &*bridge.status_ptr };
    Some(StatusSnapshot {
        frame_count: status.frame_count.load(Ordering::Relaxed),
        cutoff_freq: f32::from_bits(status.current_cutoff_u32.load(Ordering::Relaxed)),
        quality_tier: status.current_quality_tier.load(Ordering::Relaxed),
        clipping: f32::from_bits(status.current_clipping_u32.load(Ordering::Relaxed)),
        processing_load: f32::from_bits(status.processing_load_u32.load(Ordering::Relaxed)),
    })
}

#[derive(serde::Serialize, Clone)]
pub struct StatusSnapshot {
    pub frame_count: u64,
    pub cutoff_freq: f32,
    pub quality_tier: u8,
    pub clipping: f32,
    pub processing_load: f32,
}

#[cfg(windows)]
fn create_bridge() -> Result<Bridge, String> {
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
        ).map_err(|e| format!("Config mmap: {e}"))?;

        let config_view = MapViewOfFile(
            config_handle, FILE_MAP_ALL_ACCESS, 0, 0,
            std::mem::size_of::<SharedConfig>(),
        );
        if config_view.Value.is_null() {
            let _ = CloseHandle(config_handle);
            return Err("Config MapViewOfFile failed".into());
        }

        let status_handle = CreateFileMappingW(
            INVALID_HANDLE_VALUE,
            None,
            PAGE_READWRITE,
            0,
            std::mem::size_of::<SharedStatus>() as u32,
            PCWSTR(status_name.as_ptr()),
        ).map_err(|e| format!("Status mmap: {e}"))?;

        let status_view = MapViewOfFile(
            status_handle, FILE_MAP_ALL_ACCESS, 0, 0,
            std::mem::size_of::<SharedStatus>(),
        );
        if status_view.Value.is_null() {
            let _ = CloseHandle(config_handle);
            let _ = CloseHandle(status_handle);
            return Err("Status MapViewOfFile failed".into());
        }

        Ok(Bridge {
            _config_handle: config_handle,
            _status_handle: status_handle,
            config_ptr: config_view.Value as *mut SharedConfig,
            status_ptr: status_view.Value as *mut SharedStatus,
        })
    }
}

#[cfg(windows)]
fn to_wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}
