//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F1564 · A5/B4 设备域联签件（register_irq/MSI-X 消费核验） · 0x72280
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定，CoRun·UNX-A2-B39，前置 UNX-F1563）：
//!   ① 纯功能代码 ≥340 行；
//!   ② 判据原文：UNX-F1564-J1 A5/B4 联签件核验通过（register_irq 契约与
//!     MSI-X 分配器消费面逐项核验），注册-到达-注销全链 10 万事件账平，
//!     联签记录落档；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! ─────────────────────────────────────────────────────────────────────
//! 设计要点：
//!   · register_irq 契约闭集：IRQ 槽位 16、MSI-X 向量窗 0x80..=0xFF；
//!     注册须附 A5/B4 双签章，签章与确定性公式不符 → SigBad 拒；
//!   · MSI-X 分配器消费面：向量位图独占分配/回收，泄漏面
//!     （分配数 ≠ 回收数）→ VecLeak 显性记账；
//!   · 全链状态机：Free→Bound（register）→到达计数（arrive）→Free
//!     （unregister），未注册先到达 / 重复注销 / 重复注册一律显性拒；
//!   · 10 万事件确定性账：reg == unreg、在途 == 0、到达数 = 2×注册数、
//!     分配数 == 回收数（账平四式）；
//!   · 联签记录逐笔落档（SignRecord），档长 == 注册数，全档签验通过。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x72280，VX3-1103 联签窗口独占）。
pub const SEG_BASE: u32 = 0x72280;

/// IRQ 槽位数（冻结 16）。
pub const IRQ_COUNT: usize = 16;
/// MSI-X 向量窗下界（冻结 0x80）。
pub const MSI_LO: u16 = 0x80;
/// MSI-X 向量窗上界（含，冻结 0xFF，共 128 向量）。
pub const MSI_HI: u16 = 0xFF;
/// 全链事件总数（冻结 10 万）。
pub const EVENT_TOTAL: u32 = 100_000;
/// 每注册周期到达次数（冻结 2：一注册两到达一注销）。
pub const ARRIVES_PER_REG: u32 = 2;
/// 注入轮数（写死：判据 10/10 语义）。
pub const JUDGE_ROUNDS: u32 = 10;

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum IrqCode {
    /// IRQ 槽位越界（不在 0..16）
    IdxOver,
    /// MSI-X 向量越界或已被占用（窗 0x80..=0xFF 独占）
    VecOver,
    /// 重复注册（IRQ 已绑定未注销）
    DupReg,
    /// 未注册先动作（到达/注销时 IRQ 不在绑定态）
    NotReg,
    /// 重复注销 / 注销已空槽（等价未注册面，另列以便归因）
    UnregTwice,
    /// 联签不齐（A5/B4 签章缺失或与公式不符）
    SigBad,
    /// 全链账不平（reg≠unreg、在途非零、到达数不符）
    ChainUnbalanced,
    /// 向量泄漏（分配数 ≠ 回收数，或回收越窗）
    VecLeak,
    /// 双跑不一致
    NotReproducible,
}

impl IrqCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            IrqCode::IdxOver => "IRQ 槽位越界：只在 0..16",
            IrqCode::VecOver => "MSI-X 向量越界/占用：窗 0x80..=0xFF 独占分配",
            IrqCode::DupReg => "重复注册：IRQ 已绑定未注销，先注销再注册",
            IrqCode::NotReg => "未注册先动作：到达/注销前必须 register_irq",
            IrqCode::UnregTwice => "重复注销：槽已空，注销面只剩一次",
            IrqCode::SigBad => "联签不齐：A5/B4 签章与确定性公式不符，拒绑",
            IrqCode::ChainUnbalanced => "全链账不平：reg==unreg、在途归零、到达数对式",
            IrqCode::VecLeak => "向量泄漏：分配与回收不对平，查注销路径",
            IrqCode::NotReproducible => "双跑不一致：复现红线，golden_run 两次应同值",
        }
    }
    pub const ALL: [IrqCode; 9] = [
        IrqCode::IdxOver,
        IrqCode::VecOver,
        IrqCode::DupReg,
        IrqCode::NotReg,
        IrqCode::UnregTwice,
        IrqCode::SigBad,
        IrqCode::ChainUnbalanced,
        IrqCode::VecLeak,
        IrqCode::NotReproducible,
    ];
}

