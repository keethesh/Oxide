use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum ScanMode {
    Mft,
    Filesystem,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum FallbackReason {
    UacDeclined,
    MftProbeTimeout,
    MftReadError,
    MftParseError,
    MftAccessDenied,
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum PrepareScanAction {
    Scan,
    Relaunching,
}

#[derive(Debug, Clone, Serialize)]
pub struct PrepareScanResult {
    pub action: PrepareScanAction,
    pub mode: Option<ScanMode>,
    pub fallback_reason: Option<FallbackReason>,
    pub pending_drive: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct LaunchScanRequest {
    pub drive_letter: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ScanResult {
    pub root_id: u32,
    pub drive_letter: String,
    pub files_scanned: u64,
    pub dirs_scanned: u64,
    pub bytes_scanned: u64,
    pub scan_mode: ScanMode,
    pub fallback_reason: Option<FallbackReason>,
    pub duration_ms: u64,
    pub timings: ScanTimings,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct ScanTimings {
    pub scan_ms: u64,
    pub aggregate_ms: u64,
    pub largest_files_ms: u64,
    pub store_ms: u64,
    pub total_ms: u64,
}

impl PrepareScanResult {
    pub fn scan(mode: ScanMode, fallback_reason: Option<FallbackReason>) -> Self {
        Self {
            action: PrepareScanAction::Scan,
            mode: Some(mode),
            fallback_reason,
            pending_drive: None,
        }
    }

    pub fn relaunching(drive_letter: String) -> Self {
        Self {
            action: PrepareScanAction::Relaunching,
            mode: None,
            fallback_reason: None,
            pending_drive: Some(drive_letter),
        }
    }
}
