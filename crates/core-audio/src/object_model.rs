// CoreAudio audio object hierarchy.
//
// A HAL plugin must present a tree of typed objects:
//   Plugin (ID=1)
//     └── Device (ID=100)
//           ├── Stream Input (ID=200)
//           ├── Stream Output (ID=201)
//           └── Volume Control (ID=300)
//
// Each object has properties queried via Has/Get/Set property calls.

use crate::constants::*;

/// Object types in the hierarchy.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ObjectType {
    Plugin,
    Device,
    StreamInput,
    StreamOutput,
    VolumeControl,
}

/// Manages the audio object hierarchy.
pub struct ObjectStore {
    pub volume_scalar: f32, // 0.0 - 1.0
}

impl ObjectStore {
    pub fn new() -> Self {
        Self {
            volume_scalar: 1.0,
        }
    }

    /// Look up object type by AudioObjectID.
    pub fn object_type(&self, object_id: u32) -> Option<ObjectType> {
        match object_id {
            PLUGIN_OBJECT_ID => Some(ObjectType::Plugin),
            DEVICE_OBJECT_ID => Some(ObjectType::Device),
            STREAM_INPUT_OBJECT_ID => Some(ObjectType::StreamInput),
            STREAM_OUTPUT_OBJECT_ID => Some(ObjectType::StreamOutput),
            VOLUME_CONTROL_OBJECT_ID => Some(ObjectType::VolumeControl),
            _ => None,
        }
    }

    /// Get the owner (parent) object ID for a given object.
    pub fn owner_of(&self, object_id: u32) -> u32 {
        match object_id {
            DEVICE_OBJECT_ID => PLUGIN_OBJECT_ID,
            STREAM_INPUT_OBJECT_ID | STREAM_OUTPUT_OBJECT_ID => DEVICE_OBJECT_ID,
            VOLUME_CONTROL_OBJECT_ID => DEVICE_OBJECT_ID,
            _ => PLUGIN_OBJECT_ID,
        }
    }

    /// Get child object IDs owned by a given object.
    pub fn children_of(&self, object_id: u32) -> &[u32] {
        match object_id {
            PLUGIN_OBJECT_ID => &[DEVICE_OBJECT_ID],
            DEVICE_OBJECT_ID => &[
                STREAM_INPUT_OBJECT_ID,
                STREAM_OUTPUT_OBJECT_ID,
                VOLUME_CONTROL_OBJECT_ID,
            ],
            _ => &[],
        }
    }

    /// Get stream IDs for a device.
    pub fn streams_of(&self, _device_id: u32) -> &[u32] {
        &[STREAM_INPUT_OBJECT_ID, STREAM_OUTPUT_OBJECT_ID]
    }

    /// Get control IDs for a device.
    pub fn controls_of(&self, _device_id: u32) -> &[u32] {
        &[VOLUME_CONTROL_OBJECT_ID]
    }

    /// Get the class ID for an object type.
    pub fn class_of(&self, object_type: ObjectType) -> u32 {
        match object_type {
            ObjectType::Plugin => CLASS_PLUGIN,
            ObjectType::Device => CLASS_DEVICE,
            ObjectType::StreamInput | ObjectType::StreamOutput => CLASS_STREAM,
            ObjectType::VolumeControl => CLASS_VOLUME_CONTROL,
        }
    }

    /// Get the base class ID for an object type.
    pub fn base_class_of(&self, object_type: ObjectType) -> u32 {
        match object_type {
            ObjectType::Plugin => CLASS_OBJECT,
            ObjectType::Device => CLASS_OBJECT,
            ObjectType::StreamInput | ObjectType::StreamOutput => CLASS_OBJECT,
            ObjectType::VolumeControl => CLASS_LEVEL_CONTROL,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn object_hierarchy() {
        let store = ObjectStore::new();

        // Plugin is root
        assert_eq!(store.object_type(PLUGIN_OBJECT_ID), Some(ObjectType::Plugin));
        assert_eq!(store.children_of(PLUGIN_OBJECT_ID), &[DEVICE_OBJECT_ID]);

        // Device owns streams and controls
        assert_eq!(store.object_type(DEVICE_OBJECT_ID), Some(ObjectType::Device));
        assert_eq!(store.owner_of(DEVICE_OBJECT_ID), PLUGIN_OBJECT_ID);
        assert_eq!(store.children_of(DEVICE_OBJECT_ID).len(), 3);

        // Streams owned by device
        assert_eq!(store.owner_of(STREAM_INPUT_OBJECT_ID), DEVICE_OBJECT_ID);
        assert_eq!(store.owner_of(STREAM_OUTPUT_OBJECT_ID), DEVICE_OBJECT_ID);

        // Volume control owned by device
        assert_eq!(store.owner_of(VOLUME_CONTROL_OBJECT_ID), DEVICE_OBJECT_ID);
    }

    #[test]
    fn unknown_object_returns_none() {
        let store = ObjectStore::new();
        assert_eq!(store.object_type(999), None);
    }

    #[test]
    fn class_ids() {
        let store = ObjectStore::new();
        assert_eq!(store.class_of(ObjectType::Plugin), CLASS_PLUGIN);
        assert_eq!(store.class_of(ObjectType::Device), CLASS_DEVICE);
        assert_eq!(store.class_of(ObjectType::StreamInput), CLASS_STREAM);
        assert_eq!(store.class_of(ObjectType::VolumeControl), CLASS_VOLUME_CONTROL);
    }
}
