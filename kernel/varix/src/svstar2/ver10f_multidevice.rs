//! VE-F3546 · 多设备一致预留 —— 一致性冻结接口 + 适配声明 + 分界清晰 + 半实现禁止。
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F3546`
//!
//! # 职责（锚点原文拆解）
//!
//! - **一致性预留**（[`MultiDeviceReserve`]）：多设备 Persona
//!   一致性的冻结接口——形状定死、不实现，调用即拒。
//! - **适配声明**（[`AdaptLayer`]）：DPI/屏幕差异的适配层声明——
//!   哪层负责缩放、哪层负责布局重排，责任写在声明里不写在注释里。
//! - **分界清晰**（[`ResponsibilityLine`]）：适配责任分界线
//!   （引擎管像素、Persona 管审美、设备层管物理），模糊即拒。
//! - **半实现禁止**：与 F3545 同律——预留是形状冻结的桩，
//!   不是"先跑起来以后再补"的半成品。
//!
//! # 降级矩阵（锚点）
//!
//! | 故障 | 处置 |
//! |---|---|
//! | 误用预留接口 | 拦截 |
//! | 分界模糊 | 先划清（声明缺失拒绝） |
//! | 漂移 | 版本拦截 |
//!
//! # 复杂度
//!
//! O(1)（声明定长）。
//!
//! # 零 panic 面
//!
//! 无索引、无 `unwrap`、无浮点。

use alloc::vec;
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 一、诊断码（E19 段续：6100 起）
// ---------------------------------------------------------------------------

/// 多设备一致预留诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DeviceCode {
    /// 预留接口未实现。
    NotImplemented = 6100,
    /// 适配声明缺失。
    AdaptMissing = 6101,
    /// 责任分界模糊。
    BoundaryBlur = 6102,
    /// 声明版本漂移。
    DeclDrift = 6103,
    /// 半实现尝试（行为超预留桩）。
    HalfImplement = 6104,
}

impl DeviceCode {
    /// 稳定诊断号。
    pub fn code(self) -> u16 {
        self as u16
    }

    /// 中文说明。
    pub fn text(self) -> &'static str {
        match self {
            DeviceCode::NotImplemented => "多设备一致预留接口未实现",
            DeviceCode::AdaptMissing => "适配声明缺失",
            DeviceCode::BoundaryBlur => "适配责任分界模糊",
            DeviceCode::DeclDrift => "声明版本漂移",
            DeviceCode::HalfImplement => "半实现尝试已阻断",
        }
    }

    /// 是否阻断级。
    pub fn blocking(self) -> bool {
        matches!(
            self,
            DeviceCode::NotImplemented | DeviceCode::BoundaryBlur | DeviceCode::HalfImplement
        )
    }
}

// ---------------------------------------------------------------------------
// 二、声明版本
// ---------------------------------------------------------------------------

/// 适配声明版本。
pub const DECL_VERSION: u32 = 1;

// ---------------------------------------------------------------------------
// 三、适配层声明
// ---------------------------------------------------------------------------

/// 适配层职责。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum AdaptRole {
    /// 像素缩放（引擎层）。
    PixelScale,
    /// 布局重排（UI 层）。
    LayoutReflow,
    /// 审美保持（Persona 层——换设备不换性格）。
    AestheticHold,
    /// 物理换算（设备层——DPI 真值来源）。
    PhysicalMap,
}

impl AdaptRole {
    /// 职责短名。
    pub const fn label(self) -> &'static str {
        match self {
            AdaptRole::PixelScale => "像素缩放",
            AdaptRole::LayoutReflow => "布局重排",
            AdaptRole::AestheticHold => "审美保持",
            AdaptRole::PhysicalMap => "物理换算",
        }
    }
}

/// 单层适配声明。
#[derive(Clone, Copy, Debug)]
pub struct AdaptLayer {
    /// 职责。
    pub role: AdaptRole,
    /// 是否启用。
    pub enabled: bool,
}

/// 责任分界线（三层各管一段，不重叠）。
pub struct ResponsibilityLine {
    layers: Vec<AdaptLayer>,
    version: u32,
}

impl ResponsibilityLine {
    /// 标准分界（四层全声明）。
    pub fn standard() -> Self {
        let layers = alloc::vec![
            AdaptLayer { role: AdaptRole::PixelScale, enabled: true },
            AdaptLayer { role: AdaptRole::LayoutReflow, enabled: true },
            AdaptLayer { role: AdaptRole::AestheticHold, enabled: true },
            AdaptLayer { role: AdaptRole::PhysicalMap, enabled: true },
        ];
        Self {
            layers,
            version: DECL_VERSION,
        }
    }

    /// 版本核验。
    pub fn version_ok(&self) -> bool {
        self.version == DECL_VERSION
    }

    /// 某职责是否已声明（None = 缺声明 → 分界模糊）。
    pub fn declared(&self, role: AdaptRole) -> Option<bool> {
        self.layers.iter().find(|l| l.role == role).map(|l| l.enabled)
    }

    /// 分界完整性：四个职责全声明才叫清晰。
    pub fn complete(&self) -> bool {
        self.declared(AdaptRole::PixelScale).is_some()
            && self.declared(AdaptRole::LayoutReflow).is_some()
            && self.declared(AdaptRole::AestheticHold).is_some()
            && self.declared(AdaptRole::PhysicalMap).is_some()
    }

    /// 声明数。
    pub fn len(&self) -> usize {
        self.layers.len()
    }

    /// 空判定。
    pub fn is_empty(&self) -> bool {
        self.layers.is_empty()
    }
}

// ---------------------------------------------------------------------------
// 四、设备画像
// ---------------------------------------------------------------------------

/// 设备画像（DPI + 屏幕尺寸，定整数避免浮点）。
#[derive(Clone, Copy, Debug)]
pub struct DeviceProfile {
    /// DPI。
    pub dpi: u32,
    /// 屏宽（物理像素）。
    pub width_px: u32,
    /// 屏高（物理像素）。
    pub height_px: u32,
}

impl DeviceProfile {
    /// 设备档位（dpi < 144 → 标准；< 288 → 高密；其余 超高密）。
    pub const fn tier(&self) -> u8 {
        if self.dpi < 144 {
            0
        } else if self.dpi < 288 {
            1
        } else {
            2
        }
    }
}

// ---------------------------------------------------------------------------
// 五、冻结预留接口
// ---------------------------------------------------------------------------

/// 一致化请求（形状冻结）。
#[derive(Clone, Copy, Debug)]
pub struct ConsistencyRequest {
    /// 源设备。
    pub source: DeviceProfile,
    /// 目标设备。
    pub target: DeviceProfile,
    /// 需要适配的职责集（位掩码：1=缩放 2=布局 4=审美 8=物理）。
    pub roles_mask: u8,
}

/// 多设备一致预留（冻结接口——不实现）。
pub struct MultiDeviceReserve {
    line: ResponsibilityLine,
    /// 误用拦截计数。
    pub blocked_calls: u32,
}

impl MultiDeviceReserve {
    /// 新建。
    pub fn new() -> Self {
        Self {
            line: ResponsibilityLine::standard(),
            blocked_calls: 0,
        }
    }

    /// 预留入口：预检声明完整性后一律 [`DeviceCode::NotImplemented`]。
    pub fn reconcile(&mut self, req: &ConsistencyRequest) -> Result<(), DeviceCode> {
        self.blocked_calls = self.blocked_calls.saturating_add(1);
        if !self.line.version_ok() {
            return Err(DeviceCode::DeclDrift);
        }
        if !self.line.complete() {
            return Err(DeviceCode::BoundaryBlur);
        }
        // roles_mask 每个置位都必须有声明。
        let pairs = [
            (1u8, AdaptRole::PixelScale),
            (2, AdaptRole::LayoutReflow),
            (4, AdaptRole::AestheticHold),
            (8, AdaptRole::PhysicalMap),
        ];
        for (bit, role) in pairs {
            if req.roles_mask & bit != 0 && self.line.declared(role).is_none() {
                return Err(DeviceCode::AdaptMissing);
            }
        }
        // 同设备档位跳过适配（无需适配却走接口 = 半实现企图）。
        if req.source.tier() == req.target.tier() && req.roles_mask != 0 {
            return Err(DeviceCode::HalfImplement);
        }
        Err(DeviceCode::NotImplemented)
    }

    /// 分界只读。
    pub fn line(&self) -> &ResponsibilityLine {
        &self.line
    }
}

impl Default for MultiDeviceReserve {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// 六、判据
// ---------------------------------------------------------------------------

/// 多设备一致预留判据集。
pub fn run_device_checks() -> crate::checks::CheckSet {
    let mut set = crate::checks::CheckSet::new("svstar2-ver10f_multidevice");
    let mut r = MultiDeviceReserve::new();

    let lo = DeviceProfile {
        dpi: 96,
        width_px: 1920,
        height_px: 1080,
    };
    let hi = DeviceProfile {
        dpi: 288,
        width_px: 3840,
        height_px: 2160,
    };

    // 1. 冻结预留：跨档位合法请求也不实现。
    let cross = ConsistencyRequest {
        source: lo,
        target: hi,
        roles_mask: 0b0011,
    };
    set.add(
            "6100-冻结预留不实现",
            r.reconcile(&cross).err() == Some(DeviceCode::NotImplemented),
            "",
        );

    // 2. 同档位半实现企图拦截。
    let same = ConsistencyRequest {
        source: lo,
        target: lo,
        roles_mask: 0b0001,
    };
    set.add(
            "6101-半实现拦截",
            r.reconcile(&same).err() == Some(DeviceCode::HalfImplement),
            "",
        );

    // 3. 分界完整性。
    set.add("6102-分界清晰", r.line().complete(), "");

    // 4. 设备档位判定。
    set.add(
            "6103-档位判定",
            lo.tier() == 0 && hi.tier() == 2,
            "",
        );

    // 5. 声明覆盖 roles_mask。
    let full_mask = ConsistencyRequest {
        source: lo,
        target: hi,
        roles_mask: 0b1111,
    };
    set.add(
            "6104-全职责声明覆盖",
            r.reconcile(&full_mask).err() == Some(DeviceCode::NotImplemented),
            "",
        );

    // 6. 误用计数。
    set.add("6105-误用审计", r.blocked_calls == 3, "");

    set
}
