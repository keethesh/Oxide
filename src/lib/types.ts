export type ScanMode = "mft" | "filesystem";

export type FallbackReason =
  | "uac_declined"
  | "mft_probe_timeout"
  | "mft_read_error"
  | "mft_parse_error"
  | "mft_access_denied"
  | "scan_cancelled";

export interface DriveInfo {
  letter: string;
  label: string;
  filesystem: string;
  supported: boolean;
  total_bytes: number;
  free_bytes: number;
}

export interface PrepareScanResult {
  action: "scan" | "relaunching";
  mode: ScanMode | null;
  fallback_reason: FallbackReason | null;
  pending_drive: string | null;
  total_items_estimate: number | null;
}

export interface LaunchScanRequest {
  drive_letter: string | null;
}

export interface ScanProgress {
  files_scanned: number;
  dirs_scanned: number;
  bytes_scanned: number;
  phase: string;
  done: boolean;
  scan_mode: ScanMode | null;
  fallback_reason: FallbackReason | null;
  duration_ms: number;
}

export interface ScanTimings {
  scan_ms: number;
  aggregate_ms: number;
  largest_files_ms: number;
  store_ms: number;
  total_ms: number;
}

export interface ScanResult {
  root_id: number;
  drive_letter: string;
  files_scanned: number;
  dirs_scanned: number;
  bytes_scanned: number;
  scan_mode: ScanMode;
  fallback_reason: FallbackReason | null;
  duration_ms: number;
  timings: ScanTimings;
}

export interface NodeSummary {
  id: number;
  name: string;
  is_dir: boolean;
  is_hidden: boolean;
  size: number;
  child_count: number;
}

export interface ChildPage {
  items: NodeSummary[];
  total: number;
  next_offset: number | null;
}

export interface FileRow {
  id: number;
  name: string;
  size: number;
  parent_id: number;
  is_hidden: boolean;
}

export interface FilePathRow {
  id: number;
  path: string;
}

export type TreemapRectKind = "node" | "overflow";

export interface TreemapRect {
  id: number | null;
  kind: TreemapRectKind;
  label: string;
  x: number;
  y: number;
  w: number;
  h: number;
}
