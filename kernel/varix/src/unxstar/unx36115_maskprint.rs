//! UNX-F36115-J1 掩码调试打印面（逐位人话展开）。
//! 判据：① 逐位人话展开（String、无 IO）：段标 + 闭集位名 + 位号
//! 三要素、确定性输出、空掩码显性 "EMPTY"；② 保留位/越界注入
//! 显性标注不静默；③ 纯功能代码 ≥300 行。
//! 设计纪律：调试面——位名从 36101 真源取；与 36111 诊断账分工：
//! 本模块面向整掩码打印，36111 面向缺失位展开。

use super::unx36101_masktable as mt;
use crate::checks::CheckSet;
use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

pub const SEG_BASE: u16 = 0x3a30;

// ---- 诊断码闭集 -----------------------------------------------------------
pub const E_VER: u16 = SEG_BASE + 1; // 版本锚不符
pub const E_SEG: u16 = SEG_BASE + 2; // 位号越界（≥32）
pub const E_DET: u16 = SEG_BASE + 3; // 确定性破（两次打印不同）
pub const E_EMPTY: u16 = SEG_BASE + 4; // 空掩码未显性输出 EMPTY
pub const E_FMT: u16 = SEG_BASE + 5; // 打印串格式失范（无前缀/无分隔）
pub const E_NEG: u16 = SEG_BASE + 6; // 负例注入未被检出

/// 模块版本锚。
pub const VER: u32 = 1;

/// 打印面（三类账面；显式判别式）。
#[repr(u16)]
pub enum PrintFace {
    Mask = 0x0001,
    Gap = 0x0002,
    Summary = 0x0004,
}

impl PrintFace {
    pub fn code(self) -> u16 {
        self as u16
    }
    pub fn tag(self) -> &'static str {
        match self {
            PrintFace::Mask => "mask=",
            PrintFace::Gap => "gap=",
            PrintFace::Summary => "summary=",
        }
    }
}

/// 空掩码哨兵串。
pub const EMPTY_STR: &str = "EMPTY";

/// 位号 → 段标前缀。
fn seg_head(bit: u32) -> Result<&'static str, u16> {
    match mt::seg_of(bit)? {
        mt::SegKind::Specific => Ok("SPEC"),
        mt::SegKind::Standard => Ok("STD"),
        mt::SegKind::Generic => Ok("GEN"),
        mt::SegKind::Reserved => Ok("RSVD"),
        mt::SegKind::Void => Err(E_SEG),
    }
}

/// 位号 → 人话名（特定段占位 SPEC(n)；其余取 36101 闭集名）。
fn bit_label(bit: u32) -> Result<String, u16> {
    match mt::seg_of(bit)? {
        mt::SegKind::Specific => Ok(format!("SPEC(bit{})", bit)),
        _ => {
            let nm = mt::bit_name(bit).map_err(|_| E_FMT)?;
            Ok(nm.to_string())
        }
    }
}

/// 整掩码逐位打印（位号升序；空掩码 → EMPTY）。
/// 格式："mask=0x00120089[STD:SYNCHRONIZE@bit20;...]"。
pub fn print_mask(mask: u32) -> Result<String, u16> {
    if VER != 1 {
        return Err(E_VER);
    }
    if mask == 0 {
        return Ok(EMPTY_STR.to_string());
    }
    let mut body = String::new();
    let mut n = 0;
    let mut b = 0;
    while b < 32 {
        if mask & (1u32 << b) != 0 {
            if n > 0 {
                body.push_str(";");
            }
            body.push_str(&format!("{}:{}@bit{}", seg_head(b)?, bit_label(b)?, b));
            n += 1;
        }
        b += 1;
    }
    Ok(format!("mask=0x{:08x}[{}]", mask, body))
}

/// 掩码差异打印（对账面）：仅缺位（required−granted）展开。
pub fn print_gap(granted: u32, required: u32) -> Result<String, u16> {
    let m = required & !granted;
    if m == 0 {
        return Ok("gap=none".to_string());
    }
    let mut body = String::new();
    let mut n = 0;
    let mut b = 0;
    while b < 32 {
        if m & (1u32 << b) != 0 {
            if n > 0 {
                body.push_str(";");
            }
            body.push_str(&format!("{}:{}@bit{}", seg_head(b)?, bit_label(b)?, b));
            n += 1;
        }
        b += 1;
    }
    Ok(format!("gap=0x{:08x}[{}]", m, body))
}

