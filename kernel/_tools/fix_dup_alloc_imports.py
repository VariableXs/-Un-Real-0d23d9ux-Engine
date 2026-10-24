#!/usr/bin/env python3
"""消除文件级重复的 no_std 导入（E0252）。

背景：多次批量注入器叠加，同一文件出现
    use alloc::vec::Vec;
    use alloc::string::String;
    ...
    use crate::alloc::{format, vec, vec::Vec};      <- 与上面重名
    use crate::alloc::string::String;               <- 与上面重名
crate::alloc 是 alloc 的再导出，两套写法解析出同一个名字，故报 “defined multiple times”。

策略（保守）：
1. 只处理文件头（第一个非 use/非注释/非空行之前）的 use 区，绝不碰函数体。
2. 统计每个被导入的「末段名」出现次数；重名者只保留一次。
3. 保留优先级：`use alloc::` > `use crate::alloc::`（alloc 直连更少依赖）；
   同优先级保留先出现的。
4. 分组导入 `use crate::alloc::{format, string::{String, ToString}, vec, vec::Vec};`
   按名拆分：需要删的名直接剔除；分组仍非空则重写，全空则整行删除。

用法: fix_dup_alloc_imports.py [--dry] [文件...]
"""
import os
import re
import sys

ROOT = os.path.abspath(os.path.join(os.path.dirname(__file__), "..", "varix", "src"))
USE_LINE = re.compile(r"^use\s+([A-Za-z0-9_:]+(?:\{[^}]*\})?[^;]*);\s*$")


def leaf_names(spec: str):
    """把一个 use 规格拆成 (name, 是否可安全删除)。不可解析的返回 None。"""
    s = spec.strip()
    if s.startswith("{"):
        return None
    m = re.match(r"^(.*::)?\{([^{}]*)\}$", s)
    if m:
        head = m.group(1) or ""
        items = [x.strip() for x in m.group(2).split(",") if x.strip()]
        out = []
        for it in items:
            if "{" in it or " as " in it:
                return None
            out.append((head + it).rsplit("::", 1)[-1])
        return out
    if " as " in s or "*" in s:
        return None
    names = []
    for part in s.split(","):
        part = part.strip()
        if not part:
            continue
        names.append(part.rsplit("::", 1)[-1])
    return names


def process(path: str, dry: bool) -> int:
    raw = open(path, "rb").read()
    crlf = b"\r\n" in raw
    text = raw.decode("utf-8", errors="replace").replace("\r\n", "\n")
    lines = text.split("\n")

    # 定位头部 use 区边界：跳过 //!、//、空行；遇到非 use 非注释行即停。
    region = []
    for i, ln in enumerate(lines):
        st = ln.strip()
        if not st or st.startswith("//"):
            continue
        if st.startswith("use ") or st.startswith("pub use "):
            region.append(i)
            continue
        break
    if not region:
        return 0

    seen = {}          # name -> (line_idx, spec, is_alloc_direct)
    drop_lines = set()
    rewrite = {}       # line_idx -> new spec
    changed = 0

    for i in region:
        ln = lines[i]
        m = re.match(r"^(pub\s+)?use\s+(.+);\s*$", ln.strip())
        if not m:
            continue
        spec = m.group(2).strip()
        names = leaf_names(spec)
        if names is None:
            continue
        direct = spec.startswith("alloc::")
        keep = []
        for n in names:
            prev = seen.get(n)
            if prev is None:
                seen[n] = (i, spec, direct)
                keep.append(n)
                continue
            # 重名：决定谁留下
            prev_i, prev_spec, prev_direct = prev
            if direct and not prev_direct:
                # 本行更优：替换
                rewrite.setdefault(prev_i, "DROP")
                seen[n] = (i, spec, direct)
                keep.append(n)
            # 否则丢弃本行中的该名
        if len(keep) != len(names):
            changed += 1
            if not keep:
                drop_lines.add(i)
            else:
                m2 = re.match(r"^(.*::)?\{", spec)
                head = m2.group(1) or "" if m2 else ""
                if head:
                    # 分组形态重写：保留的 name 重新拼路径不可行 → 退化为逐条 use
                    rewrite[i] = "\n".join("use %s%s;" % (head, k) for k in keep)
                else:
                    rewrite[i] = "use %s;" % ", ".join(keep)

    if not changed:
        return 0

    out = []
    for i, ln in enumerate(lines):
        if i in drop_lines or rewrite.get(i) == "DROP":
            continue
        if i in rewrite:
            out.append(rewrite[i])
        else:
            out.append(ln)
    text2 = "\n".join(out)
    # 压掉删除行留下的连续空行（最多留一行）
    text2 = re.sub(r"\n{3,}", "\n\n", text2)
    if not dry:
        data = text2.replace("\n", "\r\n") if crlf else text2
        open(path, "wb").write(data.encode("utf-8"))
    return changed


def main() -> int:
    dry = "--dry" in sys.argv
    files = [a for a in sys.argv[1:] if not a.startswith("--")]
    if not files:
        files = []
        for dp, _d, fs in os.walk(ROOT):
            for f in fs:
                if f.endswith(".rs"):
                    files.append(os.path.join(dp, f))
    total = 0
    touched = 0
    for f in files:
        n = process(f, dry)
        if n:
            touched += 1
            total += n
            print("%s: %d" % (os.path.relpath(f, ROOT), n))
    print("files=%d names_dropped=%d dry=%s" % (touched, total, dry))
    return 0


if __name__ == "__main__":
    sys.exit(main())
