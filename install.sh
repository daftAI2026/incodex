#!/bin/bash
# Install the Incodex CLI onto PATH. Does not patch Codex.
set -euo pipefail

REPO="${INCODEX_REPO:-daftAI2026/incodex}"
DOWNLOAD_BASE="${INCODEX_DOWNLOAD_BASE:-https://github.com/${REPO}/releases/latest/download}"
PREFIX="${INCODEX_PREFIX:-${HOME}/.local}"
case "$PREFIX" in
  /\$bunfs | /\$bunfs/*) PREFIX="${HOME}/.local" ;;
esac
BIN_DIR="${PREFIX}/bin"

die() {
  echo "incodex installer: $*" >&2
  exit 1
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
  curl -fsSL "${DOWNLOAD_BASE}/${src}" -o "${dest}" || die "failed to download ${src}"
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
WORKDIR="$(mktemp -d "${TMPDIR:-/tmp}/incodex-setup.XXXXXX")"
cleanup() { rm -rf "$WORKDIR"; }
trap cleanup EXIT

fetch SHA256SUMS "${WORKDIR}/SHA256SUMS"
[[ -s "${WORKDIR}/SHA256SUMS" ]] || die "SHA256SUMS is missing or empty"

fetch "$ASSET" "${WORKDIR}/${ASSET}"
[[ -f "${WORKDIR}/${ASSET}" ]] || die "missing ${ASSET}"

EXPECT="$(expected_sha "${WORKDIR}/SHA256SUMS" "$ASSET")" || die "SHA256SUMS has no entry for ${ASSET}"
ACTUAL="$(shasum -a 256 "${WORKDIR}/${ASSET}" | awk '{ print $1 }')"
[[ "$EXPECT" == "$ACTUAL" ]] || die "checksum mismatch for ${ASSET}"

mkdir -p "$BIN_DIR"
cp "${WORKDIR}/${ASSET}" "${BIN_DIR}/incodex"
chmod 755 "${BIN_DIR}/incodex"
ln -sfn incodex "${BIN_DIR}/inc"

echo "installed ${BIN_DIR}/incodex"
echo "alias     ${BIN_DIR}/inc"
echo "add ${BIN_DIR} to PATH if needed, then run: incodex --help"
