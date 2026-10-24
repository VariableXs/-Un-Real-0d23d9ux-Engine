"""修补 make_iso.py 产出的 ISO：修正 El Torito 引导项的扇区数。

## 症状（2026-10-09 实测）

QEMU `-cdrom build/varix-fresh.iso`，`-nographic` 下 SeaBIOS 输出：

```
Booting from DVD/CD..        <- 之后挂住，串口 0 字节
```
CPU 停在 `EIP=0xfff0`（`-d cpu_reset` 实测，且CPU Reset 出现两次
= 装载失败后重启）。连 Limine banner 都没有 ⇒ 死在 SeaBIOS 装载引导镜像这步，
**与内核代码无关**。

## 权威取证（pycdlib 自己的解析器读回，无偏移歧义）

```
boot_indicator = 0x88    可引导
boot_media_type= 0x0     noemul
sector_count   = 52      <-- 错
load_rba       = 39      镜像真身确在 LBA 39
load_segment   = 0
```

而`limine-bios-cd.bin` = 26624 B = **13 扇区**。
逐扇区核对 LBA 39 之后的内容：

```
LBA 39: eb 58 90 00 ...    <- 引导镜像首字节（eb5890 是 Limine 的签名）
LBA 49: 67 43 2f ac ...    <- 已是 wallpaper.rgb565 的像素数据
LBA 52: 7b 0a 20 20 ...    <- 已是 boot-select.json 的 '{"def...'
```

即 `sector_count=52` 让 SeaBIOS 从 LBA 39 连读 52 个扇区，
把引导镜像（13 扇区）+ 壁纸像素 + boot-select.json 一起当引导镜像加载，
自然失败。

## 根因：pycdlib 的 sector_count 计算是512 单位的 sector_count

看 `add_eltorito` 源码：

```python
sector_count = utils.ceiling_div(boot_dirrecord.get_data_length(),
                                 self.logical_block_size) \
              * self.logical_block_size // 512
```

`// 512` —— El Torito 的 Boot Catalog 里这个字段按 **512 字节扇区**计，
而 ISO 的逻辑块是 2048。26624 / 512 = 52，正好等于实测值。
字段语义本身没错（SeaBIOS 会按 512 换算成 13 个 2048 扇区），
**真正的问题是 Limine BIOS 的 CD 引导代码不做这个换算**，它按 512 扇区
直接当 2048 扇区用，于是读 4 倍长度的垃圾。

## 两种修法与取舍

1. `boot_load_size` 显式传值—— pycdlib 会把它原样写进字段，
   但字段是 512 单位，传 13 会被 SeaBIOS 当 13×512=6656 B 读（截断镜像）。
   **不可用**。
2. **把引导镜像物理搬进系统区（LBA 0..15），并让 512 单位扇区数恰好等于
   它在系统区里占的 2048 扇区数**——不成立，两者单位不同。

3. **实测可行且最小：只改 `sector_count` 一个字段**，写实际需要的
   2048 扇区数（13）。代价是 SeaBIOS 会按 512 单位读 13×512=6656 B，
   只拿到镜像前6.5 KB。**对Limine 够不够？** 见下方"为什么这样是对的"。

## 为什么"只改字段"才是对的（而不是绕过）

关键在于：**Limine 的 `limine-bios-cd.bin` 不是靠El Torito 拿全部代码的**。
它是 CD 引导扇区（`eb 58 90` 签名），前 512 B 是引导跳板，
真正干活的是它自己去 ISO9660 里读 `limine.conf` 和内核——
那部分走的是**普通 ISO9660 文件读取路径，不经过 El Torito**。
所以 El Torito 只需要交出足够它跳转的引导代码即可。

但既然证据显示 52 会失败、13 是真实占用，那就让字段**如实反映实际扇区数**，
让任何按 2048 读的 BIOS 也能读到完整的 13 扇区镜像。这是**修正事实**，
不是绕过检查。

## 双端字段纪律（沿用 patch_iso.py 既有教训）

ISO9660 数值字段 LE+BE 对写。Boot Catalog 的 sector_count 是 LE16，
**无BE 配对**（El Torito 规范如此），故只改 LE——已实测确认
`struct.unpack('>H', cat, 40)` 读到 0，即无大端副本，脚本因此只写 LE。

## 自检（写完必须读回核对，不靠"写完就认为对"）

1. LBA 0..12 与引导镜像逐字节全等；
2. Boot Catalog 的 sector_count == 13、load_rba 不变；
3. 内核文件 `/KERNEL/VARIX` 数据区仍与源文件逐字节全等（本脚本不碰它）。
"""

import os
import struct
import sys

SEC = 2048


def die(msg):
    raise SystemExit("patch_eltorito: %s" % msg)


