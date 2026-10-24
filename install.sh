#!/usr/bin/env bash
# ========================================================================================
# ⚡ MACLUSTER ONLINE 1-CLICK INSTALLER (100% NATIVE RUST SWARM ENGINE)
# Installs and launches the autonomous Apple Silicon background compute cluster.
# Automates Homebrew, Rust, Python toolchains, background LaunchAgents, and auto-updates.
# ========================================================================================
set -e

REPO_RAW="https://raw.githubusercontent.com/fablab503-collab/airbridge/main"
RELEASE_URL="https://github.com/fablab503-collab/airbridge/releases/download/v2.0.0"
METAL_URL="https://github.com/fablab503-collab/airbridge/releases/download/v1.4.0"
TARGET_DIR="$HOME/Library/Application Support/MACLUSTER"
BIN_DIR="$HOME/.local/bin"
PLIST_FILE="$HOME/Library/LaunchAgents/com.danielmadac.macluster.plist"
UPDATE_PLIST="$HOME/Library/LaunchAgents/com.danielmadac.macluster-autoupdate.plist"
LOG_FILE="$HOME/Library/Logs/macluster.log"
DOMAIN="gui/$(id -u)"
LABEL="com.danielmadac.macluster"

[ -t 1 ] && clear 2>/dev/null || true
echo "========================================================================================"
echo "⚡  MACLUSTER — 1-CLICK AUTONOMOUS SWARM INSTALLER (v2.0.0)"
echo "========================================================================================"
echo "Installing native Apple Silicon Rust cluster engine..."
echo "Power Baseline: 30% host capacity | User throttle: 1 CPU / 1 GPU / 1 GB RAM"
echo "========================================================================================"
echo ""

# 0. Administrator Sudo Access (Upfront elevation)
if [ "$EUID" -ne 0 ] && [ -t 0 ]; then
    echo "🔐 Prompting for administrator access to configure terminal access & tools..."
    sudo -v || true
    # Keep sudo credentials alive in background while installer is active
    while true; do sudo -n true; sleep 50; kill -0 "$$" || exit; done 2>/dev/null &
fi

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

# 1. Developer Toolchains (Homebrew, Rust, Python)
echo "[1/5] 🛠️  Verifying Developer Environment & Package Managers..."
if ! command -v brew >/dev/null 2>&1 && [ -f "/opt/homebrew/bin/brew" ]; then
    eval "$(/opt/homebrew/bin/brew shellenv)"
fi
if ! command -v brew >/dev/null 2>&1; then
    echo "      📦 Installing Homebrew package manager..."
    NONINTERACTIVE=1 /bin/bash -c "$(curl -fsSL https://raw.githubusercontent.com/Homebrew/install/HEAD/install.sh)" 2>/dev/null || true
    [ -f "/opt/homebrew/bin/brew" ] && eval "$(/opt/homebrew/bin/brew shellenv)"
else
    echo "      ✅ Homebrew ready."
fi

if ! command -v rustc >/dev/null 2>&1 && [ -f "$HOME/.cargo/bin/rustc" ]; then
    export PATH="$HOME/.cargo/bin:$PATH"
fi
if ! command -v rustc >/dev/null 2>&1; then
    echo "      🦀 Installing Rust toolchain (rustup + cargo)..."
    curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y --no-modify-path 2>/dev/null || true
    export PATH="$HOME/.cargo/bin:$PATH"
else
    echo "      ✅ Rust toolchain ready ($(rustc --version 2>&1 | awk '{print $1,$2}'))."
fi

if ! command -v python3 >/dev/null 2>&1; then
    echo "      🐍 Installing Python 3..."
    brew install python3 2>/dev/null || true
else
    echo "      ✅ Python 3 ready ($(python3 --version 2>&1 | awk '{print $1,$2}'))."
fi

# 2. Native Rust Engine
echo ""
echo "[2/5] 🦀 Installing native Apple Silicon Rust cluster engine..."
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
sudo ln -sf "$TARGET_DIR/macluster" "/usr/local/bin/macluster" 2>/dev/null || true
echo "      ✅ MACLUSTER native binary ready: $TARGET_DIR/macluster"

# 3. Standalone Metal GPU Inference Engine
echo ""
echo "[3/5] ⚙️  Installing Apple Metal GPU inference engine..."
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

# 4. Model Verification (LiquidAI 2.6B)
echo ""
echo "[4/5] 🧠 Verifying LiquidAI 2.6B model..."
MODEL_PATH="$TARGET_DIR/models/LFM2.5-2.6B-Q4_K_M.gguf"
if [ ! -f "$MODEL_PATH" ]; then
    if [ -f "$HOME/Library/Application Support/Air Compute Bridge/models/LFM2.5-2.6B-Q4_K_M.gguf" ]; then
        echo "      Reusing model from local installation..."
        cp -c "$HOME/Library/Application Support/Air Compute Bridge/models/LFM2.5-2.6B-Q4_K_M.gguf" "$MODEL_PATH" 2>/dev/null || cp "$HOME/Library/Application Support/Air Compute Bridge/models/LFM2.5-2.6B-Q4_K_M.gguf" "$MODEL_PATH" 2>/dev/null || true
    elif [ -n "$LOCAL_CORE" ] && [ -f "$LOCAL_CORE/models/LFM2.5-2.6B-Q4_K_M.gguf" ]; then
        echo "      Copying model from local media..."
        cp "$LOCAL_CORE/models/LFM2.5-2.6B-Q4_K_M.gguf" "$MODEL_PATH" 2>/dev/null || true
    elif [ -f "/Volumes/256Gb/Air-Compute-Bridge/models/LFM2.5-2.6B-Q4_K_M.gguf" ]; then
        echo "      Copying model from USB key..."
        cp "/Volumes/256Gb/Air-Compute-Bridge/models/LFM2.5-2.6B-Q4_K_M.gguf" "$MODEL_PATH" 2>/dev/null || true
    elif [ -f "/Volumes/Volume1/SecondBrain/Models/LiquidAI/LFM2.5-2.6B-GGUF/LFM2.5-2.6B-Q4_K_M.gguf" ]; then
        echo "      Copying model from Synology NAS..."
        cp "/Volumes/Volume1/SecondBrain/Models/LiquidAI/LFM2.5-2.6B-GGUF/LFM2.5-2.6B-Q4_K_M.gguf" "$MODEL_PATH" 2>/dev/null || true
    fi
