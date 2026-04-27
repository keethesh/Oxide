pub mod core;
pub mod mft;
pub mod scan;
pub mod treemap;

use core::file_entry::FileEntry;
use core::file_tree::{ChildPage, FilePathRow, FileRow, FileTree};
use crate::scan::{ProgressSink, WindowProgressSink};
use scan::progress::ScanProgress;
use scan::types::{
    FallbackReason, LaunchScanRequest, PrepareScanAction, PrepareScanResult, ScanMode, ScanResult,
    ScanTimings,
};
use std::collections::{HashMap, VecDeque};
use std::env;
use std::ffi::OsStr;
use std::os::windows::ffi::OsStrExt;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, RwLock};
use std::time::Instant;
use treemap::layout::{self, LayoutInput, LayoutRect, LayoutRectKind, Rect};
use windows::core::PCWSTR;
use windows::Win32::Foundation::CloseHandle;
use windows::Win32::Foundation::HANDLE;
use windows::Win32::Security::{GetTokenInformation, TokenElevation, TOKEN_ELEVATION, TOKEN_QUERY};
use windows::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};
use windows::Win32::UI::Shell::ShellExecuteW;
use windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;

pub struct AppState {
    tree: Arc<RwLock<FileTree>>,
    treemap_layout_cache: Arc<Mutex<TreemapLayoutCache>>,
    children_sort_cache: Arc<Mutex<ChildSortCache>>,
    prepared_scan: Mutex<Option<PreparedScanState>>,
    launch_scan_request: Mutex<Option<String>>,
    scan_cancelled: Arc<AtomicBool>,
}

#[derive(Debug, Clone)]
struct PreparedScanState {
    drive_letter: String,
    mode: ScanMode,
    fallback_reason: Option<FallbackReason>,
}

const TREEMAP_MAX_RECTS: usize = 400;
const TREEMAP_CACHE_CAPACITY: usize = 16;
const CHILDREN_SORT_CACHE_CAPACITY: usize = 64;
const TREEMAP_BUCKET_SIZE: f32 = 32.0;
const TREEMAP_RECURSE_MIN_SIDE: f32 = 72.0;
const TREEMAP_RECURSE_PADDING: f32 = 1.0;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct LayoutCacheKey {
    root_id: u32,
    width_bucket: u32,
    height_bucket: u32,
}

#[derive(Debug, Clone)]
struct CachedLayout {
    width: f32,
    height: f32,
    layout: Vec<LayoutRect>,
}

#[derive(Debug)]
struct TreemapLayoutCache {
    entries: HashMap<LayoutCacheKey, CachedLayout>,
    order: VecDeque<LayoutCacheKey>,
    capacity: usize,
}

impl TreemapLayoutCache {
    fn new(capacity: usize) -> Self {
        Self {
            entries: HashMap::new(),
            order: VecDeque::new(),
            capacity,
        }
    }

    fn get(&mut self, key: &LayoutCacheKey, width: f32, height: f32) -> Option<Vec<LayoutRect>> {
        // Extract owned data in one block, then release the borrow on self.entries.
        let (cached_layout, cached_width, cached_height) = {
            let cached = self.entries.get(key)?;
            (
                cached.layout.clone(),
                cached.width,
                cached.height,
            )
        };
        let needs_scale = (cached_width - width).abs() >= f32::EPSILON
            || (cached_height - height).abs() >= f32::EPSILON;
        self.touch(key.clone());
        if needs_scale {
            Some(scale_layout(&cached_layout, cached_width, cached_height, width, height))
        } else {
            Some(cached_layout)
        }
    }

    fn insert(&mut self, key: LayoutCacheKey, width: f32, height: f32, layout: Vec<LayoutRect>) {
        self.entries.insert(
            key.clone(),
            CachedLayout {
                width,
                height,
                layout,
            },
        );
        self.touch(key);

        while self.order.len() > self.capacity {
            if let Some(oldest_key) = self.order.pop_front() {
                self.entries.remove(&oldest_key);
            }
        }
    }

    fn clear(&mut self) {
        self.entries.clear();
        self.order.clear();
    }

    fn touch(&mut self, key: LayoutCacheKey) {
        self.order.retain(|existing| existing != &key);
        self.order.push_back(key);
    }
}