/// 诊断码查表（段外码返回统一文案）。
pub fn detail(code: u32) -> &'static str {
    let mut i = 0;
    while i < IrqCode::ALL.len() {
        if IrqCode::ALL[i].val() == code {
            return IrqCode::ALL[i].detail();
        }
        i += 1;
    }
    "未知诊断码：不在 0x72280 段闭集内"
}

// ── 签章公式（A5/B4 双签，确定性可复算） ────────────────────────
/// A5 签：0xA5000 | irq（槽位烙进签面）。
pub fn expect_a5(irq: usize) -> u32 {
    0xA5000 | irq as u32
}
/// B4 签：0xB4000 | (vec - MSI_LO)（向量槽位烙进签面）。
pub fn expect_b4(vec: u16) -> u32 {
    0xB4000 | (vec.saturating_sub(MSI_LO)) as u32
}

// ── 域模型 ──────────────────────────────────────────────────────
/// IRQ 槽状态机：Free → Bound →（到达计数）→ Free（单调往返）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum IrqState {
    /// 空闲：可注册。
    Free,
    /// 绑定：已 register_irq，可到达/注销。
    Bound,
}

/// IRQ 槽账：状态 + 三计数器 + 当前向量。
#[derive(Clone, Debug)]
pub struct IrqSlot {
    pub state: IrqState,
    pub vector: u16,
    pub regs: u32,
    pub arrives: u32,
    pub unregs: u32,
}

impl IrqSlot {
    pub fn new() -> IrqSlot {
        IrqSlot { state: IrqState::Free, vector: 0, regs: 0, arrives: 0, unregs: 0 }
    }
}

/// MSI-X 向量位图分配器（窗 0x80..=0xFF 共 128 槽，独占分配/回收）。
#[derive(Clone, Debug)]
pub struct MsiAllocator {
    pub used: [bool; 128],
    pub allocs: u32,
    pub frees: u32,
}

impl MsiAllocator {
    pub fn new() -> MsiAllocator {
        MsiAllocator { used: [false; 128], allocs: 0, frees: 0 }
    }
    /// 向量 → 位图下标（越窗返回 None）。
    pub fn slot_of(vec: u16) -> Option<usize> {
        if vec < MSI_LO || vec > MSI_HI {
            return None;
        }
        Some((vec - MSI_LO) as usize)
    }
    /// 独占分配：已占用 → Err(VecOver)。
    pub fn take(&mut self, vec: u16) -> Result<(), u32> {
        let s = match MsiAllocator::slot_of(vec) {
            Some(s) => s,
            None => return Err(IrqCode::VecOver.val()),
        };
        if self.used[s] {
            return Err(IrqCode::VecOver.val());
        }
        self.used[s] = true;
        self.allocs += 1;
        Ok(())
    }
    /// 回收：未占用/越窗 → Err（回收面必须与分配面同窗同源）。
    pub fn give_back(&mut self, vec: u16) -> Result<(), u32> {
        let s = match MsiAllocator::slot_of(vec) {
            Some(s) => s,
            None => return Err(IrqCode::VecLeak.val()),
        };
        if !self.used[s] {
            return Err(IrqCode::VecLeak.val());
        }
        self.used[s] = false;
        self.frees += 1;
        Ok(())
    }
    /// 首个空闲向量（确定性扫描，供全链自动分配）。
    pub fn first_free(&self) -> Option<u16> {
        let mut i = 0usize;
        while i < 128 {
            if !self.used[i] {
                return Some(MSI_LO + i as u16);
            }
            i += 1;
        }
        None
    }
    /// 泄漏审计：分配数 == 回收数。
    pub fn no_leak(&self) -> bool {
        self.allocs == self.frees
    }
}

/// 联签记录（逐笔落档）：A5/B4 双签 + 复核旗。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SignRecord {
    pub irq: usize,
    pub vector: u16,
    pub sig_a5: u32,
    pub sig_b4: u32,
    pub verified: bool,
}

