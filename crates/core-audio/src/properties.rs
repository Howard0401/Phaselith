// CoreAudio property dispatch.
//
// Handles HasProperty, IsPropertySettable, GetPropertyDataSize,
// GetPropertyData, and SetPropertyData for all audio objects.
//
// Each function receives an AudioObjectID + PropertyAddress and dispatches
// based on the object type and property selector.

#[allow(unused_imports)]
use crate::constants::*;
#[allow(unused_imports)]
use crate::object_model::{ObjectStore, ObjectType};
#[cfg(target_os = "macos")]
use crate::plugin_interface::{AudioObjectPropertyAddress, DriverState, OSStatus, NO_ERR, ERR_BAD_PROPERTY};

// ─── AudioStreamBasicDescription (repr(C), matches CoreAudio layout) ───

#[repr(C)]
#[derive(Clone, Copy)]
pub struct AudioStreamBasicDescription {
    pub sample_rate: f64,
    pub format_id: u32,
    pub format_flags: u32,
    pub bytes_per_packet: u32,
    pub frames_per_packet: u32,
    pub bytes_per_frame: u32,
    pub channels_per_frame: u32,
    pub bits_per_channel: u32,
    pub reserved: u32,
}

/// AudioStreamBasicDescription format flags
const FORMAT_FLAG_FLOAT: u32 = 1;
const FORMAT_FLAG_BIG_ENDIAN: u32 = 2;
const FORMAT_FLAG_PACKED: u32 = 8;
const FORMAT_FLAG_NON_INTERLEAVED: u32 = 32;

/// AudioStreamRangedDescription — wraps ASBD with sample rate range
#[repr(C)]
#[derive(Clone, Copy)]
pub struct AudioStreamRangedDescription {
    pub format: AudioStreamBasicDescription,
    pub sample_rate_range_min: f64,
    pub sample_rate_range_max: f64,
}

/// AudioValueRange
#[repr(C)]
#[derive(Clone, Copy)]
pub struct AudioValueRange {
    pub minimum: f64,
    pub maximum: f64,
}

impl AudioStreamBasicDescription {
    fn stereo_float32(sample_rate: f64) -> Self {
        let channels = DEFAULT_CHANNELS;
        let bits = DEFAULT_BITS_PER_CHANNEL;
        let bytes_per_frame = channels * (bits / 8);
        Self {
            sample_rate,
            format_id: AUDIO_FORMAT_LINEAR_PCM,
            format_flags: FORMAT_FLAG_FLOAT | FORMAT_FLAG_PACKED,
            bytes_per_packet: bytes_per_frame,
            frames_per_packet: 1,
            bytes_per_frame,
            channels_per_frame: channels,
            bits_per_channel: bits,
            reserved: 0,
        }
    }
}

// ─── Property dispatch functions ───

/// Check if a property exists on a given object.
#[cfg(target_os = "macos")]
pub fn has_property(
    objects: &ObjectStore, object_id: u32, address: &AudioObjectPropertyAddress,
) -> bool {
    let obj_type = match objects.object_type(object_id) {
        Some(t) => t,
        None => return false,
    };

    match obj_type {
        ObjectType::Plugin => has_plugin_property(address.selector),
        ObjectType::Device => has_device_property(address.selector),
        ObjectType::StreamInput | ObjectType::StreamOutput => has_stream_property(address.selector),
        ObjectType::VolumeControl => has_volume_property(address.selector),
    }
}

/// Check if a property is settable.
#[cfg(target_os = "macos")]
pub fn is_property_settable(
    objects: &ObjectStore, object_id: u32, address: &AudioObjectPropertyAddress,
) -> bool {
    let obj_type = match objects.object_type(object_id) {
        Some(t) => t,
        None => return false,
    };

    match (obj_type, address.selector) {
        (ObjectType::Device, DEVICE_PROPERTY_NOMINAL_SAMPLE_RATE) => true,
        (ObjectType::VolumeControl, LEVEL_CONTROL_PROPERTY_SCALAR_VALUE) => true,
        (ObjectType::VolumeControl, LEVEL_CONTROL_PROPERTY_DECIBEL_VALUE) => true,
        _ => false,
    }
}

