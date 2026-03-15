// COM interface implementation for Phaselith APO.
//
// Wraps PhaselithApo (pure Rust DSP logic) with proper COM vtables so that
// audiodg.exe can load and call us through the standard APO protocol.
//
// Implements:
// - IAudioProcessingObject: init, format negotiation, registration properties
// - IAudioProcessingObjectRT: APOProcess (real-time audio, zero-alloc hot path)
// - IAudioProcessingObjectConfiguration: lock/unlock for processing
// - IAudioSystemEffects2: effects list reporting (required by Windows 10+)

use crate::apo_impl::PhaselithApo;
use crate::format_negotiate;
use crate::guids::*;

use std::cell::{Cell, RefCell};

use windows::core::*;
use windows::Win32::Foundation::*;
use windows::Win32::Media::Audio::Apo::*;
use windows::Win32::System::Com::{CoTaskMemAlloc, CoTaskMemFree};

// KSDATAFORMAT_SUBTYPE_IEEE_FLOAT
const KSDATAFORMAT_SUBTYPE_IEEE_FLOAT: GUID =
    GUID::from_u128(0x00000003_0000_0010_8000_00aa00389b71);

/// COM wrapper around PhaselithApo.
///
/// Provides COM vtable for the four required APO interfaces.
/// Uses RefCell for interior mutability (COM methods take &self).
/// audiodg.exe is single-threaded per stream, so no contention.
#[implement(
    IAudioProcessingObject,
    IAudioProcessingObjectRT,
    IAudioProcessingObjectConfiguration,
    IAudioSystemEffects,
    IAudioSystemEffects2,
    IAudioSystemEffects3,
)]
pub struct PhaselithApoCom {
    inner: RefCell<PhaselithApo>,
    sample_rate: Cell<u32>,
    channels: Cell<u16>,
    frame_size: Cell<usize>,
    initialized: Cell<bool>,
    locked: Cell<bool>,
    process_call_count: Cell<u64>,
}

