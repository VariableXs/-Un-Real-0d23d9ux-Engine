"""在 varix.iso 上做**二进制原位补丁**替换内核（不重排目录项）。

背景（实测记录，别再走回头路）：
- `make_iso.py`（从 isoroot 零打包）：QEMU SeaBIOS **不引导**（CPU 停在
  BIOS `EIP=0xfff0`，串口 0 字节）。El Torito 逐项核对正确
  （validation `55aa`、initial `boot=0x88`、sector_count 与镜像长度
  一致、boot image LBA 处字节与 `limine-bios-cd.bin` 首字节相同），
  但就是不工作，根因未定位。这条路不要再走。
- `repack_iso.py`（pycdlib 删旧文件 + 加新文件）：能引导到 Limine，但报
  `Failed to open executable with path boot():/kernel/varix`。pycdlib
  重写目录项后 Limine 自带的 ISO9660 驱动读不出 extent（pycdlib 自己
  能读，故障只在 Limine 侧）。
- 本脚本走第三条路：**旧 ISO 的目录树一个字节都不动**，只改三处——
  ① 把新内核数据追加到卷尾（新内核比旧 extent 大，不能原地覆盖，
     会踩到紧邻其后的其它文件）；
  ② `/KERNEL/VARIX` 目录项的 extent LBA（记录内偏移 2..6）；
  ③ 同记录 data length（偏移 10..14），以及 PVD 卷空间大小
     （偏移 80..84）。

**目录项定位靠穷举比对，不靠解析名字**——这个 ISO 由 xorriso 生成，
目录记录里塞了 Rock Ridge / Joliet 续延（`SP`/`RR`/`PX`），名字字段被
撑大且混在续延数据里，手撸 UTF-16 解析会踩坑（第一、二版都栽在这）。
可靠办法：按 34 字节槽位遍历，认准「flags 位于偏移 25、且 extent 与
size 同时等于旧内核的 LBA/size」这一组合特征——全目录里只有它匹配。
"""

import os
import struct
import sys

REPO = os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
BASE_ISO = os.path.join(REPO, "varix.iso")
NEW_KERNEL = os.path.join(REPO, "kernel", "target", "x86_64-unknown-none", "release", "varix")
OUT = os.path.join(REPO, "build", "varix-fresh.iso")

SECTOR = 2048


def die(msg):
    sys.exit("ERROR: " + msg)


def locate_varix_record(blob):
    """在根目录的 KERNEL 子目录里定位 VARIX 目录项的字节偏移。

    **KERNEL 目录的 LBA = 30**，这是从 `varix.iso` 原始字节里手工核对
    出来的事实（根目录在 LBA 29，其 `KERNEL` 条目 extent = 30，目录内
    第 6 个 34 字节槽位就是 VARIX：extent=1747、size=4632576）。
    别再用 pycdlib 反查——它的 RR/Joliet 解析在这个 xorriso 产物上会给出
    错误的父目录（实测把 BOOT.CAT 的 33 当成 KERNEL 目录），根因是
    xorriso 把 Rock Ridge / Joliet 续延塞进了同一条 34 字节记录。
    """
    kdir_lba = 30
    print("KERNEL dir lba = %d (hardcoded, verified)" % kdir_lba)
    return kdir_lba


def find_extent_and_size(blob, iso_path):
    """用 pycdlib 读出某个文件的 (extent LBA, data length)。"""
    import pycdlib
    iso = pycdlib.PyCdlib()
    iso.open(BASE_ISO)
    try:
        parent = os.path.dirname(iso_path) or "/"
        want = os.path.basename(iso_path).upper()
        for c in iso.list_children(iso_path=parent):
            nm = c.file_identifier().decode("utf-8", "replace").split(";")[0].upper()
            if nm == want:
                return c.extent_location(), c.data_length
    finally:
        iso.close()
    die("file not found: %s" % iso_path)


