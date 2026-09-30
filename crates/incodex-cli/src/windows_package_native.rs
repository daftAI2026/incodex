use std::fs;
use std::os::windows::fs::MetadataExt;
use std::path::{Path, PathBuf};

use windows::core::{Interface, HSTRING};
use windows::ApplicationModel::{Package, PackageSignatureKind};
use windows::Data::Xml::Dom::{XmlDocument, XmlElement, XmlNodeList};
use windows::Management::Deployment::PackageManager;
use windows::System::ProcessorArchitecture;
use windows_sys::Win32::Foundation::{ERROR_INSUFFICIENT_BUFFER, ERROR_SUCCESS};
use windows_sys::Win32::Storage::FileSystem::FILE_ATTRIBUTE_REPARSE_POINT;
use windows_sys::Win32::Storage::Packaging::Appx::GetStagedPackagePathByFullName;

use crate::windows_app::{
    inspect_codex_package, validate_codex_package_full_name, WindowsCodexApp,
    WindowsManifestApplication, WindowsPackageEvidence, CODEX_PACKAGE_FAMILY_NAME,
};

const CODEX_PACKAGE_NAME: &str = "OpenAI.Codex";
const APPX_MANIFEST_NAMESPACE: &str =
    "http://schemas.microsoft.com/appx/manifest/foundation/windows10";
const APPX_MANIFEST_FILE: &str = "AppxManifest.xml";
const ERROR_NOT_FOUND: u32 = 0x8007_0490;
const MAX_PACKAGE_PATH_CHARACTERS: u32 = 32_768;

fn parse_codex_manifest_applications(xml: &str) -> Result<Vec<WindowsManifestApplication>, String> {
    let document = winrt(
        "cannot create an AppX manifest XML document",
        XmlDocument::new(),
    )?;
    winrt(
        "cannot load the AppX manifest XML",
        document.LoadXml(&HSTRING::from(xml)),
    )?;

    let package_xpath =
        format!("/*[local-name()='Package' and namespace-uri()='{APPX_MANIFEST_NAMESPACE}']");
    let package_nodes = select_nodes(&document, &package_xpath)?;
    if winrt(
        "cannot count AppX manifest Package elements",
        package_nodes.Length(),
    )? != 1
    {
        return Err("AppX manifest Package namespace or root is invalid".to_string());
    }

    let applications_xpath = format!(
        "{package_xpath}/*[local-name()='Applications' and namespace-uri()='{APPX_MANIFEST_NAMESPACE}']"
    );
    let applications_nodes = select_nodes(&document, &applications_xpath)?;
    if winrt(
        "cannot count AppX manifest Applications elements",
        applications_nodes.Length(),
    )? != 1
    {
        return Err("AppX manifest Applications namespace or element is invalid".to_string());
    }

    let application_xpath = format!(
        "{applications_xpath}/*[local-name()='Application' and namespace-uri()='{APPX_MANIFEST_NAMESPACE}']"
    );
    let application_nodes = select_nodes(&document, &application_xpath)?;
    let count = winrt(
        "cannot count AppX manifest Application elements",
        application_nodes.Length(),
    )?;
    let mut applications = Vec::with_capacity(count as usize);
    for index in 0..count {
        let node = winrt(
            "cannot read an AppX manifest Application element",
            application_nodes.Item(index),
        )?;
        let element = winrt(
            "AppX manifest Application element has an invalid type",
            node.cast::<XmlElement>(),
        )?;
        let application_id = winrt(
            "cannot read AppX manifest Application Id",
            element.GetAttribute(&HSTRING::from("Id")),
        )?
        .to_string();
        let application_executable = winrt(
            "cannot read AppX manifest Application Executable",
            element.GetAttribute(&HSTRING::from("Executable")),
        )?
        .to_string();
        applications.push(WindowsManifestApplication {
            application_id,
            application_executable: PathBuf::from(application_executable),
        });
    }

    Ok(applications)
}

pub(crate) fn validate_staged_codex_package(package: &Package) -> Result<String, String> {
    let evidence = package_evidence_with_location(package, |full_name| {
        prearm_location_with(|kind| package_location(package, full_name, kind))
    })?;
    let full_name = evidence.package_full_name.clone();
    inspect_codex_package(evidence)?;
    Ok(full_name)
}

