#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""内置盘 GPT 只读体检（2026-09-22，DiskGenius 报 GUID 主分区表 CRC 错误）。

绝对只读：只 CreateFile 读扇区，不写任何字节。
诊断目标：
  1. 主 GPT 头（LBA1）CRC32 是否与存储值一致；
  2. 主分区项数组（LBA2..33）CRC32 是否与头内 EntryArrayCRC 一致；
  3. 备份 GPT 头（最后 LBA）与备份项数组是否完好；
  4. 两侧差异 → 给出最小修复清单（修复动作须 Variable 逐项拍板，本脚本不写）。
"""
import ctypes
import ctypes.wintypes as wt
import struct
import sys
import zlib

GENERIC_READ = 0x80000000
SHARE_RW = 3
OPEN_EXISTING = 3

def crc32(data: bytes) -> int:
    return zlib.crc32(data) & 0xFFFFFFFF

def read_lbas(path, lba, count, sector=512):
    GENERIC_READ = 0x80000000
    handle = ctypes.windll.kernel32.CreateFileW(
        path, GENERIC_READ, 3, None, 3, 0, None)
    if handle == -1 or handle == 0xFFFFFFFFFFFFFFFF:
        raise OSError("open %s failed err=%d" % (path, ctypes.windll.kernel32.GetLastError()))
    try:
        ctypes.windll.kernel32.SetFilePointer(handle, 0, None, 0)
        overlapped = None
        # 用 ReadFile + 大偏移：SetFilePointerEx
        li = lba * sector
        ctypes.windll.kernel32.SetFilePointer(handle, li & 0xFFFFFFFF,
                                              ctypes.byref(ctypes.c_long((li >> 32) & 0xFFFFFFFF)),
                                              0)
        buf = ctypes.create_string_buffer(sector * count)
        got = wt.DWORD(0)
        ok = ctypes.windll.kernel32.ReadFile(handle, buf, sector * count,
                                             ctypes.byref(got), None)
        if not ok:
            raise OSError("read failed err=%d" % ctypes.windll.kernel32.GetLastError())
        return buf.raw[:got.value]
    finally:
        ctypes.windll.kernel32.CloseHandle(handle)

def disk_size(path):
    handle = ctypes.windll.kernel32.CreateFileW(path, 0, 3, None, 3, 0, None)
    if handle in (-1, 0xFFFFFFFFFFFFFFFF):
        return None
    try:
        class DI(ctypes.Structure):
            _fields_ = [("cyl", ctypes.c_int64), ("t", ctypes.c_int),
                        ("tr", ctypes.c_int), ("s", ctypes.c_int),
                        ("csize", ctypes.c_int64)]
        geo = DI()
        ret = ctypes.windll.kernel32.DeviceIoControl(
            handle, 0x70000, None, 0, ctypes.byref(geo), ctypes.sizeof(geo), None, None)
        if ret and geo.csize > 0:
            return geo.csize * geo.t * geo.tr * geo.s * 512
        return None
    finally:
        ctypes.windll.kernel32.CloseHandle(handle)

def parse_gpt_header(hdr: bytes, which: str, lba_of_hdr: int, disk_lba_count: int):
    sig = hdr[0:8]
    if sig != b"EFI PART":
        print("  [%s] 头签名损坏: %r" % (which, sig))
        return None
    (revision, hsize, hcrc, _, my_lba, alt_lba, first_usable, last_usable,
     disk_guid, arr_lba, n_entries, entry_size, arr_crc) = struct.unpack_from(
        "<IIIIQQQQ16sQIII", hdr, 8)
    print("  [%s] 头内值: MyLBA=%d AltLBA=%d EntriesLBA=%d n=%d esize=%d" %
          (which, my_lba, alt_lba, arr_lba, n_entries, entry_size))
    # 头 CRC：头内 CRC 字段清零后计算
    zeroed = bytearray(hdr[:hsize])
    zeroed[16:20] = b"\x00\x00\x00\x00"
    calc_hcrc = crc32(bytes(zeroed))
    head_ok = (calc_hcrc == hcrc)
    print("  [%s] 头 CRC: 存储=%08X 计算=%08X → %s" %
          (which, hcrc, calc_hcrc, "OK" if head_ok else "损坏"))
    return dict(my_lba=my_lba, alt_lba=alt_lba, arr_lba=arr_lba, n=n_entries,
                esize=entry_size, arr_crc=arr_crc, head_ok=head_ok)

def main():
    # 1. 枚举磁盘：>800GB=内置 Crucial；其余实体盘（U 盘）也一并体检
    print("== 磁盘枚举 ==")
    targets = []
    for i in range(6):
        p = "\\\\.\\PhysicalDrive%d" % i
        sz = disk_size(p)
        if sz:
            tag = "内置Crucial" if sz > 8e11 else "其他(可能U盘)"
            print("  %s: %.1f GB (%s)" % (p, sz / 1e9, tag))
            targets.append((p, sz, tag))
    if not targets:
        print("未找到任何可读物理盘（需要管理员权限）")
        return 1

    for disk, sz, tag in targets:
        lba_count = sz // 512
        print("== 体检: %s %.1fGB (%d LBA) ==" % (disk, sz / 1e9, lba_count))
        mbr = read_lbas(disk, 0, 1)
        print("  LBA0 MBR 签名: %04X" % struct.unpack_from("<H", mbr, 510)[0])
        hdr = read_lbas(disk, 1, 1)
        prim = parse_gpt_header(hdr, "主", 1, lba_count)
        bk_hdr = read_lbas(disk, lba_count - 1, 1)
        bak = parse_gpt_header(bk_hdr, "备份", lba_count - 1, lba_count)
        for side, info, default_arr in (("主", prim, 2),
                                        ("备份", bak, lba_count - 33)):
            if not info:
                continue
            n, esize = info["n"], info["esize"]
            total = n * esize
            raw = read_lbas(disk, info["arr_lba"], (total + 511) // 512)[:total]
            calc = crc32(raw)
            ok = calc == info["arr_crc"]
            print("  [%s] 项数组 LBA%d: 存储=%08X 计算=%08X → %s" %
                  (side, info["arr_lba"], info["arr_crc"], calc,
                   "OK" if ok else "损坏"))
        if prim and bak:
            print("  [小结] 主头CRC=%s 备头CRC=%s" % (prim["head_ok"], bak["head_ok"]))
    print("== 只读体检完成（未写任何扇区）==")
    return 0

if __name__ == "__main__":
    sys.exit(main())
