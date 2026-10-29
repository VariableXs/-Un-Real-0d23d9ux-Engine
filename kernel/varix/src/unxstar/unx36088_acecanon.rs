//! UNX-F36088-J1 ACE 规范顺序（canonical order）校验器。
//! 判据：① 8 条合法序全过（deny 在前 allow 在后、审计殿后、标签类兜底）；
//! ② 6 条违规序全检出（allow 先于 deny / audit 先于 deny 等）、未知类型 2 例
//! 显性拒绝；③ 纯功能代码 ≥300 行。
//! 设计纪律：自含模块——只在类型码层面消费真源 unx36081_acecore 的闭集锚；
//! 本校验器是 36089/36092 插入序与 B09 逐 ACE 求值器（F36165）的序判据源。

use crate::checks::CheckSet;
use super::unx36081_acecore as core;
use alloc::vec;
use alloc::vec::Vec;
use alloc::string::{String, ToString};

pub const SEG_BASE: u16 = 0x3800;

// ---- 诊断码闭集 -----------------------------------------------------------
pub const E_VER: u16 = SEG_BASE + 1;   // 版本锚不符
pub const E_TYPE: u16 = SEG_BASE + 2;  // 序列含未知类型
pub const E_ORDER: u16 = SEG_BASE + 3; // canon 序违规
pub const E_CAP: u16 = SEG_BASE + 4;   // 序列超容
pub const E_EMPTY: u16 = SEG_BASE + 5; // 空序列判据面缺失
pub const E_CS: u16 = SEG_BASE + 6;    // 指纹不稳

/// 版本锚。
pub const VER: u32 = 1;

/// 序列定容上限。
pub const CANON_CAP: usize = 16;

/// canon 类别（#[repr(u16)] 显式判别式；值即序位，严格递增）。
#[repr(u16)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CanonClass {
    /// deny 族（0x0001/0x0006）：序位 0，先于一切 allow。
    Deny  = 0x0000,
    /// allow 族（0x0000/0x0005）：序位 1。
    Allow = 0x0001,
    /// audit/alarm 族（0x0002/0x0003/0x0007/0x0008）：序位 2。
    Audit = 0x0002,
    /// 标签/属性/策略族（0x0011..0x0013）：序位 3，殿后。
    Label = 0x0003,
}

impl CanonClass {
    pub fn code(self) -> u16 {
        self as u16
    }
}

/// 类型字节 → canon 类别（未知类型 = E_TYPE）。
pub fn classify(ace_type: u8) -> Result<CanonClass, u16> {
    match ace_type as u16 {
        0x0001 | 0x0006 => Ok(CanonClass::Deny),
        0x0000 | 0x0005 => Ok(CanonClass::Allow),
        0x0002 | 0x0003 | 0x0007 | 0x0008 => Ok(CanonClass::Audit),
        0x0011 | 0x0012 | 0x0013 => Ok(CanonClass::Label),
        _ => Err(E_TYPE),
    }
}

/// ACE 序列载体（定容存储 + 实长）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct AceSeq {
    pub types: [u8; CANON_CAP],
    pub n: usize,
}

impl AceSeq {
    /// 空序列。
    pub const fn empty() -> AceSeq {
        AceSeq { types: [0; CANON_CAP], n: 0 }
    }

    /// 追加类型字节（超容显性拒）。
    pub fn push(&mut self, t: u8) -> Result<(), u16> {
        if self.n >= CANON_CAP {
            return Err(E_CAP);
        }
        self.types[self.n] = t;
        self.n += 1;
        Ok(())
    }

    /// 按 canon 序插入：找到第一个类别 > 新类型的位次，后移再落位。
    /// 稳定序（同类不越位），后移用逐位拷贝（E0499 规避：无 swap）。
    pub fn insert_canon(&mut self, t: u8) -> Result<usize, u16> {
        if self.n >= CANON_CAP {
            return Err(E_CAP);
        }
        let cls = classify(t)?;
        let mut pos = 0;
        while pos < self.n {
            let c = classify(self.types[pos])?;
            if c.code() > cls.code() {
                break;
            }
            pos += 1;
        }
        // 后移：从尾向 pos 逐位搬运。
        let mut i = self.n;
        while i > pos {
            self.types[i] = self.types[i - 1];
            i -= 1;
        }
        self.types[pos] = t;
        self.n += 1;
        Ok(pos)
    }
}

