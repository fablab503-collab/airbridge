#!/usr/bin/env bash
# ========================================================================================
# 🔄 MACLUSTER ONLINE UPDATER
# Downloads latest native Rust binary release and gracefully restarts daemon.
# ========================================================================================
set -e

REPO_RAW="https://raw.githubusercontent.com/fablab503-collab/airbridge/main"
TARGET_DIR="$HOME/Library/Application Support/MACLUSTER"
BIN_DIR="$HOME/.local/bin"
DOMAIN="gui/$(id -u)"
LABEL="com.danielmadac.macluster"

echo "========================================================================================"
echo "🔄  MACLUSTER ONLINE UPDATER"
echo "========================================================================================"

LATEST_JSON=$(curl -sSL "$REPO_RAW/version.json")
LATEST_VERSION=$(echo "$LATEST_JSON" | grep '"version"' | head -n 1 | awk -F '"' '{print $4}')
DOWNLOAD_URL=$(echo "$LATEST_JSON" | grep '"rust_binary_url"' | head -n 1 | awk -F '"' '{print $4}')

echo "Checking online updates..."
echo "Online Version: v$LATEST_VERSION"

if [ -z "$DOWNLOAD_URL" ]; then
    echo "❌ Failed to retrieve download URL from GitHub."
    exit 1
fi

echo "Downloading update..."
curl -sSL "$DOWNLOAD_URL" | tar -xz -C "$TARGET_DIR"
chmod +x "$TARGET_DIR/macluster"
ln -sf "$TARGET_DIR/macluster" "$BIN_DIR/macluster"
ln -sf "$TARGET_DIR/macluster" "$BIN_DIR/airbridge"

echo "Restarting service..."
launchctl kickstart -k "$DOMAIN/$LABEL" 2>/dev/null || true

sleep 1
echo ""
echo "========================================================================================"
echo "✅ MACLUSTER UPDATED TO v$LATEST_VERSION SUCCESSFULLY!"
echo "========================================================================================"
"$BIN_DIR/macluster" status || true
