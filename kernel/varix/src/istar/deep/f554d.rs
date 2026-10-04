//! 深化层 · F554 定时勿扰（回炉补深主册【设计要点】未实装机制）。
//!
//! 补深三条（判据唯一源：主册 F554 节）：
//! ①「时段内自动进勿扰档」的**重叠取严**——两条计划时段重叠时取更严档
//!   （全静 > 仅声音），不许先到先得；跨午夜时段的周几归属按**起始日**
//!   判定（22:00-8:00 周五的计划，周六 00:30 仍按周五的周几位判定）；
//! ②「例外穿透」的**事件账**——谁在勿扰期间穿透了（重要联系人/日程
//!   提醒），逐笔记账可回看；
//! ③「档位切换有记录（第二天知道夜里静过）」的**明细账**——基础层只有
//!   切换条数，深化层记 (分钟, 旧档, 新档) 环账，新到旧可读。

use crate::checks::CheckSet;
use crate::istar::dndtimer::{DndLevel, DndPlan, DndTimer, ExceptionKind, WeekMask};
use crate::istar::ibase::ISTAR_DOMAIN;

// ---------------------------------------------------------------------------
// 重叠取严 + 跨午夜周几归属
// ---------------------------------------------------------------------------

/// 档位取严：全静 > 仅声音。
pub fn strictest(a: DndLevel, b: DndLevel) -> DndLevel {
    match (a, b) {
        (DndLevel::FullSilent, _) | (_, DndLevel::FullSilent) => DndLevel::FullSilent,
        _ => DndLevel::SoundOnly,
    }
}

/// 跨午夜时段的周几归属：`now < end` 且 `start > end`（在跨午夜后半段）
/// 时归属**前一天**的周几位。
pub fn owns_day_bit(plan_week: WeekMask, start_min: u32, end_min: u32, now_min: u32, today_bit: u8) -> bool {
    let effective_bit = if start_min > end_min && now_min < end_min {
        // 前一天（bit 环回：0 ← 6）。
        if today_bit == 0 {
            6
        } else {
            today_bit - 1
        }
    } else {
        today_bit
    };
    plan_week & (1 << effective_bit) != 0
}

/// 多计划取严判定：在 `now_min`/`today_bit` 时刻，所有启用且命中
/// （时段 + 归属周几位）的计划中取最严档；无命中返回 None。
///
/// 时段判定复用基础层 ibase::in_window 的跨午夜语义（含头不含尾）。
pub fn resolve_strictest(plans: &[DndPlan], now_min: u32, today_bit: u8) -> Option<DndLevel> {
    let mut acc: Option<DndLevel> = None;
    for p in plans {
        if !p.enabled {
            continue;
        }
        let in_win = if p.start_min == p.end_min {
            false
        } else if p.start_min < p.end_min {
            now_min >= p.start_min && now_min < p.end_min
        } else {
            now_min >= p.start_min || now_min < p.end_min
        };
        if in_win && owns_day_bit(p.week, p.start_min, p.end_min, now_min, today_bit) {
            acc = Some(match acc {
                None => p.level,
                Some(l) => strictest(l, p.level),
            });
        }
    }
    acc
}

// ---------------------------------------------------------------------------
// 穿透事件账与档位切换明细账
// ---------------------------------------------------------------------------

/// 一条穿透事件。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PassthroughEvent {
    pub kind: ExceptionKind,
    pub day_min: u32,
}

/// 穿透事件环账（容量 16）。
pub struct PassthroughLog {
    buf: [Option<PassthroughEvent>; 16],
    len: usize,
}

impl PassthroughLog {
    pub fn new() -> PassthroughLog {
        PassthroughLog { buf: [None; 16], len: 0 }
    }

    pub fn record(&mut self, kind: ExceptionKind, day_min: u32) {
        if self.len < 16 {
            self.buf[self.len] = Some(PassthroughEvent { kind, day_min });
            self.len += 1;
        } else {
            for i in 1..16 {
                self.buf[i - 1] = self.buf[i];
            }
            self.buf[15] = Some(PassthroughEvent { kind, day_min });
        }
    }

    pub fn len(&self) -> usize {
        self.len
    }

    pub fn get(&self, i: usize) -> Option<PassthroughEvent> {
        if i < self.len {
            self.buf[i]
        } else {
            None
        }
    }

    /// 按类型计数（「夜里谁的提醒进来了」的对账口径）。
    pub fn count_kind(&self, kind: ExceptionKind) -> usize {
        (0..self.len).filter(|&i| self.get(i).map(|e| e.kind) == Some(kind)).count()
    }
}

/// 一条档位切换明细。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SwitchEntry {
    pub day_min: u32,
    /// 旧档（0=无勿扰 1=仅声音 2=全静）。
    pub from: u8,
    pub to: u8,
}

fn level_code(l: Option<DndLevel>) -> u8 {
    match l {
        None => 0,
        Some(DndLevel::SoundOnly) => 1,
        Some(DndLevel::FullSilent) => 2,
    }
}

/// 档位切换明细环账（容量 16，新到旧读出）。
pub struct SwitchLog {
    buf: [Option<SwitchEntry>; 16],
    head: usize,
    len: usize,
}

impl SwitchLog {
    pub fn new() -> SwitchLog {
        SwitchLog { buf: [None; 16], head: 0, len: 0 }
    }