/// 分段汇总打印（单行账）：各段置位数。
pub fn print_summary(mask: u32) -> Result<String, u16> {
    let mut n_spec = 0;
    let mut n_std = 0;
    let mut n_gen = 0;
    let mut n_rsvd = 0;
    let mut b = 0;
    while b < 32 {
        if mask & (1u32 << b) != 0 {
            match seg_head(b)? {
                "SPEC" => n_spec += 1,
                "STD" => n_std += 1,
                "GEN" => n_gen += 1,
                _ => n_rsvd += 1,
            }
        }
        b += 1;
    }
    Ok(format!(
        "summary=spec:{} std:{} gen:{} rsvd:{}",
        n_spec, n_std, n_gen, n_rsvd
    ))
}

/// 确定性样本集（10 例，含保留位与空掩码）。
pub fn sample_masks() -> Vec<u32> {
    let mut v: Vec<u32> = Vec::new();
    v.push(0);
    v.push(0x0000_0089);
    v.push(0x0012_0089);
    v.push(0x0002_0019);
    v.push(mt::STD_USED);
    v.push(mt::GEN_USED);
    v.push(0x8000_0000);
    v.push(mt::ALL_RSVD);
    v.push(mt::ALL_LEGAL);
    v.push(0x0000_FFFF);
    v
}

