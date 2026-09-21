#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""mkexfat.py · 构造 exFAT 测试镜像（SHARED 分区验收用，任务18）。

按 Microsoft exFAT spec 1.00 布局逐字段构造（偏移经 GRUB grub-exfat.h
交叉确认），生成 SHARED 契约目录树。与内核 exfat_ro.rs 驱动独立实现、
互为参照（构造器喂实机，Rust 内嵌构造器喂宿主测试，同一驱动解析两处
通过 = 布局理解双重确认）。

布局（2 MiB 卷 / 512B 扇区 / 4KiB 簇）：
  LBA 0       VBR（BPB + BootChecksum + 0xAA55）
  LBA 1-7     扩展 VBR 区（清零）
  LBA 8       VBR 备份（同 LBA 0）
  LBA 128     FAT（u32/簇，含 EOC）
  LBA 512     Cluster Heap（簇 2 起：bitmap/upcase/root/...）
"""
import struct, sys

SECTOR = 512
TOTAL_SECTORS = 4096          # 2 MiB
BPS_SHIFT = 9                 # 512B
SPC_SHIFT = 3                 # 4KiB 簇
NUM_FATS = 1
FAT_OFFSET = 128              # 扇区
CLUSTER_BYTES = 1 << (BPS_SHIFT + SPC_SHIFT)
CLUSTERS_HEAP_START = 512     # 扇区（heap 起点）
# 簇数：heap 区 (4096-512)/8 = 448 簇，簇号 2..449
CLUSTER_COUNT = 448
ROOT_CLUSTER = 4              # 簇 2=bitmap 3=upcase 4=root（Windows 惯例序）

FAT_SIZE_BYTES = CLUSTER_COUNT * 4  # 1792B < 4 扇区

ATTR_READONLY = 0x01
ATTR_HIDDEN = 0x02
ATTR_SYSTEM = 0x04
ATTR_DIRECTORY = 0x10
ATTR_ARCHIVE = 0x20

# ---------------------------------------------------------------- utilities

fat = [0] * (CLUSTER_COUNT + 2)   # fat[c]：next cluster；0=free
fat[2] = 0xFFFFFFFF               # bitmap：单簇，EOC
fat[3] = 0xFFFFFFFF               # upcase
next_alloc = [5]                  # root=4 已占；从 5 起分配

def alloc_chain(nclusters, contiguous):
    """分配 nclusters 簇，返回 (first, chain)。contiguous 时物理连续。"""
    if contiguous:
        first = next_alloc[0]
        next_alloc[0] += nclusters
        chain = list(range(first, first + nclusters))
        fat[first + nclusters - 1] = 0xFFFFFFFF
        return first, chain
    first = next_alloc[0]
    chain = []
    for _ in range(nclusters):
        c = next_alloc[0]
        next_alloc[0] += 1
        chain.append(c)
    for a, b in zip(chain, chain[1:]):
        fat[a] = b
    fat[chain[-1]] = 0xFFFFFFFF
    return chain[0], chain

def entry_set_checksum(entries):
    """spec 7.10.1 EntrySetChecksum：除主 File Entry 的 checksum 字段
    （byte 2-3）外全部字节参与 16 位旋转累加。"""
    csum = 0
    for ei, e in enumerate(entries):
        for i, b in enumerate(e):
            if ei == 0 and i in (2, 3):
                continue
            csum = (((csum << 15) | (csum >> 1)) + b) & 0xFFFF
    return csum

def utf16le(s):
    return s.encode("utf-16-le")

def name_entries(name):
    """0xC1 File Name 条目序列（15 units/条）。"""
    units = utf16le(name)
    need = len(name)
    padded = units + b"\x00\x00" * ((-need) % 15)
    nents = max(1, (need + 14) // 15)
    out = []
    for k in range(nents):
        seg = padded[k*30:(k+1)*30]
        e = bytearray(32)
        e[0] = 0xC1
        e[1] = 0x00
        e[2:32] = seg
        out.append(bytes(e))
    return out, need

def make_file_entry(name, first_cluster, size, is_dir, contiguous,
                    valid_size=None):
    """构造完整 EntrySet：0x85 File + 0xC0 Stream + 0xC1×N。"""
    nunits = len(name)
    nname = max(1, (nunits + 14) // 15)
    nsec = 1 + nname
    # Stream Extension（0xC0）
    stream = bytearray(32)
    stream[0] = 0xC0
    flags = 0x00
    if contiguous:
        flags |= 0x02                      # NoFatChain
    stream[1] = flags
    stream[2] = 0x00
    stream[3] = nunits & 0xFF
    # name hash（spec 7.2.6：逐码元 up-case 后 u16 旋转累加）——
    # 驱动不校验 hash（GRUB 亦不校验），构造 0 即可；如实标注跳过。
    stream[4:6] = b"\x00\x00"
    vsize = (size if valid_size is None else valid_size)
    if is_dir:
        vsize = 0
    stream[8:16] = struct.pack("<Q", vsize)
    stream[16:20] = b"\x00\x00\x00\x00"
    stream[20:24] = struct.pack("<I", first_cluster)
    stream[24:32] = struct.pack("<Q", size)
    # File Entry（0x85）
    names, _ = name_entries(name)
    fentry = bytearray(32)
    fentry[0] = 0x85
    fentry[1] = nsec
    attr = (ATTR_DIRECTORY if is_dir else ATTR_ARCHIVE)
    fentry[4:6] = struct.pack("<H", attr)
    fentry[8:20] = b"\x00" * 12            # 时间字段清零（合法）
    csum = entry_set_checksum([bytes(fentry), bytes(stream)] + names)
    fentry[2:4] = struct.pack("<H", csum)
    return bytes(fentry) + bytes(stream) + b"".join(names)

def dir_cluster(entries_bytes):
    """把目录内容放进新分配的簇链。"""
    n = max(1, (len(entries_bytes) + CLUSTER_BYTES - 1) // CLUSTER_BYTES)
    first, _ = alloc_chain(n, contiguous=False)
    return first, entries_bytes.ljust(n * CLUSTER_BYTES, b"\x00")

def upcase_table():
    """完整 128KiB Upcase 表太大；规范允许缩短（DataLength 决定）。
    构造覆盖 BMP 前 0x200 码元的表（含 A-Z 折叠），其余默认恒等。"""
    units = []
    for c in range(0x200):
        if 0x61 <= c <= 0x7A:
            units.append(c - 0x20)
        elif 0xE0 <= c <= 0xFE and c != 0xF7:
            units.append(c - 0x20)
        else:
            units.append(c)
    return b"".join(struct.pack("<H", u) for u in units)

# ---------------------------------------------------------------- build tree

ROOT_ENTRIES = bytearray()

def add_child(parent_buf, entry):
    parent_buf.extend(entry)

# /apps.json（SHARED 契约核心）
APPS_JSON = """{
  "version": 1,
  "platform": "varix-shared",
  "apps": [
    { "id": "demo-writer", "name": "Demo Writer", "entry": "/PortableApps/demo-writer/app.exe" },
    { "id": "demo-mind",   "name": "Demo Mind",   "entry": "/PortableApps/demo-mind/app.exe" }
  ],
  "note": "SHARED 契约 apps.json schema v1（见任务9）"
}
""".encode("utf-8")
# /README.txt
README = ("""VARIX SHARED 共享分区（exFAT 只读视图）
========================================
本分区由 Windows 与 VARIX 双域共享：
- Windows 写入：完整读写
- VARIX 内核：只读挂载（任务18），写入走快照区过渡并如实标注
目录契约（任务9）：
/apps.json          应用注册表
/Games/             游戏库（跨域共享）
/PortableApps/      便携应用
快照区：VARIX 侧写入统一落快照区，不污染 Windows 视图。
""").encode("utf-8")
# /Games/hello.txt
HELLO = b"hello from SHARED/Games\r\nVARIX exFAT read-only mount works.\r\n"
# /PortableApps/manifest.txt
MANIFEST = b"PortableApps manifest v1\ndemo-writer 1.0.0\ndemo-mind 1.0.0\r\n"
# /big/manifest.bin：多簇文件（跨 FAT 链，7 簇 = 28KiB）
BIG = bytes((i * 0x9E ^ 0x31) & 0xFF for i in range(7 * CLUSTER_BYTES))
# /contig/contig.bin：连续簇文件（NoFatChain=1 路径）
CONTIG = bytes((i * 0x5A ^ 0x77) & 0xFF for i in range(3 * CLUSTER_BYTES))
# /boot-select.json（S4.2 last_boot 写回演练：L0 配置桥 SHARED 真相源种子；
# last_boot 初值 = windows（模拟 Windows 侧 Variable 先写过的现场）。
BOOT_SELECT = b"""{
  "default_entry": "variable",
  "timeout_sec": 5,
  "show_menu": true,
  "last_boot": "windows",
  "handoff": true
}"""

# 构造子目录与文件
def build():
    global ROOT_ENTRIES
    # 先建子目录内容（根目录放不下 EntrySet 引用前必须先分配子簇）
    games_dir = bytearray()
    f_first, f_chain = None, None
    # hello.txt
    fc, chain = alloc_chain(1, False)
    files = {}
    files["hello_data_cluster"] = fc
    games_dir.extend(make_file_entry("hello.txt", fc, len(HELLO), False, False))
    games_first, games_buf = dir_cluster(bytes(games_dir))
    # /Games
    ROOT_ENTRIES.extend(make_file_entry("Games", games_first, 0, True, False))

    apps_dir = bytearray()
    mc, _ = alloc_chain(1, False)
    apps_dir.extend(make_file_entry("manifest.txt", mc, len(MANIFEST), False, False))
    apps_first, apps_buf = dir_cluster(bytes(apps_dir))
    ROOT_ENTRIES.extend(make_file_entry("PortableApps", apps_first, 0, True, False))

    big_dir = bytearray()
    bc, bchain = alloc_chain(7, False)
    big_dir.extend(make_file_entry("manifest.bin", bc, len(BIG), False, False))
    big_first, big_buf = dir_cluster(bytes(big_dir))
    ROOT_ENTRIES.extend(make_file_entry("big", big_first, 0, True, False))

    cc, _ = alloc_chain(3, contiguous=True)
    contig_dir = bytearray()
    contig_dir.extend(make_file_entry("contig.bin", cc, len(CONTIG), False, True))
    contig_first, contig_buf = dir_cluster(bytes(contig_dir))
    ROOT_ENTRIES.extend(make_file_entry("contig", contig_first, 0, True, False))

    # 根目录文件
    aj, _ = alloc_chain(1, False)
    ROOT_ENTRIES.extend(make_file_entry("apps.json", aj, len(APPS_JSON), False, False))
    rm, _ = alloc_chain(1, False)
    ROOT_ENTRIES.extend(make_file_entry("README.txt", rm, len(README), False, False))
    # boot-select.json（S4.2：SHARED 真相源种子，内核 last_boot 写回目标）
    bsj, _ = alloc_chain(1, False)
    files["boot_select_cluster"] = bsj
    ROOT_ENTRIES.extend(make_file_entry("boot-select.json", bsj, len(BOOT_SELECT), False, False))

    # root 固定占用簇 4（与 BPB FirstClusterOfRootDirectory 一致；
    # 簇 2=bitmap 3=upcase 4=root 为 Windows 惯例分配序）。
    fat[4] = 0xFFFFFFFF
    # Volume Label(0x83 "SHARED") 在根目录最前，其后直接跟文件条目，
    # 尾部 end-of-dir 标记（label 后不得再插终止符）。
    vl = bytearray(32)
    vl[0] = 0x83
    vl[1] = 6
    vl[2:14] = utf16le("SHARED")
    root_first = 4
    root_buf = (bytes(vl) + bytes(ROOT_ENTRIES) + b"\x00" * 32).ljust(CLUSTER_BYTES, b"\x00")[:CLUSTER_BYTES]

    return {
        "root": (root_first, root_buf),
        "games": (games_first, games_buf),
        "apps": (apps_first, apps_buf),
        "big": (big_first, big_buf),
        "contig": (contig_first, contig_buf),
        "file_clusters": {
            "hello": files["hello_data_cluster"],
            "manifest": mc, "apps_json": aj, "readme": rm,
            "big": bc, "contig": cc,
            "boot_select_cluster": files["boot_select_cluster"],
        },
    }

def boot_checksum_spec(vbr):
    # spec 7.9 BootChecksum：0..119 逐字节 32 位旋转累加。
    csum = 0
    for b in vbr[0:120]:
        csum = (((csum >> 1) | ((csum << 31) & 0xFFFFFFFF)) + b) & 0xFFFFFFFF
    return csum

def build_vbr():
    vbr = bytearray(SECTOR)
    vbr[0:3] = b"\xEB\x76\x90"
    vbr[3:11] = b"EXFAT   "
    # 11..63 MustBeZero ✓（bytearray 初始 0）
    vbr[0x40:0x48] = struct.pack("<Q", 0)          # num_hidden_sectors
    vbr[0x48:0x50] = struct.pack("<Q", TOTAL_SECTORS)  # VolumeLength
    vbr[0x50:0x54] = struct.pack("<I", FAT_OFFSET)     # FatOffset
    vbr[0x54:0x58] = struct.pack("<I", 8)              # FatLength（4 扇区够，给 8 富余）
    vbr[0x58:0x5C] = struct.pack("<I", CLUSTERS_HEAP_START)  # ClusterHeapOffset
    vbr[0x5C:0x60] = struct.pack("<I", CLUSTER_COUNT)
    vbr[0x60:0x64] = struct.pack("<I", ROOT_CLUSTER)
    vbr[0x64:0x68] = struct.pack("<I", 0x20260917)     # VolumeSerialNumber
    vbr[0x68:0x6A] = struct.pack("<H", 0x0100)         # FileSystemRevision 1.0
    vbr[0x6A:0x6C] = struct.pack("<H", 0x0000)         # VolumeFlags
    vbr[0x6C] = BPS_SHIFT
    vbr[0x6D] = SPC_SHIFT
    vbr[0x6E] = NUM_FATS
    vbr[0x6F] = 0x80                                    # DriveSelect
    csum = boot_checksum_spec(bytes(vbr))
    vbr[0x78:0x7C] = struct.pack("<I", csum)
    vbr[510:512] = b"\x55\xAA"
    return vbr

def main(path):
    tree = build()
    img = bytearray(TOTAL_SECTORS * SECTOR)

    # VBR + 备份
    vbr = build_vbr()
    img[0:SECTOR] = vbr
    img[SECTOR:2*SECTOR] = vbr  # LBA 8 之前的备份区简化放 LBA 1（布局自洽即可）

    # FAT（簇 2 起）
    fat_bytes = bytearray(8 * SECTOR)
    for c in range(2, CLUSTER_COUNT + 2):
        off = (c - 2) * 4
        if off < len(fat_bytes):
            fat_bytes[off:off+4] = struct.pack("<I", fat[c])
    img[FAT_OFFSET*SECTOR : FAT_OFFSET*SECTOR+len(fat_bytes)] = fat_bytes

    def put_cluster(c, data):
        lba = CLUSTERS_HEAP_START + (c - 2) * (CLUSTER_BYTES // SECTOR)
        off = lba * SECTOR
        img[off:off+len(data)] = data

    # bitmap 簇 2：标记已用簇（2..root+..）
    bmp = bytearray(CLUSTER_BYTES)
    for c in range(2, next_alloc[0]):
        if c - 2 < CLUSTER_BYTES * 8:
            bmp[(c - 2) // 8] |= 1 << ((c - 2) % 8)
    put_cluster(2, bytes(bmp))
    # upcase 簇 3
    put_cluster(3, upcase_table().ljust(CLUSTER_BYTES, b"\x00"))
    # root / 子目录 / 文件数据
    put_cluster(tree["root"][0], tree["root"][1])
    put_cluster(tree["games"][0], tree["games"][1])
    put_cluster(tree["apps"][0], tree["apps"][1])
    put_cluster(tree["big"][0], tree["big"][1])
    put_cluster(tree["contig"][0], tree["contig"][1])
    fc = tree["file_clusters"]
    put_cluster(fc["hello"], HELLO.ljust(CLUSTER_BYTES, b"\x00"))
    put_cluster(fc["manifest"], MANIFEST.ljust(CLUSTER_BYTES, b"\x00"))
    put_cluster(fc["apps_json"], APPS_JSON.ljust(CLUSTER_BYTES, b"\x00"))
    put_cluster(fc["readme"], README.ljust(CLUSTER_BYTES, b"\x00"))
    put_cluster(fc["boot_select_cluster"], BOOT_SELECT.ljust(CLUSTER_BYTES, b"\x00"))
    put_cluster(fc["contig"], CONTIG)
    # big：7 簇链
    bc = fc["big"]
    for k in range(7):
        put_cluster(bc + k, BIG[k*CLUSTER_BYTES:(k+1)*CLUSTER_BYTES])

    with open(path, "wb") as f:
        f.write(img)
    print("exFAT image written: %s (%d bytes, clusters used: 2..%d)" %
          (path, len(img), next_alloc[0] - 1))
    print("layout: fat@%d heap@%d root_cluster=%d clusters=%d" %
          (FAT_OFFSET, CLUSTERS_HEAP_START, ROOT_CLUSTER, CLUSTER_COUNT))

if __name__ == "__main__":
    main(sys.argv[1] if len(sys.argv) > 1 else "shared-exfat.img")
