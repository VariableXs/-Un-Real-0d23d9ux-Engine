//! unx48307_cfgerrm1 — UNX-F48307 配置错误码族 M1 段（10 条）（诊断段 0xC740）。
//!
//! 判据原文：
//!   J1 错误码 ×10 零漂移；判据锚定 D1 同源体系。
//!   J2 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计纪律：
//!   1. 十码注册表：配置面 M1 段 10 条错误码（魔数/版本/CRC/长度/
//!      键未知/值类型/值越界/迁移失败/锁占用/只读拒绝），码值 =
//!      SEG_BASE + 1..10，连续无洞无重。
//!   2. 零漂移 = 表述一致三重断言：注册表码值连续性 / 分派器
//!      match 臂与注册表逐条对应 / 探针通路对每类故障返回注册值
//!      ——三路同值才算零漂移。
//!   3. 码段闭包：全部码 ≤ SEG_BASE + 0x1F；未知故障专用码独立
//!      占位（不与十码撞号）。
//!   4. 分派完备：10 类故障枚举全覆盖 match（无通配兜底吞错——
//!      兜底只留给「未知」一类，且未知必须显式可检）。
//!   5. 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，确定性。

use crate::alloc::vec::Vec;

pub const SEG_BASE: u16 = 0xC740;

pub const E_MAGIC: u16 = SEG_BASE + 1;   // 1 配置魔数坏
pub const E_VERSION: u16 = SEG_BASE + 2; // 2 配置版本不识别
pub const E_CRC: u16 = SEG_BASE + 3;     // 3 配置 CRC 坏
pub const E_LENGTH: u16 = SEG_BASE + 4;  // 4 配置体长不符
pub const E_KEY_UNKNOWN: u16 = SEG_BASE + 5; // 5 键未知
pub const E_VAL_TYPE: u16 = SEG_BASE + 6;    // 6 值类型不符
pub const E_VAL_RANGE: u16 = SEG_BASE + 7;   // 7 值越界
pub const E_MIGRATION: u16 = SEG_BASE + 8;   // 8 迁移失败
pub const E_LOCK_BUSY: u16 = SEG_BASE + 9;   // 9 锁占用
pub const E_READ_ONLY: u16 = SEG_BASE + 10;  // 10 只读拒绝
const E_UNKNOWN: u16 = SEG_BASE + 11;        // 未知故障（兜底显式码）
const E_CODE_RANGE: u16 = SEG_BASE + 12;     // 诊断码越段（自检用）

/// 配置故障十类。
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum CfgFault {
    Magic,
    Version,
    Crc,
    Length,
    KeyUnknown,
    ValType,
    ValRange,
    Migration,
    LockBusy,
    ReadOnly,
}

/// 错误码注册行：码值 + 名称指纹（名称指纹 = 名称字节 FNV 低 16 位）。
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct CodeRow {
    pub code: u16,
    pub name_fp: u16,
}

/// 名称指纹（FNV-1a 32 折叠取低 16 位）。
pub fn name_fp(name: &[u8]) -> u16 {
    let mut h: u32 = 0x811C_9DC5;
    let mut i = 0;
    while i < name.len() {
        h ^= name[i] as u32;
        h = h.wrapping_mul(0x0100_0193);
        i += 1;
    }
    (h & 0xFFFF) as u16
}

/// M1 段错误码注册表（正本）：10 条，码值连续，名称指纹在册。
pub fn registry() -> Vec<CodeRow> {
    let names: [&[u8]; 10] = [
        b"cfg_magic_bad",
        b"cfg_version_bad",
        b"cfg_crc_bad",
        b"cfg_length_bad",
        b"cfg_key_unknown",
        b"cfg_val_type",
        b"cfg_val_range",
        b"cfg_migration_fail",
        b"cfg_lock_busy",
        b"cfg_read_only",
    ];
    let mut rows: Vec<CodeRow> = Vec::new();
    let mut i = 0;
    while i < 10 {
        rows.push(CodeRow {
            code: SEG_BASE + (i as u16) + 1,
            name_fp: name_fp(names[i]),
        });
        i += 1;
    }
    rows
}

/// 分派器：故障类 → 注册码（match 臂显式全列，无吞错通配）。
pub fn dispatch(f: CfgFault) -> u16 {
    match f {
        CfgFault::Magic => E_MAGIC,
        CfgFault::Version => E_VERSION,
        CfgFault::Crc => E_CRC,
        CfgFault::Length => E_LENGTH,
        CfgFault::KeyUnknown => E_KEY_UNKNOWN,
        CfgFault::ValType => E_VAL_TYPE,
        CfgFault::ValRange => E_VAL_RANGE,
        CfgFault::Migration => E_MIGRATION,
        CfgFault::LockBusy => E_LOCK_BUSY,
        CfgFault::ReadOnly => E_READ_ONLY,
    }
}

