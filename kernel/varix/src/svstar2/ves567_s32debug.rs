//! VE-F4333 · 一致性性能调试器（U07 性能与预算组）
//! 职责：一致性性能调试工具——调试三件（单次判定耗时分解 / 扫描热点图 /
//! 预算消耗回放）+ 性能问题归因流四步（采集→分解→定位→建议）+ 与 F4286
//! 调试器分工声明（那边管正确性这边管性能——不重叠不缺位）+ 分解失真多次
//! 采样中值 + 回放缺数据标记不补造 + 调试视图键盘完整（域本色）。
//! 段位：0x3660（续表 idx567）。行数预算 340。

#![allow(dead_code)]

use alloc::vec;
use alloc::vec::Vec;
use crate::checks::CheckSet;

pub const SEG_BASE: u16 = 0x3660;

#[repr(u16)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DbCode {
    /// 分解失真（多次采样中值兜底）。
    Distort = SEG_BASE,
    /// 回放缺数据（标记不补造）。
    ReplayGap = SEG_BASE + 0x01,
    /// 归因流断步（四步顺序违规）。
    FlowSkip = SEG_BASE + 0x02,
    /// 热点越界。
    HotRange = SEG_BASE + 0x03,
}

/// 归因流四步。
#[derive(Clone, Copy, PartialEq, Eq, Debug, PartialOrd, Ord)]
pub enum FlowStep {
    Collect,
    Decompose,
    Locate,
    Suggest,
}

/// 耗时分解卡：一次判定的分项耗时（µs 抽象）。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TimeBreak {
    pub rule_load: u32,
    pub compare: u32,
    pub report: u32,
}

impl TimeBreak {
    pub fn total(&self) -> u32 {
        self.rule_load + self.compare + self.report
    }
    /// 最耗分项（定位输出）。
    pub fn hotspot(&self) -> &'static str {
        let m = self.rule_load.max(self.compare).max(self.report);
        if m == self.compare {
            "比对"
        } else if m == self.rule_load {
            "规则装载"
        } else {
            "上报"
        }
    }
}

/// 热点图条目：(区间号, 命中数)。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct HotCell {
    pub bucket: u32,
    pub hits: u32,
}

/// 调试器台。
#[derive(Default)]
pub struct PerfDbg {
    /// 采样历史（分解失真中值用）。
    pub samples: Vec<TimeBreak>,
    pub hots: Vec<HotCell>,
    /// 回放帧（None = 缺帧——标记不补造）。
    pub frames: Vec<Option<TimeBreak>>,
    pub flow: Option<FlowStep>,
    pub advices: u32,
    pub gap_marks: u32,
}

impl PerfDbg {
    pub fn new() -> Self {
        PerfDbg { ..Default::default() }
    }

    /// 采样入历史。
    pub fn collect(&mut self, b: TimeBreak) {
        self.samples.push(b);
    }

    /// 中值分解：多次采样取各分项中位（失真兜底）。
    pub fn decompose_median(&self) -> Result<TimeBreak, DbCode> {
        if self.samples.is_empty() {
            return Err(DbCode::Distort);
        }
        let med = |mut v: Vec<u32>| -> u32 {
            v.sort_unstable();
            v[v.len() / 2]
        };
        Ok(TimeBreak {
            rule_load: med(self.samples.iter().map(|s| s.rule_load).collect()),
            compare: med(self.samples.iter().map(|s| s.compare).collect()),
            report: med(self.samples.iter().map(|s| s.report).collect()),
        })
    }

    /// 热点登记（区间号校验 0-15）。
    pub fn hot(&mut self, bucket: u32, hits: u32) -> Result<(), DbCode> {
        if bucket > 15 {
            return Err(DbCode::HotRange);
        }
        if let Some(h) = self.hots.iter_mut().find(|h| h.bucket == bucket) {
            h.hits += hits;
        } else {
            self.hots.push(HotCell { bucket, hits });
        }
        Ok(())
    }

    /// 热点 top1（最耗区间）。
    pub fn hot_top(&self) -> Option<HotCell> {
        let mut best: Option<HotCell> = None;
        for h in self.hots.iter() {
            match best {
                Some(b) if b.hits >= h.hits => {}
                _ => best = Some(*h),
            }
        }
        best
    }

    /// 回放帧入带（None = 缺帧）。
    pub fn push_frame(&mut self, f: Option<TimeBreak>) {
        self.frames.push(f);
    }

    /// 回放：缺帧不补造——跳过并标记（返回有效帧数与标记数）。
    pub fn replay(&mut self) -> (usize, usize) {
        let valid = self.frames.iter().filter(|f| f.is_some()).count();
        let gaps = self.frames.iter().filter(|f| f.is_none()).count();
        self.gap_marks += gaps as u32;
        (valid, gaps)
    }

    /// 归因流四步：顺序违规拒绝（越步 = 断步）。
    pub fn flow_next(&mut self, s: FlowStep) -> Result<(), DbCode> {
        let expect = match self.flow {
            None => FlowStep::Collect,
            Some(x) => match x {
                FlowStep::Collect => FlowStep::Decompose,
                FlowStep::Decompose => FlowStep::Locate,
                FlowStep::Locate => FlowStep::Suggest,
                FlowStep::Suggest => return Err(DbCode::FlowSkip), // 已走完
            },
        };
        if s != expect {
            return Err(DbCode::FlowSkip);
        }
        self.flow = Some(s);
        if s == FlowStep::Suggest {
            self.advices += 1;
        }
        Ok(())
    }

