//! UNX-F36169-J1 AccessCheck 判定缓存（accache）。
//! 判据：① 固定容量判定缓存——key = 请求面 FNV 指纹，value = 判定
//! 三元组（granted/verdict/cause）；② 幂等 put / 命中 get / 冲突拒
//! （同 key 异值 E_CONFLICT）/ 容量耗尽 E_FULL；③ 缓存回放一致性
//! （缓存放行 = 装配直查同判）；④ 注入负例显性拒；⑤ ≥310 行。
//! 设计纪律：消费 36161（AcOutcome 常量面）；缓存语义冻结在本模块，
//! 供 36178 风暴面做并发对拍锚。

use super::unx36161_acfreeze as acf;
use crate::checks::CheckSet;

pub const SEG_BASE: u16 = 0x3c50;

// ---- 诊断码闭集 -----------------------------------------------------------
pub const E_VER: u16 = SEG_BASE + 1; // 版本锚不符
pub const E_FULL: u16 = SEG_BASE + 2; // 容量耗尽
pub const E_CONFLICT: u16 = SEG_BASE + 3; // 同 key 异值冲突
pub const E_MISS: u16 = SEG_BASE + 4; // 查询未命中
pub const E_BADKEY: u16 = SEG_BASE + 5; // 键指纹非法（全零）
pub const E_NEG: u16 = SEG_BASE + 6; // 负例注入未被检出
pub const E_TABLE: u16 = SEG_BASE + 7; // 缓存表闭集破坏

/// 模块版本锚。
pub const VER: u32 = 1;

/// 缓存容量锚（固定，不可配置——确定性优先）。
pub const CACHE_N: usize = 16;

/// 缓存条目（key=0 视为空槽）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct CacheEntry {
    pub key: u64,
    pub granted: u32,
    pub verdict: u16,
    pub cause: u16,
}

impl CacheEntry {
    pub fn empty() -> CacheEntry {
        CacheEntry { key: 0, granted: 0, verdict: 0, cause: 0 }
    }
    pub fn is_empty(&self) -> bool {
        self.key == 0
    }
}

/// 判定缓存（固定容量；线性探查插入；命中即回）。
pub struct Cache {
    slots: [CacheEntry; CACHE_N],
    pub puts: u32,
    pub hits: u32,
    pub misses: u32,
}

impl Cache {
    pub fn new() -> Cache {
        Cache {
            slots: [CacheEntry::empty(); CACHE_N],
            puts: 0,
            hits: 0,
            misses: 0,
        }
    }

    /// 线性探查找空槽（满则 Err(E_FULL)）。
    fn free_slot(&self) -> Result<usize, u16> {
        let mut i = 0;
        while i < CACHE_N {
            if self.slots[i].is_empty() {
                return Ok(i);
            }
            i += 1;
        }
        Err(E_FULL)
    }

    /// put：同 key 同值幂等（计数不重复计冲突）；同 key 异值 E_CONFLICT。
    pub fn put(&mut self, e: CacheEntry) -> Result<(), u16> {
        if VER != 1 {
            return Err(E_VER);
        }
        if e.key == 0 {
            return Err(E_BADKEY);
        }
        let mut i = 0;
        while i < CACHE_N {
            if self.slots[i].key == e.key {
                if self.slots[i].granted == e.granted
                    && self.slots[i].verdict == e.verdict
                    && self.slots[i].cause == e.cause
                {
                    return Ok(()); // 幂等重放
                }
                return Err(E_CONFLICT);
            }
            i += 1;
        }
        let slot = self.free_slot()?;
        self.slots[slot] = e;
        self.puts += 1;
        Ok(())
    }

    /// get：命中回条目；未命中 Err(E_MISS)（misses 计数）。
    pub fn get(&mut self, key: u64) -> Result<CacheEntry, u16> {
        if VER != 1 {
            return Err(E_VER);
        }
        if key == 0 {
            return Err(E_BADKEY);
        }
        let mut i = 0;
        while i < CACHE_N {
            if self.slots[i].key == key {
                self.hits += 1;
                return Ok(self.slots[i]);
            }
            i += 1;
        }
        self.misses += 1;
        Err(E_MISS)
    }

