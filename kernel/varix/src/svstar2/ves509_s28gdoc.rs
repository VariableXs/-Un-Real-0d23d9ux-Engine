//! VE-F4275 · 治理文档（U04 治理组）
//! 职责：文档族四类（治理手册/流程图/判例集/FAQ）+ 版本随规则联动（CI 检测
//! 滞后阻断合并）+ 判例冲突仲裁复核 + FAQ 过时降权归档不删除 + 双轨入口。
//! 段位：0x2860（续表 idx509）。行数预算 300。

#![allow(dead_code)]

use alloc::vec;
use alloc::vec::Vec;
use crate::checks::CheckSet;

pub const SEG_BASE: u16 = 0x2860;

#[repr(u16)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum GdCode {
    /// 文档滞后（CI 阻断）。
    Stale = SEG_BASE,
    /// 判例冲突（仲裁复核）。
    CaseConflict = SEG_BASE + 0x01,
}

/// 文档族四类。
#[repr(u8)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum GDocKind {
    Manual = 0,
    FlowChart = 1,
    Cases = 2,
    Faq = 3,
}
pub const GDOC_N: usize = 4;

/// 文档条目。
#[derive(Clone, Copy, Debug)]
pub struct GDoc {
    pub kind: GDocKind,
    pub doc_ver: u32,
    /// 绑定规则版本。
    pub rule_ver: u32,
    /// 降权标记（FAQ 过时）。
    pub demoted: bool,
    /// 双轨入口：新人/专家路径均可达。
    pub newbie_path: bool,
    pub expert_path: bool,
}

/// 治理文档册。
pub struct GovDocBook {
    pub docs: Vec<GDoc>,
    /// 判例冲突账。
    pub conflicts: Vec<u16>,
    pub today: u16,
}

impl GovDocBook {
    pub fn new(rule_ver: u32) -> Self {
        let _ = rule_ver;
        GovDocBook { docs: Vec::new(), conflicts: Vec::new(), today: 0 }
    }

    /// 登记。
    pub fn add(&mut self, d: GDoc) {
        self.docs.push(d);
    }

    /// 版本联动检：文档版本必须 ≥ 规则版本（滞后→阻断合并）。
    pub fn ci_check(&self, rule_ver: u32) -> Result<(), GdCode> {
        for d in &self.docs {
            if d.doc_ver < rule_ver {
                return Err(GdCode::Stale);
            }
        }
        Ok(())
    }

    /// 判例冲突：同题判例双答→仲裁复核登记。
    pub fn case_conflict(&mut self, topic: u16) {
        if !self.conflicts.contains(&topic) {
            self.conflicts.push(topic);
        }
    }

    /// FAQ 过时降权（归档不删除）。
    pub fn demote_faq(&mut self, idx: usize) -> bool {
        if let Some(d) = self.docs.get_mut(idx) {
            if d.kind == GDocKind::Faq {
                d.demoted = true;
                return true;
            }
        }
        false
    }

    /// 降权 FAQ 仍在册（不删除）。
    pub fn demoted_retained(&self) -> usize {
        self.docs.iter().filter(|d| d.demoted).count()
    }

    /// 双轨入口：两路均可达。
    pub fn dual_entry_ok(&self, idx: usize) -> bool {
        self.docs.get(idx).map(|d| d.newbie_path && d.expert_path).unwrap_or(false)
    }

    /// 四族齐备。
    pub fn all_kinds(&self) -> bool {
        (0..GDOC_N).all(|k| self.docs.iter().any(|d| d.kind as usize == k))
    }
}

fn chk(set: &mut CheckSet, label: &'static str, ok: bool, detail: &'static str) {
    set.add(label, ok, detail);
}

/// 判据组：C509-01..12。
pub fn run_ves509_checks() -> CheckSet {
    let mut set = CheckSet::new("ves509");

    let mut gd = GovDocBook::new(5);
    for k in [GDocKind::Manual, GDocKind::FlowChart, GDocKind::Cases, GDocKind::Faq] {
        gd.add(GDoc { kind: k, doc_ver: 5, rule_ver: 5, demoted: false, newbie_path: true, expert_path: true });
    }
    chk(&mut set, "C509-01", gd.all_kinds(), "四族文档齐备");
    chk(&mut set, "C509-02", gd.ci_check(5).is_ok(), "版本联动通过（5≥5）");
    chk(&mut set, "C509-03", gd.ci_check(6) == Err(GdCode::Stale), "规则升 6 文档滞 5→CI 阻断合并");
    chk(&mut set, "C509-04", gd.dual_entry_ok(0) && !gd.dual_entry_ok(99), "双轨入口校验（越界拒绝）");

    // 判例冲突
    gd.case_conflict(10);
    gd.case_conflict(10);
    gd.case_conflict(11);
    chk(&mut set, "C509-05", gd.conflicts == alloc::vec![10, 11], "判例冲突去重登记");
    chk(&mut set, "C509-06", gd.conflicts.len() == 2, "仲裁复核清单两题");

    // FAQ 降权归档
    let faq_idx = gd.docs.iter().position(|d| d.kind == GDocKind::Faq).unwrap_or(0);
    chk(&mut set, "C509-07", gd.demote_faq(faq_idx), "FAQ 过时降权");
    chk(&mut set, "C509-08", gd.demoted_retained() == 1, "降权仍在册（不删除）");
    let manual_idx = gd.docs.iter().position(|d| d.kind == GDocKind::Manual).unwrap_or(0);
    chk(&mut set, "C509-09", !gd.demote_faq(manual_idx), "非 FAQ 拒绝降权");

    // 修订后联动
    for d in gd.docs.iter_mut() {
        d.doc_ver = 6;
    }
    chk(&mut set, "C509-10", gd.ci_check(6).is_ok(), "全册修订后联动恢复");
    chk(&mut set, "C509-11", gd.docs.iter().all(|d| d.dual_ok()), "双轨入口全量在册");
    chk(&mut set, "C509-12", gd.docs.iter().filter(|d| d.kind == GDocKind::Faq).all(|d| d.demoted), "FAQ 降权标记保留");

    set
}

impl GDoc {
    fn dual_ok(&self) -> bool {
        self.newbie_path && self.expert_path
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gov_docs() {
        let mut gd = GovDocBook::new(1);
        gd.add(GDoc { kind: GDocKind::Faq, doc_ver: 1, rule_ver: 1, demoted: false, newbie_path: true, expert_path: false });
        assert_eq!(gd.ci_check(2), Err(GdCode::Stale));
        assert!(gd.demote_faq(0));
        assert_eq!(gd.demoted_retained(), 1);
    }
}
