// CoreAudio AudioServerPlugInDriverInterface implementation.
//
// This file implements the C vtable that coreaudiod calls into.
// All functions are extern "C" and wrapped in catch_unwind to prevent
// panics from crashing coreaudiod.
//
// Reference: AudioServerPlugIn.h from CoreAudio framework.

use crate::io_engine::IoEngine;
use crate::mmap_ipc::MmapIpc;
use crate::object_model::ObjectStore;
use std::ffi::c_void;
use std::os::raw::c_int;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Mutex;

/// OSStatus type (i32 on macOS)
pub type OSStatus = i32;

/// HRESULT-like type for QueryInterface
pub type HRESULT = i32;

/// Common OSStatus values
pub const NO_ERR: OSStatus = 0;
pub const ERR_UNSUPPORTED: OSStatus = -4; // kAudio_UnimplementedError
pub const ERR_BAD_OBJECT: OSStatus = 0x21_6F626A; // '!obj' — kAudioHardwareBadObjectError
pub const ERR_BAD_PROPERTY: OSStatus = 0x21_70726F; // '!pro' — kAudioHardwareUnknownPropertyError

/// AudioObjectPropertyAddress — identifies a property on an audio object.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct AudioObjectPropertyAddress {
    pub selector: u32,
    pub scope: u32,
    pub element: u32,
}

/// The driver vtable — matches AudioServerPlugInDriverInterface layout.
/// Each field is a function pointer that coreaudiod calls.
#[repr(C)]
pub struct PluginDriverInterface {
    // IUnknown-style methods (CFPlugIn requirement)
    pub _reserved: *const c_void,
    pub query_interface: unsafe extern "C" fn(
        driver: *mut c_void, uuid: [u8; 16], out: *mut *mut c_void,
    ) -> HRESULT,
    pub add_ref: unsafe extern "C" fn(driver: *mut c_void) -> u32,
    pub release: unsafe extern "C" fn(driver: *mut c_void) -> u32,

    // AudioServerPlugInDriverInterface methods
    pub initialize: unsafe extern "C" fn(
        driver: *mut c_void, host: *mut c_void,
    ) -> OSStatus,
    pub create_device: unsafe extern "C" fn(
        driver: *mut c_void, desc: *const c_void,
        request_ownership: u8, device_id_out: *mut u32,
    ) -> OSStatus,
    pub destroy_device: unsafe extern "C" fn(
        driver: *mut c_void, device_id: u32,
    ) -> OSStatus,
    pub add_device_client: unsafe extern "C" fn(
        driver: *mut c_void, device_id: u32, client_info: *const c_void,
    ) -> OSStatus,
    pub remove_device_client: unsafe extern "C" fn(
        driver: *mut c_void, device_id: u32, client_info: *const c_void,
    ) -> OSStatus,
    pub perform_device_config_change: unsafe extern "C" fn(
        driver: *mut c_void, device_id: u32, change_action: u64, change_info: *mut c_void,
    ) -> OSStatus,
    pub abort_device_config_change: unsafe extern "C" fn(
        driver: *mut c_void, device_id: u32, change_action: u64, change_info: *mut c_void,
    ) -> OSStatus,

    // Property operations
    pub has_property: unsafe extern "C" fn(
        driver: *mut c_void, object_id: u32, client_pid: u32,
        address: *const AudioObjectPropertyAddress,
    ) -> u8,
    pub is_property_settable: unsafe extern "C" fn(
        driver: *mut c_void, object_id: u32, client_pid: u32,
        address: *const AudioObjectPropertyAddress, out_settable: *mut u8,
    ) -> OSStatus,
    pub get_property_data_size: unsafe extern "C" fn(
        driver: *mut c_void, object_id: u32, client_pid: u32,
        address: *const AudioObjectPropertyAddress,
        qualifier_size: u32, qualifier: *const c_void,
        out_size: *mut u32,
    ) -> OSStatus,
    pub get_property_data: unsafe extern "C" fn(
        driver: *mut c_void, object_id: u32, client_pid: u32,
        address: *const AudioObjectPropertyAddress,
        qualifier_size: u32, qualifier: *const c_void,
        io_data_size: *mut u32, out_data: *mut c_void,
    ) -> OSStatus,
    pub set_property_data: unsafe extern "C" fn(
        driver: *mut c_void, object_id: u32, client_pid: u32,
        address: *const AudioObjectPropertyAddress,
        qualifier_size: u32, qualifier: *const c_void,
        data_size: u32, data: *const c_void,
    ) -> OSStatus,

