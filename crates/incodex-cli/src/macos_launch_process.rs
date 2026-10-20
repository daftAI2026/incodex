/*
 * [INPUT]: 依赖AppQuiescence、LaunchServices、serde与lsof的逐FD TCP身份。
 * [OUTPUT]: 提供普通宿主启动、进程身份及共享socket所有权证明。
 * [POS]: macOS 普通宿主适配层；按kernel socket归并继承FD，不接管会话或终止App。
 * [PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md
 */
use std::collections::{BTreeMap, BTreeSet};
use std::mem::MaybeUninit;
use std::net::{Ipv4Addr, SocketAddr, SocketAddrV4, TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::thread;
use std::time::{Duration, Instant};

use incodex_macos::AppQuiescence;
use serde::{Deserialize, Serialize};

const OPEN_PATH: &str = "/usr/bin/open";
const LSOF_PATH: &str = "/usr/sbin/lsof";
pub(crate) const STARTUP_TIMEOUT: Duration = Duration::from_secs(30);
const STARTUP_POLL_INTERVAL: Duration = Duration::from_millis(100);

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
struct ProcessStartIdentity {
    seconds: u64,
    microseconds: u64,
}

/// 只保存已验证的宿主身份，不拥有可供强制终止的子进程句柄。
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
pub(crate) struct NormalHost {
    process: ProcessOwner,
    debug_port: u16,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
pub(crate) struct ProcessOwner {
    pid: i32,
    start_identity: ProcessStartIdentity,
    executable: PathBuf,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct LsofRecord {
    pid: i32,
    file_descriptor: String,
    file_type: Option<String>,
    device: Option<String>,
    name: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct SocketOwnership {
    device: String,
    pids: Vec<i32>,
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
    if loopback_listener_socket(port)?.is_some() {
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
        let listener = loopback_listener_socket(port)?;

        if let Some(listener) = listener {
            let pid = unique_official_host_pid(&listener, &running_pids)?;
            let host = NormalHost::capture(pid, executable.clone(), port)?;
            let current_listener = loopback_listener_socket(port)?
                .ok_or("CDP listener disappeared during host capture")?;
            require_same_kernel_socket(&listener, &current_listener, "CDP listener")?;
            host.require_socket_owner(&current_listener, "CDP listener")?;
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
    pub(crate) fn validate_app(&self, app: &Path) -> Result<bool, String> {
        let expected = AppQuiescence::for_app(app)?;
        if self.debug_port == 0
            || self.process.executable != expected.executable()
            || !self.is_alive()?
        {
            return Ok(false);
        }
        self.require_listener_owner(self.debug_port)?;
        Ok(true)
    }

    pub(crate) fn activate(&self, app: &Path) -> Result<(), String> {
        if !self.validate_app(app)? {
            return Err("captured normal Codex host is no longer current".into());
        }
        let pages = crate::cdp::list_targets(self.debug_port)?;
        // 普通宿主允许多个工作窗口；只置前已有官方页，不创建新窗口。
        let page = pages
            .iter()
            .find(|p| crate::cdp::is_primary_codex_page(p))
            .ok_or("captured Codex has no primary page to activate")?;
        let mut socket = crate::cdp::connect_cdp_websocket(&page.ws, self.debug_port)?;
        crate::cdp::send_guarded_cdp(
            &mut socket,
            1,
            "Page.bringToFront",
            serde_json::json!({}),
            &|stream| self.require_connection_owner(stream),
        )?;
        if !self.is_alive()? {
            return Err("captured Codex exited during activation".into());
        }
        Ok(())
    }

    pub fn pid(&self) -> i32 {
        self.process.pid
    }

    pub fn is_alive(&self) -> Result<bool, String> {
        self.process.is_alive()
    }

    pub fn require_connection_owner(&self, stream: &TcpStream) -> Result<(), String> {
        let (server, client) = connection_endpoints(stream, self.debug_port)?;
        let socket = connection_server_socket(server, client)?
            .ok_or("cannot identify official Codex established CDP connection owner")?;
        self.require_socket_owner(&socket, "established CDP connection")
    }

    pub fn require_listener_owner(&self, port: u16) -> Result<(), String> {
        if port != self.debug_port {
            return Err(format!(
                "requested CDP listener port {port} does not match owned port {}",
                self.debug_port
            ));
        }
        let socket = loopback_listener_socket(port)?
            .ok_or("cannot identify official Codex CDP listener owner")?;
        self.require_socket_owner(&socket, "CDP listener")
    }

    fn capture(pid: i32, executable: PathBuf, debug_port: u16) -> Result<Self, String> {
        Ok(Self {
            process: ProcessOwner::capture(pid, executable)?,
            debug_port,
        })
    }

    #[cfg(test)]
    pub(crate) fn current_process_for_test(debug_port: u16) -> Result<Self, String> {
        Ok(Self {
            process: ProcessOwner::current()?,
            debug_port,
        })
    }

    #[cfg(test)]
    pub(crate) fn wait_for_test_listener_owner(&self, port: u16) -> Result<(), String> {
        self.require_listener_owner(port)
    }

    fn require_socket_owner(&self, socket: &SocketOwnership, surface: &str) -> Result<(), String> {
        if !socket.pids.contains(&self.process.pid) {
            return Err(format!(
                "official Codex {surface} socket {} is not held by captured pid {}",
                socket.device, self.process.pid
            ));
        }
        if self.is_alive()? {
            Ok(())
        } else {
            Err(format!(
                "official Codex {surface} owner pid {} is no longer the captured process",
                self.process.pid
            ))
        }
    }
}

impl ProcessOwner {
    pub(crate) fn current() -> Result<Self, String> {
        let pid = std::process::id() as i32;
        let executable = incodex_macos::process_executable_path(pid).ok_or_else(|| {
            format!("cannot read current Incodex controller executable for pid {pid}")
        })?;
        Self::capture(pid, executable)
    }

    fn capture(pid: i32, executable: PathBuf) -> Result<Self, String> {
        let start_identity = process_start_identity(pid)?
            .ok_or_else(|| format!("captured process {pid} exited during startup"))?;
        let owner = Self {
            pid,
            start_identity,
            executable,
        };
        if !owner.is_alive()? {
            return Err(format!(
                "captured process {pid} changed identity during startup"
            ));
        }
        Ok(owner)
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
                    "cannot read executable path for captured process pid {}",
                    self.pid
                )),
            };
        };
        let Some(after) = process_start_identity(self.pid)? else {
            return Ok(false);
        };
        Ok(after == self.start_identity && executable == self.executable)
    }
}

fn require_loopback_port_available(port: u16) -> Result<(), String> {
    let address = SocketAddrV4::new(Ipv4Addr::LOCALHOST, port);
    let listener = TcpListener::bind(address)
        .map_err(|error| format!("cannot reserve requested CDP loopback port {port}: {error}"))?;
    drop(listener);
    Ok(())
}

fn loopback_listener_socket(port: u16) -> Result<Option<SocketOwnership>, String> {
    let filter = format!("-iTCP@127.0.0.1:{port}");
    let records = lsof_records(&[&filter, "-sTCP:LISTEN"])?;
    let endpoint = format!("127.0.0.1:{port}");
    socket_ownership_for(records, "CDP listener", |name| name == endpoint)
}

fn connection_server_socket(
    server: SocketAddrV4,
    client: SocketAddrV4,
) -> Result<Option<SocketOwnership>, String> {
    let filter = format!("-iTCP@127.0.0.1:{}", server.port());
    let records = lsof_records(&[&filter, "-sTCP:ESTABLISHED"])?;
    let server = server.to_string();
    let client = client.to_string();
    socket_ownership_for(records, "established CDP connection", |name| {
        lsof_name_is_server_connection(name, &server, &client)
    })
}

fn lsof_records(filters: &[&str]) -> Result<Vec<LsofRecord>, String> {
    let output = Command::new(LSOF_PATH)
        .args(["-nP", "-Fpfndt"])
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

fn parse_lsof_records(output: &str) -> Result<Vec<LsofRecord>, String> {
    let mut current_pid = None;
    let mut records = Vec::new();
    let mut current_record: Option<LsofRecord> = None;
    for field in output.lines().filter(|field| !field.is_empty()) {
        if let Some(raw_pid) = field.strip_prefix('p') {
            finish_lsof_record(&mut current_record, &mut records);
            let pid = raw_pid
                .parse::<i32>()
                .map_err(|error| format!("lsof returned an invalid process id: {error}"))?;
            if pid <= 0 {
                return Err("lsof returned a non-positive process id".into());
            }
            current_pid = Some(pid);
        } else if field.starts_with('f') {
            finish_lsof_record(&mut current_record, &mut records);
            let pid = current_pid.ok_or_else(|| {
                "lsof returned a file descriptor without a process id".to_string()
            })?;
            let file_descriptor = field
                .strip_prefix('f')
                .filter(|descriptor| !descriptor.is_empty())
                .ok_or("lsof returned an empty file descriptor")?;
            current_record = Some(LsofRecord {
                pid,
                file_descriptor: file_descriptor.to_string(),
                file_type: None,
                device: None,
                name: None,
            });
        } else if let Some(value) = field.strip_prefix('t') {
            set_lsof_field(&mut current_record, value, "file type", |record| {
                &mut record.file_type
            })?;
        } else if let Some(value) = field.strip_prefix('d') {
            set_lsof_field(&mut current_record, value, "socket device", |record| {
                &mut record.device
            })?;
        } else if let Some(value) = field.strip_prefix('n') {
            set_lsof_field(&mut current_record, value, "network endpoint", |record| {
                &mut record.name
            })?;
        } else {
            return Err("lsof returned an unexpected machine-readable field".into());
        }
    }
    finish_lsof_record(&mut current_record, &mut records);
    if !output.trim().is_empty() && records.is_empty() {
        return Err("lsof returned process records without file descriptors".into());
    }
    Ok(records)
}

fn finish_lsof_record(current: &mut Option<LsofRecord>, records: &mut Vec<LsofRecord>) {
    if let Some(record) = current.take() {
        records.push(record);
    }
}

fn set_lsof_field(
    current: &mut Option<LsofRecord>,
    value: &str,
    field_name: &str,
    select: impl FnOnce(&mut LsofRecord) -> &mut Option<String>,
) -> Result<(), String> {
    if value.is_empty() {
        return Err(format!("lsof returned an empty {field_name}"));
    }
    let record = current
        .as_mut()
        .ok_or_else(|| format!("lsof returned {field_name} without a file descriptor"))?;
    if select(record).replace(value.to_string()).is_some() {
        return Err(format!("lsof returned duplicate {field_name} fields"));
    }
    Ok(())
}

fn socket_ownership_for(
    records: Vec<LsofRecord>,
    surface: &str,
    matches_endpoint: impl Fn(&str) -> bool,
) -> Result<Option<SocketOwnership>, String> {
    let mut sockets = BTreeMap::<String, BTreeSet<i32>>::new();
    for record in records {
        if record.file_descriptor.is_empty() {
            return Err(format!("lsof omitted the file descriptor for {surface}"));
        }
        let file_type = record
            .file_type
            .as_deref()
            .ok_or_else(|| format!("lsof omitted the TCP type for {surface}"))?;
        if file_type != "IPv4" {
            return Err(format!("lsof returned non-IPv4 TCP evidence for {surface}"));
        }
        let device = record
            .device
            .as_deref()
            .ok_or_else(|| format!("lsof omitted the socket device for {surface}"))?;
        let device = normalize_socket_device(device)?;
        let name = record
            .name
            .as_deref()
            .ok_or_else(|| format!("lsof omitted the endpoint for {surface}"))?;
        if matches_endpoint(name) {
            sockets.entry(device).or_default().insert(record.pid);
        }
    }

    match sockets.len() {
        0 => Ok(None),
        1 => {
            let (device, pids) = sockets.into_iter().next().expect("one socket entry");
            Ok(Some(SocketOwnership {
                device,
                pids: pids.into_iter().collect(),
            }))
        }
        _ => Err(format!(
            "lsof reported multiple kernel socket identities for {surface}; refusing ambiguous ownership"
        )),
    }
}

fn normalize_socket_device(device: &str) -> Result<String, String> {
    let Some(hex) = device.strip_prefix("0x") else {
        return Err("lsof returned a malformed socket device identity".into());
    };
    if hex.is_empty()
        || !hex.bytes().all(|byte| byte.is_ascii_hexdigit())
        || hex.bytes().all(|byte| byte == b'0')
    {
        return Err("lsof returned a malformed socket device identity".into());
    }
    Ok(device.to_ascii_lowercase())
}

fn unique_official_host_pid(socket: &SocketOwnership, running_pids: &[i32]) -> Result<i32, String> {
    let candidates = socket
        .pids
        .iter()
        .copied()
        .filter(|pid| running_pids.contains(pid))
        .collect::<Vec<_>>();
    match candidates.as_slice() {
        [pid] => Ok(*pid),
        [] => Err(format!(
            "CDP listener socket is not held by the requested official Codex executable; observed pids {:?}",
            socket.pids
        )),
        _ => Err(format!(
            "CDP listener socket has multiple exact official executable owners {candidates:?}"
        )),
    }
}

fn require_same_kernel_socket(
    expected: &SocketOwnership,
    current: &SocketOwnership,
    surface: &str,
) -> Result<(), String> {
    if expected.device == current.device {
        Ok(())
    } else {
        Err(format!(
            "official Codex {surface} socket identity changed during host capture"
        ))
    }
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

    fn socket_fixture(output: &str, surface: &str) -> Result<Option<SocketOwnership>, String> {
        socket_ownership_for(parse_lsof_records(output)?, surface, |name| {
            name == "127.0.0.1:9222"
        })
    }

    #[test]
    fn lsof_listener_parser_requires_the_exact_endpoint() {
        let records = parse_lsof_records(
            "p41\nf3\ntIPv4\nd0x41\nn127.0.0.1:9222\np42\nf4\ntIPv4\nd0x42\nn127.0.0.1:92220\np43\nf5\ntIPv4\nd0x43\nn*:9222\n",
        )
        .unwrap();
        assert_eq!(records[0].file_type.as_deref(), Some("IPv4"));
        let socket = socket_ownership_for(records, "CDP listener", |name| name == "127.0.0.1:9222")
            .unwrap()
            .unwrap();
        assert_eq!(socket.pids, vec![41]);
        assert_eq!(socket.device, "0x41");
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
        assert!(!lsof_name_is_server_connection(
            "127.0.0.1:9222->127.0.0.1:50322",
            "127.0.0.1:9222",
            "127.0.0.1:50321"
        ));
        assert!(!lsof_name_is_server_connection(
            "127.0.0.1:9223->127.0.0.1:50321",
            "127.0.0.1:9222",
            "127.0.0.1:50321"
        ));
    }

    #[test]
    fn one_kernel_socket_can_have_multiple_process_fd_owners() {
        let host_pid = std::process::id();
        let helper_pid = host_pid + 1;
        let output = format!(
            "p{host_pid}\nf64\ntIPv4\nd0x871ee9fbc6dc96b7\nn127.0.0.1:9222\np{helper_pid}\nf64\ntIPv4\nd0x871ee9fbc6dc96b7\nn127.0.0.1:9222\n"
        );
        let socket = socket_fixture(&output, "CDP listener").unwrap().unwrap();

        assert_eq!(socket.device, "0x871ee9fbc6dc96b7");
        assert_eq!(socket.pids.len(), 2);
        assert!(socket.pids.contains(&(host_pid as i32)));
        assert!(socket.pids.contains(&(helper_pid as i32)));
        let host = NormalHost::current_process_for_test(9222).unwrap();
        host.require_socket_owner(&socket, "CDP listener").unwrap();
    }

    #[test]
    fn server_connection_owners_share_one_exact_socket_identity() {
        let host_pid = std::process::id();
        let helper_pid = host_pid + 1;
        let client_pid = host_pid + 2;
        let records = parse_lsof_records(&format!(
            "p{host_pid}\nf64\ntIPv4\nd0x41\nn127.0.0.1:9222->127.0.0.1:50321 (ESTABLISHED)\np{helper_pid}\nf64\ntIPv4\nd0x41\nn127.0.0.1:9222->127.0.0.1:50321 (ESTABLISHED)\np{client_pid}\nf65\ntIPv4\nd0x99\nn127.0.0.1:50321->127.0.0.1:9222 (ESTABLISHED)\n"
        ))
        .unwrap();
        let socket = socket_ownership_for(records, "established CDP connection", |name| {
            lsof_name_is_server_connection(name, "127.0.0.1:9222", "127.0.0.1:50321")
        })
        .unwrap()
        .unwrap();

        assert_eq!(socket.device, "0x41");
        assert_eq!(socket.pids.len(), 2);
        assert!(socket.pids.contains(&(host_pid as i32)));
        assert!(socket.pids.contains(&(helper_pid as i32)));
    }

    #[test]
    fn listener_rejects_two_kernel_sockets_on_the_same_endpoint() {
        let host_pid = std::process::id();
        let output = format!(
            "p{host_pid}\nf64\ntIPv4\nd0x41\nn127.0.0.1:9222\np42556\nf65\ntIPv4\nd0x42\nn127.0.0.1:9222\n"
        );

        assert!(socket_fixture(&output, "CDP listener").is_err());
    }

    #[test]
    fn captured_host_must_hold_the_socket_and_keep_its_start_identity() {
        let host = NormalHost::current_process_for_test(9222).unwrap();
        let foreign_pid = host.pid().saturating_add(1);
        let foreign = socket_fixture(
            &format!("p{foreign_pid}\nf64\ntIPv4\nd0x41\nn127.0.0.1:9222\n"),
            "CDP listener",
        )
        .unwrap()
        .unwrap();
        assert!(host.require_socket_owner(&foreign, "CDP listener").is_err());

        let own = socket_fixture(
            &format!("p{}\nf64\ntIPv4\nd0x41\nn127.0.0.1:9222\n", host.pid()),
            "CDP listener",
        )
        .unwrap()
        .unwrap();
        let mut stale = host;
        stale.process.start_identity.seconds =
            stale.process.start_identity.seconds.saturating_add(1);
        assert!(stale.require_socket_owner(&own, "CDP listener").is_err());
    }

    #[test]
    fn startup_selects_only_one_exact_official_executable_owner() {
        let socket = SocketOwnership {
            device: "0x41".into(),
            pids: vec![41, 42],
        };

        assert_eq!(unique_official_host_pid(&socket, &[41]).unwrap(), 41);
        assert!(unique_official_host_pid(&socket, &[]).is_err());
        assert!(unique_official_host_pid(&socket, &[41, 42]).is_err());
        let replacement = SocketOwnership {
            device: "0x42".into(),
            pids: vec![41],
        };

        assert!(require_same_kernel_socket(&socket, &socket, "CDP listener").is_ok());
        assert!(require_same_kernel_socket(&socket, &replacement, "CDP listener").is_err());
    }

    #[test]
    fn missing_or_malformed_device_identity_fails_closed() {
        let missing = "p41\nf3\ntIPv4\nn127.0.0.1:9222\n";
        assert!(socket_fixture(missing, "CDP listener").is_err());

        let malformed = "p41\nf3\ntIPv4\ndnot-a-device\nn127.0.0.1:9222\n";
        assert!(socket_fixture(malformed, "CDP listener").is_err());
    }

    #[test]
    fn a_new_incomplete_file_descriptor_does_not_inherit_the_prior_device() {
        let records = parse_lsof_records(
            "p41\nf3\ntIPv4\nd0x41\nn127.0.0.1:9222\nf4\ntIPv4\nn127.0.0.1:9222\n",
        )
        .unwrap();

        assert_eq!(records.len(), 2);
        assert_eq!(records[0].device.as_deref(), Some("0x41"));
        assert_eq!(records[1].device, None);
        assert!(
            socket_ownership_for(records, "CDP listener", |name| { name == "127.0.0.1:9222" })
                .is_err()
        );
    }

    #[test]
    fn ownership_requires_ipv4_localhost_and_the_exact_debug_port() {
        assert!(require_ipv4_loopback(SocketAddr::V4(SocketAddrV4::new(
            Ipv4Addr::LOCALHOST,
            9222
        )))
        .is_ok());
        assert!(require_ipv4_loopback(SocketAddr::V4(SocketAddrV4::new(
            Ipv4Addr::new(127, 0, 0, 2),
            9222
        )))
        .is_err());
        assert!(require_ipv4_loopback(SocketAddr::V6("[::1]:9222".parse().unwrap())).is_err());

        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
        let port = listener.local_addr().unwrap().port();
        let stream = TcpStream::connect(listener.local_addr().unwrap()).unwrap();
        let (_accepted, _) = listener.accept().unwrap();
        assert!(connection_endpoints(&stream, port).is_ok());
        assert!(connection_endpoints(&stream, port.wrapping_add(1)).is_err());
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
        stale.process.start_identity.seconds =
            stale.process.start_identity.seconds.saturating_add(1);
        assert!(!stale.is_alive().unwrap());
        assert!(stale.require_listener_owner(port).is_err());
        assert!(host.require_listener_owner(port.wrapping_add(1)).is_err());
    }
}
