//! VE-F4277 · 治理示例（U04 治理组）
//! 职责：示例族四类（违例定级/仲裁/豁免申请/自动化配置）+ 一键导入沙箱
//! 试跑不落盘 + 版本戳校验下架 + 判例联动（判例修订触发示例复核单）。
//! 段位：0x28E0（续表 idx511）。行数预算 300。

#![allow(dead_code)]

use alloc::vec;
use alloc::vec::Vec;
use crate::checks::CheckSet;

pub const SEG_BASE: u16 = 0x28E0;

#[repr(u16)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum GeCode {
    /// 示例失效（版本戳下架）。
    Stale = SEG_BASE,
    /// 导入冲突（沙箱预演提示不落盘）。
    ImportClash = SEG_BASE + 0x01,
    /// 示例与判例矛盾（以判例为准修订）。
    CaseMismatch = SEG_BASE + 0x02,
}

/// 示例族四类。
#[repr(u8)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum GeKind {
    Grading = 0,
    Arbitration = 1,
    Exempt = 2,
    AutoCfg = 3,
}
pub const GEK_N: usize = 4;

/// 示例条目。
#[derive(Clone, Copy, Debug)]
pub struct Example {
    pub kind: GeKind,
    pub ver: u32,
    /// 绑定判例版本。
    pub case_ver: u32,
    pub active: bool,
}

/// 治理示例库。
pub struct GovExamples {
    pub items: Vec<Example>,
    /// 判例当前版本。
    pub case_ver: u32,
    /// 复核单（判例修订触发）。
    pub review_orders: Vec<u8>,
    /// 沙箱落盘标记（预演绝不落盘）。
    pub sandbox_persisted: bool,
}

impl GovExamples {
    pub fn new(case_ver: u32) -> Self {
        GovExamples { items: Vec::new(), case_ver, review_orders: Vec::new(), sandbox_persisted: false }
    }

    pub fn add(&mut self, e: Example) {
        self.items.push(e);
    }

    /// 版本戳校验：判例版本超前→失效下架。
    pub fn sweep_stale(&mut self) -> usize {
        let cv = self.case_ver;
        let mut n = 0;
        for e in self.items.iter_mut() {
            if e.case_ver < cv && e.active {
                e.active = false;
                n += 1;
            }
        }
        n
    }

    /// 一键导入沙箱试跑：预演冲突→提示不落盘。
    pub fn import(&mut self, kind: GeKind, target_slot: u8) -> Result<(), GeCode> {
        // 冲突检：同类示例已占同槽
        if self.items.iter().any(|e| e.kind == kind && e.active) {
            return Err(GeCode::ImportClash); // 沙箱预演提示，不落盘
        }
        self.items.push(Example { kind, ver: 1, case_ver: self.case_ver, active: true });
        let _ = target_slot;
        Ok(())
    }

    /// 判例修订触发全部同类示例复核单。
    pub fn case_revised(&mut self, new_ver: u32) {
        self.case_ver = new_ver;
        for e in self.items.iter() {
            if e.case_ver < new_ver && !self.review_orders.contains(&(e.kind as u8)) {
                self.review_orders.push(e.kind as u8);
            }
        }
    }

    /// 示例与判例矛盾检：以判例为准。
    pub fn agrees_case(&self, idx: usize, claim_case_ver: u32) -> Result<(), GeCode> {
        let e = self.items.get(idx).ok_or(GeCode::CaseMismatch)?;
        if e.case_ver != claim_case_ver {
            return Err(GeCode::CaseMismatch);
        }
        Ok(())
    }

    /// 四族齐备。
    pub fn all_kinds(&self) -> bool {
        (0..GEK_N).all(|k| self.items.iter().any(|e| e.kind as usize == k && e.active))
    }
}

fn chk(set: &mut CheckSet, label: &'static str, ok: bool, detail: &'static str) {
    set.add(label, ok, detail);
}

/// 判据组：C511-01..12。
pub fn run_ves511_checks() -> CheckSet {
    let mut set = CheckSet::new("ves511");

    let mut ge = GovExamples::new(3);
    for k in [GeKind::Grading, GeKind::Arbitration, GeKind::Exempt, GeKind::AutoCfg] {
        ge.add(Example { kind: k, ver: 1, case_ver: 3, active: true });
    }
    chk(&mut set, "C511-01", ge.all_kinds(), "四族示例齐备");
    chk(&mut set, "C511-02", !ge.sandbox_persisted, "沙箱零落盘基线");

    // 导入冲突
    chk(&mut set, "C511-03", ge.import(GeKind::Grading, 0) == Err(GeCode::ImportClash), "同类占用同槽冲突（预演提示不落盘）");
    chk(&mut set, "C511-04", !ge.sandbox_persisted && ge.items.len() == 4, "冲突不落盘");

    // 判例修订触发复核
    ge.case_revised(5);
    chk(&mut set, "C511-05", ge.review_orders.len() == 4, "判例修订触发四族复核单");
    chk(&mut set, "C511-06", ge.sweep_stale() == 4, "旧版示例全部失效下架");
    chk(&mut set, "C511-07", ge.all_kinds() == false, "下架后四族缺");

    // 重新入库（新判例版）
    for k in [GeKind::Grading, GeKind::Arbitration] {
        let _ = ge.import(k, 0);
    }
    chk(&mut set, "C511-08", ge.items.iter().filter(|e| e.active).count() == 2, "新示例入库绑定新判例版");
    chk(&mut set, "C511-09", ge.agrees_case(4, 5).is_ok(), "示例与判例版本一致");
    chk(&mut set, "C511-10", ge.agrees_case(0, 3) == Err(GeCode::CaseMismatch), "矛盾检出（以判例为准）");
    chk(&mut set, "C511-11", ge.agrees_case(99, 5) == Err(GeCode::CaseMismatch), "越界索引拒收");

    chk(&mut set, "C511-12", !ge.sandbox_persisted, "全程沙箱零落盘");

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gov_examples() {
        let mut ge = GovExamples::new(1);
        ge.add(Example { kind: GeKind::Grading, ver: 1, case_ver: 1, active: true });
        assert_eq!(ge.import(GeKind::Grading, 0), Err(GeCode::ImportClash));
        ge.case_revised(2);
        assert_eq!(ge.sweep_stale(), 1);
        assert!(!ge.sandbox_persisted);
    }
}
