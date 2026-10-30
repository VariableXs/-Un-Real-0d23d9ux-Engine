//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F15701 · IErrorInfo 面谱 · 0x676C0
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定，CoRun·UNX-D5-B26）：
//!   ① 纯功能代码 ≥300 行；
//!   ② UNX-F15701-J1 面谱成文（GetDescription/GetGUID/GetHelpContext/
//!     GetSource 四核 10/10 次），空错注入（无错时 Get 返伪错对象）
//!     检出 10/10 次；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - 错误对象四元组建模：Description/GUID/HelpContext/Source——
//!     四核语义各自分账，各 10/10 轮；
//!   - 错误通道状态机：Clear（无错）→ Set（已置错）→ Raised（已
//!     抛出）→ Clear（处理毕回环）；无错时取错对象 = 空错注入，
//!     如实检出（通道必须 Clear 态拒发对象）；
//!   - 置错闸：Description 非空 + GUID 在段闭集内才可置错（空错
//!     对象拒入通道）；
//!   - 守恒恒等式：抛出数 = 处理数 + 未处理数；置错数 ≥ 抛出数；
//!   - 空错注入面：Clear 态强取错误对象 / 伪造空错对象入账 →
//!     10/10 全抓；
//!   - LCG 种子写死 = SEG_BASE（0x676C0）；no_std + alloc；
//!     零 unwrap/expect/panic；禁 rand/浮点/IO/墙钟。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x676C0，AI-VX3-1030 批13 独占区）。
pub const SEG_BASE: u32 = 0x676C0;

// ── 四核闭集 ─────────────────────────────────────────────────────
pub const CORE_GET_DESCRIPTION: usize = 0;
pub const CORE_GET_GUID: usize = 1;
pub const CORE_GET_HELP_CONTEXT: usize = 2;
pub const CORE_GET_SOURCE: usize = 3;
pub const CORE_COUNT: usize = 4;
pub const CORE_NAMES: [&str; CORE_COUNT] =
    ["GetDescription", "GetGUID", "GetHelpContext", "GetSource"];

/// 错误 GUID 池（写死：段内派生四类错误源）。
pub const GUID_POOL: usize = 4;

/// 判据迭代轮数（写死：判据原文 10/10 次）。
pub const JUDGE_ROUNDS: u32 = 10;

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ErrCode {
    /// 四核越闭集
    CoreOutOfRange,
    /// GUID 越池
    GuidOutOfRange,
    /// 空错注入：无错时取伪错对象（如实检出）
    EmptyErrorFetch,
    /// 空错对象：Description 空的伪错对象入账
    BlankDescription,
    /// 通道态非法迁移（Clear 态抛出）
    IllegalChannelState,
    /// 未置错即抛（Raise 前提）
    RaiseBeforeSet,
    /// 通道守恒破坏（抛出≠处理+未处理）
    ChannelConservationBroken,
    /// 双置错覆盖未记（账面丢失）
    SetOverwriteUnbooked,
    /// 空错注入漏抓
    EmptyLeak,
    /// 双跑不一致
    NotReproducible,
}

impl ErrCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            ErrCode::CoreOutOfRange => "四核越闭集：Get 四法冻结",
            ErrCode::GuidOutOfRange => "GUID 越池：段内四源冻结",
            ErrCode::EmptyErrorFetch => "空错注入：无错时取伪错对象，如实检出",
            ErrCode::BlankDescription => "空错对象：Description 空拒入账",
            ErrCode::IllegalChannelState => "通道态非法迁移：Clear 态抛出",
            ErrCode::RaiseBeforeSet => "未置错即抛：Raise 前提缺失",
            ErrCode::ChannelConservationBroken => "通道守恒破坏：抛出≠处理+未处理",
            ErrCode::SetOverwriteUnbooked => "双置错覆盖未记：账面丢失",
            ErrCode::EmptyLeak => "空错注入漏抓：检出 10/10 红线",
            ErrCode::NotReproducible => "双跑不一致：复现红线",
        }
    }
    pub const ALL: [ErrCode; 10] = [
        ErrCode::CoreOutOfRange,
        ErrCode::GuidOutOfRange,
        ErrCode::EmptyErrorFetch,
        ErrCode::BlankDescription,
        ErrCode::IllegalChannelState,
        ErrCode::RaiseBeforeSet,
        ErrCode::ChannelConservationBroken,
        ErrCode::SetOverwriteUnbooked,
        ErrCode::EmptyLeak,
        ErrCode::NotReproducible,
    ];
}

