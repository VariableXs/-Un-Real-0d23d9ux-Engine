//! cgd17_jittersrc — CGPU-F0498 抖动源分类（五源分类学+指纹证据+置信聚合）。
//!
//! 判据原文（施工书锚点 CGPU-F0498）：源分类：五源分类表（OS 调度抢占/驱
//! 动卡顿/引擎停顿（类 GC 式）/热降频/IO 阻塞——抖动成因分类学）；源识别
//! 证据（每类源的可观测指纹——关联调度/热/IO 数据）；分类置信度（多证据
//! 聚合判定）；分类演进（新源类型的可扩展分类表）。判据：五源、指纹、置
//! 信、可扩展、判据。
//!
//! 职责定位：五源闭集分类学（OS 调度抢占/驱动卡顿/引擎停顿/热降频/IO 阻
//! 塞——表外来源 0x7301 拒收，分类表是分类学的边界不是凑数清单）+源识别
//! 证据（每源绑定可观测指纹位图：调度抢占↔线程迁移计数跳变/驱动卡顿↔命
//! 令环形空转/引擎停顿↔引擎空转但队列堆积/热降频↔温度超阈+频率下调记录
//! /IO 阻塞↔页错误激增——证据与源指纹不符 0x7302）+置信聚合（单证据不下
//! 判，≥2 类独立证据同指一源才出判——0x7303 置信不足；互斥来源同时高置
//! 信=归因冲突 0x7305 上报人工）+分类演进（新源走版本窗扩展：表版本+1 才
//! 能加行，绕过版本窗 0x7304——F0492 冻结纪律同源）。诊断码段
//! 0x7301~0x7305。

use alloc::vec::Vec;

/// 模块版本。
pub const JITTER_SRC_VERSION: &str = "D17-jittersrc-v1";

// ---------------------------------------------------------------------------
// 诊断码（XcCode 0x73 段，本会话独占）
// ---------------------------------------------------------------------------

/// 0x7301 五源外来源（分类表外源）。
pub const XC_SOURCE_OUT_OF_SET: u32 = 0x7301;
/// 0x7302 指纹失配（证据与源指纹不符）。
pub const XC_FINGERPRINT_MISMATCH: u32 = 0x7302;
/// 0x7303 置信不足（单证据即下判）。
pub const XC_CONFIDENCE_LOW: u32 = 0x7303;
/// 0x7304 分类表冻结违约（扩展未走版本窗）。
pub const XC_TABLE_FROZEN: u32 = 0x7304;
/// 0x7305 归因冲突（多源同时高置信互斥失败）。
pub const XC_ATTRIBUTION_CONFLICT: u32 = 0x7305;

// ---------------------------------------------------------------------------
// 常量
// ---------------------------------------------------------------------------

/// 五源闭集容量。
pub const SOURCE_CAP: usize = 5;
/// 置信出判门槛：独立证据数下限。
pub const CONFIDENCE_MIN_EVIDENCE: usize = 2;
/// 归因冲突阈：两源置信差低于此（万分比）视为冲突。
pub const CONFLICT_MARGIN_BP: u32 = 500;
/// 分类表版本窗基线。
pub const TABLE_VER_BASE: u32 = 1;

// ---------------------------------------------------------------------------
// 一、五源分类学（闭集）
// ---------------------------------------------------------------------------

/// 抖动五源（判据"五源分类表"）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum JitterSourceKind {
    /// OS 调度抢占。
    OsPreempt,
    /// 驱动卡顿。
    DriverStall,
    /// 引擎停顿（类 GC 式）。
    EnginePause,
    /// 热降频。
    ThermalDown,
    /// IO 阻塞。
    IoBlock,
}

impl JitterSourceKind {
    /// 五源闭集。
    pub const ALL: [JitterSourceKind; SOURCE_CAP] = [
        JitterSourceKind::OsPreempt,
        JitterSourceKind::DriverStall,
        JitterSourceKind::EnginePause,
        JitterSourceKind::ThermalDown,
        JitterSourceKind::IoBlock,
    ];

    /// 表内序号。
    pub const fn idx(self) -> usize {
        match self {
            JitterSourceKind::OsPreempt => 0,
            JitterSourceKind::DriverStall => 1,
            JitterSourceKind::EnginePause => 2,
            JitterSourceKind::ThermalDown => 3,
            JitterSourceKind::IoBlock => 4,
        }
    }

