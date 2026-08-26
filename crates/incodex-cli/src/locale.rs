pub(crate) fn parse_locale_override(content: &str, accepted_quotes: &[char]) -> Option<String> {
    content.lines().find_map(|line| {
        let (name, value) = line.split_once('=')?;
        if name.trim() != "localeOverride" {
            return None;
        }
        let value = value.trim();
        let unquoted = accepted_quotes.iter().find_map(|quote| {
            value
                .strip_prefix(*quote)
                .and_then(|value| value.strip_suffix(*quote))
        })?;
        let locale = unquoted.trim();
        (!locale.is_empty()).then(|| locale.to_string())
    })
}

#[cfg(test)]
mod tests {
    use super::parse_locale_override;

    #[test]
    fn shared_parser_preserves_each_platforms_quote_policy() {
        let content = "other = \"x\"\nlocaleOverride = ' zh-CN '\n";
        assert_eq!(parse_locale_override(content, &['"']), None);
        assert_eq!(
            parse_locale_override(content, &['"', '\'']),
            Some("zh-CN".to_string())
        );
    }
}
