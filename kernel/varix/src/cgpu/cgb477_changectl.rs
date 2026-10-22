//! CGPU-F0477 · 变更纪律（目标 360 行）
//! ==========================================================================
//! 锚点判据（施工书原文摘录）：
//! - 变更纪律：C 域接口变更 ADR 流程（v1.0 十年承诺下的变更门槛）；
//! - 预算语义变更纪律（守恒等不变量语义变更=域级 ADR+全域同步）；
//! - 基线变更纪律；生态插件契约的两版过渡纪律。判据：四纪律、判据。
//!
//! 设计纪律：
//! - 四纪律各自状态机：接口变更（提案→评审→双签→发布，门槛=影响
//!   面声明+十年承诺豁免理由）、语义变更（域级 ADR+全域同步确认，
//!   缺同步确认不放行）、基线变更（解冻→改→再冻结 ADR 链）、插件
//!   契约（新旧两版并存期=过渡窗，窗内双版本可查、窗外旧版下线）。
//! - 十年承诺门槛：v1.0 冻结签名变更必须携带「不可豁免理由」字段
//!   ——没有理由的变更加载即拒。
//! - 过渡窗有界：两版并存窗长常量，超窗旧版强制下线（防止永久
//!   双版本）。
//! - no_std + alloc；零 unwrap/expect 于运行路径；诊断段 0x1100 独占。

use alloc::vec;
use alloc::vec::Vec;

/// 诊断段基址（0x1100，全仓独占）。
pub const SEG_BASE: u16 = 0x1100;

pub const E_CC_REASON: u16 = SEG_BASE + 1; // 变更理由缺失
pub const E_CC_SYNC: u16 = SEG_BASE + 2; // 全域同步确认缺失
pub const E_CC_FROZEN: u16 = SEG_BASE + 3; // 基线冻结态直接改
pub const E_CC_WINDOW: u16 = SEG_BASE + 4; // 过渡窗超限
pub const E_CC_FLOW: u16 = SEG_BASE + 5; // 流程步骤违规

/// 接口变更 ADR 流程位（提案 1→评审 2→双签 4→发布 8）。
pub const A_PROPOSE: u32 = 1;
pub const A_REVIEW: u32 = 2;
pub const A_COSIGN: u32 = 4;
pub const A_RELEASE: u32 = 8;

/// 接口变更单。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IfaceChange {
    pub target: &'static str,
    /// 流程位图（按序置位）。
    pub steps: u32,
    /// 不可豁免理由（十年承诺门槛，非空才可发布）。
    pub waiver_reason: &'static str,
}

impl IfaceChange {
    /// 推进流程：按序置位（跳步拒）。
    pub fn advance(&mut self, bit: u32) -> Result<(), u16> {
        let next = match bit {
            A_PROPOSE => 0,
            A_REVIEW => A_PROPOSE,
            A_COSIGN => A_PROPOSE | A_REVIEW,
            A_RELEASE => A_PROPOSE | A_REVIEW | A_COSIGN,
            _ => return Err(E_CC_FLOW),
        };
        if self.steps != next {
            return Err(E_CC_FLOW);
        }
        if bit == A_RELEASE && self.waiver_reason.is_empty() {
            return Err(E_CC_REASON);
        }
        self.steps |= bit;
        Ok(())
    }
    pub fn released(&self) -> bool {
        self.steps & A_RELEASE != 0
    }
}

/// 预算语义变更单（域级 ADR+全域同步确认）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SemanticChange {
    pub invariant: &'static str,
    pub steps: u32, // 同 ADR 四步
    /// 全域同步确认（各组确认数）。
    pub sync_confirmed: usize,
}

/// 九组全域同步底线。
pub const SYNC_FLOOR: usize = 9;

impl SemanticChange {
    pub fn advance(&mut self, bit: u32) -> Result<(), u16> {
        let next = match bit {
            A_PROPOSE => 0,
            A_REVIEW => A_PROPOSE,
            A_COSIGN => A_PROPOSE | A_REVIEW,
            A_RELEASE => A_PROPOSE | A_REVIEW | A_COSIGN,
            _ => return Err(E_CC_FLOW),
        };
        if self.steps != next {
            return Err(E_CC_FLOW);
        }
        if bit == A_RELEASE && self.sync_confirmed < SYNC_FLOOR {
            return Err(E_CC_SYNC);
        }
        self.steps |= bit;
        Ok(())
    }
}

/// 基线变更纪律（ADR 链：解冻→改→再冻结，与 F0468 对齐）。
#[derive(Debug)]
pub struct BaselineChange {
    pub frozen: bool,
    pub version: u32,
}

