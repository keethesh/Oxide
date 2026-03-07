export interface DriveInfo {
  letter: string;
  label: string;
  filesystem: string;
  supported: boolean;
}

export interface ScanProgress {
  files_scanned: number;
  dirs_scanned: number;
  bytes_scanned: number;
  phase: string;
  done: boolean;
}

export interface ScanResult {
  root_id: number;
  drive_letter: string;
  files_scanned: number;
  dirs_scanned: number;
  bytes_scanned: number;
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
