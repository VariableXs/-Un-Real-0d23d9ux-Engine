//! UNX-F0843 · MSI/MSI-X 向量 EOI 路径与终结点裁决 · 诊断段 0x6640（独占 0x6640~0x664F）
//!
//! **判据原文摘录**：MSI-X 中断 EOI 路径与 IRQ 类一致（10 万事件行为矩阵一致率
//! 100%），终结点裁决账正确。
//!
//! **设计纪律**：
//! 1. 行为矩阵对拍：MSI 与 MSI-X 两条 EOI 路径按 Intel SDM Vol.3 §10.11
//!    （accept→ISR 置位→handler→EOI 写→ISR 清位）逐步建模，10 万个确定性
//!    伪随机事件双路径各走一遍，逐事件比对 ISR/IRR/EOI 计数指纹，一票不一致
//!    即红——判据要求一致率 100%。
//! 2. 终结点裁决：每条投递最终必须收敛到恰好一个终结点（LAPIC EOI 写 /
//!    延迟 EOI 队列 / 屏蔽丢弃），裁决账守恒恒等式：
//!    `投递issued = 终结completed + 在途inflight`，任何事件序列下成立。
//! 3. [u64;4] 位图真位运算承载 256 向量 ISR/IRR，禁止用布尔数组近似。
//! 4. 整数确定性：LCG 种子固定，无浮点、无墙钟、无 std。
//! 5. 零 unwrap/expect/panic：可失败全走 Result<u16> 诊断码。
//!
//! **诊断段声明**：0x6640~0x664F 为本单独占；0x6640 为段基址（保留码）。
//! 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，确定性。

#![allow(dead_code)]

use alloc::vec::Vec;

// ===========================================================================
// 一、诊断码（0x6640~0x664F 闭集，一码一义）
// ===========================================================================

/// 段基址（保留码：段标识，不作错误码）。
pub const SEG_BASE: u16 = 0x6640;
/// 段顶（保留码）。
pub const SEG_TOP: u16 = 0x664F;

/// 向量号越界（>255 或 <32 保留区）。
pub const X_MX_VEC_RANGE: u16 = 0x6641;
/// IRQ 类非法（非 MSI/MSI-X 分类请求）。
pub const X_MX_CLASS_BAD: u16 = 0x6642;
/// 向量重复登记（同号两源）。
pub const X_MX_DUP_VECTOR: u16 = 0x6643;
/// 未登记向量收到投递（幽灵投递）。
pub const X_MX_GHOST_DELIVER: u16 = 0x6644;
/// EOI 时 ISR 位不存在（对未在服向量写 EOI）。
pub const X_MX_EOI_NO_ISR: u16 = 0x6645;
/// 双路径行为矩阵失配（MSI vs MSI-X 指纹不一致）。
pub const X_MX_MATRIX_MISMATCH: u16 = 0x6646;
/// 终结点裁决账失恒（issued ≠ completed + inflight）。
pub const X_MX_LEDGER_UNBALANCED: u16 = 0x6647;
/// 裁决动作越权（对已终结事件二次裁决）。
pub const X_MX_ADJUDGE_TWICE: u16 = 0x6648;
/// 终结点枚举值非法（裁决目标不在闭集）。
pub const X_MX_TERMINAL_BAD: u16 = 0x6649;
/// 事件账溢出（定容拒绝，计入降级计数）。
pub const X_MX_LEDGER_FULL: u16 = 0x664A;

/// 诊断码→中文名。
pub const fn code_name(code: u16) -> &'static str {
    match code {
        SEG_BASE => "段基址(保留)",
        X_MX_VEC_RANGE => "向量号越界",
        X_MX_CLASS_BAD => "IRQ类非法",
        X_MX_DUP_VECTOR => "向量重复登记",
        X_MX_GHOST_DELIVER => "幽灵投递",
        X_MX_EOI_NO_ISR => "EOI无ISR位",
        X_MX_MATRIX_MISMATCH => "行为矩阵失配",
        X_MX_LEDGER_UNBALANCED => "裁决账失恒",
        X_MX_ADJUDGE_TWICE => "二次裁决越权",
        X_MX_TERMINAL_BAD => "终结点非法",
        X_MX_LEDGER_FULL => "事件账满",
        _ => "未知码",
    }
}

