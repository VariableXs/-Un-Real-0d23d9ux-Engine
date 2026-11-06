//! UNX-F48987 手势取消与打断路径（触点丢失中途恢复/取消语义）· 0x71700
//! ═══════════════════════════════════════════════════════════════════════
//! 判据原文（任务板逐字锚定，B10 族第四槽）：
//!   F48987 ① 纯功能代码 ≥320 行；② UNX-F48987-J1 打断 ×10^2
//!          恢复路径全过。
//!   F48988 ① 纯功能代码 ≥320 行；② UNX-F48988-J1 ×100 组识别 100%
//!          （样机集 T1 档随闸门补测登记）。
//! ─────────────────────────────────────────────────────────────────────
//! 闸门次序：时钟 → 手势码解码 → 映射 → 动作入账。
//! 解码先于映射：手势码是外部输入，解码失败是「来源坏了」；映射失败是
//! 「表坏了」。两者混报会把上游协议漂移误诊成本模块表缺项。
//! 映射先于入账：无动作的手势不得进入动作账。若先记 mapped 再查表，
//! 缺表项会被算成「已识别」，动作账虚增，回归时查不出缺项。
//! 仲裁次序（与映射分离）：先校验每条候选（序号非零、动作非空），再查
//! (来源,序号) 唯一性，最后做两段选择：最高优先级 → 同级最小序号。
//! 打断三态（F48987）：在途 → 恢复 / 取消。触点全丢后同数补齐才允许
//! 回到「恢复」；少一根就补「取消」——手势身份以起手时的指头集合为准，
//! 补不齐的手势继续执行等于把半截轨迹当完整手势用。
//! 五触点对账（F48988）：一次按下 5 点、记录原点，按质心位移与展开量
//! 判定捏合/滑动；100 组确定性序列必须 100/100，且两次复跑逐位一致。
//! 关键差异：本模块只回答「手势该变成哪个动作」与「冲突时谁赢」；
//! 不做手势识别本身（B10-CORE）、不做误触抑制（B10-META）、不做资源
//! 槽位（B10-GUARD）。仲裁只决定投放哪一个动作，不合并动作参数。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;
use super::unx48941_wheel_core as p_core;

/// 诊断段基址（0x71700，B10 族第四槽）。
pub const SEG_BASE: u32 = 0x71700;

/// 手势类数。
pub const GESTURES: usize = 8;
/// 动作码数。
pub const ACTIONS: usize = 11;
/// 来源类数。
pub const SOURCES: usize = 6;
/// 单次仲裁最大候选数。
pub const MAX_CLAIMS: usize = 8;
/// 仲裁注入轮数（卡面 ×10^3）。
pub const ARB_ROUNDS: u32 = 1_000;
/// 映射喂入轮数。
pub const FEED_ROUNDS: u32 = 1_000;
/// fuzz 轮数。
pub const FUZZ_ROUNDS: u32 = 1_000;
/// 注入路数。
pub const INJECT_ROUTES: usize = 4;
/// 账本字段数。
pub const STATS_FIELDS: usize = 6;
/// 黄金轮数。
pub const GOLDEN_ROUNDS: u32 = 8;
/// 时钟上界。
pub const TICK_CAP: u32 = 1_000_000;

// ── 诊断码闭集（窗口 +0x01..+0x0E） ───────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DpCode {
    /// 手势→动作表缺项/重项
    MapBad,
    /// 来源优先级表损坏
    PrioBad,
    /// 候选字段非法（序号为零/动作空）
    ClaimBad,
    /// 候选序号重复/歧义
    SeqBad,
    /// 时钟回退或越界
    TickBad,
    /// 未知手势码
    GestureBad,
    /// 动作账不守恒
    LedgerBad,
    /// 事件次序非法
    OrderBad,
    /// 渲染不可解析
    RenderBad,
    /// 复跑不一致
    ReplayBad,
    /// 覆盖不足
    CoverBad,
    /// 段位越窗
    SegmentBad,
    /// fuzz 与两段复算不一致
    FuzzBad,
    /// 常量表不满足守恒
    TableBad,
}

impl DpCode {
    pub fn val(self) -> u32 {
        SEG_BASE + 0x01 + self as u32
    }

    pub fn detail(self) -> &'static str {
        match self {
            DpCode::MapBad => "手势→动作表缺项、重项或空动作",
            DpCode::PrioBad => "来源优先级表为空、重值或零优先级",
            DpCode::ClaimBad => "候选序号为零或动作为空",
            DpCode::SeqBad => "候选序号重复，仲裁结果歧义",
            DpCode::TickBad => "时钟回退或越过上界",
            DpCode::GestureBad => "手势码不在闭集内",
            DpCode::LedgerBad => "识别/动作/拒账三账不守恒",
            DpCode::OrderBad => "候选次序非法",
            DpCode::RenderBad => "账本文本字段数或数值不可解析",
            DpCode::ReplayBad => "两次确定性复跑结果不一致",
            DpCode::CoverBad => "覆盖轮数为零或越过上限",
            DpCode::SegmentBad => "诊断码越出本模块段窗",
            DpCode::FuzzBad => "fuzz 与独立复算不一致",
            DpCode::TableBad => "模块常量表不满足守恒",
        }
    }

    pub const ALL: [DpCode; 14] = [
        DpCode::MapBad,
        DpCode::PrioBad,
        DpCode::ClaimBad,
        DpCode::SeqBad,
        DpCode::TickBad,
        DpCode::GestureBad,
        DpCode::LedgerBad,
        DpCode::OrderBad,
        DpCode::RenderBad,
        DpCode::ReplayBad,
        DpCode::CoverBad,
        DpCode::SegmentBad,
        DpCode::FuzzBad,
        DpCode::TableBad,
    ];
}

pub fn detail(code: u32) -> &'static str {
    let mut i = 0usize;
    while i < DpCode::ALL.len() {
        if let Some(c) = DpCode::ALL.get(i) {
            if c.val() == code {
                return c.detail();
            }
        }
        i += 1;
    }
    "未知诊断码"
}

// ── 手势 / 动作 / 来源 ────────────────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Gesture {
    Tap,
    DoubleTap,
    LongPress,
    Swipe,
    Fling,
    Pinch,
    Rotate,
    Hold,
}

impl Gesture {
    pub fn code(self) -> u32 {
        match self {
            Gesture::Tap => 0,
            Gesture::DoubleTap => 1,
            Gesture::LongPress => 2,
            Gesture::Swipe => 3,
            Gesture::Fling => 4,
            Gesture::Pinch => 5,
            Gesture::Rotate => 6,
            Gesture::Hold => 7,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Gesture::Tap => "tap",
            Gesture::DoubleTap => "double-tap",
            Gesture::LongPress => "long-press",
            Gesture::Swipe => "swipe",
            Gesture::Fling => "fling",
            Gesture::Pinch => "pinch",
            Gesture::Rotate => "rotate",
            Gesture::Hold => "hold",
        }
    }

    pub fn from_code(code: u32) -> Option<Gesture> {
        let mut i = 0usize;
        while i < Gesture::ALL.len() {
            if let Some(g) = Gesture::ALL.get(i) {
                if g.code() == code {
                    return Some(*g);
                }
            }
            i += 1;
        }
        None
    }

    pub const ALL: [Gesture; GESTURES] = [
        Gesture::Tap,
        Gesture::DoubleTap,
        Gesture::LongPress,
        Gesture::Swipe,
        Gesture::Fling,
        Gesture::Pinch,
        Gesture::Rotate,
        Gesture::Hold,
    ];
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Action {
    None,
    Click,
    DoubleClick,
    ContextMenu,
    Scroll,
    FlingScroll,
    Zoom,
    Rotate,
    Pan,
    PressHold,
    DragLock,
}

impl Action {
    pub fn code(self) -> u32 {
        match self {
            Action::None => 0,
            Action::Click => 1,
            Action::DoubleClick => 2,
            Action::ContextMenu => 3,
            Action::Scroll => 4,
            Action::FlingScroll => 5,
            Action::Zoom => 6,
            Action::Rotate => 7,
            Action::Pan => 8,
            Action::PressHold => 9,
            Action::DragLock => 10,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Action::None => "none",
            Action::Click => "click",
            Action::DoubleClick => "double-click",
            Action::ContextMenu => "context-menu",
            Action::Scroll => "scroll",
            Action::FlingScroll => "fling-scroll",
            Action::Zoom => "zoom",
            Action::Rotate => "rotate",
            Action::Pan => "pan",
            Action::PressHold => "press-hold",
            Action::DragLock => "drag-lock",
        }
    }

    pub const ALL: [Action; ACTIONS] = [
        Action::None,
        Action::Click,
        Action::DoubleClick,
        Action::ContextMenu,
        Action::Scroll,
        Action::FlingScroll,
        Action::Zoom,
        Action::Rotate,
        Action::Pan,
        Action::PressHold,
        Action::DragLock,
    ];
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Source {
    Touch,
    Mouse,
    Keyboard,
    Wheel,
    Gesture,
    System,
}

impl Source {
    pub fn name(self) -> &'static str {
        match self {
            Source::Touch => "touch",
            Source::Mouse => "mouse",
            Source::Keyboard => "keyboard",
            Source::Wheel => "wheel",
            Source::Gesture => "gesture",
            Source::System => "system",
        }
    }

