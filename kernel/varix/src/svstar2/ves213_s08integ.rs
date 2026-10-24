#![allow(dead_code)]
//! VE-F3959 · S08 联调
//!
//! 测试工具组合矩阵：全域测试工具（调度/审查器/仿真器/语料/回归/看板/
//! 参考）× 三平台 × 三辅具 = 63 格分层抽样（覆盖底线全域化）。真实剧本：
//! 20 分钟工具链剧本（全域扫描→仿真→真机→回归→看板五幕混流——重演确定
//! 性 + 信号采集）。故障注入：调度断/仿真失真/语料投毒三故障——全域红线
//! 实测场。三分流全域：P0 即断/立案/兜底。
//! 错误路径：仿真替代真机→P0；覆盖全域缺→补跑；重演分歧→立案；处置
//! 无责→兜底。
//! 性能：单组合 O(分钟)；全集夜间；剧本 O(20 分钟)；归因 O(维数)。
//! 跨批对接：F3941-F3958 全对端；F3960 输入；全域工具对端。无隐私面。

use alloc::vec;

extern crate alloc;

use alloc::vec::Vec;
use alloc::string::String;

// ── 诊断码段（0x2E40）────────────────────────────────────────────────
pub const SEG_BASE: u16 = 0x2E40;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct S8SCode(pub u16);

impl S8SCode {
    pub const OK: Self = Self(SEG_BASE);
    pub const SIM_SUBSTITUTE: Self = Self(SEG_BASE + 1);
    pub const COVER_SHORT: Self = Self(SEG_BASE + 2);
    pub const REPLAY_DIVERGE: Self = Self(SEG_BASE + 3);
    pub const NO_OWNER: Self = Self(SEG_BASE + 4);

    pub fn explain(self) -> &'static str {
        match self {
            Self::OK => "正常",
            Self::SIM_SUBSTITUTE => "仿真替代真机→P0",
            Self::COVER_SHORT => "覆盖全域缺→补跑",
            Self::REPLAY_DIVERGE => "重演分歧→立案",
            Self::NO_OWNER => "处置无责→兜底",
            _ => "未知",
        }
    }
}

// ── 63 格组合矩阵 ────────────────────────────────────────────────────

/// 七工具。
pub const TOOLS: [&str; 7] = ["调度", "审查器", "仿真器", "语料", "回归", "看板", "参考"];

/// 组合键：工具 × 平台 × 辅具。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Combo {
    pub tool: usize,
    pub platform: usize,
    pub aid: usize,
}

pub const COMBO_TOTAL: usize = 7 * 3 * 3;

/// 分层抽样器：等步长抽样（覆盖底线：抽样必触达全工具）。
#[derive(Debug, Clone)]
pub struct Stratified {
    pub sampled: Vec<Combo>,
}

impl Stratified {
    /// 全网格 63 格。
    pub fn full_grid() -> Vec<Combo> {
        let mut v = Vec::new();
        for t in 0..7 {
            for p in 0..3 {
                for a in 0..3 {
                    v.push(Combo { tool: t, platform: p, aid: a });
                }
            }
        }
        v
    }

    /// 步长抽样。
    pub fn sample(stride: usize) -> Self {
        let grid = Self::full_grid();
        let sampled = if stride == 0 { grid } else { grid.into_iter().step_by(stride).collect() };
        Self { sampled }
    }

    /// 抽样触达的工具种数。
    pub fn tools_covered(&self) -> usize {
        let mut seen = [false; 7];
        for c in self.sampled.iter() {
            seen[c.tool] = true;
        }
        seen.iter().filter(|s| **s).count()
    }
}

// ── 五幕剧本（重演确定性）────────────────────────────────────────────

/// 剧本五幕。
pub const ACTS: [&str; 5] = ["全域扫描", "仿真", "真机", "回归", "看板"];

/// 剧本执行账。
#[derive(Debug, Clone)]
pub struct ScriptRun {
    pub minutes: u32,
    /// 各幕信号值（确定性 LCG 生成）。
    pub signals: [u64; 5],
    /// 重演指纹。
    pub replay_fp: u64,
}

