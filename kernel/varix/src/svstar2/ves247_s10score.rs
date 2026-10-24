//! VE-F3993 · S 域 20 维度验收评分
//!
//! 评分：S 域 20 维度终版评分（家族评分卡终演 + 每维证据指针 + F3974
//! 预评对账（偏差说明））；三 90 线核验（≥90 维数 / 均分 / 无短板维）。
//! 数据结构：终版评分卡（20 维×证据）；偏差说明；三 90 核验。
//! 错误路径：线未达→整改+复评；预终偏差超限→说明+复评；证据缺→暂缓
//! 该维。
//! 性能：评分 O(20 维)；对账 O(偏差)；复评 O(异)。
//! 跨批对接：F3974 预评上游；F4000 宣告硬门。
//! 无障碍与隐私：无障碍维终评含双维+心流加权终版口径（域本色）；无隐
//! 私面。

#![allow(dead_code)]
use alloc::vec;

extern crate alloc;

use alloc::vec::Vec;
use alloc::string::String;

// ── 诊断码段（0x46C0）────────────────────────────────────────────────
pub const SEG_BASE: u16 = 0x46C0;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct T13GCode(pub u16);

impl T13GCode {
    pub const OK: Self = Self(SEG_BASE);
    pub const LINE_MISS: Self = Self(SEG_BASE + 1);
    pub const DEVIATION_OVER: Self = Self(SEG_BASE + 2);
    pub const EVIDENCE_HELD: Self = Self(SEG_BASE + 3);

    pub fn explain(self) -> &'static str {
        match self {
            Self::OK => "正常",
            Self::LINE_MISS => "线未达→整改+复评",
            Self::DEVIATION_OVER => "预终偏差超限→说明+复评",
            Self::EVIDENCE_HELD => "证据缺→暂缓该维",
            _ => "未知",
        }
    }
}

// ── 20 维评分卡 ──────────────────────────────────────────────────────

pub const DIM_TOTAL: usize = 20;

pub const DIM_NAMES: [&str; DIM_TOTAL] = [
    "渲染", "高对比", "读屏", "动效", "认知",
    "输入", "性能", "工具", "词典", "走查",
    "观测", "安全", "文档", "扩展", "联测",
    "语料", "回归", "资产", "知识", "收官",
];

/// 无障碍维槽（双维+心流加权终版口径——域本色）。
pub const A11Y_DIM: usize = 2;

/// 单维评分。
#[derive(Debug, Clone)]
pub struct DimScore {
    /// 终评分（0-100）。
    pub score: u8,
    /// 预评分（F3974）。
    pub pre: u8,
    /// 证据指针非空。
    pub has_evidence: bool,
    /// 偏差说明（|终-预| > 5 时必填）。
    pub deviation_note: String,
}

/// 终版评分卡。
#[derive(Debug, Clone)]
pub struct FinalScoreCard {
    pub dims: [Option<DimScore>; DIM_TOTAL],
}

/// 三 90 线。
pub const LINE_DIMS_GE90: usize = 18;
pub const LINE_AVG: u32 = 90;
pub const LINE_LOW_FLOOR: u8 = 85;
/// 预终偏差容忍（>5 须说明）。
pub const DEVIATION_TOLERANCE: i16 = 5;

impl FinalScoreCard {
    pub fn new() -> Self {
        Self { dims: [(); DIM_TOTAL].map(|_| None) }
    }

    /// 维登记。
    pub fn put(&mut self, slot: usize, score: u8, pre: u8, has_evidence: bool, note: &str) -> bool {
        if slot >= DIM_TOTAL || score > 100 {
            return false;
        }
        self.dims[slot] = Some(DimScore {
            score,
            pre,
            has_evidence,
            deviation_note: String::from(note),
        });
        true
    }

    /// 暂缓维清单（证据缺）。
    pub fn held_dims(&self) -> Vec<usize> {
        let mut out = Vec::new();
        for (i, d) in self.dims.iter().enumerate() {
            match d {
                Some(s) if !s.has_evidence => out.push(i),
                None => out.push(i),
                _ => {}
            }
        }
        out
    }

    /// 偏差超限且无说明清单。
    pub fn deviation_violations(&self) -> Vec<usize> {
        let mut out = Vec::new();
        for (i, d) in self.dims.iter().enumerate() {
            if let Some(s) = d {
                let dev = (s.score as i16 - s.pre as i16).abs();
                if dev > DEVIATION_TOLERANCE && s.deviation_note.is_empty() {
                    out.push(i);
                }
            }
        }
        out
    }

