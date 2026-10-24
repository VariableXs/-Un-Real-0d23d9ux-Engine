//! VE-F3986 · S 域无障碍终审
//!
//! 终审：S 域无障碍本体终审——双维（工具维 + 产出维）证据终版核验 +
//! 心流指标终验 + 五角色走查前置资格核（F3975 预演转正审）。
//! 数据结构：终审册（双维+心流）；资格核表。
//! 错误路径：证据缺→回补；心流指标未达→立案+预案；资格缺→禁入终
//! 走查。
//! 性能：核验 O(维×项)；终验 O(指标)；资格 O(1)。
//! 跨批对接：F3970 上游；F3994 终走查资格闸。
//! 无障碍与隐私：本条即域本色终审（双维拆分口径终版）；录证终版脱敏
//! 复核。

#![allow(dead_code)]
use alloc::vec;

extern crate alloc;

use alloc::vec::Vec;
use alloc::string::String;

// ── 诊断码段（0x40C0）────────────────────────────────────────────────
pub const SEG_BASE: u16 = 0x40C0;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct T6ACode(pub u16);

impl T6ACode {
    pub const OK: Self = Self(SEG_BASE);
    pub const EVIDENCE_GAP: Self = Self(SEG_BASE + 1);
    pub const FLOW_MISS: Self = Self(SEG_BASE + 2);
    pub const QUALITY_DENY: Self = Self(SEG_BASE + 3);

    pub fn explain(self) -> &'static str {
        match self {
            Self::OK => "正常",
            Self::EVIDENCE_GAP => "证据缺→回补",
            Self::FLOW_MISS => "心流未达→立案+预案",
            Self::QUALITY_DENY => "资格缺→禁入终走查",
            _ => "未知",
        }
    }
}

// ── 双维证据终版核验 ─────────────────────────────────────────────────

/// 双维。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DuoDim {
    /// 工具维（S 域工具链产出）。
    Tool,
    /// 产出维（最终用户体验）。
    Output,
}

pub const DUO_DIMS: [DuoDim; 2] = [DuoDim::Tool, DuoDim::Output];

impl DuoDim {
    pub fn name(self) -> &'static str {
        match self { DuoDim::Tool => "工具维", DuoDim::Output => "产出维" }
    }

    pub fn slot(self) -> usize {
        match self { DuoDim::Tool => 0, DuoDim::Output => 1 }
    }
}

/// 维证据：证据条目数 + 脱敏终版标记。
#[derive(Debug, Clone)]
pub struct DimEvidence {
    /// 证据条目数。
    pub items: u32,
    /// 录证脱敏终版复核。
    pub sanitized_final: bool,
}

/// 终审册（双维+心流+资格）。
#[derive(Debug, Clone)]
pub struct A11yFinalBook {
    pub dims: [Option<DimEvidence>; 2],
    /// 心流指标（0-100）。
    pub flow_score: u16,
    /// 心流达标线。
    pub flow_floor: u16,
    /// 心流预案（未达时必填）。
    pub flow_plan: String,
    /// 五角色资格核表：角色名 → 资格过。
    pub qualify: Vec<(&'static str, bool)>,
}

pub const FIVE_ROLES: [&str; 5] = ["视障", "听障", "运动", "认知", "老年"];

pub const FLOW_FLOOR: u16 = 75;

impl A11yFinalBook {
    pub fn new() -> Self {
        Self {
            dims: [None, None],
            flow_score: 0,
            flow_floor: FLOW_FLOOR,
            flow_plan: String::new(),
            qualify: Vec::new(),
        }
    }

    /// 维证据登记。
    pub fn put_dim(&mut self, dim: DuoDim, items: u32, sanitized: bool) {
        self.dims[dim.slot()] = Some(DimEvidence { items, sanitized_final: sanitized });
    }

