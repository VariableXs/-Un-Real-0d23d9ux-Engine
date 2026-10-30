//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F15021 · ENTER/EXITMENULOOP 序（菜单循环进入退出） · 0x66E40
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定）：
//!   ① 纯功能代码 ≥270 行；
//!   ② UNX-F15021-J1 菜单循环进入退出 4 元组对照一致；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - 两态机：Outside（循环外）/ Inside（循环内，携带深度 u32）；
//!   - 事件闭集：Enter / Exit，共 2 事件；Exit 在 Outside 态一律拒收
//!     （失配诊断），Enter 深度越过嵌套上限（写死 8，来源：菜单循环
//!     嵌套安全上限）拒收；
//!   - 4 元组 =（进入计数，退出计数，最大深度，失配拒收数）；
//!   - 黄金对照：规范序列 Enter×3 → Exit×3 后 4 元组 =（3,3,3,0），
//!     进入数==退出数（守恒恒等式一）、末态 Outside（恒等式二）；
//!   - 反向判据：故意注入失配 Exit 一次的对照序列 4 元组 =（1,1,1,1），
//!     失配数必须非零（与黄金零失配互为反向验证）；
//!   - LCG 种子写死 = SEG_BASE（0x66E40），禁 rand/浮点/IO/墙钟；
//!     no_std + alloc；零 unwrap/expect/panic。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x66E40，AI-VX3-1030 独占区）。
pub const SEG_BASE: u32 = 0x66E40;

/// 菜单循环嵌套安全上限（写死，来源：嵌套深递归防护惯例）。
pub const MAX_DEPTH: u32 = 8;

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum MlpCode {
    /// 循环外退出（无配对进入）
    ExitUnmatched,
    /// 嵌套深度越上限
    DepthOverflow,
    /// 深度下溢（内部守卫）
    DepthUnderflow,
    /// 4 元组与黄金不一致
    TupleMismatch,
    /// 末态非 Outside（会话悬挂）
    SessionHung,
    /// 守恒恒等式破（进入数≠退出数）
    EnterExitUnbalanced,
    /// 双跑不一致（不可复现）
    NotReproducible,
    /// 事件越闭集
    EventOutOfRange,
    /// 反向判据失败（失配注入未被记账）
    ReverseProbeFailed,
    /// 会话未入账
    SessionUnbooked,
}

impl MlpCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            MlpCode::ExitUnmatched => "循环外退出：无配对进入，拒收",
            MlpCode::DepthOverflow => "嵌套深度越上限 8",
            MlpCode::DepthUnderflow => "深度下溢：内部守卫触发",
            MlpCode::TupleMismatch => "4 元组与黄金对照不一致",
            MlpCode::SessionHung => "末态非 Outside：会话悬挂",
            MlpCode::EnterExitUnbalanced => "守恒破：进入数≠退出数",
            MlpCode::NotReproducible => "双跑不一致：复现红线",
            MlpCode::EventOutOfRange => "事件越闭集",
            MlpCode::ReverseProbeFailed => "反向判据失败：失配未记账",
            MlpCode::SessionUnbooked => "会话未入账",
        }
    }
    pub const ALL: [MlpCode; 10] = [
        MlpCode::ExitUnmatched,
        MlpCode::DepthOverflow,
        MlpCode::DepthUnderflow,
        MlpCode::TupleMismatch,
        MlpCode::SessionHung,
        MlpCode::EnterExitUnbalanced,
        MlpCode::NotReproducible,
        MlpCode::EventOutOfRange,
        MlpCode::ReverseProbeFailed,
        MlpCode::SessionUnbooked,
    ];
}

pub fn detail(code: u32) -> &'static str {
    let mut i = 0;
    while i < MlpCode::ALL.len() {
        if MlpCode::ALL[i].val() == code {
            return MlpCode::ALL[i].detail();
        }
        i += 1;
    }
    "未知诊断码：不在 0x66E40 段闭集内"
}

// ── 状态机 ───────────────────────────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum LoopState {
    Outside,
    Inside(u32),
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum MlpEv {
    Enter,
    Exit,
}

/// 一步转移：Outside+Exit 拒收；深度越上限拒收。
pub fn step(state: LoopState, ev: MlpEv) -> Result<LoopState, u32> {
    match (state, ev) {
        (LoopState::Outside, MlpEv::Enter) => Ok(LoopState::Inside(1)),
        (LoopState::Inside(d), MlpEv::Enter) => {
            if d >= MAX_DEPTH {
                Err(MlpCode::DepthOverflow.val())
            } else {
                Ok(LoopState::Inside(d + 1))
            }
        }
        (LoopState::Inside(d), MlpEv::Exit) => {
            if d <= 1 {
                Ok(LoopState::Outside)
            } else {
                Ok(LoopState::Inside(d - 1))
            }
        }
        (LoopState::Outside, MlpEv::Exit) => Err(MlpCode::ExitUnmatched.val()),
    }
}

// ── 4 元组 ───────────────────────────────────────────────────────
/// 4 元组 =（进入数，退出数，最大深度，失配拒收数）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct MlpTuple {
    pub enters: u32,
    pub exits: u32,
    pub max_depth: u32,
    pub mismatches: u32,
}

