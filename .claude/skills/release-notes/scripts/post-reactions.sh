#!/bin/bash
# Add the standard six reactions (+1, laugh, hooray, heart, rocket, eyes) to an
# Incodex GitHub Release. Usage: post-reactions.sh v<version>

set -euo pipefail

TAG="${1:-}"
if [[ -z "$TAG" ]]; then
    echo "Usage: $0 v<version>" >&2
    exit 1
fi

if [[ "$TAG" != v* ]]; then
    echo "Tag must start with lowercase v (release.yml ignores capital V): $TAG" >&2
    exit 1
fi

if ! command -v gh > /dev/null 2>&1; then
    echo "gh CLI is required" >&2
    exit 1
fi

REPO="${INCODEX_REPO:-daftAI2026/incodex}"
RELEASE_ID=$(gh api "repos/${REPO}/releases/tags/$TAG" --jq '.id')
if [[ -z "$RELEASE_ID" ]]; then
    echo "Release not found for tag: $TAG" >&2
    exit 1
fi

for r in +1 laugh hooray heart rocket eyes; do
    gh api "repos/${REPO}/releases/$RELEASE_ID/reactions" \
        -X POST -f content="$r" --silent
done

echo "Posted 6 reactions to $TAG (release id $RELEASE_ID)"
