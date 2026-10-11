//! vck12_hetsim — CGPU-F1612 异构模拟器（统一框架+两态校准诚实+用途表+回放+覆盖）。
//!
//! 判据原文（施工书锚点 CGPU-F1612）：模拟器：模拟器架构（各引擎模拟器
//! （NPU 模拟（批处理延迟模型/吞吐模型——DSP 模拟（实时调度模拟——逐引擎
//! 模拟器——统一模拟框架（F1454 扩展——扩展声明；校准（真机数据校准（有
//! 真机时校准/无真机用文档典型值（典型值显性标注——校准两态诚实；用途
//! （CI 全模拟/调度策略验证/回退演练/能效路由验证——用途表；回放（真机
//! trace 回放——回放复用 F1454；行为覆盖（模拟器行为覆盖表（正常/满载/
//! 失败/延迟抖动——覆盖清单；测试（框架/两态校准/用途/回放/覆盖五组）。
//! 判据：统一框架、两态诚实、用途表、回放、覆盖、五组、判据。
//!
//! 职责定位：统一模拟框架（逐引擎模拟器 NPU 批处理延迟/吞吐模型+DSP 实时
//! 调度模拟挂同一框架——F1454 扩展声明在册，游离引擎模拟器 0x7F01 拒）+
//! 校准两态诚实（有真机=真机数据校准、无真机=文档典型值校准且典型值显性
//! 标注——两态混标 0x7F02，冒充真机数据是最恶劣的造假）+用途表（CI 全模
//! 拟/调度策略验证/回退演练/能效路由验证四用途闭集——表外用途 0x7F03）+
//! 回放（真机 trace 回放结果与录制一致——漂移 0x7F04，F1454 复用声明）+
//! 行为覆盖（正常/满载/失败/延迟抖动四类场景覆盖清单——缺角 0x7F05）。诊
//! 断码段 0x7F01~0x7F05。

use alloc::vec::Vec;

/// 模块版本。
pub const HET_SIM_VERSION: &str = "K12-hetsim-v1";

// ---------------------------------------------------------------------------
// 诊断码（XcCode 0x7F 段，本会话独占）
// ---------------------------------------------------------------------------

/// 0x7F01 框架失统（引擎模拟器游离于统一框架外）。
pub const XC_FRAME_ORPHAN: u32 = 0x7F01;
/// 0x7F02 校准失诚（真机/典型值两态混标）。
pub const XC_CALIB_DISHONEST: u32 = 0x7F02;
/// 0x7F03 用途越表（模拟器用于用途表外场景）。
pub const XC_PURPOSE_OOB: u32 = 0x7F03;
/// 0x7F04 回放失真（trace 回放结果漂移）。
pub const XC_REPLAY_DRIFT: u32 = 0x7F04;
/// 0x7F05 覆盖缺角（行为覆盖表四类缺一）。
pub const XC_COVERAGE_HOLE: u32 = 0x7F05;

// ---------------------------------------------------------------------------
// 常量
// ---------------------------------------------------------------------------

/// 引擎模拟器数（NPU/DSP 两族闭集——GPU/CPU 有真机路径不走模拟）。
pub const SIM_ENGINE_COUNT: usize = 2;
/// 用途表容量（四用途闭集）。
pub const PURPOSE_COUNT: usize = 4;
/// 行为覆盖类数（正常/满载/失败/延迟抖动）。
pub const COVERAGE_CLASSES: usize = 4;
/// F1454 扩展声明锚。
pub const F1454_EXT_ANCHOR: u32 = 0x1454;

// ---------------------------------------------------------------------------
// 一、统一模拟框架（F1454 扩展）
// ---------------------------------------------------------------------------

/// 引擎模拟器行为输出。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct SimOut {
    /// 引擎档（0=Low/DSP 1=Mid/NPU）。
    pub cls: usize,
    /// 模拟延迟（µs）。
    pub latency_us: u32,
    /// 模拟吞吐（任务/秒）。
    pub tput: u32,
}

/// 统一模拟框架（判据"统一模拟框架（F1454 扩展——扩展声明"）。
#[derive(Debug)]
pub struct UnifiedSimFrame {
    /// 挂载的引擎模拟器位图（bit0=DSP bit1=NPU）。
    pub mounted: u32,
    /// F1454 扩展声明在册。
    pub ext_declared: bool,
    /// 游离事件数（0x7F01）。
    pub orphan_events: u32,
}

