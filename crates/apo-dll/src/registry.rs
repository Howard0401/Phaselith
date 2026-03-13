// COM + APO registry operations for DllRegisterServer / DllUnregisterServer.
//
// Registers ASCE APO as:
// 1. COM InprocServer32 under HKCR\CLSID\{our-guid}
// 2. AudioEngine APO under HKLM\SOFTWARE\Classes\AudioEngine\AudioProcessingObjects\{our-guid}

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

/// Get the full path of this DLL module
fn get_module_path() -> Result<String, HRESULT> {
    unsafe {
        let mut buf = [0u16; 260];
        let len = windows::Win32::System::LibraryLoader::GetModuleFileNameW(
            None,
            &mut buf,
        );
        if len == 0 {
            return Err(E_FAIL);
        }
        Ok(String::from_utf16_lossy(&buf[..len as usize]))
    }
}

/// Register COM class + APO in registry
pub fn register_server() -> HRESULT {
    let dll_path = match get_module_path() {
        Ok(p) => p,
        Err(hr) => return hr,
    };

    let guid_str = guid_to_string(&CLSID_ASCE_APO);

    // 1. Register COM InprocServer32
    let com_key_path = format!("CLSID\\{guid_str}\\InprocServer32");
    if let Err(_) = set_registry_value(
        HKEY_CLASSES_ROOT,
        &com_key_path,
        "",
        &dll_path,
    ) {
        return E_FAIL;
    }
    let _ = set_registry_value(
        HKEY_CLASSES_ROOT,
        &com_key_path,
        "ThreadingModel",
        "Both",
    );

    // Set friendly name on CLSID key
    let clsid_path = format!("CLSID\\{guid_str}");
    let _ = set_registry_value(
        HKEY_CLASSES_ROOT,
        &clsid_path,
        "",
        APO_FRIENDLY_NAME,
    );

    // 2. Register as Audio Processing Object
    let apo_key_path = format!(
        "SOFTWARE\\Classes\\AudioEngine\\AudioProcessingObjects\\{guid_str}"
    );
    let _ = set_registry_value(HKEY_LOCAL_MACHINE, &apo_key_path, "FriendlyName", APO_FRIENDLY_NAME);
    let _ = set_registry_value(HKEY_LOCAL_MACHINE, &apo_key_path, "Copyright", "ASCE Project");
    let _ = set_registry_dword(HKEY_LOCAL_MACHINE, &apo_key_path, "MajorVersion", 1);
    let _ = set_registry_dword(HKEY_LOCAL_MACHINE, &apo_key_path, "MinorVersion", 0);
    // APO_FLAG_DEFAULT (0x0E) = SFX + MFX
    let _ = set_registry_dword(HKEY_LOCAL_MACHINE, &apo_key_path, "Flags", 0x0E);
    let _ = set_registry_dword(HKEY_LOCAL_MACHINE, &apo_key_path, "MinInputConnections", 1);
    let _ = set_registry_dword(HKEY_LOCAL_MACHINE, &apo_key_path, "MaxInputConnections", 1);
    let _ = set_registry_dword(HKEY_LOCAL_MACHINE, &apo_key_path, "MinOutputConnections", 1);
    let _ = set_registry_dword(HKEY_LOCAL_MACHINE, &apo_key_path, "MaxOutputConnections", 1);
    let _ = set_registry_dword(HKEY_LOCAL_MACHINE, &apo_key_path, "MaxInstances", 0xFFFFFFFF);

    S_OK
}

/// Unregister COM class + APO from registry
pub fn unregister_server() -> HRESULT {
    let guid_str = guid_to_string(&CLSID_ASCE_APO);

    // Remove COM registration
    let com_key_path = format!("CLSID\\{guid_str}");
    let _ = delete_registry_tree(HKEY_CLASSES_ROOT, &com_key_path);

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
            return Err(());
        }

        let name_h = HSTRING::from(name);
        let value_wide: Vec<u16> = value.encode_utf16().chain(std::iter::once(0)).collect();
        let value_bytes = std::slice::from_raw_parts(
            value_wide.as_ptr() as *const u8,
            value_wide.len() * 2,
        );
        let _ = RegSetValueExW(
            hkey,
            &name_h,
            0,
            REG_SZ,
            Some(value_bytes),
        );
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
