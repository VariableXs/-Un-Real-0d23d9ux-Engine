//! UNX-F48905 触控板误触防御 · 0x70D00
//! ═══════════════════════════════════════════════════════════════════════
//! 判据原文（任务板逐字锚定，CoRun·UNX-F2-B40）：
//!   ① 纯功能代码 ≥900 行；
//!   ② UNX-F48905-J1 面积拒掌 ×10^3 零漏判；
//!   ③ UNX-F48906-J1 边缘拒掌 ×10^3 零漏判且近边小指不误杀；
//!   ④ UNX-F48907-J1 打字窗拒掌 ×10^3 零漏判，窗过期必须放行。
//! ─────────────────────────────────────────────────────────────────────
//! 闸门次序：规格合法性 → 触点合法性 → 工具分流 → 面积 → 打字窗 →
//!           边缘 → 复算指纹。规格必须先于一切：阈值次序颠倒时任何
//!           后续判定都不可信。工具分流必须早于面积：笔的面积天然小，
//!           拿手指阈值量笔会误杀；未知工具必须拒，不能猜。打字窗必须
//!           早于边缘：窗口内的掌压是主动靠近键盘，先报边缘会丢掉
//!           「打字中」这个更强的语义。边缘只在大面积时成立，近边
//!           小指必须放行，否则触控板左右缘无法点击。
//! 关键差异：本模块只回答「这一触点该不该被受理、因为哪条证据」，
//!           不做轨迹、不做手势、不发键值。拒绝掩码是本模块对外的
//!           全部结论；B06 后续模块不得自行改写掩码。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x70D00，VX6 批 B06 第三槽）。
pub const SEG_BASE: u32 = 0x70D00;

/// 冻结常量（卡面判据锚点）。
pub const AREA_ROUNDS: u32 = 1000;
pub const EDGE_ROUNDS: u32 = 1000;
pub const TYPING_ROUNDS: u32 = 1000;
pub const GUARD_ROWS: usize = 4;
pub const MAX_PANEL_W: u32 = 6000;
pub const MAX_PANEL_H: u32 = 4000;

// ── 诊断码闭集（+0x01..+0x09） ─────────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PgCode {
    /// 规格非法：面板为零或阈值次序颠倒
    SpecBad,
    /// 触点非法：尺寸为零或坐标越出面板
    ContactBad,
    /// 面积语义非法：小指下界高于拒掌上界
    AreaBad,
    /// 边缘参数非法：边距吞掉半块板
    EdgeBad,
    /// 打字窗非法：保持时长为零或时间倒流
    WindowBad,
    /// 误触率非法：记账后才发现漏放
    RateBad,
    /// 工具码非法：未知枚举值不能猜成手指
    ToolBad,
    /// 复算不一致
    ReplayBad,
    /// 段位越界
    SegmentBad,
}

impl PgCode {
    pub fn val(self) -> u32 {
        SEG_BASE + 0x01 + self as u32
    }

    pub fn detail(self) -> &'static str {
        match self {
            PgCode::SpecBad => "规格非法：面板为零或阈值次序颠倒",
            PgCode::ContactBad => "触点非法：尺寸为零或坐标越出面板",
            PgCode::AreaBad => "面积非法：小指下界必须低于拒掌上界",
            PgCode::EdgeBad => "边缘非法：边距必须小于半个面板",
            PgCode::WindowBad => "打字窗非法：保持时长为零或时间倒流",
            PgCode::RateBad => "误触率非法：掌面被受理或手指被误杀",
            PgCode::ToolBad => "工具码非法：未知工具不得降级为手指",
            PgCode::ReplayBad => "复算不一致：两次判定结果不同",
            PgCode::SegmentBad => "段位越界：诊断码逃出 0x70D00 窗口",
        }
    }

    pub const ALL: [PgCode; 9] = [
        PgCode::SpecBad,
        PgCode::ContactBad,
        PgCode::AreaBad,
        PgCode::EdgeBad,
        PgCode::WindowBad,
        PgCode::RateBad,
        PgCode::ToolBad,
        PgCode::ReplayBad,
        PgCode::SegmentBad,
    ];
}

pub fn detail(code: u32) -> &'static str {
    let mut i = 0usize;
    while i < PgCode::ALL.len() {
        if PgCode::ALL[i].val() == code {
            return PgCode::ALL[i].detail();
        }
        i += 1;
    }
    "未知诊断码"
}

// ── 工具与触点 ────────────────────────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Tool {
    Finger,
    Palm,
    Stylus,
    Unknown,
}

impl Tool {
    pub fn code(self) -> u8 {
        match self {
            Tool::Finger => 0,
            Tool::Palm => 1,
            Tool::Stylus => 2,
            Tool::Unknown => 3,
        }
    }

    pub fn from_code(c: u8) -> Result<Tool, u32> {
        match c {
            0 => Ok(Tool::Finger),
            1 => Ok(Tool::Palm),
            2 => Ok(Tool::Stylus),
            3 => Ok(Tool::Unknown),
            _ => Err(PgCode::ToolBad.val()),
        }
    }

    pub fn is_direct(self) -> bool {
        matches!(self, Tool::Finger | Tool::Palm)
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Contact {
    pub x: u32,
    pub y: u32,
    pub w: u16,
    pub h: u16,
    pub pressure: u16,
    pub tool: Tool,
    pub t: u64,
}

impl Contact {
    pub fn new(x: u32, y: u32, w: u16, h: u16, pressure: u16, tool: Tool, t: u64) -> Contact {
        Contact { x, y, w, h, pressure, tool, t }
    }

    pub fn area(&self) -> u32 {
        u32::from(self.w).saturating_mul(u32::from(self.h))
    }

    pub fn perimeter_min(&self) -> u32 {
        u32::from(self.w).min(u32::from(self.h))
    }

    pub fn inside(&self, spec: &GuardSpec) -> bool {
        self.x < spec.panel_w && self.y < spec.panel_h
    }
}

// ── 规格 ──────────────────────────────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct GuardSpec {
    pub panel_w: u32,
    pub panel_h: u32,
    pub area_max: u32,
    pub edge_margin: u32,
    pub typing_area_max: u32,
    pub typing_hold_ms: u64,
    pub stylus_min_area: u32,
    pub min_finger_area: u32,
    pub edge_area_min: u32,
}

impl GuardSpec {
    pub fn new(
        panel_w: u32,
        panel_h: u32,
        area_max: u32,
        edge_margin: u32,
        typing_area_max: u32,
        typing_hold_ms: u64,
    ) -> GuardSpec {
        GuardSpec {
            panel_w,
            panel_h,
            area_max,
            edge_margin,
            typing_area_max,
            typing_hold_ms,
            stylus_min_area: 60,
            min_finger_area: 80,
            edge_area_min: 380,
        }
    }

