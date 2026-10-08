//! F138 版本发布节奏公开 · 完整设计（STAR I 主册 G-D-13）。
//!
//! **判据（主册）**：连续两个发布窗按日历兑现或如实归档偏差；公告
//! 提前量 ≥1 窗实测。
//!
//! **设计要点（主册）**：季度功能窗（升级窗=季度第 2 周固定周次防漂
//! 移）/月度修复窗；公告最小提前量 30 天；重大变更提前一个窗公告；
//! 日历数据文件版本化；**归档不可改（历史诚实）**——日期滑动如实
//! 记录（不粉饰）；紧急安全更新打破节奏 → 公告说明+补偿性预告；
//! 每窗内容三栏（系统/借力件/破坏性变更预告）；iCal 导出（通用日历
//! 可订）；公告条目带订阅。
//!
//! 本模块是发布日历的**纯逻辑核**：窗口定义与排程、公告提前量校验、
//! 归档台账（append-only——借 ebase::SeqLedger）、iCal 导出、偏差
//! 如实登记。

use crate::checks::CheckSet;
use crate::stareco::ebase::SeqLedger;

// ---------------------------------------------------------------------------
// 规格
// ---------------------------------------------------------------------------

/// 升级窗固定周次：季度第 2 周（防漂移——不是「月中某天」）。
pub const UPGRADE_WINDOW_WEEK: u32 = 2;
/// 月度修复窗：每月第 3 周的周三（安全修复随月度版发——F142 例外条款
/// 的节奏面）。
pub const FIX_WINDOW_WEEK: u32 = 3;
/// 公告最小提前量（天）。
pub const ANNOUNCE_LEAD_DAYS: u32 = 30;
/// 「提前一个窗」= 提前一个季度窗（90 天）。
pub const ONE_WINDOW_DAYS: u32 = 90;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum WindowKind {
    /// 季度功能窗（升级窗）。
    Quarterly,
    /// 月度修复窗。
    Monthly,
    /// 紧急安全窗（打破节奏——必须公告说明）。
    EmergencySecurity,
}

/// 一扇窗的排程。
#[derive(Clone, Copy, Debug)]
pub struct ReleaseWindow {
    pub kind: WindowKind,
    /// 窗日（日序，天数——调用方注入纪元）。
    pub due_day: u32,
    /// 公告日。
    pub announced_day: u32,
    /// 内容三栏：系统/借力件/破坏性变更预告（非空 = 有预告）。
    pub system_note: &'static str,
    pub upstream_note: &'static str,
    pub breaking_note: &'static str,
}

impl ReleaseWindow {
    /// 公告提前量达标：≥30 天；破坏性变更预告再严格一层：≥1 窗
    /// （90 天）——主册「重大变更提前一个窗公告」。
    pub fn announce_lead_ok(&self) -> bool {
        if self.announced_day > self.due_day {
            return false;
        }
        let lead = self.due_day - self.announced_day;
        if self.breaking_note.is_empty() {
            lead >= ANNOUNCE_LEAD_DAYS
        } else {
            lead >= ONE_WINDOW_DAYS
        }
    }
}

// ---------------------------------------------------------------------------
// 日历
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum WindowOutcome {
    /// 按日历兑现。
    Shipped,
    /// 滑动——偏差如实归档（滑到哪天 + 原因）。
    Slipped,
}

pub struct ReleaseCalendar {
    /// 未来窗（排程面——可改，改动必须重新公告）。
    upcoming: [Option<ReleaseWindow>; 8],
    upcoming_count: usize,
    /// 历史归档（append-only 序号链——「归档不可改」的机制面）。
    archive: SeqLedger,
    /// 归档条目计数。
    archived: u32,
    /// 如实滑动的窗数（不粉饰面）。
    pub slips: u32,
}

impl ReleaseCalendar {
    pub fn new() -> ReleaseCalendar {
        ReleaseCalendar {
            upcoming: [None; 8],
            upcoming_count: 0,
            archive: SeqLedger::new(),
            archived: 0,
            slips: 0,
        }
    }

