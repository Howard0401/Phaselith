// Phaselith APO COM DLL
//
// cdylib loaded by audiodg.exe via Windows Audio Engine.
//
// Loading path:
//   regsvr32 phaselith_apo.dll → DllRegisterServer() writes registry
//   audiodg.exe → DllGetClassObject() → PhaselithApo
//   Audio thread → APOProcess() (real-time, zero-alloc)

#[cfg(windows)]
#[macro_use]
mod debug_log;
#[cfg(windows)]
mod guids;
#[cfg(windows)]
mod aggregation;
#[cfg(windows)]
mod class_factory;
#[cfg(windows)]
mod apo_impl;
#[cfg(windows)]
mod apo_com;
#[cfg(windows)]
mod format_negotiate;
#[cfg(windows)]
mod mmap_ipc;
#[cfg(windows)]
mod registry;
#[cfg(windows)]
mod audio_dump;

#[cfg(windows)]
use windows::core::HRESULT;
#[cfg(windows)]
use windows::Win32::Foundation::*;

/// Global storage for the DLL's own module handle (captured in DllMain).
/// Stored as raw pointer since HINSTANCE/HMODULE are both *mut c_void wrappers.
#[cfg(windows)]
static mut DLL_MODULE_HANDLE: *mut core::ffi::c_void = std::ptr::null_mut();

#[cfg(windows)]
#[no_mangle]
pub extern "system" fn DllMain(hinst: HINSTANCE, reason: u32, _reserved: *mut ()) -> BOOL {
    if reason == 1 { // DLL_PROCESS_ATTACH
        unsafe { DLL_MODULE_HANDLE = hinst.0; }
        apo_log!("DllMain: DLL_PROCESS_ATTACH (pid={})", std::process::id());
    }
    TRUE
}

#[cfg(windows)]
#[no_mangle]
pub extern "system" fn DllGetClassObject(
    rclsid: *const windows::core::GUID,
    riid: *const windows::core::GUID,
    ppv: *mut *mut core::ffi::c_void,
) -> HRESULT {
    apo_log!("DllGetClassObject called (pid={})", std::process::id());
    if rclsid.is_null() || riid.is_null() || ppv.is_null() {
        apo_log!("DllGetClassObject: null pointer");
        return E_POINTER;
    }
    let clsid = unsafe { &*rclsid };
    let iid = unsafe { &*riid };
    apo_log!("DllGetClassObject: clsid={:?} iid={:?}", clsid, iid);
    let result = unsafe {
        class_factory::get_class_object(clsid, iid, ppv)
    };
    apo_log!("DllGetClassObject result: {:?}", result);
    result
}

#[cfg(windows)]
#[no_mangle]
pub extern "system" fn DllCanUnloadNow() -> HRESULT {
    if class_factory::can_unload() { S_OK } else { S_FALSE }
}

#[cfg(windows)]
#[no_mangle]
pub extern "system" fn DllRegisterServer() -> HRESULT {
    registry::register_server()
}

#[cfg(windows)]
#[no_mangle]
pub extern "system" fn DllUnregisterServer() -> HRESULT {
    registry::unregister_server()
}

/// Get the DLL's own module handle as HMODULE (captured in DllMain).
#[cfg(windows)]
pub(crate) fn dll_hmodule() -> HMODULE {
    unsafe { HMODULE(DLL_MODULE_HANDLE) }
}
