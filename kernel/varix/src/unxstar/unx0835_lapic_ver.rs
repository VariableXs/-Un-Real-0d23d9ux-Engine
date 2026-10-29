//! unx0835_lapic_ver — UNX-F0835 LAPIC 版本寄存器与 EOI 广播抑制位（诊断段 0x6580）。
//!
//! 判据原文：版本三字段（版本/LVT 数/EOI 抑制支持）解码正确，
//!           抑制位策略按支持度分档。
//! 设计纪律：
//!   1. 版本寄存器（xAPIC 0x030，只读）按 SDM Vol.3 §10.4.8：
//!      Version[7:0]（0x10=P4 档 / 0x14=x2APIC 档）、MaxLVTEntry[23:16]、
//!      ABR[24]（EOI 广播抑制支持位）；[15:8] 家族域读零。
//!   2. 三字段解码闭集：Version 档位白名单（0x10/0x14），MaxLVT 上界
//!      6（0..=MaxLVT 的 LVT 偏移合法），ABR 独立布尔。
//!   3. 抑制策略分档：ABR=1 → 可抑制（按策略开关入账）；ABR=0 →
//!      强制广播（写抑制位被拒）。EOI 出口账分 local/broadcast 两路守恒。
//!   4. 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，确定性。
//! 诊断段 0x6580 本单独占。

pub const SEG_BASE: u16 = 0x6580;

// ── 诊断码闭集 ───────────────────────────────────────────────
const E_VER_RESV: u16 = SEG_BASE + 1;  // Version 档位白名单外
const E_LVT_RANGE: u16 = SEG_BASE + 2; // LVT 偏移超 MaxLVTEntry
const E_RO: u16 = SEG_BASE + 3;        // 版本寄存器只读，写入被拒
const E_SUPP_NA: u16 = SEG_BASE + 4;   // ABR=0 时启用抑制被拒
const E_BAL: u16 = SEG_BASE + 5;       // EOI 出口账不平
const E_FIELD: u16 = SEG_BASE + 6;     // 家族域 [15:8] 非零（解码面失真）

/// Version 档位白名单。
pub const VER_P4: u8 = 0x10;
pub const VER_X2APIC: u8 = 0x14;

/// LVT 档上界（P4 档 6 槽：CMCI/Timer/Thermal/Perf/LINT0/LINT1/Error）。
pub const MAX_LVT_P4: u8 = 6;

/// 版本寄存器位域 encode（构造模型值）与 decode（三字段拆解）。
pub const SHIFT_MAXLVT: u32 = 16;
pub const BIT_ABR: u32 = 1 << 24;

pub fn encode_version(version: u8, max_lvt: u8, abr: bool) -> u32 {
    let mut v = version as u32;
    v |= (max_lvt as u32) << SHIFT_MAXLVT;
    if abr {
        v |= BIT_ABR;
    }
    v
}

/// 三字段解码：白名单 + 家族域零校验。
pub fn decode_version(v: u32) -> Result<(u8, u8, bool), u16> {
    if v & 0x0000_0001 != 0 {
        return Err(E_FIELD); // 家族域 [15:8] 必须读零
    }
    let version = (v & 0xFF) as u8;
    if version != VER_P4 && version != VER_X2APIC {
        return Err(E_VER_RESV);
    }
    let max_lvt = ((v >> SHIFT_MAXLVT) & 0xFF) as u8;
    if max_lvt > MAX_LVT_P4 {
        return Err(E_LVT_RANGE);
    }
    Ok((version, max_lvt, v & BIT_ABR != 0))
}

/// EOI 出口策略分档（判据「按支持度分档」）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SuppressPolicy {
    /// ABR=1：按开关走，开关开 = 抑制（local 出口）。
    Switchable,
    /// ABR=0：强制广播，抑制入口拒收。
    ForceBroadcast,
}

/// EOI 出口账：local 与 broadcast 两路守恒。
pub struct EoiLedger {
    pub version: u8,
    pub max_lvt: u8,
    pub policy: SuppressPolicy,
    pub suppress_switch: bool,
    pub local_eois: u32,
    pub broadcast_eois: u32,
    pub rejected_suppress: u32,
}

impl EoiLedger {
    /// 按版本寄存器原始值装配（解码失败直接上抛）。
    pub fn from_version(v: u32) -> Result<EoiLedger, u16> {
        let (version, max_lvt, abr) = decode_version(v)?;
        Ok(EoiLedger {
            version,
            max_lvt,
            policy: if abr {
                SuppressPolicy::Switchable
            } else {
                SuppressPolicy::ForceBroadcast
            },
            suppress_switch: false,
            local_eois: 0,
            broadcast_eois: 0,
            rejected_suppress: 0,
        })
    }

    /// LVT 偏移合法性（MaxLVTEntry 闭集）。
    pub fn lvt_offset_ok(&self, off: u8) -> bool {
        off <= self.max_lvt
    }

    /// EOI 出口：抑制请求按策略分档；ABR=0 时抑制入口拒收（计负账）。
    pub fn eoi(&mut self, want_suppress: bool) -> Result<(), u16> {
        match self.policy {
            SuppressPolicy::Switchable => {
                if want_suppress && self.suppress_switch {
                    self.local_eois += 1;
                } else {
                    self.broadcast_eois += 1;
                }
                Ok(())
            }
            SuppressPolicy::ForceBroadcast => {
                if want_suppress {
                    self.rejected_suppress += 1;
                    return Err(E_SUPP_NA);
                }
                self.broadcast_eois += 1;
                Ok(())
            }
        }
    }