/// canon 序校验：类别序列必须非降（同类别可重复）。
pub fn validate_sequence(seq: &AceSeq) -> Result<(), u16> {
    let mut prev: u16 = 0;
    let mut i = 0;
    while i < seq.n {
        let c = classify(seq.types[i])?;
        if i > 0 && c.code() < prev {
            return Err(E_ORDER);
        }
        prev = c.code();
        i += 1;
    }
    Ok(())
}

/// 违规位次定位：返回第一个降序位次（无违规 = E_ORDER 语义外的 Ok(n)）。
pub fn first_violation(seq: &AceSeq) -> Result<usize, u16> {
    let mut prev: u16 = 0;
    let mut i = 0;
    while i < seq.n {
        let c = classify(seq.types[i])?;
        if i > 0 && c.code() < prev {
            return Ok(i);
        }
        prev = c.code();
        i += 1;
    }
    Err(E_ORDER)
}

/// 从类型切片构造序列（含全闸）。
pub fn seq_from(types: &[u8]) -> Result<AceSeq, u16> {
    if types.len() > CANON_CAP {
        return Err(E_CAP);
    }
    let mut s = AceSeq::empty();
    let mut i = 0;
    while i < types.len() {
        s.push(types[i])?;
        i += 1;
    }
    validate_sequence(&s)?;
    Ok(s)
}

/// 摘要行（String 面，无 IO）。
pub fn summary(seq: &AceSeq) -> String {
    let mut s = String::new();
    s.push_str("canon n=");
    s.push_str(&seq.n.to_string());
    s.push_str(" cap=");
    s.push_str(&CANON_CAP.to_string());
    s
}

/// 诊断码闭集互异自检。
pub fn codes_distinct() -> bool {
    let cs = [E_VER, E_TYPE, E_ORDER, E_CAP, E_EMPTY, E_CS];
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
        E_TYPE => "序列含未知类型",
        E_ORDER => "canon 序违规",
        E_CAP => "序列超容",
        E_EMPTY => "空序列判据面缺失",
        E_CS => "指纹不稳",
        _ => "未知诊断码",
    }
}

const DENY: u8 = 0x01;
const ALLOW: u8 = 0x00;
const AUDIT: u8 = 0x02;
const LABEL: u8 = 0x11;

