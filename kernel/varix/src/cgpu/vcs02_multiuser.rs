//! CGPU-F2882 · 多用户模型与角色（CGPU-S 域 · 多用户与安全域 · S01 组）。
//!
//! **锚点**：`docs/Varix/VE-STAR-II/CGPU Varix STAR II · 总纲与施工书.md#CGPU-F2882`
//!
//! **判据（锚点原文）**：三级、模型表、两组、判据。
//!
//! **职责定位（锚点原文）**：模型：模型（多用户模型（用户/租户/角色三级
//! ——模型实现——模型表；角色权限（前台/后台/服务——角色实现；测试
//! （模型/角色两组）。判据：三级、模型表、两组、判据。
//!
//! ## 一、三级模型（判据一：租户→用户→角色）
//!
//! 三级（[`Tenant`] / [`User`] / [`RoleKind`]）：租户是隔离边界（跨租户
//! 资源互不可见——隔离是模型性质不是策略选项）；用户隶属唯一租户；
//! 角色是用户的权限面（一用户一角色——角色组合走租户级策略，本模型
//! 不做角色叠加，叠加会产生权限歧义）。注册走显性校验：用户 id 全局
//! 唯一（重复显性拒 [`XcCode::DuplicateUser`]）。
//!
//! ## 二、角色权限（判据二：三角色闭集 + 权限矩阵）
//!
//! 三角色（[`RoleKind`]）：前台（交互会话——调度加权最高）/后台
//! （批处理与服务进程——让位前台）/服务（系统服务——不可让度、不参与
//! 用户配额竞争）。权限矩阵（[`ROLE_MATRIX`]）四维闭集：调度档/可让度/
//! 显存配额类/抢占权——每角色一行，行值字面量冻结，判据侧独立对拍。
//! 权限裁决（[`allows`]）是查表不是凭身份猜测；表外动作显性拒。
//!
//! ## 三、模型表与唯一权威 + 用户生命周期
//!
//! [`MultiUserModel`] 是域内唯一权威源：租户表/用户表/角色绑定三账
//! 一体，查询走模型、不走散点副本（唯一权威源纪律）。用户生命周期
//! 是状态机（[`UserState`]：在册→停权→注销——停权可复权，注销单向）
//! ：停权（[`XcCode::UserSuspended`]——会话冻结但账目保留）/注销
//! （行移出在册表，id 进墓碑账 [`TombstoneLedger`] 单调不复用——
//! 防旧消息打到新用户）。租户级策略（[`TenantPolicy`]）：角色配额
//! 倍率与调度加权偏置按租户声明——角色组合走租户级策略的落地面。
//!
//! ## 四、跨租户隔离与审计
//!
//! 资源归属账（[`ResourceOwner`]：资源 → 租户——隔离校验的取数面）；
//! 跨租户访问裁决（[`check_cross_tenant`]）：同租户放行、跨租户显性
//! 拒（[`XcCode::CrossTenantDenied`]）并留审计行（[`IsolationAudit`]：
//! append-only——隔离不是静默拒绝，是留痕拒绝）。角色变更
//! （[`MultiUserModel::change_role`]）带变更留痕（变更前后 + 动因）——
//! 权限面变化可回溯。
//!
//! **对接**：F2881（S 域开工——本模块是其模型面）；F2883（会话管理
//! ——消费用户与角色绑定）；F2884（按用户配额——消费角色配额类与
//! 租户策略倍率）；R 域（安全边界——跨租户审计与 R 域对接）。
//! 零 panic 面、零 IO、零墙钟、no_std。

use alloc::string::{String, ToString};
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 一、规格常量
// ---------------------------------------------------------------------------

/// 本项版本。
pub const MULTIUSER_VERSION: &str = "S01-multiuser-v1";

/// 租户容量（模型表配额；当前策略值，可调）。
pub const TENANT_CAPACITY: usize = 64;

/// 用户容量（全局；当前策略值，可调）。
pub const USER_CAPACITY: usize = 1_024;

/// 墓碑账容量（注销 id 留痕配额；当前策略值，可调）。
pub const TOMBSTONE_CAPACITY: usize = 4_096;

/// 隔离审计容量（append-only 封顶滚动；当前策略值，可调）。
pub const ISOLATION_AUDIT_CAPACITY: usize = 256;

