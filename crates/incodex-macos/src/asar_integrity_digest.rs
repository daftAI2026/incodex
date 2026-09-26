use serde_json::Value;
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};

const SLOT_SENTINEL: &[u8; 32] = b"AGbevlPCksUGKNL8TSn7wGmJEuJsXb2A";
const SLOT_DIGEST_SIZE: usize = 32;
const SLOT_SIZE: usize = SLOT_SENTINEL.len() + 2 + SLOT_DIGEST_SIZE;
const MACH_HEADER_64_SIZE: usize = 32;
const SEGMENT_COMMAND_64: u32 = 0x19;
const SEGMENT_COMMAND_64_SIZE: usize = 72;
const SECTION_64_SIZE: usize = 80;
const CPU_TYPE_X86_64: u32 = 0x0100_0007;
const CPU_TYPE_ARM64: u32 = 0x0100_000c;

const LC_LOAD_DYLIB: u32 = 0x0000_000c;
const LC_LOAD_WEAK_DYLIB: u32 = 0x8000_0018;
const LC_REEXPORT_DYLIB: u32 = 0x8000_001f;
const LC_LAZY_LOAD_DYLIB: u32 = 0x0000_0020;
const LC_LOAD_UPWARD_DYLIB: u32 = 0x8000_0023;
const LC_RPATH: u32 = 0x8000_001c;
const DYLIB_COMMAND_SIZE: usize = 24;
const RPATH_COMMAND_SIZE: usize = 12;
const LC_SYMTAB: u32 = 0x02;
const SYMTAB_COMMAND_SIZE: usize = 24;
const NLIST_64_SIZE: usize = 16;

const FAT_MAGIC: u32 = 0xcafe_babe;
const FAT_MAGIC_64: u32 = 0xcafe_babf;

/// Plans a package-time Electron ASAR integrity digest update without changing
/// the input buffer. `None` means there is no active slot to update or the
/// active slot already contains the digest for `new_map`.
pub(crate) fn plan_integrity_digest_update(
    macho: &[u8],
    old_map: &Value,
    new_map: &Value,
) -> Result<Option<Vec<u8>>, String> {
    let old_digest = integrity_dictionary_digest(old_map)?;
    let new_digest = integrity_dictionary_digest(new_map)?;
    let slices = parse_slices(macho)?;

    let mut observations = Vec::with_capacity(slices.len());
    for slice in &slices {
        observations.push(inspect_integrity_slot(macho, *slice)?);
    }

    let present_count = observations
        .iter()
        .filter(|observation| !matches!(observation, SlotObservation::Missing))
        .count();
    if present_count == 0 {
        return Ok(None);
    }
    if present_count != observations.len() {
        return Err("Mach-O slices disagree about the presence of the ASAR integrity slot".into());
    }

    let active_count = observations
        .iter()
        .filter(|observation| matches!(observation, SlotObservation::Active { .. }))
        .count();
    if active_count == 0 {
        // Electron deliberately ignores the version and digest when `used` is
        // false. Preserve that state; package-time code must never enable it.
        return Ok(None);
    }
    if active_count != observations.len() {
        return Err(
            "Mach-O slices disagree about whether the ASAR integrity slot is active".into(),
        );
    }

    let mut write_offsets = Vec::with_capacity(observations.len());
    for observation in observations {
        let SlotObservation::Active {
            digest,
            digest_offset,
        } = observation
        else {
            unreachable!("active_count checked above")
        };
        if digest != old_digest {
            return Err(
                "existing ASAR integrity slot digest does not match the old plist map".into(),
            );
        }
        write_offsets.push(digest_offset);
    }

    if old_digest == new_digest {
        return Ok(None);
    }

    // No writes are attempted until every slice and every old digest has been
    // validated. The caller receives a complete replacement buffer or none.
    let mut updated = macho.to_vec();
    for digest_offset in write_offsets {
        let end = digest_offset
            .checked_add(SLOT_DIGEST_SIZE)
            .ok_or_else(|| "ASAR integrity digest range overflow".to_string())?;
        updated[digest_offset..end].copy_from_slice(&new_digest);
    }
    Ok(Some(updated))
}

#[cfg(test)]
#[allow(dead_code)] // Used by the path-included parser integration test, not by the library test harness.
pub(crate) fn linked_dylib_paths(bytes: &[u8]) -> Result<Vec<String>, String> {
    Ok(parse_linked_dylib_commands(bytes)?
        .into_iter()
        .flat_map(|slice| slice.dylibs)
        .collect())
}

