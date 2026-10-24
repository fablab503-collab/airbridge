#!/usr/bin/env bash
# ========================================================================================
# 🗑️ AIRBRIDGE UNINSTALLER
# ========================================================================================
set -e

TARGET_DIR="$HOME/Library/Application Support/Air Compute Bridge"
PLIST_FILE="$HOME/Library/LaunchAgents/com.danielmadac.air-compute-bridge.plist"
BIN_DIR="$HOME/.local/bin"
DOMAIN="gui/$(id -u)"
LABEL="com.danielmadac.air-compute-bridge"

echo "Stopping service..."
launchctl bootout "$DOMAIN/$LABEL" 2>/dev/null || true
launchctl disable "$DOMAIN/$LABEL" 2>/dev/null || true
pkill -f "air_bridge_daemon.py" 2>/dev/null || true
pkill -f "llama-server.*8086" 2>/dev/null || true

echo "Removing LaunchAgent..."
rm -f "$PLIST_FILE"

echo "Removing commands & files..."
rm -f "$BIN_DIR/air-bridge" "$BIN_DIR/air-bridge-update" "$BIN_DIR/mesh-status" "$BIN_DIR/python3"
rm -rf "$TARGET_DIR" "$HOME/Applications/AirBridge.app"

echo "✅ AirBridge has been completely removed from this Mac."
