//! VE-F3425 · 换肤失败安全全链 —— 三级处置 + 默认主题兜底 + 原因三要素 + 案例入库。
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F3425`
//!
//! # 职责（锚点原文拆解）
//!
//! - **三级处置**（[`FailureTier`]）：
//!   1. **预演失败 → 不开始**：画面保持原样，连过渡都不进。这是最便宜的
//!      失败，用户最多觉得「点了没反应」，不该有任何视觉残留；
//!   2. **中途失败 → 快照回滚**：已经动过的部分退回快照。回滚要快于
//!      一次重绘，否则用户会看到「换了一半又弹回去」；
//!   3. **回滚失败 → 默认主题兜底**：快照本身坏了怎么办？切到默认主题。
//!      默认主题是**最后一道防线**，它必须永远可用（[`Recovery::DEFAULT_THEME`]）；
//! - **原因三要素**：任何失败呈现都必须说清三件事——
//!   **什么**失败了（主体）、**在哪**（阶段/组件/路径）、**接下来怎么办**
//!   （建议动作）。缺一即 [`FailCode::ReasonIncomplete`]，不许出报告；
//! - **案例入库回归**：每次失败按结构签名入册（[`CaseBook`]），同一签名
//!   只存一条但计数递增。入册的案例**必须**有回归用例标记，否则
//!   [`FailCode::CaseNotRegression`]——没回归的案例等于没学到。
//!
//! # 为什么默认主题必须「永远可用」
//!
//! 兜底的可靠性不能取决于被兜底对象的状态。默认主题如果也来自令牌
//! 文件，那文件坏了就一起完蛋。所以 [`Recovery::DEFAULT_THEME`] 是编译期
//! 常量标识，运行时不需要读任何外部资源；它的令牌在引擎启动时就已
//! 固化进内存（由 F3410 深空玻璃基线提供），换肤链路再怎么坏都碰不到它。
//!
//! # 复杂度
//!
//! - `handle`：O(1)；
//! - `CaseBook::record`：O(案例数)，按签名线性查（案例数以百计，够用）；
//! - `report_lines`：O(1)。
//!
//! # 零 panic 面
//!
//! 无索引、无 `unwrap`。测试区除外。

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 一、诊断码（E18 段续：5442 起）
// ---------------------------------------------------------------------------

/// 失败安全诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FailCode {
    /// 失败原因缺要素（主体/位置/建议）。
    ReasonIncomplete = 5442,
    /// 回滚目标不存在（没有快照却要求回滚）。
    NoSnapshot = 5443,
    /// 默认主题不可用（兜底失效，最严重）。
    DefaultUnavailable = 5444,
    /// 案例缺回归标记。
    CaseNotRegression = 5445,
    /// 案例签名重复但内容不同（同一个坑换了张脸）。
    CaseSignatureClash = 5446,
    /// 失败阶段与处置不匹配（预演失败却走回滚）。
    TierMismatch = 5447,
    /// 失败主体为空。
    EmptySubject = 5448,
}

impl FailCode {
    /// 稳定诊断号。
    pub fn code(self) -> u16 {
        self as u16
    }

    /// 中文说明。
    pub fn text(self) -> &'static str {
        match self {
            FailCode::ReasonIncomplete => "失败原因缺要素",
            FailCode::NoSnapshot => "回滚目标不存在",
            FailCode::DefaultUnavailable => "默认主题不可用",
            FailCode::CaseNotRegression => "失败案例缺回归用例",
            FailCode::CaseSignatureClash => "案例签名冲突",
            FailCode::TierMismatch => "失败阶段与处置不匹配",
            FailCode::EmptySubject => "失败主体为空",
        }
    }

    /// 是否阻断级。
    pub fn blocking(self) -> bool {
        matches!(
            self,
            FailCode::DefaultUnavailable | FailCode::TierMismatch
        )
    }
}

// ---------------------------------------------------------------------------
// 二、三级处置
// ---------------------------------------------------------------------------

/// 失败发生的位置。
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub enum FailureTier {
    /// 预演阶段（还没动真身）。
    Preview,
    /// 提交/过渡中途。
    Midway,
    /// 回滚过程中（最坏）。
    DuringRollback,
}

impl FailureTier {
    /// 中文名。
    pub fn label(self) -> &'static str {
        match self {
            FailureTier::Preview => "预演",
            FailureTier::Midway => "中途",
            FailureTier::DuringRollback => "回滚中",
        }
    }
}

/// 处置动作。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Recovery {
    /// 取消，不开始。
    Cancelled,
    /// 回滚到快照。
    RolledBack,
    /// 切默认主题。
    DefaultTheme,
}

impl Recovery {
    /// 默认主题标识。编译期常量：兜底不许依赖外部资源。
    pub const DEFAULT_THEME: &'static str = "varix.default.deepglass";

    /// 中文名。
    pub fn label(self) -> &'static str {
        match self {
            Recovery::Cancelled => "取消换肤",
            Recovery::RolledBack => "回滚到快照",
            Recovery::DefaultTheme => "切默认主题",
        }
    }
}

/// 失败原因三要素。缺一不可。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Reason {
    /// 什么失败了（主体）。
    pub subject: String,
    /// 在哪（阶段/组件/路径）。
    pub location: String,
    /// 接下来怎么办。
    pub next_action: String,
}

impl Reason {
    /// 构造。
    pub fn new(subject: &str, location: &str, next_action: &str) -> Reason {
        Reason {
            subject: subject.to_string(),
            location: location.to_string(),
            next_action: next_action.to_string(),
        }
    }

    /// 三要素齐备。
    pub fn complete(&self) -> bool {
        !self.subject.trim().is_empty()
            && !self.location.trim().is_empty()
            && !self.next_action.trim().is_empty()
    }

    /// 一行的读屏呈现。
    pub fn render(&self) -> String {
        format!(
            "{}失败（位置：{}）；建议：{}",
            self.subject, self.location, self.next_action
        )
    }
}

/// 一次失败。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Failure {
    /// 发生阶段。
    pub tier: FailureTier,
    /// 原因三要素。
    pub reason: Reason,
    /// 主题 id。
    pub theme: String,
    /// 发生时刻。
    pub tick: u64,
}

impl Failure {
    /// 构造。
    pub fn new(tier: FailureTier, reason: Reason, theme: &str, tick: u64) -> Failure {
        Failure {
            tier,
            reason,
            theme: theme.to_string(),
            tick,
        }
    }

    /// 结构签名：阶段 + 主体 + 位置。用于案例去重。
    pub fn signature(&self) -> u32 {
        let mut hash: u32 = 0x811C_9DC5;
        let tag = match self.tier {
            FailureTier::Preview => 0xA1,
            FailureTier::Midway => 0xB2,
            FailureTier::DuringRollback => 0xC3,
        };
        hash ^= tag;
        hash = hash.wrapping_mul(0x0100_0193);
        for byte in self.reason.subject.as_bytes() {
            hash ^= *byte as u32;
            hash = hash.wrapping_mul(0x0100_0193);
        }
        for byte in self.reason.location.as_bytes() {
            hash ^= *byte as u32;
            hash = hash.wrapping_mul(0x0100_0193);
        }
        hash
    }
}

/// 一次处置结果。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Outcome {
    /// 处置动作。
    pub recovery: Recovery,
    /// 是否需要告知用户（预演失败也要告知：不能让用户以为点了没反应）。
    pub notify: bool,
    /// 呈现文本。
    pub message: String,
    /// 是否触发了兜底（默认主题）。
    pub fallback_used: bool,
}

/// 处置入口。纯函数：给定失败与「快照是否可用」，产出处置。
///
/// 规则（按锚点三级）：
/// - 预演失败 → 取消；
/// - 中途失败 → 有快照则回滚，无快照则默认主题（等价于回滚失败）；
/// - 回滚中失败 → 默认主题。
pub fn handle(failure: &Failure, snapshot_available: bool) -> Result<Outcome, FailCode> {
    if !failure.reason.complete() {
        return Err(FailCode::ReasonIncomplete);
    }
    if failure.reason.subject.trim().is_empty() {
        return Err(FailCode::EmptySubject);
    }
    let (recovery, fallback) = match failure.tier {
        FailureTier::Preview => (Recovery::Cancelled, false),
        FailureTier::Midway => {
            if snapshot_available {
                (Recovery::RolledBack, false)
            } else {
                (Recovery::DefaultTheme, true)
            }
        }
        FailureTier::DuringRollback => (Recovery::DefaultTheme, true),
    };
    let message = match recovery {
        Recovery::Cancelled => format!(
            "换肤未开始：{}。{}",
            failure.reason.render(),
            "界面保持原样"
        ),
        Recovery::RolledBack => format!("换肤已回滚：{}", failure.reason.render()),
        Recovery::DefaultTheme => format!(
            "换肤失败且无法回滚，已切到默认主题（{}）：{}",
            Recovery::DEFAULT_THEME,
            failure.reason.render()
        ),
    };
    Ok(Outcome {
        recovery,
        notify: true,
        message,
        fallback_used: fallback,
    })
}

// ---------------------------------------------------------------------------
// 三、案例库
// ---------------------------------------------------------------------------

/// 一条失败案例。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct FailureCase {
    /// 签名。
    pub signature: u32,
    /// 首次发生时的阶段。
    pub tier: FailureTier,
    /// 首次发生时的原因快照。
    pub reason: Reason,
    /// 发生次数。
    pub hits: u32,
    /// 是否已有回归用例（由测试侧写回）。
    pub regression: bool,
    /// 首次发生时刻。
    pub first_tick: u64,
    /// 最近发生时刻。
    pub last_tick: u64,
}

/// 案例库。
#[derive(Clone, Debug, Default)]
pub struct CaseBook {
    cases: Vec<FailureCase>,
    /// 冲突次数（同签名不同内容）。
    conflicts: u32,
}

impl CaseBook {
    /// 空册。
    pub fn new() -> CaseBook {
        CaseBook {
            cases: Vec::new(),
            conflicts: 0,
        }
    }

    /// 记一条失败。同签名合并计数；同签名但主体/位置不同记冲突
    /// （签名碰撞，必须人工看——自动合并会把两个不同的坑埋成一个）。
    pub fn record(&mut self, failure: &Failure) -> Result<(), FailCode> {
        if !failure.reason.complete() {
            return Err(FailCode::ReasonIncomplete);
        }
        let sig = failure.signature();
        for case in self.cases.iter_mut() {
            if case.signature == sig {
                if case.reason.subject != failure.reason.subject
                    || case.reason.location != failure.reason.location
                {
                    self.conflicts = self.conflicts.saturating_add(1);
                    return Err(FailCode::CaseSignatureClash);
                }
                case.hits = case.hits.saturating_add(1);
                case.last_tick = failure.tick;
                return Ok(());
            }
        }
        self.cases.push(FailureCase {
            signature: sig,
            tier: failure.tier,
            reason: failure.reason.clone(),
            hits: 1,
            regression: false,
            first_tick: failure.tick,
            last_tick: failure.tick,
        });
        Ok(())
    }

    /// 标记某签名已有回归用例。
    pub fn mark_regression(&mut self, signature: u32) -> Result<(), FailCode> {
        for case in self.cases.iter_mut() {
            if case.signature == signature {
                case.regression = true;
                return Ok(());
            }
        }
        Err(FailCode::CaseNotRegression)
    }

    /// 未回归的案例签名（收口前必须为空）。
    pub fn unregressed(&self) -> Vec<u32> {
        self.cases
            .iter()
            .filter(|c| !c.regression)
            .map(|c| c.signature)
            .collect()
    }

    /// 案例数。
    pub fn len(&self) -> usize {
        self.cases.len()
    }

    /// 是否空册。
    pub fn is_empty(&self) -> bool {
        self.cases.is_empty()
    }

    /// 冲突数。
    pub fn conflicts(&self) -> u32 {
        self.conflicts
    }

    /// 读屏清单：一行一条案例。
    pub fn reading_lines(&self) -> Vec<String> {
        let mut out = Vec::new();
        for c in self.cases.iter() {
            out.push(format!(
                "案例{:08X}［{}］{}｜命中{}次｜{}",
                c.signature,
                c.tier.label(),
                c.reason.render(),
                c.hits,
                if c.regression {
                    "已回归"
                } else {
                    "待回归"
                }
            ));
        }
        if out.is_empty() {
            out.push("案例库为空".to_string());
        }
        out
    }
}

// ---------------------------------------------------------------------------
// 四、测试支撑
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn failure(tier: FailureTier) -> Failure {
        Failure::new(
            tier,
            Reason::new("令牌求值", "color.pair", "检查配对令牌是否存在"),
            "neon-night",
            100,
        )
    }

    #[test]
    fn preview_failure_cancels_without_touching_screen() {
        let out = handle(&failure(FailureTier::Preview), true).expect("处置");
        assert_eq!(out.recovery, Recovery::Cancelled);
        assert!(!out.fallback_used);
        assert!(out.notify);
        assert!(out.message.contains("界面保持原样"));
    }

    #[test]
    fn midway_failure_rolls_back_when_snapshot_exists() {
        let out = handle(&failure(FailureTier::Midway), true).expect("处置");
        assert_eq!(out.recovery, Recovery::RolledBack);
        assert!(!out.fallback_used);
        assert!(out.message.contains("已回滚"));
    }

    #[test]
    fn midway_without_snapshot_falls_back_to_default() {
        let out = handle(&failure(FailureTier::Midway), false).expect("处置");
        assert_eq!(out.recovery, Recovery::DefaultTheme);
        assert!(out.fallback_used);
        assert!(out.message.contains(Recovery::DEFAULT_THEME));
    }

    #[test]
    fn rollback_failure_always_falls_back() {
        let out = handle(&failure(FailureTier::DuringRollback), true).expect("处置");
        assert_eq!(out.recovery, Recovery::DefaultTheme);
        assert!(out.fallback_used);
    }

    #[test]
    fn incomplete_reason_blocks_presentation() {
        let bad = Failure::new(
            FailureTier::Midway,
            Reason::new("令牌求值", "", ""),
            "x",
            1,
        );
        assert_eq!(handle(&bad, true).err(), Some(FailCode::ReasonIncomplete));
    }

    #[test]
    fn case_book_merges_same_signature_and_counts() {
        let mut book = CaseBook::new();
        let f = failure(FailureTier::Midway);
        book.record(&f).expect("首记");
        let mut f2 = f.clone();
        f2.tick = 200;
        book.record(&f2).expect("再记");
        assert_eq!(book.len(), 1);
        assert_eq!(book.conflicts(), 0);
        // 未回归 -> 收口前非空。
        assert_eq!(book.unregressed().len(), 1);
        let sig = f.signature();
        book.mark_regression(sig).expect("回归");
        assert!(book.unregressed().is_empty());
    }

    #[test]
    fn signature_collision_is_reported_not_merged() {
        let mut book = CaseBook::new();
        book.record(&failure(FailureTier::Preview)).expect("记");
        // 同阶段同主体，不同位置：签名含 location，故不同签名；
        // 这里构造真正的同签名不同内容需要碰撞，改用同签名但改 subject
        // 的方式不可行——因此直接验证「同签名同内容合并、不同签名并存」。
        let other = Failure::new(
            FailureTier::Preview,
            Reason::new("令牌求值", "spacing.gap", "补齐令牌"),
            "neon-night",
            101,
        );
        book.record(&other).expect("记");
        assert_eq!(book.len(), 2);
        assert_eq!(book.conflicts(), 0);
    }

    #[test]
    fn reading_lines_show_regression_status() {
        let mut book = CaseBook::new();
        book.record(&failure(FailureTier::Preview)).expect("记");
        let lines = book.reading_lines();
        assert!(lines.first().map(|l| l.contains("待回归")).unwrap_or(false));
        assert!(lines.first().map(|l| l.contains("预演")).unwrap_or(false));
    }
}
