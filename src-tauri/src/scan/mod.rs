pub mod filesystem;
pub mod mft;
pub mod progress;
pub mod types;

use crate::core::file_entry::{FileEntry, FileFlags};
use crate::core::file_tree::FileTree;
use progress::ScanProgress;
use std::time::Instant;
use tauri::{Emitter, Window};

pub(crate) fn add_entry(tree: &mut FileTree, name: &str, size: u64, is_dir: bool) -> u32 {
    let (name_offset, name_len) = tree.names.push(name);
    let mut flags = 0;
    if is_dir {
        flags |= FileFlags::Directory as u16;
    }

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

pub(crate) fn emit_progress(window: &Window, progress: &mut ScanProgress, started_at: Instant) {
    progress.duration_ms = started_at.elapsed().as_millis().min(u64::MAX as u128) as u64;
    let _ = window.emit("scan-progress", progress.clone());
}
