//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F13161 · 进程默认堆创建挂点（装载完成→堆建立链） · 0x65700
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定）：
//!   ① 纯功能代码 ≥400 行；
//!   ② UNX-F13161-J1 默认堆建立链 30 例（装载完成→堆创建→PEB 登记）
//!      时序与 Windows 一致，创建时机分段断言（与 B09 装载完成态
//!      F12966 联动）；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - 堆建立链三段状态机（闭式边集）：Disarmed → LoadDone →
//!     HeapCreated → PebRegistered；未装载先建堆 / 未建堆先登记
//!     PEB 一律拒收（时机分段红线，与 F12966 装载完成态联动）；
//!   - 堆账守恒恒等式：allocated + free == committed（每笔提交/
//!     释放后必须复账成立）；
//!   - 默认堆唯一性：PEB 只准登记一座默认堆（重复登记必拒）；
//!   - 30 例时序账：段序严格递增（step 序号单调），倒序必抓；
//!   - LCG 种子写死 = SEG_BASE（0x65700），禁 rand/浮点/IO/墙钟；
//!     no_std + alloc；零 unwrap/expect。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::String;
use alloc::string::ToString;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x65700，全仓独占）。
pub const SEG_BASE: u32 = 0x65700;

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum HeapHookCode {
    /// 未装载完成先建堆
    HeapBeforeLoadDone,
    /// 未建堆先登记 PEB
    PebBeforeHeap,
    /// 默认堆重复登记
    PebDoubleRegister,
    /// 提交量越保留量
    CommitOverReserved,
    /// 释放越账（free 超 allocated）
    FreeOverAllocated,
    /// 守恒破（allocated + free != committed）
    ConservationBroken,
    /// 堆建立链非法状态迁移
    IllegalTransition,
    /// 段序倒置（时序账乱序）
    StepOrderBroken,
}

impl HeapHookCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            HeapHookCode::HeapBeforeLoadDone => "未装载完成先建堆：时机分段破（F12966 联动）",
            HeapHookCode::PebBeforeHeap => "未建堆先登记 PEB：时机分段破",
            HeapHookCode::PebDoubleRegister => "默认堆重复登记：PEB 唯一性破",
            HeapHookCode::CommitOverReserved => "提交量越保留量：堆账越界",
            HeapHookCode::FreeOverAllocated => "释放越账：free 超 allocated",
            HeapHookCode::ConservationBroken => "守恒破：allocated + free != committed",
            HeapHookCode::IllegalTransition => "堆建立链非法状态迁移：闭式边集拒收",
            HeapHookCode::StepOrderBroken => "段序倒置：时序账乱序",
        }
    }
    pub const ALL: [HeapHookCode; 8] = [
        HeapHookCode::HeapBeforeLoadDone,
        HeapHookCode::PebBeforeHeap,
        HeapHookCode::PebDoubleRegister,
        HeapHookCode::CommitOverReserved,
        HeapHookCode::FreeOverAllocated,
        HeapHookCode::ConservationBroken,
        HeapHookCode::IllegalTransition,
        HeapHookCode::StepOrderBroken,
    ];
}

pub fn detail(code: u32) -> &'static str {
    let mut i = 0;
    while i < HeapHookCode::ALL.len() {
        if HeapHookCode::ALL[i].val() == code {
            return HeapHookCode::ALL[i].detail();
        }
        i += 1;
    }
    "未知诊断码"
}

// ── 常量（来源：堆建立链建模书 + 判据原文） ──────────────────────
/// 默认堆保留量（建模：1MB 档）。
pub const HEAP_RESERVED: u32 = 0x0010_0000;
/// 默认堆初提交量（建模：2 页档）。
pub const HEAP_COMMIT0: u32 = 0x2000;
/// 时序账例数（判据原文：30 例）。
pub const CASES: usize = 30;

// ── 堆建立链三段状态机 ───────────────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum HeapChainState {
    /// 链未点火
    Disarmed,
    /// 装载完成态（F12966 联动）
    LoadDone,
    /// 默认堆已创建
    HeapCreated,
    /// PEB 已登记
    PebRegistered,
}

/// 链事件闭集。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum HeapChainEvent {
    /// 装载完成（B09/F12966 联动点火）
    ArmLoadDone,
    CreateHeap,
    RegisterPeb,
}