// ---------------------------------------------------------------------------
// 二、诊断码（0x6A 段独占：S01 多用户模型面）
// ---------------------------------------------------------------------------

/// 诊断码闭集（0x6A 段）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum XcCode {
    /// 用户 id 重复。
    DuplicateUser,
    /// 租户 id 重复。
    DuplicateTenant,
    /// 模型表满。
    ModelFull,
    /// 租户不存在（用户注册的租户未登记）。
    TenantMissing,
    /// 表外动作（权限矩阵无行）。
    UnknownAction,
    /// 用户已停权（会话冻结语义——不是拒绝服务，是冻结中）。
    UserSuspended,
    /// 用户已注销（单向终态——复权不可，走重注册新 id）。
    UserRemoved,
    /// 跨租户访问被拒（隔离裁决——留痕拒绝）。
    CrossTenantDenied,
    /// 资源归属未登记（隔离校验无法进行——显性拒不猜测）。
    ResourceUnowned,
    /// 角色变更非法（相同角色/用户不在册）。
    RoleChangeInvalid,
    /// 租户策略未登记（按用户取策略倍率时找不到）。
    PolicyMissing,
}

impl XcCode {
    /// 人读诊断。
    pub const fn detail(self) -> &'static str {
        match self {
            XcCode::DuplicateUser => "用户 id 重复：全局唯一且注销后不复用，建议排查分配源",
            XcCode::DuplicateTenant => "租户 id 重复：租户是隔离边界，id 唯一是隔离前提",
            XcCode::ModelFull => "模型表满：容量写死防膨胀，建议清理僵尸租户/用户后重试",
            XcCode::TenantMissing => "租户不存在：用户必须隶属已登记租户，建议先注册租户",
            XcCode::UnknownAction => "表外动作：权限矩阵无此行，显性拒不猜测；建议补动作闭集",
            XcCode::UserSuspended => "用户已停权：会话冻结但账目保留；建议核对停权原因后复权或注销",
            XcCode::UserRemoved => "用户已注销：单向终态不可复权；建议走重注册分配新 id",
            XcCode::CrossTenantDenied => "跨租户访问被拒：隔离是模型性质；建议核对资源归属与用户隶属租户",
            XcCode::ResourceUnowned => "资源归属未登记：隔离校验无法进行，建议先登记资源归属",
            XcCode::RoleChangeInvalid => "角色变更非法：用户不在册或新旧角色相同；建议核对在册态与角色值",
            XcCode::PolicyMissing => "租户策略未登记：按租户取策略倍率找不到行，建议先登记租户策略",
        }
    }
}

// ---------------------------------------------------------------------------
// 三、角色闭集与权限矩阵（判据二）
// ---------------------------------------------------------------------------

/// 三角色闭集。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RoleKind {
    /// 前台：交互会话，调度加权最高。
    Foreground,
    /// 后台：批处理/服务进程，让位前台。
    Background,
    /// 服务：系统服务，不可让度，不参与用户配额竞争。
    Service,
}

impl RoleKind {
    /// 角色名。
    pub const fn zh(self) -> &'static str {
        match self {
            RoleKind::Foreground => "前台",
            RoleKind::Background => "后台",
            RoleKind::Service => "服务",
        }
    }

    /// 角色序（0-2；矩阵行下标）。
    pub const fn ordinal(self) -> usize {
        match self {
            RoleKind::Foreground => 0,
            RoleKind::Background => 1,
            RoleKind::Service => 2,
        }
    }

    /// 从序号还原（越界回前台——矩阵行闭集内保守默认）。
    pub const fn from_ordinal(o: usize) -> RoleKind {
        match o {
            1 => RoleKind::Background,
            2 => RoleKind::Service,
            _ => RoleKind::Foreground,
        }
    }
}

/// 动作闭集（权限裁决的对象；表外动作显性拒）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    /// 提交渲染任务。
    SubmitRender,
    /// 申请显存配额。
    AllocVram,
    /// 抢占他人预算。
    Preempt,
    /// 读取遥测。
    ReadTelemetry,
}

