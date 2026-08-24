#!/bin/bash
# [INPUT]: 依赖已安装的 incodex/inc 二进制、macOS zip/unzip/open/osascript 与用户显式的 Raycast/Alfred 导入操作
# [OUTPUT]: 生成 Incodex 独占的 Raycast Script Commands 目录与稳定 Bundle ID 的 Alfred .alfredworkflow 导入包
# [POS]: scripts 的可选 Quick Launchers 安装器，不修改 Raycast/Alfred 私有配置，不依赖 CLI 安装器或官方 App 补丁路径
# [PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md

set -euo pipefail

OWNER_MARKER="incodex-quick-launchers owner=daftAI2026/incodex schema=1"
ROOT="${INCODEX_QUICK_LAUNCHERS_ROOT:-$HOME/.incodex/quick-launchers}"
MARKER="$ROOT/.incodex-quick-launchers"
MANIFEST="$ROOT/manifest.sha256"
RAYCAST_DIR="$ROOT/raycast"
ALFRED_DIR="$ROOT/alfred"
ALFRED_WORKFLOW="$ALFRED_DIR/Incodex Quick Launchers.alfredworkflow"
OWNED_KEYS=(raycast-open raycast-status raycast-doctor alfred-workflow)

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

assert_directory_not_redirected() {
    local path="$1"
    local label="$2"
    if [[ -L "$path" ]]; then
        die "$label is a symlink; refusing launcher filesystem changes"
    fi
    if [[ -e "$path" ]] && [[ ! -d "$path" ]]; then
        die "$label is not a directory; refusing launcher filesystem changes"
    fi
}

inspect_owned_directories() {
    assert_directory_not_redirected "$ROOT" "launcher root"
    assert_directory_not_redirected "$RAYCAST_DIR" "Raycast launcher directory"
    assert_directory_not_redirected "$ALFRED_DIR" "Alfred launcher directory"
    if [[ -L "$MARKER" ]]; then
        die "ownership marker is a symlink; refusing launcher filesystem changes"
    fi
    if [[ -L "$MANIFEST" ]]; then
        die "ownership manifest is a symlink; refusing launcher filesystem changes"
    fi
}

prepare_owned_directories() {
    inspect_owned_directories
    mkdir -p "$ROOT"
    inspect_owned_directories
}

require_owned_root() {
    if [[ ! -f "$MARKER" ]]; then
        die "ownership marker is missing; refusing to remove launcher files"
    fi
    if [[ "$(cat "$MARKER")" != "$OWNER_MARKER" ]]; then
        die "ownership marker changed; refusing to remove launcher files"
    fi
}

owned_relative_path() {
    case "$1" in
        raycast-open) printf '%s\n' "raycast/incodex-open.sh" ;;
        raycast-status) printf '%s\n' "raycast/incodex-status.sh" ;;
        raycast-doctor) printf '%s\n' "raycast/incodex-doctor.sh" ;;
        alfred-workflow) printf '%s\n' "alfred/Incodex Quick Launchers.alfredworkflow" ;;
        *) die "unknown launcher manifest key: $1" ;;
    esac
}

sha256_file() {
    /usr/bin/shasum -a 256 "$1" | /usr/bin/awk '{print $1}'
}

manifest_hash() {
    local key="$1"
    local line
    line="$(/usr/bin/grep -F "$key" "$MANIFEST" 2>/dev/null || true)"
    if [[ "$line" != *$'\t'"$key" ]] || [[ "${line%%$'\t'*}" == "$line" ]]; then
        die "ownership manifest is incomplete; refusing launcher filesystem changes"
    fi
    printf '%s\n' "${line%%$'\t'*}"
}

require_manifest() {
    if [[ ! -f "$MANIFEST" ]] || ! /usr/bin/grep -Fxq "# $OWNER_MARKER" "$MANIFEST"; then
        die "ownership manifest is missing or invalid; refusing launcher filesystem changes"
    fi
}

verify_installed_artifacts() {
    local key
    local relative
    local expected
    for key in "${OWNED_KEYS[@]}"; do
        relative="$(owned_relative_path "$key")"
        expected="$(manifest_hash "$key")"
        if [[ ! -f "$ROOT/$relative" ]] || [[ "$(sha256_file "$ROOT/$relative")" != "$expected" ]]; then
            die "launcher is modified; refusing to overwrite it: $ROOT/$relative"
        fi
    done
}

