//! CGPU-F0735 · 模拟与真实校准（目标 390 行）
//! ==========================================================================
//! 锚点判据（清单原文摘录）：
//! - 纯功能代码 ≥320 行；② 规格与判据按册内锚点原文逐条落实
//!   （CGPU Varix STAR II · 总纲与施工书#CGPU-F0735，册内文档不在本仓，
//!    以清单判据为施工依据）；③ 完成时 API/软件写回：状态=已完成。
//! - 主题：模拟与真实校准——模拟器预测值与实测值成对采样，
//!    固点比例校正（ppm 口径）、滑动窗漂移跟踪、校准质量门
//!    与校正后预测。
//!
//! 设计纪律：
//! - 校正因子：k = real*1e6/sim（ppm，整除，sim=0 拒绝）；
//  应用：pred = sim*k/1e6（溢出安全 u128 中转）。
// - 样本账定容：CAL_CAP=64 对，环形覆盖计数显式；
//   守恒 total == held + overwritten。
// - 漂移三态闭集：Stable(窗内 k 波动 ≤ DRIFT_PCT)/Drifting/
//   Diverged（sim 与 real 方向相反即发散）。
// - 质量门：最近窗内 |real-pred|*1000/real ≤ ERR_TOL_PM=30
//   （千分比）对全部样本成立。
// - 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，
//   确定性。
// - 诊断段 0x4a20 本单独占，错误码 SEG_BASE+1..+N 闭集。

use alloc::vec;
use alloc::vec::Vec;

/// 诊断段基址（0x4a20，批内 CGPU-F0735 独占）。
pub const SEG_BASE: u16 = 0x4a20;

/// 模拟值为 0（比例不可定义）。
pub const E_ZERO_SIM: u16 = SEG_BASE + 1;
/// 实测值为 0（误差千分比不可定义）。
pub const E_ZERO_REAL: u16 = SEG_BASE + 2;
/// 校准账已满（定容且覆盖关闭的口径下）。
pub const E_FULL: u16 = SEG_BASE + 3;
/// 样本账为空。
pub const E_EMPTY: u16 = SEG_BASE + 4;
/// 质量门未过。
pub const E_QUALITY: u16 = SEG_BASE + 5;
/// 账目守恒被击穿。
pub const E_LEDGER: u16 = SEG_BASE + 6;

/// 样本账容量。
pub const CAL_CAP: usize = 64;
/// 误差容差（千分比）。
pub const ERR_TOL_PM: u64 = 30;
/// 漂移判定百分比阈值（k 波动 ≤ 10% 视为稳定）。
pub const DRIFT_PCT: u64 = 10;

/// 成对样本。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CalPair {
    /// 模拟预测值。
    pub sim: u64,
    /// 实测值。
    pub real: u64,
}

/// 单对校正因子（ppm）。
pub fn factor_ppm(p: &CalPair) -> Result<u64, u16> {
    if p.sim == 0 {
        return Err(E_ZERO_SIM);
    }
    Ok(p.real * 1_000_000 / p.sim)
}

/// 应用校正因子：pred = sim*k/1e6（u128 中转防溢出）。
pub fn apply_factor(sim: u64, k_ppm: u64) -> u64 {
    ((sim as u128 * k_ppm as u128) / 1_000_000u128) as u64
}

/// 单对误差（千分比）：|real-pred|*1000/real（real=0 拒绝）。
pub fn err_pm(p: &CalPair, k_ppm: u64) -> Result<u64, u16> {
    if p.real == 0 {
        return Err(E_ZERO_REAL);
    }
    let pred = apply_factor(p.sim, k_ppm);
    let d = if p.real > pred { p.real - pred } else { pred - p.real };
    Ok(d * 1000 / p.real)
}

/// 漂移三态（闭集）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Drift {
    /// 稳定。
    Stable,
    /// 漂移中。
    Drifting,
    /// 发散（方向相反）。
    Diverged,
}

/// 校准器：样本环形账 + 全局因子。
#[derive(Clone, Debug)]
pub struct Calibrator {
    ring: [Option<CalPair>; CAL_CAP],
    w: usize,
    held: usize,
    overwritten: u64,
    total: u64,
    /// 全局校正因子（ppm，最近一次定标）。
    k_ppm: u64,
}

