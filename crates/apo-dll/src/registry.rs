// COM + APO registry operations for DllRegisterServer / DllUnregisterServer.
//
// Registration has two phases:
// 1. DllRegisterServer (regsvr32): COM InprocServer32 + AudioEngine APO catalog
// 2. bind_to_all_render_endpoints (Tauri app): writes APO CLSID to each
//    audio endpoint's FxProperties so audiodg.exe knows to load us

use crate::guids::*;
use windows::core::{GUID, HRESULT, HSTRING};
use windows::Win32::Foundation::*;
use windows::Win32::System::Registry::*;

fn guid_to_string(guid: &GUID) -> String {
    format!(
        "{{{:08X}-{:04X}-{:04X}-{:02X}{:02X}-{:02X}{:02X}{:02X}{:02X}{:02X}{:02X}}}",
        guid.data1, guid.data2, guid.data3,
        guid.data4[0], guid.data4[1], guid.data4[2], guid.data4[3],
        guid.data4[4], guid.data4[5], guid.data4[6], guid.data4[7],
    )
}

/// Get the full path of this DLL module.
/// Uses the HMODULE captured in DllMain (not None, which would return the EXE path).
fn get_module_path() -> Result<String, HRESULT> {
    unsafe {
        let hmodule = crate::dll_hmodule();
        let mut buf = [0u16; 260];
        let len = windows::Win32::System::LibraryLoader::GetModuleFileNameW(
            hmodule,
            &mut buf,
        );
        if len == 0 {
            return Err(E_FAIL);
        }
        let path = String::from_utf16_lossy(&buf[..len as usize]);
        apo_log!("get_module_path: {}", path);
        Ok(path)
    }
}

/// Register COM class + APO in registry
pub fn register_server() -> HRESULT {
    apo_log!("register_server: START");

    let dll_path = match get_module_path() {
        Ok(p) => p,
        Err(hr) => {
            apo_log!("register_server: get_module_path FAILED {:?}", hr);
            return hr;
        }
    };

    let guid_str = guid_to_string(&CLSID_PHASELITH_APO);
    apo_log!("register_server: CLSID={}, dll={}", guid_str, dll_path);

    // 1. Register COM InprocServer32
    // IMPORTANT: Write to HKLM\SOFTWARE\Classes directly instead of HKEY_CLASSES_ROOT.
    // HKCR writes can be silently virtualized (UAC/sandbox) and not persist.
    let com_key_path = format!("SOFTWARE\\Classes\\CLSID\\{guid_str}\\InprocServer32");
    match set_registry_value(HKEY_LOCAL_MACHINE, &com_key_path, "", &dll_path) {
        Ok(()) => apo_log!("register_server: InprocServer32 OK (HKLM)"),
        Err(()) => {
            apo_log!("register_server: InprocServer32 FAILED (HKLM)");
            return E_FAIL;
        }
    }
    match set_registry_value(HKEY_LOCAL_MACHINE, &com_key_path, "ThreadingModel", "Both") {
        Ok(()) => apo_log!("register_server: ThreadingModel OK"),
        Err(()) => apo_log!("register_server: ThreadingModel FAILED"),
    }

    // Set friendly name on CLSID key
    let clsid_path = format!("SOFTWARE\\Classes\\CLSID\\{guid_str}");
    let _ = set_registry_value(HKEY_LOCAL_MACHINE, &clsid_path, "", APO_FRIENDLY_NAME);

    // 2. Register as Audio Processing Object
    let apo_key_path = format!(
        "SOFTWARE\\Classes\\AudioEngine\\AudioProcessingObjects\\{guid_str}"
    );
    apo_log!("register_server: writing APO key: {}", apo_key_path);
    match set_registry_value(HKEY_LOCAL_MACHINE, &apo_key_path, "FriendlyName", APO_FRIENDLY_NAME) {
        Ok(()) => apo_log!("register_server: APO FriendlyName OK"),
        Err(()) => apo_log!("register_server: APO FriendlyName FAILED"),
    }
    let _ = set_registry_value(HKEY_LOCAL_MACHINE, &apo_key_path, "Copyright", "Phaselith Project");
    let _ = set_registry_dword(HKEY_LOCAL_MACHINE, &apo_key_path, "MajorVersion", 1);
    let _ = set_registry_dword(HKEY_LOCAL_MACHINE, &apo_key_path, "MinorVersion", 0);
    // APO_FLAG_DEFAULT (0x0E) = SFX + MFX
    let _ = set_registry_dword(HKEY_LOCAL_MACHINE, &apo_key_path, "Flags", 0x0E);
    let _ = set_registry_dword(HKEY_LOCAL_MACHINE, &apo_key_path, "MinInputConnections", 1);
    let _ = set_registry_dword(HKEY_LOCAL_MACHINE, &apo_key_path, "MaxInputConnections", 1);
    let _ = set_registry_dword(HKEY_LOCAL_MACHINE, &apo_key_path, "MinOutputConnections", 1);
    let _ = set_registry_dword(HKEY_LOCAL_MACHINE, &apo_key_path, "MaxOutputConnections", 1);
    let _ = set_registry_dword(HKEY_LOCAL_MACHINE, &apo_key_path, "MaxInstances", 0xFFFFFFFF);
    let _ = set_registry_dword(HKEY_LOCAL_MACHINE, &apo_key_path, "NumAPOInterfaces", 1);
    // IAudioProcessingObject IID
    let _ = set_registry_value(
        HKEY_LOCAL_MACHINE,
        &apo_key_path,
        "APOInterface0",
        "{FD7F2B29-24D0-4B5C-B177-592C39F9CA10}",
    );

    apo_log!("register_server: DONE");
    S_OK
}

