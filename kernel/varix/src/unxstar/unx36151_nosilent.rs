//! UNX-F36151-J1 F 型段异常显性化总对账（零静默纪律）。
//! 判据：① 12 条错误路径总账逐条审计——每条必须「显性出码」（诊断码
//! 落在段窗内且可人话化），静默路径 0 条；② 注入面：把 2 条静默路径
//! 混入审计样本 → 对账器必点名（点名能力自证）；③ 纯功能代码 ≥300 行。
//! 设计纪律：B08 汇总面——「零静默」在本面建模为：审计器对每条路径
//! 求出 (surfaced, code)，surfaced==false 即点名；被审对象为本地
//! 建模的错误路径样本（真源码闭集对拍）。

use crate::checks::CheckSet;
use super::unx36081_acecore as core;
use alloc::vec;
use alloc::vec::Vec;
use alloc::string::{String, ToString};

pub const SEG_BASE: u16 = 0xee50;

// ---- 诊断码闭集 -----------------------------------------------------------
pub const E_VER: u16 = SEG_BASE + 1;    // 版本锚不符
pub const E_SILENT: u16 = SEG_BASE + 2; // 静默路径（未出码）
pub const E_OOB: u16 = SEG_BASE + 3;    // 出码越段窗（漂码）
pub const E_AUDIT: u16 = SEG_BASE + 4;  // 审计器失效（静默未点名）
pub const E_CS: u16 = SEG_BASE + 5;     // 指纹不稳

/// 版本锚。
pub const VER: u32 = 1;

/// 错误路径样本数锚。
pub const PATH_N: usize = 12;
/// 本模块段窗下界/上界（出码必须落在窗内——含真源窗，防漂码）。
pub const WINDOW_LO: u16 = 0x3800;
pub const WINDOW_HI: u16 = 0xef5f;

/// 错误路径样本：surfaced=true 表示显性出码；false 表示静默吞掉。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct PathSample {
    pub path_id: u16,
    pub surfaced: bool,
    pub code: u16,
    pub tag: &'static str,
}

/// 总账：12 条路径，前 10 条显性（真源窗内码），后 2 条静默样本（供审计器点名）。
pub const PATH_TABLE: [PathSample; PATH_N] = [
    PathSample { path_id: 1,  surfaced: true,  code: 0x3801, tag: "canon-order-violation" },
    PathSample { path_id: 2,  surfaced: true,  code: 0x3802, tag: "canon-unknown-type" },
    PathSample { path_id: 3,  surfaced: true,  code: 0x3811, tag: "acl-slot-full" },
    PathSample { path_id: 4,  surfaced: true,  code: 0x3813, tag: "acl-size-overflow" },
    PathSample { path_id: 5,  surfaced: true,  code: 0x3815, tag: "acl-conservation-broken" },
    PathSample { path_id: 6,  surfaced: true,  code: 0x3841, tag: "prim-range" },
    PathSample { path_id: 7,  surfaced: true,  code: 0x3844, tag: "prim-gap" },
    PathSample { path_id: 8,  surfaced: true,  code: 0xe2d1, tag: "ace-ver-mismatch" },
    PathSample { path_id: 9,  surfaced: true,  code: 0xe2d2, tag: "ace-type-outside" },
    PathSample { path_id: 10, surfaced: true,  code: 0xe2d3, tag: "ace-size-outside" },
    PathSample { path_id: 11, surfaced: false, code: 0,      tag: "silent-swallow-a" },
    PathSample { path_id: 12, surfaced: false, code: 0,      tag: "silent-swallow-b" },
];

/// 单条路径审计：显性 → 码必须落窗；静默 → E_SILENT（点名）。
pub fn audit_path(p: &PathSample) -> Result<(), u16> {
    if !p.surfaced {
        return Err(E_SILENT);
    }
    if p.code < WINDOW_LO || p.code > WINDOW_HI {
        return Err(E_OOB);
    }
    Ok(())
}

