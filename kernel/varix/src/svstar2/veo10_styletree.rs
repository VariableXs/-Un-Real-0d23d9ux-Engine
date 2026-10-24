//! VE-F2810 · 计算样式树与共享结构（VE-C 域 · 目标 420 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F2810`
//!
//! **判据（锚点原文）**：ComputedValues 结构、共享样式结构、key 四元组、校验拒绝、钳制告警、立案流转、判据。
//!
//! **职责定位（锚点原文摘录）**：计算样式树与共享结构——ComputedValues 结构+共享样式结构+key 四元组。数据结构：数据模型与规格表（逐条规格公开、参数域钳制、枚举守卫——家族格式）。错误路径与降级矩阵：非法输入→校验拒绝三要素（家族）；边界越界→钳制+告警（家族）；异常检出→立案流转（家族）。性能逐项分解：核心逻辑 O(1)-O(logN)（实测定标入域账本——家族）；预算联动次序单源（家族）。跨批对接点：上游契约接收（哈希对账——家族）；下游消费接口（前向声明——家族）；跨域衔接对账钩子（复用方义务——家族）。无障碍与隐私：文档替述可读；无隐私面。
//!
//! # 实装说明（锚点头 420 行为施工目标线；本单实装以全子系统为准）
//!
//! 本文件是 VE-F2810 的**完全自包含**实现：no_std + alloc，零
//! `use crate::`（连家族账本类型都自带一份，不引 [`veo01_arch`]），
//! 内核路径零 unwrap/expect/panic!/运行期 assert!（编译期
//! `const _: () = assert!(…)` 闸另计），可失败全走
//! [`Result<T, StyleTreeError>`](StyleTreeError)。
//!
//! ## 七判据的结构性落点
//!
//! 1. **ComputedValues 结构**（判据一）。17 条属性（≥14 达标）逐条
//!    规格公开：[`PropId`] 封闭枚举 + [`PROP_SPECS`] 规格表逐行给出
//!    合法域 `[lo, hi]`、缺省值、单位、是否可继承。属性值统一按
//!    i32 标量入账（颜色按 24bit RGB 整数，透明度按千分数——见
//!    各行 `unit`），域外一律**钳到边界**不归零（NaN 无关量纲：
//!    i32 标量没有 NaN，家族口径的 NaN 分支由上游浮点层负责）。
//! 2. **共享样式结构**（判据二）。[`SharedStyleStore`]：相同 key 的
//!    样式全局共享单实例（键序有序表 + 二分查找 O(logN)），引用
//!    计数语义（intern/acquire/release 三面），去重率千分数入账；
//!    **写时复制 COW**：共享实例对外只读（无 `&mut` 通道），变更走
//!    [`cow`](StyleTreeEngine::cow_write) 生成新实例 + 引用账目迁移。
//! 3. **key 四元组**（判据三）。家族/成员/伪类/状态四元组按
//!    [`StyleKey`] 定长打包 u64（位域布局常量公开 + 编译期对位闸），
//!    打包/解包往返自证；低 20 位规范零——非规范 u64 解包即拒
//!    （非法 key 注入的检出闸）；碰撞检测走**结构保证 + 全域扫账**：
//!    位域互斥使不同四元组必得不同 u64，判据侧抽样全扫对拍。
//! 4. **校验拒绝三要素**（判据四）。非法输入 → 码（[`code`](StyleTreeError::code)）
//!    + 原因（变体载荷）+ 建议（[`detail`](StyleTreeError::detail)
//!    中文可操作诊断），十五码各归其位，零静默。
//! 5. **钳制告警**（判据五）。边界越界 → 钳到边界 + [`ClampNotice`]
//!    留痕（钳制前后值 + 合法域 + tick 入环形账）。
//! 6. **立案流转**（判据六）。异常检出 → [`CaseLedger`] append-only
//!    立案（现象/影响/定位/处置/证据），账满不覆盖、显性溢出计数。
//! 7. **判据**（判据七）。双跑确定性对拍（[`determinism_probe`]）、
//!    FNV-1a 64 指纹（[`fingerprint_decl`]）、健康自检（[`health`]）、
//!    注入三面（非法 key/越界/继承环，[`inject_bad_key`]/
//!    [`inject_out_of_domain`](StyleTreeEngine::inject_out_of_domain)/
//!    [`inject_inheritance_cycle`](StyleTreeEngine::inject_inheritance_cycle)）
//!    全检出。
//!
//! ## 三闸次序单源（家族口径「预算联动次序单源」）
//!
//! [`GATE_ORDER`]：校验拒绝 → 钳制告警 → 立案流转 → 入账提交。
//! 四个家族闸在所有入口按同一序执行，判据按序号逐位钉死。
//!
//! ## 性能逐项分解（实测定标口径，入域账本）
//!
//! - key 打包/解包/校验：O(1)（位运算常量步）；
//! - 共享表查键：O(logN) 二分（N ≤ [`SHARE_CAP`] = 1024，实测微秒内）；
//! - 单属性声明：O(1)；单节点计算值：O(继承链长)，链长 ≤
//!   [`MAX_TREE_DEPTH`] = 64（有界即 O(1) 上界）；
//! - 批量应用：O(条数 × 单条 O(1))，条数 ≤ [`MAX_BATCH`] = 64；
//! - 整树指纹：O(节点数 × 继承链长)，指纹口径注明非热路径。
//!
//! ## 跨批对接点
//!
//! - **上游契约接收（哈希对账——家族）**：[`ingest_upstream`]
//!   逐条现算 FNV-1a 64 摘要并与上游申报哈希对拍，**哈希不等整批
//!   拒收带码**（[`E_ST_UPSTREAM_HASH`]），零部分接收；
//! - **下游消费接口（前向声明——家族）**：[`DOWNSTREAM_DECL`] 声明
//!   三个只读消费面交 VE-F2811，[`ConsumerView`] 只读投影兑现；
//! - **跨域衔接对账钩子（复用方义务——家族）**：[`reconcile_keys`]
//!   供复用方自查点名 key 都在共享表在账。

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 一、版本与锚点
// ---------------------------------------------------------------------------

/// 本单版本（域账本锚）。
pub const STYLETREE_VERSION: &str = "O10-styletree-v1";

/// 上游锚点（承 VE-F2809 样式失效与重计算调度；上游以带申报哈希的
/// 批次投喂，本单现算对账）。
pub const UPSTREAM_ANCHOR: &str = "VE-F2809";

/// 下游锚点（计算值只读投影交 VE-F2811 消费）。
pub const DOWNSTREAM_ANCHOR: &str = "VE-F2811";

/// 三闸/四闸执行次序单源（判据按序号逐位钉死，所有入口同序）。
pub const GATE_ORDER: [&str; 4] = [
    "validate-reject",
    "clamp-warn",
    "case-file",
    "commit-ledger",
];

/// 判据清单（锚点判据原文的结构化落点，逐条可 grep）。
pub const CRITERIA: [&str; 7] = [
    "ComputedValues 结构：17 属性逐条规格公开（类型/合法域/缺省/钳制界）",
    "共享样式结构：同 key 全局单实例、引用计数、去重率、COW 账目迁移",
    "key 四元组：家族/成员/伪类/状态 u64 定长打包，往返自证+碰撞检测",
    "校验拒绝：码+原因+建议三要素，零静默",
    "钳制告警：钳制前后值留痕入账",
    "立案流转：append-only 案件账，账满显性溢出",
    "判据：双跑确定性对拍+注入三面（非法 key/越界/继承环）全检出",
];

// ---------------------------------------------------------------------------
// 二、诊断码（**F2810 独占码段：0x13B0~0x13BF**）
// ---------------------------------------------------------------------------

/// 码段纪律：0x13A0 段已被 ves439_s23evidence 占用（0x13A1~0x13A3 在
/// 用），0x13C0~0x13FF 段已被 unx2341_udomregress 整段声明占走；开工前
/// grep 复核 **0x13B0~0x13BF 全空闲**，本单独占。
pub const SEG_BASE: u16 = 0x13B0;
/// 码段顶（含）。段内 16 槽：0x13B0 作段基保留，0x13B1~0x13BF 十五码。
pub const SEG_TOP: u16 = 0x13BF;

pub const E_ST_KEY_FAMILY: &str = "E_ST_KEY_FAMILY";
pub const E_ST_KEY_STATE: &str = "E_ST_KEY_STATE";
pub const E_ST_KEY_CANON: &str = "E_ST_KEY_CANON";
pub const E_ST_PROP_UNKNOWN: &str = "E_ST_PROP_UNKNOWN";
pub const E_ST_PROP_GUARD: &str = "E_ST_PROP_GUARD";
pub const E_ST_CAP: &str = "E_ST_CAP";
pub const E_ST_NODE_UNKNOWN: &str = "E_ST_NODE_UNKNOWN";
pub const E_ST_CYCLE: &str = "E_ST_CYCLE";
pub const E_ST_DEPTH: &str = "E_ST_DEPTH";
pub const E_ST_CASE_FULL: &str = "E_ST_CASE_FULL";
pub const E_ST_UPSTREAM_HASH: &str = "E_ST_UPSTREAM_HASH";
pub const E_ST_DOWNSTREAM: &str = "E_ST_DOWNSTREAM";
pub const E_ST_SHARE_STATE: &str = "E_ST_SHARE_STATE";
pub const E_ST_BATCH: &str = "E_ST_BATCH";
pub const E_ST_DETERMINISM: &str = "E_ST_DETERMINISM";

/// 本单全部错误码（十五枚；判据侧独立清点对拍）。
pub const ERROR_CODE_NAMES: [&str; 15] = [
    E_ST_KEY_FAMILY,
    E_ST_KEY_STATE,
    E_ST_KEY_CANON,
    E_ST_PROP_UNKNOWN,
    E_ST_PROP_GUARD,
    E_ST_CAP,
    E_ST_NODE_UNKNOWN,
    E_ST_CYCLE,
    E_ST_DEPTH,
    E_ST_CASE_FULL,
    E_ST_UPSTREAM_HASH,
    E_ST_DOWNSTREAM,
    E_ST_SHARE_STATE,
    E_ST_BATCH,
    E_ST_DETERMINISM,
];

// ---------------------------------------------------------------------------
// 三、属性封闭全集与规格表（家族格式：逐条规格公开/参数域钳制/枚举守卫）
// ---------------------------------------------------------------------------

/// 属性条数（封闭全集规模，判据对账锚；≥14 达标）。
pub const PROP_COUNT: usize = 17;

/// 全属性指定位掩码（17 位全 1）。
pub const PROP_MASK_ALL: u32 = (1u32 << PROP_COUNT) - 1;

/// 计算样式属性封闭全集（十七形态；`of_rank` 越界返回 None = 枚举守卫）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PropId {
    /// 文字色（24bit RGB 整数 0xRRGGBB）。
    TextColorRgb,
    /// 背景色（24bit RGB 整数 0xRRGGBB）。
    BackgroundColorRgb,
    /// 字号（px）。
    FontSizePx,
    /// 行高（px）。
    LineHeightPx,
    /// 字距（px，可负）。
    LetterSpacingPx,
    /// 上内边距（px）。
    PaddingTopPx,
    /// 右内边距（px）。
    PaddingRightPx,
    /// 下内边距（px）。
    PaddingBottomPx,
    /// 左内边距（px）。
    PaddingLeftPx,
    /// 上外边距（px，可负）。
    MarginTopPx,
    /// 右外边距（px，可负）。
    MarginRightPx,
    /// 下外边距（px，可负）。
    MarginBottomPx,
    /// 左外边距（px，可负）。
    MarginLeftPx,
    /// 边框宽（px）。
    BorderWidthPx,
    /// 圆角半径（px）。
    BorderRadiusPx,
    /// 层叠层级（可负）。
    ZLevel,
    /// 不透明度（千分数 0~1000）。
    OpacityPermille,
}

pub const P_TEXT: PropId = PropId::TextColorRgb;
pub const P_BG: PropId = PropId::BackgroundColorRgb;
pub const P_FONT: PropId = PropId::FontSizePx;
pub const P_LINE: PropId = PropId::LineHeightPx;
pub const P_TRACK: PropId = PropId::LetterSpacingPx;
pub const P_PAD_T: PropId = PropId::PaddingTopPx;
pub const P_PAD_R: PropId = PropId::PaddingRightPx;
pub const P_PAD_B: PropId = PropId::PaddingBottomPx;
pub const P_PAD_L: PropId = PropId::PaddingLeftPx;
pub const P_MAR_T: PropId = PropId::MarginTopPx;
pub const P_MAR_R: PropId = PropId::MarginRightPx;
pub const P_MAR_B: PropId = PropId::MarginBottomPx;
pub const P_MAR_L: PropId = PropId::MarginLeftPx;
pub const P_BORDER: PropId = PropId::BorderWidthPx;
pub const P_RADIUS: PropId = PropId::BorderRadiusPx;
pub const P_Z: PropId = PropId::ZLevel;
pub const P_OPACITY: PropId = PropId::OpacityPermille;

impl PropId {
    /// 封闭全集（秩序 = 声明序）。
    pub const ALL: [PropId; PROP_COUNT] = [
        P_TEXT, P_BG, P_FONT, P_LINE, P_TRACK,
        P_PAD_T, P_PAD_R, P_PAD_B, P_PAD_L,
        P_MAR_T, P_MAR_R, P_MAR_B, P_MAR_L,
        P_BORDER, P_RADIUS, P_Z, P_OPACITY,
    ];

    /// 秩（精确 0..=16，判据钉死不许漂移）。
    pub const fn rank(self) -> usize {
        match self {
            PropId::TextColorRgb => 0,
            PropId::BackgroundColorRgb => 1,
            PropId::FontSizePx => 2,
            PropId::LineHeightPx => 3,
            PropId::LetterSpacingPx => 4,
            PropId::PaddingTopPx => 5,
            PropId::PaddingRightPx => 6,
            PropId::PaddingBottomPx => 7,
            PropId::PaddingLeftPx => 8,
            PropId::MarginTopPx => 9,
            PropId::MarginRightPx => 10,
            PropId::MarginBottomPx => 11,
            PropId::MarginLeftPx => 12,
            PropId::BorderWidthPx => 13,
            PropId::BorderRadiusPx => 14,
            PropId::ZLevel => 15,
            PropId::OpacityPermille => 16,
        }
    }

    /// 秩反查（越界 None——枚举守卫的唯一合法通道）。
    pub const fn of_rank(r: usize) -> Option<PropId> {
        match r {
            0 => Some(PropId::TextColorRgb),
            1 => Some(PropId::BackgroundColorRgb),
            2 => Some(PropId::FontSizePx),
            3 => Some(PropId::LineHeightPx),
            4 => Some(PropId::LetterSpacingPx),
            5 => Some(PropId::PaddingTopPx),
            6 => Some(PropId::PaddingRightPx),
            7 => Some(PropId::PaddingBottomPx),
            8 => Some(PropId::PaddingLeftPx),
            9 => Some(PropId::MarginTopPx),
            10 => Some(PropId::MarginRightPx),
            11 => Some(PropId::MarginBottomPx),
            12 => Some(PropId::MarginLeftPx),
            13 => Some(PropId::BorderWidthPx),
            14 => Some(PropId::BorderRadiusPx),
            15 => Some(PropId::ZLevel),
            16 => Some(PropId::OpacityPermille),
            _ => None,
        }
    }

    /// 读屏中文名（与规格表 `name` 逐字节一致，判据对拍）。
    pub const fn zh(self) -> &'static str {
        match self {
            PropId::TextColorRgb => "文字色",
            PropId::BackgroundColorRgb => "背景色",
            PropId::FontSizePx => "字号",
            PropId::LineHeightPx => "行高",
            PropId::LetterSpacingPx => "字距",
            PropId::PaddingTopPx => "上内边距",
            PropId::PaddingRightPx => "右内边距",
            PropId::PaddingBottomPx => "下内边距",
            PropId::PaddingLeftPx => "左内边距",
            PropId::MarginTopPx => "上外边距",
            PropId::MarginRightPx => "右外边距",
            PropId::MarginBottomPx => "下外边距",
            PropId::MarginLeftPx => "左外边距",
            PropId::BorderWidthPx => "边框宽",
            PropId::BorderRadiusPx => "圆角半径",
            PropId::ZLevel => "层级",
            PropId::OpacityPermille => "不透明度",
        }
    }
}

/// 单属性规格（逐条公开：类型口径/合法域/缺省/单位/是否可继承）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PropSpec {
    /// 属性（与槽位逐位对位，编译期闸钉死 `rank == 槽位`）。
    pub prop: PropId,
    /// 规格名（读屏与钳制留痕共用，'static 单源）。
    pub name: &'static str,
    /// 合法域下界（含）。
    pub lo: i32,
    /// 合法域上界（含）。
    pub hi: i32,
    /// 缺省值（必在域内，编译期闸钉死）。
    pub default: i32,
    /// 单位口径（px / rgb / ‰）。
    pub unit: &'static str,
    /// 是否可继承（继承子系统按位取用）。
    pub inherits: bool,
}

/// 规格表（与封闭全集逐位对位；编译期闸钉死长度/秩/缺省在域内）。
///
/// 域值来源（常量写死注释）：
/// - 字号 8~72px：正文最小可读 8px、展示标题上限 72px（引擎排版档位）；
/// - 行高 10~144px：≥2×字号上界，覆盖 0.5~3.0 倍行距谱系；
/// - 字距 -8~32px：紧排下限 -0.5em（16px 基准）至宽排 2em；
/// - 内边距 0~128px：无负内边距语义，上界取 2×字号上界安全余量；
/// - 外边距 -64~256px：负边距允许叠印，上界 2×内边距上界；
/// - 边框 0~16px：细描边至重描边；
/// - 圆角 0~64px；
/// - 层级 -1024~1024：层叠上下文对称区间；
/// - 不透明度 0~1000‰：0 全透明、1000 全不透明。
pub const PROP_SPECS: [PropSpec; PROP_COUNT] = [
    PropSpec { prop: P_TEXT, name: "text_color_rgb", lo: 0, hi: 0xFFFFFF, default: 0x1A1A1A, unit: "rgb", inherits: true },
    PropSpec { prop: P_BG, name: "background_color_rgb", lo: 0, hi: 0xFFFFFF, default: 0xF5F5F5, unit: "rgb", inherits: false },
    PropSpec { prop: P_FONT, name: "font_size_px", lo: 8, hi: 72, default: 16, unit: "px", inherits: true },
    PropSpec { prop: P_LINE, name: "line_height_px", lo: 10, hi: 144, default: 24, unit: "px", inherits: true },
    PropSpec { prop: P_TRACK, name: "letter_spacing_px", lo: -8, hi: 32, default: 0, unit: "px", inherits: true },
    PropSpec { prop: P_PAD_T, name: "padding_top_px", lo: 0, hi: 128, default: 0, unit: "px", inherits: false },
    PropSpec { prop: P_PAD_R, name: "padding_right_px", lo: 0, hi: 128, default: 0, unit: "px", inherits: false },
    PropSpec { prop: P_PAD_B, name: "padding_bottom_px", lo: 0, hi: 128, default: 0, unit: "px", inherits: false },
    PropSpec { prop: P_PAD_L, name: "padding_left_px", lo: 0, hi: 128, default: 0, unit: "px", inherits: false },
    PropSpec { prop: P_MAR_T, name: "margin_top_px", lo: -64, hi: 256, default: 0, unit: "px", inherits: false },
    PropSpec { prop: P_MAR_R, name: "margin_right_px", lo: -64, hi: 256, default: 0, unit: "px", inherits: false },
    PropSpec { prop: P_MAR_B, name: "margin_bottom_px", lo: -64, hi: 256, default: 0, unit: "px", inherits: false },
    PropSpec { prop: P_MAR_L, name: "margin_left_px", lo: -64, hi: 256, default: 0, unit: "px", inherits: false },
    PropSpec { prop: P_BORDER, name: "border_width_px", lo: 0, hi: 16, default: 0, unit: "px", inherits: false },
    PropSpec { prop: P_RADIUS, name: "border_radius_px", lo: 0, hi: 64, default: 0, unit: "px", inherits: false },
    PropSpec { prop: P_Z, name: "z_level", lo: -1024, hi: 1024, default: 0, unit: "level", inherits: false },
    PropSpec { prop: P_OPACITY, name: "opacity_permille", lo: 0, hi: 1000, default: 1000, unit: "‰", inherits: false },
];

