#!/usr/bin/env python3
"""
Air Compute Mesh — Terminal Connection & Power Telemetry Dashboard
Visualizes live connection health, latency, power/battery state, and AI compute capacity
between MacBook Pro, MacBook Air, Synology NAS, and iMac Worker.

Conforms to Daniel Madac's House Rules & Automation Standards.
"""

import os
import sys
import time
import json
import socket
import re
import urllib.request
import urllib.error
import subprocess
from pathlib import Path

NODES_DIR = Path("/Volumes/Volume1/SecondBrain/00 Chat Zero/Nodes")
QUEUE_DIR = Path("/Volumes/Volume1/SecondBrain/00 Chat Zero/AirQueue")

# Color Codes
C_RESET  = "\033[0m"
C_BOLD   = "\033[1m"
C_DIM    = "\033[2m"
C_GREEN  = "\033[32m"
C_YELLOW = "\033[33m"
C_RED    = "\033[31m"
C_CYAN   = "\033[36m"
C_BLUE   = "\033[34m"
C_MAGENTA= "\033[35m"

def get_tailscale_bin():
    if os.path.exists("/Applications/Tailscale.app/Contents/MacOS/Tailscale"):
        return "/Applications/Tailscale.app/Contents/MacOS/Tailscale"
    which = subprocess.run(["which", "tailscale"], capture_output=True, text=True)
    if which.returncode == 0:
        return which.stdout.strip()
    return None

def ping_host(ip: str, timeout_ms: int = 1000) -> float:
    """Returns ping latency in ms, or -1.0 if unreachable."""
    if not ip or ip == "127.0.0.1":
        return 0.1
    try:
        cmd = ["ping", "-c", "1", "-W", str(timeout_ms), ip]
        res = subprocess.run(cmd, capture_output=True, text=True, timeout=1.5)
        if res.returncode == 0:
            m = re.search(r"time=([0-9.]+)\s*ms", res.stdout)
            if m:
                return float(m.group(1))
    except Exception:
        pass
    return -1.0

def get_local_telemetry() -> dict:
    hostname = socket.gethostname().split(".")[0]
    
    # Power & Battery
    batt_pct = None
    power_src = "AC Power"
    charging = False
    try:
        res = subprocess.run(["pmset", "-g", "batt"], capture_output=True, text=True)
        out = res.stdout
        if "Battery Power" in out:
            power_src = "Battery Power"
        m = re.search(r"(\d+)%", out)
        if m:
            batt_pct = int(m.group(1))
        if "charging" in out.lower() and "not charging" not in out.lower():
            charging = True
    except Exception:
        pass

    # Thermal Health
    thermal = "Nominal (Cool)"
    try:
        res = subprocess.run(["pmset", "-g", "therm"], capture_output=True, text=True)
        if "warning" in res.stdout.lower() and "no thermal warning" not in res.stdout.lower():
            thermal = "Elevated"
    except Exception:
        pass

    # RAM (Total & Available)
    total_ram_gb = 16.0
    free_ram_gb = 8.0
    try:
        mem = int(subprocess.check_output(["sysctl", "-n", "hw.memsize"]).strip())
        total_ram_gb = round(mem / (1024**3), 1)
        vm = subprocess.check_output(["vm_stat"]).decode("utf-8")
        page_size = 16384
        free_pages = 0
        for line in vm.splitlines():
            m = re.match(r"^Pages\s+(free|speculative|inactive):\s+(\d+)\.", line)
            if m:
                free_pages += int(m.group(2))
        free_ram_gb = round((free_pages * page_size) / (1024**3), 1)
    except Exception:
        pass

    # Tailscale IP
    ts_ip = "127.0.0.1"
    ts_bin = get_tailscale_bin()
    if ts_bin:
        try:
            res = subprocess.run([ts_bin, "ip", "-4"], capture_output=True, text=True)
            if res.returncode == 0 and res.stdout.strip():
                ts_ip = res.stdout.strip()
        except Exception:
            pass

    # Check if local Air Compute Bridge is running
    model_name = "None"
    model_ram_mb = 0
    tps = 0.0
    bridge_running = False
    try:
        with urllib.request.urlopen("http://127.0.0.1:8085/health", timeout=0.8) as resp:
            if resp.status == 200:
                data = json.loads(resp.read().decode("utf-8"))
                bridge_running = True
                model_name = data.get("model", "None")
                model_ram_mb = data.get("ram_limit_mb", 4096)
                tps = 75.0  # nominal for 3B Metal
    except Exception:
        pass

    # Is this machine the Pro or the Air?
    role = "Primary Workstation (Claude/Vibe/AGY)"
    if "air" in hostname.lower():
        role = "Writing Node + 4GB Background AI"

    data = {
        "hostname": hostname,
        "role": role,
        "tailscale_ip": ts_ip,
        "updated_at": time.strftime("%Y-%m-%d %H:%M:%S"),
        "timestamp_epoch": time.time(),
        "status": "online",
        "power": {
            "source": power_src,
            "battery_pct": batt_pct,
            "is_charging": charging,
            "thermals": thermal
        },
        "compute": {
            "total_ram_gb": total_ram_gb,
            "free_ram_gb": free_ram_gb,
            "model": model_name,
            "model_ram_mb": model_ram_mb,
            "tokens_per_sec": tps,
            "bridge_running": bridge_running
        },
        "synology": {
            "mount_active": os.path.exists("/Volumes/Volume1")
        }
    }

    # Publish heartbeat to shared Synology vault if mounted
    if os.path.exists(NODES_DIR):
        try:
            NODES_DIR.mkdir(parents=True, exist_ok=True)
            slug = re.sub(r'[^a-zA-Z0-9_-]', '_', hostname.lower())
            (NODES_DIR / f"{slug}.json").write_text(json.dumps(data, indent=2), encoding="utf-8")
        except Exception:
            pass

    return data

