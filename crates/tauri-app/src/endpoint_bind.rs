// Endpoint binding: associate our APO with audio render devices.
//
// Uses V2 property keys (PKEY_FX_StreamEffectClsid, {d3993a3f-...},5)
// which are REG_MULTI_SZ lists of APO CLSIDs. Our CLSID is appended
// to the existing chain (preserving Realtek/system APOs).
//
// Legacy PKEY ({d04e05a6-...},5) is ignored by Windows 10+.

#[cfg(windows)]
use windows::core::HSTRING;
#[cfg(windows)]
use windows::Win32::Foundation::*;
#[cfg(windows)]
use windows::Win32::System::Registry::*;

/// APO CLSID — must match crates/apo-dll/src/guids.rs
const APO_CLSID_STRING: &str = "{A1B2C3D4-E5F6-4A5B-9C8D-1E2F3A4B5C6D}";

/// V2 Stream Effect CLSID list (REG_MULTI_SZ)
/// PKEY_FX_StreamEffectClsid = {d3993a3f-99c2-4402-b5ec-a92a0367664b},5
const PKEY_FX_STREAM_EFFECT_V2: &str = "{d3993a3f-99c2-4402-b5ec-a92a0367664b},5";

const MMDEVICES_RENDER: &str =
    "SOFTWARE\\Microsoft\\Windows\\CurrentVersion\\MMDevices\\Audio\\Render";

/// Bind APO to a specific render endpoint by appending to V2 stream effect list.
/// If endpoint_guid is None, binds to all render endpoints.
#[cfg(windows)]
pub fn bind_to_endpoint(endpoint_guid: Option<&str>) -> Result<u32, String> {
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

            // If a specific endpoint is requested, skip others
            if let Some(target) = endpoint_guid {
                if !ep.eq_ignore_ascii_case(target) {
                    idx += 1;
                    continue;
                }
            }

            let fx_path = format!("{}\\{}\\FxProperties", MMDEVICES_RENDER, ep);

            if append_to_multi_sz(
                HKEY_LOCAL_MACHINE,
                &fx_path,
                PKEY_FX_STREAM_EFFECT_V2,
                APO_CLSID_STRING,
            ).is_ok() {
                count += 1;
            }

            idx += 1;
        }

        let _ = RegCloseKey(render_key);
    }

    if count > 0 {
        Ok(count)
    } else {
        Err("No render endpoints found or binding failed".into())
    }
}

/// Legacy bind function — binds to all endpoints
#[cfg(windows)]
pub fn bind_to_all_render_endpoints() -> Result<u32, String> {
    bind_to_endpoint(None)
}

/// Unbind APO from all render audio endpoints.
/// Removes our CLSID from the V2 stream effect list.
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

            if remove_from_multi_sz(
                HKEY_LOCAL_MACHINE,
                &fx_path,
                PKEY_FX_STREAM_EFFECT_V2,
                APO_CLSID_STRING,
            ).is_ok() {
                count += 1;
            }

            idx += 1;
        }

        let _ = RegCloseKey(render_key);
    }

    Ok(count)
}

#[cfg(not(windows))]
pub fn bind_to_endpoint(_endpoint_guid: Option<&str>) -> Result<u32, String> {
    Err("Endpoint binding is only supported on Windows".into())
}

#[cfg(not(windows))]
pub fn bind_to_all_render_endpoints() -> Result<u32, String> {
    Err("Endpoint binding is only supported on Windows".into())
}

#[cfg(not(windows))]
pub fn unbind_from_all_render_endpoints() -> Result<u32, String> {
    Err("Endpoint binding is only supported on Windows".into())
}

// ---------------------------------------------------------------------------
// REG_MULTI_SZ helpers
// ---------------------------------------------------------------------------

