//! cgf15_certbench — CGPU-F0813 认证基准（全流程耗时/探测吞吐/判定延迟——入册回归）。
//!
//! 判据原文（施工书锚点 CGPU-F0813）：基准：认证全流程耗时（无人值守全程
//! 时长）；探测吞吐；判定延迟——入册回归。判据：三基准、入册、判据。
//!
//! 职责定位：三基准闭集（认证全流程耗时=无人值守全程时长红线/探测吞吐=
//! 单位时间探测数红线/判定延迟=证据齐到出判的延迟红线——三基准各自阈值
//! 显性入册）+入册回归（同口径复跑一致性——漂移即 0x7A04 立案；口径参数
//! 显性固定防挑参作弊 0x7A05——cgd13 基准纪律同源）。诊断码段
//! 0x7A01~0x7A05。

use alloc::vec::Vec;

/// 模块版本。
pub const CERT_BENCH_VERSION: &str = "F15-certbench-v1";

// ---------------------------------------------------------------------------
// 诊断码（XcCode 0x7A 段，本会话独占）
// ---------------------------------------------------------------------------

/// 0x7A01 全流程耗时红线（无人值守全程超阈）。
pub const XC_FLOW_TIME_BREACH: u32 = 0x7A01;
/// 0x7A02 探测吞吐红线（单位时间探测数低于阈）。
pub const XC_PROBE_TPUT_BREACH: u32 = 0x7A02;
/// 0x7A03 判定延迟红线。
pub const XC_VERDICT_LAT_BREACH: u32 = 0x7A03;
/// 0x7A04 入册回归漂移（同口径复跑不一致）。
pub const XC_REGRESSION_DRIFT: u32 = 0x7A04;
/// 0x7A05 口径作弊（基准参数挑选）。
pub const XC_BENCH_CHERRY_PICK: u32 = 0x7A05;

// ---------------------------------------------------------------------------
// 常量（口径显性固定——判据"入册"的阈值锚）
// ---------------------------------------------------------------------------

/// 全流程耗时红线（毫秒——无人值守认证全程）。
pub const FLOW_TIME_CAP_MS: u32 = 5_000;
/// 探测吞吐红线（探测数/秒，低于即红）。
pub const PROBE_TPUT_FLOOR: u32 = 100;
/// 判定延迟红线（微秒）。
pub const VERDICT_LAT_CAP_US: u32 = 2_000;
/// 回归样本数（复跑次数）。
pub const REGRESSION_RUNS: usize = 3;
/// 基准口径版本（参数表版本——防挑参）。
pub const BENCH_PROFILE_VER: u32 = 0x4342_0001; // "CB"1

// ---------------------------------------------------------------------------
// 一、认证全流程耗时基准
// ---------------------------------------------------------------------------

/// 全流程基准（判据"认证全流程耗时（无人值守全程时长"）。
#[derive(Debug)]
pub struct FullFlowBench {
    /// 口径版本（参数表锚——复跑必须同版本）。
    pub profile_ver: u32,
    /// 最近实测时长（毫秒）。
    pub last_ms: u32,
    /// 超红事件数（0x7A01）。
    pub breach_events: u32,
}

impl FullFlowBench {
    /// 新基准（口径版本锚定）。
    pub const fn new() -> FullFlowBench {
        FullFlowBench {
            profile_ver: BENCH_PROFILE_VER,
            last_ms: 0,
            breach_events: 0,
        }
    }

