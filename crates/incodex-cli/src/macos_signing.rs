/*
 * [INPUT]: 依赖 macos_signing_assets 的稳定私有注册与 macOS 的显式签名验收。
 * [OUTPUT]: 提供安装上下文选择、只读 patched 验收与原有 coordinator 日志的签名阶段事实。
 * [POS]: 产品安装/恢复/诊断共享的注册身份适配层；后台不创身份、不弹授权、不降级。
 * [PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md
 */
use crate::macos_signing_assets::{
    ensure_signing_identity, read_signing_identity, unlock_signing_identity,
};
use incodex_macos::{
    inspect_outer_signing, verify_app, verify_patched_bundle_with_context, PlistInfo,
    SignatureKind, SigningContext, SigningInventory,
};
use std::path::Path;

pub fn context_for_install(
    root: &Path,
    official: bool,
    background: bool,
) -> Result<SigningContext, String> {
    if !official {
        return Ok(SigningContext::Adhoc);
    }
    let identity = if background {
        let registration = crate::macos_update_restore::read_registration(root)?
            .ok_or("macOS update recovery has no registered install generation")?;
        let Some(fingerprint) = registration.signing_certificate_sha256 else {
            // 缺字段是已登记的旧 ad-hoc 模式，不因 root 后来出现证书而迁移。
            return Ok(SigningContext::Adhoc);
        };
        let identity = read_signing_identity(root)?
            .ok_or("registered local update recovery has no stable signing identity")?;
        if identity.certificate_sha256 != fingerprint {
            return Err("registered local signing identity changed before update recovery".into());
        }
        identity
    } else {
        ensure_signing_identity(root)?
    };
    unlock_signing_identity(root, &identity)?;
    Ok(SigningContext::Local(identity))
}

pub(crate) fn verify_patched(
    root: &Path,
    app: &Path,
    expected: Option<&PlistInfo>,
) -> Result<SigningInventory, String> {
    let outer = inspect_outer_signing(app)?;
    let context = if outer.kind == SignatureKind::Adhoc {
        SigningContext::Adhoc
    } else {
        SigningContext::Local(
            read_signing_identity(root)?
                .ok_or("patched local signature has no registered identity")?,
        )
    };
    verify_patched_bundle_with_context(app, expected, &context)
}

pub(crate) fn verify_for_root(root: &Path, app: &Path) -> bool {
    // 官方 original 与既有合法 generic 仍用旧 policy；无 Team 的 local 不进入 generic fallback。
    if verify_app(app) {
        return true;
    }
    let install_id = incodex_asar::Archive::open(app.join(incodex_core::paths::ASAR_REL))
        .ok()
        .and_then(|archive| archive.read_package_main().ok())
        .and_then(|package| package.install_id);
    registered_identity_for_install(root, app, install_id.as_deref())
        .ok()
        .flatten()
        .is_some()
        && verify_patched(root, app, None).is_ok()
}

pub(crate) fn log_signing_phase(
    root: &Path,
    phase: &str,
    build: &str,
    install_id: &str,
    context: &SigningContext,
) {
    if !matches!(
        phase,
        "signing-identity" | "signing-start" | "signing-finished" | "recovery-committed"
    ) {
        return;
    }
    let identity = match context {
        SigningContext::Adhoc => "signingKind=adhoc".into(),
        SigningContext::Local(identity) => format!(
            "signingKind=local certificateSha1={} certificateSha256={}",
            identity.certificate_sha1, identity.certificate_sha256
        ),
    };
    crate::macos_update_log::log_coordinator_event(
        root,
        &format!("recovery phase={phase} build={build} installId={install_id} {identity}"),
    );
}

/// local 诊断的授权来源是绑定的提交事务，不是随便出现的 ASAR marker。
pub(crate) fn registered_identity_for_install(
    root: &Path,
    app: &Path,
    install_id: Option<&str>,
) -> Result<Option<incodex_macos::LocalSigningIdentity>, String> {
    if inspect_outer_signing(app)?.kind == SignatureKind::Adhoc {
        return Ok(None);
    }
    let Some(identity) = read_signing_identity(root)? else {
        return Ok(None);
    };
    let install_id = install_id.ok_or("local signed app has no install epoch")?;
    let journal = incodex_transaction::journal_v2(root, install_id)?;
    if journal.phase != "COMMITTED"
        || std::fs::canonicalize(app).map_err(|error| error.to_string())?
            != std::fs::canonicalize(&journal.target.real_path)
                .map_err(|error| error.to_string())?
    {
        return Err("local signature is not bound to this committed install target".into());
    }
    incodex_transaction::validate_committed_live_snapshot(root, install_id, app)?;
    incodex_transaction::validate_backup_snapshot(root, install_id)?;
    Ok(Some(identity))
}

/// 复用事务 quiescence 检查时机，防止后台签名模式在预检后漂移。
pub fn validate_restore_generation(
    root: &Path,
    app: &Path,
    install_id: &str,
    helper_sha256: &str,
    certificate_sha256: Option<&str>,
) -> Result<(), String> {
    let current = crate::macos_update_restore::read_registration(root)?
        .ok_or("macOS update registration disappeared before recovery mutation")?;
    if current.install_id != install_id
        || current.app_path != app
        || current.helper_sha256 != helper_sha256
        || current.signing_certificate_sha256.as_deref() != certificate_sha256
    {
        return Err("macOS update recovery signing generation changed before mutation".into());
    }
    Ok(())
}
