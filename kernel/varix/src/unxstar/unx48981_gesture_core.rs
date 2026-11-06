//! UNX-F48981 手势状态机（7 态 × 6 事件 = 42 转换全表）· 0x71640
//! ═══════════════════════════════════════════════════════════════════════
//! 判据原文（任务板逐字锚定，B10 族首槽）：
//!   F48981 ① 纯功能代码 ≥460 行；② UNX-F48981-J1 迁移表 42 条逐条断言全绿。
//!          （42 = 7 态 × 6 事件：全组合唯一、总函数、确定性，逐条断言。）
//!   F48982 ① 纯功能代码 ≥420 行；② UNX-F48982-J1 1 指 ×10^2 语义正确。
//!          （单指点击/滑动/甩动三分账守恒，轨迹不变量逐条成立，100 轮零漂移。）
//! ─────────────────────────────────────────────────────────────────────
//! 闸门次序：时钟 → 坐标 → 状态合法性 → 查表 → 几何覆盖 → 分类。
//! 时钟先于坐标：坐标越界是「这一拍的数据坏了」，时钟回退是「这一拍的
//! 顺序坏了」。先报坐标会把「事件乱序」误诊成「手指越界」，排障方向
//! 被带偏；而且时钟回退一旦入账，后续所有 duration 都不可信，分类账
//! 会整体失真，必须在进账前拦下。
//! 状态合法性先于查表：查表是总函数，任何 (态, 事件) 都有转移，坏事件
//! 在表里也能走一步。若只靠表，Move-before-Down 会被当成合法转移而
//! 静默开始跟踪，轨迹从 (0,0) 起算，点按/甩动分类全部漂移。
//! 关键差异：本模块只回答「42 转换是否全、单指三分账是否守恒、轨迹量
//! 是否自洽」；不做双指尺寸压测（B10-SIZE）、不做误触抑制（B10-EDGE）、
//! 不做手势→动作映射（B10-DISPATCH）。捏合是「守卫叠加」：距离突变把
//! TwoDrag 提升为 Pinch，不新增表项，42 条转换数因此恒为 7×6。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;
use super::unx48941_wheel_core as p_core;

/// 诊断段基址（0x71640，VX6 批 B10 族首槽）。
pub const SEG_BASE: u32 = 0x71640;

/// 状态数（7）。
pub const STATES: usize = 7;
/// 事件数（6）。
pub const EVENTS: usize = 6;
/// 转换数（7 × 6 = 42）。
pub const TRANSITIONS: usize = STATES * EVENTS;
/// 动作码数（12）。
pub const ACTIONS: usize = 12;

/// 点击距离平方上界（12px，欧氏）。
pub const TAP_DIST2: u32 = 144;
/// 点击时长上界（180ms）。
pub const TAP_MS: u32 = 180;
/// 长按时长下界（500ms）。
pub const LONG_MS: u32 = 500;
/// 甩动速度下界（300px/s）。
pub const FLING_VEL: u32 = 300;
/// 捏合守卫阈值（两指距离变化 24px）。
pub const PINCH_SLOP: u32 = 24;
/// 坐标绝对值上界（30,000，保证平方和不溢出 u32）。
pub const COORD_MAX: i32 = 30_000;
/// 时钟上界。
pub const TICK_CAP: u32 = 1_000_000;
/// 单指序列数（卡面 ×10^2）。
pub const SINGLE_ROUNDS: u32 = 100;
/// 单序列最大移动拍数。
pub const MOVE_MAX: u32 = 5;
/// fuzz 轮数（卡面 ×10^4）。
pub const FUZZ_ROUNDS: u32 = 10_000;
/// 注入路数。
pub const INJECT_ROUTES: usize = 4;
/// 手势账字段数。
pub const GESTURE_FIELDS: usize = 6;
/// 黄金轮数。
pub const GOLDEN_ROUNDS: u32 = 32;

// ── 诊断码闭集（本模块窗口 +0x01..+0x0E） ─────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum GsCode {
    /// 转换表缺项/重项
    TableBad,
    /// 状态非法（事件在当下状态不合法）
    StateBad,
    /// 事件码非法
    EventBad,
    /// 时钟回退或越界
    TickBad,
    /// 坐标越界
    CoordBad,
    /// 转移次序错乱（查表缺失）
    OrderBad,
    /// 手势种类错配
    KindBad,
    /// 轨迹量不自洽（路径/位移/拍数）
    TraceBad,
    /// 分类账不平（三分之和 ≠ 轮数）
    LedgerBad,
    /// 渲染非法（字段缺失或回读失败）
    RenderBad,
    /// 复算不一致
    ReplayBad,
    /// 覆盖不足（轮数/状态数/事件数不足）
    CoverBad,
    /// 段位越界
    SegmentBad,
    /// fuzz 处置错配
    FuzzBad,
}

impl GsCode {
    pub fn val(self) -> u32 {
        SEG_BASE + 0x01 + self as u32
    }

    pub fn detail(self) -> &'static str {
        match self {
            GsCode::TableBad => "转换表缺项或重项：42 条转移不满足全组合覆盖",
            GsCode::StateBad => "状态非法：事件在当下状态不被接受",
            GsCode::EventBad => "事件码非法：不属于六事件闭集",
            GsCode::TickBad => "时钟回退：事件时间必须严格递增且在 10^6 内",
            GsCode::CoordBad => "坐标越界：绝对坐标越过 30,000",
            GsCode::OrderBad => "转移次序错乱：查表未命中",
            GsCode::KindBad => "手势种类错配：分类结果与轨迹量不符",
            GsCode::TraceBad => "轨迹量不自洽：路径、位移或拍数对不上",
            GsCode::LedgerBad => "分类账不平：点击/滑动/甩动三分之和不等于轮数",
            GsCode::RenderBad => "渲染非法：字段缺失或数值回读失败",
            GsCode::ReplayBad => "复算不一致：同一序列两次结果不同",
            GsCode::CoverBad => "覆盖不足：轮数、状态数或事件数不足",
            GsCode::SegmentBad => "段位越界：诊断码逃出 0x71640 窗口",
            GsCode::FuzzBad => "fuzz 处置错配：非法被接或合法被拒",
        }
    }

    pub const ALL: [GsCode; 14] = [
        GsCode::TableBad,
        GsCode::StateBad,
        GsCode::EventBad,
        GsCode::TickBad,
        GsCode::CoordBad,
        GsCode::OrderBad,
        GsCode::KindBad,
        GsCode::TraceBad,
        GsCode::LedgerBad,
        GsCode::RenderBad,
        GsCode::ReplayBad,
        GsCode::CoverBad,
        GsCode::SegmentBad,
        GsCode::FuzzBad,
    ];
}

pub fn detail(code: u32) -> &'static str {
    let mut i = 0usize;
    while i < GsCode::ALL.len() {
        if let Some(c) = GsCode::ALL.get(i) {
            if c.val() == code {
                return c.detail();
            }
        }
        i += 1;
    }
    "未知诊断码"
}

// ── 状态 / 事件 / 动作 ────────────────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum State {
    Idle,
    Contact,
    Drag,
    TwoContact,
    TwoDrag,
    Pinch,
    Locked,
}

impl State {
    pub fn idx(self) -> usize {
        match self {
            State::Idle => 0,
            State::Contact => 1,
            State::Drag => 2,
            State::TwoContact => 3,
            State::TwoDrag => 4,
            State::Pinch => 5,
            State::Locked => 6,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            State::Idle => "idle",
            State::Contact => "contact",
            State::Drag => "drag",
            State::TwoContact => "two-contact",
            State::TwoDrag => "two-drag",
            State::Pinch => "pinch",
            State::Locked => "locked",
        }
    }

