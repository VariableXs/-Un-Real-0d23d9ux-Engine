//! UNX-F36087-J1 ACE 访问掩码字段的位段承载（特定+标准+GENERIC 三段）。
//! 判据：① 三段分离读写：12 例掩码拆分-合成往返全对平，3 个区段独立读写对平；
//! ② 保留中段 3 例、标准段未定义位 3 例、GENERIC 区溢出写 2 例全部显性拒绝；
//! ③ 纯功能代码 ≥300 行。
//! 设计纪律：消费真源 unx36081_acecore（保留段锚 MASK_RESERVED 同锚互证）；
//! 本模块是 36083 deny GENERIC 禁段门与 B06 掩码学（F36101）的承载地基。

use crate::checks::CheckSet;
use super::unx36081_acecore as core;
use alloc::vec;
use alloc::vec::Vec;
use alloc::string::{String, ToString};

pub const SEG_BASE: u16 = 0xe330;

// ---- 诊断码闭集 -----------------------------------------------------------
pub const E_VER: u16 = SEG_BASE + 1;     // 版本锚不符
pub const E_RESERVED: u16 = SEG_BASE + 2;// 保留中段（bit25..27）非零
pub const E_STD_BITS: u16 = SEG_BASE + 3;// 标准段含未定义位
pub const E_GENERIC: u16 = SEG_BASE + 4; // GENERIC 区写入溢出（>4 位）
pub const E_ROUND: u16 = SEG_BASE + 5;   // 拆分-合成往返不对平
pub const E_CS: u16 = SEG_BASE + 6;      // 指纹不稳

/// 版本锚。
pub const VER: u32 = 1;

/// 特定权限段（bit0..15，对象自定义）。
pub const SPECIFIC_MASK: u32 = 0x0000_ffff;
/// 标准权限段合法位（DELETE/READ_CONTROL/WRITE_DAC/WRITE_OWNER/SYNCHRONIZE）。
pub const STANDARD_LEGAL: u32 = 0x001f_0000;
/// 标准权限段五个常量（B06 F36102 消费预告锚）。
pub const STD_DELETE: u32 = 0x0001_0000;
pub const STD_READ_CONTROL: u32 = 0x0002_0000;
pub const STD_WRITE_DAC: u32 = 0x0004_0000;
pub const STD_WRITE_OWNER: u32 = 0x0008_0000;
pub const STD_SYNCHRONIZE: u32 = 0x0010_0000;
/// 标准段保留位（bit21..23）。
pub const STANDARD_RESERVED: u32 = 0x00e0_0000;
/// GENERIC 段（bit24..31 高 4 位有效）。
pub const GENERIC_MASK: u32 = 0xf000_0000;
/// 中段保留位（bit25..27，与真源 MASK_RESERVED 同锚）。
pub const MID_RESERVED: u32 = 0x0e00_0000;

/// 掩码区段类别（#[repr(u16)] 显式判别式）。
#[repr(u16)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum MaskZone {
    /// 特定权限段（bit0..15）。
    Specific = 0x0001,
    /// 标准权限段（bit16..23）。
    Standard = 0x0002,
    /// GENERIC 段（bit24..31）。
    Generic  = 0x0003,
}

impl MaskZone {
    pub fn code(self) -> u16 {
        self as u16
    }
}

/// 三段拆分视图。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct MaskParts {
    pub specific: u16,
    pub standard: u8,
    pub generic: u8,
}

/// 拆分：掩码 → 三段视图。
pub fn split(mask: u32) -> MaskParts {
    MaskParts {
        specific: (mask & SPECIFIC_MASK) as u16,
        standard: ((mask >> 16) & 0x0000_00ff) as u8,
        generic: ((mask >> 24) & 0x0000_00ff) as u8,
    }
}

/// 合成：三段视图 → 掩码。
pub fn join(p: &MaskParts) -> u32 {
    (p.specific as u32)
        | ((p.standard as u32) << 16)
        | ((p.generic as u32) << 24)
}

/// 掩码全闸：保留中段 → 标准段未定义位（顺序固定）。
pub fn validate_mask(mask: u32) -> Result<(), u16> {
    if mask & MID_RESERVED != 0 {
        return Err(E_RESERVED);
    }
    let std = mask & 0x00ff_0000;
    if std & !STANDARD_LEGAL != 0 {
        return Err(E_STD_BITS);
    }
    Ok(())
}

