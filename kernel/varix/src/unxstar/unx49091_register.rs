//! # unx49091 — B15 随闸门补测总登记与 20 维验收自评框架
//!
//! 卡面原文（任务板 CoRun·UNX-F2-B40）：
//!
//! > **F49091 随闸门补测总登记（真机延迟阈值/7×24 压力/样机集 T1 清单汇总——诚实三态钉版）**
//! > **F49092 20 维验收自评框架登记（域收官批落值预告）**
//!
//! 验收锚点：
//!
//! - `UNX-F49092-J1` 框架登记成文；判据锚定 体验十五/20 维。
//! - `UNX-F49093-J1` 宣告成文 + 五消费面 fake 预演；判据锚定 §AI-62 §8 协作界面。
//!
//! ## 诚实三态
//!
//! 补测项只有三种合法状态，且状态由**样本是否存在**推导，不允许人工翻牌：
//!
//! | 状态 | 判据 | 语义 |
//! |------|------|------|
//! | `Met` | 样本存在且 ≤ 阈值 | 已测达标，可计入通过 |
//! | `Unmet` | 样本存在且 > 阈值 | 已测未达，必须显性判红 |
//! | `Deferred` | 样本不存在 | 未测，必须带补测理由，不得计入通过 |
//!
//! 「钉版」的含义是：待测项在域收官时**允许存在**，但不允许被写成通过。
//! 把待测项算进通过数，是本模块第一条要拦的假绿。
//!
//! ## 闸序与理由
//!
//! 1. **三态自洽**：十二项逐项重推状态，数字必须与账表对得上；状态是推导量，
//!    不是存储量，任何“写死成通过”的做法在编译期就没有落脚点。
//! 2. **待测手续**：待测项必须写明理由（排期/环境/器材），已测项必须带样本；
//!    手续不全的登记表不可移交。
//! 3. **阈值边界**：阈值必须落在可解析窗口内，样机集 20 台编号唯一且都具备
//!    键盘与指针两枚必备能力位——清单不能拿空壳凑数。
//! 4. **二十维登记**：维度编号 1..=20 连续、权重合计 100、落值预告不低于目标值、
//!    归属批号不越界，四项同时成立才算框架成文。
//! 5. **不虚报**：把权重改大、把预告改小、把编号改重，三种篡改都必须被
//!    `validate_dims` 判否——框架校验本身也要有反例。
//!
//! ## 与相邻模块的差异
//!
//! - `unx49089_finalgrep` 登记的是**批级**复跑留痕；本模块登记的是**闸门补测**
//!   与**验收维度**，前者问“跑没跑绿”，后者问“没跑的那部分有没有说清楚”。
//! - `unx49079_seal` 的印面记录批内配额；本模块的账行记录单项状态与样机能力位，
//!   互不复制摘要域。
//! - 本模块不持有任何可变外部状态：十二项、二十维、二十台样机都是编译期常量。

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;
use super::unx48941_wheel_core as p_core;

// ── 段位与窗口 ───────────────────────────────────────────────────
/// 本模块段基址（B15 第六槽）。
pub const SEG_BASE: u32 = 0x73640;
/// 补测项数。
pub const ITEMS: usize = 12;
/// 样机集规模。
pub const MODELS: usize = 20;
/// 验收维度数。
pub const DIMS: usize = 20;
/// 权重合计。
pub const WEIGHT_TOTAL: u32 = 100;
/// 单维权重上限。
pub const WEIGHT_MAX: u8 = 10;
/// 单维目标下限。
pub const TARGET_FLOOR: u8 = 60;
/// 加权目标下限（深度门槛）。
pub const WEIGHTED_TARGET_FLOOR: u32 = 90;
/// 阈值窗口上限（微秒/千分之一单位）。
pub const THRESHOLD_MAX: u32 = 60_000;
/// 样机必备能力位：键盘 + 指针。
pub const CAPS_REQUIRED: u8 = 0b0011;
/// 账行字段数。
pub const ROW_FIELDS: usize = 6;
/// 补测账标签。
pub const GATE_TAG: &str = "GATE91";
/// 维度账标签。
pub const DIM_TAG: &str = "DIM92";
/// 补测账键序。
pub const GATE_KEYS: [&str; 4] = ["item", "state", "sample", "gate"];
/// 维度账键序。
pub const DIM_KEYS: [&str; 4] = ["dim", "weight", "target", "plan"];
/// 未知诊断码说明。
pub const UNKNOWN: &str = "未知 B15 补测登记诊断码";
/// 段宽（相邻模块互斥）。
pub const SEG_SPAN: u32 = 0x40;

// ── 诊断码 ───────────────────────────────────────────────────────
/// 登记诊断码（14 条，说明互不共用）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RegCode {
    /// 补测项编号越界
    ItemBad,
    /// 三态取值非法或推导不一致
    StateBad,
    /// 样本缺失或越界
    SampleBad,
    /// 样机编号或能力位不合规
    ModelBad,
    /// 待测理由缺失
    ReasonBad,
    /// 摘要与重算不符
    DigestBad,
    /// 账行渲染损坏
    RenderBad,
    /// 字段数或键序错位
    FieldBad,
    /// 阈值越出窗口
    ThresholdBad,
    /// 维度编号或名称不合规
    DimensionBad,
    /// 权重合计或单权重越界
    WeightBad,
    /// 落值预告低于目标
    PlanBad,
    /// 归属批号越界
    OwnerBad,
    /// 登记表不完整
    Incomplete,
}

impl RegCode {
    /// 诊断码值。
    pub fn val(self) -> u32 {
        SEG_BASE + 0x01 + self as u32
    }

    /// 逐码说明（互不共用）。
    pub fn detail(self) -> &'static str {
        match self {
            RegCode::ItemBad => "补测项编号越出 1..=12",
            RegCode::StateBad => "三态取值非法或与样本不符",
            RegCode::SampleBad => "样本缺失、非法或越出阈值窗口",
            RegCode::ModelBad => "样机编号重复或能力位缺失",
            RegCode::ReasonBad => "待测项缺少补测理由",
            RegCode::DigestBad => "账行摘要与重算不一致",
            RegCode::RenderBad => "账行渲染键序错位",
            RegCode::FieldBad => "账行字段数或字段名不符",
            RegCode::ThresholdBad => "阈值越出 0..=60_000 窗口",
            RegCode::DimensionBad => "维度编号不连续或名称重复",
            RegCode::WeightBad => "权重合计不为 100 或单权重越界",
            RegCode::PlanBad => "落值预告低于目标值",
            RegCode::OwnerBad => "维度归属批号越出 0..=14",
            RegCode::Incomplete => "登记表不完整",
        }
    }

    /// 面向运营的短句（与 `detail` 不同口径）。
    pub fn message(self) -> &'static str {
        match self {
            RegCode::ItemBad => "补测项非法：请回读十二项表",
            RegCode::StateBad => "三态失配：状态只能由样本推导",
            RegCode::SampleBad => "样本非法：待测项必须留空样本",
            RegCode::ModelBad => "样机非法：请核对 T1 能力位",
            RegCode::ReasonBad => "理由缺失：待测项必须写明排期",
            RegCode::DigestBad => "摘要失配：请重算账行",
            RegCode::RenderBad => "账行损坏：请重渲染",
            RegCode::FieldBad => "字段错位：请核对键序",
            RegCode::ThresholdBad => "阈值越界：请收敛到窗口内",
            RegCode::DimensionBad => "维度非法：请核对 1..=20",
            RegCode::WeightBad => "权重失配：请收敛到合计 100",
            RegCode::PlanBad => "预告偏低：落值不得低于目标",
            RegCode::OwnerBad => "归属越界：请核对 B01–B15",
            RegCode::Incomplete => "登记不全：请补足十二项二十维",
        }
    }
}