/// 角色权限行（四维闭集：调度档/可让度/显存配额类/可抢占他人）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RoleRow {
    /// 调度档（0=最高；前台 0/后台 2/服务 1）。
    pub sched_rank: u8,
    /// 可让度（前台不可让度——交互命脉；后台可；服务不可）。
    pub preemptible: bool,
    /// 显存配额类（0=完整配额；1=受限配额；2=系统池直通不占用户配额）。
    pub vram_class: u8,
    /// 可抢占他人（仅服务在应急语义下可——矩阵显式声明）。
    pub can_preempt: bool,
    /// 调度权重万分比（加权公平的权重面：前台 6000/后台 2000/服务 2000——
    /// 服务不吃用户权重竞争，权重仅在与前台同档时生效）。
    pub weight_bps: u32,
    /// 遥测可见级（0=全量/1=自身/2=聚合——S 域遥测隐私的角色面）。
    pub telemetry_level: u8,
}

/// 角色权限矩阵（行=角色 0-2；行值字面量冻结，判据侧独立对拍）。
pub const ROLE_MATRIX: [RoleRow; 3] = [
    // 前台：调度最高、不可让度、完整配额、不可抢占他人、权重 6000、遥测全量。
    RoleRow { sched_rank: 0, preemptible: false, vram_class: 0, can_preempt: false, weight_bps: 6_000, telemetry_level: 0 },
    // 后台：调度最低、可让度、受限配额、不可抢占、权重 2000、遥测自身。
    RoleRow { sched_rank: 2, preemptible: true, vram_class: 1, can_preempt: false, weight_bps: 2_000, telemetry_level: 1 },
    // 服务：调度次高、不可让度、系统池直通、应急可抢占、权重 2000、遥测聚合。
    RoleRow { sched_rank: 1, preemptible: false, vram_class: 2, can_preempt: true, weight_bps: 2_000, telemetry_level: 2 },
];

/// 角色行读取（越界显性拒——不猜）。
pub fn role_row(kind: RoleKind) -> Result<&'static RoleRow, XcCode> {
    ROLE_MATRIX.get(kind.ordinal()).ok_or(XcCode::UnknownAction)
}

/// 租户级策略（角色组合走租户级策略的落地面：每租户可声明各角色的
/// 配额倍率与调度偏置——模型给默认，租户策略覆盖）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TenantPolicy {
    /// 租户 id。
    pub tenant_id: u32,
    /// 前台配额倍率万分比（10000=不倍）。
    pub fg_quota_bps: u32,
    /// 后台配额倍率万分比。
    pub bg_quota_bps: u32,
    /// 调度偏置（有符号；万分比——正值提升前台权重、负值压低）。
    pub sched_bias_bps: i32,
}

/// 默认租户策略（未登记租户的兜底：全不倍、零偏置）。
pub const DEFAULT_TENANT_POLICY: TenantPolicy = TenantPolicy {
    tenant_id: 0,
    fg_quota_bps: 10_000,
    bg_quota_bps: 10_000,
    sched_bias_bps: 0,
};

/// 用户生效配额倍率（角色 × 租户策略——F2884 按用户配额的取数口径）。
pub fn effective_quota_bps(role: RoleKind, policy: &TenantPolicy) -> u32 {
    match role {
        RoleKind::Foreground => policy.fg_quota_bps,
        RoleKind::Background => policy.bg_quota_bps,
        // 服务走系统池直通，倍率不参与用户配额竞争——恒 10000 占位。
        RoleKind::Service => 10_000,
    }
}

/// 用户生效调度权重（矩阵权重 ×（10000+偏置）÷10000，饱和；服务恒
/// 矩阵权重——偏置不进系统服务面）。
pub fn effective_weight_bps(role: RoleKind, policy: &TenantPolicy) -> u32 {
    let row = match role_row(role) {
        Ok(r) => r,
        Err(_) => return 0,
    };
    if role == RoleKind::Service {
        return row.weight_bps;
    }
    let biased = 10_000i64 + policy.sched_bias_bps as i64;
    let w = (row.weight_bps as i64).saturating_mul(biased) / 10_000;
    if w < 0 {
        0
    } else {
        w.min(u32::MAX as i64) as u32
    }
}

// ---------------------------------------------------------------------------
// 四、三级模型与唯一权威源（判据一、三）+ 用户生命周期
// ---------------------------------------------------------------------------

