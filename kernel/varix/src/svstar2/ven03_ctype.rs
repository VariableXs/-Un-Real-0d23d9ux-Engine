//! VE-F2603 · 控件类型体系（VE-N 域 · UI 框架内核 · N01 组）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F2603`
//!
//! # 判据（锚点原文四条）
//!
//! 1. **最小集六类**（容器 / 文本 / 图像 / 按钮 / 滑杆 / 列表占位）；
//! 2. **扩展三件套**（视觉 / 逻辑 / 属性）；
//! 3. **注册制**（类型名 → 构造器，未注册拒绝）；
//! 4. **分层边界**（内核六类最小集 / VE-N 上层完整控件库）。
//!
//! # 判据一：最小集六类
//!
//! 逐类规格见 [`BaseControlSpec`] 与 [`BASE_CONTROLS`]。六类的**共同纪律**：
//!
//! | 类 | 载荷 | 前向对接| 关键约束 |
//! | --- | --- | --- | --- |
//! | 容器 | 子节点布局委托 | F2622 Stack | 不自己算位置，只委托布局 |
//! | 文本 | 文本源 + 样式 | VE-E F0801 | 排印归 E，N 只消费测量结果 |
//! | 图像 | 纹理引用 + 适配模式 | D 域绘制 | 只持引用不持像素 |
//! | 按钮 | 点击事件 + 状态机 | N03 事件 | 状态由 F2602 状态机唯一求出 |
//! | 滑杆 | 值域 + 步进 + 拖拽 | N03 事件 | 值域钳制，不接受越界值 |
//! | 列表占位 | 项数 + 虚拟化接口位 | N04 | **占位即显式报错**，不假装能跑 |
//!
//! **为什么只做六类而不是"通用控件 + 配置"**（本条最容易走偏的一步）：
//! 六类在四个维度上真正分化——有没有子节点（容器唯一）、载荷是文本还是引用、
//! 有没有值域、能不能接收指针事件。用一个 `GenericControl { kind: String,
//! payload: Vec<u8> }` 统一，代价是把这些差异全部推到运行期分支，且类型系统
//! 完全不阻止「给图像控件塞一个字符串」。分六类的收益是：**载荷错配在构造期
//! 就是编译错误**，不是渲染时的空白。
//!
//! **为什么列表是"占位"而不是实现**（STUB 的诚实处置）：虚拟化列表要牵扯
//! 滚动（F2622 组）、项回收、视口计算三件事，属于 N04 组的交付面。本条若
//! 硬写一个"简化版列表"，会得到一个**能跑但和N04 不兼容**的实现——上层拿到它
//! 就不会再等N04，而它的滚动/回收语义与真实现不一致，替换成本高于重写。
//! 故本条只给接口位 + [`ControlSpec::ListPlaceholder`]，被调用时显式报错
//! （STUB 家族纪律：预留位被调用必须显性指路，不许静默返回空）。
//!
//! # 判据二：扩展三件套
//!
//! 自定义控件须提供三件（[`ExtensionSuite`]）：**视觉 / 逻辑 / 属性**，缺一不可。
//!
//! - 视觉（[`VisualSlot`]）：D 域绘制对接接口位。缺省时 [`check_extension`]
//!   返回 `VisualMissing`，由 [`instantiate`] 走「占位渲染 + 告警」显性降级——
//!   **不拒绝实例化**，理由是：视觉缺省只影响"长什么样"，不影响"能不能跑"，
//!   拒绝会让调试期的半成品控件完全无法使用（而半成品正是自研控件的常态）。
//!   但降级必须**显性**：产出占位渲染 + 告警，绝不静默画成空白。
//! - 逻辑（[`LogicSlot`]）：脚本位，Z 域前向预留（F2326 家族）。当前恒为
//!   [`LogicSlot::Unbound`]——本域不实现脚本，故脚本位只能是"未绑定"，
//!   任何声称"已绑定脚本"的值都是伪造。
//! - 属性（属性扩展槽）：自定义属性注册进 F2604 扩展槽。本域只提供
//!   [`ExtensionSuite::property_keys`] 的登记位与去重校验，**不实现属性引擎**
//!   （那是 F2604）。
//!
//! # 判据三：注册制
//!
//! [`TypeRegistry`]：类型名 → 构造器。纪律：
//!
//! - **未注册拒绝**（[`TypeRegistry::construct`]）：实例化前查表，未命中即
//!   `Err(TypeNotRegistered)` + 指引。理由：未注册意味着"没人负责它的构造
//!   语义"——属性默认值、事件挂载、子节点容器性全无人填。返回一个空壳控件
//!   会让错误延后到渲染期才暴露，且那时已无从追查是哪个类型名写错了。
//! - **命名空间隔离**（[`TypeRegistry::register`]):自定义类型名与内置六类
//!   同名时**拒绝**（`NameShadowed`），而不是"覆盖内置"。理由：覆盖的后果是
//!   全树范围内同名控件语义被静默换掉，且只在用到该类型名的界面上显现。
//!   要扩展就起自己的名字（VE-N 上层用 `veui.` 前缀）。
//! - **同名重复注册拒绝**（`DuplicateType`）：注册表是全局单源，两个来源
//!   注册同名类型时，后注册者覆盖前者会让「谁负责」变得不可知。
//!
//! # 判据四：分层边界
//!
//! [`LAYER_BOUNDARY_DOC`]：内核 = 引擎基座（六类最小集 + 扩展协议 + 注册制），
//! VE-N 上层 = 产品控件库（完整控件库、主题皮肤、业务控件）。
//!
//! **边界的裁决规则**（不是"感觉在底层就行"，而是可机检的三条）：
//! 1. 内核六类**不含业务语义**：不得出现"确定按钮/取消按钮/标签页"这类
//!    带角色语义的类型——它们属于上层，因为角色语义随产品变。
//! 2. 内核**不渲染**：全基座零渲染，渲染归 D 域。故 [`BaseControl`] 里
//!    没有任何像素/绘制调用位，只有"我需要被画成什么形状"的规格声明。
//! 3. 上层不得绕过注册制直接造节点——绕过即失去"未注册拒绝"这道防线，
//!    症状是自定义控件在缺属性默认值时静默异常。
//!
//! # 错误路径与降级矩阵（锚点原文六条）
//!
//! | 触发 | 处置 | 是否阻断 |
//! | --- | --- | --- |
//! | 未注册类型实例化 | 拒绝 + 指引（[`TypeRegistry::construct`]） | 是 |
//! | 扩展三件缺视觉 | 占位渲染 + 告警（[`instantiate`]） | 否（显性降级） |
//! | VE-N 接口变更 | 对账钩子（[`reconcile_upper_layer`]） | 是 |
//! | 自定义控件与内置同名 | 命名空间隔离拒绝（[`TypeRegistry::register`]） | 是 |
//! | 列表占位被调用 | 显性报错指路 N04（[`ControlSpec::materialize`]） | 是 |
//!
//! # 性能逐项分解（锚点原文四条）
//!
//! - 六类控件实例化 **O(1)**：构造器只填固定字段，载荷以引用/区间登记，
//!   不预分配子节点列表（`Vec::new()` 不分配）。
//! - 扩展注册 **O(1)**：注册表单次线性查重（[`TypeRegistry::name_taken`]），
//!   注册项数是「内置 6 + 上层/扩展数十」量级，线性查重在O(10) 内完成。
//! - 类型检查 **O(1)**：构造前查表一次。
//! - 全基座**零渲染**：渲染归 D，故本模块无任何绘制调用（见判据四第2 条）。
//!
//! # 跨批对接点
//!
//! - 文字排印 → VE-E F0801（文本控件只消费测量结果，断行/整形归 E）；
//! - 列表虚拟化 → N04（占位接口位，本条 STUB 显性）；
//! - 脚本位 → Z 域前向预留（F2326 家族，当前恒未绑定）；
//! - 扩展家族 → F2504 / F2604（属性扩展槽）；
//! - 树模型 → [`ven02_tree`]（F2602，节点挂在树上，类型体系只管"造什么"）；
//! - 布局 → F2622（容器委托布局，Stack 前向）。
//!
//! # 无障碍与隐私
//!
//! 无隐私面。六类中**按钮与滑杆的无障碍位是强制项**（[`A11yRequirement`]）：
//! 按钮须有 `aria-label` 或可见文本，滑杆须有 `aria-valuenow` 语义与
//! 键盘可调性声明——这两类是纯指针设备无法操作的控件，缺无障碍位等于
//! 把键盘/读屏用户挡在功能外。图像与文本的无障碍位按内容而定（装饰图像可显式
//! 标为 decorative，文本须随内容更新 label）。无障碍位缺失走**告警不阻断**
//! （与视觉缺省同级），但告警必须点名控件类型与实例，避免"一万个控件报同一个码"。
//!
//! 零外部依赖；逻辑 tick 注入，零墙钟；确定性算法、零 IO、回归可复现。

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

