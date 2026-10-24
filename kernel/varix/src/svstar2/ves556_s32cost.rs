//! VE-F4322 · 一致性成本模型（U07 性能与预算组）
//! 职责：一致性操作成本模型——三类操作（判定/扫描/上报）单位成本基线 +
//! 场景系数 + 成本预估器（变更提交前预估一致性开销——预估先行不盲跑）+
//! 模型校准（月度实测回灌）+ 预估偏差超阈模型回滚旧版 + 系数缺失缺省值
//! 显式标注 + 实测缺失不校准不漂移。
//! 段位：0x33A0（续表 idx556）。行数预算 360。

#![allow(dead_code)]

use alloc::vec;
use alloc::vec::Vec;
use crate::checks::CheckSet;

pub const SEG_BASE: u16 = 0x33A0;

#[repr(u16)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CmCode {
    /// 预估偏差超阈（模型回滚旧版）。
    EstimateDrift = SEG_BASE,
    /// 实测缺失（不校准不漂移）。
    NoSample = SEG_BASE + 0x01,
    /// 操作类别越界。
    OpRange = SEG_BASE + 0x02,
}

/// 三类操作。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum OpKind {
    Judge,
    Scan,
    Report,
}

/// 成本基线（单位：抽象成本点）。
#[derive(Clone, Copy, Debug)]
pub struct Baseline {
    pub kind: OpKind,
    pub unit_cost: u32,
    /// 场景系数（缺省 100 = 1.00 倍，bp 计）。
    pub scene_coef_bp: u32,
    /// 系数是否为缺省（显式标注义务）。
    pub coef_default: bool,
}

/// 校准样本。
#[derive(Clone, Copy, Debug)]
pub struct Sample {
    pub kind: OpKind,
    pub measured: u32,
}

/// 成本模型（带版本——回滚单位）。
#[derive(Clone)]
pub struct CostModel {
    pub ver: u32,
    pub rows: Vec<Baseline>,
}

/// 成本台。
#[derive(Default)]
pub struct CostDesk {
    pub model: Option<CostModel>,
    pub prev: Option<CostModel>,
    pub samples: Vec<Sample>,
    pub estimate_gap_bp: u32,
    pub calibrations: u32,
    pub rollbacks: u32,
}

/// 预估偏差容忍阈（bp）。
pub const GAP_TOLERANCE_BP: u32 = 1500;

impl CostDesk {
    pub fn new() -> Self {
        CostDesk { estimate_gap_bp: GAP_TOLERANCE_BP, ..Default::default() }
    }

    /// 装载模型（旧版自动入回滚栈）。
    pub fn load(&mut self, m: CostModel) {
        self.prev = self.model.take();
        self.model = Some(m);
    }

    /// 单位成本预估：基线 × 场景系数（系数缺失走缺省 100 并显式标注）。
    pub fn estimate(&self, kind: OpKind, scenes: u32) -> Result<u64, CmCode> {
        let m = self.model.as_ref().ok_or(CmCode::OpRange)?;
        let b = m.rows.iter().find(|b| b.kind == kind).ok_or(CmCode::OpRange)?;
        Ok(b.unit_cost as u64 * scenes as u64 * b.scene_coef_bp as u64 / 100)
    }

    /// 系数显式标注查询（缺省系数可见 = 诚实呈现）。
    pub fn coef_is_default(&self, kind: OpKind) -> Result<bool, CmCode> {
        let m = self.model.as_ref().ok_or(CmCode::OpRange)?;
        let b = m.rows.iter().find(|b| b.kind == kind).ok_or(CmCode::OpRange)?;
        Ok(b.coef_default)
    }

    /// 实测回灌（校准前样本累积）。
    pub fn feed(&mut self, s: Sample) {
        self.samples.push(s);
    }

    /// 月度校准：按类取样本均值回灌基线；无样本不校准不漂移。
    pub fn calibrate(&mut self, kind: OpKind) -> Result<u32, CmCode> {
        let vals: Vec<u32> = self.samples.iter().filter(|s| s.kind == kind).map(|s| s.measured).collect();
        if vals.is_empty() {
            return Err(CmCode::NoSample);
        }
        let sum: u64 = vals.iter().map(|v| *v as u64).sum();
        let avg = (sum / vals.len() as u64) as u32;
        if let Some(m) = self.model.as_mut() {
            if let Some(b) = m.rows.iter_mut().find(|b| b.kind == kind) {
                b.unit_cost = avg;
            }
        }
        self.calibrations += 1;
        Ok(avg)
    }

    /// 预估对账：预估 vs 实测偏差超阈 → 模型回滚旧版（返回是否回滚）。
    pub fn reconcile(&mut self, kind: OpKind, actual: u32) -> Result<bool, CmCode> {
        let est = self.estimate(kind, 1)? as u32;
        let gap_bp = if est == 0 {
            if actual == 0 { 0 } else { u32::MAX / 2 }
        } else {
            let d = if actual > est { actual - est } else { est - actual };
            (d as u64 * 10000 / est as u64) as u32
        };
        if gap_bp > self.estimate_gap_bp {
            if let Some(old) = self.prev.take() {
                self.prev = self.model.take();
                self.model = Some(old);
                self.rollbacks += 1;
                return Ok(true);
            }
            return Err(CmCode::EstimateDrift);
        }
        Ok(false)
    }