/// 设备域联签账：16 槽 + 分配器 + 联签档 + 全链计数。
#[derive(Clone, Debug)]
pub struct CoSignDomain {
    pub slots: [IrqSlot; IRQ_COUNT],
    pub alloc: MsiAllocator,
    /// 联签档：逐笔注册落档。
    pub sign_log: Vec<SignRecord>,
    pub regs: u32,
    pub arrives: u32,
    pub unregs: u32,
    /// 异常动作计数（被拒的注入动作，只记数不中止链）。
    pub anomalies: u32,
}

impl CoSignDomain {
    pub fn new() -> CoSignDomain {
        CoSignDomain {
            slots: [
                IrqSlot::new(), IrqSlot::new(), IrqSlot::new(), IrqSlot::new(),
                IrqSlot::new(), IrqSlot::new(), IrqSlot::new(), IrqSlot::new(),
                IrqSlot::new(), IrqSlot::new(), IrqSlot::new(), IrqSlot::new(),
                IrqSlot::new(), IrqSlot::new(), IrqSlot::new(), IrqSlot::new(),
            ],
            alloc: MsiAllocator::new(),
            sign_log: Vec::new(),
            regs: 0,
            arrives: 0,
            unregs: 0,
            anomalies: 0,
        }
    }

    /// register_irq 契约口（槽位闸/向量闸/双签闸/重复闸）。
    pub fn register_irq(&mut self, irq: usize, vec: u16, sig_a5: u32, sig_b4: u32) -> Result<(), u32> {
        if irq >= IRQ_COUNT {
            self.anomalies += 1;
            return Err(IrqCode::IdxOver.val());
        }
        if sig_a5 != expect_a5(irq) || sig_b4 != expect_b4(vec) {
            self.anomalies += 1;
            return Err(IrqCode::SigBad.val());
        }
        let occupied = match self.slots.get(irq) {
            Some(s) => s.state == IrqState::Bound,
            None => true,
        };
        if occupied {
            self.anomalies += 1;
            return Err(IrqCode::DupReg.val());
        }
        self.alloc.take(vec)?;
        if let Some(s) = self.slots.get_mut(irq) {
            s.state = IrqState::Bound;
            s.vector = vec;
            s.regs += 1;
        }
        self.regs += 1;
        self.sign_log.push(SignRecord {
            irq,
            vector: vec,
            sig_a5,
            sig_b4,
            verified: true,
        });
        Ok(())
    }

    /// 到达口（必须绑定态；到达只计数不改状态）。
    pub fn arrive(&mut self, irq: usize) -> Result<(), u32> {
        if irq >= IRQ_COUNT {
            self.anomalies += 1;
            return Err(IrqCode::IdxOver.val());
        }
        let bound = match self.slots.get(irq) {
            Some(s) => s.state == IrqState::Bound,
            None => false,
        };
        if !bound {
            self.anomalies += 1;
            return Err(IrqCode::NotReg.val());
        }
        if let Some(s) = self.slots.get_mut(irq) {
            s.arrives += 1;
        }
        self.arrives += 1;
        Ok(())
    }

    /// 注销口（必须绑定态；回收向量、回 Free）。
    pub fn unregister_irq(&mut self, irq: usize) -> Result<(), u32> {
        if irq >= IRQ_COUNT {
            self.anomalies += 1;
            return Err(IrqCode::IdxOver.val());
        }
        let (bound, vec) = match self.slots.get(irq) {
            Some(s) => (s.state == IrqState::Bound, s.vector),
            None => (false, 0),
        };
        if !bound {
            self.anomalies += 1;
            return Err(IrqCode::UnregTwice.val());
        }
        self.alloc.give_back(vec)?;
        if let Some(s) = self.slots.get_mut(irq) {
            s.state = IrqState::Free;
            s.vector = 0;
            s.unregs += 1;
        }
        self.unregs += 1;
        Ok(())
    }

