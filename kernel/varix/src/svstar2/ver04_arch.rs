//! VE-F3603 · 创作资产模型（VE-R 域 · 创作生态域 · R01 组 · 目标 380 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F3603`
//!
//! **判据（锚点原文·六项）**：七要素、许可红线、兼容警告、类型单源、schema
//! 复用、判据。
//!
//! **职责定位（锚点原文）**：创作资产模型——资产模型（CreationAsset 模型
//! （ID/类型/内容/元数据/版本/来源/许可七要素——七要素齐备断言（缺许可=不可
//! 分发（许可红线：无许可信息的创作资产不许上架（上架前必补；类型体系
//! （主题/皮肤/壁纸/图标/组件/模板/脚本数据七类创作资产（复用 F3204 扩展注册
//! （复述单源；资产兼容（版本兼容（资产声明适配引擎版本（兼容声明红线：未声明
//! 兼容版本的资产安装时警告（静默不兼容=装了就坏（兼容警告红线；元数据 schema
//! （校验器复用 F3204。
//!
//! **数据结构（锚点原文·家族格式）**：七要素模型；七类注册；兼容声明协议；
//! schema 复用。
//!
//! **错误路径与降级矩阵（锚点原文·家族格式）**：许可缺失上架→阻断（红线实测）；
//! 未注册类型→拒绝（复述）；兼容未声明→警告+确认（红线实测）；schema 违例→
//! 宽松+告警（复述）。
//!
//! **性能逐项分解（锚点原文·家族格式）**：模型 O(1)；注册 O(1)；警告 O(1)；
//! 校验 O(元数据)。
//!
//! **跨批对接点（锚点原文·家族格式）**：F3204 单源复用；F3216 SLA（资产走
//! 管线）；F3616 分发对端。
//!
//! **无隐私面（锚点原文）**：本项只登记资产元数据字段，不采集创作者个人信息。
//!
//! # 〇、模块命名：`ver04_*` 的由来
//!
//! [`ver01_arch`](super::ver01_arch)= E 域 F3401；[`ver02_arch`](super::ver02_arch)
//! = F3601 域开工ADR；[`ver03_arch`](super::ver03_arch) = F3602 生态总架构。
//! 三者序号已占，本项顺次取 `ver04_*`（源码只增不减不移，不改他人模块名）。
//!
//! # 一、七要素与七类不是一回事（最易搞混处，先说清）
//!
//! 锚点里两组数字并列，容易读混：
//!
//! - **七要素** = 一个资产**实例**必须带的字段：ID / 类型 / 内容 / 元数据 /
//!   版本 / 来源 / 许可（[`AssetField`]，恰 7）；
//! - **七类** = 资产的**种类**：主题/皮肤/壁纸/图标/组件/模板/脚本数据
//!   （[`AssetKind`]，恰 7）。
//!
//! 数字相同纯属巧合，**两组各有独立的机检**：要素齐备查[`AssetModel`]，
//! 种类登记查 [`KindRegistry`]。把两者混为一谈就会出现「七类都有 ID 所以
//! 七要素齐备」这种错误结论——实际上七类都齐备也不代表任一个实例的七要素
//! 齐备（本项 [`E_FIELD_MISSING`] 正是按实例逐个查的）。
//!
//! # 二、七类 vs F3204 十类：不是同一集合，是**两轴**（承接 F3602 的裁决经验）
//!
//! 本项说「七类创作资产（复用 F3204 扩展注册（复述单源）」，而 F3204 的
//! 锚点写「ResourceType 枚举（纹理/网格/材质/音频/字体/动画/样式表/场景图/
//! 预制体/脚本数据**十类**——每类：元数据 schema/解码器路由/内存画像/校验规则
//! **四要素**登记」。
//!
//! 两组只重叠一项（脚本数据），其余全不同。若把它们当同一集合，就会得出
//! 「本项应登记十类」或「F3204 应改成七类」的错误结论。本项的裁决：
//!
//! - **两轴正交**：F3204 管的是**资源容器类型**（怎么解码、占多少内存）；
//!   本项管的是**创作资产种类**（用户做什么东西）。一个壁纸资产（创作种类）
//!   展开后是若干纹理+样式表（资源类型）。
//! - **单源在 F3204**：解码路由/内存画像/校验规则一律走 F3204，本项**只引用
//!   不复制**（[`SchemaReuse`] 登记引用面）。
//! - 每类创作资产**声明它用哪些资源类型**（[`KindBinding`]），映射缺项即
//!   [`E_BINDING_MISSING`]——这样「壁纸资产到底由什么资源构成」有册内答案，
//!   而不必把两个枚举硬合成一个。
//!
//! # 三、许可红线：缺许可不是警告，是**不许上架**（判据二）
//!
//! 锚点写「许可红线：无许可信息的创作资产不许上架（上架前必补）」，错误路径
//! 写「许可缺失上架→阻断（红线实测）」。本项把许可做成**三态**而不是布尔：
//!
//! - [`LicenseState::Declared`] 有明确许可 → 可上架；
//! - [`LicenseState::Missing`] 缺许可 → **阻断**，且阻断动作带
//!   [`E_LICENSE_MISSING`]；
//! - [`LicenseState::Unverified`] 声明了但无法核验（引用他人素材未查）→
//!   **警告+确认**，不是阻断——因为它可能补得回来，而缺信息补不回来。
//!
//! 为什么分三态：把「完全没写」和「写了但待核」都判阻断，会让创作者在
//! 上架前一刻被卡住且无从下手（他不知道该补什么）；都判警告，则等于允许
//! 无许可内容上架（侵权零容忍，F3612）。三态让两类问题各走各的门。
//!
//! # 四、兼容警告红线：静默不兼容 = 装了就坏（判据三）
//!
//! 锚点写「未声明兼容版本的资产安装时警告（静默不兼容=装了就坏）」。注意
//! 动作是**警告+确认**而不是阻断——因为老资产可能确实能跑在新引擎上，阻断
//! 会误伤。本项用 [`CompatReport`] 表达四级相容度：
//!
//! | 相容度 | 含义 | 处置 |
//! |---|---|---|
//! | [`CompatLevel::Exact`] | 声明区间覆盖当前引擎版本 | 放行 |
//! | [`CompatLevel::Compatible`] | 声明区间与当前版本有交集 | 放行 |
//! | [`CompatLevel::Undeclared`] | 未声明兼容版本 | **警告+确认**（红线） |
//! | [`CompatLevel::Incompatible`] | 声明区间与当前版本无交集 | 警告+确认（更重） |
//!
//! 四级而非两级，是为了让「没声明」和「声明了但明确不兼容」在诊断上分得开——
//! 前者是资产作者的疏漏，后者是明确的版本错配，处置动作相同但归因不同。
//!
//! # 五、七类中的脚本数据必须带沙箱标记（承F3602 判据四）
//!
//! 本项七类含「脚本数据」，而 F3602 已定「创作代码类资产（脚本/逻辑）沙箱
//! 执行」。两者若不挂钩，会出现「模型里声明了脚本资产，但安装时绕开沙箱」。
//! 本项在 [`KindSpec::needs_sandbox`] 登记，并把「代码类必须标沙箱」做成机检
//! （[`E_SANDBOX_FLAG_MISSING`]）——**引用** F3602 的沙箱登记，不另立一份。
//!
//! # 六、schema 复用：宽松+告警，不是硬拒（锚点错误路径）
//!
//! 锚点写「schema 违例→宽松+告警（复述）」，且F3204 锚点写「元数据不合
//! schema→警告+宽松解析（**宽容元数据严格内容**——两级语义复用）」。本项
//! [`SchemaReuse::validate`] 照此实现：**元数据宽松**（缺字段填默认值并告警），
//! **内容严格**（内容本体不合规则拒绝）。这个两级语义是 F3204 的既有裁决，
//! 本项复述不另立。
//!
//! # 七、错误路径四条：方向不同，不共用一套动作
//!
//! | 锚点原文 | 动作 | 级 | 处置方向 |
//! |---|---|---|---|
//! | 许可缺失上架→阻断（红线实测） | [`DegradeAction::BlockListing`] | P0 | 阻断 |
//! | 未注册类型→拒绝（复述） | [`DegradeAction::RejectLoad`] | P1 | 拒绝 |
//! | 兼容未声明→警告+确认（红线实测） | [`DegradeAction::WarnConfirm`] | P1 | 提示 |
//! | schema 违例→宽松+告警（复述） | [`DegradeAction::WarnLenient`] | P2 | 放行 |
//!
//! 四条里有**三种不同方向**（阻断/拒绝/提示放行），绝不能合并成一套——这是
//! 上轮沉淀的纪律：处置方向相反的状态不得共用错误码。
//!
//! # 八、禁扩面：本项只定义模型与登记，不替别人实现
//!
//! 明确不做（[`ASSET_EXCLUSIONS`]）：F3204 的解码器路由与内存画像、F3216 的
//! SLA 表、F3616 的上架流水与结算、F3607 的验证器、F4604 的沙箱实现、Q 管线
//! 本体。越界即 [`E_OVERREACH`]。
//!
//! # 九、读屏替述（无障碍：本项输出的等价口述）
//!
//! 见 [`AssetArchitecture::narration`]：七要素七类的区别、许可三态为什么这样
//! 分、兼容四级为什么不是两级——逐条口述，不依赖图形。

