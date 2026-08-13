#!/usr/bin/env sh
# Install a prebuilt dothoard release binary.
#
# Usage:
#   curl -fsSL https://raw.githubusercontent.com/enriqTS/dothoard/main/scripts/install.sh | sh
#
# Environment overrides:
#   VERSION      Release tag to install, e.g. v1.1.0-alpha.1 (default: latest release)
#   INSTALL_DIR  Directory to install the binary into (default: $HOME/.local/bin)
#
# dothoard is experimental. Read the pre-release policy before trusting a
# release build with real data: https://enriqts.github.io/dothoard/releases/

set -eu

REPO="enriqTS/dothoard"
INSTALL_DIR="${INSTALL_DIR:-$HOME/.local/bin}"

log() { printf '==> %s\n' "$1"; }
die() {
    printf 'error: %s\n' "$1" >&2
    exit 1
}

os="$(uname -s)"
[ "$os" = "Linux" ] || die "dothoard only ships Linux binaries (detected: $os). Build from source instead."

arch="$(uname -m)"
case "$arch" in
    x86_64|amd64) target="x86_64-unknown-linux-gnu" ;;
    *) die "no prebuilt binary for architecture '$arch'. Build from source with 'cargo install --path .'." ;;
esac

command -v curl >/dev/null 2>&1 || die "curl is required"
command -v tar >/dev/null 2>&1 || die "tar is required"
command -v sha256sum >/dev/null 2>&1 || die "sha256sum is required"

if [ -z "${VERSION:-}" ]; then
    log "Looking up the latest release..."
    VERSION="$(curl -fsSL "https://api.github.com/repos/${REPO}/releases" \
        | grep -m1 '"tag_name"' \
        | sed -E 's/.*"tag_name": *"([^"]+)".*/\1/')"
    [ -n "$VERSION" ] || die "could not determine the latest release; set VERSION explicitly."
fi

asset="dothoard-${VERSION}-${target}.tar.gz"
base_url="https://github.com/${REPO}/releases/download/${VERSION}"

workdir="$(mktemp -d)"
trap 'rm -rf "$workdir"' EXIT INT TERM

log "Downloading ${asset} (${VERSION})..."
curl -fsSL -o "${workdir}/${asset}" "${base_url}/${asset}" \
    || die "download failed; check that ${VERSION} exists at https://github.com/${REPO}/releases"
curl -fsSL -o "${workdir}/${asset}.sha256" "${base_url}/${asset}.sha256" \
    || die "checksum download failed"

log "Verifying checksum..."
(cd "$workdir" && sha256sum -c "${asset}.sha256") >/dev/null \
    || die "checksum verification failed"

log "Extracting..."
tar -xzf "${workdir}/${asset}" -C "$workdir"

mkdir -p "$INSTALL_DIR"
install -m 755 "${workdir}/dothoard-${VERSION}-${target}/dothoard" "${INSTALL_DIR}/dothoard"

log "Installed dothoard ${VERSION} to ${INSTALL_DIR}/dothoard"

case ":$PATH:" in
    *":${INSTALL_DIR}:"*) ;;
    *)
        printf '\n'
        printf 'NOTE: %s is not in your PATH.\n' "$INSTALL_DIR"
        printf 'Add it with:  fish_add_path %s  (fish)\n' "$INSTALL_DIR"
        printf '         or:  export PATH="%s:$PATH"  (bash/zsh)\n' "$INSTALL_DIR"
        printf '\n'
        ;;
esac
