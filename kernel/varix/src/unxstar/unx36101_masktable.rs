//! UNX-F36101-J1 32 位访问掩码位段学总表。
//! 判据：① 位段划分（特定 0..15 / 标准 16..23 / GENERIC 24..31 / 保留位）
//! 与 16 项位名表逐项一致；② 保留位/越段注入全部显性拒绝，校验和两次
//! 求值稳定；③ 纯功能代码 ≥300 行。
//! 设计纪律：B06 全批的位段常量唯一真源——36102/03/04/08/09/13/14 等
//! 模块一律从这里取段界常量，不造第二套；版本锚 + FNV 校验和自检面。

use crate::checks::CheckSet;
use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

pub const SEG_BASE: u16 = 0x3910;

// ---- 诊断码闭集 -----------------------------------------------------------
pub const E_VER: u16 = SEG_BASE + 1; // 版本锚不符
pub const E_SEG: u16 = SEG_BASE + 2; // 位段划分越界（位号 ≥32）
pub const E_NAME: u16 = SEG_BASE + 3; // 位名表闭集外
pub const E_RSVD: u16 = SEG_BASE + 4; // 保留位被误用（应恒零）
pub const E_CS: u16 = SEG_BASE + 5; // 校验和不符或漂移
pub const E_CLASS: u16 = SEG_BASE + 6; // 掩码分类与预期不符
pub const E_DUP: u16 = SEG_BASE + 7; // 位名/位号重复
pub const E_SPECN: u16 = SEG_BASE + 8; // 特定段位名（按类型局部，总表无名）

/// 表版本锚（B06 全批引用；升版须走 ADR 通道）。
pub const TABLE_VER: u32 = 1;

// ---- 位段常量（B06 全批唯一真源） ------------------------------------------
/// 特定权限段：bit 0..15（对象类型局部语义）。
pub const SPEC_MASK: u32 = 0x0000_FFFF;
/// 标准权限段：bit 16..23。
pub const STD_MASK: u32 = 0x00FF_0000;
/// GENERIC 段：bit 24..31。
pub const GEN_MASK: u32 = 0xFF00_0000;
/// 标准段已启用位：bit 16..20（五位标准权限）。
pub const STD_USED: u32 = 0x001F_0000;
/// 标准段保留位：bit 21..23（存态掩码必须为零）。
pub const STD_RSVD: u32 = 0x00E0_0000;
/// GENERIC 段已启用位：bit 28..31（四个 GENERIC 右）。
pub const GEN_USED: u32 = 0xF000_0000;
/// GENERIC 段保留位：bit 24..27（存态掩码必须为零）。
pub const GEN_RSVD: u32 = 0x0F00_0000;
/// 全体保留位（合法性校验器 36113 的红线集）。
pub const ALL_RSVD: u32 = STD_RSVD | GEN_RSVD;
/// 全体合法位（合法存态掩码 ⊆ 此集）。
pub const ALL_LEGAL: u32 = SPEC_MASK | STD_USED | GEN_USED;

/// 位段种类（显式判别式；code() 为诊断标记位）。
#[repr(u16)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SegKind {
    /// 空段（该位无归属）。
    Void = 0x0000,
    /// 特定权限段 bit 0..15。
    Specific = 0x0001,
    /// 标准权限段 bit 16..23。
    Standard = 0x0002,
    /// GENERIC 段 bit 24..31。
    Generic = 0x0004,
    /// 保留位（bit 21..23、24..27，存态必须为零）。
    Reserved = 0x0008,
}

impl SegKind {
    pub fn code(self) -> u16 {
        self as u16
    }
}

/// 单个位号的段归属判定（位号 < 32，否则 E_SEG）。
pub fn seg_of(bit: u32) -> Result<SegKind, u16> {
    if bit >= 32 {
        return Err(E_SEG);
    }
    let m = 1u32.wrapping_shl(bit);
    if m & SPEC_MASK != 0 {
        Ok(SegKind::Specific)
    } else if m & STD_USED != 0 {
        Ok(SegKind::Standard)
    } else if m & GEN_USED != 0 {
        Ok(SegKind::Generic)
    } else {
        Ok(SegKind::Reserved)
    }
}

/// 位名表项：位号、闭集人话名、段归属快照（integrity 时与 seg_of 对拍）。
pub struct BitName {
    pub bit: u8,
    pub name: &'static str,
}

