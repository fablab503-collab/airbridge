# ⚡ AirBridge

Autonomous, stealth, zero-watt background AI compute bridge for Apple Silicon Macs (MacBook Air / MacBook Pro).

## 🚀 1-Line Online Install
```bash
curl -sSL https://raw.githubusercontent.com/fablab503-collab/airbridge/main/install.sh | bash
```

## 🔄 1-Line Online Update
```bash
curl -sSL https://raw.githubusercontent.com/fablab503-collab/airbridge/main/update.sh | bash
```

## 🗑️ 1-Line Online Uninstall
```bash
curl -sSL https://raw.githubusercontent.com/fablab503-collab/airbridge/main/uninstall.sh | bash
```

## ✨ Key Features
- **😴 0W Lid-Closed Standby**: Uses macOS IOKit `AppleClamshellState` to terminate model process immediately on lid close (0W battery drain, 0 heat in bag). Auto-revives on lid open.
- **🐍 Standalone Python 3.12**: Pre-bundled Apple Silicon CPython runtime. Zero Xcode Command Line Tools, zero Homebrew, zero sudo required.
- **⚙️ Apple Metal GPU Acceleration**: Standalone `llama-server` binary linking against embedded Metal shaders.
- **🧠 Computer-First RAM Circuit Breaker**: If writing apps need memory and free RAM drops < 2GB, the model is killed instantly and auto-restarts after 10 minutes.
- **❄️ Adaptive Whisper Thermal Governor**: Paced with 8s cooling pauses (2.5s on sustained light writing sessions) to guarantee zero chassis heat.
- **🛡️ LAN-Invisible Stealth**: Bound strictly to `127.0.0.1:8085` localhost.
- **📬 Synology NAS Hub (Chat Zero)**: Coordinates seamlessly with `00 Chat Zero/AirQueue` for multi-agent background compute.
