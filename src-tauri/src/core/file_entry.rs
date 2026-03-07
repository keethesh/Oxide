use serde::Serialize;

#[repr(C)] // Natural alignment, no UB risk
#[derive(Debug, Clone, Copy, Serialize)]
pub struct FileEntry {
    pub size: u64,               // 8 bytes (aligned 8)
    pub parent_index: u32,       // 4 bytes
    pub first_child_index: u32,  // 4 bytes
    pub next_sibling_index: u32, // 4 bytes
    pub name_offset: u32,        // 4 bytes
    pub name_len: u16,           // 2 bytes
    pub flags: u16,              // 2 bytes
}

#[derive(Debug, Clone, Copy)]
#[repr(u16)]
pub enum FileFlags {
    Directory = 1 << 0,
    ReadOnly = 1 << 1,
    Hidden = 1 << 2,
    System = 1 << 3,
    Reparse = 1 << 4,
}

impl FileEntry {
    pub const NULL_INDEX: u32 = u32::MAX;

    pub fn is_dir(&self) -> bool {
        (self.flags & FileFlags::Directory as u16) != 0
    }
}
