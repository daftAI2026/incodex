#!/bin/bash
# Install the Incodex CLI onto PATH. Does not patch Codex.
set -euo pipefail

REPO="${INCODEX_REPO:-daftAI2026/incodex}"
DOWNLOAD_BASE="${INCODEX_DOWNLOAD_BASE:-https://github.com/${REPO}/releases/latest/download}"

die() {
  echo "incodex installer: $*" >&2
  exit 1
}

legacy_bun_prefix() {
  local pid="$PPID"
  local depth=0
  local executable=""
  local bin_dir=""
  while [[ "$pid" =~ ^[0-9]+$ && "$pid" -gt 1 && "$depth" -lt 5 ]]; do
    executable="$(/bin/ps -ww -p "$pid" -o comm= 2>/dev/null | /usr/bin/sed -e 's/^[[:space:]]*//' -e 's/[[:space:]]*$//')"
    case "${executable##*/}" in
      inc | incodex)
        bin_dir="${executable%/*}"
        if [[ "$executable" == /* && "${bin_dir##*/}" == "bin" && -x "$executable" ]]; then
          bin_dir="$(cd -P "$bin_dir" 2>/dev/null && pwd)" || return 1
          printf '%s\n' "${bin_dir%/bin}"
          return 0
        fi
        ;;
    esac
    pid="$(/bin/ps -p "$pid" -o ppid= 2>/dev/null | /usr/bin/tr -d '[:space:]')"
    depth=$((depth + 1))
  done
  return 1
}

PREFIX="${INCODEX_PREFIX:-${HOME}/.local}"
case "$PREFIX" in
  /\$bunfs | /\$bunfs/*)
    PREFIX="$(legacy_bun_prefix)" ||
      die "could not recover legacy Bun update prefix; rerun install.sh with INCODEX_PREFIX=/your/prefix"
    ;;
esac
BIN_DIR="${PREFIX}/bin"

refuse_managed_conflict() {
  [[ -n "${INCODEX_DOWNLOAD_DIR:-}" ]] && return 0
  command -v brew >/dev/null 2>&1 || return 0
  local installed=""
  installed="$(HOMEBREW_NO_AUTO_UPDATE=1 brew list --versions incodex 2>/dev/null)" || return 0
  [[ -n "$installed" ]] || return 0
  local brew_prefix=""
  local location=""
  if brew_prefix="$(HOMEBREW_NO_AUTO_UPDATE=1 brew --prefix incodex 2>/dev/null)" && [[ -n "$brew_prefix" ]]; then
    location=" at ${brew_prefix}/bin/incodex"
  fi
  die "Homebrew-managed Incodex detected${location}. Run 'brew upgrade incodex', or 'brew uninstall incodex' before switching to the script installer"
}

arch_name() {
  local machine="${INCODEX_ARCH:-$(uname -m)}"
  case "$machine" in
    arm64 | aarch64) echo "incodex-darwin-arm64" ;;
    x86_64 | amd64) echo "incodex-darwin-x64" ;;
    *) die "unsupported architecture: ${machine}" ;;
  esac
}

fetch() {
  local src="$1"
  local dest="$2"
  if [[ -n "${INCODEX_DOWNLOAD_DIR:-}" ]]; then
    [[ -f "${INCODEX_DOWNLOAD_DIR}/${src}" ]] || die "missing ${src} in ${INCODEX_DOWNLOAD_DIR}"
    cp "${INCODEX_DOWNLOAD_DIR}/${src}" "${dest}"
    return
  fi
  command -v curl >/dev/null 2>&1 || die "curl is required"
  local attempt=1
  local curl_exit=0
  while true; do
    if curl -fsSL --connect-timeout 10 --max-time 60 "${DOWNLOAD_BASE}/${src}" -o "${dest}"; then
      return 0
    else
      curl_exit=$?
    fi
    rm -f "$dest"
    case "$curl_exit" in
      6 | 7 | 18 | 28 | 35 | 52 | 55 | 56) ;;
      *) die "failed to download ${src} (curl ${curl_exit})" ;;
    esac
    [[ "$attempt" -lt 3 ]] || die "failed to download ${src} after 3 attempts"
    sleep 0.2
    attempt=$((attempt + 1))
  done
}

expected_sha() {
  local sums="$1"
  local name="$2"
  awk -v name="$name" '
    $2 == name { print $1; found = 1 }
    END { if (!found) exit 1 }
  ' "$sums"
}

ASSET="$(arch_name)"
refuse_managed_conflict
WORKDIR="$(mktemp -d "${TMPDIR:-/tmp}/incodex-setup.XXXXXX")"
STAGED_CLI=""
STAGED_ALIAS_DIR=""
cleanup() {
  [[ -n "$STAGED_CLI" ]] && rm -f "$STAGED_CLI"
  [[ -n "$STAGED_ALIAS_DIR" ]] && rm -rf "$STAGED_ALIAS_DIR"
  rm -rf "$WORKDIR"
}
trap cleanup EXIT

fetch SHA256SUMS "${WORKDIR}/SHA256SUMS"
[[ -s "${WORKDIR}/SHA256SUMS" ]] || die "SHA256SUMS is missing or empty"

fetch "$ASSET" "${WORKDIR}/${ASSET}"
[[ -f "${WORKDIR}/${ASSET}" ]] || die "missing ${ASSET}"

EXPECT="$(expected_sha "${WORKDIR}/SHA256SUMS" "$ASSET")" || die "SHA256SUMS has no entry for ${ASSET}"
ACTUAL="$(shasum -a 256 "${WORKDIR}/${ASSET}" | awk '{ print $1 }')"
[[ "$EXPECT" == "$ACTUAL" ]] || die "checksum mismatch for ${ASSET}"

mkdir -p "$BIN_DIR"
[[ ! -d "${BIN_DIR}/incodex" ]] || die "${BIN_DIR}/incodex is a directory"
[[ ! -d "${BIN_DIR}/inc" ]] || die "${BIN_DIR}/inc is a directory"
STAGED_CLI="$(mktemp "${BIN_DIR}/.incodex.XXXXXX")"
cp "${WORKDIR}/${ASSET}" "$STAGED_CLI"
chmod 755 "$STAGED_CLI"

PROBE_OUTPUT="$("$STAGED_CLI" --version 2>/dev/null)" || die "downloaded ${ASSET} is not runnable"
if [[ -n "${INCODEX_EXPECTED_VERSION:-}" ]]; then
  REPORTED_VERSION="$(printf '%s\n' "$PROBE_OUTPUT" | awk '$1 == "Incodex" && $2 == "version" { print $3; exit }')"
  [[ "$REPORTED_VERSION" == "$INCODEX_EXPECTED_VERSION" ]] ||
    die "downloaded ${ASSET} does not report expected version ${INCODEX_EXPECTED_VERSION}"
fi

mv -f "$STAGED_CLI" "${BIN_DIR}/incodex"
STAGED_CLI=""
STAGED_ALIAS_DIR="$(mktemp -d "${BIN_DIR}/.inc-alias.XXXXXX")"
ln -s incodex "${STAGED_ALIAS_DIR}/inc"
mv -f "${STAGED_ALIAS_DIR}/inc" "${BIN_DIR}/inc"
rmdir "$STAGED_ALIAS_DIR"
STAGED_ALIAS_DIR=""

echo "installed ${BIN_DIR}/incodex"
echo "alias     ${BIN_DIR}/inc"
echo "add ${BIN_DIR} to PATH if needed, then run: incodex --help"
