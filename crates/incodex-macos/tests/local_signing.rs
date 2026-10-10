/*
 * [INPUT]: 依赖明确的本机签名身份与共享签名验收接口，仅使用临时私有文件。
 * [OUTPUT]: 约束证书绑定 DR、组件 identifier 分离、输入拒绝与签名失败前无修改。
 * [POS]: incodex-macos 的稳定签名身份失败先行合同，不以签名测试替代真实宿主 AX。
 * [PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md
 */
use incodex_macos::{verify_patched_bundle_with_context, LocalSigningIdentity, SigningContext};
use std::path::PathBuf;

fn identity() -> LocalSigningIdentity {
    LocalSigningIdentity::new(
        PathBuf::from("/private/tmp/synthetic.keychain-db"),
        "a".repeat(40),
        "b".repeat(64),
    )
    .unwrap()
}

#[test]
fn local_requirement_binds_certificate_and_each_components_own_identifier() {
    let identity = identity();
    let host = identity.requirement("com.example.host").unwrap();
    let helper = identity.requirement("com.example.host.helper").unwrap();
    assert!(host.contains("certificate leaf = H\"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa\""));
    assert!(host.contains("identifier \"com.example.host\""));
    assert!(helper.contains("identifier \"com.example.host.helper\""));
    assert_ne!(host, helper);
    assert!(!host.contains("2DC432GLL2"));
}

#[test]
fn malformed_certificate_or_identifier_is_rejected() {
    assert!(LocalSigningIdentity::new(
        PathBuf::from("relative.keychain"),
        "a".repeat(40),
        "b".repeat(64)
    )
    .is_err());
    assert!(LocalSigningIdentity::new(
        PathBuf::from("/private/tmp/test.keychain"),
        "bad".into(),
        "b".repeat(64)
    )
    .is_err());
    assert!(LocalSigningIdentity::new(
        PathBuf::from("/private/tmp/test.keychain"),
        "a".repeat(40),
        "bad".into()
    )
    .is_err());
    assert!(identity().requirement("invalid\" or anchor apple").is_err());
}

#[test]
fn local_verifier_does_not_accept_missing_app_or_fallback_to_adhoc() {
    let context = SigningContext::Local(identity());
    assert!(verify_patched_bundle_with_context(
        std::path::Path::new("/private/tmp/incodex-missing-synthetic-host.app"),
        None,
        &context
    )
    .is_err());
}

#[test]
fn self_signed_without_team_is_not_generic_identity_evidence() {
    use incodex_macos::{
        validate_generic_signing_inventory, EntitlementSnapshot, SignatureKind, SignedComponent,
        SigningInventory,
    };
    let inventory = SigningInventory {
        outer: SignedComponent {
            path: PathBuf::from("/private/tmp/self-signed.app"),
            identifier: Some("com.example.host".into()),
            team_identifier: Some("not set".into()),
            authorities: vec!["local certificate".into()],
            kind: SignatureKind::Other,
            verified: true,
        },
        nested: vec![],
        entitlements: EntitlementSnapshot {
            xml: String::new(),
            keys: Default::default(),
        },
        deep_strict: true,
    };
    assert!(validate_generic_signing_inventory(&inventory).is_err());
}
