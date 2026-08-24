/**
 * [INPUT]: 接收 /dev/urandom 原始熵，或用于测试的 Read 实现
 * [OUTPUT]: 对外提供经过 SHA-256 混合的两词临时名称与可测试熵读入
 * [POS]: incodex-cli 的临时身份命名边界；维护 ASCII Title Case 词表并拒绝熵源降级
 * [PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md
 */
use std::fs::File;
use std::io::{self, Read};

use sha2::{Digest, Sha256};

const ENTROPY_BYTES: usize = 32;
const ADJECTIVES: [&str; 64] = [
    "Quiet",
    "Bright",
    "Gentle",
    "Hidden",
    "Amber",
    "Swift",
    "Mellow",
    "Clever",
    "Soft",
    "Calm",
    "Nimble",
    "Kind",
    "Lunar",
    "Mild",
    "Brave",
    "Cozy",
    "Silent",
    "Warm",
    "Clear",
    "Fresh",
    "Steady",
    "Dusky",
    "Sunny",
    "Misty",
    "Sincere",
    "Patient",
    "Peaceful",
    "Polished",
    "Serene",
    "Tender",
    "Vivid",
    "Airy",
    "Sturdy",
    "Thoughtful",
    "Lucky",
    "Honest",
    "Jolly",
    "Dappled",
    "Velvet",
    "Golden",
    "Silver",
    "Azure",
    "Coral",
    "Ivory",
    "Fuzzy",
    "Breezy",
    "Crystal",
    "Mossy",
    "Crisp",
    "Hushed",
    "Humble",
    "Neat",
    "Noble",
    "Radiant",
    "Rustic",
    "Shy",
    "Teal",
    "Tidy",
    "Tranquil",
    "Whispering",
    "Zephyr",
    "Kindred",
    "Willowy",
    "Zesty",
];
const ANIMALS: [&str; 64] = [
    "Otter",
    "Badger",
    "Robin",
    "Panda",
    "Fox",
    "Koala",
    "Lark",
    "Finch",
    "Beaver",
    "Rabbit",
    "Heron",
    "Lynx",
    "Seal",
    "Wren",
    "Sable",
    "Bison",
    "Dolphin",
    "Puffin",
    "Turtle",
    "Sparrow",
    "Falcon",
    "Ferret",
    "Hedgehog",
    "Marmot",
    "Moose",
    "Raccoon",
    "Walrus",
    "Penguin",
    "Goose",
    "Pelican",
    "Quail",
    "Salmon",
    "Trout",
    "Gazelle",
    "Kangaroo",
    "Lemur",
    "Manatee",
    "Meerkat",
    "Ocelot",
    "Parakeet",
    "Reindeer",
    "Seahorse",
    "Starling",
    "Swan",
    "Vole",
    "Wombat",
    "Yak",
    "Zebra",
    "Aardvark",
    "Alpaca",
    "Bluebird",
    "Chinchilla",
    "Cormorant",
    "Crane",
    "Egret",
    "Flamingo",
    "Ibis",
    "Jaguar",
    "Kestrel",
    "Numbat",
    "Nuthatch",
    "Tarsier",
    "Possum",
    "Woodpecker",
];

pub(crate) fn friendly_name_from_entropy_digest(digest: &[u8; ENTROPY_BYTES]) -> String {
    let adjective = ADJECTIVES[digest[0] as usize % ADJECTIVES.len()];
    let animal = ANIMALS[digest[1] as usize % ANIMALS.len()];
    format!("{adjective} {animal}")
}

pub(crate) fn friendly_name_from_reader(mut reader: impl Read) -> io::Result<String> {
    let mut entropy = [0_u8; ENTROPY_BYTES];
    reader.read_exact(&mut entropy)?;
    let digest = Sha256::digest(entropy);
    let mut digest_array = [0_u8; ENTROPY_BYTES];
    digest_array.copy_from_slice(&digest);
    Ok(friendly_name_from_entropy_digest(&digest_array))
}

pub(crate) fn random_friendly_name() -> Result<String, String> {
    let file = File::open("/dev/urandom")
        .map_err(|error| format!("cannot open secure random source /dev/urandom: {error}"))?;
    friendly_name_from_reader(file)
        .map_err(|error| format!("cannot read secure random source /dev/urandom: {error}"))
}

#[cfg(test)]
mod tests {
    use std::io::{self, Read};

    use super::{
        friendly_name_from_entropy_digest, friendly_name_from_reader, ADJECTIVES, ANIMALS,
    };

    #[test]
    fn deterministic_entropy_produces_two_title_case_words_without_hex_placeholder() {
        let digest = [0_u8; 32];
        let name = friendly_name_from_entropy_digest(&digest);
        let words: Vec<&str> = name.split(' ').collect();

        assert_eq!(name, "Quiet Otter");
        assert_eq!(words.len(), 2);
        assert!(name.len() <= 64);
        assert!(words.iter().all(|word| word
            .chars()
            .all(|character| character.is_ascii_alphabetic())));
        assert!(!name.starts_with("Incognito "));
    }

    #[test]
    fn identical_entropy_is_stable_and_distinct_entropy_can_change_the_name() {
        let first = friendly_name_from_entropy_digest(&[0_u8; 32]);
        let second = friendly_name_from_entropy_digest(&[0_u8; 32]);
        let other = friendly_name_from_entropy_digest(&[255_u8; 32]);

        assert_eq!(first, second);
        assert_ne!(first, other);
    }

    #[test]
    fn entropy_reader_returns_io_failures_instead_of_using_process_fallbacks() {
        struct FailingReader;

        impl Read for FailingReader {
            fn read(&mut self, _buffer: &mut [u8]) -> io::Result<usize> {
                Err(io::Error::new(io::ErrorKind::PermissionDenied, "blocked"))
            }
        }

        let error = friendly_name_from_reader(FailingReader).unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::PermissionDenied);
    }

    #[test]
    fn reviewed_word_lists_have_power_of_two_width_for_a_wide_collision_space() {
        assert!(ADJECTIVES.len() >= 64);
        assert!(ANIMALS.len() >= 64);
        assert!(ADJECTIVES.len().is_power_of_two());
        assert!(ANIMALS.len().is_power_of_two());
        assert_eq!(ADJECTIVES[60], "Zealous");
    }
}
