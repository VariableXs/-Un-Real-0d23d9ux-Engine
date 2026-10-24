# -*- coding: utf-8 -*-
"""fix_b23_imports.py — B23 批 11 单内容驱动补导（use alloc::vec / alloc::vec::Vec）。

规则：按文件实际用到的符号补导，已导入则跳过；插入点固定在
`use crate::checks::CheckSet;` 之前，保持与既有模块一致风格。
"""
import io, os, re, sys

FILES = [
    "unx0450_sratnuma.rs", "unx0451_tblsnap.rs", "unx0452_degrade.rs",
    "unx0453_pinseal.rs", "unx0454_badinj.rs", "unx0455_mapreg.rs",
    "unx0456_handoff.rs", "unx0457_5num.rs", "unx0458_acpize.rs",
    "unx0459_ktap.rs", "unx0460_b23.rs",
]
BASE = os.path.join("varix", "src", "unxstar")


def needs_vec_macro(text: str) -> bool:
    # 用到 vec! 宏（排除注释行）。
    for ln in text.splitlines():
        s = ln.strip()
        if s.startswith("//"):
            continue
        if re.search(r"\bvec!\s*[\[\(]", ln):
            return True
    return False


def needs_vec_type(text: str) -> bool:
    for ln in text.splitlines():
        s = ln.strip()
        if s.startswith("//"):
            continue
        if re.search(r"\bVec\s*[<:]", ln):
            return True
    return False


def main() -> int:
    changed = 0
    for name in FILES:
        p = os.path.join(BASE, name)
        if not os.path.isfile(p):
            print(f"MISS {name}")
            continue
        with io.open(p, "r", encoding="utf-8", newline="") as f:
            text = f.read()
        orig = text
        has_macro = "use alloc::vec;" in text
        has_type = "use alloc::vec::Vec;" in text
        want_macro = needs_vec_macro(text) and not has_macro
        want_type = needs_vec_type(text) and not has_type
        if not (want_macro or want_type):
            print(f"OK   {name}")
            continue
        anchor = "use crate::checks::CheckSet;"
        if anchor not in text:
            print(f"NOANCHOR {name}")
            continue
        ins = ""
        if want_macro:
            ins += "use alloc::vec;\n"
        if want_type:
            ins += "use alloc::vec::Vec;\n"
        text = text.replace(anchor, ins + anchor, 1)
        if text != orig:
            with io.open(p, "w", encoding="utf-8", newline="") as f:
                f.write(text)
            print(f"FIX  {name}  +{'vec' if want_macro else ''}{'+' if want_macro and want_type else ''}{'Vec' if want_type else ''}")
            changed += 1
    print(f"changed={changed}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
