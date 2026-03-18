// COM-based APO binding via IPropertyStore.
//
// Uses IMMDeviceEnumerator → IMMDevice → IPropertyStore to write
// EndpointEffect (EFX) properties. EFX = single instance per endpoint,
// processes the final mixed signal before DAC. This avoids per-stream
// instance races that SFX causes with multi-stream playback.
//
// Bypasses TrustedInstaller registry restrictions because
// AudioEndpointBuilder's property store has the necessary privileges.

use windows::core::{imp, GUID, PWSTR};
use windows::Win32::Media::Audio::*;
use windows::Win32::System::Com::*;
use windows::Win32::UI::Shell::PropertiesSystem::*;

const APO_CLSID_STR: &str = "{A1B2C3D4-E5F6-4A5B-9C8D-1E2F3A4B5C6D}";

// VT_ constants not in windows-core 0.58 imp
const VT_LPWSTR: u16 = 31;
const VT_VECTOR: u16 = 0x1000;
const VT_BLOB: u16 = 65;

// PKEY_CompositeFX_EndpointEffectClsid = {d04e05a6-594b-4fb6-a80d-01af5eed7d1d},15
const PKEY_COMPOSITEFX_EFX: PROPERTYKEY = PROPERTYKEY {
    fmtid: GUID::from_u128(0xd04e05a6_594b_4fb6_a80d_01af5eed7d1d),
    pid: 15,
};

// PKEY_FX_EndpointEffectClsid (V1) = {d04e05a6-594b-4fb6-a80d-01af5eed7d1d},7
const PKEY_FX_EFX_V1: PROPERTYKEY = PROPERTYKEY {
    fmtid: GUID::from_u128(0xd04e05a6_594b_4fb6_a80d_01af5eed7d1d),
    pid: 7,
};

// Legacy SFX keys — used only during unbind to clean up old registrations
const PKEY_COMPOSITEFX_SFX: PROPERTYKEY = PROPERTYKEY {
    fmtid: GUID::from_u128(0xd04e05a6_594b_4fb6_a80d_01af5eed7d1d),
    pid: 13,
};
const PKEY_FX_SFX_V2: PROPERTYKEY = PROPERTYKEY {
    fmtid: GUID::from_u128(0xd3993a3f_99c2_4402_b5ec_a92a0367664b),
    pid: 5,
};
const PKEY_FX_SFX_V1: PROPERTYKEY = PROPERTYKEY {
    fmtid: GUID::from_u128(0xd04e05a6_594b_4fb6_a80d_01af5eed7d1d),
    pid: 5,
};

/// Bind our APO to all render endpoints using COM IPropertyStore.
/// Returns (success_count, error_messages).
pub fn bind_via_com() -> std::result::Result<(u32, Vec<String>), String> {
    unsafe {
        let _ = CoInitializeEx(None, COINIT_MULTITHREADED);

        let enumerator: IMMDeviceEnumerator =
            CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL)
                .map_err(|e| format!("CoCreateInstance MMDeviceEnumerator: {e}"))?;

        let collection = enumerator
            .EnumAudioEndpoints(eRender, DEVICE_STATE_ACTIVE)
            .map_err(|e| format!("EnumAudioEndpoints: {e}"))?;

        let count = collection.GetCount().map_err(|e| format!("GetCount: {e}"))?;

        let mut success = 0u32;
        let mut errors = Vec::new();

        for i in 0..count {
            let device = match collection.Item(i) {
                Ok(d) => d,
                Err(e) => {
                    errors.push(format!("Item({i}): {e}"));
                    continue;
                }
            };

            let device_id = match device.GetId() {
                Ok(id) => {
                    let s = id.to_string().unwrap_or_default();
                    CoTaskMemFree(Some(id.as_ptr() as _));
                    s
                }
                Err(_) => format!("endpoint-{i}"),
            };

            match bind_single_endpoint(&device, &device_id) {
                Ok(()) => success += 1,
                Err(e) => errors.push(format!("{device_id}: {e}")),
            }
        }

        Ok((success, errors))
    }
}

/// Bind our APO to a single endpoint via IPropertyStore.
unsafe fn bind_single_endpoint(device: &IMMDevice, device_id: &str) -> std::result::Result<(), String> {
    let store: IPropertyStore = device
        .OpenPropertyStore(STGM_READWRITE)
        .map_err(|e| format!("OpenPropertyStore(READWRITE): {e}"))?;

    // First, clean up any legacy SFX registrations to prevent dual-loading
    let _ = remove_from_property(&store, PKEY_COMPOSITEFX_SFX);
    let _ = remove_from_property(&store, PKEY_FX_SFX_V2);
    let _ = remove_from_property(&store, PKEY_FX_SFX_V1);

    // Strategy: try CompositeFX EFX first, then V1 EFX
    let bound = try_append_to_property(&store, PKEY_COMPOSITEFX_EFX, "CompositeFX-EFX")
        .or_else(|_| try_set_single_property(&store, PKEY_FX_EFX_V1, "V1-EFX"));

    match bound {
        Ok(key_name) => {
            eprintln!("Phaselith: bound to {device_id} via {key_name}");
            Ok(())
        }
        Err(e) => Err(format!("all bind attempts failed: {e}")),
    }
}

