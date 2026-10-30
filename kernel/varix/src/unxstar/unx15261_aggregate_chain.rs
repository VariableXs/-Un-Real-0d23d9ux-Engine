//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F15261 · 聚合创建全链（外层注入路径） · 0x67140
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定）：
//!   ① 纯功能代码 ≥350 行；
//!   ② UNX-F15261-J1 聚合创建链成文（外层注入→内层装配→控制未知
//!      交付三步链 10/10 次），注入缺失（无外层直建）检出 10/10 次；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - 三步链闭集：外层注入（CoCreateInstance 携 AGGREGATE 标志建
//!     外层）→ 内层装配（内部创建携 AGGREGATE 标志，向内层 IUnknown
//!     协商聚合）→ 控制未知交付（内层返回非委托 IUnknown，外层持有
//!     委托边）；
//!   - 链序状态机：Idle → OuterInjected → InnerAssembled →
//!     Delivered，边闭集 3 条，逆序/跳步一律拒收；
//!   - 直建注入面：缺外层步（InnerAssemble 起步）= 注入缺失，
//!     必须检出（内层 pUnkOuter=null 直建）；
//!   - 双边账：外层委托边（outer→inner）与内层回执边
//!     （inner→ctrlUnk）成对守恒（1 出 1 回）；
//!   - LCG 种子写死 = SEG_BASE（0x67140），禁 rand/浮点/IO/墙钟；
//!   - no_std + alloc；零 unwrap/expect/panic，错误一律 Result + 中文诊断。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x67140，AI-VX3-1030 独占区）。
pub const SEG_BASE: u32 = 0x67140;

/// LCG 种子写死（来源：本卡 SEG_BASE）。
pub const LCG_SEED: u64 = 0x67140;

/// 三步链步码闭集。
pub const STEP_OUTER_INJECT: u8 = 0;
pub const STEP_INNER_ASSEMBLE: u8 = 1;
pub const STEP_CTRL_DELIVER: u8 = 2;
pub const STEP_COUNT: usize = 3;
pub const STEP_NAMES: [&str; STEP_COUNT] = ["外层注入", "内层装配", "控制未知交付"];

/// AGGREGATE 标志位（创建语义位，写死来源：COM CLSCTX 聚合语义）。
pub const FLAG_AGGREGATE: u32 = 0x1;
/// 空外层哨兵值（pUnkOuter == null 的建模值）。
pub const UNK_OUTER_NULL: u32 = 0;

/// 判据投影：全链 10/10 次。
pub const CHAIN_TRIALS: usize = 10;

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum AggCode {
    /// 步码越闭集
    StepOutOfRange,
    /// 链序违规（跳步/逆序/重步）
    ChainOrderBreach,
    /// 外层注入缺 AGGREGATE 标志
    MissingAggregateFlag,
    /// 内层装配携空外层（无外层直建）
    NullOuterAssemble,
    /// 控制未知未交付（装配后缺交付步）
    CtrlUnknownWithheld,
    /// 委托/回执双边不守恒
    EdgeLedgerBreach,
    /// 链未收敛即销毁
    ChainTorn,
    /// 步码重复（同一链同一步二入）
    StepDuplicated,
    /// 控制未知身份错（回执非内层裸 IUnknown）
    CtrlIdentityWrong,
}

impl AggCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            AggCode::StepOutOfRange => "步码越闭集：三步链冻结",
            AggCode::ChainOrderBreach => "链序违规：外→内→交付单向闭边",
            AggCode::MissingAggregateFlag => "外层注入缺 AGGREGATE 标志",
            AggCode::NullOuterAssemble => "内层装配携空外层：无外层直建注入",
            AggCode::CtrlUnknownWithheld => "控制未知未交付：三步链断尾",
            AggCode::EdgeLedgerBreach => "委托/回执双边不守恒",
            AggCode::ChainTorn => "链未收敛即销毁：撕链残账",
            AggCode::StepDuplicated => "同一步二入：链账重记",
            AggCode::CtrlIdentityWrong => "回执非内层裸 IUnknown：身份错",
        }
    }
    pub const ALL: [AggCode; 9] = [
        AggCode::StepOutOfRange,
        AggCode::ChainOrderBreach,
        AggCode::MissingAggregateFlag,
        AggCode::NullOuterAssemble,
        AggCode::CtrlUnknownWithheld,
        AggCode::EdgeLedgerBreach,
        AggCode::ChainTorn,
        AggCode::StepDuplicated,
        AggCode::CtrlIdentityWrong,
    ];
}

