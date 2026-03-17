// CoreAudio HAL plugin constants.
// UUIDs, device identifiers, and property selectors.
// All cross-platform — pure Rust constants with no OS dependencies.

/// Plugin factory UUID (declared in Info.plist under CFPlugInFactories).
/// coreaudiod uses this to locate the factory function.
pub const FACTORY_UUID: [u8; 16] = [
    0xA1, 0xB2, 0xC3, 0xD4, 0xE5, 0xF6, 0x01, 0x23,
    0x45, 0x67, 0x89, 0xAB, 0xCD, 0xEF, 0x00, 0x01,
];

/// AudioServerPlugIn type UUID (Apple-defined, same for all HAL plugins).
/// 443ABAB8-E7B3-491A-B985-BEB9187030DB
pub const AUDIO_SERVER_PLUGIN_TYPE_UUID: [u8; 16] = [
    0x44, 0x3A, 0xBA, 0xB8, 0xE7, 0xB3, 0x49, 0x1A,
    0xB9, 0x85, 0xBE, 0xB9, 0x18, 0x70, 0x30, 0xDB,
];

/// Device UUID — uniquely identifies this virtual audio device.
pub const DEVICE_UUID: [u8; 16] = [
    0xA1, 0xB2, 0xC3, 0xD4, 0xE5, 0xF6, 0x01, 0x23,
    0x45, 0x67, 0x89, 0xAB, 0xCD, 0xEF, 0x00, 0x10,
];

/// Input stream UUID.
pub const STREAM_INPUT_UUID: [u8; 16] = [
    0xA1, 0xB2, 0xC3, 0xD4, 0xE5, 0xF6, 0x01, 0x23,
    0x45, 0x67, 0x89, 0xAB, 0xCD, 0xEF, 0x00, 0x20,
];

/// Output stream UUID.
pub const STREAM_OUTPUT_UUID: [u8; 16] = [
    0xA1, 0xB2, 0xC3, 0xD4, 0xE5, 0xF6, 0x01, 0x23,
    0x45, 0x67, 0x89, 0xAB, 0xCD, 0xEF, 0x00, 0x21,
];

/// Volume control UUID.
pub const VOLUME_CONTROL_UUID: [u8; 16] = [
    0xA1, 0xB2, 0xC3, 0xD4, 0xE5, 0xF6, 0x01, 0x23,
    0x45, 0x67, 0x89, 0xAB, 0xCD, 0xEF, 0x00, 0x30,
];

// ─── Display identifiers ───

pub const MANUFACTURER: &str = "Phaselith";
pub const DEVICE_NAME: &str = "Phaselith Audio Enhancement";
pub const DEVICE_UID: &str = "com.phaselith.virtual-device";
pub const DEVICE_MODEL_UID: &str = "com.phaselith.virtual-device.model";
pub const BUNDLE_ID: &str = "com.phaselith.coreaudio-driver";

// ─── Audio object IDs ───
// kAudioObjectPlugInObject is always 1 (Apple-defined).
// We assign fixed IDs for our objects.

pub const PLUGIN_OBJECT_ID: u32 = 1;
pub const DEVICE_OBJECT_ID: u32 = 100;
pub const STREAM_INPUT_OBJECT_ID: u32 = 200;
pub const STREAM_OUTPUT_OBJECT_ID: u32 = 201;
pub const VOLUME_CONTROL_OBJECT_ID: u32 = 300;

// ─── CoreAudio property selectors ───
// Re-declared as u32 so they compile without macOS headers.
// Values from AudioHardwareBase.h / AudioServerPlugIn.h.
// FourCC encoding: each char is one byte, MSB first.

/// 'lpcm' — Linear PCM format ID
pub const AUDIO_FORMAT_LINEAR_PCM: u32 = 0x6C70636D;

