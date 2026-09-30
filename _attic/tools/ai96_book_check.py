# -*- coding: utf-8 -*-
"""AI-96 增补册八断言校验器（治理线覆盖率统计官域账守护）。
断言：1) 300 条连续零跳号零重号；2) 判据 GOV96-###-J1 一一对应唯一；
3) 批守恒 15 批 × 20 条 × 6,000 行；4) 主册既有 ID 段零撞号（GOV96 段为主册新增）；
5) UNX-F 公理域段零触碰（本册不含 UNX-F 条目行）；6) 主题两两零真重复（长度归一化）；
7) 内核锚列非空率 100%；8) 口径宪法行在位（三口径并列/分母冻结/N/A 禁剔除关键词在册）。
输出 JSON 至 _attic/reports/ai96_book_check.json。全部断言 ≤2,000 行守约。
"""
import io, json, re, sys, hashlib, os
sys.stdout = io.TextIOWrapper(sys.stdout.buffer, encoding="utf-8")
BASE = r"D:\2\14\-Un-Real-0d23d9ux-Engine-main\docs\Varix\CoRun Varix STAR II · Unxreal"
BOOK = os.path.join(BASE, "AI-96 · 覆盖率统计官 · 300项新功能增补册（B01–B15 · GOV96-001–GOV96-300）.md")
MAIN = os.path.join(BASE, "CoRun Varix STAR II · Unxreal.md")
REPORT = r"D:\2\14\-Un-Real-0d23d9ux-Engine-main\_attic\reports\ai96_book_check.json"

txt = open(BOOK, encoding="utf-8").read()
rows = []
for line in txt.splitlines():
    m = re.match(r"^\|\s*(GOV96-\d{3})\s*\|\s*(.+?)\s*\|\s*(\d+)\s*\|\s*增补\s*\|\s*(.+?)\s*\|\s*$", line.strip())
    if m:
        rows.append({"id": m.group(1), "name": m.group(2).strip(),
                     "lines": int(m.group(3)), "evidence": m.group(4).strip()})
res = {}

# 1 连续零跳号零重号
ids = [r["id"] for r in rows]
expect = [f"GOV96-{i:03d}" for i in range(1, 301)]
res["A1_id_continuous_unique"] = ids == expect

# 2 判据一一对应
ok2 = len(rows) == 300
for r in rows:
    if f"{r['id']}-J1" not in r["evidence"]:
        ok2 = False
        break
res["A2_judge_one_to_one"] = ok2

# 3 批守恒
batches = {}
for r in rows:
    b = (int(r["id"][-3:]) - 1) // 20
    batches.setdefault(b, [0, 0])
    batches[b][0] += 1
    batches[b][1] += r["lines"]
res["A3_batch_conservation"] = (len(batches) == 15 and
    all(v == [20, 6000] for v in batches.values()) and
    sum(r["lines"] for r in rows) == 90000)

# 4 主册零撞号（并入后仅允许本域登记块内出现 GOV96；块外零撞号）
main_txt = open(MAIN, encoding="utf-8").read()
_mk = "增补卷 · AI-96 治理线覆盖率统计官"
_main_pre = main_txt.split(_mk)[0] if _mk in main_txt else main_txt
res["A4_main_no_conflict"] = not re.search(r"GOV96-\d{3}", _main_pre)

# 5 UNX-F 零触碰（本册不得登记 UNX-F 条目行）
res["A5_f_series_untouched"] = not re.search(r"^\|\s*UNX-F\d+\s*\|", txt, re.M)

# 6 主题两两零真重复（去空白归一后精确重复才算）
names = [re.sub(r"\s+", "", re.sub(r"（.*?）|\(.*?\)|GOV96-\d{3}", "", r["name"])) for r in rows]
res["A6_no_true_dup_theme"] = len(names) == len(set(names))

# 7 内核锚列非空（判据锚定含内核链锚词族或治理锚）
KW = ("ktest", "kcheck", "kbuild", "limine", "boot-select", "F12", "last_boot",
      "存储探针", "引导链", "内核", "ps2::note_key", "三线门禁", "门禁基线")
res["A7_kernel_anchor_100"] = all(any(k in r["evidence"] for k in KW) for r in rows)

# 8 口径宪法关键词在册
res["A8_constitution_present"] = all(k in txt for k in
    ("三口径", "64,000", "top1000", "10,000", "N/A", "口径版本", "GOV-96-J1"))

digest = hashlib.sha256(txt.encode("utf-8")).hexdigest()
report = {"book": BOOK, "rows": len(rows), "total_lines": sum(r["lines"] for r in rows),
          "sha256": digest, "asserts": res, "all_pass": all(res.values())}
os.makedirs(os.path.dirname(REPORT), exist_ok=True)
json.dump(report, open(REPORT, "w", encoding="utf-8"), ensure_ascii=False, indent=2)
print(json.dumps(report, ensure_ascii=False, indent=1))
sys.exit(0 if report["all_pass"] else 1)