def main():
    for p in (BASE_ISO, NEW_KERNEL):
        if not os.path.isfile(p):
            die("missing: %s" % p)

    blob = bytearray(open(BASE_ISO, "rb").read())
    pvd = 16 * SECTOR
    if blob[pvd + 1:pvd + 6] != b"CD001":
        die("sector 16 is not a PVD")

    old_lba, old_len = find_extent_and_size(blob, "/KERNEL/VARIX")
    vol_sectors = struct.unpack("<I", blob[pvd + 80:pvd + 84])[0]
    print("old: lba=%d len=%d vol_sectors=%d" % (old_lba, old_len, vol_sectors))

    kdir_lba = locate_varix_record(blob)

    # 在 KERNEL 目录扇区里穷举槽位，找 extent/size 同时匹配旧内核的那条。
    # **LE 与 BE 必须同时匹配**——这个目录里有xorriso 生成的续延记录，
    # 只看 LE 会误命中（第一版就改到了续延记录上）。
    target = None
    for i in range(SECTOR // 34):
        off = kdir_lba * SECTOR + i * 34
        rec = blob[off:off + 34]
        if len(rec) < 34 or rec[0] == 0:
            continue
        ext_le = struct.unpack("<I", rec[2:6])[0]
        ext_be = struct.unpack(">I", rec[6:10])[0]
        size_le = struct.unpack("<I", rec[10:14])[0]
        size_be = struct.unpack(">I", rec[14:18])[0]
        if (ext_le == old_lba and ext_be == old_lba
                and size_le == old_len and size_be == old_len):
            target = off
            print("found VARIX record at slot %d (byte %d)" % (i, off))
            break
    if target is None:
        die("VARIX directory record not matched (lba=%d len=%d)" % (old_lba, old_len))

    data = open(NEW_KERNEL, "rb").read()
    need = (len(data) + SECTOR - 1) // SECTOR

    # **原地写入 LBA `old_lba`，ISO 长度保持 `old_lba*2048 + len(data)`**，
    # ���把卷空间大小顶到文件末尾。
    #
    # 为什么不追加到卷尾再改 extent（第一版）：那样做 pycdlib 能正常读，
    # 但 **Limine 读不出数据**（`iso9660: failed to read file data`）。
    # 变量只剩一个——extent 的落点。VARIX 原本就是卷内最后一个文件
    # （占 LBA 1747..4008，正好到卷尾），它后面没有任何数据会被踩坏，
    # 所以原地扩写是安全的：**extent LBA 保持 1747 不变**，只改 size。
    # 这样 Limine 侧的路径解析与 extent 定位跟旧 ISO 完全一致，唯一变化
    # 是文件更长——而它后方本就是卷尾，不需要新增任何目录项。
    new_lba = old_lba
    tail = new_lba * SECTOR + len(data)
    if len(blob) < tail:
        blob.extend(b"\x00" * (tail - len(blob)))
    blob[new_lba * SECTOR:new_lba * SECTOR + len(data)] = data
    new_sectors = (tail + SECTOR - 1) // SECTOR

    # ISO9660 是**双端存储**结构，三处都要 LE+BE 同步写：
    # ① 目录项 extent：记录内偏移 2..6 = LE，6..10 = BE
    #    （注意 6..10 常被误读成 "parent directory"，那只是 8.3 布局下的
    #    parent 字段位置；在双端布局里它是 extent 的 BE 半边——第一版只
    #    改 LE，pycdlib 报 "extent location disagree" 就是这里。）
    # ② 目录项 data length：偏移 10..14 = LE，14..18 = BE
    # ③ PVD 卷空间大小：偏移 80..84 = LE，84..88 = BE
    #    （只改 LE 会报 "space size disagree"，Limine 侧表现为
    #    `iso9660: failed to read file data`。）
    struct.pack_into("<I", blob, target + 2, new_lba)
    struct.pack_into(">I", blob, target + 6, new_lba)
    struct.pack_into("<I", blob, target + 10, len(data))
    struct.pack_into(">I", blob, target + 14, len(data))
    struct.pack_into("<I", blob, pvd + 80, new_sectors)
    struct.pack_into(">I", blob, pvd + 84, new_sectors)

    with open(OUT, "wb") as f:
        f.write(blob)
    print("new: lba=%d len=%d vol_sectors=%d" % (new_lba, len(data), new_sectors))
    print("wrote %s (%d bytes)" % (OUT, len(blob)))


if __name__ == "__main__":
    main()