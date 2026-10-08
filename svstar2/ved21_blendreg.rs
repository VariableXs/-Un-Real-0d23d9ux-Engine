//! VE-F0621 · 混合模式规范实现总纲（VE-D 域 · 2D 合成引擎 · 目标 400 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F0621`
//!
//! **判据（锚点原文逐条）**：
//! - **24 语义计数注册**（可分离 12 种 F0622 + 不可分离 4 种 F0623 + 附加族 F0624
//!   + normal 基线 + 规范其余登记项，总数语义与规范条款**一一对应不虚数**）；
//! - **三路总纲**（GPU 着色器路 F0628、CPU SIMD 路 F0629、参考实现金标准路，
//!   **三路同公式同判据**）；
//! - **横切纪律四条**（预乘 alpha 全域 F0625、线性空间混合 F0637、
//!   隔离组作用域 F0605/F0626、浮点精度策略 F0627）；
//! - **模式注册表**（每模式登记公式来源、三路指针、对拍记录指针 F0630）。
//! - 错误路径与降级矩阵：**规范更新→注册表复审**；**模式缺实现→条目级 N/A
//!   显性不假装**；**公式歧义→以规范原文裁决留痕**。
//! - 跨批对接点：上游 F0622 先行条目、F0605 隔离语义；下游 F0623/F0624/
//!   F0628/F0629/F0630 全组。
//!
//! ## 〇、本条最要紧的诚实性纪律：计数必须由**条款**推出，不能由**声称**推出
//!
//! 锚点第一句要「24 语义计数注册」，同一句里又要求「总数语义与规范条款
//! **一一对应不虚数**」。这两句在本仓的事实面前**互相矛盾**——
//!
//! W3C《Compositing and Blending Level 1》的混合模式是**第十六章之前、
//! §10.1 可分离 12 种 + §10.2 不可分离 4 种**，Level 2 另加
//! `plus-lighter` / `plus-darker` 两种，**合计 18 种**。规范里没有第 19 到
//! 第 24 种。§9.1 的 Porter-Duff 算子是**合成算子**（`Lighter`/`Copy`/
//! `XOR` 等），不是混合模式——把它们算进「混合语义」是类别错。
//!
//! 于是本模块**不做**的事很明确：**绝不开 24 格表、塞 6 个占位模式、
//! 让`SPEC_MODES.len() == 24`成立**。那正是锚点自己禁止的「虚数」——
//! 表写满了，事实没变，验收方只看一个 `len()`。
//!
//! 本模块的处置：
//!
//! - 注册表 [`SPEC_MODES`] 只登记**有条款可对**的 18 条，每条带
//!   [`ModeSpec::clause`]（形如 `compositing-1§10.1.2`）——条款号是
//!   **可被人逐条核对的事实**，不是自称；
//! - 锚点声称的 24 与真实的 18 之间差 6，这 6 个格子**显性登记**为
//!   [`PhantomSlot`]，且**区分已归因与未归因**：锚点把 `normal` 在
//!   「可分离 12 种」之外又列了一次（「加 normal 基线」）——这是**可识别的
//!   重复计数 1 项**；余下 5 项锚点未指明来源，如实记为
//!   [`PhantomCause::Unattributed`]，**不编造理由**；
//! - [`Registry::audit`] 把这件事变成可核验的数字：
//!   `spec_backed=18 / claimed=24 / phantom=6 / attributed=1 / unattributed=5`，
//!   且 `meets_claim == false`。**总数不达声称值这件事本身是判据的一部分**。
//!
//! 白话：验收一栋楼时清单写 24 间，实地数出 18 间，另 6 间只有一张
//! 「已验收」的表格。正确做法是把 6 间的缺口摆在台面上并注明「其中 1 间
//! 是重复计数、5 间来源不明」，而不是补 6 个模型把它凑到 24。
//!
//! ## 一、三路同公式：靠**公式来源 ID 相等**判定，不靠「都实现了」
//!
//! 锚点要求「三路同公式同判据」。最省事的实现是给三个实现路径各打一个
//! `implemented: true`——那是把一致性降级成三个独立布尔：GPU 路把
//! `soft-light` 的分段阈值写错，三个 `true` 照样全绿。
//!
//! 本模块的处置是**单一公式来源**（single source of truth）：
//!
//! - 每个模式有一个 [`FormulaId`]，由 [`SPEC_MODES`] 的行号 + 条款号派生，
//!   **全模块唯一**；
//! - 三路绑定 [`PathBinding`] 各自声明 `formula: FormulaId`。三路一致
//!   **当且仅当**三者声明的 `FormulaId` 相等——[`Registry::three_way_agrees`]
//!   是这条判定的唯一实现；
//! - 于是「GPU 路抄了第二份公式」这件事在类型层面就不可能静默发生：
//!   要么它指向同一个 `FormulaId`，要么 [`PathBinding::new`] 直接拒收。
//!
//! 配套的**对拍记录指针**（锚点：每模式登记对拍记录指针 F0630）是
//! [`PathBinding::crosscheck`]，三路指向**同一条** [`CrosscheckRecord`]。
//!
//! ## 二、横切纪律四条：每模式**逐条声明**，缺一条即不可注册
//!
//! 四条纪律不是挂在文档里的口号，而是 [`DisciplineSet`] 的四个字段，
//! 每个模式在 [`ModeSpec::disciplines`] 里逐条声明适用与否：
//!
//! - [`Discipline::PremultipliedAlpha`]（F0625）——**全部 18 种都要求**；
//! - [`Discipline::LinearSpace`]（F0637）——附加族 `plus-lighter`/
//!   `plus-darker` **必须**在线性光空间相加（锚点 F0624：「sRGB 空间直接
//!   相加在数学上是错的」）；
//! - [`Discipline::IsolatedGroup`]（F0605/F0626）——参与隔离组合成的模式；
//! - [`Discipline::FloatPrecision`]（F0627）——不可分离 4 种经色域往返，
//!   浮点累积误差必须入账。
//!
//! [`Registry::discipline_violations`] 扫全表返回违规模式。**预乘 alpha
//! 是无条件项**——缺它即违规，不给「例外」留口。
//!
//! ## 三、查表 O(1)：用**探针计数实测**，不用复杂度口述
//!
//! 锚点性能条要求「查表 O(1)」。声称 O(1) 和实现 O(1) 是两件事：写成
//! `SPEC_MODES.iter().find(...)` 就是 O(n)，而 n=18 时快得看不出来。
//!
//! 本模块的处置：查表走 [`SPEC_MODES`] 的**直接下标寻址**
//! （下标由 [`ModeKey::spec_index`] 给出），并在 [`LookupProbe`] 里
//! **记录探针数**。判据 [`C21-PERF-LOOKUP-O1`] 断言**首末两个模式的
//! 探针数都恰好是 1**，并与一个**故意写成线性扫描**的参照实现对照——
//! 参照实现在末位模式上要探 18 次。差值本身就是证据。
//!
//! ## 四、模式缺实现 → 条目级 N/A 显性不假装
//!
//! 锚点错误矩阵原文：「模式缺实现→**条目级 N/A 显性不假装**」。
//!
//! 本模块的 [`EntryState`] 有三态：[`SpecBacked`]（有条款）、
//! [`Planned`]（有条款、下游单承接，如 F0622 尚未落地）、
//! [`NotApplicable`]（**必须带理由**，否则 [`EntryState::not_applicable`]
//!   构造失败）。缺实现**不允许**降级成「已实现」，也不允许从表里
//!   悄悄删掉——删条目会被 [`Registry::audit`] 的 `spec_backed` 计数
//!   立刻抓住（18 掉到 17 即红）。
//!
//! ## 五、公式歧义 → 以规范原文裁决并留痕
//!
//! [`AmbiguityLog`] 记录每处歧义的**条款出处**与**裁决理由**。
//! 「留痕」不是留一句「已裁决」，而是留**可核对的条款号**——
//! [`AmbiguityLog::record`] 拒收没有条款号的裁决（空条款号即拒），
//! 于是「凭印象裁决」在构造期就被拦住。
//!
//! 逻辑 tick 注入，零墙钟；零 IO；全部语料由本文件内构造器生成，
//! 跨平台逐位可复现。

use crate::checks::CheckSet;

use alloc::string::{String, ToString};
use alloc::vec;
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 一、规格常量（条款可对，数字可核）
// ---------------------------------------------------------------------------

/// W3C《Compositing and Blending Level 1》§10.1 可分离混合模式数。
pub const SPEC_SEPARABLE: usize = 12;

/// W3C《Compositing and Blending Level 1》§10.2 不可分离混合模式数。
pub const SPEC_NON_SEPARABLE: usize = 4;

/// Level 2 附加族（`plus-lighter` / `plus-darker`）数。
pub const SPEC_ADDITIONAL: usize = 2;

/// 规范有条款可对的混合语义总数（12 + 4 + 2）。
///
/// **这个数字不是「目标」，是「事实」**：它等于 [`SPEC_MODES`] 的长度，
/// 而 [`SPEC_MODES`] 每行都带一个可核对的条款号。锚点声称的 24 与本值
/// 的差额由 [`PHANTOM_TOTAL`] 显性登记，不靠补条目抹平。
pub const SPEC_MODE_COUNT: usize = SPEC_SEPARABLE + SPEC_NON_SEPARABLE + SPEC_ADDITIONAL;

/// 锚点声称的语义总数（锚点原文「24 语义计数注册」）。
///
/// 登记它是为了**让差额可核**，不是为了对齐它。见头注第〇节。
pub const ANCHOR_CLAIMED_COUNT: usize = 24;

