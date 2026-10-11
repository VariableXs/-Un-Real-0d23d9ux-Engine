//! vck04_hetprobe — CGPU-F1604 异构能力探测与登记（NPU/DSP/ISP 不猜不造）。
//!
//! 判据原文（施工书锚点 CGPU-F1604）：能力探测：探测流程（启动探测：枚举可
//! 用引擎→逐引擎能力查询→能力登记表构建——探测五步）；探测实现矩阵（NPU：
//! Windows CIM/Intel NPU 驱动接口/AMD XDNA；DSP：平台音频引擎探测；ISP/
//! MEDIA：驱动查询——矩阵表）；能力缓存（探测结果缓存（设备不变则复用——
//! 缓存键（驱动版本+设备 ID——失效重探；部分失败（某引擎探测失败→该引擎
//! 标记不可用（其余照常——失败隔离；能力变更（驱动更新→能力表失效重探——
//! 热失效复用 F1506 模式；虚拟化（无 NPU 虚拟机→空能力表+显性——空态语
//! 义；诚实登记（能力探测不到=不支持（不猜不造——不造假复用；测试（五步/
//! 矩阵/缓存/隔离/空态/不造假六组）。判据：五步、实现矩阵、缓存键、隔离、
//! 空态、诚实、六组、判据。
//!
//! 职责定位：五步探测流水线（枚举→查询→隔离→登记→缓存公示，每步可独立
//! 验证）+ 实现矩阵静态表（三引擎族×各自路线，路线缺席显性登记）+ 缓存键
//! =FNV(驱动版本,设备 ID)（键命中免重探；键失配即热失效全表重探——F1506
//! 模式）+ 失败隔离（单引擎探测异常只降级该引擎，登记表其余行照常公示）+
//! 空态语义（虚拟化环境无引擎→空能力表+Empty 态显性返回，不返回伪数据）+
//! 诚实登记（探测不到=不支持，ProbeFailed 与 NotSupported 是两个不同状态，
//! 不允许把失败包装成"能力未知待定"）。与 vck02_hetres/vck03_hettask 分
//! 工：彼消费能力表做资源/任务分配，此负责把能力表建对。诊断码段
//! 0x5301~0x5305。

use alloc::vec::Vec;

/// 模块版本。
pub const HET_PROBE_VERSION: &str = "K04-hetprobe-v1";

// ---------------------------------------------------------------------------
// 诊断码（XcCode 0x53 段，本会话独占）
// ---------------------------------------------------------------------------

pub const XC_PROBE_STEP_OUT_OF_ORDER: u32 = 0x5301; // 五步乱序（枚举未完成即查询）
pub const XC_CACHE_KEY_STALE: u32 = 0x5302; // 缓存键失配（热失效触发）
pub const XC_MATRIX_ROUTE_MISSING: u32 = 0x5303; // 实现矩阵路线缺席（显性登记非静默）
pub const XC_ISOLATION_LEAK: u32 = 0x5304; // 隔离泄漏（失败引擎数据混入登记表）
pub const XC_HONESTY_VIOLATION: u32 = 0x5305; // 诚实违约（失败被包装成"支持"）

// ---------------------------------------------------------------------------
// 常量
// ---------------------------------------------------------------------------

/// 五步流水线步数。
pub const PROBE_STEPS: usize = 5;
/// 缓存表容量。
pub const CACHE_SLOTS: usize = 8;

/// 引擎族三分类（判据矩阵行）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum EngineKind {
    Npu,
    Dsp,
    IspMedia,
}

impl EngineKind {
    pub const ALL: [EngineKind; 3] = [EngineKind::Npu, EngineKind::Dsp, EngineKind::IspMedia];

    pub const fn zh(self) -> &'static str {
        match self {
            EngineKind::Npu => "NPU",
            EngineKind::Dsp => "DSP",
            EngineKind::IspMedia => "ISP/MEDIA",
        }
    }
}

/// 实现矩阵路线（判据"NPU：Windows CIM/Intel NPU 驱动接口/AMD XDNA；DSP：
/// 平台音频引擎探测；ISP/MEDIA：驱动查询"——静态闭集表）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ProbeRoute {
    /// NPU 路线一：Windows CIM（分类信息模型）。
    NpuWindowsCim,
    /// NPU 路线二：Intel NPU 驱动接口。
    NpuIntelDriver,
    /// NPU 路线三：AMD XDNA。
    NpuAmdXdna,
    /// DSP 路线：平台音频引擎探测。
    DspAudioEngine,
    /// ISP/MEDIA 路线：驱动查询。
    MediaDriverQuery,
}

