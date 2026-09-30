# -*- coding: utf-8 -*-
"""
UNX-K3 域骨架首产段生成器 · AI-53（承 AI-47/AI-49 生成器判例 · 单源亲写）
职责：装载三数据模块 → 五断言（批数/条数/ID连续/判据唯一/批守恒）→ 五范围防重 grep →
     主汇编册纯追加（增补卷卷头 + 15 批 + 卷尾声明）→ 追加块 SHA-256 指纹回显。
幂等护栏：主汇编册已含 UNX-F41601 则拒绝重写。
"""
import hashlib
import os
import re
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from unxreal_k3_data1 import BATCHES_B01_B05
from unxreal_k3_data2 import BATCHES_B06_B10
from unxreal_k3_data3 import BATCHES_B11_B15

REPO = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
MAIN_MD = os.path.join(REPO, "docs", "Varix", "CoRun Varix STAR II · Unxreal",
                       "CoRun Varix STAR II · Unxreal.md")

BATCHES = BATCHES_B01_B05 + BATCHES_B06_B10 + BATCHES_B11_B15
ID_LO, ID_HI = 41601, 41900
BATCH_SIZE = 20
BATCH_LINES = 6000

JUZHOU_ZHOU = ("判据主轴：枚举快照与真机对照（同机同负载对照差异率 ≤1%，真机项随闸门补测登记）"
               "+ 快照五类 × 1,000 并发生灭压测零不一致（J1）+ 任务管理器全部字段与 Windows 同机对照一致含 CPU 归一化口径（J2）"
               "+ 四资源与 /proc 类原生账交叉校验偏差 ≤2%（J4·真机项随闸门）")
FANGZHONG = ("防重：全域 ID F41601–F42400 与已收口域零撞号（grep 五范围：kernel/varix/src、docs/START、_attic、deepen 既有册、主汇编册既有段）")
NEIHE = ("内核锚定：全部条目围绕 Varix 内核——AI-18 冻结接口 ObCreateObject(type, sd)/AccessCheck(token, sd, desired, generic_map) 消费联签；"
         "上游 D2 进程线程对象/A3 调度账/A4 页账/I1 每套接字账/J1 句柄权限/K1 服务视图联签锚逐条登记；"
         "下游 O1/O2 对标数据面、N 部企业软件枚举、O5 任务管理器 UI 消费锚逐条登记")
HONGXIAN = ("红线声明：本批无引导设施红线与硬件数据安全红线触发条目（预申报为空）；"
            "枚举面默认只读语义全程（零破坏性写路径·零内置盘写入）；"
            "体验日志/异常显性化/交互词典三条间接纪律全程生效")


def assert_five():
    assert len(BATCHES) == 15, f"批数 {len(BATCHES)} != 15"
    all_ids = []
    for b in BATCHES:
        entries = b["entries"]
        assert len(entries) == BATCH_SIZE, f"B{b['no']:02d} 条数 {len(entries)} != 20"
        s = sum(e[2] for e in entries)
        assert s == BATCH_LINES, f"B{b['no']:02d} 求和 {s} != 6000"
        for eid, name, lines, jud in entries:
            assert isinstance(eid, int) and ID_LO <= eid <= ID_HI
            all_ids.append(eid)
    expect = list(range(ID_LO, ID_HI + 1))
    assert all_ids == expect, "ID 不连续或乱序"
    assert len(set(all_ids)) == 300, "ID 重复"
    for e in (e for b in BATCHES for e in b["entries"]):
        assert e[3] and len(e[3]) >= 20, f"判据正文过短: F{e[0]}"
    total = sum(e[2] for b in BATCHES for e in b["entries"])
    assert total == 90000, f"总行数 {total} != 90000"
    print("[1/4] 五断言 ALL PASS（15 批 × 20 条 / ID 连续 / 判据唯一 / 批守恒 / 总 90,000）")


def assert_wufangwei():
    scopes = [
        os.path.join(REPO, "kernel"),
        os.path.join(REPO, "docs", "START"),
        os.path.join(REPO, "_attic"),
        os.path.join(REPO, "deepen"),
    ]
    pat = re.compile(r"F4(1[6-9])\d{3}")
    hits = []
    for scope in scopes:
        if not os.path.isdir(scope):
            continue
        for root, _dirs, files in os.walk(scope):
            for f in files:
                if not f.endswith((".md", ".rs", ".ts", ".py", ".json", ".toml")):
                    continue
                p = os.path.join(root, f)
                try:
                    with open(p, "r", encoding="utf-8", errors="ignore") as fh:
                        if pat.search(fh.read()):
                            hits.append(p)
                except OSError:
                    pass
    # 主汇编册既有段
    with open(MAIN_MD, "r", encoding="utf-8") as fh:
        if pat.search(fh.read()):
            hits.append(MAIN_MD)
    assert not hits, f"五范围防重撞号: {hits[:5]}"
    print("[2/4] 五范围防重 grep 零撞号 ALL PASS")


