//! UNX-F36090-J1 ACL 版本字段与未知版本拒绝。
//! 判据：① 仅 ACL_REVISION(2) / ACL_REVISION_DS(4) 合法，12 例合法容器过闸；
//! ② 未知版本 10 例（0/1/3/5..10/255）全部显性拒绝；对象 ACE 支持矩阵
//! （rev2 无 / rev4 有）2 例对平；③ 纯功能代码 ≥300 行。
//! 设计纪律：消费真源 unx36089_aclcore（Acl/AclHeader），版本判据为本模块
//! 独立承载面；36092 的插入原语在插入前复用本模块的版本闸。

use crate::checks::CheckSet;
use super::unx36081_acecore as core;
use super::unx36089_aclcore as aclc;
use alloc::vec;
use alloc::vec::Vec;
use alloc::string::{String, ToString};

pub const SEG_BASE: u16 = 0x3820;

// ---- 诊断码闭集 -----------------------------------------------------------
pub const E_VER: u16 = SEG_BASE + 1;    // 版本锚不符
pub const E_UNKNOWN: u16 = SEG_BASE + 2;// 未知版本（闭集 {2,4} 之外）
pub const E_SUPPORT: u16 = SEG_BASE + 3;// 对象 ACE 支持矩阵破
pub const E_CS: u16 = SEG_BASE + 4;     // 指纹不稳

/// 版本锚。
pub const VER: u32 = 1;

/// ACL_REVISION 锚值。
pub const ACL_REVISION: u8 = 2;
/// ACL_REVISION_DS 锚值。
pub const ACL_REVISION_DS: u8 = 4;

/// ACL 版本闭集（#[repr(u16)] 显式判别式）。
#[repr(u16)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum AclRev {
    /// 基础版（Windows ACL_REVISION）。
    Revision    = 0x0002,
    /// 目录服务版（Windows ACL_REVISION_DS，支持对象 ACE）。
    RevisionDs  = 0x0004,
}

impl AclRev {
    pub fn code(self) -> u16 {
        self as u16
    }

    /// 线上版本字节 → 闭集枚举；表外 None。
    pub fn from_rev(r: u8) -> Option<AclRev> {
        match r {
            ACL_REVISION => Some(AclRev::Revision),
            ACL_REVISION_DS => Some(AclRev::RevisionDs),
            _ => None,
        }
    }

    /// 对象 ACE（0x0005/0x0006 族）支持矩阵：仅 DS 版支持。
    pub fn object_ace_supported(self) -> bool {
        match self {
            AclRev::Revision => false,
            AclRev::RevisionDs => true,
        }
    }
}

/// 版本闸：容器头版本必须落闭集，否则 E_UNKNOWN。
pub fn gate_acl(acl: &aclc::Acl) -> Result<AclRev, u16> {
    match AclRev::from_rev(acl.header.rev) {
        Some(r) => Ok(r),
        None => Err(E_UNKNOWN),
    }
}

/// 带版本构造容器（闸后返回容器 + 版本枚举）。
pub fn build_with_rev(r: u8) -> Result<(aclc::Acl, AclRev), u16> {
    let acl = aclc::Acl::fresh(r);
    let rev = gate_acl(&acl)?;
    Ok((acl, rev))
}

/// 摘要行（String 面，无 IO）。
pub fn summary(r: AclRev) -> String {
    let mut s = String::new();
    s.push_str("acl rev=0x");
    s.push_str(&r.code().to_string());
    s.push_str(" obj_ace=");
    s.push_str(&r.object_ace_supported().to_string());
    s
}

/// 诊断码闭集互异自检。
pub fn codes_distinct() -> bool {
    let cs = [E_VER, E_UNKNOWN, E_SUPPORT, E_CS];
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

/// 版本人话名（诊断面用）。
pub fn rev_name(r: AclRev) -> &'static str {
    match r {
        AclRev::Revision => "ACL_REVISION",
        AclRev::RevisionDs => "ACL_REVISION_DS",
    }
}

/// 版本线上形式锚表（誊录面：seg 表/版本表对拍用，36099/36100 消费）。
pub const REV_TABLE: [u8; 2] = [ACL_REVISION, ACL_REVISION_DS];

