// Tauri commands exposed to the Vue3 frontend via invoke()

use crate::ipc_bridge;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
#[cfg(target_os = "macos")]
use std::process::Command;

// ─── License API ───

const LICENSE_API: &str = "https://license.phaselith.com";

#[derive(Serialize)]
pub struct HostPlatform {
    pub os: &'static str,
}

#[tauri::command]
pub fn get_host_platform() -> HostPlatform {
    HostPlatform {
        os: std::env::consts::OS,
    }
}

fn system_license_platform() -> &'static str {
    #[cfg(target_os = "macos")]
    {
        "coreaudio"
    }
    #[cfg(windows)]
    {
        "apo"
    }
    #[cfg(not(any(windows, target_os = "macos")))]
    {
        "desktop"
    }
}

#[cfg(target_os = "macos")]
const MACOS_DRIVER_INSTALL_PATH: &str = "/Library/Audio/Plug-Ins/HAL/PhaselithAudio.driver";

#[cfg(target_os = "macos")]
const MACOS_IPC_DIR: &str = "/tmp/phaselith";

#[cfg(target_os = "macos")]
const MACOS_IPC_CONFIG_PATH: &str = "/tmp/phaselith/shared_config.bin";

#[cfg(target_os = "macos")]
const MACOS_IPC_STATUS_PATH: &str = "/tmp/phaselith/shared_status.bin";

#[cfg(target_os = "macos")]
const MACOS_DEV_KEYCHAIN_NAME: &str = "PhaselithDev.keychain-db";

#[cfg(target_os = "macos")]
const MACOS_DEV_SIGNING_IDENTITY: &str = "Phaselith Local Driver Dev";

#[derive(Serialize, Deserialize, Clone)]
pub struct LicenseCache {
    pub license_key: String,
    pub device_id: String,
    pub tier: String,
    pub expires_at: String,
    pub validated_at: u64, // millis since epoch
}

#[derive(Serialize, Deserialize)]
pub struct ActivateResponse {
    pub valid: bool,
    pub tier: Option<String>,
    pub product: Option<String>,
    pub expires_at: Option<String>,
    pub devices_used: Option<u32>,
    pub devices_max: Option<u32>,
    pub error: Option<String>,
}

#[derive(Serialize, Deserialize)]
pub struct ValidateResponse {
    pub valid: bool,
    pub tier: Option<String>,
    pub expires_at: Option<String>,
    pub error: Option<String>,
}

#[derive(Serialize, Deserialize)]
pub struct DeactivateResponse {
    pub success: bool,
    pub devices_used: Option<u32>,
    pub devices_max: Option<u32>,
    pub error: Option<String>,
}

fn license_cache_path() -> PathBuf {
    let dir = dirs::data_dir()
        .unwrap_or_else(|| PathBuf::from("C:\\ProgramData"))
        .join("Phaselith");
    let _ = std::fs::create_dir_all(&dir);
    dir.join("license.json")
}

fn device_id_path() -> PathBuf {
    let dir = dirs::data_dir()
        .unwrap_or_else(|| PathBuf::from("C:\\ProgramData"))
        .join("Phaselith");
    let _ = std::fs::create_dir_all(&dir);
    dir.join("device_id.txt")
}

fn get_or_create_device_id() -> String {
    let path = device_id_path();
    if let Ok(id) = std::fs::read_to_string(&path) {
        let id = id.trim().to_string();
        if !id.is_empty() {
            return id;
        }
    }
    let id = uuid::Uuid::new_v4().to_string();
    let _ = std::fs::write(&path, &id);
    id
}

#[tauri::command]
pub async fn license_activate(key: String) -> Result<ActivateResponse, String> {
    let device_id = get_or_create_device_id();
    let client = reqwest::Client::new();
    let res = client
        .post(format!("{LICENSE_API}/license/activate"))
        .json(&serde_json::json!({
            "license_key": key,
            "device_id": device_id,
            "device_name": "Phaselith Desktop",
            "platform": system_license_platform()
        }))
        .send()
        .await
        .map_err(|e| format!("Network error: {e}"))?;

    let data: ActivateResponse = res.json().await.map_err(|e| format!("Parse error: {e}"))?;

    if data.valid {
        let cache = LicenseCache {
            license_key: key,
            device_id,
            tier: "Pro".into(),
            expires_at: data.expires_at.clone().unwrap_or_default(),
            validated_at: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis() as u64,
        };
        let _ = std::fs::write(license_cache_path(), serde_json::to_string_pretty(&cache).unwrap_or_default());
    }

    Ok(data)
}

