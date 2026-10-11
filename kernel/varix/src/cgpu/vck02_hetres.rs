//! CGPU-F1602 · 异构资源抽象（CGPU-K 域 · 异构扩展域 · K02 组 · 目标 340 行）。
//!
//! **锚点**：`docs/Varix/VE-STAR-II/CGPU Varix STAR II · 总纲与施工书.md#CGPU-F1602`
//!
//! **判据（锚点原文）**：trait 分层、四类型卡、能力 schema、句柄统一、
//! 热注册、mock、六组、判据。
//!
//! **职责定位（锚点原文）**：资源抽象：DeviceTrait 扩展（GPU DeviceTrait
//! 泛化为 ComputeDevice trait——能力方法（capabilities()/submit()/
//! sync()/memory_stats()——四核心方法+可选扩展——trait 分层（核心必选/
//! 特性可选——特性位图）；资源类型卡（每引擎类型卡：NPU（张量算力/
//! 模型格式/批处理特性）/DSP（实时约束/任务粒度）/ISP（流水线固定
//! 功能）/MEDIA（编解码固定）——四类型卡）；能力描述 schema（统一能力
//! 描述结构（算力等级/内存/功耗系数/延迟特性）——schema 版本化——
//! 能力即数据；异构句柄（跨引擎资源句柄统一（缓冲区句柄可跨引擎引用
//! （带位置标签）——句柄统一规范；引擎注册（引擎运行时注册/注销——
//! 热注册语义；mock 引擎（测试用 mock ComputeDevice——F1454 模式复用）。
//!
//! ## 一、trait 分层（判据一）
//!
//! 核心必选（[`ComputeDevice`] 四核心方法）与特性可选（[`BatchExt`]/
//! [`RealtimeExt`] 扩展 trait + 特性位图）分层：核心是所有引擎的公约数
//! （能力查询/任务提交/同步/显存统计——没有这四样的不叫计算设备）；
//! 批处理与实时约束是**能力**不是承诺（特性位图声明有没有——调用前
//! 查位图，不查就调是调用方的错，trait 层不兜底）。
//!
//! ## 二、四类型卡（判据二）
//!
//! 引擎类型卡（[`EngineKind`] 四元闭集 + 各卡结构）：NPU（张量算力
//! TOPS/模型格式闭集/批处理特性）、DSP（实时约束档/任务粒度）、ISP
//! （流水线固定功能清单）、MEDIA（编解码固定能力表）。卡是**静态规格**
//! ——运行时能力走 schema，卡管「这类引擎长什么样」。
//!
//! ## 三、能力 schema（判据三）
//!
//! 统一能力描述（[`HetCaps`]）：算力等级（分档整数）/内存（字节）/
//! 功耗系数（ppm）/延迟特性（档位闭集）——schema 版本化
//! （[`HET_SCHEMA_VERSION`]），**能力即数据**：调度器/融合规划器拿数据
//! 做决策，不反编译引擎类型 if 链。
//!
//! ## 四、句柄统一与热注册（判据四、五）
//!
//! 异构句柄（[`HetHandle`]）：缓冲区句柄统一 = 全局资源 id + home 引擎
//! id + 位置标签（驻留地闭集：本机/对端/主机——跨引擎引用先看位置标签，
//! 不在本地先走搬运）；热注册（[`EngineRegistry`]：运行时注册/注销——
//! 注销时在册任务账必须已清，带任务注销显性拒）。
//!
//! ## 五、mock 引擎（判据六）
//!
//! mock（[`MockEngine`]）：实现核心 trait 的确定性假引擎（F1454 模式
//! 复用）——提交即记账、同步即清账、显存可配——下游调度/选路逻辑在
//! mock 上可全链演练（mock 的结论不是性能结论，是逻辑结论）。
//!
//! **对接**：F1601（K 域开工——本模块是其资源抽象面）；F1454（mock
//! 模式源）；F1603+（异构调度的下游）；F0322（预算——算力等级联动）。
//! 零 panic 面、零 IO、零墙钟、no_std。

use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 一、规格常量
// ---------------------------------------------------------------------------

/// 本项版本。
pub const HET_RES_VERSION: &str = "K02-hetres-v1";

/// 能力 schema 版本（消费方按版本对账）。
pub const HET_SCHEMA_VERSION: &str = "het-caps-v2";

