//! F040 深化批次三 · 账本对拍引擎面（compatstar2/deep2 · G-A-40）。
//!
//! 批次一深化覆盖 JSONL 序列化/季报生成/类别统计，批次二覆盖季度差分/
//! 样本升级判定/攻坚名单管线/快照链单调校验；本批补齐【功能定义】
//! 「逐件实测建档、通过率季度刷新」全语义对齐的执行/边界/注入面：
//! 三源一致性表决器（main/backup/diff 同键值表决——F128 全量 JSON 开放
//! 的多源对拍底座）、季度 diff 引擎（条目增/删/改三分类，定长 64 条目
//! 对比）、攻坚甘特预算模型（周预算数组 12 周 + 进度插值 + 落后周检出
//! ——攻坚名单的排期量化面）、50 件状态机迁移合法性表（green→yellow→
//! red→archived 四态表驱动校验）。
//!
//! 判据对账：主册 G-A-40【设计细节】「账本数据文件版本化（季度快照
//! 不可变，历史可溯）」+【状态与异常】「连续两季红的件 → 移入攻坚名单
//! 公示」的排期执行面；无 MS 面（纯账本域——对拍源为主册段）。
//!
//! 零堆纪律：定长账表 + 借片扫描，无 alloc。

use crate::checks::CheckSet;

// ---------------------------------------------------------------------------
// 三源一致性表决器
// ---------------------------------------------------------------------------

/// 三源一致性表决：main/backup/diff 同键值表决。≥2 同取同值（无异议）；
/// 三方各异取 main 并记账异源（返回 (取值, 异议标记)——异议即账面）。
pub fn vote3(main: u64, backup: u64, diff: u64) -> (u64, bool) {
    if main == backup || main == diff {
        (main, false)
    } else if backup == diff {
        (backup, false)
    } else {
        (main, true) // 三方各异 → 取 main，异源如实入账
    }
}

// ---------------------------------------------------------------------------
// 季度 diff 引擎
// ---------------------------------------------------------------------------

/// 对比容量 64 条（定长——零堆纪律）。
pub const DIFF_CAP: usize = 64;

/// 一条账目（键 + 值——状态量化编码，键为件 ID）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Entry {
    pub key: u64,
    pub val: u64,
}

/// 季度 diff 三分类计数：增/删/改。
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct DiffCounts {
    pub added: u32,
    pub deleted: u32,
    pub modified: u32,
}

/// 定长季度账（64 条上限；超限拒绝并计数——显性化，不静默截断）。
pub struct QuarterLedger {
    pub entries: [Option<Entry>; DIFF_CAP],
    pub count: usize,
    pub overflow: u32,
}

impl QuarterLedger {
    pub const fn new() -> Self {
        QuarterLedger { entries: [None; DIFF_CAP], count: 0, overflow: 0 }
    }

    pub fn put(&mut self, e: Entry) -> bool {
        if self.count >= DIFF_CAP {
            self.overflow += 1;
            return false;
        }
        self.entries[self.count] = Some(e);
        self.count += 1;
        true
    }
}

/// 季度 diff 引擎：旧有新无=删、新有旧无=增、两有值异=改（三类计数）。
pub fn diff_ledgers(old: &QuarterLedger, new: &QuarterLedger) -> DiffCounts {
    let mut d = DiffCounts::default();
    for o in old.entries.iter().flatten() {
        match new.entries.iter().flatten().find(|n| n.key == o.key) {
            None => d.deleted += 1,
            Some(n) if n.val != o.val => d.modified += 1,
            Some(_) => {}
        }
    }
    for n in new.entries.iter().flatten() {
        if !old.entries.iter().flatten().any(|o| o.key == n.key) {
            d.added += 1;
        }
    }
    d
}

// ---------------------------------------------------------------------------
// 攻坚甘特预算模型
// ---------------------------------------------------------------------------

/// 攻坚甘特周数 12（周预算数组口径——攻坚名单的排期量化面）。
pub const GANTT_WEEKS: usize = 12;

