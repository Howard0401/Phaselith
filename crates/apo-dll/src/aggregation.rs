// COM aggregation support for Phaselith APO.
//
// Windows Audio Engine REQUIRES aggregation. windows-rs #[implement] doesn't support it.
//
// Strategy: "Tear-off" wrappers. Each tear-off is a #[repr(C)] struct whose first
// field is a vtable pointer. The vtable has:
//   - entries 0-2: delegating QI/AddRef/Release → outer IUnknown
//   - entries 3+:  forwarding thunks → inner's original method implementations
//
// The thunks read the inner's real interface pointer from the tear-off, then tail-call
// the inner's original vtable method with the correct `this`.

use std::ffi::c_void;
use std::sync::atomic::{AtomicU32, Ordering};

use windows::core::{IUnknown, IUnknown_Vtbl, GUID, HRESULT, Interface};
use windows::Win32::Foundation::{E_NOINTERFACE, E_POINTER, S_OK};
use windows::Win32::Media::Audio::Apo::*;

use crate::apo_com::PhaselithApoCom;

// ---------------------------------------------------------------------------
// Tear-off: COM-compatible wrapper that delegates IUnknown to outer
// and forwards methods to inner.
// ---------------------------------------------------------------------------

/// A tear-off COM interface wrapper.
/// Layout: [vtbl_ptr, inner_ptr, owner_ptr]
/// COM sees only vtbl_ptr (first pointer-sized field).
#[repr(C)]
struct TearOff {
    vtbl: *const c_void,
    inner_ptr: *mut c_void,    // inner's interface pointer for this specific interface
    owner: *mut AggregatedApo, // back-pointer for delegating to outer
}

// ---------------------------------------------------------------------------
// AggregatedApo: the aggregation wrapper returned from CreateInstance
// ---------------------------------------------------------------------------

#[repr(C)]
pub struct AggregatedApo {
    // Non-delegating IUnknown vtable (first field → this IS an IUnknown)
    nd_vtbl: *const IUnknown_Vtbl,
    ref_count: AtomicU32,
    outer: *mut c_void,
    inner: IUnknown,

    // One tear-off per interface
    to_apo: TearOff,
    to_apo_rt: TearOff,
    to_apo_config: TearOff,
    to_syseffects: TearOff,
    to_syseffects2: TearOff,
    to_syseffects3: TearOff,
}

// ---------------------------------------------------------------------------
// Non-delegating IUnknown vtable
// ---------------------------------------------------------------------------

static ND_VTBL: IUnknown_Vtbl = IUnknown_Vtbl {
    QueryInterface: nd_query_interface,
    AddRef: nd_add_ref,
    Release: nd_release,
};

// ---------------------------------------------------------------------------
// Delegating IUnknown (shared by all tear-offs)
// ---------------------------------------------------------------------------

unsafe extern "system" fn tearoff_qi(
    this: *mut c_void,
    riid: *const GUID,
    ppv: *mut *mut c_void,
) -> HRESULT {
    let to = this as *const TearOff;
    let owner = (*to).owner;
    let outer = (*owner).outer;
    apo_log!("tearoff_qi: this={:p} owner={:p} outer={:p} riid={:?}", this, owner, outer, &*riid);
    let vtbl = *(outer as *const *const IUnknown_Vtbl);
    ((*vtbl).QueryInterface)(outer, riid, ppv)
}

unsafe extern "system" fn tearoff_addref(this: *mut c_void) -> u32 {
    let to = this as *const TearOff;
    let owner = (*to).owner;
    let outer = (*owner).outer;
    let vtbl = *(outer as *const *const IUnknown_Vtbl);
    let r = ((*vtbl).AddRef)(outer);
    apo_log!("tearoff_addref: this={:p} outer={:p} → {}", this, outer, r);
    r
}