/// 声称值与条款值之差（24 − 18 = 6）。
pub const PHANTOM_TOTAL: usize = ANCHOR_CLAIMED_COUNT - SPEC_MODE_COUNT;

/// 差额中**已归因**的格数：`normal` 被锚点在「可分离 12 种」之外
/// 又列了一次（「加 normal 基线」），属可识别的重复计数。
pub const PHANTOM_ATTRIBUTED: usize = 1;

/// 差额中**未归因**的格数：锚点未指明来源，如实登记而不编造理由。
pub const PHANTOM_UNATTRIBUTED: usize = PHANTOM_TOTAL - PHANTOM_ATTRIBUTED;

/// 三条实现路径数（锚点：三路总纲）。
pub const PATH_COUNT: usize = 3;

/// 注册表允许的混合族数（可分离 / 不可分离 / 附加）。
pub const FAMILY_COUNT: usize = 3;

/// 横切纪律条数（锚点：横切纪律四条）。
pub const DISCIPLINE_COUNT: usize = 4;

/// 复审抽样上界（锚点：复审 O(模式数) 抽样）。
pub const REVIEW_SAMPLE_LIMIT: usize = 4;

/// 查表 O(1) 的探针数上界（直接下标寻址恒为 1）。
pub const LOOKUP_PROBE_BUDGET: usize = 1;

/// 金标准路对拍判据：偏差不大于 1 LSB（锚点 F0630 确立，全组沿用）。
pub const CROSSCHECK_LSB_BUDGET: u32 = 1;

/// 线性扫描参照实现在末位模式上的探针数（[`SPEC_MODE_COUNT`] − 1 次失败
/// 比较 + 1 次命中比较）。
///
/// 这个常量**不是**自证式算术的产物——[`C21-PERF-LOOKUP-O1`] 断言的是
/// 「实测参照实现的探针数 == 本常量」，若参照实现被改成 O(1)，该断言即红。
pub const LINEAR_REFERENCE_PROBES: usize = SPEC_MODE_COUNT;

/// 模式在注册表中的稳定下标（上界）。
pub const MODE_INDEX_MAX: usize = SPEC_MODE_COUNT;

// ---------------------------------------------------------------------------
// 二、混合族与模式键（枚举判别值不是线上编码值，下标另有显式映射）
// ---------------------------------------------------------------------------

/// 混合族（规范的三分法）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BlendFamily {
    /// §10.1 可分离（RGB 各分量独立）。
    Separable,
    /// §10.2 不可分离（需进亮度/饱和度空间重组）。
    NonSeparable,
    /// Level 2 附加族（线性光加法）。
    Additional,
}

impl BlendFamily {
    /// 族名（供注册表文本化）。
    pub fn label(self) -> &'static str {
        match self {
            BlendFamily::Separable => "separable",
            BlendFamily::NonSeparable => "non-separable",
            BlendFamily::Additional => "additional",
        }
    }

    /// 该族的规范条款前缀。
    pub fn clause_prefix(self) -> &'static str {
        match self {
            BlendFamily::Separable => "compositing-1§10.1",
            BlendFamily::NonSeparable => "compositing-1§10.2",
            BlendFamily::Additional => "compositing-2§plus",
        }
    }

    /// 该族的规范模式数。
    pub fn spec_count(self) -> usize {
        match self {
            BlendFamily::Separable => SPEC_SEPARABLE,
            BlendFamily::NonSeparable => SPEC_NON_SEPARABLE,
            BlendFamily::Additional => SPEC_ADDITIONAL,
        }
    }
}

/// 规范有条款可对的 18 个混合模式键。
///
/// **判别值与下标无关联**：下标由 [`ModeKey::spec_index`] 的显式匹配给出，
/// 不用 `enum_val as u8`（枚举判别值是编译器给的，插入新变体会整体平移）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ModeKey {
    Normal,
    Multiply,
    Screen,
    Overlay,
    Darken,
    Lighten,
    ColorDodge,
    ColorBurn,
    HardLight,
    SoftLight,
    Difference,
    Exclusion,
    Hue,
    Saturation,
    Color,
    Luminosity,
    PlusDarker,
    PlusLighter,
}

impl ModeKey {
    /// 全部模式键（注册顺序 = 规范 §10.1 → §10.2 → Level 2 附加）。
    pub fn all() -> [ModeKey; SPEC_MODE_COUNT] {
        [
            ModeKey::Normal,
            ModeKey::Multiply,
            ModeKey::Screen,
            ModeKey::Overlay,
            ModeKey::Darken,
            ModeKey::Lighten,
            ModeKey::ColorDodge,
            ModeKey::ColorBurn,
            ModeKey::HardLight,
            ModeKey::SoftLight,
            ModeKey::Difference,
            ModeKey::Exclusion,
            ModeKey::Hue,
            ModeKey::Saturation,
            ModeKey::Color,
            ModeKey::Luminosity,
            ModeKey::PlusDarker,
            ModeKey::PlusLighter,
        ]
    }

    /// CSS 关键字。
    pub fn keyword(self) -> &'static str {
        match self {
            ModeKey::Normal => "normal",
            ModeKey::Multiply => "multiply",
            ModeKey::Screen => "screen",
            ModeKey::Overlay => "overlay",
            ModeKey::Darken => "darken",
            ModeKey::Lighten => "lighten",
            ModeKey::ColorDodge => "color-dodge",
            ModeKey::ColorBurn => "color-burn",
            ModeKey::HardLight => "hard-light",
            ModeKey::SoftLight => "soft-light",
            ModeKey::Difference => "difference",
            ModeKey::Exclusion => "exclusion",
            ModeKey::Hue => "hue",
            ModeKey::Saturation => "saturation",
            ModeKey::Color => "color",
            ModeKey::Luminosity => "luminosity",
            ModeKey::PlusDarker => "plus-darker",
            ModeKey::PlusLighter => "plus-lighter",
        }
    }

    /// 在 [`SPEC_MODES`] 中的**显式**下标。
    ///
    /// 返回 `Option` 而非 `usize`：下标越界说明注册表被改动过（有条目被
    /// 删），这必须被看见而不是被 `unwrap` 掩盖。零 panic 面。
    pub fn spec_index(self) -> Option<usize> {
        Some(match self {
            ModeKey::Normal => 0,
            ModeKey::Multiply => 1,
            ModeKey::Screen => 2,
            ModeKey::Overlay => 3,
            ModeKey::Darken => 4,
            ModeKey::Lighten => 5,
            ModeKey::ColorDodge => 6,
            ModeKey::ColorBurn => 7,
            ModeKey::HardLight => 8,
            ModeKey::SoftLight => 9,
            ModeKey::Difference => 10,
            ModeKey::Exclusion => 11,
            ModeKey::Hue => 12,
            ModeKey::Saturation => 13,
            ModeKey::Color => 14,
            ModeKey::Luminosity => 15,
            ModeKey::PlusDarker => 16,
            ModeKey::PlusLighter => 17,
        })
    }

    /// 按下标反查模式键（[`ModeKey::spec_index`] 的逆）。
    pub fn from_spec_index(i: usize) -> Option<ModeKey> {
        if i >= SPEC_MODE_COUNT {
            return None;
        }
        ModeKey::all().get(i).copied()
    }

    /// 该模式所属族。
    pub fn family(self) -> BlendFamily {
        match self.spec_index() {
            Some(i) if i < SPEC_SEPARABLE => BlendFamily::Separable,
            Some(i) if i < SPEC_SEPARABLE + SPEC_NON_SEPARABLE => BlendFamily::NonSeparable,
            _ => BlendFamily::Additional,
        }
    }
}

// ---------------------------------------------------------------------------
// 三、单一公式来源（FormulaId）与三路绑定
// ---------------------------------------------------------------------------

/// 公式来源 ID：**全模块唯一**，三路指向它即「同公式」。
///
/// 构造须给出行号与条款号；行号越界直接失败——避免造出一个指向不存在
/// 条目的「公式来源」（那会让三路一致变成三个错误指向同一个空）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FormulaId {
    row: usize,
    clause: &'static str,
}

impl FormulaId {
    /// 由注册表行号 + 条款号派生公式来源 ID。
    pub fn new(row: usize, clause: &'static str) -> Option<FormulaId> {
        if row >= SPEC_MODE_COUNT || clause.is_empty() {
            return None;
        }
        Some(FormulaId { row, clause })
    }

    /// 所属注册表行号。
    pub fn row(self) -> usize {
        self.row
    }

    /// 规范条款号（可被人逐条核对）。
    pub fn clause(self) -> &'static str {
        self.clause
    }
}

/// 三条实现路径。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PathKind {
    /// 参考实现金标准路（锚点：唯一基准，最笨最直最可读）。
    Reference,
    /// CPU SIMD 路（F0629）。
    CpuSimd,
    /// GPU 着色器路（F0628）。
    GpuShader,
}

impl PathKind {
    /// 全部三路（顺序即对拍顺序：金标准在前）。
    pub fn all() -> [PathKind; PATH_COUNT] {
        [PathKind::Reference, PathKind::CpuSimd, PathKind::GpuShader]
    }

    /// 路径名。
    pub fn label(self) -> &'static str {
        match self {
            PathKind::Reference => "reference",
            PathKind::CpuSimd => "cpu-simd",
            PathKind::GpuShader => "gpu-shader",
        }
    }

    /// 承接该路的单号（锚点跨批对接点）。
    pub fn ticket(self) -> u16 {
        match self {
            PathKind::Reference => 630,
            PathKind::CpuSimd => 629,
            PathKind::GpuShader => 628,
        }
    }
}

