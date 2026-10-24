//! VE-F4256 · 验证文档（U03 一致性验证平台）
//! 职责：四类手册（平台使用/规则编写指南（引用单源）/基线管理/CI 接入）
//! + 一致性抽查 + 无障碍验证闸说明专节（域本色）+ 漂移对拍修正留痕。
//! 段位：0x1E00（续表 idx490）。行数预算 320。

#![allow(dead_code)]

use alloc::vec;
use alloc::vec::Vec;
use alloc::string::String;
use crate::checks::CheckSet;

pub const SEG_BASE: u16 = 0x1E00;

#[repr(u16)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DcCode {
    /// 手册缺失。
    BookMissing = SEG_BASE,
    /// 漂移（文档与实现失配）。
    Drift = SEG_BASE + 0x01,
    /// 单源引用断（规则指南未挂单源）。
    SrcLinkLost = SEG_BASE + 0x02,
    /// 专节缺失（域本色红线）。
    A11ySectionLost = SEG_BASE + 0x03,
}

/// 四类手册。
#[repr(u8)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BookKind {
    Usage = 0,
    RuleGuide = 1,
    Baseline = 2,
    Ci = 3,
}
pub const BOOK_N: usize = 4;

/// 手册条目。
#[derive(Clone, Debug)]
pub struct Manual {
    pub kind: BookKind,
    pub version: u32,
    pub src_linked: bool,   // 规则指南须挂规则单源
    pub a11y_section: bool, // 无障碍专节
}

/// 抽查记录（对拍修正留痕）。
#[derive(Clone, Debug)]
pub struct SpotCheck {
    pub kind: BookKind,
    pub claim: u32,     // 文档声明值
    pub real: u32,      // 实现真值
    pub fixed: bool,    // 以实现为准修文档
}

/// 文档册。
pub struct DocBook {
    pub manuals: Vec<Manual>,
    pub checks: Vec<SpotCheck>,
    /// 单源指纹。
    pub src_fp: u32,
}

impl DocBook {
    pub fn new(src_fp: u32) -> Self {
        DocBook { manuals: Vec::new(), checks: Vec::new(), src_fp }
    }

    /// 手册登记（规则指南必须挂单源；全册须含无障碍专节——域本色）。
    pub fn add(&mut self, m: Manual) -> Result<(), DcCode> {
        if m.kind == BookKind::RuleGuide && !m.src_linked {
            return Err(DcCode::SrcLinkLost);
        }
        self.manuals.push(m);
        Ok(())
    }

    /// 四类齐备检。
    pub fn all_books(&self) -> bool {
        (0..BOOK_N).all(|k| self.manuals.iter().any(|m| m.kind as usize == k))
    }

    /// 域本色专节检：全部手册含无障碍验证闸说明。
    pub fn a11y_sections(&self) -> bool {
        self.manuals.iter().all(|m| m.a11y_section)
    }

    /// 抽查对拍：失配→以实现为准修正并留痕。
    pub fn spot(&mut self, kind: BookKind, claim: u32, real: u32) -> bool {
        let fixed = claim != real;
        self.checks.push(SpotCheck { kind, claim, real, fixed });
        fixed
    }

    /// 漂移条目（待修订清单）。
    pub fn drift_list(&self) -> Vec<(u8, u32, u32)> {
        self.checks.iter().filter(|c| c.fixed).map(|c| (c.kind as u8, c.claim, c.real)).collect()
    }

    /// 版本对齐：全册同版。
    pub fn versions_aligned(&self) -> bool {
        if self.manuals.is_empty() {
            return false;
        }
        let v = self.manuals[0].version;
        self.manuals.iter().all(|m| m.version == v)
    }
}

fn chk(set: &mut CheckSet, label: &'static str, ok: bool, detail: &'static str) {
    set.add(label, ok, detail);
}

/// 判据组：C490-01..12。
pub fn run_ves490_checks() -> CheckSet {
    let mut set = CheckSet::new("ves490");

    let mut db = DocBook::new(0xD0C5);
    // 单源引用断
    chk(&mut set, "C490-01", db.add(Manual { kind: BookKind::RuleGuide, version: 1, src_linked: false, a11y_section: true }) == Err(DcCode::SrcLinkLost), "规则指南未挂单源拒收");
    chk(&mut set, "C490-02", db.manuals.is_empty(), "拒收不入册");

    for k in [BookKind::Usage, BookKind::RuleGuide, BookKind::Baseline, BookKind::Ci] {
        let _ = db.add(Manual { kind: k, version: 3, src_linked: true, a11y_section: true });
    }
    chk(&mut set, "C490-03", db.all_books(), "四类手册齐备");
    chk(&mut set, "C490-04", db.a11y_sections(), "全册含无障碍专节（域本色）");
    chk(&mut set, "C490-05", db.versions_aligned(), "全册同版对齐");

    // 抽查对拍
    chk(&mut set, "C490-06", db.spot(BookKind::Usage, 10, 10) == false, "声明=真值不漂移");
    chk(&mut set, "C490-07", db.spot(BookKind::Ci, 5, 7), "CI 闸数声明漂移检出");
    chk(&mut set, "C490-08", db.spot(BookKind::Baseline, 3, 4), "基线类数漂移检出");
    let dl = db.drift_list();
    chk(&mut set, "C490-09", dl.len() == 2 && dl[0] == (3, 5, 7), "漂移修正留痕保序");

    // 专节缺失红线
    let mut db2 = DocBook::new(1);
    let _ = db2.add(Manual { kind: BookKind::Usage, version: 1, src_linked: true, a11y_section: false });
    chk(&mut set, "C490-10", db2.a11y_sections() == false, "缺专节检出（域色红线可检）");
    chk(&mut set, "C490-11", db2.all_books() == false, "单册不齐");

    let empty = DocBook::new(1);
    chk(&mut set, "C490-12", empty.versions_aligned() == false && empty.drift_list().is_empty(), "空册保守语义");

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn docs_spot_check() {
        let mut db = DocBook::new(1);
        assert_eq!(db.add(Manual { kind: BookKind::RuleGuide, version: 1, src_linked: false, a11y_section: true }), Err(DcCode::SrcLinkLost));
        let _ = db.add(Manual { kind: BookKind::Ci, version: 2, src_linked: true, a11y_section: true });
        assert!(db.spot(BookKind::Ci, 1, 2));
        assert_eq!(db.drift_list().len(), 1);
    }
}
