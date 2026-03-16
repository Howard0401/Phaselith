// Memory-mapped file IPC between Tauri control panel and APO DLL.
//
// SharedConfig: Tauri writes, APO reads (atomic loads)
// SharedStatus: APO writes, Tauri reads (atomic stores)
//
// Uses FILE-BACKED mmap at C:\ProgramData\Phaselith\{config,status}.bin.
// Both sides independently open the same file → same physical pages.
// No Global\ namespace needed → no SeCreateGlobalPrivilege required.
// No chicken-and-egg: either side can create the file first.

use std::sync::atomic::{AtomicBool, AtomicU8, AtomicU32, AtomicU64, Ordering};

#[cfg(windows)]
use windows::Win32::Foundation::{HANDLE, CloseHandle, GENERIC_READ, GENERIC_WRITE};
#[cfg(windows)]
use windows::Win32::Storage::FileSystem::*;
#[cfg(windows)]
use windows::Win32::System::Memory::*;
#[cfg(windows)]
use windows::Win32::Security::*;
#[cfg(windows)]
use windows::core::HSTRING;

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
    pub synthesis_mode: AtomicU8,  // 0=LegacyAdditive, 1=FftOlaPilot, 2=FftOlaFull
    pub filter_style: AtomicU8,   // 0=Reference, 1=Warm, 2=BassPlus, 3=Custom
    // ─── 6-axis StyleConfig (for Custom filter_style) ───
    // When filter_style==3 (Custom), APO reads these directly instead of
    // deriving from a preset. All values are f32 * 10000 → u32 (same encoding).
    pub warmth_u32: AtomicU32,
    pub air_brightness_u32: AtomicU32,
    pub smoothness_u32: AtomicU32,
    pub spatial_spread_u32: AtomicU32,
    pub impact_gain_u32: AtomicU32,
    pub body_u32: AtomicU32,
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

    pub fn warmth(&self) -> f32 {
        self.warmth_u32.load(Ordering::Relaxed) as f32 / 10000.0
    }
    pub fn air_brightness(&self) -> f32 {
        self.air_brightness_u32.load(Ordering::Relaxed) as f32 / 10000.0
    }
    pub fn smoothness(&self) -> f32 {
        self.smoothness_u32.load(Ordering::Relaxed) as f32 / 10000.0
    }
    pub fn spatial_spread(&self) -> f32 {
        self.spatial_spread_u32.load(Ordering::Relaxed) as f32 / 10000.0
    }
    pub fn impact_gain(&self) -> f32 {
        self.impact_gain_u32.load(Ordering::Relaxed) as f32 / 10000.0
    }
    pub fn body(&self) -> f32 {
        self.body_u32.load(Ordering::Relaxed) as f32 / 10000.0
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
    /// RMS difference between wet (processed) and dry (input) signal in dB.
    /// When algorithm is active and modifying audio, this will be > -60 dB.
    /// When passthrough or algorithm has no effect, this will be -inf or < -80 dB.
    pub wet_dry_diff_db_u32: AtomicU32,      // f32 bits (dB)
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

    pub fn set_wet_dry_diff_db(&self, db: f32) {
        self.wet_dry_diff_db_u32.store(db.to_bits(), Ordering::Relaxed);
    }

    pub fn increment_frames(&self) {
        self.frame_count.fetch_add(1, Ordering::Relaxed);
    }
}

const SHARED_DIR: &str = r"C:\ProgramData\Phaselith";
const CONFIG_FILE: &str = r"C:\ProgramData\Phaselith\shared_config.bin";
const STATUS_FILE: &str = r"C:\ProgramData\Phaselith\shared_status.bin";

/// Structured IPC error with error code for diagnostics
#[derive(Debug)]
pub struct IpcError {
    pub code: &'static str,
    pub message: String,
}

impl std::fmt::Display for IpcError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "[{}] {}", self.code, self.message)
    }
}

impl IpcError {
    fn new(code: &'static str, msg: impl Into<String>) -> Self {
        Self { code, message: msg.into() }
    }
}

// Error codes:
// IPC-E001: Failed to create shared directory
// IPC-E002: Failed to initialize security descriptor
// IPC-E003: Failed to set security descriptor DACL
// IPC-E004: Failed to CreateFileW (config)
// IPC-E005: Failed to CreateFileMappingW (config)
// IPC-E006: Failed to MapViewOfFile (config)
// IPC-E007: Failed to CreateFileW (status)
// IPC-E008: Failed to CreateFileMappingW (status)
// IPC-E009: Failed to MapViewOfFile (status)

