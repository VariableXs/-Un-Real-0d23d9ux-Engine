//! 深化层二 · F146 插件化星图后端（2026-09-26 深化批次二）。
//!
//! 补深主册【数据与存储】源配置持久化 +【设计细节】差量同步 since
//! 协议与镜像指南（主册 G-D-21）：源配置表（优先级+启用态持久模型）、
//! 差量同步幂等合并、缓存配额与过期清理、健康历史序列与可用率、
//! 回退事件审计、镜像元数据格式兼容判定。

use crate::checks::CheckSet;
use crate::stareco::ebase::fnv1a64;
use crate::stareco::starmapprov::{SourceHealth, SourceKind, HEALTH_CHECK_INTERVAL_MS};

// ---------------------------------------------------------------------------
// 源配置表：持久模型（顺序 + 启用态——用户自主权的存储面）
// ---------------------------------------------------------------------------

pub struct SourceConfig {
    pub name: &'static str,
    pub kind: SourceKind,
    pub enabled: bool,
    /// 回退优先级（小者先试——官方默认 0，用户可改）。
    pub priority: u8,
}

pub struct SourceConfigBook {
    rows: alloc::vec::Vec<SourceConfig>,
}

impl SourceConfigBook {
    pub fn new() -> SourceConfigBook {
        SourceConfigBook { rows: alloc::vec::Vec::new() }
    }

    pub fn add(&mut self, c: SourceConfig) -> Result<(), &'static str> {
        if c.name.is_empty() {
            return Err("源名必填");
        }
        if self.rows.iter().any(|r| r.name == c.name) {
            return Err("源重复：一名一配置");
        }
        self.rows.push(c);
        Ok(())
    }

    pub fn set_enabled(&mut self, name: &str, on: bool) -> Result<(), &'static str> {
        let r = self.rows.iter_mut().find(|r| r.name == name).ok_or("源不存在")?;
        r.enabled = on;
        Ok(())
    }

    /// 回退序：启用源按优先级升序（稳定排序——同级保注册序）。
    pub fn fallback_order(&self) -> alloc::vec::Vec<&'static str> {
        let mut v: alloc::vec::Vec<&SourceConfig> =
            self.rows.iter().filter(|r| r.enabled).collect();
        v.sort_by_key(|r| r.priority);
        v.iter().map(|r| r.name).collect()
    }

    /// 官方源首位检查（默认配置不误导——用户可改但默认要正）。
    pub fn official_first_by_default(&self) -> bool {
        self.rows
            .iter()
            .filter(|r| r.enabled)
            .min_by_key(|r| r.priority)
            .map_or(true, |r| r.kind == SourceKind::Official)
    }

    pub fn len(&self) -> usize {
        self.rows.len()
    }
}

// ---------------------------------------------------------------------------
// 差量同步：since 参数 → 增量集 → 幂等合并（F128 协议复用面）
// ---------------------------------------------------------------------------

pub struct DeltaEntry {
    pub key: &'static str,
    pub version_fp: u64,
}

/// 差量集：since 指纹之后的所有变更（同键取新版本——合并幂等的关键）。
pub fn merge_delta(local: &mut alloc::vec::Vec<DeltaEntry>, delta: &[DeltaEntry]) -> usize {
    let mut applied = 0;
    for d in delta {
        if d.version_fp == 0 {
            continue; // 零指纹条目拒收（脏数据防线）
        }
        if let Some(e) = local.iter_mut().find(|e| e.key == d.key) {
            if e.version_fp != d.version_fp {
                e.version_fp = d.version_fp;
                applied += 1;
            }
            // 同版本重复推送不计数——幂等
        } else {
            local.push(DeltaEntry { key: d.key, version_fp: d.version_fp });
            applied += 1;
        }
    }
    applied
}

/// 幂等性验证：同一差量合并两次，第二次必须零变更。
pub fn merge_is_idempotent(local: &mut alloc::vec::Vec<DeltaEntry>, delta: &[DeltaEntry]) -> bool {
    merge_delta(local, delta);
    merge_delta(local, delta) == 0
}

// ---------------------------------------------------------------------------
// 缓存配额与过期清理（按源隔离的容量纪律）
// ---------------------------------------------------------------------------

pub struct CacheQuota {
    pub max_entries: usize,
    /// (key, 指纹, 最后命中日)。
    entries: alloc::vec::Vec<(&'static str, u64, u32)>,
}

impl CacheQuota {
    pub fn new(max_entries: usize) -> CacheQuota {
        CacheQuota { max_entries, entries: alloc::vec::Vec::new() }
    }

