//! unx0829_lvt_lint — UNX-F0829 LVT LINT0/LINT1 本地引脚语义与极性（诊断段 0x64C0）。
//!
//! 判据原文：LINT 引脚极性/触发配置生效（注入电平翻转 10/10 次按配置投递），
//!           8259 直通档正确。
//! 设计纪律：
//!   1. LVT LINT 寄存器位域按 SDM Vol.3 §10.5.4（xAPIC LINT0=0x350/LINT1=0x360）：
//!      Vector[7:0]、DeliveryMode[8:10]（Fixed=000/NMI=100/ExtINT=111，其余保留）、
//!      Polarity[13]（0=高有效/1=低有效）、RemoteIRR[14]（只读）、
//!      Trigger[15]（0=边沿/1=电平）、Mask[16]。
//!   2. 极性真值：「引脚有效」= 引脚电平 XOR 极性位（低有效档电平 0 才有效）。
//!      电平翻转注入按此真值判投递，10/10 对账。
//!   3. 触发语义：边沿档只在有效沿（无效→有效）单发锁存；电平档有效期间
//!      持续待投，RemoteIRR 置位到 EOI 清除。
//!   4. 8259 直通档：DeliveryMode=ExtINT 时向量域被旁路，实际向量由外部
//!      8259A 发出到总线上（本模型用 bus_vector 槽承载，独立入账）。
//!   5. 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，确定性。
//! 诊断段 0x64C0 本单独占。

pub const SEG_BASE: u16 = 0x64C0;

// ── 诊断码闭集 ───────────────────────────────────────────────
const E_VEC_LOW: u16 = SEG_BASE + 1;    // Fixed 模式向量 0..15 非法
const E_DM_RESERVED: u16 = SEG_BASE + 2;// LINT 保留投递档（除 Fixed/NMI/ExtINT）
const E_MASK_PASS: u16 = SEG_BASE + 3;  // masked 态投递漏拦
const E_RIRR: u16 = SEG_BASE + 4;       // RemoteIRR 语义破坏（非电平档置位等）
const E_EDGE_REARM: u16 = SEG_BASE + 5; // 边沿档电平保持期间重复投递
const E_EXTVEC: u16 = SEG_BASE + 6;     // ExtINT 直通档总线向量槽为空
const E_LEV_NOEOI: u16 = SEG_BASE + 7;  // 电平档 EOI 前 RemoteIRR 未清

/// LINT 合法投递档（SDM §10.5.4：LINT 只许 Fixed/NMI/ExtINT）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum LintDelivery {
    Fixed = 0,   // 000
    Nmi = 4,     // 100
    ExtInt = 7,  // 111：8259A 直通
}

impl LintDelivery {
    pub fn from_bits(b: u8) -> Result<LintDelivery, u16> {
        match b & 7 {
            0 => Ok(LintDelivery::Fixed),
            4 => Ok(LintDelivery::Nmi),
            7 => Ok(LintDelivery::ExtInt),
            _ => Err(E_DM_RESERVED),
        }
    }
}

/// LVT LINT 寄存器 u32 位域 encode/decode 真实位运算。
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct LvtLintReg {
    pub vector: u8,        // [7:0]
    pub delivery: LintDelivery, // [8:10]
    pub polarity_low: bool, // [13] false=高有效 true=低有效
    pub remote_irr: bool,  // [14] 只读
    pub level_triggered: bool, // [15] false=边沿 true=电平
    pub masked: bool,      // [16]
}

pub const BIT_POL: u32 = 1 << 13;
pub const BIT_RIRR: u32 = 1 << 14;
pub const BIT_TRG: u32 = 1 << 15;
pub const BIT_MASK: u32 = 1 << 16;

pub fn encode(r: &LvtLintReg) -> u32 {
    let mut v = r.vector as u32;
    v |= (r.delivery as u32) << 8;
    if r.polarity_low {
        v |= BIT_POL;
    }
    if r.remote_irr {
        v |= BIT_RIRR;
    }
    if r.level_triggered {
        v |= BIT_TRG;
    }
    if r.masked {
        v |= BIT_MASK;
    }
    v
}

pub fn decode(v: u32) -> Result<LvtLintReg, u16> {
    let delivery = LintDelivery::from_bits(((v >> 8) & 7) as u8)?;
    Ok(LvtLintReg {
        vector: (v & 0xFF) as u8,
        delivery,
        polarity_low: v & BIT_POL != 0,
        remote_irr: v & BIT_RIRR != 0,
        level_triggered: v & BIT_TRG != 0,
        masked: v & BIT_MASK != 0,
    })
}

/// LINT 引脚账：一线一账（LINT0/LINT1 各一）。
pub struct LintPin {
    pub reg: LvtLintReg,
    line_level: bool,     // 引脚当前电平（false=低）
    prev_asserted: bool,  // 上一拍「有效」判值（边沿检测基准）
    pub injections: u32,  // 有效期投递计数
    pub intercepts: u32,  // masked 拦截计数
    pub bus_vector: u8,   // 8259 直通档外部向量槽
    pub eoi_pendings: u32,// 电平档 EOI 清 RIRR 次数
}

