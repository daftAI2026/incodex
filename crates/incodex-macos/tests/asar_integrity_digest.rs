#[path = "../src/asar_integrity_digest.rs"]
mod asar_integrity_digest;

use asar_integrity_digest::{
    dynamic_framework_load_paths, linked_dylib_paths, plan_integrity_digest_update,
    resolved_linked_dylib_paths,
};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::path::Path;

const SLOT_SENTINEL: &[u8; 32] = b"AGbevlPCksUGKNL8TSn7wGmJEuJsXb2A";
const SLOT_SIZE: usize = 66;
const MACH_HEADER_64_SIZE: usize = 32;
const SEGMENT_COMMAND_64_SIZE: usize = 72;
const SECTION_64_SIZE: usize = 80;
const SLOT_FILE_OFFSET: usize = MACH_HEADER_64_SIZE + SEGMENT_COMMAND_64_SIZE + SECTION_64_SIZE;

fn map_pair(first_hash: &str, second_hash: &str) -> Value {
    // Reverse insertion order deliberately; the production digest must use
    // literal key ordering, not object insertion order or locale collation.
    json!({
        "z-last.asar": { "algorithm": "SHA256", "hash": second_hash },
        "A-first.asar": { "algorithm": "SHA256", "hash": first_hash }
    })
}

fn spec_digest(map: &Value) -> [u8; 32] {
    let object = map.as_object().expect("test integrity map object");
    let mut keys: Vec<&str> = object.keys().map(String::as_str).collect();
    keys.sort_by(|left, right| left.as_bytes().cmp(right.as_bytes()));

    let mut hasher = Sha256::new();
    for key in keys {
        let entry = &object[key];
        hasher.update(key.as_bytes());
        hasher.update(
            entry["algorithm"]
                .as_str()
                .expect("test algorithm string")
                .as_bytes(),
        );
        hasher.update(entry["hash"].as_str().expect("test hash string").as_bytes());
    }
    hasher.finalize().into()
}

fn slot(map: &Value, used: u8, version: u8) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(SLOT_SIZE);
    bytes.extend_from_slice(SLOT_SENTINEL);
    bytes.push(used);
    bytes.push(version);
    bytes.extend_from_slice(&spec_digest(map));
    assert_eq!(bytes.len(), SLOT_SIZE);
    bytes
}

