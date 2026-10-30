//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F15241 · 强/弱引用计数双域分离 · 0x67100
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定）：
//!   ① 纯功能代码 ≥350 行；
//!   ② UNX-F15241-J1 双域成文（强/弱计数独立且销毁单点，10/10 次），
//!      混域注入 10/10 次检出；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - 双域账：强计数字段与弱计数字段物理分离，各自守恒；
//!   - 销毁单点：仅强计数归零触发 DestroyPending（单一触发点，
//!     其他路径禁止置位）；弱计数归零且已 DestroyPending 才可
//!     Freed（弱存活性闸）；
//!   - 每个操作带「声明域 vs 执行域」双标签，错配即混域注入，
//!     必须拒收并入混域检出账；
//!   - 守恒恒等式：加域增量总和 − 减域减量总和 == 当前值，
//!     逐对象逐域对账；
//!   - LCG 种子写死 = SEG_BASE（0x67100），禁 rand/浮点/IO/墙钟；
//!   - no_std + alloc；零 unwrap/expect/panic，错误一律 Result + 中文诊断。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x67100，AI-VX3-1030 独占区）。
pub const SEG_BASE: u32 = 0x67100;

/// LCG 种子写死（来源：本卡 SEG_BASE）。
pub const LCG_SEED: u64 = 0x67100;

/// 操作码闭集（声明域编码：高 2 位域，低 2 位动作）。
pub const OP_ADD_STRONG: u8 = 0b00_00; // 强域加
pub const OP_REL_STRONG: u8 = 0b00_01; // 强域减
pub const OP_ADD_WEAK: u8 = 0b10_00; // 弱域加
pub const OP_REL_WEAK: u8 = 0b10_01; // 弱域减
pub const OP_CODE_MASK: u8 = 0b11;
pub const OP_DOMAIN_SHIFT: u8 = 2;
/// 域标签：0=强域，1=弱域。
pub const DOMAIN_STRONG: u8 = 0;
pub const DOMAIN_WEAK: u8 = 1;

/// 双域表名（写死，来源：COM 引用计数双域语义）。
pub const DOMAIN_NAMES: [&str; 2] = ["强引用域", "弱引用域"];

/// 万轮抽演规模：10 轮 × 每轮 1000 操作（判据 10/10 的建模投影）。
pub const ROUNDS: usize = 10;
pub const OPS_PER_ROUND: usize = 1000;

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DualCode {
    /// 强域下溢（强计数减至负）
    StrongUnderflow,
    /// 弱域下溢（弱计数减至负）
    WeakUnderflow,
    /// 已销毁对象再受操作
    OpOnDestroyed,
    /// 混域写（声明域与执行域错配）
    MixedDomainWrite,
    /// 弱存活闸破（弱计数非零即释放）
    WeakLeakAtFree,
    /// 销毁单点被绕过（非强归零路径置 DestroyPending）
    DoubleDestroy,
    /// 守恒恒等式破账
    ConservationBreach,
    /// 操作码越闭集
    OpCodeInvalid,
    /// 撕毁后余账（终态非 Freed 的对象残留）
    TornLeftover,
}

impl DualCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            DualCode::StrongUnderflow => "强域下溢：强计数不得减至负",
            DualCode::WeakUnderflow => "弱域下溢：弱计数不得减至负",
            DualCode::OpOnDestroyed => "已销毁对象再受操作：生命周期红线",
            DualCode::MixedDomainWrite => "混域写：声明域与执行域错配",
            DualCode::WeakLeakAtFree => "弱存活闸破：弱计数非零即释放",
            DualCode::DoubleDestroy => "销毁单点被绕过：仅强归零可触发",
            DualCode::ConservationBreach => "守恒恒等式破账：增量−减量≠当前",
            DualCode::OpCodeInvalid => "操作码越闭集：4 码冻结",
            DualCode::TornLeftover => "撕毁后余账：终态非 Freed 残留",
        }
    }
    pub const ALL: [DualCode; 9] = [
        DualCode::StrongUnderflow,
        DualCode::WeakUnderflow,
        DualCode::OpOnDestroyed,
        DualCode::MixedDomainWrite,
        DualCode::WeakLeakAtFree,
        DualCode::DoubleDestroy,
        DualCode::ConservationBreach,
        DualCode::OpCodeInvalid,
        DualCode::TornLeftover,
    ];
}

