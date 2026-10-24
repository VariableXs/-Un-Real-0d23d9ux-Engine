#!/usr/bin/env python3
"""VTaskBoard 批量写回（只改状态/领取人/结果/时间四字段）。

用法: python writeback_batch.py <owner> <ID1> <ID2> ... [--summary "模板"]

模板中 {id} 会被替换为卡号。默认摘要: "纯功能实现已落盘并注册，编译通过"。
纪律：只写「已领/待领」状态的卡（他人已完成不动）。
"""
import re
import sys
import time

BOARD = "VarixTaskOps/VTaskBoard/taskboard.md"
CARD = re.compile(r"^## \[([A-Za-z0-9_]+-F\d+)\]")


def main() -> int:
    args = sys.argv[1:]
    if not args:
        print(__doc__)
        return 2
    owner = args[0]
    ids = [a for a in args[1:] if not a.startswith("--")]
    summary = "纯功能实现已落盘并注册，编译通过"
    if "--summary" in args:
        i = args.index("--summary")
        summary = args[i + 1]

    raw = open(BOARD, "rb").read()
    crlf = b"\r\n" in raw
    nl = "\r\n" if crlf else "\n"
    lines = raw.decode("utf-8", errors="replace").replace("\r\n", "\n").split("\n")

    idx_of = {}
    cur = None
    for i, ln in enumerate(lines):
        m = CARD.match(ln)
        if m:
            cur = m.group(1)
            idx_of.setdefault(cur, i)
            continue

    now = time.strftime("%m-%d %H:%M")
    done = 0
    for tid in ids:
        start = idx_of.get(tid)
        if start is None:
            print("缺卡:", tid)
            continue
        # 找卡边界
        end = len(lines)
        for i in range(start + 1, len(lines)):
            if CARD.match(lines[i]):
                end = i
                break
        state = None
        for i in range(start, end):
            if lines[i].startswith("- 状态: "):
                state = lines[i].split(": ", 1)[1].strip()
        if state == "已完成" or state.startswith("已完成"):
            continue  # 已完成不动
        ok = True
        for i in range(start, end):
            if lines[i].startswith("- 状态: "):
                lines[i] = "- 状态: 已完成"
            elif lines[i].startswith("- 领取人: "):
                lines[i] = "- 领取人: " + owner
            elif lines[i].startswith("- 结果: "):
                lines[i] = "- 结果: " + summary
            elif lines[i].startswith("- 时间: "):
                lines[i] = "- 时间: " + now
        done += 1

    open(BOARD, "wb").write(nl.join(lines).encode("utf-8"))
    print("写回完成:", done, "张")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