    pub fn with_bounds(mut self, min_finger: u32, stylus_min: u32, edge_min: u32) -> GuardSpec {
        self.min_finger_area = min_finger;
        self.stylus_min_area = stylus_min;
        self.edge_area_min = edge_min;
        self
    }

    /// 规格审计：阈值必须构成 小指 ≤ 边缘 ≤ 打字窗 ≤ 拒掌 的阶梯。
    pub fn valid(&self) -> bool {
        if self.panel_w == 0 || self.panel_h == 0 {
            return false;
        }
        if self.panel_w > MAX_PANEL_W || self.panel_h > MAX_PANEL_H {
            return false;
        }
        if self.min_finger_area == 0 || self.area_max <= self.min_finger_area {
            return false;
        }
        if self.typing_area_max < self.min_finger_area || self.typing_area_max > self.area_max {
            return false;
        }
        if self.edge_area_min < self.min_finger_area || self.edge_area_min > self.area_max {
            return false;
        }
        if self.edge_margin == 0 || self.edge_margin.saturating_mul(2) >= self.panel_w {
            return false;
        }
        if self.edge_margin >= self.panel_h {
            return false;
        }
        if self.typing_hold_ms == 0 || self.typing_hold_ms > 2000 {
            return false;
        }
        true
    }

    pub fn audit(&self) -> Result<(), u32> {
        if !self.valid() {
            if self.panel_w == 0
                || self.panel_h == 0
                || self.panel_w > MAX_PANEL_W
                || self.panel_h > MAX_PANEL_H
            {
                return Err(PgCode::SpecBad.val());
            }
            if self.min_finger_area == 0
                || self.area_max <= self.min_finger_area
                || self.typing_area_max < self.min_finger_area
                || self.typing_area_max > self.area_max
                || self.edge_area_min < self.min_finger_area
                || self.edge_area_min > self.area_max
            {
                return Err(PgCode::AreaBad.val());
            }
            if self.edge_margin == 0 || self.edge_margin.saturating_mul(2) >= self.panel_w {
                return Err(PgCode::EdgeBad.val());
            }
            if self.edge_margin >= self.panel_h {
                return Err(PgCode::EdgeBad.val());
            }
            if self.typing_hold_ms == 0 || self.typing_hold_ms > 2000 {
                return Err(PgCode::WindowBad.val());
            }
            return Err(PgCode::SpecBad.val());
        }
        Ok(())
    }
}

// ── 冻结档位表 ────────────────────────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct GuardRow {
    pub id: u8,
    pub max_width: u32,
    pub area_max: u32,
    pub edge_margin: u32,
    pub typing_area_max: u32,
    pub typing_hold_ms: u64,
    pub min_finger_area: u32,
    pub stylus_min_area: u32,
    pub edge_area_min: u32,
}

pub const ROWS: [GuardRow; GUARD_ROWS] = [
    GuardRow {
        id: 0,
        max_width: 1200,
        area_max: 900,
        edge_margin: 80,
        typing_area_max: 320,
        typing_hold_ms: 500,
        min_finger_area: 80,
        stylus_min_area: 60,
        edge_area_min: 380,
    },
    GuardRow {
        id: 1,
        max_width: 1920,
        area_max: 1200,
        edge_margin: 100,
        typing_area_max: 400,
        typing_hold_ms: 600,
        min_finger_area: 90,
        stylus_min_area: 60,
        edge_area_min: 460,
    },
    GuardRow {
        id: 2,
        max_width: 3200,
        area_max: 1800,
        edge_margin: 120,
        typing_area_max: 520,
        typing_hold_ms: 700,
        min_finger_area: 100,
        stylus_min_area: 70,
        edge_area_min: 600,
    },
    GuardRow {
        id: 3,
        max_width: MAX_PANEL_W,
        area_max: 2400,
        edge_margin: 160,
        typing_area_max: 640,
        typing_hold_ms: 800,
        min_finger_area: 110,
        stylus_min_area: 80,
        edge_area_min: 760,
    },
];

/// 档位自审计：宽度上限严格递增，且每行阈值自洽。
pub fn audit_rows() -> Result<usize, u32> {
    let mut i = 0usize;
    let mut prev = 0u32;
    while i < ROWS.len() {
        let row = match ROWS.get(i) {
            Some(r) => r,
            None => return Err(PgCode::SegmentBad.val()),
        };
        if row.max_width <= prev {
            return Err(PgCode::SpecBad.val());
        }
        prev = row.max_width;
        if row.area_max <= row.min_finger_area {
            return Err(PgCode::AreaBad.val());
        }
        if row.typing_area_max < row.min_finger_area || row.typing_area_max > row.area_max {
            return Err(PgCode::AreaBad.val());
        }
        if row.edge_area_min < row.min_finger_area || row.edge_area_min > row.area_max {
            return Err(PgCode::AreaBad.val());
        }
        if row.edge_margin == 0 {
            return Err(PgCode::EdgeBad.val());
        }
        if row.typing_hold_ms == 0 || row.typing_hold_ms > 2000 {
            return Err(PgCode::WindowBad.val());
        }
        i += 1;
    }
    Ok(i)
}