/// 特性位图：批处理位。
pub const FEAT_BATCH: u32 = 1 << 0;
/// 特性位图：实时约束位。
pub const FEAT_REALTIME: u32 = 1 << 1;
/// 特性位图：跨引擎句柄位。
pub const FEAT_CROSS_HANDLE: u32 = 1 << 2;

// ---------------------------------------------------------------------------
// 二、诊断码（0x3B 段独占：K02 异构资源面）
// ---------------------------------------------------------------------------

/// 诊断码闭集（0x3B 段）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum XcCode {
    /// 引擎未注册（句柄指向注销/从未注册的引擎）。
    EngineUnregistered,
    /// 引擎带任务注销（热注销前置不满足）。
    HotRemoveWithTasks,
    /// 特性未声明（调用了位图未声明的扩展能力）。
    FeatureNotDeclared,
    /// 句柄位置不可达（资源在对端而本引擎无跨引擎位）。
    HandleRemoteUnreachable,
}

impl XcCode {
    /// 人读诊断。
    pub const fn detail(self) -> &'static str {
        match self {
            XcCode::EngineUnregistered => "引擎未注册：句柄指向的引擎已注销或从未注册，先查注册台账",
            XcCode::HotRemoveWithTasks => "带任务注销：热注销前置=在册任务账已清，先排干任务再注销",
            XcCode::FeatureNotDeclared => "特性未声明：扩展能力调用前先查特性位图，位图不说有没有就不许调",
            XcCode::HandleRemoteUnreachable => "句柄在对端且本引擎无跨引擎位：先走显式搬运（F1281 传输面）",
        }
    }
}

// ---------------------------------------------------------------------------
// 三、trait 分层（判据一）
// ---------------------------------------------------------------------------

/// 能力描述 schema（判据三：能力即数据；版本化 HET_SCHEMA_VERSION）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct HetCaps {
    /// 算力等级（0-9 分档；跨类型不可直接比大小——档位语义按类型卡解释）。
    pub compute_level: u8,
    /// 可用内存（字节）。
    pub memory_bytes: u64,
    /// 功耗系数（ppm；满载功率/基准功率——功耗面换算输入）。
    pub power_coeff_ppm: u32,
    /// 延迟特性档（0=硬实时 1=软实时 2=吞吐优先 3=离线批）。
    pub latency_profile: u8,
    /// 特性位图（FEAT_* 或集）。
    pub feature_mask: u32,
}

impl HetCaps {
    /// 特性判定（调用扩展前必查）。
    pub const fn has_feature(&self, feat: u32) -> bool {
        (self.feature_mask & feat) == feat
    }

    /// 延迟档名（人读）。
    pub const fn latency_zh(&self) -> &'static str {
        match self.latency_profile {
            0 => "硬实时",
            1 => "软实时",
            2 => "吞吐优先",
            _ => "离线批",
        }
    }
}

/// 核心必选 trait（四核心方法；所有引擎的公约数）。
pub trait ComputeDevice {
    /// 引擎 id。
    fn engine_id(&self) -> u32;
    /// 能力查询（能力即数据）。
    fn capabilities(&self) -> HetCaps;
    /// 任务提交（返回任务账 id；确定性序由实现保证）。
    fn submit(&mut self, kind: u32, payload_bytes: u64) -> Result<u64, XcCode>;
    /// 同步（清空在册任务账；返回清掉的账数）。
    fn sync(&mut self) -> u64;
    /// 显存统计（已用/容量，字节）。
    fn memory_stats(&self) -> (u64, u64);
}

/// 批处理扩展 trait（特性可选——FEAT_BATCH 声明后才允许调用）。
pub trait BatchExt: ComputeDevice {
    /// 最大批尺寸（一批最多任务数）。
    fn max_batch(&self) -> u32;
}

/// 实时扩展 trait（特性可选——FEAT_REALTIME 声明后才允许调用）。
pub trait RealtimeExt: ComputeDevice {
    /// 最坏中断延迟上界（μs；实时档才有效）。
    fn wcet_us(&self) -> u32;
}

// ---------------------------------------------------------------------------
// 四、四类型卡（判据二）
// ---------------------------------------------------------------------------

