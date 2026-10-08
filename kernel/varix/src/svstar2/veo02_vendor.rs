//! VE-F2802 · Servo 组件选型与集成边界（VE-O 域 · CSS/HTML 表面域 · O01 组 · 目标 380 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F2802`
//!
//! **判据（锚点原文）**：O01 架构声明、集成边界、解析子集、判据。
//!
//! **职责定位（锚点原文）**：Servo style/style_traits 两 crate 引入；layout 不引入
//! （N 域承担）；集成边界声明。
//!
//! **数据结构（锚点原文·家族格式）**：数据模型与规格表（逐条规格公开、参数域
//! 钳制、枚举守卫）。
//!
//! **错误路径与降级矩阵（锚点原文·家族格式）**：非法输入→校验拒绝三要素；
//! 边界越界→钳制 + 告警；异常检出→立案流转。
//!
//! **性能逐项分解（锚点原文·家族格式）**：核心逻辑 O(1)-O(logN)（实测定标入域
//! 账本）；预算联动次序单源。
//!
//! **跨批对接点（锚点原文·家族格式）**：上游契约接收（哈希对账）；下游消费接口
//! （前向声明）；跨域衔接对账钩子（复用方义务）。
//!
//! **无障碍与隐私（锚点原文）**：文档替述可读；无隐私面。
//!
//! # 一、与 F2801 的分工（不重复施工）
//!
//! F2801 已立**决策**（引入 style / style_traits、不引入 layout）与**边界判定契约**
//! （[`IntegrationBoundary`]）。本单**不重做决策**，只做决策的**落地与看护**：
//!
//! - 本单拥有：vendor 化**清单与体量账**、**MPL-2.0 合规文本链**、**符号导入面**
//!   （哪些 item 从外部 crate 进 O 域的可见集）、**禁扩面**的可执行闸门、
//!   **上游版本钉定**与对账、**越界引用**的检出。
//! - 本单不做：CSS 词法/声明解析（F2804/F2805）、布局（F2602 归 N 域）、
//!   属性全表（F2803）。
//!
//! 决策源在 F2801（[`CRATE_DECISIONS`]），本单以 [`crate_decision`] 只读引用，
//! **不复制一份**——两份决策表迟早分叉，正是"决策单源"要防的事。
//!
//! # 二、为什么 `layout` 不引入是**硬边界**而非偏好
//!
//! 锚点写的是"layout 不引入（N 域承担）"。若只把它当偏好，实现者会在"少写一个
//! 布局引擎"的短期诱惑下引入它。后果不是代码变多，而是**同一棵控件树出现两套
//! 布局结果**：N 域测量出的尺寸与 O 域自算的尺寸不一致，界面表现为"同一页面在两处
//! 对不齐"，且归因极难。
//!
//! 故本单把"不得引用 layout 符号"做成**可执行闸门**（[`ImportGate::check_symbol`]）：
//! 任何 `layout::` 前缀符号进入 O 域可见集即 [`E_LAYOUT_FORBIDDEN`]，不是 lint 提示
//! 而是登记失败。
//!
//! # 三、禁扩面：引入不是「整 crate 抄进来」
//!
//! vendor 化的真实成本是**日后要维护的代码量**。整 crate 引入 style 意味着把
//! 解析器、属性表、`#[cfg(feature)]` 组合一并背走，后续每次 Servo 上游变动都要
//! 重新决策。故本单要求**逐 crate 的符号导入面**（[`CrateSurface`]）：只允许
//! [`ALLOWED_SYMBOLS`] 白名单内的符号进 O 域，其余（含 `layout` 全部）阻断。
//!
//! 白名单**逐条带理由**（[`SymbolGrant::why`]）——无理由的白名单条目等于
//! "以后谁都能加"，那不是闸门是摆设。
//!
//! # 四、MPL-2.0 合规：改了就必须开源，这是引入的**代价**
//!
//! Servo 的 style / style_traits 是 MPL-2.0：**文件级 copyleft**。改了该文件就必须
//! 以 MPL-2.0 发布该文件。这不是形式主义——它是 vendor 化之后**唯一真实的义务**。
//! 故本单要求每个 vendor 化的 crate 登记 [`LicenseObligation`]：许可证标识 + 本仓
//! 改动事实 + 发布路径。缺任一项即 [`E_LICENSE_INCOMPLETE`] 阻断。
//!
//! 特别地「vendor 时顺手改了几行」不构成豁免——MPL-2.0 看的是**有没有改**，
//! 改动行数与是否「顺手」都不改变义务。
//!
//! # 五、版本钉定：pin 的是**上游 revision** 而非 crate 版本号
//!
//! style 是 workspace 内 crate，其版本号在 monorepo 内多次重排，用版本号钉定等于
//! 没钉。故本单钉 **revision 哈希**（[`VendorPin::revision`]）：40 位十六进制，
//! 且**必须与 vendored 源的实际哈希对账**（[`VendorPin::verify`]），对不上即
//! [`E_REVISION_DRIFT`]。这是"哈希对账"在 vendor 场景的具体形态。
//!
//! # 六、零 panic 面
//!
//! 全模块用 `get`/`get_mut` 与显式边界检查，**无 `unwrap()`、无 `expect()`、
//! 无 `panic!`、无裸 `[i]` 索引**（越界即视为账目不一致并阻断，不中止渲染）。
//! 零墙钟、零 IO：版本与哈希均由注入值参与运算，回归可复现。

