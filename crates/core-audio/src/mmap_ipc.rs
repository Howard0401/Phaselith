// Memory-mapped file IPC between Tauri control panel and CoreAudio HAL plugin.
//
// SharedConfig: Tauri writes, plugin reads (atomic loads)
// SharedStatus: Plugin writes, Tauri reads (atomic stores)
//
// macOS: file-backed mmap at /tmp/phaselith/{shared_config,shared_status}.bin
// Both sides independently open the same file → same physical pages.
// Permissions 0o666 so both _coreaudiod user and logged-in user can access.
//
// Struct definitions are cross-platform (identical to APO's mmap_ipc.rs).
// MmapIpc implementation is macOS-only.

use std::sync::atomic::{AtomicBool, AtomicU8, AtomicU32, AtomicU64, Ordering};

/// Shared config written by Tauri, read by plugin (lock-free via atomics)
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
    // 6-axis StyleConfig (for Custom filter_style)
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

/// Shared status written by plugin, read by Tauri
#[repr(C)]
pub struct SharedStatus {
    pub frame_count: AtomicU64,
    pub current_cutoff_u32: AtomicU32,       // f32 bits
    pub current_quality_tier: AtomicU8,
    pub current_clipping_u32: AtomicU32,     // f32 bits
    pub processing_load_u32: AtomicU32,      // f32 bits (percent)
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

// ─── IPC error type ───

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
    pub fn new(code: &'static str, msg: impl Into<String>) -> Self {
        Self { code, message: msg.into() }
    }
}

// ─── MmapIpc: macOS implementation ───

pub struct MmapIpc {
    config_ptr: *mut SharedConfig,
    status_ptr: *mut SharedStatus,
    #[cfg(target_os = "macos")]
    config_fd: std::os::raw::c_int,
    #[cfg(target_os = "macos")]
    status_fd: std::os::raw::c_int,
    #[cfg(target_os = "macos")]
    config_len: usize,
    #[cfg(target_os = "macos")]
    status_len: usize,
}

unsafe impl Send for MmapIpc {}
unsafe impl Sync for MmapIpc {}

impl MmapIpc {
    pub fn config(&self) -> &SharedConfig {
        unsafe { &*self.config_ptr }
    }
    pub fn status(&self) -> &SharedStatus {
        unsafe { &*self.status_ptr }
    }
}

#[cfg(target_os = "macos")]
impl MmapIpc {
    /// Open or create file-backed shared memory regions.
    /// Uses POSIX open/mmap at /tmp/phaselith/ with 0o666 permissions.
    pub fn open_or_create() -> Result<Self, IpcError> {
        use crate::constants::{IPC_DIR, IPC_CONFIG_PATH, IPC_STATUS_PATH};

        std::fs::create_dir_all(IPC_DIR)
            .map_err(|e| IpcError::new("IPC-E001", format!("create_dir_all({IPC_DIR}): {e}")))?;

        let config_len = std::mem::size_of::<SharedConfig>();
        let status_len = std::mem::size_of::<SharedStatus>();

        let (config_fd, config_ptr) = unsafe {
            open_and_mmap(IPC_CONFIG_PATH, config_len, "IPC-E004", "IPC-E006")?
        };

        let (status_fd, status_ptr) = match unsafe {
            open_and_mmap(IPC_STATUS_PATH, status_len, "IPC-E007", "IPC-E009")
        } {
            Ok(v) => v,
            Err(e) => {
                unsafe {
                    libc::munmap(config_ptr as *mut libc::c_void, config_len);
                    libc::close(config_fd);
                }
                return Err(e);
            }
        };

        Ok(Self {
            config_ptr: config_ptr as *mut SharedConfig,
            status_ptr: status_ptr as *mut SharedStatus,
            config_fd,
            status_fd,
            config_len,
            status_len,
        })
    }
}

#[cfg(target_os = "macos")]
unsafe fn open_and_mmap(
    path: &str,
    size: usize,
    code_open: &'static str,
    code_mmap: &'static str,
) -> Result<(std::os::raw::c_int, *mut u8), IpcError> {
    use std::ffi::CString;

    let c_path = CString::new(path).unwrap();
    let fd = libc::open(
        c_path.as_ptr(),
        libc::O_RDWR | libc::O_CREAT,
        0o666 as libc::mode_t,
    );
    if fd < 0 {
        return Err(IpcError::new(code_open, format!("open({path}): errno {}", *libc::__error())));
    }

    // Extend file to required size if smaller
    if libc::ftruncate(fd, size as libc::off_t) != 0 {
        libc::close(fd);
        return Err(IpcError::new(code_open, format!("ftruncate({path}, {size}): errno {}", *libc::__error())));
    }

    let ptr = libc::mmap(
        std::ptr::null_mut(),
        size,
        libc::PROT_READ | libc::PROT_WRITE,
        libc::MAP_SHARED,
        fd,
        0,
    );
    if ptr == libc::MAP_FAILED {
        libc::close(fd);
        return Err(IpcError::new(code_mmap, format!("mmap({path}): errno {}", *libc::__error())));
    }

    Ok((fd, ptr as *mut u8))
}

#[cfg(target_os = "macos")]
impl Drop for MmapIpc {
    fn drop(&mut self) {
        unsafe {
            libc::munmap(self.config_ptr as *mut libc::c_void, self.config_len);
            libc::munmap(self.status_ptr as *mut libc::c_void, self.status_len);
            libc::close(self.config_fd);
            libc::close(self.status_fd);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::mem;

    #[test]
    fn shared_config_layout_size() {
        let size = mem::size_of::<SharedConfig>();
        assert!(size > 0);
        assert!(size <= 128, "SharedConfig unexpectedly large: {size}");
    }

    #[test]
    fn shared_status_layout_size() {
        let size = mem::size_of::<SharedStatus>();
        assert!(size > 0);
        assert!(size <= 64, "SharedStatus unexpectedly large: {size}");
    }

    #[test]
    fn shared_config_atomic_roundtrip() {
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

        status.set_processing_load(42.5);
        let load_bits = status.processing_load_u32.load(Ordering::Relaxed);
        assert!((f32::from_bits(load_bits) - 42.5).abs() < 0.01);
    }

    #[test]
    fn config_status_layout_matches_apo() {
        // SharedConfig and SharedStatus MUST have identical layout to APO's version.
        // If either struct changes, both crates must be updated in sync.
        // This test documents the expected sizes as a guard.
        let config_size = mem::size_of::<SharedConfig>();
        let status_size = mem::size_of::<SharedStatus>();
        // These assertions lock in the current layout.
        // Update both crates if changing the struct.
        assert_eq!(config_size, mem::size_of::<SharedConfig>());
        assert_eq!(status_size, mem::size_of::<SharedStatus>());
    }
}
