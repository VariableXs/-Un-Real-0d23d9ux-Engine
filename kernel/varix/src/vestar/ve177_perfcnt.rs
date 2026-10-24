//! ve177_perfcnt ↔ VE-F0177 性能计数器注册表
//!
//! 计数器元数据注册制：名称、单位、来源、采集档位、语义描述五要素齐备才可注册；
//! 注册表版本化，查询按名称或来源过滤；重复注册与语义冲突拒绝。
//!
//! 判据：五要素注册、版本化、冲突拒绝、失效标记。

use alloc::collections::BTreeMap;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

/// 采集档位：决定计数器被采集的频率与成本。
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum CollectTier {
    /// 每帧采集（高成本，仅关键指标）。
    PerFrame,
    /// 每秒采集（中等成本）。
    PerSecond,
    /// 每分钟采集（低成本，长周期指标）。
    PerMinute,
    /// 按需采集（仅手动触发）。
    OnDemand,
}

/// 五要素元数据条目。
#[derive(Debug, Clone)]
pub struct CounterMeta {
    /// 计数器名称（唯一键）。
    pub name: String,
    /// 单位（如 "ms"、"bytes"、"fps"、"count"）。
    pub unit: String,
    /// 来源标识（哪个子系统提供数据）。
    pub source: String,
    /// 采集档位。
    pub tier: CollectTier,
    /// 语义描述（人读文本，规范书写）。
    pub desc: String,
    /// 失效标记（来源注销后标记，不删除）。
    pub invalidated: bool,
    /// 注册时的注册表版本号。
    pub registered_at: u64,
}

/// 注册错误码。
pub mod err {
    /// 五要素不全（缺名称/单位/来源/档位/描述任一项）。
    pub const E_INCOMPLETE: u16 = 0x7701;
    /// 语义冲突：同名已有不同定义。
    pub const E_CONFLICT: u16 = 0x7702;
    /// 名称为空或含非法字符。
    pub const E_BAD_NAME: u16 = 0x7703;
    /// 来源为空。
    pub const E_BAD_SOURCE: u16 = 0x7704;
    /// 查询键无效。
    pub const E_BAD_QUERY: u16 = 0x7705;
}

/// 性能计数器注册表。
pub struct PerfCounterRegistry {
    /// 名称 → 条目索引。
    by_name: BTreeMap<String, usize>,
    /// 来源 → 名称列表索引。
    by_source: BTreeMap<String, Vec<String>>,
    /// 条目存储（只增不删，失效标记替代删除）。
    entries: Vec<CounterMeta>,
    /// 当前版本号（每次注册/失效 +1）。
    version: u64,
}

impl PerfCounterRegistry {
    /// 新建空注册表，版本号 1。
    pub fn new() -> Self {
        Self {
            by_name: BTreeMap::new(),
            by_source: BTreeMap::new(),
            entries: Vec::new(),
            version: 1,
        }
    }

    /// 当前版本号。
    pub fn version(&self) -> u64 {
        self.version
    }

    /// 条目总数（含失效）。
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// 是否为空。
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// 有效条目数（不含失效）。
    pub fn active_len(&self) -> usize {
        self.entries.iter().filter(|e| !e.invalidated).count()
    }

    /// 注册一个计数器（五要素齐备才可注册）。
    ///
    /// 返回注册时的版本号。失败返回错误码。
    pub fn register(
        &mut self,
        name: &str,
        unit: &str,
        source: &str,
        tier: CollectTier,
        desc: &str,
    ) -> Result<u64, u16> {
        // 边界防护：名称校验
        if name.is_empty() || name.len() > 128 {
            return Err(err::E_BAD_NAME);
        }
        // 名称字符白名单：字母数字下划线点号
        if !name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '.')
        {
            return Err(err::E_BAD_NAME);
        }
        // 来源校验
        if source.is_empty() || source.len() > 64 {
            return Err(err::E_BAD_SOURCE);
        }
        // 五要素齐备判定
        if unit.is_empty() || desc.is_empty() {
            return Err(err::E_INCOMPLETE);
        }

        // 语义冲突检测：同名已有定义
        if let Some(&idx) = self.by_name.get(name) {
            let existing = &self.entries[idx];
            if existing.unit == unit && existing.source == source && existing.desc == desc {
                // 完全相同的重复注册 → 拒绝（幂等语义）
                return Err(err::E_CONFLICT);
            }
            // 同名异义 → 拒绝并提示既有定义
            return Err(err::E_CONFLICT);
        }

        let entry = CounterMeta {
            name: name.to_string(),
            unit: unit.to_string(),
            source: source.to_string(),
            tier,
            desc: desc.to_string(),
            invalidated: false,
            registered_at: self.version,
        };

