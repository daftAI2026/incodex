fn parse_codex_manifest_applications(
    _manifest_xml: &str,
) -> Result<Vec<crate::windows_app::WindowsManifestApplication>, String> {
    Err("native manifest adapter not implemented".to_string())
}

pub(crate) fn validate_staged_codex_package(
    _package: &windows::ApplicationModel::Package,
) -> Result<String, String> {
    Err("native package adapter not implemented".to_string())
}

pub(crate) fn registered_codex_package(
    _full_name: &str,
) -> Result<crate::windows_app::WindowsCodexApp, String> {
    Err("native package adapter not implemented".to_string())
}

pub(crate) fn codex_package_full_name_registered(_full_name: &str) -> Result<bool, String> {
    Err("native package adapter not implemented".to_string())
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU64, Ordering};

    use crate::windows_app::{
        inspect_codex_package, WindowsCodexApp, WindowsPackageEvidence,
    };

    use super::parse_codex_manifest_applications;

    const PACKAGE_FAMILY_NAME: &str = "OpenAI.Codex_2p2nqsd0c76g0";
    const PACKAGE_FULL_NAME: &str = "OpenAI.Codex_1.2.3.4_x64__2p2nqsd0c76g0";
    const APPX_NAMESPACE: &str = "http://schemas.microsoft.com/appx/manifest/foundation/windows10";
    const MANIFEST: &str = r#"<?xml version="1.0" encoding="utf-8"?>
<Package xmlns="http://schemas.microsoft.com/appx/manifest/foundation/windows10"
         xmlns:uap="http://schemas.microsoft.com/appx/manifest/uap/windows10"
         xmlns:rescap="http://schemas.microsoft.com/appx/manifest/foundation/windows10/restrictedcapabilities"
         IgnorableNamespaces="uap rescap">
  <Identity Name="OpenAI.Codex" Publisher="CN=OpenAI" Version="1.2.3.4" ProcessorArchitecture="x64" />
  <Properties>
    <DisplayName>ChatGPT</DisplayName>
    <PublisherDisplayName>OpenAI</PublisherDisplayName>
    <Logo>Assets\StoreLogo.png</Logo>
  </Properties>
  <Dependencies>
    <TargetDeviceFamily Name="Windows.Desktop" MinVersion="10.0.19041.0" MaxVersionTested="10.0.22631.0" />
  </Dependencies>
  <Resources><Resource Language="en-us" /></Resources>
  <Applications>
    <Application Id="ChatGPT" Executable="app/ChatGPT.exe" EntryPoint="Windows.FullTrustApplication">
      <uap:VisualElements DisplayName="ChatGPT" Description="ChatGPT" Square44x44Logo="Assets\Square44x44Logo.png" Square150x150Logo="Assets\Square150x150Logo.png" BackgroundColor="transparent" />
    </Application>
  </Applications>
  <Capabilities><rescap:Capability Name="runFullTrust" /></Capabilities>
</Package>"#;

    static NEXT_FIXTURE: AtomicU64 = AtomicU64::new(0);

    struct PackageFixture {
        root: PathBuf,
    }

    impl PackageFixture {
        fn new(manifest: &str) -> Self {
            let sequence = NEXT_FIXTURE.fetch_add(1, Ordering::Relaxed);
            let root = std::env::temp_dir().join(format!(
                "incodex-package-native-{}-{sequence}",
                std::process::id()
            ));
            fs::create_dir(&root).expect("create isolated package fixture");
            fs::create_dir_all(root.join("app")).expect("create package application directory");
            fs::write(root.join("AppxManifest.xml"), manifest).expect("write package manifest");
            fs::write(root.join("app").join("ChatGPT.exe"), b"fixture")
                .expect("write package executable");
            Self { root }
        }

        fn inspect(&self, manifest: &str) -> Result<WindowsCodexApp, String> {
            let applications = parse_codex_manifest_applications(manifest)?;
            inspect_codex_package(WindowsPackageEvidence {
                name: "OpenAI.Codex".to_string(),
                package_full_name: PACKAGE_FULL_NAME.to_string(),
                package_family_name: PACKAGE_FAMILY_NAME.to_string(),
                applications,
                install_location: self.root.clone(),
                architecture: "X64".to_string(),
                signature_kind: "Store".to_string(),
                status: "Ok".to_string(),
            })
        }
    }

    impl Drop for PackageFixture {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.root);
        }
    }

    #[test]
    fn parses_application_id_and_executable_from_appx_manifest_xml() {
        let applications = parse_codex_manifest_applications(MANIFEST)
            .expect("parse the AppX manifest application list");

        assert_eq!(applications.len(), 1);
        assert_eq!(applications[0].application_id, "ChatGPT");
        assert_eq!(
            applications[0].application_executable,
            PathBuf::from("app/ChatGPT.exe")
        );
    }

    #[test]
    fn multiple_chatgpt_targets_are_rejected_by_the_shared_inspector() {
        let second_application = r#"<Application Id="ChatGPT.Second" Executable="app/ChatGPT.exe" EntryPoint="Windows.FullTrustApplication" />"#;
        let manifest = MANIFEST.replace(
            "  </Applications>",
            &format!("    {second_application}\n  </Applications>"),
        );
        let fixture = PackageFixture::new(&manifest);

        let error = fixture
            .inspect(&manifest)
            .expect_err("the shared inspector must reject ambiguous launch targets");
        assert!(
            error.contains("exactly one Codex application executable"),
            "unexpected error: {error}"
        );
    }

    #[test]
    fn unknown_appx_namespace_is_rejected() {
        let manifest = MANIFEST.replace(APPX_NAMESPACE, "urn:unknown:appx-manifest");
        let fixture = PackageFixture::new(&manifest);

        let error = fixture
            .inspect(&manifest)
            .expect_err("the parser must reject an unknown AppX manifest namespace");
        assert!(
            error.to_ascii_lowercase().contains("namespace"),
            "unexpected error: {error}"
        );
    }

    #[test]
    fn missing_application_id_is_rejected_by_the_shared_inspector() {
        let manifest = MANIFEST.replace(" Id=\"ChatGPT\"", "");
        let fixture = PackageFixture::new(&manifest);

        let error = fixture
            .inspect(&manifest)
            .expect_err("the shared inspector must reject an application without an id");
        assert!(
            error.contains("Windows package identity is not the official Codex package"),
            "unexpected error: {error}"
        );
    }

    #[test]
    fn parent_components_in_application_path_are_rejected_by_the_shared_inspector() {
        let manifest = MANIFEST.replace(
            "Executable=\"app/ChatGPT.exe\"",
            "Executable=\"../ChatGPT.exe\"",
        );
        let fixture = PackageFixture::new(&manifest);

        let error = fixture
            .inspect(&manifest)
            .expect_err("the shared inspector must reject package path traversal");
        assert!(
            error.contains("application executable path is unsafe"),
            "unexpected error: {error}"
        );
    }
}
