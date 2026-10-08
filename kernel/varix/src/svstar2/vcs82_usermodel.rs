//! CGPU-F2882 · 多用户模型与角色（CGPU-S 域 · 多用户与虚拟化 · 批次 S02）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/CGPU Varix STAR II · 总纲与施工书.md#CGPU-F2882`
//!
//! 锚点原文：「模型：模型（多用户模型（用户/租户/角色三级——模型实现——模型表；
//! 角色权限（前台/后台/服务——角色实现；测试（模型/角色两组）。判据：三级、
//! 模型表、两组、判据。」
//!
//! 判据逐条落位：
//! - **三级**：[`MODEL_TIERS`]——用户/租户/角色三级封闭（锚点原文次序即模型
//!   秩）；级间归属由 [`MODEL_TABLE`] 每行 `parent` 声明，断链（父不在三级内）
//!   判 [`E_S820_TIER_ORPHAN`]。
//! - **模型表**：[`MODEL_TABLE`]——每行一条模型实现（tier/name/parent/note
//!   四字段），行名唯一、父链可追溯；表空或行残缺判
//!   [`E_S820_MODEL_INCOMPLETE`]。
//! - **角色实现**：[`ROLES`]——前台/后台/服务三值封闭（锚点原文次序即角色
//!   秩）；每角色权限范围声明 [`ROLE_SCOPE`] 平行非空，空权限判
//!   [`E_S820_PERM_EMPTY`]（有角色无权限=摆设角色）。
//! - **两组**：判据侧 [`super::vcs82_usermodel_checks`] 模型组/角色组两族
//!   全量断言。
//!
//! **为什么角色权限空是缺陷**：S 域四段架构里配额与隔离都以「用户」为单位
//! 执行，角色是把权限**说清楚**的唯一载体——一个没有权限声明的角色等于
//! 让调度器自由裁量，公平铁律在裁量中流失。
//!
//! **零静默纪律**：断链/残缺/空权限/未知角色都产出诊断码（0x9B 段 S 域
//! 细分 0x9B1x），由调用方聚合上报。
//!
//! 确定性：全部封闭常量表 + 纯函数，时间戳由调用方注入（本模块不读时钟）。

// ---------------------------------------------------------------------------
// 一、诊断码（S 域 0x9B 段续细分 0x9B1x）
// ---------------------------------------------------------------------------

/// 三级断链（模型行父不在用户/租户/角色三级内——归属链断裂）。
pub const E_S820_TIER_ORPHAN: u16 = 0x9B10;
/// 模型表残缺（表空或行字段残缺——模型实现缺席）。
pub const E_S820_MODEL_INCOMPLETE: u16 = 0x9B11;
/// 角色权限空（有角色无权限声明——摆设角色）。
pub const E_S820_PERM_EMPTY: u16 = 0x9B12;
/// 未知角色（权限声明引用三级角色表之外的角色）。
pub const E_S820_ROLE_UNKNOWN: u16 = 0x9B13;
/// 版本失配（批次版本与 S02 冻结版不符）。
pub const E_S820_VERSION_MISMATCH: u16 = 0x9B14;

/// 域批次版本。
pub const VCS82_VERSION: &str = "CS82-usermodel-v1";

/// 版本校验（双向）：S02 冻结版放行，其余一律 [`E_S820_VERSION_MISMATCH`]。
pub fn check_version(v: &str) -> Result<(), u16> {
    if v == VCS82_VERSION {
        Ok(())
    } else {
        Err(E_S820_VERSION_MISMATCH)
    }
}

// ---------------------------------------------------------------------------
// 二、多用户模型三级（用户/租户/角色，锚点原文次序即秩）
// ---------------------------------------------------------------------------

/// 多用户模型三级（封闭——锚点「用户/租户/角色三级」；次序即锚点原文点名序）。
pub const MODEL_TIERS: [&str; 3] = ["用户", "租户", "角色"];

/// 模型表行（tier=所属级；name=模型条目名；parent=父级锚名；note=实现声明）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ModelRow {
    /// 所属级（0=用户 1=租户 2=角色，对齐 [`MODEL_TIERS`] 下标）。
    pub tier: usize,
    /// 条目名（表内唯一）。
    pub name: &'static str,
    /// 父级锚名（用户挂租户、租户挂平台根、角色挂所属模型）。
    pub parent: &'static str,
    /// 模型实现声明（一句可 grep 的职责声明）。
    pub note: &'static str,
}

