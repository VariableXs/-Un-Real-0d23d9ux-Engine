//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F15601 · DoDragDrop 状态机总纲 · 0x67580
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定，CoRun·UNX-D5-B21）：
//!   ① 纯功能代码 ≥350 行；
//!   ② UNX-F15601-J1 状态机成文（空闲/拖动/放落/取消四态迁移 10/10
//!     次），非法迁移注入（取消态直接放落）检出 10/10 次；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - 四态闭集：Idle（空闲）→ Dragging（拖动）→ Dropped（放落）/
//!     Cancelled（取消）——后两态为吸收终态；
//!   - 合法边闭集：Idle→Dragging（起拖）；Dragging→Dropped（放落）；
//!     Dragging→Cancelled（Esc 取消）；其余边恒非法，特别是
//!     Cancelled→Dropped（取消态直接放落 = 注入目标）；
//!   - 事件闭集：Start/Drop/Cancel 三事件驱动，事件与当前态不配
//!     即拒（Idle 收 Drop、终态收任何事件）；
//!   - 会话账：每会话必有终态（收敛恒等式）；终态计 Dropped+
//!     Cancelled = 会话总数；
//!   - 非法迁移注入面 10/10 全抓（取消态直接放落、空闲放落等）；
//!   - LCG 种子写死 = SEG_BASE（0x67580）；no_std + alloc；
//!     零 unwrap/expect/panic；禁 rand/浮点/IO/墙钟。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x67580，AI-VX3-1030 批13 独占区）。
pub const SEG_BASE: u32 = 0x67580;

// ── 四态闭集 ─────────────────────────────────────────────────────
pub const ST_IDLE: usize = 0;
pub const ST_DRAGGING: usize = 1;
pub const ST_DROPPED: usize = 2;
pub const ST_CANCELLED: usize = 3;
pub const ST_COUNT: usize = 4;
pub const ST_NAMES: [&str; ST_COUNT] = ["空闲", "拖动", "放落", "取消"];

// ── 事件闭集 ─────────────────────────────────────────────────────
pub const EV_START: usize = 0;
pub const EV_DROP: usize = 1;
pub const EV_CANCEL: usize = 2;
pub const EV_COUNT: usize = 3;
pub const EV_NAMES: [&str; EV_COUNT] = ["Start", "Drop", "Cancel"];

/// 放落效果闭集（DROPEFFECT 建模）。
pub const EFF_NONE: usize = 0;
pub const EFF_COPY: usize = 1;
pub const EFF_MOVE: usize = 2;
pub const EFF_LINK: usize = 4;
pub const EFF_COUNT: usize = 4;
pub const EFF_NAMES: [&str; EFF_COUNT] = ["None", "Copy", "Move", "Link"];

pub fn eff_legal(e: usize) -> bool {
    e == EFF_NONE || e == EFF_COPY || e == EFF_MOVE || e == EFF_LINK
}

/// 判据迭代轮数（写死：判据原文 10/10 次）。
pub const JUDGE_ROUNDS: u32 = 10;

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DragCode {
    /// 状态下标越闭集
    StateOutOfRange,
    /// 事件下标越闭集
    EventOutOfRange,
    /// 非法迁移（含取消态直接放落，如实拒）
    IllegalTransition,
    /// 空闲收放落（事件与态不配）
    DropInIdle,
    /// 终态收事件（吸收态被破坏）
    EventInFinal,
    /// 双起拖（Dragging 收 Start）
    DoubleStart,
    /// 放落效果越闭集
    EffectOutOfRange,
    /// 会话不收敛（无终态悬挂）
    SessionHung,
    /// 会话守恒破坏（终态计≠会话数）
    SessionConservationBroken,
    /// 非法迁移注入漏抓
    TransitionLeak,
}

impl DragCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            DragCode::StateOutOfRange => "状态下标越闭集：四态冻结",
            DragCode::EventOutOfRange => "事件下标越闭集：三事件冻结",
            DragCode::IllegalTransition => "非法迁移：取消态直接放落等",
            DragCode::DropInIdle => "空闲收放落：事件态不配",
            DragCode::EventInFinal => "终态收事件：吸收态破坏",
            DragCode::DoubleStart => "双起拖：Dragging 收 Start",
            DragCode::EffectOutOfRange => "放落效果越闭集：四效果冻结",
            DragCode::SessionHung => "会话不收敛：无终态悬挂",
            DragCode::SessionConservationBroken => "会话守恒破坏：终态计≠会话数",
            DragCode::TransitionLeak => "非法迁移注入漏抓：检出 10/10 红线",
        }
    }
    pub const ALL: [DragCode; 10] = [
        DragCode::StateOutOfRange,
        DragCode::EventOutOfRange,
        DragCode::IllegalTransition,
        DragCode::DropInIdle,
        DragCode::EventInFinal,
        DragCode::DoubleStart,
        DragCode::EffectOutOfRange,
        DragCode::SessionHung,
        DragCode::SessionConservationBroken,
        DragCode::TransitionLeak,
    ];
}

