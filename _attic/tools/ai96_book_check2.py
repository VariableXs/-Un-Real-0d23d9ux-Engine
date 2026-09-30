# -*- coding: utf-8 -*-
"""AI-96 续卷（B16–B40）九断言校验器。
断言：1) 500 条（301–800）连续零跳号零重号；2) 判据一一对应唯一；
3) 批守恒 25 批 × 20 条 × 6,000 行；4) 主册登记块外零撞号；
5) UNX-F 零触碰；6) 主题两两零真重复（含跨册：与 B01–B15 册合并比对）；
7) 内核锚列非空率 100%；8) 口径宪法关键词在册；9) 与主册 B01–B15 登记卷 ID 零交叠。
输出 JSON 至 _attic/reports/ai96_book_check2.json。"""
import io, json, re, hashlib, os, sys
sys.stdout = io.TextIOWrapper(sys.stdout.buffer, encoding="utf-8")
BASE = r"D:\2\14\-Un-Real-0d23d9ux-Engine-main\docs\Varix\CoRun Varix STAR II · Unxreal"
BOOK = os.path.join(BASE, "AI-96 · 覆盖率统计官 · 500项新功能增补册续卷（B16–B40 · GOV96-301–GOV96-800）.md")
BOOK1 = os.path.join(BASE, "AI-96 · 覆盖率统计官 · 300项新功能增补册（B01–B15 · GOV96-001–GOV96-300）.md")
MAIN = os.path.join(BASE, "CoRun Varix STAR II · Unxreal.md")
REPORT = r"D:\2\14\-Un-Real-0d23d9ux-Engine-main\_attic\reports\ai96_book_check2.json"
rowre = re.compile(r"^\|\s*GOV96-(\d{3})\s*\|\s*(.+?)\s*\|\s*(\d+)\s*\|\s*增补\s*\|\s*(.+?)\s*\|\s*$")
def rows_of(path):
    out=[]
    for line in open(path, encoding="utf-8"):
        m=rowre.match(line.rstrip("\n"))
        if m: out.append({"id":int(m.group(1)),"name":m.group(2),"lines":int(m.group(3)),"ev":m.group(4)})
    return out
rows=rows_of(BOOK); res={}
res["A1_id_continuous_unique"] = [r["id"] for r in rows]==list(range(301,801))
res["A2_judge_one_to_one"] = all(f"GOV96-{r['id']:03d}-J1" in r["ev"] for r in rows) and len(rows)==500
from collections import defaultdict
b=defaultdict(int); c=defaultdict(int)
for r in rows:
    b[(r["id"]-1)//20]+=r["lines"]; c[(r["id"]-1)//20]+=1
res["A3_batch_conservation"] = len(b)==25 and all(v==6000 for v in b.values()) and all(v==20 for v in c.values())
main_txt=open(MAIN,encoding="utf-8").read()
_mk="增补卷 · AI-96"
_main_pre=main_txt.split(_mk)[0]
res["A4_main_block_aware_no_conflict"] = not re.search(r"GOV96-\d{3}", _main_pre)
res["A5_f_series_untouched"] = not re.search(r"^\|\s*UNX-F\d+\s*\|", open(BOOK,encoding="utf-8").read(), re.M)
def norm(n): return re.sub(r"\s+","",re.sub(r"（.*?）|\(.*?\)|GOV96-\d{3}","",n))
allnames=[norm(r["name"]) for r in rows]+[norm(r["name"]) for r in rows_of(BOOK1)]
res["A6_no_true_dup_theme_cross_book"] = len(allnames)==len(set(allnames))
KW=("ktest","kcheck","kbuild","limine","boot-select","F12","last_boot","存储探针","引导链","内核","ps2::note_key","三线门禁","门禁基线")
res["A7_kernel_anchor_100"] = all(any(k in r["ev"] for k in KW) for r in rows)
txt=open(BOOK,encoding="utf-8").read()
res["A8_constitution_present"] = all(k in txt for k in ("三口径","64,000","top1000","10,000","N/A","口径版本","GOV-96-J1"))
res["A9_main_book_overlap_zero"] = not (set(r["id"] for r in rows) & set(r["id"] for r in rows_of(BOOK1)))
digest=hashlib.sha256(txt.encode("utf-8")).hexdigest()
report={"book":BOOK,"rows":len(rows),"total_lines":sum(r["lines"] for r in rows),
        "sha256":digest,"asserts":res,"all_pass":all(res.values())}
os.makedirs(os.path.dirname(REPORT),exist_ok=True)
json.dump(report,open(REPORT,"w",encoding="utf-8"),ensure_ascii=False,indent=2)
print(json.dumps(report,ensure_ascii=False,indent=1))
sys.exit(0 if report["all_pass"] else 1)
