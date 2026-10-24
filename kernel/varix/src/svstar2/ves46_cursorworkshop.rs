//! VE-F3646 · 光标与指针创作（VE-S 域 · 批次 R03 · 目标 320 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F3646`
//!
//! **职责定位（锚点原文）**：光标与指针创作——
//! - **光标编辑**：帧动画光标（帧序列 + 热点设定）（热点红线：
//!   热点偏移 = 点击错位 → 热点精确断言（单像素级）；
//! - **状态集**：标准/帮助/忙/文本/移动全状态集（覆盖红线：
//!   缺状态光标 = 系统回退丑默认）；
//! - **DPI 适配**：多 DPI 光标档（复用精度）；
//! - **无障碍**：大光标/高对比光标创作支持（无障碍光标红线）。
//!
//! **判据（锚点原文六条）**：热点单像素、状态全覆盖、DPI 档、
//! 无障碍支持、精度复用、判据。
//!
//! **错误路径与降级矩阵（锚点原文四条 → 落位）**：
//! | 锚点降级项 | 本模块落位 |
//! |---|---|
//! | 热点偏移→断言拦截（红线实测） | [`C46Code::HOTSPOT_OFF`] |
//! | 状态缺→覆盖断言（红线实测） | [`C46Code::STATE_GAP`] |
//! | DPI 单档→补（复述红线） | [`C46Code::DPI_SINGLE`] |
//! | 无障碍缺模板→补（红线实测） | [`C46Code::A11Y_TEMPLATE_MISSING`] |
//!
//! 零 panic 面、零 IO、零墙钟。

use alloc::vec;
use alloc::vec::Vec;

// ── 诊断码 ──────────────────────────────────────────────────────────

/// F3646 诊断码段（独占 0x5Exx）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct C46Code(pub u16);

impl C46Code {
    /// 热点偏移（点击错位红线）。
    pub const HOTSPOT_OFF: C46Code = C46Code(0x5E01);
    /// 状态集缺口（覆盖红线）。
    pub const STATE_GAP: C46Code = C46Code(0x5E02);
    /// DPI 单档（精度红线）。
    pub const DPI_SINGLE: C46Code = C46Code(0x5E03);
    /// 无障碍模板缺失（红线实测）。
    pub const A11Y_TEMPLATE_MISSING: C46Code = C46Code(0x5E04);
    /// 帧序列为空。
    pub const NO_FRAMES: C46Code = C46Code(0x5E05);

    pub const fn value(self) -> u16 {
        self.0
    }

    pub fn explain(self) -> &'static str {
        match self {
            C46Code::HOTSPOT_OFF => "热点偏移（点击错位红线）。建议：单像素级热点断言。",
            C46Code::STATE_GAP => "状态集缺口（覆盖红线）。建议：补齐标准/帮助/忙/文本/移动。",
            C46Code::DPI_SINGLE => "DPI 单档（精度红线）。建议：补齐多 DPI 档。",
            C46Code::A11Y_TEMPLATE_MISSING => "无障碍模板缺失（红线实测）。建议：补大光标/高对比模板。",
            C46Code::NO_FRAMES => "帧序列为空。建议：至少 1 帧。",
            _ => "未知诊断码。",
        }
    }
}

// ── 状态集 ──────────────────────────────────────────────────────────

/// 光标状态（全状态集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CursorState {
    /// 标准箭头。
    Normal,
    /// 帮助。
    Help,
    /// 忙。
    Busy,
    /// 文本。
    Text,
    /// 移动。
    Move,
}

impl CursorState {
    pub const ALL: [CursorState; 5] = [
        CursorState::Normal,
        CursorState::Help,
        CursorState::Busy,
        CursorState::Text,
        CursorState::Move,
    ];
}

// ── 热点 ────────────────────────────────────────────────────────────

/// 热点（单像素级）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Hotspot {
    pub x: u32,
    pub y: u32,
}

// ── 帧 ──────────────────────────────────────────────────────────────

/// 光标帧。
#[derive(Clone, Copy)]
pub struct Frame {
    pub index: u16,
    pub delay_ms: u16,
    /// 帧内容指纹（对拍用）。
    pub fingerprint: u32,
}

// ── DPI 档 ──────────────────────────────────────────────────────────

/// DPI 档位。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DpiTier {
    /// 100%。
    S100,
    /// 150%。
    S150,
    /// 200%。
    S200,
}

impl DpiTier {
    pub const ALL: [DpiTier; 3] = [DpiTier::S100, DpiTier::S150, DpiTier::S200];

    /// 标称缩放×100。
    pub fn scale(self) -> u32 {
        match self {
            DpiTier::S100 => 100,
            DpiTier::S150 => 150,
            DpiTier::S200 => 200,
        }
    }
}