pub fn detail(code: u32) -> &'static str {
    let mut i = 0;
    while i < DragCode::ALL.len() {
        if DragCode::ALL[i].val() == code {
            return DragCode::ALL[i].detail();
        }
        i += 1;
    }
    "未知诊断码：不在 0x67580 段闭集内"
}

// ── 迁移边闭集表（写死 4×3 格：态×事件） ─────────────────────────
/// 边表语义：cell = (态, 事件) → 目标态或 0xFF（拒收）。
///   Idle  ×Start→Dragging；Idle×Drop=拒；Idle×Cancel=拒；
///   Dragging×Drop→Dropped；Dragging×Cancel→Cancelled；
///   Dragging×Start=拒（双起拖）；
///   Dropped/Cancelled（终态）×任何事件=拒。
pub const TRANS_DENY: u8 = 0xFF;
pub const TRANS_TABLE: [[u8; EV_COUNT]; ST_COUNT] = [
    [ST_DRAGGING as u8, TRANS_DENY, TRANS_DENY], // Idle
    [TRANS_DENY, ST_DROPPED as u8, ST_CANCELLED as u8], // Dragging
    [TRANS_DENY, TRANS_DENY, TRANS_DENY],        // Dropped
    [TRANS_DENY, TRANS_DENY, TRANS_DENY],        // Cancelled
];

/// 边闭集查询：合法迁移返回目标态，否则 Err。
pub fn transition(state: usize, ev: usize) -> Result<usize, u32> {
    if state >= ST_COUNT {
        return Err(DragCode::StateOutOfRange.val());
    }
    if ev >= EV_COUNT {
        return Err(DragCode::EventOutOfRange.val());
    }
    let t = TRANS_TABLE[state][ev];
    if t == TRANS_DENY {
        // 拒收细分：空闲收放落 / 双起拖 / 终态收事件。
        if state == ST_IDLE && ev == EV_DROP {
            return Err(DragCode::DropInIdle.val());
        }
        if state == ST_DRAGGING && ev == EV_START {
            return Err(DragCode::DoubleStart.val());
        }
        return Err(DragCode::EventInFinal.val());
    }
    Ok(t as usize)
}

// ── 拖放会话账 ───────────────────────────────────────────────────
#[derive(Clone, Debug)]
pub struct Session {
    pub state: usize,
    pub effect: Option<usize>,
    pub steps: u32,
    pub ended: bool,
}

pub struct DragLedger {
    pub sessions: Vec<Session>,
    pub dropped_cnt: u32,
    pub cancelled_cnt: u32,
}

impl DragLedger {
    pub fn new() -> DragLedger {
        DragLedger { sessions: Vec::new(), dropped_cnt: 0, cancelled_cnt: 0 }
    }

    /// 新会话（Idle 起手）。
    pub fn open(&mut self) -> usize {
        self.sessions.push(Session {
            state: ST_IDLE,
            effect: None,
            steps: 0,
            ended: false,
        });
        self.sessions.len() - 1
    }

    /// 事件驱动迁移。
    pub fn feed(&mut self, h: usize, ev: usize) -> Result<usize, u32> {
        if h >= self.sessions.len() {
            return Err(DragCode::StateOutOfRange.val());
        }
        let st = self.sessions[h].state;
        let nxt = transition(st, ev)?;
        self.sessions[h].state = nxt;
        self.sessions[h].steps += 1;
        if nxt == ST_DROPPED || nxt == ST_CANCELLED {
            self.sessions[h].ended = true;
            if nxt == ST_DROPPED {
                self.dropped_cnt += 1;
            } else {
                self.cancelled_cnt += 1;
            }
        }
        Ok(nxt)
    }

    /// 放落效果落账（仅 Dropped 态可落）。
    pub fn set_effect(&mut self, h: usize, eff: usize) -> Result<(), u32> {
        if h >= self.sessions.len() {
            return Err(DragCode::StateOutOfRange.val());
        }
        if !eff_legal(eff) {
            return Err(DragCode::EffectOutOfRange.val());
        }
        if self.sessions[h].state != ST_DROPPED {
            return Err(DragCode::IllegalTransition.val());
        }
        self.sessions[h].effect = Some(eff);
        Ok(())
    }

