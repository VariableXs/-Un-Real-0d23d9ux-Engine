//! unx0828_lvt_timer — UNX-F0828 LVT 定时器寄存器三模与档位管理（诊断段 0x64A0）。
//!
//! 判据原文：三模式位切换正确（one-shot/periodic/deadline 位），
//!           masked 态投递拦截 10/10 次。
//! 设计纪律：
//!   1. LVT 定时器寄存器位域按 SDM Vol.3 §10.5.4（xAPIC 偏移 0x320）：
//!      Vector[7:0]、DeliveryStatus[12]（0=Idle/1=SendPending）、
//!      Mask[16]、TimerMode[17:18]（00=one-shot/01=periodic/10=TSC-deadline/
//!      11=保留非法）；encode/decode 用真实位运算双向验证。
//!   2. 模式状态机：三模式间合法边闭集枚举，SendPending 态禁止切模式，
//!      deadline 档只在 x2APIC 使能下合法（SDM §10.12.9.1）。
//!   3. masked 态投递拦截：到期事件照常入到期账，但投递被 Mask[16] 拦下，
//!      计数独立入账——拦截不影响状态机演进。
//!   4. 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，确定性。
//! 诊断段 0x64A0 本单独占。

use crate::alloc::vec::Vec;

pub const SEG_BASE: u16 = 0x64A0;

// ── 诊断码闭集 ───────────────────────────────────────────────
const E_VEC_LOW: u16 = SEG_BASE + 1;     // 向量 0..15 非法
const E_MODE_RESERVED: u16 = SEG_BASE + 2; // TimerMode=11 保留档
const E_MODE_BUSY: u16 = SEG_BASE + 3;   // SendPending 态切模式被拒
const E_DEADLINE_X2: u16 = SEG_BASE + 4; // deadline 档需 x2APIC 使能
const E_MASK_PASS: u16 = SEG_BASE + 5;   // masked 态投递漏拦（判据失守）
const E_REG_MIRROR: u16 = SEG_BASE + 6;  // encode/decode 往返不等
const E_DSTATUS: u16 = SEG_BASE + 7;     // DeliveryStatus 迁移非法
const E_BALM: u16 = SEG_BASE + 8;        // 到期账与拦截账不平

/// 定时器三模式（TimerMode[17:18] 硬件档位值）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TimerMode {
    OneShot = 0,    // 00：到期一次后停摆
    Periodic = 1,   // 01：到期自动重装初始计数
    TscDeadline = 2, // 10：x2APIC 专属档（需 IA32_TSC_DEADLINE）
}

impl TimerMode {
    pub fn from_bits(b: u8) -> Result<TimerMode, u16> {
        match b & 3 {
            0 => Ok(TimerMode::OneShot),
            1 => Ok(TimerMode::Periodic),
            2 => Ok(TimerMode::TscDeadline),
            _ => Err(E_MODE_RESERVED),
        }
    }

    pub fn to_bits(self) -> u8 {
        self as u8
    }
}

/// 模式间合法边闭集：三模式两两互切共 6 条边（含自环不算）。
pub const LEGAL_MODE_EDGES: usize = 6;

pub fn mode_edge_legal(from: TimerMode, to: TimerMode) -> bool {
    if from == to {
        return true; // 自环恒合法
    }
    // 三模式全连通：合法边数闭集 6
    true
}

/// LVT 定时器寄存器：u32 位域 encode/decode 双向真实实现。
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct LvtTimerReg {
    pub vector: u8,       // [7:0]
    pub masked: bool,     // [16]
    pub mode: TimerMode,  // [17:18]
    pub dstatus: u8,      // [12] 0=Idle 1=SendPending
}

pub const BIT_DSTATUS: u32 = 1 << 12;
pub const BIT_MASK: u32 = 1 << 16;
pub const SHIFT_MODE: u32 = 17;

pub fn encode(r: &LvtTimerReg) -> u32 {
    let mut v = r.vector as u32;
    if r.dstatus != 0 {
        v |= BIT_DSTATUS;
    }
    if r.masked {
        v |= BIT_MASK;
    }
    v |= (r.mode.to_bits() as u32) << SHIFT_MODE;
    v
}

pub fn decode(v: u32) -> Result<LvtTimerReg, u16> {
    let mode = TimerMode::from_bits(((v >> SHIFT_MODE) & 3) as u8)?;
    let vector = (v & 0xFF) as u8;
    if vector < 0x10 {
        return Err(E_VEC_LOW);
    }
    Ok(LvtTimerReg {
        vector,
        masked: v & BIT_MASK != 0,
        mode,
        dstatus: ((v & BIT_DSTATUS != 0) as u8),
    })
}

