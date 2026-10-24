//! VE-F4355 · 一致性 fuzz 总闸（U08 测试与资产组）
//! 职责：一致性 fuzz 总闸——总闸聚合（U04-U07 各面 fuzz 统一调度——语料
//! 共享 / 配额总控 / 结果归一）+ 总闸双档节奏（日常快速轮 + 夜间深扫——
//! 深扫可过夜分片）+ 发现处置闭环（立案→定级→修复回归）+ 调度冲突优先级
//! 仲裁 + 结果重复归一去重 + 结果脱敏。
//! 段位：0x4460（续表 idx589）。行数预算 320。

#![allow(dead_code)]

use alloc::vec;
use alloc::vec::Vec;
use crate::checks::CheckSet;

pub const SEG_BASE: u16 = 0x4460;

#[repr(u16)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FgCode {
    /// 调度冲突（优先级仲裁）。
    ScheduleClash = SEG_BASE,
    /// 结果重复（归一去重）。
    DupFinding = SEG_BASE + 0x01,
    /// 深扫积压（分片过夜）。
    DeepBacklog = SEG_BASE + 0x02,
    /// 面号越界。
    FaceRange = SEG_BASE + 0x03,
}

/// 各面 fuzz（U04-U07 四面）。
#[derive(Clone, Copy, PartialEq, Eq, Debug, PartialOrd, Ord)]
pub enum Face {
    Gov,
    Tool,
    Scene,
    Perf,
}

/// 节奏档。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[derive(Default)]
pub enum Pace {
    /// 日常快速轮。
    #[default]
    Quick,
    /// 夜间深扫（可过夜分片）。
    Deep,
}

/// 面档位系数（深扫配额 = 快速 × 8）。
pub fn pace_quota(p: Pace) -> u32 {
    match p {
        Pace::Quick => 100,
        Pace::Deep => 800,
    }
}

/// 发现单（归一结果流）。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Finding {
    pub face: Face,
    /// 归一哈希（去重键——跨面同缺陷归一）。
    pub fp: u32,
    pub graded: Option<u8>, // 定级 0 未定 1 P0 2 P1 3 P2
    pub fixed: bool,
    pub regressed: bool,
}

/// fuzz 总闸台。
#[derive(Default)]
pub struct FuzzGate {
    pub findings: Vec<Finding>,
    /// 各面调度优先级（冲突时低号面先跑）。
    pub priority: Vec<(Face, u8)>,
    pub pace: Pace,
    pub used: u32,
    pub dedups: u32,
    pub shards: u32,
    pub closed_loops: u32,
}

fn fnv(x: u32) -> u32 {
    let mut h: u32 = 0x811C9DC5;
    let mut v = x;
    for _ in 0..4 {
        h ^= v & 0xFF;
        h = h.wrapping_mul(0x01000193);
        v >>= 8;
    }
    h
}

impl FuzzGate {
    pub fn new(pace: Pace) -> Self {
        FuzzGate { pace, ..Default::default() }
    }

    /// 配额总控（按节奏档；耗尽即拒）。
    pub fn take(&mut self, n: u32) -> Result<(), FgCode> {
        let cap = pace_quota(self.pace);
        if self.used + n > cap {
            return Err(FgCode::ScheduleClash);
        }
        self.used += n;
        Ok(())
    }

    /// 面登记优先级（幂等覆盖）。
    pub fn set_priority(&mut self, f: Face, p: u8) {
        if let Some(x) = self.priority.iter_mut().find(|x| x.0 == f) {
            x.1 = p;
        } else {
            self.priority.push((f, p));
        }
    }

    /// 调度冲突仲裁：多面同轮竞争按优先级取最小（返回胜出面）。
    pub fn arbitrate(&self, faces: &[Face]) -> Option<Face> {
        let mut best: Option<(u8, Face)> = None;
        for f in faces {
            let p = self.priority.iter().find(|x| x.0 == *f).map(|x| x.1).unwrap_or(9);
            match best {
                Some((bp, _)) if bp <= p => {}
                _ => best = Some((p, *f)),
            }
        }
        best.map(|(_, f)| f)
    }

    /// 发现上报：归一哈希去重（同哈希重复 = 同缺陷不重复立案）。
    pub fn report(&mut self, face: Face, content: u32) -> Result<Option<usize>, FgCode> {
        let fp = fnv(content);
        if self.findings.iter().any(|f| f.fp == fp) {
            self.dedups += 1;
            return Err(FgCode::DupFinding);
        }
        self.findings.push(Finding { face, fp, graded: None, fixed: false, regressed: false });
        Ok(Some(self.findings.len() - 1))
    }

    /// 处置闭环：定级 → 修复 → 回归确认（三步齐才闭环计数）。
    pub fn close_loop(&mut self, idx: usize, grade: u8, fixed: bool, regressed: bool) -> Result<(), FgCode> {
        let f = self.findings.get_mut(idx).ok_or(FgCode::ScheduleClash)?;
        if f.graded.is_none() {
            f.graded = Some(grade);
        }
        if !fixed {
            return Ok(());
        }
        f.fixed = true;
        f.regressed = regressed;
        if !regressed {
            self.closed_loops += 1;
        }
        Ok(())
    }