    pub const ALL: [Source; SOURCES] = [
        Source::Touch,
        Source::Mouse,
        Source::Keyboard,
        Source::Wheel,
        Source::Gesture,
        Source::System,
    ];
}

/// 手势→动作映射表（每手势恰一行）。
pub const MAP: [(Gesture, Action); GESTURES] = [
    (Gesture::Tap, Action::Click),
    (Gesture::DoubleTap, Action::DoubleClick),
    (Gesture::LongPress, Action::ContextMenu),
    (Gesture::Swipe, Action::Scroll),
    (Gesture::Fling, Action::FlingScroll),
    (Gesture::Pinch, Action::Zoom),
    (Gesture::Rotate, Action::Rotate),
    (Gesture::Hold, Action::PressHold),
];

/// 来源优先级表（数值越大越优先，必须全序）。
pub const PRIO: [(Source, u32); SOURCES] = [
    (Source::Wheel, 60),
    (Source::Touch, 50),
    (Source::Gesture, 45),
    (Source::Mouse, 40),
    (Source::Keyboard, 30),
    (Source::System, 10),
];

pub fn map(g: Gesture) -> Option<Action> {
    let mut i = 0usize;
    while i < MAP.len() {
        match MAP.get(i) {
            Some((k, a)) => {
                if *k == g {
                    return Some(*a);
                }
            }
            None => return None,
        }
        i += 1;
    }
    None
}

pub fn priority(src: Source) -> Option<u32> {
    let mut i = 0usize;
    while i < PRIO.len() {
        match PRIO.get(i) {
            Some((k, v)) => {
                if *k == src {
                    return Some(*v);
                }
            }
            None => return None,
        }
        i += 1;
    }
    None
}

// ── 表审计 ────────────────────────────────────────────────────────
pub fn audit_map() -> Result<u32, u32> {
    let mut seen = [0u8; GESTURES];
    let mut i = 0usize;
    while i < MAP.len() {
        let (g, a) = match MAP.get(i) {
            Some(v) => *v,
            None => return Err(DpCode::MapBad.val()),
        };
        if a == Action::None {
            return Err(DpCode::MapBad.val());
        }
        if a.code() as usize >= ACTIONS {
            return Err(DpCode::MapBad.val());
        }
        let idx = g.code() as usize;
        if idx >= GESTURES {
            return Err(DpCode::MapBad.val());
        }
        match seen.get_mut(idx) {
            Some(slot) => {
                if *slot != 0 {
                    return Err(DpCode::MapBad.val());
                }
                *slot = 1;
            }
            None => return Err(DpCode::MapBad.val()),
        }
        i += 1;
    }
    let mut j = 0usize;
    while j < GESTURES {
        match seen.get(j) {
            Some(1) => {}
            _ => return Err(DpCode::MapBad.val()),
        }
        j += 1;
    }
    Ok(MAP.len() as u32)
}

pub fn audit_priority() -> Result<u32, u32> {
    let mut seen = [0u8; SOURCES];
    let mut i = 0usize;
    while i < PRIO.len() {
        let (src, p) = match PRIO.get(i) {
            Some(v) => *v,
            None => return Err(DpCode::PrioBad.val()),
        };
        if p == 0 {
            return Err(DpCode::PrioBad.val());
        }
        let idx = match Source::ALL.iter().position(|s| *s == src) {
            Some(v) => v,
            None => return Err(DpCode::PrioBad.val()),
        };
        if idx >= SOURCES {
            return Err(DpCode::PrioBad.val());
        }
        match seen.get_mut(idx) {
            Some(slot) => {
                if *slot != 0 {
                    return Err(DpCode::PrioBad.val());
                }
                *slot = 1;
            }
            None => return Err(DpCode::PrioBad.val()),
        }
        let mut j = i + 1;
        while j < PRIO.len() {
            if let Some((_, q)) = PRIO.get(j) {
                if *q == p {
                    return Err(DpCode::PrioBad.val());
                }
            }
            j += 1;
        }
        i += 1;
    }
    let mut k = 0usize;
    while k < SOURCES {
        match seen.get(k) {
            Some(1) => {}
            _ => return Err(DpCode::PrioBad.val()),
        }
        k += 1;
    }
    Ok(PRIO.len() as u32)
}

/// 全序审计：按优先级降序，相邻必须严格递减。
pub fn audit_order() -> Result<u32, u32> {
    let mut prev: Option<u32> = None;
    let mut i = 0usize;
    while i < PRIO.len() {
        let (_, p) = match PRIO.get(i) {
            Some(v) => *v,
            None => return Err(DpCode::PrioBad.val()),
        };
        if let Some(b) = prev {
            if p >= b {
                return Err(DpCode::PrioBad.val());
            }
        }
        prev = Some(p);
        i += 1;
    }
    Ok(PRIO.len() as u32)
}

// ── 候选与仲裁 ────────────────────────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Claim {
    pub source: Source,
    pub seq: u32,
    pub action: Action,
}

impl Claim {
    pub fn new(source: Source, seq: u32, action: Action) -> Claim {
        Claim {
            source,
            seq,
            action,
        }
    }
}

/// 两段式复算：先取最高优先级，再在同级中取最小序号；重复对报歧义。
pub fn recompute_key(
    claims: &[Option<Claim>; MAX_CLAIMS],
) -> Result<Option<(u32, u32)>, u32> {
    let mut i = 0usize;
    while i < MAX_CLAIMS {
        match claims.get(i) {
            Some(Some(c)) => {
                if c.seq == 0 || c.action == Action::None {
                    return Err(DpCode::ClaimBad.val());
                }
                let mut j = i + 1;
                while j < MAX_CLAIMS {
                    if let Some(Some(d)) = claims.get(j) {
                        if d.source == c.source && d.seq == c.seq {
                            return Err(DpCode::SeqBad.val());
                        }
                    }
                    j += 1;
                }
            }
            _ => {}
        }
        i += 1;
    }
    let mut best_prio = 0u32;
    let mut i2 = 0usize;
    while i2 < MAX_CLAIMS {
        if let Some(Some(c)) = claims.get(i2) {
            let p = match priority(c.source) {
                Some(v) => v,
                None => return Err(DpCode::PrioBad.val()),
            };
            if p > best_prio {
                best_prio = p;
            }
        }
        i2 += 1;
    }
    if best_prio == 0 {
        return Ok(None);
    }
    let mut best_seq = 0u32;
    let mut i3 = 0usize;
    while i3 < MAX_CLAIMS {
        if let Some(Some(c)) = claims.get(i3) {
            if let Some(p) = priority(c.source) {
                if p == best_prio && (best_seq == 0 || c.seq < best_seq) {
                    best_seq = c.seq;
                }
            }
        }
        i3 += 1;
    }
    if best_seq == 0 {
        return Err(DpCode::PrioBad.val());
    }
    Ok(Some((best_prio, best_seq)))
}

/// 仲裁：最高优先级优先；同级最小序号优先；重复 (来源,序号) 判歧义。
pub fn arbitrate(claims: &[Option<Claim>; MAX_CLAIMS]) -> Result<Option<Claim>, u32> {
    // 先用独立复算把坏输入挡在选优之前。
    let key = recompute_key(claims)?;
    let target = match key {
        Some(v) => v,
        None => return Ok(None),
    };
    let mut i = 0usize;
    while i < MAX_CLAIMS {
        if let Some(Some(c)) = claims.get(i) {
            let p = match priority(c.source) {
                Some(v) => v,
                None => return Err(DpCode::PrioBad.val()),
            };
            if p == target.0 && c.seq == target.1 {
                return Ok(Some(*c));
            }
        }
        i += 1;
    }
    Err(DpCode::SeqBad.val())
}

// ── 派发引擎 ──────────────────────────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct DispStats {
    pub ops: u32,
    pub mapped: u32,
    pub actions: u32,
    pub rejected: u32,
    pub arbitrated: u32,
    pub digest: u64,
}

impl DispStats {
    pub fn balanced(&self) -> bool {
        self.ops > 0
            && self.ops == self.mapped.saturating_add(self.rejected)
            && self.mapped == self.actions
    }

    pub fn audit(&self) -> Result<(), u32> {
        if !self.balanced() {
            return Err(DpCode::LedgerBad.val());
        }
        if self.digest == 0 {
            return Err(DpCode::ReplayBad.val());
        }
        Ok(())
    }
}

pub struct Engine {
    tick: u32,
    ops: u32,
    mapped: u32,
    actions: u32,
    rejected: u32,
    arbitrated: u32,
    digest: u64,
}

impl Engine {
    pub fn new() -> Engine {
        Engine {
            tick: 0,
            ops: 0,
            mapped: 0,
            actions: 0,
            rejected: 0,
            arbitrated: 0,
            digest: p_core::FNV_OFF,
        }
    }

    fn reject(&mut self, code: u32) -> Result<Action, u32> {
        self.rejected = self.rejected.saturating_add(1);
        self.digest = p_core::fold_u32(self.digest, code);
        Err(code)
    }

    /// 喂入手势码：时钟 → 解码 → 映射 → 动作入账。
    pub fn feed_code(&mut self, code: u32, tick: u32) -> Result<Action, u32> {
        self.ops = self.ops.saturating_add(1);
        if tick <= self.tick || tick > TICK_CAP {
            return self.reject(DpCode::TickBad.val());
        }
        self.tick = tick;
        let g = match Gesture::from_code(code) {
            Some(v) => v,
            None => return self.reject(DpCode::GestureBad.val()),
        };
        let a = match map(g) {
            Some(v) => v,
            None => return self.reject(DpCode::MapBad.val()),
        };
        self.mapped = self.mapped.saturating_add(1);
        self.actions = self.actions.saturating_add(1);
        self.digest = p_core::fold_u32(self.digest, g.code());
        self.digest = p_core::fold_u32(self.digest, a.code());
        Ok(a)
    }

    pub fn feed(&mut self, g: Gesture, tick: u32) -> Result<Action, u32> {
        self.feed_code(g.code(), tick)
    }

    /// 仲裁一拍：候选集合 → 裁决动作；空集合不计为仲裁。
    ///
    /// 仲裁与喂入是两条独立账：`arbitrated` 记仲裁拍数（含拒账拍），
    /// `ops/mapped/actions/rejected` 只记喂入账。两者混跑后
    /// `DispStats::balanced` 仍只核对喂入账，仲裁拒账只进摘要，
    /// 不污染手势→动作的守恒关系。
    pub fn arbitrate_round(&mut self, claims: &[Option<Claim>; MAX_CLAIMS]) -> Result<Option<Claim>, u32> {
        self.arbitrated = self.arbitrated.saturating_add(1);
        match arbitrate(claims) {
            Ok(v) => {
                if let Some(c) = v {
                    self.digest = p_core::fold_u32(self.digest, c.source_name_code());
                    self.digest = p_core::fold_u32(self.digest, c.seq);
                    self.digest = p_core::fold_u32(self.digest, c.action.code());
                }
                Ok(v)
            }
            Err(e) => {
                self.digest = p_core::fold_u32(self.digest, e);
                Err(e)
            }
        }
    }

    pub fn stats(&self) -> DispStats {
        DispStats {
            ops: self.ops,
            mapped: self.mapped,
            actions: self.actions,
            rejected: self.rejected,
            arbitrated: self.arbitrated,
            digest: self.digest,
        }
    }
}

impl Claim {
    pub fn source_name_code(&self) -> u32 {
        let mut i = 0usize;
        while i < Source::ALL.len() {
            if let Some(s) = Source::ALL.get(i) {
                if *s == self.source {
                    return i as u32;
                }
            }
            i += 1;
        }
        0
    }
}

// 仲裁拍与喂入拍是两条独立账：`arbitrated` 计仲裁拍数，喂入账只由
// `ops/mapped/actions/rejected` 承担；`DispStats::balanced` 因此仍按
// 喂入账守恒核对，仲裁拒账只折叠进摘要。

// ── 喂入探针（F48987） ────────────────────────────────────────────
pub fn feed_probe(rounds: u32) -> Result<DispStats, u32> {
    if rounds == 0 || rounds > FEED_ROUNDS {
        return Err(DpCode::CoverBad.val());
    }
    let mut e = Engine::new();
    let mut tick = 0u32;
    let mut i = 0u32;
    while i < rounds {
        let mut j = 0usize;
        while j < GESTURES {
            let g = match Gesture::ALL.get(j) {
                Some(v) => *v,
                None => return Err(DpCode::CoverBad.val()),
            };
            let expect = match map(g) {
                Some(v) => v,
                None => return Err(DpCode::MapBad.val()),
            };
            tick = tick.saturating_add(1);
            if e.feed(g, tick)? != expect {
                return Err(DpCode::MapBad.val());
            }
            j += 1;
        }
        i = i.saturating_add(1);
    }
    let s = e.stats();
    s.audit()?;
    if s.mapped != rounds.saturating_mul(GESTURES as u32) {
        return Err(DpCode::LedgerBad.val());
    }
    Ok(s)
}

// ── 仲裁探针（F48988） ────────────────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ArbStats {
    pub rounds: u32,
    pub picked: u32,
    pub conflicts: u32,
    pub digest: u64,
}

