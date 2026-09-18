#!/usr/bin/env python3
"""引导 0xc000007b 定性：①Secure Boot 现状（注册表非提权读）②本地 BOOTX64.EFI PE 头解剖 ③哈希对账。

0xc000007b = STATUS_INVALID_IMAGE_FORMAT（镜像格式非法）。
合法 UEFI 应用应为 PE32+：machine=0x8664，subsystem=10（EFI application）。
"""
import hashlib
import os
import struct
import winreg

ROOT = r"D:\2\14\-Un-Real-0d23d9ux-Engine-main"
ISO = os.path.join(ROOT, "build", "isoroot")

# ── 1) Secure Boot 现状 ──
# 2026-09-19 教训：真实值名是 UEFISecureBootEnabled（不是 UEFISecureBoot）。
# 值名写错时 QueryValueEx 抛 FileNotFoundError，曾被误读为「键消失 = SB 已关」——
# 实际提权 Confirm-SecureBootUEFI 返回 True，SB 全程开着。0xc000007b = SB 拦截未签名链载镜像。
print("=== Secure Boot state ===")
try:
    k = winreg.OpenKey(winreg.HKEY_LOCAL_MACHINE,
                       r"SYSTEM\CurrentControlSet\Control\SecureBoot\State")
    v, _ = winreg.QueryValueEx(k, "UEFISecureBootEnabled")
    print("UEFISecureBootEnabled =", v, "(1=ON 0=OFF)")
except FileNotFoundError:
    print("key/value not found（键消失多见于非 UEFI 启动；值消失勿据此断言 SB 已关）")
except PermissionError as e:
    print("registry read denied:", e)

# ── 2) isoroot 全清单（大小）──
print("\n=== isoroot listing ===")
for dp, dirs, files in os.walk(ISO):
    for f in sorted(files):
        p = os.path.join(dp, f)
        rel = os.path.relpath(p, ISO)
        print(f"{os.path.getsize(p):>12,} B  {rel}")

# ── 3) BOOTX64.EFI PE 头解剖 ──
print("\n=== BOOTX64.EFI PE header ===")
p = os.path.join(ISO, "EFI", "BOOT", "BOOTX64.EFI")
data = open(p, "rb").read()
print("size:", len(data), "bytes")
print("head16:", data[:16].hex(" "))

mz = data[:2] == b"MZ"
if not mz:
    print("!! 不是 MZ/PE 镜像——这就是 0xc000007b 的直接原因")
else:
    e_lfanew = struct.unpack_from("<I", data, 0x3C)[0]
    pe_sig = data[e_lfanew:e_lfanew + 4]
    machine, numsec, _ts, _ptr, _n, opt_size, _chars = struct.unpack_from("<HHIIIHH", data, e_lfanew + 4)
    opt_magic = struct.unpack_from("<H", data, e_lfanew + 24)[0]
    is64 = opt_magic == 0x20b
    subsys_off = e_lfanew + 24 + (68 if is64 else 68)
    subsys = struct.unpack_from("<H", data, subsys_off)[0]
    link_time = struct.unpack_from("<I", data, e_lfanew + 8)[0]
    import datetime
    lt = datetime.datetime.utcfromtimestamp(link_time).strftime("%Y-%m-%d %H:%M:%S UTC") if link_time else "?"
    print(f"MZ={mz} e_lfanew={e_lfanew:#x} PE sig={pe_sig}")
    print(f"machine={machine:#06x} ({'x64' if machine==0x8664 else 'ia32' if machine==0x14c else '??'})  "
          f"sections={numsec} optmagic={opt_magic:#06x} ({'PE32+ 64bit' if is64 else 'PE32 32bit'})")
    print(f"subsystem={subsys} ({'EFI application' if subsys==10 else '??'})  link-time={lt}")
    verdict = (machine == 0x8664 and is64 and subsys == 10)
    print("PE verdict:", "VALID x64 EFI application" if verdict else "!! 非法/非 x64 EFI 应用")

# ── 4) 哈希对账（本地 vs U 盘 manifest 记录值）──
print("\n=== sha256 vs USB manifest ===")
DISK_MANIFEST = {
    "EFI\\BOOT\\BOOTX64.EFI": "F24EFEECF6CFD3E11DD47A8263FECE74509EC91F83B7F1D166B8CA30892D629F",
    "limine-bios.sys": "8E432DB7B4721F906C9136B3854D3D9FEC7B337D6F6D049C0F2A1BA8EA591507",
    "limine.conf": "A3A09C864D8341A331258B626BAFA99601AB2F93B07285AE4D4506DC064F086B",
    "kernel\\varix": "0E1049A6DB57534F6A23179BA1BC5FB1239042A401F72DD98232B090834606CF",
    "initrd.img": "20DB5A2E13D4D0CAA108E077FFF85DF07E230CE320F57C292F4943D0FD984F02",
}
for rel, want in DISK_MANIFEST.items():
    fp = os.path.join(ISO, rel.replace("\\", "/"))
    h = hashlib.sha256(open(fp, "rb").read()).hexdigest().upper()
    print(("OK " if h == want else "BAD"), rel, h[:16])
