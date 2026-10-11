//! cgi11_bwbench — CGPU-F1290 带宽基准（有效带宽/仲裁开销/批合并收益——入册回归）。
//!
//! 判据原文（施工书锚点 CGPU-F1290）：基准：有效带宽实测（各代 PCIe 实测
//! vs 理论）；仲裁开销；批合并收益——入册回归。判据：三基准、入册、判据。
//!
//! 职责定位：三基准闭集（有效带宽=各代 PCIe 实测对理论比红线——理论值用
//! 代际闭集常量不估计；仲裁开销=仲裁步折算耗时红线；批合并收益=批传输对
//! 单发的节时比红线——无收益/负收益显性）+入册回归（同口径复跑漂移立案
//! 0x7B04；口径版本防挑参 0x7B05——cgd13/cgf15 基准纪律同源）。诊断码段
//! 0x7B01~0x7B05。

use alloc::vec::Vec;

/// 模块版本。
pub const BW_BENCH_VERSION: &str = "I11-bwbench-v1";

// ---------------------------------------------------------------------------
// 诊断码（XcCode 0x7B 段，本会话独占）
// ---------------------------------------------------------------------------

/// 0x7B01 有效带宽红线（实测/理论比低于阈）。
pub const XC_EFF_BW_BREACH: u32 = 0x7B01;
/// 0x7B02 仲裁开销红线。
pub const XC_ARBITER_OVH_BREACH: u32 = 0x7B02;
/// 0x7B03 批合并收益红线（无收益/负收益）。
pub const XC_MERGE_GAIN_BREACH: u32 = 0x7B03;
/// 0x7B04 回归漂移。
pub const XC_BW_REG_DRIFT: u32 = 0x7B04;
/// 0x7B05 口径作弊。
pub const XC_BW_CHERRY_PICK: u32 = 0x7B05;

// ---------------------------------------------------------------------------
// 常量（口径显性）
// ---------------------------------------------------------------------------

/// PCIe 代际闭集（Gen1~Gen5 理论带宽 GB/s——cgi 域闭集常量不估计）。
pub const PCIE_THEORY_GBPS: [u32; 5] = [4, 8, 16, 32, 64];
/// 有效带宽红线：实测/理论比下限（万分比 7500=75%）。
pub const EFF_BW_FLOOR_BP: u32 = 7_500;
/// 仲裁开销红线（每次仲裁 µs 上限）。
pub const ARBITER_OVH_CAP_US: u32 = 50;
/// 批合并收益红线：节时比下限（万分比 2000=20%——低于即白合并）。
pub const MERGE_GAIN_FLOOR_BP: u32 = 2_000;
/// 回归样本数。
pub const BW_REG_RUNS: usize = 3;
/// 基准口径版本。
pub const BW_BENCH_PROFILE_VER: u32 = 0x4242_0001; // "BB"1

// ---------------------------------------------------------------------------
// 一、有效带宽基准（各代 PCIe 实测 vs 理论）
// ---------------------------------------------------------------------------

/// 有效带宽基准（判据"各代 PCIe 实测 vs 理论"）。
#[derive(Debug)]
pub struct EffectiveBwBench {
    /// 口径版本。
    pub profile_ver: u32,
    /// 最近实测比（万分比）。
    pub last_ratio_bp: u32,
    /// 超红事件数（0x7B01）。
    pub breach_events: u32,
}

impl EffectiveBwBench {
    /// 新基准。
    pub const fn new() -> EffectiveBwBench {
        EffectiveBwBench {
            profile_ver: BW_BENCH_PROFILE_VER,
            last_ratio_bp: 0,
            breach_events: 0,
        }
    }

    /// 跑一轮：gen 为代际（0..=4），measured_gbps 为实测。
    pub fn run(&mut self, gen: usize, measured_gbps: u32) -> Result<u32, u32> {
        if self.profile_ver != BW_BENCH_PROFILE_VER {
            self.breach_events += 1;
            return Err(XC_BW_CHERRY_PICK);
        }
        if gen >= PCIE_THEORY_GBPS.len() {
            self.breach_events += 1;
            return Err(XC_EFF_BW_BREACH);
        }
        let theory = PCIE_THEORY_GBPS[gen];
        if theory == 0 {
            self.breach_events += 1;
            return Err(XC_EFF_BW_BREACH);
        }
        let ratio = (measured_gbps as u64 * 10_000 / theory as u64) as u32;
        self.last_ratio_bp = ratio;
        if ratio < EFF_BW_FLOOR_BP {
            self.breach_events += 1;
            return Err(XC_EFF_BW_BREACH);
        }
        Ok(ratio)
    }
}