    /// 跑一轮（实测时长入账+红线裁决）。
    pub fn run(&mut self, elapsed_ms: u32) -> Result<(), u32> {
        if self.profile_ver != BENCH_PROFILE_VER {
            // 口径版本被换=挑参作弊 0x7A05。
            self.breach_events += 1;
            return Err(XC_BENCH_CHERRY_PICK);
        }
        self.last_ms = elapsed_ms;
        if elapsed_ms > FLOW_TIME_CAP_MS {
            self.breach_events += 1;
            return Err(XC_FLOW_TIME_BREACH);
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// 二、探测吞吐基准
// ---------------------------------------------------------------------------

/// 探测吞吐基准（判据"探测吞吐"）。
#[derive(Debug)]
pub struct ProbeThroughputBench {
    /// 口径版本。
    pub profile_ver: u32,
    /// 最近实测吞吐（探测数/秒）。
    pub last_tput: u32,
    /// 超红事件数（0x7A02）。
    pub breach_events: u32,
}

impl ProbeThroughputBench {
    /// 新基准。
    pub const fn new() -> ProbeThroughputBench {
        ProbeThroughputBench {
            profile_ver: BENCH_PROFILE_VER,
            last_tput: 0,
            breach_events: 0,
        }
    }

    /// 跑一轮（probes 为探测数，elapsed_s 为秒——吞吐=probes/elapsed_s，
    /// 整数化×1000 口径：probes*1000/elapsed_ms）。
    pub fn run(&mut self, probes: u32, elapsed_ms: u32) -> Result<u32, u32> {
        if self.profile_ver != BENCH_PROFILE_VER {
            self.breach_events += 1;
            return Err(XC_BENCH_CHERRY_PICK);
        }
        if elapsed_ms == 0 {
            self.breach_events += 1;
            return Err(XC_PROBE_TPUT_BREACH);
        }
        let tput = (probes as u64 * 1000 / elapsed_ms as u64) as u32;
        self.last_tput = tput;
        if tput < PROBE_TPUT_FLOOR {
            self.breach_events += 1;
            return Err(XC_PROBE_TPUT_BREACH);
        }
        Ok(tput)
    }
}

// ---------------------------------------------------------------------------
// 三、判定延迟基准
// ---------------------------------------------------------------------------

/// 判定延迟基准（判据"判定延迟"）。
#[derive(Debug)]
pub struct VerdictLatencyBench {
    /// 口径版本。
    pub profile_ver: u32,
    /// 最近实测延迟（微秒）。
    pub last_us: u32,
    /// 超红事件数（0x7A03）。
    pub breach_events: u32,
}

impl VerdictLatencyBench {
    /// 新基准。
    pub const fn new() -> VerdictLatencyBench {
        VerdictLatencyBench {
            profile_ver: BENCH_PROFILE_VER,
            last_us: 0,
            breach_events: 0,
        }
    }

    /// 跑一轮（证据齐→出判的延迟）。
    pub fn run(&mut self, latency_us: u32) -> Result<(), u32> {
        if self.profile_ver != BENCH_PROFILE_VER {
            self.breach_events += 1;
            return Err(XC_BENCH_CHERRY_PICK);
        }
        self.last_us = latency_us;
        if latency_us > VERDICT_LAT_CAP_US {
            self.breach_events += 1;
            return Err(XC_VERDICT_LAT_BREACH);
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// 四、入册回归（同口径复跑一致性）
// ---------------------------------------------------------------------------

/// 回归账（判据"入册回归"——同口径复跑漂移即立案）。
#[derive(Debug)]
pub struct RegressionLedger {
    /// 口径版本。
    pub profile_ver: u32,
    /// 复跑样本（三个基准各自最近样本序列）。
    pub samples_flow: Vec<u32>,
    pub samples_tput: Vec<u32>,
    pub samples_verdict: Vec<u32>,
    /// 漂移事件数（0x7A04）。
    pub drift_events: u32,
    /// 作弊事件数（0x7A05）。
    pub cheat_events: u32,
}

impl RegressionLedger {
    /// 新账。
    pub const fn new() -> RegressionLedger {
        RegressionLedger {
            profile_ver: BENCH_PROFILE_VER,
            samples_flow: Vec::new(),
            samples_tput: Vec::new(),
            samples_verdict: Vec::new(),
            drift_events: 0,
            cheat_events: 0,
        }
    }

    /// 入册一样本（口径版本失配=作弊 0x7A05）。
    pub fn enroll(&mut self, slot: usize, val: u32) -> Result<(), u32> {
        if self.profile_ver != BENCH_PROFILE_VER {
            self.cheat_events += 1;
            return Err(XC_BENCH_CHERRY_PICK);
        }
        match slot {
            0 => self.samples_flow.push(val),
            1 => self.samples_tput.push(val),
            2 => self.samples_verdict.push(val),
            _ => {
                self.cheat_events += 1;
                return Err(XC_BENCH_CHERRY_PICK);
            }
        }
        Ok(())
    }

    /// 回归裁决：样本数达 REGRESSION_RUNS 后，末位与首位漂移超 10% 即立案
    /// （0x7A04——吞吐向上漂同立案：口径不稳比慢更危险）。
    pub fn adjudicate(&mut self, slot: usize) -> Result<(), u32> {
        let (n, first, last) = match slot {
            0 => (self.samples_flow.len(), self.samples_flow.first().copied(), self.samples_flow.last().copied()),
            1 => (self.samples_tput.len(), self.samples_tput.first().copied(), self.samples_tput.last().copied()),
            2 => (self.samples_verdict.len(), self.samples_verdict.first().copied(), self.samples_verdict.last().copied()),
            _ => {
                self.cheat_events += 1;
                return Err(XC_BENCH_CHERRY_PICK);
            }
        };
        if n < REGRESSION_RUNS {
            return Ok(()); // 样本不足不裁决（半册出判=挑样本）
        }
        match (first, last) {
            (Some(f), Some(l)) => {
                if f == 0 {
                    if l != 0 {
                        self.drift_events += 1;
                        return Err(XC_REGRESSION_DRIFT);
                    }
                    return Ok(());
                }
                let diff = if l > f { l - f } else { f - l };
                // 漂移 >10%：diff*10 > f。
                if diff * 10 > f {
                    self.drift_events += 1;
                    return Err(XC_REGRESSION_DRIFT);
                }
                Ok(())
            }
            _ => {
                self.drift_events += 1;
                Err(XC_REGRESSION_DRIFT)
            }
        }
    }

    /// 口径显性：版本锚+三红线常量合成口径指纹（复跑对拍锚）。
    pub const fn profile_fingerprint() -> u64 {
        (BENCH_PROFILE_VER as u64) << 24
            | (FLOW_TIME_CAP_MS as u64) << 12
            | (PROBE_TPUT_FLOOR as u64) << 6
            | (VERDICT_LAT_CAP_US as u64 & 0x3F)
    }
}

// ---------------------------------------------------------------------------
// 五、总引擎 + 自检（三基准/回归/判据三组+段对账）
// ---------------------------------------------------------------------------

/// 认证基准总引擎。
#[derive(Debug)]
pub struct CertBenchEngine {
    /// 全流程基准。
    pub flow: FullFlowBench,
    /// 探测吞吐基准。
    pub tput: ProbeThroughputBench,
    /// 判定延迟基准。
    pub verdict: VerdictLatencyBench,
    /// 回归账。
    pub reg: RegressionLedger,
}

impl CertBenchEngine {
    /// 出厂。
    pub const fn new() -> CertBenchEngine {
        CertBenchEngine {
            flow: FullFlowBench::new(),
            tput: ProbeThroughputBench::new(),
            verdict: VerdictLatencyBench::new(),
            reg: RegressionLedger::new(),
        }
    }

    /// 组一：三基准——三红线逐个验证；过线显性。
    pub fn selfcheck_benches(&mut self) -> Result<(), u32> {
        let mut e = CertBenchEngine::new();
        // 全流程：4999ms 过/5001ms 红。
        e.flow.run(4_999)?;
        if e.flow.run(5_001) != Err(XC_FLOW_TIME_BREACH) {
            return Err(XC_FLOW_TIME_BREACH);
        }
        // 吞吐：500 探测/4ms=125000？错——口径 probes*1000/elapsed_ms=125000
        // 太大；500/5000ms=100 恰红线。用 500 探测 5000ms → 100 过。
        e.tput.run(500, 5_000)?;
        if e.tput.run(400, 5_000) != Err(XC_PROBE_TPUT_BREACH) {
            return Err(XC_PROBE_TPUT_BREACH); // 80 < 100 红
        }
        // 判定延迟：1999µs 过/2001µs 红。
        e.verdict.run(1_999)?;
        if e.verdict.run(2_001) != Err(XC_VERDICT_LAT_BREACH) {
            return Err(XC_VERDICT_LAT_BREACH);
        }
        // 超标账面。
        if e.flow.breach_events != 1 || e.tput.breach_events != 1 || e.verdict.breach_events != 1 {
            return Err(XC_FLOW_TIME_BREACH);
        }
        Ok(())
    }

    /// 组二：口径防作弊——换版本锚即 0x7A05；槽位越界 0x7A05。
    pub fn selfcheck_antipick(&mut self) -> Result<(), u32> {
        let mut e = CertBenchEngine::new();
        // 换口径版本→作弊。
        e.flow.profile_ver = 0xDEAD;
        if e.flow.run(100) != Err(XC_BENCH_CHERRY_PICK) {
            return Err(XC_BENCH_CHERRY_PICK);
        }
        // 回归槽越界→作弊。
        if e.reg.enroll(9, 1) != Err(XC_BENCH_CHERRY_PICK) {
            return Err(XC_BENCH_CHERRY_PICK);
        }
        // 换回归口径版本→作弊。
        e.reg.profile_ver = 0xBEEF;
        if e.reg.enroll(0, 1) != Err(XC_BENCH_CHERRY_PICK) {
            return Err(XC_BENCH_CHERRY_PICK);
        }
        if e.reg.cheat_events != 2 {
            return Err(XC_BENCH_CHERRY_PICK);
        }
        // 口径指纹稳定（两次合成一致——const 面无时钟依赖）。
        if RegressionLedger::profile_fingerprint() != RegressionLedger::profile_fingerprint() {
            return Err(XC_BENCH_CHERRY_PICK);
        }
        Ok(())
    }

    /// 组三：回归——三样本复跑一致过；漂移立案；样本不足不裁决。
    pub fn selfcheck_regression(&mut self) -> Result<(), u32> {
        let mut e = CertBenchEngine::new();
        // 样本不足→不裁决。
        e.reg.enroll(0, 4_000)?;
        e.reg.adjudicate(0)?;
        // 三样本 4000/4050/4100（漂移 2.5% <10%）→过。
        e.reg.enroll(0, 4_000)?;
        e.reg.enroll(0, 4_050)?;
        e.reg.enroll(0, 4_100)?;
        e.reg.adjudicate(0)?;
        // 漂移案例：4100 → 5000（22% >10%）→0x7A04。
        e.reg.enroll(0, 4_100)?;
        e.reg.enroll(0, 4_100)?;
        e.reg.enroll(0, 5_000)?;
        if e.reg.adjudicate(0) != Err(XC_REGRESSION_DRIFT) {
            return Err(XC_REGRESSION_DRIFT);
        }
        if e.reg.drift_events != 1 {
            return Err(XC_REGRESSION_DRIFT);
        }
        Ok(())
    }

    /// 组四：判据——端到端：三基准全绿+回归三册齐绿。
    pub fn selfcheck_verdict(&mut self) -> Result<(), u32> {
        let mut e = CertBenchEngine::new();
        // 三基准各跑 REGRESSION_RUNS 轮全绿入册。
        let mut i = 0u32;
        while i < REGRESSION_RUNS as u32 {
            e.flow.run(4_000 + i)?;
            e.reg.enroll(0, e.flow.last_ms)?;
            let t = e.tput.run(500, 5_000)?;
            e.reg.enroll(1, t)?;
            e.verdict.run(1_500 + i)?;
            e.reg.enroll(2, e.verdict.last_us)?;
            i += 1;
        }
        e.reg.adjudicate(0)?;
        e.reg.adjudicate(1)?;
        e.reg.adjudicate(2)?;
        // 全链零事件。
        if e.flow.breach_events != 0
            || e.tput.breach_events != 0
            || e.verdict.breach_events != 0
            || e.reg.drift_events != 0
            || e.reg.cheat_events != 0
        {
            return Err(XC_FLOW_TIME_BREACH);
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

    /// 段对账：五码全落 0x7A00..0x7B00 开区间且互异。
    pub const fn segment_guard() -> Result<(), u32> {
        let codes = [
            XC_FLOW_TIME_BREACH,
            XC_PROBE_TPUT_BREACH,
            XC_VERDICT_LAT_BREACH,
            XC_REGRESSION_DRIFT,
            XC_BENCH_CHERRY_PICK,
        ];
        let mut i = 0;
        while i < codes.len() {
            if codes[i] < 0x7A00 || codes[i] >= 0x7B00 {
                return Err(XC_FLOW_TIME_BREACH);
            }
            let mut j = i + 1;
            while j < codes.len() {
                if codes[i] == codes[j] {
                    return Err(XC_FLOW_TIME_BREACH);
                }
                j += 1;
            }
            i += 1;
        }
        Ok(())
    }
}
