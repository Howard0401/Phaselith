use std::env;
use std::process::ExitCode;
use std::sync::atomic::Ordering;

use phaselith_core_audio::mmap_ipc::MmapIpc;

fn write_enabled(ipc: &MmapIpc, enabled: bool) {
    let config = ipc.config();
    config.enabled.store(enabled, Ordering::Relaxed);
    config.version.fetch_add(1, Ordering::Release);
}

fn print_status(ipc: &MmapIpc) {
    let config = ipc.config();
    let status = ipc.status();

    let enabled = config.enabled.load(Ordering::Relaxed);
    let version = config.version.load(Ordering::Acquire);
    let frame_count = status.frame_count.load(Ordering::Relaxed);
    let cutoff = f32::from_bits(status.current_cutoff_u32.load(Ordering::Relaxed));
    let clipping = f32::from_bits(status.current_clipping_u32.load(Ordering::Relaxed));
    let load = f32::from_bits(status.processing_load_u32.load(Ordering::Relaxed));
    let wet_dry_diff = f32::from_bits(status.wet_dry_diff_db_u32.load(Ordering::Relaxed));

    println!("enabled={enabled}");
    println!("config_version={version}");
    println!("frame_count={frame_count}");
    println!("cutoff_hz={cutoff}");
    println!("clipping={clipping}");
    println!("processing_load_pct={load}");
    println!("wet_dry_diff_db={wet_dry_diff}");
}

fn print_usage() {
    eprintln!("Usage: cargo run -p phaselith-core-audio --example control -- <status|enable|disable|toggle>");
}

fn main() -> ExitCode {
    let mut args = env::args().skip(1);
    let Some(command) = args.next() else {
        print_usage();
        return ExitCode::from(2);
    };

    let ipc = match MmapIpc::open_or_create() {
        Ok(ipc) => ipc,
        Err(err) => {
            eprintln!("Failed to open Core Audio shared mmap: {err}");
            return ExitCode::from(1);
        }
    };

    match command.as_str() {
        "status" => {
            print_status(&ipc);
            ExitCode::SUCCESS
        }
        "enable" => {
            write_enabled(&ipc, true);
            print_status(&ipc);
            ExitCode::SUCCESS
        }
        "disable" => {
            write_enabled(&ipc, false);
            print_status(&ipc);
            ExitCode::SUCCESS
        }
        "toggle" => {
            let enabled = ipc.config().enabled.load(Ordering::Relaxed);
            write_enabled(&ipc, !enabled);
            print_status(&ipc);
            ExitCode::SUCCESS
        }
        _ => {
            print_usage();
            ExitCode::from(2)
        }
    }
}