impl MlpTuple {
    pub const fn zero() -> MlpTuple {
        MlpTuple { enters: 0, exits: 0, max_depth: 0, mismatches: 0 }
    }
}

/// 黄金 4 元组（写死）：规范序列 Enter×3→Exit×3。
pub const GOLDEN_MLP: MlpTuple = MlpTuple { enters: 3, exits: 3, max_depth: 3, mismatches: 0 };

/// 反向黄金 4 元组（写死）：Enter→Exit→Exit（末次失配）。
pub const GOLDEN_MLP_REV: MlpTuple =
    MlpTuple { enters: 1, exits: 1, max_depth: 1, mismatches: 1 };

// ── 菜单循环保守账本 ─────────────────────────────────────────────
pub struct MlpLedger {
    pub state: LoopState,
    pub tup: MlpTuple,
    pub booked: u32,
    pub cur_depth: u32,
}

impl MlpLedger {
    pub fn new() -> MlpLedger {
        MlpLedger {
            state: LoopState::Outside,
            tup: MlpTuple::zero(),
            booked: 0,
            cur_depth: 0,
        }
    }

    /// 注入一个事件；合法事件记账，拒收只记失配不翻态。
    pub fn feed(&mut self, ev: MlpEv) -> Result<LoopState, u32> {
        match step(self.state, ev) {
            Ok(next) => {
                self.state = next;
                match ev {
                    MlpEv::Enter => {
                        self.tup.enters += 1;
                        self.cur_depth += 1;
                        if self.cur_depth > self.tup.max_depth {
                            self.tup.max_depth = self.cur_depth;
                        }
                    }
                    MlpEv::Exit => {
                        self.tup.exits += 1;
                        if self.cur_depth >= 1 {
                            self.cur_depth -= 1;
                        } else {
                            return Err(MlpCode::DepthUnderflow.val());
                        }
                    }
                }
                if next == LoopState::Outside {
                    self.booked += 1; // 一次完整进出会话入账
                }
                Ok(next)
            }
            Err(e) => {
                if e == MlpCode::ExitUnmatched.val() {
                    self.tup.mismatches += 1;
                }
                Err(e)
            }
        }
    }

    /// 规范序列：Enter×3 → Exit×3。
    pub fn canonical_run(&mut self) -> Result<(), u32> {
        let mut i = 0;
        while i < 3 {
            self.feed(MlpEv::Enter)?;
            i += 1;
        }
        let mut j = 0;
        while j < 3 {
            self.feed(MlpEv::Exit)?;
            j += 1;
        }
        Ok(())
    }

    /// 反向探针序列：Enter → Exit → Exit（第三次失配）。
    pub fn reverse_run(&mut self) -> Result<u32, u32> {
        self.feed(MlpEv::Enter)?;
        self.feed(MlpEv::Exit)?;
        let mut misses = 0;
        if self.feed(MlpEv::Exit).is_err() {
            misses += 1;
        }
        Ok(misses)
    }

    /// 4 元组对照。
    pub fn tuple_match(&self, g: &MlpTuple) -> bool {
        self.tup == *g
    }

