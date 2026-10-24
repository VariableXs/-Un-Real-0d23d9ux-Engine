//! VE-F4330 · 一致性降级策略（U07 性能与预算组）
//! 职责：资源紧张时一致性降级——四级梯度（全量→增量→红线 only→冻结
//! 扫描）+ 加权触发器（CPU/内存/电池三信号加权）+ 恢复逐级回升 + 补扫
//! 队列（冻结期积压批量合并执行）+ 冻结期红线变更即时醒扫例外 + 降级
//! 状态诚实呈现（域本色）。
//! 段位：0x35A0（续表 idx564）。行数预算 340。

#![allow(dead_code)]

use alloc::vec;
use alloc::vec::Vec;
use crate::checks::CheckSet;

pub const SEG_BASE: u16 = 0x35A0;

#[repr(u16)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DgCode {
    /// 补扫积压（批量合并执行）。
    BacklogBurst = SEG_BASE,
    /// 冻结期红线变更（醒扫例外）。
    RedlineWake = SEG_BASE + 0x01,
    /// 降级信号越界。
    SignalRange = SEG_BASE + 0x02,
}

/// 四级梯度：0 全量 → 1 增量 → 2 红线 only → 3 冻结扫描。
#[derive(Clone, Copy, Default, PartialEq, Eq, Debug, PartialOrd, Ord)]
pub enum DegLevel {
    #[default]
    Full,
    Incremental,
    RedlineOnly,
    Frozen,
}

/// 三信号加权触发器。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LoadSignal {
    /// 各信号压力 0-100。
    pub cpu: u8,
    pub mem: u8,
    pub battery: u8,
}

/// 权重（内存最敏感，电池次之）。
pub fn weighted(s: LoadSignal) -> u32 {
    s.cpu as u32 * 3 + s.mem as u32 * 4 + s.battery as u32 * 3
}

/// 降级台。
#[derive(Default)]
pub struct DegDesk {
    pub level: DegLevel,
    /// 补扫队列（冻结/红线档积压的待扫项）。
    pub backlog: Vec<u32>,
    /// 红线集合（红线 only 档仅扫这些）。
    pub redlines: Vec<u32>,
    /// 醒扫例外记录。
    pub wakes: u32,
    pub recoveries: u32,
    pub merged_batches: u32,
    last_signal: Option<LoadSignal>,
}

impl DegDesk {
    pub fn new() -> Self {
        DegDesk { level: DegLevel::Full, ..Default::default() }
    }

    /// 信号加权降级：得分映射梯度（0-99 全量 / 100-199 增量 / 200-299
    /// 红线 / >=300 冻结）。信号越界拒绝。
    pub fn apply(&mut self, s: LoadSignal) -> Result<DegLevel, DgCode> {
        if s.cpu > 100 || s.mem > 100 || s.battery > 100 {
            return Err(DgCode::SignalRange);
        }
        let w = weighted(s);
        self.last_signal = Some(s);
        self.level = if w >= 300 {
            DegLevel::Frozen
        } else if w >= 200 {
            DegLevel::RedlineOnly
        } else if w >= 100 {
            DegLevel::Incremental
        } else {
            DegLevel::Full
        };
        Ok(self.level)
    }

    /// 恢复逐级回升（回升一级；资源持续好转逐次调用）。
    pub fn recover_step(&mut self) -> DegLevel {
        if self.level != DegLevel::Full {
            self.level = match self.level {
                DegLevel::Frozen => DegLevel::RedlineOnly,
                DegLevel::RedlineOnly => DegLevel::Incremental,
                _ => DegLevel::Full,
            };
            self.recoveries += 1;
        }
        self.level
    }

    /// 扫描准入：按当前梯度决定可扫集合（红线档只放行红线项）。
    pub fn admits(&self, item: u32) -> bool {
        match self.level {
            DegLevel::Full | DegLevel::Incremental => true,
            DegLevel::RedlineOnly | DegLevel::Frozen => {
                if self.redlines.contains(&item) && self.level == DegLevel::RedlineOnly {
                    true
                } else {
                    // 冻结档或非红线项：入补扫队列。
                    false
                }
            }
        }
    }

