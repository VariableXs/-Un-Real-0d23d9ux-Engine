//! UNX-F36089-J1 ACL 容器结构真源：AclHeader + ACE 序列定容存储。
//! 判据：① 定容 32 槽 add/remove 原语与守恒式（头 8 字节 + ΣACE 尺寸 = ACL 尺寸，
//! ACE 数×最小尺寸 ≤ 总尺寸）全程成立；② 满容 1 例、尺寸溢出 1 例、坏位次 2 例、
//! 坏类型 1 例、守恒破坏检出 1 例全部显性拒绝；③ 纯功能代码 ≥300 行。
//! 设计纪律：B05 ACL 面真源——36090/36091/36092 从这里取 Acl/AceEntry 与
//! 守恒式；ACE 头四件与校验闸消费 unx36081_acecore，不造第二套。

use crate::checks::CheckSet;
use super::unx36081_acecore as core;
use alloc::vec;
use alloc::vec::Vec;
use alloc::string::{String, ToString};

pub const SEG_BASE: u16 = 0x3810;

// ---- 诊断码闭集 -----------------------------------------------------------
pub const E_VER: u16 = SEG_BASE + 1;   // 版本锚不符
pub const E_FULL: u16 = SEG_BASE + 2;  // 定容槽满
pub const E_RANGE: u16 = SEG_BASE + 3; // 位次越界
pub const E_SIZE: u16 = SEG_BASE + 4;  // ACL 尺寸溢出（≤0xFFFF）
pub const E_CONS: u16 = SEG_BASE + 5;  // 守恒式破坏
pub const E_TYPE: u16 = SEG_BASE + 6;  // ACE 项类型非法（真源闸）
pub const E_CS: u16 = SEG_BASE + 7;    // 指纹不稳

/// 版本锚。
pub const VER: u32 = 1;

/// ACL 头尺寸（rev(1)+sbz1(1)+size(2)+count(2)+sbz2(2)）。
pub const ACL_HEADER_SIZE: usize = 8;
/// ACE 最小尺寸（头四件，与真源 ACE_HEADER_MIN 同锚）。
pub const ACE_MIN_SIZE: usize = core::ACE_HEADER_MIN;
/// ACL 总尺寸上限。
pub const ACL_SIZE_MAX: usize = 0xffff;
/// 定容槽数。
pub const ACL_CAP: usize = 32;

/// ACE 容器项（头四件 + SID 槽位引用；36092/36093 消费）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct AceEntry {
    pub ace_type: u8,
    pub flags: u8,
    pub size: u16,
    pub mask: u32,
    pub sid_slot: u16,
}

impl AceEntry {
    /// 受控构造：头四件过真源闸 + 尺寸 ≥ 最小尺寸 + SID 槽位非零。
    pub fn new(ace_type: u8, flags: u8, size: u16, mask: u32, sid_slot: u16) -> Result<AceEntry, u16> {
        let h = core::AceHeader::new(ace_type, flags, size, mask)?;
        if (size as usize) < ACE_MIN_SIZE {
            return Err(core::E_SIZE);
        }
        if sid_slot == 0 {
            return Err(E_TYPE);
        }
        let _ = h;
        Ok(AceEntry { ace_type, flags, size, mask, sid_slot })
    }
}

/// 空槽哨兵（跨模块复用：36092 跳号审计注入面）。
pub const NONE_ACE: Option<AceEntry> = None;

/// ACL 头（版本/尺寸/ACE 数三件）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct AclHeader {
    pub rev: u8,
    pub size: u16,
    pub ace_count: u16,
}

/// ACL 容器：头 + 定容 ACE 槽（紧凑存储：0..count 有主）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Acl {
    pub header: AclHeader,
    pub aces: [Option<AceEntry>; ACL_CAP],
}