    /// 双维齐备：两维均有证据且脱敏终版。
    pub fn duo_complete(&self) -> bool {
        self.dims.iter().all(|d| match d {
            Some(e) => e.items > 0 && e.sanitized_final,
            None => false,
        })
    }

    /// 心流终验：达标或带预案。
    pub fn flow_final(&self) -> T6ACode {
        if self.flow_score >= self.flow_floor {
            return T6ACode::OK;
        }
        if self.flow_plan.is_empty() {
            T6ACode::FLOW_MISS
        } else {
            T6ACode::OK
        }
    }

    /// 五角色资格登记。
    pub fn put_qualify(&mut self, role: &'static str, pass: bool) -> bool {
        if !FIVE_ROLES.contains(&role) {
            return false;
        }
        if let Some(slot) = self.qualify.iter_mut().find(|(r, _)| *r == role) {
            slot.1 = pass;
        } else {
            self.qualify.push((role, pass));
        }
        true
    }

    /// 资格闸：五角色齐备全过 → 放入终走查。
    pub fn qualify_gate(&self) -> T6ACode {
        if self.qualify.len() != 5 {
            return T6ACode::QUALITY_DENY;
        }
        if self.qualify.iter().all(|(_, p)| *p) {
            T6ACode::OK
        } else {
            T6ACode::QUALITY_DENY
        }
    }

    /// 缺资格角色清单。
    pub fn unqualified(&self) -> Vec<&'static str> {
        let mut out = Vec::new();
        for role in FIVE_ROLES.iter() {
            let ok = self.qualify.iter().find(|(r, _)| r == role).map(|(_, p)| *p);
            match ok {
                Some(true) => {}
                _ => out.push(*role),
            }
        }
        out
    }
}

// ── 判据 ─────────────────────────────────────────────────────────────

fn chk(set: &mut crate::checks::CheckSet, label: &'static str, ok: bool, detail: &'static str) {
    set.add(label, ok, detail);
}

