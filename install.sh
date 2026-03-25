#!/usr/bin/env bash
set -euo pipefail

REPO="SpeedHQ/udp-forwarder"
INSTALL_DIR="${INSTALL_DIR:-$HOME/.local/bin}"

# Detect platform
OS=$(uname -s)
ARCH=$(uname -m)

case "$OS-$ARCH" in
  Darwin-arm64)  TARGET="aarch64-apple-darwin" ;;
  Linux-x86_64)  TARGET="x86_64-unknown-linux-gnu" ;;
  *)
    echo "Unsupported platform: $OS $ARCH"
    echo "Supported: macOS (Apple Silicon), Linux (x86_64)"
    exit 1
    ;;
esac

# Get latest version
echo "Fetching latest release..."
VERSION=$(curl -sL "https://api.github.com/repos/$REPO/releases/latest" | grep '"tag_name"' | head -1 | sed 's/.*"v\(.*\)".*/\1/')

if [ -z "$VERSION" ]; then
  echo "Failed to fetch latest version. Check https://github.com/$REPO/releases"
  exit 1
fi

FILENAME="udp-forwarder-v${VERSION}-${TARGET}.zip"
URL="https://github.com/$REPO/releases/download/v${VERSION}/${FILENAME}"

# Download and extract
TMP=$(mktemp -d)
trap 'rm -rf "$TMP"' EXIT

echo "Downloading udp-forwarder v${VERSION} for ${TARGET}..."
curl -sL "$URL" -o "$TMP/$FILENAME"

unzip -qo "$TMP/$FILENAME" -d "$TMP"

# Install binary
mkdir -p "$INSTALL_DIR"
mv "$TMP/udp-forwarder" "$INSTALL_DIR/udp-forwarder"
chmod +x "$INSTALL_DIR/udp-forwarder"

# macOS: remove quarantine
if [ "$OS" = "Darwin" ]; then
  xattr -d com.apple.quarantine "$INSTALL_DIR/udp-forwarder" 2>/dev/null || true
fi

# Copy default config to current directory if none exists
if [ ! -f config.ini ]; then
  cp "$TMP/config.ini" ./config.ini
  echo "Created config.ini in current directory — edit it before running."
fi

echo ""
echo "Installed udp-forwarder v${VERSION} to $INSTALL_DIR/udp-forwarder"

# Check if install dir is in PATH
if ! echo "$PATH" | tr ':' '\n' | grep -qx "$INSTALL_DIR"; then
  echo ""
  echo "Add to your PATH:"
  echo "  export PATH=\"$INSTALL_DIR:\$PATH\""
fi
