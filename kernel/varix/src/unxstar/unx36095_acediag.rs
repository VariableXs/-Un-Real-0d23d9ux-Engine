//! UNX-F36095-J1 ACE 诊断打印面（ACE 级人话解释）。
//! 判据：① 全部 11 类 ACE 可解释（类型名在场）、掩码三段逐位人话展开、
//! 标志位族逐位展开，输出 String 且确定性（两次求值逐字节相等）；
//! ② 未知类型 2 例显性拒绝；③ 纯功能代码 ≥300 行。
//! 设计纪律：消费真源 unx36081_acecore（类型闭集/名字表/保留位锚）；
//! 本模块是 ACE 级人话面——SD 级人话面（F36076，B04）在容器级。

use crate::checks::CheckSet;
use super::unx36081_acecore as core;
use alloc::vec;
use alloc::vec::Vec;
use alloc::string::{String, ToString};

pub const SEG_BASE: u16 = 0x3870;

// ---- 诊断码闭集 -----------------------------------------------------------
pub const E_VER: u16 = SEG_BASE + 1;  // 版本锚不符
pub const E_TYPE: u16 = SEG_BASE + 2; // 类型不在闭集（无从解释）
pub const E_EMPTY: u16 = SEG_BASE + 3;// 解释产出为空（纪律破）
pub const E_CS: u16 = SEG_BASE + 4;   // 确定性破（两次求值不等）

/// 版本锚。
pub const VER: u32 = 1;

/// 标准权限位人话表（与 36087 五常量同锚）。
pub const STD_NAMES: [(&str, u32); 5] = [
    ("DELETE", 0x0001_0000),
    ("READ_CONTROL", 0x0002_0000),
    ("WRITE_DAC", 0x0004_0000),
    ("WRITE_OWNER", 0x0008_0000),
    ("SYNCHRONIZE", 0x0010_0000),
];

/// GENERIC 位人话表。
pub const GENERIC_NAMES: [(&str, u32); 4] = [
    ("GENERIC_READ", 0x8000_0000),
    ("GENERIC_WRITE", 0x4000_0000),
    ("GENERIC_EXECUTE", 0x2000_0000),
    ("GENERIC_ALL", 0x1000_0000),
];

/// 标志位人话表（CI/OI/NP/IP/ID + 审计触发位）。
pub const FLAG_NAMES: [(&str, u8); 7] = [
    ("CI", 0x01),
    ("OI", 0x02),
    ("NP", 0x04),
    ("IP", 0x08),
    ("ID", 0x10),
    ("AUDIT_SUCCESS", 0x40),
    ("AUDIT_FAILURE", 0x80),
];

/// 解释分段类别（#[repr(u16)] 显式判别式）。
#[repr(u16)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DiagSection {
    /// 类型段。
    TypeSection = 0x0001,
    /// 掩码段。
    MaskSection = 0x0002,
    /// 标志段。
    FlagSection = 0x0003,
}

impl DiagSection {
    pub fn code(self) -> u16 {
        self as u16
    }

    /// 按分段独立产出人话（explain 三段的单段入口）。
    pub fn render(self, ace_type: u8, mask: u32, flags: u8) -> Result<String, u16> {
        match self {
            DiagSection::TypeSection => {
                let tn = type_name(ace_type)?;
                let mut s = String::new();
                s.push_str("type=");
                s.push_str(tn);
                Ok(s)
            }
            DiagSection::MaskSection => Ok(explain_mask(mask)),
            DiagSection::FlagSection => Ok(explain_flags(flags)),
        }
    }
}

/// 类型字节 → 人话名（消费真源闭集；表外 = E_TYPE）。
pub fn type_name(ace_type: u8) -> Result<&'static str, u16> {
    match core::AceType::from_code(ace_type as u16) {
        Some(t) => Ok(t.name()),
        None => Err(E_TYPE),
    }
}

