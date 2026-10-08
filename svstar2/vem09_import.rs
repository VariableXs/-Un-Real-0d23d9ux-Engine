//! VE-F2409 · 动画导入（glTF animation 采样导入 · 目标 340 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F2409`
//!
//! **判据（锚点原文）**：四通道映射、三重校验、保真默认、结构化报告、判据。
//!
//! 本条是 M 域的**入口面**：把一份 glTF animation（采样 + 通道）搬进 M 域轨道
//! 容器（F2402）。它**不做求值**（F2407）、不做插值语义裁决（F2403/F2423）、
//! 不做压缩（F2425）——只负责「把外部资产搬进来，搬得干净、搬得可核对、
//! 搬坏了说清楚哪一块坏了」。
//!
//! 1. **四通道映射**（判据一）。映射表**公开且可对账**：
//!    `translation → 位置轨`、`rotation → 四元数轨（slerp 标记，F2423 前向）`、
//!    `scale → 缩放轨`、`weights → 形态键权重轨（F2441 前向对接登记）`。
//!    - **映射表不是散在代码里的 `match`**，而是 `MappingTable` 这个数据结构：
//!      注册时写入摘要（`declared`），`reconcile()` 重算比对。漂移的后果是
//!      **拦截**（`intercepted`）而不是记一笔——漂移的映射表会把 rotation
//!      映成 scale，那比拒绝导入更糟。
//!    - **四通道外通道（自定义通道）→ 跳过 + 声明**（支持范围诚实）：跳过必须
//!      留下指名道姓的声明，否则调用方以为「全通道都导进来了」。
//!
//! 2. **三重校验**（判据二）。复用 I01 F1612 校验标准在动画维的适用，三重各有
//!    **独立拒绝码**——这是刻意的：三重若共用一个码，通道引用失效与采样非法就
//!    重合了，「哪一重拦下来的」这个问题不可回答，fuzz（F2411）拿不到分流信息。
//!    - 重一 **通道引用完整性**：sampler / input accessor / output accessor /
//!      目标节点四类引用逐一在域内；越界即跳过该通道并记码。
//!    - 重二 **采样数据合法性**：时间单调非递减、值数 = 关键帧数 × 分量数、
//!      空采样、旋转分量全零（不可归一化）。
//!    - 重三 **曲线异常**：值 NaN/Inf（**钳制 + 警告**，不是丢帧——丢帧会静默
//!      改变动画长度）；时刻 NaN/Inf（**丢该帧**，时刻无法钳制，钳到 0 会把
//!      末帧叠到首帧上）；超密（相邻帧零间隔，成因见下条量化纪律）。
//!
//! 3. **保真默认**（判据三）。`Fidelity::Faithful`（默认）逐关键帧原样落地，
//!    `Fidelity::Simplified { epsilon }` 才做误差阈值抽帧（F2425 联动位）。
//!    - **为什么用 enum 而不是 bool `simplify`**：简化必须带阈值，一个裸
//!      `bool: true` 没有可复现的语义——同一份资产两次导入可能抽出不同的帧。
//!      阈值与保真是**同一个语义参数的两态**（F2278 纪律）。
//!    - **秒→毫秒量化会造出重复时刻**，这是超密的主因：glTF 输入是秒（f32），
//!      M 域是毫秒（u32），`round(0.0004 * 1000) == round(0.0006 * 1000) == 0`。
//!      重复时刻不违反「非递减」（F2407 的二分前提仍成立），但它是**资产侧的
//!      真实信息**：作者确实写了两帧。所以本条**计数并警告，不静默合并**。
//!
//! 4. **结构化报告**（判据四）。三要素齐备：轨道数 / 通道映射表 / 警告清单。
//!    另有**三要素拒绝**（锚点错误矩阵「glTF 格式非法 → 三要素拒绝」）：
//!    `ImportError { code, locator, hint }`——什么错 / 在哪 / 怎么办，
//!    `three_elements_complete()` 把「三要素齐备」变成可机检事实。
//!
//! 5. **保真默认的第二条纪律：零指纹**。锚点写明「导入清洗含隐私红线
//!    （glTF 资产零指纹——家族）」。落地方式不是写一句承诺，而是**轨道名与
//!    警告文本都从「语义 + 节点下标」构造，绝不拼入资产自带的名字/路径**。
//!    判据用「资产标签非空且逐条轨道名/警告文本均不含它」来钉——负向断言必须
//!    有一个**非空的被排除串**，否则断言恒真。
//!
//! **与 I01 的边界**：F1602 负责字节级解码（GLB chunk / base64 / accessor
//! 交错），本条只收**解码后的 f32 视图 + 声明元数据**（componentType /
//! normalized / count / comps），并校验**元数据与视图自洽**。整数 accessor 的
//! 反归一化（`i16 / 32767`）按 glTF 规范在**解码侧**完成，本条校验的是
//! 「声明说它是 normalized、而视图已是反归一化后的值」这件事不被绕过：
//! 声明 `normalized=false` 却给了整数语义的值域，是本条拦的（`VALUE_OUT_OF_DOMAIN`）。
//!
//! 跨批对接：F1602/F1612（格式联动与校验标准单源）；F2402 轨道容器（落地目标）；
//!   F2403 `Interp`（插值标记单源，**不另造插值器**）；F2407 `SoaTrack`
//!   （SoA 布局单源，F2202 家族）；F2423 slerp 前向对接；F2425 精简联动位；
//!   F2441 morph 轨前向对接登记；F2411 fuzz（校验对抗面）。
//!
//! 零 panic 面、零 IO、无全局可变状态（所有状态由调用方持有并显式传入）。

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

use super::vem03_interp::Interp;
use super::vem07_perf::{SoaTrack, TrackClass, TrackValueKind};

// ---------------------------------------------------------------------------
// 一、诊断家族（自有码段 0x2Cxx；0x2Axx 归 F2407、0x2Bxx 归 F2408、6001 段归 F2406）
// ---------------------------------------------------------------------------

/// 诊断码。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DiagCode(pub u16);

impl DiagCode {
    // —— 重一：通道引用完整性（四个引用四码，绝不共用）——
    /// sampler 下标越界。
    pub const SAMPLER_OOR: DiagCode = DiagCode(0x2C01);
    /// input accessor 下标越界。
    pub const INPUT_ACCESSOR_OOR: DiagCode = DiagCode(0x2C02);
    /// output accessor 下标越界。
    pub const OUTPUT_ACCESSOR_OOR: DiagCode = DiagCode(0x2C03);
    /// 目标节点下标越界（节点不存在）。
    pub const TARGET_NODE_MISSING: DiagCode = DiagCode(0x2C04);

    // —— 重二：采样数据合法性 ——
    /// 空采样（时间轴或值视图皆空）。
    pub const SAMPLER_EMPTY: DiagCode = DiagCode(0x2C05);
    /// 时刻非单调（存在 t[i] < t[i-1]）。
    pub const TIMES_NON_MONOTONIC: DiagCode = DiagCode(0x2C06);
    /// 值数与「关键帧数 × 分量数」不自洽。
    pub const VALUE_COUNT_MISMATCH: DiagCode = DiagCode(0x2C07);
    /// 旋转分量全零（不可归一化的退化四元数）。
    pub const QUAT_DEGENERATE: DiagCode = DiagCode(0x2C08);
    /// 值域不合理（如 scale 分量为非有限、或归一化声明与值域冲突）。
    pub const VALUE_OUT_OF_DOMAIN: DiagCode = DiagCode(0x2C09);

    // —— 重三：曲线异常 ——
    /// 采样值非有限（NaN/Inf），已钳制。
    pub const VALUE_NON_FINITE: DiagCode = DiagCode(0x2C0A);
    /// 采样时刻非有限（NaN/Inf），该帧已丢（时刻不可钳制）。
    pub const TIME_NON_FINITE: DiagCode = DiagCode(0x2C0B);
    /// 超密：相邻关键帧量化后零间隔（秒→毫秒取整的必然产物）。
    pub const OVERDENSE_KEYS: DiagCode = DiagCode(0x2C0C);
    /// 负时刻（glTF 允许负的输入时间；本域时间轴从 0 起）。
    pub const NEGATIVE_TIME: DiagCode = DiagCode(0x2C0D);

    // —— 映射与声明 ——
    /// 四通道外通道（四通道外自定义路径），已跳过。
    pub const PATH_OUT_OF_SCOPE: DiagCode = DiagCode(0x2C0E);
    /// 映射表漂移：重算摘要与注册时声明不符。
    pub const MAPPING_DRIFT: DiagCode = DiagCode(0x2C0F);
    /// 在映射表拦截态下强行导入。
    pub const MAPPING_INTERCEPTED: DiagCode = DiagCode(0x2C10);
    /// CUBICSPLINE 的切线分量不落进 SoA 轨道（本域轨道无切线存储位）。
    pub const CUBIC_TANGENT_DROPPED: DiagCode = DiagCode(0x2C11);
    /// glTF 格式非法，三要素拒绝。
    pub const DOC_MALFORMED: DiagCode = DiagCode(0x2C12);
    /// 导入在拦截态下整体失败（映射表漂移的必然后果）。
    pub const IMPORT_ABORTED: DiagCode = DiagCode(0x2C13);

