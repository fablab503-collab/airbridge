"""Open every sample file and check we get a real picture back.

Usage: python3 tests/test_formats.py FOLDER [FOLDER ...]
"""
import os
import sys
import time

import numpy as np

sys.path.insert(0, os.path.join(os.path.dirname(__file__), ".."))
from app import formats  # noqa: E402

ok = fail = 0
for folder in sys.argv[1:]:
    for name in sorted(os.listdir(folder)):
        path = os.path.join(folder, name)
        t = time.time()
        try:
            img = formats.open_any(open(path, "rb").read(), name)
            a = np.asarray(img)
            assert img.mode == "RGB" and img.width > 10 and img.height > 10, "too small or wrong mode"
            assert a.std() > 3, "picture is blank"
            print(f"  OK   {name:45s} {img.width}x{img.height}  {time.time() - t:.1f}s")
            ok += 1
        except Exception as e:
            print(f"  FAIL {name:45s} {e}")
            fail += 1
print(f"\n{ok} opened, {fail} failed")
sys.exit(1 if fail else 0)