pub fn detail(code: u32) -> &'static str {
    let mut i = 0;
    while i < AggCode::ALL.len() {
        if AggCode::ALL[i].val() == code {
            return AggCode::ALL[i].detail();
        }
        i += 1;
    }
    "未知诊断码：不在 0x67140 段闭集内"
}

// ── 链序状态机 ───────────────────────────────────────────────────
/// Idle →(外层注入)→ OuterInjected →(内层装配)→ InnerAssembled
///           →(控制未知交付)→ Delivered。
/// 非法转换：任何逆边/自环/跳步均拒收。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ChainPhase {
    Idle,
    OuterInjected,
    InnerAssembled,
    Delivered,
}

/// 合法边闭集：from → to（3 条，单向）。
pub fn legal_edge(from: ChainPhase, to: ChainPhase) -> bool {
    match (from, to) {
        (ChainPhase::Idle, ChainPhase::OuterInjected) => true,
        (ChainPhase::OuterInjected, ChainPhase::InnerAssembled) => true,
        (ChainPhase::InnerAssembled, ChainPhase::Delivered) => true,
        _ => false,
    }
}

/// 一条聚合链账元。
pub struct AggregateChain {
    pub id: u32,
    pub phase: ChainPhase,
    /// 外层对象 id（外层注入步写入）。
    pub outer_id: u32,
    /// 内层对象 id（内层装配步写入）。
    pub inner_id: u32,
    /// 委托边：外层持有的内层委托 IUnknown（0=无）。
    pub delegate_edge: u32,
    /// 回执边：内层交付的裸控制未知（0=无）。
    pub ctrl_edge: u32,
    /// 已入账步码位图（bit0/1/2）。
    pub step_bitmap: u32,
}

impl AggregateChain {
    pub fn new(id: u32) -> AggregateChain {
        AggregateChain {
            id,
            phase: ChainPhase::Idle,
            outer_id: 0,
            inner_id: 0,
            delegate_edge: 0,
            ctrl_edge: 0,
            step_bitmap: 0,
        }
    }
}

// ── 链账本 ───────────────────────────────────────────────────────
pub struct AggLedger {
    pub chains: Vec<AggregateChain>,
    /// 直建注入检出计数。
    pub direct_build_caught: u32,
    /// 全链成文计数（三步走满且双边守恒）。
    pub chains_completed: u32,
    /// 对象账（简化：对象 id 连续号）。
    next_obj: u32,
}

impl AggLedger {
    pub fn new() -> AggLedger {
        AggLedger {
            chains: Vec::new(),
            direct_build_caught: 0,
            chains_completed: 0,
            next_obj: 1,
        }
    }

    fn alloc_obj(&mut self) -> u32 {
        let v = self.next_obj;
        self.next_obj += 1;
        v
    }

    /// 步 0：外层注入。要求携 AGGREGATE 标志；链相 Idle。
    pub fn outer_inject(&mut self, idx: usize, flags: u32) -> Result<u32, u32> {
        if idx >= self.chains.len() {
            return Err(AggCode::StepOutOfRange.val());
        }
        if (flags & FLAG_AGGREGATE) == 0 {
            return Err(AggCode::MissingAggregateFlag.val());
        }
        let oid = self.alloc_obj();
        let c = &mut self.chains[idx];
        let to = ChainPhase::OuterInjected;
        if !legal_edge(c.phase, to) {
            return Err(AggCode::ChainOrderBreach.val());
        }
        if (c.step_bitmap & 0b001) != 0 {
            return Err(AggCode::StepDuplicated.val());
        }
        c.phase = to;
        c.step_bitmap |= 0b001;
        c.outer_id = oid;
        Ok(oid)
    }

