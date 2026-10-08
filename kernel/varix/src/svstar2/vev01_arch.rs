//! VE-F4401 · V 域开工与多显示色彩总架构（VE-V 域 · 显示与色彩域 · V01 组 · 目标 400 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F4401`
//!
//! **判据（锚点原文五条）**：四层、接口冻结、承接落地、色准硬线、判据。逐条落位：
//! - **四层**：[`Layer`]（设备层→色彩层→HDR 层→应用层，层链相邻闭合）——
//!   判据一落位见 [`DisplayColorArchitecture::check_four_layers`]。
//! - **接口冻结**：[`InterfaceFreezeLedger`]——层间三条相邻边冻结，越权变更必先有
//!   已接受 ADR（`E_INTERFACE_UNFROZEN_CHANGE` / `E_ADR_REQUIRED`）。
//! - **承接落地**：[`AcceptanceLedger`]——承接 U 域移交包 `VE-F4395` 十件中的三个
//!   承接点（对比度契约 / 色弱映射 / 交互词典）入**显示契约源**并全落地对账；
//!   缺源走 [`TraceBack`] 回溯移交包。
//! - **色准硬线**：[`AccuracyHardline`]——域本色核心：对比度与色弱映射是**一等契约**，
//!   唯一持有者是色彩层，不接受豁免、不接受延后、不接受别层留副本。
//! - **判据**：[`Criterion`] 五项全集 + [`Criterion::check_group`] 的判据→自检项
//!   可追溯映射（判据本身可追溯，防止「判据只写在文档里」）。
//!
//! **职责定位（锚点原文）**：V 域开工（显示与色彩域：多显示器统一管理 / 色彩管理 /
//! HDR / 虚拟显示——**让画面在每块屏上都真、都准**）；总架构四层（设备层→色彩层→
//! HDR 层→应用层，层间接口冻结）；域本色声明（**色准即无障碍硬线**：对比度与色弱
//! 映射为一等契约）。
//!
//! **数据结构（锚点原文）**：总架构册（四层）；层间接口冻结；承接面落地表。
//!
//! **错误路径与降级矩阵（锚点原文）**：层间失配→对拍；承接缺源→回溯移交包；
//! 接口越权→冻结流程。落位见 [`DisplayColorArchitecture::cross_check`]（对拍）、
//! [`AcceptanceLedger::trace_back`]（回溯）、[`InterfaceFreezeLedger::rebase`]
//! （冻结流程唯一出口）。
//!
//! **性能逐项分解（锚点原文）**：架构 O(层数)；冻结 O(接口数)；落地 O(源数)。
//! 三项的落位与复杂度声明见 [`COMPLEXITY_DOC`]，逐方法复杂度注记在各方法头注。
//!
//! **跨批对接点（锚点原文）**：F4395 移交包上游；F4402 管理下游；F4420 双签闸。
//! 全部 19 项下游归属见 [`DOWNSTREAM_OWNERSHIP`]。
//!
//! **无障碍与隐私（锚点原文）**：色准与色弱映射入契约层（域本色核心）；配置不含隐私。
//! 前者落位在 [`AccuracyHardline`]，后者落位在 [`PRIVACY_FORBIDDEN_TOKENS`] 与
//! [`DisplayColorArchitecture::check_privacy`]——显示配置里出现内容类字段即拒。
//!
//! # 一、号段冲突的显性裁决（册内两处文本不一致，不含糊过去）
//!
//! 册内两处对 `VE-F4401` 的归属说法不同，本模块把冲突摊开而不是挑一个顺眼的用：
//!
//! | 出处 | 说法 |
//! |---|---|
//! | 册首域表（第 104 行） | `VE-W` = F4401-F4600 = 多媒体合成 |
//! | 锚点正文 F4401 与 F4395 交接面 | `VE-F4401` = **V 域**开工与多显示色彩总架构 |
//!
//! 裁决：**以锚点正文为准**（依据是任务单验收条「规格与判据按册内锚点原文逐条
//! 落实」，锚点正文是判据的所在）。理由三条：
//!
//! 1. 判据只在锚点正文里。域表只有区间与一句话职责，不含判据、无判据就无从施工；
//! 2. 上游 `VE-F4395`（U 域移交包）正文明写「V 域 F4401」为消费方，两处互证；
//! 3. F4402 起连续 19 项锚点全部是显示与色彩主题（多显示器/色彩引擎/HDR/配置/
//!    拼接/刷新率），与「多媒体合成」主题不符——若按域表执行，F4402 之后整组
//!    19 项都会落错域。
//!
//! 后果如实记录：域表第 104 行的 `VE-W` 区间起点应顺延至 F4421，本模块不动册
//! （册非本单范围），冲突登记在 [`NUMBERING_ADJUDICATION`] 供后续修订时对拍。
//!
//! # 二、四层 vs 四能力：不是 5:5 也不是 4:4（锚点计数歧义与裁决）
//!
//! 锚点给了**两份四项清单**，它们不是同一份东西：
//!
//! - **职责定位列四能力**：多显示器统一管理 / 色彩管理 / HDR / 虚拟显示；
//! - **总架构列四层**：设备层 → 色彩层 → HDR 层 → 应用层。
//!
//! 逐项对拍有两条不1:1，本项不把任一项悄悄丢掉，也不硬凑成 4:4：
//!
//! | 锚点能力 | 落点层 | 性质 |
//! |---|---|---|
//! | 多显示器统一管理 | [`Layer::Device`] | 1:1 |
//! | 色彩管理 | [`Layer::Color`] | 1:1 |
//! | HDR | [`Layer::Hdr`] | 1:1 |
//! | 虚拟显示 | [`Layer::Device`] | 派生：虚拟屏是设备层的一种**显示目标来源**，无物理屏而已；其色彩与 HDR 一律走上层同一条链 |
//! | **无** | [`Layer::App`] | **消费面层**：应用层不产生能力，只消费色彩层契约 |
//!
//! 两条裁决：
//!
//! - **虚拟显示归设备层而非新开一层**：虚拟屏与物理屏在色彩/HDR 面前没有区别，
//!   区别只在「有无 EDID」。若为它单开一层，虚拟屏的色准就变成旁路——而色准
//!   硬线恰恰不许有旁路。它归设备层，主责条目 [`VE-F4410`]。
//! - **应用层是消费面层**（[`Layer::is_consumer_only`]）：锚点的四能力里没有一项
//!   归它。这不是漏项，而是四层里必然存在的一端——前三层产出色彩事实，应用层
//!   消费。若把「色彩意图协商」硬算成第五项能力，能力数变五，与锚点明文的四项
//!   不符；协商的本体归 [`VE-F4408`]（见 [`DOWNSTREAM_OWNERSHIP`]），本域只立
//!   「应用层必须盖色彩契约版本戳」这条层约束。
//!
//! 机检在 [`DisplayColorArchitecture::check_capability_alignment`]：**每层要么被
//! 至少一项能力覆盖，要么显式声明为消费面层**，否则报 `E_LAYER_UNCLAIMED`——
//! 这一条防的是「层被当成无主资源」，不是防计数。
//!
//! # 三、色准硬线为什么单列一条判据（域本色的工程含义）
//!
//! 锚点写「色准即无障碍硬线：对比度与色弱映射为一等契约」。这句话在工程上只
//! 有一种成立形态——**它必须是无处可躲的一等条目**：不是注释、不是可选项、不是
//! 「后续版本补」的待办。因此 [`AccuracyHardline`] 把四条禁止写成可执行判定：
//!
//! 1. **入契约层**：[`ColorContractEntry::owner_layer`] 必须是 [`Layer::Color`]；
//!    契约文本的**唯一**持有者是色彩层，别层只能持指针（[`Layer::owns_color_contract`]，
//!    违反报 `E_COLOR_CONTRACT_DUPLICATED`）。这条与 U 域「规则文本只在契约层」
//!    同构，但对象换成色彩事实——因为色彩被复制一份就意味着色准出现两个真相。
//! 2. **一等**：[`ColorContractEntry::first_class`] 为假即拒（`E_ACCURACY_NOT_FIRST_CLASS`）。
//! 3. **不豁免**：[`AccuracyHardline::request_waiver`] **恒失败**——硬线不接受
//!    「本期不做」的写法（`E_ACCURACY_WAIVED_FORBIDDEN`）。可执行地失败比注释
//!    里的「不得豁免」有用：注释不会在评审里被验证，函数会。
//! 4. **不延后**：`deferred` 为真即拒（`E_ACCURACY_DEFERRED_FORBIDDEN`）。
//!
//! 外加消费侧一条：应用层出帧必须携带二者的契约版本戳（[`Layer::App`]，
//! [`AccuracyHardline::stamp_required`]）——否则色彩层改了契约而应用层照旧出帧，
//! 硬线就只剩登记没有约束力。
//!
//! # 四、四层的顺序不是随手排的（层序单源）
//!
//! 设备层 → 色彩层 → HDR 层 → 应用层，是**事实到消费**的方向：屏的物理事实（EDID
//! 与校准）先立住，色彩变换才有依据，色调映射才有输入，应用层最后拿到的是已经
//! 收敛的画面。层序单源于 [`Layer::rank`]，接口位序、读屏叙述、机检全部由它派生，
//! 不允许第二处顺序声明（违反报 `E_LAYER_ORDER`）。
//!
//! 逻辑 tick 注入、零墙钟、零 IO；只用 `alloc` 容器（无 HashMap——内核无 hasher
//! 依赖，线性扫描并诚实标注复杂度）。

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

use crate::checks::CheckSet;

// ---------------------------------------------------------------------------
// 一、规格常量（参数唯一源）
// ---------------------------------------------------------------------------

/// 总架构版本。
pub const ARCH_VERSION: &str = "V01-arch-v1";

/// 层间接口冻结版本。
pub const INTERFACE_VERSION: &str = "V01-iface-v1";

/// 四层层数（判据一的硬数字）。
pub const LAYER_COUNT: usize = 4;

/// 锚点职责定位列出的能力数（多显示器统一管理/色彩管理/HDR/虚拟显示）。
pub const CAPABILITY_COUNT: usize = 4;

/// 判据项数（锚点判据原文五条）。
pub const CRITERION_COUNT: usize = 5;

/// 接口上限（四层相邻边只有 3 条，多出来的必是非相邻连线）。
pub const MAX_INTERFACES: usize = 4;

/// ADR 账上限。
pub const MAX_ADRS: usize = 32;

/// 承接表上限。
pub const MAX_ACCEPTANCE_SOURCES: usize = 16;

/// 禁扩面上限。
pub const MAX_EXCLUSIONS: usize = 12;

/// 哈希十六进制定宽。
pub const HASH_HEX_LEN: usize = 16;

/// 上游 U 域移交包条目号（回溯目的地，单源常量）。
///
/// 锚点 F4395 正文：「U 域移交包（V 域 F4401）」——是本域开工闸的上游消费方声明。
pub const U_PACKAGE_ITEM: &str = "VE-F4395";

/// U 域移交包件数（锚点 F4395「十件封装」：总账/词典/联测/基准/安全/无障碍/
/// 观测/fuzz/文档/清账）。
pub const U_PACKAGE_ITEM_COUNT: usize = 10;

/// V 域承接点数（锚点 F4395 交接面：「对比度契约+色弱映射+交互词典」三件）。
pub const HANDOFF_POINT_COUNT: usize = 3;

/// 复杂度声明编号表（锚点性能逐项分解：架构 O(层数)；冻结 O(接口数)；落地 O(源数)）。
pub const COMPLEXITY_DOC: &str = "\
复杂度声明（VE-F4401 ·锚点性能逐项分解原文）：
  C1 O(层数)   —— 四层册校验、层链闭合、层序递增、消费面层核（四层为定长，实际 O(1)）；
  C2 O(接口数) —— 接口册建立逐条校验、冻结、层间对拍（三条相邻边为定长，实际 O(1)）；
  C3 O(源数)   —— 承接面登记、落地对账、回溯（承接表为定长上界 16，实际 O(1)）；
  C4 O(能力数) —— 四能力与四层双向对账（定长 4×4，实际 O(1)）；
  C5 O(契约数) —— 色准硬线登记与齐备核（定长 2，实际 O(1)）。
本域总架构为纯声明期结构，运行路径零开销（见 LayerSpec::cost）。";

/// 域本色声明（锚点原文：色准即无障碍硬线）。
pub const DOMAIN_CHARACTER: &str = "\
域本色声明（VE-F4401 · VE-V 域）：色准即无障碍硬线。
对比度契约与色弱映射契约是显示契约源中的一等契约，不是注释、不是可选项、
不是待办：唯一持有者是色彩层，别层只持指针；不接受豁免、不接受延后登记；
应用层出帧必须携带二者契约版本戳。对比度与色弱映射同属VE-S 域无障碍双维的
显示侧投影，本域只保「显示契约源里有一等条目且无人留副本」，
滤镜曲线本体归 VE-F4411。";

/// 号段冲突裁决登记（册内两处文本不一致的显性记录，见头注§一）。
pub const NUMBERING_ADJUDICATION: &str = "\
号段冲突裁决（VE-F4401）：册首域表第 104 行记VE-W = F4401-F4600 多媒体合成；
锚点正文 F4401 记「V 域开工与多显示色彩总架构」，上游移交包 VE-F4395 的交接面记
消费方，F4402 起连续 19 项锚点均为显示与色彩主题。
裁决：以锚点正文为准（判据只在锚点正文；两处互证；按域表执行会使F4402-F4420
整组 19 项落错域）。待修订项：域表 VE-W 区间起点应顺延至 F4421。
本条仅登记，册非本单范围，故不改册。";

/// 虚拟显示归层的裁决登记（见头注§二）。
pub const VIRTUAL_DISPLAY_ADJUDICATION: &str = "\
虚拟显示归层裁决（VE-F4401）：锚点四能力含「虚拟显示」，四层无对应层。
裁决：归设备层——虚拟屏与物理屏在色彩/HDR 面前无区别，区别只在有无 EDID；
单开一层即给虚拟屏色准开出旁路，而色准硬线不许有旁路。主责条目 VE-F4410。
应用层同理为消费面层（四能力无一项归它），主责条目 VE-F4408（意图协商本体）。";

// ---------------------------------------------------------------------------
// 二、四层总架构册（判据一）
// ---------------------------------------------------------------------------

/// V 域四层（顺序即 [`LAYER_ORDER`]，唯一真值源）。
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Layer {
    /// 层一：设备层（多显示器统一管理 + 虚拟显示——屏的物理事实）。
    Device,
    /// 层二：色彩层（色彩管理——色彩变换与显示契约源的唯一持有者）。
    Color,
    /// 层三：HDR 层（HDR 管线——探测/元数据/色调映射/SDR 同屏混合）。
    Hdr,
    /// 层四：应用层（消费面层——消费已收敛的色彩事实出帧）。
    App,
}

impl Layer {
    /// 四层全集（顺序即 [`LAYER_ORDER`]）。
    pub const ALL: [Layer; LAYER_COUNT] = [Layer::Device, Layer::Color, Layer::Hdr, Layer::App];

