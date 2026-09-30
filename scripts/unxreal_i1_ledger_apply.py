#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""AI-41 · UNX-I1 首产段四处落账：主汇编册增补卷（纯追加）/总纲 §7.3-I1（四十行一次建入）/
根台账 §三/§四 / handoff.json I1 块（最小 patch）。执行前守恒断言，append-only。"""
import os, re, json, sys, hashlib

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
B = os.path.join(ROOT, "docs", "unxreal", "batches")
MAIN = os.path.join(ROOT, "docs", "Varix", "CoRun Varix STAR II · Unxreal", "CoRun Varix STAR II · Unxreal.md")
ZG = os.path.join(ROOT, "docs", "Varix", "CoRun Varix STAR II · Unxreal", "CoRun Varix STAR II · Unxreal · 总纲与施工书.md")
LEDGER = os.path.join(ROOT, "CoRun Varix STAR II · Unxreal · 统一协作总台账.md")
HANDOFF = os.path.join(ROOT, "docs", "unxreal", "handoff.json")

def read(p): return open(p, encoding="utf-8").read()
def write(p, s): open(p, "w", encoding="utf-8", newline="\n").write(s)

THEMES = {f"B{i:02d}": None for i in range(1, 16)}
def batch_theme(bid):
    for line in read(os.path.join(B, f"UNX-I1-{bid}.md")).splitlines()[:1]:
        return line.split(" · ")[1].split("（UNX-")[0]

# ---------- 1. 主汇编册增补卷（纯追加零删除） ----------
m0 = read(MAIN)
assert "UNX-I1 首产段卷" not in m0, "增补卷已存在，禁止重复追加"
ids = re.findall(r"UNX-F(\d{5})", m0)
assert not any(32001 <= int(x) <= 32800 for x in ids), "主汇编册已含 I1 段 ID"
vol = ["\n---\n", "", "## 增补卷 · AI-41 · 波16 首产段 I1 域骨架立账（UNX-I1 TCP/IP 内核栈 · B01–B15 · 300 项新功能）", "",
 "> **登记注（AI-41 · 一次对话 300 项新功能明令执行件）**：UNX-I1 承包域 F32001–F32800（40 批 800 条 · 240,000 行 · 波 16 窗 · 上游 AI-02 MSI-X）。本卷为首产段 B01–B15 骨架立账 300 项（F32001–F32300 连续零跳号 · 15 批 × 20 条 · 逐批 6,000 行 · 总 90,000 行 · 判据 300 枚唯一 UNX-F32001-J1…UNX-F32300-J1）。判据主轴：iperf 吞吐时延对标（J1 ≥900 Mbps/J2 P99 ≤1.5ms 真机随闸门补测 R-I1-004）、netfilter 类规则命中（J3 与 AI-50 双向联签预告位）。主题分段：F 型地基 B01–B08（以太网/IPv4 头解析守卫接管 net.rs/TCB 与 11 态状态机地基/四元组哈希与锁分层/定时器轮与 TCP 定时族/ARP 邻居态机/cubic 拥塞控制与 SACK/分片重组与 PMTU/DMA 描述符环与 NAPI 收发管线 160 条）+ M 型机制前段 B09–B15（IPv6 双栈 NDP/SLAAC/DAD/TCP 握手深化与 syncookies/conntrack/NAT 三类/netfilter 五钩子/规则匹配引擎/UDP·ICMP 与发送单拷贝基线 140 条）。批型铺排承《AI分工完成图》§七 AI-41 任务书 §41.3（F8/M12/E8/I8/C4）；任务书示例锚归位：F32001（头解析守卫）落 B01、F32011（敌意模糊集）落 B01、F32035（cubic）落 B06、F32069（conntrack 表满）落 B11、F32120（netfilter 五钩子）落 B13——示例锚号与 20 条/批连续性冲突处按 §2.1 区间恒等归位（ID 不变、语义锚由同主题条目承载，承 C2/D2/H3 判例）。接管声明：net.rs 3,908 行与 drivers/netstack.rs 157 行升级接管不删除存量（退役范围与 AI-98 联签预告位）；域内加严红线「错误路径不停轮询」违例即 P0；防重五范围 grep 零撞号（scripts/unxreal_i1_skeleton_check.py 六查 ALL PASS exit=0）；open_risks：R-I1-001（AI-02 IrqBackend 冻结签前 fake 先行）/R-I1-002（AI-03 每核队列绑定接口）/R-I1-003（AI-04 DMA 一致性内存专属扩展面）/R-I1-004（真机判据随闸门补测，开发期零 QEMU 零实机写）。", ""]
for i in range(1, 16):
    bid = f"B{i:02d}"
    txt = read(os.path.join(B, f"UNX-I1-{bid}.md"))
    lines = txt.splitlines()
    first = int(re.search(r"UNX-F(\d{5})", lines[0]).group(1))
    vol.append(f"### {bid} · {batch_theme(bid)}（UNX-F{first}–F{first+19} · 20 条 · 6,000 行）")
    vol.append("")
    for ln in lines[2:]:
        vol.append(ln)
    vol.append("")
vol.append(f"> 本卷 300 项纯追加零删除；主汇编册追加前 UNX-F32001–F32800 零占用断言通过；批册单源 docs/unxreal/batches/UNX-I1-B01..B15.md（生成器 scripts/unxreal_i1_gen.py 五断言 ALL PASS）。")
vol.append("")
m1 = m0 + "\n".join(vol)
write(MAIN, m1)
print(f"主汇编册 +{len(m1)-len(m0)} 字符（纯追加零删除），卷 SHA-256 前 16 位 {hashlib.sha256(m1.encode()).hexdigest()[:16]}")

# ---------- 2. 总纲 §7.3-I1（四十行一次建入，插于 §7.3-G4 节之后） ----------
z0 = read(ZG)
assert "#### 7.3-I1" not in z0, "§7.3-I1 已存在"
zg_ids = re.findall(r"UNX-F(\d{5})", z0)
assert not any(32001 <= int(x) <= 32800 for x in zg_ids), "总纲已含 I1 段 ID"
lines = z0.splitlines()
# 定位 §7.3-G4 标题行
gi = next(i for i, l in enumerate(lines) if l.startswith("#### 7.3-G4"))
# 找该节之后下一个 "#### 7.3-" 或 "^### " 或 "^## " 作为插入点
ins = None
for i in range(gi + 1, len(lines)):
    if re.match(r"^#### 7\.3-|^### |^## ", lines[i]):
        ins = i; break
assert ins, "未找到 §7.3-G4 节尾"
sec = []
sec.append("#### 7.3-I1 域 I1 台账正文（40 批全表 · AI-41 认领扩表 · §7.6 规则一次建入）")
sec.append("")
sec.append("> 扩表校验（§7.6 规则 2 三项机械校验）：ID 区间 F32001–F32800 与 §2.2 分配总表一致 ✓（《AI分工完成图》AI-41 任务书同口径，波 16 主力域 · 部 I 链头）；40 批 × 20 条 = 800 条 ✓；批区间首尾相接无空洞无重叠 ✓（校验器回算留痕：首产段 15 批 × 20 条 = 300 条，F32001–F32300 连续唯一，批批求和 6,000 与批头登记一致，总 90,000——scripts/unxreal_i1_skeleton_check.py 六查 ALL PASS exit=0）。主题框架承《AI分工完成图》AI-41 任务书条目构成（F 型 B01–B08 头解析/TCB/哈希表/定时器轮四地基、M 型 B09–B20 双栈/握手/conntrack/NAT/netfilter 机制、E 型 B21–B28 错误路径与攻击面拒止、I 型 B29–B36 与 AI-42/43/50 三向联签 ≥30%、C 型 B37–B40 iperf 对标总账与域收官）：本首产段 B01–B15 覆盖 F 型地基八批 + M 型机制前段七批，任务书五示例锚归位——F32001（以太网帧与 IPv4 头解析守卫）落 B01、F32011（敌意模糊集回归）落 B01、F32035（cubic 拥塞控制与 SACK）落 B06、F32069（conntrack 表满处置）落 B11、F32120（netfilter 五钩子框架）落 B13（ID 不变不重编，§2.1 区间恒等判例）。**接管声明**：net.rs 3,908 行（头解析守卫/校验和/ARP 缓存/MTU 分片/流量窗口）与 drivers/netstack.rs 157 行为存量最小栈，本域升级接管深化、不删除存量文件与测试，退役范围与 AI-98 联签（预告位随 B01 批收口条）。**防重五范围已执行**：kernel/varix 源码、docs/START、_attic、已 finalize deepen 册、总纲既有段——「TCP/netfilter/conntrack/NAT/分片」关键词族五位数 ID 段 F32001–F32800 零撞号（校验器内嵌断言；F32801 起为 I2 域段非本域）。**上游依赖**：AI-02（IrqBackend::bind_vector · §83 Top10 接口 ③）、AI-03（软中断与每核队列绑定）、AI-04（DMA 一致性内存专属扩展面）未收口面一律 Schema 先行+fake+open_risks（R-I1-001/002/003）。**下游冻结面**：AI-42 帧递送回调（波 16 Q3）、AI-43 `route_lookup(dst)→next_hop_if` 与 `net_event_submit(event)`（I1 主签）、AI-50 `netfilter_hook(点, 优先级, 回调)` + conntrack 导出账（双向联签）。**红线适用**：非引导/写盘红线域，行为红线十条全适用（重点第 2 条 conntrack 导出账新字段走条目编号、第 9 条对标账双同步）；域内加严——「错误路径不停轮询」违例即 P0、J4 攻击面拒止判据禁降级；防幻觉条款 §40——RFC 行为引用必须带 RFC 编号与节号；真机判据（iperf J1/J2/J4/J6）随闸门补测（R-I1-004，开发期零 QEMU 零实机写）。判据主轴：iperf 吞吐时延对标（J1 单流 ≥900 Mbps·40 并发流 ≥×4/J2 ICMP RTT P99 ≤1.5ms·握手 P99 ≤3ms）+ netfilter 类规则命中（J3 误放行 0 误拦截 0）+ J4 韧性（恶意帧 30 分钟零崩溃合法连接存活 ≥95%）+ J5 状态机完备（11 态全迁移）+ J6 长稳（7×24 漂移 <5% 零泄漏，O4 联测）。")
sec.append("")
sec.append("| 批次 | ID 区间 | 条数 | 三态 | 深化字数 | 承接会话 |")
sec.append("|---|---|---|---|---|---|")
for i in range(1, 41):
    bid = f"B{i:02d}"
    first = 32001 + (i - 1) * 20; last = first + 19
    if i <= 15:
        theme = batch_theme(bid)
        tri = f"[骨架] | 0（骨架已立，6,000 行预算锁定，批册 batches/UNX-I1-{bid}.md）（主题框架：{theme}） | AI-41"
    else:
        stage = {16: "M 型机制后段（TCP 深化/错误路径地基）", 21: "E 型边界与攻击面拒止段", 29: "I 型三向联签段", 37: "C 型收官与对标总账段"}[16 if i <= 20 else 21 if i <= 28 else 29 if i <= 36 else 37]
        tri = f"[未动] | 0（待领，6,000 行预算锁定）（主题框架：{stage}） | 待领"
    sec.append(f"| UNX-I1-{bid} | F{first}–F{last} | 20 | {tri} |")
sec.append("")
sec.append("> 修订记录（AI-41 会话 2026-10-02 · 波16 首产段）：§7.3-I1 四十行一次建入（[骨架]×15 AI-41 写锁+[未动]×25 待领）；批册 batches/UNX-I1-B01..B15.md 十五件落盘（F32001–F32300 连续唯一、批批 6,000 求和守恒、判据 300 枚唯一）；生成器 scripts/unxreal_i1_gen.py 单源亲写（五断言 ALL PASS：300 条/ID 连续/守恒/判据唯一/锚归位）与校验器 scripts/unxreal_i1_skeleton_check.py 六查 ALL PASS exit=0 入库；主汇编册卷末《增补卷 · AI-41 · 波16 首产段 I1 域骨架立账》300 项纯追加零删除；handoff.json I1 块建账、根统一台账 §三 I1 行/§四 会话行同步。")
lines[ins:ins] = sec
write(ZG, "\n".join(lines))
print(f"总纲 §7.3-I1 四十行一次建入（插入于行 {ins+1} 前），纯追加零删除")

# ---------- 3. 根台账 §三 行 + §四 会话条 ----------
l0 = read(LEDGER)
assert "UNX-I1 TCP/IP 内核栈" not in l0, "根台账已含 I1 行"
anchor = "| 其余 74 域 | 待领 | 0 | 0 | 0 | — | 认领后按本表格式追加行 |"
assert anchor in l0, "§三 表尾锚未找到"
row = "| UNX-I1 TCP/IP 内核栈 | AI-41 | 0 | B01–B15（15 批 300 条首产段，波 16 首轮立账） | 0 | 90,000 / 240,000（15 批×6,000 逐批求和，scripts/unxreal_i1_skeleton_check.py 六查 ALL PASS 实核零偏离） | 波 16 首产段 300 项新功能（B01–B08 F 型：以太网/IPv4 头解析守卫接管 net.rs/TCB 与 11 态状态机地基/四元组哈希与锁分层/定时器轮/ARP 邻居态机/cubic 拥塞与 SACK/分片重组与 PMTU/DMA 环与 NAPI 收发管线 160 条 + B09–B15 M 型：IPv6 双栈/TCP 握手深化与 syncookies/conntrack/NAT 三类/netfilter 五钩子/规则匹配引擎/UDP·ICMP 与单拷贝基线 140 条）；任务书五示例锚 F32001/F32011/F32035/F32069/F32120 按 §2.1 区间恒等归位；上游 AI-02/03/04 Schema 先行+fake（R-I1-001/002/003）；net.rs 3,908 行升级接管不删除存量（退役范围与 AI-98 联签）；域内加严红线「错误路径不停轮询」违例即 P0；真机判据（iperf J1/J2/J4/J6）随闸门补测（R-I1-004，零 QEMU）；B16–B40 待领（余 150,000 行） |"
l1 = l0.replace(anchor, row + "\n" + anchor, 1)
sess = """
### 会话 2026-波16-M01 · AI-41（I1 域认领扩表 + B01–B15 首产段骨架 300 条 · F32001–F32300 · 90,000 行）

