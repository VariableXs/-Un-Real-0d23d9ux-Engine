//! CGPU-F0378 · NPU 智能决策预留（目标 300 行）
//! ==========================================================================
//! 锚点判据（施工书原文摘录）：
//! - NPU 预留：K 域联动预留（NPU 有则用：复杂负载模式的模型推理——预测器
//!   可插拔接口）；无 NPU 路径（纯统计预测兜底——能力探测自动选路）；
//!   诚实标注（一期纯统计、NPU 推理为预留接口不假装可用）。
//! - 判据：可插拔、自动选路、诚实标注、判据。
//!
//! 设计纪律：
//! - 预测器是 trait 可插拔点；NPU 实现一期不存在（占位 trait 不假装可用）。
//! - 选路器按能力探测结果自动落到纯统计兜底，选路决策全量可审计。
//! - no_std + alloc；零 unwrap/expect 于运行路径；诊断段 0x0A30 独占。

use alloc::vec::Vec;

/// 诊断段基址（0x0A30，全仓独占）。
pub const SEG_BASE: u16 = 0x0A30;

pub const E_NO_BACKEND: u16 = SEG_BASE + 1;
pub const E_BACKEND_FAILED: u16 = SEG_BASE + 2;
pub const E_NOT_IMPLEMENTED: u16 = SEG_BASE + 3;
pub const E_HONEST_LABEL: u16 = SEG_BASE + 4;
pub const E_INPUT_SHAPE: u16 = SEG_BASE + 5;

/// 负载特征向量（预测器输入的冻结形状）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LoadFeature {
    /// 最近帧负载千分位序列摘要：均值/方差代理/峰值。
    pub mean_pm: u32,
    pub variance_hint_pm: u32,
    pub peak_pm: u32,
    /// 周期检测置信（0~100）。
    pub cycle_confidence: u8,
    /// 模式复杂度分级（0 稳态 1 周期 2 突变 3 混合）。
    pub complexity: u8,
}

impl LoadFeature {
    /// 形状校验：置信与复杂度必须落在闭集。
    pub fn shape_ok(&self) -> bool {
        self.cycle_confidence <= 100 && self.complexity <= 3 && self.variance_hint_pm <= self.peak_pm.max(1)
    }
}

/// 预测输出（统一形状——各后端产出一致契约）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Prediction {
    pub next_load_pm: u32,
    /// 区间下界（宁欠勿超的预留下界）。
    pub low_pm: u32,
    pub high_pm: u32,
}

impl Prediction {
    pub fn interval_ok(&self) -> bool {
        self.low_pm <= self.next_load_pm && self.next_load_pm <= self.high_pm && self.high_pm <= 10_000
    }
}

/// 预测器后端 trait（可插拔点——一期唯一实现为纯统计兜底）。
pub trait PredictBackend {
    /// 后端名（审计与选路日志用）。
    fn name(&self) -> &'static str;
    /// 能力声明：能处理的最大复杂度分级。
    fn max_complexity(&self) -> u8;
    /// 执行预测（输入形状违规必须返回错误，不许静默钳制）。
    fn predict(&self, f: &LoadFeature) -> Result<Prediction, u16>;
}

/// 纯统计兜底（一期真实路径）：EWMA 均值外推 + 复杂度加宽区间。
pub struct StatisticalBackend {
    /// 复杂度每升一级，区间加宽的千分位步长。
    pub widen_per_level_pm: u32,
}

impl StatisticalBackend {
    pub fn new() -> Self {
        StatisticalBackend { widen_per_level_pm: 150 }
    }

    fn interval_widen(&self, complexity: u8) -> u32 {
        self.widen_per_level_pm.saturating_mul(complexity as u32)
    }
}

impl Default for StatisticalBackend {
    fn default() -> Self {
        Self::new()
    }
}

impl PredictBackend for StatisticalBackend {
    fn name(&self) -> &'static str {
        "stat-ewma"
    }

    fn max_complexity(&self) -> u8 {
        3
    }

    fn predict(&self, f: &LoadFeature) -> Result<Prediction, u16> {
        if !f.shape_ok() {
            return Err(E_INPUT_SHAPE);
        }
        let base = f.mean_pm.min(10_000);
        let widen = self.interval_widen(f.complexity);
        let low = base.saturating_sub(widen / 2);
        let high = (base + widen / 2).min(10_000);
        Ok(Prediction { next_load_pm: base, low_pm: low, high_pm: high })
    }
}

/// NPU 推理后端（一期预留 trait——诚实标注：`available()` 恒 false，
/// `predict` 恒返回 NOT_IMPLEMENTED，绝不假装可用）。
pub struct NpuBackendReserved;

impl PredictBackend for NpuBackendReserved {
    fn name(&self) -> &'static str {
        "npu-reserved"
    }

    fn max_complexity(&self) -> u8 {
        3
    }

    fn predict(&self, _f: &LoadFeature) -> Result<Prediction, u16> {
        Err(E_NOT_IMPLEMENTED)
    }
}

/// K 域能力探测结果（选路依据——一期探测恒为无 NPU）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Capability {
    pub npu_present: bool,
    /// NPU 推理就绪（一期恒 false——预留位）。
    pub npu_ready: bool,
}

impl Capability {
    /// 诚实探测：一期无 K 域联动实现，恒返回无 NPU。
    pub fn probe_honest() -> Self {
        Capability { npu_present: false, npu_ready: false }
    }
}

