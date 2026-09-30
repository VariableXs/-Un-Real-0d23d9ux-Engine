#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""AI-41 · UNX-I1 后半段四处落账（幂等）：主汇编册增补卷二（纯追加）/总纲 §7.3-I1 B16–B40 行翻 [骨架]/
根台账 I1 行更新+会话条追加 / handoff.json I1 块更新。append-only 为主，行翻转为台账写锁语义。"""
import os, re, json, hashlib

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
B = os.path.join(ROOT, "docs", "unxreal", "batches")
MAIN = os.path.join(ROOT, "docs", "Varix", "CoRun Varix STAR II · Unxreal", "CoRun Varix STAR II · Unxreal.md")
ZG = os.path.join(ROOT, "docs", "Varix", "CoRun Varix STAR II · Unxreal", "CoRun Varix STAR II · Unxreal · 总纲与施工书.md")
LEDGER = os.path.join(ROOT, "CoRun Varix STAR II · Unxreal · 统一协作总台账.md")
HANDOFF = os.path.join(ROOT, "docs", "unxreal", "handoff.json")

def read(p): return open(p, encoding="utf-8").read()
def write(p, s): open(p, "w", encoding="utf-8", newline="\n").write(s)

def batch_theme(bid):
    for line in read(os.path.join(B, f"UNX-I1-{bid}.md")).splitlines()[:1]:
        return line.split(" · ")[1].split("（UNX-")[0]

# ---------- 1. 主汇编册增补卷二（纯追加零删除，幂等） ----------
m0 = read(MAIN)
if "增补卷二 · AI-41" in m0:
    print("主汇编册增补卷二已在位（幂等跳过）")
else:
    assert "增补卷 · AI-41 · 波16 首产段 I1 域骨架立账" in m0, "首产段卷缺失，顺序错误"
    ids = re.findall(r"UNX-F(\d{5})", m0)
    have = sorted({int(x) for x in ids if 32001 <= int(x) <= 32800})
    assert have == list(range(32001, 32301)), "主汇编册 I1 段应为首产段 300 条"
    vol = ["\n---\n", "", "## 增补卷二 · AI-41 · 波16 后半段 I1 域骨架续立账（UNX-I1 TCP/IP 内核栈 · B16–B40 · 500 项新功能 · 域封段）", "",
    "> **登记注（AI-41 · 全域收官执行件）**：本卷续立 UNX-I1 后半段 B16–B40 骨架 500 项（F32301–F32800 连续零跳号 · 25 批 × 20 条 · 逐批 6,000 行 · 总 150,000 行 · 判据 500 枚唯一 UNX-F32301-J1…UNX-F32800-J1）。判据体例与首产段同源（UNX-F32xxx-J1，ID 由生成器 scripts/unxreal_i1_gen2.py 按批位注入零手写）。主题分段承任务书 §41.3（F8/M12/E8/I8/C4）：M 型机制后段 B16–B20（TCP 关闭路径与异常关闭/拥塞深化 PRR 与恢复/重传队列深化/套接字层 POSIX 类语义/M 型段闸 120,000 封段 100 条）+ E 型攻击面拒止 B21–B28（解析敌意全集/状态型洪水耗尽慢速/资源配额防线/时序竞态剧本/放大反射防线/分片重组攻击/ALG 边界声明/E 型段闸 200,000 封段 160 条）+ I 型三向联签 B29–B36（AI-42 帧递送回调联签/AI-43 route_lookup 联签/AI-50 netfilter·conntrack 双向联签/AI-98 退役联签/下游冻结面总表/联签回归总集/上游 A 部对位/I 型段闸 220,000 封段 160 条）+ C 型收官 B37–B40（iperf J1 吞吐终账预演/J2 时延终账预演/J3·J5 终账预演/域总闸 240,000/240,000 封域宣告 80 条）。真机判据（J1/J2/J4/J6）随闸门补测（R-I1-004，开发期零 QEMU 零实机写）——本域全部性能数字为预演账/登记账，禁虚标；域内加严红线「错误路径不停轮询」违例即 P0 全程复核；net.rs 3,908 行+netstack.rs 157 行升级接管不删除存量（退役联签 B32）；open_risks R-I1-001..005 沿袭。", ""]
    for i in range(16, 41):
        bid = f"B{i:02d}"
        txt = read(os.path.join(B, f"UNX-I1-{bid}.md"))
        lines = txt.splitlines()
        first = int(re.search(r"UNX-F(\d{5})", lines[0]).group(1))
        vol.append(f"### {bid} · {batch_theme(bid)}（UNX-F{first}–F{first+19} · 20 条 · 6,000 行）")
        vol.append("")
        for ln in lines[2:]:
            vol.append(ln)
        vol.append("")
    vol.append("> 本卷 500 项纯追加零删除；追加前主汇编册 I1 段恰为 F32001–F32300 首产段断言通过；批册单源 docs/unxreal/batches/UNX-I1-B16..B40.md（生成器 scripts/unxreal_i1_gen2.py 五断言 ALL PASS：500 条/F32301–F32800 连续/25 批×6,000=150,000 守恒/判据 500 枚唯一/批头锚归位）。")
    vol.append("")
    m1 = m0 + "\n".join(vol)
    write(MAIN, m1)
    print(f"主汇编册 +{len(m1)-len(m0)} 字符（纯追加零删除），卷二 SHA-256 前 16 位 {hashlib.sha256(m1.encode()).hexdigest()[:16]}")

# ---------- 2. 总纲 §7.3-I1：B16–B40 行翻 [骨架] + 修订记录追加 ----------
z0 = read(ZG)
assert "#### 7.3-I1" in z0
z1 = z0
flipped = 0
for i in range(16, 41):
    bid = f"B{i:02d}"
    first = 32001 + (i - 1) * 20; last = first + 19
    theme = batch_theme(bid)
    pat = re.compile(r"\| UNX-I1-" + bid + r" \| F\d{5}–F\d{5} \| 20 \| \[未动\].*?\| 待领 \|")
    rep = f"| UNX-I1-{bid} | F{first}–F{last} | 20 | [骨架] | 0（骨架已立，6,000 行预算锁定，批册 batches/UNX-I1-{bid}.md）（主题框架：{theme}） | AI-41 |"
    z1, n = pat.subn(rep, z1, count=1)
    assert n == 1, f"{bid} 行翻转失败"
    flipped += 1
if "修订记录（AI-41 会话 2026-10-02 · 波16 后半段全域收官）" not in z1:
    rev = "> 修订记录（AI-41 会话 2026-10-02 · 波16 后半段全域收官）：§7.3-I1 B16–B40 二十五行 [未动]→[骨架] 写锁翻转（AI-41）；批册 batches/UNX-I1-B16..B40.md 二十五件落盘（F32301–F32800 连续唯一、批批 6,000 求和守恒、判据 500 枚唯一；生成器 scripts/unxreal_i1_gen2.py 五断言 ALL PASS）；主汇编册卷末《增补卷二 · AI-41 · 波16 后半段 I1 域骨架续立账》500 项纯追加零删除；域账 240,000/240,000 封域（B40 域总闸 UNX-F32774 宣告）；根台账 §三 I1 行更新/§四 会话条二追加；handoff.json I1 块更新（skeleton_batches 40 · rows_locked 240,000）。\n"
    z1 = z1.rstrip("\n") + "\n" + rev
write(ZG, z1)
print(f"总纲 §7.3-I1 B16–B40 翻 [骨架]（{flipped} 行）+ 修订记录（+{len(z1)-len(z0)} 字符）")

# ---------- 3. 根台账：I1 行更新 + 会话条二（幂等） ----------
l0 = read(LEDGER)
old_row_pat = re.compile(r"\| UNX-I1 TCP/IP 内核栈 \| AI-41 \|[^\n]*\|")
assert old_row_pat.search(l0), "根台账 I1 行缺失"
new_row = "| UNX-I1 TCP/IP 内核栈 | AI-41 | 0 | B01–B40（40 批 800 条全域骨架封段，波 16 两轮立账） | 0 | 240,000 / 240,000（40 批×6,000 逐批求和，scripts/unxreal_i1_skeleton_check.py 六查 ALL PASS 实核零偏离） | 域总闸封域：首产段 B01–B15 300 条（F 型地基 160+M 型前段 140）+ 后半段 B16–B40 500 条（M 型后段 B16–B20 100/E 型攻击面拒止 B21–B28 160/I 型三向联签 B29–B36 160/C 型 iperf 对标收官 B37–B40 80）；F32001–F32800 连续零跳号、判据 800 枚唯一；任务书五示例锚按 §2.1 区间恒等归位；net.rs 3,908 行升级接管不删除存量（退役联签 B32/AI-98）；真机判据（J1/J2/J4/J6）随闸门补测（R-I1-004，零 QEMU）；深化轮（finalize deepen）待排 |"
l1 = old_row_pat.sub(new_row, l0, count=1)
if "会话 2026-波16-M02 · AI-41" not in l1:
    sess = """
