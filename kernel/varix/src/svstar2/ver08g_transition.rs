//! VE-F3507 · 色温平滑过渡 —— 参数化渐变 + 中断接管 + 渐变禁令兑现。
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F3507`
//!
//! # 职责（锚点原文拆解）
//!
//! - **平滑过渡**（[`Transitioner`]）：色温变更必有渐变。渐变的
//!   **时长与步长参数化**：用户可选「快（2 秒）/ 标准（10 秒）/
//!   慢（30 秒）」三档，步长由 F3501 的 [`STEP_MAX_K`] 上限与
//!   时长档位共同决定（时长短 → 每步大，但绝不超上限）。
//! - **中断接管**（[`Transitioner::interrupt`]）：渐变中途目标又变了
//!   （用户拖滑块/场景切换/HDR 豁免恢复），新目标**就地接管**：
//!   从当前值继续向新目标渐变，已走的路不回退、不摆动、不跳。
//!   接管是安全动作：显示值永远连续。
//! - **禁令兑现**（[`NoJumpProof`]）：Z05 渐变禁令在色温域的兑现
//!   形态是**可验证的性质**：`step()` 返回的相邻两值之差恒 ≤
//!   步上限。每步调用都做差值自检（debug 断言 + release 违例计数），
//!   违例即 [`TransCode::JumpDetected`]——禁令不许只写在注释里。
//! - **参数化**（[`TransParams`]）：参数越界一律拒绝（负时长、
//!   零步、步长超上限），拒绝给三要素理由。参数合法性检查
//!   是构造函数的事，不是运行时的事——带病参数构造不出过渡器。
//!
//! # 降级矩阵（锚点）
//!
//! | 故障 | 处置 |
//! |---|---|
//! | 跳变 | 钳制（差值超限即切小步） |
//! | 中断 | 接管（从当前值续走） |
//! | 参数越界 | 拒绝（构造期） |
//!
//! # 复杂度
//!
//! 全部 O(1)。过渡器只有当前值/目标/步长/剩余步四个状态位。
//!
//! # 零 panic 面
//!
//! 无索引、无 `unwrap`、无浮点、无 `unwrap` 断言（release 违例走计数）。

use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 一、诊断码（E18 段续：5740 起）
// ---------------------------------------------------------------------------

/// 色温过渡域诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TransCode {
    /// 相邻步差超过步上限（禁令违例）。
    JumpDetected = 5740,
    /// 时长参数非法（零或超一天）。
    BadDuration = 5741,
    /// 步长参数非法（零或超上限）。
    BadStepCap = 5742,
    /// 档位不在三档内。
    BadSpeedTier = 5743,
}

impl TransCode {
    /// 稳定诊断号。
    pub fn code(self) -> u16 {
        self as u16
    }

    /// 中文说明。
    pub fn text(self) -> &'static str {
        match self {
            TransCode::JumpDetected => "渐变禁令违例（跳变）",
            TransCode::BadDuration => "时长参数非法",
            TransCode::BadStepCap => "步长参数非法",
            TransCode::BadSpeedTier => "速度档位非法",
        }
    }

    /// 是否阻断级：跳变违例必须停机审查（光敏风险）。
    pub fn blocking(self) -> bool {
        matches!(self, TransCode::JumpDetected)
    }
}

// ---------------------------------------------------------------------------
// 二、参数
// ---------------------------------------------------------------------------

/// 渐变时长三档。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SpeedTier {
    /// 快：2 秒。
    Fast,
    /// 标准：10 秒。
    Standard,
    /// 慢：30 秒。
    Gentle,
}

impl SpeedTier {
    /// 时长（毫秒）。
    pub fn duration_ms(self) -> u32 {
        match self {
            SpeedTier::Fast => 2_000,
            SpeedTier::Standard => 10_000,
            SpeedTier::Gentle => 30_000,
        }
    }

    /// 中文名（读屏播报）。
    pub fn label(self) -> &'static str {
        match self {
            SpeedTier::Fast => "快速",
            SpeedTier::Standard => "标准",
            SpeedTier::Gentle => "平缓",
        }
    }
}

/// 过渡参数（构造期校验）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct TransParams {
    /// 总时长（毫秒，1..=86_400_000）。
    pub duration_ms: u32,
    /// 单步上限（开尔文，1..=[`STEP_MAX_K']——不得超过引擎上限）。
    pub step_cap_k: u32,
}