pub(crate) fn resolved_linked_dylib_paths(
    bytes: &[u8],
    executable: &Path,
) -> Result<Vec<Vec<PathBuf>>, String> {
    let image_dir = executable
        .parent()
        .ok_or_else(|| "Mach-O executable has no parent directory".to_string())?;
    let mut groups = Vec::new();
    for slice in parse_linked_dylib_commands(bytes)? {
        for dependency in slice.dylibs {
            if let Some(relative) = dependency.strip_prefix("@rpath/") {
                if relative.is_empty() {
                    return Err("Mach-O @rpath dependency has an empty suffix".into());
                }
                let mut candidates = Vec::with_capacity(slice.rpaths.len());
                for rpath in &slice.rpaths {
                    let base = expand_runpath(rpath, image_dir)
                        .ok_or_else(|| format!("cannot resolve Mach-O LC_RPATH entry {rpath:?}"))?;
                    candidates.push(base.join(relative));
                }
                groups.push(candidates);
            } else if let Some(path) = expand_image_path(&dependency, image_dir) {
                groups.push(vec![path]);
            } else if dependency.starts_with('@') {
                // Unknown dyld tokens cannot be proven to refer to a framework
                // we are changing. Do not reinterpret them as filesystem paths.
                groups.push(Vec::new());
            } else if Path::new(&dependency).is_absolute() {
                groups.push(vec![PathBuf::from(dependency)]);
            } else {
                groups.push(Vec::new());
            }
        }
    }
    Ok(groups)
}

struct SliceDylibCommands {
    rpaths: Vec<String>,
    dylibs: Vec<String>,
}

fn parse_linked_dylib_commands(bytes: &[u8]) -> Result<Vec<SliceDylibCommands>, String> {
    let slices = parse_slices(bytes)?;
    let mut parsed_slices = Vec::with_capacity(slices.len());

    for (slice_index, slice) in slices.iter().enumerate() {
        let slice_end = slice
            .offset
            .checked_add(slice.size)
            .ok_or_else(|| format!("Mach-O slice {slice_index} range overflow"))?;
        require_range(bytes.len(), slice.offset, slice.size, "Mach-O slice")?;
        let slice_bytes = &bytes[slice.offset..slice_end];
        let (order, cpu_type) = macho_header(slice_bytes)?;
        validate_cpu_type(cpu_type)?;
        if slice
            .fat_cpu_type
            .is_some_and(|fat_cpu| fat_cpu != cpu_type)
        {
            return Err(format!(
                "fat Mach-O architecture CPU type does not match slice {slice_index} header"
            ));
        }

        let ncmds = read_u32(slice_bytes, 16, order)? as usize;
        let sizeofcmds = read_u32(slice_bytes, 20, order)? as usize;
        let load_start = MACH_HEADER_64_SIZE;
        let load_end = load_start
            .checked_add(sizeofcmds)
            .ok_or_else(|| format!("Mach-O slice {slice_index} load-command range overflow"))?;
        require_range(
            slice_bytes.len(),
            load_start,
            sizeofcmds,
            "Mach-O load commands",
        )?;
        if ncmds > sizeofcmds / 8 {
            return Err(format!(
                "Mach-O slice {slice_index} command count exceeds its load-command data"
            ));
        }

        let mut cursor = load_start;
        let mut rpaths = Vec::new();
        let mut dylibs = Vec::new();
        for command_index in 0..ncmds {
            require_range(load_end, cursor, 8, "Mach-O load-command header")?;
            let command = read_u32(slice_bytes, cursor, order)?;
            let command_size = read_u32(slice_bytes, cursor + 4, order)? as usize;
            if command_size < 8 || !command_size.is_multiple_of(8) {
                return Err(format!(
                    "Mach-O slice {slice_index} load command {command_index} has invalid size"
                ));
            }
            require_range(load_end, cursor, command_size, "Mach-O load command")?;

            if command == LC_RPATH {
                if command_size < RPATH_COMMAND_SIZE {
                    return Err(format!(
                        "Mach-O slice {slice_index} LC_RPATH command {command_index} is truncated"
                    ));
                }
                let path_offset = read_u32(slice_bytes, cursor + 8, order)? as usize;
                rpaths.push(read_load_command_string(
                    slice_bytes,
                    cursor,
                    command_size,
                    path_offset,
                    RPATH_COMMAND_SIZE,
                    (slice_index, command_index),
                    "LC_RPATH path",
                )?);
            } else if matches!(
                command,
                LC_LOAD_DYLIB
                    | LC_LOAD_WEAK_DYLIB
                    | LC_REEXPORT_DYLIB
                    | LC_LAZY_LOAD_DYLIB
                    | LC_LOAD_UPWARD_DYLIB
            ) {
                if command_size < DYLIB_COMMAND_SIZE {
                    return Err(format!(
                        "Mach-O slice {slice_index} dylib command {command_index} is truncated"
                    ));
                }
                let name_offset = read_u32(slice_bytes, cursor + 8, order)? as usize;
                dylibs.push(read_load_command_string(
                    slice_bytes,
                    cursor,
                    command_size,
                    name_offset,
                    DYLIB_COMMAND_SIZE,
                    (slice_index, command_index),
                    "dylib name",
                )?);
            }
            cursor += command_size;
        }
        if cursor != load_end {
            return Err(format!(
                "Mach-O slice {slice_index} load-command sizes do not match sizeofcmds"
            ));
        }
        parsed_slices.push(SliceDylibCommands { rpaths, dylibs });
    }

    Ok(parsed_slices)
}