/// Get the data size for a property.
#[cfg(target_os = "macos")]
pub fn get_property_data_size(
    objects: &ObjectStore, object_id: u32, address: &AudioObjectPropertyAddress,
) -> Option<u32> {
    let obj_type = match objects.object_type(object_id) {
        Some(t) => t,
        None => return None,
    };

    let size = match obj_type {
        ObjectType::Plugin => plugin_property_size(objects, address.selector),
        ObjectType::Device => device_property_size(objects, address.selector),
        ObjectType::StreamInput | ObjectType::StreamOutput => stream_property_size(address.selector),
        ObjectType::VolumeControl => volume_property_size(address.selector),
    };

    size
}

/// Get property data, writing to the output buffer.
/// Returns the number of bytes written, or None if the property doesn't exist.
#[cfg(target_os = "macos")]
pub fn get_property_data(
    objects: &ObjectStore, object_id: u32, address: &AudioObjectPropertyAddress,
    sample_rate: f64, io_running: bool,
    out: *mut u8, available_size: u32,
) -> Option<u32> {
    let obj_type = match objects.object_type(object_id) {
        Some(t) => t,
        None => return None,
    };

    match obj_type {
        ObjectType::Plugin => get_plugin_property(objects, address, out, available_size),
        ObjectType::Device => get_device_property(objects, address, sample_rate, io_running, out, available_size),
        ObjectType::StreamInput | ObjectType::StreamOutput => {
            get_stream_property(obj_type, address, sample_rate, out, available_size)
        }
        ObjectType::VolumeControl => get_volume_property(objects, address, out, available_size),
    }
}

/// Set property data.
#[cfg(target_os = "macos")]
pub fn set_property_data(
    state: &mut DriverState, object_id: u32, address: &AudioObjectPropertyAddress,
    data: *const u8, data_size: u32,
) -> OSStatus {
    let obj_type = match state.objects.object_type(object_id) {
        Some(t) => t,
        None => return ERR_BAD_PROPERTY,
    };

    match (obj_type, address.selector) {
        (ObjectType::Device, DEVICE_PROPERTY_NOMINAL_SAMPLE_RATE) => {
            if data_size >= 8 && !data.is_null() {
                let rate = unsafe { *(data as *const f64) };
                if SUPPORTED_SAMPLE_RATES.contains(&rate) {
                    ca_log!("set_property_data: sample rate → {}", rate);
                    state.sample_rate = rate;
                    state.io_engine.initialize(rate as u32, 2);
                    return NO_ERR;
                }
            }
            ERR_BAD_PROPERTY
        }
        (ObjectType::VolumeControl, LEVEL_CONTROL_PROPERTY_SCALAR_VALUE) => {
            if data_size >= 4 && !data.is_null() {
                let val = unsafe { *(data as *const f32) };
                state.objects.volume_scalar = val.clamp(0.0, 1.0);
                return NO_ERR;
            }
            ERR_BAD_PROPERTY
        }
        _ => ERR_BAD_PROPERTY,
    }
}

// ─── Plugin properties ───

fn has_plugin_property(selector: u32) -> bool {
    matches!(selector,
        PROPERTY_BASE_CLASS | PROPERTY_CLASS | PROPERTY_OWNER |
        PROPERTY_NAME | PROPERTY_MANUFACTURER |
        PLUGIN_PROPERTY_DEVICE_LIST | PLUGIN_PROPERTY_TRANSLATE_UID |
        PLUGIN_PROPERTY_RESOURCE_BUNDLE
    )
}

#[cfg(target_os = "macos")]
fn plugin_property_size(objects: &ObjectStore, selector: u32) -> Option<u32> {
    Some(match selector {
        PROPERTY_BASE_CLASS | PROPERTY_CLASS | PROPERTY_OWNER => 4,
        PROPERTY_NAME | PROPERTY_MANUFACTURER | PLUGIN_PROPERTY_RESOURCE_BUNDLE => std::mem::size_of::<*const std::ffi::c_void>() as u32,
        PLUGIN_PROPERTY_DEVICE_LIST => (objects.children_of(PLUGIN_OBJECT_ID).len() * 4) as u32,
        PLUGIN_PROPERTY_TRANSLATE_UID => 4,
        _ => return None,
    })
}

