//! F554 定时勿扰 · 完整设计（STAR I 主册 I 域批次七）。
//!
//! **判据（主册）**：时段选择器全型；时段内档位切换与恢复；例外穿透；
//! 跨午夜时段（22:00-8:00）用例；与 F341/F349 叠加优先级。
//!
//! **设计要点（主册）**：
//! - 设置页添加时段（22:00-8:00 每日/工作日/自定义周几——图形化选择器
//!   F441 同源，本模块持计划表数据模型）；
//! - 时段内自动进勿扰档（F341 全静或仅声音——时段绑定档位可选）；
//! - 例外名单独立（重要联系人/日程提醒仍可穿透——穿透规则与 F349 同表）；
//! - 时段结束自动回原档。
//!
//! 时段判定唯一语义源：[`crate::istar::ibase::in_window`]（跨午夜一处实现）。

use crate::checks::CheckSet;
use crate::istar::ibase::{hhmm, in_window, ISTAR_DOMAIN};

// ---------------------------------------------------------------------------
// 规格常量
// ---------------------------------------------------------------------------

/// 勿扰档位（F341 同源枚举——本模块只引用不另立）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DndLevel {
    /// 全静。
    FullSilent,
    /// 仅声音（横幅收进中心，无声）。
    SoundOnly,
}

/// 周几位掩码（bit0=周日 … bit6=周六）。
pub type WeekMask = u8;

/// 每日。
pub const WEEK_DAILY: WeekMask = 0b111_1111;
/// 工作日（周一至周五）。
pub const WEEK_WORKDAYS: WeekMask = 0b011_1110;

/// 例外类型（F349 同表——枚举即穿透规则唯一源）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ExceptionKind {
    /// 重要联系人。
    StarContact,
    /// 日程提醒。
    CalendarAlert,
}

// ---------------------------------------------------------------------------
// 计划表
// ---------------------------------------------------------------------------

/// 一条勿扰计划。
#[derive(Clone, Copy, Debug)]
pub struct DndPlan {
    /// 起始 HHMM（分钟数）。
    pub start_min: u32,
    /// 结束 HHMM（分钟数；跨午夜允许 start > end）。
    pub end_min: u32,
    /// 生效周几位掩码。
    pub week: WeekMask,
    /// 时段绑定的档位。
    pub level: DndLevel,
    /// 启用位。
    pub enabled: bool,
}

/// 勿扰计划引擎（设置页数据模型 + 分钟滴答判定）。
pub struct DndTimer {
    plans: [Option<DndPlan>; 8],
    /// 例外穿透名单。
    exceptions: [Option<ExceptionKind>; 8],
    exc_len: usize,
    /// 引擎判定当前应处的档位（None = 不在勿扰）。
    active_level: Option<DndLevel>,
    /// 勿扰开始前用户的原档（时段结束恢复目标——None = 本来就不勿扰）。
    restore_to: Option<DndLevel>,
    /// 档位切换记录条数（「第二天知道夜里静过」的账面）。
    switch_log: usize,
}

impl DndTimer {
    pub fn new() -> DndTimer {
        DndTimer {
            plans: [None; 8],
            exceptions: [None; 8],
            exc_len: 0,
            active_level: None,
            restore_to: None,
            switch_log: 0,
        }
    }

    /// 添加计划（满 8 条返回 None——诚实拒绝不静默挤掉旧计划）。
    pub fn add_plan(&mut self, plan: DndPlan) -> Option<usize> {
        if plan.start_min == plan.end_min {
            return None; // 空时段无意义，拒绝（诚实零覆盖）。
        }
        for (i, slot) in self.plans.iter_mut().enumerate() {
            if slot.is_none() {
                *slot = Some(plan);
                return Some(i);
            }
        }
        None
    }

    /// 移除计划。
    pub fn remove_plan(&mut self, index: usize) -> bool {
        if index < 8 && self.plans[index].is_some() {
            self.plans[index] = None;
            true
        } else {
            false
        }
    }

    /// 例外名单添加（满 8 条 None）。
    pub fn add_exception(&mut self, kind: ExceptionKind) -> Option<usize> {
        if self.exceptions[..self.exc_len].contains(&Some(kind)) {
            return None; // 幂等。
        }
        if self.exc_len < 8 {
            self.exceptions[self.exc_len] = Some(kind);
            self.exc_len += 1;
            Some(self.exc_len - 1)
        } else {
            None
        }
    }