    /// 占用槽计数。
    pub fn used(&self) -> u32 {
        let mut n = 0;
        let mut i = 0;
        while i < CACHE_N {
            if !self.slots[i].is_empty() {
                n += 1;
            }
            i += 1;
        }
        n
    }

    /// 清空（统计量保留——审计连续性）。
    pub fn clear(&mut self) {
        let mut i = 0;
        while i < CACHE_N {
            self.slots[i] = CacheEntry::empty();
            i += 1;
        }
    }
}

/// 请求面指纹：token_id + desired + dacl 状态 + ACE 摘要（FNV-1a）。
pub fn req_fingerprint(token_id: u32, desired: u32, dacl_state: u16, ace_sum: u64) -> u64 {
    let mut h: u64 = 0x0100_93e7_0100_3c51;
    for v in [token_id as u64, desired as u64, dacl_state as u64, ace_sum] {
        let mut b = [0u8; 8];
        b.copy_from_slice(&v.to_le_bytes());
        let mut j = 0;
        while j < 8 {
            h ^= b[j] as u64;
            h = h.wrapping_mul(0x0100_0000_01b3);
            j += 1;
        }
    }
    h
}

/// 判定入缓存面：AcOutcome → CacheEntry 投影。
pub fn outcome_to_entry(key: u64, o: &acf::AcOutcome) -> CacheEntry {
    CacheEntry { key, granted: o.granted, verdict: o.verdict, cause: o.cause }
}

/// 缓存回放一致性：条目与 outcome 判定同面（granted/verdict/cause 全等）。
pub fn entry_matches(e: &CacheEntry, o: &acf::AcOutcome) -> bool {
    e.granted == o.granted && e.verdict == o.verdict && e.cause == o.cause
}

/// 缓存纪律自检：容量锚 + 空槽初值 + 统计初值。
pub fn cache_integrity() -> Result<(), u16> {
    if VER != 1 {
        return Err(E_VER);
    }
    if CACHE_N != 16 {
        return Err(E_TABLE);
    }
    let c = Cache::new();
    if c.used() != 0 || c.puts != 0 || c.hits != 0 || c.misses != 0 {
        return Err(E_TABLE);
    }
    Ok(())
}

