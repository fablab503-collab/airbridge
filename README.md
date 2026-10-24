# ⚡ MACLUSTER (v2.0.0)

**Autonomous Zero-Watt Apple Silicon Swarm Compute Cluster** written in 100% native Rust.

Pools distributed Apple Silicon Macs (from 2 up to 1,000 computers) into a unified high-performance compute hive over Wi-Fi, Ethernet, internet, or mobile hotspot.

---

## 🚀 1-Line Online Commands

### Install
```bash
curl -sSL https://raw.githubusercontent.com/fablab503-collab/airbridge/main/install.sh | bash
```

### Update (Online Only)
```bash
curl -sSL https://raw.githubusercontent.com/fablab503-collab/airbridge/main/update.sh | bash
```

### Uninstall
```bash
curl -sSL https://raw.githubusercontent.com/fablab503-collab/airbridge/main/uninstall.sh | bash
```

---

## ⚡ Core Architecture & Dynamic Sizing

1. **30% Capacity Baseline Rule**:
   - Runs silently in the background using exactly 30% of the host's capacity (30% RAM, 30% CPU/GPU cores).
   - E.g., on a 32GB RAM / 10-core Mac: reserves ~9.6 GB RAM and 3 CPU threads.

2. **Micro-Throttle to 1 CPU / 1 GPU / 1 GB RAM**:
   - When the user begins intensive writing, compiling, or gaming tasks, MACLUSTER dynamically drains down to **1 CPU thread, 1 GPU layer, and 1 GB RAM**.

3. **10-Minute User Priority Circuit Breaker**:
   - If user tasks exhaust system free RAM (< 2.0 GB) or the process is killed under high load, MACLUSTER pauses completely and automatically revives after **10 minutes (600s)** once host resources equalize.

4. **Remote 80% Power Burst (`macluster boost <minutes>`)**:
   - Remotely trigger an 80% capacity burst for demanding distributed jobs.
   - Automatically returns to the 30% baseline when the countdown expires.

5. **10 GB Auto-Purge Storage Governor**:
   - Maximum storage ceiling strictly capped at **10.0 GB**.
   - As soon as cache/queue reaches 10 GB, oldest files and logs are automatically purged down to **5.0 GB**.

6. **Distributed Swarm Mesh (2 to 1,000 Computers)**:
   - 100 computers combined can pool up to **1 TB RAM and 1,000+ CPU/GPU cores**.
   - Uses **30-second atomic failover relay leases**: if a node dies, sleeps, or closes its lid, the next computer automatically takes over the relay without lost jobs.

7. **0.0W Clamshell Standby**:
   - Interrogates macOS IOKit `AppleClamshellState`. Zero battery drain and zero heat when the lid is closed.

8. **Zero-Noise 2-File Desktop UX**:
   - Exactly two visible `.command` files: `Install MACLUSTER.command` and `Uninstall MACLUSTER.command`.
   - All internal binaries, libraries, models, and caches are hidden inside `.core/`.

---

## 💻 CLI Commands

```bash
macluster status       # View live service status, mode, and RAM allocation
macluster boost 10     # Burst to 80% power for 10 minutes
macluster cluster      # Inspect all pooled nodes across the distributed swarm hive
macluster cache        # Verify and enforce 10GB auto-purge cache ceiling
```
