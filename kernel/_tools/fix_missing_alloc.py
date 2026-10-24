#!/usr/bin/env python3
"""按编译器真实报错补齐 no_std 三件套导入（宏与类型分开处理）。

只处理「文件头部已有 use 区」的文件，在 use 区末尾追加缺失项，
绝不改动函数体；已存在的名字不重复添加。每个文件处理前重新读盘，
规避并行会话改写导致的快照过期。

用法: fix_missing_alloc.py <errlog> [--dry]
errlog 为 `cargo check --message-format short` 的输出文件。
"""
import os
import re
import sys

ROOT = os.path.abspath(os.path.join(os.path.dirname(__file__), "..", "varix", "src"))

MACROS = {
    "vec": "use alloc::vec;",
    "format": "use alloc::format;",
}
TYPES = {
    "Vec": "use alloc::vec::Vec;",
    "String": "use alloc::string::String;",
    "ToString": "use alloc::string::ToString;",
    "Box": "use alloc::boxed::Box;",
}

ERR = re.compile(
    r"^varix\\src\\(.+?\.rs):\d+:\d+: error(?:\[E\d+\])?: "
    r"(?:cannot find macro `(\w+)`"
    r"|cannot find (?:type|value|function) `(\w+)`"
    r"|use of undeclared (?:type|macro|value) `(\w+)`)"
)


def collect(errlog):
    need = {}
    with open(errlog, encoding="utf-8", errors="replace") as fh:
        for ln in fh:
            if "error" not in ln:
                continue
            m = ERR.match(ln.strip())
            if not m:
                continue
            path, mac, ty1, ty2 = m.group(1), m.group(2), m.group(3), m.group(4)
            name = mac or ty1 or ty2
            if not name:
                continue
            prev = need.setdefault(path.replace("\\", "/"), set())
            prev.add(name)
    return need


def have_names(lines):
    """收集文件 use 区已导入的末段名（含分组展开）。"""
    names = set()
    for ln in lines[:80]:
        st = ln.strip()
        if not (st.startswith("use ") or st.startswith("pub use ")):
            continue
        for tok in re.findall(r"[A-Za-z_][A-Za-z0-9_]*", st):
            names.add(tok)
    return names


def process(rel, names, dry):
    path = os.path.join(ROOT, rel)
    if not os.path.exists(path):
        return 0
    raw = open(path, "rb").read()
    crlf = b"\r\n" in raw
    text = raw.decode("utf-8", errors="replace").replace("\r\n", "\n")
    lines = text.split("\n")
    present = have_names(lines)
    add = []
    for n in sorted(names):
        if n in present:
            continue
        if n in MACROS:
            add.append(MACROS[n])
        elif n in TYPES:
            add.append(TYPES[n])
    if not add:
        return 0

    # 找头部 use 区最后一行；没有 use 区则插在首个 doc 注释块之后。
    last_use = -1
    first_code = None
    for i, ln in enumerate(lines):
        st = ln.strip()
        if not st or st.startswith("//"):
            continue
        if st.startswith("use ") or st.startswith("pub use "):
            last_use = i
            continue
        first_code = i
        break
    if last_use >= 0:
        at = last_use + 1
    elif first_code is not None:
        at = first_code
    else:
        return 0

    block = list(add)
    new = lines[:at] + block + lines[at:]
    out = "\n".join(new)
    if not dry:
        data = out.replace("\n", "\r\n") if crlf else out
        open(path, "wb").write(data.encode("utf-8"))
    print("%-46s +%d %s" % (rel, len(block), ",".join(x.rsplit("::", 1)[-1].rstrip(";") for x in block)))
    return len(block)


def main():
    if len(sys.argv) < 2:
        print(__doc__)
        return 2
    dry = "--dry" in sys.argv
    need = collect(sys.argv[1])
    total = 0
    files = 0
    for rel, names in sorted(need.items()):
        n = process(rel, names, dry)
        if n:
            files += 1
            total += n
    print("files=%d imports=%d dry=%s" % (files, total, dry))
    return 0


if __name__ == "__main__":
    sys.exit(main())
