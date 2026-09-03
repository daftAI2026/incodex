const MH_MAGIC_64: u32 = 0xfeed_facf;
const LC_SEGMENT_64: u32 = 0x19;
const LC_LOAD_DYLIB: u32 = 0x0c;
const MACH_HEADER_64_SIZE: usize = 32;
const LOAD_COMMAND_HEADER_SIZE: usize = 8;
const SEGMENT_64_COMMAND_SIZE: usize = 72;
const DYLIB_COMMAND_NAME_OFFSET: usize = 24;
const DYLIB_PATH: &[u8] = b"@loader_path/IncodexKeyProvider.dylib";

/// 在 64-bit little-endian Mach-O 的现有 header padding 中加入普通依赖。
///
/// 该操作不搬移文件内容，也不删除既有 load command。任何校验失败都发生在写入前，
/// 因而调用方可以把错误视作“输入字节完全未变”。
pub fn add_load_dylib(bytes: &mut [u8]) -> Result<(), String> {
    if bytes.len() < MACH_HEADER_64_SIZE {
        return Err("Mach-O is shorter than a 64-bit header".to_owned());
    }
    if read_u32(bytes, 0)? != MH_MAGIC_64 {
        return Err("only little-endian 64-bit Mach-O files are supported".to_owned());
    }

    let command_count = read_u32(bytes, 16)? as usize;
    let commands_size = read_u32(bytes, 20)? as usize;
    let commands_end = MACH_HEADER_64_SIZE
        .checked_add(commands_size)
        .ok_or_else(|| "Mach-O load-command size overflows".to_owned())?;
    if commands_end > bytes.len() {
        return Err("Mach-O load-command region exceeds the file".to_owned());
    }

    let mut offset = MACH_HEADER_64_SIZE;
    let mut first_file_content = None;
    for _ in 0..command_count {
        let header_end = offset
            .checked_add(LOAD_COMMAND_HEADER_SIZE)
            .ok_or_else(|| "Mach-O load-command offset overflows".to_owned())?;
        if header_end > commands_end {
            return Err("Mach-O load-command header exceeds sizeofcmds".to_owned());
        }

        let command = read_u32(bytes, offset)?;
        let command_size = read_u32(bytes, offset + 4)? as usize;
        if command_size < LOAD_COMMAND_HEADER_SIZE || !command_size.is_multiple_of(4) {
            return Err("Mach-O contains an invalid load-command size".to_owned());
        }
        let command_end = offset
            .checked_add(command_size)
            .ok_or_else(|| "Mach-O load-command size overflows".to_owned())?;
        if command_end > commands_end {
            return Err("Mach-O load command exceeds sizeofcmds".to_owned());
        }

        if command == LC_LOAD_DYLIB && dylib_name(&bytes[offset..command_end])? == DYLIB_PATH {
            return Ok(());
        }

        if command == LC_SEGMENT_64 {
            if command_size < SEGMENT_64_COMMAND_SIZE {
                return Err("Mach-O contains a truncated LC_SEGMENT_64".to_owned());
            }
            let file_offset = read_u64(bytes, offset + 40)?;
            let file_size = read_u64(bytes, offset + 48)?;
            if file_offset > 0 && file_size > 0 {
                let file_offset = usize::try_from(file_offset)
                    .map_err(|_| "Mach-O segment file offset does not fit this host".to_owned())?;
                first_file_content = Some(
                    first_file_content
                        .map_or(file_offset, |current: usize| current.min(file_offset)),
                );
            }
        }

        offset = command_end;
    }
    if offset != commands_end {
        return Err("Mach-O ncmds and sizeofcmds disagree".to_owned());
    }

    let command_size = align_up(DYLIB_COMMAND_NAME_OFFSET + DYLIB_PATH.len() + 1, 8)?;
    let new_commands_end = commands_end
        .checked_add(command_size)
        .ok_or_else(|| "new Mach-O load-command size overflows".to_owned())?;
    let first_file_content = first_file_content
        .ok_or_else(|| "Mach-O has no positive file-backed segment boundary".to_owned())?;
    if first_file_content > bytes.len() || new_commands_end > first_file_content {
        return Err(
            "Mach-O has insufficient header padding for the provider dependency".to_owned(),
        );
    }
    if bytes[commands_end..new_commands_end]
        .iter()
        .any(|byte| *byte != 0)
    {
        return Err("Mach-O header padding is not empty".to_owned());
    }

    let new_command_count = command_count
        .checked_add(1)
        .and_then(|value| u32::try_from(value).ok())
        .ok_or_else(|| "Mach-O load-command count overflows".to_owned())?;
    let new_commands_size = commands_size
        .checked_add(command_size)
        .and_then(|value| u32::try_from(value).ok())
        .ok_or_else(|| "Mach-O sizeofcmds overflows".to_owned())?;

    // 所有失败分支都已结束；从这里开始的写入不会再返回错误。
    write_u32(bytes, 16, new_command_count);
    write_u32(bytes, 20, new_commands_size);
    write_u32(bytes, commands_end, LC_LOAD_DYLIB);
    write_u32(bytes, commands_end + 4, command_size as u32);
    write_u32(bytes, commands_end + 8, DYLIB_COMMAND_NAME_OFFSET as u32);
    let name_start = commands_end + DYLIB_COMMAND_NAME_OFFSET;
    bytes[name_start..name_start + DYLIB_PATH.len()].copy_from_slice(DYLIB_PATH);
    bytes[name_start + DYLIB_PATH.len()] = 0;
    Ok(())
}

fn dylib_name(command: &[u8]) -> Result<&[u8], String> {
    if command.len() < DYLIB_COMMAND_NAME_OFFSET {
        return Err("Mach-O contains a truncated LC_LOAD_DYLIB".to_owned());
    }
    let name_offset = read_u32(command, 8)? as usize;
    if name_offset < DYLIB_COMMAND_NAME_OFFSET || name_offset >= command.len() {
        return Err("Mach-O LC_LOAD_DYLIB has an invalid name offset".to_owned());
    }
    let tail = &command[name_offset..];
    let end = tail
        .iter()
        .position(|byte| *byte == 0)
        .ok_or_else(|| "Mach-O LC_LOAD_DYLIB name is not terminated".to_owned())?;
    Ok(&tail[..end])
}

fn align_up(value: usize, alignment: usize) -> Result<usize, String> {
    value
        .checked_add(alignment - 1)
        .map(|value| value & !(alignment - 1))
        .ok_or_else(|| "Mach-O command alignment overflows".to_owned())
}

fn read_u32(bytes: &[u8], offset: usize) -> Result<u32, String> {
    let end = offset
        .checked_add(4)
        .ok_or_else(|| "Mach-O integer offset overflows".to_owned())?;
    let raw = bytes
        .get(offset..end)
        .ok_or_else(|| "Mach-O integer exceeds the file".to_owned())?;
    Ok(u32::from_le_bytes(raw.try_into().expect("four-byte slice")))
}

fn read_u64(bytes: &[u8], offset: usize) -> Result<u64, String> {
    let end = offset
        .checked_add(8)
        .ok_or_else(|| "Mach-O integer offset overflows".to_owned())?;
    let raw = bytes
        .get(offset..end)
        .ok_or_else(|| "Mach-O integer exceeds the file".to_owned())?;
    Ok(u64::from_le_bytes(
        raw.try_into().expect("eight-byte slice"),
    ))
}

fn write_u32(bytes: &mut [u8], offset: usize, value: u32) {
    bytes[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
}
