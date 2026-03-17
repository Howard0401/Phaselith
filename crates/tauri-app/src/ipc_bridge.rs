// Memory-mapped file IPC bridge for Tauri ↔ APO communication.
//
// Tauri writes SharedConfig, reads SharedStatus.
// Mirror of the APO DLL's mmap_ipc.rs structures.
//
// Uses FILE-BACKED mmap at C:\ProgramData\Phaselith\{shared_config,shared_status}.bin.
// Both sides independently CreateFileW + CreateFileMappingW on the same file.
// No Global\ namespace needed → works without SeCreateGlobalPrivilege.
// No chicken-and-egg: either side can create the file first.

use std::sync::atomic::{AtomicBool, AtomicU8, AtomicU32, AtomicU64, Ordering};
use std::sync::Mutex;

#[cfg(windows)]
use windows::Win32::Foundation::{HANDLE, CloseHandle, GENERIC_READ, GENERIC_WRITE};
#[cfg(windows)]
use windows::Win32::Storage::FileSystem::*;
#[cfg(windows)]
use windows::Win32::System::Memory::*;
#[cfg(windows)]
use windows::core::HSTRING;

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
    pub filter_style: AtomicU8,   // 0=Reference, 1=Warm, 2=BassPlus, 3=Custom
    // ─── 6-axis StyleConfig (for Custom filter_style) ───
    pub warmth_u32: AtomicU32,
    pub air_brightness_u32: AtomicU32,
    pub smoothness_u32: AtomicU32,
    pub spatial_spread_u32: AtomicU32,
    pub impact_gain_u32: AtomicU32,
    pub body_u32: AtomicU32,
}

#[repr(C)]
pub struct SharedStatus {
    pub frame_count: AtomicU64,
    pub current_cutoff_u32: AtomicU32,
    pub current_quality_tier: AtomicU8,
    pub current_clipping_u32: AtomicU32,
    pub processing_load_u32: AtomicU32,
    pub wet_dry_diff_db_u32: AtomicU32,
    pub pop_muted_count: AtomicU32,
}

const SHARED_DIR: &str = r"C:\ProgramData\Phaselith";
const CONFIG_FILE: &str = r"C:\ProgramData\Phaselith\shared_config.bin";
const STATUS_FILE: &str = r"C:\ProgramData\Phaselith\shared_status.bin";

// Error codes (Tauri side, T prefix):
// IPC-T001: Failed to create shared directory
// IPC-T002: Failed to CreateFileW (config)
// IPC-T003: Failed to CreateFileMappingW (config)
// IPC-T004: Failed to MapViewOfFile (config)
// IPC-T005: Failed to CreateFileW (status)
// IPC-T006: Failed to CreateFileMappingW (status)
// IPC-T007: Failed to MapViewOfFile (status)

struct Bridge {
    #[cfg(windows)]
    config_file_handle: HANDLE,
    #[cfg(windows)]
    config_mapping_handle: HANDLE,
    #[cfg(windows)]
    status_file_handle: HANDLE,
    #[cfg(windows)]
    status_mapping_handle: HANDLE,
    config_ptr: *mut SharedConfig,
    status_ptr: *mut SharedStatus,
}

unsafe impl Send for Bridge {}
unsafe impl Sync for Bridge {}

#[cfg(windows)]
impl Drop for Bridge {
    fn drop(&mut self) {
        unsafe {
            if !self.config_ptr.is_null() {
                let _ = UnmapViewOfFile(MEMORY_MAPPED_VIEW_ADDRESS {
                    Value: self.config_ptr as *mut _,
                });
            }
            if !self.status_ptr.is_null() {
                let _ = UnmapViewOfFile(MEMORY_MAPPED_VIEW_ADDRESS {
                    Value: self.status_ptr as *mut _,
                });
            }
            if !self.config_mapping_handle.is_invalid() {
                let _ = CloseHandle(self.config_mapping_handle);
            }
            if !self.config_file_handle.is_invalid() {
                let _ = CloseHandle(self.config_file_handle);
            }
            if !self.status_mapping_handle.is_invalid() {
                let _ = CloseHandle(self.status_mapping_handle);
            }
            if !self.status_file_handle.is_invalid() {
                let _ = CloseHandle(self.status_file_handle);
            }
        }
    }
}