impl ProbeRoute {
    /// 路线所属引擎族（矩阵表行归属）。
    pub const fn kind(self) -> EngineKind {
        match self {
            ProbeRoute::NpuWindowsCim | ProbeRoute::NpuIntelDriver | ProbeRoute::NpuAmdXdna => {
                EngineKind::Npu
            }
            ProbeRoute::DspAudioEngine => EngineKind::Dsp,
            ProbeRoute::MediaDriverQuery => EngineKind::IspMedia,
        }
    }

    pub const fn zh(self) -> &'static str {
        match self {
            ProbeRoute::NpuWindowsCim => "Windows CIM",
            ProbeRoute::NpuIntelDriver => "Intel NPU 驱动接口",
            ProbeRoute::NpuAmdXdna => "AMD XDNA",
            ProbeRoute::DspAudioEngine => "平台音频引擎",
            ProbeRoute::MediaDriverQuery => "媒体驱动查询",
        }
    }
}

/// 全矩阵路线（判据"矩阵表"——静态可查）。
pub const MATRIX_ROUTES: [ProbeRoute; 5] = [
    ProbeRoute::NpuWindowsCim,
    ProbeRoute::NpuIntelDriver,
    ProbeRoute::NpuAmdXdna,
    ProbeRoute::DspAudioEngine,
    ProbeRoute::MediaDriverQuery,
];

/// 单引擎探测结果（诚实三态：可配/不可用/探测失败——三态不许混写）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ProbeOutcome {
    /// 可用：实测查询成功，附能力位集。
    Available(u32),
    /// 不可用：探测到了但设备/驱动明确声明不支持（诚实登记"不支持"）。
    NotSupported,
    /// 探测失败：路线异常（隔离对象——与"不支持"严格区分）。
    ProbeFailed,
}

/// 单引擎登记行。
#[derive(Clone, Copy, Debug)]
pub struct EngineRow {
    pub kind: EngineKind,
    /// 命中路线（None=全路线缺席——矩阵缺口显性化）。
    pub route: Option<ProbeRoute>,
    pub outcome: ProbeOutcome,
    /// 是否被隔离（探测失败行——登记表中存在但标注隔离，数据不得被消费）。
    pub isolated: bool,
}

/// 探测五步流水线状态（判据"探测五步"）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ProbeStep {
    Idle,
    Step1Enumerate,
    Step2Query,
    Step3Isolate,
    Step4Register,
    Step5Publish,
    Done,
}

impl ProbeStep {
    pub const fn ordinal(self) -> usize {
        match self {
            ProbeStep::Idle => 0,
            ProbeStep::Step1Enumerate => 1,
            ProbeStep::Step2Query => 2,
            ProbeStep::Step3Isolate => 3,
            ProbeStep::Step4Register => 4,
            ProbeStep::Step5Publish => 5,
            ProbeStep::Done => 6,
        }
    }
}

/// 缓存条目：键=驱动版本+设备 ID 的 FNV。
#[derive(Clone, Copy)]
pub struct CacheEntry {
    pub key: u64,
    /// 缓存的能力表（隔离后的干净表）。
    pub rows: [EngineRow; 3],
    /// 缓存生成序号（调试/遥测）。
    pub gen: u64,
}

/// 缓存键计算：FNV(驱动版本, 设备 ID)。
pub const fn cache_key(driver_ver: u32, device_id: u32) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    h = fnv_byte(h, (driver_ver & 0xFF) as u8);
    h = fnv_byte(h, ((driver_ver >> 8) & 0xFF) as u8);
    h = fnv_byte(h, ((driver_ver >> 16) & 0xFF) as u8);
    h = fnv_byte(h, ((driver_ver >> 24) & 0xFF) as u8);
    h = fnv_byte(h, (device_id & 0xFF) as u8);
    h = fnv_byte(h, ((device_id >> 8) & 0xFF) as u8);
    h = fnv_byte(h, ((device_id >> 16) & 0xFF) as u8);
    h = fnv_byte(h, ((device_id >> 24) & 0xFF) as u8);
    h
}

pub const fn fnv_byte(h: u64, b: u8) -> u64 {
    (h ^ (b as u64)).wrapping_mul(0x1000_0000_01b3)
}

