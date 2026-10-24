#!/usr/bin/env python3
"""check7 全量收敛修复（2026-10-22 晚 · 接管并行会话冷却未愈项）。

四类机械修复，全部以编译日志点名的 行号/文件 为准，不做全文猜测：
  P1 use-in-braces：注入器把 `use X;` 插进 use 组花括号内 → 挪到组尾 `};` 之后；
  P2 vec 重复导入：E0252 点名的重复 use 行 → 删除后一行；
  P3 cs.ok(3参) → cs.add(3参)：只改方法名 token，参数不动；
  P4 cs.check(2参) → cs.add(2参+空detail)：括号配平后在尾参后插 `, ""`；
  P5 浮点方法迁移：.floor( → .m_floor( 等 + 注入 FloatExt 导入。

CRLF 安全：读原字节，按 \r\n/\n 保留原行尾。
"""
import re
import sys
from pathlib import Path

ROOT = Path(r"D:/2/14/-Un-Real-0d23d9ux-Engine-main/kernel")
LOG = ROOT / "_attic" / "check7_full_1022_1945.log"

ERR_LINE = re.compile(r"^\s*-->\s+(varix[\\/][^:]+?):(\d+):(\d+)\s*$")

def read_lines(p: Path):
    raw = p.read_bytes().decode("utf-8", errors="replace")
    eol = "\r\n" if "\r\n" in raw else "\n"
    return raw.split(eol), eol

def write_lines(p: Path, lines, eol):
    p.write_bytes(eol.join(lines).encode("utf-8"))

def collect_log():
    """返回 {file: set(line_no)} 分错误类。"""
    out = {k: {} for k in ("usebrace", "dup", "ok3", "check2", "float")}
    kind = None
    cur = None
    for raw in LOG.read_bytes().decode("utf-8", errors="replace").splitlines():
        if raw.startswith("error"):
            if "found keyword `use`" in raw or "found `alloc`" in raw or "found `;`" in raw:
                kind = "usebrace"
            elif "E0252" in raw and "`vec` is defined multiple times" in raw:
                kind = "dup"
            elif "E0061" in raw:
                kind = "ok3"
            elif "no method named `check`" in raw:
                kind = "check2"
            elif "E0599" in raw and re.search(r"no method named `(m_)?(sqrt|round|floor|ceil|powi|powf|ln|exp|log2|sin|cos|trunc|fract|tan)`", raw):
                kind = "float"
            else:
                kind = None
            cur = None
            continue
        m = ERR_LINE.match(raw)
        if m and kind:
            f = m.group(1).replace("\\", "/")
            out[kind].setdefault(f, set()).add(int(m.group(2)))
    return out

def fix_usebrace(p: Path, lnos):
    lines, eol = read_lines(p)
    # 行号集合降序处理；每处：删除 `use X;` 行，挪到所属组 `};` 之后
    fixed = 0
    for ln in sorted(lnos, reverse=True):
        i = ln - 1
        if i >= len(lines):
            continue
        s = lines[i].strip()
        if not (s.startswith("use ") and s.endswith(";")):
            continue
        # 前一行应以 { 结尾（use 组开头）
        if i == 0 or not lines[i - 1].rstrip().endswith("{"):
            continue
        # 找组尾：从 i 起第一个 }; 行
        j = i
        while j < len(lines) and not lines[j].strip().startswith("};"):
            j += 1
        if j >= len(lines):
            continue
        stmt = lines[i].strip()
        del lines[i]
        j -= 1  # 删行后组尾索引回退
        # 再查组尾（可能因删行偏移）
        while j < len(lines) and not lines[j].strip().startswith("};"):
            j += 1
        if j >= len(lines):
            # 恢复
            return 0
        lines.insert(j + 1, stmt)
        fixed += 1
    if fixed:
        write_lines(p, lines, eol)
    return fixed

def fix_dup(p: Path, lnos):
    lines, eol = read_lines(p)
    fixed = 0
    for ln in sorted(lnos, reverse=True):
        i = ln - 1
        if i < len(lines):
            s = lines[i].strip()
            if s.startswith("use ") and s.endswith(";"):
                del lines[i]
                fixed += 1
    if fixed:
        write_lines(p, lines, eol)
    return fixed

def fix_ok3(p: Path, lnos):
    lines, eol = read_lines(p)
    fixed = 0
    for ln in lnos:
        i = ln - 1
        if i < len(lines) and ".ok(" in lines[i]:
            lines[i] = lines[i].replace(".ok(", ".add(", 1)
            fixed += 1
    if fixed:
        write_lines(p, lines, eol)
    return fixed

def fix_check2(p: Path, lnos):
    """cs.check(a, b) → cs.add(a, b, "")：括号配平，支持跨行。"""
    lines, eol = read_lines(p)
    fixed = 0
    for ln in sorted(lnos, reverse=True):
        i = ln - 1
        if i >= len(lines):
            continue
        idx = lines[i].find(".check(")
        if idx < 0:
            continue
        # 从 (.check( 的开括号起配平
        start = idx + len(".check")  # 指向 '('
        depth = 0
        j, k = i, start
        end = None
        while j < len(lines):
            for k in range(start if j == i else 0, len(lines[j])):
                c = lines[j][k]
                if c == "(":
                    depth += 1
                elif c == ")":
                    depth -= 1
                    if depth == 0:
                        end = (j, k)
                        break
            if end:
                break
            j += 1
        if not end:
            continue
        ej, ek = end
        if ej == i:
            lines[i] = lines[i][:ek] + ', ""' + lines[i][ek:]
        else:
            lines[ej] = lines[ej][:ek] + ', ""' + lines[ej][ek:]
        lines[i] = lines[i].replace(".check(", ".add(", 1)
        fixed += 1
    if fixed:
        write_lines(p, lines, eol)
    return fixed

FLOAT_METHODS = ["sqrt", "round", "floor", "ceil", "trunc", "fract",
                 "sin", "cos", "tan", "ln", "exp", "log2", "log10",
                 "powf", "powi", "atan", "atan2", "acos", "asin",
                 "sin_cos", "rem_euclid"]

def fix_float(p: Path, lnos):
    lines, eol = read_lines(p)
    fixed = 0
    for ln in lnos:
        i = ln - 1
        if i >= len(lines):
            continue
        line = lines[i]
        for m in FLOAT_METHODS:
            new = re.sub(r"\." + m + r"\(", ".m_" + m + "(", line)
            if new != line:
                lines[i] = new
                line = new
                fixed += 1
    # 注入 FloatExt（若缺）
    body = "\n".join(lines)
    if fixed and "use crate::float::FloatExt;" not in body:
        for i, l in enumerate(lines):
            if l.strip().startswith("use "):
                lines.insert(i, "use crate::float::FloatExt;")
                break
    if fixed:
        write_lines(p, lines, eol)
    return fixed

def main():
    kinds = collect_log()
    total = 0
    for kind, fixer in (("usebrace", fix_usebrace), ("dup", fix_dup),
                        ("ok3", fix_ok3), ("check2", fix_check2),
                        ("float", fix_float)):
        n_files = 0
        n_fixes = 0
        for f, lnos in kinds[kind].items():
            p = ROOT / f
            if not p.exists():
                print(f"[MISS] {kind} {f}")
                continue
            r = fixer(p, lnos)
            if r:
                n_files += 1
                n_fixes += r
        print(f"[{kind}] files={n_files} fixes={n_fixes}")
        total += n_fixes
    print(f"TOTAL={total}")

if __name__ == "__main__":
    main()
