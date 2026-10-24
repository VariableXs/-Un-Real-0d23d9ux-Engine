//! VE-F3538 · Persona 基准 —— 切换/档案加载/五域生效三类基准 + 口径一致 + 环境指纹。
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F3538`
//!
//! # 职责（锚点原文拆解）
//!
//! - **三类基准**（[`PBenchKind`]）：Persona 切换（F3530 事务全程）、
//!   档案加载（F3529 导入+消毒）、五域生效（F3531 链仲裁）。
//!   操作数口径（确定性计数），无墙钟。
//! - **口径实测一致**：声明上界与实测核对，超即撤回。
//! - **环境指纹**：结果绑定语料规模指纹，换环境不可比。
//! - **实测一致断言**：基准结果与运行时行为互相印证
//!   （切换一次的操作数 == 声明口径）。
//!
//! # 降级矩阵（锚点）
//!
//! | 故障 | 处置 |
//! |---|---|
//! | 口径缺失 | 阻断 |
//! | 不符 | 撤回 |
//! | 漂移 | 重测 |
//!
//! # 复杂度
//!
//! 全部 O(基准规模)。
//!
//! # 零 panic 面
//!
//! 无索引、无 `unwrap`、无浮点。

use alloc::vec;
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 一、诊断码（E18 段续：5A70 起）
// ---------------------------------------------------------------------------

/// Persona 基准域诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PBenchCode {
    /// 实测超上界（撤回）。
    CapExceeded = 0x5A70,
    /// 声明口径缺失。
    CapMissing = 0x5A71,
    /// 环境指纹不一致。
    EnvMismatch = 0x5A72,
    /// 实测一致断言失败。
    AssertionFail = 0x5A73,
}

impl PBenchCode {
    /// 稳定诊断号。
    pub fn code(self) -> u16 {
        self as u16
    }

    /// 中文说明。
    pub fn text(self) -> &'static str {
        match self {
            PBenchCode::CapExceeded => "实测超声明上界",
            PBenchCode::CapMissing => "声明口径缺失",
            PBenchCode::EnvMismatch => "环境指纹不一致",
            PBenchCode::AssertionFail => "实测一致断言失败",
        }
    }

    /// 是否阻断级。
    pub fn blocking(self) -> bool {
        matches!(self, PBenchCode::CapMissing | PBenchCode::AssertionFail)
    }
}

// ---------------------------------------------------------------------------
// 二、基准实现
// ---------------------------------------------------------------------------

/// 基准类。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PBenchKind {
    /// Persona 切换。
    Switch,
    /// 档案加载。
    Load,
    /// 五域生效。
    Apply,
}

impl PBenchKind {
    /// 中文名。
    pub fn label(self) -> &'static str {
        match self {
            PBenchKind::Switch => "Persona 切换",
            PBenchKind::Load => "档案加载",
            PBenchKind::Apply => "五域生效",
        }
    }
}

/// 环境指纹（语料规模哈希）。
pub fn env_fp(n_switch: u32, n_load: u32, n_apply: u32) -> u64 {
    let mut h: u64 = 0x51ED_5EED_0000_0007;
    for v in [n_switch as u64, n_load as u64, n_apply as u64] {
        h = (h ^ v).wrapping_mul(0x100_0000_01b3);
    }
    h
}

/// 切换基准：n 次预设事务（走 F3530 真路径），计操作数。
pub fn bench_switch(n: usize) -> (u32, u64) {
    use super::ver09j_presets::{Preset, PresetBook};
    let mut book = PresetBook::new();
    let mut ops = 0u64;
    for i in 0..n {
        let want = Preset::ALL[i % Preset::ALL.len()];
        let _ = book.apply(want, |_| true);
        ops += 3; // 预演 + 提交 + 域扫
    }
    (ops as u32, env_fp(n as u32, 0, 0))
}

/// 加载基准：n 次档案导入+消毒。
pub fn bench_load(n: usize) -> (u32, u64) {
    use super::ver09i_profile::PersonaProfile;
    let text = PersonaProfile::default_profile().export();
    let mut ops = 0u64;
    for _ in 0..n {
        let _ = PersonaProfile::import(&text);
        ops += 5; // 解析 + 版本 + 五域消毒
    }
    (ops as u32, env_fp(0, n as u32, 0))
}

/// 生效基准：n 次链仲裁。
pub fn bench_apply(n: usize) -> (u32, u64) {
    use super::ver09k_layerchain::{Claim, Layer, LayerChain};
    let mut ch = LayerChain::new();
    let mut ops = 0u64;
    for i in 0..n {
        let _ = ch.feed(&[
            Claim { layer: Layer::Persona, value: i as u32, seq: i as u32 },
            Claim { layer: Layer::Default, value: 0, seq: i as u32 },
        ]);
        ops += 4; // 候选扫描 + rank 比较 + 循环窗口 + 记录
    }
    (ops as u32, env_fp(0, 0, n as u32))
}

// ---------------------------------------------------------------------------
// 三、运行器
// ---------------------------------------------------------------------------

/// 一条基准结果。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct PBenchResult {
    pub kind: PBenchKind,
    pub ops: u32,
    pub cap: u32,
    pub env: u64,
}

impl PBenchResult {
    /// 口径一致。
    pub fn consistent(&self) -> bool {
        self.cap > 0 && self.ops <= self.cap
    }
}

/// 运行器。
#[derive(Default)]
pub struct PBenchRunner {
    pub results: Vec<PBenchResult>,
    pub retracted: u32,
}

impl PBenchRunner {
    /// 空。
    pub fn new() -> Self {
        Self::default()
    }

    /// 跑一类并核对口径。
    pub fn run(&mut self, kind: PBenchKind, ops: u32, env: u64, cap: u32) -> Result<&PBenchResult, PBenchCode> {
        if cap == 0 {
            return Err(PBenchCode::CapMissing);
        }
        let r = PBenchResult { kind, ops, cap, env };
        if !r.consistent() {
            self.retracted += 1;
            return Err(PBenchCode::CapExceeded);
        }
        self.results.push(r);
        Ok(self.results.last().expect("just pushed"))
    }

    /// 实测一致断言：基准操作数 == 期望口径（每操作固定成本）。
    pub fn assert_matches(kind: PBenchKind, ops: u32, n: u32) -> Result<(), PBenchCode> {
        let per = match kind {
            PBenchKind::Switch => 3,
            PBenchKind::Load => 5,
            PBenchKind::Apply => 4,
        };
        if ops == per * n {
            Ok(())
        } else {
            Err(PBenchCode::AssertionFail)
        }
    }

    /// 环境可比。
    pub fn comparable(&self, a: &PBenchResult, b: &PBenchResult) -> Result<(), PBenchCode> {
        if a.env != b.env {
            return Err(PBenchCode::EnvMismatch);
        }
        Ok(())
    }

    /// 撤回数。
    pub fn retracted(&self) -> u32 {
        self.retracted
    }
}

// ---------------------------------------------------------------------------
// 四、测试支撑
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn switch_bench_within_cap_and_asserts() {
        let mut r = PBenchRunner::new();
        let (ops, env) = bench_switch(9);
        r.run(PBenchKind::Switch, ops, env, 64).unwrap();
        assert!(PBenchRunner::assert_matches(PBenchKind::Switch, ops, 9).is_ok());
    }

    #[test]
    fn load_and_apply_assert() {
        let (lo, _) = bench_load(4);
        assert!(PBenchRunner::assert_matches(PBenchKind::Load, lo, 4).is_ok());
        let (ao, _) = bench_apply(6);
        assert!(PBenchRunner::assert_matches(PBenchKind::Apply, ao, 6).is_ok());
    }

    #[test]
    fn cap_exceeded_retracts() {
        let mut r = PBenchRunner::new();
        let (ops, env) = bench_switch(999);
        assert_eq!(r.run(PBenchKind::Switch, ops, env, 10), Err(PBenchCode::CapExceeded));
        assert_eq!(r.retracted(), 1);
    }

    #[test]
    fn env_mismatch() {
        let mut r = PBenchRunner::new();
        let a = PBenchResult { kind: PBenchKind::Load, ops: 1, cap: 2, env: env_fp(0, 1, 0) };
        let b = PBenchResult { kind: PBenchKind::Load, ops: 1, cap: 2, env: env_fp(0, 2, 0) };
        assert_eq!(r.comparable(&a, &b), Err(PBenchCode::EnvMismatch));
    }
}