use crate::svstar2::ven02_tree::{
    ControlNode, EventType, HandlerEntry, PropertyKey, StateSignals, VisualState, create_node,
    resolve_visual_state,
};

// ---------------------------------------------------------------------------
// 一、诊断码（在 F2602 树码之外新增，不改上游枚举）
// ---------------------------------------------------------------------------

/// 控件类型体系专属诊断码。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CtlDiagCode {
    /// 类型未注册：实例化了一个注册表里没有的类型名。
    TypeNotRegistered,
    /// 类型名重复注册：两个来源抢同一个名字。
    DuplicateType,
    /// 自定义类型名遮蔽内置类型。
    NameShadowed,
    /// 类型名非法（空串 / 含保留字符 / 前缀误用）。
    TypeNameInvalid,
    /// 扩展三件套不齐（缺视觉走显性降级，缺逻辑/属性走拒绝）。
    ExtensionSuiteIncomplete,
    /// 列表占位被调用（虚拟化未实现，STUB 显性报错）。
    ListStubInvoked,
    /// 上层接口漂移：对账钩子发现 VE-N 侧不认本条声明的边界。
    UpperLayerDrift,
    /// 无障碍位缺失（告警不阻断，但必须点名）。
    A11ySlotMissing,
    /// 载荷类型与控件类别错配（如给图像控件塞文本源）。
    PayloadMismatch,
    /// 值域越界（滑杆值超出值域）。
    ValueOutOfRange,
}

impl CtlDiagCode {
    /// 诊断码线缆名（日志与上报用）。
    pub const fn as_str(self) -> &'static str {
        match self {
            CtlDiagCode::TypeNotRegistered => "TYPE_NOT_REGISTERED",
            CtlDiagCode::DuplicateType => "DUPLICATE_TYPE",
            CtlDiagCode::NameShadowed => "NAME_SHADDENED",
            CtlDiagCode::TypeNameInvalid => "TYPE_NAME_INVALID",
            CtlDiagCode::ExtensionSuiteIncomplete => "EXTENSION_SUITE_INCOMPLETE",
            CtlDiagCode::ListStubInvoked => "LIST_STUB_INVOKED",
            CtlDiagCode::UpperLayerDrift => "UPPER_LAYER_DRIFT",
            CtlDiagCode::A11ySlotMissing => "A11Y_SLOT_MISSING",
            CtlDiagCode::PayloadMismatch => "PAYLOAD_MISMATCH",
            CtlDiagCode::ValueOutOfRange => "VALUE_OUT_OF_RANGE",
        }
    }

    /// 是否为阻断类（阻断 = 实例化直接失败；非阻断 = 显性降级 + 告警）。
    ///
    /// 分档依据是「处置动作」而非症状：阻断类的共同特征是"继续下去会产出
    /// 语义错误的对象"，非阻断类的共同特征是"对象语义仍正确只是缺一层表现"。
    pub const fn is_blocking(self) -> bool {
        matches!(
            self,
            CtlDiagCode::TypeNotRegistered
                | CtlDiagCode::DuplicateType
                | CtlDiagCode::NameShadowed
                | CtlDiagCode::TypeNameInvalid
                | CtlDiagCode::ExtensionSuiteIncomplete
                | CtlDiagCode::ListStubInvoked
                | CtlDiagCode::UpperLayerDrift
                | CtlDiagCode::PayloadMismatch
                | CtlDiagCode::ValueOutOfRange
        )
    }
}

/// 控件体系诊断结构（与树诊断同形：`at` 点名控件类型与实例）。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct CtlDiagnostic {
    /// 诊断码。
    pub code: CtlDiagCode,
    /// 人话描述。
    pub message: String,
    /// 处置指引。
    pub hint: String,
    /// 触发位置（类型名 / 实例 id）。
    pub at: String,
}

/// 便捷构造诊断。
pub fn cd(code: CtlDiagCode, message: &str, hint: &str, at: &str) -> CtlDiagnostic {
    CtlDiagnostic {
        code,
        message: String::from(message),
        hint: String::from(hint),
        at: String::from(at),
    }
}

/// 控件类型体系的结果类型。
///
/// **刻意不复用 `ven02_tree::TreeOutcome`**：那是以 `TreeDiagnostic` 为错误载体的
/// 类型，两套诊断码（树码 / 控件码）各有归属，混用一个 `Result` 会迫使调用方在
/// 两边都写不必要转换，且让"错误来自哪一层"在类型上消失。此处用独立载体，
/// 边界转换只在 [`crate::svstar2::ven03_ctype::cfail_from_tree`] 一处显式发生。
pub type CtlOutcome<T> = Result<T, CtlDiagnostic>;

/// 便捷构造成功。
pub fn ctok<T>(v: T) -> CtlOutcome<T> {
    Ok(v)
}

/// 便捷构造失败。
pub fn cfail<T>(code: CtlDiagCode, message: &str, hint: &str, at: &str) -> CtlOutcome<T> {
    Err(cd(code, message, hint, at))
}

// ---------------------------------------------------------------------------
// 二、判据一：六类基础控件规格
// ---------------------------------------------------------------------------

/// 图像适配模式（图像控件的九点缩放语义）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FitMode {
    /// 原始尺寸，不缩放。
    None,
    /// 等比缩放到完整显示（可能留边）。
    Contain,
    /// 等比缩放填满（可能裁切）。
    Cover,
    /// 非等比拉伸填满（会失真，故显式命名，不叫 `Stretch`）。
    Fill,
}

impl FitMode {
    /// 线缆名。
    pub const fn as_str(self) -> &'static str {
        match self {
            FitMode::None => "none",
            FitMode::Contain => "contain",
            FitMode::Cover => "cover",
            FitMode::Fill => "fill",
        }
    }

    /// 是否保持宽高比。
    pub const fn keeps_aspect(self) -> bool {
        matches!(self, FitMode::None | FitMode::Contain | FitMode::Cover)
    }
}

/// 文本换行模式（文本控件）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum WrapMode {
    /// 不换行，超出省略。
    NoWrap,
    /// 按宽度换行。
    WordWrap,
    /// 按字符换行（无空格长串用）。
    CharWrap,
}

/// 滑杆值域规格。
///
/// **只derive `PartialEq` 不derive `Eq`**：字段含 `f32`，而 `Eq` 要求
/// 自反律对全部成员成立——`f32` 的 `NaN != NaN` 破坏该要求。带 `Eq` 的类型
/// 会被 `HashSet`/`HashMap`要求，这里刻意不提供，避免误用。
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct ValueRange {
    /// 下界。
    pub min: f32,
    /// 上界。
    pub max: f32,
    /// 步进。
    pub step: f32,
}

impl ValueRange {
    /// 构造值域并校验（`min < max`、`step > 0` 且步进不粗于量程）。
    pub fn new(min: f32, max: f32, step: f32) -> CtlOutcome<ValueRange> {
        if !min.is_finite() || !max.is_finite() || !step.is_finite() {
            return cfail(
                CtlDiagCode::ValueOutOfRange,
                "值域含非有限值（NaN/Inf）",
                "值域三值须全为有限数；非有限值会让钳制结果不可预测",
                "ValueRange::new",
            );
        }
        if min >= max {
            return cfail(
                CtlDiagCode::ValueOutOfRange,
                &format!("值域下界 {} 不小于上界 {}", min, max),
                "值域须 min < max；相等即零量程，任何值都无意义",
                "ValueRange::new",
            );
        }
        if step <= 0.0 {
            return cfail(
                CtlDiagCode::ValueOutOfRange,
                &format!("步进 {} 非正", step),
                "步进须 > 0；零或负步进会让「下一个值」不存在",
                "ValueRange::new",
            );
        }
        if step > max - min {
            return cfail(
                CtlDiagCode::ValueOutOfRange,
                &format!("步进 {} 粗于量程 {}", step, max - min),
                "步进大于等于量程时量程内只有一个可达值，滑杆退化为常量",
                "ValueRange::new",
            );
        }
        ctok(ValueRange { min, max, step })
    }