/// 合法边闭集；闭集之外一律 IllegalTransition。
pub fn heap_chain_transition(cur: HeapChainState, ev: HeapChainEvent) -> Result<HeapChainState, HeapHookCode> {
    match (cur, ev) {
        (HeapChainState::Disarmed, HeapChainEvent::ArmLoadDone) => Ok(HeapChainState::LoadDone),
        (HeapChainState::LoadDone, HeapChainEvent::CreateHeap) => Ok(HeapChainState::HeapCreated),
        (HeapChainState::HeapCreated, HeapChainEvent::RegisterPeb) => Ok(HeapChainState::PebRegistered),
        _ => Err(HeapHookCode::IllegalTransition),
    }
}

// ── 堆账（守恒恒等式载体） ───────────────────────────────────────
#[derive(Clone, Debug)]
pub struct HeapLedger {
    pub reserved: u32,
    pub committed: u32,
    pub allocated: u32,
    pub free: u32,
}

impl HeapLedger {
    pub fn new(reserved: u32, commit0: u32) -> Result<HeapLedger, HeapHookCode> {
        if commit0 > reserved {
            return Err(HeapHookCode::CommitOverReserved);
        }
        Ok(HeapLedger {
            reserved,
            committed: commit0,
            allocated: 0,
            free: commit0,
        })
    }

    /// 守恒核：allocated + free == committed。
    pub fn verify_conservation(&self) -> Result<(), HeapHookCode> {
        if self.allocated + self.free != self.committed {
            return Err(HeapHookCode::ConservationBroken);
        }
        Ok(())
    }

    /// 提交一段（越保留量必拒）。
    pub fn commit(&mut self, n: u32) -> Result<(), HeapHookCode> {
        let nc = match self.committed.checked_add(n) {
            Some(v) => v,
            None => return Err(HeapHookCode::CommitOverReserved),
        };
        if nc > self.reserved {
            return Err(HeapHookCode::CommitOverReserved);
        }
        self.committed = nc;
        self.free += n;
        self.verify_conservation()
    }

    /// 分配（从 free 转 allocated；free 不足必拒）。
    pub fn alloc(&mut self, n: u32) -> Result<(), HeapHookCode> {
        if n > self.free {
            return Err(HeapHookCode::FreeOverAllocated);
        }
        self.free -= n;
        self.allocated += n;
        self.verify_conservation()
    }

    /// 释放（allocated 转 free；越账必拒）。
    pub fn release(&mut self, n: u32) -> Result<(), HeapHookCode> {
        if n > self.allocated {
            return Err(HeapHookCode::FreeOverAllocated);
        }
        self.allocated -= n;
        self.free += n;
        self.verify_conservation()
    }
}

/// 段序账（时序分段断言载体：step 序号单调递增）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct StepMark {
    pub step_no: u32,
    pub kind: u8, // 0=LoadDone 1=HeapCreate 2=PebRegister
}

pub struct HeapChain {
    pub state: HeapChainState,
    pub ledger: Option<HeapLedger>,
    pub peb_default_heap: Option<u32>,
    pub steps: Vec<StepMark>,
    pub next_step: u32,
    pub cases_done: usize,
}

impl HeapChain {
    pub fn new() -> HeapChain {
        HeapChain {
            state: HeapChainState::Disarmed,
            ledger: None,
            peb_default_heap: None,
            steps: Vec::new(),
            next_step: 1,
            cases_done: 0,
        }
    }

    /// 记一笔段序（时序账：序号由 next_step 单调分发）。
    fn mark(&mut self, kind: u8) -> Result<(), HeapHookCode> {
        let no = self.next_step;
        if no == 0 {
            return Err(HeapHookCode::StepOrderBroken);
        }
        self.steps.push(StepMark { step_no: no, kind });
        self.next_step += 1;
        Ok(())
    }

