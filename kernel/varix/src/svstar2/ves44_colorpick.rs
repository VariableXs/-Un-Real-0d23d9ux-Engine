//! VE-F3644 · 壁纸取色联动深化（VE-S 域 · 批次 R03 · 目标 340 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F3644`
//!
//! **职责定位（锚点原文）**：壁纸→系统主题色（E 域取色的创作侧）——
//! - **取色预览**：推导链可视化（推导可见红线：取色算法黑盒 = 创作者失控；
//!   每一步取色逻辑可查 = 透明取色）；
//! - **取色微调**：创作者指定锚点色（锚点优先红线：人工锚点覆盖自动值）；
//! - **取色验证**：取色产物对比度门（复用 F2895，破门红线：自动推导也要门）。
//!
//! **判据（锚点原文六条）**：推导透明、锚点主权、破门红线、取色门、
//! 人工优先、判据。
//!
//! **错误路径与降级矩阵（锚点原文四条 → 落位）**：
//! | 锚点降级项 | 本模块落位 |
//! |---|---|
//! | 推导黑箱→可视化修正（红线实测） | [`P44Code::BLACKBOX_DERIVE`] |
//! | 锚点失效→P1（主权红线实测） | [`P44Code::ANCHOR_LOST`] |
//! | 破门→P1（复述红线实测——自动推导也要门） | [`P44Code::CONTRAST_BREAK`] |
//! | 取色跳变→平滑（复述） | [`P44Code::COLOR_JUMP`] |
//!
//! 零 panic 面、零 IO、零墙钟。

use alloc::vec;
use alloc::vec::Vec;

// ── 诊断码 ──────────────────────────────────────────────────────────

/// F3644 诊断码段（独占 0x5Cxx）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct P44Code(pub u16);

impl P44Code {
    /// 推导黑箱（可视化缺失）。
    pub const BLACKBOX_DERIVE: P44Code = P44Code(0x5C01);
    /// 锚点失效（主权红线 P1）。
    pub const ANCHOR_LOST: P44Code = P44Code(0x5C02);
    /// 对比度破门（P1，自动推导同样执法）。
    pub const CONTRAST_BREAK: P44Code = P44Code(0x5C03);
    /// 取色跳变（应平滑）。
    pub const COLOR_JUMP: P44Code = P44Code(0x5C04);

    pub const fn value(self) -> u16 {
        self.0
    }

    pub fn explain(self) -> &'static str {
        match self {
            P44Code::BLACKBOX_DERIVE => "取色推导黑箱（红线实测）。建议：补推导链可视化。",
            P44Code::ANCHOR_LOST => "锚点失效（主权红线 P1）。建议：人工锚点优先于自动值。",
            P44Code::CONTRAST_BREAK => "取色产物对比度破门（P1）。建议：过 F2895 门后放行。",
            P44Code::COLOR_JUMP => "取色跳变。建议：相邻推导值平滑过渡。",
            _ => "未知诊断码。",
        }
    }
}

// ── 推导链（透明取色） ──────────────────────────────────────────────

/// 推导步：每步可查（推导可见红线）。
#[derive(Clone, Copy)]
pub struct DeriveStep {
    pub step_no: u8,
    /// 本步 RGB（24bit）。
    pub rgb: u32,
    /// 步骤说明 id（0=采样, 1=加权, 2=提亮, 3=输出）。
    pub op: u8,
}

/// 推导链（可视化数据源）。
pub struct DeriveChain {
    steps: Vec<DeriveStep>,
}

impl DeriveChain {
    pub fn new() -> Self {
        Self { steps: Vec::new() }
    }

    /// 推一步；黑箱 = 无步骤可查。
    pub fn push(&mut self, rgb: u32, op: u8) -> usize {
        let step_no = self.steps.len() as u8;
        self.steps.push(DeriveStep { step_no, rgb: rgb & 0xFF_FFFF, op });
        self.steps.len()
    }

    pub fn len(&self) -> usize {
        self.steps.len()
    }

    pub fn is_empty(&self) -> bool {
        self.steps.is_empty()
    }

    /// 链是否透明（至少 3 步且含输出步）。
    pub fn transparent(&self) -> bool {
        self.steps.len() >= 3 && self.steps.iter().any(|s| s.op == 3)
    }

    /// 取第 n 步。
    pub fn step(&self, n: usize) -> Option<&DeriveStep> {
        self.steps.get(n)
    }

    /// 链末值。
    pub fn output(&self) -> Option<u32> {
        self.steps.last().map(|s| s.rgb)
    }
}

// ── 锚点覆盖（人工主权） ────────────────────────────────────────────

/// 颜色来源。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Source {
    /// 自动推导值。
    Auto,
    /// 人工锚点覆盖。
    Anchor,
}

