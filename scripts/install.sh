#!/bin/sh
# md-viewer installer: downloads the latest prebuilt binary for your platform,
# verifies its SHA256, and installs to ~/.local/bin (override with INSTALL_DIR).
# Bundled third-party notices are installed under the matching prefix.
#
# Usage:
#   curl -fsSL https://raw.githubusercontent.com/aydiler/md-viewer/main/scripts/install.sh | sh
#   INSTALL_DIR=/usr/local/bin sh install.sh
#
# Supported: Linux x86_64, macOS x86_64, macOS arm64.
# Windows: download the .zip from https://github.com/aydiler/md-viewer/releases

set -eu

REPO="aydiler/md-viewer"
INSTALL_DIR="${INSTALL_DIR:-$HOME/.local/bin}"

err() { printf 'error: %s\n' "$1" >&2; exit 1; }
info() { printf '%s\n' "$1"; }

if [ "${1:-}" = "--help" ] || [ "${1:-}" = "-h" ]; then
    cat <<EOF
md-viewer installer

Usage:
    sh install.sh                           # install to \$HOME/.local/bin
    INSTALL_DIR=/usr/local/bin sh install.sh # install to a custom location

Environment:
    INSTALL_DIR  Target directory (default: \$HOME/.local/bin)

Detects platform via uname; downloads the matching tarball from the latest
GitHub release at https://github.com/$REPO/releases/latest and verifies its
SHA256 before installing.
EOF
    exit 0
fi

# --- detect platform ---------------------------------------------------------
OS="$(uname -s)"
ARCH="$(uname -m)"

case "$OS" in
    Linux)
        case "$ARCH" in
            x86_64|amd64) ASSET_NAME="linux-x86_64" ;;
            *) err "unsupported Linux architecture: $ARCH (only x86_64 is built)" ;;
        esac
        ;;
    Darwin)
        case "$ARCH" in
            arm64|aarch64) ASSET_NAME="macos-arm64" ;;
            x86_64) err "Intel Macs are not currently built. Build from source: cargo install md-viewer" ;;
            *) err "unsupported macOS architecture: $ARCH" ;;
        esac
        ;;
    *)
        err "unsupported OS: $OS (Windows users: download the .zip from https://github.com/$REPO/releases)"
        ;;
esac

# --- check required tools ----------------------------------------------------
for tool in curl tar install; do
    command -v "$tool" >/dev/null 2>&1 || err "$tool is required but not installed"
done

# sha256 verification helper
if command -v sha256sum >/dev/null 2>&1; then
    sha256_check() { sha256sum -c "$1" >/dev/null 2>&1; }
elif command -v shasum >/dev/null 2>&1; then
    sha256_check() { shasum -a 256 -c "$1" >/dev/null 2>&1; }
else
    err "sha256sum or shasum is required for checksum verification"
fi

# --- fetch latest tag --------------------------------------------------------
info "Fetching latest release info..."
TAG="$(curl -fsSL "https://api.github.com/repos/$REPO/releases/latest" \
    | grep -o '"tag_name": *"[^"]*"' \
    | head -1 \
    | sed 's/.*"tag_name": *"\([^"]*\)".*/\1/')"

[ -n "$TAG" ] || err "could not determine latest release tag"
VERSION="${TAG#v}"
info "Latest version: $TAG"

ASSET="md-viewer-${VERSION}-${ASSET_NAME}.tar.gz"
BASE_URL="https://github.com/$REPO/releases/download/$TAG"

# --- download into temp ------------------------------------------------------
TMPDIR="$(mktemp -d)"
STAGED_BINARY=""
STAGED_NOTICE=""
cleanup() {
    rm -rf "$TMPDIR"
    [ -z "$STAGED_BINARY" ] || rm -f "$STAGED_BINARY"
    [ -z "$STAGED_NOTICE" ] || rm -f "$STAGED_NOTICE"
}
trap cleanup EXIT
trap 'exit 1' HUP INT TERM

