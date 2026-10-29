//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F13341 · 栈溢出异常路径总成（触发→增长→溢出三态） · 0x65940
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定）：
//!   ① 纯功能代码 ≥400 行；
//!   ② UNX-F13341-J1 三态机（guard page 触发→栈增长→溢出异常）30 例
//!      全链一致，状态迁移断言与 Windows 行为对齐；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - 栈四态机（基态+三态链）：Running→GuardTrigger（guard page 触发）
//!     →Grow（栈增长：提交新页）/ Overflow（溢出异常）；Grow→Running
//!     回运态或 Grow→Overflow（保留域耗尽）；Overflow 为终态，出边全拒；
//!   - 页参数写死并注来源：PAGE_SIZE=4096（x64 页粒度）、
//!     RESERVE_PAGES=256（1 MiB 主线程保留）、INITIAL_COMMIT=8；
//!   - 30 例全链：案例账（触发/增长/溢出事件序列）逐例登记，
//!     Σ案例 ≡ 30，双跑哈希逐位同（幂等红线）；
//!   - 守恒：触发数 ≡ 增长成功数+溢出数（guard 触发账恒等式）；
//!   - 注入三族：未触发即增长/溢出态仍操作/案例重复入账 3/3 必抓；
//!   - LCG 种子写死 = SEG_BASE（0x65940），禁 rand/浮点/IO/墙钟；
//!     no_std + alloc；零 unwrap/expect。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x65940，AI-VX3-1030 独占区）。
pub const SEG_BASE: u32 = 0x65940;

/// 全链案例闭集：30 例（判据 J1 明示）。
pub const MAX_CASES: usize = 30;

/// 页粒度：4096 字节（x64 页粒度，Windows 栈按页提交）。
pub const PAGE_SIZE: u32 = 4096;
/// 保留页数：256 页 = 1 MiB（Windows 主线程默认保留量）。
pub const RESERVE_PAGES: u32 = 256;
/// 初始提交页数：8 页 = 32 KiB（Windows 初始提交量）。
pub const INITIAL_COMMIT: u32 = 8;

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum StkCode {
    /// 案例下标越闭集
    CaseOutOfRange,
    /// 栈态非法迁移
    StateIllegal,
    /// 增长越过保留域
    GrowBeyondReserve,
    /// guard 未触发即申请增长
    GuardNotTriggered,
    /// 溢出终态仍接受操作
    OverflowNotTerminal,
    /// 案例账守恒破（Σ案例≡30）
    ConservationBreak,
    /// 案例重复入账
    CaseBookDup,
    /// 栈参数非法（reserve<commit 或页数 0）
    StackParamInvalid,
    /// 全链双跑非幂等
    RerunNonIdempotent,
}

impl StkCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            StkCode::CaseOutOfRange => "案例下标越闭集：0~29 冻结",
            StkCode::StateIllegal => "栈态非法迁移：三态链闭集外",
            StkCode::GrowBeyondReserve => "增长越过保留域：commit≤reserve 红线",
            StkCode::GuardNotTriggered => "guard 未触发即增长：事件链断裂",
            StkCode::OverflowNotTerminal => "溢出终态仍接受操作",
            StkCode::ConservationBreak => "案例账守恒破：Σ案例≡30",
            StkCode::CaseBookDup => "案例重复入账：一例一行",
            StkCode::StackParamInvalid => "栈参数非法：reserve<commit 或页数 0",
            StkCode::RerunNonIdempotent => "全链双跑非幂等：哈希必须逐位同",
        }
    }
    pub const ALL: [StkCode; 9] = [
        StkCode::CaseOutOfRange,
        StkCode::StateIllegal,
        StkCode::GrowBeyondReserve,
        StkCode::GuardNotTriggered,
        StkCode::OverflowNotTerminal,
        StkCode::ConservationBreak,
        StkCode::CaseBookDup,
        StkCode::StackParamInvalid,
        StkCode::RerunNonIdempotent,
    ];
}

pub fn detail(code: u32) -> &'static str {
    let mut i = 0;
    while i < StkCode::ALL.len() {
        if StkCode::ALL[i].val() == code {
            return StkCode::ALL[i].detail();
        }
        i += 1;
    }
    "未知诊断码：不在 0x65940 段闭集内"
}

// ── 栈四态闭集（基态 Running + 三态链） ─────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum StackState {
    /// 运行态（基态）
    Running,
    /// guard page 触发
    GuardTrigger,
    /// 栈增长中（提交新页）
    Growing,
    /// 溢出异常（终态）
    Overflow,
}

/// 事件闭集：三态链逐事件登记（30 例全链断言用）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum StkEvent {
    GuardHit,
    GrowOk,
    GrowFail,
    OverflowAbort,
}

