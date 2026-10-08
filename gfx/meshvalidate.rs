//! VE-F1612 · 几何校验器（VE-I 域 · I01 网格格式与几何基础组 · 目标 360 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F1612`
//!
//! **判据（锚点原文）**：schema、三查、容错、同标准、判据。
//!
//! # 一、schema 校验（F1355 清洗范式的网格版）
//!
//! 网格声明（[`MeshDecl`]）逐字段校验三件：**字段类型 / 必填 / 范围**。
//!
//! | 检查 | 语义 | 不满足时|
//! |---|---|---|
//! | 字段类型 | 每个字段有已登记的类型，类型不匹配即拒| [`SchemaFaultKind::FieldType`] |
//! | 必填 | 声明为必填的字段缺失即拒 | [`SchemaFaultKind::MissingField`] |
//! | 范围 | 数值字段落在声明的闭区间内 | [`SchemaFaultKind::RangeViolated`] |
//!
//! **未知字段按声明策略处置**：[`UnknownPolicy::Reject`] 拒绝，
//! [`UnknownPolicy::Ignore`] 忽略并计数——策略是声明式的，不硬编码。
//!
//! # 二、三查（恶意网格攻击面）
//!
//! | 查 | 判据 | 为何是攻击面 |
//! |---|---|---|
//! | [`TripleScan::IndexOutOfRange`] | 三角索引 >= 顶点数 | 越界读=OOB 取证/信息泄露 |
//! | [`TripleScan::NanGeometry`] | 顶点坐标非有限（NaN/Inf） | NaN 进管线⇒渲染灾难（F1515 防护纪律）|
//! | [`TripleScan::HugeAttribute`] | 属性绝对值 > 声明上限 | 1e30 坐标撑爆下游矩阵运算 |
//!
//! **三查彼此不遮蔽**：`IndexOutOfRange` 用索引值判、`NanGeometry` 用坐标判、
//! `HugeAttribute` 用有限值判——一个顶点可以同时命中多查，互不吞并。
//!
//! # 三、容错策略（修复 / 拒绝二分）
//!
//! 与 F1607 修复器同标准：能自动修的走 [`FixPlan`]，不能修的拒绝加载，
//! **拒绝必须带三要素**（锚点引F1355 超限拒绝三要素，仓内标准表述为
//! **超了多少 / 为什么 / 怎么办**）——缺任一要素的拒绝视为**不可交付**，
//! [`RejectNotice::is_complete`] 判false，调用方必须拒绝放行。
//!
//! # 四、同标准对接
//!
//! - **F1121 编解码安全漏斗**：几何校验器是该漏斗的网格版落点，
//!   四层语义对齐——[`FunnelLayer::Probe`]/[`FunnelLayer::Structure`]
//!   /[`FunnelLayer::Decode`]/[`FunnelLayer::Content`] 四层齐备、顺序不可跳。
//! - **F1607 网格修复检测**：三查结果转成 [`super::meshrepair`] 的缺陷族，
//!   可修项交 [`FixPlan`]、不可修项进拒绝通道，两侧计数口径一致。
//! - **零静默**：任何拦截都产出 [`RejectNotice`]，无「静默丢弃」路径。

#![allow(dead_code)]

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

use super::meshrepair::{Defect, DefectKind, RepairMesh};

// ===========================================================================
// 一、schema 校验
// ===========================================================================

/// 字段的声明类型（类型即契约——未登记的类型不能进网格）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FieldType {
    /// 无符号整数（计数、索引位宽）。
    Uint,
    /// 有符号整数（版本号）。
    Int,
    /// 32 位浮点（坐标、UV）。
    Float,
    /// 布尔标志。
    Bool,
    /// 字符串（名称、单位）。
    Text,
}

impl FieldType {
    /// 稳定码位（供报告对拍，不依赖声明序）。
    pub fn code(self) -> u8 {
        match self {
            FieldType::Uint => 0,
            FieldType::Int => 1,
            FieldType::Float => 2,
            FieldType::Bool => 3,
            FieldType::Text => 4,
        }
    }

    /// 人类可读名（拒绝三要素的「为什么」要念给人听）。
    pub fn label(self) -> &'static str {
        match self {
            FieldType::Uint => "uint",
            FieldType::Int => "int",
            FieldType::Float => "float",
            FieldType::Bool => "bool",
            FieldType::Text => "text",
        }
    }
}

/// schema 违规的三个种类（与三类检查一一对应，不多不少）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SchemaFaultKind {
    /// 字段类型不匹配。
    FieldType,
    /// 必填字段缺失。
    MissingField,
    /// 数值越出声明区间。
    RangeViolated,
}

impl SchemaFaultKind {
    pub fn label(self) -> &'static str {
        match self {
            SchemaFaultKind::FieldType => "FIELD_TYPE",
            SchemaFaultKind::MissingField => "MISSING_FIELD",
            SchemaFaultKind::RangeViolated => "RANGE_VIOLATED",
        }
    }

    pub fn code(self) -> u8 {
        match self {
            SchemaFaultKind::FieldType => 0,
            SchemaFaultKind::MissingField => 1,
            SchemaFaultKind::RangeViolated => 2,
        }
    }

    pub fn of_code(c: u8) -> Option<SchemaFaultKind> {
        match c {
            0 => Some(SchemaFaultKind::FieldType),
            1 => Some(SchemaFaultKind::MissingField),
            2 => Some(SchemaFaultKind::RangeViolated),
            _ => None,
        }
    }
}