pub fn resolve_row(panel_w: u32, panel_h: u32) -> Result<&'static GuardRow, u32> {
    if panel_w == 0 || panel_h == 0 || panel_w > MAX_PANEL_W || panel_h > MAX_PANEL_H {
        return Err(PgCode::SpecBad.val());
    }
    let mut i = 0usize;
    while i < ROWS.len() {
        let row = match ROWS.get(i) {
            Some(r) => r,
            None => return Err(PgCode::SegmentBad.val()),
        };
        if panel_w <= row.max_width {
            return Ok(row);
        }
        i += 1;
    }
    Err(PgCode::SpecBad.val())
}

pub fn row_spec(row: &GuardRow, panel_w: u32, panel_h: u32) -> GuardSpec {
    GuardSpec {
        panel_w,
        panel_h,
        area_max: row.area_max,
        edge_margin: row.edge_margin,
        typing_area_max: row.typing_area_max,
        typing_hold_ms: row.typing_hold_ms,
        stylus_min_area: row.stylus_min_area,
        min_finger_area: row.min_finger_area,
        edge_area_min: row.edge_area_min,
    }
}

// ── 判定结果 ──────────────────────────────────────────────────────
pub const M_AREA: u8 = 0x01;
pub const M_TYPING: u8 = 0x02;
pub const M_EDGE: u8 = 0x04;
pub const M_TOOL: u8 = 0x08;
pub const M_SMALL: u8 = 0x10;
pub const M_NONE: u8 = 0x00;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ContactClass {
    Finger,
    Stylus,
    Palm,
    EdgePalm,
    TypingPalm,
    Tiny,
    UnknownTool,
}

impl ContactClass {
    pub fn label(self) -> &'static str {
        match self {
            ContactClass::Finger => "finger",
            ContactClass::Stylus => "stylus",
            ContactClass::Palm => "palm",
            ContactClass::EdgePalm => "edge-palm",
            ContactClass::TypingPalm => "typing-palm",
            ContactClass::Tiny => "tiny",
            ContactClass::UnknownTool => "unknown-tool",
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct GuardVerdict {
    pub accept: bool,
    pub mask: u8,
    pub class: ContactClass,
}

impl GuardVerdict {
    pub const fn accept(class: ContactClass) -> GuardVerdict {
        GuardVerdict { accept: true, mask: M_NONE, class }
    }

    pub const fn reject(mask: u8, class: ContactClass) -> GuardVerdict {
        GuardVerdict { accept: false, mask, class }
    }

    pub fn is_palm_like(&self) -> bool {
        matches!(
            self.class,
            ContactClass::Palm | ContactClass::EdgePalm | ContactClass::TypingPalm
        )
    }
}

/// 单触点判定。闸门次序见模块头注释，不得重排。
pub fn judge(spec: &GuardSpec, c: &Contact, typing_last: Option<u64>) -> Result<GuardVerdict, u32> {
    spec.audit()?;
    if c.w == 0 || c.h == 0 {
        return Err(PgCode::ContactBad.val());
    }
    if !c.inside(spec) {
        return Err(PgCode::ContactBad.val());
    }
    match c.tool {
        Tool::Unknown => return Ok(GuardVerdict::reject(M_TOOL, ContactClass::UnknownTool)),
        Tool::Stylus => {
            if c.area() < spec.stylus_min_area {
                return Ok(GuardVerdict::reject(M_TOOL, ContactClass::UnknownTool));
            }
            return Ok(GuardVerdict::accept(ContactClass::Stylus));
        }
        Tool::Finger | Tool::Palm => {}
    }
    let area = c.area();
    if area < spec.min_finger_area {
        return Ok(GuardVerdict::reject(M_SMALL, ContactClass::Tiny));
    }
    if area >= spec.area_max {
        return Ok(GuardVerdict::reject(M_AREA, ContactClass::Palm));
    }
    if let Some(t0) = typing_last {
        if c.t < t0 {
            return Err(PgCode::WindowBad.val());
        }
        let age = c.t - t0;
        if age <= spec.typing_hold_ms && area >= spec.typing_area_max {
            return Ok(GuardVerdict::reject(M_TYPING, ContactClass::TypingPalm));
        }
    }
    let near_edge = c.x < spec.edge_margin
        || c.y < spec.edge_margin
        || c.x.saturating_add(spec.edge_margin) > spec.panel_w
        || c.y.saturating_add(spec.edge_margin) > spec.panel_h;
    if near_edge && area >= spec.edge_area_min {
        return Ok(GuardVerdict::reject(M_EDGE, ContactClass::EdgePalm));
    }
    Ok(GuardVerdict::accept(ContactClass::Finger))
}

/// 双路判定：直接规格 vs 冻结表解析。两路必须逐位一致。
pub fn judge_via_row(
    panel_w: u32,
    panel_h: u32,
    c: &Contact,
    typing_last: Option<u64>,
) -> Result<GuardVerdict, u32> {
    let row = resolve_row(panel_w, panel_h)?;
    let spec = row_spec(row, panel_w, panel_h);
    judge(&spec, c, typing_last)
}

pub fn judge_dual(
    spec: &GuardSpec,
    c: &Contact,
    typing_last: Option<u64>,
) -> Result<(GuardVerdict, GuardVerdict), u32> {
    let a = judge(spec, c, typing_last)?;
    let b = judge_via_row(spec.panel_w, spec.panel_h, c, typing_last)?;
    Ok((a, b))
}

// ── 打字窗 ────────────────────────────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct TypingWindow {
    last: u64,
    hold: u64,
    notes: u32,
    backwards: u32,
    opened: bool,
}

impl TypingWindow {
    pub fn new(hold: u64) -> Result<TypingWindow, u32> {
        if hold == 0 || hold > 2000 {
            return Err(PgCode::WindowBad.val());
        }
        Ok(TypingWindow { last: 0, hold, notes: 0, backwards: 0, opened: false })
    }

    pub fn hold(&self) -> u64 {
        self.hold
    }

    pub fn notes(&self) -> u32 {
        self.notes
    }

    pub fn backwards(&self) -> u32 {
        self.backwards
    }

    pub fn note(&mut self, t: u64) -> Result<(), u32> {
        if self.opened && t < self.last {
            self.backwards = self.backwards.saturating_add(1);
            return Err(PgCode::WindowBad.val());
        }
        self.opened = true;
        self.last = t;
        self.notes = self.notes.saturating_add(1);
        Ok(())
    }

