// APO Diagnostic Tool: read/write mmap, verify toggle works
//
// Usage:
//   apo_diag            — show current state
//   apo_diag enable     — set enabled=true, watch for frame count change
//   apo_diag disable    — set enabled=false, watch for frame count change
//   apo_diag watch      — poll status every 500ms for 10s

use std::fs::OpenOptions;
use std::io::{Read, Write, Seek, SeekFrom};
use std::time::{Duration, Instant};
use std::thread;

const CONFIG_PATH: &str = r"C:\ProgramData\Phaselith\shared_config.bin";
const STATUS_PATH: &str = r"C:\ProgramData\Phaselith\shared_status.bin";

fn read_config() -> Option<(u32, bool, f32, f32, f32, f32, u8, u8, u8)> {
    let data = std::fs::read(CONFIG_PATH).ok()?;
    if data.len() < 27 { return None; }
    let version = u32::from_le_bytes([data[0], data[1], data[2], data[3]]);
    let enabled = data[4] != 0;
    let strength = u32::from_le_bytes([data[8], data[9], data[10], data[11]]) as f32 / 10000.0;
    let hf = u32::from_le_bytes([data[12], data[13], data[14], data[15]]) as f32 / 10000.0;
    let dynamics = u32::from_le_bytes([data[16], data[17], data[18], data[19]]) as f32 / 10000.0;
    let transient = u32::from_le_bytes([data[20], data[21], data[22], data[23]]) as f32 / 10000.0;
    let phase = data[24];
    let quality = data[25];
    let synthesis = data[26];
    Some((version, enabled, strength, hf, dynamics, transient, phase, quality, synthesis))
}

fn read_status() -> Option<(u64, f32, f32, f32)> {
    let data = std::fs::read(STATUS_PATH).ok()?;
    if data.len() < 24 { return None; }
    let frames = u64::from_le_bytes([data[0], data[1], data[2], data[3], data[4], data[5], data[6], data[7]]);
    let cutoff = f32::from_bits(u32::from_le_bytes([data[8], data[9], data[10], data[11]]));
    let clip = f32::from_bits(u32::from_le_bytes([data[16], data[17], data[18], data[19]]));
    let load = f32::from_bits(u32::from_le_bytes([data[20], data[21], data[22], data[23]]));
    Some((frames, cutoff, clip, load))
}

fn set_enabled(val: bool) {
    let mut f = OpenOptions::new().read(true).write(true).open(CONFIG_PATH).unwrap();
    // Read current data
    let mut data = vec![0u8; 28];
    f.read(&mut data).unwrap();

    // Set enabled byte
    data[4] = if val { 1 } else { 0 };

    // Bump version
    let ver = u32::from_le_bytes([data[0], data[1], data[2], data[3]]);
    let new_ver = ver + 1;
    data[0..4].copy_from_slice(&new_ver.to_le_bytes());

    // Write back
    f.seek(SeekFrom::Start(0)).unwrap();
    f.write_all(&data).unwrap();
    f.flush().unwrap();

    println!("Set enabled={val}, version={new_ver}");
}

fn print_state() {
    if let Some((ver, en, str, hf, dyn_, tr, ph, q, syn)) = read_config() {
        println!("CONFIG: version={ver}, enabled={en}");
        println!("  strength={str:.2}, hf={hf:.2}, dynamics={dyn_:.2}, transient={tr:.2}");
        println!("  phase={ph}, quality={q}, synthesis={syn}");
    } else {
        println!("CONFIG: could not read");
    }

    if let Some((frames, cutoff, clip, load)) = read_status() {
        println!("STATUS: frames={frames}, cutoff={cutoff:.1}Hz, clip={clip:.4}, load={load:.1}%");
    } else {
        println!("STATUS: could not read");
    }
}

fn watch(duration_secs: u64) {
    let start = Instant::now();
    let mut prev_frames = 0u64;
    println!("Watching APO status for {duration_secs}s...");
    println!("{:>8} {:>10} {:>10} {:>8} {:>8} {:>8}", "elapsed", "frames", "delta", "cutoff", "clip", "load");

    while start.elapsed() < Duration::from_secs(duration_secs) {
        if let Some((frames, cutoff, clip, load)) = read_status() {
            let delta = frames.saturating_sub(prev_frames);
            let elapsed = start.elapsed().as_secs_f32();
            println!("{elapsed:>8.1}s {frames:>10} {delta:>10} {cutoff:>8.0}Hz {clip:>8.4} {load:>8.1}%");
            prev_frames = frames;
        }
        thread::sleep(Duration::from_millis(500));
    }
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let cmd = args.get(1).map(|s| s.as_str()).unwrap_or("show");

    match cmd {
        "show" => print_state(),
        "enable" => {
            set_enabled(true);
            println!("\nWatching for 5s to see if APO responds...");
            watch(5);
        }
        "disable" => {
            set_enabled(false);
            println!("\nWatching for 5s...");
            watch(5);
        }
        "watch" => watch(10),
        _ => {
            println!("Usage: apo_diag [show|enable|disable|watch]");
        }
    }
}
