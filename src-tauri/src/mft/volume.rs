use serde::Serialize;
use std::ffi::OsStr;
use std::os::windows::ffi::OsStrExt;
use windows::core::{Result, PCWSTR};
use windows::Win32::Foundation::{CloseHandle, GENERIC_READ, HANDLE, INVALID_HANDLE_VALUE};
use windows::Win32::Storage::FileSystem::{
    CreateFileW, GetDriveTypeW, GetLogicalDriveStringsW, GetVolumeInformationW,
    FILE_FLAG_OVERLAPPED, FILE_FLAG_SEQUENTIAL_SCAN, FILE_SHARE_READ, FILE_SHARE_WRITE,
    OPEN_EXISTING,
};
use windows::Win32::System::Ioctl::{FSCTL_GET_NTFS_VOLUME_DATA, NTFS_VOLUME_DATA_BUFFER};
use windows::Win32::System::IO::DeviceIoControl;

const DRIVE_REMOVABLE_TYPE: u32 = 2;
const DRIVE_FIXED_TYPE: u32 = 3;
const DRIVE_RAMDISK_TYPE: u32 = 6;

#[derive(Debug)]
pub struct VolumeHandle {
    pub handle: HANDLE,
    pub info: NTFS_VOLUME_DATA_BUFFER,
}

#[derive(Debug, Clone, Serialize)]
pub struct DriveInfo {
    pub letter: String,
    pub label: String,
    pub filesystem: String,
    pub supported: bool,
}

// Safety: Windows HANDLEs can be sent between threads.
unsafe impl Send for VolumeHandle {}
unsafe impl Sync for VolumeHandle {}

impl Drop for VolumeHandle {
    fn drop(&mut self) {
        unsafe {
            if self.handle != INVALID_HANDLE_VALUE {
                let _ = CloseHandle(self.handle);
            }
        }
    }
}

impl VolumeHandle {
    pub fn record_size(&self) -> usize {
        self.info.BytesPerFileRecordSegment.max(1) as usize
    }

    pub fn total_records(&self) -> u64 {
        let record_size = self.record_size() as u64;
        let valid_length = self.info.MftValidDataLength.max(0) as u64;
        valid_length.div_ceil(record_size)
    }
}

pub fn open_volume(drive_letter: char) -> Result<VolumeHandle> {
    let path_str = format!("\\\\.\\{}:", drive_letter.to_ascii_uppercase());
    let wide_path = to_wide(&path_str);

    unsafe {
        let handle = CreateFileW(
            PCWSTR(wide_path.as_ptr()),
            GENERIC_READ.0,
            FILE_SHARE_READ | FILE_SHARE_WRITE,
            None,
            OPEN_EXISTING,
            FILE_FLAG_OVERLAPPED | FILE_FLAG_SEQUENTIAL_SCAN,
            None,
        )?;

        if handle == INVALID_HANDLE_VALUE {
            return Err(windows::core::Error::from_thread());
        }

        let mut info: NTFS_VOLUME_DATA_BUFFER = std::mem::zeroed();
        let mut bytes_returned = 0u32;

        DeviceIoControl(
            handle,
            FSCTL_GET_NTFS_VOLUME_DATA,
            None,
            0,
            Some(&mut info as *mut _ as *mut _),
            std::mem::size_of::<NTFS_VOLUME_DATA_BUFFER>() as u32,
            Some(&mut bytes_returned),
            None,
        )?;

        Ok(VolumeHandle { handle, info })
    }
}

pub fn list_drives() -> Vec<DriveInfo> {
    let length = unsafe { GetLogicalDriveStringsW(None) };
    if length == 0 {
        return Vec::new();
    }

    let mut buffer = vec![0u16; length as usize + 1];
    let written = unsafe { GetLogicalDriveStringsW(Some(buffer.as_mut_slice())) };
    if written == 0 {
        return Vec::new();
    }

    let mut drives = Vec::new();
    let mut start = 0usize;
    for index in 0..buffer.len() {
        if buffer[index] != 0 {
            continue;
        }

        if index == start {
            break;
        }

        let root = String::from_utf16_lossy(&buffer[start..index]);
        if let Some(info) = drive_info_from_root(&root) {
            drives.push(info);
        }
        start = index + 1;
    }

    drives.sort_by(|a, b| a.letter.cmp(&b.letter));
    drives
}

fn drive_info_from_root(root: &str) -> Option<DriveInfo> {
    let wide_root = to_wide(root);
    let drive_type = unsafe { GetDriveTypeW(PCWSTR(wide_root.as_ptr())) };
    if !matches!(
        drive_type,
        DRIVE_REMOVABLE_TYPE | DRIVE_FIXED_TYPE | DRIVE_RAMDISK_TYPE
    ) {
        return None;
    }

    let mut volume_name = vec![0u16; 256];
    let mut filesystem_name = vec![0u16; 64];
    let mut serial_number = 0u32;
    let mut max_component_length = 0u32;
    let mut filesystem_flags = 0u32;

    let volume_info = unsafe {
        GetVolumeInformationW(
            PCWSTR(wide_root.as_ptr()),
            Some(volume_name.as_mut_slice()),
            Some(&mut serial_number),
            Some(&mut max_component_length),
            Some(&mut filesystem_flags),
            Some(filesystem_name.as_mut_slice()),
        )
    };

    let filesystem = volume_info
        .as_ref()
        .map(|_| wide_buf_to_string(&filesystem_name))
        .unwrap_or_else(|_| "Unknown".to_string());
    let volume_label = volume_info
        .as_ref()
        .map(|_| wide_buf_to_string(&volume_name))
        .unwrap_or_default();

    let letter = root.trim_end_matches('\\').to_string();
    let label = if volume_label.is_empty() {
        letter.clone()
    } else {
        format!("{volume_label} ({letter})")
    };

    Some(DriveInfo {
        letter,
        label,
        supported: filesystem.eq_ignore_ascii_case("NTFS"),
        filesystem,
    })
}

fn wide_buf_to_string(buf: &[u16]) -> String {
    let len = buf.iter().position(|ch| *ch == 0).unwrap_or(buf.len());
    String::from_utf16_lossy(&buf[..len])
}

fn to_wide(value: &str) -> Vec<u16> {
    OsStr::new(value)
        .encode_wide()
        .chain(std::iter::once(0))
        .collect()
}
