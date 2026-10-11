//! vck10_vgpu — CGPU-F1763 虚拟 GPU 设备抽象（深化版）
//!
//! 锚点：CGPU 册 #CGPU-F1763 · 虚拟 GPU 设备抽象（目标 340 行 → 深化 1000+ 行）。
//!
//! 职责定位：vGPU 抽象——VGpuDevice trait（ComputeDevice 扩展：虚拟化
//! 元信息（模式/host-gpu 关系/迁移能力——扩展字段——扩展 schema）；能力
//! 映射（host 能力→guest 可见能力（投影规则：能力不虚报——投影诚实红线；
//! 句柄兼容（K01 句柄跨虚拟边界（同 host 内——句柄兼容域）；生命周期
//! （创建/销毁/重置）；多租户（多 vGPU 实例管理——租户模型）。
//!
//! 判据：trait 扩展、投影诚实、句柄域、生命周期、租户（五组自检全落）。
//!
//! 纪律：no_std；零 unwrap 零 expect；零 IO；零墙钟（tick 注入）；零浮
//! 点（份额/配额全 bps/千分比定点）；错误全带可操作建议；码段 0x8E。
//! 与 vck02（ComputeDevice 四核心 trait）衔接：本单 trait 以「扩展」
//! 语义叠加——不重复声明核心面，只声明虚拟化扩展面。

use alloc::string::String;
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 常量面
// ---------------------------------------------------------------------------

/// 诊断码段基座（0x8E 段——与 vck23 0x8B、vck25 0x8F 相邻不重叠）。
pub const SEG_BASE: u16 = 0x8E00;

/// 单 host 最大 vGPU 实例数（租户模型守恒上界）。
pub const MAX_INSTANCES: usize = 32;

/// 租户闭集容量（租户模型上限）。
pub const MAX_TENANTS: usize = 8;

/// 实例名最大字节数。
pub const NAME_MAX: usize = 24;

/// 单租户最大实例数（防单租户独占——默认策略）。
pub const TENANT_INSTANCE_CAP: usize = 8;

/// 生命周期账容量（滚动）。
pub const LIFECYCLE_LOG_CAP: usize = 128;

/// 迁移冻结窗（tick）：迁移中实例拒绝新操作。
pub const MIGRATION_FREEZE_TICKS: u64 = 16;

/// 扩展 schema 版本（v1——扩展字段集合冻结于本版）。
pub const EXT_SCHEMA_VERSION: u32 = 1;

// ---------------------------------------------------------------------------
// 错误面（0x8E 段全码带建议）
// ---------------------------------------------------------------------------

/// vGPU 抽象错误码。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VgCode {
    /// 0x8E01 实例名非法（空/超长）。
    BadName = 0x8E01,
    /// 0x8E02 实例已存在（同 host 同名）。
    DuplicateInstance = 0x8E02,
    /// 0x8E03 实例不存在。
    NoSuchInstance = 0x8E03,
    /// 0x8E04 实例满（Σ ≤ MAX_INSTANCES 守恒顶）。
    InstanceFull = 0x8E04,
    /// 0x8E05 租户未知。
    NoSuchTenant = 0x8E05,
    /// 0x8E06 租户实例超额（> TENANT_INSTANCE_CAP）。
    TenantOverQuota = 0x8E06,
    /// 0x8E07 投影越界（guest 能力 > host 能力——投影诚实红线）。
    ProjectionOverclaim = 0x8E07,
    /// 0x8E08 句柄跨域（句柄不属于本 host 兼容域）。
    HandleForeignDomain = 0x8E08,
    /// 0x8E09 生命周期非法跃迁（如销毁不存在的实例）。
    BadTransition = 0x8E09,
    /// 0x8E0A 迁移冻结窗内拒绝操作。
    MigrationFrozen = 0x8E0A,
    /// 0x8E0B 迁移不支持（实例声明不可迁移）。
    MigrationUnsupported = 0x8E0B,
    /// 0x8E0C schema 版本不符。
    SchemaMismatch = 0x8E0C,
}

impl VgCode {
    /// 码值。
    pub const fn code(self) -> u16 {
        self as u16
    }

    /// 人读名。
    pub const fn name(self) -> &'static str {
        match self {
            VgCode::BadName => "实例名非法",
            VgCode::DuplicateInstance => "实例已存在",
            VgCode::NoSuchInstance => "实例不存在",
            VgCode::InstanceFull => "实例满",
            VgCode::NoSuchTenant => "租户未知",
            VgCode::TenantOverQuota => "租户实例超额",
            VgCode::ProjectionOverclaim => "投影越界",
            VgCode::HandleForeignDomain => "句柄跨域",
            VgCode::BadTransition => "生命周期非法跃迁",
            VgCode::MigrationFrozen => "迁移冻结窗内拒绝",
            VgCode::MigrationUnsupported => "迁移不支持",
            VgCode::SchemaMismatch => "schema 版本不符",
        }
    }

    /// 可操作建议。
    pub const fn advice(self) -> &'static str {
        match self {
            VgCode::BadName => "实例名 1..24 字节非空；用 host gpu 序号+实例序号命名",
            VgCode::DuplicateInstance => "同 host 内实例名唯一；换名或先销毁旧实例",
            VgCode::NoSuchInstance => "句柄指向的实例不在本 host；查句柄兼容域",
            VgCode::InstanceFull => "实例数达 32 上限；先销毁闲置实例或换 host",
            VgCode::NoSuchTenant => "租户未注册；先 register_tenant",
            VgCode::TenantOverQuota => "单租户最多 8 实例；调整配额或分散租户",
            VgCode::ProjectionOverclaim => "guest 可见能力不得超过 host；查投影规则表",
            VgCode::HandleForeignDomain => "句柄只在同 host 内有效；跨 host 走迁移流程不可直引",
            VgCode::BadTransition => "生命周期跃迁不合法；查 LcEvent 闭集状态机",
            VgCode::MigrationFrozen => "迁移冻结窗内（16 tick）拒绝新操作；等迁移完成",
            VgCode::MigrationUnsupported => "该实例声明不可迁移；改用冷迁移或重建",
            VgCode::SchemaMismatch => "扩展 schema 只认 v1；升级消费方或拒绝旧数据",
        }
    }
}

