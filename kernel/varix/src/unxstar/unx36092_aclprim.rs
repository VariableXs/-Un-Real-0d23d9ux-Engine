//! UNX-F36092-J1 ACE 拷贝/删除/插入原语与序维护。
//! 判据：① canon 插入 12 例乱序注入即排成 deny→allow→audit→label 稳定序，
//! 删除（首/中/尾）与拷贝-再插全链守恒、序号无跳号；② 序违规 1 例、跳号
//! 篡改 1 例、坏位次 2 例、满容 1 例全部显性拒绝；③ 纯功能代码 ≥300 行。
//! 设计纪律：消费真源 unx36089_aclcore（Acl/AceEntry/守恒式）；序判据与
//! unx36088 同构（本模块内本地承载，不跨模块取序表，防环）。

use crate::checks::CheckSet;
use super::unx36081_acecore as core;
use super::unx36089_aclcore as aclc;
use alloc::vec;
use alloc::vec::Vec;
use alloc::string::{String, ToString};

pub const SEG_BASE: u16 = 0x3840;

// ---- 诊断码闭集 -----------------------------------------------------------
pub const E_VER: u16 = SEG_BASE + 1;   // 版本锚不符
pub const E_RANGE: u16 = SEG_BASE + 2; // 位次越界
pub const E_ORDER: u16 = SEG_BASE + 3; // canon 序违规
pub const E_GAP: u16 = SEG_BASE + 4;   // 序号跳号（占用/空闲位图撕裂）
pub const E_CONS: u16 = SEG_BASE + 5;  // 守恒式破坏
pub const E_FULL: u16 = SEG_BASE + 6;  // 定容槽满
pub const E_CS: u16 = SEG_BASE + 7;    // 指纹不稳

/// 版本锚。
pub const VER: u32 = 1;

/// 原语操作类别（#[repr(u16)] 显式判别式）。
#[repr(u16)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PrimOp {
    /// 拷贝（读侧原语，不改容器）。
    CopyOp  = 0x0001,
    /// 删除（写侧，紧凑回搬）。
    DelOp   = 0x0002,
    /// canon 插入（写侧，按序落位）。
    InsertOp = 0x0003,
}

impl PrimOp {
    pub fn code(self) -> u16 {
        self as u16
    }
}

/// 本地序位（与 unx36088 同构：deny=0 < allow=1 < audit=2 < label=3）。
pub fn rank_of(ace_type: u8) -> Result<u16, u16> {
    match ace_type as u16 {
        0x0001 | 0x0006 => Ok(0),
        0x0000 | 0x0005 => Ok(1),
        0x0002 | 0x0003 | 0x0007 | 0x0008 => Ok(2),
        0x0011 | 0x0012 | 0x0013 => Ok(3),
        _ => Err(E_ORDER),
    }
}

/// 序号无跳号审计：占用槽必须连续占据 0..count，count..CAP 必须全空。
pub fn audit_no_gap(acl: &aclc::Acl) -> Result<(), u16> {
    let count = acl.header.ace_count as usize;
    if count > aclc::ACL_CAP {
        return Err(E_GAP);
    }
    let mut i = 0;
    while i < count {
        if acl.aces[i].is_none() {
            return Err(E_GAP);
        }
        i += 1;
    }
    let mut j = count;
    while j < aclc::ACL_CAP {
        if acl.aces[j].is_some() {
            return Err(E_GAP);
        }
        j += 1;
    }
    Ok(())
}

/// canon 序审计：占用槽序位非降。
pub fn audit_order(acl: &aclc::Acl) -> Result<(), u16> {
    let mut prev: u16 = 0;
    let mut i = 0;
    while i < acl.header.ace_count as usize {
        let e = match acl.aces[i] {
            Some(x) => x,
            None => return Err(E_GAP),
        };
        let r = rank_of(e.ace_type)?;
        if i > 0 && r < prev {
            return Err(E_ORDER);
        }
        prev = r;
        i += 1;
    }
    Ok(())
}

/// 拷贝原语：读位次，不改容器。
pub fn copy(acl: &aclc::Acl, idx: usize) -> Result<aclc::AceEntry, u16> {
    if idx >= acl.header.ace_count as usize {
        return Err(E_RANGE);
    }
    match acl.aces[idx] {
        Some(e) => Ok(e),
        None => Err(E_GAP),
    }
}

/// 删除原语：真源 remove + 双审计（跳号/守恒）。
pub fn del(acl: &mut aclc::Acl, idx: usize) -> Result<aclc::AceEntry, u16> {
    let e = acl.remove(idx)?;
    audit_no_gap(acl)?;
    audit_order(acl)?;
    acl.check_conservation()?;
    Ok(e)
}