use alloc::string::{String, ToString};
use alloc::vec::Vec;

use super::veo01_arch::{CrateDecision, ServoCrateId, ServoDecision, StyleError};

use super::veo01_arch::CRATE_DECISIONS;

/// 重导出 crate 句柄常量（下游 `veo02_vendor_checks.rs` 经本模块取用，
/// 避免它直接依赖 `veo01_arch`——那会让本单与F2801 的依赖边界失守）。
pub use super::veo01_arch::{fnv1a64_hex, CRATE_LAYOUT, CRATE_STYLE, CRATE_STYLE_TRAITS};

/// 本项版本（vendor 边界契约的版本号）。
pub const VENDOR_VERSION: &str = "O01-servo-vendor-v1";

/// revision 哈希的十六进制位数（Git 对象名 40 位）。
pub const REVISION_HEX_LEN: usize = 40;

/// 单 crate 允许的符号条目上限（防「白名单写成整 crate」）。
pub const MAX_SYMBOLS_PER_CRATE: usize = 64;

/// vendor 化登记表的 crate 条目上限。
pub const MAX_VENDORED_CRATES: usize = 8;

/// 合规登记的必填字段数（许可证 / 改动事实 / 发布路径）。
pub const LICENSE_FIELD_COUNT: usize = 3;

/// 符号导入面登记上限（跨 crate 合计）。
pub const MAX_SURFACE_ENTRIES: usize = 256;

/// 变更登记（改动审计）的条目上限。
pub const MAX_CHANGE_RECORDS: usize = 64;

/// 单次越界引用的容量上限（越界是异常检出，不是常态）。
pub const MAX_OVERREACH: usize = 32;

/// 错误码：符号不在白名单。
pub const E_SYMBOL_NOT_ALLOWED: &str = "E_SYMBOL_NOT_ALLOWED";

/// 错误码：引用了 layout 符号（硬边界，越界即阻断）。
pub const E_LAYOUT_FORBIDDEN: &str = "E_LAYOUT_FORBIDDEN";

/// 错误码：合规登记不完整。
pub const E_LICENSE_INCOMPLETE: &str = "E_LICENSE_INCOMPLETE";

/// 错误码：上游 revision 漂移（pin 与实际源哈希对不上）。
pub const E_REVISION_DRIFT: &str = "E_REVISION_DRIFT";

/// 错误码：符号面条目超限。
pub const E_SURFACE_CAP: &str = "E_SURFACE_CAP";

/// 错误码：重复登记同一符号。
pub const E_SYMBOL_DUP: &str = "E_SYMBOL_DUP";

/// 错误码：改动记录缺项。
pub const E_CHANGE_INCOMPLETE: &str = "E_CHANGE_INCOMPLETE";

/// 错误码：容量守卫触发。
pub const E_CAP_FULL: &str = "E_CAP_FULL";

/// 上游仓库标识（style / style_traits 同源）。
pub const SERVO_UPSTREAM: &str = "https://github.com/servo/servo";

/// crate 名 → 句柄（只读引用 F2801 的决策表，不复制决策）。
///
/// **未知 crate 返回 `None` 而非默认句柄**：默认句柄会让"拼错的 crate 名"
/// 悄悄落到 `CRATE_UNSPECIFIED` 上，而那条路径的决策是 `Decline`——
/// 一个拼错的请求会被当成"已决策不引入"，静默吞掉。
pub fn crate_id_of(name: &str) -> Option<ServoCrateId> {
    crate_decisions()
        .iter()
        .find(|d| d.name == name)
        .map(|d| d.crate_id)
}

/// F2801 决策表（只读）。
pub fn crate_decisions() -> &'static [CrateDecision] {
    &CRATE_DECISIONS
}

/// 某 crate 的决策（未知 crate 返回 `None`）。
pub fn crate_decision(id: ServoCrateId) -> Option<&'static CrateDecision> {
    CRATE_DECISIONS.iter().find(|d| d.crate_id == id)
}

