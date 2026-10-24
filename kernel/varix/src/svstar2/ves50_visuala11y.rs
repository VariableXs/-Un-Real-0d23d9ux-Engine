//! VE-F3650 · 视觉创作无障碍（VE-S 域 · 批次 R03 · 目标 320 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F3650`
//!
//! **职责定位（锚点原文）**：视觉创作无障碍（F3611 执法深化）——
//! - **工坊无障碍**：画布键盘操作（键盘画布红线：画布类工具纯鼠标 =
//!   排除运动障碍 → 画布焦点 + 键盘移动 + 快捷工具）；
//! - **视觉替代**：视觉效果语义描述辅助（壁纸语义描述助手）；
//! - **色弱工具**：色弱模拟预览（模拟预览红线：色弱模拟是创作辅助标配）。
//!
//! **判据（锚点原文六条）**：键盘画布、模拟标配、描述辅助、
//! 自举复述、双维执法、判据。
//!
//! **错误路径与降级矩阵（锚点原文四条 → 落位）**：
//! | 锚点降级项 | 本模块落位 |
//! |---|---|
//! | 纯鼠标画布→P1（红线实测） | [`V50Code::MOUSE_ONLY`] |
//! | 模拟缺失→补（标配红线实测） | [`V50Code::SIM_MISSING`] |
//! | 描述空转→修正（复述） | [`V50Code::DESC_EMPTY`] |
//! | 自举→复述 | [`V50Code::SELFBOOT_FAIL`] |
//!
//! 零 panic 面、零 IO、零墙钟。

use alloc::vec;
use alloc::vec::Vec;

// ── 诊断码 ──────────────────────────────────────────────────────────

/// F3650 诊断码段（独占 0x62xx）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct V50Code(pub u16);

impl V50Code {
    /// 纯鼠标画布（P1 红线实测）。
    pub const MOUSE_ONLY: V50Code = V50Code(0x6201);
    /// 色弱模拟缺失（标配红线）。
    pub const SIM_MISSING: V50Code = V50Code(0x6202);
    /// 语义描述空转。
    pub const DESC_EMPTY: V50Code = V50Code(0x6203);
    /// 自举不过（预览读屏）。
    pub const SELFBOOT_FAIL: V50Code = V50Code(0x6204);

    pub const fn value(self) -> u16 {
        self.0
    }

    pub fn explain(self) -> &'static str {
        match self {
            V50Code::MOUSE_ONLY => "画布纯鼠标操作（红线实测 P1）。建议：补键盘画布协议。",
            V50Code::SIM_MISSING => "色弱模拟缺失（标配红线）。建议：补色弱模拟预览。",
            V50Code::DESC_EMPTY => "语义描述空转。建议：补壁纸语义描述助手。",
            V50Code::SELFBOOT_FAIL => "自举（预览读屏）不过。建议：补预览语义标注。",
            _ => "未知诊断码。",
        }
    }
}

// ── 键盘画布协议 ────────────────────────────────────────────────────

/// 画布操作（键盘可达面）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CanvasOp {
    /// 焦点进入画布。
    Focus,
    /// 方向移动。
    Nudge,
    /// 快捷工具切换。
    ToolHotkey,
}

/// 键盘画布协议状态机。
pub struct KeyboardCanvas {
    ops: Vec<CanvasOp>,
    mouse_ops: u32,
    focused: bool,
}

impl KeyboardCanvas {
    pub fn new() -> Self {
        Self { ops: Vec::new(), mouse_ops: 0, focused: false }
    }

    /// 键盘操作（计入可达面）。
    pub fn key_op(&mut self, op: CanvasOp) -> usize {
        self.ops.push(op);
        if op == CanvasOp::Focus {
            self.focused = true;
        }
        self.ops.len()
    }

    /// 鼠标操作（纯鼠标红线统计）。
    pub fn mouse_op(&mut self) -> u32 {
        self.mouse_ops += 1;
        self.mouse_ops
    }

    /// 键盘画布断言：键盘面存在且非纯鼠标。
    pub fn keyboard_canvas_ok(&mut self) -> Result<bool, V50Code> {
        let has_key = !self.ops.is_empty();
        let pure_mouse = self.mouse_ops > 0 && !has_key;
        if pure_mouse {
            Err(V50Code::MOUSE_ONLY)
        } else if has_key {
            Ok(true)
        } else {
            Err(V50Code::MOUSE_ONLY)
        }
    }

