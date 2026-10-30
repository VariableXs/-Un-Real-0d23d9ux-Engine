//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F15001 · VSCROLL 拖动六态序（起/拖/越界/回弹/放/取消） · 0x66E00
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定）：
//!   ① 纯功能代码 ≥270 行；
//!   ② UNX-F15001-J1 拖动拇指六态（起/拖/越界/回弹/放/取消）
//!      6 元组对照一致；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - 六态闭集（起 Armed/拖 Dragging/越界 Overflow/回弹 Bouncing/
//!     放 Released/取消 Cancelled）+ 前置态 Idle，共 7 相；
//!   - 合法边闭集表：7×7 布尔矩阵写死，非法转换一律拒收（诊断码
//!     IllegalTransition），绝不允许隐式跳态；
//!   - 轨道几何写死：轨道 0..=100（来源：VSCROLL 标准轨道域），拇指
//!     长 20，故拇指原点钳位域 0..=80，越界事件先记越界态再回弹钳位；
//!   - 6 元组黄金对照：规范双会话跑（会话 A 走完 起→拖→越界→回弹→放；
//!     会话 B 走 起→拖→取消）后六相各恰好发生一次 → (1,1,1,1,1,1)；
//!   - 守恒恒等式：六相发生计数总和 = 已消费事件中被接收的数目；
//!   - LCG 种子写死 = SEG_BASE（0x66E00），禁 rand/浮点/IO/墙钟；
//!     no_std + alloc；零 unwrap/expect/panic。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x66E00，AI-VX3-1030 独占区）。
pub const SEG_BASE: u32 = 0x66E00;

// ── 六态闭集常量 ─────────────────────────────────────────────────
pub const PH_ARMED: usize = 0; // 起
pub const PH_DRAGGING: usize = 1; // 拖
pub const PH_OVERFLOW: usize = 2; // 越界
pub const PH_BOUNCING: usize = 3; // 回弹
pub const PH_RELEASED: usize = 4; // 放
pub const PH_CANCELLED: usize = 5; // 取消
pub const PH_COUNT: usize = 6;
pub const PH_NAMES: [&str; PH_COUNT] =
    ["起", "拖", "越界", "回弹", "放", "取消"];

// ── 轨道几何（写死） ─────────────────────────────────────────────
/// 轨道域 0..=100（VSCROLL 标准轨道域，写死）。
pub const TRACK_MIN: i32 = 0;
pub const TRACK_MAX: i32 = 100;
/// 拇指长度 20（写死），拇指原点钳位域 [TRACK_MIN, TRACK_MAX-THUMB_EXT]。
pub const THUMB_EXT: i32 = 20;
/// 拇指原点上界 = 100 - 20 = 80。
pub const THUMB_MAX: i32 = TRACK_MAX - THUMB_EXT;

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum VscCode {
    /// 相位越闭集
    PhaseOutOfRange,
    /// 非法状态转换（边闭集外）
    IllegalTransition,
    /// 拇指位置越钳位域
    ThumbOutOfRange,
    /// 越界未回弹（回弹缺失）
    BounceMissing,
    /// 6 元组与黄金不一致
    TupleMismatch,
    /// 会话未收敛到终态
    SessionHung,
    /// 账本不守恒（相计数≠接收事件数）
    LedgerUnbalanced,
    /// 双跑不一致（不可复现）
    NotReproducible,
    /// 终态后仍有事件注入
    EventAfterTerminal,
    /// 会话未入账
    SessionUnbooked,
}

impl VscCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            VscCode::PhaseOutOfRange => "相位越闭集：六态冻结",
            VscCode::IllegalTransition => "非法状态转换：边闭集外拒收",
            VscCode::ThumbOutOfRange => "拇指位置越钳位域 0..=80",
            VscCode::BounceMissing => "越界未回弹：回弹缺失",
            VscCode::TupleMismatch => "6 元组与黄金对照不一致",
            VscCode::SessionHung => "会话未收敛到终态",
            VscCode::LedgerUnbalanced => "账本不守恒：相计数≠接收事件数",
            VscCode::NotReproducible => "双跑不一致：复现红线",
            VscCode::EventAfterTerminal => "终态后仍有事件注入",
            VscCode::SessionUnbooked => "会话未入账",
        }
    }
    pub const ALL: [VscCode; 10] = [
        VscCode::PhaseOutOfRange,
        VscCode::IllegalTransition,
        VscCode::ThumbOutOfRange,
        VscCode::BounceMissing,
        VscCode::TupleMismatch,
        VscCode::SessionHung,
        VscCode::LedgerUnbalanced,
        VscCode::NotReproducible,
        VscCode::EventAfterTerminal,
        VscCode::SessionUnbooked,
    ];
}

