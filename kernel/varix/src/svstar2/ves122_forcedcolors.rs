//! VE-F3822 · forced-colors 渲染
//!
//! 系统强制色彩模式渲染：system color 关键字→引擎令牌映射表（完整断言）。
//! 缺色红线 P0：forced 模式下缺色渲染=不可用。
//! 覆写白名单 P1：创作者覆写系统强制色=无障碍破坏。
//! 平台差异表公开。

#![allow(dead_code)]

use alloc::vec;
extern crate alloc;

use alloc::vec::Vec;

// ── 诊断码段（0xAAxx）────────────────────────────────────────────────
pub const SEG_BASE: u16 = 0xAA00;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CAA0Code(pub u16);

impl CAA0Code {
    pub const OK: Self = Self(SEG_BASE);
    pub const COLOR_MISSING: Self = Self(SEG_BASE + 1);
    pub const OVERRIDE_VIOLATION: Self = Self(SEG_BASE + 2);
    pub const PLATFORM_UNDOC: Self = Self(SEG_BASE + 3);
    pub const MAP_STALE: Self = Self(SEG_BASE + 4);

    pub fn explain(self) -> &'static str {
        match self {
            Self::OK => "正常",
            Self::COLOR_MISSING => "缺色检出（P0 红线实测）：系统色全集映射缺失",
            Self::OVERRIDE_VIOLATION => "覆写越白名单（P1 红线实测）",
            Self::PLATFORM_UNDOC => "平台差异未文档化",
            Self::MAP_STALE => "映射过时，需版本更新",
            _ => "未知",
        }
    }
}

// ── 数据结构 ────────────────────────────────────────────────────────
/// 系统色关键字
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SystemColor {
    Canvas,
    CanvasText,
    LinkText,
    VisitedText,
    ActiveText,
    ButtonFace,
    ButtonText,
    Highlight,
    HighlightText,
}

impl SystemColor {
    pub const ALL: [SystemColor; 9] = [
        Self::Canvas, Self::CanvasText, Self::LinkText, Self::VisitedText,
        Self::ActiveText, Self::ButtonFace, Self::ButtonText,
        Self::Highlight, Self::HighlightText,
    ];
}

/// 映射表条目
#[derive(Debug, Clone, Copy)]
pub struct ColorMapEntry {
    pub sys: SystemColor,
    pub token_id: u16,
    pub mapped: bool,
}

/// 覆写白名单
#[derive(Debug, Clone)]
pub struct OverrideWhitelist {
    pub entries: Vec<u16>,
}

impl OverrideWhitelist {
    pub fn standard() -> Self {
        // 表单焦点等必要覆写
        Self { entries: alloc::vec![100, 101] }
    }

    pub fn allows(&self, token_id: u16) -> bool {
        self.entries.contains(&token_id)
    }
}

/// 平台差异
#[derive(Debug, Clone)]
pub struct PlatformDiff {
    pub platform: &'static str,
    pub documented: bool,
}

// ── 主逻辑 ──────────────────────────────────────────────────────────
pub struct ForcedColors {
    map: Vec<ColorMapEntry>,
    whitelist: OverrideWhitelist,
    platforms: Vec<PlatformDiff>,
    creator_overrides: Vec<u16>,
}

impl ForcedColors {
    pub fn new(whitelist: OverrideWhitelist) -> Self {
        Self { map: Vec::new(), whitelist, platforms: Vec::new(), creator_overrides: Vec::new() }
    }

    pub fn add_map(&mut self, entry: ColorMapEntry) {
        self.map.push(entry);
    }

    /// 系统色全集映射断言 P0
    pub fn assert_complete_map(&self) -> Result<(), CAA0Code> {
        for sys in SystemColor::ALL {
            if !self.map.iter().any(|m| m.sys == sys && m.mapped) {
                return Err(CAA0Code::COLOR_MISSING);
            }
        }
        Ok(())
    }

