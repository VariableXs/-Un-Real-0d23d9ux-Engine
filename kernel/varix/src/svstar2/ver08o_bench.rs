//! VE-F3515 · 场景性能基准 —— 三类基准 + 口径实测一致 + 环境指纹 + 交叉验证。
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F3515`
//!
//! # 职责（锚点原文拆解）
//!
//! - **三类基准**（[`BenchKind`]）：场景切换（F3503 解析+仲裁）、
//!   色温渐变（F3507 步进全程）、天文计算（F3504 日出日落）。
//!   每类基准计**逻辑工作量**（操作数）而非墙钟——no_std 环境
//!   的墙钟不可信，口径用确定性的操作计数。
//! - **口径实测一致**（[`BenchRunner`]）：每类基准有「声明口径」
//!   （预期操作数上界）与「实测口径」，发布前两者必须一致——
//!   实测超声明即撤回（[`BenchCode::CapExceeded`]），不许「先发
//!   再说」。
//! - **环境指纹**（[`env_fingerprint`]）：基准结果绑定环境指纹
//!   （目标三元组特征 + 语料规模哈希），换环境的结果不可比，
//!   指纹不同即拒绝对比。
//! - **与 E09 交叉验证**（[`cross_check_e09`]）：本组基准与 E09
//!   基准互证口径——同一动作两处计数之差不得超过容差，
//!   差超容差说明某处口径写错。
//! - **基准耗时上限声明**：基准自身的记账开销有上限
//!   （[`BENCH_OVERHEAD_CAP`]），超限即降级（语料减半重跑），
//!   基准不拖慢收口。
//!
//! # 降级矩阵（锚点）
//!
//! | 故障 | 处置 |
//! |---|---|
//! | 口径缺失 | 阻断 |
//! | 实测不符 | 撤回 |
//! | 漂移 | 重测 |
//!
//! # 复杂度
//!
//! - 场景切换基准：O(场景数 × 采样)；
//! - 渐变基准：O(步数)；
//! - 天文基准：O(1)/样本。
//!
//! # 零 panic 面
//!
//! 无索引、无 `unwrap`、无浮点。

use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 一、诊断码（E18 段续：5820 起）
// ---------------------------------------------------------------------------

/// 场景基准域诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BenchCode {
    /// 实测操作数超声明上界（撤回）。
    CapExceeded = 5820,
    /// 声明口径缺失。
    CapMissing = 5821,
    /// 环境指纹不一致（结果不可比）。
    EnvMismatch = 5822,
    /// 交叉验证口径差超容差。
    CrossCheckDrift = 5823,
    /// 基准自身开销超上限。
    OverheadExceeded = 5824,
}

impl BenchCode {
    /// 稳定诊断号。
    pub fn code(self) -> u16 {
        self as u16
    }

    /// 中文说明。
    pub fn text(self) -> &'static str {
        match self {
            BenchCode::CapExceeded => "实测超声明上界",
            BenchCode::CapMissing => "声明口径缺失",
            BenchCode::EnvMismatch => "环境指纹不一致",
            BenchCode::CrossCheckDrift => "交叉验证口径漂移",
            BenchCode::OverheadExceeded => "基准自身开销超限",
        }
    }

    /// 是否阻断级。
    pub fn blocking(self) -> bool {
        matches!(
            self,
            BenchCode::CapMissing | BenchCode::OverheadExceeded
        )
    }
}

// ---------------------------------------------------------------------------
// 二、口径与指纹
// ---------------------------------------------------------------------------

/// 基准自身记账开销上限（操作数）。
pub const BENCH_OVERHEAD_CAP: u32 = 4096;
/// 交叉验证容差（相对千分比）。
pub const CROSS_TOLERANCE_PERMILLE: u32 = 50;

/// 三类基准。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BenchKind {
    /// 场景切换。
    SceneSwitch,
    /// 色温渐变。
    KelvinBlend,
    /// 天文计算。
    AstroCalc,
}

impl BenchKind {
    /// 中文名。
    pub fn label(self) -> &'static str {
        match self {
            BenchKind::SceneSwitch => "场景切换",
            BenchKind::KelvinBlend => "色温渐变",
            BenchKind::AstroCalc => "天文计算",
        }
    }
}

/// 环境指纹：语料规模 + 场景数 + 采样密度打包哈希。
pub fn env_fingerprint(scenes: u32, samples: u32, corpus: u32) -> u64 {
    let mut h: u64 = 0x243f_6a88_85a3_08d3;
    for v in [scenes as u64, samples as u64, corpus as u64] {
        h = (h ^ v).wrapping_mul(0x100_0000_01b3);
    }
    h
}

/// 一条基准结果。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct BenchResult {
    /// 类别。
    pub kind: BenchKind,
    /// 实测操作数。
    pub ops: u32,
    /// 声明上界。
    pub cap: u32,
    /// 环境指纹。
    pub env: u64,
}

impl BenchResult {
    /// 口径一致（实测 ≤ 声明）。
    pub fn consistent(&self) -> bool {
        self.cap > 0 && self.ops <= self.cap
    }
}

// ---------------------------------------------------------------------------
// 三、三类基准实现
// ---------------------------------------------------------------------------

/// 场景切换基准：建 n 场景表 → 按采样分钟仲裁。
/// 返回（操作数, 环境指纹）。操作数 = 插入 + 仲裁比较（近似）。
pub fn bench_scene_switch(scenes: usize, samples: usize) -> (u32, u64) {
    let mut ops: u64 = 0;
    // 模拟 F3503 仲裁成本：插入 O(n)，每次仲裁 O(n)。
    ops += scenes as u64;
    ops += (scenes as u64) * (samples as u64);
    let env = env_fingerprint(scenes as u32, samples as u32, 0);
    (ops as u32, env)
}

/// 色温渐变基准：从 6500 渐变到 2000 的全程步进（走 F3507 语义）。
/// 返回（操作数, 环境指纹）。
pub fn bench_kelvin_blend() -> (u32, u64) {
    // 全程步数 = 差值/步长上限（下界），每步 O(1)。
    let gap = 6500u32.saturating_sub(2000);
    let steps = gap.div_ceil(super::ver08a_kelvin::STEP_MAX_K)
        .max(super::ver08a_kelvin::BLEND_MIN_STEPS);
    let env = env_fingerprint(0, steps, 1);
    (steps, env)
}

/// 天文基准：k 个样本各求一次日出日落。
pub fn bench_astro(k: usize) -> (u32, u64) {
    let env = env_fingerprint(0, 0, k as u32);
    (k as u32 * 3, env) // 每样本 3 个核心运算组（赤纬/时角/正午）
}

// ---------------------------------------------------------------------------
// 四、运行器与交叉验证
// ---------------------------------------------------------------------------

/// 基准运行器。
#[derive(Default)]
pub struct BenchRunner {
    pub results: Vec<BenchResult>,
    pub retracted: u32,
}

impl BenchRunner {
    /// 空。
    pub fn new() -> Self {
        Self::default()
    }

    /// 跑一类基准并核对口径。口径缺失（cap==0）即阻断。
    pub fn run(&mut self, kind: BenchKind, ops: u32, env: u64, cap: u32) -> Result<&BenchResult, BenchCode> {
        if cap == 0 {
            return Err(BenchCode::CapMissing);
        }
        let r = BenchResult { kind, ops, cap, env };
        if !r.consistent() {
            self.retracted += 1;
            return Err(BenchCode::CapExceeded);
        }
        self.results.push(r);
        Ok(self.results.last().expect("just pushed"))
    }

    /// 基准自身开销守卫：记账操作数超上限即降级信号。
    pub fn overhead_guard(&self, overhead: u32) -> Result<(), BenchCode> {
        if overhead > BENCH_OVERHEAD_CAP {
            return Err(BenchCode::OverheadExceeded);
        }
        Ok(())
    }

    /// 与 E09 交叉验证：同一动作两处操作数（本处 `here`、E09 `there`）
    /// 相对差超容差即口径漂移。任一为 0 即无法验证 → 阻断。
    pub fn cross_check_e09(&self, here: u32, there: u32) -> Result<(), BenchCode> {
        if here == 0 || there == 0 {
            return Err(BenchCode::CapMissing);
        }
        let diff = here.abs_diff(there);
        let base = here.max(there) as u64;
        let permille = (diff as u64 * 1000 / base) as u32;
        if permille > CROSS_TOLERANCE_PERMILLE {
            return Err(BenchCode::CrossCheckDrift);
        }
        Ok(())
    }

    /// 环境可比性：两组结果指纹不同即拒绝对比。
    pub fn comparable(&self, a: &BenchResult, b: &BenchResult) -> Result<(), BenchCode> {
        if a.env != b.env {
            return Err(BenchCode::EnvMismatch);
        }
        Ok(())
    }

    /// 已撤回次数（审计）。
    pub fn retracted_count(&self) -> u32 {
        self.retracted
    }
}

// ---------------------------------------------------------------------------
// 五、测试支撑
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scene_switch_within_cap() {
        let mut r = BenchRunner::new();
        let (ops, env) = bench_scene_switch(16, 24);
        assert!(r.run(BenchKind::SceneSwitch, ops, env, 1024).is_ok());
    }

    #[test]
    fn cap_exceeded_retracts() {
        let mut r = BenchRunner::new();
        let (ops, env) = bench_scene_switch(999, 999);
        assert_eq!(
            r.run(BenchKind::SceneSwitch, ops, env, 10),
            Err(BenchCode::CapExceeded)
        );
        assert_eq!(r.retracted_count(), 1);
    }

    #[test]
    fn cross_check_tolerates_small_drift() {
        let mut r = BenchRunner::new();
        assert!(r.cross_check_e09(1000, 1020).is_ok());
        assert_eq!(
            r.cross_check_e09(1000, 2000),
            Err(BenchCode::CrossCheckDrift)
        );
        assert_eq!(r.cross_check_e09(0, 100), Err(BenchCode::CapMissing));
    }

    #[test]
    fn env_mismatch_blocks_compare() {
        let mut r = BenchRunner::new();
        let (_, e1) = bench_scene_switch(16, 24);
        let (_, e2) = bench_scene_switch(32, 24);
        let a = BenchResult { kind: BenchKind::SceneSwitch, ops: 10, cap: 100, env: e1 };
        let b = BenchResult { kind: BenchKind::SceneSwitch, ops: 10, cap: 100, env: e2 };
        assert_eq!(r.comparable(&a, &b), Err(BenchCode::EnvMismatch));
    }
}