    /// 预估先行断言：无模型预估被拒（不盲跑）。
    pub fn no_blind_run(&self, kind: OpKind) -> bool {
        self.estimate(kind, 1).is_err()
    }
}

fn chk(set: &mut CheckSet, label: &'static str, ok: bool, detail: &'static str) {
    set.add(label, ok, detail);
}

fn model(ver: u32, judge: u32, coef: u32, default: bool) -> CostModel {
    CostModel {
        ver,
        rows: vec![
            Baseline { kind: OpKind::Judge, unit_cost: judge, scene_coef_bp: coef, coef_default: default },
            Baseline { kind: OpKind::Scan, unit_cost: 300, scene_coef_bp: 100, coef_default: true },
            Baseline { kind: OpKind::Report, unit_cost: 50, scene_coef_bp: 120, coef_default: false },
        ],
    }
}

pub fn run_ves556_checks() -> CheckSet {
    let mut set = CheckSet::new("ves556");
    let mut cd = CostDesk::new();

    // C556-01 模型装载 + 预估（基线 × 系数）。
    cd.load(model(1, 100, 100, true));
    chk(&mut set, "C556-01", cd.estimate(OpKind::Judge, 4) == Ok(400), "预估基线×系数");

    // C556-02 场景系数生效（120bp）。
    chk(&mut set, "C556-02", cd.estimate(OpKind::Report, 10) == Ok(600), "场景系数生效");

    // C556-03 缺省系数显式标注。
    chk(&mut set, "C556-03", cd.coef_is_default(OpKind::Judge) == Ok(true)
        && cd.coef_is_default(OpKind::Report) == Ok(false), "缺省系数标注");

    // C556-04 预估先行：无模型不盲跑。
    let mut cd2 = CostDesk::new();
    chk(&mut set, "C556-04", cd2.no_blind_run(OpKind::Scan), "无模型不盲跑");

    // C556-05 实测回灌校准（均值回灌基线）。
    cd.feed(Sample { kind: OpKind::Judge, measured: 90 });
    cd.feed(Sample { kind: OpKind::Judge, measured: 110 });
    chk(&mut set, "C556-05", cd.calibrate(OpKind::Judge) == Ok(100) && cd.calibrations == 1, "均值校准回灌");

    // C556-06 实测缺失不校准不漂移。
    chk(&mut set, "C556-06", cd.calibrate(OpKind::Scan) == Err(CmCode::NoSample), "无样本不校准");

    // C556-07 预估对账：偏差在容忍内不回滚。
    chk(&mut set, "C556-07", cd.reconcile(OpKind::Judge, 110) == Ok(false), "偏差容忍内");

    // C556-08 偏差超阈模型回滚旧版。
    cd.load(model(2, 300, 100, true)); // 新版预估 300
    chk(&mut set, "C556-08", cd.reconcile(OpKind::Judge, 100) == Ok(true)
        && cd.rollbacks == 1 && cd.estimate(OpKind::Judge, 1) == Ok(100), "超阈回滚旧版");

    // C556-09 无旧版可回滚时拒绝（不静默）。
    chk(&mut set, "C556-09", cd.reconcile(OpKind::Judge, 900) == Err(CmCode::EstimateDrift), "无旧版拒静默");

    // C556-10 操作类别越界（模型无此类行）。
    let mut cd3 = CostDesk::new();
    cd3.load(CostModel { ver: 1, rows: vec![Baseline { kind: OpKind::Judge, unit_cost: 1, scene_coef_bp: 100, coef_default: true }] });
    chk(&mut set, "C556-10", cd3.estimate(OpKind::Scan, 1).is_err(), "类别越界拒");

    // C556-11 校准只动目标类（他类基线不动）。
    let scan_before = cd.estimate(OpKind::Scan, 1);
    let _ = cd.calibrate(OpKind::Report);
    chk(&mut set, "C556-11", cd.estimate(OpKind::Scan, 1) == scan_before, "校准只动目标类");

    // C556-12 偏差零预估防护（预估 0 实测非 0 = 最大偏差半格——回滚旧版）。
    let mut cd4 = CostDesk::new();
    cd4.load(model(1, 100, 100, true)); // 旧版在栈
    cd4.load(CostModel { ver: 2, rows: vec![Baseline { kind: OpKind::Judge, unit_cost: 0, scene_coef_bp: 100, coef_default: true }] });
    chk(&mut set, "C556-12", cd4.reconcile(OpKind::Judge, 5) == Ok(true), "零预估防护");

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn estimate_formula() {
        let mut cd = CostDesk::new();
        cd.load(model(1, 100, 150, false));
        assert_eq!(cd.estimate(OpKind::Judge, 2), Ok(300));
        assert_eq!(cd.coef_is_default(OpKind::Judge), Ok(false));
    }

    #[test]
    fn no_sample_no_drift() {
        let mut cd = CostDesk::new();
        cd.load(model(1, 100, 100, true));
        assert_eq!(cd.calibrate(OpKind::Report), Err(CmCode::NoSample));
        assert_eq!(cd.calibrations, 0);
    }

    #[test]
    fn rollback_chain() {
        let mut cd = CostDesk::new();
        cd.load(model(1, 100, 100, true));
        cd.load(model(2, 500, 100, true));
        assert_eq!(cd.reconcile(OpKind::Judge, 100), Ok(true)); // 回滚到 v1
        assert_eq!(cd.estimate(OpKind::Judge, 1), Ok(100));
        assert_eq!(cd.rollbacks, 1);
    }
}
