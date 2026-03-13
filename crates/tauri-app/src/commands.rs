// Tauri commands exposed to the Vue3 frontend via invoke()

use crate::ipc_bridge;
use serde::{Deserialize, Serialize};

#[derive(Deserialize)]
pub struct ConfigPayload {
    pub enabled: bool,
    pub strength: f32,
    pub hf_reconstruction: f32,
    pub dynamics: f32,
    pub transient: f32,
    pub phase_mode: u8,
    pub quality_preset: u8,
}

#[derive(Serialize)]
pub struct ConfigResponse {
    pub enabled: bool,
    pub strength: f32,
    pub hf_reconstruction: f32,
    pub dynamics: f32,
    pub transient: f32,
    pub phase_mode: u8,
    pub quality_preset: u8,
}

#[tauri::command]
pub fn get_status() -> Option<ipc_bridge::StatusSnapshot> {
    ipc_bridge::read_status()
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
    );
}

#[tauri::command]
pub fn get_config() -> ConfigResponse {
    // Return defaults for now; in future read from storage/mmap
    ConfigResponse {
        enabled: true,
        strength: 0.7,
        hf_reconstruction: 0.8,
        dynamics: 0.6,
        transient: 0.5,
        phase_mode: 0,
        quality_preset: 1,
    }
}

#[tauri::command]
pub fn install_apo() -> Result<String, String> {
    // Run regsvr32 to register the APO DLL
    #[cfg(windows)]
    {
        let dll_path = std::env::current_exe()
            .map_err(|e| e.to_string())?
            .parent()
            .ok_or("No parent dir")?
            .join("asce_apo.dll");

        if !dll_path.exists() {
            return Err(format!("APO DLL not found at: {}", dll_path.display()));
        }

        let output = std::process::Command::new("regsvr32")
            .arg("/s")
            .arg(&dll_path)
            .output()
            .map_err(|e| format!("Failed to run regsvr32: {e}"))?;

        if output.status.success() {
            Ok("APO registered successfully. Restart audio service to activate.".into())
        } else {
            Err("regsvr32 failed. Run as administrator.".into())
        }
    }

    #[cfg(not(windows))]
    Err("APO is only supported on Windows".into())
}

#[tauri::command]
pub fn uninstall_apo() -> Result<String, String> {
    #[cfg(windows)]
    {
        let dll_path = std::env::current_exe()
            .map_err(|e| e.to_string())?
            .parent()
            .ok_or("No parent dir")?
            .join("asce_apo.dll");

        let output = std::process::Command::new("regsvr32")
            .arg("/u")
            .arg("/s")
            .arg(&dll_path)
            .output()
            .map_err(|e| format!("Failed to run regsvr32: {e}"))?;

        if output.status.success() {
            Ok("APO unregistered. Restart audio service to deactivate.".into())
        } else {
            Err("regsvr32 /u failed.".into())
        }
    }

    #[cfg(not(windows))]
    Err("APO is only supported on Windows".into())
}