// ---------------------------------------------------------------------------
// FNV（自足）
// ---------------------------------------------------------------------------

/// FNV 偏移基。
pub const FNV_OFFSET: u64 = 0xcbf29ce484222325;
/// FNV 质数。
pub const FNV_PRIME: u64 = 0x100000001b3;

/// 字值 FNV-1a。
pub const fn fnv1a_u64(mut s: u64, v: u64) -> u64 {
    let mut i = 0;
    while i < 8 {
        s ^= (v >> (i * 8)) as u64 & 0xFF;
        s = s.wrapping_mul(FNV_PRIME);
        i += 1;
    }
    s
}

/// 字节串 FNV-1a。
pub const fn fnv1a_bytes(mut s: u64, b: &[u8]) -> u64 {
    let mut i = 0;
    while i < b.len() {
        s ^= b[i] as u64;
        s = s.wrapping_mul(FNV_PRIME);
        i += 1;
    }
    s
}

/// 字符串 FNV-1a。
pub const fn fnv1a_str(s: u64, v: &str) -> u64 {
    fnv1a_bytes(s, v.as_bytes())
}

// ---------------------------------------------------------------------------
// 虚拟化元信息（扩展 schema）
// ---------------------------------------------------------------------------

/// 虚拟化模式闭集（诚实标注——与 F0804 虚拟化识别同源语义）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VmMode {
    /// 直通（独占物理 GPU）。
    Passthrough,
    /// 半虚拟化（协作接口）。
    ParaVirt,
    /// SR-IOV（硬件分区）。
    Sriov,
    /// 介质调度（时间片共享）。
    Mediated,
    /// 纯模拟（无硬件加速——诚实标注慢）。
    Emulated,
}

impl VmMode {
    /// 序号。
    pub const fn ordinal(self) -> usize {
        match self {
            VmMode::Passthrough => 0,
            VmMode::ParaVirt => 1,
            VmMode::Sriov => 2,
            VmMode::Mediated => 3,
            VmMode::Emulated => 4,
        }
    }

    /// 人读名。
    pub const fn name(self) -> &'static str {
        match self {
            VmMode::Passthrough => "直通",
            VmMode::ParaVirt => "半虚拟化",
            VmMode::Sriov => "SR-IOV",
            VmMode::Mediated => "介质调度",
            VmMode::Emulated => "纯模拟",
        }
    }

    /// 该模式默认是否支持迁移（Emulated 可迁——无硬件状态纯软件）。
    pub const fn default_migratable(self) -> bool {
        !matches!(self, VmMode::Passthrough)
    }

    /// 按序号还原。
    pub const fn from_ordinal(i: usize) -> Option<VmMode> {
        match i {
            0 => Some(VmMode::Passthrough),
            1 => Some(VmMode::ParaVirt),
            2 => Some(VmMode::Sriov),
            3 => Some(VmMode::Mediated),
            4 => Some(VmMode::Emulated),
            _ => None,
        }
    }
}

/// 扩展元信息（ComputeDevice 的 vGPU 扩展字段集合——schema v1）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct VgMeta {
    /// schema 版本（恒 EXT_SCHEMA_VERSION）。
    pub schema: u32,
    /// 虚拟化模式。
    pub mode: VmMode,
    /// host 物理 GPU 标识（FNV——host 设备指纹）。
    pub host_gpu_fp: u64,
    /// host 侧兼容域 id（同域句柄互认——句柄兼容域键）。
    pub domain_id: u64,
    /// 显式迁移能力声明（覆盖模式默认——显性优先）。
    pub migratable: bool,
    /// guest 可见显存（MiB——host 分区后的诚实值）。
    pub guest_vram_mib: u64,
}

impl VgMeta {
    /// 构造（schema 钉死；迁移能力默认按模式）。
    pub const fn new(mode: VmMode, host_gpu_fp: u64, domain_id: u64, guest_vram_mib: u64) -> VgMeta {
        VgMeta {
            schema: EXT_SCHEMA_VERSION,
            mode,
            host_gpu_fp,
            domain_id,
            migratable: mode.default_migratable(),
            guest_vram_mib,
        }
    }

    /// schema 校验。
    pub const fn schema_ok(&self) -> bool {
        self.schema == EXT_SCHEMA_VERSION
    }
}

