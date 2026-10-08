#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""
R-C1-003 勘误：F8298 协议同构实例链重编号（B26–B30 骨架）
判例来源：R-C1-001/R-C1-002 同源纪律（机检发现 -> 判例适用 -> 脚本勘误 -> 留痕复核）。

事实：
  - 在先骨架：B16=第八、B17=第九、B22=第十、B23=第十一、B24=第十二（各批断言族判据原文）。
  - B25 骨架（F8496）未设实例号——不涉及本勘误。
  - 在后骨架 B26–B30 误沿独立计数器（第九/第十/第十一/第十二/第十三），
    与 B17/B22/B23/B24 已占号冲突（B26=九 重复 B17；B27=十 重复 B22；
    B28=十一 重复 B23；B29=十二 重复 B24；B30=十三 与勘误后链冲突）。

勘误（延续唯一真值链，B25 无号不占位）：
  B26: 第九  -> 第十三
  B27: 第十  -> 第十四
  B28: 第十一 -> 第十五
  B29: 第十二 -> 第十六
  B30: 第十三 -> 第十七

影响面：仅实例号字样（每文件恰一处）；判据语义/行数/ID 零变更。
深化册 B26–B30 按勘误后骨架誊写（判据逐条一致），册序言载 R-C1-003 注记。
执行后自检：grep 全 C1 骨架实例号链应为 8,9,10,11,12,13,14,15,16,17 零重复。
"""
import os
import re
import sys

BASE = os.path.dirname(os.path.abspath(__file__))
SKE_DIR = os.path.normpath(os.path.join(BASE, "..", "batches"))

MAP = {
    "UNX-C1-B26.md": [("F8298 协议同构（第九实例）", "F8298 协议同构（第十三实例）")],
    "UNX-C1-B27.md": [("F8298 协议同构（第十实例）", "F8298 协议同构（第十四实例）")],
    "UNX-C1-B28.md": [("F8298 协议同构（第十一实例）", "F8298 协议同构（第十五实例）")],
    "UNX-C1-B29.md": [("F8298 协议同构（第十二实例）", "F8298 协议同构（第十六实例）")],
    "UNX-C1-B30.md": [("F8298 协议同构（第十三实例）", "F8298 协议同构（第十七实例）")],
}

CN_NUM = {"一": 1, "二": 2, "三": 3, "四": 4, "五": 5, "六": 6, "七": 7,
          "八": 8, "九": 9, "十": 10}


def cn_to_int(s: str):
    """极简中文号解析（覆盖 一~十九 足够本链）。"""
    if s.startswith("十"):
        rest = s[1:]
        return 10 + (CN_NUM[rest] if rest else 0)
    if "十" in s:
        a, b = s.split("十", 1)
        return CN_NUM[a] * 10 + (CN_NUM[b] if b else 0)
    return CN_NUM.get(s)


def main():
    changed = []
    for fname, pairs in MAP.items():
        path = os.path.join(SKE_DIR, fname)
        with io_open(path) as f:
            text = f.read()
        for old, new in pairs:
            n = text.count(old)
            if n != 1:
                print("[FAIL] %s 期望恰 1 处 %r，实得 %d" % (fname, old, n))
                return 1
            text = text.replace(old, new)
            changed.append((fname, old, new))
        with io_open_write(path) as f:
            f.write(text)
        print("[OK] %s 已勘误" % fname)

    # 全链自检：收集 C1 全部骨架实例号，应为 8..17 连续零重复
    got = []
    for i in range(1, 31):
        p = os.path.join(SKE_DIR, "UNX-C1-B%02d.md" % i)
        if not os.path.exists(p):
            continue
        with io_open(p) as f:
            t = f.read()
        for m in re.finditer(r"F8298 协议同构（第([^）]+)实例）", t):
            v = cn_to_int(m.group(1))
            got.append((i, v))
    nums = sorted(v for _, v in got)
    expect = list(range(8, 18))
    dup = {v for v in nums if nums.count(v) > 1}
    print("[CHAIN] 实例号链：%s" % nums)
    if dup:
        print("[FAIL] 链内重复：%s" % sorted(dup))
        return 1
    if nums != expect:
        print("[FAIL] 链不连续，期望 %s" % expect)
        return 1
    print("[DONE] R-C1-003 勘误完成：%d 处替换，链 8..17 连续零重复。" % len(changed))
    for fname, old, new in changed:
        print("  - %s: %s -> %s" % (fname, old, new))
    return 0


def io_open(path):
    return open(path, "r", encoding="utf-8", newline="")


def io_open_write(path):
    return open(path, "w", encoding="utf-8", newline="")


if __name__ == "__main__":
    sys.exit(main())