    /// 守恒一：进入数==退出数；守恒二：末态 Outside；守恒三：当前深度 0。
    pub fn conserved(&self) -> bool {
        self.tup.enters == self.tup.exits
            && self.state == LoopState::Outside
            && self.cur_depth == 0
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
pub fn golden_run() -> Result<(MlpTuple, MlpTuple), u32> {
    let mut led = MlpLedger::new();
    led.canonical_run()?;
    if !led.tuple_match(&GOLDEN_MLP) {
        return Err(MlpCode::TupleMismatch.val());
    }
    if !led.conserved() {
        return Err(MlpCode::EnterExitUnbalanced.val());
    }
    if led.booked != 1 {
        return Err(MlpCode::SessionUnbooked.val());
    }
    // 复现：双跑一致。
    let mut led2 = MlpLedger::new();
    led2.canonical_run()?;
    if led2.tup != led.tup {
        return Err(MlpCode::NotReproducible.val());
    }
    // 反向判据：失配注入必须记账为 1 次失配。
    let mut rev = MlpLedger::new();
    let misses = rev.reverse_run()?;
    if misses != 1 || !rev.tuple_match(&GOLDEN_MLP_REV) {
        return Err(MlpCode::ReverseProbeFailed.val());
    }
    Ok((led.tup, rev.tup))
}

// ── 深度压力正演：满嵌套 8 层进出（确定性，无随机） ─────────────
pub fn depth_stress() -> Result<u32, u32> {
    let mut led = MlpLedger::new();
    let mut i = 0;
    while i < MAX_DEPTH {
        led.feed(MlpEv::Enter)?;
        i += 1;
    }
    if led.cur_depth != MAX_DEPTH {
        return Err(MlpCode::DepthOverflow.val());
    }
    // 第 9 层进入必须拒收。
    if led.feed(MlpEv::Enter).is_ok() {
        return Err(MlpCode::DepthOverflow.val());
    }
    let mut j = 0;
    while j < MAX_DEPTH {
        led.feed(MlpEv::Exit)?;
        j += 1;
    }
    if !led.conserved() {
        return Err(MlpCode::EnterExitUnbalanced.val());
    }
    Ok(led.tup.max_depth)
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
    // 族 0：循环外退出。
    ledger.tries[0] += 1;
    let mut bad0 = false;
    if let Err(e) = step(LoopState::Outside, MlpEv::Exit) {
        if e == MlpCode::ExitUnmatched.val() {
            bad0 = true;
        }
    }
    if bad0 {
        ledger.caught[0] += 1;
    }
    // 族 1：深度越上限（8 层满后第 9 次 Enter）。
    ledger.tries[1] += 1;
    let mut bad1 = false;
    if let Err(e) = step(LoopState::Inside(MAX_DEPTH), MlpEv::Enter) {
        if e == MlpCode::DepthOverflow.val() {
            bad1 = true;
        }
    }
    if bad1 {
        ledger.caught[1] += 1;
    }
    // 族 2：事件越闭集面——空账本直接退出在账本层必拒并记失配。
    ledger.tries[2] += 1;
    let mut led = MlpLedger::new();
    let mut bad2 = false;
    if let Err(e) = led.feed(MlpEv::Exit) {
        if e == MlpCode::ExitUnmatched.val() && led.tup.mismatches == 1 {
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
    while i < MlpCode::ALL.len() {
        let v = MlpCode::ALL[i].val();
        if v < SEG_BASE || v >= SEG_BASE + 0x40 {
            return false;
        }
        i += 1;
    }
    true
}

pub fn extra_checks() -> Vec<(&'static str, bool, String)> {
    let mut v: Vec<(&'static str, bool, String)> = Vec::new();
    // 1. 黄金正演：4 元组一致 + 守恒 + 复现 + 反向判据。
    let golden = golden_run();
    let g_ok = match &golden {
        Ok((g, r)) => *g == GOLDEN_MLP && *r == GOLDEN_MLP_REV,
        Err(_) => false,
    };
    v.push(("J1-4元组黄金+反向判据", g_ok, format!("{:?}", golden)));
    // 2. 深度压力：满 8 层 + 第 9 层拒收 + 守恒。
    let stress = depth_stress();
    v.push((
        "J1-满嵌套8层+第9层拒收",
        stress == Ok(MAX_DEPTH),
        format!("{:?}", stress),
    ));
    // 3. 注入三族全抓。
    let mut inj = InjectLedger::new();
    run_injections(&mut inj);
    v.push((
        "J1-注入三族全抓",
        inj.all_caught(),
        format!("t/c {:?}/{:?}", inj.tries, inj.caught),
    ));
    // 4. 独立重算黄金值（判据常量不复用被测值）：手工重演 3 进 3 出。
    let mut hand = MlpTuple::zero();
    hand.enters = 3;
    hand.exits = 3;
    hand.max_depth = 3;
    hand.mismatches = 0;
    let mut chk = MlpLedger::new();
    let mut ok_run = true;
    let mut i = 0;
    while i < 3 {
        if chk.feed(MlpEv::Enter).is_err() {
            ok_run = false;
        }
        i += 1;
    }
    let mut j = 0;
    while j < 3 {
        if chk.feed(MlpEv::Exit).is_err() {
            ok_run = false;
        }
        j += 1;
    }
    v.push((
        "J1-手工重演对照一致",
        ok_run && chk.tup == hand && hand == GOLDEN_MLP,
        format!("{:?}", chk.tup),
    ));
    // 5. 状态合法边抽验：Inside(1)+Exit→Outside；Inside(3)+Exit→Inside(2)。
    let e1 = step(LoopState::Inside(1), MlpEv::Exit) == Ok(LoopState::Outside);
    let e2 = step(LoopState::Inside(3), MlpEv::Exit) == Ok(LoopState::Inside(2));
    v.push((
        "J1-深度进出边抽验",
        e1 && e2,
        format!("d1→out={} d3→d2={}", e1, e2),
    ));
    // 6. 诊断码闭集点验：10 码互异且落段窗。
    let mut distinct = 0;
    let mut a = 0;
    while a < MlpCode::ALL.len() {
        let va = MlpCode::ALL[a].val();
        let mut uniq = true;
        let mut b = 0;
        while b < MlpCode::ALL.len() {
            if a != b && MlpCode::ALL[b].val() == va {
                uniq = false;
            }
            b += 1;
        }
        if uniq && va >= SEG_BASE && va < SEG_BASE + 0x40 {
            distinct += 1;
        }
        a += 1;
    }
    v.push((
        "J2-诊断码10码闭集互异",
        distinct == 10,
        format!("{}/10", distinct),
    ));
    // 7. 段位守卫。
    v.push(("J3-段位守卫", segment_guard(), String::from("0x66E40 窗口")));
    v
}

pub fn run_unx15021_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F15021 ENTER/EXITMENULOOP 序（菜单循环进出 4 元组）");
    for (n, p, d) in extra_checks() {
        s.add_dyn(n, p, &d);
    }
    s
}
