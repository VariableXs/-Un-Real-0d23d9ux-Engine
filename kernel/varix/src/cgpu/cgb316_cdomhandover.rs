// CGPU-F0316 C域移交包 · cgb316_cdomhandover.rs
// AI-MIMO-1022 · CGPU-B 收口
// 判据：B→C 移交包含接口面/预留点/风险/测试基线/性能基线；
//       移交完整性自检；双向签收；遗留项清单。
#![allow(dead_code)]

extern crate alloc;
use alloc::vec::Vec;
use alloc::string::String;

use super::super::checks::CheckSet;

/// 移交清单项。
#[derive(Clone, Debug)]
pub struct HandoverItem {
    pub id: u32,
    pub category: u8, // 0=接口 1=预留 2=风险 3=测试 4=性能 5=文档
    pub name_id: u32,
    pub value: u64,
    pub delivered: bool,
    pub received: bool,
}

/// 遗留项。
#[derive(Clone, Debug)]
pub struct Leftover {
    pub id: u32,
    pub description_id: u32,
    pub severity: u8,
    pub target_group: String,
    pub resolved: bool,
}

/// 移交包。
pub struct HandoverBundle {
    pub items: Vec<HandoverItem>,
    pub leftovers: Vec<Leftover>,
    pub from_group: String,
    pub to_group: String,
    pub signed_send: Option<u32>,
    pub signed_recv: Option<u32>,
    pub completeness_q8: u16,
}

impl HandoverBundle {
    pub fn new(from: &str, to: &str) -> Self {
        Self {
            items: Vec::new(),
            leftovers: Vec::new(),
            from_group: String::from(from),
            to_group: String::from(to),
            signed_send: None,
            signed_recv: None,
            completeness_q8: 0,
        }
    }

    pub fn add_item(&mut self, category: u8, name_id: u32, value: u64) -> u32 {
        let id = self.items.len() as u32 + 1;
        self.items.push(HandoverItem {
            id,
            category,
            name_id,
            value,
            delivered: false,
            received: false,
        });
        id
    }

    pub fn mark_delivered(&mut self, id: u32) -> bool {
        if let Some(i) = self.items.iter_mut().find(|i| i.id == id) {
            i.delivered = true;
            true
        } else {
            false
        }
    }

    pub fn mark_received(&mut self, id: u32) -> bool {
        if let Some(i) = self.items.iter_mut().find(|i| i.id == id) {
            i.received = true;
            true
        } else {
            false
        }
    }

    pub fn add_leftover(&mut self, description_id: u32, severity: u8, target: &str) {
        self.leftovers.push(Leftover {
            id: self.leftovers.len() as u32 + 1,
            description_id,
            severity,
            target_group: String::from(target),
            resolved: false,
        });
    }

    /// 完备性计算。
    pub fn compute_completeness(&mut self) -> u16 {
        if self.items.is_empty() {
            self.completeness_q8 = 256;
            return 256;
        }
        let done = self.items.iter().filter(|i| i.delivered && i.received).count() as u64;
        self.completeness_q8 = ((done * 256) / self.items.len() as u64) as u16;
        self.completeness_q8
    }

    /// 双签。
    pub fn sign_send(&mut self, signer: u32) -> bool {
        self.signed_send = Some(signer);
        true
    }

    pub fn sign_recv(&mut self, signer: u32) -> bool {
        self.compute_completeness();
        if self.completeness_q8 < 200 {
            return false;
        }
        self.signed_recv = Some(signer);
        true
    }

    /// 六类齐备。
    pub fn category_coverage(&self) -> u32 {
        let mut covered = 0u32;
        for cat in 0..6u8 {
            if self.items.iter().any(|i| i.category == cat) {
                covered += 1;
            }
        }
        covered
    }

    pub fn run_checks(&self) -> CheckSet {
        let mut cs = CheckSet::new("CGPU-F0316 C域移交包");
        cs.add("移交项 id 唯一", {
            let mut ids: Vec<u32> = self.items.iter().map(|i| i.id).collect();
            ids.sort_unstable();
            ids.dedup();
            ids.len() == self.items.len()
        }, "unique");
        cs.add("六类覆盖 ≥ 4", self.category_coverage() >= 4, ">=4/6");
        cs.add("完备率 ≤ 256", self.completeness_q8 <= 256, "q8");
        cs.add("收方签署有门槛", {
            self.signed_recv.is_none() || self.completeness_q8 >= 200
        }, "recv gate");
        cs.add("遗留项有目标组", {
            self.leftovers.iter().all(|l| !l.target_group.is_empty())
        }, "target");
        cs.add("来源/目标组非空", {
            !self.from_group.is_empty() && !self.to_group.is_empty()
        }, "groups");
        cs.add("已签收的均已交付", {
            self.items.iter().filter(|i| i.received).all(|i| i.delivered)
        }, "recv ⊆ delivered");
        cs
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn full_handover() {
        let mut h = HandoverBundle::new("B09", "C01");
        for cat in 0..6u8 {
            for j in 0..3 {
                let id = h.add_item(cat, (cat * 10 + j) as u32, j as u64);
                h.mark_delivered(id);
                h.mark_received(id);
            }
        }
        h.compute_completeness();
        assert!(h.sign_recv(2));
    }

    #[test]
    fn incomplete_blocks_recv() {
        let mut h = HandoverBundle::new("B09", "C01");
        h.add_item(0, 1, 1); // 未交付
        assert!(!h.sign_recv(1));
    }
}