unsafe extern "system" fn tearoff_release(this: *mut c_void) -> u32 {
    let to = this as *const TearOff;
    let owner = (*to).owner;
    let outer = (*owner).outer;
    let vtbl = *(outer as *const *const IUnknown_Vtbl);
    let r = ((*vtbl).Release)(outer);
    apo_log!("tearoff_release: this={:p} outer={:p} → {}", this, outer, r);
    r
}

// ---------------------------------------------------------------------------
// Helper: read a function pointer from an inner COM vtable by index
// ---------------------------------------------------------------------------

#[inline(always)]
unsafe fn inner_method(tearoff: *const TearOff, index: usize) -> *const c_void {
    let inner = (*tearoff).inner_ptr;
    let vtbl = *(inner as *const *const *const c_void);
    *vtbl.add(index)
}

// ---------------------------------------------------------------------------
// Forwarding thunks — IAudioProcessingObject (indices 3..9)
// vtable: [QI, AddRef, Release, Reset, GetLatency, GetRegProps, Init,
//          IsInputFmt, IsOutputFmt, GetInputChCount]
// ---------------------------------------------------------------------------

unsafe extern "system" fn fwd_apo_reset(this: *mut c_void) -> HRESULT {
    let to = this as *const TearOff;
    let inner = (*to).inner_ptr;
    let f: unsafe extern "system" fn(*mut c_void) -> HRESULT =
        std::mem::transmute(inner_method(to, 3));
    f(inner)
}

unsafe extern "system" fn fwd_apo_get_latency(this: *mut c_void, p: *mut i64) -> HRESULT {
    let to = this as *const TearOff;
    let inner = (*to).inner_ptr;
    let f: unsafe extern "system" fn(*mut c_void, *mut i64) -> HRESULT =
        std::mem::transmute(inner_method(to, 4));
    f(inner, p)
}

unsafe extern "system" fn fwd_apo_get_reg_props(
    this: *mut c_void,
    pp: *mut *mut c_void,
) -> HRESULT {
    let to = this as *const TearOff;
    let inner = (*to).inner_ptr;
    let f: unsafe extern "system" fn(*mut c_void, *mut *mut c_void) -> HRESULT =
        std::mem::transmute(inner_method(to, 5));
    f(inner, pp)
}

unsafe extern "system" fn fwd_apo_initialize(
    this: *mut c_void,
    cb: u32,
    data: *const u8,
) -> HRESULT {
    apo_log!("fwd_apo_initialize: this={:p} cb={}", this, cb);
    let to = this as *const TearOff;
    let inner = (*to).inner_ptr;
    let f: unsafe extern "system" fn(*mut c_void, u32, *const u8) -> HRESULT =
        std::mem::transmute(inner_method(to, 6));
    let hr = f(inner, cb, data);
    apo_log!("fwd_apo_initialize: result={:?}", hr);
    hr
}

unsafe extern "system" fn fwd_apo_is_input_fmt(
    this: *mut c_void,
    opp: *mut c_void,
    req: *mut c_void,
    sup: *mut *mut c_void,
) -> HRESULT {
    let to = this as *const TearOff;
    let inner = (*to).inner_ptr;
    let f: unsafe extern "system" fn(*mut c_void, *mut c_void, *mut c_void, *mut *mut c_void) -> HRESULT =
        std::mem::transmute(inner_method(to, 7));
    f(inner, opp, req, sup)
}

unsafe extern "system" fn fwd_apo_is_output_fmt(
    this: *mut c_void,
    opp: *mut c_void,
    req: *mut c_void,
    sup: *mut *mut c_void,
) -> HRESULT {
    let to = this as *const TearOff;
    let inner = (*to).inner_ptr;
    let f: unsafe extern "system" fn(*mut c_void, *mut c_void, *mut c_void, *mut *mut c_void) -> HRESULT =
        std::mem::transmute(inner_method(to, 8));
    f(inner, opp, req, sup)
}

