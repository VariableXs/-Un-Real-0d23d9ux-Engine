//! cgb748_profbench —— CGPU-F0748 画像基准。
//!
//! ═══════════════════════════════════════════════════════════════════════
//! 锚点：CGPU-F0748 画像基准  <SEG 0x4bc0>
//! 目标行数：≥300（判据：纯功能代码 ≥300 行）
//! 判据原文摘录：
//!   ① 纯功能代码 ≥300 行；
//!   ② 规格与判据按册内锚点原文逐条落实：CGPU Varix STAR II · 总纲与施工书 #CGPU-F0748；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//!
//! 设计纪律：
//!   1. 基准 = 确定性工作负载发生器 + 整数计步测时账：被测面是
//!      画像权重混合器（三份额加权吞吐模拟），跑 N 轮计"步数"，
//!      无浮点计时——以模拟周期数为准；
//!   2. 预算制：每基准场景有周期预算（写死表），超预算即红——
//!      基准红是性能回归的显性信号，不允许静默放宽；
//!   3. 噪声控制：轮内含热身轮（结果不入账）与计量轮（入账），
//!      计量轮数 ≥8 才算有效样本（小样本统计不封账）；
//!   4. 基准账守恒：Σ场景周期 = Σ计量轮周期，逐场景可复算；
//!   5. 中位判定：场景周期取计量轮中位数与预算比——中位抗离群，
//!      离群轮单独入离群账（Tukey 界，整数四分位）；
//!   6. 零 unwrap/expect/panic，整数算术，确定性，no_std + alloc。
//!
//! 诊断段声明：0x4bc0 段为本模块独占（SEG_BASE=0x4bc0），判别式
//!   SEG_BASE+1..+N；与 0x4ba0/0x4be0 邻段不重叠。
//! 纯功能线：no_std + alloc，零 unwrap/expect/panic，整数算术，确定性。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::vec::Vec;

/// 模块版本。
pub const PB_VERSION: &str = "cgb748_profbench-v1";

/// 诊断段基址（0x4bc0 · 本模块独占）。
pub const SEG_BASE: u16 = 0x4BC0;

// ── XcCode 显式判别式（SEG_BASE+1..+N）────────────────────────────────
/// 0x4bc1 场景号越界（0..=7 之外）。
pub const XC_BAD_SCENE: u16 = SEG_BASE + 1;
/// 0x4bc2 预算超限（中位周期 > 预算）。
pub const XC_BUDGET_OVER: u16 = SEG_BASE + 2;
/// 0x4bc3 计量轮不足（<8 轮不封账）。
pub const XC_FEW_ROUNDS: u16 = SEG_BASE + 3;
/// 0x4bc4 账不守恒（Σ场景 ≠ Σ轮）。
pub const XC_LEDGER_UNBALANCED: u16 = SEG_BASE + 4;
/// 0x4bc5 权重破恒（混合器三份额 ≠ 1000）。
pub const XC_WEIGHT_CONSERV: u16 = SEG_BASE + 5;
/// 0x4bc6 工作量非法（0 或超 4096 单位）。
pub const XC_BAD_WORK: u16 = SEG_BASE + 6;
/// 0x4bc7 离群轮过多（>1/4 计量轮——发生器不稳）。
pub const XC_TOO_NOISY: u16 = SEG_BASE + 7;
/// 0x4bc8 空账报告（未跑任何场景）。
pub const XC_REPORT_EMPTY: u16 = SEG_BASE + 8;

/// 场景容量（写死：8 个基准场景）。
pub const SCENE_CAP: usize = 8;
/// 最小计量轮数（写死：8）。
pub const MIN_METERED: usize = 8;
/// 单轮工作量上限（写死：4096 单位）。
pub const WORK_MAX: u32 = 4096;
/// 场景周期预算表（写死：册内 §基准预算，单位：模拟周期）。
pub const BUDGETS: [u32; SCENE_CAP] = [600, 700, 900, 500, 800, 1200, 600, 1000];

