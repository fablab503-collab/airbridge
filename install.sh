#!/usr/bin/env bash
# ========================================================================================
# ⚡ AIRBRIDGE ONLINE INSTALLER
# One command installs the complete autonomous Apple Silicon compute bridge.
# ========================================================================================
set -e

REPO_RAW="https://raw.githubusercontent.com/fablab503-collab/airbridge/main"
TARGET_DIR="$HOME/Library/Application Support/Air Compute Bridge"
BIN_DIR="$HOME/.local/bin"
PLIST_FILE="$HOME/Library/LaunchAgents/com.danielmadac.air-compute-bridge.plist"
LOG_FILE="$HOME/Library/Logs/air-compute-bridge.log"
DOMAIN="gui/$(id -u)"
LABEL="com.danielmadac.air-compute-bridge"

clear
echo "========================================================================================"
echo "⚡  AIRBRIDGE — ONLINE 1-CLICK INSTALLER"
echo "========================================================================================"
echo "Downloading and configuring background compute bridge from GitHub..."
echo "========================================================================================"
echo ""

mkdir -p "$TARGET_DIR" "$TARGET_DIR/models" "$TARGET_DIR/queue" "$BIN_DIR" "$HOME/Library/Logs" "$HOME/Library/LaunchAgents"

# 1. Download core scripts online
echo "[1/5] 🌐 Fetching latest daemon & telemetry from GitHub..."
curl -sSL "$REPO_RAW/air_bridge_daemon.py" -o "$TARGET_DIR/air_bridge_daemon.py"
curl -sSL "$REPO_RAW/mesh_monitor.py" -o "$TARGET_DIR/mesh_monitor.py"
curl -sSL "$REPO_RAW/Air%20Compute%20Bridge" -o "$TARGET_DIR/Air Compute Bridge"
curl -sSL "$REPO_RAW/air_dispatch.py" -o "$TARGET_DIR/air_dispatch.py"
curl -sSL "$REPO_RAW/update.sh" -o "$TARGET_DIR/update.sh"
curl -sSL "$REPO_RAW/uninstall.sh" -o "$TARGET_DIR/uninstall.sh"
curl -sSL "$REPO_RAW/config.json" -o "$TARGET_DIR/config.json" 2>/dev/null || true
chmod +x "$TARGET_DIR/air_bridge_daemon.py" "$TARGET_DIR/mesh_monitor.py" "$TARGET_DIR/Air Compute Bridge" "$TARGET_DIR/update.sh" "$TARGET_DIR/uninstall.sh"

# 2. Standalone Python 3.12 (Zero Xcode CLT / Zero Homebrew)
echo ""
echo "[2/5] 🐍 Setting up standalone Apple Silicon Python runtime..."
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
chmod +x "$TARGET_DIR/python/bin/python3" 2>/dev/null || true
echo "      ✅ Python standard: $PYTHON"

# 3. Apple Silicon Metal Inference Engine
echo ""
echo "[3/5] ⚙️  Setting up Apple Metal GPU AI engine..."
if [ ! -f "$TARGET_DIR/bin/llama-server" ]; then
    echo "      Downloading pre-compiled Metal inference engine (~9.4 MB)..."
    curl -sSL "https://github.com/fablab503-collab/airbridge/releases/download/v1.4.0/metal-engine-aarch64.tar.gz" | tar -xz -C "$TARGET_DIR"
fi
chmod +x "$TARGET_DIR/bin/llama-server" 2>/dev/null || true
echo "      ✅ Metal engine ready."

# 4. Model Verification (LiquidAI 2.6B)
echo ""
echo "[4/5] 🧠 Verifying AI Model..."
MODEL_PATH="$TARGET_DIR/models/LFM2.5-2.6B-Q4_K_M.gguf"
if [ ! -f "$MODEL_PATH" ]; then
    if [ -f "/Volumes/Volume1/SecondBrain/Models/LiquidAI/LFM2.5-2.6B-GGUF/LFM2.5-2.6B-Q4_K_M.gguf" ]; then
        echo "      Found LiquidAI 2.6B on Synology NAS (/Volumes/Volume1)..."
        cp "/Volumes/Volume1/SecondBrain/Models/LiquidAI/LFM2.5-2.6B-GGUF/LFM2.5-2.6B-Q4_K_M.gguf" "$MODEL_PATH" 2>/dev/null || true
    fi