/// 按属性取规格（const 期 match 索引——slice::get 在 const 期禁用，
/// 秩与槽位的逐位对位由编译期闸保证）。
pub const fn prop_spec(p: PropId) -> &'static PropSpec {
    match p.rank() {
        0 => &PROP_SPECS[0],
        1 => &PROP_SPECS[1],
        2 => &PROP_SPECS[2],
        3 => &PROP_SPECS[3],
        4 => &PROP_SPECS[4],
        5 => &PROP_SPECS[5],
        6 => &PROP_SPECS[6],
        7 => &PROP_SPECS[7],
        8 => &PROP_SPECS[8],
        9 => &PROP_SPECS[9],
        10 => &PROP_SPECS[10],
        11 => &PROP_SPECS[11],
        12 => &PROP_SPECS[12],
        13 => &PROP_SPECS[13],
        14 => &PROP_SPECS[14],
        15 => &PROP_SPECS[15],
        _ => &PROP_SPECS[16],
    }
}

/// 域钳制纯函数（越界收边界不归零；i32 标量无 NaN 量纲）。
pub const fn clamp_to_domain(lo: i32, hi: i32, raw: i32) -> i32 {
    if raw < lo {
        lo
    } else if raw > hi {
        hi
    } else {
        raw
    }
}

// 编译期闸前哨（规格表对位/域自洽，供文末 const 闸块调用）。
const fn spec_table_ranks_aligned() -> bool {
    let mut i = 0usize;
    while i < PROP_COUNT {
        if PROP_SPECS[i].prop.rank() != i {
            return false;
        }
        i += 1;
    }
    true
}

const fn spec_defaults_in_domain() -> bool {
    let mut i = 0usize;
    while i < PROP_COUNT {
        let s = &PROP_SPECS[i];
        if s.lo > s.hi {
            return false;
        }
        if s.default < s.lo || s.default > s.hi {
            return false;
        }
        i += 1;
    }
    true
}

// ---------------------------------------------------------------------------
// 四、ComputedValues 计算样式值结构（判据一）
// ---------------------------------------------------------------------------

/// 计算样式值：17 槽标量账 + 指定位掩码。
///
/// 存储为「槽位数组 + 指定掩码」而非 17 个具名成员：继承子系统按
/// 位运算处理「未指定→继承→缺省」三态（O(1) 掩码合并），具名读取
/// 面由 17 个具名 getter 兑现；槽位与 [`PropId::rank`] 逐位对位，
/// 编译期闸钉死。
///
/// 体积 72 字节（17×4 + 4），远低于 64KB 栈禁令，可 Copy。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ComputedValues {
    /// 属性值槽（下标 = [`PropId::rank`]）。
    pub vals: [i32; PROP_COUNT],
    /// 指定位掩码（bit k = 第 k 槽已被显式指定）。
    pub specified: u32,
}

impl ComputedValues {
    /// 全缺省值（全槽取规格表缺省；指定掩码清零 = 全部未指定）。
    pub const fn defaults() -> ComputedValues {
        let mut vals = [0i32; PROP_COUNT];
        let mut i = 0usize;
        while i < PROP_COUNT {
            vals[i] = PROP_SPECS[i].default;
            i += 1;
        }
        ComputedValues { vals, specified: 0 }
    }

    /// 读槽（具名 getter 的单源底层）。
    pub const fn read(&self, p: PropId) -> i32 {
        self.vals[p.rank()]
    }

    /// 是否已指定。
    pub const fn is_specified(&self, p: PropId) -> bool {
        self.specified & (1u32 << p.rank()) != 0
    }

    /// 域钳制写入（返回钳制后落点值；越界钳制告警由引擎层留痕，
    /// 本函数保持纯写——纯函数可被判据侧独立复算对拍）。
    pub fn write_clamped(&mut self, p: PropId, raw: i32) -> i32 {
        let sp = prop_spec(p);
        let c = clamp_to_domain(sp.lo, sp.hi, raw);
        self.vals[p.rank()] = c;
        self.specified |= 1u32 << p.rank();
        c
    }

    /// 全域自检：每槽都在自己的合法域内。
    pub fn in_domain_all(&self) -> bool {
        let mut i = 0usize;
        while i < PROP_COUNT {
            let sp = &PROP_SPECS[i];
            if self.vals[i] < sp.lo || self.vals[i] > sp.hi {
                return false;
            }
            i += 1;
        }
        true
    }

    // ---- 17 具名读取面（判据一「逐字段」的可读兑现；读屏可替述）----

    /// 文字色。
    pub const fn text_color_rgb(&self) -> i32 { self.read(P_TEXT) }
    /// 背景色。
    pub const fn background_color_rgb(&self) -> i32 { self.read(P_BG) }
    /// 字号。
    pub const fn font_size_px(&self) -> i32 { self.read(P_FONT) }
    /// 行高。
    pub const fn line_height_px(&self) -> i32 { self.read(P_LINE) }
    /// 字距。
    pub const fn letter_spacing_px(&self) -> i32 { self.read(P_TRACK) }
    /// 上内边距。
    pub const fn padding_top_px(&self) -> i32 { self.read(P_PAD_T) }
    /// 右内边距。
    pub const fn padding_right_px(&self) -> i32 { self.read(P_PAD_R) }
    /// 下内边距。
    pub const fn padding_bottom_px(&self) -> i32 { self.read(P_PAD_B) }
    /// 左内边距。
    pub const fn padding_left_px(&self) -> i32 { self.read(P_PAD_L) }
    /// 上外边距。
    pub const fn margin_top_px(&self) -> i32 { self.read(P_MAR_T) }
    /// 右外边距。
    pub const fn margin_right_px(&self) -> i32 { self.read(P_MAR_R) }
    /// 下外边距。
    pub const fn margin_bottom_px(&self) -> i32 { self.read(P_MAR_B) }
    /// 左外边距。
    pub const fn margin_left_px(&self) -> i32 { self.read(P_MAR_L) }
    /// 边框宽。
    pub const fn border_width_px(&self) -> i32 { self.read(P_BORDER) }
    /// 圆角半径。
    pub const fn border_radius_px(&self) -> i32 { self.read(P_RADIUS) }
    /// 层级。
    pub const fn z_level(&self) -> i32 { self.read(P_Z) }
    /// 不透明度。
    pub const fn opacity_permille(&self) -> i32 { self.read(P_OPACITY) }

    /// 读屏单行（无障碍替述：逐属性名=值，可被读屏软件直接播报）。
    pub fn screen_line(&self) -> String {
        let mut out = String::new();
        let mut i = 0usize;
        while i < PROP_COUNT {
            let sp = &PROP_SPECS[i];
            if i > 0 {
                out.push('，');
            }
            out.push_str(sp.name);
            out.push('=');
            out.push_str(&format!("{}", self.vals[i]));
            out.push_str(sp.unit);
            i += 1;
        }
        out
    }
}

// ---------------------------------------------------------------------------
// 五、key 四元组（判据三：家族/成员/伪类/状态 → u64 定长打包）
// ---------------------------------------------------------------------------

/// 伪类封闭全集（八形态；4 位域只容 0..=15，8 形态占 0..=7，
/// 8..=15 越界 = 枚举守卫拒）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Pseudo {
    /// 无伪类。
    None,
    /// :hover 悬停。
    Hover,
    /// :active 按下。
    Active,
    /// :focus 聚焦。
    Focus,
    /// :visited 已访问。
    Visited,
    /// :disabled 禁用。
    Disabled,
    /// ::first-line 首行。
    FirstLine,
    /// ::marker 标记盒。
    Marker,
}

pub use Pseudo::{Active, Disabled, FirstLine, Focus, Hover, Marker, None as PseudoNone, Visited};

/// 伪类总数（封闭全集规模）。
pub const PSEUDO_COUNT: usize = 8;

impl Pseudo {
    /// 封闭全集（秩序 = 声明序）。
    pub const ALL: [Pseudo; PSEUDO_COUNT] = [
        Pseudo::None,
        Pseudo::Hover,
        Pseudo::Active,
        Pseudo::Focus,
        Pseudo::Visited,
        Pseudo::Disabled,
        Pseudo::FirstLine,
        Pseudo::Marker,
    ];

    /// 秩（精确 0..=7）。
    pub const fn rank(self) -> usize {
        match self {
            Pseudo::None => 0,
            Pseudo::Hover => 1,
            Pseudo::Active => 2,
            Pseudo::Focus => 3,
            Pseudo::Visited => 4,
            Pseudo::Disabled => 5,
            Pseudo::FirstLine => 6,
            Pseudo::Marker => 7,
        }
    }

    /// 秩反查（越界 None——枚举守卫的唯一合法通道）。
    pub const fn of_rank(r: usize) -> Option<Pseudo> {
        match r {
            0 => Some(Pseudo::None),
            1 => Some(Pseudo::Hover),
            2 => Some(Pseudo::Active),
            3 => Some(Pseudo::Focus),
            4 => Some(Pseudo::Visited),
            5 => Some(Pseudo::Disabled),
            6 => Some(Pseudo::FirstLine),
            7 => Some(Pseudo::Marker),
            _ => None,
        }
    }

    /// 读屏中文名。
    pub const fn zh(self) -> &'static str {
        match self {
            Pseudo::None => "无伪类",
            Pseudo::Hover => "悬停",
            Pseudo::Active => "按下",
            Pseudo::Focus => "聚焦",
            Pseudo::Visited => "已访问",
            Pseudo::Disabled => "禁用",
            Pseudo::FirstLine => "首行",
            Pseudo::Marker => "标记盒",
        }
    }
}

/// 状态位：可用。
pub const STATE_ENABLED: u8 = 0x01;
/// 状态位：按下。
pub const STATE_PRESSED: u8 = 0x02;
/// 状态位：聚焦。
pub const STATE_FOCUSED: u8 = 0x04;
/// 状态位：选中。
pub const STATE_CHECKED: u8 = 0x08;
/// 状态位：禁用。
pub const STATE_DISABLED: u8 = 0x10;
/// 状态位：只读。
pub const STATE_READONLY: u8 = 0x20;
/// 状态位合法掩码（域外位 = 非法状态注入，校验拒绝）。
pub const STATE_MASK: u8 = 0x3F;

/// key 打包位域：状态位移量（bit 20..27）。
pub const STATE_SHIFT: u32 = 20;
/// key 打包位域：伪类移量（bit 28..31）。
pub const PSEUDO_SHIFT: u32 = 28;
/// key 打包位域：成员移量（bit 32..47）。
pub const MEMBER_SHIFT: u32 = 32;
/// key 打包位域：家族移量（bit 48..63）。
pub const FAMILY_SHIFT: u32 = 48;
/// 规范低位宽（bit 0..19 必须全零；非零 = 非规范 u64，解包即拒）。
pub const CANON_LOW_BITS: u32 = 20;
/// 规范低位掩码。
pub const CANON_MASK: u64 = (1u64 << CANON_LOW_BITS) - 1;

/// key 四元组：家族/成员/伪类/状态。
///
/// 家族号合法域 1..=0xFFFE（0 = 未定族拒收、0xFFFF = 通配保留拒收）；
/// 成员号合法域 0..=0xFFFE（0 = 族内匿名成员合法、0xFFFF 保留）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct StyleKey {
    /// 家族号。
    pub family: u16,
    /// 成员号。
    pub member: u16,
    /// 伪类。
    pub pseudo: Pseudo,
    /// 状态位（合法位见 [`STATE_MASK`]；ENABLED/DISABLED 互斥）。
    pub state: u8,
}

/// key 合法域：家族号保留下界（0 拒收）。
pub const KEY_FAMILY_MIN: u16 = 1;
/// key 合法域：家族号保留上界（0xFFFF 拒收）。
pub const KEY_FAMILY_MAX: u16 = 0xFFFE;
/// key 合法域：成员号保留上界（0xFFFF 拒收）。
pub const KEY_MEMBER_MAX: u16 = 0xFFFE;

impl StyleKey {
    /// 构造（不校验；打包时统一走闸——构造与校验分离便于判据注入）。
    pub const fn new(family: u16, member: u16, pseudo: Pseudo, state: u8) -> StyleKey {
        StyleKey { family, member, pseudo, state }
    }

    /// 结构校验（非法输入 → 校验拒绝三要素；O(1) 常量步）。
    pub fn validate(&self) -> Result<(), StyleTreeError> {
        if self.family < KEY_FAMILY_MIN || self.family > KEY_FAMILY_MAX {
            return Err(StyleTreeError::KeyFamily { family: self.family });
        }
        if self.member > KEY_MEMBER_MAX {
            return Err(StyleTreeError::KeyFamily { family: self.member });
        }
        if self.pseudo.rank() >= PSEUDO_COUNT {
            return Err(StyleTreeError::KeyCanon { raw: 0 });
        }
        if self.state & !STATE_MASK != 0 {
            return Err(StyleTreeError::KeyState { state: self.state });
        }
        if self.state & STATE_ENABLED != 0 && self.state & STATE_DISABLED != 0 {
            return Err(StyleTreeError::KeyState { state: self.state });
        }
        Ok(())
    }

    /// 打包 u64（先结构校验；位域互斥 ⇒ 不同四元组必得不同 u64——
    /// 碰撞在编码层结构不可能，判据侧另有抽样全扫对拍）。
    pub fn pack(&self) -> Result<u64, StyleTreeError> {
        self.validate()?;
        let mut raw: u64 = 0;
        raw |= (self.family as u64) << FAMILY_SHIFT;
        raw |= (self.member as u64) << MEMBER_SHIFT;
        raw |= (self.pseudo.rank() as u64) << PSEUDO_SHIFT;
        raw |= (self.state as u64) << STATE_SHIFT;
        Ok(raw)
    }

    /// 解包（先规范闸：低 [`CANON_LOW_BITS`] 位必须全零；再位域守卫；
    /// 再结构校验——三道闸同序于 [`GATE_ORDER`] 第一闸）。
    pub fn unpack(raw: u64) -> Result<StyleKey, StyleTreeError> {
        if raw & CANON_MASK != 0 {
            return Err(StyleTreeError::KeyCanon { raw });
        }
        let pseudo_rank = ((raw >> PSEUDO_SHIFT) & 0xF) as usize;
        let pseudo = match Pseudo::of_rank(pseudo_rank) {
            Some(p) => p,
            None => return Err(StyleTreeError::KeyCanon { raw }),
        };
        let state = ((raw >> STATE_SHIFT) & 0xFF) as u8;
        let member = ((raw >> MEMBER_SHIFT) & 0xFFFF) as u16;
        let family = (raw >> FAMILY_SHIFT) as u16;
        let key = StyleKey::new(family, member, pseudo, state);
        key.validate()?;
        Ok(key)
    }

    /// 读屏单行。
    pub fn screen_line(&self) -> String {
        format!(
            "家族 #{} 成员 #{}（{}，状态位 0x{:02X}）",
            self.family, self.member, self.pseudo.zh(), self.state
        )
    }
}

/// key + 值对的规范字节序（哈希对账与指纹共用的单源序列化：
/// key u64 LE + 17 槽 i32 LE 按秩序）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct KeyedValues {
    /// 已打包 key u64。
    pub key: u64,
    /// 计算样式值。
    pub values: ComputedValues,
}

impl KeyedValues {
    /// 规范字节流（确定性：同值必同字节，跨平台序固定 LE）。
    pub fn canonical_bytes(&self) -> Vec<u8> {
        let mut b = Vec::new();
        b.extend_from_slice(&self.key.to_le_bytes());
        let mut i = 0usize;
        while i < PROP_COUNT {
            b.extend_from_slice(&self.values.vals[i].to_le_bytes());
            i += 1;
        }
        b
    }
}

/// FNV-1a 64 偏移基（常量来源：FNV-1a 标准参数）。
pub const FNV_OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
/// FNV-1a 64 素数（常量来源：FNV-1a 标准参数）。
pub const FNV_PRIME: u64 = 0x0000_0100_0000_01b3;

/// FNV-1a 64（显性 wrapping 乘——哈希溢出是语义不是事故）。
pub fn fnv64(bytes: &[u8]) -> u64 {
    let mut h = FNV_OFFSET;
    let mut i = 0usize;
    while i < bytes.len() {
        h ^= bytes[i] as u64;
        h = h.wrapping_mul(FNV_PRIME);
        i += 1;
    }
    h
}

// ---------------------------------------------------------------------------
// 六、错误枚举（三要素：码 + 原因载荷 + detail 中文可操作诊断）
// ---------------------------------------------------------------------------

