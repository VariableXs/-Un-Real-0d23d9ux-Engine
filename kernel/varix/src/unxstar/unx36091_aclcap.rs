//! UNX-F36091-J1 ACL 容量与尺寸上限（防注入式膨胀）。
//! 判据：① 尺寸 ≤0xFFFF、ACE 数 ≤ 定容上限、余量账与理论最坏承载表对平；
//! ② 超限注入（尺寸溢出 2 例、满容 1 例）显性拒绝；③ 纯功能代码 ≥300 行。
//! 设计纪律：消费真源 unx36089_aclcore（Acl/add/守恒式/上限常量），
//! 本模块是容量审计面——36092 的插入原语插入前过本模块的容量预检。

use crate::checks::CheckSet;
use super::unx36081_acecore as core;
use super::unx36089_aclcore as aclc;
use alloc::vec;
use alloc::vec::Vec;
use alloc::string::{String, ToString};

pub const SEG_BASE: u16 = 0x3830;

// ---- 诊断码闭集 -----------------------------------------------------------
pub const E_VER: u16 = SEG_BASE + 1;     // 版本锚不符
pub const E_OVERSIZE: u16 = SEG_BASE + 2;// 尺寸越 0xFFFF
pub const E_OVERCOUNT: u16 = SEG_BASE + 3;// ACE 数越定容上限
pub const E_NOHEAD: u16 = SEG_BASE + 4;  // 余量账为负（账目破）
pub const E_CONS: u16 = SEG_BASE + 5;    // 守恒式破坏
pub const E_CS: u16 = SEG_BASE + 6;      // 指纹不稳

/// 版本锚。
pub const VER: u32 = 1;

/// ACL 尺寸上限锚（与真源同锚）。
pub const SIZE_CAP: usize = aclc::ACL_SIZE_MAX;
/// 定容上限锚（与真源同锚）。
pub const COUNT_CAP: usize = aclc::ACL_CAP;
/// 单 ACE 尺寸上限（防单发膨胀）。
pub const ACE_SIZE_CAP: usize = 0x1000;

/// 容量判定类别（#[repr(u16)] 显式判别式）。
#[repr(u16)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CapVerdict {
    /// 在容。
    Within    = 0x0001,
    /// 尺寸越限。
    Oversize  = 0x0002,
    /// 数量越限。
    Overcount = 0x0003,
    /// 余量账为负。
    NoHead    = 0x0004,
}

impl CapVerdict {
    pub fn code(self) -> u16 {
        self as u16
    }
}

/// 余量账：SIZE_CAP - 当前尺寸（为负 = 账目破）。
pub fn headroom(acl: &aclc::Acl) -> Result<usize, u16> {
    let used = acl.header.size as usize;
    if used > SIZE_CAP {
        return Err(E_NOHEAD);
    }
    Ok(SIZE_CAP - used)
}

/// 容量预检：插入 size 字节的新 ACE 前调用（数量闸 → 尺寸闸 → 单发闸）。
pub fn precheck(acl: &aclc::Acl, incoming_size: usize) -> Result<CapVerdict, u16> {
    if acl.header.ace_count as usize + 1 > COUNT_CAP {
        return Err(E_OVERCOUNT);
    }
    if incoming_size > ACE_SIZE_CAP {
        return Err(E_OVERSIZE);
    }
    let hr = headroom(acl)?;
    if incoming_size > hr {
        return Err(E_OVERSIZE);
    }
    Ok(CapVerdict::Within)
}

/// 全容器审计（36092 收口用）。
pub fn audit_acl(acl: &aclc::Acl) -> Result<CapVerdict, u16> {
    acl.check_conservation()?;
    if acl.header.size as usize > SIZE_CAP {
        return Err(E_OVERSIZE);
    }
    if acl.header.ace_count as usize > COUNT_CAP {
        return Err(E_OVERCOUNT);
    }
    Ok(CapVerdict::Within)
}

