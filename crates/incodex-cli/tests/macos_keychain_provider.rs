#![cfg(target_os = "macos")]

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

static SEQUENCE: AtomicU64 = AtomicU64::new(0);

fn scratch() -> PathBuf {
    let sequence = SEQUENCE.fetch_add(1, Ordering::Relaxed);
    let path = std::env::temp_dir().join(format!(
        "incodex-macos-keychain-provider-{}-{sequence}",
        std::process::id()
    ));
    let _ = fs::remove_dir_all(&path);
    fs::create_dir_all(&path).unwrap();
    path
}

#[test]
fn provider_reads_only_bounded_successful_helper_output_and_times_out() {
    let home = scratch();
    let probe = compile_probe(&home);
    let success = write_executable(
        &home,
        "success",
        "#!/bin/sh\nprintf 'synthetic-fixed-key'\n",
    );
    let failure = write_executable(&home, "failure", "#!/bin/sh\nexit 7\n");
    let oversized = write_executable(
        &home,
        "oversized",
        "#!/bin/sh\n/usr/bin/yes x | /usr/bin/head -c 4097\n",
    );
    let blocking = write_executable(&home, "blocking", "#!/bin/sh\n/bin/sleep 10\n");

    let output = Command::new(&probe).arg(&success).output().unwrap();
    assert!(output.status.success());
    assert_eq!(output.stdout, b"synthetic-fixed-key");

    for helper in [&failure, &oversized] {
        let output = Command::new(&probe).arg(helper).output().unwrap();
        assert!(!output.status.success());
        assert!(output.stdout.is_empty());
    }

    let started = Instant::now();
    let output = Command::new(&probe).arg(&blocking).output().unwrap();
    let elapsed = started.elapsed();
    assert!(!output.status.success());
    assert!(output.stdout.is_empty());
    assert!(
        elapsed >= Duration::from_secs(2) && elapsed < Duration::from_secs(4),
        "helper timeout took {elapsed:?}"
    );

    fs::remove_dir_all(home).unwrap();
}

#[test]
fn provider_intercepts_only_the_exact_codex_storage_data_query() {
    let home = scratch();
    let provider = compile_test_provider(&home);
    let probe = compile_query_probe(&home, &provider);
    let success = write_executable(&home, "query-success", "#!/bin/sh\nprintf 'fixed-key'\n");
    let failure = write_executable(&home, "query-failure", "#!/bin/sh\nexit 7\n");

    let output = Command::new(probe)
        .args([success.as_os_str(), failure.as_os_str()])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    fs::remove_dir_all(home).unwrap();
}

fn compile_probe(home: &Path) -> PathBuf {
    let provider = compile_test_provider(home);
    let clang = command_stdout("xcrun", &["--find", "clang"]);
    let sdk = command_stdout("xcrun", &["--sdk", "macosx", "--show-sdk-path"]);

    let source = home.join("provider_probe.c");
    fs::write(
        &source,
        r#"
#include <stddef.h>
#include <stdio.h>

extern int incodex_key_provider_test_run_helper(
    const char *path, unsigned char *output, size_t capacity, size_t *length);

int main(int argc, char **argv) {
    if (argc != 2) return 64;
    unsigned char output[4096] = {0};
    size_t length = 0;
    int status = incodex_key_provider_test_run_helper(
        argv[1], output, sizeof(output), &length);
    if (status != 0) return status;
    if (length > 0 && fwrite(output, 1, length, stdout) != length) return 74;
    return 0;
}
"#,
    )
    .unwrap();
    let probe = home.join("provider-probe");
    let status = Command::new(&clang)
        .args(["-isysroot", &sdk, "-Wall", "-Wextra", "-Werror"])
        .arg(&source)
        .arg(&provider)
        .args(["-Wl,-rpath,@executable_path", "-o"])
        .arg(&probe)
        .status()
        .unwrap();
    assert!(status.success(), "provider probe did not compile");
    probe
}

fn compile_test_provider(home: &Path) -> PathBuf {
    let clang = command_stdout("xcrun", &["--find", "clang"]);
    let sdk = command_stdout("xcrun", &["--sdk", "macosx", "--show-sdk-path"]);
    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let provider_source = manifest.join("native/macos_keychain_provider.c");
    let provider = home.join("IncodexKeyProviderTest.dylib");
    let status = Command::new(&clang)
        .args(["-isysroot", &sdk])
        .args(["-Wall", "-Wextra", "-Werror", "-DINCODEX_TESTING"])
        .args(["-dynamiclib", "-framework", "CoreFoundation"])
        .args(["-framework", "Security"])
        .arg(&provider_source)
        .arg("-o")
        .arg(&provider)
        .status()
        .unwrap();
    assert!(status.success(), "test provider did not compile");
    provider
}