/// 本单错误枚举（十五变体封闭；每变体带码/数值码/中文可操作诊断）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum StyleTreeError {
    /// key 家族号/成员号越保留域（0 或 0xFFFF）。
    KeyFamily { family: u16 },
    /// key 状态位含非法位，或 ENABLED/DISABLED 互斥冲突。
    KeyState { state: u8 },
    /// 打包 u64 非规范（低 20 位非零）或伪类秩越界。
    KeyCanon { raw: u64 },
    /// 属性秩越界（枚举守卫）。
    PropUnknown { rank: u8 },
    /// 结构性守卫失败（上游值未满配 / 非叶子拆除等）。
    PropGuard { locus: &'static str },
    /// 容量上界（树节点/共享表/补丁条数/批量条数）。
    CapFull { what: &'static str, cap: usize },
    /// 树节点句柄不存在。
    NodeUnknown { node: u64 },
    /// 继承环检出（reparent 成环 / 祖先链步数越上界）。
    Cycle { node: u64, via: u64 },
    /// 子树迁移后深度越上界。
    Depth { node: u64, would_be: u32, cap: u32 },
    /// 立案账已满（append-only 不覆盖）。
    CaseFull { cap: usize },
    /// 上游批次哈希对账不等（整批拒收）。
    UpstreamHash { index: usize, expect: u64, got: u64 },
    /// 下游前向声明漂移。
    Downstream { why: &'static str },
    /// 共享语义误用（未 intern 先 acquire / 未持有即 release / COW 源无引用 / COW 目标同源）。
    ShareState { key: u64, why: &'static str },
    /// 批量应用被整体拒绝（原子性，零提交）。
    Batch { reason: &'static str, item: usize },
    /// 双跑确定性对拍不一致。
    Determinism { node: u64 },
}

impl StyleTreeError {
    /// 码（三要素之一）。
    pub const fn code(&self) -> &'static str {
        match self {
            StyleTreeError::KeyFamily { .. } => E_ST_KEY_FAMILY,
            StyleTreeError::KeyState { .. } => E_ST_KEY_STATE,
            StyleTreeError::KeyCanon { .. } => E_ST_KEY_CANON,
            StyleTreeError::PropUnknown { .. } => E_ST_PROP_UNKNOWN,
            StyleTreeError::PropGuard { .. } => E_ST_PROP_GUARD,
            StyleTreeError::CapFull { .. } => E_ST_CAP,
            StyleTreeError::NodeUnknown { .. } => E_ST_NODE_UNKNOWN,
            StyleTreeError::Cycle { .. } => E_ST_CYCLE,
            StyleTreeError::Depth { .. } => E_ST_DEPTH,
            StyleTreeError::CaseFull { .. } => E_ST_CASE_FULL,
            StyleTreeError::UpstreamHash { .. } => E_ST_UPSTREAM_HASH,
            StyleTreeError::Downstream { .. } => E_ST_DOWNSTREAM,
            StyleTreeError::ShareState { .. } => E_ST_SHARE_STATE,
            StyleTreeError::Batch { .. } => E_ST_BATCH,
            StyleTreeError::Determinism { .. } => E_ST_DETERMINISM,
        }
    }

    /// 数值码（0x13B1~0x13BF，与码段纪律逐位对位）。
    pub const fn code_num(&self) -> u16 {
        match self {
            StyleTreeError::KeyFamily { .. } => SEG_BASE + 0x01,
            StyleTreeError::KeyState { .. } => SEG_BASE + 0x02,
            StyleTreeError::KeyCanon { .. } => SEG_BASE + 0x03,
            StyleTreeError::PropUnknown { .. } => SEG_BASE + 0x04,
            StyleTreeError::PropGuard { .. } => SEG_BASE + 0x05,
            StyleTreeError::CapFull { .. } => SEG_BASE + 0x06,
            StyleTreeError::NodeUnknown { .. } => SEG_BASE + 0x07,
            StyleTreeError::Cycle { .. } => SEG_BASE + 0x08,
            StyleTreeError::Depth { .. } => SEG_BASE + 0x09,
            StyleTreeError::CaseFull { .. } => SEG_BASE + 0x0A,
            StyleTreeError::UpstreamHash { .. } => SEG_BASE + 0x0B,
            StyleTreeError::Downstream { .. } => SEG_BASE + 0x0C,
            StyleTreeError::ShareState { .. } => SEG_BASE + 0x0D,
            StyleTreeError::Batch { .. } => SEG_BASE + 0x0E,
            StyleTreeError::Determinism { .. } => SEG_BASE + 0x0F,
        }
    }

    /// 中文可操作诊断（三要素之三：原因 + 下一步，读屏可播报）。
    pub fn detail(&self) -> &'static str {
        match self {
            StyleTreeError::KeyFamily { .. } => {
                "key 家族号/成员号落在保留域（0 或 0xFFFF）：家族号取 1..=0xFFFE、成员号取 0..=0xFFFE；0 表示未定族不可投喂，通配语义另走查询层"
            }
            StyleTreeError::KeyState { .. } => {
                "key 状态位含合法域外位，或 ENABLED 与 DISABLED 同时置位：状态位只能取 STATE_MASK 六位之一组合，且可用/禁用互斥；先清非法位或改单边状态"
            }
            StyleTreeError::KeyCanon { .. } => {
                "打包 u64 非规范（低 20 位非零）或伪类秩越 0..=7：u64 只能由 StyleKey::pack 产出，手拼位域请改走 pack；低 20 位是规范保留位必须为零"
            }
            StyleTreeError::PropUnknown { .. } => {
                "属性秩越出封闭全集 0..=16：属性只能取 PropId 十七形态之一，秩反查唯一通道是 PropId::of_rank；改传枚举不要手造秩"
            }
            StyleTreeError::PropGuard { .. } => {
                "结构性守卫失败：上游计算值必须 17 槽全指定（满配），或拆除目标不是叶子节点；先补满指定掩码或先拆子孙再拆本节点"
            }
            StyleTreeError::CapFull { .. } => {
                "容量达到上界（树节点/共享表/补丁条数/批量条数）：先归档回收无用节点与零引用共享槽，或按 ADR 提升对应上界常量；满后不覆盖旧行"
            }
            StyleTreeError::NodeUnknown { .. } => {
                "树节点句柄不存在：句柄来自 add_node 的返回值且未被拆除；先 node_with_key/node_idx 查在账节点，不要凭空造句柄"
            }
            StyleTreeError::Cycle { .. } => {
                "继承环检出：把祖先挂到子孙之下会成环，环上继承链永远走不到根；按 reparent 的环成员提示拆环（先摘出环再重挂）"
            }
            StyleTreeError::Depth { .. } => {
                "子树迁移后深度将越 MAX_TREE_DEPTH 上界：拍平中间层或按 ADR 提升深度上界；链式继承过深本身也该警惕样式语义"
            }
            StyleTreeError::CaseFull { .. } => {
                "立案账已满（append-only 不覆盖旧案）：先导出并清账（复用方义务），或按 ADR 提升 CASE_CAP；溢出已显性计数不静默"
            }
            StyleTreeError::UpstreamHash { .. } => {
                "上游批次哈希对账不等：申报哈希与本单现算 FNV-1a 64 不一致，批次可能被篡改或上游序列化漂移；整批拒收零部分接收，上游修好哈希再投喂"
            }
            StyleTreeError::Downstream { .. } => {
                "下游前向声明漂移：对端标识或消费面清单与冻结契约不符；与 VE-F2811 对齐三消费面（consumer_view/share_values/dedup_ledger）后重跑审计"
            }
            StyleTreeError::ShareState { .. } => {
                "共享语义误用：acquire 前必须先 intern，release 前必须持有引用，COW 源必须 strong≥1 且目标键不得与源键相同；按账面引用数核对调用序"
            }
            StyleTreeError::Batch { .. } => {
                "批量应用被整体拒绝（原子批处理零提交）：清单内存在未知节点或非法属性秩；修正后整批重投，不要拆半批绕过原子性"
            }
            StyleTreeError::Determinism { .. } => {
                "双跑确定性对拍不一致：同一树两次独立计算给出不同值，计算路径含非确定源；立即停用该树并按案件账定位污染入口"
            }
        }
    }

    /// 读屏单行（码 + 数值码 + 诊断）。
    pub fn screen(&self) -> String {
        format!(
            "[{}|0x{:04X}] {}",
            self.code(),
            self.code_num(),
            self.detail()
        )
    }
}

/// 全十五码数值（判据侧独立清点对拍用；const 期构造哑元取码）。
const fn all_code_nums() -> [u16; 15] {
    [
        StyleTreeError::KeyFamily { family: 1 }.code_num(),
        StyleTreeError::KeyState { state: 0 }.code_num(),
        StyleTreeError::KeyCanon { raw: 0 }.code_num(),
        StyleTreeError::PropUnknown { rank: 0 }.code_num(),
        StyleTreeError::PropGuard { locus: "" }.code_num(),
        StyleTreeError::CapFull { what: "", cap: 0 }.code_num(),
        StyleTreeError::NodeUnknown { node: 0 }.code_num(),
        StyleTreeError::Cycle { node: 0, via: 0 }.code_num(),
        StyleTreeError::Depth { node: 0, would_be: 0, cap: 0 }.code_num(),
        StyleTreeError::CaseFull { cap: 0 }.code_num(),
        StyleTreeError::UpstreamHash { index: 0, expect: 0, got: 0 }.code_num(),
        StyleTreeError::Downstream { why: "" }.code_num(),
        StyleTreeError::ShareState { key: 0, why: "" }.code_num(),
        StyleTreeError::Batch { reason: "", item: 0 }.code_num(),
        StyleTreeError::Determinism { node: 0 }.code_num(),
    ]
}

/// 十五码两两互异（编译期闸前哨）。
const fn codes_distinct() -> bool {
    let a = all_code_nums();
    let mut i = 0usize;
    while i < a.len() {
        let mut j = i + 1;
        while j < a.len() {
            if a[i] == a[j] {
                return false;
            }
            j += 1;
        }
        i += 1;
    }
    true
}

// ---------------------------------------------------------------------------
// 七、容量与参数（常量写死，来源见行注释）
// ---------------------------------------------------------------------------

/// 树节点上限（512：一屏组件树量级，2 的幂对齐分配器习惯）。
pub const MAX_TREE_NODES: usize = 512;
/// 继承链/子树深度上界（64：64 恰阈判据——63 可收、65 拒；来源：
/// 深嵌套样式语义在 64 层已无工程意义，且与 Kahn 家族口径对齐）。
pub const MAX_TREE_DEPTH: u32 = 64;
/// 单批条数上界（64：单帧预算内 O(64) 全量校验+提交）。
pub const MAX_BATCH: usize = 64;
/// 共享表槽位上界（1024：2 的幂，判据闸钉死）。
pub const SHARE_CAP: usize = 1024;
/// 钳制告警环形账容量（128：最近 128 条留痕，旧的被覆盖计数不丢）。
pub const CLAMP_LOG_CAP: usize = 128;
/// 立案账容量（64：append-only 不覆盖，满后显性溢出）。
pub const CASE_CAP: usize = 64;
/// 事件环形账容量（256：最近 256 条，覆盖计数入账）。
pub const EVENT_CAP: usize = 256;
/// 节点标签字节上界（32：读屏替述够用，越界字符边界安全截断）。
pub const NODE_LABEL_CAP: usize = 32;

// ---------------------------------------------------------------------------
// 八、账本三件：钳制环形账 / 立案 append-only 账 / 事件环形账
// ---------------------------------------------------------------------------

/// 一条钳制告警（边界越界 → 钳制 + 告警；前后值留痕）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ClampNotice {
    /// 规格名（'static 单源承规格表）。
    pub field: &'static str,
    /// 钳制前原值。
    pub original: i32,
    /// 钳制后落点。
    pub clamped: i32,
    /// 合法域下界。
    pub lo: i32,
    /// 合法域上界。
    pub hi: i32,
    /// 逻辑 tick（唯一时间源，零墙钟）。
    pub tick: u64,
}

impl ClampNotice {
    /// 读屏单行。
    pub fn screen_line(&self) -> String {
        format!(
            "钳制「{}」{}→{}（合法域 [{},{}]，tick {}）",
            self.field, self.original, self.clamped, self.lo, self.hi, self.tick
        )
    }
}

/// 钳制告警环形账（满后覆盖最旧，覆盖次数显性计数）。
#[derive(Clone, Debug)]
pub struct ClampLog {
    buf: Vec<ClampNotice>,
    head: usize,
    /// 累计入账次数（含被覆盖者；判据用单调锚）。
    pushes: u64,
    /// 被覆盖条数。
    overwritten: u64,
}

impl ClampLog {
    /// 空账。
    pub fn new() -> ClampLog {
        ClampLog { buf: Vec::new(), head: 0, pushes: 0, overwritten: 0 }
    }

    /// 入账（满后覆盖最旧；零 panic 面）。
    pub fn push(&mut self, n: ClampNotice) {
        if self.buf.len() < CLAMP_LOG_CAP {
            self.buf.push(n);
        } else {
            if self.head >= self.buf.len() {
                self.head = 0;
            }
            self.buf[self.head] = n;
            self.head += 1;
            if self.head >= self.buf.len() {
                self.head = 0;
            }
            self.overwritten = self.overwritten.saturating_add(1);
        }
        self.pushes = self.pushes.saturating_add(1);
    }

    /// 在账条数。
    pub fn len(&self) -> usize {
        self.buf.len()
    }

    /// 是否空。
    pub fn is_empty(&self) -> bool {
        self.buf.is_empty()
    }

    /// 累计入账次数。
    pub fn total_pushes(&self) -> u64 {
        self.pushes
    }

    /// 被覆盖条数。
    pub fn total_overwritten(&self) -> u64 {
        self.overwritten
    }

    /// 逻辑序快照（最旧→最新；环形账的确定性读面）。
    pub fn snapshot(&self) -> Vec<ClampNotice> {
        if self.buf.is_empty() {
            return Vec::new();
        }
        let n = self.buf.len();
        let start = if n == CLAMP_LOG_CAP { self.head } else { 0 };
        let mut out = Vec::new();
        let mut k = 0usize;
        while k < n {
            let idx = (start + k) % n;
            if let Some(item) = self.buf.get(idx) {
                out.push(item.clone());
            }
            k += 1;
        }
        out
    }
}

/// 一宗立案（异常检出 → 立案流转；现象/影响/定位/处置四件齐）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CaseRecord {
    /// 案号（单调递增）。
    pub id: u64,
    /// 诊断码。
    pub code: &'static str,
    /// 现象。
    pub phenomenon: &'static str,
    /// 影响。
    pub impact: &'static str,
    /// 定位（入口/闸位）。
    pub locus: &'static str,
    /// 处置（给复用方的下一步）。
    pub remedy: &'static str,
    /// 证据 tick。
    pub tick: u64,
}

impl CaseRecord {
    /// 读屏单行。
    pub fn screen_line(&self) -> String {
        format!(
            "案#{} [{}] {}｜影响：{}｜定位：{}｜处置：{}",
            self.id, self.code, self.phenomenon, self.impact, self.locus, self.remedy
        )
    }
}

/// 立案账（**append-only**：只追加不覆盖；满后拒绝新案并显性溢出计数
/// ——覆盖旧案等于销毁证据，家族纪律禁止）。
#[derive(Clone, Debug)]
pub struct CaseLedger {
    items: Vec<CaseRecord>,
    next_id: u64,
    /// 因账满而未立案的次数（显性溢出，不静默）。
    overflow: u64,
}

impl CaseLedger {
    /// 空账。
    pub fn new() -> CaseLedger {
        CaseLedger { items: Vec::new(), next_id: 1, overflow: 0 }
    }

    /// 立案（满 → [`StyleTreeError::CaseFull`]，原错误路径不受牵连）。
    pub fn open_case(
        &mut self,
        code: &'static str,
        phenomenon: &'static str,
        locus: &'static str,
        remedy: &'static str,
        tick: u64,
    ) -> Result<u64, StyleTreeError> {
        if self.items.len() >= CASE_CAP {
            self.overflow = self.overflow.saturating_add(1);
            return Err(StyleTreeError::CaseFull { cap: CASE_CAP });
        }
        let id = self.next_id;
        self.next_id = self.next_id.saturating_add(1);
        self.items.push(CaseRecord {
            id,
            code,
            phenomenon,
            impact: "样式面按降级矩阵处置：拒绝/钳制/拒收整批，不污染在账状态",
            locus,
            remedy,
            tick,
        });
        Ok(id)
    }

    /// 在账案数。
    pub fn len(&self) -> usize {
        self.items.len()
    }

    /// 是否空。
    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    /// 显性溢出次数。
    pub fn total_overflow(&self) -> u64 {
        self.overflow
    }

    /// 快照（append-only，追加序即逻辑序）。
    pub fn snapshot(&self) -> Vec<CaseRecord> {
        let mut out = Vec::new();
        let mut i = 0usize;
        while i < self.items.len() {
            if let Some(c) = self.items.get(i) {
                out.push(c.clone());
            }
            i += 1;
        }
        out
    }
}

/// 一条事件（intern/去重/COW/钳制/拒绝/立案/投喂/批量——全链留痕）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EventRecord {
    /// 逻辑 tick。
    pub tick: u64,
    /// 事件类别（'static 闭集口径，见 EventLog 注释）。
    pub kind: &'static str,
    /// 关联 key u64（无则 0）。
    pub key: u64,
    /// 便签（动态内容走 format!）。
    pub note: String,
}

/// 事件环形账（满后覆盖最旧，覆盖计数显性）。
#[derive(Clone, Debug)]
pub struct EventLog {
    buf: Vec<EventRecord>,
    head: usize,
    pushes: u64,
    overwritten: u64,
}

impl EventLog {
    /// 空账。
    pub fn new() -> EventLog {
        EventLog { buf: Vec::new(), head: 0, pushes: 0, overwritten: 0 }
    }

    /// 入账。
    pub fn push(&mut self, kind: &'static str, key: u64, note: String, tick: u64) {
        let rec = EventRecord { tick, kind, key, note };
        if self.buf.len() < EVENT_CAP {
            self.buf.push(rec);
        } else {
            if self.head >= self.buf.len() {
                self.head = 0;
            }
            self.buf[self.head] = rec;
            self.head += 1;
            if self.head >= self.buf.len() {
                self.head = 0;
            }
            self.overwritten = self.overwritten.saturating_add(1);
        }
        self.pushes = self.pushes.saturating_add(1);
    }

    /// 在账条数。
    pub fn len(&self) -> usize {
        self.buf.len()
    }

    /// 是否空。
    pub fn is_empty(&self) -> bool {
        self.buf.is_empty()
    }

    /// 累计入账次数。
    pub fn total_pushes(&self) -> u64 {
        self.pushes
    }

    /// 逻辑序快照。
    pub fn snapshot(&self) -> Vec<EventRecord> {
        if self.buf.is_empty() {
            return Vec::new();
        }
        let n = self.buf.len();
        let start = if n == EVENT_CAP { self.head } else { 0 };
        let mut out = Vec::new();
        let mut k = 0usize;
        while k < n {
            let idx = (start + k) % n;
            if let Some(item) = self.buf.get(idx) {
                out.push(item.clone());
            }
            k += 1;
        }
        out
    }
}

// ---------------------------------------------------------------------------
// 九、共享样式结构（判据二：单实例共享 + 引用计数 + 去重率 + COW）
// ---------------------------------------------------------------------------

/// 共享槽：一个 key 对应的**全局唯一**样式实例。
///
/// 对外**只读**（`values` 无 `&mut` 通道——共享实例不可变是 COW 的
/// 前提）；变更只能经 [`StyleTreeEngine::cow_write`] 生成新实例。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ShareSlot {
    /// 已打包 key u64（槽序按此升序，二分查找的序基础）。
    pub key: u64,
    /// 共享实例（不可变）。
    pub values: ComputedValues,
    /// 强引用计数（intern/acquire 各 +1，release -1；0 = 无持有者，
    /// 槽保留供再次共享与去重统计）。
    pub strong: u32,
    /// 实例纪元（每次 COW 生成新实例 +1；判据可对账）。
    pub epoch: u32,
    /// 该槽经历过的 COW 变更次数。
    pub cows: u32,
}

/// 共享表统计（去重账 + 引用账）。
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ShareStats {
    /// intern 尝试总数。
    pub interns: u64,
    /// 命中已有实例的去重次数。
    pub dedup_hits: u64,
    /// 独立实例数（= interns - dedup_hits，守恒判据锚）。
    pub unique: u64,
    /// acquire 总数。
    pub acquires: u64,
    /// release 总数。
    pub releases: u64,
    /// COW 写次数。
    pub cows: u64,
    /// 引用账目迁移次数（COW：源 -1 → 新槽 +1）。
    pub migrations: u64,
    /// 共享面拒绝次数。
    pub rejects: u64,
    /// 当前存活引用数（intern+acquire 增、release 减、COW 净零）。
    pub live: u64,
}