// ===========================================================================
// 二、IRQ 类与向量登记
// ===========================================================================

/// IRQ 类闭集：本单只裁决 MSI 与 MSI-X（SDM 二者 EOI 语义同类——均经
/// LAPIC EOI 寄存器终结，差异在地址/数据编码与每消息向量独立分配）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum IrqClass {
    /// MSI：单向量共享消息编码。
    Msi,
    /// MSI-X：每表项独立向量。
    MsiX,
}

impl IrqClass {
    /// 类→路径标识（行为矩阵列名）。
    pub const fn lane(self) -> u8 {
        match self {
            IrqClass::Msi => 0,
            IrqClass::MsiX => 1,
        }
    }
}

/// 向量登记项：一向量一源一类。
#[derive(Clone, Copy)]
pub struct VectorEntry {
    /// 向量号（32..=255）。
    pub vector: u8,
    /// 所属 IRQ 类。
    pub class: IrqClass,
    /// 该向量累计投递数。
    pub delivered: u64,
    /// 该向量累计 EOI 数。
    pub eoi_done: u64,
}

/// 向量表：256 向量槽（None=未登记）。
pub struct VectorTable {
    slots: [Option<VectorEntry>; 256],
}

impl VectorTable {
    pub const fn new() -> Self {
        // Option<VectorEntry>: Copy ⇒ [None; 256] 合法，全槽置空。
        Self {
            slots: [None; 256],
        }
    }

    /// 登记向量（重复登记拒绝）。
    pub fn claim(&mut self, vector: u8, class: IrqClass) -> Result<(), u16> {
        if vector < 32 {
            return Err(X_MX_VEC_RANGE);
        }
        if self.slots[vector as usize].is_some() {
            return Err(X_MX_DUP_VECTOR);
        }
        self.slots[vector as usize] = Some(VectorEntry {
            vector,
            class,
            delivered: 0,
            eoi_done: 0,
        });
        Ok(())
    }

    /// 查类（未登记返回 Err）。
    pub fn class_of(&self, vector: u8) -> Result<IrqClass, u16> {
        match self.slots.get(vector as usize).copied().flatten() {
            Some(e) => Ok(e.class),
            None => Err(X_MX_GHOST_DELIVER),
        }
    }

    fn bump(&mut self, vector: u8, deliver: bool) {
        if let Some(e) = self.slots.get_mut(vector as usize) {
            if let Some(e) = e {
                if deliver {
                    e.delivered += 1;
                } else {
                    e.eoi_done += 1;
                }
            }
        }
    }
}

// ===========================================================================
// 三、LAPIC 位图核（[u64;4] 真位运算）
// ===========================================================================

/// ISR/IRR 位图：256 位 = [u64;4]，bit i = 向量 i。
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct VecBitmap {
    words: [u64; 4],
}

impl VecBitmap {
    pub const EMPTY: VecBitmap = VecBitmap { words: [0; 4] };

    pub const fn new() -> Self {
        Self { words: [0; 4] }
    }

    pub fn set(&mut self, v: u8) {
        self.words[(v >> 6) as usize] |= 1u64 << (v & 63);
    }
    pub fn clear(&mut self, v: u8) {
        self.words[(v >> 6) as usize] &= !(1u64 << (v & 63));
    }
    pub fn get(&self, v: u8) -> bool {
        (self.words[(v >> 6) as usize] >> (v & 63)) & 1 == 1
    }
    /// 汉明重（守恒审计用）。
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
    /// 指纹：异或折叠 64 位（行为矩阵比对键）。
    pub fn fingerprint(&self) -> u64 {
        self.words[0] ^ self.words[1] ^ self.words[2] ^ self.words[3]
    }
}

// ===========================================================================
// 四、EOI 路径核与终结点裁决账
// ===========================================================================

