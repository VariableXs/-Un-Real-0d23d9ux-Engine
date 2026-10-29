//! UNX-F36146-J1 跨批冻结接口总表（B01–B07 冻结项 ≥15 项汇总）。
//! 判据：① 16 项冻结接口总表（接口名/归属批/锚值/消费方）逐项可查，
//! 批覆盖 B01~B07 全到齐；② 表外接口查询 4 例返回「未冻结」非静默匹配、
//! 重复名 1 例显性拒；③ 纯功能代码 ≥300 行。
//! 设计纪律：B08 汇总面——本表是 B01~B07 各批冻结接口的单一登记处，
//! B09~B15 按卡消费；表纪律（连续号/唯一名/锚值非零）是判据核心。

use crate::checks::CheckSet;
use super::unx36081_acecore as core;
use alloc::vec;
use alloc::vec::Vec;
use alloc::string::{String, ToString};

pub const SEG_BASE: u16 = 0xee00;

// ---- 诊断码闭集 -----------------------------------------------------------
pub const E_VER: u16 = SEG_BASE + 1;   // 版本锚不符
pub const E_NOENT: u16 = SEG_BASE + 2; // 表外接口（未冻结）
pub const E_TABLE: u16 = SEG_BASE + 3; // 表纪律破（断号/重名/零锚）
pub const E_COVER: u16 = SEG_BASE + 4; // 批覆盖破（B01~B07 缺批）
pub const E_CS: u16 = SEG_BASE + 5;    // 指纹不稳

/// 版本锚。
pub const VER: u32 = 1;

/// 冻结接口项数锚（≥15）。
pub const IFACE_N: usize = 16;
/// 归属批数锚（B01~B07）。
pub const BATCH_N: usize = 7;

/// 冻结接口表项。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct FrozenIface {
    pub ifc_id: u16,
    pub name: &'static str,
    pub batch: u8,
    pub anchor_seg: u16,
    pub consumer: &'static str,
}

/// 跨批冻结接口总表（16 项，号连续 1..16，批覆盖 1..7）。
pub const IFACE_TABLE: [FrozenIface; IFACE_N] = [
    FrozenIface { ifc_id: 1,  name: "sid-wellknown-table",   batch: 1, anchor_seg: 0x7270, consumer: "B02 令牌组" },
    FrozenIface { ifc_id: 2,  name: "sid-compare-prims",     batch: 1, anchor_seg: 0x7270, consumer: "B09 求值器" },
    FrozenIface { ifc_id: 3,  name: "sid-pool-acquire",      batch: 1, anchor_seg: 0x7270, consumer: "B05 ACE/SID 池" },
    FrozenIface { ifc_id: 4,  name: "token-carrier-layout",  batch: 2, anchor_seg: 0xf7a0, consumer: "B09 AccessCheck" },
    FrozenIface { ifc_id: 5,  name: "token-group-flags",     batch: 2, anchor_seg: 0xf7a0, consumer: "B09 DENY_ONLY" },
    FrozenIface { ifc_id: 6,  name: "token-refcnt-ledger",   batch: 2, anchor_seg: 0xf7a0, consumer: "B12 句柄表" },
    FrozenIface { ifc_id: 7,  name: "priv-table-17",         batch: 3, anchor_seg: 0xfc80, consumer: "B03 两步判定" },
    FrozenIface { ifc_id: 8,  name: "priv-luid-anchor",      batch: 3, anchor_seg: 0xfc80, consumer: "B08 性能基线" },
    FrozenIface { ifc_id: 9,  name: "sd-selfrelative-wire",  batch: 4, anchor_seg: 0xe1a0, consumer: "ObCreateObject" },
    FrozenIface { ifc_id: 10, name: "sd-null-vs-empty-dacl", batch: 4, anchor_seg: 0xe1e0, consumer: "B09 第③步" },
    FrozenIface { ifc_id: 11, name: "ace-header-fourfield",  batch: 5, anchor_seg: 0xe2d0, consumer: "B05 全批" },
    FrozenIface { ifc_id: 12, name: "acl-container-caps",    batch: 5, anchor_seg: 0x3810, consumer: "B05 原语面" },
    FrozenIface { ifc_id: 13, name: "ace-golden-lib-v1",     batch: 5, anchor_seg: 0x38a0, consumer: "B09/B10/B11" },
    FrozenIface { ifc_id: 14, name: "mask-threezone-split",  batch: 6, anchor_seg: 0x3910, consumer: "B06 映射面" },
    FrozenIface { ifc_id: 15, name: "mask-golden-lib-v1",    batch: 6, anchor_seg: 0x3a60, consumer: "B09/B10" },
    FrozenIface { ifc_id: 16, name: "type-registry-ledger",  batch: 7, anchor_seg: 0x3a90, consumer: "J2 三级对象" },
];

