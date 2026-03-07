pub mod arena;
pub mod file_entry;
pub mod file_tree;

#[cfg(test)]
mod tests {
    use super::file_entry::FileEntry;
    use std::mem;

    #[test]
    fn test_file_entry_layout() {
        // Assert size is exactly 32 bytes (28 bytes data + 4 bytes padding for 8-byte alignment)
        assert_eq!(
            mem::size_of::<FileEntry>(),
            32,
            "FileEntry should be 32 bytes"
        );

        // Assert alignment is 8 bytes (due to u64)
        assert_eq!(
            mem::align_of::<FileEntry>(),
            8,
            "FileEntry should be 8-byte aligned"
        );

        // Assert offsets to ensure expected layout
        let zeroed: FileEntry = unsafe { mem::zeroed() };
        let base_ptr = &zeroed as *const FileEntry as usize;
        let size_ptr = &zeroed.size as *const _ as usize;
        let parent_ptr = &zeroed.parent_index as *const _ as usize;

        assert_eq!(size_ptr - base_ptr, 0, "size at offset 0");
        assert_eq!(parent_ptr - base_ptr, 8, "parent_index at offset 8");
    }
}