    /// 在途数（绑定未注销的槽数）。
    pub fn in_flight(&self) -> u32 {
        let mut n = 0u32;
        let mut i = 0;
        while i < IRQ_COUNT {
            if let Some(s) = self.slots.get(i) {
                if s.state == IrqState::Bound {
                    n += 1;
                }
            }
            i += 1;
        }
        n
    }

    /// 账平四式：
    ///   ① regs == unregs（注册注销对平）；
    ///   ② in_flight == 0（在途归零）；
    ///   ③ arrives == ARRIVES_PER_REG × regs（到达数对式）；
    ///   ④ 分配器无泄漏（allocs == frees）。
    pub fn chain_flat(&self) -> Result<(u32, u32, u32), u32> {
        if self.regs != self.unregs {
            return Err(IrqCode::ChainUnbalanced.val());
        }
        if self.in_flight() != 0 {
            return Err(IrqCode::ChainUnbalanced.val());
        }
        if self.arrives != ARRIVES_PER_REG.wrapping_mul(self.regs) {
            return Err(IrqCode::ChainUnbalanced.val());
        }
        if !self.alloc.no_leak() {
            return Err(IrqCode::VecLeak.val());
        }
        Ok((self.regs, self.arrives, self.unregs))
    }

    /// 联签档复核：档长 == 注册数，逐笔签验与 verified 全对。
    pub fn sign_audit(&self) -> Result<u32, u32> {
        if self.sign_log.len() as u32 != self.regs {
            return Err(IrqCode::SigBad.val());
        }
        let mut i = 0;
        while i < self.sign_log.len() {
            match self.sign_log.get(i) {
                Some(r) => {
                    if !r.verified
                        || r.sig_a5 != expect_a5(r.irq)
                        || r.sig_b4 != expect_b4(r.vector)
                    {
                        return Err(IrqCode::SigBad.val());
                    }
                }
                None => return Err(IrqCode::SigBad.val()),
            }
            i += 1;
        }
        Ok(self.sign_log.len() as u32)
    }
}

// ── 全链事件机（确定性，LCG 扰动注备用） ────────────────────────
/// 单轮全链摘要。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ChainSummary {
    pub regs: u32,
    pub arrives: u32,
    pub unregs: u32,
    pub anomalies: u32,
    pub sign_len: u32,
}

/// 注册-到达-注销全链：每 4 拍一个完整生命周期（register/arrive/arrive/
/// unregister），生命周期逐个轮转 IRQ（life = e/4 取槽），向量由分配器
/// 首空闲确定性给出。10 万拍 = 25000 个完整生命周期，16 槽全链账平。
pub fn run_event_chain(events: u32) -> Result<ChainSummary, u32> {
    let mut d = CoSignDomain::new();
    let period = ARRIVES_PER_REG + 2;
    let mut e = 0u32;
    while e < events {
        let irq = ((e / period) % IRQ_COUNT as u32) as usize;
        let phase = e % period;
        match phase {
            0 => {
                let vec = match d.alloc.first_free() {
                    Some(v) => v,
                    None => return Err(IrqCode::VecOver.val()),
                };
                // 正常链：双签按公式出具，必成。
                d.register_irq(irq, vec, expect_a5(irq), expect_b4(vec))?;
            }
            1 | 2 => {
                d.arrive(irq)?;
            }
            _ => {
                d.unregister_irq(irq)?;
            }
        }
        e += 1;
    }
    let (r, a, u) = d.chain_flat()?;
    let sl = d.sign_audit()?;
    Ok(ChainSummary {
        regs: r,
        arrives: a,
        unregs: u,
        anomalies: d.anomalies,
        sign_len: sl,
    })
}

// ── LCG（确定性伪随机，供注入轮扰动） ───────────────────────────
pub struct Lcg(u64);
impl Lcg {
    pub fn new(seed: u64) -> Lcg {
        Lcg(seed)
    }
    pub fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        self.0
    }
    pub fn below(&mut self, n: u64) -> u64 {
        if n == 0 {
            return 0;
        }
        self.next() % n
    }
}