fn read_load_command_string(
    slice: &[u8],
    command_offset: usize,
    command_size: usize,
    string_offset: usize,
    fixed_size: usize,
    command_indices: (usize, usize),
    label: &str,
) -> Result<String, String> {
    let (slice_index, command_index) = command_indices;
    if !(fixed_size..command_size).contains(&string_offset) {
        return Err(format!(
            "Mach-O slice {slice_index} load command {command_index} {label} offset is out of range"
        ));
    }
    let command_end = command_offset
        .checked_add(command_size)
        .ok_or_else(|| format!("Mach-O {label} command range overflow"))?;
    let string_start = command_offset
        .checked_add(string_offset)
        .ok_or_else(|| format!("Mach-O {label} offset overflow"))?;
    require_range(
        slice.len(),
        string_start,
        command_end.saturating_sub(string_start),
        label,
    )?;
    let string_region = &slice[string_start..command_end];
    let terminator = string_region
        .iter()
        .position(|byte| *byte == 0)
        .ok_or_else(|| {
            format!(
            "Mach-O slice {slice_index} load command {command_index} {label} is not NUL-terminated"
        )
        })?;
    if terminator == 0 {
        return Err(format!(
            "Mach-O slice {slice_index} load command {command_index} has an empty {label}"
        ));
    }
    std::str::from_utf8(&string_region[..terminator])
        .map(str::to_owned)
        .map_err(|error| {
            format!(
                "Mach-O slice {slice_index} load command {command_index} {label} is not UTF-8: {error}"
            )
        })
}

fn expand_image_path(path: &str, image_dir: &Path) -> Option<PathBuf> {
    for token in ["@loader_path", "@executable_path"] {
        if path == token {
            return Some(image_dir.to_path_buf());
        }
        if let Some(relative) = path.strip_prefix(&format!("{token}/")) {
            return Some(image_dir.join(relative));
        }
    }
    None
}

fn expand_runpath(path: &str, image_dir: &Path) -> Option<PathBuf> {
    if Path::new(path).is_absolute() {
        return Some(PathBuf::from(path));
    }
    expand_image_path(path, image_dir)
}