    /// 把任意值钳制进值域并对齐到步进网格。
    ///
    /// 钳制与对齐**分两步且顺序固定**：先钳制（处理越界），再对齐（处理精度）。
    /// 反序会出错：越界值先对齐仍越界（如 step=0.1、值=1e9，对齐后仍是 1e9）。
    pub fn clamp_align(self, v: f32) -> f32 {
        // **非有限输入必须显式拦在钳制之前**（这是本函数最容易漏的一处）：
        //   `NaN < min` 与 `NaN > max` 都是 false，故朴素的 if/else 链会让 NaN
        //   原样穿过钳制；随后 `NaN.floor()` 仍是 NaN，`min + NaN*step` 仍是 NaN，
        //   最后两道 `aligned > max` / `aligned < min` 也都是 false —— NaN 一路
        //   畅通直达返回值。后果是滑杆值变成 NaN，而下游布局与渲染都把 NaN
        //   当合法数用，症状为"控件不显示且无任何报错"。
        //   落点取 `min`（不是 0、不是 max）：min 是量程内确定可达的端点，
        //   且对"用户误把空输入框解析成 NaN"这个最常见来源，钳到下界
        //   与"用户还没填任何值"的语义一致。
        if !v.is_finite() {
            return self.min;
        }
        let c = if v < self.min {
            self.min
        } else if v > self.max {
            self.max
        } else {
            v
        };
        if self.step <= 0.0 {
            return c;
        }
        let steps = ((c - self.min) / self.step).floor();
        let mut aligned = self.min + steps * self.step;
        // 浮点误差兜底：floor 后的乘法可能让 aligned 越过上界一丝。
        if aligned > self.max {
            aligned = self.max;
        }
        if aligned < self.min {
            aligned = self.min;
        }
        aligned
    }

    /// 可达值个数（用于「滑杆是否退化为常量」的机检）。
    pub fn step_count(self) -> usize {
        if self.step <= 0.0 {
            return 0;
        }
        let n = ((self.max - self.min) / self.step).floor();
        if n < 0.0 {
            0
        } else {
            n as usize + 1
        }
    }
}

/// 无障碍要求等级。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum A11yLevel {
    /// 必须有（缺则该控件对键盘/读屏用户不可用）。
    Required,
    /// 建议有（缺则告警）。
    Recommended,
    /// 不适用（纯装饰）。
    NotApplicable,
}

/// 一类控件的无障碍要求。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct A11yRequirement {
    /// 等级。
    pub level: A11yLevel,
    /// 缺位时的处置说明。
    pub requirement: &'static str,
    /// 声明的语义键（`aria-label` / `aria-valuenow` 等）。
    pub semantics: &'static [&'static str],
}

/// 六类基础控件之一。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BaseControl {
    /// 容器：子节点布局委托（F2622 前向）。
    Container,
    /// 文本：文本源 + 样式（VE-E F0801 契约）。
    Text,
    /// 图像：纹理引用 + 适配模式。
    Image,
    /// 按钮：点击事件 + 状态机（N03 前向）。
    Button,
    /// 滑杆：值域 + 步进 + 拖拽（N03 前向）。
    Slider,
    /// 列表占位：项数 + 虚拟化接口位（N04 前向，STUB）。
    ListPlaceholder,
}

/// 六类全集（注册制内置项的事实源）。
pub const BASE_CONTROLS: [BaseControl; 6] = [
    BaseControl::Container,
    BaseControl::Text,
    BaseControl::Image,
    BaseControl::Button,
    BaseControl::Slider,
    BaseControl::ListPlaceholder,
];

/// 内置类型的线缆名（注册表键）。
pub const fn base_name(c: BaseControl) -> &'static str {
    match c {
        BaseControl::Container => "container",
        BaseControl::Text => "text",
        BaseControl::Image => "image",
        BaseControl::Button => "button",
        BaseControl::Slider => "slider",
        BaseControl::ListPlaceholder => "list",
    }
}

/// 按线缆名解析内置类型（未知即 `None`，交注册表判「未注册」）。
pub fn base_from_name(name: &str) -> Option<BaseControl> {
    let mut i = 0usize;
    while i < BASE_CONTROLS.len() {
        let c = BASE_CONTROLS[i];
        if base_name(c) == name {
            return Some(c);
        }
        i += 1;
    }
    None
}

/// 一类基础控件的逐类规格。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct BaseControlSpec {
    /// 类别。
    pub class: BaseControl,
    /// 类型名。
    pub name: &'static str,
    /// 一句话职责（人话，进代码评审 checklist）。
    pub responsibility: &'static str,
    /// 是否可容纳子节点。
    pub accepts_children: bool,
    /// 是否接收指针事件。
    pub accepts_pointer: bool,
    /// 是否承载一个值域。
    pub has_value_range: bool,
    /// 是否为 STUB（占位未实现）。
    pub is_stub: bool,
    /// 登记的属性键（该类**必须**支持的属性子集）。
    pub property_keys: &'static [PropertyKey],
    /// 默认挂载的事件类型。
    pub default_events: &'static [EventType],
    /// 无障碍要求。
    pub a11y: A11yRequirement,
    /// 前向对接标注。
    pub forward: &'static str,
}

/// 图像控件规格。
const SPEC_IMAGE: BaseControlSpec = BaseControlSpec {
    class: BaseControl::Image,
    name: "image",
    responsibility: "显示一个纹理引用；只持引用不持像素，绘制归 D 域",
    accepts_children: false,
    accepts_pointer: false,
    has_value_range: false,
    is_stub: false,
    property_keys: &[PropertyKey::Width, PropertyKey::Height, PropertyKey::Opacity, PropertyKey::Clip],
    default_events: &[],
    a11y: A11yRequirement {
        level: A11yLevel::Recommended,
        requirement: "有信息含义的图像须有 aria-label；纯装饰须显式标为 decorative",
        semantics: &["aria-label", "aria-hidden"],
    },
    forward: "D 域绘制（纹理上传与采样）",
};

/// 容器控件规格。
const SPEC_CONTAINER: BaseControlSpec = BaseControlSpec {
    class: BaseControl::Container,
    name: "container",
    responsibility: "承载子节点并把布局委托出去；自己不计算任何子节点位置",
    accepts_children: true,
    accepts_pointer: true,
    has_value_range: false,
    is_stub: false,
    property_keys: &[
        PropertyKey::Width,
        PropertyKey::Height,
        PropertyKey::PositionX,
        PropertyKey::PositionY,
        PropertyKey::Opacity,
        PropertyKey::Clip,
    ],
    default_events: &[EventType::PointerDown, EventType::PointerMove],
    a11y: A11yRequirement {
        level: A11yLevel::Recommended,
        requirement: "纯布局容器应标为 decorative；分组容器应有 aria-label 表明分组语义",
        semantics: &["aria-label", "aria-hidden"],
    },
    forward: "F2622 布局容器（Stack/Grid/Flex/Wrap 前向）",
};

/// 文本控件规格。
const SPEC_TEXT: BaseControlSpec = BaseControlSpec {
    class: BaseControl::Text,
    name: "text",
    responsibility: "显示一段文本；断行与字距由 E 域排印，本类只消费测量结果",
    accepts_children: false,
    accepts_pointer: false,
    has_value_range: false,
    is_stub: false,
    property_keys: &[
        PropertyKey::Text,
        PropertyKey::Width,
        PropertyKey::Height,
        PropertyKey::Color,
        PropertyKey::Opacity,
        PropertyKey::AriaLabel,
    ],
    default_events: &[],
    a11y: A11yRequirement {
        level: A11yLevel::Required,
        requirement: "文本须随内容更新 aria-label，否则读屏读到的是过期内容",
        semantics: &["aria-label"],
    },
    forward: "VE-E F0801 文字渲染（排印/整形/测量）",
};

/// 按钮控件规格。
const SPEC_BUTTON: BaseControlSpec = BaseControlSpec {
    class: BaseControl::Button,
    name: "button",
    responsibility: "可点击控件；视觉状态由 F2602 状态机唯一求出，不由各处散赋值",
    accepts_children: false,
    accepts_pointer: true,
    has_value_range: false,
    is_stub: false,
    property_keys: &[
        PropertyKey::Text,
        PropertyKey::Enabled,
        PropertyKey::Width,
        PropertyKey::Height,
        PropertyKey::Opacity,
        PropertyKey::AriaLabel,
    ],
    default_events: &[EventType::PointerDown, EventType::PointerUp, EventType::Focus, EventType::Blur],
    a11y: A11yRequirement {
        level: A11yLevel::Required,
        requirement: "须有 aria-label 或可见文本；须声明键盘可触发（Enter/Space）",
        semantics: &["aria-label", "aria-disabled"],
    },
    forward: "N03 输入域（点击与键盘事件派发）",
};

