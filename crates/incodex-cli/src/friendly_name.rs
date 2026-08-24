/**
 * [INPUT]: 接收经过 SHA-256 混合的随机熵，或用于测试的 Read 实现
 * [OUTPUT]: 对外提供临时两词名称生成契约
 * [POS]: incodex-cli 的临时身份命名边界；词表与熵源实现随后在此收敛
 * [PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md
 */

#[cfg(test)]
mod tests {
    use std::io::{self, Read};

    use super::{friendly_name_from_entropy_digest, friendly_name_from_reader};

    #[test]
    fn deterministic_entropy_produces_two_title_case_words_without_hex_placeholder() {
        let digest = [0_u8; 32];
        let name = friendly_name_from_entropy_digest(&digest);
        let words: Vec<&str> = name.split(' ').collect();

        assert_eq!(name, "Quiet Otter");
        assert_eq!(words.len(), 2);
        assert!(name.len() <= 64);
        assert!(words
            .iter()
            .all(|word| word.chars().all(|character| character.is_ascii_alphabetic())));
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
}