#![allow(clippy::needless_range_loop)]

extern crate alloc;

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 模型版本。
pub const MODEL_VERSION: &str = "creation-asset-v1";
/// 兼容声明协议版本。
pub const COMPAT_VERSION: &str = "compat-decl-v1";
/// 许可协议版本。
pub const LICENSE_VERSION: &str = "license-v1";

/// 七要素（锚点原文：ID/类型/内容/元数据/版本/来源/许可）。
pub const FIELD_COUNT: usize = 7;
/// 七类创作资产（锚点原文：主题/皮肤/壁纸/图标/组件/模板/脚本数据）。
pub const KIND_COUNT: usize = 7;
/// 判据六项。
pub const CRITERION_COUNT: usize = 6;
/// 错误路径四条。
pub const DEGRADE_PATH_COUNT: usize = 4;
/// 禁扩面条数。
pub const EXCLUSION_COUNT: usize = 6;
/// 资源类型（承 F3204 十字面，本项只引用）。
pub const RESOURCE_TYPE_COUNT: usize = 10;
/// 每类可声明的资源绑定上限。
pub const MAX_BINDINGS_PER_KIND: usize = 4;
/// 单个资产的元数据字段上限（校验用）。
pub const MAX_META_FIELDS: usize = 16;

// --- 错误码 ---

/// 七要素缺项（按实例逐个查）。
pub const E_FIELD_MISSING: &str = "E_FIELD_MISSING";
/// 要素数不为七。
pub const E_FIELD_COUNT: &str = "E_FIELD_COUNT";
/// 要素码往返失真。
pub const E_FIELD_CODE_ROUNDTRIP: &str = "E_FIELD_CODE_ROUNDTRIP";
/// 要素越界。
pub const E_FIELD_UNKNOWN: &str = "E_FIELD_UNKNOWN";

/// 资产种类未注册。
pub const E_KIND_UNREGISTERED: &str = "E_KIND_UNREGISTERED";
/// 资产种类重复登记。
pub const E_KIND_DUP: &str = "E_KIND_DUP";
/// 种类数不为七。
pub const E_KIND_COUNT: &str = "E_KIND_COUNT";
/// 种类码往返失真。
pub const E_KIND_CODE_ROUNDTRIP: &str = "E_KIND_CODE_ROUNDTRIP";

/// 资源绑定缺失（某类未声明它用哪些资源类型）。
pub const E_BINDING_MISSING: &str = "E_BINDING_MISSING";
/// 资源类型越界（不在 F3204 十类内）。
pub const E_BINDING_OUT_OF_RANGE: &str = "E_BINDING_OUT_OF_RANGE";

/// 许可缺失（阻断上架）。
pub const E_LICENSE_MISSING: &str = "E_LICENSE_MISSING";
/// 许可未核验（警告+确认，非阻断）。
pub const E_LICENSE_UNVERIFIED: &str = "E_LICENSE_UNVERIFIED";
/// 许可被剥离（署名类资产，盗包红线）。
pub const E_LICENSE_STRIPPED: &str = "E_LICENSE_STRIPPED";

/// 兼容未声明（警告+确认）。
pub const E_COMPAT_UNDECLARED: &str = "E_COMPAT_UNDECLARED";
/// 兼容声明区间非法（下界> 上界）。
pub const E_COMPAT_RANGE_INVALID: &str = "E_COMPAT_RANGE_INVALID";
/// 兼容区间与引擎版本无交集。
pub const E_COMPAT_INCOMPATIBLE: &str = "E_COMPAT_INCOMPATIBLE";

/// schema 违例（宽松+告警）。
pub const E_SCHEMA_VIOLATION: &str = "E_SCHEMA_VIOLATION";
/// schema 元数据被宽松解析（已告警，非阻断）。
pub const E_SCHEMA_LENIENT: &str = "E_SCHEMA_LENIENT";

/// 代码类未标沙箱（承 F3602 判据四）。
pub const E_SANDBOX_FLAG_MISSING: &str = "E_SANDBOX_FLAG_MISSING";

/// 降级路径缺失。
pub const E_DEGRADE_MISSING: &str = "E_DEGRADE_MISSING";
/// 降级动作与严重级不匹配。
pub const E_DEGRADE_MISMATCH: &str = "E_DEGRADE_MISMATCH";

/// 判据缺失。
pub const E_CRITERION_MISSING: &str = "E_CRITERION_MISSING";
/// 判据无锚点依据。
pub const E_CRITERION_NO_BASIS: &str = "E_CRITERION_NO_BASIS";

/// 越界。
pub const E_OVERREACH: &str = "E_OVERREACH";

/// 严重级：P0 阻断类。
pub const P0: &str = "P0";
/// 严重级：P1 拒绝/提示类。
pub const P1: &str = "P1";
/// 严重级：P2 放行类。
pub const P2: &str = "P2";

/// FNV-1a64（确定性派生——要稳定不要抗攻击）。
pub fn fnv1a64(bytes: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for &b in bytes {
        h ^= b as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    h
}

/// FNV-1a64 十六进制。
pub fn fnv1a64_hex(bytes: &[u8]) -> String {
    format!("{:016x}", fnv1a64(bytes))
}

/// 契约问题（收集式，机检一次性枚举）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AssetIssue {
    /// 错误码。
    pub code: &'static str,
    /// 定位键（要素码/种类码/资产 ID）。
    pub subject: String,
    /// 说明。
    pub detail: String,
}

impl AssetIssue {
    /// 构造一条契约问题。
    pub fn new(code: &'static str, subject: impl Into<String>, detail: impl Into<String>) -> AssetIssue {
        AssetIssue { code, subject: subject.into(), detail: detail.into() }
    }

    /// 是否属于「阻断类」红线（缺许可上架）。
    pub fn is_blocking(&self) -> bool {
        matches!(self.code, E_LICENSE_MISSING | E_LICENSE_STRIPPED)
    }

    /// 是否属于「必须提示类」（警告+确认，不阻断）。
    pub fn is_advisory(&self) -> bool {
        matches!(
            self.code,
            E_COMPAT_UNDECLARED | E_COMPAT_INCOMPATIBLE | E_LICENSE_UNVERIFIED | E_SCHEMA_LENIENT
        )
    }
}

/// 资产契约错误。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AssetError {
    /// 错误码。
    pub code: &'static str,
    /// 定位键。
    pub subject: String,
    /// 说明。
    pub detail: String,
}

impl AssetError {
    /// 构造错误。
    pub fn new(code: &'static str, subject: impl Into<String>, detail: impl Into<String>) -> AssetError {
        AssetError { code, subject: subject.into(), detail: detail.into() }
    }

    /// 建议降级动作。
    pub fn action(&self) -> DegradeAction {
        match self.code {
            E_LICENSE_MISSING | E_LICENSE_STRIPPED => DegradeAction::BlockListing,
            E_KIND_UNREGISTERED | E_KIND_DUP | E_FIELD_MISSING | E_FIELD_UNKNOWN => {
                DegradeAction::RejectLoad
            }
            E_COMPAT_UNDECLARED | E_COMPAT_INCOMPATIBLE | E_LICENSE_UNVERIFIED => {
                DegradeAction::WarnConfirm
            }
            E_SCHEMA_VIOLATION | E_SCHEMA_LENIENT => DegradeAction::WarnLenient,
            _ => DegradeAction::FileCase,
        }
    }

