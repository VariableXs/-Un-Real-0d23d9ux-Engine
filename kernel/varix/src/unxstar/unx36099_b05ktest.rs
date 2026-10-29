//! UNX-F36099-J1 B05 ktest 断言集（ACE 面全判据注册）。
//! 判据：① 聚合面——36081~36098 十八个模块的 drill() 逐个 match 收敛成
//! 聚合判据（全绿才绿）；② 批内守恒计数（模块数=20、SEG 基址表对拍、
//! 判据面 18+2 齐全）；③ 纯功能代码 ≥300 行。
//! 设计纪律：本模块是 B05 批聚合面——只消费批内已存在模块的 drill()，
//! 不定义第二套 ACE 语义；36100 收口面与本模块分工（聚合 vs 守恒预核）。

use crate::checks::CheckSet;
use super::unx36081_acecore;
use super::unx36082_aceallow;
use super::unx36083_acedeny;
use super::unx36084_aceaudit;
use super::unx36085_aceflags;
use super::unx36086_aceprotect;
use super::unx36087_acemaskbits;
use super::unx36088_acecanon;
use super::unx36089_aclcore;
use super::unx36090_aclver;
use super::unx36091_aclcap;
use super::unx36092_aclprim;
use super::unx36093_acesidshare;
use super::unx36094_aceevtfmt;
use super::unx36095_acediag;
use super::unx36096_acefuzz;
use super::unx36097_aceperf;
use super::unx36098_acegolden;
use alloc::vec;
use alloc::vec::Vec;
use alloc::string::{String, ToString};

pub const SEG_BASE: u16 = 0x38b0;

// ---- 诊断码闭集 -----------------------------------------------------------
pub const E_VER: u16 = SEG_BASE + 1;  // 版本锚不符
pub const E_MODN: u16 = SEG_BASE + 2; // 模块守恒计数破
pub const E_SEG: u16 = SEG_BASE + 3;  // SEG 基址表对拍破
pub const E_DRILL: u16 = SEG_BASE + 4;// 成员 drill 出红
pub const E_FACE: u16 = SEG_BASE + 5; // 判据面数不全
pub const E_CS: u16 = SEG_BASE + 6;   // 指纹不稳

/// 版本锚。
pub const VER: u32 = 1;

/// 批内模块总数锚（18 面模块 + ktest + close = 20）。
pub const BATCH_MOD_N: usize = 20;
/// 被 drill 聚合的面模块数（36081..36098）。
pub const DRILL_FACE_N: usize = 18;

/// 批内角色段（#[repr(u16)] 显式判别式）。
#[repr(u16)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BatchZone {
    /// 面模块段（36081..36098，drill 聚合对象）。
    Faces = 0x0001,
    /// ktest 聚合段（本模块）。
    Ktest = 0x0002,
    /// 收口段（36100）。
    Close = 0x0003,
}

impl BatchZone {
    pub fn code(self) -> u16 {
        self as u16
    }
}

/// 批内模块描述（id / seg 基址 / slug）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ModuleDesc {
    pub id: u32,
    pub seg: u16,
    pub slug: &'static str,
}

/// B05 批模块表（卡号连续 36081..36100，SEG 基址按 wbj1028_segs.txt）。
pub const MODULE_TABLE: [ModuleDesc; BATCH_MOD_N] = [
    ModuleDesc { id: 36081, seg: 0xe2d0, slug: "acecore" },
    ModuleDesc { id: 36082, seg: 0xe2e0, slug: "aceallow" },
    ModuleDesc { id: 36083, seg: 0xe2f0, slug: "acedeny" },
    ModuleDesc { id: 36084, seg: 0xe300, slug: "aceaudit" },
    ModuleDesc { id: 36085, seg: 0xe310, slug: "aceflags" },
    ModuleDesc { id: 36086, seg: 0xe320, slug: "aceprotect" },
    ModuleDesc { id: 36087, seg: 0xe330, slug: "acemaskbits" },
    ModuleDesc { id: 36088, seg: 0x3800, slug: "acecanon" },
    ModuleDesc { id: 36089, seg: 0x3810, slug: "aclcore" },
    ModuleDesc { id: 36090, seg: 0x3820, slug: "aclver" },
    ModuleDesc { id: 36091, seg: 0x3830, slug: "aclcap" },
    ModuleDesc { id: 36092, seg: 0x3840, slug: "aclprim" },
    ModuleDesc { id: 36093, seg: 0x3850, slug: "acesidshare" },
    ModuleDesc { id: 36094, seg: 0x3860, slug: "aceevtfmt" },
    ModuleDesc { id: 36095, seg: 0x3870, slug: "acediag" },
    ModuleDesc { id: 36096, seg: 0x3880, slug: "acefuzz" },
    ModuleDesc { id: 36097, seg: 0x3890, slug: "aceperf" },
    ModuleDesc { id: 36098, seg: 0x38a0, slug: "acegolden" },
    ModuleDesc { id: 36099, seg: 0x38b0, slug: "b05ktest" },
    ModuleDesc { id: 36100, seg: 0x3900, slug: "b05close" },
];