/// 异构探测引擎：五步流水线+缓存+隔离+诚实登记。
pub struct HetProbeEngine {
    /// 五步当前态。
    step: ProbeStep,
    /// 步一产物：枚举到的引擎族。
    enumerated: Vec<EngineKind>,
    /// 步二产物：逐引擎原始查询结果（含失败）。
    raw: Vec<(EngineKind, ProbeOutcome)>,
    /// 步四产物：登记表（隔离后）。
    pub registry: Vec<EngineRow>,
    /// 缓存槽。
    cache: Vec<CacheEntry>,
    /// 当前键。
    cur_key: u64,
    /// 虚拟化空态（判据"无 NPU 虚拟机→空能力表+显性"）。
    pub virtualized_empty: bool,
    /// 遥测。
    pub cache_hits: u64,
    pub cache_misses: u64,
    pub hot_invalidations: u64,
    pub isolated_engines: u64,
    pub honesty_audits: u64,
}

impl HetProbeEngine {
    pub fn new() -> Self {
        HetProbeEngine {
            step: ProbeStep::Idle,
            enumerated: Vec::new(),
            raw: Vec::new(),
            registry: Vec::new(),
            cache: Vec::with_capacity(CACHE_SLOTS),
            cur_key: 0,
            virtualized_empty: false,
            cache_hits: 0,
            cache_misses: 0,
            hot_invalidations: 0,
            isolated_engines: 0,
            honesty_audits: 0,
        }
    }

    /// 步一：枚举可用引擎（虚拟机环境传空表→空态）。
    pub fn step1_enumerate(&mut self, kinds: &[EngineKind]) -> Result<(), u32> {
        if self.step != ProbeStep::Idle && self.step != ProbeStep::Done {
            return Err(XC_PROBE_STEP_OUT_OF_ORDER);
        }
        self.enumerated = Vec::new();
        let mut i = 0;
        while i < kinds.len() {
            self.enumerated.push(kinds[i]);
            i += 1;
        }
        self.virtualized_empty = kinds.len() == 0; // 空枚举=虚拟化空态（显性）
        self.raw = Vec::new();
        self.registry = Vec::new();
        self.step = ProbeStep::Step1Enumerate;
        Ok(())
    }

    /// 步二：逐引擎能力查询（路线矩阵选路：按引擎族取首条路线；模拟侧由
    /// 调用方注入结果——本模块不做硬件访问，HAL 隔离纪律同 cgc06）。
    pub fn step2_query(
        &mut self,
        results: &[(EngineKind, ProbeOutcome)],
    ) -> Result<(), u32> {
        if self.step != ProbeStep::Step1Enumerate {
            return Err(XC_PROBE_STEP_OUT_OF_ORDER);
        }
        self.raw = Vec::new();
        let mut i = 0;
        while i < results.len() {
            self.raw.push(results[i]);
            i += 1;
        }
        self.step = ProbeStep::Step2Query;
        Ok(())
    }

    /// 步三：失败隔离——ProbeFailed 行标记隔离；Available/NotSupported 保留。
    pub fn step3_isolate(&mut self) -> Result<(), u32> {
        if self.step != ProbeStep::Step2Query {
            return Err(XC_PROBE_STEP_OUT_OF_ORDER);
        }
        let mut i = 0;
        while i < self.raw.len() {
            let (kind, outcome) = self.raw[i];
            let isolated = outcome == ProbeOutcome::ProbeFailed;
            if isolated {
                self.isolated_engines += 1;
            }
            // 隔离行以 ProbeFailed 原态入登记表但打隔离标（可审计不可消费）。
            self.registry.push(EngineRow {
                kind,
                route: self.first_route_of(kind),
                outcome,
                isolated,
            });
            i += 1;
        }
        self.step = ProbeStep::Step3Isolate;
        Ok(())
    }

    /// 步四：登记表构建——剔除隔离行后生成干净能力表。
    pub fn step4_register(&mut self) -> Result<(), u32> {
        if self.step != ProbeStep::Step3Isolate {
            return Err(XC_PROBE_STEP_OUT_OF_ORDER);
        }
        // 诚实审计：登记表中不允许存在 isolated=true（消费面），隔离行已在此步剥离。
        let mut clean = Vec::new();
        let mut i = 0;
        while i < self.registry.len() {
            if self.registry[i].isolated {
                i += 1;
                continue;
            }
            clean.push(self.registry[i]);
            i += 1;
        }
        self.registry = clean;
        self.step = ProbeStep::Step4Register;
        Ok(())
    }