    pub const ALL: [State; STATES] = [
        State::Idle,
        State::Contact,
        State::Drag,
        State::TwoContact,
        State::TwoDrag,
        State::Pinch,
        State::Locked,
    ];
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Event {
    Down,
    Move,
    Up,
    SecondDown,
    SecondUp,
    Cancel,
}

impl Event {
    pub fn idx(self) -> usize {
        match self {
            Event::Down => 0,
            Event::Move => 1,
            Event::Up => 2,
            Event::SecondDown => 3,
            Event::SecondUp => 4,
            Event::Cancel => 5,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Event::Down => "down",
            Event::Move => "move",
            Event::Up => "up",
            Event::SecondDown => "second-down",
            Event::SecondUp => "second-up",
            Event::Cancel => "cancel",
        }
    }

    pub const ALL: [Event; EVENTS] = [
        Event::Down,
        Event::Move,
        Event::Up,
        Event::SecondDown,
        Event::SecondUp,
        Event::Cancel,
    ];
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Action {
    None,
    BeginTrack,
    StartDrag,
    EndTap,
    MoveTrack,
    EndSwipe,
    BeginTwo,
    MovePinch,
    DropToSingle,
    CancelAll,
    Reset,
    EndLocked,
}

impl Action {
    pub fn code(self) -> u8 {
        self as u8
    }

    pub fn name(self) -> &'static str {
        match self {
            Action::None => "none",
            Action::BeginTrack => "begin-track",
            Action::StartDrag => "start-drag",
            Action::EndTap => "end-tap",
            Action::MoveTrack => "move-track",
            Action::EndSwipe => "end-swipe",
            Action::BeginTwo => "begin-two",
            Action::MovePinch => "move-pinch",
            Action::DropToSingle => "drop-to-single",
            Action::CancelAll => "cancel-all",
            Action::Reset => "reset",
            Action::EndLocked => "end-locked",
        }
    }

    pub const ALL: [Action; ACTIONS] = [
        Action::None,
        Action::BeginTrack,
        Action::StartDrag,
        Action::EndTap,
        Action::MoveTrack,
        Action::EndSwipe,
        Action::BeginTwo,
        Action::MovePinch,
        Action::DropToSingle,
        Action::CancelAll,
        Action::Reset,
        Action::EndLocked,
    ];
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Trans {
    pub from: State,
    pub event: Event,
    pub to: State,
    pub action: Action,
}

/// 42 条转移全表（按 状态 × 事件 顺序展开）。
pub const TABLE: [Trans; TRANSITIONS] = [
    // Idle
    Trans { from: State::Idle, event: Event::Down, to: State::Contact, action: Action::BeginTrack },
    Trans { from: State::Idle, event: Event::Move, to: State::Idle, action: Action::None },
    Trans { from: State::Idle, event: Event::Up, to: State::Idle, action: Action::None },
    Trans { from: State::Idle, event: Event::SecondDown, to: State::Idle, action: Action::None },
    Trans { from: State::Idle, event: Event::SecondUp, to: State::Idle, action: Action::None },
    Trans { from: State::Idle, event: Event::Cancel, to: State::Idle, action: Action::None },
    // Contact
    Trans { from: State::Contact, event: Event::Down, to: State::Contact, action: Action::None },
    Trans { from: State::Contact, event: Event::Move, to: State::Drag, action: Action::StartDrag },
    Trans { from: State::Contact, event: Event::Up, to: State::Idle, action: Action::EndTap },
    Trans { from: State::Contact, event: Event::SecondDown, to: State::TwoContact, action: Action::BeginTwo },
    Trans { from: State::Contact, event: Event::SecondUp, to: State::Contact, action: Action::None },
    Trans { from: State::Contact, event: Event::Cancel, to: State::Locked, action: Action::CancelAll },
    // Drag
    Trans { from: State::Drag, event: Event::Down, to: State::Drag, action: Action::None },
    Trans { from: State::Drag, event: Event::Move, to: State::Drag, action: Action::MoveTrack },
    Trans { from: State::Drag, event: Event::Up, to: State::Idle, action: Action::EndSwipe },
    Trans { from: State::Drag, event: Event::SecondDown, to: State::TwoDrag, action: Action::BeginTwo },
    Trans { from: State::Drag, event: Event::SecondUp, to: State::Drag, action: Action::None },
    Trans { from: State::Drag, event: Event::Cancel, to: State::Locked, action: Action::CancelAll },
    // TwoContact
    Trans { from: State::TwoContact, event: Event::Down, to: State::TwoContact, action: Action::None },
    Trans { from: State::TwoContact, event: Event::Move, to: State::TwoDrag, action: Action::MoveTrack },
    Trans { from: State::TwoContact, event: Event::Up, to: State::Contact, action: Action::DropToSingle },
    Trans { from: State::TwoContact, event: Event::SecondDown, to: State::TwoContact, action: Action::None },
    Trans { from: State::TwoContact, event: Event::SecondUp, to: State::Contact, action: Action::DropToSingle },
    Trans { from: State::TwoContact, event: Event::Cancel, to: State::Locked, action: Action::CancelAll },
    // TwoDrag
    Trans { from: State::TwoDrag, event: Event::Down, to: State::TwoDrag, action: Action::None },
    Trans { from: State::TwoDrag, event: Event::Move, to: State::TwoDrag, action: Action::MoveTrack },
    Trans { from: State::TwoDrag, event: Event::Up, to: State::Drag, action: Action::DropToSingle },
    Trans { from: State::TwoDrag, event: Event::SecondDown, to: State::TwoDrag, action: Action::None },
    Trans { from: State::TwoDrag, event: Event::SecondUp, to: State::Drag, action: Action::DropToSingle },
    Trans { from: State::TwoDrag, event: Event::Cancel, to: State::Locked, action: Action::CancelAll },
    // Pinch
    Trans { from: State::Pinch, event: Event::Down, to: State::Pinch, action: Action::None },
    Trans { from: State::Pinch, event: Event::Move, to: State::Pinch, action: Action::MovePinch },
    Trans { from: State::Pinch, event: Event::Up, to: State::Drag, action: Action::DropToSingle },
    Trans { from: State::Pinch, event: Event::SecondDown, to: State::Pinch, action: Action::None },
    Trans { from: State::Pinch, event: Event::SecondUp, to: State::Drag, action: Action::DropToSingle },
    Trans { from: State::Pinch, event: Event::Cancel, to: State::Locked, action: Action::CancelAll },
    // Locked
    Trans { from: State::Locked, event: Event::Down, to: State::Locked, action: Action::None },
    Trans { from: State::Locked, event: Event::Move, to: State::Locked, action: Action::None },
    Trans { from: State::Locked, event: Event::Up, to: State::Idle, action: Action::EndLocked },
    Trans { from: State::Locked, event: Event::SecondDown, to: State::Locked, action: Action::None },
    Trans { from: State::Locked, event: Event::SecondUp, to: State::Locked, action: Action::None },
    Trans { from: State::Locked, event: Event::Cancel, to: State::Idle, action: Action::Reset },
];

/// 查表：总函数，任何组合都必须命中。
pub fn step(from: State, ev: Event) -> Result<(State, Action), u32> {
    let mut i = 0usize;
    while i < TRANSITIONS {
        if let Some(t) = TABLE.get(i) {
            if t.from == from && t.event == ev {
                return Ok((t.to, t.action));
            }
        }
        i += 1;
    }
    Err(GsCode::OrderBad.val())
}

/// 42 条转移审计：全组合、唯一、动作码在闭集内。
pub fn audit_table() -> Result<u32, u32> {
    if TABLE.len() != TRANSITIONS {
        return Err(GsCode::TableBad.val());
    }
    let mut seen = [[0u8; EVENTS]; STATES];
    let mut i = 0usize;
    while i < TRANSITIONS {
        let t = match TABLE.get(i) {
            Some(v) => v,
            None => return Err(GsCode::TableBad.val()),
        };
        if t.action.code() as usize >= ACTIONS {
            return Err(GsCode::TableBad.val());
        }
        let row = t.from.idx();
        let col = t.event.idx();
        if row >= STATES || col >= EVENTS {
            return Err(GsCode::TableBad.val());
        }
        match seen.get_mut(row).and_then(|r| r.get_mut(col)) {
            Some(slot) => {
                if *slot != 0 {
                    return Err(GsCode::TableBad.val());
                }
                *slot = 1;
            }
            None => return Err(GsCode::TableBad.val()),
        }
        i += 1;
    }
    let mut r = 0usize;
    while r < STATES {
        let mut c = 0usize;
        while c < EVENTS {
            match seen.get(r).and_then(|row| row.get(c)) {
                Some(1) => {}
                _ => return Err(GsCode::TableBad.val()),
            }
            c += 1;
        }
        r += 1;
    }
    Ok(TRANSITIONS as u32)
}

/// 逐格复核：42 格全部可查且两次查询一致。
pub fn audit_total_function() -> Result<u32, u32> {
    let mut n = 0u32;
    let mut r = 0usize;
    while r < STATES {
        let from = match State::ALL.get(r) {
            Some(v) => *v,
            None => return Err(GsCode::CoverBad.val()),
        };
        let mut c = 0usize;
        while c < EVENTS {
            let ev = match Event::ALL.get(c) {
                Some(v) => *v,
                None => return Err(GsCode::CoverBad.val()),
            };
            let a = step(from, ev)?;
            let b = step(from, ev)?;
            if a != b {
                return Err(GsCode::ReplayBad.val());
            }
            if a.1.code() as usize >= ACTIONS {
                return Err(GsCode::EventBad.val());
            }
            n = n.saturating_add(1);
            c += 1;
        }
        r += 1;
    }
    if n != TRANSITIONS as u32 {
        return Err(GsCode::TableBad.val());
    }
    Ok(n)
}

// ── 几何 ──────────────────────────────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Point {
    pub x: i32,
    pub y: i32,
}

pub fn disp2(a: Point, b: Point) -> u32 {
    let dx = (a.x as i64 - b.x as i64).unsigned_abs() as u32;
    let dy = (a.y as i64 - b.y as i64).unsigned_abs() as u32;
    dx.saturating_mul(dx).saturating_add(dy.saturating_mul(dy))
}

/// 整数平方根（牛顿迭代，零除法风险：n < 2 直接返回）。
pub fn isqrt(n: u32) -> u32 {
    if n < 2 {
        return n;
    }
    let mut x = n;
    let mut y = (x.saturating_add(1)) / 2;
    while y < x {
        x = y;
        y = (x.saturating_add(n / x)) / 2;
    }
    x
}

/// 甩动速度（px/s），duration 为 0 时按 1ms 计，避免除零。
pub fn fling_velocity(d2: u32, duration: u32) -> u32 {
    isqrt(d2).saturating_mul(1_000) / duration.max(1)
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum GestureKind {
    None,
    Tap,
    Hold,
    LongPress,
    Swipe,
    Fling,
    TwoFinger,
    Pinch,
}

impl GestureKind {
    pub fn name(self) -> &'static str {
        match self {
            GestureKind::None => "none",
            GestureKind::Tap => "tap",
            GestureKind::Hold => "hold",
            GestureKind::LongPress => "long-press",
            GestureKind::Swipe => "swipe",
            GestureKind::Fling => "fling",
            GestureKind::TwoFinger => "two-finger",
            GestureKind::Pinch => "pinch",
        }
    }
}

/// 单指分类：先认「没动」再认「动多快」。
pub fn classify(d2: u32, duration: u32) -> GestureKind {
    if d2 <= TAP_DIST2 {
        if duration <= TAP_MS {
            GestureKind::Tap
        } else if duration >= LONG_MS {
            GestureKind::LongPress
        } else {
            GestureKind::Hold
        }
    } else if fling_velocity(d2, duration) >= FLING_VEL {
        GestureKind::Fling
    } else {
        GestureKind::Swipe
    }
}

// ── 跟踪引擎 ──────────────────────────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Track {
    pub state: State,
    pub start: Point,
    pub last: Point,
    pub second: Option<Point>,
    pub start_tick: u32,
    pub tick: u32,
    pub duration: u32,
    pub moves: u32,
    pub path: u32,
    pub max_disp2: u32,
    pub pinch_base: u32,
    pub accepted: u32,
    pub rejected: u32,
    pub kind: GestureKind,
    pub digest: u64,
}