impl ArbStats {
    pub fn balanced(&self) -> bool {
        self.rounds > 0
            && self.rounds == self.picked.saturating_add(self.conflicts)
    }
}

fn clean_claims(round: u32) -> [Option<Claim>; MAX_CLAIMS] {
    let mut arr = [None; MAX_CLAIMS];
    let n = 1 + (round as usize % MAX_CLAIMS);
    let mut j = 0usize;
    while j < n {
        let src = match Source::ALL.get((round as usize + j * 2) % SOURCES) {
            Some(v) => *v,
            None => Source::System,
        };
        let seq = 1 + (round % 10) * 8 + j as u32;
        let act = match Action::ALL.get(1 + (round as usize + j) % (ACTIONS - 1)) {
            Some(v) => *v,
            None => Action::Click,
        };
        if let Some(slot) = arr.get_mut(j) {
            *slot = Some(Claim::new(src, seq, act));
        }
        j += 1;
    }
    arr
}

pub fn priority_probe(rounds: u32) -> Result<ArbStats, u32> {
    if rounds == 0 || rounds > ARB_ROUNDS {
        return Err(DpCode::CoverBad.val());
    }
    let mut digest = p_core::FNV_OFF;
    let mut i = 0u32;
    while i < rounds {
        let claims = clean_claims(i);
        let key = recompute_key(&claims)?;
        let chosen = arbitrate(&claims)?;
        let (best_prio, best_seq) = match key {
            Some(v) => v,
            None => return Err(DpCode::FuzzBad.val()),
        };
        let c = match chosen {
            Some(v) => v,
            None => return Err(DpCode::FuzzBad.val()),
        };
        let p = match priority(c.source) {
            Some(v) => v,
            None => return Err(DpCode::PrioBad.val()),
        };
        if p != best_prio || c.seq != best_seq {
            return Err(DpCode::FuzzBad.val());
        }
        // 独立校验：该候选必须是全集中唯一达到 (最高优先级, 最小序号) 的。
        let mut match_count = 0u32;
        let mut j = 0usize;
        while j < MAX_CLAIMS {
            if let Some(Some(d)) = claims.get(j) {
                if let Some(dp) = priority(d.source) {
                    if dp == best_prio && d.seq == best_seq {
                        match_count = match_count.saturating_add(1);
                    }
                }
            }
            j += 1;
        }
        if match_count != 1 {
            return Err(DpCode::SeqBad.val());
        }
        digest = p_core::fold_u32(digest, best_prio);
        digest = p_core::fold_u32(digest, best_seq);
        digest = p_core::fold_u32(digest, c.action.code());
        i = i.saturating_add(1);
    }
    Ok(ArbStats {
        rounds,
        picked: rounds,
        conflicts: 0,
        digest,
    })
}

/// 同级早序号优先：同一来源两手候选，序号小者胜。
pub fn tie_probe() -> Result<u32, u32> {
    let mut arr = [None; MAX_CLAIMS];
    arr[0] = Some(Claim::new(Source::Touch, 9, Action::Click));
    arr[1] = Some(Claim::new(Source::Touch, 3, Action::Scroll));
    let c = match arbitrate(&arr)? {
        Some(v) => v,
        None => return Err(DpCode::FuzzBad.val()),
    };
    if c.seq != 3 || c.action != Action::Scroll {
        return Err(DpCode::SeqBad.val());
    }
    // 高优先级压过早序号：Wheel(60,seq9) 胜 Touch(50,seq1)。
    arr[0] = Some(Claim::new(Source::Wheel, 9, Action::Scroll));
    arr[1] = Some(Claim::new(Source::Touch, 1, Action::Click));
    let c2 = match arbitrate(&arr)? {
        Some(v) => v,
        None => return Err(DpCode::FuzzBad.val()),
    };
    if c2.source != Source::Wheel || c2.seq != 9 {
        return Err(DpCode::PrioBad.val());
    }
    Ok(2)
}

/// 歧义与坏候选：四种输入必须分别炸对应码。
pub fn conflict_probe() -> Result<u32, u32> {
    let mut arr = [None; MAX_CLAIMS];
    // ① 同一 (来源,序号) 重复 → SeqBad。
    arr[0] = Some(Claim::new(Source::Touch, 4, Action::Click));
    arr[1] = Some(Claim::new(Source::Touch, 4, Action::Scroll));
    if arbitrate(&arr) != Err(DpCode::SeqBad.val()) {
        return Err(DpCode::SeqBad.val());
    }
    // ② 序号为零 → ClaimBad。
    arr[0] = Some(Claim::new(Source::Touch, 0, Action::Click));
    arr[1] = None;
    if arbitrate(&arr) != Err(DpCode::ClaimBad.val()) {
        return Err(DpCode::ClaimBad.val());
    }
    // ③ 动作为空 → ClaimBad。
    arr[0] = Some(Claim::new(Source::Touch, 2, Action::None));
    if arbitrate(&arr) != Err(DpCode::ClaimBad.val()) {
        return Err(DpCode::ClaimBad.val());
    }
    // ④ 空集合 → 无候选。
    arr[0] = None;
    if arbitrate(&arr)? != None {
        return Err(DpCode::OrderBad.val());
    }
    Ok(4)
}

// ── fuzz：随机候选集 ×10^3 ───────────────────────────────────────
pub fn fuzz_claims(rounds: u32) -> Result<ArbStats, u32> {
    if rounds == 0 || rounds > FUZZ_ROUNDS {
        return Err(DpCode::CoverBad.val());
    }
    let mut rng = p_core::Lcg::new(0x7170_0F88_0F87_0117);
    let mut picked = 0u32;
    let mut conflicts = 0u32;
    let mut digest = p_core::FNV_OFF;
    let mut i = 0u32;
    while i < rounds {
        let mut arr = [None; MAX_CLAIMS];
        let n = 1 + ((rng.next() >> 33) as usize % MAX_CLAIMS);
        let mut j = 0usize;
        while j < n {
            let src = match Source::ALL.get((rng.next() >> 33) as usize % SOURCES) {
                Some(v) => *v,
                None => Source::System,
            };
            let seq = 1 + rng.below(6);
            let act = match Action::ALL.get(1 + rng.below(ACTIONS as u32 - 1) as usize) {
                Some(v) => *v,
                None => Action::Click,
            };
            if let Some(slot) = arr.get_mut(j) {
                *slot = Some(Claim::new(src, seq, act));
            }
            j += 1;
        }
        // 独立判歧义：任一对同源同序号即为冲突。
        let mut dup = false;
        let mut a = 0usize;
        while a < MAX_CLAIMS {
            if let Some(Some(c)) = arr.get(a) {
                let mut b = a + 1;
                while b < MAX_CLAIMS {
                    if let Some(Some(d)) = arr.get(b) {
                        if c.source == d.source && c.seq == d.seq {
                            dup = true;
                        }
                    }
                    b += 1;
                }
            }
            a += 1;
        }
        if dup {
            if arbitrate(&arr) != Err(DpCode::SeqBad.val()) {
                return Err(DpCode::FuzzBad.val());
            }
            conflicts = conflicts.saturating_add(1);
        } else {
            let c = match arbitrate(&arr) {
                Ok(Some(v)) => v,
                _ => return Err(DpCode::FuzzBad.val()),
            };
            let key = recompute_key(&arr)?;
            let (bp, bs) = match key {
                Some(v) => v,
                None => return Err(DpCode::FuzzBad.val()),
            };
            let p = match priority(c.source) {
                Some(v) => v,
                None => return Err(DpCode::PrioBad.val()),
            };
            if p != bp || c.seq != bs {
                return Err(DpCode::FuzzBad.val());
            }
            picked = picked.saturating_add(1);
            digest = p_core::fold_u32(digest, bp);
            digest = p_core::fold_u32(digest, bs);
            digest = p_core::fold_u32(digest, c.action.code());
        }
        i = i.saturating_add(1);
    }
    let s = ArbStats {
        rounds,
        picked,
        conflicts,
        digest,
    };
    if !s.balanced() || s.digest == 0 {
        return Err(DpCode::LedgerBad.val());
    }
    Ok(s)
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
        return Err(DpCode::CoverBad.val());
    }
    let mut log = InjectLog {
        tries: [0; INJECT_ROUTES],
        caught: [0; INJECT_ROUTES],
    };
    let mut i = 0u32;
    while i < rounds {
        // ① 时钟回退必须 TickBad。
        let mut e = Engine::new();
        let _ = e.feed(Gesture::Tap, 5);
        if e.feed(Gesture::Tap, 5).err() == Some(DpCode::TickBad.val()) {
            log.caught[0] = log.caught[0].saturating_add(1);
        }
        log.tries[0] = log.tries[0].saturating_add(1);

        // ② 未知手势码必须 GestureBad。
        let mut e2 = Engine::new();
        if e2.feed_code(99, 1).err() == Some(DpCode::GestureBad.val()) {
            log.caught[1] = log.caught[1].saturating_add(1);
        }
        log.tries[1] = log.tries[1].saturating_add(1);

        // ③ 重复 (来源,序号) 必须 SeqBad。
        let mut arr = [None; MAX_CLAIMS];
        arr[0] = Some(Claim::new(Source::Mouse, 2, Action::Click));
        arr[1] = Some(Claim::new(Source::Mouse, 2, Action::Scroll));
        if arbitrate(&arr).err() == Some(DpCode::SeqBad.val()) {
            log.caught[2] = log.caught[2].saturating_add(1);
        }
        log.tries[2] = log.tries[2].saturating_add(1);

        // ④ 空动作候选必须 ClaimBad。
        let mut arr2 = [None; MAX_CLAIMS];
        arr2[0] = Some(Claim::new(Source::Keyboard, 1, Action::None));
        if arbitrate(&arr2).err() == Some(DpCode::ClaimBad.val()) {
            log.caught[3] = log.caught[3].saturating_add(1);
        }
        log.tries[3] = log.tries[3].saturating_add(1);
        i = i.saturating_add(1);
    }
    if !log.all_caught() {
        return Err(DpCode::CoverBad.val());
    }
    Ok(log)
}