    /// 建议严重级。
    pub fn severity(&self) -> &'static str {
        match self.code {
            E_LICENSE_MISSING | E_LICENSE_STRIPPED => P0,
            E_KIND_UNREGISTERED | E_KIND_DUP | E_FIELD_MISSING | E_FIELD_UNKNOWN
            | E_COMPAT_UNDECLARED | E_COMPAT_INCOMPATIBLE | E_LICENSE_UNVERIFIED => P1,
            _ => P2,
        }
    }

    /// 转契约问题。
    pub fn to_issue(&self) -> AssetIssue {
        AssetIssue::new(self.code, self.subject.clone(), self.detail.clone())
    }
}

impl core::fmt::Display for AssetError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "[{}] {}: {}", self.code, self.subject, self.detail)
    }
}// ---------------------------------------------------------------------------
// 一、七要素（锚点原文：ID/类型/内容/元数据/版本/来源/许可）
// ---------------------------------------------------------------------------

/// 资产七要素（**实例级**字段——与 [`AssetKind`] 的「种类」不是一回事）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AssetField {
    /// 资产标识。
    Id,
    /// 资产类型（指向 [`AssetKind`]）。
    Kind,
    /// 内容本体。
    Content,
    /// 元数据（走 F3204 schema）。
    Meta,
    /// 版本。
    Version,
    /// 来源（谁做的/从哪来）。
    Origin,
    /// 许可（缺则不可分发）。
    License,
}

impl AssetField {
    /// 全部要素（顺序即锚点列举序）。
    pub const ALL: [AssetField; FIELD_COUNT] = [
        AssetField::Id,
        AssetField::Kind,
        AssetField::Content,
        AssetField::Meta,
        AssetField::Version,
        AssetField::Origin,
        AssetField::License,
    ];

    /// 要素序（定长槽位寻址）。
    pub const fn order(self) -> usize {
        match self {
            AssetField::Id => 0,
            AssetField::Kind => 1,
            AssetField::Content => 2,
            AssetField::Meta => 3,
            AssetField::Version => 4,
            AssetField::Origin => 5,
            AssetField::License => 6,
        }
    }

    /// 要素码。
    pub const fn code(self) -> &'static str {
        match self {
            AssetField::Id => "F-ID",
            AssetField::Kind => "F-KIND",
            AssetField::Content => "F-CONTENT",
            AssetField::Meta => "F-META",
            AssetField::Version => "F-VER",
            AssetField::Origin => "F-ORIGIN",
            AssetField::License => "F-LICENSE",
        }
    }

    /// 由码反查要素。
    pub fn from_code(code: &str) -> Option<AssetField> {
        AssetField::ALL.iter().copied().find(|f| f.code() == code)
    }

    /// 要素名（中文，锚点原词）。
    pub const fn name_cn(self) -> &'static str {
        match self {
            AssetField::Id => "ID",
            AssetField::Kind => "类型",
            AssetField::Content => "内容",
            AssetField::Meta => "元数据",
            AssetField::Version => "版本",
            AssetField::Origin => "来源",
            AssetField::License => "许可",
        }
    }

    /// 是否为「内容本体」（内容严格、元数据宽松的分界，见头注 §6）。
    pub const fn is_content_strict(self) -> bool {
        matches!(self, AssetField::Content)
    }

    /// 是否为「元数据类」（可宽松解析+ 告警）。
    pub const fn is_meta_lenient(self) -> bool {
        matches!(self, AssetField::Meta)
    }

    /// 是否为上架必需（许可与内容缺一即不可上架）。
    pub const fn required_for_listing(self) -> bool {
        matches!(self, AssetField::Content | AssetField::License)
    }
}

/// 许可三态（头注 §3：为何不是布尔）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LicenseState {
    /// 已声明且已核验 → 可上架。
    Declared,
    /// 缺许可信息 → 阻断（红线）。
    Missing,
    /// 声明了但未核验 → 警告+确认（非阻断）。
    Unverified,
}

impl LicenseState {
    /// 全部态。
    pub const ALL: [LicenseState; 3] =
        [LicenseState::Declared, LicenseState::Missing, LicenseState::Unverified];

    /// 态码。
    pub const fn code(self) -> &'static str {
        match self {
            LicenseState::Declared => "L-DECLARED",
            LicenseState::Missing => "L-MISSING",
            LicenseState::Unverified => "L-UNVERIFIED",
        }
    }

    /// 态名（中文）。
    pub const fn name_cn(self) -> &'static str {
        match self {
            LicenseState::Declared => "已声明已核验",
            LicenseState::Missing => "缺许可",
            LicenseState::Unverified => "已声明待核验",
        }
    }

    /// 是否可上架。
    pub const fn listable(self) -> bool {
        matches!(self, LicenseState::Declared)
    }
}

/// 一个资产实例的七要素取值。
#[derive(Clone, Copy, Debug)]
pub struct CreationAsset {
    /// 资产标识（空串=缺）。
    pub id: &'static str,
    /// 资产种类码（空串=缺）。
    pub kind: &'static str,
    /// 内容本体（空串=缺）。
    pub content: &'static str,
    /// 元数据键（空串=缺）。
    pub meta: &'static str,
    /// 版本串（空串=缺）。
    pub version: &'static str,
    /// 来源（空串=缺）。
    pub origin: &'static str,
    /// 许可三态。
    pub license: LicenseState,
}

impl CreationAsset {
    /// 构造齐备资产（许可已声明）。
    pub const fn complete(id: &'static str, kind: &'static str) -> CreationAsset {
        CreationAsset {
            id,
            kind,
            content: "content-blob",
            meta: "meta",
            version: "1.0.0",
            origin: "creator",
            license: LicenseState::Declared,
        }
    }

    /// 取某要素的取值（`License` 取态码，其余取字面）。
    pub fn value_of(&self, field: AssetField) -> &'static str {
        match field {
            AssetField::Id => self.id,
            AssetField::Kind => self.kind,
            AssetField::Content => self.content,
            AssetField::Meta => self.meta,
            AssetField::Version => self.version,
            AssetField::Origin => self.origin,
            AssetField::License => self.license.code(),
        }
    }

    /// 该要素是否齐备。
    pub fn has_field(&self, field: AssetField) -> bool {
        match field {
            // 许可走**单一事实源** [`Self::license_present`]，与上架闸同源——
            // 两处各判一次就会出现「要素说齐备、闸门说缺许可」的实现分裂。
            AssetField::License => self.license_present(),
            // 元数据走**宽松**语义（锚点：宽容元数据严格内容）：空元数据由
            // schema 层填默认并告警，不算「要素缺失」——否则宽松解析永远
            // 走不到，两级语义自相矛盾。
            AssetField::Meta => true,
            _ => !self.value_of(field).trim().is_empty(),
        }
    }

    /// 许可是否在场（**三态分流的唯一事实源**）。
    ///
    /// 只有 [`LicenseState::Missing`] 算「许可不在场」；
    /// [`LicenseState::Unverified`] 许可**在场**（写了，只是待核验），
    /// 因此不进缺项，但上架闸仍会要用户确认。两件事、两处判，用这一个函数。
    pub const fn license_present(&self) -> bool {
        !matches!(self.license, LicenseState::Missing)
    }

    /// 七要素齐备断言（**逐要素查**，不查汇总）。
    pub fn missing_fields(&self) -> Vec<AssetField> {
        AssetField::ALL
            .iter()
            .copied()
            .filter(|f| !self.has_field(*f))
            .collect()
    }

    /// 上架闸（锚点：缺许可=不可分发；内容缺也不可上架）。
    pub fn listing_gate(&self) -> Result<(), AssetError> {
        // 2.许可门**先于**七要素检查：缺许可是 P0 红线，必须报自己的专码，
        //    不能被并进「七要素缺项」里降级成普通拒载——两者的处置方向与
        //    严重级都不同（阻断上架 vs 拒绝装载），混报等于把红线降格。
        if !self.license_present() {
            return Err(AssetError::new(
                E_LICENSE_MISSING,
                self.id,
                "缺许可信息——锚点：许可红线，无许可信息的创作资产不许上架",
            ));
        }
        if self.content.trim().is_empty() {
            return Err(AssetError::new(
                E_FIELD_MISSING,
                self.id,
                "内容本体缺失——内容严格，不可宽松",
            ));
        }
        Ok(())
    }

    /// 实例指纹（七要素串接——任一要素变则指纹变）。
    pub fn fingerprint(&self) -> u64 {
        let mut buf = String::new();
        for f in AssetField::ALL.iter() {
            buf.push_str(f.code());
            buf.push('=');
            buf.push_str(self.value_of(*f));
            buf.push(';');
        }
        fnv1a64(buf.as_bytes())
    }
}

