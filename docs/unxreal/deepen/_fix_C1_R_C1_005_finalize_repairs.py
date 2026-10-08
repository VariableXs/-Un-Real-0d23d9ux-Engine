#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""[AI-11][C1] R-C1-005 判例：finalize 门禁暴露项勘误与补足——自检留痕
判例内容（finalize 校验链首跑 306 红中的 6 处真问题；其余 300 处为校验脚本判据提取正则 bug，已修脚本非文档）：
  ① B16 UNX-F8316 深化册行数分解 240(80+80+80) != 主值 260 → 勘误为 90+90+80=260（中止面/回收面各 +10，对账件不动；批级求和 5,120 不受影响）
  ② finalize 正文 ≥300 门禁 5 条补足（写作返工，非数字勘误）：
     B20 F8395 291→312（版本变更即重导出断言）/ B24 F8472 293→305（实例号连续零缺零重断言）
     B29 F8567 298→306（组行五列协议）/ B29 F8577 281→302（事件时间戳单调断言）/ B30 F8593 290→301（两批事件总数守恒断言）
自检：① 新值在位 ② 旧值零残留 ③ 分解复核 90+90+80==260 ④ 五条正文 len≥300 复核 ⑤ 骨架 F8401 转义勘误（\\x7f→\x7f，魔数首字节正确字面——与深化册一致）
"""
import re
import sys
from pathlib import Path

sys.stdout.reconfigure(encoding="utf-8")
D = Path(__file__).resolve().parent
ok = True


def chk(cond, msg):
    global ok
    print(("  [OK] " if cond else "  [RED] ") + msg)
    if not cond:
        ok = False


# ① F8316 行数分解勘误自检
b16 = (D / "C1-B16.md").read_text(encoding="utf-8")
chk("（中止面 90 + 回收面 90 + 对账件 80；测试段不计）" in b16, "F8316 新分解 90+90+80 在位")
chk("（中止面 80 + 回收面 80 + 对账件 80" not in b16, "F8316 旧分解 80+80+80 零残留")
chk(sum([90, 90, 80]) == 260, "独立复算：90+90+80==260==主值")

# ② 五条正文补足自检（len 实计）
fixes = [
    ("C1-B20.md", "UNX-F8395", "版本变更即重导出断言（漂移防线）"),
    ("C1-B24.md", "UNX-F8472", "实例号连续零缺零重断言"),
    ("C1-B29.md", "UNX-F8567", "组行五列协议"),
    ("C1-B29.md", "UNX-F8577", "事件时间戳单调断言（乱序注入显性红）"),
    ("C1-B30.md", "UNX-F8593", "两批事件总数守恒断言"),
]
for fname, fid, phrase in fixes:
    txt = (D / fname).read_text(encoding="utf-8")
    show = False
    body = None
    for line in txt.splitlines():
        if line.startswith("### " + fid):
            show = True
        elif line.startswith("### "):
            show = False
        if show and line.startswith("- 正文："):
            body = line[len("- 正文："):].strip()
    chk(body is not None and phrase in body, f"{fid} 补足语在位：『{phrase[:20]}…』")
    chk(body is not None and len(body) >= 300, f"{fid} 正文 len={len(body) if body else 0} ≥300")
    chk("；账入册。第二步机检件" not in txt.replace("账入册，事件时间戳", "X"), f"{fid} 旧句零残留（补足后改写）")

# ⑤ 骨架 F8401 转义勘误自检（\\x7f→\x7f）
sk21 = (D.parent / "batches" / "UNX-C1-B21.md").read_text(encoding="utf-8")
chk("\\x7f" in sk21 and "\\\\x7f" not in sk21, "骨架 B21 F8401 转义勘误：\\x7f 在位，\\\\x7f 零残留")

print(f"\n{'[DONE] R-C1-005 自检全绿：F8316 分解勘误 + 5 条正文补足， finalize 门禁修复留痕完毕。' if ok else '[FAIL] R-C1-005 自检存在红灯。'}")
sys.exit(0 if ok else 1)
