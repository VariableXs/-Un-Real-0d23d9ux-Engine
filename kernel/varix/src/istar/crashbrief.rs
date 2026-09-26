//! F572 应用崩溃简报 · 完整设计（STAR I 主册 I 域批次七）。
//!
//! **判据（主册）**：三类归因；恢复联动确认；详情展开；反馈预填；
//! 24h 计数升级。
//!
//! **设计要点（主册）**：
//! - 应用崩溃后的用户面：恢复条（F311 恢复文档）旁附简报卡——原因三选一
//!   人话（内存不足/自身缺陷/系统资源紧张）+「已自动恢复你的文档」确认；
//! - 「报告详情」展开技术栈（给排障）；一键反馈（F139 通道预填崩溃哈希）；
//! - 同类崩溃 24h 内重复发生时升级提示；
//! - 简报不是甩锅（不说「你的操作有问题」）。

use crate::checks::CheckSet;
use crate::istar::ibase::ISTAR_DOMAIN;

use alloc::string::String;
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 规格常量
// ---------------------------------------------------------------------------

/// 三类归因（枚举即清单——归因不设第四类「用户操作问题」：简报不甩锅）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CrashCause {
    /// 内存不足。
    OutOfMemory,
    /// 自身缺陷。
    AppBug,
    /// 系统资源紧张。
    ResourcePressure,
}

impl CrashCause {
    /// 人话文案（三要素之一：发生了什么）。
    pub fn plain(self) -> &'static str {
        match self {
            CrashCause::OutOfMemory => "内存不足",
            CrashCause::AppBug => "应用自身缺陷",
            CrashCause::ResourcePressure => "系统资源紧张",
        }
    }
}

/// 升级线：同类崩溃 24h 内第 N 次触发升级提示。
pub const ESCALATE_AT: u32 = 3;

/// 24h 窗口（ms）。
pub const ESCALATE_WINDOW_MS: u64 = 24 * 3_600_000;

// ---------------------------------------------------------------------------
// 模型
// ---------------------------------------------------------------------------

/// 一条崩溃事件。
#[derive(Clone, Debug)]
pub struct CrashEvent {
    pub app: String,
    pub cause: CrashCause,
    pub ms: u64,
    /// 崩溃哈希（反馈预填 F139；详情展开的技术栈指纹）。
    pub hash: u32,
}

/// 崩溃简报引擎。
pub struct CrashBrief {
    events: Vec<CrashEvent>,
    /// 文档恢复确认账（「已自动恢复你的文档」）。
    recovered_confirmed: Vec<(String, bool)>,
}

impl CrashBrief {
    pub fn new() -> CrashBrief {
        CrashBrief {
            events: Vec::new(),
            recovered_confirmed: Vec::new(),
        }
    }

    /// 记一次崩溃。
    pub fn record(&mut self, app: &str, cause: CrashCause, ms: u64, hash: u32) {
        self.events.push(CrashEvent {
            app: String::from(app),
            cause,
            ms,
            hash,
        });
    }

    /// 简报卡主文案（三要素：什么崩了/为什么人话/文档已恢复）。
    pub fn brief_text(&self, app: &str, cause: CrashCause, doc_recovered: bool) -> String {
        let rec = if doc_recovered {
            "已自动恢复你的文档"
        } else {
            "未检测到未保存文档"
        };
        alloc::format!("{} 刚才异常退出——原因类别：{}；{}", app, cause.plain(), rec)
    }

    /// 恢复联动确认：用户点确认后入账（恢复条与简报卡的握手）。
    pub fn confirm_recovery(&mut self, app: &str, docs_ok: bool) {
        self.recovered_confirmed
            .push((String::from(app), docs_ok));
    }

    pub fn recovery_confirmed(&self, app: &str) -> bool {
        self.recovered_confirmed
            .iter()
            .any(|(a, _)| a == app)
    }

    /// 详情展开：技术栈指纹（崩溃哈希）——给排障，不在主文案。
    pub fn detail_hash(&self, app: &str) -> Option<u32> {
        self.events
            .iter()
            .rev()
            .find(|e| e.app == app)
            .map(|e| e.hash)
    }

    /// 反馈预填串（F139 通道）：哈希 + 归因 + 时间（脱敏——不含用户内容）。
    pub fn feedback_prefill(&self, app: &str) -> Option<String> {
        let e = self.events.iter().rev().find(|e| e.app == app)?;
        Some(alloc::format!("crash:{}:{}:{}", app, e.cause.plain(), e.hash))
    }

    /// 24h 计数：同类（同应用）在窗口内的崩溃次数。
    pub fn count_in_24h(&self, app: &str, now_ms: u64) -> u32 {
        self.events
            .iter()
            .filter(|e| e.app == app && now_ms.saturating_sub(e.ms) <= ESCALATE_WINDOW_MS)
            .count() as u32
    }

