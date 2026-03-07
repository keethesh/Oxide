pub mod core;
pub mod mft;
pub mod scan;
pub mod treemap;

use core::file_entry::FileEntry;
use core::file_tree::{FileRow, FileTree, NodeSummary};
use scan::progress::ScanProgress;
use scan::types::{
    FallbackReason, LaunchScanRequest, PrepareScanAction, PrepareScanResult, ScanMode, ScanResult,
};
use std::env;
use std::ffi::OsStr;
use std::os::windows::ffi::OsStrExt;
use std::path::PathBuf;
use std::sync::Mutex;
use std::time::Instant;
use treemap::layout::{self, LayoutInput, LayoutRect, Rect};
use windows::core::PCWSTR;
use windows::Win32::Foundation::CloseHandle;
use windows::Win32::Foundation::HANDLE;
use windows::Win32::Security::{GetTokenInformation, TokenElevation, TOKEN_ELEVATION, TOKEN_QUERY};
use windows::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};
use windows::Win32::UI::Shell::ShellExecuteW;
use windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;

pub struct AppState {
    tree: Mutex<FileTree>,
    prepared_scan: Mutex<Option<PreparedScanState>>,
    launch_scan_request: Mutex<Option<String>>,
}

#[derive(Debug, Clone)]
struct PreparedScanState {
    drive_letter: String,
    mode: ScanMode,
    fallback_reason: Option<FallbackReason>,
}

#[tauri::command]
fn list_drives() -> Vec<mft::volume::DriveInfo> {
    mft::volume::list_drives()
}

#[tauri::command]
fn prepare_scan(
    state: tauri::State<'_, AppState>,
    drive_letter: String,
) -> Result<PrepareScanResult, String> {
    let drive_letter = normalize_drive_letter(&drive_letter)?;
    let drive_info = find_drive_info(&drive_letter)
        .ok_or_else(|| format!("Drive {drive_letter} is not available on this system"))?;

    if !drive_info.supported {
        return Err(format!(
            "{} uses {}. Oxide currently supports NTFS volumes only.",
            drive_info.letter, drive_info.filesystem
        ));
    }

    let result = if is_process_elevated()? {
        let probe_result = scan::mft::probe(drive_letter_char(&drive_letter))
            .map(|_| ())
            .map_err(|error| error.reason);
        plan_elevated_prepare_result(probe_result)
    } else {
        plan_unelevated_prepare_result(
            relaunch_as_admin(&drive_letter).is_ok(),
            drive_letter.clone(),
        )
    };

    let mut prepared_scan = state.prepared_scan.lock().unwrap();
    *prepared_scan = match result.action {
        PrepareScanAction::Scan => result.mode.map(|mode| PreparedScanState {
            drive_letter: drive_letter.clone(),
            mode,
            fallback_reason: result.fallback_reason,
        }),
        PrepareScanAction::Relaunching => None,
    };

    Ok(result)
}

#[tauri::command]
fn get_launch_scan_request(state: tauri::State<'_, AppState>) -> LaunchScanRequest {
    let mut pending_request = state.launch_scan_request.lock().unwrap();
    LaunchScanRequest {
        drive_letter: pending_request.take(),
    }
}

#[tauri::command]
async fn scan_drive(
    window: tauri::Window,
    state: tauri::State<'_, AppState>,
    drive_letter: String,
    mode: ScanMode,
) -> Result<ScanResult, String> {
    let drive_letter = normalize_drive_letter(&drive_letter)?;
    let prepared_scan = take_prepared_scan(&state, &drive_letter, mode);
    let fallback_reason = prepared_scan.and_then(|prepared| prepared.fallback_reason);
    let drive = drive_letter_char(&drive_letter);
    let root_path = PathBuf::from(format!("{drive_letter}\\"));
    let window_clone = window.clone();
    let drive_letter_clone = drive_letter.clone();

    let (tree, result) = tokio::task::spawn_blocking(move || {
        run_scan(
            &window_clone,
            &drive_letter_clone,
            drive,
            root_path,
            mode,
            fallback_reason,
        )
    })
    .await
    .map_err(|err| format!("Scan task failed: {err}"))??;

    let mut state_tree = state.tree.lock().unwrap();
    *state_tree = tree;

    Ok(result)
}