    /// 人话标签。
    pub const fn label(self) -> &'static str {
        match self {
            DiagCode::SAMPLER_OOR => "采样器下标越界，通道已跳过",
            DiagCode::INPUT_ACCESSOR_OOR => "输入 accessor 下标越界，通道已跳过",
            DiagCode::OUTPUT_ACCESSOR_OOR => "输出 accessor 下标越界，通道已跳过",
            DiagCode::TARGET_NODE_MISSING => "目标节点不存在，通道已跳过",
            DiagCode::SAMPLER_EMPTY => "采样为空，通道已跳过",
            DiagCode::TIMES_NON_MONOTONIC => "关键帧时刻非单调，通道已拒绝",
            DiagCode::VALUE_COUNT_MISMATCH => "采样值数与帧数乘分量不自洽，通道已拒绝",
            DiagCode::QUAT_DEGENERATE => "旋转四元数退化（全零），该帧回退为单位四元数",
            DiagCode::VALUE_OUT_OF_DOMAIN => "采样值域不合理，已拒绝该通道",
            DiagCode::VALUE_NON_FINITE => "采样值非有限，已钳制并警告",
            DiagCode::TIME_NON_FINITE => "采样时刻非有限，该帧已丢弃",
            DiagCode::OVERDENSE_KEYS => "关键帧超密（量化后零间隔），已计数警告",
            DiagCode::NEGATIVE_TIME => "关键帧时刻为负，已钳制到 0",
            DiagCode::PATH_OUT_OF_SCOPE => "四通道外自定义通道，已跳过并留下声明",
            DiagCode::MAPPING_DRIFT => "映射表漂移，已置拦截",
            DiagCode::MAPPING_INTERCEPTED => "映射表处于拦截态，导入被拒",
            DiagCode::CUBIC_TANGENT_DROPPED => "CUBICSPLINE 切线分量未落进轨道，已声明",
            DiagCode::DOC_MALFORMED => "glTF 文档结构非法，三要素拒绝",
            DiagCode::IMPORT_ABORTED => "导入整体中止（前置校验不通过）",
            other => {
                let _ = other;
                "未登记诊断码"
            }
        }
    }

    /// 全部码（供家族完整性判据）。
    pub const ALL: [DiagCode; 19] = [
        DiagCode::SAMPLER_OOR,
        DiagCode::INPUT_ACCESSOR_OOR,
        DiagCode::OUTPUT_ACCESSOR_OOR,
        DiagCode::TARGET_NODE_MISSING,
        DiagCode::SAMPLER_EMPTY,
        DiagCode::TIMES_NON_MONOTONIC,
        DiagCode::VALUE_COUNT_MISMATCH,
        DiagCode::QUAT_DEGENERATE,
        DiagCode::VALUE_OUT_OF_DOMAIN,
        DiagCode::VALUE_NON_FINITE,
        DiagCode::TIME_NON_FINITE,
        DiagCode::OVERDENSE_KEYS,
        DiagCode::NEGATIVE_TIME,
        DiagCode::PATH_OUT_OF_SCOPE,
        DiagCode::MAPPING_DRIFT,
        DiagCode::MAPPING_INTERCEPTED,
        DiagCode::CUBIC_TANGENT_DROPPED,
        DiagCode::DOC_MALFORMED,
        DiagCode::IMPORT_ABORTED,
    ];

    /// 是否属「通道引用完整性」重（判据用它证明三重各有独立码域）。
    pub const fn is_ref_class(self) -> bool {
        matches!(
            self,
            DiagCode::SAMPLER_OOR
                | DiagCode::INPUT_ACCESSOR_OOR
                | DiagCode::OUTPUT_ACCESSOR_OOR
                | DiagCode::TARGET_NODE_MISSING
        )
    }

    /// 是否属「采样数据合法性」重。
    pub const fn is_sample_class(self) -> bool {
        matches!(
            self,
            DiagCode::SAMPLER_EMPTY
                | DiagCode::TIMES_NON_MONOTONIC
                | DiagCode::VALUE_COUNT_MISMATCH
                | DiagCode::QUAT_DEGENERATE
                | DiagCode::VALUE_OUT_OF_DOMAIN
        )
    }

    /// 是否属「曲线异常」重。
    pub const fn is_curve_class(self) -> bool {
        matches!(
            self,
            DiagCode::VALUE_NON_FINITE
                | DiagCode::TIME_NON_FINITE
                | DiagCode::OVERDENSE_KEYS
                | DiagCode::NEGATIVE_TIME
        )
    }
}

/// 诊断严重度。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Severity {
    /// 记账，不阻断。
    Minor,
    /// 显性告警：条件成立即记录，无需人工介入。
    Major,
    /// 立案：需要人看一眼。
    P1,
}

/// 一条诊断。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Diagnostic {
    /// 诊断码。
    pub code: DiagCode,
    /// 严重度。
    pub severity: Severity,
}

/// 诊断袋（**P1 是可查询的一等公民**，不另立类型）。
#[derive(Clone, Debug, Default)]
pub struct DiagBag {
    items: Vec<Diagnostic>,
}

impl DiagBag {
    /// 新建空袋。
    #[allow(clippy::new_without_default)]
    pub fn new() -> DiagBag {
        DiagBag { items: Vec::new() }
    }

    /// 记一条（记账级）。
    pub fn push(&mut self, code: DiagCode) {
        self.items.push(Diagnostic { code, severity: Severity::Minor });
    }

    /// 记一条显性告警。
    pub fn push_major(&mut self, code: DiagCode) {
        self.items.push(Diagnostic { code, severity: Severity::Major });
    }

    /// 记一条立案。
    pub fn push_p1(&mut self, code: DiagCode) {
        self.items.push(Diagnostic { code, severity: Severity::P1 });
    }

    /// 全部诊断。
    pub fn items(&self) -> &[Diagnostic] {
        &self.items
    }

    /// 条数。
    pub fn len(&self) -> usize {
        self.items.len()
    }

    /// 是否为空。
    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    /// 是否出现过某码。
    pub fn has(&self, code: DiagCode) -> bool {
        self.count_of(code) > 0
    }

    /// 某码精确出现次数（判据用 `==` 钉死「恰等于」，不用 `>=`）。
    pub fn count_of(&self, code: DiagCode) -> usize {
        let mut n = 0usize;
        let mut i = 0usize;
        while i < self.items.len() {
            if self.items[i].code == code {
                n += 1;
            }
            i += 1;
        }
        n
    }

    /// 某严重度条数。
    pub fn count_severity(&self, sev: Severity) -> usize {
        let mut n = 0usize;
        let mut i = 0usize;
        while i < self.items.len() {
            if self.items[i].severity == sev {
                n += 1;
            }
            i += 1;
        }
        n
    }

    /// P1 条数。
    pub fn p1_count(&self) -> usize {
        self.count_severity(Severity::P1)
    }
}

// ---------------------------------------------------------------------------
// 二、glTF 源模型（解码后的 f32 视图 + 声明元数据）
// ---------------------------------------------------------------------------

/// accessor 分量类型（glTF 规范三族 + 未登记）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ComponentType {
    /// 单精度浮点（glTF `5126`）。
    Float,
    /// 无符号字节（glTF `5121`）。
    U8,
    /// 无符号短（glTF `5123`）。
    U16,
    /// 未登记的分量类型（未来版本 / 格式混淆）。
    Unregistered,
}

impl ComponentType {
    /// 人话标签。
    pub const fn label(self) -> &'static str {
        match self {
            ComponentType::Float => "f32",
            ComponentType::U8 => "u8",
            ComponentType::U16 => "u16",
            ComponentType::Unregistered => "未登记分量类型",
        }
    }

    /// 该分量类型下的**满量程值**（整数 accessor 反归一化的分母来源）。
    ///
    /// `Float` 返回 `1.0`（浮点不反归一化）；整数类型返回 `2^(n-1) - 1`，
    /// 这正是 glTF 规范里 `normalized` 整数 accessor 的除数。
    pub const fn full_scale(self) -> f32 {
        match self {
            ComponentType::Float => 1.0,
            ComponentType::U8 => 127.0,
            ComponentType::U16 => 32767.0,
            ComponentType::Unregistered => 1.0,
        }
    }

    /// 是否整数型（决定「声明未归一化却给整数语义」这条域校验是否成立）。
    pub const fn is_integer(self) -> bool {
        matches!(self, ComponentType::U8 | ComponentType::U16)
    }
}

/// 解码后的 accessor 视图 + 声明元数据。
///
/// **`data` 已是解码后的 f32 序列**（整数 accessor 的反归一化由解码侧完成）。
/// 本条校验的是「声明元数据与视图自洽」：`len(data) == count * comps`。
#[derive(Clone, Debug, PartialEq)]
pub struct AccessorView {
    /// 声明的分量类型。
    pub component_type: ComponentType,
    /// 声明是否 `normalized`。
    pub normalized: bool,
    /// 声明的元素数。
    pub count: u32,
    /// 每元素分量数（1/2/3/4）。
    pub comps: u8,
    /// 解码后的展平值（`len == count * comps`）。
    pub data: Vec<f32>,
}

impl AccessorView {
    /// 构造（解码侧用）。
    pub fn new(
        component_type: ComponentType,
        normalized: bool,
        count: u32,
        comps: u8,
        data: Vec<f32>,
    ) -> AccessorView {
        AccessorView { component_type, normalized, count, comps, data }
    }

    /// 视图长度是否与声明自洽。
    pub fn shape_ok(&self) -> bool {
        self.data.len() == self.count as usize * self.comps as usize
    }

    /// 声明的整数未归一化却出现整数值域 —— 归一化被绕过（解码侧失职）。
    ///
    /// 判据：`is_integer() && !normalized` 且数据里出现**恰为整数值**的分量。
    /// 用「恰为整数」而不是「值较大」：未归一化的 `u16` 本来就可以合法地
    /// 存 1.0 / 0.0（那是米制缩放因子），只有当它承载的是 0..65535 的原始
    /// 码值时才是绕过。这里取一个**明确的哨兵**：出现 `>= 256.0` 的分量。
    /// 阈值理由：`u8` 满量程 127，`u16` 在归一化后不可能超过 1.0 —— 于是
    /// `>= 256` 只可能来自「未归一化的 u16 原始码值」。
    pub fn normalization_bypassed(&self) -> bool {
        if !self.component_type.is_integer() || self.normalized {
            return false;
        }
        let mut i = 0usize;
        while i < self.data.len() {
            let v = self.data[i];
            if v.is_finite() && v >= 256.0 {
                return true;
            }
            i += 1;
        }
        false
    }
}

/// 通道路径（glTF animation channel 的 `target.path`）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ChannelPath {
    /// `translation` → 位置轨。
    Translation,
    /// `rotation` → 四元数轨。
    Rotation,
    /// `scale` → 缩放轨。
    Scale,
    /// `weights` → 形态键权重轨（F2441 前向对接）。
    Weights,
    /// 四通道外（自定义 / 未来版本路径）。
    Other,
}

impl ChannelPath {
    /// 四通道全集（映射表的行序，单源）。
    pub const ALL: [ChannelPath; 4] =
        [ChannelPath::Translation, ChannelPath::Rotation, ChannelPath::Scale, ChannelPath::Weights];

    /// 线上编码（**显式映射**，不用 `as u8`——枚举判别值不是线上值）。
    pub const fn wire(self) -> u8 {
        match self {
            ChannelPath::Translation => 1,
            ChannelPath::Rotation => 2,
            ChannelPath::Scale => 3,
            ChannelPath::Weights => 4,
            ChannelPath::Other => 0,
        }
    }

    /// glTF 规范里的路径名（人话 + 规范原文）。
    pub const fn label(self) -> &'static str {
        match self {
            ChannelPath::Translation => "translation",
            ChannelPath::Rotation => "rotation",
            ChannelPath::Scale => "scale",
            ChannelPath::Weights => "weights",
            ChannelPath::Other => "四通道外自定义路径",
        }
    }

    /// 是否在支持范围内（四通道纪律）。
    pub const fn in_scope(self) -> bool {
        self.wire() >= 1 && self.wire() <= 4
    }

    /// 按线上编码反查（`Other` 不反查——它没有专属码位）。
    pub fn from_wire(w: u8) -> ChannelPath {
        let mut i = 0usize;
        while i < ChannelPath::ALL.len() {
            if ChannelPath::ALL[i].wire() == w {
                return ChannelPath::ALL[i];
            }
            i += 1;
        }
        ChannelPath::Other
    }
}

