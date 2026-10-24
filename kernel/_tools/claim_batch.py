#!/usr/bin/env python3
"""按 ID 区间批量领取 VTaskBoard 任务卡（只改状态/领取人/时间三行）。

用法: claim_batch.py <start_id> <count> <owner> [--dry] [--force]

行级状态机实现（不依赖文本切分，避免任务卡正文里出现 "## [" 时误切）。
纪律：
- 只领取「状态=待领」的卡，已领/已完成一律跳过（绝不覆盖他人领取记录）。
- 前置未完成的卡不领（--force 用于前置由本批自身完成的场景）。
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
    start, count, owner = sys.argv[1], int(sys.argv[2]), sys.argv[3]
    dry = "--dry" in sys.argv
    force = "--force" in sys.argv

    m = re.match(r"^([A-Za-z0-9_]+)-F(\d+)$", start)
    if not m:
        print("bad id", start)
        return 2
    prefix, num = m.group(1), int(m.group(2))
    want = ["%s-F%04d" % (prefix, num + i) for i in range(count)]

    raw = open(BOARD, "rb").read()
    crlf = b"\r\n" in raw
    lines = raw.decode("utf-8", errors="replace").replace("\r\n", "\n").split("\n")

    # 第一遍：收集每张卡的起始行与状态/前置，供前置判定使用。
    idx_of, state_of, pre_of = {}, {}, {}
    cur = None
    for i, ln in enumerate(lines):
        cm = CARD.match(ln)
        if cm:
            cur = cm.group(1)
            idx_of.setdefault(cur, i)
            continue
        if cur is None:
            continue
        if ln.startswith("- 状态: "):
            state_of.setdefault(cur, ln.split(": ", 1)[1].strip() if ": " in ln else "")
        elif ln.startswith("- 前置: "):
            pre_of.setdefault(cur, ln.split(": ", 1)[1].strip() if ": " in ln else "")

    ts = time.strftime("%m-%d %H:%M")
    claimed, skipped = [], []
    for tid in want:
        if tid not in idx_of:
            skipped.append((tid, "卡不存在"))
            continue
        if state_of.get(tid) != "待领":
            skipped.append((tid, "状态=" + state_of.get(tid, "?")))
            continue
        pv = pre_of.get(tid, "")
        if pv and not force and state_of.get(pv) != "已完成":
            skipped.append((tid, "前置%s=%s" % (pv, state_of.get(pv, "MISSING"))))
            continue

        # 就地改三行（限定在本卡内，遇到下一张卡即停）。
        i = idx_of[tid]
        j = i + 1
        seen = set()
        while j < len(lines) and not CARD.match(lines[j]):
            ln = lines[j]
            if ln.startswith("- 状态: ") and "s" not in seen:
                lines[j] = "- 状态: 已领"
                seen.add("s")
            elif ln.startswith("- 领取人: ") and "o" not in seen:
                lines[j] = "- 领取人: " + owner
                seen.add("o")
            elif ln.startswith("- 时间: ") and "t" not in seen:
                lines[j] = "- 时间: " + ts
                seen.add("t")
            j += 1
        claimed.append(tid)

    out = "\n".join(lines)
    if crlf:
        out = out.replace("\n", "\r\n")
    if not dry:
        open(BOARD, "wb").write(out.encode("utf-8"))
    print("claimed %d/%d%s" % (len(claimed), count, " (dry)" if dry else ""))
    if claimed:
        print("  " + ",".join(claimed[:6]) + (" ..." if len(claimed) > 6 else ""))
    for tid, why in skipped[:10]:
        print("  skip", tid, why[:60])
    if len(skipped) > 10:
        print("  ... 另外 %d 张跳过" % (len(skipped) - 10))
    return 0


if __name__ == "__main__":
    sys.exit(main())