    /// 例外穿透判定。
    pub fn passthrough(&self, kind: ExceptionKind) -> bool {
        self.exceptions[..self.exc_len].contains(&Some(kind))
    }

    /// 分钟滴答：按「当天分钟 + 周几」判定引擎档位，进出时段各记一笔。
    ///
    /// 叠加优先级（主册「与 F341/F349 叠加」语义）：多计划同时命中取更严档
    /// （全静 > 仅声音）；用户手动勿扰（F341）不受本引擎降档——本引擎只在
    /// 进入时段时记原档、退出时恢复，手动档在恢复时**不被覆盖**。
    pub fn tick_minute(&mut self, day_min: u32, weekday_bit: u8) {
        let mut strictest: Option<DndLevel> = None;
        for slot in self.plans.iter().flatten() {
            if !slot.enabled || slot.week & (1 << weekday_bit) == 0 {
                continue;
            }
            if in_window(day_min, slot.start_min, slot.end_min) {
                strictest = match (strictest, slot.level) {
                    (None, l) => Some(l),
                    (Some(DndLevel::SoundOnly), DndLevel::FullSilent) => Some(DndLevel::FullSilent),
                    (cur, _) => cur,
                };
            }
        }
        match (self.active_level, strictest) {
            (None, Some(l)) => {
                // 手动档（note_manual_level 注入的 restore_to）保留不覆盖——
                // 时段结束恢复到它；无手动档时本就是 None。
                self.active_level = Some(l);
                self.switch_log += 1;
            }
            (Some(_), None) => {
                self.active_level = self.restore_to.take();
                self.switch_log += 1;
            }
            (Some(a), Some(b)) if a != b => {
                self.active_level = Some(b);
                self.switch_log += 1;
            }
            _ => {}
        }
    }

    /// 用户手动档注入（F341 接缝）：手动开启勿扰时记录为恢复目标。
    pub fn note_manual_level(&mut self, level: Option<DndLevel>) {
        if self.active_level.is_none() {
            self.restore_to = level;
        }
    }

    /// 当前档位。
    pub fn level(&self) -> Option<DndLevel> {
        self.active_level
    }

    /// 档位切换记录条数。
    pub fn switch_count(&self) -> usize {
        self.switch_log
    }

    /// 计划数。
    pub fn plan_count(&self) -> usize {
        self.plans.iter().filter(|s| s.is_some()).count()
    }
}