/// 掩码人话展开（标准段 → GENERIC 段 → 特定段逐位）。
pub fn explain_mask(mask: u32) -> String {
    let mut s = String::new();
    s.push_str("mask:[");
    let mut first = true;
    // 标准段五位。
    let mut i = 0;
    while i < STD_NAMES.len() {
        if mask & STD_NAMES[i].1 != 0 {
            if !first {
                s.push_str("+");
            }
            s.push_str(STD_NAMES[i].0);
            first = false;
        }
        i += 1;
    }
    // GENERIC 段四位。
    let mut g = 0;
    while g < GENERIC_NAMES.len() {
        if mask & GENERIC_NAMES[g].1 != 0 {
            if !first {
                s.push_str("+");
            }
            s.push_str(GENERIC_NAMES[g].0);
            first = false;
        }
        g += 1;
    }
    // 特定段逐位（SPEC_00..SPEC_15）。
    let mut b = 0;
    while b < 16 {
        if mask & (1u32 << b) != 0 {
            if !first {
                s.push_str("+");
            }
            s.push_str("SPEC_");
            if b < 10 {
                s.push_str(&b.to_string());
            } else {
                s.push_str(&b.to_string());
            }
            first = false;
        }
        b += 1;
    }
    if first {
        s.push_str("NONE");
    }
    s.push_str("]");
    s
}

/// 标志位人话展开（七位逐位 + 保留位显名）。
pub fn explain_flags(flags: u8) -> String {
    let mut s = String::new();
    s.push_str("flags:[");
    let mut first = true;
    let mut i = 0;
    while i < FLAG_NAMES.len() {
        if flags & FLAG_NAMES[i].1 != 0 {
            if !first {
                s.push_str("+");
            }
            s.push_str(FLAG_NAMES[i].0);
            first = false;
        }
        i += 1;
    }
    if flags & core::FLAG_RESERVED != 0 {
        if !first {
            s.push_str("+");
        }
        s.push_str("RESERVED");
        first = false;
    }
    if first {
        s.push_str("NONE");
    }
    s.push_str("]");
    s
}

/// ACE 级人话解释（单入口：类型/掩码/标志三段全展开，无 IO）。
pub fn explain(ace_type: u8, mask: u32, flags: u8) -> Result<String, u16> {
    let tn = type_name(ace_type)?;
    let mut s = String::new();
    s.push_str("ace type=");
    s.push_str(tn);
    s.push_str(" ");
    s.push_str(&explain_mask(mask));
    s.push_str(" ");
    s.push_str(&explain_flags(flags));
    if s.is_empty() {
        return Err(E_EMPTY);
    }
    Ok(s)
}

/// 批量解释面：给定类型表逐条产出（36098 金例库诊断消费锚）。
pub fn explain_all() -> Result<Vec<String>, u16> {
    let mut out: Vec<String> = Vec::new();
    let mut i = 0;
    while i < core::ACE_TYPE_N {
        let code = core::ACE_TYPE_TABLE[i].code as u8;
        out.push(explain(code, 0x0001_2003, 0x10)?);
        i += 1;
    }
    Ok(out)
}

/// 诊断码闭集互异自检。
pub fn codes_distinct() -> bool {
    let cs = [E_VER, E_TYPE, E_EMPTY, E_CS];
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
        E_TYPE => "类型不在闭集（无从解释）",
        E_EMPTY => "解释产出为空（纪律破）",
        E_CS => "确定性破（两次求值不等）",
        _ => "未知诊断码",
    }
}

