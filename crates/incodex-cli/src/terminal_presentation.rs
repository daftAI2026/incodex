pub(crate) fn print_terminal_report(report: &str) {
    print!("{}", normalize_terminal_report(report));
}

pub(crate) fn print_terminal_result(result: &str) {
    println!("{result}");
    println!();
}

fn normalize_terminal_report(report: &str) -> String {
    format!("{}\n\n", report.trim_end_matches('\n'))
}

#[cfg(test)]
mod tests {
    use super::normalize_terminal_report;

    #[test]
    fn report_spacing_normalizes_with_or_without_a_trailing_newline() {
        assert_eq!(normalize_terminal_report("report"), "report\n\n");
        assert_eq!(normalize_terminal_report("report\n"), "report\n\n");
    }
}