    pub fn focused(&self) -> bool {
        self.focused
    }

    pub fn key_count(&self) -> usize {
        self.ops.len()
    }
}

// ── 语义描述助手 ────────────────────────────────────────────────────

/// 描述助手：视觉效果的语义文本。
pub struct DescribeHelper {
    entries: Vec<u32>,
}

impl DescribeHelper {
    pub fn new() -> Self {
        Self { entries: Vec::new() }
    }

    /// 登记一条描述（长度 > 0 的字符数）。
    pub fn describe(&mut self, chars: u32) -> Result<usize, V50Code> {
        if chars == 0 {
            return Err(V50Code::DESC_EMPTY);
        }
        self.entries.push(chars);
        Ok(self.entries.len())
    }

    pub fn count(&self) -> usize {
        self.entries.len()
    }

    /// 非空转审计。
    pub fn non_empty(&self) -> bool {
        !self.entries.is_empty() && self.entries.iter().all(|&c| c > 0)
    }
}

// ── 色弱模拟预览器 ──────────────────────────────────────────────────

/// 色弱类型（模拟档）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ColorWeak {
    /// 红绿（deuteranopia 近似）。
    RedGreen,
    /// 蓝黄。
    BlueYellow,
    /// 全色盲近似。
    Achromatopsia,
}

impl ColorWeak {
    pub const ALL: [ColorWeak; 3] = [ColorWeak::RedGreen, ColorWeak::BlueYellow, ColorWeak::Achromatopsia];
}

/// 模拟预览器（创作辅助标配）。
pub struct SimPreview {
    sims: Vec<ColorWeak>,
}

impl SimPreview {
    pub fn new() -> Self {
        Self { sims: Vec::new() }
    }

    pub fn enable(&mut self, kind: ColorWeak) -> usize {
        if !self.sims.contains(&kind) {
            self.sims.push(kind);
        }
        self.sims.len()
    }

    /// 标配红线：三档全开才算标配齐。
    pub fn standard_ready(&mut self) -> Result<bool, V50Code> {
        if ColorWeak::ALL.iter().all(|k| self.sims.contains(k)) {
            Ok(true)
        } else {
            Err(V50Code::SIM_MISSING)
        }
    }

    pub fn enabled_count(&self) -> usize {
        self.sims.len()
    }
}

// ── 双维执法面板 ────────────────────────────────────────────────────

/// 视觉创作无障碍面板。
pub struct VisualA11y {
    canvas: KeyboardCanvas,
    helper: DescribeHelper,
    sim: SimPreview,
    selfboot_ok: bool,
    last_code: V50Code,
    has_error: bool,
}

impl VisualA11y {
    pub fn new() -> Self {
        Self {
            canvas: KeyboardCanvas::new(),
            helper: DescribeHelper::new(),
            sim: SimPreview::new(),
            selfboot_ok: false,
            last_code: V50Code(0),
            has_error: false,
        }
    }

    pub fn last_code(&self) -> V50Code {
        self.last_code
    }

    pub fn has_error(&self) -> bool {
        self.has_error
    }

    fn fail(&mut self, code: V50Code) {
        self.last_code = code;
        self.has_error = true;
    }

    pub fn canvas(&mut self) -> &mut KeyboardCanvas {
        &mut self.canvas
    }

    pub fn describe(&mut self, chars: u32) -> Result<usize, V50Code> {
        self.helper.describe(chars).map_err(|c| {
            self.fail(c);
            c
        })
    }

    pub fn enable_sim(&mut self, kind: ColorWeak) -> usize {
        self.sim.enable(kind)
    }

    /// 自举复述：预览读屏通过。
    pub fn selfboot(&mut self, ok: bool) -> Result<bool, V50Code> {
        self.selfboot_ok = ok;
        if ok {
            self.last_code = V50Code(0);
            self.has_error = false;
            Ok(true)
        } else {
            self.fail(V50Code::SELFBOOT_FAIL);
            Err(V50Code::SELFBOOT_FAIL)
        }
    }

