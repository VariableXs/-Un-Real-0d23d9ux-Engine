//! CGPU-F0715 · 滚动基准（目标 370 行）
//! ==========================================================================
//! 锚点判据（清单原文摘录）：
//! - 纯功能代码 ≥300 行；② 规格与判据按册内锚点原文逐条落实
//!   （CGPU Varix STAR II · 总纲与施工书#CGPU-F0715，册内文档不在本仓，
//!    以清单判据为施工依据）；③ 完成时 API/软件写回：状态=已完成。
//! - 主题：滚动基准——对滚动子系统的确定性场景做整数成本模型
//!   计时、中位数取样、预算判定与对基线的回归比对。
//!
//! 设计纪律：
//! - 成本模型确定性：无真实时钟，用时MulCycles 模型——
//!   cycles = rows*ROW_CYCLES + cmd_submit*SUBMIT_CYCLES + 常量，
//!   系数来源注释写明（规格默认档）。
//! - 中位数取样：每场景跑 N 次（N=5，奇数），插排取中位，
//!   禁浮点均值。
//! - 预算判定：cycles ≤ budget 过；千分比超支入报告。
//! - 回归闭集：Better/Flat/Worse 三态，阈值 REGRESS_PM=50‰。
//! - 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，
//!   确定性。
//! - 诊断段 0x47a0 本单独占，错误码 SEG_BASE+1..+N 闭集。

use alloc::vec;
use alloc::vec::Vec;

/// 诊断段基址（0x47a0，批内 CGPU-F0715 独占）。
pub const SEG_BASE: u16 = 0x47a0;

/// 场景非法（0 帧或 0 表面）。
pub const E_SCENARIO: u16 = SEG_BASE + 1;
/// 样本数非法（必须为奇数且 ≥1）。
pub const E_SAMPLES: u16 = SEG_BASE + 2;
/// 预算超支（中位 cycles > budget）。
pub const E_BUDGET: u16 = SEG_BASE + 3;
/// 回归超阈值。
pub const E_REGRESS: u16 = SEG_BASE + 4;
/// 基线场景集与当前不一致（名不匹配）。
pub const E_BASELINE: u16 = SEG_BASE + 5;

/// 每滚动行成本（周期/行，规格默认档：16 行/帧 → 64 cycles/行）。
pub const ROW_CYCLES: u64 = 64;
/// 每命令提交成本（周期/命令）。
pub const SUBMIT_CYCLES: u64 = 48;
/// 每帧固定开销（周期/帧）。
pub const FRAME_CYCLES: u64 = 512;
/// 默认取样数（奇数）。
pub const SAMPLES: usize = 5;
/// 回归阈值（千分比）。
pub const REGRESS_PM: u64 = 50;

/// 滚动基准场景。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Scenario {
    /// 场景名。
    pub name: &'static str,
    /// 并发表面数。
    pub surfaces: u32,
    /// 每表面每帧行数。
    pub rows_per_surf: u32,
    /// 帧数。
    pub frames: u32,
    /// 预算（周期）。
    pub budget: u64,
}

impl Scenario {
    /// 形态校验。
    pub fn validate(&self) -> Result<(), u16> {
        if self.surfaces == 0 || self.frames == 0 {
            return Err(E_SCENARIO);
        }
        if self.surfaces > 16 {
            return Err(E_SCENARIO);
        }
        Ok(())
    }
    /// 确定性成本模型：单次执行的周期数。
    /// cycles = frames*(FRAME_CYCLES + surfaces*(rows*ROW_CYCLES + SUBMIT_CYCLES))
    pub fn cost_once(&self) -> u64 {
        let per_surf = self.rows_per_surf as u64 * ROW_CYCLES + SUBMIT_CYCLES;
        self.frames as u64 * (FRAME_CYCLES + self.surfaces as u64 * per_surf)
    }
}

/// 单场景基准结果。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BenchResult {
    /// 场景名。
    pub name: &'static str,
    /// 中位周期数。
    pub median: u64,
    /// 最小周期数。
    pub min: u64,
    /// 最大周期数。
    pub max: u64,
    /// 预算千分比占用（median*1000/budget）。
    pub budget_pm: u64,
}

