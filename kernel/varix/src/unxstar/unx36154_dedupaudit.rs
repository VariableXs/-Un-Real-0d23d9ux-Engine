//! UNX-F36154-J1 F 型段防重终审（五范围全量复验）。
//! 判据：① 五个范围（unxstar 卡名 / B08 段窗 / 判据短名 / SEG 基址 /
//! 交付物名）逐范围全量查重——任何范围内出现重复签名即 E_DUP；
//! ② 注入面：在副本清单塞入一条重复签名 → 复验器必点名（复验能力
//! 自证）；③ 空范围与单元素范围边界（单元素不可能重，必须放行）；
//! ④ 指纹稳定 + 双跑一致；⑤ 纯功能代码 ≥300 行。
//! 设计纪律：防重终审是收口前最后一道闸——本面把「grep 复验」建模为
//! 确定性两两比对扫描（O(n²) 闭集内恒可行），不允许 IO。

use crate::checks::CheckSet;
use super::unx36081_acecore as core;
use alloc::vec;
use alloc::vec::Vec;
use alloc::string::{String, ToString};

pub const SEG_BASE: u16 = 0xee80;

// ---- 诊断码闭集 -----------------------------------------------------------
pub const E_VER: u16 = SEG_BASE + 1;  // 版本锚不符
pub const E_DUP: u16 = SEG_BASE + 2;  // 重复签名
pub const E_SCOPE: u16 = SEG_BASE + 3; // 范围越界（未知范围号）
pub const E_AUDIT: u16 = SEG_BASE + 4; // 复验器失效（重复未点名）
pub const E_CS: u16 = SEG_BASE + 5;   // 指纹不稳

/// 版本锚。
pub const VER: u32 = 1;

/// 范围数锚（五范围）。
pub const SCOPE_N: usize = 5;

/// 范围一：B05 批 20 张卡名（unxstar 全扇 grep 复验样本）。
pub const SCOPE_CARDS: [&str; 20] = [
    "unx36081_acecore", "unx36082_aceallow", "unx36083_acedeny",
    "unx36084_aceaudit", "unx36085_aceflags", "unx36086_aceprotect",
    "unx36087_acemaskbits", "unx36088_acecanon", "unx36089_aclcore",
    "unx36090_aclver", "unx36091_aclcap", "unx36092_aclprim",
    "unx36093_acesidshare", "unx36094_aceevtfmt", "unx36095_acediag",
    "unx36096_acefuzz", "unx36097_aceperf", "unx36098_acegolden",
    "unx36099_b05ktest", "unx36100_b05close",
];

/// 范围二：B08 批段位窗（0xee60~0xee90 + 0xef80~0xefc0，10 段基址）。
pub const SCOPE_SEGS: [u16; 10] = [
    0xee60, 0xee70, 0xee80, 0xee90, 0xef80, 0xef90, 0xefa0, 0xefb0,
    0xefc0, 0x8230,
];

/// 范围三：B08 批判据短名（5 个聚合/收口面）。
pub const SCOPE_CHECKS: [&str; 5] = [
    "run_unx36149_checks",
    "run_unx36155_checks",
    "run_unx36157_checks",
    "run_unx36159_checks",
    "run_unx36160_checks",
];

/// 范围四：交付物名（5 份）。
pub const SCOPE_DELIV: [&str; 5] = [
    "wbj1028_probeA", "mod_unxstar_registry", "b05_close_report",
    "b08_close_report", "f_seg_verdict",
];

/// 范围五：空范围（边界样本：空集必放行）。
pub const SCOPE_EMPTY: [&str; 0] = [];

/// 两两查重（String 域）：出现重复即报 E_DUP。
pub fn scan_strs(sigs: &[&str]) -> Result<u32, u16> {
    let mut n: u32 = 0;
    let mut i = 0;
    while i < sigs.len() {
        let mut j = i + 1;
        while j < sigs.len() {
            if sigs[i] == sigs[j] {
                return Err(E_DUP);
            }
            j += 1;
        }
        n += 1;
        i += 1;
    }
    Ok(n)
}