/// glTF 采样器插值模式。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GltfInterp {
    /// 线性（glTF 默认）。
    Linear,
    /// 阶跃。
    Step,
    /// 三次样条（每关键帧 3 分量：入切线/值/出切线）。
    CubicSpline,
}

impl GltfInterp {
    /// glTF 规范名。
    pub const fn label(self) -> &'static str {
        match self {
            GltfInterp::Linear => "LINEAR",
            GltfInterp::Step => "STEP",
            GltfInterp::CubicSpline => "CUBICSPLINE",
        }
    }

    /// 每关键帧占用多少个「值分量组」。
    ///
    /// `CUBICSPLINE` 是 3 组（入切线 / 值 / 出切线）——**这是本条最容易
    /// 写错的一处**：按 1 组读会把切线当值搬进轨道，动画整体错位且数值上看
    /// 「像是对的」（切线值域与位置值域同量级）。
    pub const fn value_groups(self) -> usize {
        match self {
            GltfInterp::CubicSpline => 3,
            GltfInterp::Linear | GltfInterp::Step => 1,
        }
    }

    /// 映射到 F2403 插值器（**单源复用，不另造**）。
    ///
    /// `CubicSpline` 映射到 `Linear` 是**降级声明**：本域 `SoaTrack` 没有切线
    /// 存储位，导入只取「值」那一组，切线丢弃并留 `CUBIC_TANGENT_DROPPED`
    /// 警告。诚实标注优于把曲线画成一条谁也看不懂的形状。
    pub const fn to_interp(self) -> Interp {
        match self {
            GltfInterp::Linear => Interp::Linear,
            GltfInterp::Step => Interp::Step,
            GltfInterp::CubicSpline => Interp::Linear,
        }
    }
}

/// 一个采样器（时间 accessor + 值 accessor + 插值模式）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SamplerRef {
    /// 输入（时刻）accessor 下标。
    pub input: u32,
    /// 输出（值）accessor 下标。
    pub output: u32,
    /// 插值模式。
    pub interp: GltfInterp,
}

/// 一条动画通道（目标节点 + 目标路径 + 采样器）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ChannelRef {
    /// 目标节点下标（必须落在 `GltfAnimDoc::node_count` 内）。
    pub target_node: u32,
    /// 目标路径。
    pub path: ChannelPath,
    /// 采样器下标。
    pub sampler: u32,
}

/// 一份 glTF animation 的解码视图。
#[derive(Clone, Debug, PartialEq)]
pub struct GltfAnimDoc {
    /// 节点数（**不携带节点名**——零指纹纪律，见头注第 5 条）。
    pub node_count: u32,
    /// accessor 视图表。
    pub accessors: Vec<AccessorView>,
    /// 采样器表。
    pub samplers: Vec<SamplerRef>,
    /// 通道表。
    pub channels: Vec<ChannelRef>,
    /// 资产自带标签（真实 glTF 里是文件名/路径）。**本条保证它不流入任何输出**。
    pub asset_label: String,
}

impl GltfAnimDoc {
    /// 构造（解码侧用）。
    pub fn new(
        node_count: u32,
        accessors: Vec<AccessorView>,
        samplers: Vec<SamplerRef>,
        channels: Vec<ChannelRef>,
        asset_label: &str,
    ) -> GltfAnimDoc {
        GltfAnimDoc {
            node_count,
            accessors,
            samplers,
            channels,
            asset_label: asset_label.to_string(),
        }
    }
}

/// 三要素拒绝：什么错 / 在哪 / 怎么办。
///
/// 锚点错误矩阵：「glTF 格式非法 → 三要素拒绝」。三要素缺一即视为**降级成
/// 一句「导入失败」**——那等于把定位成本全推给调用方。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ImportError {
    /// 要素一：错误码。
    pub code: DiagCode,
    /// 要素二：定位（通道下标 / accessor 下标 / 文档字段名）。
    pub locator: String,
    /// 要素三：处置建议（怎么办）。
    pub hint: String,
}

impl ImportError {
    /// 造三要素错误。
    pub fn new(code: DiagCode, locator: &str, hint: &str) -> ImportError {
        ImportError { code, locator: locator.to_string(), hint: hint.to_string() }
    }

    /// 三要素是否齐备（**非空**才算齐备）。
    pub fn three_elements_complete(&self) -> bool {
        !self.locator.is_empty() && !self.hint.is_empty()
    }

    /// 人话渲染。
    pub fn render(&self) -> String {
        format!("[{}] {} · 定位：{} · 处置：{}", self.code.0, self.code.label(), self.locator, self.hint)
    }
}

// ---------------------------------------------------------------------------
// 三、四通道映射表（规格公开 + 摘要对账 + 漂移拦截）
// ---------------------------------------------------------------------------

/// M 域轨道语义标签（**与载体 `TrackValueKind` 分开**——见下条决策）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TrackSemantic {
    /// 位置（`translation` 落点）。
    Position,
    /// 旋转（`rotation` 落点，slerp 标记）。
    Rotation,
    /// 缩放（`scale` 落点）。
    Scale,
    /// 形态键权重（`weights` 落点，F2441 前向对接）。
    MorphWeight,
}

impl TrackSemantic {
    /// 线上编码（M 域语义码段 `0x10..=0x13`）。
    pub const fn wire(self) -> u8 {
        match self {
            TrackSemantic::Position => 0x10,
            TrackSemantic::Rotation => 0x11,
            TrackSemantic::Scale => 0x12,
            TrackSemantic::MorphWeight => 0x13,
        }
    }

    /// 人话标签。
    pub const fn label(self) -> &'static str {
        match self {
            TrackSemantic::Position => "位置轨",
            TrackSemantic::Rotation => "四元数轨",
            TrackSemantic::Scale => "缩放轨",
            TrackSemantic::MorphWeight => "形态键权重轨",
        }
    }

    /// **每关键帧的分量数**（语义层）。
    pub const fn lanes(self) -> usize {
        match self {
            TrackSemantic::Position | TrackSemantic::Scale => 3,
            TrackSemantic::Rotation => 4,
            // morph 一条轨只管**一个**形态键的权重 —— 标量。
            TrackSemantic::MorphWeight => 1,
        }
    }

    /// 落到 F2407/F2202 的 SoA 载体类型。
    ///
    /// **为什么语义与载体要分成两层**：`TrackValueKind` 只有
    /// `Scalar/Position/Quat` 三变体，**没有 Scale、也没有 Morph**。
    /// - `Scale` 与 `Position` 在 SoA 层同形（都是 3 通道 float3），故共用
    ///   `Position` 载体；
    /// - `MorphWeight` 是标量，多个形态键由**多条标量轨并行**承载
    ///   （F2441 的「多 morph 权重并行轨道」语义正落在这里）。
    ///
    /// 代价是「载体看不出语义」，所以语义必须由 `ImportedTrack.semantic`
    /// 独立承载——**不能靠载体反推**，否则 scale 轨会被下游当位置轨用。
    pub const fn carrier(self) -> TrackValueKind {
        match self {
            TrackSemantic::Position | TrackSemantic::Scale => TrackValueKind::Position,
            TrackSemantic::Rotation => TrackValueKind::Quat,
            TrackSemantic::MorphWeight => TrackValueKind::Scalar,
        }
    }

    /// 是否需 slerp 标记（**仅旋转**，F2423 前向对接）。
    pub const fn needs_slerp(self) -> bool {
        matches!(self, TrackSemantic::Rotation)
    }

    /// 全部语义（映射表列序）。
    pub const ALL: [TrackSemantic; 4] = [
        TrackSemantic::Position,
        TrackSemantic::Rotation,
        TrackSemantic::Scale,
        TrackSemantic::MorphWeight,
    ];
}

/// 映射表的一行。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MapRow {
    /// glTF 路径。
    pub path: ChannelPath,
    /// 落点语义（四通道外为 `None`）。
    pub semantic: Option<TrackSemantic>,
    /// 每帧分量数。
    pub lanes: u8,
}

impl MapRow {
    /// 造一行。
    pub const fn new(path: ChannelPath, semantic: Option<TrackSemantic>) -> MapRow {
        MapRow { path, semantic, lanes: match semantic {
            Some(s) => s.lanes() as u8,
            None => 0,
        } }
    }
}

/// 四通道映射表。
///
/// **它为什么是一个带摘要的数据结构而不是一个 `match`**：映射表是本条对外
/// 承诺的规格（锚点原文「映射规格公开文档」）。规格会被人改（F2441 落地后
/// morph 行可能要加参数），改了却没人发现，症状是「动画导入后骨骼旋转错了」
/// ——那类缺陷在运行时极难定位。所以：注册时存摘要，`reconcile()` 重算，
/// 不一致即**置拦截**，拦截态下导入整体拒绝。
#[derive(Clone, Debug)]
pub struct MappingTable {
    rows: [MapRow; 4],
    declared: u32,
    drift_count: u32,
    intercepted: bool,
}

/// FNV-1a 单步。
const fn fnv_step(h: u32, byte: u32) -> u32 {
    (h ^ byte).wrapping_mul(0x0100_0193)
}

/// **标准四通道映射的规范摘要**（对账基准的**唯一**来源）。
///
/// **为什么基准必须由「标准行」算，而不是由「建表时的行」算**：
/// 若 `declared` 取自传入的行，那么「用错映行建表」会得到
/// 「错映摘要 == 错映声明」⇒ `drifted()` 恒 false ⇒ 漂移永不被发现。
/// 那正是本条要防的缺陷本身。基准必须是**与本表无关的规格常量**。
pub fn spec_checksum() -> u32 {
    checksum_of(&spec_rows())
}

/// 标准四通道行（规格常量，**四行顺序 = `ChannelPath::ALL` 顺序**）。
const fn spec_rows() -> [MapRow; 4] {
    [
        MapRow::new(ChannelPath::Translation, Some(TrackSemantic::Position)),
        MapRow::new(ChannelPath::Rotation, Some(TrackSemantic::Rotation)),
        MapRow::new(ChannelPath::Scale, Some(TrackSemantic::Scale)),
        MapRow::new(ChannelPath::Weights, Some(TrackSemantic::MorphWeight)),
    ]
}

/// 对给定四行算摘要（`checksum` 与规格基准共用，保证同一口径）。
fn checksum_of(rows: &[MapRow; 4]) -> u32 {
    let mut h: u32 = 0x811c_9dc5;
    let mut i = 0usize;
    while i < rows.len() {
        let r = rows[i];
        h = fnv_step(h, r.path.wire() as u32);
        h = fnv_step(h, match r.semantic {
            Some(s) => s.wire() as u32,
            None => 0,
        });
        h = fnv_step(h, r.lanes as u32);
        h = fnv_step(h, r.path.label().len() as u32);
        h = fnv_step(h, match r.semantic {
            Some(s) => s.label().len() as u32,
            None => 0,
        });
        i += 1;
    }
    h
}

impl MappingTable {
    /// 标准四通道映射（**规格单源**）。
    pub fn standard() -> MappingTable {
        MappingTable::with_rows(spec_rows())
    }