fn compile_query_probe(home: &Path, provider: &Path) -> PathBuf {
    let clang = command_stdout("xcrun", &["--find", "clang"]);
    let sdk = command_stdout("xcrun", &["--sdk", "macosx", "--show-sdk-path"]);
    let source = home.join("query_probe.c");
    fs::write(
        &source,
        r#"
#include <CoreFoundation/CoreFoundation.h>
#include <Security/Security.h>
#include <stdio.h>
#include <string.h>

typedef OSStatus (*CopyMatching)(CFDictionaryRef, CFTypeRef *);
extern OSStatus incodex_key_provider_test_copy_matching(
    CFDictionaryRef query, CFTypeRef *result, const char *helper,
    CopyMatching original);

static int original_calls = 0;
static OSStatus fake_original(CFDictionaryRef query, CFTypeRef *result) {
    (void)query;
    (void)result;
    original_calls += 1;
    return -7777;
}

static CFMutableDictionaryRef query(void) {
    CFMutableDictionaryRef value = CFDictionaryCreateMutable(
        kCFAllocatorDefault, 6, &kCFTypeDictionaryKeyCallBacks,
        &kCFTypeDictionaryValueCallBacks);
    if (!value) return NULL;
    CFDictionarySetValue(value, kSecClass, kSecClassGenericPassword);
    CFDictionarySetValue(value, kSecAttrService, CFSTR("Codex Storage Key"));
    CFDictionarySetValue(value, kSecAttrAccount, CFSTR("Codex"));
    CFDictionarySetValue(value, kSecReturnData, kCFBooleanTrue);
    CFDictionarySetValue(value, kSecMatchLimit, kSecMatchLimitOne);
    return value;
}

static int expect_passthrough(CFMutableDictionaryRef value,
                              const char *helper) {
    int before = original_calls;
    CFTypeRef result = NULL;
    OSStatus status = incodex_key_provider_test_copy_matching(
        value, &result, helper, fake_original);
    if (result) CFRelease(result);
    return status == -7777 && original_calls == before + 1 ? 0 : 1;
}

int main(int argc, char **argv) {
    if (argc != 3) return 64;

    CFMutableDictionaryRef exact = query();
    CFTypeRef result = NULL;
    OSStatus status = incodex_key_provider_test_copy_matching(
        exact, &result, argv[1], fake_original);
    if (status != errSecSuccess || !result ||
        CFGetTypeID(result) != CFDataGetTypeID() || original_calls != 0) {
        return 1;
    }
    CFDataRef data = (CFDataRef)result;
    if (CFDataGetLength(data) != 9 ||
        memcmp(CFDataGetBytePtr(data), "fixed-key", 9) != 0) return 2;
    CFRelease(result);
    CFRelease(exact);

    CFMutableDictionaryRef wrong = query();
    CFDictionarySetValue(wrong, kSecAttrAccount, CFSTR("Other"));
    if (expect_passthrough(wrong, argv[1])) return 3;
    CFRelease(wrong);

    wrong = query();
    CFDictionarySetValue(wrong, kSecAttrService, CFSTR("Other"));
    if (expect_passthrough(wrong, argv[1])) return 4;
    CFRelease(wrong);

    wrong = query();
    CFDictionarySetValue(wrong, kSecClass, kSecClassInternetPassword);
    if (expect_passthrough(wrong, argv[1])) return 5;
    CFRelease(wrong);

    wrong = query();
    CFDictionarySetValue(wrong, kSecReturnData, kCFBooleanFalse);
    if (expect_passthrough(wrong, argv[1])) return 6;
    CFRelease(wrong);

    wrong = query();
    CFDictionarySetValue(wrong, kSecMatchLimit, kSecMatchLimitAll);
    if (expect_passthrough(wrong, argv[1])) return 7;
    CFRelease(wrong);

    wrong = query();
    CFDictionarySetValue(wrong, kSecReturnAttributes, kCFBooleanTrue);
    if (expect_passthrough(wrong, argv[1])) return 8;
    CFRelease(wrong);

    exact = query();
    result = NULL;
    int before = original_calls;
    status = incodex_key_provider_test_copy_matching(
        exact, &result, argv[2], fake_original);
    CFRelease(exact);
    if (result) CFRelease(result);
    if (status == errSecSuccess || original_calls != before) return 9;
    return 0;
}
"#,
    )
    .unwrap();
    let probe = home.join("query-probe");
    let status = Command::new(&clang)
        .args(["-isysroot", &sdk, "-Wall", "-Wextra", "-Werror"])
        .args(["-framework", "CoreFoundation", "-framework", "Security"])
        .arg(&source)
        .arg(provider)
        .args(["-Wl,-rpath,@executable_path", "-o"])
        .arg(&probe)
        .status()
        .unwrap();
    assert!(status.success(), "query probe did not compile");
    probe
}

fn command_stdout(command: &str, arguments: &[&str]) -> String {
    let output = Command::new(command).args(arguments).output().unwrap();
    assert!(output.status.success());
    String::from_utf8(output.stdout).unwrap().trim().to_owned()
}

fn write_executable(home: &Path, name: &str, body: &str) -> PathBuf {
    let path = home.join(name);
    fs::write(&path, body).unwrap();
    fs::set_permissions(&path, fs::Permissions::from_mode(0o700)).unwrap();
    path
}