    pub fn schedule(&mut self, w: ReleaseWindow) -> Result<(), &'static str> {
        if !w.announce_lead_ok() {
            return Err("announce lead below minimum");
        }
        if self.upcoming_count >= 8 {
            return Err("calendar full");
        }
        self.upcoming[self.upcoming_count] = Some(w);
        self.upcoming_count += 1;
        Ok(())
    }

    /// 未来两窗的预告视图（时间轴数据源）。
    pub fn preview_count(&self) -> usize {
        self.upcoming_count.min(2)
    }

    /// 兑现归档：链指纹 = 窗摘要。滑动必须写原因（不粉饰红线）。
    pub fn close(&mut self, due_day: u32, outcome: WindowOutcome, slipped_to: u32, reason: &'static str) -> Result<(), &'static str> {
        match outcome {
            WindowOutcome::Shipped => {
                self.archive.append(crate::stareco::ebase::fnv1a64(b"shipped") ^ (due_day as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15));
            }
            WindowOutcome::Slipped => {
                if reason.is_empty() || slipped_to <= due_day {
                    return Err("slip needs reason and a later date");
                }
                self.slips += 1;
                self.archive
                    .append(crate::stareco::ebase::fnv1a64(reason.as_bytes()) ^ (slipped_to as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15));
            }
        }
        self.archived += 1;
        Ok(())
    }

    pub fn archive_ok(&self) -> bool {
        self.archive.verify()
    }

    pub fn archived_count(&self) -> u32 {
        self.archived
    }

    /// 紧急安全窗例外：打破节奏合法，但必须公告说明（system_note 非空）。
    pub fn emergency_ok(&self, w: &ReleaseWindow) -> bool {
        w.kind == WindowKind::EmergencySecurity && !w.system_note.is_empty()
    }
}

// ---------------------------------------------------------------------------
// iCal 导出（VEVENT 最小面）
// ---------------------------------------------------------------------------

/// 导出一窗为 iCal VEVENT 行集（换行分隔）。缓冲不足返回写入数——
/// 调用方必须核对（诚实面：不许假装导完）。
pub fn ical_event(w: &ReleaseWindow, buf: &mut [u8]) -> usize {
    let kind = match w.kind {
        WindowKind::Quarterly => "VARIX quarterly release window",
        WindowKind::Monthly => "VARIX monthly fix window",
        WindowKind::EmergencySecurity => "VARIX emergency security release",
    };
    let lines = alloc::format!(
        "BEGIN:VEVENT\r\nSUMMARY:{kind}\r\nDUE_DAY:{due}\r\nANNOUNCED_DAY:{ann}\r\nEND:VEVENT\r\n",
        kind = kind,
        due = w.due_day,
        ann = w.announced_day,
    );
    let b = lines.as_bytes();
    let n = b.len().min(buf.len());
    buf[..n].copy_from_slice(&b[..n]);
    n
}

// ---------------------------------------------------------------------------
// 自检
// ---------------------------------------------------------------------------

pub const F138_TAG: &str = "stareco-F138-releasecal";