/// 引擎类型闭集（四元）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EngineKind {
    /// NPU（张量算力/模型格式/批处理）。
    Npu,
    /// DSP（实时约束/任务粒度）。
    Dsp,
    /// ISP（流水线固定功能）。
    Isp,
    /// MEDIA（编解码固定）。
    Media,
    /// GPU（通用图形/计算引擎——跨卡资源一致性同步用）。
    Gpu,
}

impl EngineKind {
    /// 类型名（人读）。
    pub const fn zh(self) -> &'static str {
        match self {
            EngineKind::Npu => "NPU",
            EngineKind::Dsp => "DSP",
            EngineKind::Isp => "ISP",
            EngineKind::Media => "MEDIA",
            EngineKind::Gpu => "GPU",
        }
    }

    /// 类型卡摘要（静态规格一行；判据侧独立重排对拍）。
    pub const fn card_line(self) -> &'static str {
        match self {
            EngineKind::Npu => "张量算力档位+模型格式闭集+批处理特性（FEAT_BATCH 常见）",
            EngineKind::Dsp => "硬实时档+细粒度任务（FEAT_REALTIME 常见）",
            EngineKind::Isp => "流水线固定功能（传感器直通，无通用任务面）",
            EngineKind::Media => "编解码固定（H.264/H.265/AV1 档位固定）",
            EngineKind::Gpu => "通用图形/计算档+跨卡一致性（FEAT_COHERENCE 相关）",
        }
    }
}

/// 四类型卡全集（判据侧可独立重排全等对拍）。
pub const ENGINE_KINDS: [EngineKind; 4] = [
    EngineKind::Npu,
    EngineKind::Dsp,
    EngineKind::Isp,
    EngineKind::Media,
];

// ---------------------------------------------------------------------------
// 五、句柄统一（判据四）
// ---------------------------------------------------------------------------

/// 位置标签闭集（句柄驻留地）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LocationTag {
    /// 本机（本引擎可直接寻址）。
    Local,
    /// 对端（跨引擎引用需搬运或跨引擎位）。
    Peer,
    /// 主机（CPU 内存——访问走传输面）。
    Host,
}

impl LocationTag {
    /// 位置名（人读）。
    pub const fn zh(self) -> &'static str {
        match self {
            LocationTag::Local => "本机",
            LocationTag::Peer => "对端",
            LocationTag::Host => "主机",
        }
    }
}

/// 异构句柄（统一规范：全局 id + home 引擎 + 位置标签）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct HetHandle {
    /// 全局资源 id（跨引擎唯一——分配面全局单调）。
    pub resource_id: u64,
    /// home 引擎 id。
    pub home_engine: u32,
    /// 位置标签（跨引擎引用先看这里）。
    pub location: LocationTag,
}

/// 句柄可达性判定（本机直达；对端/主机需跨引擎位——不可达显性拒，
/// 搬运是显式动作不是隐式魔法；枚举相等比较非 const 故用普通 fn）。
pub fn handle_reachable(h: &HetHandle, engine_id: u32, caps: &HetCaps) -> Result<(), XcCode> {
    if h.home_engine == engine_id && h.location == LocationTag::Local {
        return Ok(());
    }
    if caps.has_feature(FEAT_CROSS_HANDLE) {
        return Ok(());
    }
    Err(XcCode::HandleRemoteUnreachable)
}

// ---------------------------------------------------------------------------
// 六、热注册（判据五）
// ---------------------------------------------------------------------------

/// 在册引擎行。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EngineRow {
    /// 引擎 id。
    pub engine_id: u32,
    /// 类型。
    pub kind: EngineKind,
    /// 能力快照（注册时建模；能力变更重新注册——快照不热改）。
    pub caps: HetCaps,
    /// 在册任务数（热注销前置账）。
    pub inflight: u64,
}

/// 引擎注册台账（热注册语义：运行时注册/注销）。
#[derive(Clone, Debug, Default)]
pub struct EngineRegistry {
    rows: Vec<EngineRow>,
}

impl EngineRegistry {
    /// 热注册（同 id 重复注册拒绝——重注册先注销）。
    pub fn hot_register(&mut self, engine_id: u32, kind: EngineKind, caps: HetCaps) -> Result<(), XcCode> {
        if self.find(engine_id).is_some() {
            return Err(XcCode::EngineUnregistered); // 语义：已注册按未受理拒
        }
        self.rows.push(EngineRow {
            engine_id,
            kind,
            caps,
            inflight: 0,
        });
        Ok(())
    }