unsafe extern "system" fn fwd_apo_get_input_ch(this: *mut c_void, p: *mut u32) -> HRESULT {
    let to = this as *const TearOff;
    let inner = (*to).inner_ptr;
    let f: unsafe extern "system" fn(*mut c_void, *mut u32) -> HRESULT =
        std::mem::transmute(inner_method(to, 9));
    f(inner, p)
}

// ---------------------------------------------------------------------------
// Forwarding thunks — IAudioProcessingObjectRT (indices 3..5)
// vtable: [QI, AddRef, Release, APOProcess, CalcInputFrames, CalcOutputFrames]
// ---------------------------------------------------------------------------

unsafe extern "system" fn fwd_rt_process(
    this: *mut c_void,
    num_in: u32,
    pp_in: *const c_void,
    num_out: u32,
    pp_out: *mut c_void,
) {
    // No logging here — this is the real-time audio path (called ~100x/sec)
    let to = this as *const TearOff;
    let inner = (*to).inner_ptr;
    let f: unsafe extern "system" fn(*mut c_void, u32, *const c_void, u32, *mut c_void) =
        std::mem::transmute(inner_method(to, 3));
    f(inner, num_in, pp_in, num_out, pp_out)
}

unsafe extern "system" fn fwd_rt_calc_input(this: *mut c_void, frames: u32) -> u32 {
    let to = this as *const TearOff;
    let inner = (*to).inner_ptr;
    let f: unsafe extern "system" fn(*mut c_void, u32) -> u32 =
        std::mem::transmute(inner_method(to, 4));
    f(inner, frames)
}

unsafe extern "system" fn fwd_rt_calc_output(this: *mut c_void, frames: u32) -> u32 {
    let to = this as *const TearOff;
    let inner = (*to).inner_ptr;
    let f: unsafe extern "system" fn(*mut c_void, u32) -> u32 =
        std::mem::transmute(inner_method(to, 5));
    f(inner, frames)
}

// ---------------------------------------------------------------------------
// Forwarding thunks — IAudioProcessingObjectConfiguration (indices 3..4)
// vtable: [QI, AddRef, Release, LockForProcess, UnlockForProcess]
// ---------------------------------------------------------------------------

unsafe extern "system" fn fwd_cfg_lock(
    this: *mut c_void,
    num_in: u32,
    pp_in: *const c_void,
    num_out: u32,
    pp_out: *const c_void,
) -> HRESULT {
    apo_log!("fwd_cfg_lock: this={:p} num_in={} num_out={}", this, num_in, num_out);
    let to = this as *const TearOff;
    let inner = (*to).inner_ptr;
    let f: unsafe extern "system" fn(*mut c_void, u32, *const c_void, u32, *const c_void) -> HRESULT =
        std::mem::transmute(inner_method(to, 3));
    let hr = f(inner, num_in, pp_in, num_out, pp_out);
    apo_log!("fwd_cfg_lock: result={:?}", hr);
    hr
}

unsafe extern "system" fn fwd_cfg_unlock(this: *mut c_void) -> HRESULT {
    let to = this as *const TearOff;
    let inner = (*to).inner_ptr;
    let f: unsafe extern "system" fn(*mut c_void) -> HRESULT =
        std::mem::transmute(inner_method(to, 3 + 1));
    f(inner)
}

// ---------------------------------------------------------------------------
// Forwarding thunks — IAudioSystemEffects2 (index 3)
// vtable: [QI, AddRef, Release, GetEffectsList]
// ---------------------------------------------------------------------------

unsafe extern "system" fn fwd_se2_get_effects(
    this: *mut c_void,
    pp_ids: *mut *mut c_void,
    count: *mut u32,
    event: *mut c_void,
) -> HRESULT {
    let to = this as *const TearOff;
    let inner = (*to).inner_ptr;
    let f: unsafe extern "system" fn(*mut c_void, *mut *mut c_void, *mut u32, *mut c_void) -> HRESULT =
        std::mem::transmute(inner_method(to, 3));
    f(inner, pp_ids, count, event)
}

