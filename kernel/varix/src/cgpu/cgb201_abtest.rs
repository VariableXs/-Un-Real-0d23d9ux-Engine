//! CGPU-F0201 · 帧图 A/B 对照（判据段 0x0630）
//! 帧图优化的 A/B 验证：双图并行执行（同一帧的优化前后图各跑一遍
//! ——A/B 帧对照 F0169 的图级版）、结果一致性（A/B 的像素输出必须
//! 一致——优化不改语义）、性能对比（A/B 耗时差量化——优化收益量
//! 化）。判据：双图执行、一致性断言、耗时对比、收益量化、回归集成。
//!
//! 诊断码段 0x0630（本模块独占：0x0630~0x063F）。
//! no_std + alloc 零 unwrap，全部 Result 化。

use alloc::vec::Vec;

/// 诊断码段基址。
pub const SEG_BASE: u16 = 0x0630;

/// 收益判定门槛（千分比）：A/B 耗时差 ≥30‰ 才记为有效优化。
pub const GAIN_THRESHOLD_PERMILLE: u32 = 30;

/// A/B 对照样本数下限：少于该数不做收益结论（噪声主导）。
pub const AB_SAMPLE_MIN: usize = 5;

/// A/B 图侧标识。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum AbSide {
    /// A = 优化前基线图。
    Baseline,
    /// B = 优化后候选图。
    Candidate,
}

impl AbSide {
    /// 侧序号。
    pub fn ordinal(&self) -> u8 {
        match self {
            AbSide::Baseline => 0,
            AbSide::Candidate => 1,
        }
    }
}

/// 一次 A/B 对照运行（判据「双图执行」原子量）：同帧号、双图各跑
/// 一遍，各自产出像素指纹与耗时。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct AbRun {
    /// 帧号。
    pub frame: u32,
    /// A 图像素输出指纹。
    pub fp_a: u64,
    /// B 图像素输出指纹。
    pub fp_b: u64,
    /// A 图耗时（微秒）。
    pub time_a_us: u64,
    /// B 图耗时（微秒）。
    pub time_b_us: u64,
}

impl AbRun {
    /// 合法性。
    pub fn validate(&self) -> Result<(), u16> {
        if self.frame == 0 || self.time_a_us == 0 || self.time_b_us == 0 {
            return Err(SEG_BASE);
        }
        if self.fp_a == 0 || self.fp_b == 0 {
            return Err(SEG_BASE + 1);
        }
        Ok(())
    }
    /// 单次一致性（判据「一致性断言」原子断言）。
    pub fn consistent(&self) -> bool {
        self.fp_a == self.fp_b
    }
    /// 单次收益千分比（A 耗时 − B 耗时)/A ×1000，可为负（B 更慢）。
    pub fn gain_permille(&self) -> Result<i64, u16> {
        self.validate()?;
        if self.time_a_us == 0 {
            return Err(SEG_BASE + 2);
        }
        Ok(((self.time_a_us as i64 - self.time_b_us as i64) * 1000) / self.time_a_us as i64)
    }
}

/// A/B 对照台账（判据「双图执行」容器）。
#[derive(Default)]
pub struct AbLedger {
    runs: Vec<AbRun>,
}

impl AbLedger {
    /// 记录一次对照。
    pub fn record(&mut self, r: AbRun) -> Result<(), u16> {
        r.validate()?;
        if self.runs.iter().any(|x| x.frame == r.frame) {
            return Err(SEG_BASE + 3);
        }
        self.runs.push(r);
        Ok(())
    }
    /// 台账规模。
    pub fn count(&self) -> usize {
        self.runs.len()
    }
    /// 指定帧查询。
    pub fn get(&self, frame: u32) -> Result<&AbRun, u16> {
        self.runs.iter().find(|r| r.frame == frame).ok_or(SEG_BASE + 4)
    }
    /// 全部帧号（升序）。
    pub fn frames(&self) -> Vec<u32> {
        let mut f: Vec<u32> = self.runs.iter().map(|r| r.frame).collect();
        f.sort_unstable();
        f
    }
}