/// 模块表纪律自检：20 项、卡号连续、slug 非空。
pub fn table_integrity() -> Result<(), u16> {
    if MODULE_TABLE.len() != BATCH_MOD_N || BATCH_MOD_N != 20 {
        return Err(E_MODN);
    }
    let mut i = 0;
    while i < BATCH_MOD_N {
        if MODULE_TABLE[i].slug.is_empty() {
            return Err(E_MODN);
        }
        if MODULE_TABLE[i].id != 36081 + i as u32 {
            return Err(E_MODN);
        }
        i += 1;
    }
    Ok(())
}

/// 面模块位次清单（Vec 面：ktest 分组遍历用）。
pub fn face_indices() -> Vec<usize> {
    (0..DRILL_FACE_N).collect()
}

/// SEG 基址表对拍：本模块/收口模块基址 + 面模块实际常量逐项一致。
pub fn seg_table_match() -> Result<u32, u16> {
    if unx36081_acecore::SEG_BASE != MODULE_TABLE[0].seg { return Err(E_SEG); }
    if unx36082_aceallow::SEG_BASE != MODULE_TABLE[1].seg { return Err(E_SEG); }
    if unx36083_acedeny::SEG_BASE != MODULE_TABLE[2].seg { return Err(E_SEG); }
    if unx36084_aceaudit::SEG_BASE != MODULE_TABLE[3].seg { return Err(E_SEG); }
    if unx36085_aceflags::SEG_BASE != MODULE_TABLE[4].seg { return Err(E_SEG); }
    if unx36086_aceprotect::SEG_BASE != MODULE_TABLE[5].seg { return Err(E_SEG); }
    if unx36087_acemaskbits::SEG_BASE != MODULE_TABLE[6].seg { return Err(E_SEG); }
    if unx36088_acecanon::SEG_BASE != MODULE_TABLE[7].seg { return Err(E_SEG); }
    if unx36089_aclcore::SEG_BASE != MODULE_TABLE[8].seg { return Err(E_SEG); }
    if unx36090_aclver::SEG_BASE != MODULE_TABLE[9].seg { return Err(E_SEG); }
    if unx36091_aclcap::SEG_BASE != MODULE_TABLE[10].seg { return Err(E_SEG); }
    if unx36092_aclprim::SEG_BASE != MODULE_TABLE[11].seg { return Err(E_SEG); }
    if unx36093_acesidshare::SEG_BASE != MODULE_TABLE[12].seg { return Err(E_SEG); }
    if unx36094_aceevtfmt::SEG_BASE != MODULE_TABLE[13].seg { return Err(E_SEG); }
    if unx36095_acediag::SEG_BASE != MODULE_TABLE[14].seg { return Err(E_SEG); }
    if unx36096_acefuzz::SEG_BASE != MODULE_TABLE[15].seg { return Err(E_SEG); }
    if unx36097_aceperf::SEG_BASE != MODULE_TABLE[16].seg { return Err(E_SEG); }
    if unx36098_acegolden::SEG_BASE != MODULE_TABLE[17].seg { return Err(E_SEG); }
    if SEG_BASE != MODULE_TABLE[18].seg { return Err(E_SEG); }
    Ok(1)
}

/// 判据面守恒：DRILL_FACE_N + 2（ktest/close）== BATCH_MOD_N。
pub fn face_conservation() -> bool {
    DRILL_FACE_N + 2 == BATCH_MOD_N
}

/// 摘要行（String 面，无 IO）。
pub fn summary(green_n: u32) -> String {
    let mut s = String::new();
    s.push_str("b05ktest green=");
    s.push_str(&green_n.to_string());
    s.push_str("/");
    s.push_str(&DRILL_FACE_N.to_string());
    s
}

/// 诊断码闭集互异自检。
pub fn codes_distinct() -> bool {
    let cs = [E_VER, E_MODN, E_SEG, E_DRILL, E_FACE, E_CS];
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
        E_MODN => "模块守恒计数破",
        E_SEG => "SEG 基址表对拍破",
        E_DRILL => "成员 drill 出红",
        E_FACE => "判据面数不全",
        E_CS => "指纹不稳",
        _ => "未知诊断码",
    }
}