/// 滑杆控件规格。
const SPEC_SLIDER: BaseControlSpec = BaseControlSpec {
    class: BaseControl::Slider,
    name: "slider",
    responsibility: "在值域内取值；拖拽与键盘调节各走一条路，值恒经钳制对齐",
    accepts_children: false,
    accepts_pointer: true,
    has_value_range: true,
    is_stub: false,
    property_keys: &[
        PropertyKey::Enabled,
        PropertyKey::Width,
        PropertyKey::Height,
        PropertyKey::AriaLabel,
    ],
    default_events: &[
        EventType::PointerDown,
        EventType::PointerMove,
        EventType::ValueChange,
        EventType::Focus,
    ],
    a11y: A11yRequirement {
        level: A11yLevel::Required,
        requirement: "须有 aria-valuenow/valuemin/valuemax 语义；须声明方向键可调",
        semantics: &["aria-valuenow", "aria-valuemin", "aria-valuemax"],
    },
    forward: "N03 输入域（拖拽与键盘）",
};

/// 列表占位规格。
const SPEC_LIST: BaseControlSpec = BaseControlSpec {
    class: BaseControl::ListPlaceholder,
    name: "list",
    responsibility: "声明「这里要有一个虚拟化列表」；本条不实现虚拟化，被调用即显性报错",
    accepts_children: false,
    accepts_pointer: true,
    has_value_range: false,
    is_stub: true,
    property_keys: &[
        PropertyKey::Width,
        PropertyKey::Height,
        PropertyKey::Opacity,
        PropertyKey::Clip,
    ],
    default_events: &[EventType::Wheel, EventType::PointerDown],
    a11y: A11yRequirement {
        level: A11yLevel::Required,
        requirement: "虚拟化列表须有 aria-setsize/aria-posinset 语义，否则读屏只读到视口内几项",
        semantics: &["aria-setsize", "aria-posinset"],
    },
    forward: "N04 虚拟化列表（本条 STUB 显性报错指路）",
};

/// 六类规格全集（逐类规格公开的事实源）。
pub const BASE_CONTROL_SPECS: [BaseControlSpec; 6] = [
    SPEC_CONTAINER,
    SPEC_TEXT,
    SPEC_IMAGE,
    SPEC_BUTTON,
    SPEC_SLIDER,
    SPEC_LIST,
];

/// 按类别取规格。
pub fn spec_of(c: BaseControl) -> &'static BaseControlSpec {
    let mut i = 0usize;
    while i < BASE_CONTROL_SPECS.len() {
        if BASE_CONTROL_SPECS[i].class == c {
            return &BASE_CONTROL_SPECS[i];
        }
        i += 1;
    }
    &BASE_CONTROL_SPECS[0]
}

// ---------------------------------------------------------------------------
// 三、判据一/二：控件载荷与实例
// ---------------------------------------------------------------------------

/// 控件载荷（按类别分化的载荷；不做统一 `Vec<u8>` 正是判据一的用意）。
///
/// 只 `PartialEq`：滑杆变体含 `f32`，`Eq` 不适用（见 [`ValueRange`] 的同源说明）。
#[derive(Clone, PartialEq, Debug)]
pub enum Payload {
    /// 容器：无载荷。
    Container,
    /// 文本：文本源 id（内容归属性引擎）。
    Text {
        /// 文本源标识。
        source: String,
        /// 换行模式。
        wrap: WrapMode,
    },
    /// 图像：纹理引用 + 适配模式。
    Image {
        /// 纹理引用（不持像素）。
        texture: String,
        /// 适配模式。
        fit: FitMode,
    },
    /// 按钮：显示文本（点击事件由默认事件表承担）。
    Button {
        /// 按钮文本。
        label: String,
    },
    /// 滑杆：值域 + 当前值。
    Slider {
        /// 值域。
        range: ValueRange,
        /// 当前值（构造时已钳制对齐）。
        value: f32,
    },
    /// 列表占位：项数（虚拟化接口位未实现）。
    List {
        /// 项数。
        item_count: usize,
    },
}

/// 视觉对接槽（D 域绘制接口位）。
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum VisualSlot {
    /// 已绑定视觉实现（标识符；实际绘制由 D 域按标识解析）。
    Bound(String),
    /// 未绑定 → 实例化走占位渲染 + 告警（显性降级，不拒绝）。
    Unbound,
}

/// 逻辑对接槽（Z 域脚本位前向预留，F2326 家族）。
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum LogicSlot {
    /// 未绑定脚本（**本域的唯一合法值**：本域不实现脚本）。
    Unbound,
}

/// 自定义控件的扩展三件套。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct ExtensionSuite {
    /// 视觉件（缺省走占位渲染 + 告警）。
    pub visual: VisualSlot,
    /// 逻辑件（本域恒为 `Unbound`）。
    pub logic: LogicSlot,
    /// 属性扩展槽：自定义属性键（登记进F2604 扩展槽）。
    pub property_keys: Vec<PropertyKey>,
}

impl ExtensionSuite {
    /// 构造一个「三件齐备」的扩展套（视觉已绑定 + 逻辑未绑定 + 属性列表）。
    pub fn complete(visual: &str, property_keys: Vec<PropertyKey>) -> ExtensionSuite {
        ExtensionSuite {
            visual: VisualSlot::Bound(String::from(visual)),
            logic: LogicSlot::Unbound,
            property_keys,
        }
    }

    /// 构造一个「缺视觉」的扩展套（用于演练显性降级路径）。
    pub fn missing_visual(property_keys: Vec<PropertyKey>) -> ExtensionSuite {
        ExtensionSuite {
            visual: VisualSlot::Unbound,
            logic: LogicSlot::Unbound,
            property_keys,
        }
    }

    /// 视觉是否缺省。
    pub fn visual_missing(&self) -> bool {
        self.visual == VisualSlot::Unbound
    }
}

/// 视觉降级标记：实例因视觉缺省而走占位渲染。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum VisualDegradation {
    /// 正常（视觉已绑定或该类本就无视觉件要求）。
    None,
    /// 占位渲染（视觉缺省；已产出告警）。
    Placeholder,
}

/// 一个已实例化的控件（挂在 F2602 树节点上）。
#[derive(Clone, PartialEq, Debug)]
pub struct ControlInstance {
    /// 类型名（注册表键）。
    pub type_name: String,
    /// 树节点 id。
    pub node_id: String,
    /// 载荷。
    pub payload: Payload,
    /// 视觉降级状态。
    pub degradation: VisualDegradation,
}

/// 控件实例化的产出（实例 + 沿途告警）。
#[derive(Clone, PartialEq, Debug)]
pub struct InstantiateReport {
    /// 实例。
    pub instance: ControlInstance,
    /// 非阻断告警（视觉缺省 / 无障碍位缺失）。
    pub warnings: Vec<CtlDiagnostic>,
}

/// 载荷与类别的匹配校验（判据一「载荷错配在构造期就拦住」）。
///
/// 逐类核对而非"看起来对就行"：图像类收到 `Payload::Text` 时，
/// 若放行，D 域会拿一个字符串当纹理引用去采样，症状是"贴图是白的"——
/// 而纹理引用错误在采样器里通常被吞成默认值。
pub fn check_payload(class: BaseControl, p: &Payload) -> CtlOutcome<()> {
    let ok = matches!(
        (class, p),
        (BaseControl::Container, Payload::Container)
            | (BaseControl::Text, Payload::Text { .. })
            | (BaseControl::Image, Payload::Image { .. })
            | (BaseControl::Button, Payload::Button { .. })
            | (BaseControl::Slider, Payload::Slider { .. })
            | (BaseControl::ListPlaceholder, Payload::List { .. })
    );
    if ok {
        return ctok(());
    }
    let got = match p {
        Payload::Container => "container",
        Payload::Text { .. } => "text",
        Payload::Image { .. } => "image",
        Payload::Button { .. } => "button",
        Payload::Slider { .. } => "slider",
        Payload::List { .. } => "list",
    };
    cfail(
        CtlDiagCode::PayloadMismatch,
        &format!(
            "载荷类型 {} 与控件类别 {} 不匹配",
            got,
            base_name(class)
        ),
        "构造器必须与类别配套；错配会让下游按错误的载荷解释数据（如拿字符串当纹理采样）",
        base_name(class),
    )
}

/// 容器载荷的占位构造（容器无载荷，但显式给出可避免"忘记传载荷"）。
pub fn container_payload() -> Payload {
    Payload::Container
}

/// 文本载荷构造。
pub fn text_payload(source: &str, wrap: WrapMode) -> Payload {
    Payload::Text {
        source: String::from(source),
        wrap,
    }
}

/// 图像载荷构造。
pub fn image_payload(texture: &str, fit: FitMode) -> Payload {
    Payload::Image {
        texture: String::from(texture),
        fit,
    }
}

/// 按钮载荷构造。
pub fn button_payload(label: &str) -> Payload {
    Payload::Button {
        label: String::from(label),
    }
}

/// 滑杆载荷构造（值经钳制对齐，绝不原样存越界值）。
pub fn slider_payload(range: ValueRange, value: f32) -> Payload {
    Payload::Slider {
        range,
        value: range.clamp_align(value),
    }
}

