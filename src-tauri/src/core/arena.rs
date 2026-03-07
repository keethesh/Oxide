use serde::Serialize;

#[derive(Debug, Default, Serialize)]
pub struct StringArena {
    data: Vec<u8>,
}

impl StringArena {
    pub fn new() -> Self {
        Self {
            data: Vec::with_capacity(1024 * 1024 * 10), // Start with 10MB
        }
    }

    pub fn push(&mut self, s: &str) -> (u32, u16) {
        let offset = self.data.len() as u32;
        let bytes = s.as_bytes();
        let len = bytes.len() as u16; // Truncate if > 65535, but generic filenames are short

        // Safety check for 4GB limit
        if offset as u64 + len as u64 > u32::MAX as u64 {
            // In a real scenario, we might want to handle this better or panic
            // For now, we panic as per design constraints
            panic!("StringArena overflow! > 4GB strings");
        }

        self.data.extend_from_slice(bytes);
        (offset, len)
    }

    pub fn get(&self, offset: u32, len: u16) -> &str {
        let start = offset as usize;
        let end = start + len as usize;

        // Unsafe block to avoid bounds check overhead in hot loops?
        // For now, safe slice is fine.
        match self.data.get(start..end) {
            Some(bytes) => unsafe { std::str::from_utf8_unchecked(bytes) },
            None => "<CORRUPT>",
        }
    }

    pub fn len(&self) -> usize {
        self.data.len()
    }
}