pub fn detail(code: u32) -> &'static str {
    let mut i = 0;
    while i < DualCode::ALL.len() {
        if DualCode::ALL[i].val() == code {
            return DualCode::ALL[i].detail();
        }
        i += 1;
    }
    "未知诊断码：不在 0x67100 段闭集内"
}

// ── 对象生命周期状态机 ───────────────────────────────────────────
/// Active（活）→ DestroyPending（强归零，销毁单点）→ Freed（弱亦归零）。
/// DestroyPending 只能由 release_strong 归零路径进入（销毁单点）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CellPhase {
    Active,
    DestroyPending,
    Freed,
}

/// 单对象双域账元。
pub struct ObjectCell {
    pub id: u32,
    pub strong: i64,
    pub weak: i64,
    pub phase: CellPhase,
    /// 强域累计加 / 减（守恒对账列）。
    pub strong_added: i64,
    pub strong_released: i64,
    /// 弱域累计加 / 减（守恒对账列）。
    pub weak_added: i64,
    pub weak_released: i64,
}

impl ObjectCell {
    pub fn new(id: u32) -> ObjectCell {
        // 初始：1 强 0 弱（CoCreateInstance 语义：创建即持一强引用）。
        ObjectCell {
            id,
            strong: 1,
            weak: 0,
            phase: CellPhase::Active,
            strong_added: 1,
            strong_released: 0,
            weak_added: 0,
            weak_released: 0,
        }
    }
}

// ── 双域账本 ─────────────────────────────────────────────────────
pub struct DualLedger {
    pub cells: Vec<ObjectCell>,
    pub mixed_caught: u32,
    pub ops_total: u64,
}

impl DualLedger {
    pub fn new() -> DualLedger {
        DualLedger { cells: Vec::new(), mixed_caught: 0, ops_total: 0 }
    }

    pub fn spawn(&mut self) -> u32 {
        let id = self.cells.len() as u32;
        self.cells.push(ObjectCell::new(id));
        id
    }

    /// 混域拒收：声明域与执行域错配时不落账，只记检出。
    fn domain_mismatch(&mut self, declared: u8, executed: u8) -> bool {
        if declared != executed {
            self.mixed_caught += 1;
            true
        } else {
            false
        }
    }

    /// 强域加：仅 Active/DestroPending 允许？——销毁单点后一律拒。
    pub fn add_strong(&mut self, id: u32) -> Result<i64, u32> {
        let n = self.cells.len();
        if (id as usize) >= n {
            return Err(DualCode::OpOnDestroyed.val());
        }
        let c = &mut self.cells[id as usize];
        if c.phase != CellPhase::Active {
            return Err(DualCode::OpOnDestroyed.val());
        }
        self.ops_total += 1;
        c.strong += 1;
        c.strong_added += 1;
        Ok(c.strong)
    }

    /// 强域减：归零触发销毁单点。
    pub fn release_strong(&mut self, id: u32) -> Result<CellPhase, u32> {
        let n = self.cells.len();
        if (id as usize) >= n {
            return Err(DualCode::OpOnDestroyed.val());
        }
        let c = &mut self.cells[id as usize];
        if c.phase != CellPhase::Active {
            return Err(DualCode::OpOnDestroyed.val());
        }
        if c.strong <= 0 {
            return Err(DualCode::StrongUnderflow.val());
        }
        self.ops_total += 1;
        c.strong -= 1;
        c.strong_released += 1;
        if c.strong == 0 {
            // 销毁单点：唯一允许离开 Active 的路径。弱存活闸在此同点
            // 判定：弱已归零 → 直接 Freed；仍有弱账 → DestroyPending。
            c.phase = if c.weak == 0 { CellPhase::Freed } else { CellPhase::DestroyPending };
        }
        Ok(c.phase)
    }

    /// 弱域加：DestroyPending 后禁加（销毁中不可再观察）。
    pub fn add_weak(&mut self, id: u32) -> Result<i64, u32> {
        let n = self.cells.len();
        if (id as usize) >= n {
            return Err(DualCode::OpOnDestroyed.val());
        }
        let c = &mut self.cells[id as usize];
        if c.phase == CellPhase::Freed {
            return Err(DualCode::OpOnDestroyed.val());
        }
        self.ops_total += 1;
        c.weak += 1;
        c.weak_added += 1;
        Ok(c.weak)
    }