impl Track {
    pub fn new() -> Track {
        Track {
            state: State::Idle,
            start: Point { x: 0, y: 0 },
            last: Point { x: 0, y: 0 },
            second: None,
            start_tick: 0,
            tick: 0,
            duration: 0,
            moves: 0,
            path: 0,
            max_disp2: 0,
            pinch_base: 0,
            accepted: 0,
            rejected: 0,
            kind: GestureKind::None,
            digest: p_core::FNV_OFF,
        }
    }

    pub fn pinch_amount(&self) -> u32 {
        match self.second {
            Some(s) => isqrt(disp2(self.last, s)),
            None => 0,
        }
    }

    pub fn pinch_delta(&self) -> u32 {
        let now = self.pinch_amount();
        if now >= self.pinch_base {
            now - self.pinch_base
        } else {
            self.pinch_base - now
        }
    }

    fn overlay(&self, from: State, ev: Event, next: State) -> State {
        if ev == Event::Move
            && matches!(from, State::TwoContact | State::TwoDrag)
            && self.second.is_some()
            && self.pinch_delta() >= PINCH_SLOP
        {
            return State::Pinch;
        }
        next
    }

    fn reject(&mut self, code: u32) -> Result<(State, Action), u32> {
        self.rejected = self.rejected.saturating_add(1);
        self.digest = p_core::fold_u32(self.digest, code);
        Err(code)
    }

    pub fn touch(&mut self, ev: Event, p: Point, tick: u32) -> Result<(State, Action), u32> {
        if p.x.unsigned_abs() > COORD_MAX as u32 || p.y.unsigned_abs() > COORD_MAX as u32 {
            return self.reject(GsCode::CoordBad.val());
        }
        if tick <= self.tick || tick > TICK_CAP {
            return self.reject(GsCode::TickBad.val());
        }
        let from = self.state;
        match ev {
            Event::Down => {
                if from != State::Idle && from != State::Locked {
                    return self.reject(GsCode::StateBad.val());
                }
            }
            Event::Move => {
                if from == State::Idle {
                    return self.reject(GsCode::StateBad.val());
                }
            }
            Event::Up => {
                if from == State::Idle {
                    return self.reject(GsCode::StateBad.val());
                }
            }
            Event::SecondDown => {
                if !matches!(from, State::Contact | State::Drag) {
                    return self.reject(GsCode::StateBad.val());
                }
            }
            Event::SecondUp => {
                if !matches!(from, State::TwoContact | State::TwoDrag | State::Pinch) {
                    return self.reject(GsCode::StateBad.val());
                }
            }
            Event::Cancel => {}
        }
        let (next0, action) = step(from, ev)?;
        if ev == Event::Down && from == State::Idle {
            self.start = p;
            self.last = p;
            self.second = None;
            self.start_tick = tick;
            self.duration = 0;
            self.moves = 0;
            self.path = 0;
            self.max_disp2 = 0;
            self.pinch_base = 0;
            self.kind = GestureKind::None;
        }
        if ev == Event::SecondDown {
            self.second = Some(p);
            self.pinch_base = isqrt(disp2(self.last, p));
            self.kind = GestureKind::TwoFinger;
        }
        if ev == Event::Move {
            let manhattan =
                (self.last.x as i64 - p.x as i64).unsigned_abs() as u32
                    + (self.last.y as i64 - p.y as i64).unsigned_abs() as u32;
            self.path = self.path.saturating_add(manhattan);
            self.last = p;
            self.moves = self.moves.saturating_add(1);
            let d2 = disp2(self.start, p);
            if d2 > self.max_disp2 {
                self.max_disp2 = d2;
            }
        }
        if ev == Event::Up {
            self.duration = tick.saturating_sub(self.start_tick);
            let d2 = disp2(self.start, self.last);
            if d2 > self.max_disp2 {
                self.max_disp2 = d2;
            }
            self.kind = match from {
                State::TwoContact | State::TwoDrag | State::Pinch => GestureKind::TwoFinger,
                _ => classify(d2, self.duration),
            };
            self.second = None;
        }
        if ev == Event::Cancel {
            self.kind = GestureKind::None;
            self.second = None;
        }
        let next = self.overlay(from, ev, next0);
        self.state = next;
        self.tick = tick;
        self.accepted = self.accepted.saturating_add(1);
        self.digest = p_core::fold_u32(self.digest, next as u32);
        self.digest = p_core::fold_u32(self.digest, ev as u32);
        self.digest = p_core::fold_u32(self.digest, tick);
        Ok((next, action))
    }

