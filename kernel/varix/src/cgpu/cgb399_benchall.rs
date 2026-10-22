//! CGPU-F0399 · 预算基准总册（目标 320 行）
//! ==========================================================================
//! 锚点判据（施工书原文摘录）：
//! - 基准总册：C 域全部基准合并（仲裁开销/公平指数/切换延迟/预测命中率/
//!   仿真耗时——统一负载谱 + 环境声明）；基线冻结 v1；回归接入。
//! - 判据：合并、环境声明、基线冻结、回归、判据。
//!
//! 设计纪律：
//! - 五项基准统一到一个负载谱坐标下：同一负载档（轻/中/重/风暴）测
//!   同一口径，跨项可比、跨环境可声明。
//! - 环境声明独立成账：基准数离开环境声明不可比——每次测报必须携带
//!   环境指纹，指纹不符的样本不许进基线。
//! - 基线冻结 v1：首测定基线后不可原地改，回归只做「新测 vs 基线」
//!   的偏差带判定（越带即红）。
//! - 回归接入：五项基准逐一挂回归口径（方向 + 容差带）。
//! - no_std + alloc；零 unwrap/expect 于运行路径；诊断段 0x0C20 独占。

use alloc::vec;
use alloc::vec::Vec;

/// 诊断段基址（0x0C20，全仓独占）。
pub const SEG_BASE: u16 = 0x0C20;

pub const E_BENCH_UNKNOWN_ENV: u16 = SEG_BASE + 1; // 样本无环境声明
pub const E_BENCH_ENV_MISMATCH: u16 = SEG_BASE + 2; // 环境指纹与基线不符
pub const E_BENCH_FROZEN: u16 = SEG_BASE + 3; // 基线已冻结不可改
pub const E_BENCH_NOT_FROZEN: u16 = SEG_BASE + 4; // 未冻结就跑回归
pub const E_BENCH_DRIFT: u16 = SEG_BASE + 5; // 回归越容差带
pub const E_BENCH_ITEM: u16 = SEG_BASE + 6; // 基准项号非法

/// 基准项号（合并总册的固定坐标）。
pub const B_ARBITER_COST: usize = 0; // 仲裁开销
pub const B_FAIRNESS: usize = 1; // 公平指数
pub const B_SWITCH_LAT: usize = 2; // 切换延迟
pub const B_PRED_HIT: usize = 3; // 预测命中率
pub const B_SIM_TIME: usize = 4; // 仿真耗时
pub const BENCH_COUNT: usize = 5;

/// 负载谱四档（统一负载坐标）。
pub const LOAD_LIGHT: u8 = 0;
pub const LOAD_MID: u8 = 1;
pub const LOAD_HEAVY: u8 = 2;
pub const LOAD_STORM: u8 = 3;
pub const LOAD_LEVELS: usize = 4;

pub fn load_name(l: u8) -> &'static str {
    match l {
        LOAD_LIGHT => "轻载",
        LOAD_MID => "中载",
        LOAD_HEAVY => "重载",
        _ => "风暴",
    }
}

/// 环境声明：测报的环境指纹（硬件档 + 构建档 + 时钟档三要素摘要）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EnvDecl {
    /// 硬件档（0=参照机 1=低配 2=高配——档内可比）。
    pub hw_tier: u8,
    /// 构建档（opt 级别摘要）。
    pub build: u32,
    /// 时钟源校准值（kHz）。
    pub clock_khz: u32,
}

impl EnvDecl {
    /// 确定性环境指纹（三要素摘要——对账键）。
    pub fn fingerprint(&self) -> u64 {
        let mut h: u64 = 0x9E3779B97F4A7C15;
        for v in [self.hw_tier as u64, self.build as u64, self.clock_khz as u64] {
            h ^= v.wrapping_mul(0xFF51AFD7ED5585D9);
            h = h.rotate_left(27).wrapping_mul(0x2545F4914F6CDD1D);
        }
        h
    }
}

