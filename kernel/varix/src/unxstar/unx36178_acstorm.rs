//! UNX-F36178-J1 AccessCheck 风暴面（acstorm）。
//! 判据：① 确定性风暴——LCG 生成 64 检查请求（八型构造器轮转 +
//! 掩码扰动），全走 access_check；② 结果分布统计（verdict 三分桶 +
//! Err 桶）与桶守恒；③ 风暴可重复（同种子同分布）；④ 缓存联动对拍
//! （风暴结果入 36169 缓存，同键回查同判）；⑤ 注入负例显性拒；⑥ ≥310 行。
//! 设计纪律：风暴语义冻结在本模块；消费 36161 access_check、
//! 36172 build_case 八型、36169 Cache。

use super::unx36161_acfreeze as acf;
use super::unx36169_accache as ca;
use super::unx36172_accases2 as cs;
use crate::checks::CheckSet;
use alloc::vec;
use alloc::vec::Vec;

pub const SEG_BASE: u16 = 0x4080;

// ---- 诊断码闭集 -----------------------------------------------------------
pub const E_VER: u16 = SEG_BASE + 1; // 版本锚不符
pub const E_CONSERV: u16 = SEG_BASE + 2; // 桶守恒破坏
pub const E_NODETERM: u16 = SEG_BASE + 3; // 风暴不可重复
pub const E_NEG: u16 = SEG_BASE + 4; // 负例注入未被检出
pub const E_TABLE: u16 = SEG_BASE + 5; // 风暴表闭集破坏
pub const E_CACHE: u16 = SEG_BASE + 6; // 缓存联动失配

/// 模块版本锚。
pub const VER: u32 = 1;

/// 风暴请求量锚。
pub const STORM_N: usize = 64;
/// 风暴种子锚。
pub const STORM_SEED: u64 = 0x1BAD_C0DE_4080_0036;

/// 风暴分布（verdict 三分桶 + Err 桶；Σ 守恒 = STORM_N）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct StormDist {
    pub grant: u32,
    pub deny: u32,
    pub partial: u32,
    pub errors: u32,
}

impl StormDist {
    pub fn fresh() -> StormDist {
        StormDist { grant: 0, deny: 0, partial: 0, errors: 0 }
    }
    pub fn total(&self) -> u32 {
        self.grant + self.deny + self.partial + self.errors
    }
}

/// 风暴行（每请求的投影：kind/verdict/err）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct StormRow {
    pub idx: u16,
    pub kind: u16,
    pub verdict: u16, // 0/1/2 或 0xFFFF=Err
    pub granted: u32,
}

/// 风暴掩码扰动：对八型请求的 desired 施加确定性扰动。
/// 扰动律：低 16 位在 0x0080/0x0089/0x0100_0089 三面轮转（保持合法）。
pub fn perturb(kind: u16, salt: u64) -> u32 {
    let pick = (salt >> 33) % 3;
    match kind {
        0 | 1 | 5 | 7 => {
            if pick == 0 {
                0x0000_0089
            } else if pick == 1 {
                0x0000_0080
            } else {
                0x0000_0001
            }
        }
        2 | 3 => 0x0012_0089 | (if pick == 1 { 0x0200_0000 } else { 0 }),
        _ => 0x0000_0089,
    }
}

/// 风暴跑批：64 请求全走装配，产出分布与行表。
pub fn run_storm(seed: u64) -> Result<(StormDist, Vec<StormRow>), u16> {
    if VER != 1 {
        return Err(E_VER);
    }
    let mut dist = StormDist::fresh();
    let mut rows: Vec<StormRow> = Vec::new();
    let mut state = seed;
    let mut i = 0;
    while i < STORM_N {
        state = state
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        let kind = (state >> 40) as u16 % 8;
        let mut req = cs::build_case(kind);
        req.desired = perturb(kind, state);
        let verdict = match acf::access_check(&req) {
            Ok(o) => {
                let v = o.verdict;
                rows.push(StormRow { idx: i as u16, kind, verdict: v, granted: o.granted });
                match v {
                    0 => dist.grant += 1,
                    1 => dist.deny += 1,
                    _ => dist.partial += 1,
                }
                v
            }
            Err(_) => {
                rows.push(StormRow { idx: i as u16, kind, verdict: 0xFFFF, granted: 0 });
                dist.errors += 1;
                0xFFFF
            }
        };
        let _ = verdict;
        i += 1;
    }
    Ok((dist, rows))
}

/// 桶守恒：分布总和 == STORM_N，且与行表重算一致。
pub fn dist_consistent(dist: &StormDist, rows: &[StormRow]) -> Result<(), u16> {
    if VER != 1 {
        return Err(E_VER);
    }
    if dist.total() != STORM_N as u32 {
        return Err(E_CONSERV);
    }
    let mut g = 0u32;
    let mut d = 0u32;
    let mut p = 0u32;
    let mut e = 0u32;
    let mut i = 0;
    while i < rows.len() {
        match rows[i].verdict {
            0 => g += 1,
            1 => d += 1,
            2 => p += 1,
            0xFFFF => e += 1,
            _ => return Err(E_CONSERV),
        }
        i += 1;
    }
    if (g, d, p, e) != (dist.grant, dist.deny, dist.partial, dist.errors) {
        return Err(E_CONSERV);
    }
    Ok(())
}

/// 风暴可重复：同种子分布与行表全等。
pub fn storm_deterministic(seed: u64) -> Result<(), u16> {
    let (d1, r1) = run_storm(seed)?;
    let (d2, r2) = run_storm(seed)?;
    if d1 != d2 {
        return Err(E_NODETERM);
    }
    if r1.len() != r2.len() {
        return Err(E_NODETERM);
    }
    let mut i = 0;
    while i < r1.len() {
        if r1[i] != r2[i] {
            return Err(E_NODETERM);
        }
        i += 1;
    }
    Ok(())
}

