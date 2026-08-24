#!/bin/bash
# [INPUT]: 依赖 macOS 的 zip/unzip/open/osascript 与用户显式的 Raycast/Alfred 导入操作
# [OUTPUT]: 生成一个运行时解析 incodex/inc 的 runner.sh、三个 Raycast 薄 wrapper；仅在检测到 Alfred 时生成标准导入包
# [POS]: scripts 的可选 Quick Launchers 安装器；只管理 Incodex 生成文件，不修改 provider 私有配置
# [PROTOCOL]: 变更时更新此头部，然后检查 CLAUDE.md
set -euo pipefail
GENERATED_MARKER="incodex-quick-launchers generated"
ROOT="${INCODEX_QUICK_LAUNCHERS_ROOT:-$HOME/.incodex/quick-launchers}"
RAYCAST_DIR="${INCODEX_RAYCAST_SCRIPT_DIR:-$HOME/Library/Application Support/Raycast/script-commands}"
ALFRED_DIR="$ROOT/alfred"
ALFRED_WORKFLOW="$ALFRED_DIR/Incodex Quick Launchers.alfredworkflow"
ALFRED_AVAILABLE=0
ALFRED_APP=""
die() {
    printf 'Error: %s\n' "$1" >&2
    exit 1
}
assert_path_not_redirected() {
    local raw_path="$1" label="$2" path="$1" probe parent
    if [[ "$path" != /* ]]; then path="$PWD/$path"; fi
    case "/$path/" in
        */../*) die "$label contains '..'; refusing launcher filesystem changes" ;;
    esac
    probe="$path"
    while :; do
        case "$probe" in /|/tmp|/var|/private) break ;; esac
        if [[ -L "$probe" ]]; then
            die "$label or one of its parent directories is a symlink: $probe"
        fi
        if [[ "$probe" == "$HOME" ]]; then break; fi
        parent="${probe%/*}"
        if [[ -z "$parent" || "$parent" == "$probe" ]]; then parent="/"; fi
        probe="$parent"
    done
}
assert_directory_not_redirected() {
    local path="$1" label="$2"
    if [[ -L "$path" ]]; then die "$label is a symlink; refusing launcher filesystem changes"; fi
    if [[ -e "$path" && ! -d "$path" ]]; then die "$label is not a directory; refusing launcher filesystem changes"; fi
}
detect_alfred() {
    ALFRED_AVAILABLE=0
    ALFRED_APP=""
    if [[ -n "${INCODEX_ALFRED_APP:-}" ]]; then
        if [[ -d "$INCODEX_ALFRED_APP" ]]; then
            ALFRED_AVAILABLE=1
            ALFRED_APP="$INCODEX_ALFRED_APP"
        fi
        return
    fi
    if [[ -d "/Applications/Alfred 5.app" ]]; then
        ALFRED_AVAILABLE=1
        ALFRED_APP="/Applications/Alfred 5.app"
    elif [[ -d "/Applications/Alfred 4.app" ]]; then
        ALFRED_AVAILABLE=1
        ALFRED_APP="/Applications/Alfred 4.app"
    elif [[ -d "$HOME/Library/Application Support/Alfred/Alfred.alfredpreferences" ]]; then
        ALFRED_AVAILABLE=1
    fi
}
inspect_directories() {
    assert_path_not_redirected "$ROOT" "launcher root"
    assert_path_not_redirected "$RAYCAST_DIR" "Raycast launcher directory"
    assert_directory_not_redirected "$ROOT" "launcher root"
    assert_directory_not_redirected "$RAYCAST_DIR" "Raycast launcher directory"
    if (( ALFRED_AVAILABLE )); then
        assert_path_not_redirected "$ALFRED_DIR" "Alfred launcher directory"
        assert_directory_not_redirected "$ALFRED_DIR" "Alfred launcher directory"
    fi
}
prepare_directories() {
    inspect_directories
    mkdir -p "$ROOT" "$RAYCAST_DIR"
    if (( ALFRED_AVAILABLE )); then mkdir -p "$ALFRED_DIR"; fi
    inspect_directories
}
is_generated_text() {
    [[ -f "$1" && ! -L "$1" ]] && /usr/bin/grep -Fqx "# $GENERATED_MARKER" "$1"
}
is_generated_workflow() {
    [[ -f "$1" && ! -L "$1" ]] && /usr/bin/unzip -p "$1" run.sh 2>/dev/null | /usr/bin/grep -Fqx "# $GENERATED_MARKER"
}
assert_target_safe() {
    local target="$1" kind="$2"
    if [[ -L "$target" ]]; then die "refusing to replace symlinked launcher: $target"; fi
    if [[ ! -e "$target" ]]; then return; fi
    if [[ "$kind" == workflow ]]; then
        if ! is_generated_workflow "$target"; then die "refusing to replace foreign launcher: $target"; fi
    elif ! is_generated_text "$target"; then
        die "refusing to replace foreign launcher: $target"
    fi
}
preflight_targets() {
    assert_target_safe "$ROOT/runner.sh" text
    assert_target_safe "$RAYCAST_DIR/incodex-open.sh" text
    assert_target_safe "$RAYCAST_DIR/incodex-status.sh" text
    assert_target_safe "$RAYCAST_DIR/incodex-doctor.sh" text
    if (( ALFRED_AVAILABLE )); then assert_target_safe "$ALFRED_WORKFLOW" workflow; fi
}
shell_quote() {
    printf '%q' "$1"
}
finish_atomic() {
    local temporary="$1" target="$2"
    chmod 0755 "$temporary"
    if ! mv -f "$temporary" "$target"; then
        rm -f "$temporary" 2>/dev/null || true
        return 1
    fi
}
write_runner() {
    local target="$ROOT/runner.sh" temporary="$ROOT/runner.sh.tmp.$$"
    cat >"$temporary" <<'RUNNER'
#!/bin/bash
# incodex-quick-launchers generated
set -euo pipefail
resolve_incodex() {
    local candidate
    for candidate in incodex inc; do
        if command -v "$candidate" >/dev/null 2>&1; then
            command -v "$candidate"
            return 0
        fi
    done
    printf 'Error: incodex was not found in PATH; install the CLI first\n' >&2
    exit 1
}
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
            if [[ -z "$osascript_command" ]]; then return 1; fi
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
            if [[ -z "$osascript_command" ]]; then return 1; fi
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
            if ! command -v open >/dev/null 2>&1; then return 1; fi
            open -na "Alacritty" --args -e /bin/zsh -lc "$target_command"
            ;;
        Ghostty)
            if ! command -v open >/dev/null 2>&1; then return 1; fi
            open -na "Ghostty" --args -e /bin/zsh -lc "$target_command; exec /bin/zsh -l"
            ;;
        Hyper|WindTerm|Warp)
            if ! command -v open >/dev/null 2>&1; then return 1; fi
            open -na "$app" --args /bin/zsh -lc "$target_command"
            ;;
        kitty|Kitty)
            if has_bin "kitty"; then
                kitty --hold /bin/zsh -lc "$target_command"
            else
                if ! command -v open >/dev/null 2>&1; then return 1; fi
                open -na "kitty" --args --hold /bin/zsh -lc "$target_command"
            fi
            ;;
        WezTerm)
            if has_bin "wezterm"; then
                wezterm start -- /bin/zsh -lc "$target_command"
            else
                if ! command -v open >/dev/null 2>&1; then return 1; fi
                open -na "WezTerm" --args start -- /bin/zsh -lc "$target_command"
            fi
            ;;
        *) return 1 ;;
    esac
}
launch_in_terminal() {
    local subcommand="$1" target_command TERM_APP
    printf -v target_command '%q %q' "$INCODEX_BIN" "$subcommand"
    TERM_APP="$(detect_launcher_app)"
    if launch_with_app "$TERM_APP" "$target_command"; then return 0; fi
    if [[ "$TERM_APP" != Terminal ]]; then
        printf 'Could not start %s; falling back to Terminal.\n' "$TERM_APP" >&2
        if launch_with_app Terminal "$target_command"; then return 0; fi
    fi
    printf 'No terminal launcher succeeded. Run manually: %q %q\n' "$INCODEX_BIN" "$subcommand" >&2
    return 1
}
INCODEX_BIN="$(resolve_incodex)"
case "${1:-}" in
    open)
        nohup "$INCODEX_BIN" open </dev/null >/dev/null 2>&1 &
        printf '%s\n' 'Opening an incognito Codex window'
        ;;
    status|doctor)
        if [[ -n "${TERM:-}" && "${TERM}" != dumb ]]; then
            exec "$INCODEX_BIN" "$1"
        fi
        launch_in_terminal "$1"
        ;;
    *)
        printf 'Unknown Incodex launcher: %s\n' "${1:-}" >&2
        exit 64
        ;;
esac
RUNNER
    finish_atomic "$temporary" "$target"
}
write_raycast_wrapper() {
    local target="$1" title="$2" mode="$3" description="$4" command="$5" temporary="$1.tmp.$$" runner_path
    runner_path="$(shell_quote "$ROOT/runner.sh")"
    cat >"$temporary" <<EOF
#!/bin/bash
# $GENERATED_MARKER
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
exec $runner_path $command
EOF
    finish_atomic "$temporary" "$target"
}
write_raycast_launchers() {
    write_raycast_wrapper "$RAYCAST_DIR/incodex-open.sh" "Incodex Open" silent "Open an incognito Codex window" open
    write_raycast_wrapper "$RAYCAST_DIR/incodex-status.sh" "Incodex Status" fullOutput "Show whether Incodex is installed" status
    write_raycast_wrapper "$RAYCAST_DIR/incodex-doctor.sh" "Incodex Doctor" fullOutput "Diagnose the Incodex installation" doctor
}
write_alfred_runner() {
    local target="$1" runner_path
    runner_path="$(shell_quote "$ROOT/runner.sh")"
    cat >"$target" <<EOF
#!/bin/bash
# $GENERATED_MARKER
set -euo pipefail
exec $runner_path "\${1:-}"
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
    <dict><key>config</key><dict><key>argumenttype</key><integer>2</integer><key>keyword</key><string>incognito</string><key>subtext</key><string>Open an incognito Codex window</string><key>text</key><string>Incodex Open</string><key>withspace</key><false/></dict><key>type</key><string>alfred.workflow.input.keyword</string><key>uid</key><string>incodex.quick.open.input</string><key>version</key><integer>1</integer></dict>
    <dict><key>config</key><dict><key>argumenttype</key><integer>2</integer><key>keyword</key><string>inc-status</string><key>subtext</key><string>Show whether Incodex is installed</string><key>text</key><string>Incodex Status</string><key>withspace</key><false/></dict><key>type</key><string>alfred.workflow.input.keyword</string><key>uid</key><string>incodex.quick.status.input</string><key>version</key><integer>1</integer></dict>
    <dict><key>config</key><dict><key>argumenttype</key><integer>2</integer><key>keyword</key><string>inc-doctor</string><key>subtext</key><string>Diagnose the Incodex installation</string><key>text</key><string>Incodex Doctor</string><key>withspace</key><false/></dict><key>type</key><string>alfred.workflow.input.keyword</string><key>uid</key><string>incodex.quick.doctor.input</string><key>version</key><integer>1</integer></dict>
    <dict><key>config</key><dict><key>concurrently</key><true/><key>escaping</key><integer>102</integer><key>script</key><string>./run.sh open</string><key>scriptargtype</key><integer>1</integer><key>scriptfile</key><string></string><key>type</key><integer>0</integer></dict><key>type</key><string>alfred.workflow.action.script</string><key>uid</key><string>incodex.quick.open.action</string><key>version</key><integer>2</integer></dict>
    <dict><key>config</key><dict><key>concurrently</key><true/><key>escaping</key><integer>102</integer><key>script</key><string>./run.sh status</string><key>scriptargtype</key><integer>1</integer><key>scriptfile</key><string></string><key>type</key><integer>0</integer></dict><key>type</key><string>alfred.workflow.action.script</string><key>uid</key><string>incodex.quick.status.action</string><key>version</key><integer>2</integer></dict>
    <dict><key>config</key><dict><key>concurrently</key><true/><key>escaping</key><integer>102</integer><key>script</key><string>./run.sh doctor</string><key>scriptargtype</key><integer>1</integer><key>scriptfile</key><string></string><key>type</key><integer>0</integer></dict><key>type</key><string>alfred.workflow.action.script</string><key>uid</key><string>incodex.quick.doctor.action</string><key>version</key><integer>2</integer></dict>
  </array>
  <key>connections</key><dict>
    <key>incodex.quick.open.input</key><array><dict><key>destinationuid</key><string>incodex.quick.open.action</string><key>modifiers</key><integer>0</integer><key>modifiersubtext</key><string></string></dict></array>
    <key>incodex.quick.status.input</key><array><dict><key>destinationuid</key><string>incodex.quick.status.action</string><key>modifiers</key><integer>0</integer><key>modifiersubtext</key><string></string></dict></array>
    <key>incodex.quick.doctor.input</key><array><dict><key>destinationuid</key><string>incodex.quick.doctor.action</string><key>modifiers</key><integer>0</integer><key>modifiersubtext</key><string></string></dict></array>
  </dict>
</dict>
</plist>
EOF
}
write_alfred_workflow() {
    local temporary_dir temporary_archive="$ALFRED_WORKFLOW.tmp.$$"
    temporary_dir="$(mktemp -d "$ALFRED_DIR/.tmp.XXXXXX")"
    trap 'rm -rf "$temporary_dir" "$temporary_archive"' RETURN
    write_alfred_runner "$temporary_dir/run.sh"
    write_alfred_plist "$temporary_dir/info.plist"
    /usr/bin/plutil -lint "$temporary_dir/info.plist" >/dev/null
    (cd "$temporary_dir" && /usr/bin/zip -q -X "$temporary_archive" info.plist run.sh)
    if ! mv -f "$temporary_archive" "$ALFRED_WORKFLOW"; then return 1; fi
    rm -rf "$temporary_dir"
    trap - RETURN
}
maybe_open_alfred_import() {
    if [[ "${INCODEX_LAUNCHERS_NO_OPEN:-0}" == 1 ]]; then return; fi
    if [[ -n "$ALFRED_APP" ]]; then /usr/bin/open -a "$ALFRED_APP" "$ALFRED_WORKFLOW"
    else /usr/bin/open "$ALFRED_WORKFLOW"; fi
    printf 'Alfred import window opened; confirm the import in Alfred.\n'
}
setup_launchers() {
    detect_alfred
    prepare_directories
    preflight_targets
    write_runner
    write_raycast_launchers
    printf 'Quick launchers are ready.\n'
    printf 'Raycast v2: Settings > Script Commands > Script Folders > +:\n  %s\n' "$RAYCAST_DIR"
    if (( ALFRED_AVAILABLE )); then
        write_alfred_workflow
        printf 'Alfred package:\n  %s\n' "$ALFRED_WORKFLOW"
        maybe_open_alfred_import
    else
        printf 'Alfred not detected; skipped Alfred workflow generation.\n'
    fi
}
case "${1:-install}" in
    install) setup_launchers ;;
    *) die "usage: setup-quick-launchers.sh [install]" ;;
esac