    /// 以给定四行建表。**声明摘要恒取规格基准**（见 `spec_checksum`）。
    ///
    /// 因此：传入与规格不同的行 ⇒ 建表即处于漂移态、`reconcile()` 立刻拦截。
    /// 这正是「错映必须被发现」的实现方式——不是靠调用方记得对账，
    /// 而是**没有正确基线的表从建立起就是红的**。
    pub fn with_rows(rows: [MapRow; 4]) -> MappingTable {
        MappingTable { rows, declared: spec_checksum(), drift_count: 0, intercepted: false }
    }

    /// 四行。
    pub fn rows(&self) -> &[MapRow; 4] {
        &self.rows
    }

    /// 重算摘要：对四行的 `path.wire / semantic.wire / lanes / label 长度` 做 FNV-1a。
    pub fn checksum(&self) -> u32 {
        checksum_of(&self.rows)
    }

    /// 声明摘要（规格基准）。
    pub fn declared_checksum(&self) -> u32 {
        self.declared
    }

    /// 是否漂移（重算摘要 ≠ 注册时声明）。
    pub fn drifted(&self) -> bool {
        self.checksum() != self.declared
    }

    /// 查映射（**拦截态下一律 `None`**——漂移规格不得被消费）。
    pub fn map(&self, path: ChannelPath) -> Option<TrackSemantic> {
        if self.intercepted {
            return None;
        }
        let mut i = 0usize;
        while i < self.rows.len() {
            if self.rows[i].path == path {
                return self.rows[i].semantic;
            }
            i += 1;
        }
        None
    }

    /// 累计漂移次数。
    pub fn drift_count(&self) -> u32 {
        self.drift_count
    }

    /// 是否处于拦截态。
    pub fn intercepted(&self) -> bool {
        self.intercepted
    }

    /// 显式解除拦截（**必须由人确认后调用**，不随对账自动清除）。
    pub fn clear_intercept(&mut self) {
        self.intercepted = false;
    }

    /// 对账：重算摘要。**有漂移即置拦截**并记 P1。
    pub fn reconcile(&mut self, bag: &mut DiagBag) -> bool {
        let bad = self.drifted();
        if bad {
            self.drift_count = self.drift_count.saturating_add(1);
            self.intercepted = true;
            bag.push_p1(DiagCode::MAPPING_DRIFT);
        }
        bad
    }
}

/// 映射表是否覆盖四通道且互不重复（**四行路径必须恰为四通道各一次**）。
pub fn mapping_covers_four_channels(t: &MappingTable) -> bool {
    let mut i = 0usize;
    while i < ChannelPath::ALL.len() {
        let p = ChannelPath::ALL[i];
        let mut hits = 0u32;
        let mut j = 0usize;
        while j < t.rows().len() {
            if t.rows()[j].path == p {
                hits += 1;
            }
            j += 1;
        }
        if hits != 1 {
            return false;
        }
        i += 1;
    }
    // 行数必须恰好 4（多一行少一行都是规格漂移）。
    t.rows().len() == 4
}

// ---------------------------------------------------------------------------
// 四、三重校验
// ---------------------------------------------------------------------------

/// 重一：通道引用完整性。
///
/// 四类引用逐一在域内。**返回拒绝码（`None` = 通过）**，不返回 bool——
/// 「为什么跳过」和「是否跳过」同等重要。
pub fn validate_channel_refs(
    doc: &GltfAnimDoc,
    ch: ChannelRef,
    bag: &mut DiagBag,
) -> Option<DiagCode> {
    let s = match doc.samplers.get(ch.sampler as usize) {
        Some(s) => s,
        None => {
            bag.push_major(DiagCode::SAMPLER_OOR);
            return Some(DiagCode::SAMPLER_OOR);
        }
    };
    if doc.accessors.get(s.input as usize).is_none() {
        bag.push_major(DiagCode::INPUT_ACCESSOR_OOR);
        return Some(DiagCode::INPUT_ACCESSOR_OOR);
    }
    if doc.accessors.get(s.output as usize).is_none() {
        bag.push_major(DiagCode::OUTPUT_ACCESSOR_OOR);
        return Some(DiagCode::OUTPUT_ACCESSOR_OOR);
    }
    if ch.target_node >= doc.node_count {
        bag.push_major(DiagCode::TARGET_NODE_MISSING);
        return Some(DiagCode::TARGET_NODE_MISSING);
    }
    None
}

/// 重二 + 重三的采样侧结果。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SampleVerdict {
    /// 通过。
    Ok,
    /// 拒绝（附码）。
    Reject(DiagCode),
}

/// 输出 accessor 的声明摘要（把「取哪几个字段」这件事收在一处）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GltfSamplerOut {
    /// 声明的每元素分量数。
    pub comps: u8,
    /// 插值模式。
    pub interp: GltfInterp,
    /// 声明的元素（关键帧）数。
    pub count: u32,
}

impl GltfSamplerOut {
    /// **期望的每关键帧值分量数**（含 CUBICSPLINE 的 3 组展开）。
    pub const fn expected_value_comps(&self) -> usize {
        self.comps as usize * self.interp.value_groups()
    }

    /// **「值」组在帧内的起始偏移**（分量数）。
    ///
    /// glTF 每关键帧布局：
    /// - `LINEAR` / `STEP`：`[值]` ⇒ 偏移 **0**；
    /// - `CUBICSPLINE`：`[入切线 | 值 | 出切线]` ⇒ 偏移 **1 组**（即 `comps`）。
    ///
    /// **必须按插值模式分别给出，绝不能用一个算式套两种布局**：
    /// 写成 `(groups - 1) * comps` 会得到 `2 * comps`——那是**出切线**组，
    /// 于是 CUBICSPLINE 通道导入的是出切线而非值。症状隐蔽：出切线与位置
    /// 同量级、每帧也都有值，动画「能动」但形状全错。
    ///
    /// 另一侧的同类错误是无条件加 `comps`：LINEAR 布局没有切线组，
    /// 无条件加会让每帧从**下一帧首分量**读值，末帧越界读 0，
    /// 整条轨道时间轴平移一帧。两个方向的错误都只能靠「按布局取值」避免。
    pub const fn value_group_offset(&self) -> usize {
        match self.interp {
            GltfInterp::CubicSpline => self.comps as usize,
            GltfInterp::Linear | GltfInterp::Step => 0,
        }
    }

    /// 第 `key` 帧的「值」组在展平数组中的起始下标。
    pub const fn value_base(&self, key: usize) -> usize {
        key * self.expected_value_comps() + self.value_group_offset()
    }
}

/// 重二：采样数据合法性（空 / 单调 / 值数自洽 / 退化四元数 / 值域）。
pub fn validate_samples(
    times: &[f32],
    out: &GltfSamplerOut,
    values: &[f32],
    semantic: TrackSemantic,
    bag: &mut DiagBag,
) -> SampleVerdict {
    if times.is_empty() || values.is_empty() {
        bag.push_major(DiagCode::SAMPLER_EMPTY);
        return SampleVerdict::Reject(DiagCode::SAMPLER_EMPTY);
    }
    // 时刻单调非递减（F2407 二分前提）。用 `<` 而非 `<=`：等宽单帧时间
    // 合法（F2408 已立此纪律），倒序才是破二分。
    let mut i = 1usize;
    while i < times.len() {
        let prev = times[i - 1];
        let cur = times[i];
        if cur < prev {
            bag.push_major(DiagCode::TIMES_NON_MONOTONIC);
            return SampleVerdict::Reject(DiagCode::TIMES_NON_MONOTONIC);
        }
        i += 1;
    }
    // 值数自洽：`len(values) == times.len() * 期望每帧分量`。
    let expect = times.len() * out.expected_value_comps();
    if values.len() != expect {
        bag.push_major(DiagCode::VALUE_COUNT_MISMATCH);
        return SampleVerdict::Reject(DiagCode::VALUE_COUNT_MISMATCH);
    }
    // 声明的 count 与实际时刻数不一致也是不自洽（解码侧与通道侧打架）。
    //
    // **CUBICSPLINE 的 count 是关键帧数的 3 倍**（glTF 规范：输出 accessor
    // 对三次样条每个关键帧存 3 组）。所以这里比的是
    // `count == times.len() * value_groups()`，不是 `== times.len()`。
    //
    // 早先按 `== times.len()` 比，于是每个 CUBICSPLINE 通道都被判
    // `VALUE_COUNT_MISMATCH` 而跳过——「CUBICSPLINE 全都不导入」这个症状
    // 会被误读成「切线组没处理好」，而真实原因是 count 口径差 3 倍。
    let expect_count = times.len() * out.interp.value_groups();
    if out.count as usize != expect_count {
        bag.push_major(DiagCode::VALUE_COUNT_MISMATCH);
        return SampleVerdict::Reject(DiagCode::VALUE_COUNT_MISMATCH);
    }
    // 语义相关的值域：旋转全零 = 退化。
    //
    // **基址必须用 `value_base()`**（对 CUBICSPLINE 跳过入切线组）：若从帧
    // 基址直读，读到的是**入切线**。入切线全零是完全正常的（零速率起步），
    // 拿它判退化会让每一个 CUBICSPLINE 旋转通道都被误报。
    if semantic.needs_slerp() {
        let mut degenerate = 0usize;
        let mut k = 0usize;
        while k < times.len() {
            let base = out.value_base(k);
            let (mut sum, mut c) = (0.0f32, 0usize);
            while c < 4 {
                if let Some(v) = values.get(base + c) {
                    let vv = if v.is_finite() { *v } else { 0.0 };
                    sum += vv * vv;
                }
                c += 1;
            }
            if sum <= 0.0 {
                degenerate += 1;
            }
            k += 1;
        }
        if degenerate > 0 {
            bag.push_major(DiagCode::QUAT_DEGENERATE);
        }
    }
    SampleVerdict::Ok
}

/// 重三：曲线异常统计（值非有限 / 时刻非有限 / 超密 / 负时刻）。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct CurveAnomaly {
    /// 非有限值分量数（将被钳制）。
    pub non_finite_values: u32,
    /// 非有限时刻数（该帧将被丢）。
    pub non_finite_times: u32,
    /// 量化后零间隔的对数（超密）。
    pub overdense_pairs: u32,
    /// 负时刻数（将被钳到 0）。
    pub negative_times: u32,
}

/// 重三的判定阈值：秒→毫秒量化后的**零间隔**即超密。
///
/// 为什么阈值是「零」而不是「小于某值」：本域时间轴分辨率是 1ms，两帧差
/// 不到 1ms 时取整后必然相等。判「零间隔」正好等于判「分辨率以下」，换成
/// 任何正阈值都会在 1ms 这个自然分辨率上产生「差 1ms 算不算超密」的争论。
pub const OVERDENSE_MIN_DT_MS: u32 = 1;

