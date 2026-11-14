//! [INPUT]: 依赖已证明归属的原 CDP 注册连接、单次随机能力和既有生命周期监督。
//! [OUTPUT]: 只把当前私密主文档的 quit 意图转换为 Page.close 正常关闭请求。
//! [POS]: 注册连接的事件适配；不新增线程、轮询、IPC 服务或会话 owner。
//! [PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md

use super::*;

const BINDING: &str = "__incodexPrivateClose";
type Guard = Arc<dyn Fn(&TcpStream) -> Result<(), String> + Send + Sync>;

pub struct CdpRegistration {
    pub(super) socket: WebSocket<TcpStream>,
    close: Option<PrivateClose>,
}

struct PrivateClose {
    nonce: String,
    guard: Guard,
}

impl CdpRegistration {
    #[cfg(target_os = "windows")]
    pub(crate) fn plain(socket: WebSocket<TcpStream>) -> Self {
        Self {
            socket,
            close: None,
        }
    }
    #[cfg(target_os = "windows")]
    pub(crate) fn private<G>(
        mut socket: WebSocket<TcpStream>,
        nonce: String,
        guard: Arc<G>,
    ) -> Result<Self, String>
    where
        G: Fn(&TcpStream) -> Result<(), String> + Send + Sync + 'static,
    {
        let source = bridge_source(&nonce)?;
        send_guarded_cdp(
            &mut socket,
            200,
            "Runtime.enable",
            json!({}),
            guard.as_ref(),
        )?;
        send_guarded_cdp(
            &mut socket,
            201,
            "Runtime.addBinding",
            json!({"name": BINDING}),
            guard.as_ref(),
        )?;
        send_guarded_cdp(
            &mut socket,
            202,
            "Page.addScriptToEvaluateOnNewDocument",
            json!({"source": source}),
            guard.as_ref(),
        )?;
        let result = send_guarded_cdp(
            &mut socket,
            203,
            "Runtime.evaluate",
            json!({"expression": source, "returnByValue": true}),
            guard.as_ref(),
        )?;
        if result
            .pointer("/result/result/value")
            .and_then(Value::as_bool)
            != Some(true)
        {
            return Err("private close adapter did not acknowledge its document".into());
        }
        Ok(Self {
            socket,
            close: Some(PrivateClose { nonce, guard }),
        })
    }

    pub(super) fn drain(&mut self) -> bool {
        loop {
            match self.socket.read() {
                Ok(Message::Close(_)) => return true,
                Ok(Message::Ping(_)) => {
                    if self.socket.flush().is_err() {
                        return true;
                    }
                }
                Ok(Message::Text(text)) => {
                    if let Some(close) = &self.close {
                        if let Ok(message) = serde_json::from_str::<Value>(&text) {
                            if let Some(context) = close_context(&message, &close.nonce) {
                                // 两个有界命令均重新证明原连接归属；请求 ACK 不等于窗口关闭。
                                if close.request(&mut self.socket, context).is_err() {
                                    eprintln!("Windows private close request was not acknowledged");
                                }
                                if self.socket.get_mut().set_nonblocking(true).is_err() {
                                    return true;
                                }
                            }
                        }
                    }
                }
                Ok(_) => {}
                Err(tungstenite::Error::Io(error))
                    if matches!(error.kind(), ErrorKind::WouldBlock | ErrorKind::TimedOut) =>
                {
                    return false
                }
                Err(_) => return true,
            }
        }
    }
}

impl PrivateClose {
    fn request(&self, socket: &mut WebSocket<TcpStream>, context: u64) -> Result<(), String> {
        let deadline = Instant::now() + WINDOWS_LIFECYCLE_CDP_TIMEOUT;
        let proof = send_guarded_cdp_until(
            socket,
            204,
            "Runtime.evaluate",
            json!({
                "expression": "window===window.top && location.href==='app://-/index.html' && window.__incodexIncognito===true",
                "contextId": context, "returnByValue": true
            }),
            &|stream| (self.guard)(stream),
            deadline,
        )?;
        if proof
            .pointer("/result/result/value")
            .and_then(Value::as_bool)
            != Some(true)
        {
            return Err("private close document ownership is unavailable".into());
        }
        send_guarded_cdp_until(
            socket,
            205,
            "Page.close",
            json!({}),
            &|stream| (self.guard)(stream),
            deadline,
        )?;
        // 原生命周期监督继续等主 target 真正关闭；不在这里终止 Job 或焚毁 session。
        Ok(())
    }
}

fn close_context(message: &Value, nonce: &str) -> Option<u64> {
    if message["method"] != "Runtime.bindingCalled"
        || message["params"]["name"] != BINDING
        || message["params"]["payload"].as_str() != Some(nonce)
    {
        return None;
    }
    message["params"]["executionContextId"]
        .as_u64()
        .filter(|id| *id != 0)
}

#[cfg(any(target_os = "windows", test))]
fn bridge_source(nonce: &str) -> Result<String, String> {
    if nonce.len() != 32 || !nonce.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err("invalid private close capability".into());
    }
    Ok(format!(
        r#"(() => {{
      if(window!==window.top || location.href!=='app://-/index.html' || window.__incodexIncognito!==true) return false;
      if(typeof window.{BINDING}!=='function') return false;
      window.incodex ??= {{}};
      window.incodex.requestIncognitoAction ??= async payload => {{
        if(payload?.action!=='quit') return {{ok:false,code:'UNSUPPORTED'}};
        window.{BINDING}({nonce});
        return {{ok:true,code:'CLOSE_REQUESTED'}};
      }};
      return typeof window.incodex.requestIncognitoAction==='function';
    }})()"#,
        nonce = json_string(nonce)
    ))
}

#[cfg(test)]
#[path = "cdp_private_close_tests.rs"]
mod tests;
