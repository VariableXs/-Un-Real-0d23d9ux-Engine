//! F040 深化批次二 · 快照差分与样本治理面（compatstar2/deep · G-A-40）。
//!
//! 批次一深化覆盖 JSONL 序列化/季报生成器/类别统计；本批补齐：季度快照
//! 差分（两季间的升/降/平三账——「通过率季度刷新」的增量叙事面）、样本
//! 版本升级判定（semver 比较——「样本升级后回退」归因的前置判定）、
//! 攻坚名单管线（两季红入列 → 四季红建议移除——治理阶梯的完整规则）、
//! 快照链单调校验（季度号严格递增——「快照不可变」的结构级自证）。
//!
//! 零堆纪律：定长表，无 alloc。

use crate::checks::CheckSet;

/// 攻坚入列阈值（连续两季红——批次一 RED_STREAK_LIMIT 同源）。
pub const HARDBALL_PROMOTE_STREAK: u32 = 2;
/// 移除建议阈值（连续四季红——治理阶梯终点）。
pub const REMOVAL_SUGGEST_STREAK: u32 = 4;

/// 状态序（Green=0 < Yellow=1 < Red=2 < Archived=3；与 status_str 四态对齐）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum StatusRank {
    Green,
    Yellow,
    Red,
    Archived,
}

impl StatusRank {
    pub fn rank(self) -> u8 {
        match self {
            StatusRank::Green => 0,
            StatusRank::Yellow => 1,
            StatusRank::Red => 2,
            StatusRank::Archived => 3,
        }
    }
}

/// 季度差分账：升（好转）/降（回退）/平三账。
/// 归档态迁移不计升降（归档是治理动作不是回归——主册【状态与异常】）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct QuarterDiff {
    pub improved: u32,
    pub regressed: u32,
    pub unchanged: u32,
}

/// 逐件差分：rank 变化 → 升/降/平；涉及 Archived 的一律计入 unchanged。
pub fn diff_quarters(before: &[StatusRank], after: &[StatusRank]) -> QuarterDiff {
    let n = before.len().min(after.len());
    let mut d = QuarterDiff { improved: 0, regressed: 0, unchanged: 0 };
    for i in 0..n {
        let (b, a) = (before[i].rank(), after[i].rank());
        if b == StatusRank::Archived.rank() || a == StatusRank::Archived.rank() {
            d.unchanged += 1;
        } else if a < b {
            d.improved += 1;
        } else if a > b {
            d.regressed += 1;
        } else {
            d.unchanged += 1;
        }
    }
    d
}

/// 样本升级判定：新版本 semver 严格大于旧版本 = 升级事件
/// （复用 runtimes::version_cmp——比较器只有一份，一处一事实）。
pub fn is_sample_upgrade(old: &str, new: &str) -> bool {
    crate::compatstar2::runtimes::version_cmp(new, old) == core::cmp::Ordering::Greater
}

/// 攻坚名单管线：红季阶梯（连续红计数 → 治理动作）。
/// <2 = 观察；2-3 = 攻坚公示；≥4 = 建议移除（样本淘汰入口）。
pub fn hardball_action(red_streak: u32) -> &'static str {
    match red_streak {
        0..=1 => "watch",
        2..=3 => "hardball",
        _ => "suggest-removal",
    }
}

/// 快照链单调校验：季度号严格递增 = append-only 不可变的结构自证
/// （批次一 quarter_snapshots 的校验器）。
pub fn snapshot_chain_ok(quarters: &[(u32, u32)]) -> bool {
    for w in quarters.windows(2) {
        if w[1].0 <= w[0].0 {
            return false;
        }
    }
    !quarters.is_empty()
}

/// 域自检（深化批次二）。
pub fn run_f040d_checks() -> CheckSet {
    let mut cs = CheckSet::new("F040-compatledger-d2");
    // 1) 季度差分：绿→黄降 1、红→绿升 1、绿→绿平 1、→归档平 1。
    let before = [StatusRank::Green, StatusRank::Green, StatusRank::Red, StatusRank::Green];
    let after = [StatusRank::Yellow, StatusRank::Green, StatusRank::Green, StatusRank::Archived];
    let d = diff_quarters(&before, &after);
    cs.add("quarter_diff", d == QuarterDiff { improved: 1, regressed: 1, unchanged: 2 }, "");
    // 2) 升级判定：1.3.0 > 1.2.0 = 升级；同版本与回退均非升级。
    cs.add(
        "sample_upgrade",
        is_sample_upgrade("1.2.0", "1.3.0") && !is_sample_upgrade("1.2.0", "1.2.0") && !is_sample_upgrade("1.2.0", "1.1.9"),
        "",
    );
    // 3) 攻坚管线：1 季观察、2 季攻坚、4 季建议移除（阶梯完整）。
    cs.add(
        "hardball_pipeline",
        hardball_action(1) == "watch"
            && hardball_action(HARDBALL_PROMOTE_STREAK) == "hardball"
            && hardball_action(3) == "hardball"
            && hardball_action(REMOVAL_SUGGEST_STREAK) == "suggest-removal",
        "",
    );
    // 4) 快照链：季度号严格递增过；重复季与回退拒；空链拒。
    cs.add(
        "snapshot_chain_monotonic",
        snapshot_chain_ok(&[(1, 960), (2, 980), (3, 975)])
            && !snapshot_chain_ok(&[(1, 960), (1, 970)])
            && !snapshot_chain_ok(&[(2, 980), (1, 960)])
            && !snapshot_chain_ok(&[]),
        "",
    );
    cs
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rank_order_total() {
        // 四态全序：Green < Yellow < Red < Archived（rank 单调）。
        assert!(StatusRank::Green.rank() < StatusRank::Yellow.rank());
        assert!(StatusRank::Yellow.rank() < StatusRank::Red.rank());
        assert!(StatusRank::Red.rank() < StatusRank::Archived.rank());
    }

    #[test]
    fn archived_moves_not_counted() {
        // 归档→绿 与 绿→归档 都不计升降（治理动作≠回归——主册语义）。
        let d1 = diff_quarters(&[StatusRank::Archived], &[StatusRank::Green]);
        let d2 = diff_quarters(&[StatusRank::Green], &[StatusRank::Archived]);
        assert_eq!(d1, QuarterDiff { improved: 0, regressed: 0, unchanged: 1 });
        assert_eq!(d2, QuarterDiff { improved: 0, regressed: 0, unchanged: 1 });
    }

    #[test]
    fn deep2_checks_all_green() {
        let cs = run_f040d_checks();
        assert!(cs.all_passed() && !cs.truncated());
    }
}
