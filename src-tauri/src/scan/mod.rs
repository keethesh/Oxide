pub mod filesystem;
pub mod mft;
pub mod progress;
pub mod types;

use crate::core::file_entry::FileEntry;
use crate::core::file_tree::FileTree;
use progress::ScanProgress;
use std::time::Instant;

pub(crate) fn add_entry(tree: &mut FileTree, name: &str, size: u64, flags: u16) -> u32 {
    let (name_offset, name_len) = tree.names.push(name);

    tree.add_entry(FileEntry {
        size,
        parent_index: FileEntry::NULL_INDEX,
        first_child_index: FileEntry::NULL_INDEX,
        next_sibling_index: FileEntry::NULL_INDEX,
        name_offset,
        name_len,
        flags,
    })
}

/// Trait for receiving scan progress updates.
/// Implemented by both the Tauri window emitter and the CLI profiler.
pub trait ProgressSink {
    fn emit(&mut self, progress: &ScanProgress);
}

/// Tauri-window-based progress sink for the GUI scan flow.
pub(crate) struct WindowProgressSink<'a> {
    window: &'a tauri::Window,
    started_at: Instant,
}

impl<'a> WindowProgressSink<'a> {
    pub fn new(window: &'a tauri::Window, started_at: Instant) -> Self {
        Self { window, started_at }
    }
}

impl ProgressSink for WindowProgressSink<'_> {
    fn emit(&mut self, progress: &mut ScanProgress) {
        progress.duration_ms = self.started_at.elapsed().as_millis().min(u64::MAX as u128) as u64;
        let _ = self.window.emit("scan-progress", progress.clone());
    }
}

/// Silent progress sink that does nothing (used for CLI profiling).
pub struct SilentProgressSink;

impl ProgressSink for SilentProgressSink {
    fn emit(&mut self, _progress: &mut ScanProgress) {}
}
