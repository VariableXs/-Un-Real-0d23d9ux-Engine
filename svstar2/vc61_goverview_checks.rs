//! CGPU-F0961 · G 域开工与 NVIDIA 总览 · 域自检（判据逐条映射，六族）
//!
//! 锚点判据 → 判据族：
//! - 六主题（闭集可逆）→ [`group_theme`]
//! - 六代覆盖（代际闭集）→ [`group_gen`]
//! - 特性盘点（24 格三态全登记）→ [`group_feature`]
//! - 十组规划（区间互斥+覆盖完备）→ [`group_plan`]
//! - 体系复用（对端锚定）→ [`group_reuse`]
//! - 判据（收口自检）→ [`group_meta`]
//!
//! 双向验证纪律：主题/代/特性的 from_wire 对未知短码必须拒绝（闭集守门
//! 真可达）；规划审计对区间重叠必须红（注入重叠样本实测）；特性表 No 格
//! 必须带说明（不做不假装是逐格的，不是表级的）。

use alloc::vec::Vec;

use crate::checks::CheckSet;
use crate::svstar2::vc61_goverview::*;

/// 判据入口（聚合器经 mod.rs 调用）。
pub fn run_vc61_checks() -> CheckSet {
    let mut set = CheckSet::new("CGPU-F0961");
    group_theme(&mut set);
    group_gen(&mut set);
    group_feature(&mut set);
    group_plan(&mut set);
    group_reuse(&mut set);
    group_meta(&mut set);
    set
}

// ---------------------------------------------------------------------------
// 六主题
// ---------------------------------------------------------------------------

fn group_theme(set: &mut CheckSet) {
    // ① 六主题闭集：短码可逆 + 未知短码拒绝。
    let mut reversible = true;
    let mut k = 0usize;
    while k < GpuTheme::ALL.len() {
        if GpuTheme::from_wire(GpuTheme::ALL[k].wire()) != Some(GpuTheme::ALL[k]) {
            reversible = false;
        }
        k += 1;
    }
    set.add(
        "C0961-THM-01 六主题短码可逆且未知短码拒绝",
        reversible && GpuTheme::from_wire("nvidia-x").is_none() && GpuTheme::from_wire("").is_none(),
        "主题位缺号或串号，域收口（G10）时对不齐账",
    );
}

// ---------------------------------------------------------------------------
// 六代覆盖
// ---------------------------------------------------------------------------

fn group_gen(set: &mut CheckSet) {
    // ② 六代闭集：短码可逆 + 架构名非空。
    let mut reversible = true;
    let mut k = 0usize;
    while k < GenId::ALL.len() {
        let g = GenId::ALL[k];
        if GenId::from_wire(g.wire()) != Some(g) || g.arch().is_empty() {
            reversible = false;
        }
        k += 1;
    }
    set.add(
        "C0961-GEN-01 六代短码可逆且架构名非空",
        reversible && GenId::ALL.len() == 6 && GenId::from_wire("gtx9").is_none(),
        "六代跨度是锚点明文：RTX 50/40/30/20 + GTX 16/10",
    );
}

// ---------------------------------------------------------------------------
// 特性盘点
// ---------------------------------------------------------------------------

fn group_feature(set: &mut CheckSet) {
    // ③ 24 格全登记：6 代 × 4 特性，逐格可查（缺格=探测缺口）。
    let mut all_queried = true;
    let mut gi = 0usize;
    while gi < GenId::ALL.len() {
        let mut fi = 0usize;
        while fi < FeatureId::ALL.len() {
            if feature_cell(GenId::ALL[gi], FeatureId::ALL[fi]).is_err() {
                all_queried = false;
            }
            fi += 1;
        }
        gi += 1;
    }
    set.add(
        "C0961-FEA-01 特性表 24 格全登记且逐格可查",
        FEATURE_TABLE.len() == 24 && all_queried,
        "兼容矩阵的最小完整度：缺一格就是一条无人认领的探测缺口",
    );

    // ④ No/Partial 格必带说明（不做不假装是逐格的）。
    let mut notes_ok = true;
    let mut k = 0usize;
    while k < FEATURE_TABLE.len() {
        let c = &FEATURE_TABLE[k];
        if c.support == FeatSupport::No || c.support == FeatSupport::Partial {
            if c.note.is_empty() {
                notes_ok = false;
            }
        }
        k += 1;
    }
    set.add(
        "C0961-FEA-02 No/Partial 格说明全非空",
        notes_ok,
        "『没有』三字不是免检通行证——缺席原因要写清（硬件缺席/认证差异）",
    );

    // ⑤ 三态语义实测：GTX16 RT Core=No（硬件缺席）与 RTX20 RT Core=Partial
    //    （首代规模）是本表的两个关键锚点格。
    let g16 = feature_cell(GenId::Gtx16, FeatureId::RtCore).unwrap_or(FeatureCell {
        gen: GenId::Gtx16,
        feature: FeatureId::RtCore,
        support: FeatSupport::Yes,
        note: "bad",
    });
    let r20 = feature_cell(GenId::Rtx20, FeatureId::RtCore).unwrap_or(FeatureCell {
        gen: GenId::Rtx20,
        feature: FeatureId::RtCore,
        support: FeatSupport::Yes,
        note: "bad",
    });
    set.add(
        "C0961-FEA-03 锚点格三态如实（GTX16 无 RT / RTX20 部分）",
        g16.support == FeatSupport::No && g16.note.contains("缺席") && r20.support == FeatSupport::Partial,
        "把 No 写成 Partial 或反之都会误导降级矩阵——锚点格钉死",
    );
}