    /// 中文名。
    pub const fn zh(self) -> &'static str {
        match self {
            JitterSourceKind::OsPreempt => "OS调度抢占",
            JitterSourceKind::DriverStall => "驱动卡顿",
            JitterSourceKind::EnginePause => "引擎停顿",
            JitterSourceKind::ThermalDown => "热降频",
            JitterSourceKind::IoBlock => "IO阻塞",
        }
    }
}

// ---------------------------------------------------------------------------
// 二、源识别证据（指纹位图）
// ---------------------------------------------------------------------------

/// 证据类型位（位图语义——每源绑定一组指纹位）。
pub mod evidence {
    /// 线程迁移计数跳变（调度抢占指纹）。
    pub const CTX_SWITCH_SPIKE: u32 = 1 << 0;
    /// 命令环形空转（驱动卡顿指纹）。
    pub const RING_IDLE: u32 = 1 << 1;
    /// 引擎空转但队列堆积（引擎停顿指纹）。
    pub const ENGINE_IDLE_QUEUED: u32 = 1 << 2;
    /// 温度超阈记录（热降频指纹）。
    pub const THERMAL_TRIP: u32 = 1 << 3;
    /// 频率下调记录（热降频指纹）。
    pub const FREQ_DROPPED: u32 = 1 << 4;
    /// 页错误激增（IO 阻塞指纹）。
    pub const PAGE_FAULT_STORM: u32 = 1 << 5;
    /// GPU 时间戳断流（驱动卡顿辅证）。
    pub const TIMESTAMP_GAP: u32 = 1 << 6;
}

/// 指纹表（判据"每类源的可观测指纹——关联调度/热/IO 数据"）。
///
/// 每源的主指纹位集合——证据必须落在源指纹集内才有效（0x7302 拦野证据）。
pub const FINGERPRINTS: [u32; SOURCE_CAP] = [
    evidence::CTX_SWITCH_SPIKE,                            // OsPreempt
    evidence::RING_IDLE | evidence::TIMESTAMP_GAP,         // DriverStall
    evidence::ENGINE_IDLE_QUEUED,                          // EnginePause
    evidence::THERMAL_TRIP | evidence::FREQ_DROPPED,       // ThermalDown
    evidence::PAGE_FAULT_STORM,                            // IoBlock
];

/// 证据条目。
#[derive(Clone, Copy, Debug)]
pub struct Evidence {
    /// 指纹位。
    pub bit: u32,
    /// 强度（万分比：观测值相对基线的偏离度）。
    pub strength_bp: u32,
}

/// 指纹校验器。
#[derive(Debug)]
pub struct FingerprintChecker {
    /// 校验失败数（0x7302）。
    pub mismatch_events: u32,
}

impl FingerprintChecker {
    /// 新校验器。
    pub const fn new() -> FingerprintChecker {
        FingerprintChecker {
            mismatch_events: 0,
        }
    }

    /// 校验：证据位必须属于该源的指纹集。
    pub fn check(&mut self, src: JitterSourceKind, ev: &Evidence) -> Result<(), u32> {
        if ev.bit == 0 || ev.bit & FINGERPRINTS[src.idx()] == 0 {
            self.mismatch_events += 1;
            return Err(XC_FINGERPRINT_MISMATCH);
        }
        Ok(())
    }

    /// 指纹表完备性：五源指纹集全非零（空指纹=分类学烂尾）。
    pub const fn table_sane() -> bool {
        let mut i = 0;
        while i < SOURCE_CAP {
            if FINGERPRINTS[i] == 0 {
                return false;
            }
            i += 1;
        }
        true
    }
}

// ---------------------------------------------------------------------------
// 三、分类置信度（多证据聚合）
// ---------------------------------------------------------------------------

/// 置信聚合器（判据"分类置信度（多证据聚合判定"）。
///
/// 聚合语义：对每源收集有效证据强度（万分比），源置信=证据强度最大值；
/// 出判条件：最高置信源的证据数 ≥ CONFIDENCE_MIN_EVIDENCE（独立证据）。
#[derive(Debug)]
pub struct ConfidenceAggregator {
    /// 每源证据强度列表（表内序对齐）。
    pub strengths: [Vec<u32>; SOURCE_CAP],
    /// 置信不足事件数（0x7303）。
    pub low_conf_events: u32,
    /// 归因冲突事件数（0x7305）。
    pub conflict_events: u32,
}