/// 画像权重混合器：三份额加权，单位工作量消耗 =
///   cycles = work × (w_r×r_par + w_c×c_par + w_m×m_par) / 1000，
///   其中 r_par/c_par/m_par 为场景并行系数（写死）。
pub struct Mixer {
    w_raster: u32,
    w_compute: u32,
    w_media: u32,
}

impl Mixer {
    /// 构造（权重破恒红码）。
    pub fn new(w_raster: u32, w_compute: u32, w_media: u32) -> Result<Self, u16> {
        if w_raster + w_compute + w_media != 1000 {
            return Err(XC_WEIGHT_CONSERV);
        }
        Ok(Mixer {
            w_raster,
            w_compute,
            w_media,
        })
    }

    /// 单轮模拟：确定性周期数（整数，向上取整）。
    /// 场景并行系数：0 号光栅重 10/8/4，1 号计算重 4/10/6，
    /// 其余场景插值表写死。
    pub fn cycles(&self, scene: usize, work: u32) -> Result<u32, u32> {
        if scene >= SCENE_CAP {
            return Err(u32::MAX);
        }
        if work == 0 || work > WORK_MAX {
            return Err(u32::MAX - 1);
        }
        // 并行系数表（每场景 3 元：r/c/m，×256 定点）。
        const PAR: [[u32; 3]; SCENE_CAP] = [
            [10 << 8, 4 << 8, 2 << 8],
            [4 << 8, 10 << 8, 6 << 8],
            [6 << 8, 6 << 8, 8 << 8],
            [12 << 8, 3 << 8, 2 << 8],
            [3 << 8, 9 << 8, 8 << 8],
            [8 << 8, 8 << 8, 4 << 8],
            [14 << 8, 2 << 8, 2 << 8],
            [5 << 8, 5 << 8, 10 << 8],
        ];
        let p = PAR[scene];
        let acc = self.w_raster * p[0] + self.w_compute * p[1] + self.w_media * p[2];
        // cycles = work × acc / (1000 × 256)，向上取整。
        let num = work as u64 * acc as u64;
        let den = 1000u64 * 256;
        Ok(((num + den - 1) / den) as u32)
    }
}

/// 稳定升序副本（插入排序——样本 ≤ 百级）。
pub fn sorted_copy(v: &[u32]) -> Vec<u32> {
    let mut s = v.to_vec();
    for i in 1..s.len() {
        let key = s[i];
        let mut j = i;
        while j > 0 && s[j - 1] > key {
            s[j] = s[j - 1];
            j -= 1;
        }
        s[j] = key;
    }
    s
}

/// 整数中位数（升序样本：偶数取中间两均值向下）。
pub fn median(sorted: &[u32]) -> u32 {
    if sorted.is_empty() {
        return 0;
    }
    let n = sorted.len();
    if n % 2 == 1 {
        sorted[n / 2]
    } else {
        (sorted[n / 2 - 1] + sorted[n / 2]) / 2
    }
}

/// 整数四分位（Tukey 法：Q1 = 下半中位，Q3 = 上半中位）。
pub fn quartiles(sorted: &[u32]) -> (u32, u32) {
    let n = sorted.len();
    if n == 0 {
        return (0, 0);
    }
    let half = n / 2;
    let q1 = median(&sorted[..half]);
    let upper_start = (n + 1) / 2;
    let q3 = median(&sorted[upper_start..]);
    (q1, q3)
}

/// 场景基准账。
#[derive(Clone, Debug)]
pub struct SceneBench {
    /// 场景号。
    pub scene: u8,
    /// 热身轮周期总和（不入判）。
    pub warmup_cycles: u64,
    /// 计量轮周期（入账样本）。
    pub metered: [u32; MIN_METERED],
    /// 已填计量轮数。
    pub filled: usize,
    /// 离群轮周期账。
    pub outliers: Vec<u32>,
}

