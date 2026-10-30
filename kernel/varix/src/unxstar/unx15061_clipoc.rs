//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F15061 · OPEN/CLOSE/EMPTY 序（剪贴板开闭清 5 元组） · 0x66EC0
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定）：
//!   ① 纯功能代码 ≥270 行；
//!   ② UNX-F15061-J1 剪贴板开闭清 5 元组对照一致；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - 三态机：Closed（闭，无主持有人）/ Open（开，持有人在场）/
//!     OpenEmptied（开且已清）；
//!   - 事件闭集：Open / Close / Empty / Peek；规则写死——
//!      开时已开=拒（所有权独占）；闭时已闭=拒；空时已闭=拒
//!      （未开不可清）；Peek 不翻态只读；
//!   - 5 元组 =（开计数，闭计数，清计数，闭态误开拒数，闭态误清拒数）；
//!   - 黄金对照：规范序 Open→Empty→Close 后 5 元组 =（1,1,1,0,0），
//!     守恒恒等式：开计数==闭计数（会话闭合）且末态 Closed；
//!   - 反向判据：闭态误清注入 3 次后拒计数恰 3（与黄金零拒互为反向）；
//!   - LCG 种子写死 = SEG_BASE（0x66EC0），禁 rand/浮点/IO/墙钟；
//!     no_std + alloc；零 unwrap/expect/panic。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x66EC0，AI-VX3-1030 独占区）。
pub const SEG_BASE: u32 = 0x66EC0;

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ClpCode {
    /// 已开再开（所有权独占破）
    OpenWhenOpen,
    /// 已闭再闭（无配对开）
    CloseWhenClosed,
    /// 未开清空（闭态误清）
    EmptyWhenClosed,
    /// 5 元组与黄金不一致
    TupleMismatch,
    /// 会话悬挂（末态非 Closed）
    SessionHung,
    /// 开闭不守恒（开数≠闭数）
    OpenCloseUnbalanced,
    /// 双跑不一致（不可复现）
    NotReproducible,
    /// 事件越闭集
    EventOutOfRange,
    /// 闭后读面（Close 后 Peek 内容应恒空）
    PeekAfterCloseBad,
    /// 会话未入账
    SessionUnbooked,
}

impl ClpCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            ClpCode::OpenWhenOpen => "已开再开：所有权独占，拒收",
            ClpCode::CloseWhenClosed => "已闭再闭：无配对开，拒收",
            ClpCode::EmptyWhenClosed => "未开清空：闭态误清，拒收",
            ClpCode::TupleMismatch => "5 元组与黄金对照不一致",
            ClpCode::SessionHung => "会话悬挂：末态非 Closed",
            ClpCode::OpenCloseUnbalanced => "开闭不守恒：开数≠闭数",
            ClpCode::NotReproducible => "双跑不一致：复现红线",
            ClpCode::EventOutOfRange => "事件越闭集",
            ClpCode::PeekAfterCloseBad => "闭后读面：Close 后内容应恒空",
            ClpCode::SessionUnbooked => "会话未入账",
        }
    }
    pub const ALL: [ClpCode; 10] = [
        ClpCode::OpenWhenOpen,
        ClpCode::CloseWhenClosed,
        ClpCode::EmptyWhenClosed,
        ClpCode::TupleMismatch,
        ClpCode::SessionHung,
        ClpCode::OpenCloseUnbalanced,
        ClpCode::NotReproducible,
        ClpCode::EventOutOfRange,
        ClpCode::PeekAfterCloseBad,
        ClpCode::SessionUnbooked,
    ];
}

pub fn detail(code: u32) -> &'static str {
    let mut i = 0;
    while i < ClpCode::ALL.len() {
        if ClpCode::ALL[i].val() == code {
            return ClpCode::ALL[i].detail();
        }
        i += 1;
    }
    "未知诊断码：不在 0x66EC0 段闭集内"
}

// ── 三态机 ───────────────────────────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ClpState {
    Closed,
    Open,
    OpenEmptied,
}

