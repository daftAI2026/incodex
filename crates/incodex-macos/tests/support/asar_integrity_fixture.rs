/**
 * [INPUT]: 依赖 clang/codesign 构建仅含合成数据的临时 Mach-O 与 ad-hoc bundle。
 * [OUTPUT]: 为完整性及 Keychain 共存回归提供已签名 fixture 与构建命令。
 * [POS]: incodex-macos 测试的共用证据生成器，不读取真实 App 或 Keychain。
 * [PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md
 */
use std::{
    fs,
    path::Path,
    process::Command,
    sync::atomic::{AtomicU64, Ordering},
    time::{SystemTime, UNIX_EPOCH},
};

static FIXTURE_COUNTER: AtomicU64 = AtomicU64::new(0);

pub(super) fn run(program: &str, args: &[&str]) {
    let output = Command::new(program).args(args).output().unwrap();
    assert!(
        output.status.success(),
        "{program}: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}
pub(super) fn write_plist(path: &Path, executable: &str, identifier: &str, extra: &str) {
    fs::write(path, format!(r#"<?xml version="1.0"?><plist version="1.0"><dict><key>CFBundleExecutable</key><string>{executable}</string><key>CFBundleIdentifier</key><string>{identifier}</string><key>CFBundleVersion</key><string>1</string>{extra}</dict></plist>"#)).unwrap();
}

pub(super) struct SignedFixture {
    pub(super) root: std::path::PathBuf,
    pub(super) app: std::path::PathBuf,
    pub(super) framework: std::path::PathBuf,
    pub(super) binary: std::path::PathBuf,
    pub(super) helper: std::path::PathBuf,
    pub(super) helper_binary: std::path::PathBuf,
}

#[derive(Clone, Copy, Eq, PartialEq)]
pub(super) enum AbsoluteLoadTarget {
    None,
    FinalApp,
    External,
}

pub(super) fn signed_fixture(helper_identifier: &str) -> SignedFixture {
    signed_fixture_with_loader(helper_identifier, false)
}

pub(super) fn signed_fixture_with_loader(helper_identifier: &str, dynamic: bool) -> SignedFixture {
    signed_fixture_with_loader_location(helper_identifier, dynamic, false)
}

pub(super) fn signed_fixture_with_loader_location(
    helper_identifier: &str,
    dynamic: bool,
    sibling_helper: bool,
) -> SignedFixture {
    signed_fixture_with_linkage(
        helper_identifier,
        dynamic,
        sibling_helper,
        false,
        false,
        false,
    )
}

pub(super) fn signed_fixture_with_app_descendant_loader(helper_identifier: &str) -> SignedFixture {
    signed_fixture_with_linkage(helper_identifier, false, false, true, false, false)
}

pub(super) fn signed_fixture_with_rpath_loader(
    helper_identifier: &str,
    sibling_helper: bool,
    shadow_first_rpath: bool,
) -> SignedFixture {
    signed_fixture_with_linkage(
        helper_identifier,
        false,
        sibling_helper,
        false,
        true,
        shadow_first_rpath,
    )
}

pub(super) fn signed_fixture_with_linkage(
    helper_identifier: &str,
    dynamic: bool,
    sibling_helper: bool,
    app_descendant_helper: bool,
    rpath_link: bool,
    shadow_first_rpath: bool,
) -> SignedFixture {
    signed_fixture_with_linkage_options(
        helper_identifier,
        dynamic,
        sibling_helper,
        app_descendant_helper,
        rpath_link,
        shadow_first_rpath,
        AbsoluteLoadTarget::None,
    )
}

pub(super) fn signed_fixture_with_absolute_loader(helper_identifier: &str) -> SignedFixture {
    signed_fixture_with_linkage_options(
        helper_identifier,
        false,
        true,
        false,
        false,
        false,
        AbsoluteLoadTarget::FinalApp,
    )
}

pub(super) fn signed_fixture_with_external_absolute_loader(
    helper_identifier: &str,
) -> SignedFixture {
    signed_fixture_with_linkage_options(
        helper_identifier,
        false,
        true,
        false,
        false,
        false,
        AbsoluteLoadTarget::External,
    )
}

pub(super) fn signed_fixture_with_linkage_options(
    helper_identifier: &str,
    dynamic: bool,
    sibling_helper: bool,
    app_descendant_helper: bool,
    rpath_link: bool,
    shadow_first_rpath: bool,
    absolute_load_target: AbsoluteLoadTarget,
) -> SignedFixture {
    let root = std::env::temp_dir().join(format!(
        "incodex-integrity-signing-{}-{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos(),
        FIXTURE_COUNTER.fetch_add(1, Ordering::Relaxed)
    ));
    let app = root.join("ChatGPT.app");
    let framework = app.join("Contents/Frameworks/Renamed.framework");
    fs::create_dir_all(app.join("Contents/MacOS")).unwrap();
    fs::create_dir_all(framework.join("Resources")).unwrap();
    let digest = "6f22b7a4f82a2d9f48798c779ac3eec57d2cf91e549ce42866b193dd2ea3ec67";
    let bytes = (0..64)
        .step_by(2)
        .map(|i| format!("0x{}", &digest[i..i + 2]))
        .collect::<Vec<_>>()
        .join(",");
    let source = root.join("framework.c");
    fs::write(&source, format!(r#"__attribute__((used,section("__DATA_CONST,__asar_integrity"))) const struct {{ char sentinel[32]; unsigned char used, version, digest[32]; }} slot = {{"AGbevlPCksUGKNL8TSn7wGmJEuJsXb2A", 1, 1, {{{bytes}}}}}; int fixture(void) {{return 1;}}"#)).unwrap();
    let binary = framework.join("Renamed");
    let absolute_install_name = format!("-Wl,-install_name,{}", binary.display());
    let mut framework_args = vec!["-dynamiclib", source.to_str().unwrap()];
    if absolute_load_target == AbsoluteLoadTarget::FinalApp {
        framework_args.push(&absolute_install_name);
    } else if rpath_link {
        framework_args.push("-Wl,-install_name,@rpath/Renamed.framework/Renamed");
    }
    framework_args.extend(["-o", binary.to_str().unwrap()]);
    run("clang", &framework_args);
    let external_binary = root.join("external/Unrelated.dylib");
    if absolute_load_target == AbsoluteLoadTarget::External {
        fs::create_dir_all(external_binary.parent().unwrap()).unwrap();
        let external_source = root.join("external.c");
        fs::write(&external_source, "int fixture(void) { return 1; }").unwrap();
        let external_install_name = format!("-Wl,-install_name,{}", external_binary.display());
        run(
            "clang",
            &[
                "-dynamiclib",
                external_source.to_str().unwrap(),
                &external_install_name,
                "-o",
                external_binary.to_str().unwrap(),
            ],
        );
    }
    let main = root.join("main.c");
    fs::write(&main, "int main(void) {return 0;}").unwrap();
    run(
        "clang",
        &[
            main.to_str().unwrap(),
            "-o",
            app.join("Contents/MacOS/ChatGPT").to_str().unwrap(),
        ],
    );
    write_plist(
        &framework.join("Resources/Info.plist"),
        "Renamed",
        "com.openai.codex.framework",
        "",
    );
    if shadow_first_rpath {
        assert!(rpath_link && sibling_helper);
        let shadow = app.join("Contents/Frameworks/Mask/Renamed.framework");
        fs::create_dir_all(shadow.join("Resources")).unwrap();
        let shadow_source = root.join("shadow.c");
        fs::write(&shadow_source, "int fixture(void) { return 1; }").unwrap();
        run(
            "clang",
            &[
                "-dynamiclib",
                shadow_source.to_str().unwrap(),
                "-Wl,-install_name,@rpath/Renamed.framework/Renamed",
                "-o",
                shadow.join("Renamed").to_str().unwrap(),
            ],
        );
        write_plist(
            &shadow.join("Resources/Info.plist"),
            "Renamed",
            "com.openai.codex.framework.mask",
            "",
        );
        run(
            "codesign",
            &["--force", "--sign", "-", shadow.to_str().unwrap()],
        );
    }
    let integrity = format!("<key>ElectronAsarIntegrity</key><dict><key>Resources/app.asar</key><dict><key>algorithm</key><string>SHA256</string><key>hash</key><string>{}</string></dict></dict>", "a".repeat(64));
    write_plist(
        &app.join("Contents/Info.plist"),
        "ChatGPT",
        "com.openai.codex",
        &integrity,
    );
    let helper = if app_descendant_helper {
        app.join("Contents/Helpers/LinkedHelper.app")
    } else if sibling_helper {
        app.join("Contents/Frameworks/LinkedHelper.app")
    } else {
        framework.join("Helpers/LinkedHelper.app")
    };
    fs::create_dir_all(helper.join("Contents/MacOS")).unwrap();
    write_plist(
        &helper.join("Contents/Info.plist"),
        "LinkedHelper",
        helper_identifier,
        "",
    );
    let helper_source = root.join("helper.c");
    let source = if dynamic {
        r#"#include <dlfcn.h>
#include <mach-o/dyld.h>
#include <libgen.h>
#include <stdio.h>
int main(void) { char exe[4096], path[8192]; uint32_t size=sizeof(exe);
if (_NSGetExecutablePath(exe,&size)) return 1;
snprintf(path,sizeof(path),"%s/%s",dirname(exe),"../../../../Renamed");
void *handle=dlopen(path,RTLD_LAZY); if (!handle) return 2;
return dlsym(handle,"ChromeMain") ? 0 : 3; }
"#
    } else {
        "extern int fixture(void); int main(void) {return fixture()-1;}"
    };
    fs::write(&helper_source, source).unwrap();
    let helper_binary = helper.join("Contents/MacOS/LinkedHelper");
    let mut args = vec![helper_source.to_str().unwrap()];
    if absolute_load_target == AbsoluteLoadTarget::External {
        args.push(external_binary.to_str().unwrap());
    }
    if rpath_link {
        args.extend(["-F", framework.parent().unwrap().to_str().unwrap()]);
        args.extend(["-framework", "Renamed"]);
        if shadow_first_rpath {
            args.push("-Wl,-rpath,@loader_path/../../../Mask");
        }
        args.push(if sibling_helper {
            "-Wl,-rpath,@loader_path/../../../"
        } else {
            "-Wl,-rpath,@loader_path/../../../../../"
        });
    } else if !dynamic && absolute_load_target != AbsoluteLoadTarget::External {
        args.push(binary.to_str().unwrap());
    }
    args.extend(["-o", helper_binary.to_str().unwrap()]);
    run("clang", &args);
    let entitlements = root.join("helper-entitlements.plist");
    fs::write(&entitlements, r#"<?xml version="1.0"?><plist><dict><key>com.apple.security.cs.allow-jit</key><true/></dict></plist>"#).unwrap();
    run(
        "codesign",
        &[
            "--force",
            "--sign",
            "-",
            "--options",
            "runtime",
            "--entitlements",
            entitlements.to_str().unwrap(),
            helper.to_str().unwrap(),
        ],
    );
    run(
        "codesign",
        &["--force", "--sign", "-", framework.to_str().unwrap()],
    );
    run(
        "codesign",
        &["--force", "--sign", "-", app.to_str().unwrap()],
    );
    let helper_binary = helper.join("Contents/MacOS/LinkedHelper");
    SignedFixture {
        root,
        app,
        framework,
        binary,
        helper,
        helper_binary,
    }
}
