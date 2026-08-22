pub(crate) fn add_entitlement_key(xml: &str, key: &str) -> Result<String, String> {
    if xml.contains(&format!("<key>{key}</key>")) {
        return Ok(xml.to_string());
    }
    let close = root_dict_close(xml).ok_or("entitlement plist has no dictionary close")?;
    let mut output = xml.to_string();
    output.insert_str(close, &format!("  <key>{key}</key><true/>\n"));
    Ok(output)
}

fn root_dict_close(xml: &str) -> Option<usize> {
    let root = xml.find("<dict>")?;
    let mut depth = 1;
    let mut cursor = root + "<dict>".len();
    while let Some(next_close) = xml[cursor..].find("</dict>") {
        let close = cursor + next_close;
        if let Some(next_open) = xml[cursor..].find("<dict>") {
            let open = cursor + next_open;
            if open < close {
                depth += 1;
                cursor = open + "<dict>".len();
                continue;
            }
        }
        depth -= 1;
        if depth == 0 {
            return Some(close);
        }
        cursor = close + "</dict>".len();
    }
    None
}