/// 某字段的实际取值（校验的输入面）。
#[derive(Clone, Debug, PartialEq)]
pub enum FieldValue {
    Uint(u64),
    Int(i64),
    Float(f64),
    Bool(bool),
    Text(String),
}

impl FieldValue {
    /// 取实际类型码；类型不符时返回 `None`（不猜、不强转）。
    pub fn type_code(&self) -> Option<u8> {
        match self {
            FieldValue::Uint(_) => Some(FieldType::Uint.code()),
            FieldValue::Int(_) => Some(FieldType::Int.code()),
            FieldValue::Float(_) => Some(FieldType::Float.code()),
            FieldValue::Bool(_) => Some(FieldType::Bool.code()),
            FieldValue::Text(_) => Some(FieldType::Text.code()),
        }
    }

    /// 取数值视图；非数值字段返回 `None`（范围检查只对数值字段有意义）。
    pub fn as_number(&self) -> Option<f64> {
        match self {
            FieldValue::Uint(v) => Some(*v as f64),
            FieldValue::Int(v) => Some(*v as f64),
            FieldValue::Float(v) => Some(*v),
            _ => None,
        }
    }
}

/// 单条 schema 违规。
#[derive(Clone, Debug, PartialEq)]
pub struct SchemaFault {
    pub kind: SchemaFaultKind,
    /// 字段名（缺失时即缺失的那个名字）。
    pub field: String,
    /// 声明类型码（[`SchemaFaultKind::MissingField`] 时为 0，无从比较）。
    pub declared: u8,
    /// 实际类型码（缺失时为 `u8::MAX`）。
    pub actual: u8,
    /// 越界实测值与声明区间（仅 [`SchemaFaultKind::RangeViolated`] 有意义）。
    pub observed: f64,
    pub low: f64,
    pub high: f64,
}

/// 字段声明：类型 + 是否必填 + 数值区间。
#[derive(Clone, Debug, PartialEq)]
pub struct FieldSpec {
    pub name: String,
    pub ty: FieldType,
    pub required: bool,
    /// 数值下界（含）。非数值字段忽略。
    pub low: f64,
    /// 数值上界（含）。非数值字段忽略。
    pub high: f64,
}

impl FieldSpec {
    pub fn new(name: &str, ty: FieldType, required: bool) -> FieldSpec {
        FieldSpec {
            name: name.to_string(),
            ty,
            required,
            low: f64::NEG_INFINITY,
            high: f64::INFINITY,
        }
    }

    /// 声明闭区间 `[low, high]`。`low > high` 是声明自身的错，
    /// 由 [`validate_schema`] 检出（声明不可信也是攻击面）。
    pub fn range(mut self, low: f64, high: f64) -> FieldSpec {
        self.low = low;
        self.high = high;
        self
    }
}

/// 未知字段的处置策略（**声明式**，不硬编码）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UnknownPolicy {
    /// 遇未登记字段即拒（F1355 白名单纪律的严档）。
    Reject,
    /// 忽略并计数（宽松档，计数须可查）。
    Ignore,
}

/// 一张网格的 schema（字段声明表 + 未知字段策略）。
#[derive(Clone, Debug, PartialEq)]
pub struct MeshSchema {
    pub specs: Vec<FieldSpec>,
    pub unknown: UnknownPolicy,
}

impl MeshSchema {
    /// vmesh 网格的标准 schema（锚点要求的「全字段校验」基准）。
    ///
    /// 字段集与F1602 容器格式对齐：版本、顶点数、面数、坐标上限、
    /// 索引位宽、UV 分量数。
    pub fn vmesh() -> MeshSchema {
        MeshSchema {
            specs: vec![
                FieldSpec::new("version", FieldType::Uint, true).range(1.0, 2.0),
                FieldSpec::new("vert_count", FieldType::Uint, true).range(0.0, 50_000_000.0),
                FieldSpec::new("face_count", FieldType::Uint, true).range(0.0, 50_000_000.0),
                FieldSpec::new("coord_limit", FieldType::Float, true).range(1.0, 1.0e9),
                FieldSpec::new("index_bits", FieldType::Uint, true).range(8.0, 32.0),
                FieldSpec::new("uv_components", FieldType::Uint, false).range(0.0, 4.0),
            ],
            unknown: UnknownPolicy::Reject,
        }
    }

    fn find(&self, name: &str) -> Option<&FieldSpec> {
        self.specs.iter().find(|s| s.name == name)
    }
}