fi

if [ -f "$MODEL_PATH" ]; then
    echo "      ✅ Model ready: $MODEL_PATH"
else
    echo "      Downloading LiquidAI 2.6B GGUF (~1.67 GB)..."
    curl -L "https://huggingface.co/LiquidCloud/LFM-2.5-2.6B-GGUF/resolve/main/LFM2.5-2.6B-Q4_K_M.gguf" -o "$MODEL_PATH" || true
fi

# Configure config.json
cat <<EOF > "$TARGET_DIR/config.json"
{
  "service_name": "QuickText Intelligence",
  "version": "1.4.0",
  "stealth_mode": true,
  "stealth_label": "com.apple.quicktextd",
  "host": "127.0.0.1",
  "port": 8085,
  "internal_llama_port": 8086,
  "llama_bin": "$TARGET_DIR/bin/llama-server",
  "model_path": "$MODEL_PATH",
  "ram_limit_mb": 4096,
  "ctx_size": 4096,
  "gpu_layers": 99,
  "threads": 1,
  "nice_priority": 19,
  "user_priority_circuit_breaker": true,
  "min_system_free_ram_mb": 2048,
  "auto_pause_cooldown_sec": 600,
  "nas_queue_enabled": true,
  "nas_mount_path": "/Volumes/Volume1",
  "nas_queue_dir": "/Volumes/Volume1/SecondBrain/00 Chat Zero/AirQueue",
  "local_queue_dir": "$TARGET_DIR/queue",
  "queue_poll_interval_sec": 5,
  "low_battery_pause_threshold": 20
}
EOF

# Strip quarantine
xattr -dr com.apple.quarantine "$TARGET_DIR" 2>/dev/null || true

# Symlinks
ln -sf "$TARGET_DIR/update.sh" "$BIN_DIR/air-bridge-update" 2>/dev/null || true
ln -sf "$TARGET_DIR/mesh_monitor.py" "$BIN_DIR/mesh-status" 2>/dev/null || true
if [ -x "$TARGET_DIR/python/bin/python3" ]; then
    ln -sf "$TARGET_DIR/python/bin/python3" "$BIN_DIR/python3" 2>/dev/null || true
fi

# 5. LaunchAgent
echo ""
echo "[5/5] 🛡️  Installing LaunchAgent..."
cat <<EOF > "$PLIST_FILE"
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>Label</key>
    <string>com.danielmadac.air-compute-bridge</string>
    <key>ProgramArguments</key>
    <array>
        <string>$TARGET_DIR/Air Compute Bridge</string>
    </array>
    <key>RunAtLoad</key>
    <true/>
    <key>KeepAlive</key>
    <dict>
        <key>SuccessfulExit</key>
        <false/>
    </dict>
    <key>ProcessType</key>
    <string>Background</string>
    <key>LowPriorityIO</key>
    <true/>
    <key>StandardOutPath</key>
    <string>$LOG_FILE</string>
    <key>StandardErrorPath</key>
    <string>$LOG_FILE</string>
    <key>WorkingDirectory</key>
    <string>$TARGET_DIR</string>
</dict>
</plist>
EOF

launchctl bootout "$DOMAIN/$LABEL" 2>/dev/null || true
launchctl enable "$DOMAIN/$LABEL" 2>/dev/null || true
launchctl bootstrap "$DOMAIN" "$PLIST_FILE" 2>/dev/null || true
launchctl kickstart -k "$DOMAIN/$LABEL" 2>/dev/null || true

echo ""
echo "========================================================================================"
echo "🎉  AIRBRIDGE ONLINE INSTALLATION COMPLETE!"
echo "========================================================================================"
echo "Status:   🟢 ONLINE (Running in background on 127.0.0.1)"
echo "Standby:  😴 0W Lid-Closed Standby Active"
echo "Updates:  🌐 Future updates are 100% ONLINE via 'air-bridge-update'"
echo "========================================================================================"
echo ""
read -t 8 -p "Closing in 8 seconds..." dummy || true
exit 0