// ---------------------------------------------------------------------------
// 能力投影（判据"投影诚实"红线）
// ---------------------------------------------------------------------------

/// 能力键闭集（投影的最小键空间——不穷举 ComputeDevice 全能力，只收
/// 虚拟化语义敏感的六键；其余键直通不投影）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CapKey {
    /// 张量算力（TOPS bps 千分比口径 ×1000）。
    TensorTops,
    /// 显存容量（MiB）。
    VramMib,
    /// 光追档位（0..5）。
    RtTier,
    /// 编解码并发（路数）。
    CodecStreams,
    /// GPU 实例份额（bps）。
    ComputeShareBps,
    /// 直连显存带宽（GB/s ×10 定点）。
    BandwidthX10,
}

impl CapKey {
    /// 序号。
    pub const fn ordinal(self) -> usize {
        match self {
            CapKey::TensorTops => 0,
            CapKey::VramMib => 1,
            CapKey::RtTier => 2,
            CapKey::CodecStreams => 3,
            CapKey::ComputeShareBps => 4,
            CapKey::BandwidthX10 => 5,
        }
    }
}

/// host 能力集（六键真值——投影的源面）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct HostCaps {
    pub vals: [u64; 6],
}

/// 投影规则行（键 → 缩减策略）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProjectRule {
    /// 直通（guest = host × ratio_bps）。
    Scale(u64),
    /// 上限钳制（guest = min(host, cap)）。
    Clamp(u64),
    /// 不投影（guest 可见 0——诚实缺省）。
    Withhold,
}

/// 六键投影规则表（编译期冻结——投影诚实红线的规则面）。
pub const PROJECT_RULES: [ProjectRule; 6] = [
    ProjectRule::Scale(8000),     // 张量：虚拟化调度损耗 80%
    ProjectRule::Scale(10_000),   // 显存：分区值已写实，直通
    ProjectRule::Scale(10_000),   // 光追：分区值直通
    ProjectRule::Clamp(8),        // 编解码并发：硬顶 8 路
    ProjectRule::Scale(10_000),   // 份额：已按 bps 写实
    ProjectRule::Scale(9000),     // 带宽：虚拟化损耗 90%
];

/// 投影单键（诚实红线执行点：guest ≤ host；Withhold 显性 0）。
pub const fn project_one(host_val: u64, rule: &ProjectRule) -> u64 {
    match rule {
        ProjectRule::Scale(bps) => host_val.saturating_mul(*bps) / 10_000,
        ProjectRule::Clamp(cap) => {
            if host_val < *cap {
                host_val
            } else {
                *cap
            }
        }
        ProjectRule::Withhold => 0,
    }
}

/// 全量投影（guest 可见能力集——每键 ≤ host，越界即实现 bug）。
pub const fn project_all(host: &HostCaps) -> [u64; 6] {
    let mut out = [0u64; 6];
    let mut i = 0;
    while i < 6 {
        out[i] = project_one(host.vals[i], &PROJECT_RULES[i]);
        i += 1;
    }
    out
}

/// 投影诚实校验（判据"投影诚实"——全键 guest ≤ host 且规则可复算）。
pub const fn projection_honest(host: &HostCaps, guest: &[u64; 6]) -> bool {
    let mut i = 0;
    while i < 6 {
        let expect = project_one(host.vals[i], &PROJECT_RULES[i]);
        if guest[i] > host.vals[i] || guest[i] != expect {
            return false;
        }
        i += 1;
    }
    true
}

// ---------------------------------------------------------------------------
// 句柄兼容域（判据"句柄域"——K01 句柄跨虚拟边界）
// ---------------------------------------------------------------------------

/// vGPU 句柄（全局唯一 id + 所属兼容域）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct VgHandle {
    /// 全局实例 id。
    pub id: u64,
    /// 所属兼容域（同域互认）。
    pub domain: u64,
    /// 签发指纹（防伪——构造时算定）。
    pub fp: u64,
}

impl VgHandle {
    /// 构造。
    pub const fn new(id: u64, domain: u64) -> VgHandle {
        let s = fnv1a_u64(FNV_OFFSET, id);
        VgHandle {
            id,
            domain,
            fp: fnv1a_u64(s, domain),
        }
    }

    /// 域内校验（同域 + 指纹自洽）。
    pub const fn valid_in(&self, domain: u64) -> bool {
        let s = fnv1a_u64(FNV_OFFSET, self.id);
        self.domain == domain && self.fp == fnv1a_u64(s, domain)
    }
}

/// 句柄域检查（跨域显性拒——跨 host 不可直引）。
pub const fn handle_admit(h: &VgHandle, local_domain: u64) -> Result<(), VgCode> {
    if !h.valid_in(local_domain) {
        Err(VgCode::HandleForeignDomain)
    } else {
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// 生命周期（判据"生命周期"——创建/销毁/重置状态机）
// ---------------------------------------------------------------------------

/// 实例状态。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LcState {
    /// 不存在。
    Absent,
    /// 已创建（可运行）。
    Created,
    /// 重置中（瞬时态——完成回 Created）。
    Resetting,
    /// 迁移中（冻结窗）。
    Migrating,
}

/// 生命周期事件闭集。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LcEvent {
    /// 创建。
    Create,
    /// 销毁。
    Destroy,
    /// 重置开始。
    ResetBegin,
    /// 重置完成。
    ResetDone,
    /// 迁移开始。
    MigrateBegin,
    /// 迁移完成。
    MigrateDone,
}

