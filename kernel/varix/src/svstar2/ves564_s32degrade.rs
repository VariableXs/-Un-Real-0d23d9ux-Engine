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
    /// 降级振荡（短窗升降超限）。
    Oscillation = SEG_BASE + 0x03,
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

/// 档位迁移记录：from → to + 加权得分。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct LevelShift {
    pub from: u8,
    pub to: u8,
    pub score: u32,
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
    /// 深化：档位迁移履历（时间序，只增不删）。
    pub shifts: Vec<LevelShift>,
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
        let prev = self.level;
        self.level = if w >= 300 {
            DegLevel::Frozen
        } else if w >= 200 {
            DegLevel::RedlineOnly
        } else if w >= 100 {
            DegLevel::Incremental
        } else {
            DegLevel::Full
        };
        if prev != self.level {
            self.record_shift(prev, self.level, w);
        }
        Ok(self.level)
    }

    /// 恢复逐级回升（回升一级；资源持续好转逐次调用）。
    pub fn recover_step(&mut self) -> DegLevel {
        if self.level != DegLevel::Full {
            let prev = self.level;
            self.level = match self.level {
                DegLevel::Frozen => DegLevel::RedlineOnly,
                DegLevel::RedlineOnly => DegLevel::Incremental,
                _ => DegLevel::Full,
            };
            self.record_shift(prev, self.level, 0);
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

    // ─── 深化块：降级履历账本 + 振荡检测 ───

    /// 迁移履历（按时间序追加，只增不删）。
    /// 注：字段 `shifts` 在 struct 定义处；此处提供登记口。
    /// 档位迁移登记：apply/recover_step 内部自动调用。
    fn record_shift(&mut self, from: DegLevel, to: DegLevel, score: u32) {
        self.shifts.push(LevelShift { from: from as u8, to: to as u8, score });
    }

    /// 振荡检测：履历尾部 window 条内升降往复次数 ≥ limit 即告警。
    /// 往复 = 相邻两条迁移方向相反。
    pub fn oscillating(&self, window: usize, limit: u32) -> bool {
        if self.shifts.len() < 2 {
            return false;
        }
        let start = self.shifts.len().saturating_sub(window);
        let seg = &self.shifts[start..];
        let mut flips = 0u32;
        for w in seg.windows(2) {
            let d0 = w[0].to as i32 - w[0].from as i32;
            let d1 = w[1].to as i32 - w[1].from as i32;
            if d0 * d1 < 0 {
                flips += 1;
            }
        }
        flips >= limit
    }

    /// 迁移单向守恒：每条记录 to != from 且两端档位 ≤ 3。
    pub fn shifts_sane(&self) -> bool {
        self.shifts.iter().all(|s| s.from != s.to && s.from <= 3 && s.to <= 3)
    }

    // ─── 深化块：补扫优先出队（红线项优先）───

    /// 补扫优先出队：红线项先出，非红线项按入队序后出。
    /// 空队返回 None。
    pub fn pop_backlog(&mut self) -> Option<u32> {
        if let Some(pos) = self.backlog.iter().position(|i| self.redlines.contains(i)) {
            return Some(self.backlog.remove(pos));
        }
        if self.backlog.is_empty() {
            None
        } else {
            Some(self.backlog.remove(0))
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

    // ── 深化判据：迁移履历账本 ──
    // dd2 经历了 4 次 apply（3 次迁移）+ 3 次 recover，履历应 6 条且 sane。
    chk(&mut set, "C564-13", dd2.shifts.len() == 6 && dd2.shifts_sane(), "履历 6 条全 sane");

    // ── 深化判据：振荡检测（同窗升降往复 ≥2 告警）──
    let mut ddo = DegDesk::new();
    let _ = ddo.apply(LoadSignal { cpu: 30, mem: 30, battery: 0 }); // →Redline (升1)
    let _ = ddo.apply(LoadSignal { cpu: 10, mem: 10, battery: 0 }); // →Full (降)
    let _ = ddo.apply(LoadSignal { cpu: 30, mem: 30, battery: 0 }); // →Redline (升)
    let _ = ddo.apply(LoadSignal { cpu: 10, mem: 10, battery: 0 }); // →Full (降)
    chk(&mut set, "C564-14", ddo.shifts.len() == 4 && ddo.oscillating(4, 3),
        "3 次往复振荡告警");
    // 单向迁移不告警。
    let mut ddo2 = DegDesk::new();
    let _ = ddo2.apply(LoadSignal { cpu: 20, mem: 20, battery: 0 });
    let _ = ddo2.apply(LoadSignal { cpu: 30, mem: 30, battery: 0 });
    let _ = ddo2.apply(LoadSignal { cpu: 40, mem: 45, battery: 5 });
    chk(&mut set, "C564-15", !ddo2.oscillating(4, 3) && ddo2.shifts_sane(),
        "单向降级不告警");

    // ── 深化判据：补扫优先出队（红线先出）──
    let mut ddp = DegDesk::new();
    ddp.redlines.push(5);
    ddp.defer(100);
    ddp.defer(5);
    ddp.defer(101);
    chk(&mut set, "C564-16", ddp.pop_backlog() == Some(5) && ddp.pop_backlog() == Some(100)
        && ddp.pop_backlog() == Some(101) && ddp.pop_backlog().is_none(),
        "红线优先出队后 FIFO");

    // ── 深化判据：LCG 信号注入回归（映射单调性：得分升档不降）──
    let mut rng: u64 = 0x5640_DEAD;
    let mut mono_ok = true;
    for _ in 0..64 {
        rng = rng.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        let cpu = (rng % 101) as u8;
        rng = rng.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        let mem = (rng % 101) as u8;
        rng = rng.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        let bat = (rng % 101) as u8;
        let s = LoadSignal { cpu, mem, battery: bat };
        let w = weighted(s);
        let expect = if w >= 300 { DegLevel::Frozen }
            else if w >= 200 { DegLevel::RedlineOnly }
            else if w >= 100 { DegLevel::Incremental } else { DegLevel::Full };
        let mut di = DegDesk::new();
        if di.apply(s) != Ok(expect) {
            mono_ok = false;
        }
    }
    chk(&mut set, "C564-17", mono_ok, "64 轮随机信号映射全对");

    // ── 深化判据：空履历振荡检测不误报 ──
    let dde = DegDesk::new();
    chk(&mut set, "C564-18", !dde.oscillating(4, 1) && dde.shifts.is_empty(),
        "空履历不误报");

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

    #[test]
    fn shift_ledger_and_oscillation() {
        let mut dd = DegDesk::new();
        let _ = dd.apply(LoadSignal { cpu: 30, mem: 30, battery: 0 });
        let _ = dd.apply(LoadSignal { cpu: 10, mem: 10, battery: 0 });
        assert_eq!(dd.shifts.len(), 2);
        assert!(dd.shifts_sane());
        let _ = dd.apply(LoadSignal { cpu: 30, mem: 30, battery: 0 });
        assert!(dd.oscillating(3, 2));
    }

    #[test]
    fn backlog_priority_pop() {
        let mut dd = DegDesk::new();
        dd.redlines.push(9);
        dd.defer(1);
        dd.defer(9);
        assert_eq!(dd.pop_backlog(), Some(9));
        assert_eq!(dd.pop_backlog(), Some(1));
        assert_eq!(dd.pop_backlog(), None);
    }
}
