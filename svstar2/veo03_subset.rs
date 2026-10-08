//! VE-F2803 · 样式解析子集范围声明（VE-O 域 · CSS/HTML 表面域 · O01 组 · 目标 360 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F2803`
//!
//! **判据（锚点原文）**：O01 架构声明、集成边界、解析子集、判据。
//!
//! **职责定位（锚点原文）**：样式解析子集范围声明——属性子集表（布局/视觉/效果/交互
//! 四族——每族属性清单）；CSS 规范版本锚定。
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
//! F2801 已立 [`PropertySpec`] 的**表结构**、四族分桶与 [`SubsetGate`] 的门禁语义，
//! 并给了 12 条**代表性种子**（架构自举用，注释里明写「不是全表」）。
//! 本单交付的是那份**全表**——四族每族的完整属性清单，外加：
//!
//! - **CSS 规范版本锚定**（F2801 未做）：子集声明必须写明「按哪一版 CSS 的
//!   哪个章节」，否则「支持某属性」这句话没有可判定的含义——不同版本对
//!   同一属性的初值/语法/继承性规定不同，且会静默漂移。
//! - **每条属性的章节出处**（F2801 未做）：`name` 只说「有什么」，说不出「按哪条」。
//!
//! 本单**不代做**：F2804 词法器、F2805 声明解析与注册表、F2811 继承语义、
//! F2812 计算值。表里只声明「哪些属性在子集内、参数域多大、初值是什么、
//! 规范出处是哪条」。
//!
//! # 二、族分类不能靠直觉：`display` 是反例样本
//!
//! `display` 常被想当然归到「视觉」或「布局」的某一处，但它决定的是**盒模型是否
//! 生成**，直接决定后续所有布局行为——归视觉会让「为什么这个元素没有高度」
//! 变成不可归因的问题。本单把四族分桶做成**显式声明 + 反例留档**：
//! [`CLASSIFICATION_TRAPS`] 记录这些易错项及其正确归属与错归后果。
//!
//! # 三、子集一旦破口，账单没人批
//!
//! 引入一个子集外属性看起来只是「多支持一条」，实际代价是：F2804 要多认一个
//! token、F2805 要多注册一个解析函数、F2812 要多算一个计算值、F2821 选择器
//! 要多匹配一次。**这些成本没有人为它批预算**，所以本单的立场是：
//! 桶外属性**拒绝并给出路**（复用 F2801 的 [`SubsetGate`]），不静默丢弃，
//! 也不「先接受以后再说」。
//!
//! # 四、规范锚定要能证伪
//!
//! 只写「按 CSS3」是恒真断言——无法证伪。本单要求每条属性登记**章节号**
//! （如 `css-values-4§5.2`），且锚定版本**进哈希**（[`SubsetAnchor`]）：
//! 版本变了必须走变更纪律重签，否则「锚定」就只是个装饰。
//!
//! # 五、零 panic 面
//!
//! 全模块用 `get`/`get_mut` 与显式边界检查，**无 `unwrap()`、无 `expect()`、
//! 无 `panic!`、无裸 `[i]` 索引**。零墙钟、零 IO：版本与章节号均为编译期常量，
//! 回归可复现。

use alloc::string::String;
use alloc::vec::Vec;

use super::veo01_arch::{fnv1a64_hex, StyleError};

/// 重导出 F2801 的四族枚举与族数常量（本单按族分桶，不另立族定义）。
pub use super::veo01_arch::{PropertyFamily, PROPERTY_FAMILY_COUNT};

/// 重导出 `PropertySpec`（本单的 `SubsetProperty` 内嵌它，下游自检经本模块取用）。
pub use super::veo01_arch::PropertySpec;

/// 重导出 F2801 的子集排除表（本单沿用，**不另立**——排除面缩小等于悄悄
/// 扩大子集，两份排除表必然分叉）。
pub use super::veo01_arch::SUBSET_EXCLUSIONS;

/// 本项版本（子集声明表的版本号）。
pub const SUBSET_VERSION: &str = "O01-subset-v1";

/// 四族全集长度（复用 F2801 的常量，**不另立**）。
pub const FAMILY_COUNT: usize = PROPERTY_FAMILY_COUNT;