/// 用户生命周期状态（在册→停权→注销；停权可复权，注销单向终态）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UserState {
    /// 在册（正常）。
    Active,
    /// 停权（会话冻结、账目保留、可复权）。
    Suspended,
    /// 注销（单向终态——重注册走新 id）。
    Removed,
}

impl UserState {
    /// 人读名。
    pub const fn zh(self) -> &'static str {
        match self {
            UserState::Active => "在册",
            UserState::Suspended => "停权",
            UserState::Removed => "注销",
        }
    }
}

/// 租户（隔离边界）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Tenant {
    /// 租户 id（唯一）。
    pub tenant_id: u32,
    /// 租户名。
    pub name: String,
}

/// 用户（隶属唯一租户 + 一角色 + 生命周期态）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct User {
    /// 用户 id（全局唯一，注销不复用）。
    pub user_id: u32,
    /// 隶属租户。
    pub tenant_id: u32,
    /// 角色（一用户一角色——角色组合走租户级策略，不做角色叠加）。
    pub role: RoleKind,
    /// 生命周期态。
    pub state: UserState,
    /// 停权/注销动因（在册态恒空串——账面只记事实动因）。
    pub reason: String,
}

/// 墓碑（注销用户 id 留痕——id 不复用的机检面）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Tombstone {
    /// 注销的用户 id。
    pub user_id: u32,
    /// 原属租户。
    pub tenant_id: u32,
    /// 注销动因。
    pub reason: String,
}

/// 墓碑账（注销 id 单调不复用的证据面：重发同 id 显性拒）。
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct TombstoneLedger {
    stones: Vec<Tombstone>,
    dropped: u64,
}

impl TombstoneLedger {
    /// 空墓碑账。
    pub const fn new() -> TombstoneLedger {
        TombstoneLedger {
            stones: Vec::new(),
            dropped: 0,
        }
    }

    /// id 是否已注销（墓碑命中——重用拦截）。
    pub fn contains(&self, user_id: u32) -> bool {
        let mut i = 0usize;
        while i < self.stones.len() {
            if let Some(t) = self.stones.get(i) {
                if t.user_id == user_id {
                    return true;
                }
            }
            i += 1;
        }
        false
    }

    /// 记入墓碑（封顶滚动——最旧墓碑退场时如实计数；容量内 id 永不复用）。
    fn push(&mut self, t: Tombstone) {
        self.stones.push(t);
        if self.stones.len() > TOMBSTONE_CAPACITY {
            self.stones.remove(0);
            self.dropped = self.dropped.saturating_add(1);
        }
    }

    /// 墓碑数。
    pub const fn len_of(&self) -> usize {
        self.stones.len()
    }

    /// 退场墓碑数（容量滚动声明）。
    pub const fn dropped(&self) -> u64 {
        self.dropped
    }
}

/// 隔离审计行（跨租户拒绝/放行的事实留痕——隔离不是静默）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IsolationAuditLine {
    /// 访问方用户。
    pub user_id: u32,
    /// 资源 id。
    pub resource_id: u64,
    /// 资源归属租户。
    pub owner_tenant: u32,
    /// 访问方隶属租户。
    pub user_tenant: u32,
    /// 裁决（true=放行同租户/false=拒跨租户）。
    pub granted: bool,
    /// 逻辑 tick。
    pub tick: u64,
}

/// 隔离审计账（封顶滚动）。
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct IsolationAudit {
    lines: Vec<IsolationAuditLine>,
    dropped: u64,
    denied_total: u64,
    granted_total: u64,
}

impl IsolationAudit {
    /// 追加审计行（计数同步累计——账面与统计同源）。
    pub fn append(&mut self, line: IsolationAuditLine) {
        if line.granted {
            self.granted_total = self.granted_total.saturating_add(1);
        } else {
            self.denied_total = self.denied_total.saturating_add(1);
        }
        self.lines.push(line);
        if self.lines.len() > ISOLATION_AUDIT_CAPACITY {
            self.lines.remove(0);
            self.dropped = self.dropped.saturating_add(1);
        }
    }

    /// 在账行数。
    pub const fn len_of(&self) -> usize {
        self.lines.len()
    }

    /// 退场行数。
    pub const fn dropped(&self) -> u64 {
        self.dropped
    }

    /// 累计拒绝数（不随滚动丢失——统计面）。
    pub const fn denied_total(&self) -> u64 {
        self.denied_total
    }