pub fn detail(code: u32) -> &'static str {
    let mut i = 0;
    while i < ErrCode::ALL.len() {
        if ErrCode::ALL[i].val() == code {
            return ErrCode::ALL[i].detail();
        }
        i += 1;
    }
    "未知诊断码：不在 0x676C0 段闭集内"
}

// ── 错误通道状态机 ───────────────────────────────────────────────
/// 三态闭集：Clear（无错）→ Set（已置错）→ Raised（已抛）→ Clear。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ChannelState {
    Clear,
    Set,
    Raised,
}

pub fn channel_can_move(from: ChannelState, to: ChannelState) -> bool {
    match (from, to) {
        (ChannelState::Clear, ChannelState::Set) => true,
        (ChannelState::Set, ChannelState::Raised) => true,
        (ChannelState::Raised, ChannelState::Clear) => true,
        (ChannelState::Set, ChannelState::Clear) => true, // 未抛先清
        _ => false,
    }
}

/// 错误对象四元组。
#[derive(Clone, Debug)]
pub struct ErrObj {
    pub description: String,
    pub guid: usize,
    pub help_context: u32,
    pub source: String,
}

impl ErrObj {
    /// 置错闸：Description 非空 + GUID 在池内才合法。
    pub fn legal(&self) -> Result<(), u32> {
        if self.description.len() == 0 {
            return Err(ErrCode::BlankDescription.val());
        }
        if self.guid >= GUID_POOL {
            return Err(ErrCode::GuidOutOfRange.val());
        }
        Ok(())
    }
}

// ── 错误通道账（IErrorInfo 建模） ────────────────────────────────
pub struct ErrorChannel {
    pub state: ChannelState,
    pub current: Option<ErrObj>,
    pub set_cnt: u32,
    pub raise_cnt: u32,
    pub handled_cnt: u32,
    pub unhandled_cnt: u32,
    pub cleared_cnt: u32,
}

impl ErrorChannel {
    pub fn new() -> ErrorChannel {
        ErrorChannel {
            state: ChannelState::Clear,
            current: None,
            set_cnt: 0,
            raise_cnt: 0,
            handled_cnt: 0,
            unhandled_cnt: 0,
            cleared_cnt: 0,
        }
    }

    fn step(&mut self, to: ChannelState) -> Result<(), u32> {
        if !channel_can_move(self.state, to) {
            return Err(ErrCode::IllegalChannelState.val());
        }
        self.state = to;
        Ok(())
    }

    /// SetError：置错（空错对象拒入；Set 态再置=覆盖，须落账）。
    pub fn set_error(&mut self, e: ErrObj) -> Result<(), u32> {
        e.legal()?;
        if self.state == ChannelState::Set {
            // 覆盖：旧对象算未处理落账。
            self.unhandled_cnt += 1;
            self.set_cnt = self.set_cnt; // 记账守恒见 conserve
        }
        self.current = Some(e);
        if self.state == ChannelState::Clear {
            self.step(ChannelState::Set)?;
        }
        self.set_cnt += 1;
        Ok(())
    }

    /// Raise：已置错才可抛。
    pub fn raise(&mut self) -> Result<(), u32> {
        if self.state == ChannelState::Clear {
            // 无错即抛 = 空错注入（抛出通道拒）。
            return Err(ErrCode::RaiseBeforeSet.val());
        }
        if self.current.is_none() {
            return Err(ErrCode::EmptyErrorFetch.val());
        }
        self.step(ChannelState::Raised)?;
        self.raise_cnt += 1;
        Ok(())
    }

    /// 取错误对象（四核入口）：仅 Raised/Set 态有对象可取。
    pub fn fetch(&mut self) -> Result<ErrObj, u32> {
        match self.state {
            ChannelState::Clear => Err(ErrCode::EmptyErrorFetch.val()),
            _ => match &self.current {
                Some(e) => Ok(e.clone()),
                None => Err(ErrCode::EmptyErrorFetch.val()),
            },
        }
    }

    /// 处理毕回清。
    pub fn clear(&mut self, handled: bool) -> Result<(), u32> {
        if self.state == ChannelState::Clear {
            return Err(ErrCode::IllegalChannelState.val());
        }
        if handled {
            self.handled_cnt += 1;
        } else {
            self.unhandled_cnt += 1;
        }
        self.current = None;
        self.step(ChannelState::Clear)?;
        self.cleared_cnt += 1;
        Ok(())
    }