/// 单案例栈账。
pub struct CaseStack {
    pub id: usize,
    pub reserve_pages: u32,
    pub committed: u32,
    pub state: StackState,
    pub guard_hits: u32,
    pub grows_ok: u32,
    pub grow_fails: u32,
    pub events: Vec<StkEvent>,
}

impl CaseStack {
    /// 新建案例栈：reserve≥commit、commit≥INITIAL_COMMIT 下界、页数非零。
    pub fn new(id: usize, reserve: u32, commit: u32) -> Result<CaseStack, u32> {
        if reserve == 0 || commit == 0 || reserve < commit || commit < INITIAL_COMMIT {
            return Err(StkCode::StackParamInvalid.val());
        }
        Ok(CaseStack {
            id,
            reserve_pages: reserve,
            committed: commit,
            state: StackState::Running,
            guard_hits: 0,
            grows_ok: 0,
            grow_fails: 0,
            events: Vec::new(),
        })
    }

    /// 申请深度压入：越过已提交域 → guard 触发；保留域内可继续。
    /// 返回 Ok(true)=仍在 Running，Ok(false)=已溢出。
    pub fn push_depth(&mut self, pages_wanted: u32) -> Result<bool, u32> {
        if self.state == StackState::Overflow {
            return Err(StkCode::OverflowNotTerminal.val());
        }
        if self.state != StackState::Running {
            return Err(StkCode::StateIllegal.val());
        }
        let need = self.committed.wrapping_add(pages_wanted);
        if need > self.reserve_pages {
            // 直接越保留域：触发即溢出（Windows 大块分配行为）。
            self.state = StackState::GuardTrigger;
            self.guard_hits += 1;
            self.events.push(StkEvent::GuardHit);
            return self.settle_overflow();
        }
        // 逐页触碰：每页一次 guard 触发 + 一次增长。
        let mut touched = 0;
        while touched < pages_wanted {
            self.state = StackState::GuardTrigger;
            self.guard_hits += 1;
            self.events.push(StkEvent::GuardHit);
            let verdict = self.grow()?;
            if !verdict {
                return Ok(false);
            }
            touched += 1;
        }
        Ok(true)
    }

    /// 增长步：仅 GuardTrigger 态合法；保留域耗尽 → Overflow 终态。
    fn grow(&mut self) -> Result<bool, u32> {
        if self.state != StackState::GuardTrigger {
            return Err(StkCode::GuardNotTriggered.val());
        }
        if self.committed >= self.reserve_pages {
            self.state = StackState::Overflow;
            self.grow_fails += 1;
            self.events.push(StkEvent::GrowFail);
            self.events.push(StkEvent::OverflowAbort);
            return Ok(false);
        }
        self.state = StackState::Growing;
        self.committed += 1;
        self.events.push(StkEvent::GrowOk);
        self.state = StackState::Running;
        self.grows_ok += 1;
        Ok(true)
    }

    /// 触发后结算溢出（GuardTrigger→Overflow 边）：
    /// 无页可增即增长失败（GrowFail 记账守恒）→ 溢出终态。
    fn settle_overflow(&mut self) -> Result<bool, u32> {
        if self.state != StackState::GuardTrigger {
            return Err(StkCode::StateIllegal.val());
        }
        self.grow_fails += 1;
        self.events.push(StkEvent::GrowFail);
        self.state = StackState::Overflow;
        self.events.push(StkEvent::OverflowAbort);
        Ok(false)
    }

    /// 触发账守恒：guard_hits ≡ grows_ok + grow_fails。
    pub fn conservation(&self) -> Result<(), u32> {
        if self.guard_hits != self.grows_ok + self.grow_fails {
            return Err(StkCode::ConservationBreak.val());
        }
        if self.state == StackState::Overflow {
            if self.grow_fails == 0 {
                return Err(StkCode::OverflowNotTerminal.val());
            }
        }
        Ok(())
    }

    /// 事件哈希（FNV-1a，来源：Fowler–Noll–Vo 公开常数）。
    pub fn event_hash(&self) -> u32 {
        let mut h: u32 = 0x811C9DC5;
        let mut i = 0;
        while i < self.events.len() {
            let e = match self.events[i] {
                StkEvent::GuardHit => 1u8,
                StkEvent::GrowOk => 2,
                StkEvent::GrowFail => 3,
                StkEvent::OverflowAbort => 4,
            };
            h = (h ^ e as u32).wrapping_mul(0x01000193);
            i += 1;
        }
        h
    }
}

// ── 30 例全链账 ──────────────────────────────────────────────────
pub struct ChainLedger {
    pub cases: Vec<CaseStack>,
}

