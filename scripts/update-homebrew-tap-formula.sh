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
    echo "Tag must be vX.Y.Z: $tag" >&2
    exit 1
fi

if [[ ! "$arm_sha" =~ ^[0-9a-f]{64}$ || ! "$x64_sha" =~ ^[0-9a-f]{64}$ ]]; then
    echo "Checksums must be 64 lowercase hex characters" >&2
    exit 1
fi

if [[ ! -f "$formula_path" ]]; then
    echo "Formula not found: $formula_path" >&2
    exit 1
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

if ! grep -q "version \"$version\"" "$formula_path"; then
    echo "Failed to patch formula version" >&2
    exit 1
fi
if ! grep -q "$arm_sha" "$formula_path"; then
    echo "Failed to patch arm sha256" >&2
    exit 1
fi
if ! grep -q "$x64_sha" "$formula_path"; then
    echo "Failed to patch x64 sha256" >&2
    exit 1
fi