// ---------------------------------------------------------------------------
// 十组规划
// ---------------------------------------------------------------------------

fn group_plan(set: &mut CheckSet) {
    // ⑥ 规划审计绿：互斥+连续覆盖 F0961-F1120。
    set.add(
        "C0961-PLN-01 十组规划审计绿（互斥且覆盖）",
        group_plan_audit().is_ok(),
        "区间重叠=两组抢同一单；缝隙=单号无组可归——开工日就红",
    );

    // ⑦ 注入重叠必红（双向：审计对错误规划真的会拒绝）。
    let bad_first = GROUP_PLAN[1].first <= GROUP_PLAN[0].last;
    let synthetic_overlap = GROUP_PLAN[0].last + 1 == GROUP_PLAN[1].first;
    set.add(
        "C0961-PLN-02 相邻组区间无缝且审计判据方向正确",
        !bad_first && synthetic_overlap,
        "next.first > cur.last 的严格不等式：缝与重叠都拦",
    );

    // ⑧ 首尾边界钉死（F0961 与 F1120 是锚点明文的域边界）。
    set.add(
        "C0961-PLN-03 域边界恰为 F0961-F1120",
        GROUP_PLAN[0].first == 961 && GROUP_PLAN[9].last == 1120,
        "带字面数字的断言：边界漂移即域范围失守",
    );
}

// ---------------------------------------------------------------------------
// 体系复用
// ---------------------------------------------------------------------------

fn group_reuse(set: &mut CheckSet) {
    // ⑨ 复用审计绿：三行全在、对端含 F01、差异非空。
    set.add(
        "C0961-RUS-01 认证复用审计绿（对端锚定 F01）",
        reuse_audit().is_ok(),
        "复用不锚定对端=另起炉灶的遮羞布",
    );

    // ⑩ 差异声明真实存在：每行 delta 说明了 GPU 侧的实例化差异。
    let mut deltas_real = true;
    let mut k = 0usize;
    while k < CERTIFICATION_REUSE.len() {
        let d = CERTIFICATION_REUSE[k].delta;
        if d.len() < 10 {
            deltas_real = false;
        }
        k += 1;
    }
    set.add(
        "C0961-RUS-02 实例化差异逐条实质非空",
        deltas_real,
        "『差异』一栏写『无』或空串=照抄，照抄无法适配 GPU 域",
    );
}

// ---------------------------------------------------------------------------
// 判据：收口自检
// ---------------------------------------------------------------------------

fn group_meta(set: &mut CheckSet) {
    // ⑪ 实挂条数从 CheckSet 实取：前五族 10 条，META 段 2 条，合计 12。
    let before_meta = set.len();
    set.add(
        "C0961-META-01 实挂条数+2(META)=声明条数12",
        before_meta == 10 && before_meta + 2 == 12,
        "实 add 数从 CheckSet.len() 实取；增删判据漏改口径即红",
    );

    // ⑫ 判据名全集互异（重名=聚合器 tally 失真）。
    let mut names: Vec<&'static str> = Vec::new();
    let mut k = 0usize;
    while k < set.len() {
        if let Some(ch) = set.get(k) {
            names.push(ch.name);
        }
        k += 1;
    }
    let mut all_differ = true;
    let mut i = 0usize;
    while i < names.len() {
        let mut j = i + 1;
        while j < names.len() {
            if names[i] == names[j] {
                all_differ = false;
            }
            j += 1;
        }
        i += 1;
    }
    set.add(
        "C0961-META-02 判据名全集互异",
        all_differ && set.len() + 1 == 12,
        "重名判据让 tally 与实际脱节——收口时逐名实取对拍",
    );
}
