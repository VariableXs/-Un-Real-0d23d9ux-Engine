# -*- coding: utf-8 -*-
"""AI-2 · Win11 25H2 官方 ISO 下载器（微软 CDN 直链，Fido 现场解析新 token）。

流程：Fido 解析新直链（token 有时效，下载前现取）→ 流式下载（1MiB 块）
→ 增量 SHA-256 → 完成后输出 路径/大小/SHA-256。断点不续传（token 一次性，
重跑脚本即重新解析新链接）。
"""
import hashlib
import os
import subprocess
import sys
import time
import urllib.request

DEST = r"D:\VarixDeploy\Win11_25H2_Chinese_Simplified_x64_v2.iso"
FIDO_GETURL = r"D:\2\14\-Un-Real-0d23d9ux-Engine-main\_attic\ai2-fido-geturl.py"
PY = sys.executable
UA = {"User-Agent": "Mozilla/5.0 (Windows NT 10.0; Win64; x64) Chrome/126"}

CHUNK = 1024 * 1024
PROGRESS_EVERY_BYTES = 256 * 1024 * 1024


def resolve_url():
    # 支持显式传入直链（token 24h 有效；避免重复解析触发反滥用 Sentinel 拒绝）。
    if len(sys.argv) > 1 and sys.argv[1].startswith("https://"):
        return sys.argv[1]
    out = subprocess.run([PY, FIDO_GETURL, "url", "Chinese (Simplified)"],
                         capture_output=True, text=True, encoding="utf-8", errors="replace",
                         timeout=300)
    for line in out.stdout.splitlines():
        line = line.strip()
        if line.startswith("https://") and ".iso" in line:
            return line
    sys.stdout.write(out.stdout[-1500:])
    raise SystemExit("未能解析出 ISO 直链")


def main():
    os.makedirs(os.path.dirname(DEST), exist_ok=True)
    url = resolve_url()
    print(f"[url] {url[:90]}... (token 现取)", flush=True)
    req = urllib.request.Request(url, headers=UA)
    t0 = time.perf_counter()
    h = hashlib.sha256()
    done = 0
    next_mark = PROGRESS_EVERY_BYTES
    with urllib.request.urlopen(req, timeout=120) as r, open(DEST, "wb") as f:
        total = int(r.headers.get("Content-Length", "0"))
        print(f"[total] {total/1024/1024/1024:.2f} GiB", flush=True)
        while True:
            block = r.read(CHUNK)
            if not block:
                break
            f.write(block)
            h.update(block)
            done += len(block)
            if done >= next_mark:
                dt = time.perf_counter() - t0
                pct = f"{done/total*100:5.1f}%" if total else "  -- "
                print(f"[prog] {pct} {done/1024/1024:7.0f} MiB "
                      f"{done/dt/1024/1024:6.1f} MiB/s", flush=True)
                next_mark += PROGRESS_EVERY_BYTES
    dt = time.perf_counter() - t0
    print(f"[saved] {DEST}", flush=True)
    print(f"[size] {done/1024/1024/1024:.2f} GiB in {dt/60:.1f} min "
          f"(avg {done/dt/1024/1024:.1f} MiB/s)", flush=True)
    print(f"[sha256] {h.hexdigest()}", flush=True)
    print("[DONE]", flush=True)


if __name__ == "__main__":
    main()