    /// 双维执法总评：键盘画布 + 标配模拟 + 描述非空 + 自举。
    pub fn dual_enforce(&mut self) -> Result<bool, V50Code> {
        if self.canvas.keyboard_canvas_ok().is_err() {
            self.fail(V50Code::MOUSE_ONLY);
            return Err(V50Code::MOUSE_ONLY);
        }
        if self.sim.standard_ready().is_err() {
            self.fail(V50Code::SIM_MISSING);
            return Err(V50Code::SIM_MISSING);
        }
        if !self.helper.non_empty() {
            self.fail(V50Code::DESC_EMPTY);
            return Err(V50Code::DESC_EMPTY);
        }
        if !self.selfboot_ok {
            self.fail(V50Code::SELFBOOT_FAIL);
            return Err(V50Code::SELFBOOT_FAIL);
        }
        self.last_code = V50Code(0);
        self.has_error = false;
        Ok(true)
    }

    pub fn sim_count(&self) -> usize {
        self.sim.enabled_count()
    }

    pub fn desc_count(&self) -> usize {
        self.helper.count()
    }
}

// ── 判据 ────────────────────────────────────────────────────────────

pub fn run_ves50_checks() -> crate::checks::CheckSet {
    use crate::checks::CheckSet;
    fn chk(set: &mut CheckSet, label: &'static str, ok: bool, detail: &'static str) {
        set.add(label, ok, detail);
    }
    let mut set = CheckSet::new("ves50_visuala11y");
    {
        let p = VisualA11y::new();
        chk(&mut set, "C3650-01", p.sim_count() == 0 && p.desc_count() == 0, "err");
    }
    {
        let mut k = KeyboardCanvas::new();
        k.key_op(CanvasOp::Focus);
        let r = k.keyboard_canvas_ok();
        chk(&mut set, "C3650-02", r == Ok(true) && k.focused(), "err");
    }
    {
        let mut k = KeyboardCanvas::new();
        k.mouse_op();
        k.mouse_op();
        let r = k.keyboard_canvas_ok();
        chk(&mut set, "C3650-03", r.err() == Some(V50Code::MOUSE_ONLY), "err");
    }
    {
        let mut h = DescribeHelper::new();
        let r = h.describe(20);
        chk(&mut set, "C3650-04", r == Ok(1) && h.non_empty(), "err");
    }
    {
        let mut h = DescribeHelper::new();
        let r = h.describe(0);
        chk(&mut set, "C3650-05", r.err() == Some(V50Code::DESC_EMPTY), "err");
    }
    {
        let mut s = SimPreview::new();
        s.enable(ColorWeak::RedGreen);
        let r = s.standard_ready();
        chk(&mut set, "C3650-06", r.err() == Some(V50Code::SIM_MISSING), "err");
    }
    {
        let mut s = SimPreview::new();
        for k in ColorWeak::ALL.iter() {
            s.enable(*k);
        }
        let r = s.standard_ready();
        chk(&mut set, "C3650-07", r == Ok(true) && s.enabled_count() == 3, "err");
    }
    {
        let mut p = VisualA11y::new();
        let r = p.selfboot(true);
        chk(&mut set, "C3650-08", r == Ok(true), "err");
    }
    {
        let mut p = VisualA11y::new();
        let r = p.selfboot(false);
        chk(&mut set, "C3650-09", r.err() == Some(V50Code::SELFBOOT_FAIL), "err");
    }
    {
        let mut p = VisualA11y::new();
        p.canvas().key_op(CanvasOp::Focus);
        p.canvas().key_op(CanvasOp::Nudge);
        for k in ColorWeak::ALL.iter() {
            p.enable_sim(*k);
        }
        p.describe(30);
        p.selfboot(true);
        let r = p.dual_enforce();
        chk(&mut set, "C3650-10", r == Ok(true), "err");
    }
    {
        let mut p = VisualA11y::new();
        p.canvas().mouse_op();
        let r = p.dual_enforce();
        chk(&mut set, "C3650-11", r.err() == Some(V50Code::MOUSE_ONLY), "err");
    }
    {
        let mut p = VisualA11y::new();
        p.canvas().key_op(CanvasOp::Focus);
        let r = p.dual_enforce();
        chk(&mut set, "C3650-12", r.err() == Some(V50Code::SIM_MISSING), "err");
    }
    set
}