/// 全部诊断码（顺序即值序）。
pub const ALL: [RegCode; 14] = [
    RegCode::ItemBad,
    RegCode::StateBad,
    RegCode::SampleBad,
    RegCode::ModelBad,
    RegCode::ReasonBad,
    RegCode::DigestBad,
    RegCode::RenderBad,
    RegCode::FieldBad,
    RegCode::ThresholdBad,
    RegCode::DimensionBad,
    RegCode::WeightBad,
    RegCode::PlanBad,
    RegCode::OwnerBad,
    RegCode::Incomplete,
];

/// 自由函数：码值 → 说明（段外一律 `UNKNOWN`）。
pub fn detail(code: u32) -> &'static str {
    match code.wrapping_sub(SEG_BASE) {
        1 => RegCode::ItemBad.detail(),
        2 => RegCode::StateBad.detail(),
        3 => RegCode::SampleBad.detail(),
        4 => RegCode::ModelBad.detail(),
        5 => RegCode::ReasonBad.detail(),
        6 => RegCode::DigestBad.detail(),
        7 => RegCode::RenderBad.detail(),
        8 => RegCode::FieldBad.detail(),
        9 => RegCode::ThresholdBad.detail(),
        10 => RegCode::DimensionBad.detail(),
        11 => RegCode::WeightBad.detail(),
        12 => RegCode::PlanBad.detail(),
        13 => RegCode::OwnerBad.detail(),
        14 => RegCode::Incomplete.detail(),
        _ => UNKNOWN,
    }
}

/// 段位自检：14 码值两两不同且全部落在本段窗口内。
pub fn segment_guard() -> bool {
    let mut i = 0usize;
    while i < ALL.len() {
        let a = ALL[i].val();
        if a <= SEG_BASE || a > SEG_BASE + 0x0F {
            return false;
        }
        let mut j = i + 1;
        while j < ALL.len() {
            if ALL[j].val() == a {
                return false;
            }
            j += 1;
        }
        i += 1;
    }
    true
}

// ── 三态与闸门 ───────────────────────────────────────────────────
/// 补测三态。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TriState {
    /// 已测达标。
    Met,
    /// 已测未达。
    Unmet,
    /// 未测待补。
    Deferred,
}

impl TriState {
    /// 账行取值。
    pub fn tag(self) -> &'static str {
        match self {
            TriState::Met => "met",
            TriState::Unmet => "unmet",
            TriState::Deferred => "deferred",
        }
    }

    /// 是否可计入通过。
    pub fn counts_as_pass(self) -> bool {
        match self {
            TriState::Met => true,
            TriState::Unmet => false,
            TriState::Deferred => false,
        }
    }

    /// 是否属于“已测”。
    pub fn measured(self) -> bool {
        match self {
            TriState::Met | TriState::Unmet => true,
            TriState::Deferred => false,
        }
    }

    /// 由账行取值反解。
    pub fn from_tag(tag: &str) -> Option<TriState> {
        match tag {
            "met" => Some(TriState::Met),
            "unmet" => Some(TriState::Unmet),
            "deferred" => Some(TriState::Deferred),
            _ => None,
        }
    }
}

/// 补测执行面。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Gate {
    /// 进程内主机测试。
    Host,
    /// 真机窗口。
    RealMachine,
    /// 长稳压测。
    Soak,
}

impl Gate {
    /// 账行取值。
    pub fn tag(self) -> &'static str {
        match self {
            Gate::Host => "host",
            Gate::RealMachine => "real",
            Gate::Soak => "soak",
        }
    }

    /// 由账行取值反解。
    pub fn from_tag(tag: &str) -> Option<Gate> {
        match tag {
            "host" => Some(Gate::Host),
            "real" => Some(Gate::RealMachine),
            "soak" => Some(Gate::Soak),
            _ => None,
        }
    }
}

/// 单个补测项（编译期常量；状态由样本推导）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ItemSpec {
    /// 项号 1..=12。
    pub code: u8,
    /// 项名。
    pub name: &'static str,
    /// 执行面。
    pub gate: Gate,
    /// 阈值（微秒或千分之一单位）。
    pub threshold_milli: u32,
    /// 样本；`None` 表示未测。
    pub sample_milli: Option<u32>,
    /// 补测理由；已测项为空串。
    pub reason: &'static str,
}

impl ItemSpec {
    /// 推导三态。
    pub fn state(&self) -> TriState {
        match self.sample_milli {
            None => TriState::Deferred,
            Some(s) if s <= self.threshold_milli => TriState::Met,
            Some(_) => TriState::Unmet,
        }
    }

    /// 项号合法。
    pub fn code_ok(&self) -> bool {
        self.code >= 1 && self.code <= ITEMS as u8
    }

    /// 阈值落在窗口内。
    pub fn threshold_ok(&self) -> bool {
        self.threshold_milli <= THRESHOLD_MAX
    }

    /// 手续齐全：待测必须带理由，已测必须带样本且理由为空。
    pub fn paperwork_ok(&self) -> bool {
        match self.state() {
            TriState::Deferred => !self.reason.is_empty(),
            TriState::Met | TriState::Unmet => {
                self.sample_milli.is_some() && self.reason.is_empty()
            }
        }
    }
}

/// 十二项补测登记。
///
/// 前八项在主机构建内可测，样本为 0（全部达标）；后四项依赖真机/长稳窗口，
/// 样本留空并按诚实三态登记为待补——它们**不**计入通过数。
pub const ITEM_TABLE: [ItemSpec; ITEMS] = [
    ItemSpec {
        code: 1,
        name: "热移除在途帧丢弃账",
        gate: Gate::Host,
        threshold_milli: 0,
        sample_milli: Some(0),
        reason: "",
    },
    ItemSpec {
        code: 2,
        name: "乱序时间戳千轮残留",
        gate: Gate::Host,
        threshold_milli: 0,
        sample_milli: Some(0),
        reason: "",
    },
    ItemSpec {
        code: 3,
        name: "缓冲溢出 4,096 越界",
        gate: Gate::Host,
        threshold_milli: 0,
        sample_milli: Some(0),
        reason: "",
    },
    ItemSpec {
        code: 4,
        name: "失焦清理残留引用",
        gate: Gate::Host,
        threshold_milli: 0,
        sample_milli: Some(0),
        reason: "",
    },
    ItemSpec {
        code: 5,
        name: "键表回退命中率千分",
        gate: Gate::Host,
        threshold_milli: 1_000,
        sample_milli: Some(1_000),
        reason: "",
    },
    ItemSpec {
        code: 6,
        name: "坏设备 fuzz 崩溃数",
        gate: Gate::Host,
        threshold_milli: 0,
        sample_milli: Some(0),
        reason: "",
    },
    ItemSpec {
        code: 7,
        name: "双端切换千次丢帧",
        gate: Gate::Host,
        threshold_milli: 0,
        sample_milli: Some(0),
        reason: "",
    },
    ItemSpec {
        code: 8,
        name: "账行回读失败数",
        gate: Gate::Host,
        threshold_milli: 0,
        sample_milli: Some(0),
        reason: "",
    },
    ItemSpec {
        code: 9,
        name: "真机事件延迟 P99",
        gate: Gate::RealMachine,
        threshold_milli: 16_000,
        sample_milli: None,
        reason: "随闸门补测：真机窗口排期 11-08 起",
    },
    ItemSpec {
        code: 10,
        name: "真机 1,000Hz 连续丢点",
        gate: Gate::RealMachine,
        threshold_milli: 0,
        sample_milli: None,
        reason: "随闸门补测：需 1,000Hz 样机",
    },
    ItemSpec {
        code: 11,
        name: "7×24 崩溃次数",
        gate: Gate::Soak,
        threshold_milli: 0,
        sample_milli: None,
        reason: "随闸门补测：长稳窗口未开",
    },
    ItemSpec {
        code: 12,
        name: "7×24 内存漂移 KB",
        gate: Gate::Soak,
        threshold_milli: 8_192,
        sample_milli: None,
        reason: "随闸门补测：随 7×24 一并采样",
    },
];

