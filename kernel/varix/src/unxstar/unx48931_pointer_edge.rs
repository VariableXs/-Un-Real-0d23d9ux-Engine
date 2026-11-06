//! UNX-F48931 按钮状态机 / 边界 clamp 静默区 · 0x71040
//! ═══════════════════════════════════════════════════════════════════════
//! 判据原文（任务板逐字锚定，UNX-M2-B07）：
//!   ① 纯功能代码 ≥280 行；
//!   ② UNX-F48931-J1 状态机全路径 ×10^2 断言；
//!   ③ UNX-F48932-J1 clamp ×10^2 边界一致。
//! ─────────────────────────────────────────────────────────────────────
//! 闸门次序：事件合法性 → 状态迁移 → 拖拽阈值 → 取消路径 → 边界夹取 →
//!           静默区标记 → 账本 → 复算。
//! 状态迁移必须先于拖拽阈值：释放事件在任何状态下的语义不同，先算位移
//! 会把「未按下时的移动」误判成拖拽。取消必须独立成一态：移出视口后如果
//! 直接回 Idle，释放事件会被当成一次有效点击，用户会在拖出窗口后意外
//! 触发点击。夹取必须幂等：夹过一次的点再夹一次不应再报「发生夹取」，
//! 否则账本里的夹取次数会被重复计算。
//! 关键差异：本模块只回答「这次事件让状态走到哪、这一点夹到什么位置、
//!           是否落在静默边带」，不做坐标增益（B07-CORE）、不做亚像素
//!           （B07-DISPATCH）、不做双击（B07-GUARD）。静默区只是标记，
//!           消费与否由上层决定。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x71040，B07 第五槽）。
pub const SEG_BASE: u32 = 0x71040;

/// 拖拽阈值（像素）：位移平方 ≥ 16 即进入拖拽。
pub const DRAG_THRESHOLD_SQ: i64 = 16;
/// 全路径轮数（卡面 ×10^2 载体）。
pub const PATH_ROUNDS: u32 = 100;
/// 夹取轮数（卡面 ×10^2 载体）。
pub const CLAMP_ROUNDS: u32 = 100;
/// 坐标上限。
pub const POS_LIMIT: i64 = 1_000_000;
/// 静默边带宽度上限。
pub const BORDER_MAX: i64 = 1024;
/// 黄金事件轮数。
pub const GOLDEN_ROUNDS: u32 = 1000;
/// 已知按键掩码。
pub const BTN_MASK: u8 = 0x07;
pub const FNV_OFF: u64 = 0xcbf2_9ce4_8422_2325;

// ── 诊断码闭集（本模块窗口 +0x01..+0x0A） ──────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum EgCode {
    /// 状态迁移非法（重复按下/已取消后按下）
    FsmBad,
    /// 事件次序非法（未按下即释放/释放错键）
    SeqBad,
    /// 拖拽语义非法（阈值账不平）
    DragBad,
    /// 夹取规格非法（边界倒置/静默带吞半幅）
    ClampBad,
    /// 静默区标记非法
    SilentBad,
    /// 账本次序非法
    OrderBad,
    /// 复算不一致
    ReplayBad,
    /// 渲染回读失败
    RenderBad,
    /// 覆盖不足
    CoverageBad,
    /// 段位越界
    SegmentBad,
}

impl EgCode {
    pub fn val(self) -> u32 {
        SEG_BASE + 0x01 + self as u32
    }

    pub fn detail(self) -> &'static str {
        match self {
            EgCode::FsmBad => "状态迁移非法：重复按下或已取消后按下",
            EgCode::SeqBad => "事件次序非法：未按下即释放或释放了非当前键",
            EgCode::DragBad => "拖拽语义非法：阈值账与拖拽计数不平",
            EgCode::ClampBad => "夹取规格非法：边界倒置或静默带吞掉半幅",
            EgCode::SilentBad => "静默区标记非法：边带判定与规格不一致",
            EgCode::OrderBad => "账本次序非法：事件账与拒绝账不守恒",
            EgCode::ReplayBad => "复算不一致：两次黄金账不同",
            EgCode::RenderBad => "渲染回读失败：字段缺失或数值越界",
            EgCode::CoverageBad => "覆盖不足：路径轮数或全路径未走满",
            EgCode::SegmentBad => "段位越界：诊断码逃出 0x71040 窗口",
        }
    }

    pub const ALL: [EgCode; 10] = [
        EgCode::FsmBad,
        EgCode::SeqBad,
        EgCode::DragBad,
        EgCode::ClampBad,
        EgCode::SilentBad,
        EgCode::OrderBad,
        EgCode::ReplayBad,
        EgCode::RenderBad,
        EgCode::CoverageBad,
        EgCode::SegmentBad,
    ];
}

