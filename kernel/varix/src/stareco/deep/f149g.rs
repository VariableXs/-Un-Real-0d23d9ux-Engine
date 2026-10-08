//! 深化层四 · F149 季度生态报告（2026-09-27 深化批次四 · g 层）。
//!
//! 多季对比矩阵（指标×季度，缺格点名）、目标达成核对（诚实措辞）、
//! 常见问题预答（确定性映射）、报告版本归档链（append-only 哈希）、
//! 指标定义词典（一名一义）。

use crate::checks::CheckSet;
use crate::stareco::ebase;

// ---------------------------------------------------------------------------
// 多季对比矩阵：指标 × 季度格；缺格 = 账面窟窿点名
// ---------------------------------------------------------------------------

pub struct GridCell {
    pub metric: &'static str,
    pub quarter: u32,
    pub value: u32,
}

pub fn grid_holes(cells: &[GridCell], metrics: &[&'static str], quarters: &[u32]) -> alloc::vec::Vec<(&'static str, u32)> {
    let mut holes: alloc::vec::Vec<(&'static str, u32)> = alloc::vec::Vec::new();
    for m in metrics {
        for q in quarters {
            if !cells.iter().any(|c| c.metric == *m && c.quarter == *q) {
                holes.push((m, *q));
            }
        }
    }
    holes
}

/// 单指标季度趋势：连降两季 → 如实标红（数据不说谎）。
pub fn declining_two_quarters(cells: &[GridCell], metric: &str) -> bool {
    let mut v: alloc::vec::Vec<(u32, u32)> =
        cells.iter().filter(|c| c.metric == metric).map(|c| (c.quarter, c.value)).collect();
    v.sort_by_key(|(q, _)| *q);
    if v.len() < 3 {
        return false;
    }
    let n = v.len();
    v[n - 1].1 < v[n - 2].1 && v[n - 2].1 < v[n - 3].1
}

// ---------------------------------------------------------------------------
// 目标达成核对：达标/未达标如实措辞（不粉饰）
// ---------------------------------------------------------------------------

pub struct Goal {
    pub metric: &'static str,
    pub target: u32,
    pub actual: u32,
}

pub fn goal_verdict(g: &Goal) -> &'static str {
    if g.actual >= g.target {
        "达标"
    } else {
        "未达标"
    }
}

/// 达成率千分比（分母 0 如实 0）。
pub fn goal_per_mille(g: &Goal) -> u32 {
    if g.target == 0 {
        0
    } else {
        g.actual.min(g.target * 2) * 1000 / g.target
    }
}

// ---------------------------------------------------------------------------
// 常见问题预答：问题指纹 → 预答指针（确定性映射，未收录如实回报）
// ---------------------------------------------------------------------------

pub fn pre_answer(q: &str, faq: &[(&'static str, &'static str)]) -> Option<&'static str> {
    let fp = ebase::fnv1a64(q.as_bytes());
    faq.iter()
        .find(|(k, _)| ebase::fnv1a64(k.as_bytes()) == fp)
        .map(|(_, a)| *a)
}

// ---------------------------------------------------------------------------
// 报告版本归档链：append-only 哈希链（改一版断一链）
// ---------------------------------------------------------------------------

const ARCHIVE_SEED: u64 = 0xA11C_E000_0000_0001;

pub struct ArchiveChain {
    head: u64,
    versions: usize,
}

impl ArchiveChain {
    pub fn new() -> ArchiveChain {
        ArchiveChain { head: ARCHIVE_SEED, versions: 0 }
    }

    pub fn publish(&mut self, content_fp: u64) -> u64 {
        self.head = ebase::fnv1a64(&self.head.to_be_bytes()) ^ content_fp;
        self.versions += 1;
        self.head
    }

    pub fn verify_replay(&self, content_fps: &[u64]) -> bool {
        let mut h = ARCHIVE_SEED;
        for fp in content_fps {
            h = ebase::fnv1a64(&h.to_be_bytes()) ^ fp;
        }
        h == self.head && content_fps.len() == self.versions
    }

    pub fn len(&self) -> usize {
        self.versions
    }
}

// ---------------------------------------------------------------------------
// 指标定义词典：一名一义（重名重义即账面矛盾）
// ---------------------------------------------------------------------------