#[cfg(target_os = "macos")]
fn get_plugin_property(
    objects: &ObjectStore, address: &AudioObjectPropertyAddress,
    out: *mut u8, available_size: u32,
) -> Option<u32> {
    if out.is_null() { return None; }

    match address.selector {
        PROPERTY_BASE_CLASS => write_u32(out, available_size, CLASS_OBJECT),
        PROPERTY_CLASS => write_u32(out, available_size, CLASS_PLUGIN),
        PROPERTY_OWNER => write_u32(out, available_size, PLUGIN_OBJECT_ID),
        PROPERTY_NAME => {
            // Return a CFStringRef — requires CoreFoundation
            // For now, write device name as a placeholder
            write_u32(out, available_size, 0) // TODO: create CFString
        }
        PLUGIN_PROPERTY_DEVICE_LIST => {
            let devices = objects.children_of(PLUGIN_OBJECT_ID);
            write_u32_array(out, available_size, devices)
        }
        _ => None,
    }
}

// ─── Device properties ───

fn has_device_property(selector: u32) -> bool {
    matches!(selector,
        PROPERTY_BASE_CLASS | PROPERTY_CLASS | PROPERTY_OWNER |
        PROPERTY_NAME | PROPERTY_MANUFACTURER | PROPERTY_OWNED_OBJECTS |
        DEVICE_PROPERTY_UID | DEVICE_PROPERTY_MODEL_UID |
        DEVICE_PROPERTY_TRANSPORT | DEVICE_PROPERTY_RELATED |
        DEVICE_PROPERTY_CLOCK_DOMAIN | DEVICE_PROPERTY_ALIVE |
        DEVICE_PROPERTY_RUNNING | DEVICE_PROPERTY_CAN_BE_DEFAULT |
        DEVICE_PROPERTY_CAN_BE_SYSTEM | DEVICE_PROPERTY_LATENCY |
        DEVICE_PROPERTY_STREAMS | DEVICE_PROPERTY_CONTROLS |
        DEVICE_PROPERTY_SAFETY_OFFSET | DEVICE_PROPERTY_NOMINAL_SAMPLE_RATE |
        DEVICE_PROPERTY_AVAILABLE_SAMPLE_RATES | DEVICE_PROPERTY_HIDDEN |
        DEVICE_PROPERTY_ZERO_TIME_STAMP_PERIOD |
        DEVICE_PROPERTY_PREFERRED_CHANNELS
    )
}

#[cfg(target_os = "macos")]
fn device_property_size(objects: &ObjectStore, selector: u32) -> Option<u32> {
    Some(match selector {
        PROPERTY_BASE_CLASS | PROPERTY_CLASS | PROPERTY_OWNER => 4,
        PROPERTY_NAME | PROPERTY_MANUFACTURER => std::mem::size_of::<*const std::ffi::c_void>() as u32,
        PROPERTY_OWNED_OBJECTS => (objects.children_of(DEVICE_OBJECT_ID).len() * 4) as u32,
        DEVICE_PROPERTY_UID | DEVICE_PROPERTY_MODEL_UID => std::mem::size_of::<*const std::ffi::c_void>() as u32,
        DEVICE_PROPERTY_TRANSPORT | DEVICE_PROPERTY_CLOCK_DOMAIN => 4,
        DEVICE_PROPERTY_RELATED => 4,
        DEVICE_PROPERTY_ALIVE | DEVICE_PROPERTY_RUNNING => 4,
        DEVICE_PROPERTY_CAN_BE_DEFAULT | DEVICE_PROPERTY_CAN_BE_SYSTEM => 4,
        DEVICE_PROPERTY_LATENCY | DEVICE_PROPERTY_SAFETY_OFFSET => 4,
        DEVICE_PROPERTY_STREAMS => (objects.streams_of(DEVICE_OBJECT_ID).len() * 4) as u32,
        DEVICE_PROPERTY_CONTROLS => (objects.controls_of(DEVICE_OBJECT_ID).len() * 4) as u32,
        DEVICE_PROPERTY_NOMINAL_SAMPLE_RATE => 8,
        DEVICE_PROPERTY_AVAILABLE_SAMPLE_RATES => (SUPPORTED_SAMPLE_RATES.len() * std::mem::size_of::<AudioValueRange>()) as u32,
        DEVICE_PROPERTY_HIDDEN => 4,
        DEVICE_PROPERTY_ZERO_TIME_STAMP_PERIOD => 4,
        DEVICE_PROPERTY_PREFERRED_CHANNELS => 8, // 2 x u32
        _ => return None,
    })
}