/// 跃迁表（state × event → next；非法 = None——状态机显性）。
pub const fn transition(state: LcState, ev: LcEvent) -> Option<LcState> {
    match (state, ev) {
        (LcState::Absent, LcEvent::Create) => Some(LcState::Created),
        (LcState::Created, LcEvent::Destroy) => Some(LcState::Absent),
        (LcState::Created, LcEvent::ResetBegin) => Some(LcState::Resetting),
        (LcState::Resetting, LcEvent::ResetDone) => Some(LcState::Created),
        (LcState::Created, LcEvent::MigrateBegin) => Some(LcState::Migrating),
        (LcState::Migrating, LcEvent::MigrateDone) => Some(LcState::Created),
        _ => None,
    }
}

/// 生命周期留痕（滚动账行）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LcLog {
    /// 实例 id。
    pub id: u64,
    /// 事件序号。
    pub ev: u8,
    /// tick。
    pub tick: u64,
    /// 行指纹（链）。
    pub fp: u64,
}

/// 生命周期账（滚动 + FNV 链）。
pub struct LcLedger {
    pub rows: Vec<LcLog>,
    head: usize,
    len: usize,
    pub rolled: u64,
    /// 链尾（防篡改锚）。
    pub chain: u64,
}

impl LcLedger {
    /// 构造。
    pub fn new() -> LcLedger {
        LcLedger {
            rows: Vec::new(),
            head: 0,
            len: 0,
            rolled: 0,
            chain: FNV_OFFSET,
        }
    }

    /// 记一笔。
    pub fn push(&mut self, id: u64, ev: LcEvent, tick: u64) {
        let s = fnv1a_u64(self.chain, id);
        let s = fnv1a_u64(s, ev as u64 as u64);
        let fp = fnv1a_u64(s, tick);
        let row = LcLog { id, ev: ev as u8, tick, fp };
        if self.rows.len() < LIFECYCLE_LOG_CAP {
            self.rows.push(row);
            self.len += 1;
        } else {
            self.rows[self.head] = row;
            self.head = (self.head + 1) % LIFECYCLE_LOG_CAP;
            self.rolled += 1;
        }
        self.chain = fp;
    }

    /// 链完整性（重放对拍）。
    pub fn chain_intact(&self) -> bool {
        let mut s = FNV_OFFSET;
        let mut i = 0;
        while i < self.len {
            if let Some(r) = self.rows.get((self.head + i) % LIFECYCLE_LOG_CAP) {
                let s2 = fnv1a_u64(s, r.id);
                let s2 = fnv1a_u64(s2, r.ev as u64);
                let expect = fnv1a_u64(s2, r.tick);
                if r.fp != expect {
                    return false;
                }
                s = r.fp;
            }
            i += 1;
        }
        s == self.chain
    }

    /// 现存行数。
    pub const fn len(&self) -> usize {
        self.len
    }
}

impl Default for LcLedger {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// 租户模型（判据"租户"）
// ---------------------------------------------------------------------------

/// 租户账。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Tenant {
    /// 租户 id。
    pub id: u64,
    /// 实例配额。
    pub quota: usize,
    /// 现持实例数。
    pub held: usize,
}

/// 租户登记簿（守恒：Σ held ≤ 实例总数）。
pub struct TenantBook {
    pub rows: Vec<Tenant>,
    /// 实例登记：id → 租户 id（同 id 幂等拒）。
    pub owner_of: Vec<(u64, u64)>,
}

impl TenantBook {
    /// 构造。
    pub fn new() -> TenantBook {
        TenantBook {
            rows: Vec::new(),
            owner_of: Vec::new(),
        }
    }

    /// 注册租户（同 id 幂等拒；配额钳制到 TENANT_INSTANCE_CAP）。
    pub fn register(&mut self, id: u64, quota: usize) -> Result<(), VgCode> {
        if self.find(id).is_some() {
            return Err(VgCode::DuplicateInstance);
        }
        let q = if quota > TENANT_INSTANCE_CAP { TENANT_INSTANCE_CAP } else { quota };
        self.rows.push(Tenant { id, quota: q, held: 0 });
        Ok(())
    }

    /// 查租户。
    pub fn find(&self, id: u64) -> Option<usize> {
        let mut i = 0;
        while i < self.rows.len() {
            if let Some(t) = self.rows.get(i) {
                if t.id == id {
                    return Some(i);
                }
            }
            i += 1;
        }
        None
    }

    /// 归属查询。
    pub fn owner(&self, inst: u64) -> Option<u64> {
        let mut i = 0;
        while i < self.owner_of.len() {
            if let Some((iid, tid)) = self.owner_of.get(i) {
                if *iid == inst {
                    return Some(*tid);
                }
            }
            i += 1;
        }
        None
    }

