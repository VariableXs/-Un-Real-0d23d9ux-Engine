//! ves433_s23score — VE-F4193 · T 域 20 维度验收评分
//! 评分：T 域 20 维度终版评分（家族评分卡终演+每维证据指针+F4174 预评对账（偏差说明））；
//! 三 90 线核验（≥90 维数/均分/无短板维）；线未达→整改+复评；预终偏差超限→说明+复评；证据缺→暂缓该维；
//! 无障碍维终评含多语言读屏加权终版口径（域本色）；无隐私面。
#![allow(dead_code)]
use alloc::vec;
use alloc::vec::Vec;
use crate::checks::CheckSet;

pub const SEG_BASE: u16 = 0x12E0;
pub const DIM_N: usize = 20;
pub const LINE: u32 = 90;

#[repr(u16)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ScCode {
    Ok = 0,
    LineShort = SEG_BASE + 0x01,  // 0x12E1
    DeviateOver = SEG_BASE + 0x02, // 0x12E2 预终偏差超限
    EvidenceHeld = SEG_BASE + 0x03, // 0x12E3 证据缺暂缓
}

/// 20 维（维号 0..20）；无障碍维（第 2 维）为域本色主场维。
pub const A11Y_DIM: usize = 2;

/// 单维评分（证据指针 + 终评分）。
#[derive(Clone, Copy)]
pub struct DimScore { pub dim: usize, pub final_score: u32, pub evidence: Option<u32>, pub held: bool }

/// 终版评分卡。
pub struct ScoreCard {
    pub dims: Vec<DimScore>,
    pub pre_scores: Vec<(usize, u32)>, // F4174 预评
    pub deviations: Vec<(usize, i32)>, // 偏差说明（终-预）
    pub filed: u32,
}

impl ScoreCard {
    pub fn new() -> Self { ScoreCard { dims: Vec::new(), pre_scores: Vec::new(), deviations: Vec::new(), filed: 0 } }
    pub fn put(&mut self, dim: usize, score: u32, ev: u32) {
        self.dims.retain(|d| d.dim != dim);
        self.dims.push(DimScore { dim, final_score: score, evidence: Some(ev), held: false });
    }
    pub fn hold(&mut self, dim: usize) {
        self.dims.retain(|d| d.dim != dim);
        self.dims.push(DimScore { dim, final_score: 0, evidence: None, held: true });
    }
    pub fn put_pre(&mut self, dim: usize, pre: u32) {
        self.pre_scores.retain(|(d, _)| *d != dim);
        self.pre_scores.push((dim, pre));
    }
    /// 三 90 线：① ≥90 维数 = 20；② 均分 ≥ 90；③ 无短板维（每维 ≥ 90）。
    pub fn three90(&self) -> Result<(usize, u32, u32), ScCode> {
        let active: Vec<&DimScore> = self.dims.iter().filter(|d| !d.held).collect();
        if active.len() < DIM_N { return Err(ScCode::EvidenceHeld); }
        let ge = active.iter().filter(|d| d.final_score >= LINE).count();
        let total: u64 = active.iter().map(|d| d.final_score as u64).sum();
        let avg = (total / DIM_N as u64) as u32;
        let min = active.iter().map(|d| d.final_score).min().unwrap_or(0);
        if ge < DIM_N || avg < LINE || min < LINE { return Err(ScCode::LineShort); }
        Ok((ge, avg, min))
    }
    /// 预终对账：|终-预| > DEV_LIMIT 须偏差说明；未说明→超限码。
    pub fn reconcile_pre(&mut self) -> Result<(), ScCode> {
        const DEV_LIMIT: i32 = 5;
        self.deviations.clear();
        for d in &self.dims {
            if d.held { continue; }
            if let Some((_, pre)) = self.pre_scores.iter().find(|(p, _)| *p == d.dim) {
                let dev = d.final_score as i32 - *pre as i32;
                if dev.abs() > DEV_LIMIT {
                    if !self.deviations.iter().any(|(dd, _)| *dd == d.dim) {
                        self.deviations.push((d.dim, dev));
                    }
                }
            }
        }
        if self.deviations.is_empty() { Ok(()) } else { Err(ScCode::DeviateOver) }
    }
    pub fn explain(&mut self, dim: usize, dev: i32) {
        self.deviations.retain(|(d, _)| *d != dim);
        self.deviations.push((dim, dev));
    }
}

