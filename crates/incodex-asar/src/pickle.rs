fn align4(n: usize) -> usize {
    n.div_ceil(4) * 4
}

pub fn read_u32_pickle(bytes: &[u8]) -> Result<(u32, usize), String> {
    if bytes.len() < 8 {
        return Err("truncated asar size pickle".into());
    }
    let payload_size = u32::from_le_bytes(bytes[0..4].try_into().unwrap());
    if payload_size != 4 {
        return Err(format!("unexpected size pickle payload {payload_size}"));
    }
    let value = u32::from_le_bytes(bytes[4..8].try_into().unwrap());
    Ok((value, 8))
}

pub fn write_u32_pickle(value: u32) -> Vec<u8> {
    let mut out = Vec::with_capacity(8);
    out.extend_from_slice(&4u32.to_le_bytes());
    out.extend_from_slice(&value.to_le_bytes());
    out
}

pub fn read_string_pickle(bytes: &[u8]) -> Result<(String, usize), String> {
    if bytes.len() < 4 {
        return Err("truncated asar header pickle".into());
    }
    let payload_size = u32::from_le_bytes(bytes[0..4].try_into().unwrap()) as usize;
    if bytes.len() < 4 + payload_size {
        return Err("truncated asar header pickle payload".into());
    }
    let payload = &bytes[4..4 + payload_size];
    if payload.len() < 4 {
        return Err("header pickle missing string length".into());
    }
    let str_len = u32::from_le_bytes(payload[0..4].try_into().unwrap()) as usize;
    if payload.len() < 4 + str_len {
        return Err("header pickle string overruns payload".into());
    }
    let text = std::str::from_utf8(&payload[4..4 + str_len]).map_err(|err| err.to_string())?;
    Ok((text.to_string(), 4 + payload_size))
}

pub fn write_string_pickle(text: &str) -> Vec<u8> {
    let bytes = text.as_bytes();
    let payload_len = align4(4 + bytes.len());
    let mut payload = Vec::with_capacity(payload_len);
    payload.extend_from_slice(&(bytes.len() as u32).to_le_bytes());
    payload.extend_from_slice(bytes);
    payload.resize(payload_len, 0);
    let mut out = Vec::with_capacity(4 + payload.len());
    out.extend_from_slice(&(payload.len() as u32).to_le_bytes());
    out.extend_from_slice(&payload);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_size_and_string() {
        let size = write_u32_pickle(508);
        assert_eq!(read_u32_pickle(&size).unwrap(), (508, 8));
        let header = write_string_pickle(r#"{"files":{}}"#);
        let (text, used) = read_string_pickle(&header).unwrap();
        assert_eq!(text, r#"{"files":{}}"#);
        assert_eq!(used, header.len());
    }
}