/// Manages file-backed memory-mapped IPC
pub struct MmapIpc {
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

unsafe impl Send for MmapIpc {}
unsafe impl Sync for MmapIpc {}

#[cfg(windows)]
impl MmapIpc {
    /// Open or create file-backed shared memory regions.
    /// Called during APO initialization (not on real-time thread).
    ///
    /// Uses files in C:\ProgramData\Phaselith\ with NULL DACL so both:
    /// - Tauri app (logged-in user session)
    /// - APO DLL (audiodg.exe, NT AUTHORITY\LocalService, Session 0)
    /// can read/write the same physical pages.
    pub fn open_or_create() -> Result<Self, IpcError> {
        // Ensure directory exists
        std::fs::create_dir_all(SHARED_DIR)
            .map_err(|e| IpcError::new("IPC-E001", format!("create_dir_all({SHARED_DIR}): {e}")))?;

        unsafe {
            // Build NULL DACL security for the files
            let mut sd = SECURITY_DESCRIPTOR::default();
            let sd_ptr = PSECURITY_DESCRIPTOR(&mut sd as *mut _ as *mut _);
            InitializeSecurityDescriptor(sd_ptr, 1)
                .map_err(|e| IpcError::new("IPC-E002", format!("InitializeSecurityDescriptor: {e}")))?;
            SetSecurityDescriptorDacl(sd_ptr, true, None, false)
                .map_err(|e| IpcError::new("IPC-E003", format!("SetSecurityDescriptorDacl: {e}")))?;

            let sa = SECURITY_ATTRIBUTES {
                nLength: std::mem::size_of::<SECURITY_ATTRIBUTES>() as u32,
                lpSecurityDescriptor: sd_ptr.0,
                bInheritHandle: false.into(),
            };

            // Open/create config file
            let (cfg_file, cfg_map, cfg_ptr) = open_file_mmap(
                CONFIG_FILE,
                std::mem::size_of::<SharedConfig>() as u32,
                &sa,
                "IPC-E004", "IPC-E005", "IPC-E006",
            )?;

            // Open/create status file
            let (sts_file, sts_map, sts_ptr) = match open_file_mmap(
                STATUS_FILE,
                std::mem::size_of::<SharedStatus>() as u32,
                &sa,
                "IPC-E007", "IPC-E008", "IPC-E009",
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

            Ok(Self {
                config_file_handle: cfg_file,
                config_mapping_handle: cfg_map,
                status_file_handle: sts_file,
                status_mapping_handle: sts_map,
                config_ptr: cfg_ptr as *mut SharedConfig,
                status_ptr: sts_ptr as *mut SharedStatus,
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

/// Open (or create) a file and map it into memory.
/// Returns (file_handle, mapping_handle, view_ptr).
/// Error codes are passed in for precise diagnostics.
#[cfg(windows)]
unsafe fn open_file_mmap(
    path: &str,
    size: u32,
    sa: &SECURITY_ATTRIBUTES,
    code_file: &'static str,
    code_mapping: &'static str,
    code_view: &'static str,
) -> Result<(HANDLE, HANDLE, *mut std::ffi::c_void), IpcError> {
    let file_handle = CreateFileW(
        &HSTRING::from(path),
        (GENERIC_READ | GENERIC_WRITE).0,
        FILE_SHARE_READ | FILE_SHARE_WRITE,
        Some(sa),
        OPEN_ALWAYS,
        FILE_ATTRIBUTE_NORMAL,
        None,
    ).map_err(|e| IpcError::new(code_file, format!("CreateFileW({path}): {e}")))?;

    let mapping_handle = CreateFileMappingW(
        file_handle,
        Some(sa),
        PAGE_READWRITE,
        0,
        size,
        None,
    ).map_err(|e| {
        let _ = CloseHandle(file_handle);
        IpcError::new(code_mapping, format!("CreateFileMappingW({path}): {e}"))
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
        return Err(IpcError::new(code_view, format!("MapViewOfFile({path}) returned null")));
    }

    Ok((file_handle, mapping_handle, view.Value))
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
            let _ = CloseHandle(self.config_mapping_handle);
            let _ = CloseHandle(self.config_file_handle);
            let _ = CloseHandle(self.status_mapping_handle);
            let _ = CloseHandle(self.status_file_handle);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::mem;

    #[test]
    fn shared_config_layout_size() {
        // Ensure SharedConfig is repr(C) and sized correctly.
        // Both APO and Tauri must agree on size.
        let size = mem::size_of::<SharedConfig>();
        assert!(size > 0, "SharedConfig should have non-zero size");
        // 4 (version) + 1 (enabled) + 4*4 (u32 fields) + 3 (u8 fields) = 24
        // With repr(C) alignment/padding this may differ — just ensure consistency.
        assert!(size <= 128, "SharedConfig unexpectedly large: {size}");
    }

    #[test]
    fn shared_status_layout_size() {
        let size = mem::size_of::<SharedStatus>();
        assert!(size > 0, "SharedStatus should have non-zero size");
        assert!(size <= 64, "SharedStatus unexpectedly large: {size}");
    }

    #[test]
    fn shared_config_atomic_roundtrip() {
        // Verify atomic read/write roundtrip through the helper methods
        let config = SharedConfig {
            version: AtomicU32::new(0),
            enabled: AtomicBool::new(true),
            compensation_strength_u32: AtomicU32::new(7000),
            hf_reconstruction_u32: AtomicU32::new(8000),
            dynamics_restoration_u32: AtomicU32::new(6000),
            transient_repair_u32: AtomicU32::new(5000),
            phase_mode: AtomicU8::new(0),
            quality_preset: AtomicU8::new(1),
            synthesis_mode: AtomicU8::new(1),
            filter_style: AtomicU8::new(0),
            warmth_u32: AtomicU32::new(1500),
            air_brightness_u32: AtomicU32::new(5000),
            smoothness_u32: AtomicU32::new(4000),
            spatial_spread_u32: AtomicU32::new(3000),
            impact_gain_u32: AtomicU32::new(1500),
            body_u32: AtomicU32::new(4000),
        };

        assert!(config.is_enabled());
        assert!((config.compensation_strength() - 0.7).abs() < 0.001);
        assert!((config.hf_reconstruction() - 0.8).abs() < 0.001);
        assert!((config.dynamics_restoration() - 0.6).abs() < 0.001);
        assert!((config.transient_repair() - 0.5).abs() < 0.001);

        // Disable and verify
        config.enabled.store(false, Ordering::Relaxed);
        assert!(!config.is_enabled());
    }

    #[test]
    fn shared_status_atomic_roundtrip() {
        let status = SharedStatus {
            frame_count: AtomicU64::new(0),
            current_cutoff_u32: AtomicU32::new(0),
            current_quality_tier: AtomicU8::new(0),
            current_clipping_u32: AtomicU32::new(0),
            processing_load_u32: AtomicU32::new(0),
            wet_dry_diff_db_u32: AtomicU32::new(0),
        };

        status.increment_frames();
        status.increment_frames();
        assert_eq!(status.frame_count.load(Ordering::Relaxed), 2);

        status.set_cutoff(Some(14000.0));
        let cutoff_bits = status.current_cutoff_u32.load(Ordering::Relaxed);
        assert!((f32::from_bits(cutoff_bits) - 14000.0).abs() < 0.01);

        status.set_clipping(0.05);
        let clip_bits = status.current_clipping_u32.load(Ordering::Relaxed);
        assert!((f32::from_bits(clip_bits) - 0.05).abs() < 0.001);

        status.set_processing_load(42.5);
        let load_bits = status.processing_load_u32.load(Ordering::Relaxed);
        assert!((f32::from_bits(load_bits) - 42.5).abs() < 0.01);
    }

    #[test]
    fn shared_dir_path_is_absolute() {
        assert!(SHARED_DIR.starts_with("C:\\"));
        assert!(CONFIG_FILE.starts_with(SHARED_DIR));
        assert!(STATUS_FILE.starts_with(SHARED_DIR));
    }

    #[test]
    fn config_and_status_files_differ() {
        assert_ne!(CONFIG_FILE, STATUS_FILE);
    }

    #[cfg(windows)]
    #[test]
    fn file_backed_mmap_roundtrip() {
        // Integration test: create mmap, write, read back, cleanup
        use std::path::Path;

        let test_config = format!("{}\\test_config_{}.bin", SHARED_DIR, std::process::id());
        let test_status = format!("{}\\test_status_{}.bin", SHARED_DIR, std::process::id());

        // Ensure test dir exists
        let _ = std::fs::create_dir_all(SHARED_DIR);

        // Create two independent mmaps on the same files (simulating APO + Tauri)
        let ipc1 = {
            unsafe {
                let mut sd = SECURITY_DESCRIPTOR::default();
                let sd_ptr = PSECURITY_DESCRIPTOR(&mut sd as *mut _ as *mut _);
                InitializeSecurityDescriptor(sd_ptr, 1).unwrap();
                SetSecurityDescriptorDacl(sd_ptr, true, None, false).unwrap();
                let sa = SECURITY_ATTRIBUTES {
                    nLength: mem::size_of::<SECURITY_ATTRIBUTES>() as u32,
                    lpSecurityDescriptor: sd_ptr.0,
                    bInheritHandle: false.into(),
                };

                let (cf, cm, cp) = open_file_mmap(&test_config, mem::size_of::<SharedConfig>() as u32, &sa, "T-CF", "T-CM", "T-CV").unwrap();
                let (sf, sm, sp) = open_file_mmap(&test_status, mem::size_of::<SharedStatus>() as u32, &sa, "T-SF", "T-SM", "T-SV").unwrap();
                MmapIpc {
                    config_file_handle: cf,
                    config_mapping_handle: cm,
                    status_file_handle: sf,
                    status_mapping_handle: sm,
                    config_ptr: cp as *mut SharedConfig,
                    status_ptr: sp as *mut SharedStatus,
                }
            }
        };

        let ipc2 = {
            unsafe {
                let mut sd = SECURITY_DESCRIPTOR::default();
                let sd_ptr = PSECURITY_DESCRIPTOR(&mut sd as *mut _ as *mut _);
                InitializeSecurityDescriptor(sd_ptr, 1).unwrap();
                SetSecurityDescriptorDacl(sd_ptr, true, None, false).unwrap();
                let sa = SECURITY_ATTRIBUTES {
                    nLength: mem::size_of::<SECURITY_ATTRIBUTES>() as u32,
                    lpSecurityDescriptor: sd_ptr.0,
                    bInheritHandle: false.into(),
                };

                let (cf, cm, cp) = open_file_mmap(&test_config, mem::size_of::<SharedConfig>() as u32, &sa, "T-CF", "T-CM", "T-CV").unwrap();
                let (sf, sm, sp) = open_file_mmap(&test_status, mem::size_of::<SharedStatus>() as u32, &sa, "T-SF", "T-SM", "T-SV").unwrap();
                MmapIpc {
                    config_file_handle: cf,
                    config_mapping_handle: cm,
                    status_file_handle: sf,
                    status_mapping_handle: sm,
                    config_ptr: cp as *mut SharedConfig,
                    status_ptr: sp as *mut SharedStatus,
                }
            }
        };

        // Writer (simulates Tauri) writes config via ipc1
        ipc1.config().enabled.store(true, Ordering::Release);
        ipc1.config().compensation_strength_u32.store(7500, Ordering::Release);
        ipc1.config().version.store(42, Ordering::Release);

        // Reader (simulates APO) reads config via ipc2 — SAME physical file
        assert!(ipc2.config().is_enabled());
        assert_eq!(ipc2.config().compensation_strength_u32.load(Ordering::Acquire), 7500);
        assert_eq!(ipc2.config().version.load(Ordering::Acquire), 42);

        // APO writes status via ipc2
        ipc2.status().set_processing_load(55.5);
        ipc2.status().increment_frames();

        // Tauri reads status via ipc1
        let load_bits = ipc1.status().processing_load_u32.load(Ordering::Acquire);
        assert!((f32::from_bits(load_bits) - 55.5).abs() < 0.01);
        assert_eq!(ipc1.status().frame_count.load(Ordering::Acquire), 1);

        // Toggle enabled off via ipc1, verify via ipc2
        ipc1.config().enabled.store(false, Ordering::Release);
        assert!(!ipc2.config().is_enabled(), "Hot toggle must propagate instantly");

        // Cleanup
        drop(ipc1);
        drop(ipc2);
        let _ = std::fs::remove_file(&test_config);
        let _ = std::fs::remove_file(&test_status);
    }
}
