// CGPU-F0354 预算配置管理 · cgb354_config.rs
// AI-MIMO-1022 · CGPU-C 预算收口
// 判据：预算配置（阈值/权重/策略）版本化管理；
//       配置热加载；变更审计；默认值可恢复；配置校验。
#![allow(dead_code)]

extern crate alloc;
use alloc::vec::Vec;

use super::super::checks::CheckSet;

/// 配置项。
#[derive(Clone, Debug)]
pub struct ConfigItem {
    pub key: u32,
    pub value_q8: u64,
    pub default_q8: u64,
    pub min_q8: u64,
    pub max_q8: u64,
    pub version: u32,
    pub dirty: bool,
}

/// 配置变更。
#[derive(Clone, Copy, Debug)]
pub struct ConfigChange {
    pub key: u32,
    pub old_value: u64,
    pub new_value: u64,
    pub version: u32,
    pub frame: u64,
}

/// 配置管理器。
pub struct BudgetConfig {
    pub items: Vec<ConfigItem>,
    pub changes: Vec<ConfigChange>,
    pub version: u32,
    pub rollback_count: u32,
}

impl BudgetConfig {
    pub fn new() -> Self {
        Self {
            items: Vec::new(),
            changes: Vec::new(),
            version: 1,
            rollback_count: 0,
        }
    }

    /// 注册配置项。
    pub fn register(&mut self, key: u32, default_q8: u64, min_q8: u64, max_q8: u64) -> u32 {
        self.items.push(ConfigItem {
            key,
            value_q8: default_q8,
            default_q8,
            min_q8,
            max_q8,
            version: self.version,
            dirty: false,
        });
        key
    }

    /// 设置值（带范围校验）。
    pub fn set(&mut self, key: u32, value: u64, frame: u64) -> bool {
        if let Some(item) = self.items.iter_mut().find(|i| i.key == key) {
            if value < item.min_q8 || value > item.max_q8 {
                return false;
            }
            let old = item.value_q8;
            item.value_q8 = value;
            item.dirty = old != value;
            self.changes.push(ConfigChange {
                key,
                old_value: old,
                new_value: value,
                version: self.version,
                frame,
            });
            true
        } else {
            false
        }
    }

    /// 读值。
    pub fn get(&self, key: u32) -> Option<u64> {
        self.items.iter().find(|i| i.key == key).map(|i| i.value_q8)
    }

    /// 恢复默认。
    pub fn reset_to_default(&mut self, key: u32) -> bool {
        if let Some(item) = self.items.iter_mut().find(|i| i.key == key) {
            item.value_q8 = item.default_q8;
            item.dirty = false;
            self.rollback_count += 1;
            true
        } else {
            false
        }
    }

    /// 全量校验（所有值在范围内）。
    pub fn validate(&self) -> bool {
        self.items
            .iter()
            .all(|i| i.value_q8 >= i.min_q8 && i.value_q8 <= i.max_q8)
    }

    /// 版本推进。
    pub fn bump_version(&mut self) {
        self.version += 1;
        for i in self.items.iter_mut() {
            i.dirty = false;
        }
    }

    pub fn run_checks(&self) -> CheckSet {
        let mut cs = CheckSet::new("CGPU-F0354 预算配置管理");
        cs.add("key 唯一", {
            let mut keys: Vec<u32> = self.items.iter().map(|i| i.key).collect();
            keys.sort_unstable();
            keys.dedup();
            keys.len() == self.items.len()
        }, "unique");
        cs.add("值在范围内", self.validate(), "range");
        cs.add("默认值在范围内", {
            self.items.iter().all(|i| i.default_q8 >= i.min_q8 && i.default_q8 <= i.max_q8)
        }, "default range");
        cs.add("min ≤ max", {
            self.items.iter().all(|i| i.min_q8 <= i.max_q8)
        }, "min ≤ max");
        cs.add("变更记录有序", {
            self.changes.windows(2).all(|w| w[0].frame <= w[1].frame)
        }, "frame mono");
        cs.add("版本正", self.version > 0, "ver > 0");
        cs.add("回滚可数", self.rollback_count <= self.items.len() as u32 + 1, "countable");
        cs
    }
}

impl Default for BudgetConfig {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn set_and_validate() {
        let mut c = BudgetConfig::new();
        c.register(1, 100, 0, 256);
        assert!(c.set(1, 200, 1));
        assert_eq!(c.get(1), Some(200));
        assert!(!c.set(1, 300, 2)); // 超范围
        assert!(c.validate());
    }

    #[test]
    fn reset_default() {
        let mut c = BudgetConfig::new();
        c.register(1, 100, 0, 256);
        c.set(1, 200, 1);
        assert!(c.reset_to_default(1));
        assert_eq!(c.get(1), Some(100));
    }
}
