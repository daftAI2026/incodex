/*
 * [INPUT]: 依赖 signing 的组件/entitlement 策略、完整性 digest 与调用方的明确签名上下文。
 * [OUTPUT]: 提供 staged/直接 bundle 签名入口，保留外部 vendor，按组件独立 identifier 建立 DR。
 * [POS]: macOS 签名变更唯一实现；签名政策留在 signing/context，避免 CLI 复制 codesign 流程。
 * [PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md
 */
use super::read_plist_info;
use super::signature_inspection::inspect_codesign;
use super::signing::*;
use super::signing_context::{verify_patched_bundle_with_context, SigningContext};
use std::collections::BTreeSet;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

/// 使用共享 entitlement/component policy 完成 ad-hoc 签名。
pub fn sign_app_with_asar_integrity(app: &Path, hash: &str) -> Result<(), String> {
    sign_app_impl(app, Some(hash), None, &SigningContext::Adhoc)
}

/// Sign a staged copy while resolving absolute Mach-O dependencies against the
/// verified final app root. Only dependencies that remain inside that final
/// bundle are translated to their staged counterparts.
pub fn sign_staged_app_with_asar_integrity(
    staged_app: &Path,
    final_app: &Path,
    hash: &str,
) -> Result<(), String> {
    sign_app_impl(
        staged_app,
        Some(hash),
        Some(final_app),
        &SigningContext::Adhoc,
    )
}

pub fn sign_app(app: &Path) -> Result<(), String> {
    sign_app_impl(app, None, None, &SigningContext::Adhoc)
}

struct FrameworkDigestUpdate {
    bundle: PathBuf,
    bundle_identifier: String,
    binary: PathBuf,
    bytes: Vec<u8>,
    entitlements: String,
    hardened_runtime: bool,
}

struct DependentHelperUpdate {
    bundle: PathBuf,
    entitlements: String,
}

fn verified_bundle_identifier(bundle: &Path, plist_identifier: &str) -> Result<String, String> {
    verified_identifier(inspect_component(bundle)?, plist_identifier)
}

fn verified_host_identifier(app: &Path, plist_identifier: &str) -> Result<String, String> {
    // The installer validates the complete original before making its stage.
    // ASAR is intentionally edited in that stage before this signing API;
    // validate the still-sealed executable and Info.plist, not the old ASAR seal.
    // Nested helpers/Frameworks retain full validation before any writes.
    let component = inspect_codesign(app, |path| {
        Command::new("codesign")
            .args(["--verify", "--strict", "--ignore-resources", "--"])
            .arg(path)
            .output()
            .is_ok_and(|output| output.status.success())
    })?;
    verified_identifier(component, plist_identifier)
}

fn first_existing_load_candidate_matches(
    candidates: &[PathBuf],
    framework_binary: &Path,
) -> Result<bool, String> {
    for candidate in candidates {
        match fs::canonicalize(candidate) {
            Ok(resolved) => return Ok(resolved == framework_binary),
            Err(error)
                if matches!(
                    error.kind(),
                    std::io::ErrorKind::NotFound | std::io::ErrorKind::NotADirectory
                ) =>
            {
                continue;
            }
            Err(error) => {
                return Err(format!(
                    "cannot resolve Mach-O dylib dependency {}: {error}",
                    candidate.display()
                ));
            }
        }
    }
    Ok(false)
}

fn linked_dependencies_load_framework(
    dependencies: &[Vec<PathBuf>],
    framework_binary: &Path,
) -> Result<bool, String> {
    for candidates in dependencies {
        if first_existing_load_candidate_matches(candidates, framework_binary)? {
            return Ok(true);
        }
    }
    Ok(false)
}