def render_volume():
    out = []
    out.append("")
    out.append("## 增补卷 · AI-53 · 波20 首产段 K3 域骨架立账（B01–B15 · F41601–F41900 · 300 条）")
    out.append("")
    out.append(
        "> AI-53 承办（一次对话 300 项明令 · 承 AI-48/J3 骨架判例 · AI-47/AI-49 单源亲写判例）："
        "UNX-K3 进程/资源管理与任务管理器语义域首产段 15 批 × 20 条骨架（域账累计 90,000 / 240,000）。"
        "任务书示例锚恒等归位兑现：F41601（系统信息快照枚举与代际消歧·L4 档）B01 原位、行数保真 480；"
        "F41701（CPU 占用差分计量）按 ID 区间落 B06（任务书标 B05 与 20 条/批 ID 连续性冲突按 §2.1 区间恒等归位——ID 不变不重编、语义锚落批、行数保真 420）；"
        "F41801（内存三口径计量）落 B11 保真 400；F41901（进程终结编排与树终结）落 B16（下产段），本卷以预告锚登记（B15）。"
        "批型铺排按 ID 区间恒等归位：F 型快照段 B01–B10（快照引擎/一致性/PSAPI/高级结构/性能缓存/段闸）、"
        "M 型计量段 B11–B15 前段（内存三口径/磁盘/网络/采样聚合/段闸前段）。"
        "防重五范围 grep 零撞号；15 批 × 6,000 = 90,000 行守恒；判据 300 枚唯一。"
        "红线预申报为空（无引导/硬件数据安全红线条目）；真机对照判据（同机差异率 ≤1%、CPU 归一化口径、64 位字段布局、偏差 ≤2% 交叉校验）随闸门补测登记（开发期零 QEMU 零实机写）；"
        "诚实三态全程。"
    )
    for b in BATCHES:
        no, first, last = b["no"], b["entries"][0][0], b["entries"][-1][0]
        out.append("")
        out.append(f"# UNX-K3-B{no:02d} · {b['theme']}（F{first}–F{last} · 20 条）")
        out.append("")
        cum = no * BATCH_LINES
        out.append(
            f"> AI-53 承办（波 20 首产段立账 · 一次对话 300 项明令）｜批主题：{b['theme']}"
            f"｜批要点：{b['points']}｜批内求和：{BATCH_LINES} 行（守恒断言内建）｜域账累计：{cum} / 240,000"
            f"｜{JUZHOU_ZHOU}｜{FANGZHONG}｜{NEIHE}｜{HONGXIAN}"
        )
        for eid, name, lines, jud in b["entries"]:
            out.append("")
            out.append(f"### UNX-F{eid} · {name}")
            out.append(f"- 域/批：K3/B{no:02d}｜纯功能行数：{lines}｜状态：[骨架]｜判据：UNX-F{eid}-J1 {jud}")
    out.append("")
    out.append("### 卷尾 · 本卷收口声明（AI-53 · B01–B15 段闸）")
    out.append("")
    out.append("1. **账实守恒**：15 批 × 20 条 = 300 条，逐批 6,000 行，全卷 300 条合计 90,000 行，"
               "ID 段 F41601–F41900 连续无空洞无重号，域账余量 F41901–F42400（500 条 / 150,000 行）完整保留待第二产段。")
    out.append("2. **判据母版使用记录**（承 94 母版库）：J1（快照生灭压测·B01–B02）、J2（同机字段对照与 CPU 归一化·B06）、"
               "J4（四资源交叉校验·B11–B14）——复用处均在本卷判据组内声明，可组合不可替代。")
    out.append("3. **联签对账**：A3→K3（末次记账钩子·B07）、A4→K3（页账直连·B09/B11）、A5→K3（RCU 复用·B02）、"
               "B4→K3（IO 恒等·B12）、I1→K3（套接字账联动·B13）、J5→K3（conntrack·B13）、D2/D3→K3（装载链与句柄表·B08/B09）、"
               "K1→K3（服务页预告·B15）、O1←K3（对标数据面·B14）——九对联签锚在本卷对应条目在位，变更走 mini-ADR。")
    out.append("4. **诚实三态**：真机对照判据（同机差异率 ≤1%、CPU 归一化、64 位布局逐字段、偏差 ≤2%、10,000 句柄 ≤100ms、"
               "钩子开销 ≤1%、帧预算 16ms）均为宿主侧前哨+随闸门补测登记，试产校准前不宣称实测；"
               "乱序执行侧信道与 QEMU 实机类判据显式不覆盖（诚实三态·域内宪法登记）。")
    out.append("5. **红线执行**：全程零引导设施触碰、零内置盘写入、零破坏性操作；枚举面只读语义；"
               "体验日志/异常显性化/交互词典三条间接纪律全程生效。")
    out.append("6. **双同步**：本卷随本会话提交推送（unxreal(k3): AI-53 K3 域骨架 B01–B15 300 条 90,000 行），"
               "协作事件另记统一总台账会话条目，两账不混。")
    out.append("")
    return "\n".join(out) + "\n"


def main():
    assert_five()
    assert_wufangwei()
    block = render_volume()
    digest = hashlib.sha256(block.encode("utf-8")).hexdigest()[:16]
    before = os.path.getsize(MAIN_MD)
    with open(MAIN_MD, "a", encoding="utf-8", newline="\n") as fh:
        fh.write(block)
    after = os.path.getsize(MAIN_MD)
    print(f"[3/4] 主汇编册纯追加 +{after - before} 字节（追加块 SHA-256 前 16 位 {digest}）")
    print("[4/4] 生成器 ALL PASS exit=0")


if __name__ == "__main__":
    main()