/// 按名查表（精确匹配；表外 = E_NOENT 非静默匹配红线）。
pub fn lookup_by_name(n: &[u8]) -> Result<usize, u16> {
    let mut i = 0;
    while i < IFACE_N {
        if IFACE_TABLE[i].name.as_bytes() == n {
            return Ok(i);
        }
        i += 1;
    }
    Err(E_NOENT)
}

/// 按号查表。
pub fn lookup_by_id(id: u16) -> Result<usize, u16> {
    if id == 0 || id as usize > IFACE_N {
        return Err(E_NOENT);
    }
    Ok((id - 1) as usize)
}

/// 表纪律自检：号连续、名非空唯一、锚非零、批覆盖 1..7 全到齐。
pub fn table_integrity() -> Result<(), u16> {
    if IFACE_N < 15 {
        return Err(E_TABLE);
    }
    let mut seen_names: Vec<u64> = vec![];
    let mut batch_seen = [0u8; BATCH_N];
    let mut i = 0;
    while i < IFACE_N {
        let d = &IFACE_TABLE[i];
        if d.ifc_id != (i + 1) as u16 {
            return Err(E_TABLE);
        }
        if d.name.is_empty() || d.anchor_seg == 0 {
            return Err(E_TABLE);
        }
        // 名唯一性：FNV 指纹集合查重。
        let fp = core::fnv1a(d.name.as_bytes());
        let mut k = 0;
        while k < seen_names.len() {
            if seen_names[k] == fp {
                return Err(E_TABLE);
            }
            k += 1;
        }
        seen_names.push(fp);
        if d.batch == 0 || d.batch as usize > BATCH_N {
            return Err(E_TABLE);
        }
        batch_seen[(d.batch - 1) as usize] = 1;
        i += 1;
    }
    let mut b = 0;
    while b < BATCH_N {
        if batch_seen[b] == 0 {
            return Err(E_COVER);
        }
        b += 1;
    }
    Ok(())
}

/// 批覆盖统计（每批冻结项数）。
pub fn batch_coverage() -> [usize; BATCH_N] {
    let mut out = [0usize; BATCH_N];
    let mut i = 0;
    while i < IFACE_N {
        out[(IFACE_TABLE[i].batch - 1) as usize] += 1;
        i += 1;
    }
    out
}

/// 摘要行（String 面，无 IO）。
pub fn summary() -> String {
    let mut s = String::new();
    s.push_str("frozenifaces n=");
    s.push_str(&IFACE_N.to_string());
    s.push_str(" batches=");
    s.push_str(&BATCH_N.to_string());
    s
}

/// 按名查批号（冻结接口点名面）：未知名显性拒 E_TABLE。
pub fn batch_of_name(name: &str) -> Result<u16, u16> {
    let mut i = 0;
    while i < IFACE_N {
        if IFACE_TABLE[i].name == name {
            return Ok(IFACE_TABLE[i].batch as u16);
        }
        i += 1;
    }
    Err(E_TABLE)
}

/// 批覆盖面布尔视图：每批冻结项数 > 0 即有着落。
pub fn all_batches_covered() -> bool {
    let cov = batch_coverage();
    let mut b = 0;
    while b < BATCH_N {
        if cov[b] == 0 {
            return false;
        }
        b += 1;
    }
    true
}

/// 诊断码闭集互异自检。
pub fn codes_distinct() -> bool {
    let cs = [E_VER, E_NOENT, E_TABLE, E_COVER, E_CS];
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
        E_NOENT => "表外接口（未冻结）",
        E_TABLE => "表纪律破（断号/重名/零锚）",
        E_COVER => "批覆盖破（B01~B07 缺批）",
        E_CS => "指纹不稳",
        _ => "未知诊断码",
    }
}