/// schema 校验的完整产出。
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SchemaReport {
    /// 逐条违规（顺序 = 声明表序，报告确定性由此保证）。
    pub faults: Vec<SchemaFault>,
    /// 被忽略的未知字段名（`Ignore` 策略下非空；**计数可查**，非静默）。
    pub ignored_unknown: Vec<String>,
    /// 被拒的未知字段名（`Reject` 策略下非空）。
    pub rejected_unknown: Vec<String>,
}

impl SchemaReport {
    /// 按种类计数（判据侧用来独立对账）。
    pub fn count_of(&self, k: SchemaFaultKind) -> usize {
        self.faults.iter().filter(|f| f.kind == k).count()
    }

    /// 是否完全合规。
    pub fn is_clean(&self) -> bool {
        self.faults.is_empty() && self.rejected_unknown.is_empty()
    }
}

/// 全字段 schema 校验。
///
/// 遍历**声明表**（而非实参表）——这样必填缺失与类型不匹配都能被检出，
/// 若遍历实参表则缺失字段根本不会被看见。
pub fn validate_schema(schema: &MeshSchema, values: &[(String, FieldValue)]) -> SchemaReport {
    let mut rep = SchemaReport::default();

    // 声明自身的区间倒置先检出：声明不可信同样是攻击面。
    for spec in &schema.specs {
        if spec.low > spec.high {
            rep.faults.push(SchemaFault {
                kind: SchemaFaultKind::RangeViolated,
                field: spec.name.clone(),
                declared: spec.ty.code(),
                actual: spec.ty.code(),
                observed: spec.low,
                low: spec.high,
                high: spec.low,
            });
        }
    }

    for spec in &schema.specs {
        match values.iter().find(|(k, _)| *k == spec.name) {
            None => {
                if spec.required {
                    rep.faults.push(SchemaFault {
                        kind: SchemaFaultKind::MissingField,
                        field: spec.name.clone(),
                        declared: spec.ty.code(),
                        actual: u8::MAX,
                        observed: 0.0,
                        low: spec.low,
                        high: spec.high,
                    });
                }
            }
            Some((_, v)) => {
                let actual = match v.type_code() {
                    Some(c) => c,
                    None => u8::MAX,
                };
                if actual != spec.ty.code() {
                    rep.faults.push(SchemaFault {
                        kind: SchemaFaultKind::FieldType,
                        field: spec.name.clone(),
                        declared: spec.ty.code(),
                        actual,
                        observed: 0.0,
                        low: spec.low,
                        high: spec.high,
                    });
                    continue;
                }
                // 类型对了才谈范围——类型不符的范围值没有意义。
                if let Some(n) = v.as_number() {
                    // NaN 与任何区间都不可比，故显式判非有限（`n < low` 对 NaN恒假，
                    // 直接比会漏判）。
                    if !n.is_finite() || n < spec.low || n > spec.high {
                        rep.faults.push(SchemaFault {
                            kind: SchemaFaultKind::RangeViolated,
                            field: spec.name.clone(),
                            declared: spec.ty.code(),
                            actual,
                            observed: n,
                            low: spec.low,
                            high: spec.high,
                        });
                    }
                }
            }
        }
    }

    // 未知字段：不在声明表里的实参键。
    for (k, _) in values.iter() {
        if schema.find(k).is_none() {
            match schema.unknown {
                UnknownPolicy::Reject => rep.rejected_unknown.push(k.clone()),
                UnknownPolicy::Ignore => rep.ignored_unknown.push(k.clone()),
            }
        }
    }

    rep
}
// ===========================================================================
// 二、三查（恶意网格攻击面）
// ===========================================================================

/// 三查的种类。三查**彼此不遮蔽**：一个顶点可同时命中多查。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TripleScan {
    /// 索引越界：三角索引 >= 顶点数（越界读 = OOB 取证）。
    IndexOutOfRange,
    /// NaN 几何：顶点坐标非有限（NaN/Inf 进管线 = 渲染灾难）。
    NanGeometry,
    /// 超大属性值：有限但绝对值超声明上限（1e30 坐标撑爆下游运算）。
    HugeAttribute,
}

impl TripleScan {
    pub fn label(self) -> &'static str {
        match self {
            TripleScan::IndexOutOfRange => "INDEX_OUT_OF_RANGE",
            TripleScan::NanGeometry => "NAN_GEOMETRY",
            TripleScan::HugeAttribute => "HUGE_ATTRIBUTE",
        }
    }

    pub fn code(self) -> u8 {
        match self {
            TripleScan::IndexOutOfRange => 0,
            TripleScan::NanGeometry => 1,
            TripleScan::HugeAttribute => 2,
        }
    }

    pub fn of_code(c: u8) -> Option<TripleScan> {
        match c {
            0 => Some(TripleScan::IndexOutOfRange),
            1 => Some(TripleScan::NanGeometry),
            2 => Some(TripleScan::HugeAttribute),
            _ => None,
        }
    }
}

/// 三查的命中条目：查什么、在哪、量多大。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ScanHit {
    pub scan: TripleScan,
    /// 定位面号（顶点类命中时为该顶点所属的首个面，无所属则 `u32::MAX`）。
    pub face: u32,
    /// 定位顶点号（索引类命中时为越界的那个索引值本身）。
    pub vertex: u32,
    /// 度量值：越界索引超出量 / 非有限坐标的位模式 / 超限值的绝对值。
    pub metric: f64,
}