    /// 会话收敛校验：每会话必有终态。
    pub fn converge(&self) -> Result<(), u32> {
        let mut i = 0;
        while i < self.sessions.len() {
            if !self.sessions[i].ended {
                return Err(DragCode::SessionHung.val());
            }
            i += 1;
        }
        if self.dropped_cnt + self.cancelled_cnt != self.sessions.len() as u32 {
            return Err(DragCode::SessionConservationBroken.val());
        }
        Ok(())
    }

    /// 判据正演：四态迁移 10/10 轮（放落/取消各半）。
    pub fn judge_run(&mut self) -> Result<(u32, u32), u32> {
        let mut ok_drop = 0;
        let mut ok_cancel = 0;
        let mut r = 0;
        while r < JUDGE_ROUNDS {
            let h = self.open();
            // Idle→Dragging。
            if self.feed(h, EV_START)? == ST_DRAGGING {
                // Dragging→Dropped 或 Cancelled（各半）。
                if r % 2 == 0 {
                    if self.feed(h, EV_DROP)? == ST_DROPPED {
                        self.set_effect(h, EFF_COPY)?;
                        ok_drop += 1;
                    }
                } else {
                    if self.feed(h, EV_CANCEL)? == ST_CANCELLED {
                        ok_cancel += 1;
                    }
                }
            }
            r += 1;
        }
        self.converge()?;
        Ok((ok_drop, ok_cancel))
    }
}

// ── 确定性 LCG（种子 = SEG_BASE，写死） ──────────────────────────
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
        let m = if n == 0 { 1 } else { n };
        self.next() % m
    }
}

// ── 黄金正演（判据常量独立写死） ─────────────────────────────────
pub const GOLDEN_DROPS: u32 = 5; // 10 轮偶数半
pub const GOLDEN_CANCELS: u32 = 5; // 10 轮奇数半
pub const GOLDEN_ILLEGAL_CAUGHT: u32 = 10;

pub fn golden_run() -> Result<(u32, u32, u32), u32> {
    let mut led = DragLedger::new();
    let (d, c) = led.judge_run()?;
    if d != GOLDEN_DROPS || c != GOLDEN_CANCELS {
        return Err(DragCode::SessionConservationBroken.val());
    }
    led.converge()?;
    // 非法迁移注入 10/10：取消态直接放落必拒。
    let mut caught = 0;
    let mut r = 0;
    while r < JUDGE_ROUNDS {
        let mut l2 = DragLedger::new();
        let h = l2.open();
        let _ = l2.feed(h, EV_START);
        let _ = l2.feed(h, EV_CANCEL);
        // 取消态（终态）收 Drop → 必拒。
        if let Err(e) = l2.feed(h, EV_DROP) {
            if e == DragCode::EventInFinal.val() {
                caught += 1;
            }
        }
        r += 1;
    }
    if caught != GOLDEN_ILLEGAL_CAUGHT {
        return Err(DragCode::TransitionLeak.val());
    }
    Ok((d, c, caught))
}

// ── 注入面：三族 10/10 全抓 ──────────────────────────────────────
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
    // 族 0：取消态直接放落 ×10 必拒。
    ledger.tries[0] += JUDGE_ROUNDS;
    let mut r = 0;
    while r < JUDGE_ROUNDS {
        if let Err(e) = transition(ST_CANCELLED, EV_DROP) {
            if e == DragCode::EventInFinal.val() {
                ledger.caught[0] += 1;
            }
        }
        r += 1;
    }
    // 族 1：空闲收放落 ×10 必拒。
    ledger.tries[1] += JUDGE_ROUNDS;
    let mut r = 0;
    while r < JUDGE_ROUNDS {
        if let Err(e) = transition(ST_IDLE, EV_DROP) {
            if e == DragCode::DropInIdle.val() {
                ledger.caught[1] += 1;
            }
        }
        r += 1;
    }
    // 族 2：双起拖 ×10 必拒。
    ledger.tries[2] += JUDGE_ROUNDS;
    let mut r = 0;
    while r < JUDGE_ROUNDS {
        if let Err(e) = transition(ST_DRAGGING, EV_START) {
            if e == DragCode::DoubleStart.val() {
                ledger.caught[2] += 1;
            }
        }
        r += 1;
    }
}