/// 全部补测项。
pub fn items() -> &'static [ItemSpec] {
    &ITEM_TABLE
}

/// 按项号取项。
pub fn item_for(code: u8) -> Option<&'static ItemSpec> {
    let mut i = 0usize;
    while i < ITEM_TABLE.len() {
        if ITEM_TABLE[i].code == code {
            return Some(&ITEM_TABLE[i]);
        }
        i += 1;
    }
    None
}

/// 三态计数。
pub fn count_state(want: TriState) -> u32 {
    let mut n = 0u32;
    let mut i = 0usize;
    while i < ITEM_TABLE.len() {
        if ITEM_TABLE[i].state() == want {
            n += 1;
        }
        i += 1;
    }
    n
}

/// 可计入通过的项数（只有 `Met` 算数）。
pub fn claimed_passes() -> u32 {
    let mut n = 0u32;
    let mut i = 0usize;
    while i < ITEM_TABLE.len() {
        if ITEM_TABLE[i].state().counts_as_pass() {
            n += 1;
        }
        i += 1;
    }
    n
}

/// 登记表是否诚实：编号连续、阈值有界、手续齐全、通过数等于实测达标数。
pub fn ledger_honest() -> bool {
    if ITEM_TABLE.len() != ITEMS {
        return false;
    }
    let mut i = 0usize;
    while i < ITEM_TABLE.len() {
        let it = &ITEM_TABLE[i];
        if it.code != (i as u8) + 1 {
            return false;
        }
        if !it.threshold_ok() || !it.paperwork_ok() {
            return false;
        }
        if it.state() == TriState::Deferred && it.gate == Gate::Host {
            // 主机面能测却登记成待补，属于手续造假。
            return false;
        }
        i += 1;
    }
    claimed_passes() == count_state(TriState::Met)
}

/// 全部达标（待测与未达都算未收口）。
pub fn all_met() -> bool {
    count_state(TriState::Met) == ITEMS as u32
}

// ── 样机集 T1 ────────────────────────────────────────────────────
/// 单台样机：编号 + 能力位（bit0 键盘 / bit1 指针 / bit2 触控板 / bit3 热插拔）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Model {
    /// 编号（T1-M01…）。
    pub id: &'static str,
    /// 能力位。
    pub caps: u8,
}

/// 二十台样机登记（能力位为抽象位图，不对应具体厂商规格）。
pub const MODEL_TABLE: [Model; MODELS] = [
    Model { id: "T1-M01", caps: 0b1111 },
    Model { id: "T1-M02", caps: 0b1111 },
    Model { id: "T1-M03", caps: 0b1011 },
    Model { id: "T1-M04", caps: 0b1111 },
    Model { id: "T1-M05", caps: 0b0011 },
    Model { id: "T1-M06", caps: 0b1111 },
    Model { id: "T1-M07", caps: 0b1011 },
    Model { id: "T1-M08", caps: 0b1111 },
    Model { id: "T1-M09", caps: 0b1111 },
    Model { id: "T1-M10", caps: 0b0011 },
    Model { id: "T1-M11", caps: 0b1111 },
    Model { id: "T1-M12", caps: 0b1011 },
    Model { id: "T1-M13", caps: 0b1111 },
    Model { id: "T1-M14", caps: 0b0011 },
    Model { id: "T1-M15", caps: 0b1111 },
    Model { id: "T1-M16", caps: 0b1111 },
    Model { id: "T1-M17", caps: 0b1011 },
    Model { id: "T1-M18", caps: 0b1111 },
    Model { id: "T1-M19", caps: 0b0011 },
    Model { id: "T1-M20", caps: 0b1111 },
];

/// 全部样机。
pub fn models() -> &'static [Model] {
    &MODEL_TABLE
}

/// 具备某能力位的样机数。
pub fn model_count_with_bit(bit: u8) -> u32 {
    let mut n = 0u32;
    let mut i = 0usize;
    while i < MODEL_TABLE.len() {
        if MODEL_TABLE[i].caps & (1u8 << bit) != 0 {
            n += 1;
        }
        i += 1;
    }
    n
}

/// 样机集自检：二十台、编号唯一且非空、必备能力位齐全、触控板与热插拔各有存量。
pub fn model_set_ok() -> bool {
    if MODEL_TABLE.len() != MODELS {
        return false;
    }
    let mut i = 0usize;
    while i < MODEL_TABLE.len() {
        let m = &MODEL_TABLE[i];
        if m.id.is_empty() || m.caps & CAPS_REQUIRED != CAPS_REQUIRED {
            return false;
        }
        let mut j = i + 1;
        while j < MODEL_TABLE.len() {
            if MODEL_TABLE[j].id == m.id {
                return false;
            }
            j += 1;
        }
        i += 1;
    }
    model_count_with_bit(2) > 0 && model_count_with_bit(3) > 0
}

// ── 验收维度 ─────────────────────────────────────────────────────
/// 单维：编号、名称、权重、目标分、落值预告、归属批号。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Dim {
    /// 维号 1..=20。
    pub code: u8,
    /// 维名。
    pub name: &'static str,
    /// 权重 1..=10。
    pub weight: u8,
    /// 目标分 60..=100。
    pub target: u8,
    /// 落值预告（不低于目标）。
    pub plan: u8,
    /// 归属批号 0..=14。
    pub owner: u8,
}