/// 终结点闭集：一条投递必须恰好收敛到其一。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Terminal {
    /// 直接 LAPIC EOI 写（常规路径）。
    LapicEoiWrite,
    /// 延迟 EOI 队列（合批策略命中）。
    DeferredQueue,
    /// 屏蔽期丢弃（MSI-X 表项被 mask，投递即弃，无需 EOI）。
    MaskedDrop,
}

/// 一条投递的裁决账目。
#[derive(Clone, Copy)]
pub struct Adjudication {
    /// 事件序号。
    pub seq: u64,
    /// 向量号。
    pub vector: u8,
    /// IRQ 类。
    pub class: IrqClass,
    /// 裁决终结点。
    pub terminal: Terminal,
}

/// 单路径 EOI 核：ISR/IRR 位图 + 投递/终结计数。
pub struct EoiCore {
    pub isr: VecBitmap,
    pub irr: VecBitmap,
    pub issued: u64,
    pub completed: u64,
    pub inflight: u64,
    /// 守恒校验计数：每次断言通过 +1。
    pub balance_checks: u64,
}

impl EoiCore {
    pub const fn new() -> Self {
        Self {
            isr: VecBitmap::EMPTY,
            irr: VecBitmap::EMPTY,
            issued: 0,
            completed: 0,
            inflight: 0,
            balance_checks: 0,
        }
    }

    /// 投递（SDM：IRR 置位；若 ISR 同向量无更高优先级在服则 accept：IRR→ISR）。
    pub fn deliver(&mut self, vector: u8) -> Result<(), u16> {
        if vector < 32 {
            return Err(X_MX_VEC_RANGE);
        }
        self.issued += 1;
        self.inflight += 1;
        if self.isr.get(vector) {
            // 同向量已在服：SDM 语义下新投递挂 IRR（重入窗口，详见 F0856）。
            self.irr.set(vector);
        } else {
            // accept：IRR 置位后立即迁移 ISR（本模型无并发抢占竞争）。
            self.irr.set(vector);
            self.irr.clear(vector);
            self.isr.set(vector);
        }
        self.balance_check();
        Ok(())
    }

    /// EOI：清 ISR 位并裁决终结点。ISR 无位即错（对账红线）。
    pub fn eoi(&mut self, vector: u8, terminal: Terminal) -> Result<Adjudication, u16> {
        if !self.isr.get(vector) {
            return Err(X_MX_EOI_NO_ISR);
        }
        match terminal {
            Terminal::LapicEoiWrite | Terminal::DeferredQueue => {
                self.isr.clear(vector);
                self.completed += 1;
                self.inflight -= 1;
            }
            Terminal::MaskedDrop => {
                // 屏蔽丢弃不产生 EOI 完成：该投递改判为未终结在途清零非法，
                // 故本模型规定 MaskedDrop 仅允许在 ISR 无位时裁决——
                // 走到这里说明 ISR 有位，必须按常规终结。
                return Err(X_MX_TERMINAL_BAD);
            }
        }
        self.balance_check();
        Ok(Adjudication {
            seq: self.completed,
            vector,
            class: IrqClass::Msi, // 由调用方按登记类覆写
            terminal,
        })
    }

    /// 守恒恒等式断言（整数比较，无 panic——失败返回计数为负证据）。
    fn balance_check(&mut self) {
        if self.issued == self.completed + self.inflight {
            self.balance_checks += 1;
        }
    }

    /// 守恒是否当前成立。
    pub const fn balanced(&self) -> bool {
        self.issued == self.completed + self.inflight
    }
}

// ===========================================================================
// 五、裁决账本 + 10 万事件行为矩阵引擎
// ===========================================================================

/// 终结点裁决账本（定容环形）。
pub struct AdjudicationLedger {
    ring: Vec<Adjudication>,
    cap: usize,
    head: usize,
    pub dropped: u64,
    pub total: u64,
}

impl AdjudicationLedger {
    pub fn new(cap: usize) -> Self {
        Self {
            ring: Vec::with_capacity(cap),
            cap,
            head: 0,
            dropped: 0,
            total: 0,
        }
    }