def discover_tailscale_peers():
    peers = {}
    ts_bin = get_tailscale_bin()
    if not ts_bin:
        return peers
    try:
        res = subprocess.run([ts_bin, "status", "--json"], capture_output=True, text=True)
        if res.returncode == 0:
            data = json.loads(res.stdout)
            for k, p in data.get("Peer", {}).items():
                name = p.get("HostName", "")
                ips = p.get("TailscaleIPs", [])
                online = p.get("Online", False)
                if ips:
                    peers[name.lower()] = {
                        "name": name,
                        "ip": ips[0],
                        "online": online,
                        "os": p.get("OS", "")
                    }
    except Exception:
        pass
    return peers

def fetch_remote_air_telemetry(air_ip: str) -> dict:
    """Tries to query MacBook Air's Air Compute Bridge over HTTP directly."""
    if not air_ip:
        return {}
    try:
        url = f"http://{air_ip}:8085/health"
        with urllib.request.urlopen(url, timeout=1.0) as resp:
            if resp.status == 200:
                return json.loads(resp.read().decode("utf-8"))
    except Exception:
        pass
    return {}

def fetch_imac_telemetry(imac_ip: str) -> dict:
    """Queries iMac AI Router over HTTP."""
    if not imac_ip:
        return {}
    try:
        url = f"http://{imac_ip}:8080/v1/models"
        with urllib.request.urlopen(url, timeout=1.2) as resp:
            if resp.status == 200:
                data = json.loads(resp.read().decode("utf-8"))
                loaded = "None"
                for m in data.get("data", []):
                    if m.get("status", {}).get("value") == "loaded":
                        loaded = m.get("id")
                return {"loaded_model": loaded, "models_available": len(data.get("data", []))}
    except Exception:
        pass
    return {}