// ── 渲染 ──────────────────────────────────────────────────────────
pub fn render_stats(s: &DispStats) -> String {
    format!(
        "{}|{}|{}|{}|{}|0x{:016X}",
        s.ops, s.mapped, s.actions, s.rejected, s.arbitrated, s.digest
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
    let mut parts = [""; STATS_FIELDS];
    let mut n = 0usize;
    for p in text.split('|') {
        if n < STATS_FIELDS {
            if let Some(slot) = parts.get_mut(n) {
                *slot = p;
            }
        }
        n = n.saturating_add(1);
    }
    if n != STATS_FIELDS {
        return Err(DpCode::RenderBad.val());
    }
    let ops = match parts.get(0).and_then(|v| parse_u32(v)) {
        Some(v) => v,
        None => return Err(DpCode::RenderBad.val()),
    };
    let mapped = match parts.get(1).and_then(|v| parse_u32(v)) {
        Some(v) => v,
        None => return Err(DpCode::RenderBad.val()),
    };
    let actions = match parts.get(2).and_then(|v| parse_u32(v)) {
        Some(v) => v,
        None => return Err(DpCode::RenderBad.val()),
    };
    let rejected = match parts.get(3).and_then(|v| parse_u32(v)) {
        Some(v) => v,
        None => return Err(DpCode::RenderBad.val()),
    };
    let arbitrated = match parts.get(4).and_then(|v| parse_u32(v)) {
        Some(v) => v,
        None => return Err(DpCode::RenderBad.val()),
    };
    let digest = match parts.get(5).and_then(|v| parse_hex16(v)) {
        Some(v) => v,
        None => return Err(DpCode::RenderBad.val()),
    };
    if ops == 0 || mapped != actions {
        return Err(DpCode::LedgerBad.val());
    }
    if ops < mapped.saturating_add(rejected) {
        return Err(DpCode::LedgerBad.val());
    }
    if arbitrated > ops {
        return Err(DpCode::LedgerBad.val());
    }
    if digest == 0 {
        return Err(DpCode::RenderBad.val());
    }
    Ok(STATS_FIELDS)
}

// ── 黄金运行 ──────────────────────────────────────────────────────
pub fn golden_run() -> Result<DispStats, u32> {
    let mut e = Engine::new();
    let mut tick = 0u32;
    let mut i = 0u32;
    while i < GOLDEN_ROUNDS {
        let mut j = 0usize;
        while j < GESTURES {
            let g = match Gesture::ALL.get(j) {
                Some(v) => *v,
                None => return Err(DpCode::CoverBad.val()),
            };
            tick = tick.saturating_add(1);
            let a = e.feed(g, tick)?;
            let expect = match map(g) {
                Some(v) => v,
                None => return Err(DpCode::MapBad.val()),
            };
            if a != expect {
                return Err(DpCode::MapBad.val());
            }
            j += 1;
        }
        i = i.saturating_add(1);
    }
    let s = e.stats();
    s.audit()?;
    if s.mapped != GOLDEN_ROUNDS.saturating_mul(GESTURES as u32) {
        return Err(DpCode::LedgerBad.val());
    }
    Ok(s)
}

pub fn repro_check() -> Result<(), u32> {
    let a = golden_run()?;
    let b = golden_run()?;
    if a != b {
        return Err(DpCode::ReplayBad.val());
    }
    let c = fuzz_claims(256)?;
    let d = fuzz_claims(256)?;
    if c != d {
        return Err(DpCode::ReplayBad.val());
    }
    Ok(())
}

// ── 码族与段位 ────────────────────────────────────────────────────
pub fn codes_are_distinct() -> bool {
    let mut i = 0usize;
    while i < DpCode::ALL.len() {
        let mut j = i + 1;
        while j < DpCode::ALL.len() {
            match (DpCode::ALL.get(i), DpCode::ALL.get(j)) {
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
    while i < DpCode::ALL.len() {
        if let Some(c) = DpCode::ALL.get(i) {
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
    if audit_map()? != GESTURES as u32 {
        return Err(DpCode::MapBad.val());
    }
    if audit_priority()? != SOURCES as u32 {
        return Err(DpCode::PrioBad.val());
    }
    if audit_order()? != SOURCES as u32 {
        return Err(DpCode::PrioBad.val());
    }
    Ok(())
}

pub fn probe_seq_b() -> Result<(), u32> {
    let s = feed_probe(FEED_ROUNDS)?;
    if !s.balanced() {
        return Err(DpCode::LedgerBad.val());
    }
    Ok(())
}

pub fn probe_seq_c() -> Result<(), u32> {
    let s = priority_probe(ARB_ROUNDS)?;
    if !s.balanced() {
        return Err(DpCode::LedgerBad.val());
    }
    if tie_probe()? != 2 || conflict_probe()? != 4 {
        return Err(DpCode::FuzzBad.val());
    }
    Ok(())
}

// ── F48987 手势取消与打断（触点丢失中途恢复/取消语义） ────────────
/// 打断场景轮数（卡面 ×10^2）。
pub const RECOVER_ROUNDS: u32 = 100;
/// 单手势触点上限（五指）。
pub const RECOVER_MAX: u8 = 5;
/// 五触点组数（卡面 ×100 组）。
pub const CONC_ROUNDS: u32 = 100;
/// 每组触点数。
pub const CONC_FINGERS: usize = 5;
/// 对账屏宽（像素）。
pub const SEQ_W: i32 = 1_920;
/// 对账屏高（像素）。
pub const SEQ_H: i32 = 1_080;
/// 滑动判定最小质心位移（像素）。
pub const SWIPE_MIN: i64 = 120;
/// 捏合判定最小展开量（像素，五指曼哈顿和）。
pub const SPREAD_MIN: i64 = 90;

/// 手势生命周期：在途 / 已恢复 / 已取消。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CancelState {
    Idle,
    Tracking,
    Recovered,
    Cancelled,
}

impl CancelState {
    pub fn code(self) -> u32 {
        match self {
            CancelState::Idle => 0,
            CancelState::Tracking => 1,
            CancelState::Recovered => 2,
            CancelState::Cancelled => 3,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            CancelState::Idle => "idle",
            CancelState::Tracking => "tracking",
            CancelState::Recovered => "recovered",
            CancelState::Cancelled => "cancelled",
        }
    }

    pub const ALL: [CancelState; 4] = [
        CancelState::Idle,
        CancelState::Tracking,
        CancelState::Recovered,
        CancelState::Cancelled,
    ];
}

/// 打断跟踪器：以起手指头集合为身份，丢点/补点/显式取消三路推进。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct RecoveryTrack {
    pub initial: u8,
    pub live: u8,
    pub state: CancelState,
    pub lost: u32,
    pub resumed: u32,
    pub cancelled: u32,
    digest: u64,
}

impl RecoveryTrack {
    pub fn new(initial: u8) -> Result<RecoveryTrack, u32> {
        if initial == 0 || initial > RECOVER_MAX {
            return Err(DpCode::ClaimBad.val());
        }
        Ok(RecoveryTrack {
            initial,
            live: initial,
            state: CancelState::Tracking,
            lost: 0,
            resumed: 0,
            cancelled: 0,
            digest: p_core::FNV_OFF,
        })
    }

    fn fold(&mut self, a: u32, b: u32) {
        self.digest = p_core::fold_u32(self.digest, a);
        self.digest = p_core::fold_u32(self.digest, b);
    }

    /// 触点丢失：只能丢在途且不超过在途数。
    pub fn lose(&mut self, n: u8) -> Result<u8, u32> {
        if self.state != CancelState::Tracking {
            return Err(DpCode::OrderBad.val());
        }
        if n == 0 || n > self.live {
            return Err(DpCode::ClaimBad.val());
        }
        self.live -= n;
        self.lost = self.lost.saturating_add(n as u32);
        let live = self.live;
        self.fold(1, live as u32);
        Ok(live)
    }

    /// 触点补回：补到起手数才恢复；超补一律 ClaimBad。
    pub fn resume(&mut self, n: u8) -> Result<u8, u32> {
        if self.state != CancelState::Tracking {
            return Err(DpCode::OrderBad.val());
        }
        if n == 0 || self.live.saturating_add(n) > self.initial {
            return Err(DpCode::ClaimBad.val());
        }
        self.live += n;
        self.resumed = self.resumed.saturating_add(n as u32);
        if self.live == self.initial && self.lost > 0 {
            self.state = CancelState::Recovered;
        }
        let live = self.live;
        self.fold(2, live as u32);
        Ok(live)
    }

    /// 部分抬指：手势身份已破，按取消语义收尾。
    pub fn partial_release(&mut self, n: u8) -> Result<u8, u32> {
        if self.state != CancelState::Tracking {
            return Err(DpCode::OrderBad.val());
        }
        if n == 0 || n > self.live {
            return Err(DpCode::ClaimBad.val());
        }
        self.live -= n;
        self.state = CancelState::Cancelled;
        self.cancelled = self.cancelled.saturating_add(1);
        let live = self.live;
        self.fold(3, live as u32);
        Ok(live)
    }

    /// 显式取消：在途 → 已取消。
    pub fn cancel(&mut self) -> Result<(), u32> {
        if self.state != CancelState::Tracking {
            return Err(DpCode::OrderBad.val());
        }
        self.state = CancelState::Cancelled;
        self.cancelled = self.cancelled.saturating_add(1);
        self.fold(4, self.live as u32);
        Ok(())
    }

    /// 正常完成：只有在途数仍等于起手数才允许「恢复」收尾。
    pub fn complete(&mut self) -> Result<(), u32> {
        if self.state != CancelState::Tracking {
            return Err(DpCode::OrderBad.val());
        }
        if self.live != self.initial {
            return Err(DpCode::OrderBad.val());
        }
        self.state = CancelState::Recovered;
        self.fold(5, self.live as u32);
        Ok(())
    }

    /// 审计：状态必须已决、恢复者手指数必须补齐、取消者只记一次、
    /// 丢失不超起手数。
    pub fn audit(&self) -> Result<(), u32> {
        if self.initial == 0 || self.initial > RECOVER_MAX {
            return Err(DpCode::ClaimBad.val());
        }
        if self.state == CancelState::Tracking || self.state == CancelState::Idle {
            return Err(DpCode::OrderBad.val());
        }
        if self.live > self.initial {
            return Err(DpCode::LedgerBad.val());
        }
        if self.lost > self.initial as u32 {
            return Err(DpCode::LedgerBad.val());
        }
        if self.state == CancelState::Recovered && self.live != self.initial {
            return Err(DpCode::LedgerBad.val());
        }
        if self.state == CancelState::Cancelled && self.cancelled != 1 {
            return Err(DpCode::LedgerBad.val());
        }
        if self.digest == 0 {
            return Err(DpCode::ReplayBad.val());
        }
        Ok(())
    }
}

/// 打断批量账（×10^2 场景汇总）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct RecoveryStats {
    pub rounds: u32,
    pub recovered: u32,
    pub cancelled: u32,
    pub lost: u32,
    pub resumed: u32,
    pub digest: u64,
}

impl RecoveryStats {
    pub fn new() -> RecoveryStats {
        RecoveryStats {
            rounds: 0,
            recovered: 0,
            cancelled: 0,
            lost: 0,
            resumed: 0,
            digest: p_core::FNV_OFF,
        }
    }

    /// 每场景恰落在恢复/取消之一，且两路都出现过。
    pub fn clean(&self) -> bool {
        self.rounds > 0
            && self.recovered.saturating_add(self.cancelled) == self.rounds
            && self.recovered > 0
            && self.cancelled > 0
            && self.lost > 0
            && self.resumed > 0
            && self.digest != 0
    }

    pub fn audit(&self) -> Result<(), u32> {
        if !self.clean() {
            return Err(DpCode::LedgerBad.val());
        }
        Ok(())
    }
}

/// 打断 ×10^2：四类路径轮转（全丢全补 → 恢复；全丢半补 → 取消；
/// 部分抬指 → 取消；显式取消 → 取消），逐场景核对终态与账目。
pub fn recovery_rounds(rounds: u32) -> Result<RecoveryStats, u32> {
    if rounds == 0 || rounds > RECOVER_ROUNDS {
        return Err(DpCode::CoverBad.val());
    }
    let mut s = RecoveryStats::new();
    let mut i = 0u32;
    while i < rounds {
        let initial = 2 + (i % 3) as u8;
        let mut r = RecoveryTrack::new(initial)?;
        match i % 4 {
            0 => {
                // 全丢 → 全补：手势身份复原。
                r.lose(initial)?;
                if r.state != CancelState::Tracking {
                    return Err(DpCode::OrderBad.val());
                }
                r.resume(initial)?;
                if r.state != CancelState::Recovered || r.live != initial {
                    return Err(DpCode::FuzzBad.val());
                }
                s.recovered = s.recovered.saturating_add(1);
            }
            1 => {
                // 全丢 → 只补一根：补不齐，取消语义收尾。
                r.lose(initial)?;
                r.resume(1)?;
                if r.live != 1 || r.state != CancelState::Tracking {
                    return Err(DpCode::FuzzBad.val());
                }
                r.cancel()?;
                if r.state != CancelState::Cancelled || r.resumed != 1 {
                    return Err(DpCode::FuzzBad.val());
                }
                s.cancelled = s.cancelled.saturating_add(1);
            }
            2 => {
                // 中途部分抬指：立即取消，不再接受补点。
                r.partial_release(1)?;
                if r.state != CancelState::Cancelled || r.live + 1 != initial {
                    return Err(DpCode::FuzzBad.val());
                }
                if r.resume(1) != Err(DpCode::OrderBad.val()) {
                    return Err(DpCode::FuzzBad.val());
                }
                s.cancelled = s.cancelled.saturating_add(1);
            }
            _ => {
                // 显式取消：在途数不变，只改状态。
                r.cancel()?;
                if r.state != CancelState::Cancelled || r.live != initial {
                    return Err(DpCode::FuzzBad.val());
                }
                s.cancelled = s.cancelled.saturating_add(1);
            }
        }
        r.audit()?;
        s.rounds = s.rounds.saturating_add(1);
        s.lost = s.lost.saturating_add(r.lost);
        s.resumed = s.resumed.saturating_add(r.resumed);
        s.digest = p_core::fold_u32(s.digest, r.state.code());
        s.digest = p_core::fold_u32(s.digest, r.live as u32);
        s.digest = p_core::fold_u32(s.digest, r.digest as u32);
        i = i.saturating_add(1);
    }
    s.audit()?;
    Ok(s)
}

/// 四语义探针：恢复 / 半补取消 / 部分抬指取消 / 显式取消，外加越界负判据。
pub fn recovery_semantics_probe() -> Result<u32, u32> {
    // ① 全丢全补：恢复，且丢/补数守恒。
    let mut a = RecoveryTrack::new(3)?;
    a.lose(3)?;
    a.resume(3)?;
    if a.state != CancelState::Recovered || a.lost != 3 || a.resumed != 3 {
        return Err(DpCode::FuzzBad.val());
    }
    a.audit()?;
    // ② 全丢半补后取消：恢复路径被拒，取消路径成立。
    let mut b = RecoveryTrack::new(4)?;
    b.lose(4)?;
    b.resume(2)?;
    if b.state == CancelState::Recovered {
        return Err(DpCode::FuzzBad.val());
    }
    b.cancel()?;
    b.audit()?;
    // ③ 部分抬指：直接判取消，后续补点必须 OrderBad。
    let mut c = RecoveryTrack::new(2)?;
    c.partial_release(1)?;
    if c.state != CancelState::Cancelled || c.live != 1 {
        return Err(DpCode::FuzzBad.val());
    }
    if c.lose(1) != Err(DpCode::OrderBad.val()) {
        return Err(DpCode::FuzzBad.val());
    }
    c.audit()?;
    // ④ 显式取消：在途数不变。
    let mut d = RecoveryTrack::new(5)?;
    d.cancel()?;
    if d.state != CancelState::Cancelled || d.live != 5 {
        return Err(DpCode::FuzzBad.val());
    }
    d.audit()?;
    // 负判据：起手数越界、超补、超丢、未决审计。
    if RecoveryTrack::new(0).is_ok() || RecoveryTrack::new(6).is_ok() {
        return Err(DpCode::ClaimBad.val());
    }
    let mut e = RecoveryTrack::new(2)?;
    if e.resume(1) != Err(DpCode::ClaimBad.val()) {
        return Err(DpCode::FuzzBad.val());
    }
    if e.lose(3) != Err(DpCode::ClaimBad.val()) {
        return Err(DpCode::FuzzBad.val());
    }
    if e.audit() != Err(DpCode::OrderBad.val()) {
        return Err(DpCode::FuzzBad.val());
    }
    if e.complete() != Ok(()) || e.state != CancelState::Recovered {
        return Err(DpCode::FuzzBad.val());
    }
    Ok(4)
}

// ── F48988 五触点并发对账（×100 组识别 100%） ────────────────────
/// 五触点复合手势。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum MultiGesture {
    None,
    PinchIn,
    PinchOut,
    SwipeLeft,
    SwipeRight,
    SwipeUp,
    SwipeDown,
}

impl MultiGesture {
    pub fn code(self) -> u32 {
        match self {
            MultiGesture::None => 0,
            MultiGesture::PinchIn => 1,
            MultiGesture::PinchOut => 2,
            MultiGesture::SwipeLeft => 3,
            MultiGesture::SwipeRight => 4,
            MultiGesture::SwipeUp => 5,
            MultiGesture::SwipeDown => 6,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            MultiGesture::None => "none",
            MultiGesture::PinchIn => "pinch-in",
            MultiGesture::PinchOut => "pinch-out",
            MultiGesture::SwipeLeft => "swipe-left",
            MultiGesture::SwipeRight => "swipe-right",
            MultiGesture::SwipeUp => "swipe-up",
            MultiGesture::SwipeDown => "swipe-down",
        }
    }

    pub const ALL: [MultiGesture; 7] = [
        MultiGesture::None,
        MultiGesture::PinchIn,
        MultiGesture::PinchOut,
        MultiGesture::SwipeLeft,
        MultiGesture::SwipeRight,
        MultiGesture::SwipeUp,
        MultiGesture::SwipeDown,
    ];
}

/// 单触点（id 为组内序号，坐标逻辑像素）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Finger {
    pub id: u8,
    pub x: i32,
    pub y: i32,
}

/// 五点正五边形（相对屏心），供 100 组确定性序列使用。
pub const RING: [(i32, i32); CONC_FINGERS] = [
    (0, -260),
    (247, -80),
    (153, 212),
    (-153, 212),
    (-247, -80),
];
/// 屏心 x。
pub const SEQ_CX: i32 = SEQ_W / 2;
/// 屏心 y。
pub const SEQ_CY: i32 = SEQ_H / 2;

/// 五触点对账器：按下记原点，移动后按「展开量 → 质心位移」判手势。
#[derive(Clone)]
pub struct FiveTouchReconciler {
    pts: [Option<Finger>; CONC_FINGERS],
    origin: [Option<Finger>; CONC_FINGERS],
    count: u8,
    pub moves: u32,
    digest: u64,
}

fn centroid_of(pts: &[Option<Finger>; CONC_FINGERS]) -> (i64, i64) {
    let mut sx = 0i64;
    let mut sy = 0i64;
    let mut n = 0i64;
    let mut i = 0usize;
    while i < CONC_FINGERS {
        if let Some(p) = pts.get(i).and_then(|v| *v) {
            sx = sx.saturating_add(p.x as i64);
            sy = sy.saturating_add(p.y as i64);
            n = n.saturating_add(1);
        }
        i += 1;
    }
    if n == 0 {
        (0, 0)
    } else {
        (sx / n, sy / n)
    }
}

fn spread_of(pts: &[Option<Finger>; CONC_FINGERS]) -> i64 {
    let (cx, cy) = centroid_of(pts);
    let mut sum = 0i64;
    let mut i = 0usize;
    while i < CONC_FINGERS {
        if let Some(p) = pts.get(i).and_then(|v| *v) {
            sum = sum
                .saturating_add((p.x as i64 - cx).unsigned_abs() as i64)
                .saturating_add((p.y as i64 - cy).unsigned_abs() as i64);
        }
        i += 1;
    }
    sum
}

impl FiveTouchReconciler {
    pub fn new() -> FiveTouchReconciler {
        FiveTouchReconciler {
            pts: [None; CONC_FINGERS],
            origin: [None; CONC_FINGERS],
            count: 0,
            moves: 0,
            digest: p_core::FNV_OFF,
        }
    }

    pub fn count(&self) -> u8 {
        self.count
    }

    fn find(&self, id: u8) -> Option<usize> {
        let mut i = 0usize;
        while i < CONC_FINGERS {
            if let Some(p) = self.pts.get(i).and_then(|v| *v) {
                if p.id == id {
                    return Some(i);
                }
            }
            i += 1;
        }
        None
    }

    /// 按下：坐标界 → id 唯一 → 槽位 → 第五点落位时记录原点。
    pub fn down(&mut self, f: Finger) -> Result<u8, u32> {
        if f.x < 0 || f.y < 0 || f.x >= SEQ_W || f.y >= SEQ_H {
            return Err(DpCode::ClaimBad.val());
        }
        if self.find(f.id).is_some() {
            return Err(DpCode::SeqBad.val());
        }
        if self.count as usize >= CONC_FINGERS {
            return Err(DpCode::OrderBad.val());
        }
        let mut slot = CONC_FINGERS;
        let mut i = 0usize;
        while i < CONC_FINGERS {
            match self.pts.get(i) {
                Some(None) => {
                    slot = i;
                    i = CONC_FINGERS;
                }
                _ => i += 1,
            }
        }
        if slot == CONC_FINGERS {
            return Err(DpCode::OrderBad.val());
        }
        if let Some(cell) = self.pts.get_mut(slot) {
            *cell = Some(f);
        }
        self.count = self.count.saturating_add(1);
        if self.count as usize == CONC_FINGERS {
            // 第五点落位：整组原位快照成为分类基线。
            let mut k = 0usize;
            while k < CONC_FINGERS {
                let p = self.pts.get(k).and_then(|v| *v);
                if let Some(cell) = self.origin.get_mut(k) {
                    *cell = p;
                }
                k += 1;
            }
        }
        self.digest = p_core::fold_u32(self.digest, 1);
        self.digest = p_core::fold_u32(self.digest, f.id as u32);
        Ok(slot as u8)
    }

    /// 移动：坐标界 → id 存在 → 更新当前点（不改基线）。
    pub fn move_to(&mut self, id: u8, x: i32, y: i32) -> Result<u8, u32> {
        if x < 0 || y < 0 || x >= SEQ_W || y >= SEQ_H {
            return Err(DpCode::ClaimBad.val());
        }
        let slot = match self.find(id) {
            Some(v) => v,
            None => return Err(DpCode::SeqBad.val()),
        };
        if let Some(cell) = self.pts.get_mut(slot) {
            *cell = Some(Finger { id, x, y });
        }
        self.moves = self.moves.saturating_add(1);
        self.digest = p_core::fold_u32(self.digest, 2);
        self.digest = p_core::fold_u32(self.digest, x as u32);
        self.digest = p_core::fold_u32(self.digest, y as u32);
        Ok(slot as u8)
    }

    /// 抬起：id 存在才允许释放。
    pub fn lift(&mut self, id: u8) -> Result<u8, u32> {
        let slot = match self.find(id) {
            Some(v) => v,
            None => return Err(DpCode::SeqBad.val()),
        };
        if let Some(cell) = self.pts.get_mut(slot) {
            *cell = None;
        }
        self.count = self.count.saturating_sub(1);
        self.digest = p_core::fold_u32(self.digest, 3);
        self.digest = p_core::fold_u32(self.digest, id as u32);
        Ok(slot as u8)
    }

    /// 分类：展开量优先（捏合），其次质心位移（四向滑动），都不够 → None。
    pub fn classify(&self) -> Result<MultiGesture, u32> {
        if self.count as usize != CONC_FINGERS {
            return Err(DpCode::ClaimBad.val());
        }
        let mut k = 0usize;
        while k < CONC_FINGERS {
            if self.origin.get(k).and_then(|v| *v).is_none()
                || self.pts.get(k).and_then(|v| *v).is_none()
            {
                return Err(DpCode::OrderBad.val());
            }
            k += 1;
        }
        let (ox, oy) = centroid_of(&self.origin);
        let (cx, cy) = centroid_of(&self.pts);
        let dx = cx.saturating_sub(ox);
        let dy = cy.saturating_sub(oy);
        let dspread = spread_of(&self.pts).saturating_sub(spread_of(&self.origin));
        let adx = dx.unsigned_abs() as i64;
        let ady = dy.unsigned_abs() as i64;
        let adspread = dspread.unsigned_abs() as i64;
        // 展开量压过质心位移才算捏合：纯平移时展开量恒为零。
        if adspread >= SPREAD_MIN && adspread >= adx.saturating_add(ady) {
            return Ok(if dspread < 0 {
                MultiGesture::PinchIn
            } else {
                MultiGesture::PinchOut
            });
        }
        if adx < SWIPE_MIN && ady < SWIPE_MIN {
            return Ok(MultiGesture::None);
        }
        if adx == ady {
            return Ok(MultiGesture::None);
        }
        let (major, minor, horiz) = if adx > ady {
            (adx, ady, true)
        } else {
            (ady, adx, false)
        };
        if major.saturating_mul(2) < minor.saturating_mul(3) {
            return Ok(MultiGesture::None);
        }
        if horiz {
            Ok(if dx > 0 {
                MultiGesture::SwipeRight
            } else {
                MultiGesture::SwipeLeft
            })
        } else if dy < 0 {
            Ok(MultiGesture::SwipeUp)
        } else {
            Ok(MultiGesture::SwipeDown)
        }
    }
}

/// 五触点批量账（×100 组）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ConcurrentStats {
    pub sequences: u32,
    pub matched: u32,
    pub mismatched: u32,
    pub fingers: u32,
    pub digest: u64,
}