info "Downloading $ASSET..."
curl -fsSL -o "$TMPDIR/$ASSET" "$BASE_URL/$ASSET"
curl -fsSL -o "$TMPDIR/$ASSET.sha256" "$BASE_URL/$ASSET.sha256"

info "Verifying checksum..."
(cd "$TMPDIR" && sha256_check "$ASSET.sha256") \
    || err "SHA256 checksum verification failed"

# --- install -----------------------------------------------------------------
# Derive the data prefix from the binary directory so custom INSTALL_DIR values
# keep their license material in the same installation prefix.
case "$INSTALL_DIR" in
    */bin) INSTALL_PREFIX="${INSTALL_DIR%/bin}" ;;
    *) INSTALL_PREFIX="${INSTALL_DIR%/}" ;;
esac
NOTICE_DIR="$INSTALL_PREFIX/share/licenses/md-viewer"

info "Installing to $INSTALL_DIR..."
mkdir -p "$INSTALL_DIR" "$NOTICE_DIR"
tar -xzf "$TMPDIR/$ASSET" -C "$TMPDIR"
[ -f "$TMPDIR/md-viewer" ] || err "release archive is missing md-viewer"
[ -f "$TMPDIR/THIRD_PARTY_NOTICES" ] \
    || err "release archive is missing THIRD_PARTY_NOTICES"

# Stage inside each destination directory, then rename atomically over old files.
STAGED_BINARY="$INSTALL_DIR/.md-viewer.new.$$"
STAGED_NOTICE="$NOTICE_DIR/.THIRD_PARTY_NOTICES.new.$$"
rm -f "$STAGED_BINARY" "$STAGED_NOTICE"
install -m 0755 "$TMPDIR/md-viewer" "$STAGED_BINARY"
install -m 0644 "$TMPDIR/THIRD_PARTY_NOTICES" "$STAGED_NOTICE"
mv -f "$STAGED_BINARY" "$INSTALL_DIR/md-viewer"
STAGED_BINARY=""
mv -f "$STAGED_NOTICE" "$NOTICE_DIR/THIRD_PARTY_NOTICES"
STAGED_NOTICE=""

# Desktop integration (Linux only)
if [ "$OS" = "Linux" ]; then
    DESKTOP_DIR="$HOME/.local/share/applications"
    mkdir -p "$DESKTOP_DIR"
    cat > "$DESKTOP_DIR/md-viewer.desktop" <<'EOF'
[Desktop Entry]
Name=Markdown Viewer
Comment=View markdown files with live reload
Exec=md-viewer %f
Icon=text-markdown
Terminal=false
Type=Application
Categories=Utility;TextEditor;Viewer;
MimeType=text/markdown;text/x-markdown;
StartupNotify=false
StartupWMClass=md-viewer
EOF
    command -v update-desktop-database >/dev/null 2>&1 \
        && update-desktop-database "$DESKTOP_DIR" 2>/dev/null || true
fi

# macOS Gatekeeper quarantine: unsigned binaries downloaded via curl don't get
# quarantined, but if a user manually drags the binary they may need:
#   xattr -d com.apple.quarantine ~/.local/bin/md-viewer
# Print a hint just in case.
if [ "$OS" = "Darwin" ]; then
    info ""
    info "Note: if macOS refuses to run the binary with a Gatekeeper warning, run:"
    info "    xattr -d com.apple.quarantine \"$INSTALL_DIR/md-viewer\""
fi

# --- PATH check --------------------------------------------------------------
case ":$PATH:" in
    *":$INSTALL_DIR:"*) PATH_OK=1 ;;
    *) PATH_OK=0 ;;
esac

info ""
info "md-viewer $TAG installed to $INSTALL_DIR/md-viewer"
if [ "$PATH_OK" = "0" ]; then
    info ""
    info "Warning: $INSTALL_DIR is not on your PATH."
    info "Add this to your shell profile (~/.bashrc, ~/.zshrc, etc.):"
    info "    export PATH=\"$INSTALL_DIR:\$PATH\""
fi