/// 重三：曲线异常检测（**只检测不改数据**——改动在 `import_channel`）。
pub fn detect_curve_anomaly(
    times: &[f32],
    out: &GltfSamplerOut,
    values: &[f32],
) -> CurveAnomaly {
    let mut a = CurveAnomaly::default();
    let mut k = 0usize;
    let mut prev_ms: Option<u32> = None;
    while k < times.len() {
        let t = match times.get(k) {
            Some(v) => *v,
            None => break,
        };
        if !t.is_finite() {
            a.non_finite_times = a.non_finite_times.saturating_add(1);
            k += 1;
            continue;
        }
        if t < 0.0 {
            a.negative_times = a.negative_times.saturating_add(1);
        }
        // 秒 → 毫秒（四舍五入；负值先归零再取整，避免 `as u32` 溢出语义依赖）。
        let secs = if t < 0.0 { 0.0 } else { t };
        let ms = secs_to_ms(secs);
        if let Some(p) = prev_ms {
            if ms < p.saturating_add(OVERDENSE_MIN_DT_MS) {
                a.overdense_pairs = a.overdense_pairs.saturating_add(1);
            }
        }
        prev_ms = Some(ms);
        // 值侧：只扫「值」那一组，切线组不参与（本域不存切线）。
        let base = out.value_base(k);
        let mut c = 0usize;
        while c < out.comps as usize {
            if let Some(v) = values.get(base + c) {
                if !v.is_finite() {
                    a.non_finite_values = a.non_finite_values.saturating_add(1);
                }
            }
            c += 1;
        }
        k += 1;
    }
    a
}

// ---------------------------------------------------------------------------
// 五、数值工具（自持，不依赖 std 的 f32 方法）
// ---------------------------------------------------------------------------

/// 秒 → 毫秒（四舍五入，**不用 `f32::round`** 以保跨档确定性）。
pub const fn secs_to_ms(secs: f32) -> u32 {
    let x = secs * 1000.0 + 0.5;
    if x <= 0.0 {
        0u32
    } else if x >= 4_294_967_295.0 {
        u32::MAX
    } else {
        x as u32
    }
}

/// 牛顿迭代开方（**固定 6 轮，不按收敛提前退出**——分支即不确定性）。
///
/// 真 `no_std`（kernel-image）下 `f32::sqrt` 不存在，而引入 `gfx::meshquant`
/// 会让本条对渲染域产生依赖。固定轮数的牛顿法在 `f32` 上 3 轮即到满精度，
/// 6 轮是余量；**确定性优先于最后一 ulp 的速度**（F2215 家族纪律）。
pub fn fsqrt(x: f32) -> f32 {
    if !(x > 0.0) {
        return 0.0;
    }
    let mut g = x * 0.5 + 0.5;
    let mut i = 0;
    while i < 6 {
        g = 0.5 * (g + x / g);
        i += 1;
    }
    g
}

/// 采样侧 NaN/Inf 钳制目标：位置/缩放/morph → 0，旋转 → 单位四元数。
///
/// **钳制目标按语义取，不是统一 0**：把 scale 的 NaN 钳成 0 会让物体瞬间
/// 塌成一个点（比 NaN 更坏——NaN 至少可见）；单位四元数是旋转的**恒等**值，
/// 钳成它等于「这一帧没有旋转」，是最诚实的降级。
pub fn clamp_target(semantic: TrackSemantic, lane: usize) -> f32 {
    if semantic == TrackSemantic::Rotation {
        if lane == 3 {
            1.0
        } else {
            0.0
        }
    } else {
        0.0
    }
}

/// 四元数归一化（就地改写 `out`）。返回是否发生改写。
pub fn normalize_quat(out: &mut [f32]) -> bool {
    if out.len() < 4 {
        return false;
    }
    let mut sum = 0.0f32;
    let mut i = 0usize;
    while i < 4 {
        let v = if out[i].is_finite() { out[i] } else { 0.0 };
        out[i] = v;
        sum += v * v;
        i += 1;
    }
    let n = fsqrt(sum);
    if !(n > 0.0) {
        out[0] = 0.0;
        out[1] = 0.0;
        out[2] = 0.0;
        out[3] = 1.0;
        return true;
    }
    let inv = 1.0 / n;
    let mut changed = false;
    let mut k = 0usize;
    while k < 4 {
        let want = out[k] * inv;
        if want != out[k] {
            changed = true;
        }
        out[k] = want;
        k += 1;
    }
    changed
}

// ---------------------------------------------------------------------------
// 六、导入产物与结构化报告
// ---------------------------------------------------------------------------

/// 导入出的一条轨道。
#[derive(Clone, Debug, PartialEq)]
pub struct ImportedTrack {
    /// 语义标签（**不靠载体反推**，见 `TrackSemantic::carrier`）。
    pub semantic: TrackSemantic,
    /// 目标节点下标。
    pub target_node: u32,
    /// 形态键下标（仅 `MorphWeight` 有意义；其余为 `u16::MAX`）。
    pub morph_slot: u16,
    /// 轨道名（**由语义 + 节点下标构造，绝不含资产标签**）。
    pub name: String,
    /// 落地轨道（SoA 布局单源：F2202 家族）。
    pub track: SoaTrack,
    /// 插值标记（F2403 枚举单源）。
    pub interp: Interp,
    /// 是否带 slerp 标记（仅旋转为 true，F2423 前向对接）。
    pub slerp: bool,
    /// 导入期归一化改写过的四元数个数。
    pub normalized: u32,
    /// 导入期钳制过的分量数。
    pub clamped: u32,
}

/// 导入模式（**enum 而非 bool**：阈值必须与保真同时存在，见头注第 3 条）。
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Fidelity {
    /// 保真默认：逐关键帧原样落地。
    Faithful,
    /// 误差阈值抽帧（F2425 联动位）。`epsilon` 是允许的最大线性重建误差。
    Simplified {
        /// 误差上限（调用方按资产特性给定；本条不猜）。
        epsilon: f32,
    },
}

impl Fidelity {
    /// 是否为精简模式。
    pub const fn is_simplified(self) -> bool {
        matches!(self, Fidelity::Simplified { .. })
    }

    /// 精简阈值（非精简模式为 `0.0`）。
    pub const fn epsilon(self) -> f32 {
        match self {
            Fidelity::Faithful => 0.0,
            Fidelity::Simplified { epsilon } => epsilon,
        }
    }

    /// 精简模式但阈值非正（**非法组合**：无阈值的「精简」等于随机抽帧）。
    pub fn simplified_without_threshold(self) -> bool {
        match self {
            Fidelity::Simplified { epsilon } => !(epsilon > 0.0) || !epsilon.is_finite(),
            Fidelity::Faithful => false,
        }
    }
}

/// 一条通道的映射记录（**报告三要素之二**）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MappingRecord {
    /// 通道下标。
    pub channel: u32,
    /// glTF 路径。
    pub path: ChannelPath,
    /// 落点语义（四通道外 / 被跳过时为 `None`）。
    pub semantic: Option<TrackSemantic>,
    /// 产出的轨道下标区间（`start..start + count`；被跳过时 `count == 0`）。
    pub track_start: u32,
    /// 产出轨道数。
    pub track_count: u32,
    /// 输入关键帧数（**精简前**）。
    pub keys_in: u32,
    /// 落地关键帧数（**精简后**）。
    pub keys_out: u32,
    /// 处置码（`None` = 正常导入）。
    pub verdict: Option<DiagCode>,
}

impl MappingRecord {
    /// 人话渲染（**零指纹**：不含资产标签）。
    pub fn render(&self) -> String {
        let target = match self.semantic {
            Some(s) => s.label(),
            None => "未落地",
        };
        let verdict = match self.verdict {
            Some(c) => format!("处置：{}", c.label()),
            None => String::from("处置：正常导入"),
        };
        format!(
            "ch{} {} → {}（轨道 {} 条，帧 {}→{}）{}",
            self.channel,
            self.path.label(),
            target,
            self.track_count,
            self.keys_in,
            self.keys_out,
            verdict
        )
    }
}

/// 一条导入警告（**报告三要素之三**）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ImportWarning {
    /// 诊断码。
    pub code: DiagCode,
    /// 通道下标。
    pub channel: u32,
    /// 人话正文（**必须非空**——无障碍要求：报告要能被替述读出）。
    pub text: String,
}

impl ImportWarning {
    /// 构造。
    pub fn new(code: DiagCode, channel: u32, text: &str) -> ImportWarning {
        ImportWarning { code, channel, text: text.to_string() }
    }

    /// 正文是否非空（判据逐条钉）。
    pub fn text_present(&self) -> bool {
        !self.text.is_empty()
    }
}

/// 导入报告（**结构化三要素**：轨道数 / 通道映射表 / 警告清单）。
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ImportReport {
    /// 通道总数（文档里的）。
    pub channels_total: u32,
    /// 成功导入的通道数。
    pub channels_imported: u32,
    /// 跳过的通道数。
    pub channels_skipped: u32,
    /// 产出轨道总数。
    pub tracks: u32,
    /// 输入关键帧总数（精简前）。
    pub keys_in: u64,
    /// 落地关键帧总数（精简后）。
    pub keys_out: u64,
    /// 钳制过的分量总数。
    pub clamped: u32,
    /// 通道映射表。
    pub mapping: Vec<MappingRecord>,
    /// 警告清单。
    pub warnings: Vec<ImportWarning>,
    /// 支持范围声明（四通道外通道 / CUBICSPLINE 切线丢弃）。
    pub declarations: Vec<String>,
}

impl ImportReport {
    /// 精简掉的关键帧数（`keys_in - keys_out`；保真模式恒为 0）。
    pub fn keys_removed(&self) -> u64 {
        self.keys_in.saturating_sub(self.keys_out)
    }

    /// 精简率（整数口径 `(分子, 分母)`，判据精确对账，不用浮点除）。
    pub fn reduction_ratio(&self) -> (u64, u64) {
        (self.keys_removed(), self.keys_in)
    }

    /// 跳过通道的映射记录数（**独立重算**，不读 `channels_skipped`）。
    pub fn skipped_records(&self) -> usize {
        let mut n = 0usize;
        let mut i = 0usize;
        while i < self.mapping.len() {
            if self.mapping[i].track_count == 0 {
                n += 1;
            }
            i += 1;
        }
        n
    }

    /// 映射记录数是否恰等于通道总数（**恰等于**，不用 `>=`）。
    pub fn mapping_covers_channels(&self) -> bool {
        self.mapping.len() as u32 == self.channels_total
    }

    /// 全部警告是否都有人话正文。
    pub fn all_warnings_readable(&self) -> bool {
        let mut i = 0usize;
        while i < self.warnings.len() {
            if !self.warnings[i].text_present() {
                return false;
            }
            i += 1;
        }
        true
    }