#[derive(Debug)]
struct ChildSortCache {
    entries: HashMap<u32, Arc<[u32]>>,
    order: VecDeque<u32>,
    capacity: usize,
}

impl ChildSortCache {
    fn new(capacity: usize) -> Self {
        Self {
            entries: HashMap::new(),
            order: VecDeque::new(),
            capacity,
        }
    }

    fn get(&mut self, node_id: u32) -> Option<Arc<[u32]>> {
        let child_ids = self.entries.get(&node_id)?.clone();
        self.touch(node_id);
        Some(child_ids)
    }

    fn insert(&mut self, node_id: u32, child_ids: Vec<u32>) {
        self.entries.insert(node_id, Arc::from(child_ids));
        self.touch(node_id);

        while self.order.len() > self.capacity {
            if let Some(oldest_key) = self.order.pop_front() {
                self.entries.remove(&oldest_key);
            }
        }
    }

    fn clear(&mut self) {
        self.entries.clear();
        self.order.clear();
    }

    fn touch(&mut self, node_id: u32) {
        self.order.retain(|existing| existing != &node_id);
        self.order.push_back(node_id);
    }
}

#[tauri::command]
fn list_drives() -> Vec<mft::volume::DriveInfo> {
    mft::volume::list_drives()
}

#[tauri::command]
fn cancel_scan(state: tauri::State<'_, AppState>) {
    state.scan_cancelled.store(true, Ordering::SeqCst);
}

#[tauri::command]
async fn open_in_explorer(
    state: tauri::State<'_, AppState>,
    node_id: u32,
) -> Result<(), String> {
    let tree = state.tree.clone();
    let path = tokio::task::spawn_blocking(move || {
        let tree = tree.read().unwrap();
        tree.get_full_path(node_id)
    })
    .await
    .map_err(|err| format!("Path query task failed: {err}"))?;

    open_path_in_explorer(&path)?;
    Ok(())
}

fn open_path_in_explorer(path: &str) -> Result<(), String> {
    let wide_path = to_wide(path);
    let operation = to_wide("open");
    let result = unsafe {
        ShellExecuteW(
            None,
            PCWSTR(operation.as_ptr()),
            PCWSTR(wide_path.as_ptr()),
            PCWSTR::null(),
            PCWSTR::null(),
            SW_SHOWNORMAL,
        )
    };

    if result.0 as usize <= 32 {
        Err("Failed to open path in Explorer".to_string())
    } else {
        Ok(())
    }
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
        let probe_result = scan::mft::probe(drive_letter_char(&drive_letter), &state.scan_cancelled)
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
    let drive_letter_clone = drive_letter.clone();
    let window_clone = window.clone();
    let cancel_flag = state.scan_cancelled.clone();

    cancel_flag.store(false, Ordering::SeqCst);

    let (tree, mut result) = tokio::task::spawn_blocking(move || {
        let scan_started_at = Instant::now();
        let mut sink = WindowProgressSink::new(&window_clone, scan_started_at);
        run_scan(
            &mut sink,
            &drive_letter_clone,
            drive,
            root_path,
            mode,
            fallback_reason,
            &cancel_flag,
        )
    })
    .await
    .map_err(|err| format!("Scan task failed: {err}"))??;

    let store_started_at = Instant::now();
    let mut state_tree = state.tree.write().unwrap();
    *state_tree = tree;
    drop(state_tree);
    state.treemap_layout_cache.lock().unwrap().clear();
    state.children_sort_cache.lock().unwrap().clear();
    let store_ms = elapsed_ms(store_started_at);
    result.timings.store_ms = store_ms;
    result.timings.total_ms = result.timings.total_ms.saturating_add(store_ms);
    result.duration_ms = result.timings.total_ms;

    eprintln!(
        "oxide scan profile drive={} mode={:?} scan_ms={} aggregate_ms={} largest_files_ms={} store_ms={} total_ms={}",
        result.drive_letter,
        result.scan_mode,
        result.timings.scan_ms,
        result.timings.aggregate_ms,
        result.timings.largest_files_ms,
        result.timings.store_ms,
        result.timings.total_ms
    );

    Ok(result)
}