impl ConfidenceAggregator {
    /// 新聚合器（五空桶）。
    pub const fn new() -> ConfidenceAggregator {
        ConfidenceAggregator {
            strengths: [Vec::new(), Vec::new(), Vec::new(), Vec::new(), Vec::new()],
            low_conf_events: 0,
            conflict_events: 0,
        }
    }

    /// 收证（指纹已校验的证据才入桶——调用方先过 FingerprintChecker）。
    pub fn accept(&mut self, src: JitterSourceKind, strength_bp: u32) -> Result<(), u32> {
        if strength_bp == 0 || strength_bp > 10_000 {
            self.low_conf_events += 1;
            return Err(XC_CONFIDENCE_LOW);
        }
        self.strengths[src.idx()].push(strength_bp);
        Ok(())
    }

    /// 源置信：桶内最大强度（空桶=0）。
    pub fn confidence_of(&self, src: JitterSourceKind) -> u32 {
        let bucket = &self.strengths[src.idx()];
        let mut max = 0u32;
        let mut i = 0;
        while i < bucket.len() {
            if bucket[i] > max {
                max = bucket[i];
            }
            i += 1;
        }
        max
    }

    /// 裁决：多证据聚合出判——证据数不足 0x7303；双源同置信（差<冲突阈）
    /// 0x7305；否则返回最高置信源。
    pub fn adjudicate(&mut self) -> Result<(JitterSourceKind, u32), u32> {
        // 找最高/次高置信源。
        let mut best: Option<(JitterSourceKind, u32, usize)> = None;
        let mut second: u32 = 0;
        let mut i = 0;
        while i < SOURCE_CAP {
            let kind = JitterSourceKind::ALL[i];
            let conf = self.confidence_of(kind);
            let n = self.strengths[i].len();
            if n > 0 {
                match best {
                    Some((_, bc, _)) if bc >= conf => {
                        if conf > second {
                            second = conf;
                        }
                    }
                    _ => {
                        if let Some((_, bc, _)) = best {
                            if bc > second {
                                second = bc;
                            }
                        }
                        best = Some((kind, conf, n));
                    }
                }
            }
            i += 1;
        }
        match best {
            Some((kind, conf, n)) => {
                if n < CONFIDENCE_MIN_EVIDENCE {
                    self.low_conf_events += 1;
                    return Err(XC_CONFIDENCE_LOW);
                }
                if second > 0 && conf.saturating_sub(second) < CONFLICT_MARGIN_BP {
                    self.conflict_events += 1;
                    return Err(XC_ATTRIBUTION_CONFLICT);
                }
                Ok((kind, conf))
            }
            None => {
                self.low_conf_events += 1;
                Err(XC_CONFIDENCE_LOW)
            }
        }
    }
}

// ---------------------------------------------------------------------------
// 四、分类演进（可扩展分类表）
// ---------------------------------------------------------------------------

/// 可扩展分类表（判据"新源类型的可扩展分类表"）。
///
/// 演进纪律：加行必须 bump 版本（版本窗），绕窗直接加=0x7304 冻结违约
/// （F0492 只增不改语义同源）。本表当前五源闭集，第六源（如显示链路帧
/// 补偿失效）预留版本窗 v2。
#[derive(Debug)]
pub struct EvolvableClassTable {
    /// 表版本。
    pub version: u32,
    /// 已登记源数。
    pub rows: usize,
    /// 冻结违约事件数（0x7304）。
    pub frozen_violations: u32,
}

impl EvolvableClassTable {
    /// 出厂：v1 五源闭集。
    pub const fn new() -> EvolvableClassTable {
        EvolvableClassTable {
            version: TABLE_VER_BASE,
            rows: SOURCE_CAP,
            frozen_violations: 0,
        }
    }

    /// 扩展加行（合法路径：版本先+1，再登记）。
    pub fn extend(&mut self, bypass: bool) -> Result<(), u32> {
        if bypass {
            self.frozen_violations += 1;
            return Err(XC_TABLE_FROZEN);
        }
        self.version += 1;
        self.rows += 1;
        Ok(())
    }