// ---------------------------------------------------------------------------
// Forwarding thunks — IAudioSystemEffects3 (indices 3..5)
// vtable: [QI, AddRef, Release, GetEffectsList,
//          GetControllableSystemEffectsList, SetAudioSystemEffectState]
// ---------------------------------------------------------------------------

unsafe extern "system" fn fwd_se3_get_effects(
    this: *mut c_void,
    pp_ids: *mut *mut c_void,
    count: *mut u32,
    event: *mut c_void,
) -> HRESULT {
    let to = this as *const TearOff;
    let inner = (*to).inner_ptr;
    let f: unsafe extern "system" fn(*mut c_void, *mut *mut c_void, *mut u32, *mut c_void) -> HRESULT =
        std::mem::transmute(inner_method(to, 3));
    f(inner, pp_ids, count, event)
}

unsafe extern "system" fn fwd_se3_get_controllable(
    this: *mut c_void,
    effects: *mut *mut c_void,
    num: *mut u32,
    event: *mut c_void,
) -> HRESULT {
    let to = this as *const TearOff;
    let inner = (*to).inner_ptr;
    let f: unsafe extern "system" fn(*mut c_void, *mut *mut c_void, *mut u32, *mut c_void) -> HRESULT =
        std::mem::transmute(inner_method(to, 4));
    f(inner, effects, num, event)
}

unsafe extern "system" fn fwd_se3_set_state(
    this: *mut c_void,
    id: *const c_void,
    state: i32,
) -> HRESULT {
    let to = this as *const TearOff;
    let inner = (*to).inner_ptr;
    let f: unsafe extern "system" fn(*mut c_void, *const c_void, i32) -> HRESULT =
        std::mem::transmute(inner_method(to, 5));
    f(inner, id, state)
}

// ---------------------------------------------------------------------------
// Static vtable arrays for each tear-off interface
// ---------------------------------------------------------------------------

// IAudioProcessingObject: 3 IUnknown + 7 methods = 10 entries
#[repr(C)]
struct VtblApo {
    qi: unsafe extern "system" fn(*mut c_void, *const GUID, *mut *mut c_void) -> HRESULT,
    addref: unsafe extern "system" fn(*mut c_void) -> u32,
    release: unsafe extern "system" fn(*mut c_void) -> u32,
    reset: unsafe extern "system" fn(*mut c_void) -> HRESULT,
    get_latency: unsafe extern "system" fn(*mut c_void, *mut i64) -> HRESULT,
    get_reg_props: unsafe extern "system" fn(*mut c_void, *mut *mut c_void) -> HRESULT,
    initialize: unsafe extern "system" fn(*mut c_void, u32, *const u8) -> HRESULT,
    is_input_fmt: unsafe extern "system" fn(*mut c_void, *mut c_void, *mut c_void, *mut *mut c_void) -> HRESULT,
    is_output_fmt: unsafe extern "system" fn(*mut c_void, *mut c_void, *mut c_void, *mut *mut c_void) -> HRESULT,
    get_input_ch: unsafe extern "system" fn(*mut c_void, *mut u32) -> HRESULT,
}

static VTBL_APO: VtblApo = VtblApo {
    qi: tearoff_qi,
    addref: tearoff_addref,
    release: tearoff_release,
    reset: fwd_apo_reset,
    get_latency: fwd_apo_get_latency,
    get_reg_props: fwd_apo_get_reg_props,
    initialize: fwd_apo_initialize,
    is_input_fmt: fwd_apo_is_input_fmt,
    is_output_fmt: fwd_apo_is_output_fmt,
    get_input_ch: fwd_apo_get_input_ch,
};

