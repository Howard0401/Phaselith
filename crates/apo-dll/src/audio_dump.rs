//! Audio dump module for A/B comparison analysis.
//!
//! Records input (pre-DSP) and output (post-DSP) audio to WAV files.
//! Triggered by the existence of `C:\ProgramData\Phaselith\dump_trigger`.
//!
//! RT-safe: only does an atomic check + memcpy on the audio thread.
//! WAV writing happens on a background thread.

use std::sync::atomic::{AtomicU8, AtomicU32, Ordering};
use std::sync::Mutex;

const DUMP_DIR: &str = r"C:\ProgramData\Phaselith";
const TRIGGER_FILE: &str = r"C:\ProgramData\Phaselith\dump_trigger";
const BEFORE_WAV: &str = r"C:\ProgramData\Phaselith\dump_before.wav";
const AFTER_WAV: &str = r"C:\ProgramData\Phaselith\dump_after.wav";

/// 30 seconds @ 48kHz stereo = 2,880,000 samples
const MAX_SAMPLES: usize = 48000 * 2 * 30;

/// Check trigger file every N frames to avoid filesystem hits on every call
const CHECK_INTERVAL: u32 = 480; // ~every 10ms * 480 = ~every 4.8 seconds at 480 frames/call

// States
const STATE_IDLE: u8 = 0;
const STATE_RECORDING: u8 = 1;
const STATE_WRITING: u8 = 2;

static STATE: AtomicU8 = AtomicU8::new(STATE_IDLE);
static FRAME_COUNTER: AtomicU32 = AtomicU32::new(0);
static SAMPLE_RATE: AtomicU32 = AtomicU32::new(48000);
static CHANNELS: AtomicU32 = AtomicU32::new(2);

struct DumpBuffers {
    before: Vec<f32>,
    after: Vec<f32>,
    pos: usize,
}

static BUFFERS: Mutex<Option<DumpBuffers>> = Mutex::new(None);

/// Called once from APO init to set sample rate and channels.
pub fn init(sample_rate: u32, channels: u32) {
    SAMPLE_RATE.store(sample_rate, Ordering::Relaxed);
    CHANNELS.store(channels, Ordering::Relaxed);
}

/// Called from APOProcess after process(). RT-safe path.
///
/// `input` = pre-DSP samples (interleaved), `output` = post-DSP samples (interleaved).
pub fn record(input: &[f32], output: &[f32]) {
    let state = STATE.load(Ordering::Relaxed);

    match state {
        STATE_IDLE => {
            // Periodically check for trigger file
            let count = FRAME_COUNTER.fetch_add(1, Ordering::Relaxed);
            if count % CHECK_INTERVAL == 0 {
                // try_lock: if contended, skip this check
                if let Ok(mut buf) = BUFFERS.try_lock() {
                    if std::path::Path::new(TRIGGER_FILE).exists() {
                        // Allocate buffers and start recording
                        *buf = Some(DumpBuffers {
                            before: Vec::with_capacity(MAX_SAMPLES),
                            after: Vec::with_capacity(MAX_SAMPLES),
                            pos: 0,
                        });
                        STATE.store(STATE_RECORDING, Ordering::Relaxed);
                        // Delete trigger file
                        let _ = std::fs::remove_file(TRIGGER_FILE);
                        apo_log!("AUDIO_DUMP: recording started ({} Hz, {} ch)",
                            SAMPLE_RATE.load(Ordering::Relaxed),
                            CHANNELS.load(Ordering::Relaxed));
                    }
                }
            }
        }
        STATE_RECORDING => {
            if let Ok(mut buf) = BUFFERS.try_lock() {
                if let Some(ref mut b) = *buf {
                    let remaining = MAX_SAMPLES - b.pos;
                    let copy_len = input.len().min(output.len()).min(remaining);

                    if copy_len > 0 {
                        b.before.extend_from_slice(&input[..copy_len]);
                        b.after.extend_from_slice(&output[..copy_len]);
                        b.pos += copy_len;
                    }

                    if b.pos >= MAX_SAMPLES {
                        STATE.store(STATE_WRITING, Ordering::Relaxed);
                        apo_log!("AUDIO_DUMP: recording complete, {} samples captured", b.pos);

                        // Move buffers out for background write
                        let before_data = std::mem::take(&mut b.before);
                        let after_data = std::mem::take(&mut b.after);
                        let sr = SAMPLE_RATE.load(Ordering::Relaxed);
                        let ch = CHANNELS.load(Ordering::Relaxed) as u16;

                        // Spawn background thread for WAV writing (NOT on RT thread)
                        std::thread::spawn(move || {
                            write_wav(BEFORE_WAV, &before_data, sr, ch);
                            write_wav(AFTER_WAV, &after_data, sr, ch);
                            apo_log!("AUDIO_DUMP: WAV files written");
                            STATE.store(STATE_IDLE, Ordering::Relaxed);
                        });

                        *buf = None;
                    }
                }
            }
            // If try_lock fails, we just skip this frame's samples — acceptable loss
        }
        STATE_WRITING => {
            // Background thread is writing WAVs, do nothing
        }
        _ => {}
    }
}

/// Write f32 interleaved samples to a WAV file.
fn write_wav(path: &str, samples: &[f32], sample_rate: u32, channels: u16) {
    use std::io::Write;

    let num_samples = samples.len() as u32;
    let byte_rate = sample_rate * channels as u32 * 4; // 32-bit float
    let block_align = channels * 4;
    let data_size = num_samples * 4;

    let result = (|| -> std::io::Result<()> {
        let mut f = std::fs::File::create(path)?;

        // RIFF header
        f.write_all(b"RIFF")?;
        f.write_all(&(36 + data_size).to_le_bytes())?;
        f.write_all(b"WAVE")?;

        // fmt chunk — IEEE float
        f.write_all(b"fmt ")?;
        f.write_all(&16u32.to_le_bytes())?; // chunk size
        f.write_all(&3u16.to_le_bytes())?;  // format: IEEE float
        f.write_all(&channels.to_le_bytes())?;
        f.write_all(&sample_rate.to_le_bytes())?;
        f.write_all(&byte_rate.to_le_bytes())?;
        f.write_all(&block_align.to_le_bytes())?;
        f.write_all(&32u16.to_le_bytes())?; // bits per sample

        // data chunk
        f.write_all(b"data")?;
        f.write_all(&data_size.to_le_bytes())?;

        // Write samples as raw f32 bytes
        let byte_slice = unsafe {
            std::slice::from_raw_parts(
                samples.as_ptr() as *const u8,
                samples.len() * 4,
            )
        };
        f.write_all(byte_slice)?;

        Ok(())
    })();

    match result {
        Ok(()) => apo_log!("AUDIO_DUMP: wrote {} ({} samples)", path, num_samples),
        Err(e) => apo_log!("AUDIO_DUMP: failed to write {}: {}", path, e),
    }
}