// ── 光标工坊 ────────────────────────────────────────────────────────

/// 光标与指针创作器。
pub struct CursorWorkshop {
    frames: Vec<Frame>,
    states: Vec<CursorState>,
    dpis: Vec<DpiTier>,
    hotspot: Hotspot,
    /// 画布边长（热点边界）。
    extent: u32,
    a11y_templates: u32,
    last_code: C46Code,
    has_error: bool,
}

impl CursorWorkshop {
    /// 新建；`extent` 为光标画布边长，`hotspot` 初始热点。
    pub fn new(extent: u32, hotspot: Hotspot) -> Self {
        Self {
            frames: Vec::new(),
            states: Vec::new(),
            dpis: Vec::new(),
            hotspot,
            extent,
            a11y_templates: 0,
            last_code: C46Code(0),
            has_error: false,
        }
    }

    pub fn last_code(&self) -> C46Code {
        self.last_code
    }

    pub fn has_error(&self) -> bool {
        self.has_error
    }

    fn fail(&mut self, code: C46Code) {
        self.last_code = code;
        self.has_error = true;
    }

    /// 加帧；序号须连续。
    pub fn add_frame(&mut self, index: u16, delay_ms: u16, fingerprint: u32) -> Result<usize, C46Code> {
        if index as usize != self.frames.len() {
            self.fail(C46Code::NO_FRAMES);
            return Err(C46Code::NO_FRAMES);
        }
        self.frames.push(Frame { index, delay_ms, fingerprint });
        self.last_code = C46Code(0);
        self.has_error = false;
        Ok(self.frames.len())
    }

    /// 设定热点：单像素级断言（坐标必须落在画布内）。
    pub fn set_hotspot(&mut self, x: u32, y: u32) -> Result<Hotspot, C46Code> {
        if x >= self.extent || y >= self.extent {
            self.fail(C46Code::HOTSPOT_OFF);
            return Err(C46Code::HOTSPOT_OFF);
        }
        self.hotspot = Hotspot { x, y };
        self.last_code = C46Code(0);
        self.has_error = false;
        Ok(self.hotspot)
    }

    pub fn hotspot(&self) -> Hotspot {
        self.hotspot
    }

    /// 热点断言：与期望逐像素一致。
    pub fn assert_hotspot(&mut self, expect: Hotspot) -> Result<bool, C46Code> {
        if self.hotspot == expect {
            self.last_code = C46Code(0);
            self.has_error = false;
            Ok(true)
        } else {
            self.fail(C46Code::HOTSPOT_OFF);
            Err(C46Code::HOTSPOT_OFF)
        }
    }

    /// 登记状态。
    pub fn cover_state(&mut self, state: CursorState) -> usize {
        if !self.states.contains(&state) {
            self.states.push(state);
        }
        self.states.len()
    }

    /// 状态全覆盖。
    pub fn states_complete(&self) -> bool {
        CursorState::ALL.iter().all(|s| self.states.contains(s))
    }

    /// 缺口状态清单。
    pub fn missing_states(&self) -> Vec<CursorState> {
        CursorState::ALL.iter().copied().filter(|s| !self.states.contains(s)).collect()
    }

    /// 登记 DPI 档。
    pub fn cover_dpi(&mut self, tier: DpiTier) -> usize {
        if !self.dpis.contains(&tier) {
            self.dpis.push(tier);
        }
        self.dpis.len()
    }

    /// DPI 多档审计（单档 = 精度红线）。
    pub fn dpi_multi(&mut self) -> Result<bool, C46Code> {
        if self.dpis.len() >= 2 {
            self.last_code = C46Code(0);
            self.has_error = false;
            Ok(true)
        } else {
            self.fail(C46Code::DPI_SINGLE);
            Err(C46Code::DPI_SINGLE)
        }
    }

    /// 登记无障碍模板（大光标/高对比）。
    pub fn add_a11y_template(&mut self) -> u32 {
        self.a11y_templates += 1;
        self.a11y_templates
    }

    /// 无障碍支持断言（至少 2 模板：大光标 + 高对比）。
    pub fn a11y_ready(&mut self) -> Result<bool, C46Code> {
        if self.a11y_templates >= 2 {
            self.last_code = C46Code(0);
            self.has_error = false;
            Ok(true)
        } else {
            self.fail(C46Code::A11Y_TEMPLATE_MISSING);
            Err(C46Code::A11Y_TEMPLATE_MISSING)
        }
    }

    pub fn frame_count(&self) -> usize {
        self.frames.len()
    }

    /// 帧序列总时长（毫秒）。
    pub fn total_delay_ms(&self) -> u32 {
        self.frames.iter().map(|f| f.delay_ms as u32).sum()
    }