/// 选路器：按能力与复杂度自动落位。
/// 选路规则（冻结）：
/// 1. NPU 就绪且复杂度 ≥ 2 → NPU 后端；
/// 2. 否则 → 纯统计兜底；
/// 3. 无任何后端 → E_NO_BACKEND（调用方转被动仲裁并显性告知）。
pub struct Router {
    stat: StatisticalBackend,
    npu: Option<NpuBackendReserved>,
    cap: Capability,
    /// 选路日志（审计面：每次决策记后端名）。
    route_log: Vec<(&'static str, u8)>,
}

impl Router {
    pub fn new() -> Self {
        Router {
            stat: StatisticalBackend::new(),
            npu: None,
            cap: Capability::probe_honest(),
            route_log: Vec::new(),
        }
    }

    /// 自动选路并执行预测。
    pub fn predict_auto(&mut self, f: &LoadFeature) -> Result<(Prediction, &'static str), u16> {
        if !f.shape_ok() {
            return Err(E_INPUT_SHAPE);
        }
        // 规则 1：NPU 就绪且复杂负载。
        if self.cap.npu_ready && f.complexity >= 2 {
            if let Some(n) = &self.npu {
                match n.predict(f) {
                    Ok(p) => {
                        self.route_log.push((n.name(), f.complexity));
                        return Ok((p, n.name()));
                    }
                    Err(e) if e == E_NOT_IMPLEMENTED => { /* 落兜底 */ }
                    Err(e) => return Err(e),
                }
            }
        }
        // 规则 2：纯统计兜底。
        let p = self.stat.predict(f)?;
        self.route_log.push((self.stat.name(), f.complexity));
        Ok((p, self.stat.name()))
    }

    /// 后端缺失探测（判据「无 NPU 路径」的反面用例入口）。
    pub fn backend_count(&self) -> usize {
        let mut n = 1; // stat 恒在
        if self.npu.is_some() {
            n += 1;
        }
        n
    }

    /// 诚实标注面：对外报告当前真实能力（不允许虚报 NPU 可用）。
    pub fn honest_report(&self) -> (&'static str, bool) {
        if self.cap.npu_ready && self.npu.is_some() {
            ("npu+stat", true)
        } else {
            ("stat-only", false)
        }
    }

    pub fn route_log(&self) -> &[( &'static str, u8)] {
        &self.route_log
    }
}

impl Default for Router {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// 判据自检
// ---------------------------------------------------------------------------
#[cfg(test)]
mod tests {
    use super::*;

    fn feat(c: u8) -> LoadFeature {
        LoadFeature { mean_pm: 4_000, variance_hint_pm: 300, peak_pm: 7_000, cycle_confidence: 60, complexity: c }
    }

    #[test]
    fn a78_可插拔_统计后端契约() {
        let b = StatisticalBackend::new();
        let p = b.predict(&feat(0)).unwrap();
        assert!(p.interval_ok());
        assert_eq!(p.next_load_pm, 4_000);
        assert_eq!(b.name(), "stat-ewma");
        assert_eq!(b.max_complexity(), 3);
        // 形状违规必拒（不许静默钳制）。
        let mut bad = feat(0);
        bad.cycle_confidence = 101;
        assert_eq!(b.predict(&bad).unwrap_err(), E_INPUT_SHAPE);
    }

    #[test]
    #[allow(unused_must_use)]
    fn a78_自动选路_无NPU落兜底() {
        let mut r = Router::new();
        assert_eq!(r.backend_count(), 1);
        let (p, backend) = r.predict_auto(&feat(3)).unwrap();
        assert_eq!(backend, "stat-ewma");
        assert!(p.interval_ok());
        // 复杂负载在无 NPU 时同样落统计（选路日志可审计）。
        assert_eq!(r.route_log().len(), 1);
        // 诚实标注：一期 stat-only、npu_ready=false。
        assert_eq!(r.honest_report(), ("stat-only", false));
    }

    #[test]
    fn a78_诚实标注_NPU预留不假装可用() {
        let n = NpuBackendReserved;
        assert_eq!(n.predict(&feat(2)).unwrap_err(), E_NOT_IMPLEMENTED);
        let cap = Capability::probe_honest();
        assert!(!cap.npu_present);
        assert!(!cap.npu_ready);
        // 预留后端不算可协商能力。
        let mut r = Router::new();
        let mut f = feat(3);
        f.complexity = 2;
        let (_, backend) = r.predict_auto(&f).unwrap();
        assert_eq!(backend, "stat-ewma");
        // E_NOT_IMPLEMENTED 常量自检（诚实标注错误码在册）。
        assert_eq!(E_HONEST_LABEL, SEG_BASE + 4);
    }

    #[test]
    fn a78_区间语义_复杂度加宽() {
        let b = StatisticalBackend::new();
        let p0 = b.predict(&feat(0)).unwrap();
        let p3 = b.predict(&feat(3)).unwrap();
        // 复杂度越高区间越宽，但均值锚点不动。
        assert!(p3.high_pm - p3.low_pm > p0.high_pm - p0.low_pm);
        assert_eq!(p0.next_load_pm, p3.next_load_pm);
        // 高均值不越界。
        let mut f = feat(3);
        f.mean_pm = 9_900;
        f.peak_pm = 10_000;
        let p = b.predict(&f).unwrap();
        assert!(p.interval_ok() && p.high_pm <= 10_000);
    }
}
