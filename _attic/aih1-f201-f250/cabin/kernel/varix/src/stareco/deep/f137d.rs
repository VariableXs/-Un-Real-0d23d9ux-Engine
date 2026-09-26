//! 深化层 · F137 API 稳定性承诺（2026-09-26 回炉补深化）。
//!
//! 补深：破面通知三渠道、实验 API 首次调用日志提示、迁移指南模型、
//! 破面历史册（append-only）、实验级转正流程（考察期满→授予）。

use alloc::vec;
use crate::checks::CheckSet;
use crate::stareco::apistab::{signature_fp_of, ApiRegistry, Stability, MIGRATION_WINDOW_DAYS};
use crate::stareco::ebase::SeqLedger;

// ---------------------------------------------------------------------------
// 破面通知三渠道
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum NotifyChannel {
    Docs,
    RuntimeLog,
    StarmapAnnounce,
}

pub const NOTIFY_CHANNELS: [NotifyChannel; 3] = [
    NotifyChannel::Docs,
    NotifyChannel::RuntimeLog,
    NotifyChannel::StarmapAnnounce,
];

/// 破面通知单：三渠道必须全部送达（缺一 = 通知未完成）。
pub struct BreakNotice {
    pub symbol: &'static str,
    pub delivered: [bool; 3],
}

impl BreakNotice {
    pub fn complete(&self) -> bool {
        self.delivered.iter().all(|&b| b)
    }

    pub fn missing(&self) -> alloc::vec::Vec<&'static str> {
        self.delivered
            .iter()
            .zip(NOTIFY_CHANNELS.iter())
            .filter(|(d, _)| !**d)
            .map(|(_, c)| match c {
                NotifyChannel::Docs => "docs",
                NotifyChannel::RuntimeLog => "runtime-log",
                NotifyChannel::StarmapAnnounce => "starmap",
            })
            .collect()
    }
}

// ---------------------------------------------------------------------------
// 实验 API 首次调用日志提示（运行时面）
// ---------------------------------------------------------------------------

/// 每符号只提示一次（开发者可见不扰用户）。
pub struct FirstCallLog {
    warned: [Option<&'static str>; 16],
    count: usize,
}

impl FirstCallLog {
    pub fn new() -> FirstCallLog {
        FirstCallLog { warned: [None; 16], count: 0 }
    }

    /// 返回 Some(提示文案) = 首次调用需打日志；None = 已提示过。
    pub fn on_call(&mut self, symbol: &'static str) -> Option<&'static str> {
        if self.warned[..self.count].iter().any(|w| *w == Some(symbol)) {
            return None;
        }
        if self.count >= 16 {
            return None;
        }
        self.warned[self.count] = Some(symbol);
        self.count += 1;
        Some("提示：该 API 为实验级，签名可能变更——迁移指南见文档")
    }
}

// ---------------------------------------------------------------------------
// 破面历史册（append-only）
// ---------------------------------------------------------------------------

pub struct BreakHistory {
    ledger: SeqLedger,
    pub count: u32,
}

impl BreakHistory {
    pub const fn new() -> BreakHistory {
        BreakHistory { ledger: SeqLedger::new(), count: 0 }
    }

    pub fn record(&mut self, symbol: &str, old_fp: u64, new_fp: u64) {
        let payload = crate::stareco::ebase::fnv1a64(symbol.as_bytes())
            ^ crate::stareco::ebase::fnv1a64(&old_fp.to_le_bytes())
            ^ crate::stareco::ebase::fnv1a64(&new_fp.to_le_bytes());
        self.ledger.append(payload);
        self.count += 1;
    }

    pub fn intact(&self) -> bool {
        self.ledger.verify()
    }
}

// ---------------------------------------------------------------------------
// 实验级转正流程（考察期满 → 授予稳定级）
// ---------------------------------------------------------------------------

/// 转正三条件：满 90 天考察期 / 有页（文档四段齐）/ 破面历史零条。
pub fn promote_eligible(days_since_registered: u32, doc_complete: bool, breaks: u32) -> Result<(), &'static str> {
    if days_since_registered < 90 {
        return Err("考察期未满一季");
    }
    if !doc_complete {
        return Err("文档四段未齐：转正先补页");
    }
    if breaks > 0 {
        return Err("考察期内有破面：续考察一季");
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// 自检
// ---------------------------------------------------------------------------

pub const F137D_TAG: &str = "stareco-F137-deep";

pub fn run_f137_deep_checks() -> CheckSet {
    let mut set = CheckSet::new(F137D_TAG);

    // 三渠道通知
    let full = BreakNotice { symbol: "vx_draw_rect", delivered: [true, true, true] };
    set.add("f137d three channels complete", full.complete() && full.missing().is_empty(), "docs+log+starmap");
    let partial = BreakNotice { symbol: "vx_draw_rect", delivered: [true, false, false] };
    set.add(
        "f137d missing channels listed",
        partial.missing() == vec!["runtime-log", "starmap"],
        "缺一如实列出",
    );

    // 首次调用日志：一次有、二次无
    let mut log = FirstCallLog::new();
    let first = log.on_call("vx_beta_glow");
    let second = log.on_call("vx_beta_glow");
    set.add(
        "f137d first-call warns once",
        first.is_some() && second.is_none(),
        "不扰用户",
    );

    // 破面历史册
    let a = signature_fp_of(&["u32"]);
    let b = signature_fp_of(&["u64"]);
    let mut hist = BreakHistory::new();
    hist.record("f", a, b);
    hist.record("g", b, a);
    set.add("f137d break history intact", hist.intact() && hist.count == 2, "append-only");

    // 转正流程
    set.add(
        "f137d promote gate",
        promote_eligible(89, true, 0).is_err()
            && promote_eligible(90, false, 0).is_err()
            && promote_eligible(90, true, 1).is_err()
            && promote_eligible(90, true, 0).is_ok(),
        "三条件缺一不可",
    );

    // 注册表联动：实验自由变 → 转正（新符号注册稳定级）→ 门禁立即生效；
    // 重复符号注册拒绝（符号唯一性纪律）。
    let mut reg = ApiRegistry::new();
    reg.register("vx_beta", Stability::Experimental, a, 0).ok();
    assert_eq!(reg.ci_compare("vx_beta", b, false, 1), Ok(crate::stareco::apistab::CiVerdict::Compatible));
    set.add("f137d duplicate symbol rejected", reg.register("vx_beta", Stability::Stable, b, 200).is_err(), "符号唯一");
    let mut reg2 = ApiRegistry::new();
    reg2.register("vx_beta", Stability::Stable, b, 200).ok();
    let v = reg2.ci_compare("vx_beta", signature_fp_of(&["u128"]), false, 300);
    set.add(
        "f137d promoted api gated",
        v == Ok(crate::stareco::apistab::CiVerdict::UnauthorizedBreak) && reg2.blocked_breaks == 1,
        "转正即受门禁保护",
    );
    // 迁移窗常量与 F138 节奏一致性（一窗 = 90 天）
    set.add("f137d window matches season", MIGRATION_WINDOW_DAYS == 90, "一季迁移窗");

    set
}

#[cfg(test)]
mod deep_tests {
    use super::*;

    #[test]
    fn warn_once_only() {
        let mut log = FirstCallLog::new();
        assert!(log.on_call("a").is_some());
        assert!(log.on_call("a").is_none());
        assert!(log.on_call("b").is_some());
    }
}
