#!/usr/bin/env bash
# ========================================================================================
# ⚡ MACLUSTER ONLINE 1-CLICK INSTALLER (100% NATIVE RUST SWARM ENGINE)
# Installs and launches the autonomous Apple Silicon background compute cluster.
# Zero Python required. Zero Homebrew required. Zero Xcode CLT required.
# ========================================================================================
set -e

REPO_RAW="https://raw.githubusercontent.com/fablab503-collab/airbridge/main"
RELEASE_URL="https://github.com/fablab503-collab/airbridge/releases/download/v2.0.0"
METAL_URL="https://github.com/fablab503-collab/airbridge/releases/download/v1.4.0"
TARGET_DIR="$HOME/Library/Application Support/MACLUSTER"
BIN_DIR="$HOME/.local/bin"
PLIST_FILE="$HOME/Library/LaunchAgents/com.danielmadac.macluster.plist"
LOG_FILE="$HOME/Library/Logs/macluster.log"
DOMAIN="gui/$(id -u)"
LABEL="com.danielmadac.macluster"

clear
echo "========================================================================================"
echo "⚡  MACLUSTER — 1-CLICK AUTONOMOUS SWARM INSTALLER (v2.0.0)"
echo "========================================================================================"
echo "Installing native Apple Silicon Rust cluster engine..."
echo "Power Baseline: 30% host capacity | User throttle: 1 CPU / 1 GPU / 1 GB RAM"
echo "========================================================================================"
echo ""

# Stop old daemons if running
launchctl bootout "$DOMAIN" "$HOME/Library/LaunchAgents/com.danielmadac.air-compute-bridge.plist" 2>/dev/null || true
launchctl bootout "$DOMAIN/com.danielmadac.air-compute-bridge" 2>/dev/null || true
launchctl bootout "$DOMAIN" "$PLIST_FILE" 2>/dev/null || true
launchctl bootout "$DOMAIN/$LABEL" 2>/dev/null || true

mkdir -p "$TARGET_DIR" "$TARGET_DIR/bin" "$TARGET_DIR/lib" "$TARGET_DIR/models" "$TARGET_DIR/queue" "$TARGET_DIR/cache" "$BIN_DIR" "$HOME/Library/Logs" "$HOME/Library/LaunchAgents"

# Check if local .core exists alongside installer (USB or local disk)
LOCAL_CORE=""
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" 2>/dev/null && pwd || true)"
if [ -n "$SCRIPT_DIR" ] && [ -d "$SCRIPT_DIR/.core" ]; then
    LOCAL_CORE="$SCRIPT_DIR/.core"
elif [ -d "/Volumes/256Gb/.core" ]; then
    LOCAL_CORE="/Volumes/256Gb/.core"
elif [ -d "/Volumes/Volume1/SecondBrain/Systems/MACLUSTER/.core" ]; then
    LOCAL_CORE="/Volumes/Volume1/SecondBrain/Systems/MACLUSTER/.core"
fi

# 1. Native Rust Engine
echo "[1/4] 🦀 Installing native Apple Silicon Rust engine..."
if [ -n "$LOCAL_CORE" ] && [ -f "$LOCAL_CORE/macluster" ]; then
    echo "      Copying from local media: $LOCAL_CORE/macluster"
    cp "$LOCAL_CORE/macluster" "$TARGET_DIR/macluster"
else
    echo "      Downloading from GitHub Releases (v2.0.0)..."
    curl -sSL "$RELEASE_URL/macluster-aarch64.tar.gz" | tar -xz -C "$TARGET_DIR"
fi
chmod +x "$TARGET_DIR/macluster"
ln -sf "$TARGET_DIR/macluster" "$BIN_DIR/macluster"
ln -sf "$TARGET_DIR/macluster" "$BIN_DIR/airbridge"
echo "      ✅ MACLUSTER native binary ready: $TARGET_DIR/macluster"