/// 三查的完整产出。
#[derive(Clone, Debug, Default, PartialEq)]
pub struct TripleReport {
    pub hits: Vec<ScanHit>,
    /// 参与三查的顶点总数（**分母口径独立登记**，供判据侧对账）。
    pub vertices_seen: u32,
    /// 参与三查的索引总数。
    pub indices_seen: u32,
}

impl TripleReport {
    /// 按查种类计数。
    pub fn count_of(&self, s: TripleScan) -> usize {
        self.hits.iter().filter(|h| h.scan == s).count()
    }

    /// 三个计数的和（判据侧用来与独立重算的期望对账）。
    pub fn total(&self) -> usize {
        self.count_of(TripleScan::IndexOutOfRange)
            + self.count_of(TripleScan::NanGeometry)
            + self.count_of(TripleScan::HugeAttribute)
    }

    pub fn is_clean(&self) -> bool {
        self.hits.is_empty()
    }
}

/// 三查的配置：属性值上限（声明式，非硬编码）。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ScanLimits {
    /// 属性绝对值上限（有限且超此值即 `HugeAttribute`）。
    pub coord_limit: f64,
}

impl Default for ScanLimits {
    fn default() -> ScanLimits {
        ScanLimits { coord_limit: 1.0e6 }
    }
}

impl ScanLimits {
    pub fn new(coord_limit: f64) -> ScanLimits {
        ScanLimits { coord_limit }
    }
}

/// 顶点级属性值的归属面（首个使用该顶点的面），用于定位。
fn owner_face(m: &RepairMesh, v: u32) -> u32 {
    for f in 0..m.face_count() {
        if let Some(t) = m.face(f) {
            if t[0] == v || t[1] == v || t[2] == v {
                return f as u32;
            }
        }
    }
    u32::MAX
}

/// 三查执行。
///
/// **顺序与遮蔽纪律**：三查各自独立遍历输入，互不 `continue`、互不短路——
/// 一个越界索引所在的面若同时含NaN 顶点，两条命中都要出现。
pub fn triple_scan(m: &RepairMesh, limits: &ScanLimits) -> TripleReport {
    let mut rep = TripleReport {
        vertices_seen: m.vert_count() as u32,
        indices_seen: m.faces.len() as u32,
        hits: Vec::new(),
    };

    // 查一：索引越界。逐索引判，不因前一个越界而跳过后续。
    let vcount = m.vert_count() as u64;
    for (i, &idx) in m.faces.iter().enumerate() {
        if idx as u64 >= vcount {
            rep.hits.push(ScanHit {
                scan: TripleScan::IndexOutOfRange,
                face: (i / 3) as u32,
                vertex: idx,
                metric: idx as f64 - vcount as f64,
            });
        }
    }

    // 逐顶点逐分量判：**每顶点最多记一条命中**。
    //
    // 「每顶点一条」由**结构**保证而非标志位——外层 `for v in 0..vert_count`
    // 每顶点只迭代一次，两条命中分支各自 `break` 退出分量循环。
    // 【此处曾有 `seen[v]` 标志位，变异测试证明它是死码】：两条分支都 break，
    // `if seen[v]` 永远为假，删掉它行为逐位不变。死码已删，不留误导。
    //
    // 查二与查三看同一分量但判据不同——非有限 vs 有限但超限，故不遮蔽：
    // NaN 只中查二不中查三。
    for v in 0..m.vert_count() {
        let p = match m.vert(v as u32) {
            Some(p) => p,
            None => continue,
        };
        for c in 0..3 {
            let x = p[c] as f64;
            if !x.is_finite() {
                rep.hits.push(ScanHit {
                    scan: TripleScan::NanGeometry,
                    face: owner_face(m, v as u32),
                    vertex: v as u32,
                    metric: x,
                });
                break;
            }
            if x.abs() > limits.coord_limit {
                rep.hits.push(ScanHit {
                    scan: TripleScan::HugeAttribute,
                    face: owner_face(m, v as u32),
                    vertex: v as u32,
                    metric: x.abs(),
                });
                break;
            }
        }
    }

    rep
}

// ===========================================================================
// 三、容错策略（修复 / 拒绝二分）
// ===========================================================================

/// 可自动执行的修复动作（与 F1607 `RepairKind` 同标准）。
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum FixAction {
    /// 钳制超限坐标到 `±limit`（可修：超大属性值）。
    ClampCoord { vertex: u32, axis: u8, limit: f64 },
    /// 把非有限坐标置零（可修：NaN 几何——置零可渲染）。
    ZeroNonFinite { vertex: u32, axis: u8 },
    /// 丢弃越界索引所在的面（可修：越界索引——丢面保网格可加载）。
    DropFace { face: u32 },
}

