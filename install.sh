#!/usr/bin/env bash
# ========================================================================================
# ⚡ AIRBRIDGE ONLINE 1-CLICK INSTALLER (RUST CORE)
# Downloads and configures the native Apple Silicon Rust engine.
# Zero Python required. Zero Homebrew required. Zero Xcode CLT required.
# ========================================================================================
set -e

REPO_RAW="https://raw.githubusercontent.com/fablab503-collab/airbridge/main"
RELEASE_URL="https://github.com/fablab503-collab/airbridge/releases/download/v1.5.0"
TARGET_DIR="$HOME/Library/Application Support/Air Compute Bridge"
BIN_DIR="$HOME/.local/bin"
PLIST_FILE="$HOME/Library/LaunchAgents/com.danielmadac.air-compute-bridge.plist"
LOG_FILE="$HOME/Library/Logs/air-compute-bridge.log"
DOMAIN="gui/$(id -u)"
LABEL="com.danielmadac.air-compute-bridge"

clear
echo "========================================================================================"
echo "⚡  AIRBRIDGE — 1-CLICK RUST CORE INSTALLER"
echo "========================================================================================"
echo "Installing native Apple Silicon Rust engine (Zero Python / Zero Homebrew)..."
echo "========================================================================================"
echo ""

mkdir -p "$TARGET_DIR" "$TARGET_DIR/models" "$TARGET_DIR/queue" "$TARGET_DIR/cache" "$BIN_DIR" "$HOME/Library/Logs" "$HOME/Library/LaunchAgents"

# 1. Download Native Rust Engine (1.1 MB)
echo "[1/4] 🦀 Downloading native Apple Silicon Rust core (~1.1 MB)..."
curl -sSL "$RELEASE_URL/airbridge-aarch64.tar.gz" | tar -xz -C "$TARGET_DIR"
chmod +x "$TARGET_DIR/airbridge"
ln -sf "$TARGET_DIR/airbridge" "$BIN_DIR/airbridge"
ln -sf "$TARGET_DIR/airbridge" "$BIN_DIR/air-bridge"
echo "      ✅ Native Rust engine installed: $TARGET_DIR/airbridge"

# 2. Download Standalone Metal AI Engine (9.4 MB)
echo ""
echo "[2/4] ⚙️  Downloading Apple Metal GPU inference engine (~9.4 MB)..."
if [ ! -f "$TARGET_DIR/bin/llama-server" ]; then
    curl -sSL "$RELEASE_URL/metal-engine-aarch64.tar.gz" | tar -xz -C "$TARGET_DIR"
fi
chmod +x "$TARGET_DIR/bin/llama-server" 2>/dev/null || true
echo "      ✅ Metal engine ready."

# 3. Model Verification
echo ""
echo "[3/4] 🧠 Verifying LiquidAI 2.6B Model..."
MODEL_PATH="$TARGET_DIR/models/LFM2.5-2.6B-Q4_K_M.gguf"
if [ ! -f "$MODEL_PATH" ]; then
    if [ -f "/Volumes/Volume1/SecondBrain/Models/LiquidAI/LFM2.5-2.6B-GGUF/LFM2.5-2.6B-Q4_K_M.gguf" ]; then
        echo "      Linking LiquidAI 2.6B from Synology NAS..."
        cp "/Volumes/Volume1/SecondBrain/Models/LiquidAI/LFM2.5-2.6B-GGUF/LFM2.5-2.6B-Q4_K_M.gguf" "$MODEL_PATH" 2>/dev/null || true
    elif [ -f "/Volumes/256Gb/Air-Compute-Bridge/models/LFM2.5-2.6B-Q4_K_M.gguf" ]; then
        echo "      Copying pre-loaded model from USB-C key..."
        cp "/Volumes/256Gb/Air-Compute-Bridge/models/LFM2.5-2.6B-Q4_K_M.gguf" "$MODEL_PATH" 2>/dev/null || true
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
echo "[4/4] 🛡️  Installing LaunchAgent..."
cat <<EOF > "$PLIST_FILE"
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>Label</key>
    <string>com.danielmadac.air-compute-bridge</string>
    <key>ProgramArguments</key>
    <array>
        <string>$TARGET_DIR/airbridge</string>
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
</dict>
</plist>
EOF

launchctl bootout "$DOMAIN/$LABEL" 2>/dev/null || true
launchctl enable "$DOMAIN/$LABEL" 2>/dev/null || true
launchctl bootstrap "$DOMAIN" "$PLIST_FILE" 2>/dev/null || true
launchctl kickstart -k "$DOMAIN/$LABEL" 2>/dev/null || true

echo ""
echo "========================================================================================"
echo "🎉  AIRBRIDGE (RUST ENGINE) INSTALLED SUCCESSFULLY!"
echo "========================================================================================"
echo "Engine:          🦀 100% Native Rust Core (1.9 MB, Zero Python, Zero Homebrew)"
echo "Baseline Power:  ⚡ 30% Host Capacity by default"
echo "Standby:         😴 0.0W Lid-Closed Standby (Auto-revive on lid open)"
echo "Circuit Breaker: 🧠 10-Minute User Priority Circuit-Breaker (<2GB Free RAM)"
echo "Cache Guard:     🧹 10 GB Auto-Purge Manager"
echo "Swarm Mesh:      🌐 Distributed Failover Relay active"
echo "========================================================================================"
echo ""
echo "Terminal Commands available anywhere:"
echo "  airbridge status         # View live status and memory allocation"
echo "  airbridge boost 5        # Remote 80% Power Burst for 5 minutes"
echo "  airbridge swarm          # View pooled RAM and cores across all computers"
echo "  airbridge cache          # Check cache and purge if near 10GB"
echo "========================================================================================"
echo ""
read -t 8 -p "Closing in 8 seconds..." dummy || true
exit 0