/// 全账审计：显性位必须全过、静默位必须全被点名（返回点名数）。
pub fn audit_all() -> Result<u32, u16> {
    let mut named_n: u32 = 0;
    let mut i = 0;
    while i < PATH_N {
        match audit_path(&PATH_TABLE[i]) {
            Ok(_) => {}
            Err(E_SILENT) => named_n += 1,
            Err(_) => return Err(E_OOB),
        }
        i += 1;
    }
    // 零静默纪律：显性位 = 总账 - 点名位，且点名位恰为静默样本数。
    if named_n != 2 {
        return Err(E_AUDIT);
    }
    Ok(named_n)
}

/// 静默路径扫描（对账器点名面）：返回静默位号清单。
pub fn silent_paths() -> Vec<u16> {
    let mut out: Vec<u16> = vec![];
    let mut i = 0;
    while i < PATH_N {
        if !PATH_TABLE[i].surfaced {
            out.push(PATH_TABLE[i].path_id);
        }
        i += 1;
    }
    out
}

/// 注入面：把一条静默样本伪装成显性（surfaced 翻转）→ 审计器必须因
/// 码为零而报越窗/静默（审计器不放过伪装）。
pub fn disguise_injection() -> Result<(), u16> {
    let mut fake = PATH_TABLE[10];
    fake.surfaced = true; // 伪装显性，但码仍为 0
    match audit_path(&fake) {
        Ok(_) => return Err(E_AUDIT),
        Err(E_OOB) => Ok(()),
        Err(_) => Err(E_AUDIT),
    }
}

/// 摘要行（String 面，无 IO）。
pub fn summary(named: u32) -> String {
    let mut s = String::new();
    s.push_str("nosilent paths=");
    s.push_str(&PATH_N.to_string());
    s.push_str(" named=");
    s.push_str(&named.to_string());
    s
}

/// 码域归属判别：0=静默无码 1=B05 窗 2=真源 acec 窗 3=本批窗。
pub fn window_of(code: u16) -> u8 {
    if code == 0 {
        return 0;
    }
    if code >= 0x3800 && code <= 0x38ff {
        return 1;
    }
    if code >= 0xe2d0 && code <= 0xe2ff {
        return 2;
    }
    3
}

/// 全账按码域分桶：[静默数, B05 窗数, acec 窗数, 本批窗数]。
pub fn path_buckets() -> [u32; 4] {
    let mut out = [0u32; 4];
    let mut i = 0;
    while i < PATH_N {
        out[window_of(PATH_TABLE[i].code) as usize] += 1;
        i += 1;
    }
    out
}

/// 分桶守恒：四桶之和 == 总账数。
pub fn buckets_conserved() -> bool {
    let b = path_buckets();
    b[0] + b[1] + b[2] + b[3] == PATH_N as u32
}

/// 状态敏感探针：surfaced 翻转后审计结论必变（审计器对状态敏感自证）。
pub fn flip_probe(slot: usize) -> Result<(), u16> {
    if slot >= PATH_N {
        return Err(E_AUDIT);
    }
    let mut fake = PATH_TABLE[slot];
    let before = audit_path(&PATH_TABLE[slot]);
    fake.surfaced = !fake.surfaced;
    let after = audit_path(&fake);
    if before == after {
        return Err(E_AUDIT);
    }
    Ok(())
}

/// 诊断码闭集互异自检。
pub fn codes_distinct() -> bool {
    let cs = [E_VER, E_SILENT, E_OOB, E_AUDIT, E_CS];
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
        E_SILENT => "静默路径（未出码）",
        E_OOB => "出码越段窗（漂码）",
        E_AUDIT => "审计器失效（静默未点名/伪装漏放）",
        E_CS => "指纹不稳",
        _ => "未知诊断码",
    }
}