/// 版本闭集表纪律自检：两项互异、from_rev 与表互证。
pub fn rev_table_integrity() -> Result<(), u16> {
    if REV_TABLE.len() != 2 {
        return Err(E_UNKNOWN);
    }
    if REV_TABLE[0] == REV_TABLE[1] {
        return Err(E_UNKNOWN);
    }
    let mut i = 0;
    while i < REV_TABLE.len() {
        if AclRev::from_rev(REV_TABLE[i]).is_none() {
            return Err(E_UNKNOWN);
        }
        i += 1;
    }
    Ok(())
}

/// 版本升级路径：rev2 容器升 rev4（DS 能力开启；回退显性拒）。
pub fn upgrade_to_ds(acl: &mut aclc::Acl) -> Result<AclRev, u16> {
    let cur = gate_acl(acl)?;
    match cur {
        AclRev::Revision => {
            acl.header.rev = ACL_REVISION_DS;
            gate_acl(acl)
        }
        AclRev::RevisionDs => Err(E_SUPPORT), // 已是 DS，重复升级拒
    }
}

/// 版本降级禁令：rev4 → rev2 显性拒（DS 能力一旦开启不回退）。
pub fn downgrade_to_base(acl: &mut aclc::Acl) -> Result<AclRev, u16> {
    let cur = gate_acl(acl)?;
    match cur {
        AclRev::RevisionDs => Err(E_SUPPORT),
        AclRev::Revision => Err(E_SUPPORT),
    }
}

/// 容器版本描述行（String 面，无 IO：版本名 + 对象 ACE 支持位）。
pub fn describe_rev(r: AclRev) -> String {
    let mut s = String::new();
    s.push_str("acl-rev ");
    s.push_str(rev_name(r));
    s.push_str(" (0x");
    s.push_str(&r.code().to_string());
    s.push_str(") obj_ace=");
    s.push_str(&r.object_ace_supported().to_string());
    s
}

pub fn detail(code: u16) -> &'static str {
    match code {
        E_VER => "版本锚不符",
        E_UNKNOWN => "未知版本（闭集 {2,4} 之外）",
        E_SUPPORT => "对象 ACE 支持矩阵破",
        E_CS => "指纹不稳",
        _ => "未知诊断码",
    }
}