#[tauri::command]
pub async fn license_validate() -> Result<ValidateResponse, String> {
    let path = license_cache_path();
    let cache_str = std::fs::read_to_string(&path).map_err(|_| "No license cached".to_string())?;
    let cache: LicenseCache = serde_json::from_str(&cache_str).map_err(|_| "Invalid cache".to_string())?;

    let client = reqwest::Client::new();
    let res = client
        .post(format!("{LICENSE_API}/license/validate"))
        .json(&serde_json::json!({
            "license_key": cache.license_key,
            "device_id": cache.device_id
        }))
        .send()
        .await
        .map_err(|e| format!("Network error: {e}"))?;

    let data: ValidateResponse = res.json().await.map_err(|e| format!("Parse error: {e}"))?;

    if data.valid {
        // Update validated_at
        let updated = LicenseCache {
            validated_at: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis() as u64,
            ..cache
        };
        let _ = std::fs::write(&path, serde_json::to_string_pretty(&updated).unwrap_or_default());
    }

    Ok(data)
}

#[tauri::command]
pub async fn license_deactivate() -> Result<DeactivateResponse, String> {
    let path = license_cache_path();
    let cache_str = std::fs::read_to_string(&path).map_err(|_| "No license cached".to_string())?;
    let cache: LicenseCache = serde_json::from_str(&cache_str).map_err(|_| "Invalid cache".to_string())?;

    let client = reqwest::Client::new();
    let res = client
        .post(format!("{LICENSE_API}/license/deactivate"))
        .json(&serde_json::json!({
            "license_key": cache.license_key,
            "device_id": cache.device_id
        }))
        .send()
        .await
        .map_err(|e| format!("Network error: {e}"))?;

    let data: DeactivateResponse = res.json().await.map_err(|e| format!("Parse error: {e}"))?;

    // Remove cache regardless
    let _ = std::fs::remove_file(&path);

    Ok(data)
}

#[tauri::command]
pub fn license_get_cached() -> Option<LicenseCache> {
    let path = license_cache_path();
    let cache_str = std::fs::read_to_string(path).ok()?;
    serde_json::from_str(&cache_str).ok()
    // Expiry check is done on the frontend side
}

#[derive(Deserialize)]
pub struct ConfigPayload {
    pub enabled: bool,
    pub strength: f32,
    pub hf_reconstruction: f32,
    pub dynamics: f32,
    pub transient: f32,
    pub phase_mode: u8,
    pub quality_preset: u8,
    #[serde(default = "default_synthesis_mode")]
    pub synthesis_mode: u8,
    #[serde(default)]
    pub filter_style: u8, // 0=Reference, 1=Warm, 2=BassPlus, 3=Custom
    // ─── 6-axis StyleConfig (debug/advanced UI) ───
    #[serde(default = "default_warmth")]
    pub warmth: f32,
    #[serde(default = "default_air_brightness")]
    pub air_brightness: f32,
    #[serde(default = "default_smoothness")]
    pub smoothness: f32,
    #[serde(default = "default_spatial_spread")]
    pub spatial_spread: f32,
    #[serde(default = "default_impact_gain")]
    pub impact_gain: f32,
    #[serde(default = "default_body")]
    pub body: f32,
}

fn default_synthesis_mode() -> u8 { 1 } // FftOlaPilot
fn default_warmth() -> f32 { 0.15 }
fn default_air_brightness() -> f32 { 0.50 }
fn default_smoothness() -> f32 { 0.40 }
fn default_spatial_spread() -> f32 { 0.30 }
fn default_impact_gain() -> f32 { 0.15 }
fn default_body() -> f32 { 0.40 }

#[derive(Serialize)]
pub struct ConfigResponse {
    pub enabled: bool,
    pub strength: f32,
    pub hf_reconstruction: f32,
    pub dynamics: f32,
    pub transient: f32,
    pub phase_mode: u8,
    pub quality_preset: u8,
    pub synthesis_mode: u8,
    pub filter_style: u8,
    pub warmth: f32,
    pub air_brightness: f32,
    pub smoothness: f32,
    pub spatial_spread: f32,
    pub impact_gain: f32,
    pub body: f32,
}

/// Track frame_count for staleness detection.
/// If frame_count hasn't changed for STALE_THRESHOLD consecutive polls,
/// the APO is not running → return None (Disconnected).
static LAST_FRAME_COUNT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(u64::MAX);
static STALE_COUNTER: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
const STALE_THRESHOLD: u32 = 15; // 15 × 200ms = 3 seconds

#[tauri::command]
pub fn get_status() -> Result<Option<ipc_bridge::StatusSnapshot>, String> {
    let snap = ipc_bridge::read_status();
    match snap {
        Some(ref s) => {
            let prev = LAST_FRAME_COUNT.swap(s.frame_count, std::sync::atomic::Ordering::Relaxed);
            if s.frame_count == prev {
                let count = STALE_COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed) + 1;
                if count >= STALE_THRESHOLD {
                    // APO not running — stale mmap data
                    return Ok(None);
                }
            } else {
                STALE_COUNTER.store(0, std::sync::atomic::Ordering::Relaxed);
            }
            Ok(snap)
        }
        None => {
            STALE_COUNTER.store(0, std::sync::atomic::Ordering::Relaxed);
            Ok(None)
        }
    }
}

#[tauri::command]
pub fn get_ipc_state() -> IpcState {
    IpcState {
        connected: ipc_bridge::is_connected(),
    }
}