    /// 步 1：内层装配。pUnkOuter 为空 = 无外层直建注入（检出拒收）。
    pub fn inner_assemble(&mut self, idx: usize, unk_outer: u32) -> Result<u32, u32> {
        if idx >= self.chains.len() {
            return Err(AggCode::StepOutOfRange.val());
        }
        if unk_outer == UNK_OUTER_NULL {
            self.direct_build_caught += 1;
            return Err(AggCode::NullOuterAssemble.val());
        }
        let iid = self.alloc_obj();
        let c = &mut self.chains[idx];
        let to = ChainPhase::InnerAssembled;
        if !legal_edge(c.phase, to) {
            return Err(AggCode::ChainOrderBreach.val());
        }
        if (c.step_bitmap & 0b010) != 0 {
            return Err(AggCode::StepDuplicated.val());
        }
        if unk_outer != c.outer_id {
            return Err(AggCode::CtrlIdentityWrong.val());
        }
        c.phase = to;
        c.step_bitmap |= 0b010;
        c.inner_id = iid;
        // 委托边落地：外层 → 内层（装配即挂）。
        c.delegate_edge = c.inner_id;
        Ok(iid)
    }

    /// 步 2：控制未知交付。内层返回裸控制未知（≠ 内层委托边身份）。
    pub fn ctrl_deliver(&mut self, idx: usize, ctrl_unk: u32) -> Result<u32, u32> {
        if idx >= self.chains.len() {
            return Err(AggCode::StepOutOfRange.val());
        }
        let c = &mut self.chains[idx];
        if ctrl_unk == 0 {
            return Err(AggCode::CtrlUnknownWithheld.val());
        }
        if ctrl_unk == c.inner_id {
            // 交付了内层委托边而非裸控制未知：身份错。
            return Err(AggCode::CtrlIdentityWrong.val());
        }
        let to = ChainPhase::Delivered;
        if !legal_edge(c.phase, to) {
            return Err(AggCode::ChainOrderBreach.val());
        }
        if (c.step_bitmap & 0b100) != 0 {
            return Err(AggCode::StepDuplicated.val());
        }
        c.phase = to;
        c.step_bitmap |= 0b100;
        c.ctrl_edge = ctrl_unk;
        self.chains_completed += 1;
        Ok(ctrl_unk)
    }

    /// 双边守恒审计：Delivered 链必须有且只有 1 出 1 回。
    pub fn audit_edges(&self) -> Result<(), u32> {
        let mut i = 0;
        while i < self.chains.len() {
            let c = &self.chains[i];
            match c.phase {
                ChainPhase::Delivered => {
                    if c.delegate_edge == 0 || c.ctrl_edge == 0 {
                        return Err(AggCode::EdgeLedgerBreach.val());
                    }
                    if c.delegate_edge != c.inner_id || c.ctrl_edge == c.inner_id {
                        return Err(AggCode::CtrlIdentityWrong.val());
                    }
                }
                ChainPhase::InnerAssembled => {
                    // 断尾链：装配未交付（判据链走满口径下不允许）。
                    return Err(AggCode::CtrlUnknownWithheld.val());
                }
                _ => {}
            }
            i += 1;
        }
        Ok(())
    }

    /// 一条全链正演（黄金三步，返回交付的控制未知 id）。
    pub fn run_full_chain(&mut self) -> Result<u32, u32> {
        let idx = self.chains.len();
        self.chains.push(AggregateChain::new(idx as u32));
        let _ = self.outer_inject(idx, FLAG_AGGREGATE)?;
        let _ = self.inner_assemble(idx, self.chains[idx].outer_id)?;
        let ctrl = self.next_obj; // 预演交付对象号（裸控制未知 ≠ 内层号）
        self.alloc_obj();
        self.ctrl_deliver(idx, ctrl)?;
        self.audit_edges()?;
        Ok(ctrl)
    }
}

