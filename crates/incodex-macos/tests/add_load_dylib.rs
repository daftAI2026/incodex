use incodex_macos::add_load_dylib;

const MH_MAGIC_64: u32 = 0xfeed_facf;
const LC_SEGMENT_64: u32 = 0x19;
const LC_UUID: u32 = 0x1b;
const LC_LOAD_DYLIB: u32 = 0x0c;
const MACH_HEADER_64_SIZE: usize = 32;
const SEGMENT_64_COMMAND_SIZE: usize = 72;
const UUID_COMMAND_SIZE: usize = 24;
const LOAD_DYLIB_COMMAND_SIZE: usize = 64;
const SEGMENT_COMMAND_OFFSET: usize = MACH_HEADER_64_SIZE;
const DATA_SEGMENT_COMMAND_OFFSET: usize = SEGMENT_COMMAND_OFFSET + SEGMENT_64_COMMAND_SIZE;
const UUID_COMMAND_OFFSET: usize = DATA_SEGMENT_COMMAND_OFFSET + SEGMENT_64_COMMAND_SIZE;
const OLD_LOAD_COMMANDS_SIZE: usize = SEGMENT_64_COMMAND_SIZE * 2 + UUID_COMMAND_SIZE;
const OLD_LOAD_COMMANDS_END: usize = MACH_HEADER_64_SIZE + OLD_LOAD_COMMANDS_SIZE;
const DATA_FILE_OFFSET: usize = 0x200;
const NORMAL_HEADER_PADDING: usize = DATA_FILE_OFFSET - OLD_LOAD_COMMANDS_END;
const DYLIB_NAME: &[u8] = b"@loader_path/IncodexKeyProvider.dylib";
const UUID: [u8; 16] = [
    0x01, 0x23, 0x45, 0x67, 0x89, 0xab, 0xcd, 0xef, 0x10, 0x32, 0x54, 0x76, 0x98, 0xba, 0xdc, 0xfe,
];
const FILE_CONTENT: &[u8] = b"content must remain at its original file offset\n";

#[test]
fn adds_normal_load_dylib_in_header_padding_without_moving_content_or_uuid() {
    let mut bytes = synthetic_macho(NORMAL_HEADER_PADDING);
    let before = bytes.clone();

    add_load_dylib(&mut bytes).unwrap();

    assert_eq!(bytes.len(), before.len());
    assert_eq!(bytes[DATA_FILE_OFFSET..], before[DATA_FILE_OFFSET..]);
    assert_eq!(
        bytes[MACH_HEADER_64_SIZE..OLD_LOAD_COMMANDS_END],
        before[MACH_HEADER_64_SIZE..OLD_LOAD_COMMANDS_END]
    );
    assert_eq!(
        read_u32(&bytes, 16),
        4,
        "the command count must include the new command"
    );
    assert_eq!(
        read_u32(&bytes, 20),
        (OLD_LOAD_COMMANDS_SIZE + LOAD_DYLIB_COMMAND_SIZE) as u32,
        "sizeofcmds must grow in place"
    );

    let command = find_command(&bytes, LC_LOAD_DYLIB).expect("normal dylib command");
    assert_eq!(command.len(), LOAD_DYLIB_COMMAND_SIZE);
    assert_eq!(read_u32(command, 4), LOAD_DYLIB_COMMAND_SIZE as u32);
    assert_eq!(
        read_u32(command, 8),
        24,
        "dylib name must use the normal name offset"
    );
    assert_eq!(
        &command[24..24 + DYLIB_NAME.len()],
        DYLIB_NAME,
        "the helper must be loaded by its stable loader-relative path"
    );
    assert_eq!(command[24 + DYLIB_NAME.len()], 0);
    assert!(command[25 + DYLIB_NAME.len()..]
        .iter()
        .all(|byte| *byte == 0));

    let uuid = find_command(&bytes, LC_UUID).expect("existing LC_UUID");
    assert_eq!(uuid.len(), UUID_COMMAND_SIZE);
    assert_eq!(&uuid[8..], &UUID);

    let mut expected = before;
    write_u32(&mut expected, 16, 4);
    write_u32(
        &mut expected,
        20,
        (OLD_LOAD_COMMANDS_SIZE + LOAD_DYLIB_COMMAND_SIZE) as u32,
    );
    expected[OLD_LOAD_COMMANDS_END..OLD_LOAD_COMMANDS_END + LOAD_DYLIB_COMMAND_SIZE]
        .copy_from_slice(&expected_load_dylib_command());
    assert_eq!(
        bytes, expected,
        "only header metadata and zero padding may change"
    );
}