#[derive(Serialize)]
pub struct IpcState {
    pub connected: bool,
}

#[tauri::command]
pub fn set_config(config: ConfigPayload) {
    ipc_bridge::write_config(
        config.enabled,
        config.strength,
        config.hf_reconstruction,
        config.dynamics,
        config.transient,
        config.phase_mode,
        config.quality_preset,
        config.synthesis_mode,
        config.filter_style,
        config.warmth,
        config.air_brightness,
        config.smoothness,
        config.spatial_spread,
        config.impact_gain,
        config.body,
    );
}

#[tauri::command]
pub fn get_config() -> ConfigResponse {
    // Read current config from mmap if available.
    // If mmap is not connected yet (APO not installed), return defaults.
    // This ensures that on Tauri restart, we read back whatever the user
    // last set — not hardcoded defaults that would overwrite their toggle state.
    ipc_bridge::read_config().unwrap_or(ConfigResponse {
        enabled: true,
        strength: 0.7,
        hf_reconstruction: 0.8,
        dynamics: 0.6,
        transient: 0.5,
        phase_mode: 0,
        quality_preset: 1,
        synthesis_mode: 1, // FftOlaPilot (default)
        filter_style: 0,   // Reference (default)
        warmth: 0.15,
        air_brightness: 0.50,
        smoothness: 0.40,
        spatial_spread: 0.30,
        impact_gain: 0.15,
        body: 0.40,
    })
}

/// Resolve the APO DLL path, preferring the release build over debug.
///
/// Search order:
/// 1. `target/release/phaselith_apo.dll` (workspace release build — always up to date)
/// 2. DLL next to the running executable (fallback for packaged/installed app)
///
/// This prevents accidentally installing a stale debug-built DLL when the
/// Tauri app is running from `target/debug/` during development.
#[cfg(windows)]
fn resolve_apo_dll() -> Result<PathBuf, String> {
    let exe_dir = std::env::current_exe()
        .map_err(|e| e.to_string())?
        .parent()
        .ok_or("No parent dir")?
        .to_path_buf();

    // In dev mode, exe is at <workspace>/target/{debug,release}/phaselith-tauri.exe
    // → workspace root = exe_dir / ../../
    // → release DLL = <workspace>/target/release/phaselith_apo.dll
    let workspace_release = exe_dir
        .parent() // target/
        .and_then(|p| p.parent()) // workspace root
        .map(|root| root.join("target").join("release").join("phaselith_apo.dll"));

    if let Some(ref release_path) = workspace_release {
        if release_path.exists() && *release_path != exe_dir.join("phaselith_apo.dll") {
            eprintln!("APO install: using release DLL at {}", release_path.display());
            return Ok(release_path.clone());
        }
    }

    // Fallback: DLL next to exe (packaged app or release mode)
    let local_path = exe_dir.join("phaselith_apo.dll");
    if local_path.exists() {
        eprintln!("APO install: using local DLL at {}", local_path.display());
        return Ok(local_path);
    }

    Err(format!(
        "APO DLL not found. Checked:\n  - {:?}\n  - {}",
        workspace_release,
        exe_dir.join("phaselith_apo.dll").display()
    ))
}

#[cfg(target_os = "macos")]
fn resolve_workspace_root() -> Result<PathBuf, String> {
    let mut candidates = Vec::new();

    if let Ok(exe) = std::env::current_exe() {
        if let Some(debug_target) = exe.parent() {
            if let Some(target_dir) = debug_target.parent() {
                if let Some(workspace_root) = target_dir.parent() {
                    candidates.push(workspace_root.to_path_buf());
                }
            }
        }
    }

    if let Ok(cwd) = std::env::current_dir() {
        candidates.push(cwd);
    }

    for root in candidates {
        if root.join("Cargo.toml").exists() && root.join("crates/core-audio/resources/Info.plist").exists() {
            return Ok(root);
        }
    }

    Err("Could not locate workspace root for Core Audio bundle preparation".into())
}

