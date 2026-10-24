//! VE-F3974 · 20 维度预评分
//!
//! S 域 20 维度预评分（家族 20 维评分卡预演版）：20 维逐维打分 + 每维
//! 证据指针。预评分与终评分离声明（预评不外发——外泄→撤回+立案）。
//! 错误路径：证据缺→该维暂缓+登记；分数异常→复评；预评外泄→撤回+立案。
//! 性能：评分 O(20 维)；复评 O(异)；登记 O(1)。
//! 跨批对接：F3973 汇总上游；F3993 终版评分下游。
//! 无障碍与隐私：无障碍维预评含双维+心流加权口径（域本色）；无隐私面。

#![allow(dead_code)]
use alloc::vec;

extern crate alloc;

use alloc::vec::Vec;
use alloc::string::String;

// ── 诊断码段（0x3540）────────────────────────────────────────────────
pub const SEG_BASE: u16 = 0x3540;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct S9NCode(pub u16);

impl S9NCode {
    pub const OK: Self = Self(SEG_BASE);
    pub const EVIDENCE_HELD: Self = Self(SEG_BASE + 1);
    pub const SCORE_ANOMALY: Self = Self(SEG_BASE + 2);
    pub const LEAKED: Self = Self(SEG_BASE + 3);
    pub const DIM_OVER: Self = Self(SEG_BASE + 4);

    pub fn explain(self) -> &'static str {
        match self {
            Self::OK => "正常",
            Self::EVIDENCE_HELD => "证据缺→该维暂缓",
            Self::SCORE_ANOMALY => "分数异常→复评",
            Self::LEAKED => "预评外泄→撤回+立案",
            Self::DIM_OVER => "维度越界→拒收",
            _ => "未知",
        }
    }
}

// ── 20 维评分卡 ──────────────────────────────────────────────────────

/// 维数 20。
pub const DIMS: usize = 20;

/// 无障碍维下标（双维+心流加权口径主场）。
pub const A11Y_DIM: usize = 4;

/// 单维评分行。
#[derive(Debug, Clone)]
pub struct ScoreRow {
    pub dim: usize,
    /// 分数（0-100）。
    pub score: u8,
    /// 证据指针（0 = 缺）。
    pub evidence: u32,
    /// 暂缓标记。
    pub held: bool,
}

impl ScoreRow {
    pub fn valid(&self) -> bool {
        self.dim < DIMS && self.score <= 100
    }
}

/// 20 维评分卡（预演版）。
#[derive(Debug, Clone)]
pub struct PreScoreCard {
    pub rows: Vec<ScoreRow>,
    /// 分离声明：预评未外发。
    pub sealed: bool,
    /// 外泄标记。
    pub leaked: bool,
}

impl PreScoreCard {
    pub fn new() -> Self {
        Self { rows: Vec::new(), sealed: true, leaked: false }
    }

    /// 打分：证据缺 → 该维暂缓 + 登记。
    pub fn score(&mut self, dim: usize, s: u8, evidence: u32) -> S9NCode {
        if dim >= DIMS {
            return S9NCode::DIM_OVER;
        }
        if s > 100 {
            return S9NCode::SCORE_ANOMALY;
        }
        let held = evidence == 0;
        match self.rows.iter_mut().find(|r| r.dim == dim) {
            Some(r) => {
                r.score = s;
                r.evidence = evidence;
                r.held = held;
            }
            None => self.rows.push(ScoreRow { dim, score: s, evidence, held }),
        }
        if held {
            S9NCode::EVIDENCE_HELD
        } else {
            S9NCode::OK
        }
    }

    /// 暂缓维清单。
    pub fn held_dims(&self) -> Vec<usize> {
        self.rows.iter().filter(|r| r.held).map(|r| r.dim).collect()
    }

    /// 复评：分数异常检出（超出加权口径容差的重打）。
    pub fn anomaly(&self, expect: &[u8; DIMS], tol: u8) -> Vec<usize> {
        self.rows
            .iter()
            .filter(|r| !r.held && r.score.abs_diff(expect[r.dim]) > tol)
            .map(|r| r.dim)
            .collect()
    }

    /// 撤回：外泄后全卡作废。
    pub fn withdraw(&mut self) {
        self.leaked = true;
        self.rows.clear();
        self.sealed = false;
    }

    /// 可用断言：密封且无暂缓。
    pub fn usable(&self) -> bool {
        self.sealed && !self.leaked && self.rows.len() == DIMS && self.held_dims().is_empty()
    }

    /// 加权均分（无障碍维双倍权）。
    pub fn weighted_avg(&self) -> u32 {
        if self.rows.len() != DIMS {
            return 0;
        }
        let mut num: u64 = 0;
        let mut den: u64 = 0;
        for r in self.rows.iter() {
            let w: u64 = if r.dim == A11Y_DIM { 2 } else { 1 };
            num += r.score as u64 * w;
            den += w;
        }
        if den == 0 {
            0
        } else {
            (num / den) as u32
        }
    }
}

impl Default for PreScoreCard {
    fn default() -> Self {
        Self::new()
    }
}

// ── 预终分离声明 ─────────────────────────────────────────────────────

/// 分离声明规则。
pub const SEPARATION_RULES: [&str; 3] = ["预评不外发", "终评不预演", "两卡不同源"];