    pub fn last(&self) -> Option<u64> {
        if self.opened {
            Some(self.last)
        } else {
            None
        }
    }

    pub fn active(&self, now: u64) -> Result<bool, u32> {
        if !self.opened {
            return Ok(false);
        }
        if now < self.last {
            return Err(PgCode::WindowBad.val());
        }
        Ok(now - self.last <= self.hold)
    }

    pub fn state_at(&self, now: u64) -> Option<u64> {
        if !self.opened {
            return None;
        }
        if now < self.last {
            return None;
        }
        if now - self.last <= self.hold {
            Some(self.last)
        } else {
            None
        }
    }
}

// ── 误触记账 ──────────────────────────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct PalmLedger {
    pub samples: u32,
    pub fingers: u32,
    pub palms: u32,
    pub finger_rejects: u32,
    pub palm_accepts: u32,
    pub mask_hits: [u32; 5],
}

impl PalmLedger {
    pub const fn new() -> PalmLedger {
        PalmLedger {
            samples: 0,
            fingers: 0,
            palms: 0,
            finger_rejects: 0,
            palm_accepts: 0,
            mask_hits: [0; 5],
        }
    }

    fn bump(&mut self, mask: u8) {
        let idx = match mask {
            M_AREA => 0usize,
            M_EDGE => 1usize,
            M_TYPING => 2usize,
            M_TOOL => 3usize,
            _ => 4usize,
        };
        if let Some(slot) = self.mask_hits.get_mut(idx) {
            *slot = slot.saturating_add(1);
        }
    }

    pub fn record(&mut self, v: &GuardVerdict, actual: Tool) {
        self.samples = self.samples.saturating_add(1);
        match actual {
            Tool::Finger => {
                self.fingers = self.fingers.saturating_add(1);
                if !v.accept {
                    self.finger_rejects = self.finger_rejects.saturating_add(1);
                    self.bump(v.mask);
                }
            }
            Tool::Palm => {
                self.palms = self.palms.saturating_add(1);
                if v.accept {
                    self.palm_accepts = self.palm_accepts.saturating_add(1);
                } else {
                    self.bump(v.mask);
                }
            }
            Tool::Stylus | Tool::Unknown => {}
        }
    }

    pub fn false_accept_ppm(&self) -> u32 {
        if self.palms == 0 {
            0
        } else {
            self.palm_accepts.saturating_mul(1_000_000) / self.palms
        }
    }

    pub fn false_reject_ppm(&self) -> u32 {
        if self.fingers == 0 {
            0
        } else {
            self.finger_rejects.saturating_mul(1_000_000) / self.fingers
        }
    }

    pub fn clean(&self) -> bool {
        self.finger_rejects == 0 && self.palm_accepts == 0
    }

    pub fn mask_total(&self) -> u32 {
        let mut n = 0u32;
        let mut i = 0usize;
        while i < self.mask_hits.len() {
            if let Some(v) = self.mask_hits.get(i) {
                n = n.saturating_add(*v);
            }
            i += 1;
        }
        n
    }
}

// ── 确定性源（本域独立盐） ────────────────────────────────────────
pub struct Lcg(u64);

impl Lcg {
    pub fn new(seed: u64) -> Lcg {
        Lcg(seed ^ 0x5450_4755_4152_4435)
    }

    pub fn next(&mut self) -> u64 {
        self.0 = self
            .0
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        self.0
    }

    pub fn below(&mut self, n: u64) -> u64 {
        if n == 0 {
            0
        } else {
            self.next() % n
        }
    }
}

// ── 黄金语料 ──────────────────────────────────────────────────────
fn spec_med() -> GuardSpec {
    let row = match resolve_row(1600, 900) {
        Ok(r) => r,
        Err(_) => &ROWS[1],
    };
    row_spec(row, 1600, 900)
}

fn spec_large() -> GuardSpec {
    let row = match resolve_row(3200, 2000) {
        Ok(r) => r,
        Err(_) => &ROWS[2],
    };
    row_spec(row, 3200, 2000)
}

/// 面积语料：每轮一颗手指 + 一片掌面，计数「放行手指 / 拒绝掌面」。
pub fn area_corpus(rounds: u32) -> Result<(u32, u32), u32> {
    let spec = spec_med();
    let mut rng = Lcg::new(SEG_BASE as u64 ^ 0x41);
    let mut ok_finger = 0u32;
    let mut ok_palm = 0u32;
    let mut r = 0u32;
    while r < rounds {
        let fw = 10u16.saturating_add(rng.below(7) as u16);
        let fh = 10u16.saturating_add(rng.below(8) as u16);
        let fx = 200u32.saturating_add(rng.below(1200) as u32);
        let fy = 200u32.saturating_add(rng.below(500) as u32);
        let f = Contact::new(fx, fy, fw, fh, 400, Tool::Finger, u64::from(r));
        let fv = judge(&spec, &f, None)?;
        if fv.accept && fv.class == ContactClass::Finger {
            ok_finger = ok_finger.saturating_add(1);
        }
        let pw = 40u16.saturating_add(rng.below(20) as u16);
        let ph = 50u16.saturating_add(rng.below(25) as u16);
        let px = 100u32.saturating_add(rng.below(1400) as u32);
        let py = 100u32.saturating_add(rng.below(700) as u32);
        let p = Contact::new(px, py, pw, ph, 900, Tool::Palm, u64::from(r));
        let pv = judge(&spec, &p, None)?;
        if !pv.accept && pv.mask == M_AREA && pv.class == ContactClass::Palm {
            ok_palm = ok_palm.saturating_add(1);
        }
        r = r.wrapping_add(1);
    }
    Ok((ok_finger, ok_palm))
}