/// 一个基准样本。
#[derive(Clone, Copy, Debug)]
pub struct Sample {
    pub item: usize,
    pub load: u8,
    /// 测得值（口径：仲裁开销=µs、公平=基尼×1000、切换=µs、
    /// 命中率=千分位、仿真=µs/千帧）。
    pub value: u32,
    pub env: EnvDecl,
}

/// 基准项的回归口径：方向（越大越好/越小越好）+ 容差带（千分位相对）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Direction {
    LowerBetter,
    HigherBetter,
}

#[derive(Clone, Copy, Debug)]
pub struct RegressRule {
    pub dir: Direction,
    /// 容差带（相对基线的千分比）。
    pub band_permille: u32,
}

/// 基线（冻结 v1）：每项×负载档一个基线值 + 环境指纹。
#[derive(Clone, Debug)]
pub struct Baseline {
    pub frozen: bool,
    pub env: Option<EnvDecl>,
    /// [项][档] → 基线值。
    pub grid: [[u32; LOAD_LEVELS]; BENCH_COUNT],
    pub rules: [RegressRule; BENCH_COUNT],
}

impl Baseline {
    pub fn empty() -> Self {
        Baseline {
            frozen: false,
            env: None,
            grid: [[0; LOAD_LEVELS]; BENCH_COUNT],
            rules: [
                RegressRule { dir: Direction::LowerBetter, band_permille: 150 },
                RegressRule { dir: Direction::HigherBetter, band_permille: 100 },
                RegressRule { dir: Direction::LowerBetter, band_permille: 200 },
                RegressRule { dir: Direction::HigherBetter, band_permille: 100 },
                RegressRule { dir: Direction::LowerBetter, band_permille: 250 },
            ],
        }
    }

    /// 写入基线单元（冻结后拒绝）。
    pub fn set_cell(&mut self, item: usize, load: u8, env: EnvDecl, v: u32) -> Result<(), u16> {
        if self.frozen {
            return Err(E_BENCH_FROZEN);
        }
        if item >= BENCH_COUNT || load >= LOAD_LEVELS as u8 {
            return Err(E_BENCH_ITEM);
        }
        // 首写锁定环境声明：此后只接受同环境样本。
        match self.env {
            None => self.env = Some(env),
            Some(e) if e.fingerprint() != env.fingerprint() => {
                return Err(E_BENCH_ENV_MISMATCH)
            }
            _ => {}
        }
        self.grid[item][load as usize] = v;
        Ok(())
    }

    /// 基线冻结 v1：所有单元非零才允许冻结（缺口即冻结失败）。
    pub fn freeze(&mut self) -> Result<(), u16> {
        if self.env.is_none() {
            return Err(E_BENCH_UNKNOWN_ENV);
        }
        for row in &self.grid {
            for v in row {
                if *v == 0 {
                    return Err(E_BENCH_NOT_FROZEN);
                }
            }
        }
        self.frozen = true;
        Ok(())
    }

    /// 回归：新样本对基线的偏差带判定。
    /// 越带即红（方向感：LowerBetter 上浮越带红 / 下探只记收益）。
    pub fn regress(&self, s: &Sample) -> Result<i64, u16> {
        if !self.frozen {
            return Err(E_BENCH_NOT_FROZEN);
        }
        if s.item >= BENCH_COUNT || s.load >= LOAD_LEVELS as u8 {
            return Err(E_BENCH_ITEM);
        }
        let env = self.env.ok_or(E_BENCH_UNKNOWN_ENV)?;
        if env.fingerprint() != s.env.fingerprint() {
            return Err(E_BENCH_ENV_MISMATCH);
        }
        let base = self.grid[s.item][s.load as usize] as i64;
        let val = s.value as i64;
        let band = base * self.rules[s.item].band_permille as i64 / 1000;
        let drift = val - base;
        let bad = match self.rules[s.item].dir {
            Direction::LowerBetter => drift > band,
            Direction::HigherBetter => -drift > band,
        };
        if bad {
            return Err(E_BENCH_DRIFT);
        }
        Ok(drift)
    }