pub(crate) fn dynamic_framework_load_paths(bytes: &[u8]) -> Result<Vec<String>, String> {
    const LC_SEGMENT_64: u32 = 0x19;
    const N_EXT: u8 = 0x01;
    const N_TYPE: u8 = 0x0e;
    const N_UNDF: u8 = 0x00;
    const N_STAB: u8 = 0xe0;
    const CSTRING_PATH_PREFIX: &[u8] = b"../../../../";

    let slices = parse_slices(bytes)?;
    let mut paths = Vec::new();

    for (slice_index, slice) in slices.iter().enumerate() {
        let slice_end = slice
            .offset
            .checked_add(slice.size)
            .ok_or_else(|| format!("Mach-O slice {slice_index} range overflow"))?;
        require_range(bytes.len(), slice.offset, slice.size, "Mach-O slice")?;
        let slice_bytes = &bytes[slice.offset..slice_end];
        let (order, cpu_type) = macho_header(slice_bytes)?;
        validate_cpu_type(cpu_type)?;
        if slice
            .fat_cpu_type
            .is_some_and(|fat_cpu| fat_cpu != cpu_type)
        {
            return Err(format!(
                "fat Mach-O architecture CPU type does not match slice {slice_index} header"
            ));
        }

        let ncmds = read_u32(slice_bytes, 16, order)? as usize;
        let sizeofcmds = read_u32(slice_bytes, 20, order)? as usize;
        let load_start = MACH_HEADER_64_SIZE;
        let load_end = load_start
            .checked_add(sizeofcmds)
            .ok_or_else(|| format!("Mach-O slice {slice_index} load-command range overflow"))?;
        require_range(
            slice_bytes.len(),
            load_start,
            sizeofcmds,
            "Mach-O load commands",
        )?;
        if ncmds > sizeofcmds / 8 {
            return Err(format!(
                "Mach-O slice {slice_index} command count exceeds its load-command data"
            ));
        }

        let mut cursor = load_start;
        let mut text_string_sections = Vec::new();
        let mut symtab = None;
        for command_index in 0..ncmds {
            require_range(load_end, cursor, 8, "Mach-O load-command header")?;
            let command = read_u32(slice_bytes, cursor, order)?;
            let command_size = read_u32(slice_bytes, cursor + 4, order)? as usize;
            if command_size < 8 || !command_size.is_multiple_of(8) {
                return Err(format!(
                    "Mach-O slice {slice_index} load command {command_index} has invalid size"
                ));
            }
            require_range(load_end, cursor, command_size, "Mach-O load command")?;

            match command {
                LC_SEGMENT_64 => inspect_dynamic_text_segment(
                    slice_bytes,
                    order,
                    cursor,
                    command_size,
                    command_index,
                    &mut text_string_sections,
                )?,
                LC_SYMTAB => {
                    if command_size != SYMTAB_COMMAND_SIZE {
                        return Err(format!(
                            "Mach-O slice {slice_index} LC_SYMTAB command {command_index} has invalid size"
                        ));
                    }
                    if symtab.is_some() {
                        return Err(format!(
                            "Mach-O slice {slice_index} has duplicate LC_SYMTAB commands"
                        ));
                    }
                    symtab = Some((
                        read_u32(slice_bytes, cursor + 8, order)? as usize,
                        read_u32(slice_bytes, cursor + 12, order)? as usize,
                        read_u32(slice_bytes, cursor + 16, order)? as usize,
                        read_u32(slice_bytes, cursor + 20, order)? as usize,
                    ));
                }
                _ => {}
            }
            cursor += command_size;
        }
        if cursor != load_end {
            return Err(format!(
                "Mach-O slice {slice_index} load-command sizes do not match sizeofcmds"
            ));
        }

        let mut strings = Vec::new();
        for section in &text_string_sections {
            let end = section
                .offset
                .checked_add(section.size)
                .ok_or_else(|| "Mach-O text string section range overflow".to_string())?;
            require_range(
                slice_bytes.len(),
                section.offset,
                section.size,
                "Mach-O text string section",
            )?;
            let data = &slice_bytes[section.offset..end];
            strings.extend(parse_bounded_c_strings(
                data,
                section.allow_non_string_tail,
                slice_index,
                section.name,
            )?);
        }

        let (has_dlopen, has_dlsym) = if let Some((
            symbol_offset,
            symbol_count,
            string_offset,
            string_size,
        )) = symtab
        {
            let symbol_bytes_size = symbol_count
                .checked_mul(NLIST_64_SIZE)
                .ok_or_else(|| format!("Mach-O slice {slice_index} nlist64 table size overflow"))?;
            require_range(
                slice_bytes.len(),
                symbol_offset,
                symbol_bytes_size,
                "Mach-O nlist64 symbol table",
            )?;
            require_range(
                slice_bytes.len(),
                string_offset,
                string_size,
                "Mach-O symbol string table",
            )?;
            let symbol_strings = &slice_bytes[string_offset..string_offset + string_size];
            let mut has_dlopen = false;
            let mut has_dlsym = false;
            for symbol_index in 0..symbol_count {
                let entry_offset = symbol_offset
                    .checked_add(symbol_index * NLIST_64_SIZE)
                    .ok_or_else(|| "Mach-O nlist64 entry offset overflow".to_string())?;
                let string_index = read_u32(slice_bytes, entry_offset, order)? as usize;
                let n_type = slice_bytes[entry_offset + 4];
                if string_index == 0 {
                    continue;
                }
                if string_index >= symbol_strings.len() {
                    return Err(format!(
                        "Mach-O slice {slice_index} symbol {symbol_index} string index is out of range"
                    ));
                }
                let name_region = &symbol_strings[string_index..];
                let name_end = name_region.iter().position(|byte| *byte == 0).ok_or_else(|| {
                    format!(
                        "Mach-O slice {slice_index} symbol {symbol_index} name is not NUL-terminated"
                    )
                })?;
                let is_undefined_external =
                    n_type & N_EXT != 0 && n_type & N_TYPE == N_UNDF && n_type & N_STAB == 0;
                if is_undefined_external {
                    match &name_region[..name_end] {
                        b"_dlopen" => has_dlopen = true,
                        b"_dlsym" => has_dlsym = true,
                        _ => {}
                    }
                }
            }
            (has_dlopen, has_dlsym)
        } else {
            (false, false)
        };

        if !has_dlopen || !has_dlsym || !strings.iter().any(|value| *value == b"ChromeMain") {
            continue;
        }
        for value in strings {
            let Some(executable_name) = value.strip_prefix(CSTRING_PATH_PREFIX) else {
                continue;
            };
            if executable_name.is_empty() {
                continue;
            }
            let path = std::str::from_utf8(value).map_err(|error| {
                format!("Mach-O slice {slice_index} framework loader path is not UTF-8: {error}")
            })?;
            paths.push(path.to_owned());
        }
    }

    Ok(paths)
}

#[derive(Clone, Copy)]
struct TextStringSection {
    offset: usize,
    size: usize,
    name: &'static str,
    allow_non_string_tail: bool,
}