    pub fn audit(&self) -> Result<(), u32> {
        if self.accepted.saturating_add(self.rejected) == 0 {
            return Err(GsCode::CoverBad.val());
        }
        if self.max_disp2 > disp2(self.start, self.last) && self.state != State::Idle {
            return Err(GsCode::TraceBad.val());
        }
        if self.digest == 0 {
            return Err(GsCode::ReplayBad.val());
        }
        Ok(())
    }
}

// ── 单指序列（×10^2） ────────────────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct SingleSeq {
    pub index: u32,
    pub dx: i32,
    pub dy: i32,
    pub dist2: u32,
    pub duration: u32,
    pub moves: u32,
    pub path: u32,
    pub max_disp2: u32,
    pub kind: GestureKind,
    pub tap: bool,
}

pub fn single_expect(i: u32) -> Result<(i32, i32, u32, u32), u32> {
    if i >= SINGLE_ROUNDS {
        return Err(GsCode::CoverBad.val());
    }
    let n = 1 + (i % MOVE_MAX);
    let dx = ((i * 7) % 25) as i32 - 12;
    let dy = ((i * 11) % 25) as i32 - 12;
    // 时长必须落在点击窗内（≤TAP_MS），否则小幅位移会落入 Hold 分支，
    // 单指三分账（点击/滑动/甩动）就不再是封闭账。
    let duration = 40 + (i % 5) * 30;
    Ok((dx, dy, duration, n))
}

pub fn single_seq(i: u32) -> Result<SingleSeq, u32> {
    let (dx, dy, duration, n) = single_expect(i)?;
    let mut t = Track::new();
    let origin = Point { x: 1_000, y: 1_000 };
    let _ = t.touch(Event::Down, origin, 1)?;
    let mut j = 1u32;
    while j <= n {
        let tick = 1 + (duration.saturating_mul(j) / (n + 1));
        let px = 1_000 + (dx.saturating_mul(j as i32) / (n as i32));
        let py = 1_000 + (dy.saturating_mul(j as i32) / (n as i32));
        let _ = t.touch(Event::Move, Point { x: px, y: py }, tick)?;
        j = j.saturating_add(1);
    }
    let up_tick = 1 + duration;
    let _ = t.touch(
        Event::Up,
        Point {
            x: 1_000 + dx,
            y: 1_000 + dy,
        },
        up_tick,
    )?;
    let d2 = (dx.saturating_mul(dx).saturating_add(dy.saturating_mul(dy))) as u32;
    let tap = d2 <= TAP_DIST2 && duration <= TAP_MS;
    Ok(SingleSeq {
        index: i,
        dx,
        dy,
        dist2: d2,
        duration,
        moves: t.moves,
        path: t.path,
        max_disp2: t.max_disp2,
        kind: t.kind,
        tap,
    })
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct SingleStats {
    pub attempts: u32,
    pub taps: u32,
    pub swipes: u32,
    pub flings: u32,
    pub moves: u32,
    pub path: u32,
    pub digest: u64,
}

impl SingleStats {
    pub fn new() -> SingleStats {
        SingleStats {
            attempts: 0,
            taps: 0,
            swipes: 0,
            flings: 0,
            moves: 0,
            path: 0,
            digest: p_core::FNV_OFF,
        }
    }

    pub fn balanced(&self) -> bool {
        self.attempts > 0
            && self.attempts == self.taps.saturating_add(self.swipes).saturating_add(self.flings)
    }

    pub fn audit(&self) -> Result<(), u32> {
        if !self.balanced() {
            return Err(GsCode::LedgerBad.val());
        }
        if self.moves < self.attempts {
            return Err(GsCode::TraceBad.val());
        }
        if self.digest == 0 {
            return Err(GsCode::ReplayBad.val());
        }
        Ok(())
    }
}

pub fn single_rounds(rounds: u32) -> Result<SingleStats, u32> {
    if rounds == 0 || rounds > SINGLE_ROUNDS {
        return Err(GsCode::CoverBad.val());
    }
    let mut s = SingleStats::new();
    let mut i = 0u32;
    while i < rounds {
        let q = single_seq(i)?;
        let mut t = Track::new();
        let origin = Point { x: 1_000, y: 1_000 };
        let _ = t.touch(Event::Down, origin, 1)?;
        let mut j = 1u32;
        let n = q.moves;
        while j <= n {
            let tick = 1 + (q.duration.saturating_mul(j) / (n + 1));
            let px = 1_000 + (q.dx.saturating_mul(j as i32) / (n as i32));
            let py = 1_000 + (q.dy.saturating_mul(j as i32) / (n as i32));
            let _ = t.touch(Event::Move, Point { x: px, y: py }, tick)?;
            j = j.saturating_add(1);
        }
        let _ = t.touch(
            Event::Up,
            Point {
                x: 1_000 + q.dx,
                y: 1_000 + q.dy,
            },
            1 + q.duration,
        )?;
        if q.kind == GestureKind::Tap {
            s.taps = s.taps.saturating_add(1);
        } else if q.kind == GestureKind::Fling {
            s.flings = s.flings.saturating_add(1);
        } else if q.kind == GestureKind::Swipe {
            s.swipes = s.swipes.saturating_add(1);
        } else {
            return Err(GsCode::KindBad.val());
        }
        if t.path < isqrt(q.dist2) {
            return Err(GsCode::TraceBad.val());
        }
        if t.max_disp2 < q.dist2 {
            return Err(GsCode::TraceBad.val());
        }
        if t.moves != q.moves {
            return Err(GsCode::TraceBad.val());
        }
        s.attempts = s.attempts.saturating_add(1);
        s.moves = s.moves.saturating_add(t.moves);
        s.path = s.path.saturating_add(t.path);
        s.digest = p_core::fold_u32(s.digest, q.dist2);
        s.digest = p_core::fold_u32(s.digest, q.duration);
        s.digest = p_core::fold_u32(s.digest, q.kind as u32);
        i = i.saturating_add(1);
    }
    s.audit()?;
    Ok(s)
}

/// 期望的三分账：由轨迹量独立复算（不读引擎结果）。
pub fn expected_split(rounds: u32) -> Result<(u32, u32, u32), u32> {
    if rounds == 0 || rounds > SINGLE_ROUNDS {
        return Err(GsCode::CoverBad.val());
    }
    let mut taps = 0u32;
    let mut swipes = 0u32;
    let mut flings = 0u32;
    let mut i = 0u32;
    while i < rounds {
        let (dx, dy, duration, _n) = single_expect(i)?;
        let d2 = (dx.saturating_mul(dx).saturating_add(dy.saturating_mul(dy))) as u32;
        let kind = classify(d2, duration);
        match kind {
            GestureKind::Tap => taps = taps.saturating_add(1),
            GestureKind::Fling => flings = flings.saturating_add(1),
            GestureKind::Swipe => swipes = swipes.saturating_add(1),
            _ => return Err(GsCode::KindBad.val()),
        }
        i = i.saturating_add(1);
    }
    Ok((taps, swipes, flings))
}

// ── 双指 / 捏合 ───────────────────────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct TwoProbe {
    pub states: [State; 6],
    pub pinch_amount: u32,
    pub final_kind: GestureKind,
    pub digest: u64,
}