impl BaselineChange {
    pub fn new() -> Self {
        Self { frozen: true, version: 1 }
    }
    pub fn modify(&mut self) -> Result<(), u16> {
        if self.frozen {
            return Err(E_CC_FROZEN);
        }
        self.version += 1;
        Ok(())
    }
    pub fn refreeze(&mut self) -> Result<(), u16> {
        if self.frozen {
            return Err(E_CC_FROZEN);
        }
        self.frozen = true;
        Ok(())
    }
}

/// 插件契约两版过渡：窗内双版本可查，超窗旧版下线。
pub const TRANSITION_WINDOW_TICKS: u64 = 7200;

#[derive(Debug)]
pub struct ContractTransition {
    pub old_live: bool,
    pub cutover_tick: u64,
}

impl ContractTransition {
    pub fn new(cutover_tick: u64) -> Self {
        Self { old_live: true, cutover_tick }
    }
    /// 查询旧版可用性（窗内可查，窗外下线）。
    pub fn old_available(&mut self, now: u64) -> bool {
        if now.saturating_sub(self.cutover_tick) > TRANSITION_WINDOW_TICKS {
            self.old_live = false; // 超窗强制下线
        }
        self.old_live
    }
}

pub(crate) fn run_checks() -> Vec<(&'static str, bool)> {
    let mut v: Vec<(&'static str, bool)> = Vec::new();
    // 一、接口变更 ADR：按序+理由门槛。
    {
        let mut c = IfaceChange { target: "c02.arb.grant", steps: 0, waiver_reason: "守恒语义修正，无法在旧签名下表达" };
        v.push(("F0477-接口ADR-跳步发布拒", c.advance(A_RELEASE) == Err(E_CC_FLOW)));
        c.advance(A_PROPOSE).ok();
        c.advance(A_REVIEW).ok();
        c.advance(A_COSIGN).ok();
        v.push(("F0477-接口ADR-发布过闸", c.advance(A_RELEASE).is_ok() && c.released()));
        // 无理由拒。
        let mut c2 = IfaceChange { target: "x", steps: A_PROPOSE | A_REVIEW | A_COSIGN, waiver_reason: "" };
        v.push(("F0477-接口ADR-无理由拒", c2.advance(A_RELEASE) == Err(E_CC_REASON)));
    }
    // 二、语义变更：全域同步确认门槛。
    {
        let mut s = SemanticChange { invariant: "conservation", steps: A_PROPOSE | A_REVIEW | A_COSIGN, sync_confirmed: 5 };
        v.push(("F0477-语义ADR-同步不足拒", s.advance(A_RELEASE) == Err(E_CC_SYNC)));
        s.sync_confirmed = SYNC_FLOOR;
        v.push(("F0477-语义ADR-九组确认过闸", s.advance(A_RELEASE).is_ok()));
        // 跳步。
        let mut s2 = SemanticChange { invariant: "x", steps: 0, sync_confirmed: 9 };
        v.push(("F0477-语义ADR-跳步拒", s2.advance(A_COSIGN) == Err(E_CC_FLOW)));
    }
    // 三、基线变更：ADR 链。
    {
        let mut b = BaselineChange::new();
        v.push(("F0477-基线-冻结态改拒", b.modify() == Err(E_CC_FROZEN)));
        b.frozen = false;
        v.push(("F0477-基线-解冻后改+版本推进", b.modify().is_ok() && b.version == 2));
        b.refreeze().ok();
        v.push(("F0477-基线-再冻结后拒改", b.modify() == Err(E_CC_FROZEN)));
    }
    // 四、插件契约两版过渡。
    {
        let mut t = ContractTransition::new(1000);
        v.push(("F0477-插件-窗内旧版可查", t.old_available(1000 + TRANSITION_WINDOW_TICKS)));
        v.push(("F0477-插件-超窗旧版下线", !t.old_available(1000 + TRANSITION_WINDOW_TICKS + 1)));
        v.push(("F0477-插件-超窗判定有界", TRANSITION_WINDOW_TICKS > 0));
    }
    // 五、判据自检。
    {
        v.push(("F0477-判据-ADR四位闭环", A_PROPOSE | A_REVIEW | A_COSIGN | A_RELEASE == 0b1111));
        v.push(("F0477-判据-同步底线九组", SYNC_FLOOR == 9));
    }
    v
}

#[cfg(test)]
mod tests {
    use super::run_checks;
    #[test]
    fn f0477_changectl_checks() {
        for (name, ok) in run_checks() {
            assert!(ok, "红项: {}", name);
        }
    }
}
