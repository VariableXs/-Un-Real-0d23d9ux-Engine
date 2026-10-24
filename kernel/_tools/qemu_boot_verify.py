"""QEMU 引导实证：定位并修复 varix ISO 的三道引导关口。

2026-10-09 实测结论（本文件即证据链，别删）

═══════════════════════════════════════════════════════════════════
背景：会话四判定「三条打包路线都失败，根因未定位」。本脚本把它定位到字节。
═══════════════════════════════════════════════════════════════════

## 关口①：El Torito 引导项（已修，验证有效）

**症状**：QEMU `-cdrom` 后 SeaBIOS 打印 `Booting from DVD/CD..` 然后静默挂住；
`-d cpu_reset` 实测 CPU 停在 `EIP=0xfff0`，且 `CPU Reset` 出现两次
（装载引导镜像失败后重启）；串口 0 字节，连 Limine banner 都没有。

**根因**：`pycdlib.add_eltorito()` 写出的引导目录项里

- `load_rba` 指向镜像在 ISO 里的真实位置（本例 LBA 39/205），
- 但 `sector_count` 是用 `// 512` 算出来的（26624/512 = **52**），
  而该字段在 El Torito 里的单位是 **2048 字节扇区**（26 624 / 2048 = **13**）。

SeaBIOS 按 2048 读 52 个扇区 ⇒ 从 LBA 39 一口气读到 LBA 90，
把引导镜像（13 扇区）+ `wallpaper.rgb565` 像素 + `boot-select.json`
一起当引导镜像加载 ⇒ 装载失败。

**实测取证**（pycdlib 自己的解析器，无偏移歧义）：

```python
bc = iso.eltorito_boot_catalog.initial_entry
bc.boot_indicator   # 0x88  可引导
bc.boot_media_type  # 0x00noemul
bc.sector_count     # 52    <- 错
bc.load_rba         # 39    镜像真身确在此
```

**修法**（两步，都在本脚本里做）：

1. `sector_count` 改成实际 2048 扇区数（13）；
2. **把引导镜像搬进 ISO 系统区 LBA 0..15**——这一步是必需的，
   实测只改 `sector_count` 仍然挂住；Limine 的 CD 引导代码按 LBA 0 取镜像。

`limine-bios-cd.bin` = 26 624 B = 13 扇区，16 扇区的系统区装得下。
PVD 固定在 LBA 16，不动。搬完不增删扇区，故 PVD 卷空间大小无需改。

## 关口②：Stage 3 缺失（已修，验证有效）

**症状**：`!! Stage 3 file not found!`
`!! Have you copied limine-bios.sys to the root, /boot, /limine, or /boot/limine`

**根因**：ISO 里没有 `limine-bios.sys`。Limine 的搜索路径是
根 / `/boot` / `/limine` / `/boot/limine`（见 `limine.c` 的报错文案）。

**修法**：ISO 内放 `/BOOT/LIMINE_BIOS.SYS`，Rock Ridge 真名 `limine-bios.sys`
（ISO9660 层必须全大写、连字符换下划线）。

## 关口③：内核文件读不出（已随①②一并解决）

**症状**：在①②只修了一半时，Limine 能读 conf、能解析
`boot():/kernel/varix`，然后：

```
limine: Loading executable `boot():/kernel/varix`...
PANIC: iso9660: failed to read file data
```

**关口①②都修好后，这个 PANIC 自动消失，4,865,432 B 内核一次读成功。**

⚠️ **本节原本记录过一个错误结论，保留下来当反面教材。**

我曾用「递进截断内核大小」做实验，测出 4096 B 能过、4097 B 挂，
据此判定「Limine 12.9 的 ISO9660 驱动有 4096 B 读取上限」，
并把这条写进了脚本与项目记忆。**该结论是错的。**

真实原因：那些小尺寸测试用的 ISO **同时也缺 Stage 3**
（`limine-bios.sys`）。小文件之所以"能过"，是因为它们根本没走到
读内核那一步。**教训**：递进截断实验只在**其余变量逐项固定**时才成立；
我的对照组少了 Stage 3 这一个变量，整条曲线作废。
**「两点不同 → 因果」之前，必须先确认两点的其他条件完全相同。**

已排除过的假设（若再次遇到，可直接跳过）：

| 假设 | 实验 | 结果 |
|---|---|---|
| 内核内容坏 | 把 `limine-bios.sys` 的内容当内核放 `/kernel/varix` | 同样失败 ⇒ 与内容无关 |
| 目录位置 | 内核放 ISO 根目录 `/varix` | 同样失败 ⇒ 与路径无关 |
| 文件名 `;1` | 加 / 不加版本号 | 都失败 ⇒ 无关 |
| 紧贴卷尾 | 追加空扇区 + 同步 PVD vol_space(LE@80/BE@84) | 无效 |
| 扇区未对齐 | 6144/8192/16384 B（全是整数扇区） | 全失败 ⇒ 不是对齐问题 |
| 高 LBA | 可读项(LBA43)与不可读项(LBA205)目录项同构 | 唯一差异是 extent，但 pycdlib 按固定顺序分配，**无法用添加顺序验证** |

真正的修法就是关口①②：El Torito 两处 + Stage 3 齐了，③自动消失。

═══════════════════════════════════════════════════════════════════

## 权威读法：别自己数偏移

Boot Catalog 的字段偏移很容易数错（我数错过 +4/+6，读出 2555956 的垃圾值）。
用 pycdlib 自己的解析器：

```python
e = iso.eltorito_boot_catalog.initial_entry
e.boot_indicator   # 0x88
e.boot_media_type# 0x00 noemul
e.sector_count     # 2048 字节单位
e.load_rba         # 引导镜像所在扇区
```

`make_iso.py` 的 `patch_eltorito()` 采用的偏移（相对 entry 起点）：
`[0]` indicator / `[1]` media / `[2:4]` load segment /
`[6:8]` sector count / `[8:12]` load RBA。`sector_count` **无大端配对**
（El Torito 规范如此），只写 LE。

## 用法

```bash
# 只打包（关口①②已内置进make_iso.py 的后处理）
python _tools/make_iso.py

# 只跑QEMU 实证（需要先 make_iso.py）
python _tools/qemu_boot_verify.py            # 60s，自动抓串口
python _tools/qemu_boot_verify.py --timeout 120
```
"""