#[tauri::command]
async fn get_children(
    state: tauri::State<'_, AppState>,
    node_id: u32,
    offset: usize,
    limit: usize,
) -> Result<ChildPage, String> {
    let tree = state.tree.clone();
    let cache = state.children_sort_cache.clone();
    tokio::task::spawn_blocking(move || {
        let started_at = Instant::now();

        let cached_child_ids = cache.lock().unwrap().get(node_id);
        let cache_hit = cached_child_ids.is_some();

        let tree = tree.read().unwrap();
        let (page, cache_insert) = if let Some(child_ids) = cached_child_ids {
            (
                tree.get_children_page_from_sorted_ids(child_ids.as_ref(), offset, limit),
                None,
            )
        } else {
            let child_ids = tree.get_sorted_child_ids(node_id);
            let page = tree.get_children_page_from_sorted_ids(&child_ids, offset, limit);
            (page, Some(child_ids))
        };
        drop(tree);

        if let Some(child_ids) = cache_insert {
            cache.lock().unwrap().insert(node_id, child_ids);
        }

        log_slow_query(
            "get_children",
            started_at,
            format!(
                "node_id={node_id} returned={} total={} cache_hit={cache_hit}",
                page.items.len(),
                page.total
            ),
        );
        page
    })
    .await
    .map_err(|err| format!("Child query task failed: {err}"))
}

#[tauri::command]
async fn get_largest_files(
    state: tauri::State<'_, AppState>,
    root_id: u32,
    offset: usize,
    limit: usize,
) -> Result<Vec<FileRow>, String> {
    let tree = state.tree.clone();
    tokio::task::spawn_blocking(move || {
        let started_at = Instant::now();
        let tree = tree.read().unwrap();
        let rows = tree.get_largest_files(root_id, offset, limit);
        log_slow_query(
            "get_largest_files",
            started_at,
            format!("root_id={root_id} returned={}", rows.len()),
        );
        rows
    })
    .await
    .map_err(|err| format!("Largest-files query task failed: {err}"))
}

#[tauri::command]
async fn get_file_paths(
    state: tauri::State<'_, AppState>,
    file_ids: Vec<u32>,
) -> Result<Vec<FilePathRow>, String> {
    let tree = state.tree.clone();
    tokio::task::spawn_blocking(move || {
        let tree = tree.read().unwrap();
        tree.get_file_paths(&file_ids)
    })
    .await
    .map_err(|err| format!("File-path query task failed: {err}"))
}

#[tauri::command]
async fn get_treemap_layout(
    state: tauri::State<'_, AppState>,
    root_id: u32,
    width: f32,
    height: f32,
) -> Result<Vec<LayoutRect>, String> {
    let tree = state.tree.clone();
    let cache = state.treemap_layout_cache.clone();
    tokio::task::spawn_blocking(move || {
        let started_at = Instant::now();
        if width <= 0.0 || height <= 0.0 {
            return Vec::new();
        }

        let cache_key = LayoutCacheKey {
            root_id,
            width_bucket: dimension_bucket(width),
            height_bucket: dimension_bucket(height),
        };
        if let Some(layout) = cache.lock().unwrap().get(&cache_key, width, height) {
            return layout;
        }

        let tree = tree.read().unwrap();
        if tree.entries.is_empty() || !tree.has_node(root_id) {
            return Vec::new();
        }

        let layout = build_recursive_treemap(
            &tree,
            root_id,
            Rect {
                x: 0.0,
                y: 0.0,
                w: width,
                h: height,
            },
            TREEMAP_MAX_RECTS,
        );
        drop(tree);

        cache
            .lock()
            .unwrap()
            .insert(cache_key, width, height, layout.clone());
        log_slow_query(
            "get_treemap_layout",
            started_at,
            format!("root_id={root_id} rects={}", layout.len()),
        );

        layout
    })
    .await
    .map_err(|err| format!("Treemap query task failed: {err}"))
}

#[tauri::command]
async fn get_file_path(
    state: tauri::State<'_, AppState>,
    id: u32,
) -> Result<Vec<(u32, String)>, String> {
    let tree = state.tree.clone();
    tokio::task::spawn_blocking(move || {
        let tree = tree.read().unwrap();
        tree.get_file_path(id)
    })
    .await
    .map_err(|err| format!("Breadcrumb query task failed: {err}"))
}