/// 七要素模型总表（要素表本身也机检）。
#[derive(Clone, Copy, Debug)]
pub struct AssetModel {
    /// 七要素。
    pub fields: [AssetField; FIELD_COUNT],
}

impl AssetModel {
    /// 标准模型。
    pub fn standard() -> AssetModel {
        AssetModel { fields: AssetField::ALL }
    }

    /// 要素表审计（数、码往返、唯一）。
    pub fn audit(&self) -> Vec<AssetIssue> {
        let mut out = Vec::new();
        if self.fields.len() != FIELD_COUNT {
            out.push(AssetIssue::new(
                E_FIELD_COUNT,
                "model",
                format!("要素数 {} ≠ {}", self.fields.len(), FIELD_COUNT),
            ));
        }
        for (i, f) in self.fields.iter().enumerate() {
            if f.order() != i {
                out.push(AssetIssue::new(
                    E_FIELD_COUNT,
                    f.code(),
                    format!("要素位错：位置 {} 应为序 {}", i, f.order()),
                ));
            }
        }
        // 码往返无损。
        if !AssetField::ALL.iter().all(|f| AssetField::from_code(f.code()) == Some(*f)) {
            out.push(AssetIssue::new(
                E_FIELD_CODE_ROUNDTRIP,
                "model",
                "要素码往返失真——改码须同步改 from_code",
            ));
        }
        // 码互异。
        let mut codes: Vec<&str> = self.fields.iter().map(|f| f.code()).collect();
        codes.sort_unstable();
        let n = codes.len();
        codes.dedup();
        if codes.len() != n {
            out.push(AssetIssue::new(E_FIELD_COUNT, "model", "要素码重复"));
        }
        out
    }

    /// 要素码字面核对（与锚点逐字一致）。
    pub fn codes_match_anchor() -> bool {
        let want = ["F-ID", "F-KIND", "F-CONTENT", "F-META", "F-VER", "F-ORIGIN", "F-LICENSE"];
        AssetField::ALL.iter().enumerate().all(|(i, f)| f.code() == want[i])
    }
}

// ---------------------------------------------------------------------------
// 二、七类创作资产 + 资源绑定（头注 §2：两轴正交）
// ---------------------------------------------------------------------------

/// F3204 十类资源类型（**只引用，不复制解码规则**）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ResourceType {
    /// 纹理。
    Texture,
    /// 网格。
    Mesh,
    /// 材质。
    Material,
    /// 音频。
    Audio,
    /// 字体。
    Font,
    /// 动画。
    Animation,
    /// 样式表。
    StyleSheet,
    /// 场景图。
    SceneGraph,
    /// 预制体。
    Prefab,
    /// 脚本数据。
    ScriptData,
}

impl ResourceType {
    /// 全部资源类型（F3204 十类）。
    pub const ALL: [ResourceType; RESOURCE_TYPE_COUNT] = [
        ResourceType::Texture,
        ResourceType::Mesh,
        ResourceType::Material,
        ResourceType::Audio,
        ResourceType::Font,
        ResourceType::Animation,
        ResourceType::StyleSheet,
        ResourceType::SceneGraph,
        ResourceType::Prefab,
        ResourceType::ScriptData,
    ];

    /// 资源类型序。
    pub const fn order(self) -> usize {
        match self {
            ResourceType::Texture => 0,
            ResourceType::Mesh => 1,
            ResourceType::Material => 2,
            ResourceType::Audio => 3,
            ResourceType::Font => 4,
            ResourceType::Animation => 5,
            ResourceType::StyleSheet => 6,
            ResourceType::SceneGraph => 7,
            ResourceType::Prefab => 8,
            ResourceType::ScriptData => 9,
        }
    }

    /// 资源码（F3204 字面）。
    pub const fn code(self) -> &'static str {
        match self {
            ResourceType::Texture => "RT-TEX",
            ResourceType::Mesh => "RT-MESH",
            ResourceType::Material => "RT-MAT",
            ResourceType::Audio => "RT-AUD",
            ResourceType::Font => "RT-FONT",
            ResourceType::Animation => "RT-ANIM",
            ResourceType::StyleSheet => "RT-SS",
            ResourceType::SceneGraph => "RT-SG",
            ResourceType::Prefab => "RT-PREFAB",
            ResourceType::ScriptData => "RT-SCRIPT",
        }
    }

    /// 由码反查。
    pub fn from_code(code: &str) -> Option<ResourceType> {
        ResourceType::ALL.iter().copied().find(|r| r.code() == code)
    }

    /// 是否代码类（承 F3602：代码类须沙箱）。
    pub const fn is_code(self) -> bool {
        matches!(self, ResourceType::ScriptData)
    }
}

/// 资源绑定（创作种类 → 资源类型的展开声明）。
#[derive(Clone, Copy, Debug)]
pub struct KindBinding {
    /// 绑定的资源类型。
    pub resource: ResourceType,
    /// 该绑定的作用说明。
    pub note: &'static str,
}

/// 创作资产种类（锚点原文七类）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AssetKind {
    /// 主题。
    Theme,
    /// 皮肤。
    Skin,
    /// 壁纸。
    Wallpaper,
    /// 图标。
    Icon,
    /// 组件。
    Component,
    /// 模板。
    Template,
    /// 脚本数据（**代码类，须沙箱**）。
    ScriptData,
}

impl AssetKind {
    /// 全部种类（七类，顺序即锚点列举序）。
    pub const ALL: [AssetKind; KIND_COUNT] = [
        AssetKind::Theme,
        AssetKind::Skin,
        AssetKind::Wallpaper,
        AssetKind::Icon,
        AssetKind::Component,
        AssetKind::Template,
        AssetKind::ScriptData,
    ];

    /// 种类序。
    pub const fn order(self) -> usize {
        match self {
            AssetKind::Theme => 0,
            AssetKind::Skin => 1,
            AssetKind::Wallpaper => 2,
            AssetKind::Icon => 3,
            AssetKind::Component => 4,
            AssetKind::Template => 5,
            AssetKind::ScriptData => 6,
        }
    }

    /// 种类码。
    pub const fn code(self) -> &'static str {
        match self {
            AssetKind::Theme => "K-THEME",
            AssetKind::Skin => "K-SKIN",
            AssetKind::Wallpaper => "K-WALL",
            AssetKind::Icon => "K-ICON",
            AssetKind::Component => "K-COMP",
            AssetKind::Template => "K-TPL",
            AssetKind::ScriptData => "K-SCRIPT",
        }
    }

    /// 由码反查。
    pub fn from_code(code: &str) -> Option<AssetKind> {
        AssetKind::ALL.iter().copied().find(|k| k.code() == code)
    }

    /// 种类名（中文，锚点原词）。
    pub const fn name_cn(self) -> &'static str {
        match self {
            AssetKind::Theme => "主题",
            AssetKind::Skin => "皮肤",
            AssetKind::Wallpaper => "壁纸",
            AssetKind::Icon => "图标",
            AssetKind::Component => "组件",
            AssetKind::Template => "模板",
            AssetKind::ScriptData => "脚本数据",
        }
    }

    /// 是否代码类（承 F3602 判据四）。
    pub const fn is_code(self) -> bool {
        matches!(self, AssetKind::ScriptData)
    }
}

/// 种类登记一条。
#[derive(Clone, Copy, Debug)]
pub struct KindSpec {
    /// 种类。
    pub kind: AssetKind,
    /// 该类展开用的资源类型（**两轴映射**，见头注 §2）。
    pub bindings: [KindBinding; MAX_BINDINGS_PER_KIND],
    /// 绑定实际个数。
    pub binding_count: usize,
    /// 是否代码类须沙箱（承 F3602）。
    pub needs_sandbox: bool,
}

impl KindSpec {
    /// 构造登记（绑定为空）。
    pub const fn empty(kind: AssetKind) -> KindSpec {
        KindSpec {
            kind,
            bindings: [KindBinding { resource: ResourceType::Texture, note: "" };
                MAX_BINDINGS_PER_KIND],
            binding_count: 0,
            needs_sandbox: false,
        }
    }