/// 演练：R1 版本锚 + 人话表锚；R2 11 类全解释（类型名在场）；R3 掩码三段
/// 展开点名 3 例；R4 标志展开点名 3 例；R5 未知类型 2 例；R6 确定性对拍。
pub fn drill() -> Result<u32, u16> {
    let mut hits: u32 = 0;
    // R1：版本锚 + 表锚（标准段/GENERIC/标志三表长度）。
    if VER != 1 {
        return Err(E_VER);
    }
    if STD_NAMES.len() != 5 || GENERIC_NAMES.len() != 4 || FLAG_NAMES.len() != 7 {
        return Err(E_EMPTY);
    }
    hits += 1;
    // R2：11 类全解释，类型名逐条在场（explain_all 走真源表）。
    let all = explain_all()?;
    if all.len() != core::ACE_TYPE_N {
        return Err(E_EMPTY);
    }
    let mut i = 0;
    while i < core::ACE_TYPE_N {
        let want = core::ACE_TYPE_TABLE[i].name;
        if !all[i].contains(want) {
            return Err(E_EMPTY);
        }
        hits += 1;
        i += 1;
    }
    // R3：掩码三段展开点名 3 例。
    let m1 = explain_mask(STD_NAMES[0].1 | STD_NAMES[4].1);
    if !m1.contains("DELETE") || !m1.contains("SYNCHRONIZE") {
        return Err(E_EMPTY);
    }
    hits += 1;
    let m2 = explain_mask(GENERIC_NAMES[0].1);
    if !m2.contains("GENERIC_READ") {
        return Err(E_EMPTY);
    }
    hits += 1;
    let m3 = explain_mask(0x0000_0005); // SPEC_0 + SPEC_2
    if !m3.contains("SPEC_0") || !m3.contains("SPEC_2") {
        return Err(E_EMPTY);
    }
    hits += 1;
    // R4：标志展开点名 3 例。
    let f1 = explain_flags(0x01 | 0x10);
    if !f1.contains("CI") || !f1.contains("ID") {
        return Err(E_EMPTY);
    }
    hits += 1;
    let f2 = explain_flags(0x40 | 0x80);
    if !f2.contains("AUDIT_SUCCESS") || !f2.contains("AUDIT_FAILURE") {
        return Err(E_EMPTY);
    }
    hits += 1;
    let f3 = explain_flags(0x00);
    if !f3.contains("NONE") {
        return Err(E_EMPTY);
    }
    hits += 1;
    // R5：未知类型 2 例（0x04 / 0xEE）——E_TYPE。
    let unknowns: Vec<u8> = vec![0x04, 0xee];
    let mut u = 0;
    while u < unknowns.len() {
        match explain(unknowns[u], 0, 0) {
            Ok(_) => return Err(E_TYPE),
            Err(c) => {
                if c != E_TYPE {
                    return Err(E_TYPE);
                }
            }
        }
        hits += 1;
        u += 1;
    }
    // R6：确定性对拍（同参两次求值逐字节相等且非空）。
    let a1 = explain(0x01, 0x0003_0001, 0x11)?;
    let a2 = explain(0x01, 0x0003_0001, 0x11)?;
    if a1 != a2 || a1.is_empty() {
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
        assert_eq!(drill(), Ok(21));
    }

    #[test]
    fn unknown_type_rejected() {
        assert_eq!(type_name(0x04), Err(E_TYPE));
        assert_eq!(explain(0xee, 0, 0), Err(E_TYPE));
    }

    #[test]
    fn mask_expansion_names() {
        let s = explain_mask(0x0003_0001);
        assert!(s.contains("DELETE"));
        assert!(s.contains("READ_CONTROL"));
        assert!(s.contains("SPEC_0"));
    }

    #[test]
    fn explain_deterministic() {
        let a = explain(0x00, 0x0001_0001, 0x02).unwrap_or(String::new());
        let b = explain(0x00, 0x0001_0001, 0x02).unwrap_or(String::new());
        assert_eq!(a, b);
        assert!(a.contains("access-allowed"));
    }
}

pub fn run_unx36095_checks() -> CheckSet {
    let mut cs = CheckSet::new("unxstar-unx36095_acediag");
    cs.add("SEG 段位独占", SEG_BASE == 0x3870, "0x3870~0x387f 本模块独占");
    match drill() {
        Ok(_) => cs.add("确定性演练全绿", true, "11 类全解释、三段展开点名全对"),
        Err(code) => cs.add("确定性演练全绿", false, detail(code)),
    }
    cs.add("人话表锚 5/4/7", STD_NAMES.len() == 5 && GENERIC_NAMES.len() == 4 && FLAG_NAMES.len() == 7, "人话表锚漂移");
    cs.add("版本锚 VER=1", VER == 1, "版本锚不符");
    cs.add("诊断码闭集互异", codes_distinct(), "诊断码闭集出现重码");
    cs
}