impl Calibrator {
    pub fn new() -> Self {
        Self {
            ring: core::array::from_fn(|_| None),
            w: 0,
            held: 0,
            overwritten: 0,
            total: 0,
            k_ppm: 1_000_000, // 默认 1:1
        }
    }
    /// 采一对样本（定标前校验 sim/real 非 0）。
    pub fn sample(&mut self, p: CalPair) -> Result<u64, u16> {
        if p.sim == 0 {
            return Err(E_ZERO_SIM);
        }
        if p.real == 0 {
            return Err(E_ZERO_REAL);
        }
        let k = factor_ppm(&p)?;
        self.ring[self.w] = Some(p);
        self.w = (self.w + 1) % CAL_CAP;
        if self.held < CAL_CAP {
            self.held += 1;
        } else {
            self.overwritten += 1;
        }
        self.total += 1;
        self.k_ppm = k;
        Ok(k)
    }
    /// 守恒：total == held + overwritten。
    pub fn audit(&self) -> Result<(), u16> {
        if self.total != self.held as u64 + self.overwritten {
            return Err(E_LEDGER);
        }
        Ok(())
    }
    /// 视图（旧→新）。
    pub fn view(&self) -> Vec<CalPair> {
        let start = if self.held < CAL_CAP { 0 } else { self.w };
        let mut out = Vec::with_capacity(self.held);
        for i in 0..self.held {
            let idx = (start + i) % CAL_CAP;
            if let Some(p) = self.ring[idx] {
                out.push(p);
            }
        }
        out
    }
    /// 全局因子。
    pub fn factor(&self) -> u64 {
        self.k_ppm
    }
    /// 漂移判定：对窗内相邻样本 k 序列，max/min 比（百分比口径）：
    ///   有反向样本（real-sim 符号相反）→ Diverged；
    ///   max*100/min ≤ 100+DRIFT_PCT → Stable；
    ///   否则 Drifting。窗内样本 <2 → Stable（无信息）。
    pub fn drift(&self) -> Result<Drift, u16> {
        let v = self.view();
        if v.len() < 2 {
            return Ok(Drift::Stable);
        }
        let mut ks: Vec<u64> = Vec::with_capacity(v.len());
        for p in &v {
            ks.push(factor_ppm(p)?);
        }
        // 方向发散检测：real>sim 与 real<sim 混合。
        let up = v.iter().filter(|p| p.real > p.sim).count();
        let down = v.iter().filter(|p| p.real < p.sim).count();
        if up > 0 && down > 0 {
            return Ok(Drift::Diverged);
        }
        let mut mn = ks[0];
        let mut mx = ks[0];
        for k in &ks {
            if *k < mn {
                mn = *k;
            }
            if *k > mx {
                mx = *k;
            }
        }
        if mx * 100 <= mn * (100 + DRIFT_PCT) {
            Ok(Drift::Stable)
        } else {
            Ok(Drift::Drifting)
        }
    }
    /// 质量门：用当前因子对窗内全部样本复算误差，全 ≤ 容差才过。
    pub fn quality_gate(&self) -> Result<(), u16> {
        let v = self.view();
        if v.is_empty() {
            return Err(E_EMPTY);
        }
        for p in &v {
            if err_pm(p, self.k_ppm)? > ERR_TOL_PM {
                return Err(E_QUALITY);
            }
        }
        Ok(())
    }
    /// 校正后预测。
    pub fn predict(&self, sim: u64) -> u64 {
        apply_factor(sim, self.k_ppm)
    }
    /// 样本数。
    pub fn len(&self) -> usize {
        self.held
    }
}

/// 校准报告。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CalReport {
    /// 样本数。
    pub samples: u64,
    /// 覆盖数。
    pub overwritten: u64,
    /// 当前因子（ppm）。
    pub k_ppm: u64,
    /// 漂移态。
    pub drift: Drift,
    /// 质量门通过。
    pub quality_ok: bool,
}

/// 报告化。
pub fn report(c: &Calibrator) -> Result<CalReport, u16> {
    Ok(CalReport {
        samples: c.total,
        overwritten: c.overwritten,
        k_ppm: c.k_ppm,
        drift: c.drift()?,
        quality_ok: c.quality_gate().is_ok(),
    })
}