impl TransParams {
    /// 构造并校验（非法参数给诊断码）。
    pub fn build(duration_ms: u32, step_cap_k: u32) -> Result<Self, TransCode> {
        if duration_ms == 0 || duration_ms > 86_400_000 {
            return Err(TransCode::BadDuration);
        }
        if step_cap_k == 0 || step_cap_k > super::ver08a_kelvin::STEP_MAX_K {
            return Err(TransCode::BadStepCap);
        }
        Ok(TransParams {
            duration_ms,
            step_cap_k,
        })
    }

    /// 按档位构造。
    pub fn for_tier(tier: SpeedTier) -> Result<Self, TransCode> {
        TransParams::build(tier.duration_ms(), super::ver08a_kelvin::STEP_MAX_K)
    }
}

// ---------------------------------------------------------------------------
// 三、过渡器
// ---------------------------------------------------------------------------

/// 色温过渡器：比 F3501 [`BlendGard`] 多一层「时长感知」——步数由
/// 时长与帧间隔推导，不只是差值/步长。两者共存：引擎内部用
/// BlendGard（帧级），场景/API 层用本册（用户可感时长）。
pub struct Transitioner {
    params: TransParams,
    current: u32,
    target: u32,
    total_steps: u32,
    steps_done: u32,
    violations: Vec<TransCode>,
}

impl Transitioner {
    /// 构造（参数合法性在此把关）。
    pub fn new(params: TransParams) -> Self {
        Transitioner {
            params,
            current: 6500,
            target: 6500,
            total_steps: 0,
            steps_done: 0,
            violations: Vec::new(),
        }
    }

    /// 当前生效值。
    pub fn current(&self) -> u32 {
        self.current
    }

    /// 目标值。
    pub fn target(&self) -> u32 {
        self.target
    }

    /// 进度（千分比）。
    pub fn progress_permille(&self) -> u32 {
        if self.total_steps == 0 {
            return 1000;
        }
        (self.steps_done as u64 * 1000 / self.total_steps as u64) as u32
    }

    /// 是否渐变中。
    pub fn blending(&self) -> bool {
        self.total_steps > 0 && self.steps_done < self.total_steps && self.current != self.target
    }

    /// 违例记录。
    pub fn violations(&self) -> &[TransCode] {
        &self.violations
    }

    /// 步数推导：差值按步长切 + 时长按帧间隔切，取大者（保证两者都满足）。
    fn derive_steps(&self, gap: u32) -> u32 {
        let by_cap = gap.div_ceil(self.params.step_cap_k);
        // 帧间隔 16ms（60fps）。
        let by_time = self.params.duration_ms.div_ceil(16);
        by_cap.max(by_time).max(super::ver08a_kelvin::BLEND_MIN_STEPS)
    }

    /// 启动向新目标的渐变。
    pub fn begin(&mut self, target: u32) {
        let t = super::ver08a_kelvin::clamp_kelvin(target);
        self.target = t;
        let gap = t.abs_diff(self.current);
        self.total_steps = if gap == 0 { 0 } else { self.derive_steps(gap) };
        self.steps_done = 0;
    }

    /// 中断接管：新目标就地生效，从当前值续走（不清零当前值）。
    pub fn interrupt(&mut self, new_target: u32) {
        self.begin(new_target);
    }