pub(crate) fn registered_codex_package(full_name: &str) -> Result<WindowsCodexApp, String> {
    validate_codex_package_full_name(full_name)?;
    if !codex_package_full_name_registered(full_name)? {
        return Err(
            "Windows Codex package generation is not registered for the current user".into(),
        );
    }

    let package = lookup_current_user_package(full_name)
        .map_err(|error| format!("cannot query the registered Windows Codex package: {error}"))?;
    let evidence = package_evidence(&package)?;
    if evidence.package_full_name != full_name {
        return Err(
            "registered Windows Codex package generation did not match the requested identity"
                .into(),
        );
    }
    inspect_codex_package(evidence)
}

pub(crate) fn codex_package_full_name_registered(full_name: &str) -> Result<bool, String> {
    validate_codex_package_full_name(full_name)?;
    let manager = winrt(
        "cannot create the Windows PackageManager",
        PackageManager::new(),
    )?;
    let package = match manager
        .FindPackageByUserSecurityIdPackageFullName(&HSTRING::new(), &HSTRING::from(full_name))
    {
        Ok(package) => package,
        Err(error) if error.code().0 as u32 == ERROR_NOT_FOUND => return Ok(false),
        Err(error) => {
            return Err(format!(
                "cannot query the current user's Windows Codex package registration: {error}"
            ));
        }
    };

    let registered_full_name = winrt(
        "cannot read the registered Windows package identity",
        package.Id(),
    )?;
    let registered_full_name = winrt(
        "cannot read the registered Windows package full name",
        registered_full_name.FullName(),
    )?
    .to_string();
    if registered_full_name != full_name {
        return Err("Windows PackageManager returned a different package generation".into());
    }
    Ok(true)
}

fn lookup_current_user_package(full_name: &str) -> windows::core::Result<Package> {
    PackageManager::new()?
        .FindPackageByUserSecurityIdPackageFullName(&HSTRING::new(), &HSTRING::from(full_name))
}