/// 一致性总断言（判据「一致性断言」主判据）：台账内全部 A/B 输出
/// 指纹必须两两相同——一条不一致即整个优化被否决（优化不改语义
/// 是硬约束）。返回通过样本数。
pub fn assert_all_consistent(ledger: &AbLedger) -> Result<usize, u16> {
    if ledger.count() == 0 {
        return Err(SEG_BASE + 5);
    }
    let mut ok = 0usize;
    for r in &ledger.runs {
        if !r.consistent() {
            return Err(SEG_BASE + 6);
        }
        ok += 1;
    }
    Ok(ok)
}

/// 收益量化（判据「收益量化」主判据）：全部一致样本的平均收益
/// 千分比（含符号）与达标判定。样本不足拒绝下结论。
pub fn quantify_gain(ledger: &AbLedger) -> Result<(i64, bool), u16> {
    if ledger.count() < AB_SAMPLE_MIN {
        return Err(SEG_BASE + 7);
    }
    assert_all_consistent(ledger)?;
    let mut sum: i64 = 0;
    for r in &ledger.runs {
        sum += r.gain_permille()?;
    }
    let avg = sum / ledger.count() as i64;
    Ok((avg, avg as u32 >= GAIN_THRESHOLD_PERMILLE as i32 as u32))
}

/// 逐帧收益序列（判据「耗时对比」明细面）：按帧序返回 (帧, 收益‰)。
pub fn gain_series(ledger: &AbLedger) -> Result<Vec<(u32, i64)>, u16> {
    if ledger.count() == 0 {
        return Err(SEG_BASE + 8);
    }
    let mut out: Vec<(u32, i64)> = Vec::new();
    for f in ledger.frames() {
        let r = ledger.get(f)?;
        out.push((f, r.gain_permille()?));
    }
    Ok(out)
}

/// 收益稳定性（判据「回归集成」门禁面）：全部帧收益同号（全正或
/// 全负）才稳定；正负混杂说明优化对部分帧劣化——需人工审查。
pub fn gain_stable(ledger: &AbLedger) -> Result<u8, u16> {
    let series = gain_series(ledger)?;
    if series.len() < AB_SAMPLE_MIN {
        return Err(SEG_BASE + 9);
    }
    let has_pos = series.iter().any(|&(_, g)| g > 0);
    let has_neg = series.iter().any(|&(_, g)| g < 0);
    if has_pos && has_neg {
        return Err(SEG_BASE + 10);
    }
    Ok(if has_pos { 1 } else { 0 })
}

/// 双图结构等价预检（判据「双图执行」前置面）：A/B 图必须同节点
/// 数同边数（纯重排/合并类优化允许边数变化——但节点数必须一致，
/// 节点数不同的对照是无效对照）。
pub fn structural_precheck(nodes_a: usize, edges_a: usize, nodes_b: usize, edges_b: usize) -> Result<(), u16> {
    if nodes_a == 0 || nodes_b == 0 {
        return Err(SEG_BASE + 11);
    }
    if nodes_a != nodes_b {
        return Err(SEG_BASE + 12);
    }
    if edges_a == 0 || edges_b == 0 {
        return Err(SEG_BASE + 13);
    }
    Ok(())
}

/// A/B 帧对照指纹（可审计面）：单次运行的不可篡改指纹。
pub fn ab_run_fp(r: &AbRun) -> u64 {
    let mut acc: u64 = 0xcbf29ce484222325;
    acc = acc.wrapping_mul(0x100000001b3) ^ r.frame as u64;
    acc = acc.wrapping_mul(0x100000001b3) ^ r.fp_a;
    acc = acc.wrapping_mul(0x100000001b3) ^ r.fp_b;
    acc = acc.wrapping_mul(0x100000001b3) ^ r.time_a_us;
    acc = acc.wrapping_mul(0x100000001b3) ^ r.time_b_us;
    acc
}