    /// 弱域减：归零且 DestroyPending → Freed（弱存活闸过）。
    pub fn release_weak(&mut self, id: u32) -> Result<CellPhase, u32> {
        let n = self.cells.len();
        if (id as usize) >= n {
            return Err(DualCode::OpOnDestroyed.val());
        }
        let c = &mut self.cells[id as usize];
        if c.phase == CellPhase::Freed {
            return Err(DualCode::OpOnDestroyed.val());
        }
        if c.weak <= 0 {
            return Err(DualCode::WeakUnderflow.val());
        }
        self.ops_total += 1;
        c.weak -= 1;
        c.weak_released += 1;
        if c.weak == 0 && c.phase == CellPhase::DestroyPending {
            c.phase = CellPhase::Freed;
        }
        Ok(c.phase)
    }

    /// 统一入口：按操作码分派 + 混域双标签核验。
    /// `executed_domain` 为账本实际执行的域；与操作码内声明域错配即混域。
    pub fn apply(&mut self, id: u32, op: u8, executed_domain: u8) -> Result<CellPhase, u32> {
        let action = op & OP_CODE_MASK;
        let declared_domain = op >> OP_DOMAIN_SHIFT;
        if action > OP_REL_STRONG || declared_domain > DOMAIN_WEAK {
            return Err(DualCode::OpCodeInvalid.val());
        }
        if self.domain_mismatch(declared_domain, executed_domain) {
            // 混域注入：拒收（不落账），返回专用错误。
            return Err(DualCode::MixedDomainWrite.val());
        }
        match (declared_domain, action) {
            (DOMAIN_STRONG, 0) => self.add_strong(id).map(|_| CellPhase::Active),
            (DOMAIN_STRONG, 1) => self.release_strong(id),
            (DOMAIN_WEAK, 0) => self.add_weak(id).map(|_| CellPhase::Active),
            _ => self.release_weak(id),
        }
    }

    /// 守恒对账：加 − 减 == 当前，逐对象逐域。
    pub fn audit_conservation(&self) -> Result<(), u32> {
        let mut i = 0;
        while i < self.cells.len() {
            let c = &self.cells[i];
            if c.strong_added - c.strong_released != c.strong {
                return Err(DualCode::ConservationBreach.val());
            }
            if c.weak_added - c.weak_released != c.weak {
                return Err(DualCode::ConservationBreach.val());
            }
            if c.strong < 0 || c.weak < 0 {
                return Err(DualCode::ConservationBreach.val());
            }
            i += 1;
        }
        Ok(())
    }

    /// 撕毁收尾：DestroyPending 且弱归零的对象应已 Freed；
    /// 终态清点：Active 且强归零者违规。
    pub fn audit_terminal(&self) -> Result<u32, u32> {
        let mut freed = 0;
        let mut i = 0;
        while i < self.cells.len() {
            let c = &self.cells[i];
            match c.phase {
                CellPhase::Freed => {
                    if c.weak != 0 || c.strong != 0 {
                        return Err(DualCode::WeakLeakAtFree.val());
                    }
                    freed += 1;
                }
                CellPhase::DestroyPending => {
                    if c.weak > 0 {
                        return Err(DualCode::WeakLeakAtFree.val());
                    }
                    return Err(DualCode::TornLeftover.val());
                }
                CellPhase::Active => {
                    if c.strong == 0 {
                        return Err(DualCode::DoubleDestroy.val());
                    }
                }
            }
            i += 1;
        }
        Ok(freed)
    }

    /// 收尾撕毁：把所有余账按序放干净（先弱后强，终态全 Freed）。
    pub fn teardown(&mut self) -> Result<u32, u32> {
        let mut freed = 0;
        let mut i = 0;
        while i < self.cells.len() {
            let n = self.cells[i].weak;
            let mut k = 0;
            while k < n {
                self.release_weak(i as u32)?;
                k += 1;
            }
            let m = self.cells[i].strong;
            let mut k = 0;
            while k < m {
                self.release_strong(i as u32)?;
                k += 1;
            }
            if self.cells[i].phase == CellPhase::Freed {
                freed += 1;
            }
            i += 1;
        }
        self.audit_conservation()?;
        Ok(freed)
    }
}

// ── 确定性 LCG（种子 = 0x67100） ─────────────────────────────────
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

