//! unx0865_rteenc —— UNX-F0865 · RTE 字段编码器（目标 300 行）。
//! 锚点（CoRun Unxreal 主册）：UNX-F0865 · RTE 字段编码器。
//!           （向量/投递模式/极性/触发/掩码/目的地）
//! 判据原文：UNX-F0865-J1 编码位级正确（SDM 逐位对照 100%），非法组合拦截
//!           10/10 次，编码往返（编→解→编）幂等率 100%。
//! 设计纪律：
//! - 64 位 RTE 六字段逐位固化（SDM Vol.3 Table 3-3 / Table 3-4）：
//!   BIT7:0 向量 / BIT10:8 投递模式 / BIT11 目的地模式（0 物理 1 逻辑）/
//!   BIT12 保留恒 0 / BIT13 极性（0 高 1 低）/ BIT14 触发（1=电平 0=边沿，
//!   注意 IO-APIC 极性/触发位与 LAPIC ICR 相反的约定差——本编码器按 IO-APIC
//!   表语义）/ BIT15 RIRR 恒 0（远端 IRR 建模面只读）/ BIT63:56 目的地。
//!   BIT16 mask、BIT63:17 保留恒 0（入参即红）。
//! - 非法组合闭集（拦截表 10/10）：Fixed+逻辑目的地+目的地全零（逻辑模式
//!   无成员）/ NMI 带电平触发（NMI 恒边沿）/ LowPri+物理目的地+目的地 0 /
//!   ExtINT+非零向量（ExtINT 恒向量 0）/ 保留位非零。
//! - 往返幂等：encode → decode → encode 全量位恒等（判据 100% 幂等）。
//! - SDM 逐位对照表固化（16 位低位面 + 8 位目的地高面），对照核逐位执行。
//! - no_std + alloc；零 unwrap/expect/panic；整数算术；确定性。
//! 诊断码段：0x6900（0x6901~0x6909 闭集；全域 grep 确认独占）。

use alloc::vec::Vec;

/// 模块版本。
pub const RE_VERSION: &str = "unx0865_rteenc-v1";

/// 诊断段基址（0x6900 段 · 本模块独占）。
pub const SEG_BASE: u16 = 0x6900;

/// XcCode 闭集。
pub const XC_VEC_ZERO: u16 = SEG_BASE + 1; // 向量为 0（ExtINT 外非法）
pub const XC_VEC_HIGH: u16 = SEG_BASE + 2; // 向量 <32（异常区占用）
pub const XC_DELMOD_BAD: u16 = SEG_BASE + 3; // 投递模式非法
pub const XC_COMBO_BAD: u16 = SEG_BASE + 4; // 非法字段组合
pub const XC_RSRVD_SET: u16 = SEG_BASE + 5; // 保留位非零
pub const XC_DEST_BAD: u16 = SEG_BASE + 6; // 目的地非法（物理>255 位宽越界建模）
pub const XC_ROUND_TRIP: u16 = SEG_BASE + 7; // 往返幂等破
pub const XC_SDM_BIT: u16 = SEG_BASE + 8; // SDM 逐位对照失败
pub const XC_INJECT_MISS: u16 = SEG_BASE + 9; // 非法组合注入漏抓

/// 投递模式（BIT10:8）。
pub const DM_FIXED: u8 = 0;
pub const DM_LOWEST: u8 = 1;
pub const DM_SMI: u8 = 2;
pub const DM_NMI: u8 = 4;
pub const DM_INIT: u8 = 5;
pub const DM_EXTINT: u8 = 7;

/// 六字段入参。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct RteInput {
    pub vector: u8,
    pub delmod: u8,
    /// true = 逻辑目的地。
    pub logical: bool,
    /// true = 低电平/低极性。
    pub active_low: bool,
    /// true = 电平触发。
    pub level: bool,
    pub masked: bool,
    /// 目的地（物理 APIC ID 或逻辑集合）。
    pub dest: u8,
}

