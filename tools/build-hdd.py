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
# UEFI 走查镜像输出路径（GPT + ESP；--uefi 时生效）
OUT_UEFI = os.path.join(ROOT, "_attic", "varix-uefi.img")

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
    kernel_cmdline: desktop=1 boot_timeout=0
"""

# UEFI 走查用配置：让内核三卡菜单亮出并停留 60s（MAX 上限），便于键注入与截图。
# 正式产物仍用上面的 LIMINE_CONF（boot_timeout=0 直接进 Variable）。
LIMINE_CONF_MENU = """# 三卡菜单走查（--uefi 时生效；正式盘为 boot_timeout=0）
timeout: 0
serial: yes

/kernel/varix
    protocol: limine
    kernel_path: boot():/kernel/varix
    kernel_cmdline: boot_timeout=60 boot_default=varix
"""

# 任务4：boot-select.json 骨架（SHARED 目录契约；真盘 FS 落地前由
# 引导卷 module 通道携带）。字段与 kernel/varix/src/bootcfg.rs 默认值表一致。
# 任务5 负向演练可临时改写此内容（损坏/超界/缺字段）验证容错路径。
BOOT_SELECT_JSON = """{
  "default_entry": "variable",
  "timeout_sec": 5,
  "show_menu": true,
  "last_boot": "variable",
  "windows_bootnext": null
}
"""


def short_checksum(short_11: bytes) -> int:
    s = 0
    for b in short_11:
        s = (((s & 1) << 7) + (s >> 1) + b) & 0xFF
    return s


def _crc32(data: bytes) -> int:
    """GPT 用的 CRC32（标准多项式 0xEDB88320，初始/终值取反）。"""
    import binascii
    return binascii.crc32(data) & 0xFFFFFFFF


def _guid_bytes(mixed: str) -> bytes:
    """把 'C12A7328-F81F-11D2-BA4B-00A0C93EC93B' 转成 GPT 的 16 字节混合端序。"""
    a, b, c, d, e = mixed.split("-")
    out = struct.pack("<IHH", int(a, 16), int(b, 16), int(c, 16))
    out += bytes.fromhex(d) + bytes.fromhex(e)
    return out


def build_gpt(img: bytearray, first_usable: int, last_usable: int,
              esp_first: int, esp_last: int, disk_guid_seed: int = 0x5641524958455350) -> None:
    """在镜像头部写入保护 MBR + GPT 主/备头 + 分区项（UEFI 引导必需）。

    布局：LBA0 保护 MBR；LBA1 GPT 头；LBA2..33 分区项（128 x 128B）；
    尾部：备用分区项 + 备用 GPT 头（以 1 个 LBA 为粒度按规范镜像到末尾）。
    """
    total_lba = len(img) // SECTOR

    # ---- LBA0：保护 MBR（0xEE 类型，覆盖整盘；UEFI 规范要求）----
    mbr = bytearray(SECTOR)
    e = bytearray(16)
    e[0] = 0x00
    e[1:4] = bytes([0x00, 0x02, 0x00])
    e[4] = 0xEE
    e[5:8] = bytes([0xFF, 0xFF, 0xFF])
    struct.pack_into("<I", e, 8, 1)
    struct.pack_into("<I", e, 12, min(total_lba - 1, 0xFFFFFFFF))
    mbr[446:462] = e
    mbr[510:512] = b"\x55\xAA"
    img[0:SECTOR] = mbr

    # ---- GPT 分区项（128 x 128B = 32 扇区）----
    entries = bytearray(128 * 128)
    pe = bytearray(128)
    pe[0:16] = _guid_bytes("C12A7328-F81F-11D2-BA4B-00A0C93EC93B")   # ESP 类型
    # 分区唯一 GUID：用固定种子派生（可复现，便于比对）
    pe[16:24] = struct.pack("<Q", disk_guid_seed ^ 0x1111111111111111)
    pe[24:32] = struct.pack("<Q", 0x5641524958455350)
    struct.pack_into("<Q", pe, 32, esp_first)
    struct.pack_into("<Q", pe, 40, esp_last)
    # 属性：bit0 = RequiredPartition，必须置位 —— OVMF BdsDxe 用属性位筛选
    # 可引导分区，全 0 时会把 ESP 当成普通数据分区，直接报
    # `failed to load Boot0001 "UEFI QEMU HARDDISK" ... Not Found`（实测确认）。
    struct.pack_into("<Q", pe, 48, 0x1)
    name = "VARIX ESP".encode("utf-16-le")
    pe[56:56 + len(name)] = name
    entries[0:128] = pe
    entries_crc = _crc32(bytes(entries))

    # ---- GPT 头（主 LBA1 / 备末 LBA）----
    def gpt_header(my_lba: int, alt_lba: int, entries_lba: int) -> bytes:
        # 严格按 UEFI 规范的 GPT 头字段偏移（错一个字节 OVMF 就认不出）：
        #   0  Signature(8) | 8  Revision(4) | 12 HeaderSize(4) | 16 HeaderCRC(4)
        #   20 Reserved(4)  | 24 MyLBA(8) | 32 AlternateLBA(8) | 40 FirstUsable(8)
        #   48 LastUsable(8)| 56 DiskGUID(16) | 72 PartitionEntryLBA(8)
        #   80 NumberOfPartitionEntries(4) | 84 SizeOfPartitionEntry(4)
        #   88 PartitionEntryArrayCRC32(4)  → 共 92 字节
        h = bytearray(SECTOR)
        h[0:8] = b"EFI PART"
        struct.pack_into("<I", h, 8, 0x00010000)      # 修订 1.0
        struct.pack_into("<I", h, 12, 92)             # 头大小
        struct.pack_into("<I", h, 16, 0)              # 头 CRC（稍后回填）
        struct.pack_into("<I", h, 20, 0)              # 保留
        struct.pack_into("<Q", h, 24, my_lba)
        struct.pack_into("<Q", h, 32, alt_lba)
        struct.pack_into("<Q", h, 40, first_usable)
        struct.pack_into("<Q", h, 48, last_usable)
        h[56:64] = struct.pack("<Q", 0x5641524958455350)   # DiskGUID 低 8 字节
        h[64:72] = struct.pack("<Q", 0x4F5353454C424156)   # DiskGUID 高 8 字节（"VABLEOSO"）
        struct.pack_into("<Q", h, 72, entries_lba)         # ★ 分区项数组所在 LBA
        struct.pack_into("<I", h, 80, 128)                 # 分区项数量
        struct.pack_into("<I", h, 84, 128)                 # 单项大小
        struct.pack_into("<I", h, 88, entries_crc)         # ★ 分区项数组 CRC
        struct.pack_into("<I", h, 16, _crc32(bytes(h[0:92])))
        return bytes(h)

    img[SECTOR:2 * SECTOR] = gpt_header(1, total_lba - 1, 2)
    img[2 * SECTOR:34 * SECTOR] = bytes(entries)
    # 备用：分区项在末尾前 32 扇区，备用头在最后一个扇区
    img[(total_lba - 33) * SECTOR:(total_lba - 1) * SECTOR] = bytes(entries)
    img[(total_lba - 1) * SECTOR:total_lba * SECTOR] = gpt_header(
        total_lba - 1, 1, total_lba - 33)


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
    # 槽位方向（**微软 FAT32 规范原文**，切勿再按直觉翻转）：
    #   "firstly comes the last LFN entry (the last part of the filename)...
    #    The last LFN entry has the largest sequence number which decreases in
    #    following entries. The first LFN entry has sequence number 1."
    #   例："File with very long filename.ext" → 0x43"me.ext" | 0x02"y long filena"
    #       | 0x01"File with ver" | 8.3 项
    # 即：物理**第一条**装名字**尾部**（序号最大、带 0x40）；序号 1 装名字**开头**、
    # 紧跟在 8.3 项之前。pyfatfs 的 make_lfn_entry 也是这个顺序，实测互证。
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
        content = self.dir_content(parent_start, child_records)
        return self.alloc(content)

    @staticmethod
    def dir_content(parent_start: int, child_records: list) -> bytes:
        dot = bytearray(32)
        dot[0:11] = b".          "
        dot[11] = 0x10
        dotdot = bytearray(32)
        dotdot[0:11] = b"..         "
        dotdot[11] = 0x10
        if parent_start:
            struct.pack_into("<H", dotdot, 26, parent_start)
        content = bytes(dot) + bytes(dotdot) + b"".join(child_records)
        return content.ljust(SPC * SECTOR, b"\x00")

    def rewrite(self, start: int, content: bytes):
        """把单簇目录/文件内容原位重写（用于先占簇后回填父指针的循环依赖）。"""
        assert len(content) <= SPC * SECTOR, "rewrite overflow"
        off = (start - 2) * SPC * SECTOR
        self.data[off:off + len(content)] = content


def main() -> int:
    # --uefi：产出 GPT + ESP 镜像（OVMF/真机 UEFI 引导），供 UEFI 走查用；
    # 默认（无参数）保持原 BIOS 行为与输出路径，零回归。
    uefi = "--uefi" in sys.argv
    # --diag：诊断专用镜像——kernel_cmdline 追加 panic_halt=1，panic 后保护屏
    # 停机不复位（实机排障：错误码可从容拍摄）。输出 varix-diag.img。
    diag = "--diag" in sys.argv
    out_path = OUT_UEFI if uefi else (OUT.replace(".img", "-diag.img") if diag else OUT)
    if not os.path.isfile(KERNEL_ELF):
        print("ERROR: kernel ELF missing; run cargo kbuild first", file=sys.stderr)
        return 1
    kernel = open(KERNEL_ELF, "rb").read()
    bios_sys = open(os.path.join(LIMINE_DIR, "limine-bios.sys"), "rb").read()

    # UEFI 模式额外留 64 扇区尾部余量：GPT 要求分区必须落在
    # [FirstUsable, LastUsable] 内，而 LastUsable = 末 LBA - 33（给备用分区项留位）。
    # 不留余量时 ESP 的末 LBA 会**越过** LastUsable（实测 131071 > 131038），
    # OVMF PartitionDxe 判定 GPT 表无效 → 不装分区子句柄 → BdsDxe 只能拿裸盘
    # 去抓 \EFI\BOOT\BOOTX64.EFI → `Not Found`（已用 MBR 对照实验证实）。
    total_sectors = (IMG_SECTORS + 64) if uefi else IMG_SECTORS
    img = bytearray(total_sectors * SECTOR)

    # ---- MBR：一个占位分区表项（bios-install 覆盖引导代码）----
    # UEFI 模式下 MBR 会被 build_gpt 的**保护 MBR** 覆盖（0xEE 覆盖整盘），
    # 这是 UEFI 规范要求的形态，故此处的引导代码段留空即可。
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

    # ---- /EFI/BOOT/BOOTX64.EFI（UEFI 引导体，OVMF/真机 UEFI 靠它）----
    bootx64 = open(os.path.join(LIMINE_DIR, "BOOTX64.EFI"), "rb").read()
    bootx64_rec = fat.file_entry("BOOTX64.EFI", b"BOOTX64 EFI", bootx64)
    boot_dir_start = fat.make_dir(0, bootx64_rec)      # 先占簇（dotdot 暂空）
    efi_dir_start = fat.make_dir(0, fat.dir_entry("boot", b"BOOT      ", boot_dir_start))
    fat.rewrite(boot_dir_start, FatImage.dir_content(efi_dir_start, bootx64_rec))  # 回填父指针

    # ---- 根目录 ----
    root_recs = []
    root_recs += fat.dir_entry("kernel", b"KERNEL   ", kernel_dir_start)
    root_recs += fat.dir_entry("efi", b"EFI       ", efi_dir_start)
    root_recs += fat.file_entry("limine-bios.sys", b"LIMINE~1SYS", bios_sys)
    conf_text = LIMINE_CONF_MENU if uefi else LIMINE_CONF
    if diag:
        conf_text = conf_text.replace(
            "kernel_cmdline: desktop=1 boot_timeout=0",
            "kernel_cmdline: desktop=1 boot_timeout=0 panic_halt=1",
        )
    # 外部 conf 覆盖（诊断实验用）：VARIX_CONF_FILE 指向的文件整体替换 conf。
    # 用途：把 repo 根 limine.conf（真机部署唯一事实源）灌进镜像，QEMU 串口
    # 对比 BIOS/UEFI 两种模式下内核实收 cmdline（limine.rs cmdline() 日志）。
    env_conf = os.environ.get("VARIX_CONF_FILE")
    if env_conf:
        with open(env_conf, "rb") as cf:
            conf_text = cf.read().decode("utf-8")
    # 短名必须严格 8+3：位置 0..7 是名字（不足用空格补齐），8..10 是扩展名。
    # 写成 b"LIMINE.CONF"（11 字节）会把第 8 字节填成 '.'，落成
    # "LIMINE.C.ONF" 这种畸形短名（实测 OVMF 能靠 LFN 找到文件，但不符合
    # 规范，且某些固件只认短名时就会 Not Found）。
    root_recs += fat.file_entry("limine.conf", b"LIMINE~1CON", conf_text.encode())
    root_recs += fat.file_entry("boot-select.json", b"BOOTSE~1JSO", BOOT_SELECT_JSON.encode())
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
    bpb[21] = 0xF8                               # 介质描述符（OVMF FAT 驱动校验）
    struct.pack_into("<H", bpb, 22, 0)           # FAT32: FATSz16=0
    struct.pack_into("<H", bpb, 24, 63)          # 每磁道扇区
    struct.pack_into("<H", bpb, 26, 16)          # 磁头数
    struct.pack_into("<I", bpb, 28, PART_LBA)    # HiddSec：分区前隐藏扇区
    struct.pack_into("<I", bpb, 32, PART_SECTORS)  # TotSec32：分区总扇区数
    struct.pack_into("<I", bpb, 36, FAT_SECTORS)  # FATSz32
    struct.pack_into("<H", bpb, 40, 0)           # ExtFlags = 0（镜像所有 FAT）
    struct.pack_into("<H", bpb, 42, 0)           # FSVer = 0
    struct.pack_into("<I", bpb, 44, 2)           # FAT32: 根目录簇 = 2
    struct.pack_into("<H", bpb, 48, 1)           # FSInfo   = 分区内扇区 1（与实际写入位置一致）
    struct.pack_into("<H", bpb, 50, 6)           # BkBootSec= 分区内扇区 6（备份引导扇区本体；7 为备份 FSInfo）
    bpb[64] = 0x80                               # DriveNum
    bpb[65] = 0                                  # Reserved
    bpb[66] = 0x29                               # 扩展引导签名（FAT32 位于偏移 66，OVMF 校验点）
    struct.pack_into("<I", bpb, 67, 0x56415258)  # 卷序列号
    bpb[71:82] = b"VARIXDISK   "                 # 卷标（11 字节）
    bpb[82:90] = b"FAT32    "                    # 文件系统类型（8+1）
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
    # 备份引导扇区/备份 FSInfo（BPB 声明在分区内扇区 6/7，必须真实在位）
    img[(PART_LBA + 6) * SECTOR:(PART_LBA + 7) * SECTOR] = bpb
    img[(PART_LBA + 7) * SECTOR:(PART_LBA + 8) * SECTOR] = fsi

    # FAT 两份
    fat_bytes = b"".join(struct.pack("<I", v & 0xFFFFFFFF) for v in fat.fat)
    fa = (PART_LBA + RESERVED) * SECTOR
    img[fa:fa + len(fat_bytes)] = fat_bytes
    fb = (PART_LBA + RESERVED + FAT_SECTORS) * SECTOR
    img[fb:fb + len(fat_bytes)] = fat_bytes

    # 数据区（含根目录簇）
    data_lba = (PART_LBA + DATA_OFF) * SECTOR
    img[data_lba:data_lba + len(fat.data)] = fat.data

    if uefi:
        # GPT：ESP 覆盖本分区（first_usable 取分区前一个 LBA 之后，避免与
        # 分区项数组（LBA2..33）重叠——PART_LBA=63 已远大于 33，安全）。
        total_lba = len(img) // SECTOR
        build_gpt(img, first_usable=34, last_usable=total_lba - 34,
                  esp_first=PART_LBA, esp_last=PART_LBA + PART_SECTORS - 1)
        os.makedirs(os.path.dirname(out_path), exist_ok=True)

    # 镜像必须是 512 的整数倍：多出的零头会让 QEMU 把末 LBA 报成 +1，
    # GPT 头的 AlternateLBA 就与末 LBA 不符（实测产物曾带 4 字节尾巴）。
    if len(img) % SECTOR != 0:
        img = img[:(len(img) // SECTOR) * SECTOR]

    with open(out_path, "wb") as f:
        f.write(img)
    kind = "GPT+ESP(UEFI)" if uefi else "MBR(FAT32)"
    print("OK: %s (%s, %d bytes, kernel %d, next_free cluster %d)"
          % (out_path, kind, len(img), len(kernel), fat.next_free))
    return 0


if __name__ == "__main__":
    sys.exit(main())