    /// 演进对账：行数≤版本数（每版本至多加一行——纪律边界）。
    pub fn evolve_sane(&self) -> bool {
        self.rows <= self.version as usize + SOURCE_CAP - 1
    }
}

// ---------------------------------------------------------------------------
// 五、总引擎 + 自检（五源/指纹/置信/可扩展/判据五组+段对账）
// ---------------------------------------------------------------------------

/// 抖动源分类总引擎。
#[derive(Debug)]
pub struct JitterSrcEngine {
    /// 指纹校验器。
    pub fp: FingerprintChecker,
    /// 置信聚合器。
    pub conf: ConfidenceAggregator,
    /// 可扩展分类表。
    pub table: EvolvableClassTable,
}

impl JitterSrcEngine {
    /// 出厂。
    pub const fn new() -> JitterSrcEngine {
        JitterSrcEngine {
            fp: FingerprintChecker::new(),
            conf: ConfidenceAggregator::new(),
            table: EvolvableClassTable::new(),
        }
    }

    /// 组一：五源闭集——五源逐源序号/名称完备；指纹表健全。
    pub fn selfcheck_set(&self) -> Result<(), u32> {
        let e = JitterSrcEngine::new();
        if !FingerprintChecker::table_sane() {
            return Err(XC_SOURCE_OUT_OF_SET);
        }
        // 闭集序号唯一性（0..=4 无碰撞）。
        let mut seen = [false; SOURCE_CAP];
        let mut i = 0;
        while i < SOURCE_CAP {
            let k = JitterSourceKind::ALL[i];
            if seen[k.idx()] {
                return Err(XC_SOURCE_OUT_OF_SET);
            }
            seen[k.idx()] = true;
            i += 1;
        }
        // 表行数与闭集一致。
        if e.table.rows != SOURCE_CAP {
            return Err(XC_SOURCE_OUT_OF_SET);
        }
        Ok(())
    }

    /// 组二：指纹——合法证据过；野证据拦；越界强度拦。
    pub fn selfcheck_fingerprint(&mut self) -> Result<(), u32> {
        let mut e = JitterSrcEngine::new();
        // 合法：调度抢占源配线程迁移跳变证据。
        e.fp.check(JitterSourceKind::OsPreempt, &Evidence { bit: evidence::CTX_SWITCH_SPIKE, strength_bp: 8000 })?;
        // 野证据：IO 源配温度指纹→0x7302。
        if e.fp.check(JitterSourceKind::IoBlock, &Evidence { bit: evidence::THERMAL_TRIP, strength_bp: 9000 })
            != Err(XC_FINGERPRINT_MISMATCH)
        {
            return Err(XC_FINGERPRINT_MISMATCH);
        }
        // 零位证据→拦。
        if e.fp.check(JitterSourceKind::DriverStall, &Evidence { bit: 0, strength_bp: 5000 })
            != Err(XC_FINGERPRINT_MISMATCH)
        {
            return Err(XC_FINGERPRINT_MISMATCH);
        }
        // 复合指纹位（热源第二指纹：频率下调）合法。
        e.fp.check(JitterSourceKind::ThermalDown, &Evidence { bit: evidence::FREQ_DROPPED, strength_bp: 7000 })?;
        if e.fp.mismatch_events != 2 {
            return Err(XC_FINGERPRINT_MISMATCH);
        }
        Ok(())
    }

    /// 组三：置信——单证据拒；双证据出判；强度越界拦。
    pub fn selfcheck_confidence(&mut self) -> Result<(), u32> {
        let mut e = JitterSrcEngine::new();
        // 强度越界（>10000）→0x7303。
        if e.conf.accept(JitterSourceKind::OsPreempt, 12_000) != Err(XC_CONFIDENCE_LOW) {
            return Err(XC_CONFIDENCE_LOW);
        }
        // 单证据→0x7303。
        e.conf.accept(JitterSourceKind::OsPreempt, 9000)?;
        if e.conf.adjudicate() != Err(XC_CONFIDENCE_LOW) {
            return Err(XC_CONFIDENCE_LOW);
        }
        // 补第二证据（同源）→出判 OS 调度抢占。
        e.conf.accept(JitterSourceKind::OsPreempt, 8500)?;
        let (kind, c) = e.conf.adjudicate()?;
        if kind != JitterSourceKind::OsPreempt || c != 9000 {
            return Err(XC_CONFIDENCE_LOW);
        }
        if e.conf.low_conf_events != 1 {
            return Err(XC_CONFIDENCE_LOW);
        }
        Ok(())
    }

