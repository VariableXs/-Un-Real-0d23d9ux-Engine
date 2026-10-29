//! UNX-F36100-J1 B05 守恒预核、防重扫描与收口。
//! 判据：① 全批 SEG 窗口独占对拍（20 基址两两距离 ≥13 → 诊断码闭集不撞窗）
//! 与版本锚一致性（20 模块 VER 全 1）；② 防重扫描（同指纹模块重复检出 = 0）；
//! ③ 纯功能代码 ≥300 行。
//! 设计纪律：B05 收口面——与 36099 聚合面分工；消费批内 18 个面模块的
//! SEG_BASE/VER 常量与真源 FNV，不重复定义任何 ACE 语义。

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
use super::unx36099_b05ktest;
use alloc::vec;
use alloc::vec::Vec;
use alloc::string::{String, ToString};

pub const SEG_BASE: u16 = 0x3900;

// ---- 诊断码闭集 -----------------------------------------------------------
pub const E_VER: u16 = SEG_BASE + 1;     // 版本锚不符
pub const E_SEGW: u16 = SEG_BASE + 2;    // SEG 窗口对拍破（撞窗/表不符）
pub const E_ANCHOR: u16 = SEG_BASE + 3;  // 版本锚不一致
pub const E_DUP: u16 = SEG_BASE + 4;     // 同指纹模块重复
pub const E_RANGE: u16 = SEG_BASE + 5;   // 诊断码窗口距离不足
pub const E_CS: u16 = SEG_BASE + 6;      // 指纹不稳

/// 版本锚。
pub const VER: u32 = 1;

/// 批内模块总数锚。
pub const BATCH_MOD_N: usize = 20;
/// 诊断码窗口最小距离（每窗 ≤12 码：SEG_BASE+1..+12，距离 <13 即可能撞码）。
pub const MIN_SEG_GAP: u16 = 13;

/// 收口步骤段（#[repr(u16)] 显式判别式）。
#[repr(u16)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CloseStep {
    /// 步 1：SEG 窗口独占对拍。
    SegWindow = 0x0001,
    /// 步 2：版本锚一致性。
    VerAnchor = 0x0002,
    /// 步 3：防重扫描。
    DedupScan = 0x0003,
    /// 步 4：窗口距离锚。
    GapAnchor = 0x0004,
}

impl CloseStep {
    pub fn code(self) -> u16 {
        self as u16
    }
}

/// 全批 SEG 基址表（按 wbj1028_segs.txt 逐行誊录）。
pub const SEG_TABLE: [u16; BATCH_MOD_N] = [
    0xe2d0, 0xe2e0, 0xe2f0, 0xe300, 0xe310, 0xe320, 0xe330,
    0x3800, 0x3810, 0x3820, 0x3830, 0x3840, 0x3850, 0x3860,
    0x3870, 0x3880, 0x3890, 0x38a0, 0x38b0, 0x3900,
];

/// 全批版本锚表（20 模块 VER，B05 恒 1）。
pub const VER_TABLE: [u32; BATCH_MOD_N] = [1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1, 1];

/// 全批位次清单（Vec 面：防重扫描分组遍历用）。
fn all_indices() -> Vec<usize> {
    (0..BATCH_MOD_N).collect()
}

/// 面模块实际 SEG_BASE 常量回读（防表誊错）。
pub fn actual_seg_bases() -> [u16; BATCH_MOD_N] {
    [
        unx36081_acecore::SEG_BASE,
        unx36082_aceallow::SEG_BASE,
        unx36083_acedeny::SEG_BASE,
        unx36084_aceaudit::SEG_BASE,
        unx36085_aceflags::SEG_BASE,
        unx36086_aceprotect::SEG_BASE,
        unx36087_acemaskbits::SEG_BASE,
        unx36088_acecanon::SEG_BASE,
        unx36089_aclcore::SEG_BASE,
        unx36090_aclver::SEG_BASE,
        unx36091_aclcap::SEG_BASE,
        unx36092_aclprim::SEG_BASE,
        unx36093_acesidshare::SEG_BASE,
        unx36094_aceevtfmt::SEG_BASE,
        unx36095_acediag::SEG_BASE,
        unx36096_acefuzz::SEG_BASE,
        unx36097_aceperf::SEG_BASE,
        unx36098_acegolden::SEG_BASE,
        unx36099_b05ktest::SEG_BASE,
        SEG_BASE,
    ]
}