/// 插入排序（确定性，n ≤ SAMPLES）。
fn sort_u64(v: &mut [u64]) {
    for i in 1..v.len() {
        let mut j = i;
        while j > 0 && v[j - 1] > v[j] {
            v.swap(j - 1, j);
            j -= 1;
        }
    }
}

/// 跑一个场景 SAMPLES 次，取中位/最小/最大。
pub fn run_bench(sc: &Scenario) -> Result<BenchResult, u16> {
    sc.validate()?;
    let mut xs = Vec::with_capacity(SAMPLES);
    for _ in 0..SAMPLES {
        xs.push(sc.cost_once());
    }
    sort_u64(&mut xs);
    let median = xs[SAMPLES / 2];
    let min = xs[0];
    let max = xs[SAMPLES - 1];
    let budget_pm = if sc.budget == 0 {
        u64::MAX
    } else {
        median * 1000 / sc.budget
    };
    Ok(BenchResult {
        name: sc.name,
        median,
        min,
        max,
        budget_pm,
    })
}

/// 回归三态（闭集）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Trend {
    /// 变快（超 REGRESS_PM 千分比的负向差）。
    Better,
    /// 持平。
    Flat,
    /// 变慢。
    Worse,
}

/// 对基线比较：delta_pm = (cur - base)*1000/base（有符号语义用差值方向表达）。
pub fn compare(base: u64, cur: u64) -> (Trend, u64) {
    if cur < base {
        let pm = (base - cur) * 1000 / base.max(1);
        if pm > REGRESS_PM {
            (Trend::Better, pm)
        } else {
            (Trend::Flat, pm)
        }
    } else if cur > base {
        let pm = (cur - base) * 1000 / base.max(1);
        if pm > REGRESS_PM {
            (Trend::Worse, pm)
        } else {
            (Trend::Flat, pm)
        }
    } else {
        (Trend::Flat, 0)
    }
}

/// 回归门：任一场景 Worse 即拒绝。
pub fn regress_gate(base: &[BenchResult], cur: &[BenchResult]) -> Result<(), u16> {
    if base.len() != cur.len() {
        return Err(E_BASELINE);
    }
    for (b, c) in base.iter().zip(cur.iter()) {
        if b.name != c.name {
            return Err(E_BASELINE);
        }
        let (t, _) = compare(b.median, c.median);
        if t == Trend::Worse {
            return Err(E_REGRESS);
        }
    }
    Ok(())
}

/// 预算门：全部场景 budget_pm ≤ 1000 才过。
pub fn budget_gate(rs: &[BenchResult]) -> Result<(), u16> {
    for r in rs {
        if r.budget_pm > 1000 {
            return Err(E_BUDGET);
        }
    }
    Ok(())
}

/// 报告行（导出格式：名|中位|预算千分比）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReportRow {
    /// 场景名。
    pub name: &'static str,
    /// 中位周期。
    pub median: u64,
    /// 预算占用千分比。
    pub pm: u64,
}

/// 生成报告（按 median 升序排）。
pub fn report(rs: &[BenchResult]) -> Vec<ReportRow> {
    let mut rows: Vec<ReportRow> = rs
        .iter()
        .map(|r| ReportRow {
            name: r.name,
            median: r.median,
            pm: r.budget_pm,
        })
        .collect();
    for i in 1..rows.len() {
        let mut j = i;
        while j > 0 && rows[j - 1].median > rows[j].median {
            rows.swap(j - 1, j);
            j -= 1;
        }
    }
    rows
}