static BRIDGE: Mutex<Option<Bridge>> = Mutex::new(None);

/// Write all default config values to a freshly-created mmap.
/// Ensures the APO starts with correct parameters instead of all-zeros.
fn write_defaults(config: &SharedConfig) {
    use std::sync::atomic::Ordering::Relaxed;
    config.enabled.store(true, Relaxed);
    config.compensation_strength_u32.store(7000, Relaxed);  // 0.7
    config.hf_reconstruction_u32.store(8000, Relaxed);       // 0.8
    config.dynamics_restoration_u32.store(6000, Relaxed);    // 0.6
    config.transient_repair_u32.store(5000, Relaxed);        // 0.5
    config.phase_mode.store(0, Relaxed);                     // Linear
    config.quality_preset.store(1, Relaxed);                 // Standard
    config.synthesis_mode.store(0, Relaxed);                 // LegacyAdditive
    config.filter_style.store(0, Relaxed);                   // Reference
    // Reference preset style axes
    config.warmth_u32.store(1500, Relaxed);                  // 0.15
    config.air_brightness_u32.store(5000, Relaxed);          // 0.50
    config.smoothness_u32.store(4000, Relaxed);              // 0.40
    config.spatial_spread_u32.store(3000, Relaxed);          // 0.30
    config.impact_gain_u32.store(1500, Relaxed);             // 0.15
    config.body_u32.store(4000, Relaxed);                    // 0.40
    // Version bump LAST — Release ordering
    config.version.fetch_add(1, std::sync::atomic::Ordering::Release);
}

pub fn init() {
    #[cfg(windows)]
    {
        let mut guard = BRIDGE.lock().unwrap();
        if guard.is_none() {
            match create_bridge() {
                Ok(b) => {
                    eprintln!("Phaselith IPC bridge: connected (file-backed mmap)");
                    // Push full default config so APO starts with correct values.
                    // Without this, freshly-created mmap has all zeros → wrong parameters.
                    if !b.config_ptr.is_null() {
                        write_defaults(unsafe { &*b.config_ptr });
                    }
                    *guard = Some(b);
                }
                Err(e) => {
                    eprintln!("Phaselith IPC bridge init failed: {e}");
                }
            }
        }
    }
}

/// Reconnect IPC bridge — call after Install.
/// With file-backed mmap this always succeeds (both sides open same file).
pub fn reconnect() -> bool {
    #[cfg(windows)]
    {
        let mut guard = BRIDGE.lock().unwrap();
        *guard = None;
        match create_bridge() {
            Ok(b) => {
                eprintln!("Phaselith IPC bridge: reconnected (file-backed mmap)");
                // Push full default config on reconnect (same reason as init)
                if !b.config_ptr.is_null() {
                    write_defaults(unsafe { &*b.config_ptr });
                }
                *guard = Some(b);
                true
            }
            Err(e) => {
                eprintln!("Phaselith IPC bridge reconnect failed: {e}");
                false
            }
        }
    }
    #[cfg(not(windows))]
    false
}