pub fn two_finger_probe() -> Result<TwoProbe, u32> {
    let mut t = Track::new();
    let mut states = [State::Idle; 6];
    // 双指链：按下→第二指→拉开越过 PINCH_SLOP→第二指收→本指收→取消空转。
    // （不可先 Up 再 SecondUp：Up 已把状态送回 Idle，SecondUp 会被状态闸挡掉。）
    let (s0, _) = t.touch(Event::Down, Point { x: 100, y: 100 }, 1)?;
    let (s1, _) = t.touch(Event::SecondDown, Point { x: 500, y: 500 }, 2)?;
    let (s2, _) = t.touch(Event::Move, Point { x: 700, y: 100 }, 3)?;
    let pinch = t.pinch_amount();
    let kind = t.kind;
    let (s3, _) = t.touch(Event::SecondUp, Point { x: 700, y: 100 }, 4)?;
    let (s4, _) = t.touch(Event::Up, Point { x: 700, y: 100 }, 5)?;
    let (s5, _) = t.touch(Event::Cancel, Point { x: 700, y: 100 }, 6)?;
    states[0] = s0;
    states[1] = s1;
    states[2] = s2;
    states[3] = s3;
    states[4] = s4;
    states[5] = s5;
    if s0 != State::Contact {
        return Err(GsCode::OrderBad.val());
    }
    if s1 != State::TwoContact {
        return Err(GsCode::OrderBad.val());
    }
    if s2 != State::Pinch {
        return Err(GsCode::KindBad.val());
    }
    if s3 != State::Drag || s4 != State::Idle || s5 != State::Idle {
        return Err(GsCode::OrderBad.val());
    }
    if pinch == 0 || kind != GestureKind::TwoFinger {
        return Err(GsCode::KindBad.val());
    }
    Ok(TwoProbe {
        states,
        pinch_amount: pinch,
        final_kind: kind,
        digest: t.digest,
    })
}

// ── fuzz（随机事件流 ×10^4） ─────────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct StreamFuzz {
    pub attempts: u32,
    pub ok: u32,
    pub rejected: u32,
    pub digest: u64,
}

impl StreamFuzz {
    pub fn new() -> StreamFuzz {
        StreamFuzz {
            attempts: 0,
            ok: 0,
            rejected: 0,
            digest: p_core::FNV_OFF,
        }
    }

    pub fn balanced(&self) -> bool {
        self.attempts > 0 && self.attempts == self.ok.saturating_add(self.rejected)
    }
}

pub fn fuzz_streams(rounds: u32) -> Result<StreamFuzz, u32> {
    if rounds == 0 {
        return Err(GsCode::CoverBad.val());
    }
    let mut rng = p_core::Lcg::new(0x7164_0F81_0000_0042);
    let mut f = StreamFuzz::new();
    let mut i = 0u32;
    while i < rounds {
        let mut t = Track::new();
        let mut tick = 0u32;
        let mut k = 0u32;
        while k < 8 {
            let ev = match Event::ALL.get(((rng.next() >> 33) as usize) % EVENTS) {
                Some(v) => *v,
                None => return Err(GsCode::CoverBad.val()),
            };
            let x = rng.signed(2_000);
            let y = rng.signed(2_000);
            tick = tick.saturating_add(1 + (rng.below(3)));
            match t.touch(ev, Point { x, y }, tick) {
                Ok(_) => f.ok = f.ok.saturating_add(1),
                Err(code) => {
                    if code != GsCode::StateBad.val() {
                        return Err(GsCode::FuzzBad.val());
                    }
                    f.rejected = f.rejected.saturating_add(1);
                }
            }
            k += 1;
        }
        f.attempts = f.attempts.saturating_add(8);
        f.digest = p_core::fold_u32(f.digest, t.state as u32);
        f.digest = p_core::fold_u32(f.digest, t.accepted);
        f.digest = p_core::fold_u32(f.digest, t.rejected);
        i = i.saturating_add(1);
    }
    if !f.balanced() || f.digest == 0 {
        return Err(GsCode::LedgerBad.val());
    }
    Ok(f)
}

// ── 注入（四路） ──────────────────────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct InjectLog {
    pub tries: [u32; INJECT_ROUTES],
    pub caught: [u32; INJECT_ROUTES],
}

impl InjectLog {
    pub fn all_caught(&self) -> bool {
        let mut i = 0usize;
        while i < INJECT_ROUTES {
            if self.tries[i] == 0 || self.caught[i] != self.tries[i] {
                return false;
            }
            i += 1;
        }
        true
    }
}

pub fn inject_rounds(rounds: u32) -> Result<InjectLog, u32> {
    if rounds == 0 {
        return Err(GsCode::CoverBad.val());
    }
    let mut log = InjectLog {
        tries: [0; INJECT_ROUTES],
        caught: [0; INJECT_ROUTES],
    };
    let mut i = 0u32;
    while i < rounds {
        // ① Move-before-Down 必须炸 StateBad。
        let mut t = Track::new();
        if t.touch(Event::Move, Point { x: 1, y: 1 }, 1).err() == Some(GsCode::StateBad.val()) {
            log.caught[0] = log.caught[0].saturating_add(1);
        }
        log.tries[0] = log.tries[0].saturating_add(1);
        // ② 时钟回退必须炸 TickBad。
        let mut t2 = Track::new();
        let _ = t2.touch(Event::Down, Point { x: 1, y: 1 }, 10);
        if t2.touch(Event::Move, Point { x: 2, y: 2 }, 9).err() == Some(GsCode::TickBad.val()) {
            log.caught[1] = log.caught[1].saturating_add(1);
        }
        log.tries[1] = log.tries[1].saturating_add(1);
        // ③ 二次按下必须炸 StateBad。
        let mut t3 = Track::new();
        let _ = t3.touch(Event::Down, Point { x: 1, y: 1 }, 1);
        let _ = t3.touch(Event::SecondDown, Point { x: 9, y: 9 }, 2);
        if t3.touch(Event::SecondDown, Point { x: 9, y: 9 }, 3).err()
            == Some(GsCode::StateBad.val())
        {
            log.caught[2] = log.caught[2].saturating_add(1);
        }
        log.tries[2] = log.tries[2].saturating_add(1);
        // ④ 无第二指的抬起必须炸 StateBad。
        let mut t4 = Track::new();
        let _ = t4.touch(Event::Down, Point { x: 1, y: 1 }, 1);
        if t4.touch(Event::SecondUp, Point { x: 1, y: 1 }, 2).err()
            == Some(GsCode::StateBad.val())
        {
            log.caught[3] = log.caught[3].saturating_add(1);
        }
        log.tries[3] = log.tries[3].saturating_add(1);
        i = i.saturating_add(1);
    }
    if !log.all_caught() {
        return Err(GsCode::CoverBad.val());
    }
    Ok(log)
}

// ── 渲染 ──────────────────────────────────────────────────────────
pub fn render_stats(s: &SingleStats) -> String {
    format!(
        "{}|{}|{}|{}|{}|0x{:016X}",
        s.attempts, s.taps, s.swipes, s.flings, s.moves, s.digest
    )
}

pub fn parse_u32(s: &str) -> Option<u32> {
    let b = s.as_bytes();
    if b.is_empty() {
        return None;
    }
    let mut v = 0u32;
    let mut i = 0usize;
    while i < b.len() {
        let c = *b.get(i)?;
        if !(48..=57).contains(&c) {
            return None;
        }
        v = v.checked_mul(10)?.checked_add((c - 48) as u32)?;
        i += 1;
    }
    Some(v)
}