    /// **零指纹断言**：报告任何位置都不得含资产标签。
    ///
    /// 判据侧用「非空的资产标签」调用本方法——若实现某天把 `asset_label`
    /// 拼进轨道名或警告正文，这里立刻转红。
    pub fn fingerprint_free(&self, asset_label: &str) -> bool {
        if asset_label.is_empty() {
            return false;
        }
        let mut i = 0usize;
        while i < self.mapping.len() {
            if self.mapping[i].render().contains(asset_label) {
                return false;
            }
            i += 1;
        }
        let mut j = 0usize;
        while j < self.warnings.len() {
            if self.warnings[j].text.contains(asset_label) {
                return false;
            }
            j += 1;
        }
        let mut k = 0usize;
        while k < self.declarations.len() {
            if self.declarations[k].contains(asset_label) {
                return false;
            }
            k += 1;
        }
        true
    }

    /// 人话渲染（三要素齐出）。
    pub fn render(&self) -> String {
        let mut s = String::new();
        let _ = s.push_str(&format!(
            "动画导入报告：通道 {}/{} 导入、{} 跳过；轨道 {} 条；关键帧 {}→{}（精简 {}）\n",
            self.channels_imported,
            self.channels_total,
            self.channels_skipped,
            self.tracks,
            self.keys_in,
            self.keys_out,
            self.keys_removed()
        ));
        let _ = s.push_str("映射表：\n");
        let mut i = 0usize;
        while i < self.mapping.len() {
            let _ = s.push_str(&format!("  {}\n", self.mapping[i].render()));
            i += 1;
        }
        if !self.warnings.is_empty() {
            let _ = s.push_str(&format!("警告清单（{} 条）：\n", self.warnings.len()));
            let mut j = 0usize;
            while j < self.warnings.len() {
                let w = &self.warnings[j];
                let _ = s.push_str(&format!("  ch{} [{}] {}\n", w.channel, w.code.0, w.text));
                j += 1;
            }
        }
        if !self.declarations.is_empty() {
            let _ = s.push_str("支持范围声明：\n");
            let mut k = 0usize;
            while k < self.declarations.len() {
                let _ = s.push_str(&format!("  {}\n", self.declarations[k]));
                k += 1;
            }
        }
        s
    }
}

/// 导入产物（轨道 + 报告）。
#[derive(Clone, Debug, PartialEq)]
pub struct ImportResult {
    /// 产出轨道（顺序 = 报告映射表的 `track_start` 顺序）。
    pub tracks: Vec<ImportedTrack>,
    /// 结构化报告。
    pub report: ImportReport,
}

impl ImportResult {
    /// 轨道数。
    pub fn track_count(&self) -> usize {
        self.tracks.len()
    }

    /// 报告里的轨道数是否与实际一致（**恰等于**）。
    pub fn report_matches_tracks(&self) -> bool {
        self.report.tracks as usize == self.tracks.len()
    }

    /// 某节点上某语义的轨道（供骨骼映射 F2422 消费）。
    pub fn find(&self, node: u32, semantic: TrackSemantic) -> Option<&ImportedTrack> {
        let mut i = 0usize;
        while i < self.tracks.len() {
            let t = &self.tracks[i];
            if t.target_node == node && t.semantic == semantic && t.morph_slot == u16::MAX {
                return Some(t);
            }
            i += 1;
        }
        None
    }

    /// 某节点上的全部形态键轨道（**条数 = 形态键数**，F2441 前向对接）。
    pub fn morph_tracks(&self, node: u32) -> usize {
        let mut n = 0usize;
        let mut i = 0usize;
        while i < self.tracks.len() {
            let t = &self.tracks[i];
            if t.target_node == node && t.semantic == TrackSemantic::MorphWeight {
                n += 1;
            }
            i += 1;
        }
        n
    }
}

// ---------------------------------------------------------------------------
// 七、抽帧（F2425 联动位 · 端点钉死 · 误差自证）
// ---------------------------------------------------------------------------

/// 抽帧结果。
#[derive(Clone, Debug, PartialEq)]
pub struct ThinOutcome {
    /// 输入关键帧数。
    pub keys_in: u32,
    /// 保留关键帧数。
    pub keys_out: u32,
    /// 被删关键帧的**实测**最大线性重建误差（**不是 epsilon 本身**）。
    pub max_error: f32,
    /// 保留帧下标（**升序、含首末**）。
    ///
    /// **为什么必须返回下标而不是只回计数**：抽帧有两处消费方——缓冲重建
    /// 与误差复核。若让 `import_channel` 用「同一套贪心」重算一遍保留集，
    /// 那就有两份可能漂移的贪心实现；实测里两份一旦漂移，表现是
    /// 「帧数对得上但抽错了帧」——最难查的一类。故此处返回下标单源。
    pub kept: Vec<usize>,
}

/// 帧的线性重建误差（抽帧与复核**共用同一函数**，误差公式单源）。
///
/// `idx` 帧在 `li`/`ri` 两保留帧之间的线性重建误差（各分量取最大值）。
fn recon_error(
    times: &[u32],
    values: &[f32],
    stride: usize,
    idx: usize,
    li: usize,
    ri: usize,
) -> f32 {
    let tl = match times.get(li) {
        Some(v) => *v,
        None => return 0.0,
    };
    let tr = match times.get(ri) {
        Some(v) => *v,
        None => return 0.0,
    };
    let ti = match times.get(idx) {
        Some(v) => *v,
        None => return 0.0,
    };
    let span = tr.saturating_sub(tl);
    if span == 0 {
        return 0.0;
    }
    let t = (ti - tl) as f32 / span as f32;
    let mut worst = 0.0f32;
    let mut c = 0usize;
    while c < stride {
        let cur = lane(values, stride, idx, c);
        let l = lane(values, stride, li, c);
        let r = lane(values, stride, ri, c);
        let recon = l + (r - l) * t;
        let d = (cur - recon).abs();
        if d > worst {
            worst = d;
        }
        c += 1;
    }
    worst
}

/// 误差阈值抽帧（贪心 + 端点钉死 + 返回保留下标）。
///
/// **端点钉死**：首末关键帧**必留**。端点若也按误差删掉，轨道的时间跨度就
/// 短了——动画会在循环时跳一下，而误差度量完全看不出这件事（F2408 已在
/// 曲线抽稀上栽过同一类坑，纪律沿用）。
///
/// **误差是实测的**：返回的 `max_error` 是对**每个被删帧**用其**最终保留的
/// 左右邻居**重算的线性插值误差的最大值。贪心逐点删时用的近邻会随后续删除
/// 而失效，所以「删除当时误差 ≤ ε」**推不出**「最终误差 ≤ ε」——必须重算。
/// 这就是本函数要多跑一遍的原因。
pub fn thin_keys(times: &[u32], values: &[f32], stride: usize, epsilon: f32) -> ThinOutcome {
    let n = times.len();
    if n <= 2 || !(epsilon > 0.0) || stride == 0 {
        let mut kept: Vec<usize> = Vec::new();
        let mut i = 0usize;
        while i < n {
            kept.push(i);
            i += 1;
        }
        return ThinOutcome { keys_in: n as u32, keys_out: n as u32, max_error: 0.0, kept };
    }
    let mut keep: Vec<usize> = Vec::new();
    let mut i = 0usize;
    while i < n {
        keep.push(i);
        i += 1;
    }
    // 贪心：从左往右，考察每个非端点帧，若「用当前左右保留邻居重建」的
    // 误差 ≤ epsilon 就删。端点永不进入候选。
    let mut j = 1usize;
    while j + 1 < keep.len() {
        let li = keep[j - 1];
        let ri = keep[j + 1];
        if recon_error(times, values, stride, j, li, ri) <= epsilon {
            keep.remove(j);
        } else {
            j += 1;
        }
    }
    // **实测最终误差**：对每个被删帧，用最终保留的左右邻居重算。
    let mut max_err = 0.0f32;
    let mut r = 1usize;
    while r + 1 < keep.len() {
        let li = keep[r - 1];
        let ri = keep[r + 1];
        let mut d = li + 1;
        while d < ri {
            let e = recon_error(times, values, stride, d, li, ri);
            if e > max_err {
                max_err = e;
            }
            d += 1;
        }
        r += 1;
    }
    ThinOutcome { keys_in: n as u32, keys_out: keep.len() as u32, max_error: max_err, kept: keep }
}

/// 取展平值数组中「第 `key` 帧第 `lane` 分量」；越界返回 0（**不 panic**）。
fn lane(values: &[f32], stride: usize, key: usize, l: usize) -> f32 {
    let idx = key.checked_mul(stride).and_then(|b| b.checked_add(l));
    match idx.and_then(|i| values.get(i)) {
        Some(v) => *v,
        None => 0.0,
    }
}

/// 单关键帧最大分量数（SoA 载体口径，F2407 `MAX_CHANNELS` 同量级）。
pub const MAX_LANES_PER_KEY: usize = 4;

/// 单帧行缓冲宽度上限（**必须 ≥ 声明的输出分量数**）。
///
/// 与 `MAX_LANES_PER_KEY` 分开的原因：前者是**落进 SoA 轨道**的载体宽度
/// （四元数 4 通道封顶），后者是**解码侧一行的原始宽度**——形态键通道的
/// `comps` 等于形态键个数，可以远大于 4。两者混用会让第 5 个形态键起的
/// 分量被静默丢弃（`shape_ok()` 仍为真，因为轨道侧只放 1 通道）。
pub const MAX_ROW_COMPONENTS: usize = 16;

// ---------------------------------------------------------------------------
// 八、导入主流程
// ---------------------------------------------------------------------------