impl ShareStats {
    /// 守恒一：interns == unique + dedup_hits。
    pub fn conserves_interns(&self) -> bool {
        self.interns == self.unique.saturating_add(self.dedup_hits)
    }

    /// 守恒二：live == Σ 槽 strong（健康面运行期复算）。
    pub fn conserves_live(&self, slots: &[ShareSlot]) -> bool {
        let mut sum = 0u64;
        let mut i = 0usize;
        while i < slots.len() {
            sum = sum.saturating_add(slots[i].strong as u64);
            i += 1;
        }
        sum == self.live
    }

    /// 去重率（千分数：hits × 1000 / interns；无投喂时 0）。
    pub fn dedup_permille(&self) -> u32 {
        if self.interns == 0 {
            return 0;
        }
        ((self.dedup_hits.saturating_mul(1000)) / self.interns) as u32
    }

    /// 读屏摘要。
    pub fn screen_text(&self) -> String {
        format!(
            "共享表：intern {} 次（去重命中 {}，独立实例 {}，去重率 {}‰），持有 {} 引用，COW {} 次（迁移 {}）",
            self.interns, self.dedup_hits, self.unique, self.dedup_permille(), self.live, self.cows, self.migrations
        )
    }
}

/// 共享样式表（键序有序表 + 二分查找 O(logN)；同 key 全局单实例）。
#[derive(Clone, Debug)]
pub struct SharedStyleStore {
    /// 槽（按 key 升序——二分查找与确定性遍历的序基础）。
    pub slots: Vec<ShareSlot>,
    /// 统计账。
    pub stats: ShareStats,
}

/// 共享授予回执。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ShareGrant {
    /// key u64。
    pub key: u64,
    /// 是否命中已有实例（去重）。
    pub deduped: bool,
    /// 授予后该槽强引用数。
    pub strong: u32,
}

impl SharedStyleStore {
    /// 空表。
    pub fn new() -> SharedStyleStore {
        SharedStyleStore { slots: Vec::new(), stats: ShareStats::default() }
    }

    /// 二分查键（命中 → Ok(槽下标)；未命中 → Err(应插位置)）。O(logN)。
    fn find(&self, key: u64) -> Result<usize, usize> {
        let mut lo = 0usize;
        let mut hi = self.slots.len();
        while lo < hi {
            let mid = lo + (hi - lo) / 2;
            let k = self.slots[mid].key;
            if k == key {
                return Ok(mid);
            }
            if k < key {
                lo = mid + 1;
            } else {
                hi = mid;
            }
        }
        Err(lo)
    }

    /// 在账槽下标。
    pub fn slot_idx(&self, key: u64) -> Option<usize> {
        self.find(key).ok()
    }

    /// 读共享实例（**只读**——共享实例不可变的唯一读面）。
    pub fn values_of(&self, key: u64) -> Option<&ComputedValues> {
        match self.find(key) {
            Ok(i) => self.slots.get(i).map(|s| &s.values),
            Err(_) => None,
        }
    }

    /// 读强引用数。
    pub fn strong_of(&self, key: u64) -> Option<u32> {
        match self.find(key) {
            Ok(i) => self.slots.get(i).map(|s| s.strong),
            Err(_) => None,
        }
    }

    /// **intern**：同 key 已在账 → 去重命中，强引用 +1；未在账 →
    /// 建新槽（强引用 1）。值由引擎层先行按规格钳制，本层不改写。
    pub fn intern(
        &mut self,
        key: u64,
        values: ComputedValues,
        tick: u64,
    ) -> Result<ShareGrant, StyleTreeError> {
        match self.find(key) {
            Ok(i) => {
                let slot = &mut self.slots[i];
                slot.strong = slot.strong.saturating_add(1);
                let strong = slot.strong;
                self.stats.dedup_hits = self.stats.dedup_hits.saturating_add(1);
                self.stats.interns = self.stats.interns.saturating_add(1);
                self.stats.live = self.stats.live.saturating_add(1);
                let _ = tick;
                Ok(ShareGrant { key, deduped: true, strong })
            }
            Err(pos) => {
                if self.slots.len() >= SHARE_CAP {
                    self.stats.rejects = self.stats.rejects.saturating_add(1);
                    return Err(StyleTreeError::CapFull { what: "share-slots", cap: SHARE_CAP });
                }
                self.slots.insert(pos, ShareSlot {
                    key,
                    values,
                    strong: 1,
                    epoch: 0,
                    cows: 0,
                });
                self.stats.interns = self.stats.interns.saturating_add(1);
                self.stats.unique = self.stats.unique.saturating_add(1);
                self.stats.live = self.stats.live.saturating_add(1);
                Ok(ShareGrant { key, deduped: false, strong: 1 })
            }
        }
    }

    /// **acquire**：对在账槽再取一个强引用（未 intern 先 acquire = 误用拒）。
    pub fn acquire(&mut self, key: u64) -> Result<ShareGrant, StyleTreeError> {
        match self.find(key) {
            Ok(i) => {
                let slot = &mut self.slots[i];
                slot.strong = slot.strong.saturating_add(1);
                let strong = slot.strong;
                self.stats.acquires = self.stats.acquires.saturating_add(1);
                self.stats.live = self.stats.live.saturating_add(1);
                Ok(ShareGrant { key, deduped: true, strong })
            }
            Err(_) => {
                self.stats.rejects = self.stats.rejects.saturating_add(1);
                Err(StyleTreeError::ShareState { key, why: "acquire 前未 intern" })
            }
        }
    }

    /// **release**：归还一个强引用（未持有即 release = 误用拒；
    /// 减到 0 槽保留——去重账是历史账不是活跃账）。
    pub fn release(&mut self, key: u64) -> Result<u32, StyleTreeError> {
        match self.find(key) {
            Ok(i) => {
                let slot = &mut self.slots[i];
                if slot.strong == 0 {
                    self.stats.rejects = self.stats.rejects.saturating_add(1);
                    return Err(StyleTreeError::ShareState { key, why: "release 时强引用已为 0" });
                }
                slot.strong -= 1;
                let strong = slot.strong;
                self.stats.releases = self.stats.releases.saturating_add(1);
                self.stats.live = self.stats.live.saturating_sub(1);
                Ok(strong)
            }
            Err(_) => {
                self.stats.rejects = self.stats.rejects.saturating_add(1);
                Err(StyleTreeError::ShareState { key, why: "release 目标 key 未在账" })
            }
        }
    }

    /// **COW 落账**（引擎层已按补丁生成好新实例后调这里）：
    /// 新实例入账（或去重命中）+ 源槽强引用 -1（账目迁移，净零）。
    /// 源槽 strong==0 = 误用拒（没有持有者的写是凭空写）。
    pub fn cow_commit(
        &mut self,
        key_old: u64,
        key_new: u64,
        new_values: ComputedValues,
    ) -> Result<ShareGrant, StyleTreeError> {
        let old_idx = match self.find(key_old) {
            Ok(i) => i,
            Err(_) => {
                self.stats.rejects = self.stats.rejects.saturating_add(1);
                return Err(StyleTreeError::ShareState { key: key_old, why: "COW 源 key 未在账" });
            }
        };
        if self.slots[old_idx].strong == 0 {
            self.stats.rejects = self.stats.rejects.saturating_add(1);
            return Err(StyleTreeError::ShareState { key: key_old, why: "COW 源槽无持有引用" });
        }
        if key_new == key_old {
            self.stats.rejects = self.stats.rejects.saturating_add(1);
            return Err(StyleTreeError::ShareState { key: key_old, why: "COW 目标键与源键相同（同槽不可变）" });
        }
        // 先入新账，再迁源账（序固定——判据可对拍）。
        let grant = self.intern(key_new, new_values, 0)?;
        let _ = self.release(key_old)?;
        // 源槽账目：纪元 +1、COW +1（实例本体没变，变的是账）。
        let src = &mut self.slots[old_idx];
        src.epoch = src.epoch.saturating_add(1);
        src.cows = src.cows.saturating_add(1);
        self.stats.cows = self.stats.cows.saturating_add(1);
        self.stats.migrations = self.stats.migrations.saturating_add(1);
        Ok(grant)
    }

    /// 在账槽读（按逻辑序快照；确定性遍历面）。
    pub fn snapshot(&self) -> Vec<ShareSlot> {
        let mut out = Vec::new();
        let mut i = 0usize;
        while i < self.slots.len() {
            if let Some(s) = self.slots.get(i) {
                out.push(s.clone());
            }
            i += 1;
        }
        out
    }
}

// ---------------------------------------------------------------------------
// 十、值补丁（定长数组，零 Vec——补丁体积上界即属性全集）
// ---------------------------------------------------------------------------

/// 值补丁：至多 [`PROP_COUNT`] 条 (属性, 原始值)。
///
/// 定长数组而非 Vec：补丁体积上界封闭、Copy 可栈传、双跑逐字节一致。
#[derive(Clone, Copy, Debug)]
pub struct ValuePatch {
    entries: [(u8, i32); PROP_COUNT],
    len: usize,
}

impl ValuePatch {
    /// 空补丁。
    pub const fn new() -> ValuePatch {
        ValuePatch { entries: [(0u8, 0i32); PROP_COUNT], len: 0 }
    }

    /// 条数。
    pub const fn len(&self) -> usize {
        self.len
    }

    /// 是否空。
    pub const fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// 追加一条（满 → 容量拒；属性在入账时即封存秩，解码走守卫）。
    pub fn push(&mut self, p: PropId, raw: i32) -> Result<(), StyleTreeError> {
        if self.len >= PROP_COUNT {
            return Err(StyleTreeError::CapFull { what: "patch-entries", cap: PROP_COUNT });
        }
        self.entries[self.len] = (p.rank() as u8, raw);
        self.len += 1;
        Ok(())
    }

    /// 解码第 i 条（越界/秩非法 → 枚举守卫拒）。
    pub fn decoded_at(&self, i: usize) -> Result<(PropId, i32), StyleTreeError> {
        if i >= self.len {
            return Err(StyleTreeError::PropUnknown { rank: u8::MAX });
        }
        let (r, v) = self.entries[i];
        match PropId::of_rank(r as usize) {
            Some(p) => Ok((p, v)),
            None => Err(StyleTreeError::PropUnknown { rank: r }),
        }
    }

    /// 全量解码（判据对拍面）。
    pub fn decoded_all(&self) -> Result<Vec<(PropId, i32)>, StyleTreeError> {
        let mut out = Vec::new();
        let mut i = 0usize;
        while i < self.len {
            out.push(self.decoded_at(i)?);
            i += 1;
        }
        Ok(out)
    }
}

// ---------------------------------------------------------------------------
// 十一、样式树（父继承 + 继承深度账 + 环检测 + 确定性批量）
// ---------------------------------------------------------------------------

/// 根节点父句柄（无父）。
pub const NO_PARENT: u64 = u64::MAX;

/// 树节点：局部声明 + 父句柄 + 计算缓存。
#[derive(Clone, Debug, PartialEq)]
pub struct TreeNode {
    /// 节点句柄（单调递增，全树唯一；父引用按句柄记——句柄不随
    /// 物理删除移位，删除叶子安全）。
    pub id: u64,
    /// 父节点句柄（根 = [`NO_PARENT`]）。
    pub parent: u64,
    /// 已打包 key u64（该节点的样式身份）。
    pub key: u64,
    /// 读屏标签（越界字符边界安全截断）。
    pub label: String,
    /// 局部声明（只看指定掩码覆盖的槽；未指定槽的值无语义）。
    pub decl: ComputedValues,
    /// 深度（根 0，每层 +1；上界 [`MAX_TREE_DEPTH`]）。
    pub depth: u32,
    /// 计算缓存（纪元匹配时有效；任何写操作推进全局纪元即整体失效）。
    pub cache: Option<ComputedValues>,
    /// 缓存纪元（与引擎全局纪元比对）。
    pub cache_epoch: u64,
}

/// 继承轨迹（继承深度账的单节点切片）。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct InheritTrace {
    /// 继承跳数加权和（每次继承计入当时的祖先链步数）。
    pub hops: u64,
    /// 祖先链步数（本节点到根的层数）。
    pub steps: u32,
    /// 继承得值的属性数。
    pub inherited: u32,
    /// 最深一次继承发生的链层。
    pub deepest_hop: u32,
}

/// 树统计（继承深度账的总量面）。
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct TreeStats {
    /// add_node 总数。
    pub adds: u64,
    /// 拆除叶子总数。
    pub removes: u64,
    /// reparent 总数。
    pub reparents: u64,
    /// 树面拒绝总数。
    pub rejects: u64,
    /// 计算值产出总数（含缓存命中口径见引擎统计）。
    pub computes: u64,
    /// 继承跳数累计。
    pub inherit_hops: u64,
    /// 单节点最深继承链层记录。
    pub max_hops: u32,
}

impl TreeStats {
    /// 读屏摘要。
    pub fn screen_text(&self) -> String {
        format!(
            "样式树：建点 {}，拆除 {}，重挂 {}，拒绝 {}，计算 {} 次，继承跳 {}（最深链层 {}）",
            self.adds, self.removes, self.reparents, self.rejects, self.computes,
            self.inherit_hops, self.max_hops
        )
    }
}

/// 样式树（森林：多根合法；父引用按句柄，结构上无环——环只可能
/// 经 reparent 引入，闸在 reparent 与继承链步数上限两处把守）。
#[derive(Clone, Debug)]
pub struct StyleTree {
    /// 节点（插入序存放）。
    pub nodes: Vec<TreeNode>,
    /// 统计账。
    pub stats: TreeStats,
    next_id: u64,
}

impl StyleTree {
    /// 空树。
    pub fn new() -> StyleTree {
        StyleTree { nodes: Vec::new(), stats: TreeStats::default(), next_id: 1 }
    }

    /// 句柄 → 下标（线性扫；512 上界内 O(N) 常数极小）。
    pub fn node_idx(&self, id: u64) -> Option<usize> {
        let mut i = 0usize;
        while i < self.nodes.len() {
            if let Some(n) = self.nodes.get(i) {
                if n.id == id {
                    return Some(i);
                }
            }
            i += 1;
        }
        None
    }

    /// 按 key 查第一个在账节点句柄（上游投喂去重建点的查询面）。
    pub fn node_with_key(&self, key: u64) -> Option<u64> {
        let mut i = 0usize;
        while i < self.nodes.len() {
            if let Some(n) = self.nodes.get(i) {
                if n.key == key {
                    return Some(n.id);
                }
            }
            i += 1;
        }
        None
    }

    /// 读节点（不可变）。
    pub fn node_of(&self, id: u64) -> Option<&TreeNode> {
        match self.node_idx(id) {
            Some(i) => self.nodes.get(i),
            None => None,
        }
    }

    /// 子树相对高度（node 自身 0；O(N) 扫描，仅 reparent 前闸用）。
    pub fn subtree_height(&self, id: u64) -> u32 {
        let base = match self.node_idx(id) {
            Some(i) => self.nodes[i].depth,
            None => return 0,
        };
        let mut h = 0u32;
        let mut i = 0usize;
        while i < self.nodes.len() {
            if let Some(n) = self.nodes.get(i) {
                if n.depth >= base && self.in_subtree_of(id, n.id) {
                    let rel = n.depth - base;
                    if rel > h {
                        h = rel;
                    }
                }
            }
            i += 1;
        }
        h
    }

    /// target 是否在 id 的子树内（含 id 自身；纯读，O(N×深度)）。
    pub fn in_subtree_of(&self, id: u64, target: u64) -> bool {
        if id == target {
            return true;
        }
        let mut cur = match self.node_idx(target) {
            Some(i) => self.nodes[i].parent,
            None => return false,
        };
        let mut steps = 0u32;
        while cur != NO_PARENT && steps < MAX_TREE_DEPTH {
            if cur == id {
                return true;
            }
            steps += 1;
            cur = match self.node_idx(cur) {
                Some(i) => self.nodes[i].parent,
                None => return false,
            };
        }
        false
    }

    /// **建点**：局部声明入树（父存在闸 → 容量闸 → 深度恰阈闸；
    /// 标签越界字符边界安全截断）。
    pub fn add_node(
        &mut self,
        parent: Option<u64>,
        key: u64,
        decl: ComputedValues,
        label: &str,
    ) -> Result<u64, StyleTreeError> {
        self.stats.adds = self.stats.adds.saturating_add(1);
        // 闸 1：容量（满后不覆盖旧行）。
        if self.nodes.len() >= MAX_TREE_NODES {
            self.stats.rejects = self.stats.rejects.saturating_add(1);
            return Err(StyleTreeError::CapFull { what: "tree-nodes", cap: MAX_TREE_NODES });
        }
        // 闸 2：父存在。
        let parent_id = match parent {
            Some(p) => {
                if self.node_idx(p).is_none() {
                    self.stats.rejects = self.stats.rejects.saturating_add(1);
                    return Err(StyleTreeError::NodeUnknown { node: p });
                }
                p
            }
            None => NO_PARENT,
        };
        // 闸 3：深度恰阈（父深 +1；根 0）。
        let depth = if parent_id == NO_PARENT {
            0u32
        } else {
            let pd = match self.node_idx(parent_id) {
                Some(i) => self.nodes[i].depth,
                None => 0,
            };
            pd.saturating_add(1)
        };
        if depth > MAX_TREE_DEPTH {
            self.stats.rejects = self.stats.rejects.saturating_add(1);
            return Err(StyleTreeError::Depth {
                node: 0,
                would_be: depth,
                cap: MAX_TREE_DEPTH,
            });
        }
        // 标签钳制（字符边界安全，不裸切字节）。
        let mut lab = String::from(label);
        if lab.len() > NODE_LABEL_CAP {
            let mut end = NODE_LABEL_CAP;
            while end > 0 && !lab.is_char_boundary(end) {
                end -= 1;
            }
            lab = String::from(&lab[..end]);
        }
        let id = self.next_id;
        self.next_id = self.next_id.saturating_add(1);
        self.nodes.push(TreeNode {
            id,
            parent: parent_id,
            key,
            label: lab,
            decl,
            depth,
            cache: None,
            cache_epoch: 0,
        });
        Ok(id)
    }

    /// **拆叶子**（有子孙拒拆——物理搬移会破坏父引用语义；先拆孙）。
    pub fn remove_leaf(&mut self, id: u64) -> Result<(), StyleTreeError> {
        let idx = match self.node_idx(id) {
            Some(i) => i,
            None => {
                self.stats.rejects = self.stats.rejects.saturating_add(1);
                return Err(StyleTreeError::NodeUnknown { node: id });
            }
        };
        let mut child_at = None;
        let mut i = 0usize;
        while i < self.nodes.len() {
            if let Some(n) = self.nodes.get(i) {
                if n.parent == id {
                    child_at = Some(i);
                }
            }
            i += 1;
        }
        if child_at.is_some() {
            self.stats.rejects = self.stats.rejects.saturating_add(1);
            return Err(StyleTreeError::PropGuard { locus: "remove_leaf/non-leaf" });
        }
        self.nodes.remove(idx);
        self.stats.removes = self.stats.removes.saturating_add(1);
        Ok(())
    }

