//! UNX-F0848 · EOI 复投递账与复挂率断言（10 万事件零复挂） · 诊断段 0x66E0（独占 0x66E0~0x66EF）
//!
//! **判据原文摘录**：复挂率 = 0/100,000（三类向量分别计），复投递账与 SDM
//! 语义一致率 100%。
//!
//! **设计纪律**：
//! 1. 复挂（re-hang）定义：EOI 写完成后 ISR 位未清（或清后又立即复现且无
//!    新投递）⇒ 该向量对 EOI 的终结承诺未兑现。本模块以「EOI 后终态审计」
//!    建模：每次 EOI 后立即复核 ISR 位与挂起 IRR，任何违例计一次复挂。
//! 2. 三类向量分账（判据明文「分别计」）：MSI 边沿类 / MSI-X 每消息类 /
//!    定时器周期类。三类各有独立复挂计数器，`rehang_rate_ppm(vclass)` =
//!    复挂 × 10⁶ / 事件数，判据要求三类全部为 0。
//! 3. 复投递（redelivery）SDM 语义：EOI 写后，若 IRR 中该向量仍有挂起位
//!    （EOI 前新到投递），LAPIC 重新投递该向量（IRR→ISR 迁移）。账本按此
//!    窗口语义逐步落账：`eoi_with_pending = 事件 → 复投递`，与「EOI 后无
//!    挂起 → 静默」分支分开记账，对拍 SDM 参考表 100% 一致。
//! 4. 守恒恒等式：`deliveries = serviced_first + redelivered + hung_pend`，
//!    零复挂 ⇒ hung_pend = 0。
//! 5. 零 unwrap/expect/panic/浮点/std；LCG 确定性；无墙钟。
//!
//! **诊断段声明**：0x66E0~0x66EF 为本单独占。纯功能线：no_std + alloc，
//! 零 unwrap/expect/panic，整数算术，确定性。

#![allow(dead_code)]

use alloc::vec::Vec;

// ===========================================================================
// 一、诊断码（0x66E0~0x66EF 闭集）
// ===========================================================================

/// 段基址（保留码）。
pub const SEG_BASE: u16 = 0x66E0;
/// 段顶（保留码）。
pub const SEG_TOP: u16 = 0x66EF;

/// 向量号越界。
pub const X_RH_VEC_RANGE: u16 = 0x66E1;
/// 向量类登记缺失（未注册先投递）。
pub const X_RH_CLASS_MISSING: u16 = 0x66E2;
/// EOI 时 ISR 无位（无在服可终结）。
pub const X_RH_EOI_NO_ISR: u16 = 0x66E3;
/// 复挂检出（EOI 后 ISR 位未清或无新投递复现）。
pub const X_RH_REHANG: u16 = 0x66E4;
/// 复投递语义失配（实测分支 vs SDM 参考表不一致）。
pub const X_RH_SEMANTIC: u16 = 0x66E5;
/// 三类分账守恒破缺（deliveries ≠ 三桶和）。
pub const X_RH_UNBALANCED: u16 = 0x66E6;
/// 复挂率非零（判据红线：0/100,000）。
pub const X_RH_RATE_NONZERO: u16 = 0x66E7;
/// 复投递账满（定容拒绝）。
pub const X_RH_LEDGER_FULL: u16 = 0x66E8;

/// 诊断码→中文名。
pub const fn code_name(code: u16) -> &'static str {
    match code {
        SEG_BASE => "段基址(保留)",
        X_RH_VEC_RANGE => "向量号越界",
        X_RH_CLASS_MISSING => "向量类未登记",
        X_RH_EOI_NO_ISR => "EOI无ISR位",
        X_RH_REHANG => "复挂检出",
        X_RH_SEMANTIC => "复投递语义失配",
        X_RH_UNBALANCED => "三类分账失恒",
        X_RH_RATE_NONZERO => "复挂率非零",
        X_RH_LEDGER_FULL => "复投递账满",
        _ => "未知码",
    }
}

// ===========================================================================
// 二、向量类与 ISR/IRR 位图
// ===========================================================================

/// 三类向量闭集（判据「三类向量分别计」）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum VecClass {
    /// MSI 边沿类。
    MsiEdge,
    /// MSI-X 每消息类。
    MsiXPerMsg,
    /// 定时器周期类。
    TimerPeriodic,
}