/// Read a REG_MULTI_SZ value and return the list of strings.
#[cfg(windows)]
fn read_multi_sz(root: HKEY, subkey: &str, name: &str) -> Result<Vec<String>, ()> {
    unsafe {
        let subkey_h = HSTRING::from(subkey);
        let mut hkey = HKEY::default();
        let r = RegOpenKeyExW(root, &subkey_h, 0, KEY_READ, &mut hkey);
        if r != ERROR_SUCCESS { return Err(()); }

        let name_h = HSTRING::from(name);

        // First call: get size
        let mut data_type = REG_VALUE_TYPE::default();
        let mut size = 0u32;
        let r = RegQueryValueExW(hkey, &name_h, None, Some(&mut data_type), None, Some(&mut size));
        if r != ERROR_SUCCESS || data_type != REG_MULTI_SZ {
            let _ = RegCloseKey(hkey);
            return Err(());
        }

        // Second call: read data
        let mut buf = vec![0u8; size as usize];
        let r = RegQueryValueExW(
            hkey, &name_h, None, None,
            Some(buf.as_mut_ptr()), Some(&mut size),
        );
        let _ = RegCloseKey(hkey);
        if r != ERROR_SUCCESS { return Err(()); }

        // Parse MULTI_SZ: sequence of null-terminated UTF-16 strings, double-null terminated
        let wide: &[u16] = std::slice::from_raw_parts(
            buf.as_ptr() as *const u16,
            buf.len() / 2,
        );

        let mut strings = Vec::new();
        let mut start = 0;
        for i in 0..wide.len() {
            if wide[i] == 0 {
                if i > start {
                    strings.push(String::from_utf16_lossy(&wide[start..i]));
                }
                start = i + 1;
            }
        }

        Ok(strings)
    }
}

/// Write a list of strings as REG_MULTI_SZ.
#[cfg(windows)]
fn write_multi_sz(root: HKEY, subkey: &str, name: &str, values: &[String]) -> Result<(), ()> {
    unsafe {
        let subkey_h = HSTRING::from(subkey);
        let mut hkey = HKEY::default();
        let r = RegCreateKeyExW(
            root, &subkey_h, 0, None,
            REG_OPTION_NON_VOLATILE, KEY_SET_VALUE, None,
            &mut hkey, None,
        );
        if r != ERROR_SUCCESS { return Err(()); }

        // Build MULTI_SZ: each string null-terminated, extra null at end
        let mut wide: Vec<u16> = Vec::new();
        for s in values {
            wide.extend(s.encode_utf16());
            wide.push(0);
        }
        wide.push(0); // double-null terminator

        let bytes = std::slice::from_raw_parts(
            wide.as_ptr() as *const u8,
            wide.len() * 2,
        );

        let name_h = HSTRING::from(name);
        let r = RegSetValueExW(hkey, &name_h, 0, REG_MULTI_SZ, Some(bytes));
        let _ = RegCloseKey(hkey);
        if r == ERROR_SUCCESS { Ok(()) } else { Err(()) }
    }
}

/// Append a CLSID to a REG_MULTI_SZ value if not already present.
#[cfg(windows)]
fn append_to_multi_sz(root: HKEY, subkey: &str, name: &str, clsid: &str) -> Result<(), ()> {
    let mut list = read_multi_sz(root, subkey, name).unwrap_or_default();

    // Check if already present (case-insensitive)
    let already = list.iter().any(|s| s.eq_ignore_ascii_case(clsid));
    if already {
        return Ok(()); // Already bound
    }

    list.push(clsid.to_string());
    write_multi_sz(root, subkey, name, &list)
}

/// Remove a CLSID from a REG_MULTI_SZ value.
#[cfg(windows)]
fn remove_from_multi_sz(root: HKEY, subkey: &str, name: &str, clsid: &str) -> Result<(), ()> {
    let list = read_multi_sz(root, subkey, name).map_err(|_| ())?;

    let new_list: Vec<String> = list
        .into_iter()
        .filter(|s| !s.eq_ignore_ascii_case(clsid))
        .collect();

    write_multi_sz(root, subkey, name, &new_list)
}