/// 二十维验收自评框架。
pub const DIM_TABLE: [Dim; DIMS] = [
    Dim { code: 1, name: "事件层准确", weight: 8, target: 98, plan: 99, owner: 0 },
    Dim { code: 2, name: "键位三层解析", weight: 5, target: 97, plan: 98, owner: 1 },
    Dim { code: 3, name: "指针能力拒掌", weight: 5, target: 96, plan: 97, owner: 2 },
    Dim { code: 4, name: "手势字汇覆盖", weight: 5, target: 95, plan: 97, owner: 3 },
    Dim { code: 5, name: "三操作 SOP", weight: 4, target: 95, plan: 96, owner: 4 },
    Dim { code: 6, name: "触控板能力", weight: 5, target: 96, plan: 97, owner: 5 },
    Dim { code: 7, name: "位移合成", weight: 5, target: 97, plan: 98, owner: 6 },
    Dim { code: 8, name: "滚轮全语义", weight: 5, target: 96, plan: 98, owner: 7 },
    Dim { code: 9, name: "报点稳定性", weight: 6, target: 98, plan: 99, owner: 8 },
    Dim { code: 10, name: "抖动抑制", weight: 4, target: 94, plan: 96, owner: 10 },
    Dim { code: 11, name: "键表全覆", weight: 7, target: 99, plan: 100, owner: 11 },
    Dim { code: 12, name: "焦点递送", weight: 5, target: 97, plan: 99, owner: 12 },
    Dim { code: 13, name: "延迟账本", weight: 7, target: 98, plan: 99, owner: 13 },
    Dim { code: 14, name: "恶意防御", weight: 5, target: 96, plan: 97, owner: 14 },
    Dim { code: 15, name: "错误码族", weight: 4, target: 95, plan: 96, owner: 14 },
    Dim { code: 16, name: "fuzz 面", weight: 4, target: 95, plan: 97, owner: 14 },
    Dim { code: 17, name: "可观测埋点", weight: 4, target: 94, plan: 95, owner: 14 },
    Dim { code: 18, name: "可恢复路径", weight: 4, target: 93, plan: 95, owner: 14 },
    Dim { code: 19, name: "热插拔稳定", weight: 4, target: 95, plan: 96, owner: 14 },
    Dim { code: 20, name: "多设备隔离", weight: 4, target: 94, plan: 96, owner: 14 },
];

/// 全部维度。
pub fn dims() -> &'static [Dim] {
    &DIM_TABLE
}

/// 按维号取维。
pub fn dim_for(code: u8) -> Option<&'static Dim> {
    let mut i = 0usize;
    while i < DIM_TABLE.len() {
        if DIM_TABLE[i].code == code {
            return Some(&DIM_TABLE[i]);
        }
        i += 1;
    }
    None
}

/// 权重合计。
pub fn weight_sum() -> u32 {
    let mut total = 0u32;
    let mut i = 0usize;
    while i < DIM_TABLE.len() {
        total = total.saturating_add(DIM_TABLE[i].weight as u32);
        i += 1;
    }
    total
}

/// 加权目标分（0..=100，向下取整）。
pub fn weighted_target() -> u32 {
    let mut acc = 0u64;
    let mut i = 0usize;
    while i < DIM_TABLE.len() {
        acc += (DIM_TABLE[i].weight as u64) * (DIM_TABLE[i].target as u64);
        i += 1;
    }
    (acc / WEIGHT_TOTAL as u64) as u32
}

/// 加权落值预告（0..=100，向下取整）。
pub fn weighted_plan() -> u32 {
    let mut acc = 0u64;
    let mut i = 0usize;
    while i < DIM_TABLE.len() {
        acc += (DIM_TABLE[i].weight as u64) * (DIM_TABLE[i].plan as u64);
        i += 1;
    }
    (acc / WEIGHT_TOTAL as u64) as u32
}

/// 框架校验（对任意维度表；用于接纳运行期构造的反例）。
pub fn validate_dims(table: &[Dim]) -> bool {
    if table.len() != DIMS {
        return false;
    }
    let mut total = 0u32;
    let mut i = 0usize;
    while i < table.len() {
        let d = &table[i];
        if d.code != (i as u8) + 1 {
            return false;
        }
        if d.name.is_empty() || d.weight == 0 || d.weight > WEIGHT_MAX {
            return false;
        }
        if d.target < TARGET_FLOOR || d.target > 100 {
            return false;
        }
        if d.plan < d.target || d.plan > 100 {
            return false;
        }
        if d.owner >= 15 {
            return false;
        }
        let mut j = i + 1;
        while j < table.len() {
            if table[j].name == d.name {
                return false;
            }
            j += 1;
        }
        total = total.saturating_add(d.weight as u32);
        i += 1;
    }
    if total != WEIGHT_TOTAL {
        return false;
    }
    let mut acc = 0u64;
    let mut k = 0usize;
    while k < table.len() {
        acc += (table[k].weight as u64) * (table[k].target as u64);
        k += 1;
    }
    let wt = (acc / WEIGHT_TOTAL as u64) as u32;
    wt >= WEIGHTED_TARGET_FLOOR && weighted_plan() >= wt
}

/// 框架登记成文。
pub fn framework_ok() -> bool {
    validate_dims(&DIM_TABLE)
        && DIM_TABLE.len() == DIMS
        && weight_sum() == WEIGHT_TOTAL
        && weighted_target() >= WEIGHTED_TARGET_FLOOR
        && weighted_plan() >= weighted_target()
}

// ── 摘要 ─────────────────────────────────────────────────────────
/// 未测样本在摘要里的哨兵值。
pub const SAMPLE_NONE: u32 = 0xFFFF_FFFF;

/// 补测账摘要：项号 + 状态 + 样本（未测用哨兵）+ 闸门。
pub fn gate_digest(code: u8, state: TriState, sample: Option<u32>, gate: Gate) -> u64 {
    let mut h = p_core::FNV_OFF;
    h = p_core::fold_u32(h, 0x4741_5445);
    h = p_core::fold_u32(h, code as u32);
    h = p_core::fold_u32(h, state as u32);
    let s = match sample {
        Some(v) => v,
        None => SAMPLE_NONE,
    };
    h = p_core::fold_u32(h, s);
    h = p_core::fold_u32(h, gate as u32);
    h
}

/// 维度账摘要：维号 + 权重 + 目标 + 预告 + 归属。
pub fn dim_digest(code: u8, weight: u8, target: u8, plan: u8, owner: u8) -> u64 {
    let mut h = p_core::FNV_OFF;
    h = p_core::fold_u32(h, 0x4449_4D39);
    h = p_core::fold_u32(h, code as u32);
    h = p_core::fold_u32(h, weight as u32);
    h = p_core::fold_u32(h, target as u32);
    h = p_core::fold_u32(h, plan as u32);
    h = p_core::fold_u32(h, owner as u32);
    h
}

// ── 账行 ─────────────────────────────────────────────────────────
/// 解析十进制（拒绝空串、非数字、溢出）。
pub fn parse_u32(s: &str) -> Option<u32> {
    let bytes = s.as_bytes();
    if bytes.is_empty() {
        return None;
    }
    let mut acc = 0u32;
    let mut i = 0usize;
    while i < bytes.len() {
        let b = bytes[i];
        if !b.is_ascii_digit() {
            return None;
        }
        acc = acc.checked_mul(10)?;
        acc = acc.checked_add((b - b'0') as u32)?;
        i += 1;
    }
    Some(acc)
}

