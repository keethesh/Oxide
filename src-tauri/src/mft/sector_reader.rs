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
        if value == 0 {
            0
        } else {
            self.align_down_to_sector_size(value.saturating_add(self.sector_size as u64 - 1))
        }
    }
}

impl<R> Read for SectorReader<R>
where
    R: Read + Seek,
{
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        if buf.is_empty() {
            return Ok(0);
        }

        let aligned_position = self.align_down_to_sector_size(self.stream_position);
        let start = (self.stream_position - aligned_position) as usize;
        let requested_end = start + buf.len();
        let aligned_end = self.align_up_to_sector_size(self.stream_position + buf.len() as u64);
        let aligned_bytes_to_read = (aligned_end - aligned_position) as usize;

        self.temp_buf.resize(aligned_bytes_to_read, 0);
        self.inner.seek(SeekFrom::Start(aligned_position))?;
        self.inner.read_exact(&mut self.temp_buf)?;
        buf.copy_from_slice(&self.temp_buf[start..requested_end]);

        self.stream_position += buf.len() as u64;
        self.inner.seek(SeekFrom::Start(
            self.align_down_to_sector_size(self.stream_position),
        ))?;
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Cursor, Read, Seek};

    fn reader() -> SectorReader<Cursor<Vec<u8>>> {
        SectorReader::new(Cursor::new((0u8..64).collect()), 8).unwrap()
    }

    #[test]
    fn unaligned_sequential_reads_return_logical_bytes() {
        let mut reader = reader();
        reader.seek(SeekFrom::Start(3)).unwrap();

        let mut first = [0; 4];
        reader.read_exact(&mut first).unwrap();
        let mut second = [0; 5];
        reader.read_exact(&mut second).unwrap();

        assert_eq!(first, [3, 4, 5, 6]);
        assert_eq!(second, [7, 8, 9, 10, 11]);
        assert_eq!(reader.stream_position, 12);
        assert_eq!(reader.inner.stream_position().unwrap(), 8);
    }

    #[test]
    fn seek_then_read_uses_aligned_source_offset() {
        let mut reader = reader();
        reader.seek(SeekFrom::Start(17)).unwrap();

        let mut bytes = [0; 3];
        reader.read_exact(&mut bytes).unwrap();

        assert_eq!(bytes, [17, 18, 19]);
        assert_eq!(reader.stream_position, 20);
        assert_eq!(reader.inner.stream_position().unwrap(), 16);
    }

    #[test]
    fn reads_can_span_sector_boundaries() {
        let mut reader = reader();
        reader.seek(SeekFrom::Start(6)).unwrap();

        let mut bytes = [0; 7];
        reader.read_exact(&mut bytes).unwrap();

        assert_eq!(bytes, [6, 7, 8, 9, 10, 11, 12]);
        assert_eq!(reader.stream_position, 13);
        assert_eq!(reader.inner.stream_position().unwrap(), 8);
    }
}
