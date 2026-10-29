//! unx0827_esr_read2 — UNX-F0827 ESR 错误状态寄存器两次读语义与错误分账（诊断段 0x6480）。
//!
//! 判据原文：两次读语义正确（首读锁定次读清零，注入 10/10 次符合），
//!           错误位七类分账独立。
//! 设计纪律：
//!   1. ESR 位域按 SDM Vol.3 §10.5.4（xAPIC 偏移 0x280）：bit0 发送校验和错、
//!      bit1 接收校验和错、bit2 发送接受错、bit3 接收接受错、bit4 发送非法向量、
//!      bit5 接收非法向量、bit6 非法寄存器地址——七类闭集，bit7+ 保留。
//!   2. 两次读语义：首读返回锁存值并冻结（此后错误不再入锁存），
//!      次读恒零；置零义务由软件写 0 完成（xAPIC 语义），写 1 非法。
//!   3. 七类分账独立：每类独立计数器 [u32;7]，锁存时按位入账，
//!      各类账互不串扰，总账 = Σ各类账。
//!   4. 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，确定性。
//! 诊断段 0x6480 本单独占。

use crate::alloc::{format, string::{String, ToString}, vec::Vec};

pub const SEG_BASE: u16 = 0x6480;

// ── 诊断码闭集 ───────────────────────────────────────────────
const E_RESERVED_BIT: u16 = SEG_BASE + 1; // 保留位（bit7+）注入被拒
const E_WRITE_ONE: u16 = SEG_BASE + 2;    // xAPIC 语义：写 1 非法，只许写 0
const E_LATCH_LEAK: u16 = SEG_BASE + 3;   // 锁存后新错误未进挂起账
const E_READ2_NONZERO: u16 = SEG_BASE + 4;// 次读非零：两次读语义破坏
const E_BAL: u16 = SEG_BASE + 5;          // 分账与总账不平
const E_SEQ: u16 = SEG_BASE + 6;          // 锁存期间再触发计数器错序

/// 七类错误位定义（SDM §10.5.4 / §10.5.4.4 扩展位说明）。
pub const BIT_SEND_CS: u8 = 0;      // 发送校验和错（串行总线）
pub const BIT_RECV_CS: u8 = 1;      // 接收校验和错
pub const BIT_SEND_ACCEPT: u8 = 2;  // 发送接受错（目的地拒收）
pub const BIT_RECV_ACCEPT: u8 = 3;  // 接收接受错
pub const BIT_SEND_ILLVEC: u8 = 4;  // 发送非法向量（ICR 向量 < 16）
pub const BIT_RECV_ILLVEC: u8 = 5;  // 接收非法向量
pub const BIT_ILL_REG: u8 = 6;      // 非法寄存器地址（MMIO 未对齐/越界）
/// 合法错误位闭集：0..=6。
pub const LEGAL_ERR_BITS: u8 = 0x7F;

/// ESR 核心：挂起/锁存/分账三账并立。
pub struct EsrCore {
    pending: u8,        // 挂起错误位（未锁存）
    latched: u8,        // 首读锁存值
    read_phase: u8,     // 0=未读，1=已首读，2=已次读
    ledger: [u32; 7],   // 七类独立分账
    raise_events: u32,  // 注入事件总数（10/10 判据分母）
    latch_events: u32,  // 首读锁存事件数
    zero_writes: u32,   // 软件写 0 清零次数
}

impl EsrCore {
    pub fn new() -> EsrCore {
        EsrCore {
            pending: 0,
            latched: 0,
            read_phase: 0,
            ledger: [0u32; 7],
            raise_events: 0,
            latch_events: 0,
            zero_writes: 0,
        }
    }

    /// 注入错误：按位累入挂起账。锁存冻结期间新错误只进挂起账
    /// （等下次首读收走），保留位注入直接拒绝。
    pub fn raise(&mut self, mask: u8) -> Result<(), u16> {
        if mask & !LEGAL_ERR_BITS != 0 {
            return Err(E_RESERVED_BIT);
        }
        if mask == 0 {
            return Err(E_RESERVED_BIT);
        }
        self.pending |= mask;
        self.raise_events += 1;
        Ok(())
    }

