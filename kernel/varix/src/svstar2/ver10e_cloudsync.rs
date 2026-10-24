//! VE-F3545 · 云同步预留 —— 冻结接口预留 + 同步边界声明 + 隐私红线预声明。
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F3545`
//!
//! # 职责（锚点原文拆解）
//!
//! - **冻结预留**（[`CloudSyncReserve`]）：接口形状冻结但**不实现**——
//!   不做半实现（半实现=假承诺），调用即 [`SyncCode::NotImplemented`]。
//! - **边界声明**（[`SyncBoundary`]）：哪些档案字段可同步、
//!   哪些仅本地，逐字段声明——不是"以后再说"，是现在就写清楚。
//! - **隐私红线预声明**（[`PrivacyLine`]）：同步内容用户可见可关；
//!   最小化预声明——只同步必要项。
//!
//! # 降级矩阵（锚点）
//!
//! | 故障 | 处置 |
//! |---|---|
//! | 误用预留接口 | 拦截（NotImplemented） |
//! | 边界缺失 | 补声明（缺字段拒绝） |
//! | 漂移 | 版本拦截 |
//!
//! # 复杂度
//!
//! O(1)（声明编译期定长）。
//!
//! # 零 panic 面
//!
//! 无索引、无 `unwrap`、无浮点。

use alloc::vec;
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 一、诊断码（E19 段续：6080 起）
// ---------------------------------------------------------------------------

/// 云同步预留诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SyncCode {
    /// 预留接口未实现（冻结中）。
    NotImplemented = 6080,
    /// 同步边界缺失字段声明。
    BoundaryMissing = 6081,
    /// 隐私红线违规（尝试同步禁同步项）。
    PrivacyViolation = 6082,
    /// 声明版本漂移。
    DeclDrift = 6083,
    /// 最小化违规（同步了非必要项）。
    MinimizationBreach = 6084,
}

impl SyncCode {
    /// 稳定诊断号。
    pub fn code(self) -> u16 {
        self as u16
    }

    /// 中文说明。
    pub fn text(self) -> &'static str {
        match self {
            SyncCode::NotImplemented => "云同步预留接口未实现",
            SyncCode::BoundaryMissing => "同步边界声明缺失",
            SyncCode::PrivacyViolation => "隐私红线违规已拦截",
            SyncCode::DeclDrift => "声明版本漂移",
            SyncCode::MinimizationBreach => "同步最小化违规",
        }
    }

    /// 是否阻断级。
    pub fn blocking(self) -> bool {
        matches!(
            self,
            SyncCode::NotImplemented | SyncCode::PrivacyViolation | SyncCode::MinimizationBreach
        )
    }
}

// ---------------------------------------------------------------------------
// 二、声明版本
// ---------------------------------------------------------------------------

/// 边界声明版本（漂移即拦）。
pub const DECL_VERSION: u32 = 1;

// ---------------------------------------------------------------------------
// 三、同步边界（逐字段声明）
// ---------------------------------------------------------------------------

/// 字段同步策略。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SyncPolicy {
    /// 可同步（必要项）。
    Syncable,
    /// 仅本地（永不上传）。
    LocalOnly,
    /// 可同步但非必要（最小化默认排除）。
    Optional,
}

impl SyncPolicy {
    /// 策略短名。
    pub const fn label(self) -> &'static str {
        match self {
            SyncPolicy::Syncable => "可同步",
            SyncPolicy::LocalOnly => "仅本地",
            SyncPolicy::Optional => "可选同步",
        }
    }
}

/// 单字段边界声明。
#[derive(Clone, Copy, Debug)]
pub struct FieldBoundary {
    /// 字段 id。
    pub field: u32,
    /// 策略。
    pub policy: SyncPolicy,
}

/// 边界声明集（封闭集，缺字段即 [`SyncCode::BoundaryMissing`]）。
pub struct SyncBoundary {
    fields: Vec<FieldBoundary>,
    version: u32,
}

impl SyncBoundary {
    /// 标准声明集（五域各一字段 + 一项本地）。
    pub fn standard() -> Self {
        let fields = alloc::vec![
            FieldBoundary { field: 1, policy: SyncPolicy::Syncable },   // 色彩
            FieldBoundary { field: 2, policy: SyncPolicy::Syncable },   // 声音
            FieldBoundary { field: 3, policy: SyncPolicy::Syncable },   // 字体
            FieldBoundary { field: 4, policy: SyncPolicy::Optional },   // 图标
            FieldBoundary { field: 5, policy: SyncPolicy::Syncable },   // 动效
            FieldBoundary { field: 6, policy: SyncPolicy::LocalOnly },  // 本地密钥
        ];
        Self {
            fields,
            version: DECL_VERSION,
        }
    }

    /// 版本核验。
    pub fn version_ok(&self) -> bool {
        self.version == DECL_VERSION
    }

    /// 查字段策略（None = 未声明 → 边界缺失）。
    pub fn policy_of(&self, field: u32) -> Option<SyncPolicy> {
        self.fields.iter().find(|f| f.field == field).map(|f| f.policy)
    }

    /// 声明完整性：给定字段集每个都有声明。
    pub fn covers(&self, wanted: &[u32]) -> bool {
        wanted.iter().all(|w| self.policy_of(*w).is_some())
    }

    /// 最小化清单：仅 [`SyncPolicy::Syncable`] 项。
    pub fn minimization_list(&self) -> Vec<u32> {
        self.fields
            .iter()
            .filter(|f| f.policy == SyncPolicy::Syncable)
            .map(|f| f.field)
            .collect()
    }
}

// ---------------------------------------------------------------------------
// 四、隐私红线
// ---------------------------------------------------------------------------

/// 隐私红线预声明。
pub struct PrivacyLine {
    /// 用户可见（同步内容清单可查）。
    pub user_visible: bool,
    /// 用户可关（一键关同步）。
    pub user_can_disable: bool,
    /// 最小化（只同步必要项）。
    pub minimized: bool,
}

impl PrivacyLine {
    /// 标准红线（三条全开）。
    pub const fn standard() -> Self {
        Self {
            user_visible: true,
            user_can_disable: true,
            minimized: true,
        }
    }

    /// 三红线是否全守住。
    pub fn holds(&self) -> bool {
        self.user_visible && self.user_can_disable && self.minimized
    }
}

// ---------------------------------------------------------------------------
// 五、冻结预留接口
// ---------------------------------------------------------------------------

/// 同步请求（形状冻结；调用即拒）。
#[derive(Clone, Debug)]
pub struct SyncRequest {
    /// 欲同步字段集。
    pub fields: Vec<u32>,
}

/// 云同步预留（冻结接口——不实现）。
pub struct CloudSyncReserve {
    boundary: SyncBoundary,
    privacy: PrivacyLine,
    /// 误用拦截计数（审计）。
    pub blocked_calls: u32,
}

impl CloudSyncReserve {
    /// 新建。
    pub fn new() -> Self {
        Self {
            boundary: SyncBoundary::standard(),
            privacy: PrivacyLine::standard(),
            blocked_calls: 0,
        }
    }

    /// 预留入口：任何调用一律 [`SyncCode::NotImplemented`]。
    /// 但在拒绝前做边界/隐私/最小化预检——把将来实现时会犯的
    /// 错误在今天就暴露出来（预留不是空白，是形状定死的桩）。
    pub fn sync(&mut self, req: &SyncRequest) -> Result<(), SyncCode> {
        self.blocked_calls = self.blocked_calls.saturating_add(1);
        // 版本漂移先拦。
        if !self.boundary.version_ok() {
            return Err(SyncCode::DeclDrift);
        }
        // 边界完整性。
        if !self.boundary.covers(&req.fields) {
            return Err(SyncCode::BoundaryMissing);
        }
        // 隐私红线：LocalOnly 不得出现在请求里。
        for &f in &req.fields {
            match self.boundary.policy_of(f) {
                Some(SyncPolicy::LocalOnly) => return Err(SyncCode::PrivacyViolation),
                None => return Err(SyncCode::BoundaryMissing),
                _ => {}
            }
        }
        // 最小化：Optional 项只有用户显式开才允许（此处形状冻结，
        // 无用户开关参数 → Optional 项一律最小化排除）。
        for &f in &req.fields {
            if self.boundary.policy_of(f) == Some(SyncPolicy::Optional) {
                return Err(SyncCode::MinimizationBreach);
            }
        }
        // 全部预检通过——仍然不实现（冻结纪律）。
        Err(SyncCode::NotImplemented)
    }

    /// 边界只读。
    pub fn boundary(&self) -> &SyncBoundary {
        &self.boundary
    }

    /// 隐私只读。
    pub fn privacy(&self) -> &PrivacyLine {
        &self.privacy
    }
}

impl Default for CloudSyncReserve {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// 六、判据
// ---------------------------------------------------------------------------

/// 云同步预留判据集。
pub fn run_sync_checks() -> crate::checks::CheckSet {
    let mut set = crate::checks::CheckSet::new("svstar2-ver10e_cloudsync");
    let mut r = CloudSyncReserve::new();

    // 1. 冻结预留：合法请求也返回 NotImplemented。
    let legal = SyncRequest {
        fields: alloc::vec![1, 2],
    };
    set.add(
            "6080-冻结预留不实现",
            r.sync(&legal).err() == Some(SyncCode::NotImplemented),
            "",
        );

    // 2. LocalOnly 字段隐私拦截。
    let leak = SyncRequest {
        fields: alloc::vec![1, 6],
    };
    set.add(
            "6081-仅本地字段拦截",
            r.sync(&leak).err() == Some(SyncCode::PrivacyViolation),
            "",
        );

    // 3. 未声明字段边界缺失。
    let unknown = SyncRequest {
        fields: alloc::vec![99],
    };
    set.add(
            "6082-未声明字段拦截",
            r.sync(&unknown).err() == Some(SyncCode::BoundaryMissing),
            "",
        );

    // 4. Optional 项最小化排除。
    let optional = SyncRequest {
        fields: alloc::vec![1, 4],
    };
    set.add(
            "6083-可选项最小化排除",
            r.sync(&optional).err() == Some(SyncCode::MinimizationBreach),
            "",
        );

    // 5. 边界覆盖判定。
    let b = SyncBoundary::standard();
    set.add("6084-边界覆盖", b.covers(&[1, 2, 3, 5, 6]) && !b.covers(&[1, 77]), "");

    // 6. 最小化清单只含 Syncable。
    let ml = b.minimization_list();
    let only_syncable = ml.iter().all(|&f| f != 4 && f != 6);
    set.add("6085-最小化清单", only_syncable && ml.len() == 4, "");

    // 7. 隐私红线三全。
    set.add("6086-隐私红线", PrivacyLine::standard().holds(), "");

    // 8. 误用计数累计。
    set.add("6087-误用审计计数", r.blocked_calls == 4, "");

    set
}
