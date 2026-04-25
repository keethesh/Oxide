use super::{add_entry, emit_progress};
use crate::core::file_entry::FileFlags;
use crate::core::file_tree::FileTree;
use crate::scan::progress::ScanProgress;
use std::fs;
use std::os::windows::fs::MetadataExt;
use std::path::PathBuf;
use std::time::Instant;
use tauri::Window;
use windows::Win32::Storage::FileSystem::{
    FILE_ATTRIBUTE_HIDDEN, FILE_ATTRIBUTE_READONLY, FILE_ATTRIBUTE_REPARSE_POINT,
    FILE_ATTRIBUTE_SYSTEM,
};

pub fn scan(
    root_path: PathBuf,
    window: &Window,
    progress: &mut ScanProgress,
    started_at: Instant,
) -> Result<FileTree, String> {
    let root_name = root_path.to_string_lossy().to_string();
    let mut tree = FileTree::with_root(&root_name);
    let mut stack = vec![(root_path, tree.root_id())];

    while let Some((path, parent_id)) = stack.pop() {
        progress.phase = format!("Walking {}", path.display());
        emit_progress(window, progress, started_at);

        let entries = match fs::read_dir(&path) {
            Ok(entries) => entries,
            Err(_) => continue,
        };

        for entry in entries.flatten() {
            let child_path = entry.path();
            let metadata = match fs::symlink_metadata(&child_path) {
                Ok(metadata) => metadata,
                Err(_) => continue,
            };

            if metadata.file_type().is_symlink() {
                continue;
            }

            let name = entry.file_name().to_string_lossy().to_string();
            let attributes = metadata.file_attributes();
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

            if progress.files_scanned.saturating_add(progress.dirs_scanned) % 512 == 0 {
                emit_progress(window, progress, started_at);
            }
        }
    }

    Ok(tree)
}