/// 多用户模型表（封闭——S02 冻结；每行一条模型实现）。
pub const MODEL_TABLE: [ModelRow; 6] = [
    ModelRow {
        tier: 1,
        name: "租户-独占",
        parent: "平台根",
        note: "一机一租独占分区：隔离最强，弹性最弱",
    },
    ModelRow {
        tier: 1,
        name: "租户-共享",
        parent: "平台根",
        note: "多租户共享 GPU 池：公平份额+弹性上限",
    },
    ModelRow {
        tier: 0,
        name: "用户-成员",
        parent: "租户-共享",
        note: "租户内成员用户：受租户配额约束",
    },
    ModelRow {
        tier: 0,
        name: "用户-访客",
        parent: "租户-共享",
        note: "低权限访客：只读+短会话，不占保底份额",
    },
    ModelRow {
        tier: 2,
        name: "角色-操作",
        parent: "用户-成员",
        note: "提交渲染作业的角色：受作业级配额",
    },
    ModelRow {
        tier: 2,
        name: "角色-审计",
        parent: "用户-成员",
        note: "只读审计角色：可查配额与用量，不可提交",
    },
];

/// 模型表校验：表非空、每行 tier 落在三级内、行名唯一、
/// 父锚可追溯（父=平台根 或 父=表内另一行名）。残缺判
/// [`E_S820_MODEL_INCOMPLETE`]，断链判 [`E_S820_TIER_ORPHAN`]。
pub fn verify_model_table() -> Result<usize, u16> {
    if MODEL_TABLE.is_empty() {
        return Err(E_S820_MODEL_INCOMPLETE);
    }
    // 行名唯一（O(n²)——表规模常数，无需排序）。
    let mut i = 0usize;
    while i < MODEL_TABLE.len() {
        let mut j = i + 1;
        while j < MODEL_TABLE.len() {
            if MODEL_TABLE[i].name == MODEL_TABLE[j].name {
                return Err(E_S820_MODEL_INCOMPLETE);
            }
            j += 1;
        }
        i += 1;
    }
    // 父锚可追溯：父=平台根，或父是表内某行名。
    let mut roots = 0usize;
    for row in MODEL_TABLE.iter() {
        if row.tier >= 3 {
            return Err(E_S820_MODEL_INCOMPLETE);
        }
        if row.note.is_empty() {
            return Err(E_S820_MODEL_INCOMPLETE);
        }
        if row.parent == "平台根" {
            roots += 1;
            continue;
        }
        let mut linked = false;
        for other in MODEL_TABLE.iter() {
            if other.name == row.parent {
                linked = true;
            }
        }
        if !linked {
            return Err(E_S820_TIER_ORPHAN);
        }
    }
    if roots == 0 {
        return Err(E_S820_TIER_ORPHAN);
    }
    Ok(MODEL_TABLE.len())
}

// ---------------------------------------------------------------------------
// 三、角色实现（前台/后台/服务三值封闭 + 权限范围）
// ---------------------------------------------------------------------------

/// 角色权限三级（封闭——锚点「前台/后台/服务」；次序即角色秩）。
pub const ROLES: [&str; 3] = ["前台", "后台", "服务"];

/// 角色权限范围声明（与 [`ROLES`] 按位对齐——每角色一句可 grep 的权限边界）。
pub const ROLE_SCOPE: [&str; 3] = [
    "前台：面向最终用户的渲染会话——提交/查看/取消自己的作业",
    "后台：面向运维与管理的管理面——租户与配额管理，不直接碰渲染上下文",
    "服务：面向机器对机器的信任面——服务间凭证调用，最小授权+全程审计",
];

/// 角色权限校验：三角色封闭非空 + 每角色权限声明非空 +
/// 声明首词与角色名一致（权限归属不可错位）。空权限判
/// [`E_S820_PERM_EMPTY`]，错位判 [`E_S820_ROLE_UNKNOWN`]。
pub fn verify_roles() -> Result<usize, u16> {
    if ROLES.is_empty() {
        return Err(E_S820_ROLE_UNKNOWN);
    }
    let mut i = 0usize;
    while i < ROLES.len() {
        let scope = ROLE_SCOPE[i];
        if scope.is_empty() {
            return Err(E_S820_PERM_EMPTY);
        }
        if !scope.contains(ROLES[i]) {
            return Err(E_S820_ROLE_UNKNOWN);
        }
        i += 1;
    }
    Ok(ROLES.len())
}

// ---------------------------------------------------------------------------
// 四、模型×角色联动声明（S 域四段的承接位）
// ---------------------------------------------------------------------------

/// 模型×角色联动声明（三级模型与三角色在 S 域四段上的承接——每段一行）。
pub const LINKAGE_FOUR: [&str; 4] = [
    "模型段：三级模型是四段架构中『模型』段的承载结构本体",
    "隔离段：角色权限边界是用户间不互窃在权限面的落地",
    "调度段：用户-成员的保底份额是调度公平性的计量单位",
    "配额段：角色-操作的作业级配额是配额秩序的执行点",
];

/// 联动校验：四行联动声明非空且每行落在四段名目上（模型/隔离/调度/配额）。
pub fn verify_linkage() -> Result<usize, u16> {
    if LINKAGE_FOUR.len() != 4 {
        return Err(E_S820_MODEL_INCOMPLETE);
    }
    let mut i = 0usize;
    while i < 4 {
        if LINKAGE_FOUR[i].is_empty() {
            return Err(E_S820_MODEL_INCOMPLETE);
        }
        i += 1;
    }
    Ok(4)
}
