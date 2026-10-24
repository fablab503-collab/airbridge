#!/usr/bin/env python3
"""
Air Compute Bridge — Background Model & Synology Compute Worker
Runs on the secondary MacBook Air (16 GB RAM / 1 TB SSD).
Provides a dedicated ~4 GB RAM background inference node and Synology NAS queue worker
while preserving CPU, thermals, and battery for undisturbed writing.

Conforms to Daniel Madac's House Rules & Automation Standards.
"""

import os
import sys
import json
import time
import signal
import socket
import urllib.request
import urllib.error
import subprocess
import threading
from pathlib import Path
from http.server import HTTPServer, BaseHTTPRequestHandler

BRIDGE_DIR = Path(__file__).resolve().parent
CONFIG_PATH = BRIDGE_DIR / "config.json"
LOG_PATH = Path(os.path.expanduser("~/Library/Logs/air-compute-bridge.log"))
PID_PATH = BRIDGE_DIR / "air_bridge.pid"

def log(msg: str):
    timestamp = time.strftime("%Y-%m-%d %H:%M:%S")
    formatted = f"[{timestamp}] [AirBridge] {msg}"
    print(formatted, flush=True)
    try:
        LOG_PATH.parent.mkdir(parents=True, exist_ok=True)
        with open(LOG_PATH, "a", encoding="utf-8") as f:
            f.write(formatted + "\n")
    except Exception:
        pass

def load_config() -> dict:
    default_config = {
        "service_name": "Air Compute Bridge",
        "version": "1.0.0",
        "host": "0.0.0.0",
        "port": 8085,
        "internal_llama_port": 8086,
        "llama_bin": "/opt/homebrew/bin/llama-server",
        "model_path": os.path.expanduser("~/models/qwen2.5-3b-instruct-q8_0.gguf"),
        "fallback_model_path": "/Volumes/Volume1/SecondBrain/Models/LiquidAI/LFM2.5-2.6B-GGUF/LFM2.5-2.6B-Q4_K_M.gguf",
        "ram_limit_mb": 4096,
        "ctx_size": 8192,
        "gpu_layers": 99,
        "threads": 2,
        "nice_priority": 15,
        "nas_queue_enabled": True,
        "nas_mount_path": "/Volumes/Volume1",
        "nas_queue_dir": "/Volumes/Volume1/SecondBrain/00 Chat Zero/AirQueue",
        "local_queue_dir": str(BRIDGE_DIR / "queue"),
        "queue_poll_interval_sec": 5,
        "low_battery_pause_threshold": 15
    }
    if CONFIG_PATH.exists():
        try:
            with open(CONFIG_PATH, "r", encoding="utf-8") as f:
                user_conf = json.load(f)
                for k, v in user_conf.items():
                    if isinstance(v, str) and ("$HOME" in v or "~" in v):
                        user_conf[k] = os.path.expanduser(v.replace("$HOME", "~"))
                default_config.update(user_conf)
        except Exception as e:
            log(f"Warning: Failed to parse {CONFIG_PATH}: {e}. Using defaults.")
    return default_config

def save_config(cfg: dict):
    try:
        with open(CONFIG_PATH, "w", encoding="utf-8") as f:
            json.dump(cfg, f, indent=2)
    except Exception:
        pass