#[test]
fn adding_the_same_load_dylib_twice_is_idempotent() {
    let mut bytes = synthetic_macho(NORMAL_HEADER_PADDING);

    add_load_dylib(&mut bytes).unwrap();
    let once = bytes.clone();
    add_load_dylib(&mut bytes).unwrap();

    assert_eq!(bytes, once);
    assert_eq!(read_u32(&bytes, 16), 4);
    assert_eq!(
        read_u32(&bytes, 20),
        (OLD_LOAD_COMMANDS_SIZE + LOAD_DYLIB_COMMAND_SIZE) as u32
    );
    assert_eq!(&find_command(&bytes, LC_UUID).unwrap()[8..], &UUID);
}

#[test]
fn refuses_insufficient_header_padding_without_mutating_bytes() {
    let mut bytes = synthetic_macho(LOAD_DYLIB_COMMAND_SIZE - 1);
    let before = bytes.clone();

    assert!(add_load_dylib(&mut bytes).is_err());
    assert_eq!(bytes, before);
}

#[test]
fn refuses_non_macho_bytes_without_mutating_bytes() {
    let mut bytes = vec![0xa5; DATA_FILE_OFFSET + FILE_CONTENT.len()];
    let before = bytes.clone();

    assert!(add_load_dylib(&mut bytes).is_err());
    assert_eq!(bytes, before);
}

#[test]
fn refuses_a_corrupt_load_command_without_mutating_bytes() {
    let mut bytes = synthetic_macho(NORMAL_HEADER_PADDING);
    write_u32(&mut bytes, UUID_COMMAND_OFFSET + 4, 0);
    let before = bytes.clone();

    assert!(add_load_dylib(&mut bytes).is_err());
    assert_eq!(bytes, before);
}

#[test]
fn refuses_a_segment_range_that_overflows_without_mutating_bytes() {
    let mut bytes = synthetic_macho(NORMAL_HEADER_PADDING);
    write_u64(&mut bytes, DATA_SEGMENT_COMMAND_OFFSET + 48, u64::MAX);
    let before = bytes.clone();

    assert!(add_load_dylib(&mut bytes).is_err());
    assert_eq!(bytes, before);
}

#[test]
fn refuses_segment_section_count_that_exceeds_its_command_without_mutating_bytes() {
    let mut bytes = synthetic_macho(NORMAL_HEADER_PADDING);
    write_u32(&mut bytes, DATA_SEGMENT_COMMAND_OFFSET + 64, 1);
    let before = bytes.clone();

    assert!(add_load_dylib(&mut bytes).is_err());
    assert_eq!(bytes, before);
}

#[test]
fn existing_provider_does_not_hide_a_later_corrupt_command() {
    let mut bytes = synthetic_macho(NORMAL_HEADER_PADDING + 8);
    add_load_dylib(&mut bytes).unwrap();
    let corrupt_offset = OLD_LOAD_COMMANDS_END + LOAD_DYLIB_COMMAND_SIZE;
    write_u32(&mut bytes, 16, 5);
    write_u32(
        &mut bytes,
        20,
        (OLD_LOAD_COMMANDS_SIZE + LOAD_DYLIB_COMMAND_SIZE + 8) as u32,
    );
    write_u32(&mut bytes, corrupt_offset, LC_UUID);
    write_u32(&mut bytes, corrupt_offset + 4, 0);
    let before = bytes.clone();

    assert!(add_load_dylib(&mut bytes).is_err());
    assert_eq!(bytes, before);
}