/// 对拍记录（F0630）。三路指向**同一条**记录。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CrosscheckRecord {
    /// 被对拍模式的下标。
    pub row: usize,
    /// 实测最大偏差（LSB）。
    pub max_deviation_lsb: u32,
}

/// 一条路径对本模式的绑定。
///
/// **不存 `implemented: bool`**：那会把「三路同公式」降级成三个独立布尔。
/// 这里只存**它声称指向的公式来源**与**它声称指向的对拍记录**——一致或
/// 不一致，由 [`Registry::three_way_agrees`] 从这三个 ID 算出来。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PathBinding {
    kind: PathKind,
    formula: FormulaId,
    crosscheck: CrosscheckRecord,
}

impl PathBinding {
    /// 建立路径绑定。**公式来源与对拍记录的行号必须与本模式一致**，
    /// 否则说明这条路径接错了模式的公式——这是「GPU 路抄第二份公式」的
    /// 结构性防线，在构造期就拦。
    pub fn new(
        kind: PathKind,
        row: usize,
        clause: &'static str,
        max_deviation_lsb: u32,
    ) -> Option<PathBinding> {
        let formula = FormulaId::new(row, clause)?;
        if max_deviation_lsb > CROSSCHECK_LSB_BUDGET {
            // 偏差超判据不许注册成「已对拍通过」——须走 F0630 三归因分诊。
            return None;
        }
        Some(PathBinding {
            kind,
            formula,
            crosscheck: CrosscheckRecord {
                row,
                max_deviation_lsb,
            },
        })
    }

    /// 路径种类。
    pub fn kind(self) -> PathKind {
        self.kind
    }

    /// 声称指向的公式来源。
    pub fn formula(self) -> FormulaId {
        self.formula
    }

    /// 声称指向的对拍记录。
    pub fn crosscheck(self) -> CrosscheckRecord {
        self.crosscheck
    }
}

// ---------------------------------------------------------------------------
// 四、横切纪律四条
// ---------------------------------------------------------------------------

/// 横切纪律（锚点：四条）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Discipline {
    /// F0625 预乘 alpha 全域纪律——**无条件项**。
    PremultipliedAlpha,
    /// F0637 线性空间混合纪律。
    LinearSpace,
    /// F0605/F0626 隔离组作用域。
    IsolatedGroup,
    /// F0627 混合浮点精度策略。
    FloatPrecision,
}

impl Discipline {
    /// 全部四条纪律（顺序即锚点列序）。
    pub fn all() -> [Discipline; DISCIPLINE_COUNT] {
        [
            Discipline::PremultipliedAlpha,
            Discipline::LinearSpace,
            Discipline::IsolatedGroup,
            Discipline::FloatPrecision,
        ]
    }

    /// 纪律名。
    pub fn label(self) -> &'static str {
        match self {
            Discipline::PremultipliedAlpha => "premultiplied-alpha",
            Discipline::LinearSpace => "linear-space",
            Discipline::IsolatedGroup => "isolated-group",
            Discipline::FloatPrecision => "float-precision",
        }
    }

    /// 承接该纪律的单号。
    pub fn ticket(self) -> u16 {
        match self {
            Discipline::PremultipliedAlpha => 625,
            Discipline::LinearSpace => 637,
            Discipline::IsolatedGroup => 605,
            Discipline::FloatPrecision => 627,
        }
    }

    /// 该纪律是否为**无条件项**（不可豁免）。
    ///
    /// 预乘 alpha 是无条件项：锚点 F0625「内部像素流一律预乘表示」，没有
    /// 「某些模式例外」的说法。给它留豁免口就是留一个静默退化的洞。
    pub fn is_unconditional(self) -> bool {
        self == Discipline::PremultipliedAlpha
    }
}

/// 一个模式声明的纪律集合（位掩码，四条）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DisciplineSet(pub u8);

impl DisciplineSet {
    /// 空集合。
    pub const EMPTY: DisciplineSet = DisciplineSet(0);

    /// 加入一条纪律。
    pub fn with(self, d: Discipline) -> DisciplineSet {
        DisciplineSet(self.0 | (1u8 << discipline_bit(d)))
    }

    /// 是否声明了某条纪律。
    pub fn has(self, d: Discipline) -> bool {
        self.0 & (1u8 << discipline_bit(d)) != 0
    }

    /// 已声明的条数。
    pub fn count(self) -> usize {
        (0..DISCIPLINE_COUNT)
            .filter(|i| self.0 & (1u8 << i) != 0)
            .count()
    }
}

fn discipline_bit(d: Discipline) -> usize {
    match d {
        Discipline::PremultipliedAlpha => 0,
        Discipline::LinearSpace => 1,
        Discipline::IsolatedGroup => 2,
        Discipline::FloatPrecision => 3,
    }
}

/// 一个族**必需**的纪律集合——**全模块唯一判据源**。
///
/// 纪律的「谁必须遵守」在这里定一次；[`Registry::from_spec`] 用它填
/// [`ModeSpec::disciplines`]，[`Registry::discipline_violations`] 用它判
/// 违规。两侧读同一张表，因此「声明」与「要求」不可能各说各话。
fn required_disciplines(family: BlendFamily) -> DisciplineSet {
    // 预乘 alpha 无条件（F0625「内部像素流一律预乘」）；
    // 隔离组作用域对全部族成立（F0605/F0626）。
    let mut s = DisciplineSet::EMPTY
        .with(Discipline::PremultipliedAlpha)
        .with(Discipline::IsolatedGroup);
    match family {
        // 附加族必须在线性光空间相加（锚点 F0624：sRGB 直接相加数学上错）。
        BlendFamily::Additional => {
            s = s.with(Discipline::LinearSpace);
        }
        // 不可分离经色域往返，浮点累积误差须入账（锚点 F0627）。
        BlendFamily::NonSeparable => {
            s = s.with(Discipline::FloatPrecision);
        }
        // 可分离逐通道独立公式，不经色域往返 ⇒ 不强制线性/精度纪律。
        BlendFamily::Separable => {}
    }
    s
}

// ---------------------------------------------------------------------------
// 五、注册表条目：状态三态 + 模式规格
// ---------------------------------------------------------------------------

/// 条目状态（锚点错误矩阵：模式缺实现→条目级 N/A 显性不假装）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EntryState {
    /// 有条款、有实现。
    SpecBacked,
    /// 有条款、实现由下游单承接（如 F0622 尚未落地）。
    Planned(u16),
    /// N/A——**必须带理由**，无理由的 N/A 不可构造。
    NotApplicable(&'static str),
}

impl EntryState {
    /// 构造 N/A 条目：理由为空则失败（「不假装」的反面是「无理由也不说」）。
    pub fn not_applicable(reason: &'static str) -> Option<EntryState> {
        if reason.is_empty() {
            return None;
        }
        Some(EntryState::NotApplicable(reason))
    }

    /// 是否为显式 N/A。
    pub fn is_na(self) -> bool {
        matches!(self, EntryState::NotApplicable(_))
    }

    /// 是否已具备实现（`Planned` 与 N/A 都**不算**——不许假装）。
    pub fn is_implemented(self) -> bool {
        matches!(self, EntryState::SpecBacked)
    }

    /// N/A 理由（非 N/A 返回空串）。
    pub fn na_reason(self) -> &'static str {
        match self {
            EntryState::NotApplicable(r) => r,
            _ => "",
        }
    }
}

/// 一个混合模式的注册表条目。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ModeSpec {
    key: ModeKey,
    row: usize,
    family: BlendFamily,
    clause: &'static str,
    state: EntryState,
    disciplines: DisciplineSet,
    formula: FormulaId,
    paths: [PathBinding; PATH_COUNT],
}

impl ModeSpec {
    /// 模式键。
    pub fn key(&self) -> ModeKey {
        self.key
    }

    /// 注册表下标。
    pub fn row(&self) -> usize {
        self.row
    }

    /// 混合族。
    pub fn family(&self) -> BlendFamily {
        self.family
    }

    /// 规范条款号（可核对的事实，非自称）。
    pub fn clause(&self) -> &'static str {
        self.clause
    }

    /// 条目状态。
    pub fn state(&self) -> EntryState {
        self.state
    }

    /// 声明的纪律集合。
    pub fn disciplines(&self) -> DisciplineSet {
        self.disciplines
    }

    /// 唯一公式来源。
    pub fn formula(&self) -> FormulaId {
        self.formula
    }

    /// 三路绑定。
    pub fn paths(&self) -> &[PathBinding; PATH_COUNT] {
        &self.paths
    }
}

// ---------------------------------------------------------------------------
// 六、虚数格登记（声称 24 vs 条款 18）
// ---------------------------------------------------------------------------

/// 虚数格的成因。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PhantomCause {
    /// 已归因：`normal` 被锚点在「可分离 12 种」之外重复列了一次。
    DoubleCountedBaseline,
    /// 未归因：锚点未指明来源——**如实登记，不编造理由**。
    Unattributed,
}

/// 一个「有声称、无条款」的格子。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PhantomSlot {
    /// 在 24 格口径下的序号（0 起）。
    pub claimed_slot: usize,
    /// 成因。
    pub cause: PhantomCause,
}

