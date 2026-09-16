#!/usr/bin/env python3
"""AURORA · varix HDD 镜像打包（FAT32 手写格式化 + Limine bios-install 前置）

Limine 的 iso9660 驱动读大文件有未决缺陷（QEMU 实证：>7.69MB 即
"failed to read file data"，2KB/64KB 对齐均无效）；FAT16 亦不被 12.9
识别（QEMU 实证 stage3 找不到）。故走 FAT32 HDD：Limine 的 FAT32
驱动成熟稳定。本脚本产出仓库根 varix.img：
  - MBR + 一个 FAT32 主分区（LBA 63 起，64MiB）
  - /limine-bios.sys + /limine.conf + /kernel/varix（均带 LFN 长名）
产出后用 limine.exe bios-install 安装引导，再 qemu -drive format=raw。
"""
import os
import struct
import sys

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
KERNEL_ELF = os.path.join(ROOT, "kernel", "target", "x86_64-unknown-none", "release", "varix")
LIMINE_DIR = os.path.join(ROOT, "tools", "limine", "limine-binary")
OUT = os.path.join(ROOT, "varix.img")

SECTOR = 512
PART_LBA = 63                 # 分区起始 LBA（bios-install 要求 >=63）
IMG_SECTORS = 131072          # 64 MiB
PART_SECTORS = IMG_SECTORS - PART_LBA
SPC = 1                       # 扇区/簇
RESERVED = 32                 # 保留扇区（含 FSInfo、备份引导）
FAT_SECTORS = 1024
DATA_OFF = RESERVED + 2 * FAT_SECTORS

LIMINE_CONF = """timeout: 0
serial: yes

/ Varix OS (aurora desktop demo)
    protocol: limine
    kernel_path: boot():/kernel/varix
    kernel_cmdline: desktop=1
"""


def short_checksum(short_11: bytes) -> int:
    s = 0
    for b in short_11:
        s = (((s & 1) << 7) + (s >> 1) + b) & 0xFF
    return s


def lfn_records(long_name: str, short_11: bytes):
    """LFN 目录项列表（逆序 13-UCS2 槽位，attr 0x0F，带短名校验和）。"""
    chk = short_checksum(short_11)
    padded = long_name + "\x00"
    while len(padded) % 13 != 0:
        padded += "\uFFFF"
    chunks = [padded[i:i + 13] for i in range(0, len(padded), 13)]
    n = len(chunks)
    out = []
    for idx, chunk in enumerate(reversed(chunks)):
        seq = n - idx
        e = bytearray(32)
        e[0] = (0x40 | seq) if idx == 0 else seq
        ucs = chunk.encode("utf-16-le")
        e[1:11] = ucs[0:10]
        e[11] = 0x0F
        e[12] = 0
        e[13] = chk
        e[14:26] = ucs[10:22].ljust(12, b"\xFF")[:12]
        e[26:28] = b"\x00\x00"
        e[28:32] = ucs[22:26].ljust(4, b"\xFF")[:4]
        out.append(bytes(e))
    return out


MAX_DATA_BYTES = (PART_SECTORS - DATA_OFF) * SECTOR