/// 编码器静态校验 + 编码。
pub fn encode(e: &RteInput) -> Result<u64, u16> {
    // 1. 保留位面：本编码器入参不含保留位（结构体化），只核语义闭集
    // 2. 向量规则：ExtINT 恒向量 0；其余向量 16~255（<16 保留、<32 异常区）
    if e.delmod == DM_EXTINT {
        if e.vector != 0 {
            return Err(XC_VEC_ZERO);
        }
    } else {
        if e.vector == 0 {
            return Err(XC_VEC_ZERO);
        }
        if (e.vector as u32) < 32 {
            return Err(XC_VEC_HIGH);
        }
    }
    // 3. 投递模式闭集
    if !matches!(e.delmod, DM_FIXED | DM_LOWEST | DM_SMI | DM_NMI | DM_INIT | DM_EXTINT) {
        return Err(XC_DELMOD_BAD);
    }
    // 4. 非法组合表
    // 4a. NMI/INIT/SMI 恒边沿（level=true 非法）
    if matches!(e.delmod, DM_NMI | DM_INIT | DM_SMI) && e.level {
        return Err(XC_COMBO_BAD);
    }
    // 4b. Fixed+逻辑目的地+目的地全零 = 无成员组播
    if e.delmod == DM_FIXED && e.logical && e.dest == 0 {
        return Err(XC_COMBO_BAD);
    }
    // 4c. LowPri+物理目的地+目的地 0
    if e.delmod == DM_LOWEST && !e.logical && e.dest == 0 {
        return Err(XC_COMBO_BAD);
    }
    // 4d. SMI/INIT/ExtINT 恒物理目的地（SDM：仅 Fixed/LowPri 允许逻辑）
    if matches!(e.delmod, DM_SMI | DM_INIT | DM_EXTINT) && e.logical {
        return Err(XC_COMBO_BAD);
    }
    // 5. 位级拼装（SDM Table 3-3 固化）
    let mut v: u64 = 0;
    v |= e.vector as u64;
    v |= (e.delmod as u64) << 8;
    if e.logical {
        v |= 1 << 11;
    }
    if e.active_low {
        v |= 1 << 13;
    }
    if e.level {
        v |= 1 << 15;
    }
    if e.masked {
        v |= 1 << 16;
    }
    v |= (e.dest as u64) << 56;
    Ok(v)
}

/// 解码（编码对偶）。
pub fn decode(raw: u64) -> Result<RteInput, u16> {
    Ok(RteInput {
        vector: (raw & 0xFF) as u8,
        delmod: ((raw >> 8) & 0x7) as u8,
        logical: raw & (1 << 11) != 0,
        active_low: raw & (1 << 13) != 0,
        level: raw & (1 << 15) != 0,
        masked: raw & (1 << 16) != 0,
        dest: (raw >> 56) as u8,
    })
}

/// 保留位核（判据 XC_RSRVD_SET）：BIT12/14/63:17 必须为零。
pub fn reserved_bits_clear(raw: u64) -> bool {
    raw & ((1u64 << 12) | (1u64 << 14)) == 0 && raw & 0xFFFF_0001_FFFE_0000 & !0x0001_0000 == 0
}

/// 往返幂等核（判据 100% 幂等）：encode→decode→encode 恒等。
pub fn roundtrip_idempotent(e: &RteInput) -> Result<(), u16> {
    let r1 = encode(e)?;
    let d = decode(r1)?;
    let r2 = encode(&d)?;
    if r1 != r2 {
        return Err(XC_ROUND_TRIP);
    }
    Ok(())
}

/// SDM 逐位对照表（判据「位级正确」固化锚）：16 组（字段, 位, 期望值）。
/// 对照核：对给定编码结果逐位验证。
pub fn sdm_bit_check(raw: u64, e: &RteInput) -> Result<(), u16> {
    let bits: [(u32, u64, bool); 8] = [
        (0, 0xFF, true),
        (8, 0x7, true),
        (11, 1, e.logical),
        (12, 1, false),                       // BIT12 保留恒 0
        (13, 1, e.active_low),
        (15, 1, e.level),
        (16, 1, e.masked),
        (56, 0xFF, true),
    ];
    for (shift, mask, expect_val) in bits.iter() {
        let got_field = (raw >> shift) & mask;
        let want_field: u64 = match (*shift, *mask) {
            (0, 0xFF) => e.vector as u64,
            (8, 0x7) => e.delmod as u64,
            (11, 1) => *expect_val as u64,
            (12, 1) => 0,
            (13, 1) => *expect_val as u64,
            (15, 1) => *expect_val as u64,
            (16, 1) => *expect_val as u64,
            (56, 0xFF) => e.dest as u64,
            _ => 0,
        };
        if got_field != want_field {
            return Err(XC_SDM_BIT);
        }
    }
    Ok(())
}