/// 自例程：主路径（等比样本→稳定→质量门→覆盖回绕）+ 错误路径。
pub fn drill() -> Result<u32, u16> {
    let mut c = Calibrator::new();
    // 错误路径 0：sim=0。
    if c.sample(CalPair { sim: 0, real: 10 }) != Err(E_ZERO_SIM) {
        return Err(E_ZERO_SIM);
    }
    // 错误路径 1：real=0。
    if c.sample(CalPair { sim: 10, real: 0 }) != Err(E_ZERO_REAL) {
        return Err(E_ZERO_REAL);
    }
    // 主路径：真实慢 20% 的稳定系统（real = sim*1.2），
    // 样本 sim=1000,2000,...,64000 → k=1200000 恒定。
    for i in 1..=8u64 {
        let sim = i * 1000;
        let real = sim * 12 / 10;
        let k = c.sample(CalPair { sim, real })?;
        if k != 1_200_000 {
            return Err(E_ZERO_SIM);
        }
    }
    c.audit()?;
    if c.len() != 8 || c.factor() != 1_200_000 {
        return Err(E_LEDGER);
    }
    // 漂移：等比 → Stable。
    if c.drift()? != Drift::Stable {
        return Err(E_ZERO_SIM);
    }
    // 质量门：k=1.2 时 pred=real 精确 → 误差 0 → 过。
    c.quality_gate()?;
    // 预测：sim=100000 → 120000。
    if c.predict(100_000) != 120_000 {
        return Err(E_ZERO_SIM);
    }
    // 主路径 2：漂移系统（real 比例从 1.1 逐样本升到 1.5）→ Drifting。
    let mut c2 = Calibrator::new();
    // real/sim: 1.10, 1.15, 1.20, 1.25（同向增）→ k 波动 (1.25/1.10)*100=113 > 110。
    let ratios = [11u64, 115, 120, 125]; // 千分比 *100？直接用 sim=10000, real=sim*r/10
    for r in ratios {
        let sim = 10_000u64;
        let real = sim * r / 10;
        c2.sample(CalPair { sim, real })?;
    }
    c2.audit()?;
    if c2.drift()? != Drift::Drifting {
        return Err(E_ZERO_SIM);
    }
    // 错误路径 2：发散系统（方向混合）。
    let mut c3 = Calibrator::new();
    c3.sample(CalPair { sim: 1000, real: 1100 })?; // real>sim
    c3.sample(CalPair { sim: 1000, real: 900 })?; // real<sim
    if c3.drift()? != Drift::Diverged {
        return Err(E_ZERO_SIM);
    }
    // 错误路径 3：质量门未过（k 取自最近样本，但早期样本误差大）。
    let mut c4 = Calibrator::new();
    c4.sample(CalPair { sim: 1000, real: 1100 })?; // k=1.1
    c4.sample(CalPair { sim: 10000, real: 20000 })?; // k=2.0（最近）
    // 用 k=2.0 复算第一样本：pred=2000，误差 = 900*1000/1100 = 818 > 30。
    if c4.quality_gate() != Err(E_QUALITY) {
        return Err(E_QUALITY);
    }
    // 错误路径 4：空账质量门。
    let c0 = Calibrator::new();
    if c0.quality_gate() != Err(E_EMPTY) {
        return Err(E_EMPTY);
    }
    // 主路径 5：覆盖回绕（灌 CAL_CAP+8 条 → overwritten=8）。
    // 用偶数 sim 保证 real=sim*15/10 精确（k 恒 1.5e6）。
    let mut c5 = Calibrator::new();
    for i in 0..(CAL_CAP as u64 + 8) {
        let sim = 2000 + i * 2;
        c5.sample(CalPair { sim, real: sim * 15 / 10 })?;
    }
    c5.audit()?;
    let rep = report(&c5)?;
    if rep.overwritten != 8 || rep.samples != (CAL_CAP as u64 + 8) {
        return Err(E_LEDGER);
    }
    if rep.k_ppm != 1_500_000 || rep.drift != Drift::Stable || !rep.quality_ok {
        return Err(E_ZERO_SIM);
    }
    Ok(0)
}