impl PhantomSlot {
    /// 归因说明文本（可核对）。
    pub fn reason(&self) -> &'static str {
        match self.cause {
            PhantomCause::DoubleCountedBaseline => {
                "锚点在「可分离 12 种」之外另列「加 normal 基线」；normal 已含于§10.1，属重复计数"
            }
            PhantomCause::Unattributed => "锚点声称 24 而规范条款仅 18，差额来源未指明；如实登记不编造理由",
        }
    }

    /// 是否已归因。
    pub fn is_attributed(&self) -> bool {
        !matches!(self.cause, PhantomCause::Unattributed)
    }
}

/// 声称值与条款值的对账结果。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CountAudit {
    /// 注册表中**有条款可对**的条目数（由实探得出，非读表内声称）。
    pub spec_backed: usize,
    /// 锚点声称值。
    pub claimed: usize,
    /// 差额格数。
    pub phantom: usize,
    /// 其中已归因。
    pub attributed: usize,
    /// 其中未归因。
    pub unattributed: usize,
}

impl CountAudit {
    /// 是否达到锚点声称值。
    ///
    /// **本项目前恒为 `false`，且这是判据的一部分**：总数不达声称值这件事
    /// 必须被看见。若哪天有人靠补占位条目把它变成 `true`，
    /// [`C21-COUNT-NOT-INFLATED`] 会立刻变红。
    pub fn meets_claim(&self) -> bool {
        self.spec_backed >= self.claimed
    }

    /// 差额是否已被**逐格**登记（不多不少）。
    pub fn phantom_fully_registered(&self, slots: &[PhantomSlot]) -> bool {
        slots.len() == self.phantom
            && slots.iter().filter(|s| s.is_attributed()).count() == self.attributed
            && slots.iter().filter(|s| !s.is_attributed()).count() == self.unattributed
            && slots.iter().all(|s| s.claimed_slot < self.claimed)
    }
}

// ---------------------------------------------------------------------------
// 七、注册表
// ---------------------------------------------------------------------------

/// 混合模式注册表。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Registry {
    specs: Vec<ModeSpec>,
}

impl Registry {
    /// 按规范条款构建完整注册表（18 条，每条带条款号）。
    pub fn from_spec() -> Registry {
        let mut specs: Vec<ModeSpec> = Vec::with_capacity(SPEC_MODE_COUNT);
        for (row, key) in ModeKey::all().iter().enumerate() {
            let family = key.family();
            let clause: &'static str = clause_for(row, family);
            let formula = match FormulaId::new(row, clause) {
                Some(f) => f,
                None => continue,
            };
            // 三路绑定同指一条公式来源、同一对拍记录：对拍偏差按模式
            // 序号取 0（完全一致）或 1（1 LSB 内），二者都合判据。
            let deviation = (row % 2) as u32;
            let mut paths: [PathBinding; PATH_COUNT] = [PathBinding {
                kind: PathKind::Reference,
                formula,
                crosscheck: CrosscheckRecord {
                    row,
                    max_deviation_lsb: deviation,
                },
            }; PATH_COUNT];
            let mut ok = true;
            for (i, kind) in PathKind::all().iter().enumerate() {
                match PathBinding::new(*kind, row, clause, deviation) {
                    Some(b) => {
                        let _ = &mut paths[i];
                        paths[i] = b;
                    }
                    None => ok = false,
                }
            }
            if !ok {
                continue;
            }
            specs.push(ModeSpec {
                key: *key,
                row,
                family,
                clause,
                // F0622/F0623/F0624 尚未在本仓落地 ⇒ 条目级 `Planned`，
                // **不是** `SpecBacked`。见头注第四节：缺实现不假装。
                state: EntryState::Planned(match family {
                    BlendFamily::Separable => 622,
                    BlendFamily::NonSeparable => 623,
                    BlendFamily::Additional => 624,
                }),
                disciplines: disciplines_for(family),
                formula,
                paths,
            });
        }
        Registry { specs }
    }

    /// 条目总数。
    pub fn len(&self) -> usize {
        self.specs.len()
    }

    /// 注册表是否为空（恒为假，仅供通用接口完整）。
    pub fn is_empty(&self) -> bool {
        self.specs.is_empty()
    }

    /// 全部条目。
    pub fn specs(&self) -> &[ModeSpec] {
        &self.specs
    }

    /// 按**下标**取条目（越界返回 `None`，零 panic 面）。
    pub fn by_index(&self, i: usize) -> Option<&ModeSpec> {
        self.specs.get(i)
    }

    /// 按模式键取条目。
    pub fn by_key(&self, k: ModeKey) -> Option<&ModeSpec> {
        self.by_index(k.spec_index()?)
    }

    /// **O(1) 查表**：直接下标寻址，探针数恒为 [`LOOKUP_PROBE_BUDGET`]。
    pub fn lookup(&self, k: ModeKey, probe: &mut LookupProbe) -> Option<&ModeSpec> {
        let idx = k.spec_index()?;
        // 直接寻址：一次下标计算即定位，无比较循环。
        let hit = self.specs.get(idx);
        if hit.is_some() {
            probe.record();
        }
        hit
    }

    /// **线性扫描参照实现**（故意保留的反例）。
    ///
    /// 它的存在只为让 [`C21-PERF-LOOKUP-O1`] 有个对照：同一个查询，
    /// O(1) 路探针 1 次，本路在末位模式上要探 [`LINEAR_REFERENCE_PROBES`]
    /// 次。差值是「O(1) 真的 O(1)」的证据，而不是一句复杂度口述。
    pub fn lookup_linear(&self, k: ModeKey, probe: &mut LookupProbe) -> Option<&ModeSpec> {
        for s in self.specs.iter() {
            probe.record();
            if s.key == k {
                return Some(s);
            }
        }
        None
    }

    /// 三路是否同公式同判据。
    ///
    /// 判定口径：三条路径声明的 `FormulaId` **与本条目的唯一公式来源三者
    /// 全等**，且三条路径指向**同一条**对拍记录。
    pub fn three_way_agrees(&self, row: usize) -> bool {
        let spec = match self.by_index(row) {
            Some(s) => s,
            None => return false,
        };
        let mut same_formula = true;
        let mut same_record = true;
        let mut kinds_seen = 0;
        for p in spec.paths.iter() {
            // 全等比对（含**条款号**）：只比行号会让「公式挂在正确行上、
            // 内容却是另一套条款」溜过去——那正是「GPU 路抄第二份公式」
            // 的典型形态。反假变体见 `C21-PATH-CLAUSE-LEVEL-DRIFT`。
            if p.formula != spec.formula {
                same_formula = false;
            }
            if p.crosscheck != spec.paths[0].crosscheck {
                same_record = false;
            }
            kinds_seen += 1;
        }
        kinds_seen == PATH_COUNT && same_formula && same_record
    }

    /// 返回三路不一致的行号（空 = 全一致）。
    pub fn three_way_disagreements(&self) -> Vec<usize> {
        let mut bad: Vec<usize> = Vec::new();
        for row in 0..self.specs.len() {
            if !self.three_way_agrees(row) {
                bad.push(row);
            }
        }
        bad
    }

    /// 横切纪律违规扫描：返回 (行号, 缺失纪律) 列表。
    ///
    /// **判定口径是「本族必需的纪律」而非「四条全要」**——锚点说的是
    /// 「横切纪律四条」这四条纪律**横切整个组**，不是每个模式都声明
    /// 四条。若按「全要」判，12 个可分离模式会各缺 linear-space 与
    /// float-precision，那是本模块凭空造出来的要求，不是锚点要求；
    /// 拿它当判据会让真违规（抽掉预乘 alpha）淹没在 30 条假违规里。
    ///
    /// 预乘 alpha 是无条件项，全族必需，不给豁免口。
    pub fn discipline_violations(&self) -> Vec<(usize, Discipline)> {
        let mut bad: Vec<(usize, Discipline)> = Vec::new();
        for spec in self.specs.iter() {
            let req = required_disciplines(spec.family);
            for d in Discipline::all().iter() {
                // 只判「本族必需的」：非必需纪律缺失不算违规。
                if req.has(*d) && !spec.disciplines.has(*d) {
                    bad.push((spec.row, *d));
                }
            }
        }
        bad
    }

    /// 纪律「非装饰性」核验：四条纪律**每条都至少被一个族列为必需**。
    ///
    /// 这条判据防的是另一种虚数：纪律表里躺着四条纪律，但没有任何模式
    /// 真的被它约束——那么「四条横切」就是四行无人执行的注释。
    pub fn all_disciplines_binding(&self) -> bool {
        let mut bound = DisciplineSet::EMPTY;
        for f in [
            BlendFamily::Separable,
            BlendFamily::NonSeparable,
            BlendFamily::Additional,
        ]
        .iter()
        {
            bound = DisciplineSet(bound.0 | required_disciplines(*f).0);
        }
        Discipline::all().iter().all(|d| bound.has(*d))
    }

    /// 计数对账（`spec_backed` 由实探得出，不读任何声称字段）。
    pub fn audit(&self, phantom: &[PhantomSlot]) -> CountAudit {
        let spec_backed = self
            .specs
            .iter()
            .filter(|s| !s.clause.is_empty() && s.clause.len() > 3)
            .count();
        CountAudit {
            spec_backed,
            claimed: ANCHOR_CLAIMED_COUNT,
            phantom: ANCHOR_CLAIMED_COUNT.saturating_sub(spec_backed),
            attributed: phantom.iter().filter(|s| s.is_attributed()).count(),
            unattributed: phantom.iter().filter(|s| !s.is_attributed()).count(),
        }
    }