    /// 累计放行数。
    pub const fn granted_total(&self) -> u64 {
        self.granted_total
    }

    /// 某用户的审计行（回溯面）。
    pub fn trace_user(&self, user_id: u32) -> Vec<IsolationAuditLine> {
        let mut out = Vec::new();
        let mut i = 0usize;
        while i < self.lines.len() {
            if let Some(l) = self.lines.get(i) {
                if l.user_id == user_id {
                    out.push(l.clone());
                }
            }
            i += 1;
        }
        out
    }
}

/// 资源归属（资源 → 租户——隔离校验取数面）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ResourceOwner {
    /// 资源 id。
    pub resource_id: u64,
    /// 归属租户。
    pub tenant_id: u32,
}

/// 资源归属表容量（当前策略值，可调）。
pub const RESOURCE_OWNER_CAPACITY: usize = 16_384;

/// 跨租户访问裁决（纯函数面：同租户放行、跨租户拒、归属未登记拒——
/// 三种结论都有码，不静默）。
pub fn check_cross_tenant(
    user_tenant: u32,
    owner_tenant: Option<u32>,
) -> Result<bool, XcCode> {
    match owner_tenant {
        None => Err(XcCode::ResourceUnowned),
        Some(owner) => {
            if owner == user_tenant {
                Ok(true)
            } else {
                Err(XcCode::CrossTenantDenied)
            }
        }
    }
}

/// 角色变更留痕行。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RoleChangeLine {
    /// 用户 id。
    pub user_id: u32,
    /// 旧角色。
    pub from: RoleKind,
    /// 新角色。
    pub to: RoleKind,
    /// 动因。
    pub reason: String,
    /// 逻辑 tick。
    pub tick: u64,
}

/// 多用户模型（域内唯一权威源：租户/用户/资源归属/租户策略四账一体）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MultiUserModel {
    tenants: Vec<Tenant>,
    users: Vec<User>,
    policies: Vec<TenantPolicy>,
    resources: Vec<ResourceOwner>,
    tombstones: TombstoneLedger,
    role_changes: Vec<RoleChangeLine>,
    next_tenant_id: u32,
    next_user_id: u32,
}

impl MultiUserModel {
    /// 新模型。
    pub const fn new() -> MultiUserModel {
        MultiUserModel {
            tenants: Vec::new(),
            users: Vec::new(),
            policies: Vec::new(),
            resources: Vec::new(),
            tombstones: TombstoneLedger::new(),
            role_changes: Vec::new(),
            next_tenant_id: 1,
            next_user_id: 1,
        }
    }

    /// 注册租户（id 唯一；容量封顶显性拒）。
    pub fn add_tenant(&mut self, name: &str) -> Result<u32, XcCode> {
        if self.tenants.len() >= TENANT_CAPACITY {
            return Err(XcCode::ModelFull);
        }
        let tenant_id = self.next_tenant_id;
        self.next_tenant_id = self.next_tenant_id.saturating_add(1);
        self.tenants.push(Tenant {
            tenant_id,
            name: name.to_string(),
        });
        Ok(tenant_id)
    }

    /// 登记租户策略（覆盖默认——每租户一策略，重复登记覆盖旧值）。
    pub fn set_policy(&mut self, policy: TenantPolicy) -> Result<(), XcCode> {
        let mut found = false;
        let mut i = 0usize;
        while i < self.policies.len() {
            if let Some(p) = self.policies.get_mut(i) {
                if p.tenant_id == policy.tenant_id {
                    *p = policy;
                    found = true;
                    break;
                }
            }
            i += 1;
        }
        if !found {
            self.policies.push(policy);
        }
        Ok(())
    }

    /// 取租户策略（未登记给默认——不拒不猜，默认策略是声明值）。
    pub fn policy_of(&self, tenant_id: u32) -> TenantPolicy {
        let mut i = 0usize;
        while i < self.policies.len() {
            if let Some(p) = self.policies.get(i) {
                if p.tenant_id == tenant_id {
                    return *p;
                }
            }
            i += 1;
        }
        TenantPolicy {
            tenant_id,
            ..DEFAULT_TENANT_POLICY
        }
    }

