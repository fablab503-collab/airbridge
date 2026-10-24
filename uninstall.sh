#!/usr/bin/env bash
# ========================================================================================
# 🗑️ MACLUSTER 1-CLICK UNINSTALLER
# Completely removes all traces of MACLUSTER from this Mac.
# ========================================================================================
set -e

DOMAIN="gui/$(id -u)"
TARGET_DIR="$HOME/Library/Application Support/MACLUSTER"
OLD_TARGET="$HOME/Library/Application Support/Air Compute Bridge"
PLIST_FILE="$HOME/Library/LaunchAgents/com.danielmadac.macluster.plist"
OLD_PLIST="$HOME/Library/LaunchAgents/com.danielmadac.air-compute-bridge.plist"
LOG_FILE="$HOME/Library/Logs/macluster.log"
OLD_LOG="$HOME/Library/Logs/air-compute-bridge.log"

echo "========================================================================================"
echo "🗑️  MACLUSTER — 1-CLICK UNINSTALLER"
echo "========================================================================================"
echo "Stopping background processes and removing all files..."

# Unload daemons
launchctl bootout "$DOMAIN" "$PLIST_FILE" 2>/dev/null || true
launchctl bootout "$DOMAIN/com.danielmadac.macluster" 2>/dev/null || true
launchctl bootout "$DOMAIN" "$OLD_PLIST" 2>/dev/null || true
launchctl bootout "$DOMAIN/com.danielmadac.air-compute-bridge" 2>/dev/null || true

# Kill any leftover model processes
pkill -f "macluster" 2>/dev/null || true
pkill -f "llama-server" 2>/dev/null || true
pkill -f "airbridge" 2>/dev/null || true

# Remove binaries and symlinks
rm -f "$HOME/.local/bin/macluster"
rm -f "$HOME/.local/bin/airbridge"
rm -f "$HOME/.local/bin/air-bridge"

# Remove Plists
rm -f "$PLIST_FILE" "$OLD_PLIST"

# Remove Application Support folders
rm -rf "$TARGET_DIR" "$OLD_TARGET"

# Remove Logs
rm -f "$LOG_FILE" "$OLD_LOG"

echo ""
echo "========================================================================================"
echo "✅ MACLUSTER COMPLETELY REMOVED FROM THIS SYSTEM"
echo "========================================================================================"
