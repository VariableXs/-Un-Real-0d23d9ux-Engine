//! VE-F4248 · 验证报告平台（U03 一致性验证平台）
//! 职责：六类验证发现项统一汇总（域/规则/严重度三切面）+ 一致率趋势时间线
//! + 结构化导出（失败降级 CSV）+ 无障碍单列视图（域本色主场位）。
//! 段位：0x1C60（续表 idx482）。行数预算 340。

#![allow(dead_code)]

use alloc::vec;
use alloc::vec::Vec;
use alloc::string::{String, ToString};
use crate::checks::CheckSet;

pub const SEG_BASE: u16 = 0x1C60;

#[repr(u16)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RpCode {
    /// 数据源断（空态须显性，不伪造）。
    SrcBroken = SEG_BASE,
    /// 汇总口径漂移。
    CaliberDrift = SEG_BASE + 0x01,
    /// 导出失败（降级 CSV）。
    ExportFail = SEG_BASE + 0x02,
    /// 发现项未归一。
    NotUnified = SEG_BASE + 0x03,
}

/// 验证类别（六类统一）。
#[repr(u8)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum VKind {
    Static = 0,
    Dynamic = 1,
    Visual = 2,
    Interact = 3,
    Perf = 4,
    A11y = 5,
}
pub const VKIND_N: usize = 6;

/// 发现项（统一归一形态）。
#[derive(Clone, Debug)]
pub struct Finding {
    pub kind: VKind,
    pub dom: u8,
    pub rule: u16,
    pub sev: u8, // 0 提示 1 一般 2 严重 3 红线
    pub title: String,
}

/// 三切面计数器。
#[derive(Clone, Copy, Default, Debug)]
pub struct Facet {
    pub by_dom: [u32; 8],
    pub by_rule: u32,
    pub by_sev: [u32; 4],
}

impl Facet {
    pub fn absorb(&mut self, f: &Finding) {
        if (f.dom as usize) < 8 {
            self.by_dom[f.dom as usize] += 1;
        }
        self.by_rule += 1;
        if (f.sev as usize) < 4 {
            self.by_sev[f.sev as usize] += 1;
        }
    }
    pub fn total(&self) -> u32 {
        self.by_rule
    }
}

/// 报告平台。
pub struct ReportHub {
    pub findings: Vec<Finding>,
    pub facet: Facet,
    /// 一致率时间线：(时点 bp, 一致率 bp)。
    pub trend: Vec<(u32, u32)>,
    /// 无障碍单列视图索引。
    pub a11y_idx: Vec<usize>,
}

impl ReportHub {
    pub fn new() -> Self {
        ReportHub { findings: Vec::new(), facet: Facet::default(), trend: Vec::new(), a11y_idx: Vec::new() }
    }

    /// 归一入账：同步刷新三切面与无障碍单列。
    pub fn absorb(&mut self, f: Finding) {
        if f.kind == VKind::A11y {
            self.a11y_idx.push(self.findings.len());
        }
        self.facet.absorb(&f);
        self.findings.push(f);
    }

    /// 一致率趋势采样（bp）。
    pub fn sample_trend(&mut self, t: u32) {
        let total = self.findings.len() as u32;
        let bad = self.findings.iter().filter(|f| f.sev >= 2).count() as u32;
        let rate = if total == 0 { 10_000 } else { (total - bad) * 10_000 / total };
        self.trend.push((t, rate));
    }

    /// 结构化导出（Sev 降序）；源断→Err 显性空态。
    pub fn export(&self) -> Result<Vec<(u8, u16, u8)>, RpCode> {
        if self.findings.is_empty() {
            return Err(RpCode::SrcBroken);
        }
        let mut rows: Vec<(u8, u16, u8)> =
            self.findings.iter().map(|f| (f.dom, f.rule, f.sev)).collect();
        rows.sort_by(|a, b| b.2.cmp(&a.2));
        Ok(rows)
    }