impl ConcurrentStats {
    pub fn new() -> ConcurrentStats {
        ConcurrentStats {
            sequences: 0,
            matched: 0,
            mismatched: 0,
            fingers: 0,
            digest: p_core::FNV_OFF,
        }
    }

    /// 100/100：逐组一致、零错配、触点数 = 组数 × 5。
    pub fn perfect(&self) -> bool {
        self.sequences > 0
            && self.matched == self.sequences
            && self.mismatched == 0
            && self.fingers == self.sequences.saturating_mul(CONC_FINGERS as u32)
            && self.digest != 0
    }

    pub fn audit(&self) -> Result<(), u32> {
        if !self.perfect() {
            return Err(DpCode::LedgerBad.val());
        }
        Ok(())
    }
}

/// 五触点 ×100：七类手势轮转（捏合进/出、四向滑动、静默），期望值由
/// 序列构造给出，逐组与识别结果比对。
pub fn concurrent_sequences(rounds: u32) -> Result<ConcurrentStats, u32> {
    if rounds == 0 || rounds > CONC_ROUNDS {
        return Err(DpCode::CoverBad.val());
    }
    let mut s = ConcurrentStats::new();
    let mut i = 0u32;
    while i < rounds {
        let want = match i % 7 {
            0 => MultiGesture::PinchIn,
            1 => MultiGesture::PinchOut,
            2 => MultiGesture::SwipeLeft,
            3 => MultiGesture::SwipeRight,
            4 => MultiGesture::SwipeUp,
            5 => MultiGesture::SwipeDown,
            _ => MultiGesture::None,
        };
        let mut r = FiveTouchReconciler::new();
        let mut k = 0usize;
        while k < CONC_FINGERS {
            let off = match RING.get(k) {
                Some(v) => *v,
                None => return Err(DpCode::CoverBad.val()),
            };
            let f = Finger {
                id: (k as u8) + 1,
                x: SEQ_CX.saturating_add(off.0),
                y: SEQ_CY.saturating_add(off.1),
            };
            r.down(f)?;
            s.fingers = s.fingers.saturating_add(1);
            k = k.saturating_add(1);
        }
        if r.count() as usize != CONC_FINGERS {
            return Err(DpCode::LedgerBad.val());
        }
        let mut m = 0usize;
        while m < CONC_FINGERS {
            let off = match RING.get(m) {
                Some(v) => *v,
                None => return Err(DpCode::CoverBad.val()),
            };
            let (nx, ny) = match want {
                MultiGesture::PinchIn => (
                    SEQ_CX.saturating_add(off.0 * 7 / 10),
                    SEQ_CY.saturating_add(off.1 * 7 / 10),
                ),
                MultiGesture::PinchOut => (
                    SEQ_CX.saturating_add(off.0 * 13 / 10),
                    SEQ_CY.saturating_add(off.1 * 13 / 10),
                ),
                MultiGesture::SwipeLeft => (
                    SEQ_CX.saturating_add(off.0).saturating_sub(150),
                    SEQ_CY.saturating_add(off.1),
                ),
                MultiGesture::SwipeRight => (
                    SEQ_CX.saturating_add(off.0).saturating_add(150),
                    SEQ_CY.saturating_add(off.1),
                ),
                MultiGesture::SwipeUp => (
                    SEQ_CX.saturating_add(off.0),
                    SEQ_CY.saturating_add(off.1).saturating_sub(150),
                ),
                MultiGesture::SwipeDown => (
                    SEQ_CX.saturating_add(off.0),
                    SEQ_CY.saturating_add(off.1).saturating_add(150),
                ),
                MultiGesture::None => (
                    SEQ_CX
                        .saturating_add(off.0)
                        .saturating_add((i % 5) as i32 - 2),
                    SEQ_CY
                        .saturating_add(off.1)
                        .saturating_add((i % 3) as i32 - 1),
                ),
            };
            r.move_to((m as u8) + 1, nx, ny)?;
            m = m.saturating_add(1);
        }
        let got = r.classify()?;
        if got != want {
            return Err(DpCode::FuzzBad.val());
        }
        s.matched = s.matched.saturating_add(1);
        s.digest = p_core::fold_u32(s.digest, got.code());
        s.digest = p_core::fold_u32(s.digest, i);
        let mut l = 1u8;
        while l <= CONC_FINGERS as u8 {
            r.lift(l)?;
            l = l.saturating_add(1);
        }
        if r.count() != 0 {
            return Err(DpCode::LedgerBad.val());
        }
        s.sequences = s.sequences.saturating_add(1);
        i = i.saturating_add(1);
    }
    s.audit()?;
    Ok(s)
}