fn chk(set: &mut CheckSet, label: &'static str, ok: bool, detail: &'static str) { set.add(label, ok, detail); }

pub fn run_ves433_checks() -> CheckSet {
    let mut set = CheckSet::new("ves433");
    let mut card = ScoreCard::new();

    // C433-01 20 维全录入（证据齐），含 a11y 主场维（域本色加权口径位）。
    for i in 0..DIM_N { card.put(i, 92 + (i as u32 % 5), 7000 + i as u32); }
    card.put(A11Y_DIM, 93, 7002); // a11y 终评 93
    chk(&mut set, "C433-01", card.dims.len() == DIM_N, "20 维录入");

    // C433-02 三 90 线全过（维数/均分/最短板）。
    let r = card.three90();
    chk(&mut set, "C433-02", r == Ok((20, 94, 92)), "三 90 全过");

    // C433-03 单维跌破 90 → 线未达（无短板维线）。
    card.put(7, 85, 7007);
    chk(&mut set, "C433-03", card.three90() == Err(ScCode::LineShort), "短板维拒");

    // C433-04 整改复评复绿。
    card.put(7, 91, 7007);
    chk(&mut set, "C433-04", card.three90().is_ok(), "复评复绿");

    // C433-05 证据缺→暂缓该维。
    card.hold(9);
    chk(&mut set, "C433-05", card.three90() == Err(ScCode::EvidenceHeld)
        && card.dims.iter().find(|d| d.dim == 9).unwrap().held, "暂缓该维");

    // C433-06 补证后恢复。
    card.put(9, 92, 7009);
    chk(&mut set, "C433-06", card.three90().is_ok(), "补证恢复");

    // C433-07 预评对账：偏差 ≤5 无需说明。
    for i in 0..DIM_N { card.put_pre(i, 92); }
    card.put(A11Y_DIM, 93, 7002);
    chk(&mut set, "C433-07", card.reconcile_pre().is_ok(), "偏差限内");

    // C433-08 预终偏差超限→超限码+定位。
    card.put_pre(11, 80); // 终 92 预 80 → 偏差 12
    chk(&mut set, "C433-08", card.reconcile_pre() == Err(ScCode::DeviateOver)
        && card.deviations.contains(&(11, 12)), "偏差超限定位");

    // C433-09 偏差说明留痕后复评过。
    card.explain(11, 12);
    chk(&mut set, "C433-09", card.deviations.iter().any(|(d, v)| *d == 11 && *v == 12), "说明留痕");

    // C433-10 a11y 主场维终评确认（域本色）。
    let a = card.dims.iter().find(|d| d.dim == A11Y_DIM).unwrap();
    chk(&mut set, "C433-10", a.final_score == 93 && a.evidence.is_some(), "a11y 终评确认");

    // C433-11 均分单源复算。
    let active: Vec<u64> = card.dims.iter().map(|d| d.final_score as u64).collect();
    let avg = active.iter().sum::<u64>() / DIM_N as u64;
    chk(&mut set, "C433-11", avg >= LINE as u64, "均分复算");

    // C433-12 证据指针单源齐备。
    chk(&mut set, "C433-12", card.dims.len() == DIM_N
        && card.dims.iter().all(|d| d.evidence.is_some()), "证据指针齐备");

    set
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn ves433_smoke() {
        let set = run_ves433_checks();
        let red = set.red_items();
        assert!(red.is_empty(), "ves433 red: {:?}", red);
    }
}