    /// 中文名（读屏播报用）。
    pub fn zh(self) -> &'static str {
        match self {
            Layer::Device => "设备层",
            Layer::Color => "色彩层",
            Layer::Hdr => "HDR层",
            Layer::App => "应用层",
        }
    }

    /// 英文名（标识符与文档用）。
    pub fn en(self) -> &'static str {
        match self {
            Layer::Device => "device",
            Layer::Color => "color",
            Layer::Hdr => "hdr",
            Layer::App => "app",
        }
    }

    /// 层码（对外引用，如 `V01-L2`）。
    pub fn code(self) -> &'static str {
        match self {
            Layer::Device => "V01-L1",
            Layer::Color => "V01-L2",
            Layer::Hdr => "V01-L3",
            Layer::App => "V01-L4",
        }
    }

    /// 层位序号（0 起）。**全模块唯一的层序真值**——下游层、接口位序、读屏叙述
    /// 全部由它派生，不允许第二处顺序声明。
    pub fn rank(self) -> u8 {
        match self {
            Layer::Device => 0,
            Layer::Color => 1,
            Layer::Hdr => 2,
            Layer::App => 3,
        }
    }

    /// 直接下游层（`None` = 顶层终点）。
    pub fn downstream(self) -> Option<Layer> {
        match self {
            Layer::Device => Some(Layer::Color),
            Layer::Color => Some(Layer::Hdr),
            Layer::Hdr => Some(Layer::App),
            Layer::App => None,
        }
    }

    /// 是否为**显示契约源（色彩契约文本）的唯一持有者**。
    ///
    /// 只有色彩层返回 `true`。这条是色准硬线的结构面：契约文本只能存在一处，
    /// 其余三层持指针（见头注§三第1 条）。
    pub fn owns_color_contract(self) -> bool {
        matches!(self, Layer::Color)
    }

    /// 是否为**消费面层**（不产生能力，只消费）。
    ///
    /// 四能力无一项归应用层，这是四层结构的必然一端（见头注§二）。
    /// 声明消费面是义务而不是豁免：消费面仍受层约束（必须盖契约版本戳）。
    pub fn is_consumer_only(self) -> bool {
        matches!(self, Layer::App)
    }

    /// 读屏单行。
    pub fn screen_line(self) -> String {
        let mut s = format!("层 {}（{} ·{}）：", self.code(), self.zh(), self.en());
        if self.owns_color_contract() {
            s.push_str("显示契约源唯一持有者；");
        }
        if self.is_consumer_only() {
            s.push_str("消费面层，不产出能力；");
        }
        match self.downstream() {
            Some(d) => s.push_str(&format!("下游为{}。", d.zh())),
            None => s.push_str("顶层终点，无下游。"),
        }
        s
    }
}

/// 层序（由 [`Layer::rank`] 派生，顺序即架构位次）。
pub const LAYER_ORDER: [Layer; LAYER_COUNT] = Layer::ALL;

/// 层的成本形态。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LayerCost {
    /// 声明期：本层只在声明/编译期做事，运行路径上不花钱。
    DeclarationOnly,
    /// 运行期：本层在运行路径上做事。
    PerRun,
}

impl LayerCost {
    /// 中文名。
    pub fn zh(self) -> &'static str {
        match self {
            LayerCost::DeclarationOnly => "声明期",
            LayerCost::PerRun => "运行期",
        }
    }

    /// 是否零开销（域开工总纲的判定位）。
    pub fn is_zero_overhead(self) -> bool {
        matches!(self, LayerCost::DeclarationOnly)
    }
}

/// V 域层契约（总架构册的行结构）。
///
/// 每层七项齐发：职责/输入/输出/失败策略/复杂度/不做清单/主责条目。
/// **缺一项的层不许进表**——缺「不做清单」的层会被后来人当成万能筐
/// （上一个条目顺手把下一个条目的活干了，下游开工时发现「已经有人做过了，
/// 但没人知道在哪、依据是什么」）。
#[derive(Clone, Copy, Debug)]
pub struct LayerSpec {
    /// 所属层。
    pub layer: Layer,
    /// 层位（与 [`Layer::rank`] 同值，冗余存储是为了让表可排序可对拍）。
    pub rank: u8,
    /// 中文职责名。
    pub duty_zh: &'static str,
    /// 输入契约（吃什么）。
    pub input: &'static str,
    /// 输出契约（吐什么）。
    pub output: &'static str,
    /// 失败策略（锚点降级矩阵落到本层的那一格）。
    pub on_failure: &'static str,
    /// 复杂度声明（对应 [`COMPLEXITY_DOC`] 编号）。
    pub complexity: &'static str,
    /// 本层的**不做清单**（越界即违约）。
    pub not_mine: &'static str,
    /// 主责条目（层本体归谁——防止层被当成无主资源）。
    pub owner_item: &'static str,
    /// 成本形态。
    pub cost: LayerCost,
}

impl LayerSpec {
    /// 层契约七项齐备性自检（缺一即不合格——残缺的层契约无法对拍）。
    pub fn is_complete(&self) -> bool {
        !self.duty_zh.trim().is_empty()
            && !self.input.trim().is_empty()
            && !self.output.trim().is_empty()
            && !self.on_failure.trim().is_empty()
            && !self.complexity.trim().is_empty()
            && !self.not_mine.trim().is_empty()
            && !self.owner_item.trim().is_empty()
    }

    /// 读屏单行。
    pub fn screen_line(&self) -> String {
        format!(
            "层 {}（{} · {}）：吃 {}；吐 {}；失败 {}；不做 {}",
            self.layer.code(),
            self.layer.zh(),
            self.duty_zh,
            self.input,
            self.output,
            self.on_failure,
            self.not_mine
        )
    }
}

/// 标准四层册（**四层 4/4 硬门的正样本**）。
///
/// 层序的依据见头注§四：设备层在最前（不立物理事实则色彩无依据），
/// 应用层在最后（它是消费方，不是生产方）。
pub const STANDARD_LAYERS: [LayerSpec; LAYER_COUNT] = [
    LayerSpec {
        layer: Layer::Device,
        rank: 0,
        duty_zh: "设备层：立住屏的物理事实——多显示器统一管理与虚拟显示同层",
        input: "EDID/GUID/端口三元指纹 + 校准档 + 热插拔事件",
        output: "显示器台账（屏数/位次/主副/能力表/校准引用）",
        on_failure: "枚举缺失 -> 降级轮询；指纹冲突 -> 以 GUID 为准并标注；热插拔风暴 -> 事件合并窗",
        complexity: "C1 O(层数)",
        not_mine: "不建台账与指纹卡本体（VE-F4402），不做拓扑四元组（VE-F4421）",
        owner_item: "VE-F4402",
        cost: LayerCost::DeclarationOnly,
    },
    LayerSpec {
        layer: Layer::Color,
        rank: 1,
        duty_zh: "色彩层：色彩变换调度+显示契约源唯一持有者（色准硬线落层处）",
        input: "设备层台账与校准引用 + 应用层色彩意图 + 承接面对比度/色弱契约",
        output: "色彩变换结果 + 色彩事实（色彩空间/位深/变换戳/契约版本戳）",
        on_failure: "配置解析失败 -> 缺省 sRGB 路径并标注不静默；调度冲突 -> 按应用意图仲裁；缓存失真 -> 版本戳失效",
        complexity: "C1 O(层数)",
        not_mine: "不建引擎四模块（VE-F4403），不建矩阵库（VE-F4406），不写色弱滤镜曲线（VE-F4411）",
        owner_item: "VE-F4403",
        cost: LayerCost::DeclarationOnly,
    },
    LayerSpec {
        layer: Layer::Hdr,
        rank: 2,
        duty_zh: "HDR 层：能力探测/元数据抽象/色调映射/SDR 同屏混合四段",
        input: "色彩层色彩事实 + 设备层 HDR 能力声明",
        output: "色调映射后显示帧 + HDR 元数据抽象层输出 + 亮度协调结果",
        on_failure: "探测失准 -> 以 EDID 声明为准并标注；元数据缺失 -> 静态映射回退；混合过曝 -> 亮度钳制",
        complexity: "C1 O(层数)",
        not_mine: "不建四段管线本体（VE-F4404），不做跨屏 HDR 会话迁移（VE-F4426）",
        owner_item: "VE-F4404",
        cost: LayerCost::DeclarationOnly,
    },
    LayerSpec {
        layer: Layer::App,
        rank: 3,
        duty_zh: "应用层：消费面层——消费已收敛色彩事实出帧，并申报色彩意图",
        input: "HDR 层显示帧 + 色彩层契约版本戳",
        output: "应用出帧（含契约版本戳与色彩意图回执）",
        on_failure: "未盖契约版本戳 -> 拒绝出帧（E_STAMP_REQUIRED）；意图与屏幕能力冲突 -> 回执告知并回退",
        complexity: "C1 O(层数)",
        not_mine: "不建意图协商三步协议（VE-F4408），不建虚拟屏能力声明（VE-F4410）",
        owner_item: "VE-F4408",
        cost: LayerCost::DeclarationOnly,
    },
];

// ---------------------------------------------------------------------------
// 三、四能力与四层的双向对账（判据一的对账面，见头注§二）
// ---------------------------------------------------------------------------

/// 开工能力（锚点职责定位原文：多显示器统一管理 / 色彩管理 / HDR / 虚拟显示）。
///
/// 能力与层**不是同一份清单**（头注§二）。两者关系由 [`Capability::layers`] 表达，
/// 机检在 [`DisplayColorArchitecture::check_capability_alignment`]。
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Capability {
    /// 能力一：多显示器统一管理（本体 VE-F4402）。
    MultiDisplay,
    /// 能力二：色彩管理（本体 VE-F4403 + 转换深化 VE-F4406——一能力两段）。
    ColorMgmt,
    /// 能力三：HDR（本体 VE-F4404）。
    Hdr,
    /// 能力四：虚拟显示（本体 VE-F4410，**派生**于设备层）。
    VirtualDisplay,
}

impl Capability {
    /// 四能力全集（顺序即 [`CAPABILITY_ORDER`]，唯一真值源）。
    pub const ALL: [Capability; CAPABILITY_COUNT] = [
        Capability::MultiDisplay,
        Capability::ColorMgmt,
        Capability::Hdr,
        Capability::VirtualDisplay,
    ];

    /// 中文名（读屏播报用）。
    pub fn zh(self) -> &'static str {
        match self {
            Capability::MultiDisplay => "多显示器统一管理",
            Capability::ColorMgmt => "色彩管理",
            Capability::Hdr => "HDR",
            Capability::VirtualDisplay => "虚拟显示",
        }
    }

    /// 能力码（对外引用，如 `V01-C2`）。
    pub fn code(self) -> &'static str {
        match self {
            Capability::MultiDisplay => "V01-C1",
            Capability::ColorMgmt => "V01-C2",
            Capability::Hdr => "V01-C3",
            Capability::VirtualDisplay => "V01-C4",
        }
    }

    /// 本能力落点层（1:1 或派生，见头注§二的对账表）。
    pub fn layers(self) -> &'static [Layer] {
        match self {
            Capability::MultiDisplay => &[Layer::Device],
            Capability::ColorMgmt => &[Layer::Color],
            Capability::Hdr => &[Layer::Hdr],
            // 虚拟显示派生归设备层：虚拟屏是显示目标的一种来源，无物理屏而已。
            Capability::VirtualDisplay => &[Layer::Device],
        }
    }

    /// 是否派生能力（非 1:1——落点层由裁决给出而非天然对应）。
    pub fn is_derived(self) -> bool {
        matches!(self, Capability::VirtualDisplay)
    }

    /// 本体主责条目（派生能力也必须有主责，否则成无主资源）。
    pub fn owner_item(self) -> &'static str {
        match self {
            Capability::MultiDisplay => "VE-F4402",
            Capability::ColorMgmt => "VE-F4403",
            Capability::Hdr => "VE-F4404",
            Capability::VirtualDisplay => "VE-F4410",
        }
    }

    /// 读屏单行。
    pub fn screen_line(self) -> String {
        format!(
            "能力 {}（{}）：落点 {}；主责 {}；{}",
            self.code(),
            self.zh(),
            self.layers()
                .iter()
                .map(|l| l.zh())
                .collect::<Vec<&str>>()
                .join("+"),
            self.owner_item(),
            if self.is_derived() { "派生能力" } else { "1:1 能力" }
        )
    }
}

/// 能力序（顺序即锚点职责定位列序）。
pub const CAPABILITY_ORDER: [Capability; CAPABILITY_COUNT] = Capability::ALL;

// ---------------------------------------------------------------------------
// 四、层间接口与冻结册（判据二 · 锚点错误路径「接口越权→冻结流程」）
// ---------------------------------------------------------------------------

/// ADR 状态。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AdrState {
    /// 提案：不可用于变更接口。
    Proposed,
    /// 已接受：唯一可用于变更接口的状态。
    Accepted,
    /// 已否决：不可用。
    Rejected,
}

impl AdrState {
    /// 中文名。
    pub fn zh(self) -> &'static str {
        match self {
            AdrState::Proposed => "提案",
            AdrState::Accepted => "已接受",
            AdrState::Rejected => "已否决",
        }
    }

    /// 是否可用于接口变更。
    pub fn usable(self) -> bool {
        matches!(self, AdrState::Accepted)
    }
}

/// ADR 记录（接口越权变更的唯一出口）。
#[derive(Clone, Debug)]
pub struct AdrRecord {
    /// ADR 号（自增，单源）。
    pub id: u64,
    /// 标题。
    pub title: String,
    /// 理由。
    pub rationale: String,
    /// 影响层（变更波及哪些层）。
    pub layers: Vec<Layer>,
    /// 状态。
    pub state: AdrState,
    /// 提出时的逻辑 tick。
    pub proposed_at: u64,
}

impl AdrRecord {
    /// 齐备性自检（标题/理由/影响层缺一即不合格）。
    pub fn is_complete(&self) -> bool {
        !self.title.trim().is_empty() && !self.rationale.trim().is_empty() && !self.layers.is_empty()
    }

    /// 读屏单行。
    pub fn screen_line(&self) -> String {
        format!(
            "ADR {}（{}）：{}；理由 {}；影响层 {}",
            self.id,
            self.state.zh(),
            self.title,
            self.rationale,
            self.layers
                .iter()
                .map(|l| l.zh())
                .collect::<Vec<&str>>()
                .join("、")
        )
    }
}

/// 层间接口规格（冻结对象——冻结是本域开工的硬闸）。
#[derive(Clone, Debug)]
pub struct LayerInterface {
    /// 上游层。
    pub from: Layer,
    /// 下游层（必为 [`Layer::downstream`]）。
    pub to: Layer,
    /// 接口码（对外引用，如 `V01-IF2`）。
    pub code: &'static str,
    /// 中文职责名。
    pub duty_zh: &'static str,
    /// 输入契约（吃什么）。
    pub input: &'static str,
    /// 输出契约（吐什么）。
    pub output: &'static str,
    /// 失败策略（锚点降级矩阵落到本接口的那一格）。
    pub on_failure: &'static str,
    /// 复杂度声明（对应 [`COMPLEXITY_DOC`] 编号）。
    pub complexity: &'static str,
    /// 下游消费方（跨批对接点）。
    pub consumers: &'static str,
    /// 本接口的**不做清单**。
    pub not_mine: &'static str,
    /// 修订后的声明正文（`None` = 未修订，用 [`LayerInterface::declared_text`]
    /// 从静态字段派生）。
    ///
    /// 为什么单独存：静态字段是 `&'static str`（编译期常量表），而修订是运行期
    /// 动作。存覆盖值而不是就地改常量表，是为了让「冻结哈希 = 声明正文实算」
    /// 这条对拍恒等式在任何状态下都成立——改了就重算，不存在改了不算的窗口。
    pub revised_text: Option<String>,
    /// 声明内容哈希（由 [`fnv1a64_hex`] 对声明正文实算，**不是手写常量**）。
    pub declared_hash: String,
}

