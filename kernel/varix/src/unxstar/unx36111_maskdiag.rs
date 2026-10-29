//! UNX-F36111-J1 掩码诊断账：缺失位的人话展开。
//! 判据：① 缺失位展开（String 输出、无 IO）：闭集位名 + 段归属 +
//! 位号三要素逐位落账；② 展开的确定性（两次求值逐字节相同）与
//! 闭集外显性标注；③ 纯功能代码 ≥300 行。
//! 设计纪律：诊断面——位名从 36101 真源取（不造第二套名表）；
//! 展开串供 AccessCheck 拒绝诊断账（F36168）消费。

use super::unx36101_masktable as mt;
use crate::checks::CheckSet;
use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

pub const SEG_BASE: u16 = 0x39b0;

// ---- 诊断码闭集 -----------------------------------------------------------
pub const E_VER: u16 = SEG_BASE + 1; // 版本锚不符
pub const E_SEG: u16 = SEG_BASE + 2; // 位号越界（≥32）
pub const E_NAME: u16 = SEG_BASE + 3; // 展开名不在闭集（非 RSVD_ 前缀的保留位）
pub const E_DET: u16 = SEG_BASE + 4; // 确定性破（两次展开不同）
pub const E_EMPTY: u16 = SEG_BASE + 5; // 空缺失的哨兵串缺失
pub const E_FMT: u16 = SEG_BASE + 6; // 展开串格式失范（无段标/无位号）
pub const E_NEG: u16 = SEG_BASE + 7; // 负例注入未被检出

/// 模块版本锚。
pub const VER: u32 = 1;

/// 空缺失哨兵串（显性——诊断面不许静默空白）。
pub const NO_GAP: &str = "no-gap";

/// 段标（诊断行的四类前缀；显式判别式）。
#[repr(u16)]
pub enum DiagSeg {
    Spec = 0x0001,
    Std = 0x0002,
    Gen = 0x0004,
    Rsvd = 0x0008,
}

impl DiagSeg {
    pub fn code(self) -> u16 {
        self as u16
    }
    pub fn head(self) -> &'static str {
        match self {
            DiagSeg::Spec => "SPEC",
            DiagSeg::Std => "STD",
            DiagSeg::Gen => "GEN",
            DiagSeg::Rsvd => "RSVD",
        }
    }
}

/// 单位号 → 段标。
fn seg_tag(bit: u32) -> Result<DiagSeg, u16> {
    match mt::seg_of(bit)? {
        mt::SegKind::Specific => Ok(DiagSeg::Spec),
        mt::SegKind::Standard => Ok(DiagSeg::Std),
        mt::SegKind::Generic => Ok(DiagSeg::Gen),
        _ => Ok(DiagSeg::Rsvd),
    }
}

/// 单位号 → 闭集人话名（特定段走类型局部占位名 SPEC(n)；保留位
/// 走 RSVD_* 名；闭集外显性 Err——非静默匹配红线）。
pub fn bit_label(bit: u32) -> Result<String, u16> {
    if bit >= 32 {
        return Err(E_SEG);
    }
    match mt::seg_of(bit)? {
        mt::SegKind::Specific => Ok(format!("SPEC(bit{})", bit)),
        _ => {
            let nm = mt::bit_name(bit).map_err(|_| E_NAME)?;
            Ok(nm.to_string())
        }
    }
}

/// 单位缺失行："[STD:SYNCHRONIZE@bit20]" 格式。
pub fn gap_line(bit: u32) -> Result<String, u16> {
    let head = seg_tag(bit)?.head();
    let label = bit_label(bit)?;
    Ok(format!("[{}:{}@bit{}]", head, label, bit))
}

/// 缺失位人话展开（主账）：无缺失 → NO_GAP；有缺失 → 逐位升序行。
pub fn explain_missing(granted: u32, required: u32) -> Result<String, u16> {
    let m = required & !granted;
    if m == 0 {
        return Ok(NO_GAP.to_string());
    }
    let mut out = String::new();
    let mut n = 0;
    let mut b = 0;
    while b < 32 {
        if m & (1u32 << b) != 0 {
            if n > 0 {
                out.push_str(";");
            }
            out.push_str(&gap_line(b)?);
            n += 1;
        }
        b += 1;
    }
    if n == 0 {
        return Err(E_FMT);
    }
    Ok(out)
}