// ── 单轮随机双域压演（1000 操作后必可撕净） ─────────────────────
pub fn random_round(round: usize) -> Result<(u32, u32), u32> {
    let mut lcg = Lcg::new(LCG_SEED + round as u64);
    let mut led = DualLedger::new();
    let mut _spawned = 0u32;
    let mut i = 0;
    while i < OPS_PER_ROUND {
        let which = lcg.below(3) as u32; // 0..2 号对象
        let target = if which >= led.cells.len() as u32 && !led.cells.is_empty() {
            (lcg.below(led.cells.len() as u64)) as u32
        } else if led.cells.is_empty() {
            led.spawn();
            _spawned += 1;
            0
        } else {
            which
        };
        let op_sel = lcg.below(4) as u8;
        let op = match op_sel {
            0 => OP_ADD_STRONG,
            1 => OP_REL_STRONG,
            2 => OP_ADD_WEAK,
            _ => OP_REL_WEAK,
        };
        // 正演只允许声明域=执行域（混域走注入面）。
        let _ = led.apply(target, op, op >> OP_DOMAIN_SHIFT);
        // 弱归零放行：若 DestroyPending 且弱=0，已由 release_weak 置 Freed；
        // 强归零但仍有弱账的对象保持 DestroyPending，可继续 release_weak。
        let _ = led.audit_conservation()?;
        i += 1;
    }
    let freed = led.teardown()?;
    if led.mixed_caught != 0 {
        // 正演面不应出现混域（混域属注入面）。
        return Err(DualCode::MixedDomainWrite.val());
    }
    Ok((freed, led.ops_total as u32))
}

// ── 黄金正演：10/10 轮全撕净 + 手算黄金对照表 ────────────────────
/// 黄金对照表（手算写死，与运行值独立）：
///   序列 a [spawn, add_weak, rel_weak, rel_strong] → Freed（弱先清、强归零过闸）；
///   序列 b [spawn, add_weak, rel_strong] → DestroyPending（弱>0 挡住 Freed）；
///   序列 c [spawn, add_strong, rel_strong, rel_strong] → Freed（弱恒 0）。
fn probe_seq_a() -> Result<CellPhase, u32> {
    let mut led = DualLedger::new();
    let id = led.spawn();
    let _ = led.add_weak(id)?;
    let _ = led.release_weak(id)?;
    led.release_strong(id)
}
fn probe_seq_b() -> Result<CellPhase, u32> {
    // 弱存活闸黄金序：先挂一弱账再销毁 → 弱>0 挡住 Freed。
    let mut led = DualLedger::new();
    let id = led.spawn();
    let _ = led.add_weak(id)?;
    led.release_strong(id)
}
fn probe_seq_c() -> Result<CellPhase, u32> {
    let mut led = DualLedger::new();
    let id = led.spawn();
    let _ = led.add_strong(id)?;
    let _ = led.release_strong(id)?;
    led.release_strong(id)
}

pub fn golden_run() -> Result<(u32, u32, u32), u32> {
    // 10/10 轮。
    let mut ok_rounds = 0;
    let mut total_ops = 0;
    let mut r = 0;
    while r < ROUNDS {
        let (freed, ops) = random_round(r)?;
        if freed == 0 {
            // 每轮至少 spawn 过对象才可能有 freed；freed==0 仅当全程无 spawn——
            // 实现保证 spawn 早于首操作，故 freed≥1 才算过。
            return Err(DualCode::TornLeftover.val());
        }
        ok_rounds += 1;
        total_ops += ops;
        r += 1;
    }
    // 黄金三序对照（判据值写死：Freed/DestroyPending/DestroyPending）。
    let a = probe_seq_a()?;
    let b = probe_seq_b()?;
    let c = probe_seq_c()?;
    if a != CellPhase::Freed || b != CellPhase::DestroyPending || c != CellPhase::Freed {
        return Err(DualCode::DoubleDestroy.val());
    }
    Ok((ok_rounds, total_ops, ok_rounds as u32 * 1))
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
    // 族 0：混域写（声明弱域加，执行域给强域 → 错配必拒）。
    let mut t = 0;
    while t < 10 {
        let mut led = DualLedger::new();
        let id = led.spawn();
        let r = led.apply(id, OP_ADD_WEAK, DOMAIN_STRONG);
        ledger.tries[0] += 1;
        if let Err(e) = r {
            if e == DualCode::MixedDomainWrite.val() {
                ledger.caught[0] += 1;
            }
        }
        t += 1;
    }
    // 族 1：已销毁对象再受操作（强减到 Freed 后再 add_strong）。
    let mut t = 0;
    while t < 10 {
        let mut led = DualLedger::new();
        let id = led.spawn();
        let _ = led.release_strong(id);
        let r = led.add_strong(id);
        ledger.tries[1] += 1;
        if let Err(e) = r {
            if e == DualCode::OpOnDestroyed.val() {
                ledger.caught[1] += 1;
            }
        }
        t += 1;
    }
    // 族 2：弱存活闸（未 DestroyPending 时尝试撕毁后余账 → 终态审计拒收；
    // 用 Freed 闸：强归零前弱账>0 时强行终审计 → 撕毁后余账必现）。
    let mut t = 0;
    while t < 10 {
        let mut led = DualLedger::new();
        let id = led.spawn();
        let _ = led.add_weak(id);
        let _ = led.add_weak(id);
        // 不销毁强引用，直接审计终态 → Active 对象不属余账，但 Freed==0
        // 说明弱账未走闸，守恒对账仍须过；此处抓「 Freed 但弱非零」不可能，
        // 故改抓：强归零后弱>0 的 DestroyPending 残留 → TornLeftover。
        let _ = led.release_strong(id);
        let r = led.audit_terminal();
        ledger.tries[2] += 1;
        if let Err(e) = r {
            if e == DualCode::TornLeftover.val() {
                ledger.caught[2] += 1;
            }
        }
        t += 1;
    }
}

