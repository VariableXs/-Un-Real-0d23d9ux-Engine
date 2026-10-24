//! VE-F4304 · 一致性×版本发布（U06 场景应用组）
//! 职责：发布前全量扫描+基线对拍 + 发布说明一致性报告双级（摘要+全文）
//! + 发布后回归窗热修绿色通道（带事后追认——追认拒绝即回滚）
//! + 扫描不过发布阻断出整改包 + 报告缺失禁发 + 无障碍格式强制。
//! 段位：0x3040（续表 idx538）。行数预算 320。

#![allow(dead_code)]

use alloc::vec;
use alloc::vec::Vec;
use crate::checks::CheckSet;

pub const SEG_BASE: u16 = 0x3040;

#[repr(u16)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RlCode {
    /// 扫描不过（发布阻断出整改包）。
    ScanFail = SEG_BASE,
    /// 报告缺失（禁发）。
    ReportMissing = SEG_BASE + 0x01,
    /// 追认拒绝即回滚。
    RatifyReject = SEG_BASE + 0x02,
    /// 报告无障碍格式缺（域色）。
    A11yFmtMissing = SEG_BASE + 0x03,
}

/// 双级报告。
#[derive(Clone, Copy, Debug)]
pub struct RelReport {
    pub summary: bool, // 摘要级
    pub full: bool,    // 全文级
    pub a11y_fmt: bool, // 无障碍格式（域色强制）
}

impl RelReport {
    pub fn ok(&self) -> bool {
        self.summary && self.full && self.a11y_fmt
    }
}

/// 热修绿色通道条目。
#[derive(Clone, Copy, Debug)]
pub struct Hotfix {
    pub id: u16,
    pub ratified: Option<bool>, // None=待追认
}

/// 发布闸。
pub struct ReleaseGate {
    /// 全量扫描结果：(规则, 过)。
    pub scan: Vec<(u16, bool)>,
    /// 基线快照。
    pub baseline: Vec<(u16, u32)>,
    /// 当前值。
    pub current: Vec<(u16, u32)>,
    pub report: Option<RelReport>,
    pub hotfixes: Vec<Hotfix>,
    pub remedy: Vec<u16>,
    pub rolled_back: u32,
}

impl ReleaseGate {
    pub fn new() -> Self {
        ReleaseGate { scan: Vec::new(), baseline: Vec::new(), current: Vec::new(), report: None, hotfixes: Vec::new(), remedy: Vec::new(), rolled_back: 0 }
    }

    pub fn set_scan(&mut self, s: Vec<(u16, bool)>) {
        self.scan = s;
    }

    /// 全量扫描对拍基线：漂移项即失败。
    pub fn full_check(&mut self) -> Result<(), RlCode> {
        self.remedy.clear();
        let mut fail = false;
        for (r, ok) in &self.scan {
            if !ok {
                self.remedy.push(*r);
                fail = true;
            }
        }
        // 基线漂移
        for (k, bv) in &self.baseline {
            if let Some((_, cv)) = self.current.iter().find(|(ck, _)| ck == k) {
                if cv != bv {
                    self.remedy.push(*k + 10_000);
                    fail = true;
                }
            }
        }
        if fail {
            Err(RlCode::ScanFail)
        } else {
            Ok(())
        }
    }

    /// 发布：报告齐备+扫描过。
    pub fn release(&self) -> Result<(), RlCode> {
        match &self.report {
            None => Err(RlCode::ReportMissing),
            Some(r) if !r.ok() => {
                if !r.a11y_fmt {
                    Err(RlCode::A11yFmtMissing)
                } else {
                    Err(RlCode::ReportMissing)
                }
            }
            Some(_) => self.full_check_pass(),
        }
    }

    fn full_check_pass(&self) -> Result<(), RlCode> {
        if self.scan.iter().any(|(_, ok)| !ok) {
            return Err(RlCode::ScanFail);
        }
        Ok(())
    }

    /// 热修绿色通道：登记待追认。
    pub fn hotfix(&mut self, id: u16) {
        self.hotfixes.push(Hotfix { id, ratified: None });
    }

    /// 事后追认：拒绝即回滚。
    pub fn ratify(&mut self, id: u16, ok: bool) -> Result<(), RlCode> {
        let h = self.hotfixes.iter_mut().find(|h| h.id == id && h.ratified.is_none()).ok_or(RlCode::RatifyReject)?;
        if ok {
            h.ratified = Some(true);
            Ok(())
        } else {
            h.ratified = Some(false);
            self.rolled_back += 1;
            Err(RlCode::RatifyReject)
        }
    }
}

fn chk(set: &mut CheckSet, label: &'static str, ok: bool, detail: &'static str) {
    set.add(label, ok, detail);
}

/// 判据组：C538-01..12。
pub fn run_ves538_checks() -> CheckSet {
    let mut set = CheckSet::new("ves538");

    let mut rg = ReleaseGate::new();
    rg.set_scan(alloc::vec![(1, true), (2, true)]);
    rg.baseline = alloc::vec![(10, 100)];
    rg.current = alloc::vec![(10, 100)];
    chk(&mut set, "C538-01", rg.full_check().is_ok(), "扫描全过+基线对齐");
    rg.report = Some(RelReport { summary: true, full: true, a11y_fmt: true });
    chk(&mut set, "C538-02", rg.release().is_ok(), "全条件发布放行");

    // 扫描失败
    rg.set_scan(alloc::vec![(1, true), (2, false)]);
    chk(&mut set, "C538-03", rg.release() == Err(RlCode::ScanFail), "扫描不过发布阻断");
    chk(&mut set, "C538-04", rg.remedy == alloc::vec![2], "整改包定位失败项");
    rg.current[0].1 = 999;
    let _ = rg.full_check();
    chk(&mut set, "C538-05", rg.remedy.contains(&(2)) && rg.remedy.contains(&(10_010)), "基线漂移入整改包（10010=键10+万位标）");

    // 报告缺失
    rg.set_scan(alloc::vec![(1, true)]);
    rg.current[0].1 = 100;
    rg.report = None;
    chk(&mut set, "C538-06", rg.release() == Err(RlCode::ReportMissing), "报告缺失禁发");
    rg.report = Some(RelReport { summary: true, full: true, a11y_fmt: false });
    chk(&mut set, "C538-07", rg.release() == Err(RlCode::A11yFmtMissing), "无障碍格式缺拒绝（域色）");

    // 热修追认
    rg.hotfix(7);
    chk(&mut set, "C538-08", rg.ratify(7, true).is_ok(), "热修追认通过");
    rg.hotfix(8);
    chk(&mut set, "C538-09", rg.ratify(8, false) == Err(RlCode::RatifyReject), "追认拒绝即回滚");
    chk(&mut set, "C538-10", rg.rolled_back == 1, "回滚计数");
    chk(&mut set, "C538-11", rg.ratify(8, true) == Err(RlCode::RatifyReject), "已裁断不可重复追认");
    chk(&mut set, "C538-12", rg.hotfixes.iter().all(|h| h.ratified.is_some()), "热修账全裁断");

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn release_gate() {
        let mut rg = ReleaseGate::new();
        rg.set_scan(alloc::vec![(1, true)]);
        assert!(rg.release().is_err()); // 无报告
        rg.report = Some(RelReport { summary: true, full: true, a11y_fmt: true });
        assert!(rg.release().is_ok());
        rg.hotfix(1);
        assert!(rg.ratify(1, false).is_err());
    }
}