    /// 升级判定：24h 内同类崩溃 ≥ ESCALATE_AT → 升级提示。
    pub fn escalate(&self, app: &str, now_ms: u64) -> Option<String> {
        if self.count_in_24h(app, now_ms) >= ESCALATE_AT {
            Some(alloc::format!(
                "该应用今日已崩溃 {} 次——建议查看更新或反馈",
                self.count_in_24h(app, now_ms)
            ))
        } else {
            None
        }
    }

    /// 事件总数。
    pub fn total(&self) -> usize {
        self.events.len()
    }
}

impl Default for CrashBrief {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// 自检
// ---------------------------------------------------------------------------

pub fn run_crashbrief_checks() -> CheckSet {
    let mut set = CheckSet::new(ISTAR_DOMAIN);

    // 1. 三类归因人话齐备（枚举即清单，无第四类甩锅项）。
    set.add(
        "three causes plain text",
        !CrashCause::OutOfMemory.plain().is_empty()
            && !CrashCause::AppBug.plain().is_empty()
            && !CrashCause::ResourcePressure.plain().is_empty(),
        "",
    );

    // 2. 简报卡三要素：应用名 + 人话归因 + 文档恢复确认。
    let mut b = CrashBrief::new();
    b.record("记事本", CrashCause::OutOfMemory, 1_000, 0xAB12);
    let text = b.brief_text("记事本", CrashCause::OutOfMemory, true);
    set.add(
        "brief three elements",
        text.contains("记事本") && text.contains("内存不足") && text.contains("已自动恢复你的文档"),
        "",
    );

    // 3. 无未保存文档态：文案诚实（不说「已恢复」）。
    let no_doc = b.brief_text("记事本", CrashCause::AppBug, false);
    set.add(
        "no doc honest text",
        no_doc.contains("未检测到未保存文档"),
        "",
    );

    // 4. 恢复联动确认：确认入账可查。
    b.confirm_recovery("记事本", true);
    set.add("recovery confirm handshake", b.recovery_confirmed("记事本"), "");

    // 5. 详情展开：技术栈哈希可得（与主文案分离——收得起放得出）。
    let hash = b.detail_hash("记事本");
    set.add("detail hash available", hash == Some(0xAB12), "");

    // 6. 反馈预填：哈希+归因入串（F139 预填格式）。
    let pre = b.feedback_prefill("记事本");
    set.add(
        "feedback prefill",
        pre.as_ref().map(|s| s.contains("0xab12") || s.contains("43794")).unwrap_or(false)
            && pre.as_ref().map(|s| s.contains("内存不足")).unwrap_or(false),
        "",
    );

    // 7. 24h 计数升级：第三次触发升级提示；窗外的不计数。
    let mut b2 = CrashBrief::new();
    b2.record("游戏", CrashCause::AppBug, 0, 1);
    b2.record("游戏", CrashCause::AppBug, 3_600_000, 1);
    let two = b2.escalate("游戏", 7_200_000);
    b2.record("游戏", CrashCause::AppBug, 7_200_000, 1);
    let three = b2.escalate("游戏", 7_200_000);
    // 25 小时后：窗外事件全部出窗——计数归零不升级。
    let next_day = b2.escalate("游戏", 25 * 3_600_000 + 10);
    set.add(
        "24h escalation at third crash",
        two.is_none() && three.is_some() && next_day.is_none(),
        "",
    );

    // 8. 不同应用分账（同窗其他应用崩溃不并进升级计数）。
    b2.record("浏览器", CrashCause::AppBug, 7_300_000, 2);
    set.add(
        "per app counting",
        b2.count_in_24h("游戏", 7_300_000) == 3 && b2.total() == 4,
        "",
    );

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detail_returns_latest_hash() {
        let mut b = CrashBrief::new();
        b.record("x", CrashCause::AppBug, 0, 111);
        b.record("x", CrashCause::AppBug, 5, 222);
        assert_eq!(b.detail_hash("x"), Some(222));
    }

    #[test]
    fn unknown_app_no_detail() {
        let b = CrashBrief::new();
        assert!(b.detail_hash("无").is_none());
        assert!(b.feedback_prefill("无").is_none());
    }

    #[test]
    fn escalation_counts_only_same_app() {
        let mut b = CrashBrief::new();
        for i in 0..3u32 {
            b.record("a", CrashCause::ResourcePressure, i as u64, i);
            b.record("b", CrashCause::ResourcePressure, i as u64, i);
        }
        assert!(b.escalate("a", 1_000).is_some());
        assert!(b.escalate("b", 1_000).is_some());
        assert!(b.escalate("c", 1_000).is_none());
    }
}