// ---------------------------------------------------------------------------
// 二、仲裁开销基准
// ---------------------------------------------------------------------------

/// 仲裁开销基准（判据"仲裁开销"）。
#[derive(Debug)]
pub struct ArbiterOverheadBench {
    /// 口径版本。
    pub profile_ver: u32,
    /// 最近实测开销（µs）。
    pub last_us: u32,
    /// 超红事件数（0x7B02）。
    pub breach_events: u32,
}

impl ArbiterOverheadBench {
    /// 新基准。
    pub const fn new() -> ArbiterOverheadBench {
        ArbiterOverheadBench {
            profile_ver: BW_BENCH_PROFILE_VER,
            last_us: 0,
            breach_events: 0,
        }
    }

    /// 跑一轮：rounds 为仲裁轮数，elapsed_us 为总耗时（开销=总耗时/轮数）。
    pub fn run(&mut self, rounds: u32, elapsed_us: u32) -> Result<u32, u32> {
        if self.profile_ver != BW_BENCH_PROFILE_VER {
            self.breach_events += 1;
            return Err(XC_BW_CHERRY_PICK);
        }
        if rounds == 0 {
            self.breach_events += 1;
            return Err(XC_ARBITER_OVH_BREACH);
        }
        let per = elapsed_us / rounds;
        self.last_us = per;
        if per > ARBITER_OVH_CAP_US {
            self.breach_events += 1;
            return Err(XC_ARBITER_OVH_BREACH);
        }
        Ok(per)
    }
}

// ---------------------------------------------------------------------------
// 三、批合并收益基准
// ---------------------------------------------------------------------------

/// 批合并收益基准（判据"批合并收益"）。
#[derive(Debug)]
pub struct MergeGainBench {
    /// 口径版本。
    pub profile_ver: u32,
    /// 最近收益比（万分比）。
    pub last_gain_bp: u32,
    /// 超红事件数（0x7B03）。
    pub breach_events: u32,
}

impl MergeGainBench {
    /// 新基准。
    pub const fn new() -> MergeGainBench {
        MergeGainBench {
            profile_ver: BW_BENCH_PROFILE_VER,
            last_gain_bp: 0,
            breach_events: 0,
        }
    }

    /// 跑一轮：solo_us 为逐单发总耗时，batch_us 为批发总耗时。
    /// 收益比=(solo-batch)*10000/solo（batch>solo=负收益即红）。
    pub fn run(&mut self, solo_us: u32, batch_us: u32) -> Result<u32, u32> {
        if self.profile_ver != BW_BENCH_PROFILE_VER {
            self.breach_events += 1;
            return Err(XC_BW_CHERRY_PICK);
        }
        if solo_us == 0 {
            self.breach_events += 1;
            return Err(XC_MERGE_GAIN_BREACH);
        }
        if batch_us >= solo_us {
            self.breach_events += 1;
            return Err(XC_MERGE_GAIN_BREACH); // 负收益白合并
        }
        let gain = ((solo_us - batch_us) as u64 * 10_000 / solo_us as u64) as u32;
        self.last_gain_bp = gain;
        if gain < MERGE_GAIN_FLOOR_BP {
            self.breach_events += 1;
            return Err(XC_MERGE_GAIN_BREACH);
        }
        Ok(gain)
    }
}

// ---------------------------------------------------------------------------
// 四、入册回归
// ---------------------------------------------------------------------------

/// 回归账（判据"入册回归"）。
#[derive(Debug)]
pub struct BwRegression {
    /// 口径版本。
    pub profile_ver: u32,
    /// 三基准样本册。
    pub samples_bw: Vec<u32>,
    pub samples_ovh: Vec<u32>,
    pub samples_gain: Vec<u32>,
    /// 漂移事件数（0x7B04）。
    pub drift_events: u32,
    /// 作弊事件数（0x7B05）。
    pub cheat_events: u32,
}

impl BwRegression {
    /// 新账。
    pub const fn new() -> BwRegression {
        BwRegression {
            profile_ver: BW_BENCH_PROFILE_VER,
            samples_bw: Vec::new(),
            samples_ovh: Vec::new(),
            samples_gain: Vec::new(),
            drift_events: 0,
            cheat_events: 0,
        }
    }