/// Try to append our CLSID to an existing multi-string property.
unsafe fn try_append_to_property(
    store: &IPropertyStore,
    key: PROPERTYKEY,
    key_name: &str,
) -> std::result::Result<String, String> {
    let existing = store.GetValue(&key)
        .map_err(|e| format!("{key_name}: GetValue: {e}"))?;

    let mut clsids = read_multi_sz_from_propvariant(&existing);

    // Check if already bound
    let our_upper = APO_CLSID_STR.to_uppercase();
    if clsids.iter().any(|c| c.to_uppercase() == our_upper) {
        return Ok(format!("{key_name}(already-bound)"));
    }

    // If no existing values, skip (need at least an existing chain)
    if clsids.is_empty() {
        return Err(format!("{key_name}: empty, skipping"));
    }

    // Append our CLSID
    clsids.push(APO_CLSID_STR.to_string());

    // Write back
    let pv = create_multi_sz_propvariant(&clsids)
        .map_err(|e| format!("{key_name}: create propvariant: {e}"))?;

    store.SetValue(&key, &pv)
        .map_err(|e| format!("{key_name}: SetValue: {e}"))?;
    store.Commit().ok();

    Ok(key_name.to_string())
}

/// Set a single CLSID as V1 SFX property.
unsafe fn try_set_single_property(
    store: &IPropertyStore,
    key: PROPERTYKEY,
    key_name: &str,
) -> std::result::Result<String, String> {
    // Check if already set
    if let Ok(pv) = store.GetValue(&key) {
        let vals = read_multi_sz_from_propvariant(&pv);
        if vals.iter().any(|v| v.to_uppercase() == APO_CLSID_STR.to_uppercase()) {
            return Ok(format!("{key_name}(already-bound)"));
        }
    }

    // Write as single string using VT_LPWSTR
    let wide: Vec<u16> = APO_CLSID_STR.encode_utf16().chain(std::iter::once(0)).collect();
    let pwstr = CoTaskMemAlloc(wide.len() * 2) as *mut u16;
    if pwstr.is_null() {
        return Err(format!("{key_name}: CoTaskMemAlloc failed"));
    }
    std::ptr::copy_nonoverlapping(wide.as_ptr(), pwstr, wide.len());

    let raw_pv = imp::PROPVARIANT {
        Anonymous: imp::PROPVARIANT_0 {
            Anonymous: imp::PROPVARIANT_0_0 {
                vt: VT_LPWSTR,
                wReserved1: 0,
                wReserved2: 0,
                wReserved3: 0,
                Anonymous: imp::PROPVARIANT_0_0_0 {
                    pwszVal: pwstr,
                },
            },
        },
    };

    let pv = windows::core::PROPVARIANT::from_raw(raw_pv);
    store.SetValue(&key, &pv)
        .map_err(|e| format!("{key_name}: SetValue: {e}"))?;
    store.Commit().ok();

    // Don't let Drop call PropVariantClear — we gave ownership to SetValue
    std::mem::forget(pv);

    Ok(key_name.to_string())
}

/// Remove our CLSID from a property (for cleaning up legacy SFX keys during bind).
unsafe fn remove_from_property(store: &IPropertyStore, key: PROPERTYKEY) -> std::result::Result<(), String> {
    let pv = store.GetValue(&key).map_err(|e| format!("GetValue: {e}"))?;
    let mut vals = read_multi_sz_from_propvariant(&pv);
    let before = vals.len();
    vals.retain(|v| v.to_uppercase() != APO_CLSID_STR.to_uppercase());
    if vals.len() < before {
        if vals.is_empty() {
            let _ = store.SetValue(&key, &windows::core::PROPVARIANT::default());
        } else if let Ok(new_pv) = create_multi_sz_propvariant(&vals) {
            let _ = store.SetValue(&key, &new_pv);
        }
        let _ = store.Commit();
    }
    Ok(())
}