/// 面模块实际 VER 常量回读。
pub fn actual_vers() -> [u32; BATCH_MOD_N] {
    [
        unx36081_acecore::VER,
        unx36082_aceallow::VER,
        unx36083_acedeny::VER,
        unx36084_aceaudit::VER,
        unx36085_aceflags::VER,
        unx36086_aceprotect::VER,
        unx36087_acemaskbits::VER,
        unx36088_acecanon::VER,
        unx36089_aclcore::VER,
        unx36090_aclver::VER,
        unx36091_aclcap::VER,
        unx36092_aclprim::VER,
        unx36093_acesidshare::VER,
        unx36094_aceevtfmt::VER,
        unx36095_acediag::VER,
        unx36096_acefuzz::VER,
        unx36097_aceperf::VER,
        unx36098_acegolden::VER,
        unx36099_b05ktest::VER,
        VER,
    ]
}

/// SEG 窗口对拍：表 == 实际常量；且两两距离 ≥ MIN_SEG_GAP（诊断码闭集不撞窗）。
pub fn seg_window_audit() -> Result<u32, u16> {
    let actual = actual_seg_bases();
    let mut i = 0;
    while i < BATCH_MOD_N {
        if actual[i] != SEG_TABLE[i] {
            return Err(E_SEGW);
        }
        i += 1;
    }
    let mut hits: u32 = 0;
    let mut a = 0;
    while a < BATCH_MOD_N {
        let mut b = a + 1;
        while b < BATCH_MOD_N {
            let d = if SEG_TABLE[a] > SEG_TABLE[b] {
                SEG_TABLE[a] - SEG_TABLE[b]
            } else {
                SEG_TABLE[b] - SEG_TABLE[a]
            };
            if d < MIN_SEG_GAP {
                return Err(E_RANGE);
            }
            b += 1;
        }
        hits += 1;
        a += 1;
    }
    Ok(hits)
}

/// 版本锚一致性：表 == 实际常量，且全 1。
pub fn ver_anchor_audit() -> Result<u32, u16> {
    let actual = actual_vers();
    let mut i = 0;
    while i < BATCH_MOD_N {
        if actual[i] != VER_TABLE[i] {
            return Err(E_ANCHOR);
        }
        if actual[i] != 1 {
            return Err(E_ANCHOR);
        }
        i += 1;
    }
    Ok(BATCH_MOD_N as u32)
}

/// 模块指纹（真源 FNV over (seg, ver)）。
pub fn module_fingerprint(seg: u16, ver: u32) -> u64 {
    let mut feed = vec![];
    feed.extend_from_slice(&seg.to_le_bytes());
    feed.extend_from_slice(&ver.to_le_bytes());
    unx36081_acecore::fnv1a(&feed)
}

/// 防重扫描：全批指纹两两比对，重复检出数必为 0。
pub fn duplicate_scan() -> Result<u32, u16> {
    let segs = actual_seg_bases();
    let vers = actual_vers();
    let mut dup_n: u32 = 0;
    let idxs = all_indices();
    if idxs.len() != BATCH_MOD_N {
        return Err(E_DUP);
    }
    let mut a = 0;
    while a < BATCH_MOD_N {
        let mut b = a + 1;
        while b < BATCH_MOD_N {
            if module_fingerprint(segs[a], vers[a]) == module_fingerprint(segs[b], vers[b]) {
                dup_n += 1;
            }
            b += 1;
        }
        a += 1;
    }
    if dup_n != 0 {
        return Err(E_DUP);
    }
    Ok(dup_n)
}

/// 全批最小窗口距离计算（走查面：返回两两最小距离）。
pub fn min_seg_gap() -> u16 {
    let mut min: u16 = 0xffff;
    let mut a = 0;
    while a < BATCH_MOD_N {
        let mut b = a + 1;
        while b < BATCH_MOD_N {
            let d = if SEG_TABLE[a] > SEG_TABLE[b] {
                SEG_TABLE[a] - SEG_TABLE[b]
            } else {
                SEG_TABLE[b] - SEG_TABLE[a]
            };
            if d < min {
                min = d;
            }
            b += 1;
        }
        a += 1;
    }
    min
}