/// 理论最坏承载：给定字节预算，最小 ACE 尺寸下最多装几条（预算不足头 8 字节 = 0）。
pub fn worst_case_entries(size_budget: usize) -> usize {
    if size_budget < aclc::ACL_HEADER_SIZE + aclc::ACE_MIN_SIZE {
        return 0;
    }
    (size_budget - aclc::ACL_HEADER_SIZE) / aclc::ACE_MIN_SIZE
}

/// 最坏承载锚表（誊录面：36099/36100 对拍用）。
pub const WORST_CASE_TABLE: [usize; 4] = [
    (0xffff - aclc::ACL_HEADER_SIZE) / aclc::ACE_MIN_SIZE,
    (0x1000 - aclc::ACL_HEADER_SIZE) / aclc::ACE_MIN_SIZE,
    0,
    0,
];

/// 批量预检：按序对一组新 ACE 尺寸做滚动预检（不落账，纯预演）。
/// 任一发越限即返回位次与错误（否则返回全部发数）。
pub fn burst_precheck(acl: &aclc::Acl, incoming: &[usize]) -> Result<usize, u16> {
    let mut used = acl.header.size as usize;
    let mut count = acl.header.ace_count as usize;
    let mut i = 0;
    while i < incoming.len() {
        if count + 1 > COUNT_CAP {
            return Err(E_OVERCOUNT);
        }
        if incoming[i] > ACE_SIZE_CAP {
            return Err(E_OVERSIZE);
        }
        if used + incoming[i] > SIZE_CAP {
            return Err(E_OVERSIZE);
        }
        used += incoming[i];
        count += 1;
        i += 1;
    }
    Ok(incoming.len())
}

/// 容量报告（String 面，无 IO：余量/计数/最坏承载三行）。
pub fn cap_report(acl: &aclc::Acl) -> String {
    let hr = headroom(acl).unwrap_or(0);
    let mut s = String::new();
    s.push_str("report head=");
    s.push_str(&hr.to_string());
    s.push_str(" worst=");
    s.push_str(&worst_case_entries(hr).to_string());
    s.push_str(" count=");
    s.push_str(&acl.header.ace_count.to_string());
    s.push_str("/");
    s.push_str(&COUNT_CAP.to_string());
    s
}

/// 摘要行（String 面，无 IO）。
pub fn summary(acl: &aclc::Acl) -> String {
    let hr = headroom(acl).unwrap_or(0);
    let mut s = String::new();
    s.push_str("cap count=");
    s.push_str(&acl.header.ace_count.to_string());
    s.push_str("/");
    s.push_str(&COUNT_CAP.to_string());
    s.push_str(" head=");
    s.push_str(&hr.to_string());
    s
}

/// 诊断码闭集互异自检。
pub fn codes_distinct() -> bool {
    let cs = [E_VER, E_OVERSIZE, E_OVERCOUNT, E_NOHEAD, E_CONS, E_CS];
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
        E_OVERSIZE => "尺寸越 0xFFFF",
        E_OVERCOUNT => "ACE 数越定容上限",
        E_NOHEAD => "余量账为负（账目破）",
        E_CONS => "守恒式破坏",
        E_CS => "指纹不稳",
        _ => "未知诊断码",
    }
}