    /// 注册用户（租户必须在册；墓碑 id 显性拒——不复用；容量封顶显性拒）。
    pub fn add_user(&mut self, tenant_id: u32, role: RoleKind) -> Result<u32, XcCode> {
        if self.users.len() >= USER_CAPACITY {
            return Err(XcCode::ModelFull);
        }
        // 墓碑拦截：注销 id 永不复用（幂等性前提）。
        if self.tombstones.contains(self.next_user_id) {
            return Err(XcCode::DuplicateUser);
        }
        // 租户在册校验（唯一权威源的正确读法：查模型不查副本）。
        let mut found = false;
        let mut i = 0usize;
        while i < self.tenants.len() {
            if let Some(t) = self.tenants.get(i) {
                if t.tenant_id == tenant_id {
                    found = true;
                    break;
                }
            }
            i += 1;
        }
        if !found {
            return Err(XcCode::TenantMissing);
        }
        let user_id = self.next_user_id;
        self.next_user_id = self.next_user_id.saturating_add(1);
        self.users.push(User {
            user_id,
            tenant_id,
            role,
            state: UserState::Active,
            reason: String::new(),
        });
        Ok(user_id)
    }

    /// 查用户角色（权限裁决的前置；停权/注销返回 None——查表不见即无权）。
    pub fn role_of(&self, user_id: u32) -> Option<RoleKind> {
        let mut i = 0usize;
        while i < self.users.len() {
            if let Some(u) = self.users.get(i) {
                if u.user_id == user_id && u.state == UserState::Active {
                    return Some(u.role);
                }
            }
            i += 1;
        }
        None
    }

    /// 查用户生命周期态（未注册 None）。
    pub fn state_of(&self, user_id: u32) -> Option<UserState> {
        let mut i = 0usize;
        while i < self.users.len() {
            if let Some(u) = self.users.get(i) {
                if u.user_id == user_id {
                    return Some(u.state);
                }
            }
            i += 1;
        }
        None
    }

    /// 用户在册且活跃判定（停权 false——会话冻结语义；注销 false）。
    pub fn user_active(&self, user_id: u32) -> bool {
        self.state_of(user_id) == Some(UserState::Active)
    }

    /// 停权用户（在册→停权；动因必填——无因停权显性拒；幂等：重复停权拒）。
    pub fn suspend_user(&mut self, user_id: u32, reason: &str) -> Result<(), XcCode> {
        if reason.trim().is_empty() {
            return Err(XcCode::RoleChangeInvalid);
        }
        let mut i = 0usize;
        while i < self.users.len() {
            if let Some(u) = self.users.get_mut(i) {
                if u.user_id == user_id {
                    if u.state != UserState::Active {
                        return Err(XcCode::UserSuspended);
                    }
                    u.state = UserState::Suspended;
                    u.reason = reason.to_string();
                    return Ok(());
                }
            }
            i += 1;
        }
        Err(XcCode::TenantMissing)
    }

    /// 复权用户（停权→在册；注销态不可复权——单向终态；动因清空）。
    pub fn restore_user(&mut self, user_id: u32) -> Result<(), XcCode> {
        let mut i = 0usize;
        while i < self.users.len() {
            if let Some(u) = self.users.get_mut(i) {
                if u.user_id == user_id {
                    match u.state {
                        UserState::Suspended => {
                            u.state = UserState::Active;
                            u.reason = String::new();
                            return Ok(());
                        }
                        UserState::Removed => return Err(XcCode::UserRemoved),
                        UserState::Active => return Err(XcCode::RoleChangeInvalid),
                    }
                }
            }
            i += 1;
        }
        Err(XcCode::TenantMissing)
    }

    /// 注销用户（在册/停权 → 注销：行移出在册表，id 进墓碑账——
    /// `next_user_id` 只前进不回退 + 墓碑显性拦截，旧消息打不到新用户；
    /// 幂等：重复注销返回 false 不报错）。
    pub fn remove_user(&mut self, user_id: u32) -> bool {
        let mut i = 0usize;
        while i < self.users.len() {
            let hit = match self.users.get(i) {
                Some(u) => u.user_id == user_id,
                None => false,
            };
            if hit {
                let u = match self.users.get(i) {
                    Some(u) => u.clone(),
                    None => return false,
                };
                // 墓碑留痕（id 不复用的证据面）。
                self.tombstones.push(Tombstone {
                    user_id: u.user_id,
                    tenant_id: u.tenant_id,
                    reason: u.reason,
                });
                // swap_remove 在无序表语义下安全（模型表是查找结构非有序账）。
                let last = self.users.len() - 1;
                self.users.swap(i, last);
                self.users.pop();
                return true;
            }
            i += 1;
        }
        false
    }