// ── 反向判据：合法四态迁移必须全过 ───────────────────────────────
pub fn reverse_probe() -> Result<(), u32> {
    if transition(ST_IDLE, EV_START)? != ST_DRAGGING {
        return Err(DragCode::IllegalTransition.val());
    }
    if transition(ST_DRAGGING, EV_CANCEL)? != ST_CANCELLED {
        return Err(DragCode::IllegalTransition.val());
    }
    if transition(ST_DRAGGING, EV_DROP)? != ST_DROPPED {
        return Err(DragCode::IllegalTransition.val());
    }
    // 效果闭集反向。
    if eff_legal(EFF_COPY) || eff_legal(3) {
        return Err(DragCode::EffectOutOfRange.val());
    }
    Ok(())
}

// ── 段位守卫 ─────────────────────────────────────────────────────
pub fn segment_guard() -> bool {
    let mut i = 0;
    while i < DragCode::ALL.len() {
        let v = DragCode::ALL[i].val();
        if v < SEG_BASE || v >= SEG_BASE + 0x40 {
            return false;
        }
        i += 1;
    }
    true
}

// ── 边表守恒点验：4×3=12 格中恰 3 格合法 ─────────────────────────
pub fn trans_table_audit() -> Result<(usize, usize), u32> {
    let mut legal = 0;
    let mut denied = 0;
    let mut s = 0;
    while s < ST_COUNT {
        let mut e = 0;
        while e < EV_COUNT {
            if TRANS_TABLE[s][e] == TRANS_DENY {
                denied += 1;
            } else {
                legal += 1;
            }
            e += 1;
        }
        s += 1;
    }
    if legal + denied != ST_COUNT * EV_COUNT {
        return Err(DragCode::StateOutOfRange.val());
    }
    Ok((legal, denied))
}

// ── 附加点验（≥6 条） ────────────────────────────────────────────
pub fn extra_checks() -> Vec<(&'static str, bool, String)> {
    let mut v: Vec<(&'static str, bool, String)> = Vec::new();
    // 1. 黄金正演：四态迁移 10/10 + 非法迁移 10/10 检出。
    let g = golden_run();
    v.push((
        "J1-四态迁移10/10+非法注入10/10检出",
        matches!(g, Ok((5, 5, 10))),
        format!("{:?}", g),
    ));
    // 2. 注入三族全抓。
    let mut led = InjectLedger::new();
    run_injections(&mut led);
    v.push((
        "J1-注入三族全抓",
        led.all_caught(),
        format!("t/c {:?}/{:?}", led.tries, led.caught),
    ));
    // 3. 边表守恒：12 格恰 3 合法 9 拒。
    let audit = trans_table_audit();
    v.push((
        "J1-边表守恒3合法9拒",
        matches!(audit, Ok((3, 9))),
        format!("{:?}", audit),
    ));
    // 4. 状态/事件名点验。
    let names_ok = ST_NAMES[0].len() > 0 && EV_NAMES[2].len() > 0 && ST_COUNT == 4;
    v.push(("J1-四态三事件名点验", names_ok, format!("{}态/{}事件", ST_COUNT, EV_COUNT)));
    // 5. 反向判据：合法迁移必须全过（防判据恒真）。
    v.push(("J1-反向探针合法迁移", matches!(reverse_probe(), Ok(())), String::from("三合法边+效果闭集")));
    // 6. 会话账守恒 + 反向（人为少记必拒）。
    let mut bad = DragLedger::new();
    let h = bad.open();
    let _ = bad.feed(h, EV_START);
    let _ = bad.feed(h, EV_DROP);
    bad.dropped_cnt = 0; // 人为漏记
    let rev = if let Err(e) = bad.converge() {
        e == DragCode::SessionConservationBroken.val()
    } else {
        false
    };
    v.push(("J1-会话守恒反向", rev, String::from("终态计=会话数")));
    // 7. LCG 确定性 + 反向。
    let mut l1 = Lcg::new(SEG_BASE as u64);
    let mut l2 = Lcg::new(SEG_BASE as u64);
    let a = l1.next();
    let det = a == l2.next();
    let mut l3 = Lcg::new((SEG_BASE as u64) + 0x80);
    let nondet = l3.next() != a;
    v.push(("J1-LCG确定性+反向", det && nondet, format!("seed=0x{:X}", SEG_BASE)));
    // 8. 段位守卫。
    v.push(("J3-段位守卫", segment_guard(), String::from("0x67580 窗口")));
    v
}

// ── 判据入口 ─────────────────────────────────────────────────────
pub fn run_unx15601_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F15601 DoDragDrop 状态机总纲");
    for (n, p, d) in extra_checks() {
        s.add_dyn(n, p, &d);
    }
    s
}