#[cfg(target_os = "macos")]
fn prepare_core_audio_bundle() -> Result<PathBuf, String> {
    let workspace_root = resolve_workspace_root()?;
    let dylib_path = workspace_root.join("target").join("release").join("libphaselith_core_audio.dylib");
    let info_plist_path = workspace_root.join("crates").join("core-audio").join("resources").join("Info.plist");
    if !dylib_path.exists() {
        return Err(format!(
            "Core Audio dylib not found at {}. Build it first with `cargo build -p phaselith-core-audio --release`.",
            dylib_path.display()
        ));
    }
    if !info_plist_path.exists() {
        return Err(format!("Core Audio Info.plist not found at {}", info_plist_path.display()));
    }

    let bundle_root = workspace_root.join("target").join("PhaselithAudio.driver");
    let contents_dir = bundle_root.join("Contents");
    let macos_dir = contents_dir.join("MacOS");
    let executable_path = macos_dir.join("phaselith_core_audio");

    if bundle_root.exists() {
        std::fs::remove_dir_all(&bundle_root)
            .map_err(|e| format!("Failed to remove old Core Audio bundle: {e}"))?;
    }
    std::fs::create_dir_all(&macos_dir)
        .map_err(|e| format!("Failed to create Core Audio bundle dir: {e}"))?;
    std::fs::copy(&info_plist_path, contents_dir.join("Info.plist"))
        .map_err(|e| format!("Failed to copy Info.plist: {e}"))?;
    std::fs::copy(&dylib_path, &executable_path)
        .map_err(|e| format!("Failed to copy Core Audio dylib: {e}"))?;

    use std::os::unix::fs::PermissionsExt;
    let mut perms = std::fs::metadata(&executable_path)
        .map_err(|e| format!("Failed to stat Core Audio dylib: {e}"))?
        .permissions();
    perms.set_mode(0o755);
    std::fs::set_permissions(&executable_path, perms)
        .map_err(|e| format!("Failed to chmod Core Audio dylib: {e}"))?;

    codesign_core_audio_bundle(&bundle_root)?;

    Ok(bundle_root)
}

#[cfg(target_os = "macos")]
fn macos_local_dev_keychain_path() -> Option<PathBuf> {
    let home = std::env::var_os("HOME")?;
    Some(PathBuf::from(home).join("Library").join("Keychains").join(MACOS_DEV_KEYCHAIN_NAME))
}

#[cfg(target_os = "macos")]
fn codesign_core_audio_bundle(bundle_root: &std::path::Path) -> Result<(), String> {
    if let Some(keychain_path) = macos_local_dev_keychain_path() {
        if keychain_path.exists() {
            let has_identity = Command::new("/usr/bin/security")
                .args(["find-certificate", "-c", MACOS_DEV_SIGNING_IDENTITY])
                .arg(&keychain_path)
                .status()
                .map_err(|e| format!("Failed to query local macOS signing identity: {e}"))?;

            if has_identity.success() {
                let status = Command::new("/usr/bin/codesign")
                    .args(["--force", "--deep", "--keychain"])
                    .arg(&keychain_path)
                    .args(["-s", MACOS_DEV_SIGNING_IDENTITY])
                    .arg(bundle_root)
                    .status()
                    .map_err(|e| format!("Failed to run codesign with local macOS identity: {e}"))?;
                if status.success() {
                    eprintln!(
                        "Phaselith Core Audio bundle signed with local identity '{}'",
                        MACOS_DEV_SIGNING_IDENTITY
                    );
                    return Ok(());
                }
                return Err(format!(
                    "codesign failed with local identity '{}'",
                    MACOS_DEV_SIGNING_IDENTITY
                ));
            }
        }
    }

    let status = Command::new("/usr/bin/codesign")
        .args(["--force", "--deep", "-s", "-"])
        .arg(bundle_root)
        .status()
        .map_err(|e| format!("Failed to run ad-hoc codesign: {e}"))?;
    if status.success() {
        Ok(())
    } else {
        Err("codesign failed while preparing Core Audio bundle".into())
    }
}

#[cfg(target_os = "macos")]
fn escape_applescript_string(input: &str) -> String {
    input.replace('\\', "\\\\").replace('"', "\\\"")
}

#[cfg(target_os = "macos")]
fn temp_script_path(name: &str) -> PathBuf {
    std::env::temp_dir().join(name)
}

#[cfg(target_os = "macos")]
fn run_macos_admin_script(script_name: &str, body: &str) -> Result<(), String> {
    let script_path = temp_script_path(script_name);
    std::fs::write(&script_path, body)
        .map_err(|e| format!("Failed to write macOS admin script: {e}"))?;

    use std::os::unix::fs::PermissionsExt;
    let mut perms = std::fs::metadata(&script_path)
        .map_err(|e| format!("Failed to stat macOS admin script: {e}"))?
        .permissions();
    perms.set_mode(0o755);
    std::fs::set_permissions(&script_path, perms)
        .map_err(|e| format!("Failed to chmod macOS admin script: {e}"))?;

    let apple_script = format!(
        "do shell script \"/bin/sh {}\" with administrator privileges",
        escape_applescript_string(&script_path.display().to_string())
    );

    let status = Command::new("/usr/bin/osascript")
        .arg("-e")
        .arg(&apple_script)
        .status()
        .map_err(|e| format!("Failed to launch macOS admin prompt: {e}"))?;

    let _ = std::fs::remove_file(&script_path);

    if status.success() {
        Ok(())
    } else {
        Err("macOS administrator authorization was cancelled or failed".into())
    }
}