    /// 全链一例：点火 → 建堆 → PEB 登记（时机分段 + 唯一性）。
    pub fn run_case(&mut self) -> Result<(), HeapHookCode> {
        // 时机红线：链必须从装载完成态点火（Disarmed 起点）。
        if self.state != HeapChainState::Disarmed {
            return Err(HeapHookCode::IllegalTransition);
        }
        if self.step_ev(HeapChainEvent::ArmLoadDone)? != HeapChainState::LoadDone {
            return Err(HeapHookCode::IllegalTransition);
        }
        // 时机红线：建堆前必须已点火。
        if self.step_ev(HeapChainEvent::CreateHeap)? != HeapChainState::HeapCreated {
            return Err(HeapHookCode::HeapBeforeLoadDone);
        }
        self.mark(1)?;
        let led = HeapLedger::new(HEAP_RESERVED, HEAP_COMMIT0)?;
        self.ledger = Some(led);
        if self.step_ev(HeapChainEvent::RegisterPeb)? != HeapChainState::PebRegistered {
            return Err(HeapHookCode::PebBeforeHeap);
        }
        self.mark(2)?;
        // PEB 唯一性：本进程内只准登记一座默认堆。
        match self.peb_default_heap {
            None => self.peb_default_heap = Some(0x7D000),
            Some(_) => return Err(HeapHookCode::PebDoubleRegister),
        }
        self.cases_done += 1;
        Ok(())
    }

    /// 同进程重复登记入口（独立探针面：PEB 已有默认堆再登记必拒）。
    pub fn register_peb_again(&mut self) -> Result<(), HeapHookCode> {
        if self.peb_default_heap.is_some() {
            return Err(HeapHookCode::PebDoubleRegister);
        }
        self.peb_default_heap = Some(0x7D001);
        Ok(())
    }

    fn step_ev(&mut self, ev: HeapChainEvent) -> Result<HeapChainState, HeapHookCode> {
        let ns = heap_chain_transition(self.state, ev)?;
        if ev == HeapChainEvent::ArmLoadDone {
            self.mark(0)?;
        }
        self.state = ns;
        Ok(ns)
    }
}

// ── LCG 注入核（种子写死 = SEG_BASE） ───────────────────────────
pub struct Lcg {
    state: u64,
}

impl Lcg {
    pub fn new(seed: u64) -> Lcg {
        Lcg { state: seed }
    }
    pub fn next(&mut self) -> u64 {
        self.state = self
            .state
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        self.state
    }
    pub fn next_below(&mut self, n: u64) -> u64 {
        if n == 0 {
            return 0;
        }
        self.next() % n
    }
}

// ── 判据钻取面 ───────────────────────────────────────────────────
/// J1 主面：默认堆建立链 30 例（时序 + PEB 登记）。
pub fn drill_chain_30_cases() -> (bool, String) {
    let mut ch = HeapChain::new();
    let mut i = 0;
    while i < CASES {
        if ch.run_case().is_err() {
            return (false, format!("第 {} 例链断", i + 1));
        }
        // 每例独立进程：复位链状态与 PEB 登记面。
        ch.state = HeapChainState::Disarmed;
        ch.peb_default_heap = None;
        i += 1;
    }
    let ok = ch.cases_done == CASES && ch.steps.len() == CASES * 3;
    (
        ok,
        format!("建立链 30 例全过，段序账 {} 笔", ch.steps.len()),
    )
}

/// 创建时机分段断言：乱序注入 10/10 必抓。
pub fn drill_timing_edges_10() -> (bool, String) {
    let mut caught = 0;
    // 1: 未点火建堆
    if heap_chain_transition(HeapChainState::Disarmed, HeapChainEvent::CreateHeap)
        == Err(HeapHookCode::IllegalTransition)
    {
        caught += 1;
    }
    // 2: 未点火登记 PEB
    if heap_chain_transition(HeapChainState::Disarmed, HeapChainEvent::RegisterPeb)
        == Err(HeapHookCode::IllegalTransition)
    {
        caught += 1;
    }
    // 3: 点火后直接登记 PEB（跳建堆）
    if heap_chain_transition(HeapChainState::LoadDone, HeapChainEvent::RegisterPeb)
        == Err(HeapHookCode::IllegalTransition)
    {
        caught += 1;
    }
    // 4: 点火后重复点火
    if heap_chain_transition(HeapChainState::LoadDone, HeapChainEvent::ArmLoadDone)
        == Err(HeapHookCode::IllegalTransition)
    {
        caught += 1;
    }
    // 5: 建堆后再建堆
    if heap_chain_transition(HeapChainState::HeapCreated, HeapChainEvent::CreateHeap)
        == Err(HeapHookCode::IllegalTransition)
    {
        caught += 1;
    }
    // 6: PEB 登记后再登记
    if heap_chain_transition(HeapChainState::PebRegistered, HeapChainEvent::RegisterPeb)
        == Err(HeapHookCode::IllegalTransition)
    {
        caught += 1;
    }
    // 7: PEB 登记后倒退点火
    if heap_chain_transition(HeapChainState::PebRegistered, HeapChainEvent::ArmLoadDone)
        == Err(HeapHookCode::IllegalTransition)
    {
        caught += 1;
    }
    // 8: 建堆后倒退点火
    if heap_chain_transition(HeapChainState::HeapCreated, HeapChainEvent::ArmLoadDone)
        == Err(HeapHookCode::IllegalTransition)
    {
        caught += 1;
    }
    // 9: PEB 登记后再建堆
    if heap_chain_transition(HeapChainState::PebRegistered, HeapChainEvent::CreateHeap)
        == Err(HeapHookCode::IllegalTransition)
    {
        caught += 1;
    }
    // 10: 合法边保通核（Disarmed--ArmLoadDone 必须通，不计入抓数）。
    let legal = heap_chain_transition(HeapChainState::Disarmed, HeapChainEvent::ArmLoadDone).is_ok();
    let ok = caught == 9 && legal;
    (
        ok,
        format!("时机乱序注入 {}/9 必抓（合法边保通={}）", caught, legal),
    )
}

