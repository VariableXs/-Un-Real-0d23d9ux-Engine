//! VE-F3970 · 无障碍与隐私资产汇总
//!
//! S 域无障碍与隐私资产汇总：八组双维无障碍证据/心流指标/隐私评审项归集。
//! 汇总规则：每资产挂组/挂条/挂色标或录证双指针。S 域本色专项：双维拆分
//! 口径统一核对 + 心流指标口径统一核对。
//! 错误路径：口径不一→仲裁统一；录证缺→回补；隐私项缺→立案。
//! 性能：归集 O(资产)；核对 O(口径)；索引 O(1)。
//! 跨批对接：F3936/F3956 增维单源；F3962-F3964 上游；F3986 无障碍终审
//! 下游。无障碍与隐私：本条即双维 + 隐私归集本体（域本色）；录证脱敏
//! 复核。

#![allow(dead_code)]
use alloc::vec;

extern crate alloc;

use alloc::vec::Vec;
use alloc::string::String;

// ── 诊断码段（0x33C0）────────────────────────────────────────────────
pub const SEG_BASE: u16 = 0x33C0;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct S9JCode(pub u16);

impl S9JCode {
    pub const OK: Self = Self(SEG_BASE);
    pub const CALIBER_UNMATCH: Self = Self(SEG_BASE + 1);
    pub const RECORDING_GAP: Self = Self(SEG_BASE + 2);
    pub const PRIVACY_ITEM_MISSING: Self = Self(SEG_BASE + 3);
    pub const POINTER_SINGLE: Self = Self(SEG_BASE + 4);

    pub fn explain(self) -> &'static str {
        match self {
            Self::OK => "正常",
            Self::CALIBER_UNMATCH => "口径不一→仲裁统一",
            Self::RECORDING_GAP => "录证缺→回补",
            Self::PRIVACY_ITEM_MISSING => "隐私项缺→立案",
            Self::POINTER_SINGLE => "双指针缺一→补",
            _ => "未知",
        }
    }
}

// ── 双维资产 ─────────────────────────────────────────────────────────

/// 双维种别。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DuoDim {
    /// 工具维。
    Tool,
    /// 产出维。
    Output,
}

/// 资产条目：双指针（色标 或 录证，至少其一；样本须双齐）。
#[derive(Debug, Clone)]
pub struct DuoRow {
    pub name: &'static str,
    pub dim: DuoDim,
    /// 挂组。
    pub group: u8,
    /// 挂条。
    pub clause: u32,
    /// 色标指针（0 = 无）。
    pub color_ref: u32,
    /// 录证指针（0 = 无）。
    pub recording_ref: u32,
    /// 是否样本类（样本必须双指针齐 + 脱敏）。
    pub is_sample: bool,
    /// 录证脱敏。
    pub sanitized: bool,
}

impl DuoRow {
    /// 双指针断言：普通项至少一指针；样本类双指针齐。
    pub fn pointers_ok(&self) -> bool {
        let has_any = self.color_ref > 0 || self.recording_ref > 0;
        let both = self.color_ref > 0 && self.recording_ref > 0;
        self.group >= 1 && self.group <= 8 && self.clause > 0 && (if self.is_sample { both } else { has_any })
    }

    /// 样本脱敏断言。
    pub fn sample_sanitized(&self) -> bool {
        !self.is_sample || self.sanitized
    }
}

/// 归集台账。
#[derive(Debug, Clone)]
pub struct DuoLedger {
    pub rows: Vec<DuoRow>,
}

impl DuoLedger {
    pub fn new() -> Self {
        Self { rows: Vec::new() }
    }

    pub fn admit(&mut self, r: DuoRow) -> S9JCode {
        if !r.pointers_ok() {
            return S9JCode::POINTER_SINGLE;
        }
        if !r.sample_sanitized() {
            return S9JCode::RECORDING_GAP;
        }
        self.rows.push(r);
        S9JCode::OK
    }