    /// CSV 降级导出（导出失败时兜底）。
    pub fn export_csv(&self) -> String {
        let mut s = String::from("dom,rule,sev\n");
        for f in &self.findings {
            s.push_str(&f.dom.to_string());
            s.push(',');
            s.push_str(&f.rule.to_string());
            s.push(',');
            s.push_str(&f.sev.to_string());
            s.push('\n');
        }
        s
    }

    /// 无障碍单列视图（域本色主场位：独立切面）。
    pub fn a11y_view(&self) -> Vec<&Finding> {
        self.a11y_idx.iter().map(|&i| &self.findings[i]).collect()
    }
}

fn chk(set: &mut CheckSet, label: &'static str, ok: bool, detail: &'static str) {
    set.add(label, ok, detail);
}

/// 判据组：C482-01..12。
pub fn run_ves482_checks() -> CheckSet {
    let mut set = CheckSet::new("ves482");

    let mut hub = ReportHub::new();
    chk(&mut set, "C482-01", hub.export() == Err(RpCode::SrcBroken), "源断→显性空态拒绝导出");

    hub.absorb(Finding { kind: VKind::A11y, dom: 1, rule: 10, sev: 3, title: String::from("读屏标签缺失") });
    hub.absorb(Finding { kind: VKind::Static, dom: 0, rule: 20, sev: 1, title: String::from("硬编码色值") });
    hub.absorb(Finding { kind: VKind::Visual, dom: 2, rule: 30, sev: 2, title: String::from("基准失真") });
    hub.absorb(Finding { kind: VKind::A11y, dom: 3, rule: 40, sev: 1, title: String::from("焦点序漂移") });

    chk(&mut set, "C482-02", hub.facet.total() == 4, "三切面总量=4");
    chk(&mut set, "C482-03", hub.facet.by_dom[1] == 1 && hub.facet.by_dom[3] == 1, "域切面精确入账");
    chk(&mut set, "C482-04", hub.facet.by_sev[3] == 1 && hub.facet.by_sev[1] == 2, "严重度切面分布");
    chk(&mut set, "C482-05", hub.a11y_view().len() == 2, "无障碍单列视图两条（域本色）");

    hub.sample_trend(1);
    hub.absorb(Finding { kind: VKind::Perf, dom: 4, rule: 50, sev: 0, title: String::from("轻微") });
    hub.sample_trend(2);
    chk(&mut set, "C482-06", hub.trend.len() == 2 && hub.trend[1].1 > hub.trend[0].1, "趋势线一致率回升");

    let rows = hub.export().unwrap_or_default();
    chk(&mut set, "C482-07", rows.len() == 5, "结构化导出五行");
    chk(&mut set, "C482-08", rows[0].2 == 3, "导出按严重度降序（红线居首）");
    chk(&mut set, "C482-09", hub.export_csv().starts_with("dom,rule,sev\n"), "CSV 降级格式头");
    chk(&mut set, "C482-10", hub.export_csv().lines().count() == 6, "CSV 行数=发现项+表头");

    // 口径漂移防护：严重度越界不入账
    let mut f2 = Facet::default();
    f2.absorb(&Finding { kind: VKind::Static, dom: 0, rule: 1, sev: 9, title: String::new() });
    chk(&mut set, "C482-11", f2.total() == 1 && f2.by_sev.iter().sum::<u32>() == 0, "越界严重度只计数不污染切面");

    let empty = ReportHub::new();
    chk(&mut set, "C482-12", empty.a11y_view().is_empty() && empty.trend.is_empty(), "空平台各视图空而不断");

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn report_hub() {
        let mut h = ReportHub::new();
        assert!(h.export().is_err());
        h.absorb(Finding { kind: VKind::A11y, dom: 0, rule: 1, sev: 3, title: String::from("x") });
        assert_eq!(h.export().unwrap()[0].2, 3);
        assert_eq!(h.a11y_view().len(), 1);
    }
}