fn package_evidence(package: &Package) -> Result<WindowsPackageEvidence, String> {
    package_evidence_with_location(package, |full_name| {
        package_location(package, full_name, PackageLocationKind::Registered)
    })
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum PackageLocationKind {
    Registered,
    Staged,
}

fn prearm_location_with(
    query: impl FnOnce(PackageLocationKind) -> Result<PathBuf, String>,
) -> Result<PathBuf, String> {
    // 更新事件的目标可健康且已暂存，但尚未向本用户注册；InstalledLocation 此时不可用。
    query(PackageLocationKind::Staged)
}

fn package_location(
    package: &Package,
    full_name: &str,
    kind: PackageLocationKind,
) -> Result<PathBuf, String> {
    match kind {
        PackageLocationKind::Registered => Ok(PathBuf::from(
            winrt(
                "cannot read Windows Codex package install location",
                winrt(
                    "cannot query Windows Codex package install location",
                    package.InstalledLocation(),
                )?
                .Path(),
            )?
            .to_string(),
        )),
        PackageLocationKind::Staged => {
            staged_package_path_with(full_name, |name, length, buffer| {
                let destination = buffer.map_or(std::ptr::null_mut(), |buffer| buffer.as_mut_ptr());
                unsafe { GetStagedPackagePathByFullName(name.as_ptr(), length, destination) }
            })
        }
    }
}

fn staged_package_path_with(
    full_name: &str,
    mut query: impl FnMut(&[u16], &mut u32, Option<&mut [u16]>) -> u32,
) -> Result<PathBuf, String> {
    validate_codex_package_full_name(full_name)?;
    let name: Vec<u16> = full_name.encode_utf16().chain(Some(0)).collect();
    let mut length = 0;
    let result = query(&name, &mut length, None);
    if result != ERROR_INSUFFICIENT_BUFFER || !(2..=MAX_PACKAGE_PATH_CHARACTERS).contains(&length) {
        return Err(format!(
            "cannot size Windows staged Codex package path: code={result}, length={length}"
        ));
    }
    let mut buffer = vec![0u16; length as usize];
    let result = query(&name, &mut length, Some(&mut buffer));
    if result != ERROR_SUCCESS || length < 2 || length as usize > buffer.len() {
        return Err(format!(
            "cannot read Windows staged Codex package path: code={result}, length={length}"
        ));
    }
    let path = &buffer[..length as usize];
    if path.last() != Some(&0) || path[..path.len() - 1].contains(&0) {
        return Err("Windows staged Codex package path has invalid termination".into());
    }
    let path = PathBuf::from(
        String::from_utf16(&path[..path.len() - 1])
            .map_err(|_| "Windows staged Codex package path is not valid UTF-16")?,
    );
    incodex_core::windows_path::require_local_disk_absolute(
        &path,
        "Windows staged Codex package path",
    )?;
    Ok(path)
}

fn package_evidence_with_location(
    package: &Package,
    location: impl FnOnce(&str) -> Result<PathBuf, String>,
) -> Result<WindowsPackageEvidence, String> {
    let id = winrt("cannot read Windows Codex package identity", package.Id())?;
    let name = winrt("cannot read Windows Codex package name", id.Name())?.to_string();
    let package_full_name =
        winrt("cannot read Windows Codex package full name", id.FullName())?.to_string();
    let package_family_name =
        winrt("cannot read Windows Codex package family", id.FamilyName())?.to_string();
    if name != CODEX_PACKAGE_NAME || package_family_name != CODEX_PACKAGE_FAMILY_NAME {
        return Err("Windows package identity is not the official Codex package".to_string());
    }
    validate_codex_package_full_name(&package_full_name)?;

    let signature_kind = winrt(
        "cannot read Windows Codex package signature kind",
        package.SignatureKind(),
    )?;
    if signature_kind != PackageSignatureKind::Store {
        return Err("official Codex package does not have a Store signature".to_string());
    }

    let status = winrt("cannot read Windows Codex package status", package.Status())?;
    let status_is_ok = winrt(
        "cannot verify Windows Codex package status",
        status.VerifyIsOK(),
    )?;
    let disabled = winrt(
        "cannot read Windows Codex package disabled status",
        status.Disabled(),
    )?;
    let servicing = winrt(
        "cannot read Windows Codex package servicing status",
        status.Servicing(),
    )?;
    if !status_is_ok || disabled || servicing {
        return Err("official Codex Microsoft Store package is not healthy".to_string());
    }

    let architecture = architecture_name(winrt(
        "cannot read Windows Codex package architecture",
        id.Architecture(),
    )?)?
    .to_string();
    let install_location = location(&package_full_name)?;
    incodex_core::windows_path::require_local_disk_absolute(
        &install_location,
        "Windows Codex package path",
    )?;

    let manifest_path = install_location.join(APPX_MANIFEST_FILE);
    require_normal_file(&manifest_path, "AppX manifest")?;
    let manifest = fs::read_to_string(&manifest_path).map_err(|error| {
        format!(
            "cannot read Windows Codex AppX manifest {}: {error}",
            manifest_path.display()
        )
    })?;
    let applications =
        parse_codex_manifest_applications(manifest.strip_prefix('\u{feff}').unwrap_or(&manifest))?;

    Ok(WindowsPackageEvidence {
        name,
        package_full_name,
        package_family_name,
        applications,
        install_location,
        architecture,
        signature_kind: "Store".to_string(),
        status: "Ok".to_string(),
    })
}

fn architecture_name(architecture: ProcessorArchitecture) -> Result<&'static str, String> {
    match architecture {
        ProcessorArchitecture::X86 => Ok("X86"),
        ProcessorArchitecture::Arm => Ok("Arm"),
        ProcessorArchitecture::X64 => Ok("X64"),
        ProcessorArchitecture::Neutral => Ok("Neutral"),
        ProcessorArchitecture::Arm64 => Ok("Arm64"),
        ProcessorArchitecture::X86OnArm64 => Ok("X86OnArm64"),
        ProcessorArchitecture::Unknown => Ok("Unknown"),
        _ => Err("Windows Codex package architecture is unknown".to_string()),
    }
}

fn select_nodes(document: &XmlDocument, xpath: &str) -> Result<XmlNodeList, String> {
    winrt(
        "cannot select AppX manifest XML elements",
        document.SelectNodes(&HSTRING::from(xpath)),
    )
}

fn require_normal_file(path: &Path, label: &str) -> Result<(), String> {
    let metadata = fs::symlink_metadata(path)
        .map_err(|error| format!("cannot inspect {label} {}: {error}", path.display()))?;
    if !metadata.file_type().is_file()
        || metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0
    {
        return Err(format!("{label} is not a normal file: {}", path.display()));
    }
    Ok(())
}