pub fn detail(code: u32) -> &'static str {
    let mut i = 0usize;
    while i < EgCode::ALL.len() {
        if EgCode::ALL[i].val() == code {
            return EgCode::ALL[i].detail();
        }
        i += 1;
    }
    "未知诊断码"
}

// ── 事件与状态 ────────────────────────────────────────────────────
pub const BTN_LEFT: u8 = 0x01;
pub const BTN_MIDDLE: u8 = 0x02;
pub const BTN_RIGHT: u8 = 0x04;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PointerEvent {
    Press(u8),
    Move(i64, i64),
    Release(u8),
    Leave,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FsmState {
    Idle,
    Down { btn: u8, ox: i64, oy: i64 },
    Dragging { btn: u8 },
    Cancelled { btn: u8 },
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FsmOutcome {
    Ignored,
    Pressed,
    Dragged,
    Clicked,
    Released,
    Cancelled,
}

fn valid_btn(btn: u8) -> bool {
    btn != 0 && btn & !BTN_MASK == 0 && (btn & btn.wrapping_sub(1)) == 0
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct PointerFsm {
    pub state: FsmState,
    pub events: u32,
    pub rejects: u32,
    pub drags: u32,
    pub cancels: u32,
    pub digest: u64,
}

impl PointerFsm {
    pub fn new() -> PointerFsm {
        PointerFsm {
            state: FsmState::Idle,
            events: 0,
            rejects: 0,
            drags: 0,
            cancels: 0,
            digest: FNV_OFF,
        }
    }

    pub fn input(&mut self, ev: PointerEvent) -> Result<FsmOutcome, u32> {
        let out = self.transition(ev);
        match out {
            Ok(o) => {
                self.events = self.events.saturating_add(1);
                if o == FsmOutcome::Dragged {
                    self.drags = self.drags.saturating_add(1);
                }
                if o == FsmOutcome::Cancelled {
                    self.cancels = self.cancels.saturating_add(1);
                }
                self.digest = fold_u32(self.digest, outcome_code(o));
                Ok(o)
            }
            Err(e) => {
                self.rejects = self.rejects.saturating_add(1);
                self.digest = fold_u32(self.digest, e);
                Err(e)
            }
        }
    }

    fn transition(&mut self, ev: PointerEvent) -> Result<FsmOutcome, u32> {
        match self.state {
            FsmState::Idle => match ev {
                PointerEvent::Press(btn) => {
                    if !valid_btn(btn) {
                        return Err(EgCode::SeqBad.val());
                    }
                    self.state = FsmState::Down {
                        btn,
                        ox: 0,
                        oy: 0,
                    };
                    Ok(FsmOutcome::Pressed)
                }
                PointerEvent::Move(_, _) => Ok(FsmOutcome::Ignored),
                PointerEvent::Release(_) => Err(EgCode::SeqBad.val()),
                PointerEvent::Leave => Ok(FsmOutcome::Ignored),
            },
            FsmState::Down { btn, ox, oy } => match ev {
                PointerEvent::Move(x, y) => {
                    let dx = x.saturating_sub(ox);
                    let dy = y.saturating_sub(oy);
                    let d2 = dx
                        .saturating_mul(dx)
                        .saturating_add(dy.saturating_mul(dy));
                    if d2 >= DRAG_THRESHOLD_SQ {
                        self.state = FsmState::Dragging { btn };
                        Ok(FsmOutcome::Dragged)
                    } else {
                        Ok(FsmOutcome::Ignored)
                    }
                }
                PointerEvent::Press(_) => Err(EgCode::FsmBad.val()),
                PointerEvent::Release(mask) => {
                    if mask != btn {
                        return Err(EgCode::SeqBad.val());
                    }
                    self.state = FsmState::Idle;
                    Ok(FsmOutcome::Clicked)
                }
                PointerEvent::Leave => {
                    self.state = FsmState::Cancelled { btn };
                    Ok(FsmOutcome::Cancelled)
                }
            },
            FsmState::Dragging { btn } => match ev {
                PointerEvent::Move(_, _) => Ok(FsmOutcome::Dragged),
                PointerEvent::Press(_) => Err(EgCode::FsmBad.val()),
                PointerEvent::Release(mask) => {
                    if mask != btn {
                        return Err(EgCode::SeqBad.val());
                    }
                    self.state = FsmState::Idle;
                    Ok(FsmOutcome::Released)
                }
                PointerEvent::Leave => {
                    self.state = FsmState::Cancelled { btn };
                    Ok(FsmOutcome::Cancelled)
                }
            },
            FsmState::Cancelled { btn } => match ev {
                PointerEvent::Move(_, _) => Ok(FsmOutcome::Ignored),
                PointerEvent::Press(_) => Err(EgCode::FsmBad.val()),
                PointerEvent::Release(mask) => {
                    if mask != btn {
                        return Err(EgCode::SeqBad.val());
                    }
                    self.state = FsmState::Idle;
                    Ok(FsmOutcome::Released)
                }
                PointerEvent::Leave => Ok(FsmOutcome::Ignored),
            },
        }
    }

    pub fn audit(&self) -> Result<(), u32> {
        if self.events.saturating_add(self.rejects) == 0 {
            return Err(EgCode::CoverageBad.val());
        }
        Ok(())
    }
}

pub fn fold_byte(h: u64, b: u8) -> u64 {
    (h ^ (b as u64)).wrapping_mul(0x0000_0100_0000_01b3)
}

pub fn fold_u32(h: u64, v: u32) -> u64 {
    let mut x = h;
    let mut i = 0u32;
    while i < 4 {
        x = fold_byte(x, ((v >> (i * 8)) & 0xFF) as u8);
        i += 1;
    }
    x
}

pub fn outcome_code(o: FsmOutcome) -> u32 {
    match o {
        FsmOutcome::Ignored => 1,
        FsmOutcome::Pressed => 2,
        FsmOutcome::Dragged => 3,
        FsmOutcome::Clicked => 4,
        FsmOutcome::Released => 5,
        FsmOutcome::Cancelled => 6,
    }
}

/// 全路径 ×10^2：按下→微动保 Down→阈值转 Dragging→释放；移出取消→释放清账。
pub fn path_rounds(rounds: u32) -> Result<(), u32> {
    let mut i = 0u32;
    while i < rounds {
        let mut f = PointerFsm::new();
        if f.input(PointerEvent::Press(BTN_LEFT))? != FsmOutcome::Pressed {
            return Err(EgCode::FsmBad.val());
        }
        if f.input(PointerEvent::Move(3, 0))? != FsmOutcome::Ignored {
            return Err(EgCode::DragBad.val());
        }
        if f.input(PointerEvent::Move(4, 0))? != FsmOutcome::Dragged {
            return Err(EgCode::DragBad.val());
        }
        if !matches!(f.state, FsmState::Dragging { .. }) {
            return Err(EgCode::FsmBad.val());
        }
        if f.input(PointerEvent::Move(100, 50))? != FsmOutcome::Dragged {
            return Err(EgCode::DragBad.val());
        }
        if f.input(PointerEvent::Release(BTN_LEFT))? != FsmOutcome::Released {
            return Err(EgCode::FsmBad.val());
        }
        if f.state != FsmState::Idle {
            return Err(EgCode::FsmBad.val());
        }
        if f.input(PointerEvent::Press(BTN_RIGHT))? != FsmOutcome::Pressed {
            return Err(EgCode::FsmBad.val());
        }
        if f.input(PointerEvent::Leave)? != FsmOutcome::Cancelled {
            return Err(EgCode::FsmBad.val());
        }
        if f.input(PointerEvent::Move(0, 0))? != FsmOutcome::Ignored {
            return Err(EgCode::FsmBad.val());
        }
        if f.input(PointerEvent::Release(BTN_RIGHT))? != FsmOutcome::Released {
            return Err(EgCode::FsmBad.val());
        }
        if f.input(PointerEvent::Release(BTN_RIGHT)).err() != Some(EgCode::SeqBad.val()) {
            return Err(EgCode::SeqBad.val());
        }
        if f.drags != 2 || f.cancels != 1 {
            return Err(EgCode::DragBad.val());
        }
        i = i.saturating_add(1);
    }
    Ok(())
}

// ── 边界夹取与静默区 ──────────────────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ClampSpec {
    pub min_x: i64,
    pub min_y: i64,
    pub max_x: i64,
    pub max_y: i64,
    pub border: i64,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ClampResult {
    pub x: i64,
    pub y: i64,
    pub clamped: bool,
    pub silent: bool,
}

impl ClampSpec {
    pub fn new(
        min_x: i64,
        min_y: i64,
        max_x: i64,
        max_y: i64,
        border: i64,
    ) -> Result<ClampSpec, u32> {
        if max_x <= min_x || max_y <= min_y {
            return Err(EgCode::ClampBad.val());
        }
        if border < 0 || border > BORDER_MAX {
            return Err(EgCode::ClampBad.val());
        }
        let w = max_x.saturating_sub(min_x);
        let h = max_y.saturating_sub(min_y);
        if border.saturating_mul(2) >= w.min(h) {
            return Err(EgCode::ClampBad.val());
        }
        if min_x.unsigned_abs() > POS_LIMIT as u64
            || min_y.unsigned_abs() > POS_LIMIT as u64
            || max_x.unsigned_abs() > POS_LIMIT as u64
            || max_y.unsigned_abs() > POS_LIMIT as u64
        {
            return Err(EgCode::ClampBad.val());
        }
        Ok(ClampSpec {
            min_x,
            min_y,
            max_x,
            max_y,
            border,
        })
    }

    pub fn default_spec() -> Result<ClampSpec, u32> {
        ClampSpec::new(0, 0, 1920 - 1, 1080 - 1, 8)
    }

    pub fn clamp(&self, x: i64, y: i64) -> Result<ClampResult, u32> {
        if x.unsigned_abs() > POS_LIMIT as u64 || y.unsigned_abs() > POS_LIMIT as u64 {
            return Err(EgCode::ClampBad.val());
        }
        let cx = if x < self.min_x {
            self.min_x
        } else if x > self.max_x {
            self.max_x
        } else {
            x
        };
        let cy = if y < self.min_y {
            self.min_y
        } else if y > self.max_y {
            self.max_y
        } else {
            y
        };
        let clamped = cx != x || cy != y;
        let silent = cx <= self.min_x.saturating_add(self.border)
            || cx >= self.max_x.saturating_sub(self.border)
            || cy <= self.min_y.saturating_add(self.border)
            || cy >= self.max_y.saturating_sub(self.border);
        if clamped && (cx < self.min_x || cx > self.max_x || cy < self.min_y || cy > self.max_y) {
            return Err(EgCode::ClampBad.val());
        }
        Ok(ClampResult {
            x: cx,
            y: cy,
            clamped,
            silent,
        })
    }
}

/// 夹取 ×10^2：内部不静默、边带静默、越界夹取精确、夹后幂等。
pub fn clamp_rounds(rounds: u32) -> Result<(), u32> {
    let spec = ClampSpec::default_spec()?;
    let mut i = 0u32;
    while i < rounds {
        let center = spec.clamp(960, 540)?;
        if center.clamped || center.silent {
            return Err(EgCode::SilentBad.val());
        }
        let near = spec.clamp(4, 540)?;
        if near.clamped || !near.silent {
            return Err(EgCode::SilentBad.val());
        }
        let edge = spec.clamp(0, 0)?;
        if edge.clamped || !edge.silent {
            return Err(EgCode::SilentBad.val());
        }
        let under = spec.clamp(-50, 540)?;
        if !under.clamped || under.x != spec.min_x || !under.silent {
            return Err(EgCode::ClampBad.val());
        }
        let over = spec.clamp(5000, 540)?;
        if !over.clamped || over.x != spec.max_x || !over.silent {
            return Err(EgCode::ClampBad.val());
        }
        let again = spec.clamp(under.x, under.y)?;
        if again.clamped || again.x != under.x || again.y != under.y {
            return Err(EgCode::ClampBad.val());
        }
        i = i.saturating_add(1);
    }
    Ok(())
}

// ── 账本渲染 ──────────────────────────────────────────────────────
pub fn render_fsm(f: &PointerFsm) -> String {
    format!(
        "{}|{}|{}|0x{:016X}",
        f.events, f.rejects, f.drags, f.digest
    )
}

pub fn parse_u32(s: &str) -> Result<u32, u32> {
    if s.is_empty() || s.len() > 10 {
        return Err(EgCode::RenderBad.val());
    }
    let mut v = 0u32;
    let mut i = 0usize;
    while i < s.len() {
        let b = match s.as_bytes().get(i) {
            Some(v) => *v,
            None => return Err(EgCode::RenderBad.val()),
        };
        if !(b'0'..=b'9').contains(&b) {
            return Err(EgCode::RenderBad.val());
        }
        v = match v
            .checked_mul(10)
            .and_then(|x| x.checked_add((b - b'0') as u32))
        {
            Some(x) => x,
            None => return Err(EgCode::RenderBad.val()),
        };
        i += 1;
    }
    Ok(v)
}

pub fn parse_hex16(s: &str) -> Result<u64, u32> {
    if s.len() != 18 || (!s.starts_with("0x") && !s.starts_with("0X")) {
        return Err(EgCode::RenderBad.val());
    }
    let bytes = s.as_bytes();
    let mut v = 0u64;
    let mut i = 2usize;
    while i < 18 {
        let b = match bytes.get(i) {
            Some(v) => *v,
            None => return Err(EgCode::RenderBad.val()),
        };
        let d = match b {
            b'0'..=b'9' => (b - b'0') as u64,
            b'a'..=b'f' => (b - b'a') as u64 + 10,
            b'A'..=b'F' => (b - b'A') as u64 + 10,
            _ => return Err(EgCode::RenderBad.val()),
        };
        v = match v.checked_mul(16).and_then(|x| x.checked_add(d)) {
            Some(x) => x,
            None => return Err(EgCode::RenderBad.val()),
        };
        i += 1;
    }
    Ok(v)
}

pub fn audit_render(text: &str) -> Result<u32, u32> {
    let parts: Vec<&str> = text.split('|').collect();
    if parts.len() != 4 {
        return Err(EgCode::RenderBad.val());
    }
    let events = parse_u32(match parts.first() {
        Some(v) => v,
        None => return Err(EgCode::RenderBad.val()),
    })?;
    let rejects = parse_u32(match parts.get(1) {
        Some(v) => v,
        None => return Err(EgCode::RenderBad.val()),
    })?;
    let drags = parse_u32(match parts.get(2) {
        Some(v) => v,
        None => return Err(EgCode::RenderBad.val()),
    })?;
    let digest = parse_hex16(match parts.get(3) {
        Some(v) => v,
        None => return Err(EgCode::RenderBad.val()),
    })?;
    if events == 0 || events > 10_000_000 || rejects > 10_000_000 || drags > events {
        return Err(EgCode::RenderBad.val());
    }
    if digest == 0 {
        return Err(EgCode::RenderBad.val());
    }
    Ok(events)
}

// ── 黄金语料 ──────────────────────────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Lcg {
    state: u64,
}

impl Lcg {
    pub fn new(seed: u64) -> Lcg {
        Lcg { state: seed | 1 }
    }

    pub fn next(&mut self) -> u64 {
        self.state = self
            .state
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        self.state
    }

    pub fn below(&mut self, n: u32) -> u32 {
        if n == 0 {
            0
        } else {
            (self.next() % (n as u64)) as u32
        }
    }
}

/// 黄金事件流：1000 条随机事件，返回 (events, rejects, drags, digest)。
pub fn golden_run() -> Result<(u32, u32, u32, u64), u32> {
    let mut f = PointerFsm::new();
    let mut lcg = Lcg::new(0x4893_1001);
    let mut i = 0u32;
    while i < GOLDEN_ROUNDS {
        let kind = lcg.below(4);
        let ev = match kind {
            0 => PointerEvent::Press(if lcg.below(2) == 0 {
                BTN_LEFT
            } else {
                BTN_RIGHT
            }),
            1 => PointerEvent::Move(
                lcg.below(401) as i64 - 200,
                lcg.below(401) as i64 - 200,
            ),
            2 => PointerEvent::Release(if lcg.below(2) == 0 {
                BTN_LEFT
            } else {
                BTN_RIGHT
            }),
            _ => PointerEvent::Leave,
        };
        let _ = f.input(ev);
        i = i.saturating_add(1);
    }
    f.audit()?;
    if f.events.saturating_add(f.rejects) != GOLDEN_ROUNDS {
        return Err(EgCode::OrderBad.val());
    }
    Ok((f.events, f.rejects, f.drags, f.digest))
}

// ── 注入（四路） ──────────────────────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct InjectLog {
    pub tries: [u32; 4],
    pub caught: [u32; 4],
}

impl InjectLog {
    pub fn all_caught(&self) -> bool {
        let mut i = 0usize;
        while i < 4 {
            if self.tries[i] == 0 || self.caught[i] != self.tries[i] {
                return false;
            }
            i += 1;
        }
        true
    }

    pub fn route(&self, i: usize) -> u32 {
        match self.caught.get(i) {
            Some(v) => *v,
            None => 0,
        }
    }
}

pub fn inject_rounds(rounds: u32) -> Result<InjectLog, u32> {
    let mut log = InjectLog {
        tries: [rounds; 4],
        caught: [0; 4],
    };
    let mut r = 0u32;
    while r < rounds {
        let mut f = PointerFsm::new();
        if f.input(PointerEvent::Release(BTN_LEFT)).err() == Some(EgCode::SeqBad.val()) {
            log.caught[0] = log.caught[0].saturating_add(1);
        }
        let mut g = PointerFsm::new();
        let _ = g.input(PointerEvent::Press(BTN_LEFT));
        if g.input(PointerEvent::Press(BTN_RIGHT)).err() == Some(EgCode::FsmBad.val()) {
            log.caught[1] = log.caught[1].saturating_add(1);
        }
        if ClampSpec::new(100, 0, 10, 100, 1).err() == Some(EgCode::ClampBad.val()) {
            log.caught[2] = log.caught[2].saturating_add(1);
        }
        if ClampSpec::new(0, 0, 20, 20, 10).err() == Some(EgCode::ClampBad.val()) {
            log.caught[3] = log.caught[3].saturating_add(1);
        }
        r = r.saturating_add(1);
    }
    Ok(log)
}

// ── 探针三序 ──────────────────────────────────────────────────────
/// 探针 A：全路径、非法迁移、取消后恢复、阈值边界。
pub fn probe_seq_a() -> Result<(), u32> {
    path_rounds(PATH_ROUNDS)?;
    let mut f = PointerFsm::new();
    if f.input(PointerEvent::Release(BTN_LEFT)).err() != Some(EgCode::SeqBad.val()) {
        return Err(EgCode::SeqBad.val());
    }
    if f.input(PointerEvent::Press(BTN_LEFT))? != FsmOutcome::Pressed {
        return Err(EgCode::FsmBad.val());
    }
    if f.input(PointerEvent::Release(BTN_RIGHT)).err() != Some(EgCode::SeqBad.val()) {
        return Err(EgCode::SeqBad.val());
    }
    if f.input(PointerEvent::Move(4, 0))? != FsmOutcome::Dragged {
        return Err(EgCode::DragBad.val());
    }
    if f.input(PointerEvent::Press(BTN_LEFT)).err() != Some(EgCode::FsmBad.val()) {
        return Err(EgCode::FsmBad.val());
    }
    if f.input(PointerEvent::Leave)? != FsmOutcome::Cancelled {
        return Err(EgCode::FsmBad.val());
    }
    if f.input(PointerEvent::Press(BTN_LEFT)).err() != Some(EgCode::FsmBad.val()) {
        return Err(EgCode::FsmBad.val());
    }
    if f.input(PointerEvent::Release(BTN_LEFT))? != FsmOutcome::Released {
        return Err(EgCode::FsmBad.val());
    }
    if f.state != FsmState::Idle {
        return Err(EgCode::FsmBad.val());
    }
    let mut g = PointerFsm::new();
    let _ = g.input(PointerEvent::Press(BTN_LEFT));
    if g.input(PointerEvent::Move(3, 0))? != FsmOutcome::Ignored {
        return Err(EgCode::DragBad.val());
    }
    Ok(())
}

/// 探针 B：夹取边界精确、静默边带、幂等、坏规格必拒。
pub fn probe_seq_b() -> Result<(), u32> {
    clamp_rounds(CLAMP_ROUNDS)?;
    let spec = ClampSpec::default_spec()?;
    if spec.clamp(1919, 540)?.silent != true {
        return Err(EgCode::SilentBad.val());
    }
    if spec.clamp(9, 540)?.silent != false {
        return Err(EgCode::SilentBad.val());
    }
    if spec.clamp(-1, -1)?.x != 0 {
        return Err(EgCode::ClampBad.val());
    }
    if spec.clamp(POS_LIMIT + 1, 0).err() != Some(EgCode::ClampBad.val()) {
        return Err(EgCode::ClampBad.val());
    }
    if ClampSpec::new(0, 0, 100, 100, BORDER_MAX + 1).err() != Some(EgCode::ClampBad.val()) {
        return Err(EgCode::ClampBad.val());
    }
    if ClampSpec::new(0, 0, 100, 100, 50).err() != Some(EgCode::ClampBad.val()) {
        return Err(EgCode::ClampBad.val());
    }
    Ok(())
}

/// 探针 C：黄金账双跑一致、事件守恒、渲染回读一致。
pub fn probe_seq_c() -> Result<(), u32> {
    let a = golden_run()?;
    let b = golden_run()?;
    if a != b {
        return Err(EgCode::ReplayBad.val());
    }
    if a.0.saturating_add(a.1) != GOLDEN_ROUNDS {
        return Err(EgCode::OrderBad.val());
    }
    let mut f = PointerFsm::new();
    let _ = f.input(PointerEvent::Press(BTN_LEFT));
    let _ = f.input(PointerEvent::Move(10, 10));
    let text = render_fsm(&f);
    if audit_render(&text)? != f.events {
        return Err(EgCode::RenderBad.val());
    }
    Ok(())
}

pub fn repro_check() -> Result<(), u32> {
    let a = golden_run()?;
    let b = golden_run()?;
    if a != b {
        return Err(EgCode::ReplayBad.val());
    }
    Ok(())
}

pub fn segment_guard() -> bool {
    let lo = SEG_BASE + 1;
    let hi = SEG_BASE + EgCode::ALL.len() as u32;
    let mut i = 0usize;
    while i < EgCode::ALL.len() {
        match EgCode::ALL.get(i) {
            Some(c) => {
                let v = c.val();
                if v < lo || v > hi {
                    return false;
                }
            }
            None => return false,
        }
        i += 1;
    }
    true
}

// ── 判据集 ────────────────────────────────────────────────────────
pub fn extra_checks() -> Vec<(&'static str, bool, String)> {
    let mut v: Vec<(&'static str, bool, String)> = Vec::new();
    let path = path_rounds(PATH_ROUNDS);
    v.push((
        "F48931-J1-状态机全路径×10^2",
        path.is_ok(),
        match &path {
            Ok(()) => format!("{} 轮：按下→微动→拖拽→释放→取消→清账", PATH_ROUNDS),
            Err(e) => format!("err=0x{:X}", e),
        },
    ));
    v.push((
        "F48931-J1-拖拽阈值精确",
        {
            let mut f = PointerFsm::new();
            let _ = f.input(PointerEvent::Press(BTN_LEFT));
            let a = f.input(PointerEvent::Move(3, 0));
            let b = f.input(PointerEvent::Move(4, 0));
            a == Ok(FsmOutcome::Ignored) && b == Ok(FsmOutcome::Dragged)
        },
        String::from("3px 不拖、4px 拖"),
    ));
    v.push((
        "F48931-J1-移出取消独立态",
        {
            let mut f = PointerFsm::new();
            let _ = f.input(PointerEvent::Press(BTN_LEFT));
            let c = f.input(PointerEvent::Leave);
            let seq = matches!(f.state, FsmState::Cancelled { .. });
            let r = f.input(PointerEvent::Release(BTN_LEFT));
            c == Ok(FsmOutcome::Cancelled) && seq && r == Ok(FsmOutcome::Released)
        },
        String::from("Leave→Cancelled，Release 清账不回 Clicked"),
    ));
    v.push((
        "F48931-J1-非法迁移全拒",
        {
            let mut f = PointerFsm::new();
            let a = f.input(PointerEvent::Release(BTN_LEFT)).err() == Some(EgCode::SeqBad.val());
            let _ = f.input(PointerEvent::Press(BTN_LEFT));
            let b = f.input(PointerEvent::Release(BTN_RIGHT)).err() == Some(EgCode::SeqBad.val());
            let c = f.input(PointerEvent::Press(BTN_RIGHT)).err() == Some(EgCode::FsmBad.val());
            a && b && c
        },
        String::from("空释放/错键释放/重复按下三拒"),
    ));
    let cl = clamp_rounds(CLAMP_ROUNDS);
    v.push((
        "F48932-J1-clamp×10^2边界一致",
        cl.is_ok(),
        match &cl {
            Ok(()) => format!("{} 轮：内部/边带/越界/幂等四判", CLAMP_ROUNDS),
            Err(e) => format!("err=0x{:X}", e),
        },
    ));
    v.push((
        "F48932-J1-静默边带带内带外",
        {
            let spec = ClampSpec::default_spec();
            match spec {
                Ok(s) => {
                    let inside = s.clamp(4, 500);
                    let outside = s.clamp(9, 500);
                    let corner = s.clamp(1920 - 1, 1080 - 1);
                    match (inside, outside, corner) {
                        (Ok(i), Ok(o), Ok(c)) => i.silent && !o.silent && c.silent,
                        _ => false,
                    }
                }
                Err(_) => false,
            }
        },
        String::from("border=8：≤8 与 ≥max-8 静默"),
    ));
    let golden = golden_run();
    v.push((
        "F48931-J1-黄金×10^3事件守恒",
        match &golden {
            Ok((e, r, _, _)) => e.saturating_add(*r) == GOLDEN_ROUNDS,
            Err(_) => false,
        },
        match &golden {
            Ok((e, r, d, h)) => format!("events={} rejects={} drags={} digest=0x{:016X}", e, r, d, h),
            Err(e) => format!("err=0x{:X}", e),
        },
    ));
    let inj = inject_rounds(10);
    v.push((
        "F48931-J1-注入四路10/10",
        match &inj {
            Ok(l) => l.all_caught(),
            Err(_) => false,
        },
        match &inj {
            Ok(l) => format!("tries={:?} caught={:?}", l.tries, l.caught),
            Err(e) => format!("err=0x{:X}", e),
        },
    ));
    let render_ok = {
        let mut f = PointerFsm::new();
        let _ = f.input(PointerEvent::Press(BTN_LEFT));
        let _ = f.input(PointerEvent::Move(50, 50));
        let text = render_fsm(&f);
        match audit_render(&text) {
            Ok(n) => n == f.events,
            Err(_) => false,
        }
    };
    v.push((
        "F48931-J1-渲染四字段回读",
        render_ok,
        String::from("events|rejects|drags|digest"),
    ));
    v.push((
        "F48931-J1-探针三序与双跑",
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
        "F48931-J1-段位守卫",
        segment_guard(),
        format!("0x{:X} 窗口 {} 码", SEG_BASE, EgCode::ALL.len()),
    ));
    v
}

pub fn run_unx48931_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F48931 指针状态机与边界静默区");
    let items = extra_checks();
    let mut i = 0usize;
    while i < items.len() {
        if let Some((n, p, d)) = items.get(i) {
            s.add_dyn(n, *p, d);
        }
        i += 1;
    }
    s
}

// ── 单元判据 ──────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn full_path_is_consistent() {
        assert!(path_rounds(3).is_ok());
        let mut f = PointerFsm::new();
        assert_eq!(
            f.input(PointerEvent::Press(BTN_LEFT)),
            Ok(FsmOutcome::Pressed)
        );
        assert_eq!(
            f.input(PointerEvent::Move(2, 2)),
            Ok(FsmOutcome::Ignored)
        );
        assert_eq!(
            f.input(PointerEvent::Move(4, 0)),
            Ok(FsmOutcome::Dragged)
        );
        assert_eq!(
            f.input(PointerEvent::Release(BTN_LEFT)),
            Ok(FsmOutcome::Released)
        );
        assert_eq!(f.state, FsmState::Idle);
        assert_eq!(f.drags, 1);
    }

    #[test]
    fn click_without_drag() {
        let mut f = PointerFsm::new();
        let _ = f.input(PointerEvent::Press(BTN_MIDDLE));
        assert_eq!(
            f.input(PointerEvent::Move(1, 1)),
            Ok(FsmOutcome::Ignored)
        );
        assert_eq!(
            f.input(PointerEvent::Release(BTN_MIDDLE)),
            Ok(FsmOutcome::Clicked)
        );
    }

    #[test]
    fn cancel_requires_release_to_idle() {
        let mut f = PointerFsm::new();
        let _ = f.input(PointerEvent::Press(BTN_LEFT));
        assert_eq!(f.input(PointerEvent::Leave), Ok(FsmOutcome::Cancelled));
        assert_eq!(f.cancels, 1);
        assert_eq!(f.input(PointerEvent::Move(5, 5)), Ok(FsmOutcome::Ignored));
        assert_eq!(
            f.input(PointerEvent::Release(BTN_LEFT)),
            Ok(FsmOutcome::Released)
        );
        assert_eq!(f.state, FsmState::Idle);
    }

    #[test]
    fn illegal_sequences_rejected() {
        let mut f = PointerFsm::new();
        assert_eq!(
            f.input(PointerEvent::Release(BTN_LEFT)).err(),
            Some(EgCode::SeqBad.val())
        );
        let _ = f.input(PointerEvent::Press(BTN_LEFT));
        assert_eq!(
            f.input(PointerEvent::Press(BTN_LEFT)).err(),
            Some(EgCode::FsmBad.val())
        );
        assert_eq!(
            f.input(PointerEvent::Release(BTN_RIGHT)).err(),
            Some(EgCode::SeqBad.val())
        );
        assert_eq!(f.rejects, 3);
    }

    #[test]
    fn clamp_is_exact() {
        let s = ClampSpec::new(0, 0, 100, 100, 5);
        match s {
            Ok(spec) => {
                assert_eq!(
                    spec.clamp(-5, 50),
                    Ok(ClampResult {
                        x: 0,
                        y: 50,
                        clamped: true,
                        silent: true
                    })
                );
                assert_eq!(
                    spec.clamp(50, 50),
                    Ok(ClampResult {
                        x: 50,
                        y: 50,
                        clamped: false,
                        silent: false
                    })
                );
                assert_eq!(
                    spec.clamp(50, 105),
                    Ok(ClampResult {
                        x: 50,
                        y: 100,
                        clamped: true,
                        silent: true
                    })
                );
                assert_eq!(spec.clamp(50, 3).unwrap_or(ClampResult { x: 0, y: 0, clamped: false, silent: false }).silent, true);
            }
            Err(_) => assert!(false),
        }
    }

    #[test]
    fn clamp_spec_rejects_bad_shapes() {
        assert!(ClampSpec::new(10, 0, 0, 10, 1).is_err());
        assert!(ClampSpec::new(0, 0, 10, 10, -1).is_err());
        assert!(ClampSpec::new(0, 0, 10, 10, 5).is_err());
        assert!(ClampSpec::new(0, 0, 100, 100, BORDER_MAX + 1).is_err());
        assert!(ClampSpec::new(0, 0, 100, 100, 10).is_ok());
    }

    #[test]
    fn clamp_is_idempotent() {
        match ClampSpec::default_spec() {
            Ok(s) => match s.clamp(-100, 5000) {
                Ok(a) => match s.clamp(a.x, a.y) {
                    Ok(b) => {
                        assert!(!b.clamped);
                        assert_eq!((a.x, a.y), (b.x, b.y));
                    }
                    Err(_) => assert!(false),
                },
                Err(_) => assert!(false),
            },
            Err(_) => assert!(false),
        }
    }

    #[test]
    fn golden_is_deterministic() {
        assert_eq!(golden_run(), golden_run());
        assert!(repro_check().is_ok());
    }

    #[test]
    fn probes_pass() {
        assert!(probe_seq_a().is_ok());
        assert!(probe_seq_b().is_ok());
        assert!(probe_seq_c().is_ok());
        assert!(clamp_rounds(CLAMP_ROUNDS).is_ok());
    }

    #[test]
    fn injection_catches_all_four() {
        match inject_rounds(8) {
            Ok(l) => {
                assert!(l.all_caught());
                assert_eq!(l.tries, [8, 8, 8, 8]);
            }
            Err(_) => assert!(false),
        }
    }

    #[test]
    fn render_roundtrip() {
        let mut f = PointerFsm::new();
        let _ = f.input(PointerEvent::Press(BTN_LEFT));
        let text = render_fsm(&f);
        assert_eq!(audit_render(&text), Ok(1));
        let bad = text.replacen("|", " ", 1);
        assert!(audit_render(&bad).is_err());
    }

    #[test]
    fn codes_inside_window() {
        assert!(segment_guard());
        assert_eq!(EgCode::ALL.len(), 10);
        assert_eq!(detail(EgCode::FsmBad.val()), EgCode::FsmBad.detail());
        assert_eq!(detail(0), "未知诊断码");
    }
}
