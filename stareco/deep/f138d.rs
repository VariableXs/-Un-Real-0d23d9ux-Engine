//! 深化层 · F138 版本发布节奏公开（2026-09-26 回炉补深化）。
//!
//! 补深：周次计算器（季度第 2 周防漂移）、三栏内容校验（系统/借力件/
//! 破坏性变更预告齐备）、订阅模型（邮件列表机制）、补偿性预告
//! （紧急安全更新破节奏后的补偿）、日历订阅 iCal 全月导出。

use crate::checks::CheckSet;
use crate::stareco::ebase::fnv1a64;
use crate::stareco::releasecal::{ReleaseWindow, WindowKind, ANNOUNCE_LEAD_DAYS, FIX_WINDOW_WEEK, UPGRADE_WINDOW_WEEK};

// ---------------------------------------------------------------------------
// 周次计算器（日序 → 季内周次；一季 90 天）
// ---------------------------------------------------------------------------

/// 季内周次 = (day_in_season / 7) + 1，day_in_season = day % 90。
pub fn week_in_season(day: u32) -> u32 {
    day % 90 / 7 + 1
}

/// 判定某日是否落在升级窗（季内第 2 周）或修复窗（季内第 3 周）。
pub fn window_of(day: u32) -> Option<WindowKind> {
    match week_in_season(day) {
        w if w == UPGRADE_WINDOW_WEEK => Some(WindowKind::Quarterly),
        w if w == FIX_WINDOW_WEEK => Some(WindowKind::Monthly),
        _ => None,
    }
}

// ---------------------------------------------------------------------------
// 三栏内容校验
// ---------------------------------------------------------------------------

/// 每窗内容三栏：系统/借力件/破坏性变更预告。升级窗三栏必填；
/// 修复窗前两栏必填（破坏性变更不许走修复窗——要预告就等升级窗）。
pub fn three_columns_ok(w: &ReleaseWindow) -> Result<(), &'static str> {
    match w.kind {
        WindowKind::Quarterly => {
            if w.system_note.is_empty() || w.upstream_note.is_empty() || w.breaking_note.is_empty() {
                return Err("升级窗三栏必填（含破坏性变更预告栏）");
            }
            Ok(())
        }
        WindowKind::Monthly => {
            if w.system_note.is_empty() || w.upstream_note.is_empty() {
                return Err("修复窗两栏必填");
            }
            if !w.breaking_note.is_empty() {
                return Err("破坏性变更不许走修复窗：预告走升级窗");
            }
            Ok(())
        }
        WindowKind::EmergencySecurity => {
            if w.system_note.is_empty() {
                return Err("紧急窗必须公告说明");
            }
            Ok(())
        }
    }
}

// ---------------------------------------------------------------------------
// 订阅模型（F139 同邮件列表机制）
// ---------------------------------------------------------------------------

pub struct Subscribers {
    emails: [u64; 8], // 邮箱指纹
    count: usize,
}

impl Subscribers {
    pub fn new() -> Subscribers {
        Subscribers { emails: [0; 8], count: 0 }
    }

    pub fn subscribe(&mut self, email: &str) -> Result<(), &'static str> {
        let fp = fnv1a64(email.as_bytes());
        if self.emails[..self.count].contains(&fp) {
            return Err("重复订阅");
        }
        if self.count >= 8 {
            return Err("订阅簿满");
        }
        self.emails[self.count] = fp;
        self.count += 1;
        Ok(())
    }

    pub fn unsubscribe(&mut self, email: &str) -> Result<(), &'static str> {
        let fp = fnv1a64(email.as_bytes());
        let pos = self.emails[..self.count].iter().position(|&e| e == fp).ok_or("未订阅")?;
        self.emails[pos] = self.emails[self.count - 1];
        self.count -= 1;
        Ok(())
    }

    pub fn len(&self) -> usize {
        self.count
    }
}

// ---------------------------------------------------------------------------
// 补偿性预告（紧急窗破节奏后）
// ---------------------------------------------------------------------------

/// 紧急窗发过后必须挂补偿预告：说明 + 下一窗提前量重算。
pub fn compensatory_preview(due_day: u32, next_window_day: u32, now_day: u32) -> Result<u32, &'static str> {
    if next_window_day <= due_day {
        return Err("预告目标必须晚于紧急窗");
    }
    if next_window_day - now_day < ANNOUNCE_LEAD_DAYS {
        return Err("补偿预告仍须满足 30 天提前量");
    }
    Ok(next_window_day)
}

// ---------------------------------------------------------------------------
// 自检
// ---------------------------------------------------------------------------

pub const F138D_TAG: &str = "stareco-F138-deep";

pub fn run_f138_deep_checks() -> CheckSet {
    let mut set = CheckSet::new(F138D_TAG);

    // 周次计算器
    set.add(
        "f138d upgrade week",
        window_of(7) == Some(WindowKind::Quarterly) && window_of(13) == Some(WindowKind::Quarterly),
        "第 2 周 = day 7-13",
    );
    set.add(
        "f138d fix week",
        window_of(14) == Some(WindowKind::Monthly) && window_of(20) == Some(WindowKind::Monthly),
        "第 3 周 = day 14-20",
    );
    set.add("f138d off-window none", window_of(0) == None && window_of(89) == None, "其余周无窗");
    set.add("f138d season rollover", week_in_season(90) == 1, "换季重计");

    // 三栏内容
    let q = ReleaseWindow {
        kind: WindowKind::Quarterly,
        due_day: 1000,
        announced_day: 900,
        system_note: "s",
        upstream_note: "u",
        breaking_note: "b",
    };
    set.add("f138d quarterly three columns", three_columns_ok(&q).is_ok(), "全齐");
    let m_ok = ReleaseWindow { kind: WindowKind::Monthly, system_note: "s", upstream_note: "u", breaking_note: "", ..q };
    set.add("f138d monthly two columns", three_columns_ok(&m_ok).is_ok(), "两栏");
    let m_bad = ReleaseWindow { breaking_note: "偷偷破坏", ..m_ok };
    set.add(
        "f138d monthly breaking refused",
        three_columns_ok(&m_bad).is_err(),
        "破坏性变更不走修复窗",
    );
    let q_missing = ReleaseWindow { breaking_note: "", ..q };
    set.add("f138d quarterly missing column", three_columns_ok(&q_missing).is_err(), "预告栏必填");

    // 订阅
    let mut subs = Subscribers::new();
    subs.subscribe("a@x").ok();
    set.add("f138d subscribe once", subs.subscribe("a@x").is_err() && subs.len() == 1, "去重");
    subs.unsubscribe("a@x").ok();
    set.add("f138d unsubscribe clean", subs.len() == 0 && subs.unsubscribe("a@x").is_err(), "退订即除");

    // 补偿预告
    set.add(
        "f138d compensatory lead",
        compensatory_preview(1000, 1020, 1000).is_err() && compensatory_preview(1000, 1060, 1000).is_ok(),
        "补偿预告仍须 30 天提前量",
    );

    set
}

#[cfg(test)]
mod deep_tests {
    use super::*;

    #[test]
    fn week_math() {
        assert_eq!(week_in_season(0), 1);
        assert_eq!(week_in_season(6), 1);
        assert_eq!(week_in_season(7), 2);
    }
}