    /// 步五：缓存写入与公示（键命中则跳过重探直接复用）。
    pub fn step5_publish(&mut self, driver_ver: u32, device_id: u32) -> Result<bool, u32> {
        if self.step != ProbeStep::Step4Register {
            return Err(XC_PROBE_STEP_OUT_OF_ORDER);
        }
        let key = cache_key(driver_ver, device_id);
        self.cur_key = key;
        // 缓存命中检查。
        let mut i = 0;
        while i < self.cache.len() {
            if self.cache[i].key == key {
                self.cache_hits += 1;
                // 复用：登记表从缓存恢复（设备不变不重探——判据）。
                self.registry = Vec::new();
                let mut r = 0;
                while r < 3 {
                    self.registry.push(self.cache[i].rows[r]);
                    r += 1;
                }
                self.step = ProbeStep::Done;
                return Ok(true);
            }
            i += 1;
        }
        self.cache_misses += 1;
        // 缓存写入（隔离后干净表；空表也缓存——空态也是确定事实）。
        if self.cache.len() >= CACHE_SLOTS {
            self.cache.remove(0); // 简单 FIFO 淘汰
        }
        let mut rows = [EngineRow {
            kind: EngineKind::Npu,
            route: None,
            outcome: ProbeOutcome::NotSupported,
            isolated: false,
        }; 3];
        let mut j = 0;
        while j < 3 {
            rows[j].kind = EngineKind::ALL[j];
            // 登记表中找该族行；没有=该族未探测（诚实：NotSupported 而非凭空）。
            let mut found = None;
            let mut k = 0;
            while k < self.registry.len() {
                if self.registry[k].kind == EngineKind::ALL[j] {
                    found = Some(self.registry[k]);
                    break;
                }
                k += 1;
            }
            match found {
                Some(row) => rows[j] = row,
                None => rows[j].kind = EngineKind::ALL[j], // 缺席行保持 NotSupported 语义
            }
            j += 1;
        }
        self.cache.push(CacheEntry {
            key,
            rows,
            gen: self.cache_misses,
        });
        self.step = ProbeStep::Done;
        Ok(false)
    }

    /// 热失效：驱动更新→全表失效重探（判据"能力变更"复用 F1506 模式）。
    pub fn hot_invalidate(&mut self, driver_ver: u32, device_id: u32) -> Result<bool, u32> {
        let key = cache_key(driver_ver, device_id);
        // 新键与当前键失配 → 表作废重探。
        if key != self.cur_key {
            self.hot_invalidations += 1;
            self.registry = Vec::new();
            self.raw = Vec::new();
            self.enumerated = Vec::new();
            self.step = ProbeStep::Idle; // 回到步一待重探
            return Ok(true);
        }
        Ok(false) // 键未变：无失效
    }

    /// 能力查询（消费面）：引擎族是否可用（隔离行与缺席行均=不可用）。
    pub fn capability(&self, kind: EngineKind) -> Option<u32> {
        let mut i = 0;
        while i < self.registry.len() {
            if self.registry[i].kind == kind {
                // 隔离行不外泄（防御性——step4 已剥离，这里兜底）。
                if self.registry[i].isolated {
                    return None;
                }
                match self.registry[i].outcome {
                    ProbeOutcome::Available(bits) => return Some(bits),
                    _ => return None,
                }
            }
            i += 1;
        }
        None
    }

    /// 空态查询：虚拟化环境显性 Empty（判据"空能力表+显性"）。
    pub const fn is_empty_state(&self) -> bool {
        self.virtualized_empty
    }

    /// 引擎族首条矩阵路线（矩阵表查询；缺席返回 Err 显性）。
    fn first_route_of(&self, kind: EngineKind) -> Option<ProbeRoute> {
        let mut i = 0;
        while i < MATRIX_ROUTES.len() {
            if MATRIX_ROUTES[i].kind() == kind {
                return Some(MATRIX_ROUTES[i]);
            }
            i += 1;
        }
        None
    }

    /// 流水线当前步。
    pub const fn current_step(&self) -> ProbeStep {
        self.step
    }
}

// ---------------------------------------------------------------------------
// 自检（判据六组：五步 / 矩阵 / 缓存 / 隔离 / 空态 / 不造假）
// ---------------------------------------------------------------------------