/// 五触点复跑：两次 100 组必须逐位一致。
pub fn concurrent_replay() -> Result<u64, u32> {
    let a = concurrent_sequences(CONC_ROUNDS)?;
    let b = concurrent_sequences(CONC_ROUNDS)?;
    if a != b {
        return Err(DpCode::ReplayBad.val());
    }
    Ok(a.digest)
}

/// 五触点账本文本：`组数|一致|错配|触点数|保留|指纹`。
pub fn render_concurrent(s: &ConcurrentStats) -> String {
    format!(
        "{}|{}|{}|{}|{}|0x{:016X}",
        s.sequences, s.matched, s.mismatched, s.fingers, 0u32, s.digest
    )
}

/// 五触点账本回读：字段数、100/100、触点数吻合、指纹非零。
pub fn audit_concurrent_render(text: &str) -> Result<usize, u32> {
    let mut parts = [""; STATS_FIELDS];
    let mut n = 0usize;
    for p in text.split('|') {
        if n < STATS_FIELDS {
            if let Some(slot) = parts.get_mut(n) {
                *slot = p;
            }
        }
        n = n.saturating_add(1);
    }
    if n != STATS_FIELDS {
        return Err(DpCode::RenderBad.val());
    }
    let sequences = match parts.get(0).and_then(|v| parse_u32(v)) {
        Some(v) => v,
        None => return Err(DpCode::RenderBad.val()),
    };
    let matched = match parts.get(1).and_then(|v| parse_u32(v)) {
        Some(v) => v,
        None => return Err(DpCode::RenderBad.val()),
    };
    let mismatched = match parts.get(2).and_then(|v| parse_u32(v)) {
        Some(v) => v,
        None => return Err(DpCode::RenderBad.val()),
    };
    let fingers = match parts.get(3).and_then(|v| parse_u32(v)) {
        Some(v) => v,
        None => return Err(DpCode::RenderBad.val()),
    };
    let digest = match parts.get(5).and_then(|v| parse_hex16(v)) {
        Some(v) => v,
        None => return Err(DpCode::RenderBad.val()),
    };
    if sequences == 0 || matched != sequences || mismatched != 0 {
        return Err(DpCode::LedgerBad.val());
    }
    if fingers != sequences.saturating_mul(CONC_FINGERS as u32) {
        return Err(DpCode::LedgerBad.val());
    }
    if digest == 0 {
        return Err(DpCode::RenderBad.val());
    }
    Ok(STATS_FIELDS)
}