pub fn run_releasecal_checks() -> CheckSet {
    let mut set = CheckSet::new(F138_TAG);

    let mut cal = ReleaseCalendar::new();

    // 提前量门禁：普通窗 ≥30 天；带破坏性变更预告 ≥1 窗（90 天）
    let w1 = ReleaseWindow {
        kind: WindowKind::Quarterly,
        due_day: 1000,
        announced_day: 960,
        system_note: "s",
        upstream_note: "u",
        breaking_note: "",
    };
    let w2 = ReleaseWindow {
        kind: WindowKind::Quarterly,
        due_day: 2000,
        announced_day: 1880,
        system_note: "s",
        upstream_note: "wine",
        breaking_note: " breaking: vx_draw_rect v2",
    };
    let w_bad = ReleaseWindow { kind: WindowKind::Quarterly, due_day: 1000, announced_day: 980, system_note: "s", upstream_note: "u", breaking_note: "" };
    let w_bad_break = ReleaseWindow { kind: WindowKind::Quarterly, due_day: 2000, announced_day: 1950, system_note: "s", upstream_note: "u", breaking_note: "breaking" };
    set.add("f138 lead 40d ok", w1.announce_lead_ok() && cal.schedule(w1).is_ok(), ">=30d");
    set.add("f138 lead 120d with breaking ok", w2.announce_lead_ok() && cal.schedule(w2).is_ok(), ">=1 window");
    set.add("f138 lead 20d rejected", !w_bad.announce_lead_ok() && cal.schedule(w_bad).is_err(), "<30d");
    set.add(
        "f138 breaking with 50d rejected",
        !w_bad_break.announce_lead_ok() && cal.schedule(w_bad_break).is_err(),
        "breaking needs full window",
    );

    // 预告视图：未来两窗
    set.add("f138 two-window preview", cal.preview_count() == 2, "timeline");

    // 兑现/滑动归档：滑动必须带原因和更晚日期
    assert!(cal.close(1000, WindowOutcome::Shipped, 0, "").is_ok());
    set.add("f138 slip without reason rejected", cal.close(2000, WindowOutcome::Slipped, 2050, "").is_err(), "no silent slip");
    assert!(cal.close(2000, WindowOutcome::Slipped, 2060, "upstream regression").is_ok());
    set.add("f138 slip archived honestly", cal.slips == 1 && cal.archived_count() == 2, "1 ship + 1 slip");
    set.add("f138 archive append-only green", cal.archive_ok(), "chain intact");
    set.add("f138 slip date before due rejected", cal.close(2000, WindowOutcome::Slipped, 1990, "x").is_err(), "no time travel");

    // 判据主句：连续两窗「按日历兑现**或**如实归档偏差」——上两行
    // 正是 1 兑现 + 1 如实滑动 = 达标形态。
    set.add("f138 two-window criterion", cal.archived_count() == 2, "shipped-or-honest-slip");

    // 紧急安全窗：打破节奏合法但必须公告说明
    let emerg = ReleaseWindow {
        kind: WindowKind::EmergencySecurity,
        due_day: 1050,
        announced_day: 1045,
        system_note: "security fix off-cycle",
        upstream_note: "",
        breaking_note: "",
    };
    set.add("f138 emergency with note ok", cal.emergency_ok(&emerg), "explained off-cycle");
    let mut silent = emerg;
    silent.system_note = "";
    set.add("f138 silent emergency refused", !cal.emergency_ok(&silent), "no unexplained rhythm break");

    // iCal 导出
    let mut buf = [0u8; 256];
    let n = ical_event(&w1, &mut buf);
    let text = core::str::from_utf8(&buf[..n]).unwrap_or("");
    set.add(
        "f138 ical event shape",
        text.contains("BEGIN:VEVENT") && text.contains("SUMMARY:VARIX quarterly") && text.contains("END:VEVENT"),
        "vexport",
    );
    let mut tiny = [0u8; 8];
    let tn = ical_event(&w1, &mut tiny);
    set.add("f138 ical truncation honest", tn == 8 && tn < 256, "truncated returns real length");

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    fn win(due: u32, ann: u32) -> ReleaseWindow {
        ReleaseWindow {
            kind: WindowKind::Monthly,
            due_day: due,
            announced_day: ann,
            system_note: "s",
            upstream_note: "u",
            breaking_note: "",
        }
    }

    #[test]
    fn schedule_and_archive() {
        let mut cal = ReleaseCalendar::new();
        cal.schedule(win(100, 50)).unwrap();
        cal.schedule(win(200, 160)).unwrap();
        assert_eq!(cal.preview_count(), 2);
        cal.close(100, WindowOutcome::Shipped, 0, "").unwrap();
        assert!(cal.archive_ok());
    }
}