/// 回归门禁（判据「回归集成」主判据）：优化合入前的门禁裁决——
/// ① 全样本一致 ② 样本数达标 ③ 平均收益达标 ④ 收益稳定。
/// 全过返回收益值，任一不过返回对应错误码。
pub fn regression_gate(ledger: &AbLedger) -> Result<i64, u16> {
    if ledger.count() < AB_SAMPLE_MIN {
        return Err(SEG_BASE + 14);
    }
    assert_all_consistent(ledger)?;
    let (gain, enough) = quantify_gain(ledger)?;
    if !enough {
        return Err(SEG_BASE + 15);
    }
    gain_stable(ledger)?;
    Ok(gain)
}

/// A/B 双图执行模型（独立函数面）：同一帧的双图在执行器里各跑
/// 一遍的时序核算——串行执行总耗时 = A + B（对照本身的开销由
/// 对照窗口承担，不计入游戏帧）。
pub fn ab_execution_model(time_a_us: u64, time_b_us: u64, ab_window_budget_us: u64) -> Result<bool, u16> {
    if time_a_us == 0 || time_b_us == 0 || ab_window_budget_us == 0 {
        return Err(SEG_BASE + 16);
    }
    let total = time_a_us
        .checked_add(time_b_us)
        .ok_or(SEG_BASE + 17)?;
    Ok(total <= ab_window_budget_us)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn run(frame: u32, fp: u64, ta: u64, tb: u64) -> AbRun {
        AbRun { frame, fp_a: fp, fp_b: fp, time_a_us: ta, time_b_us: tb }
    }
    #[test]
    fn single_run_math() {
        let r = run(1, 0xABC, 1000, 800);
        assert!(r.consistent());
        assert_eq!(r.gain_permille().unwrap(), 200);
    }
    #[test]
    fn inconsistent_detected() {
        let mut l = AbLedger::default();
        for i in 1..=5u32 {
            l.record(run(i, 0x100 + i as u64, 1000, 900)).unwrap();
        }
        assert!(assert_all_consistent(&l).is_ok());
        // 塞入一条不一致 → 全局否决。
        let mut bad = run(9, 0x500, 1000, 900);
        bad.fp_b = 0x999;
        l.record(bad).unwrap();
        assert_eq!(assert_all_consistent(&l).unwrap_err(), SEG_BASE + 6);
    }
    #[test]
    fn gain_quantified_and_gated() {
        let mut l = AbLedger::default();
        for i in 1..=6u32 {
            l.record(run(i, 0x42, 1000, 850)).unwrap();
        }
        let (gain, enough) = quantify_gain(&l).unwrap();
        assert_eq!(gain, 150);
        assert!(enough);
        assert_eq!(regression_gate(&l).unwrap(), 150);
    }
    #[test]
    fn insufficient_samples_rejected() {
        let mut l = AbLedger::default();
        for i in 1..=4u32 {
            l.record(run(i, 0x42, 1000, 800)).unwrap();
        }
        assert!(quantify_gain(&l).is_err());
        assert!(regression_gate(&l).is_err());
    }
    #[test]
    fn mixed_gain_unstable() {
        let mut l = AbLedger::default();
        l.record(run(1, 0x7, 1000, 900)).unwrap();
        l.record(run(2, 0x7, 1000, 900)).unwrap();
        l.record(run(3, 0x7, 1000, 1200)).unwrap(); // B 更慢。
        l.record(run(4, 0x7, 1000, 900)).unwrap();
        l.record(run(5, 0x7, 1000, 900)).unwrap();
        assert!(gain_stable(&l).is_err());
    }
    #[test]
    fn structural_precheck_rules() {
        assert!(structural_precheck(10, 12, 10, 10).is_ok());
        assert!(structural_precheck(10, 12, 9, 10).is_err());
        assert!(structural_precheck(10, 0, 10, 5).is_err());
    }
    #[test]
    fn ab_window_model() {
        assert!(ab_execution_model(400, 400, 1000).unwrap());
        assert!(!ab_execution_model(600, 500, 1000).unwrap());
        assert!(ab_execution_model(0, 1, 1).is_err());
    }
}