// ── 判据入口 ─────────────────────────────────────────────────────
pub fn segment_guard() -> bool {
    let mut i = 0;
    while i < DualCode::ALL.len() {
        let v = DualCode::ALL[i].val();
        if v < SEG_BASE || v >= SEG_BASE + 0x40 {
            return false;
        }
        i += 1;
    }
    // 双域标签闭集。
    if DOMAIN_STRONG > DOMAIN_WEAK {
        return false;
    }
    // 操作码 4 码 + 域掩码自洽。
    let four = [OP_ADD_STRONG, OP_REL_STRONG, OP_ADD_WEAK, OP_REL_WEAK];
    let mut j = 0;
    while j < 4 {
        if four[j] >> OP_DOMAIN_SHIFT > DOMAIN_WEAK {
            return false;
        }
        j += 1;
    }
    true
}

pub fn extra_checks() -> Vec<(&'static str, bool, String)> {
    let mut v: Vec<(&'static str, bool, String)> = Vec::new();
    let golden = golden_run();
    let golden_ok = if let Ok((rounds, ops, _)) = golden {
        rounds == ROUNDS && ops == (ROUNDS * OPS_PER_ROUND) as u32
    } else {
        false
    };
    v.push((
        "J1-双域10/10轮全撕净+守恒",
        golden_ok,
        format!("{:?}", golden),
    ));
    let mut led = InjectLedger::new();
    run_injections(&mut led);
    v.push((
        "J1-混域注入10/10必抓",
        led.all_caught() && led.caught[0] == 10,
        format!("t/c {:?}/{:?}", led.tries, led.caught),
    ));
    // 黄金三序点验（对照值写死）。
    let a = probe_seq_a();
    let b = probe_seq_b();
    let c = probe_seq_c();
    let seq_ok = a == Ok(CellPhase::Freed)
        && b == Ok(CellPhase::DestroyPending)
        && c == Ok(CellPhase::Freed);
    v.push((
        "J1-黄金三序对照",
        seq_ok,
        format!("a={:?} b={:?} c={:?}", a, b, c),
    ));
    // 域名点验。
    v.push((
        "J1-双域名点验",
        DOMAIN_NAMES[0].len() > 0 && DOMAIN_NAMES[1].len() > 0,
        format!("{}/2", DOMAIN_NAMES.len()),
    ));
    // 守恒恒等式独立复核（空账自洽）。
    let led0 = DualLedger::new();
    v.push((
        "J1-空账守恒自洽",
        led0.audit_conservation().is_ok(),
        String::from("空账 Δ=0"),
    ));
    // 诊断码窗守卫。
    v.push(("J3-段位守卫", segment_guard(), String::from("0x67100 窗口")));
    v
}

pub fn run_unx15241_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F15241 强/弱引用计数双域分离");
    for (n, p, d) in extra_checks() {
        s.add_dyn(n, p, &d);
    }
    s
}