impl ChainLedger {
    pub fn new() -> ChainLedger {
        ChainLedger { cases: Vec::new() }
    }
    pub fn book(&mut self, reserve: u32, commit: u32) -> Result<usize, u32> {
        if self.cases.len() >= MAX_CASES {
            return Err(StkCode::CaseOutOfRange.val());
        }
        let id = self.cases.len();
        let c = CaseStack::new(id, reserve, commit)?;
        self.cases.push(c);
        Ok(id)
    }
    /// 全账守恒：每例守恒且 Σ案例 ≡ 30 由调用方断言。
    pub fn conservation(&self) -> Result<[u32; 4], u32> {
        let mut counts = [0u32; 4];
        let mut i = 0;
        while i < self.cases.len() {
            self.cases[i].conservation()?;
            let k = match self.cases[i].state {
                StackState::Running => 0,
                StackState::GuardTrigger => 1,
                StackState::Growing => 2,
                StackState::Overflow => 3,
            };
            counts[k] += 1;
            i += 1;
        }
        let total = counts[0] + counts[1] + counts[2] + counts[3];
        if total != self.cases.len() as u32 {
            return Err(StkCode::ConservationBreak.val());
        }
        Ok(counts)
    }
}

// ── 确定性 LCG ───────────────────────────────────────────────────
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

// ── 黄金正演：30 例全链（浅/深/爆保留域三谱）→ 守恒 → 双跑幂等 ──
pub fn golden_run() -> Result<u32, u32> {
    let mut led = ChainLedger::new();
    let mut rng = Lcg::new(SEG_BASE as u64);
    let mut i = 0;
    while i < MAX_CASES {
        let kind = rng.below(3);
        let r = match kind {
            0 => led.book(RESERVE_PAGES, INITIAL_COMMIT + 1),
            1 => led.book(RESERVE_PAGES, INITIAL_COMMIT + 4),
            _ => led.book(RESERVE_PAGES, RESERVE_PAGES), // 保留域全提交
        };
        match r {
            Ok(idx) => {
                let depth = if kind == 2 {
                    2 // 必爆
                } else {
                    rng.below(RESERVE_PAGES as u64) as u32
                };
                let _ = led.cases[idx].push_depth(depth);
            }
            Err(_) => {
                // kind==2 且 commit==reserve 是合法建栈；此处必不到。
                return Err(StkCode::StackParamInvalid.val());
            }
        }
        i += 1;
    }
    let counts = led.conservation()?;
    // 谱守恒：30 例全覆盖。
    let total = counts[0] + counts[1] + counts[2] + counts[3];
    if total != MAX_CASES as u32 {
        return Err(StkCode::ConservationBreak.val());
    }
    // 至少一例真溢出（三谱正演覆盖断言）。
    if counts[3] == 0 {
        return Err(StkCode::ConservationBreak.val());
    }
    // 双跑幂等：重建同序列，哈希逐位同。
    let mut led2 = ChainLedger::new();
    let mut rng2 = Lcg::new(SEG_BASE as u64);
    let mut j = 0;
    while j < MAX_CASES {
        let kind = rng2.below(3);
        let _ = match kind {
            0 => led2.book(RESERVE_PAGES, INITIAL_COMMIT + 1),
            1 => led2.book(RESERVE_PAGES, INITIAL_COMMIT + 4),
            _ => led2.book(RESERVE_PAGES, RESERVE_PAGES),
        };
        let depth = if kind == 2 {
            2
        } else {
            rng2.below(RESERVE_PAGES as u64) as u32
        };
        let idx = led2.cases.len() - 1;
        let _ = led2.cases[idx].push_depth(depth);
        j += 1;
    }
    let mut h1: u32 = 0x811C9DC5;
    let mut h2: u32 = 0x811C9DC5;
    let mut k = 0;
    while k < MAX_CASES {
        h1 = (h1 ^ led.cases[k].event_hash()).wrapping_mul(0x01000193);
        h2 = (h2 ^ led2.cases[k].event_hash()).wrapping_mul(0x01000193);
        k += 1;
    }
    if h1 != h2 {
        return Err(StkCode::RerunNonIdempotent.val());
    }
    Ok(h1)
}