// ---------------------------------------------------------------------------
// 一、符号导入面（引入不是整 crate 抄进来）
// ---------------------------------------------------------------------------

/// 单个符号的准入授权。
#[derive(Clone, Copy, Debug)]
pub struct SymbolGrant {
    /// 符号名（`crate::path` 的末段或完整路径）。
    pub symbol: &'static str,
    /// 授权理由（**必填**——无理由的白名单等于没有闸门）。
    pub why: &'static str,
}

/// 一个 crate 的符号导入面。
#[derive(Clone, Debug)]
pub struct CrateSurface {
    /// crate 句柄。
    pub crate_id: ServoCrateId,
    /// 允许进O 域可见集的符号。
    pub symbols: Vec<SymbolGrant>,
}

impl CrateSurface {
    /// 空导入面（**默认不放行**：未登记的符号一律阻断）。
    pub fn new(crate_id: ServoCrateId) -> Self {
        CrateSurface {
            crate_id,
            symbols: Vec::new(),
        }
    }

    /// 授权一个符号。
    ///
    /// 拒绝两事：理由为空（授权无据）、重复授权（两份理由说明口径已分叉）。
    pub fn grant(&mut self, g: SymbolGrant) -> Result<(), StyleError> {
        if g.why.trim().is_empty() {
            return Err(StyleError::new(
                E_SYMBOL_NOT_ALLOWED,
                "符号授权被拒：理由为空",
                "白名单条目没有理由，等于任何人都能加——那不是闸门是摆设",
                "写明该符号为何是 O 域必需（例：解析产物的类型载体）",
                "O 域组件负责人",
            ));
        }
        if self.symbols.iter().any(|s| s.symbol == g.symbol) {
            return Err(StyleError::new(
                E_SYMBOL_DUP,
                "符号授权被拒：重复授权",
                "同一符号已有授权条目；两份理由说明口径已分叉",
                "合并为一条授权并写明最终口径",
                "O 域组件负责人",
            ));
        }
        if self.symbols.len() >= MAX_SYMBOLS_PER_CRATE {
            return Err(StyleError::new(
                E_SURFACE_CAP,
                "符号授权被拒：超过单 crate 上限",
                "导入面已满；继续加符号等于把整 crate 抄进来，失去 vendor 化省成本的意义",
                "收窄导入面，或把非必需符号留给后续批次按需引入",
                "O 域组件负责人",
            ));
        }
        self.symbols.push(g);
        Ok(())
    }

    /// 某符号是否已授权。
    pub fn allows(&self, symbol: &str) -> bool {
        self.symbols.iter().any(|s| s.symbol == symbol)
    }

    /// 符号数。
    pub fn len(&self) -> usize {
        self.symbols.len()
    }

    /// 是否为空。
    pub fn is_empty(&self) -> bool {
        self.symbols.is_empty()
    }

    /// 只读遍历。
    pub fn iter(&self) -> impl Iterator<Item = &SymbolGrant> {
        self.symbols.iter()
    }
}

/// style crate 的**准入符号白名单**（最小集）。
///
/// 挑选口径：只放「O 域必须直接触摸的类型载体」。解析行为本身归 F2804/F2805，
/// 不在此列——放进来等于让 O 域绕过解析子集直接调上游实现。
pub const ALLOWED_STYLE_SYMBOLS: [SymbolGrant; 6] = [
    SymbolGrant {
        symbol: "StyleSheet",
        why: "样式表产物容器：O 域对外的唯一声明入口类型",
    },
    SymbolGrant {
        symbol: "Declaration",
        why: "单条声明载体：属性名 + 值 + important 位的最小结构",
    },
    SymbolGrant {
        symbol: "PropertyId",
        why: "属性标识：F2805 注册表的主键，须与上游枚举对齐",
    },
    SymbolGrant {
        symbol: "Value",
        why: "计算值载体：投影到 N/D 域时的取值类型",
    },
    SymbolGrant {
        symbol: "ComputedStyle",
        why: "计算样式：级联收敛后的属性快照容器",
    },
    SymbolGrant {
        symbol: "SpecifiedStyle",
        why: "指定值样式：级联前的逐属性累积结果",
    },
];

/// style_traits crate 的**准入符号白名单**（最小集）。
pub const ALLOWED_STYLE_TRAITS_SYMBOLS: [SymbolGrant; 4] = [
    SymbolGrant {
        symbol: "PropertySet",
        why: "属性元数据集：继承性/初值的批量查询面",
    },
    SymbolGrant {
        symbol: "PropertyIdSet",
        why: "属性集合：子集门禁与批量初始化的集合类型",
    },
    SymbolGrant {
        symbol: "PerPropertyGeneric",
        why: "属性元数据载体：每属性的元数据类型参数",
    },
    SymbolGrant {
        symbol: "ComputedValue",
        why: "计算值 trait：属性的取值约束面",
    },
];

