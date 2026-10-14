#!/bin/sh
# [INPUT]: 依赖 xcrun clang、macOS Foundation 与本目录的 Sparkle 假对象夹具。
# [OUTPUT]: 编译并执行真实 interposer，断言闭集诊断事件及原回调语义。
# [POS]: tests/native 的离线烟测；只启动合成进程，不调用 Sparkle 更新或真实宿主。
# [PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md
set -eu
umask 077

script_dir=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
repo_root=$(CDPATH= cd -- "$script_dir/../.." && pwd)
interposer="$repo_root/crates/incodex-cli/native/macos_sparkle_interpose.m"
fixture="$script_dir/macos-sparkle-interpose-trace-smoke.m"
clang=$(xcrun --find clang)
sdk=$(xcrun --sdk macosx --show-sdk-path)
evidence_root=
temporary_root=
run_dir=
evidence_ready=0

cleanup() {
    if [ -n "$temporary_root" ]; then
        rm -rf "$temporary_root"
    fi
    if [ -n "$evidence_root" ] && [ "$evidence_ready" -eq 0 ] && [ -n "$run_dir" ]; then
        rmdir "$run_dir" 2>/dev/null || true
    fi
    if [ -n "$evidence_root" ] && [ "$evidence_ready" -eq 1 ]; then
        printf 'Evidence retained in private run directory.\n'
    fi
}
trap cleanup EXIT

temporary_root=$(mktemp -d "${TMPDIR:-/tmp}/incodex-sparkle-trace.XXXXXX")

if [ -n "${INCODEX_NATIVE_TRACE_EVIDENCE_DIR:-}" ]; then
    evidence_root=$INCODEX_NATIVE_TRACE_EVIDENCE_DIR
    mkdir -p "$evidence_root"
    run_dir=$(mktemp -d "$evidence_root/run.XXXXXX")
    chmod 700 "$run_dir"
    evidence_ready=1
else
    evidence_root=
    run_dir=$temporary_root
fi

bundle_root="$temporary_root/bundles"
mkdir -p "$bundle_root"

make_bundle() {
    bundle_name=$1
    bundle_id=$2
    executable_name=$3
    bundle_path="$bundle_root/$bundle_name.app"
    mkdir -p "$bundle_path/Contents/MacOS"
    cat > "$bundle_path/Contents/Info.plist" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>CFBundleIdentifier</key><string>$bundle_id</string>
    <key>CFBundleExecutable</key><string>$executable_name</string>
    <key>CFBundleName</key><string>$bundle_name</string>
    <key>CFBundlePackageType</key><string>APPL</string>
</dict>
</plist>
PLIST
    : > "$bundle_path/Contents/MacOS/$executable_name"
    printf '%s' "$bundle_path"
}

host_app=$(make_bundle host com.openai.codex FixtureHost)
coordinator_app=$(make_bundle coordinator com.daftai.incodex.fixture FixtureCoordinator)
wrong_host_app=$(make_bundle wrong-host org.example.not-codex FixtureWrongHost)
wrong_application_app=$(make_bundle wrong-application org.example.not-coordinator FixtureWrongApplication)

printf '%s\n' \
    "clang -isysroot $sdk -fobjc-arc -fblocks -dynamiclib -DINCODEX_DIAGNOSTIC_TRACE_TEST_STDERR=1 native/macos_sparkle_interpose.m -framework Foundation" \
    "clang -isysroot $sdk -fobjc-arc -fblocks tests/native/macos-sparkle-interpose-trace-smoke.m -framework Foundation" \
    > "$run_dir/compile-command.txt"
if ! "$clang" -isysroot "$sdk" -fobjc-arc -fblocks -DINCODEX_DIAGNOSTIC_TRACE_TEST_STDERR=1 \
    -dynamiclib "$interposer" -framework Foundation -o "$temporary_root/libincodex-sparkle-interpose.dylib" \
    > "$run_dir/interposer-build.stdout" 2> "$run_dir/interposer-build.stderr"; then
    printf 'RED: native fixture failed to compile; this is not the expected behavioral RED.\n'
    exit 2
fi
if ! "$clang" -isysroot "$sdk" -fobjc-arc -fblocks "$fixture" -framework Foundation \
    -o "$temporary_root/smoke" \
    > "$run_dir/fixture-build.stdout" 2> "$run_dir/fixture-build.stderr"; then
    printf 'RED: native fixture failed to compile; this is not the expected behavioral RED.\n'
    exit 2
fi

failures=0