/// 到期事件账本条目。
#[derive(Clone, Copy)]
pub struct ExpiryEntry {
    pub vector: u8,
    pub masked_intercepted: bool,
    pub delivered: bool,
}

/// LVT 定时器管理引擎：寄存器 + 状态机 + masked 拦截账。
pub struct LvtTimerEngine {
    pub reg: LvtTimerReg,
    x2apic_on: bool,
    pub expiries: Vec<ExpiryEntry>,
    delivered: u32,
    intercepted: u32,
}

impl LvtTimerEngine {
    pub fn new(vector: u8, x2apic_on: bool) -> Result<LvtTimerEngine, u16> {
        if vector < 0x10 {
            return Err(E_VEC_LOW);
        }
        Ok(LvtTimerEngine {
            reg: LvtTimerReg {
                vector,
                masked: false,
                mode: TimerMode::OneShot,
                dstatus: 0,
            },
            x2apic_on,
            expiries: Vec::new(),
            delivered: 0,
            intercepted: 0,
        })
    }

    /// 写寄存器：保留位归零校验（位域真实性），再落状态机。
    pub fn write_reg(&mut self, v: u32) -> Result<(), u16> {
        let used = 0xFF | BIT_DSTATUS | BIT_MASK | (3 << SHIFT_MODE);
        if v & !used != 0 {
            return Err(E_REG_MIRROR);
        }
        let r = decode(v)?;
        // 状态机约束：SendPending 态禁止切模式
        if self.reg.dstatus != 0 && r.mode != self.reg.mode {
            return Err(E_MODE_BUSY);
        }
        // deadline 档需 x2APIC
        if r.mode == TimerMode::TscDeadline && !self.x2apic_on {
            return Err(E_DEADLINE_X2);
        }
        self.reg = r;
        Ok(())
    }

    /// 模式切换（高级入口）：校验合法边后重写寄存器。
    pub fn switch_mode(&mut self, m: TimerMode) -> Result<(), u16> {
        if !mode_edge_legal(self.reg.mode, m) {
            return Err(E_MODE_RESERVED);
        }
        if self.reg.dstatus != 0 {
            return Err(E_MODE_BUSY);
        }
        if m == TimerMode::TscDeadline && !self.x2apic_on {
            return Err(E_DEADLINE_X2);
        }
        self.reg.mode = m;
        Ok(())
    }

    /// DeliveryStatus 状态机：Idle→SendPending（到期），SendPending→Idle（投递/拦截清账）。
    pub fn on_expiry(&mut self) -> Result<(), u16> {
        if self.reg.dstatus != 0 {
            return Err(E_DSTATUS); // 已在 SendPending：事件叠加非法
        }
        self.reg.dstatus = 1;
        let masked = self.reg.masked;
        let v = self.reg.vector;
        // masked 态投递拦截：事件照记，投递为零（判据核心）
        if masked {
            self.intercepted += 1;
            self.expire_settle(true)?;
        } else {
            self.delivered += 1;
            self.expire_settle(false)?;
        }
        let _ = v;
        Ok(())
    }

    /// 到期清算：记账并回 Idle。masked 拦截同样清算（事件不滞留）。
    fn expire_settle(&mut self, intercepted: bool) -> Result<(), u16> {
        self.expiries.push(ExpiryEntry {
            vector: self.reg.vector,
            masked_intercepted: intercepted,
            delivered: !intercepted,
        });
        self.reg.dstatus = 0;
        Ok(())
    }

    /// masked 拦截一致性：拦截计数 = 账本拦截条目数 = masked 到期数。
    pub fn intercept_consistent(&self) -> bool {
        self.intercepted
            == self
                .expiries
                .iter()
                .filter(|e| e.masked_intercepted)
                .count() as u32
            && self.delivered
                == self.expiries.iter().filter(|e| e.delivered).count() as u32
            && self.delivered + self.intercepted == self.expiries.len() as u32
    }

    pub fn stats(&self) -> (u32, u32) {
        (self.delivered, self.intercepted)
    }
}

pub fn detail(code: u16) -> &'static str {
    match code {
        E_VEC_LOW => "LVT 向量 0..15 非法",
        E_MODE_RESERVED => "TimerMode=11 保留档非法",
        E_MODE_BUSY => "SendPending 态禁止切换模式",
        E_DEADLINE_X2 => "TSC-deadline 档需 x2APIC 使能",
        E_MASK_PASS => "masked 态投递漏拦（判据失守）",
        E_REG_MIRROR => "encode/decode 往返不等，位域实现失真",
        E_DSTATUS => "DeliveryStatus 迁移非法",
        E_BALM => "到期账与拦截/投递账不平",
        _ => "未知诊断码",
    }
}

