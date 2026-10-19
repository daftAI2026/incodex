/*
 * [INPUT]: 依赖 AppQuiescence 的精确宿主查询、LaunchServices 与 macOS TCP 事实。
 * [OUTPUT]: 提供普通宿主启动、PID/启动身份 owner 与 CDP loopback 所有权断言。
 * [POS]: macOS 普通宿主适配层；不替换 HOME，不接管会话，也不终止官方 App。
 * [PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md
 */
use std::collections::BTreeSet;
use std::mem::MaybeUninit;
use std::net::{Ipv4Addr, SocketAddr, SocketAddrV4, TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::thread;
use std::time::{Duration, Instant};

use incodex_macos::AppQuiescence;

const OPEN_PATH: &str = "/usr/bin/open";
const LSOF_PATH: &str = "/usr/sbin/lsof";
const STARTUP_TIMEOUT: Duration = Duration::from_secs(30);
const STARTUP_POLL_INTERVAL: Duration = Duration::from_millis(100);

#[derive(Debug, Clone, PartialEq, Eq)]
struct ProcessStartIdentity {
    seconds: u64,
    microseconds: u64,
}

/// 只保存已验证的宿主身份，不拥有可供强制终止的子进程句柄。
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct NormalHost {
    pid: i32,
    start_identity: ProcessStartIdentity,
    executable: PathBuf,
    debug_port: u16,
}

pub(crate) fn launch_normal(app: &Path, port: u16) -> Result<NormalHost, String> {
    if port == 0 {
        return Err("CDP port must be non-zero".into());
    }

    let app = std::fs::canonicalize(app).map_err(|error| {
        format!(
            "cannot resolve official Codex app {}: {error}",
            app.display()
        )
    })?;
    let quiescence = AppQuiescence::for_app(&app)?;
    let executable = quiescence.executable().to_path_buf();

    if let Some(pid) = quiescence.running_pids()?.first().copied() {
        return Err(format!(
            "official Codex is already running (pid {pid}); quit it normally before starting a CDP host"
        ));
    }

    require_loopback_port_available(port)?;
    if !loopback_listener_owners(port)?.is_empty() {
        return Err(format!(
            "CDP loopback port {port} is already owned by another process"
        ));
    }

    let port_argument = format!("--remote-debugging-port={port}");
    let output = Command::new(OPEN_PATH)
        .arg("-a")
        .arg(&app)
        .arg("--args")
        .arg("--remote-debugging-address=127.0.0.1")
        .arg(port_argument)
        .output()
        .map_err(|error| format!("cannot launch official Codex through LaunchServices: {error}"))?;
    if !output.status.success() {
        return Err(format!(
            "LaunchServices could not open official Codex: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }

    let deadline = Instant::now() + STARTUP_TIMEOUT;
    loop {
        let running_pids = quiescence.running_pids()?;
        let listener_pids = loopback_listener_owners(port)?;

        if !listener_pids.is_empty() {
            if listener_pids.len() != 1 {
                return Err(format!(
                    "CDP loopback port {port} has multiple listener owners; refusing ambiguous host"
                ));
            }
            let pid = listener_pids[0];
            if !running_pids.contains(&pid) {
                return Err(format!(
                    "CDP loopback port {port} belongs to pid {pid}, not the requested official app"
                ));
            }
            let host = NormalHost::capture(pid, executable.clone(), port)?;
            host.require_listener_owner(port)?;
            return Ok(host);
        }

        if Instant::now() >= deadline {
            return Err(format!(
                "timed out waiting for official Codex CDP listener on 127.0.0.1:{port}; the launched app was left running"
            ));
        }
        thread::sleep(STARTUP_POLL_INTERVAL);
    }
}

impl NormalHost {
    pub fn pid(&self) -> i32 {
        self.pid
    }

    pub fn is_alive(&self) -> Result<bool, String> {
        let Some(before) = process_start_identity(self.pid)? else {
            return Ok(false);
        };
        if before != self.start_identity {
            return Ok(false);
        }

        let Some(executable) = incodex_macos::process_executable_path(self.pid) else {
            return match process_start_identity(self.pid)? {
                None => Ok(false),
                Some(after) if after != self.start_identity => Ok(false),
                Some(_) => Err(format!(
                    "cannot read executable path for official Codex owner pid {}",
                    self.pid
                )),
            };
        };
        let Some(after) = process_start_identity(self.pid)? else {
            return Ok(false);
        };
        Ok(after == self.start_identity && executable == self.executable)
    }

    pub fn require_connection_owner(&self, stream: &TcpStream) -> Result<(), String> {
        let (server, client) = connection_endpoints(stream, self.debug_port)?;
        let owners = connection_server_owners(server, client)?;
        self.require_single_owner(&owners, "established CDP connection")
    }

    pub fn require_listener_owner(&self, port: u16) -> Result<(), String> {
        if port != self.debug_port {
            return Err(format!(
                "requested CDP listener port {port} does not match owned port {}",
                self.debug_port
            ));
        }
        let owners = loopback_listener_owners(port)?;
        self.require_single_owner(&owners, "CDP listener")
    }

    fn capture(pid: i32, executable: PathBuf, debug_port: u16) -> Result<Self, String> {
        let start_identity = process_start_identity(pid)?
            .ok_or_else(|| format!("official Codex owner pid {pid} exited during startup"))?;
        let host = Self {
            pid,
            start_identity,
            executable,
            debug_port,
        };
        if !host.is_alive()? {
            return Err(format!(
                "official Codex owner pid {pid} changed identity during startup"
            ));
        }
        Ok(host)
    }

    #[cfg(test)]
    pub(crate) fn current_process_for_test(debug_port: u16) -> Result<Self, String> {
        if debug_port == 0 {
            return Err("CDP port must be non-zero".into());
        }
        let pid = std::process::id() as i32;
        let executable = incodex_macos::process_executable_path(pid)
            .ok_or_else(|| format!("cannot read current test process executable for pid {pid}"))?;
        Self::capture(pid, executable, debug_port)
    }

    #[cfg(test)]
    pub(crate) fn wait_for_test_listener_owner(&self, port: u16) -> Result<(), String> {
        // 并行测试fork到exec之间会继承FD；最终仍须通过生产单owner断言。
        let deadline = Instant::now() + Duration::from_secs(2);
        loop {
            match self.require_listener_owner(port) {
                Err(error) if error.contains("multiple owners") && Instant::now() < deadline => {
                    eprintln!("test listener awaiting fork/exec completion: {error}");
                    thread::sleep(Duration::from_millis(20));
                }
                result => return result,
            }
        }
    }

    fn require_single_owner(&self, owners: &[i32], surface: &str) -> Result<(), String> {
        match owners {
            [pid] if *pid == self.pid => {
                if self.is_alive()? {
                    Ok(())
                } else {
                    Err(format!(
                        "official Codex {surface} owner pid {} is no longer the captured process",
                        self.pid
                    ))
                }
            }
            [] => Err(format!("cannot identify official Codex {surface} owner")),
            [pid] => Err(format!(
                "official Codex {surface} belongs to pid {pid}, not captured pid {}",
                self.pid
            )),
            _ => Err(format!(
                "official Codex {surface} has multiple owners {owners:?}; refusing ambiguous identity"
            )),
        }
    }
}

fn require_loopback_port_available(port: u16) -> Result<(), String> {
    let address = SocketAddrV4::new(Ipv4Addr::LOCALHOST, port);
    let listener = TcpListener::bind(address)
        .map_err(|error| format!("cannot reserve requested CDP loopback port {port}: {error}"))?;
    drop(listener);
    Ok(())
}

fn loopback_listener_owners(port: u16) -> Result<Vec<i32>, String> {
    let filter = format!("-iTCP@127.0.0.1:{port}");
    let records = lsof_records(&[&filter, "-sTCP:LISTEN"])?;
    let endpoint = format!("127.0.0.1:{port}");
    Ok(unique_pids(records.into_iter().filter_map(
        |(pid, name)| (name == endpoint).then_some(pid),
    )))
}

fn connection_server_owners(
    server: SocketAddrV4,
    client: SocketAddrV4,
) -> Result<Vec<i32>, String> {
    let filter = format!("-iTCP@127.0.0.1:{}", server.port());
    let records = lsof_records(&[&filter, "-sTCP:ESTABLISHED"])?;
    let server = server.to_string();
    let client = client.to_string();
    Ok(unique_pids(records.into_iter().filter_map(
        |(pid, name)| lsof_name_is_server_connection(&name, &server, &client).then_some(pid),
    )))
}

fn lsof_records(filters: &[&str]) -> Result<Vec<(i32, String)>, String> {
    let output = Command::new(LSOF_PATH)
        .args(["-nP", "-Fpn"])
        .args(filters)
        .output()
        .map_err(|error| format!("cannot inspect macOS TCP ownership with lsof: {error}"))?;

    if !output.status.success() {
        if output.status.code() == Some(1) && output.stdout.is_empty() && output.stderr.is_empty() {
            return Ok(Vec::new());
        }
        return Err(format!(
            "lsof could not prove macOS TCP ownership: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }

    parse_lsof_records(&String::from_utf8_lossy(&output.stdout))
}

fn parse_lsof_records(output: &str) -> Result<Vec<(i32, String)>, String> {
    let mut current_pid = None;
    let mut records = Vec::new();
    for field in output.lines().filter(|field| !field.is_empty()) {
        if let Some(raw_pid) = field.strip_prefix('p') {
            let pid = raw_pid
                .parse::<i32>()
                .map_err(|error| format!("lsof returned an invalid process id: {error}"))?;
            if pid <= 0 {
                return Err("lsof returned a non-positive process id".into());
            }
            current_pid = Some(pid);
        } else if let Some(name) = field.strip_prefix('n') {
            if name.is_empty() {
                return Err("lsof returned an empty network endpoint".into());
            }
            let pid = current_pid.ok_or_else(|| {
                "lsof returned a network endpoint without a process id".to_string()
            })?;
            records.push((pid, name.to_string()));
        } else if field.starts_with('f') {
            if current_pid.is_none() {
                return Err("lsof returned a file descriptor without a process id".into());
            }
        } else {
            return Err("lsof returned an unexpected machine-readable field".into());
        }
    }
    Ok(records)
}

fn unique_pids(pids: impl Iterator<Item = i32>) -> Vec<i32> {
    pids.collect::<BTreeSet<_>>().into_iter().collect()
}

fn lsof_name_is_server_connection(name: &str, server: &str, client: &str) -> bool {
    let name = name.strip_suffix(" (ESTABLISHED)").unwrap_or(name);
    let Some((local, remote)) = name.split_once("->") else {
        return false;
    };
    local.trim() == server && remote.trim() == client
}

fn connection_endpoints(
    stream: &TcpStream,
    expected_port: u16,
) -> Result<(SocketAddrV4, SocketAddrV4), String> {
    let local = require_ipv4_loopback(
        stream
            .local_addr()
            .map_err(|error| format!("cannot read CDP client endpoint: {error}"))?,
    )?;
    let remote = require_ipv4_loopback(
        stream
            .peer_addr()
            .map_err(|error| format!("cannot read CDP server endpoint: {error}"))?,
    )?;
    if remote.port() != expected_port {
        return Err(format!(
            "CDP connection targets port {}, not owned port {expected_port}",
            remote.port()
        ));
    }
    Ok((remote, local))
}

fn require_ipv4_loopback(address: SocketAddr) -> Result<SocketAddrV4, String> {
    match address {
        SocketAddr::V4(address) if *address.ip() == Ipv4Addr::LOCALHOST => Ok(address),
        _ => Err("CDP ownership proof requires an IPv4 loopback connection".into()),
    }
}

fn process_start_identity(pid: i32) -> Result<Option<ProcessStartIdentity>, String> {
    if pid <= 0 {
        return Ok(None);
    }
    let mut info = MaybeUninit::<libc::proc_bsdinfo>::zeroed();
    let expected_size = std::mem::size_of::<libc::proc_bsdinfo>();
    let written = unsafe {
        libc::proc_pidinfo(
            pid,
            libc::PROC_PIDTBSDINFO,
            0,
            info.as_mut_ptr().cast(),
            expected_size as libc::c_int,
        )
    };
    if written <= 0 {
        return if process_is_dead(pid)? {
            Ok(None)
        } else {
            Err(format!(
                "cannot read macOS start identity for process {pid}: {}",
                std::io::Error::last_os_error()
            ))
        };
    }
    if written as usize != expected_size {
        return Err(format!(
            "macOS returned an incomplete start identity for process {pid}"
        ));
    }

    let info = unsafe { info.assume_init() };
    if info.pbi_pid != pid as u32 {
        return Ok(None);
    }
    Ok(Some(ProcessStartIdentity {
        seconds: info.pbi_start_tvsec,
        microseconds: info.pbi_start_tvusec,
    }))
}

fn process_is_dead(pid: i32) -> Result<bool, String> {
    if unsafe { libc::kill(pid, 0) } == 0 {
        return Ok(false);
    }
    match std::io::Error::last_os_error().raw_os_error() {
        Some(libc::ESRCH) => Ok(true),
        Some(libc::EPERM) => Ok(false),
        _ => Err(format!(
            "cannot determine whether process {pid} is alive: {}",
            std::io::Error::last_os_error()
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lsof_listener_parser_requires_the_exact_endpoint() {
        let records = parse_lsof_records(
            "p41\nf3\nn127.0.0.1:9222\np42\nf4\nn127.0.0.1:92220\np43\nf5\nn*:9222\n",
        )
        .unwrap();
        let pids = unique_pids(
            records
                .into_iter()
                .filter_map(|(pid, name)| (name == "127.0.0.1:9222").then_some(pid)),
        );
        assert_eq!(pids, vec![41]);
    }

    #[test]
    fn lsof_connection_parser_accepts_only_the_server_side_tuple() {
        assert!(lsof_name_is_server_connection(
            "127.0.0.1:9222->127.0.0.1:50321",
            "127.0.0.1:9222",
            "127.0.0.1:50321"
        ));
        assert!(lsof_name_is_server_connection(
            "127.0.0.1:9222->127.0.0.1:50321 (ESTABLISHED)",
            "127.0.0.1:9222",
            "127.0.0.1:50321"
        ));
        assert!(!lsof_name_is_server_connection(
            "127.0.0.1:50321->127.0.0.1:9222",
            "127.0.0.1:9222",
            "127.0.0.1:50321"
        ));
    }

    #[test]
    fn current_process_listener_and_connection_require_exact_process_identity() {
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
        let port = listener.local_addr().unwrap().port();
        let host = NormalHost::current_process_for_test(port).unwrap();

        assert!(host.is_alive().unwrap());
        host.wait_for_test_listener_owner(port).unwrap();

        let client = TcpStream::connect(listener.local_addr().unwrap()).unwrap();
        let (_server, _) = listener.accept().unwrap();
        host.require_connection_owner(&client).unwrap();

        let mut stale = host.clone();
        stale.start_identity.seconds = stale.start_identity.seconds.saturating_add(1);
        assert!(!stale.is_alive().unwrap());
        assert!(stale.require_listener_owner(port).is_err());
        assert!(host.require_listener_owner(port.wrapping_add(1)).is_err());
    }
}