impl HetProbeEngine {
    /// 组一：五步——乱序拒绝、顺序通过。
    pub fn selfcheck_steps(&mut self) -> Result<(), u32> {
        let mut e = HetProbeEngine::new();
        // 未枚举就查询：拒绝。
        let r0 = e.step2_query(&[(EngineKind::Npu, ProbeOutcome::Available(0xF))]);
        if r0 != Err(XC_PROBE_STEP_OUT_OF_ORDER) {
            return Err(XC_PROBE_STEP_OUT_OF_ORDER);
        }
        let kinds = [EngineKind::Npu, EngineKind::Dsp];
        e.step1_enumerate(&kinds)?;
        e.step2_query(&[
            (EngineKind::Npu, ProbeOutcome::Available(0b11)),
            (EngineKind::Dsp, ProbeOutcome::NotSupported),
        ])?;
        e.step3_isolate()?;
        e.step4_register()?;
        let hit = e.step5_publish(0x0501, 0x7D00)?;
        if hit {
            return Err(XC_CACHE_KEY_STALE);
        }
        if e.current_step() != ProbeStep::Done {
            return Err(XC_PROBE_STEP_OUT_OF_ORDER);
        }
        Ok(())
    }

    /// 组二：矩阵——五路线三引擎族归属正确，族内路线可查。
    pub fn selfcheck_matrix(&mut self) -> Result<(), u32> {
        let mut i = 0;
        while i < MATRIX_ROUTES.len() {
            let k = MATRIX_ROUTES[i].kind();
            match MATRIX_ROUTES[i] {
                ProbeRoute::NpuWindowsCim
                | ProbeRoute::NpuIntelDriver
                | ProbeRoute::NpuAmdXdna => {
                    if k != EngineKind::Npu {
                        return Err(XC_MATRIX_ROUTE_MISSING);
                    }
                }
                ProbeRoute::DspAudioEngine => {
                    if k != EngineKind::Dsp {
                        return Err(XC_MATRIX_ROUTE_MISSING);
                    }
                }
                ProbeRoute::MediaDriverQuery => {
                    if k != EngineKind::IspMedia {
                        return Err(XC_MATRIX_ROUTE_MISSING);
                    }
                }
            }
            i += 1;
        }
        // 矩阵覆盖三族：每族至少一条路线。
        let mut fam = 0;
        let mut f = 0;
        while f < EngineKind::ALL.len() {
            let mut j = 0;
            while j < MATRIX_ROUTES.len() {
                if MATRIX_ROUTES[j].kind() == EngineKind::ALL[f] {
                    fam += 1;
                    break;
                }
                j += 1;
            }
            f += 1;
        }
        if fam != 3 {
            return Err(XC_MATRIX_ROUTE_MISSING);
        }
        Ok(())
    }

    /// 组三：缓存——同键命中复用、异键重探。
    pub fn selfcheck_cache(&mut self) -> Result<(), u32> {
        let mut e = HetProbeEngine::new();
        let kinds = [EngineKind::Npu];
        e.step1_enumerate(&kinds)?;
        e.step2_query(&[(EngineKind::Npu, ProbeOutcome::Available(0x1))])?;
        e.step3_isolate()?;
        e.step4_register()?;
        let _ = e.step5_publish(0x0501, 0x7D00)?;
        let miss1 = e.cache_misses;
        // 同键再次五步 → 命中。
        e.step1_enumerate(&kinds)?;
        e.step2_query(&[(EngineKind::Npu, ProbeOutcome::Available(0x2))])?;
        e.step3_isolate()?;
        e.step4_register()?;
        let hit = e.step5_publish(0x0501, 0x7D00)?;
        if !hit || e.cache_hits != 1 || e.cache_misses != miss1 {
            return Err(XC_CACHE_KEY_STALE);
        }
        // 命中后能力应来自缓存（0x1）而非本次查询（0x2）。
        if e.capability(EngineKind::Npu) != Some(0x1) {
            return Err(XC_CACHE_KEY_STALE);
        }
        // 换驱动版本 → 热失效。
        let inv = e.hot_invalidate(0x0502, 0x7D00)?;
        if !inv || e.hot_invalidations != 1 || e.current_step() != ProbeStep::Idle {
            return Err(XC_CACHE_KEY_STALE);
        }
        Ok(())
    }