# 2. Standalone Metal GPU Inference Engine
echo ""
echo "[2/4] ⚙️  Installing Apple Metal GPU inference engine..."
if [ ! -f "$TARGET_DIR/bin/llama-server" ]; then
    if [ -n "$LOCAL_CORE" ] && [ -f "$LOCAL_CORE/bin/llama-server" ]; then
        echo "      Copying Metal engine from local media..."
        cp -R "$LOCAL_CORE/bin" "$TARGET_DIR/"
        cp -R "$LOCAL_CORE/lib" "$TARGET_DIR/"
    else
        echo "      Downloading Apple Metal runtime from GitHub..."
        curl -sSL "$METAL_URL/metal-engine-aarch64.tar.gz" | tar -xz -C "$TARGET_DIR"
    fi
fi
chmod +x "$TARGET_DIR/bin/llama-server" 2>/dev/null || true
echo "      ✅ Metal GPU engine ready."

# 3. Model Verification (LiquidAI 2.6B)
echo ""
echo "[3/4] 🧠 Verifying LiquidAI 2.6B model..."
MODEL_PATH="$TARGET_DIR/models/LFM2.5-2.6B-Q4_K_M.gguf"
if [ ! -f "$MODEL_PATH" ]; then
    if [ -f "/Volumes/Volume1/SecondBrain/Models/LiquidAI/LFM2.5-2.6B-GGUF/LFM2.5-2.6B-Q4_K_M.gguf" ]; then
        echo "      Copying model from Synology NAS..."
        cp "/Volumes/Volume1/SecondBrain/Models/LiquidAI/LFM2.5-2.6B-GGUF/LFM2.5-2.6B-Q4_K_M.gguf" "$MODEL_PATH" 2>/dev/null || true
    elif [ -n "$LOCAL_CORE" ] && [ -f "$LOCAL_CORE/models/LFM2.5-2.6B-Q4_K_M.gguf" ]; then
        echo "      Copying model from local media..."
        cp "$LOCAL_CORE/models/LFM2.5-2.6B-Q4_K_M.gguf" "$MODEL_PATH" 2>/dev/null || true
    fi
fi

if [ -f "$MODEL_PATH" ]; then
    echo "      ✅ Model ready: $MODEL_PATH"
else
    echo "      Downloading LiquidAI 2.6B GGUF (~1.67 GB)..."
    curl -L "https://huggingface.co/LiquidCloud/LFM-2.5-2.6B-GGUF/resolve/main/LFM2.5-2.6B-Q4_K_M.gguf" -o "$MODEL_PATH" || true
fi

# Strip quarantine
xattr -dr com.apple.quarantine "$TARGET_DIR" 2>/dev/null || true

# 4. Configure & Start LaunchAgent
echo ""
echo "[4/4] 🛡️  Configuring LaunchAgent..."
cat <<EOF > "$PLIST_FILE"
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>Label</key>
    <string>$LABEL</string>
    <key>ProgramArguments</key>
    <array>
        <string>$TARGET_DIR/macluster</string>
        <string>daemon</string>
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
    <key>EnvironmentVariables</key>
    <dict>
        <key>PATH</key>
        <string>/usr/local/bin:/usr/bin:/bin:/usr/sbin:/sbin:$BIN_DIR</string>
        <key>DYLD_LIBRARY_PATH</key>
        <string>$TARGET_DIR/lib</string>
    </dict>
</dict>
</plist>
EOF

chmod 644 "$PLIST_FILE"

# Bootstrap daemon
launchctl bootstrap "$DOMAIN" "$PLIST_FILE" 2>/dev/null || launchctl load "$PLIST_FILE" 2>/dev/null || true

sleep 2

echo ""
echo "========================================================================================"
echo "🟢 MACLUSTER INSTALLED & RUNNING SUCCESSFULLY!"
echo "========================================================================================"
"$BIN_DIR/macluster" status || true
echo "========================================================================================"
echo "⚡ Quick Commands:"
echo "  • Status:     macluster status"
echo "  • 80% Boost:  macluster boost 5    (5 min burst, then auto-returns to 30%)"
echo "  • Mesh Hive:  macluster cluster    (see all pooled computers)"
echo "  • Logs:       tail -f ~/Library/Logs/macluster.log"
echo "========================================================================================"