    // IO operations
    pub start_io: unsafe extern "C" fn(
        driver: *mut c_void, device_id: u32, client_id: u32,
    ) -> OSStatus,
    pub stop_io: unsafe extern "C" fn(
        driver: *mut c_void, device_id: u32, client_id: u32,
    ) -> OSStatus,
    pub get_zero_time_stamp: unsafe extern "C" fn(
        driver: *mut c_void, device_id: u32,
        out_sample_time: *mut f64, out_host_time: *mut u64,
        out_seed: *mut u64,
    ) -> OSStatus,
    pub will_do_io_operation: unsafe extern "C" fn(
        driver: *mut c_void, device_id: u32, client_id: u32,
        operation_id: u32, out_will_do: *mut u8, out_is_input: *mut u8,
    ) -> OSStatus,
    pub begin_io_operation: unsafe extern "C" fn(
        driver: *mut c_void, device_id: u32, client_id: u32,
        operation_id: u32, io_buffer_frame_size: u32,
        io_cycle_info: *const c_void,
    ) -> OSStatus,
    pub do_io_operation: unsafe extern "C" fn(
        driver: *mut c_void, device_id: u32,
        stream_id: u32, client_id: u32,
        operation_id: u32, io_buffer_frame_size: u32,
        io_cycle_info: *const c_void,
        io_main_buffer: *mut c_void, io_secondary_buffer: *mut c_void,
    ) -> OSStatus,
    pub end_io_operation: unsafe extern "C" fn(
        driver: *mut c_void, device_id: u32, client_id: u32,
        operation_id: u32, io_buffer_frame_size: u32,
        io_cycle_info: *const c_void,
    ) -> OSStatus,
}

/// Global driver state, created by the factory function.
pub struct DriverState {
    pub ref_count: AtomicU32,
    pub host: *mut c_void,
    pub io_engine: IoEngine,
    pub mmap: Option<MmapIpc>,
    pub objects: ObjectStore,
    pub io_running: bool,
    pub io_client_count: u32,
    pub sample_rate: f64,
    pub zero_time_stamp_counter: u64,
    /// Mutex for property access (non-RT thread).
    /// IO thread must NOT take this lock.
    pub property_lock: Mutex<()>,
}

/// The vtable + driver state, allocated together.
/// coreaudiod holds a pointer to the vtable at offset 0.
#[repr(C)]
pub struct DriverInstance {
    pub vtable_ptr: *const PluginDriverInterface,
    pub vtable: PluginDriverInterface,
    pub state: DriverState,
}

// ─── Factory function ───
// This is the entry point called by coreaudiod via CFPlugIn.

/// # Safety
/// Called by coreaudiod. Must return a valid pointer to a PluginDriverInterface.
#[no_mangle]
pub unsafe extern "C" fn phaselith_driver_factory(
    _allocator: *mut c_void,
    requested_type_uuid: [u8; 16],
) -> *mut c_void {
    use crate::constants::AUDIO_SERVER_PLUGIN_TYPE_UUID;

    // Verify the requested type is AudioServerPlugIn
    if requested_type_uuid != AUDIO_SERVER_PLUGIN_TYPE_UUID {
        return std::ptr::null_mut();
    }

    ca_log!("phaselith_driver_factory: creating driver instance");

    let instance = Box::new(DriverInstance {
        vtable_ptr: std::ptr::null(), // will point to self.vtable
        vtable: create_vtable(),
        state: DriverState {
            ref_count: AtomicU32::new(1),
            host: std::ptr::null_mut(),
            io_engine: IoEngine::new(),
            mmap: None,
            objects: ObjectStore::new(),
            io_running: false,
            io_client_count: 0,
            sample_rate: crate::constants::DEFAULT_SAMPLE_RATE,
            zero_time_stamp_counter: 0,
            property_lock: Mutex::new(()),
        },
    });

    let raw = Box::into_raw(instance);
    // Point vtable_ptr to the vtable field within the same allocation
    (*raw).vtable_ptr = &(*raw).vtable as *const PluginDriverInterface;

    ca_log!("phaselith_driver_factory: driver instance created at {:p}", raw);

    raw as *mut c_void
}