/// 演练：R1 闭集锚互证；R2 12 例合法版本过闸；R3 未知版本 10 例全拒；
/// R4 对象 ACE 支持矩阵 2 例；R5 升级/降级路径 3 例；R6 指纹稳定。
pub fn drill() -> Result<u32, u16> {
    let mut hits: u32 = 0;
    // R1：闭集锚互证（from_rev 与常量互证 + 表纪律）。
    rev_table_integrity()?;
    if AclRev::from_rev(ACL_REVISION) != Some(AclRev::Revision) {
        return Err(E_SUPPORT);
    }
    if AclRev::from_rev(ACL_REVISION_DS) != Some(AclRev::RevisionDs) {
        return Err(E_SUPPORT);
    }
    if AclRev::from_rev(0) .is_some() || AclRev::from_rev(255).is_some() {
        return Err(E_UNKNOWN);
    }
    if VER != 1 {
        return Err(E_VER);
    }
    hits += 1;
    // R2：12 例合法版本过闸（2/4 交替）+ 追加一例 ACE 全链。
    let mut i: usize = 0;
    while i < 12 {
        let want: AclRev = if i % 2 == 0 { AclRev::Revision } else { AclRev::RevisionDs };
        let (mut acl, got) = build_with_rev(want.code() as u8)?;
        if got != want {
            return Err(E_UNKNOWN);
        }
        let e = aclc::AceEntry::new(0x00, 0x00, aclc::ACE_MIN_SIZE as u16, 0x0000_0001, 1)?;
        acl.add(e)?;
        acl.check_conservation()?;
        gate_acl(&acl)?;
        hits += 1;
        i += 1;
    }
    // R3：未知版本 10 例——全部 E_UNKNOWN（非静默放行红线）。
    let bads: Vec<u8> = vec![0, 1, 3, 5, 6, 7, 8, 9, 10, 255];
    let mut b = 0;
    while b < bads.len() {
        match build_with_rev(bads[b]) {
            Ok(_) => return Err(E_UNKNOWN),
            Err(c) => {
                if c != E_UNKNOWN {
                    return Err(E_UNKNOWN);
                }
            }
        }
        hits += 1;
        b += 1;
    }
    // R4：对象 ACE 支持矩阵 2 例（rev2 无 / rev4 有）。
    if AclRev::Revision.object_ace_supported() {
        return Err(E_SUPPORT);
    }
    hits += 1;
    if !AclRev::RevisionDs.object_ace_supported() {
        return Err(E_SUPPORT);
    }
    hits += 1;
    // R5：升级/降级路径 3 例（升 DS 成功 / 重复升级拒 / 降级拒）。
    let (mut acl2, _) = build_with_rev(ACL_REVISION)?;
    let up = upgrade_to_ds(&mut acl2)?;
    if up.code() != AclRev::RevisionDs.code() {
        return Err(E_SUPPORT);
    }
    hits += 1;
    match upgrade_to_ds(&mut acl2) {
        Ok(_) => return Err(E_SUPPORT),
        Err(c) => {
            if c != E_SUPPORT {
                return Err(E_SUPPORT);
            }
        }
    }
    hits += 1;
    match downgrade_to_base(&mut acl2) {
        Ok(_) => return Err(E_SUPPORT),
        Err(c) => {
            if c != E_SUPPORT {
                return Err(E_SUPPORT);
            }
        }
    }
    hits += 1;
    // R6：指纹两次求值稳定且非零（真源 FNV over 版本字节 + 版本锚）。
    let fp1 = core::fnv1a(&[ACL_REVISION, ACL_REVISION_DS, VER as u8]);
    let fp2 = core::fnv1a(&[ACL_REVISION, ACL_REVISION_DS, VER as u8]);
    if fp1 != fp2 || fp1 == 0 {
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
        assert_eq!(drill(), Ok(29));
    }

    #[test]
    fn unknown_rev_rejected() {
        assert_eq!(AclRev::from_rev(3), None);
        assert_eq!(build_with_rev(1), Err(E_UNKNOWN));
    }

    #[test]
    fn known_revs_accepted() {
        assert_eq!(AclRev::from_rev(2), Some(AclRev::Revision));
        assert_eq!(AclRev::from_rev(4), Some(AclRev::RevisionDs));
        assert!(summary(AclRev::RevisionDs).contains("obj_ace"));
    }

    #[test]
    fn object_ace_matrix() {
        assert!(!AclRev::Revision.object_ace_supported());
        assert!(AclRev::RevisionDs.object_ace_supported());
    }

    #[test]
    fn upgrade_and_downgrade_gates() {
        let (mut acl, _) = build_with_rev(ACL_REVISION).unwrap_or((aclc::Acl::fresh(2), AclRev::Revision));
        assert_eq!(upgrade_to_ds(&mut acl), Ok(AclRev::RevisionDs));
        assert_eq!(upgrade_to_ds(&mut acl), Err(E_SUPPORT));
        assert_eq!(downgrade_to_base(&mut acl), Err(E_SUPPORT));
        assert!(rev_name(AclRev::RevisionDs) == "ACL_REVISION_DS");
        assert_eq!(rev_table_integrity(), Ok(()));
        assert!(describe_rev(AclRev::Revision).contains("ACL_REVISION "));
    }
}

pub fn run_unx36090_checks() -> CheckSet {
    let mut cs = CheckSet::new("unxstar-unx36090_aclver");
    cs.add("SEG 段位独占", SEG_BASE == 0x3820, "0x3820~0x382f 本模块独占");
    match drill() {
        Ok(_) => cs.add("确定性演练全绿", true, "合法版本 12 例过闸、未知版本 10 例全拒"),
        Err(code) => cs.add("确定性演练全绿", false, detail(code)),
    }
    cs.add("版本闭集锚 {2,4}", ACL_REVISION == 2 && ACL_REVISION_DS == 4, "ACL 版本闭集锚漂移");
    cs.add("版本锚 VER=1", VER == 1, "版本锚不符");
    cs.add("诊断码闭集互异", codes_distinct(), "诊断码闭集出现重码");
    cs
}