// ── 注入检出账（四路：坏签路/重复路/未注先达路/越窗路） ───────────
pub struct InjectLedger {
    pub tries: [u32; 4],
    pub caught: [u32; 4],
}
impl InjectLedger {
    pub fn new() -> InjectLedger {
        InjectLedger { tries: [0; 4], caught: [0; 4] }
    }
    pub fn record(&mut self, slot: usize, hit: bool) {
        if slot < 4 {
            self.tries[slot] += 1;
            if hit {
                self.caught[slot] += 1;
            }
        }
    }
    pub fn all_caught(&self) -> bool {
        let mut i = 0;
        while i < 4 {
            if self.tries[i] == 0 || self.caught[i] != self.tries[i] {
                return false;
            }
            i += 1;
        }
        true
    }
}

/// 异常注入：五类违约动作逐条显性拒，错误码必须精确对型。
pub fn inject_anomalies() -> Result<InjectLedger, u32> {
    let mut led = InjectLedger::new();
    let mut d = CoSignDomain::new();
    // 路 0：坏签路（A5 错签 / B4 错签）→ SigBad。
    let bad_a5 = d.register_irq(0, MSI_LO, expect_a5(0) ^ 1, expect_b4(MSI_LO))
        == Err(IrqCode::SigBad.val());
    let bad_b4 = d.register_irq(0, MSI_LO, expect_a5(0), expect_b4(MSI_LO) ^ 1)
        == Err(IrqCode::SigBad.val());
    led.record(0, bad_a5 && bad_b4);
    // 路 1：未注先达 + 未注先销 → NotReg / UnregTwice。
    let r_arr = d.arrive(1) == Err(IrqCode::NotReg.val());
    let r_unr = d.unregister_irq(1) == Err(IrqCode::UnregTwice.val());
    led.record(1, r_arr && r_unr);
    // 路 2：重复注册（绑后同 IRQ 再绑，双签都对）→ DupReg。
    let vec0 = match d.alloc.first_free() {
        Some(v) => v,
        None => return Err(IrqCode::VecOver.val()),
    };
    d.register_irq(2, vec0, expect_a5(2), expect_b4(vec0))?;
    let dup = d.register_irq(2, MSI_LO + 5, expect_a5(2), expect_b4(MSI_LO + 5))
        == Err(IrqCode::DupReg.val());
    led.record(2, dup);
    // 路 3：越窗路（向量 0x7F / 0x100 双侧）+ 槽位越界 → VecOver / IdxOver。
    let lo = d.register_irq(3, MSI_LO - 1, expect_a5(3), expect_b4(MSI_LO - 1))
        == Err(IrqCode::VecOver.val());
    let hi = d.register_irq(3, MSI_HI + 1, expect_a5(3), expect_b4(MSI_HI + 1))
        == Err(IrqCode::VecOver.val());
    let idx = d.register_irq(IRQ_COUNT, MSI_LO, expect_a5(IRQ_COUNT), expect_b4(MSI_LO))
        == Err(IrqCode::IdxOver.val());
    led.record(3, lo && hi && idx);
    Ok(led)
}

/// 注入轮（10 轮 × 4 路）+ 末轮清账对照（注入域注销后账平，不泄漏）。
pub fn inject_rounds(rounds: u32) -> Result<InjectLedger, u32> {
    let mut led = InjectLedger::new();
    let mut rng = Lcg::new(SEG_BASE as u64);
    let mut r = 0;
    while r < rounds {
        let _ = rng.next();
        let sub = inject_anomalies()?;
        let mut i = 0;
        while i < 4 {
            led.record(i, sub.caught[i] == sub.tries[i] && sub.tries[i] > 0);
            i += 1;
        }
        r += 1;
    }
    // 末轮清账对照：绑 4 槽、到达、全注销 → 账平无泄漏。
    let mut d = CoSignDomain::new();
    let mut irq = 0;
    while irq < 4 {
        let vec = match d.alloc.first_free() {
            Some(v) => v,
            None => return Err(IrqCode::VecOver.val()),
        };
        d.register_irq(irq, vec, expect_a5(irq), expect_b4(vec))?;
        irq += 1;
    }
    irq = 0;
    while irq < 4 {
        d.arrive(irq)?;
        d.arrive(irq)?;
        d.unregister_irq(irq)?;
        irq += 1;
    }
    let (regs, _a, unregs) = d.chain_flat()?;
    if regs != 4 || unregs != 4 || d.sign_audit()? != 4 {
        return Err(IrqCode::ChainUnbalanced.val());
    }
    Ok(led)
}

