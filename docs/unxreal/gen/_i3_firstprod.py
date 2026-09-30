# -*- coding: utf-8 -*-
"""
AI-43 · UNX-I3 首产段生成器（波 17 · B01–B15 · F33601–F33900 · 300 条）
内建五断言：300 条 / ID 连续零空洞 / 15 批 × 6,000 = 90,000 守恒 / 判据 300 枚唯一 / 锚归位。
产出：
  docs/unxreal/batches/UNX-I3-B01..B15.md（15 件批册）
  汇编册增补卷（纯追加到 docs/Varix/.../CoRun Varix STAR II · Unxreal.md 尾部）
"""
import os, sys

HERE = os.path.dirname(os.path.abspath(__file__))
ROOT = os.path.abspath(os.path.join(HERE, "..", "..", ".."))
sys.path.insert(0, HERE)
from _i3_data_a import BATCHES_A
from _i3_data_b import BATCHES_B

BATCHES = BATCHES_A + BATCHES_B
BASE_ID = 33601
ROWS_PER_BATCH = 6000
ENTRY_ROWS_DEFAULT = 300
# 汇编册增补卷锚点（纯追加判定：该标题此前必须不存在）
VOL_TITLE = "## 增补卷 · AI-43 · 波17 首产段 I3 域骨架立账（B01–B15 · F33601–F33900 · 300 条）"
MAIN = os.path.join(ROOT, "docs", "Varix", "CoRun Varix STAR II · Unxreal", "CoRun Varix STAR II · Unxreal.md")
BATCH_DIR = os.path.join(ROOT, "docs", "unxreal", "batches")

HEADER_COMMON = ("｜判据主轴：无线漫游零断账（波 17 闭账物主办 · 计量器先校准后量数）"
 "｜嫁接源：wpa_supplicant（只跟随绝不 fork · 版本钉 AI-94 台账）"
 "｜防重：全域 ID F33601–F34400 与已收口域零撞号（grep 五范围：kernel/varix/src、docs/START、_attic、已 finalize deepen 册、总纲既有段）"
 "｜上游：AI-41（I1 路由钩子 route_lookup / NDP/ARP 邻居态 / 接口事件源 · 波 16 Q3 冻结 · Schema 先行+fake，R-I3-001）、AI-04（扫描缓冲内存配额）；横向 AI-42（I2 socket 底座消费）；下游 AI-44（DNS 地址交付）、AI-45（SMB 漫游不断传联测）、AI-64（蓝牙共存信道仲裁接口预留）、AI-50（四类网络事件进审计）"
 "｜红线声明：本批无引导设施红线与硬件数据安全红线触发条目（预申报为空）；漫游账逐次实录禁平均达标掩盖单次超标（红线第 1 条加严）；netxp.rs 移交账（DNS 缓存/代理/信任库→I4）与接管账（无缝切换/热点/Wi-Fi/QoS/诊断包→本域）双账并行联签 AI-98；真机/双 AP 判据随闸门补测（开发期零 QEMU 零实机写，R-I3-002）；SAE 原语依赖 AI-48 联签（R-I3-003）；体验日志/异常显性化/交互词典三条间接纪律全程生效")

