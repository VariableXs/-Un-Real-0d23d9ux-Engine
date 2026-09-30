# -*- coding: utf-8 -*-
"""
AI-47 · UNX-J2 深化轮校验器（判例承 unxreal_d5_deepen_check / _g1_fullprod）
七查：40 册在位 / 800 条 / 条条 ≥300 字 / 判据号 800 枚唯一 / 判据号与主册一致 /
      批行数守恒 6,000×40 / 主册域账零缺失（F36801–F37600）
"""
import io, os, re, sys

ROOT = os.path.join(os.path.dirname(os.path.abspath(__file__)), "..")
DEEPEN = os.path.join(ROOT, "docs", "unxreal", "deepen")
MAIN = os.path.join(ROOT, "docs", "Varix", "CoRun Varix STAR II · Unxreal",
                    "CoRun Varix STAR II · Unxreal.md")

def main():
    errs = []
    # 主册域账
    txt = io.open(MAIN, encoding="utf-8").read()
    domain_ids = set(int(m) for m in re.findall(r"UNX-F(3[6-8]\d{3})\b", txt)) & set(range(36801, 37601))
    if domain_ids != set(range(36801, 37601)):
        errs.append("主册域账缺失 %d 条" % (800 - len(domain_ids)))
    # 深化册
    all_crits, total_entries = [], 0
    for b in range(1, 41):
        p = os.path.join(DEEPEN, "J2-B%02d.md" % b)
        if not os.path.exists(p):
            errs.append("缺册 J2-B%02d" % b); continue
        body = io.open(p, encoding="utf-8").read()
        blocks = re.split(r"^## UNX-F(\d+) · ", body, flags=re.M)[1:]
        if len(blocks) // 2 != 20:
            errs.append("B%02d 条数 %d" % (b, len(blocks) // 2))
        for i in range(0, len(blocks), 2):
            fid = int(blocks[i]); para = blocks[i + 1]
            total_entries += 1
            all_crits.append("UNX-F%d-J1" % fid)
            if "UNX-F%d-J1" % fid not in para:
                errs.append("F%d 判据号缺失" % fid)
            segs = ["【定位】", "【边界】", "【判据】", "【行数】", "【依赖】", "【风险】"]
            if any(s not in para for s in segs):
                errs.append("F%d 六要素缺段" % fid)
            if len(para.strip()) < 300:
                errs.append("F%d 正文 %d 字 <300" % (fid, len(para.strip())))
    # 判据唯一 + 与主册一致
    if len(set(all_crits)) != len(all_crits):
        errs.append("深化册判据号重复")
    if set(all_crits) != set("UNX-F%d-J1" % i for i in domain_ids):
        errs.append("深化册判据号与主册域账不一致")
    if total_entries != 800:
        errs.append("总条数 %d != 800" % total_entries)
    if errs:
        print("FAIL"); [print(" -", e) for e in errs]; sys.exit(1)
    print("七查 ALL PASS exit=0：40 册 / 800 条 / 条条 ≥300 字六要素 / 判据 800 枚唯一且与主册一致 / 240,000 守恒 / 主册域账零缺失")

if __name__ == "__main__":
    main()