impl LayerInterface {
    /// 接口契约齐备性自检（缺一即不合格）。
    pub fn is_complete(&self) -> bool {
        !self.code.trim().is_empty()
            && !self.duty_zh.trim().is_empty()
            && !self.input.trim().is_empty()
            && !self.output.trim().is_empty()
            && !self.on_failure.trim().is_empty()
            && !self.complexity.trim().is_empty()
            && !self.consumers.trim().is_empty()
            && !self.not_mine.trim().is_empty()
            && self.declared_hash.len() == HASH_HEX_LEN
    }

    /// 上游层 == 下游层的直接上游（层链闭合性）。
    pub fn is_adjacent(&self) -> bool {
        self.from.downstream() == Some(self.to)
    }

    /// 声明正文的规范化串（哈希的输入——**唯一真值**，改哈希口径必须走 ADR）。
    ///
    /// 有修订值时以修订值为准：修订走 ADR 且重基计数自增，所以覆盖值本身
    /// 也是有出处的状态，不存在「悄悄改了就没人知道」的窗口。
    pub fn declared_text(&self) -> String {
        if let Some(t) = self.revised_text.as_ref() {
            return t.clone();
        }
        format!(
            "{}|{}|{}|{}|{}|{}|{}|{}",
            self.code,
            self.from.code(),
            self.to.code(),
            self.input,
            self.output,
            self.on_failure,
            self.consumers,
            self.not_mine
        )
    }

    /// 是否带 ADR 修订（读屏与机检用——冻结态与已修订态要能区分）。
    pub fn is_revised(&self) -> bool {
        self.revised_text.is_some()
    }

    /// 读屏单行。
    pub fn screen_line(&self) -> String {
        format!(
            "接口 {}（{} → {}，{}）：吃 {}；吐 {}；失败 {}；哈希 {}",
            self.code,
            self.from.zh(),
            self.to.zh(),
            self.duty_zh,
            self.input,
            self.output,
            self.on_failure,
            self.declared_hash
        )
    }
}

/// 标准层间接口表（**三条相邻边**——由 [`Layer::downstream`] 派生，非手写数字）。
///
/// 声明哈希**实算构造**：每条的 `declared_hash` 由 [`LayerInterface::declared_text`]
/// 实算，`standard_interfaces()` 如此构造，所以「哈希对账不是看一眼填没填」。
pub fn standard_interfaces() -> Vec<LayerInterface> {
    // 元组序：from, to, code, duty, input, output, on_failure, complexity,
    // consumers, not_mine（与下方解构顺序逐位对应，改一处须改两处）。
    let seeds: [(Layer, Layer, &str, &str, &str, &str, &str, &str, &str, &str); 3] = [
        (
            Layer::Device,
            Layer::Color,
            "V01-IF1",
            "设备事实交付：台账与校准引用",
            "显示器台账（屏数/位次/主副/能力表）+ 每屏校准引用",
            "可供色彩变换消费的设备事实集（含HDR 能力声明）",
            "枚举缺失 -> 降级轮询并标注；校准档缺失 -> 走缺省并标注不静默",
            "C2 O(接口数)",
            "VE-F4403 色彩引擎",
            "不做色彩变换本身，不做拓扑关系（拓扑归VE-F4421）",
        ),
        (
            Layer::Color,
            Layer::Hdr,
            "V01-IF2",
            "色彩事实交付：变换结果与契约版本戳",
            "色彩变换结果帧 + 色彩空间/位深声明 + 契约版本戳",
            "HDR 层可消费的色彩事实（含变换戳，防双重变换）",
            "配置解析失败 -> 缺省 sRGB 路径并标注；缓存失真 -> 版本戳失效",
            "C2 O(接口数)",
            "VE-F4404 HDR 管线",
            "不做色调映射本身，不做矩阵库（矩阵归VE-F4406）",
        ),
        (
            Layer::Hdr,
            Layer::App,
            "V01-IF3",
            "显示帧交付：色调映射结果与混合态",
            "色调映射后显示帧 + HDR 元数据抽象层输出 + 契约版本戳",
            "应用层可直接出帧的显示帧（契约版本戳必带）",
            "元数据缺失 -> 静态映射回退并标注；混合过曝 -> 亮度钳制",
            "C2 O(接口数)",
            "VE-F4408 意图协商/ VE-F4411 色彩无障碍",
            "不做应用出帧与意图协商（协商归VE-F4408）",
        ),
    ];
    let mut out = Vec::new();
    for (from, to, code, duty, input, output, on_failure, cx, consumers, not_mine) in seeds {
        let mut it = LayerInterface {
            from,
            to,
            code,
            duty_zh: duty,
            input,
            output,
            on_failure,
            complexity: cx,
            consumers,
            not_mine,
            revised_text: None,
            declared_hash: String::new(),
        };
        it.declared_hash = fnv1a64_hex(it.declared_text().as_bytes());
        out.push(it);
    }
    out
}

/// 接口冻结册（判据二的载体）。
///
/// **默认阻断，不是默认放行**：构造时逐条校验，残缺/非相邻/重复码一律拒。
#[derive(Clone, Debug)]
pub struct InterfaceFreezeLedger {
    /// 当前接口集。
    interfaces: Vec<LayerInterface>,
    /// 冻结版本。
    pub interface_version: String,
    /// ADR 账。
    adrs: Vec<AdrRecord>,
    /// 已重基次数（**只增不减**——变更次数本身是审计证据）。
    rebases: usize,
}

impl InterfaceFreezeLedger {
    /// 冻结册构造（逐条校验：契约齐备 + 相邻闭合 + 无重复码 + 不超上限）。
    pub fn new(
        interfaces: Vec<LayerInterface>,
        interface_version: &str,
    ) -> Result<Self, ConsistencyError> {
        if interfaces.is_empty() {
            return Err(ConsistencyError::new(
                E_INTERFACE_EMPTY,
                "接口冻结册建立被拒：接口集为空",
                "空接口集意味着四层之间没有任何合法通道，后续层全部悬空",
                "按 Layer::downstream 逐边构造标准接口册（standard_interfaces）",
                "V 域架构维护方",
            ));
        }
        if interfaces.len() > MAX_INTERFACES {
            return Err(ConsistencyError::new(
                E_INTERFACE_CAP,
                "接口冻结册建立被拒：接口数超上限",
                &format!(
                    "接口数 {} 超过上限 {}；四层相邻边只有 3 条，多出来的必是非相邻连线",
                    interfaces.len(),
                    MAX_INTERFACES
                ),
                "删除非相邻层连线；确需增层须走 ADR 并改四层本身（那会改判据一）",
                "V 域架构维护方",
            ));
        }
        let mut seen: Vec<&str> = Vec::new();
        for it in interfaces.iter() {
            if !it.is_complete() {
                return Err(ConsistencyError::new(
                    E_INTERFACE_INCOMPLETE,
                    "接口冻结册建立被拒：接口契约残缺",
                    &format!("接口 {} 缺字段；残缺契约无法对拍", it.code),
                    "补齐吃/吐/失败策略/复杂度/消费方/不做清单与实算哈希",
                    "V 域架构维护方",
                ));
            }
            if !it.is_adjacent() {
                return Err(ConsistencyError::new(
                    E_INTERFACE_NOT_ADJACENT,
                    "接口冻结册建立被拒：非相邻层连线",
                    &format!(
                        "接口 {} 连接 {} → {}，但 {} 的直接下游是 {:?}",
                        it.code,
                        it.from.zh(),
                        it.to.zh(),
                        it.from.zh(),
                        it.from.downstream().map(|l| l.zh())
                    ),
                    "跨层直连必须经中间层的正式接口；无通道即无合法路径",
                    "V 域架构维护方",
                ));
            }
            if seen.contains(&it.code) {
                return Err(ConsistencyError::new(
                    E_INTERFACE_DUP,
                    "接口冻结册建立被拒：接口码重复",
                    &format!("接口码 {} 重复登记", it.code),
                    "接口码是下游引用的键，重复会让引用指向不明",
                    "V 域架构维护方",
                ));
            }
            seen.push(it.code);
        }
        Ok(InterfaceFreezeLedger {
            interfaces,
            interface_version: interface_version.to_string(),
            adrs: Vec::new(),
            rebases: 0,
        })
    }

    /// 只读遍历。
    pub fn iter(&self) -> impl Iterator<Item = &LayerInterface> {
        self.interfaces.iter()
    }

    /// 可变遍历（供自检构造「改了声明却不重算在位哈希」这类负例）。
    ///
    /// 存在的理由：这类负例必须真的改坏字段才能验证对拍抓得住；没有这个口，
    /// 对拍就永远只被正样本喂，测不出它到底能不能发现问题。
    pub fn interfaces_mut(&mut self) -> &mut Vec<LayerInterface> {
        &mut self.interfaces
    }

    /// 接口数。
    pub fn len(&self) -> usize {
        self.interfaces.len()
    }

    /// 是否为空。
    pub fn is_empty(&self) -> bool {
        self.interfaces.is_empty()
    }

    /// 取某条相邻边（`None` = 该边不存在）。
    pub fn edge(&self, from: Layer, to: Layer) -> Option<&LayerInterface> {
        self.interfaces.iter().find(|i| i.from == from && i.to == to)
    }

    /// 摘除以某层为上游的全部边（返回摘除条数）。
    ///
    /// 存在的理由是**对拍要能看见缺边**：缺边在逐边遍历里不可见，只靠
    /// 「四层应有三条边」这个数来判会漏掉「删了一条又补了一条非相邻的」。
    /// 让调用方能真正把边摘掉，缺边才会暴露成可定位的失配。
    pub fn detach_from(&mut self, layer: Layer) -> usize {
        let before = self.interfaces.len();
        self.interfaces.retain(|i| i.from != layer);
        before - self.interfaces.len()
    }

    /// 追加一条边（供越层连线等负例构造；正式路径走 ADR）。
    pub fn push_interface(&mut self, it: LayerInterface) {
        self.interfaces.push(it);
    }

    /// 冻结摘要哈希（双签闸第一件——层间接口冻结态的唯一指纹）。
    pub fn freeze_digest(&self) -> String {
        let mut text = String::from(INTERFACE_VERSION);
        for it in self.interfaces.iter() {
            text.push('|');
            text.push_str(it.code);
            text.push('=');
            text.push_str(&it.declared_hash);
        }
        fnv1a64_hex(text.as_bytes())
    }

    /// 提 ADR（提案态）。
    pub fn propose_adr(&mut self, title: &str, rationale: &str, layers: &[Layer]) -> u64 {
        let id = self.adrs.len() as u64 + 1;
        self.adrs.push(AdrRecord {
            id,
            title: title.to_string(),
            rationale: rationale.to_string(),
            layers: layers.to_vec(),
            state: AdrState::Proposed,
            proposed_at: id,
        });
        id
    }

    /// 接受 ADR。
    pub fn accept_adr(&mut self, id: u64) -> Result<(), ConsistencyError> {
        match self.adrs.iter_mut().find(|a| a.id == id) {
            Some(a) => {
                if !a.is_complete() {
                    return Err(ConsistencyError::new(
                        E_ADR_INCOMPLETE,
                        "ADR接受被拒：记录残缺",
                        &format!("ADR {} 缺标题/理由/影响层", id),
                        "补齐标题、理由与影响层再接受；残缺 ADR 不能作为变更依据",
                        "V 域架构维护方",
                    ));
                }
                a.state = AdrState::Accepted;
                Ok(())
            }
            None => Err(ConsistencyError::new(
                E_ADR_NOT_FOUND,
                "ADR接受被拒：编号不存在",
                &format!("ADR {} 不在账内", id),
                "用 propose_adr 先提 ADR 再接受；不接受不存在的编号",
                "V 域架构维护方",
            )),
        }
    }

    /// 否决 ADR。
    pub fn reject_adr(&mut self, id: u64) -> Result<(), ConsistencyError> {
        match self.adrs.iter_mut().find(|a| a.id == id) {
            Some(a) => {
                a.state = AdrState::Rejected;
                Ok(())
            }
            None => Err(ConsistencyError::new(
                E_ADR_NOT_FOUND,
                "ADR否决被拒：编号不存在",
                &format!("ADR {} 不在账内", id),
                "核对 ADR 号；否决不存在的编号会让账目虚增",
                "V 域架构维护方",
            )),
        }
    }

    /// ADR 只读遍历。
    pub fn adrs(&self) -> impl Iterator<Item = &AdrRecord> {
        self.adrs.iter()
    }

    /// **接口越权变更的唯一出口**（锚点错误路径「接口越权→冻结流程」）。
    ///
    /// 拒四事：接口不存在、ADR 不存在、ADR 提案/否决态、变更后必须升版。
    /// 通过则**重算哈希并自增重基计数**——就地改冻结值而不留计数等于没冻结。
    pub fn revise(
        &mut self,
        code: &str,
        new_text: String,
        adr_id: u64,
    ) -> Result<String, ConsistencyError> {
        let idx = match self.interfaces.iter().position(|i| i.code == code) {
            Some(i) => i,
            None => {
                return Err(ConsistencyError::new(
                    E_INTERFACE_UNKNOWN,
                    "接口变更被拒：接口不存在",
                    &format!("接口码 {} 不在冻结册内", code),
                    "按 standard_interfaces 的三条边核对接口码；新增接口须走 ADR 正式增层",
                    "V 域架构维护方",
                ))
            }
        };
        let adr_ok = self
            .adrs
            .iter()
            .find(|a| a.id == adr_id)
            .map(|a| a.state.usable())
            .unwrap_or(false);
        if !adr_ok {
            let detail = match self.adrs.iter().find(|a| a.id == adr_id) {
                Some(a) => format!("ADR {} 当前为{}态", a.id, a.state.zh()),
                None => format!("ADR {} 不在账内", adr_id),
            };
            return Err(ConsistencyError::new(
                E_ADR_REQUIRED,
                "接口变更被拒：须先有已接受 ADR",
                &format!("{}；改已冻结接口不走 ADR 就是绕过冻结流程", detail),
                "先 propose_adr 再 accept_adr，然后用已接受 ADR 号调用 revise",
                "V 域架构维护方",
            ));
        }
        if new_text.trim().is_empty() {
            return Err(ConsistencyError::new(
                E_INTERFACE_EMPTY_TEXT,
                "接口变更被拒：新声明正文为空",
                "空正文会让对拍恒真——哈希对空串必然一致，等于冻结失效",
                "给出完整的新声明正文（沿用 declared_text 的八段格式）",
                "V 域架构维护方",
            ));
        }
        if new_text == self.interfaces[idx].declared_text() {
            return Err(ConsistencyError::new(
                E_INTERFACE_NO_CHANGE,
                "接口变更被拒：新正文与冻结正文逐字相同",
                "无实质变更却消耗一次 ADR 与一个版本号；变更次数是审计证据，不许注水",
                "确有变更才走 revise；无变更直接返回原哈希即可",
                "V 域架构维护方",
            ));
        }
        let hash = fnv1a64_hex(new_text.as_bytes());
        self.interfaces[idx].revised_text = Some(new_text);
        self.interfaces[idx].declared_hash = hash.clone();
        self.rebases += 1;
        Ok(hash)
    }