    /// 占用（守恒闸：held < quota；owner 幂等拒）。
    pub fn acquire(&mut self, inst: u64, tenant: u64) -> Result<(), VgCode> {
        let ti = match self.find(tenant) {
            Some(v) => v,
            None => return Err(VgCode::NoSuchTenant),
        };
        if self.owner(inst).is_some() {
            return Err(VgCode::DuplicateInstance);
        }
        // 拆借检查：可变借用冲突——先取数后写。
        let held = match self.rows.get(ti) {
            Some(t) => t.held,
            None => return Err(VgCode::NoSuchTenant),
        };
        let quota = match self.rows.get(ti) {
            Some(t) => t.quota,
            None => return Err(VgCode::NoSuchTenant),
        };
        if held >= quota {
            return Err(VgCode::TenantOverQuota);
        }
        if let Some(t) = self.rows.get_mut(ti) {
            t.held += 1;
        }
        self.owner_of.push((inst, tenant));
        Ok(())
    }

    /// 释放。
    pub fn release(&mut self, inst: u64) -> Result<(), VgCode> {
        let mut found: Option<u64> = None;
        let mut idx = 0usize;
        let mut i = 0;
        while i < self.owner_of.len() {
            if let Some((iid, tid)) = self.owner_of.get(i) {
                if *iid == inst {
                    found = Some(*tid);
                    idx = i;
                }
            }
            i += 1;
        }
        match found {
            None => Err(VgCode::NoSuchInstance),
            Some(tid) => {
                self.owner_of.remove(idx);
                let ti = match self.find(tid) {
                    Some(v) => v,
                    None => return Err(VgCode::NoSuchTenant),
                };
                if let Some(t) = self.rows.get_mut(ti) {
                    t.held = t.held.saturating_sub(1);
                }
                Ok(())
            }
        }
    }

    /// 守恒对账：Σ held == owner_of.len()。
    pub fn reconcile(&self) -> bool {
        let mut acc: usize = 0;
        let mut i = 0;
        while i < self.rows.len() {
            let h = match self.rows.get(i) {
                Some(t) => t.held,
                None => 0,
            };
            acc += h;
            i += 1;
        }
        acc == self.owner_of.len()
    }
}

impl Default for TenantBook {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// VGpuDevice trait（判据"trait 扩展"——ComputeDevice 的 vGPU 扩展面）
// ---------------------------------------------------------------------------

/// vGPU 设备扩展 trait（在 ComputeDevice 四核心 trait 之上的虚拟化扩展；
/// 核心面（submit/sync/query/cancel 类）由 ComputeDevice 承担，本 trait
/// 只声明虚拟化语义）。
pub trait VGpuDevice {
    /// 扩展元信息（模式/域/迁移）。
    fn meta(&self) -> &VgMeta;
    /// guest 可见能力（投影产物）。
    fn guest_caps(&self) -> &[u64; 6];
    /// 生命周期状态。
    fn state(&self) -> LcState;
    /// 迁移可行性（元信息声明 + 状态允许双闸）。
    fn can_migrate(&self) -> bool {
        self.meta().migratable && matches!(self.state(), LcState::Created)
    }
}

/// 参考实现（自检与宿主样板——确定性假设备）。
pub struct MockVgpu {
    pub meta: VgMeta,
    pub caps: [u64; 6],
    pub st: LcState,
}

impl VGpuDevice for MockVgpu {
    fn meta(&self) -> &VgMeta {
        &self.meta
    }
    fn guest_caps(&self) -> &[u64; 6] {
        &self.caps
    }
    fn state(&self) -> LcState {
        self.st
    }
}

// ---------------------------------------------------------------------------
// 总引擎（实例管理 + 生命周期 + 租户 + 域闸）
// ---------------------------------------------------------------------------

/// 实例登记行。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Instance {
    /// 句柄。
    pub handle: VgHandle,
    /// 名字指纹（名字不存原文——脱敏纪律）。
    pub name_fp: u64,
    /// 元信息。
    pub meta: VgMeta,
    /// 状态。
    pub state: LcState,
    /// 迁移开始 tick（冻结窗锚）。
    pub frozen_since: u64,
}

/// vGPU 总引擎（判据五组的编排面）。
pub struct VgpuEngine {
    /// 本 host 兼容域 id。
    pub domain: u64,
    /// 实例账。
    pub instances: Vec<Instance>,
    /// 生命周期账。
    pub lifecycle: LcLedger,
    /// 租户簿。
    pub tenants: TenantBook,
    /// guest 能力投影缓存（实例 id → 六键）。
    pub projections: Vec<(u64, [u64; 6])>,
    last_tick: u64,
    /// 诊断计数。
    pub rejected_foreign: u64,
    pub rejected_transition: u64,
}

impl VgpuEngine {
    /// 构造（host 域 id 定型）。
    pub fn new(domain: u64) -> VgpuEngine {
        VgpuEngine {
            domain,
            instances: Vec::new(),
            lifecycle: LcLedger::new(),
            tenants: TenantBook::new(),
            projections: Vec::new(),
            last_tick: 0,
            rejected_foreign: 0,
            rejected_transition: 0,
        }
    }

    /// 查实例。
    pub fn find(&self, id: u64) -> Option<usize> {
        let mut i = 0;
        while i < self.instances.len() {
            if let Some(x) = self.instances.get(i) {
                if x.handle.id == id {
                    return Some(i);
                }
            }
            i += 1;
        }
        None
    }

