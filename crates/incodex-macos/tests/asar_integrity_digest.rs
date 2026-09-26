#[path = "../src/asar_integrity_digest.rs"]
mod asar_integrity_digest;

use asar_integrity_digest::plan_integrity_digest_update;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};

const SLOT_SENTINEL: &[u8; 32] = b"AGbevlPCksUGKNL8TSn7wGmJEuJsXb2A";
const SLOT_SIZE: usize = 66;
const MACH_HEADER_64_SIZE: usize = 32;
const SEGMENT_COMMAND_64_SIZE: usize = 72;
const SECTION_64_SIZE: usize = 80;
const SLOT_FILE_OFFSET: usize =
    MACH_HEADER_64_SIZE + SEGMENT_COMMAND_64_SIZE + SECTION_64_SIZE;

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
        hasher.update(
            entry["hash"]
                .as_str()
                .expect("test hash string")
                .as_bytes(),
        );
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
    write_u32_le(&mut bytes, 20, (SEGMENT_COMMAND_64_SIZE + SECTION_64_SIZE) as u32);

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

fn read_slot_digest(bytes: &[u8], slice_offset: usize) -> [u8; 32] {
    let start = slice_offset + SLOT_FILE_OFFSET + 34;
    bytes[start..start + 32].try_into().expect("slot digest bytes")
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

    assert_eq!(&updated[..SLOT_FILE_OFFSET + 34], &original[..SLOT_FILE_OFFSET + 34]);
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