    pub fn record(&mut self, day_min: u32, from: Option<DndLevel>, to: Option<DndLevel>) {
        if from == to {
            return;
        }
        self.buf[self.head] = Some(SwitchEntry { day_min, from: level_code(from), to: level_code(to) });
        self.head = (self.head + 1) % 16;
        if self.len < 16 {
            self.len += 1;
        }
    }

    /// 新到旧快照。
    pub fn newest_first(&self) -> alloc::vec::Vec<SwitchEntry> {
        let mut out = alloc::vec::Vec::with_capacity(self.len);
        let mut i = (self.head + 16 - 1) % 16;
        for _ in 0..self.len {
            if let Some(e) = self.buf[i] {
                out.push(e);
            }
            i = (i + 16 - 1) % 16;
        }
        out
    }

    pub fn len(&self) -> usize {
        self.len
    }
}

impl Default for PassthroughLog {
    fn default() -> Self {
        Self::new()
    }
}

impl Default for SwitchLog {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// 深化自检
// ---------------------------------------------------------------------------

pub fn run_f554_deep_checks() -> CheckSet {
    let mut cs = CheckSet::new(ISTAR_DOMAIN);

    // 1) 重叠取严：全静 + 仅声音重叠 → 全静。
    let plans = [
        DndPlan { start_min: 22 * 60, end_min: 8 * 60, week: 0b111_1111, level: DndLevel::FullSilent, enabled: true },
        DndPlan { start_min: 23 * 60, end_min: 7 * 60, week: 0b111_1111, level: DndLevel::SoundOnly, enabled: true },
    ];
    cs.add(
        "overlap strictest wins",
        resolve_strictest(&plans, 23 * 60 + 30, 5) == Some(DndLevel::FullSilent),
        "",
    );

    // 2) 跨午夜周几归属：周六 00:30 归属周五（起始日）计划；周五 00:30
    //    归属周四（也是工作日，仍命中）；周日 00:30 归属周六（不命中）。
    let workday_nightly = DndPlan {
        start_min: 22 * 60,
        end_min: 8 * 60,
        week: 0b011_1110, // 周一~周五（bit1..bit5）
        level: DndLevel::SoundOnly,
        enabled: true,
    };
    let fri = 5u8; // bit5=周五
    let sat = 6u8; // bit6=周六
    let sun = 0u8; // bit0=周日
    cs.add(
        "cross midnight owns start day",
        resolve_strictest(&[workday_nightly], 0 * 60 + 30, sat) == Some(DndLevel::SoundOnly)
            && resolve_strictest(&[workday_nightly], 0 * 60 + 30, fri).is_some()
            && resolve_strictest(&[workday_nightly], 0 * 60 + 30, sun).is_none(),
        "",
    );

    // 3) 周六白天不在工作日夜段（own-day 语义不误扩周末白天）。
    cs.add("saturday daytime uncovered", resolve_strictest(&[workday_nightly], 12 * 60, sat).is_none(), "");

    // 4) 穿透事件账：两类各记一笔、按类计数、环滚动。
    let mut pl = PassthroughLog::new();
    pl.record(ExceptionKind::StarContact, 23 * 60 + 15);
    pl.record(ExceptionKind::CalendarAlert, 23 * 60 + 40);
    pl.record(ExceptionKind::StarContact, 23 * 60 + 55);
    cs.add(
        "passthrough log per kind",
        pl.len() == 3 && pl.count_kind(ExceptionKind::StarContact) == 2
            && pl.count_kind(ExceptionKind::CalendarAlert) == 1,
        "",
    );

    // 5) 档位切换明细账：进勿扰/出勿扰/换档逐笔，新到旧。
    let mut sl = SwitchLog::new();
    sl.record(22 * 60, None, Some(DndLevel::FullSilent));
    sl.record(23 * 60, Some(DndLevel::FullSilent), Some(DndLevel::SoundOnly));
    sl.record(8 * 60, Some(DndLevel::SoundOnly), None);
    let log = sl.newest_first();
    cs.add(
        "switch log newest first",
        sl.len() == 3
            && log[0] == SwitchEntry { day_min: 8 * 60, from: 1, to: 0 }
            && log[2] == SwitchEntry { day_min: 22 * 60, from: 0, to: 2 },
        "",
    );

    // 6) 引擎与深化判定一致：单计划场景下 tick_minute 的档位 == 取严判定。
    let mut t = DndTimer::new();
    let _ = t.add_plan(workday_nightly);
    t.tick_minute(23 * 60, fri);
    let engine = t.level();
    cs.add(
        "engine matches strictest resolver",
        engine == resolve_strictest(&[workday_nightly], 23 * 60, fri),
        "",
    );

    cs
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strictest_total_order() {
        assert_eq!(strictest(DndLevel::SoundOnly, DndLevel::SoundOnly), DndLevel::SoundOnly);
        assert_eq!(strictest(DndLevel::FullSilent, DndLevel::SoundOnly), DndLevel::FullSilent);
    }

    #[test]
    fn passthrough_ring_rolls() {
        let mut pl = PassthroughLog::new();
        for m in 0..20u32 {
            pl.record(ExceptionKind::StarContact, m);
        }
        assert_eq!(pl.len(), 16);
        assert_eq!(pl.get(0).unwrap().day_min, 4); // 最旧 4 笔滚出
    }

    #[test]
    fn switch_log_ignores_noop() {
        let mut sl = SwitchLog::new();
        sl.record(100, Some(DndLevel::FullSilent), Some(DndLevel::FullSilent));
        assert_eq!(sl.len(), 0);
    }
}
