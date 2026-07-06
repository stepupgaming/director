#!/usr/bin/env bash
set -e

VENDOR_DIR="$(cd "$(dirname "$0")/.." && pwd)/vendor/skia-safe"
PATCHES_DIR="$(cd "$(dirname "$0")" && pwd)"

if [ ! -f "$VENDOR_DIR/Cargo.toml" ]; then
    echo "vendor/skia-safe not found. Run cargo vendor first or ensure the directory exists."
    exit 1
fi

for patch in "$PATCHES_DIR"/*.patch; do
    if [ -f "$patch" ]; then
        echo "Applying $(basename "$patch") ..."
        (cd "$VENDOR_DIR" && git apply --ignore-whitespace "$patch" 2>/dev/null) || \
        (cd "$VENDOR_DIR" && patch -p1 -i "$patch")
    fi
done

echo "Done. Patches applied to vendor/skia-safe."
