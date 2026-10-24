#!/usr/bin/env bash
# ========================================================================================
# 🔄 AIRBRIDGE ONLINE RUST UPDATER
# ========================================================================================
set -e

RELEASE_URL="https://github.com/fablab503-collab/airbridge/releases/download/v1.5.0"
TARGET_DIR="$HOME/Library/Application Support/Air Compute Bridge"
BIN_DIR="$HOME/.local/bin"
DOMAIN="gui/$(id -u)"
LABEL="com.danielmadac.air-compute-bridge"

clear
echo "========================================================================================"
echo "🔄  AIRBRIDGE — ONLINE RUST UPDATER"
echo "========================================================================================"
echo "Downloading latest native Rust binary from GitHub (1.1 MB)..."
echo ""

mkdir -p "$TARGET_DIR" "$BIN_DIR"
curl -sSL "$RELEASE_URL/airbridge-aarch64.tar.gz" | tar -xz -C "$TARGET_DIR"
chmod +x "$TARGET_DIR/airbridge"
ln -sf "$TARGET_DIR/airbridge" "$BIN_DIR/airbridge"
ln -sf "$TARGET_DIR/airbridge" "$BIN_DIR/air-bridge"
xattr -dr com.apple.quarantine "$TARGET_DIR/airbridge" 2>/dev/null || true

echo "Restarting service..."
launchctl kickstart -k "$DOMAIN/$LABEL" 2>/dev/null || true

echo "✅ AirBridge updated to latest Rust release (v1.5.0)!"
read -t 5 -p "Closing in 5 seconds..." dummy || true
exit 0