/// Unregister COM class + APO from registry
pub fn unregister_server() -> HRESULT {
    let guid_str = guid_to_string(&CLSID_PHASELITH_APO);

    // Remove COM registration (HKLM path, matching register_server)
    let com_key_path = format!("SOFTWARE\\Classes\\CLSID\\{guid_str}");
    let _ = delete_registry_tree(HKEY_LOCAL_MACHINE, &com_key_path);
    // Also try HKCR in case old registration exists there
    let hkcr_path = format!("CLSID\\{guid_str}");
    let _ = delete_registry_tree(HKEY_CLASSES_ROOT, &hkcr_path);

    // Remove APO registration
    let apo_key_path = format!(
        "SOFTWARE\\Classes\\AudioEngine\\AudioProcessingObjects\\{guid_str}"
    );
    let _ = delete_registry_tree(HKEY_LOCAL_MACHINE, &apo_key_path);

    S_OK
}

fn set_registry_value(
    root: HKEY,
    subkey: &str,
    name: &str,
    value: &str,
) -> Result<(), ()> {
    unsafe {
        let subkey_h = HSTRING::from(subkey);
        let mut hkey = HKEY::default();
        let result = RegCreateKeyExW(
            root,
            &subkey_h,
            0,
            None,
            REG_OPTION_NON_VOLATILE,
            KEY_SET_VALUE,
            None,
            &mut hkey,
            None,
        );
        if result != ERROR_SUCCESS {
            apo_log!("RegCreateKeyExW FAILED: subkey={}, err={:?}", subkey, result);
            return Err(());
        }

        let name_h = HSTRING::from(name);
        let value_wide: Vec<u16> = value.encode_utf16().chain(std::iter::once(0)).collect();
        let value_bytes = std::slice::from_raw_parts(
            value_wide.as_ptr() as *const u8,
            value_wide.len() * 2,
        );
        let set_result = RegSetValueExW(
            hkey,
            &name_h,
            0,
            REG_SZ,
            Some(value_bytes),
        );
        if set_result != ERROR_SUCCESS {
            apo_log!("RegSetValueExW FAILED: subkey={} name={} err={:?}", subkey, name, set_result);
        }
        let _ = RegCloseKey(hkey);
        Ok(())
    }
}

fn set_registry_dword(
    root: HKEY,
    subkey: &str,
    name: &str,
    value: u32,
) -> Result<(), ()> {
    unsafe {
        let subkey_h = HSTRING::from(subkey);
        let mut hkey = HKEY::default();
        let result = RegCreateKeyExW(
            root,
            &subkey_h,
            0,
            None,
            REG_OPTION_NON_VOLATILE,
            KEY_SET_VALUE,
            None,
            &mut hkey,
            None,
        );
        if result != ERROR_SUCCESS {
            return Err(());
        }

        let name_h = HSTRING::from(name);
        let value_bytes = value.to_ne_bytes();
        let _ = RegSetValueExW(
            hkey,
            &name_h,
            0,
            REG_DWORD,
            Some(&value_bytes),
        );
        let _ = RegCloseKey(hkey);
        Ok(())
    }
}

fn delete_registry_tree(root: HKEY, subkey: &str) -> Result<(), ()> {
    unsafe {
        let subkey_h = HSTRING::from(subkey);
        let result = RegDeleteTreeW(root, &subkey_h);
        if result == ERROR_SUCCESS { Ok(()) } else { Err(()) }
    }
}

// ---------------------------------------------------------------------------
// Endpoint binding: write APO CLSID to audio device FxProperties
// ---------------------------------------------------------------------------

/// PKEY_CompositeFX_StreamEffectClsid = {d04e05a6-594b-4fb6-a80d-01af5eed7d1d},13
/// This is the correct key for writing APO CLSIDs.
/// NOTE: {d3993a3f...},5 is PKEY_SFX_ProcessingModes (mode GUIDs, NOT CLSIDs!)
const PKEY_FX_STREAM_EFFECT_CLSID_NAME: &str =
    "{d04e05a6-594b-4fb6-a80d-01af5eed7d1d},13";