// IAudioProcessingObjectRT: 3 IUnknown + 3 methods = 6 entries
#[repr(C)]
struct VtblApoRt {
    qi: unsafe extern "system" fn(*mut c_void, *const GUID, *mut *mut c_void) -> HRESULT,
    addref: unsafe extern "system" fn(*mut c_void) -> u32,
    release: unsafe extern "system" fn(*mut c_void) -> u32,
    process: unsafe extern "system" fn(*mut c_void, u32, *const c_void, u32, *mut c_void),
    calc_input: unsafe extern "system" fn(*mut c_void, u32) -> u32,
    calc_output: unsafe extern "system" fn(*mut c_void, u32) -> u32,
}

static VTBL_APO_RT: VtblApoRt = VtblApoRt {
    qi: tearoff_qi,
    addref: tearoff_addref,
    release: tearoff_release,
    process: fwd_rt_process,
    calc_input: fwd_rt_calc_input,
    calc_output: fwd_rt_calc_output,
};

// IAudioProcessingObjectConfiguration: 3 IUnknown + 2 methods = 5 entries
#[repr(C)]
struct VtblApoCfg {
    qi: unsafe extern "system" fn(*mut c_void, *const GUID, *mut *mut c_void) -> HRESULT,
    addref: unsafe extern "system" fn(*mut c_void) -> u32,
    release: unsafe extern "system" fn(*mut c_void) -> u32,
    lock: unsafe extern "system" fn(*mut c_void, u32, *const c_void, u32, *const c_void) -> HRESULT,
    unlock: unsafe extern "system" fn(*mut c_void) -> HRESULT,
}

static VTBL_APO_CFG: VtblApoCfg = VtblApoCfg {
    qi: tearoff_qi,
    addref: tearoff_addref,
    release: tearoff_release,
    lock: fwd_cfg_lock,
    unlock: fwd_cfg_unlock,
};

// IAudioSystemEffects: 3 IUnknown + 0 methods = 3 entries (marker interface)
static VTBL_SE: IUnknown_Vtbl = IUnknown_Vtbl {
    QueryInterface: tearoff_qi,
    AddRef: tearoff_addref,
    Release: tearoff_release,
};

// IAudioSystemEffects2: 3 IUnknown + 1 method = 4 entries
#[repr(C)]
struct VtblSe2 {
    qi: unsafe extern "system" fn(*mut c_void, *const GUID, *mut *mut c_void) -> HRESULT,
    addref: unsafe extern "system" fn(*mut c_void) -> u32,
    release: unsafe extern "system" fn(*mut c_void) -> u32,
    get_effects: unsafe extern "system" fn(*mut c_void, *mut *mut c_void, *mut u32, *mut c_void) -> HRESULT,
}

static VTBL_SE2: VtblSe2 = VtblSe2 {
    qi: tearoff_qi,
    addref: tearoff_addref,
    release: tearoff_release,
    get_effects: fwd_se2_get_effects,
};

// IAudioSystemEffects3: 3 IUnknown + 3 methods = 6 entries
#[repr(C)]
struct VtblSe3 {
    qi: unsafe extern "system" fn(*mut c_void, *const GUID, *mut *mut c_void) -> HRESULT,
    addref: unsafe extern "system" fn(*mut c_void) -> u32,
    release: unsafe extern "system" fn(*mut c_void) -> u32,
    get_effects: unsafe extern "system" fn(*mut c_void, *mut *mut c_void, *mut u32, *mut c_void) -> HRESULT,
    get_controllable: unsafe extern "system" fn(*mut c_void, *mut *mut c_void, *mut u32, *mut c_void) -> HRESULT,
    set_state: unsafe extern "system" fn(*mut c_void, *const c_void, i32) -> HRESULT,
}

static VTBL_SE3: VtblSe3 = VtblSe3 {
    qi: tearoff_qi,
    addref: tearoff_addref,
    release: tearoff_release,
    get_effects: fwd_se3_get_effects,
    get_controllable: fwd_se3_get_controllable,
    set_state: fwd_se3_set_state,
};

// ---------------------------------------------------------------------------
// Non-delegating IUnknown implementation
// ---------------------------------------------------------------------------