/// 导入一份 glTF animation。
///
/// **拒绝优先于降级**：文档级非法（`node_count == 0` / 通道引用全灭 /
/// 映射表拦截态）直接返回 `Err(ImportError)`；单个通道的非法才走
/// 「跳过 + 警告」。这个分界是刻意的——通道级问题不阻断其它通道，
/// 文档级问题继续导只会产出**半截动画**，那比不导更危险。
pub fn import_gltf_anim(
    doc: &GltfAnimDoc,
    fidelity: Fidelity,
    table: &MappingTable,
    bag: &mut DiagBag,
) -> Result<ImportResult, ImportError> {
    // —— 前置一：映射表拦截态 ——
    if table.intercepted() {
        bag.push_p1(DiagCode::MAPPING_INTERCEPTED);
        bag.push_p1(DiagCode::IMPORT_ABORTED);
        return Err(ImportError::new(
            DiagCode::MAPPING_INTERCEPTED,
            "MappingTable",
            "映射表已因漂移置拦截，先修映射规格并 clear_intercept 后重导",
        ));
    }
    // —— 前置二：文档级合法性（三要素拒绝）——
    if doc.node_count == 0 {
        bag.push_p1(DiagCode::DOC_MALFORMED);
        return Err(ImportError::new(
            DiagCode::DOC_MALFORMED,
            "GltfAnimDoc.node_count",
            "节点数为 0，通道无处落地；请检查场景根是否被导出",
        ));
    }
    if doc.channels.is_empty() {
        bag.push_p1(DiagCode::DOC_MALFORMED);
        return Err(ImportError::new(
            DiagCode::DOC_MALFORMED,
            "GltfAnimDoc.channels",
            "通道表为空：这不是动画文档，或动画段未被解析（查 I01 F1602 解码）",
        ));
    }
    // —— 前置三：精简模式必须带正阈值 ——
    if fidelity.simplified_without_threshold() {
        bag.push_major(DiagCode::DOC_MALFORMED);
        return Err(ImportError::new(
            DiagCode::DOC_MALFORMED,
            "Fidelity::Simplified.epsilon",
            "精简阈值必须为正有限值；无阈值的精简不可复现，改用 Faithful 或给定 epsilon",
        ));
    }

    let mut report = ImportReport {
        channels_total: doc.channels.len() as u32,
        ..ImportReport::default()
    };
    let mut tracks: Vec<ImportedTrack> = Vec::new();
    let mut declared_cubic = false;
    let mut declared_scope = false;

    let mut ci = 0usize;
    while ci < doc.channels.len() {
        let ch = doc.channels[ci];
        let ch_idx = ci as u32;

        // 重一：通道引用完整性。
        if let Some(code) = validate_channel_refs(doc, ch, bag) {
            report.channels_skipped = report.channels_skipped.saturating_add(1);
            report.mapping.push(MappingRecord {
                channel: ch_idx,
                path: ch.path,
                semantic: None,
                track_start: tracks.len() as u32,
                track_count: 0,
                keys_in: 0,
                keys_out: 0,
                verdict: Some(code),
            });
            report.warnings.push(ImportWarning::new(code, ch_idx, code.label()));
            ci += 1;
            continue;
        }

        // 四通道外通道：跳过 + 声明（支持范围诚实）。
        let semantic = match table.map(ch.path) {
            Some(s) => s,
            None => {
                bag.push_major(DiagCode::PATH_OUT_OF_SCOPE);
                report.channels_skipped = report.channels_skipped.saturating_add(1);
                report.mapping.push(MappingRecord {
                    channel: ch_idx,
                    path: ch.path,
                    semantic: None,
                    track_start: tracks.len() as u32,
                    track_count: 0,
                    keys_in: 0,
                    keys_out: 0,
                    verdict: Some(DiagCode::PATH_OUT_OF_SCOPE),
                });
                report.warnings.push(ImportWarning::new(
                    DiagCode::PATH_OUT_OF_SCOPE,
                    ch_idx,
                    "四通道外自定义通道未导入；本域只支持 translation/rotation/scale/weights",
                ));
                if !declared_scope {
                    declared_scope = true;
                    report.declarations.push(String::from(
                        "存在四通道外通道，已跳过且未产出轨道（支持范围：四通道）",
                    ));
                }
                ci += 1;
                continue;
            }
        };

        // 取采样器与两侧 accessor。
        let sampler = match doc.samplers.get(ch.sampler as usize) {
            Some(s) => *s,
            None => {
                ci += 1;
                continue;
            }
        };
        let in_acc = match doc.accessors.get(sampler.input as usize) {
            Some(a) => a,
            None => {
                ci += 1;
                continue;
            }
        };
        let out_acc = match doc.accessors.get(sampler.output as usize) {
            Some(a) => a,
            None => {
                ci += 1;
                continue;
            }
        };

        // 值域声明自洽（重二的一部分：声明与视图打架 = 解码侧失职）。
        if !out_acc.shape_ok() || out_acc.normalization_bypassed() {
            bag.push_major(DiagCode::VALUE_OUT_OF_DOMAIN);
            report.channels_skipped = report.channels_skipped.saturating_add(1);
            report.mapping.push(MappingRecord {
                channel: ch_idx,
                path: ch.path,
                semantic: None,
                track_start: tracks.len() as u32,
                track_count: 0,
                keys_in: 0,
                keys_out: 0,
                verdict: Some(DiagCode::VALUE_OUT_OF_DOMAIN),
            });
            report.warnings.push(ImportWarning::new(
                DiagCode::VALUE_OUT_OF_DOMAIN,
                ch_idx,
                "输出 accessor 声明与视图不自洽，或归一化被绕过；通道已跳过",
            ));
            ci += 1;
            continue;
        }

        let out = GltfSamplerOut { comps: out_acc.comps, interp: sampler.interp, count: out_acc.count };

        // 基础分量必须与语义相符：morph 的输出 comps = 形态键数（≥1），
        // 位置/缩放 = 3，旋转 = 4。**这里不校验会造出「1 分量的位置轨」**。
        let base_ok = match semantic {
            TrackSemantic::Position | TrackSemantic::Scale => out.comps == 3,
            TrackSemantic::Rotation => out.comps == 4,
            // 形态键数上界 = 行缓冲宽度：**超出的分量无处安放**，若放行则
            // 第 `MAX_ROW_COMPONENTS` 个之后的形态键被静默丢弃，而轨道的
            // `shape_ok()` 仍为真（每轨只放 1 通道）。显式拒绝才是诚实的。
            TrackSemantic::MorphWeight => {
                out.comps >= 1 && out.comps as usize <= MAX_ROW_COMPONENTS
            }
        };
        if !base_ok {
            bag.push_major(DiagCode::VALUE_OUT_OF_DOMAIN);
            report.channels_skipped = report.channels_skipped.saturating_add(1);
            report.mapping.push(MappingRecord {
                channel: ch_idx,
                path: ch.path,
                semantic: None,
                track_start: tracks.len() as u32,
                track_count: 0,
                keys_in: 0,
                keys_out: 0,
                verdict: Some(DiagCode::VALUE_OUT_OF_DOMAIN),
            });
            report.warnings.push(ImportWarning::new(
                DiagCode::VALUE_OUT_OF_DOMAIN,
                ch_idx,
                "通道分量数与语义不符（位置/缩放需 3、旋转需 4、形态键需 ≥1）；通道已跳过",
            ));
            ci += 1;
            continue;
        }

        // CUBICSPLINE 切线丢弃声明（**只声明一次**，不逐帧刷屏）。
        if sampler.interp == GltfInterp::CubicSpline && !declared_cubic {
            declared_cubic = true;
            bag.push_major(DiagCode::CUBIC_TANGENT_DROPPED);
            report.declarations.push(String::from(
                "CUBICSPLINE 通道：入/出切线分量未落进 SoA 轨道（轨道无切线位），只取值组并按线性标记",
            ));
        }

        // 重二：采样合法性。
        match validate_samples(&in_acc.data, &out, &out_acc.data, semantic, bag) {
            SampleVerdict::Reject(code) => {
                report.channels_skipped = report.channels_skipped.saturating_add(1);
                report.mapping.push(MappingRecord {
                    channel: ch_idx,
                    path: ch.path,
                    semantic: None,
                    track_start: tracks.len() as u32,
                    track_count: 0,
                    keys_in: 0,
                    keys_out: 0,
                    verdict: Some(code),
                });
                report.warnings.push(ImportWarning::new(code, ch_idx, code.label()));
                ci += 1;
                continue;
            }
            SampleVerdict::Ok => {}
        }

        // 重三：曲线异常检测 + 落地（钳制 / 丢帧 / 超密计数）。
        let anomaly = detect_curve_anomaly(&in_acc.data, &out, &out_acc.data);
        if anomaly.non_finite_values > 0 {
            bag.push_major(DiagCode::VALUE_NON_FINITE);
            report.warnings.push(ImportWarning::new(
                DiagCode::VALUE_NON_FINITE,
                ch_idx,
                "采样值含 NaN/Inf，已按语义钳制（旋转→单位四元数，其余→0）",
            ));
        }
        if anomaly.non_finite_times > 0 {
            bag.push_major(DiagCode::TIME_NON_FINITE);
            report.warnings.push(ImportWarning::new(
                DiagCode::TIME_NON_FINITE,
                ch_idx,
                "采样时刻含 NaN/Inf，该帧已丢弃（时刻不可钳制，钳到 0 会把末帧叠回首帧）",
            ));
        }
        if anomaly.overdense_pairs > 0 {
            bag.push_major(DiagCode::OVERDENSE_KEYS);
            report.warnings.push(ImportWarning::new(
                DiagCode::OVERDENSE_KEYS,
                ch_idx,
                "存在量化后零间隔的相邻帧（秒→毫秒取整所致）；已保留并计数，未静默合并",
            ));
        }
        if anomaly.negative_times > 0 {
            bag.push(DiagCode::NEGATIVE_TIME);
            report.warnings.push(ImportWarning::new(
                DiagCode::NEGATIVE_TIME,
                ch_idx,
                "存在负时刻，已钳制到 0",
            ));
        }

        let produced = import_channel(
            ch_idx,
            ch,
            semantic,
            &in_acc.data,
            &out,
            &out_acc.data,
            fidelity,
            tracks.len() as u32,
            &mut report,
            bag,
        );
        tracks.extend(produced);
        report.channels_imported = report.channels_imported.saturating_add(1);
        ci += 1;
    }

    // 报告口径自洽：轨道数 = 实际产出。
    report.tracks = tracks.len() as u32;
    Ok(ImportResult { tracks, report })
}

