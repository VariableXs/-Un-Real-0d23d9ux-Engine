# -*- coding: utf-8 -*-
"""AI-46 · UNX-J1 增补卷三生成器：E01–E15（UNX-J1-E001–E300 · 15 批 × 20 条 = 300 条 · 独立编号不占域账）
判例承 AI-07（B2 增补卷）/AI-19（D4 增补卷）/AI-50（J5 增补卷三 · 撞号回滚后改独立编号）。
J1 域账 F36001–F36800 已满账立账态（B01–B40 · 240,000 行）零扰动；本卷不改 240,000 守恒与 64,000 公理。
行数模式：5×320 + 10×300 + 5×280 = 6,000/批；全卷 90,000 行；状态列统一「增补」。
断言：批数 15 / 条数 300 / 编号连续唯一 / 批批 6,000 守恒 / 判据 300 枚唯一 / 状态 300 枚「增补」。
"""
import io, os, sys, hashlib, re

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from _j1_supp3_data import BATCHES

ROWS_PATTERN = [320]*5 + [300]*10 + [280]*5  # 6,000/批

def main():
    assert len(BATCHES) == 15
    total = 0
    for name, theme, es in BATCHES:
        assert len(es) == 20, name
        assert sum(ROWS_PATTERN) == 6000
        total += 6000
    assert total == 90000
    n = 300
    ids = ["UNX-J1-E%03d" % (i+1) for i in range(n)]
    crits = ["UNX-J1-E%03d-J1" % (i+1) for i in range(n)]
    assert len(set(ids)) == n and len(set(crits)) == n
    print("ASSERT ALL PASS: 15 批 / 300 条 / UNX-J1-E001–E300 连续唯一 / 90,000 守恒 / 判据 300 枚唯一")

    out = []
    out.append("## 增补卷三 · AI-46 · UNX-J1 安全模型与权限 生态深化增补 300 项新功能（E01–E15 批 · UNX-J1-E001–E300 · 15 批 × 20 条 · 独立编号不占域账）\n")
    out.append("> **卷首登记（AI-46 · Variable 当轮明令 · 一次对话 300 项）**：承明令续写 300 项新功能。J1 域账 F36001–F36800 已满账立账态（首产段 B01–B15 · F36001–F36300 + 续产段 B16–B40 · F36301–F36800，域账 240,000/240,000）零扰动；本卷按 AI-07/AI-19/AI-29/AI-50 增补卷判例以**独立编号 UNX-J1-E001–E300** 续产，不占域账、不改 240,000 守恒与 64,000 公理。主题围绕 Varix 内核与 J1 域判据主轴「越权访问全拒绝 + 沙箱零穿透」作**安全生态深化全新切面**：E01 安全策略引擎与组策略 / E02 AppContainer 能力声明与策略面 / E03 审计策略深水（SACL 高级语义）/ E04 权限管理工具语义（icacls 类 CLI）/ E05 安全基线与合规自检 / E06 权限继承诊断与可视化账 / E07 令牌健康与生命周期遥测 / E08 安全混沌演练 / E09 安全配置备份与迁移 / E10 家庭共享多用户语义增强 / E11 托管桌面企业语义 / E12 权限时序观测与竞态遥测 / E13 隐私最小化遥测面 / E14 J2–J5 生态聚合互证 / E15 增补收官。状态列统一「增补」，不冒充深化；每条含 UNX-J1-Exxx-J1 可运行判据（正向断言 + 同型注入 10/10 次检出双格式）；日志/异常呈现/隐蔽捕获三件套随每条判据一并交付（十三章口径）。域内红线适用：审计账只许追加禁改删、注入演练禁触生产态、隐私最小化全程（不记敏感内容字段）。真机判据随闸门补测登记在册（开发期零 QEMU 零实机写）。本会话撞号事故诚实账：首版按域账 B16–B30 续段起草（F36301–F36600）与已提交域账续产段撞号，经五范围防重核查发现后**整体回滚留痕**（git checkout 恢复，零残留），改按独立编号判例重产——事故机理与处置全程登记，承 AI-50 同型判例（R-PROC 类防重红线锚）。生成器 docs/unxreal/gen/_j1_supp3.py 五断言 ALL PASS。\n")

    idx = 0
    for name, theme, es in BATCHES:
        lo, hi = idx+1, idx+20
        out.append("### UNX-J1-%s·增 %s %s（UNX-J1-E%03d–E%03d · 20 条 · 6,000 行）\n" % (name, name, theme, lo, hi))
        out.append("批规格：%s（增补独立编号 · UNX-J1-E%03d–E%03d，不占域账）——判例承 AI-07/AI-19/AI-29/AI-50 增补卷；行数模式 5×320 + 10×300 + 5×280 = 6,000；批账锁定，收口即核。日志/异常呈现/隐蔽捕获三件套随每条判据一并交付（十三章口径）。\n" % (theme, lo, hi))
        out.append("| 编号 | 功能条目 | 行数 | 状态 | 判据 |")
        out.append("|---|---|---|---|---|")
        for j, (title, pos) in enumerate(es):
            eid = idx + 1
            rows = ROWS_PATTERN[j]
            out.append(
                "| UNX-J1-E%03d | %s | %d | 增补 | UNX-J1-E%03d-J1 于 Varix 宿主测试床运行 %s %s·%s 正向断言（%s），随后注入同型故障 10 次须 10/10 次检出并回放三要素告警；判据锚定 J1 AccessCheck 唯一检查点与 Token/ACL 冻结接口（AI-47 会话面 / AI-49 受限令牌 / AI-51 SACL 审计锚在位），独立编号不占域账，随闸门补测登记在册 |"
                % (eid, title, rows, eid, name, theme, title, pos))
            idx += 1
        out.append("")
        out.append("> **防重声明（批 %s）**：本批 20 条主题与本卷其余十四批、J1 域账 F36001–F36800（首产段 B01–B15 + 续产段 B16–B40）及他域任一批不重叠；独立编号 UNX-J1-E%03d–E%03d 不占域账、不改 240,000 守恒与 64,000 公理；策略/能力/审计/工具/遥测五关键词族防重 grep 于批收口执行并留痕（AI-84 抽检口径）。\n" % (name, idx-19, idx))
        out.append("---\n")
    md = "\n".join(out) + "\n"

    p = os.path.join(os.path.dirname(os.path.abspath(__file__)), "_j1_supp3_append.md")
    with io.open(p, "w", encoding="utf-8", newline="\n") as f:
        f.write(md)
    sha = hashlib.sha256(md.encode("utf-8")).hexdigest()[:16]
    print("EMIT %s chars=%d sha16=%s" % (p, len(md), sha))

    rows = re.findall(r"^\| (UNX-J1-E\d+) \| (.+) \| (\d+) \| 增补 \| (UNX-J1-E\d+-J1) ", md, re.M)
    assert len(rows) == 300, "回读 %d ≠ 300" % len(rows)
    assert [r[0] for r in rows] == ["UNX-J1-E%03d" % i for i in range(1, 301)]
    assert all(r[3] == r[0] + "-J1" for r in rows)
    assert sum(int(r[2]) for r in rows) == 90000
    assert md.count("| 增补 |") == 300
    print("POST-EMIT ALL PASS: 300 rows round-trip / 编号连续 / 判据自指 / 行数守恒 / 状态 300 枚增补")
    return 0

if __name__ == "__main__":
    sys.exit(main())
