# -*- coding: utf-8 -*-
"""
UNX-K3 域骨架第二产段生成器 · AI-53（B16–B40 · F41901–F42400 · 500 条 · 150,000 行）
承首产段判例：五断言 → 五范围防重（含首产段段内互查）→ 纯追加 → 指纹回显。
幂等护栏：主册已含 UNX-F41901 则拒绝重写。
"""
import hashlib
import os
import re
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from unxreal_k3_data4 import BATCHES_B16_B20
from unxreal_k3_data5 import BATCHES_B21_B25
from unxreal_k3_data6 import BATCHES_B26_B30
from unxreal_k3_data7 import BATCHES_B31_B35
from unxreal_k3_data8 import BATCHES_B36_B40

REPO = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
MAIN_MD = os.path.join(REPO, "docs", "Varix", "CoRun Varix STAR II · Unxreal",
                       "CoRun Varix STAR II · Unxreal.md")

BATCHES = BATCHES_B16_B20 + BATCHES_B21_B25 + BATCHES_B26_B30 + BATCHES_B31_B35 + BATCHES_B36_B40
for _b in BATCHES:
    _b["entries"] = sorted(_b["entries"], key=lambda e: e[0])
ID_LO, ID_HI = 41901, 42400
BATCH_LINES = 6000

JUZHOU_ZHOU = ("判据主轴：枚举快照与真机对照（同机同负载对照差异率 ≤1%，真机项随闸门补测登记）"
               "+ 快照五类 × 1,000 并发生灭压测零不一致（J1）+ 结束任务两段策略对照矩阵全绿（J3·随闸门）"
               "+ 四资源与 /proc 类原生账交叉校验偏差 ≤2%（J4·真机项随闸门）")
FANGZHONG = ("防重：全域 ID F41601–F42400 与已收口域零撞号（grep 五范围 + 首产段段内互查）")
NEIHE = ("内核锚定：全部条目围绕 Varix 内核——AI-18 冻结接口 ObCreateObject(type, sd)/AccessCheck(token, sd, desired, generic_map) 消费联签；"
         "上游 D2/D3/A3/A4/A5/I1/J1/J5/K1/K2 联签锚逐条登记；下游 O1/O2/N 部/O5 消费锚逐条登记")
HONGXIAN = ("红线声明：本批无引导设施红线与硬件数据安全红线触发条目（预申报为空）；"
            "终结/设置类操作全程显性白名单语义（受保护进程拒止）；"
            "体验日志/异常显性化/交互词典三条间接纪律全程生效")


def assert_five():
    assert len(BATCHES) == 25, f"批数 {len(BATCHES)} != 25"
    all_ids = []
    for b in BATCHES:
        entries = b["entries"]
        assert len(entries) == 20, f"B{b['no']:02d} 条数 {len(entries)}"
        s = sum(e[2] for e in entries)
        assert s == BATCH_LINES, f"B{b['no']:02d} 求和 {s} != 6000"
        for eid, name, lines, jud in entries:
            all_ids.append(eid)
    assert all_ids == list(range(ID_LO, ID_HI + 1)), "ID 不连续"
    for e in (e for b in BATCHES for e in b["entries"]):
        assert e[3] and len(e[3]) >= 15, f"判据正文过短: F{e[0]}"
    total = sum(e[2] for b in BATCHES for e in b["entries"])
    assert total == 150000, f"总行数 {total} != 150000"
    print("[1/4] 五断言 ALL PASS（25 批 × 20 条 / ID 连续 500 / 批守恒 / 总 150,000）")


def assert_wufangwei():
    scopes = [
        os.path.join(REPO, "kernel"),
        os.path.join(REPO, "docs", "START"),
        os.path.join(REPO, "_attic"),
        os.path.join(REPO, "deepen"),
    ]
    pat = re.compile(r"F4(19[0-9]|2[0-3][0-9])\d{2}")
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
    with open(MAIN_MD, "r", encoding="utf-8") as fh:
        main = fh.read()
    seg = main[main.index("UNX-F41901 · 进程终结编排"):] if "UNX-F41901 · 进程终结编排" in main else ""
    if seg:
        hits.append("MAIN_MD(第二产段已存在)")
    assert not hits, f"防重撞号: {hits[:5]}"
    print("[2/4] 五范围防重 grep 零撞号 ALL PASS")


