#!/bin/bash
# [INPUT]: 依赖已安装的 incodex/inc 二进制、macOS zip/unzip/open/osascript 与用户显式的 Raycast/Alfred 导入操作
# [OUTPUT]: 生成 Incodex 独占的 Raycast Script Commands 目录与稳定 Bundle ID 的 Alfred .alfredworkflow 导入包
# [POS]: scripts 的可选 Quick Launchers 安装器，不修改 Raycast/Alfred 私有配置，不依赖 CLI 安装器或官方 App 补丁路径
# [PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md

set -euo pipefail

OWNER_MARKER="incodex-quick-launchers owner=daftAI2026/incodex schema=1"
ROOT="${INCODEX_QUICK_LAUNCHERS_ROOT:-$HOME/.incodex/quick-launchers}"
MARKER="$ROOT/.incodex-quick-launchers"
RAYCAST_DIR="$ROOT/raycast"
ALFRED_DIR="$ROOT/alfred"
ALFRED_WORKFLOW="$ALFRED_DIR/Incodex Quick Launchers.alfredworkflow"

die() {
    printf 'Error: %s\n' "$1" >&2
    exit 1
}

resolve_incodex() {
    local candidate
    for candidate in incodex inc; do
        if command -v "$candidate" >/dev/null 2>&1; then
            command -v "$candidate"
            return 0
        fi
    done
    die "incodex was not found in PATH; install the CLI first"
}

shell_quote() {
    printf '%q' "$1"
}

require_owned_root() {
    if [[ ! -f "$MARKER" ]]; then
        die "ownership marker is missing; refusing to remove launcher files"
    fi
    if [[ "$(cat "$MARKER")" != "$OWNER_MARKER" ]]; then
        die "ownership marker changed; refusing to remove launcher files"
    fi
}

assert_owned_or_absent() {
    local path="$1"
    if [[ -e "$path" ]] && ! grep -Fq "$OWNER_MARKER" "$path"; then
        die "refusing to overwrite a launcher not owned by Incodex: $path"
    fi
}

assert_alfred_owned_or_absent() {
    if [[ ! -e "$ALFRED_WORKFLOW" ]]; then
        return
    fi
    if ! /usr/bin/unzip -p "$ALFRED_WORKFLOW" run.sh 2>/dev/null | grep -Fq "$OWNER_MARKER"; then
        die "refusing to overwrite an Alfred package not owned by Incodex: $ALFRED_WORKFLOW"
    fi
}

preflight_install_ownership() {
    assert_owned_or_absent "$RAYCAST_DIR/incodex-open.sh"
    assert_owned_or_absent "$RAYCAST_DIR/incodex-status.sh"
    assert_owned_or_absent "$RAYCAST_DIR/incodex-doctor.sh"
    assert_alfred_owned_or_absent
}

write_raycast_script() {
    local target="$1"
    local title="$2"
    local mode="$3"
    local description="$4"
    local command="$5"
    local quoted_binary="$6"
    local temporary="$target.tmp.$$"

    assert_owned_or_absent "$target"
    cat >"$temporary" <<EOF
#!/bin/bash
# $OWNER_MARKER

# Required parameters:
# @raycast.schemaVersion 1
# @raycast.title $title
# @raycast.mode $mode

# Optional parameters:
# @raycast.packageName Incodex
# @raycast.description $description
# @raycast.icon 🕶️
# @raycast.platform macos

set -euo pipefail
INCODEX_BIN=$quoted_binary
EOF

    if [[ "$command" == "open" ]]; then
        cat >>"$temporary" <<'EOF'
nohup "$INCODEX_BIN" open </dev/null >/dev/null 2>&1 &
printf '%s\n' 'Opening an incognito Codex window'
EOF
    else
        printf 'exec "$INCODEX_BIN" %s\n' "$command" >>"$temporary"
    fi
    chmod 0755 "$temporary"
    mv -f "$temporary" "$target"
}

write_raycast_launchers() {
    local quoted_binary="$1"
    mkdir -p "$RAYCAST_DIR"
    write_raycast_script \
        "$RAYCAST_DIR/incodex-open.sh" \
        "Incodex Open" \
        "silent" \
        "Open an incognito Codex window" \
        "open" \
        "$quoted_binary"
    write_raycast_script \
        "$RAYCAST_DIR/incodex-status.sh" \
        "Incodex Status" \
        "fullOutput" \
        "Show whether Incodex is installed" \
        "status" \
        "$quoted_binary"
    write_raycast_script \
        "$RAYCAST_DIR/incodex-doctor.sh" \
        "Incodex Doctor" \
        "fullOutput" \
        "Diagnose the Incodex installation" \
        "doctor" \
        "$quoted_binary"
}

