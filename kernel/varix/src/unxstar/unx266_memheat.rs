//! unx266_memheat ↔ UNX-F0266 mem 命令深化（热图分节视图）
//!
//! 热图按用途/节点双维度分节展示，数值与 F0077 账逐项一致。
//!
//! 判据：UNX-F0266-J1 热图按用途/节点双维度分节展示，数值与 F0077 账逐项一致。

use alloc::format;
use alloc::vec;
use alloc::collections::BTreeMap;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

/// 用途分类。
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Usage {
    /// 内核代码。
    KernelCode,
    /// 内核数据。
    KernelData,
    /// 用户空间。
    UserSpace,
    /// 页缓存。
    PageCache,
    /// 碎片。
    Fragment,
    /// 空闲。
    Free,
}

/// 节点内存条目。
#[derive(Debug, Clone)]
pub struct NodeMemEntry {
    /// 节点 ID。
    pub node_id: u32,
    /// 用途。
    pub usage: Usage,
    /// 字节数。
    pub bytes: u64,
}

/// 热图分节视图。
#[derive(Debug, Clone)]
pub struct HeatSection {
    /// 分节维度标签。
    pub label: String,
    /// 字节数合计。
    pub total_bytes: u64,
    /// 条目列表。
    pub entries: Vec<(u32, u64)>, // (node_id, bytes)
}

/// mem 热图分节视图系统。
pub struct MemHeatMap {
    /// 数据表。
    entries: Vec<NodeMemEntry>,
    /// 账本一致性校验计数。
    reconcile_pass: u64,
    /// 账本一致性失败计数。
    reconcile_fail: u64,
}

impl MemHeatMap {
    /// 新建空热图。
    pub fn new() -> Self {
        Self {
            entries: Vec::new(),
            reconcile_pass: 0,
            reconcile_fail: 0,
        }
    }

    /// 登记条目。
    pub fn add(&mut self, node_id: u32, usage: Usage, bytes: u64) {
        self.entries.push(NodeMemEntry {
            node_id,
            usage,
            bytes,
        });
    }

    /// 按用途分节展示。
    pub fn section_by_usage(&self) -> Vec<HeatSection> {
        let mut map: BTreeMap<Usage, Vec<(u32, u64)>> = BTreeMap::new();
        for e in &self.entries {
            map.entry(e.usage).or_default().push((e.node_id, e.bytes));
        }
        map.into_iter()
            .map(|(usage, entries)| {
                let total = entries.iter().map(|(_, b)| b).sum();
                HeatSection {
                    label: alloc::format!("{:?}", usage),
                    total_bytes: total,
                    entries,
                }
            })
            .collect()
    }

    /// 按节点分节展示。
    pub fn section_by_node(&self) -> Vec<HeatSection> {
        let mut map: BTreeMap<u32, Vec<(u32, u64)>> = BTreeMap::new();
        for e in &self.entries {
            map.entry(e.node_id)
                .or_default()
                .push((e.node_id, e.bytes));
        }
        map.into_iter()
            .map(|(node_id, entries)| {
                let total = entries.iter().map(|(_, b)| b).sum();
                HeatSection {
                    label: alloc::format!("node_{}", node_id),
                    total_bytes: total,
                    entries,
                }
            })
            .collect()
    }

    /// 与账本对账（数值逐项一致）。
    pub fn reconcile_with_ledger(&mut self, ledger: &[(u32, Usage, u64)]) -> bool {
        let mut ok = true;
        for &(node_id, usage, bytes) in ledger {
            let found = self
                .entries
                .iter()
                .find(|e| e.node_id == node_id && e.usage == usage);
            match found {
                Some(e) if e.bytes == bytes => {}
                _ => {
                    ok = false;
                    break;
                }
            }
        }
        if ok {
            self.reconcile_pass += 1;
        } else {
            self.reconcile_fail += 1;
        }
        ok
    }

    /// 总字节数。
    pub fn total_bytes(&self) -> u64 {
        self.entries.iter().map(|e| e.bytes).sum()
    }

    /// 条目数。
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// 是否为空。
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// 对账统计。
    pub fn reconcile_stats(&self) -> (u64, u64) {
        (self.reconcile_pass, self.reconcile_fail)
    }
}

impl Default for MemHeatMap {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_section_by_usage() {
        let mut h = MemHeatMap::new();
        h.add(0, Usage::KernelCode, 1024);
        h.add(1, Usage::KernelCode, 2048);
        h.add(0, Usage::UserSpace, 4096);
        let sections = h.section_by_usage();
        assert_eq!(sections.len(), 2); // KernelCode + UserSpace
        assert_eq!(sections[0].total_bytes, 3072); // 1024+2048
    }

    #[test]
    fn test_section_by_node() {
        let mut h = MemHeatMap::new();
        h.add(0, Usage::KernelCode, 1024);
        h.add(0, Usage::UserSpace, 2048);
        h.add(1, Usage::KernelCode, 4096);
        let sections = h.section_by_node();
        assert_eq!(sections.len(), 2); // node_0 + node_1
    }

    #[test]
    fn test_reconcile_ok() {
        let mut h = MemHeatMap::new();
        h.add(0, Usage::KernelCode, 1024);
        h.add(1, Usage::UserSpace, 2048);
        let ledger = vec![(0, Usage::KernelCode, 1024), (1, Usage::UserSpace, 2048)];
        assert!(h.reconcile_with_ledger(&ledger));
        assert_eq!(h.reconcile_stats().0, 1);
    }

    #[test]
    fn test_reconcile_fail() {
        let mut h = MemHeatMap::new();
        h.add(0, Usage::KernelCode, 1024);
        let ledger = vec![(0, Usage::KernelCode, 9999)];
        assert!(!h.reconcile_with_ledger(&ledger));
        assert_eq!(h.reconcile_stats().1, 1);
    }

    #[test]
    fn test_total_bytes() {
        let mut h = MemHeatMap::new();
        h.add(0, Usage::Free, 100);
        h.add(1, Usage::Free, 200);
        assert_eq!(h.total_bytes(), 300);
    }
}