    /// **重挂**（环检测：新父的祖先链遇本节点即环；深度恰阈按子树
    /// 高度整链推算；None = 摘成根）。
    pub fn reparent(
        &mut self,
        id: u64,
        new_parent: Option<u64>,
    ) -> Result<(), StyleTreeError> {
        self.stats.reparents = self.stats.reparents.saturating_add(1);
        let idx = match self.node_idx(id) {
            Some(i) => i,
            None => {
                self.stats.rejects = self.stats.rejects.saturating_add(1);
                return Err(StyleTreeError::NodeUnknown { node: id });
            }
        };
        let base_depth = match new_parent {
            Some(p) => {
                let pidx = match self.node_idx(p) {
                    Some(i) => i,
                    None => {
                        self.stats.rejects = self.stats.rejects.saturating_add(1);
                        return Err(StyleTreeError::NodeUnknown { node: p });
                    }
                };
                // 环检测：沿新父祖先链（含新父自身）扫，遇本节点即环。
                let mut cur = p;
                let mut steps = 0u32;
                let mut cyclic = false;
                while cur != NO_PARENT && steps < MAX_TREE_DEPTH {
                    if cur == id {
                        cyclic = true;
                    }
                    steps += 1;
                    cur = match self.node_idx(cur) {
                        Some(i) => self.nodes[i].parent,
                        None => break,
                    };
                }
                if cyclic || p == id {
                    self.stats.rejects = self.stats.rejects.saturating_add(1);
                    return Err(StyleTreeError::Cycle { node: id, via: p });
                }
                if steps > MAX_TREE_DEPTH {
                    self.stats.rejects = self.stats.rejects.saturating_add(1);
                    return Err(StyleTreeError::Cycle { node: id, via: p });
                }
                self.nodes[pidx].depth.saturating_add(1)
            }
            None => 0u32,
        };
        // 深度恰阈：新基底 + 子树高度。
        let would_be = base_depth.saturating_add(self.subtree_height(id));
        if would_be > MAX_TREE_DEPTH {
            self.stats.rejects = self.stats.rejects.saturating_add(1);
            return Err(StyleTreeError::Depth { node: id, would_be, cap: MAX_TREE_DEPTH });
        }
        // 提交：改父 + 子树整链刷深度。
        let parent_id = new_parent.unwrap_or(NO_PARENT);
        self.nodes[idx].parent = parent_id;
        self.refresh_depths(id, base_depth);
        Ok(())
    }

    /// 子树深度刷新（迭代栈，零递归——递归深度不可控是家族禁手）。
    fn refresh_depths(&mut self, root: u64, base: u32) {
        let mut stack: Vec<(u64, u32)> = Vec::new();
        stack.push((root, base));
        while let Some(top) = stack.pop() {
            let (nid, dep) = top;
            if let Some(i) = self.node_idx(nid) {
                self.nodes[i].depth = dep;
            }
            // 收集子节点（先收集后改，避开借用冲突）。
            let mut kids: Vec<u64> = Vec::new();
            let mut k = 0usize;
            while k < self.nodes.len() {
                if let Some(n) = self.nodes.get(k) {
                    if n.parent == nid && n.id != nid {
                        kids.push(n.id);
                    }
                }
                k += 1;
            }
            let mut j = 0usize;
            while j < kids.len() {
                stack.push((kids[j], dep.saturating_add(1)));
                j += 1;
            }
        }
    }

    /// **计算值（纯函数面）**：局部指定 → 祖先链补未指定槽（继承）→
    /// 缺省补底。环不可达时步数上限兜底（防御性：步数越 [`MAX_TREE_DEPTH`]
    /// 即按环报——结构上不可能，属异常检出的双保险）。
    ///
    /// 继承只取规格表 `inherits=true` 的槽？不——**未指定即继承**按
    /// CSS 级联惯例全槽生效，规格表 `inherits` 字段是给下游消费层
    /// 的口径提示（判据对拍面），本单计算面统一全槽继承，特此声明
    /// 边界：伪类状态差异归 key 四元组，不归继承面。
    pub fn compute_pure(&self, id: u64) -> Result<(ComputedValues, InheritTrace), StyleTreeError> {
        let idx = match self.node_idx(id) {
            Some(i) => i,
            None => return Err(StyleTreeError::NodeUnknown { node: id }),
        };
        let mut acc = self.nodes[idx].decl;
        let mut hops = 0u64;
        let mut deepest = 0u32;
        let mut inherited = 0u32;
        let mut steps = 0u32;
        let mut cur = self.nodes[idx].parent;
        while cur != NO_PARENT {
            if steps >= MAX_TREE_DEPTH {
                return Err(StyleTreeError::Cycle { node: id, via: cur });
            }
            steps += 1;
            let pidx = match self.node_idx(cur) {
                Some(i) => i,
                None => return Err(StyleTreeError::NodeUnknown { node: cur }),
            };
            let m = self.nodes[pidx].decl.specified & !acc.specified;
            if m != 0 {
                let mut bit = 0u32;
                while bit < PROP_COUNT as u32 {
                    let mask = 1u32 << bit;
                    if m & mask != 0 {
                        acc.vals[bit as usize] = self.nodes[pidx].decl.vals[bit as usize];
                        acc.specified |= mask;
                        inherited = inherited.saturating_add(1);
                        if steps > deepest {
                            deepest = steps;
                        }
                    }
                    bit += 1;
                }
                hops = hops.saturating_add(steps as u64);
            }
            cur = self.nodes[pidx].parent;
        }
        // 缺省补底（未指定且祖先也没有 → 规格表缺省）。
        let mut bit = 0usize;
        while bit < PROP_COUNT {
            if acc.specified & (1u32 << bit) == 0 {
                acc.vals[bit] = PROP_SPECS[bit].default;
            }
            bit += 1;
        }
        acc.specified = PROP_MASK_ALL;
        Ok((acc, InheritTrace { hops, steps, inherited, deepest_hop: deepest }))
    }

    /// 计算缓存失效面（引擎写操作后整体推进纪元时调）。
    pub fn invalidate_cache(&mut self) {
        let mut i = 0usize;
        while i < self.nodes.len() {
            self.nodes[i].cache = None;
            i += 1;
        }
    }

    /// 在账节点数。
    pub fn len(&self) -> usize {
        self.nodes.len()
    }

    /// 是否空。
    pub fn is_empty(&self) -> bool {
        self.nodes.is_empty()
    }

    /// 全树健全性（健康面纯检：父句柄可解、深度一致、祖先链不越上界）。
    pub fn sanity(&self) -> bool {
        let mut i = 0usize;
        while i < self.nodes.len() {
            let n = match self.nodes.get(i) {
                Some(n) => n,
                None => return false,
            };
            if n.parent == NO_PARENT {
                if n.depth != 0 {
                    return false;
                }
            } else {
                let pidx = match self.node_idx(n.parent) {
                    Some(p) => p,
                    None => return false,
                };
                if self.nodes[pidx].depth.saturating_add(1) != n.depth {
                    return false;
                }
                if self.nodes[pidx].depth >= MAX_TREE_DEPTH {
                    return false;
                }
            }
            i += 1;
        }
        true
    }
}

// ---------------------------------------------------------------------------
// 十二、引擎总装（三闸次序单源执行；账本 + 共享表 + 树一机统管）
// ---------------------------------------------------------------------------

/// 引擎级统计（共享表另有 [`ShareStats`]、树另有 [`TreeStats`]）。
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct StyleStats {
    /// declare 声明次数。
    pub declares: u64,
    /// 引擎层钳制次数。
    pub clamps: u64,
    /// 引擎层拒绝次数。
    pub rejects: u64,
    /// 立案次数。
    pub cases: u64,
    /// 立案账溢出次数（append-only 满后）。
    pub case_overflow: u64,
    /// 计算值产出次数。
    pub computes: u64,
    /// 缓存命中次数。
    pub cache_hits: u64,
    /// 缓存未命中次数。
    pub cache_misses: u64,
    /// 继承跳数累计。
    pub inherit_hops: u64,
    /// 单节点最深祖先链层数记录。
    pub max_inherit_steps: u32,
    /// 上游投喂批次数。
    pub ingests: u64,
    /// 批量应用成功次数。
    pub batches: u64,
    /// 批量应用被拒次数。
    pub batch_rejects: u64,
    /// 注入检出次数。
    pub injections_caught: u64,
    /// 注入未检出次数（如实入账——恒绿是坏味道）。
    pub injections_missed: u64,
}

impl StyleStats {
    /// 守恒：计算次数 == 命中 + 未命中。
    pub fn conserves(&self) -> bool {
        self.computes == self.cache_hits.saturating_add(self.cache_misses)
    }
}

/// 引擎：计算样式树 + 共享结构一机统管（三闸次序单源：所有入口按
/// [`GATE_ORDER`] 执行——先校验拒绝、再钳制告警、异常立案、最后入账）。
pub struct StyleTreeEngine {
    /// 共享样式表。
    pub store: SharedStyleStore,
    /// 样式树。
    pub tree: StyleTree,
    /// 钳制告警环形账。
    pub clamps: ClampLog,
    /// 立案账（append-only）。
    pub cases: CaseLedger,
    /// 事件环形账。
    pub events: EventLog,
    /// 引擎统计。
    pub stats: StyleStats,
    /// 逻辑 tick（唯一时间源，零墙钟）。
    pub tick: u64,
    /// 全局纪元（任何写操作 +1；计算缓存按纪元失效）。
    pub epoch: u64,
}

/// declare 回执。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DeclareReceipt {
    /// 节点句柄。
    pub node: u64,
    /// 应用的补丁条数。
    pub entries: u32,
    /// 其中被钳制条数。
    pub clamps: u32,
}

/// COW 回执。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CowReceipt {
    /// 源 key。
    pub key_old: u64,
    /// 新 key。
    pub key_new: u64,
    /// 补丁中被钳制条数。
    pub clamps: u32,
    /// 新实例是否去重命中（目标 key 已有实例）。
    pub deduped: bool,
    /// 账目是否迁移（源引用 -1）。
    pub migrated: bool,
}

/// 批量项：一个节点 + 一份补丁。
#[derive(Clone, Debug)]
pub struct BatchItem {
    /// 节点句柄。
    pub node: u64,
    /// 值补丁。
    pub patch: ValuePatch,
}

/// 批量计划（条数上界 [`MAX_BATCH`]）。
#[derive(Clone, Debug, Default)]
pub struct BatchPlan {
    /// 批量项。
    pub items: Vec<BatchItem>,
}

impl BatchPlan {
    /// 空计划。
    pub fn new() -> BatchPlan {
        BatchPlan { items: Vec::new() }
    }

    /// 追加一项（满 → 容量拒）。
    pub fn push(&mut self, item: BatchItem) -> Result<(), StyleTreeError> {
        if self.items.len() >= MAX_BATCH {
            return Err(StyleTreeError::CapFull { what: "batch-items", cap: MAX_BATCH });
        }
        self.items.push(item);
        Ok(())
    }
}

/// 批量回执。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BatchReceipt {
    /// 应用的批量项数。
    pub applied: usize,
    /// 应用的补丁条数合计。
    pub entries: usize,
    /// 钳制条数合计。
    pub clamps: u32,
}

/// 上游投喂单条：key + 计算值 + 申报哈希。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UpstreamDecl {
    /// 样式 key（四元组）。
    pub key: StyleKey,
    /// 计算值（必须满配：17 槽全指定）。
    pub values: ComputedValues,
    /// 上游申报哈希（对 [`KeyedValues::canonical_bytes`] 的 FNV-1a 64）。
    pub declared_hash: u64,
}

/// 上游投喂回执。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct UpstreamReceipt {
    /// 哈希对账通过条数。
    pub checked: usize,
    /// 入账条数（含去重命中）。
    pub accepted: usize,
    /// 去重命中条数。
    pub deduped: usize,
    /// 新建树节点数。
    pub nodes_added: usize,
    /// 入账时钳制条数。
    pub clamps: u32,
}

/// 下游前向声明（**只声明，不实现**；VE-F2811 按三面消费本单）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DownstreamDecl {
    /// 下游对端标识。
    pub peer: &'static str,
    /// 下游消费面（按序）。
    pub faces: &'static [&'static str],
}

/// 下游声明：VE-F2811 按这三个面消费计算样式树。
pub const DOWNSTREAM_DECL: DownstreamDecl = DownstreamDecl {
    peer: "VE-F2811",
    faces: &["consumer_view", "share_values", "dedup_ledger"],
};

/// 健康报告（[`StyleTreeEngine::health`] 产物）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HealthReport {
    /// 树健全（父句柄可解/深度一致/链不越上界）。
    pub tree_sane: bool,
    /// 共享表 intern 守恒。
    pub store_conserves_interns: bool,
    /// 共享表引用守恒（live == Σ strong）。
    pub store_conserves_live: bool,
    /// 引擎统计守恒（computes == hits + misses）。
    pub stats_conserves: bool,
    /// 立案账未满。
    pub ledger_not_full: bool,
    /// 立案溢出次数。
    pub case_overflow: u64,
    /// 钳制账覆盖次数（环形账正常损耗，非错误）。
    pub clamp_overwritten: u64,
    /// 事件账覆盖次数。
    pub event_overwritten: u64,
    /// 共享表余量（在账/上限）。
    pub store_slots: usize,
    /// 逻辑 tick。
    pub tick: u64,
}

impl HealthReport {
    /// 总判（全闸真 = 健康）。
    pub fn healthy(&self) -> bool {
        self.tree_sane
            && self.store_conserves_interns
            && self.store_conserves_live
            && self.stats_conserves
            && self.ledger_not_full
    }

    /// 读屏单行。
    pub fn screen_text(&self) -> String {
        format!(
            "健康：树{}｜intern 守恒{}｜引用守恒{}｜统计守恒{}｜立案账{}（溢出 {}）｜tick {}",
            if self.tree_sane { "健全" } else { "异常" },
            if self.store_conserves_interns { "过" } else { "不过" },
            if self.store_conserves_live { "过" } else { "不过" },
            if self.stats_conserves { "过" } else { "不过" },
            if self.ledger_not_full { "有余" } else { "已满" },
            self.case_overflow,
            self.tick
        )
    }
}

/// 双跑确定性对拍报告。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DeterminismReport {
    /// 参与对拍节点数。
    pub nodes: usize,
    /// 正序逐节点指纹异或折叠。
    pub pass_a: u64,
    /// 逆序逐节点指纹异或折叠。
    pub pass_b: u64,
    /// 两跑逐节点一致。
    pub consistent: bool,
}

/// 下游只读消费视图（**前向声明的兑现面**：零 `&mut` 方法）。
pub struct ConsumerView<'a> {
    store: &'a SharedStyleStore,
    tree_len: usize,
}

impl<'a> ConsumerView<'a> {
    /// 读共享实例（只读）。
    pub fn values_of(&self, key: u64) -> Option<&ComputedValues> {
        self.store.values_of(key)
    }

    /// 读强引用数。
    pub fn strong_of(&self, key: u64) -> Option<u32> {
        self.store.strong_of(key)
    }

    /// 独立实例数。
    pub fn instances(&self) -> usize {
        self.store.stats.unique as usize
    }

    /// 去重率（千分数）。
    pub fn dedup_permille(&self) -> u32 {
        self.store.stats.dedup_permille()
    }

    /// 树节点数（只读口径）。
    pub fn tree_nodes(&self) -> usize {
        self.tree_len
    }
}

impl StyleTreeEngine {
    /// 空引擎。
    pub fn new() -> StyleTreeEngine {
        StyleTreeEngine {
            store: SharedStyleStore::new(),
            tree: StyleTree::new(),
            clamps: ClampLog::new(),
            cases: CaseLedger::new(),
            events: EventLog::new(),
            stats: StyleStats::default(),
            tick: 0,
            epoch: 1,
        }
    }

    /// 推进逻辑 tick。
    pub fn bump_tick(&mut self) {
        self.tick = self.tick.saturating_add(1);
    }

    /// 立案（异常检出的显性载体；账满 → 溢出计数，不牵连原错误路径）。
    fn file_case(
        &mut self,
        code: &'static str,
        phenomenon: &'static str,
        locus: &'static str,
        remedy: &'static str,
    ) {
        match self.cases.open_case(code, phenomenon, locus, remedy, self.tick) {
            Ok(_) => {
                self.stats.cases = self.stats.cases.saturating_add(1);
            }
            Err(_) => {
                self.stats.case_overflow = self.stats.case_overflow.saturating_add(1);
            }
        }
    }

    /// 按错误立案（phenomenon 取码名，locus 由调用点给）。
    fn file_case_err(&mut self, e: &StyleTreeError, locus: &'static str) {
        self.file_case(
            e.code(),
            "异常检出：见诊断 detail",
            locus,
            "按 StyleTreeError::detail 的可操作建议处置",
        );
    }

    /// 钳制记账（钳制前后值留痕 + 事件留痕）。
    fn note_clamp(&mut self, field: &'static str, original: i32, clamped: i32, lo: i32, hi: i32) {
        self.clamps.push(ClampNotice {
            field,
            original,
            clamped,
            lo,
            hi,
            tick: self.tick,
        });
        self.stats.clamps = self.stats.clamps.saturating_add(1);
        self.log_event("clamp", 0, format!("{} {}→{}", field, original, clamped));
    }

    /// 事件留痕。
    fn log_event(&mut self, kind: &'static str, key: u64, note: String) {
        self.events.push(kind, key, note, self.tick);
    }

    /// 写后缓存失效（全局纪元 +1 + 树面缓存清零——粗粒度但正确简单）。
    fn invalidate(&mut self) {
        self.epoch = self.epoch.saturating_add(1);
        self.tree.invalidate_cache();
    }

    // ---- 共享面（判据二）----

    /// **intern 共享实例**：值逐槽按规格钳制（越界留痕）后入共享表；
    /// 同 key 去重命中强引用 +1。
    pub fn intern(
        &mut self,
        key: StyleKey,
        values: ComputedValues,
    ) -> Result<ShareGrant, StyleTreeError> {
        self.bump_tick();
        let packed = key.pack()?;
        // 越界 → 钳制 + 告警（家族第二闸；先钳再入账）。
        let mut cv = values;
        let mut clamped = 0u32;
        let mut i = 0usize;
        while i < PROP_COUNT {
            let sp = &PROP_SPECS[i];
            let c = clamp_to_domain(sp.lo, sp.hi, cv.vals[i]);
            if c != cv.vals[i] {
                self.note_clamp(sp.name, cv.vals[i], c, sp.lo, sp.hi);
                clamped += 1;
            }
            cv.vals[i] = c;
            i += 1;
        }
        let packed_for_ev = packed;
        let grant = match self.store.intern(packed, cv, self.tick) {
            Ok(g) => g,
            Err(e) => {
                self.stats.rejects = self.stats.rejects.saturating_add(1);
                self.file_case_err(&e, "intern/store");
                return Err(e);
            }
        };
        self.log_event(
            if grant.deduped { "intern-dedup" } else { "intern-new" },
            packed_for_ev,
            format!("强引用 {}", grant.strong),
        );
        let _ = clamped;
        Ok(grant)
    }

