use std::io;
use std::io::{Read, Seek, SeekFrom};

pub struct SectorReader<R>
where
    R: Read + Seek,
{
    inner: R,
    sector_size: usize,
    stream_position: u64,
    temp_buf: Vec<u8>,
}

impl<R> SectorReader<R>
where
    R: Read + Seek,
{
    pub fn new(inner: R, sector_size: usize) -> io::Result<Self> {
        if !sector_size.is_power_of_two() {
            return Err(io::Error::other("sector_size is not a power of two"));
        }

        Ok(Self {
            inner,
            sector_size,
            stream_position: 0,
            temp_buf: Vec::new(),
        })
    }

    fn align_down_to_sector_size(&self, value: u64) -> u64 {
        value / self.sector_size as u64 * self.sector_size as u64
    }

    fn align_up_to_sector_size(&self, value: u64) -> u64 {
        self.align_down_to_sector_size(value) + self.sector_size as u64
    }
}

impl<R> Read for SectorReader<R>
where
    R: Read + Seek,
{
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let aligned_position = self.align_down_to_sector_size(self.stream_position);
        let start = (self.stream_position - aligned_position) as usize;
        let end = start + buf.len();
        let aligned_bytes_to_read = self.align_up_to_sector_size(end as u64) as usize;

        self.temp_buf.resize(aligned_bytes_to_read, 0);
        self.inner.read_exact(&mut self.temp_buf)?;
        buf.copy_from_slice(&self.temp_buf[start..end]);

        self.stream_position += buf.len() as u64;
        Ok(buf.len())
    }
}

impl<R> Seek for SectorReader<R>
where
    R: Read + Seek,
{
    fn seek(&mut self, pos: SeekFrom) -> io::Result<u64> {
        let new_pos = match pos {
            SeekFrom::Start(value) => Some(value),
            SeekFrom::End(_) => {
                return Err(io::Error::other(
                    "SeekFrom::End is unsupported for SectorReader",
                ));
            }
            SeekFrom::Current(delta) => {
                if delta >= 0 {
                    self.stream_position.checked_add(delta as u64)
                } else {
                    self.stream_position
                        .checked_sub(delta.wrapping_neg() as u64)
                }
            }
        };

        match new_pos {
            Some(value) => {
                let aligned_value = self.align_down_to_sector_size(value);
                self.inner.seek(SeekFrom::Start(aligned_value))?;
                self.stream_position = value;
                Ok(self.stream_position)
            }
            None => Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "invalid seek to a negative or overflowing position",
            )),
        }
    }
}