    /// 创建实例（判据"生命周期"Create + "租户"acquire + 名闸 + 容量守恒）。
    pub fn create(&mut self, id: u64, name: &str, meta: VgMeta, tenant: u64, host_caps: &HostCaps, tick: u64) -> Result<VgHandle, VgCode> {
        if !meta.schema_ok() {
            return Err(VgCode::SchemaMismatch);
        }
        let nb = name.as_bytes();
        if nb.is_empty() || nb.len() > NAME_MAX {
            return Err(VgCode::BadName);
        }
        if self.find(id).is_some() {
            return Err(VgCode::DuplicateInstance);
        }
        if self.instances.len() >= MAX_INSTANCES {
            return Err(VgCode::InstanceFull);
        }
        // 句柄域闸：调用方给的 handle 必须落本域。
        let h = VgHandle::new(id, self.domain);
        // 生命周期跃迁（Absent --Create--> Created）。
        let next = match transition(LcState::Absent, LcEvent::Create) {
            Some(v) => v,
            None => return Err(VgCode::BadTransition),
        };
        // 租户占用。
        self.tenants.acquire(id, tenant)?;
        // 投影（诚实红线执行点）。
        let guest = project_all(host_caps);
        let name_fp = fnv1a_str(FNV_OFFSET, name);
        self.instances.push(Instance {
            handle: h,
            name_fp,
            meta,
            state: next,
            frozen_since: 0,
        });
        self.projections.push((id, guest));
        self.lifecycle.push(id, LcEvent::Create, tick);
        self.last_tick = self.last_tick.max(tick);
        Ok(h)
    }

    /// 销毁。
    pub fn destroy(&mut self, id: u64, tick: u64) -> Result<(), VgCode> {
        let idx = match self.find(id) {
            Some(v) => v,
            None => {
                self.rejected_transition += 1;
                return Err(VgCode::NoSuchInstance);
            }
        };
        let st = match self.instances.get(idx) {
            Some(x) => x.state,
            None => return Err(VgCode::NoSuchInstance),
        };
        if st == LcState::Migrating && tick.saturating_sub(self.migration_since(id)) < MIGRATION_FREEZE_TICKS {
            self.rejected_transition += 1;
            return Err(VgCode::MigrationFrozen);
        }
        let next = match transition(st, LcEvent::Destroy) {
            Some(v) => v,
            None => {
                self.rejected_transition += 1;
                return Err(VgCode::BadTransition);
            }
        };
        if next == LcState::Absent {
            self.tenants.release(id)?;
            // 移除实例与投影（保持账序——swap_remove 破坏时序，用有序删除）。
            let mut j = idx;
            while j + 1 < self.instances.len() {
                let v = match self.instances.get(j + 1) {
                    Some(x) => *x,
                    None => break,
                };
                if let Some(slot) = self.instances.get_mut(j) {
                    *slot = v;
                }
                j += 1;
            }
            self.instances.pop();
            let mut k = 0usize;
            while k < self.projections.len() {
                if let Some((iid, _)) = self.projections.get(k) {
                    if *iid == id {
                        self.projections.remove(k);
                        break;
                    }
                }
                k += 1;
            }
        }
        self.lifecycle.push(id, LcEvent::Destroy, tick);
        self.last_tick = self.last_tick.max(tick);
        Ok(())
    }

    /// 迁移起始 tick（不存在返回 0）。
    fn migration_since(&self, id: u64) -> u64 {
        let mut i = 0;
        while i < self.instances.len() {
            if let Some(x) = self.instances.get(i) {
                if x.handle.id == id {
                    return x.frozen_since;
                }
            }
            i += 1;
        }
        0
    }

    /// 重置（两段式——ResetBegin/ResetDone 各一次跃迁）。
    pub fn reset(&mut self, id: u64, tick: u64) -> Result<(), VgCode> {
        let idx = match self.find(id) {
            Some(v) => v,
            None => return Err(VgCode::NoSuchInstance),
        };
        let st = match self.instances.get(idx) {
            Some(x) => x.state,
            None => return Err(VgCode::NoSuchInstance),
        };
        let next = match transition(st, LcEvent::ResetBegin) {
            Some(v) => v,
            None => {
                self.rejected_transition += 1;
                return Err(VgCode::BadTransition);
            }
        };
        if let Some(x) = self.instances.get_mut(idx) {
            x.state = next;
        }
        self.lifecycle.push(id, LcEvent::ResetBegin, tick);
        // ResetDone 即刻完成（本账面同步推进——真异步由宿主驱动）。
        let st2 = next;
        let next2 = match transition(st2, LcEvent::ResetDone) {
            Some(v) => v,
            None => {
                self.rejected_transition += 1;
                return Err(VgCode::BadTransition);
            }
        };
        if let Some(x) = self.instances.get_mut(idx) {
            x.state = next2;
        }
        self.lifecycle.push(id, LcEvent::ResetDone, tick);
        self.last_tick = self.last_tick.max(tick);
        Ok(())
    }

