//! VE-F3825 · 色盲安全色板
//!
//! 安全色板库（色弱可辨色对库）：每色对三类模拟验证入库（入库断言 P1）。
//! 推荐与应用分离。第三方提交扩展复用治理。

#![allow(dead_code)]

extern crate alloc;

use alloc::vec::Vec;

// ── 诊断码段（0xADxx）────────────────────────────────────────────────
pub const SEG_BASE: u16 = 0xAD00;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CADCode(pub u16);

impl CADCode {
    pub const OK: Self = Self(SEG_BASE);
    pub const UNVERIFIED_ENTRY: Self = Self(SEG_BASE + 1);
    pub const PAIR_INVALID: Self = Self(SEG_BASE + 2);
    pub const RECOMMEND_FORCED: Self = Self(SEG_BASE + 3);
    pub const BYPASS_BLOCKED: Self = Self(SEG_BASE + 4);

    pub fn explain(self) -> &'static str {
        match self {
            Self::OK => "正常",
            Self::UNVERIFIED_ENTRY => "未验证入库（P1 红线实测）：库审计失败",
            Self::PAIR_INVALID => "色对验证失败",
            Self::RECOMMEND_FORCED => "推荐强推，需分离（复用红线）",
            Self::BYPASS_BLOCKED => "第三方绕验，已拦截",
            _ => "未知",
        }
    }
}

// ── 数据结构 ────────────────────────────────────────────────────────
/// 色对
#[derive(Debug, Clone, Copy)]
pub struct ColorPair {
    pub id: u32,
    pub fg: u32,
    pub bg: u32,
    pub verified_rg: bool,
    pub verified_by: bool,
    pub verified_total: bool,
}

impl ColorPair {
    /// 三类验证
    pub fn all_verified(&self) -> bool {
        self.verified_rg && self.verified_by && self.verified_total
    }
}

/// 安全色板库
#[derive(Debug, Clone)]
pub struct SafePalette {
    entries: Vec<ColorPair>,
}

impl SafePalette {
    pub fn new() -> Self {
        Self { entries: Vec::new() }
    }

    /// 入库断言 P1：未验证不得入库
    pub fn insert(&mut self, pair: ColorPair) -> Result<(), CADCode> {
        if pair.all_verified() {
            self.entries.push(pair);
            Ok(())
        } else {
            Err(CADCode::UNVERIFIED_ENTRY)
        }
    }

    /// 库审计
    pub fn audit(&self) -> Result<(), CADCode> {
        if self.entries.iter().all(|e| e.all_verified()) {
            Ok(())
        } else {
            Err(CADCode::UNVERIFIED_ENTRY)
        }
    }

    pub fn lookup(&self, id: u32) -> Option<&ColorPair> {
        self.entries.iter().find(|e| e.id == id)
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

impl Default for SafePalette {
    fn default() -> Self {
        Self::new()
    }
}

/// 推荐器（与应用分离）
#[derive(Debug, Clone)]
pub struct Recommender {
    forced: bool,
}

impl Recommender {
    pub fn new() -> Self {
        Self { forced: false }
    }

    /// 推荐分离：不得强推
    pub fn recommend(&self, _pair_id: u32) -> Result<(), CADCode> {
        if self.forced {
            Err(CADCode::RECOMMEND_FORCED)
        } else {
            Ok(())
        }
    }

    pub fn set_forced(&mut self, forced: bool) {
        self.forced = forced;
    }
}

impl Default for Recommender {
    fn default() -> Self {
        Self::new()
    }
}

/// 第三方扩展门
#[derive(Debug, Clone)]
pub struct ExtensionGate {
    pub require_verification: bool,
}

impl ExtensionGate {
    pub fn standard() -> Self {
        Self { require_verification: true }
    }

    /// 绕验拦截
    pub fn admit(&self, pair: &ColorPair) -> Result<(), CADCode> {
        if self.require_verification && !pair.all_verified() {
            Err(CADCode::BYPASS_BLOCKED)
        } else {
            Ok(())
        }
    }
}

// ── 判据 ────────────────────────────────────────────────────────────
pub fn run_ves125_checks() -> crate::checks::CheckSet {
    use crate::checks::CheckSet;
    fn chk(set: &mut CheckSet, label: &'static str, ok: bool, detail: &'static str) {
        set.add(label, ok, detail);
    }
    let mut set = CheckSet::new("ves125_cvdsafe");

    fn verified_pair(id: u32) -> ColorPair {
        ColorPair { id, fg: 0xFFFFFF, bg: 0x000000, verified_rg: true, verified_by: true, verified_total: true }
    }

    // 01 入库断言 P1
    {
        let mut p = SafePalette::new();
        let bad = ColorPair { id: 1, fg: 0, bg: 0, verified_rg: true, verified_by: false, verified_total: true };
        chk(&mut set, "C3825-01", p.insert(bad) == Err(CADCode::UNVERIFIED_ENTRY), "err");
    }
    // 02 已验证入库
    {
        let mut p = SafePalette::new();
        chk(&mut set, "C3825-02", p.insert(verified_pair(1)).is_ok() && p.len() == 1, "err");
    }
    // 03 库审计
    {
        let mut p = SafePalette::new();
        p.insert(verified_pair(1)).unwrap();
        chk(&mut set, "C3825-03", p.audit().is_ok(), "err");
    }
    // 04 推荐强推
    {
        let mut r = Recommender::new();
        r.set_forced(true);
        chk(&mut set, "C3825-04", r.recommend(1) == Err(CADCode::RECOMMEND_FORCED), "err");
    }
    // 05 推荐正常
    {
        let r = Recommender::new();
        chk(&mut set, "C3825-05", r.recommend(1).is_ok(), "err");
    }
    // 06 绕验拦截
    {
        let g = ExtensionGate::standard();
        let bad = ColorPair { id: 1, fg: 0, bg: 0, verified_rg: false, verified_by: false, verified_total: false };
        chk(&mut set, "C3825-06", g.admit(&bad) == Err(CADCode::BYPASS_BLOCKED), "err");
    }
    // 07 验证通过
    {
        let g = ExtensionGate::standard();
        chk(&mut set, "C3825-07", g.admit(&verified_pair(1)).is_ok(), "err");
    }
    // 08 查找
    {
        let mut p = SafePalette::new();
        p.insert(verified_pair(42)).unwrap();
        chk(&mut set, "C3825-08", p.lookup(42).is_some() && p.lookup(99).is_none(), "err");
    }
    // 09 空库审计
    {
        let p = SafePalette::new();
        chk(&mut set, "C3825-09", p.audit().is_ok(), "err");
    }
    // 10 解释非空
    {
        chk(&mut set, "C3825-10", !CADCode::UNVERIFIED_ENTRY.explain().is_empty(), "err");
    }

    set
}