unsafe extern "system" fn nd_query_interface(
    this: *mut c_void,
    riid: *const GUID,
    ppv: *mut *mut c_void,
) -> HRESULT {
    if ppv.is_null() {
        return E_POINTER;
    }
    *ppv = std::ptr::null_mut();

    let obj = this as *mut AggregatedApo;
    let riid_val = &*riid;

    // IID_IUnknown → return non-delegating self
    if *riid_val == IUnknown::IID {
        apo_log!("nd_QI: IUnknown → self");
        *ppv = this;
        nd_add_ref(this);
        return S_OK;
    }

    // IAudioProcessingObject / RT / Configuration
    if *riid_val == IAudioProcessingObject::IID
        || *riid_val == IAudioProcessingObjectRT::IID
        || *riid_val == IAudioProcessingObjectConfiguration::IID
    {
        // Determine which tear-off to return
        let to = if *riid_val == IAudioProcessingObject::IID {
            apo_log!("nd_QI: IAudioProcessingObject → to_apo");
            &mut (*obj).to_apo as *mut TearOff as *mut c_void
        } else if *riid_val == IAudioProcessingObjectRT::IID {
            apo_log!("nd_QI: IAudioProcessingObjectRT → to_apo_rt");
            &mut (*obj).to_apo_rt as *mut TearOff as *mut c_void
        } else {
            apo_log!("nd_QI: IAudioProcessingObjectConfiguration → to_apo_config");
            &mut (*obj).to_apo_config as *mut TearOff as *mut c_void
        };
        *ppv = to;
        // AddRef via outer (delegating)
        tearoff_addref(to);
        return S_OK;
    }

    // IAudioSystemEffects
    if *riid_val == IAudioSystemEffects::IID {
        apo_log!("nd_QI: IAudioSystemEffects → to_syseffects");
        let to = &mut (*obj).to_syseffects as *mut TearOff as *mut c_void;
        *ppv = to;
        tearoff_addref(to);
        return S_OK;
    }

    // IAudioSystemEffects2
    if *riid_val == IAudioSystemEffects2::IID {
        apo_log!("nd_QI: IAudioSystemEffects2 → to_syseffects2");
        let to = &mut (*obj).to_syseffects2 as *mut TearOff as *mut c_void;
        *ppv = to;
        tearoff_addref(to);
        return S_OK;
    }

    // IAudioSystemEffects3
    if *riid_val == IAudioSystemEffects3::IID {
        apo_log!("nd_QI: IAudioSystemEffects3 → to_syseffects3");
        let to = &mut (*obj).to_syseffects3 as *mut TearOff as *mut c_void;
        *ppv = to;
        tearoff_addref(to);
        return S_OK;
    }

    apo_log!("nd_QI: {:?} → E_NOINTERFACE", riid_val);
    E_NOINTERFACE
}

unsafe extern "system" fn nd_add_ref(this: *mut c_void) -> u32 {
    let obj = this as *mut AggregatedApo;
    let new = (*obj).ref_count.fetch_add(1, Ordering::SeqCst) + 1;
    apo_log!("nd_AddRef → {}", new);
    new
}

unsafe extern "system" fn nd_release(this: *mut c_void) -> u32 {
    let obj = this as *mut AggregatedApo;
    let new = (*obj).ref_count.fetch_sub(1, Ordering::SeqCst) - 1;
    apo_log!("nd_Release → {}", new);
    if new == 0 {
        apo_log!("AggregatedApo: ref_count=0, dropping");
        // Drop the AggregatedApo (releases inner IUnknown → inner freed)
        let _ = Box::from_raw(obj);
    }
    new
}

// ---------------------------------------------------------------------------
// Helper: QI the inner for an interface and return the raw pointer.
// Caller must release the returned pointer when done (or store it).
// ---------------------------------------------------------------------------