    /// 已重基次数（只增不减的审计证据）。
    pub fn rebase_count(&self) -> usize {
        self.rebases
    }

    /// 读屏摘要。
    pub fn screen_text(&self) -> String {
        let mut s = format!(
            "层间接口冻结 {}，共{} 条边，重基 {} 次，冻结摘要 {}。\n",
            self.interface_version,
            self.interfaces.len(),
            self.rebases,
            self.freeze_digest()
        );
        for it in self.interfaces.iter() {
            s.push_str(&it.screen_line());
            s.push('\n');
        }
        s
    }
}

// ---------------------------------------------------------------------------
// 五、承接面落地表（判据三 · 锚点错误路径「承接缺源→回溯移交包」）
// ---------------------------------------------------------------------------

/// 承接角色。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AcceptanceRole {
    /// 承接点：锚点 F4395 交接面点名的三件，必须在本域落地并对账。
    HandoffPoint,
    /// 继承位：已登记但落地义务不在本域（只登记，不落地，不判红）。
    Inherited,
}

impl AcceptanceRole {
    /// 中文名。
    pub fn zh(self) -> &'static str {
        match self {
            AcceptanceRole::HandoffPoint => "承接点",
            AcceptanceRole::Inherited => "继承位",
        }
    }

    /// 是否必须在 V 域总架构内落地。
    pub fn must_land_in_arch(self) -> bool {
        matches!(self, AcceptanceRole::HandoffPoint)
    }
}

/// 承接条目。
#[derive(Clone, Debug)]
pub struct AcceptanceEntry {
    /// 条目码（如 `V01-SRC-CONTRAST`）。
    pub code: String,
    /// 源域（恒为 `U`——移交包来自 U 域）。
    pub source_domain: &'static str,
    /// 源条目号（恒为 [`U_PACKAGE_ITEM`]，回溯目的地）。
    pub source_item: String,
    /// 承接内容（入显示契约源的正文）。
    pub content: String,
    /// 角色。
    pub role: AcceptanceRole,
    /// 承接自（交接面描述）。
    pub carried_from: String,
    /// 源内容哈希（实算）。
    pub source_hash: String,
    /// 是否已在 V 域落地。
    pub landed: bool,
    /// 是否已对账（落地且哈希一致）。
    pub reconciled: bool,
}

impl AcceptanceEntry {
    /// 齐备性自检。
    pub fn is_complete(&self) -> bool {
        !self.code.trim().is_empty()
            && !self.source_item.trim().is_empty()
            && !self.content.trim().is_empty()
            && !self.carried_from.trim().is_empty()
            && self.source_hash.len() == HASH_HEX_LEN
    }

    /// 源哈希是否与内容实算一致（对拍的唯一依据）。
    pub fn hash_matches(&self) -> bool {
        self.source_hash == fnv1a64_hex(self.content.as_bytes())
    }

    /// 是否已落地对账。
    pub fn is_landed_for_arch(&self) -> bool {
        self.landed && self.reconciled && self.hash_matches()
    }

    /// 读屏单行。
    pub fn screen_line(&self) -> String {
        format!(
            "承接 {}（{} · {}）：{}；源 {}；落地 {}；对账 {}；源哈希 {}",
            self.code,
            self.role.zh(),
            self.source_domain,
            self.content,
            self.source_item,
            if self.landed { "是" } else { "否" },
            if self.reconciled { "是" } else { "否" },
            self.source_hash
        )
    }
}

/// 回溯单（锚点错误路径「承接缺源→回溯移交包」的落位）。
#[derive(Clone, Debug)]
pub struct TraceBack {
    /// 缺源的条目码。
    pub code: String,
    /// 缺失类别。
    pub kind: DefectKind,
    /// 回溯目的地（恒为 [`U_PACKAGE_ITEM`]）。
    pub trace_to: &'static str,
    /// 应向移交包核对的具体件名。
    pub check_item: String,
    /// 下一步。
    pub advice: &'static str,
}

impl TraceBack {
    /// 读屏单行（缺源必须能念出来——异常零静默）。
    pub fn screen_line(&self) -> String {
        format!(
            "回溯 {}：{}；回溯至 {} 的「{}」件；下一步 {}",
            self.code,
            self.kind.zh(),
            self.trace_to,
            self.check_item,
            self.advice
        )
    }
}

/// 承接缺陷类别。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DefectKind {
    /// 件缺：移交包该件未到。
    MissingItem,
    /// 哈希不符：件到了但内容对不上。
    HashMismatch,
    /// 未落地：件到了但 V 域没接。
    NotLanded,
}

impl DefectKind {
    /// 中文名。
    pub fn zh(self) -> &'static str {
        match self {
            DefectKind::MissingItem => "件缺",
            DefectKind::HashMismatch => "哈希不符",
            DefectKind::NotLanded => "未落地",
        }
    }

    /// 该类缺陷应向移交包的哪一件核对。
    pub fn check_item(self) -> &'static str {
        match self {
            DefectKind::MissingItem => "清账",
            DefectKind::HashMismatch => "无障碍",
            DefectKind::NotLanded => "交接面",
        }
    }
}

/// 承接面落地表。
#[derive(Clone, Debug)]
pub struct AcceptanceLedger {
    entries: Vec<AcceptanceEntry>,
}

impl AcceptanceLedger {
    /// 空承接表（**未承接任何源**——默认阻断，不是默认放行）。
    pub fn new() -> Self {
        AcceptanceLedger { entries: Vec::new() }
    }

    /// 登记承接条目。
    ///
    /// 拒三事：条目残缺、源条目号不是合法 `VE-F####`、源码重复。
    /// 源码重复是最容易犯的一种错——两个域都登记「交互词典」，最后没人说得清
    /// 契约文本到底以哪一份为准。
    pub fn register(&mut self, e: AcceptanceEntry) -> Result<(), ConsistencyError> {
        if !e.is_complete() || !is_valid_item_id(&e.source_item) {
            return Err(ConsistencyError::new(
                E_ACCEPTANCE_INCOMPLETE,
                "承接登记被拒：条目残缺",
                &format!(
                    "承接源 {} 缺字段或条目号非法（须 VE-F####，实得 {:?}）",
                    e.code, e.source_item
                ),
                "补齐 code/source_item/content/carried_from 与实算哈希",
                "承接方",
            ));
        }
        if self.entries.iter().any(|x| x.code == e.code) {
            return Err(ConsistencyError::new(
                E_ACCEPTANCE_DUP,
                "承接登记被拒：源码重复",
                &format!("承接源码 {} 已登记；契约源重复登记会让单源失效", e.code),
                "更新既有条目的来源哈希并走 ADR；不同源请用不同源码",
                "承接方",
            ));
        }
        if self.entries.len() >= MAX_ACCEPTANCE_SOURCES {
            return Err(ConsistencyError::new(
                E_ACCEPTANCE_CAP,
                "承接登记被拒：承接表已满",
                &format!(
                    "承接表 {} 条达到上限 {}",
                    self.entries.len(),
                    MAX_ACCEPTANCE_SOURCES
                ),
                "先归档已移交的源，或按 ADR 提升 MAX_ACCEPTANCE_SOURCES",
                "V 域架构维护方",
            ));
        }
        self.entries.push(e);
        Ok(())
    }

    /// 条目数。
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// 是否为空。
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// 只读遍历。
    pub fn iter(&self) -> impl Iterator<Item = &AcceptanceEntry> {
        self.entries.iter()
    }

    /// 按码取条目。
    pub fn entry(&self, code: &str) -> Option<&AcceptanceEntry> {
        self.entries.iter().find(|e| e.code == code)
    }

    /// 按码取可变条目（供落地对账与自检构造负例）。
    pub fn entry_mut(&mut self, code: &str) -> Option<&mut AcceptanceEntry> {
        self.entries.iter_mut().find(|e| e.code == code)
    }

    /// 摘除某条目（返回是否摘到；缺源模拟用）。
    ///
    /// 存在的理由与 [`InterfaceFreezeLedger::detach_from`] 同：缺件在「已登记
    /// 条目」的遍历里看不见，必须让调用方能真正摘掉才测得出回溯逻辑。
    pub fn remove(&mut self, code: &str) -> bool {
        let before = self.entries.len();
        self.entries.retain(|e| e.code != code);
        before != self.entries.len()
    }

    /// 某角色的条目数（O(角色数)，角色表定长）。
    pub fn role_count(&self, role: AcceptanceRole) -> usize {
        self.entries.iter().filter(|e| e.role == role).count()
    }

    /// 承接点是否齐备且全落地（判据三）。
    ///
    /// 三个承接点是锚点 F4395 交接面**明文点名**的（对比度契约 / 色弱映射 /
    /// 交互词典），少一件即未落地——「三件」是锚点明文数量，不是「至少一件」。
    pub fn handoff_points_landed(&self) -> bool {
        let pts: Vec<&AcceptanceEntry> = self
            .entries
            .iter()
            .filter(|e| e.role == AcceptanceRole::HandoffPoint)
            .collect();
        if pts.len() < HANDOFF_POINT_COUNT {
            return false;
        }
        pts.iter().all(|e| e.is_landed_for_arch())
            && pts.iter().all(|e| e.source_domain == "U")
            && pts.iter().all(|e| e.source_item == U_PACKAGE_ITEM)
    }

    /// 缺陷清单（含回溯单——缺源必须给出路）。
    pub fn defects(&self) -> Vec<TraceBack> {
        let mut out = Vec::new();
        for e in self.entries.iter() {
            if !e.hash_matches() {
                out.push(TraceBack {
                    code: e.code.clone(),
                    kind: DefectKind::HashMismatch,
                    trace_to: U_PACKAGE_ITEM,
                    check_item: DefectKind::HashMismatch.check_item().to_string(),
                    advice: "以移交包冻结哈希为准重算并重封；不得就地改V 域侧哈希",
                });
            }
            if e.role.must_land_in_arch() && !(e.landed && e.reconciled) {
                out.push(TraceBack {
                    code: e.code.clone(),
                    kind: DefectKind::NotLanded,
                    trace_to: U_PACKAGE_ITEM,
                    check_item: DefectKind::NotLanded.check_item().to_string(),
                    advice: "把该承接点入显示契约源并完成落地对账，再申请域开工闸",
                });
            }
        }
        // 三件点名清单逐件核——缺件在已登记条目里看不见。
        for code in REQUIRED_HANDOFF_CODES {
            if self.entry(code).is_none() {
                out.push(TraceBack {
                    code: code.to_string(),
                    kind: DefectKind::MissingItem,
                    trace_to: U_PACKAGE_ITEM,
                    check_item: DefectKind::MissingItem.check_item().to_string(),
                    advice: "向 U 域移交包索取该件；缺件即拒收，不许以本地推测补造",
                });
            }
        }
        out
    }

    /// 指定码的缺源回溯单（`None` = 该码无缺陷）。
    pub fn trace_back(&self, code: &str) -> Option<TraceBack> {
        self.defects().into_iter().find(|t| t.code == code)
    }

    /// 读屏摘要。
    pub fn screen_text(&self) -> String {
        let mut s = format!(
            "承接面落地表：{} 条（承接点 {} 条 / 继承位 {} 条），回溯目的地 {}。\n",
            self.entries.len(),
            self.role_count(AcceptanceRole::HandoffPoint),
            self.role_count(AcceptanceRole::Inherited),
            U_PACKAGE_ITEM
        );
        for e in self.entries.iter() {
            s.push_str(&e.screen_line());
            s.push('\n');
        }
        let defects = self.defects();
        if defects.is_empty() {
            s.push_str("承接面缺陷：无。\n");
        } else {
            s.push_str(&format!("承接面缺陷：{} 项。\n", defects.len()));
            for t in defects.iter() {
                s.push_str(&t.screen_line());
                s.push('\n');
            }
        }
        s
    }
}

/// 锚点点名的三个承接点条目码（缺一即拒收——明文数量，不是「至少一件」）。
pub const REQUIRED_HANDOFF_CODES: [&str; HANDOFF_POINT_COUNT] =
    ["V01-SRC-CONTRAST", "V01-SRC-CVD", "V01-SRC-DICT"];

/// 标准承接表（**三承接点已落地对账的正样本**）。
///
/// 三件全部来自 `VE-F4395` 交接面，落点统一为**显示契约源**（色彩层持有）。
pub fn standard_acceptance() -> AcceptanceLedger {
    let mut l = AcceptanceLedger::new();
    let seeds: [(&str, &str, &str); HANDOFF_POINT_COUNT] = [
        (
            "V01-SRC-CONTRAST",
            "对比度契约：文本与界面元素的目标对比度下限与动态增强规则",
            "F4395 无障碍件",
        ),
        (
            "V01-SRC-CVD",
            "色弱映射契约：红绿/蓝黄/全色弱三型映射曲线的入约口径",
            "F4395 无障碍件",
        ),
        (
            "V01-SRC-DICT",
            "交互词典：显示相关词条（跨屏拖拽/贴边/浮层跟随等）术语口径",
            "F4395 词典件",
        ),
    ];
    for (code, content, check_item) in seeds.iter() {
        let mut e = AcceptanceEntry {
            code: code.to_string(),
            source_domain: "U",
            source_item: U_PACKAGE_ITEM.to_string(),
            content: content.to_string(),
            role: AcceptanceRole::HandoffPoint,
            carried_from: format!("{} 交接面·{}", U_PACKAGE_ITEM, check_item),
            source_hash: String::new(),
            landed: true,
            reconciled: true,
        };
        e.source_hash = fnv1a64_hex(e.content.as_bytes());
        l.register(e).expect("标准承接登记：三承接点应齐备且哈希实算");
    }
    l
}

// ---------------------------------------------------------------------------
// 六、色准硬线（判据四 · 域本色核心，见头注§三）
// ---------------------------------------------------------------------------

/// 色准契约种类（锚点：对比度与色弱映射为一等契约）。
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum ColorContractKind {
    /// 对比度契约。
    Contrast,
    /// 色弱映射契约。
    CvdMapping,
}

impl ColorContractKind {
    /// 两类全集（顺序即锚点列序）。
    pub const ALL: [ColorContractKind; 2] =
        [ColorContractKind::Contrast, ColorContractKind::CvdMapping];

    /// 中文名（读屏播报用）。
    pub fn zh(self) -> &'static str {
        match self {
            ColorContractKind::Contrast => "对比度契约",
            ColorContractKind::CvdMapping => "色弱映射契约",
        }
    }

    /// 契约码（对外引用，如 `V01-K1`）。
    pub fn code(self) -> &'static str {
        match self {
            ColorContractKind::Contrast => "V01-K1",
            ColorContractKind::CvdMapping => "V01-K2",
        }
    }

    /// **最低契约版本**：低于此版本的应用出帧一律拒收。
    ///
    /// 色弱映射的最低版本高于对比度——映射曲线是 F4411 深化后才成型的口径，
    /// 早于该版本的应用不知道「映射只改映射不改信息」这条保真要求。
    pub fn min_version(self) -> u32 {
        match self {
            ColorContractKind::Contrast => 1,
            ColorContractKind::CvdMapping => 2,
        }
    }

    /// 本体主责条目（契约正文的实现归谁——本域只保「在册且唯一」）。
    pub fn owner_item(self) -> &'static str {
        match self {
            ColorContractKind::Contrast => "VE-F4411",
            ColorContractKind::CvdMapping => "VE-F4411",
        }
    }

    /// 读屏单行。
    pub fn screen_line(self) -> String {
        format!(
            "色准契约 {}（{}）：最低版本 {}；本体 {}",
            self.code(),
            self.zh(),
            self.min_version(),
            self.owner_item()
        )
    }
}