/// 演练：R1 锚位互证；R2 12 例逐条预检-追加-审计全链；R3 尺寸溢出注入 2 例；
/// R4 满容注入 1 例；R5 余量账对平 2 例；R6 最坏承载表 4 例；R7 批量预检 3 例；
/// R8 指纹稳定 + 报告面。
pub fn drill() -> Result<u32, u16> {
    let mut hits: u32 = 0;
    // R1：锚位互证（本模块锚 == 真源锚）。
    if SIZE_CAP != aclc::ACL_SIZE_MAX || COUNT_CAP != aclc::ACL_CAP {
        return Err(E_VER);
    }
    if VER != 1 {
        return Err(E_VER);
    }
    hits += 1;
    // R2：12 例逐条「预检 → 追加 → 审计 → 余量对平」全链。
    let mut acl = aclc::Acl::fresh(2);
    let mut i: usize = 0;
    while i < 12 {
        let sz = aclc::ACE_MIN_SIZE + (i % 3) * 4;
        let v = precheck(&acl, sz)?;
        if v.code() != CapVerdict::Within.code() {
            return Err(E_NOHEAD);
        }
        let e = aclc::AceEntry::new(0x00, 0x00, sz as u16, 0x0001_2003, (i as u16) + 1)?;
        acl.add(e)?;
        audit_acl(&acl)?;
        let expect_head = SIZE_CAP - acl.header.size as usize;
        if headroom(&acl)? != expect_head {
            return Err(E_NOHEAD);
        }
        hits += 1;
        i += 1;
    }
    // R3：尺寸溢出注入 2 例（单发 0xf000 可容；复检越余量/越单发上限拒）。
    let mut big = aclc::Acl::fresh(2);
    let e1 = aclc::AceEntry::new(0x00, 0x00, 0xf000, 0x0000_0001, 1)?;
    big.add(e1)?;
    match precheck(&big, 0xf000) {
        Ok(_) => return Err(E_OVERSIZE),
        Err(c) => {
            if c != E_OVERSIZE {
                return Err(E_OVERSIZE);
            }
        }
    }
    hits += 1;
    match precheck(&big, ACE_SIZE_CAP + 1) {
        Ok(_) => return Err(E_OVERSIZE),
        Err(c) => {
            if c != E_OVERSIZE {
                return Err(E_OVERSIZE);
            }
        }
    }
    hits += 1;
    // R4：满容注入 1 例（补满 32 槽后再 precheck → E_OVERCOUNT）。
    let mut full = aclc::Acl::fresh(2);
    let mut k = 0;
    while k < COUNT_CAP {
        let e = aclc::AceEntry::new(0x00, 0x00, aclc::ACE_MIN_SIZE as u16, 0x0000_0001, 1)?;
        full.add(e)?;
        k += 1;
    }
    match precheck(&full, aclc::ACE_MIN_SIZE) {
        Ok(_) => return Err(E_OVERCOUNT),
        Err(c) => {
            if c != E_OVERCOUNT {
                return Err(E_OVERCOUNT);
            }
        }
    }
    audit_acl(&full)?;
    hits += 1;
    // R5：余量账对平 2 例（空容器 / 32 槽满载最小尺寸容器）。
    let empty = aclc::Acl::fresh(2);
    if headroom(&empty)? != SIZE_CAP - aclc::ACL_HEADER_SIZE {
        return Err(E_NOHEAD);
    }
    hits += 1;
    let expect_full_head = SIZE_CAP - (aclc::ACL_HEADER_SIZE + COUNT_CAP * aclc::ACE_MIN_SIZE);
    if headroom(&full)? != expect_full_head {
        return Err(E_NOHEAD);
    }
    hits += 1;
    // R6：最坏承载表 4 例（表誊录对拍 + 逐预算复核）。
    let budgets: Vec<usize> = vec![0xffff, 0x1000, 8, 4];
    let mut w = 0;
    while w < budgets.len() {
        if worst_case_entries(budgets[w]) != WORST_CASE_TABLE[w] {
            return Err(E_NOHEAD);
        }
        hits += 1;
        w += 1;
    }
    // R7：批量预检 3 例（5 发全容 / 越单发上限 / 越定容数）。
    let mid = aclc::Acl::fresh(2);
    let burst: Vec<usize> = vec![
        aclc::ACE_MIN_SIZE, aclc::ACE_MIN_SIZE, aclc::ACE_MIN_SIZE,
        aclc::ACE_MIN_SIZE, aclc::ACE_MIN_SIZE,
    ];
    let ok_n = burst_precheck(&mid, &burst)?;
    if ok_n != 5 {
        return Err(E_NOHEAD);
    }
    hits += 1;
    let bad_burst: Vec<usize> = vec![8, 8, SIZE_CAP - 16 + 1];
    match burst_precheck(&mid, &bad_burst) {
        Ok(_) => return Err(E_OVERSIZE),
        Err(c) => {
            if c != E_OVERSIZE {
                return Err(E_OVERSIZE);
            }
        }
    }
    hits += 1;
    let count_burst: Vec<usize> = vec![8; COUNT_CAP + 2];
    match burst_precheck(&mid, &count_burst) {
        Ok(_) => return Err(E_OVERCOUNT),
        Err(c) => {
            if c != E_OVERCOUNT {
                return Err(E_OVERCOUNT);
            }
        }
    }
    hits += 1;
    // R8：容量报告非空 + 指纹两次求值稳定且非零（真源 FNV over 容器指纹 + 版本锚）。
    let fp1 = aclc::acl_fingerprint(&acl) ^ core::fnv1a(&VER.to_le_bytes());
    let fp2 = aclc::acl_fingerprint(&acl) ^ core::fnv1a(&VER.to_le_bytes());
    if fp1 != fp2 || fp1 == 0 {
        return Err(E_CS);
    }
    hits += 1;
    if cap_report(&acl).is_empty() {
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
        assert_eq!(drill(), Ok(27));
    }

    #[test]
    fn burst_precheck_shapes() {
        let acl = aclc::Acl::fresh(2);
        assert_eq!(burst_precheck(&acl, &[8, 8, 8]), Ok(3));
        assert_eq!(burst_precheck(&acl, &[8, 0x2000]), Err(E_OVERSIZE));
        assert!(cap_report(&acl).contains("worst"));
    }

    #[test]
    fn oversize_rejected() {
        let acl = aclc::Acl::fresh(2);
        assert_eq!(precheck(&acl, 0x1001), Err(E_OVERSIZE));
    }

    #[test]
    fn overcount_rejected() {
        let mut acl = aclc::Acl::fresh(2);
        let e = aclc::AceEntry::new(0, 0, 8, 1, 1).unwrap_or(aclc::AceEntry {
            ace_type: 0, flags: 0, size: 0, mask: 0, sid_slot: 0,
        });
        let _ = acl.add(e);
        let mut n = 0;
        while n < COUNT_CAP - 1 {
            let _ = acl.add(e);
            n += 1;
        }
        assert_eq!(precheck(&acl, 8), Err(E_OVERCOUNT));
    }

    #[test]
    fn headroom_shapes() {
        let acl = aclc::Acl::fresh(2);
        assert!(summary(&acl).contains("cap"));
        assert_eq!(worst_case_entries(4), 0);
        assert_eq!(audit_acl(&acl), Ok(CapVerdict::Within));
    }
}

pub fn run_unx36091_checks() -> CheckSet {
    let mut cs = CheckSet::new("unxstar-unx36091_aclcap");
    cs.add("SEG 段位独占", SEG_BASE == 0x3830, "0x3830~0x383f 本模块独占");
    match drill() {
        Ok(_) => cs.add("确定性演练全绿", true, "容量账全程对平、超限注入 3 例全拒"),
        Err(code) => cs.add("确定性演练全绿", false, detail(code)),
    }
    cs.add("尺寸上限锚 0xFFFF", SIZE_CAP == 0xffff, "尺寸上限锚漂移");
    cs.add("单发尺寸锚 0x1000", ACE_SIZE_CAP == 0x1000, "单发尺寸锚与真源漂移");
    cs.add("版本锚 VER=1", VER == 1, "版本锚不符");
    cs.add("诊断码闭集互异", codes_distinct(), "诊断码闭集出现重码");
    cs
}