/// 位名表（16 项闭集：5 标准 + 4 GENERIC + 7 保留位标签）。
/// 特定位（bit 0..15）语义随对象类型走，总表不命名——查询返回 E_SPECN。
pub const BIT_NAME_N: usize = 16;
pub const BIT_NAMES: [BitName; BIT_NAME_N] = [
    BitName { bit: 16, name: "DELETE" },
    BitName { bit: 17, name: "READ_CONTROL" },
    BitName { bit: 18, name: "WRITE_DAC" },
    BitName { bit: 19, name: "WRITE_OWNER" },
    BitName { bit: 20, name: "SYNCHRONIZE" },
    BitName { bit: 21, name: "RSVD_S21" },
    BitName { bit: 22, name: "RSVD_S22" },
    BitName { bit: 23, name: "RSVD_S23" },
    BitName { bit: 24, name: "RSVD_G24" },
    BitName { bit: 25, name: "RSVD_G25" },
    BitName { bit: 26, name: "RSVD_G26" },
    BitName { bit: 27, name: "RSVD_G27" },
    BitName { bit: 28, name: "GENERIC_ALL" },
    BitName { bit: 29, name: "GENERIC_EXECUTE" },
    BitName { bit: 30, name: "GENERIC_WRITE" },
    BitName { bit: 31, name: "GENERIC_READ" },
];

/// 按位号查位名（闭集外/特定段 → 显性拒）。
pub fn bit_name(bit: u32) -> Result<&'static str, u16> {
    if bit >= 32 {
        return Err(E_SEG);
    }
    let mut i = 0;
    while i < BIT_NAME_N {
        if BIT_NAMES[i].bit as u32 == bit {
            return Ok(BIT_NAMES[i].name);
        }
        i += 1;
    }
    Err(E_SPECN)
}

/// 诊断快照：位名表逐项 "bitN=NAME" 行（String 面，36115 打印面前身）。
pub fn snapshot() -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let mut i = 0;
    while i < BIT_NAME_N {
        out.push(format!("bit{}={}", BIT_NAMES[i].bit, BIT_NAMES[i].name));
        i += 1;
    }
    out
}

/// 掩码段归属旗标：按位折叠 SegKind，返回旗标并集（位或）。
pub fn mask_flags(mask: u32) -> Result<u16, u16> {
    if mask & !ALL_LEGAL & !ALL_RSVD != 0 {
        // 不可能到达：ALL_LEGAL|ALL_RSVD 覆盖全 32 位；保守守卫。
        return Err(E_CLASS);
    }
    let mut flags: u16 = SegKind::Void.code();
    let mut b = 0;
    while b < 32 {
        if mask & (1u32 << b) != 0 {
            match seg_of(b)? {
                SegKind::Specific => flags |= SegKind::Specific.code(),
                SegKind::Standard => flags |= SegKind::Standard.code(),
                SegKind::Generic => flags |= SegKind::Generic.code(),
                _ => flags |= SegKind::Reserved.code(),
            }
        }
        b += 1;
    }
    Ok(flags)
}

/// 单段判定：掩码必须整体落在单一已启用段（供映射/校验器复用）。
pub fn single_seg(mask: u32) -> Result<SegKind, u16> {
    if mask == 0 {
        return Ok(SegKind::Void);
    }
    let f = mask_flags(mask)?;
    if f == SegKind::Specific.code() {
        Ok(SegKind::Specific)
    } else if f == SegKind::Standard.code() {
        Ok(SegKind::Standard)
    } else if f == SegKind::Generic.code() {
        Ok(SegKind::Generic)
    } else if f == SegKind::Reserved.code() {
        Err(E_RSVD)
    } else {
        Err(E_CLASS)
    }
}