    /// 双维覆盖。
    pub fn dim_gaps(&self) -> Vec<DuoDim> {
        [DuoDim::Tool, DuoDim::Output]
            .iter()
            .filter(|d| !self.rows.iter().any(|r| r.dim == **d))
            .map(|d| *d)
            .collect()
    }

    /// 录证缺回补：录证指针为 0 的样本行。
    pub fn recording_gaps(&self) -> Vec<&'static str> {
        self.rows.iter().filter(|r| r.is_sample && r.recording_ref == 0).map(|r| r.name).collect()
    }
}

impl Default for DuoLedger {
    fn default() -> Self {
        Self::new()
    }
}

// ── 双维拆分口径统一核对 ─────────────────────────────────────────────

/// 口径声明：维度 → 口径版本。
#[derive(Debug, Clone)]
pub struct CaliberDecl {
    pub dim: DuoDim,
    pub ver: u32,
}

/// 口径统一核对：两组声明同维不同版 = 不一 → 仲裁统一。
pub fn caliber_unified(decls: &[CaliberDecl]) -> bool {
    for d in [DuoDim::Tool, DuoDim::Output].iter() {
        let vers: Vec<u32> = decls.iter().filter(|x| x.dim == *d).map(|x| x.ver).collect();
        if vers.windows(2).any(|w| w[0] != w[1]) {
            return false;
        }
    }
    true
}

/// 心流指标口径核对。
#[derive(Debug, Clone)]
pub struct FlowCaliber {
    /// 口径版本。
    pub ver: u32,
    /// 采样窗口（分钟）。
    pub window_min: u32,
}

pub const FLOW_WINDOW_STANDARD: u32 = 30;

pub fn flow_ok(f: &FlowCaliber) -> bool {
    f.window_min == FLOW_WINDOW_STANDARD
}

// ── 隐私评审项 ───────────────────────────────────────────────────────

/// 隐私评审项。
#[derive(Debug, Clone)]
pub struct PrivacyItem {
    pub name: &'static str,
    /// 评审结论（true = 通过）。
    pub pass: bool,
}

/// 隐私项立案：缺项或不过。
pub fn privacy_cases(items: &[PrivacyItem]) -> Vec<&'static str> {
    items.iter().filter(|i| !i.pass).map(|i| i.name).collect()
}

// ── 判据 ─────────────────────────────────────────────────────────────