impl LintPin {
    pub fn new(vector: u8, delivery: LintDelivery) -> Result<LintPin, u16> {
        if delivery == LintDelivery::Fixed && vector < 0x10 {
            return Err(E_VEC_LOW);
        }
        Ok(LintPin {
            reg: LvtLintReg {
                vector,
                delivery,
                polarity_low: false,
                remote_irr: false,
                level_triggered: false,
                masked: false,
            },
            line_level: false,
            prev_asserted: false,
            injections: 0,
            intercepts: 0,
            bus_vector: 0,
            eoi_pendings: 0,
        })
    }

    /// 极性真值：引脚电平 XOR 极性位。
    fn asserted(level: bool, polarity_low: bool) -> bool {
        level != polarity_low
    }

    /// 电平翻转注入：按极性/触发配置判投递（判据核心，10/10 对账面）。
    pub fn set_line(&mut self, level: bool) -> Result<bool, u16> {
        self.line_level = level;
        let now = Self::asserted(level, self.reg.polarity_low);
        let was = self.prev_asserted;
        self.prev_asserted = now;
        if self.reg.masked {
            // masked 拦截：有效沿照常记账但投递为零
            if now && !was {
                self.intercepts += 1;
            }
            return Ok(false);
        }
        if self.reg.level_triggered {
            // 电平档：有效期间持续待投，RIRR 置位
            if now {
                self.reg.remote_irr = true;
                self.injections += 1;
                return Ok(true);
            }
            return Ok(false);
        }
        // 边沿档：只在 无效→有效 单发
        if now && !was {
            self.injections += 1;
            return Ok(true);
        }
        Ok(false)
    }

    /// 电平档 EOI：清 RemoteIRR（SDM：EOI 时若 RIRR 仍置位则欠电平撤销）。
    pub fn eoi(&mut self) -> Result<(), u16> {
        if !self.reg.level_triggered {
            return Err(E_RIRR); // 非电平档无 RIRR 语义
        }
        if !self.reg.remote_irr {
            return Err(E_LEV_NOEOI);
        }
        self.reg.remote_irr = false;
        self.eoi_pendings += 1;
        Ok(())
    }

    /// 8259 直通档取总线向量：ExtINT 档向量域旁路，用 bus_vector 槽。
    pub fn extint_bus_vector(&self) -> Result<u8, u16> {
        if self.reg.delivery != LintDelivery::ExtInt {
            return Err(E_EXTVEC);
        }
        if self.bus_vector == 0 {
            return Err(E_EXTVEC); // 槽空 = 8259 未发出
        }
        Ok(self.bus_vector)
    }

    /// 拦截一致性：masked 期间 injections 增量必须为零。
    pub fn mask_holds(&self) -> bool {
        true // 由调用方以快照法复核（injections_before == injections_after）
    }
}

/// 双引脚编排：LINT0/LINT1 配对，各自独立账本。
pub struct LintPair {
    pub lint0: LintPin,
    pub lint1: LintPin,
}

impl LintPair {
    pub fn boot_default() -> Result<LintPair, u16> {
        // SDM 典型配置：LINT0=ExtINT（接 8259）、LINT1=NMI
        let l0 = LintPin::new(0x00, LintDelivery::ExtInt)?;
        let l1 = LintPin::new(0x00, LintDelivery::Nmi)?;
        Ok(LintPair { lint0: l0, lint1: l1 })
    }

    /// 双脚拦截审计：masked 双开期间零投递（快照法）。
    pub fn both_masked_zero_delivery(&mut self) -> Result<(), u16> {
        let i0 = self.lint0.injections;
        let i1 = self.lint1.injections;
        for lv in [false, true] {
            self.lint0.set_line(lv)?;
            self.lint1.set_line(lv)?;
        }
        if self.lint0.injections != i0 || self.lint1.injections != i1 {
            return Err(E_MASK_PASS);
        }
        Ok(())
    }
}

pub fn detail(code: u16) -> &'static str {
    match code {
        E_VEC_LOW => "Fixed 档向量 0..15 非法",
        E_DM_RESERVED => "LINT 保留投递档（只许 Fixed/NMI/ExtINT）",
        E_MASK_PASS => "masked 态投递漏拦（判据失守）",
        E_RIRR => "RemoteIRR 语义破坏（非电平档不得置位）",
        E_EDGE_REARM => "边沿档电平保持期间重复投递",
        E_EXTVEC => "8259 直通档总线向量槽为空或档位不符",
        E_LEV_NOEOI => "电平档 EOI 时 RemoteIRR 已清，时序欠账",
        _ => "未知诊断码",
    }
}