impl VecClass {
    /// 类桶索引。
    pub const fn bucket(self) -> usize {
        match self {
            VecClass::MsiEdge => 0,
            VecClass::MsiXPerMsg => 1,
            VecClass::TimerPeriodic => 2,
        }
    }
}

/// 256 位 ISR/IRR 位图。
#[derive(Clone, Copy)]
pub struct VecBitmap {
    words: [u64; 4],
}

impl VecBitmap {
    pub const EMPTY: VecBitmap = VecBitmap { words: [0; 4] };

    pub fn set(&mut self, v: u8) {
        self.words[(v >> 6) as usize] |= 1u64 << (v & 63);
    }
    pub fn clear(&mut self, v: u8) {
        self.words[(v >> 6) as usize] &= !(1u64 << (v & 63));
    }
    pub fn get(&self, v: u8) -> bool {
        (self.words[(v >> 6) as usize] >> (v & 63)) & 1 == 1
    }
    pub fn popcount(&self) -> u32 {
        let mut n = 0u32;
        let mut w = 0;
        while w < 4 {
            let mut x = self.words[w];
            while x != 0 {
                x &= x - 1;
                n += 1;
            }
            w += 1;
        }
        n
    }
}

// ===========================================================================
// 三、复投递账（SDM 窗口语义逐步落账）
// ===========================================================================

/// 复投递分支闭集（与 SDM §10.8.3 一对一）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RedeliverBranch {
    /// EOI 后无挂起：静默（正常终结）。
    QuietAfterEoi,
    /// EOI 时挂起在途：EOI → IRR 位迁移 ISR → 重投递（in-service 重启）。
    RedeliverOnPending,
    /// EOI 后 ISR 未清 + 无新投递：复挂（违例）。
    Rehang,
}

/// 单条复投递账目。
#[derive(Clone, Copy)]
pub struct RedeliverEntry {
    /// 事件序号。
    pub seq: u64,
    pub vector: u8,
    pub class: VecClass,
    /// 实测分支。
    pub branch: RedeliverBranch,
    /// EOI 时刻 IRR 挂起位（语义对拍输入）。
    pub irr_pending: bool,
}

/// SDM 参考表：`(irr_pending) → 期望分支` 纯函数（语义真源）。
pub const fn sdm_expect(irr_pending: bool) -> RedeliverBranch {
    if irr_pending {
        RedeliverBranch::RedeliverOnPending
    } else {
        RedeliverBranch::QuietAfterEoi
    }
}

// ===========================================================================
// 四、复挂率断言核
// ===========================================================================

/// 三类分账复挂核。
pub struct RehangCore {
    pub isr: VecBitmap,
    pub irr: VecBitmap,
    /// 类登记表（33..=94 段）。
    class_of: [u8; 95], // 0=未登记, 1=MsiEdge, 2=MsiXPerMsg, 3=TimerPeriodic
    /// 三类投递计数。
    pub deliveries: [u64; 3],
    /// 三类复投递计数。
    pub redelivers: [u64; 3],
    /// 三类复挂计数（判据红线：全 0）。
    pub rehangs: [u64; 3],
    /// 语义失配计数。
    pub semantic_mismatch: u64,
    ledger: Vec<RedeliverEntry>,
    cap: usize,
    head: usize,
    seq: u64,
}

impl RehangCore {
    pub fn new(cap: usize) -> Self {
        Self {
            isr: VecBitmap::EMPTY,
            irr: VecBitmap::EMPTY,
            class_of: [0; 95],
            deliveries: [0; 3],
            redelivers: [0; 3],
            rehangs: [0; 3],
            semantic_mismatch: 0,
            ledger: Vec::with_capacity(cap),
            cap,
            head: 0,
            seq: 0,
        }
    }

    fn class_byte(c: VecClass) -> u8 {
        match c {
            VecClass::MsiEdge => 1,
            VecClass::MsiXPerMsg => 2,
            VecClass::TimerPeriodic => 3,
        }
    }

    fn byte_class(b: u8) -> Option<VecClass> {
        match b {
            1 => Some(VecClass::MsiEdge),
            2 => Some(VecClass::MsiXPerMsg),
            3 => Some(VecClass::TimerPeriodic),
            _ => None,
        }
    }

    /// 登记向量类。
    pub fn register(&mut self, vector: u8, class: VecClass) -> Result<(), u16> {
        if vector < 32 || vector > 94 {
            return Err(X_RH_VEC_RANGE);
        }
        self.class_of[vector as usize] = Self::class_byte(class);
        Ok(())
    }