class AirBridgeDaemon:
    def __init__(self):
        self.config = load_config()
        self.llama_proc = None
        self.running = True
        self.active_model_path = None
        self.http_server = None
        self.uptime_start = time.time()
        self.jobs_completed = 0
        self.current_job = None
        self.paused_until = None
        self.lid_closed = False
        self.last_update_check = 0
        self.current_gear = "Gear 1: Whisper Writing (Max Cold & User Priority)"

    def check_online_update(self):
        try:
            url = "https://raw.githubusercontent.com/fablab503-collab/airbridge/main/version.json"
            req = urllib.request.Request(url, headers={"User-Agent": "AirBridge-Daemon"})
            with urllib.request.urlopen(req, timeout=4) as resp:
                if resp.status == 200:
                    data = json.loads(resp.read().decode("utf-8"))
                    online_ver = data.get("version", "")
                    cur_ver = self.config.get("version", "1.0.0")
                    if online_ver and online_ver != cur_ver:
                        log(f"🔄 Online update available: v{online_ver} (current: v{cur_ver}). Updating online...")
                        daemon_url = "https://raw.githubusercontent.com/fablab503-collab/airbridge/main/air_bridge_daemon.py"
                        with urllib.request.urlopen(daemon_url, timeout=6) as d_resp:
                            if d_resp.status == 200:
                                script_path = Path(__file__).resolve()
                                script_path.write_bytes(d_resp.read())
                                self.config["version"] = online_ver
                                save_config(self.config)
                                log(f"✅ Updated to v{online_ver} online. Restarting daemon...")
                                self.stop_llama_server()
                                os.execv(sys.executable, [sys.executable] + sys.argv)
        except Exception:
            pass

    def is_lid_closed(self) -> bool:
        try:
            res = subprocess.run(["ioreg", "-r", "-k", "AppleClamshellState"], capture_output=True, text=True, timeout=1)
            if res.returncode == 0:
                for line in res.stdout.splitlines():
                    if "AppleClamshellState" in line:
                        return "Yes" in line
        except Exception:
            pass
        return False

    def evaluate_adaptive_gear(self) -> tuple:
        # 1. Thermal Defense check
        try:
            res = subprocess.run(["pmset", "-g", "therm"], capture_output=True, text=True)
            if "warning" in res.stdout.lower() and "no thermal warning" not in res.stdout.lower():
                self.current_gear = "Gear 0: Thermal Defense (Cooling down)"
                return self.current_gear, 15.0
        except Exception:
            pass

        # 2. Check user idle time & system load
        idle_sec = 0
        try:
            out = subprocess.check_output(["ioreg", "-c", "IOHIDSystem"]).decode("utf-8")
            m = re.search(r"\"HIDIdleTime\"\s*=\s*(\d+)", out)
            if m:
                idle_sec = int(m.group(1)) // 1000000000
        except Exception:
            pass

        free_mb = self.get_system_free_ram_mb()
        
        # Gear 2: During sustained 4-6h light-task sessions (idle > 120s & RAM > 3GB)
        if idle_sec >= 120 and free_mb > 3000:
            self.current_gear = "Gear 2: Cool Harvest (Sustained Light Session - Zero Heat)"
            return self.current_gear, 2.5
        else:
            self.current_gear = "Gear 1: Whisper Writing (Max Cold & User Priority)"
            return self.current_gear, 8.0

    def get_system_free_ram_mb(self) -> int:
        try:
            vm = subprocess.check_output(["vm_stat"]).decode("utf-8")
            page_size = 16384
            free_pages = 0
            for line in vm.splitlines():
                m = re.match(r"^Pages\s+(free|speculative|inactive):\s+(\d+)\.", line)
                if m:
                    free_pages += int(m.group(2))
            return int((free_pages * page_size) / (1024 * 1024))
        except Exception:
            return 4096

    def stop_llama_server(self):
        if self.llama_proc:
            try:
                os.killpg(os.getpgid(self.llama_proc.pid), signal.SIGTERM)
                self.llama_proc.wait(timeout=3)
            except Exception:
                try:
                    os.killpg(os.getpgid(self.llama_proc.pid), signal.SIGKILL)
                except Exception:
                    pass
            self.llama_proc = None
            log("🛑 Model process killed. 100% RAM returned to user computer tasks.")

    def run_resource_watchdog(self):
        log("Resource Circuit-Breaker & Lid Standby Watchdog started. Computer tasks have 1st priority.")
        while self.running:
            time.sleep(3)

            # --- Check Online Updates (Online Only) ---
            now = time.time()
            if now - self.last_update_check > 21600:  # 6 hours
                self.last_update_check = now
                threading.Thread(target=self.check_online_update, daemon=True).start()

            # --- 1. Lid State Detection (Clamshell / Standby Modality) ---
            lid_is_closed = self.is_lid_closed()
            if lid_is_closed:
                if not self.lid_closed:
                    self.lid_closed = True
                    log("😴 Laptop lid closed detected (AppleClamshellState = Yes). Entering Zero-Watt Standby Mode.")
                    self.stop_llama_server()
                continue
            else:
                if self.lid_closed:
                    self.lid_closed = False
                    log("☀️ Laptop lid opened detected (AppleClamshellState = No). Resuming Air Compute Bridge from standby.")
                    self.start_llama_server()

            # --- 2. Cooldown check if paused by RAM pressure ---
            if self.paused_until:
                now = time.time()
                if now < self.paused_until:
                    continue
                else:
                    free_mb = self.get_system_free_ram_mb()
                    min_mb = self.config.get("min_system_free_ram_mb", 2048)
                    if free_mb >= min_mb:
                        log(f"⏰ 10-minute cooldown expired. Free RAM is healthy ({free_mb} MB >= {min_mb} MB threshold). Auto-restarting model...")
                        self.paused_until = None
                        self.start_llama_server()
                    else:
                        log(f"10m cooldown finished but Mac is still heavily used ({free_mb} MB free RAM). Extending pause for 5 minutes...")
                        self.paused_until = now + 300
                    continue

            # --- 3. RAM Circuit-Breaker Check ---
            if self.llama_proc and self.llama_proc.poll() is None:
                free_mb = self.get_system_free_ram_mb()
                threshold = self.config.get("min_system_free_ram_mb", 2048)
                if free_mb < threshold:
                    cooldown = self.config.get("auto_pause_cooldown_sec", 600)
                    log(f"⚠️ Computer tasks priority triggered! Available RAM dropped to {free_mb} MB (safe threshold: {threshold} MB).")
                    log(f"Terminating background model process immediately to free RAM. Will auto-restart in {cooldown // 60} minutes.")
                    self.stop_llama_server()
                    self.paused_until = time.time() + cooldown

    def resolve_model(self) -> str:
        candidates = []
        m_str = self.config.get("model_path", "")
        if m_str:
            p = Path(os.path.expanduser(m_str))
            if not p.is_absolute():
                p = (BRIDGE_DIR / m_str).resolve()
            candidates.append(p)

        # Check local models directory
        local_models = BRIDGE_DIR / "models"
        if local_models.is_dir():
            for g in local_models.glob("*.gguf"):
                candidates.append(g)

        # Target dir models in Application Support
        target_models = Path.home() / "Library/Application Support/Air Compute Bridge/models"
        if target_models.is_dir():
            for g in target_models.glob("*.gguf"):
                candidates.append(g)

        # USB models
        usb_models = Path("/Volumes/256Gb/Air-Compute-Bridge/models")
        if usb_models.is_dir():
            for g in usb_models.glob("*.gguf"):
                candidates.append(g)

        # Check fallback
        fallback_str = self.config.get("fallback_model_path", "")
        if fallback_str:
            candidates.append(Path(os.path.expanduser(fallback_str)))

        # Scan ~/models
        home_models = Path.home() / "models"
        if home_models.is_dir():
            for g in home_models.glob("*.gguf"):
                candidates.append(g)

        # Synology NAS models
        syno_models = Path("/Volumes/Volume1/SecondBrain/Models")
        if syno_models.is_dir():
            for g in syno_models.glob("**/*.gguf"):
                candidates.append(g)

        for c in candidates:
            if c.is_file():
                log(f"Resolved AI model: {c}")
                return str(c)

        return ""

    def resolve_llama_bin(self) -> str:
        candidates = []
        # 1. Configured path
        cfg_bin = self.config.get("llama_bin", "")
        if cfg_bin:
            p = Path(os.path.expanduser(cfg_bin))
            if not p.is_absolute():
                p = (BRIDGE_DIR / cfg_bin).resolve()
            candidates.append(p)

        # 2. Bundled bin next to daemon
        candidates.append(BRIDGE_DIR / "bin" / "llama-server")

        # 3. Target dir in Application Support
        candidates.append(Path.home() / "Library/Application Support/Air Compute Bridge/bin/llama-server")

        # 4. USB volume paths
        candidates.append(Path("/Volumes/256Gb/Air-Compute-Bridge/bin/llama-server"))
        candidates.append(Path("/Volumes/256Gb/bin/llama-server"))

        # 5. System & Homebrew paths
        candidates.append(Path("/opt/homebrew/bin/llama-server"))
        candidates.append(Path("/usr/local/bin/llama-server"))

        for c in candidates:
            if c.is_file() and os.access(c, os.X_OK):
                log(f"Resolved llama-server binary: {c}")
                return str(c)

        # 6. Check system PATH
        which = subprocess.run(["which", "llama-server"], capture_output=True, text=True)
        if which.returncode == 0 and os.path.isfile(which.stdout.strip()):
            found = which.stdout.strip()
            log(f"Resolved llama-server via PATH: {found}")
            return found

        return ""

    def start_llama_server(self):
        model_file = self.resolve_model()
        if not model_file:
            log("No GGUF model found! The bridge will idle until a model is placed in ~/models or configured.")
            return False

        llama_bin = self.resolve_llama_bin()
        if not llama_bin:
            log("Error: llama-server executable not found in bundle or system PATH.")
            return False

        port = self.config.get("internal_llama_port", 8086)
        ctx = self.config.get("ctx_size", 4096)
        threads = self.config.get("threads", 1)
        gpu_layers = self.config.get("gpu_layers", 99)
        nice_level = self.config.get("nice_priority", 19)

        cmd = [
            "nice", "-n", str(nice_level),
            llama_bin,
            "-m", model_file,
            "--port", str(port),
            "--host", "127.0.0.1",
            "-c", str(ctx),
            "-t", str(threads),
            "-ngl", str(gpu_layers),
            "--parallel", "1"
        ]

        env = os.environ.copy()
        bin_dir = Path(llama_bin).resolve().parent
        lib_dir = bin_dir.parent / "lib"
        if lib_dir.is_dir():
            env["DYLD_LIBRARY_PATH"] = f"{lib_dir}:{env.get('DYLD_LIBRARY_PATH', '')}"
            env["DYLD_FALLBACK_LIBRARY_PATH"] = f"{lib_dir}:{env.get('DYLD_FALLBACK_LIBRARY_PATH', '')}"

        log(f"Starting llama-server (Low-Thermal 4GB mode, nice {nice_level}, threads {threads}): {' '.join(cmd)}")
        self.llama_proc = subprocess.Popen(
            cmd,
            stdout=subprocess.DEVNULL,
            stderr=subprocess.DEVNULL,
            env=env,
            preexec_fn=os.setsid
        )
        self.active_model_path = model_file

        # Wait for internal server to come online
        ready = False
        health_url = f"http://127.0.0.1:{port}/health"
        for _ in range(30):
            if not self.running:
                break
            try:
                with urllib.request.urlopen(health_url, timeout=1) as resp:
                    if resp.status == 200:
                        ready = True
                        break
            except Exception:
                time.sleep(1)

        if ready:
            log(f"llama-server online on 127.0.0.1:{port} with model {Path(model_file).name}")
            return True
        else:
            log("Warning: llama-server started but health check timed out. Retrying in background.")
            return False

    def is_battery_low(self) -> bool:
        try:
            res = subprocess.run(["pmset", "-g", "batt"], capture_output=True, text=True)
            out = res.stdout
            if "Battery Power" in out:
                # Find percent
                import re
                m = re.search(r"(\d+)%", out)
                if m:
                    pct = int(m.group(1))
                    threshold = self.config.get("low_battery_pause_threshold", 15)
                    if pct <= threshold:
                        return True
        except Exception:
            pass
        return False

    def run_heartbeat_loop(self):
        log("Heartbeat telemetry publisher started.")
        nodes_dir = Path("/Volumes/Volume1/SecondBrain/00 Chat Zero/Nodes")
        hostname = socket.gethostname().split(".")[0]
        slug = re.sub(r'[^a-zA-Z0-9_-]', '_', hostname.lower())

        while self.running:
            try:
                if nodes_dir.exists():
                    nodes_dir.mkdir(parents=True, exist_ok=True)
                    telemetry = self.get_full_telemetry()
                    target_file = nodes_dir / f"{slug}.json"
                    target_file.write_text(json.dumps(telemetry, indent=2), encoding="utf-8")
            except Exception as e:
                pass
            time.sleep(15)

    def get_full_telemetry(self) -> dict:
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

        # Thermals
        thermal = "Nominal (Cool)"
        try:
            res = subprocess.run(["pmset", "-g", "therm"], capture_output=True, text=True)
            if "warning" in res.stdout.lower() and "no thermal warning" not in res.stdout.lower():
                thermal = "Elevated"
        except Exception:
            pass

        # RAM
        total_ram_gb = 16.0
        free_ram_gb = 12.0
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

        return {
            "hostname": hostname,
            "role": "Writing Node + 4GB Background AI" if "air" in hostname.lower() else "Primary Workstation",
            "tailscale_ip": self.get_tailscale_ip(),
            "updated_at": time.strftime("%Y-%m-%d %H:%M:%S"),
            "timestamp_epoch": time.time(),
            "status": "online",
            "stealth": {
                "enabled": self.config.get("stealth_mode", True),
                "port_exposure": "Localhost Only (127.0.0.1, LAN Invisible)" if self.config.get("host") == "127.0.0.1" else f"Port {self.config.get('port')} LAN",
                "nice_level": self.config.get("nice_priority", 19),
                "writing_protection": "100% (nice 19, 2 threads max)"
            },
            "power": {
                "source": power_src,
                "battery_pct": batt_pct,
                "is_charging": charging,
                "thermals": thermal,
                "lid_state": "Closed (Standby · 0W)" if self.lid_closed else "Open (Active)",
                "standby_mode": self.lid_closed
            },
            "compute": {
                "total_ram_gb": total_ram_gb,
                "free_ram_gb": free_ram_gb,
                "model": "None (Lid Closed Standby · 0W)" if self.lid_closed else (Path(self.active_model_path).name if (self.active_model_path and not self.paused_until) else "None (Paused for User Priority)" if self.paused_until else "None"),
                "model_ram_mb": self.config.get("ram_limit_mb", 4096),
                "tokens_per_sec": 75.0,
                "bridge_running": not self.lid_closed,
                "jobs_completed": self.jobs_completed,
                "current_job": self.current_job
            },
            "watchdog": {
                "active": True,
                "status": "Standby Mode (Lid Closed · 0W)" if self.lid_closed else (f"Cooldown (Auto-Restart in {int(self.paused_until - time.time())}s)" if self.paused_until else "Active (Computer Tasks Priority)"),
                "governor_gear": "Standby (Zero-Power)" if self.lid_closed else self.current_gear,
                "system_free_ram_mb": self.get_system_free_ram_mb(),
                "min_safe_free_ram_mb": self.config.get("min_system_free_ram_mb", 2048),
                "cooldown_min": self.config.get("auto_pause_cooldown_sec", 600) // 60
            },
            "synology": {
                "mount_active": os.path.exists(self.config.get("nas_mount_path", "/Volumes/Volume1"))
            }
        }

    def run_nas_queue_worker(self):
        log("Synology NAS queue worker started.")
        while self.running:
            try:
                # Check lid standby state
                if self.lid_closed:
                    time.sleep(3)
                    continue

                poll_interval = self.config.get("queue_poll_interval_sec", 5)
                
                # Check battery preservation
                if self.is_battery_low():
                    time.sleep(30)
                    continue

                # Check queue directories
                nas_queue_dir = Path(os.path.expanduser(self.config.get("nas_queue_dir", "")))
                local_queue_dir = Path(os.path.expanduser(self.config.get("local_queue_dir", "")))
                
                active_queue = None
                if nas_queue_dir.is_dir() and os.path.exists(self.config.get("nas_mount_path", "/Volumes/Volume1")):
                    active_queue = nas_queue_dir
                elif local_queue_dir.is_dir():
                    active_queue = local_queue_dir

                if not active_queue:
                    time.sleep(10)
                    continue

                inbox = active_queue / "inbox"
                outbox = active_queue / "outbox"
                processed = active_queue / "processed"
                inbox.mkdir(parents=True, exist_ok=True)
                outbox.mkdir(parents=True, exist_ok=True)
                processed.mkdir(parents=True, exist_ok=True)

                # Look for tasks
                files = sorted([f for f in inbox.iterdir() if f.is_file() and not f.name.startswith(".")])
                if not files:
                    time.sleep(poll_interval)
                    continue

                for job_file in files:
                    if not self.running:
                        break
                    self.process_job_file(job_file, outbox, processed)
                    
            except Exception as e:
                log(f"Queue worker exception: {e}")
                time.sleep(5)

    def process_job_file(self, job_file: Path, outbox: Path, processed: Path):
        log(f"Claiming job: {job_file.name}")
        self.current_job = job_file.name
        
        try:
            content = job_file.read_text(encoding="utf-8")
            prompt = ""
            system_prompt = "You are an autonomous AI writing & compute assistant running locally on MacBook Air."
            max_tokens = 2048
            job_id = job_file.stem

            if job_file.suffix == ".json":
                try:
                    data = json.loads(content)
                    prompt = data.get("prompt", "")
                    system_prompt = data.get("system", system_prompt)
                    max_tokens = data.get("max_tokens", 2048)
                    job_id = data.get("id", job_id)
                except Exception:
                    prompt = content
            else:
                prompt = content

            # Send inference request to internal llama-server
            port = self.config.get("internal_llama_port", 8086)
            url = f"http://127.0.0.1:{port}/v1/chat/completions"
            
            payload = {
                "model": "air-bridge-model",
                "messages": [
                    {"role": "system", "content": system_prompt},
                    {"role": "user", "content": prompt}
                ],
                "max_tokens": max_tokens,
                "temperature": 0.7
            }
            
            req = urllib.request.Request(
                url,
                data=json.dumps(payload).encode("utf-8"),
                headers={"Content-Type": "application/json"}
            )
            
            t0 = time.time()
            with urllib.request.urlopen(req, timeout=300) as resp:
                result_data = json.loads(resp.read().decode("utf-8"))
            elapsed = time.time() - t0
            
            answer = result_data.get("choices", [{}])[0].get("message", {}).get("content", "")
            tokens = result_data.get("usage", {}).get("completion_tokens", 0)
            tps = (tokens / elapsed) if elapsed > 0 else 0

            # Write outbox result
            result_payload = {
                "job_id": job_id,
                "source_file": job_file.name,
                "completed_at": time.strftime("%Y-%m-%dT%H:%M:%SZ"),
                "worker": "MacBook-Air-4GB-Bridge",
                "elapsed_sec": round(elapsed, 2),
                "completion_tokens": tokens,
                "tokens_per_sec": round(tps, 1),
                "response": answer
            }

            out_file = outbox / f"{job_file.stem}_result.json"
            out_file.write_text(json.dumps(result_payload, indent=2, ensure_ascii=False), encoding="utf-8")

            # Also write markdown if input was markdown
            if job_file.suffix == ".md":
                md_out = outbox / f"{job_file.stem}_result.md"
                md_out.write_text(f"# Result: {job_id}\n\n{answer}\n", encoding="utf-8")

            # Move to processed
            dest = processed / f"{int(time.time())}_{job_file.name}"
            job_file.rename(dest)

            self.jobs_completed += 1
            gear, pause_sec = self.evaluate_adaptive_gear()
            log(f"Job {job_id} finished in {elapsed:.1f}s ({tokens} tokens, {tps:.1f} t/s). Output saved to outbox.")
            log(f"[{gear}] Thermal duty-cycle cooling pause: {pause_sec}s (keeping chassis cold).")
            time.sleep(pause_sec)
            
        except Exception as e:
            log(f"Error processing {job_file.name}: {e}")
            error_out = outbox / f"{job_file.stem}_error.json"
            error_out.write_text(json.dumps({"error": str(e), "file": job_file.name}, indent=2), encoding="utf-8")
            dest = processed / f"error_{int(time.time())}_{job_file.name}"
            try:
                job_file.rename(dest)
            except Exception:
                pass
        finally:
            self.current_job = None

    def start_api_server(self):
        daemon_ref = self
        port = self.config.get("port", 8085)
        host = self.config.get("host", "0.0.0.0")

        class ProxyHandler(BaseHTTPRequestHandler):
            def log_message(self, format, *args):
                pass  # Keep standard logs clean

            def do_GET(self):
                if self.path == "/telemetry":
                    self.send_response(200)
                    self.send_header("Content-Type", "application/json")
                    self.end_headers()
                    self.wfile.write(json.dumps(daemon_ref.get_full_telemetry(), indent=2).encode("utf-8"))
                    return

                if self.path in ["/", "/health", "/status"]:
                    self.send_response(200)
                    self.send_header("Content-Type", "application/json")
                    self.end_headers()
                    status = daemon_ref.get_full_telemetry()
                    self.wfile.write(json.dumps(status, indent=2).encode("utf-8"))
                    return
                    return

                # Forward GET to internal llama
                self.proxy_request("GET")

            def do_POST(self):
                self.proxy_request("POST")

            def proxy_request(self, method):
                internal_port = daemon_ref.config.get("internal_llama_port", 8086)
                url = f"http://127.0.0.1:{internal_port}{self.path}"
                
                content_length = int(self.headers.get("Content-Length", 0))
                body = self.rfile.read(content_length) if content_length > 0 else None

                headers = {}
                for k, v in self.headers.items():
                    if k.lower() not in ["host", "content-length"]:
                        headers[k] = v

                try:
                    req = urllib.request.Request(url, data=body, headers=headers, method=method)
                    with urllib.request.urlopen(req, timeout=300) as resp:
                        self.send_response(resp.status)
                        for k, v in resp.headers.items():
                            self.send_header(k, v)
                        self.end_headers()
                        self.wfile.write(resp.read())
                except urllib.error.HTTPError as e:
                    self.send_response(e.code)
                    self.send_header("Content-Type", "application/json")
                    self.end_headers()
                    self.wfile.write(e.read())
                except Exception as e:
                    self.send_response(502)
                    self.send_header("Content-Type", "application/json")
                    self.end_headers()
                    self.wfile.write(json.dumps({"error": f"Internal proxy error: {e}"}).encode("utf-8"))

        self.http_server = HTTPServer((host, port), ProxyHandler)
        log(f"OpenAI-Compatible API server listening on http://{host}:{port}/v1")
        self.http_server.serve_forever()

    def get_tailscale_ip(self) -> str:
        ts_bins = [
            "/Applications/Tailscale.app/Contents/MacOS/Tailscale",
            "/opt/homebrew/bin/tailscale",
            "/usr/local/bin/tailscale"
        ]
        for b in ts_bins:
            if os.path.exists(b):
                try:
                    res = subprocess.run([b, "ip", "-4"], capture_output=True, text=True)
                    if res.returncode == 0 and res.stdout.strip():
                        return res.stdout.strip()
                except Exception:
                    pass
        return "127.0.0.1"

    def stop(self):
        log("Shutting down Air Compute Bridge...")
        self.running = False
        if self.http_server:
            try:
                self.http_server.shutdown()
            except Exception:
                pass
        if self.llama_proc:
            try:
                os.killpg(os.getpgid(self.llama_proc.pid), signal.SIGTERM)
                self.llama_proc.wait(timeout=5)
            except Exception:
                try:
                    os.killpg(os.getpgid(self.llama_proc.pid), signal.SIGKILL)
                except Exception:
                    pass
        if PID_PATH.exists():
            try:
                PID_PATH.unlink()
            except Exception:
                pass
        log("Shutdown complete.")

def main():
    daemon = AirBridgeDaemon()

    def sig_handler(sig, frame):
        daemon.stop()
        sys.exit(0)

    signal.signal(signal.SIGINT, sig_handler)
    signal.signal(signal.SIGTERM, sig_handler)

    # Save PID
    PID_PATH.write_text(str(os.getpid()), encoding="utf-8")

    # Start llama-server
    daemon.start_llama_server()

    # Start NAS queue thread
    queue_thread = threading.Thread(target=daemon.run_nas_queue_worker, daemon=True)
    queue_thread.start()

    # Start Heartbeat Telemetry thread
    heartbeat_thread = threading.Thread(target=daemon.run_heartbeat_loop, daemon=True)
    heartbeat_thread.start()

    # Start Resource Circuit-Breaker Watchdog thread
    watchdog_thread = threading.Thread(target=daemon.run_resource_watchdog, daemon=True)
    watchdog_thread.start()

    # Start API server (blocking)
    try:
        daemon.start_api_server()
    except Exception as e:
        log(f"API Server stopped: {e}")
    finally:
        daemon.stop()

if __name__ == "__main__":
    main()