unsafe fn qi_inner(inner: &IUnknown, iid: &GUID) -> *mut c_void {
    let mut ptr: *mut c_void = std::ptr::null_mut();
    let hr = inner.query(iid, &mut ptr);
    if hr == S_OK && !ptr.is_null() {
        // Release the ref — inner stays alive via AggregatedApo.inner field.
        // We only needed the pointer value to store in the tear-off.
        let vtbl = *(ptr as *const *const IUnknown_Vtbl);
        ((*vtbl).Release)(ptr);
        ptr
    } else {
        std::ptr::null_mut()
    }
}

// ---------------------------------------------------------------------------
// AggregatedApo::create
// ---------------------------------------------------------------------------

impl AggregatedApo {
    pub fn create(outer: *mut c_void) -> *mut c_void {
        apo_log!("AggregatedApo::create: outer={:p}", outer);

        let inner: IUnknown = PhaselithApoCom::new().into();

        // QI inner for each interface to get the correct `this` pointers.
        // These are offsets into the inner COM object for each interface vtable.
        let ptr_apo = unsafe { qi_inner(&inner, &IAudioProcessingObject::IID) };
        let ptr_apo_rt = unsafe { qi_inner(&inner, &IAudioProcessingObjectRT::IID) };
        let ptr_apo_cfg = unsafe { qi_inner(&inner, &IAudioProcessingObjectConfiguration::IID) };
        let ptr_se = unsafe { qi_inner(&inner, &IAudioSystemEffects::IID) };
        let ptr_se2 = unsafe { qi_inner(&inner, &IAudioSystemEffects2::IID) };
        let ptr_se3 = unsafe { qi_inner(&inner, &IAudioSystemEffects3::IID) };

        apo_log!("  inner ptrs: apo={:p} rt={:p} cfg={:p} se={:p} se2={:p} se3={:p}",
            ptr_apo, ptr_apo_rt, ptr_apo_cfg, ptr_se, ptr_se2, ptr_se3);

        let null_owner = std::ptr::null_mut();

        let wrapper = Box::new(AggregatedApo {
            nd_vtbl: &ND_VTBL,
            ref_count: AtomicU32::new(1),
            outer,
            inner,
            to_apo: TearOff {
                vtbl: &VTBL_APO as *const VtblApo as *const c_void,
                inner_ptr: ptr_apo,
                owner: null_owner,
            },
            to_apo_rt: TearOff {
                vtbl: &VTBL_APO_RT as *const VtblApoRt as *const c_void,
                inner_ptr: ptr_apo_rt,
                owner: null_owner,
            },
            to_apo_config: TearOff {
                vtbl: &VTBL_APO_CFG as *const VtblApoCfg as *const c_void,
                inner_ptr: ptr_apo_cfg,
                owner: null_owner,
            },
            to_syseffects: TearOff {
                vtbl: &VTBL_SE as *const IUnknown_Vtbl as *const c_void,
                inner_ptr: ptr_se,
                owner: null_owner,
            },
            to_syseffects2: TearOff {
                vtbl: &VTBL_SE2 as *const VtblSe2 as *const c_void,
                inner_ptr: ptr_se2,
                owner: null_owner,
            },
            to_syseffects3: TearOff {
                vtbl: &VTBL_SE3 as *const VtblSe3 as *const c_void,
                inner_ptr: ptr_se3,
                owner: null_owner,
            },
        });

        let ptr = Box::into_raw(wrapper);

        // Fix up owner back-pointers
        unsafe {
            (*ptr).to_apo.owner = ptr;
            (*ptr).to_apo_rt.owner = ptr;
            (*ptr).to_apo_config.owner = ptr;
            (*ptr).to_syseffects.owner = ptr;
            (*ptr).to_syseffects2.owner = ptr;
            (*ptr).to_syseffects3.owner = ptr;
        }

        apo_log!("AggregatedApo::create: wrapper at {:p}", ptr);
        ptr as *mut c_void
    }
}
