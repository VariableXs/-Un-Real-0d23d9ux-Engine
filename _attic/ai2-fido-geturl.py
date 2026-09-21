# -*- coding: utf-8 -*-
"""AI-2 · 获取 Fido.ps1（pbatard 官方仓库）并解析 Win11 zh-CN x64 ISO 直链。

下载通道：raw.githubusercontent 直连 → 失败走 api.github.com contents(base64)。
运行：powershell -File Fido.ps1 -GetUrl（先列版本再取 URL，迭代于真实输出）。
"""
import base64
import io
import json
import subprocess
import sys
import urllib.request

UA = {"User-Agent": "Mozilla/5.0", "Accept": "application/vnd.github+json"}
DEST = r"D:\2\14\-Un-Real-0d23d9ux-Engine-main\_attic\Fido.ps1"


def fetch_fido():
    try:
        req = urllib.request.Request(
            "https://raw.githubusercontent.com/pbatard/Fido/master/Fido.ps1", headers=UA)
        with urllib.request.urlopen(req, timeout=60) as r:
            data = r.read()
        print(f"[fido] raw OK {len(data)} bytes")
        return data
    except Exception as e:
        print(f"[fido] raw failed: {e} -> api.github.com fallback")
        req = urllib.request.Request(
            "https://api.github.com/repos/pbatard/Fido/contents/Fido.ps1", headers=UA)
        with urllib.request.urlopen(req, timeout=60) as r:
            j = json.load(r)
        data = base64.b64decode(j["content"])
        print(f"[fido] api OK {len(data)} bytes")
        return data


def main():
    data = fetch_fido()
    with open(DEST, "wb") as f:
        f.write(data)
    mode = sys.argv[1] if len(sys.argv) > 1 else "url"
    if mode == "winlist":
        PS = f"& '{DEST}' -Win List\n"
    elif mode == "rellist":
        win = sys.argv[2] if len(sys.argv) > 2 else "Windows 11"
        PS = f"& '{DEST}' -Win \"{win}\" -Rel List\n"
    elif mode == "langlist":
        win = sys.argv[2] if len(sys.argv) > 2 else "Windows 11"
        PS = f"& '{DEST}' -Win \"{win}\" -Rel List\n"
    elif mode == "list":
        win = sys.argv[2] if len(sys.argv) > 2 else "Windows 11"
        PS = f"& '{DEST}' -Win \"{win}\" -Lang List\n"
    else:  # url: [lang] [rel]
        win = "Windows 11"
        lang = sys.argv[2] if len(sys.argv) > 2 else "Chinese (Simplified)"
        rel = sys.argv[3] if len(sys.argv) > 3 else ""
        rel_part = f" -Rel \"{rel}\"" if rel else ""
        PS = f"& '{DEST}' -Win \"{win}\"{rel_part} -Lang \"{lang}\" -Arch x64 -GetUrl\n"
    out = subprocess.run(
        ["powershell", "-NoProfile", "-NonInteractive", "-ExecutionPolicy", "Bypass", "-Command", PS],
        capture_output=True, text=True, encoding="utf-8", errors="replace", timeout=300,
    )
    sys.stdout.write(out.stdout[-4000:])
    if out.returncode != 0:
        sys.stdout.write("\n[stderr]\n" + out.stderr[-4000:])
    sys.exit(out.returncode or 0)


if __name__ == "__main__":
    main()
