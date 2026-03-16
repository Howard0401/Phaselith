// Tauri commands exposed to the Vue3 frontend via invoke()

use crate::ipc_bridge;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

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
pub fn get_status() -> Result<Option<ipc_bridge::StatusSnapshot>, String> {
    Ok(ipc_bridge::read_status())
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
    })
}

#[tauri::command]
pub fn install_apo() -> Result<String, String> {
    #[cfg(windows)]
    {
        let dll_path = std::env::current_exe()
            .map_err(|e| e.to_string())?
            .parent()
            .ok_or("No parent dir")?
            .join("phaselith_apo.dll");

        if !dll_path.exists() {
            return Err(format!("APO DLL not found at: {}", dll_path.display()));
        }

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

    #[cfg(not(windows))]
    Err("APO is only supported on Windows".into())
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
    #[cfg(not(windows))]
    {
        false
    }
}

#[tauri::command]
pub fn uninstall_apo() -> Result<String, String> {
    #[cfg(windows)]
    {
        let dll_path = std::env::current_exe()
            .map_err(|e| e.to_string())?
            .parent()
            .ok_or("No parent dir")?
            .join("phaselith_apo.dll");

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

        Ok(format!("APO uninstalled. {elevated_msg}."))
    }

    #[cfg(not(windows))]
    Err("APO is only supported on Windows".into())
}

/// Build the PowerShell uninstall script content.
/// Removes APO CLSID from CompositeFX-SFX ({d04e05a6...},13) and V1-SFX ({d04e05a6...},5).
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