    /// 建议输出：最耗分项降耗建议（基于中值分解）。
    pub fn advice(&self) -> Result<&'static str, DbCode> {
        let b = self.decompose_median()?;
        Ok(match b.hotspot() {
            "比对" => "比对分项最耗——建议增量比对",
            "规则装载" => "规则装载最耗——建议规则缓存",
            _ => "上报最耗——建议批量上报",
        })
    }

    /// 分工声明：正确性调试归 F4286（本台不受理正确性问题）。
    pub fn scope_of(&self, correctness: bool) -> Result<(), DbCode> {
        if correctness {
            Err(DbCode::Distort) // 正确性问题转 F4286——本台只管性能
        } else {
            Ok(())
        }
    }
}

fn chk(set: &mut CheckSet, label: &'static str, ok: bool, detail: &'static str) {
    set.add(label, ok, detail);
}

pub fn run_ves567_checks() -> CheckSet {
    let mut set = CheckSet::new("ves567");
    let mut dbg = PerfDbg::new();

    // C567-01 采样入历史 + 中值分解。
    dbg.collect(TimeBreak { rule_load: 10, compare: 100, report: 5 });
    dbg.collect(TimeBreak { rule_load: 12, compare: 90, report: 7 });
    dbg.collect(TimeBreak { rule_load: 30, compare: 95, report: 6 }); // 野点
    let med = dbg.decompose_median();
    chk(&mut set, "C567-01", med == Ok(TimeBreak { rule_load: 12, compare: 95, report: 6 }), "中值分解剔野点");

    // C567-02 空样本拒绝（不臆测）。
    let dbg0 = PerfDbg::new();
    chk(&mut set, "C567-02", dbg0.decompose_median() == Err(DbCode::Distort), "空样本拒绝");

    // C567-03 定位：最耗分项 = 比对。
    chk(&mut set, "C567-03", med.map(|b| b.hotspot()) == Ok("比对"), "最耗分项定位");

    // C567-04 热点登记 + 越界拒。
    dbg.hot(3, 50).unwrap_or(());
    dbg.hot(7, 80).unwrap_or(());
    chk(&mut set, "C567-04", dbg.hot(16, 1).is_err() && dbg.hots.len() == 2, "热点登记+越界拒");

    // C567-05 热点 top1。
    chk(&mut set, "C567-05", dbg.hot_top() == Some(HotCell { bucket: 7, hits: 80 }), "热点 top1");

    // C567-06 回放缺帧标记不补造。
    dbg.push_frame(Some(TimeBreak { rule_load: 1, compare: 2, report: 3 }));
    dbg.push_frame(None);
    dbg.push_frame(Some(TimeBreak { rule_load: 1, compare: 2, report: 3 }));
    let (valid, gaps) = dbg.replay();
    chk(&mut set, "C567-06", valid == 2 && gaps == 1 && dbg.gap_marks == 1, "缺帧标记不补造");

    // C567-07 归因流四步顺序贯通。
    let mut dbg2 = PerfDbg::new();
    let mut ok = true;
    for s in [FlowStep::Collect, FlowStep::Decompose, FlowStep::Locate, FlowStep::Suggest] {
        if dbg2.flow_next(s).is_err() {
            ok = false;
        }
    }
    chk(&mut set, "C567-07", ok && dbg2.advices == 1, "四步顺序贯通");

    // C567-08 越步断步拦截（Collect 后直跳 Suggest）。
    let mut dbg3 = PerfDbg::new();
    let _ = dbg3.flow_next(FlowStep::Collect);
    chk(&mut set, "C567-08", dbg3.flow_next(FlowStep::Suggest) == Err(DbCode::FlowSkip), "越步断步拦截");

    // C567-09 走完后再走 = 拒（流已闭）。
    let _ = dbg2.flow_next(FlowStep::Collect);
    chk(&mut set, "C567-09", dbg2.flow_next(FlowStep::Decompose) == Err(DbCode::FlowSkip), "流闭再走拒");

    // C567-10 建议输出（比对最耗 → 增量比对建议）。
    chk(&mut set, "C567-10", dbg.advice() == Ok("比对分项最耗——建议增量比对"), "建议按最耗分项");

    // C567-11 分工声明：正确性转 F4286、性能本台受理。
    chk(&mut set, "C567-11", dbg.scope_of(true).is_err() && dbg.scope_of(false).is_ok(), "分工不重叠");

    // C567-12 热点空图 top1 恒 None（不臆测）。
    let dbg4 = PerfDbg::new();
    chk(&mut set, "C567-12", dbg4.hot_top().is_none() && dbg4.samples.is_empty(), "空图不臆测");

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn median_decompose() {
        let mut d = PerfDbg::new();
        d.collect(TimeBreak { rule_load: 5, compare: 50, report: 1 });
        d.collect(TimeBreak { rule_load: 100, compare: 55, report: 2 });
        d.collect(TimeBreak { rule_load: 6, compare: 60, report: 3 });
        let m = d.decompose_median().unwrap_or(TimeBreak { rule_load: 0, compare: 0, report: 0 });
        assert_eq!(m, TimeBreak { rule_load: 6, compare: 55, report: 2 });
    }

    #[test]
    fn flow_order_enforced() {
        let mut d = PerfDbg::new();
        assert_eq!(d.flow_next(FlowStep::Locate), Err(DbCode::FlowSkip));
        assert!(d.flow_next(FlowStep::Collect).is_ok());
        assert_eq!(d.flow_next(FlowStep::Suggest), Err(DbCode::FlowSkip));
    }

    #[test]
    fn replay_gap_marks() {
        let mut d = PerfDbg::new();
        d.push_frame(None);
        d.push_frame(None);
        let (valid, gaps) = d.replay();
        assert_eq!((valid, gaps), (0, 2));
        assert_eq!(d.gap_marks, 2);
    }
}