impl PhaselithApoCom {
    pub fn new() -> Self {
        apo_log!("PhaselithApoCom::new() creating instance");
        Self {
            inner: RefCell::new(PhaselithApo::new()),
            sample_rate: Cell::new(48000),
            channels: Cell::new(2),
            frame_size: Cell::new(480),
            initialized: Cell::new(false),
            locked: Cell::new(false),
            process_call_count: Cell::new(0),
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
impl IAudioProcessingObject_Impl for PhaselithApoCom_Impl {
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
            props.clsid = CLSID_PHASELITH_APO;
            props.Flags = APO_FLAG(0x0E); // APO_FLAG_DEFAULT = SFX + MFX
            // Copy friendly name
            let name_wide: Vec<u16> = APO_FRIENDLY_NAME
                .encode_utf16()
                .chain(std::iter::once(0))
                .collect();
            let copy_len = name_wide.len().min(256);
            props.szFriendlyName[..copy_len].copy_from_slice(&name_wide[..copy_len]);
            // Copyright
            let copyright = "Phaselith Project";
            let cr_wide: Vec<u16> = copyright
                .encode_utf16()
                .chain(std::iter::once(0))
                .collect();
            let cr_len = cr_wide.len().min(256);
            props.szCopyrightInfo[..cr_len].copy_from_slice(&cr_wide[..cr_len]);

            props.u32MajorVersion = 1;
            props.u32MinorVersion = 0;
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
        apo_log!("IAudioProcessingObject::Initialize(sr={}, ch={})", self.sample_rate.get(), self.channels.get());
        let sr = self.sample_rate.get();
        let ch = self.channels.get();
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            self.inner.borrow_mut().initialize(sr, ch);
        }));
        if result.is_err() {
            apo_log!("Initialize: caught panic in inner.initialize!");
        }
        self.initialized.set(true);
        apo_log!("Initialize: done OK");
        Ok(())
    }

    fn IsInputFormatSupported(
        &self,
        _poppositeformat: Option<&IAudioMediaType>,
        prequestedinputformat: Option<&IAudioMediaType>,
    ) -> Result<IAudioMediaType> {
        let requested = prequestedinputformat.ok_or_else(|| Error::from(E_POINTER))?;
        let (sample_rate, channels, bits, is_float) = unsafe { extract_format(requested)? };
        let supported = format_negotiate::is_format_supported(sample_rate, bits, channels, is_float);
        apo_log!("IsInputFormatSupported: sr={} ch={} bits={} float={} -> {}", sample_rate, channels, bits, is_float, supported);

        if supported {
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
impl IAudioProcessingObjectConfiguration_Impl for PhaselithApoCom_Impl {
    fn LockForProcess(
        &self,
        _u32numinputconnections: u32,
        ppinputconnections: *const *const APO_CONNECTION_DESCRIPTOR,
        _u32numoutputconnections: u32,
        _ppoutputconnections: *const *const APO_CONNECTION_DESCRIPTOR,
    ) -> Result<()> {
        apo_log!("LockForProcess called");
        if self.locked.get() {
            apo_log!("LockForProcess: already locked!");
            return Err(APOERR_ALREADY_INITIALIZED.into());
        }

        // Read format from input connection descriptor (with safety guard)
        let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            if _u32numinputconnections > 0 && !ppinputconnections.is_null() {
                let desc_ptr = unsafe { *ppinputconnections };
                if !desc_ptr.is_null() {
                    let desc = unsafe { &*desc_ptr };
                    let frame_count = desc.u32MaxFrameCount as usize;
                    apo_log!("LockForProcess: descriptor frame_count={}", frame_count);

                    // Try to read the format from the media type
                    if let Some(ref media_type) = *desc.pFormat {
                        let mut fmt = UNCOMPRESSEDAUDIOFORMAT::default();
                        if unsafe { media_type.GetUncompressedAudioFormat(&mut fmt) }.is_ok() {
                            let sr = fmt.fFramesPerSecond as u32;
                            let ch = fmt.dwSamplesPerFrame as u16;
                            apo_log!("LockForProcess: format sr={} ch={}", sr, ch);
                            self.sample_rate.set(sr);
                            self.channels.set(ch);
                        }
                    }

                    self.frame_size.set(frame_count);
                }
            }
        }));

        let sr = self.sample_rate.get();
        let ch = self.channels.get();
        let fs = self.frame_size.get();
        apo_log!("LockForProcess: sr={} ch={} fs={}", sr, ch, fs);

        apo_log!("LockForProcess: about to init engine...");
        let init_ok = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            apo_log!("LockForProcess: inside catch_unwind, calling initialize...");
            let mut inner = self.inner.borrow_mut();
            inner.initialize(sr, ch);
            apo_log!("LockForProcess: initialize done, calling lock_for_process...");
            inner.lock_for_process(fs);
            apo_log!("LockForProcess: lock_for_process done");
        }));

        if init_ok.is_err() {
            apo_log!("LockForProcess: PANIC caught! entering bypass mode");
            self.inner.borrow_mut().set_bypass_mode();
        } else {
            apo_log!("LockForProcess: engine init succeeded");
        }

        self.locked.set(true);
        apo_log!("LockForProcess: done OK");
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
impl IAudioProcessingObjectRT_Impl for PhaselithApoCom_Impl {
    fn APOProcess(
        &self,
        _u32numinputconnections: u32,
        ppinputconnections: *const *const APO_CONNECTION_PROPERTY,
        _u32numoutputconnections: u32,
        ppoutputconnections: *mut *mut APO_CONNECTION_PROPERTY,
    ) {
        let count = self.process_call_count.get();
        self.process_call_count.set(count + 1);
        if count == 0 {
            apo_log!("APOProcess: first call! ch={} locked={}", self.channels.get(), self.locked.get());
        }
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
            if ch == 0 { return; }
            let sample_count = frames * ch;

            // Validate buffer pointers before creating slices
            let in_ptr = input_prop.pBuffer as *const f32;
            let out_ptr = output_prop.pBuffer as *mut f32;
            if in_ptr.is_null() || out_ptr.is_null() || sample_count == 0 {
                return;
            }

            let input = unsafe {
                std::slice::from_raw_parts(in_ptr, sample_count)
            };
            let output = unsafe {
                std::slice::from_raw_parts_mut(out_ptr, sample_count)
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
impl IAudioSystemEffects_Impl for PhaselithApoCom_Impl {}

// ---------------------------------------------------------------------------
// IAudioSystemEffects2 — reports effect list to Windows (required Win10+)
// ---------------------------------------------------------------------------

// AUDIO_EFFECT_TYPE GUIDs — we report as a "generic" stream effect
// AUDIO_EFFECT_TYPE_ACOUSTIC_ECHO_CANCELLATION etc. are defined in audiomediatype.h
// For a custom effect, we use our own APO CLSID as the effect type.
impl IAudioSystemEffects2_Impl for PhaselithApoCom_Impl {
    fn GetEffectsList(
        &self,
        ppeffectsids: *mut *mut GUID,
        pceffects: *mut u32,
        _event: HANDLE,
    ) -> Result<()> {
        apo_log!("IAudioSystemEffects2::GetEffectsList called");

        if ppeffectsids.is_null() || pceffects.is_null() {
            return Err(E_POINTER.into());
        }

        unsafe {
            let size = std::mem::size_of::<GUID>();
            let ptr = CoTaskMemAlloc(size) as *mut GUID;
            if ptr.is_null() {
                return Err(E_OUTOFMEMORY.into());
            }
            *ptr = CLSID_PHASELITH_APO;
            *ppeffectsids = ptr;
            *pceffects = 1;
        }

        apo_log!("GetEffectsList: reported 1 effect");
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// IAudioSystemEffects3 — required by Windows 11 (build 22000+)
// ---------------------------------------------------------------------------
impl IAudioSystemEffects3_Impl for PhaselithApoCom_Impl {
    fn GetControllableSystemEffectsList(
        &self,
        effects: *mut *mut AUDIO_SYSTEMEFFECT,
        numeffects: *mut u32,
        _event: HANDLE,
    ) -> Result<()> {
        apo_log!("IAudioSystemEffects3::GetControllableSystemEffectsList called");

        if effects.is_null() || numeffects.is_null() {
            return Err(E_POINTER.into());
        }

        unsafe {
            // Report one effect
            let size = std::mem::size_of::<AUDIO_SYSTEMEFFECT>();
            let ptr = CoTaskMemAlloc(size) as *mut AUDIO_SYSTEMEFFECT;
            if ptr.is_null() {
                return Err(E_OUTOFMEMORY.into());
            }
            (*ptr).id = CLSID_PHASELITH_APO;
            (*ptr).canSetState = TRUE;
            (*ptr).state = AUDIO_SYSTEMEFFECT_STATE_ON;
            *effects = ptr;
            *numeffects = 1;
        }

        apo_log!("GetControllableSystemEffectsList: reported 1 effect (ON)");
        Ok(())
    }

    fn SetAudioSystemEffectState(
        &self,
        effectid: &GUID,
        state: AUDIO_SYSTEMEFFECT_STATE,
    ) -> Result<()> {
        apo_log!("SetAudioSystemEffectState: id={:?} state={:?}", effectid, state);
        // We only have one effect, always accept state changes
        Ok(())
    }
}
