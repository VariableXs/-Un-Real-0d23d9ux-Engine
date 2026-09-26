//! 深化层 · F572 应用崩溃简报（回炉补深主册【设计要点】未实装机制）。
//!
//! 补深三条（判据唯一源：主册 F572 节）：
//! ①「原因类别：内存不足/自身缺陷/系统资源紧张」的**归因规则表**——
//!   从失败信号签名到人话归因的判定唯一源（简报的三选一不是猜的，
//!   是签名→归因的规则表查出来的）；
//! ②「同类崩溃 24h 内重复发生时升级提示」的**升级去重账**——同一
//!   应用同一窗口期只升一次级（升级提示刷屏本身就是体验事故）；
//! ③简报**三要素合同**——为什么（人话原因）/ 东西还在吗（已恢复
//!   确认）/ 接下来能干嘛（反馈与更新出路）。基础层简报只有前两
//!   要素，出路要素由深化层补齐合成（主册「一键反馈预填 + 建议查看
//!   更新或反馈」的字面落地）。

use alloc::string::String;
use crate::checks::CheckSet;
use crate::istar::crashbrief::{CrashBrief, CrashCause, ESCALATE_AT};
use crate::istar::ibase::ISTAR_DOMAIN;

// ---------------------------------------------------------------------------
// 归因规则表（签名 → 人话归因）
// ---------------------------------------------------------------------------

/// 失败信号签名（崩溃现场可提取的判定材料）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FailSignal {
    /// 分配器拒绝大块请求 / OOM 击杀记录。
    AllocDenied,
    /// 非法地址访问 / 空指针解引用。
    BadPointer,
    /// 句柄/内存水位越过系统红线（整机资源紧张，不是应用一个人的错）。
    SystemWatermark,
}

/// 签名 → 归因（规则表唯一源）。
pub fn attribute(sig: FailSignal) -> CrashCause {
    match sig {
        FailSignal::AllocDenied => CrashCause::OutOfMemory,
        FailSignal::BadPointer => CrashCause::AppBug,
        FailSignal::SystemWatermark => CrashCause::ResourcePressure,
    }
}

/// 归因责任口径：系统资源紧张不算应用自己的错（简报不许甩锅）。
pub fn blame_app(cause: CrashCause) -> bool {
    !matches!(cause, CrashCause::ResourcePressure)
}

// ---------------------------------------------------------------------------
// 升级去重账
// ---------------------------------------------------------------------------

/// 升级事件（同应用同窗口只记一次）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Escalation {
    pub app_key: u64,
    pub window_start_ms: u64,
}

/// 升级去重账（容量 8；超容诚实拒绝新窗口——不丢旧账）。
pub struct EscalationLedger {
    entries: [Option<Escalation>; 8],
    len: usize,
}

impl EscalationLedger {
    pub fn new() -> EscalationLedger {
        EscalationLedger { entries: [None; 8], len: 0 }
    }

    /// 判定本次是否该升级：窗口内已升级过 → 不再升（去重）。
    pub fn should_escalate(&mut self, app_key: u64, window_start_ms: u64) -> bool {
        if self.entries[..self.len]
            .iter()
            .flatten()
            .any(|e| e.app_key == app_key && e.window_start_ms == window_start_ms)
        {
            return false;
        }
        if self.len < 8 {
            self.entries[self.len] = Some(Escalation { app_key, window_start_ms });
            self.len += 1;
            true
        } else {
            false
        }
    }

    pub fn len(&self) -> usize {
        self.len
    }
}

impl Default for EscalationLedger {
    fn default() -> Self {
        Self::new()
    }
}

/// app 名 → 稳定键（演示口径：字节和；真实层换 FNV——接口不变）。
pub fn app_key(name: &str) -> u64 {
    name.bytes().map(|b| b as u64).sum()
}

// ---------------------------------------------------------------------------
// 简报三要素合同（出路要素由深化层补齐）
// ---------------------------------------------------------------------------

/// 三要素齐检：人话原因 / 恢复确认 / 出路动作。
pub fn brief_complete(text: &str, recovery_confirmed: bool) -> bool {
    let has_plain_cause = text.contains("原因");
    let has_recovery = recovery_confirmed && text.contains("恢复");
    let has_next_step = text.contains("反馈") || text.contains("更新");
    has_plain_cause && has_recovery && has_next_step
}

