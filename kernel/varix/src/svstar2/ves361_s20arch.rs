//! ves361_s20arch — VE-F4121 · T07 开工与 i18n 性能总架构
//! T07 定位（成本模型/预算/基准/回归/内存/降级/长稳七线）+ 家族框架复用（Q/R 性能组同构——框架单源+i18n 特化参数）+ 总架构五段闭环（度量→建模→预算→门禁→调优）+ 参数越域钳制 + 无障碍性能预算专列（色标/朗读/快照——域本色）。
#![allow(dead_code)]
use alloc::vec;
use alloc::vec::Vec;
use alloc::string::String;
use crate::checks::CheckSet;

pub const SEG_BASE: u16 = 0xCF40;

#[repr(u16)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ArCode {
    Ok = 0,
    ParamClamped = SEG_BASE + 0x01,
    FrameDrift = SEG_BASE + 0x02,
    StageMismatch = SEG_BASE + 0x03,
}

/// 七线。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Line { CostModel = 0, Budget = 1, Bench = 2, Regression = 3, Memory = 4, Degrade = 5, Soak = 6 }

impl Line {
    pub const ALL: [Line; 7] = [
        Line::CostModel, Line::Budget, Line::Bench, Line::Regression,
        Line::Memory, Line::Degrade, Line::Soak];
    pub fn name(self) -> &'static str {
        match self { Line::CostModel => "cost-model", Line::Budget => "budget", Line::Bench => "bench",
            Line::Regression => "regression", Line::Memory => "memory", Line::Degrade => "degrade", Line::Soak => "soak" }
    }
}