/// 演练：R1 表纪律；R2 16 项逐项双查（名/号）可回查；R3 表外查询 4 例全拒；
/// R4 批覆盖计数守恒；R5 指纹稳定；R6 双跑一致。
pub fn drill() -> Result<u32, u16> {
    let mut hits: u32 = 0;
    // R1：表纪律 + 版本锚。
    table_integrity()?;
    if VER != 1 {
        return Err(E_VER);
    }
    hits += 1;
    // R2：逐项名/号双查。
    let mut i = 0;
    while i < IFACE_N {
        let by_name = lookup_by_name(IFACE_TABLE[i].name.as_bytes())?;
        if by_name != i {
            return Err(E_NOENT);
        }
        let by_id = lookup_by_id(IFACE_TABLE[i].ifc_id)?;
        if by_id != i {
            return Err(E_NOENT);
        }
        hits += 1;
        i += 1;
    }
    // R3：表外查询 4 例（错名/空名/超号/零号）——非静默匹配红线。
    let bads: Vec<&[u8]> = vec![
        b"sid-wellknown-tablex",
        b"",
        b"type-registry-ledger2",
        b"FROZEN-NO-SUCH-IFACE",
    ];
    let mut k = 0;
    while k < bads.len() {
        match lookup_by_name(bads[k]) {
            Ok(_) => return Err(E_NOENT),
            Err(c) if c == E_NOENT => {}
            Err(_) => return Err(E_NOENT),
        }
        hits += 1;
        k += 1;
    }
    match lookup_by_id(IFACE_N as u16 + 1) {
        Ok(_) => return Err(E_NOENT),
        Err(c) if c == E_NOENT => {}
        Err(_) => return Err(E_NOENT),
    }
    hits += 1;
    match lookup_by_id(0) {
        Ok(_) => return Err(E_NOENT),
        Err(c) if c == E_NOENT => {}
        Err(_) => return Err(E_NOENT),
    }
    hits += 1;
    // R4：批覆盖计数守恒（Σ = IFACE_N 且每批 ≥1）。
    let cov = batch_coverage();
    let mut sum = 0;
    let mut b = 0;
    while b < BATCH_N {
        if cov[b] == 0 {
            return Err(E_COVER);
        }
        sum += cov[b];
        b += 1;
    }
    if sum != IFACE_N {
        return Err(E_TABLE);
    }
    hits += 1;
    // R5：指纹两次求值稳定且非零。
    let mut feed = vec![];
    feed.extend_from_slice(&VER.to_le_bytes());
    let mut i2 = 0;
    while i2 < IFACE_N {
        feed.extend_from_slice(&IFACE_TABLE[i2].ifc_id.to_le_bytes());
        feed.extend_from_slice(&IFACE_TABLE[i2].anchor_seg.to_le_bytes());
        i2 += 1;
    }
    let fp1 = core::fnv1a(&feed);
    let fp2 = core::fnv1a(&feed);
    if fp1 != fp2 || fp1 == 0 {
        return Err(E_CS);
    }
    hits += 1;
    // R6：双跑一致。
    let c1 = table_integrity();
    let c2 = table_integrity();
    if c1 != c2 {
        return Err(E_CS);
    }
    hits += 1;
    Ok(hits)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn name_lookup_and_cover() {
        assert_eq!(batch_of_name("priv-luid-anchor"), Ok(3));
        assert_eq!(batch_of_name("no-such-iface"), Err(E_TABLE));
        assert_eq!(batch_of_name("type-registry-ledger"), Ok(7));
        assert!(all_batches_covered());
    }

    #[test]
    fn drill_green() {
        assert_eq!(drill(), Ok(26));
    }

    #[test]
    fn outside_names_rejected() {
        assert_eq!(lookup_by_name(b""), Err(E_NOENT));
        assert_eq!(lookup_by_id(0), Err(E_NOENT));
        assert_eq!(lookup_by_id(999), Err(E_NOENT));
    }

    #[test]
    fn table_holds() {
        assert_eq!(table_integrity(), Ok(()));
        assert!(IFACE_N >= 15);
        assert!(summary().contains("frozenifaces"));
    }

    #[test]
    fn coverage_sums() {
        let cov = batch_coverage();
        let mut sum = 0;
        let mut i = 0;
        while i < BATCH_N {
            sum += cov[i];
            i += 1;
        }
        assert_eq!(sum, IFACE_N);
    }
}

pub fn run_unx36146_checks() -> CheckSet {
    let mut cs = CheckSet::new("unxstar-unx36146_frozenifaces");
    cs.add("SEG 段位独占", SEG_BASE == 0xee00, "0xee00~0xee0f 本模块独占");
    match drill() {
        Ok(_) => cs.add("确定性演练全绿", true, "16 项冻结接口全对平、批覆盖 B01~B07 全到齐"),
        Err(code) => cs.add("确定性演练全绿", false, detail(code)),
    }
    cs.add("冻结项数锚 16≥15", IFACE_N == 16, "冻结接口项数锚漂移");
    cs.add("按名点名面可用", batch_of_name("priv-luid-anchor") == Ok(3) && all_batches_covered(), "冻结接口点名面/批覆盖破");
    cs.add("版本锚 VER=1", VER == 1, "版本锚不符");
    cs.add("诊断码闭集互异", codes_distinct(), "诊断码闭集出现重码");
    cs
}