pub fn detail(code: u32) -> &'static str {
    let mut i = 0;
    while i < VscCode::ALL.len() {
        if VscCode::ALL[i].val() == code {
            return VscCode::ALL[i].detail();
        }
        i += 1;
    }
    "未知诊断码：不在 0x66E00 段闭集内"
}

// ── 状态机：7 相 + 事件闭集 ──────────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Phase {
    Idle,
    Armed,
    Dragging,
    Overflow,
    Bouncing,
    Released,
    Cancelled,
}

impl Phase {
    pub fn idx(self) -> Result<usize, u32> {
        match self {
            Phase::Idle => Err(VscCode::PhaseOutOfRange.val()), // Idle 不入六相账
            Phase::Armed => Ok(PH_ARMED),
            Phase::Dragging => Ok(PH_DRAGGING),
            Phase::Overflow => Ok(PH_OVERFLOW),
            Phase::Bouncing => Ok(PH_BOUNCING),
            Phase::Released => Ok(PH_RELEASED),
            Phase::Cancelled => Ok(PH_CANCELLED),
        }
    }
    pub fn is_terminal(self) -> bool {
        self == Phase::Released || self == Phase::Cancelled
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Ev {
    /// 起：抓住拇指
    Grab,
    /// 拖：移动（携带拇指原点目标位）
    Move(i32),
    /// 越界：移动出钳位域（携带原始目标位）
    OutRaw(i32),
    /// 回弹
    Back,
    /// 放
    Drop,
    /// 取消（Esc）
    Escape,
}

/// 7×7 合法边闭集表（写死）：边表[from][to]=true 表示允许。
/// 规则来源：拖动语义推定，冻结如下——
///   Idle→Armed（起）；Armed→Dragging（拖）|Cancelled（取消）；
///   Dragging→Overflow（越界）|Dragging（拖内移动）|Released（放）|Cancelled；
///   Overflow→Bouncing（回弹）|Cancelled；
///   Bouncing→Dragging（回弹后继续拖）|Released（放）|Cancelled；
///   Released/Cancelled 为终态，无出边。
fn edge_allowed(from: Phase, to: Phase) -> bool {
    match (from, to) {
        (Phase::Idle, Phase::Armed) => true,
        (Phase::Armed, Phase::Dragging) => true,
        (Phase::Armed, Phase::Cancelled) => true,
        (Phase::Dragging, Phase::Dragging) => true,
        (Phase::Dragging, Phase::Overflow) => true,
        (Phase::Dragging, Phase::Released) => true,
        (Phase::Dragging, Phase::Cancelled) => true,
        (Phase::Overflow, Phase::Bouncing) => true,
        (Phase::Overflow, Phase::Cancelled) => true,
        (Phase::Bouncing, Phase::Dragging) => true,
        (Phase::Bouncing, Phase::Released) => true,
        (Phase::Bouncing, Phase::Cancelled) => true,
        _ => false,
    }
}

/// 一步转移：先查终态拒收，再查边闭集，再处理位置钳位。
pub fn step(state: Phase, ev: Ev, thumb: i32) -> Result<Phase, u32> {
    if state.is_terminal() {
        return Err(VscCode::EventAfterTerminal.val());
    }
    let target = match ev {
        Ev::Grab => Phase::Armed,
        Ev::Move(_) => Phase::Dragging,
        Ev::OutRaw(_) => Phase::Overflow,
        Ev::Back => Phase::Bouncing,
        Ev::Drop => Phase::Released,
        Ev::Escape => Phase::Cancelled,
    };
    if !edge_allowed(state, target) {
        return Err(VscCode::IllegalTransition.val());
    }
    // 位置合法性：Move 目标必须在钳位域内；OutRaw 必须在域外。
    match ev {
        Ev::Move(p) => {
            if p < TRACK_MIN || p > THUMB_MAX {
                return Err(VscCode::ThumbOutOfRange.val());
            }
        }
        Ev::OutRaw(p) => {
            if p <= THUMB_MAX {
                return Err(VscCode::ThumbOutOfRange.val());
            }
        }
        _ => {}
    }
    Ok(target)
}

/// 回弹钳位：越界原始位回弹到 THUMB_MAX。
pub fn bounce_clamp(raw: i32) -> Result<i32, u32> {
    if raw <= THUMB_MAX {
        return Err(VscCode::ThumbOutOfRange.val());
    }
    Ok(THUMB_MAX)
}

// ── 6 元组 ───────────────────────────────────────────────────────
/// 六相发生计数元组：(起,拖,越界,回弹,放,取消)。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct SixTuple {
    pub c: [u32; PH_COUNT],
}

impl SixTuple {
    pub const fn zero() -> SixTuple {
        SixTuple { c: [0; PH_COUNT] }
    }
    pub fn bump(&mut self, ph: usize) -> Result<(), u32> {
        if ph >= PH_COUNT {
            return Err(VscCode::PhaseOutOfRange.val());
        }
        self.c[ph] += 1;
        Ok(())
    }
    pub fn total(&self) -> u32 {
        let mut t = 0;
        let mut i = 0;
        while i < PH_COUNT {
            t += self.c[i];
            i += 1;
        }
        t
    }
}

/// 黄金 6 元组：规范双会话跑后六相各恰好 1 次。
pub const GOLDEN_SIX: SixTuple = SixTuple { c: [1, 1, 1, 1, 1, 1] };

// ── 拖动会话账本 ─────────────────────────────────────────────────
pub struct DragLedger {
    pub state: Phase,
    pub thumb: i32,
    pub six: SixTuple,
    pub accepted: u32,
    pub sessions: u32,
    pub booked: u32,
}

impl DragLedger {
    pub fn new() -> DragLedger {
        DragLedger {
            state: Phase::Idle,
            thumb: TRACK_MIN,
            six: SixTuple::zero(),
            accepted: 0,
            sessions: 0,
            booked: 0,
        }
    }