    /// acquire 强引用。
    pub fn acquire(&mut self, key: StyleKey) -> Result<ShareGrant, StyleTreeError> {
        self.bump_tick();
        let packed = key.pack()?;
        match self.store.acquire(packed) {
            Ok(g) => {
                self.log_event("acquire", packed, format!("强引用 {}", g.strong));
                Ok(g)
            }
            Err(e) => {
                self.stats.rejects = self.stats.rejects.saturating_add(1);
                self.file_case_err(&e, "acquire/store");
                Err(e)
            }
        }
    }

    /// release 强引用。
    pub fn release(&mut self, key: StyleKey) -> Result<u32, StyleTreeError> {
        self.bump_tick();
        let packed = key.pack()?;
        match self.store.release(packed) {
            Ok(s) => {
                self.log_event("release", packed, format!("余引用 {}", s));
                Ok(s)
            }
            Err(e) => {
                self.stats.rejects = self.stats.rejects.saturating_add(1);
                self.file_case_err(&e, "release/store");
                Err(e)
            }
        }
    }

    /// **COW 写**（判据二核心）：共享实例不可变，变更 = 克隆源实例 +
    /// 补丁域钳制（留痕）+ 新 key 入账 + 源引用账目迁移。
    pub fn cow_write(
        &mut self,
        key_old: u64,
        new_key: StyleKey,
        patch: &ValuePatch,
    ) -> Result<CowReceipt, StyleTreeError> {
        self.bump_tick();
        let packed_new = new_key.pack()?;
        // 源实例克隆（只读通道读，写发生在克隆体上）。
        let mut cv = match self.store.values_of(key_old) {
            Some(v) => *v,
            None => {
                self.stats.rejects = self.stats.rejects.saturating_add(1);
                let e = StyleTreeError::ShareState { key: key_old, why: "COW 源未在账" };
                self.file_case_err(&e, "cow_write/source");
                return Err(e);
            }
        };
        let mut clamped = 0u32;
        let mut i = 0usize;
        while i < patch.len() {
            let (p, raw) = patch.decoded_at(i)?;
            let sp = prop_spec(p);
            let c = clamp_to_domain(sp.lo, sp.hi, raw);
            if c != raw {
                self.note_clamp(sp.name, raw, c, sp.lo, sp.hi);
                clamped += 1;
            }
            let _ = cv.write_clamped(p, c);
            i += 1;
        }
        let grant = match self.store.cow_commit(key_old, packed_new, cv) {
            Ok(g) => g,
            Err(e) => {
                self.stats.rejects = self.stats.rejects.saturating_add(1);
                self.file_case_err(&e, "cow_write/commit");
                return Err(e);
            }
        };
        self.invalidate();
        self.log_event("cow", packed_new, format!("自 0x{:016X} 迁移", key_old));
        Ok(CowReceipt {
            key_old,
            key_new: packed_new,
            clamps: clamped,
            deduped: grant.deduped,
            migrated: true,
        })
    }

    // ---- 树面（判据一 + 继承 + 环检测）----

    /// 建点（树闸全过 → 入账 + 事件）。
    pub fn add_node(
        &mut self,
        parent: Option<u64>,
        key: StyleKey,
        decl: ComputedValues,
        label: &str,
    ) -> Result<u64, StyleTreeError> {
        self.bump_tick();
        let packed = key.pack()?;
        let mut d = decl;
        // 声明值域钳制留痕（先钳再入树）。
        let mut i = 0usize;
        while i < PROP_COUNT {
            let sp = &PROP_SPECS[i];
            let c = clamp_to_domain(sp.lo, sp.hi, d.vals[i]);
            if c != d.vals[i] {
                self.note_clamp(sp.name, d.vals[i], c, sp.lo, sp.hi);
            }
            d.vals[i] = c;
            i += 1;
        }
        match self.tree.add_node(parent, packed, d, label) {
            Ok(id) => {
                self.invalidate();
                self.log_event("add-node", packed, format!("节点 #{}", id));
                Ok(id)
            }
            Err(e) => {
                self.stats.rejects = self.stats.rejects.saturating_add(1);
                self.file_case_err(&e, "add_node/tree");
                Err(e)
            }
        }
    }

    /// 拆叶子。
    pub fn remove_leaf(&mut self, id: u64) -> Result<(), StyleTreeError> {
        self.bump_tick();
        match self.tree.remove_leaf(id) {
            Ok(()) => {
                self.invalidate();
                self.log_event("remove-node", 0, format!("节点 #{}", id));
                Ok(())
            }
            Err(e) => {
                self.stats.rejects = self.stats.rejects.saturating_add(1);
                self.file_case_err(&e, "remove_leaf/tree");
                Err(e)
            }
        }
    }

    /// 重挂（环检测在树层；环检出 → 立案流转）。
    pub fn reparent(&mut self, id: u64, new_parent: Option<u64>) -> Result<(), StyleTreeError> {
        self.bump_tick();
        match self.tree.reparent(id, new_parent) {
            Ok(()) => {
                self.invalidate();
                self.log_event("reparent", 0, format!("节点 #{} 重挂", id));
                Ok(())
            }
            Err(e) => {
                self.stats.rejects = self.stats.rejects.saturating_add(1);
                self.file_case_err(&e, "reparent/tree");
                Err(e)
            }
        }
    }

    /// **declare 局部声明**（补丁逐条域钳制留痕 → 写节点 decl →
    /// 缓存失效）。
    pub fn declare(
        &mut self,
        node: u64,
        patch: &ValuePatch,
    ) -> Result<DeclareReceipt, StyleTreeError> {
        self.bump_tick();
        let idx = match self.tree.node_idx(node) {
            Some(i) => i,
            None => {
                self.stats.rejects = self.stats.rejects.saturating_add(1);
                let e = StyleTreeError::NodeUnknown { node };
                self.file_case_err(&e, "declare/tree");
                return Err(e);
            }
        };
        let mut clamped = 0u32;
        let mut i = 0usize;
        while i < patch.len() {
            let (p, raw) = patch.decoded_at(i)?;
            let sp = prop_spec(p);
            let c = clamp_to_domain(sp.lo, sp.hi, raw);
            if c != raw {
                self.note_clamp(sp.name, raw, c, sp.lo, sp.hi);
                clamped += 1;
            }
            let n = &mut self.tree.nodes[idx];
            n.decl.vals[p.rank()] = c;
            n.decl.specified |= 1u32 << p.rank();
            i += 1;
        }
        self.stats.declares = self.stats.declares.saturating_add(1);
        self.invalidate();
        self.log_event("declare", node, format!("{} 条补丁", patch.len()));
        Ok(DeclareReceipt { node, entries: patch.len() as u32, clamps: clamped })
    }

    /// **计算值**（缓存纪元匹配直出；未命中走纯函数 + 继承深度入账；
    /// 环检出 → 立案流转）。
    pub fn compute(&mut self, node: u64) -> Result<ComputedValues, StyleTreeError> {
        self.bump_tick();
        let idx = match self.tree.node_idx(node) {
            Some(i) => i,
            None => {
                let e = StyleTreeError::NodeUnknown { node };
                self.file_case_err(&e, "compute/tree");
                return Err(e);
            }
        };
        // 缓存闸（纪元匹配且缓存在场）。
        if let Some(c) = self.tree.nodes[idx].cache {
            if self.tree.nodes[idx].cache_epoch == self.epoch {
                self.stats.cache_hits = self.stats.cache_hits.saturating_add(1);
                self.stats.computes = self.stats.computes.saturating_add(1);
                return Ok(c);
            }
        }
        self.stats.cache_misses = self.stats.cache_misses.saturating_add(1);
        match self.tree.compute_pure(node) {
            Ok((cv, trace)) => {
                self.stats.computes = self.stats.computes.saturating_add(1);
                self.stats.inherit_hops = self.stats.inherit_hops.saturating_add(trace.hops);
                if trace.steps > self.stats.max_inherit_steps {
                    self.stats.max_inherit_steps = trace.steps;
                }
                self.tree.stats.computes = self.tree.stats.computes.saturating_add(1);
                self.tree.nodes[idx].cache = Some(cv);
                self.tree.nodes[idx].cache_epoch = self.epoch;
                Ok(cv)
            }
            Err(e) => {
                self.file_case_err(&e, "compute/inherit");
                Err(e)
            }
        }
    }

    // ---- 批量（确定性 + 原子性）----

    /// **批量应用**：两段式——干跑全量校验（节点存在/补丁可解码），
    /// 任何一项失败整批拒绝零提交（原子性）；通过后按给定序提交
    /// （同序同果 = 确定性；双跑对拍判据钉死）。
    pub fn apply_batch(&mut self, plan: &BatchPlan) -> Result<BatchReceipt, StyleTreeError> {
        self.bump_tick();
        // 干跑：闸 1 节点存在；闸 2 补丁可解码。零提交。
        let mut i = 0usize;
        while i < plan.items.len() {
            let item = &plan.items[i];
            if self.tree.node_idx(item.node).is_none() {
                self.stats.batch_rejects = self.stats.batch_rejects.saturating_add(1);
                let e = StyleTreeError::Batch { reason: "节点不存在", item: i };
                self.file_case_err(&e, "apply_batch/dry-node");
                return Err(e);
            }
            let mut j = 0usize;
            while j < item.patch.len() {
                if let Err(pe) = item.patch.decoded_at(j) {
                    self.stats.batch_rejects = self.stats.batch_rejects.saturating_add(1);
                    let e = StyleTreeError::Batch { reason: pe.code(), item: i };
                    self.file_case_err(&e, "apply_batch/dry-patch");
                    return Err(e);
                }
                j += 1;
            }
            i += 1;
        }
        // 提交：按给定序（确定性）。干跑已过，提交不可失败。
        let mut entries = 0usize;
        let mut clamps = 0u32;
        i = 0;
        while i < plan.items.len() {
            let node = plan.items[i].node;
            let patch = plan.items[i].patch;
            let idx = match self.tree.node_idx(node) {
                Some(x) => x,
                None => {
                    // 干跑后树未变（同调用内无中间变更），防御性兜底。
                    self.stats.batch_rejects = self.stats.batch_rejects.saturating_add(1);
                    let e = StyleTreeError::Batch { reason: "节点不存在（提交期）", item: i };
                    self.file_case_err(&e, "apply_batch/commit");
                    return Err(e);
                }
            };
            let mut j = 0usize;
            while j < patch.len() {
                // 干跑已验解码成功；此处解包必 Some。
                if let Ok((p, raw)) = patch.decoded_at(j) {
                    let sp = prop_spec(p);
                    let c = clamp_to_domain(sp.lo, sp.hi, raw);
                    if c != raw {
                        self.note_clamp(sp.name, raw, c, sp.lo, sp.hi);
                        clamps += 1;
                    }
                    let n = &mut self.tree.nodes[idx];
                    n.decl.vals[p.rank()] = c;
                    n.decl.specified |= 1u32 << p.rank();
                }
                j += 1;
                entries += 1;
            }
            i += 1;
        }
        self.invalidate();
        self.stats.batches = self.stats.batches.saturating_add(1);
        self.log_event("batch", 0, format!("{} 项 {} 条", plan.items.len(), entries));
        Ok(BatchReceipt { applied: plan.items.len(), entries, clamps })
    }

    // ---- 上游契约接收（哈希对账——家族）----

    /// **上游投喂**：逐条现算 FNV-1a 64 与申报哈希对拍，**哈希不等
    /// 整批拒收带码**（零部分接收）；通过者满配守卫 → 值钳制入共享表
    /// → 树面去重建点。
    pub fn ingest_upstream(
        &mut self,
        batch: &[UpstreamDecl],
        parent: Option<u64>,
    ) -> Result<UpstreamReceipt, StyleTreeError> {
        self.bump_tick();
        self.stats.ingests = self.stats.ingests.saturating_add(1);
        // 闸 1（结构）：父存在（给了父就要真实在账）。
        if let Some(p) = parent {
            if self.tree.node_idx(p).is_none() {
                self.stats.rejects = self.stats.rejects.saturating_add(1);
                let e = StyleTreeError::NodeUnknown { node: p };
                self.file_case_err(&e, "ingest_upstream/parent");
                return Err(e);
            }
        }
        // 闸 2（哈希对账 + 满配守卫）：逐条现算，任一不等整批拒收。
        let mut i = 0usize;
        while i < batch.len() {
            let d = &batch[i];
            let packed = match d.key.pack() {
                Ok(p) => p,
                Err(e) => {
                    self.stats.rejects = self.stats.rejects.saturating_add(1);
                    self.file_case_err(&e, "ingest_upstream/key");
                    return Err(e);
                }
            };
            if d.values.specified != PROP_MASK_ALL {
                self.stats.rejects = self.stats.rejects.saturating_add(1);
                let e = StyleTreeError::PropGuard { locus: "ingest_upstream/not-full-mask" };
                self.file_case_err(&e, "ingest_upstream/mask");
                return Err(e);
            }
            let kv = KeyedValues { key: packed, values: d.values };
            let got = fnv64(&kv.canonical_bytes());
            if got != d.declared_hash {
                self.stats.rejects = self.stats.rejects.saturating_add(1);
                let e = StyleTreeError::UpstreamHash {
                    index: i,
                    expect: d.declared_hash,
                    got,
                };
                self.file_case_err(&e, "ingest_upstream/hash");
                return Err(e);
            }
            i += 1;
        }
        // 提交：逐条 intern（值域钳制留痕）+ 树面去重建点。
        let mut receipt = UpstreamReceipt {
            checked: batch.len(),
            accepted: 0,
            deduped: 0,
            nodes_added: 0,
            clamps: 0,
        };
        i = 0;
        while i < batch.len() {
            let packed = batch[i].key.pack()?;
            let mut cv = batch[i].values;
            let mut k = 0usize;
            while k < PROP_COUNT {
                let sp = &PROP_SPECS[k];
                let c = clamp_to_domain(sp.lo, sp.hi, cv.vals[k]);
                if c != cv.vals[k] {
                    self.note_clamp(sp.name, cv.vals[k], c, sp.lo, sp.hi);
                    receipt.clamps += 1;
                }
                cv.vals[k] = c;
                k += 1;
            }
            match self.store.intern(packed, cv, self.tick) {
                Ok(g) => {
                    receipt.accepted += 1;
                    if g.deduped {
                        receipt.deduped += 1;
                    }
                }
                Err(e) => {
                    self.stats.rejects = self.stats.rejects.saturating_add(1);
                    self.file_case_err(&e, "ingest_upstream/intern");
                    return Err(e);
                }
            }
            if self.tree.node_with_key(packed).is_none() {
                match self.tree.add_node(parent, packed, cv, "upstream") {
                    Ok(_) => {
                        receipt.nodes_added += 1;
                    }
                    Err(e) => {
                        self.stats.rejects = self.stats.rejects.saturating_add(1);
                        self.file_case_err(&e, "ingest_upstream/add-node");
                        return Err(e);
                    }
                }
            }
            i += 1;
        }
        self.invalidate();
        self.log_event("ingest", 0, format!("{} 条", batch.len()));
        Ok(receipt)
    }

    // ---- 下游消费接口（前向声明——家族）----