/// 主题色条目。
#[derive(Clone, Copy)]
pub struct ThemeColor {
    pub rgb: u32,
    pub source: Source,
    /// 锚点有效位（人工主权）。
    pub anchor_live: bool,
}

// ── 对比度门（复用 F2895 口径） ─────────────────────────────────────

/// 对比度门：相对亮度差（万分制）。
pub struct ContrastGate {
    pub threshold_bps: u32,
    pub breaks: u32,
}

impl ContrastGate {
    /// 亮度近似（Rec.709 加权，整数）。
    pub fn luma(rgb: u32) -> u32 {
        let r = ((rgb >> 16) & 0xFF) * 2126;
        let g = ((rgb >> 8) & 0xFF) * 7152;
        let b = (rgb & 0xFF) * 722;
        (r + g + b) / 10_000
    }

    /// 两色对比度（亮度差基点）。
    pub fn contrast_bps(a: u32, b: u32) -> u32 {
        let la = Self::luma(a);
        let lb = Self::luma(b);
        if la > lb {
            (la - lb) * 10_000 / 255
        } else {
            (lb - la) * 10_000 / 255
        }
    }

    /// 执法：低于门槛即破门。
    pub fn check(&mut self, fg: u32, bg: u32) -> bool {
        if Self::contrast_bps(fg, bg) < self.threshold_bps {
            self.breaks += 1;
            false
        } else {
            true
        }
    }
}

// ── 取色器 ──────────────────────────────────────────────────────────

/// 壁纸取色联动器。
pub struct ColorPicker {
    chain: DeriveChain,
    colors: Vec<ThemeColor>,
    gate: ContrastGate,
    jumps: u32,
    last_code: P44Code,
    has_error: bool,
}

impl ColorPicker {
    /// 新建；`threshold_bps` 为对比度门门槛。
    pub fn new(threshold_bps: u32) -> Self {
        Self {
            chain: DeriveChain::new(),
            colors: Vec::new(),
            gate: ContrastGate { threshold_bps, breaks: 0 },
            jumps: 0,
            last_code: P44Code(0),
            has_error: false,
        }
    }

    pub fn last_code(&self) -> P44Code {
        self.last_code
    }

    pub fn has_error(&self) -> bool {
        self.has_error
    }

    fn fail(&mut self, code: P44Code) {
        self.last_code = code;
        self.has_error = true;
    }

    /// 走一步推导。
    pub fn derive(&mut self, rgb: u32, op: u8) -> usize {
        self.chain.push(rgb, op)
    }

    /// 输出自动色：链必须透明，否则黑箱红线。
    pub fn commit_auto(&mut self, bg: u32) -> Result<u32, P44Code> {
        if !self.chain.transparent() {
            self.fail(P44Code::BLACKBOX_DERIVE);
            return Err(P44Code::BLACKBOX_DERIVE);
        }
        let rgb = self.chain.output().unwrap_or(0);
        if !self.gate.check(rgb, bg) {
            self.fail(P44Code::CONTRAST_BREAK);
            return Err(P44Code::CONTRAST_BREAK);
        }
        if let Some(prev) = self.colors.last() {
            let drift = if prev.rgb > rgb { prev.rgb - rgb } else { rgb - prev.rgb };
            if drift > 0x40_0000 {
                self.jumps += 1;
                self.fail(P44Code::COLOR_JUMP);
                return Err(P44Code::COLOR_JUMP);
            }
        }
        self.colors.push(ThemeColor { rgb, source: Source::Auto, anchor_live: false });
        self.last_code = P44Code(0);
        self.has_error = false;
        Ok(rgb)
    }

    /// 人工锚点覆盖：主权优先，仍须过门。
    pub fn set_anchor(&mut self, rgb: u32, bg: u32) -> Result<u32, P44Code> {
        if !self.gate.check(rgb, bg) {
            self.fail(P44Code::CONTRAST_BREAK);
            return Err(P44Code::CONTRAST_BREAK);
        }
        self.colors.push(ThemeColor { rgb: rgb & 0xFF_FFFF, source: Source::Anchor, anchor_live: true });
        self.last_code = P44Code(0);
        self.has_error = false;
        Ok(rgb & 0xFF_FFFF)
    }

    /// 当前生效色：锚点优先（人工主权红线）。
    pub fn effective(&self) -> Option<&ThemeColor> {
        self.colors.iter().rev().find(|c| c.source == Source::Anchor && c.anchor_live)
            .or_else(|| self.colors.last())
    }

    pub fn color_count(&self) -> usize {
        self.colors.len()
    }

    pub fn chain(&self) -> &DeriveChain {
        &self.chain
    }