// AudioObject property selectors
pub const PROPERTY_BASE_CLASS: u32 = 0x62636C73;             // 'bcls'
pub const PROPERTY_CLASS: u32 = 0x636C6173;                  // 'clas'
pub const PROPERTY_OWNER: u32 = 0x73746476;                  // 'stdv'
pub const PROPERTY_NAME: u32 = 0x6C6E616D;                   // 'lnam'
pub const PROPERTY_MANUFACTURER: u32 = 0x6C6D616B;           // 'lmak'
pub const PROPERTY_OWNED_OBJECTS: u32 = 0x6F776E64;           // 'ownd'
pub const PROPERTY_IDENTIFY: u32 = 0x69646E74;               // 'idnt'
pub const PROPERTY_SERIAL_NUMBER: u32 = 0x73726C6E;           // 'srln'
pub const PROPERTY_FIRMWARE_VERSION: u32 = 0x66776D76;        // 'fwmv'

// Plugin property selectors
pub const PLUGIN_PROPERTY_DEVICE_LIST: u32 = 0x64657623;     // 'dev#'
pub const PLUGIN_PROPERTY_TRANSLATE_UID: u32 = 0x75696474;   // 'uidt'
pub const PLUGIN_PROPERTY_RESOURCE_BUNDLE: u32 = 0x7372636E; // 'srcn'

// Device property selectors
pub const DEVICE_PROPERTY_UID: u32 = 0x75696420;             // 'uid '
pub const DEVICE_PROPERTY_MODEL_UID: u32 = 0x6D756964;       // 'muid'
pub const DEVICE_PROPERTY_TRANSPORT: u32 = 0x7472616E;       // 'tran'
pub const DEVICE_PROPERTY_RELATED: u32 = 0x616B696E;         // 'akin'
pub const DEVICE_PROPERTY_CLOCK_DOMAIN: u32 = 0x636C6B64;    // 'clkd'
pub const DEVICE_PROPERTY_ALIVE: u32 = 0x616C6976;           // 'aliv'
pub const DEVICE_PROPERTY_RUNNING: u32 = 0x676F696E;         // 'goin'
pub const DEVICE_PROPERTY_CAN_BE_DEFAULT: u32 = 0x64666C74;  // 'dflt'
pub const DEVICE_PROPERTY_CAN_BE_SYSTEM: u32 = 0x73666C74;   // 'sflt'
pub const DEVICE_PROPERTY_LATENCY: u32 = 0x6C746E63;         // 'ltnc'
pub const DEVICE_PROPERTY_STREAMS: u32 = 0x73746D23;         // 'stm#'
pub const DEVICE_PROPERTY_CONTROLS: u32 = 0x63746C23;        // 'ctl#'
pub const DEVICE_PROPERTY_SAFETY_OFFSET: u32 = 0x73616674;   // 'saft'
pub const DEVICE_PROPERTY_NOMINAL_SAMPLE_RATE: u32 = 0x6E737274; // 'nsrt'
pub const DEVICE_PROPERTY_AVAILABLE_SAMPLE_RATES: u32 = 0x6E737223; // 'nsr#'
pub const DEVICE_PROPERTY_ICON: u32 = 0x69636F6E;            // 'icon'
pub const DEVICE_PROPERTY_HIDDEN: u32 = 0x68696468;          // 'hidn'
pub const DEVICE_PROPERTY_PREFERRED_CHANNELS: u32 = 0x64636832; // 'dch2'
pub const DEVICE_PROPERTY_ZERO_TIME_STAMP_PERIOD: u32 = 0x72747023; // 'rtp#'

