# -*- coding: utf-8 -*-
"""
AI-43 · UNX-I3 续产段生成器（波 17 · B16–B40 · F33901–F34400 · 500 条）
内建七断言：500 条 / ID 连续零空洞 / 25 批 × 6,000 = 150,000 守恒 / 判据 500 枚唯一 /
批次序号错位 / 域闭账物主办条目 F34400 归位 / 主汇编册防重。
产出：
  docs/unxreal/batches/UNX-I3-B16..B40.md（25 件批册）
  汇编册续产段增补卷（纯追加到主汇编册尾部）
"""
import os, sys

HERE = os.path.dirname(os.path.abspath(__file__))
ROOT = os.path.abspath(os.path.join(HERE, "..", "..", ".."))
sys.path.insert(0, HERE)
from _i3_data_c import BATCHES_C
from _i3_data_d import BATCHES_D

BATCHES = BATCHES_C + BATCHES_D
BASE_ID = 33901
ROWS_PER_BATCH = 6000
VOL_TITLE = "## 增补卷 · AI-43 · 波17 续产段 I3 域骨架立账（B16–B40 · F33901–F34400 · 500 条）"
MAIN = os.path.join(ROOT, "docs", "Varix", "CoRun Varix STAR II · Unxreal", "CoRun Varix STAR II · Unxreal.md")
BATCH_DIR = os.path.join(ROOT, "docs", "unxreal", "batches")

HEADER_COMMON = ("｜判据主轴：无线漫游零断账（波 17 闭账物主办 · 计量器先校准后量数）"
 "｜嫁接源：wpa_supplicant（只跟随绝不 fork · 版本钉 AI-94 台账）"
 "｜防重：续产段 ID F33901–F34400 与首产段/已收口域零撞号（grep 五范围）"
 "｜上游：AI-41（R-I3-001 fake 先行）；横向 AI-42/48、下游 AI-44/45/50/64 联签；AI-98 双账会签"
 "｜红线声明：本批无引导设施红线与硬件数据安全红线触发条目（预申报为空）；漫游账逐次实录禁平均达标掩盖单次超标（红线第 1 条加严）；真机/双 AP 判据随闸门补测（开发期零 QEMU 零实机写，R-I3-002）；体验日志/异常显性化/交互词典三条间接纪律全程生效")

def build():
    entries_out = []
    batches_md = {}
    fid = BASE_ID
    cumulative = 90000  # 首产段已锁 90,000
    for bi, b in enumerate(BATCHES):
        assert b["id"] == f"B{bi+16:02d}", f"批次序号错位: {b['id']}"
        assert len(b["entries"]) == 20, f"{b['id']} 条数 {len(b['entries'])} != 20"
        rows_sum = 0
        lines = []
        lines.append(f"# UNX-I3-{b['id']} · {b['theme']}（F{fid:05d}–F{fid+19:05d} · 20 条）")
        lines.append("")
        cumulative += ROWS_PER_BATCH
        lines.append(f"> AI-43 承办（波 17 续产段立账 · 全域 800 条收官段）｜批主题：{b['theme']}｜域账累计：{cumulative} / 240,000{HEADER_COMMON}")
        lines.append("")
        for (name, rows, crit) in b["entries"]:
            e_fid = fid
            lines.append(f"### UNX-F{e_fid:05d} · {name}")
            lines.append(f"- 域/批：I3/{b['id']}｜纯功能行数：{rows}｜状态：[骨架]｜判据：UNX-F{e_fid:05d}-J1 {crit}")
            entries_out.append((e_fid, b["id"], name, rows, crit))
            rows_sum += rows
            fid += 1
        assert rows_sum == ROWS_PER_BATCH, f"{b['id']} 行数 {rows_sum} != 6000"
        batches_md[b["id"]] = "\n".join(lines) + "\n"
    # 断言 1：500 条
    assert len(entries_out) == 500, f"总条数 {len(entries_out)} != 500"
    # 断言 2：ID 连续零空洞
    ids = [e[0] for e in entries_out]
    assert ids == list(range(BASE_ID, BASE_ID + 500)), "ID 非连续"
    # 断言 3：守恒
    total = sum(e[3] for e in entries_out)
    assert total == 25 * ROWS_PER_BATCH == 150000, f"总行数 {total} != 150000"
    # 断言 4：判据唯一
    crits = [e[4] for e in entries_out]
    assert len(set(crits)) == 500, "判据存在重复"
    fids = [e[0] for e in entries_out]
    assert len(set(fids)) == 500, "fid 重复"
    # 断言 5：批次序号（B16–B40 与 ID 区间绑定已在循环断言，此处终验）
    id_map = {e[0]: e for e in entries_out}
    assert id_map[33901][1] == "B16" and id_map[34400][1] == "B40", "首尾批位绑定异常"
    # 断言 6：域闭账物主办条目 F34400 归位
    last = id_map[34400]
    assert "闭账" in last[2] and "主办" in last[2], "F34400 未承载域闭账物主办语义"
    # 断言 7：首条 F33901 归位 B16
    assert "三档选择器" in id_map[33901][2], "F33901 首条语义异常"
    print("[ASSERT-1..7] ALL PASS（500 条 / F33901–F34400 连续零空洞 / 150,000 行守恒 / 判据 500 枚唯一 / 批序 B16–B40 / F34400 闭账主办归位 / F33901 归位）")
    return batches_md