fn create_vtable() -> PluginDriverInterface {
    PluginDriverInterface {
        _reserved: std::ptr::null(),
        query_interface: driver_query_interface,
        add_ref: driver_add_ref,
        release: driver_release,
        initialize: driver_initialize,
        create_device: driver_create_device,
        destroy_device: driver_destroy_device,
        add_device_client: driver_add_device_client,
        remove_device_client: driver_remove_device_client,
        perform_device_config_change: driver_perform_device_config_change,
        abort_device_config_change: driver_abort_device_config_change,
        has_property: driver_has_property,
        is_property_settable: driver_is_property_settable,
        get_property_data_size: driver_get_property_data_size,
        get_property_data: driver_get_property_data,
        set_property_data: driver_set_property_data,
        start_io: driver_start_io,
        stop_io: driver_stop_io,
        get_zero_time_stamp: driver_get_zero_time_stamp,
        will_do_io_operation: driver_will_do_io_operation,
        begin_io_operation: driver_begin_io_operation,
        do_io_operation: driver_do_io_operation,
        end_io_operation: driver_end_io_operation,
    }
}

// ─── Helper to recover DriverState from driver pointer ───

unsafe fn get_state<'a>(driver: *mut c_void) -> &'a mut DriverState {
    let instance = driver as *mut DriverInstance;
    &mut (*instance).state
}

// ─── IUnknown methods ───

unsafe extern "C" fn driver_query_interface(
    driver: *mut c_void, _uuid: [u8; 16], out: *mut *mut c_void,
) -> HRESULT {
    // Return self for any interface query (simplified)
    if !out.is_null() {
        *out = driver;
        let state = get_state(driver);
        state.ref_count.fetch_add(1, Ordering::Relaxed);
    }
    0 // S_OK
}

unsafe extern "C" fn driver_add_ref(driver: *mut c_void) -> u32 {
    let state = get_state(driver);
    state.ref_count.fetch_add(1, Ordering::Relaxed) + 1
}

unsafe extern "C" fn driver_release(driver: *mut c_void) -> u32 {
    let state = get_state(driver);
    let prev = state.ref_count.fetch_sub(1, Ordering::Relaxed);
    if prev == 1 {
        // Last reference — deallocate
        ca_log!("driver_release: deallocating driver instance");
        drop(Box::from_raw(driver as *mut DriverInstance));
        return 0;
    }
    prev - 1
}

// ─── Plugin lifecycle ───

unsafe extern "C" fn driver_initialize(
    driver: *mut c_void, host: *mut c_void,
) -> OSStatus {
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let state = get_state(driver);
        state.host = host;

        ca_log!("driver_initialize: host={:p}", host);

        // Initialize IPC
        match MmapIpc::open_or_create() {
            Ok(ipc) => state.mmap = Some(ipc),
            Err(e) => {
                ca_log!("driver_initialize: IPC init failed: {e}");
                // Non-fatal: continue without IPC
            }
        }

        // Initialize IO engine
        state.io_engine.initialize(state.sample_rate as u32, 2);

        NO_ERR
    }));

    result.unwrap_or_else(|_| {
        ca_log!("driver_initialize: PANIC caught");
        ERR_UNSUPPORTED
    })
}

unsafe extern "C" fn driver_create_device(
    _driver: *mut c_void, _desc: *const c_void,
    _request_ownership: u8, _device_id_out: *mut u32,
) -> OSStatus {
    // We don't support dynamic device creation
    ERR_UNSUPPORTED
}

unsafe extern "C" fn driver_destroy_device(
    _driver: *mut c_void, _device_id: u32,
) -> OSStatus {
    ERR_UNSUPPORTED
}

unsafe extern "C" fn driver_add_device_client(
    _driver: *mut c_void, _device_id: u32, _client_info: *const c_void,
) -> OSStatus {
    NO_ERR
}