    /// 注入一个事件；内部更新状态/拇指/六相计数。
    pub fn feed(&mut self, ev: Ev, arg: i32) -> Result<Phase, u32> {
        let next = step(self.state, ev, arg)?;
        self.state = next;
        match ev {
            Ev::Move(p) => self.thumb = p,
            Ev::OutRaw(p) => {
                let cl = bounce_clamp(p)?;
                self.thumb = cl; // 越界即记回弹钳位落点
            }
            _ => {}
        }
        if let Ok(ix) = next.idx() {
            self.six.bump(ix)?;
        }
        self.accepted += 1;
        if next.is_terminal() {
            self.sessions += 1;
            self.booked += 1;
            self.state = Phase::Idle; // 终态后回到前置态，开新会话
        }
        Ok(next)
    }

    /// 规范双会话跑：A=起,拖,越界,回弹,放；B=起,拖,取消。
    pub fn canonical_run(&mut self) -> Result<(), u32> {
        self.feed(Ev::Grab, 0)?;
        self.feed(Ev::Move(40), 40)?;
        self.feed(Ev::OutRaw(THUMB_MAX + 15), THUMB_MAX + 15)?;
        self.feed(Ev::Back, 0)?;
        self.feed(Ev::Drop, 0)?;
        self.feed(Ev::Grab, 0)?;
        self.feed(Ev::Move(10), 10)?;
        self.feed(Ev::Escape, 0)?;
        Ok(())
    }

    /// 6 元组对照。
    pub fn tuple_match(&self) -> bool {
        self.six == GOLDEN_SIX
    }

    /// 守恒：六相计数总和 = 接收事件数。
    pub fn conserved(&self) -> bool {
        self.six.total() == self.accepted
    }

    /// 会话账：两次会话都已入账。
    pub fn sessions_booked(&self) -> bool {
        self.sessions == 2 && self.booked == 2
    }
}

// ── 确定性 LCG（种子=SEG_BASE） ─────────────────────────────────
pub struct Lcg(u64);
impl Lcg {
    pub const fn new(seed: u64) -> Lcg {
        Lcg(seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407))
    }
    pub fn next(&mut self) -> u64 {
        self.0 = self
            .0
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        self.0 >> 33
    }
    pub fn below(&mut self, n: u64) -> u64 {
        self.next() % (n | 1)
    }
}

// ── 黄金正演 ─────────────────────────────────────────────────────
pub fn golden_run() -> Result<(u32, u32, u32), u32> {
    let mut led = DragLedger::new();
    led.canonical_run()?;
    if !led.tuple_match() {
        return Err(VscCode::TupleMismatch.val());
    }
    if !led.conserved() {
        return Err(VscCode::LedgerUnbalanced.val());
    }
    if !led.sessions_booked() {
        return Err(VscCode::SessionUnbooked.val());
    }
    // 复现：双跑一致。
    let mut led2 = DragLedger::new();
    led2.canonical_run()?;
    if led2.six != led.six {
        return Err(VscCode::NotReproducible.val());
    }
    Ok((led.accepted, led.sessions, led.six.total()))
}

// ── 注入面：三族 tries/caught 全抓 ───────────────────────────────
pub struct InjectLedger {
    pub tries: [u32; 3],
    pub caught: [u32; 3],
}

impl InjectLedger {
    pub const fn new() -> InjectLedger {
        InjectLedger { tries: [0; 3], caught: [0; 3] }
    }
    pub fn all_caught(&self) -> bool {
        let mut i = 0;
        while i < 3 {
            if self.caught[i] != self.tries[i] {
                return false;
            }
            i += 1;
        }
        true
    }
}