#[cfg(target_os = "macos")]
fn get_device_property(
    objects: &ObjectStore, address: &AudioObjectPropertyAddress,
    sample_rate: f64, io_running: bool,
    out: *mut u8, available_size: u32,
) -> Option<u32> {
    if out.is_null() { return None; }

    match address.selector {
        PROPERTY_BASE_CLASS => write_u32(out, available_size, CLASS_OBJECT),
        PROPERTY_CLASS => write_u32(out, available_size, CLASS_DEVICE),
        PROPERTY_OWNER => write_u32(out, available_size, PLUGIN_OBJECT_ID),
        PROPERTY_OWNED_OBJECTS => {
            write_u32_array(out, available_size, objects.children_of(DEVICE_OBJECT_ID))
        }
        DEVICE_PROPERTY_UID => {
            write_u32(out, available_size, 0) // TODO: CFString
        }
        DEVICE_PROPERTY_TRANSPORT => write_u32(out, available_size, TRANSPORT_TYPE_VIRTUAL),
        DEVICE_PROPERTY_CLOCK_DOMAIN => write_u32(out, available_size, 0),
        DEVICE_PROPERTY_ALIVE => write_u32(out, available_size, 1),
        DEVICE_PROPERTY_RUNNING => write_u32(out, available_size, io_running as u32),
        DEVICE_PROPERTY_CAN_BE_DEFAULT => write_u32(out, available_size, 1),
        DEVICE_PROPERTY_CAN_BE_SYSTEM => write_u32(out, available_size, 1),
        DEVICE_PROPERTY_LATENCY => write_u32(out, available_size, 0),
        DEVICE_PROPERTY_SAFETY_OFFSET => write_u32(out, available_size, 0),
        DEVICE_PROPERTY_HIDDEN => write_u32(out, available_size, 0),
        DEVICE_PROPERTY_STREAMS => {
            write_u32_array(out, available_size, objects.streams_of(DEVICE_OBJECT_ID))
        }
        DEVICE_PROPERTY_CONTROLS => {
            write_u32_array(out, available_size, objects.controls_of(DEVICE_OBJECT_ID))
        }
        DEVICE_PROPERTY_NOMINAL_SAMPLE_RATE => {
            if available_size >= 8 {
                unsafe { *(out as *mut f64) = sample_rate; }
                Some(8)
            } else {
                None
            }
        }
        DEVICE_PROPERTY_AVAILABLE_SAMPLE_RATES => {
            let ranges: Vec<AudioValueRange> = SUPPORTED_SAMPLE_RATES.iter().map(|&r| {
                AudioValueRange { minimum: r, maximum: r }
            }).collect();
            let size = (ranges.len() * std::mem::size_of::<AudioValueRange>()) as u32;
            if available_size >= size {
                unsafe {
                    std::ptr::copy_nonoverlapping(
                        ranges.as_ptr() as *const u8, out, size as usize,
                    );
                }
                Some(size)
            } else {
                None
            }
        }
        DEVICE_PROPERTY_ZERO_TIME_STAMP_PERIOD => {
            // Number of frames between zero time stamps
            write_u32(out, available_size, sample_rate as u32)
        }
        DEVICE_PROPERTY_PREFERRED_CHANNELS => {
            // Two channels: 1 (left) and 2 (right)
            if available_size >= 8 {
                unsafe {
                    *(out as *mut u32) = 1;
                    *((out as *mut u32).add(1)) = 2;
                }
                Some(8)
            } else {
                None
            }
        }
        _ => None,
    }
}

// ─── Stream properties ───

fn has_stream_property(selector: u32) -> bool {
    matches!(selector,
        PROPERTY_BASE_CLASS | PROPERTY_CLASS | PROPERTY_OWNER |
        PROPERTY_NAME | STREAM_PROPERTY_ACTIVE |
        STREAM_PROPERTY_DIRECTION | STREAM_PROPERTY_TERMINAL_TYPE |
        STREAM_PROPERTY_START_CHANNEL | STREAM_PROPERTY_LATENCY |
        STREAM_PROPERTY_VIRTUAL_FORMAT | STREAM_PROPERTY_PHYSICAL_FORMAT |
        STREAM_PROPERTY_AVAILABLE_VIRTUAL_FORMATS |
        STREAM_PROPERTY_AVAILABLE_PHYSICAL_FORMATS
    )
}