fn inspect_dynamic_text_segment(
    slice: &[u8],
    order: ByteOrder,
    command_offset: usize,
    command_size: usize,
    command_index: usize,
    text_string_sections: &mut Vec<TextStringSection>,
) -> Result<(), String> {
    if command_size < SEGMENT_COMMAND_64_SIZE {
        return Err(format!(
            "Mach-O LC_SEGMENT_64 command {command_index} is truncated"
        ));
    }
    let nsects = read_u32(slice, command_offset + 64, order)? as usize;
    let expected_size = nsects
        .checked_mul(SECTION_64_SIZE)
        .and_then(|sections| SEGMENT_COMMAND_64_SIZE.checked_add(sections))
        .ok_or_else(|| format!("Mach-O segment command {command_index} size overflow"))?;
    if command_size != expected_size {
        return Err(format!(
            "Mach-O LC_SEGMENT_64 command {command_index} has inconsistent section count"
        ));
    }

    let segment_name = &slice[command_offset + 8..command_offset + 24];
    let is_text_segment = fixed_name_eq(segment_name, b"__TEXT");
    let segment_file_offset = read_u64(slice, command_offset + 40, order)?;
    let segment_file_size = read_u64(slice, command_offset + 48, order)?;
    let (segment_start, segment_end) = if is_text_segment {
        let segment_start = usize::try_from(segment_file_offset)
            .map_err(|_| "__TEXT file offset is not representable".to_string())?;
        let segment_size = usize::try_from(segment_file_size)
            .map_err(|_| "__TEXT file size is not representable".to_string())?;
        require_range(
            slice.len(),
            segment_start,
            segment_size,
            "__TEXT segment file range",
        )?;
        let segment_end = segment_start
            .checked_add(segment_size)
            .ok_or_else(|| "__TEXT segment file range overflow".to_string())?;
        (segment_start, segment_end)
    } else {
        (0, 0)
    };

    for section_index in 0..nsects {
        let section_offset =
            command_offset + SEGMENT_COMMAND_64_SIZE + section_index * SECTION_64_SIZE;
        let section_name = &slice[section_offset..section_offset + 16];
        let section_segment_name = &slice[section_offset + 16..section_offset + 32];
        let (name, allow_non_string_tail) = if fixed_name_eq(section_name, b"__cstring") {
            ("__cstring", false)
        } else if fixed_name_eq(section_name, b"__const") {
            ("__const", true)
        } else {
            continue;
        };
        let section_claims_text = fixed_name_eq(section_segment_name, b"__TEXT");
        if section_claims_text != is_text_segment {
            return Err(format!(
                "Mach-O segment command {command_index} has a {name} section with a mismatched segment name"
            ));
        }
        if !is_text_segment {
            continue;
        }
        if text_string_sections
            .iter()
            .any(|existing| existing.name == name)
        {
            return Err(format!("duplicate __TEXT,{name} sections in Mach-O slice"));
        }

        let size = usize::try_from(read_u64(slice, section_offset + 40, order)?)
            .map_err(|_| format!("__TEXT,{name} size is not representable"))?;
        let file_offset = read_u32(slice, section_offset + 48, order)? as usize;
        require_range(
            slice.len(),
            file_offset,
            size,
            &format!("Mach-O __TEXT,{name} section range"),
        )?;
        let section_end = file_offset
            .checked_add(size)
            .ok_or_else(|| format!("__TEXT,{name} section range overflow"))?;
        if file_offset < segment_start || section_end > segment_end {
            return Err(format!(
                "__TEXT,{name} section lies outside the __TEXT segment"
            ));
        }
        text_string_sections.push(TextStringSection {
            offset: file_offset,
            size,
            name,
            allow_non_string_tail,
        });
    }
    Ok(())
}

fn parse_bounded_c_strings<'a>(
    data: &'a [u8],
    allow_non_string_tail: bool,
    slice_index: usize,
    section_name: &str,
) -> Result<Vec<&'a [u8]>, String> {
    if !allow_non_string_tail && !data.is_empty() && data.last() != Some(&0) {
        return Err(format!(
            "Mach-O slice {slice_index} __TEXT,{section_name} section is not NUL-terminated"
        ));
    }

    let mut strings = Vec::new();
    let mut start = 0;
    for (index, byte) in data.iter().enumerate() {
        if *byte == 0 {
            if index > start {
                strings.push(&data[start..index]);
            }
            start = index + 1;
        }
    }
    // __const contains non-string constants too. Only complete NUL-terminated
    // entries are candidates; an unterminated trailing constant is ignored.
    Ok(strings)
}

fn integrity_dictionary_digest(map: &Value) -> Result<[u8; SLOT_DIGEST_SIZE], String> {
    let entries = map
        .as_object()
        .ok_or_else(|| "ElectronAsarIntegrity plist value is not a dictionary".to_string())?;
    let mut keys: Vec<&str> = entries.keys().map(String::as_str).collect();
    keys.sort_by(|left, right| left.as_bytes().cmp(right.as_bytes()));

    let mut hasher = Sha256::new();
    for key in keys {
        let entry = entries[key]
            .as_object()
            .ok_or_else(|| format!("ElectronAsarIntegrity entry {key:?} is not a dictionary"))?;
        let algorithm = entry
            .get("algorithm")
            .and_then(Value::as_str)
            .ok_or_else(|| {
                format!("ElectronAsarIntegrity entry {key:?} has no string algorithm")
            })?;
        let hash = entry
            .get("hash")
            .and_then(Value::as_str)
            .ok_or_else(|| format!("ElectronAsarIntegrity entry {key:?} has no string hash"))?;
        hasher.update(key.as_bytes());
        hasher.update(algorithm.as_bytes());
        hasher.update(hash.as_bytes());
    }
    Ok(hasher.finalize().into())
}