fn winrt<T>(context: &str, result: windows::core::Result<T>) -> Result<T, String> {
    result.map_err(|error| format!("{context}: {error}"))
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU64, Ordering};

    use crate::windows_app::{inspect_codex_package, WindowsCodexApp, WindowsPackageEvidence};

    use super::{
        lookup_current_user_package, package_evidence, parse_codex_manifest_applications,
        prearm_location_with, registered_codex_package, staged_package_path_with,
        validate_staged_codex_package, PackageLocationKind,
    };

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
    fn healthy_staged_target_does_not_require_registered_install_location() {
        let fixture = PackageFixture::new(MANIFEST);
        let mut queried = None;
        let path = prearm_location_with(|kind| {
            queried = Some(kind);
            match kind {
                PackageLocationKind::Registered => {
                    Err("InstalledLocation: invalid parameter (0x80070057)".into())
                }
                PackageLocationKind::Staged => Ok(fixture.root.clone()),
            }
        })
        .expect("healthy staged-only target must be available before registration");
        assert_eq!(queried, Some(PackageLocationKind::Staged));
        assert_eq!(path, fixture.root);
    }

    #[test]
    fn staged_path_uses_exact_identity_and_native_two_call_protocol() {
        let fixture = PackageFixture::new(MANIFEST);
        let wide: Vec<u16> = fixture
            .root
            .to_str()
            .unwrap()
            .encode_utf16()
            .chain(Some(0))
            .collect();
        let mut calls = 0;
        let path = staged_package_path_with(PACKAGE_FULL_NAME, |name, length, buffer| {
            assert_eq!(
                String::from_utf16(&name[..name.len() - 1]).unwrap(),
                PACKAGE_FULL_NAME
            );
            calls += 1;
            match buffer {
                None => {
                    *length = wide.len() as u32;
                    windows_sys::Win32::Foundation::ERROR_INSUFFICIENT_BUFFER
                }
                Some(buffer) => {
                    assert_eq!(*length as usize, wide.len());
                    buffer.copy_from_slice(&wide);
                    windows_sys::Win32::Foundation::ERROR_SUCCESS
                }
            }
        })
        .expect("read exact staged path");
        assert_eq!(calls, 2);
        assert_eq!(path, fixture.root);
    }

    #[test]
    fn staged_path_rejects_missing_package_and_invalid_native_lengths() {
        for (code, count) in [(1168, 0), (122, 0), (122, 1), (122, 32_769)] {
            let mut calls = 0;
            let result = staged_package_path_with(PACKAGE_FULL_NAME, |_, length, _| {
                calls += 1;
                *length = count;
                code
            });
            assert!(result.is_err(), "unexpected sizing result {code}/{count}");
            assert_eq!(calls, 1);
        }
    }

    #[test]
    fn staged_path_rejects_failed_or_unterminated_native_reply() {
        for failed in [true, false] {
            let mut calls = 0;
            let result = staged_package_path_with(PACKAGE_FULL_NAME, |_, length, buffer| {
                calls += 1;
                if let Some(buffer) = buffer {
                    buffer.fill(b'x' as u16);
                    if failed {
                        5
                    } else {
                        0
                    }
                } else {
                    *length = 4;
                    122
                }
            });
            assert!(result.is_err());
            assert_eq!(calls, 2);
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

    #[test]
    #[ignore = "requires an installed, healthy OpenAI.Codex Microsoft Store package"]
    fn reads_current_store_package_through_native_adapter() {
        let _apartment = crate::windows_update_repair::WindowsRuntimeApartment::initialize()
            .expect("initialize WinRT apartment");
        let expected = crate::windows_app::discover_codex_package()
            .expect("discover the current official Codex package");

        let package = lookup_current_user_package(&expected.package_full_name)
            .expect("find that exact package for the current user");
        let staged_full_name = validate_staged_codex_package(&package)
            .expect("validate the exact package through the staged package path");
        assert_eq!(staged_full_name, expected.package_full_name);

        let staged = inspect_codex_package(
            package_evidence(&package).expect("read native package evidence"),
        )
        .expect("inspect native package evidence");
        assert_eq!(
            staged, expected,
            "staged native metadata must match discovery"
        );

        let registered = registered_codex_package(&expected.package_full_name)
            .expect("resolve the exact current-user package through PackageManager");
        assert_eq!(
            registered, expected,
            "registered native metadata must match discovery"
        );
    }
}