run_case() {
    case_name=$1
    trace_mode=$2
    output_dir="$run_dir/$case_name"
    mkdir -p "$output_dir"

    if [ "$trace_mode" = on ]; then
        if INCODEX_DIAGNOSTIC_UPDATE_TRACE=1 \
            INCODEX_MACOS_UPDATE_HOST_APP="$host_app" \
            INCODEX_MACOS_UPDATE_COORDINATOR_APP="$coordinator_app" \
            INCODEX_TEST_HOST_APP="$host_app" \
            INCODEX_TEST_COORDINATOR_APP="$coordinator_app" \
            INCODEX_TEST_WRONG_HOST_APP="$wrong_host_app" \
            INCODEX_TEST_WRONG_APPLICATION_APP="$wrong_application_app" \
            INCODEX_TEST_INTERPOSER_DYLIB="$temporary_root/libincodex-sparkle-interpose.dylib" \
                "$temporary_root/smoke" "$case_name" \
                > "$output_dir/stdout" 2> "$output_dir/stderr"; then
            case_status=0
        else
            case_status=$?
        fi
    else
        if (
            unset INCODEX_DIAGNOSTIC_UPDATE_TRACE
            INCODEX_MACOS_UPDATE_HOST_APP="$host_app" \
            INCODEX_MACOS_UPDATE_COORDINATOR_APP="$coordinator_app" \
            INCODEX_TEST_HOST_APP="$host_app" \
            INCODEX_TEST_COORDINATOR_APP="$coordinator_app" \
            INCODEX_TEST_WRONG_HOST_APP="$wrong_host_app" \
            INCODEX_TEST_WRONG_APPLICATION_APP="$wrong_application_app" \
            INCODEX_TEST_INTERPOSER_DYLIB="$temporary_root/libincodex-sparkle-interpose.dylib" \
                "$temporary_root/smoke" "$case_name" \
                > "$output_dir/stdout" 2> "$output_dir/stderr"
        ); then
            case_status=0
        else
            case_status=$?
        fi
    fi
    printf '%s\n' "$case_status" > "$output_dir/exit-code"
    if [ "$case_status" -ne 0 ]; then
        printf 'RED: fixture semantics failed for %s (exit=%s).\n' "$case_name" "$case_status"
        failures=$((failures + 1))
    fi
    if [ "$(cat "$output_dir/stdout")" != "FIXTURE_PASS: $case_name" ]; then
        printf 'RED: fixture did not report semantic PASS for %s.\n' "$case_name"
        failures=$((failures + 1))
    fi
}

assert_events() {
    case_name=$1
    shift
    output_file="$run_dir/$case_name/stderr"
    expected_events=
    if [ "$#" -gt 0 ]; then
        expected_events=$(printf '%s\n' "$@")
    fi
    valid_lines=$(sed -nE \
        's/^\[incodex-update-diagnostic\] code=([a-z-]+) pid=([0-9]+) subject_pid=([0-9]+) value=(-?[0-9]+)$/\1:\4/p' \
        "$output_file")
    valid_count=$(printf '%s\n' "$valid_lines" | awk 'NF { count += 1 } END { print count + 0 }')
    total_count=$(wc -l < "$output_file" | tr -d ' ')
    if [ "$valid_count" -ne "$total_count" ]; then
        printf 'RED: non-closed diagnostic output for %s.\n' "$case_name"
        failures=$((failures + 1))
    fi
    if [ "$valid_lines" != "$expected_events" ]; then
        printf 'RED: diagnostic event mismatch for %s.\n' "$case_name"
        printf '  expected: %s\n' "${expected_events:-<no events>}"
        printf '  actual:   %s\n' "${valid_lines:-<no events>}"
        failures=$((failures + 1))
    fi
}

run_case default  off
run_case matched-no on
run_case matched-yes on
run_case matched-exception on
run_case matched-no-delegate on
run_case nonmatch-many on
run_case matched-then-nonmatch on

assert_events default
assert_events matched-no \
    interposer-installed:1 updater-matched:1 application-replaced:1 delegate-observer-ready:1 \
    will-install-entered:1 will-install-returned:0
assert_events matched-yes \
    interposer-installed:1 updater-matched:1 application-replaced:1 delegate-observer-ready:1 \
    will-install-entered:1 will-install-returned:1
assert_events matched-exception \
    interposer-installed:1 updater-matched:1 application-replaced:1 delegate-observer-ready:1 \
    will-install-entered:1 will-install-threw:-1
assert_events matched-no-delegate \
    interposer-installed:1 updater-matched:1 application-replaced:1 delegate-observer-ready:0
assert_events nonmatch-many interposer-installed:1
assert_events matched-then-nonmatch \
    interposer-installed:1 updater-matched:1 application-replaced:1 delegate-observer-ready:1 \
    will-install-entered:1 will-install-returned:1

if [ "$failures" -gt 0 ]; then
    printf 'RED: %s assertion group(s) failed after native compile and execution.\n' "$failures"
    exit 1
fi

printf 'PASS: native Sparkle interposer diagnostics preserve the original callback contract.\n'