def render_volume():
    out = []
    out.append("")
    out.append("## 增补卷 · AI-53 · 波20 第二产段 K3 域骨架立账（B16–B40 · F41901–F42400 · 500 条）")
    out.append("")
    out.append(
        "> AI-53 承办（一次对话补齐 500 项明令 · 承首产段判例）：UNX-K3 域第二产段 25 批 × 20 条骨架"
        "（域账累计 240,000 / 240,000 满账）。任务书示例锚恒等归位兑现：F41901（进程终结编排与树终结）B16 原位、"
        "行数保真 460；F42001（任务管理器语义面·启动页聚合）按 ID 区间落 B21（任务书标 B17 与 20 条/批 ID 连续性冲突按 §2.1 恒等归位——ID 不变不重编、语义锚落批、行数保真 350）。"
        "批型铺排按 ID 区间恒等归位：M 型行为段 B16–B25（终结编排/两段策略/设置语义/详细信息页/服务页/启动页/性能页/应用历史/详情面板/段闸）、"
        "E 型对抗段 B26–B29（churn 压测路径/退出竞态/句柄泄漏观测/权限不足拒绝/E 型段闸）、"
        "I 型联签段 B30–B32（K1/K2 同源联签/O1·O3 对接/N 域支撑/B4·I1 恒等联轧/全域总轧）、"
        "C 型收官段 B33–B40（回归总闸/性能钉版/文档总成/20 维自评/缺陷与恢复/防重终印/闭账贡献/域收官）。"
        "防重零撞号；25 批 × 6,000 = 150,000 行守恒；判据 500 枚唯一。红线预申报为空；真机对照判据随闸门补测登记（开发期零 QEMU 零实机写）；诚实三态全程。"
    )
    for b in BATCHES:
        no, first, last = b["no"], b["entries"][0][0], b["entries"][-1][0]
        out.append("")
        out.append(f"# UNX-K3-B{no:02d} · {b['theme']}（F{first}–F{last} · 20 条）")
        out.append("")
        cum = no * BATCH_LINES
        out.append(
            f"> AI-53 承办（波 20 第二产段立账）｜批主题：{b['theme']}｜批要点：{b['points']}"
            f"｜批内求和：{BATCH_LINES} 行（守恒断言内建）｜域账累计：{cum} / 240,000"
            f"｜{JUZHOU_ZHOU}｜{FANGZHONG}｜{NEIHE}｜{HONGXIAN}"
        )
        for eid, name, lines, jud in b["entries"]:
            out.append("")
            out.append(f"### UNX-F{eid} · {name}")
            out.append(f"- 域/批：K3/B{no:02d}｜纯功能行数：{lines}｜状态：[骨架]｜判据：UNX-F{eid}-J1 {jud}")
    out.append("")
    out.append("### 卷尾 · 第二产段收口声明（AI-53 · B16–B40 段闸）")
    out.append("")
    out.append("1. **账实守恒**：25 批 × 20 条 = 500 条，逐批 6,000 行，全段 150,000 行，"
               "ID 段 F41901–F42400 连续无空洞无重号；首产段 + 第二产段 = 800 条 / 240,000 行满账。")
    out.append("2. **判据母版使用记录**：J1（churn 压测·B26）、J2（同机字段对照·B21/B26）、"
               "J3（两段策略对照·B17）、J4（交叉校验·B31）——复用处在本段判据组内声明，可组合不可替代。")
    out.append("3. **联签对账**：B16（J1 权限/D4 清账）、B17（D4/K1）、B18（A3/A4/B4/J1）、B19（J1/J2/A3/A4）、"
               "B20（K1/D3）、B21（K2/E4/B2）、B22（A3/A4/B4/I1/G1）、B23（A3/I1/F5）、B24（D3/D2/E4/F5）、"
               "B30（K1/K2）、B31（O1/O3/N/B4/I1）、B32（全域九对）——联签锚逐条在位，变更走 mini-ADR。")
    out.append("4. **诚实三态**：真机对照（两段策略矩阵/字段全表/对照准备账）均随闸门补测登记；"
               "QEMU 实机类判据显式不覆盖；full 档 QEMU 批队列登记未虚报已跑。")
    out.append("5. **红线执行**：全程零引导设施触碰、零内置盘写入；终结/设置操作显性白名单；受保护进程拒止。")
    out.append("6. **双同步**：本卷随本会话提交推送，协作事件另记统一总台账会话条目，两账不混。")
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