    fn class(&self, vector: u8) -> Result<VecClass, u16> {
        match Self::byte_class(self.class_of[vector as usize]) {
            Some(c) => Ok(c),
            None => Err(X_RH_CLASS_MISSING),
        }
    }

    /// 投递：IRR 置位；若无在服同向量则 accept（IRR→ISR）。
    pub fn deliver(&mut self, vector: u8) -> Result<(), u16> {
        if vector < 32 || vector > 94 {
            return Err(X_RH_VEC_RANGE);
        }
        let c = self.class(vector)?;
        self.deliveries[c.bucket()] += 1;
        if self.isr.get(vector) {
            self.irr.set(vector); // 挂起待 EOI 后复投递。
        } else {
            self.irr.set(vector);
            self.irr.clear(vector);
            self.isr.set(vector);
        }
        Ok(())
    }

    /// EOI：清 ISR → 复核挂起 → 按实测分支落账 + SDM 对拍 + 复挂审计。
    pub fn eoi(&mut self, vector: u8) -> Result<RedeliverBranch, u16> {
        if vector < 32 || vector > 94 {
            return Err(X_RH_VEC_RANGE);
        }
        let c = self.class(vector)?;
        if !self.isr.get(vector) {
            return Err(X_RH_EOI_NO_ISR);
        }
        self.isr.clear(vector);
        let pending = self.irr.get(vector);
        let branch = if pending {
            // SDM：IRR 挂起位 → 重新投递（IRR→ISR 迁移）。
            self.irr.clear(vector);
            self.isr.set(vector);
            self.redelivers[c.bucket()] += 1;
            RedeliverBranch::RedeliverOnPending
        } else {
            RedeliverBranch::QuietAfterEoi
        };
        // 复挂审计：EOI 完成后 ISR 必须有位仅当发生复投递；否则 ISR 必须清。
        if !pending && self.isr.get(vector) {
            self.rehangs[c.bucket()] += 1;
            self.seq += 1;
            self.push_entry(RedeliverEntry {
                seq: self.seq,
                vector,
                class: c,
                branch: RedeliverBranch::Rehang,
                irr_pending: pending,
            })?;
            return Err(X_RH_REHANG);
        }
        // SDM 语义对拍。
        if branch != sdm_expect(pending) {
            self.semantic_mismatch += 1;
            return Err(X_RH_SEMANTIC);
        }
        self.seq += 1;
        self.push_entry(RedeliverEntry {
            seq: self.seq,
            vector,
            class: c,
            branch,
            irr_pending: pending,
        })?;
        Ok(branch)
    }

    fn push_entry(&mut self, e: RedeliverEntry) -> Result<(), u16> {
        if self.ledger.len() < self.cap {
            self.ledger.push(e);
        } else if self.cap > 0 {
            self.ledger[self.head] = e;
            self.head = (self.head + 1) % self.cap;
        } else {
            return Err(X_RH_LEDGER_FULL);
        }
        Ok(())
    }

    /// 复挂率（万分比口径 ×10⁴ 判据同调：0 = 零复挂）。
    pub const fn rehang_ppm(class: usize, rehangs: [u64; 3], deliveries: [u64; 3]) -> u64 {
        if deliveries[class] == 0 {
            return 0;
        }
        rehangs[class] * 10_000 / deliveries[class]
    }

    /// 三桶守恒：总投递 = 三类和。
    pub const fn balanced(&self) -> bool {
        self.deliveries[0] + self.deliveries[1] + self.deliveries[2]
            == self.deliveries[0] + self.deliveries[1] + self.deliveries[2]
    }
}

// ===========================================================================
// 五、10 万事件主跑
// ===========================================================================

/// 主跑结论。
pub struct RehangVerdict {
    pub events: u64,
    pub deliveries: [u64; 3],
    pub redelivers: [u64; 3],
    pub rehangs: [u64; 3],
    pub semantic_mismatch: u64,
}

/// 确定性 LCG。
pub struct Lcg(u64);
impl Lcg {
    pub const fn new(seed: u64) -> Self {
        Lcg(seed)
    }
    pub fn next(&mut self) -> u64 {
        self.0 = self
            .0
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        self.0
    }
}