assert_fresh_targets_absent() {
    local key
    local relative
    for key in "${OWNED_KEYS[@]}"; do
        relative="$(owned_relative_path "$key")"
        if [[ -e "$ROOT/$relative" ]]; then
            die "refusing to overwrite a launcher without installation ownership: $ROOT/$relative"
        fi
    done
    if [[ -e "$MANIFEST" ]]; then
        die "refusing to overwrite an unowned launcher manifest: $MANIFEST"
    fi
}

preflight_install_ownership() {
    if [[ -e "$MARKER" ]]; then
        require_owned_root
        require_manifest
        verify_installed_artifacts
    else
        assert_fresh_targets_absent
    fi
}

write_manifest() {
    local base="$1"
    local target="$base/manifest.sha256"
    local key
    local relative
    printf '# %s\n' "$OWNER_MARKER" >"$target"
    for key in "${OWNED_KEYS[@]}"; do
        relative="$(owned_relative_path "$key")"
        printf '%s\t%s\n' "$(sha256_file "$base/$relative")" "$key" >>"$target"
    done
}

write_raycast_script() {
    local target="$1"
    local title="$2"
    local mode="$3"
    local description="$4"
    local command="$5"
    local quoted_binary="$6"
    local temporary="$target.tmp.$$"

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
if [[ ! -x "\$INCODEX_BIN" ]]; then
    printf 'Incodex binary is no longer executable: %s\n' "\$INCODEX_BIN" >&2
    exit 1
fi
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
EOF
    cat >>"$target" <<'EOF'

has_app() {
    local name="$1"
    [[ -d "/Applications/${name}.app" || -d "$HOME/Applications/${name}.app" ]]
}

has_bin() {
    command -v "$1" >/dev/null 2>&1
}

launcher_available() {
    case "$1" in
        Terminal) return 0 ;;
        iTerm2) has_app "iTerm" || has_app "iTerm2" ;;
        Alacritty) has_app "Alacritty" ;;
        kitty) has_bin "kitty" || has_app "kitty" ;;
        WezTerm) has_bin "wezterm" || has_app "WezTerm" ;;
        Ghostty) has_bin "ghostty" || has_app "Ghostty" ;;
        Hyper) has_app "Hyper" ;;
        WindTerm) has_app "WindTerm" ;;
        Warp) has_app "Warp" ;;
        *) return 1 ;;
    esac
}

detect_launcher_app() {
    if [[ -n "${INCODEX_LAUNCHER_APP:-}" ]]; then
        if ! launcher_available "$INCODEX_LAUNCHER_APP"; then
            printf 'Requested terminal is unavailable: %s\n' "$INCODEX_LAUNCHER_APP" >&2
            return 1
        fi
        printf '%s\n' "$INCODEX_LAUNCHER_APP"
        return
    fi

    local candidate
    for candidate in Warp Ghostty Alacritty kitty WezTerm WindTerm Hyper iTerm2 Terminal; do
        if launcher_available "$candidate"; then
            printf '%s\n' "$candidate"
            return
        fi
    done
}

launch_in_terminal() {
    local subcommand="$1"
    local terminal
    local target_command
    printf -v target_command '%q %q' "$INCODEX_BIN" "$subcommand"
    terminal="$(detect_launcher_app)" || return 1

    case "$terminal" in
        Terminal)
            /usr/bin/osascript - "$target_command" <<'APPLESCRIPT'
on run argv
    tell application "Terminal"
        activate
        do script item 1 of argv
    end tell
end run
APPLESCRIPT
            ;;
        iTerm2)
            /usr/bin/osascript - "$target_command" <<'APPLESCRIPT'
on run argv
    tell application "iTerm2"
        activate
        if (count of windows) is 0 then create window with default profile
        tell current session of current window to write text item 1 of argv
    end tell
end run
APPLESCRIPT
            ;;
        Alacritty|Ghostty|Hyper|WindTerm|Warp)
            /usr/bin/open -na "$terminal" --args -e /bin/zsh -lc "$target_command; exec /bin/zsh -l"
            ;;
        kitty)
            if has_bin kitty; then
                kitty --hold /bin/zsh -lc "$target_command"
            else
                /usr/bin/open -na "kitty" --args --hold /bin/zsh -lc "$target_command"
            fi
            ;;
        WezTerm)
            if has_bin wezterm; then
                wezterm start -- /bin/zsh -lc "$target_command; exec /bin/zsh -l"
            else
                /usr/bin/open -na "WezTerm" --args start -- /bin/zsh -lc "$target_command; exec /bin/zsh -l"
            fi
            ;;
    esac
}

