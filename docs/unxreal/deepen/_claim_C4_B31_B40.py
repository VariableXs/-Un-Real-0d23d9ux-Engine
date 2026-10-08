# -*- coding: utf-8 -*-
"""C4 B31–B40 收官段写锁认领（AI-14 · 波08-M20）。
操作一：总纲 C4 表 B31–B40 十行 [未动]→[骨架]，预算与承接会话回填。
操作二：§7.3-C4 引言追加收官段写锁批注（批主题一次定档）。
异常零静默：所有锚点唯一性断言，失败即抛不改账。
"""
import io

ZONGGANG = "docs/Varix/CoRun Varix STAR II · Unxreal/CoRun Varix STAR II · Unxreal · 总纲与施工书.md"

BUDGET = [("B31", 11001, 5330), ("B32", 11021, 5330), ("B33", 11041, 5330),
          ("B34", 11061, 5330), ("B35", 11081, 5330), ("B36", 11101, 5330),
          ("B37", 11121, 5210), ("B38", 11141, 5210), ("B39", 11161, 5210),
          ("B40", 11181, 5210)]
assert sum(b for _, _, b in BUDGET) == 52820, "收官段预算求和 != 52,820"

ANCHOR_INTRO = "域账守恒：B01–B15 深化 85,680 + B16–B30 骨架 101,500 + B31–B40 收官 52,820 = 240,000 ✓（域累计至 B30 = 187,180/240,000）。"
CLAIM_NOTE = (
    "> 【波08-M20 收官段写锁批注（AI-14）】本会话收官段写锁认领 B31–B40（F11001–F11200，200 条），"
    "上表十行 [未动]→[骨架] 翻态、预算逐批回填（B31–B36 各 5,330 合计 31,980 + B37–B40 各 5,210 合计 20,840 = 52,820）。"
    "一次对话 300 项明令在本域收官段以 200 条全量达成 + 域闭账交付兑现（沿 AI-13 C3 域收官判例，不越界代写他域）。"
    "批主题一次定档：B31 挂号联签一（futex/epoll 族挂点对账）/B32 挂号联签二（SysV/POSIX/pty/socket 挂号对账）"
    "/B33 LTP ipc·pty 组对照联签（R-C3-003 释放闸对账面）/B34 五族全量回归矩阵/B35 压力长稳演习（多进程 IPC 压测/pty vim 全屏）"
    "/B36 联签总对账与 I 型总收口/B37 清账一段（B01–B20 判据重放）/B38 清账二段（B21–B39 判据重放）"
    "/B39 域验收总核销·域经快照·移交包三件/B40 域闭账（F11200 收官锚三断言 + 240,000 满账宣告）。"
    "与任务书勾稽：B37–B40 承担 C 型「LTP ipc/pty 组回归与压测核账」收官主题的核账面（LTP 对照与压测核账实体在 B33–B35 联测段显式立条），任务书主题零缺失。"
)

def main():
    t = io.open(ZONGGANG, encoding="utf-8").read()
    # 操作一：十行翻态
    for i, (bid, fs, rows) in enumerate(BUDGET):
        fe = fs + 19
        old = "| UNX-C4-%s | F%d–F%d | 20 | [未动] | 0 | 待领 |" % (bid, fs, fe)
        new = "| UNX-C4-%s | F%d–F%d | 20 | [骨架] | —（骨架 %s 行） | AI-14 |" % (bid, fs, fe, format(rows, ","))
        assert t.count(old) == 1, "%s 行锚不唯一（count=%d）" % (bid, t.count(old))
        t = t.replace(old, new)
    # 操作二：引言批注
    assert t.count(ANCHOR_INTRO) == 1, "引言守恒锚不唯一（count=%d）" % t.count(ANCHOR_INTRO)
    t = t.replace(ANCHOR_INTRO, ANCHOR_INTRO + "\n" + CLAIM_NOTE)
    io.open(ZONGGANG, "w", encoding="utf-8", newline="").write(t)
    # 复核
    t2 = io.open(ZONGGANG, encoding="utf-8").read()
    ok = sum(1 for i, (bid, fs, rows) in enumerate(BUDGET)
             if ("| UNX-C4-%s | F%d–F%d | 20 | [骨架] | —（骨架 %s 行） | AI-14 |" % (bid, fs, fs + 19, format(rows, ","))) in t2)
    print("写锁认领完成：十行 [骨架] 复核 %d/10；引言批注在位 %s；预算求和 52,820 ✓"
          % (ok, "✓" if CLAIM_NOTE[:40] in t2 else "✗"))

if __name__ == "__main__":
    main()