    /// 热注销（带任务显性拒——先排干）。
    pub fn hot_unregister(&mut self, engine_id: u32) -> Result<(), XcCode> {
        let mut idx = None;
        let mut i = 0usize;
        while i < self.rows.len() {
            let hit = match self.rows.get(i) {
                Some(r) => r.engine_id == engine_id,
                None => false,
            };
            if hit {
                idx = Some(i);
                break;
            }
            i += 1;
        }
        let i = match idx {
            Some(v) => v,
            None => return Err(XcCode::EngineUnregistered),
        };
        let busy = match self.rows.get(i) {
            Some(r) => r.inflight > 0,
            None => return Err(XcCode::EngineUnregistered),
        };
        if busy {
            return Err(XcCode::HotRemoveWithTasks);
        }
        self.rows.remove(i);
        Ok(())
    }

    /// 查行（只读）。
    pub fn find(&self, engine_id: u32) -> Option<&EngineRow> {
        let mut i = 0usize;
        while i < self.rows.len() {
            if let Some(r) = self.rows.get(i) {
                if r.engine_id == engine_id {
                    return Some(r);
                }
            }
            i += 1;
        }
        None
    }

    /// 任务入账/出账（句柄提交与同步的联动账）。
    pub fn adjust_inflight(&mut self, engine_id: u32, delta: i64) -> Result<(), XcCode> {
        let mut i = 0usize;
        while i < self.rows.len() {
            let hit = match self.rows.get(i) {
                Some(r) => r.engine_id == engine_id,
                None => false,
            };
            if hit {
                if let Some(r) = self.rows.get_mut(i) {
                    if delta >= 0 {
                        r.inflight = r.inflight.saturating_add(delta as u64);
                    } else {
                        r.inflight = r.inflight.saturating_sub((-delta) as u64);
                    }
                    return Ok(());
                }
            }
            i += 1;
        }
        Err(XcCode::EngineUnregistered)
    }

    /// 在册数。
    pub const fn len(&self) -> usize {
        self.rows.len()
    }

    /// 空判定。
    pub const fn is_empty(&self) -> bool {
        self.rows.is_empty()
    }
}

// ---------------------------------------------------------------------------
// 七、mock 引擎（判据六；F1454 模式复用）
// ---------------------------------------------------------------------------

/// mock 引擎（确定性假引擎：提交即记账、同步即清账、显存可配——
/// 逻辑结论可全链演练，非性能结论）。
#[derive(Clone, Debug)]
pub struct MockEngine {
    id: u32,
    caps: HetCaps,
    used_bytes: u64,
    total_bytes: u64,
    inflight: Vec<u64>,
    next_task: u64,
}

impl MockEngine {
    /// 新 mock（能力与显存注入）。
    pub const fn new(id: u32, caps: HetCaps, total_bytes: u64) -> MockEngine {
        MockEngine {
            id,
            caps,
            used_bytes: 0,
            total_bytes,
            inflight: Vec::new(),
            next_task: 1,
        }
    }
}

impl ComputeDevice for MockEngine {
    fn engine_id(&self) -> u32 {
        self.id
    }

    fn capabilities(&self) -> HetCaps {
        self.caps
    }

    fn submit(&mut self, _kind: u32, payload_bytes: u64) -> Result<u64, XcCode> {
        let new_used = self.used_bytes.saturating_add(payload_bytes);
        if new_used > self.total_bytes {
            return Err(XcCode::HandleRemoteUnreachable); // 语义：显存超载按不可达拒（mock 无降级面）
        }
        self.used_bytes = new_used;
        let tid = self.next_task;
        self.next_task = self.next_task.saturating_add(1);
        self.inflight.push(tid);
        Ok(tid)
    }

    fn sync(&mut self) -> u64 {
        let n = self.inflight.len() as u64;
        self.inflight.clear();
        self.used_bytes = 0;
        n
    }

    fn memory_stats(&self) -> (u64, u64) {
        (self.used_bytes, self.total_bytes)
    }
}

impl BatchExt for MockEngine {
    fn max_batch(&self) -> u32 {
        64
    }
}

impl RealtimeExt for MockEngine {
    fn wcet_us(&self) -> u32 {
        100
    }
}
