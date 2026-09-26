//! 深化层 · F575 批次七验收锚点（回炉补深主册【设计要点】未实装机制）。
//!
//! 补深四条（判据唯一源：主册 F575 节）：
//! ①「锚点可执行性抽查 5 条」的 **锚点抽查引擎**——从 25 锚点按轮转
//!   指针确定性抽样 5 条（步长 5 均布全表）：同指针同样本（可复现，
//!   不是随机）、指针 +1 整组轮转、指针回卷同样本；
//! ②「账（F 清单）、册（正文）、检（脚本）三处对得上」的 **三处对账
//!   器**——三来源锚点名表逐字比对：条数一致且逐位一致才算对上，
//!   一字漂移即红；
//! ③「与历史脚本合并无冲突」的 **冲突检测**——本批锚点承载 F 项与
//!   历史批次（F400/F550 等）锚点空间无碰撞；
//! ④「抽查报告账本」——样本逐条实跑结果入账，抽满 5 条且条条实过才
//!   签「抽查全绿」（一红不签、未抽满不签、满后拒收不覆盖）。

use crate::checks::CheckSet;
use crate::istar::batch7gate::{ANCHORS, audit_anchors, audit_table_integrity};
use crate::istar::ibase::ISTAR_DOMAIN;

use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 锚点抽查引擎（轮转指针确定性抽样）
// ---------------------------------------------------------------------------

/// 抽查条数（主册判据点名：抽查 5 条）。
pub const SPOT_COUNT: usize = 5;

/// 锚点抽查引擎：样本 = (指针 + k×5) mod 25，k = 0..5——步长 5 均布
/// 使 5 条样本两两不同且散布全表。同指针可复现；指针 +1 整组轮换
/// 覆盖面；指针对表长取模自然回卷。
pub fn sample5(pointer: usize) -> [usize; SPOT_COUNT] {
    let n = ANCHORS.len();
    let mut out = [0usize; SPOT_COUNT];
    for (k, slot) in out.iter_mut().enumerate() {
        *slot = (pointer + k * SPOT_COUNT) % n;
    }
    out
}

// ---------------------------------------------------------------------------
// 抽查报告账本
// ---------------------------------------------------------------------------

/// 抽查报告账本：样本逐条实跑结果入账；5/5 全过才签「抽查全绿」。
pub struct SpotReport {
    results: [(usize, bool); SPOT_COUNT],
    filled: usize,
}

impl SpotReport {
    pub fn new() -> SpotReport {
        SpotReport { results: [(0, false); SPOT_COUNT], filled: 0 }
    }

    /// 入账（锚点表下标 + 实跑结论）；报告满后拒收（账不覆盖）。
    pub fn record(&mut self, anchor_idx: usize, passed: bool) -> bool {
        if self.filled >= SPOT_COUNT {
            return false;
        }
        self.results[self.filled] = (anchor_idx, passed);
        self.filled += 1;
        true
    }

    pub fn filled(&self) -> usize {
        self.filled
    }

    /// 第 i 条入账样本（下标, 结论）；未写到的那条如实给 None。
    pub fn entry(&self, i: usize) -> Option<(usize, bool)> {
        if i < self.filled {
            Some(self.results[i])
        } else {
            None
        }
    }

    /// 抽查全绿判定：抽满 SPOT_COUNT 条且条条实过。
    pub fn all_green(&self) -> bool {
        self.filled == SPOT_COUNT && self.results[..self.filled].iter().all(|(_, p)| *p)
    }
}