    pub fn put(&mut self, key: &'static str, fp: u64, today: u32) -> Result<(), &'static str> {
        if fp == 0 {
            return Err("零指纹不缓存");
        }
        if let Some(e) = self.entries.iter_mut().find(|(k, _, _)| *k == key) {
            e.1 = fp;
            e.2 = today;
            return Ok(());
        }
        if self.entries.len() >= self.max_entries {
            // LRU 淘汰：最后命中日最旧者出（并列取先登记——确定性）
            let victim = self
                .entries
                .iter()
                .enumerate()
                .min_by_key(|(i, (_, _, d))| (*d, *i as u32))
                .map(|(i, _)| i)
                .expect("non-empty");
            self.entries.remove(victim);
        }
        self.entries.push((key, fp, today));
        Ok(())
    }

    /// 过期清理：last-hit 距今超过 stale_days 的条目出。
    pub fn evict_stale(&mut self, today: u32, stale_days: u32) -> usize {
        let before = self.entries.len();
        self.entries.retain(|(_, _, d)| today.saturating_sub(*d) <= stale_days);
        before - self.entries.len()
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }
}

// ---------------------------------------------------------------------------
// 健康历史与可用率（6h 节拍的统计面）
// ---------------------------------------------------------------------------

pub struct HealthHistory {
    /// (时间戳, 健康) 有序序列。
    samples: alloc::vec::Vec<(u64, SourceHealth)>,
}

impl HealthHistory {
    pub fn new() -> HealthHistory {
        HealthHistory { samples: alloc::vec::Vec::new() }
    }

    pub fn record(&mut self, ts_ms: u64, h: SourceHealth) {
        self.samples.push((ts_ms, h));
    }

    /// 节拍合规：相邻采样间隔 ≤ 6h（漏检可测）。
    pub fn cadence_ok(&self) -> bool {
        self.samples.windows(2).all(|w| w[1].0 - w[0].0 <= HEALTH_CHECK_INTERVAL_MS)
    }

    /// 可用率（万分比）：Reachable 占比。
    pub fn availability_bp(&self) -> u32 {
        if self.samples.is_empty() {
            return 0;
        }
        let ok = self.samples.iter().filter(|(_, h)| *h == SourceHealth::Ok).count();
        ((ok * 10_000) / self.samples.len()) as u32
    }

    pub fn len(&self) -> usize {
        self.samples.len()
    }
}

// ---------------------------------------------------------------------------
// 回退事件审计：原因/耗时/最终源（降级不留黑箱）
// ---------------------------------------------------------------------------

pub struct FailoverEvent {
    pub day: u32,
    pub from: &'static str,
    pub to: &'static str,
    pub cause_fp: u64,
    pub elapsed_ms: u32,
}

pub struct FailoverLog {
    events: alloc::vec::Vec<FailoverEvent>,
}

impl FailoverLog {
    pub fn new() -> FailoverLog {
        FailoverLog { events: alloc::vec::Vec::new() }
    }

    pub fn record(&mut self, e: FailoverEvent) -> Result<(), &'static str> {
        if e.from == e.to {
            return Err("回退到自身：不叫回退");
        }
        if e.cause_fp == 0 {
            return Err("原因指纹缺失：黑箱降级拒绝");
        }
        self.events.push(e);
        Ok(())
    }

    /// 最终落点统计（全部源断 → 本地缓存的终态可查）。
    pub fn final_target(&self, from: &str) -> Option<&'static str> {
        self.events.iter().rev().find(|e| e.from == from).map(|e| e.to)
    }

    pub fn len(&self) -> usize {
        self.events.len()
    }
}

// ---------------------------------------------------------------------------
// 镜像元数据格式兼容判定（镜像指南的机器校验面）
// ---------------------------------------------------------------------------

/// 镜像元数据行：`mirror|fp|since`——三段且指纹与官方版本一致才兼容。
pub fn mirror_manifest_ok(line: &str, official_fp: u64) -> Result<bool, &'static str> {
    let p: alloc::vec::Vec<&str> = line.split('|').collect();
    if p.len() != 3 {
        return Err("镜像元数据必须三段：mirror|fp|since");
    }
    if p[0].is_empty() {
        return Err("镜像名缺失");
    }
    let fp: u64 = p[1].parse().map_err(|_| "指纹非数字")?;
    let _since: u32 = p[2].parse().map_err(|_| "since 非数字")?;
    Ok(fp == official_fp) // 同版本数据才判兼容（三源一致判据）
}

// ---------------------------------------------------------------------------
// 自检
// ---------------------------------------------------------------------------

pub const F146E_TAG: &str = "stareco-F146-deep2";

