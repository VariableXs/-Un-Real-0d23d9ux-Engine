#!/usr/bin/env python3
"""全仓补齐 `vec!` 宏导入（no_std 三件套里最常漏的一项）。

背景：`use alloc::vec::Vec;` 导入的是**类型**，`vec![]` 是**宏**，两者是
完全不同的条目。历史批量注入器反复只补类型不补宏，导致同一类错误在
上百个文件里重现。

策略（保守、可重复执行）：
- 只扫文件头部的 use 区（首个非 use 非注释行之前）；
- 若正文出现 `vec![` 而 use 区没有 `use alloc::vec;` / `use crate::alloc::vec;`
  / `use alloc::vec::*;` 之类的宏导入，则在该区末尾追加 `use alloc::vec;`；
- 已有 `use alloc::{vec, ...}` 分组导入的视为已覆盖；
- 幂等：重复运行不会叠加。

用法: ensure_vec_macro.py [--dry] [目录]
"""
import os
import re
import sys

ROOT = os.path.abspath(os.path.join(os.path.dirname(__file__), "..", "varix", "src"))

# 判定「已导入 vec 宏」的几种合法写法。
# 注意：分组导入（`use alloc::{vec, string::String}`）也要命中——漏判会
# 导致再补一条单行 `use alloc::vec;`，直接触发 E0252（实测 109 处）。
HAVE = [
    re.compile(r"^\s*use\s+alloc::vec\s*;", re.M),
    re.compile(r"^\s*use\s+crate::alloc::vec\s*;", re.M),
    re.compile(r"^\s*use\s+alloc::vec::\{[^}]*\bvec\b", re.M),
    re.compile(r"^\s*use\s+alloc::\s*\{[^}]*\bvec\b", re.M),
    re.compile(r"^\s*use\s+crate::alloc::\s*\{[^}]*\bvec\b", re.M),
    re.compile(r"^\s*use\s+alloc::\*;", re.M),
    re.compile(r"^\s*use\s+super::\*;", re.M),
]
USES_VEC = re.compile(r"\bvec!\s*\[")


def head_region(lines):
    """返回 (use 区最后一行下标, 首个代码行下标)；无 use 区时前者为 -1。

    注意：扫描范围必须覆盖**整个文件**而不是前 N 行——本仓部分模块的头注
    （`//!` 深化说明）长达上百行，use 区在 120 行之后。用「前 120 行」
    做过一次，结果对这类文件每次运行都会再插一次 `use alloc::vec;`，
    造成重复导入（实测 3 份）。这里的口径是「首个非注释非空行」，
    与文件总行数无关。
    """
    last_use = -1
    first_code = None
    last_inner_attr = -1
    for i, ln in enumerate(lines):
        st = ln.strip()
        if not st or st.startswith("//"):
            continue
        # 内层属性 `#![...]` 必须在**所有 use 之前**，否则整文件报
        # 「an inner attribute is not permitted in this context」。这条坑
        # 在批量插 use 时踩过一次（112 个文件同时炸），故在此显式记录。
        if st.startswith("#!["):
            last_inner_attr = i
            continue
        if st.startswith("use ") or st.startswith("pub use "):
            last_use = i
            continue
        first_code = i
        break
    # 插入点必须在最后一个内层属性之后。
    anchor = last_use if last_use > last_inner_attr else last_inner_attr
    return anchor, first_code


def process(path, dry):
    raw = open(path, "rb").read()
    crlf = b"\r\n" in raw
    text = raw.decode("utf-8", errors="replace").replace("\r\n", "\n")
    if not USES_VEC.search(text):
        return 0
    last_use, first_code = head_region(text.split("\n"))
    head = "\n".join(text.split("\n")[: max(last_use, first_code or 0) + 1])
    for pat in HAVE:
        if pat.search(head):
            return 0
    lines = text.split("\n")
    if last_use >= 0:
        at = last_use + 1
    elif first_code is not None:
        at = first_code
    else:
        return 0
    lines.insert(at, "use alloc::vec;")
    out = "\n".join(lines)
    if not dry:
        open(path, "wb").write((out.replace("\n", "\r\n") if crlf else out).encode("utf-8"))
    print(os.path.relpath(path, ROOT))
    return 1


def main():
    dry = "--dry" in sys.argv
    target = ROOT
    for a in sys.argv[1:]:
        if not a.startswith("--"):
            target = os.path.abspath(a)
    n = 0
    for dp, _d, fs in os.walk(target):
        for f in fs:
            if f.endswith(".rs"):
                n += process(os.path.join(dp, f), dry)
    print("patched=%d dry=%s" % (n, dry))
    return 0


if __name__ == "__main__":
    sys.exit(main())