pub fn run_ves224_checks() -> crate::checks::CheckSet {
    use crate::checks::CheckSet;
    fn chk(set: &mut CheckSet, label: &'static str, ok: bool, detail: &'static str) {
        set.add(label, ok, detail);
    }
    let mut set = CheckSet::new("ves224_s09duoasset");

    // 01 双维清单守恒
    {
        chk(&mut set, "C3970-01", DuoDim::Tool as usize == 0 && DuoDim::Output as usize == 1, "err");
    }
    // 02 双指针齐备（样本类）
    {
        let r = DuoRow { name: "录证样本", dim: DuoDim::Output, group: 2, clause: 9, color_ref: 3, recording_ref: 5, is_sample: true, sanitized: true };
        chk(&mut set, "C3970-02", r.pointers_ok() && r.sample_sanitized(), "err");
    }
    // 03 样本类单指针拒收
    {
        let r = DuoRow { name: "缺录证", dim: DuoDim::Tool, group: 1, clause: 9, color_ref: 3, recording_ref: 0, is_sample: true, sanitized: false };
        chk(&mut set, "C3970-03", !r.pointers_ok(), "err");
    }
    // 04 普通项单指针放行
    {
        let r = DuoRow { name: "普通", dim: DuoDim::Tool, group: 3, clause: 9, color_ref: 7, recording_ref: 0, is_sample: false, sanitized: false };
        chk(&mut set, "C3970-04", r.pointers_ok(), "err");
    }
    // 05 未脱敏样本拒收
    {
        let mut l = DuoLedger::new();
        let r = DuoRow { name: "裸录证", dim: DuoDim::Output, group: 4, clause: 9, color_ref: 1, recording_ref: 1, is_sample: true, sanitized: false };
        chk(&mut set, "C3970-05", l.admit(r) == S9JCode::RECORDING_GAP, "err");
    }
    // 06 双维覆盖缺口
    {
        let mut l = DuoLedger::new();
        l.admit(DuoRow { name: "工具证", dim: DuoDim::Tool, group: 1, clause: 1, color_ref: 1, recording_ref: 0, is_sample: false, sanitized: false });
        chk(&mut set, "C3970-06", l.dim_gaps() == vec![DuoDim::Output], "err");
    }
    // 07 录证缺回补名单
    {
        let mut l = DuoLedger::new();
        l.admit(DuoRow { name: "半证", dim: DuoDim::Output, group: 5, clause: 1, color_ref: 2, recording_ref: 0, is_sample: true, sanitized: false });
        chk(&mut set, "C3970-07", l.recording_gaps() == vec!["半证"], "err");
    }
    // 08 双维口径统一
    {
        let decls = vec![CaliberDecl { dim: DuoDim::Tool, ver: 2 }, CaliberDecl { dim: DuoDim::Tool, ver: 2 }, CaliberDecl { dim: DuoDim::Output, ver: 1 }];
        chk(&mut set, "C3970-08", caliber_unified(&decls), "err");
    }
    // 09 口径不一检出→仲裁
    {
        let decls = vec![CaliberDecl { dim: DuoDim::Output, ver: 1 }, CaliberDecl { dim: DuoDim::Output, ver: 3 }];
        chk(&mut set, "C3970-09", !caliber_unified(&decls), "err");
    }
    // 10 心流口径守恒
    {
        let f = FlowCaliber { ver: 1, window_min: 30 };
        let bad = FlowCaliber { ver: 1, window_min: 45 };
        chk(&mut set, "C3970-10", flow_ok(&f) && !flow_ok(&bad) && FLOW_WINDOW_STANDARD == 30, "err");
    }
    // 11 隐私项立案
    {
        let items = vec![
            PrivacyItem { name: "偏好不外传", pass: true },
            PrivacyItem { name: "录证知情同意", pass: false },
        ];
        chk(&mut set, "C3970-11", privacy_cases(&items) == vec!["录证知情同意"], "err");
    }
    // 12 隐私全过零立案
    {
        let items = vec![PrivacyItem { name: "脱敏归档", pass: true }];
        chk(&mut set, "C3970-12", privacy_cases(&items).is_empty(), "err");
    }
    // 13 诊断码窗界自守
    {
        chk(&mut set, "C3970-13", S9JCode::POINTER_SINGLE.0 < SEG_BASE + 16, "err");
    }
    // 14 错误码解释覆盖
    {
        let all = [S9JCode::OK, S9JCode::CALIBER_UNMATCH, S9JCode::RECORDING_GAP, S9JCode::PRIVACY_ITEM_MISSING, S9JCode::POINTER_SINGLE];
        chk(&mut set, "C3970-14", all.iter().all(|c| c.explain() != "未知"), "err");
    }

    set
}

// ── 测试支撑 ─────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_s09_duoasset_all_green() {
        let s = run_ves224_checks();
        assert!(s.all_passed(), "ves224 red: {}", s.red_items_text());
    }

    #[test]
    fn test_pointer_matrix() {
        let base = DuoRow { name: "x", dim: DuoDim::Tool, group: 1, clause: 1, color_ref: 0, recording_ref: 0, is_sample: false, sanitized: false };
        let mut r = base.clone();
        assert!(!r.pointers_ok()); // 全无
        r.color_ref = 1;
        assert!(r.pointers_ok()); // 普通单指针
        r.is_sample = true;
        assert!(!r.pointers_ok()); // 样本需双
        r.recording_ref = 1;
        assert!(r.pointers_ok());
    }

    #[test]
    fn test_caliber_cross_dim() {
        let decls = vec![
            CaliberDecl { dim: DuoDim::Tool, ver: 5 },
            CaliberDecl { dim: DuoDim::Output, ver: 5 },
            CaliberDecl { dim: DuoDim::Output, ver: 5 },
        ];
        assert!(caliber_unified(&decls));
    }
}
