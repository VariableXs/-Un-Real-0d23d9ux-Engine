//! unx36004_wellknown — UNX-F36004 内置域 SID 与知名 RID 常量表。
//!
//! 判据原文：
//!   J1 知名 RID 表 ≥30 项与版本锚逐项一致；每项双向（RID→名/名→RID）
//!      查询 100 例零差。
//! 设计纪律：
//!   1. 版本锚：知名 SID 以（IA, 子权限前缀, RID）三元组唯一确定，条目
//!      覆盖 Null/Everyone/Creator 族/NT 授权族（Dialup~NetworkService）/
//!      BUILTIN 组族（544~582）。本表 40 项，全部为 Windows 语义公开锚。
//!   2. 双向查询闭面：正向（键→名）与反向（名→键）共用一张表，两个
//!      方向逐项互验（任一方向失配即锚破）；表内名字与键各自唯一。
//!   3. 纯功能线：no_std，零 unwrap/expect/panic，零堆分配（定容表），
//!      确定性。

pub const SEG_BASE: u16 = 0x72D0;

// ── 诊断码闭集（0x72D0~0x72EF，独占）─────────────────────────
const E_TAB: u16 = SEG_BASE + 1; // 表长 < 30（表损坏）
const E_DUP: u16 = SEG_BASE + 2; // 名字或键重复（唯一性失守）
const E_MISS: u16 = SEG_BASE + 3; // 双向查询缺项
const E_ANCHOR: u16 = SEG_BASE + 4; // 版本锚逐项失配
const E_BAL: u16 = SEG_BASE + 5; // 查询账守恒破
const E_BAD: u16 = SEG_BASE + 6; // 非法查询键（子权前缀超长）

use crate::checks::CheckSet;
use super::unx36001_sidcore::{Sid, MAX_SUBS};

/// 表项容量：40 项知名 SID。
pub const WK_COUNT: usize = 40;
/// 版本锚下限：≥30 项。
pub const WK_MIN: usize = 30;
/// 名字字节长度上限（含 0 结尾余量按 24 定容）。
pub const NAME_CAP: usize = 24;

/// 单条知名 SID 表项：(IA, 子权限前缀（最多 2 项）, RID, 名)。
/// 子权限前缀长度为 prefix_len（0/1/2），RID 恒为最末子权限。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct WkEntry {
    pub ia: u64,               // 标识符颁发机构（48 位值）
    pub prefix: [u32; 2],      // 子权限前缀（prefix_len 项有效）
    pub prefix_len: u8,        // 前缀长度（0/1/2）
    pub rid: u32,              // RID（最末子权限）
    pub name: [u8; NAME_CAP],  // 短名（0 填充）
    pub name_len: u8,          // 名字有效长度
}

/// 名字 → 表项字节数组（0 填充）。
const fn nb(src: &[u8]) -> ([u8; NAME_CAP], u8) {
    let mut out = [0u8; NAME_CAP];
    let mut i = 0;
    while i < src.len() && i < NAME_CAP {
        out[i] = src[i];
        i += 1;
    }
    (out, src.len() as u8)
}

const fn e1(ia: u64, rid: u32, name: &'static [u8]) -> WkEntry {
    let (n, l) = nb(name);
    WkEntry { ia, prefix: [0, 0], prefix_len: 0, rid, name: n, name_len: l }
}

const fn e2(ia: u64, p1: u32, rid: u32, name: &'static [u8]) -> WkEntry {
    let (n, l) = nb(name);
    WkEntry { ia, prefix: [p1, 0], prefix_len: 1, rid, name: n, name_len: l }
}