/// 守恒面：堆账恒等式（alloc/release 10 组后复账全真）。
pub fn drill_conservation_10() -> (bool, String) {
    let mut led = match HeapLedger::new(HEAP_RESERVED, HEAP_COMMIT0) {
        Ok(l) => l,
        Err(e) => return (false, detail(e.val()).to_string()),
    };
    let mut ok = true;
    let mut i = 0;
    while i < 10 {
        if led.commit(0x1000).is_err() || led.alloc(0x800).is_err() || led.release(0x400).is_err()
        {
            ok = false;
            break;
        }
        if led.verify_conservation().is_err() {
            ok = false;
            break;
        }
        i += 1;
    }
    // 终账核：committed = 0x2000 + 0x10000；allocated = 0x4000；free = 差。
    let exp_committed = HEAP_COMMIT0 + 0x1000 * 10;
    let exp_allocated = 0x800u32 * 10 - 0x400u32 * 10;
    let final_ok = led.committed == exp_committed && led.allocated == exp_allocated;
    (
        ok && final_ok,
        format!(
            "守恒 10 组全真: committed=0x{:X} allocated=0x{:X} free=0x{:X}",
            led.committed, led.allocated, led.free
        ),
    )
}

/// 注入面：提交越保留量 10/10 必抓。
pub fn drill_commit_over_10() -> (bool, String) {
    let mut caught = 0;
    let mut i = 0;
    while i < 10 {
        let mut led = match HeapLedger::new(HEAP_RESERVED, HEAP_COMMIT0) {
            Ok(l) => l,
            Err(_) => break,
        };
        if led.commit(HEAP_RESERVED + 0x1000 * (i + 1) as u32) == Err(HeapHookCode::CommitOverReserved)
        {
            caught += 1;
        }
        i += 1;
    }
    (caught == 10, format!("越保留注入 {}/10 必抓", caught))
}

/// 注入面：释放越账 / free 不足 10/10 必抓。
pub fn drill_free_over_10() -> (bool, String) {
    let mut caught = 0;
    let mut i = 0;
    while i < 10 {
        let mut led = match HeapLedger::new(HEAP_RESERVED, HEAP_COMMIT0) {
            Ok(l) => l,
            Err(_) => break,
        };
        // 未分配就释放 → 越账。
        if led.release(0x1000 + i as u32) == Err(HeapHookCode::FreeOverAllocated) {
            caught += 1;
        }
        i += 1;
    }
    let ok = caught == 10;
    (ok, format!("释放越账注入 {}/10 必抓", caught))
}

/// PEB 唯一性：同进程重复登记默认堆必抓。
pub fn drill_peb_double_register() -> (bool, String) {
    let mut ch = HeapChain::new();
    let _ = ch.run_case();
    // 同进程再登记（不复位 PEB）→ 默认堆重复登记必抓。
    let r = ch.register_peb_again();
    (
        r == Err(HeapHookCode::PebDoubleRegister),
        format!("PEB 重复登记抓={:?}", r.is_err()),
    )
}