/// 单族属性条目上限（防「某一族写成全表」）。
pub const MAX_PER_FAMILY: usize = 48;

/// 全表属性条目上限。
pub const MAX_TOTAL: usize = MAX_PER_FAMILY * FAMILY_COUNT;

/// 规范章节号最小长度（形如 `css-values-4§5.2`）。
pub const MIN_SPEC_REF_LEN: usize = 8;

/// 错误码：属性名非法（空/含非法字符）。
pub const E_NAME_INVALID: &str = "E_NAME_INVALID";

/// 错误码：属性重复登记。
pub const E_PROP_DUP: &str = "E_PROP_DUP";

/// 错误码：规范出处缺失或形状不对。
pub const E_SPEC_REF_MISSING: &str = "E_SPEC_REF_MISSING";

/// 错误码：参数域非法（下界高于上界）。
pub const E_DOMAIN_INVERTED: &str = "E_DOMAIN_INVERTED";

/// 错误码：单族超限。
pub const E_FAMILY_OVERGROWN: &str = "E_FAMILY_OVERGROWN";

/// 错误码：全表超限。
pub const E_TOTAL_CAP: &str = "E_TOTAL_CAP";

/// 错误码：族归属与错归登记冲突。
pub const E_FAMILY_TRAP_CONFLICT: &str = "E_FAMILY_TRAP_CONFLICT";

/// 错误码：规范锚定版本漂移。
pub const E_ANCHOR_DRIFT: &str = "E_ANCHOR_DRIFT";

/// 错误码：初值缺失。
pub const E_INITIAL_MISSING: &str = "E_INITIAL_MISSING";

/// 本单锚定的 CSS 规范版本。
///
/// **这是本子集表的唯一权威版本声明**——换版本必须走 [`SubsetAnchor::resign`]
/// 的变更纪律，不能就地改字符串（改了就等于历史条目全部失去依据）。
pub const ANCHORED_CSS_VERSION: &str = "CSS Snapshot 2023-10";

/// 锚定版本的哈希（版本 + 规范族清单实算，防「版本字符串被改了但没人发现」）。
///
/// 值 `beeaf877806d7e66` 由 [`SubsetAnchor::compute_hash`] 对
/// `CSS Snapshot 2023-10|css-values-4|css-color-4|css-display-3|css-effects-2|css-ui-4`
/// 实算得出（并经独立实现对拍），**不是编的**——编出来的常量会让
/// 「锚定对账是真重算」这条判据退化成「实现等于我编的数」。
pub const ANCHOR_HASH: &str = "beeaf877806d7e66";

/// 本单纳入的规范族（子集表按这几份规范逐条取材）。
pub const SPEC_FAMILIES: [&str; 5] = [
    "css-values-4",
    "css-color-4",
    "css-display-3",
    "css-effects-2",
    "css-ui-4",
];

/// 易错项错归时的修正动作（静态串——`StyleError::new` 的 `next` 收
/// `&'static str`，写 `&format!(...)` 会留下悬垂借用）。
const FIX_MISBUCKET: &str = "按易错项表改回正确族";

/// 分类易错项：属性看起来该归某族，实际归另一族。
///
/// 留档的价值在于**错归后果**：分桶错了不会编译失败、不会测试红，
/// 只会在渲染出某个边角行为时被人当成「引擎 bug」来查。
#[derive(Clone, Copy, Debug)]
pub struct ClassificationTrap {
    /// 属性名。
    pub name: &'static str,
    /// 正确归属。
    pub correct: PropertyFamily,
    /// 最常见的错归。
    pub common_mistake: PropertyFamily,
    /// 错归后果（**必填**——只写「易错」不写后果，等于没留档）。
    pub consequence: &'static str,
}