#[cfg(target_os = "macos")]
fn verify_macos_driver_load() -> Result<(), String> {
    use std::thread;
    use std::time::Duration;

    thread::sleep(Duration::from_millis(1200));

    let output = Command::new("/usr/bin/log")
        .args([
            "show",
            "--last",
            "2m",
            "--style",
            "compact",
            "--predicate",
            "(process == \"coreaudiod\" OR process == \"amfid\") && eventMessage CONTAINS[c] \"Phaselith\"",
        ])
        .output()
        .map_err(|e| format!("Failed to inspect macOS Core Audio logs: {e}"))?;

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    let log_text = format!("{stdout}\n{stderr}");

    if log_text.contains("unknown certificate chain")
        || log_text.contains("adhoc signed")
        || log_text.contains("unable to load plug-in")
        || log_text.contains("Remote driver service was unable to load plug-in")
    {
        return Err(
            "Core Audio driver files were copied into the system, but macOS refused to load the driver because the current code-signing chain is not accepted by coreaudiod. Use an Apple Development / Developer ID signing identity for real driver loading."
                .into(),
        );
    }

    Ok(())
}

#[tauri::command]
pub fn install_apo() -> Result<String, String> {
    #[cfg(windows)]
    {
        let dll_path = resolve_apo_dll()?;

        // Phase 1: Elevated script for regsvr32 + service restart (needs admin)
        let marker_path = marker_file_path();
        let _ = std::fs::remove_file(&marker_path);

        let ps_script = build_install_script(&dll_path, &marker_path);
        let script_path = temp_script_path("phaselith_install.ps1");
        std::fs::write(&script_path, &ps_script)
            .map_err(|e| format!("Failed to write install script: {e}"))?;

        run_elevated(
            "powershell",
            &format!(
                "-ExecutionPolicy Bypass -WindowStyle Hidden -File \"{}\"",
                script_path.display()
            ),
        )?;

        // Elevated script does: regsvr32 + service restart + COM binding + test sound
        // Allow up to 120 polls × 500ms = 60s (Add-Type C# compilation takes time)
        let poll_result = poll_for_marker(&marker_path, 120, 500);
        let elevated_msg = match &poll_result {
            Ok(content) => content.trim().to_string(),
            Err(e) => format!("elevated script: {e}"),
        };

        let _ = std::fs::remove_file(&script_path);
        let _ = std::fs::remove_file(&marker_path);

        // Phase 2: Reconnect IPC
        // COM-based endpoint binding is now done inside the elevated PS script
        let ipc_ok = retry_reconnect(15, 1000);
        let ipc_msg = if ipc_ok {
            "IPC connected"
        } else {
            "IPC pending"
        };

        Ok(format!("APO installed. {elevated_msg}. {ipc_msg}."))
    }

    #[cfg(target_os = "macos")]
    {
        let bundle_root = prepare_core_audio_bundle()?;
        let install_script = format!(
            r#"#!/bin/sh
set -eu
DST_DIR="/Library/Audio/Plug-Ins/HAL"
DST="$DST_DIR/PhaselithAudio.driver"
mkdir -p "$DST_DIR"
rm -rf "$DST"
cp -R "{bundle}" "$DST"
chown -R root:wheel "$DST"
chmod -R a+rX "$DST"
/bin/launchctl kickstart -k system/com.apple.audio.coreaudiod >/dev/null 2>&1 || /usr/bin/killall coreaudiod >/dev/null 2>&1 || true
"#,
            bundle = bundle_root.display()
        );
        run_macos_admin_script("phaselith_install_core_audio.sh", &install_script)?;
        verify_macos_driver_load()?;
        let ipc_ok = ipc_bridge::reconnect();
        let ipc_msg = if ipc_ok { "IPC connected" } else { "IPC pending" };
        Ok(format!("Core Audio driver installed. {ipc_msg}."))
    }

    #[cfg(not(any(windows, target_os = "macos")))]
    Err("System audio runtime install is only supported on Windows and macOS".into())
}

/// Build the PowerShell install script content.
/// Runs elevated with admin privileges to:
/// 1. Copy DLL to C:\Program Files\Phaselith\ (accessible by LocalService/audiodg)
/// 2. Register COM from that location
/// 3. Write APO CLSID into CompositeFX StreamEffectClsid ({d04e05a6...},13)
/// 4. Restart AudioEndpointBuilder so audiodg picks up the new APO
/// 5. Play test sound to trigger APO instantiation
///
/// Key insight: audiodg.exe runs as LocalService and cannot access user directories.
/// Key insight: {d3993a3f...},5 is PKEY_SFX_ProcessingModes (modes, NOT CLSIDs).
///              Only {d04e05a6...},13 (CompositeFX) and {d04e05a6...},5 (V1) hold CLSIDs.
#[cfg(windows)]
fn build_install_script(dll_path: &std::path::Path, marker_path: &std::path::Path) -> String {
    let apo_clsid = "{A1B2C3D4-E5F6-4A5B-9C8D-1E2F3A4B5C6D}";
    let install_dir = r"C:\Program Files\Phaselith";
    let mmdevices_render =
        r"SOFTWARE\Microsoft\Windows\CurrentVersion\MMDevices\Audio\Render";

    // Use include_str! to embed the .ps1 file at compile time.
    // This avoids all Rust format!() escaping issues with PowerShell/C# syntax.
    let template = include_str!("install_script.ps1");
    template
        .replace("__DLL_PATH__", &dll_path.display().to_string())
        .replace("__INSTALL_DIR__", install_dir)
        .replace("__RENDER_BASE__", mmdevices_render)
        .replace("__APO_CLSID__", apo_clsid)
        .replace("__MARKER__", &marker_path.display().to_string())
}

