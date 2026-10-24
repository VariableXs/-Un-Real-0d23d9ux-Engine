#!/usr/bin/env python3
"""把 Star/VE 图形栈的浮点调用从 std 方法迁到内核浮点层（FloatExt 的 m_ 前缀法）。

为什么需要迁移：`x86_64-unknown-none` 下 core 不提供任何浮点 transcendentals，
`.sqrt()` / `.round()` / `.sin()` 等 18 种方法全部缺失。宿主 `cargo ktest` 有 std
所以绿，真机 `cargo kbuild` 红——这正是文档反复警告的「模拟器绿 ≠ 真机绿」。

迁移只做两件事，刻意保持手术式：
  1. 把已报错的行上的 `.method(` 改名为 `.m_method(`（只改编译器点名的那一行，
     不做全文替换——全文替换会误伤注释、字符串、判据名与非浮点同名方法）。
  2. 在受影响的文件顶部注入 `use crate::float::FloatExt;`（若尚未导入）。

不做的事：不改判据名、不改阈值、不重构算法、不动非浮点代码。
"""

import re
import subprocess
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
CRATE = ROOT / "varix"

# 编译器在该target 下报缺失的方法全集（f32 + f64 合并）。
FLOAT_METHODS = [
    "sqrt", "round", "floor", "ceil", "trunc", "fract",
    "sin", "cos", "tan", "ln", "exp", "log2", "log10",
    "powf", "powi", "atan", "atan2", "acos", "asin", "sin_cos", "rem_euclid",
]

ERR_RE = re.compile(r"^(varix[\\/][^\s:]+?):(\d+):(\d+):\s+error\[(E\d+)\]")


def collect_errors():
    """跑一次 kcheck，抓全部错误行。

    用 `kcheck` 而非 `kbuild`：本脚本只需要错误行定位，`check` 与 `build`
    的诊断集合相同但省去代码生成，单轮快一个数量级（实测 40min → 5min 级）。
    """
    proc = subprocess.run(
        ["cargo", "kcheck", "--offline", "--message-format=short"],
        cwd=ROOT, capture_output=True, text=True, encoding="utf-8", errors="replace",
    )
    out = []
    for line in proc.stderr.splitlines():
        if ": error" in line:
            out.append(line.strip())
    return out


def parse(errs):
    """把错误行解析成 {relpath: {(line, col, method_or_None)}}。

    只处理 E0599（缺失方法）与 E0689（浮点字面量歧义）；其余错误码不碰。
    """
    need = {}
    for line in errs:
        m = ERR_RE.match(line)
        if not m:
            continue
        path, lineno, _col, code = m.group(1), int(m.group(2)), int(m.group(3)), m.group(4)
        if code not in ("E0599", "E0689"):
            continue
        rel = path.replace("\\", "/")
        mm = re.search(r"no method named `([a-z_0-9]+)`", line)
        method = mm.group(1) if mm else None
        need.setdefault(rel, set()).add((lineno, method))
    return need


def rename_on_line(text_lines, lineno, methods):
    """在指定行上把 `.method(` 改为 `.m_method(`。返回是否改动。"""
    idx = lineno - 1
    if idx < 0 or idx >= len(text_lines):
        return False
    line = text_lines[idx]
    orig = line
    for mname in methods:
        # 只替换「方法调用」形态：前面紧跟非标识符字符（. 或 ::）。
        line = re.sub(r"(?<![A-Za-z0-9_])" + re.escape(mname) + r"\(",
                      "m_" + mname + "(", line)
    if line != orig:
        text_lines[idx] = line
        return True
    return False


def ensure_import(text, rel):
    """在文件顶部注入 `use crate::float::FloatExt;`（幂等）。

    位置纪律（Rust 语法硬要求）：**内层属性 `#![...]` 必须位于文件最前**，
    `use` 属 item，只能排在所有内层属性**之后**。
    原实现只跳过 `//!` 与空行，遇到 `//!` 头之后紧跟 `#![allow(..)]` 的文件
    会把 `use` 插到内层属性之前 ⇒
    `error: an inner attribute is not permitted in this context`
    （实测 vej08_probe.rs / vet02_highcontrast_engine.rs 两处）。
    故此处先跳过 `//!`/空行/普通注释，再继续跳过连续的内层属性行，
    最后才插入。
    """
    if "float::FloatExt" in text:
        return text, False
    lines = text.split("\n")
    i = 0
    # 第一段：模块文档注释与空行、普通行注释
    while i < len(lines) and (
        lines[i].startswith("//!")
        or lines[i].strip() == ""
        or lines[i].lstrip().startswith("//")
    ):
        i += 1
    # 第二段：内层属性（#![...]），必须保持在最前，use 只能排在其后
    while i < len(lines) and lines[i].lstrip().startswith("#!["):
        i += 1
    note = [
        "// 内核浮点层：`x86_64-unknown-none` 下 core 不提供浮点 transcendentals，",
        "// 本文件用到的浮点方法统一走 `FloatExt`（转发 libm，见 crate::float 文档）。",
        "use crate::float::FloatExt;",
    ]
    lines[i:i] = ([""] if i > 0 else []) + note
    return "\n".join(lines), True


def main():
    dry = "--apply" not in sys.argv
    errs = collect_errors()
    print(f"errors: {len(errs)}")
    need = parse(errs)
    print(f"files needing float migration: {len(need)}")

    changed_files = 0
    renamed_total = 0
    for rel, sites in sorted(need.items()):
        f = CRATE.parent / rel if rel.startswith("varix/") else None
        if f is None or not f.exists():
            print(f"  [skip] {rel} (not found)")
            continue
        text = f.read_text(encoding="utf-8")
        lines = text.split("\n")

        methods = sorted({m for (_, m) in sites if m})
        by_line = {}
        for (ln, m) in sites:
            by_line.setdefault(ln, set())
            if m:
                by_line[ln].add(m)

        touched = False
        for ln, ms in by_line.items():
            if ln == 0:
                continue  # E0689 类型歧义不在调用点，另行处理
            if rename_on_line(lines, ln, ms):
                touched = True

        new_text = "\n".join(lines)
        if touched:
            new_text, imp = ensure_import(new_text, rel)
            renamed_total += 1
            changed_files += 1
            action = "FIX" if not dry else "DRY"
            print(f"  [{action}] {rel}: {len(by_line)} lines, import={'added' if imp else 'present'}")

    print(f"\n{'would change' if dry else 'changed'} {changed_files} files ({renamed_total} call sites)")
    if dry:
        print("re-run with --apply to write")
        return 0

    # 写入
    for rel, sites in sorted(need.items()):
        f = CRATE.parent / rel if rel.startswith("varix/") else None
        if f is None or not f.exists():
            continue
        text = f.read_text(encoding="utf-8")
        lines = text.split("\n")
        by_line = {}
        for (ln, m) in sites:
            by_line.setdefault(ln, set())
            if m:
                by_line[ln].add(m)
        touched = False
        for ln, ms in by_line.items():
            if ln == 0:
                continue
            if rename_on_line(lines, ln, ms):
                touched = True
        if not touched:
            continue
        new_text = "\n".join(lines)
        new_text, _ = ensure_import(new_text, rel)
        f.write_text(new_text, encoding="utf-8")
    return 0


if __name__ == "__main__":
    sys.exit(main())