#!/bin/bash
set -euo pipefail

# Package UDP Forwarder as a macOS .app bundle and .dmg
# Usage: ./package-macos.sh [--sign]

APP_NAME="UDP Forwarder"
BUNDLE_ID="com.speedhq.udp-forwarder"
VERSION=$(grep '^version' Cargo.toml | head -1 | sed 's/.*"\(.*\)"/\1/')
BINARY="udp-forwarder"
SIGN="${1:-}"

echo "Building release binary..."
cargo build --release --target aarch64-apple-darwin

# Find the generated app icon PNG from build
OUT_DIR=$(find target/aarch64-apple-darwin/release/build/udp-forwarder-*/out -name "app_icon.png" -print -quit 2>/dev/null)
if [ -z "$OUT_DIR" ]; then
    echo "Error: app_icon.png not found in build output. Run cargo build first."
    exit 1
fi

echo "Creating .icns from generated icon..."
ICONSET_DIR=$(mktemp -d)/AppIcon.iconset
mkdir -p "$ICONSET_DIR"

# Generate all required icon sizes from our 256x256 source
sips -z 16 16     "$OUT_DIR" --out "$ICONSET_DIR/icon_16x16.png"      > /dev/null
sips -z 32 32     "$OUT_DIR" --out "$ICONSET_DIR/icon_16x16@2x.png"   > /dev/null
sips -z 32 32     "$OUT_DIR" --out "$ICONSET_DIR/icon_32x32.png"      > /dev/null
sips -z 64 64     "$OUT_DIR" --out "$ICONSET_DIR/icon_32x32@2x.png"   > /dev/null
sips -z 128 128   "$OUT_DIR" --out "$ICONSET_DIR/icon_128x128.png"    > /dev/null
sips -z 256 256   "$OUT_DIR" --out "$ICONSET_DIR/icon_128x128@2x.png" > /dev/null
sips -z 256 256   "$OUT_DIR" --out "$ICONSET_DIR/icon_256x256.png"    > /dev/null

ICNS_PATH="target/AppIcon.icns"
iconutil -c icns "$ICONSET_DIR" -o "$ICNS_PATH"
rm -rf "$(dirname "$ICONSET_DIR")"

echo "Creating .app bundle..."
APP_DIR="target/${APP_NAME}.app"
rm -rf "$APP_DIR"
mkdir -p "$APP_DIR/Contents/MacOS"
mkdir -p "$APP_DIR/Contents/Resources"

# Copy binary
cp "target/aarch64-apple-darwin/release/$BINARY" "$APP_DIR/Contents/MacOS/$BINARY"

# Copy icon
cp "$ICNS_PATH" "$APP_DIR/Contents/Resources/AppIcon.icns"

# Copy config example
cp config.ini.example "$APP_DIR/Contents/Resources/config.ini"

# Create Info.plist
cat > "$APP_DIR/Contents/Info.plist" << PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>CFBundleName</key>
    <string>${APP_NAME}</string>
    <key>CFBundleDisplayName</key>
    <string>${APP_NAME}</string>
    <key>CFBundleIdentifier</key>
    <string>${BUNDLE_ID}</string>
    <key>CFBundleVersion</key>
    <string>${VERSION}</string>
    <key>CFBundleShortVersionString</key>
    <string>${VERSION}</string>
    <key>CFBundleExecutable</key>
    <string>${BINARY}</string>
    <key>CFBundleIconFile</key>
    <string>AppIcon</string>
    <key>CFBundlePackageType</key>
    <string>APPL</string>
    <key>LSMinimumSystemVersion</key>
    <string>11.0</string>
    <key>LSUIElement</key>
    <false/>
    <key>NSHighResolutionCapable</key>
    <true/>
</dict>
</plist>
PLIST

# Code signing and notarization
if [ "$SIGN" = "--sign" ]; then
    # Requires these environment variables:
    #   APPLE_TEAM_ID        — e.g. "XXXXXXXXXX"
    #   APPLE_ID             — your Apple ID email
    #   APPLE_ID_PASSWORD    — app-specific password
    #
    # And a "Developer ID Application: <TEAM_ID>" identity in your keychain.
    # Install your .p12 certificate via Keychain Access if not already done.

    : "${APPLE_TEAM_ID:?Set APPLE_TEAM_ID}"
    : "${APPLE_ID:?Set APPLE_ID}"
    : "${APPLE_ID_PASSWORD:?Set APPLE_ID_PASSWORD}"

    echo "Signing .app bundle..."
    codesign --force --deep --options runtime \
        --sign "Developer ID Application: ${APPLE_TEAM_ID}" \
        "$APP_DIR"
    codesign --verify --verbose "$APP_DIR"
    echo "Signature verified."
fi

echo "Creating .dmg..."
DMG_NAME="UDP-Forwarder-v${VERSION}-macOS"
DMG_PATH="target/${DMG_NAME}.dmg"
rm -f "$DMG_PATH"

# Create DMG with Applications symlink
DMG_STAGING="target/dmg-staging"
rm -rf "$DMG_STAGING"
mkdir -p "$DMG_STAGING"
cp -R "$APP_DIR" "$DMG_STAGING/"
ln -s /Applications "$DMG_STAGING/Applications"

hdiutil create -volname "$APP_NAME" \
    -srcfolder "$DMG_STAGING" \
    -ov -format UDZO \
    "$DMG_PATH"

rm -rf "$DMG_STAGING"

# Notarize the DMG
if [ "$SIGN" = "--sign" ]; then
    echo "Notarizing .dmg..."
    xcrun notarytool submit "$DMG_PATH" \
        --apple-id "$APPLE_ID" \
        --team-id "$APPLE_TEAM_ID" \
        --password "$APPLE_ID_PASSWORD" \
        --wait

    echo "Stapling notarization ticket..."
    xcrun stapler staple "$DMG_PATH"
    echo "Notarization complete."
fi

echo ""
echo "Done!"
echo "  App: $APP_DIR"
echo "  DMG: $DMG_PATH"