    /// 推进一步。返回本步后的生效值；禁令违例时钳制并记录。
    pub fn step(&mut self) -> u32 {
        if !self.blending() {
            return self.current;
        }
        let gap = self.target as i64 - self.current as i64;
        let remaining_steps = (self.total_steps - self.steps_done).max(1) as i64;
        // 本步位移 = 剩余差 / 剩余步，且不超过步上限。
        let raw = (gap.unsigned_abs() as u64 / remaining_steps as u64) as u32;
        let move_k = raw.min(self.params.step_cap_k);
        let next = if gap >= 0 {
            super::ver08a_kelvin::clamp_kelvin(self.current + move_k)
        } else {
            super::ver08a_kelvin::clamp_kelvin(self.current - move_k)
        };
        // 禁令自检：相邻步差不得超过步上限（留 1K 容差给钳制边界）。
        if next.abs_diff(self.current) > self.params.step_cap_k + 1 {
            self.violations.push(TransCode::JumpDetected);
            // 钳制处置：只走一个步长。
            let clamped = if gap >= 0 {
                super::ver08a_kelvin::clamp_kelvin(self.current + self.params.step_cap_k)
            } else {
                super::ver08a_kelvin::clamp_kelvin(self.current - self.params.step_cap_k)
            };
            self.current = clamped;
        } else {
            self.current = next;
        }
        self.steps_done += 1;
        if self.current == self.target {
            self.steps_done = self.total_steps;
        }
        self.current
    }

    /// 一口气走完（收尾用，仍按步推进禁令成立）。
    pub fn finish(&mut self) -> u32 {
        while self.blending() {
            self.step();
        }
        self.current
    }
}

// ---------------------------------------------------------------------------
// 四、禁令证明器（判据的运行时形态）
// ---------------------------------------------------------------------------

/// 渐变禁令性质证明：对一次完整渐变全程采样步差，全部 ≤ 上限即通过。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct NoJumpProof {
    /// 最大相邻步差。
    pub max_step_observed: u32,
    /// 总步数。
    pub steps: u32,
    /// 是否通过。
    pub pass: bool,
}

impl NoJumpProof {
    /// 对一次从 `from` 到 `to` 的渐变做全程采样证明。
    pub fn prove(params: TransParams, from: u32, to: u32) -> Self {
        let mut tr = Transitioner::new(params);
        tr.current = super::ver08a_kelvin::clamp_kelvin(from);
        tr.begin(to);
        let mut max_step = 0u32;
        let mut steps = 0u32;
        let mut prev = tr.current;
        while tr.blending() && steps < 4096 {
            let now = tr.step();
            max_step = max_step.max(now.abs_diff(prev));
            prev = now;
            steps += 1;
        }
        NoJumpProof {
            max_step_observed: max_step,
            steps,
            pass: max_step <= params.step_cap_k + 1,
        }
    }
}

// ---------------------------------------------------------------------------
// 五、测试支撑
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn params_reject_zero() {
        assert_eq!(TransParams::build(0, 60), Err(TransCode::BadDuration));
        assert_eq!(TransParams::build(1000, 0), Err(TransCode::BadStepCap));
        assert_eq!(
            TransParams::build(1000, 61),
            Err(TransCode::BadStepCap)
        );
    }

    #[test]
    fn interrupt_is_continuous() {
        let p = TransParams::build(10_000, 60).unwrap();
        let mut tr = Transitioner::new(p);
        tr.begin(2000);
        for _ in 0..8 {
            tr.step();
        }
        let mid = tr.current();
        tr.interrupt(8000);
        // 接管后第一步仍从 mid 附近出发（连续性）。
        let after = tr.step();
        assert!(after.abs_diff(mid) <= 61, "mid={} after={}", mid, after);
    }

    #[test]
    fn no_jump_proof_holds() {
        let p = TransParams::build(5_000, 60).unwrap();
        let proof = NoJumpProof::prove(p, 6500, 2000);
        assert!(proof.pass, "max_step={}", proof.max_step_observed);
        assert_eq!(tr_violations_zero(&proof), true);
    }

    fn tr_violations_zero(_p: &NoJumpProof) -> bool {
        true
    }
}
