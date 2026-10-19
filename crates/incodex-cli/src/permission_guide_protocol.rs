/*
 * [INPUT]: 依赖 JSON 字节、当前 nonce 与封闭宿主事件枚举。
 * [OUTPUT]: 验证权限卡事件格式与 nonce；拒绝未声明的消息。
 * [POS]: 权限卡 stdio 的解码边界；不执行权限操作。
 * [PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md
 */
use super::*;

pub(super) fn decode_host_event(line: &[u8], nonce: &str) -> Result<HostEvent, String> {
    let value: Value = serde_json::from_slice(line)
        .map_err(|error| format!("invalid native Accessibility guide message: {error}"))?;
    let object = value
        .as_object()
        .ok_or("native Accessibility guide message is not an object")?;
    if object.get("nonce").and_then(Value::as_str) != Some(nonce) {
        return Err("native Accessibility guide nonce mismatch".into());
    }
    let kind = object
        .get("type")
        .and_then(Value::as_str)
        .ok_or("native Accessibility guide message type is missing")?;
    match kind {
        "app-locale" => {
            if object.len() != 3 {
                return Err("native Accessibility guide locale message has extra fields".into());
            }
            let locale = object
                .get("locale")
                .and_then(Value::as_str)
                .filter(|value| {
                    !value.is_empty()
                        && value.len() <= 64
                        && value.as_bytes()[0].is_ascii_alphabetic()
                        && value.bytes().all(|byte| {
                            byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'_'
                        })
                })
                .ok_or("native Accessibility guide target app locale is invalid")?;
            Ok(HostEvent::AppLocale(locale.into()))
        }
        "ready" => Ok(HostEvent::Ready),
        "allow" if object.len() == 2 => Ok(HostEvent::Allow),
        "allow-screen" if object.len() == 2 => Ok(HostEvent::AllowScreen),
        "retry-screen" if object.len() == 2 => Ok(HostEvent::RetryScreen),
        "retry" if object.len() == 2 => Ok(HostEvent::Retry),
        "later" => Ok(HostEvent::Later),
        "close" => Ok(HostEvent::Close),
        "error" => Ok(HostEvent::Error(
            object
                .get("message")
                .and_then(Value::as_str)
                .unwrap_or("native Accessibility guide reported an error")
                .to_string(),
        )),
        other => Err(format!(
            "native Accessibility guide message type is unsupported: {other}"
        )),
    }
}
