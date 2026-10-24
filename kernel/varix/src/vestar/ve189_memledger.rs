//! ve189_memledger ↔ VE-F0189 域内存占用总账
//!
//! A 域常驻与峰值内存逐组件登记（交换链、命令缓冲池、调试设施、缓存），
//! 常驻上限承诺与超限处置（告警加逐出策略）显性化；与全域内存预算体系对账。
//!
//! 判据：常驻上限、超限逐出、对账补登、分位呈现。

use alloc::format;
use alloc::collections::BTreeMap;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

/// 组件类别。
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum ComponentKind {
    /// 交换链。
    SwapChain,
    /// 命令缓冲池。
    CommandPool,
    /// 调试设施。
    DebugFacility,
    /// 缓存。
    Cache,
    /// 其他。
    Other,
}

/// 组件内存记录。
#[derive(Debug, Clone)]
pub struct MemEntry {
    /// 组件名称。
    pub name: String,
    /// 组件类别。
    pub kind: ComponentKind,
    /// 常驻内存（字节）。
    pub resident: u64,
    /// 峰值内存（字节）。
    pub peak: u64,
    /// 常驻上限（字节）。
    pub resident_limit: u64,
    /// 是否超限。
    pub over_limit: bool,
}

/// 逐出优先级（缓存类先逐出）。
pub fn evict_priority(kind: ComponentKind) -> u8 {
    match kind {
        ComponentKind::Cache => 0,          // 最先逐出
        ComponentKind::DebugFacility => 1,  // 其次
        ComponentKind::Other => 2,
        ComponentKind::CommandPool => 3,
        ComponentKind::SwapChain => 4,      // 最后
    }
}

/// 域内存占用总账。
pub struct MemLedger {
    /// 组件表。
    entries: BTreeMap<String, MemEntry>,
    /// 全域预算（对账用）。
    pub domain_budget: u64,
    /// 告警计数。
    warn_count: u64,
    /// 逐出计数。
    evict_count: u64,
}

impl MemLedger {
    /// 新建总账。
    pub fn new(domain_budget: u64) -> Self {
        Self {
            entries: BTreeMap::new(),
            domain_budget,
            warn_count: 0,
            evict_count: 0,
        }
    }

    /// 登记组件。
    pub fn register(
        &mut self,
        name: &str,
        kind: ComponentKind,
        resident: u64,
        peak: u64,
        resident_limit: u64,
    ) {
        let over_limit = resident > resident_limit;
        if over_limit {
            self.warn_count += 1;
        }
        self.entries.insert(
            name.to_string(),
            MemEntry {
                name: name.to_string(),
                kind,
                resident,
                peak,
                resident_limit,
                over_limit,
            },
        );
    }

    /// 更新占用（超限告警）。
    pub fn update(&mut self, name: &str, resident: u64, peak: u64) -> Result<bool, u16> {
        let entry = self.entries.get_mut(name).ok_or(0x8201u16)?;
        entry.resident = resident;
        entry.peak = entry.peak.max(peak); // 峰值只增不减
        entry.over_limit = resident > entry.resident_limit;
        if entry.over_limit {
            self.warn_count += 1;
        }
        Ok(entry.over_limit)
    }

    /// 超限逐出：返回逐出候选（按优先级排序）。
    pub fn evict_candidates(&self) -> Vec<&MemEntry> {
        let mut over: Vec<&MemEntry> = self.entries.values().filter(|e| e.over_limit).collect();
        over.sort_by_key(|e| evict_priority(e.kind));
        over
    }

    /// 执行逐出（减少常驻到上限以下）。
    pub fn evict_one(&mut self, name: &str) -> Result<u64, u16> {
        let entry = self.entries.get_mut(name).ok_or(0x8201u16)?;
        if entry.resident <= entry.resident_limit {
            return Ok(0); // 无需逐出
        }
        let freed = entry.resident - entry.resident_limit;
        entry.resident = entry.resident_limit;
        entry.over_limit = false;
        self.evict_count += 1;
        Ok(freed)
    }