/// 禁扩面关键词（出现在任何 O 域可见符号里即阻断）。
///
/// `layout` 单独成条是因为它是**归属裁决**（N 域承担），不是「本期不做」——
/// 实现者最可能的越界就是把布局顺手做了。
pub const FORBIDDEN_PREFIXES: [&str; 3] = ["layout::", "layout::thread::", "webrender::"];

/// 引用面闸门：判断某符号能否进 O 域可见集。
#[derive(Clone, Debug)]
pub struct ImportGate {
    /// crate 句柄 → 导入面。
    surfaces: Vec<CrateSurface>,
}

impl ImportGate {
    /// 空闸门（**未登记即阻断**——默认拒绝，不是默认放行）。
    pub fn new() -> Self {
        ImportGate { surfaces: Vec::new() }
    }

    /// 登记一个 crate 的导入面。
    pub fn register(&mut self, s: CrateSurface) -> Result<(), StyleError> {
        if self.surfaces.iter().any(|x| x.crate_id == s.crate_id) {
            return Err(StyleError::new(
                E_SYMBOL_DUP,
                "导入面登记被拒：crate 重复",
                "同一 crate 已有导入面；两份导入面意味着两套可见集",
                "合并到既有导入面并显式增补符号",
                "O 域组件负责人",
            ));
        }
        if crate_decision(s.crate_id).is_none() {
            return Err(StyleError::new(
                E_SYMBOL_NOT_ALLOWED,
                "导入面登记被拒：未知 crate",
                "该 crate 未在 F2801 决策表里；未经决策就登记导入面等于绕过选型",
                "先在 F2801 决策表补条目（引入/不引入 + 理由），再登记导入面",
                "O 域组件负责人",
            ));
        }
        if self.total() + s.len() > MAX_SURFACE_ENTRIES {
            return Err(StyleError::new(
                E_SURFACE_CAP,
                "导入面登记被拒：超过全局上限",
                "符号面总量已满；这通常意味着在把整 crate 抄进来",
                "收窄到O 域直接必需的类型，其余留后续批次",
                "O 域组件负责人",
            ));
        }
        self.surfaces.push(s);
        Ok(())
    }

    /// 全部符号条目数。
    pub fn total(&self) -> usize {
        self.surfaces.iter().map(|s| s.len()).sum()
    }

    /// 某 crate 的导入面。
    pub fn surface(&self, crate_id: ServoCrateId) -> Option<&CrateSurface> {
        self.surfaces.iter().find(|s| s.crate_id == crate_id)
    }

    /// 某 crate 的导入面（可变）。
    pub fn surface_mut(&mut self, crate_id: ServoCrateId) -> Option<&mut CrateSurface> {
        self.surfaces.iter_mut().find(|s| s.crate_id == crate_id)
    }

    /// 符号准入判定（复杂度 O(符号面条目数)，实测定标入域账本）。
    ///
    /// 顺序有意固定：**先查禁扩面再查白名单**。若先查白名单，
    /// 一个未授权的 `layout::foo` 会以「不在白名单」被拒，错误码就丢了
    /// 「越界」这层语义——而越界是需要立案的严重级别，不是普通未授权。
    pub fn check_symbol(&self, crate_id: ServoCrateId, symbol: &str) -> Result<(), StyleError> {
        for bad in FORBIDDEN_PREFIXES.iter() {
            if symbol.starts_with(bad) {
                return Err(StyleError::new(
                    E_LAYOUT_FORBIDDEN,
                    "符号引用被拒：命中禁扩面",
                    &format!(
                        "{} 属禁扩面；布局归 N 域，双布局并存会导致同树异结果且极难归因",
                        symbol
                    ),
                    "改为消费 N 域的测量结果；确需布局能力时走 N 域契约申请",
                    "O 域组件负责人",
                ));
            }
        }
        let surface = match self.surface(crate_id) {
            Some(s) => s,
            None => {
                return Err(StyleError::new(
                    E_SYMBOL_NOT_ALLOWED,
                    "符号引用被拒：crate 无导入面",
                    &format!("{} 未登记导入面；未登记的 crate 默认不可见", symbol),
                    "先登记该 crate 的导入面并逐符号给出理由",
                    "O 域组件负责人",
                ))
            }
        };
        if !surface.allows(symbol) {
            return Err(StyleError::new(
                E_SYMBOL_NOT_ALLOWED,
                "符号引用被拒：不在白名单",
                &format!(
                    "{} 未列入 {} 的准入白名单；准入面收窄是 vendor 化省成本的前提",
                    symbol,
                    crate_decision(crate_id).map(|d| d.name).unwrap_or("未知 crate")
                ),
                "确有必需时补一条带理由的授权，否则改用 O 域自有封装",
                "O 域组件负责人",
            ));
        }
        Ok(())
    }