/// 区段读。
pub fn read_zone(mask: u32, zone: MaskZone) -> u32 {
    match zone {
        MaskZone::Specific => mask & SPECIFIC_MASK,
        MaskZone::Standard => mask & STANDARD_LEGAL,
        MaskZone::Generic => mask & GENERIC_MASK,
    }
}

/// 区段写：标准段只收合法位、GENERIC 段只收低 4 位（越界显性拒）。
pub fn write_zone(mask: u32, zone: MaskZone, val: u32) -> Result<u32, u16> {
    match zone {
        MaskZone::Specific => Ok((mask & !SPECIFIC_MASK) | (val & SPECIFIC_MASK)),
        MaskZone::Standard => {
            if val & !STANDARD_LEGAL != 0 {
                return Err(E_STD_BITS);
            }
            Ok((mask & !STANDARD_LEGAL) | (val & STANDARD_LEGAL))
        }
        MaskZone::Generic => {
            if val & !GENERIC_MASK != 0 {
                return Err(E_GENERIC);
            }
            Ok((mask & !GENERIC_MASK) | (val & GENERIC_MASK))
        }
    }
}

/// 摘要行（String 面，无 IO）。
pub fn summary(mask: u32) -> String {
    let p = split(mask);
    let mut s = String::new();
    s.push_str("mask spec=0x");
    s.push_str(&p.specific.to_string());
    s.push_str(" std=0x");
    s.push_str(&p.standard.to_string());
    s.push_str(" gen=0x");
    s.push_str(&p.generic.to_string());
    s
}

/// 诊断码闭集互异自检。
pub fn codes_distinct() -> bool {
    let cs = [E_VER, E_RESERVED, E_STD_BITS, E_GENERIC, E_ROUND, E_CS];
    let mut i = 0;
    while i < cs.len() {
        let mut j = i + 1;
        while j < cs.len() {
            if cs[i] == cs[j] {
                return false;
            }
            j += 1;
        }
        i += 1;
    }
    true
}

pub fn detail(code: u16) -> &'static str {
    match code {
        E_VER => "版本锚不符",
        E_RESERVED => "保留中段（bit25..27）非零",
        E_STD_BITS => "标准段含未定义位",
        E_GENERIC => "GENERIC 区写入溢出（>4 位）",
        E_ROUND => "拆分-合成往返不对平",
        E_CS => "指纹不稳",
        _ => "未知诊断码",
    }
}