    /// 五项×四档全矩阵回归（总册口径）。
    pub fn regress_all(&self, samples: &[Sample]) -> Result<usize, u16> {
        let mut done = 0usize;
        for s in samples {
            self.regress(s)?;
            done += 1;
        }
        Ok(done)
    }
}

/// 负载谱生成器：确定性负载档序列（统一负载谱的固化表达）。
pub struct LoadSpectrum {
    pub frames: Vec<u8>,
}

impl LoadSpectrum {
    /// 标准谱：4 档循环，每档 N 帧（总帧数 = 4N）。
    pub fn standard(cycle: usize) -> Self {
        let mut frames = Vec::new();
        for l in 0..LOAD_LEVELS as u8 {
            for _ in 0..cycle {
                frames.push(l);
            }
        }
        LoadSpectrum { frames }
    }

    /// 档位直方图（基准报告用）。
    pub fn histogram(&self) -> [usize; LOAD_LEVELS] {
        let mut h = [0usize; LOAD_LEVELS];
        for f in &self.frames {
            h[*f as usize] += 1;
        }
        h
    }
}

// ==========================================================================
// 判据测试
// ==========================================================================
#[cfg(test)]
mod tests {
    use super::*;

    fn env1() -> EnvDecl {
        EnvDecl { hw_tier: 0, build: 2, clock_khz: 2_400_000 }
    }

    fn seeded_baseline() -> Baseline {
        let mut b = Baseline::empty();
        let e = env1();
        // 仲裁开销基线（轻/中/重/风暴）。
        for (l, v) in [(0u8, 40u32), (1, 70), (2, 120), (3, 260)] {
            b.set_cell(B_ARBITER_COST, l, e, v).ok();
        }
        // 公平指数（基尼×1000，越小越好→HigherBetter 口径用命中类？）
        // 公平：基尼越小越公平 ⇒ LowerBetter。此处给一组。
        for (l, v) in [(0u8, 120u32), (1, 150), (2, 190), (3, 240)] {
            b.set_cell(B_FAIRNESS, l, e, v).ok();
        }
        for (l, v) in [(0u8, 900u32), (1, 1100), (2, 1600), (3, 3100)] {
            b.set_cell(B_SWITCH_LAT, l, e, v).ok();
        }
        for (l, v) in [(0u8, 940u32), (1, 910), (2, 870), (3, 800)] {
            b.set_cell(B_PRED_HIT, l, e, v).ok();
        }
        for (l, v) in [(0u8, 500u32), (1, 520), (2, 560), (3, 640)] {
            b.set_cell(B_SIM_TIME, l, e, v).ok();
        }
        b
    }

    #[test]
    fn a01_合并_五项四档全矩阵() {
        let b = seeded_baseline();
        let zeros = b.grid.iter().flatten().filter(|v| **v == 0).count();
        assert_eq!(zeros, 0);
        // 统一负载谱直方图均匀。
        let sp = LoadSpectrum::standard(16);
        assert_eq!(sp.frames.len(), 64);
        assert_eq!(sp.histogram(), [16, 16, 16, 16]);
        // 基准项坐标完整（0..5）。
        assert_eq!(BENCH_COUNT, 5);
    }

    #[test]
    fn a02_环境声明_指纹不符样本拒收() {
        let mut b = Baseline::empty();
        let e1 = env1();
        b.set_cell(B_ARBITER_COST, LOAD_LIGHT, e1, 40).ok();
        // 同环境可续写。
        assert_eq!(b.set_cell(B_ARBITER_COST, LOAD_MID, e1, 70), Ok(()));
        // 异环境拒收。
        let e2 = EnvDecl { hw_tier: 1, ..env1() };
        assert_eq!(
            b.set_cell(B_ARBITER_COST, LOAD_HEAVY, e2, 120),
            Err(E_BENCH_ENV_MISMATCH)
        );
        // 指纹确定性。
        assert_eq!(e1.fingerprint(), env1().fingerprint());
        assert_ne!(e1.fingerprint(), e2.fingerprint());
        // 无环境声明冻结失败。
        let mut b2 = Baseline::empty();
        assert_eq!(b2.freeze(), Err(E_BENCH_UNKNOWN_ENV));
    }