/// Write config values to shared memory (Tauri → APO).
/// All atomic stores happen BEFORE version.fetch_add(Release) — APO sees
/// consistent state when it detects the version change (Acquire).
pub fn write_config(
    enabled: bool,
    strength: f32,
    hf_reconstruction: f32,
    dynamics: f32,
    transient: f32,
    phase_mode: u8,
    quality_preset: u8,
    synthesis_mode: u8,
    filter_style: u8,
    warmth: f32,
    air_brightness: f32,
    smoothness: f32,
    spatial_spread: f32,
    impact_gain: f32,
    body: f32,
) {
    let guard = BRIDGE.lock().unwrap();
    if let Some(bridge) = guard.as_ref() {
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
        config.filter_style.store(filter_style, Ordering::Relaxed);
        // 6-axis style parameters (used when filter_style==3 Custom)
        config.warmth_u32.store((warmth * 10000.0) as u32, Ordering::Relaxed);
        config.air_brightness_u32.store((air_brightness * 10000.0) as u32, Ordering::Relaxed);
        config.smoothness_u32.store((smoothness * 10000.0) as u32, Ordering::Relaxed);
        config.spatial_spread_u32.store((spatial_spread * 10000.0) as u32, Ordering::Relaxed);
        config.impact_gain_u32.store((impact_gain * 10000.0) as u32, Ordering::Relaxed);
        config.body_u32.store((body * 10000.0) as u32, Ordering::Relaxed);
        // Version bump LAST — Release ordering ensures all stores above are visible
        config.version.fetch_add(1, Ordering::Release);
    }
}

/// Read config from shared memory (for restoring state on Tauri restart).
/// Returns None if mmap is not connected.
pub fn read_config() -> Option<crate::commands::ConfigResponse> {
    let guard = BRIDGE.lock().unwrap();
    let bridge = guard.as_ref()?;
    if bridge.config_ptr.is_null() { return None; }
    let config = unsafe { &*bridge.config_ptr };
    Some(crate::commands::ConfigResponse {
        enabled: config.enabled.load(Ordering::Relaxed),
        strength: config.compensation_strength_u32.load(Ordering::Relaxed) as f32 / 10000.0,
        hf_reconstruction: config.hf_reconstruction_u32.load(Ordering::Relaxed) as f32 / 10000.0,
        dynamics: config.dynamics_restoration_u32.load(Ordering::Relaxed) as f32 / 10000.0,
        transient: config.transient_repair_u32.load(Ordering::Relaxed) as f32 / 10000.0,
        phase_mode: config.phase_mode.load(Ordering::Relaxed),
        quality_preset: config.quality_preset.load(Ordering::Relaxed),
        synthesis_mode: config.synthesis_mode.load(Ordering::Relaxed),
        filter_style: config.filter_style.load(Ordering::Relaxed),
        warmth: config.warmth_u32.load(Ordering::Relaxed) as f32 / 10000.0,
        air_brightness: config.air_brightness_u32.load(Ordering::Relaxed) as f32 / 10000.0,
        smoothness: config.smoothness_u32.load(Ordering::Relaxed) as f32 / 10000.0,
        spatial_spread: config.spatial_spread_u32.load(Ordering::Relaxed) as f32 / 10000.0,
        impact_gain: config.impact_gain_u32.load(Ordering::Relaxed) as f32 / 10000.0,
        body: config.body_u32.load(Ordering::Relaxed) as f32 / 10000.0,
    })
}

/// Read status from shared memory (APO → Tauri)
pub fn read_status() -> Option<StatusSnapshot> {
    let guard = BRIDGE.lock().unwrap();
    let bridge = guard.as_ref()?;
    if bridge.status_ptr.is_null() { return None; }
    let status = unsafe { &*bridge.status_ptr };
    Some(StatusSnapshot {
        frame_count: status.frame_count.load(Ordering::Relaxed),
        cutoff_freq: f32::from_bits(status.current_cutoff_u32.load(Ordering::Relaxed)),
        quality_tier: status.current_quality_tier.load(Ordering::Relaxed),
        clipping: f32::from_bits(status.current_clipping_u32.load(Ordering::Relaxed)),
        processing_load: f32::from_bits(status.processing_load_u32.load(Ordering::Relaxed)),
        wet_dry_diff_db: f32::from_bits(status.wet_dry_diff_db_u32.load(Ordering::Relaxed)),
        pop_muted_count: status.pop_muted_count.load(Ordering::Relaxed),
    })
}

