// CGPU-F0305 B域接口总账 · cgb305_ifaceledger.rs
// AI-MIMO-1022 · CGPU-B 收口
// 判据：B 域全部导出接口登记入账（名称/签名/族/稳定性/消费方）；
//       未登记接口不得导出；账本一致性可审计；版本变更可追踪。
#![allow(dead_code)]

use alloc::format;
extern crate alloc;
use alloc::vec::Vec;
use alloc::string::String;

use super::super::checks::CheckSet;

/// 接口条目。
#[derive(Clone, Debug)]
pub struct IfaceEntry {
    pub id: u32,
    pub name: String,
    pub family: u8,
    pub sig_hash: u32,
    pub stability: u8,
    pub consumers: Vec<u32>,
    pub registered_at: u32,
    pub last_change: u32,
    pub change_count: u32,
}

/// 账本变更记录。
#[derive(Clone, Debug)]
pub struct LedgerChange {
    pub entry_id: u32,
    pub version: u32,
    pub change_kind: u8, // 0=add 1=modify 2=deprecate 3=remove
    pub old_hash: u32,
    pub new_hash: u32,
}

/// 接口总账。
pub struct IfaceLedger {
    pub entries: Vec<IfaceEntry>,
    pub changes: Vec<LedgerChange>,
    pub current_version: u32,
    pub unregistered_count: u32,
}

impl IfaceLedger {
    pub fn new() -> Self {
        Self {
            entries: Vec::new(),
            changes: Vec::new(),
            current_version: 1,
            unregistered_count: 0,
        }
    }

    /// 登记接口。
    pub fn register(&mut self, name: &str, family: u8, sig_hash: u32, stability: u8) -> u32 {
        let id = self.entries.len() as u32 + 1;
        self.entries.push(IfaceEntry {
            id,
            name: String::from(name),
            family,
            sig_hash,
            stability,
            consumers: Vec::new(),
            registered_at: self.current_version,
            last_change: self.current_version,
            change_count: 1,
        });
        self.changes.push(LedgerChange {
            entry_id: id,
            version: self.current_version,
            change_kind: 0,
            old_hash: 0,
            new_hash: sig_hash,
        });
        id
    }

    /// 修改签名（记变更）。
    pub fn update_hash(&mut self, id: u32, new_hash: u32) -> bool {
        if let Some(e) = self.entries.iter_mut().find(|e| e.id == id) {
            let old = e.sig_hash;
            e.sig_hash = new_hash;
            e.last_change = self.current_version;
            e.change_count += 1;
            self.changes.push(LedgerChange {
                entry_id: id,
                version: self.current_version,
                change_kind: 1,
                old_hash: old,
                new_hash,
            });
            true
        } else {
            false
        }
    }

    /// 绑定消费方。
    pub fn add_consumer(&mut self, id: u32, consumer: u32) -> bool {
        if let Some(e) = self.entries.iter_mut().find(|e| e.id == id) {
            if !e.consumers.contains(&consumer) {
                e.consumers.push(consumer);
            }
            true
        } else {
            false
        }
    }

    /// 标记未登记导出（审计发现）。
    pub fn report_unregistered(&mut self) {
        self.unregistered_count += 1;
    }

    /// 审计：全部导出已登记。
    pub fn is_complete(&self) -> bool {
        self.unregistered_count == 0
    }

    /// 稳定接口（stability==0 Frozen）变更检测。
    pub fn frozen_mutations(&self) -> u32 {
        self.changes
            .iter()
            .filter(|c| {
                c.change_kind == 1
                    && self
                        .entries
                        .iter()
                        .find(|e| e.id == c.entry_id)
                        .map(|e| e.stability == 0)
                        .unwrap_or(false)
            })
            .count() as u32
    }

    /// 无消费方的孤儿接口。
    pub fn orphan_count(&self) -> u32 {
        self.entries.iter().filter(|e| e.consumers.is_empty()).count() as u32
    }

    /// 版本推进。
    pub fn bump_version(&mut self) {
        self.current_version += 1;
    }

    pub fn run_checks(&self) -> CheckSet {
        let mut cs = CheckSet::new("CGPU-F0305 B域接口总账");
        cs.add("接口 id 唯一", {
            let mut ids: Vec<u32> = self.entries.iter().map(|e| e.id).collect();
            ids.sort_unstable();
            ids.dedup();
            ids.len() == self.entries.len()
        }, "unique");
        cs.add("名称非空", self.entries.iter().all(|e| !e.name.is_empty()), "non-empty");
        cs.add("变更记录 id 有效", {
            self.changes.iter().all(|c| self.entries.iter().any(|e| e.id == c.entry_id))
        }, "valid refs");
        cs.add("变更版本不超当前", {
            self.changes.iter().all(|c| c.version <= self.current_version)
        }, "ver ≤ cur");
        cs.add_dyn("完整登记", self.is_complete(), &alloc::format!("unreg={}", self.unregistered_count));
        cs.add("Frozen 零变更", self.frozen_mutations() == 0, "no frozen mutation");
        cs.add("变更计数与记录一致", {
            self.entries.iter().all(|e| {
                let count = self.changes.iter().filter(|c| c.entry_id == e.id).count() as u32;
                count == e.change_count
            })
        }, "count match");
        cs.add("消费方 id 去重", {
            self.entries.iter().all(|e| {
                let mut c = e.consumers.clone();
                c.sort_unstable();
                c.dedup();
                c.len() == e.consumers.len()
            })
        }, "unique consumers");
        cs
    }
}

impl Default for IfaceLedger {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn register_and_audit() {
        let mut l = IfaceLedger::new();
        let id = l.register("scene_add_node", 0, 0xABCD, 0);
        l.add_consumer(id, 100);
        l.update_hash(id, 0xDCBA);
        assert_eq!(l.entries[0].change_count, 2);
        assert_eq!(l.frozen_mutations(), 1); // Frozen 被改
        assert!(l.is_complete());
    }

    #[test]
    fn orphan_detection() {
        let mut l = IfaceLedger::new();
        l.register("lonely", 1, 1, 1);
        assert_eq!(l.orphan_count(), 1);
    }
}