    /// 只读遍历。
    pub fn iter(&self) -> impl Iterator<Item = &CrateSurface> {
        self.surfaces.iter()
    }
}

// ---------------------------------------------------------------------------
// 二、MPL-2.0 合规（vendor 的真实代价）
// ---------------------------------------------------------------------------

/// vendor 化后的合规义务登记。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LicenseObligation {
    /// 未改动上游文件：可随本仓闭源许可分发（引用不传染）。
    ModifiedUpstream,
    /// 改动了上游文件：该文件须以 MPL-2.0 发布。
    MustPublishModified,
}

impl LicenseObligation {
    /// 中文名。
    pub fn zh(self) -> &'static str {
        match self {
            LicenseObligation::ModifiedUpstream => "未改动上游（可随本仓许可分发）",
            LicenseObligation::MustPublishModified => "已改动上游（该文件须以 MPL-2.0 发布）",
        }
    }

    /// 枚举往返守卫：未知码拒绝。
    pub fn from_code(code: &str) -> Option<LicenseObligation> {
        match code {
            "MODIFIED_UPSTREAM" => Some(LicenseObligation::ModifiedUpstream),
            "MUST_PUBLISH_MODIFIED" => Some(LicenseObligation::MustPublishModified),
            _ => None,
        }
    }
}

/// 单 crate 的 vendor 登记（合规 + 版本 + 源哈希三合一）。
#[derive(Clone, Debug)]
pub struct VendorRecord {
    /// crate 句柄。
    pub crate_id: ServoCrateId,
    /// 许可证标识（须逐字登记，"都是 MPL" 不是登记）。
    pub license: String,
    /// 是否有本仓改动。
    pub obligation: LicenseObligation,
    /// 改动事实说明（**有改动时必填**——MPL-2.0 看有没有改，不看改了几行）。
    pub change_note: String,
    /// 发布路径（有改动时必填——MPL-2.0 的实际义务是「可得」，不是「声明」）。
    pub publish_path: String,
    /// 导入面是否已登记（合规与准入两件事都要过）。
    pub surface_registered: bool,
}

impl VendorRecord {
    /// 未改动上游的登记。
    pub fn pristine(crate_id: ServoCrateId, license: &str) -> Self {
        VendorRecord {
            crate_id,
            license: license.to_string(),
            obligation: LicenseObligation::ModifiedUpstream,
            change_note: String::new(),
            publish_path: String::new(),
            surface_registered: false,
        }
    }

    /// 本仓有改动的登记。
    pub fn modified(
        crate_id: ServoCrateId,
        license: &str,
        change_note: &str,
        publish_path: &str,
    ) -> Self {
        VendorRecord {
            crate_id,
            license: license.to_string(),
            obligation: LicenseObligation::MustPublishModified,
            change_note: change_note.to_string(),
            publish_path: publish_path.to_string(),
            surface_registered: false,
        }
    }

    /// 合规字段齐整性（许可证 + 改动事实 + 发布路径三件）。
    pub fn compliance_complete(&self) -> bool {
        if self.license.trim().is_empty() {
            return false;
        }
        match self.obligation {
            LicenseObligation::ModifiedUpstream => true,
            LicenseObligation::MustPublishModified => {
                !self.change_note.trim().is_empty() && !self.publish_path.trim().is_empty()
            }
        }
    }

    /// 读屏单行。
    pub fn screen_line(&self) -> String {
        format!(
            "crate {} 许可证 {}，{}，导入面{}",
            crate_decision(self.crate_id).map(|d| d.name).unwrap_or("未知"),
            self.license,
            self.obligation.zh(),
            if self.surface_registered { "已登记" } else { "未登记" }
        )
    }
}

/// vendor 化登记表。
#[derive(Clone, Debug)]
pub struct VendorRegistry {
    /// 登记条目。
    pub records: Vec<VendorRecord>,
}

impl VendorRegistry {
    /// 空登记表。
    pub fn new() -> Self {
        VendorRegistry { records: Vec::new() }
    }