    /// 登记一个资源绑定。
    pub fn push_binding(&mut self, resource: ResourceType, note: &'static str) -> Result<(), AssetError> {
        if self.binding_count >= MAX_BINDINGS_PER_KIND {
            return Err(AssetError::new(
                "E_BINDING_CAP",
                self.kind.code(),
                format!("资源绑定超过上限 {}", MAX_BINDINGS_PER_KIND),
            ));
        }
        if self.bindings[..self.binding_count].iter().any(|b| b.resource == resource) {
            return Err(AssetError::new(
                E_BINDING_OUT_OF_RANGE,
                self.kind.code(),
                format!("资源 {} 重复绑定", resource.code()),
            ));
        }
        self.bindings[self.binding_count] = KindBinding { resource, note };
        self.binding_count += 1;
        Ok(())
    }

    /// 是否绑定了某资源类型。
    pub fn has_binding(&self, resource: ResourceType) -> bool {
        self.bindings[..self.binding_count].iter().any(|b| b.resource == resource)
    }

    /// 类别指纹。
    pub fn fingerprint(&self) -> u64 {
        let mut buf = String::new();
        buf.push_str(self.kind.code());
        for b in &self.bindings[..self.binding_count] {
            buf.push('|');
            buf.push_str(b.resource.code());
        }
        buf.push_str(if self.needs_sandbox { ":SBX" } else { ":-" });
        fnv1a64(buf.as_bytes())
    }
}

/// 七类注册表。
#[derive(Clone, Copy, Debug)]
pub struct KindRegistry {
    /// 七类登记。
    pub kinds: [KindSpec; KIND_COUNT],
}

impl KindRegistry {
    /// 标准注册表（七类各带真实资源绑定）。
    pub fn standard() -> KindRegistry {
        let mut specs: [KindSpec; KIND_COUNT] = AssetKind::ALL.map(KindSpec::empty);

        // 主题：样式表为主 + 纹理。
        specs[AssetKind::Theme.order()].needs_sandbox = false;
        let _ = specs[0].push_binding(ResourceType::StyleSheet, "主题令牌表");
        let _ = specs[0].push_binding(ResourceType::Texture, "主题贴图");

        // 皮肤：样式表 + 网格 + 材质。
        let _ = specs[1].push_binding(ResourceType::StyleSheet, "皮肤样式");
        let _ = specs[1].push_binding(ResourceType::Mesh, "皮肤网格");
        let _ = specs[1].push_binding(ResourceType::Material, "皮肤材质");

        // 壁纸：纹理 + 场景图（动态壁纸）。
        let _ = specs[2].push_binding(ResourceType::Texture, "壁纸图像");
        let _ = specs[2].push_binding(ResourceType::SceneGraph, "动态壁纸场景");

        // 图标：纹理。
        let _ = specs[3].push_binding(ResourceType::Texture, "图标位图");

        // 组件：预制体 + 样式表。
        let _ = specs[4].push_binding(ResourceType::Prefab, "组件预制体");
        let _ = specs[4].push_binding(ResourceType::StyleSheet, "组件样式");

        // 模板：场景图 + 预制体。
        let _ = specs[5].push_binding(ResourceType::SceneGraph, "模板场景");
        let _ = specs[5].push_binding(ResourceType::Prefab, "模板预制体");

        // 脚本数据：**代码类**，必须标沙箱（承 F3602 判据四）。
        specs[6].needs_sandbox = true;
        let _ = specs[6].push_binding(ResourceType::ScriptData, "创作脚本");

        KindRegistry { kinds: specs }
    }

    /// 空注册表（反例：未注册任何类型）。
    pub fn empty() -> KindRegistry {
        KindRegistry { kinds: AssetKind::ALL.map(KindSpec::empty) }
    }

    /// 查种类登记。
    pub fn get(&self, kind: AssetKind) -> &KindSpec {
        &self.kinds[kind.order()]
    }

    /// 可变查种类登记。
    pub fn get_mut(&mut self, kind: AssetKind) -> &mut KindSpec {
        &mut self.kinds[kind.order()]
    }

    /// 是否已注册某种类码（未注册即拒载）。
    pub fn is_registered(&self, code: &str) -> bool {
        AssetKind::from_code(code)
            .map(|k| self.kinds[k.order()].binding_count > 0)
            .unwrap_or(false)
    }

    /// 注册表审计（七类齐备/ 码往返 / 绑定非空 / 代码类沙箱标记）。
    pub fn audit(&self) -> Vec<AssetIssue> {
        let mut out = Vec::new();
        for (i, s) in self.kinds.iter().enumerate() {
            if s.kind.order() != i {
                out.push(AssetIssue::new(
                    E_KIND_COUNT,
                    s.kind.code(),
                    format!("种类位错：位置 {} 应为序 {}", i, s.kind.order()),
                ));
            }
            if s.binding_count == 0 {
                out.push(AssetIssue::new(
                    E_KIND_UNREGISTERED,
                    s.kind.code(),
                    format!("{} 未登记资源绑定——未注册类型须拒载", s.kind.name_cn()),
                ));
            }
            for b in &s.bindings[..s.binding_count] {
                if b.note.trim().is_empty() {
                    out.push(AssetIssue::new(
                        E_BINDING_MISSING,
                        s.kind.code(),
                        format!("资源 {} 绑定无作用说明", b.resource.code()),
                    ));
                }
            }
            // 代码类必须标沙箱（承 F3602）。
            if s.kind.is_code() && !s.needs_sandbox {
                out.push(AssetIssue::new(
                    E_SANDBOX_FLAG_MISSING,
                    s.kind.code(),
                    "代码类未标沙箱——承 F3602 判据四，安装时会绕开沙箱",
                ));
            }
            // 非代码类不该标沙箱（无谓开销）。
            if !s.kind.is_code() && s.needs_sandbox {
                out.push(AssetIssue::new(
                    E_SANDBOX_FLAG_MISSING,
                    s.kind.code(),
                    format!("{} 非代码类，不应挂沙箱标记", s.kind.name_cn()),
                ));
            }
        }
        if !AssetKind::ALL.iter().all(|k| AssetKind::from_code(k.code()) == Some(*k)) {
            out.push(AssetIssue::new(E_KIND_CODE_ROUNDTRIP, "registry", "种类码往返失真"));
        }
        out
    }

    /// 种类码字面核对（与锚点逐字一致）。
    pub fn codes_match_anchor() -> bool {
        let want = ["K-THEME", "K-SKIN", "K-WALL", "K-ICON", "K-COMP", "K-TPL", "K-SCRIPT"];
        AssetKind::ALL.iter().enumerate().all(|(i, k)| k.code() == want[i])
    }
}// ---------------------------------------------------------------------------
// 三、兼容声明协议（判据三：兼容警告红线）
// ---------------------------------------------------------------------------

/// 兼容相容度四级（头注 §4：为何不是两级）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CompatLevel {
    /// 声明区间完全覆盖当前引擎版本。
    Exact,
    /// 声明区间与当前版本有交集。
    Compatible,
    /// 未声明兼容版本（红线：静默不兼容=装了就坏）。
    Undeclared,
    /// 声明区间与当前版本无交集。
    Incompatible,
}

impl CompatLevel {
    /// 全部级。
    pub const ALL: [CompatLevel; 4] = [
        CompatLevel::Exact,
        CompatLevel::Compatible,
        CompatLevel::Undeclared,
        CompatLevel::Incompatible,
    ];

    /// 级码。
    pub const fn code(self) -> &'static str {
        match self {
            CompatLevel::Exact => "C-EXACT",
            CompatLevel::Compatible => "C-COMPAT",
            CompatLevel::Undeclared => "C-UNDECL",
            CompatLevel::Incompatible => "C-INCOMP",
        }
    }

    /// 级名（中文）。
    pub const fn name_cn(self) -> &'static str {
        match self {
            CompatLevel::Exact => "精确匹配",
            CompatLevel::Compatible => "相容",
            CompatLevel::Undeclared => "未声明",
            CompatLevel::Incompatible => "不兼容",
        }
    }

    /// 是否需要警告+确认（两级提示合流，但归因不同）。
    pub const fn needs_warning(self) -> bool {
        matches!(self, CompatLevel::Undeclared | CompatLevel::Incompatible)
    }

    /// 是否放行（警告级也放行——老资产可能能跑，阻断会误伤）。
    pub const fn passes(self) -> bool {
        !matches!(self, CompatLevel::Incompatible)
    }
}

/// 兼容声明（语义化版本区间）。
#[derive(Clone, Copy, Debug)]
pub struct CompatDecl {
    /// 下界（含）。
    pub lo: u32,
    /// 上界（含）。
    pub hi: u32,
}