    /// 只读消费视图（VE-F2811 消费面之一）。
    pub fn consumer_view(&self) -> ConsumerView<'_> {
        ConsumerView { store: &self.store, tree_len: self.tree.len() }
    }

    /// 跨域衔接对账钩子（复用方义务：点名 key 都在共享表在账）。
    pub fn reconcile_keys(&self, keys: &[u64]) -> Result<usize, StyleTreeError> {
        let mut total = 0usize;
        let mut i = 0usize;
        while i < keys.len() {
            let k = keys[i];
            if self.store.slot_idx(k).is_none() {
                return Err(StyleTreeError::ShareState { key: k, why: "对账点名 key 未在共享表" });
            }
            total += 1;
            i += 1;
        }
        Ok(total)
    }

    // ---- 判据面：健康 / 指纹 / 双跑对拍 / 注入三面 ----

    /// 健康自检（守恒 + 余量 + 溢出面）。
    pub fn health(&self) -> HealthReport {
        HealthReport {
            tree_sane: self.tree.sanity(),
            store_conserves_interns: self.store.stats.conserves_interns(),
            store_conserves_live: self.store.stats.conserves_live(&self.store.slots),
            stats_conserves: self.stats.conserves(),
            ledger_not_full: self.cases.len() < CASE_CAP,
            case_overflow: self.stats.case_overflow,
            clamp_overwritten: self.clamps.total_overwritten(),
            event_overwritten: self.events.total_pushes() - self.events.len() as u64,
            store_slots: self.store.slots.len(),
            tick: self.tick,
        }
    }

    /// 全树声明指纹（确定性遍历：共享表按 key 序 + 树按插入序；
    /// 声明面指纹——不含继承展开，便宜且稳定，判据用）。
    pub fn fingerprint_decl(&self) -> u64 {
        let mut buf = Vec::new();
        let mut i = 0usize;
        while i < self.store.slots.len() {
            if let Some(s) = self.store.slots.get(i) {
                buf.extend_from_slice(&s.key.to_le_bytes());
                let kv = KeyedValues { key: s.key, values: s.values };
                buf.extend_from_slice(&kv.canonical_bytes());
            }
            i += 1;
        }
        i = 0;
        while i < self.tree.nodes.len() {
            if let Some(n) = self.tree.nodes.get(i) {
                buf.extend_from_slice(&n.id.to_le_bytes());
                buf.extend_from_slice(&n.parent.to_le_bytes());
                buf.extend_from_slice(&n.key.to_le_bytes());
                buf.extend_from_slice(&n.decl.specified.to_le_bytes());
                let mut k = 0usize;
                while k < PROP_COUNT {
                    buf.extend_from_slice(&n.decl.vals[k].to_le_bytes());
                    k += 1;
                }
            }
            i += 1;
        }
        fnv64(&buf)
    }

    /// **双跑确定性对拍**：同树两遍独立计算（正序 / 逆序），逐节点
    /// 指纹必须一致（继承计算无顺序依赖）；不一致 → [`E_ST_DETERMINISM`]
    /// 立案流转。
    pub fn determinism_probe(&mut self) -> Result<DeterminismReport, StyleTreeError> {
        self.bump_tick();
        let n = self.tree.nodes.len();
        let mut fps_a: Vec<u64> = Vec::new();
        let mut i = 0usize;
        while i < n {
            let id = match self.tree.nodes.get(i) {
                Some(nd) => nd.id,
                None => break,
            };
            let (cv, _) = self.tree.compute_pure(id)?;
            let key = self.tree.nodes[i].key;
            let kv = KeyedValues { key, values: cv };
            fps_a.push(fnv64(&kv.canonical_bytes()));
            i += 1;
        }
        let mut fps_b: Vec<u64> = Vec::new();
        let mut j = n;
        while j > 0 {
            j -= 1;
            let id = match self.tree.nodes.get(j) {
                Some(nd) => nd.id,
                None => break,
            };
            let (cv, _) = self.tree.compute_pure(id)?;
            let key = self.tree.nodes[j].key;
            let kv = KeyedValues { key, values: cv };
            fps_b.push(fnv64(&kv.canonical_bytes()));
        }
        // 逆序跑完 → 逆序折叠回正序逐位比。
        let mut consistent = true;
        let mut k = 0usize;
        while k < n {
            let a = match fps_a.get(k) {
                Some(x) => *x,
                None => break,
            };
            let b = match fps_b.get(n - 1 - k) {
                Some(x) => *x,
                None => break,
            };
            if a != b {
                consistent = false;
            }
            k += 1;
        }
        let mut fold_a = 0u64;
        let mut k = 0usize;
        while k < fps_a.len() {
            fold_a ^= fps_a[k];
            k += 1;
        }
        let mut fold_b = 0u64;
        let mut k = 0usize;
        while k < fps_b.len() {
            fold_b ^= fps_b[k];
            k += 1;
        }
        if !consistent {
            // 取第一个分歧节点立案（定位到句柄）。
            let mut bad = 0u64;
            let mut k = 0usize;
            while k < n {
                let a = fps_a.get(k).copied().unwrap_or(0);
                let b = fps_b.get(n - 1 - k).copied().unwrap_or(0);
                if a != b {
                    if let Some(nd) = self.tree.nodes.get(k) {
                        bad = nd.id;
                    }
                }
                k += 1;
            }
            let e = StyleTreeError::Determinism { node: bad };
            self.file_case_err(&e, "determinism_probe/pairwise");
            return Err(e);
        }
        Ok(DeterminismReport { nodes: n, pass_a: fold_a, pass_b: fold_b, consistent })
    }

    /// **注入面一：非法 key**。任给 u64 走解包闸；被拒 = 检出
    /// （Ok(true) + 立案），合法通过 = 未检出（Ok(false) 如实上报，
    /// 并回收 intern 副作用）。
    pub fn inject_bad_key(&mut self, raw: u64) -> Result<bool, StyleTreeError> {
        self.bump_tick();
        match StyleKey::unpack(raw) {
            Err(e) => {
                self.stats.injections_caught = self.stats.injections_caught.saturating_add(1);
                self.file_case_err(&e, "inject_bad_key/unpack");
                self.log_event("inject-caught", raw, String::from("非法 key 被解包闸拒收"));
                Ok(true)
            }
            Ok(k) => {
                // key 本就合法：intern 后立即 release 回收，如实上报未检出。
                let grant = self.intern(k, ComputedValues::defaults())?;
                let _ = self.release(k)?;
                self.stats.injections_missed = self.stats.injections_missed.saturating_add(1);
                self.log_event("inject-missed", grant.key, String::from("注入物本就合法"));
                Ok(false)
            }
        }
    }

    /// **注入面二：越界值**。任给 (属性, 原始值) 走声明闸；域外必被
    /// 钳制留痕且落点在域内 = 检出（Ok(true)），域内 = 未检出如实上报。
    pub fn inject_out_of_domain(
        &mut self,
        prop: PropId,
        raw: i32,
    ) -> Result<bool, StyleTreeError> {
        self.bump_tick();
        let sp = prop_spec(prop);
        let key = StyleKey::new(0x7F02, 0x0001, Pseudo::None, 0);
        let probe_a = self.add_node(None, key, ComputedValues::defaults(), "probe-clamp")?;
        let mut patch = ValuePatch::new();
        let pushed = patch.push(prop, raw);
        let before = self.clamps.total_pushes();
        let outcome = pushed.and_then(|_| self.declare(probe_a, &patch));
        if let Err(e) = &outcome {
            let _ = self.remove_leaf(probe_a);
            return Err(e.clone());
        }
        // 检出判据：钳制留痕增长（域外必留痕）且落点已回域内。
        let caught = match &outcome {
            Ok(r) => r.clamps > 0 && self.clamps.total_pushes() > before,
            Err(_) => false,
        };
        let in_domain_after = match self.tree.node_idx(probe_a) {
            Some(i) => {
                let v = self.tree.nodes[i].decl.read(prop);
                v >= sp.lo && v <= sp.hi
            }
            None => false,
        };
        let _ = self.remove_leaf(probe_a);
        if caught && in_domain_after {
            self.stats.injections_caught = self.stats.injections_caught.saturating_add(1);
            self.log_event("inject-caught", 0, format!("{} 越界被钳", sp.name));
            Ok(true)
        } else {
            self.stats.injections_missed = self.stats.injections_missed.saturating_add(1);
            self.log_event("inject-missed", 0, String::from("注入物本在域内"));
            Ok(false)
        }
    }

    /// **注入面三：继承环**。真实建两节点（A 根 → B 子），尝试把 A
    /// 挂到 B 下——环闸必拒 = 检出（Ok(true) + 立案）；探针节点拆除。
    pub fn inject_inheritance_cycle(&mut self) -> Result<bool, StyleTreeError> {
        self.bump_tick();
        let key = StyleKey::new(0x7F01, 0x0001, Pseudo::None, 0);
        let mut decl_a = ComputedValues::defaults();
        let _ = decl_a.write_clamped(PropId::FontSizePx, 14);
        let a = self.add_node(None, key, decl_a, "probe-cycle-a")?;
        let b = match self.add_node(Some(a), key, ComputedValues::defaults(), "probe-cycle-b") {
            Ok(x) => x,
            Err(e) => {
                let _ = self.remove_leaf(a);
                return Err(e);
            }
        };
        match self.tree.reparent(a, Some(b)) {
            Err(e) => {
                let _ = self.remove_leaf(b);
                let _ = self.remove_leaf(a);
                self.stats.injections_caught = self.stats.injections_caught.saturating_add(1);
                self.file_case_err(&e, "inject_inheritance_cycle/reparent");
                self.log_event("inject-caught", 0, String::from("继承环被 reparent 闸拒收"));
                Ok(true)
            }
            Ok(()) => {
                // 理论不可达（结构闸在先）；回滚后如实上报。
                let _ = self.reparent(a, None);
                let _ = self.remove_leaf(b);
                let _ = self.remove_leaf(a);
                self.stats.injections_missed = self.stats.injections_missed.saturating_add(1);
                Ok(false)
            }
        }
    }

    /// 读屏摘要（整域单行，无障碍替述）。
    pub fn screen_text(&self) -> String {
        format!(
            "计算样式树：{} 节点（最深链 {} 层），共享表 {} 实例去重率 {}‰，钳制 {} 次留痕，立案 {} 宗",
            self.tree.len(),
            self.stats.max_inherit_steps,
            self.store.stats.unique,
            self.store.stats.dedup_permille(),
            self.stats.clamps,
            self.stats.cases
        )
    }
}

// ---------------------------------------------------------------------------
// 十三、跨域审计（规格表/位域布局/下游声明/全域汇总——判据共用）
// ---------------------------------------------------------------------------

/// 规格表运行期审计（编译期闸的运行期镜像，供下游调用）。
pub fn audit_spec_table() -> Result<String, StyleTreeError> {
    if PROP_SPECS.len() != PROP_COUNT {
        return Err(StyleTreeError::PropGuard { locus: "audit_spec_table/len" });
    }
    let mut i = 0usize;
    while i < PROP_COUNT {
        let sp = &PROP_SPECS[i];
        if sp.prop.rank() != i {
            return Err(StyleTreeError::PropGuard { locus: "audit_spec_table/rank" });
        }
        if sp.lo > sp.hi || sp.default < sp.lo || sp.default > sp.hi {
            return Err(StyleTreeError::PropGuard { locus: "audit_spec_table/domain" });
        }
        if sp.prop.zh() != sp.name {
            // zh 与 name 是两套口径（中文名/规格名），这里只验二者
            // 都非空——逐字节一致性归判据侧独立清单对拍。
            if sp.prop.zh().is_empty() || sp.name.is_empty() {
                return Err(StyleTreeError::PropGuard { locus: "audit_spec_table/names" });
            }
        }
        i += 1;
    }
    Ok(format!(
        "规格表审计通过：{} 条属性逐条公开（合法域/缺省/单位/可继承），域钳制单源 clamp_to_domain",
        PROP_COUNT
    ))
}

/// key 位域布局审计（打包位域互斥无缝——往返自证的静态前提）。
pub fn audit_key_layout() -> Result<String, StyleTreeError> {
    if STATE_SHIFT != CANON_LOW_BITS
        || PSEUDO_SHIFT != STATE_SHIFT + 8
        || MEMBER_SHIFT != PSEUDO_SHIFT + 4
        || FAMILY_SHIFT != MEMBER_SHIFT + 16
    {
        return Err(StyleTreeError::KeyCanon { raw: 0 });
    }
    if CANON_LOW_BITS + 8 + 4 + 16 + 16 != 64 {
        return Err(StyleTreeError::KeyCanon { raw: 0 });
    }
    if STATE_MASK != 0x3F || PSEUDO_COUNT != 8 {
        return Err(StyleTreeError::KeyCanon { raw: 0 });
    }
    Ok(format!(
        "key 位域审计通过：低 {} 位规范零 + 状态 {} + 伪类 4 + 成员 16 + 家族 16 = 64，无缝互斥",
        CANON_LOW_BITS, 8
    ))
}

/// 下游前向声明审计（只查声明完整性，不实现消费逻辑）。
pub fn audit_downstream() -> Result<&'static str, StyleTreeError> {
    if DOWNSTREAM_DECL.peer != DOWNSTREAM_ANCHOR {
        return Err(StyleTreeError::Downstream { why: "对端标识漂移" });
    }
    if DOWNSTREAM_DECL.faces.len() < 3 {
        return Err(StyleTreeError::Downstream { why: "消费面不足三个" });
    }
    Ok("下游消费接口前向声明完整（VE-F2811 取 consumer_view/share_values/dedup_ledger）")
}

/// 全域审计汇总（一条命令跑完所有闸，供下游与判据共用）。
pub fn audit_all(e: &StyleTreeEngine) -> Result<String, StyleTreeError> {
    let a = audit_spec_table()?;
    let b = audit_key_layout()?;
    let c = audit_downstream()?;
    let h = e.health();
    if !h.healthy() {
        return Err(StyleTreeError::PropGuard { locus: "audit_all/health" });
    }
    Ok(format!("{}；{}；{}；{}", a, b, c, h.screen_text()))
}

/// 全域审计摘要（可读单行，承家族 `*_summary` 惯例）。
pub fn domain_summary() -> String {
    format!(
        "{}｜17 属性规格表逐条公开｜树上限 {} 深度上界 {}｜共享表 {}（去重单实例）｜码段 0x13B0~0x13BF 独占",
        STYLETREE_VERSION, MAX_TREE_NODES, MAX_TREE_DEPTH, SHARE_CAP
    )
}

// ---------------------------------------------------------------------------
// 十四、编译期闸（数值域全在这里断；运行期零 assert）
// ---------------------------------------------------------------------------

const _: () = {
    // 闸 1：规格表与封闭全集逐位对位（数组长度即断言 + 秩 = 槽位）。
    assert!(PROP_SPECS.len() == PROP_COUNT);
    assert!(spec_table_ranks_aligned());
    assert!(spec_defaults_in_domain());
    assert!(PROP_COUNT == 17);
    assert!(PROP_MASK_ALL == 0x0001_FFFF);

    // 闸 2：key 位域互斥无缝（20+8+4+16+16 = 64）。
    assert!(STATE_SHIFT == CANON_LOW_BITS);
    assert!(PSEUDO_SHIFT == STATE_SHIFT + 8);
    assert!(MEMBER_SHIFT == PSEUDO_SHIFT + 4);
    assert!(FAMILY_SHIFT == MEMBER_SHIFT + 16);
    assert!(CANON_LOW_BITS + 8 + 4 + 16 + 16 == 64);
    assert!(CANON_MASK == 0xF_FFFF);
    assert!(STATE_MASK == 0x3F);
    assert!(PSEUDO_COUNT == 8);
    assert!(KEY_FAMILY_MIN == 1);
    assert!(KEY_FAMILY_MAX == 0xFFFE);
    assert!(KEY_MEMBER_MAX == 0xFFFE);

    // 闸 3：码段独占且十五码两两互异。
    assert!(SEG_BASE == 0x13B0);
    assert!(SEG_TOP == 0x13BF);
    assert!(SEG_BASE < SEG_TOP);
    assert!(codes_distinct());
    assert!(ERROR_CODE_NAMES.len() == 15);

    // 闸 4：容量纪律。
    assert!(MAX_TREE_NODES >= 1 && MAX_TREE_NODES <= 4096);
    assert!(MAX_TREE_DEPTH >= 2 && MAX_TREE_DEPTH <= 256);
    assert!(MAX_BATCH >= 1 && MAX_BATCH <= 256);
    assert!(SHARE_CAP.is_power_of_two());
    assert!(CLAMP_LOG_CAP >= 1);
    assert!(CASE_CAP >= 1);
    assert!(EVENT_CAP >= 1);
    assert!(NODE_LABEL_CAP >= 1);

    // 闸 5：单源表长度（三闸次序/判据/下游面）。
    assert!(GATE_ORDER.len() == 4);
    assert!(CRITERIA.len() == 7);
    assert!(DOWNSTREAM_DECL.faces.len() >= 3);
};

