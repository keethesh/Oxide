use super::{add_entry, ProgressSink};
use crate::core::file_entry::FileFlags;
use crate::core::file_tree::FileTree;
use crate::scan::progress::ScanProgress;
use crate::scan::types::ScanSkipCounts;
use std::fs;
use std::os::windows::fs::MetadataExt;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use windows::Win32::Storage::FileSystem::{
    FILE_ATTRIBUTE_HIDDEN, FILE_ATTRIBUTE_READONLY, FILE_ATTRIBUTE_REPARSE_POINT,
    FILE_ATTRIBUTE_SYSTEM,
};

const PROGRESS_NODE_INTERVAL: u64 = 512;
const PROGRESS_EMIT_INTERVAL: Duration = Duration::from_millis(125);

pub fn scan(
    root_path: PathBuf,
    sink: &mut dyn ProgressSink,
    progress: &mut ScanProgress,
    _started_at: Instant,
    cancel_flag: &Arc<AtomicBool>,
) -> Result<FileSystemScan, String> {
    if cancel_flag.load(Ordering::SeqCst) {
        return Err("Scan cancelled".to_string());
    }

    let root_name = root_path.to_string_lossy().to_string();
    let mut tree = FileTree::with_root(&root_name);
    let mut skipped = ScanSkipCounts::default();
    let mut stack = vec![(root_path, tree.root_id())];
    let mut last_progress_emit_at = Instant::now();

    while let Some((path, parent_id)) = stack.pop() {
        if cancel_flag.load(Ordering::SeqCst) {
            return Err("Scan cancelled".to_string());
        }

        if should_emit_progress(&mut last_progress_emit_at) {
            progress.phase = format!("Walking {}", path.display());
            sink.emit(progress);
        }

        let entries = match fs::read_dir(&path) {
            Ok(entries) => entries,
            Err(_) => {
                skipped.errors_skipped = skipped.errors_skipped.saturating_add(1);
                continue;
            }
        };

        for entry in entries {
            if cancel_flag.load(Ordering::SeqCst) {
                return Err("Scan cancelled".to_string());
            }

            let entry = match entry {
                Ok(entry) => entry,
                Err(_) => {
                    skipped.errors_skipped = skipped.errors_skipped.saturating_add(1);
                    continue;
                }
            };
            let child_path = entry.path();
            let metadata = match fs::symlink_metadata(&child_path) {
                Ok(metadata) => metadata,
                Err(_) => {
                    skipped.errors_skipped = skipped.errors_skipped.saturating_add(1);
                    continue;
                }
            };

            let attributes = metadata.file_attributes();
            if metadata.file_type().is_symlink() {
                increment_skipped(&mut skipped, metadata.is_dir());
                continue;
            }
            if should_skip_reparse_directory(metadata.is_dir(), attributes) {
                skipped.dirs_skipped = skipped.dirs_skipped.saturating_add(1);
                continue;
            }

            let name = entry.file_name().to_string_lossy().to_string();
            let mut flags = 0u16;
            if metadata.is_dir() {
                flags |= FileFlags::Directory as u16;
            }
            if (attributes & FILE_ATTRIBUTE_READONLY.0) != 0 {
                flags |= FileFlags::ReadOnly as u16;
            }
            if (attributes & FILE_ATTRIBUTE_HIDDEN.0) != 0 {
                flags |= FileFlags::Hidden as u16;
            }
            if (attributes & FILE_ATTRIBUTE_SYSTEM.0) != 0 {
                flags |= FileFlags::System as u16;
            }
            if (attributes & FILE_ATTRIBUTE_REPARSE_POINT.0) != 0 {
                flags |= FileFlags::Reparse as u16;
            }
            if metadata.is_dir() {
                let child_id = add_entry(&mut tree, &name, 0, flags);
                tree.attach_child(parent_id, child_id);
                progress.dirs_scanned = progress.dirs_scanned.saturating_add(1);
                stack.push((child_path, child_id));
            } else {
                let child_id = add_entry(&mut tree, &name, metadata.len(), flags);
                tree.attach_child(parent_id, child_id);
                progress.files_scanned = progress.files_scanned.saturating_add(1);
                progress.bytes_scanned = progress.bytes_scanned.saturating_add(metadata.len());
            }

            if progress
                .files_scanned
                .saturating_add(progress.dirs_scanned)
                .is_multiple_of(PROGRESS_NODE_INTERVAL)
                && should_emit_progress(&mut last_progress_emit_at)
            {
                sink.emit(progress);
            }
        }
    }

    Ok(FileSystemScan { tree, skipped })
}

pub struct FileSystemScan {
    pub tree: FileTree,
    pub skipped: ScanSkipCounts,
}

pub(crate) fn should_skip_reparse_directory(is_dir: bool, attributes: u32) -> bool {
    is_dir && (attributes & FILE_ATTRIBUTE_REPARSE_POINT.0) != 0
}

fn increment_skipped(skipped: &mut ScanSkipCounts, is_dir: bool) {
    if is_dir {
        skipped.dirs_skipped = skipped.dirs_skipped.saturating_add(1);
    } else {
        skipped.files_skipped = skipped.files_skipped.saturating_add(1);
    }
}

fn should_emit_progress(last_progress_emit_at: &mut Instant) -> bool {
    if last_progress_emit_at.elapsed() < PROGRESS_EMIT_INTERVAL {
        return false;
    }

    *last_progress_emit_at = Instant::now();
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reparse_directories_are_skipped() {
        assert!(should_skip_reparse_directory(
            true,
            FILE_ATTRIBUTE_REPARSE_POINT.0
        ));
        assert!(!should_skip_reparse_directory(
            false,
            FILE_ATTRIBUTE_REPARSE_POINT.0
        ));
        assert!(!should_skip_reparse_directory(true, 0));
    }

    #[test]
    fn skipped_counts_track_file_dir_kinds() {
        let mut skipped = ScanSkipCounts::default();

        increment_skipped(&mut skipped, true);
        increment_skipped(&mut skipped, false);

        assert_eq!(skipped.dirs_skipped, 1);
        assert_eq!(skipped.files_skipped, 1);
        assert_eq!(skipped.errors_skipped, 0);
    }
}
