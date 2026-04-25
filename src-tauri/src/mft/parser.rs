use std::char::decode_utf16;

use crate::core::file_entry::FileFlags;

#[derive(Debug, Clone)]
pub struct MftEntry {
    pub id: u64,
    pub parent_id: u64,
    pub size: u64,
    pub name: String,
    pub is_dir: bool,
    pub flags: u16,
}

pub fn parse_record(data: &[u8], entry_id: u64) -> Option<MftEntry> {
    let mut scratch = Vec::with_capacity(data.len());
    parse_record_with_scratch(data, entry_id, &mut scratch)
}

pub fn parse_record_with_scratch(
    data: &[u8],
    entry_id: u64,
    scratch: &mut Vec<u8>,
) -> Option<MftEntry> {
    let data = apply_fixup(data, scratch)?;
    if data.len() < 42 || &data[0..4] != b"FILE" {
        return Option::None;
    }

    // Offset to first attribute
    let mut attr_offset = u16::from_le_bytes([data[20], data[21]]) as usize;
    let flags = u16::from_le_bytes([data[22], data[23]]);
    if (flags & 0x0001) == 0 {
        return None;
    }
    let is_dir = (flags & 0x0002) != 0;

    let mut name = String::new();
    let mut parent_id = 0;
    let mut size = 0;
    let mut file_name_size = 0;
    let mut found_name = false;
    let mut entry_flags = if is_dir {
        FileFlags::Directory as u16
    } else {
        0
    };

    // Iterate through attributes
    while attr_offset + 8 < data.len() {
        let attr_type = u32::from_le_bytes([
            data[attr_offset],
            data[attr_offset + 1],
            data[attr_offset + 2],
            data[attr_offset + 3],
        ]);

        if attr_type == 0xFFFFFFFF {
            break; // End of attributes
        }

        let attr_len = u32::from_le_bytes([
            data[attr_offset + 4],
            data[attr_offset + 5],
            data[attr_offset + 6],
            data[attr_offset + 7],
        ]) as usize;

        if attr_len == 0 || attr_offset + attr_len > data.len() {
            break;
        }

        match attr_type {
            0x30 => {
                // $FILE_NAME
                let content_offset =
                    u16::from_le_bytes([data[attr_offset + 20], data[attr_offset + 21]]) as usize;
                let name_start = attr_offset + content_offset;

                if name_start + 66 <= data.len() {
                    let name_len = data[name_start + 64] as usize;
                    let name_type = data[name_start + 65];
                    let is_preferred_name = name_type == 1 || name_type == 3;
                    if found_name && !is_preferred_name {
                        attr_offset += attr_len;
                        continue;
                    }

                    let name_data_start = name_start + 66;

                    if name_data_start + (name_len * 2) <= data.len() {
                        let current_name = decode_utf16((0..name_len).map(|i| {
                            u16::from_le_bytes([
                                data[name_data_start + i * 2],
                                data[name_data_start + i * 2 + 1],
                            ])
                        }))
                        .map(|result| result.unwrap_or(char::REPLACEMENT_CHARACTER))
                        .collect::<String>();

                        if !found_name || is_preferred_name {
                            parent_id = u64::from_le_bytes([
                                data[name_start],
                                data[name_start + 1],
                                data[name_start + 2],
                                data[name_start + 3],
                                data[name_start + 4],
                                data[name_start + 5],
                                0,
                                0, // MFT reference is 6 bytes
                            ]);
                            file_name_size = u64::from_le_bytes([
                                data[name_start + 48],
                                data[name_start + 49],
                                data[name_start + 50],
                                data[name_start + 51],
                                data[name_start + 52],
                                data[name_start + 53],
                                data[name_start + 54],
                                data[name_start + 55],
                            ]);
                            let file_attributes = u32::from_le_bytes([
                                data[name_start + 56],
                                data[name_start + 57],
                                data[name_start + 58],
                                data[name_start + 59],
                            ]);
                            entry_flags = apply_file_attributes(entry_flags, file_attributes);
                            name = current_name;
                            found_name = true;
                        }
                    }
                }
            }
            0x80 => {
                // $DATA
                let non_resident = data[attr_offset + 8] != 0;
                if !non_resident {
                    // Resident data
                    let content_len = u32::from_le_bytes([
                        data[attr_offset + 16],
                        data[attr_offset + 17],
                        data[attr_offset + 18],
                        data[attr_offset + 19],
                    ]) as u64;
                    size = content_len;
                } else {
                    // Non-resident data
                    let real_size = u64::from_le_bytes([
                        data[attr_offset + 48],
                        data[attr_offset + 49],
                        data[attr_offset + 50],
                        data[attr_offset + 51],
                        data[attr_offset + 52],
                        data[attr_offset + 53],
                        data[attr_offset + 54],
                        data[attr_offset + 55],
                    ]);
                    size = real_size;
                }
            }
            _ => {}
        }

        attr_offset += attr_len;
    }

    if found_name {
        if !is_dir && size == 0 {
            size = file_name_size;
        }

        Some(MftEntry {
            id: entry_id,
            parent_id,
            size,
            name,
            is_dir,
            flags: entry_flags,
        })
    } else {
        None
    }
}

fn apply_fixup<'a>(data: &'a [u8], scratch: &'a mut Vec<u8>) -> Option<&'a [u8]> {
    if data.len() < 42 || &data[0..4] != b"FILE" {
        return None;
    }

    let update_sequence_offset = u16::from_le_bytes([data[4], data[5]]) as usize;
    let update_sequence_count = u16::from_le_bytes([data[6], data[7]]) as usize;
    if update_sequence_count == 0 {
        return None;
    }

    let update_sequence_end = update_sequence_offset + update_sequence_count * 2;
    if update_sequence_end > data.len() {
        return None;
    }

    scratch.clear();
    scratch.extend_from_slice(data);
    let sequence_number = [
        scratch[update_sequence_offset],
        scratch[update_sequence_offset + 1],
    ];

    for sector_index in 1..update_sequence_count {
        let fixup_offset = sector_index * 512 - 2;
        let replacement_offset = update_sequence_offset + sector_index * 2;
        if fixup_offset + 2 > scratch.len() || replacement_offset + 2 > update_sequence_end {
            return None;
        }

        if scratch[fixup_offset] != sequence_number[0]
            || scratch[fixup_offset + 1] != sequence_number[1]
        {
            return None;
        }

        scratch[fixup_offset] = scratch[replacement_offset];
        scratch[fixup_offset + 1] = scratch[replacement_offset + 1];
    }

    Some(scratch)
}

fn apply_file_attributes(mut flags: u16, file_attributes: u32) -> u16 {
    if (file_attributes & 0x0000_0001) != 0 {
        flags |= FileFlags::ReadOnly as u16;
    }
    if (file_attributes & 0x0000_0002) != 0 {
        flags |= FileFlags::Hidden as u16;
    }
    if (file_attributes & 0x0000_0004) != 0 {
        flags |= FileFlags::System as u16;
    }
    if (file_attributes & 0x0000_0400) != 0 {
        flags |= FileFlags::Reparse as u16;
    }
    flags
}
