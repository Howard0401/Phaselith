// COM interface implementation for ASCE APO.
//
// Wraps AsceApo (pure Rust DSP logic) with proper COM vtables so that
// audiodg.exe can load and call us through the standard APO protocol.
//
// Implements:
// - IAudioProcessingObject: init, format negotiation, registration properties
// - IAudioProcessingObjectRT: APOProcess (real-time audio, zero-alloc hot path)
// - IAudioProcessingObjectConfiguration: lock/unlock for processing
// - IAudioSystemEffects: marker interface (no methods)

use crate::apo_impl::AsceApo;
use crate::format_negotiate;
use crate::guids::*;

use std::cell::{Cell, RefCell};

use windows::core::*;
use windows::Win32::Foundation::*;
use windows::Win32::Media::Audio::Apo::*;
use windows::Win32::System::Com::CoTaskMemAlloc;

// KSDATAFORMAT_SUBTYPE_IEEE_FLOAT
const KSDATAFORMAT_SUBTYPE_IEEE_FLOAT: GUID =
    GUID::from_u128(0x00000003_0000_0010_8000_00aa00389b71);

/// COM wrapper around AsceApo.
///
/// Provides COM vtable for the four required APO interfaces.
/// Uses RefCell for interior mutability (COM methods take &self).
/// audiodg.exe is single-threaded per stream, so no contention.
#[implement(
    IAudioProcessingObject,
    IAudioProcessingObjectRT,
    IAudioProcessingObjectConfiguration,
    IAudioSystemEffects,
)]
pub struct AsceApoCom {
    inner: RefCell<AsceApo>,
    sample_rate: Cell<u32>,
    channels: Cell<u16>,
    frame_size: Cell<usize>,
    initialized: Cell<bool>,
    locked: Cell<bool>,
}

impl AsceApoCom {
    pub fn new() -> Self {
        Self {
            inner: RefCell::new(AsceApo::new()),
            sample_rate: Cell::new(48000),
            channels: Cell::new(2),
            frame_size: Cell::new(480),
            initialized: Cell::new(false),
            locked: Cell::new(false),
        }
    }
}

/// Helper: extract format parameters from IAudioMediaType.
/// Returns (sample_rate, channels, bits_per_sample, is_float).
unsafe fn extract_format(media_type: &IAudioMediaType) -> Result<(u32, u16, u16, bool)> {
    let mut fmt = UNCOMPRESSEDAUDIOFORMAT::default();
    media_type.GetUncompressedAudioFormat(&mut fmt)?;
    let sample_rate = fmt.fFramesPerSecond as u32;
    let channels = fmt.dwSamplesPerFrame as u16;
    let bits = (fmt.dwBytesPerSampleContainer * 8) as u16;
    let is_float = fmt.guidFormatType == KSDATAFORMAT_SUBTYPE_IEEE_FLOAT;
    Ok((sample_rate, channels, bits, is_float))
}

// ---------------------------------------------------------------------------
// IAudioProcessingObject
// ---------------------------------------------------------------------------
impl IAudioProcessingObject_Impl for AsceApoCom_Impl {
    fn Reset(&self) -> Result<()> {
        if self.locked.get() {
            return Err(APOERR_ALREADY_INITIALIZED.into());
        }
        Ok(())
    }

    fn GetLatency(&self) -> Result<i64> {
        // SFX APO adds no latency (processes in-place, same frame count)
        Ok(0)
    }

    fn GetRegistrationProperties(&self) -> Result<*mut APO_REG_PROPERTIES> {
        unsafe {
            let size = std::mem::size_of::<APO_REG_PROPERTIES>();
            let ptr = CoTaskMemAlloc(size) as *mut APO_REG_PROPERTIES;
            if ptr.is_null() {
                return Err(E_OUTOFMEMORY.into());
            }

            // Zero-initialize
            std::ptr::write_bytes(ptr, 0, 1);

            let props = &mut *ptr;
            props.clsid = CLSID_ASCE_APO;
            props.Flags = APO_FLAG(0x0E); // APO_FLAG_DEFAULT = SFX + MFX
            // Copy friendly name
            let name_wide: Vec<u16> = APO_FRIENDLY_NAME
                .encode_utf16()
                .chain(std::iter::once(0))
                .collect();
            let copy_len = name_wide.len().min(256);
            props.szFriendlyName[..copy_len].copy_from_slice(&name_wide[..copy_len]);
            // Copyright
            let copyright = "ASCE Project";
            let cr_wide: Vec<u16> = copyright
                .encode_utf16()
                .chain(std::iter::once(0))
                .collect();
            let cr_len = cr_wide.len().min(256);
            props.szCopyrightInfo[..cr_len].copy_from_slice(&cr_wide[..cr_len]);

            props.u32MajorVersion = 0;
            props.u32MinorVersion = 1;
            props.u32MinInputConnections = 1;
            props.u32MaxInputConnections = 1;
            props.u32MinOutputConnections = 1;
            props.u32MaxOutputConnections = 1;
            props.u32MaxInstances = 0xFFFFFFFF;
            props.u32NumAPOInterfaces = 1;
            props.iidAPOInterfaceList[0] = IAudioProcessingObject::IID;

            Ok(ptr)
        }
    }

    fn Initialize(&self, _cbdatasize: u32, _pbydata: *const u8) -> Result<()> {
        // APO initialization is called by audiodg before any processing.
        // We defer engine creation to LockForProcess where we know the format.
        self.inner
            .borrow_mut()
            .initialize(self.sample_rate.get(), self.channels.get());
        self.initialized.set(true);
        Ok(())
    }

