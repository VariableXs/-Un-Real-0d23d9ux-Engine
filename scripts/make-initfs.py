#!/usr/bin/env python3
"""VARIABLE-200 F180 · initrd 打包流水线（一键脚本）。

把用户程序 / 应用 / 资产 / 配置 / 驱动自动打包成 Linux 内核可直接吃下的
`newc` cpio 归档（initramfs），供 Limine -> 内核 -> init(pid1) 在挂在只读根。

产出：
  build/initrd.img              归档本体
  build/isoroot/initrd.img      若 isoroot 存在，顺带放进去（make-iso.sh 会打进 ISO）

用法：
  python scripts/make-initfs.py                    # 用内置默认清单（可在无源码时跑通）
  python scripts/make-initfs.py --root kernel/userspace --root build/assets
  python scripts/make-initfs.py --out build/initrd.img --verbose
  python scripts/make-initfs.py --check            # 只校验已有归档的目录表

纪律：纯标准库；不依赖任何第三方包；不变量——目录表项一律以 '/' 开头、
无重复名、载荷按 4 字节对齐（cpio newc 硬要求）。
"""

from __future__ import annotations

import argparse
import os
import struct
import sys

REPO_ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
DEFAULT_OUT = os.path.join(REPO_ROOT, "build", "initrd.img")
ISOROOT = os.path.join(REPO_ROOT, "build", "isoroot")

# 归档中的类别前缀（与 kernel/varix/src/bootchain.rs 的 standard_initrd 对齐）。
CATEGORIES = ("programs", "assets", "etc", "drivers")

# 内置默认清单：源文件缺失时用占位载荷，保证流水线今天就能端到端跑通。
DEFAULT_MANIFEST = (
    ("/programs/init", "kernel/userspace/init/target/x86_64-varix/release/init"),
    ("/programs/fileman", "kernel/userspace/apps/fileman/target/x86_64-varix/release/fileman"),
    ("/assets/icons/app-128.ico", "assets/icons/app-128.ico"),
    ("/assets/themes/dark.toml", "assets/themes/dark.toml"),
    ("/etc/firstboot.toml", "assets/etc/firstboot.toml"),
    ("/drivers/virtio_gpu.vxd", "kernel/userspace/drivers/virtio_gpu/target/x86_64-varix/release/virtio_gpu.vxd"),
)

# 扩展名 -> 归档目录 的归类规则（--root 扫描模式用）。
EXT_CATEGORY = {
    ".toml": "etc",
    ".conf": "etc",
    ".kbl": "etc",
    ".ico": "assets",
    ".png": "assets",
    ".ttf": "assets",
    ".vxd": "drivers",
    ".ko": "drivers",
    ".elf": "programs",
    "": "programs",
}

MODE_FILE = 0o100644
MODE_EXEC = 0o100755

NEWC_MAGIC = b"070701"
TRAILER = "TRAILER!!!"


def pad4(n: int) -> int:
    return (4 - (n % 4)) % 4


def classify(path: str) -> str:
    ext = os.path.splitext(path)[1].lower()
    return EXT_CATEGORY.get(ext, "programs")


def entry_mode(name: str, executable: bool) -> int:
    if executable or name.startswith("/programs/") or name.startswith("/drivers/"):
        return MODE_EXEC
    return MODE_FILE


class Entry:
    __slots__ = ("name", "data", "mode", "placeholder")

    def __init__(self, name: str, data: bytes, mode: int, placeholder: bool = False):
        self.name = name
        self.data = data
        self.mode = mode
        self.placeholder = placeholder


def write_newc(out_path: str, entries: list[Entry]) -> int:
    """写出 cpio newc 归档，返回归档字节数。"""
    os.makedirs(os.path.dirname(out_path) or ".", exist_ok=True)
    total = 0
    ino = 1
    with open(out_path, "wb") as fh:
        for e in entries:
            name_bytes = e.name.encode("utf-8") + b"\x00"
            hdr = NEWC_MAGIC + b"".join(
                b"%08X" % v
                for v in (
                    ino,
                    e.mode,
                    0,  # uid
                    0,  # gid
                    1,  # nlink
                    0,  # mtime
                    len(e.data),
                    0,  # devmajor
                    0,  # devminor
                    0,  # rdevmajor
                    0,  # rdevminor
                    len(name_bytes),
                    0,  # check
                )
            )
            block = hdr + name_bytes + b"\x00" * pad4(len(hdr) + len(name_bytes))
            fh.write(block)
            total += len(block)
            fh.write(e.data)
            pad = pad4(len(e.data))
            if pad:
                fh.write(b"\x00" * pad)
            total += len(e.data) + pad
            ino += 1
        trailer = TRAILER.encode("utf-8") + b"\x00"
        hdr = NEWC_MAGIC + b"".join(
            b"%08X" % v for v in (0, MODE_FILE, 0, 0, 1, 0, 0, 0, 0, 0, 0, len(trailer), 0)
        )
        block = hdr + trailer + b"\x00" * pad4(len(hdr) + len(trailer))
        fh.write(block)
        total += len(block)
    return total


def parse_newc(path: str):
    """解析 newc 归档，返回 (entries, trailer_ok)。用于 --check / 自校验。"""
    entries = []
    trailer_ok = False
    with open(path, "rb") as fh:
        blob = fh.read()
    pos = 0
    while pos + 110 <= len(blob):
        hdr = blob[pos : pos + 110]
        if hdr[:6] != NEWC_MAGIC:
            break
        fields = [int(hdr[6 + i * 8 : 6 + (i + 1) * 8], 16) for i in range(13)]
        filesize, namesize = fields[6], fields[11]
        pos += 110
        name = blob[pos : pos + namesize - 1].decode("utf-8", "replace")
        pos += namesize + pad4(110 + namesize)
        data = blob[pos : pos + filesize]
        pos += filesize + pad4(filesize)
        if name == TRAILER:
            trailer_ok = True
            break
        entries.append((name, data))
    return entries, trailer_ok


