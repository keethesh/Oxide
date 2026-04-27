export interface DriveInfo {
  letter: string;
  label: string;
  filesystem: string;
  supported: boolean;
  total_bytes?: number;
  free_bytes?: number;
}

export interface PrepareScanResult {
  action: "Scan" | "Relaunching";
  mode?: "Mft" | "Filesystem";
  fallback_reason?: string;
  pending_drive?: string;
  total_items_estimate?: number;
}

export interface ScanProgress {
  files_scanned: number;
  dirs_scanned: number;
  bytes_scanned: number;
  phase: string;
  done: boolean;
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
  duration_ms: number;
  timings: ScanTimings;
}

export interface NodeSummary {
  id: number;
  name: string;
  is_dir: boolean;
  size: number;
  child_count: number;
}

export interface FileRow {
  id: number;
  name: string;
  size: number;
  path: string;
  parent_id: number;
}

export type Breadcrumb = [number, string];

export function formatBytes(bytes: number): string {
  if (bytes <= 0) {
    return "0 B";
  }

  const units = ["B", "KB", "MB", "GB", "TB"];
  const index = Math.min(Math.floor(Math.log(bytes) / Math.log(1024)), units.length - 1);
  const value = bytes / 1024 ** index;
  return `${value >= 10 || index === 0 ? value.toFixed(0) : value.toFixed(1)} ${units[index]}`;
}