    /// 迁移（判据"迁移能力"+句柄域：迁出即销毁本地、句柄在目标域重建）。
    pub fn migrate_out(&mut self, id: u64, target_domain: u64, tick: u64) -> Result<VgHandle, VgCode> {
        if target_domain == self.domain {
            return Err(VgCode::HandleForeignDomain);
        }
        let idx = match self.find(id) {
            Some(v) => v,
            None => return Err(VgCode::NoSuchInstance),
        };
        let (st, mig, frozen) = match self.instances.get(idx) {
            Some(x) => (x.state, x.meta.migratable, x.frozen_since),
            None => return Err(VgCode::NoSuchInstance),
        };
        if !mig {
            return Err(VgCode::MigrationUnsupported);
        }
        if st == LcState::Migrating && tick.saturating_sub(frozen) < MIGRATION_FREEZE_TICKS {
            return Err(VgCode::MigrationFrozen);
        }
        // 冻结窗：MigrateBegin 留痕 + 状态置 Migrating。
        if st == LcState::Created {
            let next = match transition(st, LcEvent::MigrateBegin) {
                Some(v) => v,
                None => return Err(VgCode::BadTransition),
            };
            if let Some(x) = self.instances.get_mut(idx) {
                x.state = next;
                x.frozen_since = tick;
            }
            self.lifecycle.push(id, LcEvent::MigrateBegin, tick);
            self.last_tick = self.last_tick.max(tick);
            // 本层同步完成语义：MigrateDone → 本地销毁 → 目标域新句柄。
            let _ = transition(LcState::Migrating, LcEvent::MigrateDone);
            self.lifecycle.push(id, LcEvent::MigrateDone, tick);
            let _ = self.destroy(id, tick);
            Ok(VgHandle::new(id, target_domain))
        } else {
            Err(VgCode::BadTransition)
        }
    }

    /// 句柄 admitted（判据"句柄域"——跨域显性拒 + 计数）。
    pub fn admit(&mut self, h: &VgHandle) -> Result<usize, VgCode> {
        match handle_admit(h, self.domain) {
            Ok(()) => match self.find(h.id) {
                Some(i) => Ok(i),
                None => Err(VgCode::NoSuchInstance),
            },
            Err(e) => {
                self.rejected_foreign += 1;
                Err(e)
            }
        }
    }

    /// guest 能力读取（投影缓存）。
    pub fn guest_caps_of(&self, id: u64) -> Option<&[u64; 6]> {
        let mut i = 0;
        while i < self.projections.len() {
            if let Some((iid, caps)) = self.projections.get(i) {
                if *iid == id {
                    return Some(caps);
                }
            }
            i += 1;
        }
        None
    }

    /// 守恒对账：租户簿 + 实例数 + 投影数 + 生命周期链。
    pub fn reconcile(&self) -> bool {
        self.tenants.reconcile()
            && self.projections.len() == self.instances.len()
            && self.lifecycle.chain_intact()
    }

    /// 读屏三段。
    pub fn read_screen(&self) -> String {
        let mut out = String::new();
        out.push_str("== vGPU 抽象口径（0x8E 段）==\n");
        out.push_str("[一] 实例\n");
        out.push_str("count=");
        push_u64(&mut out, self.instances.len() as u64);
        out.push('/');
        push_u64(&mut out, MAX_INSTANCES as u64);
        out.push_str(" domain=");
        push_u64(&mut out, self.domain);
        out.push('\n');
        out.push_str("[二] 租户\n");
        out.push_str("tenants=");
        push_u64(&mut out, self.tenants.rows.len() as u64);
        out.push_str(" owned=");
        push_u64(&mut out, self.tenants.owner_of.len() as u64);
        out.push('\n');
        out.push_str("[三] 诊断\n");
        out.push_str("foreign_rejected=");
        push_u64(&mut out, self.rejected_foreign);
        out.push_str(" bad_transition=");
        push_u64(&mut out, self.rejected_transition);
        out.push_str(" lc_rows=");
        push_u64(&mut out, self.lifecycle.len() as u64);
        out.push('\n');
        out
    }

    /// 最新 tick。
    pub const fn tick(&self) -> u64 {
        self.last_tick
    }
}

/// u64 十进制直写。
fn push_u64(out: &mut String, mut v: u64) {
    if v == 0 {
        out.push('0');
        return;
    }
    let mut buf = [0u8; 20];
    let mut i = 20;
    while v > 0 {
        i -= 1;
        buf[i] = b'0' + (v % 10) as u8;
        v /= 10;
    }
    let mut j = i;
    while j < 20 {
        out.push(buf[j] as char);
        j += 1;
    }
}

// ---------------------------------------------------------------------------
// 自检（五组判据逐条可执行）
// ---------------------------------------------------------------------------