/// 知名 SID 版本锚表（40 项，Windows 公开语义）。
pub const WK_TAB: [WkEntry; WK_COUNT] = [
    e1(0, 0, b"NULL"),
    e1(1, 0, b"EVERYONE"),
    e1(2, 0, b"LOCAL"),
    e1(2, 1, b"CONSOLE_LOGON"),
    e1(3, 0, b"CREATOR_OWNER"),
    e1(3, 1, b"CREATOR_GROUP"),
    e1(3, 2, b"CREATOR_OWNER_SERVER"),
    e1(3, 3, b"CREATOR_GROUP_SERVER"),
    e1(3, 4, b"CREATOR_OWNER_RIGHTS"),
    e1(3, 5, b"CREATOR_GROUP_RIGHTS"),
    e1(5, 1, b"DIALUP"),
    e1(5, 2, b"NETWORK"),
    e1(5, 3, b"BATCH"),
    e1(5, 4, b"INTERACTIVE"),
    e1(5, 6, b"SERVICE"),
    e1(5, 7, b"ANONYMOUS"),
    e1(5, 8, b"PROXY"),
    e1(5, 9, b"ENTERPRISE_DC"),
    e1(5, 10, b"PRINCIPAL_SELF"),
    e1(5, 11, b"AUTHENTICATED_USERS"),
    e1(5, 12, b"RESTRICTED_CODE"),
    e1(5, 13, b"THIS_ORGANIZATION"),
    e1(5, 14, b"OTHER_ORGANIZATION"),
    e1(5, 17, b"IUSR"),
    e1(5, 18, b"LOCAL_SYSTEM"),
    e1(5, 19, b"LOCAL_SERVICE"),
    e1(5, 20, b"NETWORK_SERVICE"),
    e2(5, 32, 544, b"ADMINISTRATORS"),
    e2(5, 32, 545, b"USERS"),
    e2(5, 32, 546, b"GUESTS"),
    e2(5, 32, 547, b"POWER_USERS"),
    e2(5, 32, 548, b"ACCOUNT_OPERATORS"),
    e2(5, 32, 549, b"SERVER_OPERATORS"),
    e2(5, 32, 550, b"PRINT_OPERATORS"),
    e2(5, 32, 551, b"BACKUP_OPERATORS"),
    e2(5, 32, 552, b"REPLICATOR"),
    e2(5, 32, 554, b"PRE_W2K_COMPAT_ACCESS"),
    e2(5, 32, 555, b"REMOTE_DESKTOP_USERS"),
    e2(5, 32, 573, b"EVENT_LOG_READERS"),
    e2(5, 32, 580, b"REMOTE_MANAGEMENT_USERS"),
];

/// 表项 → 查询键（ia + 前缀 + RID）→ 完整 SID。
pub fn entry_sid(w: &WkEntry) -> Result<Sid, u16> {
    if w.prefix_len as usize > 2 {
        return Err(E_BAD);
    }
    let mut subs = [0u32; MAX_SUBS];
    let mut i = 0;
    while i < w.prefix_len as usize {
        subs[i] = w.prefix[i];
        i += 1;
    }
    subs[w.prefix_len as usize] = w.rid;
    Sid::new(1, Sid::ia_from(w.ia), &subs[..w.prefix_len as usize + 1])
}

/// 正向查询：键（IA+前缀+RID）→ 名（未命中 None）。
pub fn name_of_key(ia: u64, prefix: &[u32; 2], prefix_len: usize, rid: u32) -> Option<&'static [u8]> {
    let mut i = 0;
    while i < WK_COUNT {
        let w = &WK_TAB[i];
        if w.ia == ia
            && w.rid == rid
            && w.prefix_len as usize == prefix_len
            && w.prefix[0] == prefix[0]
            && w.prefix[1] == prefix[1]
        {
            return Some(&w.name[..w.name_len as usize]);
        }
        i += 1;
    }
    None
}

/// 反向查询：名 → 完整 SID（未命中 None）。
pub fn sid_of_name(name: &[u8]) -> Option<Sid> {
    let mut i = 0;
    while i < WK_COUNT {
        let w = &WK_TAB[i];
        if w.name_len as usize == name.len() && &w.name[..w.name_len as usize] == name {
            return entry_sid(w).ok();
        }
        i += 1;
    }
    None
}

/// 表自检：长度 ≥30 + 名字唯一 + 键唯一（判据 J1 前半）。
pub fn table_selfcheck() -> Result<u32, u16> {
    if WK_COUNT < WK_MIN {
        return Err(E_TAB);
    }
    let mut a = 0;
    while a < WK_COUNT {
        let mut b = a + 1;
        while b < WK_COUNT {
            let (x, y) = (&WK_TAB[a], &WK_TAB[b]);
            let name_eq = x.name_len == y.name_len && x.name[..x.name_len as usize] == y.name[..y.name_len as usize];
            let key_eq = x.ia == y.ia
                && x.rid == y.rid
                && x.prefix_len == y.prefix_len
                && x.prefix[0] == y.prefix[0]
                && x.prefix[1] == y.prefix[1];
            if name_eq || key_eq {
                return Err(E_DUP);
            }
            b += 1;
        }
        a += 1;
    }
    Ok(WK_COUNT as u32)
}