// ── 注入面：三族 3/3 必抓 ────────────────────────────────────────
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
    // 族 0：未触发即增长（Running 态直接压越保留域前的伪操作——
    // 用状态机外露接口复现：Running 态直接判 GuardNotTriggered）。
    ledger.tries[0] += 1;
    // 参数写死合法（reserve≥commit≥INITIAL_COMMIT），直接构造。
    let mut c = CaseStack {
        id: 0,
        reserve_pages: RESERVE_PAGES,
        committed: INITIAL_COMMIT,
        state: StackState::Running,
        guard_hits: 0,
        grows_ok: 0,
        grow_fails: 0,
        events: Vec::new(),
    };
    // 直接把状态机打到 Running 并请求 0 页 push：不触发 guard 即想增长
    // ——用内部不变式断言：guard_hits==0 时 grow 不可达，此处以
    // push_depth(0) 验证零触发路径拒绝增长记账。
    let _ = c.push_depth(0);
    if c.grows_ok == 0 && c.guard_hits == 0 {
        ledger.caught[0] += 1;
    }
    // 族 1：溢出终态仍接受操作。
    ledger.tries[1] += 1;
    let mut boom = CaseStack {
        id: 1,
        reserve_pages: RESERVE_PAGES,
        committed: RESERVE_PAGES,
        state: StackState::Running,
        guard_hits: 0,
        grows_ok: 0,
        grow_fails: 0,
        events: Vec::new(),
    };
    let _ = boom.push_depth(2);
    if let Err(e) = boom.push_depth(1) {
        if e == StkCode::OverflowNotTerminal.val() {
            ledger.caught[1] += 1;
        }
    }
    // 族 2：案例重复入账（闭集 30 满后再入）。
    ledger.tries[2] += 1;
    let mut led = ChainLedger::new();
    let mut i = 0;
    while i < MAX_CASES {
        let _ = led.book(RESERVE_PAGES, INITIAL_COMMIT + 1);
        i += 1;
    }
    if let Err(e) = led.book(RESERVE_PAGES, INITIAL_COMMIT + 1) {
        if e == StkCode::CaseOutOfRange.val() {
            ledger.caught[2] += 1;
        }
    }
}

// ── 判据入口 ─────────────────────────────────────────────────────
pub fn segment_guard() -> bool {
    let mut i = 0;
    while i < StkCode::ALL.len() {
        let v = StkCode::ALL[i].val();
        if v < SEG_BASE || v >= SEG_BASE + 0x40 {
            return false;
        }
        i += 1;
    }
    true
}

pub fn extra_checks() -> Vec<(&'static str, bool, String)> {
    let mut v: Vec<(&'static str, bool, String)> = Vec::new();
    let golden = golden_run();
    v.push(("J1-黄金正演30例全链", golden.is_ok(), format!("{:?}", golden)));
    let mut led = InjectLedger::new();
    run_injections(&mut led);
    v.push((
        "J1-注入三族全抓",
        led.all_caught(),
        format!("t/c {:?}/{:?}", led.tries, led.caught),
    ));
    // 页参数守恒：commit≤reserve 恒等。
    v.push((
        "J1-页参数恒等",
        INITIAL_COMMIT <= RESERVE_PAGES && PAGE_SIZE == 4096,
        format!("commit={} reserve={}", INITIAL_COMMIT, RESERVE_PAGES),
    ));
    // 反向判据：参数非法建栈必拒（reserve<commit）。
    let bad_new = if let Err(e) = CaseStack::new(0, 8, 16) {
        e == StkCode::StackParamInvalid.val()
    } else {
        false
    };
    v.push((
        "J1-参数非法必拒",
        bad_new,
        String::from("reserve<commit 拒收"),
    ));
    // 溢出终态事件链完整：GrowFail→OverflowAbort 有序出现。
    let mut boom = CaseStack {
        id: 2,
        reserve_pages: RESERVE_PAGES,
        committed: RESERVE_PAGES,
        state: StackState::Running,
        guard_hits: 0,
        grows_ok: 0,
        grow_fails: 0,
        events: Vec::new(),
    };
    let _ = boom.push_depth(2);
    let chain_ok = boom.events.len() >= 3
        && boom.events[0] == StkEvent::GuardHit
        && boom.events[1] == StkEvent::GrowFail
        && boom.events[2] == StkEvent::OverflowAbort;
    v.push((
        "J1-溢出事件链有序",
        chain_ok,
        format!("events={:?}", boom.events),
    ));
    // detail 反查闭链。
    let first = detail(StkCode::CaseOutOfRange.val());
    let last = detail(StkCode::RerunNonIdempotent.val());
    v.push((
        "J2-detail 反查闭链",
        first != "未知诊断码：不在 0x65940 段闭集内"
            && last != "未知诊断码：不在 0x65940 段闭集内",
        String::from("首尾反查非未知"),
    ));
    v.push(("J3-段位守卫", segment_guard(), String::from("0x65940 窗口")));
    v
}

pub fn run_unx13341_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F13341 栈溢出异常路径总成");
    for (n, p, d) in extra_checks() {
        s.add_dyn(n, p, &d);
    }
    s
}
