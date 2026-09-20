#!/usr/bin/env python3
"""需求 5：把根目录的非功能性过程产物整理进 _attic。

原则（红线）：
  * 只**移动**，绝不删除 —— 全部可逆。
  * 只动「过程产物」：走查截图、跑测日志、QEMU 残留、临时镜像/固件变量、
    会话归档包、过程清单。
  * 明确**不动**：vite MPA 入口 html（app-*.html / desktop.html / taskbar.html /
    explorer.html / datavault.html）、工程配置、构建脚本、README/CHANGELOG、
    以及被 docs/acceptance/*.py 与 README 依赖的交付产物 varix.iso /
    varix.img / varix-qemu.iso。
  * 目标不存在时才移动；已存在同名则加时间戳后缀，绝不覆盖。
"""
import os
import shutil
import sys
import time

ROOT = os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
ATTIC = os.path.join(ROOT, "_attic")

FILES = [
    # 走查截图 / 中间图
    "bs-hl-crop.png", "bs-k0.png", "bs-k1.png", "bs1.png", "bs2.png",
    "bs1.ppm", "bs2.ppm", "ca-v6b-sel.png",
    # 跑测日志
    "cow-test.log", "int.log", "int2.log", "pf-test.log", "qs-bs.log",
    "qs-uefi4.log", "ring3-test.log", "ring3b.log",
    # QEMU 残留
    "bs-mon.sock", "qemu-bs.err", "qemu-bs.pid",
    # 临时测试镜像 / 固件变量（全仓无引用，已核实）
    "nvme0.img", "ovmf-vars.fd",
    # npm audit 输出
    "_attic_npm_audit_err.txt",
    # 过程清单（非功能文档）
    "UNREAL-X-15000-未完成清单.md",
]

ARCHIVE_DIRS = sorted(
    d for d in os.listdir(ROOT)
    if d.startswith("archive-") and os.path.isdir(os.path.join(ROOT, d))
)


def move(src_name: str, dest_sub: str) -> str:
    src = os.path.join(ROOT, src_name)
    if not os.path.exists(src):
        return "SKIP(missing) %s" % src_name
    dest_dir = os.path.join(ATTIC, dest_sub)
    os.makedirs(dest_dir, exist_ok=True)
    dest = os.path.join(dest_dir, src_name)
    if os.path.exists(dest):
        stamp = time.strftime("%Y%m%d-%H%M%S")
        base, ext = os.path.splitext(src_name)
        dest = os.path.join(dest_dir, "%s.%s%s" % (base, stamp, ext))
    shutil.move(src, dest)
    return "MOVED %s -> _attic/%s/%s" % (src_name, dest_sub, os.path.basename(dest))


def main() -> int:
    print("=== 需求 5：非功能性产物归入 _attic（只移动，不删除）===")
    n = 0
    for f in FILES:
        r = move(f, "tidy")
        print("  " + r)
        if r.startswith("MOVED"):
            n += 1
    for d in ARCHIVE_DIRS:
        r = move(d, "tidy")
        print("  " + r)
        if r.startswith("MOVED"):
            n += 1
    print("\n共移动 %d 项。" % n)
    print("\n=== 保留在根目录（功能性 / 被脚本依赖）===")
    keep = ["desktop.html", "taskbar.html", "explorer.html", "datavault.html",
            "app-write.html", "app-mind.html", "app-code.html", "app-fate.html",
            "limine.conf", "varix.iso", "varix.img", "varix-qemu.iso",
            "build-windows.bat", "package.json", "vite.config.ts", "tsconfig.json",
            "README.md", "CHANGELOG.md"]
    for k in keep:
        print("  %-24s %s" % (k, "在" if os.path.exists(os.path.join(ROOT, k)) else "缺失!"))
    return 0


if __name__ == "__main__":
    sys.exit(main())