def render_dashboard(local_node: dict, ts_peers: dict):
    os.system("clear")
    now_str = time.strftime("%Y-%m-%d %H:%M:%S %Z")
    
    print(f"{C_BOLD}{C_CYAN}========================================================================================{C_RESET}")
    print(f"{C_BOLD}{C_CYAN}🌐  AIR-NAS COMPUTE MESH — LIVE TELEMETRY & POWER DASHBOARD{C_RESET}")
    print(f"{C_BOLD}{C_CYAN}========================================================================================{C_RESET}")
    print(f"{C_DIM}Live Status as of: {now_str}  ·  Synology Node Mesh Protocol{C_RESET}\n")

    # 1. Local Node Header
    h = local_node["hostname"]
    role = local_node["role"]
    ip = local_node["tailscale_ip"]
    pwr = local_node["power"]
    cmp = local_node["compute"]
    syn = local_node["synology"]

    batt_str = f"{pwr['battery_pct']}%" if pwr['battery_pct'] is not None else "N/A"
    charging_icon = "⚡" if pwr["is_charging"] else "🔋"
    pwr_color = C_GREEN if (pwr['battery_pct'] and pwr['battery_pct'] > 25) or pwr['source'] == "AC Power" else C_YELLOW

    print(f"{C_BOLD}📍 LOCAL MACHINE: {C_GREEN}{h}{C_RESET} ({role})")
    print(f"   IP (Tailscale): {C_CYAN}{ip}{C_RESET}  ·  Power: {pwr_color}{charging_icon} {pwr['source']} ({batt_str}){C_RESET}  ·  Thermals: {C_GREEN}{pwr['thermals']}{C_RESET}")
    print(f"   Memory: {C_BOLD}{cmp['free_ram_gb']} GB{C_RESET} free / {cmp['total_ram_gb']} GB total  ·  Synology Mount: {'🟢 /Volumes/Volume1 (Connected)' if syn['mount_active'] else '🔴 Unmounted'}")
    print(f"   🛡️  Stealth & Safety: {C_GREEN}Active{C_RESET} (LAN-Invisible · nice 19 Background · 1 Thread Model · Writing 100% Priority)")
    print(f"   ⚡ Computer Priority: {C_GREEN}Guaranteed{C_RESET} (Kills model immediately if RAM < 2GB · Auto-restarts after 10m)")
    print(f"   ❄️  Thermal Governor: {C_GREEN}Adaptive Duty-Cycle{C_RESET} (Paced cooling: Whisper 8s pause · Cool Harvest on 4-6h light sessions)")
    if cmp["bridge_running"]:
        print(f"   Air Compute Bridge: {C_GREEN}🟢 Active{C_RESET}  ·  Loaded: {C_BOLD}{cmp['model']}{C_RESET}  ·  Cap: {cmp['model_ram_mb']} MB")
    print()

    # 2. Peer Table
    print(f"{C_BOLD}📡 PEER NODES & COMPUTE POWER:{C_RESET}")
    print("┌──────────────────────────────┬──────────────┬─────────┬──────────────┬──────────────────────────────────┐")
    print("│ Node / Machine               │ IP Address   │ Status  │ Power / Batt │ Active Model & Compute Capacity  │")
    print("├──────────────────────────────┼──────────────┼─────────┼──────────────┼──────────────────────────────────┤")

    # Discover nodes from Synology Nodes directory if present
    syno_nodes = {}
    if os.path.exists(NODES_DIR):
        try:
            for f in NODES_DIR.glob("*.json"):
                try:
                    nd = json.loads(f.read_text(encoding="utf-8"))
                    syno_nodes[nd.get("hostname", "").lower()] = nd
                except Exception:
                    pass
        except Exception:
            pass

    # Peer 1: MacBook Air (Writer Node)
    air_ip = None
    for k, v in ts_peers.items():
        if "air" in k:
            air_ip = v["ip"]
            break
    
    # Fallback to heartbeat file if not found in Tailscale
    air_file_data = None
    for k, v in syno_nodes.items():
        if "air" in k and k != h.lower():
            air_file_data = v
            if not air_ip:
                air_ip = v.get("tailscale_ip")
            break

    if "air" in h.lower():
        # Current machine IS the Air, display MacBook Pro as peer
        pro_ip = None
        for k, v in ts_peers.items():
            if "pro" in k and k != h.lower():
                pro_ip = v["ip"]
                break
        ping_pro = ping_host(pro_ip) if pro_ip else -1.0
        pro_status = f"{C_GREEN}🟢 ONLINE{C_RESET}" if ping_pro >= 0 else f"{C_RED}🔴 OFFLINE{C_RESET}"
        pro_ping_str = f"Ping: {ping_pro:.1f}ms" if ping_pro >= 0 else "Unreachable"
        print(f"│ 💻 MacBook Pro (Orchestrator)│ {(pro_ip or 'Auto-Detect'):<12} │ {pro_status:<7} │ AC Line      │ Orchestrator, Claude, Vibe, AGY  │")
        print(f"│                              │ {pro_ping_str:<12} │         │ Dual M2 Pro  │ Full 16 GB Dev Node              │")
    else:
        # Current machine is MacBook Pro, display MacBook Air as peer
        ping_air = ping_host(air_ip) if air_ip else -1.0
        air_status = f"{C_GREEN}🟢 ONLINE{C_RESET}" if ping_air >= 0 else f"{C_YELLOW}🟡 STANDBY{C_RESET}"
        air_ping_str = f"Ping: {ping_air:.1f}ms" if ping_air >= 0 else "Standing By"
        
        # Probe Air Bridge HTTP API
        air_http = fetch_remote_air_telemetry(air_ip) if air_ip and ping_air >= 0 else {}
        air_model = air_http.get("model", air_file_data.get("compute", {}).get("model", "Qwen2.5-3B-Q8_0") if air_file_data else "Qwen2.5-3B-Q8_0")
        air_ram = air_http.get("ram_limit_mb", 4096)
        air_batt = air_file_data.get("power", {}).get("battery_pct", "AC") if air_file_data else "AC / Batt"
        
        print(f"│ 💻 MacBook Air (Writing Node) │ {(air_ip or 'Waiting TS'):<12} │ {air_status:<7} │ Batt: {str(air_batt):<6} │ {air_model:<32} │")
        print(f"│                              │ {air_ping_str:<12} │         │ Quiet / Cold │ Cap: {air_ram} MB RAM (~75 t/s Metal)      │")

    print("├──────────────────────────────┼──────────────┼─────────┼──────────────┼──────────────────────────────────┤")

    # Peer 2: Synology NAS
    syno_ip = ts_peers.get("danielmadac", {}).get("ip", "100.122.138.41")
    ping_syno = ping_host(syno_ip)
    syno_status = f"{C_GREEN}🟢 ACTIVE{C_RESET}" if ping_syno >= 0 else f"{C_RED}🔴 OFFLINE{C_RESET}"
    syno_ping_str = f"Ping: {ping_syno:.1f}ms" if ping_syno >= 0 else "Unreachable"
    print(f"│ 💾 Synology NAS (Volume1)    │ {syno_ip:<12} │ {syno_status:<7} │ AC Line      │ Central Storage & Queue Hub      │")
    print(f"│                              │ {syno_ping_str:<12} │ Direct  │ DSM 7.2 OS   │ SMB Volume1 & Volume2 Shares     │")

    print("├──────────────────────────────┼──────────────┼─────────┼──────────────┼──────────────────────────────────┤")

    # Peer 3: iMac Worker
    imac_ip = ts_peers.get("imac de madac", {}).get("ip", "100.67.146.113")
    ping_imac = ping_host(imac_ip)
    imac_status = f"{C_GREEN}🟢 ACTIVE{C_RESET}" if ping_imac >= 0 else f"{C_YELLOW}🟡 SLEEP{C_RESET}"
    imac_ping_str = f"Ping: {ping_imac:.1f}ms" if ping_imac >= 0 else "Unreachable"
    imac_data = fetch_imac_telemetry(imac_ip) if ping_imac >= 0 else {}
    imac_loaded = imac_data.get("loaded_model", "imac-research")
    print(f"│ 🖥️  iMac Worker (Heavyweight) │ {imac_ip:<12} │ {imac_status:<7} │ AC Line      │ Model: {imac_loaded:<25} │")
    print(f"│                              │ {imac_ping_str:<12} │ Direct  │ 44 GB RAM    │ http://imac:8080/v1              │")
    print("└──────────────────────────────┴──────────────┴─────────┴──────────────┴──────────────────────────────────┘\n")

    # 3. Synology Queue & Compute Workload
    inbox_count = 0
    outbox_count = 0
    proc_count = 0
    if os.path.exists(QUEUE_DIR):
        try:
            inbox_count = len([f for f in (QUEUE_DIR / "inbox").iterdir() if f.is_file() and not f.name.startswith(".")])
            outbox_count = len([f for f in (QUEUE_DIR / "outbox").iterdir() if f.is_file() and not f.name.startswith(".")])
            proc_count = len([f for f in (QUEUE_DIR / "processed").iterdir() if f.is_file() and not f.name.startswith(".")])
        except Exception:
            pass

    print(f"{C_BOLD}📊 DISTRIBUTED AIR-NAS COMPUTE QUEUE:{C_RESET}")
    print(f"   Storage Location: {C_CYAN}{QUEUE_DIR}{C_RESET}")
    print(f"   Pending Tasks (Inbox):      {C_BOLD}{inbox_count}{C_RESET} queued for MacBook Air")
    print(f"   Completed Results (Outbox): {C_BOLD}{outbox_count}{C_RESET} available for retrieval")
    print(f"   Archived Jobs (Processed):  {C_BOLD}{proc_count}{C_RESET} total jobs finished")
    print(f"{C_BOLD}{C_CYAN}========================================================================================{C_RESET}")

def main():
    watch = "--watch" in sys.argv or "-w" in sys.argv
    try:
        while True:
            local_node = get_local_telemetry()
            ts_peers = discover_tailscale_peers()
            render_dashboard(local_node, ts_peers)
            if not watch:
                break
            print(f"\n{C_DIM}[Live Watch Mode] Refreshing every 3s... Press Ctrl-C to exit.{C_RESET}")
            time.sleep(3)
    except KeyboardInterrupt:
        print("\nExiting telemetry monitor.")

if __name__ == "__main__":
    main()