impl UnifiedSimFrame {
    /// 新框架（扩展声明出厂即在册）。
    pub const fn new() -> UnifiedSimFrame {
        UnifiedSimFrame {
            mounted: 0,
            ext_declared: true,
            orphan_events: 0,
        }
    }

    /// 挂载引擎模拟器。
    pub fn mount(&mut self, cls: usize) -> Result<(), u32> {
        if cls >= SIM_ENGINE_COUNT {
            self.orphan_events += 1;
            return Err(XC_FRAME_ORPHAN);
        }
        self.mounted |= 1 << cls;
        Ok(())
    }

    /// 完备裁决：两族模拟器全挂载+扩展声明在册（游离 0x7F01）。
    pub fn complete(&mut self) -> Result<(), u32> {
        if !self.ext_declared || self.mounted != 0b11 {
            self.orphan_events += 1;
            return Err(XC_FRAME_ORPHAN);
        }
        Ok(())
    }

    /// NPU 模拟：批处理延迟模型（批大小×单批延迟线性）+吞吐模型。
    pub fn simulate_npu(&self, batch: u32) -> SimOut {
        let latency = 200 + batch * 50; // 批处理延迟模型（µs）
        let tput = if batch == 0 { 0 } else { 1_000_000 / (50 + batch * 10) };
        SimOut { cls: 1, latency_us: latency, tput }
    }

    /// DSP 模拟：实时调度模拟（任务按截止期排序——EDF 语义简化：延迟=剩余截止余量）。
    pub fn simulate_dsp(&self, tasks: &[u32]) -> SimOut {
        // tasks 为各任务截止期（µs）——取最小截止期为延迟上界模拟。
        let mut min_dl = u32::MAX;
        let mut i = 0;
        while i < tasks.len() {
            if tasks[i] < min_dl {
                min_dl = tasks[i];
            }
            i += 1;
        }
        let latency = if tasks.is_empty() { 0 } else { min_dl / 2 };
        SimOut { cls: 0, latency_us: latency, tput: tasks.len() as u32 * 1_000 }
    }
}

// ---------------------------------------------------------------------------
// 二、校准两态诚实
// ---------------------------------------------------------------------------

/// 校准状态（两态闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CalibState {
    /// 真机数据校准（有真机）。
    RealMachine,
    /// 文档典型值校准（无真机——典型值显性标注）。
    DocTypical,
}

/// 校准记录。
#[derive(Clone, Copy, Debug)]
pub struct CalibRecord {
    /// 引擎档。
    pub cls: usize,
    /// 校准态。
    pub state: CalibState,
    /// 校准值（µs）。
    pub value_us: u32,
    /// 典型值标注（DocTypical 态必须 true——显性标注）。
    pub typical_annotated: bool,
}

/// 校准簿（判据"校准两态诚实"）。
#[derive(Debug)]
pub struct CalibBook {
    /// 校准记录。
    pub records: Vec<CalibRecord>,
    /// 失诚事件数（0x7F02）。
    pub dishonest_events: u32,
}

impl CalibBook {
    /// 新簿。
    pub const fn new() -> CalibBook {
        CalibBook {
            records: Vec::new(),
            dishonest_events: 0,
        }
    }

    /// 入账（两态纪律：DocTypical 必须带典型值标注；RealMachine 不得标典型）。
    pub fn enroll(&mut self, r: CalibRecord) -> Result<(), u32> {
        let ok = match r.state {
            CalibState::RealMachine => !r.typical_annotated, // 真机数据不是典型值
            CalibState::DocTypical => r.typical_annotated,   // 典型值必须显性标注
        };
        if !ok {
            self.dishonest_events += 1;
            return Err(XC_CALIB_DISHONEST);
        }
        self.records.push(r);
        Ok(())
    }

    /// 查询带态（消费方永远知道数据来源态）。
    pub fn query(&self, cls: usize) -> Option<(CalibState, u32)> {
        let mut i = 0;
        while i < self.records.len() {
            if self.records[i].cls == cls {
                return Some((self.records[i].state, self.records[i].value_us));
            }
            i += 1;
        }
        None
    }
}

// ---------------------------------------------------------------------------
// 三、用途表（四用途闭集）
// ---------------------------------------------------------------------------