write_alfred_runner() {
    local target="$1"
    local quoted_binary="$2"
    cat >"$target" <<EOF
#!/bin/bash
# $OWNER_MARKER
set -euo pipefail
INCODEX_BIN=$quoted_binary

case "\${1:-}" in
    open)
        nohup "\$INCODEX_BIN" open </dev/null >/dev/null 2>&1 &
        ;;
    status|doctor)
        /usr/bin/osascript - "\$INCODEX_BIN" "\$1" <<'APPLESCRIPT'
on run argv
    set binaryPath to item 1 of argv
    set subcommandName to item 2 of argv
    set targetCommand to quoted form of binaryPath & " " & quoted form of subcommandName
    tell application "Terminal"
        activate
        do script targetCommand
    end tell
end run
APPLESCRIPT
        ;;
    *)
        printf 'Unknown Incodex launcher: %s\\n' "\${1:-}" >&2
        exit 64
        ;;
esac
EOF
    chmod 0755 "$target"
}

write_alfred_plist() {
    local target="$1"
    cat >"$target" <<'EOF'
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>bundleid</key><string>com.daftai.incodex.quick-launchers</string>
  <key>createdby</key><string>Incodex</string>
  <key>description</key><string>Open and inspect Incodex from Alfred.</string>
  <key>name</key><string>Incodex Quick Launchers</string>
  <key>readme</key><string>Keywords: incognito, inc-status, inc-doctor. Incodex must already be installed.</string>
  <key>uid</key><string>user.workflow.com.daftai.incodex.quick-launchers</string>
  <key>version</key><integer>1</integer>
  <key>objects</key>
  <array>
    <dict>
      <key>config</key><dict>
        <key>argumenttype</key><integer>2</integer>
        <key>keyword</key><string>incognito</string>
        <key>subtext</key><string>Open an incognito Codex window</string>
        <key>text</key><string>Incodex Open</string>
        <key>withspace</key><false/>
      </dict>
      <key>type</key><string>alfred.workflow.input.keyword</string>
      <key>uid</key><string>incodex.quick.open.input</string>
      <key>version</key><integer>1</integer>
    </dict>
    <dict>
      <key>config</key><dict>
        <key>argumenttype</key><integer>2</integer>
        <key>keyword</key><string>inc-status</string>
        <key>subtext</key><string>Show whether Incodex is installed</string>
        <key>text</key><string>Incodex Status</string>
        <key>withspace</key><false/>
      </dict>
      <key>type</key><string>alfred.workflow.input.keyword</string>
      <key>uid</key><string>incodex.quick.status.input</string>
      <key>version</key><integer>1</integer>
    </dict>
    <dict>
      <key>config</key><dict>
        <key>argumenttype</key><integer>2</integer>
        <key>keyword</key><string>inc-doctor</string>
        <key>subtext</key><string>Diagnose the Incodex installation</string>
        <key>text</key><string>Incodex Doctor</string>
        <key>withspace</key><false/>
      </dict>
      <key>type</key><string>alfred.workflow.input.keyword</string>
      <key>uid</key><string>incodex.quick.doctor.input</string>
      <key>version</key><integer>1</integer>
    </dict>
    <dict>
      <key>config</key><dict>
        <key>concurrently</key><true/>
        <key>escaping</key><integer>102</integer>
        <key>script</key><string>./run.sh open</string>
        <key>scriptargtype</key><integer>1</integer>
        <key>scriptfile</key><string></string>
        <key>type</key><integer>0</integer>
      </dict>
      <key>type</key><string>alfred.workflow.action.script</string>
      <key>uid</key><string>incodex.quick.open.action</string>
      <key>version</key><integer>2</integer>
    </dict>
    <dict>
      <key>config</key><dict>
        <key>concurrently</key><true/>
        <key>escaping</key><integer>102</integer>
        <key>script</key><string>./run.sh status</string>
        <key>scriptargtype</key><integer>1</integer>
        <key>scriptfile</key><string></string>
        <key>type</key><integer>0</integer>
      </dict>
      <key>type</key><string>alfred.workflow.action.script</string>
      <key>uid</key><string>incodex.quick.status.action</string>
      <key>version</key><integer>2</integer>
    </dict>
    <dict>
      <key>config</key><dict>
        <key>concurrently</key><true/>
        <key>escaping</key><integer>102</integer>
        <key>script</key><string>./run.sh doctor</string>
        <key>scriptargtype</key><integer>1</integer>
        <key>scriptfile</key><string></string>
        <key>type</key><integer>0</integer>
      </dict>
      <key>type</key><string>alfred.workflow.action.script</string>
      <key>uid</key><string>incodex.quick.doctor.action</string>
      <key>version</key><integer>2</integer>
    </dict>
  </array>
  <key>connections</key>
  <dict>
    <key>incodex.quick.open.input</key><array><dict>
      <key>destinationuid</key><string>incodex.quick.open.action</string>
      <key>modifiers</key><integer>0</integer>
      <key>modifiersubtext</key><string></string>
    </dict></array>
    <key>incodex.quick.status.input</key><array><dict>
      <key>destinationuid</key><string>incodex.quick.status.action</string>
      <key>modifiers</key><integer>0</integer>
      <key>modifiersubtext</key><string></string>
    </dict></array>
    <key>incodex.quick.doctor.input</key><array><dict>
      <key>destinationuid</key><string>incodex.quick.doctor.action</string>
      <key>modifiers</key><integer>0</integer>
      <key>modifiersubtext</key><string></string>
    </dict></array>
  </dict>
</dict>
</plist>
EOF
}

