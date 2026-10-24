//! VE-F3828 · 图形对比替代
//!
//! 低对比图形替代渲染：边界描边/反色填充/图形替换三策略。
//! 替代库覆盖断言（高频图形）。替代语义一致。第三方图形治理复用。

#![allow(dead_code)]

extern crate alloc;

use alloc::vec::Vec;

// ── 诊断码段（0xB0xx）────────────────────────────────────────────────
pub const SEG_BASE: u16 = 0xB000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CB0Code(pub u16);

impl CB0Code {
    pub const OK: Self = Self(SEG_BASE);
    pub const COVERAGE_GAP: Self = Self(SEG_BASE + 1);
    pub const SEMANTIC_CHANGED: Self = Self(SEG_BASE + 2);
    pub const LIB_STALE: Self = Self(SEG_BASE + 3);
    pub const STRATEGY_UNKNOWN: Self = Self(SEG_BASE + 4);

    pub fn explain(self) -> &'static str {
        match self {
            Self::OK => "正常",
            Self::COVERAGE_GAP => "覆盖缺格（红线实测）：高频图形无替代",
            Self::SEMANTIC_CHANGED => "替代语义变（P1 复用红线实测）",
            Self::LIB_STALE => "库过时，需复审",
            Self::STRATEGY_UNKNOWN => "策略未知",
            _ => "未知",
        }
    }
}

// ── 数据结构 ────────────────────────────────────────────────────────
/// 替代策略
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReplaceStrategy {
    BorderOutline,
    InvertFill,
    GraphicSwap,
}

/// 图形替代条目
#[derive(Debug, Clone)]
pub struct GraphicEntry {
    pub id: u32,
    pub high_freq: bool,
    pub strategy: ReplaceStrategy,
    pub semantic_same: bool,
    pub has_replacement: bool,
}

/// 替代库
#[derive(Debug, Clone)]
pub struct ReplaceLibrary {
    pub entries: Vec<GraphicEntry>,
    pub version: u16,
}

impl ReplaceLibrary {
    pub fn new(version: u16) -> Self {
        Self { entries: Vec::new(), version }
    }

    pub fn add(&mut self, entry: GraphicEntry) {
        self.entries.push(entry);
    }

    /// 覆盖断言：高频图形必须有替代
    pub fn assert_coverage(&self) -> Result<(), CB0Code> {
        for e in &self.entries {
            if e.high_freq && !e.has_replacement {
                return Err(CB0Code::COVERAGE_GAP);
            }
        }
        Ok(())
    }

    /// 语义一致断言 P1
    pub fn assert_semantic(&self) -> Result<(), CB0Code> {
        if self.entries.iter().any(|e| !e.semantic_same) {
            Err(CB0Code::SEMANTIC_CHANGED)
        } else {
            Ok(())
        }
    }

    pub fn assert_version(&self, expected: u16) -> Result<(), CB0Code> {
        if self.version == expected {
            Ok(())
        } else {
            Err(CB0Code::LIB_STALE)
        }
    }
}

/// 替代渲染器
#[derive(Debug, Clone)]
pub struct GraphicReplacer {
    lib: ReplaceLibrary,
    applied: Vec<u32>,
}

impl GraphicReplacer {
    pub fn new(lib: ReplaceLibrary) -> Self {
        Self { lib, applied: Vec::new() }
    }

    pub fn replace(&mut self, id: u32) -> Result<ReplaceStrategy, CB0Code> {
        let entry = self.lib.entries.iter().find(|e| e.id == id).ok_or(CB0Code::STRATEGY_UNKNOWN)?;
        if !entry.has_replacement {
            return Err(CB0Code::COVERAGE_GAP);
        }
        if !entry.semantic_same {
            return Err(CB0Code::SEMANTIC_CHANGED);
        }
        self.applied.push(id);
        Ok(entry.strategy)
    }

    pub fn applied_count(&self) -> usize {
        self.applied.len()
    }
}

// ── 判据 ────────────────────────────────────────────────────────────
pub fn run_ves128_checks() -> crate::checks::CheckSet {
    use crate::checks::CheckSet;
    fn chk(set: &mut CheckSet, label: &'static str, ok: bool, detail: &'static str) {
        set.add(label, ok, detail);
    }
    let mut set = CheckSet::new("ves128_graphicreplace");

    // 01 覆盖缺格
    {
        let mut lib = ReplaceLibrary::new(1);
        lib.add(GraphicEntry { id: 1, high_freq: true, strategy: ReplaceStrategy::BorderOutline, semantic_same: true, has_replacement: false });
        chk(&mut set, "C3828-01", lib.assert_coverage() == Err(CB0Code::COVERAGE_GAP), "err");
    }
    // 02 覆盖完整
    {
        let mut lib = ReplaceLibrary::new(1);
        lib.add(GraphicEntry { id: 1, high_freq: true, strategy: ReplaceStrategy::BorderOutline, semantic_same: true, has_replacement: true });
        chk(&mut set, "C3828-02", lib.assert_coverage().is_ok(), "err");
    }
    // 03 语义变 P1
    {
        let mut lib = ReplaceLibrary::new(1);
        lib.add(GraphicEntry { id: 1, high_freq: false, strategy: ReplaceStrategy::InvertFill, semantic_same: false, has_replacement: true });
        chk(&mut set, "C3828-03", lib.assert_semantic() == Err(CB0Code::SEMANTIC_CHANGED), "err");
    }
    // 04 语义一致
    {
        let mut lib = ReplaceLibrary::new(1);
        lib.add(GraphicEntry { id: 1, high_freq: false, strategy: ReplaceStrategy::GraphicSwap, semantic_same: true, has_replacement: true });
        chk(&mut set, "C3828-04", lib.assert_semantic().is_ok(), "err");
    }
    // 05 替代成功
    {
        let mut lib = ReplaceLibrary::new(1);
        lib.add(GraphicEntry { id: 42, high_freq: true, strategy: ReplaceStrategy::BorderOutline, semantic_same: true, has_replacement: true });
        let mut r = GraphicReplacer::new(lib);
        chk(&mut set, "C3828-05", r.replace(42) == Ok(ReplaceStrategy::BorderOutline), "err");
    }
    // 06 无替代
    {
        let mut lib = ReplaceLibrary::new(1);
        lib.add(GraphicEntry { id: 1, high_freq: false, strategy: ReplaceStrategy::BorderOutline, semantic_same: true, has_replacement: false });
        let mut r = GraphicReplacer::new(lib);
        chk(&mut set, "C3828-06", r.replace(1) == Err(CB0Code::COVERAGE_GAP), "err");
    }
    // 07 版本过时
    {
        let lib = ReplaceLibrary::new(1);
        chk(&mut set, "C3828-07", lib.assert_version(2) == Err(CB0Code::LIB_STALE), "err");
    }
    // 08 版本一致
    {
        let lib = ReplaceLibrary::new(1);
        chk(&mut set, "C3828-08", lib.assert_version(1).is_ok(), "err");
    }
    // 09 未知 id
    {
        let lib = ReplaceLibrary::new(1);
        let mut r = GraphicReplacer::new(lib);
        chk(&mut set, "C3828-09", r.replace(999) == Err(CB0Code::STRATEGY_UNKNOWN), "err");
    }
    // 10 解释非空
    {
        chk(&mut set, "C3828-10", !CB0Code::COVERAGE_GAP.explain().is_empty(), "err");
    }

    set
}