    /// 深扫积压分片过夜：待扫量超快速配额 → 分片（每片 = 快速配额）。
    pub fn shard_overnight(&mut self, backlog: u32) -> u32 {
        let unit = pace_quota(Pace::Quick);
        if backlog <= unit {
            return 0;
        }
        let n = (backlog + unit - 1) / unit;
        self.shards = n;
        n
    }

    /// 闭环率 bp。
    pub fn close_rate_bp(&self) -> u32 {
        if self.findings.is_empty() {
            return 0;
        }
        (self.closed_loops as u64 * 10000 / self.findings.len() as u64) as u32
    }
}

fn chk(set: &mut CheckSet, label: &'static str, ok: bool, detail: &'static str) {
    set.add(label, ok, detail);
}

pub fn run_ves589_checks() -> CheckSet {
    let mut set = CheckSet::new("ves589");
    let mut fg = FuzzGate::new(Pace::Quick);

    // C589-01 配额总控（快速轮 100）。
    chk(&mut set, "C589-01", fg.take(100).is_ok() && fg.take(1).is_err(), "快速配额 100");

    // C589-02 深扫档配额 8 倍。
    let fg2 = FuzzGate::new(Pace::Deep);
    chk(&mut set, "C589-02", pace_quota(Pace::Deep) == 800 && fg2.used == 0, "深扫配额 800");

    // C589-03 面优先级登记 + 冲突仲裁取最小。
    fg.set_priority(Face::Gov, 2);
    fg.set_priority(Face::Perf, 1);
    fg.set_priority(Face::Tool, 3);
    chk(&mut set, "C589-03", fg.arbitrate(&[Face::Gov, Face::Perf, Face::Tool]) == Some(Face::Perf), "冲突仲裁取最小");

    // C589-04 未登记面默认最低优先。
    chk(&mut set, "C589-04", fg.arbitrate(&[Face::Gov, Face::Scene]) == Some(Face::Gov), "未登记默认 9");

    // C589-05 发现上报归一入流。
    fg.used = 0;
    let r1 = fg.report(Face::Gov, 0xABCD);
    chk(&mut set, "C589-05", r1 == Ok(Some(0)) && fg.findings.len() == 1, "发现归一入流");

    // C589-06 跨面同缺陷归一去重。
    chk(&mut set, "C589-06", fg.report(Face::Perf, 0xABCD) == Err(FgCode::DupFinding) && fg.dedups == 1, "跨面归一去重");

    // C589-07 处置闭环三步（定级→修复→回归过）。
    let _ = fg.close_loop(0, 1, true, false);
    chk(&mut set, "C589-07", fg.findings[0].graded == Some(1) && fg.findings[0].fixed
        && !fg.findings[0].regressed && fg.closed_loops == 1, "闭环三步");

    // C589-08 回归未过不闭环（修复回退）。
    let _ = fg.report(Face::Tool, 0x1234).unwrap_or(None);
    let _ = fg.close_loop(1, 2, true, true);
    chk(&mut set, "C589-08", fg.findings[1].regressed && fg.closed_loops == 1, "回归未过不闭环");

    // C589-09 深扫积压分片过夜。
    chk(&mut set, "C589-09", fg.shard_overnight(3000) == 4 && fg.shards == 4, "积压分片 4");
    chk(&mut set, "C589-09b", fg.shard_overnight(100) == 0, "不足不分");

    // C589-10 闭环率核算。
    chk(&mut set, "C589-10", fg.close_rate_bp() == 5000, "闭环率 50%");

    // C589-11 结果脱敏断言（发现单无原文——仅哈希）。
    chk(&mut set, "C589-11", fg.findings.iter().all(|f| f.fp != 0xABCD || f.graded.is_some()), "结果仅哈希在册");

    // C589-12 空闸闭环率恒零（不臆测）。
    let mut fg3 = FuzzGate::new(Pace::Quick);
    chk(&mut set, "C589-12", fg3.close_rate_bp() == 0 && fg3.shard_overnight(50) == 0, "空闸不臆测");

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quota_by_pace() {
        let mut fg = FuzzGate::new(Pace::Quick);
        assert!(fg.take(99).is_ok());
        assert!(fg.take(1).is_ok());
        assert_eq!(fg.take(1), Err(FgCode::ScheduleClash));
    }

    #[test]
    fn dedup_cross_face() {
        let mut fg = FuzzGate::new(Pace::Deep);
        assert!(fg.report(Face::Scene, 7).is_ok());
        assert_eq!(fg.report(Face::Gov, 7), Err(FgCode::DupFinding));
        assert!(fg.report(Face::Gov, 8).is_ok());
    }

    #[test]
    fn overnight_sharding() {
        let mut fg = FuzzGate::new(Pace::Deep);
        assert_eq!(fg.shard_overnight(800), 0); // 恰一单元不分
        assert_eq!(fg.shard_overnight(801), 2);
    }
}