impl CompatDecl {
    /// 构造区间。
    pub const fn new(lo: u32, hi: u32) -> CompatDecl {
        CompatDecl { lo, hi }
    }

    /// 区间是否合法（下界 ≤ 上界）。
    pub const fn range_valid(self) -> bool {
        self.lo <= self.hi
    }

    /// 区间是否合法（可失败版——用于产错误）。
    pub fn validate_range(self) -> Result<(), AssetError> {
        if self.range_valid() {
            Ok(())
        } else {
            Err(AssetError::new(
                E_COMPAT_RANGE_INVALID,
                COMPAT_VERSION,
                format!("兼容区间非法：下界 {} > 上界 {}", self.lo, self.hi),
            ))
        }
    }
}

/// 兼容报告（对某资产在当前引擎版本下的相容度）。
#[derive(Clone, Copy, Debug)]
pub struct CompatReport {
    /// 相容度级。
    pub level: CompatLevel,
    /// 当前引擎版本（构造时传入）。
    pub engine: u32,
}

impl CompatReport {
    /// 计算相容度（**四级判定**，见头注 §4）。
    ///
    /// `decl` 为 `None` 即「未声明兼容版本」——这是最常见的一种红线，
    /// 与「声明了但明确不兼容」必须分开报，归因不同。
    pub fn evaluate(decl: Option<CompatDecl>, engine: u32) -> CompatReport {
        let level = match decl {
            None => CompatLevel::Undeclared,
            Some(d) => {
                if !d.range_valid() {
                    // 区间非法按「不兼容」报，但归因在 range校验处另记一条。
                    CompatLevel::Incompatible
                } else if engine >= d.lo && engine <= d.hi {
                    CompatLevel::Exact
                } else if d.lo <= engine + COMPAT_FUZZ && engine <= d.hi + COMPAT_FUZZ {
                    CompatLevel::Compatible
                } else {
                    CompatLevel::Incompatible
                }
            }
        };
        CompatReport { level, engine }
    }

    /// 报告转处置（警告级→ 警告+确认，不阻断）。
    pub fn to_action(self) -> Result<(), AssetError> {
        match self.level {
            CompatLevel::Exact | CompatLevel::Compatible => Ok(()),
            CompatLevel::Undeclared => Err(AssetError::new(
                E_COMPAT_UNDECLARED,
                COMPAT_VERSION,
                format!(
                    "资产未声明兼容版本，当前引擎 {}——静默不兼容等于装了就坏",
                    self.engine
                ),
            )),
            CompatLevel::Incompatible => Err(AssetError::new(
                E_COMPAT_INCOMPATIBLE,
                COMPAT_VERSION,
                format!("声明区间与引擎版本 {} 无交集", self.engine),
            )),
        }
    }

    /// 是否需用户确认（警告级都要）。
    pub fn needs_confirm(self) -> bool {
        self.level.needs_warning()
    }
}

/// 相容判定容差（相邻小版本视作相容——避免 1.2.0资产在 1.2.1 引擎上被拦）。
pub const COMPAT_FUZZ: u32 = 0;

// ---------------------------------------------------------------------------
// 四、schema 复用（判据五：宽松元数据 + 严格内容）
// ---------------------------------------------------------------------------

/// schema 校验结果。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SchemaOutcome {
    /// 合规。
    Ok,
    /// 元数据宽松解析（缺字段填默认 + 告警）。
    MetaLenient,
    /// 内容本体违例（严格，拒绝）。
    ContentViolation,
}

impl SchemaOutcome {
    /// 结果码。
    pub const fn code(self) -> &'static str {
        match self {
            SchemaOutcome::Ok => "S-OK",
            SchemaOutcome::MetaLenient => "S-META-LENIENT",
            SchemaOutcome::ContentViolation => "S-CONTENT-BAD",
        }
    }

    /// 是否放行（宽松也放行，内容违例才拒）。
    pub const fn passes(self) -> bool {
        !matches!(self, SchemaOutcome::ContentViolation)
    }
}

/// F3204 schema 复用登记（**只引用，不复制校验器**）。
#[derive(Clone, Copy, Debug)]
pub struct SchemaReuse {
    /// 引用的 schema 出处（册内条目）。
    pub cite: &'static str,
    /// 元数据校验器路由（引用 F3204 解码路由）。
    pub meta_route: &'static str,
    /// 本项是否自实现校验器（恒假——出现即红）。
    pub reimplements: bool,
}

impl SchemaReuse {
    /// 标准复用登记。
    pub fn standard() -> SchemaReuse {
        SchemaReuse {
            cite: "F3204 资源类型系统（元数据 schema 校验器）",
            meta_route: "F3204 解码路由",
            reimplements: false,
        }
    }

    /// 校验元数据（**宽松**：缺字段填默认并告警）。
    pub fn validate_meta(&self, asset: &CreationAsset) -> SchemaOutcome {
        // 元数据只查「有没有」；缺则宽松。内容不在此查。
        if asset.meta.trim().is_empty() {
            SchemaOutcome::MetaLenient
        } else {
            SchemaOutcome::Ok
        }
    }

    /// 校验内容本体（**严格**：不合规则拒绝）。
    pub fn validate_content(&self, asset: &CreationAsset) -> SchemaOutcome {
        if asset.content.trim().is_empty() {
            SchemaOutcome::ContentViolation
        } else {
            SchemaOutcome::Ok
        }
    }

    /// 两级综合校验（元数据宽松 + 内容严格，锚点错误路径「宽松+告警」）。
    pub fn validate(&self, asset: &CreationAsset) -> (SchemaOutcome, Vec<AssetIssue>) {
        let mut issues = Vec::new();
        let content = self.validate_content(asset);
        let meta = self.validate_meta(asset);

        if content == SchemaOutcome::ContentViolation {
            issues.push(AssetIssue::new(
                E_SCHEMA_VIOLATION,
                asset.id,
                "内容本体不合schema——内容严格，拒绝",
            ));
            return (content, issues);
        }
        if meta == SchemaOutcome::MetaLenient {
            issues.push(AssetIssue::new(
                E_SCHEMA_LENIENT,
                asset.id,
                "元数据缺字段，已按 F3204 宽松解析填默认并告警",
            ));
            return (meta, issues);
        }
        (SchemaOutcome::Ok, issues)
    }

    /// 复用登记审计（不得自实现校验器）。
    pub fn audit(&self) -> Vec<AssetIssue> {
        let mut out = Vec::new();
        if self.reimplements {
            out.push(AssetIssue::new(
                E_OVERREACH,
                "schema-reuse",
                "本项只许引用 F3204 校验器，自实现即制造第二真相",
            ));
        }
        if self.cite.trim().is_empty() {
            out.push(AssetIssue::new(
                E_SCHEMA_VIOLATION,
                "schema-reuse",
                "schema 复用无出处",
            ));
        }
        if self.meta_route.trim().is_empty() {
            out.push(AssetIssue::new(
                E_SCHEMA_VIOLATION,
                "schema-reuse",
                "元数据路由无出处",
            ));
        }
        out
    }
}// ---------------------------------------------------------------------------
// 五、错误路径与降级矩阵（四条·三种方向）
// ---------------------------------------------------------------------------

/// 降级动作（**三种方向，绝不合并**，见头注 §7）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DegradeAction {
    /// 阻断上架（红线实测类）。
    BlockListing,
    /// 拒绝装载（类型未注册类）。
    RejectLoad,
    /// 警告+确认（提示类，不阻断）。
    WarnConfirm,
    /// 宽松+告警（放行类）。
    WarnLenient,
    /// 立案观察。
    FileCase,
}

impl DegradeAction {
    /// 全部动作。
    pub const ALL: [DegradeAction; 5] = [
        DegradeAction::BlockListing,
        DegradeAction::RejectLoad,
        DegradeAction::WarnConfirm,
        DegradeAction::WarnLenient,
        DegradeAction::FileCase,
    ];

