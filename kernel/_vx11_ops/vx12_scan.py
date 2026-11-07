# -*- coding: utf-8 -*-
"""VX12 批次扫描器：对给定单号清单做三闸扫描（污染/行数/命名）。
用法：python vx12_scan.py <num> [<num> ...]   （如 81001 81002 ...）
输出每单一行 PASS/FAIL + 明细。"""
import glob
import io
import os
import re
import sys

MODDIR = r"D:\2\14\-Un-Real-0d23d9ux-Engine-main\kernel\varix\src\unxstar"

FLOOR = {81001: 289}


def scan(num):
    hits = glob.glob(os.path.join(MODDIR, "unx%d_*.rs" % num))
    if not hits:
        return ("FAIL", "missing file")
    p = hits[0]
    text = io.open(p, "r", encoding="utf-8").read()
    lines = text.count("\n")
    problems = []
    for bad in ["if False else", "if True else", "todo!", "unimplemented!"]:
        if bad in text:
            problems.append(bad)
    if re.search(r"(?<!unwrap_or)\bunwrap\(\)|\.expect\(|panic!|std::", text):
        # 注释行不算：逐行查，跳过 //! 与 //
        for ln in text.splitlines():
            s = ln.strip()
            if s.startswith("//"):
                continue
            if re.search(r"\bunwrap\(\)|\.expect\(|panic!|std::", s):
                problems.append("panic-face:" + s.strip()[:40])
                break
    if not re.search(r"pub fn run_unx%d_checks" % num, text):
        problems.append("no-run-fn")
    if not re.search(r"pub const SEG_BASE: u32 = 0x9", text):
        problems.append("seg-base-suspect")
    if lines < FLOOR.get(num, 289):
        problems.append("lines=%d" % lines)
    if not re.search(r"mod tests", text):
        problems.append("no-tests")
    checks = len(re.findall(r's\.add(?:_dyn)?\("F%d' % (num + 1), text))
    if checks < 8:
        problems.append("checks=%d" % checks)
    if problems:
        return ("FAIL", ";".join(problems))
    return ("PASS", "lines=%d checks=%d" % (lines, checks))


def main():
    bad = 0
    for a in sys.argv[1:]:
        n = int(a)
        st, det = scan(n)
        print("UNX-F%d %s %s" % (n, st, det))
        if st == "FAIL":
            bad += 1
    print("TOTAL=%d FAIL=%d" % (len(sys.argv) - 1, bad))


if __name__ == "__main__":
    main()
