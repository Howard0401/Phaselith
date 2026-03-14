// Endpoint binding: associate our APO with audio render devices.
//
// Writes PKEY_FX_StreamEffectClsid to each endpoint's FxProperties
// so that audiodg.exe loads our APO for audio processing.

#[cfg(windows)]
use windows::core::HSTRING;
#[cfg(windows)]
use windows::Win32::Foundation::*;
#[cfg(windows)]
use windows::Win32::System::Registry::*;

/// APO CLSID — must match crates/apo-dll/src/guids.rs
const APO_CLSID_STRING: &str = "{A1B2C3D4-E5F6-4A5B-9C8D-1E2F3A4B5C6D}";

/// PKEY_FX_StreamEffectClsid = {d04e05a6-594b-4fb6-a80d-01af5eed7d1d},5
const PKEY_FX_STREAM_EFFECT: &str = "{d04e05a6-594b-4fb6-a80d-01af5eed7d1d},5";

const MMDEVICES_RENDER: &str =
    "SOFTWARE\\Microsoft\\Windows\\CurrentVersion\\MMDevices\\Audio\\Render";

/// Bind APO to all render audio endpoints.
/// Returns the number of endpoints successfully bound.
#[cfg(windows)]
pub fn bind_to_all_render_endpoints() -> Result<u32, String> {
    let mut count = 0u32;

    unsafe {
        let render_h = HSTRING::from(MMDEVICES_RENDER);
        let mut render_key = HKEY::default();
        let r = RegOpenKeyExW(
            HKEY_LOCAL_MACHINE,
            &render_h,
            0,
            KEY_READ,
            &mut render_key,
        );
        if r != ERROR_SUCCESS {
            return Err(format!("Cannot open MMDevices Render: {:?}", r));
        }

        let mut idx = 0u32;
        loop {
            let mut name = [0u16; 260];
            let mut name_len = name.len() as u32;
            let r = RegEnumKeyExW(
                render_key,
                idx,
                windows::core::PWSTR(name.as_mut_ptr()),
                &mut name_len,
                None,
                windows::core::PWSTR::null(),
                None,
                None,
            );
            if r != ERROR_SUCCESS {
                break;
            }

            let ep = String::from_utf16_lossy(&name[..name_len as usize]);
            let fx_path = format!("{}\\{}\\FxProperties", MMDEVICES_RENDER, ep);

            if set_reg_sz(HKEY_LOCAL_MACHINE, &fx_path, PKEY_FX_STREAM_EFFECT, APO_CLSID_STRING).is_ok() {
                count += 1;
            }

            idx += 1;
        }

        let _ = RegCloseKey(render_key);
    }

    if count > 0 {
        Ok(count)
    } else {
        Err("No render endpoints found".into())
    }
}

/// Unbind APO from all render audio endpoints.
#[cfg(windows)]
pub fn unbind_from_all_render_endpoints() -> Result<u32, String> {
    let mut count = 0u32;

    unsafe {
        let render_h = HSTRING::from(MMDEVICES_RENDER);
        let mut render_key = HKEY::default();
        let r = RegOpenKeyExW(
            HKEY_LOCAL_MACHINE,
            &render_h,
            0,
            KEY_READ,
            &mut render_key,
        );
        if r != ERROR_SUCCESS {
            return Ok(0);
        }

        let mut idx = 0u32;
        loop {
            let mut name = [0u16; 260];
            let mut name_len = name.len() as u32;
            let r = RegEnumKeyExW(
                render_key,
                idx,
                windows::core::PWSTR(name.as_mut_ptr()),
                &mut name_len,
                None,
                windows::core::PWSTR::null(),
                None,
                None,
            );
            if r != ERROR_SUCCESS {
                break;
            }

            let ep = String::from_utf16_lossy(&name[..name_len as usize]);
            let fx_path = format!("{}\\{}\\FxProperties", MMDEVICES_RENDER, ep);

            if del_reg_value(HKEY_LOCAL_MACHINE, &fx_path, PKEY_FX_STREAM_EFFECT).is_ok() {
                count += 1;
            }

            idx += 1;
        }

        let _ = RegCloseKey(render_key);
    }

    Ok(count)
}

#[cfg(not(windows))]
pub fn bind_to_all_render_endpoints() -> Result<u32, String> {
    Err("Endpoint binding is only supported on Windows".into())
}

#[cfg(not(windows))]
pub fn unbind_from_all_render_endpoints() -> Result<u32, String> {
    Err("Endpoint binding is only supported on Windows".into())
}

#[cfg(windows)]
fn set_reg_sz(root: HKEY, subkey: &str, name: &str, value: &str) -> Result<(), ()> {
    unsafe {
        let subkey_h = HSTRING::from(subkey);
        let mut hkey = HKEY::default();
        let r = RegCreateKeyExW(
            root, &subkey_h, 0, None,
            REG_OPTION_NON_VOLATILE, KEY_SET_VALUE, None,
            &mut hkey, None,
        );
        if r != ERROR_SUCCESS { return Err(()); }

        let name_h = HSTRING::from(name);
        let wide: Vec<u16> = value.encode_utf16().chain(std::iter::once(0)).collect();
        let bytes = std::slice::from_raw_parts(wide.as_ptr() as *const u8, wide.len() * 2);
        let _ = RegSetValueExW(hkey, &name_h, 0, REG_SZ, Some(bytes));
        let _ = RegCloseKey(hkey);
        Ok(())
    }
}

#[cfg(windows)]
fn del_reg_value(root: HKEY, subkey: &str, name: &str) -> Result<(), ()> {
    unsafe {
        let subkey_h = HSTRING::from(subkey);
        let mut hkey = HKEY::default();
        let r = RegOpenKeyExW(root, &subkey_h, 0, KEY_SET_VALUE, &mut hkey);
        if r != ERROR_SUCCESS { return Err(()); }
        let name_h = HSTRING::from(name);
        let r = RegDeleteValueW(hkey, &name_h);
        let _ = RegCloseKey(hkey);
        if r == ERROR_SUCCESS { Ok(()) } else { Err(()) }
    }
}