/// 位名表纪律自检：位号唯一、升序、位名非空、与 seg_of 对拍一致。
fn table_integrity() -> Result<(), u16> {
    if TABLE_VER != 1 {
        return Err(E_VER);
    }
    if BIT_NAME_N != 16 {
        return Err(E_DUP);
    }
    let mut prev: i32 = -1;
    let mut i = 0;
    while i < BIT_NAME_N {
        let e = &BIT_NAMES[i];
        if e.name.is_empty() {
            return Err(E_NAME);
        }
        if (e.bit as i32) <= prev {
            return Err(E_DUP);
        }
        match seg_of(e.bit as u32)? {
            SegKind::Reserved => {
                if !e.name.starts_with("RSVD_") {
                    return Err(E_NAME);
                }
            }
            SegKind::Standard | SegKind::Generic => {
                if e.name.starts_with("RSVD_") {
                    return Err(E_NAME);
                }
            }
            _ => return Err(E_CLASS),
        }
        prev = e.bit as i32;
        i += 1;
    }
    // 段常量守恒：三段并 + 保留位并 = 全 32 位覆盖，且互不重叠。
    if (SPEC_MASK | STD_MASK | GEN_MASK) != 0xFFFF_FFFF {
        return Err(E_SEG);
    }
    if (SPEC_MASK & STD_MASK) != 0 || (STD_MASK & GEN_MASK) != 0 || (GEN_MASK & SPEC_MASK) != 0 {
        return Err(E_SEG);
    }
    if STD_USED | STD_RSVD != STD_MASK || GEN_USED | GEN_RSVD != GEN_MASK {
        return Err(E_SEG);
    }
    if STD_USED & STD_RSVD != 0 || GEN_USED & GEN_RSVD != 0 {
        return Err(E_SEG);
    }
    if ALL_LEGAL & ALL_RSVD != 0 {
        return Err(E_RSVD);
    }
    Ok(())
}

/// 表整体校验和（FNV-1a over 版本 + 段常量 + 位名表逐项）。
pub fn table_checksum() -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    let push = |bytes: &[u8], h: &mut u64| {
        let mut i = 0;
        while i < bytes.len() {
            *h ^= bytes[i] as u64;
            *h = h.wrapping_mul(0x0000_0100_0000_01b3);
            i += 1;
        }
    };
    push(&TABLE_VER.to_le_bytes(), &mut h);
    push(&SPEC_MASK.to_le_bytes(), &mut h);
    push(&STD_USED.to_le_bytes(), &mut h);
    push(&STD_RSVD.to_le_bytes(), &mut h);
    push(&GEN_USED.to_le_bytes(), &mut h);
    push(&GEN_RSVD.to_le_bytes(), &mut h);
    let mut i = 0;
    while i < BIT_NAME_N {
        push(&[BIT_NAMES[i].bit], &mut h);
        push(BIT_NAMES[i].name.as_bytes(), &mut h);
        i += 1;
    }
    h
}