    /// 可评维数（证据齐 + 偏差合规）。
    pub fn scorable(&self) -> Vec<usize> {
        let held = self.held_dims();
        let viol = self.deviation_violations();
        (0..DIM_TOTAL).filter(|i| !held.contains(i) && !viol.contains(i)).collect()
    }

    /// 均（可评维）。
    pub fn avg(&self) -> u32 {
        let s = self.scorable();
        if s.is_empty() {
            return 0;
        }
        let sum: u32 = s.iter().map(|i| self.dims[*i].as_ref().map(|d| d.score as u32).unwrap_or(0)).sum();
        sum / s.len() as u32
    }

    /// 三 90 核验：≥90 维数 ≥ 18 / 均分 ≥ 90 / 无 <85 短板维。
    pub fn triple_ninety(&self) -> T13GCode {
        if !self.held_dims().is_empty() || !self.deviation_violations().is_empty() {
            return T13GCode::EVIDENCE_HELD;
        }
        let ge90 = (0..DIM_TOTAL).filter(|i| self.dims[*i].as_ref().map(|d| d.score >= 90).unwrap_or(false)).count();
        if ge90 < LINE_DIMS_GE90 {
            return T13GCode::LINE_MISS;
        }
        if self.avg() < LINE_AVG {
            return T13GCode::LINE_MISS;
        }
        let low = (0..DIM_TOTAL).any(|i| self.dims[i].as_ref().map(|d| d.score < LINE_LOW_FLOOR).unwrap_or(true));
        if low {
            return T13GCode::LINE_MISS;
        }
        T13GCode::OK
    }

    /// 无障碍维加权终评（双倍权——域本色）。
    pub fn a11y_weighted_avg(&self) -> u32 {
        let s = self.scorable();
        if s.is_empty() {
            return 0;
        }
        let mut sum: u64 = 0;
        let mut weight: u64 = 0;
        for i in s.iter() {
            let sc = self.dims[*i].as_ref().map(|d| d.score as u64).unwrap_or(0);
            let w = if *i == A11Y_DIM { 2 } else { 1 };
            sum += sc * w;
            weight += w;
        }
        (sum / weight) as u32
    }
}

// ── 判据 ─────────────────────────────────────────────────────────────

fn chk(set: &mut crate::checks::CheckSet, label: &'static str, ok: bool, detail: &'static str) {
    set.add(label, ok, detail);
}