#[derive(Clone, Copy)]
enum ByteOrder {
    Little,
    Big,
}

#[derive(Clone, Copy)]
struct MachSlice {
    offset: usize,
    size: usize,
    fat_cpu_type: Option<u32>,
}

#[derive(Clone, Copy)]
enum SlotObservation {
    Missing,
    Unused,
    Active {
        digest: [u8; SLOT_DIGEST_SIZE],
        digest_offset: usize,
    },
}

fn parse_slices(bytes: &[u8]) -> Result<Vec<MachSlice>, String> {
    if bytes.len() < 4 {
        return Err("Mach-O input is shorter than its magic".into());
    }

    match fat_magic(bytes) {
        Some((order, is_64)) => parse_fat_slices(bytes, order, is_64),
        None => Ok(vec![MachSlice {
            offset: 0,
            size: bytes.len(),
            fat_cpu_type: None,
        }]),
    }
}

fn fat_magic(bytes: &[u8]) -> Option<(ByteOrder, bool)> {
    let magic = bytes.get(..4)?;
    let big = u32::from_be_bytes(magic.try_into().ok()?);
    if big == FAT_MAGIC {
        return Some((ByteOrder::Big, false));
    }
    if big == FAT_MAGIC_64 {
        return Some((ByteOrder::Big, true));
    }
    let little = u32::from_le_bytes(magic.try_into().ok()?);
    if little == FAT_MAGIC {
        return Some((ByteOrder::Little, false));
    }
    if little == FAT_MAGIC_64 {
        return Some((ByteOrder::Little, true));
    }
    None
}

fn parse_fat_slices(bytes: &[u8], order: ByteOrder, is_64: bool) -> Result<Vec<MachSlice>, String> {
    require_range(bytes.len(), 0, 8, "fat Mach-O header")?;
    let count = read_u32(bytes, 4, order)? as usize;
    if count == 0 {
        return Err("fat Mach-O has no architecture slices".into());
    }
    let entry_size = if is_64 { 32_usize } else { 20_usize };
    let table_size = count
        .checked_mul(entry_size)
        .ok_or_else(|| "fat Mach-O architecture table size overflow".to_string())?;
    let table_end = 8_usize
        .checked_add(table_size)
        .ok_or_else(|| "fat Mach-O architecture table range overflow".to_string())?;
    require_range(bytes.len(), 8, table_size, "fat Mach-O architecture table")?;

    let mut slices = Vec::with_capacity(count);
    for index in 0..count {
        let entry = 8 + index * entry_size;
        let cpu_type = read_u32(bytes, entry, order)?;
        validate_cpu_type(cpu_type)?;
        let (offset, size, align) = if is_64 {
            (
                read_u64(bytes, entry + 8, order)?,
                read_u64(bytes, entry + 16, order)?,
                read_u32(bytes, entry + 24, order)?,
            )
        } else {
            (
                read_u32(bytes, entry + 8, order)? as u64,
                read_u32(bytes, entry + 12, order)? as u64,
                read_u32(bytes, entry + 16, order)?,
            )
        };
        if is_64 {
            let reserved = read_u32(bytes, entry + 28, order)?;
            if reserved != 0 {
                return Err(format!(
                    "fat Mach-O slice {index} has nonzero reserved bits"
                ));
            }
        }
        let alignment = 1_u64
            .checked_shl(align)
            .ok_or_else(|| format!("fat Mach-O slice {index} has invalid alignment exponent"))?;
        if offset % alignment != 0 {
            return Err(format!(
                "fat Mach-O slice {index} offset violates its alignment"
            ));
        }
        let offset = usize::try_from(offset)
            .map_err(|_| format!("fat Mach-O slice {index} offset is not representable"))?;
        let size = usize::try_from(size)
            .map_err(|_| format!("fat Mach-O slice {index} size is not representable"))?;
        if offset < table_end {
            return Err(format!(
                "fat Mach-O slice {index} overlaps its architecture table"
            ));
        }
        require_range(
            bytes.len(),
            offset,
            size,
            &format!("fat Mach-O slice {index}"),
        )?;
        if size < MACH_HEADER_64_SIZE {
            return Err(format!(
                "fat Mach-O slice {index} is shorter than mach_header_64"
            ));
        }
        slices.push(MachSlice {
            offset,
            size,
            fat_cpu_type: Some(cpu_type),
        });
    }

    let mut ranges: Vec<(usize, usize, usize)> = slices
        .iter()
        .enumerate()
        .map(|(index, slice)| (slice.offset, slice.offset + slice.size, index))
        .collect();
    ranges.sort_by_key(|range| range.0);
    for pair in ranges.windows(2) {
        if pair[0].1 > pair[1].0 {
            return Err(format!(
                "fat Mach-O slices {} and {} overlap",
                pair[0].2, pair[1].2
            ));
        }
    }
    Ok(slices)
}