/// 列表载荷构造。
pub fn list_payload(item_count: usize) -> Payload {
    Payload::List { item_count }
}

// ---------------------------------------------------------------------------
// 四、判据三：类型注册制
// ---------------------------------------------------------------------------

/// 类型描述（注册项）。
#[derive(Clone, PartialEq, Debug)]
pub struct TypeEntry {
    /// 类型名。
    pub name: String,
    /// 归属层：内置 / 上层VE-N / 扩展。
    pub layer: Layer,
    /// 绑定的基类（内置六类之一；自定义类型须挂到某个基类上）。
    pub base: BaseControl,
    /// 扩展三件套（内置类型为 `None`）。
    pub extension: Option<ExtensionSuite>,
}

/// 归属层。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Layer {
    /// 内核基座（内置六类）。
    Kernel,
    /// VE-N 上层产品控件库。
    UpperVeN,
    /// 扩展（自定义控件）。
    Extension,
}

impl Layer {
    /// 线缆名。
    pub const fn as_str(self) -> &'static str {
        match self {
            Layer::Kernel => "kernel",
            Layer::UpperVeN => "ve-n-upper",
            Layer::Extension => "extension",
        }
    }
}

/// 上层 VE-N 类型的命名空间前缀（判据三「命名空间隔离」的执行面）。
pub const UPPER_PREFIX: &str = "veui.";

/// 类型名合法性校验（判据三：注册制的前置门）。
///
/// 规则与理由：
/// - 非空、长度 ≤ 64：空名会让诊断的 `at` 字段失去意义；过长名是拼接失误。
/// - 字符集 `[A-Za-z0-9_.-]`：与绑定路径段名同字符集（[`ven02_tree`]
///   的解析器同规则），否则同一字符串在类型名位置合法、在绑定路径位置非法，
///   排查时两边对不上。
/// - 内置名不得带 `.`：内置名是平铺单词，带 `.` 说明有人在冒充分层。
/// - 上层/扩展名**必须**带前缀：`veui.`（上层）或 `ext.`（扩展）。这条是
///   命名空间隔离的硬约束——不带前缀的自定义名会与未来新增的内置名抢位。
pub fn check_type_name(name: &str, layer: Layer) -> CtlOutcome<()> {
    if name.is_empty() {
        return cfail(
            CtlDiagCode::TypeNameInvalid,
            "类型名为空",
            "类型名是注册表主键，不可为空",
            name,
        );
    }
    if name.len() > 64 {
        return cfail(
            CtlDiagCode::TypeNameInvalid,
            &format!("类型名长度 {} 超过上限 64", name.len()),
            "超长名几乎都是拼接失误；请拆成语义化的短名",
            name,
        );
    }
    for b in name.bytes() {
        let ok = b.is_ascii_alphanumeric() || b == b'_' || b == b'.' || b == b'-';
        if !ok {
            return cfail(
                CtlDiagCode::TypeNameInvalid,
                &format!("类型名含非法字符 0x{:02x}", b),
                "只允许字母数字、下划线、点与连字符（与绑定路径段名同字符集）",
                name,
            );
        }
    }
    if layer == Layer::Kernel && name.contains('.') {
        return cfail(
            CtlDiagCode::TypeNameInvalid,
            &format!("内置类型名不得含点：{}", name),
            "内置名是平铺单词；带点会与上层命名空间混淆",
            name,
        );
    }
    if layer == Layer::UpperVeN && !name.starts_with(UPPER_PREFIX) {
        return cfail(
            CtlDiagCode::TypeNameInvalid,
            &format!("上层类型名 {} 缺少前缀 {}", name, UPPER_PREFIX),
            "上层类型必须带命名空间前缀，这是自定义控件不遮蔽内置类型的唯一保证",
            name,
        );
    }
    if layer == Layer::Extension && !name.starts_with("ext.") {
        return cfail(
            CtlDiagCode::TypeNameInvalid,
            &format!("扩展类型名 {} 缺少前缀 ext.", name),
            "扩展类型必须带 ext. 前缀；否则会与未来新增的内置名抢位",
            name,
        );
    }
    ctok(())
}

/// 控件类型注册表。
#[derive(Clone, Debug, Default)]
pub struct TypeRegistry {
    entries: Vec<TypeEntry>,
}

impl TypeRegistry {
    /// 新建注册表（**空表**，不预置内置项）。
    ///
    /// 刻意不自动装入六个内置项：让"用了哪个类型"完全由注册序列决定，
    /// 自检才能发现"某内置类型忘了注册"这类漏装。若构造即装满，
    /// 漏装与正确装入在表里长得一样。
    pub fn new() -> TypeRegistry {
        TypeRegistry {
            entries: Vec::new(),
        }
    }

    /// 装入全部六个内置类型（幂等：重复装入同名即拒绝，不静默覆盖）。
    pub fn with_builtin(mut self) -> CtlOutcome<TypeRegistry> {
        let mut i = 0usize;
        while i < BASE_CONTROLS.len() {
            let c = BASE_CONTROLS[i];
            self.register(
                base_name(c),
                Layer::Kernel,
                c,
                None,
            )?;
            i += 1;
        }
        ctok(self)
    }

    /// 类型名是否已被占用（注册查重的事实源，O(条目数)）。
    pub fn name_taken(&self, name: &str) -> bool {
        self.entries.iter().any(|e| e.name == name)
    }

    /// 取注册项。
    pub fn get(&self, name: &str) -> Option<&TypeEntry> {
        self.entries.iter().find(|e| e.name == name)
    }

    /// 条目数。
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// 是否为空。
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// 注册一个类型。
    ///
    /// 三重拒绝（判据三）：
    /// 1. 名字非法 → `TypeNameInvalid`；
    /// 2. 与内置六类同名（且来路不是内核层）→ `NameShadowed`；
    /// 3. 名字已被占用 → `DuplicateType`。
    pub fn register(
        &mut self,
        name: &str,
        layer: Layer,
        base: BaseControl,
        extension: Option<ExtensionSuite>,
    ) -> CtlOutcome<()> {
        // **遮蔽检查必须先于名字合法性检查**（顺序有讲究，不是随便写的）：
        //   开发者写 `register("button", Extension, ...)` 时同时触犯两条
        //   （遮蔽内置 + 缺 ext. 前缀）。若先查合法性，会报出
        //   `TYPE_NAME_INVALID`——而那条提示只说"加前缀"，开发者照做后
        //   再提交，第二轮才撞上遮蔽，两轮往返；若先报遮蔽，则一次就说清了
        //   真正的危害（覆盖内置会让全树同名控件语义被静默换掉）。
        //   故按「危害严重度」排序：遮蔽 > 合法性 > 重复。
        if layer != Layer::Kernel && base_from_name(name).is_some() {
            return cfail(
                CtlDiagCode::NameShadowed,
                &format!("自定义类型名 {} 与内置类型同名", name),
                "改名或加命名空间前缀（上层用 veui. / 扩展用 ext.）；覆盖内置会让同名控件语义被静默换掉",
                name,
            );
        }
        check_type_name(name, layer)?;
        if self.name_taken(name) {
            return cfail(
                CtlDiagCode::DuplicateType,
                &format!("类型名 {} 已被占用", name),
                "注册表是全局单源；同名重复注册会让「谁负责构造」不可知",
                name,
            );
        }
        // 扩展三件套校验：视觉缺省**允许**（走显性降级），但逻辑位若不是
        // Unbound 则必须真有绑定标识——本域不实现脚本，声称已绑定即为伪造。
        if let Some(ext) = extension.as_ref() {
            if ext.logic != LogicSlot::Unbound {
                return cfail(
                    CtlDiagCode::ExtensionSuiteIncomplete,
                    "逻辑件声明已绑定，但本域不实现脚本",
                    "Z 域脚本位前向未落地；逻辑件当前只能是 Unbound",
                    name,
                );
            }
            // 属性扩展槽去重：重复键会让 F2604 的键表出现二义。
            let mut i = 0usize;
            while i < ext.property_keys.len() {
                let mut j = i + 1;
                while j < ext.property_keys.len() {
                    if ext.property_keys[i] == ext.property_keys[j] {
                        return cfail(
                            CtlDiagCode::ExtensionSuiteIncomplete,
                            "属性扩展槽含重复键",
                            "去重后再注册；重复键会让 F2604 键表二义",
                            name,
                        );
                    }
                    j += 1;
                }
                i += 1;
            }
        }
        self.entries.push(TypeEntry {
            name: String::from(name),
            layer,
            base,
            extension,
        });
        ctok(())
    }