unsafe extern "C" fn driver_remove_device_client(
    _driver: *mut c_void, _device_id: u32, _client_info: *const c_void,
) -> OSStatus {
    NO_ERR
}

unsafe extern "C" fn driver_perform_device_config_change(
    _driver: *mut c_void, _device_id: u32, _change_action: u64, _change_info: *mut c_void,
) -> OSStatus {
    NO_ERR
}

unsafe extern "C" fn driver_abort_device_config_change(
    _driver: *mut c_void, _device_id: u32, _change_action: u64, _change_info: *mut c_void,
) -> OSStatus {
    NO_ERR
}

// ─── Property operations ───
// Delegated to properties.rs

unsafe extern "C" fn driver_has_property(
    driver: *mut c_void, object_id: u32, _client_pid: u32,
    address: *const AudioObjectPropertyAddress,
) -> u8 {
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let state = get_state(driver);
        let addr = &*address;
        crate::properties::has_property(&state.objects, object_id, addr) as u8
    }));
    result.unwrap_or(0)
}

unsafe extern "C" fn driver_is_property_settable(
    driver: *mut c_void, object_id: u32, _client_pid: u32,
    address: *const AudioObjectPropertyAddress, out_settable: *mut u8,
) -> OSStatus {
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let state = get_state(driver);
        let addr = &*address;
        if !out_settable.is_null() {
            *out_settable = crate::properties::is_property_settable(&state.objects, object_id, addr) as u8;
        }
        NO_ERR
    }));
    result.unwrap_or(ERR_UNSUPPORTED)
}

unsafe extern "C" fn driver_get_property_data_size(
    driver: *mut c_void, object_id: u32, _client_pid: u32,
    address: *const AudioObjectPropertyAddress,
    _qualifier_size: u32, _qualifier: *const c_void,
    out_size: *mut u32,
) -> OSStatus {
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let state = get_state(driver);
        let addr = &*address;
        match crate::properties::get_property_data_size(&state.objects, object_id, addr) {
            Some(size) => {
                if !out_size.is_null() {
                    *out_size = size;
                }
                NO_ERR
            }
            None => ERR_BAD_PROPERTY,
        }
    }));
    result.unwrap_or(ERR_UNSUPPORTED)
}

unsafe extern "C" fn driver_get_property_data(
    driver: *mut c_void, object_id: u32, _client_pid: u32,
    address: *const AudioObjectPropertyAddress,
    _qualifier_size: u32, _qualifier: *const c_void,
    io_data_size: *mut u32, out_data: *mut c_void,
) -> OSStatus {
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let state = get_state(driver);
        let addr = &*address;
        let available_size = if !io_data_size.is_null() { *io_data_size } else { 0 };
        match crate::properties::get_property_data(
            &state.objects, object_id, addr,
            state.sample_rate, state.io_running,
            out_data as *mut u8, available_size,
        ) {
            Some(written) => {
                if !io_data_size.is_null() {
                    *io_data_size = written;
                }
                NO_ERR
            }
            None => ERR_BAD_PROPERTY,
        }
    }));
    result.unwrap_or(ERR_UNSUPPORTED)
}

unsafe extern "C" fn driver_set_property_data(
    driver: *mut c_void, object_id: u32, _client_pid: u32,
    address: *const AudioObjectPropertyAddress,
    _qualifier_size: u32, _qualifier: *const c_void,
    data_size: u32, data: *const c_void,
) -> OSStatus {
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let state = get_state(driver);
        let addr = &*address;
        crate::properties::set_property_data(
            state, object_id, addr,
            data as *const u8, data_size,
        )
    }));
    result.unwrap_or(ERR_UNSUPPORTED)
}

// ─── IO operations ───

unsafe extern "C" fn driver_start_io(
    driver: *mut c_void, _device_id: u32, _client_id: u32,
) -> OSStatus {
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let state = get_state(driver);
        state.io_client_count += 1;
        if !state.io_running {
            ca_log!("driver_start_io: starting IO engine");
            // Default frame size: 512 frames at current sample rate
            let frame_size = 512;
            state.io_engine.start(frame_size);
            state.io_running = true;
        }
        NO_ERR
    }));
    result.unwrap_or_else(|_| {
        ca_log!("driver_start_io: PANIC caught");
        ERR_UNSUPPORTED
    })
}