/// Poll for a marker file written by the elevated script.
/// Returns the file content on success.
#[cfg(windows)]
fn poll_for_marker(
    path: &std::path::Path,
    max_attempts: u32,
    interval_ms: u64,
) -> Result<String, String> {
    for _ in 0..max_attempts {
        if path.exists() {
            // Small delay to ensure file is fully written
            std::thread::sleep(std::time::Duration::from_millis(100));
            return std::fs::read_to_string(path)
                .map_err(|e| format!("Failed to read marker: {e}"));
        }
        std::thread::sleep(std::time::Duration::from_millis(interval_ms));
    }
    Err(format!(
        "Elevated script did not complete within {}s",
        max_attempts as u64 * interval_ms / 1000
    ))
}

/// Retry reconnect() multiple times, waiting between attempts.
/// The APO in audiodg may take time to create the Global mmap after service restart.
#[cfg(windows)]
fn retry_reconnect(max_attempts: u32, interval_ms: u64) -> bool {
    for attempt in 1..=max_attempts {
        if ipc_bridge::reconnect() {
            eprintln!("Phaselith IPC: reconnected on attempt {attempt}");
            return true;
        }
        if attempt < max_attempts {
            std::thread::sleep(std::time::Duration::from_millis(interval_ms));
        }
    }
    eprintln!("Phaselith IPC: failed to reconnect after {max_attempts} attempts");
    false
}

/// Path for the temp install script
#[cfg(windows)]
fn temp_script_path(name: &str) -> PathBuf {
    std::env::temp_dir().join(name)
}

/// Path for the marker file used to signal completion
#[cfg(windows)]
fn marker_file_path() -> PathBuf {
    std::env::temp_dir().join("phaselith_install_done.txt")
}

#[tauri::command]
pub fn is_apo_installed() -> bool {
    #[cfg(windows)]
    {
        use windows::Win32::System::Registry::*;
        use windows::core::HSTRING;

        let apo_clsid = r"SOFTWARE\Classes\CLSID\{A1B2C3D4-E5F6-4A5B-9C8D-1E2F3A4B5C6D}";
        let subkey = HSTRING::from(apo_clsid);
        let mut hkey = HKEY::default();
        let result = unsafe {
            RegOpenKeyExW(HKEY_LOCAL_MACHINE, &subkey, 0, KEY_READ, &mut hkey)
        };
        if result.is_ok() {
            unsafe { let _ = RegCloseKey(hkey); }
            true
        } else {
            false
        }
    }
    #[cfg(target_os = "macos")]
    {
        std::path::Path::new(MACOS_DRIVER_INSTALL_PATH).exists()
    }
    #[cfg(not(any(windows, target_os = "macos")))]
    {
        false
    }
}

#[tauri::command]
pub fn uninstall_apo() -> Result<String, String> {
    #[cfg(windows)]
    {
        let dll_path = resolve_apo_dll().unwrap_or_else(|_| {
            // For uninstall, we still need a path for regsvr32 /u even if DLL moved
            std::env::current_exe()
                .unwrap()
                .parent()
                .unwrap()
                .join("phaselith_apo.dll")
        });

        // Write an elevated script that unbinds + unregisters + restarts
        let marker_path = marker_file_path();
        let _ = std::fs::remove_file(&marker_path);

        let ps_script = build_uninstall_script(&dll_path, &marker_path);
        let script_path = temp_script_path("phaselith_uninstall.ps1");
        std::fs::write(&script_path, &ps_script)
            .map_err(|e| format!("Failed to write uninstall script: {e}"))?;

        run_elevated(
            "powershell",
            &format!(
                "-ExecutionPolicy Bypass -WindowStyle Hidden -File \"{}\"",
                script_path.display()
            ),
        )?;

        let poll_result = poll_for_marker(&marker_path, 30, 500);
        let elevated_msg = match &poll_result {
            Ok(content) => content.trim().to_string(),
            Err(e) => format!("elevated script: {e}"),
        };

        let _ = std::fs::remove_file(&script_path);
        let _ = std::fs::remove_file(&marker_path);

        // Disconnect IPC bridge so UI shows "Disconnected" immediately
        ipc_bridge::disconnect();

        // Clean up mmap files — they're stale now that APO is gone
        let _ = std::fs::remove_file(r"C:\ProgramData\Phaselith\shared_config.bin");
        let _ = std::fs::remove_file(r"C:\ProgramData\Phaselith\shared_status.bin");
        let _ = std::fs::remove_dir(r"C:\ProgramData\Phaselith");

        Ok(format!("APO uninstalled. {elevated_msg}."))
    }

    #[cfg(target_os = "macos")]
    {
        let uninstall_script = format!(
            r#"#!/bin/sh
set -eu
DST="{driver}"
if [ -e "$DST" ]; then
  rm -rf "$DST"
fi
/bin/launchctl kickstart -k system/com.apple.audio.coreaudiod >/dev/null 2>&1 || /usr/bin/killall coreaudiod >/dev/null 2>&1 || true
"#,
            driver = MACOS_DRIVER_INSTALL_PATH
        );
        run_macos_admin_script("phaselith_uninstall_core_audio.sh", &uninstall_script)?;
        ipc_bridge::disconnect();
        let _ = std::fs::remove_file(MACOS_IPC_CONFIG_PATH);
        let _ = std::fs::remove_file(MACOS_IPC_STATUS_PATH);
        let _ = std::fs::remove_dir(MACOS_IPC_DIR);
        Ok("Core Audio driver removed. System audio has been restored.".into())
    }

    #[cfg(not(any(windows, target_os = "macos")))]
    Err("System audio runtime uninstall is only supported on Windows and macOS".into())
}