#[cfg(target_os = "macos")]
fn stream_property_size(selector: u32) -> Option<u32> {
    Some(match selector {
        PROPERTY_BASE_CLASS | PROPERTY_CLASS | PROPERTY_OWNER => 4,
        PROPERTY_NAME => std::mem::size_of::<*const std::ffi::c_void>() as u32,
        STREAM_PROPERTY_ACTIVE => 4,
        STREAM_PROPERTY_DIRECTION | STREAM_PROPERTY_TERMINAL_TYPE => 4,
        STREAM_PROPERTY_START_CHANNEL | STREAM_PROPERTY_LATENCY => 4,
        STREAM_PROPERTY_VIRTUAL_FORMAT | STREAM_PROPERTY_PHYSICAL_FORMAT => {
            std::mem::size_of::<AudioStreamBasicDescription>() as u32
        }
        STREAM_PROPERTY_AVAILABLE_VIRTUAL_FORMATS |
        STREAM_PROPERTY_AVAILABLE_PHYSICAL_FORMATS => {
            (SUPPORTED_SAMPLE_RATES.len() * std::mem::size_of::<AudioStreamRangedDescription>()) as u32
        }
        _ => return None,
    })
}

#[cfg(target_os = "macos")]
fn get_stream_property(
    obj_type: ObjectType, address: &AudioObjectPropertyAddress,
    sample_rate: f64,
    out: *mut u8, available_size: u32,
) -> Option<u32> {
    if out.is_null() { return None; }

    match address.selector {
        PROPERTY_BASE_CLASS => write_u32(out, available_size, CLASS_OBJECT),
        PROPERTY_CLASS => write_u32(out, available_size, CLASS_STREAM),
        PROPERTY_OWNER => write_u32(out, available_size, DEVICE_OBJECT_ID),
        STREAM_PROPERTY_ACTIVE => write_u32(out, available_size, 1),
        STREAM_PROPERTY_DIRECTION => {
            // 0 = output, 1 = input
            let dir = if obj_type == ObjectType::StreamInput { 1u32 } else { 0u32 };
            write_u32(out, available_size, dir)
        }
        STREAM_PROPERTY_TERMINAL_TYPE => {
            let term = if obj_type == ObjectType::StreamInput {
                INPUT_TERMINAL_MICROPHONE
            } else {
                OUTPUT_TERMINAL_SPEAKER
            };
            write_u32(out, available_size, term)
        }
        STREAM_PROPERTY_START_CHANNEL => write_u32(out, available_size, 1),
        STREAM_PROPERTY_LATENCY => write_u32(out, available_size, 0),
        STREAM_PROPERTY_VIRTUAL_FORMAT | STREAM_PROPERTY_PHYSICAL_FORMAT => {
            let asbd = AudioStreamBasicDescription::stereo_float32(sample_rate);
            let size = std::mem::size_of::<AudioStreamBasicDescription>() as u32;
            if available_size >= size {
                unsafe {
                    std::ptr::copy_nonoverlapping(
                        &asbd as *const _ as *const u8, out, size as usize,
                    );
                }
                Some(size)
            } else {
                None
            }
        }
        STREAM_PROPERTY_AVAILABLE_VIRTUAL_FORMATS |
        STREAM_PROPERTY_AVAILABLE_PHYSICAL_FORMATS => {
            let formats: Vec<AudioStreamRangedDescription> = SUPPORTED_SAMPLE_RATES.iter().map(|&r| {
                AudioStreamRangedDescription {
                    format: AudioStreamBasicDescription::stereo_float32(r),
                    sample_rate_range_min: r,
                    sample_rate_range_max: r,
                }
            }).collect();
            let size = (formats.len() * std::mem::size_of::<AudioStreamRangedDescription>()) as u32;
            if available_size >= size {
                unsafe {
                    std::ptr::copy_nonoverlapping(
                        formats.as_ptr() as *const u8, out, size as usize,
                    );
                }
                Some(size)
            } else {
                None
            }
        }
        _ => None,
    }
}

// ─── Volume control properties ───

fn has_volume_property(selector: u32) -> bool {
    matches!(selector,
        PROPERTY_BASE_CLASS | PROPERTY_CLASS | PROPERTY_OWNER |
        LEVEL_CONTROL_PROPERTY_SCALAR_VALUE |
        LEVEL_CONTROL_PROPERTY_DECIBEL_VALUE |
        LEVEL_CONTROL_PROPERTY_DECIBEL_RANGE |
        LEVEL_CONTROL_PROPERTY_CONVERT_SCALAR_TO_DB |
        LEVEL_CONTROL_PROPERTY_CONVERT_DB_TO_SCALAR
    )
}

