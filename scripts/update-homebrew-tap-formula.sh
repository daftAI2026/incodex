#!/usr/bin/env bash

set -euo pipefail

usage() {
  cat << 'EOF'
Usage:
  update-homebrew-tap-formula.sh \
    --formula /path/to/Formula/incodex.rb \
    --tag v0.1.0 \
    --arm-sha <sha256> \
    --x64-sha <sha256>
EOF
}

die() {
  echo "$1" >&2
  exit 1
}

assert_formula_contains() {
  local expected="$1"
  local failure_message="$2"
  grep -q "$expected" "$formula_path" || die "$failure_message"
}

formula_path=""
tag=""
arm_sha=""
x64_sha=""

while [[ $# -gt 0 ]]; do
  case "$1" in
    --formula)
      formula_path="${2:-}"
      shift 2
      ;;
    --tag)
      tag="${2:-}"
      shift 2
      ;;
    --arm-sha)
      arm_sha="${2:-}"
      shift 2
      ;;
    --x64-sha)
      x64_sha="${2:-}"
      shift 2
      ;;
    -h | --help)
      usage
      exit 0
      ;;
    *)
      echo "Unknown argument: $1" >&2
      usage >&2
      exit 1
      ;;
  esac
done

if [[ -z "$formula_path" || -z "$tag" || -z "$arm_sha" || -z "$x64_sha" ]]; then
  usage >&2
  exit 1
fi

if [[ ! "$tag" =~ ^v[0-9]+\.[0-9]+\.[0-9]+$ ]]; then
  die "Tag must be vX.Y.Z: $tag"
fi

if [[ ! "$arm_sha" =~ ^[0-9a-f]{64}$ || ! "$x64_sha" =~ ^[0-9a-f]{64}$ ]]; then
  die "Checksums must be 64 lowercase hex characters"
fi

if [[ ! -f "$formula_path" ]]; then
  die "Formula not found: $formula_path"
fi

version="${tag#v}"

VERSION="$version" \
  ARM_SHA="$arm_sha" \
  X64_SHA="$x64_sha" \
  perl -0pi -e '
    s{version "[^"]+"}{version "$ENV{VERSION}"};

    s{(if Hardware::CPU\.arm\?\s+url "[^"]+"\s+sha256 ")[^"]+(")}{$1$ENV{ARM_SHA}$2}s;

    s{(elsif Hardware::CPU\.intel\?\s+url "[^"]+"\s+sha256 ")[^"]+(")}{$1$ENV{X64_SHA}$2}s;
' "$formula_path"

assert_formula_contains "version \"$version\"" "Failed to patch formula version"
assert_formula_contains "$arm_sha" "Failed to patch arm sha256"
assert_formula_contains "$x64_sha" "Failed to patch x64 sha256"