// ── 域检查项（动态明细槽上限 8） ──────────────────────────────────
pub fn extra_checks() -> Vec<(&'static str, bool, String)> {
    let mut out = Vec::new();

    let map_audit = audit_map();
    let feed = feed_probe(FEED_ROUNDS);
    out.push((
        "F48987 八手势映射总函数",
        map_audit == Ok(GESTURES as u32)
            && matches!(&feed, Ok(s) if s.balanced() && s.mapped == FEED_ROUNDS * GESTURES as u32),
        match (&map_audit, &feed) {
            (Ok(n), Ok(s)) => format!("map={} feed={}", n, s.mapped),
            (Err(e), _) => format!("err=0x{:X}", e),
            (_, Err(e)) => format!("err=0x{:X}", e),
        },
    ));

    let rec = recovery_rounds(RECOVER_ROUNDS);
    out.push((
        "F48987 打断×10^2恢复全过",
        matches!(&rec, Ok(s) if s.clean() && s.recovered == 25 && s.cancelled == 75),
        match &rec {
            Ok(s) => format!("rec={} can={} ok", s.recovered, s.cancelled),
            Err(e) => format!("err=0x{:X}", e),
        },
    ));

    let sem = recovery_semantics_probe();
    out.push((
        "F48987 触点丢失四语义",
        matches!(&sem, Ok(4)),
        match &sem {
            Ok(n) => format!("paths={} all-ok", n),
            Err(e) => format!("err=0x{:X}", e),
        },
    ));

    let conc = concurrent_sequences(CONC_ROUNDS);
    out.push((
        "F48988 五触点×100全识别",
        matches!(&conc, Ok(s) if s.perfect() && s.matched == CONC_ROUNDS),
        match &conc {
            Ok(s) => format!("hit={} miss={}", s.matched, s.mismatched),
            Err(e) => format!("err=0x{:X}", e),
        },
    ));

    let crender = concurrent_sequences(CONC_ROUNDS).map(|s| render_concurrent(&s));
    let creplay = concurrent_replay();
    out.push((
        "F48988 五触点复跑与账本",
        matches!(&crender, Ok(t) if audit_concurrent_render(t) == Ok(STATS_FIELDS))
            && creplay == Ok(match &conc { Ok(s) => s.digest, Err(_) => 0 }),
        match (&crender, &creplay) {
            (Ok(t), Ok(_)) => format!("txt={} rep=ok", t.len()),
            (Err(e), _) => format!("err=0x{:X}", e),
            (_, Err(e)) => format!("err=0x{:X}", e),
        },
    ));

    let prio = audit_priority();
    let tie = tie_probe();
    out.push((
        "F48988 六来源全序与早序",
        prio == Ok(SOURCES as u32)
            && audit_order() == Ok(SOURCES as u32)
            && matches!(&tie, Ok(2)),
        match (&prio, &tie) {
            (Ok(n), Ok(t)) => format!("src={} tie={}", n, t),
            (Err(e), _) => format!("err=0x{:X}", e),
            (_, Err(e)) => format!("err=0x{:X}", e),
        },
    ));

    let arb = priority_probe(ARB_ROUNDS);
    let fuzz = fuzz_claims(FUZZ_ROUNDS);
    out.push((
        "F48987/88 千轮仲裁与fuzz",
        matches!(&arb, Ok(s) if s.balanced() && s.picked == ARB_ROUNDS)
            && matches!(&fuzz, Ok(s) if s.balanced()),
        match (&arb, &fuzz) {
            (Ok(s), Ok(_)) => format!("pick={} fz={}", s.picked, s.conflicts),
            (Err(e), _) => format!("err=0x{:X}", e),
            (_, Err(e)) => format!("err=0x{:X}", e),
        },
    ));

    let conflict = conflict_probe();
    let inj = inject_rounds(4);
    let render = golden_run().map(|s| render_stats(&s));
    out.push((
        "F48987/88 歧义注入与账本",
        matches!(&conflict, Ok(4))
            && matches!(&inj, Ok(l) if l.all_caught())
            && matches!(&render, Ok(t) if audit_render(t) == Ok(STATS_FIELDS)),
        match (&conflict, &inj, &render) {
            (Ok(c), Ok(_), Ok(t)) => format!("cf={} txt={}", c, t.len()),
            (Err(e), _, _) => format!("err=0x{:X}", e),
            (_, Err(e), _) => format!("err=0x{:X}", e),
            (_, _, Err(e)) => format!("err=0x{:X}", e),
        },
    ));

    out
}