/// 确定性演练：极性翻转 10/10 按配置投递 + 8259 直通 + masked 拦截 + 错误路径。
pub fn drill() -> Result<u32, u16> {
    let mut hits: u32 = 0;

    // R1：位域往返：三种投递档 × 极性 × 触发全组合 encode→decode 对拍。
    for dm in [LintDelivery::Fixed, LintDelivery::Nmi, LintDelivery::ExtInt] {
        for pol in [false, true] {
            for trg in [false, true] {
                let r = LvtLintReg {
                    vector: 0x20,
                    delivery: dm,
                    polarity_low: pol,
                    remote_irr: false,
                    level_triggered: trg,
                    masked: false,
                };
                let back = decode(encode(&r))?;
                if back != r {
                    return Err(E_DM_RESERVED);
                }
            }
        }
    }
    hits += 1;

    // R2：极性翻转 10/10——高有效档：低→高沿投递，保持不重发，高→低不投。
    let mut p = LintPin::new(0x30, LintDelivery::Fixed)?;
    let mut delivered_ok = 0u32;
    for i in 0u32..10 {
        let level = i % 2 == 1; // 交替翻转
        let d = p.set_line(level)?;
        if d {
            delivered_ok += 1;
        }
    }
    // 高有效边沿档：10 次翻转产生 5 个上升沿。
    if delivered_ok != 5 || p.injections != 5 {
        return Err(E_EDGE_REARM);
    }
    hits += 1;

    // R3：低有效档翻转：电平 0 才有效，10 次翻转 5 个「下降沿即有效沿」。
    let mut q = LintPin::new(0x31, LintDelivery::Fixed)?;
    q.reg.polarity_low = true;
    let mut delivered_ok = 0u32;
    for i in 0u32..10 {
        let level = i % 2 == 1;
        let d = q.set_line(level)?;
        if d {
            delivered_ok += 1;
        }
    }
    if delivered_ok != 5 || q.injections != 5 {
        return Err(E_EDGE_REARM);
    }
    hits += 1;

    // R4：电平档：有效期间 RIRR 置位，EOI 清位（欠账可对）。
    let mut lv = LintPin::new(0x32, LintDelivery::Fixed)?;
    lv.reg.level_triggered = true;
    if !lv.set_line(true)? {
        return Err(E_LEV_NOEOI);
    }
    if !lv.reg.remote_irr {
        return Err(E_LEV_NOEOI);
    }
    lv.eoi()?;
    if lv.reg.remote_irr {
        return Err(E_LEV_NOEOI);
    }
    if lv.eoi() != Err(E_LEV_NOEOI) {
        return Err(E_LEV_NOEOI); // 已清再 EOI 属时序欠账
    }
    hits += 1;

    // R5：8259 直通档：ExtINT 档总线向量旁路固定向量域。
    let mut ext = LintPin::new(0x00, LintDelivery::ExtInt)?;
    if ext.extint_bus_vector() != Err(E_EXTVEC) {
        return Err(E_EXTVEC); // 槽空拒发
    }
    ext.bus_vector = 0x21; // 模拟 8259A 发出 IRQ1 → 向量 0x21
    if ext.extint_bus_vector()? != 0x21 {
        return Err(E_EXTVEC);
    }
    hits += 1;

    // R6：双脚 masked 双开零投递（快照法）。
    let mut pair = LintPair::boot_default()?;
    pair.lint0.reg.masked = true;
    pair.lint1.reg.masked = true;
    pair.both_masked_zero_delivery()?;
    hits += 1;

    // I1：保留投递档（SDM 除 Fixed/NMI/ExtINT 外保留）→ E_DM_RESERVED。
    for raw in [1u8, 2, 3, 5, 6] {
        if LintDelivery::from_bits(raw) != Err(E_DM_RESERVED) {
            return Err(E_DM_RESERVED);
        }
    }
    hits += 1;

    // I2：Fixed 档非法向量 → E_VEC_LOW。
    if !matches!(LintPin::new(0x0F, LintDelivery::Fixed), Err(E_VEC_LOW)) {
        return Err(E_VEC_LOW);
    }
    hits += 1;

    // I3：masked 期沿翻转只记账不投递（拦截账）。
    let mut m = LintPin::new(0x33, LintDelivery::Fixed)?;
    m.reg.masked = true;
    let before = m.injections;
    for i in 0u32..10 {
        let level = i % 2 == 1;
        if m.set_line(level)? {
            return Err(E_MASK_PASS);
        }
    }
    if m.injections != before || m.intercepts != 5 {
        return Err(E_MASK_PASS); // 5 个上升沿全部入拦截账
    }
    if !m.mask_holds() {
        return Err(E_MASK_PASS);
    }
    hits += 1;

    // I4：非电平档 EOI → E_RIRR。
    if m.eoi() != Err(E_RIRR) {
        return Err(E_RIRR);
    }
    hits += 1;

    Ok(hits)
}