fn synthetic_macho(header_padding: usize) -> Vec<u8> {
    let data_file_offset = OLD_LOAD_COMMANDS_END + header_padding;
    let mut bytes = vec![0; data_file_offset + FILE_CONTENT.len()];

    write_u32(&mut bytes, 0, MH_MAGIC_64);
    write_u32(&mut bytes, 4, 0x0100_0007); // x86_64
    write_u32(&mut bytes, 8, 3); // CPU_SUBTYPE_X86_64_ALL
    write_u32(&mut bytes, 12, 2); // MH_EXECUTE
    write_u32(&mut bytes, 16, 3);
    write_u32(&mut bytes, 20, OLD_LOAD_COMMANDS_SIZE as u32);

    write_segment_64(
        &mut bytes[SEGMENT_COMMAND_OFFSET..],
        b"__TEXT",
        0x1_0000_0000,
        data_file_offset as u64,
        0,
        data_file_offset as u64,
        5,
        5,
    );
    write_segment_64(
        &mut bytes[DATA_SEGMENT_COMMAND_OFFSET..],
        b"__DATA",
        0x1_0000_0000 + data_file_offset as u64,
        FILE_CONTENT.len() as u64,
        data_file_offset as u64,
        FILE_CONTENT.len() as u64,
        3,
        3,
    );

    write_u32(&mut bytes, UUID_COMMAND_OFFSET, LC_UUID);
    write_u32(
        &mut bytes,
        UUID_COMMAND_OFFSET + 4,
        UUID_COMMAND_SIZE as u32,
    );
    bytes[UUID_COMMAND_OFFSET + 8..UUID_COMMAND_OFFSET + 24].copy_from_slice(&UUID);
    bytes[data_file_offset..].copy_from_slice(FILE_CONTENT);
    bytes
}

#[allow(clippy::too_many_arguments)] // 测试构造器按 Mach-O 字段顺序展开，调用点更易核对。
fn write_segment_64(
    command: &mut [u8],
    name: &[u8],
    vmaddr: u64,
    vmsize: u64,
    fileoff: u64,
    filesize: u64,
    maxprot: u32,
    initprot: u32,
) {
    write_u32(command, 0, LC_SEGMENT_64);
    write_u32(command, 4, SEGMENT_64_COMMAND_SIZE as u32);
    command[8..8 + name.len()].copy_from_slice(name);
    write_u64(command, 24, vmaddr);
    write_u64(command, 32, vmsize);
    write_u64(command, 40, fileoff);
    write_u64(command, 48, filesize);
    write_u32(command, 56, maxprot);
    write_u32(command, 60, initprot);
    write_u32(command, 64, 0); // nsects
    write_u32(command, 68, 0); // flags
}

fn expected_load_dylib_command() -> [u8; LOAD_DYLIB_COMMAND_SIZE] {
    let mut command = [0; LOAD_DYLIB_COMMAND_SIZE];
    write_u32(&mut command, 0, LC_LOAD_DYLIB);
    write_u32(&mut command, 4, LOAD_DYLIB_COMMAND_SIZE as u32);
    write_u32(&mut command, 8, 24);
    command[24..24 + DYLIB_NAME.len()].copy_from_slice(DYLIB_NAME);
    command[24 + DYLIB_NAME.len()] = 0;
    command
}

fn find_command(bytes: &[u8], kind: u32) -> Option<&[u8]> {
    let mut offset = MACH_HEADER_64_SIZE;
    let end = MACH_HEADER_64_SIZE + read_u32(bytes, 20) as usize;
    for _ in 0..read_u32(bytes, 16) {
        if offset + 8 > end {
            return None;
        }
        let command_size = read_u32(bytes, offset + 4) as usize;
        if command_size < 8 || !command_size.is_multiple_of(4) || offset + command_size > end {
            return None;
        }
        if read_u32(bytes, offset) == kind {
            return Some(&bytes[offset..offset + command_size]);
        }
        offset += command_size;
    }
    None
}

fn read_u32(bytes: &[u8], offset: usize) -> u32 {
    u32::from_le_bytes(bytes[offset..offset + 4].try_into().unwrap())
}

fn write_u32(bytes: &mut [u8], offset: usize, value: u32) {
    bytes[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
}

fn write_u64(bytes: &mut [u8], offset: usize, value: u64) {
    bytes[offset..offset + 8].copy_from_slice(&value.to_le_bytes());
}
