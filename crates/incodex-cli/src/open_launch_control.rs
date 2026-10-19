#[cfg(test)]
mod tests {
    use super::{bring_to_front_command, is_raise_line};

    #[test]
    fn control_accepts_only_the_closed_raise_line() {
        for request in [
            b"".as_slice(),
            b"raise",
            b"raise\r\n",
            b"raise now\n",
            b"Raise\n",
        ] {
            assert!(!is_raise_line(request), "unexpected request: {request:?}");
        }
        assert!(is_raise_line(b"raise\n"));
    }

    #[test]
    fn raise_uses_the_page_bring_to_front_command() {
        let (method, params) = bring_to_front_command();

        assert_eq!(method, "Page.bringToFront");
        assert!(params.as_object().is_some_and(|value| value.is_empty()));
    }
}
