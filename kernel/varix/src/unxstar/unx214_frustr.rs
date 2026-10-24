//! UNX-F0214 · 体验事件式埋点（引导期挫败信号）
//!
//! 锚点判据（施工书原文）：UNX-F0214-J1 重试/降级/超时三类
//! "挫败指纹"自动标记为体验事件，事件带结论字段（十三端口径）。
//!
//! 设计：引导事件流 → 挫败指纹识别器（三类指纹规则：重试 ≥2 次、
//! 发生降级、单步超时）→ 自动升级为体验事件（带结论字段：
//! 挫败源 + 严重度）→ 十三端口径红线（事件必带结论，无结论事件拒）。
//!
//! 错误码段：0x2C60（全仓普查后独占分配，AI-WBX1022）。

use alloc::string::{String, ToString};
use alloc::vec::Vec;

/// 模块版本。
pub const F0214_VERSION: &str = "F0214-frustr-v1";

/// 诊断码段基址（UNX-F0214 独占）。
pub const SEG_BASE: u16 = 0x2C60;

/// 重试挫败阈值。
pub const RETRY_THRESHOLD: u64 = 2;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u16)]
pub enum E {
    /// 事件缺结论字段。
    NoConclusion = SEG_BASE + 1,
    /// 指纹漏标（应升级未升级）。
    MissedMark = SEG_BASE + 2,
    /// 未知事件类型。
    UnknownEvent = SEG_BASE + 3,
    /// 严重度越界。
    SeverityOut = SEG_BASE + 4,
    /// 升级重复（同一源重复升级）。
    DupEscalate = SEG_BASE + 5,
}

impl E {
    pub const fn code(self) -> u16 {
        self as u16
    }
}

/// 引导事件（原始流）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BootEvent {
    /// 重试（同一步第 n 次）。
    Retry { step: &'static str, nth: u64 },
    /// 降级（从主路径切后备）。
    Degrade { step: &'static str },
    /// 超时（单步耗时 ms）。
    Timeout { step: &'static str, ms: u64 },
    /// 正常步进。
    Ok { step: &'static str },
}

/// 挫败指纹类别。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FrustrKind {
    /// 重试类。
    Retry,
    /// 降级类。
    Degrade,
    /// 超时类。
    Timeout,
}

/// 体验事件（十三端口径：必带结论）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExperienceEvent {
    /// 挫败类别。
    pub kind: FrustrKind,
    /// 挫败源（步名）。
    pub source: String,
    /// 结论字段（必非空）。
    pub conclusion: String,
    /// 严重度 1~5。
    pub severity: u8,
}

/// 识别器。
#[derive(Debug, Default)]
pub struct FrustrDetector {
    /// 体验事件流。
    pub events: Vec<ExperienceEvent>,
    /// 已升级源（防重复升级）。
    escalated: Vec<String>,
}

impl FrustrDetector {
    /// 喂事件：命中指纹即自动升级为体验事件。
    pub fn feed(&mut self, ev: BootEvent) -> Result<bool, u16> {
        let hit: Option<(FrustrKind, &str, u8)> = match ev {
            BootEvent::Retry { step, nth } => {
                if nth >= RETRY_THRESHOLD {
                    Some((FrustrKind::Retry, step, 3))
                } else {
                    None
                }
            }
            BootEvent::Degrade { step } => Some((FrustrKind::Degrade, step, 2)),
            BootEvent::Timeout { step, ms } => {
                if ms > 0 {
                    // 严重度随时长抬升（1s 以下 3 级，以上 4 级）。
                    Some((FrustrKind::Timeout, step, if ms >= 1000 { 4 } else { 3 }))
                } else {
                    None
                }
            }
            BootEvent::Ok { .. } => None,
        };
        let Some((kind, source, sev)) = hit else {
            return Ok(false);
        };
        if sev < 1 || sev > 5 {
            return Err(E::SeverityOut.code());
        }
        let src = String::from(source);
        if self.escalated.contains(&src) {
            return Err(E::DupEscalate.code());
        }
        self.escalated.push(src.clone());
        // 结论字段按类别模板生成（必非空）。
        let conclusion = match kind {
            FrustrKind::Retry => String::from("重试链触发：该步连续多次未过，怀疑介质或链路抖动"),
            FrustrKind::Degrade => String::from("主路径放弃：已切后备路径，功能保全但性能预期下降"),
            FrustrKind::Timeout => String::from("单步超时：该步等待超预算，后续步骤顺延"),
        };
        self.events.push(ExperienceEvent {
            kind,
            source: src,
            conclusion,
            severity: sev,
        });
        Ok(true)
    }

    /// 十三端口径红线：全部事件必带非空结论。
    pub fn conclusion_audit(&self) -> Result<(), u16> {
        if self.events.iter().any(|e| e.conclusion.is_empty()) {
            return Err(E::NoConclusion.code());
        }
        Ok(())
    }

    /// 指纹覆盖红线：三类指纹各至少一例升级。
    pub fn kind_coverage(&self) -> Result<(), u16> {
        let has = |k: FrustrKind| self.events.iter().any(|e| e.kind == k);
        if !has(FrustrKind::Retry) || !has(FrustrKind::Degrade) || !has(FrustrKind::Timeout) {
            return Err(E::MissedMark.code());
        }
        Ok(())
    }
}

/// J1 自证（判据原文：三类挫败指纹自动标记，事件带结论字段）。
pub fn self_audit() -> Result<(), u16> {
    // 一、夹具：正常流 + 三类挫败各一例。
    let mut d = FrustrDetector::default();
    if d.feed(BootEvent::Ok { step: "fw-handoff" })? {
        return Err(E::MissedMark.code());
    }
    if !d.feed(BootEvent::Retry { step: "disk-read", nth: 3 })? {
        return Err(E::MissedMark.code());
    }
    if !d.feed(BootEvent::Degrade { step: "gfx-mode" })? {
        return Err(E::MissedMark.code());
    }
    if !d.feed(BootEvent::Timeout { step: "ucode-load", ms: 1200 })? {
        return Err(E::MissedMark.code());
    }

    // 二、J1 主体：三类全覆盖 + 结论全非空 + 严重度落界。
    d.kind_coverage()?;
    d.conclusion_audit()?;
    for e in d.events.iter() {
        if e.severity < 1 || e.severity > 5 {
            return Err(E::SeverityOut.code());
        }
    }

    // 三、指纹阈值：单次重试不升级（阈值 2）。
    let mut d2 = FrustrDetector::default();
    if d2.feed(BootEvent::Retry { step: "disk-read", nth: 1 })? {
        return Err(E::MissedMark.code());
    }
    if !d2.events.is_empty() {
        return Err(E::MissedMark.code());
    }

    // 四、红线逐项：同源重复升级拒 / 严重度越界无入口（模板内建）。
    if d.feed(BootEvent::Retry { step: "disk-read", nth: 5 }).is_ok() {
        return Err(E::DupEscalate.code());
    }

    // 五、判别式段位对拍。
    if E::NoConclusion.code() != SEG_BASE + 1 || E::DupEscalate.code() != SEG_BASE + 5 {
        return Err(E::NoConclusion.code());
    }
    Ok(())
}
