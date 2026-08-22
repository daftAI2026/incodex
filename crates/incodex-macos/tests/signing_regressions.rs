use std::collections::BTreeSet;
use std::ffi::OsString;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

use incodex_macos::{
    inspect_signing_inventory, plan_adhoc_entitlements, sign_app, verify_app,
    verify_original_vendor_bundle, verify_patched_adhoc_bundle_deep_strict, EntitlementSnapshot,
};

static PATH_LOCK: Mutex<()> = Mutex::new(());

struct PathGuard(Option<OsString>);

impl Drop for PathGuard {
    fn drop(&mut self) {
        match &self.0 {
            Some(path) => std::env::set_var("PATH", path),
            None => std::env::remove_var("PATH"),
        }
    }
}

#[derive(Clone, Copy)]
enum NestedIdentity {
    Vendor,
    VendorWithoutAuthority,
    Other,
    OtherWithoutAuthority,
}

#[derive(Clone, Copy)]
enum OuterIdentity {
    Adhoc,
    Vendor(&'static str),
    ThirdParty,
}

struct Fixture {
    root: PathBuf,
    app: PathBuf,
    fake_bin: PathBuf,
    display_count: PathBuf,
    malformed_entitlements: bool,
}

impl Fixture {
    fn new(
        nested_identity: NestedIdentity,
        deep_nested: bool,
        outer_identity: OuterIdentity,
        malformed_entitlements: bool,
    ) -> Self {
        let root = std::env::temp_dir().join(format!(
            "incodex-signing-regression-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let app = root.join("ChatGPT.app");
        let fake_bin = root.join("fake-bin");
        let display_count = root.join("display-count");
        fs::create_dir_all(app.join("Contents/MacOS")).unwrap();
        fs::create_dir_all(&fake_bin).unwrap();
        fs::write(app.join("Contents/MacOS/ChatGPT"), "binary\n").unwrap();
        fs::write(
            app.join("Contents/Info.plist"),
            r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
<key>CFBundleIdentifier</key><string>com.expected.bundle</string>
<key>CFBundleShortVersionString</key><string>1.0.0</string>
<key>CFBundleVersion</key><string>1</string>
<key>CFBundleExecutable</key><string>ChatGPT</string>
</dict></plist>
"#,
        )
        .unwrap();

        let component = if deep_nested {
            let parent = app.join("Contents/Frameworks/OuterVendor.app");
            let deep = parent.join("Contents/Frameworks/DeepVendor.xpc");
            fs::create_dir_all(deep.join("Contents/_CodeSignature")).unwrap();
            parent
        } else {
            let nested = app.join("Contents/Frameworks/NestedVendor.xpc");
            fs::create_dir_all(nested.join("Contents/_CodeSignature")).unwrap();
            nested
        };
        if deep_nested {
            fs::create_dir_all(component.join("Contents/_CodeSignature")).unwrap();
        }

        let nested_display = match nested_identity {
            NestedIdentity::Vendor => {
                "printf '%s\\n' 'Identifier=com.example.vendor' 'TeamIdentifier=2DC432GLL2' 'Authority=Developer ID Application: fixture'"
            }
            NestedIdentity::VendorWithoutAuthority => {
                "printf '%s\\n' 'Identifier=com.example.vendor' 'TeamIdentifier=2DC432GLL2'"
            }
            NestedIdentity::Other => {
                "printf '%s\\n' 'Identifier=com.example.other' 'TeamIdentifier=OTHERTEAM' 'Authority=Other Signer'"
            }
            NestedIdentity::OtherWithoutAuthority => {
                "printf '%s\\n' 'Identifier=com.example.other' 'TeamIdentifier=OTHERTEAM'"
            }
        };
        let outer_display = match outer_identity {
            OuterIdentity::Vendor(identifier) => format!(
                "printf '%s\\n' 'Identifier={identifier}' 'TeamIdentifier=2DC432GLL2' 'Authority=Developer ID Application: fixture'"
            ),
            OuterIdentity::ThirdParty => "printf '%s\\n' 'Identifier=com.example.third-party' 'TeamIdentifier=THIRDPARTY' 'Authority=Developer ID Application: third-party fixture'".to_string(),
            OuterIdentity::Adhoc => {
                "printf '%s\\n' 'Identifier=com.example.fixture' 'Signature=adhoc'".to_string()
            }
        };
        let script = format!(
            r#"#!/bin/sh
target=""
for arg in "$@"; do target="$arg"; done
if [ "$1" = "--display" ] && [ "$2" = "--entitlements" ]; then
  printf '%s' "$INCODEX_CODESIGN_ENTITLEMENTS_OUTPUT"
  exit 0
fi
if [ "$1" = "--display" ] && [ "$2" = "--verbose=4" ]; then
  count="$(cat "$INCODEX_CODESIGN_DISPLAY_COUNT" 2>/dev/null || echo 0)"
  echo $((count + 1)) > "$INCODEX_CODESIGN_DISPLAY_COUNT"
  case "$target" in
    *NestedVendor.xpc|*OuterVendor.app|*DeepVendor.xpc)
      {nested_display}
      ;;
    *)
      {outer_display}
      ;;
  esac
  exit 0
fi
if [ "$1" = "--force" ] && [ "$2" = "--sign" ]; then
  printf '%s\n' signed > "$INCODEX_SIGN_CAPTURE"
  exit 0
fi
if [ "$1" = "--force" ] && [ "$2" = "--deep" ]; then exit 0; fi
if [ "$1" = "--verify" ] && [ "$INCODEX_CODESIGN_VERIFY_FAILURE" = "1" ]; then exit 1; fi
if [ "$1" = "--verify" ]; then exit 0; fi
exit 0
"#
        );
        let codesign = fake_bin.join("codesign");
        fs::write(&codesign, script).unwrap();
        let mut permissions = fs::metadata(&codesign).unwrap().permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(&codesign, permissions).unwrap();
        Self {
            root,
            app,
            fake_bin,
            display_count,
            malformed_entitlements,
        }
    }

