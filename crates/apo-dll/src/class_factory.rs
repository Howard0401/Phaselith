// IClassFactory implementation for ASCE APO.
//
// Windows COM calls DllGetClassObject → IClassFactory::CreateInstance → our APO.
// Uses windows-rs #[implement] macro for proper COM vtable generation.

use crate::apo_com::AsceApoCom;
use crate::guids::CLSID_ASCE_APO;

use std::sync::atomic::{AtomicU32, Ordering};
use windows::core::*;
use windows::Win32::Foundation::*;
use windows::Win32::System::Com::*;

/// Global lock count for DllCanUnloadNow
static SERVER_LOCK_COUNT: AtomicU32 = AtomicU32::new(0);

pub fn can_unload() -> bool {
    SERVER_LOCK_COUNT.load(Ordering::SeqCst) == 0
}

/// COM Class Factory for creating ASCE APO instances.
#[implement(IClassFactory)]
pub struct AsceClassFactory;

impl IClassFactory_Impl for AsceClassFactory_Impl {
    fn CreateInstance(
        &self,
        punkouter: Option<&IUnknown>,
        riid: *const GUID,
        ppvobject: *mut *mut core::ffi::c_void,
    ) -> Result<()> {
        if ppvobject.is_null() {
            return Err(E_POINTER.into());
        }
        unsafe { *ppvobject = std::ptr::null_mut(); }

        // No aggregation support
        if punkouter.is_some() {
            return Err(CLASS_E_NOAGGREGATION.into());
        }

        // Create our APO COM object
        let apo: IUnknown = AsceApoCom::new().into();

        // QueryInterface for the requested interface
        unsafe { apo.query(riid, ppvobject).ok() }
    }

    fn LockServer(&self, flock: BOOL) -> Result<()> {
        if flock.as_bool() {
            SERVER_LOCK_COUNT.fetch_add(1, Ordering::SeqCst);
        } else {
            SERVER_LOCK_COUNT.fetch_sub(1, Ordering::SeqCst);
        }
        Ok(())
    }
}

/// Called by DllGetClassObject to get an IClassFactory for our CLSID.
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

    // Create class factory and QueryInterface for the requested interface
    let factory: IUnknown = AsceClassFactory.into();
    unsafe { factory.query(riid, ppv) }
}