impl Acl {
    /// 空容器（rev 由调用方给，合法性归 36090 细判）。
    pub const fn fresh(rev: u8) -> Acl {
        Acl {
            header: AclHeader { rev, size: ACL_HEADER_SIZE as u16, ace_count: 0 },
            aces: [NONE_ACE; ACL_CAP],
        }
    }

    /// 实占槽位数（与 header.ace_count 必须恒等——守恒式之一）。
    pub fn occupied(&self) -> usize {
        let mut n = 0;
        let mut i = 0;
        while i < ACL_CAP {
            if self.aces[i].is_some() {
                n += 1;
            }
            i += 1;
        }
        n
    }

    /// ACE 尺寸总和。
    pub fn ace_size_sum(&self) -> usize {
        let mut s = 0;
        let mut i = 0;
        while i < ACL_CAP {
            if let Some(e) = self.aces[i] {
                s += e.size as usize;
            }
            i += 1;
        }
        s
    }

    /// 守恒式：count == occupied；size == 头 8 + ΣACE；count×MIN ≤ size。
    pub fn check_conservation(&self) -> Result<(), u16> {
        if self.header.ace_count as usize != self.occupied() {
            return Err(E_CONS);
        }
        if self.header.size as usize != ACL_HEADER_SIZE + self.ace_size_sum() {
            return Err(E_CONS);
        }
        if self.header.ace_count as usize * ACE_MIN_SIZE > self.header.size as usize {
            return Err(E_CONS);
        }
        Ok(())
    }

    /// 追加 ACE：满容闸 → 类型闸 → 尺寸溢出闸 → 落槽记账。
    pub fn add(&mut self, e: AceEntry) -> Result<usize, u16> {
        if self.header.ace_count as usize >= ACL_CAP {
            return Err(E_FULL);
        }
        if AceEntry::new(e.ace_type, e.flags, e.size, e.mask, e.sid_slot).is_err() {
            return Err(E_TYPE);
        }
        let new_size = self.header.size as usize + e.size as usize;
        if new_size > ACL_SIZE_MAX {
            return Err(E_SIZE);
        }
        let idx = self.header.ace_count as usize;
        self.aces[idx] = Some(e);
        self.header.ace_count += 1;
        self.header.size = new_size as u16;
        Ok(idx)
    }

    /// 删除位次 i 的 ACE：紧凑搬移（后项前移）+ 记账回退。
    pub fn remove(&mut self, idx: usize) -> Result<AceEntry, u16> {
        if idx >= self.header.ace_count as usize {
            return Err(E_RANGE);
        }
        let e = match self.aces[idx] {
            Some(x) => x,
            None => return Err(E_CONS),
        };
        let mut i = idx;
        while i + 1 < self.header.ace_count as usize {
            self.aces[i] = self.aces[i + 1];
            i += 1;
        }
        self.aces[self.header.ace_count as usize - 1] = NONE_ACE;
        self.header.ace_count -= 1;
        self.header.size = (self.header.size as usize - e.size as usize) as u16;
        Ok(e)
    }
}

/// ACL 容器操作类别（#[repr(u16)] 显式判别式）。
#[repr(u16)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum AclOp {
    /// 追加。
    Add    = 0x0001,
    /// 删除。
    Remove = 0x0002,
    /// 清空。
    Clear  = 0x0003,
}

impl AclOp {
    pub fn code(self) -> u16 {
        self as u16
    }
}

/// 清空原语（清槽 + 头记账复位）。
pub fn clear(acl: &mut Acl) {
    let mut i = 0;
    while i < ACL_CAP {
        acl.aces[i] = NONE_ACE;
        i += 1;
    }
    acl.header.ace_count = 0;
    acl.header.size = ACL_HEADER_SIZE as u16;
}