    /// 首读：锁存当前挂起值并按位入七类分账；此后进入冻结相。
    pub fn read_first(&mut self) -> u8 {
        if self.read_phase == 0 {
            self.latched = self.pending;
            self.pending = 0;
            self.read_phase = 1;
            self.latch_events += 1;
            // 七类分账：逐位独立入账
            for b in 0u8..7 {
                if self.latched & (1 << b) != 0 {
                    self.ledger[b as usize] += 1;
                }
            }
        }
        self.latched
    }

    /// 次读：两次读语义——首读后任何读都恒零，直到软件写 0 复位。
    pub fn read_second(&self) -> u8 {
        if self.read_phase == 0 {
            // 没做首读就调次读属调用错序，返回 0 但不计违约（防线保底）
            return 0;
        }
        0 // 锁存后恒零（判据核心）
    }

    /// 软件写 0：清锁存，回未读相（xAPIC：写 0 清 ESR；写 1 非法）。
    pub fn software_clear(&mut self, val: u8) -> Result<(), u16> {
        if val != 0 {
            return Err(E_WRITE_ONE);
        }
        self.latched = 0;
        self.read_phase = 0;
        self.zero_writes += 1;
        Ok(())
    }

    /// 首读锁存后，注入 10/10 判据的核对面：
    /// 冻结相内新注入不进本次锁存值，只进下次锁存（防泄漏）。
    pub fn frozen_raise_isolated(&self) -> bool {
        // 锁存值与挂起账在冻结相内互不污染
        self.latched & self.pending == 0
    }

    /// 分账守恒：各类账之和 == 锁存事件 × 每事件位数的总入账。
    pub fn ledger_sum(&self) -> u32 {
        let mut s = 0u32;
        for n in &self.ledger {
            s += *n;
        }
        s
    }

    /// 单类账查询（判据「七类分账独立」的读出口）。
    pub fn ledger_at(&self, bit: u8) -> Result<u32, u16> {
        if bit > 6 {
            return Err(E_RESERVED_BIT);
        }
        Ok(self.ledger[bit as usize])
    }

    pub fn stats(&self) -> (u32, u32, u32) {
        (self.raise_events, self.latch_events, self.zero_writes)
    }
}

/// 七类分账报告：每类一行，含独立计数（门禁点名用）。
pub fn ledger_report(c: &EsrCore) -> Result<Vec<String>, u16> {
    if c.ledger_sum() == 0 {
        // 零分账不算错误，返回空报告但保持 Result 通道一致
        let mut v = Vec::new();
        v.push("分账全零：无错误注入记录".to_string());
        return Ok(v);
    }
    let mut out = Vec::new();
    for b in 0u8..7 {
        let n = c.ledger_at(b)?;
        if n > 0 {
            out.push(format!("bit{}({})={}", b, class_name(b), n));
        }
    }
    Ok(out)
}

fn class_name(bit: u8) -> &'static str {
    match bit {
        BIT_SEND_CS => "发送校验和错",
        BIT_RECV_CS => "接收校验和错",
        BIT_SEND_ACCEPT => "发送接受错",
        BIT_RECV_ACCEPT => "接收接受错",
        BIT_SEND_ILLVEC => "发送非法向量",
        BIT_RECV_ILLVEC => "接收非法向量",
        BIT_ILL_REG => "非法寄存器地址",
        _ => "保留位",
    }
}

pub fn detail(code: u16) -> &'static str {
    match code {
        E_RESERVED_BIT => "保留位（bit7+）或空掩码注入被拒",
        E_WRITE_ONE => "xAPIC ESR 只许写 0 清零，写 1 非法",
        E_LATCH_LEAK => "锁存后新错误未隔离进挂起账",
        E_READ2_NONZERO => "次读非零：两次读语义破坏",
        E_BAL => "七类分账与总账不平",
        E_SEQ => "锁存期间计数错序",
        _ => "未知诊断码",
    }
}