/// 模拟器用途（判据"用途表"四闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SimPurpose {
    /// CI 全模拟。
    CiFull,
    /// 调度策略验证。
    SchedVerify,
    /// 回退演练。
    FallbackDrill,
    /// 能效路由验证。
    EfficacyVerify,
}

impl SimPurpose {
    /// 四用途闭集。
    pub const ALL: [SimPurpose; PURPOSE_COUNT] = [
        SimPurpose::CiFull,
        SimPurpose::SchedVerify,
        SimPurpose::FallbackDrill,
        SimPurpose::EfficacyVerify,
    ];

    /// 序号。
    pub const fn idx(self) -> usize {
        match self {
            SimPurpose::CiFull => 0,
            SimPurpose::SchedVerify => 1,
            SimPurpose::FallbackDrill => 2,
            SimPurpose::EfficacyVerify => 3,
        }
    }
}

/// 用途表（表外用途 0x7F03）。
#[derive(Debug)]
pub struct PurposeTable {
    /// 已登记用途位图。
    pub allowed: u32,
    /// 越表事件数（0x7F03）。
    pub oob_events: u32,
}

impl PurposeTable {
    /// 新表（出厂四用途全允许）。
    pub const fn new() -> PurposeTable {
        PurposeTable {
            allowed: 0b1111,
            oob_events: 0,
        }
    }

    /// 用途裁决（位图内放行）。
    pub fn check(&mut self, p: SimPurpose) -> Result<(), u32> {
        if self.allowed & (1 << p.idx()) == 0 {
            self.oob_events += 1;
            return Err(XC_PURPOSE_OOB);
        }
        Ok(())
    }

    /// 吊销用途（管理面——演练越界可吊销）。
    pub fn revoke(&mut self, p: SimPurpose) {
        self.allowed &= !(1 << p.idx());
    }
}

// ---------------------------------------------------------------------------
// 四、真机 trace 回放（F1454 复用）
// ---------------------------------------------------------------------------

/// trace 回放器（判据"回放（真机 trace 回放——回放复用 F1454"）。
#[derive(Debug)]
pub struct TraceReplay {
    /// 录制的输出序列。
    pub recorded: Vec<SimOut>,
    /// 回放结果序列。
    pub replayed: Vec<SimOut>,
    /// F1454 复用声明在册。
    pub f1454_declared: bool,
    /// 失真事件数（0x7F04）。
    pub drift_events: u32,
}

impl TraceReplay {
    /// 新回放器。
    pub const fn new() -> TraceReplay {
        TraceReplay {
            recorded: Vec::new(),
            replayed: Vec::new(),
            f1454_declared: true,
            drift_events: 0,
        }
    }

    /// 录制真机输出。
    pub fn record(&mut self, out: SimOut) {
        self.recorded.push(out);
    }