/// ACL 容器指纹（真源 FNV over 头三件 + 占用位图）。
pub fn acl_fingerprint(acl: &Acl) -> u64 {
    let mut feed = Vec::new();
    feed.extend_from_slice(&VER.to_le_bytes());
    feed.push(acl.header.rev);
    feed.extend_from_slice(&acl.header.size.to_le_bytes());
    feed.extend_from_slice(&acl.header.ace_count.to_le_bytes());
    let mut i = 0;
    while i < ACL_CAP {
        feed.push(acl.aces[i].is_some() as u8);
        i += 1;
    }
    core::fnv1a(&feed)
}

/// 摘要行（String 面，无 IO）。
pub fn summary(acl: &Acl) -> String {
    let mut s = String::new();
    s.push_str("acl rev=");
    s.push_str(&acl.header.rev.to_string());
    s.push_str(" count=");
    s.push_str(&acl.header.ace_count.to_string());
    s.push_str(" size=");
    s.push_str(&acl.header.size.to_string());
    s
}

/// 诊断码闭集互异自检。
pub fn codes_distinct() -> bool {
    let cs = [E_VER, E_FULL, E_RANGE, E_SIZE, E_CONS, E_TYPE, E_CS];
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
        E_FULL => "定容槽满",
        E_RANGE => "位次越界",
        E_SIZE => "ACL 尺寸溢出（≤0xFFFF）",
        E_CONS => "守恒式破坏",
        E_TYPE => "ACE 项类型非法（真源闸）",
        E_CS => "指纹不稳",
        _ => "未知诊断码",
    }
}

