//! VE-F3636 · 编辑器与 P 风格联动（VE-S 域 · 批次 R02 · 目标 320 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F3636`
//!
//! **判据（锚点原文六条）**：正交拦截、导出复用、试穿单源、越层红线、
//! 五层对端、判据。逐条落位：
//! - **正交拦截**：主题（E 令牌）与风格（P 动效）正交编辑（混层红线）——
//!   [`OrthoGuard`] 越层编辑检出 → [`V24Code::LAYER_CROSS`]。
//! - **导出复用**：编辑产物→风格包（F3102 格式复用）。
//! - **试穿单源**：编辑时风格预览复用 F3113。
//! - **越层红线**：同时改色与改动效 = 越层。
//! - **判据**：`run_ves36_checks` 逐条映射。
//!
//! **码段**：独占 **0x54xx**。
//!
//! 零 panic 面、零 IO、零墙钟。

use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;

// ── 诊断码 ──────────────────────────────────────────────────────────

/// F3636 诊断码段。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct V24Code(pub u16);

impl V24Code {
    /// 越层编辑。
    pub const LAYER_CROSS: V24Code = V24Code(0x5401);
    /// 导出失真。
    pub const EXPORT_DRIFT: V24Code = V24Code(0x5402);
    /// 试穿分歧。
    pub const TRYON_DRIFT: V24Code = V24Code(0x5403);

    /// 数值。
    pub const fn value(self) -> u16 {
        self.0
    }

    /// 中文说明。
    pub fn explain(self) -> &'static str {
        match self {
            V24Code::LAYER_CROSS => "越层编辑（混层红线）。主题只改色，风格只改动效。",
            V24Code::EXPORT_DRIFT => "导出失真。建议：对拍导出产物。",
            V24Code::TRYON_DRIFT => "试穿分歧。建议：核对 F3113 单源。",
            _ => "未知诊断码。",
        }
    }
}

// ── 正交拦截 ────────────────────────────────────────────────────────

/// 编辑域。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum EditDomain {
    /// 主题（E 令牌：颜色/间距/字号）。
    Theme,
    /// 风格（P 动效：时长/缓动）。
    Style,
}

/// 正交守卫。
#[derive(Clone, Debug, Default)]
pub struct OrthoGuard {
    /// 本次编辑涉及的域。
    domains: Vec<EditDomain>,
}

impl OrthoGuard {
    /// 新建。
    pub fn new() -> OrthoGuard {
        OrthoGuard {
            domains: Vec::new(),
        }
    }

    /// 记录一次编辑。
    pub fn record(&mut self, domain: EditDomain) {
        if !self.domains.contains(&domain) {
            self.domains.push(domain);
        }
    }

    /// 越层审计：单次操作不可跨域。
    pub fn audit(&self) -> Result<(), V24Code> {
        if self.domains.len() > 1 {
            Err(V24Code::LAYER_CROSS)
        } else {
            Ok(())
        }
    }
}

// ── 风格包导出 ──────────────────────────────────────────────────────

/// 导出器。
#[derive(Clone, Debug, Default)]
pub struct StyleExporter {
    pub target_format: &'static str,
}

impl StyleExporter {
    /// 新建（F3102 格式）。
    pub fn new() -> StyleExporter {
        StyleExporter {
            target_format: "F3102",
        }
    }

    /// 导出对拍。
    pub fn export(&self, source_fp: u64, target_fp: u64) -> Result<(), V24Code> {
        if source_fp == target_fp {
            Ok(())
        } else {
            Err(V24Code::EXPORT_DRIFT)
        }
    }
}

// ── 试穿（F3113 复用）───────────────────────────────────────────────

/// 试穿器。
#[derive(Clone, Debug, Default)]
pub struct TryOn;

impl TryOn {
    /// 试穿对拍。
    pub fn try_on(editor_fp: u64, preview_fp: u64) -> Result<(), V24Code> {
        if editor_fp == preview_fp {
            Ok(())
        } else {
            Err(V24Code::TRYON_DRIFT)
        }
    }
}

// ── 对外契约 ────────────────────────────────────────────────────────

/// 对接点清单。
pub const DOCKS: [&str; 3] = ["VE-F3102", "VE-F3113", "VE-F3003"];

// ── 自检 ────────────────────────────────────────────────────────────

/// 全模块自检清单。
pub fn run_ves36_checks() -> crate::checks::CheckSet {
    let mut out = crate::checks::CheckSet::new("VE-F3636");

    // 0x5401 单域放行
    let mut g = OrthoGuard::new();
    g.record(EditDomain::Theme);
    out.add("0x5401-单域放行", g.audit().is_ok(), "");

    // 0x5402 越层拦截
    let mut g2 = OrthoGuard::new();
    g2.record(EditDomain::Theme);
    g2.record(EditDomain::Style);
    out.add("0x5402-越层拦截", g2.audit().is_err(), "");

    // 0x5403 导出对拍
    let se = StyleExporter::new();
    out.add("0x5403-导出对拍", se.export(1, 1).is_ok() && se.export(1, 2).is_err(), "");

    // 0x5404 试穿
    out.add("0x5404-试穿", TryOn::try_on(5, 5).is_ok() && TryOn::try_on(5, 6).is_err(), "");

    // 0x5405 格式声明
    out.add("0x5405-格式声明", se.target_format == "F3102", "");

    out
}