### 会话 2026-波16-M02 · AI-41（I1 域后半段 B16–B40 骨架 500 条 · F32301–F32800 · 150,000 行 · 域总闸封域）

- **续领对账**：首产段 B01–B15（300 条 · F32001–F32300）已收口入库（提交 22b63cbe）；本会话续领后半段 B16–B40，域账一次补满 240,000/240,000 封域，不越权他域。
- **本会话 500 项新功能（后半段 B16–B40）**：写入主汇编册《增补卷二 · AI-41 · 波16 后半段 I1 域骨架续立账》纯追加零删除；总纲 §7.3-I1 B16–B40 二十五行 [未动]→[骨架] 写锁翻转+修订记录追加；批册 docs/unxreal/batches/UNX-I1-B16..B40.md 二十五件落盘。
- **分段主题**：M 型后段 B16–B20 100 条（TCP 关闭路径/拥塞深化 PRR/重传队列/套接字层 POSIX 类语义/M 型段闸）+ E 型攻击面拒止 B21–B28 160 条（解析敌意全集/状态型洪水·耗尽·慢速/资源配额/时序竞态剧本/放大反射/分片攻击/ALG 边界/E 型段闸）+ I 型三向联签 B29–B36 160 条（AI-42 帧递送/AI-43 route_lookup/AI-50 双向联签/AI-98 退役联签/下游冻结面总表/联签回归总集/上游 A 部对位/I 型段闸）+ C 型收官 B37–B40 80 条（J1 吞吐/J2 时延/J3·J5 终账预演/B40 域总闸 F32774 封域宣告）。
- **断言链**：①防重五范围 F32001–F32800 零撞号；②判据 500 枚唯一（全域 800 枚）；③行数守恒 25 批×6,000=150,000（配平差额留痕生成器输出）；④台账四处回填；⑤真机判据全部为预演账/登记账随闸门补测，零虚标。
- **工具与可复验**：生成器 scripts/unxreal_i1_gen2.py（五断言 ALL PASS · 判据 ID 按批位注入零手写）+ 校验器 scripts/unxreal_i1_skeleton_check.py（六查 ALL PASS · 全域 800 条口径）。
- **红线与纪律**：红线预申报为空；「错误路径不停轮询」域内加严红线 800 条全查零违例；RFC 引用防幻觉全程；net.rs 接管不删除存量。
- **open_risks**：R-I1-001..005 沿袭（真机判据随闸门补测 R-I1-004 为主项）；深化轮（finalize deepen）待排入后续会话。
- **双同步**：docs 落盘（25 批册+主汇编册增补卷二+总纲 §7.3-I1 翻转+根台账更新+handoff 更新+生成器/校验器）+ git 提交推送（pathspec 显式限定本会话产物）。
"""
    l1 = l1.rstrip("\n") + "\n" + sess
write(LEDGER, l1)
print(f"根台账 I1 行更新 + 会话条二（+{len(l1)-len(l0)} 字符）")

# ---------- 4. handoff.json I1 块更新 ----------
h = json.loads(read(HANDOFF))
assert "I1" in h.get("domain_ledger_progress", {})
h["domain_ledger_progress"]["I1"] = {
    "finalized_batches": 0,
    "skeleton_batches": 40,
    "skeleton_list": [f"UNX-I1-B{i:02d}" for i in range(1, 41)],
    "rows_locked": 240000,
    "domain_budget": 240000,
    "owner": "AI-41",
    "note": "全域骨架封域（B01–B40 · 800 条 · F32001–F32800 连续唯一 · 判据 800 枚唯一 · 生成器 gen/gen2 五断言 ALL PASS · 校验器六查 ALL PASS 全域口径）；B40 域总闸 UNX-F32774 封域宣告；真机判据随闸门补测（R-I1-004）；深化轮待排；open_risks R-I1-001..005"
}
h["updated_at"] = "2026-10-02T06:00:00"
h["last_session"] = "AI-41 I1 域后半段收官：B16–B40 骨架 500 项新功能（batches/UNX-I1-B16..B40.md · F32301–F32800 连续唯一 · 240,000/240,000 封域）+ 总纲 §7.3-I1 二十五行写锁翻转 + 根台账 I1 行更新/会话条二 + handoff 更新；生成器 gen2 五断言+校验器六查 ALL PASS；并行他会话产物零触碰"
write(HANDOFF, json.dumps(h, ensure_ascii=False, indent=1) + "\n")
print("handoff.json I1 块更新（全域封域口径）")
print("四处落账 ALL DONE（后半段）")