    /// 四核取用：描述/GUID/帮助上下文/来源 四读。
    pub fn read_four(&mut self) -> Result<(String, usize, u32, String), u32> {
        let e = self.fetch()?;
        if e.description.len() == 0 {
            return Err(ErrCode::BlankDescription.val());
        }
        if e.guid >= GUID_POOL {
            return Err(ErrCode::GuidOutOfRange.val());
        }
        Ok((e.description, e.guid, e.help_context, e.source))
    }

    /// 守恒：抛出数 = 处理数 + 未处理数；置错数 ≥ 抛出数。
    pub fn conserve(&self) -> Result<(), u32> {
        if self.raise_cnt != self.handled_cnt + self.unhandled_cnt {
            return Err(ErrCode::ChannelConservationBroken.val());
        }
        if self.set_cnt < self.raise_cnt {
            return Err(ErrCode::ChannelConservationBroken.val());
        }
        Ok(())
    }

    /// 四核判据正演：10/10 轮。
    pub fn judge_run(&mut self) -> Result<(u32, u32, u32, u32), u32> {
        let mut ok = [0u32; CORE_COUNT];
        let mut r = 0;
        while r < JUDGE_ROUNDS {
            let guid = (r as usize) % GUID_POOL;
            let e = ErrObj {
                description: format!("err#{}", r),
                guid,
                help_context: 100 + r as u32,
                source: format!("src{}", guid),
            };
            self.set_error(e)?;
            // 四核四读。
            let (d, g, hc, s) = self.read_four()?;
            if d.len() > 0 {
                ok[CORE_GET_DESCRIPTION] += 1;
            }
            if g == guid {
                ok[CORE_GET_GUID] += 1;
            }
            if hc == 100 + r as u32 {
                ok[CORE_GET_HELP_CONTEXT] += 1;
            }
            if s.len() > 0 {
                ok[CORE_GET_SOURCE] += 1;
            }
            // 抛出→处理→回清。
            self.raise()?;
            self.clear(true)?;
            r += 1;
        }
        self.conserve()?;
        Ok((ok[0], ok[1], ok[2], ok[3]))
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
pub const GOLDEN_CORE_OK: u32 = 10;
pub const GOLDEN_EMPTY_CAUGHT: u32 = 10;

pub fn golden_run() -> Result<(u32, u32), u32> {
    let mut ch = ErrorChannel::new();
    let (a, b, c, d) = ch.judge_run()?;
    if a != GOLDEN_CORE_OK || b != GOLDEN_CORE_OK || c != GOLDEN_CORE_OK || d != GOLDEN_CORE_OK {
        return Err(ErrCode::ChannelConservationBroken.val());
    }
    // 空错注入 10/10：Clear 态取对象 / 空描述对象入账 必检出。
    let mut caught = 0;
    let mut r = 0;
    while r < JUDGE_ROUNDS {
        let mut c2 = ErrorChannel::new();
        // 型 1：Clear 态取伪错对象。
        if let Err(e) = c2.fetch() {
            if e == ErrCode::EmptyErrorFetch.val() {
                caught += 1;
            }
        }
        // 型 2：空描述对象入账。
        let blank = ErrObj {
            description: String::from(""),
            guid: 0,
            help_context: 0,
            source: String::from("x"),
        };
        if let Err(e) = c2.set_error(blank) {
            if e == ErrCode::BlankDescription.val() {
                caught += 1;
            }
        }
        r += 1;
    }
    if caught != GOLDEN_EMPTY_CAUGHT {
        return Err(ErrCode::EmptyLeak.val());
    }
    Ok((4, caught))
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
    // 族 0：空错注入（Clear 态取对象）×10 必检出。
    ledger.tries[0] += JUDGE_ROUNDS;
    let mut r = 0;
    while r < JUDGE_ROUNDS {
        let mut c = ErrorChannel::new();
        if let Err(e) = c.fetch() {
            if e == ErrCode::EmptyErrorFetch.val() {
                ledger.caught[0] += 1;
            }
        }
        r += 1;
    }
    // 族 1：未置错即抛注入 ×10 必拒。
    ledger.tries[1] += JUDGE_ROUNDS;
    let mut r = 0;
    while r < JUDGE_ROUNDS {
        let mut c = ErrorChannel::new();
        if let Err(e) = c.raise() {
            if e == ErrCode::RaiseBeforeSet.val() {
                ledger.caught[1] += 1;
            }
        }
        r += 1;
    }
    // 族 2：GUID 越池注入 ×10 必拒。
    ledger.tries[2] += JUDGE_ROUNDS;
    let mut r = 0;
    while r < JUDGE_ROUNDS {
        let mut c = ErrorChannel::new();
        let e = ErrObj {
            description: format!("bad{}", r),
            guid: GUID_POOL + (r as usize),
            help_context: 0,
            source: String::from("s"),
        };
        if let Err(e2) = c.set_error(e) {
            if e2 == ErrCode::GuidOutOfRange.val() {
                ledger.caught[2] += 1;
            }
        }
        r += 1;
    }
}

// ── 反向判据：合法四核通道必须全过 ───────────────────────────────
pub fn reverse_probe() -> Result<(), u32> {
    let mut ch = ErrorChannel::new();
    let e = ErrObj {
        description: String::from("legal-error"),
        guid: 1,
        help_context: 7,
        source: String::from("probe"),
    };
    ch.set_error(e)?;
    let (d, g, _hc, s) = ch.read_four()?;
    if d.len() == 0 || s.len() == 0 || g != 1 {
        return Err(ErrCode::BlankDescription.val());
    }
    ch.raise()?;
    ch.clear(true)?;
    ch.conserve()?;
    Ok(())
}

// ── 段位守卫 ─────────────────────────────────────────────────────
pub fn segment_guard() -> bool {
    let mut i = 0;
    while i < ErrCode::ALL.len() {
        let v = ErrCode::ALL[i].val();
        if v < SEG_BASE || v >= SEG_BASE + 0x40 {
            return false;
        }
        i += 1;
    }
    true
}

// ── 附加点验（≥6 条） ────────────────────────────────────────────
pub fn extra_checks() -> Vec<(&'static str, bool, String)> {
    let mut v: Vec<(&'static str, bool, String)> = Vec::new();
    // 1. 黄金正演：四核 10/10 + 空错注入 10/10 检出。
    let g = golden_run();
    v.push((
        "J1-四核10/10+空错注入10/10检出",
        matches!(g, Ok((4, 10))),
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
    // 3. 通道边闭集：Clear→Raised 跳步恒非法（未置错即抛）。
    let closed = !channel_can_move(ChannelState::Clear, ChannelState::Raised)
        && channel_can_move(ChannelState::Raised, ChannelState::Clear);
    v.push(("J1-通道边闭集", closed, String::from("C→S→R→C 闭集")));
    // 4. 守恒反向：人为失衡必拒。
    let mut bad = ErrorChannel::new();
    bad.raise_cnt = 3;
    bad.handled_cnt = 1;
    bad.unhandled_cnt = 0;
    let rev = if let Err(e) = bad.conserve() {
        e == ErrCode::ChannelConservationBroken.val()
    } else {
        false
    };
    v.push(("J1-通道守恒反向", rev, String::from("抛出=处理+未处理")));
    // 5. 反向判据：合法通道必须全过。
    v.push(("J1-反向探针合法通道", matches!(reverse_probe(), Ok(())), String::from("置/抛/清全过")));
    // 6. 四核名点验。
    let names_ok = {
        let mut ok = true;
        let mut i = 0;
        while i < CORE_COUNT {
            if CORE_NAMES[i].len() == 0 {
                ok = false;
            }
            i += 1;
        }
        ok
    };
    v.push(("J1-四核名点验", names_ok, format!("{} 核", CORE_COUNT)));
    // 7. LCG 确定性 + 反向。
    let mut l1 = Lcg::new(SEG_BASE as u64);
    let mut l2 = Lcg::new(SEG_BASE as u64);
    let a = l1.next();
    let det = a == l2.next();
    let mut l3 = Lcg::new((SEG_BASE as u64) + 0xC1);
    let nondet = l3.next() != a;
    v.push(("J1-LCG确定性+反向", det && nondet, format!("seed=0x{:X}", SEG_BASE)));
    // 8. 段位守卫。
    v.push(("J3-段位守卫", segment_guard(), String::from("0x676C0 窗口")));
    v
}

// ── 判据入口 ─────────────────────────────────────────────────────
pub fn run_unx15701_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F15701 IErrorInfo 面谱");
    for (n, p, d) in extra_checks() {
        s.add_dyn(n, p, &d);
    }
    s
}
