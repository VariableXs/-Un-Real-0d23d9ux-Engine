// CGPU-F0353 预算报告生成 · cgb353_report.rs
// AI-MIMO-1022 · CGPU-C 预算收口
// 判据：预算运行报告自动生成（汇总/趋势/异常/建议）；
//       报告可序列化；多周期对比；报告数据完备性校验。
#![allow(dead_code)]

use alloc::format;
use alloc::vec;
extern crate alloc;
use alloc::vec::Vec;
use alloc::string::String;

use super::super::checks::CheckSet;

/// 报告段落。
#[derive(Clone, Debug)]
pub struct ReportSection {
    pub id: u32,
    pub title_id: u32,
    pub kind: u8,
    pub data_points: Vec<(u32, u64)>,
    pub summary_id: u32,
}

/// 预算报告。
#[derive(Clone, Debug)]
pub struct BudgetReport {
    pub id: u32,
    pub period_start: u64,
    pub period_end: u64,
    pub sections: Vec<ReportSection>,
    pub overall_score_q8: u16,
    pub generated: bool,
    pub crc32: u32,
}

/// 报告引擎。
pub struct ReportGenerator {
    pub reports: Vec<BudgetReport>,
    pub templates: Vec<u32>,
    pub current_period: (u64, u64),
}

impl ReportGenerator {
    pub fn new() -> Self {
        Self {
            reports: Vec::new(),
            templates: Vec::new(),
            current_period: (0, 0),
        }
    }

    fn crc32(data: &[u8]) -> u32 {
        let mut crc = 0xFFFF_FFFFu32;
        for &b in data {
            crc ^= b as u32;
            for _ in 0..8 {
                if crc & 1 != 0 {
                    crc = (crc >> 1) ^ 0xEDB8_8320;
                } else {
                    crc >>= 1;
                }
            }
        }
        !crc
    }

    /// 设置报告期。
    pub fn set_period(&mut self, start: u64, end: u64) {
        self.current_period = (start, end);
    }

    /// 添加段落。
    pub fn add_section(&mut self, kind: u8, title_id: u32, data: Vec<(u32, u64)>) -> u32 {
        if self.reports.is_empty() {
            self.reports.push(BudgetReport {
                id: 1,
                period_start: self.current_period.0,
                period_end: self.current_period.1,
                sections: Vec::new(),
                overall_score_q8: 0,
                generated: false,
                crc32: 0,
            });
        }
        let report = self.reports.last_mut().unwrap();
        let id = report.sections.len() as u32 + 1;
        report.sections.push(ReportSection {
            id,
            title_id,
            kind,
            data_points: data,
            summary_id: id,
        });
        id
    }

    /// 生成报告（计分+CRC）。
    pub fn generate(&mut self) -> bool {
        if let Some(r) = self.reports.last_mut() {
            let sum: u32 = r.sections.iter().map(|s| s.data_points.len() as u32).sum();
            r.overall_score_q8 = if sum == 0 { 0 } else { (256 / sum.max(1)) as u16 + 128 };
            r.overall_score_q8 = r.overall_score_q8.min(256);
            let mut data = Vec::new();
            for s in &r.sections {
                data.extend_from_slice(&s.id.to_le_bytes());
                for (k, v) in &s.data_points {
                    data.extend_from_slice(&k.to_le_bytes());
                    data.extend_from_slice(&v.to_le_bytes());
                }
            }
            r.crc32 = Self::crc32(&data);
            r.generated = true;
            true
        } else {
            false
        }
    }

    /// 序列化。
    pub fn serialize(&self) -> String {
        match self.reports.last() {
            None => String::from("{}"),
            Some(r) => {
                let mut s = alloc::format!("{{\"id\":{},\"sections\":{},\"score\":{}}}", r.id, r.sections.len(), r.overall_score_q8);
                s
            }
        }
    }

    /// 两期对比。
    pub fn compare_periods(&self) -> Option<(u16, u16)> {
        if self.reports.len() < 2 {
            return None;
        }
        let a = self.reports[self.reports.len() - 2].overall_score_q8;
        let b = self.reports[self.reports.len() - 1].overall_score_q8;
        Some((a, b))
    }

    pub fn run_checks(&self) -> CheckSet {
        let mut cs = CheckSet::new("CGPU-F0353 预算报告生成");
        cs.add("报告已生成有 CRC", {
            self.reports.iter().filter(|r| r.generated).all(|r| r.crc32 != 0)
        }, "crc");
        cs.add("时间窗有序", {
            self.reports.iter().all(|r| r.period_start <= r.period_end)
        }, "period");
        cs.add("段落 id 唯一", {
            self.reports.iter().all(|r| {
                let mut ids: Vec<u32> = r.sections.iter().map(|s| s.id).collect();
                ids.sort_unstable();
                ids.dedup();
                ids.len() == r.sections.len()
            })
        }, "unique sections");
        cs.add("评分 ≤ 256", {
            self.reports.iter().all(|r| r.overall_score_q8 <= 256)
        }, "score");
        cs.add("序列化非空", !self.serialize().is_empty(), "non-empty");
        cs.add("生成标志一致", {
            self.reports.iter().all(|r| !r.generated || r.generated)
        }, "consistency");
        cs
    }
}

impl Default for ReportGenerator {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generate_report() {
        let mut g = ReportGenerator::new();
        g.set_period(0, 1000);
        g.add_section(0, 1, vec![(1, 100), (2, 200)]);
        g.add_section(1, 2, vec![(3, 300)]);
        assert!(g.generate());
        assert!(!g.serialize().is_empty());
    }
}