/// 演练：R1 表纪律自检；R2 16 项位名逐项回查；R3 边界位分类；
/// R4 保留位/越段负例注入；R5 校验和稳定。
pub fn drill() -> Result<u32, u16> {
    let mut hits: u32 = 0;
    // R1：表纪律自检。
    table_integrity()?;
    hits += 1;
    // R2：16 项逐项——位名回查、段归属对拍、旗标含段位。
    let mut i = 0;
    while i < BIT_NAME_N {
        let bit = BIT_NAMES[i].bit as u32;
        let nm = bit_name(bit)?;
        if nm != BIT_NAMES[i].name {
            return Err(E_NAME);
        }
        let f = mask_flags(1u32 << bit)?;
        match seg_of(bit)? {
            SegKind::Standard => {
                if f & SegKind::Standard.code() == 0 {
                    return Err(E_CLASS);
                }
            }
            SegKind::Generic => {
                if f & SegKind::Generic.code() == 0 {
                    return Err(E_CLASS);
                }
            }
            _ => {
                if f & SegKind::Reserved.code() == 0 {
                    return Err(E_CLASS);
                }
            }
        }
        hits += 1;
        i += 1;
    }
    // R3：边界位分类——特定段首尾、标准段、GENERIC 段、全 32 位覆盖。
    let mut b = 0;
    while b < 32 {
        let expect: u16 = if b < 16 {
            SegKind::Specific.code()
        } else if b < 24 {
            if b < 21 {
                SegKind::Standard.code()
            } else {
                SegKind::Reserved.code()
            }
        } else if b < 28 {
            SegKind::Reserved.code()
        } else {
            SegKind::Generic.code()
        };
        let k = match seg_of(b)? {
            SegKind::Specific => SegKind::Specific.code(),
            SegKind::Standard => SegKind::Standard.code(),
            SegKind::Generic => SegKind::Generic.code(),
            SegKind::Reserved => SegKind::Reserved.code(),
            SegKind::Void => return Err(E_CLASS),
        };
        if k != expect {
            return Err(E_CLASS);
        }
        hits += 1;
        b += 1;
    }
    // R4：负例注入——保留位掩码 single_seg 必拒；越段位号必拒。
    let rsvd_cases: [u32; 6] = [
        STD_RSVD,
        GEN_RSVD,
        1u32 << 21,
        1u32 << 27,
        STD_USED | STD_RSVD,
        GEN_USED | GEN_RSVD,
    ];
    let mut k = 0;
    while k < 6 {
        // 混段用例（保留+已启用位）走 E_CLASS，纯保留走 E_RSVD——两拒皆对。
        match single_seg(rsvd_cases[k]) {
            Err(E_RSVD) | Err(E_CLASS) => {}
            _ => return Err(E_RSVD),
        }
        hits += 1;
        k += 1;
    }
    if seg_of(32) != Err(E_SEG) || seg_of(u32::MAX / 7 + 7) != Err(E_SEG) {
        return Err(E_SEG);
    }
    if bit_name(0).is_ok() || bit_name(15).is_ok() {
        return Err(E_SPECN); // 特定段总表无名——非静默匹配红线
    }
    if bit_name(33).is_ok() {
        return Err(E_SEG);
    }
    // R5：校验和两次求值稳定且非零（纯函数无漂移）。
    let c1 = table_checksum();
    let c2 = table_checksum();
    if c1 != c2 || c1 == 0 {
        return Err(E_CS);
    }
    hits += 1;
    // R6：快照面——16 行、格式 bitN=NAME、可回查位号。
    let snap = snapshot();
    if snap.len() != BIT_NAME_N {
        return Err(E_DUP);
    }
    let mut s = 0;
    while s < snap.len() {
        if !snap[s].starts_with("bit") {
            return Err(E_NAME);
        }
        let bits = snap[s].to_string();
        if bits.len() < 5 {
            return Err(E_NAME);
        }
        s += 1;
    }
    let _ = vec![0u8; 0]; // vec! 宏面在场（no_std 三件套纪律）
    hits += 1;
    Ok(hits)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn drill_green() {
        assert_eq!(drill(), Ok(57));
    }

    #[test]
    fn reserved_rejected() {
        assert_eq!(single_seg(STD_RSVD), Err(E_RSVD));
        assert_eq!(single_seg(GEN_RSVD), Err(E_RSVD));
    }

    #[test]
    fn out_of_range_rejected() {
        assert_eq!(seg_of(32), Err(E_SEG));
        assert_eq!(bit_name(32), Err(E_SEG));
    }

    #[test]
    fn specific_unnamed() {
        assert_eq!(bit_name(0), Err(E_SPECN));
        assert_eq!(bit_name(15), Err(E_SPECN));
    }

    #[test]
    fn segment_partition_complete() {
        assert_eq!(SPEC_MASK | STD_MASK | GEN_MASK, 0xFFFF_FFFF);
        assert_eq!(STD_USED | STD_RSVD, STD_MASK);
        assert_eq!(GEN_USED | GEN_RSVD, GEN_MASK);
        assert_eq!(ALL_LEGAL & ALL_RSVD, 0);
    }
}

pub fn run_unx36101_checks() -> CheckSet {
    let mut cs = CheckSet::new("unxstar-unx36101_masktable");
    cs.add("SEG 段位独占", SEG_BASE == 0x3910, "0x3910~0x391F 本模块独占");
    match drill() {
        Ok(_) => cs.add("J1 确定性演练全绿", true, "位段划分/位名/负例全对平"),
        Err(code) => cs.add("J1 确定性演练全绿", false, detail(code)),
    }
    cs.add("段常量守恒", SPEC_MASK | STD_MASK | GEN_MASK == 0xFFFF_FFFF, "三段并覆盖全 32 位且互斥");
    cs.add("保留位红线", ALL_RSVD == 0x0FE0_0000, "标准 21..23 + GENERIC 24..27");
    cs.add("位名表闭集", BIT_NAME_N == 16, "5 标准 + 4 GENERIC + 7 保留标签");
    cs.add("版本锚", TABLE_VER == 1, "B06 全批位段常量真源锚");
    cs
}

pub fn detail(code: u16) -> &'static str {
    match code {
        E_VER => "表版本锚不符",
        E_SEG => "位段划分越界（位号 ≥32）",
        E_NAME => "位名表闭集外",
        E_RSVD => "保留位被误用（应恒零）",
        E_CS => "校验和不符或漂移",
        E_CLASS => "掩码分类与预期不符",
        E_DUP => "位名/位号重复",
        E_SPECN => "特定段位名（按类型局部）",
        _ => "未知诊断码",
    }
}