fn remap_final_app_load_candidates_to_stage(
    dependencies: &mut [Vec<PathBuf>],
    staged_app: &Path,
    final_app: &Path,
) -> Result<(), String> {
    let staged_root = fs::canonicalize(staged_app).map_err(|error| {
        format!(
            "cannot resolve staged app {}: {error}",
            staged_app.display()
        )
    })?;
    let final_root = fs::canonicalize(final_app)
        .map_err(|error| format!("cannot resolve final app {}: {error}", final_app.display()))?;

    for candidate in dependencies.iter_mut().flatten() {
        if !candidate.is_absolute() {
            continue;
        }
        let resolved_final = match fs::canonicalize(&*candidate) {
            Ok(path) => path,
            Err(error)
                if matches!(
                    error.kind(),
                    std::io::ErrorKind::NotFound | std::io::ErrorKind::NotADirectory
                ) =>
            {
                continue;
            }
            Err(error) => {
                return Err(format!(
                    "cannot resolve final-app Mach-O dependency {}: {error}",
                    candidate.display()
                ));
            }
        };
        let Ok(relative) = resolved_final.strip_prefix(&final_root) else {
            // In-bundle symlinks that escape the verified final root are
            // external dependencies, not aliases for staged app contents.
            continue;
        };
        let staged_candidate = staged_root.join(relative);
        match fs::canonicalize(&staged_candidate) {
            Ok(resolved_staged) => {
                if !resolved_staged.starts_with(&staged_root) {
                    return Err(format!(
                        "staged Mach-O dependency escapes app root: {}",
                        staged_candidate.display()
                    ));
                }
                *candidate = resolved_staged;
            }
            Err(error)
                if matches!(
                    error.kind(),
                    std::io::ErrorKind::NotFound | std::io::ErrorKind::NotADirectory
                ) =>
            {
                // Preserve the candidate's place in the dyld search order,
                // but model the path that will exist after stage placement.
                *candidate = staged_candidate;
            }
            Err(error) => {
                return Err(format!(
                    "cannot resolve staged Mach-O dependency {}: {error}",
                    staged_candidate.display()
                ));
            }
        }
    }
    Ok(())
}

fn verified_identifier(
    component: SignedComponent,
    plist_identifier: &str,
) -> Result<String, String> {
    let bundle = &component.path;
    if !component.verified {
        return Err(format!(
            "signature verification failed: {}",
            bundle.display()
        ));
    }
    let identifier = component
        .identifier
        .ok_or_else(|| format!("signature identifier unavailable: {}", bundle.display()))?;
    if identifier != plist_identifier {
        return Err(format!(
            "signature identifier mismatch: {}",
            bundle.display()
        ));
    }
    Ok(identifier)
}

