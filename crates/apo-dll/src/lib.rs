// ASCE APO COM DLL
//
// cdylib loaded by audiodg.exe via Windows Audio Engine.
//
// Loading path:
//   regsvr32 asce_apo.dll → DllRegisterServer() writes registry
//   audiodg.exe → DllGetClassObject() → AsceApo
//   Audio thread → APOProcess() (real-time, zero-alloc)

#[cfg(windows)]
mod guids;
#[cfg(windows)]
mod class_factory;
#[cfg(windows)]
mod apo_impl;
#[cfg(windows)]
mod format_negotiate;
#[cfg(windows)]
mod mmap_ipc;
#[cfg(windows)]
mod registry;

#[cfg(windows)]
use windows::core::HRESULT;
#[cfg(windows)]
use windows::Win32::Foundation::*;

#[cfg(windows)]
#[no_mangle]
pub extern "system" fn DllMain(_hinst: HINSTANCE, _reason: u32, _reserved: *mut ()) -> BOOL {
    TRUE
}

#[cfg(windows)]
#[no_mangle]
pub extern "system" fn DllGetClassObject(
    rclsid: *const windows::core::GUID,
    riid: *const windows::core::GUID,
    ppv: *mut *mut core::ffi::c_void,
) -> HRESULT {
    if rclsid.is_null() || riid.is_null() || ppv.is_null() {
        return E_POINTER;
    }
    unsafe {
        class_factory::AsceClassFactory::get_class_object(&*rclsid, &*riid, ppv)
    }
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
