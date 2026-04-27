use super::{add_entry, emit_progress};
use crate::core::file_tree::FileTree;
use crate::mft::{parser, sector_reader::SectorReader, volume};
use crate::scan::progress::ScanProgress;
use crate::scan::types::FallbackReason;
use ntfs::Ntfs;
use rayon::prelude::*;
use std::fmt;
use std::fs::OpenOptions;
use std::io::{BufReader, ErrorKind, Read};
use std::os::windows::fs::OpenOptionsExt;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Instant;
use tauri::Window;
use windows::core::HRESULT;
use windows::Win32::Foundation::ERROR_ACCESS_DENIED;
use windows::Win32::Storage::FileSystem::{
    FILE_FLAG_SEQUENTIAL_SCAN, FILE_SHARE_DELETE, FILE_SHARE_READ, FILE_SHARE_WRITE,
};

const PROBE_RECORDS: u32 = 16;
const CHUNK_RECORDS: u32 = 16_384;
const PROGRESS_RECORD_INTERVAL: u64 = 262_144;
const MAX_PREALLOCATED_ENTRIES: usize = 8_000_000;
const MFT_RECORD_ID: u64 = 0;
const ROOT_RECORD_ID: u64 = 5;
const METADATA_RECORD_IDS: [u64; 10] = [0, 1, 2, 3, 4, 6, 7, 8, 9, 10];
const EXTEND_RECORD_ID: u64 = 11;
const MFT_INDEX_UNMAPPED: u32 = u32::MAX;

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

pub fn probe(drive_letter: char, _cancel_flag: &Arc<AtomicBool>) -> Result<u64, MftScanError> {
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

    Ok(total_records)
}

pub fn scan(
    drive_letter: char,
    window: &Window,
    progress: &mut ScanProgress,
    started_at: Instant,
    cancel_flag: &Arc<AtomicBool>,
) -> Result<FileTree, MftScanError> {
    if cancel_flag.load(Ordering::SeqCst) {
        return Err(MftScanError::new(FallbackReason::ScanCancelled, "Scan cancelled"));
    }

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
    let total_records_len = usize::try_from(total_records).map_err(|_| {
        MftScanError::new(
            FallbackReason::MftParseError,
            "The $MFT record count exceeds platform limits",
        )
    })?;

    let mut tree = FileTree::with_root_capacity(
        &root_name,
        total_records_len.min(MAX_PREALLOCATED_ENTRIES),
    );
    let root_id = tree.root_id();
    let mut mft_to_index = vec![MFT_INDEX_UNMAPPED; total_records_len];
    let mut metadata_record_ids = MetadataRecordSet::new(total_records_len);
    let mut parent_links =
        Vec::with_capacity(total_records_len.min(MAX_PREALLOCATED_ENTRIES).saturating_sub(1));
    let mut parsed_records = 0usize;
    let mut parse_ms = 0u64;
    let mut ingest_ms = 0u64;
    let mut mft_stream = mft_data_value.attach(&mut fs);
    let mut buffer = vec![0u8; CHUNK_RECORDS as usize * record_size];

    set_mft_index(&mut mft_to_index, ROOT_RECORD_ID, root_id);

    let mut start_record = 0u64;
    let mut last_progress_record = 0u64;
    while start_record < total_records {
        if cancel_flag.load(Ordering::SeqCst) {
            return Err(MftScanError::new(FallbackReason::ScanCancelled, "Scan cancelled"));
        }

        let remaining = total_records - start_record;
        let records_this_chunk = remaining.min(CHUNK_RECORDS as u64) as usize;
        let chunk_end = start_record + records_this_chunk as u64;

        buffer.resize(records_this_chunk * record_size, 0);
        mft_stream
            .read_exact(&mut buffer)
            .map_err(|err| map_read_error(err, false))?;

        let parse_started_at = Instant::now();
        let parsed_entries: Vec<_> = buffer
            .par_chunks(record_size)
            .enumerate()
            .map_init(
                || Vec::with_capacity(record_size),
                |scratch, (offset, record)| {
                    let record_id = start_record + offset as u64;
                    if is_builtin_metadata_record(record_id) {
                        return None;
                    }
                    parser::parse_record_with_scratch(record, record_id, scratch)
                },
            )
            .filter_map(|entry| entry)
            .collect();
        parse_ms = parse_ms.saturating_add(elapsed_ms(parse_started_at));

        let ingest_started_at = Instant::now();
        for entry in parsed_entries {
            if should_skip_mft_entry(&entry, &metadata_record_ids) {
                metadata_record_ids.insert(entry.id);
                continue;
            }

            let node_id = add_entry(&mut tree, &entry.name, entry.size, entry.flags);
            set_mft_index(&mut mft_to_index, entry.id, node_id);
            parent_links.push((node_id, entry.parent_id));
            parsed_records += 1;

            if entry.is_dir {
                progress.dirs_scanned = progress.dirs_scanned.saturating_add(1);
            } else {
                progress.files_scanned = progress.files_scanned.saturating_add(1);
                progress.bytes_scanned = progress.bytes_scanned.saturating_add(entry.size);
            }
        }

        ingest_ms = ingest_ms.saturating_add(elapsed_ms(ingest_started_at));

        if chunk_end == total_records
            || chunk_end.saturating_sub(last_progress_record) >= PROGRESS_RECORD_INTERVAL
        {
            progress.phase = format!("Scanning MFT records {} / {}", chunk_end, total_records);
            emit_progress(window, progress, started_at);
            last_progress_record = chunk_end;
        }
        start_record = chunk_end;
    }

    if parsed_records == 0 {
        return Err(MftScanError::new(
            FallbackReason::MftParseError,
            "MFT scan did not yield any valid records",
        ));
    }

    let link_started_at = Instant::now();
    link_mft_entries(&mut tree, &parent_links, &mft_to_index, root_id);
    let link_ms = elapsed_ms(link_started_at);

    eprintln!(
        "oxide mft profile records={} parsed={} parse_ms={} ingest_ms={} link_ms={}",
        total_records, parsed_records, parse_ms, ingest_ms, link_ms
    );

    Ok(tree)
}