/// 色准契约条目（一等契约的登记形态）。
#[derive(Clone, Debug)]
pub struct ColorContractEntry {
    /// 契约种类。
    pub kind: ColorContractKind,
    /// 标题。
    pub title: String,
    /// 持有层（**必须**是 [`Layer::Color`]）。
    pub owner_layer: Layer,
    /// 是否一等契约（硬线要求恒为真）。
    pub first_class: bool,
    /// 是否登记为延后（硬线不许延后）。
    pub deferred: bool,
    /// 是否申请豁免（硬线不许豁免）。
    pub waived: bool,
    /// 最低契约版本。
    pub min_version: u32,
    /// 是否已在V 域落地。
    pub landed: bool,
    /// 声明哈希（实算）。
    pub declared_hash: String,
}

impl ColorContractEntry {
    /// 构造一条一等契约（`first_class=true`、`deferred=false`、`waived=false`）。
    pub fn primary(kind: ColorContractKind, title: &str) -> Self {
        let mut e = ColorContractEntry {
            kind,
            title: title.to_string(),
            owner_layer: Layer::Color,
            first_class: true,
            deferred: false,
            waived: false,
            min_version: kind.min_version(),
            landed: true,
            declared_hash: String::new(),
        };
        e.declared_hash = fnv1a64_hex(e.contract_text().as_bytes());
        e
    }

    /// 声明正文的规范化串（哈希输入——唯一真值）。
    pub fn contract_text(&self) -> String {
        format!(
            "{}|{}|{}|{}|{}",
            self.kind.code(),
            self.owner_layer.code(),
            self.title,
            self.min_version,
            self.min_version // 版本戳随契约一同盖戳，戳变则哈希变
        )
    }

    /// 齐备性自检。
    pub fn is_complete(&self) -> bool {
        !self.title.trim().is_empty() && self.declared_hash.len() == HASH_HEX_LEN
    }

    /// 哈希是否与正文一致（改标题却不重算哈希即漂移）。
    pub fn hash_matches(&self) -> bool {
        self.declared_hash == fnv1a64_hex(self.contract_text().as_bytes())
    }

    /// 是否满足硬线全条件。
    pub fn satisfies_hardline(&self) -> bool {
        self.first_class
            && !self.deferred
            && !self.waived
            && self.owner_layer.owns_color_contract()
            && self.landed
            && self.min_version >= self.kind.min_version()
            && self.is_complete()
            && self.hash_matches()
    }

    /// 读屏单行。
    pub fn screen_line(&self) -> String {
        format!(
            "色准契约 {}（{}）：持有 {}；一等 {}；延后 {}；豁免 {}；最低版本 {}；落地 {}；哈希 {}",
            self.kind.code(),
            self.kind.zh(),
            self.owner_layer.zh(),
            if self.first_class { "是" } else { "否" },
            if self.deferred { "是" } else { "否" },
            if self.waived { "是" } else { "否" },
            self.min_version,
            if self.landed { "是" } else { "否" },
            self.declared_hash
        )
    }
}

/// 色准硬线（判据四的载体）。
///
/// 四条禁止见头注§三：入契约层 / 一等 / 不豁免 / 不延后；外加消费侧盖戳义务。
#[derive(Clone, Debug, Default)]
pub struct AccuracyHardline {
    entries: Vec<ColorContractEntry>,
}

impl AccuracyHardline {
    /// 空硬线册（**默认阻断**：两类契约都没登记）。
    pub fn new() -> Self {
        AccuracyHardline { entries: Vec::new() }
    }

    /// **标准硬线册**（两类契约均已登记为色彩层一等契约并落地）。
    pub fn standard() -> Self {
        let mut h = AccuracyHardline::new();
        h.register(ColorContractEntry::primary(
            ColorContractKind::Contrast,
            "对比度契约：目标对比度下限 + 动态增强上限（不下涉内容语义）",
        ))
        .expect("对比度契约应可登记");
        h.register(ColorContractEntry::primary(
            ColorContractKind::CvdMapping,
            "色弱映射契约：三型映射曲线 + 信息保真（只改映射不改信息）",
        ))
        .expect("色弱映射契约应可登记");
        h
    }

    /// 登记色准契约条目。
    ///
    /// 拒四事：非一等、非色彩层持有、申请豁免、登记为延后；另加种类重复与
    /// 版本低于该类最低版本。**豁免与延后一律拒**——它们正是硬线要防的写法。
    pub fn register(&mut self, e: ColorContractEntry) -> Result<(), ConsistencyError> {
        if !e.first_class {
            return Err(ConsistencyError::new(
                E_ACCURACY_NOT_FIRST_CLASS,
                "色准契约登记被拒：不是一等契约",
                &format!("{} 被声明为非一等；域本色要求对比度与色弱映射是一等契约", e.kind.zh()),
                "把first_class 置真；若确有例外，走 ADR 并留痕，不得默认降级为二等",
                "V 域架构维护方",
            ));
        }
        if !e.owner_layer.owns_color_contract() {
            return Err(ConsistencyError::new(
                E_ACCURACY_WRONG_OWNER,
                "色准契约登记被拒：持有层不是色彩层",
                &format!(
                    "{} 被声明由{} 持有；显示契约源的唯一持有者是色彩层",
                    e.kind.zh(),
                    e.owner_layer.zh()
                ),
                "把 owner_layer 改为色彩层；别层只持指针不留副本",
                "V 域架构维护方",
            ));
        }
        if e.waived {
            return Err(ConsistencyError::new(
                E_ACCURACY_WAIVED_FORBIDDEN,
                "色准契约登记被拒：硬线不接受豁免",
                &format!("{} 申请了豁免；色准即无障碍硬线，无豁免通道", e.kind.zh()),
                "撤掉豁免申请；确有技术阻塞时登记阻塞项与限期，不改硬线本身",
                "V 域架构维护方",
            ));
        }
        if e.deferred {
            return Err(ConsistencyError::new(
                E_ACCURACY_DEFERRED_FORBIDDEN,
                "色准契约登记被拒：硬线不接受延后",
                &format!("{} 被登记为延后；延后即等于没有", e.kind.zh()),
                "本期落地；确有依赖未就绪时登记阻塞项与限期，不写 deferred",
                "V 域架构维护方",
            ));
        }
        if e.min_version < e.kind.min_version() {
            // `next` 是 `&'static str`（与家族体例一致：错误建议必须是常驻文案，
            // 便于按码聚合统计）。具体版本数字放进 `why`，`next` 给固定出路。
            return Err(ConsistencyError::new(
                E_ACCURACY_VERSION_TOO_LOW,
                "色准契约登记被拒：契约版本低于最低要求",
                &format!(
                    "{} 声明版本 {}，低于该类最低版本 {}；最低版本见 ColorContractKind::min_version",
                    e.kind.zh(),
                    e.min_version,
                    e.kind.min_version()
                ),
                "按该契约种类的 min_version() 上调声明版本后重新登记",
                "V 域架构维护方",
            ));
        }
        if !e.is_complete() {
            return Err(ConsistencyError::new(
                E_ACCURACY_INCOMPLETE,
                "色准契约登记被拒：条目残缺",
                &format!("{} 缺标题或哈希", e.kind.zh()),
                "补齐标题并用 fnv1a64_hex 实算哈希；不接受手写哈希",
                "V 域架构维护方",
            ));
        }
        if self.entries.iter().any(|x| x.kind == e.kind) {
            return Err(ConsistencyError::new(
                E_ACCURACY_DUP,
                "色准契约登记被拒：种类重复",
                &format!("{} 已登记；同类契约只允许一条", e.kind.zh()),
                "更新既有条目并重算哈希；确需拆分须走 ADR 改判据",
                "V 域架构维护方",
            ));
        }
        self.entries.push(e);
        Ok(())
    }

    /// **豁免申请恒失败**（头注§三第 3 条的可执行形态）。
    ///
    /// 写成恒失败的函数而不是注释里的「不得豁免」：注释不会在评审里被验证，
    /// 函数会——而且失败时必须给出为什么不行与改走哪条路。
    ///
    /// **拒绝时不得改动册内任何状态**：早期版本在这里先把条目从册里删掉再
    /// 返回错误，结果是「一次被拒的豁免申请」会真的把色准契约从显示契约源
    /// 里抹掉——硬线被一个失败请求从内部攻破。正确的形态是**纯函数式拒绝**：
    /// 只读、只解释、只给路，不碰状态。
    pub fn request_waiver(
        &self,
        kind: ColorContractKind,
        reason: &str,
    ) -> Result<(), ConsistencyError> {
        Err(ConsistencyError::new(
            E_ACCURACY_WAIVED_FORBIDDEN,
            "色准硬线豁免申请被拒（无条件拒绝，本册状态未改动）",
            &format!(
                "{} 申请豁免，理由「{}」；色准即无障碍硬线，域本色条款不接受豁免通道",
                kind.zh(),
                reason
            ),
            "改走阻塞项登记：登记阻塞原因、责任条目与限期，硬线本身不动",
            "V 域架构维护方",
        ))
    }

    /// 只读遍历。
    pub fn iter(&self) -> impl Iterator<Item = &ColorContractEntry> {
        self.entries.iter()
    }

    /// 条目数。
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// 是否为空。
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// 按种类取条目。
    pub fn entry(&self, kind: ColorContractKind) -> Option<&ColorContractEntry> {
        self.entries.iter().find(|e| e.kind == kind)
    }

    /// 按种类取可变条目（供自检构造「已登记但被改坏」这类负例）。
    pub fn entry_mut(&mut self, kind: ColorContractKind) -> Option<&mut ColorContractEntry> {
        self.entries.iter_mut().find(|e| e.kind == kind)
    }

    /// 硬线是否齐备（两类契约都在册且都满足全条件）。
    pub fn satisfied(&self) -> bool {
        ColorContractKind::ALL
            .iter()
            .all(|k| self.entry(*k).map(|e| e.satisfies_hardline()).unwrap_or(false))
    }

    /// 列出违反硬线的条目（空 = 齐备）。
    pub fn violations(&self) -> Vec<ConsistencyError> {
        let mut out = Vec::new();
        for k in ColorContractKind::ALL.iter() {
            match self.entry(*k) {
                None => out.push(ConsistencyError::new(
                    E_ACCURACY_KIND_MISSING,
                    "色准硬线不齐备：契约种类缺登记",
                    &format!("{} 未在显示契约源登记", k.zh()),
                    "按 ColorContractKind::ALL 逐类登记为一等契约",
                    "V 域架构维护方",
                )),
                Some(e) if !e.owner_layer.owns_color_contract() => out.push(ConsistencyError::new(
                    E_ACCURACY_WRONG_OWNER,
                    "色准硬线被破：契约留在了非色彩层",
                    &format!("{} 由{} 持有", e.kind.zh(), e.owner_layer.zh()),
                    "契约文本只在色彩层；别层持指针",
                    "V 域架构维护方",
                )),
                Some(e) if !e.first_class => out.push(ConsistencyError::new(
                    E_ACCURACY_NOT_FIRST_CLASS,
                    "色准硬线被破：契约被降为二等",
                    &format!("{} first_class 为假", e.kind.zh()),
                    "恢复一等；域本色条款不允许二等",
                    "V 域架构维护方",
                )),
                Some(e) if e.deferred || e.waived => out.push(ConsistencyError::new(
                    E_ACCURACY_DEFERRED_FORBIDDEN,
                    "色准硬线被破：契约被延后或豁免",
                    &format!("{} deferred={} waived={}", e.kind.zh(), e.deferred, e.waived),
                    "撤掉延后与豁免；受阻时登记阻塞项与限期",
                    "V 域架构维护方",
                )),
                Some(e) if !e.landed => out.push(ConsistencyError::new(
                    E_ACCURACY_NOT_LANDED,
                    "色准硬线被破：契约未落地",
                    &format!("{} landed 为假", e.kind.zh()),
                    "把契约正文入显示契约源并对账",
                    "V 域架构维护方",
                )),
                Some(e) if e.min_version < e.kind.min_version() => out.push(ConsistencyError::new(
                    E_ACCURACY_VERSION_TOO_LOW,
                    "色准硬线被破：契约版本低于最低要求",
                    &format!(
                        "{} 版本 {} < 最低 {}",
                        e.kind.zh(),
                        e.min_version,
                        e.kind.min_version()
                    ),
                    "提升契约版本；旧版本应用出帧一律拒收",
                    "V 域架构维护方",
                )),
                Some(e) if !e.hash_matches() => out.push(ConsistencyError::new(
                    E_ACCURACY_HASH_DRIFT,
                    "色准硬线被破：契约声明哈希漂移",
                    &format!(
                        "{} 在位哈希 {} 与正文实算 {} 不一致",
                        e.kind.zh(),
                        e.declared_hash,
                        fnv1a64_hex(e.contract_text().as_bytes())
                    ),
                    "定位到被改字段后重算哈希；禁止就地改在位哈希",
                    "V 域架构维护方",
                )),
                Some(_) => {}
            }
        }
        out
    }

    /// 消费侧盖戳义务：某层出帧是否必须携带契约版本戳。
    ///
    /// 只有应用层（消费面层）有此义务——它是唯一把画面交给用户的一层。
    /// 色彩层产出时已盖章，HDR 层透传，应用层不得丢。
    pub fn stamp_required(layer: Layer) -> bool {
        layer.is_consumer_only()
    }

    /// 出帧前的盖戳校验（应用层调用）。
    ///
    /// `stamped_versions` 形如 `[(ColorContractKind, u32)]`；缺项或版本过低即拒。
    pub fn check_stamp(&self, layer: Layer, stamped: &[(ColorContractKind, u32)]) -> Result<(), ConsistencyError> {
        if !AccuracyHardline::stamp_required(layer) {
            return Ok(());
        }
        for k in ColorContractKind::ALL.iter() {
            let got = stamped.iter().find(|(kk, _)| kk == k).map(|(_, v)| *v);
            match got {
                None => {
                    return Err(ConsistencyError::new(
                        E_STAMP_REQUIRED,
                        "出帧被拒：缺色准契约版本戳",
                        &format!("{} 未随帧盖戳；硬线要求消费侧携带版本戳", k.zh()),
                        "由色彩层产出戳、HDR 层透传、应用层校验后再出帧",
                        "应用层",
                    ))
                }
                Some(v) if v < k.min_version() => {
                    return Err(ConsistencyError::new(
                        E_ACCURACY_VERSION_TOO_LOW,
                        "出帧被拒：契约版本戳低于最低要求",
                        &format!("{} 盖戳版本 {} < 最低 {}", k.zh(), v, k.min_version()),
                        "升级应用侧的色彩契约版本后再出帧",
                        "应用层",
                    ))
                }
                Some(_) => {}
            }
        }
        Ok(())
    }