pub fn metric_dict_ok(defs: &[(&'static str, &'static str)]) -> bool {
    for (i, (n, _)) in defs.iter().enumerate() {
        if defs[i + 1..].iter().any(|(n2, _)| n2 == n) {
            return false;
        }
    }
    // 定义文本不许空（定义缺失 = 指标不可解释）。
    defs.iter().all(|(_, d)| !d.trim().is_empty())
}

// ---------------------------------------------------------------------------
// 自检
// ---------------------------------------------------------------------------

pub const F149G_TAG: &str = "stareco-F149-deep4";

pub fn run_f149_deep4_checks() -> CheckSet {
    let mut set = CheckSet::new(F149G_TAG);

    // 多季矩阵
    let cells = [
        GridCell { metric: "themes", quarter: 1, value: 120 },
        GridCell { metric: "themes", quarter: 2, value: 130 },
        GridCell { metric: "packs", quarter: 1, value: 40 },
    ];
    let holes = grid_holes(&cells, &["themes", "packs"], &[1, 2]);
    set.add("f149g holes", holes == alloc::vec![("packs", 2)], "缺格点名");
    set.add("f149g decline", declining_two_quarters(&cells, "themes") == false, "上行不标红");
    let down = [
        GridCell { metric: "packs", quarter: 1, value: 50 },
        GridCell { metric: "packs", quarter: 2, value: 40 },
        GridCell { metric: "packs", quarter: 3, value: 30 },
    ];
    set.add("f149g decline red", declining_two_quarters(&down, "packs"), "连降两季标红");
    set.add("f149g decline short", !declining_two_quarters(&down[0..2], "packs"), "窗口不足不判");

    // 目标核对
    let g1 = Goal { metric: "themes", target: 100, actual: 120 };
    let g2 = Goal { metric: "packs", target: 100, actual: 50 };
    set.add(
        "f149g goal verdict",
        goal_verdict(&g1) == "达标" && goal_verdict(&g2) == "未达标",
        "诚实措辞",
    );
    set.add("f149g goal per mille", goal_per_mille(&g1) == 1200 && goal_per_mille(&g2) == 500, "超额 1200‰/半程 500‰");
    set.add(
        "f149g goal cap",
        goal_per_mille(&Goal { metric: "x", target: 10, actual: 999 }) == 2000,
        "超速达成封顶防注水",
    );

    // 预答
    let faq = [("如何打包主题", "见 F127 打包工具文档")];
    set.add(
        "f149g preanswer hit",
        pre_answer("如何打包主题", &faq) == Some("见 F127 打包工具文档"),
        "命中预答",
    );
    set.add("f149g preanswer miss", pre_answer("不明问题", &faq).is_none(), "未收录如实 None");

    // 归档链
    let mut arc = ArchiveChain::new();
    arc.publish(0xAA);
    arc.publish(0xBB);
    set.add("f149g archive ok", arc.verify_replay(&[0xAA, 0xBB]) && arc.len() == 2, "链重放一致");
    set.add("f149g archive tamper", !arc.verify_replay(&[0xAA, 0xCC]), "内容篡改检出");
    set.add("f149g archive drop", !arc.verify_replay(&[0xAA]), "版本丢失检出");

    // 指标词典
    set.add(
        "f149g dict ok",
        metric_dict_ok(&[("themes", "收录主题总数"), ("packs", "图标包总数")]),
        "一名一义",
    );
    set.add(
        "f149g dict dup",
        !metric_dict_ok(&[("themes", "a"), ("themes", "b")]),
        "重名矛盾",
    );
    set.add(
        "f149g dict empty",
        !metric_dict_ok(&[("themes", " ")]),
        "空定义拒绝",
    );

    set
}

#[cfg(test)]
mod deep4_tests {
    use super::*;

    #[test]
    fn decline_exact_boundary() {
        // 恰好三季连降（含平季不判红）。
        let flat = [
            GridCell { metric: "m", quarter: 1, value: 50 },
            GridCell { metric: "m", quarter: 2, value: 50 },
            GridCell { metric: "m", quarter: 3, value: 49 },
        ];
        assert!(!declining_two_quarters(&flat, "m"));
    }

    #[test]
    fn archive_order_matters() {
        let mut a = ArchiveChain::new();
        a.publish(1);
        a.publish(2);
        assert!(!a.verify_replay(&[2, 1])); // 顺序即事实
    }
}
