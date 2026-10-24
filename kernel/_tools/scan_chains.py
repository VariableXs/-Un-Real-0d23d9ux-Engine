#!/usr/bin/env python3
"""扫描任务板：找「连续待领链」（链头前置已完成/为空，链内全待领，同册同域）。
用法: python _tools/scan_chains.py [min_len]
"""
import os, re, sys

ROOT = os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
BOARD = os.path.join(ROOT, "VarixTaskOps", "VTaskBoard", "taskboard.md")

def main():
    min_len = int(sys.argv[1]) if len(sys.argv) > 1 else 30
    raw = open(BOARD, "rb").read().decode("utf-8", "replace")
    cards = {}   # id -> dict
    order = []   # preserve file order
    cur = None
    for line in raw.splitlines():
        m = re.match(r"^## \[([A-Z]+-F\d+)\]\s*(.*)", line)
        if m:
            cur = {"id": m.group(1), "title": m.group(2).strip()}
            order.append(cur["id"])
            cards[cur["id"]] = cur
            continue
        if cur:
            m2 = re.match(r"^- (册|域|行数|前置|状态|领取人):\s*(.*)", line)
            if m2:
                cur[m2.group(1)] = m2.group(2).strip()

    def done(cid):
        c = cards.get(cid)
        return c is None or c.get("状态") == "已完成"

    # find maximal runs of 待领 within same 册+域
    runs = []
    i = 0
    n = len(order)
    while i < n:
        cid = order[i]
        c = cards[cid]
        if c.get("状态") == "待领" and c.get("领取人", "") == "":
            j = i
            while j < n:
                nid = order[j]
                nc = cards[nid]
                if nc.get("状态") != "待领" or nc.get("领取人") or nc.get("册") != c.get("册") or nc.get("域") != c.get("域"):
                    break
                j += 1
            length = j - i
            head_pre = cards[order[i]].get("前置", "")
            head_ok = done(head_pre)
            if length >= min_len:
                runs.append((length, order[i], order[j-1], head_ok, head_pre, c.get("册"), c.get("域"), c.get("行数")))
            i = j
        else:
            i += 1

    runs.sort(reverse=True)
    print(f"{'长度':>4} {'链头':<14} {'链尾':<14} {'头前置OK':<5} {'头前置':<14} {'册':<8} {'域':<10} 行数")
    for length, head, tail, ok, pre, bk, dom, rows in runs[:40]:
        print(f"{length:>4} {head:<14} {tail:<14} {str(ok):<5} {pre:<14} {bk:<8} {dom:<10} {rows}")

if __name__ == "__main__":
    main()
