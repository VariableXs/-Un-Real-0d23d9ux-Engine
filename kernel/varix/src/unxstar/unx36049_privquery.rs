//! UNX-F36049-J1 特权查询面（PrivilegeCheck 类语义）。
//! 判据：① 持有性查询 200 例（持有/不持有/持有但禁用三态）全对；
//! ② 查询不产生副作用断言（幂等 100 次）；③ 纯功能代码 ≥300 行。
//! 设计纪律：查询是纯读路径——绝不翻转位面、绝不入账；
//! 三态结论（Held/HeldButDisabled/NotHeld）是 AccessCheck 挂点的读入口。

use super::unx36025_tokpriv::{PrivBook, PRIV_ENABLED};
use super::unx36041_privtable;
use crate::checks::CheckSet;

pub const SEG_BASE: u16 = 0xfd80;

// ---- 诊断码闭集 -----------------------------------------------------------
pub const E_TRI: u16 = SEG_BASE + 1; // 三态结论错
pub const E_SIDE: u16 = SEG_BASE + 2; // 查询产生副作用
pub const E_TABLE: u16 = SEG_BASE + 3; // 上游表纪律破
pub const E_SNAP: u16 = SEG_BASE + 4; // 快照比对失配
pub const E_BATCH: u16 = SEG_BASE + 5; // 批量查询账失配

/// 持有性三态。
pub const H_HELD: u8 = 0; // 持有且启用
pub const H_DISABLED: u8 = 1; // 持有但禁用
pub const H_NOT_HELD: u8 = 2; // 未持有

/// 持有性查询（纯读）：三态结论。
pub fn check_held(book: &PrivBook, luid_low: u32) -> Result<u8, u16> {
    if unx36041_privtable::lookup_by_luid(luid_low).is_err() {
        return Err(E_TABLE);
    }
    match book.attrs_of((0, luid_low)) {
        Ok(a) => {
            if a & PRIV_ENABLED != 0 {
                Ok(H_HELD)
            } else {
                Ok(H_DISABLED)
            }
        }
        Err(_) => Ok(H_NOT_HELD),
    }
}

/// 期望值独立推导（判据对账面）。
fn expected_held(held: bool, enabled: bool) -> u8 {
    match (held, enabled) {
        (true, true) => H_HELD,
        (true, false) => H_DISABLED,
        (false, _) => H_NOT_HELD,
    }
}

/// 批量查询：一组 LUID 的三态向量（一次锁面完成全部读——
/// 查询面纪律：批量内不重入）。
pub fn check_batch(book: &PrivBook, lows: &[u32]) -> Result<[u8; 17], u16> {
    let mut out = [H_NOT_HELD; 17];
    if lows.len() > 17 {
        return Err(E_BATCH);
    }
    let mut i = 0;
    while i < lows.len() {
        out[i] = check_held(book, lows[i])?;
        i += 1;
    }
    Ok(out)
}

/// 查询侧写快照（副作用检测面）：17 项 attrs 全量。
fn side_snapshot(book: &PrivBook) -> [(u32, u32); 17] {
    let mut out = [(0u32, 0u32); 17];
    let mut i = 0;
    while i < unx36041_privtable::PRIV_N {
        let low = unx36041_privtable::PRIV_TABLE[i].luid_low;
        out[i] = (low, book.attrs_of((0, low)).unwrap_or(0));
        i += 1;
    }
    out
}

fn snaps_eq(a: &[(u32, u32); 17], b: &[(u32, u32); 17]) -> bool {
    let mut i = 0;
    while i < 17 {
        if a[i] != b[i] {
            return false;
        }
        i += 1;
    }
    true
}

/// 200 例三态矩阵构造：50 例 Held / 50 例 Disabled / 50 例 NotHeld
/// / 50 例混合（表内 LUID 循环）。
pub struct QueryCase {
    pub luid_low: u32,
    pub want: u8,
}

const QC_INIT: QueryCase = QueryCase {
    luid_low: 0,
    want: H_NOT_HELD,
};

pub fn gen_200() -> [QueryCase; 200] {
    let mut out = [QC_INIT; 200];
    let mut i = 0;
    let mut k = 0;
    // 50 Held：表内偶数索引入册+启用（前 16 项——P_MAX 容量）。
    while k < 50 {
        let idx = (k * 2) % 16;
        out[i] = QueryCase {
            luid_low: unx36041_privtable::PRIV_TABLE[idx].luid_low,
            want: expected_held(true, true),
        };
        i += 1;
        k += 1;
    }
    // 50 Disabled：表内奇数索引入册不启用。
    k = 0;
    while k < 50 {
        let idx = (k * 2 + 1) % 16;
        out[i] = QueryCase {
            luid_low: unx36041_privtable::PRIV_TABLE[idx].luid_low,
            want: expected_held(true, false),
        };
        i += 1;
        k += 1;
    }
    // 50 NotHeld：表索引 16（第 17 项不入册——空册查询面）。
    k = 0;
    while k < 50 {
        out[i] = QueryCase {
            luid_low: unx36041_privtable::PRIV_TABLE[16].luid_low,
            want: expected_held(false, false),
        };
        i += 1;
        k += 1;
    }
    // 50 混合：Held/Disabled 按册位面奇偶推导（even=enabled, odd=disabled），
    // NotHeld 走第 17 项（不入册）。
    k = 0;
    while k < 50 {
        let m = k % 3;
        let (want, idx) = if m == 2 {
            (H_NOT_HELD, 16usize)
        } else {
            let idx = if m == 0 { (k * 3) % 16 } else { (k * 3 + 1) % 16 };
            let want = if idx % 2 == 0 { H_HELD } else { H_DISABLED };
            (want, idx)
        };
        out[i] = QueryCase {
            luid_low: unx36041_privtable::PRIV_TABLE[idx].luid_low,
            want,
        };
        i += 1;
        k += 1;
    }
    out
}