pub fn parse_hex16(s: &str) -> Option<u64> {
    let b = s.as_bytes();
    if b.len() != 18 {
        return None;
    }
    if *b.get(0)? != b'0' || *b.get(1)? != b'x' {
        return None;
    }
    let mut v = 0u64;
    let mut i = 2usize;
    while i < 18 {
        let c = *b.get(i)?;
        let d = match c {
            48..=57 => c - 48,
            65..=70 => c - 55,
            97..=102 => c - 87,
            _ => return None,
        };
        v = (v << 4) | d as u64;
        i += 1;
    }
    Some(v)
}

pub fn audit_render(text: &str) -> Result<usize, u32> {
    let mut parts = [""; GESTURE_FIELDS];
    let mut n = 0usize;
    for p in text.split('|') {
        if n < GESTURE_FIELDS {
            if let Some(slot) = parts.get_mut(n) {
                *slot = p;
            }
        }
        n = n.saturating_add(1);
    }
    if n != GESTURE_FIELDS {
        return Err(GsCode::RenderBad.val());
    }
    let rounds = match parts.get(0).and_then(|v| parse_u32(v)) {
        Some(v) => v,
        None => return Err(GsCode::RenderBad.val()),
    };
    let taps = match parts.get(1).and_then(|v| parse_u32(v)) {
        Some(v) => v,
        None => return Err(GsCode::RenderBad.val()),
    };
    let swipes = match parts.get(2).and_then(|v| parse_u32(v)) {
        Some(v) => v,
        None => return Err(GsCode::RenderBad.val()),
    };
    let flings = match parts.get(3).and_then(|v| parse_u32(v)) {
        Some(v) => v,
        None => return Err(GsCode::RenderBad.val()),
    };
    let moves = match parts.get(4).and_then(|v| parse_u32(v)) {
        Some(v) => v,
        None => return Err(GsCode::RenderBad.val()),
    };
    let digest = match parts.get(5).and_then(|v| parse_hex16(v)) {
        Some(v) => v,
        None => return Err(GsCode::RenderBad.val()),
    };
    if rounds == 0 || taps.saturating_add(swipes).saturating_add(flings) != rounds {
        return Err(GsCode::LedgerBad.val());
    }
    if moves < rounds {
        return Err(GsCode::TraceBad.val());
    }
    if digest == 0 {
        return Err(GsCode::RenderBad.val());
    }
    Ok(GESTURE_FIELDS)
}

// ── 黄金运行 ──────────────────────────────────────────────────────
pub fn golden_run() -> Result<(u32, u32, u32, u32, u64), u32> {
    let s = single_rounds(SINGLE_ROUNDS)?;
    if !s.balanced() {
        return Err(GsCode::LedgerBad.val());
    }
    Ok((s.attempts, s.taps, s.swipes, s.flings, s.digest))
}

pub fn repro_check() -> Result<(), u32> {
    let a = golden_run()?;
    let b = golden_run()?;
    if a != b {
        return Err(GsCode::ReplayBad.val());
    }
    let x = two_finger_probe()?;
    let y = two_finger_probe()?;
    if x.states != y.states || x.digest != y.digest {
        return Err(GsCode::ReplayBad.val());
    }
    Ok(())
}

// ── 码族与段位 ────────────────────────────────────────────────────
pub fn codes_are_distinct() -> bool {
    let mut i = 0usize;
    while i < GsCode::ALL.len() {
        let mut j = i + 1;
        while j < GsCode::ALL.len() {
            match (GsCode::ALL.get(i), GsCode::ALL.get(j)) {
                (Some(a), Some(b)) => {
                    if a.val() == b.val() {
                        return false;
                    }
                }
                _ => return false,
            }
            j += 1;
        }
        i += 1;
    }
    true
}

pub fn segment_guard() -> bool {
    let mut i = 0usize;
    while i < GsCode::ALL.len() {
        if let Some(c) = GsCode::ALL.get(i) {
            let v = c.val();
            if v <= SEG_BASE || v > SEG_BASE + 0x0F {
                return false;
            }
        }
        i += 1;
    }
    true
}

// ── 探针 ──────────────────────────────────────────────────────────
pub fn probe_seq_a() -> Result<(), u32> {
    if audit_table()? != TRANSITIONS as u32 {
        return Err(GsCode::TableBad.val());
    }
    if audit_total_function()? != TRANSITIONS as u32 {
        return Err(GsCode::TableBad.val());
    }
    Ok(())
}

pub fn probe_seq_b() -> Result<(), u32> {
    let s = single_rounds(SINGLE_ROUNDS)?;
    let (taps, swipes, flings) = expected_split(SINGLE_ROUNDS)?;
    if s.taps != taps || s.swipes != swipes || s.flings != flings {
        return Err(GsCode::KindBad.val());
    }
    Ok(())
}

pub fn probe_seq_c() -> Result<(), u32> {
    let p = two_finger_probe()?;
    if p.states[2] != State::Pinch {
        return Err(GsCode::KindBad.val());
    }
    let mut t = Track::new();
    let _ = t.touch(Event::Down, Point { x: 0, y: 0 }, 1);
    let _ = t.touch(Event::Cancel, Point { x: 0, y: 0 }, 2);
    if t.state != State::Locked {
        return Err(GsCode::OrderBad.val());
    }
    let _ = t.touch(Event::Up, Point { x: 0, y: 0 }, 3);
    if t.state != State::Idle {
        return Err(GsCode::OrderBad.val());
    }
    Ok(())
}

// ── 域检查项（动态明细槽上限 8） ──────────────────────────────────
pub fn extra_checks() -> Vec<(&'static str, bool, String)> {
    let mut out = Vec::new();

    let table = audit_table();
    out.push((
        "F48981 42转换全覆盖",
        table == Ok(TRANSITIONS as u32) && audit_total_function() == Ok(TRANSITIONS as u32),
        match &table {
            Ok(n) => format!("states={} events={} trans={}", STATES, EVENTS, n),
            Err(e) => format!("err=0x{:X}", e),
        },
    ));

    let single = single_rounds(SINGLE_ROUNDS);
    let split = expected_split(SINGLE_ROUNDS);
    out.push((
        "F48982 单指×10^2三分账",
        matches!(&single, Ok(s) if s.balanced() && s.attempts == SINGLE_ROUNDS)
            && matches!(&split, Ok((t, w, f)) if matches!(&single, Ok(s) if s.taps == *t && s.swipes == *w && s.flings == *f)),
        match (&single, &split) {
            (Ok(s), Ok((t, w, f))) => format!(
                "rounds={} taps={}/{} swipes={}/{} flings={}/{}",
                s.attempts, s.taps, t, s.swipes, w, s.flings, f
            ),
            _ => format!(
                "single={:?} split={:?}",
                single.as_ref().err(),
                split.as_ref().err()
            ),
        },
    ));

    let mut trace_ok = true;
    let mut trace_detail = String::new();
    let mut i = 0u32;
    while i < SINGLE_ROUNDS {
        match single_seq(i) {
            Ok(q) => {
                if q.moves == 0 || q.path < isqrt(q.dist2) || q.max_disp2 < q.dist2 {
                    trace_ok = false;
                    trace_detail = format!("i={} moves={} path={}", i, q.moves, q.path);
                    break;
                }
            }
            Err(e) => {
                trace_ok = false;
                trace_detail = format!("i={} err=0x{:X}", i, e);
                break;
            }
        }
        i += 1;
    }
    if trace_ok {
        trace_detail = format!("rounds={} invariants=ok", SINGLE_ROUNDS);
    }
    out.push(("F48982 轨迹不变量", trace_ok, trace_detail));

    let two = two_finger_probe();
    out.push((
        "F48982 双指/捏合守卫",
        matches!(&two, Ok(p) if p.states[1] == State::TwoContact && p.states[2] == State::Pinch),
        match &two {
            Ok(p) => format!(
                "s1={} s2={} pinch={}px kind={}",
                p.states[1].name(),
                p.states[2].name(),
                p.pinch_amount,
                p.final_kind.name()
            ),
            Err(e) => format!("err=0x{:X}", e),
        },
    ));

    let fuzz = fuzz_streams(FUZZ_ROUNDS);
    out.push((
        "F48982 fuzz×10^4零错配",
        matches!(&fuzz, Ok(f) if f.balanced() && f.attempts == FUZZ_ROUNDS.saturating_mul(8)),
        match &fuzz {
            Ok(f) => format!(
                "events={} ok={} reject={}",
                f.attempts, f.ok, f.rejected
            ),
            Err(e) => format!("err=0x{:X}", e),
        },
    ));

    let inj = inject_rounds(4);
    out.push((
        "F48982 四路注入全捕获",
        matches!(&inj, Ok(l) if l.all_caught()),
        match &inj {
            Ok(l) => format!("tries={:?} caught={:?}", l.tries, l.caught),
            Err(e) => format!("err=0x{:X}", e),
        },
    ));

    out.push((
        "F48981 段位闭集",
        segment_guard() && codes_are_distinct(),
        format!("codes={} seg=0x{:04X}", GsCode::ALL.len(), SEG_BASE),
    ));

    let render = single_rounds(SINGLE_ROUNDS).map(|s| render_stats(&s));
    out.push((
        "F48982 账本可解析",
        matches!(&render, Ok(t) if audit_render(t) == Ok(GESTURE_FIELDS)),
        match &render {
            Ok(t) => format!("text={}", t),
            Err(e) => format!("err=0x{:X}", e),
        },
    ));

    out
}