    /// 用户隶属租户（隔离校验面：跨租户资源访问先过此关）。
    pub fn tenant_of(&self, user_id: u32) -> Option<u32> {
        let mut i = 0usize;
        while i < self.users.len() {
            if let Some(u) = self.users.get(i) {
                if u.user_id == user_id && u.state == UserState::Active {
                    return Some(u.tenant_id);
                }
            }
            i += 1;
        }
        None
    }

    /// 变更角色（在册用户限定；新旧同角色/不在册显性拒；变更留痕）。
    pub fn change_role(
        &mut self,
        user_id: u32,
        to: RoleKind,
        reason: &str,
        tick: u64,
    ) -> Result<RoleKind, XcCode> {
        if reason.trim().is_empty() {
            return Err(XcCode::RoleChangeInvalid);
        }
        let mut i = 0usize;
        while i < self.users.len() {
            if let Some(u) = self.users.get_mut(i) {
                if u.user_id == user_id {
                    if u.state != UserState::Active || u.role == to {
                        return Err(XcCode::RoleChangeInvalid);
                    }
                    let from = u.role;
                    u.role = to;
                    self.role_changes.push(RoleChangeLine {
                        user_id,
                        from,
                        to,
                        reason: reason.to_string(),
                        tick,
                    });
                    return Ok(from);
                }
            }
            i += 1;
        }
        Err(XcCode::TenantMissing)
    }

    /// 角色变更留痕（回溯面）。
    pub fn role_change_log(&self) -> &[RoleChangeLine] {
        &self.role_changes
    }

    /// 登记资源归属（重复登记覆盖——归属以最后一次登记为准并显性可见）。
    pub fn register_resource(&mut self, resource_id: u64, tenant_id: u32) -> Result<(), XcCode> {
        if self.resources.len() >= RESOURCE_OWNER_CAPACITY {
            return Err(XcCode::ModelFull);
        }
        let mut i = 0usize;
        while i < self.resources.len() {
            if let Some(r) = self.resources.get_mut(i) {
                if r.resource_id == resource_id {
                    r.tenant_id = tenant_id;
                    return Ok(());
                }
            }
            i += 1;
        }
        self.resources.push(ResourceOwner {
            resource_id,
            tenant_id,
        });
        Ok(())
    }

    /// 查资源归属租户（未登记 None——裁决面显性拒）。
    pub fn owner_of(&self, resource_id: u64) -> Option<u32> {
        let mut i = 0usize;
        while i < self.resources.len() {
            if let Some(r) = self.resources.get(i) {
                if r.resource_id == resource_id {
                    return Some(r.tenant_id);
                }
            }
            i += 1;
        }
        None
    }

    /// 隔离裁决全流程（模型内闭环：用户→租户、资源→归属、跨租户裁决、
    /// 审计留痕——一次调用完成全部四步）。
    pub fn access_check(
        &self,
        user_id: u32,
        resource_id: u64,
        tick: u64,
        audit: &mut IsolationAudit,
    ) -> Result<bool, XcCode> {
        let user_tenant = match self.tenant_of(user_id) {
            Some(t) => t,
            None => return Err(XcCode::TenantMissing),
        };
        let owner_tenant = self.owner_of(resource_id);
        let verdict = check_cross_tenant(user_tenant, owner_tenant);
        audit.append(IsolationAuditLine {
            user_id,
            resource_id,
            owner_tenant: owner_tenant.unwrap_or(0),
            user_tenant,
            granted: verdict == Ok(true),
            tick,
        });
        verdict
    }

    /// 在册租户数。
    pub const fn tenant_len(&self) -> usize {
        self.tenants.len()
    }

    /// 在册用户数（含停权——生命周期账都在表）。
    pub const fn user_len(&self) -> usize {
        self.users.len()
    }