/// 五段闭环。
pub const FIVE_STAGES: [&'static str; 5] = ["measure", "model", "budget", "gate", "tune"];

/// 家族框架单源：Q/R 性能组同构的段序。
pub const FAMILY_STAGES: [&'static str; 5] = FIVE_STAGES;

/// i18n 特化参数（越域钳制）。
#[derive(Clone, Copy)]
pub struct I18nParams {
    pub lookup_weight_bp: u32,   // 查找线权重（万分比）
    pub layout_weight_bp: u32,   // 排版线权重
    pub load_weight_bp: u32,     // 资源加载线权重
    /// 无障碍专列权重（色标/朗读/快照三列合计）——域本色。
    pub a11y_weight_bp: u32,
}

pub const PARAM_MAX_BP: u32 = 10_000;

impl I18nParams {
    pub fn new() -> Self {
        I18nParams { lookup_weight_bp: 4_000, layout_weight_bp: 3_000, load_weight_bp: 2_000, a11y_weight_bp: 1_000 }
    }

    /// 权重和必须恰为 10000（单源守恒）；越域钳制。
    pub fn clamp(&mut self) -> bool {
        let mut clamped = false;
        for w in [&mut self.lookup_weight_bp, &mut self.layout_weight_bp, &mut self.load_weight_bp, &mut self.a11y_weight_bp] {
            if *w > PARAM_MAX_BP { *w = PARAM_MAX_BP; clamped = true; }
        }
        let sum = self.lookup_weight_bp + self.layout_weight_bp + self.load_weight_bp + self.a11y_weight_bp;
        if sum != PARAM_MAX_BP {
            // 按比例归一（保守：全部等比缩到守恒）。
            self.lookup_weight_bp = self.lookup_weight_bp * PARAM_MAX_BP / sum;
            self.layout_weight_bp = self.layout_weight_bp * PARAM_MAX_BP / sum;
            self.load_weight_bp = self.load_weight_bp * PARAM_MAX_BP / sum;
            self.a11y_weight_bp = PARAM_MAX_BP - self.lookup_weight_bp - self.layout_weight_bp - self.load_weight_bp;
            clamped = true;
        }
        clamped
    }

    pub fn conserved(&self) -> bool {
        self.lookup_weight_bp + self.layout_weight_bp + self.load_weight_bp + self.a11y_weight_bp == PARAM_MAX_BP
    }
}

/// 七线映射：线 → 五段中的职责段。
pub struct LineMap { pub owner: [u8; 7] } // 每线主责段 idx

impl LineMap {
    pub fn new() -> Self {
        // cost-model→model(1) budget→budget(2) bench→measure(0) regression→gate(3)
        // memory→tune(4) degrade→tune(4) soak→measure(0)
        LineMap { owner: [1, 2, 0, 3, 4, 4, 0] }
    }
    pub fn stage_of(&self, l: Line) -> &'static str { FIVE_STAGES[self.owner[l as usize] as usize] }
    /// 七线全覆盖：每段至少一线主责。
    pub fn covers_all(&self) -> bool {
        FIVE_STAGES.iter().enumerate().all(|(i, _)| self.owner.iter().any(|o| *o as usize == i))
    }
}

/// 无障碍专列（域本色）：三列预算（色标/朗读/快照）。
pub const A11Y_COLUMNS: [&'static str; 3] = ["color-mark", "speech", "snapshot"];

pub struct A11yBudget { pub cols: [u32; 3] } // 每列 ms 预算

impl A11yBudget {
    pub fn new() -> Self { A11yBudget { cols: [8, 40, 60] } }
    /// 专列不减红线：任何调优不得低于保底。
    pub fn floor_respected(&self, tuned: &[u32; 3]) -> bool {
        tuned.iter().zip(self.cols.iter()).all(|(t, f)| t >= f)
    }
}

/// 段间对拍：五段闭环序一致。
pub fn stages_agree(a: &[&'static str], b: &[&'static str]) -> bool {
    a.len() == b.len() && a.iter().zip(b.iter()).all(|(x, y)| x == y)
}

fn chk(set: &mut CheckSet, label: &'static str, ok: bool, detail: &'static str) { set.add(label, ok, detail); }

pub fn run_ves361_checks() -> CheckSet {
    let mut set = CheckSet::new("ves361");

    // C361-01 七线名单单源。
    chk(&mut set, "C361-01", Line::ALL.len() == 7 && Line::ALL[6].name() == "soak", "七线单源");

    // C361-02 五段闭环单源。
    chk(&mut set, "C361-02", FIVE_STAGES == ["measure", "model", "budget", "gate", "tune"], "五段闭环");

    // C361-03 框架单源：与家族段序一致。
    chk(&mut set, "C361-03", stages_agree(&FIVE_STAGES, &FAMILY_STAGES), "框架与家族对拍一致");

    // C361-04 框架漂移检出。
    let drifted = ["measure", "model", "gate", "budget", "tune"];
    chk(&mut set, "C361-04", !stages_agree(&FIVE_STAGES, &drifted), "段序漂移检出");

    // C361-05 特化参数守恒。
    let p = I18nParams::new();
    chk(&mut set, "C361-05", p.conserved(), "四线权重和 10000");

    // C361-06 越域钳制：单参超界。
    let mut p2 = I18nParams { lookup_weight_bp: 20_000, layout_weight_bp: 3_000, load_weight_bp: 2_000, a11y_weight_bp: 1_000 };
    let cl = p2.clamp();
    chk(&mut set, "C361-06", cl && p2.conserved() && p2.lookup_weight_bp <= PARAM_MAX_BP, "超界钳制+归一");

    // C361-07 钳制幂等：守恒参数再钳不动。
    let mut p3 = I18nParams::new();
    chk(&mut set, "C361-07", !p3.clamp(), "守恒再钳无动作");

    // C361-08 七线映射全覆盖五段。
    let lm = LineMap::new();
    chk(&mut set, "C361-08", lm.covers_all(), "七线覆盖五段");

    // C361-09 线→段查询单源。
    chk(&mut set, "C361-09",
        lm.stage_of(Line::CostModel) == "model" && lm.stage_of(Line::Regression) == "gate"
        && lm.stage_of(Line::Bench) == "measure", "三线段归属");

    // C361-10 无障碍专列三列。
    let ab = A11yBudget::new();
    chk(&mut set, "C361-10", A11Y_COLUMNS.len() == 3 && ab.cols == [8, 40, 60], "专列三列预算");

    // C361-11 专列不减红线：调优高于保底过。
    chk(&mut set, "C361-11", ab.floor_respected(&[8, 45, 70]), "调优不减过");

    // C361-12 专列红线实测：削减朗读预算 → 拒。
    chk(&mut set, "C361-12", !ab.floor_respected(&[8, 30, 60]), "朗读 30 < 40 => 红线拒");

    set
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ves361_smoke() {
        let set = run_ves361_checks();
        assert!(set.failed_count() == 0, "ves361 red: {}", set.red_items_text());
    }

    #[test]
    fn ves361_clamp() {
        let mut p = I18nParams { lookup_weight_bp: 0, layout_weight_bp: 0, load_weight_bp: 0, a11y_weight_bp: 20_000 };
        assert!(p.clamp());
        assert!(p.conserved());
        assert_eq!(p.a11y_weight_bp, PARAM_MAX_BP);
    }
}
