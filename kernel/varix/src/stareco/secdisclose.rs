//! F142 安全披露通道 · 完整设计（STAR I 主册 G-D-17）。
//!
//! **判据（主册）**：security.txt 格式校验过；演练全流程（内部红队
//! 样本）时限达标；致谢页更新机制实测。
//!
//! **设计要点（主册）**：三件套（security.txt RFC 9116 / 报告邮箱
//! PGP 可用 / 致谢页）；处理时限承诺：48h 确认 / 90 天披露窗；披露
//! 窗 90 天可协商延长但需说明；安全修复随月度版发（安全例外 F138
//! 节奏条款）；致谢页排序按首个报告时间（匿名选项尊重）；重复报告
//! 合并致谢首报者；超时未修（90 天）→ 按承诺披露（对自家施压的
//! 宪法条款）；报告无效 → 礼貌说明+归类。
//!
//! 本模块是披露通道的**纯逻辑核**：security.txt 字段模型与 RFC 9116
//! 校验、报告接收与 48h 确认时限、披露窗倒计时与强制披露、致谢页
//! 排序。真签名走内核 ksha256/kvault（ebase 边界声明在册）。

use crate::checks::CheckSet;
use crate::stareco::ebase::SeqLedger;

// ---------------------------------------------------------------------------
// security.txt（RFC 9116）
// ---------------------------------------------------------------------------

/// RFC 9116 必需字段：Contact。推荐：Expires/Encryption/Policy/
/// Preferred-Languages。本实现按「Contact 必填 + Expires 必填 + 其余
/// 可选但值非空」校验。
pub struct SecurityTxt {
    pub contact: &'static str,
    pub expires_day: u32,
    pub encryption: Option<&'static str>,
    pub policy: Option<&'static str>,
    pub preferred_languages: Option<&'static str>,
}

impl SecurityTxt {
    /// RFC 9116 格式校验：Contact 必须是 URI（mailto:/https:// 开头）；
    /// Expires 必填（自校验日 365 天内）；可选字段一旦存在值非空。
    pub fn validate(&self, today: u32) -> Result<(), &'static str> {
        if !(self.contact.starts_with("mailto:") || self.contact.starts_with("https://")) {
            return Err("Contact 必须是 mailto: 或 https: URI");
        }
        if self.expires_day <= today {
            return Err("Expires 已过期（一年内须续签）");
        }
        if self.expires_day > today + 366 {
            return Err("Expires 超过一年（RFC 建议上限）");
        }
        for (name, v) in [
            ("Encryption", self.encryption),
            ("Policy", self.policy),
            ("Preferred-Languages", self.preferred_languages),
        ] {
            if let Some(s) = v {
                if s.is_empty() {
                    return Err(name);
                }
            }
        }
        Ok(())
    }

    /// 渲染（RFC 行格式 `Field: value`）。
    pub fn render(&self) -> alloc::vec::Vec<&'static str> {
        let mut lines = alloc::vec::Vec::new();
        lines.push("Contact");
        lines.push("Expires");
        if self.encryption.is_some() {
            lines.push("Encryption");
        }
        if self.policy.is_some() {
            lines.push("Policy");
        }
        if self.preferred_languages.is_some() {
            lines.push("Preferred-Languages");
        }
        lines
    }
}

// ---------------------------------------------------------------------------
// 报告与时限状态机
// ---------------------------------------------------------------------------

pub const CONFIRM_LIMIT_DAYS: u32 = 2; // 48h
pub const DISCLOSURE_WINDOW_DAYS: u32 = 90;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DiscloseState {
    Received,
    Confirmed,
    Fixed,
    /// 90 天窗满强制披露（宪法条款：对自家施压）。
    ForceDisclosed,
    /// 无效报告：礼貌说明+归类（不进披露窗）。
    InvalidClassified,
}

pub struct SecReport {
    pub state: DiscloseState,
    /// 接收日。
    pub received_day: u32,
    /// 确认日（0 = 未确认）。
    pub confirmed_day: u32,
    /// 披露窗满日 = received + 90（可协商延长，延长必须带说明）。
    pub window_end: u32,
    pub extension_note: Option<&'static str>,
    /// 首报者（合并报告致谢首报者）。
    pub first_reporter: &'static str,
}