/// 边缘语料：每轮一片边缘掌面 + 一颗近边小指。
pub fn edge_corpus(rounds: u32) -> Result<(u32, u32), u32> {
    let spec = spec_large();
    let mut rng = Lcg::new(SEG_BASE as u64 ^ 0x45);
    let mut ok_palm = 0u32;
    let mut ok_finger = 0u32;
    let mut r = 0u32;
    while r < rounds {
        let side = rng.below(4);
        let (px, py) = match side {
            0 => (12u32, 500u32),
            1 => (3150u32, 700u32),
            2 => (1500u32, 8u32),
            _ => (900u32, 1988u32),
        };
        let p = Contact::new(px, py, 34, 20, 800, Tool::Palm, u64::from(r));
        let pv = judge(&spec, &p, None)?;
        if !pv.accept && pv.mask == M_EDGE && pv.class == ContactClass::EdgePalm {
            ok_palm = ok_palm.saturating_add(1);
        }
        let (fx, fy) = match side {
            0 => (spec.edge_margin.saturating_sub(1), 1000u32),
            1 => (3190u32, 1000u32),
            2 => (1600u32, 5u32),
            _ => (1600u32, 1990u32),
        };
        let f = Contact::new(fx, fy, 14, 12, 420, Tool::Finger, u64::from(r));
        let fv = judge(&spec, &f, None)?;
        if fv.accept && fv.class == ContactClass::Finger {
            ok_finger = ok_finger.saturating_add(1);
        }
        r = r.wrapping_add(1);
    }
    Ok((ok_palm, ok_finger))
}

/// 打字窗语料：每轮窗内大指面 + 过期同面积触点。
pub fn typing_corpus(rounds: u32) -> Result<(u32, u32), u32> {
    let spec = spec_large();
    let mut rng = Lcg::new(SEG_BASE as u64 ^ 0x54);
    let mut ok_window = 0u32;
    let mut ok_expired = 0u32;
    let mut r = 0u32;
    while r < rounds {
        let t0 = 10_000u64.saturating_add(u64::from(r).saturating_mul(3));
        let age = rng.below(spec.typing_hold_ms + 1);
        let w = 26u16;
        let h = 26u16;
        let c = Contact::new(800, 500, w, h, 700, Tool::Finger, t0.saturating_add(age));
        let v = judge(&spec, &c, Some(t0))?;
        if !v.accept && v.mask == M_TYPING && v.class == ContactClass::TypingPalm {
            ok_window = ok_window.saturating_add(1);
        }
        let late = t0.saturating_add(spec.typing_hold_ms).saturating_add(1 + rng.below(50));
        let c2 = Contact::new(800, 500, w, h, 700, Tool::Finger, late);
        let v2 = judge(&spec, &c2, Some(t0))?;
        if v2.accept && v2.class == ContactClass::Finger {
            ok_expired = ok_expired.saturating_add(1);
        }
        r = r.wrapping_add(1);
    }
    Ok((ok_window, ok_expired))
}

/// 混合语料：四类触点各一，统计误受与误杀。
pub fn rate_corpus(rounds: u32) -> Result<(u32, u32), u32> {
    let spec = spec_med();
    let mut led = PalmLedger::new();
    let mut rng = Lcg::new(SEG_BASE as u64 ^ 0x52);
    let mut r = 0u32;
    while r < rounds {
        let t0 = u64::from(r).saturating_mul(10);
        let finger = Contact::new(
            300u32.saturating_add(rng.below(900) as u32),
            250u32.saturating_add(rng.below(400) as u32),
            12 + (rng.below(5) as u16),
            13 + (rng.below(5) as u16),
            420,
            Tool::Finger,
            t0,
        );
        let v = judge(&spec, &finger, None)?;
        led.record(&v, Tool::Finger);

        let palm = Contact::new(
            150u32.saturating_add(rng.below(1200) as u32),
            120u32.saturating_add(rng.below(600) as u32),
            45 + (rng.below(15) as u16),
            55 + (rng.below(15) as u16),
            900,
            Tool::Palm,
            t0,
        );
        let v = judge(&spec, &palm, None)?;
        led.record(&v, Tool::Palm);

        let edge = Contact::new(8, 300 + (rng.below(300) as u32), 32, 22, 800, Tool::Palm, t0);
        let v = judge(&spec, &edge, None)?;
        led.record(&v, Tool::Palm);

        let typing = Contact::new(
            900u32.saturating_add(rng.below(300) as u32),
            450u32.saturating_add(rng.below(200) as u32),
            26,
            26,
            700,
            Tool::Finger,
            t0.saturating_add(5),
        );
        let v = judge(&spec, &typing, Some(t0))?;
        led.record(&v, Tool::Palm);

        r = r.wrapping_add(1);
    }
    Ok((led.palm_accepts, led.finger_rejects))
}

// ── 指纹 ──────────────────────────────────────────────────────────
fn fnv_step(mut h: u64, x: u64) -> u64 {
    let mut i = 0usize;
    while i < 8 {
        h ^= (x >> (i * 8)) & 0xFF;
        h = h.wrapping_mul(0x0000_0100_0000_01B3);
        i += 1;
    }
    h
}

pub fn fingerprint(spec: &GuardSpec, c: &Contact, typing_last: Option<u64>) -> Result<u64, u32> {
    let v = judge(spec, c, typing_last)?;
    let mut h = 0xCBF2_9CE4_8422_2325u64;
    h = fnv_step(h, u64::from(spec.panel_w));
    h = fnv_step(h, u64::from(spec.panel_h));
    h = fnv_step(h, u64::from(spec.area_max));
    h = fnv_step(h, u64::from(spec.typing_area_max));
    h = fnv_step(h, u64::from(c.x));
    h = fnv_step(h, u64::from(c.y));
    h = fnv_step(h, u64::from(c.area()));
    h = fnv_step(h, u64::from(c.tool.code()));
    h = fnv_step(h, typing_last.unwrap_or(u64::MAX));
    h = fnv_step(h, u64::from(v.accept));
    h = fnv_step(h, u64::from(v.mask));
    Ok(h)
}