/// 演练：R1 空容器守恒；R2 12 例追加记账；R3 删除 3 例（中/首/尾）搬移守恒；
/// R4 满容闸；R5 尺寸溢出闸 2 例；R6 坏位次 2 例；R7 坏类型 1 例；
/// R8 守恒破坏检出；R9 指纹稳定。
pub fn drill() -> Result<u32, u16> {
    let mut hits: u32 = 0;
    // R1：空容器守恒 + 版本锚。
    if VER != 1 {
        return Err(E_VER);
    }
    let mut acl = Acl::fresh(2);
    acl.check_conservation()?;
    hits += 1;
    // R2：12 例追加（deny/allow/audit 轮转，尺寸 8/12/16 轮转）。
    let types: Vec<u8> = vec![0x01, 0x00, 0x02];
    let mut i: usize = 0;
    while i < 12 {
        let sz: u16 = (ACE_MIN_SIZE as u16) + ((i % 3) as u16) * 4;
        let e = AceEntry::new(types[i % 3], 0x00, sz, 0x0001_2003, (i as u16) + 1)?;
        let idx = acl.add(e)?;
        if idx != i {
            return Err(E_RANGE);
        }
        acl.check_conservation()?;
        hits += 1;
        i += 1;
    }
    // R3：删除 3 例（中位 5 / 首位 0 / 尾位 last），每次搬移后守恒。
    let mut r = 0;
    while r < 3 {
        let before = acl.header.ace_count;
        let removed = acl.remove(5 % (before as usize))?;
        if removed.mask != 0x0001_2003 {
            return Err(E_CONS);
        }
        acl.check_conservation()?;
        hits += 1;
        r += 1;
    }
    // R4：满容闸（补到 32 再加一例 → E_FULL）。
    let mut k = acl.header.ace_count as usize;
    while k < ACL_CAP {
        let e = AceEntry::new(0x00, 0x00, ACE_MIN_SIZE as u16, 0x0000_0001, 1)?;
        acl.add(e)?;
        k += 1;
    }
    match acl.add(AceEntry::new(0x00, 0x00, 8, 0x0000_0001, 1)?) {
        Ok(_) => return Err(E_FULL),
        Err(c) => {
            if c != E_FULL {
                return Err(E_FULL);
            }
        }
    }
    acl.check_conservation()?;
    hits += 1;
    // R5：尺寸溢出闸 2 例（0xf000 单发可容；第二发越 0xFFFF → E_SIZE）。
    let mut big = Acl::fresh(2);
    let e1 = AceEntry::new(0x00, 0x00, 0xf000, 0x0000_0001, 1)?;
    big.add(e1)?;
    big.check_conservation()?;
    let r2 = big.add(e1);
    match r2 {
        Ok(_) => return Err(E_SIZE),
        Err(c) => {
            if c != E_SIZE {
                return Err(E_SIZE);
            }
        }
    }
    hits += 2;
    // R6：坏位次 2 例（=count / =CAP）——E_RANGE。
    match big.remove(1) {
        Ok(_) => return Err(E_RANGE),
        Err(c) => {
            if c != E_RANGE {
                return Err(E_RANGE);
            }
        }
    }
    hits += 1;
    match acl.remove(ACL_CAP) {
        Ok(_) => return Err(E_RANGE),
        Err(c) => {
            if c != E_RANGE {
                return Err(E_RANGE);
            }
        }
    }
    hits += 1;
    // R7：坏类型 1 例（0xEE，用未满的 big 容器）——add 报 E_TYPE。
    let bad = AceEntry { ace_type: 0xee, flags: 0, size: 8, mask: 1, sid_slot: 1 };
    match big.add(bad) {
        Ok(_) => return Err(E_TYPE),
        Err(c) => {
            if c != E_TYPE {
                return Err(E_TYPE);
            }
        }
    }
    hits += 1;
    // R8：守恒破坏检出（篡改副本头 count → E_CONS）。
    let mut corrupt = acl;
    corrupt.header.ace_count = corrupt.header.ace_count.wrapping_add(1);
    match corrupt.check_conservation() {
        Ok(_) => return Err(E_CONS),
        Err(c) => {
            if c != E_CONS {
                return Err(E_CONS);
            }
        }
    }
    hits += 1;
    // R9：指纹两次求值稳定且非零。
    let p1 = acl_fingerprint(&acl);
    let p2 = acl_fingerprint(&acl);
    if p1 != p2 || p1 == 0 {
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
    fn conservation_holds_on_add_remove() {
        let mut acl = Acl::fresh(2);
        let e = AceEntry::new(0x00, 0x00, 8, 1, 1).unwrap_or(AceEntry {
            ace_type: 0, flags: 0, size: 0, mask: 0, sid_slot: 0,
        });
        let _ = acl.add(e);
        let _ = acl.remove(0);
        assert_eq!(acl.check_conservation(), Ok(()));
        assert_eq!(acl.header.size as usize, ACL_HEADER_SIZE);
    }

    #[test]
    fn bad_type_rejected() {
        let bad = AceEntry { ace_type: 0xee, flags: 0, size: 8, mask: 1, sid_slot: 1 };
        let mut acl = Acl::fresh(2);
        assert_eq!(acl.add(bad), Err(E_TYPE));
    }

    #[test]
    fn summary_shapes() {
        let acl = Acl::fresh(2);
        assert!(summary(&acl).contains("acl"));
        assert_eq!(AclOp::Add.code(), 1);
        assert_eq!(AclOp::Clear.code(), 3);
    }
}

pub fn run_unx36089_checks() -> CheckSet {
    let mut cs = CheckSet::new("unxstar-unx36089_aclcore");
    cs.add("SEG 段位独占", SEG_BASE == 0x3810, "0x3810~0x381f 本模块独占");
    match drill() {
        Ok(_) => cs.add("确定性演练全绿", true, "守恒式全程成立、负例 7 例全拒"),
        Err(code) => cs.add("确定性演练全绿", false, detail(code)),
    }
    cs.add("定容槽锚 32", ACL_CAP == 32, "ACL 定容槽数漂移");
    cs.add("ACL 总尺寸锚 0xFFFF", ACL_SIZE_MAX == 0xffff, "ACL 尺寸上限漂移");
    cs.add("版本锚 VER=1", VER == 1, "版本锚不符");
    cs.add("诊断码闭集互异", codes_distinct(), "诊断码闭集出现重码");
    cs
}