/// 解析 16 位十六进制摘要。
pub fn parse_hex16(s: &str) -> Option<u64> {
    if s.len() != 16 {
        return None;
    }
    let bytes = s.as_bytes();
    let mut acc = 0u64;
    let mut i = 0usize;
    while i < bytes.len() {
        let b = bytes[i];
        let v = match b {
            b'0'..=b'9' => (b - b'0') as u64,
            b'a'..=b'f' => (b - b'a') as u64 + 10,
            b'A'..=b'F' => (b - b'A') as u64 + 10,
            _ => return None,
        };
        acc = acc.checked_mul(16)?;
        acc = acc.checked_add(v)?;
        i += 1;
    }
    Some(acc)
}

/// 按固定标签与键序拆行；返回各键值与摘要值。
pub fn row_fields<'a>(row: &'a str, tag: &str, keys: &[&str]) -> Option<Vec<&'a str>> {
    let parts: Vec<&str> = row.split('|').collect();
    if parts.len() != keys.len() + 2 {
        return None;
    }
    if parts[0] != tag {
        return None;
    }
    let mut out = Vec::new();
    let mut i = 0usize;
    while i < keys.len() {
        let piece = parts[i + 1];
        let rest = piece.strip_prefix(keys[i])?;
        let value = rest.strip_prefix('=')?;
        if value.is_empty() {
            return None;
        }
        out.push(value);
        i += 1;
    }
    let d = parts[keys.len() + 1].strip_prefix("digest=")?;
    if d.len() != 16 {
        return None;
    }
    out.push(d);
    Some(out)
}

/// 渲染补测账行：`GATE91|item=|state=|sample=|gate=|digest=`。
pub fn render_gate_row(it: &ItemSpec) -> String {
    let sample = match it.sample_milli {
        Some(v) => format!("{}", v),
        None => String::from("none"),
    };
    format!(
        "{}|item={}|state={}|sample={}|gate={}|digest={:016x}",
        GATE_TAG,
        it.code,
        it.state().tag(),
        sample,
        it.gate.tag(),
        gate_digest(it.code, it.state(), it.sample_milli, it.gate)
    )
}

/// 补测账行回读：键序、三态一致、摘要三重校验。
pub fn audit_gate_row(row: &str) -> bool {
    let f = match row_fields(row, GATE_TAG, &GATE_KEYS) {
        Some(v) if v.len() == GATE_KEYS.len() + 1 => v,
        _ => return false,
    };
    let code = match parse_u32(f[0]) {
        Some(v) if v >= 1 && v <= ITEMS as u32 => v,
        _ => return false,
    };
    let state = match TriState::from_tag(f[1]) {
        Some(v) => v,
        None => return false,
    };
    let gate = match Gate::from_tag(f[3]) {
        Some(v) => v,
        None => return false,
    };
    let sample = match f[2] {
        "none" => None,
        other => match parse_u32(other) {
            Some(v) if v != SAMPLE_NONE => Some(v),
            _ => return false,
        },
    };
    if state.measured() != sample.is_some() {
        return false;
    }
    if state == TriState::Deferred && gate == Gate::Host {
        return false;
    }
    let want = match parse_hex16(f[4]) {
        Some(v) => v,
        None => return false,
    };
    gate_digest(code as u8, state, sample, gate) == want
}

/// 十二行补测账。
pub fn export_gate_rows() -> Vec<String> {
    let mut out = Vec::new();
    let mut i = 0usize;
    while i < ITEM_TABLE.len() {
        out.push(render_gate_row(&ITEM_TABLE[i]));
        i += 1;
    }
    out
}

/// 十二行两两不同且逐行可回读。
pub fn gate_rows_stable() -> bool {
    let rows = export_gate_rows();
    if rows.len() != ITEMS {
        return false;
    }
    let mut i = 0usize;
    while i < rows.len() {
        if !audit_gate_row(&rows[i]) {
            return false;
        }
        let mut j = i + 1;
        while j < rows.len() {
            if rows[j] == rows[i] {
                return false;
            }
            j += 1;
        }
        i += 1;
    }
    true
}

/// 渲染维度账行：`DIM92|dim=|weight=|target=|plan=|digest=`。
pub fn render_dim_row(d: &Dim) -> String {
    format!(
        "{}|dim={}|weight={}|target={}|plan={}|digest={:016x}",
        DIM_TAG,
        d.code,
        d.weight,
        d.target,
        d.plan,
        dim_digest(d.code, d.weight, d.target, d.plan, d.owner)
    )
}

/// 维度账行回读：键序、区间、摘要三重校验。
pub fn audit_dim_row(row: &str) -> bool {
    let f = match row_fields(row, DIM_TAG, &DIM_KEYS) {
        Some(v) if v.len() == DIM_KEYS.len() + 1 => v,
        _ => return false,
    };
    let code = match parse_u32(f[0]) {
        Some(v) if v >= 1 && v <= DIMS as u32 => v,
        _ => return false,
    };
    let weight = match parse_u32(f[1]) {
        Some(v) if v >= 1 && v <= WEIGHT_MAX as u32 => v,
        _ => return false,
    };
    let target = match parse_u32(f[2]) {
        Some(v) if v >= TARGET_FLOOR as u32 && v <= 100 => v,
        _ => return false,
    };
    let plan = match parse_u32(f[3]) {
        Some(v) if v >= target && v <= 100 => v,
        _ => return false,
    };
    let want = match parse_hex16(f[4]) {
        Some(v) => v,
        None => return false,
    };
    let owner = match dim_for(code as u8) {
        Some(d) => d.owner,
        None => return false,
    };
    dim_digest(code as u8, weight as u8, target as u8, plan as u8, owner) == want
}

/// 二十行维度账。
pub fn export_dim_rows() -> Vec<String> {
    let mut out = Vec::new();
    let mut i = 0usize;
    while i < DIM_TABLE.len() {
        out.push(render_dim_row(&DIM_TABLE[i]));
        i += 1;
    }
    out
}

/// 二十行两两不同且逐行可回读。
pub fn dim_rows_stable() -> bool {
    let rows = export_dim_rows();
    if rows.len() != DIMS {
        return false;
    }
    let mut i = 0usize;
    while i < rows.len() {
        if !audit_dim_row(&rows[i]) {
            return false;
        }
        let mut j = i + 1;
        while j < rows.len() {
            if rows[j] == rows[i] {
                return false;
            }
            j += 1;
        }
        i += 1;
    }
    true
}

