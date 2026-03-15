// IClassFactory implementation for Phaselith APO.
//
// Windows COM calls DllGetClassObject → IClassFactory::CreateInstance → our APO.
// Uses windows-rs #[implement] macro for proper COM vtable generation.
//
// COM Aggregation: Windows Audio Engine always creates APOs with pUnkOuter.
// When aggregating, we return a non-delegating IUnknown via AggregatedApo.

use crate::aggregation::AggregatedApo;
use crate::apo_com::PhaselithApoCom;
use crate::guids::CLSID_PHASELITH_APO;

use std::sync::atomic::{AtomicU32, Ordering};
use windows::core::*;
use windows::Win32::Foundation::*;
use windows::Win32::System::Com::*;

/// Global lock count for DllCanUnloadNow
static SERVER_LOCK_COUNT: AtomicU32 = AtomicU32::new(0);

pub fn can_unload() -> bool {
    SERVER_LOCK_COUNT.load(Ordering::SeqCst) == 0
}

/// COM Class Factory for creating Phaselith APO instances.
#[implement(IClassFactory)]
pub struct PhaselithClassFactory;

impl IClassFactory_Impl for PhaselithClassFactory_Impl {
    fn CreateInstance(
        &self,
        punkouter: Option<&IUnknown>,
        riid: *const GUID,
        ppvobject: *mut *mut core::ffi::c_void,
    ) -> Result<()> {
        let riid_val = unsafe { *riid };
        apo_log!("CreateInstance called, riid={:?}", riid_val);

        if ppvobject.is_null() {
            apo_log!("CreateInstance: ppvobject is null");
            return Err(E_POINTER.into());
        }
        unsafe { *ppvobject = std::ptr::null_mut(); }

        if let Some(outer) = punkouter {
            // COM Aggregation: pUnkOuter is provided.
            // Per COM rules, riid MUST be IID_IUnknown when aggregating.
            if riid_val != IUnknown::IID {
                apo_log!("CreateInstance: aggregation requested but riid != IID_IUnknown");
                return Err(CLASS_E_NOAGGREGATION.into());
            }

            // Create aggregated wrapper that provides non-delegating IUnknown
            // IMPORTANT: outer is &IUnknown which is a reference to a transparent wrapper.
            // We need the underlying COM pointer value, not the address of the reference.
            let outer_raw = unsafe { *(outer as *const IUnknown as *const *mut core::ffi::c_void) };
            let nd_unknown = AggregatedApo::create(outer_raw);
            apo_log!("CreateInstance: created aggregated APO at {:p}", nd_unknown);
            unsafe { *ppvobject = nd_unknown; }
            Ok(())
        } else {
            // Non-aggregated: create object directly
            apo_log!("CreateInstance: non-aggregated, creating PhaselithApoCom directly");
            let apo: IUnknown = PhaselithApoCom::new().into();
            let result = unsafe { apo.query(riid, ppvobject) };
            apo_log!("CreateInstance: query result={:?}", result);
            result.ok()
        }
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

    if *rclsid != CLSID_PHASELITH_APO {
        return CLASS_E_CLASSNOTAVAILABLE;
    }

    // Create class factory and QueryInterface for the requested interface
    let factory: IUnknown = PhaselithClassFactory.into();
    unsafe { factory.query(riid, ppv) }
}