    /// 登记一个 crate 的 vendor 条目。
    pub fn register(&mut self, r: VendorRecord) -> Result<(), StyleError> {
        if self.records.iter().any(|x| x.crate_id == r.crate_id) {
            return Err(StyleError::new(
                E_SYMBOL_DUP,
                "vendor 登记被拒：crate 重复",
                "同一 crate 已有 vendor 条目；两份条目意味着两份合规状态",
                "更新既有条目而非并行登记",
                "O 域组件负责人",
            ));
        }
        if self.records.len() >= MAX_VENDORED_CRATES {
            return Err(StyleError::new(
                E_CAP_FULL,
                "vendor 登记被拒：超过条目上限",
                "登记条目已满；style 域只应 vendor 两crate",
                "核对是否把不该引入的 crate 也登记了（layout 应不引入）",
                "O 域组件负责人",
            ));
        }
        if !r.compliance_complete() {
            return Err(StyleError::new(
                E_LICENSE_INCOMPLETE,
                "vendor 登记被拒：合规字段不完整",
                "MPL-2.0 是文件级copyleft；有改动就必须给出改动事实与发布路径",
                "补齐许可证标识、改动事实、发布路径三项后重新登记",
                "O 域组件负责人",
            ));
        }
        self.records.push(r);
        Ok(())
    }

    /// 某 crate 的条目。
    pub fn record(&self, crate_id: ServoCrateId) -> Option<&VendorRecord> {
        self.records.iter().find(|r| r.crate_id == crate_id)
    }

    /// 只读遍历。
    pub fn iter(&self) -> impl Iterator<Item = &VendorRecord> {
        self.records.iter()
    }

    /// 条目数。
    pub fn len(&self) -> usize {
        self.records.len()
    }

    /// 是否为空。
    pub fn is_empty(&self) -> bool {
        self.records.is_empty()
    }

