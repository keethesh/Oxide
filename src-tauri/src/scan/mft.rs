use super::{add_entry, emit_progress};
use crate::core::file_tree::FileTree;
use crate::mft::{parser, reader, volume};
use crate::scan::progress::ScanProgress;
use crate::scan::types::FallbackReason;
use rayon::prelude::*;
use std::collections::HashMap;
use std::fmt;
use std::io::ErrorKind;
use std::time::Instant;
use tauri::Window;
use windows::core::HRESULT;
use windows::Win32::Foundation::ERROR_ACCESS_DENIED;

const PROBE_RECORDS: u32 = 16;
const PROBE_TIMEOUT_MS: u32 = 2_000;
const CHUNK_RECORDS: u32 = 4_096;
const CHUNK_TIMEOUT_MS: u32 = 10_000;
const ROOT_RECORD_ID: u64 = 5;

#[derive(Debug, Clone)]
pub struct MftScanError {
    pub reason: FallbackReason,
    pub message: String,
}

impl MftScanError {
    fn new(reason: FallbackReason, message: impl Into<String>) -> Self {
        Self {
            reason,
            message: message.into(),
        }
    }
}

impl fmt::Display for MftScanError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for MftScanError {}

pub fn probe(drive_letter: char) -> Result<(), MftScanError> {
    let volume = open_volume(drive_letter)?;
    let record_size = volume.record_size();
    let buffer = reader::read_mft_chunk(&volume, 0, PROBE_RECORDS, record_size, PROBE_TIMEOUT_MS)
        .map_err(|err| map_read_error(err, true))?;

    let valid_records = buffer
        .chunks(record_size)
        .enumerate()
        .filter_map(|(offset, record)| parser::parse_record(record, offset as u64))
        .count();

    if valid_records == 0 {
        return Err(MftScanError::new(
            FallbackReason::MftParseError,
            "MFT probe did not yield any valid records",
        ));
    }

    Ok(())
}

pub fn scan(
    drive_letter: char,
    window: &Window,
    progress: &mut ScanProgress,
    started_at: Instant,
) -> Result<FileTree, MftScanError> {
    let volume = open_volume(drive_letter)?;
    let root_name = format!("{}:\\", drive_letter.to_ascii_uppercase());
    let record_size = volume.record_size();
    let total_records = volume.total_records().max(PROBE_RECORDS as u64);

    let mut tree = FileTree::with_root(&root_name);
    let root_id = tree.root_id();
    let mut mft_to_index = HashMap::new();
    let mut parent_links = Vec::new();
    let mut parsed_records = 0usize;

    mft_to_index.insert(ROOT_RECORD_ID, root_id);

    let mut start_record = 0u64;
    while start_record < total_records {
        let remaining = total_records - start_record;
        let records_this_chunk = remaining.min(CHUNK_RECORDS as u64) as u32;
        let chunk_end = start_record + u64::from(records_this_chunk);

        progress.phase = format!("Reading MFT records {}-{}", start_record + 1, chunk_end);
        emit_progress(window, progress, started_at);

        let buffer = reader::read_mft_chunk(
            &volume,
            start_record,
            records_this_chunk,
            record_size,
            CHUNK_TIMEOUT_MS,
        )
        .map_err(|err| map_read_error(err, false))?;

        progress.phase = format!("Parsing MFT records {}-{}", start_record + 1, chunk_end);
        emit_progress(window, progress, started_at);

        let parsed_entries: Vec<_> = buffer
            .par_chunks(record_size)
            .enumerate()
            .filter_map(|(offset, record)| {
                let record_id = start_record + offset as u64;
                parser::parse_record(record, record_id)
            })
            .collect();

        for entry in parsed_entries {
            if entry.id == ROOT_RECORD_ID {
                continue;
            }

            let node_id = add_entry(&mut tree, &entry.name, entry.size, entry.is_dir);
            mft_to_index.insert(entry.id, node_id);
            parent_links.push((node_id, entry.parent_id));
            parsed_records += 1;

            if entry.is_dir {
                progress.dirs_scanned = progress.dirs_scanned.saturating_add(1);
            } else {
                progress.files_scanned = progress.files_scanned.saturating_add(1);
                progress.bytes_scanned = progress.bytes_scanned.saturating_add(entry.size);
            }
        }

        emit_progress(window, progress, started_at);
        start_record = chunk_end;
    }

    if parsed_records == 0 {
        return Err(MftScanError::new(
            FallbackReason::MftParseError,
            "MFT scan did not yield any valid records",
        ));
    }

    link_mft_entries(&mut tree, &parent_links, &mft_to_index, root_id);

    Ok(tree)
}

pub(crate) fn link_mft_entries(
    tree: &mut FileTree,
    parent_links: &[(u32, u64)],
    mft_to_index: &HashMap<u64, u32>,
    root_id: u32,
) {
    for (child_index, parent_mft_id) in parent_links.iter().copied() {
        let mut parent_index = mft_to_index.get(&parent_mft_id).copied().unwrap_or(root_id);
        if parent_index == child_index {
            parent_index = root_id;
        }
        tree.attach_child(parent_index, child_index);
    }
}

fn open_volume(drive_letter: char) -> Result<volume::VolumeHandle, MftScanError> {
    volume::open_volume(drive_letter).map_err(map_volume_error)
}

fn map_volume_error(err: windows::core::Error) -> MftScanError {
    let reason = if err.code() == HRESULT::from_win32(ERROR_ACCESS_DENIED.0) {
        FallbackReason::MftAccessDenied
    } else {
        FallbackReason::MftReadError
    };

    MftScanError::new(reason, format!("Failed to open volume: {err}"))
}

fn map_read_error(err: std::io::Error, is_probe: bool) -> MftScanError {
    let reason = match err.kind() {
        ErrorKind::TimedOut if is_probe => FallbackReason::MftProbeTimeout,
        ErrorKind::PermissionDenied => FallbackReason::MftAccessDenied,
        _ => FallbackReason::MftReadError,
    };

    MftScanError::new(reason, format!("Failed to read MFT: {err}"))
}