pub fn run_unx48987_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F48987 手势映射与优先级仲裁");
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

    fn claim(src: Source, seq: u32) -> Option<Claim> {
        Some(Claim::new(src, seq, Action::Click))
    }

    #[test]
    fn constants_are_anchored() {
        assert_eq!(SEG_BASE, 0x71700);
        assert_eq!(GESTURES, 8);
        assert_eq!(ACTIONS, 11);
        assert_eq!(SOURCES, 6);
        assert_eq!(MAX_CLAIMS, 8);
        assert_eq!(DpCode::ALL.len(), 14);
    }

    #[test]
    fn mapping_is_total() {
        assert_eq!(audit_map(), Ok(8));
        let mut i = 0usize;
        while i < Gesture::ALL.len() {
            assert!(map(Gesture::ALL[i]).is_some());
            i += 1;
        }
    }

    #[test]
    fn mapping_is_unique() {
        let mut i = 0usize;
        while i < MAP.len() {
            let mut j = i + 1;
            while j < MAP.len() {
                assert!(MAP[i].0 != MAP[j].0);
                j += 1;
            }
            i += 1;
        }
    }

    #[test]
    fn gesture_codes_round_trip() {
        let mut i = 0u32;
        while i < GESTURES as u32 {
            let g = Gesture::from_code(i);
            assert_eq!(g.map(|v| v.code()), Some(i));
            i += 1;
        }
        assert_eq!(Gesture::from_code(99), None);
    }

    #[test]
    fn known_mappings_are_pinned() {
        assert_eq!(map(Gesture::Tap), Some(Action::Click));
        assert_eq!(map(Gesture::DoubleTap), Some(Action::DoubleClick));
        assert_eq!(map(Gesture::LongPress), Some(Action::ContextMenu));
        assert_eq!(map(Gesture::Swipe), Some(Action::Scroll));
        assert_eq!(map(Gesture::Fling), Some(Action::FlingScroll));
        assert_eq!(map(Gesture::Pinch), Some(Action::Zoom));
        assert_eq!(map(Gesture::Rotate), Some(Action::Rotate));
        assert_eq!(map(Gesture::Hold), Some(Action::PressHold));
    }

    #[test]
    fn priority_is_total_and_distinct() {
        assert_eq!(audit_priority(), Ok(6));
        assert_eq!(audit_order(), Ok(6));
        let mut i = 0usize;
        while i < PRIO.len() {
            let mut j = i + 1;
            while j < PRIO.len() {
                assert!(PRIO[i].1 != PRIO[j].1);
                j += 1;
            }
            i += 1;
        }
    }

    #[test]
    fn priority_values_are_pinned() {
        assert_eq!(priority(Source::Wheel), Some(60));
        assert_eq!(priority(Source::Touch), Some(50));
        assert_eq!(priority(Source::Gesture), Some(45));
        assert_eq!(priority(Source::Mouse), Some(40));
        assert_eq!(priority(Source::Keyboard), Some(30));
        assert_eq!(priority(Source::System), Some(10));
    }

    #[test]
    fn empty_claims_yield_none() {
        let arr = [None; MAX_CLAIMS];
        assert_eq!(arbitrate(&arr), Ok(None));
        assert_eq!(recompute_key(&arr), Ok(None));
    }

    #[test]
    fn single_claim_wins() {
        let mut arr = [None; MAX_CLAIMS];
        arr[0] = Some(Claim::new(Source::Mouse, 1, Action::DoubleClick));
        match arbitrate(&arr) {
            Ok(Some(c)) => {
                assert_eq!(c.source, Source::Mouse);
                assert_eq!(c.action, Action::DoubleClick);
            }
            _ => assert!(false),
        }
    }

    #[test]
    fn highest_priority_wins() {
        let mut arr = [None; MAX_CLAIMS];
        arr[0] = claim(Source::Touch, 1);
        arr[1] = claim(Source::Wheel, 9);
        arr[2] = claim(Source::System, 2);
        match arbitrate(&arr) {
            Ok(Some(c)) => assert_eq!(c.source, Source::Wheel),
            _ => assert!(false),
        }
    }

    #[test]
    fn same_priority_earliest_seq_wins() {
        let mut arr = [None; MAX_CLAIMS];
        arr[0] = claim(Source::Mouse, 7);
        arr[1] = claim(Source::Mouse, 2);
        arr[2] = claim(Source::Mouse, 5);
        match arbitrate(&arr) {
            Ok(Some(c)) => assert_eq!(c.seq, 2),
            _ => assert!(false),
        }
    }

    #[test]
    fn duplicate_pair_is_ambiguous() {
        let mut arr = [None; MAX_CLAIMS];
        arr[0] = Some(Claim::new(Source::Touch, 3, Action::Click));
        arr[1] = Some(Claim::new(Source::Touch, 3, Action::Scroll));
        assert_eq!(arbitrate(&arr), Err(DpCode::SeqBad.val()));
        assert_eq!(recompute_key(&arr), Err(DpCode::SeqBad.val()));
    }

    #[test]
    fn zero_seq_is_claim_bad() {
        let mut arr = [None; MAX_CLAIMS];
        arr[0] = Some(Claim::new(Source::Touch, 0, Action::Click));
        assert_eq!(arbitrate(&arr), Err(DpCode::ClaimBad.val()));
    }

    #[test]
    fn none_action_is_claim_bad() {
        let mut arr = [None; MAX_CLAIMS];
        arr[0] = Some(Claim::new(Source::Touch, 1, Action::None));
        assert_eq!(arbitrate(&arr), Err(DpCode::ClaimBad.val()));
    }

    #[test]
    fn recompute_and_arbitrate_agree() {
        let mut i = 0u32;
        while i < 256 {
            let arr = clean_claims(i);
            let a = arbitrate(&arr);
            let b = recompute_key(&arr);
            match (a, b) {
                (Ok(Some(c)), Ok(Some((p, s)))) => {
                    assert_eq!(priority(c.source), Some(p));
                    assert_eq!(c.seq, s);
                }
                (Ok(None), Ok(None)) => {}
                _ => assert!(false),
            }
            i += 1;
        }
    }

    #[test]
    fn feed_maps_every_gesture() {
        assert!(feed_probe(8).is_ok());
    }

    #[test]
    fn feed_rejects_unknown_code() {
        let mut e = Engine::new();
        assert_eq!(
            e.feed_code(42, 1),
            Err(DpCode::GestureBad.val())
        );
        assert_eq!(e.stats().rejected, 1);
    }

    #[test]
    fn feed_rejects_tick_regression() {
        let mut e = Engine::new();
        let _ = e.feed(Gesture::Tap, 5);
        assert_eq!(e.feed(Gesture::Tap, 5), Err(DpCode::TickBad.val()));
    }

    #[test]
    fn feed_stats_balance() {
        match feed_probe(4) {
            Ok(s) => {
                assert!(s.balanced());
                assert_eq!(s.mapped, 32);
                assert_eq!(s.actions, 32);
                assert_eq!(s.rejected, 0);
            }
            Err(_) => assert!(false),
        }
    }

    #[test]
    fn feed_probe_range() {
        assert_eq!(feed_probe(0), Err(DpCode::CoverBad.val()));
        assert_eq!(
            feed_probe(FEED_ROUNDS + 1),
            Err(DpCode::CoverBad.val())
        );
    }

    #[test]
    fn priority_probe_is_exact() {
        match priority_probe(64) {
            Ok(s) => {
                assert!(s.balanced());
                assert_eq!(s.picked, 64);
                assert_eq!(s.conflicts, 0);
                assert!(s.digest != 0);
            }
            Err(_) => assert!(false),
        }
    }

    #[test]
    fn tie_probe_checks_order() {
        assert_eq!(tie_probe(), Ok(2));
    }

    #[test]
    fn conflict_probe_checks_four() {
        assert_eq!(conflict_probe(), Ok(4));
    }

    #[test]
    fn fuzz_claims_balances() {
        match fuzz_claims(FUZZ_ROUNDS) {
            Ok(s) => {
                assert!(s.balanced());
                assert!(s.digest != 0);
            }
            Err(_) => assert!(false),
        }
    }

    #[test]
    fn fuzz_rejects_zero() {
        assert_eq!(fuzz_claims(0), Err(DpCode::CoverBad.val()));
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
        assert_eq!(inject_rounds(0), Err(DpCode::CoverBad.val()));
    }

    #[test]
    fn render_round_trips() {
        match golden_run().map(|s| render_stats(&s)) {
            Ok(text) => {
                assert_eq!(text.split('|').count(), STATS_FIELDS);
                assert_eq!(audit_render(&text), Ok(STATS_FIELDS));
            }
            Err(_) => assert!(false),
        }
    }

    #[test]
    fn render_tamper_is_rejected() {
        match golden_run().map(|s| render_stats(&s)) {
            Ok(text) => {
                let bad = text.replacen("64|", "63|", 1);
                assert!(bad != text);
                assert!(audit_render(&bad).is_err());
                assert_eq!(audit_render("64|1"), Err(DpCode::RenderBad.val()));
            }
            Err(_) => assert!(false),
        }
    }

    #[test]
    fn golden_is_reproducible() {
        assert!(repro_check().is_ok());
        assert_eq!(golden_run(), golden_run());
        match golden_run() {
            Ok(s) => {
                assert_eq!(s.mapped, GOLDEN_ROUNDS * GESTURES as u32);
                assert_eq!(s.actions, s.mapped);
                assert_eq!(s.rejected, 0);
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
        assert_eq!(detail(DpCode::MapBad.val()), DpCode::MapBad.detail());
        assert_eq!(detail(0xDEAD), "未知诊断码");
    }

    #[test]
    fn checks_are_green() {
        let s = run_unx48987_checks();
        assert!(s.all_passed(), "{}", s.red_items_text());
        assert!(!s.truncated());
    }

    // ── F48987 取消与打断 ──────────────────────────────────────────
    #[test]
    fn recovery_new_is_bounded() {
        assert!(RecoveryTrack::new(0).is_err());
        assert!(RecoveryTrack::new(6).is_err());
        match RecoveryTrack::new(3) {
            Ok(t) => {
                assert_eq!(t.state, CancelState::Tracking);
                assert_eq!(t.live, 3);
                // 未决手势不允许收账。
                assert_eq!(t.audit(), Err(DpCode::OrderBad.val()));
            }
            Err(_) => assert!(false),
        }
        assert_eq!(CancelState::ALL.len(), 4);
        assert_eq!(CancelState::Cancelled.code(), 3);
        assert_eq!(CancelState::Recovered.name(), "recovered");
    }

    #[test]
    fn recovery_full_loss_and_resume() {
        match RecoveryTrack::new(3) {
            Ok(mut t) => {
                assert_eq!(t.lose(3), Ok(0));
                assert_eq!(t.lost, 3);
                assert_eq!(t.state, CancelState::Tracking);
                assert_eq!(t.resume(3), Ok(3));
                assert_eq!(t.state, CancelState::Recovered);
                assert_eq!(t.resumed, 3);
                assert_eq!(t.audit(), Ok(()));
                // 已决手势不得再丢点。
                assert_eq!(t.lose(1), Err(DpCode::OrderBad.val()));
            }
            Err(_) => assert!(false),
        }
    }

    #[test]
    fn recovery_half_resume_must_cancel() {
        match RecoveryTrack::new(4) {
            Ok(mut t) => {
                let _ = t.lose(4);
                let _ = t.resume(2);
                // 补不齐：不算恢复。
                assert_eq!(t.state, CancelState::Tracking);
                assert_eq!(t.resume(2), Ok(4));
                // 补齐后才是恢复；此分支验证边界值。
                assert_eq!(t.state, CancelState::Recovered);
                assert_eq!(t.audit(), Ok(()));
            }
            Err(_) => assert!(false),
        }
        match RecoveryTrack::new(4) {
            Ok(mut t) => {
                let _ = t.lose(4);
                let _ = t.resume(1);
                assert_eq!(t.cancel(), Ok(()));
                assert_eq!(t.state, CancelState::Cancelled);
                assert_eq!(t.cancelled, 1);
                assert_eq!(t.resumed, 1);
                assert_eq!(t.audit(), Ok(()));
            }
            Err(_) => assert!(false),
        }
    }

    #[test]
    fn recovery_partial_release_cancels() {
        match RecoveryTrack::new(2) {
            Ok(mut t) => {
                assert_eq!(t.partial_release(1), Ok(1));
                assert_eq!(t.state, CancelState::Cancelled);
                assert_eq!(t.live, 1);
                // 取消后补点/取消/完成都必须 OrderBad。
                assert_eq!(t.resume(1), Err(DpCode::OrderBad.val()));
                assert_eq!(t.cancel(), Err(DpCode::OrderBad.val()));
                assert_eq!(t.complete(), Err(DpCode::OrderBad.val()));
                assert_eq!(t.audit(), Ok(()));
            }
            Err(_) => assert!(false),
        }
    }

    #[test]
    fn recovery_overdraw_is_rejected() {
        match RecoveryTrack::new(3) {
            Ok(mut t) => {
                assert_eq!(t.lose(0), Err(DpCode::ClaimBad.val()));
                assert_eq!(t.lose(4), Err(DpCode::ClaimBad.val()));
                // 未丢点就补：超过起手数。
                assert_eq!(t.resume(1), Err(DpCode::ClaimBad.val()));
                assert_eq!(t.complete(), Ok(()));
                assert_eq!(t.state, CancelState::Recovered);
                assert_eq!(t.audit(), Ok(()));
            }
            Err(_) => assert!(false),
        }
    }

    #[test]
    fn recovery_100_rounds_clean() {
        match recovery_rounds(RECOVER_ROUNDS) {
            Ok(s) => {
                assert_eq!(s.rounds, RECOVER_ROUNDS);
                // i%4：0 恢复，其余取消 → 25 / 75。
                assert_eq!(s.recovered, 25);
                assert_eq!(s.cancelled, 75);
                assert!(s.clean());
                assert_eq!(s.audit(), Ok(()));
            }
            Err(_) => assert!(false),
        }
        assert_eq!(recovery_semantics_probe(), Ok(4));
        assert_eq!(recovery_rounds(0), Err(DpCode::CoverBad.val()));
        assert_eq!(
            recovery_rounds(RECOVER_ROUNDS + 1),
            Err(DpCode::CoverBad.val())
        );
    }

    // ── F48988 五触点并发对账 ──────────────────────────────────────
    fn seeded() -> FiveTouchReconciler {
        let mut r = FiveTouchReconciler::new();
        let mut k = 0usize;
        while k < CONC_FINGERS {
            let off = RING[k];
            let f = Finger {
                id: (k as u8) + 1,
                x: SEQ_CX + off.0,
                y: SEQ_CY + off.1,
            };
            assert!(r.down(f).is_ok());
            k += 1;
        }
        r
    }

    fn move_all(r: &mut FiveTouchReconciler, sx: i64, sy: i64, num: i64, den: i64) {
        let mut k = 0usize;
        while k < CONC_FINGERS {
            let off = RING[k];
            let nx = SEQ_CX + ((off.0 as i64 * num) / den) as i32 + sx as i32;
            let ny = SEQ_CY + ((off.1 as i64 * num) / den) as i32 + sy as i32;
            assert!(r.move_to((k as u8) + 1, nx, ny).is_ok());
            k += 1;
        }
    }

    #[test]
    fn multi_gesture_codes_are_closed() {
        assert_eq!(MultiGesture::ALL.len(), 7);
        assert_eq!(MultiGesture::None.code(), 0);
        assert_eq!(MultiGesture::SwipeDown.name(), "swipe-down");
        assert_eq!(CONC_FINGERS, 5);
        assert!(SPREAD_MIN > 0 && SWIPE_MIN > SPREAD_MIN);
    }

    #[test]
    fn five_touch_pinches() {
        let mut a = seeded();
        move_all(&mut a, 0, 0, 7, 10);
        assert_eq!(a.classify(), Ok(MultiGesture::PinchIn));
        let mut b = seeded();
        move_all(&mut b, 0, 0, 13, 10);
        assert_eq!(b.classify(), Ok(MultiGesture::PinchOut));
    }

    #[test]
    fn five_touch_swipes() {
        let dirs = [
            (-150i64, 0i64, MultiGesture::SwipeLeft),
            (150, 0, MultiGesture::SwipeRight),
            (0, -150, MultiGesture::SwipeUp),
            (0, 150, MultiGesture::SwipeDown),
        ];
        let mut i = 0usize;
        while i < dirs.len() {
            let (sx, sy, want) = dirs[i];
            let mut r = seeded();
            move_all(&mut r, sx, sy, 10, 10);
            assert_eq!(r.classify(), Ok(want));
            i += 1;
        }
    }

    #[test]
    fn five_touch_quiet_and_rejections() {
        let mut r = seeded();
        move_all(&mut r, 2, 1, 10, 10);
        assert_eq!(r.classify(), Ok(MultiGesture::None));
        // 不足五指不能分类。
        let mut q = FiveTouchReconciler::new();
        assert!(q.down(Finger { id: 1, x: 100, y: 100 }).is_ok());
        assert_eq!(q.classify(), Err(DpCode::ClaimBad.val()));
        // 重复 id / 越界坐标 / 未知 id。
        assert_eq!(
            q.down(Finger { id: 1, x: 200, y: 100 }),
            Err(DpCode::SeqBad.val())
        );
        assert_eq!(
            q.down(Finger { id: 2, x: -1, y: 100 }),
            Err(DpCode::ClaimBad.val())
        );
        assert_eq!(q.move_to(9, 300, 300), Err(DpCode::SeqBad.val()));
        assert_eq!(q.lift(9), Err(DpCode::SeqBad.val()));
        // 满组后再按：OrderBad。
        let mut full = seeded();
        assert_eq!(
            full.down(Finger { id: 9, x: 500, y: 500 }),
            Err(DpCode::OrderBad.val())
        );
    }

    #[test]
    fn concurrent_100_is_perfect() {
        match (concurrent_sequences(CONC_ROUNDS), concurrent_replay()) {
            (Ok(s), Ok(d)) => {
                assert_eq!(s.sequences, CONC_ROUNDS);
                assert_eq!(s.matched, CONC_ROUNDS);
                assert_eq!(s.mismatched, 0);
                assert_eq!(s.fingers, CONC_ROUNDS * CONC_FINGERS as u32);
                assert!(s.perfect());
                assert_eq!(s.digest, d);
                let text = render_concurrent(&s);
                assert_eq!(text.split('|').count(), STATS_FIELDS);
                assert_eq!(
                    audit_concurrent_render(&text),
                    Ok(STATS_FIELDS)
                );
                let bad = text.replacen("|100|0|", "|99|0|", 1);
                assert!(bad != text);
                assert!(audit_concurrent_render(&bad).is_err());
                assert_eq!(
                    audit_concurrent_render("100|1"),
                    Err(DpCode::RenderBad.val())
                );
            }
            _ => assert!(false),
        }
    }

    #[test]
    fn concurrent_rounds_are_bounded() {
        assert_eq!(concurrent_sequences(0), Err(DpCode::CoverBad.val()));
        assert_eq!(
            concurrent_sequences(CONC_ROUNDS + 1),
            Err(DpCode::CoverBad.val())
        );
    }
}