    /// 按名构造控件实例（未注册即拒绝）。
    ///
    /// **列表占位在此处即显性报错**（[`CtlDiagCode::ListStubInvoked`]）：
    /// 理由是"能声明一个列表"与"这个列表能跑"必须分开——本条只提供前者的
    /// 声明能力，若这里放行，上层拿到一个空列表壳就以为完成了，
    /// 而用户看到的是"列表区域永远空白且不报错"，这类缺陷最难归因。
    pub fn construct(
        &self,
        name: &str,
        node_id: &str,
        payload: &Payload,
    ) -> CtlOutcome<InstantiateReport> {
        let entry = match self.get(name) {
            None => {
                let known = self.names_joined();
                return cfail(
                    CtlDiagCode::TypeNotRegistered,
                    &format!("类型 {} 未注册", name),
                    &format!(
                        "先注册（内置类型名：{}）；已注册：{}",
                        BASE_NAMES_JOINED,
                        known
                    ),
                    name,
                );
            }
            Some(e) => e.clone(),
        };
        check_payload(entry.base, payload)?;
        if entry.base == BaseControl::ListPlaceholder {
            return cfail(
                CtlDiagCode::ListStubInvoked,
                "列表占位被实例化：虚拟化列表未实现",
                "虚拟化列表属 N04 组交付面；本条只提供声明位，实现到位前请勿实例化",
                name,
            );
        }
        ctok(self.instantiate_from(&entry, node_id, payload))
    }

    /// 从已取到的注册项实例化（内部复用，避开二次查表）。
    fn instantiate_from(
        &self,
        entry: &TypeEntry,
        node_id: &str,
        payload: &Payload,
    ) -> InstantiateReport {
        let mut warnings: Vec<CtlDiagnostic> = Vec::new();
        let mut degradation = VisualDegradation::None;
        if let Some(ext) = entry.extension.as_ref() {
            if ext.visual_missing() {
                degradation = VisualDegradation::Placeholder;
                warnings.push(cd(
                    CtlDiagCode::ExtensionSuiteIncomplete,
                    &format!("扩展类型 {} 视觉件缺省，已走占位渲染", entry.name),
                    "补上视觉件绑定；占位渲染只保证结构可跑，不代表外观正确",
                    entry.name.as_str(),
                ));
            }
        }
        // 无障碍位检查：按类别的 Required 等级，缺位即告警（不阻断）。
        let spec = spec_of(entry.base);
        if spec.a11y.level == A11yLevel::Required {
            let has_label = payload_has_a11y(entry.base, payload);
            if !has_label {
                warnings.push(cd(
                    CtlDiagCode::A11ySlotMissing,
                    &format!(
                        "类型 {} 属无障碍必需类但未见无障碍位（要求：{}）",
                        entry.name, spec.a11y.requirement
                    ),
                    "补 aria-label / valuenow 语义；缺位等于把键盘与读屏用户挡在功能外",
                    entry.name.as_str(),
                ));
            }
        }
        InstantiateReport {
            instance: ControlInstance {
                type_name: String::from(entry.name.as_str()),
                node_id: String::from(node_id),
                payload: payload.clone(),
                degradation,
            },
            warnings,
        }
    }

    /// 已注册类型名的连接串（诊断指引用）。
    pub fn names_joined(&self) -> String {
        let mut s = String::new();
        for (i, e) in self.entries.iter().enumerate() {
            if i > 0 {
                s.push('|');
            }
            s.push_str(e.name.as_str());
        }
        if s.is_empty() {
            s.push_str("(空)");
        }
        s
    }

    /// 列出全部注册项（分层视图，供自检与文档生成）。
    pub fn list(&self) -> Vec<TypeEntry> {
        self.entries.clone()
    }
}

/// 六个内置类型名的连接串（诊断指引常量，避免每次格式化重拼）。
pub const BASE_NAMES_JOINED: &str = "container|text|image|button|slider|list";

/// 载荷是否携带该类别的无障碍语义位。
///
/// 逐类判定而非"有 aria 属性就算"：滑杆的值语义由值域与当前值**隐含**，
/// 不需要显式 aria 属性（渲染时由 D 域按值域生成 valuenow/valuemin/valuemax），
/// 但**必须**有标签（否则读屏只报"滑块"不报用途）。
fn payload_has_a11y(class: BaseControl, p: &Payload) -> bool {
    match (class, p) {
        // 容器：分组语义可选，故视为满足（规格里是 Recommended）。
        (BaseControl::Container, _) => true,
        // 文本：文本源非空即视为有可读内容。
        (BaseControl::Text, Payload::Text { source, .. }) => !source.is_empty(),
        // 图像：规格为 Recommended，不走本函数（只有 Required 才进来）。
        (BaseControl::Image, _) => true,
        // 按钮：可见文本非空即可见文本；空按钮必须有 aria-label（本类不建模
        // 独立的 aria 属性，故空 label 即缺位）。
        (BaseControl::Button, Payload::Button { label }) => !label.is_empty(),
        // 滑杆：值域本身提供 min/max 语义，故只要值域合法即满足。
        (BaseControl::Slider, Payload::Slider { .. }) => true,
        (BaseControl::ListPlaceholder, _) => true,
        // 错配情形不会走到这里（构造期已拦），保守判为满足以免重复报错。
        _ => true,
    }
}

// ---------------------------------------------------------------------------
// 五、实例落到F2602 树节点
// ---------------------------------------------------------------------------

/// 把实例物化为F2602 树节点（属性键 + 事件表 + 初始状态）。
///
/// 三条纪律：
/// 1. **属性键只登记不取值**：值归F2604 属性引擎，树只持键（F2602 判据一）；
/// 2. **事件按类别默认表挂载**并追加类型自定义项，不覆盖默认项——
///    覆盖会让"按钮必须有 pointer-up"这条保证在某个自定义按钮上失效；
/// 3. **初始状态恒为 Normal**：状态只由状态机求（N03 输入域驱动），
///    构造期写死一个非Normal 态等于伪造输入信号。
pub fn materialize(
    report: &InstantiateReport,
    extra_handlers: &[HandlerEntry],
) -> ControlNode {
    let entry_kind = base_from_name(&report.instance.type_name).unwrap_or(BaseControl::Container);
    let spec = spec_of(entry_kind);
    let mut n = create_node(&report.instance.node_id, spec.name);
    n.property_keys = spec.property_keys.to_vec();
    for h in spec.default_events.iter() {
        n.handlers.push(HandlerEntry {
            event: *h,
            handler_id: format!("{}.default.{}", spec.name, h.as_str()),
            blocking: false,
        });
    }
    for h in extra_handlers.iter() {
        n.handlers.push(h.clone());
    }
    n.state = resolve_visual_state(StateSignals::default());
    n
}

/// 把控件的按钮状态按信号推进（演示「状态由状态机唯一求出」这条纪律）。
pub fn button_state_for(signals: StateSignals) -> VisualState {
    resolve_visual_state(signals)
}

/// 滑杆取值（钳制 + 对齐，调用方不必重复做）。
pub fn slider_value(range: ValueRange, raw: f32) -> f32 {
    range.clamp_align(raw)
}

/// 滑杆按键步进（键盘可达性的一部分：方向键调一个步进）。
///
/// 返回 `None` 表示已在边界且该方向不可再走——**不做环绕**（滑杆到端点
/// 停住是通用预期；环绕会让"一直按左键"在端点反复跳值）。
pub fn slider_step(range: ValueRange, current: f32, forward: bool) -> Option<f32> {
    let cur = range.clamp_align(current);
    let next = if forward {
        cur + range.step
    } else {
        cur - range.step
    };
    let aligned = range.clamp_align(next);
    if forward {
        if aligned > cur {
            Some(aligned)
        } else {
            None
        }
    } else if aligned < cur {
        Some(aligned)
    } else {
        None
    }
}

/// 列表占位被调用的显性报错（供N03/N04 接线处直接复用，避免各写一份文案）。
pub fn list_stub_error(at: &str) -> CtlDiagnostic {
    cd(
        CtlDiagCode::ListStubInvoked,
        "列表虚拟化未实现：列表占位不可用于实际渲染",
        "虚拟化列表属 N04 组交付面（滚动/项回收/视口计算）；实现到位前请显式改用容器 + 逐项展开",
        at,
    )
}

// ---------------------------------------------------------------------------
// 六、判据四：分层边界与对账钩子
// ---------------------------------------------------------------------------

/// 分层边界声明（判据四的文档化形态）。
pub struct LayerBoundaryDoc;