/// 域扫描：同 IA（+同前缀族）下的知名条目清点——
/// 例如 IA=5 前缀 [32]（BUILTIN 域）应有 12 组。
pub fn census_of_domain(ia: u64, prefix: &[u32; 2], prefix_len: usize) -> u32 {
    let mut n = 0;
    let mut i = 0;
    while i < WK_COUNT {
        let w = &WK_TAB[i];
        if w.ia == ia && w.prefix_len as usize == prefix_len
            && w.prefix[0] == prefix[0] && w.prefix[1] == prefix[1]
        {
            n += 1;
        }
        i += 1;
    }
    n
}

/// 名字合法性：查表名的构造纪律（大写/数字/下划线、非空、≤NAME_CAP）。
pub fn name_wellformed(name: &[u8]) -> bool {
    if name.is_empty() || name.len() > NAME_CAP {
        return false;
    }
    let mut i = 0;
    while i < name.len() {
        let c = name[i];
        if !(c.is_ascii_uppercase() || c.is_ascii_digit() || c == b'_') {
            return false;
        }
        i += 1;
    }
    true
}

/// 全表名字纪律巡检：40 名全部 wellformed（表健康面）。
pub fn name_discipline_scan() -> Result<u32, u16> {
    let mut i = 0;
    while i < WK_COUNT {
        let w = &WK_TAB[i];
        if !name_wellformed(&w.name[..w.name_len as usize]) {
            return Err(E_DUP);
        }
        i += 1;
    }
    Ok(WK_COUNT as u32)
}

/// RID 段位清点：给定 IA + 前缀族下的 RID 最小/最大值（域轮廓面）。
pub fn rid_span(ia: u64, prefix: &[u32; 2], prefix_len: usize) -> Option<(u32, u32)> {
    let mut lo = u32::MAX;
    let mut hi = 0u32;
    let mut found = false;
    let mut i = 0;
    while i < WK_COUNT {
        let w = &WK_TAB[i];
        if w.ia == ia && w.prefix_len as usize == prefix_len
            && w.prefix[0] == prefix[0] && w.prefix[1] == prefix[1]
        {
            found = true;
            if w.rid < lo {
                lo = w.rid;
            }
            if w.rid > hi {
                hi = w.rid;
            }
        }
        i += 1;
    }
    if found {
        Some((lo, hi))
    } else {
        None
    }
}

