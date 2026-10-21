/*
 * [INPUT]: 依赖原owner记录、合成App与标准profile锁选择。
 * [OUTPUT]: 锁定同profile跨包路径唤回的精确owner选择。
 * [POS]: 普通宿主选择回归；只使用本测试进程与loopback listener。
 * [PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md
 */
use super::*;
use std::fs;
use std::net::TcpListener;
use std::os::unix::fs::symlink;
use std::os::unix::net::UnixListener;

struct Fixture {
    root: PathBuf,
    app: PathBuf,
    profile: PathBuf,
    host: NormalHost,
    owner: Option<LaunchOwner>,
    _listener: TcpListener,
    _profile_socket: UnixListener,
}

impl Fixture {
    fn new() -> Self {
        let root = PathBuf::from("/private/tmp").join(format!(
            "incodex-profile-owner-{}",
            incodex_transaction::new_install_id()
        ));
        let app = root.join("previous/ChatGPT.app");
        fs::create_dir_all(app.join("Contents/MacOS")).unwrap();
        fs::write(app.join("Contents/Info.plist"), "<?xml version=\"1.0\"?><plist version=\"1.0\"><dict><key>CFBundleExecutable</key><string>Host</string></dict></plist>").unwrap();
        symlink(
            std::env::current_exe().unwrap(),
            app.join("Contents/MacOS/Host"),
        )
        .unwrap();
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let host = NormalHost::current_process_for_test(port).unwrap();
        host.wait_for_test_listener_owner(port).unwrap();
        let LaunchClaim::Primary(mut owner) = claim(&root, &app).unwrap() else {
            panic!("isolated fixture must own its first launch");
        };
        owner.publish(&host).unwrap();
        let profile = root.join("standard-profile");
        fs::create_dir(&profile).unwrap();
        symlink(
            format!("fixture-host-{}", std::process::id()),
            profile.join("SingletonLock"),
        )
        .unwrap();
        let socket_path = root.join("profile.sock");
        let profile_socket = UnixListener::bind(&socket_path).unwrap();
        symlink(&socket_path, profile.join("SingletonSocket")).unwrap();
        Self {
            root,
            app,
            profile,
            host,
            owner: Some(owner),
            _listener: listener,
            _profile_socket: profile_socket,
        }
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        self.owner.take();
        let _ = fs::remove_dir_all(&self.root);
    }
}

#[test]
fn standard_profile_reuses_a_verified_owner_from_another_app_location() {
    let fixture = Fixture::new();
    let selected = active_profile_host(&fixture.root, &fixture.profile)
        .unwrap()
        .expect("profile already owned by our live host must not launch another bundle");
    assert_eq!(
        selected,
        (
            fs::canonicalize(&fixture.app).unwrap(),
            fixture.host.clone()
        )
    );
    assert!(fixture.owner.as_ref().unwrap().path.exists());
}

#[test]
fn a_profile_lock_is_not_authority_to_reuse_an_unknown_or_stale_owner() {
    let fixture = Fixture::new();
    assert!(active_profile_host(&fixture.root, &fixture.profile)
        .unwrap()
        .is_some());
    let record = &fixture.owner.as_ref().unwrap().path;
    let mut changed: serde_json::Value =
        serde_json::from_slice(&fs::read(record).unwrap()).unwrap();
    let seconds = changed["controller"]["start_identity"]["seconds"]
        .as_u64()
        .unwrap();
    changed["controller"]["start_identity"]["seconds"] = serde_json::json!(seconds + 1);
    fs::write(record, serde_json::to_vec(&changed).unwrap()).unwrap();
    assert!(active_profile_host(&fixture.root, &fixture.profile)
        .unwrap()
        .is_none());
    assert!(fixture.host.is_alive().unwrap());
}

#[test]
fn no_profile_owner_keeps_the_original_launch_route() {
    let fixture = Fixture::new();
    fs::remove_file(fixture.profile.join("SingletonLock")).unwrap();
    assert!(active_profile_host(&fixture.root, &fixture.profile)
        .unwrap()
        .is_none());
    symlink("fixture-host-1", fixture.profile.join("SingletonLock")).unwrap();
    assert!(active_profile_host(&fixture.root, &fixture.profile)
        .unwrap()
        .is_none());
    assert!(fixture.host.is_alive().unwrap());
}

#[test]
fn matching_lock_pid_without_the_profiles_unix_socket_is_not_ownership() {
    let fixture = Fixture::new();
    assert!(active_profile_host(&fixture.root, &fixture.profile).unwrap().is_some());
    fs::remove_file(fixture.profile.join("SingletonSocket")).unwrap();
    symlink(fixture.root.join("not-owned.sock"), fixture.profile.join("SingletonSocket")).unwrap();
    assert!(active_profile_host(&fixture.root, &fixture.profile).unwrap().is_none(), "a live PID and unrelated CDP listener do not prove this profile's ownership");
    assert!(fixture.host.is_alive().unwrap());
}