    fn IsInputFormatSupported(
        &self,
        _poppositeformat: Option<&IAudioMediaType>,
        prequestedinputformat: Option<&IAudioMediaType>,
    ) -> Result<IAudioMediaType> {
        let requested = prequestedinputformat.ok_or_else(|| Error::from(E_POINTER))?;
        let (sample_rate, channels, bits, is_float) = unsafe { extract_format(requested)? };

        if format_negotiate::is_format_supported(sample_rate, bits, channels, is_float) {
            // Format is supported — return the same media type
            Ok(requested.clone())
        } else {
            Err(APOERR_FORMAT_NOT_SUPPORTED.into())
        }
    }

    fn IsOutputFormatSupported(
        &self,
        _poppositeformat: Option<&IAudioMediaType>,
        prequestedoutputformat: Option<&IAudioMediaType>,
    ) -> Result<IAudioMediaType> {
        let requested = prequestedoutputformat.ok_or_else(|| Error::from(E_POINTER))?;
        let (sample_rate, channels, bits, is_float) = unsafe { extract_format(requested)? };

        if format_negotiate::is_format_supported(sample_rate, bits, channels, is_float) {
            Ok(requested.clone())
        } else {
            Err(APOERR_FORMAT_NOT_SUPPORTED.into())
        }
    }

    fn GetInputChannelCount(&self) -> Result<u32> {
        Ok(self.channels.get() as u32)
    }
}

// ---------------------------------------------------------------------------
// IAudioProcessingObjectConfiguration
// ---------------------------------------------------------------------------
impl IAudioProcessingObjectConfiguration_Impl for AsceApoCom_Impl {
    fn LockForProcess(
        &self,
        _u32numinputconnections: u32,
        ppinputconnections: *const *const APO_CONNECTION_DESCRIPTOR,
        _u32numoutputconnections: u32,
        _ppoutputconnections: *const *const APO_CONNECTION_DESCRIPTOR,
    ) -> Result<()> {
        if self.locked.get() {
            return Err(APOERR_ALREADY_INITIALIZED.into());
        }

        // Read format from input connection descriptor
        if !ppinputconnections.is_null() {
            let desc = unsafe { &**ppinputconnections };
            let frame_count = desc.u32MaxFrameCount as usize;

            // Try to read the format from the media type
            if let Some(ref media_type) = *desc.pFormat {
                let mut fmt = UNCOMPRESSEDAUDIOFORMAT::default();
                if unsafe { media_type.GetUncompressedAudioFormat(&mut fmt) }.is_ok() {
                    self.sample_rate.set(fmt.fFramesPerSecond as u32);
                    self.channels.set(fmt.dwSamplesPerFrame as u16);
                }
            }

            self.frame_size.set(frame_count);
        }

        // Initialize engine with discovered format, then lock
        self.inner
            .borrow_mut()
            .initialize(self.sample_rate.get(), self.channels.get());
        self.inner
            .borrow_mut()
            .lock_for_process(self.frame_size.get());
        self.locked.set(true);

        Ok(())
    }

    fn UnlockForProcess(&self) -> Result<()> {
        self.inner.borrow_mut().unlock_for_process();
        self.locked.set(false);
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// IAudioProcessingObjectRT (real-time thread — zero alloc!)
// ---------------------------------------------------------------------------
impl IAudioProcessingObjectRT_Impl for AsceApoCom_Impl {
    fn APOProcess(
        &self,
        _u32numinputconnections: u32,
        ppinputconnections: *const *const APO_CONNECTION_PROPERTY,
        _u32numoutputconnections: u32,
        ppoutputconnections: *mut *mut APO_CONNECTION_PROPERTY,
    ) {
        // catch_unwind: a panic here would kill audiodg.exe
        let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            if ppinputconnections.is_null() || ppoutputconnections.is_null() {
                return;
            }

            let input_prop = unsafe { &**ppinputconnections };
            let output_prop = unsafe { &mut **ppoutputconnections };

            // Silent buffer → propagate silence, skip processing
            if input_prop.u32BufferFlags == APO_BUFFER_FLAGS(0) {
                // BUFFER_INVALID
                output_prop.u32BufferFlags = APO_BUFFER_FLAGS(0);
                output_prop.u32ValidFrameCount = 0;
                return;
            }

            let frames = input_prop.u32ValidFrameCount as usize;
            if frames == 0 {
                output_prop.u32BufferFlags = APO_BUFFER_FLAGS(2); // BUFFER_SILENT
                output_prop.u32ValidFrameCount = 0;
                return;
            }

            let ch = self.channels.get() as usize;
            let sample_count = frames * ch;

            let input = unsafe {
                std::slice::from_raw_parts(input_prop.pBuffer as *const f32, sample_count)
            };
            let output = unsafe {
                std::slice::from_raw_parts_mut(output_prop.pBuffer as *mut f32, sample_count)
            };

            self.inner.borrow_mut().process(input, output);

            output_prop.u32ValidFrameCount = frames as u32;
            output_prop.u32BufferFlags = APO_BUFFER_FLAGS(1); // BUFFER_VALID
        }));
    }

    fn CalcInputFrames(&self, u32outputframecount: u32) -> u32 {
        // 1:1 — SFX APO doesn't change frame count
        u32outputframecount
    }

    fn CalcOutputFrames(&self, u32inputframecount: u32) -> u32 {
        // 1:1 — SFX APO doesn't change frame count
        u32inputframecount
    }
}

// ---------------------------------------------------------------------------
// IAudioSystemEffects — marker interface, no methods to implement
// ---------------------------------------------------------------------------
impl IAudioSystemEffects_Impl for AsceApoCom_Impl {}