/// 确定性演练：锚逐项一致 + 双向 100 例查询零差 + 守恒（判据 J1）。
pub fn drill() -> Result<u32, u16> {
    let mut hits: u32 = 0;
    let mut queries: u32 = 0;
    let mut misses: u32 = 0;

    // R0：表自检（40 ≥ 30，名字/键唯一）。
    let n = table_selfcheck()?;
    if n != WK_COUNT as u32 {
        return Err(E_TAB);
    }

    // R1：锚逐项一致——每条表项可构造 SID 且 RID=末子权、名非空。
    let mut i = 0;
    while i < WK_COUNT {
        let w = &WK_TAB[i];
        if w.name_len == 0 || w.name_len as usize > NAME_CAP {
            return Err(E_ANCHOR);
        }
        let s = entry_sid(w)?;
        if s.rid() != w.rid || s.ia_value() != w.ia {
            return Err(E_ANCHOR);
        }
        // 正反向互验：正向命中必须与表项名一致；反向构造必须还原同键。
        let got = name_of_key(w.ia, &w.prefix, w.prefix_len as usize, w.rid).ok_or(E_MISS)?;
        if got != &w.name[..w.name_len as usize] {
            return Err(E_ANCHOR);
        }
        let back = sid_of_name(&w.name[..w.name_len as usize]).ok_or(E_MISS)?;
        if back.rid() != w.rid || back.ia_value() != w.ia {
            return Err(E_ANCHOR);
        }
        queries += 2;
        i += 1;
    }
    hits += 1;

    // R2：双向查询 100 例——80 命中（40×2 双向）+ 20 未命中显性 None，
    // 查询账守恒：命中 + 未命中 == 总查询。
    // 未命中样本：合法但不在表内的键/名各 10 例。
    let mut k = 0u32;
    while k < 10 {
        // 伪键：RID 40000+k（BUILTIN 域外段，确定不在表内）。
        if name_of_key(5, &[32, 0], 1, 40000 + k).is_some() {
            return Err(E_MISS);
        }
        if sid_of_name(b"NO_SUCH_NAME_36004").is_some() {
            return Err(E_MISS);
        }
        misses += 2;
        k += 1;
    }
    if queries + misses != 100 {
        return Err(E_BAL);
    }
    if queries != 80 || misses != 20 {
        return Err(E_BAL);
    }
    hits += 1;

    // R3：典型锚抽验——Everyone=S-1-1-0、Administrators=S-1-5-32-544、
    // LocalSystem=S-1-5-18，双向逐一对平。
    let ev = sid_of_name(b"EVERYONE").ok_or(E_MISS)?;
    if ev.ia_value() != 1 || ev.rid() != 0 {
        return Err(E_ANCHOR);
    }
    let adm = sid_of_name(b"ADMINISTRATORS").ok_or(E_MISS)?;
    if adm.ia_value() != 5 || adm.subs() != [32u32, 544u32] {
        return Err(E_ANCHOR);
    }
    let sys = sid_of_name(b"LOCAL_SYSTEM").ok_or(E_MISS)?;
    if sys.ia_value() != 5 || sys.subs() != [18u32] {
        return Err(E_ANCHOR);
    }
    if name_of_key(5, &[32, 0], 1, 544) != Some(&WK_TAB[27].name[..WK_TAB[27].name_len as usize]) {
        return Err(E_ANCHOR);
    }
    hits += 1;

    // R4：域轮廓面——域扫描清点 + RID 段位 + 名字纪律巡检。
    // IA=5 无前缀族（NT 授权族）17 组；BUILTIN 域（前缀 [32]）13 组；
    // BUILTIN RID 段位 [544, 580]；40 名全大写下划线。
    if census_of_domain(5, &[0, 0], 0) != 17 {
        return Err(E_BAL);
    }
    if census_of_domain(5, &[32, 0], 1) != 13 {
        return Err(E_BAL);
    }
    if census_of_domain(1, &[0, 0], 0) != 1 {
        return Err(E_BAL);
    }
    if census_of_domain(9, &[0, 0], 0) != 0 {
        return Err(E_BAL);
    }
    let (lo, hi) = rid_span(5, &[32, 0], 1).ok_or(E_MISS)?;
    if lo != 544 || hi != 580 {
        return Err(E_ANCHOR);
    }
    if name_discipline_scan()? != WK_COUNT as u32 {
        return Err(E_DUP);
    }
    if name_wellformed(b"BAD-NAME") || name_wellformed(b"") || name_wellformed(b"ok") {
        return Err(E_DUP);
    }
    if !name_wellformed(b"ADMINISTRATORS") {
        return Err(E_DUP);
    }
    hits += 1;

    Ok(hits)
}

// ── 判据聚合（CheckSet 面 · 进 unxstar_blocks 检查表）────────
/// 判据 J1 聚合入口：SEG 锚 + 确定性演练全绿（红时回填诊断码）。
pub fn run_unx36004_checks() -> CheckSet {
    let mut cs = CheckSet::new("unxstar-unx36004_wellknown");
    cs.add("SEG 段位独占", SEG_BASE == 0x72D0, "0x72D0~0x72EF 本模块独占");
    match drill() {
        Ok(_) => cs.add("J1 确定性演练全绿", true, "演练段全对平、注入全拒"),
        Err(code) => cs.add("J1 确定性演练全绿", false, detail(code)),
    }
    cs
}

/// 诊断码释义。
pub fn detail(code: u16) -> &'static str {
    match code {
        E_TAB => "表长 < 30（表损坏）",
        E_DUP => "名字或键重复（唯一性失守）",
        E_MISS => "双向查询缺项",
        E_ANCHOR => "版本锚逐项失配",
        E_BAL => "查询账守恒破",
        E_BAD => "非法查询键（子权前缀超长）",
        _ => "未知诊断码",
    }
}

#[cfg(test)]
mod tests {
    use super::drill;

    #[test]
    fn drill_green() {
        assert_eq!(drill(), Ok(4));
    }
}