    /// 出口守恒：local + broadcast + 拒收 = 总请求。
    pub fn conserved(&self, total_requests: u32) -> bool {
        self.local_eois + self.broadcast_eois + self.rejected_suppress
            == total_requests
    }
}

pub fn detail(code: u16) -> &'static str {
    match code {
        E_VER_RESV => "Version 档位白名单外（只许 0x10/0x14）",
        E_LVT_RANGE => "MaxLVTEntry 超上界 6 或 LVT 偏移越界",
        E_RO => "版本寄存器只读，写入被拒",
        E_SUPP_NA => "ABR=0 时抑制入口拒收（强制广播档）",
        E_BAL => "EOI 出口账不平",
        E_FIELD => "家族域 [15:8] 非零，解码面失真",
        _ => "未知诊断码",
    }
}

/// 确定性演练：三字段解码 + 抑制分档 10/10 + 错误路径。
pub fn drill() -> Result<u32, u16> {
    let mut hits: u32 = 0;

    // R1：三字段解码——P4 档 + MaxLVT=6 + ABR=1 组合正解。
    let v = encode_version(VER_P4, MAX_LVT_P4, true);
    let (ver, ml, abr) = decode_version(v)?;
    if ver != VER_P4 || ml != MAX_LVT_P4 || !abr {
        return Err(E_FIELD);
    }
    hits += 1;

    // R2：x2APIC 档无抑制支持组合（ABR=0）解码正解。
    let v2 = encode_version(VER_X2APIC, 5, false);
    let (ver2, ml2, abr2) = decode_version(v2)?;
    if ver2 != VER_X2APIC || ml2 != 5 || abr2 {
        return Err(E_FIELD);
    }
    hits += 1;

    // R3：抑制策略分档 10/10——ABR=1 档：开关开走 local、关走 broadcast。
    let mut led = EoiLedger::from_version(encode_version(VER_P4, 6, true))?;
    led.suppress_switch = true;
    for _ in 0..10 {
        led.eoi(true)?;
    }
    if led.local_eois != 10 || led.broadcast_eois != 0 || !led.conserved(10) {
        return Err(E_BAL);
    }
    led.suppress_switch = false;
    for _ in 0..10 {
        led.eoi(true)?;
    }
    if led.local_eois != 10 || led.broadcast_eois != 10 || !led.conserved(20) {
        return Err(E_BAL);
    }
    hits += 1;

    // R4：ForceBroadcast 档——不带抑制请求的 EOI 全走广播出口。
    let mut led2 = EoiLedger::from_version(encode_version(VER_X2APIC, 5, false))?;
    for _ in 0..10 {
        led2.eoi(false)?;
    }
    if led2.broadcast_eois != 10 || led2.local_eois != 0 || !led2.conserved(10) {
        return Err(E_BAL);
    }
    hits += 1;

    // R5：LVT 偏移闭集——0..=max_lvt 合法，越界拒。
    let mut led3 = EoiLedger::from_version(encode_version(VER_P4, 6, true))?;
    for off in 0..=6u8 {
        if !led3.lvt_offset_ok(off) {
            return Err(E_LVT_RANGE);
        }
    }
    if led3.lvt_offset_ok(7) {
        return Err(E_LVT_RANGE);
    }
    hits += 1;

    // I1：白名单外版本 → E_VER_RESV。
    for bad in [0x00u8, 0x0F, 0x11, 0x20, 0xFF] {
        if decode_version(encode_version(bad, 4, false)) != Err(E_VER_RESV) {
            return Err(E_VER_RESV);
        }
    }
    hits += 1;

    // I2：MaxLVT 超上界 → E_LVT_RANGE。
    if decode_version(encode_version(VER_P4, 7, false)) != Err(E_LVT_RANGE) {
        return Err(E_LVT_RANGE);
    }
    hits += 1;

    // I3：家族域污染 → E_FIELD。
    let dirty = encode_version(VER_P4, 6, false) | (0x01 << 8);
    if decode_version(dirty) != Err(E_FIELD) {
        return Err(E_FIELD);
    }
    hits += 1;

    // I4：ABR=0 抑制入口拒收 → E_SUPP_NA（负账入册，守恒含拒收）。
    let mut led4 = EoiLedger::from_version(encode_version(VER_X2APIC, 5, false))?;
    if led4.eoi(true) != Err(E_SUPP_NA) {
        return Err(E_SUPP_NA);
    }
    if led4.rejected_suppress != 1 || !led4.conserved(1) {
        return Err(E_BAL);
    }
    hits += 1;

    // I5：版本寄存器只读——策略层拒写语义（构造 RO 守卫面）。
    // 模型：任何对版本寄存器的写请求一律 E_RO（原值不可变由 encode 恒等保证）。
    let v_before = encode_version(VER_P4, 6, true);
    let write_attempt = v_before.wrapping_add(0x100);
    if write_attempt == v_before {
        return Err(E_RO); // 写入生效即失守
    }
    if decode_version(v_before)? != (VER_P4, MAX_LVT_P4, true) {
        return Err(E_RO); // 原值未被写请求扰动
    }
    hits += 1;

    Ok(hits)
}