/// 分离合规断言：预评卡外发通道关闭。
pub fn separation_ok(card: &PreScoreCard, out_channel_open: bool) -> bool {
    card.sealed && !out_channel_open
}

// ── 判据 ─────────────────────────────────────────────────────────────

pub fn run_ves228_checks() -> crate::checks::CheckSet {
    use crate::checks::CheckSet;
    fn chk(set: &mut CheckSet, label: &'static str, ok: bool, detail: &'static str) {
        set.add(label, ok, detail);
    }
    let mut set = CheckSet::new("ves228_s09prescore");

    // 01 20 维全打分 + 证据齐
    {
        let mut c = PreScoreCard::new();
        for d in 0..DIMS {
            c.score(d, 90, 1000 + d as u32);
        }
        chk(&mut set, "C3974-01", c.usable() && c.rows.len() == 20, "err");
    }
    // 02 证据缺→暂缓+登记实测
    {
        let mut c = PreScoreCard::new();
        let code = c.score(7, 88, 0);
        chk(&mut set, "C3974-02", code == S9NCode::EVIDENCE_HELD && c.held_dims() == vec![7] && !c.usable(), "err");
    }
    // 03 补证据解除暂缓
    {
        let mut c = PreScoreCard::new();
        c.score(7, 88, 0);
        c.score(7, 88, 77);
        chk(&mut set, "C3974-03", c.held_dims().is_empty() && c.rows[0].evidence == 77, "err");
    }
    // 04 分数越界拒收
    {
        let mut c = PreScoreCard::new();
        chk(&mut set, "C3974-04", c.score(0, 101, 1) == S9NCode::SCORE_ANOMALY, "err");
    }
    // 05 维度越界拒收
    {
        let mut c = PreScoreCard::new();
        chk(&mut set, "C3974-05", c.score(20, 50, 1) == S9NCode::DIM_OVER, "err");
    }
    // 06 复评：异常检出
    {
        let mut c = PreScoreCard::new();
        let mut expect = [90u8; DIMS];
        expect[3] = 91;
        for d in 0..DIMS {
            let s = if d == 3 { 60 } else { expect[d] };
            c.score(d, s, 1);
        }
        chk(&mut set, "C3974-06", c.anomaly(&expect, 5) == vec![3], "err");
    }
    // 07 容差内不复评
    {
        let mut c = PreScoreCard::new();
        let expect = [90u8; DIMS];
        for d in 0..DIMS {
            let s = if d == 5 { 94 } else { 90 };
            c.score(d, s, 1);
        }
        chk(&mut set, "C3974-07", c.anomaly(&expect, 5).is_empty(), "err");
    }
    // 08 预评外泄→撤回+立案
    {
        let mut c = PreScoreCard::new();
        for d in 0..DIMS {
            c.score(d, 90, 1);
        }
        c.withdraw();
        chk(&mut set, "C3974-08", c.leaked && c.rows.is_empty() && !c.sealed && !c.usable(), "err");
    }
    // 09 预终分离合规
    {
        let c = PreScoreCard::new();
        chk(&mut set, "C3974-09", separation_ok(&c, false) && !separation_ok(&c, true), "err");
    }
    // 10 分离规则守恒
    {
        chk(&mut set, "C3974-10", SEPARATION_RULES.len() == 3 && SEPARATION_RULES[0] == "预评不外发", "err");
    }
    // 11 加权均分（无障碍双倍权）
    {
        let mut c = PreScoreCard::new();
        for d in 0..DIMS {
            let s = if d == A11Y_DIM { 80 } else { 90 };
            c.score(d, s, 1);
        }
        // (19*90 + 80*2) / 21 = (1710+160)/21 = 1870/21 = 89
        chk(&mut set, "C3974-11", c.weighted_avg() == 89, "err");
    }
    // 12 打分覆盖幂等（重打同维不重复行）
    {
        let mut c = PreScoreCard::new();
        c.score(2, 80, 1);
        c.score(2, 85, 2);
        chk(&mut set, "C3974-12", c.rows.len() == 1 && c.rows[0].score == 85, "err");
    }
    // 13 诊断码窗界自守
    {
        chk(&mut set, "C3974-13", S9NCode::DIM_OVER.0 < SEG_BASE + 16, "err");
    }
    // 14 错误码解释覆盖
    {
        let all = [S9NCode::OK, S9NCode::EVIDENCE_HELD, S9NCode::SCORE_ANOMALY, S9NCode::LEAKED, S9NCode::DIM_OVER];
        chk(&mut set, "C3974-14", all.iter().all(|c| c.explain() != "未知"), "err");
    }

    set
}

// ── 测试支撑 ─────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_s09_prescore_all_green() {
        let s = run_ves228_checks();
        assert!(s.all_passed(), "ves228 red: {}", s.red_items_text());
    }

    #[test]
    fn test_held_release_cycle() {
        let mut c = PreScoreCard::new();
        for d in 0..DIMS {
            let ev = if d == 9 { 0 } else { 1 };
            c.score(d, 88, ev).ok();
        }
        assert_eq!(c.held_dims(), vec![9]);
        c.score(9, 88, 9).ok();
        assert!(c.usable());
    }

    #[test]
    fn test_weighted_zero_guard() {
        let c = PreScoreCard::new();
        assert_eq!(c.weighted_avg(), 0);
    }
}