fn entitlement_enabled(source: &EntitlementSnapshot, key: &str) -> Result<bool, String> {
    if !source.keys.contains(key) {
        return Ok(false);
    }
    let mut child = Command::new("plutil")
        .args(["-convert", "json", "-o", "-", "--", "-"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|error| error.to_string())?;
    child
        .stdin
        .take()
        .ok_or("plutil stdin is unavailable")?
        .write_all(source.xml.as_bytes())
        .map_err(|error| error.to_string())?;
    let output = child
        .wait_with_output()
        .map_err(|error| error.to_string())?;
    if !output.status.success() {
        return Err("cannot parse library-validation entitlement".into());
    }
    let raw: serde_json::Value =
        serde_json::from_slice(&output.stdout).map_err(|error| error.to_string())?;
    raw.get(key)
        .and_then(serde_json::Value::as_bool)
        .ok_or_else(|| "library-validation entitlement must be Boolean".into())
}

fn dependent_helper_updates(
    app: &Path,
    frameworks: &[FrameworkDigestUpdate],
    final_app: Option<&Path>,
) -> Result<Vec<DependentHelperUpdate>, String> {
    if frameworks.is_empty() {
        return Ok(Vec::new());
    }
    let app_identifier = read_plist_info(app)
        .ok_or("app identity unavailable")?
        .bundle_identifier;
    let app_identifier = verified_host_identifier(app, &app_identifier)?;
    if let Some(final_app) = final_app {
        let final_identifier = read_plist_info(final_app)
            .ok_or("final app identity unavailable")?
            .bundle_identifier;
        let final_identifier = verified_host_identifier(final_app, &final_identifier)?;
        if app_identifier != final_identifier {
            return Err("staged and final app identities do not match".into());
        }
    }
    let namespace = format!("{app_identifier}.helper");
    let mut updates = Vec::new();
    for bundle in enumerate_component_paths(app)?
        .into_iter()
        .filter(|path| path.extension().is_some_and(|ext| ext == "app"))
    {
        let info = read_plist_info(&bundle).ok_or("nested helper plist unavailable")?;
        if info.executable.is_empty()
            || Path::new(&info.executable).components().count() != 1
            || info.executable == "."
            || info.executable == ".."
        {
            return Err("invalid helper executable name".into());
        }
        let binary = fs::canonicalize(bundle.join("Contents/MacOS").join(&info.executable))
            .map_err(|error| error.to_string())?;
        let canonical_bundle = fs::canonicalize(&bundle).map_err(|error| error.to_string())?;
        if !binary.starts_with(canonical_bundle) {
            return Err("helper executable escapes its bundle".into());
        }
        let bytes = fs::read(&binary).map_err(|error| error.to_string())?;
        let mut dependencies =
            super::asar_integrity_digest::resolved_linked_dylib_paths(&bytes, &binary)?;
        if let Some(final_app) = final_app {
            remap_final_app_load_candidates_to_stage(&mut dependencies, app, final_app)?;
        }
        let mut matching_framework = None;
        for framework in frameworks {
            if linked_dependencies_load_framework(&dependencies, &framework.binary)? {
                if matching_framework.is_some() {
                    return Err("helper loads multiple changed frameworks".into());
                }
                matching_framework = Some(framework);
            }
        }
        if matching_framework.is_none() {
            let dynamic_paths = super::asar_integrity_digest::dynamic_framework_load_paths(&bytes)?;
            for framework in frameworks {
                let dynamically_loads_framework = dynamic_paths.iter().any(|relative| {
                    binary
                        .parent()
                        .and_then(|parent| fs::canonicalize(parent.join(relative)).ok())
                        .is_some_and(|path| path == framework.binary)
                });
                if dynamically_loads_framework {
                    if matching_framework.is_some() {
                        return Err("helper loads multiple changed frameworks".into());
                    }
                    matching_framework = Some(framework);
                }
            }
        }
        let Some(framework) = matching_framework else {
            continue;
        };
        if !inspect_hardened_runtime(&bundle)? {
            continue;
        }
        let source = read_entitlements(&bundle)?;
        if entitlement_enabled(&source, DISABLE_LIBRARY_VALIDATION)? {
            continue;
        }
        let helper_identifier = verified_bundle_identifier(&bundle, &info.bundle_identifier)?;
        // Chromium's notification helper has a Framework-derived identity,
        // separate from Electron's .helper namespace. Admit only that exact
        // role under the host's own Framework, never arbitrary Framework children.
        let is_electron_helper = helper_identifier == namespace
            || helper_identifier.starts_with(&format!("{namespace}."));
        let is_notification_helper = framework.bundle_identifier
            == format!("{app_identifier}.framework")
            && helper_identifier
                == format!("{}.AlertNotificationService", framework.bundle_identifier);
        // CUA sidecars are neither of these identities. An unknown dependent
        // fails closed before any digest, metadata or signature changes.
        if !is_electron_helper && !is_notification_helper {
            return Err(format!(
                "protected or unknown helper requires changed framework: {}",
                bundle.display()
            ));
        }
        let mut source = source;
        if source.keys.remove(DISABLE_LIBRARY_VALIDATION) {
            source.xml = strip_unretainable_entitlements(
                &source.xml,
                &BTreeSet::from([DISABLE_LIBRARY_VALIDATION.to_string()]),
            )?;
        }
        let entitlements = plan_adhoc_entitlements(&source)?.xml;
        updates.push(DependentHelperUpdate {
            bundle,
            entitlements,
        });
    }
    Ok(updates)
}

fn framework_digest_updates(
    app: &Path,
    old: &serde_json::Value,
    new: &serde_json::Value,
) -> Result<Vec<FrameworkDigestUpdate>, String> {
    let mut updates = Vec::new();
    for bundle in enumerate_component_paths(app)?
        .into_iter()
        .filter(|path| path.extension().is_some_and(|ext| ext == "framework"))
    {
        let plist = bundle.join("Resources/Info.plist");
        if !plist.exists() {
            continue;
        }
        let raw = super::read_plist_json_result(&plist)?;
        let name = raw
            .get("CFBundleExecutable")
            .and_then(serde_json::Value::as_str)
            .ok_or("framework has no CFBundleExecutable")?;
        if name.is_empty()
            || Path::new(name).components().count() != 1
            || name == "."
            || name == ".."
        {
            return Err("invalid framework executable name".into());
        }
        let binary = fs::canonicalize(bundle.join(name))
            .map_err(|error| format!("cannot resolve framework executable: {error}"))?;
        if !binary.starts_with(fs::canonicalize(&bundle).map_err(|error| error.to_string())?) {
            return Err("framework executable escapes its bundle".into());
        }
        let bytes = fs::read(&binary).map_err(|error| error.to_string())?;
        if let Some(bytes) =
            super::asar_integrity_digest::plan_integrity_digest_update(&bytes, old, new)?
        {
            let bundle_identifier = raw
                .get("CFBundleIdentifier")
                .and_then(serde_json::Value::as_str)
                .filter(|identifier| !identifier.is_empty())
                .ok_or("framework has no CFBundleIdentifier")?;
            let bundle_identifier = verified_bundle_identifier(&bundle, bundle_identifier)?;
            let source = read_entitlements(&bundle)?;
            let stripped = source
                .keys
                .iter()
                .filter(|key| ADHOC_UNRETAINABLE_ENTITLEMENTS.contains(&key.as_str()))
                .cloned()
                .collect();
            let entitlements = if source.xml.is_empty() {
                empty_entitlements_xml()
            } else {
                strip_unretainable_entitlements(&source.xml, &stripped)?
            };
            updates.push(FrameworkDigestUpdate {
                hardened_runtime: has_hardened_runtime(&bundle),
                bundle_identifier: bundle_identifier.to_string(),
                bundle,
                binary,
                bytes,
                entitlements,
            });
        }
    }
    if updates.len() > 1 {
        return Err(
            "multiple frameworks enforce the host ASAR dictionary; refusing ambiguous mutation"
                .into(),
        );
    }
    Ok(updates)
}

fn sign_app_impl(
    app: &Path,
    integrity_hash: Option<&str>,
    final_app: Option<&Path>,
    context: &SigningContext,
) -> Result<(), String> {
    let before = read_entitlements(app)?;
    let plan = plan_adhoc_entitlements(&before)?;
    let provider = app.join(KEYCHAIN_PROVIDER);
    let modified_codex_framework = provider.is_file().then(|| app.join(CODEX_FRAMEWORK));
    if let Some(framework) = &modified_codex_framework {
        // Provider 的 hash marker 与 LC_LOAD_DYLIB 都是在官方签名之后改写的。
        // 先给二者建立同代 ad-hoc 身份，签名清单才能继续验证真正需要保留的 vendor helpers。
        sign_component(&provider, context)?;
        sign_component(framework, context)?;
    }
    let outer = inspect_component(app)?;
    let integrity = integrity_hash
        .map(|hash| super::asar_integrity_payload(app, hash))
        .transpose()?;
    let updates = match &integrity {
        Some((old, new)) => framework_digest_updates(app, old, new)?,
        None => Vec::new(),
    };
    let helpers = dependent_helper_updates(app, &updates, final_app)?;
    // Validate every existing nested signature before altering any digest. Only the
    // proven digest-bearing framework and necessary loading Electron helpers
    // are excluded; all other vendor children stay stashed.
    let excluded: Vec<_> = updates
        .iter()
        .map(|update| update.bundle.clone())
        .chain(helpers.iter().map(|helper| helper.bundle.clone()))
        .collect();
    let mut preserve = collect_vendor_helper_roots_excluding(app, &outer, &excluded, context)?;
    if let Some(framework) = &modified_codex_framework {
        // Provider-bearing Framework 的 helpers 必须延续实验的同代签名；外部 vendor 保持原样。
        preserve.retain(|path| !path.starts_with(framework));
    }
    let stash_root = if preserve.is_empty() {
        None
    } else {
        let dir = temporary_dir("incodex-vendor");
        fs::create_dir_all(&dir).map_err(|error| error.to_string())?;
        Some(dir)
    };
    let mut stashed = Vec::new();
    if let Some(root) = &stash_root {
        for (index, src) in preserve.iter().enumerate() {
            let dest = root.join(index.to_string()).join(
                src.file_name()
                    .ok_or_else(|| "vendor helper missing name".to_string())?,
            );
            super::ditto(src, &dest)?;
            fs::remove_dir_all(src).map_err(|error| error.to_string())?;
            stashed.push((src.clone(), dest));
        }
    }
    let deep = (|| {
        if let Some((_, new)) = &integrity {
            super::write_asar_integrity_payload(&app.join("Contents/Info.plist"), new)?;
        }
        for update in &updates {
            fs::write(&update.binary, &update.bytes).map_err(|error| error.to_string())?;
        }
        deep_sign_command(app, context)?
            .output()
            .map_err(|error| error.to_string())
            .and_then(command_success)
    })();
    let restore = restore_stashed_helpers(&stashed, stash_root.as_deref());
    if let Err(error) = restore {
        return Err(match deep {
            Ok(()) => error,
            Err(deep_error) => format!("{deep_error}; {error}"),
        });
    }
    deep?;
    stamp_local_components(app, &preserve, context)?;
    for helper in &helpers {
        sign_component_with_entitlements(&helper.bundle, &helper.entitlements, true, context)?;
    }
    for update in &updates {
        sign_component_with_entitlements(
            &update.bundle,
            &update.entitlements,
            update.hardened_runtime,
            context,
        )?;
        if let Some((_, new)) = &integrity {
            let bytes = fs::read(&update.binary).map_err(|error| error.to_string())?;
            // An already matching digest is a no-op, not a validation failure.
            super::asar_integrity_digest::plan_integrity_digest_update(&bytes, new, new)?;
        }
    }
    sign_component_with_entitlements(app, &plan.xml, true, context)?;
    verify_patched_bundle_with_context(app, None, context)
        .map(|_| ())
        .map_err(|error| format!("codesign --verify --deep --strict failed after resign: {error}"))
}

fn sign_component_with_entitlements(
    app: &Path,
    entitlements: &str,
    hardened_runtime: bool,
    context: &SigningContext,
) -> Result<(), String> {
    let root = temporary_dir("incodex-ent");
    fs::create_dir_all(&root).map_err(|error| error.to_string())?;
    let file = root.join("entitlements.plist");
    if let Err(error) = fs::write(&file, entitlements) {
        let _ = fs::remove_dir_all(&root);
        return Err(error.to_string());
    }
    let mut command = Command::new("codesign");
    command.args(["--force"]);
    context.configure_sign_command(&mut command, app, true)?;
    if hardened_runtime {
        command.args(["--options", "runtime"]);
    }
    let result = command
        .args(["--entitlements"])
        .arg(&file)
        .args(["--"])
        .arg(app)
        .output()
        .map_err(|error| error.to_string())
        .and_then(command_success);
    let cleanup = fs::remove_dir_all(root).map_err(|error| error.to_string());
    match (result, cleanup) {
        (Ok(()), Ok(())) => Ok(()),
        (Err(error), Ok(())) => Err(error),
        (Ok(()), Err(error)) => Err(error),
        (Err(sign), Err(cleanup)) => {
            Err(format!("{sign}; failed to clean entitlements: {cleanup}"))
        }
    }
}

fn command_success(output: std::process::Output) -> Result<(), String> {
    if output.status.success() {
        Ok(())
    } else {
        Err(String::from_utf8_lossy(&output.stderr).trim().to_string())
    }
}

fn temporary_dir(prefix: &str) -> PathBuf {
    let suffix = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |duration| duration.as_nanos());
    std::env::temp_dir().join(format!("{prefix}-{}-{suffix}", std::process::id()))
}

