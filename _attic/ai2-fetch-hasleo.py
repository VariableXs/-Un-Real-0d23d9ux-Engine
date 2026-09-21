# -*- coding: utf-8 -*-
"""AI-2 · Hasleo WinToUSB 官方安装包下载（easyuefi.com 官网，只读官网+写 D:\\VarixDeploy）。

流程：抓产品页 → 解析最新 WinToUSB 安装包直链 → 下载 → 记录大小与 SHA-256。
"""
import hashlib
import re
import subprocess
import sys
import urllib.request

DEST_DIR = r"D:\VarixDeploy"
PAGE = "https://www.easyuefi.com/wintousb/"
UA = {"User-Agent": "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 Chrome/126 Safari/537.36"}


def http_get(url, timeout=60):
    req = urllib.request.Request(url, headers=UA)
    with urllib.request.urlopen(req, timeout=timeout) as r:
        return r.read()


def resolve_installer_url():
    # 直链猜测优先（官网固定命名），失败再解析页面。
    candidates = [
        "https://www.easyuefi.com/WinToUSB/downloads/WinToUSB_Free.exe",
    ]
    try:
        html = http_get(PAGE).decode("utf-8", "replace")
        for m in re.finditer(r'href="([^"]*WinToUSB[^"]*\.exe)"', html, re.I):
            u = m.group(1)
            if u.startswith("//"):
                u = "https:" + u
            elif u.startswith("/"):
                u = "https://www.easyuefi.com" + u
            candidates.insert(0, u)
    except Exception as e:
        print(f"[warn] page parse failed: {e}")
    for u in candidates:
        try:
            req = urllib.request.Request(u, headers=UA, method="HEAD")
            with urllib.request.urlopen(req, timeout=30) as r:
                if r.status == 200 and int(r.headers.get("Content-Length", "0")) > 1_000_000:
                    return u
        except Exception as e:
            print(f"[try] {u} -> {e}")
    raise SystemExit("未能解析出安装包直链")


def main():
    url = resolve_installer_url()
    print(f"[url] {url}")
    data = http_get(url, timeout=300)
    import os
    os.makedirs(DEST_DIR, exist_ok=True)
    name = url.rsplit("/", 1)[-1]
    dest = os.path.join(DEST_DIR, name)
    with open(dest, "wb") as f:
        f.write(data)
    h = hashlib.sha256(data).hexdigest()
    print(f"[saved] {dest}")
    print(f"[size] {len(data)/1024/1024:.1f} MiB")
    print(f"[sha256] {h}")


if __name__ == "__main__":
    main()