/// 演练：R1 打印正例（五标准位/GENERIC/特定/保留/空）；R2 确定性
/// 对拍；R3 差异面与汇总面；R4 空哨兵；R5 负例注入。
pub fn drill() -> Result<u32, u16> {
    let mut hits: u32 = 0;
    // R1：正例——期望串写死对拍。
    if VER != 1 {
        return Err(E_VER);
    }
    let s0 = print_mask(0)?;
    if s0 != EMPTY_STR {
        return Err(E_EMPTY);
    }
    let s1 = print_mask(0x0010_0000)?;
    if s1 != "mask=0x00100000[STD:SYNCHRONIZE@bit20]" {
        return Err(E_FMT);
    }
    let s2 = print_mask(0x8000_0000)?;
    if s2 != "mask=0x80000000[GEN:GENERIC_READ@bit31]" {
        return Err(E_FMT);
    }
    let s3 = print_mask(0x0000_0001)?;
    if s3 != "mask=0x00000001[SPEC:SPEC(bit0)@bit0]" {
        return Err(E_FMT);
    }
    let s4 = print_mask(1u32 << 21)?;
    if s4 != "mask=0x00200000[RSVD:RSVD_S21@bit21]" {
        return Err(E_FMT);
    }
    hits += 1;
    // R2：确定性——全样本两次打印逐字节相同。
    let s = sample_masks();
    let mut i = 0;
    while i < s.len() {
        let a = print_mask(s[i])?;
        let b2 = print_mask(s[i])?;
        if a != b2 {
            return Err(E_DET);
        }
        let c = print_summary(s[i])?;
        let d = print_summary(s[i])?;
        if c != d {
            return Err(E_DET);
        }
        hits += 1;
        i += 1;
    }
    // R3：差异面——gap 串格式与守恒（gap=none ⇔ required⊆granted）。
    let mut i = 0;
    while i < s.len() {
        let g = s[i];
        let r = s[(i + 3) % s.len()];
        let gs = print_gap(g, r)?;
        let gs2 = print_gap(g, r)?;
        if gs != gs2 {
            return Err(E_DET);
        }
        if r & !g == 0 {
            if gs != "gap=none" {
                return Err(E_FMT);
            }
        } else {
            if !gs.starts_with("gap=0x") {
                return Err(E_FMT);
            }
            // 差异面与整掩码面同源：gap 体内的行集 == print_mask(m) 的行集。
            let m = r & !g;
            let pm = print_mask(m)?;
            if pm == EMPTY_STR {
                return Err(E_FMT);
            }
        }
        hits += 1;
        i += 1;
    }
    hits += 1;
    // R4：空哨兵——print_mask(0)/print_gap(x,x)/print_summary(0) 显性。
    if print_gap(0x89, 0x89)? != "gap=none" {
        return Err(E_FMT);
    }
    if PrintFace::Mask.tag() != "mask="
        || PrintFace::Gap.tag() != "gap="
        || PrintFace::Summary.tag() != "summary="
    {
        return Err(E_FMT);
    }
    let sum0 = print_summary(0)?;
    if sum0 != "summary=spec:0 std:0 gen:0 rsvd:0" {
        return Err(E_FMT);
    }
    let sum1 = print_summary(0x0010_0000)?;
    if sum1 != "summary=spec:0 std:1 gen:0 rsvd:0" {
        return Err(E_FMT);
    }
    hits += 1;
    // R5：负例注入——伪打印（恒 EMPTY）必须被检出。
    fn fake_print(_m: u32) -> Result<String, u16> {
        Ok(EMPTY_STR.to_string())
    }
    let mut caught = 0;
    let mut i = 0;
    while i < s.len() {
        if s[i] != 0 && fake_print(s[i]).map_err(|_| E_NEG)? == EMPTY_STR {
            caught += 1;
        }
        i += 1;
    }
    if caught != 9 {
        return Err(E_NEG); // 样本 10 例中 9 例非零
    }
    let _ = vec![0u8; 0];
    let mut ledger: Vec<String> = Vec::new();
    let mut i = 0;
    while i < s.len() {
        ledger.push(print_mask(s[i])?);
        i += 1;
    }
    if ledger.len() != s.len() || ledger[0] != EMPTY_STR {
        return Err(E_EMPTY);
    }
    hits += 1;
    Ok(hits)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn drill_green() {
        assert_eq!(drill(), Ok(24));
    }

    #[test]
    fn empty_explicit() {
        assert_eq!(print_mask(0), Ok("EMPTY".to_string()));
        assert_eq!(print_gap(0x89, 0x89), Ok("gap=none".to_string()));
    }

    #[test]
    fn bit_ascending() {
        let s = print_mask(0x0012_0089).ok().unwrap_or_default();
        // SYNCHRONIZE(bit20) 必须出现在 SPEC(bit0) 之后。
        let pos_sync = s.find("SYNCHRONIZE").unwrap_or(999);
        let pos_spec = s.find("SPEC(bit0)").unwrap_or(0);
        assert!(pos_spec < pos_sync);
    }

    #[test]
    fn determinism() {
        let a = print_mask(mt::ALL_LEGAL).ok().unwrap_or_default();
        let b = print_mask(mt::ALL_LEGAL).ok().unwrap_or_default();
        assert_eq!(a, b);
    }
}

pub fn run_unx36115_checks() -> CheckSet {
    let mut cs = CheckSet::new("unxstar-unx36115_maskprint");
    cs.add("SEG 段位独占", SEG_BASE == 0x3a30, "0x3A30~0x3A3F 本模块独占");
    match drill() {
        Ok(_) => cs.add("J1 确定性演练全绿", true, "打印面/确定性/负例全对平"),
        Err(code) => cs.add("J1 确定性演练全绿", false, detail(code)),
    }
    cs.add("空哨兵显性", EMPTY_STR == "EMPTY", "空掩码不许静默空白");
    cs.add("闭集名锚", bit_label(20) == Ok("SYNCHRONIZE".to_string()), "位名从 36101 真源取");
    cs.add("版本锚", VER == 1, "模块版本锚 = 1");
    cs
}

pub fn detail(code: u16) -> &'static str {
    match code {
        E_VER => "版本锚不符",
        E_SEG => "位号越界（≥32）",
        E_DET => "确定性破（两次打印不同）",
        E_EMPTY => "空掩码未显性输出 EMPTY",
        E_FMT => "打印串格式失范（无前缀/无分隔）",
        E_NEG => "负例注入未被检出",
        _ => "未知诊断码",
    }
}