fn build_treemap_inputs(tree: &FileTree, root_id: u32, max_rects: usize) -> Vec<LayoutInput> {
    if max_rects == 0 || !tree.has_node(root_id) {
        return Vec::new();
    }

    let mut candidates = Vec::new();
    let mut child_idx = tree.entries[root_id as usize].first_child_index;

    while child_idx != FileEntry::NULL_INDEX {
        let entry = &tree.entries[child_idx as usize];
        if entry.size > 0 {
            candidates.push((child_idx, entry.size));
        }
        child_idx = entry.next_sibling_index;
    }

    let mut inputs = Vec::new();

    if candidates.len() > max_rects {
        // Partial sort: only fully sort the top N, avoid sorting the rest
        let overflow_start = max_rects.saturating_sub(1);
        candidates.select_nth_unstable_by(overflow_start, |a, b| {
            b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0))
        });
        let overflow_value: u64 = candidates[overflow_start..]
            .iter()
            .map(|(_, value)| *value)
            .sum();

        // Only sort the top portion we actually use
        candidates[..overflow_start].sort_unstable_by(|a, b| {
            b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0))
        });

        for (child_id, value) in candidates.into_iter().take(overflow_start) {
            inputs.push(LayoutInput {
                id: Some(child_id),
                label: tree.display_name(child_id),
                kind: LayoutRectKind::Node,
                value,
            });
        }

        inputs.push(LayoutInput {
            id: None,
            label: "Other".to_string(),
            kind: LayoutRectKind::Overflow,
            value: overflow_value,
        });
        return inputs;
    }

    candidates.sort_unstable_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));

    for (child_id, value) in candidates {
        inputs.push(LayoutInput {
            id: Some(child_id),
            label: tree.display_name(child_id),
            kind: LayoutRectKind::Node,
            value,
        });
    }

    inputs
}

fn build_recursive_treemap(
    tree: &FileTree,
    root_id: u32,
    bounds: Rect,
    max_rects: usize,
) -> Vec<LayoutRect> {
    let mut layout = Vec::with_capacity(max_rects);
    append_treemap_layout(tree, root_id, bounds, max_rects, &mut layout);
    layout
}

fn append_treemap_layout(
    tree: &FileTree,
    root_id: u32,
    bounds: Rect,
    max_rects: usize,
    layout: &mut Vec<LayoutRect>,
) {
    if layout.len() >= max_rects || bounds.w <= 0.0 || bounds.h <= 0.0 {
        return;
    }

    let remaining = max_rects - layout.len();
    let node_layout = layout::squarify(build_treemap_inputs(tree, root_id, remaining), bounds);
    if node_layout.is_empty() {
        return;
    }

    let start_index = layout.len();
    layout.extend(node_layout);

    for rect in layout[start_index..].to_vec() {
        if layout.len() >= max_rects
            || rect.kind != LayoutRectKind::Node
            || rect.w < TREEMAP_RECURSE_MIN_SIDE
            || rect.h < TREEMAP_RECURSE_MIN_SIDE
        {
            continue;
        }

        let Some(child_id) = rect.id else {
            continue;
        };
        let child_entry = &tree.entries[child_id as usize];
        if !child_entry.is_dir() || child_entry.first_child_index == FileEntry::NULL_INDEX {
            continue;
        }

        let child_bounds = inset_rect(&rect, TREEMAP_RECURSE_PADDING);
        append_treemap_layout(tree, child_id, child_bounds, max_rects, layout);
    }
}

fn inset_rect(rect: &LayoutRect, padding: f32) -> Rect {
    let width = (rect.w - padding * 2.0).max(0.0);
    let height = (rect.h - padding * 2.0).max(0.0);

    Rect {
        x: rect.x + padding,
        y: rect.y + padding,
        w: width,
        h: height,
    }
}

fn dimension_bucket(value: f32) -> u32 {
    ((value / TREEMAP_BUCKET_SIZE).round().max(1.0)) as u32
}