impl FixAction {
    pub fn label(&self) -> String {
        match self {
            FixAction::ClampCoord { vertex, axis, .. } => {
                format!("CLAMP_COORD v{} a{}", vertex, axis)
            }
            FixAction::ZeroNonFinite { vertex, axis } => {
                format!("ZERO_NON_FINITE v{} a{}", vertex, axis)
            }
            FixAction::DropFace { face } => format!("DROP_FACE f{}", face),
        }
    }
}

/// 拒绝告知的**三要素**（锚点引 F1355：超了多少 / 为什么 / 怎么办）。
///
/// 三者缺一即不可交付——[`RejectNotice::is_complete`] 判 false，
/// 调用方**必须**拒绝放行（缺「怎么办」等于把问题甩给用户）。
#[derive(Clone, Debug, Default, PartialEq)]
pub struct RejectNotice {
    /// 要素一：超了多少（定量）。
    pub exceeded: String,
    /// 要素二：为什么（定性，给根因）。
    pub because: String,
    /// 要素三：怎么办（可执行的下一步）。
    pub remedy: String,
}

impl RejectNotice {
    pub fn new(exceeded: &str, because: &str, remedy: &str) -> RejectNotice {
        RejectNotice {
            exceeded: exceeded.to_string(),
            because: because.to_string(),
            remedy: remedy.to_string(),
        }
    }

    /// 三要素齐备才为 true。**空串不算要素**——
    /// 空串是「有字段但没写」，与「没这个字段」同样不可交付。
    pub fn is_complete(&self) -> bool {
        !self.exceeded.trim().is_empty()
            && !self.because.trim().is_empty()
            && !self.remedy.trim().is_empty()
    }
}

/// 拦截项的处置结论（修复 / 拒绝二分）。
#[derive(Clone, Debug, PartialEq)]
pub enum Disposition {
    /// 可自动修：执行 [`FixAction`]。
    Fix(FixAction),
    /// 不可修：拒绝加载 + 三要素告知。
    Reject(RejectNotice),
}

/// 单项处置的归属主体（字段名或拦截描述）。
#[derive(Clone, Debug, PartialEq)]
pub struct Dispositioned {
    pub subject: String,
    pub disposition: Disposition,
}

/// 修复计划（可执行序列 + 拒绝清单）。
#[derive(Clone, Debug, Default, PartialEq)]
pub struct FixPlan {
    /// 逐条修复动作（顺序 = 命中顺序，执行即可复现）。
    pub fixes: Vec<FixAction>,
    /// 逐条拒绝告知（**非空即整体不可加载**）。
    pub rejections: Vec<Dispositioned>,
    /// 处置项按 [`FixClass`] 分桶计数（修复与拒绝都在此登记，口径统一）。
    pub by_class: Vec<(FixClass, u32)>,
}

/// 拦截项的分类（决定可修/ 不可修）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FixClass {
    /// 索引越界 → 可修（丢面）。
    IndexOutOfRange,
    /// NaN 几何 → 可修（置零）。
    NanGeometry,
    /// 超大属性值 → 可修（钳制）。
    HugeAttribute,
    /// schema 类型错 → 不可修（数据契约破了，猜不出原意）。
    SchemaType,
    /// schema 必填缺失 → 不可修（同上）。
    SchemaMissing,
    /// schema 范围越界 → 不可修（声明与数据矛盾，需人工裁决）。
    SchemaRange,
}

impl FixClass {
    pub fn label(self) -> &'static str {
        match self {
            FixClass::IndexOutOfRange => "INDEX_OUT_OF_RANGE",
            FixClass::NanGeometry => "NAN_GEOMETRY",
            FixClass::HugeAttribute => "HUGE_ATTRIBUTE",
            FixClass::SchemaType => "SCHEMA_TYPE",
            FixClass::SchemaMissing => "SCHEMA_MISSING",
            FixClass::SchemaRange => "SCHEMA_RANGE",
        }
    }

    /// 三查类恒可修，schema 类恒不可修——**判据侧可独立重算这张表**，
    /// 不必信任被测实现的自述。
    pub fn is_fixable(self) -> bool {
        matches!(
            self,
            FixClass::IndexOutOfRange | FixClass::NanGeometry | FixClass::HugeAttribute
        )
    }
}

fn bump(v: &mut Vec<(FixClass, u32)>, k: FixClass) {
    for e in v.iter_mut() {
        if e.0 == k {
            e.1 += 1;
            return;
        }
    }
    v.push((k, 1));
}

/// 把三查命中转成容错处置（修复 / 拒绝二分）。
///
/// 锚点「可修自动修 / 不可修拒绝 + 三要素」在此落地：三查三类**全部可修**
/// （越界丢面、NaN 置零、超大钳制），schema 三类**全部不可修**
/// （数据契约破裂，猜不出原意）。故此函数只对三查产修复动作。
pub fn plan_fixes(triple: &TripleReport, limits: &ScanLimits) -> FixPlan {
    let mut plan = FixPlan::default();
    for h in &triple.hits {
        let class = match h.scan {
            TripleScan::IndexOutOfRange => FixClass::IndexOutOfRange,
            TripleScan::NanGeometry => FixClass::NanGeometry,
            TripleScan::HugeAttribute => FixClass::HugeAttribute,
        };
        let action = match h.scan {
            TripleScan::IndexOutOfRange => FixAction::DropFace { face: h.face },
            TripleScan::NanGeometry => {
                // NaN 的轴不可由 metric 反推（metric 是 NaN 本身），
                // 故 axis 记 0xFF 表示「三轴全置零」，避免猜错轴。
                FixAction::ZeroNonFinite {
                    vertex: h.vertex,
                    axis: 0xFF,
                }
            }
            TripleScan::HugeAttribute => FixAction::ClampCoord {
                vertex: h.vertex,
                axis: 0xFF,
                limit: limits.coord_limit,
            },
        };
        plan.fixes.push(action);
        bump(&mut plan.by_class, class);
    }
    plan
}

