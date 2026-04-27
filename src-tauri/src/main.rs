// Prevents additional console window on Windows in release, DO NOT REMOVE!!
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use oxide_lib::scan::filesystem;
use oxide_lib::scan::progress::ScanProgress;
use oxide_lib::scan::types::ScanMode;
use oxide_lib::scan::SilentProgressSink;
use std::path::PathBuf;
use std::sync::atomic::AtomicBool;
use std::sync::Arc;
use std::time::Instant;

fn main() {
    let args: Vec<String> = std::env::args().collect();

    // Check for CLI profile mode: oxide profile <drive>
    if args.len() >= 2 && args[1] == "profile" {
        if let Err(err) = run_profile(&args) {
            eprintln!("error: {err}");
            std::process::exit(1);
        }
        return;
    }

    oxide_lib::run()
}

fn run_profile(args: &[String]) -> Result<(), String> {
    let drive_arg = args.get(2).map(|s| s.as_str()).unwrap_or("C");
    let drive: char = drive_arg
        .trim()
        .trim_end_matches(':')
        .chars()
        .next()
        .ok_or("Drive letter required: oxide profile <drive>")?
        .to_ascii_uppercase();

    if !drive.is_ascii_alphabetic() {
        return Err(format!("Invalid drive letter: {drive_arg}"));
    }

    let cancel_flag = Arc::new(AtomicBool::new(false));
    let mut sink = SilentProgressSink;
    let mut progress = ScanProgress::new("Preparing scan", Some(ScanMode::Mft));
    let started_at = Instant::now();

    // Try MFT first, fall back to filesystem
    let (mut tree, actual_mode) = match oxide_lib::scan::mft::scan(
        drive,
        &mut sink,
        &mut progress,
        started_at,
        &cancel_flag,
    ) {
        Ok(tree) => (tree, ScanMode::Mft),
        Err(err) => {
            eprintln!("MFT scan failed ({}), falling back to filesystem walk...", err.message);
            let root_path = PathBuf::from(format!("{}:\\", drive));
            let mut fs_progress = ScanProgress::new("Walking filesystem", Some(ScanMode::Filesystem));
            let tree = filesystem::scan(root_path, &mut sink, &mut fs_progress, started_at, &cancel_flag)
                .map_err(|e| format!("Filesystem scan failed: {e}"))?;
            (tree, ScanMode::Filesystem)
        }
    };

    let scan_ms = started_at.elapsed().as_millis() as u64;

    let aggregate_started_at = Instant::now();
    tree.aggregate_sizes();
    let aggregate_ms = aggregate_started_at.elapsed().as_millis() as u64;

    let index_started_at = Instant::now();
    tree.rebuild_largest_files();
    let index_ms = index_started_at.elapsed().as_millis() as u64;

    let total_ms = started_at.elapsed().as_millis() as u64;

    let entry_count = tree.entries.len();
    let name_bytes = tree.names.len();

    eprintln!(
        "oxide profile drive={}: mode={:?} entries={} name_bytes={} scan_ms={} aggregate_ms={} index_ms={} total_ms={}",
        drive, actual_mode, entry_count, name_bytes, scan_ms, aggregate_ms, index_ms, total_ms
    );

    Ok(())
}