fn scale_layout(
    layout: &[LayoutRect],
    source_width: f32,
    source_height: f32,
    target_width: f32,
    target_height: f32,
) -> Vec<LayoutRect> {
    if layout.is_empty()
        || source_width <= 0.0
        || source_height <= 0.0
        || (source_width - target_width).abs() < f32::EPSILON
            && (source_height - target_height).abs() < f32::EPSILON
    {
        return layout.to_vec();
    }

    let scale_x = target_width / source_width;
    let scale_y = target_height / source_height;

    layout
        .iter()
        .map(|rect| LayoutRect {
            id: rect.id,
            kind: rect.kind,
            label: rect.label.clone(),
            x: rect.x * scale_x,
            y: rect.y * scale_y,
            w: rect.w * scale_x,
            h: rect.h * scale_y,
        })
        .collect()
}

fn log_slow_query(name: &str, started_at: Instant, details: String) {
    let elapsed = started_at.elapsed();
    if elapsed.as_millis() >= 250 {
        eprintln!(
            "[oxide] slow query {name} took {} ms ({details})",
            elapsed.as_millis()
        );
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .manage(AppState {
            tree: Arc::new(RwLock::new(FileTree::new())),
            treemap_layout_cache: Arc::new(Mutex::new(TreemapLayoutCache::new(
                TREEMAP_CACHE_CAPACITY,
            ))),
            children_sort_cache: Arc::new(Mutex::new(ChildSortCache::new(
                CHILDREN_SORT_CACHE_CAPACITY,
            ))),
            prepared_scan: Mutex::new(None),
            launch_scan_request: Mutex::new(parse_launch_scan_request()),
            scan_cancelled: Arc::new(AtomicBool::new(false)),
        })
        .plugin(tauri_plugin_opener::init())
        .invoke_handler(tauri::generate_handler![
            list_drives,
            prepare_scan,
            get_launch_scan_request,
            scan_drive,
            cancel_scan,
            open_in_explorer,
            get_children,
            get_largest_files,
            get_file_paths,
            get_treemap_layout,
            get_file_path
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

fn run_scan(
    sink: &mut dyn ProgressSink,
    drive_letter: &str,
    drive: char,
    root_path: PathBuf,
    requested_mode: ScanMode,
    initial_fallback: Option<FallbackReason>,
    cancel_flag: &Arc<AtomicBool>,
) -> Result<(FileTree, ScanResult), String> {
    let started_at = Instant::now();
    let mut progress = ScanProgress::new("Preparing scan", Some(requested_mode));
    progress.fallback_reason = initial_fallback;
    sink.emit(&mut progress);

    if cancel_flag.load(Ordering::SeqCst) {
        return Err("Scan cancelled".to_string());
    }

    let scan_started_at = Instant::now();
    let (mut tree, actual_mode, fallback_reason) = match requested_mode {
        ScanMode::Mft => match scan::mft::scan(drive, sink, &mut progress, started_at, cancel_flag) {
            Ok(tree) => (tree, ScanMode::Mft, initial_fallback),
            Err(error) => {
                if cancel_flag.load(Ordering::SeqCst) {
                    return Err("Scan cancelled".to_string());
                }
                progress = ScanProgress::new(
                    "Switching to slower filesystem fallback",
                    Some(ScanMode::Filesystem),
                );
                progress.fallback_reason = Some(error.reason);
                sink.emit(&mut progress);

                let mut fallback_progress =
                    ScanProgress::new("Walking filesystem", Some(ScanMode::Filesystem));
                fallback_progress.fallback_reason = Some(error.reason);
                sink.emit(&mut fallback_progress);

                let tree =
                    scan::filesystem::scan(root_path, sink, &mut fallback_progress, started_at, cancel_flag)
                        .map_err(|err| format!("Failed to scan {}: {err}", drive_letter))?;
                progress = fallback_progress;
                (tree, ScanMode::Filesystem, Some(error.reason))
            }
        },
        ScanMode::Filesystem => {
            progress.phase = "Walking filesystem".to_string();
            progress.scan_mode = Some(ScanMode::Filesystem);
            sink.emit(&mut progress);

            let tree = scan::filesystem::scan(root_path, sink, &mut progress, started_at, cancel_flag)
                .map_err(|err| format!("Failed to scan {}: {err}", drive_letter))?;
            (tree, ScanMode::Filesystem, initial_fallback)
        }
    };
    let scan_ms = elapsed_ms(scan_started_at);

    let root_id = tree.root_id();

    progress.phase = "Aggregating sizes".to_string();
    progress.scan_mode = Some(actual_mode);
    progress.fallback_reason = fallback_reason;
    sink.emit(&mut progress);

    let aggregate_started_at = Instant::now();
    tree.aggregate_sizes();
    let aggregate_ms = elapsed_ms(aggregate_started_at);

    progress.phase = "Indexing largest files".to_string();
    sink.emit(&mut progress);
    let largest_files_started_at = Instant::now();
    tree.rebuild_largest_files();
    let largest_files_ms = elapsed_ms(largest_files_started_at);

    progress.phase = "Completed".to_string();
    progress.done = true;
    sink.emit(&mut progress);
    let total_ms = elapsed_ms(started_at);

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
            duration_ms: total_ms,
            timings: ScanTimings {
                scan_ms,
                aggregate_ms,
                largest_files_ms,
                store_ms: 0,
                total_ms,
            },
        },
    ))
}

fn elapsed_ms(started_at: Instant) -> u64 {
    started_at.elapsed().as_millis().min(u64::MAX as u128) as u64
}

fn plan_elevated_prepare_result(probe_result: Result<u64, FallbackReason>) -> PrepareScanResult {
    match probe_result {
        Ok(total_items) => PrepareScanResult::scan(ScanMode::Mft, None, Some(total_items)),
        Err(reason) => PrepareScanResult::scan(ScanMode::Filesystem, Some(reason), None),
    }
}

fn plan_unelevated_prepare_result(
    relaunch_succeeded: bool,
    drive_letter: String,
) -> PrepareScanResult {
    if relaunch_succeeded {
        PrepareScanResult::relaunching(drive_letter)
    } else {
        PrepareScanResult::scan(ScanMode::Filesystem, Some(FallbackReason::UacDeclined), None)
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
        let mut lookup = vec![u32::MAX; 6];
        lookup[5] = docs;

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
        let mut lookup = vec![u32::MAX; 43];
        lookup[42] = parent;

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
    fn child_pages_are_sorted_and_paginated() {
        let mut tree = FileTree::with_root("C:\\");
        let folder = dir_entry(&mut tree, "folder");
        let file_a = file_entry(&mut tree, "a.bin", 10);
        let file_b = file_entry(&mut tree, "b.bin", 5);
        tree.attach_child(tree.root_id(), file_b);
        tree.attach_child(tree.root_id(), file_a);
        tree.attach_child(tree.root_id(), folder);

        let page = tree.get_children_page(tree.root_id(), 1, 1);

        assert_eq!(page.total, 3);
        assert_eq!(page.next_offset, Some(2));
        assert_eq!(page.items.len(), 1);
        assert_eq!(page.items[0].name, "a.bin");
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
    fn scoped_largest_files_are_selected_from_descendants() {
        let mut tree = FileTree::with_root("C:\\");
        let folder = dir_entry(&mut tree, "folder");
        let nested = dir_entry(&mut tree, "nested");
        let root_file = file_entry(&mut tree, "root.bin", 100);
        let file_a = file_entry(&mut tree, "a.bin", 10);
        let file_b = file_entry(&mut tree, "b.bin", 30);
        let file_c = file_entry(&mut tree, "c.bin", 20);
        tree.attach_child(tree.root_id(), folder);
        tree.attach_child(tree.root_id(), root_file);
        tree.attach_child(folder, file_a);
        tree.attach_child(folder, nested);
        tree.attach_child(nested, file_b);
        tree.attach_child(nested, file_c);
        tree.aggregate_sizes();
        tree.rebuild_largest_files();

        let page = tree.get_largest_files(folder, 0, 2);

        assert_eq!(page.len(), 2);
        assert_eq!(page[0].name, "b.bin");
        assert_eq!(page[1].name, "c.bin");
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
    fn file_path_rows_return_only_requested_ids() {
        let mut tree = FileTree::with_root("C:\\");
        let folder = dir_entry(&mut tree, "folder");
        let file_a = file_entry(&mut tree, "a.txt", 5);
        let file_b = file_entry(&mut tree, "b.txt", 6);
        tree.attach_child(tree.root_id(), folder);
        tree.attach_child(folder, file_a);
        tree.attach_child(folder, file_b);

        let rows = tree.get_file_paths(&[file_b, 999, file_a]);

        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].id, file_b);
        assert_eq!(rows[0].path, "C:\\folder\\b.txt");
        assert_eq!(rows[1].id, file_a);
        assert_eq!(rows[1].path, "C:\\folder\\a.txt");
    }

    #[test]
    fn root_directory_records_are_normalized_to_the_virtual_root() {
        let mut tree = FileTree::with_root("C:\\");
        let top_level = dir_entry(&mut tree, "Users");
        let mut lookup = vec![u32::MAX; 6];
        let root_id = tree.root_id();
        lookup[5] = root_id;

        scan::mft::link_mft_entries(&mut tree, &[(top_level, 5)], &lookup, root_id);

        assert_eq!(
            tree.entries[top_level as usize].parent_index,
            tree.root_id()
        );
    }

    #[test]
    fn ntfs_plus_elevated_chooses_mft_mode() {
        let result = plan_elevated_prepare_result(Ok(1_000_000));

        assert_eq!(result.action, PrepareScanAction::Scan);
        assert_eq!(result.mode, Some(ScanMode::Mft));
        assert_eq!(result.fallback_reason, None);
        assert_eq!(result.total_items_estimate, Some(1_000_000));
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
    fn treemap_inputs_group_the_overflow_tail() {
        let mut tree = FileTree::with_root("C:\\");
        let file_a = file_entry(&mut tree, "a.bin", 100);
        let file_b = file_entry(&mut tree, "b.bin", 80);
        let file_c = file_entry(&mut tree, "c.bin", 20);
        tree.attach_child(tree.root_id(), file_c);
        tree.attach_child(tree.root_id(), file_b);
        tree.attach_child(tree.root_id(), file_a);

        let inputs = build_treemap_inputs(&tree, tree.root_id(), 2);

        assert_eq!(inputs.len(), 2);
        assert_eq!(inputs[0].label, "a.bin");
        assert_eq!(inputs[0].kind, LayoutRectKind::Node);
        assert_eq!(inputs[1].label, "Other");
        assert_eq!(inputs[1].kind, LayoutRectKind::Overflow);
        assert_eq!(inputs[1].value, 100);
    }

    #[test]
    fn treemap_layout_recurses_into_large_directories() {
        let mut tree = FileTree::with_root("C:\\");
        let folder = dir_entry(&mut tree, "folder");
        let nested_a = file_entry(&mut tree, "nested-a.bin", 70);
        let nested_b = file_entry(&mut tree, "nested-b.bin", 30);
        let sibling = file_entry(&mut tree, "sibling.bin", 10);

        tree.attach_child(tree.root_id(), sibling);
        tree.attach_child(tree.root_id(), folder);
        tree.attach_child(folder, nested_a);
        tree.attach_child(folder, nested_b);
        tree.aggregate_sizes();

        let layout = build_recursive_treemap(
            &tree,
            tree.root_id(),
            Rect {
                x: 0.0,
                y: 0.0,
                w: 800.0,
                h: 600.0,
            },
            16,
        );

        assert!(layout.iter().any(|rect| rect.id == Some(folder)));
        assert!(layout.iter().any(|rect| rect.id == Some(nested_a)));
        assert!(layout.iter().any(|rect| rect.id == Some(nested_b)));
    }

    #[test]
    fn treemap_cache_returns_scaled_layout_for_the_same_bucket() {
        let key = LayoutCacheKey {
            root_id: 1,
            width_bucket: dimension_bucket(401.0),
            height_bucket: dimension_bucket(401.0),
        };
        let mut cache = TreemapLayoutCache::new(4);
        let cached_layout = vec![LayoutRect {
            id: Some(7),
            kind: LayoutRectKind::Node,
            label: "node".to_string(),
            x: 0.0,
            y: 0.0,
            w: 200.0,
            h: 100.0,
        }];
        cache.insert(key.clone(), 400.0, 400.0, cached_layout);

        let layout = cache.get(&key, 416.0, 416.0).unwrap();

        assert_eq!(layout.len(), 1);
        assert_eq!(layout[0].label, "node");
        assert!((layout[0].w - 208.0).abs() < 0.1);
        assert!((layout[0].h - 104.0).abs() < 0.1);
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