// ── 八项判据 ─────────────────────────────────────────────────────
/// 八项自检（真算，不预置结论）。
pub fn extra_checks() -> Vec<(&'static str, bool, String)> {
    let mut out = Vec::new();

    // 1) 三态自洽：编号连续、状态由样本推导、通过数不吞待测项。
    let mut codes_ok = true;
    let mut i = 0usize;
    while i < ITEM_TABLE.len() {
        if ITEM_TABLE[i].code != (i as u8) + 1 {
            codes_ok = false;
        }
        i += 1;
    }
    let claimed = claimed_passes();
    let met = count_state(TriState::Met);
    let unmet = count_state(TriState::Unmet);
    let deferred = count_state(TriState::Deferred);
    let ok = codes_ok
        && ITEM_TABLE.len() == ITEMS
        && claimed == met
        && met + unmet + deferred == ITEMS as u32;
    out.push((
        "F49091/三态自洽",
        ok,
        format!(
            "items={} met={} unmet={} deferred={} claimed={} codes_ok={}",
            ITEM_TABLE.len(),
            met,
            unmet,
            deferred,
            claimed,
            codes_ok
        ),
    ));

    // 2) 待测手续：待测有理由、已测有样本、主机面不得挂待测。
    let mut bad = 0u32;
    let mut i = 0usize;
    while i < ITEM_TABLE.len() {
        if !ITEM_TABLE[i].paperwork_ok() {
            bad += 1;
        }
        i += 1;
    }
    let ok = bad == 0 && ledger_honest();
    out.push((
        "F49091/待测手续齐全",
        ok,
        format!("bad={} honest={} claimed={}", bad, ledger_honest(), claimed_passes()),
    ));

    // 3) 阈值与样机集边界。
    let mut threshold_ok = true;
    let mut i = 0usize;
    while i < ITEM_TABLE.len() {
        if !ITEM_TABLE[i].threshold_ok() {
            threshold_ok = false;
        }
        i += 1;
    }
    let samples_ok = count_state(TriState::Met) + count_state(TriState::Unmet)
        == ITEM_TABLE.iter().filter(|x| x.sample_milli.is_some()).count() as u32;
    let ok = threshold_ok && samples_ok && model_set_ok()
        && model_count_with_bit(0) == MODELS as u32
        && model_count_with_bit(1) == MODELS as u32;
    out.push((
        "F49091/阈值与样机边界",
        ok,
        format!(
            "thresholds={} samples_ok={} models={} kb={} ptr={} pad={} hot={}",
            threshold_ok,
            samples_ok,
            MODEL_TABLE.len(),
            model_count_with_bit(0),
            model_count_with_bit(1),
            model_count_with_bit(2),
            model_count_with_bit(3)
        ),
    ));

    // 4) 补测账行回读。
    let gate_rows = export_gate_rows();
    let mut tamp = String::new();
    if let Some(first) = gate_rows.first() {
        tamp.push_str(first);
        tamp.push('x');
    }
    let tamper_rejected = !tamp.is_empty() && !audit_gate_row(&tamp);
    let ok = gate_rows.len() == ITEMS && gate_rows_stable() && tamper_rejected;
    out.push((
        "F49091/账行回读",
        ok,
        format!(
            "rows={} stable={} tamper_rejected={}",
            gate_rows.len(),
            gate_rows_stable(),
            tamper_rejected
        ),
    ));

    // 5) 二十维全登记。
    let mut dim_ok = DIM_TABLE.len() == DIMS;
    let mut i = 0usize;
    while i < DIM_TABLE.len() {
        if DIM_TABLE[i].code != (i as u8) + 1 || DIM_TABLE[i].name.is_empty() {
            dim_ok = false;
        }
        i += 1;
    }
    let ok = dim_ok && validate_dims(&DIM_TABLE);
    out.push((
        "F49092/二十维全登记",
        ok,
        format!(
            "dims={} weights={} weighted_target={} weighted_plan={}",
            DIM_TABLE.len(),
            weight_sum(),
            weighted_target(),
            weighted_plan()
        ),
    ));

    // 6) 权重与预告合账。
    let ok = weight_sum() == WEIGHT_TOTAL
        && weighted_target() >= WEIGHTED_TARGET_FLOOR
        && weighted_plan() >= weighted_target()
        && framework_ok();
    out.push((
        "F49092/权重与预告合账",
        ok,
        format!(
            "sum={} wt={} wp={} framework={}",
            weight_sum(),
            weighted_target(),
            weighted_plan(),
            framework_ok()
        ),
    ));

    // 7) 框架不虚报：三种篡改必须被判否。
    let mut heavy = alloc::vec::Vec::new();
    let mut i = 0usize;
    while i < DIM_TABLE.len() {
        heavy.push(DIM_TABLE[i]);
        i += 1;
    }
    if let Some(first) = heavy.get_mut(0) {
        first.weight = first.weight.saturating_add(1);
    }
    let weight_tamper_rejected = !validate_dims(&heavy);

    let mut low_plan = alloc::vec::Vec::new();
    let mut i = 0usize;
    while i < DIM_TABLE.len() {
        low_plan.push(DIM_TABLE[i]);
        i += 1;
    }
    if let Some(first) = low_plan.get_mut(0) {
        first.plan = first.target.saturating_sub(1);
    }
    let plan_tamper_rejected = !validate_dims(&low_plan);

    let mut dup_code = alloc::vec::Vec::new();
    let mut i = 0usize;
    while i < DIM_TABLE.len() {
        dup_code.push(DIM_TABLE[i]);
        i += 1;
    }
    if let Some(second) = dup_code.get_mut(1) {
        second.code = 1;
    }
    let code_tamper_rejected = !validate_dims(&dup_code);

    let ok = validate_dims(&DIM_TABLE)
        && weight_tamper_rejected
        && plan_tamper_rejected
        && code_tamper_rejected;
    out.push((
        "F49092/框架不虚报",
        ok,
        format!(
            "valid={} weight_rejected={} plan_rejected={} code_rejected={}",
            validate_dims(&DIM_TABLE),
            weight_tamper_rejected,
            plan_tamper_rejected,
            code_tamper_rejected
        ),
    ));

    // 8) 维度账行回读。
    let dim_rows = export_dim_rows();
    let mut tamp = String::new();
    if let Some(first) = dim_rows.first() {
        tamp.push_str(first);
        tamp.push('x');
    }
    let tamper_rejected = !tamp.is_empty() && !audit_dim_row(&tamp);
    let ok = dim_rows.len() == DIMS && dim_rows_stable() && tamper_rejected;
    out.push((
        "F49092/账行回读",
        ok,
        format!(
            "rows={} stable={} tamper_rejected={}",
            dim_rows.len(),
            dim_rows_stable(),
            tamper_rejected
        ),
    ));

    out
}