fn write_u32_le(bytes: &mut [u8], offset: usize, value: u32) {
    bytes[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
}

fn write_u64_le(bytes: &mut [u8], offset: usize, value: u64) {
    bytes[offset..offset + 8].copy_from_slice(&value.to_le_bytes());
}

fn write_u64_be(bytes: &mut [u8], offset: usize, value: u64) {
    bytes[offset..offset + 8].copy_from_slice(&value.to_be_bytes());
}

fn write_u32_be(bytes: &mut [u8], offset: usize, value: u32) {
    bytes[offset..offset + 4].copy_from_slice(&value.to_be_bytes());
}

fn thin_macho(cpu_type: u32, section_data: Option<&[u8]>) -> Vec<u8> {
    let Some(section_data) = section_data else {
        let mut bytes = vec![0; MACH_HEADER_64_SIZE];
        bytes[0..4].copy_from_slice(&[0xcf, 0xfa, 0xed, 0xfe]); // MH_MAGIC_64, little endian
        write_u32_le(&mut bytes, 4, cpu_type);
        write_u32_le(&mut bytes, 12, 6); // MH_DYLIB
        return bytes;
    };

    let data_offset = SLOT_FILE_OFFSET;
    let file_size = data_offset + section_data.len();
    let mut bytes = vec![0; file_size];

    // mach_header_64
    bytes[0..4].copy_from_slice(&[0xcf, 0xfa, 0xed, 0xfe]); // MH_MAGIC_64
    write_u32_le(&mut bytes, 4, cpu_type);
    write_u32_le(&mut bytes, 12, 6); // MH_DYLIB
    write_u32_le(&mut bytes, 16, 1); // ncmds
    write_u32_le(
        &mut bytes,
        20,
        (SEGMENT_COMMAND_64_SIZE + SECTION_64_SIZE) as u32,
    );

    // LC_SEGMENT_64 with one __DATA_CONST,__asar_integrity section.
    let command = MACH_HEADER_64_SIZE;
    write_u32_le(&mut bytes, command, 0x19);
    write_u32_le(
        &mut bytes,
        command + 4,
        (SEGMENT_COMMAND_64_SIZE + SECTION_64_SIZE) as u32,
    );
    bytes[command + 8..command + 20].copy_from_slice(b"__DATA_CONST");
    write_u64_le(&mut bytes, command + 32, section_data.len() as u64); // vmsize
    write_u64_le(&mut bytes, command + 40, data_offset as u64); // fileoff
    write_u64_le(&mut bytes, command + 48, section_data.len() as u64); // filesize
    write_u32_le(&mut bytes, command + 56, 7); // maxprot
    write_u32_le(&mut bytes, command + 60, 3); // initprot
    write_u32_le(&mut bytes, command + 64, 1); // nsects

    let section = command + SEGMENT_COMMAND_64_SIZE;
    bytes[section..section + 16].copy_from_slice(b"__asar_integrity");
    bytes[section + 16..section + 28].copy_from_slice(b"__DATA_CONST");
    write_u64_le(&mut bytes, section + 40, section_data.len() as u64);
    write_u32_le(&mut bytes, section + 48, data_offset as u32);
    bytes[data_offset..].copy_from_slice(section_data);
    bytes
}

fn set_thin_section_offset(bytes: &mut [u8], offset: u32) {
    let section_offset_field = MACH_HEADER_64_SIZE + SEGMENT_COMMAND_64_SIZE + 48;
    write_u32_le(bytes, section_offset_field, offset);
}

fn fat32_macho(slices: &[(u32, Vec<u8>)]) -> (Vec<u8>, Vec<usize>) {
    let header_size = 8 + slices.len() * 20;
    let mut offsets = Vec::with_capacity(slices.len());
    let mut next_offset = (header_size + 3) & !3;
    for (_, slice) in slices {
        offsets.push(next_offset);
        next_offset = (next_offset + slice.len() + 3) & !3;
    }
    let mut bytes = vec![0; next_offset];
    bytes[0..4].copy_from_slice(&0xcafebabe_u32.to_be_bytes()); // FAT_MAGIC
    write_u32_be(&mut bytes, 4, slices.len() as u32);
    for (index, (cpu_type, slice)) in slices.iter().enumerate() {
        let entry = 8 + index * 20;
        write_u32_be(&mut bytes, entry, *cpu_type);
        write_u32_be(&mut bytes, entry + 4, 0); // cpusubtype
        write_u32_be(&mut bytes, entry + 8, offsets[index] as u32);
        write_u32_be(&mut bytes, entry + 12, slice.len() as u32);
        write_u32_be(&mut bytes, entry + 16, 2); // 4-byte alignment
        bytes[offsets[index]..offsets[index] + slice.len()].copy_from_slice(slice);
    }
    (bytes, offsets)
}

fn fat64_macho(slices: &[(u32, Vec<u8>)]) -> (Vec<u8>, Vec<usize>) {
    let header_size = 8 + slices.len() * 32;
    let mut offsets = Vec::with_capacity(slices.len());
    let mut next_offset = (header_size + 7) & !7;
    for (_, slice) in slices {
        offsets.push(next_offset);
        next_offset = (next_offset + slice.len() + 7) & !7;
    }
    let mut bytes = vec![0; next_offset];
    bytes[0..4].copy_from_slice(&0xcafebabf_u32.to_be_bytes()); // FAT_MAGIC_64
    write_u32_be(&mut bytes, 4, slices.len() as u32);
    for (index, (cpu_type, slice)) in slices.iter().enumerate() {
        let entry = 8 + index * 32;
        write_u32_be(&mut bytes, entry, *cpu_type);
        write_u32_be(&mut bytes, entry + 4, 0); // cpusubtype
        write_u64_be(&mut bytes, entry + 8, offsets[index] as u64);
        write_u64_be(&mut bytes, entry + 16, slice.len() as u64);
        write_u32_be(&mut bytes, entry + 24, 3); // 8-byte alignment
        write_u32_be(&mut bytes, entry + 28, 0); // reserved
        bytes[offsets[index]..offsets[index] + slice.len()].copy_from_slice(slice);
    }
    (bytes, offsets)
}

fn dylib_load_command(command: u32, path: &str) -> Vec<u8> {
    let command_size = (24 + path.len() + 1 + 7) & !7;
    let mut bytes = vec![0; command_size];
    write_u32_le(&mut bytes, 0, command);
    write_u32_le(&mut bytes, 4, command_size as u32);
    write_u32_le(&mut bytes, 8, 24); // dylib.name.offset
    bytes[24..24 + path.len()].copy_from_slice(path.as_bytes());
    bytes
}

fn rpath_load_command(path: &str) -> Vec<u8> {
    const LC_RPATH: u32 = 0x8000_001c;
    let command_size = (12 + path.len() + 1 + 7) & !7;
    let mut bytes = vec![0; command_size];
    write_u32_le(&mut bytes, 0, LC_RPATH);
    write_u32_le(&mut bytes, 4, command_size as u32);
    write_u32_le(&mut bytes, 8, 12); // rpath.path.offset
    bytes[12..12 + path.len()].copy_from_slice(path.as_bytes());
    bytes
}

fn thin_macho_with_dylib_commands(cpu_type: u32, commands: &[Vec<u8>]) -> Vec<u8> {
    let command_bytes: usize = commands.iter().map(Vec::len).sum();
    let mut bytes = vec![0; MACH_HEADER_64_SIZE + command_bytes];
    bytes[0..4].copy_from_slice(&[0xcf, 0xfa, 0xed, 0xfe]); // MH_MAGIC_64
    write_u32_le(&mut bytes, 4, cpu_type);
    write_u32_le(&mut bytes, 12, 6); // MH_DYLIB
    write_u32_le(&mut bytes, 16, commands.len() as u32);
    write_u32_le(&mut bytes, 20, command_bytes as u32);
    let mut cursor = MACH_HEADER_64_SIZE;
    for command in commands {
        bytes[cursor..cursor + command.len()].copy_from_slice(command);
        cursor += command.len();
    }
    bytes
}

fn thin_macho_with_cstrings_and_symbols(
    cpu_type: u32,
    segment_name: &str,
    section_name: &str,
    cstrings: &[&str],
    symbols: &[(&str, u8)],
) -> Vec<u8> {
    let mut section_data = Vec::from([0]);
    for value in cstrings {
        section_data.extend_from_slice(value.as_bytes());
        section_data.push(0);
    }
    thin_macho_with_text_section_and_symbols(
        cpu_type,
        segment_name,
        section_name,
        &section_data,
        symbols,
    )
}

fn thin_macho_with_text_section_and_symbols(
    cpu_type: u32,
    segment_name: &str,
    section_name: &str,
    section_data: &[u8],
    symbols: &[(&str, u8)],
) -> Vec<u8> {
    const LC_SEGMENT_64: u32 = 0x19;
    const LC_SYMTAB: u32 = 0x02;
    const SEGMENT_COMMAND_64_SIZE: usize = 72;
    const SECTION_64_SIZE: usize = 80;
    const SYMTAB_COMMAND_SIZE: usize = 24;
    const NLIST_64_SIZE: usize = 16;

    let mut string_table = Vec::from([0]);
    let mut string_indexes = Vec::with_capacity(symbols.len());
    for (name, _) in symbols {
        string_indexes.push(string_table.len() as u32);
        string_table.extend_from_slice(name.as_bytes());
        string_table.push(0);
    }

    let command_bytes = SEGMENT_COMMAND_64_SIZE + SECTION_64_SIZE + SYMTAB_COMMAND_SIZE;
    let cstring_offset = MACH_HEADER_64_SIZE + command_bytes;
    let symbol_offset = (cstring_offset + section_data.len() + 7) & !7;
    let symbol_bytes_len = symbols.len() * NLIST_64_SIZE;
    let string_offset = symbol_offset + symbol_bytes_len;
    let total_len = string_offset + string_table.len();
    let mut bytes = vec![0; total_len];

    // mach_header_64
    bytes[0..4].copy_from_slice(&[0xcf, 0xfa, 0xed, 0xfe]); // MH_MAGIC_64
    write_u32_le(&mut bytes, 4, cpu_type);
    write_u32_le(&mut bytes, 12, 2); // MH_EXECUTE
    write_u32_le(&mut bytes, 16, 2); // LC_SEGMENT_64 + LC_SYMTAB
    write_u32_le(&mut bytes, 20, command_bytes as u32);

    // LC_SEGMENT_64 with the requested cstring section.
    let segment = MACH_HEADER_64_SIZE;
    write_u32_le(&mut bytes, segment, LC_SEGMENT_64);
    write_u32_le(
        &mut bytes,
        segment + 4,
        (SEGMENT_COMMAND_64_SIZE + SECTION_64_SIZE) as u32,
    );
    bytes[segment + 8..segment + 8 + segment_name.len()].copy_from_slice(segment_name.as_bytes());
    write_u64_le(&mut bytes, segment + 32, section_data.len() as u64); // vmsize
    write_u64_le(&mut bytes, segment + 40, cstring_offset as u64); // fileoff
    write_u64_le(&mut bytes, segment + 48, section_data.len() as u64); // filesize
    write_u32_le(&mut bytes, segment + 56, 7); // maxprot
    write_u32_le(&mut bytes, segment + 60, 5); // initprot
    write_u32_le(&mut bytes, segment + 64, 1); // nsects

    let section = segment + SEGMENT_COMMAND_64_SIZE;
    bytes[section..section + section_name.len()].copy_from_slice(section_name.as_bytes());
    bytes[section + 16..section + 16 + segment_name.len()].copy_from_slice(segment_name.as_bytes());
    write_u64_le(&mut bytes, section + 40, section_data.len() as u64);
    write_u32_le(&mut bytes, section + 48, cstring_offset as u32);

    // LC_SYMTAB; every supplied tuple is (symbol name, n_type).
    let symtab = segment + SEGMENT_COMMAND_64_SIZE + SECTION_64_SIZE;
    write_u32_le(&mut bytes, symtab, LC_SYMTAB);
    write_u32_le(&mut bytes, symtab + 4, SYMTAB_COMMAND_SIZE as u32);
    write_u32_le(&mut bytes, symtab + 8, symbol_offset as u32);
    write_u32_le(&mut bytes, symtab + 12, symbols.len() as u32);
    write_u32_le(&mut bytes, symtab + 16, string_offset as u32);
    write_u32_le(&mut bytes, symtab + 20, string_table.len() as u32);

    for (index, ((_, n_type), string_index)) in symbols.iter().zip(string_indexes).enumerate() {
        let nlist = symbol_offset + index * NLIST_64_SIZE;
        write_u32_le(&mut bytes, nlist, string_index);
        bytes[nlist + 4] = *n_type;
    }
    bytes[cstring_offset..cstring_offset + section_data.len()].copy_from_slice(section_data);
    bytes[string_offset..string_offset + string_table.len()].copy_from_slice(&string_table);
    bytes
}

fn read_slot_digest(bytes: &[u8], slice_offset: usize) -> [u8; 32] {
    let start = slice_offset + SLOT_FILE_OFFSET + 34;
    bytes[start..start + 32]
        .try_into()
        .expect("slot digest bytes")
}

#[test]
fn updates_thin_arm64_slot_only_after_matching_old_map_digest() {
    const ARM64: u32 = 0x0100_000c;
    let old_map = map_pair(&"11".repeat(32), &"22".repeat(32));
    let new_map = map_pair(&"33".repeat(32), &"44".repeat(32));
    let original = thin_macho(ARM64, Some(&slot(&old_map, 1, 1)));

    let updated = plan_integrity_digest_update(&original, &old_map, &new_map)
        .expect("valid current digest map")
        .expect("active slot should be modified");

    assert_eq!(
        &updated[..SLOT_FILE_OFFSET + 34],
        &original[..SLOT_FILE_OFFSET + 34]
    );
    assert_eq!(read_slot_digest(&updated, 0), spec_digest(&new_map));
    assert_eq!(updated[SLOT_FILE_OFFSET + 32], 1, "slot remains used");
    assert_eq!(updated[SLOT_FILE_OFFSET + 33], 1, "slot version remains v1");
    assert_eq!(original[SLOT_FILE_OFFSET + 34..], spec_digest(&old_map));
}

#[test]
fn updates_every_arm64_and_x64_slice_before_returning_a_fat_file() {
    const ARM64: u32 = 0x0100_000c;
    const X86_64: u32 = 0x0100_0007;
    let old_map = map_pair(&"55".repeat(32), &"66".repeat(32));
    let new_map = map_pair(&"77".repeat(32), &"88".repeat(32));
    let (original, offsets) = fat32_macho(&[
        (ARM64, thin_macho(ARM64, Some(&slot(&old_map, 1, 1)))),
        (X86_64, thin_macho(X86_64, Some(&slot(&old_map, 1, 1)))),
    ]);

    let updated = plan_integrity_digest_update(&original, &old_map, &new_map)
        .expect("valid universal framework")
        .expect("both active slices should be updated");

    for offset in offsets {
        assert_eq!(read_slot_digest(&updated, offset), spec_digest(&new_map));
        assert_eq!(updated[offset + SLOT_FILE_OFFSET + 32], 1);
        assert_eq!(updated[offset + SLOT_FILE_OFFSET + 33], 1);
    }
}

#[test]
fn updates_fat64_slices_and_leaves_architecture_table_unchanged() {
    const ARM64: u32 = 0x0100_000c;
    const X86_64: u32 = 0x0100_0007;
    let old_map = map_pair(&"13".repeat(32), &"24".repeat(32));
    let new_map = map_pair(&"35".repeat(32), &"46".repeat(32));
    let (original, offsets) = fat64_macho(&[
        (ARM64, thin_macho(ARM64, Some(&slot(&old_map, 1, 1)))),
        (X86_64, thin_macho(X86_64, Some(&slot(&old_map, 1, 1)))),
    ]);

    let updated = plan_integrity_digest_update(&original, &old_map, &new_map)
        .expect("valid FAT_MAGIC_64 file")
        .expect("both active fat64 slices should be updated");

    assert_eq!(&updated[..8 + 2 * 32], &original[..8 + 2 * 32]);
    for offset in offsets {
        assert_eq!(read_slot_digest(&updated, offset), spec_digest(&new_map));
    }
}

#[test]
fn old_macho_without_the_slot_is_compatible_and_unchanged() {
    const ARM64: u32 = 0x0100_000c;
    let old_map = map_pair(&"99".repeat(32), &"aa".repeat(32));
    let new_map = map_pair(&"bb".repeat(32), &"cc".repeat(32));
    let original = thin_macho(ARM64, None);

    assert_eq!(
        plan_integrity_digest_update(&original, &old_map, &new_map).unwrap(),
        None
    );
}

#[test]
fn unused_slot_is_preserved_without_enabling_or_rewriting_it() {
    const X86_64: u32 = 0x0100_0007;
    let old_map = map_pair(&"dd".repeat(32), &"ee".repeat(32));
    let new_map = map_pair(&"ff".repeat(32), &"00".repeat(32));
    let original = thin_macho(X86_64, Some(&slot(&old_map, 0, 1)));

    assert_eq!(
        plan_integrity_digest_update(&original, &old_map, &new_map).unwrap(),
        None
    );
    assert_eq!(original[SLOT_FILE_OFFSET + 32], 0);
}

#[test]
fn identical_new_map_is_a_noop_for_an_already_matching_active_slot() {
    const ARM64: u32 = 0x0100_000c;
    let map = map_pair(&"47".repeat(32), &"58".repeat(32));
    let original = thin_macho(ARM64, Some(&slot(&map, 1, 1)));

    assert_eq!(
        plan_integrity_digest_update(&original, &map, &map).unwrap(),
        None
    );
}

#[test]
fn unknown_used_value_fails_closed_without_mutating_the_input() {
    const ARM64: u32 = 0x0100_000c;
    let old_map = map_pair(&"69".repeat(32), &"7a".repeat(32));
    let new_map = map_pair(&"8b".repeat(32), &"9c".repeat(32));
    let original = thin_macho(ARM64, Some(&slot(&old_map, 2, 1)));

    let error = plan_integrity_digest_update(&original, &old_map, &new_map)
        .expect_err("unknown used state must fail closed");
    assert!(error.to_lowercase().contains("used"), "{error}");
    assert_eq!(original[SLOT_FILE_OFFSET + 32], 2);
}

#[test]
fn sentinel_outside_the_named_integrity_section_is_ignored() {
    const ARM64: u32 = 0x0100_000c;
    let old_map = map_pair(&"ad".repeat(32), &"be".repeat(32));
    let new_map = map_pair(&"cf".repeat(32), &"d0".repeat(32));
    let mut original = thin_macho(ARM64, Some(&slot(&old_map, 1, 1)));
    let section_name = MACH_HEADER_64_SIZE + SEGMENT_COMMAND_64_SIZE;
    original[section_name..section_name + 16].fill(0);
    original[section_name..section_name + 6].copy_from_slice(b"__text");

    assert_eq!(
        plan_integrity_digest_update(&original, &old_map, &new_map).unwrap(),
        None,
        "the scanner must not find the sentinel elsewhere in the Mach-O"
    );
}

#[test]
fn malformed_load_command_range_fails_closed_without_panicking() {
    const ARM64: u32 = 0x0100_000c;
    let old_map = map_pair(&"e1".repeat(32), &"f2".repeat(32));
    let new_map = map_pair(&"a3".repeat(32), &"b4".repeat(32));
    let mut original = thin_macho(ARM64, Some(&slot(&old_map, 1, 1)));
    write_u32_le(&mut original, 20, 8); // load command advertises more bytes than sizeofcmds

    assert!(plan_integrity_digest_update(&original, &old_map, &new_map).is_err());
}

#[test]
fn overlapping_fat_slices_fail_closed() {
    const ARM64: u32 = 0x0100_000c;
    const X86_64: u32 = 0x0100_0007;
    let old_map = map_pair(&"c5".repeat(32), &"d6".repeat(32));
    let new_map = map_pair(&"e7".repeat(32), &"f8".repeat(32));
    let (mut original, offsets) = fat32_macho(&[
        (ARM64, thin_macho(ARM64, Some(&slot(&old_map, 1, 1)))),
        (X86_64, thin_macho(X86_64, Some(&slot(&old_map, 1, 1)))),
    ]);
    write_u32_be(&mut original, 8 + 20 + 8, offsets[0] as u32);

    let error = plan_integrity_digest_update(&original, &old_map, &new_map)
        .expect_err("overlapping fat slices are ambiguous");
    assert!(error.to_lowercase().contains("overlap"), "{error}");
}

#[test]
fn parses_supported_dylib_load_command_paths_in_command_order() {
    const ARM64: u32 = 0x0100_000c;
    const LC_LOAD_DYLIB: u32 = 0x0000_000c;
    const LC_LOAD_WEAK_DYLIB: u32 = 0x8000_0018;
    const LC_REEXPORT_DYLIB: u32 = 0x8000_001f;
    const LC_LOAD_UPWARD_DYLIB: u32 = 0x8000_0023;
    const LC_ID_DYLIB: u32 = 0x0000_000d;
    let macho = thin_macho_with_dylib_commands(
        ARM64,
        &[
            dylib_load_command(LC_LOAD_DYLIB, "@rpath/Aperitif.framework/Aperitif"),
            dylib_load_command(LC_ID_DYLIB, "ignored-install-name.dylib"),
            dylib_load_command(LC_LOAD_WEAK_DYLIB, "@loader_path/libOptional.dylib"),
            dylib_load_command(LC_REEXPORT_DYLIB, "/usr/lib/libReexport.dylib"),
            dylib_load_command(LC_LOAD_UPWARD_DYLIB, "@executable_path/Host.framework/Host"),
        ],
    );

    assert_eq!(
        linked_dylib_paths(&macho).unwrap(),
        [
            "@rpath/Aperitif.framework/Aperitif",
            "@loader_path/libOptional.dylib",
            "/usr/lib/libReexport.dylib",
            "@executable_path/Host.framework/Host",
        ]
    );
}

#[test]
fn parses_lazy_dylib_load_paths_for_each_supported_architecture() {
    const LC_LAZY_LOAD_DYLIB: u32 = 0x20;
    for cpu in [0x0100_000c, 0x0100_0007] {
        let macho = thin_macho_with_dylib_commands(
            cpu,
            &[dylib_load_command(
                LC_LAZY_LOAD_DYLIB,
                "@rpath/Renamed.framework/Renamed",
            )],
        );
        assert_eq!(
            linked_dylib_paths(&macho).unwrap(),
            ["@rpath/Renamed.framework/Renamed"]
        );
    }
}

#[test]
fn resolves_lazy_dylib_load_against_the_same_runpath_search() {
    let executable = Path::new("/Applications/ChatGPT.app/Contents/MacOS/Helper");
    let macho = thin_macho_with_dylib_commands(
        0x0100_000c,
        &[
            rpath_load_command("@loader_path/../Frameworks"),
            dylib_load_command(0x20, "@rpath/Renamed.framework/Renamed"),
        ],
    );
    assert_eq!(
        resolved_linked_dylib_paths(&macho, executable).unwrap(),
        [vec![executable
            .parent()
            .unwrap()
            .join("../Frameworks/Renamed.framework/Renamed")]]
    );
}

#[test]
fn rejects_lazy_dylib_names_outside_the_command() {
    let mut macho = thin_macho_with_dylib_commands(
        0x0100_000c,
        &[dylib_load_command(0x20, "@rpath/Renamed.framework/Renamed")],
    );
    write_u32_le(&mut macho, MACH_HEADER_64_SIZE + 8, 0);
    assert!(linked_dylib_paths(&macho).is_err());
}

#[test]
fn resolves_rpath_install_name_against_loader_path() {
    const ARM64: u32 = 0x0100_000c;
    const LC_LOAD_DYLIB: u32 = 0x0000_000c;
    let executable = Path::new(
        "/Applications/ChatGPT.app/Contents/Frameworks/LinkedHelper.app/Contents/MacOS/LinkedHelper",
    );
    let macho = thin_macho_with_dylib_commands(
        ARM64,
        &[
            rpath_load_command("@loader_path/../../../"),
            dylib_load_command(LC_LOAD_DYLIB, "@rpath/Renamed.framework/Renamed"),
        ],
    );
    let expected = executable
        .parent()
        .unwrap()
        .join("../../../")
        .join("Renamed.framework/Renamed");

    assert_eq!(
        resolved_linked_dylib_paths(&macho, executable).unwrap(),
        [vec![expected]]
    );
}

#[test]
fn preserves_rpath_search_order_when_first_candidate_is_a_non_target() {
    const ARM64: u32 = 0x0100_000c;
    const LC_LOAD_DYLIB: u32 = 0x0000_000c;
    let executable = Path::new(
        "/Applications/ChatGPT.app/Contents/Frameworks/LinkedHelper.app/Contents/MacOS/LinkedHelper",
    );
    let macho = thin_macho_with_dylib_commands(
        ARM64,
        &[
            rpath_load_command("@loader_path/../../../Other"),
            rpath_load_command("@loader_path/../../../"),
            dylib_load_command(LC_LOAD_DYLIB, "@rpath/Renamed.framework/Renamed"),
        ],
    );
    let loader_dir = executable.parent().unwrap();
    let non_target = loader_dir
        .join("../../../Other")
        .join("Renamed.framework/Renamed");
    let target = loader_dir
        .join("../../../")
        .join("Renamed.framework/Renamed");

    assert_eq!(
        resolved_linked_dylib_paths(&macho, executable).unwrap(),
        [vec![non_target.clone(), target.clone()]],
        "retain per-load-command alternatives in LC_RPATH search order"
    );
    assert_ne!(
        non_target, target,
        "same install name must not imply same binary"
    );
}

#[test]
fn resolves_runpaths_within_each_fat_slice_without_cross_pairing() {
    const ARM64: u32 = 0x0100_000c;
    const X86_64: u32 = 0x0100_0007;
    const LC_LOAD_DYLIB: u32 = 0x0000_000c;
    let executable = Path::new("/Applications/ChatGPT.app/Contents/MacOS/LinkedHelper");
    let (macho, _) = fat32_macho(&[
        (
            ARM64,
            thin_macho_with_dylib_commands(
                ARM64,
                &[
                    rpath_load_command("@loader_path/arm64"),
                    dylib_load_command(LC_LOAD_DYLIB, "@rpath/Framework/Framework"),
                ],
            ),
        ),
        (
            X86_64,
            thin_macho_with_dylib_commands(
                X86_64,
                &[
                    rpath_load_command("@loader_path/x86_64"),
                    dylib_load_command(LC_LOAD_DYLIB, "@rpath/Framework/Framework"),
                ],
            ),
        ),
    ]);

    let loader_dir = executable.parent().unwrap();
    assert_eq!(
        resolved_linked_dylib_paths(&macho, executable).unwrap(),
        [
            vec![loader_dir.join("arm64/Framework/Framework")],
            vec![loader_dir.join("x86_64/Framework/Framework")],
        ]
    );
}

#[test]
fn rejects_out_of_range_and_unterminated_rpath_strings() {
    const ARM64: u32 = 0x0100_000c;
    const LC_LOAD_DYLIB: u32 = 0x0000_000c;
    let executable = Path::new("/tmp/LinkedHelper");
    let mut out_of_range = thin_macho_with_dylib_commands(
        ARM64,
        &[
            rpath_load_command("@loader_path/Frameworks"),
            dylib_load_command(LC_LOAD_DYLIB, "@rpath/Renamed.framework/Renamed"),
        ],
    );
    write_u32_le(&mut out_of_range, MACH_HEADER_64_SIZE + 8, u32::MAX);
    assert!(resolved_linked_dylib_paths(&out_of_range, executable).is_err());

    let mut unterminated = thin_macho_with_dylib_commands(
        ARM64,
        &[
            rpath_load_command("@loader_path/Frameworks"),
            dylib_load_command(LC_LOAD_DYLIB, "@rpath/Renamed.framework/Renamed"),
        ],
    );
    let command_size = u32::from_le_bytes(
        unterminated[MACH_HEADER_64_SIZE + 4..MACH_HEADER_64_SIZE + 8]
            .try_into()
            .unwrap(),
    ) as usize;
    unterminated[MACH_HEADER_64_SIZE + 12..MACH_HEADER_64_SIZE + command_size].fill(b'x');
    assert!(resolved_linked_dylib_paths(&unterminated, executable).is_err());
}

#[test]
fn rejects_dylib_name_offsets_outside_the_command() {
    const ARM64: u32 = 0x0100_000c;
    const LC_LOAD_DYLIB: u32 = 0x0000_000c;
    let mut macho = thin_macho_with_dylib_commands(
        ARM64,
        &[dylib_load_command(LC_LOAD_DYLIB, "@rpath/libExample.dylib")],
    );
    write_u32_le(&mut macho, MACH_HEADER_64_SIZE + 8, u32::MAX);

    assert!(linked_dylib_paths(&macho).is_err());
}

#[test]
fn rejects_dylib_names_without_a_terminator_inside_the_command() {
    const ARM64: u32 = 0x0100_000c;
    const LC_LOAD_DYLIB: u32 = 0x0000_000c;
    let mut macho = thin_macho_with_dylib_commands(
        ARM64,
        &[dylib_load_command(LC_LOAD_DYLIB, "@rpath/libExample.dylib")],
    );
    let command_start = MACH_HEADER_64_SIZE;
    let command_size = u32::from_le_bytes(
        macho[command_start + 4..command_start + 8]
            .try_into()
            .unwrap(),
    ) as usize;
    macho[command_start + 24..command_start + command_size].fill(b'x');

    assert!(linked_dylib_paths(&macho).is_err());
}

#[test]
fn aggregates_linked_dylib_paths_from_each_fat_slice() {
    const ARM64: u32 = 0x0100_000c;
    const X86_64: u32 = 0x0100_0007;
    const LC_LOAD_DYLIB: u32 = 0x0000_000c;
    let (macho, _) = fat32_macho(&[
        (
            ARM64,
            thin_macho_with_dylib_commands(
                ARM64,
                &[dylib_load_command(LC_LOAD_DYLIB, "@rpath/arm64.dylib")],
            ),
        ),
        (
            X86_64,
            thin_macho_with_dylib_commands(
                X86_64,
                &[dylib_load_command(LC_LOAD_DYLIB, "@rpath/x86_64.dylib")],
            ),
        ),
    ]);

    assert_eq!(
        linked_dylib_paths(&macho).unwrap(),
        ["@rpath/arm64.dylib", "@rpath/x86_64.dylib"]
    );
}

#[test]
fn recognizes_classic_dynamic_framework_loader_from_real_cstrings_and_imports() {
    const ARM64: u32 = 0x0100_000c;
    const N_UNDF_EXT: u8 = 0x01;
    let macho = thin_macho_with_cstrings_and_symbols(
        ARM64,
        "__TEXT",
        "__cstring",
        &["ChromeMain", "../../../../Codex Framework"],
        &[("_dlopen", N_UNDF_EXT), ("_dlsym", N_UNDF_EXT)],
    );

    assert_eq!(
        dynamic_framework_load_paths(&macho).unwrap(),
        ["../../../../Codex Framework"]
    );
}

#[test]
fn aggregates_classic_dynamic_framework_paths_from_fat_arm64_and_x64_slices() {
    const ARM64: u32 = 0x0100_000c;
    const X86_64: u32 = 0x0100_0007;
    const N_UNDF_EXT: u8 = 0x01;
    let (macho, _) = fat32_macho(&[
        (
            ARM64,
            thin_macho_with_cstrings_and_symbols(
                ARM64,
                "__TEXT",
                "__cstring",
                &["ChromeMain", "../../../../Codex Framework"],
                &[("_dlopen", N_UNDF_EXT), ("_dlsym", N_UNDF_EXT)],
            ),
        ),
        (
            X86_64,
            thin_macho_with_cstrings_and_symbols(
                X86_64,
                "__TEXT",
                "__cstring",
                &["ChromeMain", "../../../../Codex Framework x64"],
                &[("_dlopen", N_UNDF_EXT), ("_dlsym", N_UNDF_EXT)],
            ),
        ),
    ]);

    assert_eq!(
        dynamic_framework_load_paths(&macho).unwrap(),
        [
            "../../../../Codex Framework",
            "../../../../Codex Framework x64"
        ]
    );
}

#[test]
fn recognizes_classic_loader_paths_in_text_const_for_arm64_with_non_string_tail() {
    const ARM64: u32 = 0x0100_000c;
    const N_UNDF_EXT: u8 = 0x01;
    let section_data = b"ChromeMain\0../../../../Renamed\0\x91\x92\x93";
    let macho = thin_macho_with_text_section_and_symbols(
        ARM64,
        "__TEXT",
        "__const",
        section_data,
        &[("_dlopen", N_UNDF_EXT), ("_dlsym", N_UNDF_EXT)],
    );

    assert_eq!(
        dynamic_framework_load_paths(&macho).unwrap(),
        ["../../../../Renamed"]
    );
}

#[test]
fn aggregates_text_const_loader_paths_from_fat_arm64_and_x64_slices() {
    const ARM64: u32 = 0x0100_000c;
    const X86_64: u32 = 0x0100_0007;
    const N_UNDF_EXT: u8 = 0x01;
    let (macho, _) = fat32_macho(&[
        (
            ARM64,
            thin_macho_with_text_section_and_symbols(
                ARM64,
                "__TEXT",
                "__const",
                b"ChromeMain\0../../../../Renamed\0\x91\x92",
                &[("_dlopen", N_UNDF_EXT), ("_dlsym", N_UNDF_EXT)],
            ),
        ),
        (
            X86_64,
            thin_macho_with_text_section_and_symbols(
                X86_64,
                "__TEXT",
                "__const",
                b"ChromeMain\0../../../../RenamedX64\0\x81",
                &[("_dlopen", N_UNDF_EXT), ("_dlsym", N_UNDF_EXT)],
            ),
        ),
    ]);

    assert_eq!(
        dynamic_framework_load_paths(&macho).unwrap(),
        ["../../../../Renamed", "../../../../RenamedX64"]
    );
}

#[test]
fn classic_loader_contract_requires_both_imports_chromemain_and_framework_path() {
    const ARM64: u32 = 0x0100_000c;
    const N_UNDF_EXT: u8 = 0x01;
    let both_imports = &[("_dlopen", N_UNDF_EXT), ("_dlsym", N_UNDF_EXT)];

    let missing_dlopen = thin_macho_with_cstrings_and_symbols(
        ARM64,
        "__TEXT",
        "__cstring",
        &["ChromeMain", "../../../../Codex Framework"],
        &[("_dlsym", N_UNDF_EXT)],
    );
    let missing_chrome_main = thin_macho_with_cstrings_and_symbols(
        ARM64,
        "__TEXT",
        "__cstring",
        &["../../../../Codex Framework"],
        both_imports,
    );
    let missing_framework_path = thin_macho_with_cstrings_and_symbols(
        ARM64,
        "__TEXT",
        "__cstring",
        &["ChromeMain"],
        both_imports,
    );

    for macho in [missing_dlopen, missing_chrome_main, missing_framework_path] {
        assert!(dynamic_framework_load_paths(&macho).unwrap().is_empty());
    }
}

#[test]
fn classic_loader_requires_undefined_external_symbols_instead_of_defined_symbols() {
    const ARM64: u32 = 0x0100_000c;
    const N_SECT_EXT: u8 = 0x0f;
    const N_UNDF: u8 = 0x00;
    let macho = thin_macho_with_cstrings_and_symbols(
        ARM64,
        "__TEXT",
        "__cstring",
        &["ChromeMain", "../../../../Codex Framework"],
        &[("_dlopen", N_SECT_EXT), ("_dlsym", N_UNDF)],
    );

    assert!(dynamic_framework_load_paths(&macho).unwrap().is_empty());
}

#[test]
fn accepts_valid_symbol_name_suffixes_in_the_shared_lc_symtab_string_pool() {
    const ARM64: u32 = 0x0100_000c;
    const N_UNDF_EXT: u8 = 0x01;
    let mut macho = thin_macho_with_cstrings_and_symbols(
        ARM64,
        "__TEXT",
        "__cstring",
        &["ChromeMain", "../../../../Codex Framework"],
        &[("_prefix_dlopen", N_UNDF_EXT), ("_dlsym", N_UNDF_EXT)],
    );

    let symtab_command = MACH_HEADER_64_SIZE + SEGMENT_COMMAND_64_SIZE + SECTION_64_SIZE;
    let symbol_offset = u32::from_le_bytes(
        macho[symtab_command + 8..symtab_command + 12]
            .try_into()
            .unwrap(),
    ) as usize;
    let string_offset = u32::from_le_bytes(
        macho[symtab_command + 16..symtab_command + 20]
            .try_into()
            .unwrap(),
    ) as usize;
    let suffix_index = 1 + "_prefix".len();
    assert_eq!(
        &macho[string_offset + suffix_index..string_offset + suffix_index + 8],
        b"_dlopen\0"
    );
    assert_ne!(macho[string_offset + suffix_index - 1], 0);
    write_u32_le(&mut macho, symbol_offset, suffix_index as u32);

    assert_eq!(
        dynamic_framework_load_paths(&macho).unwrap(),
        ["../../../../Codex Framework"]
    );
}

#[test]
fn dynamic_loader_does_not_scan_outside_the_text_cstring_section() {
    const ARM64: u32 = 0x0100_000c;
    const N_UNDF_EXT: u8 = 0x01;
    let macho = thin_macho_with_cstrings_and_symbols(
        ARM64,
        "__DATA",
        "__cstring",
        &["ChromeMain", "../../../../Codex Framework"],
        &[("_dlopen", N_UNDF_EXT), ("_dlsym", N_UNDF_EXT)],
    );

    assert!(dynamic_framework_load_paths(&macho).unwrap().is_empty());
}

#[test]
fn dynamic_loader_rejects_out_of_range_cstring_and_symbol_tables() {
    const ARM64: u32 = 0x0100_000c;
    const N_UNDF_EXT: u8 = 0x01;
    let make_fixture = || {
        thin_macho_with_cstrings_and_symbols(
            ARM64,
            "__TEXT",
            "__cstring",
            &["ChromeMain", "../../../../Codex Framework"],
            &[("_dlopen", N_UNDF_EXT), ("_dlsym", N_UNDF_EXT)],
        )
    };

    let mut bad_cstring_range = make_fixture();
    let cstring_offset_field = MACH_HEADER_64_SIZE + SEGMENT_COMMAND_64_SIZE + 48;
    write_u32_le(&mut bad_cstring_range, cstring_offset_field, u32::MAX);
    assert!(dynamic_framework_load_paths(&bad_cstring_range).is_err());

    let mut bad_symbol_range = make_fixture();
    let symtab_command = MACH_HEADER_64_SIZE + SEGMENT_COMMAND_64_SIZE + SECTION_64_SIZE;
    write_u32_le(&mut bad_symbol_range, symtab_command + 8, u32::MAX);
    assert!(dynamic_framework_load_paths(&bad_symbol_range).is_err());
}

#[test]
fn dynamic_loader_rejects_unterminated_cstrings_and_symbol_names() {
    const ARM64: u32 = 0x0100_000c;
    const N_UNDF_EXT: u8 = 0x01;
    let make_fixture = || {
        thin_macho_with_cstrings_and_symbols(
            ARM64,
            "__TEXT",
            "__cstring",
            &["ChromeMain", "../../../../Codex Framework"],
            &[("_dlopen", N_UNDF_EXT), ("_dlsym", N_UNDF_EXT)],
        )
    };

    let mut unterminated_cstring = make_fixture();
    let cstring_offset = MACH_HEADER_64_SIZE + SEGMENT_COMMAND_64_SIZE + SECTION_64_SIZE + 24;
    // The fixture's __cstring table ends immediately before 8-byte symtab alignment.
    let cstring_size = 1 + "ChromeMain".len() + 1 + "../../../../Codex Framework".len() + 1;
    unterminated_cstring[cstring_offset + cstring_size - 1] = b'x';
    assert!(dynamic_framework_load_paths(&unterminated_cstring).is_err());

    let mut unterminated_symbol = make_fixture();
    let symtab_command = MACH_HEADER_64_SIZE + SEGMENT_COMMAND_64_SIZE + SECTION_64_SIZE;
    let symbol_offset = u32::from_le_bytes(
        unterminated_symbol[symtab_command + 8..symtab_command + 12]
            .try_into()
            .unwrap(),
    ) as usize;
    let string_offset = u32::from_le_bytes(
        unterminated_symbol[symtab_command + 16..symtab_command + 20]
            .try_into()
            .unwrap(),
    ) as usize;
    let string_size = u32::from_le_bytes(
        unterminated_symbol[symtab_command + 20..symtab_command + 24]
            .try_into()
            .unwrap(),
    ) as usize;
    assert!(symbol_offset < string_offset, "fixture has nlist64 entries");
    unterminated_symbol[string_offset + string_size - 1] = b'x';
    assert!(dynamic_framework_load_paths(&unterminated_symbol).is_err());
}

#[test]
fn active_slot_requires_its_existing_digest_to_match_the_old_map() {
    const ARM64: u32 = 0x0100_000c;
    let old_map = map_pair(&"12".repeat(32), &"34".repeat(32));
    let other_map = map_pair(&"56".repeat(32), &"78".repeat(32));
    let new_map = map_pair(&"9a".repeat(32), &"bc".repeat(32));
    let original = thin_macho(ARM64, Some(&slot(&other_map, 1, 1)));

    let error = plan_integrity_digest_update(&original, &old_map, &new_map)
        .expect_err("mismatched old map must fail closed");
    assert!(error.to_lowercase().contains("digest"), "{error}");
    assert_eq!(read_slot_digest(&original, 0), spec_digest(&other_map));
}

#[test]
fn unknown_active_slot_version_fails_closed() {
    const ARM64: u32 = 0x0100_000c;
    let old_map = map_pair(&"de".repeat(32), &"ad".repeat(32));
    let new_map = map_pair(&"be".repeat(32), &"ef".repeat(32));
    let original = thin_macho(ARM64, Some(&slot(&old_map, 1, 2)));

    let error = plan_integrity_digest_update(&original, &old_map, &new_map)
        .expect_err("unknown active digest-slot version must fail closed");
    assert!(error.to_lowercase().contains("version"), "{error}");
}

#[test]
fn duplicate_slots_in_the_integrity_section_fail_closed() {
    const ARM64: u32 = 0x0100_000c;
    let old_map = map_pair(&"01".repeat(32), &"02".repeat(32));
    let new_map = map_pair(&"03".repeat(32), &"04".repeat(32));
    let mut data = slot(&old_map, 1, 1);
    data.extend_from_slice(&slot(&old_map, 1, 1));
    let original = thin_macho(ARM64, Some(&data));

    let error = plan_integrity_digest_update(&original, &old_map, &new_map)
        .expect_err("duplicate sentinel must fail closed");
    assert!(error.to_lowercase().contains("duplicate"), "{error}");
}

#[test]
fn out_of_range_integrity_section_fails_closed() {
    const ARM64: u32 = 0x0100_000c;
    let old_map = map_pair(&"05".repeat(32), &"06".repeat(32));
    let new_map = map_pair(&"07".repeat(32), &"08".repeat(32));
    let mut original = thin_macho(ARM64, Some(&slot(&old_map, 1, 1)));
    set_thin_section_offset(&mut original, u32::MAX);

    let error = plan_integrity_digest_update(&original, &old_map, &new_map)
        .expect_err("out-of-range section must fail closed");
    assert!(error.to_lowercase().contains("range"), "{error}");
}

#[test]
fn mixed_slot_presence_in_a_universal_file_fails_closed() {
    const ARM64: u32 = 0x0100_000c;
    const X86_64: u32 = 0x0100_0007;
    let old_map = map_pair(&"09".repeat(32), &"0a".repeat(32));
    let new_map = map_pair(&"0b".repeat(32), &"0c".repeat(32));
    let (original, _) = fat32_macho(&[
        (ARM64, thin_macho(ARM64, Some(&slot(&old_map, 1, 1)))),
        (X86_64, thin_macho(X86_64, None)),
    ]);

    assert!(plan_integrity_digest_update(&original, &old_map, &new_map).is_err());
}

#[test]
fn malformed_integrity_map_fails_closed() {
    const ARM64: u32 = 0x0100_000c;
    let old_map = json!({"main.js": {"algorithm": "SHA256", "hash": "0d".repeat(32)}});
    let malformed_new_map = json!({"main.js": {"algorithm": "SHA256", "hash": 17}});
    let original = thin_macho(ARM64, Some(&slot(&old_map, 1, 1)));

    assert!(plan_integrity_digest_update(&original, &old_map, &malformed_new_map).is_err());
}