/// 攻坚甘特预算模型：周预算数组 12 周；进度插值（已完成/总数 × 周数，
/// 百分之一周精度）vs 计划线；落后周检出（计划累计 > 已完成的周数）。
pub struct Gantt {
    /// 每周计划完成件数（周预算线）。
    pub plan: [u32; GANTT_WEEKS],
}

impl Gantt {
    pub const fn new(plan: [u32; GANTT_WEEKS]) -> Self {
        Gantt { plan }
    }

    /// 总预算件数。
    pub fn total(&self) -> u32 {
        self.plan.iter().sum()
    }

    /// 进度插值：已完成/总数 × 12 周（×100 百分之一周精度；600 = 第 6 周）。
    pub fn progress_weeks_x100(&self, done: u32) -> u32 {
        let total = self.total();
        if total == 0 {
            return 0;
        }
        ((done as u64 * 100 * GANTT_WEEKS as u64) / total as u64) as u32
    }

    /// 落后周数检出：计划累计到该周末仍 > 已完成 → 该周落后。
    pub fn lag_weeks(&self, done: u32) -> u32 {
        let mut cum = 0u32;
        let mut lag = 0u32;
        for i in 0..GANTT_WEEKS {
            cum += self.plan[i];
            if cum > done {
                lag += 1;
            }
        }
        lag
    }
}

// ---------------------------------------------------------------------------
// 50 件状态机迁移合法性表
// ---------------------------------------------------------------------------

/// 50 件状态机四态（主册 ItemStatus 同源口径：绿/黄/红/归档）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ItemState {
    Green,
    Yellow,
    Red,
    Archived,
}

/// 合法迁移表：仅允许 g→y、y→r、r→a、y→g（修复）、r→y（部分修复）。
pub const LEGAL_TRANSITIONS: [(ItemState, ItemState); 5] = [
    (ItemState::Green, ItemState::Yellow),
    (ItemState::Yellow, ItemState::Red),
    (ItemState::Red, ItemState::Archived),
    (ItemState::Yellow, ItemState::Green),
    (ItemState::Red, ItemState::Yellow),
];

/// 迁移合法性校验（表驱动——一张表即全部规则）。
pub fn transition_allowed(from: ItemState, to: ItemState) -> bool {
    LEGAL_TRANSITIONS.iter().any(|&(f, t)| f == from && t == to)
}

/// 迁移执行器：非法迁移拒绝并计数（零静默吞错）。
pub struct StateMachine {
    pub state: ItemState,
    pub rejections: u32,
    /// 合法迁移执行计数。
    pub transitions: u32,
}

impl StateMachine {
    pub const fn new() -> Self {
        StateMachine { state: ItemState::Green, rejections: 0, transitions: 0 }
    }

    pub fn apply(&mut self, to: ItemState) -> Result<ItemState, &'static str> {
        if !transition_allowed(self.state, to) {
            self.rejections += 1;
            return Err("illegal-transition");
        }
        self.state = to;
        self.transitions += 1;
        Ok(to)
    }
}

