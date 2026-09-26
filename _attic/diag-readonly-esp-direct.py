#!/usr/bin/env python3
r"""U 盘 ESP 内容只读核查（免提权路径）。

戒律：不做 assign（需提权）、不写任何东西。改走两条免提权路线：
  1) 卷标签哈希 —— 确认 U 盘各分区标签与 GUID（已有）
  2) 直接从 \\.\PhysicalDrive1 读 GPT 分区表头，确认 ESP 分区存在与边界
  3) 若系统已给 ESP 挂过字母（历史残留），顺带 Test-Path 探测
诚实原则：拿不到就说拿不到，不假装读过。
"""
import struct
import subprocess
import sys


def ps(cmd: str, timeout: int = 120) -> str:
    r = subprocess.run(
        ["powershell", "-NoProfile", "-NonInteractive", "-Command", cmd],
        capture_output=True, timeout=timeout,
    )
    return (r.stdout.decode("gbk", "replace")
            + r.stderr.decode("gbk", "replace")).strip() or "(空输出)"


print("=== A) 现有盘符探测（免提权，看 ESP 是否已挂着字母）===")
print(ps(
    "foreach ($L in 65..90) { $c = [char]$L; "
    "  if (Test-Path ($c.ToString() + ':\\EFI\\BOOT\\BOOTX64.EFI')) { "
    "    Write-Output ('FOUND ESP at ' + $c + ':') } } ; "
    "Write-Output '(扫描完毕)'"
))

print("=== B) 直读 GPT 分区表（\\\\.\\PhysicalDrive1，免提权可读）===")
try:
    with open(r"\\.\PhysicalDrive1", "rb") as fh:
        mbr = fh.read(512)
        if mbr[510:512] != b"\x55\xAA":
            print("不是有效 MBR 扇区")
            sys.exit(0)
        # 读 GPT 头（LBA1）
        fh.seek(512)
        hdr = fh.read(512)
        sig = hdr[0:8]
        print(f"GPT 签名 = {sig!r}  (期望 b'EFI PART')")
        part_lba = struct.unpack_from("<Q", hdr, 72)[0]
        num_parts = struct.unpack_from("<I", hdr, 80)[0]
        entry_sz = struct.unpack_from("<I", hdr, 84)[0]
        print(f"分区表起始 LBA = {part_lba}, 分区数 = {num_parts}, 表项大小 = {entry_sz}")

        # 读分区表
        fh.seek(part_lba * 512)
        table = fh.read(entry_sz * num_parts)

        names = {
            "{c12a7328-f81f-11d2-ba4b-00a0c93ec93b}": "EFI System (ESP)",
            "{ebd0a0a2-b9e5-4433-87c0-68b6b72699c7}": "Microsoft Basic Data",
            "{de94bba4-06d1-4d40-a16a-bfd50179d6ac}": "Windows Recovery",
            "{e3c9e316-0b5c-4db8-817d-f92df00215ae}": "Microsoft Reserved",
        }

        def guid_le(b: bytes) -> str:
            d1 = b[0:4][::-1].hex()
            d2 = b[4:6][::-1].hex()
            d3 = b[6:8][::-1].hex()
            d4 = b[8:10].hex()
            d5 = b[10:16].hex()
            return f"{{{d1}-{d2}-{d3}-{d4}-{d5}}}"

        print("\n--- U 盘分区表（直读 GPT）---")
        for i in range(num_parts):
            e = table[i * entry_sz:(i + 1) * entry_sz]
            if len(e) < entry_sz:
                break
            tguid = guid_le(e[0:16])
            if tguid == "{00000000-0000-0000-0000-000000000000}":
                continue
            first = struct.unpack_from("<Q", e, 32)[0]
            last = struct.unpack_from("<Q", e, 40)[0]
            attr = struct.unpack_from("<Q", e, 48)[0]
            nm = e[56:128].decode("utf-16-le", "replace").rstrip("\x00")
            sz = (last - first + 1) * 512
            print(f"P{i+1}: {names.get(tguid, tguid)}")
            print(f"     名称={nm!r}  LBA {first}..{last}  大小={sz/1024/1024:.1f} MiB  属性={attr:#x}")
except PermissionError as exc:
    print(f"n/a（PhysicalDrive1 读取被拒：{exc}）")
except OSError as exc:
    print(f"n/a（{exc}）")

print("\n=== C) 结论 ===")
print("ESP 存在性、内容正确性需以【提权 assign 后 Test-Path】为准；")
print("本轮免提权只能确认分区表层面的布局。")
