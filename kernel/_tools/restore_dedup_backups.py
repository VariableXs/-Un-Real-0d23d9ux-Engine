#!/usr/bin/env python3
"""从 g3fix-backup 的 dedup_* 备份恢复被 fix_e0252_dedup 误删的文件（2026-10-22 晚）。"""
import shutil
from pathlib import Path

ROOT = Path(r"D:/2/14/-Un-Real-0d23d9ux-Engine-main/kernel")
BK = ROOT / "_attic" / "g3fix-backup-2026-10-22"
SRC = ROOT / "varix" / "src"

key2path = {}
for p in SRC.rglob("*.rs"):
    rel = str(p.relative_to(SRC)).replace("\\", "_").replace("/", "_")
    key = "dedup_varix_src_" + rel[:-3]
    key2path[key] = p

restored, miss = 0, []
for b in BK.glob("dedup_*.rs"):
    key = b.name[:-3]
    if key in key2path:
        shutil.copy2(b, key2path[key])
        restored += 1
    else:
        miss.append(b.name)
print("restored:", restored, "miss:", len(miss))
for m in miss[:10]:
    print("  MISS", m)