pub fn run_injections(ledger: &mut InjectLedger) {
    // 族 0：非法边（Idle 直接 Drop，跳过起）。
    ledger.tries[0] += 1;
    let mut bad0 = false;
    if let Err(e) = step(Phase::Idle, Ev::Drop, 0) {
        if e == VscCode::IllegalTransition.val() {
            bad0 = true;
        }
    }
    if bad0 {
        ledger.caught[0] += 1;
    }
    // 族 1：拇指越钳位域（Move 200 > 80）。
    ledger.tries[1] += 1;
    let mut bad1 = false;
    if let Err(e) = step(Phase::Armed, Ev::Move(200), 200) {
        if e == VscCode::ThumbOutOfRange.val() {
            bad1 = true;
        }
    }
    if bad1 {
        ledger.caught[1] += 1;
    }
    // 族 2：终态后再注入事件。
    ledger.tries[2] += 1;
    let mut bad2 = false;
    if let Err(e) = step(Phase::Released, Ev::Move(5), 5) {
        if e == VscCode::EventAfterTerminal.val() {
            bad2 = true;
        }
    }
    if bad2 {
        ledger.caught[2] += 1;
    }
}

// ── 判据入口 ─────────────────────────────────────────────────────
pub fn segment_guard() -> bool {
    let mut i = 0;
    while i < VscCode::ALL.len() {
        let v = VscCode::ALL[i].val();
        if v < SEG_BASE || v >= SEG_BASE + 0x40 {
            return false;
        }
        i += 1;
    }
    true
}

pub fn extra_checks() -> Vec<(&'static str, bool, String)> {
    let mut v: Vec<(&'static str, bool, String)> = Vec::new();
    // 1. 黄金正演：双会话 6 元组一致 + 守恒 + 复现。
    let golden = golden_run();
    let g_ok = match &golden {
        Ok((acc, ses, tot)) => *acc == 8 && *ses == 2 && *tot == 6,
        Err(_) => false,
    };
    v.push(("J1-双会话6元组+守恒+复现", g_ok, format!("{:?}", golden)));
    // 2. 注入三族全抓。
    let mut inj = InjectLedger::new();
    run_injections(&mut inj);
    v.push((
        "J1-注入三族全抓",
        inj.all_caught(),
        format!("t/c {:?}/{:?}", inj.tries, inj.caught),
    ));
    // 3. 六相名点验。
    let mut named = 0;
    let mut i = 0;
    while i < PH_COUNT {
        if PH_NAMES[i].len() > 0 {
            named += 1;
        }
        i += 1;
    }
    v.push(("J1-六相名点验", named == 6, format!("{}/6", named)));
    // 4. 钳位域边界：0 与 80 合法，81 非法（Move）。
    let lo_ok = step(Phase::Armed, Ev::Move(TRACK_MIN), 0).is_ok();
    let hi_ok = step(Phase::Armed, Ev::Move(THUMB_MAX), THUMB_MAX).is_ok();
    let over_bad = step(Phase::Armed, Ev::Move(THUMB_MAX + 1), THUMB_MAX + 1).is_err();
    v.push((
        "J1-钳位域边界三点",
        lo_ok && hi_ok && over_bad,
        format!("lo={} hi={} over_bad={}", lo_ok, hi_ok, over_bad),
    ));
    // 5. 边闭集抽验：Armed→Overflow 非法（必须先拖）。
    let skip_bad = step(Phase::Armed, Ev::OutRaw(THUMB_MAX + 1), THUMB_MAX + 1).is_err();
    v.push((
        "J1-起直达越界拒收",
        skip_bad,
        String::from("边闭集：Armed→Overflow 禁"),
    ));
    // 6. 回弹钳位恒等：任意越界原始位回弹后落点恒为 THUMB_MAX。
    let mut lcg = Lcg::new(SEG_BASE as u64);
    let mut clamp_ok = 0;
    let mut k = 0;
    while k < 16 {
        let raw = THUMB_MAX + 1 + (lcg.below(500) as i32);
        if bounce_clamp(raw) == Ok(THUMB_MAX) {
            clamp_ok += 1;
        }
        k += 1;
    }
    v.push((
        "J1-回弹钳位16点恒等",
        clamp_ok == 16,
        format!("{}/16", clamp_ok),
    ));
    // 7. 段位守卫。
    v.push(("J3-段位守卫", segment_guard(), String::from("0x66E00 窗口")));
    v
}

pub fn run_unx15001_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F15001 VSCROLL 拖动六态序（起/拖/越界/回弹/放/取消）");
    for (n, p, d) in extra_checks() {
        s.add_dyn(n, p, &d);
    }
    s
}