/// 合成完整简报：基础层两要素 + 深化层补「接下来能干嘛」出路要素。
///
/// `escalated` = 该应用是否触发 24h 升级线（升级线触发则出路含「建议
/// 查看更新或反馈」——主册升级提示的字面落点）。
pub fn full_brief(base_text: &str, escalated: bool) -> String {
    if escalated {
        alloc::format!("{}；该应用今日已多次异常退出——建议查看更新或反馈", base_text)
    } else {
        alloc::format!("{}；一键反馈已预填崩溃哈希，可在「报告详情」展开技术栈", base_text)
    }
}

// ---------------------------------------------------------------------------
// 深化自检
// ---------------------------------------------------------------------------

pub fn run_f572_deep_checks() -> CheckSet {
    let mut cs = CheckSet::new(ISTAR_DOMAIN);

    // 1) 归因规则表：三类签名各归其位（主册三选一唯一源）。
    cs.add(
        "attribution table",
        attribute(FailSignal::AllocDenied) == CrashCause::OutOfMemory
            && attribute(FailSignal::BadPointer) == CrashCause::AppBug
            && attribute(FailSignal::SystemWatermark) == CrashCause::ResourcePressure,
        "",
    );

    // 2) 责任口径：资源紧张不甩锅应用（简报的价值观判定面）。
    cs.add(
        "no blame shifting",
        blame_app(CrashCause::AppBug) && !blame_app(CrashCause::ResourcePressure),
        "",
    );

    // 3) 升级去重：同应用同窗口第二次不再升级；跨窗口重新升。
    let mut led = EscalationLedger::new();
    let key = app_key("游戏");
    let first = led.should_escalate(key, 0);
    let dup = led.should_escalate(key, 0);
    let later = led.should_escalate(key, 24 * 3_600_000 + 1);
    cs.add("escalation dedup per window", first && !dup && later, "");

    // 4) 升级阈值联动：24h 内第 3 次才触发（基础层 ESCALATE_AT 对账）。
    let mut b = CrashBrief::new();
    for i in 0..ESCALATE_AT {
        b.record("游戏", CrashCause::AppBug, (i as u64) * 1_000, 0xABCD);
    }
    let esc = b.escalate("游戏", ESCALATE_AT as u64 * 1_000);
    cs.add("escalate at third crash", ESCALATE_AT == 3 && esc.is_some(), "");

    // 5) 三要素合同：基础简报（两要素）+ 深化出路要素 = 合格简报。
    let mut b2 = CrashBrief::new();
    b2.record("编辑器", CrashCause::OutOfMemory, 1_000, 777);
    let _ = b2.confirm_recovery("编辑器", true);
    let base_text = b2.brief_text("编辑器", CrashCause::OutOfMemory, true);
    let full = full_brief(&base_text, false);
    cs.add(
        "brief three elements via deep",
        !brief_complete(&base_text, true) && brief_complete(&full, b2.recovery_confirmed("编辑器")),
        "",
    );

    // 6) 升级线触发时的出路文案带「更新或反馈」（主册升级提示落点）：
    //    escalated=true 走升级文案路（文案合同与基础 escalate 计数解耦
    //    ——计数已由检查 4 与基础层自检覆盖）。
    let full2 = full_brief(&base_text, true);
    cs.add(
        "escalated brief suggests update",
        full2.contains("建议查看更新或反馈"),
        "",
    );

    // 7) 反馈预填带崩溃哈希（一键反馈的技术细节在详情折叠里）。
    let prefill = b2.feedback_prefill("编辑器");
    cs.add("feedback prefill carries hash", prefill.map(|s| s.contains("777")).unwrap_or(false), "");

    cs
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn escalation_ledger_capacity() {
        let mut led = EscalationLedger::new();
        for i in 0..10u64 {
            led.should_escalate(i, 0);
        }
        assert_eq!(led.len(), 8);
        assert!(!led.should_escalate(999, 0)); // 超容诚实拒绝
    }

    #[test]
    fn attribution_is_total() {
        // 三签名全覆盖（规则表完备性——没有查不出的签名）。
        for s in [FailSignal::AllocDenied, FailSignal::BadPointer, FailSignal::SystemWatermark] {
            let _ = attribute(s);
        }
    }
}
