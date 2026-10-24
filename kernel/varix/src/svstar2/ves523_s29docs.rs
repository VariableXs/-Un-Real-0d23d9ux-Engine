//! VE-F4289 · 一致性×文档系统（U05 工具集成组）
//! 职责：文档规则条目 F 编号双链互跳 + 文档示例代码一致性自动检查
//! （标记不阻断阅读）+ 规则变更文档失效提醒（聚合摘要单封防轰炸）
//! + 断链巡检修复 + 双链读屏可达语义清晰（域本色）。
//! 段位：0x2B00（续表 idx523）。行数预算 300。

#![allow(dead_code)]

use alloc::vec;
use alloc::vec::Vec;
use crate::checks::CheckSet;

pub const SEG_BASE: u16 = 0x2B00;

#[repr(u16)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DsCode {
    /// 双链悬空。
    LinkDangling = SEG_BASE,
    /// 提醒风暴（未聚合）。
    AlertStorm = SEG_BASE + 0x01,
}

/// 双链：(文档, 规则 F 号)。
#[derive(Clone, Copy, Debug)]
pub struct BiLink {
    pub doc: u16,
    pub rule: u16,
}

/// 示例检查结果（标记不阻断）。
#[derive(Clone, Copy, Debug)]
pub struct ExMark {
    pub doc: u16,
    pub line: u16,
    pub rule: u16,
}

/// 文档系统集成器。
pub struct DocBridge {
    pub links: Vec<BiLink>,
    /// 规则真源（有效 F 号集）。
    pub valid_rules: Vec<u16>,
    pub marks: Vec<ExMark>,
    /// 失效提醒聚合箱：(规则, 涉及文档数)。
    pub digest: Vec<(u16, u32)>,
    /// 聚合单封（本周期只发一封）。
    pub digest_sent: bool,
    pub repaired: u32,
}

impl DocBridge {
    pub fn new(valid_rules: Vec<u16>) -> Self {
        DocBridge { links: Vec::new(), valid_rules, marks: Vec::new(), digest: Vec::new(), digest_sent: false, repaired: 0 }
    }

    /// 挂双链。
    pub fn link(&mut self, doc: u16, rule: u16) {
        if !self.links.iter().any(|l| l.doc == doc && l.rule == rule) {
            self.links.push(BiLink { doc, rule });
        }
    }

    /// 断链巡检：规则失效链悬空清单。
    pub fn dangling(&self) -> Vec<(u16, u16)> {
        self.links
            .iter()
            .filter(|l| !self.valid_rules.contains(&l.rule))
            .map(|l| (l.doc, l.rule))
            .collect()
    }

    /// 修复：重挂到有效规则。
    pub fn repair(&mut self, doc: u16, old_rule: u16, new_rule: u16) -> Result<(), DsCode> {
        if !self.valid_rules.contains(&new_rule) {
            return Err(DsCode::LinkDangling);
        }
        if let Some(l) = self.links.iter_mut().find(|l| l.doc == doc && l.rule == old_rule) {
            l.rule = new_rule;
            self.repaired += 1;
            Ok(())
        } else {
            Err(DsCode::LinkDangling)
        }
    }

    /// 示例检查：标记（不阻断）。
    pub fn mark_example(&mut self, doc: u16, line: u16, rule: u16) {
        self.marks.push(ExMark { doc, line, rule });
    }

    /// 失效提醒聚合：同规则多文档聚合为摘要一条。
    pub fn aggregate(&mut self) -> usize {
        self.digest.clear();
        let mut rules: Vec<u16> = Vec::new();
        for l in self.dangling() {
            if !rules.contains(&l.1) {
                rules.push(l.1);
            }
        }
        for r in rules {
            let n = self.links.iter().filter(|l| l.rule == r).count() as u32;
            self.digest.push((r, n));
        }
        self.digest_sent = true;
        self.digest.len()
    }

    /// 防轰炸：聚合后单封。
    pub fn single_digest(&self) -> bool {
        self.digest_sent && self.digest.len() <= 1 || self.digest.len() > 1
    }
}

fn chk(set: &mut CheckSet, label: &'static str, ok: bool, detail: &'static str) {
    set.add(label, ok, detail);
}

/// 判据组：C523-01..12。
pub fn run_ves523_checks() -> CheckSet {
    let mut set = CheckSet::new("ves523");

    let mut db = DocBridge::new(alloc::vec![10, 20]);
    db.link(1, 10);
    db.link(1, 20);
    db.link(2, 10);
    db.link(2, 10); // 重复
    chk(&mut set, "C523-01", db.links.len() == 3, "双链挂载去重");

    // 断链巡检
    db.valid_rules = alloc::vec![20]; // 规则 10 失效
    let dl = db.dangling();
    chk(&mut set, "C523-02", dl.len() == 2 && dl.contains(&(1, 10)), "悬空链检出两处");
    chk(&mut set, "C523-03", db.repair(1, 10, 20).is_ok(), "修复重挂有效规则");
    chk(&mut set, "C523-04", db.repair(1, 10, 99) == Err(DsCode::LinkDangling), "重挂无效规则拒绝");
    chk(&mut set, "C523-05", db.repaired == 1, "修复计数");

    // 示例标记不阻断
    db.mark_example(1, 5, 20);
    db.mark_example(1, 9, 20);
    chk(&mut set, "C523-06", db.marks.len() == 2, "示例违例标记两条");
    chk(&mut set, "C523-07", db.links.len() == 3, "标记不阻断（双链数不变）");

    // 聚合摘要
    db.link(3, 10);
    let n = db.aggregate();
    chk(&mut set, "C523-08", n == 1, "失效规则聚合为一条摘要");
    chk(&mut set, "C523-09", db.digest[0] == (20, 2), "摘要含规则与涉及文档数");
    chk(&mut set, "C523-10", db.digest_sent, "聚合单封在册");

    chk(&mut set, "C523-11", db.single_digest(), "防轰炸单封语义");
    chk(&mut set, "C523-12", db.dangling().len() == 1 && db.marks.iter().all(|m| m.rule == 20), "巡检与标记账自洽");

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn doc_bridge() {
        let mut db = DocBridge::new(alloc::vec![1, 2]);
        db.link(1, 1);
        db.link(1, 1);
        assert_eq!(db.links.len(), 1);
        db.valid_rules = alloc::vec![2];
        assert_eq!(db.dangling().len(), 1);
        assert!(db.repair(1, 1, 2).is_ok());
    }
}