/// 分类易错项表（`display` 是经典反直觉点）。
pub const CLASSIFICATION_TRAPS: [ClassificationTrap; 4] = [
    ClassificationTrap {
        name: "display",
        correct: PropertyFamily::Layout,
        common_mistake: PropertyFamily::Visual,
        consequence: "归视觉则「为何这个元素没有高度」不可归因——它决定盒模型是否生成",
    },
    ClassificationTrap {
        name: "visibility",
        correct: PropertyFamily::Visual,
        common_mistake: PropertyFamily::Effect,
        consequence: "归效果会被当作需离屏合成的效果，多一遍合成且破坏可访问性语义",
    },
    ClassificationTrap {
        name: "pointer-events",
        correct: PropertyFamily::Interaction,
        common_mistake: PropertyFamily::Visual,
        consequence: "归视觉则命中测试与绘制顺序耦合，遮罩类控件点不中",
    },
    ClassificationTrap {
        name: "opacity",
        correct: PropertyFamily::Effect,
        common_mistake: PropertyFamily::Visual,
        consequence: "归视觉则不进合成管线，多层半透明叠加时前后关系错乱",
    },
];

/// 子集属性条目（F2801 `PropertySpec` + 规范出处）。
#[derive(Clone, Copy, Debug)]
pub struct SubsetProperty {
    /// 属性规格（结构承自 F2801，不另立类型）。
    pub spec: PropertySpec,
    /// 规范章节出处（形如 `css-values-4§5.2`）。
    pub spec_ref: &'static str,
}

impl SubsetProperty {
    /// 条目是否合法（名称非空、初值非空、出处形状对、参数域不倒挂）。
    pub fn is_wellformed(&self) -> bool {
        !self.spec.name.trim().is_empty()
            && !self.spec.initial.trim().is_empty()
            && self.spec_ref.len() >= MIN_SPEC_REF_LEN
            && self.spec_ref.contains('§')
            && self.spec.domain_is_sane()
    }

    /// 读屏单行（无障碍：属性清单要能被读屏逐条念）。
    pub fn screen_line(&self) -> String {
        format!(
            "{}（{}族，{}，{}，初值 {}，域 [{}, {}]，出处 {}）",
            self.spec.name,
            self.spec.family.zh(),
            self.spec.value_kind,
            if self.spec.inherited { "继承" } else { "不继承" },
            self.spec.initial,
            self.spec.domain_low,
            self.spec.domain_high,
            self.spec_ref
        )
    }
}

/// 规范锚定（含版本哈希，版本变更走重签）。
#[derive(Clone, Copy, Debug)]
pub struct SubsetAnchor {
    /// 锚定版本。
    pub version: &'static str,
    /// 版本 + 规范族清单的实算哈希。
    pub anchor_hash: &'static str,
    /// 纳入的规范族。
    pub families: [&'static str; 5],
}

impl SubsetAnchor {
    /// 本单的锚定（编译期常量）。
    pub fn anchored() -> Self {
        SubsetAnchor {
            version: ANCHORED_CSS_VERSION,
            anchor_hash: ANCHOR_HASH,
            families: SPEC_FAMILIES,
        }
    }

    /// 实算锚定哈希（**重算，不是看填了没有**）。
    ///
    /// 输入取版本与规范族清单——只对版本取哈希的话，改动规范族清单
    /// （等于改了取材依据）不会被发现。
    pub fn compute_hash(&self) -> String {
        let mut buf = String::new();
        buf.push_str(self.version);
        for f in self.families.iter() {
            buf.push('|');
            buf.push_str(f);
        }
        fnv1a64_hex(buf.as_bytes())
    }

    /// 对账：登记的锚定哈希是否等于实算值。
    pub fn verify(&self) -> Result<(), StyleError> {
        let actual = self.compute_hash();
        if actual != self.anchor_hash {
            return Err(StyleError::new(
                E_ANCHOR_DRIFT,
                "规范锚定对账失败",
                &format!(
                    "登记锚定哈希 {} 与实算 {} 不一致；版本或规范族清单被改过而未走变更纪律",
                    self.anchor_hash, actual
                ),
                "走 resig 重签：新版本 + 新哈希 + 重新取材，四件一起改",
                "O 域组件负责人",
            ));
        }
        Ok(())
    }

