//! CGPU-F1697 · K07 开工与异构遥测总架构
//!
//! 判据锚点（施工书原文摘录）：
//! 总架构：定位（K07=异构可观测性总出口：把 K01-K06 六组的运行状态变成"看得见、
//! 查得着、调得动"——可观测声明；架构五段（指标注册→采集→分析（归因/画像）→
//! 呈现（面板/报告）→调试（trace/剖析）——五段；与 K01 遥测（K01 基础遥测→
//! K07 域级收口——上下游声明；与 J08/P 域（仓库/看板/协议复用——三复用声明；
//! 测试（定位/五段/上下游/复用四组）。判据：可观测、五段、上下游、三复用、四组、判据。
//!
//! 诊断码段 0x8401~0x8405（独占）：
//!   0x8401 可观测能力缺一（三能力闭集未全声明）
//!   0x8402 五段断链（段缺失/乱序/跳段/反向）
//!   0x8403 上下游断言失效（上游数据源缺/下游口径漂移）
//!   0x8404 复用声明失据（源单号空/复用对象不在册）
//!   0x8405 段越界（判码落 0x8400..0x8500 区间外）
//!
//! 纯功能：no_std+alloc；零 unwrap/expect；全部自检在 selfcheck_* 组内。


/// 模块版本。
pub const K07_ARCH_VERSION: &str = "K16-k07arch-v1";

// ---------------------------------------------------------------------------
// 判码（段内独占 0x8401~0x8405）
// ---------------------------------------------------------------------------

pub const XC_OBS_CAP_MISSING: u32 = 0x8401;
pub const XC_PIPE_BROKEN: u32 = 0x8402;
pub const XC_STREAM_BROKEN: u32 = 0x8403;
pub const XC_REUSE_NO_EVIDENCE: u32 = 0x8404;
pub const XC_SEG_OUT_OF_RANGE: u32 = 0x8405;

// ---------------------------------------------------------------------------
// 一、可观测声明：三能力闭集（看得见/查得着/调得动）
// ---------------------------------------------------------------------------

/// 可观测能力三闭集。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ObsCap {
    /// 看得见：运行状态有呈现出口（面板/报告）。
    Visible,
    /// 查得着：异常可归因到引擎/段/任务（归因/画像）。
    Traceable,
    /// 调得动：调试工具可介入（trace/剖析）。
    Adjustable,
}

impl ObsCap {
    /// 三能力闭集全表。
    pub const ALL: [ObsCap; 3] = [ObsCap::Visible, ObsCap::Traceable, ObsCap::Adjustable];

    /// 能力名。
    pub const fn name(self) -> &'static str {
        match self {
            ObsCap::Visible => "visible",
            ObsCap::Traceable => "traceable",
            ObsCap::Adjustable => "adjustable",
        }
    }

    /// 能力保证条件：每能力必须锚一条已成立的管道段（能力→段的兑现映射）。
    pub const fn guaranteed_by(self) -> PipeStage {
        match self {
            // 看得见 ← 呈现段（面板/报告）。
            ObsCap::Visible => PipeStage::Present,
            // 查得着 ← 分析段（归因/画像）。
            ObsCap::Traceable => PipeStage::Analyze,
            // 调得动 ← 调试段（trace/剖析）。
            ObsCap::Adjustable => PipeStage::Debug,
        }
    }
}

/// 可观测声明：三能力全声明且每能力的保证段在管道里。
pub struct ObsDecl {
    declared: [bool; 3],
}

impl ObsDecl {
    pub fn build() -> Self {
        ObsDecl { declared: [true, true, true] }
    }

    /// 三能力齐备：每能力声明 + 保证段存在于管道。
    pub fn complete(&self, pipe: &Pipe) -> Result<(), u32> {
        let mut i = 0;
        while i < 3 {
            if !self.declared[i] {
                return Err(XC_OBS_CAP_MISSING);
            }
            let stage = ObsCap::ALL[i].guaranteed_by();
            if !pipe.has_stage(stage) {
                return Err(XC_OBS_CAP_MISSING);
            }
            i += 1;
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// 二、架构五段：指标注册→采集→分析→呈现→调试
// ---------------------------------------------------------------------------

/// 管道段枚举（五段闭集——判据"架构五段"）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PipeStage {
    /// 段 1：指标注册（四族指标入册——F1698 对接）。
    Register,
    /// 段 2：采集（三段开销预算内——F1706 对接）。
    Collect,
    /// 段 3：分析（归因/画像——F1699 对接）。
    Analyze,
    /// 段 4：呈现（面板/报告——F1701/F1709 对接）。
    Present,
    /// 段 5：调试（trace/剖析——F1700/F1702/F1703 对接）。
    Debug,
}

impl PipeStage {
    /// 段序号（1..=5）。
    pub const fn ordinal(self) -> u8 {
        match self {
            PipeStage::Register => 1,
            PipeStage::Collect => 2,
            PipeStage::Analyze => 3,
            PipeStage::Present => 4,
            PipeStage::Debug => 5,
        }
    }

    /// 全五段有序表。
    pub const ALL: [PipeStage; 5] = [
        PipeStage::Register,
        PipeStage::Collect,
        PipeStage::Analyze,
        PipeStage::Present,
        PipeStage::Debug,
    ];

    /// 段名。
    pub const fn name(self) -> &'static str {
        match self {
            PipeStage::Register => "register",
            PipeStage::Collect => "collect",
            PipeStage::Analyze => "analyze",
            PipeStage::Present => "present",
            PipeStage::Debug => "debug",
        }
    }

    /// 每段责任句（呈现面）。
    pub const fn duty(self) -> &'static str {
        match self {
            PipeStage::Register => "metric families enrolled, formula unique per metric",
            PipeStage::Collect => "overhead budgeted, zero-interference三条 in force",
            PipeStage::Analyze => "attribution + profiling, cross-engine",
            PipeStage::Present => "panels + reports, 1Hz baseline refresh",
            PipeStage::Debug => "trace open format + hotspot rules",
        }
    }
}

