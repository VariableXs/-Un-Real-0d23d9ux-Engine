# -*- coding: utf-8 -*-
"""五桶精算 v3：深化批次后的行数对账（铁律二口径）。"""
import os, re, json

D = r"kernel/varix/src/svstar"
out = {}
total = dict(func=0, checks=0, tests=0, comments=0, blank=0)

for fn in sorted(os.listdir(D)):
    if not fn.endswith(".rs"):
        continue
    path = os.path.join(D, fn)
    lines = open(path, encoding="utf-8").read().split("\n")
    raw = len(lines)
    in_test = False
    in_check = False
    depth = 0
    bucket = dict(func=0, checks=0, tests=0, comments=0, blank=0)
    for i, l in enumerate(lines):
        st = l.strip()
        if not in_test and st.startswith("#[cfg(test)]"):
            in_test = True
            continue
        if in_test:
            bucket["tests"] += 1
            continue
        if st == "" :
            bucket["blank"] += 1
            continue
        if st.startswith("//") or st.startswith("//!"):
            bucket["comments"] += 1
            continue
        if st.startswith("pub fn run_") or re.match(r"pub fn run_\w+_checks", st):
            in_check = True
        if in_check:
            bucket["checks"] += 1
            # 自检函数结束判定：回到顶层（收尾的 "}"）
            if st == "}" and l == "}":
                in_check = False
            continue
        bucket["func"] += 1
    raw = len(lines)
    out[fn] = dict(bucket, raw=raw)
    for k in total:
        total[k] += bucket[k]

out["_TOTAL"] = total
out["_RAW_TOTAL"] = raw
print(json.dumps(out, indent=1, ensure_ascii=False))