class FatImage:
    def __init__(self):
        self.fat = [0x0FFFFFF8, 0x0FFFFFFF, 0x0FFFFFFF] + [0] * (FAT_SECTORS * SECTOR // 4 - 3)
        self.data = bytearray(MAX_DATA_BYTES)
        self.next_free = 3  # 簇 2 留给根目录

    def alloc(self, content: bytes) -> int:
        n_cl = (len(content) + SPC * SECTOR - 1) // (SPC * SECTOR)
        start = self.next_free
        if start + n_cl - 2 > len(self.fat) - 2:
            raise RuntimeError("FAT overflow")
        if (start - 2 + n_cl) * SPC * SECTOR > len(self.data):
            raise RuntimeError("data area overflow")
        off = (start - 2) * SPC * SECTOR
        self.data[off:off + len(content)] = content
        for c in range(start, start + n_cl - 1):
            self.fat[c] = c + 1
        self.fat[start + n_cl - 1] = 0x0FFFFFFF
        self.next_free = start + n_cl
        return start

    def file_entry(self, long_name: str, short_11: bytes, content: bytes):
        start = self.alloc(content)
        e = bytearray(32)
        e[0:11] = short_11.ljust(11, b" ")[:11]
        e[11] = 0x20
        e[24:28] = b"\x21\x08\x01\x01"
        struct.pack_into("<H", e, 26, start)
        struct.pack_into("<I", e, 28, len(content))
        return lfn_records(long_name, short_11) + [bytes(e)]

    def dir_entry(self, long_name: str, short_11: bytes, start: int, is_dir=True):
        e = bytearray(32)
        e[0:11] = short_11.ljust(11, b" ")[:11]
        e[11] = 0x10 if is_dir else 0x20
        e[24:28] = b"\x21\x08\x01\x01"
        struct.pack_into("<H", e, 26, start)
        struct.pack_into("<I", e, 28, 0)
        return lfn_records(long_name, short_11) + [bytes(e)]

    def make_dir(self, parent_start: int, child_records: list) -> int:
        dot = bytearray(32)
        dot[0:11] = b".          "
        dot[11] = 0x10
        dotdot = bytearray(32)
        dotdot[0:11] = b"..         "
        dotdot[11] = 0x10
        if parent_start:
            struct.pack_into("<H", dotdot, 26, parent_start)
        content = bytes(dot) + bytes(dotdot) + b"".join(child_records)
        content = content.ljust(SPC * SECTOR, b"\x00")
        return self.alloc(content)


def main() -> int:
    if not os.path.isfile(KERNEL_ELF):
        print("ERROR: kernel ELF missing; run cargo kbuild first", file=sys.stderr)
        return 1
    kernel = open(KERNEL_ELF, "rb").read()
    bios_sys = open(os.path.join(LIMINE_DIR, "limine-bios.sys"), "rb").read()

    img = bytearray(IMG_SECTORS * SECTOR)

    # ---- MBR：一个占位分区表项（bios-install 覆盖引导代码）----
    e = bytearray(16)
    e[0] = 0x80
    # CHS(16 heads/63 spt): start LBA 63 -> C0 H1 S1；end LBA 131071 -> C130 H15 S63
    e[1:4] = bytes([0x01, 0x01, 0x00])
    e[4] = 0x0C                                  # FAT32 LBA
    e[5:8] = bytes([0x0F, 0x3F, 0x82])
    struct.pack_into("<I", e, 8, PART_LBA)
    struct.pack_into("<I", e, 12, PART_SECTORS)
    img[446:462] = e
    img[510:512] = b"\x55\xAA"

    # ---- FAT32 逻辑 ----
    fat = FatImage()
    root_start = 2  # 根目录固定簇 2（BPB rootclus=2）；next_free 已从 3 起
    fat.fat[2] = 0x0FFFFFFF
    fat.next_free = 3

    # ---- /kernel 子目录（含 varix）----
    varix_rec = fat.file_entry("varix", b"VARIX   ", kernel)
    kernel_dir_start = fat.make_dir(0, varix_rec)

    # ---- 根目录 ----
    root_recs = []
    root_recs += fat.dir_entry("kernel", b"KERNEL   ", kernel_dir_start)
    root_recs += fat.file_entry("limine-bios.sys", b"LIMINE~1SYS", bios_sys)
    root_recs += fat.file_entry("limine.conf", b"LIMINE.CONF", LIMINE_CONF.encode())
    root = b"".join(root_recs)
    root = root.ljust(SPC * SECTOR, b"\x00")
    if len(root) > SPC * SECTOR:
        print("ERROR: root directory overflow", file=sys.stderr)
        return 1
    root_off = (root_start - 2) * SPC * SECTOR
    fat.data[root_off:root_off + len(root)] = root

    # ---- BPB（分区第一扇区）----
    bpb = bytearray(SECTOR)
    bpb[0:3] = b"\xEB\x3C\x90"
    bpb[3:11] = b"LIMINE  "
    struct.pack_into("<H", bpb, 11, SECTOR)
    bpb[13] = SPC
    struct.pack_into("<H", bpb, 14, RESERVED)
    bpb[16] = 2
    struct.pack_into("<H", bpb, 17, 0)           # FAT32: 根目录非固定区
    struct.pack_into("<H", bpb, 19, 0)
    bpb[21] = 0xF8
    struct.pack_into("<H", bpb, 22, 0)           # FAT32: FATSz16=0
    struct.pack_into("<H", bpb, 24, 63)
    struct.pack_into("<H", bpb, 26, 16)
    struct.pack_into("<I", bpb, 28, PART_SECTORS)
    struct.pack_into("<I", bpb, 32, PART_LBA)
    struct.pack_into("<I", bpb, 36, FAT_SECTORS)  # FATSz32
    bpb[40] = 0x29
    struct.pack_into("<I", bpb, 41, 0x56415258)
    bpb[43:54] = b"VARIXDISK   "
    bpb[54:62] = b"FAT32   "
    struct.pack_into("<H", bpb, 46, 6)           # 备份引导扇区
    struct.pack_into("<I", bpb, 44, 2)           # FAT32: 根目录簇 = 2
    bpb[510:512] = b"\x55\xAA"
    p0 = PART_LBA * SECTOR
    img[p0:p0 + SECTOR] = bpb

    # FSInfo（分区相对扇区 1）
    fsi = bytearray(SECTOR)
    struct.pack_into("<I", fsi, 0, 0x41615252)   # "RRaA"
    struct.pack_into("<I", fsi, 484, 0x61417272)
    struct.pack_into("<I", fsi, 488, 0xFFFFFFFF)
    struct.pack_into("<I", fsi, 492, 0xFFFFFFFF)
    struct.pack_into("<I", fsi, 508, 0xAA550000)
    img[(PART_LBA + 1) * SECTOR:(PART_LBA + 2) * SECTOR] = fsi

    # FAT 两份
    fat_bytes = b"".join(struct.pack("<I", v & 0xFFFFFFFF) for v in fat.fat)
    fa = (PART_LBA + RESERVED) * SECTOR
    img[fa:fa + len(fat_bytes)] = fat_bytes
    fb = (PART_LBA + RESERVED + FAT_SECTORS) * SECTOR
    img[fb:fb + len(fat_bytes)] = fat_bytes

    # 数据区（含根目录簇）
    data_lba = (PART_LBA + DATA_OFF) * SECTOR
    img[data_lba:data_lba + len(fat.data)] = fat.data

    with open(OUT, "wb") as f:
        f.write(img)
    print("OK: %s (FAT32, %d bytes, kernel %d, next_free cluster %d)" % (OUT, len(img), len(kernel), fat.next_free))
    return 0


if __name__ == "__main__":
    sys.exit(main())
