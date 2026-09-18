pub(crate) fn parse_locale_override(content: &str, accepted_quotes: &[char]) -> Option<String> {
    content.lines().find_map(|line| {
        let (name, value) = line.split_once('=')?;
        if name.trim() != "localeOverride" {
            return None;
        }
        let value = value.trim();
        let quote = value.chars().next()?;
        if !accepted_quotes.contains(&quote) {
            return None;
        }
        let value = value.strip_prefix(quote)?;
        let end = value.find(quote)?;
        let unquoted = &value[..end];
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

    #[test]
    fn macos_config_accepts_a_comment_after_the_double_quoted_locale() {
        let content = "localeOverride = \"zh-CN\" # keep the Codex locale\n";
        assert_eq!(
            parse_locale_override(content, &['"']),
            Some("zh-CN".to_string())
        );
    }
}