    pub fn dpi_count(&self) -> usize {
        self.dpis.len()
    }

    pub fn a11y_templates(&self) -> u32 {
        self.a11y_templates
    }

    /// 帧对拍：全部指纹互异（防复制失真）。
    pub fn frames_distinct(&self) -> bool {
        for (i, a) in self.frames.iter().enumerate() {
            for b in self.frames.iter().skip(i + 1) {
                if a.fingerprint == b.fingerprint {
                    return false;
                }
            }
        }
        true
    }

    /// 精度复用总评：DPI 多档 + 热点在界内。
    pub fn precision_green(&mut self) -> bool {
        self.dpi_multi().is_ok()
            && self.hotspot.x < self.extent
            && self.hotspot.y < self.extent
    }
}

// ── 判据 ────────────────────────────────────────────────────────────

pub fn run_ves46_checks() -> crate::checks::CheckSet {
    use crate::checks::CheckSet;
    fn chk(set: &mut CheckSet, label: &'static str, ok: bool, detail: &'static str) {
        set.add(label, ok, detail);
    }
    let mut set = CheckSet::new("ves46_cursorworkshop");
    {
        let w = CursorWorkshop::new(32, Hotspot { x: 0, y: 0 });
        chk(&mut set, "C3646-01", w.frame_count() == 0 && w.dpi_count() == 0, "err");
    }
    {
        let mut w = CursorWorkshop::new(32, Hotspot { x: 0, y: 0 });
        let r = w.add_frame(0, 100, 0xA1);
        chk(&mut set, "C3646-02", r == Ok(1), "err");
    }
    {
        let mut w = CursorWorkshop::new(32, Hotspot { x: 0, y: 0 });
        let r = w.add_frame(5, 100, 0xA1);
        chk(&mut set, "C3646-03", r.err() == Some(C46Code::NO_FRAMES), "err");
    }
    {
        let mut w = CursorWorkshop::new(32, Hotspot { x: 0, y: 0 });
        let r = w.set_hotspot(31, 31);
        chk(&mut set, "C3646-04", r == Ok(Hotspot { x: 31, y: 31 }), "err");
    }
    {
        let mut w = CursorWorkshop::new(32, Hotspot { x: 0, y: 0 });
        let r = w.set_hotspot(32, 0);
        chk(&mut set, "C3646-05", r.err() == Some(C46Code::HOTSPOT_OFF), "err");
    }
    {
        let mut w = CursorWorkshop::new(32, Hotspot { x: 2, y: 3 });
        let r = w.assert_hotspot(Hotspot { x: 2, y: 3 });
        chk(&mut set, "C3646-06", r == Ok(true), "err");
    }
    {
        let mut w = CursorWorkshop::new(32, Hotspot { x: 2, y: 3 });
        let r = w.assert_hotspot(Hotspot { x: 2, y: 4 });
        chk(&mut set, "C3646-07", r.err() == Some(C46Code::HOTSPOT_OFF), "err");
    }
    {
        let mut w = CursorWorkshop::new(32, Hotspot { x: 0, y: 0 });
        for s in CursorState::ALL.iter() {
            w.cover_state(*s);
        }
        chk(&mut set, "C3646-08", w.states_complete() && w.missing_states().is_empty(), "err");
    }
    {
        let mut w = CursorWorkshop::new(32, Hotspot { x: 0, y: 0 });
        w.cover_state(CursorState::Normal);
        let r = w.dpi_multi();
        chk(&mut set, "C3646-09", r.err() == Some(C46Code::DPI_SINGLE), "err");
    }
    {
        let mut w = CursorWorkshop::new(32, Hotspot { x: 0, y: 0 });
        w.cover_dpi(DpiTier::S100);
        w.cover_dpi(DpiTier::S200);
        let r = w.dpi_multi();
        chk(&mut set, "C3646-10", r == Ok(true), "err");
    }
    {
        let mut w = CursorWorkshop::new(32, Hotspot { x: 0, y: 0 });
        w.add_a11y_template();
        let r = w.a11y_ready();
        chk(&mut set, "C3646-11", r.err() == Some(C46Code::A11Y_TEMPLATE_MISSING), "err");
    }
    {
        let mut w = CursorWorkshop::new(32, Hotspot { x: 1, y: 1 });
        w.add_frame(0, 50, 1);
        w.add_frame(1, 50, 2);
        w.cover_dpi(DpiTier::S100);
        w.cover_dpi(DpiTier::S150);
        w.add_a11y_template();
        w.add_a11y_template();
        chk(&mut set, "C3646-12", w.precision_green() && w.a11y_ready().is_ok() && w.frames_distinct(), "err");
    }
    set
}
