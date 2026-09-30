# -*- coding: utf-8 -*-
"""AI-52 · K2 深化轮校验器（八查）：deepen/K2-B01..B40.md 全断言，exit 0 = ALL PASS"""
import io, os, re, sys, importlib.util

HERE = os.path.dirname(os.path.abspath(__file__))
def _load(name, fn):
    spec = importlib.util.spec_from_file_location(name, os.path.join(HERE, fn))
    m = importlib.util.module_from_spec(spec); spec.loader.exec_module(m); return m
s1 = _load("_s1", "_k2_supp1.py"); s2 = _load("_s2", "_k2_supp2.py")
ALL = s1.BATCHES + s2.BATCHES; ROWS = s1.ROWS

OUT = os.path.normpath(os.path.join(HERE, "..", "deepen"))
fails = []
def chk(name, ok):
    print(("PASS " if ok else "FAIL ") + name)
    if not ok: fails.append(name)

books, entries, bad_len, ok_rows = {}, [], 0, True
for b in ALL:
    p = os.path.join(OUT, f"K2-B{b['no']:02d}.md")
    t = io.open(p, encoding="utf-8").read()
    books[b["no"]] = t
    ids = re.findall(r"^### UNX-F(\d+) · ", t, re.M)
    exp = [b["id_start"] + i for i in range(20)]
    if [int(x) for x in ids] != exp:
        print(f"  B{b['no']:02d} ID 顺序错"); ok_rows = False
    # 判据号与增补册逐一一致（J1 起头出现）
    for i, (name, ev) in enumerate(b["items"]):
        fid = b["id_start"] + i
        seg = t.split(f"### UNX-F{fid} · ")[1].split("\n### ")[0]
        entries.append((b["no"], fid, seg))
        if f"UNX-F{fid}-J1" not in seg: print(f"  F{fid} 判据号缺失"); ok_rows = False
        if len(seg.replace(" ", "")) < 420: bad_len += 1
        if "状态：[已深化]" not in seg: print(f"  F{fid} 状态缺失"); ok_rows = False
        if not all(k in seg for k in ["**定位**", "**语义边界**", "**依赖与嫁接源**", "**风险与回退**", "正文："]):
            print(f"  F{fid} 六要素缺"); ok_rows = False
        # 拆分账之和 = 条目行数
        rows = ROWS[i]
        m = re.search(r"核心逻辑 (\d+) \+ 账本与埋点 (\d+) \+ 判据与注入 (\d+) \+ 文档与防重 (\d+)", seg)
        if not m or sum(map(int, m.groups())) != rows: print(f"  F{fid} 拆分账≠{rows}"); ok_rows = False

chk("1 四十册 K2-B01..B40 全在位", len(books) == 40)
chk("2 800 条深化条目且 ID 与域账逐一一致", len(entries) == 800 and ok_rows)
chk("3 每条六要素+正文 ≥420 字符（零空转）", bad_len == 0)
chk("4 批行数 6,000 守恒（拆分账逐条核对）", ok_rows)
chk("5 状态 [已深化] ×800", all("状态：[已深化]" in s for _, _, s in entries))
chk("6 E 型批 J1R 反判据在深化册保留", all("-J1R" in s for bno, _, s in entries if 21 <= bno <= 28))
chk("7 判据主轴锚定全域在册（准时触发账/只读优先）",
    sum("准时触发账" in s for _, _, s in entries) >= 40 and sum("只读优先" in s for _, _, s in entries) >= 100)
chk("8 联签域锚定在 I 型批深化册", ("AI-53" in books[29] and "AI-54" in books[30]
    and "AI-50" in books[31] and "AI-71" in books[32] and "AI-73" in books[36]))

print()
if fails: print("ALL FAIL:", fails); sys.exit(1)
print("ALL PASS (8 checks) · exit=0")