/// 演练：R1 模块表纪律；R2 SEG 基址表对拍；R3 十八面 drill 全绿聚合；
/// R4 判据面守恒；R5 指纹稳定。
pub fn drill() -> Result<u32, u16> {
    let mut hits: u32 = 0;
    // R1：模块表纪律。
    table_integrity()?;
    if VER != 1 {
        return Err(E_VER);
    }
    hits += 1;
    // R2：SEG 基址表对拍（含 18 面模块实际常量）。
    if face_indices().len() != DRILL_FACE_N {
        return Err(E_FACE);
    }
    seg_table_match()?;
    hits += 1;
    // R3：十八面 drill 逐个 match 收敛（全绿才绿，一面出红即 E_DRILL）。
    match unx36081_acecore::drill() {
        Ok(_) => hits += 1,
        Err(_) => return Err(E_DRILL),
    }
    match unx36082_aceallow::drill() {
        Ok(_) => hits += 1,
        Err(_) => return Err(E_DRILL),
    }
    match unx36083_acedeny::drill() {
        Ok(_) => hits += 1,
        Err(_) => return Err(E_DRILL),
    }
    match unx36084_aceaudit::drill() {
        Ok(_) => hits += 1,
        Err(_) => return Err(E_DRILL),
    }
    match unx36085_aceflags::drill() {
        Ok(_) => hits += 1,
        Err(_) => return Err(E_DRILL),
    }
    match unx36086_aceprotect::drill() {
        Ok(_) => hits += 1,
        Err(_) => return Err(E_DRILL),
    }
    match unx36087_acemaskbits::drill() {
        Ok(_) => hits += 1,
        Err(_) => return Err(E_DRILL),
    }
    match unx36088_acecanon::drill() {
        Ok(_) => hits += 1,
        Err(_) => return Err(E_DRILL),
    }
    match unx36089_aclcore::drill() {
        Ok(_) => hits += 1,
        Err(_) => return Err(E_DRILL),
    }
    match unx36090_aclver::drill() {
        Ok(_) => hits += 1,
        Err(_) => return Err(E_DRILL),
    }
    match unx36091_aclcap::drill() {
        Ok(_) => hits += 1,
        Err(_) => return Err(E_DRILL),
    }
    match unx36092_aclprim::drill() {
        Ok(_) => hits += 1,
        Err(_) => return Err(E_DRILL),
    }
    match unx36093_acesidshare::drill() {
        Ok(_) => hits += 1,
        Err(_) => return Err(E_DRILL),
    }
    match unx36094_aceevtfmt::drill() {
        Ok(_) => hits += 1,
        Err(_) => return Err(E_DRILL),
    }
    match unx36095_acediag::drill() {
        Ok(_) => hits += 1,
        Err(_) => return Err(E_DRILL),
    }
    match unx36096_acefuzz::drill() {
        Ok(_) => hits += 1,
        Err(_) => return Err(E_DRILL),
    }
    match unx36097_aceperf::drill() {
        Ok(_) => hits += 1,
        Err(_) => return Err(E_DRILL),
    }
    match unx36098_acegolden::drill() {
        Ok(_) => hits += 1,
        Err(_) => return Err(E_DRILL),
    }
    // R4：判据面守恒（18 面 + ktest/close = 20）。
    if !face_conservation() {
        return Err(E_FACE);
    }
    hits += 1;
    // R5：指纹两次求值稳定且非零（FNV over 表基址字节 + 版本锚）。
    let mut feed = vec![];
    feed.extend_from_slice(&VER.to_le_bytes());
    let mut i = 0;
    while i < BATCH_MOD_N {
        feed.extend_from_slice(&MODULE_TABLE[i].seg.to_le_bytes());
        i += 1;
    }
    let fp1 = unx36081_acecore::fnv1a(&feed);
    let fp2 = unx36081_acecore::fnv1a(&feed);
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
        assert_eq!(drill(), Ok(22));
    }

    #[test]
    fn table_holds() {
        assert_eq!(table_integrity(), Ok(()));
        assert_eq!(MODULE_TABLE.len(), 20);
        assert!(summary(18).contains("b05ktest"));
    }

    #[test]
    fn seg_table_holds() {
        assert_eq!(seg_table_match(), Ok(1));
    }

    #[test]
    fn faces_conserved() {
        assert!(face_conservation());
        assert_eq!(DRILL_FACE_N, 18);
    }
}

pub fn run_unx36099_checks() -> CheckSet {
    let mut cs = CheckSet::new("unxstar-unx36099_b05ktest");
    cs.add("SEG 段位独占", SEG_BASE == 0x38b0, "0x38b0~0x38bf 本模块独占");
    match drill() {
        Ok(_) => cs.add("B05 聚合演练全绿", true, "十八面 drill 全绿收敛、守恒计数齐"),
        Err(code) => cs.add("B05 聚合演练全绿", false, detail(code)),
    }
    cs.add("模块守恒计数 20", BATCH_MOD_N == 20, "批内模块数锚漂移");
    cs.add("SEG 基址表对拍", seg_table_match().is_ok(), "SEG 基址表与实际常量不符");
    cs.add("版本锚 VER=1", VER == 1, "版本锚不符");
    cs.add("诊断码闭集互异", codes_distinct(), "诊断码闭集出现重码");
    cs
}