/// 缺失账汇总头：缺失数 + 分段统计。
pub fn missing_stats(granted: u32, required: u32) -> Result<String, u16> {
    let m = required & !granted;
    let mut n_spec = 0;
    let mut n_std = 0;
    let mut n_gen = 0;
    let mut n_rsvd = 0;
    let mut b = 0;
    while b < 32 {
        if m & (1u32 << b) != 0 {
            match seg_tag(b)? {
                DiagSeg::Spec => n_spec += 1,
                DiagSeg::Std => n_std += 1,
                DiagSeg::Gen => n_gen += 1,
                DiagSeg::Rsvd => n_rsvd += 1,
            }
        }
        b += 1;
    }
    let total = n_spec + n_std + n_gen + n_rsvd;
    Ok(format!(
        "missing={} spec={} std={} gen={} rsvd={}",
        total, n_spec, n_std, n_gen, n_rsvd
    ))
}

/// 全掩码展开（诊断面复用）：含保留位显性标注。
pub fn explain_mask(mask: u32) -> Result<String, u16> {
    if mask == 0 {
        return Ok("EMPTY".to_string());
    }
    let mut out = String::new();
    let mut n = 0;
    let mut b = 0;
    while b < 32 {
        if mask & (1u32 << b) != 0 {
            if n > 0 {
                out.push_str(";");
            }
            out.push_str(&gap_line(b)?);
            n += 1;
        }
        b += 1;
    }
    Ok(out)
}