    fn install_path(&self) -> OsString {
        let mut path = OsString::from(self.fake_bin.as_os_str());
        path.push(":");
        if let Some(existing) = std::env::var_os("PATH") {
            path.push(existing);
        }
        path
    }

    fn configure_environment(&self) -> PathGuard {
        let original_path = std::env::var_os("PATH");
        let guard = PathGuard(original_path);
        std::env::set_var("PATH", self.install_path());
        std::env::set_var(
            "INCODEX_CODESIGN_ENTITLEMENTS_OUTPUT",
            if self.malformed_entitlements {
                "not a plist"
            } else {
                "<?xml version=\"1.0\"?><plist><dict><key>com.apple.security.cs.allow-jit</key><true/></dict></plist>"
            },
        );
        std::env::set_var("INCODEX_SIGN_CAPTURE", self.root.join("sign-capture"));
        std::env::set_var("INCODEX_CODESIGN_DISPLAY_COUNT", &self.display_count);
        guard
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

#[test]
fn generic_verify_rejects_a_nested_other_without_identity_evidence() {
    let _path_lock = PATH_LOCK.lock().unwrap_or_else(|error| error.into_inner());
    let fixture = Fixture::new(
        NestedIdentity::OtherWithoutAuthority,
        false,
        OuterIdentity::Vendor("com.expected.bundle"),
        false,
    );
    let _path = fixture.configure_environment();

    assert!(!verify_app(&fixture.app));
}

#[test]
fn generic_verify_rejects_deep_strict_invalid_outer_without_nested_components() {
    let _path_lock = PATH_LOCK.lock().unwrap_or_else(|error| error.into_inner());
    let fixture = Fixture::new(NestedIdentity::Vendor, false, OuterIdentity::Adhoc, false);
    let _path = fixture.configure_environment();
    fs::remove_dir_all(fixture.app.join("Contents/Frameworks/NestedVendor.xpc")).unwrap();
    std::env::set_var("INCODEX_CODESIGN_VERIFY_FAILURE", "1");

    assert!(!verify_app(&fixture.app));

    std::env::remove_var("INCODEX_CODESIGN_VERIFY_FAILURE");
}

#[test]
fn generic_verify_accepts_a_verified_third_party_outer_with_identity_evidence() {
    let _path_lock = PATH_LOCK.lock().unwrap_or_else(|error| error.into_inner());
    let fixture = Fixture::new(
        NestedIdentity::Vendor,
        false,
        OuterIdentity::ThirdParty,
        false,
    );
    let _path = fixture.configure_environment();

    assert!(verify_app(&fixture.app));
    assert!(verify_patched_adhoc_bundle_deep_strict(&fixture.app, None).is_err());
    assert!(
        verify_original_vendor_bundle(&fixture.app, Some("com.expected.bundle"), None, None)
            .is_err()
    );
}

#[test]
fn generic_verify_accepts_a_verified_third_party_nested_component() {
    let _path_lock = PATH_LOCK.lock().unwrap_or_else(|error| error.into_inner());
    let fixture = Fixture::new(
        NestedIdentity::Other,
        false,
        OuterIdentity::ThirdParty,
        false,
    );
    let _path = fixture.configure_environment();

    assert!(verify_app(&fixture.app));
}

#[test]
fn verify_app_reuses_one_signing_inventory() {
    let _path_lock = PATH_LOCK.lock().unwrap_or_else(|error| error.into_inner());
    let fixture = Fixture::new(
        NestedIdentity::Vendor,
        false,
        OuterIdentity::ThirdParty,
        false,
    );
    let _path = fixture.configure_environment();

    assert!(verify_app(&fixture.app));
    let count = fs::read_to_string(&fixture.display_count)
        .unwrap()
        .trim()
        .parse::<u32>()
        .unwrap();
    assert_eq!(
        count, 2,
        "outer and nested components should be inspected once"
    );
    std::env::remove_var("INCODEX_CODESIGN_DISPLAY_COUNT");
}

#[test]
fn official_vendor_acceptance_requires_outer_signature_identifier() {
    let _path_lock = PATH_LOCK.lock().unwrap_or_else(|error| error.into_inner());
    let fixture = Fixture::new(
        NestedIdentity::Vendor,
        false,
        OuterIdentity::Vendor("com.actual.bundle"),
        false,
    );
    let _path = fixture.configure_environment();

    let result =
        verify_original_vendor_bundle(&fixture.app, Some("com.expected.bundle"), None, None);
    assert!(
        result.is_err(),
        "outer signature identifier must match the expected bundle"
    );
}

#[test]
fn inventory_recurses_into_signed_bundle_components() {
    let _path_lock = PATH_LOCK.lock().unwrap_or_else(|error| error.into_inner());
    let fixture = Fixture::new(NestedIdentity::Vendor, true, OuterIdentity::Adhoc, false);
    let _path = fixture.configure_environment();

    let inventory = inspect_signing_inventory(&fixture.app).unwrap();
    assert!(inventory
        .nested
        .iter()
        .any(|component| component.path.ends_with("DeepVendor.xpc")));
}

#[test]
fn nested_vendor_without_authority_is_rejected_before_signing() {
    let _path_lock = PATH_LOCK.lock().unwrap_or_else(|error| error.into_inner());
    let fixture = Fixture::new(
        NestedIdentity::VendorWithoutAuthority,
        false,
        OuterIdentity::Adhoc,
        false,
    );
    let _path = fixture.configure_environment();

    let result = sign_app(&fixture.app);
    assert!(result.is_err(), "vendor components need authority evidence");
    assert!(!fixture.root.join("sign-capture").exists());
}

#[test]
fn successful_nonempty_malformed_entitlements_fail_closed() {
    let _path_lock = PATH_LOCK.lock().unwrap_or_else(|error| error.into_inner());
    let fixture = Fixture::new(NestedIdentity::Vendor, false, OuterIdentity::Adhoc, true);
    let _path = fixture.configure_environment();

    let result = sign_app(&fixture.app);
    assert!(
        result.is_err(),
        "malformed successful entitlement output must fail closed"
    );
    assert!(!fixture.root.join("sign-capture").exists());
}

#[test]
fn strips_a_self_closing_unretainable_entitlement_value() {
    let key = "com.apple.application-identifier".to_string();
    let source = EntitlementSnapshot {
        xml: "<?xml version=\"1.0\"?><plist><dict><key>com.apple.application-identifier</key><array/></dict></plist>".to_string(),
        keys: BTreeSet::from([key.clone()]),
    };

    let plan = plan_adhoc_entitlements(&source).expect("self-closing plist values are valid");
    assert!(plan.stripped_keys.contains(&key));
    assert!(!plan.xml.contains("<array/>"));
}

#[test]
fn adds_entitlement_to_the_root_dictionary() {
    let source = EntitlementSnapshot {
        xml: "<?xml version=\"1.0\"?><plist><dict><key>nested</key><dict><key>child</key><string>x</string></dict></dict></plist>".to_string(),
        keys: BTreeSet::new(),
    };

    let plan = plan_adhoc_entitlements(&source).expect("nested entitlement plist is valid");
    let marker = "<key>com.apple.security.cs.disable-library-validation</key>";
    let marker_position = plan.xml.find(marker).unwrap();
    let nested_close = plan.xml.find("</dict>").unwrap();
    let root_close = plan.xml.rfind("</dict>").unwrap();
    assert!(nested_close < marker_position && marker_position < root_close);
}