VOL_INTRO = (
 f"> AI-43 承办（波 17 续产段 · 全域收官）：UNX-I3 域续产段 25 批 × 20 条骨架（域账累计 240,000 / 240,000，与首产段 90,000 合计域账全额立账）。"
 f"主题框架承任务书：M 型后段 B16–B20（漫游执行三档执行面 + 网管面状态总账与事件流）、E 型边界 B21–B28（异常 AP 拒止/环境劣化/断电恢复/资源耗尽/fuzz 汇总/时序竞态/休眠唤醒移动/域级终章）、I 型联签 B29–B36（I1/I2/I4/I5/J3/J5/AI-64 七向联签 + 终章）、C 型收官 B37–B40（J1 漫游零断账 ×50 复测总账 / J6 网管 CLI ×40 回归总账 / 20 维度验收总对账 / 闭账物六包与波 17 闭账宣告）。"
 f"域闭账物主办条目 F34400 于 B40 末条兑现（任务书判据主轴主办义务）；全条目 300 行配平，25 批 × 6,000 = 150,000 行守恒；判据 500 枚唯一可运行 UNX-F34xxx-J1 体例。"
 f"批册全文 batches/UNX-I3-B16..B40.md 二十五件收录如下。生成器 docs/unxreal/gen/_i3_secondprod.py 内建七断言 ALL PASS；真机/双 AP 判据随闸门补测（R-I3-002）；联签登记随各批落档。\n"
)

def append_main_volume(batches_md):
    with open(MAIN, "r", encoding="utf-8") as f:
        main_text = f.read()
    assert VOL_TITLE not in main_text, "增补卷标题已存在（防重）"
    # 防重：续产段 500 个 ID 均不得已存在于汇编册
    for i in range(BASE_ID, BASE_ID + 500):
        assert f"UNX-F{i:05d}" not in main_text, f"汇编册已存在 ID（防重）: UNX-F{i:05d}"
    with open(MAIN, "a", encoding="utf-8") as f:
        f.write("\n" + VOL_TITLE + "\n\n" + VOL_INTRO)
        for bid in sorted(batches_md):
            f.write(batches_md[bid] + "\n")
    print(f"[APPEND] 汇编册续产段增补卷已纯追加 → {MAIN}")

def write_batches(batches_md):
    os.makedirs(BATCH_DIR, exist_ok=True)
    for bid, text in batches_md.items():
        p = os.path.join(BATCH_DIR, f"UNX-I3-{bid}.md")
        if os.path.exists(p):
            with open(p, "r", encoding="utf-8") as f:
                assert f.read() == text, f"批册已存在且内容不一致（防重）: {p}"
            continue
        with open(p, "w", encoding="utf-8") as f:
            f.write(text)
    print(f"[WRITE] 25 件批册落盘 → {BATCH_DIR}/UNX-I3-B16..B40.md")

if __name__ == "__main__":
    bm = build()
    if "--dry" in sys.argv:
        print("[DRY] 未写盘")
        sys.exit(0)
    write_batches(bm)
    append_main_volume(bm)
    print("[DONE] I3 续产段立账完成")