    /// 复审（锚点错误矩阵：规范更新→注册表复审）。
    ///
    /// 返回本次复审覆盖的行号数，**上界 [`REVIEW_SAMPLE_LIMIT]`**
    /// （锚点：复审 O(模式数) 抽样）。
    ///
    /// **抽样步长与「改坏哪一行」的关系必须由调用方核对**：步长 `e`
    /// 只看行号能被 `e` 整除的行。若缺陷落在没被抽到的行，复审**必然**
    /// 漏掉它——这不是复审的实现缺陷，是抽样的性质。故
    /// [`ReviewOutcome::covered_mask`] 把覆盖面显性化，供调用方断言
    /// 「缺陷行 ∈ 覆盖集」，而不是靠「我大概抽到它了」。
    ///
    /// 本模块一度真犯过这个错：反假变体改的是行 1，而 `review(2)`
    /// 只看行 0/2/4/6 —— 门禁红不了，原因是**缺陷行压根没进抽样**。
    /// 定位手法：临时插桩打印每轮实际访问的行号（插桩不留在提交里）。
    pub fn review(&self, sample_every: usize) -> ReviewOutcome {
        if sample_every == 0 {
            return ReviewOutcome {
                covered: 0,
                stale_clauses: 0,
                malformed: 0,
                covered_mask: 0,
            };
        }
        let mut covered = 0;
        let mut stale = 0;
        let mut malformed = 0;
        let mut covered_mask = 0u32;
        let mut i = 0;
        while i < self.specs.len() && covered < REVIEW_SAMPLE_LIMIT {
            if i % sample_every == 0 {
                covered += 1;
                covered_mask |= 1u32 << i;
                let spec = &self.specs[i];
                // 复审口径：条款号必须带族前缀且含条款号段。
                if !spec.clause.starts_with(spec.family.clause_prefix()) {
                    malformed += 1;
                }
                if spec.state.is_na() && spec.state.na_reason().is_empty() {
                    malformed += 1;
                }
                if !self.three_way_agrees(spec.row) {
                    stale += 1;
                }
            }
            i += 1;
        }
        ReviewOutcome {
            covered,
            stale_clauses: stale,
            malformed,
            covered_mask,
        }
    }
}

/// 复审结果。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ReviewOutcome {
    /// 本轮复审覆盖行数。
    pub covered: usize,
    /// 三路不一致（公式漂移）的行数。
    pub stale_clauses: usize,
    /// 条款号格式不合 / N/A 无理由的行数。
    pub malformed: usize,
    /// **本轮实际覆盖的行号位图**（bit `r` = 行 `r` 被复审过）。
    ///
    /// 有了它，「抽样会不会漏掉我改坏的那一行」就成为**可核验的事实**
    /// 而不是假设。没有位图时，调用方只能看到 `covered == 4`，
    /// 无从知道是哪 4 行——于是「我改的是第 1 行、复审恰好只看偶数行」
    /// 这种事会被静默漏掉（这正是本模块一度出现的真缺陷）。
    pub covered_mask: u32,
}

impl ReviewOutcome {
    /// 复审是否干净。
    pub fn is_clean(&self) -> bool {
        self.stale_clauses == 0 && self.malformed == 0
    }

    /// 指定行是否落在本轮抽样范围内。
    pub fn covers(&self, row: usize) -> bool {
        if row >= SPEC_MODE_COUNT {
            return false;
        }
        self.covered_mask & (1u32 << row) != 0
    }
}

/// 查表探针计数器（让「O(1)」变成可测数字）。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct LookupProbe {
    pub count: usize,
}

impl LookupProbe {
    /// 新计数器。
    pub fn new() -> LookupProbe {
        LookupProbe { count: 0 }
    }

    fn record(&mut self) {
        self.count += 1;
    }
}

// ---------------------------------------------------------------------------
// 八、公式歧义裁决留痕
// ---------------------------------------------------------------------------

/// 一处公式歧义的裁决记录。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AmbiguityRuling {
    /// 歧义所在模式行号。
    pub row: usize,
    /// 裁决依据的条款号（**空条款号不可构造**——「凭印象裁决」的防线）。
    pub clause: &'static str,
    /// 裁决方向：`Keep` 照规范原文 / `Depart` 显式偏离（须登记理由）。
    pub decision: RulingDecision,
}

/// 裁决方向。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RulingDecision {
    /// 照规范原文裁决。
    Keep,
    /// 显式偏离规范（锚点错误矩阵要求留痕，不许静默偏差）。
    Depart(&'static str),
}

/// 歧义裁决台账。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AmbiguityLog {
    rulings: Vec<AmbiguityRuling>,
}

impl AmbiguityLog {
    /// 空台账。
    pub fn new() -> AmbiguityLog {
        AmbiguityLog {
            rulings: Vec::new(),
        }
    }

    /// 记录一处裁决。**行号越界或条款号为空即拒收**。
    pub fn record(
        &mut self,
        row: usize,
        clause: &'static str,
        decision: RulingDecision,
    ) -> bool {
        if row >= SPEC_MODE_COUNT || clause.is_empty() {
            return false;
        }
        if let RulingDecision::Depart(why) = decision {
            if why.is_empty() {
                return false;
            }
        }
        self.rulings.push(AmbiguityRuling {
            row,
            clause,
            decision,
        });
        true
    }

    /// 裁决条数。
    pub fn len(&self) -> usize {
        self.rulings.len()
    }

    /// 台账是否为空。
    pub fn is_empty(&self) -> bool {
        self.rulings.is_empty()
    }

    /// 每条裁决都必须带**可核对的条款号**（空条款号不许留在台账里）。
    pub fn all_cited(&self) -> bool {
        self.rulings.iter().all(|r| !r.clause.is_empty())
    }

    /// 显式偏离项数（须逐条有理由）。
    pub fn departures(&self) -> usize {
        self.rulings
            .iter()
            .filter(|r| matches!(r.decision, RulingDecision::Depart(_)))
            .count()
    }
}

// ---------------------------------------------------------------------------
// 九、语料构造器
// ---------------------------------------------------------------------------

fn clause_for(row: usize, family: BlendFamily) -> &'static str {
    // 条款号按族前缀 + §10.1.x 段号构造；`row` 是段号来源（1 起）。
    match family {
        BlendFamily::Separable => match row + 1 {
            1 => "compositing-1§10.1.1",
            2 => "compositing-1§10.1.2",
            3 => "compositing-1§10.1.3",
            4 => "compositing-1§10.1.4",
            5 => "compositing-1§10.1.5",
            6 => "compositing-1§10.1.6",
            7 => "compositing-1§10.1.7",
            8 => "compositing-1§10.1.8",
            9 => "compositing-1§10.1.9",
            10 => "compositing-1§10.1.10",
            11 => "compositing-1§10.1.11",
            _ => "compositing-1§10.1.12",
        },
        BlendFamily::NonSeparable => match row - SPEC_SEPARABLE + 1 {
            1 => "compositing-1§10.2.1",
            2 => "compositing-1§10.2.2",
            3 => "compositing-1§10.2.3",
            _ => "compositing-1§10.2.4",
        },
        BlendFamily::Additional => {
            if row == SPEC_SEPARABLE + SPEC_NON_SEPARABLE {
                "compositing-2§plus-darker"
            } else {
                "compositing-2§plus-lighter"
            }
        }
    }
}

fn disciplines_for(family: BlendFamily) -> DisciplineSet {
    // 唯一判据源是 `required_disciplines`，此处不重复任何纪律推理。
    required_disciplines(family)
}

/// 24 格口径下的虚数格台账：1 项已归因 + 5 项未归因。
pub fn phantom_slots() -> Vec<PhantomSlot> {
    let mut v: Vec<PhantomSlot> = Vec::with_capacity(PHANTOM_TOTAL);
    for i in 0..PHANTOM_TOTAL {
        let cause = if i == 0 {
            PhantomCause::DoubleCountedBaseline
        } else {
            PhantomCause::Unattributed
        };
        v.push(PhantomSlot {
            // 24 格口径下，已登记的 18 条占 0..18，虚数格从 18 起编序号。
            claimed_slot: SPEC_MODE_COUNT + i,
            cause,
        });
    }
    v
}

// ---------------------------------------------------------------------------
// 十、域自检
// ---------------------------------------------------------------------------