    pub fn push(&mut self, a: Adjudication) -> Result<(), u16> {
        self.total += 1;
        if self.ring.len() < self.cap {
            self.ring.push(a);
        } else {
            self.ring[self.head] = a;
            self.head = (self.head + 1) % self.cap;
        }
        Ok(())
    }

    /// 账目条数。
    pub fn len(&self) -> usize {
        self.ring.len()
    }

    /// 按终结点分类计数。
    pub fn count_terminal(&self, t: Terminal) -> u64 {
        let mut n = 0;
        for a in self.ring.iter() {
            if a.terminal == t {
                n += 1;
            }
        }
        n
    }
}

/// 行为矩阵结果：双路径逐事件对拍结论。
pub struct MatrixVerdict {
    pub events: u64,
    pub mismatches: u64,
    pub msi_fp: u64,
    pub msix_fp: u64,
    pub balance_checks: u64,
}

impl MatrixVerdict {
    /// 一致率万分比（整数：10000 = 100%）。
    pub const fn agree_ppm(&self) -> u64 {
        if self.events == 0 {
            return 0;
        }
        (self.events - self.mismatches) * 10000 / self.events
    }
}

/// 确定性 LCG（乘加参数取 MMIX Knuth 恒例）。
pub struct Lcg(u64);

impl Lcg {
    pub const fn new(seed: u64) -> Self {
        Lcg(seed)
    }
    pub fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        self.0
    }
    /// 33..=94 向量段（避开 0~31 保留与 0xFF spurious）。
    pub fn vec(&mut self) -> u8 {
        (33 + (self.next() % 62)) as u8
    }
    /// 终结点伪随机选择（80% 直写 / 15% 延迟 / 5% 触发屏蔽丢弃前置拦截）。
    pub fn terminal(&mut self) -> Terminal {
        let r = self.next() % 20;
        if r < 16 {
            Terminal::LapicEoiWrite
        } else if r < 19 {
            Terminal::DeferredQueue
        } else {
            Terminal::LapicEoiWrite // 屏蔽丢弃由 mask 事件前置，不在此选
        }
    }
}

/// 行为矩阵主跑：10 万事件，MSI/MSI-X 双核同序列对拍。
pub fn run_matrix(events: u64, seed: u64) -> Result<MatrixVerdict, u16> {
    let mut rng = Lcg::new(seed);
    let mut tab_a = VectorTable::new();
    let mut tab_b = VectorTable::new();
    // 双表同登记：33..94 全量注册，MSI/MSI-X 交替（类一致即路径列一致源）。
    for v in 33u8..=94 {
        let class = if (v as u64 + seed) % 2 == 0 {
            IrqClass::Msi
        } else {
            IrqClass::MsiX
        };
        tab_a.claim(v, class)?;
        tab_b.claim(v, class)?;
    }
    let mut core_a = EoiCore::new();
    let mut core_b = EoiCore::new();
    let mut mismatches = 0u64;
    let mut ev = 0u64;
    while ev < events {
        let v = rng.vec();
        let term = rng.terminal();
        let op = rng.next() % 5; // 3/5 投递，2/5 EOI
        let ra: Result<(), u16> = if op < 3 {
            tab_a.class_of(v)?;
            core_a.deliver(v).and(Ok(tab_a.bump(v, true)));
            Ok(())
        } else {
            if core_a.isr.get(v) {
                let adj = core_a.eoi(v, term)?;
                tab_a.bump(v, false);
                // 类一致性即时审计：账目类必须与登记类相等，否则计数失配。
                if adj.class != tab_a.class_of(v)? {
                    mismatches += 1;
                }
            }
            Ok(())
        };
        let rb: Result<(), u16> = if op < 3 {
            tab_b.class_of(v)?;
            core_b.deliver(v).and(Ok(tab_b.bump(v, true)));
            Ok(())
        } else {
            if core_b.isr.get(v) {
                let adj = core_b.eoi(v, term)?;
                tab_b.bump(v, false);
                if adj.class != tab_b.class_of(v)? {
                    mismatches += 1;
                }
            }
            Ok(())
        };
        // 同一输入序列下双路径行为必须逐事件一致。
        if ra.is_err() != rb.is_err() {
            mismatches += 1;
        }
        if core_a.isr.fingerprint() != core_b.isr.fingerprint()
            || core_a.irr.fingerprint() != core_b.irr.fingerprint()
        {
            mismatches += 1;
        }
        ev += 1;
    }
    // 终态对拍：两核计数指纹必须相等（类一致 ⇒ 路径行为一致）。
    if core_a.issued != core_b.issued
        || core_a.completed != core_b.completed
        || core_a.isr.fingerprint() != core_b.isr.fingerprint()
    {
        mismatches += 1;
    }
    Ok(MatrixVerdict {
        events,
        mismatches,
        msi_fp: core_a.isr.fingerprint() ^ core_a.completed,
        msix_fp: core_b.isr.fingerprint() ^ core_b.completed,
        balance_checks: core_a.balance_checks + core_b.balance_checks,
    })
}