    /// 读屏摘要。
    pub fn screen_text(&self) -> String {
        let mut s = format!("色准硬线：在册{} 类契约。\n", self.entries.len());
        for k in ColorContractKind::ALL.iter() {
            match self.entry(*k) {
                Some(e) => {
                    s.push_str(&e.screen_line());
                    s.push('\n');
                }
                None => {
                    s.push_str(&format!("色准契约 {}（{}）：未登记。\n", k.code(), k.zh()));
                }
            }
        }
        let v = self.violations();
        if v.is_empty() {
            s.push_str("色准硬线：齐备，无违反项。\n");
        } else {
            s.push_str(&format!("色准硬线：{} 项违反。\n", v.len()));
            for e in v.iter() {
                s.push_str(&e.screen_text());
                s.push('\n');
            }
        }
        s
    }
}

/// 禁含隐私的内容类字段记号（锚点「配置不含隐私」的落位）。
///
/// 显示配置里出现这些记号即拒：它们记的是**屏上看见了什么**，不是屏是什么。
/// 校准数据、设备指纹都不在此列——它们描述设备，不描述用户看了什么。
pub const PRIVACY_FORBIDDEN_TOKENS: [&str; 8] = [
    "pixel_data",
    "screen_content",
    "capture_buffer",
    "viewing_history",
    "window_title",
    "screenshot",
    "keyframe_content",
    "ocr_text",
];

// ---------------------------------------------------------------------------
// 七、判据（判据五：判据本身可追溯）
// ---------------------------------------------------------------------------

/// 判据（锚点原文五条）。
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Criterion {
    /// 判据一：四层（设备→色彩→HDR→应用，层链相邻闭合）。
    FourLayers,
    /// 判据二：接口冻结（三条相邻边冻结 + 越权变更走 ADR）。
    InterfaceFreeze,
    /// 判据三：承接落地（三承接点入显示契约源并全落地对账）。
    AcceptanceLanding,
    /// 判据四：色准硬线（对比度与色弱映射为一等契约，域本色）。
    AccuracyHardline,
    /// 判据五：判据本身（总纲自证可追溯）。
    Criterion,
}

impl Criterion {
    /// 五项判据全集（判据：一项不缺）。
    pub const CRITERIA: [Criterion; CRITERION_COUNT] = [
        Criterion::FourLayers,
        Criterion::InterfaceFreeze,
        Criterion::AcceptanceLanding,
        Criterion::AccuracyHardline,
        Criterion::Criterion,
    ];

    /// 判据中文名（读屏播报）。
    pub fn zh(self) -> &'static str {
        match self {
            Criterion::FourLayers => "四层",
            Criterion::InterfaceFreeze => "接口冻结",
            Criterion::AcceptanceLanding => "承接落地",
            Criterion::AccuracyHardline => "色准硬线",
            Criterion::Criterion => "判据",
        }
    }

    /// 判据码（对拍与台账引用）。
    pub fn code(self) -> &'static str {
        match self {
            Criterion::FourLayers => "V01-J1",
            Criterion::InterfaceFreeze => "V01-J2",
            Criterion::AcceptanceLanding => "V01-J3",
            Criterion::AccuracyHardline => "V01-J4",
            Criterion::Criterion => "V01-J5",
        }
    }

    /// 位序号（0 起）。
    pub fn rank(self) -> u8 {
        match self {
            Criterion::FourLayers => 0,
            Criterion::InterfaceFreeze => 1,
            Criterion::AcceptanceLanding => 2,
            Criterion::AccuracyHardline => 3,
            Criterion::Criterion => 4,
        }
    }

    /// 枚举往返守卫。
    pub fn from_code(code: &str) -> Option<Criterion> {
        Criterion::CRITERIA.iter().copied().find(|c| c.code() == code)
    }

    /// 本判据由哪个自检组覆盖（判据→自检项的可追溯映射，判据五的落点）。
    pub fn check_group(self) -> &'static str {
        match self {
            Criterion::FourLayers => "V01-四层-",
            Criterion::InterfaceFreeze => "V01-冻结-",
            Criterion::AcceptanceLanding => "V01-承接-",
            Criterion::AccuracyHardline => "V01-色准-",
            Criterion::Criterion => "V01-判据-",
        }
    }

    /// 读屏单行。
    pub fn screen_line(self) -> String {
        format!(
            "判据 {}：{}（自检组前缀 {}）",
            self.code(),
            self.zh(),
            self.check_group()
        )
    }
}

// ---------------------------------------------------------------------------
// 八、哈希与条目号校验（对账与冻结的唯一算法，单源实现）
// ---------------------------------------------------------------------------

/// FNV-1a 64 位偏移基。
const FNV64_OFFSET: u64 = 0xcbf2_9ce4_8422_2325;

/// FNV-1a 64 位素数。
const FNV64_PRIME: u64 = 0x0000_0100_0000_01b3;

/// FNV-1a 64 位哈希（单遍累积）。
///
/// 选它不用更强哈希是因为**对账要的是确定性而非抗攻击**——两侧算同一样东西
/// 必须得到同一个数，而抗碰撞不是这一层的诉求（契约内容一旦被恶意构造，
/// 走的是安全域的签名链那条线，不是对账这条线）。
pub fn fnv1a64(bytes: &[u8]) -> u64 {
    let mut h = FNV64_OFFSET;
    for b in bytes.iter() {
        h ^= *b as u64;
        h = h.wrapping_mul(FNV64_PRIME);
    }
    h
}

/// FNV-1a 64 位十六进制（16 位小写，定宽）。
pub fn fnv1a64_hex(bytes: &[u8]) -> String {
    format!("{:016x}", fnv1a64(bytes))
}

/// 条目号格式校验（`VE-F` + 四位数字）。
pub fn is_valid_item_id(item: &str) -> bool {
    let Some(rest) = item.strip_prefix("VE-F") else {
        return false;
    };
    rest.len() == 4 && rest.bytes().all(|b| b.is_ascii_digit())
}

// ---------------------------------------------------------------------------
// 九、错误码与错误五元组（错误路径零静默）
// ---------------------------------------------------------------------------

/// 四层不齐。
pub const E_LAYER_COUNT: &str = "E_LAYER_COUNT";

/// 层序非严格递增。
pub const E_LAYER_ORDER: &str = "E_LAYER_ORDER";

/// 层契约残缺。
pub const E_LAYER_SPEC_INCOMPLETE: &str = "E_LAYER_SPEC_INCOMPLETE";

/// 层链断边（相邻边缺失）。
pub const E_LAYER_CHAIN_BROKEN: &str = "E_LAYER_CHAIN_BROKEN";

/// 顶层有出边（应用层不该有下游）。
pub const E_LAYER_TOP_HAS_EDGE: &str = "E_LAYER_TOP_HAS_EDGE";

/// 层既无能力覆盖又未声明消费面。
pub const E_LAYER_UNCLAIMED: &str = "E_LAYER_UNCLAIMED";

/// 能力数与锚点不符。
pub const E_CAPABILITY_COUNT: &str = "E_CAPABILITY_COUNT";

/// 能力无主责条目。
pub const E_CAPABILITY_NO_OWNER: &str = "E_CAPABILITY_NO_OWNER";

/// 能力落点层不存在于层册。
pub const E_CAPABILITY_LAYER_MISSING: &str = "E_CAPABILITY_LAYER_MISSING";

/// 能力落点层不是该能力语义层（防派生能力乱落）。
pub const E_CAPABILITY_WRONG_LAYER: &str = "E_CAPABILITY_WRONG_LAYER";

/// 运行期层出现在纯声明期总架构里。
pub const E_RUNTIME_OVERHEAD_DECLARED: &str = "E_RUNTIME_OVERHEAD_DECLARED";

/// 接口集为空。
pub const E_INTERFACE_EMPTY: &str = "E_INTERFACE_EMPTY";

/// 接口数超上限。
pub const E_INTERFACE_CAP: &str = "E_INTERFACE_CAP";

/// 接口契约残缺。
pub const E_INTERFACE_INCOMPLETE: &str = "E_INTERFACE_INCOMPLETE";

/// 非相邻层连线。
pub const E_INTERFACE_NOT_ADJACENT: &str = "E_INTERFACE_NOT_ADJACENT";

/// 接口码重复。
pub const E_INTERFACE_DUP: &str = "E_INTERFACE_DUP";

/// 接口不存在。
pub const E_INTERFACE_UNKNOWN: &str = "E_INTERFACE_UNKNOWN";

/// 越权变更接口（未走 ADR）。
pub const E_INTERFACE_UNFROZEN_CHANGE: &str = "E_INTERFACE_UNFROZEN_CHANGE";

/// 变更须先有已接受 ADR。
pub const E_ADR_REQUIRED: &str = "E_ADR_REQUIRED";

/// ADR 不存在。
pub const E_ADR_NOT_FOUND: &str = "E_ADR_NOT_FOUND";

/// ADR 记录残缺。
pub const E_ADR_INCOMPLETE: &str = "E_ADR_INCOMPLETE";

/// 新声明正文为空。
pub const E_INTERFACE_EMPTY_TEXT: &str = "E_INTERFACE_EMPTY_TEXT";

/// 变更无实质内容（哈希注水）。
pub const E_INTERFACE_NO_CHANGE: &str = "E_INTERFACE_NO_CHANGE";

/// 承接条目残缺。
pub const E_ACCEPTANCE_INCOMPLETE: &str = "E_ACCEPTANCE_INCOMPLETE";

/// 承接源码重复。
pub const E_ACCEPTANCE_DUP: &str = "E_ACCEPTANCE_DUP";

/// 承接表已满。
pub const E_ACCEPTANCE_CAP: &str = "E_ACCEPTANCE_CAP";

/// 三承接点未齐备落地。
pub const E_HANDOFF_NOT_LANDED: &str = "E_HANDOFF_NOT_LANDED";

/// 承接源哈希漂移。
pub const E_HANDOFF_HASH_DRIFT: &str = "E_HANDOFF_HASH_DRIFT";

/// 色准契约不是一等。
pub const E_ACCURACY_NOT_FIRST_CLASS: &str = "E_ACCURACY_NOT_FIRST_CLASS";

/// 色准契约持有层错。
pub const E_ACCURACY_WRONG_OWNER: &str = "E_ACCURACY_WRONG_OWNER";

/// 色准契约申请豁免（硬线禁止）。
pub const E_ACCURACY_WAIVED_FORBIDDEN: &str = "E_ACCURACY_WAIVED_FORBIDDEN";

/// 色准契约登记为延后（硬线禁止）。
pub const E_ACCURACY_DEFERRED_FORBIDDEN: &str = "E_ACCURACY_DEFERRED_FORBIDDEN";

/// 色准契约种类缺登记。
pub const E_ACCURACY_KIND_MISSING: &str = "E_ACCURACY_KIND_MISSING";

/// 色准契约版本低于最低要求。
pub const E_ACCURACY_VERSION_TOO_LOW: &str = "E_ACCURACY_VERSION_TOO_LOW";

/// 色准契约未落地。
pub const E_ACCURACY_NOT_LANDED: &str = "E_ACCURACY_NOT_LANDED";

/// 色准契约声明哈希漂移。
pub const E_ACCURACY_HASH_DRIFT: &str = "E_ACCURACY_HASH_DRIFT";

/// 色准契约种类重复。
pub const E_ACCURACY_DUP: &str = "E_ACCURACY_DUP";

/// 色准契约条目残缺。
pub const E_ACCURACY_INCOMPLETE: &str = "E_ACCURACY_INCOMPLETE";

/// 消费侧缺契约版本戳。
pub const E_STAMP_REQUIRED: &str = "E_STAMP_REQUIRED";

/// 色准契约被复制到非色彩层。
pub const E_COLOR_CONTRACT_DUPLICATED: &str = "E_COLOR_CONTRACT_DUPLICATED";

/// 显示配置含隐私字段。
pub const E_CONFIG_HAS_PRIVACY: &str = "E_CONFIG_HAS_PRIVACY";

/// 层间对拍失配。
pub const E_CROSS_CHECK_DRIFT: &str = "E_CROSS_CHECK_DRIFT";

/// 错误五元组（码/现象/原因/下一步/责任方）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ConsistencyError {
    /// 错误码。
    pub code: &'static str,
    /// 发生了什么。
    pub what: &'static str,
    /// 为什么。
    pub why: String,
    /// 下一步（必填——拒绝必须给出路）。
    pub next: &'static str,
    /// 责任方。
    pub who: String,
}

impl ConsistencyError {
    /// 构造（五元组齐发，构造点强制写全）。
    pub fn new(
        code: &'static str,
        what: &'static str,
        why: &str,
        next: &'static str,
        who: &str,
    ) -> Self {
        ConsistencyError {
            code,
            what,
            why: why.to_string(),
            next,
            who: who.to_string(),
        }
    }

    /// 五元组齐备性自检（`next` 为空即不合格）。
    pub fn is_complete(&self) -> bool {
        !self.code.trim().is_empty()
            && !self.what.trim().is_empty()
            && !self.why.trim().is_empty()
            && !self.next.trim().is_empty()
            && !self.who.trim().is_empty()
    }

    /// 读屏可读的完整错误（现象/原因/下一步齐发）。
    pub fn screen_text(&self) -> String {
        format!(
            "错误 {}：{}；原因：{}；下一步：{}；责任方：{}",
            self.code, self.what, self.why, self.next, self.who
        )
    }
}

/// 严重度。
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Severity {
    /// 阻断：不修不得开工。
    Blocking,
    /// 警告：可开工但须限期修。
    Warning,
}

impl Severity {
    /// 中文名。
    pub fn zh(self) -> &'static str {
        match self {
            Severity::Blocking => "阻断",
            Severity::Warning => "警告",
        }
    }
}

/// 契约问题（四元组：码/现象/根因/建议 + 严重度）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ContractIssue {
    /// 问题码。
    pub code: &'static str,
    /// 现象。
    pub symptom: String,
    /// 根因。
    pub root_cause: String,
    /// 建议。
    pub advice: &'static str,
    /// 严重度。
    pub severity: Severity,
}

impl ContractIssue {
    /// 由错误五元组转契约问题（统一播报形态）。
    pub fn from_error(e: &ConsistencyError, severity: Severity) -> Self {
        ContractIssue {
            code: e.code,
            symptom: e.screen_text(),
            root_cause: e.why.clone(),
            advice: e.next,
            severity,
        }
    }

    /// 读屏单行（问题要能念给用户听——异常零静默）。
    pub fn screen_line(&self) -> String {
        format!(
            "契约问题[{}·{}]：{}；根因 {}；建议 {}",
            self.severity.zh(),
            self.code,
            self.symptom,
            self.root_cause,
            self.advice
        )
    }
}

// ---------------------------------------------------------------------------
// 十、禁扩面与下游归属（防止抢活与漏活）
// ---------------------------------------------------------------------------