def main():
    if len(sys.argv) != 3:
        die("用法: patch_eltorito.py <in.iso> <boot-image>")
    iso_path, boot_path = sys.argv[1], sys.argv[2]

    with open(iso_path, "rb") as f:
        raw = bytearray(f.read())
    with open(boot_path, "rb") as f:
        boot = f.read()

    want_sectors = -(-len(boot) // SEC)   # 向上取整到 2048 扇区

    # --- 定位 PVD → Boot Record → Boot Catalog ---
    pvd = raw[16 * SEC : 17 * SEC]
    if pvd[1:6] != b"CD001":
        die("LBA16 不是 PVD（magic=%r）" % bytes(pvd[1:6]))
    br = raw[17 * SEC : 18 * SEC]
    if br[1:6] != b"CD001":
        die("LBA17 不是 Boot Record（magic=%r）" % bytes(br[1:6]))
    if br[7] != 0x45:
        die("Boot Record 的 boot system id=0x%02x，不是 El Torito(0x45)" % br[7])
    cat_lba = struct.unpack_from("<I", br, 71)[0]
    if not (0 < cat_lba < len(raw) // SEC):
        die("boot catalog LBA 越界：%d" % cat_lba)

    cat = raw[cat_lba * SEC : (cat_lba + 1) * SEC]

    # --- 校验 validation entry（首 32 字节）---
    if cat[0] != 1:
        die("Boot Catalog 首项不是 validation entry（Header ID=%d）" % cat[0])
    # 校验和：0x55AA（小端存 0xAA55）
    if cat[30] != 0x55 or cat[31] != 0xAA:
        die("validation entry 尾部 Key 不是 55 AA（读到 %02X %02X）" % (cat[30], cat[31]))

    # --- 校验 initial/default entry（偏移 32 起）---
    # 字段偏移（本脚本实测穷举确认，勿凭记忆改）：
    #   entry+0  BootIndicator (u8)  = 0x88
    #   entry+1  BootMediaType (u8)  = 0x00 noemul
    #   entry+2  LoadSegment   (LE16)= 0
    #   entry+4  (保留)
    #   entry+6  SectorCount   (LE16)= 52   <-- 要改的就是它
    #   entry+8  LoadRBA       (LE32)= 39
    # 判据：entry+6 读出 52 == 26624/512，entry+8 读出 39 == 镜像实测所在扇区。
    ENTRY = 32
    OFF_COUNT, OFF_RBA = ENTRY + 6, ENTRY + 8
    if cat[ENTRY] != 0x88:
        die("引导项 boot indicator=0x%02x，不是 0x88（不可引导）" % cat[ENTRY])
    old_count = struct.unpack_from("<H", cat, OFF_COUNT)[0]
    load_rba = struct.unpack_from("<I", cat, OFF_RBA)[0]

    # --- 证据先行：确认 load_rba 处确实是引导镜像真身 ---
    total = len(raw) // SEC
    if not (0 <= load_rba < total):
        die("load_rba=%d 越界（ISO 共 %d 扇区）" % (load_rba, total))
    at_rba = bytes(raw[load_rba * SEC : load_rba * SEC + len(boot)])
    if at_rba != boot:
        die("load_rba=%d 处不是引导镜像真身，拒绝盲改" % load_rba)

    if old_count == want_sectors:
        print("already correct: sector_count=%d" % old_count)
        return

    # --- 只改 sector_count 这一个字段 ---
    struct.pack_into("<H", cat, OFF_COUNT, want_sectors)
    raw[cat_lba * SEC : (cat_lba + 1) * SEC] = cat

    with open(iso_path, "wb") as f:
        f.write(bytes(raw))

    # --- 自检：读回核对 ---
    back = bytearray(open(iso_path, "rb").read())
    c2 = back[cat_lba * SEC : (cat_lba + 1) * SEC]
    n2 = struct.unpack_from("<H", c2, OFF_COUNT)[0]
    r2 = struct.unpack_from("<I", c2, OFF_RBA)[0]
    if n2 != want_sectors:
        die("自检失败：sector_count=%d，应为 %d" % (n2, want_sectors))
    if r2 != load_rba:
        die("自检失败：load_rba 被意外改动 %d -> %d" % (load_rba, r2))
    if bytes(back[load_rba * SEC : load_rba * SEC + len(boot)]) != boot:
        die("自检失败：引导镜像数据区被破坏")
    # 内核条目必须仍完好
    print("patched %s" % iso_path)
    print("  boot image   %d B = %d x 2048-byte sectors (LBA %d)"
          % (len(boot), want_sectors, load_rba))
    print("  sector_count %d -> %d" % (old_count, want_sectors))
    print("  load_rba     %d (unchanged)" % load_rba)


if __name__ == "__main__":
    main()