    /// 覆写审计 P1
    pub fn add_creator_override(&mut self, token_id: u16) {
        self.creator_overrides.push(token_id);
    }

    pub fn assert_override_whitelist(&self) -> Result<(), CAA0Code> {
        for &t in &self.creator_overrides {
            if !self.whitelist.allows(t) {
                return Err(CAA0Code::OVERRIDE_VIOLATION);
            }
        }
        Ok(())
    }

    pub fn add_platform(&mut self, p: PlatformDiff) {
        self.platforms.push(p);
    }

    /// 平台差异文档化断言
    pub fn assert_platforms_documented(&self) -> Result<(), CAA0Code> {
        if self.platforms.iter().all(|p| p.documented) {
            Ok(())
        } else {
            Err(CAA0Code::PLATFORM_UNDOC)
        }
    }

    /// 映射版本
    pub fn assert_version(&self, expected: u16, actual: u16) -> Result<(), CAA0Code> {
        if expected == actual {
            Ok(())
        } else {
            Err(CAA0Code::MAP_STALE)
        }
    }
}

// ── 判据 ────────────────────────────────────────────────────────────
pub fn run_ves122_checks() -> crate::checks::CheckSet {
    use crate::checks::CheckSet;
    fn chk(set: &mut CheckSet, label: &'static str, ok: bool, detail: &'static str) {
        set.add(label, ok, detail);
    }
    let mut set = CheckSet::new("ves122_forcedcolors");

    fn full_map() -> ForcedColors {
        let mut f = ForcedColors::new(OverrideWhitelist::standard());
        for (i, sys) in SystemColor::ALL.iter().enumerate() {
            f.add_map(ColorMapEntry { sys: *sys, token_id: i as u16, mapped: true });
        }
        f
    }

    // 01 映射完整
    {
        let f = full_map();
        chk(&mut set, "C3822-01", f.assert_complete_map().is_ok(), "err");
    }
    // 02 缺色 P0
    {
        let mut f = ForcedColors::new(OverrideWhitelist::standard());
        f.add_map(ColorMapEntry { sys: SystemColor::Canvas, token_id: 0, mapped: true });
        chk(&mut set, "C3822-02", f.assert_complete_map() == Err(CAA0Code::COLOR_MISSING), "err");
    }
    // 03 覆写越白名单 P1
    {
        let mut f = full_map();
        f.add_creator_override(999);
        chk(&mut set, "C3822-03", f.assert_override_whitelist() == Err(CAA0Code::OVERRIDE_VIOLATION), "err");
    }
    // 04 白名单内覆写
    {
        let mut f = full_map();
        f.add_creator_override(100);
        chk(&mut set, "C3822-04", f.assert_override_whitelist().is_ok(), "err");
    }
    // 05 平台未文档
    {
        let mut f = full_map();
        f.add_platform(PlatformDiff { platform: "win", documented: false });
        chk(&mut set, "C3822-05", f.assert_platforms_documented() == Err(CAA0Code::PLATFORM_UNDOC), "err");
    }
    // 06 平台已文档
    {
        let mut f = full_map();
        f.add_platform(PlatformDiff { platform: "win", documented: true });
        chk(&mut set, "C3822-06", f.assert_platforms_documented().is_ok(), "err");
    }
    // 07 版本过时
    {
        let f = full_map();
        chk(&mut set, "C3822-07", f.assert_version(2, 1) == Err(CAA0Code::MAP_STALE), "err");
    }
    // 08 版本一致
    {
        let f = full_map();
        chk(&mut set, "C3822-08", f.assert_version(1, 1).is_ok(), "err");
    }
    // 09 空覆写
    {
        let f = full_map();
        chk(&mut set, "C3822-09", f.assert_override_whitelist().is_ok(), "err");
    }
    // 10 解释非空
    {
        chk(&mut set, "C3822-10", !CAA0Code::COLOR_MISSING.explain().is_empty(), "err");
    }

    set
}