pub fn run_f146_deep2_checks() -> CheckSet {
    let mut set = CheckSet::new(F146E_TAG);

    // 源配置
    let mut book = SourceConfigBook::new();
    let _ = book.add(SourceConfig { name: "official", kind: SourceKind::Official, enabled: true, priority: 0 });
    let _ = book.add(SourceConfig { name: "local", kind: SourceKind::LocalFile, enabled: true, priority: 5 });
    let _ = book.add(SourceConfig { name: "mirror-eu", kind: SourceKind::Mirror, enabled: true, priority: 2 });
    set.add(
        "f146e official first",
        book.official_first_by_default(),
        "默认官方首位",
    );
    set.add(
        "f146e fallback order",
        book.fallback_order() == alloc::vec!["official", "mirror-eu", "local"],
        "优先级回退序",
    );
    let _ = book.set_enabled("official", false);
    set.add(
        "f146e disabled skip",
        book.fallback_order() == alloc::vec!["mirror-eu", "local"],
        "停用源不入回退链",
    );
    set.add("f146e dup", book.add(SourceConfig { name: "local", kind: SourceKind::LocalFile, enabled: true, priority: 9 }).is_err(), "重名拒绝");

    // 差量幂等
    let mut local: alloc::vec::Vec<DeltaEntry> = alloc::vec![DeltaEntry { key: "k1", version_fp: 10 }];
    let delta = alloc::vec![
        DeltaEntry { key: "k1", version_fp: 20 },
        DeltaEntry { key: "k2", version_fp: 30 },
        DeltaEntry { key: "k3", version_fp: 0 }, // 脏条目
    ];
    set.add("f146e merge apply", merge_delta(&mut local, &delta) == 2, "新键+更新=2（脏条目拒）");
    set.add("f146e merge idempotent", merge_is_idempotent(&mut local, &delta), "重放零变更");

    // 缓存配额
    let mut cache = CacheQuota::new(3);
    let _ = cache.put("a", 1, 100);
    let _ = cache.put("b", 2, 101);
    let _ = cache.put("c", 3, 102);
    let _ = cache.put("d", 4, 103); // 淘汰 a（最旧命中）
    set.add("f146e cache lru", cache.len() == 3, "容量上限淘汰");
    let _ = cache.put("b", 22, 104); // b 更新不增容
    set.add("f146e cache update", cache.len() == 3, "同键更新不增容");
    set.add("f146e cache zero", cache.put("e", 0, 1).is_err(), "零指纹拒缓存");
    let evicted = cache.evict_stale(400, 100);
    set.add("f146e evict stale", evicted >= 3 && cache.len() == 0, "全量过期清空");

    // 健康历史
    let mut hh = HealthHistory::new();
    let six_h: u64 = HEALTH_CHECK_INTERVAL_MS;
    hh.record(0, SourceHealth::Ok);
    hh.record(six_h, SourceHealth::Unreachable);
    hh.record(2 * six_h, SourceHealth::Ok);
    set.add("f146e cadence ok", hh.cadence_ok(), "6h 节拍合规");
    set.add("f146e availability", hh.availability_bp() == 6_667, "可用率 2/3");
    set.add("f146e empty avail", HealthHistory::new().availability_bp() == 0, "空序列零可用率");

    // 回退审计
    let mut log = FailoverLog::new();
    let _ = log.record(FailoverEvent { day: 1, from: "official", to: "mirror-eu", cause_fp: fnv1a64(b"unreachable"), elapsed_ms: 40 });
    set.add("f146e failover log", log.len() == 1, "回退事件在册");
    set.add(
        "f146e final target",
        log.final_target("official") == Some("mirror-eu"),
        "终落点可查",
    );
    set.add(
        "f146e self failover",
        log.record(FailoverEvent { day: 2, from: "official", to: "official", cause_fp: 1, elapsed_ms: 1 }).is_err(),
        "自回退拒绝",
    );
    set.add(
        "f146e no cause",
        log.record(FailoverEvent { day: 2, from: "a", to: "b", cause_fp: 0, elapsed_ms: 1 }).is_err(),
        "无原因拒绝",
    );

    // 镜像元数据
    set.add(
        "f146e mirror ok",
        mirror_manifest_ok("eu|12345|20260901", 12345) == Ok(true),
        "同版本指纹兼容",
    );
    set.add(
        "f146e mirror stale",
        mirror_manifest_ok("eu|99999|20260901", 12345) == Ok(false),
        "旧版本不兼容（判 false 不判错）",
    );
    set.add("f146e mirror bad", mirror_manifest_ok("eu|abc|1", 1).is_err(), "指纹非数字拒绝");

    set
}

#[cfg(test)]
mod deep2_tests {
    use super::*;

    #[test]
    fn merge_new_and_update() {
        let mut l: alloc::vec::Vec<DeltaEntry> = alloc::vec![];
        let d = alloc::vec![DeltaEntry { key: "x", version_fp: 1 }];
        assert_eq!(merge_delta(&mut l, &d), 1);
        assert_eq!(l.len(), 1);
        assert!(merge_is_idempotent(&mut l, &d));
    }

    #[test]
    fn config_empty_fallback() {
        let b = SourceConfigBook::new();
        assert!(b.fallback_order().is_empty());
        assert!(b.official_first_by_default()); // 空配置不误导
    }

    #[test]
    fn health_gap_detected() {
        let mut h = HealthHistory::new();
        h.record(0, SourceHealth::Ok);
        h.record(HEALTH_CHECK_INTERVAL_MS * 3, SourceHealth::Ok); // 漏检 2 拍
        assert!(!h.cadence_ok());
    }
}