    /// 组四：隔离——失败引擎不污染登记表，其余照常。
    pub fn selfcheck_isolation(&mut self) -> Result<(), u32> {
        let mut e = HetProbeEngine::new();
        let kinds = [EngineKind::Npu, EngineKind::Dsp, EngineKind::IspMedia];
        e.step1_enumerate(&kinds)?;
        e.step2_query(&[
            (EngineKind::Npu, ProbeOutcome::Available(0b111)),
            (EngineKind::Dsp, ProbeOutcome::ProbeFailed), // 失败：隔离对象
            (EngineKind::IspMedia, ProbeOutcome::NotSupported),
        ])?;
        e.step3_isolate()?;
        if e.isolated_engines != 1 {
            return Err(XC_ISOLATION_LEAK);
        }
        e.step4_register()?;
        // 登记表不得含隔离行（step4 剥离）。
        let mut i = 0;
        while i < e.registry.len() {
            if e.registry[i].isolated {
                return Err(XC_ISOLATION_LEAK);
            }
            i += 1;
        }
        if e.registry.len() != 2 {
            return Err(XC_ISOLATION_LEAK);
        }
        // NPU 照常可用、DSP 不可用（失败≠支持）。
        if e.capability(EngineKind::Npu) != Some(0b111) {
            return Err(XC_ISOLATION_LEAK);
        }
        if e.capability(EngineKind::Dsp).is_some() {
            return Err(XC_ISOLATION_LEAK);
        }
        Ok(())
    }

    /// 组五：空态——虚拟机空枚举→空能力表+显性 Empty。
    pub fn selfcheck_empty(&mut self) -> Result<(), u32> {
        let mut e = HetProbeEngine::new();
        e.step1_enumerate(&[])?;
        e.step2_query(&[])?;
        e.step3_isolate()?;
        e.step4_register()?;
        let _ = e.step5_publish(0x0501, 0x9000)?;
        if !e.is_empty_state() {
            return Err(XC_ISOLATION_LEAK);
        }
        if e.registry.len() != 0 {
            return Err(XC_ISOLATION_LEAK);
        }
        if e.capability(EngineKind::Npu).is_some() {
            return Err(XC_HONESTY_VIOLATION);
        }
        Ok(())
    }

    /// 组六：不造假——NotSupported 与 ProbeFailed 语义不得互换包装。
    pub fn selfcheck_honesty(&mut self) -> Result<(), u32> {
        // NotSupported 行 capability 必须为 None（探测到但不支持≠可用）。
        let mut e = HetProbeEngine::new();
        let kinds = [EngineKind::Npu];
        e.step1_enumerate(&kinds)?;
        e.step2_query(&[(EngineKind::Npu, ProbeOutcome::NotSupported)])?;
        e.step3_isolate()?;
        e.step4_register()?;
        e.honesty_audits += 1;
        if e.capability(EngineKind::Npu).is_some() {
            return Err(XC_HONESTY_VIOLATION);
        }
        // 登记行 outcome 仍是 NotSupported（没有偷换成 Available）。
        if e.registry[0].outcome != ProbeOutcome::NotSupported {
            return Err(XC_HONESTY_VIOLATION);
        }
        // 缺席引擎在缓存行内保持 NotSupported 语义（不凭空造 Available）。
        let mut e2 = HetProbeEngine::new();
        let kinds2 = [EngineKind::Npu];
        e2.step1_enumerate(&kinds2)?;
        e2.step2_query(&[(EngineKind::Npu, ProbeOutcome::Available(0x1))])?;
        e2.step3_isolate()?;
        e2.step4_register()?;
        let _ = e2.step5_publish(0x0501, 0x7D00)?;
        // DSP 未探测：缓存 rows 中 DSP 行必须 NotSupported。
        let mut i = 0;
        while i < e2.cache.len() {
            let rows = e2.cache[i].rows;
            if rows[1].kind == EngineKind::Dsp && rows[1].outcome != ProbeOutcome::NotSupported {
                return Err(XC_HONESTY_VIOLATION);
            }
            i += 1;
        }
        Ok(())
    }

    /// 全量自检入口。
    pub fn selfcheck_all(&mut self) -> Result<(), u32> {
        self.selfcheck_steps()?;
        self.selfcheck_matrix()?;
        self.selfcheck_cache()?;
        self.selfcheck_isolation()?;
        self.selfcheck_empty()?;
        self.selfcheck_honesty()?;
        Ok(())
    }
}
