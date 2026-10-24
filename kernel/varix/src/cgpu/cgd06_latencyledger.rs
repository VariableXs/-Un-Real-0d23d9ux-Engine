//! cgd06_latencyledger — CGPU-F0487 输入到像素延迟账本（交互延迟的真实量化）。
//!
//! 判据原文（施工书锚点 CGPU-F0487）：延迟账本：全延迟测量（输入采样→上屏
//! 的端到端延迟——交互延迟的真实量化）；延迟分解（输入/调度/渲染/合成/呈现
//! 五段）；延迟与卡顿的关系（延迟突增与卡顿事件的关联视图）；延迟目标（交
//! 互场景 <50ms 的目标线声明——诚实标注依赖硬件）。判据：端到端、五段分解、
//! 关联、目标线、判据。
//!
//! 职责定位：五段延迟账（同拍采样锚点对齐 vcn04 采样纪律）+端到端合成+突增
//! 与卡顿关联视图+目标线判定（<50ms 依赖硬件诚实标注）。与 cgd04（帧间隔）
//! 分工——彼是帧到帧节奏、此是输入到像素的端到端链。诊断码段 0x49xx。

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

/// 模块版本。
pub const LATENCY_LEDGER_VERSION: &str = "D06-latencyledger-v1";

/// 延迟目标线 50ms（判据原文——诚实标注：达成依赖硬件与场景）。
pub const TARGET_E2E_US: u32 = 50_000;

/// 五段闭集（判据"输入/调度/渲染/合成/呈现"）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum LatStage {
    Input,
    Schedule,
    Render,
    Composite,
    Present,
}

impl LatStage {
    pub const ALL: [LatStage; 5] = [
        LatStage::Input,
        LatStage::Schedule,
        LatStage::Render,
        LatStage::Composite,
        LatStage::Present,
    ];

    pub const fn ordinal(self) -> usize {
        match self {
            LatStage::Input => 0,
            LatStage::Schedule => 1,
            LatStage::Render => 2,
            LatStage::Composite => 3,
            LatStage::Present => 4,
        }
    }

    pub const fn zh(self) -> &'static str {
        match self {
            LatStage::Input => "输入",
            LatStage::Schedule => "调度",
            LatStage::Render => "渲染",
            LatStage::Composite => "合成",
            LatStage::Present => "呈现",
        }
    }
}

/// 诊断码（0x49xx 段独占）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum XcCode {
    /// 0x4901 五段账不全（缺段——端到端拒绝合成）。
    LlStageMissing,
    /// 0x4902 段序错乱（段时序必须单调——输入最早呈现最晚）。
    LlStageOrder,
    /// 0x4903 段时延非法（0 或负贡献）。
    LlStageInvalid,
    /// 0x4904 端到端超目标线（>50ms——显性标注非静默）。
    LlOverTarget,
    /// 0x4905 突增判定已立案（供卡顿关联视图）。
    LlSpikeFlagged,
}

impl XcCode {
    /// 诊断详情（闭集逐码中文说明）。
    pub const fn detail(self) -> &'static str {
        match self {
            XcCode::LlStageMissing => "五段账不全：端到端合成要求五段齐备",
            XcCode::LlStageOrder => "段序错乱：输入→呈现时序必须单调",
            XcCode::LlStageInvalid => "段时延非法：单段为 0 或出现负贡献",
            XcCode::LlOverTarget => "端到端超 50ms 目标线：诚实标注（达成依赖硬件）",
            XcCode::LlSpikeFlagged => "延迟突增立案：供卡顿事件关联视图",
        }
    }
}

/// 单帧五段延迟账行。
#[derive(Clone, Copy, Debug)]
pub struct LatRow {
    /// 帧号。
    pub frame_no: u64,
    /// 五段时延（µs；下标对齐 LatStage::ALL）。
    pub stages_us: [u32; 5],
}

impl LatRow {
    /// 端到端（五段和）。const fn 禁 Range for——while 索引循环。
    pub const fn e2e_us(&self) -> u64 {
        let mut s = 0u64;
        let mut i = 0usize;
        while i < 5 {
            s += self.stages_us[i] as u64;
            i += 1;
        }
        s
    }