impl Default for DndTimer {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// 自检
// ---------------------------------------------------------------------------

pub fn run_dndtimer_checks() -> CheckSet {
    let mut set = CheckSet::new(ISTAR_DOMAIN);

    // 1. 时段选择器全型：每日/工作日/自定义周掩码三型可表达。
    let daily = DndPlan { start_min: hhmm(22, 0).unwrap(), end_min: hhmm(8, 0).unwrap(), week: WEEK_DAILY, level: DndLevel::FullSilent, enabled: true };
    let work = DndPlan { week: WEEK_WORKDAYS, ..daily };
    let custom = DndPlan { week: 0b001_0001, ..daily }; // 周日+周五
    set.add("three week mask shapes", WEEK_DAILY == 127 && WEEK_WORKDAYS == 62 && (custom.week & 1) == 1 && (work.week & 1) == 0, "");

    // 2. 跨午夜 22:00-8:00：23:00 命中全静；08:00 恢复（档位回 None）。
    let mut d = DndTimer::new();
    d.add_plan(daily);
    d.tick_minute(hhmm(23, 0).unwrap(), 3);
    let in_dnd = d.level() == Some(DndLevel::FullSilent);
    d.tick_minute(hhmm(8, 0).unwrap(), 3);
    set.add("cross-midnight enter and restore", in_dnd && d.level().is_none(), "");

    // 3. 时段绑定档位可选：仅声音档同样进出。
    let mut d2 = DndTimer::new();
    d2.add_plan(DndPlan { level: DndLevel::SoundOnly, ..daily });
    d2.tick_minute(hhmm(23, 30).unwrap(), 3);
    let sound = d2.level() == Some(DndLevel::SoundOnly);
    d2.tick_minute(hhmm(9, 0).unwrap(), 3);
    set.add("sound-only level binds and restores", sound && d2.level().is_none(), "");

    // 4. 例外穿透：名单内类型穿透为 true，名单外 false。
    let mut d3 = DndTimer::new();
    d3.add_exception(ExceptionKind::StarContact);
    set.add(
        "exception passthrough list",
        d3.passthrough(ExceptionKind::StarContact) && !d3.passthrough(ExceptionKind::CalendarAlert),
        "",
    );

    // 5. 周几过滤：工作日计划在周日不生效（bit0 不在掩码）。
    let mut d4 = DndTimer::new();
    d4.add_plan(work);
    d4.tick_minute(hhmm(23, 0).unwrap(), 0);
    let sunday_quiet = d4.level().is_none();
    d4.tick_minute(hhmm(23, 0).unwrap(), 3);
    set.add("weekday mask filters days", sunday_quiet && d4.level() == Some(DndLevel::FullSilent), "");

    // 6. 叠加优先级：全静与仅声音同时命中 → 取全静。
    let mut d5 = DndTimer::new();
    d5.add_plan(daily);
    d5.add_plan(DndPlan { start_min: hhmm(21, 0).unwrap(), end_min: hhmm(23, 59).unwrap(), week: WEEK_DAILY, level: DndLevel::SoundOnly, enabled: true });
    d5.tick_minute(hhmm(22, 30).unwrap(), 3);
    set.add("overlap picks strictest level", d5.level() == Some(DndLevel::FullSilent), "");

    // 7. 档位切换有记录：进一笔、出一笔。
    let mut d6 = DndTimer::new();
    d6.add_plan(daily);
    d6.tick_minute(hhmm(23, 0).unwrap(), 3);
    d6.tick_minute(hhmm(8, 30).unwrap(), 3);
    set.add("switch log records both ways", d6.switch_count() == 2, "");

    // 8. 计划容量与移除：满 8 条拒绝第 9；移除后可再加。
    let mut d7 = DndTimer::new();
    let mut added_all = true;
    for _ in 0..8 {
        if d7.add_plan(daily).is_none() {
            added_all = false;
        }
    }
    let ninth = d7.add_plan(daily).is_none();
    let removed = d7.remove_plan(2);
    let re_add = d7.add_plan(daily).is_some();
    set.add("plan cap and remove", added_all && ninth && removed && re_add, "");

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    fn daily_full() -> DndPlan {
        DndPlan {
            start_min: hhmm(22, 0).unwrap(),
            end_min: hhmm(8, 0).unwrap(),
            week: WEEK_DAILY,
            level: DndLevel::FullSilent,
            enabled: true,
        }
    }

    #[test]
    fn boundary_minutes_exact() {
        let mut d = DndTimer::new();
        d.add_plan(daily_full());
        // 21:59 未进、22:00 进、7:59 还在、8:00 出。
        d.tick_minute(21 * 60 + 59, 3);
        assert!(d.level().is_none());
        d.tick_minute(22 * 60, 3);
        assert!(d.level().is_some());
        d.tick_minute(7 * 60 + 59, 3);
        assert!(d.level().is_some());
        d.tick_minute(8 * 60, 3);
        assert!(d.level().is_none());
    }

    #[test]
    fn empty_plan_rejected() {
        let mut d = DndTimer::new();
        let bad = DndPlan { start_min: 600, end_min: 600, ..daily_full() };
        assert!(d.add_plan(bad).is_none());
    }

    #[test]
    fn exception_idempotent() {
        let mut d = DndTimer::new();
        assert!(d.add_exception(ExceptionKind::CalendarAlert).is_some());
        assert!(d.add_exception(ExceptionKind::CalendarAlert).is_none());
    }

    #[test]
    fn manual_level_not_overwritten_by_restore() {
        let mut d = DndTimer::new();
        d.note_manual_level(Some(DndLevel::FullSilent)); // 用户手动全静
        d.add_plan(daily_full());
        d.tick_minute(23 * 60, 3); // 进计划时段（已在手动勿扰——档一致不重复记账）
        d.tick_minute(9 * 60, 3); // 出时段——恢复到手动档
        assert_eq!(d.level(), Some(DndLevel::FullSilent));
        assert_eq!(d.restore_to, None); // 恢复目标已被取走
    }
}