    /// 对账：计算总常驻与全域预算的差值。
    pub fn reconcile(&self) -> i64 {
        let total: u64 = self.entries.values().map(|e| e.resident).sum();
        self.domain_budget as i64 - total as i64
    }

    /// 补登记：对账发现未登记占用时调用。
    pub fn register_missing(&mut self, name: &str, kind: ComponentKind, resident: u64, peak: u64) {
        let limit = resident; // 新登记默认上限=当前值（待调）
        self.register(name, kind, resident, peak, limit);
    }

    /// 分位呈现：峰值的 P50/P95/P99（按组件排序取分位）。
    pub fn peak_percentiles(&self) -> (f64, f64, f64) {
        let mut peaks: Vec<u64> = self.entries.values().map(|e| e.peak).collect();
        if peaks.is_empty() {
            return (0.0, 0.0, 0.0);
        }
        peaks.sort_unstable();
        let n = peaks.len();
        let p50 = peaks[n / 2] as f64;
        let p95 = peaks[(n * 95 / 100).min(n - 1)] as f64;
        let p99 = peaks[(n * 99 / 100).min(n - 1)] as f64;
        (p50, p95, p99)
    }

    /// 查询组件。
    pub fn get(&self, name: &str) -> Option<&MemEntry> {
        self.entries.get(name)
    }

    /// 全量组件。
    pub fn all(&self) -> Vec<&MemEntry> {
        self.entries.values().collect()
    }

    /// 告警计数。
    pub fn warn_count(&self) -> u64 {
        self.warn_count
    }

    /// 逐出计数。
    pub fn evict_count(&self) -> u64 {
        self.evict_count
    }

    /// 总常驻。
    pub fn total_resident(&self) -> u64 {
        self.entries.values().map(|e| e.resident).sum()
    }

    /// 组件总数。
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// 是否为空。
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_register_and_check() {
        let mut l = MemLedger::new(1_000_000);
        l.register("swapchain", ComponentKind::SwapChain, 500_000, 600_000, 550_000);
        assert!(l.get("swapchain").unwrap().over_limit);
        assert_eq!(l.warn_count(), 1);
    }

    #[test]
    fn test_evict_priority_order() {
        let mut l = MemLedger::new(1_000_000);
        l.register("cache1", ComponentKind::Cache, 200_000, 200_000, 100_000);
        l.register("swap1", ComponentKind::SwapChain, 200_000, 200_000, 100_000);
        let candidates = l.evict_candidates();
        // Cache 先逐出
        assert_eq!(candidates[0].kind, ComponentKind::Cache);
        assert_eq!(candidates[1].kind, ComponentKind::SwapChain);
    }

    #[test]
    fn test_evict_one() {
        let mut l = MemLedger::new(1_000_000);
        l.register("c", ComponentKind::Cache, 200, 200, 100);
        let freed = l.evict_one("c").unwrap();
        assert_eq!(freed, 100);
        assert!(!l.get("c").unwrap().over_limit);
    }

    #[test]
    fn test_reconcile() {
        let mut l = MemLedger::new(1000);
        l.register("a", ComponentKind::Other, 300, 300, 500);
        l.register("b", ComponentKind::Other, 400, 400, 500);
        assert_eq!(l.reconcile(), 300); // 1000 - 700
    }

    #[test]
    fn test_peak_percentiles() {
        let mut l = MemLedger::new(1_000_000);
        for i in 0..10 {
            l.register(
                alloc::format!("c{}", i).as_str(),
                ComponentKind::Other,
                i * 100,
                i * 100,
                10_000,
            );
        }
        let (p50, p95, _) = l.peak_percentiles();
        assert!(p50 >= 400.0);
        assert!(p95 >= 800.0);
    }

    #[test]
    fn test_update_over_limit() {
        let mut l = MemLedger::new(1_000_000);
        l.register("x", ComponentKind::Cache, 50, 50, 100);
        let over = l.update("x", 150, 150).unwrap();
        assert!(over);
        assert_eq!(l.warn_count(), 1);
    }
}