fn inspect_integrity_slot(bytes: &[u8], slice: MachSlice) -> Result<SlotObservation, String> {
    let slice_end = slice
        .offset
        .checked_add(slice.size)
        .ok_or_else(|| "Mach-O slice range overflow".to_string())?;
    require_range(bytes.len(), slice.offset, slice.size, "Mach-O slice")?;
    let slice_bytes = &bytes[slice.offset..slice_end];
    let (order, cpu_type) = macho_header(slice_bytes)?;
    validate_cpu_type(cpu_type)?;
    if slice
        .fat_cpu_type
        .is_some_and(|fat_cpu| fat_cpu != cpu_type)
    {
        return Err("fat Mach-O architecture CPU type does not match its slice header".into());
    }

    let ncmds = read_u32(slice_bytes, 16, order)? as usize;
    let sizeofcmds = read_u32(slice_bytes, 20, order)? as usize;
    let load_start = MACH_HEADER_64_SIZE;
    let load_end = load_start
        .checked_add(sizeofcmds)
        .ok_or_else(|| "Mach-O load-command range overflow".to_string())?;
    require_range(
        slice_bytes.len(),
        load_start,
        sizeofcmds,
        "Mach-O load commands",
    )?;
    if ncmds > sizeofcmds / 8 {
        return Err("Mach-O command count exceeds its load-command data".into());
    }

    let mut cursor = load_start;
    let mut target_section: Option<(usize, usize)> = None;
    for command_index in 0..ncmds {
        require_range(load_end, cursor, 8, "Mach-O load-command header")?;
        let command = read_u32(slice_bytes, cursor, order)?;
        let command_size = read_u32(slice_bytes, cursor + 4, order)? as usize;
        if command_size < 8 || !command_size.is_multiple_of(8) {
            return Err(format!(
                "Mach-O load command {command_index} has invalid size"
            ));
        }
        require_range(load_end, cursor, command_size, "Mach-O load command")?;

        if command == SEGMENT_COMMAND_64 {
            inspect_segment_command(
                slice_bytes,
                order,
                cursor,
                command_size,
                command_index,
                &mut target_section,
            )?;
        }
        cursor += command_size;
    }
    if cursor != load_end {
        return Err("Mach-O load-command sizes do not match sizeofcmds".into());
    }

    let Some((section_offset, section_size)) = target_section else {
        return Ok(SlotObservation::Missing);
    };
    let section_end = section_offset
        .checked_add(section_size)
        .ok_or_else(|| "ASAR integrity section range overflow".to_string())?;
    let section_bytes = &slice_bytes[section_offset..section_end];
    let matches: Vec<usize> = section_bytes
        .windows(SLOT_SENTINEL.len())
        .enumerate()
        .filter_map(|(index, bytes)| (bytes == SLOT_SENTINEL).then_some(index))
        .collect();
    match matches.as_slice() {
        [] => Ok(SlotObservation::Missing),
        [relative_offset] => {
            require_range(
                section_bytes.len(),
                *relative_offset,
                SLOT_SIZE,
                "ASAR integrity slot",
            )?;
            let used = section_bytes[relative_offset + SLOT_SENTINEL.len()];
            let version = section_bytes[relative_offset + SLOT_SENTINEL.len() + 1];
            match used {
                0 => Ok(SlotObservation::Unused),
                1 => {
                    if version != 1 {
                        return Err(format!(
                            "unsupported active ASAR integrity slot version {version}"
                        ));
                    }
                    let digest_start = relative_offset + SLOT_SENTINEL.len() + 2;
                    let digest = section_bytes[digest_start..digest_start + SLOT_DIGEST_SIZE]
                        .try_into()
                        .expect("range checked above");
                    let digest_offset = slice
                        .offset
                        .checked_add(section_offset)
                        .and_then(|offset| offset.checked_add(digest_start))
                        .ok_or_else(|| "ASAR integrity digest offset overflow".to_string())?;
                    Ok(SlotObservation::Active {
                        digest,
                        digest_offset,
                    })
                }
                other => Err(format!("invalid ASAR integrity slot used value {other}")),
            }
        }
        _ => Err("duplicate ASAR integrity slots in __DATA_CONST,__asar_integrity".into()),
    }
}