impl ClpState {
    pub fn is_open(self) -> bool {
        self == ClpState::Open || self == ClpState::OpenEmptied
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ClpEv {
    Open,
    Close,
    Empty,
    Peek,
}

/// 一步转移（写死边闭集）：
///   Closed+Open→Open；Open/OpenEmptied+Close→Closed；
///   Open+Empty→OpenEmptied；OpenEmptied+Empty→OpenEmptied（幂等清）；
///   Peek 任意开态不翻态；其余全拒。
pub fn step(state: ClpState, ev: ClpEv) -> Result<ClpState, u32> {
    match (state, ev) {
        (ClpState::Closed, ClpEv::Open) => Ok(ClpState::Open),
        (ClpState::Open, ClpEv::Close) => Ok(ClpState::Closed),
        (ClpState::OpenEmptied, ClpEv::Close) => Ok(ClpState::Closed),
        (ClpState::Open, ClpEv::Empty) => Ok(ClpState::OpenEmptied),
        (ClpState::OpenEmptied, ClpEv::Empty) => Ok(ClpState::OpenEmptied),
        (ClpState::Open, ClpEv::Peek) => Ok(ClpState::Open),
        (ClpState::OpenEmptied, ClpEv::Peek) => Ok(ClpState::OpenEmptied),
        (ClpState::Open, ClpEv::Open) => Err(ClpCode::OpenWhenOpen.val()),
        (ClpState::OpenEmptied, ClpEv::Open) => Err(ClpCode::OpenWhenOpen.val()),
        (ClpState::Closed, ClpEv::Close) => Err(ClpCode::CloseWhenClosed.val()),
        (ClpState::Closed, ClpEv::Empty) => Err(ClpCode::EmptyWhenClosed.val()),
        (ClpState::Closed, ClpEv::Peek) => Err(ClpCode::PeekAfterCloseBad.val()),
    }
}

// ── 5 元组 ───────────────────────────────────────────────────────
/// 5 元组 =（开数，闭数，清数，闭态误开拒数，闭态误清拒数）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ClpTuple {
    pub opens: u32,
    pub closes: u32,
    pub empties: u32,
    pub open_rejects: u32,
    pub empty_rejects: u32,
}

/// 黄金 5 元组（写死）：规范序 Open→Empty→Close。
pub const GOLDEN_CLP: ClpTuple =
    ClpTuple { opens: 1, closes: 1, empties: 1, open_rejects: 0, empty_rejects: 0 };

/// 反向黄金 5 元组（写死）：闭态误清×3 注入后。
pub const GOLDEN_CLP_REV: ClpTuple =
    ClpTuple { opens: 1, closes: 1, empties: 1, open_rejects: 0, empty_rejects: 3 };

// ── 剪贴板开闭清账本 ─────────────────────────────────────────────
pub struct ClpLedger {
    pub state: ClpState,
    pub tup: ClpTuple,
    /// 清空前暂存的内容项（清空即转出）。
    pub staged: Vec<u32>,
    pub emptied_total: u32,
    pub booked: u32,
}

impl ClpLedger {
    pub fn new() -> ClpLedger {
        ClpLedger {
            state: ClpState::Closed,
            tup: ClpTuple { opens: 0, closes: 0, empties: 0, open_rejects: 0, empty_rejects: 0 },
            staged: Vec::new(),
            emptied_total: 0,
            booked: 0,
        }
    }

    /// 注入事件；拒收事件按类记账，不翻态。
    pub fn feed(&mut self, ev: ClpEv) -> Result<ClpState, u32> {
        match step(self.state, ev) {
            Ok(next) => {
                let prev = self.state;
                self.state = next;
                match ev {
                    ClpEv::Open => self.tup.opens += 1,
                    ClpEv::Close => {
                        self.tup.closes += 1;
                        self.booked += 1;
                        // 闭后暂存区必须为空（已清或本就无物）。
                        self.staged.clear();
                    }
                    ClpEv::Empty => {
                        self.tup.empties += 1;
                        self.emptied_total += self.staged.len() as u32;
                        self.staged.clear();
                    }
                    ClpEv::Peek => {
                        let _ = prev;
                    }
                }
                Ok(next)
            }
            Err(e) => {
                if e == ClpCode::OpenWhenOpen.val() {
                    self.tup.open_rejects += 1;
                }
                if e == ClpCode::EmptyWhenClosed.val() {
                    self.tup.empty_rejects += 1;
                }
                Err(e)
            }
        }
    }