impl SecReport {
    pub fn new(received_day: u32, reporter: &'static str) -> SecReport {
        SecReport {
            state: DiscloseState::Received,
            received_day,
            confirmed_day: 0,
            window_end: received_day + DISCLOSURE_WINDOW_DAYS,
            extension_note: None,
            first_reporter: reporter,
        }
    }

    /// 48h 确认时限（ebase::StateTrack 同口径的披露版）。
    pub fn confirm(&mut self, day: u32) -> Result<bool, &'static str> {
        if self.state != DiscloseState::Received {
            return Err("already routed");
        }
        self.state = DiscloseState::Confirmed;
        self.confirmed_day = day;
        Ok(day.saturating_sub(self.received_day) <= CONFIRM_LIMIT_DAYS)
    }

    /// 修复入册。
    pub fn fix(&mut self, day: u32) -> Result<(), &'static str> {
        if self.state != DiscloseState::Confirmed {
            return Err("must be confirmed first");
        }
        self.state = DiscloseState::Fixed;
        let _ = day;
        Ok(())
    }

    /// 披露窗延长：必须带说明（可协商但不许无声延长）。
    pub fn extend(&mut self, new_end: u32, note: &'static str) -> Result<(), &'static str> {
        if new_end <= self.window_end || note.is_empty() {
            return Err("extension needs later date and reason");
        }
        self.window_end = new_end;
        self.extension_note = Some(note);
        Ok(())
    }

    /// 到期判定：Fixed 且今日 > 窗满 → 强制披露（超时未修按承诺披露）。
    pub fn due_for_disclosure(&self, today: u32) -> bool {
        matches!(self.state, DiscloseState::Fixed | DiscloseState::Confirmed)
            && today > self.window_end
    }

    pub fn force_disclose(&mut self, today: u32) -> Result<(), &'static str> {
        if !self.due_for_disclosure(today) {
            return Err("window still open");
        }
        self.state = DiscloseState::ForceDisclosed;
        Ok(())
    }

    pub fn classify_invalid(&mut self) -> Result<(), &'static str> {
        if self.state != DiscloseState::Received {
            return Err("only fresh reports can be classified invalid");
        }
        self.state = DiscloseState::InvalidClassified;
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// 致谢页
// ---------------------------------------------------------------------------

/// 致谢条目：排序按首个报告时间（received_day 升序），匿名尊重。
pub struct Credit {
    pub reporter: &'static str, // "" = 匿名
    pub day: u32,
    pub kind: &'static str,
}

/// 致谢页构建：按日升序，匿名条目显示「匿名研究者」。
pub fn credits_page(mut list: alloc::vec::Vec<Credit>) -> alloc::vec::Vec<(usize, &'static str, u32)> {
    list.sort_by_key(|c| c.day);
    list.iter()
        .enumerate()
        .map(|(i, c)| {
            let name = if c.reporter.is_empty() { "匿名研究者" } else { c.reporter };
            (i + 1, name, c.day)
        })
        .collect()
}

/// 报告台账（内部册：不公开细节只公布时间线——append-only）。
pub struct SecLedger {
    inner: SeqLedger,
    pub count: u32,
}

impl SecLedger {
    pub const fn new() -> SecLedger {
        SecLedger { inner: SeqLedger::new(), count: 0 }
    }

    pub fn record(&mut self, payload_fp: u64) {
        self.inner.append(payload_fp);
        self.count += 1;
    }

    pub fn timeline_ok(&self) -> bool {
        self.inner.verify()
    }
}

// ---------------------------------------------------------------------------
// 自检
// ---------------------------------------------------------------------------

pub const F142_TAG: &str = "stareco-F142-secdisclose";