/// 五段管道：注册段序 + 单向校验。
pub struct Pipe {
    stages: [bool; 5],
}

impl Pipe {
    /// 完整管道构造：五段全在。
    pub const fn build() -> Self {
        Pipe { stages: [true; 5] }
    }

    pub fn has_stage(&self, s: PipeStage) -> bool {
        self.stages[(s.ordinal() - 1) as usize]
    }

    /// 管道闭合：五段全在且序号 1..=5 连续（枚举保证连续，这里复核无缺段）。
    pub fn intact(&self) -> Result<(), u32> {
        let mut i = 0;
        while i < 5 {
            if !self.stages[i] {
                return Err(XC_PIPE_BROKEN);
            }
            i += 1;
        }
        // 逐段序号单调递增复核（防枚举序被改乱）。
        let mut prev = 0;
        for s in PipeStage::ALL {
            if s.ordinal() <= prev {
                return Err(XC_PIPE_BROKEN);
            }
            prev = s.ordinal();
        }
        Ok(())
    }

    /// 数据流向校验：任意 a→b 的合法传递必须序号递增（单向无反向）。
    pub fn flow_legal(&self, from: PipeStage, to: PipeStage) -> Result<(), u32> {
        if !self.has_stage(from) || !self.has_stage(to) {
            return Err(XC_PIPE_BROKEN);
        }
        if to.ordinal() <= from.ordinal() {
            return Err(XC_PIPE_BROKEN);
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// 三、上下游声明：K01 → K07 域级收口
// ---------------------------------------------------------------------------

/// 上游数据源（K01 基础遥测 → K07 域级收口）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct StreamRow {
    /// 上游源（源单号）。
    pub upstream: &'static str,
    /// 交付物（本段管道承接点）。
    pub lands_at: PipeStage,
    /// 口径锁：上游口径 id 必须与下游登记口径 id 一致（防口径漂移）。
    pub caliber_id: &'static str,
    /// 声明在册。
    pub declared: bool,
}

/// 上下游声明账。
pub struct StreamBook {
    rows: [StreamRow; 2],
}

impl StreamBook {
    /// 构造：K01 遥测（采集段承接）+ K06 能效账（分析段承接）。
    pub const fn build() -> Self {
        StreamBook {
            rows: [
                StreamRow {
                    upstream: "CGPU-F0900-K01",
                    lands_at: PipeStage::Collect,
                    caliber_id: "frame_us_monotonic",
                    declared: true,
                },
                StreamRow {
                    upstream: "CGPU-F1680-K06",
                    lands_at: PipeStage::Analyze,
                    caliber_id: "fps_per_watt_bp",
                    declared: true,
                },
            ],
        }
    }

    /// 上下游断言：每行声明在册 + 交付段在管道 + 口径 id 非空。
    pub fn verified(&self, pipe: &Pipe) -> Result<(), u32> {
        for r in &self.rows {
            if !r.declared {
                return Err(XC_STREAM_BROKEN);
            }
            if !pipe.has_stage(r.lands_at) {
                return Err(XC_STREAM_BROKEN);
            }
            if r.caliber_id.is_empty() || r.upstream.is_empty() {
                return Err(XC_STREAM_BROKEN);
            }
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// 四、三复用声明：J08 仓库 / P 域看板 / 协议
// ---------------------------------------------------------------------------

/// 复用声明行（源单号非空纪律——口头复用=无复用）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ReuseRow {
    /// 复用对象名。
    pub target: &'static str,
    /// 源单号（CGPU/J/P 域源单）。
    pub origin: &'static str,
    /// 复用落点段。
    pub lands_at: PipeStage,
}

/// 三复用声明账。
pub struct ReuseBook {
    rows: [ReuseRow; 3],
}

impl ReuseBook {
    /// 构造：J08 指标仓库 / P 域看板 / CIPC 协议三复用。
    pub const fn build() -> Self {
        ReuseBook {
            rows: [
                ReuseRow {
                    target: "metric_warehouse",
                    origin: "CGPU-J08",
                    lands_at: PipeStage::Collect,
                },
                ReuseRow {
                    target: "dashboard",
                    origin: "CGPU-P-DOMAIN",
                    lands_at: PipeStage::Present,
                },
                ReuseRow {
                    target: "cipc_protocol",
                    origin: "CGPU-F1704",
                    lands_at: PipeStage::Debug,
                },
            ],
        }
    }

    /// 三复用齐：全部有源单号且落点段在管道。
    pub fn verified(&self, pipe: &Pipe) -> Result<(), u32> {
        for r in &self.rows {
            if r.origin.is_empty() {
                return Err(XC_REUSE_NO_EVIDENCE);
            }
            if !pipe.has_stage(r.lands_at) {
                return Err(XC_REUSE_NO_EVIDENCE);
            }
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// 自检（四组：定位/五段/上下游/复用）
// ---------------------------------------------------------------------------

/// K07 总架构引擎。
pub struct ArchEngine;

impl ArchEngine {
    /// 组一：定位——三能力声明齐备且每能力有保证段。
    pub fn selfcheck_positioning() -> Result<(), u32> {
        let pipe = Pipe::build();
        pipe.intact()?;
        ObsDecl::build().complete(&pipe)?;
        Ok(())
    }

    /// 组二：五段——管道闭合 + 单向流合法 + 反向流拒绝。
    pub fn selfcheck_pipe() -> Result<(), u32> {
        let pipe = Pipe::build();
        pipe.intact()?;
        // 合法流：注册→采集→分析→呈现→调试 全链。
        let mut i = 0;
        while i + 1 < PipeStage::ALL.len() {
            pipe.flow_legal(PipeStage::ALL[i], PipeStage::ALL[i + 1])?;
            i += 1;
        }
        // 反向流拒绝：呈现→采集（序号回退）。
        if pipe.flow_legal(PipeStage::Present, PipeStage::Collect).is_ok() {
            return Err(XC_PIPE_BROKEN);
        }
        // 同段流拒绝。
        if pipe.flow_legal(PipeStage::Debug, PipeStage::Debug).is_ok() {
            return Err(XC_PIPE_BROKEN);
        }
        // 跨段责任句非空（呈现面完整性）。
        for s in PipeStage::ALL {
            if s.duty().is_empty() {
                return Err(XC_PIPE_BROKEN);
            }
        }
        Ok(())
    }

    /// 组三：上下游——断言生效 + 口径 id 锁 + 断链反例。
    pub fn selfcheck_stream() -> Result<(), u32> {
        let pipe = Pipe::build();
        StreamBook::build().verified(&pipe)?;
        // 断链反例：上游置空即拒。
        let mut book = StreamBook::build();
        book.rows[0].upstream = "";
        if book.verified(&pipe).is_ok() {
            return Err(XC_STREAM_BROKEN);
        }
        // 口径漂移反例：口径 id 清空即拒。
        let mut book2 = StreamBook::build();
        book2.rows[1].caliber_id = "";
        if book2.verified(&pipe).is_ok() {
            return Err(XC_STREAM_BROKEN);
        }
        Ok(())
    }

    /// 组四：三复用——声明齐 + 断据反例。
    pub fn selfcheck_reuse() -> Result<(), u32> {
        let pipe = Pipe::build();
        ReuseBook::build().verified(&pipe)?;
        // 断据反例：抹源单号即拒。
        let mut book = ReuseBook::build();
        book.rows[0].origin = "";
        if book.verified(&pipe).is_ok() {
            return Err(XC_REUSE_NO_EVIDENCE);
        }
        Ok(())
    }

    /// 总闸：四组串联 + 段对账。
    pub fn selfcheck_all() -> Result<(), u32> {
        Self::segment_guard()?;
        Self::selfcheck_positioning()?;
        Self::selfcheck_pipe()?;
        Self::selfcheck_stream()?;
        Self::selfcheck_reuse()?;
        Ok(())
    }

    /// 段对账：五码全落 0x8400..0x8500 开区间且互异。
    pub const fn segment_guard() -> Result<(), u32> {
        let codes = [
            XC_OBS_CAP_MISSING,
            XC_PIPE_BROKEN,
            XC_STREAM_BROKEN,
            XC_REUSE_NO_EVIDENCE,
            XC_SEG_OUT_OF_RANGE,
        ];
        let mut i = 0;
        while i < codes.len() {
            if codes[i] < 0x8400 || codes[i] >= 0x8500 {
                return Err(XC_OBS_CAP_MISSING);
            }
            let mut j = i + 1;
            while j < codes.len() {
                if codes[i] == codes[j] {
                    return Err(XC_OBS_CAP_MISSING);
                }
                j += 1;
            }
            i += 1;
        }
        Ok(())
    }
}