    /// 入册（口径失配=作弊）。
    pub fn enroll(&mut self, slot: usize, val: u32) -> Result<(), u32> {
        if self.profile_ver != BW_BENCH_PROFILE_VER {
            self.cheat_events += 1;
            return Err(XC_BW_CHERRY_PICK);
        }
        match slot {
            0 => self.samples_bw.push(val),
            1 => self.samples_ovh.push(val),
            2 => self.samples_gain.push(val),
            _ => {
                self.cheat_events += 1;
                return Err(XC_BW_CHERRY_PICK);
            }
        }
        Ok(())
    }

    /// 回归裁决（漂移 >10% 立案——与 cgf15 同口径）。
    pub fn adjudicate(&mut self, slot: usize) -> Result<(), u32> {
        let (n, first, last) = match slot {
            0 => (self.samples_bw.len(), self.samples_bw.first().copied(), self.samples_bw.last().copied()),
            1 => (self.samples_ovh.len(), self.samples_ovh.first().copied(), self.samples_ovh.last().copied()),
            2 => (self.samples_gain.len(), self.samples_gain.first().copied(), self.samples_gain.last().copied()),
            _ => {
                self.cheat_events += 1;
                return Err(XC_BW_CHERRY_PICK);
            }
        };
        if n < BW_REG_RUNS {
            return Ok(());
        }
        match (first, last) {
            (Some(f), Some(l)) => {
                if f == 0 {
                    if l != 0 {
                        self.drift_events += 1;
                        return Err(XC_BW_REG_DRIFT);
                    }
                    return Ok(());
                }
                let diff = if l > f { l - f } else { f - l };
                if diff * 10 > f {
                    self.drift_events += 1;
                    return Err(XC_BW_REG_DRIFT);
                }
                Ok(())
            }
            _ => {
                self.drift_events += 1;
                Err(XC_BW_REG_DRIFT)
            }
        }
    }
}

// ---------------------------------------------------------------------------
// 五、总引擎 + 自检（三基准/回归/判据三组+段对账）
// ---------------------------------------------------------------------------

/// 带宽基准总引擎。
#[derive(Debug)]
pub struct BwBenchEngine {
    /// 有效带宽基准。
    pub bw: EffectiveBwBench,
    /// 仲裁开销基准。
    pub ovh: ArbiterOverheadBench,
    /// 批合并收益基准。
    pub gain: MergeGainBench,
    /// 回归账。
    pub reg: BwRegression,
}

impl BwBenchEngine {
    /// 出厂。
    pub const fn new() -> BwBenchEngine {
        BwBenchEngine {
            bw: EffectiveBwBench::new(),
            ovh: ArbiterOverheadBench::new(),
            gain: MergeGainBench::new(),
            reg: BwRegression::new(),
        }
    }

    /// 组一：三基准——三红线逐个验证。
    pub fn selfcheck_benches(&mut self) -> Result<(), u32> {
        let mut e = BwBenchEngine::new();
        // Gen4 理论 32：实测 26 → 8125bp 过；实测 20 → 6250bp 红。
        e.bw.run(3, 26)?;
        if e.bw.run(3, 20) != Err(XC_EFF_BW_BREACH) {
            return Err(XC_EFF_BW_BREACH);
        }
        // 越界代际→红。
        if e.bw.run(9, 26) != Err(XC_EFF_BW_BREACH) {
            return Err(XC_EFF_BW_BREACH);
        }
        // 仲裁：400 轮 12000µs → 30µs/轮 过；400 轮 24000µs → 60µs 红。
        e.ovh.run(400, 12_000)?;
        if e.ovh.run(400, 24_000) != Err(XC_ARBITER_OVH_BREACH) {
            return Err(XC_ARBITER_OVH_BREACH);
        }
        // 批合并：solo 1000µs batch 700µs → 3000bp 过；batch 900µs → 1000bp 红。
        e.gain.run(1_000, 700)?;
        if e.gain.run(1_000, 900) != Err(XC_MERGE_GAIN_BREACH) {
            return Err(XC_MERGE_GAIN_BREACH);
        }
        // 负收益：batch 1100 > solo 1000 → 红。
        if e.gain.run(1_000, 1_100) != Err(XC_MERGE_GAIN_BREACH) {
            return Err(XC_MERGE_GAIN_BREACH);
        }
        // 超标账面。
        if e.bw.breach_events != 2 || e.ovh.breach_events != 1 || e.gain.breach_events != 2 {
            return Err(XC_EFF_BW_BREACH);
        }
        Ok(())
    }