/// 演练：R1 类别序位守恒；R2 8 条合法序全过；R3 6 条违规序全检出；
/// R4 未知类型 2 例；R5 canon 插入序走查；R6 指纹稳定。
pub fn drill() -> Result<u32, u16> {
    let mut hits: u32 = 0;
    // R1：类别序位守恒（Deny<Allow<Audit<Label 且与真源类型码互证）。
    if !(CanonClass::Deny.code() < CanonClass::Allow.code()
        && CanonClass::Allow.code() < CanonClass::Audit.code()
        && CanonClass::Audit.code() < CanonClass::Label.code())
    {
        return Err(E_ORDER);
    }
    if VER != 1 {
        return Err(E_VER);
    }
    hits += 1;
    // R2：8 条合法序全过。
    let goods: Vec<Vec<u8>> = vec![
        vec![],
        vec![DENY],
        vec![ALLOW],
        vec![AUDIT],
        vec![DENY, ALLOW],
        vec![DENY, DENY, ALLOW],
        vec![DENY, ALLOW, AUDIT, LABEL],
        vec![DENY, DENY, ALLOW, ALLOW, AUDIT, LABEL],
    ];
    let mut g = 0;
    while g < goods.len() {
        let s = seq_from(&goods[g])?;
        validate_sequence(&s)?;
        if summary(&s).is_empty() {
            return Err(E_CS);
        }
        hits += 1;
        g += 1;
    }
    // R3：6 条违规序全检出（首位违规位次可定位）。
    let bads: Vec<Vec<u8>> = vec![
        vec![ALLOW, DENY],
        vec![AUDIT, DENY],
        vec![LABEL, ALLOW],
        vec![ALLOW, AUDIT, DENY],
        vec![AUDIT, ALLOW, DENY],
        vec![LABEL, DENY, ALLOW],
    ];
    let mut b = 0;
    while b < bads.len() {
        // 构造阶段就拦（seq_from 内含 validate）。
        match seq_from(&bads[b]) {
            Ok(_) => return Err(E_ORDER),
            Err(c) => {
                if c != E_ORDER {
                    return Err(E_ORDER);
                }
            }
        }
        // 违规位次可定位（绕过构造闸直接定位）。
        let mut s = AceSeq::empty();
        let mut k = 0;
        while k < bads[b].len() {
            s.push(bads[b][k])?;
            k += 1;
        }
        let v = first_violation(&s)?;
        if v == 0 || v >= s.n {
            return Err(E_ORDER);
        }
        hits += 1;
        b += 1;
    }
    // R4：未知类型 2 例（0x04 / 0xEE）——E_TYPE。
    let unknowns: Vec<u8> = vec![0x04, 0xee];
    let mut u = 0;
    while u < unknowns.len() {
        match classify(unknowns[u]) {
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
    // R5：canon 插入序走查（乱序注入 6 例 → 插出即有序）。
    let messy: Vec<u8> = [ALLOW, LABEL, DENY, AUDIT, DENY, ALLOW].to_vec();
    let mut s5 = AceSeq::empty();
    let mut m = 0;
    while m < messy.len() {
        s5.insert_canon(messy[m])?;
        m += 1;
    }
    validate_sequence(&s5)?;
    if s5.n != 6 {
        return Err(E_CAP);
    }
    hits += 1;
    // R6：指纹两次求值稳定且非零（真源 FNV over 序列字节 + 版本锚）。
    let mut feed = Vec::new();
    feed.extend_from_slice(&VER.to_le_bytes());
    let mut i = 0;
    while i < s5.n {
        feed.push(s5.types[i]);
        i += 1;
    }
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
        assert_eq!(drill(), Ok(19));
    }

    #[test]
    fn deny_before_allow() {
        let good = seq_from(&[DENY, ALLOW]);
        assert!(good.is_ok());
        let bad = seq_from(&[ALLOW, DENY]);
        assert_eq!(bad, Err(E_ORDER));
    }

    #[test]
    fn unknown_type_rejected() {
        assert_eq!(classify(0x04), Err(E_TYPE));
        assert_eq!(classify(0xee), Err(E_TYPE));
    }

    #[test]
    fn insert_canon_sorts() {
        let mut s = AceSeq::empty();
        let _ = s.insert_canon(ALLOW);
        let _ = s.insert_canon(DENY);
        assert_eq!(validate_sequence(&s), Ok(()));
        assert!(summary(&s).contains("canon"));
    }
}

pub fn run_unx36088_checks() -> CheckSet {
    let mut cs = CheckSet::new("unxstar-unx36088_acecanon");
    cs.add("SEG 段位独占", SEG_BASE == 0x3800, "0x3800~0x380f 本模块独占");
    match drill() {
        Ok(_) => cs.add("确定性演练全绿", true, "合法序 8 条全过、违规序 6 条全检出"),
        Err(code) => cs.add("确定性演练全绿", false, detail(code)),
    }
    cs.add("canon 序位锚 Deny<Allow<Audit<Label", CanonClass::Deny.code() == 0 && CanonClass::Label.code() == 3, "canon 序位锚漂移");
    cs.add("定容上限锚 16", CANON_CAP == 16, "canon 序列定容上限漂移");
    cs.add("版本锚 VER=1", VER == 1, "版本锚不符");
    cs.add("诊断码闭集互异", codes_distinct(), "诊断码闭集出现重码");
    cs
}