#[cfg(target_os = "macos")]
fn volume_property_size(selector: u32) -> Option<u32> {
    Some(match selector {
        PROPERTY_BASE_CLASS | PROPERTY_CLASS | PROPERTY_OWNER => 4,
        LEVEL_CONTROL_PROPERTY_SCALAR_VALUE => 4,
        LEVEL_CONTROL_PROPERTY_DECIBEL_VALUE => 4,
        LEVEL_CONTROL_PROPERTY_DECIBEL_RANGE => std::mem::size_of::<AudioValueRange>() as u32,
        LEVEL_CONTROL_PROPERTY_CONVERT_SCALAR_TO_DB => 4,
        LEVEL_CONTROL_PROPERTY_CONVERT_DB_TO_SCALAR => 4,
        _ => return None,
    })
}

#[cfg(target_os = "macos")]
fn get_volume_property(
    objects: &ObjectStore, address: &AudioObjectPropertyAddress,
    out: *mut u8, available_size: u32,
) -> Option<u32> {
    if out.is_null() { return None; }

    match address.selector {
        PROPERTY_BASE_CLASS => write_u32(out, available_size, CLASS_LEVEL_CONTROL),
        PROPERTY_CLASS => write_u32(out, available_size, CLASS_VOLUME_CONTROL),
        PROPERTY_OWNER => write_u32(out, available_size, DEVICE_OBJECT_ID),
        LEVEL_CONTROL_PROPERTY_SCALAR_VALUE => {
            if available_size >= 4 {
                unsafe { *(out as *mut f32) = objects.volume_scalar; }
                Some(4)
            } else {
                None
            }
        }
        LEVEL_CONTROL_PROPERTY_DECIBEL_VALUE => {
            let db = scalar_to_db(objects.volume_scalar);
            if available_size >= 4 {
                unsafe { *(out as *mut f32) = db; }
                Some(4)
            } else {
                None
            }
        }
        LEVEL_CONTROL_PROPERTY_DECIBEL_RANGE => {
            let range = AudioValueRange { minimum: -96.0, maximum: 0.0 };
            let size = std::mem::size_of::<AudioValueRange>() as u32;
            if available_size >= size {
                unsafe {
                    std::ptr::copy_nonoverlapping(
                        &range as *const _ as *const u8, out, size as usize,
                    );
                }
                Some(size)
            } else {
                None
            }
        }
        _ => None,
    }
}

// ─── Helpers ───

fn scalar_to_db(scalar: f32) -> f32 {
    if scalar <= 0.0 { -96.0 } else { 20.0 * scalar.log10() }
}

#[allow(dead_code)]
fn db_to_scalar(db: f32) -> f32 {
    if db <= -96.0 { 0.0 } else { 10.0f32.powf(db / 20.0) }
}

#[cfg(target_os = "macos")]
fn write_u32(out: *mut u8, available_size: u32, value: u32) -> Option<u32> {
    if available_size >= 4 {
        unsafe { *(out as *mut u32) = value; }
        Some(4)
    } else {
        None
    }
}

#[cfg(target_os = "macos")]
fn write_u32_array(out: *mut u8, available_size: u32, values: &[u32]) -> Option<u32> {
    let size = (values.len() * 4) as u32;
    if available_size >= size {
        unsafe {
            std::ptr::copy_nonoverlapping(
                values.as_ptr() as *const u8, out, size as usize,
            );
        }
        Some(size)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scalar_to_db_conversion() {
        assert!((scalar_to_db(1.0) - 0.0).abs() < 0.01);
        assert!((scalar_to_db(0.5) - (-6.02)).abs() < 0.1);
        assert_eq!(scalar_to_db(0.0), -96.0);
    }

    #[test]
    fn db_to_scalar_conversion() {
        assert!((db_to_scalar(0.0) - 1.0).abs() < 0.01);
        assert!((db_to_scalar(-6.02) - 0.5).abs() < 0.01);
        assert_eq!(db_to_scalar(-96.0), 0.0);
    }

    #[test]
    fn asbd_stereo_float32() {
        let asbd = AudioStreamBasicDescription::stereo_float32(48000.0);
        assert_eq!(asbd.sample_rate, 48000.0);
        assert_eq!(asbd.format_id, AUDIO_FORMAT_LINEAR_PCM);
        assert_eq!(asbd.channels_per_frame, 2);
        assert_eq!(asbd.bits_per_channel, 32);
        assert_eq!(asbd.bytes_per_frame, 8); // 2 channels * 4 bytes
        assert_eq!(asbd.frames_per_packet, 1);
    }
}