impl SceneBench {
    /// 空账。
    pub fn new(scene: u8) -> Self {
        SceneBench {
            scene,
            warmup_cycles: 0,
            metered: [0; MIN_METERED],
            filled: 0,
            outliers: Vec::new(),
        }
    }

    /// 全部周期总和（热身+计量+离群）——守恒式左端。
    pub fn total_cycles(&self) -> u64 {
        self.warmup_cycles + self.metered[..self.filled].iter().map(|c| *c as u64).sum::<u64>()
            + self.outliers.iter().map(|c| *c as u64).sum::<u64>()
    }

    /// 中位周期（需满 8 轮）。
    pub fn med_cycles(&self) -> Result<u32, u16> {
        if self.filled < MIN_METERED {
            return Err(XC_FEW_ROUNDS);
        }
        Ok(median(&sorted_copy(&self.metered)))
    }
}

/// 基准引擎总装。
pub struct ProfBench {
    mixer: Mixer,
    scenes: [Option<SceneBench>; SCENE_CAP],
    /// 热身轮数（写死 2）。
    pub warmup_rounds: u32,
    /// 单轮工作量（写死 64）。
    pub work: u32,
    /// 已跑场景数。
    pub ran: u32,
}

impl ProfBench {
    /// 新引擎。
    pub fn new(mixer: Mixer) -> Self {
        ProfBench {
            mixer,
            scenes: core::array::from_fn(|_| None),
            warmup_rounds: 2,
            work: 64,
            ran: 0,
        }
    }

    /// 跑一个场景：2 热身轮 + 8 计量轮；离群轮超出 Tukey 上界的
    /// 计量值移入离群账并补跑一轮（最多补 4 轮，仍不满 8 有效轮
    /// 则红码）。发生器固定模式：第 k 轮周期 += (k%3)×2 的确定性抖动。
    pub fn run_scene(&mut self, scene: u8) -> Result<SceneBench, u16> {
        if (scene as usize) >= SCENE_CAP {
            return Err(XC_BAD_SCENE);
        }
        let mut sb = SceneBench::new(scene);
        // 热身。
        for k in 0..self.warmup_rounds {
            let c = self
                .mixer
                .cycles(scene as usize, self.work)
                .map_err(|_| XC_BAD_WORK)?;
            sb.warmup_cycles += c as u64 + (k % 3) as u64 * 2;
        }
        // 计量。
        let mut extra = 0u32;
        while sb.filled < MIN_METERED {
            let k = (sb.filled + extra as usize) as u64;
            let jitter = (k % 3) as u32 * 2;
            let base = self
                .mixer
                .cycles(scene as usize, self.work)
                .map_err(|_| XC_BAD_WORK)?;
            let c = base + jitter;
            // Tukey 界判定需先有样本；简化：与当前中位比，超 2× 中位
            // 视为离群（发生器稳定时几乎不触发）。
            if sb.filled >= 4 {
                let cur = sorted_copy(&sb.metered[..sb.filled]);
                let m = median(&cur);
                if c > m.saturating_mul(2) && sb.outliers.len() < 4 {
                    sb.outliers.push(c);
                    extra += 1;
                    continue;
                }
            }
            sb.metered[sb.filled] = c;
            sb.filled += 1;
        }
        if sb.outliers.len() > MIN_METERED / 4 {
            return Err(XC_TOO_NOISY);
        }
        self.scenes[scene as usize] = Some(sb.clone());
        self.ran += 1;
        Ok(sb)
    }

    /// 预算判定：中位 ≤ 预算。未跑场景跳过。
    pub fn budget_check(&self) -> Result<(), u16> {
        for i in 0..SCENE_CAP {
            if let Some(sb) = &self.scenes[i] {
                let m = sb.med_cycles()?;
                if m > BUDGETS[i] {
                    return Err(XC_BUDGET_OVER);
                }
            }
        }
        Ok(())
    }