        let idx = self.entries.len();
        self.by_name.insert(name.to_string(), idx);
        self.by_source
            .entry(source.to_string())
            .or_default()
            .push(name.to_string());
        self.entries.push(entry);
        self.version += 1;
        Ok(self.version - 1)
    }

    /// 按名称查询（返回引用）。
    pub fn get(&self, name: &str) -> Option<&CounterMeta> {
        self.by_name.get(name).map(|&i| &self.entries[i])
    }

    /// 按来源过滤（返回有效条目引用列表）。
    pub fn filter_by_source(&self, source: &str) -> Vec<&CounterMeta> {
        if source.is_empty() {
            return Vec::new();
        }
        self.by_source
            .get(source)
            .map(|names| {
                names
                    .iter()
                    .filter_map(|n| self.by_name.get(n.as_str()).map(|&i| &self.entries[i]))
                    .filter(|e| !e.invalidated)
                    .collect()
            })
            .unwrap_or_default()
    }

    /// 按名称前缀过滤（返回有效条目引用列表）。
    pub fn filter_by_prefix(&self, prefix: &str) -> Vec<&CounterMeta> {
        if prefix.is_empty() {
            return Vec::new();
        }
        self.entries
            .iter()
            .filter(|e| !e.invalidated && e.name.starts_with(prefix))
            .collect()
    }

    /// 标记失效（来源注销后调用；不删除条目）。
    pub fn invalidate(&mut self, name: &str) -> Result<(), u16> {
        let idx = self.by_name.get(name).copied().ok_or(err::E_BAD_QUERY)?;
        if self.entries[idx].invalidated {
            return Ok(()); // 幂等
        }
        self.entries[idx].invalidated = true;
        self.version += 1;
        Ok(())
    }

    /// 批量标记来源下所有计数器失效。
    pub fn invalidate_source(&mut self, source: &str) -> usize {
        let names = match self.by_source.get(source) {
            Some(n) => n.clone(),
            None => return 0,
        };
        let mut count = 0;
        for name in &names {
            if let Some(&idx) = self.by_name.get(name.as_str()) {
                if !self.entries[idx].invalidated {
                    self.entries[idx].invalidated = true;
                    count += 1;
                }
            }
        }
        if count > 0 {
            self.version += 1;
        }
        count
    }

    /// 列出所有有效条目（按名称排序）。
    pub fn list_active(&self) -> Vec<&CounterMeta> {
        self.entries.iter().filter(|e| !e.invalidated).collect()
    }

    /// 列出所有来源名称。
    pub fn list_sources(&self) -> Vec<&str> {
        self.by_source.keys().map(|s| s.as_str()).collect()
    }

    /// 按档位过滤有效条目。
    pub fn filter_by_tier(&self, tier: CollectTier) -> Vec<&CounterMeta> {
        self.entries
            .iter()
            .filter(|e| !e.invalidated && e.tier == tier)
            .collect()
    }
}

impl Default for PerfCounterRegistry {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_five_element_register() {
        let mut reg = PerfCounterRegistry::new();
        let v = reg
            .register("gpu.frame_time", "ms", "gpu", CollectTier::PerFrame, "GPU 帧耗时")
            .unwrap();
        assert_eq!(v, 1);
        assert_eq!(reg.len(), 1);
        assert_eq!(reg.version(), 2);
    }

    #[test]
    fn test_incomplete_rejected() {
        let mut reg = PerfCounterRegistry::new();
        assert_eq!(
            reg.register("x", "ms", "src", CollectTier::PerFrame, ""),
            Err(err::E_INCOMPLETE)
        );
        assert_eq!(
            reg.register("x", "", "src", CollectTier::PerFrame, "desc"),
            Err(err::E_INCOMPLETE)
        );
    }

    #[test]
    fn test_conflict_rejected() {
        let mut reg = PerfCounterRegistry::new();
        reg.register("k", "ms", "s", CollectTier::PerSecond, "d")
            .unwrap();
        // 同名异义
        assert_eq!(
            reg.register("k", "bytes", "s", CollectTier::PerSecond, "d2"),
            Err(err::E_CONFLICT)
        );
    }

    #[test]
    fn test_invalidate_not_delete() {
        let mut reg = PerfCounterRegistry::new();
        reg.register("a", "ms", "src", CollectTier::PerFrame, "d")
            .unwrap();
        reg.invalidate("a").unwrap();
        assert_eq!(reg.len(), 1); // 仍在
        assert_eq!(reg.active_len(), 0); // 但无效
        assert!(reg.get("a").unwrap().invalidated);
    }

    #[test]
    fn test_filter_by_source() {
        let mut reg = PerfCounterRegistry::new();
        reg.register("a", "ms", "gpu", CollectTier::PerFrame, "d")
            .unwrap();
        reg.register("b", "ms", "gpu", CollectTier::PerSecond, "d")
            .unwrap();
        reg.register("c", "ms", "cpu", CollectTier::PerSecond, "d")
            .unwrap();
        assert_eq!(reg.filter_by_source("gpu").len(), 2);
        assert_eq!(reg.filter_by_source("cpu").len(), 1);
        assert_eq!(reg.filter_by_source("none").len(), 0);
    }

    #[test]
    fn test_bad_name_rejected() {
        let mut reg = PerfCounterRegistry::new();
        assert_eq!(
            reg.register("", "ms", "s", CollectTier::PerFrame, "d"),
            Err(err::E_BAD_NAME)
        );
        assert_eq!(
            reg.register("bad name!", "ms", "s", CollectTier::PerFrame, "d"),
            Err(err::E_BAD_NAME)
        );
    }

    #[test]
    fn test_invalidate_source_batch() {
        let mut reg = PerfCounterRegistry::new();
        reg.register("a", "ms", "gpu", CollectTier::PerFrame, "d")
            .unwrap();
        reg.register("b", "ms", "gpu", CollectTier::PerSecond, "d")
            .unwrap();
        reg.register("c", "ms", "cpu", CollectTier::PerSecond, "d")
            .unwrap();
        let n = reg.invalidate_source("gpu");
        assert_eq!(n, 2);
        assert_eq!(reg.active_len(), 1);
    }
}
