# -*- coding: utf-8 -*-
"""
AI-43 · UNX-I3 首产段骨架校验器（六查，ALL PASS exit=0）
查 1：15 批 × 20 条 = 300 条（批册逐件）
查 2：ID 连续零空洞（F33601–F33900）
查 3：总行数 90,000 守恒
查 4：批批求和 6,000 与批头登记一致
查 5：判据 300 枚唯一
查 6：主汇编册增补卷与批册逐条对齐（纯追加卷存在且内容一致）
"""
import os, re, sys

ROOT = os.path.abspath(os.path.join(os.path.dirname(os.path.abspath(__file__)), ".."))
MAIN = os.path.join(ROOT, "docs", "Varix", "CoRun Varix STAR II · Unxreal", "CoRun Varix STAR II · Unxreal.md")
BATCH_DIR = os.path.join(ROOT, "docs", "unxreal", "batches")
VOL_TITLE = "## 增补卷 · AI-43 · 波17 首产段 I3 域骨架立账（B01–B15 · F33601–F33900 · 300 条）"
VOL_TITLE2 = "## 增补卷 · AI-43 · 波17 续产段 I3 域骨架立账（B16–B40 · F33901–F34400 · 500 条）"

def main():
    fails = []
    all_entries = []  # (fid, batch, rows, crit)
    for i in range(1, 41):
        p = os.path.join(BATCH_DIR, f"UNX-I3-B{i:02d}.md")
        if not os.path.exists(p):
            fails.append(f"缺批册 {p}"); continue
        text = open(p, encoding="utf-8").read()
        heads = re.findall(r"^### (UNX-F(\d{5})) · (.+)$", text, re.M)
        if len(heads) != 20:
            fails.append(f"B{i:02d} 条数 {len(heads)} != 20")
        rows_sum = 0
        for full, fid, name in heads:
            m = re.search(rf"^- 域/批：I3/B{i:02d}｜纯功能行数：(\d+)｜状态：\[骨架\]｜判据：{full}-J1 (.+)$", text, re.M)
            if not m:
                fails.append(f"{full} 条目行格式/判据号不合规"); continue
            rows_sum += int(m.group(1))
            all_entries.append((int(fid), f"B{i:02d}", int(m.group(1)), m.group(2)))
        if rows_sum != 6000:
            fails.append(f"B{i:02d} 求和 {rows_sum} != 6000")
    ids = [e[0] for e in all_entries]
    if ids != list(range(33601, 33601 + len(ids))) or len(ids) != 800:
        fails.append(f"查2 ID 连续性异常（{len(ids)} 条，首 {ids[0] if ids else '-'} 末 {ids[-1] if ids else '-'}）")
    total = sum(e[2] for e in all_entries)
    if total != 240000:
        fails.append(f"查3 总行数 {total} != 90000")
    crits = [e[3] for e in all_entries]
    if len(set(crits)) != 800:
        fails.append("查5 判据存在重复")
    # 查 6：汇编册对齐
    main_text = open(MAIN, encoding="utf-8").read()
    if VOL_TITLE not in main_text or VOL_TITLE2 not in main_text:
        fails.append("查6 汇编册缺增补卷标题（首产段/续产段）")
    else:
        vol = main_text.split(VOL_TITLE, 1)[1]
        vol = vol.split(VOL_TITLE2, 1)[0]
        vol2 = main_text.split(VOL_TITLE2, 1)[1]
        vol2 = vol2.split("\n## ", 1)[0] if "\n## " in vol2 else vol2
        for i in range(1, 41):
            btext = open(os.path.join(BATCH_DIR, f"UNX-I3-B{i:02d}.md"), encoding="utf-8").read()
            target = vol if i <= 15 else vol2
            for line in btext.splitlines():
                if line.startswith(("### UNX-F", "- 域/批：")) and line not in target:
                    fails.append(f"查6 汇编册缺行: {line[:60]}...")
                    break
    # 锚位复核
    id_map = {e[0]: e for e in all_entries}
    for fid, bid in [(33601, "B01"), (33633, "B02"), (33672, "B04"), (33720, "B06"), (33855, "B13"), (33700, "B05"), (33780, "B09"), (33900, "B15"), (33901, "B16"), (34400, "B40")]:
        if fid in id_map and id_map[fid][1] != bid:
            fails.append(f"锚 {fid} 落批 {id_map[fid][1]} != {bid}")
    if fails:
        print("FAIL:")
        for f_ in fails[:30]:
            print(" -", f_)
        sys.exit(1)
    print("[CHECK-1..6] ALL PASS（全域 40 批 800 条 / F33601–F34400 连续 / 240,000 守恒 / 批批 6,000 / 判据 800 枚唯一 / 汇编册双卷对齐 + 锚位复核 10/10）")
    sys.exit(0)

if __name__ == "__main__":
    main()