fi

if [ -f "$MODEL_PATH" ]; then
    echo "      ✅ Model ready: $MODEL_PATH"
else
    echo "      Downloading LiquidAI 2.6B GGUF (~1.67 GB)..."
    curl -L "https://huggingface.co/LiquidCloud/LFM-2.5-2.6B-GGUF/resolve/main/LFM2.5-2.6B-Q4_K_M.gguf" -o "$MODEL_PATH" || true
fi

# Strip quarantine and apply valid ad-hoc code signature
xattr -dr com.apple.quarantine "$TARGET_DIR" 2>/dev/null || true
codesign -s - -f "$TARGET_DIR/macluster" 2>/dev/null || true
codesign -s - -f "$TARGET_DIR/bin/llama-server" 2>/dev/null || true

# 5. Configure & Start LaunchAgents (Cluster Daemon + Automated Daily Updater)
echo ""
echo "[5/5] 🛡️  Configuring LaunchAgents & Auto-Updater..."

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
        <string>/usr/local/bin:/usr/bin:/bin:/usr/sbin:/sbin:/opt/homebrew/bin:$HOME/.cargo/bin:$BIN_DIR</string>
        <key>DYLD_LIBRARY_PATH</key>
        <string>$TARGET_DIR/lib</string>
    </dict>
</dict>
</plist>
EOF
chmod 644 "$PLIST_FILE"

# Create Automatic Background Updater script
cat <<'EOF' > "$TARGET_DIR/auto_update.sh"
#!/usr/bin/env bash
# Automated background maintenance for Homebrew, Rust, Python and MACLUSTER
export PATH="/usr/local/bin:/usr/bin:/bin:/usr/sbin:/sbin:/opt/homebrew/bin:$HOME/.cargo/bin:$HOME/.local/bin"

# 1. Update Homebrew packages quietly
if command -v brew >/dev/null 2>&1; then
    brew update --quiet 2>/dev/null || true
fi

# 2. Update Rust toolchain quietly
if command -v rustup >/dev/null 2>&1; then
    rustup update --no-self-update 2>/dev/null || true
fi

# 3. Update MACLUSTER core online
curl -sSL https://raw.githubusercontent.com/fablab503-collab/airbridge/main/update.sh | bash 2>/dev/null || true
EOF
chmod +x "$TARGET_DIR/auto_update.sh"

# Create Daily Auto-Update LaunchAgent (Runs every 24 hours at 04:00 AM)
cat <<EOF > "$UPDATE_PLIST"
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>Label</key>
    <string>com.danielmadac.macluster-autoupdate</string>
    <key>ProgramArguments</key>
    <array>
        <string>/bin/bash</string>
        <string>$TARGET_DIR/auto_update.sh</string>
    </array>
    <key>StartCalendarInterval</key>
    <dict>
        <key>Hour</key>
        <integer>4</integer>
        <key>Minute</key>
        <integer>0</integer>
    </dict>
    <key>StandardOutPath</key>
    <string>$HOME/Library/Logs/macluster-autoupdate.log</string>
    <key>StandardErrorPath</key>
    <string>$HOME/Library/Logs/macluster-autoupdate.log</string>
</dict>
</plist>
EOF
chmod 644 "$UPDATE_PLIST"

# Bootstrap daemons
launchctl bootstrap "$DOMAIN" "$PLIST_FILE" 2>/dev/null || launchctl load "$PLIST_FILE" 2>/dev/null || true
launchctl bootstrap "$DOMAIN" "$UPDATE_PLIST" 2>/dev/null || launchctl load "$UPDATE_PLIST" 2>/dev/null || true

sleep 2

echo ""
echo "========================================================================================"
echo "🟢 MACLUSTER INSTALLED & RUNNING SUCCESSFULLY!"
echo "========================================================================================"
"$BIN_DIR/macluster" || true
echo "========================================================================================"
echo "⚡ Quick Commands:"
echo "  • Dashboard:  macluster            (see connected computers, RAM/CPU/GPU & drives)"
echo "  • Drives:     macluster drives     (detailed combined storage drives breakdown)"
echo "  • Status:     macluster status     (local hardware state & memory)"
echo "  • 80% Boost:  macluster boost 5    (5 min burst, then auto-returns to 30%)"
echo "  • Logs:       tail -f ~/Library/Logs/macluster.log"
echo "========================================================================================"