impl LayerBoundaryDoc {
    /// 内核职责。
    pub const KERNEL: &'static str = "N域内核= 引擎基座：六类最小集+ 扩展三件套协议 + 类型注册制 + 树/属性接口位；不含业务语义、不渲染";
    /// 上层职责。
    pub const UPPER: &'static str = "VE-N 上层 = 产品控件库：完整控件库、主题皮肤、业务角色控件（确定/取消/标签页等）；须经注册制接入，不得绕过";
    /// 渲染边界。
    pub const RENDER: &'static str = "D 域= 渲染：N 域产结构与规格，D 域产像素；全基座零渲染";
    /// 布局边界。
    pub const LAYOUT: &'static str = "F2622 布局域 = 位置计算：容器只声明「委托布局」，不自行计算子节点位置";
    /// 脚本边界。
    pub const SCRIPT: &'static str = "Z 域（预留）= 脚本：逻辑件当前恒Unbound，声称已绑定即为伪造";
}

impl LayerBoundaryDoc {
    /// 边界声明全集（自检与文档生成共用同一份，避免两处漂移）。
    pub fn statements() -> [&'static str; 5] {
        [
            Self::KERNEL,
            Self::UPPER,
            Self::RENDER,
            Self::LAYOUT,
            Self::SCRIPT,
        ]
    }
}

/// 上层接口对账结果。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct ReconcileReport {
    /// 是否通过。
    pub ok: bool,
    /// 失配项（每项：上层声明 vs 本条声明）。
    pub mismatches: Vec<String>,
}

/// 对账钩子：核验上层 VE-N 是否遵守本条的分层边界。
///
/// 核验三条（每条都能机检，故不是"文档写了算"）：
/// 1. 上层类型全部带 `veui.` 前缀（否则会遮蔽内置）；
/// 2. 上层没有声称本条的内核职责（渲染/脚本）——越界声明即漂移；
/// 3. 内核六类一个不缺（缺装说明构建期漏了 `with_builtin`）。
pub fn reconcile_upper_layer(reg: &TypeRegistry) -> ReconcileReport {
    let mut mismatches: Vec<String> = Vec::new();
    let mut i = 0usize;
    while i < BASE_CONTROLS.len() {
        let c = BASE_CONTROLS[i];
        if reg.get(base_name(c)).is_none() {
            mismatches.push(format!("内置类型 {} 未注册", base_name(c)));
        }
        i += 1;
    }
    for e in reg.list().iter() {
        if e.layer == Layer::UpperVeN && !e.name.starts_with(UPPER_PREFIX) {
            mismatches.push(format!("上层类型 {} 缺命名空间前缀", e.name));
        }
        if e.layer == Layer::Kernel && e.extension.is_some() {
            mismatches.push(format!("内核类型 {} 不应携带扩展套", e.name));
        }
    }
    ReconcileReport {
        ok: mismatches.is_empty(),
        mismatches,
    }
}

/// 内核是否越界（判据四第 1、2 条的可机检形式）。
///
/// 越界指：内核层出现了带业务角色语义的名字，或内核层携带了绘制调用位。
/// 后者在本模块的结构上已被类型系统排除（[`Payload`] 无像素字段），
/// 故这里核验的是「内核层名字是否含角色语义词」。
const ROLE_WORDS: [&str; 6] = ["ok", "cancel", "tab", "dialog", "menu", "toolbar"];

/// 角色语义词表（越界核验的事实源）。
pub fn role_words() -> [&'static str; 6] {
    ROLE_WORDS
}

/// 核验一层里是否有角色语义名（返回违例名，空切片=通过）。
pub fn role_violations(reg: &TypeRegistry, layer: Layer) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for e in reg.list().iter() {
        if e.layer != layer {
            continue;
        }
        let lower = e.name.to_ascii_lowercase();
        let mut j = 0usize;
        while j < ROLE_WORDS.len() {
            if lower.contains(ROLE_WORDS[j]) {
                out.push(e.name.clone());
                break;
            }
            j += 1;
        }
    }
    out
}

// ---------------------------------------------------------------------------
// 七、性能逐项分解（锚点原文四条）
// ---------------------------------------------------------------------------

/// 性能声明（锚点「性能逐项分解」的可核对形态）。
pub struct PerfDoc;

impl PerfDoc {
    /// 六类实例化 O(1)。
    pub const INSTANTIATE: &'static str = "实例化O(1)：构造器只填固定字段，Vec::new() 不分配";
    /// 扩展注册 O(1)（条目数量级）。
    pub const REGISTER: &'static str = "扩展注册O(1)：注册项数十量级，线性查重在O(10)内完成";
    /// 类型检查 O(1)。
    pub const TYPE_CHECK: &'static str = "类型检查O(1)：构造前查表一次";
    /// 全基座零渲染。
    pub const ZERO_RENDER: &'static str = "全基座零渲染：无任何绘制调用位，渲染归 D 域";
}

impl PerfDoc {
    /// 性能分解全集。
    pub fn statements() -> [&'static str; 4] {
        [
            Self::INSTANTIATE,
            Self::REGISTER,
            Self::TYPE_CHECK,
            Self::ZERO_RENDER,
        ]
    }
}

// ---------------------------------------------------------------------------
// 八、跨批对接台账
// ---------------------------------------------------------------------------

/// 一条对接记录。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Handoff {
    /// 对接对象（任务号或域标识）。
    pub target: &'static str,
    /// 关系。
    pub relation: &'static str,
    /// 本条提供的接口位。
    pub port: &'static str,
}

/// 跨批对接台账（锚点「跨批对接点」的结构化形态）。
pub const HANDOFFS: [Handoff; 6] = [
    Handoff {
        target: "VE-E#VE-F0801",
        relation: "文本排印契约：E 断行/整形/测量，N 只消费",
        port: "Payload::Text",
    },
    Handoff {
        target: "VE-N04",
        relation: "虚拟化列表前向（本条 STUB 显性报错）",
        port: "Payload::List",
    },
    Handoff {
        target: "VE-Z#VE-F2326",
        relation: "脚本位前向预留（本域恒Unbound）",
        port: "LogicSlot",
    },
    Handoff {
        target: "VE-F2504/F2604",
        relation: "扩展家族：属性扩展槽登记",
        port: "ExtensionSuite::property_keys",
    },
    Handoff {
        target: "VE-F2602",
        relation: "树模型：实例物化为树节点",
        port: "materialize",
    },
    Handoff {
        target: "VE-F2622",
        relation: "布局：容器委托布局",
        port: "BaseControl::Container",
    },
];

#[cfg(test)]
mod tests {
    use super::*;
    use crate::svstar2::ven02_tree::{ControlTree, SingleParentPolicy, insert};

    fn full_reg() -> TypeRegistry {
        TypeRegistry::new().with_builtin().unwrap()
    }

    #[test]
    fn 六类_规格齐备且职责各异() {
        assert_eq!(BASE_CONTROLS.len(), 6);
        assert_eq!(BASE_CONTROL_SPECS.len(), 6);
        // 逐类三态（容器性/指针/值域）不得全同——若全同说明退化成了通用控件。
        let mut shapes: Vec<(bool, bool, bool)> = Vec::new();
        for c in BASE_CONTROLS.iter() {
            let s = spec_of(*c);
            shapes.push((s.accepts_children, s.accepts_pointer, s.has_value_range));
        }
        for i in 0..shapes.len() {
            for j in (i + 1)..shapes.len() {
                assert_ne!(shapes[i], shapes[j], "两类规格形态相同：{:?}", shapes[i]);
            }
        }
        // 只有容器可挂子节点。
        assert!(spec_of(BaseControl::Container).accepts_children);
        let mut k = 0usize;
        while k < BASE_CONTROLS.len() {
            let c = BASE_CONTROLS[k];
            if c != BaseControl::Container {
                assert!(!spec_of(c).accepts_children, "{} 不该可挂子节点", base_name(c));
            }
            k += 1;
        }
        // 只有滑杆有值域，只有列表是 STUB。
        assert!(spec_of(BaseControl::Slider).has_value_range);
        assert!(spec_of(BaseControl::ListPlaceholder).is_stub);
    }

    #[test]
    fn 注册制_未注册拒绝并给指引() {
        let r = full_reg();
        let e = r.construct("nope", "n1", &container_payload()).unwrap_err();
        assert_eq!(e.code, CtlDiagCode::TypeNotRegistered);
        // 指引里必须列出已注册项（否则开发者不知有哪些可用类型）。
        assert!(e.hint.contains("container"));
        assert!(e.hint.contains("list"));
    }