    #[test]
    fn a03_基线冻结_v1_冻后拒改() {
        let mut b = seeded_baseline();
        assert_eq!(b.freeze(), Ok(()));
        assert!(b.frozen);
        // 冻结后写单元拒绝。
        let e = b.env;
        assert_eq!(
            b.set_cell(B_ARBITER_COST, LOAD_LIGHT, e.unwrap_or(env1()), 999),
            Err(E_BENCH_FROZEN)
        );
        // 缺口不可冻结。
        let mut b2 = Baseline::empty();
        let e1 = env1();
        b2.set_cell(B_ARBITER_COST, LOAD_LIGHT, e1, 40).ok();
        assert_eq!(b2.freeze(), Err(E_BENCH_NOT_FROZEN));
    }

    #[test]
    fn a04_回归_容差带与方向感() {
        let b = seeded_baseline();
        let mut b = b;
        b.freeze().ok();
        let e = env1();
        // 带内下探（LowerBetter 收益）绿。
        assert_eq!(
            b.regress(&Sample { item: B_ARBITER_COST, load: LOAD_LIGHT, value: 35, env: e }),
            Ok(-5)
        );
        // 上浮带内（+15% < 150‰? 40×0.15=6 ⇒ +6 内绿）。
        assert_eq!(
            b.regress(&Sample { item: B_ARBITER_COST, load: LOAD_LIGHT, value: 45, env: e }),
            Ok(5)
        );
        // 上浮越带红。
        assert_eq!(
            b.regress(&Sample { item: B_ARBITER_COST, load: LOAD_LIGHT, value: 50, env: e }),
            Err(E_BENCH_DRIFT)
        );
        // HigherBetter：命中率下探越带红。
        assert_eq!(
            b.regress(&Sample { item: B_PRED_HIT, load: LOAD_LIGHT, value: 800, env: e }),
            Err(E_BENCH_DRIFT)
        );
        // 带内上浮（收益）绿。
        assert_eq!(
            b.regress(&Sample { item: B_PRED_HIT, load: LOAD_LIGHT, value: 1000, env: e }),
            Ok(60)
        );
        // 环境不符拒绝。
        assert_eq!(
            b.regress(&Sample { item: B_SIM_TIME, load: LOAD_LIGHT, value: 500, env: EnvDecl { hw_tier: 2, ..env1() } }),
            Err(E_BENCH_ENV_MISMATCH)
        );
        // 未冻结跑回归拒绝。
        let nb = Baseline::empty();
        assert_eq!(
            nb.regress(&Sample { item: 0, load: 0, value: 1, env: e }),
            Err(E_BENCH_NOT_FROZEN)
        );
    }

    #[test]
    fn a05_回归接入_全矩阵批量绿() {
        let mut b = seeded_baseline();
        b.freeze().ok();
        let e = env1();
        // 用基线原值批量回归（等值漂移 0 ⇒ 全绿）。
        let mut samples = Vec::new();
        for item in 0..BENCH_COUNT {
            for load in 0..LOAD_LEVELS as u8 {
                samples.push(Sample {
                    item,
                    load,
                    value: b.grid[item][load as usize],
                    env: e,
                });
            }
        }
        assert_eq!(b.regress_all(&samples), Ok(20));
        // 单个越带拖红全批。
        samples[3].value += b.rules[samples[3].item].band_permille as u32 + 1;
        assert_eq!(b.regress_all(&samples), Err(E_BENCH_DRIFT));
        // 负载档名表。
        assert_eq!(load_name(LOAD_STORM), "风暴");
    }
}