impl ScriptRun {
    /// 20 分钟五幕混流剧本（确定性生成）。
    pub fn twenty_minutes() -> Self {
        let mut signals = [0u64; 5];
        let mut x: u64 = 0x243F_6A88_85A3_08D3;
        for s in signals.iter_mut() {
            x = x.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
            *s = x;
        }
        let mut fp: u64 = 0xcbf2_9ce4_8422_2325;
        for s in signals.iter() {
            fp ^= s;
            fp = fp.wrapping_mul(0x100_0000_01b3);
        }
        Self { minutes: 20, signals, replay_fp: fp }
    }

    /// 五幕全过。
    pub fn acts_full(&self) -> bool {
        self.signals.iter().all(|s| *s != 0)
    }

    /// 重演确定性：两次运行指纹一致。
    pub fn deterministic(&self) -> bool {
        let again = ScriptRun::twenty_minutes();
        again.replay_fp == self.replay_fp && again.signals == self.signals
    }
}

// ── 三故障注入（红线实测场）──────────────────────────────────────────

/// 故障种别。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Fault {
    /// 调度断。
    SchedBreak,
    /// 仿真失真。
    SimDistort,
    /// 语料投毒。
    CorpusPoison,
}

pub const FAULTS: [Fault; 3] = [Fault::SchedBreak, Fault::SimDistort, Fault::CorpusPoison];

/// 故障演练结果：每故障必须被红线检出。
#[derive(Debug, Clone)]
pub struct FaultDrill {
    pub fault: Fault,
    /// 是否被检出。
    pub caught: bool,
}

/// 全域红线场：三故障全检出才算场过。
#[derive(Debug, Clone)]
pub struct RedlineField {
    pub drills: Vec<FaultDrill>,
}

impl RedlineField {
    pub fn new() -> Self {
        Self { drills: Vec::new() }
    }

    pub fn drill(&mut self, f: Fault, caught: bool) {
        self.drills.push(FaultDrill { fault: f, caught });
    }

    pub fn uncaught(&self) -> Vec<Fault> {
        FAULTS
            .iter()
            .filter(|f| !self.drills.iter().any(|d| d.fault == **f && d.caught))
            .map(|f| *f)
            .collect()
    }

    pub fn passed(&self) -> bool {
        self.uncaught().is_empty() && self.drills.len() >= FAULTS.len()
    }
}

impl Default for RedlineField {
    fn default() -> Self {
        Self::new()
    }
}

// ── 三分流全域（P0 即断 / 立案 / 兜底）────────────────────────────────

/// 处置三分流。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Triage {
    /// P0 即断。
    CutNow,
    /// 立案。
    Case,
    /// 兜底。
    Fallback,
}

/// 分流裁决：按错误码分流。
pub fn triage_of(code: S8SCode) -> Triage {
    match code {
        S8SCode::SIM_SUBSTITUTE => Triage::CutNow,
        S8SCode::REPLAY_DIVERGE | S8SCode::COVER_SHORT => Triage::Case,
        _ => Triage::Fallback,
    }
}

// ── 判据 ─────────────────────────────────────────────────────────────

