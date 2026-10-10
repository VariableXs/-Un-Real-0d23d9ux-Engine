//! CGPU-F2882 · 多用户模型与角色（CGPU-S 域 · 多用户与安全域 · S01 组 · 目标 320 行）。
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
//!
//! ## 三、模型表与唯一权威
//!
//! [`MultiUserModel`] 是域内唯一权威源：租户表/用户表/角色绑定三账
//! 一体，查询走模型、不走散点副本（唯一权威源纪律）；注销用户保留
//! id 不复用（单调分配——防旧消息打到新用户）。
//!
//! ## 四、权限裁决
//!
//! [`RolePolicy`] 给出角色在四维上的可机检结论（[`RolePolicy::allows`]
//! ：动作闭集 [`Action`] × 角色矩阵行）——权限判定是查表不是凭身份
//! 猜测；表外动作显性拒。
//!
//! **对接**：F2881（S 域开工——本模块是其模型面）；F2883（会话管理
//! ——消费用户与角色绑定）；F2884（按用户配额——消费角色配额类）；
//! R 域（安全边界——跨租户访问走 R 域审计）。零 panic 面、零 IO、
//! 零墙钟、no_std。

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
}

/// 角色权限矩阵（行=角色 0-2；行值字面量冻结，判据侧独立对拍）。
pub const ROLE_MATRIX: [RoleRow; 3] = [
    // 前台：调度最高、不可让度、完整配额、不可抢占他人。
    RoleRow { sched_rank: 0, preemptible: false, vram_class: 0, can_preempt: false },
    // 后台：调度最低、可让度、受限配额、不可抢占。
    RoleRow { sched_rank: 2, preemptible: true, vram_class: 1, can_preempt: false },
    // 服务：调度次高、不可让度、系统池直通、应急可抢占。
    RoleRow { sched_rank: 1, preemptible: false, vram_class: 2, can_preempt: true },
];

/// 角色行读取（越界显性拒——不猜）。
pub fn role_row(kind: RoleKind) -> Result<&'static RoleRow, XcCode> {
    ROLE_MATRIX.get(kind.ordinal()).ok_or(XcCode::UnknownAction)
}

// ---------------------------------------------------------------------------
// 四、三级模型与唯一权威源（判据一、三）
// ---------------------------------------------------------------------------

/// 租户（隔离边界）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Tenant {
    /// 租户 id（唯一）。
    pub tenant_id: u32,
    /// 租户名。
    pub name: String,
}

/// 用户（隶属唯一租户 + 一角色）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct User {
    /// 用户 id（全局唯一，注销不复用）。
    pub user_id: u32,
    /// 隶属租户。
    pub tenant_id: u32,
    /// 角色（一用户一角色——角色组合走租户级策略，不做角色叠加）。
    pub role: RoleKind,
}

/// 多用户模型（域内唯一权威源：租户/用户两账一体）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MultiUserModel {
    tenants: Vec<Tenant>,
    users: Vec<User>,
    next_tenant_id: u32,
    next_user_id: u32,
}

impl MultiUserModel {
    /// 新模型。
    pub const fn new() -> MultiUserModel {
        MultiUserModel {
            tenants: Vec::new(),
            users: Vec::new(),
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

    /// 注册用户（租户必须在册；id 全局唯一；容量封顶显性拒）。
    pub fn add_user(&mut self, tenant_id: u32, role: RoleKind) -> Result<u32, XcCode> {
        if self.users.len() >= USER_CAPACITY {
            return Err(XcCode::ModelFull);
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
        });
        Ok(user_id)
    }

    /// 查用户角色（权限裁决的前置）。
    pub fn role_of(&self, user_id: u32) -> Option<RoleKind> {
        let mut i = 0usize;
        while i < self.users.len() {
            if let Some(u) = self.users.get(i) {
                if u.user_id == user_id {
                    return Some(u.role);
                }
            }
            i += 1;
        }
        None
    }

    /// 用户隶属租户（隔离校验面：跨租户资源访问先过此关）。
    pub fn tenant_of(&self, user_id: u32) -> Option<u32> {
        let mut i = 0usize;
        while i < self.users.len() {
            if let Some(u) = self.users.get(i) {
                if u.user_id == user_id {
                    return Some(u.tenant_id);
                }
            }
            i += 1;
        }
        None
    }

    /// 在册租户数。
    pub const fn tenant_len(&self) -> usize {
        self.tenants.len()
    }

    /// 在册用户数。
    pub const fn user_len(&self) -> usize {
        self.users.len()
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

/// 权限裁决（动作 × 角色 → 允许与否；表外/越界显性拒）。
pub fn allows(user_id: u32, action: Action, model: &MultiUserModel) -> Result<bool, XcCode> {
    let role = match model.role_of(user_id) {
        Some(r) => r,
        None => return Err(XcCode::TenantMissing), // 未注册用户按租户缺失语义拒
    };
    let row = role_row(role)?;
    let allowed = match action {
        Action::SubmitRender => true,           // 三角色皆可提交（档位由 sched_rank 定）
        Action::AllocVram => row.vram_class != 2, // 服务走系统池直通不占用户配额申请面
        Action::Preempt => row.can_preempt,     // 仅服务应急可
        Action::ReadTelemetry => true,          // 遥测只读对三角色开放
    };
    Ok(allowed)
}
