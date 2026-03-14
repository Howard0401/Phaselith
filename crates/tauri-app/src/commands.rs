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
    #[serde(default = "default_synthesis_mode")]
    pub synthesis_mode: u8,
}

fn default_synthesis_mode() -> u8 { 1 } // FftOlaPilot

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
        config.synthesis_mode,
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
        synthesis_mode: 1, // FftOlaPilot (default)
    }
}

#[tauri::command]
pub fn install_apo() -> Result<String, String> {
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

        // Use ShellExecuteW with "runas" to request UAC elevation
        run_elevated("regsvr32", &format!("/s \"{}\"", dll_path.display()))?;

        // After COM registration, bind APO to all render endpoints (also needs admin)
        // Use an elevated helper to write registry
        let bind_result = crate::endpoint_bind::bind_to_all_render_endpoints();
        match bind_result {
            Ok(n) => Ok(format!(
                "APO registered and bound to {n} endpoint(s). Restart audio service to activate."
            )),
            Err(e) => Ok(format!(
                "APO registered but endpoint binding failed: {e}. Manual binding may be needed."
            )),
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

        // Unbind from endpoints first
        let _ = crate::endpoint_bind::unbind_from_all_render_endpoints();

        // Use ShellExecuteW with "runas" to request UAC elevation
        run_elevated("regsvr32", &format!("/u /s \"{}\"", dll_path.display()))?;

        Ok("APO unregistered and unbound. Restart audio service to deactivate.".into())
    }

    #[cfg(not(windows))]
    Err("APO is only supported on Windows".into())
}

/// Run a command with UAC elevation via ShellExecuteW("runas").
/// Shows a UAC prompt to the user, then runs the command as administrator.
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
        // Wait a moment for regsvr32 to complete
        std::thread::sleep(std::time::Duration::from_secs(2));
        Ok(())
    } else {
        Err(format!("UAC elevation failed or was cancelled (code: {code})"))
    }
}