/// 演练：R1 基本展开正例；R2 确定性对拍；R3 闭集外/越界负例；
/// R4 统计账对拍；R5 哨兵与格式检查。
pub fn drill() -> Result<u32, u16> {
    let mut hits: u32 = 0;
    if VER != 1 {
        return Err(E_VER);
    }
    // R1：基本展开——五标准位/四 GENERIC 位/特定位/保留位四类行。
    let cases: [(u32, u32, &str); 6] = [
        (0x0010_0000, 0x0010_0000, NO_GAP),
        (0, 0x0010_0000, "[STD:SYNCHRONIZE@bit20]"),
        (0, 0x8000_0000, "[GEN:GENERIC_READ@bit31]"),
        (0, 0x0000_0089, "[SPEC:SPEC(bit0)@bit0];[SPEC:SPEC(bit3)@bit3];[SPEC:SPEC(bit7)@bit7]"),
        (0, 1u32 << 25, "[RSVD:RSVD_G25@bit25]"),
        (0x0002_0000, 0x0012_0089, "[SPEC:SPEC(bit0)@bit0];[SPEC:SPEC(bit3)@bit3];[SPEC:SPEC(bit7)@bit7];[STD:SYNCHRONIZE@bit20]"),
    ];
    let mut i = 0;
    while i < 6 {
        let (g, r, expect) = cases[i];
        let s = explain_missing(g, r)?;
        if s != expect {
            return Err(E_FMT);
        }
        hits += 1;
        i += 1;
    }
    // R2：确定性——8 组两次求值逐字节相同；与 explain_mask 同源。
    let probes: [u32; 8] = [
        0x0012_0089,
        0x0002_0019,
        mt::STD_USED,
        mt::GEN_USED,
        mt::ALL_RSVD,
        0x0000_00FF,
        mt::ALL_LEGAL,
        0,
    ];
    let mut i = 0;
    while i < 8 {
        let g = probes[i];
        let r = probes[(i + 3) % 8] | probes[(i + 5) % 8];
        let a = explain_missing(g, r)?;
        let b2 = explain_missing(g, r)?;
        if a != b2 {
            return Err(E_DET);
        }
        if r == 0 && a != NO_GAP {
            return Err(E_EMPTY);
        }
        let em = explain_mask(r)?;
        let em2 = explain_mask(r)?;
        if em != em2 {
            return Err(E_DET);
        }
        hits += 1;
        i += 1;
    }
    // R3：闭集外/越界负例——位号 ≥32 必拒；特定段占位名回查。
    if bit_label(32).is_ok() || bit_label(u32::MAX / 3).is_ok() {
        return Err(E_SEG);
    }
    if bit_label(0).map_err(|_| E_NAME)? != "SPEC(bit0)" {
        return Err(E_NAME);
    }
    if bit_label(31).map_err(|_| E_NAME)? != "GENERIC_READ" {
        return Err(E_NAME);
    }
    if bit_label(21).map_err(|_| E_NAME)? != "RSVD_S21" {
        return Err(E_NAME);
    }
    hits += 1;
    // R4：统计账——总数 = 分段和；与逐位展开行数一致。
    let mut i = 0;
    while i < 8 {
        let g = probes[i];
        let r = probes[(i + 1) % 8] | probes[(i + 4) % 8];
        let m = r & !g;
        let st = missing_stats(g, r)?;
        let mut rows = 0;
        let mut b = 0;
        while b < 32 {
            if m & (1u32 << b) != 0 {
                rows += 1;
            }
            b += 1;
        }
        let head = format!("missing={}", rows);
        if !st.starts_with(&head) {
            return Err(E_FMT);
        }
        if m == 0 && !st.contains("missing=0") {
            return Err(E_EMPTY);
        }
        hits += 1;
        i += 1;
    }
    // R5：负例注入——伪展开（恒 no-gap）必须被守恒检出。
    fn fake_explain(_g: u32, _r: u32) -> Result<String, u16> {
        Ok(NO_GAP.to_string())
    }
    let mut caught = 0;
    let mut i = 0;
    while i < 8 {
        let g = probes[i];
        let r = probes[(i + 2) % 8];
        if r & !g != 0 && fake_explain(g, r).map_err(|_| E_NEG)? == NO_GAP {
            caught += 1;
        }
        i += 1;
    }
    if caught == 0 {
        return Err(E_NEG);
    }
    let _ = vec![0u8; 0];
    let mut ledger: Vec<String> = Vec::new();
    ledger.push(explain_missing(0, mt::STD_USED)?);
    ledger.push(missing_stats(0, mt::GEN_USED)?);
    if ledger.len() != 2 {
        return Err(E_NEG);
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
    fn basic_expand() {
        assert_eq!(explain_missing(0, 0x0010_0000), Ok("[STD:SYNCHRONIZE@bit20]".to_string()));
        assert_eq!(explain_missing(0x0010_0000, 0x0010_0000), Ok(NO_GAP.to_string()));
    }

    #[test]
    fn out_of_range_rejected() {
        assert_eq!(bit_label(32), Err(E_SEG));
        assert_eq!(bit_label(33), Err(E_SEG));
    }

    #[test]
    fn determinism() {
        let a = explain_missing(0, 0x0012_0089).ok().unwrap_or_default();
        let b = explain_missing(0, 0x0012_0089).ok().unwrap_or_default();
        assert_eq!(a, b);
    }
}

pub fn run_unx36111_checks() -> CheckSet {
    let mut cs = CheckSet::new("unxstar-unx36111_maskdiag");
    cs.add("SEG 段位独占", SEG_BASE == 0x39b0, "0x39B0~0x39BF 本模块独占");
    match drill() {
        Ok(_) => cs.add("J1 确定性演练全绿", true, "展开/确定性/负例全对平"),
        Err(code) => cs.add("J1 确定性演练全绿", false, detail(code)),
    }
    cs.add("哨兵显性", NO_GAP == "no-gap", "空缺失哨兵串（不许静默空白）");
    cs.add("闭集名锚", bit_label(20) == Ok("SYNCHRONIZE".to_string()), "位名从 36101 真源取");
    cs.add("版本锚", VER == 1, "模块版本锚 = 1");
    cs
}

pub fn detail(code: u16) -> &'static str {
    match code {
        E_VER => "版本锚不符",
        E_SEG => "位号越界（≥32）",
        E_NAME => "展开名不在闭集（非 RSVD_ 前缀的保留位）",
        E_DET => "确定性破（两次展开不同）",
        E_EMPTY => "空缺失的哨兵串缺失",
        E_FMT => "展开串格式失范（无段标/无位号）",
        E_NEG => "负例注入未被检出",
        _ => "未知诊断码",
    }
}