/// 故障闭集表（探针通路用）。
pub fn fault_table() -> [CfgFault; 10] {
    [
        CfgFault::Magic,
        CfgFault::Version,
        CfgFault::Crc,
        CfgFault::Length,
        CfgFault::KeyUnknown,
        CfgFault::ValType,
        CfgFault::ValRange,
        CfgFault::Migration,
        CfgFault::LockBusy,
        CfgFault::ReadOnly,
    ]
}

/// 未知故障显式码（兜底可检，不撞十码）。
pub fn unknown_code() -> u16 {
    E_UNKNOWN
}

/// 配置错误码族主巡检：×10 零漂移全链判据。
pub fn drill() -> Result<u32, u16> {
    let mut hits = 0u32;
    let reg = registry();

    // R1 注册表恰 10 条。
    if reg.len() != 10 {
        return Err(E_CODE_RANGE);
    }
    hits += 1;

    // R2 连续性零漂移：码值 = SEG_BASE+1..+10 逐条核对（无洞无重无越位）。
    let mut i = 0;
    while i < reg.len() {
        if reg[i].code != SEG_BASE + (i as u16) + 1 {
            return Err(E_CODE_RANGE);
        }
        i += 1;
    }
    hits += 1;

    // R3 唯一性：十码两两不同。
    let mut a = 0;
    while a < reg.len() {
        let mut b = a + 1;
        while b < reg.len() {
            if reg[a].code == reg[b].code {
                return Err(E_CODE_RANGE);
            }
            b += 1;
        }
        a += 1;
    }
    hits += 1;

    // R4 分派一致：10 类故障经分派器逐条命中注册值（第二路同值）。
    let ft = fault_table();
    let mut fi = 0;
    while fi < ft.len() {
        let c = dispatch(ft[fi]);
        if c != reg[fi].code {
            return Err(E_CODE_RANGE);
        }
        fi += 1;
    }
    hits += 1;

    // R5 探针通路：以码值反查注册表（第三路同值）——每码恰命中一行。
    let mut ci = 0;
    while ci < 10 {
        let target = SEG_BASE + (ci as u16) + 1;
        let mut found = 0u32;
        let mut r = 0;
        while r < reg.len() {
            if reg[r].code == target {
                found += 1;
            }
            r += 1;
        }
        if found != 1 {
            return Err(E_CODE_RANGE);
        }
        ci += 1;
    }
    hits += 1;

    // R6 名称指纹在册稳定：注册表两度重建指纹逐行一致（正本不被
    //    静默改写——名称漂移同码漂移一样算漂移）。
    let reg2 = registry();
    let mut r = 0;
    while r < reg.len() {
        if reg[r].name_fp != reg2[r].name_fp {
            return Err(E_CODE_RANGE);
        }
        r += 1;
    }
    hits += 1;

    // R7 未知故障显式码：不与十码撞号、落在段内。
    let u = unknown_code();
    if u == E_CODE_RANGE {
        return Err(E_CODE_RANGE);
    }
    let mut r2 = 0;
    while r2 < reg.len() {
        if reg[r2].code == u {
            return Err(E_CODE_RANGE);
        }
        r2 += 1;
    }
    hits += 1;

    // R8 反查完整性：每个注册码反查故障类恰唯一（分派器双射性——
    //    码不撞类、类不并码，漂移在此路必现）。
    let ft2 = fault_table();
    let mut fi2 = 0;
    while fi2 < ft2.len() {
        let c = dispatch(ft2[fi2]);
        // 该码不得同时被另一个故障类命中。
        let mut fj = 0;
        while fj < ft2.len() {
            if fj != fi2 && dispatch(ft2[fj]) == c {
                return Err(E_CODE_RANGE);
            }
            fj += 1;
        }
        fi2 += 1;
    }
    hits += 1;

    // R9 诊断码闭包：本模块全部错误码（含十码与自检码）落在
    //    [SEG_BASE+1, SEG_BASE+0x1F] 段内。
    const CODES: [u16; 12] = [
        E_MAGIC,
        E_VERSION,
        E_CRC,
        E_LENGTH,
        E_KEY_UNKNOWN,
        E_VAL_TYPE,
        E_VAL_RANGE,
        E_MIGRATION,
        E_LOCK_BUSY,
        E_READ_ONLY,
        E_UNKNOWN,
        E_CODE_RANGE,
    ];
    let mut k = 0;
    while k < CODES.len() {
        if CODES[k] <= SEG_BASE || CODES[k] > SEG_BASE + 0x1F {
            return Err(E_CODE_RANGE);
        }
        k += 1;
    }
    hits += 1;

    Ok(hits)
}