    /// 回放（逐条对拍——漂移 0x7F04）。
    pub fn replay(&mut self, outputs: &[SimOut]) -> Result<(), u32> {
        if !self.f1454_declared || outputs.len() != self.recorded.len() {
            self.drift_events += 1;
            return Err(XC_REPLAY_DRIFT);
        }
        let mut i = 0;
        while i < outputs.len() {
            if outputs[i] != self.recorded[i] {
                self.drift_events += 1;
                return Err(XC_REPLAY_DRIFT);
            }
            self.replayed.push(outputs[i]);
            i += 1;
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// 五、行为覆盖表（正常/满载/失败/延迟抖动四类）
// ---------------------------------------------------------------------------

/// 行为覆盖表（判据"覆盖清单"）。
#[derive(Debug)]
pub struct BehaviorCoverage {
    /// 四类覆盖标记（0=正常 1=满载 2=失败 3=延迟抖动）。
    pub covered: u32,
    /// 缺角事件数（0x7F05）。
    pub hole_events: u32,
}

impl BehaviorCoverage {
    /// 新表（零覆盖）。
    pub const fn new() -> BehaviorCoverage {
        BehaviorCoverage {
            covered: 0,
            hole_events: 0,
        }
    }

    /// 标记覆盖。
    pub fn cover(&mut self, class: usize) -> Result<(), u32> {
        if class >= COVERAGE_CLASSES {
            self.hole_events += 1;
            return Err(XC_COVERAGE_HOLE);
        }
        self.covered |= 1 << class;
        Ok(())
    }

    /// 完备裁决：四类全覆盖（缺角 0x7F05）。
    pub fn complete(&mut self) -> Result<(), u32> {
        if self.covered != 0b1111 {
            self.hole_events += 1;
            return Err(XC_COVERAGE_HOLE);
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// 六、总引擎 + 五组自检
// ---------------------------------------------------------------------------

/// 异构模拟器总引擎。
#[derive(Debug)]
pub struct HetSimEngine {
    /// 统一框架。
    pub frame: UnifiedSimFrame,
    /// 校准簿。
    pub calib: CalibBook,
    /// 用途表。
    pub purpose: PurposeTable,
    /// 回放器。
    pub replay: TraceReplay,
    /// 行为覆盖表。
    pub cover: BehaviorCoverage,
}

impl HetSimEngine {
    /// 出厂。
    pub const fn new() -> HetSimEngine {
        HetSimEngine {
            frame: UnifiedSimFrame::new(),
            calib: CalibBook::new(),
            purpose: PurposeTable::new(),
            replay: TraceReplay::new(),
            cover: BehaviorCoverage::new(),
        }
    }

    /// 组一：框架——两族挂载完备；游离拒；模型输出合理。
    pub fn selfcheck_frame(&mut self) -> Result<(), u32> {
        let mut e = HetSimEngine::new();
        // 越界档挂载→0x7F01。
        if e.frame.mount(9) != Err(XC_FRAME_ORPHAN) {
            return Err(XC_FRAME_ORPHAN);
        }
        // 两族挂载。
        e.frame.mount(0)?;
        e.frame.mount(1)?;
        e.frame.complete()?;
        if e.frame.ext_declared != true || e.frame.mounted != 0b11 {
            return Err(XC_FRAME_ORPHAN);
        }
        // NPU 批处理模型：批 4 → 延迟 400µs 吞吐非零。
        let npu = e.frame.simulate_npu(4);
        if npu.latency_us != 400 || npu.tput == 0 {
            return Err(XC_FRAME_ORPHAN);
        }
        // DSP 实时调度模型：截止期 [1000, 2000] → 延迟 500。
        let dsp = e.frame.simulate_dsp(&[1_000, 2_000]);
        if dsp.latency_us != 500 {
            return Err(XC_FRAME_ORPHAN);
        }
        Ok(())
    }

    /// 组二：两态校准诚实——混标 0x7F02；查询带态。
    pub fn selfcheck_calib(&mut self) -> Result<(), u32> {
        let mut e = HetSimEngine::new();
        // 真机态标典型值→失诚。
        if e.calib.enroll(CalibRecord { cls: 1, state: CalibState::RealMachine, value_us: 300, typical_annotated: true })
            != Err(XC_CALIB_DISHONEST)
        {
            return Err(XC_CALIB_DISHONEST);
        }
        // 典型值态不标注→失诚。
        if e.calib.enroll(CalibRecord { cls: 0, state: CalibState::DocTypical, value_us: 150, typical_annotated: false })
            != Err(XC_CALIB_DISHONEST)
        {
            return Err(XC_CALIB_DISHONEST);
        }
        if e.calib.dishonest_events != 2 {
            return Err(XC_CALIB_DISHONEST);
        }
        // 合法两态入账。
        e.calib.enroll(CalibRecord { cls: 1, state: CalibState::RealMachine, value_us: 320, typical_annotated: false })?;
        e.calib.enroll(CalibRecord { cls: 0, state: CalibState::DocTypical, value_us: 150, typical_annotated: true })?;
        // 查询带态：NPU 真机 320 / DSP 典型 150。
        match e.calib.query(1) {
            Some((CalibState::RealMachine, 320)) => {}
            _ => return Err(XC_CALIB_DISHONEST),
        }
        match e.calib.query(0) {
            Some((CalibState::DocTypical, 150)) => {}
            _ => return Err(XC_CALIB_DISHONEST),
        }
        Ok(())
    }

    /// 组三：用途表——四用途放行；吊销后越表 0x7F03。
    pub fn selfcheck_purpose(&mut self) -> Result<(), u32> {
        let mut e = HetSimEngine::new();
        // 四用途全放行。
        let mut i = 0;
        while i < PURPOSE_COUNT {
            e.purpose.check(SimPurpose::ALL[i])?;
            i += 1;
        }
        // 吊销能效路由验证→越表。
        e.purpose.revoke(SimPurpose::EfficacyVerify);
        if e.purpose.check(SimPurpose::EfficacyVerify) != Err(XC_PURPOSE_OOB) {
            return Err(XC_PURPOSE_OOB);
        }
        if e.purpose.oob_events != 1 {
            return Err(XC_PURPOSE_OOB);
        }
        // 其余三用途仍放行。
        e.purpose.check(SimPurpose::CiFull)?;
        Ok(())
    }

    /// 组四：回放——一致过；漂移 0x7F04；长度失配拒。
    pub fn selfcheck_replay(&mut self) -> Result<(), u32> {
        let mut e = HetSimEngine::new();
        let o1 = SimOut { cls: 1, latency_us: 400, tput: 2500 };
        let o2 = SimOut { cls: 0, latency_us: 500, tput: 2000 };
        e.replay.record(o1);
        e.replay.record(o2);
        // 一致回放过。
        e.replay.replay(&[o1, o2])?;
        // 漂移回放→0x7F04。
        let drift = SimOut { cls: 1, latency_us: 401, tput: 2500 };
        if e.replay.replay(&[drift, o2]) != Err(XC_REPLAY_DRIFT) {
            return Err(XC_REPLAY_DRIFT);
        }
        // 长度失配→0x7F04。
        if e.replay.replay(&[o1]) != Err(XC_REPLAY_DRIFT) {
            return Err(XC_REPLAY_DRIFT);
        }
        if e.replay.drift_events != 2 {
            return Err(XC_REPLAY_DRIFT);
        }
        Ok(())
    }

    /// 组五：覆盖——四类逐类覆盖；缺角 0x7F05；端到端全绿。
    pub fn selfcheck_coverage(&mut self) -> Result<(), u32> {
        let mut e = HetSimEngine::new();
        // 缺角→0x7F05。
        e.cover.cover(0)?;
        e.cover.cover(1)?;
        if e.cover.complete() != Err(XC_COVERAGE_HOLE) {
            return Err(XC_COVERAGE_HOLE);
        }
        // 四类全覆盖。
        e.cover.cover(2)?;
        e.cover.cover(3)?;
        e.cover.complete()?;
        if e.cover.hole_events != 1 {
            return Err(XC_COVERAGE_HOLE);
        }
        // 端到端：框架+校准+用途+回放+覆盖五件全绿。
        let mut full = HetSimEngine::new();
        full.frame.mount(0)?;
        full.frame.mount(1)?;
        full.frame.complete()?;
        full.calib.enroll(CalibRecord { cls: 1, state: CalibState::RealMachine, value_us: 320, typical_annotated: false })?;
        full.purpose.check(SimPurpose::CiFull)?;
        let out = full.frame.simulate_npu(4);
        full.replay.record(out);
        full.replay.replay(&[out])?;
        let mut c = 0;
        while c < COVERAGE_CLASSES {
            full.cover.cover(c)?;
            c += 1;
        }
        full.cover.complete()?;
        if full.replay.drift_events != 0 || full.cover.hole_events != 0 {
            return Err(XC_REPLAY_DRIFT);
        }
        Ok(())
    }

    /// 全量自检入口（五组+段对账）。
    pub fn selfcheck_all(&mut self) -> Result<(), u32> {
        self.selfcheck_frame()?;
        self.selfcheck_calib()?;
        self.selfcheck_purpose()?;
        self.selfcheck_replay()?;
        self.selfcheck_coverage()?;
        Self::segment_guard()?;
        Ok(())
    }

    /// 段对账：五码全落 0x7F00..0x8000 开区间且互异。
    pub const fn segment_guard() -> Result<(), u32> {
        let codes = [
            XC_FRAME_ORPHAN,
            XC_CALIB_DISHONEST,
            XC_PURPOSE_OOB,
            XC_REPLAY_DRIFT,
            XC_COVERAGE_HOLE,
        ];
        let mut i = 0;
        while i < codes.len() {
            if codes[i] < 0x7F00 || codes[i] >= 0x8000 {
                return Err(XC_FRAME_ORPHAN);
            }
            let mut j = i + 1;
            while j < codes.len() {
                if codes[i] == codes[j] {
                    return Err(XC_FRAME_ORPHAN);
                }
                j += 1;
            }
            i += 1;
        }
        Ok(())
    }
}
