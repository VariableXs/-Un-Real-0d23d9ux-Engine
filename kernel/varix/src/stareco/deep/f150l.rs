//! 深化层六 · F150 生态域总判据（2026-09-27 深化批次六 · f150l 交接与持续深化面）。
//!
//! 收口冲刺件二：低达成项排序器（g 层快照 → 下阶段深化优先级）、
//! 修理工单生成（域内缺陷账 → 结构化工单）、域术语词典（一名一义
//! 复核）、90% 线闸门复证（跨线判定 + 收口措辞）。

use super::f150i::DOMAIN_SNAPSHOT;
use crate::checks::CheckSet;

// ---------------------------------------------------------------------------
// 低达成项排序器：快照 → 达成率最低的 n 项（下阶段深化优先级）
// ---------------------------------------------------------------------------

pub struct LaggardRow {
    pub item: &'static str,
    pub per_mille: u32,
}

pub fn laggard_ranking(n: usize) -> alloc::vec::Vec<LaggardRow> {
    let mut rows: alloc::vec::Vec<(u32, &'static str)> = DOMAIN_SNAPSHOT
        .iter()
        .map(|(name, actual, target)| ((actual * 1000 / target) as u32, *name))
        .collect();
    // 升序（最低在前），平局编号序。
    for i in 1..rows.len() {
        let k = rows[i];
        let mut j = i;
        while j > 0 && rows[j - 1] > k {
            rows[j] = rows[j - 1];
            j -= 1;
        }
        rows[j] = k;
    }
    rows.into_iter()
        .take(n)
        .map(|(pm, item)| LaggardRow { item, per_mille: pm })
        .collect()
}

// ---------------------------------------------------------------------------
// 修理工单生成：域内缺陷 → 结构化工单（账本字段齐备性门）
// ---------------------------------------------------------------------------

pub struct DefectTicket {
    pub id: &'static str,
    pub location: &'static str,
    pub severity: u8, // 0=🔴 1=🟡 2=🟢
    pub repro: &'static str,
}

/// 工单齐备门：🔴/🟡 必须带复现路径；🟢 可记"攒批"。
/// 分级越界（>2）拒绝——分级只有三档。
pub fn ticket_complete(t: &DefectTicket) -> Result<(), &'static str> {
    if t.severity > 2 {
        return Err("分级越界：只有 🔴/🟡/🟢 三档");
    }
    if t.severity <= 1 && t.repro.trim().is_empty() {
        return Err("🔴/🟡 工单必须带复现路径");
    }
    if t.location.trim().is_empty() || t.id.trim().is_empty() {
        return Err("定位与编号必填");
    }
    Ok(())
}

/// 批量出单：逐张校验，不合格的点名（结构化工单发修理窗口的数据面）。
pub fn batch_tickets(tickets: &[DefectTicket]) -> (alloc::vec::Vec<&'static str>, alloc::vec::Vec<&'static str>) {
    let mut ok: alloc::vec::Vec<&'static str> = alloc::vec::Vec::new();
    let mut rejected: alloc::vec::Vec<&'static str> = alloc::vec::Vec::new();
    for t in tickets {
        match ticket_complete(t) {
            Ok(()) => ok.push(t.id),
            Err(_) => rejected.push(t.id),
        }
    }
    (ok, rejected)
}

// ---------------------------------------------------------------------------
// 域术语词典：一名一义（f149j 口径的域级复核）
// ---------------------------------------------------------------------------

/// 生态域高频术语——两处定义漂移即用户困惑源。
pub const GLOSSARY: [(&'static str, &'static str); 6] = [
    ("星卡", "应用兼容性自动生成的信息卡（F036/F037）"),
    ("判例", "兼容域账本中逐应用实测记录（F040）"),
    ("闸门", "开发期暂缓、收口期统一补测的判据集合"),
    ("随闸门补测", "组织/环境类判据的登记待办形态"),
    ("收口", "工作包通过轻门禁并归档证据的完成态"),
    ("回炉", "低于验收线的交付整批退回补深化的动作"),
];

