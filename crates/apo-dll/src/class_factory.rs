// IClassFactory implementation for ASCE APO.
//
// Windows COM calls DllGetClassObject → IClassFactory::CreateInstance → our APO.
// This is the standard COM object creation pattern.

use crate::apo_impl::AsceApo;
use crate::guids::CLSID_ASCE_APO;

use std::sync::atomic::{AtomicU32, Ordering};
use windows::core::*;
use windows::Win32::Foundation::*;
use windows::Win32::System::Com::*;

/// Global reference count for DllCanUnloadNow
static GLOBAL_REF_COUNT: AtomicU32 = AtomicU32::new(0);

pub fn can_unload() -> bool {
    GLOBAL_REF_COUNT.load(Ordering::SeqCst) == 0
}

/// COM Class Factory for creating ASCE APO instances
pub struct AsceClassFactory;

impl AsceClassFactory {
    pub fn new() -> Self {
        GLOBAL_REF_COUNT.fetch_add(1, Ordering::SeqCst);
        Self
    }

    /// Called by DllGetClassObject to get an IClassFactory for our CLSID
    pub fn get_class_object(
        rclsid: &GUID,
        riid: &GUID,
        ppv: *mut *mut core::ffi::c_void,
    ) -> HRESULT {
        if ppv.is_null() {
            return E_POINTER;
        }

        unsafe { *ppv = std::ptr::null_mut(); }

        if *rclsid != CLSID_ASCE_APO {
            return CLASS_E_CLASSNOTAVAILABLE;
        }

        // For now, create a simple pass-through APO wrapper
        // In production, this would use #[implement(IClassFactory)] from windows-rs
        // but for Phase 1 we use a minimal COM-compatible approach
        let factory = AsceClassFactory::new();

        // Store the factory and return its IUnknown-compatible pointer
        // This is a simplified version - full COM vtable in production
        let _ = factory; // prevent drop
        let _ = riid;

        // Create the APO directly for now
        let apo = Box::new(AsceApo::new());
        let ptr = Box::into_raw(apo);
        unsafe { *ppv = ptr as *mut core::ffi::c_void; }

        S_OK
    }
}

impl Drop for AsceClassFactory {
    fn drop(&mut self) {
        GLOBAL_REF_COUNT.fetch_sub(1, Ordering::SeqCst);
    }
}

/// Create an APO instance (called from DllGetClassObject path)
pub fn create_apo_instance() -> Box<AsceApo> {
    GLOBAL_REF_COUNT.fetch_add(1, Ordering::SeqCst);
    Box::new(AsceApo::new())
}