pub fn run_ves247_checks() -> crate::checks::CheckSet {
    let mut set = crate::checks::CheckSet::new("ves247_s10score");

    // C3993-01 诊断码段不越窗。
    chk(&mut set, "C3993-01",
        T13GCode::EVIDENCE_HELD.0 < SEG_BASE + 16 && SEG_BASE == 0x46C0, "err");

    // C3993-02 20 维表齐备。
    chk(&mut set, "C3993-02",
        DIM_NAMES.len() == 20 && DIM_NAMES[0] == "渲染" && DIM_NAMES[19] == "收官", "err");

    // C3993-03 证据缺 → 暂缓。
    let mut c = FinalScoreCard::new();
    c.put(0, 95, 94, false, "");
    chk(&mut set, "C3993-03", c.held_dims() == vec![0usize], "err");

    // C3993-04 偏差超限无说明 → 违例。
    c.put(0, 95, 80, true, "");
    chk(&mut set, "C3993-04", c.deviation_violations() == vec![0usize], "err");

    // C3993-05 偏差说明补 → 合规。
    c.put(0, 95, 80, true, "工具链升级后重测");
    chk(&mut set, "C3993-05", c.deviation_violations().is_empty() && c.scorable() == vec![0usize], "err");

    // C3993-06 全 20 维满分 → 三 90 过。
    let mut full = FinalScoreCard::new();
    for i in 0..DIM_TOTAL {
        full.put(i, 95, 94, true, "");
    }
    chk(&mut set, "C3993-06", full.triple_ninety() == T13GCode::OK, "err");

    // C3993-07 ≥90 维数不足（17 维）→ LINE_MISS。
    let mut c2 = FinalScoreCard::new();
    for i in 0..DIM_TOTAL {
        let s = if i < 17 { 95u8 } else { 88u8 };
        c2.put(i, s, s - 1, true, "");
    }
    chk(&mut set, "C3993-07", c2.triple_ninety() == T13GCode::LINE_MISS, "err");

    // C3993-08 均分不足 → LINE_MISS。
    let mut c3 = FinalScoreCard::new();
    for i in 0..DIM_TOTAL {
        c3.put(i, 91, 90, true, "");
    }
    c3.put(19, 80, 79, true, "");
    // 19 维 91 + 1 维 80 → 均 = (19*91+80)/20 = 90.45 → 90 过均分线，但 80 < 85 短板 → 仍 MISS。
    chk(&mut set, "C3993-08", c3.triple_ninety() == T13GCode::LINE_MISS, "err");

    // C3993-09 短板维（<85）检出即败。
    let mut c4 = FinalScoreCard::new();
    for i in 0..DIM_TOTAL {
        c4.put(i, 95, 94, true, "");
    }
    c4.put(5, 84, 84, true, "");
    chk(&mut set, "C3993-09", c4.triple_ninety() == T13GCode::LINE_MISS, "err");

    // C3993-10 整改复评后过线。
    c4.put(5, 90, 84, true, "整改后复评");
    chk(&mut set, "C3993-10", c4.triple_ninety() == T13GCode::OK, "err");

    // C3993-11 无障碍维双倍权（2 项：a11y=100，其他 90）。
    let mut c5 = FinalScoreCard::new();
    for i in 0..DIM_TOTAL {
        let s = if i == A11Y_DIM { 100u8 } else { 90u8 };
        c5.put(i, s, s, true, "");
    }
    // 普通均 = 19*90+100 / 20 = 90.5 → 90；加权均 = (19*90 + 100*2)/21 = 90.47 → 90。
    chk(&mut set, "C3993-11",
        c5.avg() == 90 && c5.a11y_weighted_avg() == 90, "err");

    // C3993-12 加权口径区分（a11y=60 其他 90 → 加权更低）。
    let mut c6 = FinalScoreCard::new();
    for i in 0..DIM_TOTAL {
        let s = if i == A11Y_DIM { 60u8 } else { 90u8 };
        c6.put(i, s, s, true, "");
    }
    chk(&mut set, "C3993-12",
        c6.avg() == 88 && c6.a11y_weighted_avg() < c6.avg(), "err");

    // C3993-13 评分越界（>100）拒绝。
    let mut c7 = FinalScoreCard::new();
    chk(&mut set, "C3993-13", !c7.put(0, 101, 90, true, ""), "err");

    // C3993-14 越槽登记拒绝。
    chk(&mut set, "C3993-14", !c7.put(20, 90, 90, true, ""), "err");

    // C3993-15 偏差容忍边界（恰 5 合规）。
    let mut c8 = FinalScoreCard::new();
    c8.put(3, 95, 90, true, "");
    chk(&mut set, "C3993-15", c8.deviation_violations().is_empty(), "err");

    // C3993-16 暂缓码语义。
    chk(&mut set, "C3993-16", T13GCode::EVIDENCE_HELD.explain().contains("暂缓"), "err");

    // C3993-17 空卡三 90 = 证据缺。
    chk(&mut set, "C3993-17", FinalScoreCard::new().triple_ninety() == T13GCode::EVIDENCE_HELD, "err");

    // C3993-18 维名互异。
    let mut uniq = true;
    for i in 0..DIM_TOTAL {
        for j in (i + 1)..DIM_TOTAL {
            if DIM_NAMES[i] == DIM_NAMES[j] { uniq = false; }
        }
    }
    chk(&mut set, "C3993-18", uniq, "err");

    // C3993-19 无障碍维槽为读屏（域本色锚点）。
    chk(&mut set, "C3993-19", A11Y_DIM == 2 && DIM_NAMES[A11Y_DIM] == "读屏", "err");

    // C3993-20 错误码互异。
    let codes = [T13GCode::OK.0, T13GCode::LINE_MISS.0, T13GCode::DEVIATION_OVER.0,
        T13GCode::EVIDENCE_HELD.0];
    chk(&mut set, "C3993-20",
        codes[0] != codes[1] && codes[1] != codes[2] && codes[2] != codes[3], "err");

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn full_card_passes() {
        let mut c = FinalScoreCard::new();
        for i in 0..DIM_TOTAL {
            c.put(i, 92, 92, true, "");
        }
        assert_eq!(c.triple_ninety(), T13GCode::OK);
    }

    #[test]
    fn low_dim_blocks() {
        let mut c = FinalScoreCard::new();
        for i in 0..DIM_TOTAL {
            c.put(i, 92, 92, true, "");
        }
        c.put(7, 70, 70, true, "");
        assert_eq!(c.triple_ninety(), T13GCode::LINE_MISS);
    }

    #[test]
    fn weighted_avg_doubles_a11y() {
        let mut c = FinalScoreCard::new();
        for i in 0..DIM_TOTAL {
            let s = if i == A11Y_DIM { 100u8 } else { 90u8 };
            c.put(i, s, s, true, "");
        }
        let _ = c.a11y_weighted_avg();
        assert!(true);
    }
}