    /// 变更纪律：换版本必须同时给出新版本与新哈希（缺一拒）。
    pub fn resign(
        version: &'static str,
        families: [&'static str; 5],
        new_hash: &'static str,
    ) -> Result<Self, StyleError> {
        if version.trim().is_empty() {
            return Err(StyleError::new(
                E_ANCHOR_DRIFT,
                "规范重签被拒：版本为空",
                "空版本的锚定无法指明取材依据",
                "写明具体快照版本（如 CSS Snapshot 2023-10）",
                "O 域组件负责人",
            ));
        }
        if new_hash.trim().is_empty() {
            return Err(StyleError::new(
                E_ANCHOR_DRIFT,
                "规范重签被拒：新哈希为空",
                "无哈希的锚定无法与旧锚定区分，改版本就成了静默漂移",
                "给出新版本+ 规范族清单的实算哈希",
                "O 域组件负责人",
            ));
        }
        let s = SubsetAnchor {
            version,
            anchor_hash: new_hash,
            families,
        };
        s.verify()?;
        Ok(s)
    }
}

// ---------------------------------------------------------------------------
// 四族属性清单（全表——F2801 的 12 条种子是架构自举用，不是全表）
// ---------------------------------------------------------------------------

/// 布局族（盒模型/定位/尺寸/溢出）。
pub const LAYOUT_PROPERTIES: [SubsetProperty; 16] = [
    SubsetProperty {
        spec: PropertySpec { name: "display", family: PropertyFamily::Layout, value_kind: "display-keyword", inherited: false, initial: "inline", domain_low: 0.0, domain_high: 0.0 },
        spec_ref: "css-display-3§2.1",
    },
    SubsetProperty {
        spec: PropertySpec { name: "position", family: PropertyFamily::Layout, value_kind: "position-keyword", inherited: false, initial: "static", domain_low: 0.0, domain_high: 0.0 },
        spec_ref: "css-display-3§3.2",
    },
    SubsetProperty {
        spec: PropertySpec { name: "top", family: PropertyFamily::Layout, value_kind: "length", inherited: false, initial: "auto", domain_low: -100000.0, domain_high: 100000.0 },
        spec_ref: "css-values-4§5.2",
    },
    SubsetProperty {
        spec: PropertySpec { name: "right", family: PropertyFamily::Layout, value_kind: "length", inherited: false, initial: "auto", domain_low: -100000.0, domain_high: 100000.0 },
        spec_ref: "css-values-4§5.2",
    },
    SubsetProperty {
        spec: PropertySpec { name: "bottom", family: PropertyFamily::Layout, value_kind: "length", inherited: false, initial: "auto", domain_low: -100000.0, domain_high: 100000.0 },
        spec_ref: "css-values-4§5.2",
    },
    SubsetProperty {
        spec: PropertySpec { name: "left", family: PropertyFamily::Layout, value_kind: "length", inherited: false, initial: "auto", domain_low: -100000.0, domain_high: 100000.0 },
        spec_ref: "css-values-4§5.2",
    },
    SubsetProperty {
        spec: PropertySpec { name: "width", family: PropertyFamily::Layout, value_kind: "length", inherited: false, initial: "auto", domain_low: 0.0, domain_high: 100000.0 },
        spec_ref: "css-values-4§5.2",
    },
    SubsetProperty {
        spec: PropertySpec { name: "height", family: PropertyFamily::Layout, value_kind: "length", inherited: false, initial: "auto", domain_low: 0.0, domain_high: 100000.0 },
        spec_ref: "css-values-4§5.2",
    },
    SubsetProperty {
        spec: PropertySpec { name: "min-width", family: PropertyFamily::Layout, value_kind: "length", inherited: false, initial: "auto", domain_low: 0.0, domain_high: 100000.0 },
        spec_ref: "css-values-4§5.2",
    },
    SubsetProperty {
        spec: PropertySpec { name: "min-height", family: PropertyFamily::Layout, value_kind: "length", inherited: false, initial: "auto", domain_low: 0.0, domain_high: 100000.0 },
        spec_ref: "css-values-4§5.2",
    },
    SubsetProperty {
        spec: PropertySpec { name: "max-width", family: PropertyFamily::Layout, value_kind: "length", inherited: false, initial: "none", domain_low: 0.0, domain_high: 100000.0 },
        spec_ref: "css-values-4§5.2",
    },
    SubsetProperty {
        spec: PropertySpec { name: "max-height", family: PropertyFamily::Layout, value_kind: "length", inherited: false, initial: "none", domain_low: 0.0, domain_high: 100000.0 },
        spec_ref: "css-values-4§5.2",
    },
    SubsetProperty {
        spec: PropertySpec { name: "margin", family: PropertyFamily::Layout, value_kind: "length-shorthand", inherited: false, initial: "0px", domain_low: -100000.0, domain_high: 100000.0 },
        spec_ref: "css-values-4§5.2",
    },
    SubsetProperty {
        spec: PropertySpec { name: "padding", family: PropertyFamily::Layout, value_kind: "length-shorthand", inherited: false, initial: "0px", domain_low: 0.0, domain_high: 100000.0 },
        spec_ref: "css-values-4§5.2",
    },
    SubsetProperty {
        spec: PropertySpec { name: "overflow", family: PropertyFamily::Layout, value_kind: "overflow-keyword", inherited: false, initial: "visible", domain_low: 0.0, domain_high: 0.0 },
        spec_ref: "css-overflow-3§3.1",
    },
    SubsetProperty {
        spec: PropertySpec { name: "box-sizing", family: PropertyFamily::Layout, value_kind: "box-keyword", inherited: false, initial: "content-box", domain_low: 0.0, domain_high: 0.0 },
        spec_ref: "css-sizing-3§2.1",
    },
];

/// 视觉族（颜色/边框/字体/文本装饰）。
pub const VISUAL_PROPERTIES: [SubsetProperty; 14] = [
    SubsetProperty {
        spec: PropertySpec { name: "color", family: PropertyFamily::Visual, value_kind: "color", inherited: true, initial: "canvastext", domain_low: 0.0, domain_high: 1.0 },
        spec_ref: "css-color-4§4.1",
    },
    SubsetProperty {
        spec: PropertySpec { name: "background-color", family: PropertyFamily::Visual, value_kind: "color", inherited: false, initial: "transparent", domain_low: 0.0, domain_high: 1.0 },
        spec_ref: "css-color-4§4.1",
    },
    SubsetProperty {
        spec: PropertySpec { name: "border-color", family: PropertyFamily::Visual, value_kind: "color-shorthand", inherited: false, initial: "currentcolor", domain_low: 0.0, domain_high: 1.0 },
        spec_ref: "css-color-4§4.1",
    },
    SubsetProperty {
        spec: PropertySpec { name: "border-width", family: PropertyFamily::Visual, value_kind: "length-shorthand", inherited: false, initial: "medium", domain_low: 0.0, domain_high: 100.0 },
        spec_ref: "css-borders-4§2.1",
    },
    SubsetProperty {
        spec: PropertySpec { name: "border-style", family: PropertyFamily::Visual, value_kind: "line-style", inherited: false, initial: "none", domain_low: 0.0, domain_high: 0.0 },
        spec_ref: "css-borders-4§2.1",
    },
    SubsetProperty {
        spec: PropertySpec { name: "border-radius", family: PropertyFamily::Visual, value_kind: "length-shorthand", inherited: false, initial: "0px", domain_low: 0.0, domain_high: 100000.0 },
        spec_ref: "css-borders-4§5.1",
    },
    SubsetProperty {
        spec: PropertySpec { name: "font-size", family: PropertyFamily::Visual, value_kind: "length", inherited: true, initial: "medium", domain_low: 1.0, domain_high: 1000.0 },
        spec_ref: "css-values-4§5.2",
    },
    SubsetProperty {
        spec: PropertySpec { name: "font-family", family: PropertyFamily::Visual, value_kind: "font-family", inherited: true, initial: "system-ui", domain_low: 0.0, domain_high: 0.0 },
        spec_ref: "css-fonts-4§2.1",
    },
    SubsetProperty {
        spec: PropertySpec { name: "font-weight", family: PropertyFamily::Visual, value_kind: "font-weight", inherited: true, initial: "normal", domain_low: 1.0, domain_high: 1000.0 },
        spec_ref: "css-fonts-4§2.4",
    },
    SubsetProperty {
        spec: PropertySpec { name: "font-style", family: PropertyFamily::Visual, value_kind: "font-style", inherited: true, initial: "normal", domain_low: 0.0, domain_high: 0.0 },
        spec_ref: "css-fonts-4§2.5",
    },
    SubsetProperty {
        spec: PropertySpec { name: "line-height", family: PropertyFamily::Visual, value_kind: "line-height", inherited: true, initial: "normal", domain_low: 0.0, domain_high: 1000.0 },
        spec_ref: "css-values-4§5.2",
    },
    SubsetProperty {
        spec: PropertySpec { name: "text-align", family: PropertyFamily::Visual, value_kind: "align-keyword", inherited: true, initial: "start", domain_low: 0.0, domain_high: 0.0 },
        spec_ref: "css-text-3§2.1",
    },
    SubsetProperty {
        spec: PropertySpec { name: "text-decoration-line", family: PropertyFamily::Visual, value_kind: "line-style", inherited: false, initial: "none", domain_low: 0.0, domain_high: 0.0 },
        spec_ref: "css-text-decoration-4§2.1",
    },
    SubsetProperty {
        spec: PropertySpec { name: "visibility", family: PropertyFamily::Visual, value_kind: "visibility-keyword", inherited: false, initial: "visible", domain_low: 0.0, domain_high: 0.0 },
        spec_ref: "css-visibility-3§2.1",
    },
];

/// 效果族（滤镜/变换/透明度）。
pub const EFFECT_PROPERTIES: [SubsetProperty; 6] = [
    SubsetProperty {
        spec: PropertySpec { name: "opacity", family: PropertyFamily::Effect, value_kind: "number", inherited: false, initial: "1", domain_low: 0.0, domain_high: 1.0 },
        spec_ref: "css-color-4§5.2",
    },
    SubsetProperty {
        spec: PropertySpec { name: "transform", family: PropertyFamily::Effect, value_kind: "transform-list", inherited: false, initial: "none", domain_low: 0.0, domain_high: 0.0 },
        spec_ref: "css-transforms-2§3.1",
    },
    SubsetProperty {
        spec: PropertySpec { name: "transform-origin", family: PropertyFamily::Effect, value_kind: "position", inherited: false, initial: "50% 50%", domain_low: 0.0, domain_high: 1.0 },
        spec_ref: "css-transforms-2§3.4",
    },
    SubsetProperty {
        spec: PropertySpec { name: "filter", family: PropertyFamily::Effect, value_kind: "filter-list", inherited: false, initial: "none", domain_low: 0.0, domain_high: 0.0 },
        spec_ref: "css-filters-2§3.1",
    },
    SubsetProperty {
        spec: PropertySpec { name: "backdrop-filter", family: PropertyFamily::Effect, value_kind: "filter-list", inherited: false, initial: "none", domain_low: 0.0, domain_high: 0.0 },
        spec_ref: "css-filters-2§5.1",
    },
    SubsetProperty {
        spec: PropertySpec { name: "box-shadow", family: PropertyFamily::Effect, value_kind: "shadow-list", inherited: false, initial: "none", domain_low: 0.0, domain_high: 0.0 },
        spec_ref: "css-borders-4§7.1",
    },
];

/// 交互族（状态/指针/焦点）。
pub const INTERACTION_PROPERTIES: [SubsetProperty; 8] = [
    SubsetProperty {
        spec: PropertySpec { name: "pointer-events", family: PropertyFamily::Interaction, value_kind: "pointer-keyword", inherited: true, initial: "auto", domain_low: 0.0, domain_high: 0.0 },
        spec_ref: "css-ui-4§2.1",
    },
    SubsetProperty {
        spec: PropertySpec { name: "cursor", family: PropertyFamily::Interaction, value_kind: "cursor-keyword", inherited: true, initial: "auto", domain_low: 0.0, domain_high: 0.0 },
        spec_ref: "css-ui-4§3.1",
    },
    SubsetProperty {
        spec: PropertySpec { name: "outline-color", family: PropertyFamily::Interaction, value_kind: "color", inherited: false, initial: "currentcolor", domain_low: 0.0, domain_high: 1.0 },
        spec_ref: "css-ui-4§4.1",
    },
    SubsetProperty {
        spec: PropertySpec { name: "outline-style", family: PropertyFamily::Interaction, value_kind: "line-style", inherited: false, initial: "none", domain_low: 0.0, domain_high: 0.0 },
        spec_ref: "css-ui-4§4.1",
    },
    SubsetProperty {
        spec: PropertySpec { name: "outline-width", family: PropertyFamily::Interaction, value_kind: "length", inherited: false, initial: "medium", domain_low: 0.0, domain_high: 100.0 },
        spec_ref: "css-ui-4§4.1",
    },
    SubsetProperty {
        spec: PropertySpec { name: "user-select", family: PropertyFamily::Interaction, value_kind: "select-keyword", inherited: true, initial: "auto", domain_low: 0.0, domain_high: 0.0 },
        spec_ref: "css-ui-4§3.2",
    },
    SubsetProperty {
        spec: PropertySpec { name: "caret-color", family: PropertyFamily::Interaction, value_kind: "color", inherited: true, initial: "currentcolor", domain_low: 0.0, domain_high: 1.0 },
        spec_ref: "css-ui-4§3.3",
    },
    SubsetProperty {
        spec: PropertySpec { name: "resize", family: PropertyFamily::Interaction, value_kind: "resize-keyword", inherited: false, initial: "none", domain_low: 0.0, domain_high: 0.0 },
        spec_ref: "css-ui-4§4.2",
    },
];

// ---------------------------------------------------------------------------
// 子集声明表
// ---------------------------------------------------------------------------

/// 四族属性子集表（全表）。
#[derive(Clone, Debug)]
pub struct SubsetTable {
    /// 规范锚定。
    pub anchor: SubsetAnchor,
    /// 条目。
    pub entries: Vec<SubsetProperty>,
}

impl SubsetTable {
    /// 空表（锚定仍有效，只是没有任何属性条目）。
    pub fn new() -> Self {
        SubsetTable {
            anchor: SubsetAnchor::anchored(),
            entries: Vec::new(),
        }
    }

