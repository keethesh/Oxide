use super::volume::VolumeHandle;
use std::io::{Error, ErrorKind};
use windows::core::HRESULT;
use windows::Win32::Foundation::{ERROR_IO_PENDING, WAIT_TIMEOUT};
use windows::Win32::Storage::FileSystem::ReadFile;
use windows::Win32::System::IO::{
    CancelIoEx, GetOverlappedResultEx, OVERLAPPED, OVERLAPPED_0, OVERLAPPED_0_0,
};

pub fn read_mft_chunk(
    volume: &VolumeHandle,
    start_record: u64,
    count: u32,
    record_size: usize,
    timeout_ms: u32,
) -> std::io::Result<Vec<u8>> {
    let mft_offset =
        (volume.info.MftStartLcn as u64).saturating_mul(volume.info.BytesPerCluster as u64);
    let mut buffer = vec![0u8; count as usize * record_size];
    let mut total_read = 0usize;

    while total_read < buffer.len() {
        let absolute_offset = mft_offset
            .saturating_add(start_record.saturating_mul(record_size as u64))
            .saturating_add(total_read as u64);
        let bytes_read = read_at_offset(
            volume,
            absolute_offset,
            &mut buffer[total_read..],
            timeout_ms,
        )?;

        if bytes_read == 0 {
            return Err(Error::new(
                ErrorKind::UnexpectedEof,
                format!(
                    "short read at MFT record {start_record}; expected {} bytes, got {}",
                    buffer.len(),
                    total_read
                ),
            ));
        }

        total_read += bytes_read;
    }

    Ok(buffer)
}

fn read_at_offset(
    volume: &VolumeHandle,
    absolute_offset: u64,
    buffer: &mut [u8],
    timeout_ms: u32,
) -> std::io::Result<usize> {
    let mut overlapped = OVERLAPPED {
        Internal: 0,
        InternalHigh: 0,
        Anonymous: OVERLAPPED_0 {
            Anonymous: OVERLAPPED_0_0 {
                Offset: absolute_offset as u32,
                OffsetHigh: (absolute_offset >> 32) as u32,
            },
        },
        hEvent: Default::default(),
    };

    let read_result = unsafe { ReadFile(volume.handle, Some(buffer), None, Some(&mut overlapped)) };
    match read_result {
        Ok(()) => {}
        Err(err) if err.code() == HRESULT::from_win32(ERROR_IO_PENDING.0) => {}
        Err(err) => {
            return Err(Error::new(ErrorKind::Other, format!("read failed: {err}")));
        }
    }

    let mut bytes_read = 0u32;
    let wait_result = unsafe {
        GetOverlappedResultEx(
            volume.handle,
            &overlapped,
            &mut bytes_read,
            timeout_ms,
            false,
        )
    };

    match wait_result {
        Ok(()) => Ok(bytes_read as usize),
        Err(err) if err.code() == HRESULT::from_win32(WAIT_TIMEOUT.0) => {
            let _ = unsafe { CancelIoEx(volume.handle, Some(&overlapped)) };
            Err(Error::new(
                ErrorKind::TimedOut,
                format!("timed out reading offset {absolute_offset}"),
            ))
        }
        Err(err) => Err(Error::new(ErrorKind::Other, format!("wait failed: {err}"))),
    }
}