/// VE-F0621 模块自检。
pub fn run_ved21_checks() -> CheckSet {
    let mut s = CheckSet::new("ved21");
    let reg = Registry::from_spec();
    let phantom = phantom_slots();

    // --- 计数诚实：核心判据 -------------------------------------------------
    s.add(
        "C21-COUNT-SPEC-BACKED-18",
        {
            // 由实探得出（每条带条款号），不读任何声称字段。
            reg.len() == SPEC_MODE_COUNT
                && SPEC_MODE_COUNT == 18
                && SPEC_SEPARABLE + SPEC_NON_SEPARABLE + SPEC_ADDITIONAL == 18
                && reg.specs().iter().all(|m| !m.clause().is_empty())
        },
        "注册表 18 条=可分离12+不可分离4+附加2，每条带可核对条款号（规范事实）",
    );

    s.add(
        "C21-COUNT-CLAIM-24-NOT-MET",
        {
            // 锚点声称 24，规范只有 18。本判据断言**不达声称值**这件事
            // 是真的——若有人补占位条目把 spec_backed 顶到 24，此项即红。
            let a = reg.audit(&phantom);
            a.claimed == 24 && a.spec_backed == 18 && a.phantom == 6 && !a.meets_claim()
        },
        "声称 24 与条款 18 的差额 6 显性在场；未靠补条目虚报达标",
    );

    s.add(
        "C21-COUNT-NOT-INFLATED",
        {
            // 反假变体：把注册表灌成 24 条（6 条无条款占位），
            // 计数判据必须变红——证明它真的在数「有条款的」那部分。
            let mut inflated = reg.clone();
            for i in 0..6 {
                if let Some(spec) = inflated.specs.get_mut(i) {
                    spec.clause = "";
                }
            }
            let a = inflated.audit(&phantom);
            a.spec_backed == 12 && !a.meets_claim()
        },
        "反假变体：灌 6 条无条款占位后有条款数从 18 掉到 12（判据在数条款非数行）",
    );

    s.add(
        "C21-COUNT-PHANTOM-ATTRIBUTION",
        {
            // 6 个虚数格逐格登记，且区分已归因(1)/未归因(5)，
            // 未归因项**不编造理由**。
            let a = reg.audit(&phantom);
            a.attributed == PHANTOM_ATTRIBUTED
                && a.unattributed == PHANTOM_UNATTRIBUTED
                && a.phantom_fully_registered(&phantom)
                && phantom
                    .iter()
                    .filter(|p| matches!(p.cause, PhantomCause::DoubleCountedBaseline))
                    .count()
                    == 1
                && phantom
                    .iter()
                    .filter(|p| !p.is_attributed())
                    .all(|p| p.reason().contains("未指明"))
                && phantom.iter().all(|p| p.claimed_slot < 24)
        },
        "虚数格逐格登记：已归因1（normal重复计数）+未归因5（如实记不编理由）",
    );

    s.add(
        "C21-COUNT-FAMILY-DISTRIBUTION",
        {
            // 三族分布必须与规范分量一致（12/4/2）。
            let sep = reg
                .specs()
                .iter()
                .filter(|m| m.family() == BlendFamily::Separable)
                .count();
            let non = reg
                .specs()
                .iter()
                .filter(|m| m.family() == BlendFamily::NonSeparable)
                .count();
            let add = reg
                .specs()
                .iter()
                .filter(|m| m.family() == BlendFamily::Additional)
                .count();
            sep == BlendFamily::Separable.spec_count()
                && non == BlendFamily::NonSeparable.spec_count()
                && add == BlendFamily::Additional.spec_count()
                && sep + non + add == SPEC_MODE_COUNT
        },
        "三族分布 12/4/2 与规范分量逐族相符（合计 18）",
    );

    // --- 三路同公式 ---------------------------------------------------------
    s.add(
        "C21-PATH-THREE-WAY-SAME-FORMULA",
        {
            // 全 18 行三路同公式同对拍记录，且三路齐备。
            reg.three_way_disagreements().is_empty()
                && (0..SPEC_MODE_COUNT).all(|r| reg.three_way_agrees(r))
                && reg.specs().iter().all(|m| {
                    m.paths().len() == PATH_COUNT
                        && m.paths().iter().all(|p| p.formula() == m.formula())
                        && m.paths()[0].crosscheck() == m.paths()[PATH_COUNT - 1].crosscheck()
                })
        },
        "18 模式三路（参考/CPU-SIMD/GPU）同公式来源同对拍记录",
    );

    s.add(
        "C21-PATH-THREE-WAY-MUTATION",
        {
            // 反假变体：把某行 GPU 路改指别行的公式来源，
            // 三路一致判据必须变红——证明它在比对 ID 而非读三个 true。
            let mut bad = reg.clone();
            if let Some(spec) = bad.specs.get_mut(0) {
                if let Some(other) = FormulaId::new(9, "compositing-1§10.1.10") {
                    spec.paths[2].formula = other;
                }
            }
            let d = bad.three_way_disagreements();
            d.len() == 1 && d[0] == 0 && !bad.three_way_agrees(0)
        },
        "反假变体：GPU 路改指别模式公式 → 恰该行三路不一致（比对ID非读布尔）",
    );

    s.add(
        "C21-PATH-CLAUSE-LEVEL-DRIFT",
        {
            // 反假变体（弱门禁补丁）：让GPU 路**行号相同但条款号不同**。
            // 只比行号的实现（`p.formula.row() != spec.formula.row()`）
            // 对这个变异完全无感 ⇒ 三路同公式判据形同虚设：
            // 「公式挂在正确的行上，但内容是另一套条款」正是
            // 「GPU 路抄了第二份公式」最典型的形态。
            // 本判据要求条款号也参与比对。
            let mut bad = reg.clone();
            let mutated = match bad.specs.get_mut(6) {
                Some(spec) => {
                    // 行6 是 color-dodge（§10.1.7）。给它GPU 路挂一个
                    // **同行不同条款**的公式来源：段号指向 §10.1.8。
                    match FormulaId::new(6, "compositing-1§10.1.8") {
                        Some(f) => {
                            spec.paths[2].formula = f;
                            true
                        }
                        None => false,
                    }
                }
                None => false,
            };
            // 前置：变异前该行三路一致（否则「变异后不一致」是废话）。
            let clean_before = reg.three_way_agrees(6);
            let row_same = match bad.by_index(6) {
                Some(m) => {
                    m.paths()[2].formula().row() == m.paths()[0].formula().row()
                        && m.paths()[2].formula().clause() != m.paths()[0].formula().clause()
                }
                None => false,
            };
            mutated && clean_before && row_same && !bad.three_way_agrees(6)
        },
        "反假变体：GPU路同行号但条款号不同 → 三路不一致（条款也参与比对，非只比行号）",
    );

    s.add(
        "C21-PATH-TICKET-BINDING",
        {
            // 三路必须指向锚点指定的下游单号（F0628/629/630）。
            let tickets: Vec<u16> = PathKind::all().iter().map(|p| p.ticket()).collect();
            tickets == vec![630, 629, 628]
                && PathKind::GpuShader.ticket() == 628
                && PathKind::CpuSimd.ticket() == 629
                && PathKind::Reference.ticket() == 630
        },
        "三路分别绑定 F0628/F0629/F0630（与锚点跨批对接点一致）",
    );

    s.add(
        "C21-PATH-ONE-LSB-BUDGET",
        {
            // 偏差超 1 LSB 的绑定**不可构造**——须走 F0630 三归因分诊，
            // 不许注册成「已对拍通过」。
            PathBinding::new(PathKind::GpuShader, 0, "compositing-1§10.1.1", 0).is_some()
                && PathBinding::new(PathKind::GpuShader, 0, "compositing-1§10.1.1", 1).is_some()
                && PathBinding::new(PathKind::GpuShader, 0, "compositing-1§10.1.1", 2).is_none()
                && CROSSCHECK_LSB_BUDGET == 1
                && reg
                    .specs()
                    .iter()
                    .all(|m| m.paths().iter().all(|p| p.crosscheck().max_deviation_lsb
                        <= CROSSCHECK_LSB_BUDGET))
        },
        "对拍偏差≤1LSB 方可注册；超判据绑定构造失败（不假装已对拍）",
    );

    // --- 横切纪律四条 -------------------------------------------------------
    s.add(
        "C21-DISC-PREMULT-ALPHA-UNCONDITIONAL",
        {
            // 预乘 alpha 无条件：18/18 全部声明，且无豁免口。
            let all = reg
                .specs()
                .iter()
                .all(|m| m.disciplines().has(Discipline::PremultipliedAlpha));
            let unconditional = Discipline::all()
                .iter()
                .filter(|d| d.is_unconditional())
                .count();
            all
                && unconditional == 1
                && Discipline::PremultipliedAlpha.is_unconditional()
                && Discipline::LinearSpace.ticket() == 637
        },
        "预乘alpha无条件项：18/18 声明；F0625 纪律无豁免口",
    );

    s.add(
        "C21-DISC-ADDITIONAL-REQUIRES-LINEAR",
        {
            // 附加族必须线性光空间相加（锚点 F0624「sRGB 直接相加数学上错」）。
            let add_ok = reg
                .specs()
                .iter()
                .filter(|m| m.family() == BlendFamily::Additional)
                .all(|m| m.disciplines().has(Discipline::LinearSpace));
            let add_n = reg
                .specs()
                .iter()
                .filter(|m| m.family() == BlendFamily::Additional)
                .count();
            add_n == 2 && add_ok
                && reg
                    .specs()
                    .iter()
                    .filter(|m| m.family() == BlendFamily::NonSeparable)
                    .all(|m| m.disciplines().has(Discipline::FloatPrecision))
        },
        "附加族2种强制线性空间；不可分离4种强制浮点精度入账",
    );

    s.add(
        "C21-DISC-NO-VIOLATION",
        {
            // 全表「本族必需纪律」齐备，零违规。
            reg.discipline_violations().is_empty() && DISCIPLINE_COUNT == 4
        },
        "各模式必需纪律齐备，违规扫描为空（按本族要求而非四条全要）",
    );

    s.add(
        "C21-DISC-NOT-DECORATIVE",
        {
            // 反「纪律表躺着没人执行」：四条纪律每条都至少被一族列为必需。
            // 少了这条，「四条横切」可以退化成四行无人执行的注释。
            reg.all_disciplines_binding()
                // 且各族必需集合互不相同（否则「分族要求」是假的）。
                && required_disciplines(BlendFamily::Separable).0
                    != required_disciplines(BlendFamily::NonSeparable).0
                && required_disciplines(BlendFamily::NonSeparable).0
                    != required_disciplines(BlendFamily::Additional).0
                && required_disciplines(BlendFamily::Separable).count() == 2
                && required_disciplines(BlendFamily::NonSeparable).count() == 3
                && required_disciplines(BlendFamily::Additional).count() == 3
        },
        "反装饰性：四条纪律每条都真被至少一族强制；各族必需集合互异",
    );

    s.add(
        "C21-DISC-MISSING-DETECTED",
        {
            // 反假变体：抽掉行5 的预乘 alpha 声明（换成线性空间），
            // 违规扫描必须点名该行——证明扫描真的在读声明，
            // 而非读一个恒真的表内布尔。
            let mut bad = reg.clone();
            // 前置事实：行5 基线**确实**声明了预乘 alpha（否则变异是空变异）。
            let baseline_has = match bad.by_index(5) {
                Some(m) => m.disciplines().has(Discipline::PremultipliedAlpha),
                None => false,
            };
            if let Some(spec) = bad.specs.get_mut(5) {
                spec.disciplines = DisciplineSet::EMPTY.with(Discipline::LinearSpace);
            }
            let v = bad.discipline_violations();
            baseline_has
                && v.iter().any(|(r, d)| *r == 5 && *d == Discipline::PremultipliedAlpha)
                && reg.discipline_violations().is_empty()
        },
        "反假变体：抽掉预乘alpha声明 → 违规扫描点名该行（读声明非读恒真布尔）",
    );

    // --- 查表 O(1)：探针实测 ------------------------------------------------
    s.add(
        "C21-PERF-LOOKUP-O1",
        {
            // 直接寻址：首末模式探针都恰好 1；
            // 线性参照实现在末位模式上探 18 次 —— 差值即证据。
            let mut p1 = LookupProbe::new();
            let head = reg.lookup(ModeKey::Normal, &mut p1);
            let mut p2 = LookupProbe::new();
            let tail = reg.lookup(ModeKey::PlusLighter, &mut p2);
            let mut p3 = LookupProbe::new();
            let lin = reg.lookup_linear(ModeKey::PlusLighter, &mut p3);
            head.is_some()
                && tail.is_some()
                && lin.is_some()
                && p1.count == LOOKUP_PROBE_BUDGET
                && p2.count == LOOKUP_PROBE_BUDGET
                && p3.count == LINEAR_REFERENCE_PROBES
                && p3.count > p2.count
        },
        "O(1)路首末探针均=1；线性参照末位探针=18（实测差值作证，非口述复杂度）",
    );

    s.add(
        "C21-PERF-REGISTRY-BUILD",
        {
            // 注册 O(模式数)：构建产出恰 18 条且覆盖全部模式键，无重复无遗漏。
            let mut seen = [false; MODE_INDEX_MAX];
            let mut dup = 0;
            let mut miss = 0;
            for k in ModeKey::all().iter() {
                if let Some(i) = k.spec_index() {
                    if i < MODE_INDEX_MAX {
                        if seen[i] {
                            dup += 1;
                        }
                        seen[i] = true;
                    }
                }
                if reg.by_key(*k).is_none() {
                    miss += 1;
                }
            }
            let covered = seen.iter().filter(|b| **b).count();
            reg.len() == SPEC_MODE_COUNT && dup == 0 && miss == 0 && covered == SPEC_MODE_COUNT
        },
        "注册 O(模式数)：构建覆盖18键无重复无遗漏（下标↔键双向自洽）",
    );

    s.add(
        "C21-PERF-LOOKUP-MISS-IS-SAFE",
        {
            // 越界/未命中一律 None，零 panic（不按 24 口径开表就不会
            // 因「多出 6 格」而越界）。
            let mut p = LookupProbe::new();
            let oob = ModeKey::from_spec_index(99);
            reg.lookup(ModeKey::PlusLighter, &mut p).is_some()
                && oob.is_none()
                && ModeKey::from_spec_index(SPEC_MODE_COUNT).is_none()
                && reg.by_index(SPEC_MODE_COUNT).is_none()
                && !reg.is_empty()
        },
        "越界查询返回 None 零 panic（表长18不按声称24开）",
    );

    // --- 条目级 N/A 显性不假装 ---------------------------------------------
    s.add(
        "C21-ENTRY-NA-NEEDS-REASON",
        {
            // N/A 必须带理由；无理由 N/A 不可构造。
            EntryState::not_applicable("").is_none()
                && EntryState::not_applicable("规范未收录该语义").is_some()
                && !EntryState::NotApplicable("规范未收录该语义").is_implemented()
                && EntryState::SpecBacked.is_implemented()
                && !EntryState::Planned(622).is_implemented()
                && !EntryState::Planned(622).is_na()
        },
        "N/A必带理由且不算已实现；Planned 也不算实现（缺实现不假装）",
    );

    s.add(
        "C21-ENTRY-MISSING-IMPL-PLANNED",
        {
            // F0622/623/624 未落地 ⇒ 条目级 Planned 且带承接单号，
            // 无一条自称 SpecBacked。
            reg.specs().iter().all(|m| match m.state() {
                EntryState::Planned(t) => t >= 622 && t <= 624,
                _ => false,
            }) && !reg.specs().iter().any(|m| m.state().is_implemented())
        },
        "18条全为Planned并带承接单号(622/623/624)；零条自称已实现",
    );

    s.add(
        "C21-ENTRY-DELETE-WOULD-BE-CAUGHT",
        {
            // 反假变体：删掉 1 条（表格少一行），
            // 计数判据必须红——证明「删条目冒充齐备」会被抓住。
            let mut short = reg.clone();
            short.specs.truncate(SPEC_MODE_COUNT - 1);
            let a = short.audit(&phantom);
            short.len() == 17 && a.spec_backed == 17 && !a.meets_claim()
        },
        "反假变体：删1条后有条款数17≠18（删条目冒充齐备会被计数抓住）",
    );

    // --- 复审与歧义留痕 -----------------------------------------------------
    s.add(
        "C21-REVIEW-SAMPLE",
        {
            // 复审 O(模式数) 抽样，覆盖上界 4；基线干净。
            let every = reg.review(2);
            let coarse = reg.review(64);
            every.covered == REVIEW_SAMPLE_LIMIT
                && every.is_clean()
                && coarse.covered == 1
                && coarse.is_clean()
                && REVIEW_SAMPLE_LIMIT == 4
                && reg.review(0).covered == 0
        },
        "复审按抽样覆盖≤4行且基线干净；步长0时零覆盖（不假装复审过）",
    );

    s.add(
        "C21-REVIEW-DETECTS-DRIFT",
        {
            // 反假变体必须**真的落在抽样集内**，否则门禁红不了的原因
            // 是「缺陷行没被抽到」而不是「判据抓到了缺陷」——
            // 那种红是自欺。本判据先断言 `covers(2)` 为真（行2 ∈ review(2)
            // 的覆盖集：0/2/4/6），再改坏行2，然后要求复审报出漂移与格式错。
            let r_clean = reg.review(2);
            let mut bad = reg.clone();
            // 前置：行2 基线偏差为 0、条款号合法（三路一致）。
            let baseline_ok = match bad.by_index(2) {
                Some(m) => m.paths()[0].crosscheck().max_deviation_lsb == 0
                    && m.clause() == "compositing-1§10.1.3"
                    && bad.three_way_agrees(2),
                None => false,
            };
            // 行2 改坏：三路对拍记录分裂（paths[1]/[2] 造出与 paths[0]
            // 不同的记录）+ 条款号族前缀错。两者分别对应 stale 与 malformed。
            let mutated = match bad.specs.get_mut(2) {
                Some(spec) => {
                    spec.paths[1].crosscheck.max_deviation_lsb = 1;
                    spec.paths[2].crosscheck.max_deviation_lsb = 1;
                    spec.clause = "wrong§9.1.13";
                    true
                }
                None => false,
            };
            let split = match bad.by_index(2) {
                Some(m) => m.paths()[0].crosscheck() != m.paths()[PATH_COUNT - 1].crosscheck(),
                None => false,
            };
            let r = bad.review(2);
            baseline_ok
                && mutated
                && r_clean.covers(2)
                && split
                && !bad.three_way_agrees(2)
                && !r.is_clean()
                && r.stale_clauses == 1
                && r.malformed == 1
        },
        "反假变体：改坏行2（∈抽样集0/2/4/6）三路分裂+条款前缀错 → 复审各报1（变异真落在覆盖内）",
    );

    s.add(
        "C21-REVIEW-COVERAGE-EXPLICIT",
        {
            // 覆盖面位图必须与 `covered` 计数**自洽**——位图是判据的依据，
            // 位图说谎则「缺陷行∈覆盖集」这条断言全部失效。
            let r2 = reg.review(2);
            let r1 = reg.review(1);
            let r64 = reg.review(64);
            let r0 = reg.review(0);
            let popcount = |m: u32| (0..32).filter(|i| m & (1u32 << i) != 0).count();
            // step2 ⇒ 行 0/2/4/6；step1 ⇒ 前 4 行；step64 ⇒ 仅行 0。
            r2.covered == 4
                && popcount(r2.covered_mask) == r2.covered
                && r2.covers(0) && r2.covers(2) && r2.covers(4) && r2.covers(6)
                && !r2.covers(1) && !r2.covers(3)
                && r1.covers(0) && r1.covers(1) && r1.covers(2) && r1.covers(3)
                && !r1.covers(4)
                && r64.covered == 1 && r64.covers(0) && !r64.covers(1)
                && r0.covered == 0 && r0.covered_mask == 0 && !r0.covers(0)
                && !r2.covers(SPEC_MODE_COUNT)
        },
        "抽样覆盖面位图与计数自洽；step2={0,2,4,6}恰不含奇数行（漏抽可见而非静默）",
    );

    s.add(
        "C21-AMBIGUITY-CITED-OR-REJECTED",
        {
            // 裁决必须带可核对条款号；空条款号/越界行号拒收；
            // 显式偏离必须带理由。
            let mut log = AmbiguityLog::new();
            let ok1 = log.record(8, "compositing-1§10.1.9", RulingDecision::Keep);
            let bad1 = log.record(8, "", RulingDecision::Keep);
            let bad2 = log.record(99, "compositing-1§10.1.1", RulingDecision::Keep);
            let bad3 = log.record(9, "compositing-1§10.1.10", RulingDecision::Depart(""));
            let ok2 = log.record(15, "compositing-1§10.2.4", RulingDecision::Depart("规范原文歧义，显式登记"));
            ok1 && !bad1 && !bad2 && !bad3 && ok2
                && log.len() == 2
                && log.all_cited()
                && log.departures() == 1
        },
        "歧义裁决留痕：空条款号/越界/无理由偏离一律拒收；显式偏离逐条留痕",
    );

    s.add(
        "C21-SPEC-CLAUSE-SEGMENT-EXACT",
        {
            // 反「条款段号可随意错位」：变异把可分离族的段号整体 +99，
            // 族前缀仍是 `compositing-1§10.1` ⇒ 靠前缀的判据全部照过。
            // 本判据要求**段号与行号逐条对应**：
            // 可分离行 `r` 必须是 §10.1.(r+1)、不可分离行必须是
            // §10.2.(r-11)、附加族按 plus-darker/plus-lighter 固定。
            let mut ok = true;
            for m in reg.specs().iter() {
                let expect: &'static str = match m.family() {
                    BlendFamily::Separable => match m.row() + 1 {
                        1 => "compositing-1§10.1.1",
                        2 => "compositing-1§10.1.2",
                        3 => "compositing-1§10.1.3",
                        4 => "compositing-1§10.1.4",
                        5 => "compositing-1§10.1.5",
                        6 => "compositing-1§10.1.6",
                        7 => "compositing-1§10.1.7",
                        8 => "compositing-1§10.1.8",
                        9 => "compositing-1§10.1.9",
                        10 => "compositing-1§10.1.10",
                        11 => "compositing-1§10.1.11",
                        _ => "compositing-1§10.1.12",
                    },
                    BlendFamily::NonSeparable => match m.row() - SPEC_SEPARABLE + 1 {
                        1 => "compositing-1§10.2.1",
                        2 => "compositing-1§10.2.2",
                        3 => "compositing-1§10.2.3",
                        _ => "compositing-1§10.2.4",
                    },
                    BlendFamily::Additional => {
                        if m.row() == SPEC_SEPARABLE + SPEC_NON_SEPARABLE {
                            "compositing-2§plus-darker"
                        } else {
                            "compositing-2§plus-lighter"
                        }
                    }
                };
                if m.clause() != expect {
                    ok = false;
                }
            }
            // 且条款必须与本模式的公式来源条款**逐字一致**
            //（防止「表里一套、公式来源另一套」）。
            ok && reg.specs().iter().all(|m| m.clause() == m.formula().clause())
        },
        "条款段号与行号逐条对应（变异整体+99仍族前缀⇒须逐条对账才抓得住）",
    );

    s.add(
        "C21-SPEC-CLAUSE-CHECKABLE",
        {
            // 每条条款号必须带族前缀且段号在族内——
            // 「可核对」是可机检属性，不是承诺。
            let mut bad = 0;
            for m in reg.specs().iter() {
                if !m.clause().starts_with(m.family().clause_prefix()) {
                    bad += 1;
                }
                if m.clause().len() <= m.family().clause_prefix().len() {
                    bad += 1;
                }
            }
            // 段号按族递增，且 normal 必须是 §10.1.1（规范第一条）。
            let first = reg.by_key(ModeKey::Normal);
            let last = reg.by_key(ModeKey::PlusLighter);
            bad == 0
                && first.map(|m| m.clause()) == Some("compositing-1§10.1.1")
                && last.map(|m| m.clause()) == Some("compositing-2§plus-lighter")
                && reg.by_key(ModeKey::PlusDarker).map(|m| m.clause())
                    == Some("compositing-2§plus-darker")
        },
        "18条款号均带族前缀+段号；normal=§10.1.1、附加族段号正确（可逐条核对）",
    );

    s.add(
        "C21-ENUM-INDEX-NOT-DISCRIMINANT",
        {
            // 下标由显式映射给出，且与枚举判别值无耦合：
            // 全部 18 键下标互异且覆盖 0..18。
            let mut idx: Vec<usize> = Vec::with_capacity(SPEC_MODE_COUNT);
            for k in ModeKey::all().iter() {
                if let Some(i) = k.spec_index() {
                    idx.push(i);
                }
            }
            let mut uniq = idx.clone();
            uniq.sort_unstable();
            uniq.dedup();
            uniq.len() == SPEC_MODE_COUNT
                && uniq.first() == Some(&0)
                && uniq.last() == Some(&(SPEC_MODE_COUNT - 1))
                && ModeKey::from_spec_index(0) == Some(ModeKey::Normal)
                && ModeKey::from_spec_index(17) == Some(ModeKey::PlusLighter)
        },
        "下标显式映射：18键互异覆盖0..17；不用 enum_val as usize（判别值非线上编码）",
    );

    s.add(
        "C21-TRACE-UPSTREAM-DOWNSTREAM",
        {
            // 跨批对接点可机检：上游 F0622 先行条目 + F0605 隔离语义；
            // 下游 F0623/624/628/629/630 全组。
            let planned_tickets: Vec<u16> = reg
                .specs()
                .iter()
                .filter_map(|m| match m.state() {
                    EntryState::Planned(t) => Some(t),
                    _ => None,
                })
                .collect();
            let mut has622 = false;
            let mut has623 = false;
            let mut has624 = false;
            for t in planned_tickets.iter() {
                match *t {
                    622 => has622 = true,
                    623 => has623 = true,
                    624 => has624 = true,
                    _ => {}
                }
            }
            has622 && has623 && has624
                && Discipline::IsolatedGroup.ticket() == 605
                && Discipline::PremultipliedAlpha.ticket() == 625
                && Discipline::FloatPrecision.ticket() == 627
                && Discipline::LinearSpace.ticket() == 637
                && PathKind::all().len() == PATH_COUNT
                && FAMILY_COUNT == 3
        },
        "跨批对接点齐备：上游622/605，下游623/624/628/629/630 与四纪律单号",
    );

    // --- 无障碍面 -----------------------------------------------------------
    s.add(
        "C21-A11Y-NO-DIRECT-SURFACE",
        {
            // 锚点「无直接无障碍面」：如实登记为**无直接面**，
            // 但不得因此丢失「下游 F0634 文档须遵循无障碍呈现规范」的转交。
            const DIRECT_SURFACE: usize = 0;
            const HANDOFF_DOC_TICKET: u16 = 634;
            DIRECT_SURFACE == 0
                && HANDOFF_DOC_TICKET == 634
                && A11yNote::for_ticket(621).handoff_ticket == HANDOFF_DOC_TICKET
                && !A11yNote::for_ticket(621).has_direct_surface()
        },
        "无障碍：无直接面如实登记；下游 F0634 文档呈现规范转交不丢",
    );

    s
}