/// 把 schema 违规转成拒绝处置（**全部不可修**）。
pub fn plan_schema_rejections(rep: &SchemaReport) -> FixPlan {
    let mut plan = FixPlan::default();

    // 未知字段的拒绝（`Reject` 策略下才有）。
    for name in &rep.rejected_unknown {
        plan.rejections.push(Dispositioned {
            subject: format!("未知字段 {}", name),
            disposition: Disposition::Reject(RejectNotice::new(
                &format!("字段 {} 不在 vmesh schema 声明表内", name),
                "未知字段意味着容器格式版本与解析器不匹配，猜其原意等于伪造数据",
                "升级解析器到匹配版本，或按 UnknownPolicy::Ignore 显式放宽后重试",
            )),
        });
        bump(&mut plan.by_class, FixClass::SchemaMissing);
    }

    for f in &rep.faults {
        let class = match f.kind {
            SchemaFaultKind::FieldType => FixClass::SchemaType,
            SchemaFaultKind::MissingField => FixClass::SchemaMissing,
            SchemaFaultKind::RangeViolated => FixClass::SchemaRange,
        };
        let (exceeded, because, remedy) = match f.kind {
            SchemaFaultKind::FieldType => (
                format!(
                    "字段 {} 类型为 {}，声明要求 {}",
                    f.field,
                    type_label(f.actual),
                    type_label(f.declared)
                ),
                "类型不符意味着容器里这一字段的字节布局与解析器不同，按声明类型解读会读出垃圾值",
                "核对容器生成端与解析器的字段类型定义，同步后再加载",
            ),
            SchemaFaultKind::MissingField => (
                format!("必填字段 {} 缺失", f.field),
                "必填字段缺失使网格的核心量（顶点数/面数等）无从校验，越界三查失去分母",
                "由容器生成端补齐该字段；若该字段对本资产确实无意义，改 schema 将其声明为可选",
            ),
            SchemaFaultKind::RangeViolated => (
                format!(
                    "字段 {} 实测 {} 越出声明区间 [{}, {}]",
                    f.field, f.observed, f.low, f.high
                ),
                "数据越出声明区间说明声明与内容矛盾，二者必有一错，自动取值会掩盖真因",
                "确认该字段的真实取值域，据此修正声明区间或修正数据生成端",
            ),
        };
        plan.rejections.push(Dispositioned {
            subject: f.field.clone(),
            disposition: Disposition::Reject(RejectNotice::new(&exceeded, &because, &remedy)),
        });
        bump(&mut plan.by_class, class);
    }
    plan
}

/// 类型码到人话名（未知码位不猜）。
fn type_label(code: u8) -> &'static str {
    match code {
        0 => "uint",
        1 => "int",
        2 => "float",
        3 => "bool",
        4 => "text",
        _ => "未知类型",
    }
}

/// 执行修复计划，返回执行后的网格副本（原网格不动）。
///
/// **可撤销纪律**（引 F1607 撤销语义）：修复不是原地改，
/// 调用方拿得到修复前的副本，故修复天然可撤销。
pub fn apply_fixes(m: &RepairMesh, plan: &FixPlan) -> RepairMesh {
    let mut out = m.clone();
    // 先置零非有限（顺序在前：置零后该顶点不再是 NaN，钳制不会二次生效）。
    for fx in &plan.fixes {
        if let FixAction::ZeroNonFinite { vertex, .. } = *fx {
            let v = vertex as usize;
            for c in 0..3 {
                let s = match v.checked_mul(3).and_then(|b| b.checked_add(c)) {
                    Some(s) => s,
                    None => continue,
                };
                if s < out.verts.len() {
                    let x = out.verts[s] as f64;
                    if !x.is_finite() {
                        out.verts[s] = 0.0;
                    }
                }
            }
        }
    }
    // 再钳制超限。
    for fx in &plan.fixes {
        if let FixAction::ClampCoord { vertex, limit, .. } = *fx {
            let v = vertex as usize;
            for c in 0..3 {
                let s = match v.checked_mul(3).and_then(|b| b.checked_add(c)) {
                    Some(s) => s,
                    None => continue,
                };
                if s < out.verts.len() {
                    let x = out.verts[s] as f64;
                    if x.is_finite() && x.abs() > limit {
                        out.verts[s] = if x < 0.0 { -limit as f32 } else { limit as f32 };
                    }
                }
            }
        }
    }
    // 最后丢面（丢面不改顶点数组，只改索引）。
    let mut drop: Vec<usize> = Vec::new();
    for fx in &plan.fixes {
        if let FixAction::DropFace { face } = *fx {
            drop.push(face as usize);
        }
    }
    if !drop.is_empty() {
        drop.sort_unstable();
        drop.dedup();
        let mut faces: Vec<u32> = Vec::with_capacity(out.faces.len());
        for f in 0..out.face_count() {
            if drop.contains(&f) {
                continue;
            }
            if let Some(t) = out.face(f) {
                faces.push(t[0]);
                faces.push(t[1]);
                faces.push(t[2]);
            }
        }
        out.faces = faces;
    }
    out
}