    /// 拒扫项入补扫队列（幂等）。
    pub fn defer(&mut self, item: u32) {
        if !self.backlog.contains(&item) {
            self.backlog.push(item);
        }
    }

    /// 补扫积压：批量合并执行（≥16 项触发，清队留痕）。
    pub fn flush_backlog(&mut self) -> Result<usize, DgCode> {
        if self.backlog.len() >= 16 {
            let n = self.backlog.len();
            self.backlog.clear();
            self.merged_batches += 1;
            return Ok(n);
        }
        Err(DgCode::BacklogBurst)
    }

    /// 冻结期红线变更醒扫例外：红线项即时醒扫（不入补扫队列）。
    pub fn redline_wake(&mut self, item: u32) -> Result<(), DgCode> {
        if self.level != DegLevel::Frozen {
            return Ok(()); // 非冻结期无需例外
        }
        if self.redlines.contains(&item) {
            self.wakes += 1;
            return Ok(());
        }
        Err(DgCode::RedlineWake)
    }

    /// 降级状态诚实呈现（域本色）：返回用户可读档位。
    pub fn honest_state(&self) -> &'static str {
        match self.level {
            DegLevel::Full => "全量扫描中",
            DegLevel::Incremental => "增量扫描中",
            DegLevel::RedlineOnly => "仅红线扫描",
            DegLevel::Frozen => "扫描已暂停",
        }
    }
}

fn chk(set: &mut CheckSet, label: &'static str, ok: bool, detail: &'static str) {
    set.add(label, ok, detail);
}