/// Disconnect the bridge (e.g. after uninstall).
/// Drops all handles so read_status() returns None → UI shows Disconnected.
pub fn disconnect() {
    let mut guard = BRIDGE.lock().unwrap();
    *guard = None;
    eprintln!("Phaselith IPC bridge: disconnected");
}

/// Check if connected
pub fn is_connected() -> bool {
    let guard = BRIDGE.lock().unwrap();
    guard.as_ref().map_or(false, |b| !b.config_ptr.is_null())
}

#[derive(serde::Serialize, Clone)]
pub struct StatusSnapshot {
    pub frame_count: u64,
    pub cutoff_freq: f32,
    pub quality_tier: u8,
    pub clipping: f32,
    pub processing_load: f32,
    pub wet_dry_diff_db: f32,
    pub pop_muted_count: u32,
}

/// Open (or create) a file and map it into memory.
/// Error codes are passed in for precise diagnostics.
#[cfg(windows)]
unsafe fn open_file_mmap(
    path: &str,
    size: u32,
    code_file: &str,
    code_mapping: &str,
    code_view: &str,
) -> Result<(HANDLE, HANDLE, *mut std::ffi::c_void), String> {
    let file_handle = CreateFileW(
        &HSTRING::from(path),
        (GENERIC_READ | GENERIC_WRITE).0,
        FILE_SHARE_READ | FILE_SHARE_WRITE,
        None,
        OPEN_ALWAYS,
        FILE_ATTRIBUTE_NORMAL,
        None,
    ).map_err(|e| format!("[{code_file}] CreateFileW({path}): {e}"))?;

    let mapping_handle = CreateFileMappingW(
        file_handle,
        None,
        PAGE_READWRITE,
        0,
        size,
        None,
    ).map_err(|e| {
        let _ = CloseHandle(file_handle);
        format!("[{code_mapping}] CreateFileMappingW({path}): {e}")
    })?;

    let view = MapViewOfFile(
        mapping_handle,
        FILE_MAP_ALL_ACCESS,
        0,
        0,
        size as usize,
    );
    if view.Value.is_null() {
        let _ = CloseHandle(mapping_handle);
        let _ = CloseHandle(file_handle);
        return Err(format!("[{code_view}] MapViewOfFile({path}) returned null"));
    }

    Ok((file_handle, mapping_handle, view.Value))
}

/// Create file-backed mmap bridge.
#[cfg(windows)]
fn create_bridge() -> Result<Bridge, String> {
    // Ensure directory exists
    std::fs::create_dir_all(SHARED_DIR)
        .map_err(|e| format!("[IPC-T001] create_dir_all({SHARED_DIR}): {e}"))?;

    unsafe {
        let (cfg_file, cfg_map, cfg_ptr) = open_file_mmap(
            CONFIG_FILE,
            std::mem::size_of::<SharedConfig>() as u32,
            "IPC-T002", "IPC-T003", "IPC-T004",
        )?;

        let (sts_file, sts_map, sts_ptr) = match open_file_mmap(
            STATUS_FILE,
            std::mem::size_of::<SharedStatus>() as u32,
            "IPC-T005", "IPC-T006", "IPC-T007",
        ) {
            Ok(v) => v,
            Err(e) => {
                let _ = UnmapViewOfFile(MEMORY_MAPPED_VIEW_ADDRESS {
                    Value: cfg_ptr as *mut _,
                });
                let _ = CloseHandle(cfg_map);
                let _ = CloseHandle(cfg_file);
                return Err(e);
            }
        };

        Ok(Bridge {
            config_file_handle: cfg_file,
            config_mapping_handle: cfg_map,
            status_file_handle: sts_file,
            status_mapping_handle: sts_map,
            config_ptr: cfg_ptr as *mut SharedConfig,
            status_ptr: sts_ptr as *mut SharedStatus,
        })
    }
}