// Stream property selectors
pub const STREAM_PROPERTY_ACTIVE: u32 = 0x73616374;          // 'sact'
pub const STREAM_PROPERTY_DIRECTION: u32 = 0x73646972;       // 'sdir'
pub const STREAM_PROPERTY_TERMINAL_TYPE: u32 = 0x7465726D;   // 'term'
pub const STREAM_PROPERTY_START_CHANNEL: u32 = 0x73636E6C;   // 'scnl'
pub const STREAM_PROPERTY_LATENCY: u32 = 0x6C746E63;         // 'ltnc'
pub const STREAM_PROPERTY_VIRTUAL_FORMAT: u32 = 0x73666D74;  // 'sfmt'
pub const STREAM_PROPERTY_PHYSICAL_FORMAT: u32 = 0x70667420; // 'pft '
pub const STREAM_PROPERTY_AVAILABLE_VIRTUAL_FORMATS: u32 = 0x73666D61; // 'sfma'
pub const STREAM_PROPERTY_AVAILABLE_PHYSICAL_FORMATS: u32 = 0x70667461; // 'pfta'

// Level control property selectors
pub const LEVEL_CONTROL_PROPERTY_SCALAR_VALUE: u32 = 0x6C637376; // 'lcsv'
pub const LEVEL_CONTROL_PROPERTY_DECIBEL_VALUE: u32 = 0x6C636476; // 'lcdv'
pub const LEVEL_CONTROL_PROPERTY_DECIBEL_RANGE: u32 = 0x6C636472; // 'lcdr'
pub const LEVEL_CONTROL_PROPERTY_CONVERT_SCALAR_TO_DB: u32 = 0x6C637364; // 'lcsd'
pub const LEVEL_CONTROL_PROPERTY_CONVERT_DB_TO_SCALAR: u32 = 0x6C636473; // 'lcds'

// Property scope / element
pub const SCOPE_GLOBAL: u32 = 0x676C6F62;   // 'glob'
pub const SCOPE_INPUT: u32 = 0x696E7074;    // 'inpt'
pub const SCOPE_OUTPUT: u32 = 0x6F757470;   // 'outp'
pub const ELEMENT_MAIN: u32 = 0;

// Object class IDs
pub const CLASS_OBJECT: u32 = 0x616F626A;        // 'aobj'
pub const CLASS_PLUGIN: u32 = 0x61706C67;         // 'aplg'
pub const CLASS_DEVICE: u32 = 0x61646576;         // 'adev'
pub const CLASS_STREAM: u32 = 0x61737472;         // 'astr'
pub const CLASS_CONTROL: u32 = 0x6163746C;        // 'actl'
pub const CLASS_LEVEL_CONTROL: u32 = 0x6C63746C;  // 'lctl'
pub const CLASS_VOLUME_CONTROL: u32 = 0x766C6374; // 'vlct'

// Transport types
pub const TRANSPORT_TYPE_VIRTUAL: u32 = 0x7672746C; // 'vrtl'

// Terminal types
pub const INPUT_TERMINAL_MICROPHONE: u32 = 0x6D696372; // 'micr'
pub const OUTPUT_TERMINAL_SPEAKER: u32 = 0x73706B72;   // 'spkr'

// ─── Supported sample rates ───

pub const SUPPORTED_SAMPLE_RATES: &[f64] = &[44100.0, 48000.0, 96000.0, 192000.0];
pub const DEFAULT_SAMPLE_RATE: f64 = 48000.0;
pub const DEFAULT_CHANNELS: u32 = 2;
pub const DEFAULT_BITS_PER_CHANNEL: u32 = 32;

// ─── IPC paths (macOS) ───

#[cfg(target_os = "macos")]
pub const IPC_DIR: &str = "/tmp/phaselith";
#[cfg(target_os = "macos")]
pub const IPC_CONFIG_PATH: &str = "/tmp/phaselith/shared_config.bin";
#[cfg(target_os = "macos")]
pub const IPC_STATUS_PATH: &str = "/tmp/phaselith/shared_status.bin";

#[cfg(not(target_os = "macos"))]
pub const IPC_DIR: &str = "";
#[cfg(not(target_os = "macos"))]
pub const IPC_CONFIG_PATH: &str = "";
#[cfg(not(target_os = "macos"))]
pub const IPC_STATUS_PATH: &str = "";