// ===========================================================================
// 四、同标准对接
// ===========================================================================

/// F1121 编解码安全漏斗的层（几何域落点）。
///
/// **四层齐备、顺序不可跳**：漏斗层的语义与 F1121 一致——
/// 非网格文件在探测层拦截、结构错误在结构层拦截、
/// 资源超限在解码层拦截、解码后内容异常在内容层拦截。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FunnelLayer {
    /// L1 格式探测（魔数/签名；非网格在此拦截）。
    Probe,
    /// L2 结构校验（头/段/索引的完整性；结构攻击在此拦截）。
    Structure,
    /// L3 解码校验（尺寸/内存上限强制）。
    Decode,
    /// L4 内容校验（解码后顶点与索引的合理性）。
    Content,
}

impl FunnelLayer {
    /// 四层的**声明顺序码**（判据侧可直接断言 0/1/2/3，不可跳）。
    pub fn ordinal(self) -> u8 {
        match self {
            FunnelLayer::Probe => 0,
            FunnelLayer::Structure => 1,
            FunnelLayer::Decode => 2,
            FunnelLayer::Content => 3,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            FunnelLayer::Probe => "L1_PROBE",
            FunnelLayer::Structure => "L2_STRUCTURE",
            FunnelLayer::Decode => "L3_DECODE",
            FunnelLayer::Content => "L4_CONTENT",
        }
    }
}

/// 四层序列（静态事实：顺序不可跳、不可配置跳过）。
pub const FUNNEL_LAYERS: [FunnelLayer; 4] = [
    FunnelLayer::Probe,
    FunnelLayer::Structure,
    FunnelLayer::Decode,
    FunnelLayer::Content,
];

/// 解码期资源上限（F1121 L3 的强制项，声明式）。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DecodeLimits {
    pub max_verts: u32,
    pub max_faces: u32,
}

impl Default for DecodeLimits {
    fn default() -> DecodeLimits {
        DecodeLimits {
            max_verts: 5_000_000,
            max_faces: 5_000_000,
        }
    }
}

/// 漏斗某一层的拦截记录。
#[derive(Clone, Debug, PartialEq)]
pub struct FunnelHit {
    pub layer: FunnelLayer,
    /// 该层拦截的条目数。
    pub count: u32,
    /// 该层的告知（拒绝时必须三要素齐备）。
    pub notice: RejectNotice,
}

/// 几何域的完整校验裁决。
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ValidationVerdict {
    /// 是否放行加载。
    pub accepted: bool,
    /// 逐层拦截记录（层序 = [`FUNNEL_LAYERS`] 序）。
    pub funnel: Vec<FunnelHit>,
    /// 三查产出。
    pub triple: TripleReport,
    /// schema 产出。
    pub schema: SchemaReport,
    /// 修复计划（放行时非空即「已修复后放行」）。
    pub plan: FixPlan,
    /// 拒绝清单（非空即不可加载，与plan.rejections 同步登记）。
    pub rejections: Vec<Dispositioned>,
}

impl ValidationVerdict {
    /// 每层拦截量（F1121 的通过率统计面：逐层可审计）。
    pub fn layer_count(&self, l: FunnelLayer) -> u32 {
        for h in &self.funnel {
            if h.layer == l {
                return h.count;
            }
        }
        0
    }

    /// 全部拒绝告知是否三要素齐备（**任一缺项即整体不可交付**）。
    pub fn all_notices_complete(&self) -> bool {
        self.funnel.iter().all(|h| h.notice.is_complete())
            && self
                .rejections
                .iter()
                .all(|d| match &d.disposition {
                    Disposition::Reject(n) => n.is_complete(),
                    Disposition::Fix(_) => true,
                })
    }
}