    /// 活跃用户数。
    pub fn active_len(&self) -> u32 {
        let mut n = 0u32;
        let mut i = 0usize;
        while i < self.users.len() {
            if let Some(u) = self.users.get(i) {
                if u.state == UserState::Active {
                    n = n.saturating_add(1);
                }
            }
            i += 1;
        }
        n
    }

    /// 墓碑账（只读）。
    pub const fn tombstones(&self) -> &TombstoneLedger {
        &self.tombstones
    }

    /// 某租户活跃用户数（租户画像面）。
    pub fn tenant_active_users(&self, tenant_id: u32) -> u32 {
        let mut n = 0u32;
        let mut i = 0usize;
        while i < self.users.len() {
            if let Some(u) = self.users.get(i) {
                if u.tenant_id == tenant_id && u.state == UserState::Active {
                    n = n.saturating_add(1);
                }
            }
            i += 1;
        }
        n
    }

    /// 模型守恒自检：在册用户 id 互异 + 无墓碑 id 在册（id 不复用闭环）
    /// + 资源归属租户都在册。
    pub fn reconcile(&self) -> bool {
        // ① 在册 id 互异。
        let mut i = 0usize;
        while i < self.users.len() {
            let a = match self.users.get(i) {
                Some(u) => u.user_id,
                None => return false,
            };
            let mut j = i + 1;
            while j < self.users.len() {
                if let Some(b) = self.users.get(j) {
                    if a == b.user_id {
                        return false;
                    }
                }
                j += 1;
            }
            i += 1;
        }
        // ② 在册 id 不得命中墓碑。
        let mut i = 0usize;
        while i < self.users.len() {
            if let Some(u) = self.users.get(i) {
                if self.tombstones.contains(u.user_id) {
                    return false;
                }
            }
            i += 1;
        }
        // ③ 资源归属租户都在册。
        let mut i = 0usize;
        while i < self.resources.len() {
            let t = match self.resources.get(i) {
                Some(r) => r.tenant_id,
                None => return false,
            };
            let mut found = false;
            let mut j = 0usize;
            while j < self.tenants.len() {
                if let Some(x) = self.tenants.get(j) {
                    if x.tenant_id == t {
                        found = true;
                        break;
                    }
                }
                j += 1;
            }
            if !found {
                return false;
            }
            i += 1;
        }
        true
    }
}

impl Default for MultiUserModel {
    fn default() -> MultiUserModel {
        MultiUserModel::new()
    }
}

// ---------------------------------------------------------------------------
// 五、权限裁决（判据二的函数面：查表不是猜测）
// ---------------------------------------------------------------------------

/// 权限裁决（动作 × 角色 → 允许与否；表外/越界/未注册/已注销/停权显性拒）。
pub fn allows(user_id: u32, action: Action, model: &MultiUserModel) -> Result<bool, XcCode> {
    // 生命周期前置：停权显性区分于未注册（冻结≠无此人）。
    match model.state_of(user_id) {
        Some(UserState::Suspended) => return Err(XcCode::UserSuspended),
        Some(UserState::Removed) => return Err(XcCode::UserRemoved),
        Some(UserState::Active) => {}
        None => return Err(XcCode::TenantMissing), // 未注册按租户缺失语义拒
    }
    let role = match model.role_of(user_id) {
        Some(r) => r,
        None => return Err(XcCode::TenantMissing),
    };
    let row = role_row(role)?;
    let allowed = match action {
        Action::SubmitRender => true,             // 三角色皆可提交（档位由 sched_rank 定）
        Action::AllocVram => row.vram_class != 2, // 服务走系统池直通不占用户配额申请面
        Action::Preempt => row.can_preempt,       // 仅服务应急可
        Action::ReadTelemetry => true,            // 遥测只读对三角色开放（可见级由 telemetry_level 定）
    };
    Ok(allowed)
}

/// 遥测可见级裁决（动作之外的第二裁决维：按角色行 telemetry_level
/// 返回可见级——S 域遥测隐私的角色面，0=全量/1=自身/2=聚合）。
pub fn telemetry_level_of(user_id: u32, model: &MultiUserModel) -> Result<u8, XcCode> {
    let role = match model.role_of(user_id) {
        Some(r) => r,
        None => return Err(XcCode::TenantMissing),
    };
    let row = role_row(role)?;
    Ok(row.telemetry_level)
}
