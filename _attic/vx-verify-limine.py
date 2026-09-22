# -*- coding: utf-8 -*-
"""提权核验：U 盘 ESP limine.conf 是否含 Windows 链载项。只读。"""
import os

OUT = r"D:\2\14\-Un-Real-0d23d9ux-Engine-main\_attic\vx-verify-limine.rpt"
lines = []
for p in [r"Y:\limine.conf", r"Y:\boot-select.json"]:
    try:
        s = open(p, encoding="utf-8", errors="replace").read()
        lines.append(f"=== {p} ===")
        lines.append(s)
        lines.append(f"[contains Windows entry] {'Windows 11 (USB)' in s and 'protocol: efi' in s}")
    except Exception as ex:
        lines.append(f"{p}: {ex!r}")
with open(OUT, "w", encoding="utf-8") as f:
    f.write("\n".join(lines))
print("\n".join(lines))