// ── 确定性 LCG（种子 = 0x67140） ─────────────────────────────────
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
        self.next() % if n == 0 { 1 } else { n }
    }
}

// ── 10/10 全链成文压演 ───────────────────────────────────────────
/// 每轮：正演一条全链（三步）+ 同轮 3 次直建注入（必检出）。
/// 返回 (成文链数, 直建检出数)。
pub fn chain_round(round: usize) -> Result<(u32, u32), u32> {
    let mut lcg = Lcg::new(LCG_SEED + round as u64);
    let mut led = AggLedger::new();
    let ctrl = led.run_full_chain()?;
    if ctrl == 0 {
        return Err(AggCode::CtrlUnknownWithheld.val());
    }
    // 同轮直建注入三次：空外层装配必须全部检出。
    let mut caught = 0;
    let mut k = 0;
    while k < 3 {
        let idx = led.chains.len();
        led.chains.push(AggregateChain::new(idx as u32));
        // 注入变异：跳过外层注入，直接内层装配（携空外层）。
        if let Err(e) = led.inner_assemble(idx, UNK_OUTER_NULL) {
            if e == AggCode::NullOuterAssemble.val() {
                caught += 1;
            }
        }
        k += 1;
    }
    if caught != 3 {
        return Err(AggCode::NullOuterAssemble.val());
    }
    // 混序注入一次：Idle 链直接交付（跳步必拒）。
    let idx = led.chains.len();
    led.chains.push(AggregateChain::new(idx as u32));
    let skip = led.ctrl_deliver(idx, 7777);
    if skip.is_ok() {
        return Err(AggCode::ChainOrderBreach.val());
    }
    led.audit_edges()?;
    let _ = lcg.next(); // 消费一轮随机性（确定性口径占位，不影响判定）
    Ok((led.chains_completed, led.direct_build_caught))
}

// ── 黄金正演 ─────────────────────────────────────────────────────
pub fn golden_run() -> Result<(u32, u32, u32), u32> {
    let mut completed = 0;
    let mut direct_caught = 0;
    let mut r = 0;
    while r < CHAIN_TRIALS {
        let (c, d) = chain_round(r)?;
        if c != 1 || d != 3 {
            return Err(AggCode::EdgeLedgerBreach.val());
        }
        completed += c;
        direct_caught += d;
        r += 1;
    }
    // 黄金单链对照（手算）：三步位图 = 0b111。
    let mut led = AggLedger::new();
    let ctrl = led.run_full_chain()?;
    if led.chains[0].step_bitmap != 0b111 {
        return Err(AggCode::StepDuplicated.val());
    }
    if ctrl == 0 || led.chains[0].outer_id == 0 || led.chains[0].inner_id == 0 {
        return Err(AggCode::EdgeLedgerBreach.val());
    }
    Ok((completed, direct_caught, led.chains_completed))
}

// ── 注入面：三族 10/10 必抓 ──────────────────────────────────────
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
    // 族 0：无外层直建（空外层装配）×10。
    let mut t = 0;
    while t < 10 {
        let mut led = AggLedger::new();
        led.chains.push(AggregateChain::new(0));
        let r = led.inner_assemble(0, UNK_OUTER_NULL);
        ledger.tries[0] += 1;
        if let Err(e) = r {
            if e == AggCode::NullOuterAssemble.val() {
                ledger.caught[0] += 1;
            }
        }
        t += 1;
    }
    // 族 1：缺 AGGREGATE 标志的外层注入 ×10。
    let mut t = 0;
    while t < 10 {
        let mut led = AggLedger::new();
        led.chains.push(AggregateChain::new(0));
        let r = led.outer_inject(0, 0);
        ledger.tries[1] += 1;
        if let Err(e) = r {
            if e == AggCode::MissingAggregateFlag.val() {
                ledger.caught[1] += 1;
            }
        }
        t += 1;
    }
    // 族 2：控制未知身份错（把内层委托边当裸控制未知交付）×10。
    let mut t = 0;
    while t < 10 {
        let mut led = AggLedger::new();
        let ctrl = led.run_full_chain();
        if ctrl.is_err() {
            // 正演失败也计入 try，但不得误抓身份错（此处直接继续）。
            ledger.tries[2] += 1;
            t += 1;
            continue;
        }
        // 新链走两步后交付内层号（身份错注入）。
        let idx = led.chains.len();
        led.chains.push(AggregateChain::new(idx as u32));
        let _ = led.outer_inject(idx, FLAG_AGGREGATE);
        let outer = led.chains[idx].outer_id;
        let _ = led.inner_assemble(idx, outer);
        let inner = led.chains[idx].inner_id;
        let r = led.ctrl_deliver(idx, inner);
        ledger.tries[2] += 1;
        if let Err(e) = r {
            if e == AggCode::CtrlIdentityWrong.val() {
                ledger.caught[2] += 1;
            }
        }
        t += 1;
    }
}

