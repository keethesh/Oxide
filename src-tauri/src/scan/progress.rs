use super::types::{FallbackReason, ScanMode};
use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
pub struct ScanProgress {
    pub files_scanned: u64,
    pub dirs_scanned: u64,
    pub bytes_scanned: u64,
    pub phase: String,
    pub done: bool,
    pub scan_mode: Option<ScanMode>,
    pub fallback_reason: Option<FallbackReason>,
    pub duration_ms: u64,
}

impl ScanProgress {
    pub fn new(phase: &str, scan_mode: Option<ScanMode>) -> Self {
        Self {
            files_scanned: 0,
            dirs_scanned: 0,
            bytes_scanned: 0,
            phase: phase.to_string(),
            done: false,
            scan_mode,
            fallback_reason: None,
            duration_ms: 0,
        }
    }
}