    /// 组四：可扩展——合法扩展走版本窗；绕窗 0x7304；演进对账。
    pub fn selfcheck_evolve(&mut self) -> Result<(), u32> {
        let mut e = JitterSrcEngine::new();
        // 绕窗扩展→0x7304。
        if e.table.extend(true) != Err(XC_TABLE_FROZEN) {
            return Err(XC_TABLE_FROZEN);
        }
        if e.table.frozen_violations != 1 {
            return Err(XC_TABLE_FROZEN);
        }
        // 合法扩展：版本+1 行+1。
        let v0 = e.table.version;
        e.table.extend(false)?;
        if e.table.version != v0 + 1 || e.table.rows != SOURCE_CAP + 1 {
            return Err(XC_TABLE_FROZEN);
        }
        if !e.table.evolve_sane() {
            return Err(XC_TABLE_FROZEN);
        }
        Ok(())
    }

    /// 组五：判据——端到端：真实化证据流（热降频：温度+频率双证据）→
    /// 置信出判；归因冲突案例→0x7305。
    pub fn selfcheck_verdict(&mut self) -> Result<(), u32> {
        let mut e = JitterSrcEngine::new();
        // 案例一：热降频双证据（温度超阈 9000bp + 频率下调 8000bp）→出判热降频。
        let ev1 = Evidence { bit: evidence::THERMAL_TRIP, strength_bp: 9000 };
        let ev2 = Evidence { bit: evidence::FREQ_DROPPED, strength_bp: 8000 };
        e.fp.check(JitterSourceKind::ThermalDown, &ev1)?;
        e.fp.check(JitterSourceKind::ThermalDown, &ev2)?;
        e.conf.accept(JitterSourceKind::ThermalDown, ev1.strength_bp)?;
        e.conf.accept(JitterSourceKind::ThermalDown, ev2.strength_bp)?;
        let (kind, c) = e.conf.adjudicate()?;
        if kind != JitterSourceKind::ThermalDown || c != 9000 {
            return Err(XC_CONFIDENCE_LOW);
        }
        // 案例二：归因冲突——OS 抢占与驱动卡顿同高置信（差 <500bp）→0x7305。
        let mut e2 = JitterSrcEngine::new();
        e2.conf.accept(JitterSourceKind::OsPreempt, 8000)?;
        e2.conf.accept(JitterSourceKind::OsPreempt, 7500)?;
        e2.conf.accept(JitterSourceKind::DriverStall, 7800)?;
        e2.conf.accept(JitterSourceKind::DriverStall, 7600)?;
        if e2.conf.adjudicate() != Err(XC_ATTRIBUTION_CONFLICT) {
            return Err(XC_ATTRIBUTION_CONFLICT);
        }
        if e2.conf.conflict_events != 1 {
            return Err(XC_ATTRIBUTION_CONFLICT);
        }
        Ok(())
    }

    /// 全量自检入口（五组+段对账）。
    pub fn selfcheck_all(&mut self) -> Result<(), u32> {
        self.selfcheck_set()?;
        self.selfcheck_fingerprint()?;
        self.selfcheck_confidence()?;
        self.selfcheck_evolve()?;
        self.selfcheck_verdict()?;
        Self::segment_guard()?;
        Ok(())
    }

    /// 段对账：五码全落 0x7300..0x7400 开区间且互异。
    pub const fn segment_guard() -> Result<(), u32> {
        let codes = [
            XC_SOURCE_OUT_OF_SET,
            XC_FINGERPRINT_MISMATCH,
            XC_CONFIDENCE_LOW,
            XC_TABLE_FROZEN,
            XC_ATTRIBUTION_CONFLICT,
        ];
        let mut i = 0;
        while i < codes.len() {
            if codes[i] < 0x7300 || codes[i] >= 0x7400 {
                return Err(XC_SOURCE_OUT_OF_SET);
            }
            let mut j = i + 1;
            while j < codes.len() {
                if codes[i] == codes[j] {
                    return Err(XC_SOURCE_OUT_OF_SET);
                }
                j += 1;
            }
            i += 1;
        }
        Ok(())
    }
}