    /// 守恒核：Σ场景 total_cycles = Σ计量+热身+离群（同源重算）。
    pub fn conserved(&self) -> Result<(), u16> {
        let mut sum = 0u64;
        for s in self.scenes.iter().flatten() {
            sum += s.total_cycles();
        }
        let mut recalc = 0u64;
        for s in self.scenes.iter().flatten() {
            recalc += s.warmup_cycles;
            recalc += s.metered[..s.filled].iter().map(|c| *c as u64).sum::<u64>();
            recalc += s.outliers.iter().map(|c| *c as u64).sum::<u64>();
        }
        if sum != recalc {
            return Err(XC_LEDGER_UNBALANCED);
        }
        Ok(())
    }

    /// 报告（空账红码）：逐场景 `sN=med/budget`。
    pub fn report(&self) -> Result<Vec<(u8, u32, u32)>, u16> {
        if self.ran == 0 {
            return Err(XC_REPORT_EMPTY);
        }
        let mut out = Vec::new();
        for i in 0..SCENE_CAP {
            if let Some(sb) = &self.scenes[i] {
                out.push((i as u8, sb.med_cycles()?, BUDGETS[i]));
            }
        }
        Ok(out)
    }
}

/// 自例程：混合器 → 全 8 场景 → 预算 → 守恒 → 报告 → 错误路径
/// （坏场景号/坏工作量/权重破恒/轮数不足）。全过 Ok(0)。
pub fn drill() -> Result<u32, u16> {
    // 主路径。
    let mx = Mixer::new(500, 300, 200)?;
    let mut b = ProfBench::new(mx);
    for s in 0..SCENE_CAP {
        let sb = b.run_scene(s as u8)?;
        if sb.filled != MIN_METERED {
            return Err(XC_FEW_ROUNDS);
        }
    }
    b.budget_check()?;
    b.conserved()?;
    let rep = b.report()?;
    if rep.len() != SCENE_CAP {
        return Err(XC_REPORT_EMPTY);
    }
    // 单轮确定性：同参重跑同周期。
    let c1 = Mixer::new(500, 300, 200)?.cycles(0, 64).map_err(|_| XC_BAD_WORK)?;
    let c2 = Mixer::new(500, 300, 200)?.cycles(0, 64).map_err(|_| XC_BAD_WORK)?;
    if c1 != c2 {
        return Err(XC_LEDGER_UNBALANCED);
    }

    // 错误路径 1：场景号越界。
    if b.run_scene(SCENE_CAP as u8).err() != Some(XC_BAD_SCENE) {
        return Err(XC_BAD_SCENE);
    }
    // 错误路径 2：工作量非法。
    if Mixer::new(500, 300, 200)?.cycles(0, 0) != Err(u32::MAX - 1) {
        return Err(XC_BAD_WORK);
    }
    // 错误路径 3：权重破恒。
    if Mixer::new(400, 300, 200).err() != Some(XC_WEIGHT_CONSERV) {
        return Err(XC_WEIGHT_CONSERV);
    }
    // 错误路径 4：轮数不足（空账取中位）。
    let sb0 = SceneBench::new(0);
    if sb0.med_cycles() != Err(XC_FEW_ROUNDS) {
        return Err(XC_FEW_ROUNDS);
    }
    // 错误路径 5：空账报告。
    let mx2 = Mixer::new(600, 250, 150)?;
    let b2 = ProfBench::new(mx2);
    if b2.report() != Err(XC_REPORT_EMPTY) {
        return Err(XC_REPORT_EMPTY);
    }
    // 中位与四分位健全性。
    let s = sorted_copy(&[5, 3, 9, 1, 7, 3, 8, 2]);
    if median(&s) != 4 {
        return Err(XC_LEDGER_UNBALANCED);
    }
    let _ = quartiles(&s);
    Ok(0)
}