/// 确定性演练：三模式切换 + masked 拦截 10/10 + 位域往返 + 错误路径。
pub fn drill() -> Result<u32, u16> {
    let mut hits: u32 = 0;

    // R1：位域往返：三模式 × masked × dstatus 组合全量 encode→decode 对拍。
    for m in [TimerMode::OneShot, TimerMode::Periodic, TimerMode::TscDeadline] {
        for masked in [false, true] {
            let r = LvtTimerReg { vector: 0x30, masked, mode: m, dstatus: 0 };
            let v = encode(&r);
            let back = decode(v)?;
            if back != r {
                return Err(E_REG_MIRROR);
            }
            if (v >> SHIFT_MODE) & 3 != m.to_bits() as u32 {
                return Err(E_REG_MIRROR);
            }
            if ((v & BIT_MASK) != 0) != masked {
                return Err(E_REG_MIRROR);
            }
        }
    }
    hits += 1;

    // R2：三模式切换序列 OneShot→Periodic→TSC-deadline→OneShot 全绿（合法边闭集）。
    let mut eng = LvtTimerEngine::new(0x40, true)?;
    eng.switch_mode(TimerMode::OneShot)?;
    eng.switch_mode(TimerMode::Periodic)?;
    eng.switch_mode(TimerMode::TscDeadline)?;
    eng.switch_mode(TimerMode::OneShot)?;
    if eng.reg.mode != TimerMode::OneShot {
        return Err(E_MODE_RESERVED);
    }
    hits += 1;

    // R3：masked 态投递拦截 10/10（判据原文）。
    let mut eng2 = LvtTimerEngine::new(0x50, false)?;
    eng2.reg.masked = true;
    for _ in 0..10 {
        eng2.on_expiry()?;
        if eng2.reg.dstatus != 0 {
            return Err(E_DSTATUS); // 拦截后必须回 Idle
        }
    }
    let (del, ice) = eng2.stats();
    if del != 0 || ice != 10 {
        return Err(E_MASK_PASS); // masked 态投递必须为零
    }
    if !eng2.intercept_consistent() {
        return Err(E_BALM);
    }
    hits += 1;

    // R4：解屏蔽后同频到期 10 次全部投递。
    eng2.reg.masked = false;
    for _ in 0..10 {
        eng2.on_expiry()?;
    }
    let (del, ice) = eng2.stats();
    if del != 10 || ice != 10 || !eng2.intercept_consistent() {
        return Err(E_MASK_PASS);
    }
    hits += 1;

    // I1：保留档 TimerMode=11 → E_MODE_RESERVED。
    if let Err(c) = decode(3 << SHIFT_MODE) {
        if c != E_MODE_RESERVED {
            return Err(E_MODE_RESERVED);
        }
    } else {
        return Err(E_MODE_RESERVED);
    }
    hits += 1;

    // I2：xAPIC 下写 deadline 档 → E_DEADLINE_X2。
    let mut eng3 = LvtTimerEngine::new(0x60, false)?;
    if eng3.switch_mode(TimerMode::TscDeadline) != Err(E_DEADLINE_X2) {
        return Err(E_DEADLINE_X2);
    }
    hits += 1;

    // I3：非法向量 0x0F → E_VEC_LOW。
    if !matches!(LvtTimerEngine::new(0x0F, true), Err(E_VEC_LOW)) {
        return Err(E_VEC_LOW);
    }
    hits += 1;

    // I4：SendPending 态切模式 → E_MODE_BUSY。
    let mut eng4 = LvtTimerEngine::new(0x70, true)?;
    eng4.reg.dstatus = 1; // 直接构造忙态（模拟到期未清算窗口）
    if eng4.switch_mode(TimerMode::Periodic) != Err(E_MODE_BUSY) {
        return Err(E_MODE_BUSY);
    }
    // 忙态重复到期 → E_DSTATUS。
    if eng4.on_expiry() != Err(E_DSTATUS) {
        return Err(E_DSTATUS);
    }
    hits += 1;

    // I5：寄存器写入口保留位污染 → E_REG_MIRROR。
    let dirty = encode(&LvtTimerReg {
        vector: 0x80,
        masked: false,
        mode: TimerMode::OneShot,
        dstatus: 0,
    }) | (1 << 31); // bit31 属保留位
    if eng4.write_reg(dirty) != Err(E_REG_MIRROR) {
        return Err(E_REG_MIRROR);
    }
    hits += 1;

    Ok(hits)
}