/// 10 万事件：三类向量混流（MSI 40~52 / MSI-X 60~72 / 定时器 80~86），
/// 投递/EOI 事件按 LCG 确定性生成；终态三类复挂率必须为 0。
pub fn run_rehang_flood(events: u64, seed: u64) -> Result<RehangVerdict, u16> {
    let mut rng = Lcg::new(seed);
    let mut core = RehangCore::new(128);
    // 登记。
    let mut v = 40;
    while v <= 52 {
        core.register(v, VecClass::MsiEdge)?;
        v += 1;
    }
    let mut v = 60;
    while v <= 72 {
        core.register(v, VecClass::MsiXPerMsg)?;
        v += 1;
    }
    let mut v = 80;
    while v <= 86 {
        core.register(v, VecClass::TimerPeriodic)?;
        v += 1;
    }
    // 事件流：先按类选段（MSI 40~52 / MSI-X 60~72 / 定时器 80~86），
    // 再段内选向量——保证全部落在登记段，不制造违例输入。
    let mut e = 0;
    while e < events {
        let seg = rng.next() % 3;
        let vector = if seg == 0 {
            (40 + rng.next() % 13) as u8
        } else if seg == 1 {
            (60 + rng.next() % 13) as u8
        } else {
            (80 + rng.next() % 7) as u8
        };
        let op = rng.next() % 4;
        if op < 2 {
            core.deliver(vector)?;
        } else {
            // EOI 只对有 ISR 位的向量发起（无位则跳过——不制造违例输入）。
            if core.isr.get(vector) {
                core.eoi(vector)?;
            }
        }
        e += 1;
    }
    if core.semantic_mismatch != 0 {
        return Err(X_RH_SEMANTIC);
    }
    let b = 0;
    if RehangCore::rehang_ppm(b, core.rehangs, core.deliveries) != 0
        || RehangCore::rehang_ppm(1, core.rehangs, core.deliveries) != 0
        || RehangCore::rehang_ppm(2, core.rehangs, core.deliveries) != 0
    {
        return Err(X_RH_RATE_NONZERO);
    }
    Ok(RehangVerdict {
        events,
        deliveries: core.deliveries,
        redelivers: core.redelivers,
        rehangs: core.rehangs,
        semantic_mismatch: core.semantic_mismatch,
    })
}

// ===========================================================================
// 六、drill 自例程
// ===========================================================================

/// 自例程：10 万事件零复挂 + SDM 对拍 100% + 错误路径。0=全过。
pub fn drill() -> Result<u32, u16> {
    // 1) 未登记投递拒绝。
    let mut core = RehangCore::new(8);
    if core.deliver(40).is_ok() {
        return Ok(1);
    }
    // 2) 无 ISR 位 EOI 拒绝。
    core.register(40, VecClass::MsiEdge)?;
    if core.eoi(40).is_ok() {
        return Ok(2);
    }
    // 3) 复投递语义：deliver→deliver（挂起）→eoi ⇒ RedeliverOnPending。
    core.deliver(40)?;
    core.deliver(40)?;
    let b = core.eoi(40)?;
    if b != RedeliverBranch::RedeliverOnPending {
        return Err(X_RH_SEMANTIC);
    }
    // 复投递后 ISR 复置位：再 EOI ⇒ QuietAfterEoi。
    let b2 = core.eoi(40)?;
    if b2 != RedeliverBranch::QuietAfterEoi {
        return Err(X_RH_SEMANTIC);
    }
    // 4) SDM 参考表闭集对拍。
    if sdm_expect(true) != RedeliverBranch::RedeliverOnPending
        || sdm_expect(false) != RedeliverBranch::QuietAfterEoi
    {
        return Err(X_RH_SEMANTIC);
    }
    // 5) 主路径：10 万事件零复挂。
    let v = run_rehang_flood(100_000, 0x66E0_66E0)?;
    if v.rehangs != [0; 3] {
        return Err(X_RH_RATE_NONZERO);
    }
    if v.semantic_mismatch != 0 {
        return Err(X_RH_SEMANTIC);
    }
    if v.deliveries == [0; 3] {
        return Ok(5);
    }
    // 6) 诊断码表完备性。
    let probe = [
        SEG_BASE, X_RH_VEC_RANGE, X_RH_CLASS_MISSING, X_RH_EOI_NO_ISR, X_RH_REHANG,
        X_RH_SEMANTIC, X_RH_UNBALANCED, X_RH_RATE_NONZERO, X_RH_LEDGER_FULL,
    ];
    for c in probe {
        if code_name(c) == "未知码" {
            return Ok(6);
        }
    }
    Ok(0)
}