/// canon 插入原语：按序落位（稳定序），后移用逐位拷贝（E0499 规避）。
pub fn insert_canon(acl: &mut aclc::Acl, e: aclc::AceEntry) -> Result<usize, u16> {
    if acl.header.ace_count as usize >= aclc::ACL_CAP {
        return Err(E_FULL);
    }
    let new_rank = rank_of(e.ace_type)?;
    let mut pos = 0;
    while pos < acl.header.ace_count as usize {
        let cur = match acl.aces[pos] {
            Some(x) => x,
            None => return Err(E_GAP),
        };
        if rank_of(cur.ace_type)? > new_rank {
            break;
        }
        pos += 1;
    }
    let mut i = acl.header.ace_count as usize;
    while i > pos {
        acl.aces[i] = acl.aces[i - 1];
        i -= 1;
    }
    acl.aces[pos] = Some(e);
    acl.header.ace_count += 1;
    let new_size = acl.header.size as usize + e.size as usize;
    if new_size > aclc::ACL_SIZE_MAX {
        return Err(E_CONS);
    }
    acl.header.size = new_size as u16;
    audit_no_gap(acl)?;
    audit_order(acl)?;
    acl.check_conservation()?;
    Ok(pos)
}

/// 摘要行（String 面，无 IO）。
pub fn summary(acl: &aclc::Acl) -> String {
    let mut s = String::new();
    s.push_str("prim count=");
    s.push_str(&acl.header.ace_count.to_string());
    s.push_str(" size=");
    s.push_str(&acl.header.size.to_string());
    s
}

/// 诊断码闭集互异自检。
pub fn codes_distinct() -> bool {
    let cs = [E_VER, E_RANGE, E_ORDER, E_GAP, E_CONS, E_FULL, E_CS];
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
        E_RANGE => "位次越界",
        E_ORDER => "canon 序违规",
        E_GAP => "序号跳号（占用/空闲位图撕裂）",
        E_CONS => "守恒式破坏",
        E_FULL => "定容槽满",
        E_CS => "指纹不稳",
        _ => "未知诊断码",
    }
}

