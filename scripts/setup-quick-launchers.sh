#!/bin/bash
# [INPUT]: 依赖已安装的 incodex/inc 二进制、macOS zip/unzip/open/osascript 与用户显式的 Raycast/Alfred 导入操作
# [OUTPUT]: 向 Raycast 标准 Script Commands 目录写入 Incodex 专属脚本并生成稳定 Bundle ID 的 Alfred .alfredworkflow 导入包；发布失败或被终止时恢复原完整集合
# [POS]: scripts 的可选 Quick Launchers 安装器，对齐 Mole 的公开脚本目录但不修改 Raycast/Alfred 私有配置
# [PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md
set -euo pipefail
OWNER_MARKER="incodex-quick-launchers owner=daftAI2026/incodex schema=1"
ROOT="${INCODEX_QUICK_LAUNCHERS_ROOT:-$HOME/.incodex/quick-launchers}"
MARKER="$ROOT/.incodex-quick-launchers"
MANIFEST="$ROOT/manifest.sha256"
RAYCAST_DIR="${INCODEX_RAYCAST_SCRIPT_DIR:-$HOME/Library/Application Support/Raycast/script-commands}"
ALFRED_DIR="$ROOT/alfred"
ALFRED_WORKFLOW="$ALFRED_DIR/Incodex Quick Launchers.alfredworkflow"
OWNED_KEYS=(raycast-open raycast-status raycast-doctor alfred-workflow)
MARKER_TOKEN_VALUE=""
MARKER_MANIFEST_HASH=""
INSTALL_STAGE=""
INSTALL_BACKUP=""
LOCK_DIR=""
LOCK_TOKEN=""
LOCK_OWNED=0
PUBLISH_ACTIVE=0
PUBLISH_ROLLBACK_FAILED=0
PUBLISH_BACKED_UP=()
PUBLISH_PUBLISHED=()
PUBLISH_MARKER_BACKED=0
PUBLISH_MANIFEST_BACKED=0
PUBLISH_NEW_MARKER=0
PUBLISH_NEW_MANIFEST=0
die() {
    printf 'Error: %s\n' "$1" >&2
    exit 1
}
cleanup_install() {
    if [[ -n "$INSTALL_STAGE" ]]; then rm -rf "$INSTALL_STAGE"; fi
    if [[ -n "$INSTALL_BACKUP" ]]; then rm -rf "$INSTALL_BACKUP"; fi
}
rollback_staged() {
    local key path relative i
    PUBLISH_ROLLBACK_FAILED=0
    if (( PUBLISH_NEW_MARKER )); then rm -f "$MARKER" || PUBLISH_ROLLBACK_FAILED=1; fi
    if (( PUBLISH_NEW_MANIFEST )); then rm -f "$MANIFEST" || PUBLISH_ROLLBACK_FAILED=1; fi
    for ((i=${#PUBLISH_PUBLISHED[@]} - 1; i >= 0; i--)); do
        key="${PUBLISH_PUBLISHED[i]}"
        rm -f "$(owned_artifact_path "$key")" || PUBLISH_ROLLBACK_FAILED=1
    done
    if (( PUBLISH_MARKER_BACKED )) && [[ -e "$INSTALL_BACKUP/.incodex-quick-launchers" ]]; then
        mv -f "$INSTALL_BACKUP/.incodex-quick-launchers" "$MARKER" || PUBLISH_ROLLBACK_FAILED=1
    fi
    if (( PUBLISH_MANIFEST_BACKED )) && [[ -e "$INSTALL_BACKUP/manifest.sha256" ]]; then
        mv -f "$INSTALL_BACKUP/manifest.sha256" "$MANIFEST" || PUBLISH_ROLLBACK_FAILED=1
    fi
    for ((i=${#PUBLISH_BACKED_UP[@]} - 1; i >= 0; i--)); do
        key="${PUBLISH_BACKED_UP[i]}"
        relative="$(owned_staged_relative_path "$key")"
        path="$(owned_artifact_path "$key")"
        if [[ -e "$INSTALL_BACKUP/$relative" ]]; then mv -f "$INSTALL_BACKUP/$relative" "$path" || PUBLISH_ROLLBACK_FAILED=1; fi
    done
    PUBLISH_ACTIVE=0
}
on_install_exit() {
    local status="$?"
    if (( PUBLISH_ACTIVE )); then rollback_staged; fi
    cleanup_install
    release_install_lock
    return "$status"
}
acquire_install_lock() {
    local lock="$ROOT/.install.lock"
    assert_path_not_redirected "$ROOT" "launcher root"
    assert_directory_not_redirected "$ROOT" "launcher root"
    mkdir -p "$ROOT"
    if ! mkdir "$lock" 2>/dev/null; then
        die "another quick launcher install is active or left a stale lock; refusing concurrent setup"
    fi
    LOCK_DIR="$lock"
    LOCK_OWNED=1
    LOCK_TOKEN="$(new_marker_token)"
    printf '%s:%s\n' "$$" "$LOCK_TOKEN" >"$LOCK_DIR/owner"
}
release_install_lock() {
    local owner
    if (( LOCK_OWNED )) && [[ -d "$LOCK_DIR" && ! -L "$LOCK_DIR" ]] && owner="$(cat "$LOCK_DIR/owner" 2>/dev/null)" && [[ "$owner" == "$$:$LOCK_TOKEN" ]]; then
        rm -f "$LOCK_DIR/owner"
        rmdir "$LOCK_DIR" 2>/dev/null || true
    fi
    LOCK_OWNED=0
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
assert_path_not_redirected() {
    local raw_path="$1" label="$2" path="$1" probe parent
    if [[ "$path" != /* ]]; then
        path="$PWD/$path"
    fi
    case "/$path/" in
        */../*)
            die "$label contains '..'; refusing launcher filesystem changes"
            ;;
    esac
    probe="$path"
    while :; do
        case "$probe" in
            /|/tmp|/var|/private)
                break
                ;;
        esac
        if [[ -L "$probe" ]]; then
            die "$label or one of its parent directories is a symlink: $probe"
        fi
        if [[ "$probe" == "$HOME" ]]; then
            break
        fi
        parent="${probe%/*}"
        if [[ -z "$parent" || "$parent" == "$probe" ]]; then
            parent="/"
        fi
        probe="$parent"
    done
}
assert_directory_not_redirected() {
    local path="$1" label="$2"
    if [[ -L "$path" ]]; then
        die "$label is a symlink; refusing launcher filesystem changes"
    fi
    if [[ -e "$path" ]] && [[ ! -d "$path" ]]; then
        die "$label is not a directory; refusing launcher filesystem changes"
    fi
}
inspect_owned_directories() {
    assert_path_not_redirected "$ROOT" "launcher root"
    assert_path_not_redirected "$RAYCAST_DIR" "Raycast launcher directory"
    assert_path_not_redirected "$ALFRED_DIR" "Alfred launcher directory"
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
    local parsed
    if [[ ! -f "$MARKER" ]]; then
        die "ownership marker is missing; refusing to remove launcher files"
    fi
    if ! parsed="$(/usr/bin/awk -v owner="$OWNER_MARKER" '
        NR == 1 {
            if ($0 != owner) bad=1
            next
        }
        NR == 2 {
            if (length($0) != 38 || substr($0, 1, 6) != "token=" || substr($0, 7) ~ /[^0-9a-f]/) {
                bad=1
            } else {
                token=substr($0, 7)
            }
            next
        }
        NR == 3 {
            if (length($0) != 80 || substr($0, 1, 16) != "manifest-sha256=" || substr($0, 17) ~ /[^0-9a-f]/) {
                bad=1
            } else {
                digest=substr($0, 17)
            }
            next
        }
        { bad=1 }
        END {
            if (NR != 3 || bad || token == "" || digest == "") exit 1
            printf "%s\t%s\n", token, digest
        }
    ' "$MARKER")"; then
        die "ownership marker is missing or invalid; refusing launcher filesystem changes"
    fi
    MARKER_TOKEN_VALUE="${parsed%%$'\t'*}"
    MARKER_MANIFEST_HASH="${parsed#*$'\t'}"
}
owned_staged_relative_path() {
    case "$1" in
        raycast-open) printf '%s\n' "raycast/incodex-open.sh" ;;
        raycast-status) printf '%s\n' "raycast/incodex-status.sh" ;;
        raycast-doctor) printf '%s\n' "raycast/incodex-doctor.sh" ;;
        alfred-workflow) printf '%s\n' "alfred/Incodex Quick Launchers.alfredworkflow" ;;
        *) die "unknown launcher manifest key: $1" ;;
    esac
}
owned_artifact_path() {
    case "$1" in
        raycast-open) printf '%s\n' "$RAYCAST_DIR/incodex-open.sh" ;;
        raycast-status) printf '%s\n' "$RAYCAST_DIR/incodex-status.sh" ;;
        raycast-doctor) printf '%s\n' "$RAYCAST_DIR/incodex-doctor.sh" ;;
        alfred-workflow) printf '%s\n' "$ALFRED_WORKFLOW" ;;
        *) die "unknown launcher manifest key: $1" ;;
    esac
}
sha256_file() {
    /usr/bin/shasum -a 256 "$1" | /usr/bin/awk '{print $1}'
}
manifest_hash() {
    local key="$1" line
    if ! line="$(/usr/bin/awk -F '\t' -v key="$key" '
        $2 == key {
            count++
            value=$1
        }
        END {
            if (count != 1) exit 1
            print value
        }
    ' "$MANIFEST")"; then
        die "ownership manifest is incomplete; refusing launcher filesystem changes"
    fi
    if [[ "${#line}" -ne 64 ]] || [[ "$line" == *[!0-9a-f]* ]]; then
        die "ownership manifest contains an invalid hash; refusing launcher filesystem changes"
    fi
    printf '%s\n' "$line"
}
require_manifest() {
    local actual_manifest_hash
    if [[ ! -f "$MANIFEST" ]]; then
        die "ownership manifest is missing or invalid; refusing launcher filesystem changes"
    fi
    actual_manifest_hash="$(sha256_file "$MANIFEST")"
    if [[ "$actual_manifest_hash" != "$MARKER_MANIFEST_HASH" ]]; then
        die "ownership manifest changed; refusing launcher filesystem changes"
    fi
    if ! /usr/bin/awk -F '\t' \
        -v owner="$OWNER_MARKER" \
        -v token="$MARKER_TOKEN_VALUE" \
        'BEGIN {
            split("raycast-open,raycast-status,raycast-doctor,alfred-workflow", names, ",")
            for (i in names) expected[names[i]]=1
        }
        NR == 1 {
            if ($0 != "# " owner) bad=1
            next
        }
        NR == 2 {
            if ($0 != "# token=" token) bad=1
            next
        }
        NR >= 3 {
            if (NF != 2 || $0 != $1 "\t" $2 || !($2 in expected) || seen[$2] > 0 || length($1) != 64 || $1 ~ /[^0-9a-f]/) {
                bad=1
            }
            seen[$2]++
            count++
        }
        END {
            for (name in expected) {
                if (seen[name] != 1) bad=1
            }
            if (NR != 6 || count != 4 || bad) exit 1
        }' "$MANIFEST"; then
        die "ownership manifest is missing or invalid; refusing launcher filesystem changes"
    fi
}
verify_installed_artifacts() {
    local key path expected
    for key in "${OWNED_KEYS[@]}"; do
        path="$(owned_artifact_path "$key")"
        expected="$(manifest_hash "$key")"
        if [[ -L "$path" ]] || [[ ! -f "$path" ]] || [[ "$(sha256_file "$path")" != "$expected" ]]; then
            die "launcher is modified; refusing to overwrite it: $path"
        fi
    done
}
assert_fresh_targets_absent() {
    local key path
    for key in "${OWNED_KEYS[@]}"; do
        path="$(owned_artifact_path "$key")"
        if [[ -e "$path" || -L "$path" ]]; then
            die "refusing to overwrite a launcher without installation ownership: $path"
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
new_marker_token() {
    local token
    token="$(/usr/bin/od -An -N16 -tx1 /dev/urandom | /usr/bin/tr -d ' \n')"
    if [[ "${#token}" -ne 32 ]] || [[ "$token" == *[!0-9a-f]* ]]; then
        die "could not create an ownership token"
    fi
    printf '%s\n' "$token"
}
write_manifest() {
    local base="$1" token="$2" target="$1/manifest.sha256" key relative
    printf '# %s\n# token=%s\n' "$OWNER_MARKER" "$token" >"$target"
    for key in "${OWNED_KEYS[@]}"; do
        relative="$(owned_staged_relative_path "$key")"
        printf '%s\t%s\n' "$(sha256_file "$base/$relative")" "$key" >>"$target"
    done
}
write_terminal_router() {
    local target="$1"
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
        iTerm|iTerm2) has_app "iTerm" || has_app "iTerm2" ;;
        Alacritty) has_app "Alacritty" ;;
        kitty|Kitty) has_bin "kitty" || has_app "kitty" ;;
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
        if launcher_available "$INCODEX_LAUNCHER_APP"; then
            printf '%s\n' "$INCODEX_LAUNCHER_APP"
            return
        fi
        printf 'Requested terminal is unavailable; falling back to Terminal.\n' >&2
        printf '%s\n' 'Terminal'
        return
    fi
    local candidate
    for candidate in Warp Ghostty Alacritty kitty WezTerm WindTerm Hyper iTerm2 Terminal; do
        if launcher_available "$candidate"; then
            printf '%s\n' "$candidate"
            return
        fi
    done
    printf '%s\n' 'Terminal'
}
launch_with_app() {
    local app="$1" target_command="$2" osascript_command
    case "$app" in
        Terminal)
            osascript_command="$(command -v osascript 2>/dev/null || true)"
            if [[ -z "$osascript_command" ]]; then
                return 1
            fi
            "$osascript_command" - "$target_command" <<'APPLESCRIPT'
on run argv
    tell application "Terminal"
        activate
        do script item 1 of argv
    end tell
end run
APPLESCRIPT
            ;;
        iTerm|iTerm2)
            osascript_command="$(command -v osascript 2>/dev/null || true)"
            if [[ -z "$osascript_command" ]]; then
                return 1
            fi
            "$osascript_command" - "$target_command" <<'APPLESCRIPT'
on run argv
    tell application "iTerm2"
        activate
        if (count of windows) is 0 then create window with default profile
        tell current session of current window to write text item 1 of argv
    end tell
end run
APPLESCRIPT
            ;;
        Alacritty)
            if ! command -v open >/dev/null 2>&1; then
                return 1
            fi
            open -na "Alacritty" --args -e /bin/zsh -lc "$target_command"
            ;;
        Ghostty)
            if ! command -v open >/dev/null 2>&1; then
                return 1
            fi
            open -na "Ghostty" --args -e /bin/zsh -lc "$target_command; exec /bin/zsh -l"
            ;;
        Hyper)
            if ! command -v open >/dev/null 2>&1; then
                return 1
            fi
            open -na "Hyper" --args /bin/zsh -lc "$target_command"
            ;;
        WindTerm)
            if ! command -v open >/dev/null 2>&1; then
                return 1
            fi
            open -na "WindTerm" --args /bin/zsh -lc "$target_command"
            ;;
        Warp)
            if ! command -v open >/dev/null 2>&1; then
                return 1
            fi
            open -na "Warp" --args /bin/zsh -lc "$target_command"
            ;;
        kitty|Kitty)
            if has_bin "kitty"; then
                kitty --hold /bin/zsh -lc "$target_command"
            else
                if ! command -v open >/dev/null 2>&1; then
                    return 1
                fi
                open -na "kitty" --args --hold /bin/zsh -lc "$target_command"
            fi
            ;;
        WezTerm)
            if has_bin "wezterm"; then
                wezterm start -- /bin/zsh -lc "$target_command"
            else
                if ! command -v open >/dev/null 2>&1; then
                    return 1
                fi
                open -na "WezTerm" --args start -- /bin/zsh -lc "$target_command"
            fi
            ;;
        *)
            return 1
            ;;
    esac
}
launch_in_terminal() {
    local subcommand="$1"
    local target_command
    local TERM_APP
    printf -v target_command '%q %q' "$INCODEX_BIN" "$subcommand"
    TERM_APP="$(detect_launcher_app)"
    if launch_with_app "$TERM_APP" "$target_command"; then
        return 0
    fi
    if [[ "$TERM_APP" != "Terminal" ]]; then
        printf 'Could not start %s; falling back to Terminal.\n' "$TERM_APP" >&2
        if launch_with_app "Terminal" "$target_command"; then
            return 0
        fi
    fi
    printf 'No terminal launcher succeeded. Run manually: %q %q\n' "$INCODEX_BIN" "$subcommand" >&2
    return 1
}
EOF
}
write_raycast_script() {
    local target="$1" title="$2" mode="$3" description="$4" command="$5" quoted_binary="$6" temporary="$1.tmp.$$"
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
        write_terminal_router "$temporary"
        {
            printf '%s\n' 'if [[ -n "${TERM:-}" && "${TERM}" != "dumb" ]]; then'
            printf '    exec "$INCODEX_BIN" %s\n' "$command"
            printf '%s\n' 'fi'
            printf 'launch_in_terminal %s\n' "$command"
        } >>"$temporary"
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
    local target="$1" quoted_binary="$2"
    cat >"$target" <<EOF
#!/bin/bash
# $OWNER_MARKER
set -euo pipefail
INCODEX_BIN=$quoted_binary
EOF
    write_terminal_router "$target"
    cat >>"$target" <<'EOF'
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
    local quoted_binary="$1" temporary_dir temporary_archive="$ALFRED_WORKFLOW.tmp.$$"
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
publish_staged() {
    local stage="$1" backup="$2" marker_token="$3" key path relative manifest_digest failed=0
    manifest_digest="$(sha256_file "$stage/manifest.sha256")"
    {
        printf '%s\n' "$OWNER_MARKER"
        printf 'token=%s\n' "$marker_token"
        printf 'manifest-sha256=%s\n' "$manifest_digest"
    } >"$stage/.incodex-quick-launchers"
    inspect_owned_directories
    mkdir -p "$RAYCAST_DIR" "$ALFRED_DIR" "$backup/raycast" "$backup/alfred"
    if [[ ! -w "$RAYCAST_DIR" ]] || [[ ! -w "$ALFRED_DIR" ]]; then
        die "launcher destination is not writable; no launcher files were published"
    fi
    PUBLISH_BACKED_UP=()
    PUBLISH_PUBLISHED=()
    PUBLISH_MARKER_BACKED=0 PUBLISH_MANIFEST_BACKED=0 PUBLISH_NEW_MARKER=0 PUBLISH_NEW_MANIFEST=0 PUBLISH_ACTIVE=1
    for key in "${OWNED_KEYS[@]}"; do
        path="$(owned_artifact_path "$key")"
        if [[ -e "$path" ]]; then
            relative="$(owned_staged_relative_path "$key")"
            PUBLISH_BACKED_UP+=("$key")
            if ! mv -f "$path" "$backup/$relative"; then failed=1; break; fi
        fi
    done
    if (( failed == 0 )) && [[ -e "$MANIFEST" ]]; then PUBLISH_MANIFEST_BACKED=1; mv -f "$MANIFEST" "$backup/manifest.sha256" || failed=1; fi
    if (( failed == 0 )) && [[ -e "$MARKER" ]]; then PUBLISH_MARKER_BACKED=1; mv -f "$MARKER" "$backup/.incodex-quick-launchers" || failed=1; fi
    if (( failed == 0 )); then
        for key in "${OWNED_KEYS[@]}"; do
            relative="$(owned_staged_relative_path "$key")"
            path="$(owned_artifact_path "$key")"
            PUBLISH_PUBLISHED+=("$key")
            if ! mv -f "$stage/$relative" "$path"; then failed=1; break; fi
        done
    fi
    if (( failed == 0 )); then PUBLISH_NEW_MANIFEST=1; mv -f "$stage/manifest.sha256" "$MANIFEST" || failed=1; fi
    if (( failed == 0 )); then PUBLISH_NEW_MARKER=1; mv -f "$stage/.incodex-quick-launchers" "$MARKER" || failed=1; fi
    if (( failed != 0 )); then
        rollback_staged
        if (( PUBLISH_ROLLBACK_FAILED != 0 )); then
            die "launcher publication failed and rollback failed; refusing to claim ownership"
        fi
        die "launcher publication failed; previous launcher state restored"
    fi
}
install_launchers() {
    local binary quoted_binary stage marker_token backup final_raycast_dir="$RAYCAST_DIR" final_alfred_dir="$ALFRED_DIR" final_alfred_workflow="$ALFRED_WORKFLOW"
    binary="$(resolve_incodex)"
    quoted_binary="$(shell_quote "$binary")"
    trap on_install_exit EXIT
    trap 'exit 143' HUP INT TERM
    acquire_install_lock
    prepare_owned_directories
    preflight_install_ownership
    marker_token="$(new_marker_token)"
    stage="$(mktemp -d "${TMPDIR:-/tmp}/incodex-launchers.XXXXXX")"
    INSTALL_STAGE="$stage"
    backup="$(mktemp -d "${TMPDIR:-/tmp}/incodex-launchers-backup.XXXXXX")"
    INSTALL_BACKUP="$backup"
    RAYCAST_DIR="$stage/raycast"
    ALFRED_DIR="$stage/alfred"
    ALFRED_WORKFLOW="$ALFRED_DIR/Incodex Quick Launchers.alfredworkflow"
    write_raycast_launchers "$quoted_binary"
    write_alfred_workflow "$quoted_binary"
    write_manifest "$stage" "$marker_token"
    RAYCAST_DIR="$final_raycast_dir"
    ALFRED_DIR="$final_alfred_dir"
    ALFRED_WORKFLOW="$final_alfred_workflow"
    publish_staged "$stage" "$backup" "$marker_token"
    PUBLISH_ACTIVE=0; trap - EXIT HUP INT TERM
    cleanup_install
    release_install_lock
    INSTALL_STAGE=""
    INSTALL_BACKUP=""
    printf 'Quick launchers are ready.\n'
    printf 'Raycast v2: Settings > Script Commands > Script Folders > +:\n  %s\n' "$RAYCAST_DIR"
    printf 'Alfred package:\n  %s\n' "$ALFRED_WORKFLOW"
    maybe_open_alfred_import
}
case "${1:-install}" in
    install) install_launchers ;;
    *) die "usage: setup-quick-launchers.sh [install]" ;;
esac