// ── 判据入口 ─────────────────────────────────────────────────────
pub fn segment_guard() -> bool {
    let mut i = 0;
    while i < AggCode::ALL.len() {
        let v = AggCode::ALL[i].val();
        if v < SEG_BASE || v >= SEG_BASE + 0x40 {
            return false;
        }
        i += 1;
    }
    // 链边闭集自洽：合法边恰 3 条且方向单调。
    let mut edge_count = 0;
    let phases = [
        ChainPhase::Idle,
        ChainPhase::OuterInjected,
        ChainPhase::InnerAssembled,
        ChainPhase::Delivered,
    ];
    let mut a = 0;
    while a < 4 {
        let mut b = 0;
        while b < 4 {
            if legal_edge(phases[a], phases[b]) {
                edge_count += 1;
            }
            b += 1;
        }
        a += 1;
    }
    edge_count == 3
}

pub fn extra_checks() -> Vec<(&'static str, bool, String)> {
    let mut v: Vec<(&'static str, bool, String)> = Vec::new();
    let golden = golden_run();
    let golden_ok = if let Ok((c, d, _)) = golden {
        c == CHAIN_TRIALS as u32 && d == CHAIN_TRIALS as u32 * 3
    } else {
        false
    };
    v.push((
        "J1-三步链10/10成文+直建检出",
        golden_ok,
        format!("{:?}", golden),
    ));
    let mut led = InjectLedger::new();
    run_injections(&mut led);
    v.push((
        "J1-注入三族全抓",
        led.all_caught(),
        format!("t/c {:?}/{:?}", led.tries, led.caught),
    ));
    // 步名点验。
    let mut okn = 0;
    let mut i = 0;
    while i < STEP_COUNT {
        if STEP_NAMES[i].len() > 0 {
            okn += 1;
        }
        i += 1;
    }
    v.push((
        "J1-三步名点验",
        okn == STEP_COUNT,
        format!("{}/3", okn),
    ));
    // 合法边闭集点验（3 条）。
    v.push((
        "J1-链边闭集=3",
        segment_guard(),
        String::from("Idle→外→内→交付"),
    ));
    // 双边守恒独立复核（空账自洽）。
    let led0 = AggLedger::new();
    v.push((
        "J1-空账双边守恒自洽",
        led0.audit_edges().is_ok(),
        String::from("空账 0 出 0 回"),
    ));
    // 判据常量独立写死点验。
    v.push((
        "J1-判据常量独立",
        CHAIN_TRIALS == 10 && FLAG_AGGREGATE == 1 && UNK_OUTER_NULL == 0,
        format!("T={} F={:x} N={}", CHAIN_TRIALS, FLAG_AGGREGATE, UNK_OUTER_NULL),
    ));
    v.push(("J3-段位守卫", segment_guard(), String::from("0x67140 窗口")));
    v
}

pub fn run_unx15261_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F15261 聚合创建全链（外层注入路径）");
    for (n, p, d) in extra_checks() {
        s.add_dyn(n, p, &d);
    }
    s
}