fn restore_stashed_helpers(
    stashed: &[(PathBuf, PathBuf)],
    stash_root: Option<&Path>,
) -> Result<(), String> {
    let mut failures = Vec::new();
    for (src, dest) in stashed {
        if let Err(error) = super::ditto(dest, src) {
            failures.push(format!("failed to restore {}: {error}", src.display()));
        }
    }
    if failures.is_empty() {
        if let Some(root) = stash_root {
            if let Err(error) = fs::remove_dir_all(root) {
                failures.push(format!(
                    "failed to remove vendor stash {}: {error}",
                    root.display()
                ));
            }
        }
    }
    if failures.is_empty() {
        Ok(())
    } else {
        Err(failures.join("; "))
    }
}

/// 显式上下文版本只用于已注册身份的产品安装；旧入口保持 ad-hoc 合同。
pub fn sign_staged_app_with_context(
    staged: &Path,
    final_app: &Path,
    hash: &str,
    context: &SigningContext,
) -> Result<(), String> {
    sign_app_impl(staged, Some(hash), Some(final_app), context)
}

fn sign_component(path: &Path, context: &SigningContext) -> Result<(), String> {
    let mut command = Command::new("codesign");
    command.arg("--force");
    context.configure_sign_command(&mut command, path, true)?;
    command
        .arg("--")
        .arg(path)
        .output()
        .map_err(|error| error.to_string())
        .and_then(command_success)
}