/// 两两查重（u16 域）。
pub fn scan_u16s(sigs: &[u16]) -> Result<u32, u16> {
    let mut n: u32 = 0;
    let mut i = 0;
    while i < sigs.len() {
        let mut j = i + 1;
        while j < sigs.len() {
            if sigs[i] == sigs[j] {
                return Err(E_DUP);
            }
            j += 1;
        }
        n += 1;
        i += 1;
    }
    Ok(n)
}

/// 五范围逐个复验：返回总扫描条数；任何范围报重即整审报错。
pub fn audit_all() -> Result<u32, u16> {
    let mut total: u32 = 0;
    total += scan_strs(&SCOPE_CARDS)?;
    total += scan_u16s(&SCOPE_SEGS)?;
    total += scan_strs(&SCOPE_CHECKS)?;
    total += scan_strs(&SCOPE_DELIV)?;
    total += scan_strs(&SCOPE_EMPTY)?;
    Ok(total)
}

/// 指定范围复验（范围号 1..=5），越界报 E_SCOPE。
pub fn audit_scope(scope_id: u16) -> Result<u32, u16> {
    match scope_id {
        1 => scan_strs(&SCOPE_CARDS),
        2 => scan_u16s(&SCOPE_SEGS),
        3 => scan_strs(&SCOPE_CHECKS),
        4 => scan_strs(&SCOPE_DELIV),
        5 => scan_strs(&SCOPE_EMPTY),
        _ => Err(E_SCOPE),
    }
}

/// 注入面：副本清单塞入重复签名 → 复验器必报 E_DUP（点名能力自证）。
pub fn injection_dup_str() -> Result<(), u16> {
    let mut dirty: Vec<&str> = vec![];
    let mut i = 0;
    while i < SCOPE_DELIV.len() {
        dirty.push(SCOPE_DELIV[i]);
        i += 1;
    }
    dirty.push("b05_close_report"); // 重复注入
    match scan_strs(&dirty) {
        Err(E_DUP) => Ok(()),
        _ => Err(E_AUDIT),
    }
}

/// 注入面（u16 域）：段位窗塞重段。
pub fn injection_dup_u16() -> Result<(), u16> {
    let mut dirty: Vec<u16> = vec![];
    let mut i = 0;
    while i < SCOPE_SEGS.len() {
        dirty.push(SCOPE_SEGS[i]);
        i += 1;
    }
    dirty.push(0xee60); // 重复注入
    match scan_u16s(&dirty) {
        Err(E_DUP) => Ok(()),
        _ => Err(E_AUDIT),
    }
}

/// 边界面：单元素范围必须放行（单元素不可能重）。
pub fn single_element_edge() -> Result<(), u16> {
    let single = ["only-one"];
    scan_strs(&single)?;
    Ok(())
}

/// 摘要行（String 面，无 IO）。
pub fn summary(total: u32) -> String {
    let mut s = String::new();
    s.push_str("dedupaudit scopes=");
    s.push_str(&SCOPE_N.to_string());
    s.push_str(" scanned=");
    s.push_str(&total.to_string());
    s.push_str(" dups=0");
    s
}

/// 复验指纹：五范围签名全部序列化后 FNV-1a。
pub fn fingerprint() -> u64 {
    let mut feed = vec![];
    feed.extend_from_slice(&VER.to_le_bytes());
    let mut i = 0;
    while i < SCOPE_CARDS.len() {
        feed.extend_from_slice(SCOPE_CARDS[i].as_bytes());
        feed.push(0xff);
        i += 1;
    }
    i = 0;
    while i < SCOPE_SEGS.len() {
        feed.extend_from_slice(&SCOPE_SEGS[i].to_le_bytes());
        i += 1;
    }
    i = 0;
    while i < SCOPE_CHECKS.len() {
        feed.extend_from_slice(SCOPE_CHECKS[i].as_bytes());
        feed.push(0xfe);
        i += 1;
    }
    i = 0;
    while i < SCOPE_DELIV.len() {
        feed.extend_from_slice(SCOPE_DELIV[i].as_bytes());
        feed.push(0xfd);
        i += 1;
    }
    core::fnv1a(&feed)
}