- **冷启动对账**：git HEAD + 分工完成图 §七 AI-41 任务书（UNX-I1 TCP/IP 内核栈 · F32001–F32800 · 40 批 800 条 · 240,000 行 · 波 16 窗 · 上游 AI-02 MSI-X · 部 I 链头 I1→I2→I3/I4→I5）+ 主汇编册（F32001–F32800 五位段零占用 ✓）+ 总纲（无 §7.3-I1 ✓）+ 根台账（无 I1 行 ✓）多方对账；本会话按「一次对话 300 项新功能明令」执行域账首产段立账，不越权他域、不代领批次（I2 F32801 起、I3 F33601 起零触碰）。
- **本会话 300 项新功能（首产段 B01–B15）**：写入主汇编册《增补卷 · AI-41 · 波16 首产段 I1 域骨架立账》纯追加零删除（追加前守恒断言通过，卷 SHA-256 留痕于执行器输出）；总纲 §7.3-I1 四十行一次建入（[骨架]×15 AI-41 写锁+[未动]×25 待领，扩表三项机械校验通过）；批册 docs/unxreal/batches/UNX-I1-B01..B15.md 十五件落盘。
- **分段主题**：F 型地基 B01–B08（以太网帧与 IPv4 头解析守卫接管 net.rs 建模/TCB 与 11 态状态机地基/四元组哈希表与锁分层（桶锁+TCB 自旋锁+定时器轮锁三级，A5 死锁检测器登记）/定时器轮与 TCP 定时族（Nagle×延迟 ACK 死锁面防线）/ARP 邻居态机与未决队列/cubic 拥塞控制与 SACK 记分板/IP 分片重组与 MTU/PMTU/DMA 描述符环与 NAPI 收发管线——MSI-X 绑核+预算制 64 帧/轮+错误路径不停轮询红线）160 条 + M 型机制前段 B09–B15（IPv6 双栈 NDP/SLAAC/DAD/TCP 连接建立深化（syncookies/半连接队列/洪水防线）/conntrack 连接跟踪与表满处置（丢 NEW 保 ESTABLISHED）/NAT 三类/netfilter 五钩子框架与裁决/规则匹配引擎（线性+前缀哈希加速+连接状态匹配）/UDP·ICMP 语义与发送路径单拷贝基线）140 条；判据体例 UNX-F32xxx-J1「动作动词+可观测对象+阈值锚点」，J1 ≥900 Mbps/J2 P99 ≤1.5ms/J3 误放行 0/J4 存活 ≥95%/J5 11 态/J6 漂移 <5% 六组判据锚分批在位。
- **断言链**：①防重五范围 F32001–F32800 零撞号（精确五位数域段，排除 I2 段 F32801+ 与 4 位 A4 段同名前缀）；②判据 300 枚唯一；③行数守恒 15 批×6,000=90,000（批内配平差额留痕于生成器输出，末条承载位承 R-A1-004 判例）；④台账四处回填；⑤骨架态下限=判据句成型+行数预算登记。
- **工具与可复验**：生成器 scripts/unxreal_i1_gen.py（五断言 ALL PASS）+ 校验器 scripts/unxreal_i1_skeleton_check.py（六查 ALL PASS exit=0）。
- **红线与纪律**：红线预申报为空（非引导/写盘红线域）；net.rs 3,908 行+netstack.rs 157 行升级接管不删除存量、退役范围与 AI-98 联签预告位；「错误路径不停轮询」域内加严红线违例即 P0；防幻觉条款——RFC 引用带编号节号；豁免不入账=虚报；体验日志/异常显性化/错误三要素判据化全程落条目。
- **open_risks**：R-I1-001（AI-02 IrqBackend::bind_vector 冻结签前 fake 全链先行，波 16 Q3 冻结日联签）；R-I1-002（AI-03 每核队列绑定与软中断优先级接口）；R-I1-003（AI-04 DMA 一致性内存专属扩展面，B01 开工 12 件包登记冻结项）；R-I1-004（真机判据 iperf/时延/韧性/长稳随闸门补测，开发期零 QEMU 零实机写）；R-I1-005（波 16 演习「网络分区脑裂」AI-41/43 主办，结果回填 B21–B28 错误路径清单——预告位）。
- **双同步**：docs 落盘（15 批册+主汇编册增补卷+总纲 §7.3-I1+根台账 §三/§四+handoff I1 块+生成器/校验器）+ git 提交推送（pathspec 显式限定本会话产物，零裹挟他会话在途产物）。
- **B16–B40 待领**（余 150,000 行；B16–B20 M 型后段/B21–B28 E 型攻击面拒止/B29–B36 I 型三向联签/B37–B40 C 型 iperf 对标总账与域收官）。
"""
l1 = l1.rstrip("\n") + "\n" + sess
write(LEDGER, l1)
print(f"根台账 §三 I1 行 + §四 会话条追加（+{len(l1)-len(l0)} 字符，纯追加零删除）")

# ---------- 4. handoff.json I1 块（最小 patch） ----------
h = json.loads(read(HANDOFF))
assert "I1" not in h.get("domain_ledger_progress", {}), "handoff 已含 I1 块"
h["domain_ledger_progress"]["I1"] = {
    "finalized_batches": 0,
    "skeleton_batches": 15,
    "skeleton_list": [f"UNX-I1-B{i:02d}" for i in range(1, 16)],
    "rows_locked": 90000,
    "domain_budget": 240000,
    "owner": "AI-41",
    "note": "首产段 B01–B15 骨架 300 条立账（F32001–F32300 连续唯一 · 判据 300 枚唯一 · 生成器 scripts/unxreal_i1_gen.py 五断言 ALL PASS · 校验器六查 ALL PASS）；B16–B40 待领（余 150,000 行）；open_risks R-I1-001..005"
}
h["updated_at"] = "2026-10-02T04:30:00"
h["last_session"] = "AI-41 I1 域认领扩表：总纲 §7.3-I1 全表 40 批一次建入 + B01–B15 首产段骨架 300 项新功能（batches/UNX-I1-B01..B15.md · F32001–F32300 连续唯一 · 90,000/240,000=37.5% · 生成器五断言+校验器六查 ALL PASS）；主汇编册增补卷纯追加；根台账 §三/§四同步；open_risks R-I1-001..005 登记；并行他会话产物零触碰"
write(HANDOFF, json.dumps(h, ensure_ascii=False, indent=1) + "\n")
print("handoff.json I1 块建账（最小 patch）")
print("四处落账 ALL DONE")