fn deep_sign_command(app: &Path, context: &SigningContext) -> Result<Command, String> {
    let mut command = Command::new("codesign");
    command.args(["--force", "--deep"]);
    // deep 不能把 host 的 DR 套给 children；下一步从内向外写各自的 DR。
    context.configure_sign_command(&mut command, app, false)?;
    command.arg("--").arg(app);
    Ok(command)
}

fn stamp_local_components(
    app: &Path,
    preserved: &[PathBuf],
    context: &SigningContext,
) -> Result<(), String> {
    if matches!(context, SigningContext::Adhoc) {
        return Ok(());
    }
    let mut paths = enumerate_component_paths(app)?;
    paths.retain(|path| !preserved.iter().any(|root| path.starts_with(root)));
    paths.sort_by_key(|path| std::cmp::Reverse(path.components().count()));
    for path in paths {
        let entitlements = read_entitlements(&path)?;
        // DR 写入只保留现有权限，不能把 host 所需的 library-validation 例外
        // 扩散到未改动的 updater/子组件；原有明确变更组件的策略仍在各自流程。
        let stripped = entitlements
            .keys
            .iter()
            .filter(|key| ADHOC_UNRETAINABLE_ENTITLEMENTS.contains(&key.as_str()))
            .cloned()
            .collect();
        let xml = if entitlements.xml.is_empty() {
            empty_entitlements_xml()
        } else {
            strip_unretainable_entitlements(&entitlements.xml, &stripped)?
        };
        sign_component_with_entitlements(&path, &xml, inspect_hardened_runtime(&path)?, context)?;
    }
    Ok(())
}
