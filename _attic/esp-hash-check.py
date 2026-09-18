#!/usr/bin/env python3
"""ESP 五件套字节闭环：本地 isoroot sha256 vs U 盘 ESP-MANIFEST.json 记录值。

manifest 值来自 2026-09-19 提权核验 v1 读取的 U 盘 ESP（Z:\ESP-MANIFEST.json）原文，
该 manifest 由 Build-ESP 部署时对盘上文件 Get-FileHash 生成（Build-ESP.ps1:69），
且部署 Verify 段已做过盘上重算比对全过。此处比对 = 盘上文件与仓库源字节一致。
"""
import hashlib
import os

ROOT = r"D:\2\14\-Un-Real-0d23d9ux-Engine-main"
ISO = os.path.join(ROOT, "build", "isoroot")

# U 盘 ESP-MANIFEST.json（v1 核验原文）记录值
DISK_MANIFEST = {
    "EFI\\BOOT\\BOOTX64.EFI": "F24EFEECF6CFD3E11DD47A8263FECE74509EC91F83B7F1D166B8CA30892D629F",
    "limine-bios.sys": "8E432DB7B4721F906C9136B3854D3D9FEC7B337D6F6D049C0F2A1BA8EA591507",
    "limine.conf": "A3A09C864D8341A331258B626BAFA99601AB2F93B07285AE4D4506DC064F086B",
    "kernel\\varix": "0E1049A6DB57534F6A23179BA1BC5FB1239042A401F72DD98232B090834606CF",
    "initrd.img": "20DB5A2E13D4D0CAA108E077FFF85DF07E230CE320F57C292F4943D0FD984F02",
}


def sha(p):
    h = hashlib.sha256()
    with open(p, "rb") as f:
        for b in iter(lambda: f.read(1 << 20), b""):
            h.update(b)
    return h.hexdigest().upper()


allok = True
for rel, want in DISK_MANIFEST.items():
    p = os.path.join(ISO, rel.replace("\\", "/"))
    got = sha(p)
    ok = got == want.upper()
    allok &= ok
    print(("OK " if ok else "BAD"), rel, got[:16])
print("ESP five-file set:", "ALL MATCH" if allok else "MISMATCH")