    /// 与 F2801 决策的一致性核验（**引入决策与 vendor 登记必须一一对应**）。
    ///
    /// 两个方向都要查：决策引入却没登记（漏做），登记了却决策不引入（越界）。
    /// 只查前一个方向会漏掉「把 layout vendor 进来」这个最严重的错。
    pub fn reconcile_with_decisions(&self) -> Result<(), StyleError> {
        for d in crate_decisions() {
            let registered = self.record(d.crate_id).is_some();
            match d.decision {
                ServoDecision::Vendorize => {
                    if !registered {
                        return Err(StyleError::new(
                            E_LICENSE_INCOMPLETE,
                            "决策与登记不一致：决策引入但未登记",
                            &format!("{} 已决策引入，vendor 表里却没有条目", d.name),
                            "补齐 vendor 登记（许可证 + 导入面）",
                            "O 域组件负责人",
                        ));
                    }
                }
                ServoDecision::Decline => {
                    if registered {
                        return Err(StyleError::new(
                            E_LAYOUT_FORBIDDEN,
                            "决策与登记不一致：决策不引入却已登记",
                            &format!(
                                "{} 已决策不引入（归 {}），vendor 表却出现条目",
                                d.name, d.owner_elsewhere
                            ),
                            "撤销该 vendor 登记，改走归属域契约",
                            "O 域组件负责人",
                        ));
                    }
                }
            }
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// 三、版本钉定与哈希对账
// ---------------------------------------------------------------------------

/// 上游版本钉定（钉 revision 哈希，不钉 crate 版本号）。
#[derive(Clone, Copy, Debug)]
pub struct VendorPin {
    /// crate 句柄。
    pub crate_id: ServoCrateId,
    /// 上游 revision（40 位十六进制）。
    pub revision: &'static str,
    /// vendored 源的实际内容哈希（十六进制）。
    pub source_hash: &'static str,
}

impl VendorPin {
    /// 构造并做**形状**校验（revision 位数）。
    pub fn new(
        crate_id: ServoCrateId,
        revision: &'static str,
        source_hash: &'static str,
    ) -> Result<Self, StyleError> {
        if revision.len() != REVISION_HEX_LEN {
            return Err(StyleError::new(
                E_REVISION_DRIFT,
                "版本钉定被拒：revision 形状不对",
                &format!(
                    "期望 {} 位十六进制（Git 对象名），实得 {} 位",
                    REVISION_HEX_LEN,
                    revision.len()
                ),
                "填入完整的上游 commit 哈希",
                "O 域组件负责人",
            ));
        }
        Ok(VendorPin {
            crate_id,
            revision,
            source_hash,
        })
    }

    /// revision 是否为合法十六进制。
    pub fn revision_is_hex(&self) -> bool {
        self.revision
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    }

    /// 对账：钉定的源哈希是否等于实际 vendored 源的哈希。
    ///
    /// 这是"哈希对账"在 vendor 场景的形态——不是看一眼填了没有，
    /// 是**重算一遍**。传入实际重算值。
    pub fn verify(&self, actual_source_hash: &str) -> Result<(), StyleError> {
        if actual_source_hash != self.source_hash {
            return Err(StyleError::new(
                E_REVISION_DRIFT,
                "版本对账失败：源已漂移",
                &format!(
                    "钉定哈希 {} 与实际 vendored 源 {} 不一致；pin 形同虚设",
                    self.source_hash, actual_source_hash
                ),
                "重新 vendor 到钉定 revision，或改 pin 到当前实际 revision（走变更纪律）",
                "O 域组件负责人",
            ));
        }
        Ok(())
    }
}

/// 改动审计记录（vendor 后每次本地改动都要留痕）。
#[derive(Clone, Copy, Debug)]
pub struct ChangeRecord {
    /// crate 句柄。
    pub crate_id: ServoCrateId,
    /// 改动的文件（相对 vendor 树）。
    pub file: &'static str,
    /// 改动摘要（**必填**——"优化"不是摘要）。
    pub summary: &'static str,
}

/// 改动审计台账。
#[derive(Clone, Debug)]
pub struct ChangeLedger {
    /// 记录条目。
    pub records: Vec<ChangeRecord>,
}

impl ChangeLedger {
    /// 空台账。
    pub fn new() -> Self {
        ChangeLedger { records: Vec::new() }
    }

    /// 记一条改动。
    pub fn record(&mut self, r: ChangeRecord) -> Result<(), StyleError> {
        if r.summary.trim().is_empty() {
            return Err(StyleError::new(
                E_CHANGE_INCOMPLETE,
                "改动登记被拒：摘要为空",
                "「优化」「修复」这类空摘要让合规审计无法复核",
                "写明改了什么、为什么改",
                "O 域组件负责人",
            ));
        }
        if r.file.trim().is_empty() {
            return Err(StyleError::new(
                E_CHANGE_INCOMPLETE,
                "改动登记被拒：文件名为空",
                "没有文件名的改动无法定位到 copyleft 义务的具体范围",
                "填入 vendor 树内的相对路径",
                "O 域组件负责人",
            ));
        }
        if self.records.len() >= MAX_CHANGE_RECORDS {
            return Err(StyleError::new(
                E_CAP_FULL,
                "改动登记被拒：超过条目上限",
                "台账已满；持续改动而无审计会让 MPL-2.0 义务失据",
                "先归档旧记录，并核对是否存在本可避免的改动",
                "O 域组件负责人",
            ));
        }
        self.records.push(r);
        Ok(())
    }

    /// 某 crate 的改动条数。
    pub fn count_of(&self, crate_id: ServoCrateId) -> usize {
        self.records.iter().filter(|r| r.crate_id == crate_id).count()
    }

    /// 条目数。
    pub fn len(&self) -> usize {
        self.records.len()
    }

    /// 是否为空。
    pub fn is_empty(&self) -> bool {
        self.records.is_empty()
    }
}

// ---------------------------------------------------------------------------
// 四、越界检出（异常检出→立案流转）
// ---------------------------------------------------------------------------

/// 越界引用立案（一条=一次检出）。
#[derive(Clone, Copy, Debug)]
pub struct OverreachCase {
    /// 符号名。
    pub symbol: &'static str,
    /// 检出理由码。
    pub code: &'static str,
}

/// 越界检出台账。
#[derive(Clone, Debug)]
pub struct OverreachLedger {
    /// 立案条目。
    pub cases: Vec<OverreachCase>,
}

impl OverreachLedger {
    /// 空台账。
    pub fn new() -> Self {
        OverreachLedger { cases: Vec::new() }
    }

    /// 立案（同一符号重复检出只记一次——重复不增加信息量）。
    pub fn file(&mut self, c: OverreachCase) -> Result<(), StyleError> {
        if self.cases.iter().any(|x| x.symbol == c.symbol) {
            return Ok(());
        }
        if self.cases.len() >= MAX_OVERREACH {
            return Err(StyleError::new(
                E_CAP_FULL,
                "越界立案被拒：超过条目上限",
                "越界条目堆积说明导入面整体失控，不是零星越界",
                "收窄导入面到真正必需的符号集，再重新检出",
                "O 域组件负责人",
            ));
        }
        self.cases.push(c);
        Ok(())
    }

    /// 条目数。
    pub fn len(&self) -> usize {
        self.cases.len()
    }

    /// 是否为空。
    pub fn is_empty(&self) -> bool {
        self.cases.is_empty()
    }

    /// 只读遍历。
    pub fn iter(&self) -> impl Iterator<Item = &OverreachCase> {
        self.cases.iter()
    }
}

// ---------------------------------------------------------------------------
// 五、边界总账（一个结构回答「引了什么/没引什么/凭什么」）
// ---------------------------------------------------------------------------

/// Servo 集成边界总账。
#[derive(Clone, Debug)]
pub struct ServoBoundary {
    /// 导入面闸门。
    pub gate: ImportGate,
    /// vendor 登记。
    pub registry: VendorRegistry,
    /// 改动审计。
    pub changes: ChangeLedger,
    /// 越界立案。
    pub overreach: OverreachLedger,
}

impl ServoBoundary {
    /// 空边界（**未配置即阻断**：闸门空表 ⇒ 无任何 crate 可见）。
    pub fn new() -> Self {
        ServoBoundary {
            gate: ImportGate::new(),
            registry: VendorRegistry::new(),
            changes: ChangeLedger::new(),
            overreach: OverreachLedger::new(),
        }
    }

    /// 标准边界：按 F2801 决策装配（style / style_traits 引入 + 白名单，
    /// layout 不引入且**不登记**）。
    pub fn standard() -> Self {
        let mut b = ServoBoundary::new();
        let mut style_surface = CrateSurface::new(CRATE_STYLE);
        for g in ALLOWED_STYLE_SYMBOLS.iter() {
            let _ = style_surface.grant(*g);
        }
        let _ = b.gate.register(style_surface);
        let mut traits_surface = CrateSurface::new(CRATE_STYLE_TRAITS);
        for g in ALLOWED_STYLE_TRAITS_SYMBOLS.iter() {
            let _ = traits_surface.grant(*g);
        }
        let _ = b.gate.register(traits_surface);
        let _ = b.registry.register(VendorRecord::pristine(CRATE_STYLE, "MPL-2.0"));
        let _ = b
            .registry
            .register(VendorRecord::pristine(CRATE_STYLE_TRAITS, "MPL-2.0"));
        b.mark_surfaces_registered();
        b
    }

    /// 把已登记的 vendor 条目标为「导入面已登记」。
    fn mark_surfaces_registered(&mut self) {
        for r in self.registry.records.iter_mut() {
            if self.gate.surface(r.crate_id).is_some() {
                r.surface_registered = true;
            }
        }
    }

    /// 准入一个符号（越界自动立案）。
    ///
    /// 返回 `Ok(())` 或`Err`；**越界同时进台账**——拒绝只是当场挡住，
    /// 立案才让「有多少人试过」可被统计。
    pub fn admit(&mut self, crate_id: ServoCrateId, symbol: &'static str) -> Result<(), StyleError> {
        match self.gate.check_symbol(crate_id, symbol) {
            Ok(()) => Ok(()),
            Err(e) => {
                let _ = self.overreach.file(OverreachCase {
                    symbol,
                    code: e.code,
                });
                Err(e)
            }
        }
    }

    /// 总账自检（**结构一致性**；任一不成立即阻断）。
    ///
    /// 四条：决策与登记一致、无 layout 符号入面、合规字段齐整、
    /// 导入面与 vendor 登记交叉齐整。
    pub fn audit(&self) -> Result<(), StyleError> {
        self.registry.reconcile_with_decisions()?;
        for s in self.gate.iter() {
            for g in s.iter() {
                for bad in FORBIDDEN_PREFIXES.iter() {
                    if g.symbol.starts_with(bad) {
                        return Err(StyleError::new(
                            E_LAYOUT_FORBIDDEN,
                            "边界审计失败：导入面含禁扩符号",
                            &format!("{} 的导入面里有 {}；禁扩面被从后门放进来", s.crate_id, g.symbol),
                            "移除该授权；布局能力走 N 域契约",
                            "O 域组件负责人",
                        ));
                    }
                }
            }
        }
        for r in self.registry.iter() {
            if !r.compliance_complete() {
                return Err(StyleError::new(
                    E_LICENSE_INCOMPLETE,
                    "边界审计失败：合规字段不完整",
                    &format!("{}：{}", r.crate_id, r.screen_line()),
                    "补齐许可证标识、改动事实、发布路径",
                    "O 域组件负责人",
                ));
            }
            if self.gate.surface(r.crate_id).is_none() {
                return Err(StyleError::new(
                    E_LICENSE_INCOMPLETE,
                    "边界审计失败：vendor 已登记但导入面缺",
                    "vendor 落地却没有准入面，等于没有闸门",
                    "登记该 crate 的导入面",
                    "O 域组件负责人",
                ));
            }
        }
        Ok(())
    }

    /// 读屏摘要（无障碍：文档替述可读）。
    pub fn screen_summary(&self) -> String {
        let mut s = String::new();
        s.push_str("Servo 集成边界总账：");
        for d in crate_decisions() {
            s.push_str(&format!(
                "{}决策{}；",
                d.name,
                d.decision.zh()
            ));
        }
        s.push_str(&format!(
            "导入面共{} 个符号，vendor登记{} 项，越界立案{} 条。",
            self.gate.total(),
            self.registry.len(),
            self.overreach.len()
        ));
        s
    }
}