/// Build the PowerShell uninstall script content.
/// Removes APO CLSID from all FxProperties values (EFX, SFX, V1, V2).
/// Also unregisters COM and removes installed DLL.
#[cfg(windows)]
fn build_uninstall_script(_dll_path: &std::path::Path, marker_path: &std::path::Path) -> String {
    let apo_clsid = "{A1B2C3D4-E5F6-4A5B-9C8D-1E2F3A4B5C6D}";
    let install_dir = r"C:\Program Files\Phaselith";
    let mmdevices_render =
        r"SOFTWARE\Microsoft\Windows\CurrentVersion\MMDevices\Audio\Render";

    format!(
        r#"$ErrorActionPreference = 'Continue'
$log = @()

# Enable TakeOwnership
Add-Type -TypeDefinition @'
using System;
using System.Runtime.InteropServices;
public static class PU {{
    [DllImport("advapi32.dll", SetLastError = true)]
    static extern bool OpenProcessToken(IntPtr h, uint a, out IntPtr t);
    [DllImport("advapi32.dll", SetLastError = true, CharSet = CharSet.Unicode)]
    static extern bool LookupPrivilegeValue(string s, string n, out long l);
    [DllImport("advapi32.dll", SetLastError = true)]
    static extern bool AdjustTokenPrivileges(IntPtr t, bool d, ref TP n, int b, IntPtr p, IntPtr r);
    [StructLayout(LayoutKind.Sequential)]
    struct TP {{ public int C; public long L; public int A; }}
    public static bool E(string p) {{
        IntPtr t; if (!OpenProcessToken((IntPtr)(-1), 0x28, out t)) return false;
        long l; if (!LookupPrivilegeValue(null, p, out l)) return false;
        TP tp = new TP(); tp.C=1; tp.L=l; tp.A=2;
        return AdjustTokenPrivileges(t, false, ref tp, 0, IntPtr.Zero, IntPtr.Zero);
    }}
}}
'@ -ErrorAction Stop
[PU]::E("SeTakeOwnershipPrivilege") | Out-Null
[PU]::E("SeRestorePrivilege") | Out-Null

function Fix-KeyPermission {{
    param([string]$SubKeyPath)
    try {{
        $key = [Microsoft.Win32.Registry]::LocalMachine.OpenSubKey(
            $SubKeyPath,
            [Microsoft.Win32.RegistryKeyPermissionCheck]::ReadWriteSubTree,
            [System.Security.AccessControl.RegistryRights]::TakeOwnership)
        if (-not $key) {{ return $false }}
        $acl = $key.GetAccessControl([System.Security.AccessControl.AccessControlSections]::Owner)
        $admin = [System.Security.Principal.NTAccount]'BUILTIN\Administrators'
        $acl.SetOwner($admin)
        $key.SetAccessControl($acl)
        $key.Close()
        $key = [Microsoft.Win32.Registry]::LocalMachine.OpenSubKey(
            $SubKeyPath,
            [Microsoft.Win32.RegistryKeyPermissionCheck]::ReadWriteSubTree,
            [System.Security.AccessControl.RegistryRights]::ChangePermissions -bor
            [System.Security.AccessControl.RegistryRights]::ReadKey)
        if (-not $key) {{ return $false }}
        $acl = $key.GetAccessControl()
        $rule = New-Object System.Security.AccessControl.RegistryAccessRule(
            'BUILTIN\Administrators', 'FullControl',
            'ContainerInherit,ObjectInherit', 'None', 'Allow')
        $acl.AddAccessRule($rule)
        $key.SetAccessControl($acl)
        $key.Close()
        return $true
    }} catch {{ return $false }}
}}

# 1. Unbind APO from ALL render endpoints using .NET Registry API
$renderBase = '{mmdevices_render}'
$apoClsid = '{apo_clsid}'
$unboundCount = 0
$pkeyCompositeSfx = '{{d04e05a6-594b-4fb6-a80d-01af5eed7d1d}},13'
$pkeyV1Sfx = '{{d04e05a6-594b-4fb6-a80d-01af5eed7d1d}},5'
$pkeyModes = '{{d3993a3f-99c2-4402-b5ec-a92a0367664b}},5'

$renderKey = [Microsoft.Win32.Registry]::LocalMachine.OpenSubKey($renderBase)
if ($renderKey) {{
    foreach ($epGuid in $renderKey.GetSubKeyNames()) {{
        $fxSubKey = "$renderBase\$epGuid\FxProperties"
        $fxTest = [Microsoft.Win32.Registry]::LocalMachine.OpenSubKey($fxSubKey)
        if (-not $fxTest) {{ continue }}
        $fxTest.Close()

        try {{
            Fix-KeyPermission -SubKeyPath $fxSubKey | Out-Null
        }} catch {{}}

        try {{
            $fxKey = [Microsoft.Win32.Registry]::LocalMachine.OpenSubKey($fxSubKey, $true)
            if (-not $fxKey) {{ continue }}

            # Clean ALL values containing our CLSID
            foreach ($valName in $fxKey.GetValueNames()) {{
                try {{
                    $val = $fxKey.GetValue($valName)
                    $kind = $fxKey.GetValueKind($valName)
                    if ($kind -eq 'MultiString' -and $val -is [string[]]) {{
                        $hasOur = $false
                        foreach ($v in $val) {{ if ($v -ieq $apoClsid) {{ $hasOur = $true; break }} }}
                        if ($hasOur) {{
                            $newVal = @($val | Where-Object {{ $_ -ine $apoClsid }})
                            if ($newVal.Count -gt 0) {{
                                $fxKey.SetValue($valName, [string[]]$newVal, [Microsoft.Win32.RegistryValueKind]::MultiString)
                            }} else {{
                                $fxKey.DeleteValue($valName)
                            }}
                            $unboundCount++
                        }}
                    }} elseif ($kind -eq 'String' -and $val -ieq $apoClsid) {{
                        $fxKey.DeleteValue($valName)
                        $unboundCount++
                    }}
                }} catch {{}}
            }}
            $fxKey.Close()
        }} catch {{}}
    }}
    $renderKey.Close()
}}
$log += "unbound $unboundCount value(s)"

# 2. Unregister COM server via BOTH reg.exe and .NET API
reg delete "HKCR\CLSID\$apoClsid" /f 2>&1 | Out-Null
reg delete "HKLM\SOFTWARE\Classes\CLSID\$apoClsid" /f 2>&1 | Out-Null
try {{
    [Microsoft.Win32.Registry]::LocalMachine.DeleteSubKeyTree("SOFTWARE\Classes\CLSID\$apoClsid", $false)
}} catch {{}}
try {{
    [Microsoft.Win32.Registry]::ClassesRoot.DeleteSubKeyTree("CLSID\$apoClsid", $false)
}} catch {{}}
$log += "COM removed"

# 3. Remove APO catalog via BOTH reg.exe and .NET API
reg delete "HKLM\SOFTWARE\Classes\AudioEngine\AudioProcessingObjects\$apoClsid" /f 2>&1 | Out-Null
try {{
    [Microsoft.Win32.Registry]::LocalMachine.DeleteSubKeyTree("SOFTWARE\Classes\AudioEngine\AudioProcessingObjects\$apoClsid", $false)
}} catch {{}}
$log += "APO catalog removed"

# 4. Clean up installed files
if (Test-Path '{install_dir}') {{
    Remove-Item -Path '{install_dir}' -Recurse -Force -ErrorAction SilentlyContinue
    $log += "install dir removed"
}}

# 5. Restart audio service
Restart-Service AudioEndpointBuilder -Force
Start-Sleep -Seconds 3
$log += "service restarted"

# 6. Write marker
$log -join '; ' | Out-File -FilePath '{marker}' -Encoding UTF8
"#,
        mmdevices_render = mmdevices_render,
        apo_clsid = apo_clsid,
        install_dir = install_dir,
        marker = marker_path.display(),
    )
}

/// Run a command with UAC elevation via ShellExecuteW("runas").
/// Shows a UAC prompt to the user, then returns immediately.
/// The caller should poll for completion using a marker file.
#[cfg(windows)]
fn run_elevated(program: &str, args: &str) -> Result<(), String> {
    use windows::core::HSTRING;
    use windows::Win32::UI::Shell::ShellExecuteW;
    use windows::Win32::UI::WindowsAndMessaging::SW_HIDE;

    let verb = HSTRING::from("runas");
    let file = HSTRING::from(program);
    let params = HSTRING::from(args);

    let result = unsafe {
        ShellExecuteW(None, &verb, &file, &params, None, SW_HIDE)
    };

    // ShellExecuteW returns HINSTANCE > 32 on success
    let code = result.0 as usize;
    if code > 32 {
        // ShellExecuteW is async — caller polls for marker file instead of sleeping
        Ok(())
    } else {
        Err(format!("UAC elevation failed or was cancelled (code: {code})"))
    }
}