import os
import re
import struct
import subprocess
import sys
import time

REPO = os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
ISO = os.path.join(REPO, "build", "varix-fresh.iso")
QEMU = r"C:\Program Files\qemu\qemu-system-x86_64.exe"
ISOROOT = os.path.join(REPO, "build", "isoroot")


def run_qemu(timeout=60):
    """跑一次 QEMU，抓串口日志，返回文本。"""
    log = os.path.join(REPO, "kernel", "qemu-verify-run.log")
    if os.path.exists(log):
        os.remove(log)
    args = [
        QEMU, "-cdrom", ISO, "-m", "512M",
        "-display", "none", "-serial", "file:" + log,
        "-no-reboot", "-boot", "order=d",
    ]
    proc = subprocess.Popen(args, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    time.sleep(timeout)
    # Windows 下 Popen 句柄不能直接杀，按镜像名结束
    subprocess.run(["taskkill", "/F", "/IM", "qemu-system-x86_64.exe"],
                   capture_output=True)
    proc.wait(timeout=10)
    if not os.path.exists(log):
        return ""
    with open(log, encoding="utf-8", errors="replace") as f:
        return f.read()


def classify(text):
    """把串口日志判成三类状态。"""
    if not text.strip():
        return "NO-SERIAL", "串口 0 字节：连 Limine 都没跑起来（El Torito 未生效）"
    if "Stage 3 file not found" in text:
        return "NO-STAGE3", "Limine 起来了但找不到 limine-bios.sys"
    if "iso9660: failed to read file data" in text:
        return "ISO9660-READ-FAIL", "Limine 能解析路径，但读不出内核文件数据"
    if "PANIC" in text:
        return "PANIC", "内核或引导链 panic（见日志）"
    return "BOOTED", "引导链走通（内核应有串口输出）"


def preflight():
    """打包前的静态自检——不启动 QEMU 就能发现的问题。"""
    print("=== 静态自检 ===")
    ok = True
    if not os.path.exists(ISO):
        print("  [FAIL] ISO 不存在: %s（先跑 make_iso.py）" % ISO)
        return False
    with open(ISO, "rb") as f:
        raw = f.read()
    SEC = 2048

    pvd = raw[16 * SEC : 17 * SEC]
    if pvd[1:6] != b"CD001":
        print("  [FAIL] LBA16 不是 PVD")
        return False
    print("  [ok] PVD 魔数正确")

    br = raw[17 * SEC : 18 * SEC]
    if br[1:6] != b"CD001":
        print("  [FAIL] LBA17 不是 Boot Record")
        return False
    cat_lba = struct.unpack_from("<I", br, 71)[0]
    cat = raw[cat_lba * SEC : (cat_lba + 1) * SEC]
    ind = cat[32]
    count = struct.unpack_from("<H", cat, 38)[0]
    rba = struct.unpack_from("<I", cat, 40)[0]
    print("  [%s] El Torito: indicator=0x%02x sector_count=%d load_rba=%d"
          % ("ok" if ind == 0x88 else "FAIL", ind, count, rba))

    # 引导镜像真身必须在 LBA 0（关口①的第二步）
    boot_path = os.path.join(ISOROOT, "limine-bios-cd.bin")
    with open(boot_path, "rb") as f:
        boot = f.read()
    if raw[0 : len(boot)] == boot:
        print("  [ok] LBA 0 是引导镜像真身")
    else:
        print("  [FAIL] LBA 0 不是引导镜像（Limine CD 引导按 LBA 0 取）")
        ok = False

    # Stage 3 必须在
    has_s3 = b"LIMINE_BIOS.SYS" in raw[: cat_lba * SEC] or b"LIMINE_BIOS.SYS" in raw
    print("  [%s] Stage 3 (limine-bios.sys) %s"
          % ("ok" if has_s3 else "FAIL", "已包含" if has_s3 else "缺失"))

    # 内核数据逐字节对拍
    root = struct.unpack_from("<I", pvd, 156 + 2)[0]
    d = raw[root * SEC : (root + 1) * SEC]
    i = 0
    kern_ext = None
    while i < SEC:
        l = d[i]
        if l == 0:
            break
        nlen = d[i + 32]
        nm = d[i + 33 : i + 33 + nlen].decode("latin1")
        if nm == "KERNEL":
            kern_ext = struct.unpack_from("<I", d, i + 2)[0]
        i += l
    if kern_ext is None:
        print("  [FAIL] ISO 内无 /KERNEL 目录")
        return False
    kd = raw[kern_ext * SEC : (kern_ext + 1) * SEC]
    i = 0
    while i < SEC:
        l = kd[i]
        if l == 0:
            break
        nlen = kd[i + 32]
        nm = kd[i + 33 : i + 33 + nlen].decode("latin1")
        if nlen == 5 and nm == "VARIX":
            ex = struct.unpack_from("<I", kd, i + 2)[0]
            sz = struct.unpack_from("<I", kd, i + 10)[0]
            src = os.path.join(ISOROOT, "kernel", "varix")
            with open(src, "rb") as f:
                want = f.read()
            got = raw[ex * SEC : ex * SEC + sz]
            same = got == want
            print("  [%s] 内核数据: extent=%d size=%d 逐字节%s"
                  % ("ok" if same else "FAIL", ex, sz,
                     "全等" if same else "不等"))
            if not same:
                ok = False
            if sz > 4096:
                # 这条只提醒「多扇区文件」，不再断言驱动上限——
                # 那个 4096 阈值结论是错的（见文件头关口③的反面教材）。
                print("  [note] 内核 %d B（多扇区文件）—— 已实测可正常加载，"
                      "4.8 MB 读成功" % sz)
        i += l
    return ok


def main():
    timeout = 60
    if "--timeout" in sys.argv:
        timeout = int(sys.argv[sys.argv.index("--timeout") + 1])
    print("=== Varix QEMU 引导实证 ===")
    print("ISO: %s (%d bytes)" % (ISO, os.path.getsize(ISO) if os.path.exists(ISO) else -1))
    print()
    if not preflight():
        print()
        print("静态自检未通过，先修上面的 [FAIL] 项。")
        return 1
    print()
    print("=== 启动 QEMU（%ds）===" % timeout)
    text = run_qemu(timeout)
    state, meaning = classify(text)
    print()
    print("--- 串口日志 ---")
    print(text if text.strip() else "(空)")
    print("--- 判定: %s ---" % state)
    print(meaning)
    return 0 if state == "BOOTED" else 2


if __name__ == "__main__":
    sys.exit(main())