pub fn run_ves564_checks() -> CheckSet {
    let mut set = CheckSet::new("ves564");
    let mut dd = DegDesk::new();

    // C564-01 全量档信号不放级。
    chk(&mut set, "C564-01", dd.apply(LoadSignal { cpu: 20, mem: 20, battery: 10 }) == Ok(DegLevel::Full), "低压全量");

    // C564-02 四级梯度映射（加权 3/4/3：90→全量 140→增量 210→红线 315→冻结）。
    let mut dd2 = DegDesk::new();
    let r1 = dd2.apply(LoadSignal { cpu: 30, mem: 30, battery: 0 }); // 90+120=210→RedlineOnly? 30*3+30*4=210
    let r2 = dd2.apply(LoadSignal { cpu: 20, mem: 20, battery: 0 }); // 60+80=140→Incremental
    let r3 = dd2.apply(LoadSignal { cpu: 10, mem: 10, battery: 0 }); // 30+40=70→Full
    let r4 = dd2.apply(LoadSignal { cpu: 40, mem: 45, battery: 5 }); // 120+180+15=315→Frozen
    chk(&mut set, "C564-02", r1 == Ok(DegLevel::RedlineOnly) && r2 == Ok(DegLevel::Incremental)
        && r3 == Ok(DegLevel::Full) && r4 == Ok(DegLevel::Frozen), "四级梯度映射");

    // C564-03 信号越界拒绝。
    chk(&mut set, "C564-03", dd2.apply(LoadSignal { cpu: 101, mem: 0, battery: 0 }).is_err(), "信号越界拒");

    // C564-04 恢复逐级回升（冻结→红线→增量→全量）。
    chk(&mut set, "C564-04", dd2.level == DegLevel::Frozen
        && dd2.recover_step() == DegLevel::RedlineOnly
        && dd2.recover_step() == DegLevel::Incremental
        && dd2.recover_step() == DegLevel::Full
        && dd2.recoveries == 3, "逐级回升");

    // C564-05 全量档回升不再计次。
    let r5 = dd2.recover_step();
    chk(&mut set, "C564-05", r5 == DegLevel::Full && dd2.recoveries == 3, "全量不再计");

    // C564-06 红线档准入（红线项过·非红线项入补扫）。
    let mut dd3 = DegDesk::new();
    dd3.redlines.push(7);
    let _ = dd3.apply(LoadSignal { cpu: 30, mem: 30, battery: 0 }); // RedlineOnly
    chk(&mut set, "C564-06", dd3.admits(7) && !dd3.admits(9), "红线档准入");

    // C564-07 拒扫项入补扫队列（幂等）。
    dd3.defer(9);
    dd3.defer(9);
    dd3.defer(11);
    chk(&mut set, "C564-07", dd3.backlog == [9, 11], "补扫幂等入队");

    // C564-08 补扫积压批量合并执行（≥16 触发）。
    for i in 0..14u32 {
        dd3.defer(100 + i);
    }
    chk(&mut set, "C564-08", dd3.flush_backlog() == Err(DgCode::BacklogBurst), "不足 16 不放");
    dd3.defer(999);
    let flushed = dd3.flush_backlog();
    chk(&mut set, "C564-08b", flushed == Ok(16) && dd3.backlog.is_empty() && dd3.merged_batches == 1, "满 16 批量执行");

    // C564-09 冻结期红线变更醒扫例外。
    dd3.redlines.push(42);
    let _ = dd3.apply(LoadSignal { cpu: 40, mem: 45, battery: 5 }); // Frozen
    chk(&mut set, "C564-09", dd3.redline_wake(42).is_ok() && dd3.wakes == 1, "红线醒扫例外");

    // C564-10 非红线项醒扫拒绝。
    chk(&mut set, "C564-10", dd3.redline_wake(43) == Err(DgCode::RedlineWake), "非红线醒扫拒");

    // C564-11 降级状态诚实呈现（域本色）。
    chk(&mut set, "C564-11", dd3.honest_state() == "扫描已暂停", "状态诚实呈现");
    let _ = dd3.recover_step();
    chk(&mut set, "C564-11b", dd3.honest_state() == "仅红线扫描", "回升状态同步");

    // C564-12 权重梯度（内存 > CPU = 电池）。
    chk(&mut set, "C564-12", weighted(LoadSignal { cpu: 0, mem: 100, battery: 0 }) == 400
        && weighted(LoadSignal { cpu: 100, mem: 0, battery: 0 }) == 300
        && weighted(LoadSignal { cpu: 0, mem: 0, battery: 100 }) == 300, "权重梯度");

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn level_mapping() {
        let mut dd = DegDesk::new();
        assert_eq!(dd.apply(LoadSignal { cpu: 10, mem: 10, battery: 0 }), Ok(DegLevel::Full));
        assert_eq!(dd.apply(LoadSignal { cpu: 20, mem: 20, battery: 0 }), Ok(DegLevel::Incremental));
        assert_eq!(dd.apply(LoadSignal { cpu: 30, mem: 30, battery: 0 }), Ok(DegLevel::RedlineOnly));
        assert_eq!(dd.apply(LoadSignal { cpu: 40, mem: 40, battery: 0 }), Ok(DegLevel::Frozen));
        assert_eq!(dd.apply(LoadSignal { cpu: 200, mem: 0, battery: 0 }), Err(DgCode::SignalRange));
    }

    #[test]
    fn recover_ladder() {
        let mut dd = DegDesk::new();
        let _ = dd.apply(LoadSignal { cpu: 40, mem: 40, battery: 0 });
        assert_eq!(dd.recover_step(), DegLevel::RedlineOnly);
        assert_eq!(dd.recover_step(), DegLevel::Incremental);
        assert_eq!(dd.recover_step(), DegLevel::Full);
        assert_eq!(dd.recover_step(), DegLevel::Full); // 顶格不再动
    }

    #[test]
    fn redline_gate() {
        let mut dd = DegDesk::new();
        dd.redlines.push(1);
        let _ = dd.apply(LoadSignal { cpu: 30, mem: 30, battery: 0 });
        assert!(dd.admits(1));
        assert!(!dd.admits(2));
        dd.defer(2);
        assert_eq!(dd.backlog, [2]);
    }
}