case "${1:-}" in
    open)
        nohup "$INCODEX_BIN" open </dev/null >/dev/null 2>&1 &
        ;;
    status|doctor)
        launch_in_terminal "$1"
        ;;
    *)
        printf 'Unknown Incodex launcher: %s\n' "${1:-}" >&2
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
    local stage
    local final_raycast_dir="$RAYCAST_DIR"
    local final_alfred_dir="$ALFRED_DIR"
    local final_alfred_workflow="$ALFRED_WORKFLOW"
    binary="$(resolve_incodex)"
    quoted_binary="$(shell_quote "$binary")"

    prepare_owned_directories
    preflight_install_ownership

    stage="$(mktemp -d "${TMPDIR:-/tmp}/incodex-launchers.XXXXXX")"
    RAYCAST_DIR="$stage/raycast"
    ALFRED_DIR="$stage/alfred"
    ALFRED_WORKFLOW="$ALFRED_DIR/Incodex Quick Launchers.alfredworkflow"
    write_raycast_launchers "$quoted_binary"
    write_alfred_workflow "$quoted_binary"
    write_manifest "$stage"

    RAYCAST_DIR="$final_raycast_dir"
    ALFRED_DIR="$final_alfred_dir"
    ALFRED_WORKFLOW="$final_alfred_workflow"
    mkdir -p "$RAYCAST_DIR" "$ALFRED_DIR"
    if [[ ! -w "$RAYCAST_DIR" ]] || [[ ! -w "$ALFRED_DIR" ]]; then
        rm -rf "$stage"
        die "launcher destination is not writable; no launcher files were published"
    fi

    mv -f "$stage/raycast/incodex-open.sh" "$RAYCAST_DIR/incodex-open.sh"
    mv -f "$stage/raycast/incodex-status.sh" "$RAYCAST_DIR/incodex-status.sh"
    mv -f "$stage/raycast/incodex-doctor.sh" "$RAYCAST_DIR/incodex-doctor.sh"
    mv -f "$stage/alfred/Incodex Quick Launchers.alfredworkflow" "$ALFRED_WORKFLOW"
    mv -f "$stage/manifest.sha256" "$MANIFEST"
    printf '%s\n' "$OWNER_MARKER" >"$MARKER.tmp.$$"
    mv -f "$MARKER.tmp.$$" "$MARKER"
    rm -rf "$stage"

    printf 'Quick launchers are ready.\n'
    printf 'Raycast: Settings > Script Commands > Add Script Directory:\n  %s\n' "$RAYCAST_DIR"
    printf 'Alfred package:\n  %s\n' "$ALFRED_WORKFLOW"
    maybe_open_alfred_import
}

uninstall_launchers() {
    local key
    local relative
    local path
    local expected
    local modified=0

    inspect_owned_directories
    require_owned_root
    require_manifest
    for key in "${OWNED_KEYS[@]}"; do
        relative="$(owned_relative_path "$key")"
        path="$ROOT/$relative"
        expected="$(manifest_hash "$key")"
        if [[ ! -e "$path" ]]; then
            continue
        fi
        if [[ -f "$path" ]] && [[ "$(sha256_file "$path")" == "$expected" ]]; then
            rm -f "$path"
        else
            printf 'Skipped modified launcher: %s\n' "$path" >&2
            modified=1
        fi
    done

    if [[ "$modified" -ne 0 ]]; then
        die "modified launchers remain; ownership proof was preserved"
    fi

    rm -f "$MANIFEST" "$MARKER"
    rmdir "$RAYCAST_DIR" "$ALFRED_DIR" "$ROOT" 2>/dev/null || true
    printf 'Removed Incodex-owned launcher files.\n'
    printf 'Remove the imported workflow manually in Alfred Preferences > Workflows.\n'
    printf 'Remove the Script Directory manually in Raycast Settings if you no longer want it registered.\n'
}

case "${1:-install}" in
    install) install_launchers ;;
    uninstall) uninstall_launchers ;;
    *) die "usage: setup-quick-launchers.sh [install|uninstall]" ;;
esac