pub fn run_ves240_checks() -> crate::checks::CheckSet {
    let mut set = crate::checks::CheckSet::new("ves240_s10a11y");

    // C3986-01 诊断码段不越窗。
    chk(&mut set, "C3986-01",
        T6ACode::QUALITY_DENY.0 < SEG_BASE + 16 && SEG_BASE == 0x40C0, "err");

    // C3986-02 双维齐备判定：缺一维 → 回补。
    let mut b = A11yFinalBook::new();
    b.put_dim(DuoDim::Tool, 30, true);
    chk(&mut set, "C3986-02", !b.duo_complete() && T6ACode::EVIDENCE_GAP.explain().contains("回补"), "err");

    // C3986-03 双维齐 + 脱敏终版 → 过。
    b.put_dim(DuoDim::Output, 45, true);
    chk(&mut set, "C3986-03", b.duo_complete(), "err");

    // C3986-04 证据非脱敏 → 仍缺（录证终版脱敏复核）。
    let mut b2 = A11yFinalBook::new();
    b2.put_dim(DuoDim::Tool, 30, true);
    b2.put_dim(DuoDim::Output, 45, false);
    chk(&mut set, "C3986-04", !b2.duo_complete(), "err");

    // C3986-05 心流达标直过。
    let mut b3 = A11yFinalBook::new();
    b3.flow_score = 88;
    chk(&mut set, "C3986-05", b3.flow_final() == T6ACode::OK, "err");

    // C3986-06 心流未达无预案 → 立案。
    let mut b4 = A11yFinalBook::new();
    b4.flow_score = 60;
    chk(&mut set, "C3986-06", b4.flow_final() == T6ACode::FLOW_MISS, "err");

    // C3986-07 心流未达带预案 → 放行（预案制）。
    b4.flow_plan = String::from("预案：认知维引导重设计");
    chk(&mut set, "C3986-07", b4.flow_final() == T6ACode::OK, "err");

    // C3986-08 资格闸：五角色齐全过 → 放行。
    let mut b5 = A11yFinalBook::new();
    for r in FIVE_ROLES.iter() {
        b5.put_qualify(r, true);
    }
    chk(&mut set, "C3986-08", b5.qualify_gate() == T6ACode::OK, "err");

    // C3986-09 一角色缺格 → 禁入终走查。
    b5.put_qualify("认知", false);
    chk(&mut set, "C3986-09",
        b5.qualify_gate() == T6ACode::QUALITY_DENY
            && b5.unqualified() == vec!["认知"], "err");

    // C3986-10 角色少登 → 禁入。
    let mut b6 = A11yFinalBook::new();
    for r in FIVE_ROLES.iter().take(4) {
        b6.put_qualify(r, true);
    }
    chk(&mut set, "C3986-10", b6.qualify_gate() == T6ACode::QUALITY_DENY, "err");

    // C3986-11 越名角色拒绝。
    let mut b7 = A11yFinalBook::new();
    chk(&mut set, "C3986-11", !b7.put_qualify("外星", true), "err");

    // C3986-12 五角色表互异齐备。
    chk(&mut set, "C3986-12", FIVE_ROLES.len() == 5
        && FIVE_ROLES[0] == "视障" && FIVE_ROLES[4] == "老年", "err");

    // C3986-13 心流达标线 75。
    chk(&mut set, "C3986-13", FLOW_FLOOR == 75, "err");

    // C3986-14 维槽双射。
    chk(&mut set, "C3986-14", DuoDim::Tool.slot() != DuoDim::Output.slot(), "err");

    // C3986-15 缺格清单含未登角色。
    let b8 = A11yFinalBook::new();
    chk(&mut set, "C3986-15", b8.unqualified().len() == 5, "err");

    // C3986-16 证据零条目 = 缺。
    let mut b9 = A11yFinalBook::new();
    b9.put_dim(DuoDim::Tool, 0, true);
    b9.put_dim(DuoDim::Output, 45, true);
    chk(&mut set, "C3986-16", !b9.duo_complete(), "err");

    // C3986-17 心流线边界（恰 75 过）。
    let mut b10 = A11yFinalBook::new();
    b10.flow_score = 75;
    chk(&mut set, "C3986-17", b10.flow_final() == T6ACode::OK, "err");

    // C3986-18 资格重登更新（不重复）。
    b5.put_qualify("认知", true);
    chk(&mut set, "C3986-18", b5.qualify.len() == 5 && b5.qualify_gate() == T6ACode::OK, "err");

    // C3986-19 错误码互异。
    let codes = [T6ACode::OK.0, T6ACode::EVIDENCE_GAP.0, T6ACode::FLOW_MISS.0, T6ACode::QUALITY_DENY.0];
    chk(&mut set, "C3986-19",
        codes[0] != codes[1] && codes[1] != codes[2] && codes[2] != codes[3], "err");

    // C3986-20 双维名称终版口径。
    chk(&mut set, "C3986-20",
        DuoDim::Tool.name() == "工具维" && DuoDim::Output.name() == "产出维", "err");

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn duo_needs_both_and_sanitized() {
        let mut b = A11yFinalBook::new();
        b.put_dim(DuoDim::Tool, 3, true);
        assert!(!b.duo_complete());
        b.put_dim(DuoDim::Output, 3, false);
        assert!(!b.duo_complete());
    }

    #[test]
    fn flow_plan_saves_miss() {
        let mut b = A11yFinalBook::new();
        b.flow_score = 10;
        assert_eq!(b.flow_final(), T6ACode::FLOW_MISS);
        b.flow_plan = String::from("预案");
        assert_eq!(b.flow_final(), T6ACode::OK);
    }

    #[test]
    fn qualify_gate_denies_partial() {
        let mut b = A11yFinalBook::new();
        for r in FIVE_ROLES.iter().take(3) {
            b.put_qualify(r, true);
        }
        assert_eq!(b.qualify_gate(), T6ACode::QUALITY_DENY);
    }
}