/// 域自检（深化批次三）。
pub fn run_f040e_checks() -> CheckSet {
    let mut cs = CheckSet::new("F040-compatledger-d3");
    // 1) 表决：main 与 backup 同 → 取同值无异议。
    cs.add("vote_main_backup_agree", vote3(7, 7, 9) == (7, false), "");
    // 2) 表决：backup 与 diff 同（main 异）→ 取多数值。
    cs.add("vote_backup_diff_agree", vote3(7, 9, 9) == (9, false), "");
    // 3) 表决：三方各异 → 取 main 并记账异源（异议标记 true）。
    cs.add("vote_all_differ_takes_main", vote3(7, 9, 11) == (7, true), "");
    // 4) 季度 diff 三分类：删 1（key1）、改 1（key2）、增 1（key4）。
    let mut old = QuarterLedger::new();
    let _ = old.put(Entry { key: 1, val: 10 });
    let _ = old.put(Entry { key: 2, val: 20 });
    let _ = old.put(Entry { key: 3, val: 30 });
    let mut new = QuarterLedger::new();
    let _ = new.put(Entry { key: 2, val: 22 });
    let _ = new.put(Entry { key: 3, val: 30 });
    let _ = new.put(Entry { key: 4, val: 40 });
    let d = diff_ledgers(&old, &new);
    cs.add(
        "quarter_diff_classes",
        d == DiffCounts { added: 1, deleted: 1, modified: 1 },
        "",
    );
    // 5) 定长 64 条账：第 65 条拒收并记账（不静默截断）。
    let mut full = QuarterLedger::new();
    for k in 0..65u64 {
        let _ = full.put(Entry { key: k, val: k });
    }
    cs.add("ledger_cap_64", full.count == DIFF_CAP && full.overflow == 1, "");
    // 6) 甘特：周预算 1 件/周、完成 6 件 → 进度恰第 6 周、落后 6 周。
    let g = Gantt::new([1; GANTT_WEEKS]);
    cs.add(
        "gantt_on_plan_progress",
        g.total() == 12 && g.progress_weeks_x100(6) == 600 && g.lag_weeks(6) == 6,
        "",
    );
    // 7) 甘特：完成 12 件 → 零落后；完成 11 件 → 仅第 12 周落后；完成 0 → 12 周全落后。
    cs.add(
        "gantt_lag_weeks",
        g.lag_weeks(12) == 0 && g.lag_weeks(11) == 1 && g.lag_weeks(0) == 12,
        "",
    );
    // 8) 迁移合法性表：五条合法边全过（含修复 g←y 与部分修复 y←r）。
    cs.add(
        "transitions_legal_table",
        transition_allowed(ItemState::Green, ItemState::Yellow)
            && transition_allowed(ItemState::Yellow, ItemState::Red)
            && transition_allowed(ItemState::Red, ItemState::Archived)
            && transition_allowed(ItemState::Yellow, ItemState::Green)
            && transition_allowed(ItemState::Red, ItemState::Yellow),
        "",
    );
    // 9) 非法迁移拒绝并计数：g→r、g→a、y→y、r→g、a→y 全拒（表外即非法）。
    let mut sm = StateMachine::new();
    let r1 = sm.apply(ItemState::Red);
    let r2 = sm.apply(ItemState::Archived);
    let _ = sm.apply(ItemState::Yellow);
    let r3 = sm.apply(ItemState::Yellow);
    let _ = sm.apply(ItemState::Red);
    let r4 = sm.apply(ItemState::Green);
    let _ = sm.apply(ItemState::Archived);
    let r5 = sm.apply(ItemState::Yellow);
    cs.add(
        "transitions_illegal_counted",
        r1 == Err("illegal-transition")
            && r2 == Err("illegal-transition")
            && r3 == Err("illegal-transition")
            && r4 == Err("illegal-transition")
            && r5 == Err("illegal-transition")
            && sm.rejections == 5,
        "",
    );
    // 10) 合法全走：g→y→r→a 归档不删除（主册「样本淘汰 → 归档不删除」）。
    let mut sw = StateMachine::new();
    let walk = sw.apply(ItemState::Yellow).is_ok()
        && sw.apply(ItemState::Red).is_ok()
        && sw.apply(ItemState::Archived).is_ok();
    cs.add(
        "machine_walk_archive",
        walk && sw.state == ItemState::Archived && sw.transitions == 3 && sw.rejections == 0,
        "",
    );
    cs
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vote_unanimous() {
        assert_eq!(vote3(5, 5, 5), (5, false), "三方一致 → 取值无异议");
    }

    #[test]
    fn gantt_lag_boundary() {
        let g = Gantt::new([1; GANTT_WEEKS]);
        assert_eq!(g.lag_weeks(11), 1, "差 1 件 → 仅第 12 周落后");
        assert_eq!(g.lag_weeks(12), 0, "齐平 → 零落后");
        assert_eq!(g.progress_weeks_x100(3), 300, "插值 1% 周精度");
    }

    #[test]
    fn deep3_checks_all_green() {
        let cs = run_f040e_checks();
        assert!(cs.all_passed() && !cs.truncated());
    }
}