/// 确定性演练：注入 10/10 两次读符合 + 七类独立分账 + 错误路径。
pub fn drill() -> Result<u32, u16> {
    let mut hits: u32 = 0;
    let mut esr = EsrCore::new();

    // R1：十轮注入-首读-次读，每轮两次读语义 10/10 符合（判据原文）。
    let masks: [u8; 10] = [
        1 << BIT_SEND_CS,
        1 << BIT_RECV_CS,
        1 << BIT_SEND_ACCEPT,
        1 << BIT_RECV_ACCEPT,
        1 << BIT_SEND_ILLVEC,
        1 << BIT_RECV_ILLVEC,
        1 << BIT_ILL_REG,
        (1 << BIT_SEND_ACCEPT) | (1 << BIT_RECV_ACCEPT),
        (1 << BIT_SEND_CS) | (1 << BIT_SEND_ILLVEC) | (1 << BIT_ILL_REG),
        LEGAL_ERR_BITS,
    ];
    for m in masks.iter() {
        esr.raise(*m)?;
        let r1 = esr.read_first();
        if r1 != *m {
            return Err(E_LATCH_LEAK);
        }
        let r2 = esr.read_second();
        if r2 != 0 {
            return Err(E_READ2_NONZERO);
        }
        esr.software_clear(0)?;
        if esr.read_first() != 0 {
            return Err(E_LATCH_LEAK); // 清零后首读必须归零
        }
        esr.software_clear(0)?;
    }
    let (r, l, z) = esr.stats();
    if r != 10 || l != 10 || z != 20 {
        return Err(E_SEQ);
    }
    hits += 1;

    // R2：七类分账独立——第 9 轮(三重)与第 10 轮(全七)使各类至少 2 次。
    for b in 0u8..7 {
        let n = esr.ledger_at(b)?;
        if n < 2 {
            return Err(E_BAL);
        }
    }
    // 各类账之和 = Σ每轮掩码 popcount（确定性重算对账）。
    let mut expect_sum = 0u32;
    for m in masks.iter() {
        expect_sum += m.count_ones();
    }
    if esr.ledger_sum() != expect_sum {
        return Err(E_BAL);
    }
    hits += 1;

    // R3：分账报告非空且行数 ≤ 7。
    let rep = ledger_report(&esr)?;
    if rep.len() > 7 {
        return Err(E_BAL);
    }
    hits += 1;

    // R4：冻结相隔离：首读后新注入不污染已锁存值。
    let mut esr2 = EsrCore::new();
    esr2.raise(1 << BIT_SEND_ACCEPT)?;
    let latched = esr2.read_first();
    esr2.raise(1 << BIT_ILL_REG)?;
    if esr2.frozen_raise_isolated() {
        if latched & (1 << BIT_ILL_REG) != 0 {
            return Err(E_LATCH_LEAK);
        }
    } else {
        return Err(E_LATCH_LEAK);
    }
    // 冻结相次读仍恒零。
    if esr2.read_second() != 0 {
        return Err(E_READ2_NONZERO);
    }
    // 下一次首读收走新注入。
    let next = esr2.read_first();
    if next & (1 << BIT_ILL_REG) == 0 {
        return Err(E_LATCH_LEAK);
    }
    hits += 1;

    // I1：保留位注入 → E_RESERVED_BIT。
    if esr2.raise(0x80) != Err(E_RESERVED_BIT) {
        return Err(E_RESERVED_BIT);
    }
    // I1b：空掩码注入 → E_RESERVED_BIT。
    if esr2.raise(0) != Err(E_RESERVED_BIT) {
        return Err(E_RESERVED_BIT);
    }
    hits += 1;

    // I2：写 1 清零 → E_WRITE_ONE（xAPIC 只许写 0）。
    if esr2.software_clear(1) != Err(E_WRITE_ONE) {
        return Err(E_WRITE_ONE);
    }
    hits += 1;

    // I3：超闭集单类账查询 → E_RESERVED_BIT。
    if esr2.ledger_at(7) != Err(E_RESERVED_BIT) {
        return Err(E_RESERVED_BIT);
    }
    hits += 1;

    // I4：次读恒零再证——锁存相连续多次读全零。
    esr2.software_clear(0)?;
    esr2.raise(LEGAL_ERR_BITS)?;
    let _ = esr2.read_first();
    for _ in 0..5 {
        if esr2.read_second() != 0 {
            return Err(E_READ2_NONZERO);
        }
    }
    hits += 1;

    Ok(hits)
}