unsafe extern "C" fn driver_stop_io(
    driver: *mut c_void, _device_id: u32, _client_id: u32,
) -> OSStatus {
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let state = get_state(driver);
        if state.io_client_count > 0 {
            state.io_client_count -= 1;
        }
        if state.io_client_count == 0 && state.io_running {
            ca_log!("driver_stop_io: stopping IO engine");
            state.io_engine.stop();
            state.io_running = false;
        }
        NO_ERR
    }));
    result.unwrap_or(ERR_UNSUPPORTED)
}

unsafe extern "C" fn driver_get_zero_time_stamp(
    driver: *mut c_void, _device_id: u32,
    out_sample_time: *mut f64, out_host_time: *mut u64,
    out_seed: *mut u64,
) -> OSStatus {
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let state = get_state(driver);
        // TODO: implement proper clock synchronization with mach_absolute_time()
        // For now, return a simple counter-based timestamp
        state.zero_time_stamp_counter += 1;
        if !out_sample_time.is_null() {
            *out_sample_time = 0.0;
        }
        if !out_host_time.is_null() {
            *out_host_time = 0;
        }
        if !out_seed.is_null() {
            *out_seed = state.zero_time_stamp_counter;
        }
        NO_ERR
    }));
    result.unwrap_or(ERR_UNSUPPORTED)
}

unsafe extern "C" fn driver_will_do_io_operation(
    _driver: *mut c_void, _device_id: u32, _client_id: u32,
    operation_id: u32, out_will_do: *mut u8, out_is_input: *mut u8,
) -> OSStatus {
    // Operation IDs:
    // 1 = ReadInput, 2 = ProcessOutput, 3 = WriteMix
    if !out_will_do.is_null() {
        *out_will_do = match operation_id {
            2 => 1, // We do ProcessOutput
            _ => 0,
        };
    }
    if !out_is_input.is_null() {
        *out_is_input = 0; // We're an output processor
    }
    NO_ERR
}

unsafe extern "C" fn driver_begin_io_operation(
    _driver: *mut c_void, _device_id: u32, _client_id: u32,
    _operation_id: u32, _io_buffer_frame_size: u32,
    _io_cycle_info: *const c_void,
) -> OSStatus {
    NO_ERR
}

unsafe extern "C" fn driver_do_io_operation(
    driver: *mut c_void, _device_id: u32,
    _stream_id: u32, _client_id: u32,
    operation_id: u32, io_buffer_frame_size: u32,
    _io_cycle_info: *const c_void,
    io_main_buffer: *mut c_void, _io_secondary_buffer: *mut c_void,
) -> OSStatus {
    if operation_id != 2 { return NO_ERR; } // Only ProcessOutput

    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let state = get_state(driver);
        let frames = io_buffer_frame_size as usize;
        let channels = 2usize;
        let sample_count = frames * channels;

        if io_main_buffer.is_null() || sample_count == 0 {
            return NO_ERR;
        }

        let buffer = std::slice::from_raw_parts_mut(
            io_main_buffer as *mut f32,
            sample_count,
        );

        // Read enabled state from IPC
        let enabled = if let Some(ref mmap) = state.mmap {
            let config = mmap.config();
            state.io_engine.update_config_from_shared(config);
            config.is_enabled()
        } else {
            false
        };

        // Process in-place: copy input, process, write output
        // The buffer is both input and output for in-place processing
        let input_copy = buffer.to_vec(); // TODO: pre-allocate this
        state.io_engine.process(&input_copy, buffer, enabled);

        // Update status
        if let Some(ref mmap) = state.mmap {
            state.io_engine.write_status(mmap.status(), &input_copy, buffer, enabled);
        }

        NO_ERR
    }));

    result.unwrap_or_else(|_| {
        ca_log!("driver_do_io_operation: PANIC caught — passthrough");
        NO_ERR
    })
}

unsafe extern "C" fn driver_end_io_operation(
    _driver: *mut c_void, _device_id: u32, _client_id: u32,
    _operation_id: u32, _io_buffer_frame_size: u32,
    _io_cycle_info: *const c_void,
) -> OSStatus {
    NO_ERR
}