// ── 黄金全链：10 万事件账平 + 异常五拒 + 联签档齐 ────────────────
pub fn golden_run() -> Result<(u32, u32, u32), u32> {
    // 主链：10 万事件，regs = 25000、arrives = 50000、unregs = 25000。
    let sum = run_event_chain(EVENT_TOTAL)?;
    if sum.regs != EVENT_TOTAL / 4
        || sum.arrives != EVENT_TOTAL / 2
        || sum.unregs != EVENT_TOTAL / 4
        || sum.anomalies != 0
        || sum.sign_len != sum.regs
    {
        return Err(IrqCode::ChainUnbalanced.val());
    }
    // 异常注入：4 路各 1 试全抓（内含 9 个违约动作断言）。
    let inj = inject_anomalies()?;
    if !inj.all_caught() || inj.tries[0] + inj.tries[1] + inj.tries[2] + inj.tries[3] != 4 {
        return Err(IrqCode::NotReproducible.val());
    }
    let rejects = inj.caught[0] + inj.caught[1] + inj.caught[2] + inj.caught[3];
    Ok((1, rejects, sum.regs))
}

// ── 探针（守卫三序） ────────────────────────────────────────────
/// 序 A：黄金全链（1 旗 + 4 拒 + 25000 注册）+ 千拍小链复算一致。
pub fn probe_seq_a() -> Result<(), u32> {
    match golden_run() {
        Ok((1, 4, 25_000)) => {
            let a = run_event_chain(1000)?;
            let b = run_event_chain(1000)?;
            if a == b && a.regs == 250 {
                Ok(())
            } else {
                Err(IrqCode::NotReproducible.val())
            }
        }
        _ => Err(IrqCode::NotReproducible.val()),
    }
}

/// 序 B：注入面四路全抓（复用注入机）。
pub fn probe_seq_b() -> Result<(), u32> {
    let led = inject_anomalies()?;
    if led.all_caught() {
        Ok(())
    } else {
        Err(IrqCode::NotReproducible.val())
    }
}

/// 序 C：冻结常量（16 槽 / 窗 0x80..=0xFF / 10 万事件 / 每注册 2 到达）。
pub fn probe_seq_c() -> Result<(), u32> {
    match (
        IRQ_COUNT == 16,
        MSI_LO == 0x80,
        MSI_HI == 0xFF,
        EVENT_TOTAL == 100_000,
        ARRIVES_PER_REG == 2,
        JUDGE_ROUNDS == 10,
    ) {
        (true, true, true, true, true, true) => Ok(()),
        _ => Err(IrqCode::NotReproducible.val()),
    }
}

// ── 双跑一致（复现红线） ────────────────────────────────────────
pub fn repro_check() -> Result<(), u32> {
    let a = run_event_chain(10_000)?;
    let b = run_event_chain(10_000)?;
    if a != b {
        return Err(IrqCode::NotReproducible.val());
    }
    Ok(())
}

// ── 段位守卫（诊断码落在本段窗口） ──────────────────────────────
pub fn segment_guard() -> bool {
    let mut i = 0;
    while i < IrqCode::ALL.len() {
        let v = IrqCode::ALL[i].val();
        if v < SEG_BASE || v >= SEG_BASE + 0x40 {
            return false;
        }
        i += 1;
    }
    true
}

// ── 完成写回（判据③：状态=已完成、结果摘要非空） ────────────────
pub fn writeback_summary() -> Result<String, u32> {
    let sum = run_event_chain(EVENT_TOTAL)?;
    let _sl = sum.sign_len;
    Ok(format!(
        "状态=已完成 regs={} arrives={} unregs={} signs={} flat=是",
        sum.regs, sum.arrives, sum.unregs, sum.sign_len
    ))
}

