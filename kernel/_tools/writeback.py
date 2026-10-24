#!/usr/bin/env python3
"""VTaskBoard 单卡写回（状态 / 领取人 / 结果 / 时间）。

用法:
  python writeback.py <ID> <状态> <结果摘要> [--owner AI-WB-1022]

只改目标卡内的四个字段，其它字节不动（保留原行尾风格）。
"""
import re
import sys
import time

BOARD = "VarixTaskOps/VTaskBoard/taskboard.md"
CARD = re.compile(r"^## \[([A-Za-z0-9_]+-F\d+)\]")


def main() -> int:
    if len(sys.argv) < 4:
        print(__doc__)
        return 2
    tid, status, summary = sys.argv[1], sys.argv[2], sys.argv[3]
    owner = ""
    if "--owner" in sys.argv:
        i = sys.argv.index("--owner")
        if i + 1 < len(sys.argv):
            owner = sys.argv[i + 1]

    raw = open(BOARD, "rb").read()
    crlf = b"\r\n" in raw
    lines = raw.decode("utf-8", errors="replace").replace("\r\n", "\n").split("\n")

    start = None
    for i, ln in enumerate(lines):
        m = CARD.match(ln)
        if m and m.group(1) == tid:
            start = i
            break
    if start is None:
        print("卡不存在:", tid)
        return 1

    end = len(lines)
    for j in range(start + 1, len(lines)):
        if CARD.match(lines[j]):
            end = j
            break

    seen = set()
    ts = time.strftime("%m-%d %H:%M")
    for j in range(start + 1, end):
        ln = lines[j]
        if ln.startswith("- 状态: ") and "s" not in seen:
            lines[j] = "- 状态: " + status
            seen.add("s")
        elif ln.startswith("- 领取人: ") and "o" not in seen and owner:
            lines[j] = "- 领取人: " + owner
            seen.add("o")
        elif ln.startswith("- 结果: ") and "r" not in seen:
            lines[j] = "- 结果: " + summary
            seen.add("r")
        elif ln.startswith("- 时间: ") and "t" not in seen:
            lines[j] = "- 时间: " + ts
            seen.add("t")

    out = "\n".join(lines)
    if crlf:
        out = out.replace("\n", "\r\n")
    open(BOARD, "wb").write(out.encode("utf-8"))
    print("写回", tid, "->", status, "缺字段:", 4 - len(seen))
    return 0


if __name__ == "__main__":
    sys.exit(main())