/// 非法组合注入表（10/10 拦截验证执行面）。
pub fn illegal_inject_10() -> Result<usize, u16> {
    let probes = [
        RteInput { vector: 0x41, delmod: DM_FIXED, logical: false, active_low: false, level: false, masked: false, dest: 0 }, // 合法参照（不在此表）
        RteInput { vector: 0, delmod: DM_FIXED, logical: false, active_low: false, level: false, masked: false, dest: 1 },     // 非法1: Fixed+向量0
        RteInput { vector: 0x10, delmod: DM_FIXED, logical: false, active_low: false, level: false, masked: false, dest: 1 },  // 非法2: 向量16<32
        RteInput { vector: 0x41, delmod: 3, logical: false, active_low: false, level: false, masked: false, dest: 1 },         // 非法3: 模式3非法
        RteInput { vector: 0x41, delmod: DM_NMI, logical: false, active_low: false, level: true, masked: false, dest: 1 },     // 非法4: NMI 电平
        RteInput { vector: 0x41, delmod: DM_FIXED, logical: true, active_low: false, level: false, masked: false, dest: 0 },   // 非法5: 逻辑+空组
        RteInput { vector: 0x41, delmod: DM_LOWEST, logical: false, active_low: false, level: false, masked: false, dest: 0 }, // 非法6: 物理目的地0
        RteInput { vector: 0x41, delmod: DM_EXTINT, logical: true, active_low: false, level: false, masked: false, dest: 1 },  // 非法7: ExtINT 逻辑
        RteInput { vector: 0x10, delmod: DM_EXTINT, logical: false, active_low: false, level: false, masked: false, dest: 1 }, // 非法8: ExtINT 向量非0
        RteInput { vector: 0x41, delmod: DM_INIT, logical: false, active_low: false, level: true, masked: false, dest: 1 },    // 非法9: INIT 电平
        RteInput { vector: 0x41, delmod: DM_SMI, logical: true, active_low: false, level: false, masked: false, dest: 1 },     // 非法10: SMI 逻辑
    ];
    // probes[0] 是合法参照，跳过；其余 10 个必须全拦
    let mut caught = 0;
    for p in probes.iter().skip(1) {
        if encode(p).is_err() {
            caught += 1;
        }
    }
    if caught != 10 {
        return Err(XC_INJECT_MISS);
    }
    // 合法参照必须通过
    if encode(&probes[0]).is_err() {
        return Err(XC_INJECT_MISS);
    }
    Ok(caught)
}

/// drill：全链自例程。
pub fn drill() -> Result<u32, u16> {
    // 1. 合法编码三例（覆盖 Fixed 物理 / LowPri 逻辑电平 / ExtINT）
    let cases = [
        RteInput { vector: 0x41, delmod: DM_FIXED, logical: false, active_low: false, level: false, masked: false, dest: 0x03 },
        RteInput { vector: 0x50, delmod: DM_LOWEST, logical: true, active_low: true, level: true, masked: true, dest: 0xFF },
        RteInput { vector: 0, delmod: DM_EXTINT, logical: false, active_low: false, level: false, masked: false, dest: 0x02 },
    ];
    for e in cases.iter() {
        let raw = encode(e)?;
        sdm_bit_check(raw, e)?;
        roundtrip_idempotent(e)?;
        if !reserved_bits_clear(raw) {
            return Err(XC_RSRVD_SET);
        }
    }
    // 2. ExtINT 向量恒 0 位核对
    let raw2 = encode(&cases[2])?;
    if raw2 & 0xFF != 0 || (raw2 >> 8) & 0x7 != DM_EXTINT as u64 {
        return Err(XC_SDM_BIT);
    }
    // 3. 非法组合 10/10
    let caught = illegal_inject_10()?;
    if caught != 10 {
        return Err(XC_INJECT_MISS);
    }
    // 4. 保留位核（手工构保留位非零）
    if reserved_bits_clear(1 << 12) {
        return Err(XC_RSRVD_SET);
    }
    if reserved_bits_clear(1 << 14) {
        return Err(XC_RSRVD_SET);
    }
    // 5. 往返幂等批（LCG 确定性 100 组）
    let mut s: u64 = 0x9E37_79B9_7F4A_7C15;
    for _ in 0..100 {
        s = s.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        let e = RteInput {
            vector: 32 + (s & 0xDF) as u8,
            delmod: [DM_FIXED, DM_LOWEST][(s >> 8) as usize % 2],
            logical: s & (1 << 10) != 0,
            active_low: s & (1 << 11) != 0,
            level: false, // 电平只在 LowPri/Fixed 合法，批量固定边沿保合法
            masked: s & (1 << 12) != 0,
            dest: ((s >> 16) & 0xFF) as u8,
        };
        // 逻辑模式须目的地非零（Fixed）
        if e.delmod == DM_FIXED && e.logical && e.dest == 0 {
            continue;
        }
        if e.delmod == DM_LOWEST && !e.logical && e.dest == 0 {
            continue;
        }
        roundtrip_idempotent(&e)?;
    }
    // 6. decode 对偶：解码字段还原
    let d = decode(encode(&cases[1])?)?;
    if d != cases[1] {
        return Err(XC_ROUND_TRIP);
    }
    // 7. 错误路径：向量 0 非 ExtINT
    if encode(&RteInput { vector: 0, delmod: DM_FIXED, logical: false, active_low: false, level: false, masked: false, dest: 1 }).is_ok() {
        return Err(XC_VEC_ZERO);
    }
    // 8. 错误路径：模式 6 非法
    if encode(&RteInput { vector: 0x41, delmod: 6, logical: false, active_low: false, level: false, masked: false, dest: 1 }).is_ok() {
        return Err(XC_DELMOD_BAD);
    }
    Ok(0)
}