pub fn run_ves213_checks() -> crate::checks::CheckSet {
    use crate::checks::CheckSet;
    fn chk(set: &mut CheckSet, label: &'static str, ok: bool, detail: &'static str) {
        set.add(label, ok, detail);
    }
    let mut set = CheckSet::new("ves213_s08integ");

    // 01 全网格 63 格守恒
    {
        chk(&mut set, "C3959-01", Stratified::full_grid().len() == 63 && COMBO_TOTAL == 63, "err");
    }
    // 02 分层抽样触达七工具
    {
        let s = Stratified::sample(9);
        chk(&mut set, "C3959-02", s.tools_covered() == 7, "err");
    }
    // 03 步长 1 全量
    {
        let s = Stratified::sample(1);
        chk(&mut set, "C3959-03", s.sampled.len() == 63, "err");
    }
    // 04 大步长覆盖不足被底线检出
    {
        let s = Stratified::sample(63);
        chk(&mut set, "C3959-04", s.sampled.len() == 1 && s.tools_covered() == 1, "err");
    }
    // 05 剧本 20 分钟五幕混流
    {
        let s = ScriptRun::twenty_minutes();
        chk(&mut set, "C3959-05", s.minutes == 20 && s.acts_full(), "err");
    }
    // 06 重演确定性：指纹一致
    {
        let s = ScriptRun::twenty_minutes();
        chk(&mut set, "C3959-06", s.deterministic(), "err");
    }
    // 07 重演分歧→立案
    {
        let mut s = ScriptRun::twenty_minutes();
        s.replay_fp = 0xDEAD_BEEF;
        chk(&mut set, "C3959-07", !s.deterministic() && triage_of(S8SCode::REPLAY_DIVERGE) == Triage::Case, "err");
    }
    // 08 三故障全检出
    {
        let mut f = RedlineField::new();
        for fl in FAULTS.iter() {
            f.drill(*fl, true);
        }
        chk(&mut set, "C3959-08", f.passed() && f.uncaught().is_empty(), "err");
    }
    // 09 漏检故障被红线场留难
    {
        let mut f = RedlineField::new();
        f.drill(Fault::SchedBreak, true);
        f.drill(Fault::SimDistort, true);
        f.drill(Fault::CorpusPoison, false);
        chk(&mut set, "C3959-09", !f.passed() && f.uncaught() == vec![Fault::CorpusPoison], "err");
    }
    // 10 三分流裁决映射
    {
        chk(&mut set, "C3959-10", triage_of(S8SCode::SIM_SUBSTITUTE) == Triage::CutNow
            && triage_of(S8SCode::COVER_SHORT) == Triage::Case
            && triage_of(S8SCode::NO_OWNER) == Triage::Fallback, "err");
    }
    // 11 仿真替代真机→P0 即断（剧本混流约束）
    {
        let s = ScriptRun::twenty_minutes();
        let sim_ok = s.signals[1] != 0;
        let dev_ok = s.signals[2] != 0;
        // 真机幕缺失时仿真幕再好也是 P0。
        let tri = if dev_ok { Triage::Case } else { Triage::CutNow };
        chk(&mut set, "C3959-11", sim_ok && dev_ok && tri == Triage::Case, "err");
    }
    // 12 覆盖缺→补跑分流
    {
        let s = Stratified::sample(20);
        let need_rerun = s.tools_covered() < 7;
        chk(&mut set, "C3959-12", need_rerun && triage_of(S8SCode::COVER_SHORT) == Triage::Case, "err");
    }
    // 13 诊断码窗界自守
    {
        chk(&mut set, "C3959-13", S8SCode::NO_OWNER.0 < SEG_BASE + 16, "err");
    }
    // 14 错误码解释覆盖
    {
        let all = [S8SCode::OK, S8SCode::SIM_SUBSTITUTE, S8SCode::COVER_SHORT, S8SCode::REPLAY_DIVERGE, S8SCode::NO_OWNER];
        chk(&mut set, "C3959-14", all.iter().all(|c| c.explain() != "未知"), "err");
    }

    set
}

// ── 测试支撑 ─────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_s08_integ_all_green() {
        let s = run_ves213_checks();
        assert!(s.all_passed(), "ves213 red: {}", s.red_items_text());
    }

    #[test]
    fn test_grid_tool_cycle() {
        // 每工具恰好 9 格。
        let grid = Stratified::full_grid();
        for t in 0..7 {
            assert_eq!(grid.iter().filter(|c| c.tool == t).count(), 9);
        }
    }

    #[test]
    fn test_triage_full_mapping() {
        let codes = [S8SCode::OK, S8SCode::SIM_SUBSTITUTE, S8SCode::COVER_SHORT, S8SCode::REPLAY_DIVERGE, S8SCode::NO_OWNER];
        let triages: Vec<Triage> = codes.iter().map(|c| triage_of(*c)).collect();
        assert!(triages.contains(&Triage::CutNow));
        assert!(triages.contains(&Triage::Case));
        assert!(triages.contains(&Triage::Fallback));
    }
}