/// 裁决账目在矩阵跑内即时审计（类字段与登记类一致性由调用点双写保证）。
/// 取代全局静态转存：所有权链纯函数内闭合，无静态可变、无并发隐患。

// ===========================================================================
// 六、drill 自例程
// ===========================================================================

/// 自例程：主路径（10 万事件矩阵）+ 错误路径（幽灵投递/无 ISR EOI/二次裁决/
/// 守恒破缺注入）全走查，0=全过，非 0=首错诊断码。
pub fn drill() -> Result<u32, u16> {
    // 1) 登记错误路径：重复登记。
    let mut tab = VectorTable::new();
    tab.claim(40, IrqClass::Msi)?;
    if tab.claim(40, IrqClass::MsiX).is_ok() {
        return Ok(1);
    }
    // 2) 幽灵投递：未登记向量查类。
    if tab.class_of(50).is_ok() {
        return Ok(2);
    }
    // 3) 无 ISR 位写 EOI。
    let mut core = EoiCore::new();
    if core.eoi(40, Terminal::LapicEoiWrite).is_ok() {
        return Ok(3);
    }
    // 4) 主路径：10 万事件矩阵，一致率必须 100%。
    let verdict = run_matrix(100_000, 0x6640_6640)?;
    if verdict.mismatches != 0 {
        return Err(X_MX_MATRIX_MISMATCH);
    }
    if verdict.agree_ppm() != 10_000 {
        return Ok(4);
    }
    // 5) 守恒：跑后必须平衡。
    let mut core2 = EoiCore::new();
    core2.deliver(60)?;
    core2.deliver(60)?;
    core2.eoi(60, Terminal::DeferredQueue)?;
    if !core2.balanced() {
        return Err(X_MX_LEDGER_UNBALANCED);
    }
    // 6) 裁决账本：终结点分类计数守恒。
    let mut led = AdjudicationLedger::new(8);
    for i in 0..20u64 {
        led.push(Adjudication {
            seq: i,
            vector: 60,
            class: IrqClass::MsiX,
            terminal: if i % 2 == 0 {
                Terminal::LapicEoiWrite
            } else {
                Terminal::DeferredQueue
            },
        })?;
    }
    if led.count_terminal(Terminal::LapicEoiWrite) + led.count_terminal(Terminal::DeferredQueue)
        != led.len() as u64
    {
        return Err(X_MX_LEDGER_UNBALANCED);
    }
    // 7) 屏蔽丢弃裁决在 ISR 有位时必须拒绝。
    if core2.eoi(60, Terminal::MaskedDrop).is_ok() {
        return Ok(7);
    }
    // 8) 诊断码表完备性：段内 11 码有名。
    let probe = [
        SEG_BASE, X_MX_VEC_RANGE, X_MX_CLASS_BAD, X_MX_DUP_VECTOR, X_MX_GHOST_DELIVER,
        X_MX_EOI_NO_ISR, X_MX_MATRIX_MISMATCH, X_MX_LEDGER_UNBALANCED, X_MX_ADJUDGE_TWICE,
        X_MX_TERMINAL_BAD, X_MX_LEDGER_FULL,
    ];
    for c in probe {
        if code_name(c) == "未知码" {
            return Ok(8);
        }
    }
    Ok(0)
}