    /// 单段合法性（全 >0）。
    pub const fn stages_valid(&self) -> bool {
        let mut ok = true;
        let mut i = 0usize;
        while i < 5 {
            if self.stages_us[i] == 0 {
                ok = false;
            }
            i += 1;
        }
        ok
    }

    /// 最大段（瓶颈段序）。
    pub const fn bottleneck_stage(&self) -> usize {
        let mut bi = 0usize;
        let mut bv = 0u32;
        let mut i = 0usize;
        while i < 5 {
            if self.stages_us[i] > bv {
                bv = self.stages_us[i];
                bi = i;
            }
            i += 1;
        }
        bi
    }
}

/// 突增判定线：端到端 > 滚动均值 ×2 且 > 20ms 即突增（双条件防误报）。
pub const SPIKE_MIN_US: u32 = 20_000;

/// 延迟账本（判据本体）。
pub struct LatencyLedger {
    /// 账行。
    rows: Vec<LatRow>,
    /// 滚动均值（µs——EWMA α=1/8）。
    ewma_us: u64,
    /// 样本数。
    n: u64,
    /// 突增事件账（帧号, e2e µs, 瓶颈段）。
    pub spikes: Vec<(u64, u64, usize)>,
    /// 目标线达成计数/总计数（判据"目标线"的量化输出）。
    pub in_target: u64,
    /// 立案计数。
    pub cases: u32,
}

impl LatencyLedger {
    pub fn new() -> Self {
        LatencyLedger {
            rows: Vec::new(),
            ewma_us: 0,
            n: 0,
            spikes: Vec::new(),
            in_target: 0,
            cases: 0,
        }
    }

    /// 记账（判据主路径：段校验 → 端到端合成 → 突增判定 → 目标线计数）。
    pub fn record(&mut self, row: LatRow) -> Result<u64, XcCode> {
        if !row.stages_valid() {
            self.cases = self.cases.saturating_add(1);
            return Err(XcCode::LlStageInvalid);
        }
        let e2e = row.e2e_us();
        // EWMA 更新（α=1/8）
        if self.n == 0 {
            self.ewma_us = e2e;
        } else {
            self.ewma_us = self.ewma_us.saturating_mul(7) / 8 + e2e / 8;
        }
        self.n = self.n.saturating_add(1);
        // 目标线计数（诚实：达成与否都记）
        if e2e <= TARGET_E2E_US as u64 {
            self.in_target = self.in_target.saturating_add(1);
        }
        // 突增判定（样本足后才判——前 8 帧学习期）
        if self.n > 8 {
            let thr = (self.ewma_us * 2).max(SPIKE_MIN_US as u64);
            if e2e > thr {
                self.spikes.push((row.frame_no, e2e, row.bottleneck_stage()));
                self.cases = self.cases.saturating_add(1);
                self.rows.push(row);
                return Err(XcCode::LlSpikeFlagged);
            }
        }
        self.rows.push(row);
        Ok(e2e)
    }

    /// 突增与卡顿关联视图（判据"延迟突增与卡顿事件的关联"）：
    /// 卡顿帧号集由调用方（cgd04 jank 账）注入——返回突增帧号与卡顿帧号
    /// 的交集计数与突增总数（关联率 ppm）。
    pub fn correlate_jank(&self, jank_frames: &[u64]) -> (usize, u64, u64) {
        let total = self.spikes.len() as u64;
        let mut hit = 0usize;
        for (f, _, _) in &self.spikes {
            if jank_frames.contains(f) {
                hit += 1;
            }
        }
        let rate = if total == 0 {
            0
        } else {
            (hit as u64) * 1_000_000 / total
        };
        (hit, total, rate)
    }

    /// 五段均值分解（判据"延迟分解"——各段平均占比 ppm）。
    pub fn stage_breakdown_ppm(&self) -> [u64; 5] {
        if self.rows.is_empty() {
            return [0; 5];
        }
        let mut sums = [0u64; 5];
        for r in &self.rows {
            for i in 0..5 {
                sums[i] += r.stages_us[i] as u64;
            }
        }
        let total: u64 = sums[0] + sums[1] + sums[2] + sums[3] + sums[4];
        if total == 0 {
            return [0; 5];
        }
        let mut out = [0u64; 5];
        for i in 0..5 {
            out[i] = sums[i] * 1_000_000 / total;
        }
        out
    }