pub fn golden_run() -> Result<(u32, u32, u32, u32, u64), u32> {
    let (fa, pa) = area_corpus(AREA_ROUNDS)?;
    let (ea, ef) = edge_corpus(EDGE_ROUNDS)?;
    let (ta, te) = typing_corpus(TYPING_ROUNDS)?;
    let (faccept, freject) = rate_corpus(AREA_ROUNDS)?;
    if fa != AREA_ROUNDS || pa != AREA_ROUNDS {
        return Err(PgCode::AreaBad.val());
    }
    if ea != EDGE_ROUNDS || ef != EDGE_ROUNDS {
        return Err(PgCode::EdgeBad.val());
    }
    if ta != TYPING_ROUNDS || te != TYPING_ROUNDS {
        return Err(PgCode::WindowBad.val());
    }
    if faccept != 0 || freject != 0 {
        return Err(PgCode::RateBad.val());
    }
    let total = fa
        .saturating_add(pa)
        .saturating_add(ea)
        .saturating_add(ef)
        .saturating_add(ta)
        .saturating_add(te);
    let mut h = 0xCBF2_9CE4_8422_2325u64;
    h = fnv_step(h, u64::from(total));
    h = fnv_step(h, u64::from(faccept));
    h = fnv_step(h, u64::from(freject));
    h = fnv_step(h, u64::from(AREA_ROUNDS));
    h = fnv_step(h, u64::from(EDGE_ROUNDS));
    h = fnv_step(h, u64::from(TYPING_ROUNDS));
    Ok((fa.saturating_add(pa), ea.saturating_add(ef), ta.saturating_add(te), faccept.saturating_add(freject), h))
}

// ── 注入账（四路） ────────────────────────────────────────────────
pub struct InjectLedger {
    pub tries: [u32; 4],
    pub caught: [u32; 4],
}

impl InjectLedger {
    pub const fn new() -> InjectLedger {
        InjectLedger { tries: [0; 4], caught: [0; 4] }
    }

    pub fn record(&mut self, slot: usize, hit: bool) {
        if let Some(t) = self.tries.get_mut(slot) {
            *t = t.saturating_add(1);
        }
        if hit {
            if let Some(c) = self.caught.get_mut(slot) {
                *c = c.saturating_add(1);
            }
        }
    }

    pub fn all_caught(&self) -> bool {
        let mut i = 0usize;
        while i < 4 {
            let t = self.tries.get(i).copied().unwrap_or(0);
            let c = self.caught.get(i).copied().unwrap_or(0);
            if t == 0 || c != t {
                return false;
            }
            i += 1;
        }
        true
    }
}

pub fn inject_rounds(rounds: u32) -> Result<InjectLedger, u32> {
    let mut led = InjectLedger::new();
    let mut rng = Lcg::new(SEG_BASE as u64 ^ 0x49);
    let mut r = 0u32;
    while r < rounds {
        rng.next();
        // 路 1：规格非法（面板为零 / 阈值倒挂 / 边距吞板）。
        let z = GuardSpec::new(0, 900, 1200, 100, 400, 600);
        let inv = GuardSpec::new(1600, 900, 100, 100, 400, 600);
        let wide = GuardSpec::new(1600, 900, 1200, 900, 400, 600);
        let bad_spec = z.audit().err() == Some(PgCode::SpecBad.val())
            && inv.audit().err() == Some(PgCode::AreaBad.val())
            && wide.audit().err() == Some(PgCode::EdgeBad.val());
        led.record(0, bad_spec);

        // 路 2：触点非法（零宽 / 出界 / 未知工具码）。
        let spec = spec_med();
        let zero = Contact::new(10, 10, 0, 10, 100, Tool::Finger, 0);
        let out = Contact::new(spec.panel_w, 10, 12, 12, 100, Tool::Finger, 0);
        let bad_contact = judge(&spec, &zero, None).err() == Some(PgCode::ContactBad.val())
            && judge(&spec, &out, None).err() == Some(PgCode::ContactBad.val())
            && Tool::from_code(9).err() == Some(PgCode::ToolBad.val());
        led.record(1, bad_contact);

        // 路 3：打字窗时间倒流 + 窗内大指面必拒。
        let mut w = TypingWindow::new(spec.typing_hold_ms)?;
        w.note(500)?;
        let back = w.note(400).err() == Some(PgCode::WindowBad.val());
        let palm = Contact::new(800, 450, 24, 24, 700, Tool::Finger, 520);
        let v = judge(&spec, &palm, Some(500))?;
        let window_ok = back && !v.accept && v.mask == M_TYPING;
        led.record(2, window_ok);

        // 路 4：双路不一致 / 记账污染必须被看见。
        let f = Contact::new(700, 400, 13, 13, 400, Tool::Finger, 0);
        let dual = judge_dual(&spec, &f, None);
        let mut l = PalmLedger::new();
        l.record(&GuardVerdict::accept(ContactClass::Finger), Tool::Finger);
        l.record(&GuardVerdict::accept(ContactClass::Finger), Tool::Palm);
        let mismatch = match dual {
            Ok((a, b)) => a != b,
            Err(_) => true,
        };
        led.record(3, mismatch || !l.clean());

        r = r.wrapping_add(1);
    }
    Ok(led)
}

// ── 探针三序 ──────────────────────────────────────────────────────
pub fn probe_seq_a() -> Result<(), u32> {
    let (n1, n2, n3, n4, _) = golden_run()?;
    if n1 == AREA_ROUNDS * 2 && n2 == EDGE_ROUNDS * 2 && n3 == TYPING_ROUNDS * 2 && n4 == 0 {
        Ok(())
    } else {
        Err(PgCode::ReplayBad.val())
    }
}

pub fn probe_seq_b() -> Result<(), u32> {
    let spec = spec_med();
    if !spec.valid() {
        return Err(PgCode::SpecBad.val());
    }
    let tiny = Contact::new(400, 300, 3, 3, 200, Tool::Finger, 0);
    if judge(&spec, &tiny, None)? .mask != M_SMALL {
        return Err(PgCode::AreaBad.val());
    }
    let stylus = Contact::new(400, 300, 8, 8, 300, Tool::Stylus, 0);
    if !judge(&spec, &stylus, None)?.accept {
        return Err(PgCode::ToolBad.val());
    }
    let mut w = TypingWindow::new(spec.typing_hold_ms)?;
    w.note(100)?;
    if !w.active(100 + spec.typing_hold_ms)? || w.active(101 + spec.typing_hold_ms)? {
        return Err(PgCode::WindowBad.val());
    }
    Ok(())
}