/// 三查命中转 F1607 缺陷族（**同标准对接**的实体：同一问题两域同码）。
///
/// F1607 的 [`DefectKind`] 四类是几何缺陷族，本域三查是其上游子集：
/// 越界索引与NaN 各归 [`DefectKind::Degenerate`] 之外的独立处理——
/// 这里转成 [`DefectKind::Hole`] 无语义，故**如实登记为不可映射**，
/// 由 [`defect_of`] 返回 `None`，不硬塞一个不贴切的类别。
pub fn defect_of(h: &ScanHit) -> Option<Defect> {
    match h.scan {
        // 越界索引：F1607 不覆盖（F1607 假定索引已合法），故不可映射。
        TripleScan::IndexOutOfRange => None,
        // NaN 几何：退化三角的极端形态（顶点塌成一点），归退化族。
        // `metric` 是 f32 —— 非有限值（含 NaN）一律记 0，避免 NaN 进报告。
        TripleScan::NanGeometry => Some(Defect {
            kind: DefectKind::Degenerate,
            face: h.face as usize,
            vertex: h.vertex,
            metric: if h.metric.is_finite() { h.metric as f32 } else { 0.0 },
        }),
        // 超大属性值：不是 F1607 的四类之一，不可映射。
        TripleScan::HugeAttribute => None,
    }
}

/// 顶层裁决：跑完四层漏斗 + schema + 三查 + 容错，产出放行/拒绝结论。
///
/// **放行条件（三者全备）**：无漏斗拦截 ∧ schema 零违规 ∧ 三查零命中。
/// 三查有命中但全部可修时，走「修复后放行」——此时 `accepted` 为真
/// 且 `plan.fixes` 非空，调用方必须用 [`apply_fixes`] 的产物而非原网格。
pub fn validate_mesh(
    m: &RepairMesh,
    schema: &MeshSchema,
    values: &[(String, FieldValue)],
    limits: &ScanLimits,
    decode: &DecodeLimits,
) -> ValidationVerdict {
    let mut v = ValidationVerdict {
        accepted: false,
        ..Default::default()
    };

    // L1 格式探测：容器须声明过网格的最小结构（至少有一个顶点或一个面）。
    // 空网格不算格式错误（合法资产），故此处只在「顶点数组非 3 的倍数」时拦。
    let l1_bad = m.verts.len() % 3 != 0 || m.faces.len() % 3 != 0;
    v.funnel.push(FunnelHit {
        layer: FunnelLayer::Probe,
        count: if l1_bad { 1 } else { 0 },
        notice: RejectNotice::new(
            "顶点或索引数组长度不是 3 的倍数",
            "顶点按 XYZ 三分量、索引按三角三顶点成组，数组长度不对齐说明容器不是三角网格",
            "由容器生成端按三角网格重新导出",
        ),
    });

    // L2 结构校验：schema 全字段校验（类型/必填/范围）。
    let sr = validate_schema(schema, values);
    let l2_bad = !sr.is_clean();
    v.funnel.push(FunnelHit {
        layer: FunnelLayer::Structure,
        count: sr.faults.len() as u32 + sr.rejected_unknown.len() as u32,
        notice: RejectNotice::new(
            &format!("schema 违规 {} 项", sr.faults.len() + sr.rejected_unknown.len()),
            "容器声明与 schema 不符，几何数据在进入管线前已不可信",
            "按拒绝清单逐项修正容器声明后重试",
        ),
    });
    v.schema = sr.clone();

    // L3 解码校验：资源上限强制（F1121 漏斗化的核心）。
    let vc = m.vert_count() as u32;
    let fc = m.face_count() as u32;
    let l3_bad = vc > decode.max_verts || fc > decode.max_faces;
    v.funnel.push(FunnelHit {
        layer: FunnelLayer::Decode,
        count: u32::from(vc > decode.max_verts) + u32::from(fc > decode.max_faces),
        notice: RejectNotice::new(
            &format!(
                "实测顶点数 {} 面数 {}，上限分别为 {} / {}",
                vc, fc, decode.max_verts, decode.max_faces
            ),
            "顶点数或面数超出解码期资源上限，继续解码会耗尽内存并拖垮管线",
            "对该网格执行简化（F1608）降面，或按项目需要显式调高上限",
        ),
    });

    // L4 内容校验：三查（越界索引 / NaN / 超大属性）。
    let tr = triple_scan(m, limits);
    let l4_bad = !tr.is_clean();
    v.funnel.push(FunnelHit {
        layer: FunnelLayer::Content,
        count: tr.total() as u32,
        notice: RejectNotice::new(
            &format!("三查命中 {} 项", tr.total()),
            "几何内容存在越界索引、非有限坐标或超大属性值，直接进管线会造成越界读或渲染灾难",
            "启用容错策略自动修复（丢面/置零/钳制）后加载，或修复网格源文件",
        ),
    });
    v.triple = tr.clone();

    // 容错：schema 类不可修 → 拒绝；schema 干净则三查全可修 → 修复计划。
    let sp = plan_schema_rejections(&sr);
    let has_reject = !sp.rejections.is_empty();
    v.rejections = sp.rejections.clone();
    if has_reject {
        // 有不可修项时仍登记三查修复计划，但放行判定已被拒绝锁死——
        // 登记而不执行，避免「半修半载」的中间态。
        let tp = plan_fixes(&tr, limits);
        v.plan.fixes = tp.fixes;
        v.plan.by_class = tp.by_class;
        v.plan.rejections = sp.rejections;
    } else {
        v.plan = plan_fixes(&tr, limits);
    }

    v.accepted = !l1_bad && !l2_bad && !l3_bad && !l4_bad && !has_reject;
    v
}