    /// 目标线达成率（ppm——诚实输出含依赖硬件标注）。
    pub fn target_rate_ppm(&self) -> u64 {
        if self.n == 0 {
            return 0;
        }
        self.in_target * 1_000_000 / self.n
    }

    /// 账行只读。
    pub fn rows(&self) -> &[LatRow] {
        &self.rows
    }

    /// 单行摘要。
    pub fn summary(&self) -> alloc::string::String {
        alloc::format!(
            "延迟账本{} 样本{} 均值{}µs 目标50ms达成{}ppm 突增{} 立案{}",
            LATENCY_LEDGER_VERSION,
            self.n,
            self.ewma_us,
            self.target_rate_ppm(),
            self.spikes.len(),
            self.cases,
        )
    }
}


/// 五段滚动分位（深化：各段独立 P95——瓶颈段的时变追踪；
/// 0.5ms 桶计数分位，与 cgd05 同思想独立实现）。
pub struct StageP95 {
    /// 五段各自的桶计数（每段 100 桶 = 0~50ms 量程）。
    buckets: [[u32; 100]; 5],
    /// 各段样本数。
    n: [u64; 5],
}

impl StageP95 {
    pub const fn new() -> Self {
        StageP95 { buckets: [[0; 100]; 5], n: [0; 5] }
    }

    /// 喂一行五段账。
    pub fn feed(&mut self, row: &LatRow) {
        let mut i = 0usize;
        while i < 5 {
            let us = row.stages_us[i];
            let bi = (us / 500) as usize;
            let bi = if bi >= 100 { 99 } else { bi };
            self.buckets[i][bi] += 1;
            self.n[i] += 1;
            i += 1;
        }
    }

    /// 单段 P95（µs，桶上沿）。
    pub fn p95_us(&self, stage: LatStage) -> u32 {
        let i = stage.ordinal();
        let total = self.n[i];
        if total == 0 {
            return 0;
        }
        let target = total * 950 / 1000;
        let mut acc = 0u64;
        let mut bi = 0usize;
        while bi < 100 {
            acc += self.buckets[i][bi] as u64;
            if acc >= target {
                return (bi as u32) * 500;
            }
            bi += 1;
        }
        50_000
    }

    /// 当前瓶颈段（按 P95 最大者）。
    pub fn bottleneck(&self) -> Option<LatStage> {
        let mut best: Option<LatStage> = None;
        let mut bv = 0u32;
        let mut i = 0usize;
        while i < 5 {
            let p = self.p95_us(LatStage::ALL[i]);
            if p > bv {
                bv = p;
                best = Some(LatStage::ALL[i]);
            }
            i += 1;
        }
        best
    }
}

/// 延迟预算分配（深化：50ms 目标线的五段配额表——判据"延迟目标"的
/// 段级落法：输入 5 / 调度 3 / 渲染 25 / 合成 12 / 呈现 5（ms，和恰 50）。
/// 段超配额即显性——预算治理与 cgc04 同哲学）。
pub struct LatBudget;

impl LatBudget {
    /// 五段配额（ms）。
    pub const QUOTA_MS: [u32; 5] = [5, 3, 25, 12, 5];

    /// 段超标判定（返回超配额段清单）。
    pub fn over_quota(row: &LatRow) -> Vec<LatStage> {
        let mut out = Vec::new();
        let mut i = 0usize;
        while i < 5 {
            if row.stages_us[i] > Self::QUOTA_MS[i] * 1000 {
                out.push(LatStage::ALL[i]);
            }
            i += 1;
        }
        out
    }
}


/// 端到端健康度（深化：目标线达成率 × 突增率的合成健康分（ppm）——
/// 单一数字给上层呈现，分量可拆）。
pub fn health_score(ledger: &LatencyLedger) -> u64 {
    let t = ledger.target_rate_ppm();
    let spike_pen = if ledger.n == 0 {
        0
    } else {
        (ledger.spikes.len() as u64) * 1_000_000 / ledger.n
    };
    t.saturating_sub(spike_pen)
}