/// 单通道落地：把一条通道变成 1 条（位置/缩放/旋转）或 N 条（形态键）轨道。
fn import_channel(
    ch_idx: u32,
    ch: ChannelRef,
    semantic: TrackSemantic,
    times: &[f32],
    out: &GltfSamplerOut,
    values: &[f32],
    fidelity: Fidelity,
    track_start: u32,
    report: &mut ImportReport,
    bag: &mut DiagBag,
) -> Vec<ImportedTrack> {
    let lanes = semantic.lanes();
    let stride = out.expected_value_comps();
    // 形态键并行轨数 = 声明的基础分量数（`expected_value_comps` 含切线组，
    // 故 morph 轨数须用 `comps` 而不是 stride，否则 CUBICSPLINE 下会按
    // 3 倍数产出幽灵轨）。
    let base_comps = out.comps as usize;

    // 逐帧构建：丢非有限时刻帧；值非有限按语义钳制。
    let mut ms_times: Vec<u32> = Vec::new();
    let mut flat: Vec<f32> = Vec::new();
    let mut clamped_total = 0u32;
    let mut normalized_total = 0u32;

    let mut k = 0usize;
    while k < times.len() {
        let t = match times.get(k) {
            Some(v) => *v,
            None => break,
        };
        if !t.is_finite() {
            k += 1;
            continue;
        }
        let secs = if t < 0.0 { 0.0 } else { t };
        ms_times.push(secs_to_ms(secs));
        let base = out.value_base(k);
        // **行宽 = `base_comps`（声明的输出分量数），不是语义 lanes**。
        //
        // 形态键的语义 lanes 是 1（每轨一个权重），但**一行必须容纳全部
        // 形态键分量**——否则只写入分量 0，随后每个 morph 轨都从同一列取值，
        // 症状是「N 个形态键的动画完全相同」。这里的 `row` 缓冲按
        // `base_comps` 开，抽取时再按各轨的 `morph_slot` 取对应列。
        let mut row = [0.0f32; MAX_ROW_COMPONENTS];
        let mut c = 0usize;
        while c < base_comps {
            let mut v = match values.get(base + c) {
                Some(x) => *x,
                None => 0.0,
            };
            if !v.is_finite() {
                // 形态键是标量权重，钳制目标 0；旋转按分量语义（w→1）。
                v = if semantic == TrackSemantic::MorphWeight {
                    0.0
                } else {
                    clamp_target(semantic, c)
                };
                clamped_total = clamped_total.saturating_add(1);
            }
            if c < row.len() {
                row[c] = v;
            }
            c += 1;
        }
        // 旋转归一化（F2423 联动：导入期归一化 + 运行期防御归一化）。
        if semantic.needs_slerp() && normalize_quat(&mut row[..4]) {
            normalized_total = normalized_total.saturating_add(1);
        }
        // 写入展平缓冲（行宽 `base_comps`）。
        let mut w = 0usize;
        while w < base_comps {
            flat.push(if w < row.len() { row[w] } else { 0.0 });
            w += 1;
        }
        k += 1;
    }

    let keys_in = times.len() as u32;
    let keys_kept = ms_times.len() as u32;

    // 抽帧（F2425 联动位）。**端点由 thin_keys 保证**，此处不再动时间轴。
    //
    // **行宽用 `base_comps`**（与上面写入时的行宽一致），不是语义 lanes。
    let mut out_times = ms_times;
    let mut out_flat = flat;
    if fidelity.is_simplified() && out_times.len() > 2 {
        let eps = fidelity.epsilon();
        let oc = thin_keys(&out_times, &out_flat, base_comps, eps);
        if oc.keys_out < out_times.len() as u32 {
            // **按 thin_keys 返回的保留下标重建**（贪心单源，不在此重算）。
            let mut nt: Vec<u32> = Vec::new();
            let mut nf: Vec<f32> = Vec::new();
            let mut i = 0usize;
            while i < oc.kept.len() {
                let src = oc.kept[i];
                if let Some(t) = out_times.get(src) {
                    nt.push(*t);
                }
                let mut c = 0usize;
                while c < base_comps {
                    nf.push(lane(&out_flat, base_comps, src, c));
                    c += 1;
                }
                i += 1;
            }
            out_times = nt;
            out_flat = nf;
        }
    }

    // 落地为轨道。
    let carrier = semantic.carrier();
    let interp = out.interp.to_interp();
    let mut produced: Vec<ImportedTrack> = Vec::new();

    if semantic == TrackSemantic::MorphWeight {
        // 形态键：输出 comps 个标量轨并行（F2441 前向对接）。
        //
        // **取值步长必须是 `base_comps`**：`out_flat` 的行宽是 `base_comps`
        // （一行装下全部形态键分量），第 m 个形态键取每行的第 m 列。
        // 传 `lanes`(=1) 会让所有形态键都读第 0 列——症状是
        // 「N 个形态键的动画一模一样」，而每条轨的 `shape_ok()` 仍为真。
        let mut m = 0usize;
        while m < base_comps {
            let mut tv: Vec<u32> = Vec::new();
            let mut vv: Vec<f32> = Vec::new();
            let mut i = 0usize;
            while i < out_times.len() {
                if let Some(t) = out_times.get(i) {
                    tv.push(*t);
                }
                vv.push(lane(&out_flat, base_comps, i, m));
                i += 1;
            }
            produced.push(ImportedTrack {
                semantic,
                target_node: ch.target_node,
                morph_slot: m as u16,
                name: format!("m.morph/n{}/s{}", ch.target_node, m),
                track: SoaTrack::new(
                    &format!("m.morph/n{}/s{}", ch.target_node, m),
                    TrackClass::Continuous,
                    TrackValueKind::Scalar,
                    false,
                    tv,
                    vv,
                ),
                interp,
                slerp: false,
                normalized: 0,
                clamped: clamped_total,
            });
            m += 1;
        }
    } else {
        let name = format!("m.{}/n{}", semantic_tag(semantic), ch.target_node);
        produced.push(ImportedTrack {
            semantic,
            target_node: ch.target_node,
            morph_slot: u16::MAX,
            name: name.clone(),
            track: SoaTrack::new(&name, TrackClass::Continuous, carrier, false, out_times, out_flat),
            interp,
            slerp: semantic.needs_slerp(),
            normalized: normalized_total,
            clamped: clamped_total,
        });
    }

    let keys_out = if produced.is_empty() { 0 } else { produced[0].track.key_count() as u32 };
    report.keys_in = report.keys_in.saturating_add(keys_in as u64);
    report.keys_out = report.keys_out.saturating_add(keys_out as u64);
    report.clamped = report.clamped.saturating_add(clamped_total);
    report.mapping.push(MappingRecord {
        channel: ch_idx,
        path: ch.path,
        semantic: Some(semantic),
        // 通道 → 轨道下标区间必须对得上「已产出轨道数」，故用调用方传入的
        // 起点（`tracks.len()`），而不是中途尚未结算的 `report.tracks`。
        track_start,
        track_count: produced.len() as u32,
        keys_in,
        keys_out,
        verdict: None,
    });
    if keys_in != keys_kept {
        bag.push(DiagCode::TIME_NON_FINITE);
    }
    produced
}

/// 语义 → 轨道名标签（**零指纹**：不含资产名/节点名）。
pub const fn semantic_tag(s: TrackSemantic) -> &'static str {
    match s {
        TrackSemantic::Position => "pos",
        TrackSemantic::Rotation => "rot",
        TrackSemantic::Scale => "scl",
        TrackSemantic::MorphWeight => "morph",
    }
}

/// 抽帧保留判定已并入 [`thin_keys`]（保留集由它返回，本模块不再有第二份贪心）。

// ---------------------------------------------------------------------------
// 九、族声明与冒烟
// ---------------------------------------------------------------------------

/// 家族声明一致性（判据用）。
pub fn family_is_consistent() -> bool {
    DiagCode::ALL.len() == 19
        && ChannelPath::ALL.len() == 4
        && TrackSemantic::ALL.len() == 4
        && TrackSemantic::Rotation.lanes() == 4
        && TrackSemantic::MorphWeight.lanes() == 1
        && GltfInterp::CubicSpline.value_groups() == 3
}

/// 三重校验的码域互不重合（**证明三重各有独立拒绝面**）。
pub fn triple_validation_codes_disjoint() -> bool {
    let mut i = 0usize;
    while i < DiagCode::ALL.len() {
        let c = DiagCode::ALL[i];
        let memberships =
            u32::from(c.is_ref_class()) + u32::from(c.is_sample_class()) + u32::from(c.is_curve_class());
        // 每个码至多属一重；既不属三重的「映射/声明」类码 memberships = 0。
        if memberships > 1 {
            return false;
        }
        // 三重的每一重都必须至少有一个码（否则那一重形同虚设）。
        i += 1;
    }
    let mut ref_n = 0u32;
    let mut sample_n = 0u32;
    let mut curve_n = 0u32;
    let mut k = 0usize;
    while k < DiagCode::ALL.len() {
        let c = DiagCode::ALL[k];
        if c.is_ref_class() {
            ref_n += 1;
        }
        if c.is_sample_class() {
            sample_n += 1;
        }
        if c.is_curve_class() {
            curve_n += 1;
        }
        k += 1;
    }
    ref_n > 0 && sample_n > 0 && curve_n > 0
}

/// 码标签互异（防两码共用一句人话——那是最难查的一类缺陷）。
pub fn labels_unique() -> bool {
    let mut i = 0usize;
    while i < DiagCode::ALL.len() {
        let mut j = i + 1;
        while j < DiagCode::ALL.len() {
            if DiagCode::ALL[i].label() == DiagCode::ALL[j].label() {
                return false;
            }
            j += 1;
        }
        i += 1;
    }
    true
}

/// 描述。
pub fn describe() -> String {
    let mut s = String::new();
    let _ = s.push_str("动画导入（glTF animation → M 域轨道）：\n");
    let _ = s.push_str("· 四通道映射表是带摘要的数据结构：漂移即置拦截，拦截态下导入整体拒绝（不是记一笔）。\n");
    let _ = s.push_str("· 四通道外通道跳过 + 指名声明（支持范围诚实：translation/rotation/scale/weights）。\n");
    let _ = s.push_str("· 三重校验各有独立拒绝码域：引用完整性 4 码 / 采样合法性 5 码 / 曲线异常 4 码，不共用。\n");
    let _ = s.push_str("· 保真默认逐帧落地；精简是带阈值的 enum 变体（无阈值的精简不可复现，拒收）。\n");
    let _ = s.push_str("· 值 NaN 按语义钳制（旋转→单位四元数）、时刻 NaN 丢帧（钳到 0 会把末帧叠回首帧）。\n");
    let _ = s.push_str("· 超密（秒→毫秒取整产生的零间隔）计数并警告，不静默合并——那是资产侧的真实信息。\n");
    let _ = s.push_str("· 抽帧端点钉死，误差对最终保留集重算（删除当时的近邻会随后续删除失效）。\n");
    let _ = s.push_str("· 零指纹：轨道名与警告文本只由语义+节点下标构造，不含资产名/路径。\n");
    let _ = s.push_str("· CUBICSPLINE 只取值组、切线丢弃并声明（SoA 轨道无切线存储位）。\n");
    s
}

/// 冒烟：导一份三通道小文档。
pub fn smoke() -> String {
    let acc_t = AccessorView::new(ComponentType::Float, false, 3, 1, vec![0.0f32, 0.5, 1.0]);
    let acc_p = AccessorView::new(
        ComponentType::Float,
        false,
        3,
        3,
        vec![0.0f32, 0.0, 0.0, 1.0, 0.0, 0.0, 2.0, 0.0, 0.0],
    );
    let acc_r = AccessorView::new(
        ComponentType::Float,
        false,
        3,
        4,
        vec![0.0f32, 0.0, 0.0, 1.0, 0.0, 0.0, 0.707, 0.707, 0.0, 0.0, 0.0, 1.0],
    );
    let doc = GltfAnimDoc::new(
        1,
        vec![acc_t, acc_p, acc_r],
        vec![
            SamplerRef { input: 0, output: 1, interp: GltfInterp::Linear },
            SamplerRef { input: 0, output: 2, interp: GltfInterp::Linear },
        ],
        vec![
            ChannelRef { target_node: 0, path: ChannelPath::Translation, sampler: 0 },
            ChannelRef { target_node: 0, path: ChannelPath::Rotation, sampler: 1 },
        ],
        "smoke.gltf",
    );
    let mut table = MappingTable::standard();
    let mut bag = DiagBag::new();
    let _ = table.reconcile(&mut bag);
    match import_gltf_anim(&doc, Fidelity::Faithful, &table, &mut bag) {
        Err(e) => e.render(),
        Ok(res) => res.report.render(),
    }
}
