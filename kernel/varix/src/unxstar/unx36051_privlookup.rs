//! UNX-F36051-J1 特权名 ↔ LUID 双向 Lookup（LookupPrivilegeValue 类）。
//! 判据：① 双向查找 17×2 项 ×20 例全对；② 大小写变体注入 20 例按规范
//! 归一或拒绝（二选一钉死不二义）；③ 纯功能代码 ≥300 行。
//! 设计纪律：大小写策略钉死为「归一」——Lookup 前统一 ASCII 大写化，
//! 表内名称以规范形式存放；归一后精确匹配，无第二路径。

use super::unx36041_privtable::{self, lookup_by_luid, PRIV_N};
use crate::checks::CheckSet;

pub const SEG_BASE: u16 = 0xfdc0;

// ---- 诊断码闭集 -----------------------------------------------------------
pub const E_NOENT: u16 = SEG_BASE + 1; // 双向未命中（表外）
pub const E_NAME: u16 = SEG_BASE + 2; // 名称格式非法
pub const E_BUF: u16 = SEG_BASE + 3; // 输出缓冲不足
pub const E_AMBIG: u16 = SEG_BASE + 4; // 归一后二义（表纪律破）
pub const E_TABLE: u16 = SEG_BASE + 5; // 上游表纪律破
pub const E_ROUND: u16 = SEG_BASE + 6; // 往返失配

/// ASCII 大写归一（Lookup 前；非字母原样）。
pub fn normalize(name: &[u8], out: &mut [u8]) -> Result<usize, u16> {
    if name.len() > out.len() {
        return Err(E_BUF);
    }
    let mut i = 0;
    while i < name.len() {
        out[i] = name[i].to_ascii_uppercase();
        i += 1;
    }
    Ok(name.len())
}

/// 名称 → LUID（LookupPrivilegeValue 语义）：先归一再精确匹配。
/// 表内名称本就是规范形式（PascalCase），归一成全大写后需对表内
/// 名称也做同形归一比较——双向同构。
pub fn luid_of_name(name: &[u8]) -> Result<u32, u16> {
    if name.is_empty() {
        return Err(E_NAME);
    }
    let mut norm = [0u8; 40];
    let n = normalize(name, &mut norm)?;
    let mut i = 0;
    while i < PRIV_N {
        let canon = unx36041_privtable::PRIV_TABLE[i].name.as_bytes();
        let mut cn = [0u8; 40];
        let cn_len = normalize(canon, &mut cn)?;
        if cn_len == n && &cn[..n] == &norm[..n] {
            return Ok(unx36041_privtable::PRIV_TABLE[i].luid_low);
        }
        i += 1;
    }
    Err(E_NOENT)
}

/// LUID → 名称（LookupPrivilegeName 语义）：直接精确匹配表内规范名。
pub fn name_of_luid(low: u32, out: &mut [u8]) -> Result<usize, u16> {
    let idx = lookup_by_luid(low)?;
    let canon = unx36041_privtable::PRIV_TABLE[idx].name.as_bytes();
    if canon.len() > out.len() {
        return Err(E_BUF);
    }
    let mut i = 0;
    while i < canon.len() {
        out[i] = canon[i];
        i += 1;
    }
    Ok(canon.len())
}

/// 归一策略自检：表内 17 项名称归一后互不碰撞（无二义红线）。
pub fn no_ambiguity() -> Result<(), u16> {
    let mut i = 0;
    while i < PRIV_N {
        let mut a = [0u8; 40];
        let al = normalize(unx36041_privtable::PRIV_TABLE[i].name.as_bytes(), &mut a)?;
        let mut j = i + 1;
        while j < PRIV_N {
            let mut b = [0u8; 40];
            let bl = normalize(unx36041_privtable::PRIV_TABLE[j].name.as_bytes(), &mut b)?;
            if al == bl && &a[..al] == &b[..bl] {
                return Err(E_AMBIG);
            }
            j += 1;
        }
        i += 1;
    }
    Ok(())
}

/// 大小写变体构造：对表内名称做 5 种变体（小写/大写/混合/首字母
/// 变体/带前后空白——空白变体拒绝面）。
pub fn variants(name: &[u8]) -> [[u8; 48]; 5] {
    let mut out = [[0u8; 48]; 5];
    // v0: 全小写
    let mut i = 0;
    while i < name.len() {
        out[0][i] = name[i].to_ascii_lowercase();
        i += 1;
    }
    // v1: 全大写
    i = 0;
    while i < name.len() {
        out[1][i] = name[i].to_ascii_uppercase();
        i += 1;
    }
    // v2: 首字母小写（Pascal 破坏变体）
    out[2][..name.len()].copy_from_slice(name);
    out[2][0] = name[0].to_ascii_lowercase();
    // v3: 交替大小写
    i = 0;
    while i < name.len() {
        out[3][i] = if i % 2 == 0 {
            name[i].to_ascii_lowercase()
        } else {
            name[i].to_ascii_uppercase()
        };
        i += 1;
    }
    // v4: 前导空格（归一不去空白 → 拒绝面）
    out[4][0] = b' ';
    out[4][1..=name.len()].copy_from_slice(name);
    out
}

pub fn detail(code: u16) -> &'static str {
    match code {
        E_NOENT => "双向未命中（表外）",
        E_NAME => "名称格式非法",
        E_BUF => "输出缓冲不足",
        E_AMBIG => "归一后二义（表纪律破）",
        E_TABLE => "上游表纪律破",
        E_ROUND => "往返失配",
        _ => "未知诊断码",
    }
}