    /// 推导透明审计。
    pub fn audit_transparent(&mut self) -> Result<bool, P44Code> {
        if self.chain.transparent() {
            self.last_code = P44Code(0);
            self.has_error = false;
            Ok(true)
        } else {
            self.fail(P44Code::BLACKBOX_DERIVE);
            Err(P44Code::BLACKBOX_DERIVE)
        }
    }

    pub fn gate_breaks(&self) -> u32 {
        self.gate.breaks
    }

    pub fn jumps(&self) -> u32 {
        self.jumps
    }

    /// 锚点条目数。
    pub fn anchor_count(&self) -> usize {
        self.colors.iter().filter(|c| c.source == Source::Anchor).count()
    }

    /// 人工优先总评：有锚点时生效色必为锚点。
    pub fn human_priority_green(&self) -> bool {
        if self.colors.iter().any(|c| c.source == Source::Anchor && c.anchor_live) {
            matches!(self.effective(), Some(c) if c.source == Source::Anchor)
        } else {
            true
        }
    }
}

// ── 判据 ────────────────────────────────────────────────────────────

pub fn run_ves44_checks() -> crate::checks::CheckSet {
    use crate::checks::CheckSet;
    fn chk(set: &mut CheckSet, label: &'static str, ok: bool, detail: &'static str) {
        set.add(label, ok, detail);
    }
    let mut set = CheckSet::new("ves44_colorpick");
    {
        let c = ColorPicker::new(1_000);
        chk(&mut set, "C3644-01", c.color_count() == 0 && c.chain().is_empty(), "err");
    }
    {
        let mut c = ColorPicker::new(0);
        c.derive(0x102030, 0);
        c.derive(0x204060, 1);
        c.derive(0x306090, 3);
        let r = c.commit_auto(0xFFFFFF);
        chk(&mut set, "C3644-02", r.is_ok() && c.color_count() == 1, "err");
    }
    {
        let mut c = ColorPicker::new(0);
        c.derive(0x102030, 0);
        let r = c.commit_auto(0xFFFFFF);
        chk(&mut set, "C3644-03", r.err() == Some(P44Code::BLACKBOX_DERIVE), "err");
    }
    {
        let mut c = ColorPicker::new(0);
        let r = c.set_anchor(0x336699, 0x000000);
        chk(&mut set, "C3644-04", r == Ok(0x336699) && c.anchor_count() == 1, "err");
    }
    {
        let mut c = ColorPicker::new(9_000);
        let r = c.set_anchor(0x101010, 0x101010);
        chk(&mut set, "C3644-05", r.err() == Some(P44Code::CONTRAST_BREAK), "err");
    }
    {
        let mut c = ColorPicker::new(0);
        c.derive(0x102030, 0);
        c.derive(0x204060, 1);
        c.derive(0x306090, 3);
        c.commit_auto(0xFFFFFF);
        let r = c.set_anchor(0x000000, 0xFFFFFF);
        chk(&mut set, "C3644-06", r.is_ok() && c.effective().unwrap().source == Source::Anchor, "err");
    }
    {
        let mut c = ColorPicker::new(0);
        c.derive(0x000000, 0);
        c.derive(0x101010, 1);
        c.derive(0x202020, 3);
        c.commit_auto(0xFFFFFF);
        c.derive(0xFF0000, 0);
        c.derive(0xFF4000, 1);
        c.derive(0xFF8000, 3);
        let r = c.commit_auto(0xFFFFFF);
        chk(&mut set, "C3644-07", r.err() == Some(P44Code::COLOR_JUMP) && c.jumps() == 1, "err");
    }
    {
        let mut c = ColorPicker::new(0);
        let r = c.audit_transparent();
        chk(&mut set, "C3644-08", r.err() == Some(P44Code::BLACKBOX_DERIVE), "err");
    }
    {
        let mut c = ColorPicker::new(0);
        c.derive(0x000000, 0);
        c.derive(0x101010, 1);
        c.derive(0x202020, 3);
        let r = c.audit_transparent();
        chk(&mut set, "C3644-09", r == Ok(true), "err");
    }
    {
        let mut c = ColorPicker::new(1_000);
        c.derive(0x000000, 0);
        c.derive(0x101010, 1);
        c.derive(0x202020, 3);
        let r = c.commit_auto(0x202020);
        chk(&mut set, "C3644-10", r.err() == Some(P44Code::CONTRAST_BREAK) && c.gate_breaks() == 1, "err");
    }
    {
        let g = ContrastGate { threshold_bps: 0, breaks: 0 };
        chk(&mut set, "C3644-11", ContrastGate::contrast_bps(0x000000, 0xFFFFFF) == 10_000, "err");
    }
    {
        let mut c = ColorPicker::new(0);
        c.set_anchor(0x112233, 0x000000);
        chk(&mut set, "C3644-12", c.human_priority_green(), "err");
    }
    set
}