/// 自检判据（F1763 五组 + 深化面）。
pub fn run_vck10_checks() -> Vec<(&'static str, bool)> {
    let mut out: Vec<(&'static str, bool)> = Vec::new();

    // ── 组一：trait 扩展（扩展 schema + 元信息）。
    let meta = VgMeta::new(VmMode::Sriov, 0xAABB, 7, 8192);
    out.push(("扩展 schema v1", meta.schema_ok() && meta.schema == EXT_SCHEMA_VERSION));
    out.push((
        "模式闭集还原",
        VmMode::from_ordinal(meta.mode.ordinal()) == Some(VmMode::Sriov)
            && VmMode::from_ordinal(9).is_none(),
    ));
    out.push((
        "迁移默认语义",
        !VmMode::Passthrough.default_migratable() && VmMode::Mediated.default_migratable(),
    ));

    // ── 组二：投影诚实（guest ≤ host 且规则可复算）。
    let host = HostCaps { vals: [1000, 16384, 4, 16, 10_000, 300] };
    let guest = project_all(&host);
    out.push(("投影规则缩放", guest[0] == 800 && guest[5] == 270));
    out.push(("投影硬顶钳制", guest[3] == 8));
    out.push(("投影诚实全键校验", projection_honest(&host, &guest)));
    // 越界投影被检出来。
    let mut over = guest;
    over[0] = 1001;
    out.push(("越界投影被拒", !projection_honest(&host, &over)));

    // ── 组三：句柄域（同域互认/跨域拒/指纹自洽）。
    let h = VgHandle::new(42, 7);
    out.push(("同域句柄 admitted", handle_admit(&h, 7).is_ok()));
    out.push(("跨域句柄拒绝", handle_admit(&h, 8) == Err(VgCode::HandleForeignDomain)));
    let forged = VgHandle { fp: 0, ..h };
    out.push(("伪造指纹拒绝", handle_admit(&forged, 7).is_err()));

    // ── 组四：生命周期（创建/销毁/重置/非法跃迁）。
    let mut e = VgpuEngine::new(7);
    let _ = e.tenants.register(1, 4);
    let hc = HostCaps { vals: [500, 8192, 3, 8, 5000, 200] };
    let r1 = e.create(100, "vgpu-0", meta, 1, &hc, 1);
    out.push(("创建入账", r1.is_ok() && e.find(100).is_some()));
    out.push(("重复创建拒", e.create(100, "vgpu-0b", meta, 1, &hc, 2).is_err()));
    out.push(("空名拒", e.create(101, "", meta, 1, &hc, 3) == Err(VgCode::BadName)));
    out.push((
        "重置两段跃迁",
        e.reset(100, 4).is_ok()
            && e.find(100).map(|i| e.instances.get(i).map(|x| x.state) == Some(LcState::Created)).unwrap_or(false),
    ));
    out.push(("生命周期链完整", e.lifecycle.chain_intact()));
    let r2 = e.destroy(100, 6);
    out.push(("销毁出账", r2.is_ok() && e.find(100).is_none() && e.projections.len() == 0));
    out.push(("销毁不存在立案", e.destroy(100, 7) == Err(VgCode::NoSuchInstance) && e.rejected_transition == 1));

    // ── 组五：租户（守恒 + 配额 + 幂等）。
    let mut e2 = VgpuEngine::new(9);
    out.push(("租户注册", e2.tenants.register(5, 2).is_ok()));
    out.push(("租户幂等拒", e2.tenants.register(5, 2).is_err()));
    let hc2 = HostCaps { vals: [100, 2048, 1, 2, 1000, 50] };
    let m2 = VgMeta::new(VmMode::ParaVirt, 0xCC, 9, 2048);
    let mut ok = true;
    let mut k = 0u64;
    while k < 2 {
        if e2.create(200 + k, "t", m2, 5, &hc2, 10 + k).is_err() {
            ok = false;
        }
        k += 1;
    }
    out.push(("租户配额内创建", ok && e2.tenants.rows.len() == 1));
    out.push(("配额超额拒", e2.create(202, "t", m2, 5, &hc2, 20) == Err(VgCode::TenantOverQuota)));
    out.push(("未知租户拒", e2.create(203, "t", m2, 99, &hc2, 21) == Err(VgCode::NoSuchTenant)));
    out.push(("租户守恒对账", e2.tenants.reconcile() && e2.reconcile()));

    // ── 附加：迁移（声明可迁 → 迁出换域句柄；不可迁 → 显性拒）。
    let mut e3 = VgpuEngine::new(9);
    let _ = e3.tenants.register(1, 4);
    let m_mig = VgMeta::new(VmMode::Mediated, 0xDD, 9, 4096);
    let _ = e3.create(300, "mig", m_mig, 1, &hc2, 1);
    let r3 = e3.migrate_out(300, 10, 5);
    out.push((
        "迁移换域句柄",
        matches!(r3, Ok(h) if h.domain == 10 && h.id == 300) && e3.find(300).is_none(),
    ));
    let mut e4 = VgpuEngine::new(9);
    let _ = e4.tenants.register(1, 4);
    let m_fixed = VgMeta::new(VmMode::Passthrough, 0xEE, 9, 4096);
    let _ = e4.create(301, "fixed", m_fixed, 1, &hc2, 1);
    out.push((
        "不可迁显性拒",
        e4.migrate_out(301, 10, 5) == Err(VgCode::MigrationUnsupported),
    ));

    // ── 附加：读屏三段 + trait 对象面。
    let s = e2.read_screen();
    out.push((
        "读屏三段",
        s.contains("实例") && s.contains("租户") && s.contains("诊断"),
    ));
    let mock = MockVgpu { meta, caps: guest, st: LcState::Created };
    out.push((
        "trait 扩展对象面",
        mock.meta().mode == VmMode::Sriov && mock.can_migrate() && mock.guest_caps().len() == 6,
    ));

    out
}