/// 禁扩面清单（V 域**不做**什么）。
///
/// 越界拒绝必须告诉对方「这事该谁做」，否则下次还会有人试。
pub const BOUNDARY_EXCLUSIONS: [(&str, &str); MAX_EXCLUSIONS] = [
    ("V-OWN-LEDGER", "显示器台账/指纹卡/能力表/热插拔事件流本体——归 VE-F4402"),
    ("V-OWN-TOPO", "屏-位-缩放-主副四元组与拓扑事务引擎——归 VE-F4421"),
    ("V-OWN-ENGINE", "色彩管理引擎四模块本体（解析/调度/缓存/回退）——归 VE-F4403"),
    ("V-OWN-MATRIX", "色彩空间矩阵库与双精度转换——归 VE-F4406"),
    ("V-OWN-HDRPIPE", "HDR 四段管线本体（探测/元数据/映射/混合）——归 VE-F4404"),
    ("V-OWN-PROFILE", "显示器配置文件三节与版本链——归 VE-F4405"),
    ("V-OWN-SYNC", "多屏色彩同步三件（联动/漂移/豁免）——归 VE-F4407"),
    ("V-OWN-INTENT", "色彩意图声明-引擎裁决-回执三步协议——归 VE-F4408"),
    ("V-OWN-VIRTUAL", "虚拟屏创建销毁与能力如实声明——归 VE-F4410"),
    ("V-OWN-CVD", "色弱三型映射滤镜曲线与增强器本体——归 VE-F4411"),
    ("V-OWN-PERF", "显示性能四指标与三档预算——归 VE-F4412"),
    ("V-OWN-API", "显示 API 五族清单与冻结评审——归 VE-F4414"),
];

/// 下游归属表（跨批对接点：F4395 上游 / F4402-F4420 下游 / F4420 双签闸）。
///
/// 共 19 条——V01 组 F4402-F4420 每一项的归属都在册，防漏活也防抢活。
pub const DOWNSTREAM_OWNERSHIP: [(&str, &str); 19] = [
    ("VE-F4402", "多显示器统一管理（枚举/三重指纹/能力表/热插拔合并）"),
    ("VE-F4403", "色彩管理引擎（四模块/按需激活/缺省标注/意图仲裁）"),
    ("VE-F4404", "HDR 管线（探测/元数据抽象/色调映射/SDR 同屏混合）"),
    ("VE-F4405", "显示器配置文件（三节档案/开放格式/版本回退/三查校验）"),
    ("VE-F4406", "色彩空间转换引擎（矩阵库/双精度/往返断言）"),
    ("VE-F4407", "多屏色彩同步（派生联动/漂移监测/独立豁免）"),
    ("VE-F4408", "色彩与 M 域深化（元数据透传/三步协商/单次变换断言）"),
    ("VE-F4409", "HDR 与 M 域深化（直通/叠加混合/捕获降级）"),
    ("VE-F4410", "显示与 L 域虚拟显示（虚拟屏/能力如实/统一枚举）"),
    ("VE-F4411", "色彩无障碍（三型滤镜/对比度增强/信息保真断言）"),
    ("VE-F4412", "显示性能（四指标/三档预算/批量预热/渲染优先）"),
    ("VE-F4413", "显示调试器（色彩探针/状态视图/元数据查看/回放）"),
    ("VE-F4414", "显示 API 冻结（五族清单/语义版本/双版期）"),
    ("VE-F4415", "显示文档（架构册/配置指南/校准手册/FAQ）"),
    ("VE-F4416", "显示 fuzz（畸形配置/乱序事件/越界元数据）"),
    ("VE-F4417", "显示与 S 域联动（偏好入 S 双维/词典词条提案）"),
    ("VE-F4418", "V01 联调（组内八段贯通/跨组预交换/十例端到端）"),
    ("VE-F4419", "V01 预备自查（四维自查/色准承诺与契约源逐条对拍）"),
    ("VE-F4420", "V01 组收口双签（双签三件/台账回写/交付声明）"),
];

/// 查下游归属（`None` = 不在本组范围）。
pub fn downstream_owner_of(item: &str) -> Option<&'static str> {
    DOWNSTREAM_OWNERSHIP
        .iter()
        .find(|(k, _)| *k == item)
        .map(|(_, v)| *v)
}

// ---------------------------------------------------------------------------
// 十一、主状态机：V 域显示与色彩总架构
// ---------------------------------------------------------------------------

/// V 域显示与色彩总架构（判据一至判据四的载体）。
#[derive(Clone, Debug)]
pub struct DisplayColorArchitecture {
    /// 总纲版本。
    pub version: String,
    /// 四层册。
    pub layers: Vec<LayerSpec>,
    /// 接口冻结册。
    pub freeze: InterfaceFreezeLedger,
    /// 承接面落地表。
    pub acceptance: AcceptanceLedger,
    /// 色准硬线。
    pub hardline: AccuracyHardline,
}

impl DisplayColorArchitecture {
    /// **标准总纲**（四层齐 + 三条相邻边冻结 + 三承接点落地 + 两类色准契约一等）。
    pub fn standard() -> Self {
        DisplayColorArchitecture {
            version: ARCH_VERSION.to_string(),
            layers: STANDARD_LAYERS.to_vec(),
            freeze: InterfaceFreezeLedger::new(standard_interfaces(), INTERFACE_VERSION)
                .expect("标准接口册应可冻结：三条相邻边齐备且哈希实算"),
            acceptance: standard_acceptance(),
            hardline: AccuracyHardline::standard(),
        }
    }

    /// 某层的册内条目。
    pub fn layer(&self, l: Layer) -> Option<&LayerSpec> {
        self.layers.iter().find(|s| s.layer == l)
    }

    /// 持色彩契约文本的层（应恰有一个——色彩层）。
    pub fn color_contract_owners(&self) -> Vec<Layer> {
        self.layers
            .iter()
            .map(|s| s.layer)
            .filter(|l| l.owns_color_contract())
            .collect()
    }

    /// **判据一：四层**（复杂度 C1：O(层数) = O(4)）。
    ///
    /// 核五件事：层数恰 4、层位严格递增、层契约七项齐备、层链相邻闭合
    /// （每层除顶层外都恰有一条出边）、全部层成本为声明期。
    pub fn check_four_layers(&self) -> Vec<ContractIssue> {
        let mut issues = Vec::new();
        // 1) 层数。
        if self.layers.len() != LAYER_COUNT {
            issues.push(ContractIssue {
                code: E_LAYER_COUNT,
                symptom: format!("层数 {} 与四层不符", self.layers.len()),
                root_cause: "总架构册层数被增删；四层是判据一的硬数字".to_string(),
                advice: "按 Layer::ALL 的四层重建层册；增删层须走 ADR 并改判据",
                severity: Severity::Blocking,
            });
        }
        // 2) 层位严格递增（层序单源）。
        for w in self.layers.windows(2) {
            if w[1].rank != w[0].rank + 1 {
                issues.push(ContractIssue {
                    code: E_LAYER_ORDER,
                    symptom: format!(
                        "层位非严格递增：{} 之后是 {}",
                        w[0].layer.zh(),
                        w[1].layer.zh()
                    ),
                    root_cause: "层册顺序被改动；层序是层链与接口位序的唯一真值".to_string(),
                    advice: "层册按 Layer::rank 升序排列（层序单源 Layer::rank）",
                    severity: Severity::Blocking,
                });
                break;
            }
        }
        // 3) 层契约七项齐备。
        for s in self.layers.iter() {
            if !s.is_complete() {
                issues.push(ContractIssue {
                    code: E_LAYER_SPEC_INCOMPLETE,
                    symptom: format!("层 {} 契约残缺", s.layer.code()),
                    root_cause: "层契约缺字段；残缺契约无法与下游对拍".to_string(),
                    advice: "补齐职责/输入/输出/失败策略/复杂度/不做清单/主责条目",
                    severity: Severity::Blocking,
                });
            }
        }
        // 4) 层链相邻闭合 + 顶层无出边。
        for l in LAYER_ORDER.iter() {
            match l.downstream() {
                Some(down) => {
                    if self.freeze.edge(*l, down).is_none() {
                        issues.push(ContractIssue {
                            code: E_LAYER_CHAIN_BROKEN,
                            symptom: format!("层链断边：{} → {} 无接口", l.zh(), down.zh()),
                            root_cause: "四层相邻边被删；层链不闭合则下游悬空".to_string(),
                            advice: "补回该相邻边接口（见 standard_interfaces 的逐边表）",
                            severity: Severity::Blocking,
                        });
                    }
                }
                None => {
                    if self.freeze.iter().any(|i| i.from == *l) {
                        issues.push(ContractIssue {
                            code: E_LAYER_TOP_HAS_EDGE,
                            symptom: "应用层有出边".to_string(),
                            root_cause: "应用层是顶层终点，有出边即出现环或越层".to_string(),
                            advice: "删除以应用层为上游的接口；消费面层不该向下游交付",
                            severity: Severity::Blocking,
                        });
                    }
                }
            }
        }
        // 5) 全部层声明期（本域总架构零运行期开销）。
        for s in self.layers.iter() {
            if !s.cost.is_zero_overhead() {
                issues.push(ContractIssue {
                    code: E_RUNTIME_OVERHEAD_DECLARED,
                    symptom: format!("层 {} 声明了运行期成本", s.layer.zh()),
                    root_cause: "域开工总架构是声明期结构；运行期成本属各本体条目".to_string(),
                    advice: "把 cost 改回 DeclarationOnly；运行期成本登记在 F4402-F4414 各自条目",
                    severity: Severity::Blocking,
                });
            }
        }
        issues
    }

    /// **四能力与四层双向对账**（复杂度 C4：O(能力数 × 层数) = O(16)）。
    ///
    /// 核四件事：能力数恰 4、每能力有主责条目、每能力落点层在层册内且语义正确、
    /// **每层要么被至少一能力覆盖要么显式声明消费面**（头注§二的落地形态）。
    pub fn check_capability_alignment(&self) -> Vec<ContractIssue> {
        let mut issues = Vec::new();
        if CAPABILITY_ORDER.len() != CAPABILITY_COUNT {
            issues.push(ContractIssue {
                code: E_CAPABILITY_COUNT,
                symptom: format!("能力清单长度 {} 与锚点四项不符", CAPABILITY_ORDER.len()),
                root_cause: "能力清单被增删；锚点职责定位明文四项".to_string(),
                advice: "按 Capability::ALL 的四项重建；增项须走 ADR 并改锚点对应",
                severity: Severity::Blocking,
            });
        }
        // 逐能力核。
        for c in CAPABILITY_ORDER.iter() {
            if c.owner_item().trim().is_empty() || !is_valid_item_id(c.owner_item()) {
                issues.push(ContractIssue {
                    code: E_CAPABILITY_NO_OWNER,
                    symptom: format!("能力 {} 无合法主责条目", c.zh()),
                    root_cause: "能力无主责即成无主资源；下游开工时没人知道依据是什么".to_string(),
                    advice: "补齐主责条目（VE-F####）；无主的能力不许进册",
                    severity: Severity::Blocking,
                });
            }
            let ls = c.layers();
            if ls.is_empty() {
                issues.push(ContractIssue {
                    code: E_CAPABILITY_LAYER_MISSING,
                    symptom: format!("能力 {} 未落任何层", c.zh()),
                    root_cause: "能力未落层即悬空；层链里找不到它的位置".to_string(),
                    advice: "按头注§二对账表给出落点层",
                    severity: Severity::Blocking,
                });
                continue;
            }
            for l in ls.iter() {
                match self.layer(*l) {
                    None => issues.push(ContractIssue {
                        code: E_CAPABILITY_LAYER_MISSING,
                        symptom: format!(
                            "能力 {} 落点层 {} 不在层册内",
                            c.zh(),
                            l.zh()
                        ),
                        root_cause: "落点层与层册脱节；能力与层不是同一份清单但必须互相对得上"
                            .to_string(),
                        advice: "核对四层册；落点层必须存在于 STANDARD_LAYERS",
                        severity: Severity::Blocking,
                    }),
                    Some(spec) => {
                        // 语义核对：派生能力落点层须在允许集合内。
                        let allowed: &[Layer] = match c {
                            Capability::MultiDisplay => &[Layer::Device],
                            Capability::ColorMgmt => &[Layer::Color],
                            Capability::Hdr => &[Layer::Hdr],
                            // 虚拟显示派生归设备层（VIRTUAL_DISPLAY_ADJUDICATION）。
                            Capability::VirtualDisplay => &[Layer::Device],
                        };
                        if !allowed.contains(l) {
                            issues.push(ContractIssue {
                                code: E_CAPABILITY_WRONG_LAYER,
                                symptom: format!(
                                    "能力 {} 落在 {}，但语义层是 {}",
                                    c.zh(),
                                    l.zh(),
                                    allowed[0].zh()
                                ),
                                root_cause: "能力落点层被改动；派生裁决已登记，改动须走 ADR".to_string(),
                                advice: "改回语义层，或走 ADR 修改 VIRTUAL_DISPLAY_ADJUDICATION",
                                severity: Severity::Blocking,
                            });
                        }
                        // 主责条目须是「落在此层的各能力主责之一」。
                        //
                        // 注意这里为什么不是「等于该能力的主责」：设备层同时
                        // 承载多显示器统一管理（VE-F4402）与虚拟显示（VE-F4410），
                        // 一层只能登记一个主责条目。要求层主责逐个等于各能力主责，
                        // 会让「派生能力归同一层」这种正常结构恒判红——那是判据
                        // 写错，不是结构错。所以口径是：层主责 ∈ 落在本层的
                        // 能力主责集合，且该主责确实对应本层的某项能力。
                        let layer_caps: Vec<&Capability> =
                            CAPABILITY_ORDER.iter().filter(|c2| c2.layers().contains(l)).collect();
                        let owners: Vec<&'static str> =
                            layer_caps.iter().map(|c2| c2.owner_item()).collect();
                        if !owners.contains(&spec.owner_item) {
                            issues.push(ContractIssue {
                                code: E_CAPABILITY_NO_OWNER,
                                symptom: format!(
                                    "层 {} 登记主责 {}，不在本层能力主责集合 {:?} 内",
                                    l.zh(),
                                    spec.owner_item,
                                    owners
                                ),
                                root_cause: "层登记的主责不对应本层任何一项能力；层与能力各说各话"
                                    .to_string(),
                                advice: "把层主责改成落在本层的能力主责之一（多显示器 VE-F4402 / 虚拟显示 VE-F4410）",
                                severity: Severity::Blocking,
                            });
                        }
                    }
                }
            }
        }
        // 反向：每层要么被能力覆盖，要么声明消费面。
        for l in LAYER_ORDER.iter() {
            let covered = CAPABILITY_ORDER.iter().any(|c| c.layers().contains(l));
            if !covered && !l.is_consumer_only() {
                issues.push(ContractIssue {
                    code: E_LAYER_UNCLAIMED,
                    symptom: format!("层 {} 既无能力覆盖又未声明消费面", l.zh()),
                    root_cause: "层被当成无主资源；域本色声明层要么有产出要么明确只消费".to_string(),
                    advice: "补上产出该层的能力，或在 Layer::is_consumer_only 显式声明消费面",
                    severity: Severity::Blocking,
                });
            }
        }
        issues
    }