/// Read strings from a PROPVARIANT (handles VT_LPWSTR, VT_VECTOR|VT_LPWSTR, VT_BLOB).
unsafe fn read_multi_sz_from_propvariant(pv: &windows::core::PROPVARIANT) -> Vec<String> {
    let raw = pv.as_raw();
    let vt = raw.Anonymous.Anonymous.vt;

    // VT_LPWSTR — single string
    if vt == VT_LPWSTR {
        let raw_ptr = raw.Anonymous.Anonymous.Anonymous.pwszVal;
        if !raw_ptr.is_null() {
            if let Ok(s) = PWSTR(raw_ptr).to_string() {
                if !s.is_empty() {
                    return vec![s];
                }
            }
        }
        return Vec::new();
    }

    // VT_VECTOR | VT_LPWSTR — array of strings
    if vt == (VT_VECTOR | VT_LPWSTR) {
        let calpwstr = &raw.Anonymous.Anonymous.Anonymous.calpwstr;
        let count = calpwstr.cElems as usize;
        let ptrs = calpwstr.pElems;
        let mut result = Vec::with_capacity(count);
        for i in 0..count {
            let raw_ptr: *mut u16 = *ptrs.add(i);
            if let Ok(s) = PWSTR(raw_ptr).to_string() {
                if !s.is_empty() {
                    result.push(s);
                }
            }
        }
        return result;
    }

    // VT_BLOB — raw binary (REG_MULTI_SZ format)
    if vt == VT_BLOB {
        let blob = &raw.Anonymous.Anonymous.Anonymous.blob;
        let size = blob.cbSize as usize;
        let ptr = blob.pBlobData;
        if !ptr.is_null() && size >= 4 {
            let u16_count = size / 2;
            let slice = std::slice::from_raw_parts(ptr as *const u16, u16_count);
            let mut result = Vec::new();
            let mut start = 0;
            for i in 0..u16_count {
                if slice[i] == 0 {
                    if i > start {
                        result.push(String::from_utf16_lossy(&slice[start..i]));
                    }
                    start = i + 1;
                }
            }
            return result;
        }
    }

    Vec::new()
}

/// Create a PROPVARIANT with VT_VECTOR | VT_LPWSTR from a list of strings.
unsafe fn create_multi_sz_propvariant(strings: &[String]) -> std::result::Result<windows::core::PROPVARIANT, String> {
    let count = strings.len();

    // Allocate array of PWSTR pointers
    let ptrs_size = count * std::mem::size_of::<*mut u16>();
    let ptrs = CoTaskMemAlloc(ptrs_size) as *mut *mut u16;
    if ptrs.is_null() {
        return Err("CoTaskMemAlloc for ptrs failed".into());
    }

    for (i, s) in strings.iter().enumerate() {
        let wide: Vec<u16> = s.encode_utf16().chain(std::iter::once(0)).collect();
        let buf = CoTaskMemAlloc(wide.len() * 2) as *mut u16;
        if buf.is_null() {
            return Err("CoTaskMemAlloc for string failed".into());
        }
        std::ptr::copy_nonoverlapping(wide.as_ptr(), buf, wide.len());
        *ptrs.add(i) = buf;
    }

    let raw_pv = imp::PROPVARIANT {
        Anonymous: imp::PROPVARIANT_0 {
            Anonymous: imp::PROPVARIANT_0_0 {
                vt: VT_VECTOR | VT_LPWSTR,
                wReserved1: 0,
                wReserved2: 0,
                wReserved3: 0,
                Anonymous: imp::PROPVARIANT_0_0_0 {
                    calpwstr: imp::CALPWSTR {
                        cElems: count as u32,
                        pElems: ptrs,
                    },
                },
            },
        },
    };

    Ok(windows::core::PROPVARIANT::from_raw(raw_pv))
}

/// Unbind our APO from all render endpoints.
pub fn unbind_via_com() -> std::result::Result<(u32, Vec<String>), String> {
    unsafe {
        let _ = CoInitializeEx(None, COINIT_MULTITHREADED);

        let enumerator: IMMDeviceEnumerator =
            CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL)
                .map_err(|e| format!("CoCreateInstance: {e}"))?;

        let collection = enumerator
            .EnumAudioEndpoints(eRender, DEVICE_STATE_ACTIVE)
            .map_err(|e| format!("EnumAudioEndpoints: {e}"))?;

        let count = collection.GetCount().map_err(|e| format!("GetCount: {e}"))?;
        let mut success = 0u32;
        let mut errors = Vec::new();

        for i in 0..count {
            let device = match collection.Item(i) {
                Ok(d) => d,
                Err(e) => {
                    errors.push(format!("Item({i}): {e}"));
                    continue;
                }
            };

            let store = match device.OpenPropertyStore(STGM_READWRITE) {
                Ok(s) => s,
                Err(e) => {
                    errors.push(format!("OpenPropertyStore: {e}"));
                    continue;
                }
            };

            // Remove from all EFX and legacy SFX keys
            for (key, _name) in [
                (PKEY_COMPOSITEFX_EFX, "CompositeFX-EFX"),
                (PKEY_FX_EFX_V1, "V1-EFX"),
                (PKEY_COMPOSITEFX_SFX, "CompositeFX-SFX"),
                (PKEY_FX_SFX_V2, "V2-SFX"),
                (PKEY_FX_SFX_V1, "V1-SFX"),
            ] {
                if let Ok(pv) = store.GetValue(&key) {
                    let mut vals = read_multi_sz_from_propvariant(&pv);
                    let before = vals.len();
                    vals.retain(|v| v.to_uppercase() != APO_CLSID_STR.to_uppercase());
                    if vals.len() < before {
                        if vals.is_empty() {
                            let _ = store.SetValue(&key, &windows::core::PROPVARIANT::default());
                        } else if let Ok(new_pv) = create_multi_sz_propvariant(&vals) {
                            let _ = store.SetValue(&key, &new_pv);
                        }
                        let _ = store.Commit();
                    }
                }
            }

            success += 1;
        }

        Ok((success, errors))
    }
}