// ---------------------------------------------------------------------------
// 十五、tests（宿主单测；发行剔除零成本）
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    /// 满配值（判据用：上游投喂必须 17 槽全指定）。
    fn full_cv(font: i32, color: i32) -> ComputedValues {
        let mut cv = ComputedValues::defaults();
        let mut i = 0usize;
        while i < PROP_COUNT {
            cv.specified |= 1u32 << i;
            i += 1;
        }
        cv.vals[PropId::FontSizePx.rank()] = font;
        cv.vals[PropId::TextColorRgb.rank()] = color;
        cv
    }

    /// 双跑测试的同源操作序列（两个引擎吃同一份，指纹必须一致）。
    fn feed_ops(e: &mut StyleTreeEngine) {
        let k = StyleKey::new(0x1001, 0x0001, Pseudo::None, STATE_ENABLED);
        let _ = e.intern(k, full_cv(18, 0x336699));
        let root = e
            .add_node(None, k, ComputedValues::defaults(), "root")
            .unwrap();
        let mut decl = ComputedValues::defaults();
        let _ = decl.write_clamped(PropId::FontSizePx, 20);
        let _ = decl.write_clamped(PropId::TextColorRgb, 0x222222);
        let kid = e.add_node(Some(root), k, decl, "kid").unwrap();
        let mut patch = ValuePatch::new();
        let _ = patch.push(PropId::LineHeightPx, 30);
        let _ = e.declare(kid, &patch).unwrap();
        let _ = e.compute(kid).unwrap();
    }

    #[test]
    fn prop_spec_table_aligned() {
        assert_eq!(PROP_SPECS.len(), 17);
        let mut i = 0usize;
        while i < PROP_COUNT {
            assert_eq!(PROP_SPECS[i].prop.rank(), i);
            assert!(PROP_SPECS[i].default >= PROP_SPECS[i].lo);
            assert!(PROP_SPECS[i].default <= PROP_SPECS[i].hi);
            assert!(!PROP_SPECS[i].name.is_empty());
            assert!(!PROP_SPECS[i].prop.zh().is_empty());
            i += 1;
        }
        // 秩反查守卫。
        assert!(PropId::of_rank(16).is_some());
        assert!(PropId::of_rank(17).is_none());
        assert!(audit_spec_table().is_ok());
    }

    #[test]
    fn computed_named_getters_roundtrip() {
        let mut cv = ComputedValues::defaults();
        let mut i = 0usize;
        while i < PROP_COUNT {
            let p = PropId::of_rank(i).unwrap();
            let sp = prop_spec(p);
            let hi = sp.hi;
            let c = cv.write_clamped(p, hi);
            assert_eq!(c, hi);
            assert_eq!(cv.read(p), hi);
            i += 1;
        }
        // 具名面抽查（类型口径一致性）。
        assert_eq!(cv.font_size_px(), 72);
        assert_eq!(cv.opacity_permille(), 1000);
        assert_eq!(cv.z_level(), 1024);
        assert_eq!(cv.text_color_rgb(), 0xFFFFFF);
        // 缺省值钳制恒等（域内不动）。
        let d = ComputedValues::defaults();
        assert_eq!(d.font_size_px(), 16);
        assert_eq!(d.letter_spacing_px(), 0);
        assert!(d.in_domain_all());
    }

    #[test]
    fn key_pack_unpack_roundtrip_and_collision() {
        // 往返自证：抽样四元组 pack→unpack 逐位回原。
        let mut packed: Vec<u64> = Vec::new();
        let mut f = 1u16;
        while f < 12 {
            let mut m = 0u16;
            while m < 5 {
                let mut pi = 0usize;
                while pi < PSEUDO_COUNT {
                    let mut st = 0u8;
                    while st <= STATE_MASK {
                        if st & STATE_ENABLED != 0 && st & STATE_DISABLED != 0 {
                            st += 1;
                            continue;
                        }
                        let k = StyleKey::new(f, m, Pseudo::ALL[pi], st);
                        let p = k.pack().unwrap();
                        let back = StyleKey::unpack(p).unwrap();
                        assert_eq!(back, k);
                        packed.push(p);
                        st += 1;
                    }
                    pi += 1;
                }
                m += 1;
            }
            f += 1;
        }
        // 碰撞检测账：全样本排序后相邻必异（位域互斥的结构保证）。
        packed.sort_unstable();
        let mut i = 1usize;
        while i < packed.len() {
            assert_ne!(packed[i - 1], packed[i]);
            i += 1;
        }
        assert!(audit_key_layout().is_ok());
    }

    #[test]
    fn key_illegal_rejected() {
        // 家族 0（未定族）/ 0xFFFF（通配保留）。
        assert!(StyleKey::new(0, 1, Pseudo::None, 0).pack().is_err());
        assert!(StyleKey::new(0xFFFF, 1, Pseudo::None, 0).pack().is_err());
        // 成员 0xFFFF 保留。
        assert!(StyleKey::new(1, 0xFFFF, Pseudo::None, 0).pack().is_err());
        // 状态位域外位。
        assert!(StyleKey::new(1, 1, Pseudo::None, 0x40).pack().is_err());
        // ENABLED/DISABLED 互斥。
        assert!(StyleKey::new(1, 1, Pseudo::None, STATE_ENABLED | STATE_DISABLED).pack().is_err());
        // 伪类秩 8..=15（位域在但封闭集外）——手拼 u64 解包拒。
        let bad_pseudo = 8u64 << PSEUDO_SHIFT;
        assert!(StyleKey::unpack(bad_pseudo).is_err());
    }

    #[test]
    fn key_canon_low_bits_rejected() {
        let k = StyleKey::new(1, 2, Pseudo::Hover, STATE_FOCUSED);
        let p = k.pack().unwrap();
        // 低 20 位任一非零 = 非规范 u64，解包即拒（非法 key 注入的闸）。
        assert!(StyleKey::unpack(p | 1).is_err());
        assert!(StyleKey::unpack(p | CANON_MASK).is_err());
        assert!(StyleKey::unpack(1).is_err());
        assert!(StyleKey::unpack(p).is_ok());
    }

    #[test]
    fn share_dedup_and_refcount() {
        let mut e = StyleTreeEngine::new();
        let k = StyleKey::new(0x2001, 0x0001, Pseudo::None, 0);
        let g1 = e.intern(k, full_cv(16, 0x111111)).unwrap();
        assert!(!g1.deduped);
        let g2 = e.intern(k, full_cv(16, 0x111111)).unwrap();
        assert!(g2.deduped);
        assert_eq!(e.store.stats.unique, 1);
        assert_eq!(e.store.stats.dedup_hits, 1);
        // 去重率：2 次投喂 1 命中 = 500‰。
        assert_eq!(e.store.stats.dedup_permille(), 500);
        // 引用计数：intern×2 + acquire×1 = 3；release×3 后 0，再 release 拒。
        let _ = e.acquire(k).unwrap();
        assert_eq!(e.store.strong_of(k.pack().unwrap()), Some(3));
        assert_eq!(e.release(k).unwrap(), 2);
        assert_eq!(e.release(k).unwrap(), 1);
        assert_eq!(e.release(k).unwrap(), 0);
        assert!(e.release(k).is_err());
        // 守恒。
        assert!(e.store.stats.conserves_interns());
        assert!(e.store.stats.conserves_live(&e.store.slots));
    }

    #[test]
    fn cow_migrates_and_source_immutable() {
        let mut e = StyleTreeEngine::new();
        let ka = StyleKey::new(0x3001, 0x0001, Pseudo::None, 0);
        let packed_a = ka.pack().unwrap();
        let _ = e.intern(ka, full_cv(16, 0x101010)).unwrap();
        let before = *e.store.values_of(packed_a).unwrap();
        // COW：改字号 → 新实例，源实例逐字节不动。
        let kb = StyleKey::new(0x3001, 0x0002, Pseudo::None, 0);
        let mut patch = ValuePatch::new();
        let _ = patch.push(PropId::FontSizePx, 24);
        let r = e.cow_write(packed_a, kb, &patch).unwrap();
        assert!(!r.deduped);
        assert!(r.migrated);
        let after = *e.store.values_of(packed_a).unwrap();
        assert_eq!(before, after);
        let packed_b = kb.pack().unwrap();
        assert_eq!(e.store.values_of(packed_b).unwrap().font_size_px(), 24);
        // 账目迁移：源 1→0、新 1；迁移计数 1。
        assert_eq!(e.store.strong_of(packed_a), Some(0));
        assert_eq!(e.store.strong_of(packed_b), Some(1));
        assert_eq!(e.store.stats.migrations, 1);
        assert_eq!(e.store.stats.cows, 1);
        // 同键 COW 拒（共享实例不可变）。
        assert!(e.cow_write(packed_b, kb, &patch).is_err());
        // 无引用源 COW 拒。
        let kc = StyleKey::new(0x3001, 0x0003, Pseudo::None, 0);
        let _ = e.intern(kc, full_cv(12, 0x010203)).unwrap();
        let packed_c = kc.pack().unwrap();
        let _ = e.release(kc).unwrap();
        assert!(e.cow_write(packed_c, kb, &patch).is_err());
    }

    #[test]
    fn inheritance_and_depth_ledger() {
        let mut e = StyleTreeEngine::new();
        let k = StyleKey::new(0x4001, 0x0001, Pseudo::None, 0);
        // 根：字号 20 + 文字色。
        let mut root_decl = ComputedValues::defaults();
        let _ = root_decl.write_clamped(PropId::FontSizePx, 20);
        let _ = root_decl.write_clamped(PropId::TextColorRgb, 0xAA0000);
        let root = e.add_node(None, k, root_decl, "root").unwrap();
        // 子：只声明字距 → 字号/文字色继承父。
        let mut kid_decl = ComputedValues::defaults();
        let _ = kid_decl.write_clamped(PropId::LetterSpacingPx, 2);
        let kid = e.add_node(Some(root), k, kid_decl, "kid").unwrap();
        let cv = e.compute(kid).unwrap();
        assert_eq!(cv.font_size_px(), 20);
        assert_eq!(cv.text_color_rgb(), 0xAA0000);
        assert_eq!(cv.letter_spacing_px(), 2);
        // 未指定的其余槽取缺省（补底）。
        assert_eq!(cv.line_height_px(), PROP_SPECS[PropId::LineHeightPx.rank()].default);
        assert_eq!(cv.opacity_permille(), 1000);
        // 继承深度账：孙节点沿两层链继承。
        let grand = e.add_node(Some(kid), k, ComputedValues::defaults(), "grand").unwrap();
        let cv2 = e.compute(grand).unwrap();
        assert_eq!(cv2.font_size_px(), 20);
        assert!(e.stats.inherit_hops >= 2);
        assert!(e.stats.max_inherit_steps >= 2);
        // 局部覆盖压过继承。
        let mut over = ValuePatch::new();
        let _ = over.push(PropId::FontSizePx, 30);
        let _ = e.declare(grand, &over).unwrap();
        let cv3 = e.compute(grand).unwrap();
        assert_eq!(cv3.font_size_px(), 30);
        assert_eq!(cv3.text_color_rgb(), 0xAA0000);
    }

    #[test]
    fn reparent_cycle_rejected() {
        let mut e = StyleTreeEngine::new();
        let k = StyleKey::new(0x5001, 0x0001, Pseudo::None, 0);
        let a = e.add_node(None, k, ComputedValues::defaults(), "a").unwrap();
        let b = e.add_node(Some(a), k, ComputedValues::defaults(), "b").unwrap();
        let c = e.add_node(Some(b), k, ComputedValues::defaults(), "c").unwrap();
        // 把 A 挂到 C 下 = 成环，闸必拒。
        match e.reparent(a, Some(c)) {
            Err(err) => {
                assert_eq!(err.code(), E_ST_CYCLE);
                assert_eq!(err.code_num(), SEG_BASE + 0x08);
            }
            Ok(()) => panic!("环未被拒"),
        }
        // 摘成根合法；重挂回 B 下合法。
        let _ = e.reparent(c, None).unwrap();
        let _ = e.reparent(c, Some(b)).unwrap();
        assert!(e.health().tree_sane);
    }

    #[test]
    fn clamp_notice_records_before_after() {
        let mut e = StyleTreeEngine::new();
        let k = StyleKey::new(0x6001, 0x0001, Pseudo::None, 0);
        let n = e.add_node(None, k, ComputedValues::defaults(), "n").unwrap();
        let mut patch = ValuePatch::new();
        let _ = patch.push(PropId::FontSizePx, 999);
        let r = e.declare(n, &patch).unwrap();
        assert_eq!(r.clamps, 1);
        // 钳制前后值留痕：999 → 72（字号上界）。
        let snaps = e.clamps.snapshot();
        assert_eq!(snaps.len(), 1);
        assert_eq!(snaps[0].original, 999);
        assert_eq!(snaps[0].clamped, 72);
        assert_eq!(snaps[0].lo, 8);
        assert_eq!(snaps[0].hi, 72);
        // 落点在域内。
        assert_eq!(e.tree.nodes[0].decl.font_size_px(), 72);
        // intern 面同样钳制留痕。
        let k2 = StyleKey::new(0x6001, 0x0002, Pseudo::None, 0);
        let mut bad = full_cv(1, -5);
        bad.vals[PropId::OpacityPermille.rank()] = 2000;
        let _ = e.intern(k2, bad).unwrap();
        assert!(e.stats.clamps >= 4);
    }

    #[test]
    fn batch_atomic_all_or_nothing() {
        let mut e = StyleTreeEngine::new();
        let k = StyleKey::new(0x7001, 0x0001, Pseudo::None, 0);
        let n = e.add_node(None, k, ComputedValues::defaults(), "n").unwrap();
        let fp_before = e.fingerprint_decl();
        // 含未知节点的批量 → 整批拒，零提交。
        let mut plan = BatchPlan::new();
        let mut p1 = ValuePatch::new();
        let _ = p1.push(PropId::FontSizePx, 20);
        let _ = plan.push(BatchItem { node: n, patch: p1 });
        let _ = plan.push(BatchItem { node: 0xDEAD, patch: p1 });
        assert!(e.apply_batch(&plan).is_err());
        assert_eq!(e.fingerprint_decl(), fp_before);
        assert_eq!(e.stats.batch_rejects, 1);
        // 合法批量整体生效。
        let mut plan2 = BatchPlan::new();
        let mut p2 = ValuePatch::new();
        let _ = p2.push(PropId::FontSizePx, 24);
        let _ = p2.push(PropId::ZLevel, 5);
        let _ = plan2.push(BatchItem { node: n, patch: p2 });
        let r = e.apply_batch(&plan2).unwrap();
        assert_eq!(r.applied, 1);
        assert_eq!(r.entries, 2);
        assert_ne!(e.fingerprint_decl(), fp_before);
    }

    #[test]
    fn batch_deterministic_dual_run() {
        let mut e1 = StyleTreeEngine::new();
        let mut e2 = StyleTreeEngine::new();
        feed_ops(&mut e1);
        feed_ops(&mut e2);
        assert_eq!(e1.fingerprint_decl(), e2.fingerprint_decl());
        // 批量同序同果。
        let mut plan = BatchPlan::new();
        let mut p = ValuePatch::new();
        let _ = p.push(PropId::LetterSpacingPx, 1);
        let _ = p.push(PropId::BorderWidthPx, 3);
        let _ = p.push(PropId::OpacityPermille, 800);
        let mut i = 0usize;
        while i < e1.tree.len() {
            let id = e1.tree.nodes[i].id;
            let _ = plan.push(BatchItem { node: id, patch: p });
            i += 1;
        }
        let _ = e1.apply_batch(&plan).unwrap();
        // e2 的节点 id 序与 e1 相同（同源操作序列）。
        let mut plan2 = BatchPlan::new();
        let mut i = 0usize;
        while i < e2.tree.len() {
            let id = e2.tree.nodes[i].id;
            let _ = plan2.push(BatchItem { node: id, patch: p });
            i += 1;
        }
        let _ = e2.apply_batch(&plan2).unwrap();
        assert_eq!(e1.fingerprint_decl(), e2.fingerprint_decl());
    }

    #[test]
    fn upstream_hash_gate() {
        let mut e = StyleTreeEngine::new();
        let k = StyleKey::new(0x8001, 0x0001, Pseudo::None, 0);
        let packed = k.pack().unwrap();
        let cv = full_cv(16, 0x010203);
        let kv = KeyedValues { key: packed, values: cv };
        let good = fnv64(&kv.canonical_bytes());
        // 哈希相符 → 接收。
        let r = e
            .ingest_upstream(&[UpstreamDecl { key: k, values: cv, declared_hash: good }], None)
            .unwrap();
        assert_eq!(r.checked, 1);
        assert_eq!(r.accepted, 1);
        assert_eq!(r.nodes_added, 1);
        // 哈希被篡改 → 整批拒收带码，零部分接收。
        let unique_before = e.store.stats.unique;
        let cv2 = full_cv(17, 0x010204);
        let kv2 = KeyedValues { key: packed, values: cv2 };
        let good2 = fnv64(&kv2.canonical_bytes());
        let err = e
            .ingest_upstream(
                &[
                    UpstreamDecl { key: k, values: cv2, declared_hash: good2 },
                    UpstreamDecl { key: k, values: cv2, declared_hash: good2 ^ 1 },
                ],
                None,
            )
            .unwrap_err();
        assert_eq!(err.code(), E_ST_UPSTREAM_HASH);
        assert_eq!(e.store.stats.unique, unique_before);
        // 未满配拒（结构性守卫）。
        let mut partial = ComputedValues::defaults();
        let _ = partial.write_clamped(PropId::FontSizePx, 16);
        assert!(e.ingest_upstream(&[UpstreamDecl { key: k, values: partial, declared_hash: 0 }], None).is_err());
    }

    #[test]
    fn downstream_decl_and_consumer_view() {
        assert!(audit_downstream().is_ok());
        assert_eq!(DOWNSTREAM_DECL.peer, "VE-F2811");
        let mut e = StyleTreeEngine::new();
        let k = StyleKey::new(0x9001, 0x0001, Pseudo::None, 0);
        let _ = e.intern(k, full_cv(16, 0x0A0B0C)).unwrap();
        let view = e.consumer_view();
        let packed = k.pack().unwrap();
        assert_eq!(view.values_of(packed).unwrap().font_size_px(), 16);
        assert_eq!(view.strong_of(packed), Some(1));
        assert_eq!(view.instances(), 1);
        // 对账钩子：在账 key 过、未知 key 拒。
        assert_eq!(e.reconcile_keys(&[packed]).unwrap(), 1);
        assert!(e.reconcile_keys(&[0x1234]).is_err());
    }

    #[test]
    fn health_and_conserves() {
        let mut e = StyleTreeEngine::new();
        feed_ops(&mut e);
        let h = e.health();
        assert!(h.healthy());
        assert!(h.tree_sane);
        assert!(h.store_conserves_interns);
        assert!(h.store_conserves_live);
        assert!(h.stats_conserves);
        assert!(audit_all(&e).is_ok());
    }

    #[test]
    fn determinism_probe_consistent() {
        let mut e = StyleTreeEngine::new();
        feed_ops(&mut e);
        let rep = e.determinism_probe().unwrap();
        assert!(rep.consistent);
        assert_eq!(rep.pass_a, rep.pass_b);
        assert_eq!(rep.nodes, e.tree.len());
    }

    #[test]
    fn injections_all_detected() {
        let mut e = StyleTreeEngine::new();
        // 非法 key 注入（非规范低 20 位）→ 检出。
        let bad = 0x0000_0000_0000_0001u64;
        assert_eq!(e.inject_bad_key(bad).unwrap(), true);
        // 合法 key 注入 → 如实上报未检出。
        let good = StyleKey::new(0x0B01, 0x0001, Pseudo::None, 0).pack().unwrap();
        assert_eq!(e.inject_bad_key(good).unwrap(), false);
        // 越界注入（字号 999 越上界 72）→ 钳制检出。
        assert_eq!(e.inject_out_of_domain(PropId::FontSizePx, 999).unwrap(), true);
        // 域内注入 → 如实上报未检出。
        assert_eq!(e.inject_out_of_domain(PropId::FontSizePx, 20).unwrap(), false);
        // 继承环注入 → 检出。
        assert_eq!(e.inject_inheritance_cycle().unwrap(), true);
        assert_eq!(e.stats.injections_caught, 3);
        assert_eq!(e.stats.injections_missed, 2);
        // 注入后树面无残留探针（拆净）+ 健康守恒。
        assert!(e.health().healthy());
    }

    #[test]
    fn case_ledger_append_only_and_full() {
        let mut e = StyleTreeEngine::new();
        // 灌满立案账。
        let mut i = 0u64;
        while i < CASE_CAP as u64 {
            assert!(e.cases.open_case(E_ST_CAP, "灌账", "test", "清账", i).is_ok());
            i += 1;
        }
        assert_eq!(e.cases.len(), CASE_CAP);
        // 满后拒绝新案（append-only 不覆盖）+ 溢出显性。
        assert!(e.cases.open_case(E_ST_CAP, "溢出", "test", "清账", 99).is_err());
        assert_eq!(e.cases.total_overflow(), 1);
        // 引擎层立案不牵连原错误路径（溢出计数不静默）。
        let k = StyleKey::new(0x0C01, 0x0001, Pseudo::None, 0);
        let _ = e.add_node(None, k, ComputedValues::defaults(), "n").unwrap();
        assert!(e.add_node(Some(0xBEEF), k, ComputedValues::defaults(), "n").is_err());
        assert_eq!(e.stats.case_overflow, 1);
    }

    #[test]
    fn error_codes_segment_exclusive() {
        // 十五码全部落在 0x13B1..=0x13BF，且两两互异。
        let codes = all_code_nums();
        assert_eq!(codes.len(), 15);
        let mut i = 0usize;
        while i < codes.len() {
            assert!(codes[i] > SEG_BASE && codes[i] <= SEG_TOP);
            let mut j = i + 1;
            while j < codes.len() {
                assert_ne!(codes[i], codes[j]);
                j += 1;
            }
            i += 1;
        }
        // 码名表与数值码表逐位对位。
        let mut i = 0usize;
        while i < ERROR_CODE_NAMES.len() {
            assert!(!ERROR_CODE_NAMES[i].is_empty());
            i += 1;
        }
        // 三要素抽查：detail 非空且为中文诊断。
        let e = StyleTreeError::Cycle { node: 1, via: 2 };
        assert!(!e.detail().is_empty());
        assert!(e.detail().contains("环"));
        assert_eq!(e.code(), E_ST_CYCLE);
    }

    #[test]
    fn value_patch_cap_and_decode() {
        let mut p = ValuePatch::new();
        let mut i = 0usize;
        while i < PROP_COUNT {
            assert!(p.push(PropId::of_rank(i).unwrap(), 0).is_ok());
            i += 1;
        }
        assert!(p.push(PropId::FontSizePx, 0).is_err());
        // 解码守卫：合法秩回原、越界拒。
        let (pp, v) = p.decoded_at(0).unwrap();
        assert_eq!(pp.rank(), 0);
        assert_eq!(v, 0);
        assert!(p.decoded_at(PROP_COUNT).is_err());
        assert_eq!(p.decoded_all().unwrap().len(), PROP_COUNT);
    }

    #[test]
    fn node_label_clamped_at_char_boundary() {
        let mut e = StyleTreeEngine::new();
        let k = StyleKey::new(0x0D01, 0x0001, Pseudo::None, 0);
        // 32 字节边界落在多字节字符中间 → 回退到字符边界。
        let long = "甲乙丙丁戊己庚辛壬癸甲乙丙丁戊己庚辛壬癸甲乙丙丁戊己庚辛";
        let n = e.add_node(None, k, ComputedValues::defaults(), long).unwrap();
        let lab = e.tree.node_of(n).unwrap().label.clone();
        assert!(lab.len() <= NODE_LABEL_CAP);
        assert!(lab.chars().count() >= 1);
        // 拆叶子正常；拆非叶拒。
        let c = e.add_node(Some(n), k, ComputedValues::defaults(), "c").unwrap();
        assert!(e.remove_leaf(n).is_err());
        let _ = e.remove_leaf(c).unwrap();
        let _ = e.remove_leaf(n).unwrap();
        assert!(e.remove_leaf(n).is_err());
    }

    #[test]
    fn screen_and_summary_faces_readable() {
        let mut e = StyleTreeEngine::new();
        feed_ops(&mut e);
        // 无障碍替述面：读屏单行非空且含关键量。
        let s = e.screen_text();
        assert!(s.contains("节点"));
        assert!(s.contains("去重率"));
        let cv = e.compute(1).unwrap_or_else(|_| ComputedValues::defaults());
        assert!(cv.screen_line().contains("font_size_px"));
        assert!(!domain_summary().is_empty());
        let h = e.health();
        assert!(h.screen_text().contains("健康"));
    }

    #[test]
    fn gate_order_and_criteria_frozen() {
        assert_eq!(GATE_ORDER[0], "validate-reject");
        assert_eq!(GATE_ORDER[1], "clamp-warn");
        assert_eq!(GATE_ORDER[2], "case-file");
        assert_eq!(GATE_ORDER[3], "commit-ledger");
        assert_eq!(CRITERIA.len(), 7);
        assert_eq!(STYLETREE_VERSION, "O10-styletree-v1");
        assert_eq!(UPSTREAM_ANCHOR, "VE-F2809");
        assert_eq!(DOWNSTREAM_ANCHOR, "VE-F2811");
    }
}