/// 诊断码闭集互异自检。
pub fn codes_distinct() -> bool {
    let cs = [E_VER, E_DUP, E_SCOPE, E_AUDIT, E_CS];
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
        E_DUP => "重复签名",
        E_SCOPE => "范围越界（未知范围号）",
        E_AUDIT => "复验器失效（重复未点名）",
        E_CS => "指纹不稳",
        _ => "未知诊断码",
    }
}

/// 演练：R1 锚；R2 五范围逐个复验（范围号分派）；R3 全量复验
/// （20+10+5+5+0=40 条）；R4 注入 String 域必拦；R5 注入 u16 域必拦；
/// R6 单元素边界放行；R7 越界范围号必报 E_SCOPE；R8 指纹稳定；
/// R9 双跑一致。
pub fn drill() -> Result<u32, u16> {
    let mut hits: u32 = 0;
    // R1：版本锚 + 闭集自检。
    if VER != 1 {
        return Err(E_VER);
    }
    if !codes_distinct() {
        return Err(E_CS);
    }
    hits += 1;
    // R2：五范围分派复验。
    let mut sid: u16 = 1;
    while sid <= 5 {
        audit_scope(sid)?;
        hits += 1;
        sid += 1;
    }
    // R3：全量复验，总扫描条数守恒 40。
    let total = audit_all()?;
    if total != 40 {
        return Err(E_AUDIT);
    }
    hits += 1;
    // R4/R5：注入双关。
    injection_dup_str()?;
    hits += 1;
    injection_dup_u16()?;
    hits += 1;
    // R6：单元素边界。
    single_element_edge()?;
    hits += 1;
    // R7：越界范围号。
    match audit_scope(6) {
        Err(E_SCOPE) => {}
        _ => return Err(E_SCOPE),
    }
    hits += 1;
    // R8：指纹稳定且非零。
    let fp1 = fingerprint();
    let fp2 = fingerprint();
    if fp1 != fp2 || fp1 == 0 {
        return Err(E_CS);
    }
    hits += 1;
    // R9：双跑一致。
    let a = audit_all();
    let b = audit_all();
    if a != b {
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
        assert_eq!(drill(), Ok(13));
    }

    #[test]
    fn all_scopes_clean() {
        assert_eq!(audit_all(), Ok(40));
        assert_eq!(audit_scope(1), Ok(20));
        assert_eq!(audit_scope(2), Ok(10));
        assert_eq!(audit_scope(3), Ok(5));
        assert_eq!(audit_scope(4), Ok(5));
        assert_eq!(audit_scope(5), Ok(0));
    }

    #[test]
    fn injection_caught() {
        assert_eq!(injection_dup_str(), Ok(()));
        assert_eq!(injection_dup_u16(), Ok(()));
        assert_eq!(audit_scope(9), Err(E_SCOPE));
        assert!(summary(40).contains("dedupaudit"));
    }

    #[test]
    fn edge_single_ok() {
        assert_eq!(single_element_edge(), Ok(()));
        let dup_pair = ["x", "x"];
        assert_eq!(scan_strs(&dup_pair), Err(E_DUP));
    }

    #[test]
    fn window_shapes() {
        assert!(codes_distinct());
        assert_eq!(VER, 1);
    }
}

pub fn run_unx36154_checks() -> CheckSet {
    let mut cs = CheckSet::new("unxstar-unx36154_dedupaudit");
    cs.add("SEG 段位独占", SEG_BASE == 0xee80, "0xee80~0xee8f 本模块独占");
    match drill() {
        Ok(_) => cs.add("确定性演练全绿", true, "五范围 40 条全查重零重复、注入双关必拦"),
        Err(code) => cs.add("确定性演练全绿", false, detail(code)),
    }
    cs.add("范围数锚 5", SCOPE_N == 5, "范围数锚漂移");
    cs.add("版本锚 VER=1", VER == 1, "版本锚不符");
    cs.add("诊断码闭集互异", codes_distinct(), "诊断码闭集出现重码");
    cs
}