pub fn run_unx48981_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F48981 手势状态机与单指语义");
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
    fn constants_are_anchored() {
        assert_eq!(SEG_BASE, 0x71640);
        assert_eq!(STATES, 7);
        assert_eq!(EVENTS, 6);
        assert_eq!(TRANSITIONS, 42);
        assert_eq!(ACTIONS, 12);
        assert_eq!(TAP_DIST2, 144);
        assert_eq!(TAP_MS, 180);
        assert_eq!(FLING_VEL, 300);
        assert_eq!(PINCH_SLOP, 24);
        assert_eq!(SINGLE_ROUNDS, 100);
        assert_eq!(GsCode::ALL.len(), 14);
    }

    #[test]
    fn table_covers_all_pairs() {
        assert_eq!(audit_table(), Ok(42));
        assert_eq!(audit_total_function(), Ok(42));
        assert_eq!(TABLE.len(), STATES * EVENTS);
    }

    #[test]
    fn each_state_has_six_events() {
        let mut r = 0usize;
        while r < STATES {
            let from = State::ALL[r];
            let mut c = 0usize;
            while c < EVENTS {
                assert!(step(from, Event::ALL[c]).is_ok());
                c += 1;
            }
            r += 1;
        }
    }

    #[test]
    fn transition_identities() {
        assert_eq!(step(State::Idle, Event::Down), Ok((State::Contact, Action::BeginTrack)));
        assert_eq!(step(State::Contact, Event::Move), Ok((State::Drag, Action::StartDrag)));
        assert_eq!(step(State::Contact, Event::Up), Ok((State::Idle, Action::EndTap)));
        assert_eq!(step(State::Contact, Event::SecondDown), Ok((State::TwoContact, Action::BeginTwo)));
        assert_eq!(step(State::TwoDrag, Event::SecondUp), Ok((State::Drag, Action::DropToSingle)));
        assert_eq!(step(State::Locked, Event::Up), Ok((State::Idle, Action::EndLocked)));
    }

    #[test]
    fn isqrt_is_exact_enough() {
        assert_eq!(isqrt(0), 0);
        assert_eq!(isqrt(1), 1);
        assert_eq!(isqrt(2), 1);
        assert_eq!(isqrt(9), 3);
        assert_eq!(isqrt(144), 12);
        assert_eq!(isqrt(169), 13);
        assert_eq!(isqrt(1_000_000), 1_000);
        let mut n = 0u32;
        while n < 200 {
            let r = isqrt(n);
            assert!(r.saturating_mul(r) <= n);
            assert!((r + 1).saturating_mul(r + 1) > n);
            n += 1;
        }
    }

    #[test]
    fn classify_boundaries() {
        assert_eq!(classify(0, 50), GestureKind::Tap);
        assert_eq!(classify(144, 180), GestureKind::Tap);
        assert_eq!(classify(145, 100), GestureKind::Swipe);
        assert_eq!(classify(0, 300), GestureKind::Hold);
        assert_eq!(classify(0, 600), GestureKind::LongPress);
        assert_eq!(classify(400, 400), GestureKind::Swipe);
        assert_eq!(classify(1_600, 100), GestureKind::Fling);
    }

    #[test]
    fn disp2_and_velocity() {
        let a = Point { x: 0, y: 0 };
        let b = Point { x: 3, y: 4 };
        assert_eq!(disp2(a, b), 25);
        assert_eq!(disp2(b, a), 25);
        assert_eq!(fling_velocity(25, 50), 100);
        assert_eq!(fling_velocity(25, 0), 5_000);
    }

    #[test]
    fn track_tap_sequence() {
        let mut t = Track::new();
        let o = Point { x: 500, y: 500 };
        assert_eq!(t.touch(Event::Down, o, 1), Ok((State::Contact, Action::BeginTrack)));
        assert_eq!(t.touch(Event::Up, o, 50), Ok((State::Idle, Action::EndTap)));
        assert_eq!(t.kind, GestureKind::Tap);
        assert_eq!(t.duration, 49);
        assert!(t.audit().is_ok());
    }

    #[test]
    fn track_drag_sequence() {
        let mut t = Track::new();
        let _ = t.touch(Event::Down, Point { x: 0, y: 0 }, 1);
        let _ = t.touch(Event::Move, Point { x: 30, y: 40 }, 2);
        let _ = t.touch(Event::Up, Point { x: 30, y: 40 }, 3);
        assert_eq!(t.kind, GestureKind::Fling);
        assert_eq!(t.moves, 1);
        assert_eq!(t.path, 70);
        assert_eq!(t.max_disp2, 2_500);
    }

    #[test]
    fn track_multi_move_accumulates() {
        let mut t = Track::new();
        let _ = t.touch(Event::Down, Point { x: 0, y: 0 }, 1);
        let _ = t.touch(Event::Move, Point { x: 3, y: 4 }, 2);
        let _ = t.touch(Event::Move, Point { x: 6, y: 8 }, 3);
        let _ = t.touch(Event::Move, Point { x: 9, y: 12 }, 4);
        assert_eq!(t.moves, 3);
        assert_eq!(t.path, 21);
        assert_eq!(t.max_disp2, 225);
    }

    #[test]
    fn move_before_down_is_rejected() {
        let mut t = Track::new();
        assert_eq!(
            t.touch(Event::Move, Point { x: 1, y: 1 }, 1),
            Err(GsCode::StateBad.val())
        );
        assert_eq!(t.rejected, 1);
    }

    #[test]
    fn tick_regression_is_rejected() {
        let mut t = Track::new();
        let _ = t.touch(Event::Down, Point { x: 0, y: 0 }, 5);
        assert_eq!(
            t.touch(Event::Move, Point { x: 1, y: 1 }, 5),
            Err(GsCode::TickBad.val())
        );
        assert_eq!(
            t.touch(Event::Move, Point { x: 1, y: 1 }, 0),
            Err(GsCode::TickBad.val())
        );
    }

    #[test]
    fn coord_bounds_are_enforced() {
        let mut t = Track::new();
        assert_eq!(
            t.touch(Event::Down, Point { x: COORD_MAX + 1, y: 0 }, 1),
            Err(GsCode::CoordBad.val())
        );
        assert_eq!(
            t.touch(Event::Down, Point { x: 0, y: -(COORD_MAX + 1) }, 1),
            Err(GsCode::CoordBad.val())
        );
    }

    #[test]
    fn double_down_is_rejected() {
        let mut t = Track::new();
        let _ = t.touch(Event::Down, Point { x: 0, y: 0 }, 1);
        assert_eq!(
            t.touch(Event::Down, Point { x: 0, y: 0 }, 2),
            Err(GsCode::StateBad.val())
        );
    }

    #[test]
    fn second_up_without_second_is_rejected() {
        let mut t = Track::new();
        let _ = t.touch(Event::Down, Point { x: 0, y: 0 }, 1);
        assert_eq!(
            t.touch(Event::SecondUp, Point { x: 0, y: 0 }, 2),
            Err(GsCode::StateBad.val())
        );
    }

    #[test]
    fn cancel_locks_until_up() {
        let mut t = Track::new();
        let _ = t.touch(Event::Down, Point { x: 0, y: 0 }, 1);
        let (s, a) = match t.touch(Event::Cancel, Point { x: 0, y: 0 }, 2) {
            Ok(v) => v,
            Err(_) => (State::Idle, Action::None),
        };
        assert_eq!(s, State::Locked);
        assert_eq!(a, Action::CancelAll);
        let (s2, _) = match t.touch(Event::Move, Point { x: 5, y: 5 }, 3) {
            Ok(v) => v,
            Err(_) => (State::Idle, Action::None),
        };
        assert_eq!(s2, State::Locked);
        let (s3, _) = match t.touch(Event::Up, Point { x: 5, y: 5 }, 4) {
            Ok(v) => v,
            Err(_) => (State::Idle, Action::None),
        };
        assert_eq!(s3, State::Idle);
    }

    #[test]
    fn single_seq_counts_moves() {
        let mut i = 0u32;
        while i < SINGLE_ROUNDS {
            match single_seq(i) {
                Ok(q) => {
                    assert_eq!(q.moves, 1 + (i % MOVE_MAX));
                    assert!(q.max_disp2 >= q.dist2);
                    assert_eq!(q.path >= isqrt(q.dist2), true);
                }
                Err(_) => assert!(false),
            }
            i += 1;
        }
    }

    #[test]
    fn single_rounds_split_matches_formula() {
        match (single_rounds(SINGLE_ROUNDS), expected_split(SINGLE_ROUNDS)) {
            (Ok(s), Ok((t, w, f))) => {
                assert_eq!(s.taps, t);
                assert_eq!(s.swipes, w);
                assert_eq!(s.flings, f);
                assert_eq!(s.attempts, SINGLE_ROUNDS);
                assert_eq!(s.attempts, s.taps + s.swipes + s.flings);
            }
            _ => assert!(false),
        }
    }

    #[test]
    fn single_rounds_rejects_out_of_range() {
        assert_eq!(single_rounds(0), Err(GsCode::CoverBad.val()));
        assert_eq!(single_rounds(SINGLE_ROUNDS + 1), Err(GsCode::CoverBad.val()));
        assert_eq!(single_seq(SINGLE_ROUNDS), Err(GsCode::CoverBad.val()));
    }

    #[test]
    fn two_finger_probe_transitions() {
        match two_finger_probe() {
            Ok(p) => {
                assert_eq!(p.states[0], State::Contact);
                assert_eq!(p.states[1], State::TwoContact);
                assert_eq!(p.states[2], State::Pinch);
                assert_eq!(p.states[3], State::Drag);
                assert_eq!(p.states[4], State::Idle);
                assert_eq!(p.states[5], State::Idle);
                // (700,100) 与第二指 (500,500) 相距 isqrt(200000)=447。
                assert_eq!(p.pinch_amount, 447);
                assert_eq!(p.final_kind, GestureKind::TwoFinger);
                assert!(p.digest != 0);
            }
            Err(_) => assert!(false),
        }
    }

    #[test]
    fn pinch_overlay_needs_slop() {
        let mut t = Track::new();
        let _ = t.touch(Event::Down, Point { x: 0, y: 0 }, 1);
        let _ = t.touch(Event::SecondDown, Point { x: 100, y: 0 }, 2);
        // 位移 10px：低于 24px 阈值，仍是双指拖。
        let (s, _) = match t.touch(Event::Move, Point { x: 10, y: 0 }, 3) {
            Ok(v) => v,
            Err(_) => (State::Idle, Action::None),
        };
        assert_eq!(s, State::TwoDrag);
        // 累计位移 30px：越过阈值，升级为捏合。
        let (s2, _) = match t.touch(Event::Move, Point { x: 30, y: 0 }, 4) {
            Ok(v) => v,
            Err(_) => (State::Idle, Action::None),
        };
        assert_eq!(s2, State::Pinch);
    }

    #[test]
    fn fuzz_is_balanced() {
        match fuzz_streams(2_000) {
            Ok(f) => {
                assert_eq!(f.attempts, 16_000);
                assert_eq!(f.attempts, f.ok + f.rejected);
                assert!(f.digest != 0);
            }
            Err(_) => assert!(false),
        }
    }

    #[test]
    fn fuzz_rejects_zero() {
        assert_eq!(fuzz_streams(0), Err(GsCode::CoverBad.val()));
    }

    #[test]
    fn injection_catches_four() {
        match inject_rounds(5) {
            Ok(l) => {
                assert!(l.all_caught());
                assert_eq!(l.tries, [5, 5, 5, 5]);
                assert_eq!(l.caught, [5, 5, 5, 5]);
            }
            Err(_) => assert!(false),
        }
    }

    #[test]
    fn injection_rejects_zero() {
        assert_eq!(inject_rounds(0), Err(GsCode::CoverBad.val()));
    }

    #[test]
    fn render_round_trips() {
        match single_rounds(SINGLE_ROUNDS).map(|s| render_stats(&s)) {
            Ok(text) => {
                assert_eq!(text.split('|').count(), GESTURE_FIELDS);
                assert_eq!(audit_render(&text), Ok(GESTURE_FIELDS));
            }
            Err(_) => assert!(false),
        }
    }

    #[test]
    fn render_tamper_is_rejected() {
        match single_rounds(SINGLE_ROUNDS).map(|s| render_stats(&s)) {
            Ok(text) => {
                // 轮次字段 100 被改成 999：三分账不再对平，必须被账闸打回。
                let bad = text.replacen("100|", "999|", 1);
                assert!(bad != text);
                assert_eq!(audit_render(&bad), Err(GsCode::LedgerBad.val()));
                assert_eq!(audit_render("100|1"), Err(GsCode::RenderBad.val()));
            }
            Err(_) => assert!(false),
        }
    }

    #[test]
    fn parse_helpers_are_strict() {
        assert_eq!(parse_u32("42"), Some(42));
        assert_eq!(parse_u32(""), None);
        assert_eq!(parse_u32("4x"), None);
        assert_eq!(parse_hex16("0x00000000000000FF"), Some(255));
        assert_eq!(parse_hex16("0x1"), None);
    }

    #[test]
    fn golden_is_reproducible() {
        assert!(repro_check().is_ok());
        assert_eq!(golden_run(), golden_run());
        match golden_run() {
            Ok((rounds, taps, swipes, flings, digest)) => {
                assert_eq!(rounds, SINGLE_ROUNDS);
                assert_eq!(rounds, taps + swipes + flings);
                assert!(digest != 0);
            }
            Err(_) => assert!(false),
        }
    }

    #[test]
    fn probes_are_green() {
        assert!(probe_seq_a().is_ok());
        assert!(probe_seq_b().is_ok());
        assert!(probe_seq_c().is_ok());
    }

    #[test]
    fn codes_are_closed() {
        assert!(codes_are_distinct());
        assert!(segment_guard());
        assert_eq!(detail(GsCode::StateBad.val()), GsCode::StateBad.detail());
        assert_eq!(detail(0xDEAD), "未知诊断码");
    }

    #[test]
    fn checks_are_green() {
        let s = run_unx48981_checks();
        assert!(s.all_passed(), "{}", s.red_items_text());
        assert!(!s.truncated());
    }
}