write_alfred_workflow() {
    local quoted_binary="$1"
    local temporary_dir
    local temporary_archive="$ALFRED_WORKFLOW.tmp.$$"
    temporary_dir="$(mktemp -d "${TMPDIR:-/tmp}/incodex-alfred.XXXXXX")"
    trap 'rm -rf "$temporary_dir" "$temporary_archive"' RETURN

    mkdir -p "$ALFRED_DIR"
    write_alfred_runner "$temporary_dir/run.sh" "$quoted_binary"
    write_alfred_plist "$temporary_dir/info.plist"
    /usr/bin/plutil -lint "$temporary_dir/info.plist" >/dev/null
    (
        cd "$temporary_dir"
        /usr/bin/zip -q -X "$temporary_archive" info.plist run.sh
    )
    mv -f "$temporary_archive" "$ALFRED_WORKFLOW"
    rm -rf "$temporary_dir"
    trap - RETURN
}

maybe_open_alfred_import() {
    if [[ "${INCODEX_LAUNCHERS_NO_OPEN:-0}" == "1" ]]; then
        return
    fi
    local app="${INCODEX_ALFRED_APP:-}"
    if [[ -z "$app" ]]; then
        if [[ -d "/Applications/Alfred 5.app" ]]; then
            app="/Applications/Alfred 5.app"
        elif [[ -d "/Applications/Alfred 4.app" ]]; then
            app="/Applications/Alfred 4.app"
        fi
    fi
    if [[ -n "$app" && -d "$app" ]]; then
        /usr/bin/open -a "$app" "$ALFRED_WORKFLOW"
        printf 'Alfred import window opened; confirm the import in Alfred.\n'
    else
        printf 'Alfred was not found; double-click this package after installing Alfred:\n  %s\n' "$ALFRED_WORKFLOW"
    fi
}

install_launchers() {
    local binary
    local quoted_binary
    binary="$(resolve_incodex)"
    quoted_binary="$(shell_quote "$binary")"

    mkdir -p "$ROOT"
    if [[ -e "$MARKER" ]] && [[ "$(cat "$MARKER")" != "$OWNER_MARKER" ]]; then
        die "ownership marker changed; refusing to install launcher files"
    fi
    preflight_install_ownership
    printf '%s\n' "$OWNER_MARKER" >"$MARKER"
    write_raycast_launchers "$quoted_binary"
    write_alfred_workflow "$quoted_binary"

    printf 'Quick launchers are ready.\n'
    printf 'Raycast: Settings > Script Commands > Add Script Directory:\n  %s\n' "$RAYCAST_DIR"
    printf 'Alfred package:\n  %s\n' "$ALFRED_WORKFLOW"
    maybe_open_alfred_import
}

remove_if_owned() {
    local path="$1"
    if [[ ! -e "$path" ]]; then
        return
    fi
    if grep -Fq "$OWNER_MARKER" "$path"; then
        rm -f "$path"
    else
        printf 'Skipped modified or foreign file: %s\n' "$path" >&2
    fi
}

uninstall_launchers() {
    require_owned_root
    remove_if_owned "$RAYCAST_DIR/incodex-open.sh"
    remove_if_owned "$RAYCAST_DIR/incodex-status.sh"
    remove_if_owned "$RAYCAST_DIR/incodex-doctor.sh"
    if [[ -f "$ALFRED_WORKFLOW" ]]; then
        if /usr/bin/unzip -p "$ALFRED_WORKFLOW" run.sh 2>/dev/null | grep -Fq "$OWNER_MARKER"; then
            rm -f "$ALFRED_WORKFLOW"
        else
            printf 'Skipped modified or foreign Alfred package: %s\n' "$ALFRED_WORKFLOW" >&2
        fi
    fi
    rm -f "$MARKER"
    printf 'Removed Incodex-owned launcher files.\n'
    printf 'Remove the imported workflow manually in Alfred Preferences > Workflows.\n'
    printf 'Remove the Script Directory manually in Raycast Settings if you no longer want it registered.\n'
}

case "${1:-install}" in
    install) install_launchers ;;
    uninstall) uninstall_launchers ;;
    *) die "usage: setup-quick-launchers.sh [install|uninstall]" ;;
esac