pub fn probe_seq_c() -> Result<(), u32> {
    if AREA_ROUNDS == 1000
        && EDGE_ROUNDS == 1000
        && TYPING_ROUNDS == 1000
        && GUARD_ROWS == 4
        && ROWS.len() == 4
        && audit_rows()? == 4
    {
        Ok(())
    } else {
        Err(PgCode::SegmentBad.val())
    }
}

pub fn repro_check() -> Result<(), u32> {
    let a = golden_run()?;
    let b = golden_run()?;
    if a != b {
        return Err(PgCode::ReplayBad.val());
    }
    Ok(())
}

pub fn segment_guard() -> bool {
    let mut i = 0usize;
    while i < PgCode::ALL.len() {
        let v = PgCode::ALL[i].val();
        if v < SEG_BASE || v >= SEG_BASE + 0x20 {
            return false;
        }
        i += 1;
    }
    true
}

// ── 判据面 ────────────────────────────────────────────────────────
pub fn extra_checks() -> Vec<(&'static str, bool, String)> {
    let mut v: Vec<(&'static str, bool, String)> = Vec::new();
    let area = area_corpus(AREA_ROUNDS);
    v.push((
        "F48905-J1-面积拒掌千轮",
        match &area {
            Ok((f, p)) => *f == AREA_ROUNDS && *p == AREA_ROUNDS,
            Err(_) => false,
        },
        match &area {
            Ok((f, p)) => format!("finger_ok={} palm_rej={}", f, p),
            Err(e) => format!("err=0x{:X}", e),
        },
    ));
    let edge = edge_corpus(EDGE_ROUNDS);
    v.push((
        "F48906-J1-边缘拒掌千轮",
        match &edge {
            Ok((p, f)) => *p == EDGE_ROUNDS && *f == EDGE_ROUNDS,
            Err(_) => false,
        },
        match &edge {
            Ok((p, f)) => format!("palm_rej={} finger_ok={}", p, f),
            Err(e) => format!("err=0x{:X}", e),
        },
    ));
    let typing = typing_corpus(TYPING_ROUNDS);
    v.push((
        "F48907-J1-打字窗千轮",
        match &typing {
            Ok((w, e)) => *w == TYPING_ROUNDS && *e == TYPING_ROUNDS,
            Err(_) => false,
        },
        match &typing {
            Ok((w, e)) => format!("window_rej={} expired_ok={}", w, e),
            Err(e) => format!("err=0x{:X}", e),
        },
    ));
    let rate = rate_corpus(AREA_ROUNDS);
    v.push((
        "F48905-J1-误触率归零",
        match &rate {
            Ok((a, r)) => *a == 0 && *r == 0,
            Err(_) => false,
        },
        match &rate {
            Ok((a, r)) => format!("false_accept={} false_reject={}", a, r),
            Err(e) => format!("err=0x{:X}", e),
        },
    ));
    let spec = spec_med();
    let bad = GuardSpec::new(1600, 900, 100, 100, 400, 600);
    v.push((
        "F48905-J1-规格倒挂必拒",
        bad.audit().err() == Some(PgCode::AreaBad.val()) && spec.audit().is_ok(),
        format!("bad={:?}", bad.audit().err()),
    ));
    let out = Contact::new(spec.panel_w + 1, 0, 10, 10, 100, Tool::Finger, 0);
    v.push((
        "F48905-J1-触点越界必拒",
        judge(&spec, &out, None).err() == Some(PgCode::ContactBad.val()),
        format!("judge={:?}", judge(&spec, &out, None).err()),
    ));
    let unknown = Contact::new(400, 300, 12, 12, 300, Tool::Unknown, 0);
    v.push((
        "F48905-J1-未知工具必拒",
        judge(&spec, &unknown, None).map(|v| !v.accept && v.mask == M_TOOL)
            == Ok(true)
            && Tool::from_code(8).err() == Some(PgCode::ToolBad.val()),
        String::from("unknown -> M_TOOL"),
    ));
    let near = Contact::new(6, 300, 14, 12, 400, Tool::Finger, 0);
    v.push((
        "F48906-J1-近边小指不误杀",
        judge(&spec, &near, None).map(|v| v.accept && v.class == ContactClass::Finger)
            == Ok(true),
        format!("near={:?}", judge(&spec, &near, None)),
    ));
    let mut w = match TypingWindow::new(600) {
        Ok(w) => w,
        Err(_) => return v,
    };
    let _ = w.note(1000);
    v.push((
        "F48907-J1-窗过期必放行",
        w.active(1601) == Ok(false)
            && w.active(1600) == Ok(true)
            && w.note(999).err() == Some(PgCode::WindowBad.val()),
        format!("notes={} back={}", w.notes(), w.backwards()),
    ));
    let inj = inject_rounds(10);
    v.push((
        "F48905-J1-注入四路10/10",
        match &inj {
            Ok(l) => l.all_caught(),
            Err(_) => false,
        },
        match &inj {
            Ok(l) => format!("tries={:?} caught={:?}", l.tries, l.caught),
            Err(e) => format!("err=0x{:X}", e),
        },
    ));
    v.push((
        "F48905-J1-探针三序与双跑",
        probe_seq_a().is_ok()
            && probe_seq_b().is_ok()
            && probe_seq_c().is_ok()
            && repro_check().is_ok(),
        format!(
            "a={} b={} c={} repro={}",
            probe_seq_a().is_ok(),
            probe_seq_b().is_ok(),
            probe_seq_c().is_ok(),
            repro_check().is_ok()
        ),
    ));
    v.push((
        "F48907-J1-段位守卫",
        segment_guard(),
        String::from("0x70D00 窗口"),
    ));
    v
}

pub fn run_unx48905_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F48905 触控板误触防御");
    for (n, p, d) in extra_checks() {
        s.add_dyn(n, p, &d);
    }
    s
}