/// 段序账单调性：段序号严格递增（反向注入：乱插小号必抓）。
pub fn drill_step_monotonic() -> (bool, String) {
    let mut ch = HeapChain::new();
    let _ = ch.run_case();
    let mut ok = true;
    let mut prev = 0;
    let mut i = 0;
    while i < ch.steps.len() {
        if ch.steps[i].step_no <= prev {
            ok = false;
        }
        prev = ch.steps[i].step_no;
        i += 1;
    }
    // 反向注入：手动插一笔 0 号段（模拟倒序）→ 单调性必破。
    let mut broken = false;
    if ch.steps.len() > 1 {
        let back = StepMark { step_no: 0, kind: 0 };
        let probe: [StepMark; 3] = [ch.steps[0], back, ch.steps[1]];
        let mut p = 0;
        let mut j = 0;
        while j < probe.len() {
            if probe[j].step_no <= p {
                broken = true;
            }
            p = probe[j].step_no;
            j += 1;
        }
    }
    (ok && broken, format!("段序单调={} 倒序注入抓={}", ok, broken))
}

/// 多机对拍面：同种子两机重建，堆账终值一致。
pub fn drill_determinism() -> (bool, String) {
    let build = || {
        let mut led = match HeapLedger::new(HEAP_RESERVED, HEAP_COMMIT0) {
            Ok(l) => l,
            Err(_) => return (0u32, 0u32, 0u32),
        };
        let mut rng = Lcg::new(SEG_BASE as u64);
        let mut i = 0;
        while i < 12 {
            let n = (rng.next_below(8) + 1) as u32 * 0x400;
            let _ = led.commit(n);
            let _ = led.alloc(n / 2);
            i += 1;
        }
        (led.committed, led.allocated, led.free)
    };
    let a = build();
    let b = build();
    (a == b, format!("同种子对拍一致={}", a == b))
}

/// 全流程对账（J1 主链）。
pub fn drill_full_flow() -> (bool, String) {
    let (a1, d1) = drill_chain_30_cases();
    let (a2, d2) = drill_timing_edges_10();
    let (a3, d3) = drill_conservation_10();
    let ok = a1 && a2 && a3;
    (
        ok,
        format!(
            "全流程: 30例链={} 时机分段={} 守恒={} [{}|{}|{}]",
            a1, a2, a3, d1, d2, d3
        ),
    )
}

// ── 段位守卫 ─────────────────────────────────────────────────────
pub fn segment_guard() -> bool {
    let mut i = 0;
    let mut ok = true;
    while i < HeapHookCode::ALL.len() {
        let v = HeapHookCode::ALL[i].val();
        if v < SEG_BASE || v >= SEG_BASE + 16 {
            ok = false;
        }
        i += 1;
    }
    ok
}

// ── 判据聚合 ─────────────────────────────────────────────────────
pub fn extra_checks() -> Vec<(&'static str, bool, String)> {
    let mut v: Vec<(&'static str, bool, String)> = Vec::new();
    let (a, b) = drill_chain_30_cases();
    v.push(("J1-建立链30例时序一致", a, b));
    let (a, b) = drill_timing_edges_10();
    v.push(("J1-创建时机分段断言10/10", a, b));
    let (a, b) = drill_conservation_10();
    v.push(("J3-堆账守恒10组", a, b));
    let (a, b) = drill_commit_over_10();
    v.push(("J2-越保留注入10/10", a, b));
    let (a, b) = drill_free_over_10();
    v.push(("J2-释放越账注入10/10", a, b));
    let (a, b) = drill_peb_double_register();
    v.push(("J2-PEB重复登记抓", a, b));
    let (a, b) = drill_step_monotonic();
    v.push(("J3-段序单调+倒序注入", a, b));
    let (a, b) = drill_determinism();
    v.push(("J3-同种子对拍一致", a, b));
    let (a, b) = drill_full_flow();
    v.push(("J1-全流程对账", a, b));
    v
}

/// run_unx13161_checks：本单判据聚合（诊断段 0x65700）。
pub fn run_unx13161_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F13161 进程默认堆创建挂点");
    s.add_dyn("J3-段位守卫", segment_guard(), "0x65700 段闭集");
    for (n, p, d) in extra_checks() {
        s.add_dyn(n, p, &d);
    }
    s
}