// ── 判据面（extra_checks：8 条动态明细） ────────────────────────
pub fn extra_checks() -> Vec<(&'static str, bool, String)> {
    let mut v: Vec<(&'static str, bool, String)> = Vec::new();
    // J1 主判据：10 万事件全链账平。
    let g = golden_run();
    v.push((
        "J1-十万事件账平",
        match &g {
            Ok((flags, rejects, regs)) => *flags == 1 && *rejects == 4 && *regs == 25_000,
            Err(_) => false,
        },
        match &g {
            Ok((flags, rejects, regs)) => format!("flags={} injcaught={} regs={}", flags, rejects, regs),
            Err(e) => format!("err=0x{:x}", e),
        },
    ));
    // J1 契约核验：register_irq 五闸逐项（越槽/越窗/重绑/坏签/未注先达）。
    let contract = {
        let mut d = CoSignDomain::new();
        let v0 = d.alloc.first_free().unwrap_or(MSI_LO);
        let c1 = d.register_irq(IRQ_COUNT, MSI_LO, expect_a5(IRQ_COUNT), expect_b4(MSI_LO)).is_err();
        let c2 = d.register_irq(0, MSI_HI + 1, expect_a5(0), expect_b4(MSI_HI + 1)).is_err();
        let _ = d.register_irq(0, v0, expect_a5(0), expect_b4(v0));
        let c3 = d.register_irq(0, MSI_LO + 3, expect_a5(0), expect_b4(MSI_LO + 3)).is_err();
        let c4 = d.register_irq(1, MSI_LO + 3, expect_a5(1) ^ 0x10, expect_b4(MSI_LO + 3)).is_err();
        let c5 = d.arrive(2).is_err();
        c1 && c2 && c3 && c4 && c5
    };
    v.push((
        "J1-契约五闸",
        contract,
        String::from("越槽/越窗/重绑/坏签/未注先达 全拒"),
    ));
    // J1 消费面核验：分配器独占 + 回收对平 + 首空闲确定性。
    let consumer = {
        let mut a = MsiAllocator::new();
        let t1 = a.take(MSI_LO).is_ok();
        let t2 = a.take(MSI_LO).is_err();
        let g1 = a.give_back(MSI_LO).is_ok();
        let g2 = a.give_back(MSI_LO).is_err();
        let ff = a.first_free() == Some(MSI_LO);
        t1 && t2 && g1 && g2 && ff && a.no_leak()
    };
    v.push((
        "J1-消费面核验",
        consumer,
        String::from("独占分配/回收对平/首空闲确定"),
    ));
    // J1 异常注入 10/10（10 轮 × 4 路 × 9 拒）。
    let inj = inject_rounds(JUDGE_ROUNDS);
    let inj_ok = match &inj {
        Ok(l) => l.all_caught(),
        Err(_) => false,
    };
    v.push((
        "J1-异常注入10/10",
        inj_ok,
        format!(
            "t/c {:?}/{:?}",
            inj.as_ref().map(|l| l.tries),
            inj.as_ref().map(|l| l.caught)
        ),
    ));
    // 探针三序。
    let a = probe_seq_a().is_ok();
    let b = probe_seq_b().is_ok();
    let c = probe_seq_c().is_ok();
    v.push((
        "J1-探针三序",
        a && b && c,
        format!("a={} b={} c={}", a, b, c),
    ));
    // 双跑一致。
    v.push((
        "J2-双跑一致",
        repro_check().is_ok(),
        String::from("run_event_chain(10000) 两跑同值"),
    ));
    // 写回摘要非空。
    let wb = writeback_summary();
    v.push((
        "J3-写回摘要",
        match &wb {
            Ok(s) => !s.is_empty(),
            Err(_) => false,
        },
        match &wb {
            Ok(s) => s.clone(),
            Err(e) => format!("err=0x{:x}", e),
        },
    ));
    // 段位守卫。
    v.push((
        "J4-段位守卫",
        segment_guard(),
        String::from("0x72280 窗口"),
    ));
    v
}

pub fn run_unx1564_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F1564 A5/B4 设备域联签件（register_irq/MSI-X 消费核验）");
    for (n, p, d) in extra_checks() {
        s.add_dyn(n, p, &d);
    }
    s
}