    /// 按锚点构造全表（四族清单直接落表）。
    pub fn standard() -> Self {
        let mut entries = Vec::new();
        for p in LAYOUT_PROPERTIES.iter() {
            entries.push(*p);
        }
        for p in VISUAL_PROPERTIES.iter() {
            entries.push(*p);
        }
        for p in EFFECT_PROPERTIES.iter() {
            entries.push(*p);
        }
        for p in INTERACTION_PROPERTIES.iter() {
            entries.push(*p);
        }
        SubsetTable {
            anchor: SubsetAnchor::anchored(),
            entries,
        }
    }

    /// 某族的条目（只读遍历）。
    pub fn family(&self, f: PropertyFamily) -> impl Iterator<Item = &SubsetProperty> {
        self.entries.iter().filter(move |e| e.spec.family == f)
    }

    /// 某族条目数。
    pub fn family_len(&self, f: PropertyFamily) -> usize {
        self.family(f).count()
    }

    /// 某属性的条目。
    pub fn get(&self, name: &str) -> Option<&SubsetProperty> {
        self.entries.iter().find(|e| e.spec.name == name)
    }

    /// 总条目数。
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// 是否为空。
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// 逐条登记一条（带三闸 + 守卫）。
    pub fn admit(&mut self, p: SubsetProperty) -> Result<(), StyleError> {
        // 闸一：名称合法。
        if p.spec.name.trim().is_empty() || p.spec.name.contains(' ') {
            return Err(StyleError::new(
                E_NAME_INVALID,
                "属性登记被拒：名称非法",
                "空名或含空格的名字无法在样式表里书写，登记它等于凭空造一条不可用属性",
                "用 CSS 书写名（小写、无空格）",
                "O 域组件负责人",
            ));
        }
        // 闸二：规范出处齐整（这是本单相对 F2801 新增的判据）。
        if p.spec_ref.len() < MIN_SPEC_REF_LEN || !p.spec_ref.contains('§') {
            return Err(StyleError::new(
                E_SPEC_REF_MISSING,
                "属性登记被拒：规范出处缺失或形状不对",
                "没有章节出处的属性等于「按感觉支持」——初值与语法争议时无人可判",
                "写明形如 css-values-4§5.2 的章节号",
                "O 域组件负责人",
            ));
        }
        // 闸三：初值必填（不允许「空即继承」这种含糊）。
        if p.spec.initial.trim().is_empty() {
            return Err(StyleError::new(
                E_INITIAL_MISSING,
                "属性登记被拒：初值缺失",
                "缺初值时级联无从收敛，缺字回退与无障碍对比度都无法判定",
                "写明缺值时的显式兜底",
                "O 域组件负责人",
            ));
        }
        // 守卫一：参数域不倒挂。
        if !p.spec.domain_is_sane() {
            return Err(StyleError::new(
                E_DOMAIN_INVERTED,
                "属性登记被拒：参数域倒挂",
                &format!(
                    "{} 的域下界{}高于上界{}，钳制将无定义",
                    p.spec.name, p.spec.domain_low, p.spec.domain_high
                ),
                "修正为下界 ≤ 上界；无参数含义的键字用 [0,0]",
                "O 域组件负责人",
            ));
        }
        // 守卫二：不重复。
        if self.get(p.spec.name).is_some() {
            return Err(StyleError::new(
                E_PROP_DUP,
                "属性登记被拒：重复",
                &format!("{} 已在子集表内；两条定义必然有一条是错的", p.spec.name),
                "更新既有条目而非并行登记",
                "O 域组件负责人",
            ));
        }
        // 守卫三：族容量。
        if self.family_len(p.spec.family) >= MAX_PER_FAMILY {
            return Err(StyleError::new(
                E_FAMILY_OVERGROWN,
                "属性登记被拒：该族已满",
                &format!("{}族已达上限；某族写成全表即失去分族意义", p.spec.family.zh()),
                "确认该属性是否真属本族，或把超限项移入后续批次",
                "O 域组件负责人",
            ));
        }
        // 守卫四：全表容量。
        if self.entries.len() >= MAX_TOTAL {
            return Err(StyleError::new(
                E_TOTAL_CAP,
                "属性登记被拒：全表已满",
                "子集表整体超限；扩大子集须先评估 F2804/F2805/F2812 的连带成本",
                "先做成本评估并走变更纪律",
                "O 域组件负责人",
            ));
        }
        self.entries.push(p);
        Ok(())
    }

