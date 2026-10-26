"""Video: turn speech into text with Whisper (faster-whisper, MIT licence).

Gives back subtitles (.srt) and a transcript we store in the Immich description,
so Immich's search can find a video by what people say in it.
"""
from __future__ import annotations

import os
import subprocess
import tempfile
import wave

import numpy as np

WHISPER_MODEL = os.environ.get("STUDIO_WHISPER_MODEL", "base")
_model = None


def _whisper():
    global _model
    if _model is None:
        from faster_whisper import WhisperModel
        _model = WhisperModel(WHISPER_MODEL, device="cpu", compute_type="int8",
                              download_root=os.path.join(os.environ.get("STUDIO_MODEL_DIR", "/models"), "whisper"))
    return _model


def _ts(t: float) -> str:
    h, rem = divmod(int(t * 1000), 3600_000)
    m, rem = divmod(rem, 60_000)
    s, ms = divmod(rem, 1000)
    return f"{h:02}:{m:02}:{s:02},{ms:03}"


def transcribe(video_bytes: bytes, filename: str = "video.mp4") -> dict:
    with tempfile.TemporaryDirectory() as d:
        src = os.path.join(d, filename)
        wav = os.path.join(d, "audio.wav")
        open(src, "wb").write(video_bytes)
        r = subprocess.run(["ffmpeg", "-loglevel", "error", "-y", "-i", src, "-vn", "-ac", "1", "-ar", "16000", wav],
                           capture_output=True)
        if r.returncode != 0 or not os.path.exists(wav):
            return {"language": None, "text": "", "srt": "", "segments": [], "note": "no audio track"}
        with wave.open(wav) as w:  # 16 kHz mono 16-bit, made by ffmpeg above
            audio = np.frombuffer(w.readframes(w.getnframes()), np.int16).astype(np.float32) / 32768
        segments, info = _whisper().transcribe(audio, vad_filter=True)
        segs = [{"start": s.start, "end": s.end, "text": s.text.strip()} for s in segments]
    srt = "\n".join(f"{i}\n{_ts(s['start'])} --> {_ts(s['end'])}\n{s['text']}\n" for i, s in enumerate(segs, 1))
    return {"language": info.language, "text": " ".join(s["text"] for s in segs), "srt": srt, "segments": segs}