/// 批级联合指纹：20 模块 (seg, ver) 指纹逐个 XOR 折叠（对拍面：重跑一致）。
pub fn batch_fingerprint() -> u64 {
    let segs = actual_seg_bases();
    let vers = actual_vers();
    let mut acc: u64 = 0;
    let mut i = 0;
    while i < BATCH_MOD_N {
        acc ^= module_fingerprint(segs[i], vers[i]);
        i += 1;
    }
    acc
}

/// 摘要行（String 面，无 IO）。
pub fn summary() -> String {
    let mut s = String::new();
    s.push_str("b05close mods=");
    s.push_str(&BATCH_MOD_N.to_string());
    s.push_str(" gap=");
    s.push_str(&MIN_SEG_GAP.to_string());
    s
}

/// 诊断码闭集互异自检。
pub fn codes_distinct() -> bool {
    let cs = [E_VER, E_SEGW, E_ANCHOR, E_DUP, E_RANGE, E_CS];
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
        E_SEGW => "SEG 窗口对拍破（表不符）",
        E_ANCHOR => "版本锚不一致",
        E_DUP => "同指纹模块重复",
        E_RANGE => "诊断码窗口距离不足",
        E_CS => "指纹不稳",
        _ => "未知诊断码",
    }
}

/// 演练：R1 SEG 窗口对拍（表/实际/两两距离 20 行走查）；R2 版本锚一致性
/// （20 项全 1）；R3 防重扫描（重复检出 = 0）；R4 最小窗口距离锚恰 16；
/// R5 批级联合指纹两次求值稳定。
pub fn drill() -> Result<u32, u16> {
    let mut hits: u32 = 0;
    // R1：SEG 窗口对拍（20 行走查 + 190 对距离检查）。
    if VER != 1 {
        return Err(E_VER);
    }
    let w = seg_window_audit()?;
    if w != BATCH_MOD_N as u32 {
        return Err(E_SEGW);
    }
    hits += w;
    // R2：版本锚一致性（20 项）。
    let v = ver_anchor_audit()?;
    if v != BATCH_MOD_N as u32 {
        return Err(E_ANCHOR);
    }
    hits += v;
    // R3：防重扫描（重复检出 = 0）。
    let d = duplicate_scan()?;
    if d != 0 {
        return Err(E_DUP);
    }
    hits += 1;
    // R4：最小窗口距离锚恰 16（本批紧凑段全 0x10 间距）。
    if min_seg_gap() != 16 {
        return Err(E_RANGE);
    }
    hits += 1;
    // R5：批级联合指纹两次求值稳定且非零。
    let fp1 = batch_fingerprint();
    let fp2 = batch_fingerprint();
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
        assert_eq!(drill(), Ok(43));
    }

    #[test]
    fn min_gap_and_batch_fp() {
        assert_eq!(min_seg_gap(), 16);
        let a = batch_fingerprint();
        let b = batch_fingerprint();
        assert_eq!(a, b);
        assert!(a != 0);
        assert!(summary().contains("b05close"));
    }

    #[test]
    fn seg_windows_exclusive() {
        assert_eq!(seg_window_audit(), Ok(20));
        assert_eq!(SEG_TABLE[0], 0xe2d0);
        assert_eq!(SEG_TABLE[19], 0x3900);
    }

    #[test]
    fn vers_all_one() {
        assert_eq!(ver_anchor_audit(), Ok(20));
        assert!(summary().contains("b05close"));
    }

    #[test]
    fn no_duplicates() {
        assert_eq!(duplicate_scan(), Ok(0));
    }
}

pub fn run_unx36100_checks() -> CheckSet {
    let mut cs = CheckSet::new("unxstar-unx36100_b05close");
    cs.add("SEG 段位独占", SEG_BASE == 0x3900, "0x3900~0x390f 本模块独占");
    match drill() {
        Ok(_) => cs.add("B05 收口演练全绿", true, "窗口独占/版本锚一致/防重全过"),
        Err(code) => cs.add("B05 收口演练全绿", false, detail(code)),
    }
    cs.add("批内模块数 20", BATCH_MOD_N == 20, "批内模块数锚漂移");
    cs.add("窗口距离锚 13", MIN_SEG_GAP == 13, "诊断码窗口距离锚漂移");
    cs.add("版本锚 VER=1", VER == 1, "版本锚不符");
    cs.add("诊断码闭集互异", codes_distinct(), "诊断码闭集出现重码");
    cs
}