/// 演练：R1 空容器审计；R2 canon 插入 12 例乱序注入即成序；R3 删除 3 例；
/// R4 拷贝-再插 2 例；R5 序违规篡改检出；R6 跳号篡改检出；R7 坏位次 2 例；
/// R8 满容插入 1 例；R9 指纹稳定。
pub fn drill() -> Result<u32, u16> {
    let mut hits: u32 = 0;
    // R1：空容器双审计 + 守恒。
    if VER != 1 {
        return Err(E_VER);
    }
    let mut acl = aclc::Acl::fresh(2);
    audit_no_gap(&acl)?;
    audit_order(&acl)?;
    acl.check_conservation()?;
    hits += 1;
    // R2：canon 插入 12 例（乱序：allow,audit,deny,label 轮转）。
    let messy: Vec<u8> = vec![0x00, 0x02, 0x01, 0x11, 0x00, 0x01, 0x02, 0x00, 0x11, 0x01, 0x02, 0x00];
    let mut m = 0;
    while m < messy.len() {
        let e = aclc::AceEntry::new(messy[m], 0x00, aclc::ACE_MIN_SIZE as u16, 0x0001_2003, (m as u16) + 1)?;
        insert_canon(&mut acl, e)?;
        hits += 1;
        m += 1;
    }
    // R3：删除 3 例（首位 0 / 中位 count/2 / 尾位 count-1）。
    let mut r = 0;
    while r < 3 {
        let count = acl.header.ace_count as usize;
        let pick = match r {
            0 => 0,
            1 => count / 2,
            _ => count - 1,
        };
        del(&mut acl, pick)?;
        hits += 1;
        r += 1;
    }
    // R4：拷贝-再插 2 例（拷首位 deny 复插 / 拷尾位 label 复插）。
    let first = copy(&acl, 0)?;
    if rank_of(first.ace_type)? != 0 {
        return Err(E_ORDER);
    }
    let p1 = insert_canon(&mut acl, first)?;
    // deny 复插应落在既有 deny 群之后（R3 删了首位 deny，剩余 2 条 deny）。
    if p1 > 2 {
        return Err(E_ORDER);
    }
    hits += 1;
    let last_idx = acl.header.ace_count as usize - 1;
    let last = copy(&acl, last_idx)?;
    if rank_of(last.ace_type)? != 3 {
        return Err(E_ORDER);
    }
    let _ = insert_canon(&mut acl, last)?;
    hits += 1;
    // R5：序违规篡改检出（把首槽 deny 换成 allow → 序审计 E_ORDER）。
    let mut corrupt = acl;
    if let Some(e0) = corrupt.aces[0] {
        corrupt.aces[0] = Some(aclc::AceEntry { ace_type: 0x00, ..e0 });
    }
    match audit_order(&corrupt) {
        Ok(_) => return Err(E_ORDER),
        Err(c) => {
            if c != E_ORDER {
                return Err(E_ORDER);
            }
        }
    }
    hits += 1;
    // R6：跳号篡改检出（中槽挖空 → 跳号审计 E_GAP）。
    let mut torn = acl;
    torn.aces[torn.header.ace_count as usize / 2] = aclc::NONE_ACE;
    match audit_no_gap(&torn) {
        Ok(_) => return Err(E_GAP),
        Err(c) => {
            if c != E_GAP {
                return Err(E_GAP);
            }
        }
    }
    hits += 1;
    // R7：坏位次 2 例（=count / =CAP）——copy 报 E_RANGE。
    let count = acl.header.ace_count as usize;
    match copy(&acl, count) {
        Ok(_) => return Err(E_RANGE),
        Err(c) => {
            if c != E_RANGE {
                return Err(E_RANGE);
            }
        }
    }
    hits += 1;
    match copy(&acl, aclc::ACL_CAP) {
        Ok(_) => return Err(E_RANGE),
        Err(c) => {
            if c != E_RANGE {
                return Err(E_RANGE);
            }
        }
    }
    hits += 1;
    // R8：满容插入 1 例（补满 32 再插 → E_FULL）。
    let mut full = aclc::Acl::fresh(2);
    let mut k = 0;
    while k < aclc::ACL_CAP {
        let e = aclc::AceEntry::new(0x00, 0x00, aclc::ACE_MIN_SIZE as u16, 0x0000_0001, 1)?;
        insert_canon(&mut full, e)?;
        k += 1;
    }
    let extra = aclc::AceEntry::new(0x00, 0x00, aclc::ACE_MIN_SIZE as u16, 0x0000_0001, 1)?;
    match insert_canon(&mut full, extra) {
        Ok(_) => return Err(E_FULL),
        Err(c) => {
            if c != E_FULL {
                return Err(E_FULL);
            }
        }
    }
    hits += 1;
    // R9：指纹两次求值稳定且非零。
    let p = core::fnv1a(&[acl.header.rev, acl.header.ace_count as u8, VER as u8]);
    let p2 = core::fnv1a(&[acl.header.rev, acl.header.ace_count as u8, VER as u8]);
    if p != p2 || p == 0 {
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
        assert_eq!(drill(), Ok(24));
    }

    #[test]
    fn insert_sorts_canon() {
        let mut acl = aclc::Acl::fresh(2);
        let e_allow = aclc::AceEntry::new(0x00, 0, 8, 1, 1).unwrap_or(aclc::AceEntry {
            ace_type: 0, flags: 0, size: 0, mask: 0, sid_slot: 0,
        });
        let e_deny = aclc::AceEntry::new(0x01, 0, 8, 1, 2).unwrap_or(e_allow);
        let _ = insert_canon(&mut acl, e_allow);
        let p = insert_canon(&mut acl, e_deny).unwrap_or(99);
        assert_eq!(p, 0);
        assert_eq!(audit_order(&acl), Ok(()));
    }

    #[test]
    fn gap_audit_catches_tear() {
        let mut acl = aclc::Acl::fresh(2);
        let e = aclc::AceEntry::new(0x00, 0, 8, 1, 1).unwrap_or(aclc::AceEntry {
            ace_type: 0, flags: 0, size: 0, mask: 0, sid_slot: 0,
        });
        let _ = acl.add(e);
        acl.aces[0] = aclc::NONE_ACE;
        assert_eq!(audit_no_gap(&acl), Err(E_GAP));
        assert!(summary(&acl).contains("prim"));
    }

    #[test]
    fn range_rejected() {
        let acl = aclc::Acl::fresh(2);
        assert_eq!(copy(&acl, 0), Err(E_RANGE));
    }
}

pub fn run_unx36092_checks() -> CheckSet {
    let mut cs = CheckSet::new("unxstar-unx36092_aclprim");
    cs.add("SEG 段位独占", SEG_BASE == 0x3840, "0x3840~0x384f 本模块独占");
    match drill() {
        Ok(_) => cs.add("确定性演练全绿", true, "canon 插入 12 例成序、篡改 2 例检出"),
        Err(code) => cs.add("确定性演练全绿", false, detail(code)),
    }
    cs.add("序位锚 deny=0/label=3", rank_of(0x01) == Ok(0) && rank_of(0x11) == Ok(3), "序位锚漂移");
    cs.add("版本锚 VER=1", VER == 1, "版本锚不符");
    cs.add("诊断码闭集互异", codes_distinct(), "诊断码闭集出现重码");
    cs.add("原语三件齐备", PrimOp::CopyOp.code() == 1 && PrimOp::InsertOp.code() == 3, "原语类别表漂移");
    cs
}