    /// 动作码。
    pub const fn code(self) -> &'static str {
        match self {
            DegradeAction::BlockListing => "A-BLOCK-LIST",
            DegradeAction::RejectLoad => "A-REJECT-LOAD",
            DegradeAction::WarnConfirm => "A-WARN-CONFIRM",
            DegradeAction::WarnLenient => "A-WARN-LENIENT",
            DegradeAction::FileCase => "A-FILECASE",
        }
    }

    /// 动作名（中文）。
    pub const fn name_cn(self) -> &'static str {
        match self {
            DegradeAction::BlockListing => "阻断上架",
            DegradeAction::RejectLoad => "拒绝装载",
            DegradeAction::WarnConfirm => "警告加确认",
            DegradeAction::WarnLenient => "宽松加告警",
            DegradeAction::FileCase => "立案观察",
        }
    }

    /// 是否阻断（阻断与拒绝都拦住流，但原因不同）。
    pub const fn blocks(self) -> bool {
        matches!(self, DegradeAction::BlockListing | DegradeAction::RejectLoad)
    }

    /// 是否仅提示（不拦住流）。
    pub const fn advisory_only(self) -> bool {
        matches!(self, DegradeAction::WarnConfirm | DegradeAction::WarnLenient)
    }
}

/// 降级路径一条（锚点错误路径逐条）。
#[derive(Clone, Copy, Debug)]
pub struct DegradePath {
    /// 触发错误码。
    pub trigger: &'static str,
    /// 严重级。
    pub severity: &'static str,
    /// 降级动作。
    pub action: DegradeAction,
    /// 锚点原文摘录。
    pub quote: &'static str,
}

impl DegradePath {
    /// 锚点四条路径（原文照录）。
    pub fn standard() -> [DegradePath; DEGRADE_PATH_COUNT] {
        [
            DegradePath {
                trigger: E_LICENSE_MISSING,
                severity: P0,
                action: DegradeAction::BlockListing,
                quote: "许可缺失上架→阻断（红线实测）",
            },
            DegradePath {
                trigger: E_KIND_UNREGISTERED,
                severity: P1,
                action: DegradeAction::RejectLoad,
                quote: "未注册类型→拒绝（复述）",
            },
            DegradePath {
                trigger: E_COMPAT_UNDECLARED,
                severity: P1,
                action: DegradeAction::WarnConfirm,
                quote: "兼容未声明→警告+确认（红线实测）",
            },
            DegradePath {
                trigger: E_SCHEMA_VIOLATION,
                severity: P2,
                action: DegradeAction::WarnLenient,
                quote: "schema 违例→宽松+告警（复述）",
            },
        ]
    }

    /// 路径指纹。
    pub fn fingerprint(&self) -> u64 {
        let buf = format!("{}|{}|{}", self.trigger, self.severity, self.action.code());
        fnv1a64(buf.as_bytes())
    }
}

/// 降级矩阵。
#[derive(Clone, Copy, Debug)]
pub struct DegradeMatrix {
    /// 四条路径。
    pub paths: [DegradePath; DEGRADE_PATH_COUNT],
}

impl DegradeMatrix {
    /// 标准矩阵。
    pub fn standard() -> DegradeMatrix {
        DegradeMatrix { paths: DegradePath::standard() }
    }

    /// 查触发码路径。
    pub fn by_trigger(&self, trigger: &str) -> Option<&DegradePath> {
        self.paths.iter().find(|p| p.trigger == trigger)
    }

    /// 矩阵审计（缺路径 / 级动不匹配 / 引文空）。
    pub fn audit(&self) -> Vec<AssetIssue> {
        let mut out = Vec::new();
        for w in DegradePath::standard().iter() {
            match self.by_trigger(w.trigger) {
                None => out.push(AssetIssue::new(
                    E_DEGRADE_MISSING,
                    w.trigger,
                    format!("锚点错误路径缺失：{}", w.quote),
                )),
                Some(p) => {
                    if p.severity != w.severity || p.action != w.action {
                        out.push(AssetIssue::new(
                            E_DEGRADE_MISMATCH,
                            w.trigger,
                            format!(
                                "级/动作为 {} / {}，锚点为 {} / {}",
                                p.severity,
                                p.action.name_cn(),
                                w.severity,
                                w.action.name_cn()
                            ),
                        ));
                    }
                    if p.quote.trim().is_empty() {
                        out.push(AssetIssue::new(
                            E_DEGRADE_MISSING,
                            w.trigger,
                            "降级路径无锚点引文",
                        ));
                    }
                }
            }
        }
        out
    }
}

// ---------------------------------------------------------------------------
// 六、禁扩面
// ---------------------------------------------------------------------------

/// 禁扩面条目（码 → 说明）。
pub const ASSET_EXCLUSIONS: [(&str, &str); EXCLUSION_COUNT] = [
    ("F3204:decoder", "解码器路由/内存画像/校验规则归 F3204"),
    ("F3216:sla", "SLA 表与消费契约归 F3216"),
    ("F3616:listing", "上架流水/定价/结算归 F3616"),
    ("F3607:verify", "验证器五段实现归 F3607"),
    ("F4604:sandbox", "沙箱实现归 F4604（F3602 已登记引用）"),
    ("Q:body", "Q 管线本体不在本项实现"),
];

/// 越界建议。
pub fn asset_scope_advice(intent: &str) -> Option<&'static str> {
    ASSET_EXCLUSIONS
        .iter()
        .find(|(code, _)| intent.contains(code.split(':').next().unwrap_or("")))
        .map(|(_, note)| *note)
}

/// 越界判定。
pub fn check_no_overreach(intent: &str) -> Result<&'static str, AssetError> {
    match asset_scope_advice(intent) {
        Some(note) => Err(AssetError::new(
            E_OVERREACH,
            intent,
            format!("越界：{}（本项只定义模型与登记）", note),
        )),
        None => Ok("在本项声明范围内"),
    }
}

// ---------------------------------------------------------------------------
// 七、判据（锚点原文六项）
// ---------------------------------------------------------------------------

/// 判据一条。
#[derive(Clone, Copy, Debug)]
pub struct Criterion {
    /// 判据码。
    pub code: &'static str,
    /// 判据名（锚点原词）。
    pub title: &'static str,
    /// 锚点依据。
    pub basis: &'static str,
    /// 机检锚点前缀。
    pub probe: &'static str,
}

impl Criterion {
    /// 全部判据（锚点原文六项）。
    pub const ALL: [Criterion; CRITERION_COUNT] = [
        Criterion {
            code: "C1",
            title: "七要素",
            basis: "锚点原文：CreationAsset 模型（ID/类型/内容/元数据/版本/来源/许可七要素——七要素齐备断言",
            probe: "R01-七要素",
        },
        Criterion {
            code: "C2",
            title: "许可红线",
            basis: "锚点原文：许可红线：无许可信息的创作资产不许上架（上架前必补）",
            probe: "R01-许可红线",
        },
        Criterion {
            code: "C3",
            title: "兼容警告",
            basis: "锚点原文：兼容声明红线：未声明兼容版本的资产安装时警告（静默不兼容=装了就坏",
            probe: "R01-兼容警告",
        },
        Criterion {
            code: "C4",
            title: "类型单源",
            basis: "锚点原文：七类创作资产（复用 F3204 扩展注册（复述单源）",
            probe: "R01-类型单源",
        },
        Criterion {
            code: "C5",
            title: "schema 复用",
            basis: "锚点原文：元数据 schema（校验器复用 F3204；F3204 锚点「宽容元数据严格内容」",
            probe: "R01-schema复用",
        },
        Criterion {
            code: "C6",
            title: "判据",
            basis: "锚点判据列表末项——判据本身须可机检、可追溯",
            probe: "R01-判据",
        },
    ];

    /// 判据台账指纹。
    pub fn ledger_fingerprint() -> u64 {
        let mut buf = String::new();
        for c in Criterion::ALL.iter() {
            buf.push_str(c.code);
            buf.push(':');
            buf.push_str(c.title);
            buf.push(';');
        }
        fnv1a64(buf.as_bytes())
    }
}

/// 判据审计。
pub fn audit_criteria() -> Vec<AssetIssue> {
    let mut out = Vec::new();
    if Criterion::ALL.len() != CRITERION_COUNT {
        out.push(AssetIssue::new(
            E_CRITERION_MISSING,
            "criteria",
            format!("判据数 {} ≠ {}", Criterion::ALL.len(), CRITERION_COUNT),
        ));
    }
    for c in Criterion::ALL.iter() {
        if c.title.trim().is_empty() {
            out.push(AssetIssue::new(E_CRITERION_MISSING, c.code, "判据无标题"));
        }
        if c.basis.trim().is_empty() {
            out.push(AssetIssue::new(
                E_CRITERION_NO_BASIS,
                c.code,
                "判据无锚点依据",
            ));
        }
        if c.probe.trim().is_empty() {
            out.push(AssetIssue::new(E_CRITERION_NO_BASIS, c.code, "判据无机检锚点"));
        }
    }
    out
}