pub fn run_secdisclose_checks() -> CheckSet {
    let mut set = CheckSet::new(F142_TAG);
    let today = 20000;

    // security.txt 校验
    let ok = SecurityTxt {
        contact: "mailto:security@example.org",
        expires_day: today + 180,
        encryption: Some("https://example.org/pgp.txt"),
        policy: Some("https://example.org/policy"),
        preferred_languages: Some("zh, en"),
    };
    set.add("f142 security.txt valid", ok.validate(today).is_ok(), "RFC 9116");
    set.add(
        "f142 contact scheme enforced",
        SecurityTxt { contact: "ftp://x", ..ok }.validate(today).is_err(),
        "mailto/https only",
    );
    set.add(
        "f142 expired rejected",
        SecurityTxt { expires_day: today - 1, ..ok }.validate(today).is_err(),
        "stale expires",
    );
    set.add(
        "f142 over-1y rejected",
        SecurityTxt { expires_day: today + 400, ..ok }.validate(today).is_err(),
        "annual renewal",
    );
    set.add("f142 render fields", ok.render().len() == 5, "five lines");

    // 48h 确认时限
    let mut r = SecReport::new(100, "红队研究员");
    let on_time = r.confirm(102);
    let mut late = SecReport::new(100, "x");
    let _ = late.confirm(103);
    set.add("f142 48h confirm on time", on_time == Ok(true), "2 days");
    set.add(
        "f142 48h confirm late flagged",
        SecReport::new(100, "y").confirm(103) == Ok(false),
        "late still confirmed but flagged",
    );

    // 披露窗 90 天：到期强制披露；窗内拒绝
    assert!(r.fix(150).is_ok());
    set.add("f142 within window safe", !r.due_for_disclosure(100 + DISCLOSURE_WINDOW_DAYS), "boundary");
    set.add("f142 overdue forces disclosure", r.due_for_disclosure(100 + DISCLOSURE_WINDOW_DAYS + 1), "constitution clause");
    assert!(r.force_disclose(100 + DISCLOSURE_WINDOW_DAYS + 1).is_ok());
    set.add("f142 force disclosed state", r.state == DiscloseState::ForceDisclosed, "pressure kept");

    // 延长必须带说明
    let mut r2 = SecReport::new(100, "z");
    r2.confirm(101).ok();
    r2.fix(120).ok();
    set.add("f142 silent extension rejected", r2.extend(200, "").is_err(), "reason required");
    assert!(r2.extend(200, "complex fix in progress").is_ok());
    set.add("f142 extension noted", r2.extension_note == Some("complex fix in progress"), "transparent");

    // 无效报告归类
    let mut bad = SecReport::new(100, "w");
    bad.classify_invalid().ok();
    set.add(
        "f142 invalid classified politely",
        bad.state == DiscloseState::InvalidClassified && bad.due_for_disclosure(5000) == false,
        "no disclosure window",
    );
    set.add("f142 invalid cannot confirm", bad.confirm(101).is_err(), "routing law");

    // 致谢页：按首报时间排序 + 匿名尊重
    let page = credits_page(alloc::vec![
        Credit { reporter: "后来者", day: 300, kind: "report" },
        Credit { reporter: "", day: 100, kind: "report" },
        Credit { reporter: "首报者", day: 200, kind: "report" },
    ]);
    set.add(
        "f142 credits sorted by first report",
        page[0].1 == "匿名研究者" && page[1].1 == "首报者" && page[2].1 == "后来者",
        "anonymous respected",
    );

    // 内部台账（只公布时间线）
    let mut led = SecLedger::new();
    led.record(0xAA);
    led.record(0xBB);
    set.add("f142 internal ledger chain", led.timeline_ok() && led.count == 2, "append-only");

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn full_drill_timeline() {
        // 内部红队演练全流程：接收→确认(48h内)→修复→(协商延长)→披露
        let mut r = SecReport::new(0, "red-team");
        assert_eq!(r.confirm(1), Ok(true));
        r.fix(30).unwrap();
        r.extend(120, "coordinated").unwrap();
        assert!(!r.due_for_disclosure(120));
        assert!(r.due_for_disclosure(121));
        r.force_disclose(121).unwrap();
        assert_eq!(r.state, DiscloseState::ForceDisclosed);
    }
}