    #[test]
    fn 注册制_同名遮蔽与重复均拒绝() {
        let mut r = full_reg();
        // 自定义类型不得与内置同名。
        let e = r
            .register(
                "button",
                Layer::Extension,
                BaseControl::Button,
                Some(ExtensionSuite::complete("v", Vec::new())),
            )
            .unwrap_err();
        assert_eq!(e.code, CtlDiagCode::NameShadowed);
        // 缺前缀的上层名拒绝。
        let e2 = r
            .register("mybutton", Layer::UpperVeN, BaseControl::Button, None)
            .unwrap_err();
        assert_eq!(e2.code, CtlDiagCode::TypeNameInvalid);
        // 缺 ext. 前缀的扩展名拒绝。
        let e3 = r
            .register(
                "mywidget",
                Layer::Extension,
                BaseControl::Button,
                Some(ExtensionSuite::complete("v", Vec::new())),
            )
            .unwrap_err();
        assert_eq!(e3.code, CtlDiagCode::TypeNameInvalid);
        // 合规名可注册，重复注册拒绝。
        r.register(
            "ext.richbtn",
            Layer::Extension,
            BaseControl::Button,
            Some(ExtensionSuite::complete("v", Vec::new())),
        )
        .unwrap();
        let e4 = r
            .register(
                "ext.richbtn",
                Layer::Extension,
                BaseControl::Button,
                Some(ExtensionSuite::complete("v", Vec::new())),
            )
            .unwrap_err();
        assert_eq!(e4.code, CtlDiagCode::DuplicateType);
    }

    #[test]
    fn 扩展三件套_视觉缺省走占位不阻断() {
        let mut r = full_reg();
        r.register(
            "ext.半成品",
            Layer::Extension,
            BaseControl::Button,
            Some(ExtensionSuite::missing_visual(Vec::new())),
        )
        .unwrap();
        let rep = r
            .construct("ext.半成品", "n1", &button_payload("Go"))
            .unwrap();
        assert_eq!(rep.instance.degradation, VisualDegradation::Placeholder);
        // 降级必须显性告警，且告警点名类型。
        assert!(rep.warnings.iter().any(|w| w.at == "ext.半成品"));
        // 逻辑位伪绑定必须被拒（本域不实现脚本）。
        let mut bad = ExtensionSuite::missing_visual(Vec::new());
        bad.logic = LogicSlot::Unbound;
        // 伪造绑定不存在可构造路径（枚举只有 Unbound 一个变体）——
        // 这本身就是设计约束：类型系统层面挡住伪造。
        let e = r
            .register(
                "ext.attrdup",
                Layer::Extension,
                BaseControl::Button,
                Some(ExtensionSuite {
                    visual: VisualSlot::Bound(String::from("v")),
                    logic: LogicSlot::Unbound,
                    property_keys: alloc::vec![PropertyKey::Text, PropertyKey::Text],
                }),
            )
            .unwrap_err();
        assert_eq!(e.code, CtlDiagCode::ExtensionSuiteIncomplete);
        let _ = bad;
    }

    #[test]
    fn 列表占位_实例化即显性报错() {
        let r = full_reg();
        let e = r.construct("list", "n1", &list_payload(100)).unwrap_err();
        assert_eq!(e.code, CtlDiagCode::ListStubInvoked);
        assert!(!e.hint.is_empty());
        // 报错文案必须指路N04（否则开发者不知该等谁）。
        assert!(e.hint.contains("N04"));
    }

    #[test]
    fn 载荷错配_构造期拦住() {
        let r = full_reg();
        // 给图像类塞文本载荷。
        let e = r
            .construct("image", "n1", &text_payload("s", WrapMode::NoWrap))
            .unwrap_err();
        assert_eq!(e.code, CtlDiagCode::PayloadMismatch);
        // 给容器塞按钮载荷。
        let e2 = r.construct("container", "n1", &button_payload("x")).unwrap_err();
        assert_eq!(e2.code, CtlDiagCode::PayloadMismatch);
    }

    #[test]
    fn 滑杆_值域钳制与步进不环绕() {
        let r = ValueRange::new(0.0, 10.0, 0.5).unwrap();
        // 越界值必被钳制。
        assert_eq!(r.clamp_align(-3.0), 0.0);
        assert_eq!(r.clamp_align(1e9), 10.0);
        // 对齐到步进网格。
        let a = r.clamp_align(3.3);
        assert!(a >= 3.0 && a <= 3.5, "未对齐到 0.5 网格：{}", a);
        // 边界不环绕。
        assert_eq!(slider_step(r, 0.0, false), None);
        assert_eq!(slider_step(r, 10.0, true), None);
        assert!(slider_step(r, 0.0, true).is_some());
        // 非法值域拒绝。
        assert!(ValueRange::new(1.0, 1.0, 0.1).is_err());
        assert!(ValueRange::new(0.0, 1.0, 0.0).is_err());
        assert!(ValueRange::new(0.0, 1.0, 2.0).is_err());
    }

    #[test]
    fn 无障碍_必需类缺位必告警且点名() {
        let mut r = full_reg();
        r.register(
            "ext.空按钮",
            Layer::Extension,
            BaseControl::Button,
            Some(ExtensionSuite::complete("v", Vec::new())),
        )
        .unwrap();
        // 空标签按钮 → 无障碍告警。
        let rep = r.construct("ext.空按钮", "n1", &button_payload("")).unwrap();
        assert!(rep
            .warnings
            .iter()
            .any(|w| w.code == CtlDiagCode::A11ySlotMissing && w.at == "ext.空按钮"));
        // 有标签按钮 → 无该告警。
        let rep2 = r.construct("ext.空按钮", "n2", &button_payload("确定")).unwrap();
        assert!(!rep2
            .warnings
            .iter()
            .any(|w| w.code == CtlDiagCode::A11ySlotMissing));
    }

    #[test]
    fn 物化_属性键与事件表落F2602树() {
        let r = full_reg();
        let rep = r.construct("button", "btn1", &button_payload("确定")).unwrap();
        let node = materialize(&rep, &[]);
        // 属性键只登记不取值。
        assert!(node.property_keys.contains(&PropertyKey::Text));
        assert!(node.property_keys.contains(&PropertyKey::AriaLabel));
        // 按钮默认事件齐备。
        assert!(node.handlers.iter().any(|h| h.event == EventType::PointerUp));
        assert!(node.handlers.iter().any(|h| h.event == EventType::Focus));
        // 初始状态恒为 Normal。
        assert_eq!(node.state, VisualState::Normal);
        // 真能挂到 F2602 树上。
        let mut t = ControlTree::new("root").unwrap();
        insert(&mut t, "root", "btn1", None, SingleParentPolicy::Reject).unwrap();
        let live = t.snapshot("btn1").unwrap();
        assert_eq!(live.property_keys.len(), node.property_keys.len());
    }

    #[test]
    fn 分层边界_内核无角色语义且对账通过() {
        let r = full_reg();
        // 内核六类无角色语义词。
        assert!(role_violations(&r, Layer::Kernel).is_empty());
        // 对账通过（内置齐备、前缀合规、内核不带扩展套）。
        let rep = reconcile_upper_layer(&r);
        assert!(rep.ok, "对账失配：{:?}", rep.mismatches);
        // 缺装内置即失配。
        let empty = TypeRegistry::new();
        let rep2 = reconcile_upper_layer(&empty);
        assert!(!rep2.ok);
        assert_eq!(rep2.mismatches.len(), 6);
    }

    #[test]
    fn 诊断码_阻断分档与线缆名唯一() {
        let mut names: Vec<&str> = Vec::new();
        let all = [
            CtlDiagCode::TypeNotRegistered,
            CtlDiagCode::DuplicateType,
            CtlDiagCode::NameShadowed,
            CtlDiagCode::TypeNameInvalid,
            CtlDiagCode::ExtensionSuiteIncomplete,
            CtlDiagCode::ListStubInvoked,
            CtlDiagCode::UpperLayerDrift,
            CtlDiagCode::A11ySlotMissing,
            CtlDiagCode::PayloadMismatch,
            CtlDiagCode::ValueOutOfRange,
        ];
        for c in all.iter() {
            names.push(c.as_str());
        }
        let before = names.len();
        names.sort_unstable();
        names.dedup();
        assert_eq!(before, names.len(), "诊断码线缆名重复");
        // 无障碍告警必须是非阻断档。
        assert!(!CtlDiagCode::A11ySlotMissing.is_blocking());
        // 其余全部阻断。
        for c in all.iter() {
            if *c != CtlDiagCode::A11ySlotMissing {
                assert!(c.is_blocking(), "{:?} 应为阻断档", c);
            }
        }
    }

    #[test]
    fn 文档与台账_无重复且非空() {
        assert_eq!(LayerBoundaryDoc::statements().len(), 5);
        assert_eq!(PerfDoc::statements().len(), 4);
        assert_eq!(HANDOFFS.len(), 6);
        for s in LayerBoundaryDoc::statements().iter() {
            assert!(!s.is_empty());
        }
        for s in PerfDoc::statements().iter() {
            assert!(!s.is_empty());
        }
        let mut ports: Vec<&str> = HANDOFFS.iter().map(|h| h.port).collect();
        let before = ports.len();
        ports.sort_unstable();
        ports.dedup();
        assert_eq!(before, ports.len(), "对接台账接口位重复");
    }
}