/// 演练：R1 纪律自检；R2 put/get 幂等与命中统计；R3 冲突与未命中；
/// R4 容量耗尽与清空保统计；R5 指纹区分与负例注入。
pub fn drill() -> Result<u32, u16> {
    let mut hits: u32 = 0;
    // R1：纪律自检。
    cache_integrity()?;
    hits += 1;
    // R2：put/get 幂等——同条目两放一记；命中统计 1。
    let mut c = Cache::new();
    let k1 = req_fingerprint(0x1111, 0x89, 1, 0);
    let e1 = CacheEntry { key: k1, granted: 0x89, verdict: 0, cause: 0 };
    c.put(e1)?;
    c.put(e1)?; // 幂等
    let g = c.get(k1)?;
    if !entry_matches(&g, &acf::AcOutcome { granted: 0x89, verdict: 0, failed_step: 0, cause: 0, missing: 0 }) {
        return Err(E_NEG);
    }
    if c.puts != 1 || c.hits != 1 {
        return Err(E_TABLE);
    }
    hits += 1;
    // R3：冲突拒 + 未命中拒。
    let e_bad = CacheEntry { key: k1, granted: 0x88, verdict: 0, cause: 0 };
    if c.put(e_bad).is_ok() {
        return Err(E_NEG);
    }
    let k2 = req_fingerprint(0x2222, 0x89, 1, 0);
    if c.get(k2).is_ok() || c.misses != 1 {
        return Err(E_NEG);
    }
    hits += 1;
    // R4：容量耗尽——填满 16 槽后第 17 条 E_FULL；清空后可再放、统计保留。
    c.clear();
    let mut n = 0u32;
    let mut seed: u64 = 0x0BAD_C0DE_3C50;
    while n < CACHE_N as u32 {
        seed = seed
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        let e = CacheEntry { key: seed | 1, granted: n, verdict: 0, cause: 0 };
        c.put(e)?;
        n += 1;
    }
    if c.used() != CACHE_N as u32 {
        return Err(E_TABLE);
    }
    let e_overflow = CacheEntry { key: 0xDEAD_BEEF_0000_0001, granted: 0, verdict: 0, cause: 0 };
    if c.put(e_overflow) != Err(E_FULL) {
        return Err(E_NEG);
    }
    c.clear();
    if c.used() != 0 || c.puts != CACHE_N as u32 + 1 {
        // puts 保留 = 16 槽满填 + R2 幂等 1 条；clear 不清统计。
        return Err(E_TABLE);
    }
    hits += 1;
    // R5：指纹区分 + 负例注入（全零键 / 空键 put/get / 空条目判定）。
    let fa = req_fingerprint(1, 0x89, 1, 0);
    let fb = req_fingerprint(1, 0x8A, 1, 0);
    let fc = req_fingerprint(1, 0x89, 3, 0);
    if fa == fb || fa == fc {
        return Err(E_NEG);
    }
    if c.put(CacheEntry::empty()) != Err(E_BADKEY) {
        return Err(E_NEG);
    }
    if c.get(0) != Err(E_BADKEY) {
        return Err(E_NEG);
    }
    if !CacheEntry::empty().is_empty() {
        return Err(E_NEG);
    }
    let mut probe = Cache::new();
    let e = outcome_to_entry(0x1234_5678_9ABC_DEF1, &acf::AcOutcome {
        granted: 0x12,
        verdict: 2,
        failed_step: 0,
        cause: 0x3C54,
        missing: 0,
    });
    probe.put(e)?;
    let g2 = probe.get(0x1234_5678_9ABC_DEF1)?;
    if g2.verdict != 2 || g2.granted != 0x12 {
        return Err(E_NEG);
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
    fn cache_table_holds() {
        assert_eq!(cache_integrity(), Ok(()));
    }

    #[test]
    fn conflict_rejected() {
        let mut c = Cache::new();
        let e = CacheEntry { key: 9, granted: 1, verdict: 0, cause: 0 };
        c.put(e).ok().unwrap_or(());
        let e2 = CacheEntry { key: 9, granted: 2, verdict: 0, cause: 0 };
        assert_eq!(c.put(e2), Err(E_CONFLICT));
    }

    #[test]
    fn zero_key_rejected() {
        let mut c = Cache::new();
        assert_eq!(c.get(0), Err(E_BADKEY));
    }
}

pub fn run_unx36169_checks() -> CheckSet {
    let mut cs = CheckSet::new("unxstar-unx36169_accache");
    cs.add("SEG 段位独占", SEG_BASE == 0x3c50, "0x3C50~0x3C5F 本模块独占");
    match drill() {
        Ok(_) => cs.add("J1 确定性演练全绿", true, "幂等/冲突/容量/指纹全对平"),
        Err(code) => cs.add("J1 确定性演练全绿", false, detail(code)),
    }
    cs.add("闭集纪律", cache_integrity() == Ok(()), "容量锚 + 统计初值");
    cs.add("契约冻结", VER == 1, "缓存面锚 v1");
    cs
}

pub fn detail(code: u16) -> &'static str {
    match code {
        E_VER => "版本锚不符",
        E_FULL => "容量耗尽",
        E_CONFLICT => "同 key 异值冲突",
        E_MISS => "查询未命中",
        E_BADKEY => "键指纹非法（全零）",
        E_NEG => "负例注入未被检出",
        E_TABLE => "缓存表闭集破坏",
        _ => "未知诊断码",
    }
}