fn inspect_segment_command(
    slice: &[u8],
    order: ByteOrder,
    command_offset: usize,
    command_size: usize,
    command_index: usize,
    target_section: &mut Option<(usize, usize)>,
) -> Result<(), String> {
    if command_size < SEGMENT_COMMAND_64_SIZE {
        return Err(format!(
            "Mach-O LC_SEGMENT_64 command {command_index} is truncated"
        ));
    }
    let nsects = read_u32(slice, command_offset + 64, order)? as usize;
    let expected_size = nsects
        .checked_mul(SECTION_64_SIZE)
        .and_then(|sections| SEGMENT_COMMAND_64_SIZE.checked_add(sections))
        .ok_or_else(|| format!("Mach-O segment command {command_index} size overflow"))?;
    if command_size != expected_size {
        return Err(format!(
            "Mach-O LC_SEGMENT_64 command {command_index} has inconsistent section count"
        ));
    }

    let segment_name = &slice[command_offset + 8..command_offset + 24];
    let is_data_const = fixed_name_eq(segment_name, b"__DATA_CONST");
    let segment_file_offset = read_u64(slice, command_offset + 40, order)?;
    let segment_file_size = read_u64(slice, command_offset + 48, order)?;
    if is_data_const {
        let segment_start = usize::try_from(segment_file_offset)
            .map_err(|_| "__DATA_CONST file offset is not representable".to_string())?;
        let segment_size = usize::try_from(segment_file_size)
            .map_err(|_| "__DATA_CONST file size is not representable".to_string())?;
        require_range(
            slice.len(),
            segment_start,
            segment_size,
            "__DATA_CONST segment file range",
        )?;

        for section_index in 0..nsects {
            let section_offset =
                command_offset + SEGMENT_COMMAND_64_SIZE + section_index * SECTION_64_SIZE;
            let section_name = &slice[section_offset..section_offset + 16];
            let section_segment_name = &slice[section_offset + 16..section_offset + 32];
            if !fixed_name_eq(section_name, b"__asar_integrity")
                || !fixed_name_eq(section_segment_name, b"__DATA_CONST")
            {
                continue;
            }
            if target_section.is_some() {
                return Err(
                    "duplicate __DATA_CONST,__asar_integrity sections in Mach-O slice".into(),
                );
            }

            let size = read_u64(slice, section_offset + 40, order)?;
            let file_offset = read_u32(slice, section_offset + 48, order)? as usize;
            let size = usize::try_from(size)
                .map_err(|_| "ASAR integrity section size is not representable".to_string())?;
            require_range(
                slice.len(),
                file_offset,
                size,
                "ASAR integrity section file range",
            )?;
            let segment_end = segment_start
                .checked_add(segment_size)
                .ok_or_else(|| "__DATA_CONST segment file range overflow".to_string())?;
            let section_end = file_offset
                .checked_add(size)
                .ok_or_else(|| "ASAR integrity section range overflow".to_string())?;
            if file_offset < segment_start || section_end > segment_end {
                return Err("ASAR integrity section lies outside __DATA_CONST file range".into());
            }
            *target_section = Some((file_offset, size));
        }
    }
    Ok(())
}

fn macho_header(slice: &[u8]) -> Result<(ByteOrder, u32), String> {
    if slice.len() < MACH_HEADER_64_SIZE {
        return Err("Mach-O slice is shorter than mach_header_64".into());
    }
    let magic = &slice[..4];
    let order = if magic == [0xcf, 0xfa, 0xed, 0xfe] {
        ByteOrder::Little
    } else if magic == [0xfe, 0xed, 0xfa, 0xcf] {
        ByteOrder::Big
    } else {
        return Err("unsupported or invalid Mach-O magic".into());
    };
    let cpu_type = read_u32(slice, 4, order)?;
    Ok((order, cpu_type))
}

fn validate_cpu_type(cpu_type: u32) -> Result<(), String> {
    match cpu_type {
        CPU_TYPE_ARM64 | CPU_TYPE_X86_64 => Ok(()),
        other => Err(format!("unsupported Mach-O CPU type 0x{other:08x}")),
    }
}

fn fixed_name_eq(bytes: &[u8], name: &[u8]) -> bool {
    bytes.len() == 16
        && name.len() <= 16
        && bytes[..name.len()] == *name
        && bytes[name.len()..].iter().all(|byte| *byte == 0)
}

fn read_u32(bytes: &[u8], offset: usize, order: ByteOrder) -> Result<u32, String> {
    require_range(bytes.len(), offset, 4, "Mach-O u32 field")?;
    let field: [u8; 4] = bytes[offset..offset + 4]
        .try_into()
        .expect("range checked above");
    Ok(match order {
        ByteOrder::Little => u32::from_le_bytes(field),
        ByteOrder::Big => u32::from_be_bytes(field),
    })
}

fn read_u64(bytes: &[u8], offset: usize, order: ByteOrder) -> Result<u64, String> {
    require_range(bytes.len(), offset, 8, "Mach-O u64 field")?;
    let field: [u8; 8] = bytes[offset..offset + 8]
        .try_into()
        .expect("range checked above");
    Ok(match order {
        ByteOrder::Little => u64::from_le_bytes(field),
        ByteOrder::Big => u64::from_be_bytes(field),
    })
}

fn require_range(length: usize, offset: usize, size: usize, label: &str) -> Result<(), String> {
    let end = offset
        .checked_add(size)
        .ok_or_else(|| format!("{label} range overflow"))?;
    if end > length {
        return Err(format!("{label} is out of range"));
    }
    Ok(())
}
