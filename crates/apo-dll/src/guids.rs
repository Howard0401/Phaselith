use windows::core::GUID;

/// CLSID for Phaselith Audio Processing Object
pub const CLSID_PHASELITH_APO: GUID =
    GUID::from_u128(0xA1B2C3D4_E5F6_4A5B_9C8D_1E2F3A4B5C6D);

/// Friendly name shown in Windows audio settings
pub const APO_FRIENDLY_NAME: &str = "Phaselith Audio Enhancement";