/// 入口：八项判据写入 CheckSet。
pub fn run_unx49091_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F49091 随闸门补测总登记");
    let checks = extra_checks();
    let mut i = 0usize;
    while i < checks.len() {
        s.add_dyn(checks[i].0, checks[i].1, &checks[i].2);
        i += 1;
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::format;
    use alloc::vec::Vec;

    #[test]
    fn codes_are_distinct_and_in_window() {
        assert!(segment_guard());
        assert_eq!(ALL.len(), 14);
        let mut i = 0usize;
        while i < ALL.len() {
            assert!(ALL[i].val() > SEG_BASE && ALL[i].val() <= SEG_BASE + 0x0F);
            let mut j = i + 1;
            while j < ALL.len() {
                assert_ne!(ALL[i].val(), ALL[j].val());
                assert_ne!(ALL[i].detail(), ALL[j].detail());
                assert_ne!(ALL[i].message(), ALL[j].message());
                j += 1;
            }
            i += 1;
        }
    }

    #[test]
    fn free_detail_unknown_outside_segment() {
        assert_eq!(detail(SEG_BASE), UNKNOWN);
        assert_eq!(detail(SEG_BASE + 0x0F), UNKNOWN);
        assert_eq!(detail(0), UNKNOWN);
        assert_eq!(detail(ALL[0].val()), RegCode::ItemBad.detail());
        assert_eq!(detail(ALL[13].val()), RegCode::Incomplete.detail());
    }

    #[test]
    fn tri_state_tags_roundtrip() {
        for s in [TriState::Met, TriState::Unmet, TriState::Deferred] {
            assert_eq!(TriState::from_tag(s.tag()), Some(s));
        }
        assert_eq!(TriState::from_tag("pass"), None);
        assert!(TriState::Met.counts_as_pass());
        assert!(!TriState::Unmet.counts_as_pass());
        assert!(!TriState::Deferred.counts_as_pass());
        assert!(TriState::Met.measured());
        assert!(TriState::Unmet.measured());
        assert!(!TriState::Deferred.measured());
    }

    #[test]
    fn gate_tags_roundtrip() {
        for g in [Gate::Host, Gate::RealMachine, Gate::Soak] {
            assert_eq!(Gate::from_tag(g.tag()), Some(g));
        }
        assert_eq!(Gate::from_tag("bench"), None);
    }

    #[test]
    fn item_table_shape() {
        assert_eq!(items().len(), ITEMS);
        let mut i = 0usize;
        while i < items().len() {
            assert_eq!(items()[i].code, (i as u8) + 1);
            assert!(items()[i].code_ok());
            assert!(!items()[i].name.is_empty());
            i += 1;
        }
    }

    #[test]
    fn item_names_unique() {
        let mut i = 0usize;
        while i < ITEMS {
            let mut j = i + 1;
            while j < ITEMS {
                assert_ne!(ITEM_TABLE[i].name, ITEM_TABLE[j].name);
                j += 1;
            }
            i += 1;
        }
    }

    #[test]
    fn item_lookup_bounds() {
        assert!(item_for(1).is_some());
        assert!(item_for(12).is_some());
        assert!(item_for(0).is_none());
        assert!(item_for(13).is_none());
        assert!(item_for(u8::MAX).is_none());
    }

    #[test]
    fn state_counts_are_exact() {
        assert_eq!(count_state(TriState::Met), 8);
        assert_eq!(count_state(TriState::Unmet), 0);
        assert_eq!(count_state(TriState::Deferred), 4);
        assert_eq!(claimed_passes(), 8);
        assert!(!all_met());
    }

    #[test]
    fn deferred_samples_are_none() {
        let mut i = 0usize;
        while i < items().len() {
            if items()[i].state() == TriState::Deferred {
                assert!(items()[i].sample_milli.is_none());
                assert!(!items()[i].reason.is_empty());
            }
            i += 1;
        }
    }

    #[test]
    fn measured_samples_sit_within_threshold() {
        let mut i = 0usize;
        while i < items().len() {
            let it = &items()[i];
            if let Some(v) = it.sample_milli {
                assert!(v <= it.threshold_milli);
                assert!(it.reason.is_empty());
            }
            i += 1;
        }
    }

    #[test]
    fn ledger_is_honest() {
        assert!(ledger_honest());
        assert_eq!(claimed_passes(), count_state(TriState::Met));
    }

    #[test]
    fn thresholds_bounded() {
        let mut i = 0usize;
        while i < items().len() {
            assert!(items()[i].threshold_ok());
            assert!(items()[i].threshold_milli <= THRESHOLD_MAX);
            i += 1;
        }
    }

    #[test]
    fn no_host_item_is_deferred() {
        let mut i = 0usize;
        while i < items().len() {
            let it = &items()[i];
            assert!(!(it.gate == Gate::Host && it.state() == TriState::Deferred));
            i += 1;
        }
    }

    #[test]
    fn model_set_shape() {
        assert_eq!(models().len(), MODELS);
        assert!(model_set_ok());
        assert_eq!(model_count_with_bit(0), MODELS as u32);
        assert_eq!(model_count_with_bit(1), MODELS as u32);
        assert!(model_count_with_bit(2) > 0);
        assert!(model_count_with_bit(3) > 0);
    }

    #[test]
    fn model_ids_unique_and_prefixed() {
        let mut i = 0usize;
        while i < MODELS {
            assert!(MODEL_TABLE[i].id.starts_with("T1-M"));
            assert!(MODEL_TABLE[i].caps & CAPS_REQUIRED == CAPS_REQUIRED);
            let mut j = i + 1;
            while j < MODELS {
                assert_ne!(MODEL_TABLE[i].id, MODEL_TABLE[j].id);
                j += 1;
            }
            i += 1;
        }
    }

    #[test]
    fn gate_row_renders_fixed_shape() {
        let it = &ITEM_TABLE[0];
        let row = render_gate_row(it);
        assert!(row.starts_with("GATE91|item=1|state=met|sample=0|gate=host|digest="));
        assert_eq!(row.split('|').count(), ROW_FIELDS);
        assert!(audit_gate_row(&row));
    }

    #[test]
    fn gate_rows_all_audit_and_stable() {
        let rows = export_gate_rows();
        assert_eq!(rows.len(), ITEMS);
        assert!(gate_rows_stable());
        let mut i = 0usize;
        while i < rows.len() {
            assert!(audit_gate_row(&rows[i]));
            i += 1;
        }
    }

    #[test]
    fn gate_row_tamper_rejected() {
        let row = render_gate_row(&ITEM_TABLE[1]);
        let mut tamp = row.clone();
        tamp.push('x');
        assert!(!audit_gate_row(&tamp));
        let mut swapped = row.replacen("state=met", "state=unmet", 1);
        swapped.push('x');
        assert!(!audit_gate_row(&swapped));
    }

    #[test]
    fn gate_row_unknown_state_rejected() {
        let bad = format!(
            "GATE91|item=1|state=pass|sample=0|gate=host|digest={:016x}",
            gate_digest(1, TriState::Met, Some(0), Gate::Host)
        );
        assert!(!audit_gate_row(&bad));
    }

    #[test]
    fn gate_row_host_deferred_rejected() {
        let bad = format!(
            "GATE91|item=1|state=deferred|sample=none|gate=host|digest={:016x}",
            gate_digest(1, TriState::Deferred, None, Gate::Host)
        );
        assert!(!audit_gate_row(&bad));
    }

    #[test]
    fn gate_row_sample_state_mismatch_rejected() {
        let bad = format!(
            "GATE91|item=1|state=deferred|sample=0|gate=host|digest={:016x}",
            gate_digest(1, TriState::Deferred, Some(0), Gate::Host)
        );
        assert!(!audit_gate_row(&bad));
    }

    #[test]
    fn gate_digest_domains_distinct() {
        assert_ne!(
            gate_digest(1, TriState::Met, Some(0), Gate::Host),
            gate_digest(2, TriState::Met, Some(0), Gate::Host)
        );
        assert_ne!(
            gate_digest(1, TriState::Met, Some(0), Gate::Host),
            gate_digest(1, TriState::Met, None, Gate::Host)
        );
        assert_ne!(
            gate_digest(1, TriState::Met, Some(0), Gate::Host),
            gate_digest(1, TriState::Met, Some(0), Gate::Soak)
        );
        assert_ne!(gate_digest(1, TriState::Met, Some(0), Gate::Host), p_core::FNV_OFF);
    }

    #[test]
    fn row_fields_rejects_wrong_tag_and_arity() {
        let row = render_gate_row(&ITEM_TABLE[2]);
        assert!(row_fields(&row, "GATE91", &GATE_KEYS).is_some());
        assert!(row_fields(&row, "DIM92", &GATE_KEYS).is_none());
        assert!(row_fields(&row, "GATE91", &DIM_KEYS).is_none());
    }

    #[test]
    fn parse_u32_boundaries() {
        assert_eq!(parse_u32("0"), Some(0));
        assert_eq!(parse_u32("4294967295"), Some(u32::MAX));
        assert_eq!(parse_u32("4294967296"), None);
        assert_eq!(parse_u32(""), None);
        assert_eq!(parse_u32("12a"), None);
        assert_eq!(parse_u32("-1"), None);
        assert_eq!(parse_u32(" 1"), None);
    }

    #[test]
    fn parse_hex16_boundaries() {
        assert_eq!(parse_hex16("ffffffffffffffff"), Some(u64::MAX));
        assert_eq!(parse_hex16("0000000000000000"), Some(0));
        assert_eq!(parse_hex16("FFFF"), None);
        assert_eq!(parse_hex16("000000000000000g"), None);
        assert_eq!(parse_hex16(""), None);
    }

    #[test]
    fn dim_table_shape() {
        assert_eq!(dims().len(), DIMS);
        let mut i = 0usize;
        while i < dims().len() {
            assert_eq!(dims()[i].code, (i as u8) + 1);
            assert!(!dims()[i].name.is_empty());
            assert!(dims()[i].owner < 15);
            i += 1;
        }
    }

    #[test]
    fn dim_names_unique() {
        let mut i = 0usize;
        while i < DIMS {
            let mut j = i + 1;
            while j < DIMS {
                assert_ne!(DIM_TABLE[i].name, DIM_TABLE[j].name);
                j += 1;
            }
            i += 1;
        }
    }

    #[test]
    fn dim_lookup_bounds() {
        assert!(dim_for(1).is_some());
        assert!(dim_for(20).is_some());
        assert!(dim_for(0).is_none());
        assert!(dim_for(21).is_none());
    }

    #[test]
    fn dim_weights_sum_to_hundred() {
        assert_eq!(weight_sum(), WEIGHT_TOTAL);
        let mut i = 0usize;
        while i < DIMS {
            assert!(DIM_TABLE[i].weight >= 1 && DIM_TABLE[i].weight <= WEIGHT_MAX);
            i += 1;
        }
    }

    #[test]
    fn dim_targets_and_plans_are_sane() {
        let mut i = 0usize;
        while i < DIMS {
            let d = &DIM_TABLE[i];
            assert!(d.target >= TARGET_FLOOR && d.target <= 100);
            assert!(d.plan >= d.target && d.plan <= 100);
            i += 1;
        }
    }

    #[test]
    fn weighted_scores_meet_depth_floor() {
        assert!(weighted_target() >= WEIGHTED_TARGET_FLOOR);
        assert!(weighted_plan() >= weighted_target());
        assert!(weighted_target() <= 100);
        assert!(weighted_plan() <= 100);
    }

    #[test]
    fn framework_registers() {
        assert!(framework_ok());
        assert!(validate_dims(&DIM_TABLE));
    }

    #[test]
    fn validate_rejects_short_table() {
        let short = &DIM_TABLE[..DIMS - 1];
        assert!(!validate_dims(short));
    }

    #[test]
    fn validate_rejects_weight_bump() {
        let mut v: Vec<Dim> = Vec::new();
        let mut i = 0usize;
        while i < DIMS {
            v.push(DIM_TABLE[i]);
            i += 1;
        }
        v[0].weight = v[0].weight.saturating_add(1);
        assert!(!validate_dims(&v));
    }

    #[test]
    fn validate_rejects_low_plan() {
        let mut v: Vec<Dim> = Vec::new();
        let mut i = 0usize;
        while i < DIMS {
            v.push(DIM_TABLE[i]);
            i += 1;
        }
        v[3].plan = v[3].target.saturating_sub(1);
        assert!(!validate_dims(&v));
    }

    #[test]
    fn validate_rejects_duplicate_code() {
        let mut v: Vec<Dim> = Vec::new();
        let mut i = 0usize;
        while i < DIMS {
            v.push(DIM_TABLE[i]);
            i += 1;
        }
        v[5].code = 1;
        assert!(!validate_dims(&v));
    }

    #[test]
    fn validate_rejects_duplicate_name() {
        let mut v: Vec<Dim> = Vec::new();
        let mut i = 0usize;
        while i < DIMS {
            v.push(DIM_TABLE[i]);
            i += 1;
        }
        v[6].name = "事件层准确";
        assert!(!validate_dims(&v));
    }

    #[test]
    fn validate_rejects_bad_owner() {
        let mut v: Vec<Dim> = Vec::new();
        let mut i = 0usize;
        while i < DIMS {
            v.push(DIM_TABLE[i]);
            i += 1;
        }
        v[7].owner = 15;
        assert!(!validate_dims(&v));
    }

    #[test]
    fn dim_rows_audit_and_stable() {
        let rows = export_dim_rows();
        assert_eq!(rows.len(), DIMS);
        assert!(dim_rows_stable());
        let mut i = 0usize;
        while i < rows.len() {
            assert!(audit_dim_row(&rows[i]));
            i += 1;
        }
    }

    #[test]
    fn dim_row_tamper_rejected() {
        let row = render_dim_row(&DIM_TABLE[0]);
        let mut tamp = row.clone();
        tamp.push('9');
        assert!(!audit_dim_row(&tamp));
        let mut low = row.replacen("plan=99", "plan=97", 1);
        low.push('x');
        assert!(!audit_dim_row(&low));
    }

    #[test]
    fn dim_row_bad_weight_rejected() {
        let bad = format!(
            "DIM92|dim=1|weight=0|target=98|plan=99|digest={:016x}",
            dim_digest(1, 0, 98, 99, 0)
        );
        assert!(!audit_dim_row(&bad));
    }

    #[test]
    fn dim_digest_domains_distinct() {
        assert_ne!(dim_digest(1, 8, 98, 99, 0), dim_digest(2, 8, 98, 99, 0));
        assert_ne!(dim_digest(1, 8, 98, 99, 0), dim_digest(1, 7, 98, 99, 0));
        assert_ne!(dim_digest(1, 8, 98, 99, 0), dim_digest(1, 8, 97, 99, 0));
        assert_ne!(dim_digest(1, 8, 98, 99, 0), p_core::FNV_OFF);
    }

    #[test]
    fn extra_checks_are_eight_and_green() {
        let checks = extra_checks();
        assert_eq!(checks.len(), 8);
        let mut i = 0usize;
        while i < checks.len() {
            assert!(!checks[i].0.is_empty());
            assert!(!checks[i].2.is_empty());
            assert!(checks[i].1, "red: {}", checks[i].0);
            let mut j = i + 1;
            while j < checks.len() {
                assert_ne!(checks[i].0, checks[j].0);
                j += 1;
            }
            i += 1;
        }
    }

    #[test]
    fn checkset_contract() {
        let s = run_unx49091_checks();
        assert!(!s.truncated());
        assert_eq!(s.len(), 8);
        assert_eq!(s.passed_count(), 8);
        assert_eq!(s.failed_count(), 0);
        assert!(s.all_passed());
    }
}
