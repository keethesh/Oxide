#[derive(Debug, Clone)]
pub struct MftEntry {
    pub id: u64,
    pub parent_id: u64,
    pub size: u64,
    pub name: String,
    pub is_dir: bool,
}

pub fn parse_record(data: &[u8], entry_id: u64) -> Option<MftEntry> {
    let data = apply_fixup(data)?;
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
    let mut found_name = false;

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

                    let name_len = data[name_start + 64] as usize;
                    let name_data_start = name_start + 66;

                    if name_data_start + (name_len * 2) <= data.len() {
                        let utf16_data: Vec<u16> = (0..name_len)
                            .map(|i| {
                                u16::from_le_bytes([
                                    data[name_data_start + i * 2],
                                    data[name_data_start + i * 2 + 1],
                                ])
                            })
                            .collect();

                        let current_name = String::from_utf16_lossy(&utf16_data);

                        // NTFS can have multiple names (DOS vs Win32). We prefer Win32.
                        let name_type = data[name_start + 65];
                        if !found_name || name_type == 1 || name_type == 3 {
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
        Some(MftEntry {
            id: entry_id,
            parent_id,
            size,
            name,
            is_dir,
        })
    } else {
        None
    }
}

fn apply_fixup(data: &[u8]) -> Option<Vec<u8>> {
    if data.len() < 8 {
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

    let mut record = data.to_vec();
    let update_sequence = record[update_sequence_offset..update_sequence_end].to_vec();
    let sequence_number = [update_sequence[0], update_sequence[1]];

    for sector_index in 1..update_sequence_count {
        let fixup_offset = sector_index * 512 - 2;
        if fixup_offset + 2 > record.len() {
            return None;
        }

        if record[fixup_offset] != sequence_number[0]
            || record[fixup_offset + 1] != sequence_number[1]
        {
            return None;
        }

        record[fixup_offset..fixup_offset + 2]
            .copy_from_slice(&update_sequence[sector_index * 2..sector_index * 2 + 2]);
    }

    Some(record)
}