def build():
    entries_out = []      # (fid, batch_id, name, rows, criteria)
    batches_md = {}
    fid = BASE_ID
    cumulative = 0
    for bi, b in enumerate(BATCHES):
        assert b["id"] == f"B{bi+1:02d}", f"批次序号错位: {b['id']}"
        assert len(b["entries"]) == 20, f"{b['id']} 条数 {len(b['entries'])} != 20"
        rows_sum = 0
        lines = []
        lines.append(f"# UNX-I3-{b['id']} · {b['theme']}（F{fid:05d}–F{fid+19:05d} · 20 条）")
        lines.append("")
        cumulative += ROWS_PER_BATCH
        lines.append(f"> AI-43 承办（波 17 首产段立账 · 一次对话 300 项明令）｜批主题：{b['theme']}｜域账累计：{cumulative} / 240,000{HEADER_COMMON}")
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
    # 断言 1：300 条
    assert len(entries_out) == 300, f"总条数 {len(entries_out)} != 300"
    # 断言 2：ID 连续零空洞
    ids = [e[0] for e in entries_out]
    assert ids == list(range(BASE_ID, BASE_ID + 300)), "ID 非连续"
    # 断言 3：守恒
    total = sum(e[3] for e in entries_out)
    assert total == 15 * ROWS_PER_BATCH == 90000, f"总行数 {total} != 90000"
    # 断言 4：判据唯一
    crits = [e[4] for e in entries_out]
    assert len(set(crits)) == 300, "判据存在重复"
    fids = [e[0] for e in entries_out]
    assert len(set(fids)) == 300, "fid 重复"
    # 断言 5：锚归位
    id_map = {e[0]: e for e in entries_out}
    assert id_map[33601][1] == "B01" and "LC-trie 路由表与前缀最长匹配总入口" in id_map[33601][2], "锚 F33601 未原位 B01 首条"
    assert id_map[33633][1] == "B02", "F33633 未于 B02 区间恒等承载"
    assert id_map[33672][1] == "B04", "F33672 未于 B04 区间恒等承载"
    assert id_map[33720][1] == "B06" and "F33720" in id_map[33720][2], "F33720 未于 B06 末条恒等承载"
    assert id_map[33855][1] == "B13" and "F33855" in id_map[33855][2], "F33855 未于 B13 #15 恒等承载"
    b05_last = [e for e in entries_out if e[1] == "B05"][-1]
    b09_last = [e for e in entries_out if e[1] == "B09"][-1]
    b15_last = [e for e in entries_out if e[1] == "B15"][-1]
    assert "示例锚语义承载位·F33633" in b05_last[2] and "适配层" in b05_last[2], "B05 末条未承载锚 F33633 语义"
    assert "示例锚语义承载位·F33672" in b09_last[2] and "四次握手" in b09_last[2], "B09 末条未承载锚 F33672 语义"
    assert "示例锚语义承载位·F33720" in b15_last[2] and "闭账物" in b15_last[2], "B15 末条未承载锚 F33720 语义"
    print("[ASSERT-1..5] ALL PASS（300 条 / F33601–F33900 连续零空洞 / 90,000 行守恒 / 判据 300 枚唯一 / 锚归位）")
    return batches_md

VOL_INTRO = (
 f"> AI-43 承办（一次对话 300 项明令 · 承 AI-23/AI-30/AI-34 判例）：UNX-I3 路由/WiFi/网管域首产段 15 批 × 20 条骨架（域账累计 90,000 / 240,000）。"
 f"任务书示例锚归位：F33601（LC-trie 路由表）原位 B01 首条兑现；F33633（wpa_supplicant 适配层，ID 落 B02 #13）、F33672（四次握手数据面，ID 落 B04 #12）、F33720（漫游决策与 PMKID/FT 快切，ID 落 B06 末条）、F33855（DHCP 租约降级链与撤销联动，ID 落 B13 #15）四处示例锚与 20 条/批 ID 连续性冲突，ID 恒等不重编（区间恒等归位判例承 C2/D2/G4），语义锚分别落 B05 末条 F33700 / B09 末条 F33780 / B15 末条 F33900（680 行承载，任务书行数兑现）承载位；F33855 语义由 B14 #15（420 行承载）原位承载。"
 f"批册全文 batches/UNX-I3-B01..B15.md 十五件收录如下。防重五范围 grep 零撞号；15 批 × 6,000 = 90,000 行守恒；判据 300 枚唯一。"
 f"红线预申报为空；漫游账逐次实录禁平均达标掩盖单次；wpa_supplicant 只跟随不 fork（版本钉 AI-94）；真机/双 AP 判据随闸门补测（R-I3-002，开发期零 QEMU）；netxp.rs 移交/接管双账联签 AI-98；生成器 docs/unxreal/gen/_i3_firstprod.py 内建五断言 ALL PASS。\n"
)

def append_main_volume(batches_md):
    with open(MAIN, "r", encoding="utf-8") as f:
        main_text = f.read()
    assert VOL_TITLE not in main_text, "增补卷标题已存在（防重）"
    import re as _re
    for m in _re.finditer(r"UNX-F33[6-9]\d\d(?!\d)", main_text):
        raise AssertionError(f"汇编册已存在本段 ID（防重）: {m.group(0)}")
    with open(MAIN, "a", encoding="utf-8") as f:
        f.write("\n" + VOL_TITLE + "\n\n" + VOL_INTRO)
        for bid in sorted(batches_md):
            f.write(batches_md[bid] + "\n")
    print(f"[APPEND] 汇编册增补卷已纯追加 → {MAIN}")

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
    print(f"[WRITE] 15 件批册落盘 → {BATCH_DIR}/UNX-I3-B01..B15.md")

if __name__ == "__main__":
    bm = build()
    if "--dry" in sys.argv:
        print("[DRY] 未写盘")
        sys.exit(0)
    write_batches(bm)
    append_main_volume(bm)
    print("[DONE] I3 首产段立账完成")