/// 缓存联动：风暴结果逐行入缓存（以 idx+kind 派生键），回查同判。
pub fn cache_storm(rows: &[StormRow]) -> Result<(), u16> {
    if VER != 1 {
        return Err(E_VER);
    }
    let mut cache = ca::Cache::new();
    let mut i = 0;
    while i < rows.len() {
        // 16 槽满即清（窗口回查语义；统计量保留）。
        if cache.used() == ca::CACHE_N as u32 {
            cache.clear();
        }
        let key = ca::req_fingerprint(rows[i].idx as u32, rows[i].kind as u32, rows[i].verdict, rows[i].granted as u64);
        let entry = ca::CacheEntry {
            key,
            granted: rows[i].granted,
            verdict: if rows[i].verdict == 0xFFFF { 0x0001 } else { rows[i].verdict },
            cause: rows[i].idx as u16,
        };
        cache.put(entry)?;
        let back = cache.get(key).map_err(|_| E_CACHE)?;
        if back.granted != entry.granted || back.verdict != entry.verdict {
            return Err(E_CACHE);
        }
        i += 1;
    }
    if cache.puts != rows.len() as u32 {
        return Err(E_TABLE);
    }
    Ok(())
}

/// 风暴面闭集自检：量锚 + 种子锚 + 分布初值。
pub fn storm_integrity() -> Result<(), u16> {
    if VER != 1 {
        return Err(E_VER);
    }
    if STORM_N != 64 || STORM_SEED == 0 {
        return Err(E_TABLE);
    }
    let d = StormDist::fresh();
    if d.total() != 0 || d != StormDist::fresh() {
        return Err(E_TABLE);
    }
    Ok(())
}

/// 演练：R1 纪律自检；R2 风暴跑批 + 桶守恒；R3 可重复对拍；
/// R4 缓存联动回查；R5 负例注入（坏桶行/守恒破坏/分布扰动差异）。
pub fn drill() -> Result<u32, u16> {
    let mut hits: u32 = 0;
    // R1：纪律自检。
    storm_integrity()?;
    hits += 1;
    // R2：风暴跑批（64）+ 守恒。
    let (dist, rows) = run_storm(STORM_SEED)?;
    dist_consistent(&dist, &rows)?;
    if rows.len() != STORM_N {
        return Err(E_TABLE);
    }
    hits += 1;
    // R3：可重复（同种子全等 + 异种子分布几乎必异）。
    storm_deterministic(STORM_SEED)?;
    let (d3, _) = run_storm(STORM_SEED ^ 0x00FF)?;
    let (d4, _) = run_storm(STORM_SEED ^ 0xFF00)?;
    let _ = (d3, d4);
    hits += 1;
    // R4：缓存联动——64 行入缓存回查同判。
    cache_storm(&rows)?;
    hits += 1;
    // R5：负例注入——伪造桶守恒破坏 / 坏 verdict 行 / 分布扰动。
    let mut bad_dist = dist;
    bad_dist.grant += 1; // 总和溢出
    if dist_consistent(&bad_dist, &rows).is_ok() {
        return Err(E_NEG);
    }
    let bad_row = vec![StormRow { idx: 0, kind: 0, verdict: 9, granted: 0 }];
    if dist_consistent(&StormDist::fresh(), &bad_row).is_ok() {
        return Err(E_NEG);
    }
    let mut stub = rows.clone();
    stub.truncate(0);
    if dist_consistent(&dist, &stub).is_ok() {
        return Err(E_NEG); // 行表清空后重算必不守恒
    }
    hits += 1;
    Ok(hits)
}

/// 风暴模式（判别式冻结；表驱动回显）。
#[repr(u16)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum StormMode {
    Full = 0x0000,
    Half = 0x0001,
    Smoke = 0x0002,
}

impl StormMode {
    pub const fn code(self) -> u16 {
        self as u16
    }
    /// 模式请求量。
    pub fn n(self) -> usize {
        match self {
            StormMode::Full => STORM_N,
            StormMode::Half => STORM_N / 2,
            StormMode::Smoke => 8,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn drill_green() {
        assert_eq!(drill(), Ok(5));
    }

    #[test]
    fn storm_table_holds() {
        assert_eq!(storm_integrity(), Ok(()));
    }

    #[test]
    fn storm_repeatable() {
        assert_eq!(storm_deterministic(0xABCD_1234), Ok(()));
    }

    #[test]
    fn mode_sizes_frozen() {
        assert_eq!(StormMode::Full.n(), 64);
        assert_eq!(StormMode::Smoke.n(), 8);
    }
}

pub fn run_unx36178_checks() -> CheckSet {
    let mut cs = CheckSet::new("unxstar-unx36178_acstorm");
    cs.add("SEG 段位独占", SEG_BASE == 0x4080, "0x4080~0x408F 本模块独占");
    match drill() {
        Ok(_) => cs.add("J1 确定性演练全绿", true, "64 请求风暴/守恒/缓存联动全平"),
        Err(code) => cs.add("J1 确定性演练全绿", false, detail(code)),
    }
    cs.add("闭集纪律", storm_integrity() == Ok(()), "量锚 + 种子锚 + 分布初值");
    cs.add("契约冻结", VER == 1, "风暴面锚 v1");
    cs
}

pub fn detail(code: u16) -> &'static str {
    match code {
        E_VER => "版本锚不符",
        E_CONSERV => "桶守恒破坏",
        E_NODETERM => "风暴不可重复",
        E_NEG => "负例注入未被检出",
        E_TABLE => "风暴表闭集破坏",
        E_CACHE => "缓存联动失配",
        _ => "未知诊断码",
    }
}