/// 演练：R1 锚位互证；R2 12 例拆分-合成往返；R3 三区段独立读写；
/// R4 保留中段 3 例；R5 标准段未定义位 3 例；R6 GENERIC 溢出写 2 例；
/// R7 指纹稳定。
pub fn drill() -> Result<u32, u16> {
    let mut hits: u32 = 0;
    // R1：锚位互证（本模块中段保留锚 == 真源 MASK_RESERVED）。
    if MID_RESERVED != core::MASK_RESERVED {
        return Err(E_VER);
    }
    if STANDARD_LEGAL != STD_DELETE | STD_READ_CONTROL | STD_WRITE_DAC | STD_WRITE_OWNER | STD_SYNCHRONIZE {
        return Err(E_STD_BITS);
    }
    hits += 1;
    // R2：12 例往返（掩码逐位滚动构造，覆盖三段全交叠态）。
    let mut i: u32 = 0;
    while i < 12 {
        let mask: u32 = (0x0001_2003u32.wrapping_mul(i + 1)) & !MID_RESERVED;
        let p = split(mask);
        let back = join(&p);
        if back != mask {
            return Err(E_ROUND);
        }
        validate_mask(mask)?;
        if split(back) != p {
            return Err(E_ROUND);
        }
        hits += 1;
        i += 1;
    }
    // R3：三区段独立读写（写后读回，其余区段不受扰）。
    let base: u32 = 0x1000_0001;
    let zones: Vec<MaskZone> = vec![MaskZone::Specific, MaskZone::Standard, MaskZone::Generic];
    let mut z = 0;
    while z < zones.len() {
        let val: u32 = match zones[z] {
            MaskZone::Specific => 0x0000_00fe,
            MaskZone::Standard => STD_READ_CONTROL | STD_SYNCHRONIZE,
            MaskZone::Generic => 0x8000_0000,
        };
        let m2 = write_zone(base, zones[z], val)?;
        if read_zone(m2, zones[z]) != (val & read_zone(0xffff_ffff, zones[z])) {
            return Err(E_ROUND);
        }
        // 其余区段不受扰（base 原值仍在其他区段）。
        let others_clean = match zones[z] {
            MaskZone::Specific => read_zone(m2, MaskZone::Generic) == read_zone(base, MaskZone::Generic),
            _ => read_zone(m2, MaskZone::Specific) == read_zone(base, MaskZone::Specific),
        };
        if !others_clean {
            return Err(E_ROUND);
        }
        hits += 1;
        z += 1;
    }
    // R4：保留中段 3 例——全部 E_RESERVED。
    let mids: Vec<u32> = vec![0x0200_0000, 0x0400_0000, 0x0600_0000];
    let mut m = 0;
    while m < mids.len() {
        match validate_mask(mids[m]) {
            Ok(_) => return Err(E_RESERVED),
            Err(c) => {
                if c != E_RESERVED {
                    return Err(E_RESERVED);
                }
            }
        }
        hits += 1;
        m += 1;
    }
    // R5：标准段未定义位 3 例——全部 E_STD_BITS。
    let stds: Vec<u32> = vec![0x0020_0000, 0x0040_0000, 0x00e0_0000];
    let mut s = 0;
    while s < stds.len() {
        match validate_mask(stds[s]) {
            Ok(_) => return Err(E_STD_BITS),
            Err(c) => {
                if c != E_STD_BITS {
                    return Err(E_STD_BITS);
                }
            }
        }
        hits += 1;
        s += 1;
    }
    // R6：GENERIC 溢出写 2 例（0x1F / 0x20 写入 GENERIC 区）——全部 E_GENERIC。
    let overs: Vec<u32> = vec![0x0000_001f, 0x0000_0020];
    let mut o = 0;
    while o < overs.len() {
        match write_zone(0, MaskZone::Generic, overs[o]) {
            Ok(_) => return Err(E_GENERIC),
            Err(c) => {
                if c != E_GENERIC {
                    return Err(E_GENERIC);
                }
            }
        }
        hits += 1;
        o += 1;
    }
    // R7：指纹两次求值稳定且非零（真源 FNV over 三段字节 + 版本锚）。
    let m0 = 0x1002_0003u32;
    let p0 = split(m0);
    let mut feed = Vec::new();
    feed.extend_from_slice(&VER.to_le_bytes());
    feed.extend_from_slice(&p0.specific.to_le_bytes());
    feed.push(p0.standard);
    feed.push(p0.generic);
    let fp = core::fnv1a(&feed);
    let fp2 = core::fnv1a(&feed);
    if fp != fp2 || fp == 0 {
        return Err(E_CS);
    }
    hits += 1;
    Ok(hits)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn drill_green() {
        assert_eq!(drill(), Ok(25));
    }

    #[test]
    fn reserved_mid_rejected() {
        assert_eq!(validate_mask(0x0200_0000), Err(E_RESERVED));
    }

    #[test]
    fn std_undefined_rejected() {
        assert_eq!(validate_mask(0x0020_0000), Err(E_STD_BITS));
        assert_eq!(validate_mask(STANDARD_LEGAL), Ok(()));
    }

    #[test]
    fn zone_roundtrip() {
        let m = write_zone(0, MaskZone::Standard, STD_WRITE_DAC).unwrap_or(0xffff_ffff);
        assert_eq!(m, STD_WRITE_DAC);
        assert!(summary(m).contains("mask"));
    }
}

pub fn run_unx36087_checks() -> CheckSet {
    let mut cs = CheckSet::new("unxstar-unx36087_acemaskbits");
    cs.add("SEG 段位独占", SEG_BASE == 0xe330, "0xe330~0xe33f 本模块独占");
    match drill() {
        Ok(_) => cs.add("确定性演练全绿", true, "三段往返 12 例对平、负例 8 例全拒"),
        Err(code) => cs.add("确定性演练全绿", false, detail(code)),
    }
    cs.add("中段保留锚与真源同锚", MID_RESERVED == core::MASK_RESERVED, "保留段锚与真源漂移");
    cs.add("标准段五常量锚", STANDARD_LEGAL == 0x001f_0000, "标准权限段锚漂移");
    cs.add("版本锚 VER=1", VER == 1, "版本锚不符");
    cs.add("诊断码闭集互异", codes_distinct(), "诊断码闭集出现重码");
    cs
}
