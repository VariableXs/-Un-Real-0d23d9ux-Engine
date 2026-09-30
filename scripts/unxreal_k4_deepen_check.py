# -*- coding: utf-8 -*-
"""UNX-K4 深化轮校验器 · 七查（判例承 scripts/unxreal_j2_deepen_check.py）
1 40 册在位 2 800 条 3 每条 ≥300 字（保守 420 字符） 4 判据号唯一
5 判据号与主册域账一致 6 批 6,000 行守恒·域 240,000 7 状态 [已深化] 全量
exit=0 ALL PASS"""
import io, os, re, sys, glob

HERE = os.path.dirname(os.path.abspath(__file__))
DEEP = os.path.abspath(os.path.join(HERE, "..", "docs", "unxreal", "deepen"))
MAIN = os.path.abspath(os.path.join(HERE, "..", "docs", "Varix", "CoRun Varix STAR II · Unxreal", "CoRun Varix STAR II · Unxreal.md"))

def main():
    errs = []
    books = sorted(glob.glob(os.path.join(DEEP, "K4-B[0-9][0-9].md")))
    # 查1
    if len(books) != 40: errs.append(f"查1 FAIL: 册数 {len(books)} != 40")
    entry_re = re.compile(r"^### UNX-F(\d+) · ", re.M)
    meta_re = re.compile(r"^- 域/批：K4/(B\d+)｜判据：(UNX-F\d+-J1[^｜]*)｜纯功能行数：(\d+) 行", re.M)
    total, ids, by_batch = 0, [], {}
    for bk in books:
        txt = io.open(bk, encoding="utf-8").read()
        blocks = re.split(r"(?=^### UNX-F\d+ · )", txt, flags=re.M)[1:]
        if len(blocks) != 20: errs.append(f"查2 FAIL: {os.path.basename(bk)} 条数 {len(blocks)} != 20")
        batch_sum = 0
        for blk in blocks:
            m = entry_re.search(blk); eid = int(m.group(1)); ids.append(eid)
            # 查3 字长（去空白全字符）
            n = len(re.sub(r"\s", "", blk))
            if n < 420: errs.append(f"查3 FAIL: F{eid} 字长 {n} < 420")
            # 查6 行数
            mm = meta_re.search(blk)
            if not mm: errs.append(f"查7 FAIL: F{eid} 元数据行缺失"); continue
            if mm.group(1) not in os.path.basename(bk): errs.append(f"查6 FAIL: F{eid} 批号与册名不符")
            batch_sum += int(mm.group(3))
            # 查7 状态
            if "[已深化]" not in blk: errs.append(f"查7 FAIL: F{eid} 非 [已深化]")
        if batch_sum != 6000: errs.append(f"查6 FAIL: {os.path.basename(bk)} 行数 {batch_sum} != 6000")
        total += batch_sum
    if total != 240000: errs.append(f"查6 FAIL: 域总行数 {total} != 240000")
    # 查4 判据号唯一（每条元数据一枚）
    crit_ids = re.findall(r"UNX-F\d+-J1", "".join(io.open(b, encoding="utf-8").read() for b in books))
    meta_ids = []
    for bk in books:
        meta_ids += re.findall(r"判据：(UNX-F\d+-J1)", io.open(bk, encoding="utf-8").read())
    if len(meta_ids) != 800: errs.append(f"查2 FAIL: 元数据判据号 {len(meta_ids)} != 800")
    if len(set(meta_ids)) != 800: errs.append("查4 FAIL: 判据号有重复")
    if sorted(ids) != list(range(42401, 43201)): errs.append("查5 FAIL: ID 不连续/越段")
    # 查5 与主册一致
    main_txt = io.open(MAIN, encoding="utf-8").read()
    for i in meta_ids:
        if i + " " not in main_txt and i not in main_txt:
            errs.append(f"查5 FAIL: {i} 不在主册域账"); break
    if errs:
        print("\n".join(errs)); print(f"FAIL {len(errs)}"); sys.exit(1)
    print("七查 ALL PASS exit=0（40 册/800 条/≥420 字符/判据唯一/与主册一致/批守恒 240,000/状态已深化）")

if __name__ == "__main__":
    main()
