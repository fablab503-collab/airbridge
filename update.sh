#!/usr/bin/env bash
# ========================================================================================
# 🔄 AIRBRIDGE ONLINE UPDATER
# Pulls the latest implementation directly from GitHub over HTTPS.
# Zero USB required. Zero network drive mounting required.
# ========================================================================================
set -e

REPO_RAW="https://raw.githubusercontent.com/fablab503-collab/airbridge/main"
TARGET_DIR="$HOME/Library/Application Support/Air Compute Bridge"
BIN_DIR="$HOME/.local/bin"
DOMAIN="gui/$(id -u)"
LABEL="com.danielmadac.air-compute-bridge"

clear
echo "========================================================================================"
echo "🔄  AIRBRIDGE — ONLINE UPDATER"
echo "========================================================================================"
echo "Checking GitHub for the latest release (HTTPS)..."
echo ""

mkdir -p "$TARGET_DIR" "$TARGET_DIR/models" "$TARGET_DIR/queue" "$BIN_DIR"

# 1. Fetch latest version metadata
echo "[1/4] 🌐 Checking online version..."
ONLINE_VER=$(curl -sSL "$REPO_RAW/version.json" | grep '"version"' | cut -d'"' -f4 || echo "latest")
echo "      Found latest release: v$ONLINE_VER"

# 2. Update core scripts online
echo ""
echo "[2/4] 📥 Downloading latest daemon & telemetry monitor..."
curl -sSL "$REPO_RAW/air_bridge_daemon.py" -o "$TARGET_DIR/air_bridge_daemon.py"
curl -sSL "$REPO_RAW/mesh_monitor.py" -o "$TARGET_DIR/mesh_monitor.py"
curl -sSL "$REPO_RAW/Air%20Compute%20Bridge" -o "$TARGET_DIR/Air Compute Bridge"
curl -sSL "$REPO_RAW/air_dispatch.py" -o "$TARGET_DIR/air_dispatch.py"
curl -sSL "$REPO_RAW/uninstall.sh" -o "$TARGET_DIR/uninstall.sh"
curl -sSL "$REPO_RAW/update.sh" -o "$TARGET_DIR/update.sh"
chmod +x "$TARGET_DIR/air_bridge_daemon.py" "$TARGET_DIR/mesh_monitor.py" "$TARGET_DIR/Air Compute Bridge" "$TARGET_DIR/update.sh" "$TARGET_DIR/uninstall.sh"

# 3. Verify Python Runtime
echo ""
echo "[3/4] 🐍 Verifying standalone Python..."
PYTHON=""
if [ -x "$TARGET_DIR/python/bin/python3" ]; then
    PYTHON="$TARGET_DIR/python/bin/python3"
elif [ -x "/opt/homebrew/bin/python3" ]; then
    PYTHON="/opt/homebrew/bin/python3"
elif command -v python3 >/dev/null 2>&1 && xcode-select -p >/dev/null 2>&1 && python3 -c "import sys" 2>/dev/null; then
    PYTHON="$(command -v python3)"
else
    echo "      Downloading standalone Apple Silicon Python 3.12 (~15 MB)..."
    curl -sSL "https://github.com/astral-sh/python-build-standalone/releases/download/20241016/cpython-3.12.7+20241016-aarch64-apple-darwin-install_only.tar.gz" | tar -xz -C "$TARGET_DIR"
    PYTHON="$TARGET_DIR/python/bin/python3"
fi

# Verify Metal inference engine
if [ ! -f "$TARGET_DIR/bin/llama-server" ]; then
    echo "      Downloading standalone Apple Silicon Metal engine (~9.4 MB)..."
    curl -sSL "https://github.com/fablab503-collab/airbridge/releases/download/v1.4.0/metal-engine-aarch64.tar.gz" | tar -xz -C "$TARGET_DIR"
fi
chmod +x "$TARGET_DIR/bin/llama-server" 2>/dev/null || true

# Strip quarantine
xattr -dr com.apple.quarantine "$TARGET_DIR" 2>/dev/null || true

# 4. Graceful Restart
echo ""
echo "[4/4] 🚀 Activating latest version..."
launchctl kickstart -k "$DOMAIN/$LABEL" 2>/dev/null || true

# Verification
UPDATED=false
for i in {1..6}; do
    if curl -s "http://127.0.0.1:8085/health" >/dev/null 2>&1; then
        UPDATED=true
        break
    fi
    sleep 1
done

echo ""
echo "========================================================================================"
if [ "$UPDATED" = "true" ]; then
    echo "🎉  AIRBRIDGE ONLINE UPDATE COMPLETE (v$ONLINE_VER)!"
else
    echo "✅  ONLINE UPDATE INSTALLED!"
fi
echo "========================================================================================"
echo "✨ Features active:"
echo "  - 😴 Zero-Power Lid-Closed Standby (AppleClamshellState detection)"
echo "  - ☀️ Instant Lid-Open Auto-Revive"
echo "  - 🧠 10-Minute User Priority Circuit-Breaker (<2GB free RAM threshold)"
echo "  - ❄️ Adaptive Whisper Thermal Governor (8s pauses)"
echo "  - 🌐 Online-Only Auto-Updating (zero USB needed)"
echo "========================================================================================"
echo ""
read -t 6 -p "Closing in 6 seconds..." dummy || true
exit 0