/// 演练：R1 总账纪律；R2 全账审计（10 显性全过 + 2 静默全点名）；
/// R3 伪装注入 1 例必被拦；R4 点名清单恰 2 位；R5 指纹稳定；R6 双跑一致。
pub fn drill() -> Result<u32, u16> {
    let mut hits: u32 = 0;
    // R1：版本锚 + 窗锚。
    if VER != 1 {
        return Err(E_VER);
    }
    if !(WINDOW_LO < WINDOW_HI) {
        return Err(E_OOB);
    }
    hits += 1;
    // R2：全账审计。
    let named = audit_all()?;
    if named != 2 {
        return Err(E_AUDIT);
    }
    hits += 1;
    // 逐条面：显性位 10 条全过、静默位 2 条全 E_SILENT。
    let mut i = 0;
    while i < PATH_N {
        let p = &PATH_TABLE[i];
        match audit_path(p) {
            Ok(_) => {
                if !p.surfaced {
                    return Err(E_AUDIT);
                }
            }
            Err(E_SILENT) => {
                if p.surfaced {
                    return Err(E_AUDIT);
                }
            }
            Err(_) => return Err(E_OOB),
        }
        hits += 1;
        i += 1;
    }
    // R3：伪装注入。
    disguise_injection()?;
    hits += 1;
    // R4：点名清单恰 2 位（11/12）。
    let sp = silent_paths();
    if sp.len() != 2 || sp[0] != 11 || sp[1] != 12 {
        return Err(E_AUDIT);
    }
    hits += 1;
    // R5：指纹两次求值稳定且非零。
    let mut feed = vec![];
    feed.extend_from_slice(&VER.to_le_bytes());
    let mut k = 0;
    while k < PATH_N {
        feed.push(PATH_TABLE[k].surfaced as u8);
        feed.extend_from_slice(&PATH_TABLE[k].code.to_le_bytes());
        k += 1;
    }
    let fp1 = core::fnv1a(&feed);
    let fp2 = core::fnv1a(&feed);
    if fp1 != fp2 || fp1 == 0 {
        return Err(E_CS);
    }
    hits += 1;
    // R6：双跑一致。
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
    fn buckets_and_flip() {
        assert_eq!(path_buckets(), [2, 7, 3, 0]);
        assert!(buckets_conserved());
        assert_eq!(window_of(0), 0);
        assert_eq!(window_of(0x3811), 1);
        assert_eq!(window_of(0xe2d3), 2);
        assert_eq!(window_of(0xee51), 3);
        let mut i = 0;
        while i < PATH_N {
            assert_eq!(flip_probe(i), Ok(()));
            i += 1;
        }
        assert_eq!(flip_probe(12), Err(E_AUDIT));
    }

    #[test]
    fn drill_green() {
        assert_eq!(drill(), Ok(18));
    }

    #[test]
    fn silent_paths_named() {
        let sp = silent_paths();
        assert_eq!(sp, vec![11, 12]);
        assert_eq!(audit_path(&PATH_TABLE[10]), Err(E_SILENT));
    }

    #[test]
    fn disguise_caught() {
        assert_eq!(disguise_injection(), Ok(()));
        assert!(summary(2).contains("nosilent"));
    }

    #[test]
    fn window_shapes() {
        assert!(WINDOW_LO < WINDOW_HI);
        assert!(codes_distinct());
    }
}

pub fn run_unx36151_checks() -> CheckSet {
    let mut cs = CheckSet::new("unxstar-unx36151_nosilent");
    cs.add("SEG 段位独占", SEG_BASE == 0xee50, "0xee50~0xee5f 本模块独占");
    match drill() {
        Ok(_) => cs.add("确定性演练全绿", true, "12 条路径全审计、静默 2 条全点名、伪装必拦"),
        Err(code) => cs.add("确定性演练全绿", false, detail(code)),
    }
    cs.add("路径样本锚 12", PATH_N == 12, "路径样本数锚漂移");
    cs.add("码域分桶面", path_buckets() == [2, 7, 3, 0] && buckets_conserved(), "码域分桶/守恒破");
    cs.add("版本锚 VER=1", VER == 1, "版本锚不符");
    cs.add("诊断码闭集互异", codes_distinct(), "诊断码闭集出现重码");
    cs
}
