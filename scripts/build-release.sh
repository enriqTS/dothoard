#!/usr/bin/env bash
# Build a release binary for dothoard.
#
# Usage:
#   ./scripts/build-release.sh
#
# Output:
#   target/release/dothoard — optimized, stripped, LTO-enabled binary
#
# The binary is statically linked against system libraries and suitable
# for distribution on any x86_64 Linux system with glibc.

set -euo pipefail

echo "==> Running quality checks..."
cargo fmt --check
cargo clippy --all-targets --all-features -- -D warnings

echo "==> Running tests..."
cargo test --all-targets --all-features -- --test-threads=1

echo "==> Building release binary..."
cargo build --release

BINARY="target/release/dothoard"
VERSION=$("$BINARY" --version | awk '{print $2}')
ARCH=$(uname -m)
SIZE=$(du -h "$BINARY" | cut -f1)

echo ""
echo "==> Release build complete"
echo "    Binary:  $BINARY"
echo "    Version: $VERSION"
echo "    Arch:    $ARCH"
echo "    Size:    $SIZE"
echo ""
echo "Install with:"
echo "    cp $BINARY ~/.local/bin/"
echo ""
echo "Or system-wide:"
echo "    sudo cp $BINARY /usr/local/bin/"