/// Base registry path for audio render endpoints
const MMDEVICES_RENDER_PATH: &str =
    "SOFTWARE\\Microsoft\\Windows\\CurrentVersion\\MMDevices\\Audio\\Render";

/// Bind our APO to all render audio endpoints.
/// Called from Tauri app after DllRegisterServer succeeds.
/// Writes PKEY_FX_StreamEffectClsid to each endpoint's FxProperties.
pub fn bind_to_all_render_endpoints() -> std::result::Result<u32, String> {
    let guid_str = guid_to_string(&CLSID_PHASELITH_APO);
    let mut bound_count = 0u32;

    unsafe {
        // Open the Render devices key
        let render_h = HSTRING::from(MMDEVICES_RENDER_PATH);
        let mut render_key = HKEY::default();
        let result = RegOpenKeyExW(
            HKEY_LOCAL_MACHINE,
            &render_h,
            0,
            KEY_READ,
            &mut render_key,
        );
        if result != ERROR_SUCCESS {
            return Err(format!("Cannot open MMDevices\\Render: {:?}", result));
        }

        // Enumerate endpoint subkeys (each is a GUID like {xxxxxxxx-...})
        let mut index = 0u32;
        loop {
            let mut name_buf = [0u16; 260];
            let mut name_len = name_buf.len() as u32;
            let result = RegEnumKeyExW(
                render_key,
                index,
                windows::core::PWSTR(name_buf.as_mut_ptr()),
                &mut name_len,
                None,
                windows::core::PWSTR::null(),
                None,
                None,
            );
            if result != ERROR_SUCCESS {
                break; // No more subkeys
            }

            let endpoint_name = String::from_utf16_lossy(&name_buf[..name_len as usize]);
            let fx_path = format!("{}\\{}\\FxProperties", MMDEVICES_RENDER_PATH, endpoint_name);

            // Write our APO CLSID as the stream effect
            if set_registry_value(
                HKEY_LOCAL_MACHINE,
                &fx_path,
                PKEY_FX_STREAM_EFFECT_CLSID_NAME,
                &guid_str,
            ).is_ok() {
                bound_count += 1;
            }

            index += 1;
        }

        let _ = RegCloseKey(render_key);
    }

    if bound_count > 0 {
        Ok(bound_count)
    } else {
        Err("No audio render endpoints found".into())
    }
}

/// Unbind our APO from all render audio endpoints.
/// Removes PKEY_FX_StreamEffectClsid from each endpoint's FxProperties.
pub fn unbind_from_all_render_endpoints() -> std::result::Result<u32, String> {
    let mut unbound_count = 0u32;

    unsafe {
        let render_h = HSTRING::from(MMDEVICES_RENDER_PATH);
        let mut render_key = HKEY::default();
        let result = RegOpenKeyExW(
            HKEY_LOCAL_MACHINE,
            &render_h,
            0,
            KEY_READ,
            &mut render_key,
        );
        if result != ERROR_SUCCESS {
            return Err(format!("Cannot open MMDevices\\Render: {:?}", result));
        }

        let mut index = 0u32;
        loop {
            let mut name_buf = [0u16; 260];
            let mut name_len = name_buf.len() as u32;
            let result = RegEnumKeyExW(
                render_key,
                index,
                windows::core::PWSTR(name_buf.as_mut_ptr()),
                &mut name_len,
                None,
                windows::core::PWSTR::null(),
                None,
                None,
            );
            if result != ERROR_SUCCESS {
                break;
            }

            let endpoint_name = String::from_utf16_lossy(&name_buf[..name_len as usize]);
            let fx_path = format!("{}\\{}\\FxProperties", MMDEVICES_RENDER_PATH, endpoint_name);

            // Delete the stream effect CLSID value
            if delete_registry_value(
                HKEY_LOCAL_MACHINE,
                &fx_path,
                PKEY_FX_STREAM_EFFECT_CLSID_NAME,
            ).is_ok() {
                unbound_count += 1;
            }

            index += 1;
        }

        let _ = RegCloseKey(render_key);
    }

    Ok(unbound_count)
}

fn delete_registry_value(root: HKEY, subkey: &str, name: &str) -> Result<(), ()> {
    unsafe {
        let subkey_h = HSTRING::from(subkey);
        let mut hkey = HKEY::default();
        let result = RegOpenKeyExW(root, &subkey_h, 0, KEY_SET_VALUE, &mut hkey);
        if result != ERROR_SUCCESS {
            return Err(());
        }
        let name_h = HSTRING::from(name);
        let result = RegDeleteValueW(hkey, &name_h);
        let _ = RegCloseKey(hkey);
        if result == ERROR_SUCCESS { Ok(()) } else { Err(()) }
    }
}