#[tauri::command]
fn get_children(state: tauri::State<'_, AppState>, node_id: u32) -> Vec<NodeSummary> {
    let tree = state.tree.lock().unwrap();
    tree.get_children(node_id)
}

#[tauri::command]
fn get_largest_files(
    state: tauri::State<'_, AppState>,
    root_id: u32,
    offset: usize,
    limit: usize,
) -> Vec<FileRow> {
    let tree = state.tree.lock().unwrap();
    tree.get_largest_files(root_id, offset, limit)
}

#[tauri::command]
fn get_treemap_layout(
    state: tauri::State<'_, AppState>,
    root_id: u32,
    width: f32,
    height: f32,
) -> Vec<LayoutRect> {
    let tree = state.tree.lock().unwrap();
    if tree.entries.is_empty() || !tree.has_node(root_id) {
        return Vec::new();
    }

    let mut inputs = Vec::new();
    let mut child_idx = tree.entries[root_id as usize].first_child_index;

    while child_idx != FileEntry::NULL_INDEX {
        let entry = &tree.entries[child_idx as usize];
        if entry.size > 0 {
            inputs.push(LayoutInput {
                id: child_idx,
                value: entry.size,
            });
        }
        child_idx = entry.next_sibling_index;
    }

    layout::squarify(
        inputs,
        Rect {
            x: 0.0,
            y: 0.0,
            w: width,
            h: height,
        },
    )
}