pub(crate) fn link_mft_entries(
    tree: &mut FileTree,
    parent_links: &[(u32, u64)],
    mft_to_index: &[u32],
    root_id: u32,
) {
    for (child_index, parent_mft_id) in parent_links.iter().copied() {
        let mut parent_index = lookup_mft_index(mft_to_index, parent_mft_id).unwrap_or(root_id);
        if parent_index == child_index {
            parent_index = root_id;
        }
        tree.attach_child(parent_index, child_index);
    }
}

fn set_mft_index(mft_to_index: &mut [u32], mft_record_id: u64, node_id: u32) {
    let Some(offset) = mft_record_offset(mft_record_id) else {
        return;
    };
    if let Some(slot) = mft_to_index.get_mut(offset) {
        *slot = node_id;
    }
}

fn lookup_mft_index(mft_to_index: &[u32], mft_record_id: u64) -> Option<u32> {
    let offset = mft_record_offset(mft_record_id)?;
    mft_to_index
        .get(offset)
        .copied()
        .filter(|index| *index != MFT_INDEX_UNMAPPED)
}

fn mft_record_offset(mft_record_id: u64) -> Option<usize> {
    usize::try_from(mft_record_id).ok()
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

fn should_skip_mft_entry(
    entry: &parser::MftEntry,
    metadata_record_ids: &MetadataRecordSet,
) -> bool {
    entry.name == "." || entry.name == ".." || metadata_record_ids.contains(entry.parent_id)
}

fn is_builtin_metadata_record(record_id: u64) -> bool {
    record_id == ROOT_RECORD_ID
        || record_id == EXTEND_RECORD_ID
        || METADATA_RECORD_IDS.contains(&record_id)
}

#[derive(Debug, Clone)]
struct MetadataRecordSet {
    records: Vec<bool>,
}

impl MetadataRecordSet {
    fn new(total_records: usize) -> Self {
        let mut set = Self {
            records: vec![false; total_records],
        };
        for record_id in METADATA_RECORD_IDS {
            set.insert(record_id);
        }
        set.insert(EXTEND_RECORD_ID);
        set
    }

    fn insert(&mut self, record_id: u64) {
        if let Some(record) = self.records.get_mut(record_id as usize) {
            *record = true;
        }
    }

    fn contains(&self, record_id: u64) -> bool {
        self.records
            .get(record_id as usize)
            .copied()
            .unwrap_or(false)
    }
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

fn elapsed_ms(started_at: Instant) -> u64 {
    started_at.elapsed().as_millis().min(u64::MAX as u128) as u64
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
        assert!(is_builtin_metadata_record(8));
        assert!(is_builtin_metadata_record(EXTEND_RECORD_ID));
        assert!(is_builtin_metadata_record(ROOT_RECORD_ID));
    }

    #[test]
    fn keeps_regular_root_system_folders() {
        let metadata_record_ids = MetadataRecordSet::new(64);
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
        let mut metadata_record_ids = MetadataRecordSet::new(64);
        metadata_record_ids.insert(24);
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