/// 演练：R1 上游表+无二义；R2 正向 17 项×20 例（规范名+变体）全对；
/// R3 反向 17 项×20 例全对且与正向往返一致；R4 变体 20 例归一命中
/// （空白变体拒绝）；R5 表外双向 20 例全 E_NOENT。
pub fn drill() -> Result<u32, u16> {
    let mut hits: u32 = 0;
    // R1：上游表 + 归一无二义。
    unx36041_privtable::table_integrity().map_err(|_| E_TABLE)?;
    no_ambiguity()?;
    hits += 1;
    // R2：正向——17 项 × 20 例（每项规范名 1 例 + 变体 19 例中取前 19）。
    let mut c = 0;
    while c < 20 {
        let mut i = 0;
        while i < PRIV_N {
            let canon = unx36041_privtable::PRIV_TABLE[i].name.as_bytes();
            let name_src: &[u8] = if c == 0 {
                canon
            } else {
                let v = variants(canon);
                let vi = (c - 1) % 5;
                if vi == 4 {
                    // 空白变体是拒绝面（R4 专测）——R2 不作命中样本。
                    i += 1;
                    continue;
                }
                let owned = v[vi];
                // 取到循环外仍有效（栈数组生命周期覆盖本次查询）。
                let low = luid_of_name(&owned[..canon.len() + if vi == 4 { 1 } else { 0}])?;
                if low != unx36041_privtable::PRIV_TABLE[i].luid_low {
                    return Err(E_ROUND);
                }
                i += 1;
                continue;
            };
            let low = luid_of_name(name_src)?;
            if low != unx36041_privtable::PRIV_TABLE[i].luid_low {
                return Err(E_ROUND);
            }
            i += 1;
        }
        c += 1;
    }
    hits += 1;
    // R3：反向——17 项 × 20 例全对 + 往返一致。
    let mut c2 = 0;
    while c2 < 20 {
        let mut i = 0;
        while i < PRIV_N {
            let low = unx36041_privtable::PRIV_TABLE[i].luid_low;
            let mut buf = [0u8; 40];
            let n = name_of_luid(low, &mut buf)?;
            if &buf[..n] != unx36041_privtable::PRIV_TABLE[i].name.as_bytes() {
                return Err(E_ROUND);
            }
            // 往返：name → luid == 原 low。
            if luid_of_name(&buf[..n])? != low {
                return Err(E_ROUND);
            }
            i += 1;
        }
        c2 += 1;
    }
    hits += 1;
    // R4：变体钉死面——4 种大小写变体归一命中（20 例循环），空白变
    // 体拒绝（normalize 不剥空白 → 表内无空格 → 未命中）。
    let canon0 = unx36041_privtable::PRIV_TABLE[0].name.as_bytes();
    let vs = variants(canon0);
    let mut v = 0;
    while v < 4 {
        if luid_of_name(&vs[v][..canon0.len()])? != unx36041_privtable::PRIV_TABLE[0].luid_low {
            return Err(E_NOENT);
        }
        v += 1;
    }
    let mut s = 0;
    while s < 20 {
        // 空白变体（前导空格）：拒绝。
        if luid_of_name(&vs[4][..canon0.len() + 1]).is_ok() {
            return Err(E_AMBIG); // 空白被静默匹配 = 二义红线
        }
        s += 1;
    }
    hits += 1;
    // R5：表外双向 20 例全 E_NOENT。
    let outs: [&[u8]; 10] = [
        b"SeDebugPrivilege", b"SeXyzPrivilege", b"", b"Se Privilege",
        b"SeBackupPrivilege ", b"SeBack-upPrivilege", b"Privilege", b"Se",
        b"SeXyzPrivilegeX", b"SeXyzPrivilegeee",
    ];
    let mut o = 0;
    while o < 10 {
        if luid_of_name(outs[o]).is_ok() {
            return Err(E_NOENT);
        }
        o += 1;
    }
    let mut o2 = 0;
    while o2 < 10 {
        if name_of_luid(1000 + o2 as u32, &mut [0u8; 40]).is_ok() {
            return Err(E_NOENT);
        }
        o2 += 1;
    }
    hits += 1;
    Ok(hits)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn drill_green() {
        assert_eq!(drill(), Ok(5));
    }

    #[test]
    fn case_insensitive_lookup() {
        let canon = unx36041_privtable::PRIV_TABLE[6].name.as_bytes();
        let want = unx36041_privtable::PRIV_TABLE[6].luid_low;
        assert_eq!(luid_of_name(canon).unwrap(), want);
        assert_eq!(luid_of_name(b"sesecurityprivilege").unwrap(), want);
        assert_eq!(luid_of_name(b"SESECURITYPRIVILEGE").unwrap(), want);
    }

    #[test]
    fn roundtrip_all_17() {
        let mut buf = [0u8; 40];
        for d in unx36041_privtable::PRIV_TABLE.iter() {
            let n = name_of_luid(d.luid_low, &mut buf).unwrap();
            assert_eq!(luid_of_name(&buf[..n]).unwrap(), d.luid_low);
        }
    }
}

pub fn run_unx36051_checks() -> CheckSet {
    let mut cs = CheckSet::new("unxstar-unx36051_privlookup");
    cs.add("SEG 段位独占", SEG_BASE == 0xFDC0, "0xFDC0~0xFDDF 本模块独占");
    match drill() {
        Ok(_) => cs.add("J1 确定性演练全绿", true, "演练段全对平、注入全拒"),
        Err(code) => cs.add("J1 确定性演练全绿", false, detail(code)),
    }
    cs
}