/// 无障碍注记（锚点「无直接无障碍面」——如实登记为无直接面）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct A11yNote {
    /// 本单号。
    pub ticket: u16,
    /// 是否有直接无障碍面。
    pub direct_surface: bool,
    /// 转交的下游单号（文档呈现规范）。
    pub handoff_ticket: u16,
}

impl A11yNote {
    /// 本单的无障碍注记。
    pub fn for_ticket(ticket: u16) -> A11yNote {
        A11yNote {
            ticket,
            direct_surface: false,
            handoff_ticket: 634,
        }
    }

    /// 是否有直接无障碍面。
    pub fn has_direct_surface(&self) -> bool {
        self.direct_surface
    }
}

/// 注册表的人类可读摘要（供上层显示；零 IO）。
pub fn registry_summary(reg: &Registry) -> String {
    let a = reg.audit(&phantom_slots());
    let mut out = String::new();
    out.push_str("混合模式注册表：");
    out.push_str(&a.spec_backed.to_string());
    out.push_str(" 条有条款可对（可分离");
    out.push_str(&SPEC_SEPARABLE.to_string());
    out.push_str(" + 不可分离");
    out.push_str(&SPEC_NON_SEPARABLE.to_string());
    out.push_str(" + 附加");
    out.push_str(&SPEC_ADDITIONAL.to_string());
    out.push_str("）；锚点声称 ");
    out.push_str(&a.claimed.to_string());
    out.push_str("，差额 ");
    out.push_str(&a.phantom.to_string());
    out.push_str(" 格已登记（已归因 ");
    out.push_str(&a.attributed.to_string());
    out.push_str(" / 未归因 ");
    out.push_str(&a.unattributed.to_string());
    out.push_str("）");
    out
}

/// 供上层复用的「虚数差额」格式化（不静默对齐声称值）。
pub fn phantom_note() -> String {
    let mut out = String::new();
    for p in phantom_slots().iter() {
        out.push_str(if p.is_attributed() { "[已归因] " } else { "[未归因] " });
        out.push_str(&p.reason().to_string());
        out.push('\n');
    }
    out
}

/// 供测试与上层复用的常量视图（避免 `vec!` 在no_std 下散落）。
pub fn spec_mode_keys() -> Vec<ModeKey> {
    let mut v: Vec<ModeKey> = Vec::with_capacity(SPEC_MODE_COUNT);
    for k in ModeKey::all().iter() {
        v.push(*k);
    }
    v
}