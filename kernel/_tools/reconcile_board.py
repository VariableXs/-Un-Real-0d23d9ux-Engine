#!/usr/bin/env python3
"""任务板 × 代码现实 对账。

产出：
1. 已实现（代码头注有 ID 锚点）但任务板仍待领/已领 —— 任务板落后；
2. 任务板已完成但代码无实现 —— 疑似虚报或实现被删；
3. 待领且代码无实现 —— 真实缺口。

用法:
  python reconcile_board.py [--gaps N] [--stale] [--book VE]
"""
import argparse
import os
import re
import sys

ROOT = os.path.abspath(os.path.join(os.path.dirname(__file__), "..", ".."))
SRC = os.path.join(ROOT, "kernel", "varix", "src")
BOARD = os.path.join(ROOT, "VarixTaskOps", "VTaskBoard", "taskboard.md")
CARD = re.compile(r"^## \[([A-Za-z0-9_]+-F\d+)\]")
HEAD_ID = re.compile(r"([A-Z][A-Z0-9_]{1,8})-F(\d{3,5})")


def scan_code():
    impl = {}
    for dirpath, _dirs, files in os.walk(SRC):
        for fn in files:
            if not fn.endswith(".rs"):
                continue
            path = os.path.join(dirpath, fn)
            try:
                with open(path, encoding="utf-8", errors="replace") as fh:
                    head = fh.read(800)
            except OSError:
                continue
            first = head.splitlines()[0] if head else ""
            if "F" not in first:
                continue
            m = HEAD_ID.search(first)
            if not m:
                continue
            tid = m.group(1) + "-F" + m.group(2)
            impl.setdefault(tid, []).append(
                os.path.relpath(path, ROOT).replace("\\", "/")
            )
    return impl


def scan_board():
    state, title = {}, {}
    cur = None
    with open(BOARD, encoding="utf-8", errors="replace") as fh:
        for ln in fh:
            m = CARD.match(ln)
            if m:
                cur = m.group(1)
                title[cur] = ln.split("]", 1)[1].strip()[:56]
                continue
            if cur and ln.startswith("- 状态: ") and cur not in state:
                state[cur] = ln.split(": ", 1)[1].strip()
    return state, title


def num(tid):
    m = re.search(r"F(\d+)", tid)
    return int(m.group(1)) if m else 0


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--gaps", type=int, default=0)
    ap.add_argument("--stale", action="store_true")
    ap.add_argument("--book", default="")
    args = ap.parse_args()

    impl = scan_code()
    state, title = scan_board()

    stale = [t for t, s in state.items() if s in ("待领", "已领") and t in impl]
    phantom = [t for t, s in state.items() if s.startswith("已完成") and t not in impl]
    missing = [t for t, s in state.items() if s == "待领" and t not in impl]

    print("代码侧有锚点实现的任务号: %d" % len(impl))
    print("任务板卡片 %d（待领 %d）"
          % (len(state), sum(1 for v in state.values() if v == "待领")))
    print("[1] 已实现但板子待领/已领: %d" % len(stale))
    print("[2] 板子已完成但代码无锚点: %d" % len(phantom))
    print("[3] 待领且无实现（真实缺口）: %d" % len(missing))

    if args.stale:
        for t in sorted(stale, key=num)[:60]:
            print("  STALE %s %s | %s -> %s" % (t, state[t], title.get(t, ""), impl[t][0]))
    if args.gaps:
        sel = [t for t in missing if not args.book or t.startswith(args.book)]
        for t in sorted(sel, key=num)[: args.gaps]:
            print("  GAP %s | %s" % (t, title.get(t, "")))
    if phantom:
        print("  疑似虚报样例:", ", ".join(sorted(phantom, key=num)[:10]))
    return 0


if __name__ == "__main__":
    sys.exit(main())