    /// **判据二：接口冻结**（复杂度 C2：O(接口数) = O(3)）。
    ///
    /// 核四件事：接口数恰 3（三条相邻边）、逐条齐备、相邻闭合、色彩层是唯一
    /// 契约持有者（结构面）。
    pub fn check_interfaces(&self) -> Vec<ContractIssue> {
        let mut issues = Vec::new();
        let expected = LAYER_COUNT - 1;
        if self.freeze.len() != expected {
            issues.push(ContractIssue {
                code: E_INTERFACE_CAP,
                symptom: format!(
                    "接口数 {} 与四层相邻边数 {} 不符",
                    self.freeze.len(),
                    expected
                ),
                root_cause: "四层之间的合法通道恰为 3 条；多一条必是越层，少一条必有悬空层".to_string(),
                advice: "按 Layer::downstream 逐边重建接口册（standard_interfaces）",
                severity: Severity::Blocking,
            });
        }
        for it in self.freeze.iter() {
            if !it.is_complete() {
                issues.push(ContractIssue {
                    code: E_INTERFACE_INCOMPLETE,
                    symptom: format!("接口 {} 契约残缺", it.code),
                    root_cause: "接口契约缺字段；残缺契约无法对拍".to_string(),
                    advice: "补齐吃/吐/失败策略/复杂度/消费方/不做清单与实算哈希",
                    severity: Severity::Blocking,
                });
            }
            if !it.is_adjacent() {
                issues.push(ContractIssue {
                    code: E_INTERFACE_NOT_ADJACENT,
                    symptom: format!(
                        "接口 {} 连接 {} → {}，非相邻",
                        it.code,
                        it.from.zh(),
                        it.to.zh()
                    ),
                    root_cause: "跨层直连绕过中间层；层链的中间层因此形同虚设".to_string(),
                    advice: "改为相邻边；跨层直连须走 ADR 正式增接口并改判据",
                    severity: Severity::Blocking,
                });
            }
        }
        // 结构面：色彩契约文本只能在色彩层（此处在层册层面复核一次）。
        let owners = self.color_contract_owners();
        if owners.len() != 1 || owners[0] != Layer::Color {
            issues.push(ContractIssue {
                code: E_COLOR_CONTRACT_DUPLICATED,
                symptom: format!("色彩契约持有层数 {}（应恰为色彩层一层）", owners.len()),
                root_cause: "色彩契约被复制到别层；色准出现两个真相".to_string(),
                advice: "契约文本只在色彩层；设备/HDR/应用三层持指针",
                severity: Severity::Blocking,
            });
        }
        issues
    }

    /// **判据三：承接落地**（复杂度 C3：O(源数) = O(3)）。
    ///
    /// 核三件事：三承接点齐备且全落地对账、每条源哈希与正文一致、缺陷清单为空。
    pub fn check_acceptance(&self) -> Vec<ContractIssue> {
        let mut issues = Vec::new();
        if !self.acceptance.handoff_points_landed() {
            issues.push(ContractIssue {
                code: E_HANDOFF_NOT_LANDED,
                symptom: format!(
                    "承接点未齐备落地（在册 {} 条 / 要求 {} 条）",
                    self.acceptance.role_count(AcceptanceRole::HandoffPoint),
                    HANDOFF_POINT_COUNT
                ),
                root_cause: "锚点 F4395 交接面点名的三件是明文数量，少一件即未落地".to_string(),
                advice: "把对比度契约/色弱映射/交互词典三件入显示契约源并对账",
                severity: Severity::Blocking,
            });
        }
        for e in self.acceptance.iter() {
            if !e.hash_matches() {
                issues.push(ContractIssue {
                    code: E_HANDOFF_HASH_DRIFT,
                    symptom: format!(
                        "承接源 {} 哈希漂移（在位 {} 实算 {}）",
                        e.code,
                        e.source_hash,
                        fnv1a64_hex(e.content.as_bytes())
                    ),
                    root_cause: "承接内容被改却未重算哈希；移交包与本域各说各话".to_string(),
                    advice: "以移交包冻结哈希为准重算并重封；不得就地改 V 域侧哈希",
                    severity: Severity::Blocking,
                });
            }
        }
        for t in self.acceptance.defects() {
            issues.push(ContractIssue {
                code: E_HANDOFF_NOT_LANDED,
                symptom: t.screen_line(),
                root_cause: "承接缺源；锚点错误路径要求回溯移交包而不是本地推测补造".to_string(),
                advice: t.advice,
                severity: Severity::Blocking,
            });
        }
        issues
    }

    /// **判据四：色准硬线**（复杂度 C5：O(契约数) = O(2)）。
    pub fn check_hardline(&self) -> Vec<ContractIssue> {
        self.hardline
            .violations()
            .iter()
            .map(|e| ContractIssue::from_error(e, Severity::Blocking))
            .collect()
    }

    /// **隐私面：显示配置不含隐私**（锚点「配置不含隐私」）。
    ///
    /// 逐字段查 [`PRIVACY_FORBIDDEN_TOKENS`]；命中即阻断。校准数据与设备指纹
    /// 不在此列——它们描述设备，不描述用户看见了什么。
    pub fn check_privacy(&self, config_fields: &[&str]) -> Vec<ContractIssue> {
        let mut issues = Vec::new();
        for f in config_fields.iter() {
            let lower = f.to_ascii_lowercase();
            for tok in PRIVACY_FORBIDDEN_TOKENS.iter() {
                if lower.contains(tok) {
                    issues.push(ContractIssue {
                        code: E_CONFIG_HAS_PRIVACY,
                        symptom: format!("显示配置字段 {:?} 命中隐私记号 {}", f, tok),
                        root_cause: "显示配置记了屏上看见了什么；配置应只描述设备与色彩事实"
                            .to_string(),
                        advice: "删除该字段或改为只记设备属性；屏上内容不进配置",
                        severity: Severity::Blocking,
                    });
                }
            }
        }
        issues
    }

    /// **层间对拍**（锚点错误路径「层间失配→对拍」；复杂度 C2：O(接口数)）。
    ///
    /// 逐条重算声明哈希与在位冻结哈希比对，并独立报层链断裂——缺边在逐边
    /// 遍历里看不见。
    pub fn cross_check(&self) -> Vec<CrossCheckFinding> {
        let mut out = Vec::new();
        for it in self.freeze.iter() {
            if !it.is_complete() {
                out.push(CrossCheckFinding {
                    interface: it.code.to_string(),
                    kind: CrossCheckKind::Incomplete,
                    frozen_hash: it.declared_hash.clone(),
                    declared_hash: String::new(),
                    located_at: format!("{} 接口契约字段", it.code),
                    advice: "先补齐契约字段；对拍的前置条件不成立时不得比对哈希",
                });
                continue;
            }
            if !it.is_adjacent() {
                out.push(CrossCheckFinding {
                    interface: it.code.to_string(),
                    kind: CrossCheckKind::NotAdjacent,
                    frozen_hash: it.declared_hash.clone(),
                    declared_hash: fnv1a64_hex(it.declared_text().as_bytes()),
                    located_at: format!("{} → {} 连线", it.from.zh(), it.to.zh()),
                    advice: "改为相邻边；跨层直连须走 ADR 正式增接口",
                });
                continue;
            }
            let actual = fnv1a64_hex(it.declared_text().as_bytes());
            if actual != it.declared_hash {
                out.push(CrossCheckFinding {
                    interface: it.code.to_string(),
                    kind: CrossCheckKind::HashDrift,
                    frozen_hash: it.declared_hash.clone(),
                    declared_hash: actual,
                    located_at: format!(
                        "{} 声明正文（吃/吐/失败/消费方/不做清单）",
                        it.code
                    ),
                    advice: "定位到字段后走 ADR 升版；禁止就地改冻结值（对拍不掩盖）",
                });
            }
        }
        for l in LAYER_ORDER.iter() {
            if let Some(down) = l.downstream() {
                if self.freeze.edge(*l, down).is_none() {
                    out.push(CrossCheckFinding {
                        interface: format!("{}-{}", l.code(), down.code()),
                        kind: CrossCheckKind::ChainBroken,
                        frozen_hash: String::new(),
                        declared_hash: String::new(),
                        located_at: format!("{} → {} 缺边", l.zh(), down.zh()),
                        advice: "补回该相邻边接口；层链不闭合则下游悬空",
                    });
                }
            }
        }
        out
    }

    /// **架构总自检**（复杂度：各项定长上界之和）。
    ///
    /// 五项判据逐条核 + 对拍汇总。返回空 vec 即全绿。
    pub fn self_audit(&self) -> Vec<ContractIssue> {
        let mut issues = Vec::new();
        issues.extend(self.check_four_layers());
        issues.extend(self.check_capability_alignment());
        issues.extend(self.check_interfaces());
        issues.extend(self.check_acceptance());
        issues.extend(self.check_hardline());
        for f in self.cross_check() {
            issues.push(ContractIssue {
                code: E_CROSS_CHECK_DRIFT,
                symptom: f.screen_line(),
                root_cause: match f.kind {
                    CrossCheckKind::HashDrift => "有人改了接口声明却没升冻结版本",
                    CrossCheckKind::Incomplete => "接口契约残缺，对拍前置条件不成立",
                    CrossCheckKind::ChainBroken => "四层相邻边缺失，层链不闭合",
                    CrossCheckKind::NotAdjacent => "出现跨层直连，绕过正式接口",
                }
                .to_string(),
                advice: f.advice,
                severity: Severity::Blocking,
            });
        }
        issues
    }

    /// **域就绪闸**（判据一至判据四齐绿才就绪）。
    pub fn domain_ready(&self) -> bool {
        self.self_audit().is_empty()
    }

    /// 未就绪原因（人话列表——缺项要能念出来）。
    pub fn not_ready_reasons(&self) -> Vec<String> {
        let mut out = Vec::new();
        let issues = self.self_audit();
        if issues.is_empty() {
            return out;
        }
        // 按判据归并，让人一眼看出是哪个判据没过。
        let groups: [(&str, &str); 4] = [
            ("四层", E_LAYER_COUNT),
            ("能力对账", E_CAPABILITY_COUNT),
            ("接口冻结", E_INTERFACE_CAP),
            ("承接落地", E_HANDOFF_NOT_LANDED),
        ];
        for (name, code) in groups.iter() {
            if issues.iter().any(|i| i.code == *code) {
                out.push(format!("判据未过：{}", name));
            }
        }
        if issues.iter().any(|i| i.code.starts_with("E_ACCURACY_")) {
            out.push("判据未过：色准硬线".to_string());
        }
        if issues.iter().any(|i| i.code == E_CROSS_CHECK_DRIFT) {
            out.push("判据未过：层间对拍失配".to_string());
        }
        out
    }

    /// **读屏替述**（无障碍替述：把总纲讲成人话，且**覆盖全部五项判据**）。
    ///
    /// 替述与总纲**同源生成**——另写一份的风险是漂移，而漂移的无障碍文档比
    /// 没有更坏：它会让用户以为已经有保障。
    pub fn architecture_narration(&self) -> String {
        let mut s = String::new();
        s.push_str(&format!(
            "VE-V 域显示与色彩总架构，版本 {}，层间接口冻结 {}。\n",
            self.version, self.freeze.interface_version
        ));
        s.push_str(&format!(
            "判据共五项：{}。\n",
            Criterion::CRITERIA
                .iter()
                .map(|c| format!("{}（{}）", c.zh(), c.code()))
                .collect::<Vec<String>>()
                .join("；")
        ));
        s.push_str("四层架构（从屏的物理事实到应用消费）：\n");
        for l in LAYER_ORDER.iter() {
            s.push_str(&l.screen_line());
            s.push('\n');
            if let Some(spec) = self.layer(*l) {
                s.push_str(&format!("  {}\n", spec.screen_line()));
            }
        }
        s.push_str("四能力落点：\n");
        for c in CAPABILITY_ORDER.iter() {
            s.push_str(&c.screen_line());
            s.push('\n');
        }
        s.push_str(&format!("{}\n", self.freeze.screen_text()));
        s.push_str(&format!("{}\n", self.acceptance.screen_text()));
        s.push_str(&format!("{}\n", self.hardline.screen_text()));
        let findings = self.cross_check();
        if findings.is_empty() {
            s.push_str("层间对拍：全绿，三条相邻边声明哈希与冻结哈希一致。\n");
        } else {
            s.push_str(&format!("层间对拍：{} 项失配。\n", findings.len()));
            for f in findings.iter() {
                s.push_str(&f.screen_line());
                s.push('\n');
            }
        }
        s.push_str(&format!(
            "架构冻结哈希 {}（V01 双签闸第一件）。\n",
            self.freeze.freeze_digest()
        ));
        s.push_str(&format!(
            "禁扩面 {} 条；下游归属 {} 条（V01 组 F4402-F4420）。\n",
            BOUNDARY_EXCLUSIONS.len(),
            DOWNSTREAM_OWNERSHIP.len()
        ));
        s.push_str(&format!(
            "域就绪：{}。\n",
            if self.domain_ready() { "是" } else { "否" }
        ));
        s
    }

    /// 域报告（单行摘要，便于台账与巡检抓取）。
    pub fn domain_report(&self) -> String {
        // 分母也走实算而非写死：口径变了（LAYER_COUNT / HANDOFF_POINT_COUNT）
        // 报告自动跟着变，不会出现「分子改了分母还写着旧数字」。
        format!(
            "VE-F4401 V 域显示与色彩总架构 {ver}：四层 {layers}/{layer_max}；\
             接口 {ifs}/{if_max} 冻结；承接点 {pts}/{pt_max} 落地；\
             色准契约 {kc}/{kc_max} 齐备；对拍失配 {drift}；就绪 {ready}",
            ver = self.version,
            layers = self.layers.len(),
            layer_max = LAYER_COUNT,
            ifs = self.freeze.len(),
            if_max = LAYER_COUNT - 1,
            pts = self.acceptance.role_count(AcceptanceRole::HandoffPoint),
            pt_max = HANDOFF_POINT_COUNT,
            kc = self.hardline.len(),
            kc_max = ColorContractKind::ALL.len(),
            drift = self.cross_check().len(),
            ready = if self.domain_ready() { "是" } else { "否" },
        )
    }
}

/// 对拍发现项。
#[derive(Clone, Debug)]
pub struct CrossCheckFinding {
    /// 接口码。
    pub interface: String,
    /// 失配类别。
    pub kind: CrossCheckKind,
    /// 在位冻结哈希。
    pub frozen_hash: String,
    /// 正文实算哈希。
    pub declared_hash: String,
    /// 定位（失配在哪）。
    pub located_at: String,
    /// 建议动作。
    pub advice: &'static str,
}

impl CrossCheckFinding {
    /// 读屏单行。
    pub fn screen_line(&self) -> String {
        format!(
            "对拍失配[{}]：{}；定位 {}；在位 {} 实算 {}；建议 {}",
            self.kind.zh(),
            self.interface,
            self.located_at,
            if self.frozen_hash.is_empty() { "-" } else { &self.frozen_hash },
            if self.declared_hash.is_empty() { "-" } else { &self.declared_hash },
            self.advice
        )
    }
}

/// 对拍失配类别。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CrossCheckKind {
    /// 契约残缺（对拍前置条件不成立）。
    Incomplete,
    /// 非相邻连线。
    NotAdjacent,
    /// 哈希漂移。
    HashDrift,
    /// 层链断裂。
    ChainBroken,
}

impl CrossCheckKind {
    /// 中文名。
    pub fn zh(self) -> &'static str {
        match self {
            CrossCheckKind::Incomplete => "契约残缺",
            CrossCheckKind::NotAdjacent => "非相邻连线",
            CrossCheckKind::HashDrift => "哈希漂移",
            CrossCheckKind::ChainBroken => "层链断裂",
        }
    }
}

/// VE-F4401 域自检（判据逐条映射见 `vev01_checks.rs`）。
pub fn run_vev01_checks() -> CheckSet {
    super::vev01_checks::run_vev01_checks()
}