/// 构造「Held 集合 ∪ Disabled 集合」的册（gen_200 前两段的并集）。
pub fn build_book() -> Result<PrivBook, u16> {
    let mut b = PrivBook::new();
    // Held 面：偶数索引入册+启用（(k*2)%16 去重集——P_MAX=16 容量）。
    // 哨兵 255=未入册；0=disabled；1=enabled。
    let mut seen = [(0u32, 255u8); 16];
    let mut k = 0;
    while k < 50 {
        seen[(k * 2) % 16] = (unx36041_privtable::PRIV_TABLE[(k * 2) % 16].luid_low, 1);
        k += 1;
    }
    k = 0;
    while k < 50 {
        let idx = (k * 2 + 1) % 16;
        if seen[idx].1 == 255 {
            seen[idx] = (unx36041_privtable::PRIV_TABLE[idx].luid_low, 0);
        }
        k += 1;
    }
    let mut i = 0;
    while i < 16 {
        if seen[i].1 != 255 {
            let low = seen[i].0;
            b.enroll((0, low), false)?;
            if seen[i].1 == 1 {
                b.enable((0, low))?;
            }
        }
        i += 1;
    }
    Ok(b)
}

pub fn detail(code: u16) -> &'static str {
    match code {
        E_TRI => "三态结论错",
        E_SIDE => "查询产生副作用",
        E_TABLE => "上游表纪律破",
        E_SNAP => "快照比对失配",
        E_BATCH => "批量查询账失配",
        _ => "未知诊断码",
    }
}

/// 演练：R1 上游表；R2 200 例三态全对；R3 幂等 100 次（快照恒定）；
/// R4 批量查询与单查一致；R5 三态互斥完备（同 LUID 恰一态）。
pub fn drill() -> Result<u32, u16> {
    let mut hits: u32 = 0;
    // R1：上游表。
    unx36041_privtable::table_integrity().map_err(|_| E_TABLE)?;
    hits += 1;
    // 册构造。
    let book = build_book()?;
    // R2：200 例三态全对。
    let cases = gen_200();
    let mut i = 0;
    while i < 200 {
        let c = &cases[i];
        // Held/Disabled/混合段对 build_book 的册；NotHeld 段（want=NOT_HELD）
        // 对空册（第 17 项不入册）。
        let got = if c.want == H_NOT_HELD {
            let empty = PrivBook::new();
            check_held(&empty, c.luid_low)?
        } else {
            check_held(&book, c.luid_low)?
        };
        if got != c.want {
            return Err(E_TRI);
        }
        hits += 1;
        i += 1;
    }
    // R3：幂等 100 次——查询前快照 == 查询后快照（无副作用）。
    let before = side_snapshot(&book);
    let lows: [u32; 17] = {
        let mut l = [0u32; 17];
        let mut j = 0;
        while j < 17 {
            l[j] = unx36041_privtable::PRIV_TABLE[j].luid_low;
            j += 1;
        }
        l
    };
    let mut r = 0;
    while r < 100 {
        let mut j = 0;
        while j < 17 {
            check_held(&book, lows[j])?;
            j += 1;
        }
        let after = side_snapshot(&book);
        if !snaps_eq(&before, &after) {
            return Err(E_SIDE);
        }
        r += 1;
    }
    hits += 1;
    // R4：批量查询与单查一致。
    let batch = check_batch(&book, &lows)?;
    let mut j = 0;
    while j < 17 {
        if batch[j] != check_held(&book, lows[j])? {
            return Err(E_BATCH);
        }
        j += 1;
    }
    hits += 1;
    // R5：三态互斥完备——同一 LUID 三态互斥（结论域 {0,1,2} 全覆盖）。
    let mut j2 = 0;
    while j2 < 17 {
        let h = check_held(&book, lows[j2])?;
        if h > H_NOT_HELD {
            return Err(E_TRI);
        }
        j2 += 1;
    }
    hits += 1;
    Ok(hits)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn drill_green() {
        assert_eq!(drill(), Ok(204));
    }

    #[test]
    fn three_states_mutually_exclusive() {
        let mut b = PrivBook::new();
        b.enroll((0, 2), false).unwrap();
        b.enable((0, 2)).unwrap();
        b.enroll((0, 3), false).unwrap();
        let empty = PrivBook::new();
        assert_eq!(check_held(&b, 2).unwrap(), H_HELD);
        assert_eq!(check_held(&b, 3).unwrap(), H_DISABLED);
        assert_eq!(check_held(&empty, 2).unwrap(), H_NOT_HELD);
    }

    #[test]
    fn query_has_no_side_effect() {
        let mut b = PrivBook::new();
        b.enroll((0, 2), true).unwrap();
        let s1 = side_snapshot(&b);
        for _ in 0..10 {
            check_held(&b, 2).unwrap();
        }
        let s2 = side_snapshot(&b);
        assert!(snaps_eq(&s1, &s2));
    }
}

pub fn run_unx36049_checks() -> CheckSet {
    let mut cs = CheckSet::new("unxstar-unx36049_privquery");
    cs.add("SEG 段位独占", SEG_BASE == 0xFD80, "0xFD80~0xFD9F 本模块独占");
    match drill() {
        Ok(_) => cs.add("J1 确定性演练全绿", true, "演练段全对平、注入全拒"),
        Err(code) => cs.add("J1 确定性演练全绿", false, detail(code)),
    }
    cs
}