    /// 暂存一个内容项（仅开态合法）。
    pub fn stage(&mut self, item: u32) -> Result<(), u32> {
        if !self.state.is_open() {
            return Err(ClpCode::PeekAfterCloseBad.val());
        }
        self.staged.push(item);
        Ok(())
    }

    /// 规范序：Open→stage 3 项→Empty→Peek→Close。
    pub fn canonical_run(&mut self) -> Result<(), u32> {
        self.feed(ClpEv::Open)?;
        self.stage(11)?;
        self.stage(22)?;
        self.stage(33)?;
        self.feed(ClpEv::Empty)?;
        self.feed(ClpEv::Peek)?;
        self.feed(ClpEv::Close)?;
        Ok(())
    }

    /// 5 元组对照。
    pub fn tuple_match(&self, g: &ClpTuple) -> bool {
        self.tup == *g
    }

    /// 守恒：开数==闭数；末态 Closed；暂存区空。
    pub fn conserved(&self) -> bool {
        self.tup.opens == self.tup.closes
            && self.state == ClpState::Closed
            && self.staged.is_empty()
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
pub fn golden_run() -> Result<(ClpTuple, u32), u32> {
    let mut led = ClpLedger::new();
    led.canonical_run()?;
    if !led.tuple_match(&GOLDEN_CLP) {
        return Err(ClpCode::TupleMismatch.val());
    }
    if !led.conserved() {
        return Err(ClpCode::OpenCloseUnbalanced.val());
    }
    if led.emptied_total != 3 {
        return Err(ClpCode::TupleMismatch.val());
    }
    if led.booked != 1 {
        return Err(ClpCode::SessionUnbooked.val());
    }
    // 复现：双跑一致。
    let mut led2 = ClpLedger::new();
    led2.canonical_run()?;
    if led2.tup != led.tup || led2.emptied_total != led.emptied_total {
        return Err(ClpCode::NotReproducible.val());
    }
    // 反向判据：闭态误清×3 → 拒计数恰 3。
    let mut rev = ClpLedger::new();
    rev.feed(ClpEv::Open)?;
    rev.feed(ClpEv::Empty)?;
    rev.feed(ClpEv::Close)?;
    let mut k = 0;
    while k < 3 {
        if rev.feed(ClpEv::Empty).is_err() {
            // 拒收即预期，计数在 feed 内已记。
        }
        k += 1;
    }
    if !rev.tuple_match(&GOLDEN_CLP_REV) {
        return Err(ClpCode::TupleMismatch.val());
    }
    Ok((led.tup, led.emptied_total))
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
    // 族 0：已开再开。
    ledger.tries[0] += 1;
    let mut bad0 = false;
    if let Err(e) = step(ClpState::Open, ClpEv::Open) {
        if e == ClpCode::OpenWhenOpen.val() {
            bad0 = true;
        }
    }
    if bad0 {
        ledger.caught[0] += 1;
    }
    // 族 1：已闭再闭。
    ledger.tries[1] += 1;
    let mut bad1 = false;
    if let Err(e) = step(ClpState::Closed, ClpEv::Close) {
        if e == ClpCode::CloseWhenClosed.val() {
            bad1 = true;
        }
    }
    if bad1 {
        ledger.caught[1] += 1;
    }
    // 族 2：闭态 Peek（闭后读面必拒）。
    ledger.tries[2] += 1;
    let mut bad2 = false;
    if let Err(e) = step(ClpState::Closed, ClpEv::Peek) {
        if e == ClpCode::PeekAfterCloseBad.val() {
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
    while i < ClpCode::ALL.len() {
        let v = ClpCode::ALL[i].val();
        if v < SEG_BASE || v >= SEG_BASE + 0x40 {
            return false;
        }
        i += 1;
    }
    true
}

pub fn extra_checks() -> Vec<(&'static str, bool, String)> {
    let mut v: Vec<(&'static str, bool, String)> = Vec::new();
    // 1. 黄金正演：5 元组一致 + 守恒 + 清点 3 + 复现 + 反向判据。
    let golden = golden_run();
    let g_ok = match &golden {
        Ok((t, e3)) => *t == GOLDEN_CLP && *e3 == 3,
        Err(_) => false,
    };
    v.push(("J1-5元组黄金+反向判据", g_ok, format!("{:?}", golden)));
    // 2. 注入三族全抓。
    let mut inj = InjectLedger::new();
    run_injections(&mut inj);
    v.push((
        "J1-注入三族全抓",
        inj.all_caught(),
        format!("t/c {:?}/{:?}", inj.tries, inj.caught),
    ));
    // 3. 暂存面：闭态 stage 必拒；开态 stage 合法。
    let mut st = ClpLedger::new();
    let closed_stage_bad = st.stage(1).is_err();
    st.feed(ClpEv::Open).ok();
    let open_stage_ok = st.stage(7).is_ok();
    v.push((
        "J1-暂存开合法闭必拒",
        closed_stage_bad && open_stage_ok,
        format!("closed_bad={} open_ok={}", closed_stage_bad, open_stage_ok),
    ));
    // 4. 幂等清：OpenEmptied 再 Empty 不翻态不增拒。
    let idem = step(ClpState::OpenEmptied, ClpEv::Empty) == Ok(ClpState::OpenEmptied);
    v.push((
        "J1-幂等清自环",
        idem,
        String::from("OpenEmptied+Empty→OpenEmptied"),
    ));
    // 5. LCG 混合序压力：40 事件随机序后开闭守恒仍成立（只统计全合法
    //    收敛的会话）。
    let mut lcg = Lcg::new(SEG_BASE as u64);
    let mut mixed_ok = 0;
    let mut trial = 0;
    while trial < 8 {
        let mut ml = ClpLedger::new();
        let mut opens = 0;
        let mut target_opens = 1 + lcg.below(3) as u32;
        let mut steps = 0;
        let mut converged = true;
        while opens < target_opens && steps < 64 {
            let pick = lcg.below(2);
            if ml.state == ClpState::Closed {
                if ml.feed(ClpEv::Open).is_err() {
                    converged = false;
                    break;
                }
                opens += 1;
            } else if pick == 0 {
                let _ = ml.feed(ClpEv::Empty);
            } else if ml.feed(ClpEv::Close).is_ok() {
                // 闭合一会话，若还需开会话继续。
            }
            steps += 1;
        }
        // 收尾：若仍开态则闭合。
        if ml.state.is_open() {
            let _ = ml.feed(ClpEv::Close);
        }
        if converged && ml.conserved() {
            mixed_ok += 1;
        }
        trial += 1;
        let _ = target_opens;
        target_opens = 0;
    }
    v.push((
        "J2-8试混合序守恒",
        mixed_ok == 8,
        format!("{}/8", mixed_ok),
    ));
    // 6. 诊断码闭集点验：10 码互异落窗。
    let mut distinct = 0;
    let mut a = 0;
    while a < ClpCode::ALL.len() {
        let va = ClpCode::ALL[a].val();
        let mut uniq = true;
        let mut b = 0;
        while b < ClpCode::ALL.len() {
            if a != b && ClpCode::ALL[b].val() == va {
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
    v.push(("J3-段位守卫", segment_guard(), String::from("0x66EC0 窗口")));
    v
}

pub fn run_unx15061_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F15061 OPEN/CLOSE/EMPTY 序（剪贴板开闭清 5 元组）");
    for (n, p, d) in extra_checks() {
        s.add_dyn(n, p, &d);
    }
    s
}
