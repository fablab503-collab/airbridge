#!/usr/bin/env python3
"""
air_dispatch.py — Dispatch a task from this chat / MacBook Pro to the MacBook Air via Synology NAS
"""
import sys
import json
import time
import uuid
from pathlib import Path

QUEUE_DIR = Path("/Volumes/Volume1/SecondBrain/00 Chat Zero/AirQueue")
INBOX = QUEUE_DIR / "inbox"
OUTBOX = QUEUE_DIR / "outbox"

def dispatch(prompt: str, system: str = None, max_tokens: int = 1024, timeout_sec: int = 60) -> dict:
    if not QUEUE_DIR.is_dir():
        return {"error": "Synology NAS AirQueue not mounted at /Volumes/Volume1"}
    
    INBOX.mkdir(parents=True, exist_ok=True)
    OUTBOX.mkdir(parents=True, exist_ok=True)
    
    job_id = f"job_{int(time.time())}_{uuid.uuid4().hex[:6]}"
    job_file = INBOX / f"{job_id}.json"
    result_file = OUTBOX / f"{job_id}.json"
    
    payload = {
        "id": job_id,
        "prompt": prompt,
        "system": system or "You are an autonomous AI writing & compute assistant running on MacBook Air.",
        "max_tokens": max_tokens,
        "submitted_at": time.time(),
        "client": "Antigravity Chat"
    }
    
    job_file.write_text(json.dumps(payload, indent=2), encoding="utf-8")
    
    start = time.time()
    while time.time() - start < timeout_sec:
        if result_file.is_file():
            try:
                res = json.loads(result_file.read_text(encoding="utf-8"))
                return res
            except Exception:
                pass
        time.sleep(1)
        
    return {"error": f"Timeout after {timeout_sec}s. MacBook Air is idle or in lid-standby."}

if __name__ == "__main__":
    if len(sys.argv) > 1:
        p = " ".join(sys.argv[1:])
        res = dispatch(p)
        print(json.dumps(res, indent=2))
    else:
        print("Usage: air_dispatch.py <prompt>")