def collect_from_roots(roots: list[str], verbose: bool) -> list[Entry]:
    entries: list[Entry] = []
    seen: set[str] = set()
    for root in roots:
        root = os.path.abspath(root)
        if not os.path.isdir(root):
            if verbose:
                print(f"  [skip] 目录不存在: {root}")
            continue
        for dirpath, _dirnames, filenames in sorted(os.walk(root)):
            for fn in sorted(filenames):
                full = os.path.join(dirpath, fn)
                rel = os.path.relpath(full, root).replace(os.sep, "/")
                cat = classify(full)
                archive_name = "/" + cat + "/" + rel
                if archive_name in seen:
                    continue
                with open(full, "rb") as fh:
                    data = fh.read()
                seen.add(archive_name)
                entries.append(Entry(archive_name, data, entry_mode(archive_name, True)))
    return entries


def collect_default(verbose: bool) -> list[Entry]:
    entries: list[Entry] = []
    for archive_name, src_rel in DEFAULT_MANIFEST:
        src = os.path.join(REPO_ROOT, src_rel)
        if os.path.isfile(src):
            with open(src, "rb") as fh:
                data = fh.read()
            placeholder = False
        else:
            # 占位载荷：内容自带可读标记，便于串口/归档检查识别。
            data = ("VARIABLE-200 placeholder for %s\n" % archive_name).encode("utf-8")
            placeholder = True
            if verbose:
                print(f"  [stub] 源缺失，写入占位载荷: {archive_name}")
        entries.append(Entry(archive_name, data, entry_mode(archive_name, True), placeholder))
    return entries


def validate(entries: list[Entry]) -> list[str]:
    """目录表完整性：'/' 前缀 + 无重复 + 名称合法。"""
    problems: list[str] = []
    if not entries:
        problems.append("目录表为空")
        return problems
    seen: set[str] = set()
    for e in entries:
        if not e.name.startswith("/"):
            problems.append(f"路径未以 '/' 开头: {e.name}")
        if e.name in seen:
            problems.append(f"重复路径: {e.name}")
        seen.add(e.name)
        if not e.name.split("/")[1:2] or e.name.split("/")[1] not in CATEGORIES:
            problems.append(f"未知类别前缀: {e.name}")
    return problems


def main(argv=None) -> int:
    ap = argparse.ArgumentParser(description="VARIABLE-200 initrd 打包流水线")
    ap.add_argument("--out", default=DEFAULT_OUT, help="归档输出路径（默认 build/initrd.img）")
    ap.add_argument("--root", action="append", default=[], help="扫描目录（可重复；给定时走扫描模式）")
    ap.add_argument("--check", metavar="ARCHIVE", nargs="?", const=DEFAULT_OUT,
                    help="只校验归档的目录表，不重新打包")
    ap.add_argument("--no-isoroot", action="store_true", help="不把归档放进 build/isoroot/")
    ap.add_argument("--verbose", action="store_true")
    args = ap.parse_args(argv)

    if args.check:
        archive = args.check
        if not os.path.isfile(archive):
            print(f"ERROR: 归档不存在: {archive}", file=sys.stderr)
            return 1
        entries, trailer_ok = parse_newc(archive)
        problems = validate([Entry(n, d, MODE_FILE) for n, d in entries])
        if not trailer_ok:
            problems.append("缺少 TRAILER!!! 结束项")
        if problems:
            print("CHECK FAIL:")
            for p in problems:
                print("  -", p)
            return 1
        total = sum(len(d) for _n, d in entries)
        print(f"CHECK OK: {len(entries)} 项 / 载荷 {total} 字节 / {archive}")
        for n, d in entries:
            print(f"  {len(d):>8}  {n}")
        return 0

    if args.root:
        entries = collect_from_roots(args.root, args.verbose)
        if not entries:
            print("WARN: 扫描模式下没有收集到任何文件，回退到内置默认清单", file=sys.stderr)
            entries = collect_default(args.verbose)
    else:
        entries = collect_default(args.verbose)

    problems = validate(entries)
    if problems:
        print("ERROR: 目录表不合格：", file=sys.stderr)
        for p in problems:
            print("  -", p, file=sys.stderr)
        return 1

    written = write_newc(args.out, entries)
    payload = sum(len(e.data) for e in entries)
    stubs = sum(1 for e in entries if e.placeholder)
    print(f"OK: {args.out}")
    print(f"    目录表 {len(entries)} 项 / 载荷 {payload} 字节 / 归档 {written} 字节")
    if stubs:
        print(f"    含 {stubs} 项占位载荷（源程序尚未构建）")
    if args.verbose:
        for e in entries:
            tag = " (stub)" if e.placeholder else ""
            print(f"    {len(e.data):>8}  {e.name}{tag}")

    if not args.no_isoroot and os.path.isdir(ISOROOT):
        dst = os.path.join(ISOROOT, "initrd.img")
        with open(args.out, "rb") as src, open(dst, "wb") as fh:
            fh.write(src.read())
        print(f"    已放入 ISO 根: {dst}")

    # 打包后立刻自校验一遍（归档可读、目录表完整）。
    entries2, trailer_ok = parse_newc(args.out)
    if not trailer_ok or len(entries2) != len(entries):
        print("ERROR: 打包后自校验失败", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
