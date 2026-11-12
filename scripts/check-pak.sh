#!/bin/sh
# Copyright 2026 the Brick Controller contributors
# SPDX-License-Identifier: GPL-3.0-only
# Check pak.json and the Pak Store archive before a release.
#
#   ./scripts/check-pak.sh v1.0.0
#
# Run package.sh first. This script only checks the metadata and archive layout
# the Pak Store reads; it does not replace the firmware tests.

set -eu

ROOT=$(cd "$(dirname "$0")/.." && pwd)
cd "$ROOT"

PAK_JSON="$ROOT/pak.json"
VERSION="${1:-}"

command -v jq >/dev/null 2>&1 || {
    echo "jq not found; install it to validate pak.json" >&2
    exit 1
}
command -v unzip >/dev/null 2>&1 || {
    echo "unzip not found; install it to inspect the archive" >&2
    exit 1
}
[ -f "$PAK_JSON" ] || {
    echo "missing pak.json at the repository root" >&2
    exit 1
}

for key in name version type description author repo_url release_filename; do
    value=$(jq -r --arg key "$key" '.[$key] // ""' "$PAK_JSON")
    [ -n "$value" ] || {
        echo "pak.json: missing or empty '$key'" >&2
        exit 1
    }
done

PAK_NAME=$(jq -r '.name' "$PAK_JSON")
PAK_VERSION=$(jq -r '.version' "$PAK_JSON")
PAK_TYPE=$(jq -r '.type' "$PAK_JSON")
RELEASE_FILENAME=$(jq -r '.release_filename' "$PAK_JSON")
REPO_URL=$(jq -r '.repo_url' "$PAK_JSON")

[ "$PAK_TYPE" = "TOOL" ] || {
    echo "pak.json: type must be TOOL, got '$PAK_TYPE'" >&2
    exit 1
}
[ "$PAK_NAME" = "Brick Controller" ] || {
    echo "pak.json: name must be 'Brick Controller', got '$PAK_NAME'" >&2
    exit 1
}
case "$PAK_VERSION" in
    v[0-9]*) ;;
    *)
        echo "pak.json: version must start with v, for example v1.0.0" >&2
        exit 1
        ;;
esac
case "$RELEASE_FILENAME" in
    *" "*)
        echo "pak.json: release_filename must not contain spaces" >&2
        exit 1
        ;;
esac
case "$REPO_URL" in
    https://github.com/*) ;;
    *)
        echo "pak.json: repo_url must be a GitHub URL" >&2
        exit 1
        ;;
esac
jq -e '.platforms | index("tg5040") != null' "$PAK_JSON" >/dev/null || {
    echo "pak.json: platforms must include tg5040" >&2
    exit 1
}
if [ -n "$VERSION" ] && [ "$PAK_VERSION" != "$VERSION" ]; then
    echo "pak.json: version '$PAK_VERSION' does not match '$VERSION'" >&2
    exit 1
fi
jq -e --arg version "$PAK_VERSION" \
    '.changelog[$version] | (type == "string" and length > 0)' \
    "$PAK_JSON" >/dev/null || {
    echo "pak.json: changelog has no entry for $PAK_VERSION" >&2
    exit 1
}

for screenshot in $(jq -r '.screenshots[]?' "$PAK_JSON"); do
    [ -f "$ROOT/$screenshot" ] || {
        echo "missing screenshot: $screenshot" >&2
        exit 1
    }
done

ARCHIVE="$ROOT/dist/$RELEASE_FILENAME"
[ -f "$ARCHIVE" ] || {
    echo "missing archive: dist/$RELEASE_FILENAME" >&2
    echo "run VERSION=$PAK_VERSION ./apps/brick-controller/deploy/package.sh first" >&2
    exit 1
}

ENTRIES=$(unzip -Z1 "$ARCHIVE")
for file in launch.sh brick-controller icon.png README.txt LICENSE \
    ADDITIONAL-TERMS.md CREDITS.md OFL.txt; do
    printf '%s\n' "$ENTRIES" | grep -Fqx "$file" || {
        echo "archive: missing '$file' at the root" >&2
        exit 1
    }
done

if printf '%s\n' "$ENTRIES" | grep -Eq '^[^/]+\.pak/'; then
    echo "archive: contains a top-level .pak folder; the Pak Store would nest it" >&2
    exit 1
fi
if printf '%s\n' "$ENTRIES" | grep -Eq '(^|/)__MACOSX/|(^|/)\.DS_Store$|(^|/)brick-controller\.next$'; then
    echo "archive: contains a macOS or development artifact" >&2
    exit 1
fi
if printf '%s\n' "$ENTRIES" | grep -Eq '^(dist|target)/'; then
    echo "archive: contains build output" >&2
    exit 1
fi

printf 'pak.json: %s %s (%s)\n' "$PAK_NAME" "$PAK_VERSION" "$PAK_TYPE"
printf 'archive: dist/%s\n' "$RELEASE_FILENAME"
printf 'check: OK\n'