#[tauri::command]
fn get_file_path(state: tauri::State<'_, AppState>, id: u32) -> Vec<(u32, String)> {
    let tree = state.tree.lock().unwrap();
    tree.get_file_path(id)
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .manage(AppState {
            tree: Mutex::new(FileTree::new()),
            prepared_scan: Mutex::new(None),
            launch_scan_request: Mutex::new(parse_launch_scan_request()),
        })
        .plugin(tauri_plugin_opener::init())
        .invoke_handler(tauri::generate_handler![
            list_drives,
            prepare_scan,
            get_launch_scan_request,
            scan_drive,
            get_children,
            get_largest_files,
            get_treemap_layout,
            get_file_path
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

fn run_scan(
    window: &tauri::Window,
    drive_letter: &str,
    drive: char,
    root_path: PathBuf,
    requested_mode: ScanMode,
    initial_fallback: Option<FallbackReason>,
) -> Result<(FileTree, ScanResult), String> {
    let started_at = Instant::now();
    let mut progress = ScanProgress::new("Preparing scan", Some(requested_mode));
    progress.fallback_reason = initial_fallback;
    scan::emit_progress(window, &mut progress, started_at);

    let (mut tree, actual_mode, fallback_reason) = match requested_mode {
        ScanMode::Mft => match scan::mft::scan(drive, window, &mut progress, started_at) {
            Ok(tree) => (tree, ScanMode::Mft, initial_fallback),
            Err(error) => {
                progress = ScanProgress::new(
                    "Switching to slower filesystem fallback",
                    Some(ScanMode::Filesystem),
                );
                progress.fallback_reason = Some(error.reason);
                scan::emit_progress(window, &mut progress, started_at);

                let mut fallback_progress =
                    ScanProgress::new("Walking filesystem", Some(ScanMode::Filesystem));
                fallback_progress.fallback_reason = Some(error.reason);
                scan::emit_progress(window, &mut fallback_progress, started_at);

                let tree =
                    scan::filesystem::scan(root_path, window, &mut fallback_progress, started_at)
                        .map_err(|err| format!("Failed to scan {}: {err}", drive_letter))?;
                progress = fallback_progress;
                (tree, ScanMode::Filesystem, Some(error.reason))
            }
        },
        ScanMode::Filesystem => {
            progress.phase = "Walking filesystem".to_string();
            progress.scan_mode = Some(ScanMode::Filesystem);
            scan::emit_progress(window, &mut progress, started_at);

            let tree = scan::filesystem::scan(root_path, window, &mut progress, started_at)
                .map_err(|err| format!("Failed to scan {}: {err}", drive_letter))?;
            (tree, ScanMode::Filesystem, initial_fallback)
        }
    };

    let root_id = tree.root_id();

    progress.phase = "Aggregating sizes".to_string();
    progress.scan_mode = Some(actual_mode);
    progress.fallback_reason = fallback_reason;
    scan::emit_progress(window, &mut progress, started_at);
    tree.aggregate_sizes();
    tree.rebuild_largest_files();

    progress.phase = "Completed".to_string();
    progress.done = true;
    scan::emit_progress(window, &mut progress, started_at);

    Ok((
        tree,
        ScanResult {
            root_id,
            drive_letter: drive_letter.to_string(),
            files_scanned: progress.files_scanned,
            dirs_scanned: progress.dirs_scanned,
            bytes_scanned: progress.bytes_scanned,
            scan_mode: actual_mode,
            fallback_reason,
            duration_ms: progress.duration_ms,
        },
    ))
}

fn plan_elevated_prepare_result(probe_result: Result<(), FallbackReason>) -> PrepareScanResult {
    match probe_result {
        Ok(()) => PrepareScanResult::scan(ScanMode::Mft, None),
        Err(reason) => PrepareScanResult::scan(ScanMode::Filesystem, Some(reason)),
    }
}

fn plan_unelevated_prepare_result(
    relaunch_succeeded: bool,
    drive_letter: String,
) -> PrepareScanResult {
    if relaunch_succeeded {
        PrepareScanResult::relaunching(drive_letter)
    } else {
        PrepareScanResult::scan(ScanMode::Filesystem, Some(FallbackReason::UacDeclined))
    }
}

fn find_drive_info(drive_letter: &str) -> Option<mft::volume::DriveInfo> {
    mft::volume::list_drives()
        .into_iter()
        .find(|drive| drive.letter.eq_ignore_ascii_case(drive_letter))
}

fn take_prepared_scan(
    state: &tauri::State<'_, AppState>,
    drive_letter: &str,
    mode: ScanMode,
) -> Option<PreparedScanState> {
    let mut prepared_scan = state.prepared_scan.lock().unwrap();
    match prepared_scan.take() {
        Some(prepared)
            if prepared.drive_letter.eq_ignore_ascii_case(drive_letter)
                && prepared.mode == mode =>
        {
            Some(prepared)
        }
        Some(other) => {
            *prepared_scan = Some(other);
            None
        }
        None => None,
    }
}

fn normalize_drive_letter(raw_drive_letter: &str) -> Result<String, String> {
    let drive = raw_drive_letter
        .trim()
        .chars()
        .next()
        .ok_or_else(|| "Drive letter is required".to_string())?
        .to_ascii_uppercase();

    if !drive.is_ascii_alphabetic() {
        return Err(format!("Invalid drive letter: {raw_drive_letter}"));
    }

    Ok(format!("{drive}:"))
}

fn drive_letter_char(drive_letter: &str) -> char {
    drive_letter.chars().next().unwrap_or('C')
}

fn parse_launch_scan_request() -> Option<String> {
    let args: Vec<String> = env::args().collect();
    parse_launch_scan_request_from_args(&args)
}

fn parse_launch_scan_request_from_args(args: &[String]) -> Option<String> {
    args.windows(2).find_map(|window| {
        if window[0] == "--scan-drive" {
            normalize_drive_letter(&window[1]).ok()
        } else {
            None
        }
    })
}

fn is_process_elevated() -> Result<bool, String> {
    let mut token = HANDLE::default();
    unsafe {
        OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token)
            .map_err(|err| format!("Failed to inspect process token: {err}"))?;

        let mut elevation = TOKEN_ELEVATION::default();
        let mut return_length = 0u32;
        let result = GetTokenInformation(
            token,
            TokenElevation,
            Some(&mut elevation as *mut _ as *mut _),
            std::mem::size_of::<TOKEN_ELEVATION>() as u32,
            &mut return_length,
        )
        .map_err(|err| format!("Failed to read elevation state: {err}"));

        let _ = CloseHandle(token);
        result?;
        Ok(elevation.TokenIsElevated != 0)
    }
}

fn relaunch_as_admin(drive_letter: &str) -> Result<(), String> {
    let executable =
        env::current_exe().map_err(|err| format!("Failed to locate executable: {err}"))?;
    let operation = to_wide("runas");
    let executable_path = to_wide(&executable.to_string_lossy());
    let arguments = to_wide(&format!(
        "--scan-drive {}",
        drive_letter.trim_end_matches(':')
    ));

    let result = unsafe {
        ShellExecuteW(
            None,
            PCWSTR(operation.as_ptr()),
            PCWSTR(executable_path.as_ptr()),
            PCWSTR(arguments.as_ptr()),
            PCWSTR::null(),
            SW_SHOWNORMAL,
        )
    };

    if result.0 as usize <= 32 {
        Err("Elevation was cancelled or failed".to_string())
    } else {
        Ok(())
    }
}

fn to_wide(value: &str) -> Vec<u16> {
    OsStr::new(value)
        .encode_wide()
        .chain(std::iter::once(0))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::file_entry::FileFlags;
    use std::collections::HashMap;

    fn dir_entry(tree: &mut FileTree, name: &str) -> u32 {
        let (name_offset, name_len) = tree.names.push(name);
        tree.add_entry(FileEntry {
            size: 0,
            parent_index: FileEntry::NULL_INDEX,
            first_child_index: FileEntry::NULL_INDEX,
            next_sibling_index: FileEntry::NULL_INDEX,
            name_offset,
            name_len,
            flags: FileFlags::Directory as u16,
        })
    }

    fn file_entry(tree: &mut FileTree, name: &str, size: u64) -> u32 {
        let (name_offset, name_len) = tree.names.push(name);
        tree.add_entry(FileEntry {
            size,
            parent_index: FileEntry::NULL_INDEX,
            first_child_index: FileEntry::NULL_INDEX,
            next_sibling_index: FileEntry::NULL_INDEX,
            name_offset,
            name_len,
            flags: 0,
        })
    }

    #[test]
    fn link_entries_attach_missing_parents_to_root() {
        let mut tree = FileTree::with_root("C:\\");
        let root_id = tree.root_id();
        let docs = dir_entry(&mut tree, "docs");
        let orphan = file_entry(&mut tree, "orphan.bin", 100);
        let mut lookup = HashMap::new();
        lookup.insert(5, docs);

        scan::mft::link_mft_entries(&mut tree, &[(docs, 0), (orphan, 999)], &lookup, root_id);

        assert_eq!(tree.entries[docs as usize].parent_index, root_id);
        assert_eq!(tree.entries[orphan as usize].parent_index, root_id);
    }

    #[test]
    fn link_entries_handles_skipped_records() {
        let mut tree = FileTree::with_root("C:\\");
        let root_id = tree.root_id();
        let parent = dir_entry(&mut tree, "parent");
        let child = file_entry(&mut tree, "child.bin", 25);
        let mut lookup = HashMap::new();
        lookup.insert(42, parent);

        scan::mft::link_mft_entries(&mut tree, &[(child, 42)], &lookup, root_id);

        assert_eq!(tree.entries[child as usize].parent_index, parent);
        assert_eq!(tree.entries[parent as usize].first_child_index, child);
    }

    #[test]
    fn aggregate_sizes_roll_up_nested_directories() {
        let mut tree = FileTree::with_root("C:\\");
        let folder = dir_entry(&mut tree, "folder");
        let nested = dir_entry(&mut tree, "nested");
        let file_a = file_entry(&mut tree, "a.bin", 10);
        let file_b = file_entry(&mut tree, "b.bin", 20);

        tree.attach_child(tree.root_id(), folder);
        tree.attach_child(folder, nested);
        tree.attach_child(folder, file_a);
        tree.attach_child(nested, file_b);

        tree.aggregate_sizes();

        assert_eq!(tree.entries[nested as usize].size, 20);
        assert_eq!(tree.entries[folder as usize].size, 30);
        assert_eq!(tree.entries[tree.root_id() as usize].size, 30);
    }

    #[test]
    fn largest_files_are_sorted_and_paginated() {
        let mut tree = FileTree::with_root("C:\\");
        let file_a = file_entry(&mut tree, "a.bin", 10);
        let file_b = file_entry(&mut tree, "b.bin", 30);
        let file_c = file_entry(&mut tree, "c.bin", 20);
        tree.attach_child(tree.root_id(), file_a);
        tree.attach_child(tree.root_id(), file_b);
        tree.attach_child(tree.root_id(), file_c);
        tree.aggregate_sizes();
        tree.rebuild_largest_files();

        let page = tree.get_largest_files(tree.root_id(), 1, 1);

        assert_eq!(page.len(), 1);
        assert_eq!(page[0].name, "c.bin");
        assert_eq!(page[0].size, 20);
    }

    #[test]
    fn file_paths_reconstruct_full_paths() {
        let mut tree = FileTree::with_root("C:\\");
        let folder = dir_entry(&mut tree, "folder");
        let file = file_entry(&mut tree, "file.txt", 5);
        tree.attach_child(tree.root_id(), folder);
        tree.attach_child(folder, file);

        assert_eq!(tree.get_full_path(file), "C:\\folder\\file.txt");
    }

    #[test]
    fn root_directory_records_are_normalized_to_the_virtual_root() {
        let mut tree = FileTree::with_root("C:\\");
        let top_level = dir_entry(&mut tree, "Users");
        let mut lookup = HashMap::new();
        let root_id = tree.root_id();
        lookup.insert(5, root_id);

        scan::mft::link_mft_entries(&mut tree, &[(top_level, 5)], &lookup, root_id);

        assert_eq!(
            tree.entries[top_level as usize].parent_index,
            tree.root_id()
        );
    }

    #[test]
    fn ntfs_plus_elevated_chooses_mft_mode() {
        let result = plan_elevated_prepare_result(Ok(()));

        assert_eq!(result.action, PrepareScanAction::Scan);
        assert_eq!(result.mode, Some(ScanMode::Mft));
        assert_eq!(result.fallback_reason, None);
    }

    #[test]
    fn declined_uac_chooses_filesystem_fallback() {
        let result = plan_unelevated_prepare_result(false, "C:".to_string());

        assert_eq!(result.action, PrepareScanAction::Scan);
        assert_eq!(result.mode, Some(ScanMode::Filesystem));
        assert_eq!(result.fallback_reason, Some(FallbackReason::UacDeclined));
    }

    #[test]
    fn probe_timeout_chooses_filesystem_fallback() {
        let result = plan_elevated_prepare_result(Err(FallbackReason::MftProbeTimeout));

        assert_eq!(result.action, PrepareScanAction::Scan);
        assert_eq!(result.mode, Some(ScanMode::Filesystem));
        assert_eq!(
            result.fallback_reason,
            Some(FallbackReason::MftProbeTimeout)
        );
    }

    #[test]
    fn startup_relaunch_request_parsing_returns_pending_drive() {
        let args = vec![
            "oxide.exe".to_string(),
            "--scan-drive".to_string(),
            "d".to_string(),
        ];

        assert_eq!(
            parse_launch_scan_request_from_args(&args),
            Some("D:".to_string())
        );
    }
}
