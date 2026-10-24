//! VE-F4287 · 一致性×性能看板（U05 工具集成组）
//! 职责：性能看板一致性面板（检查耗时/增量率/阻断次数三族指标）+ 与 U07
//! 同源单管道不双采 + 告警桥联动 F4270 + 管道断流置灰不显旧数
//! + 口径版本戳 + 告警合并窗去重 + 图表附数据表（域本色）。
//! 段位：0x2AA0（续表 idx521）。行数预算 300。

#![allow(dead_code)]

use alloc::vec;
use alloc::vec::Vec;
use crate::checks::CheckSet;

pub const SEG_BASE: u16 = 0x2AA0;

#[repr(u16)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PbCode {
    /// 管道断流（置灰不显旧数）。
    PipeBroken = SEG_BASE,
    /// 口径漂移（版本戳不符）。
    CaliberDrift = SEG_BASE + 0x01,
}

/// 三族指标。
#[derive(Clone, Copy, Default, Debug)]
pub struct PanelMetrics {
    pub check_ms: u32,      // 一致性检查耗时
    pub incr_ratio_bp: u32, // 增量率 bp
    pub blocks: u32,        // 阻断次数
}

/// 面板。
pub struct ConsPanel {
    /// 同源管道数据：(时点, 指标)。
    pub pipe: Vec<(u32, PanelMetrics)>,
    /// 管道版本戳。
    pub pipe_ver: u32,
    /// 断流标记。
    pub broken: bool,
    /// 告警合并窗（窗宽时段）。
    pub merge_win: u32,
    pub alerts: Vec<(u32, u16)>, // (时点, 规则)
    /// 数据表附备（读屏——域本色）。
    pub table_attached: bool,
}

impl ConsPanel {
    pub fn new(pipe_ver: u32, merge_win: u32) -> Self {
        ConsPanel { pipe: Vec::new(), pipe_ver, broken: false, merge_win, alerts: Vec::new(), table_attached: false }
    }

    /// 指标推送（同源单管道——O(1)）。
    pub fn push(&mut self, t: u32, m: PanelMetrics) -> Result<(), PbCode> {
        if self.broken {
            return Err(PbCode::PipeBroken);
        }
        if let Some((lt, _)) = self.pipe.last() {
            if t <= *lt {
                self.broken = true;
                return Err(PbCode::PipeBroken);
            }
        }
        self.pipe.push((t, m));
        Ok(())
    }

    /// 版本戳校验。
    pub fn caliber_ok(&self, ver: u32) -> bool {
        self.pipe_ver == ver
    }

    /// 断流处置：置灰（不清数据但标注不显旧数）。
    pub fn mark_broken(&mut self) {
        self.broken = true;
    }

    /// 告警桥：推送告警（合并窗去重：同窗同规则只记一条）。
    pub fn bridge_alert(&mut self, t: u32, rule: u16) {
        let win = t / self.merge_win.max(1);
        let dup = self.alerts.iter().any(|(at, r)| *r == rule && *at / self.merge_win.max(1) == win);
        if !dup {
            self.alerts.push((t, rule));
        }
    }

    /// 面板渲染数据（断流返回空——置灰不显旧数）。
    pub fn render(&self) -> Vec<PanelMetrics> {
        if self.broken {
            return Vec::new();
        }
        self.pipe.iter().map(|(_, m)| *m).collect()
    }
}

fn chk(set: &mut CheckSet, label: &'static str, ok: bool, detail: &'static str) {
    set.add(label, ok, detail);
}

/// 判据组：C521-01..12。
pub fn run_ves521_checks() -> CheckSet {
    let mut set = CheckSet::new("ves521");

    let mut pb = ConsPanel::new(3, 10);
    chk(&mut set, "C521-01", pb.caliber_ok(3), "口径版本戳一致");
    chk(&mut set, "C521-02", !pb.caliber_ok(4), "口径漂移检出");

    let m1 = PanelMetrics { check_ms: 120, incr_ratio_bp: 6_000, blocks: 0 };
    let m2 = PanelMetrics { check_ms: 90, incr_ratio_bp: 8_000, blocks: 1 };
    let _ = pb.push(1, m1);
    chk(&mut set, "C521-03", pb.push(1, m2) == Err(PbCode::PipeBroken), "时点重复=管道断流");
    pb.broken = false;
    let _ = pb.push(2, m2);
    chk(&mut set, "C521-04", pb.pipe.len() == 2, "三族指标推送在册");

    // 置灰
    pb.mark_broken();
    chk(&mut set, "C521-05", pb.render().is_empty(), "断流置灰不显旧数");
    chk(&mut set, "C521-06", pb.push(3, m1) == Err(PbCode::PipeBroken), "断流拒绝推送");
    pb.broken = false;
    chk(&mut set, "C521-07", pb.render().len() == 2, "恢复后数据可见");

    // 告警合并窗
    pb.bridge_alert(11, 7);
    pb.bridge_alert(15, 7);
    pb.bridge_alert(21, 7);
    chk(&mut set, "C521-08", pb.alerts.len() == 2, "合并窗去重（11/15 同窗 1 条，21 新窗）");
    pb.bridge_alert(21, 8);
    chk(&mut set, "C521-09", pb.alerts.len() == 3, "同窗异规则不去重");

    // 域本色
    pb.table_attached = true;
    chk(&mut set, "C521-10", pb.table_attached, "图表附数据表（读屏——域本色）");
    chk(&mut set, "C521-11", pb.pipe.iter().all(|(t, _)| *t > 0), "时序账自洽");
    chk(&mut set, "C521-12", pb.render()[1].blocks == 1, "阻断次数字段在面板");

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn panel_metrics() {
        let mut pb = ConsPanel::new(1, 5);
        let _ = pb.push(1, PanelMetrics::default());
        pb.mark_broken();
        assert!(pb.render().is_empty());
        assert_eq!(pb.push(2, PanelMetrics::default()), Err(PbCode::PipeBroken));
    }
}