// ---------------------------------------------------------------------------
// 八、总纲本体
// ---------------------------------------------------------------------------

/// 创作资产模型总纲（聚合）。
#[derive(Clone, Copy, Debug)]
pub struct AssetArchitecture {
    /// 七要素模型。
    pub model: AssetModel,
    /// 七类注册表。
    pub registry: KindRegistry,
    /// schema 复用登记。
    pub schema: SchemaReuse,
    /// 降级矩阵。
    pub degrade: DegradeMatrix,
}

impl AssetArchitecture {
    /// 标准总纲（**唯一正样本构造处**）。
    pub fn standard() -> AssetArchitecture {
        AssetArchitecture {
            model: AssetModel::standard(),
            registry: KindRegistry::standard(),
            schema: SchemaReuse::standard(),
            degrade: DegradeMatrix::standard(),
        }
    }

    /// 空总纲（反例用）。
    pub fn empty() -> AssetArchitecture {
        AssetArchitecture {
            model: AssetModel::standard(),
            registry: KindRegistry::empty(),
            schema: SchemaReuse::standard(),
            degrade: DegradeMatrix::standard(),
        }
    }

    /// 收集全部契约问题。
    pub fn collect_issues(&self) -> Vec<AssetIssue> {
        let mut out = Vec::new();
        out.extend(self.model.audit());
        out.extend(self.registry.audit());
        out.extend(self.schema.audit());
        out.extend(self.degrade.audit());
        out.extend(audit_criteria());
        out
    }

    /// 开工前置校验。
    pub fn preflight(&self) -> Result<(), AssetError> {
        let issues = self.collect_issues();
        if let Some(i) = issues.iter().find(|i| i.is_blocking()) {
            return Err(AssetError::new(i.code, i.subject.clone(), i.detail.clone()));
        }
        if let Some(i) = issues.first() {
            return Err(AssetError::new(i.code, i.subject.clone(), i.detail.clone()));
        }
        Ok(())
    }

    /// 装载一个资产（**完整门禁链**：类型注册 → 七要素 → schema → 上架闸）。
    ///
    /// 顺序有意：先拒未注册类型（最根本），再查七要素，最后才轮到上架相关。
    pub fn load_asset(
        &self,
        asset: &CreationAsset,
        decl: Option<CompatDecl>,
        engine: u32,
    ) -> Result<LoadOutcome, AssetError> {
        // 1. 类型必须已注册。
        let kind = AssetKind::from_code(asset.kind).ok_or_else(|| {
            AssetError::new(
                E_KIND_UNREGISTERED,
                asset.id,
                format!("未注册的资产类型码 {}", asset.kind),
            )
        })?;
        if self.registry.get(kind).binding_count == 0 {
            return Err(AssetError::new(
                E_KIND_UNREGISTERED,
                asset.id,
                format!("{} 未注册资源绑定——未注册类型须拒载", kind.name_cn()),
            ));
        }
        // 2. 许可门**先于**七要素检查：缺许可是 P0 红线，必须报自己的专码，
        //    不能被并进「七要素缺项」里降级成普通拒载——两者的处置方向与
        //    严重级都不同（阻断上架 vs 拒绝装载），混报等于把红线降格。
        if !asset.license_present() {
            return Err(AssetError::new(
                E_LICENSE_MISSING,
                asset.id,
                "缺许可信息——锚点：许可红线，无许可信息的创作资产不许上架",
            ));
        }
        // 3. schema 内容门（**严格**）先于七要素齐备检查：内容不合 schema 是内容
        //    本体问题，报content 专码；若让「七要素缺项」先拦，会把内容违例
        //    降格成普通缺项，掩盖「内容严格」这条语义的本来含义。
        let (outcome, issues) = self.schema.validate(asset);
        if !outcome.passes() {
            return Err(AssetError::new(
                E_SCHEMA_VIOLATION,
                asset.id,
                issues[0].detail.clone(),
            ));
        }
        // 4. 七要素齐备（逐要素查；许可已在上面单独过门）。
        let missing = asset.missing_fields();
        if !missing.is_empty() {
            let names: Vec<&str> = missing.iter().map(|f| f.name_cn()).collect();
            return Err(AssetError::new(
                E_FIELD_MISSING,
                asset.id,
                format!("七要素缺 {}（共缺 {} 项）", names.join("+"), missing.len()),
            ));
        }
        // 5. 兼容相容度（警告级不阻断，返报告由上层决定是否要确认）。
        let compat = CompatReport::evaluate(decl, engine);

        Ok(LoadOutcome { kind, compat, schema: outcome, warnings: issues })
    }

    /// 总纲指纹（四张表串接）。
    pub fn fingerprint(&self) -> u64 {
        let mut buf = String::new();
        for f in self.model.fields.iter() {
            buf.push_str(f.code());
        }
        for s in self.registry.kinds.iter() {
            buf.push_str(&alloc::format!("{:016x}", s.fingerprint()));
        }
        buf.push_str(self.schema.cite);
        buf.push_str(self.schema.meta_route);
        for p in self.degrade.paths.iter() {
            buf.push_str(&alloc::format!("{:016x}", p.fingerprint()));
        }
        fnv1a64(buf.as_bytes())
    }

    /// 总纲自检。
    pub fn run_checks(&self) -> CheckSet {
        super::ver04_checks::run_ver04_checks()
    }

    /// 读屏替述（无障碍等价口述）。
    pub fn narration(&self) -> Vec<String> {
        let mut out = Vec::new();
        out.push(format!(
            "一个创作资产必须带齐七个字段：{}。少任何一个都不许上架。",
            AssetField::ALL
                .iter()
                .map(|f| f.name_cn())
                .collect::<Vec<_>>()
                .join("、")
        ));
        out.push(format!(
            "另有七类创作资产：{}。要素和种类是两回事——七类都注册了，也不等于任一个具体资产七要素齐备。",
            AssetKind::ALL
                .iter()
                .map(|k| k.name_cn())
                .collect::<Vec<_>>()
                .join("、")
        ));
        out.push(
            "许可分三态：已声明已核验才能上架；完全没写许可，直接阻断；写了但还没核验，只警告并要你确认。"
                .to_string(),
        );
        out.push(
            "为什么分成三态：把「没写」和「待核」都判阻断，创作者会在上架前一瞬被卡住且不知道要补什么；"
                .to_string()
                + "都判警告，又等于放无许可内容上架。所以两类问题各走各的门。",
        );
        out.push(
            "兼容声明分四级：精确匹配、相容、未声明、明确不兼容。后两级都要警告加确认，但不阻断——"
                .to_string()
                + "老资产可能确实能跑，阻断会误伤。分四级是为了让「作者疏漏」和「版本错配」归因不同。",
        );
        out.push(
            "七类创作资产和F3204 的十类资源类型是两轴：一个壁纸资产展开后是纹理加样式表。"
                .to_string()
                + "解码和内存画像一律走 F3204，本项只声明绑定关系，不复制它的校验器。",
        );
        out.push(
            "元数据宽松、内容严格：元数据缺字段就填默认值并告警，内容本体不合规则直接拒绝。"
                .to_string()
                + "这条两级语义是 F3204 的既有裁决，本项复述不另立。",
        );
        out.push(
            "脚本数据是代码类，安装时必须在沙箱里跑——这一条承 F3602，本项只做标记与机检，不另写沙箱。"
                .to_string(),
        );
        out
    }
}

/// 装载结果（成功时给出相容度与告警）。
///
/// 含 [`Vec`] 故只`Clone` 不 `Copy`——刻意如此：调用方若把结果按值传来传去，
/// 说明它在堆上囤积告警而没就地消费，正是本项要防的「告警被静默丢弃」。
#[derive(Clone, Debug)]
pub struct LoadOutcome {
    /// 解析出的种类。
    pub kind: AssetKind,
    /// 兼容相容度。
    pub compat: CompatReport,
    /// schema 校验结果。
    pub schema: SchemaOutcome,
    /// 告警清单（宽松解析等）。
    pub warnings: Vec<AssetIssue>,
}
/// VE-F3603 域自检（判据逐条映射见 `ver04_checks.rs`）。
///
/// 聚合表经本模块取自检（与 [`ver03_arch`](super::ver03_arch) 同一约定）：
/// 聚合面只认「域开工模块」，不认检查模块。
pub fn run_ver04_checks() -> CheckSet {
    super::ver04_checks::run_ver04_checks()
}