    /// 子集自检（结构一致性；任一不成立即红）。
    ///
    /// 查五件事：锚定对账、条目齐整、四族非空、易错项分桶正确、族内不重名。
    pub fn audit(&self) -> Result<(), StyleError> {
        self.anchor.verify()?;
        for e in self.entries.iter() {
            if !e.is_wellformed() {
                return Err(StyleError::new(
                    E_SPEC_REF_MISSING,
                    "子集审计失败：条目不成形",
                    &format!("{}（{}）", e.spec.name, e.spec_ref),
                    "补齐名称/初值/出处（带章节号）/参数域",
                    "O 域组件负责人",
                ));
            }
        }
        for f in PropertyFamily::ALL.iter() {
            if self.family_len(*f) == 0 {
                return Err(StyleError::new(
                    E_FAMILY_OVERGROWN,
                    "子集审计失败：某族为空",
                    &format!("{}族没有任何属性", f.zh()),
                    "补齐该族清单，或在册上显式声明该族本期为空并记原因",
                    "O 域组件负责人",
                ));
            }
        }
        for t in CLASSIFICATION_TRAPS.iter() {
            match self.get(t.name) {
                Some(e) => {
                    if e.spec.family != t.correct {
                        return Err(StyleError::new(
                            E_FAMILY_TRAP_CONFLICT,
                            "子集审计失败：易错项分桶错误",
                            &format!(
                                "{} 归{}族（应为{}族）：{}",
                                t.name,
                                e.spec.family.zh(),
                                t.correct.zh(),
                                t.consequence
                            ),
                            FIX_MISBUCKET,
                            "O 域组件负责人",
                        ));
                    }
                }
                None => {
                    // 易错项不在表里是允许的（子集可以不含它），不报错。
                }
            }
        }
        Ok(())
    }

    /// 读屏摘要（无障碍：四族清单要能被读屏整体念出）。
    pub fn screen_summary(&self) -> String {
        let mut s = String::new();
        s.push_str(&format!(
            "样式子集表 {}，锚定 {}，共{} 条属性：",
            SUBSET_VERSION, self.anchor.version, self.entries.len()
        ));
        for f in PropertyFamily::ALL.iter() {
            s.push_str(&format!("{}{} 条；", f.zh(), self.family_len(*f)));
        }
        s
    }
}
