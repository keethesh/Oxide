use super::{add_entry, emit_progress};
use crate::core::file_tree::FileTree;
use crate::mft::{parser, sector_reader::SectorReader, volume};
use crate::scan::progress::ScanProgress;
use crate::scan::types::FallbackReason;
use ntfs::Ntfs;
use rayon::prelude::*;
use std::collections::{HashMap, HashSet};
use std::fmt;
use std::fs::OpenOptions;
use std::io::{BufReader, ErrorKind, Read};
use std::os::windows::fs::OpenOptionsExt;
use std::time::Instant;
use tauri::Window;
use windows::core::HRESULT;
use windows::Win32::Foundation::ERROR_ACCESS_DENIED;
use windows::Win32::Storage::FileSystem::{
    FILE_FLAG_SEQUENTIAL_SCAN, FILE_SHARE_DELETE, FILE_SHARE_READ, FILE_SHARE_WRITE,
};

const PROBE_RECORDS: u32 = 16;
const CHUNK_RECORDS: u32 = 16_384;
const MFT_RECORD_ID: u64 = 0;
const ROOT_RECORD_ID: u64 = 5;
const METADATA_RECORD_IDS: [u64; 10] = [0, 1, 2, 3, 4, 6, 7, 8, 9, 10];
const EXTEND_RECORD_ID: u64 = 11;

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
    let sector_size = volume.info.BytesPerSector.max(1) as usize;
    let mut fs = open_volume_reader(drive_letter, sector_size)?;
    let ntfs = Ntfs::new(&mut fs).map_err(map_ntfs_error)?;
    let mft_file = ntfs.file(&mut fs, MFT_RECORD_ID).map_err(map_ntfs_error)?;
    let mft_data_item = mft_file
        .data(&mut fs, "")
        .ok_or_else(|| {
            MftScanError::new(
                FallbackReason::MftParseError,
                "The $MFT data stream is missing",
            )
        })?
        .map_err(map_ntfs_error)?;
    let mft_data_attribute = mft_data_item.to_attribute().map_err(map_ntfs_error)?;
    let mft_data_value = mft_data_attribute.value(&mut fs).map_err(map_ntfs_error)?;
    let record_size = ntfs.file_record_size() as usize;
    let total_records = total_mft_records(&volume, record_size, mft_data_value.len());
    let probe_records = total_records.min(PROBE_RECORDS as u64) as usize;

    if probe_records == 0 {
        return Err(MftScanError::new(
            FallbackReason::MftParseError,
            "The $MFT data stream is empty",
        ));
    }

    let mut mft_stream = mft_data_value.attach(&mut fs);
    let mut buffer = vec![0u8; probe_records * record_size];
    mft_stream
        .read_exact(&mut buffer)
        .map_err(|err| map_read_error(err, true))?;

    let valid_records = buffer
        .chunks(record_size)
        .enumerate()
        .filter_map({
            let mut scratch = Vec::with_capacity(record_size);
            move |(offset, record)| {
                parser::parse_record_with_scratch(record, offset as u64, &mut scratch)
            }
        })
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
    let sector_size = volume.info.BytesPerSector.max(1) as usize;
    let mut fs = open_volume_reader(drive_letter, sector_size)?;
    let ntfs = Ntfs::new(&mut fs).map_err(map_ntfs_error)?;
    let mft_file = ntfs.file(&mut fs, MFT_RECORD_ID).map_err(map_ntfs_error)?;
    let mft_data_item = mft_file
        .data(&mut fs, "")
        .ok_or_else(|| {
            MftScanError::new(
                FallbackReason::MftParseError,
                "The $MFT data stream is missing",
            )
        })?
        .map_err(map_ntfs_error)?;
    let mft_data_attribute = mft_data_item.to_attribute().map_err(map_ntfs_error)?;
    let mft_data_value = mft_data_attribute.value(&mut fs).map_err(map_ntfs_error)?;

    let root_name = format!("{}:\\", drive_letter.to_ascii_uppercase());
    let record_size = ntfs.file_record_size() as usize;
    let total_records = total_mft_records(&volume, record_size, mft_data_value.len());
    if total_records == 0 {
        return Err(MftScanError::new(
            FallbackReason::MftParseError,
            "The $MFT data stream is empty",
        ));
    }

    let mut tree = FileTree::with_root(&root_name);
    let root_id = tree.root_id();
    let mut mft_to_index = HashMap::new();
    let mut metadata_record_ids = HashSet::from(METADATA_RECORD_IDS);
    metadata_record_ids.insert(EXTEND_RECORD_ID);
    let mut parent_links = Vec::new();
    let mut parsed_records = 0usize;
    let mut mft_stream = mft_data_value.attach(&mut fs);
    let mut buffer = vec![0u8; CHUNK_RECORDS as usize * record_size];

    mft_to_index.insert(ROOT_RECORD_ID, root_id);

    let mut start_record = 0u64;
    while start_record < total_records {
        let remaining = total_records - start_record;
        let records_this_chunk = remaining.min(CHUNK_RECORDS as u64) as usize;
        let chunk_end = start_record + records_this_chunk as u64;

        progress.phase = format!("Reading MFT records {}-{}", start_record + 1, chunk_end);
        emit_progress(window, progress, started_at);

        buffer.resize(records_this_chunk * record_size, 0);
        mft_stream
            .read_exact(&mut buffer)
            .map_err(|err| map_read_error(err, false))?;

        progress.phase = format!("Parsing MFT records {}-{}", start_record + 1, chunk_end);
        emit_progress(window, progress, started_at);

        let parsed_entries: Vec<_> = buffer
            .par_chunks(record_size)
            .enumerate()
            .map_init(
                || Vec::with_capacity(record_size),
                |scratch, (offset, record)| {
                    let record_id = start_record + offset as u64;
                    parser::parse_record_with_scratch(record, record_id, scratch)
                },
            )
            .filter_map(|entry| entry)
            .collect();

        for entry in parsed_entries {
            if entry.id == ROOT_RECORD_ID {
                continue;
            }

            if should_skip_mft_entry(&entry, &metadata_record_ids) {
                metadata_record_ids.insert(entry.id);
                continue;
            }

            let node_id = add_entry(&mut tree, &entry.name, entry.size, entry.flags);
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

fn open_volume_reader(
    drive_letter: char,
    sector_size: usize,
) -> Result<BufReader<SectorReader<std::fs::File>>, MftScanError> {
    let path = format!("\\\\.\\{}:", drive_letter.to_ascii_uppercase());
    let file = OpenOptions::new()
        .read(true)
        .share_mode(FILE_SHARE_READ.0 | FILE_SHARE_WRITE.0 | FILE_SHARE_DELETE.0)
        .custom_flags(FILE_FLAG_SEQUENTIAL_SCAN.0)
        .open(path)
        .map_err(|err| map_read_error(err, false))?;
    let sector_reader =
        SectorReader::new(file, sector_size).map_err(|err| map_read_error(err, false))?;
    Ok(BufReader::new(sector_reader))
}

fn total_mft_records(volume: &volume::VolumeHandle, record_size: usize, stream_len: u64) -> u64 {
    if record_size == 0 {
        return 0;
    }

    let volume_records = volume.total_records();
    let stream_records = stream_len / record_size as u64;

    match (volume_records, stream_records) {
        (0, 0) => 0,
        (0, stream_records) => stream_records,
        (volume_records, 0) => volume_records,
        (volume_records, stream_records) => volume_records.min(stream_records),
    }
}

fn should_skip_mft_entry(entry: &parser::MftEntry, metadata_record_ids: &HashSet<u64>) -> bool {
    entry.name == "."
        || entry.name == ".."
        || entry.id == EXTEND_RECORD_ID
        || METADATA_RECORD_IDS.contains(&entry.id)
        || metadata_record_ids.contains(&entry.parent_id)
}

fn map_volume_error(err: windows::core::Error) -> MftScanError {
    let reason = if err.code() == HRESULT::from_win32(ERROR_ACCESS_DENIED.0) {
        FallbackReason::MftAccessDenied
    } else {
        FallbackReason::MftReadError
    };

    MftScanError::new(reason, format!("Failed to open volume: {err}"))
}

fn map_ntfs_error(err: ntfs::NtfsError) -> MftScanError {
    MftScanError::new(
        FallbackReason::MftReadError,
        format!("Failed to read NTFS volume: {err}"),
    )
}

fn map_read_error(err: std::io::Error, is_probe: bool) -> MftScanError {
    let reason = match err.kind() {
        ErrorKind::TimedOut if is_probe => FallbackReason::MftProbeTimeout,
        ErrorKind::PermissionDenied => FallbackReason::MftAccessDenied,
        _ => FallbackReason::MftReadError,
    };

    MftScanError::new(reason, format!("Failed to read MFT: {err}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn skips_known_ntfs_metadata_records() {
        let metadata_record_ids = HashSet::from(METADATA_RECORD_IDS);
        let badclus = parser::MftEntry {
            id: 8,
            parent_id: ROOT_RECORD_ID,
            size: 0,
            name: "$BadClus".to_string(),
            is_dir: false,
            flags: 0,
        };

        assert!(should_skip_mft_entry(&badclus, &metadata_record_ids));
    }

    #[test]
    fn keeps_regular_root_system_folders() {
        let metadata_record_ids = HashSet::from(METADATA_RECORD_IDS);
        let recycle_bin = parser::MftEntry {
            id: 42,
            parent_id: ROOT_RECORD_ID,
            size: 0,
            name: "$Recycle.Bin".to_string(),
            is_dir: true,
            flags: 0,
        };

        assert!(!should_skip_mft_entry(&recycle_bin, &metadata_record_ids));
    }

    #[test]
    fn skips_metadata_descendants_under_extend() {
        let metadata_record_ids = HashSet::from([EXTEND_RECORD_ID, 24]);
        let usn_journal_stream = parser::MftEntry {
            id: 48,
            parent_id: 24,
            size: 0,
            name: "$J".to_string(),
            is_dir: false,
            flags: 0,
        };

        assert!(should_skip_mft_entry(
            &usn_journal_stream,
            &metadata_record_ids
        ));
    }
}