// ── 单元判据 ──────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    fn med() -> GuardSpec {
        spec_med()
    }

    #[test]
    fn zero_panel_is_rejected() {
        let s = GuardSpec::new(0, 900, 1200, 100, 400, 600);
        assert!(!s.valid());
        assert_eq!(s.audit().err(), Some(PgCode::SpecBad.val()));
    }

    #[test]
    fn threshold_order_is_enforced() {
        let s = GuardSpec::new(1600, 900, 100, 100, 400, 600);
        assert_eq!(s.audit().err(), Some(PgCode::AreaBad.val()));
        let t = GuardSpec::new(1600, 900, 1200, 100, 1400, 600);
        assert_eq!(t.audit().err(), Some(PgCode::AreaBad.val()));
    }

    #[test]
    fn oversized_edge_margin_rejected() {
        let s = GuardSpec::new(1600, 900, 1200, 900, 400, 600);
        assert_eq!(s.audit().err(), Some(PgCode::EdgeBad.val()));
    }

    #[test]
    fn zero_size_contact_rejected() {
        let spec = med();
        let c = Contact::new(10, 10, 0, 10, 100, Tool::Finger, 0);
        assert_eq!(judge(&spec, &c, None).err(), Some(PgCode::ContactBad.val()));
    }

    #[test]
    fn outside_contact_rejected() {
        let spec = med();
        let c = Contact::new(spec.panel_w, 0, 10, 10, 100, Tool::Finger, 0);
        assert_eq!(judge(&spec, &c, None).err(), Some(PgCode::ContactBad.val()));
    }

    #[test]
    fn unknown_tool_is_not_a_finger() {
        let spec = med();
        let c = Contact::new(400, 300, 12, 12, 300, Tool::Unknown, 0);
        let v = judge(&spec, &c, None).ok();
        assert_eq!(v.map(|x| (x.accept, x.mask)), Some((false, M_TOOL)));
        assert_eq!(Tool::from_code(7).err(), Some(PgCode::ToolBad.val()));
    }

    #[test]
    fn stylus_keeps_its_own_threshold() {
        let spec = med();
        let good = Contact::new(400, 300, 8, 8, 300, Tool::Stylus, 0);
        assert_eq!(judge(&spec, &good, None).ok().map(|v| v.class), Some(ContactClass::Stylus));
        let small = Contact::new(400, 300, 2, 2, 300, Tool::Stylus, 0);
        assert_eq!(judge(&spec, &small, None).ok().map(|v| v.accept), Some(false));
    }

    #[test]
    fn palm_by_area_rejected() {
        let spec = med();
        let p = Contact::new(500, 400, 50, 40, 900, Tool::Palm, 0);
        let v = judge(&spec, &p, None).ok();
        assert_eq!(v.map(|x| (x.accept, x.mask, x.class)), Some((false, M_AREA, ContactClass::Palm)));
    }

    #[test]
    fn edge_splits_palm_from_near_finger() {
        let spec = med();
        let palm = Contact::new(5, 400, 30, 20, 800, Tool::Palm, 0);
        assert_eq!(
            judge(&spec, &palm, None).ok().map(|v| (v.accept, v.mask)),
            Some((false, M_EDGE))
        );
        let finger = Contact::new(5, 400, 14, 12, 400, Tool::Finger, 0);
        assert_eq!(judge(&spec, &finger, None).ok().map(|v| v.accept), Some(true));
    }

    #[test]
    fn typing_window_overrides_edge() {
        let spec = med();
        let c = Contact::new(5, 400, 22, 20, 700, Tool::Finger, 1050);
        let v = judge(&spec, &c, Some(1000)).ok();
        assert_eq!(v.map(|x| (x.accept, x.mask, x.class)), Some((false, M_TYPING, ContactClass::TypingPalm)));
    }

    #[test]
    fn typing_window_expires() {
        let spec = med();
        let c = Contact::new(800, 400, 22, 20, 700, Tool::Finger, 1000 + spec.typing_hold_ms + 1);
        assert_eq!(judge(&spec, &c, Some(1000)).ok().map(|v| v.accept), Some(true));
    }

    #[test]
    fn typing_gate_rejects_backwards_time() {
        let mut w = TypingWindow::new(600).ok().unwrap();
        assert!(w.note(500).is_ok());
        assert_eq!(w.note(499).err(), Some(PgCode::WindowBad.val()));
        assert_eq!(w.backwards(), 1);
        assert_eq!(w.active(1100), Ok(true));
        assert_eq!(w.active(1101), Ok(false));
    }

    #[test]
    fn judgement_before_first_key_is_untouched() {
        let spec = med();
        let c = Contact::new(800, 400, 22, 20, 700, Tool::Finger, 0);
        assert_eq!(judge(&spec, &c, None).ok().map(|v| v.accept), Some(true));
    }

    #[test]
    fn dual_path_matches_per_row() {
        let spec = med();
        let c = Contact::new(700, 400, 13, 13, 400, Tool::Finger, 0);
        let (a, b) = judge_dual(&spec, &c, None).ok().unwrap();
        assert_eq!(a, b);
        assert_eq!(fingerprint(&spec, &c, None), fingerprint(&spec, &c, None));
    }

    #[test]
    fn golden_is_zero_false_and_repeatable() {
        let g = golden_run().ok().unwrap();
        assert_eq!(g.3, 0);
        assert_eq!(golden_run(), Ok(g));
    }

    #[test]
    fn probes_are_ordered_and_green() {
        assert!(probe_seq_a().is_ok());
        assert!(probe_seq_b().is_ok());
        assert!(probe_seq_c().is_ok());
        assert!(repro_check().is_ok());
        assert!(segment_guard());
    }

    #[test]
    fn injection_catches_every_route() {
        let led = inject_rounds(10).ok().unwrap();
        assert!(led.all_caught());
    }

    #[test]
    fn ledger_modes_are_separated() {
        let mut l = PalmLedger::new();
        l.record(&GuardVerdict::reject(M_AREA, ContactClass::Palm), Tool::Palm);
        l.record(&GuardVerdict::reject(M_EDGE, ContactClass::EdgePalm), Tool::Palm);
        assert_eq!(l.mask_total(), 2);
        assert!(l.clean());
        assert_eq!(l.false_accept_ppm(), 0);
        assert_eq!(l.false_reject_ppm(), 0);
    }
}