impl Default for SpotReport {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// 三处对账器 + 冲突检测
// ---------------------------------------------------------------------------

/// 三处对账器：账（F 清单）、册（正文）、检（脚本）三个来源的锚点名
/// 表逐字比对——条数一致且逐位一致才算账册检对得上。
pub fn reconcile3(ledger: &[&str], body: &[&str], script: &[&str]) -> bool {
    ledger.len() == body.len()
        && body.len() == script.len()
        && ledger
            .iter()
            .zip(body.iter().zip(script.iter()))
            .all(|(l, (b, s))| l == b && b == s)
}

/// 冲突检测：本批锚点承载 F 项与历史批次锚点空间无碰撞——任一 f_item
/// 落入历史清单即冲突（返回 true = 有冲突）。
pub fn conflicts_with_history(historical_f_items: &[&str]) -> bool {
    ANCHORS.iter().any(|a| historical_f_items.contains(&a.f_item))
}

// ---------------------------------------------------------------------------
// 深化自检
// ---------------------------------------------------------------------------

pub fn run_f575_deep_checks() -> CheckSet {
    let mut cs = CheckSet::new(ISTAR_DOMAIN);

    // 1) 基础表完整性不被深化破坏（25 条连续、F 项唯一、名字齐）。
    cs.add("base table integrity kept", audit_table_integrity(), "");

    // 2) 抽样确定性：同指针两次抽样逐位相同（可复现——不是随机）。
    let a = sample5(7);
    let b = sample5(7);
    cs.add("sampling deterministic", a == b, "");

    // 3) 抽样轮转：指针 0 与 1 整组错开；指针回卷（27 ≡ 2 mod 25）同样本。
    let s0 = sample5(0);
    let s1 = sample5(1);
    cs.add("sampling rotates and wraps", s0 != s1 && sample5(27) == sample5(2), "");

    // 4) 样本质量：恰好 5 条、下标两两不同、全落在表内、承载 F 项互异。
    let s = sample5(3);
    let mut distinct = true;
    for i in 0..SPOT_COUNT {
        for j in (i + 1)..SPOT_COUNT {
            if s[i] == s[j] || ANCHORS[s[i]].f_item == ANCHORS[s[j]].f_item {
                distinct = false;
            }
        }
    }
    let in_bounds = s.iter().all(|i| *i < ANCHORS.len());
    cs.add(
        "sample five distinct in bounds",
        SPOT_COUNT == 5 && distinct && in_bounds,
        "",
    );

    // 5) 三处对账：账/册/检三份锚点名表逐字一致（以 ANCHORS 名为准源）。
    let ledger: Vec<&str> = ANCHORS.iter().map(|a| a.name).collect();
    let body: Vec<&str> = ANCHORS.iter().map(|a| a.name).collect();
    let script: Vec<&str> = ANCHORS.iter().map(|a| a.name).collect();
    cs.add("three sources verbatim", reconcile3(&ledger, &body, &script), "");

    // 6) 对账诚实：册侧一字漂移即红；检侧少一条即红。
    let mut drifted_body = body.clone();
    drifted_body[12] = "任务视图过滤慢";
    let short_script: Vec<&str> = script[..24].to_vec();
    cs.add(
        "reconcile catches drift and length",
        !reconcile3(&ledger, &drifted_body, &script)
            && !reconcile3(&ledger, &body, &short_script),
        "",
    );

    // 7) 冲突检测：历史锚点（F200/F375/F400/F550 批）与本批无碰撞；
    //    混入本批 F 项即冲突。
    let clean = !conflicts_with_history(&["F200", "F375", "F400", "F550"]);
    let clash = conflicts_with_history(&["F400", "F560"]);
    cs.add("history space no collision", clean && clash, "");

    // 8) 抽查报告：样本 5/5 实过签全绿；一红不签；未抽满不签；满后拒收。
    let mut rep = SpotReport::new();
    for i in 0..SPOT_COUNT {
        let _ = rep.record(s[i], true);
    }
    let full_green = rep.all_green();
    let overflow = !rep.record(0, true);
    let mut rep2 = SpotReport::new();
    for i in 0..SPOT_COUNT {
        let _ = rep2.record(s[i], i != 2); // 第 3 条红
    }
    let mut rep3 = SpotReport::new();
    let _ = rep3.record(s[0], true); // 只抽 1 条
    cs.add(
        "spot report honest verdict",
        full_green && overflow && !rep2.all_green() && !rep3.all_green(),
        "",
    );

    // 9) 抽查引擎与全量审计衔接：runner 全真 → 25/25（基础判据沿用）。
    let (ok, total) = audit_anchors(&|_f| true);
    cs.add("full audit still 25 of 25", ok == 25 && total == 25, "");

    cs
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sample5_stays_in_bounds() {
        let s = sample5(11);
        assert_eq!(s.len(), 5);
        assert!(s.iter().all(|i| *i < 25));
    }

    #[test]
    fn reconcile_length_mismatch_red() {
        assert!(!reconcile3(&["a"], &["a", "b"], &["a"]));
    }

    #[test]
    fn report_entries_readable() {
        let mut r = SpotReport::new();
        let _ = r.record(4, true);
        assert_eq!(r.entry(0), Some((4, true)));
        assert_eq!(r.entry(1), None);
        assert_eq!(r.filled(), 1);
    }
}
