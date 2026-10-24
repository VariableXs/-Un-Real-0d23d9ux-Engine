//! VE-F4273 · 治理性能（U04 治理组）
//! 职责：治理面性能工程（查询 P95<100ms 目标 + 批量吞吐分档）+ 热点缓存
//! （版本戳失效重取）+ 三层归档（热/温/冷——冷层双写备份）+ 分片扩容。
//! 段位：0x2820（续表 idx507）。行数预算 300。

#![allow(dead_code)]

use alloc::vec;
use alloc::vec::Vec;
use crate::checks::CheckSet;

pub const SEG_BASE: u16 = 0x2820;

#[repr(u16)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum GpCode {
    /// 缓存失真（版本戳不符）。
    CacheStale = SEG_BASE,
    /// 归档误删（冷备恢复）。
    ArchiveLost = SEG_BASE + 0x01,
    /// 吞吐不达标。
    ThruLow = SEG_BASE + 0x02,
}

/// 三层归档。
#[repr(u8)]
#[derive(Clone, Copy, PartialEq, Eq, Debug, PartialOrd, Ord)]
pub enum Tier {
    Hot = 0,
    Warm = 1,
    Cold = 2,
}

/// 缓存条目（带版本戳）。
#[derive(Clone, Copy, Debug)]
pub struct CacheEntry {
    pub key: u64,
    pub val: u32,
    pub ver: u32,
}

/// 归档条目（冷层带双写副本）。
#[derive(Clone, Copy, Debug)]
pub struct ArchEntry {
    pub key: u64,
    pub tier: Tier,
    pub cold_backup: bool,
}

/// 治理性能器。
pub struct GovPerf {
    pub cache: Vec<CacheEntry>,
    pub arch: Vec<ArchEntry>,
    /// P95 目标 ms。
    pub p95_cap: u32,
    /// 吞吐分档（低/中/高 最小值）。
    pub thru_tiers: [u32; 3],
    pub p95_samples: Vec<u32>,
}

impl GovPerf {
    pub fn new(p95_cap: u32, thru_tiers: [u32; 3]) -> Self {
        GovPerf { cache: Vec::new(), arch: Vec::new(), p95_cap, thru_tiers, p95_samples: Vec::new() }
    }

    /// 缓存读取：版本戳不符→失效重取（返回 None 并清除）。
    pub fn cache_get(&mut self, key: u64, cur_ver: u32) -> Option<u32> {
        let idx = self.cache.iter().position(|c| c.key == key)?;
        if self.cache[idx].ver != cur_ver {
            self.cache.remove(idx);
            return None;
        }
        Some(self.cache[idx].val)
    }

    pub fn cache_put(&mut self, key: u64, val: u32, ver: u32) {
        match self.cache.iter_mut().find(|c| c.key == key) {
            Some(c) => {
                c.val = val;
                c.ver = ver;
            }
            None => self.cache.push(CacheEntry { key, val, ver }),
        }
    }

    /// 归档降层：热→温→冷（冷层强制双写）。
    pub fn demote(&mut self, key: u64, to: Tier) -> Result<(), GpCode> {
        let e = self.arch.iter_mut().find(|e| e.key == key).ok_or(GpCode::ArchiveLost)?;
        e.tier = to;
        if to == Tier::Cold && !e.cold_backup {
            e.cold_backup = true; // 冷层双写备份
        }
        Ok(())
    }

    /// 归档登记。
    pub fn arch_new(&mut self, key: u64) {
        if !self.arch.iter().any(|e| e.key == key) {
            self.arch.push(ArchEntry { key, tier: Tier::Hot, cold_backup: false });
        }
    }

    /// 误删恢复：冷备双写恢复检查。
    pub fn restore_ok(&self, key: u64) -> bool {
        self.arch.iter().any(|e| e.key == key && e.tier == Tier::Cold && e.cold_backup)
    }

    /// 查询 P95 核验。
    pub fn p95(&self) -> u32 {
        if self.p95_samples.is_empty() {
            return 0;
        }
        let mut s = self.p95_samples.clone();
        s.sort_unstable();
        let idx = (s.len() * 95 / 100).saturating_sub(1).min(s.len() - 1);
        s[idx]
    }

    pub fn sample(&mut self, ms: u32) {
        self.p95_samples.push(ms);
    }

    /// 吞吐分档：返回档位（0 低 1 中 2 高 / None 不达标）。
    pub fn thru_tier(&self, ops_per_min: u32) -> Option<usize> {
        if ops_per_min >= self.thru_tiers[2] {
            Some(2)
        } else if ops_per_min >= self.thru_tiers[1] {
            Some(1)
        } else if ops_per_min >= self.thru_tiers[0] {
            Some(0)
        } else {
            None
        }
    }

    /// 分片扩容：n 项 / p 片。
    pub fn shard(&self, n: u32, p: u32) -> u32 {
        if p == 0 {
            return 0;
        }
        (n + p - 1) / p
    }
}

fn chk(set: &mut CheckSet, label: &'static str, ok: bool, detail: &'static str) {
    set.add(label, ok, detail);
}

/// 判据组：C507-01..12。
pub fn run_ves507_checks() -> CheckSet {
    let mut set = CheckSet::new("ves507");

    let mut gp = GovPerf::new(100, [100, 500, 2000]);
    gp.cache_put(1, 111, 1);
    chk(&mut set, "C507-01", gp.cache_get(1, 1) == Some(111), "缓存命中");
    chk(&mut set, "C507-02", gp.cache_get(1, 2) == None, "版本戳不符失效重取");
    chk(&mut set, "C507-03", gp.cache.iter().find(|c| c.key == 1).is_none(), "失真条目清除");

    // 三层归档
    gp.arch_new(9);
    chk(&mut set, "C507-04", gp.arch[0].tier == Tier::Hot, "新档热层");
    let _ = gp.demote(9, Tier::Warm);
    let _ = gp.demote(9, Tier::Cold);
    chk(&mut set, "C507-05", gp.arch[0].tier == Tier::Cold && gp.arch[0].cold_backup, "降层至冷层强制双写");
    chk(&mut set, "C507-06", gp.restore_ok(9), "冷备可恢复");
    chk(&mut set, "C507-07", gp.demote(99, Tier::Cold) == Err(GpCode::ArchiveLost), "未知档报错");

    // P95
    for i in 0..20u32 {
        gp.sample(50 + i);
    }
    let p = gp.p95();
    chk(&mut set, "C507-08", p <= gp.p95_cap, "查询 P95 ≤ 100ms 目标");
    chk(&mut set, "C507-09", p >= 65 && p <= 70, "P95 高位取值精确");

    // 吞吐分档
    chk(&mut set, "C507-10", gp.thru_tier(2500) == Some(2) && gp.thru_tier(600) == Some(1) && gp.thru_tier(150) == Some(0), "吞吐三档映射");
    chk(&mut set, "C507-11", gp.thru_tier(50) == None, "吞吐不达标显性拒绝");

    chk(&mut set, "C507-12", gp.shard(101, 10) == 11 && gp.shard(100, 0) == 0, "分片扩容取整+零片防护");

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gov_perf_cache() {
        let mut gp = GovPerf::new(100, [1, 2, 3]);
        gp.cache_put(1, 5, 1);
        assert_eq!(gp.cache_get(1, 1), Some(5));
        assert_eq!(gp.cache_get(1, 2), None);
        gp.arch_new(2);
        let _ = gp.demote(2, Tier::Cold);
        assert!(gp.restore_ok(2));
    }
}