    /// 组二：口径防作弊。
    pub fn selfcheck_antipick(&mut self) -> Result<(), u32> {
        let mut e = BwBenchEngine::new();
        e.bw.profile_ver = 0xDEAD;
        if e.bw.run(3, 26) != Err(XC_BW_CHERRY_PICK) {
            return Err(XC_BW_CHERRY_PICK);
        }
        if e.reg.enroll(9, 1) != Err(XC_BW_CHERRY_PICK) {
            return Err(XC_BW_CHERRY_PICK);
        }
        e.reg.profile_ver = 0xBEEF;
        if e.reg.enroll(0, 1) != Err(XC_BW_CHERRY_PICK) {
            return Err(XC_BW_CHERRY_PICK);
        }
        if e.reg.cheat_events != 2 {
            return Err(XC_BW_CHERRY_PICK);
        }
        Ok(())
    }

    /// 组三：回归——一致过/漂移立案。
    pub fn selfcheck_regression(&mut self) -> Result<(), u32> {
        let mut e = BwBenchEngine::new();
        e.reg.enroll(0, 8_000)?;
        e.reg.adjudicate(0)?;
        e.reg.enroll(0, 8_000)?;
        e.reg.enroll(0, 8_100)?;
        e.reg.enroll(0, 8_200)?;
        e.reg.adjudicate(0)?;
        // 漂移：8200 → 9500（16%）→0x7B04。
        e.reg.enroll(0, 8_200)?;
        e.reg.enroll(0, 8_200)?;
        e.reg.enroll(0, 9_500)?;
        if e.reg.adjudicate(0) != Err(XC_BW_REG_DRIFT) {
            return Err(XC_BW_REG_DRIFT);
        }
        if e.reg.drift_events != 1 {
            return Err(XC_BW_REG_DRIFT);
        }
        Ok(())
    }

    /// 组四：判据——端到端：五代全测+三册齐绿。
    pub fn selfcheck_verdict(&mut self) -> Result<(), u32> {
        let mut e = BwBenchEngine::new();
        // 五代逐代实测（理论 75%+）：4→3.2(取整32? 用万分比口径 3GB/s=7500bp)。
        let measured = [3u32, 6, 12, 26, 52];
        let mut g = 0;
        while g < PCIE_THEORY_GBPS.len() {
            let r = e.bw.run(g, measured[g])?;
            e.reg.enroll(0, r)?;
            g += 1;
        }
        let mut i = 0;
        while i < BW_REG_RUNS {
            let o = e.ovh.run(400, 12_000)?;
            e.reg.enroll(1, o)?;
            let gn = e.gain.run(1_000, 700)?;
            e.reg.enroll(2, gn)?;
            i += 1;
        }
        e.reg.adjudicate(0)?;
        e.reg.adjudicate(1)?;
        e.reg.adjudicate(2)?;
        // 全链零事件。
        if e.bw.breach_events != 0
            || e.ovh.breach_events != 0
            || e.gain.breach_events != 0
            || e.reg.drift_events != 0
            || e.reg.cheat_events != 0
        {
            return Err(XC_EFF_BW_BREACH);
        }
        Ok(())
    }

    /// 全量自检入口（四组+段对账）。
    pub fn selfcheck_all(&mut self) -> Result<(), u32> {
        self.selfcheck_benches()?;
        self.selfcheck_antipick()?;
        self.selfcheck_regression()?;
        self.selfcheck_verdict()?;
        Self::segment_guard()?;
        Ok(())
    }

    /// 段对账：五码全落 0x7B00..0x7C00 开区间且互异。
    pub const fn segment_guard() -> Result<(), u32> {
        let codes = [
            XC_EFF_BW_BREACH,
            XC_ARBITER_OVH_BREACH,
            XC_MERGE_GAIN_BREACH,
            XC_BW_REG_DRIFT,
            XC_BW_CHERRY_PICK,
        ];
        let mut i = 0;
        while i < codes.len() {
            if codes[i] < 0x7B00 || codes[i] >= 0x7C00 {
                return Err(XC_EFF_BW_BREACH);
            }
            let mut j = i + 1;
            while j < codes.len() {
                if codes[i] == codes[j] {
                    return Err(XC_EFF_BW_BREACH);
                }
                j += 1;
            }
            i += 1;
        }
        Ok(())
    }
}