/// 词典完整性：定义非空 + 无重名词。
pub fn glossary_ok(defs: &[(&'static str, &'static str)]) -> bool {
    for (i, (n, d)) in defs.iter().enumerate() {
        if d.trim().is_empty() {
            return false;
        }
        if defs[i + 1..].iter().any(|(n2, _)| n2 == n) {
            return false;
        }
    }
    !defs.is_empty()
}

// ---------------------------------------------------------------------------
// 90% 线闸门复证：跨线判定 + 收口措辞（诚实两态）
// ---------------------------------------------------------------------------

pub fn cross_verdict(wc_total: usize, line: usize) -> (&'static str, bool) {
    if wc_total >= line {
        ("已跨 90% 线：转入闸门补测与收尾冲刺", true)
    } else {
        ("未跨线：继续深化（缺口如实登记）", false)
    }
}

// ---------------------------------------------------------------------------
// 自检
// ---------------------------------------------------------------------------

pub const F150L_TAG: &str = "stareco-F150-deep6b";

pub fn run_f150_deep6b_checks() -> CheckSet {
    let mut set = CheckSet::new(F150L_TAG);

    // 低达成排序
    let rank = laggard_ranking(3);
    set.add("f150l rank 3", rank.len() == 3, "取最低三项");
    set.add(
        "f150l rank order",
        rank[0].per_mille <= rank[1].per_mille && rank[1].per_mille <= rank[2].per_mille,
        "升序",
    );
    // F133 目标仅 195 行、达成率常年在前列——最低三项不应含 F133。
    set.add("f133 not laggard", !rank.iter().any(|r| r.item == "F133"), "超配项不误入欠账单");

    // 工单门
    let good = DefectTicket { id: "J1", location: "f135g.rs:88", severity: 1, repro: "cargo test f135g" };
    let green = DefectTicket { id: "J2", location: "f135h.rs:20", severity: 2, repro: "" };
    let bad = DefectTicket { id: "J3", location: "f135i.rs", severity: 1, repro: "" };
    let oob = DefectTicket { id: "J4", location: "x", severity: 3, repro: "r" };
    set.add("f150l ticket yellow", ticket_complete(&good).is_ok(), "🟡 带复现放行");
    set.add("f150l ticket green", ticket_complete(&green).is_ok(), "🟢 攒批合法");
    set.add("f150l ticket no repro", ticket_complete(&bad).is_err(), "🟡 无复现拒绝");
    set.add("f150l ticket oob", ticket_complete(&oob).is_err(), "分级越界拒绝");
    let (ok, rej) = batch_tickets(&[good, green, bad]);
    set.add(
        "f150l batch",
        ok == alloc::vec!["J1", "J2"] && rej == alloc::vec!["J3"],
        "批量出单分列",
    );

    // 词典
    set.add("f150l glossary", glossary_ok(&GLOSSARY), "六术语一名一义");
    set.add(
        "f150l glossary dup",
        !glossary_ok(&[("闸门", "a"), ("闸门", "b")]),
        "重名拒",
    );
    set.add(
        "f150l glossary empty",
        !glossary_ok(&[("空", " ")]),
        "空定义拒",
    );

    // 闸门复证（f150j 口径：WC 30,969 / 线 31,239——批次六收口后跨线）
    let (msg, crossed) = cross_verdict(31700, 31239);
    let (msg2, crossed2) = cross_verdict(30969, 31239);
    set.add(
        "f150l cross",
        crossed && msg.contains("闸门补测"),
        "跨线收口措辞",
    );
    set.add(
        "f150l not crossed honest",
        !crossed2 && msg2.contains("继续深化"),
        "未跨线如实措辞",
    );

    set
}

#[cfg(test)]
mod deep6_tests {
    use super::*;

    #[test]
    fn laggard_never_empty_for_real_snapshot() {
        // 快照里必有达成率最低项（20 项非空快照）。
        assert!(!laggard_ranking(1).is_empty());
    }

    #[test]
    fn glossary_defines_not_translations() {
        // 定义必须是"是什么"，不是"翻译成英文"——抽查两条含中文语义词。
        assert!(GLOSSARY[2].1.contains("判据集合"));
        assert!(GLOSSARY[4].1.contains("完成态"));
    }
}
