# -*- coding: utf-8 -*-
"""定位截图中的壁纸：查 Windows 当前壁纸 + WE workshop 场景静态图。只读。"""
import ctypes
import ctypes.wintypes as wt
import os
import struct

W = r"W:\SteamLibrary\steamapps\workshop\content\431960"


def current_wallpaper():
    """SystemParametersInfo SPI_GETDESKWALLPAPER。"""
    SPI_GETDESKWALLPAPER = 0x0073
    buf = ctypes.create_unicode_buffer(520)
    ok = ctypes.windll.user32.SystemParametersInfoW(
        SPI_GETDESKWALLPAPER, 520, buf, 0)
    return buf.value if ok else "(failed)"


def scan_we():
    out = []
    if not os.path.isdir(W):
        return out
    for d in sorted(os.listdir(W))[:200]:
        pj = os.path.join(W, d, "project.json")
        if not os.path.isfile(pj):
            continue
        try:
            with open(pj, "r", encoding="utf-8", errors="ignore") as f:
                head = f.read(600)
        except OSError:
            continue
        title = ""
        for line in head.splitlines():
            if '"title"' in line:
                title = line.strip()[:90]
                break
        # 找目录里的静态图
        imgs = []
        for root, _, files in os.walk(os.path.join(W, d)):
            for fn in files:
                if fn.lower().endswith((".jpg", ".png", ".jpeg")):
                    p = os.path.join(root, fn)
                    try:
                        imgs.append((os.path.getsize(p), p))
                    except OSError:
                        pass
        imgs.sort(reverse=True)
        out.append((d, title, imgs[0][0] if imgs else 0, imgs[0][1] if imgs else ""))
    return out


def main():
    print("current wallpaper:", current_wallpaper())
    rows = scan_we()
    print("WE projects:", len(rows))
    rows.sort(key=lambda r: -r[2])
    for d, title, sz, p in rows[:12]:
        print("  %s | %.2fMB | %s | %s" % (d, sz / 1048576, p, title))


if __name__ == "__main__":
    main()