/// 自例程：主路径（三场景→基准→预算门→回归门）+ 错误路径。
pub fn drill() -> Result<u32, u16> {
    let s1 = Scenario {
        name: "light_scroll",
        surfaces: 2,
        rows_per_surf: 16,
        frames: 60,
        budget: 8_000_000,
    };
    let s2 = Scenario {
        name: "heavy_scroll",
        surfaces: 16,
        rows_per_surf: 64,
        frames: 60,
        budget: 60_000_000,
    };
    let s3 = Scenario {
        name: "tiny_scroll",
        surfaces: 1,
        rows_per_surf: 4,
        frames: 30,
        budget: 2_000_000,
    };
    // 主路径。
    let r1 = run_bench(&s1)?;
    let r2 = run_bench(&s2)?;
    let r3 = run_bench(&s3)?;
    // 手算期望：s1 = 60*(512 + 2*(16*64+48)) = 60*(512+2*1072) = 60*2656 = 159360。
    if r1.median != 159_360 || r1.min != 159_360 || r1.max != 159_360 {
        return Err(E_SCENARIO);
    }
    if r1.budget_pm != 159_360 * 1000 / 8_000_000 {
        return Err(E_BUDGET);
    }
    // s2 = 60*(512 + 16*(64*64+48)) = 60*(512+16*4144) = 60*66816 = 4_008_960。
    if r2.median != 4_008_960 {
        return Err(E_SCENARIO);
    }
    // s3 = 30*(512 + 1*(4*64+48)) = 30*(512+304) = 30*816 = 24480。
    if r3.median != 24_480 {
        return Err(E_SCENARIO);
    }
    budget_gate(&[r1.clone(), r2.clone(), r3.clone()])?;
    // 回归门：同场景集自身比较应平。
    regress_gate(&[r1.clone(), r2.clone(), r3.clone()], &[r1.clone(), r2.clone(), r3.clone()])?;
    // compare 三态：
    let (t0, _) = compare(1000, 1000);
    if t0 != Trend::Flat {
        return Err(E_REGRESS);
    }
    let (t_up, _) = compare(1000, 1600); // +600‰ → Worse
    if t_up != Trend::Worse {
        return Err(E_REGRESS);
    }
    let (t_dn, _) = compare(1000, 400); // -600‰ → Better
    if t_dn != Trend::Better {
        return Err(E_REGRESS);
    }
    // 错误路径 1：0 表面场景。
    let bad = Scenario {
        name: "bad",
        surfaces: 0,
        rows_per_surf: 8,
        frames: 10,
        budget: 1000,
    };
    if run_bench(&bad) != Err(E_SCENARIO) {
        return Err(E_SCENARIO);
    }
    // 错误路径 2：超 16 表面。
    let bad2 = Scenario {
        name: "bad2",
        surfaces: 17,
        rows_per_surf: 8,
        frames: 10,
        budget: 1000,
    };
    if run_bench(&bad2) != Err(E_SCENARIO) {
        return Err(E_SCENARIO);
    }
    // 错误路径 3：预算超支（预算压到 100000 < 159360）。
    let tight = Scenario {
        name: "tight",
        surfaces: 2,
        rows_per_surf: 16,
        frames: 60,
        budget: 100_000,
    };
    let rt = run_bench(&tight)?;
    if budget_gate(&[rt]) != Err(E_BUDGET) {
        return Err(E_BUDGET);
    }
    // 错误路径 4：回归（cur 比 base 慢 50‰+）。
    let slower = BenchResult {
        name: "light_scroll",
        median: r1.median * 2,
        min: 0,
        max: 0,
        budget_pm: 0,
    };
    if regress_gate(&[r1.clone()], &[slower]) != Err(E_REGRESS) {
        return Err(E_REGRESS);
    }
    // 错误路径 5：基线名不匹配。
    let renamed = BenchResult {
        name: "renamed",
        median: r1.median,
        min: 0,
        max: 0,
        budget_pm: 0,
    };
    if regress_gate(&[r1.clone()], &[renamed]) != Err(E_BASELINE) {
        return Err(E_BASELINE);
    }
    // 报告排序：三行按 median 升序。
    let rows = report(&[r2, r3, r1]);
    if rows.len() != 3 || !(rows[0].median <= rows[1].median && rows[1].median <= rows[2].median) {
        return Err(E_SCENARIO);
    }
    Ok(0)
}
