/*
 * [INPUT]: 依赖真实 COMMITTED 夹具、请求生产器和私有 marker CAS。
 * [OUTPUT]: 验证更新重读已完成授权、同代 Skip 不复活与 pending 复用。
 * [POS]: 更新请求的非干扰回归；不调用真实 App 或 TCC。
 * [PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md
 */
use super::*;
use std::os::unix::fs::MetadataExt;

const A: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const B: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";

fn update(f: &Fixture, hash: &str) -> Result<Option<String>, String> {
    super::super::request_update_setup(&f.root, &f.app, &f.install_id, hash)
}
fn finish(f: &Fixture, request: &str, state: &str) {
    super::super::finish_cli_setup(&f.root, &f.app, &f.install_id, request, state).unwrap();
}

#[test]
fn unchanged_runtime_does_not_rearm_a_user_skip() {
    let f = committed_fixture("deferred");
    let request = update(&f, A).unwrap().unwrap();
    finish(&f, &request, "deferred");
    let before = fs::read(f.marker_path()).unwrap();
    let inode = fs::metadata(f.marker_path()).unwrap().ino();
    assert_eq!(update(&f, A).unwrap(), None, "same generation respects Skip");
    assert_eq!(fs::read(f.marker_path()).unwrap(), before);
    assert_eq!(fs::metadata(f.marker_path()).unwrap().ino(), inode);
}

#[test]
fn successful_publication_rechecks_permissions_after_previous_granted_result() {
    let f = committed_fixture("granted-is-not-current-permission");
    let first = update(&f, A).unwrap().unwrap();
    finish(&f, &first, "granted");

    let second = update(&f, A)
        .unwrap()
        .expect("a completed read must not suppress the next update's fresh probe");
    assert_ne!(first, second);
    let marker = read_marker(&f);
    assert_eq!(marker["state"], "pending");
    assert_eq!(marker["updateRuntimeSha256"], A);
    assert_eq!(marker["resumeReason"], "post-update");
}

#[test]
fn fresh_update_check_remains_single_request_until_its_outcome() {
    let f = committed_fixture("fresh-update-single-request");
    let first = update(&f, A).unwrap().unwrap();
    finish(&f, &first, "granted");
    let next = update(&f, A)
        .unwrap()
        .expect("the update must admit a fresh permission check");
    let before = fs::read(f.marker_path()).unwrap();
    assert_eq!(update(&f, A).unwrap(), Some(next));
    assert_eq!(fs::read(f.marker_path()).unwrap(), before);
}

#[test]
fn pending_update_reuses_the_request_without_publishing_another_edge() {
    let f = committed_fixture("update-pending");
    let request = update(&f, A).unwrap().unwrap();
    super::super::mark_continuation(
        &f.root,
        &f.app,
        &f.install_id,
        &request,
        super::super::ResumeReason::PostUpdate,
    )
    .unwrap();
    finish(&f, &request, "pending");
    let before = fs::read(f.marker_path()).unwrap();
    let inode = fs::metadata(f.marker_path()).unwrap().ino();
    assert_eq!(update(&f, A).unwrap(), Some(request));
    assert_eq!(fs::read(f.marker_path()).unwrap(), before);
    assert_eq!(fs::metadata(f.marker_path()).unwrap().ino(), inode);
}

#[test]
fn a_new_runtime_generation_may_check_again_after_skip() {
    let f = committed_fixture("update-next");
    let first = update(&f, A).unwrap().unwrap();
    finish(&f, &first, "deferred");
    let second = update(&f, B).unwrap().unwrap();
    assert_ne!(first, second);
    let marker = read_marker(&f);
    assert_eq!(marker["updateRuntimeSha256"], B);
    assert_eq!(marker["resumeReason"], "post-update");
    assert_eq!(marker["state"], "pending");
}

#[test]
fn later_generation_preserves_an_unfinished_authorization_intent() {
    let f = committed_fixture("update-resume-next");
    let request = update(&f, A).unwrap().unwrap();
    super::super::mark_continuation(
        &f.root,
        &f.app,
        &f.install_id,
        &request,
        super::super::ResumeReason::ScreenAuthorization,
    )
    .unwrap();
    assert_eq!(update(&f, B).unwrap(), Some(request));
    let marker = read_marker(&f);
    assert_eq!(marker["resumeReason"], "screen-authorization");
    assert_eq!(marker["updateRuntimeSha256"], B);
}

#[test]
fn explicit_setup_can_reenter_after_a_same_runtime_skip() {
    let f = committed_fixture("explicit-after-skip");
    let first = update(&f, A).unwrap().unwrap();
    finish(&f, &first, "deferred");
    let explicit = request_setup(&f.root, &f.app, &f.install_id).unwrap();
    assert_ne!(first, explicit);
    assert!(read_marker(&f)["updateRuntimeSha256"].is_null());
}

#[test]
fn update_admission_rejects_bad_generation_and_unknown_owner_without_writes() {
    let f = committed_fixture("update-reject");
    assert!(update(&f, "not-a-hash").is_err());
    assert!(!f.marker_path().exists());
    let request = update(&f, A).unwrap().unwrap();
    let mut marker = read_marker(&f);
    marker["presentationOwner"] = "unrecognized".into();
    write_marker(&f, &marker);
    let before = fs::read(f.marker_path()).unwrap();
    assert!(update(&f, B).is_err());
    assert_eq!(fs::read(f.marker_path()).unwrap(), before);
    assert_eq!(read_marker(&f)["requestId"], request);
}
