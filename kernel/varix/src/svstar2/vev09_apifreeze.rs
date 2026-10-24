//! VE-F4607 · 插件 API 冻结层（VE-V 域 · 插件组 · 目标 340 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F4607`
//!
//! **判据（锚点原文）**：三类冻结、语义版本、十年承诺、门面隔离、变更评审、判据。
//!
//! **职责定位（锚点原文摘录）**：插件 API 冻结层（对插件暴露的 API 面总冻结：三类 API（显示/色彩/通用 UI）逐一冻结+语义版本+双版期承诺——十年不变口径对生态兑现；API 门面模式：插件只见门面不见内部实现；API 变更评审与 V 域变更纪律同规）。数据结构：冻结 API 门面；语义版本登记；变更评审单。错误路径与降级矩阵：门面外泄→拒绝；破坏兼容→版本拦截；越权变更→冻结评审。性能逐项分解：门面 O(API 数)；拦截 O(1)；评审 O(变更面)。
//!
//! ---------------------------------------------------------------------------
//! # 施工分节（本文件为自包含子系统，零 `crate::` 依赖，no_std + alloc）
//!
//! * **一、诊断码段**：0x1BC0..=0x1BCF 独占段（整仓 grep 零占用后选定，见常量注释），
//!   十二码互异、段内、每码配中文可操作 `detail`。
//! * **二、三类闭集**：[`ApiClass`] 只有 Display/Color/Ui 三态——闭集即冻结，
//!   第四类进不来（编译期封闭 + 运行期登记校验双保险）。
//! * **三、语义版本**：[`SemVer`] MAJOR.MINOR 双段语义版本。纪律：MINOR 只增
//!   兼容（门面 MINOR ≥ 插件 MINOR 即可供应）；MAJOR 前进 = 破坏兼容，必须走评审。
//! * **四、冻结登记簿**：[`FrozenApi`] 每条目带 api id / 签名指纹 / 语义版本 /
//!   冻结日期账（账面日序，第 0 日 = 2026-01-01 冻结基准日，写死口径）。
//! * **五、API 门面**：[`FACADE_TABLE`] 常量注册——插件可见面只含门面符号；
//!   内部符号表 [`INTERNAL_SYMBOLS`] 不在门面导出，[`probe_symbol`] 命中内部
//!   符号即拒绝带码（外泄→拒绝）。按 id 直查 O(1)，全枚举 O(API 数)。
//! * **六、十年承诺账**：[`commitment_phase`] 双版期（支持期 3650 日 + 宽限期
//!   730 日）逐 API 起算、到期检查 O(API 数)；宽限期只修不增，过期即冻结。
//! * **七、兼容性门**：[`CompatibilityGate`] 插件声明版本 vs 门面版本——MAJOR
//!   相等且门面 MINOR ≥ 插件 MINOR 才放行；破坏兼容即拦截 + 留痕（O(1)）。
//! * **八、变更评审**：[`ChangeReview`] 提案→影响面分析→四态流转（提交/评审中/
//!   通过/否决）；未过评审改冻结面即拦截 + 红账（越权→冻结评审）。
//! * **九、门面指纹**：全 API 面 FNV-1a 总指纹——任何冻结面变更指纹必变 =
//!   变更可检测；承诺期内无账变更 → [`FreezeLayerError::FingerprintDrift`]。
//! * **十、事件账 + health + 双跑对拍 + 注入面**：外泄注入/破坏兼容注入/越权
//!   变更注入各检出；同脚本双实例 digest 全等（确定性对拍）。
//!
//! # 性能逐项分解（锚点口径落实）
//!
//! * 门面：按 id 直查 **O(1)**（id 密集 1..=N 直接下标）；全枚举 O(API 数)。
//! * 拦截：[`CompatibilityGate::gate`] 纯比较 + 定长留痕 **O(1)**。
//! * 评审：影响面分析扫同类 API，**O(变更面)**（变更面 = 同类 API 计数）。
//! * 指纹：全表面 FNV 一遍 **O(API 数)**；承诺到期检查 **O(API 数)**。

use alloc::string::{String, ToString};
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 一、诊断码段（0x1BC0..=0x1BCF 独占——整仓 grep 零占用后选定）
// ---------------------------------------------------------------------------

// 段选择依据（逐项 grep 在案）：0x1B01..=0x1B08 已被 vea41_heaps.rs 占用；
// 0x1B20..=0x1B23 已被 ves480_s26perf.rs 占用；0x1B40=cga34_postfx、
// 0x1B50=cga35_clkadapt、0x1B60=cga36_mixedprec（其级联码用到 0x1B70..=0x1B77）、
// 0x1B90=cga37_graphbench、0x1BA0=cga38_allocpool、0x1BB0=cga39_cmdqueue、
// 0x1BD0=cga40_predict、0x1BE0=cga41_errdisc（见 cga44_closeout.rs 段账）；
// 0x1B30 段有 0x1B36（PCI 厂商号 Red Hat）文本命中，为避嫌不用；
// 0x1BC0..=0x1BCF 整段零命中——本层独占。

/// 诊断码：门面外泄（内部符号经门面查询被拒）。
pub const CODE_FACADE_LEAK: u16 = 0x1BC0;
/// 诊断码：破坏兼容被版本拦截。
pub const CODE_VERSION_BLOCKED: u16 = 0x1BC1;
/// 诊断码：越权变更（未过评审改冻结面）。
pub const CODE_UNAUTHORIZED: u16 = 0x1BC2;
/// 诊断码：评审未到态（流转非法 / 评审单不存在）。
pub const CODE_REVIEW_BLOCKED: u16 = 0x1BC3;
/// 诊断码：未知 API（不在冻结登记簿）。
pub const CODE_API_NOT_FOUND: u16 = 0x1BC4;
/// 诊断码：账簿满（事件/评审/版本/留痕任一容量耗尽——显性拒收不静默丢）。
pub const CODE_LEDGER_FULL: u16 = 0x1BC5;
/// 诊断码：承诺过期（宽限期外还试图增改）。
pub const CODE_COMMITMENT_EXPIRED: u16 = 0x1BC6;
/// 诊断码：API 类别越界（三类闭集之外）。
pub const CODE_CLASS_INVALID: u16 = 0x1BC7;
/// 诊断码：签名指纹漂移（条目签名与登记不符）。
pub const CODE_SIGNATURE_DRIFT: u16 = 0x1BC8;
/// 诊断码：重复登记（api id 撞账）。
pub const CODE_DUPLICATE_API: u16 = 0x1BC9;
/// 诊断码：双跑对拍不一致。
pub const CODE_DUALRUN_MISMATCH: u16 = 0x1BCA;
/// 诊断码：冻结面指纹漂移（有变更无账）。
pub const CODE_FINGERPRINT_DRIFT: u16 = 0x1BCB;

/// 全部诊断码（判据用：互异 + 段内双校验）。
pub const ALL_CODES: [u16; 12] = [
    CODE_FACADE_LEAK,
    CODE_VERSION_BLOCKED,
    CODE_UNAUTHORIZED,
    CODE_REVIEW_BLOCKED,
    CODE_API_NOT_FOUND,
    CODE_LEDGER_FULL,
    CODE_COMMITMENT_EXPIRED,
    CODE_CLASS_INVALID,
    CODE_SIGNATURE_DRIFT,
    CODE_DUPLICATE_API,
    CODE_DUALRUN_MISMATCH,
    CODE_FINGERPRINT_DRIFT,
];

/// 诊断段基址与上界（0x1BC0..=0x1BCF，独占 0x10）。
pub const SEG_BASE: u16 = 0x1BC0;
/// 诊断段上界（不含）。
pub const SEG_END: u16 = 0x1BD0;

// ---------------------------------------------------------------------------
// 二、错误枚举（每变体带 code + detail 中文可操作诊断）
// ---------------------------------------------------------------------------

/// 冻结层全部可失败路径——上层只认 [`FreezeLayerError`]，不猜原因。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FreezeLayerError {
    /// 门面外泄：内部符号经门面查询命中即拒。
    FacadeLeak(&'static str),
    /// 版本拦截：插件声明版本与门面版本破坏兼容。
    VersionBlocked {
        /// 目标 API id。
        api_id: u16,
        /// 插件声明版本。
        declared: SemVer,
        /// 门面当前版本。
        facade: SemVer,
    },
    /// 越权变更：未过评审（或评审未批）就改冻结面。
    UnauthorizedChange {
        /// 目标 API id。
        api_id: u16,
        /// 越权原因（固定文案）。
        why: &'static str,
    },
    /// 评审流转非法：状态机不放行或评审单不存在。
    ReviewBlocked {
        /// 评审单 id（0 = 不存在）。
        review_id: u32,
        /// 拒绝原因（固定文案）。
        why: &'static str,
    },
    /// 未知 API：id 不在冻结登记簿。
    ApiNotFound(u16),
    /// 账簿满：定长账簿容量耗尽（显性拒收，不静默丢账）。
    LedgerFull {
        /// 账簿容量。
        cap: usize,
        /// 账簿名（固定文案）。
        which: &'static str,
    },
    /// 承诺过期：承诺期（含宽限期）外试图增改冻结面。
    CommitmentExpired {
        /// 目标 API id。
        api_id: u16,
        /// 当前账面日序。
        day: u32,
    },
    /// 类别越界：三类闭集之外的类别值。
    ClassInvalid(u8),
    /// 签名漂移：条目实际签名指纹与登记指纹不符。
    SignatureDrift {
        /// 目标 API id。
        api_id: u16,
    },
    /// 重复登记：api id 撞账。
    DuplicateApi(u16),
    /// 双跑对拍不一致：同脚本两实例 digest 不等。
    DualRunMismatch {
        /// 实例 A digest。
        lhs: u32,
        /// 实例 B digest。
        rhs: u32,
    },
    /// 指纹漂移：冻结面指纹变了但事件账里无对应变更账。
    FingerprintDrift {
        /// 当前指纹。
        current: u32,
        /// 冻结基线指纹。
        baseline: u32,
    },
}

impl FreezeLayerError {
    /// 诊断码（0x1BC0..=0x1BCF 段内，互异）。
    pub const fn code(&self) -> u16 {
        match self {
            FreezeLayerError::FacadeLeak(_) => CODE_FACADE_LEAK,
            FreezeLayerError::VersionBlocked { .. } => CODE_VERSION_BLOCKED,
            FreezeLayerError::UnauthorizedChange { .. } => CODE_UNAUTHORIZED,
            FreezeLayerError::ReviewBlocked { .. } => CODE_REVIEW_BLOCKED,
            FreezeLayerError::ApiNotFound(_) => CODE_API_NOT_FOUND,
            FreezeLayerError::LedgerFull { .. } => CODE_LEDGER_FULL,
            FreezeLayerError::CommitmentExpired { .. } => CODE_COMMITMENT_EXPIRED,
            FreezeLayerError::ClassInvalid(_) => CODE_CLASS_INVALID,
            FreezeLayerError::SignatureDrift { .. } => CODE_SIGNATURE_DRIFT,
            FreezeLayerError::DuplicateApi(_) => CODE_DUPLICATE_API,
            FreezeLayerError::DualRunMismatch { .. } => CODE_DUALRUN_MISMATCH,
            FreezeLayerError::FingerprintDrift { .. } => CODE_FINGERPRINT_DRIFT,
        }
    }

    /// 中文可操作诊断（读屏口径：说清发生了什么、下一步做什么）。
    pub const fn detail(&self) -> &'static str {
        match self {
            FreezeLayerError::FacadeLeak(_) => {
                "门面外泄：查询命中内部符号——插件只许走门面符号表，改用 FACADE_TABLE 里的门面符号"
            }
            FreezeLayerError::VersionBlocked { .. } => {
                "版本拦截：插件声明版本与门面破坏兼容——MAJOR 必须相等且门面 MINOR ≥ 插件 MINOR，请升级插件或等门面升 MINOR"
            }
            FreezeLayerError::UnauthorizedChange { .. } => {
                "越权变更：冻结面变更必须先开变更评审单并获通过——请走 open_review → UnderReview → Approved 流转"
            }
            FreezeLayerError::ReviewBlocked { .. } => {
                "评审流转非法：按 提交→评审中→通过/否决 四态走，不可跳态；查 review_id 是否存在"
            }
            FreezeLayerError::ApiNotFound(_) => {
                "未知 API：id 不在冻结登记簿——查 FACADE_TABLE 的 id 段（1..=9）后再调"
            }
            FreezeLayerError::LedgerFull { .. } => {
                "账簿满：定长账簿容量耗尽——先结算/归档旧账再写新账，不静默丢账"
            }
            FreezeLayerError::CommitmentExpired { .. } => {
                "承诺过期：双版期（支持期+宽限期）外只读不增改——需要变更请先走承诺修订评审"
            }
            FreezeLayerError::ClassInvalid(_) => {
                "类别越界：API 类别只有 显示/色彩/通用UI 三类闭集——按 ApiClass 三态重报"
            }
            FreezeLayerError::SignatureDrift { .. } => {
                "签名漂移：条目实际签名指纹与登记不符——登记簿被外部改动，先对账再修"
            }
            FreezeLayerError::DuplicateApi(_) => {
                "重复登记：api id 撞账——换未占用的 id，冻结登记簿 id 全程唯一"
            }
            FreezeLayerError::DualRunMismatch { .. } => {
                "双跑不一致：同脚本两实例 digest 不等——确定性被破坏，查注入面外的隐藏可变态"
            }
            FreezeLayerError::FingerprintDrift { .. } => {
                "指纹漂移：冻结面指纹变了但无变更账——补开变更评审单对账，或回滚未记账的改动"
            }
        }
    }
}

// ---------------------------------------------------------------------------
// 三、ApiClass 三类闭集（显示 / 色彩 / 通用 UI）
// ---------------------------------------------------------------------------

/// API 三类闭集——冻结只认这三类，第四类不存在（闭集即冻结的第一道门）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ApiClass {
    /// 显示类（呈现/模式/同步面）。
    Display,
    /// 色彩类（色彩空间/色调映射面）。
    Color,
    /// 通用 UI 类（布局/输入/无障碍面）。
    Ui,
}

impl ApiClass {
    /// 类名（读屏口径）。
    pub const fn as_str(self) -> &'static str {
        match self {
            ApiClass::Display => "显示",
            ApiClass::Color => "色彩",
            ApiClass::Ui => "通用UI",
        }
    }

    /// 闭集下标（0/1/2——账面序）。
    pub const fn as_index(self) -> u8 {
        match self {
            ApiClass::Display => 0,
            ApiClass::Color => 1,
            ApiClass::Ui => 2,
        }
    }

    /// 闭集校验：u8 → 类（越界即 None——三类之外没有第四类）。
    pub const fn from_index(v: u8) -> Option<ApiClass> {
        match v {
            0 => Some(ApiClass::Display),
            1 => Some(ApiClass::Color),
            2 => Some(ApiClass::Ui),
            _ => None,
        }
    }

    /// 全部类别（枚举口径固定序，账面遍历用）。
    pub const ALL: [ApiClass; 3] = [ApiClass::Display, ApiClass::Color, ApiClass::Ui];
}

// ---------------------------------------------------------------------------
// 四、语义版本（MAJOR.MINOR——MINOR 只增兼容，MAJOR 前进必须走评审）
// ---------------------------------------------------------------------------

/// 语义版本（MAJOR.MINOR 双段——冻结层口径不需要 PATCH：补丁不进门面）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SemVer {
    /// 主版本：前进 = 破坏兼容，必须走变更评审。
    pub major: u16,
    /// 次版本：只增兼容（新增面不改既有语义）。
    pub minor: u16,
}

impl SemVer {
    /// 构造（const——种子表/判据表都可编译期算）。
    pub const fn new(major: u16, minor: u16) -> SemVer {
        SemVer { major, minor }
    }

    /// 兼容判定（纪律核心，const O(1)）：
    /// MAJOR 相等（无破坏）且门面 MINOR ≥ 插件 MINOR（门面向后兼容供应）。
    /// `self` = 插件声明版本，`facade` = 门面当前版本。
    pub const fn is_compatible_with(self, facade: SemVer) -> bool {
        self.major == facade.major && facade.minor >= self.minor
    }

    /// MAJOR 是否前进（破坏兼容——评审强制线）。
    pub const fn breaks_from(self, old: SemVer) -> bool {
        self.major != old.major
    }

    /// MINOR 兼容增补（old 基础上 minor+1，饱和——不回绕）。
    pub const fn bump_minor(self) -> SemVer {
        SemVer { major: self.major, minor: self.minor.saturating_add(1) }
    }

    /// MAJOR 前进（minor 归零——破坏兼容必须走评审后由本函数落账）。
    pub const fn bump_major(self) -> SemVer {
        SemVer { major: self.major.saturating_add(1), minor: 0 }
    }

    /// 版本行（读屏口径：M.m）。
    pub fn as_line(self) -> String {
        let mut s = String::new();
        s.push_str("v");
        s.push_str(&self.major.to_string());
        s.push_str(".");
        s.push_str(&self.minor.to_string());
        s
    }
}

// ---------------------------------------------------------------------------
// 五、FNV-1a 指纹基元（32 位，参数写死注释来源）
// ---------------------------------------------------------------------------

/// FNV-1a 32 位偏移基（标准参数 0x811C9DC5，FNV-1a 规范口径写死）。
pub const FNV_OFFSET: u32 = 0x811C_9DC5;
/// FNV-1a 32 位素数（标准参数 0x01000193，FNV-1a 规范口径写死）。
pub const FNV_PRIME: u32 = 0x0100_0193;

/// 单字节折叠（const——种子签名可编译期算）。
pub const fn fnv_fold_byte(h: u32, b: u8) -> u32 {
    (h ^ (b as u32)).wrapping_mul(FNV_PRIME)
}

/// 字符串折叠（const——`&'static str` 种子可编译期算）。
pub const fn fnv_fold_str(h: u32, s: &str) -> u32 {
    let bytes = s.as_bytes();
    let mut h = h;
    let mut i = 0usize;
    while i < bytes.len() {
        h = fnv_fold_byte(h, bytes[i]);
        i += 1;
    }
    h
}

/// u16 折叠（小端两字节——指纹输入定宽，跨机一致）。
pub const fn fnv_fold_u16(h: u32, v: u16) -> u32 {
    let h = fnv_fold_byte(h, (v & 0xFF) as u8);
    fnv_fold_byte(h, ((v >> 8) & 0xFF) as u8)
}

/// u32 折叠（小端四字节）。
pub const fn fnv_fold_u32(h: u32, v: u32) -> u32 {
    let h = fnv_fold_u16(h, (v & 0xFFFF) as u16);
    fnv_fold_u16(h, ((v >> 16) & 0xFFFF) as u16)
}

/// const 字符串判等（`&str` 的 `==` 走 derive PartialEq 非 const——
/// const fn 里禁用，此处按字节手写等价判定）。
pub const fn str_eq(a: &str, b: &str) -> bool {
    let ab = a.as_bytes();
    let bb = b.as_bytes();
    if ab.len() != bb.len() {
        return false;
    }
    let mut i = 0usize;
    while i < ab.len() {
        if ab[i] != bb[i] {
            return false;
        }
        i += 1;
    }
    true
}

/// API 签名指纹：签名串（api 名 + 0xFF 分隔 + 门面符号）FNV-1a。
/// 签名变 → 指纹变 → 可检测（登记时算一次写账）。
pub const fn signature_fingerprint(name: &str, facade_symbol: &str) -> u32 {
    let h = fnv_fold_str(FNV_OFFSET, name);
    let h = fnv_fold_byte(h, 0xFF);
    fnv_fold_str(h, facade_symbol)
}

// ---------------------------------------------------------------------------
// 六、冻结登记簿（种子表 const + FrozenApi 条目 + 冻结日期账）
// ---------------------------------------------------------------------------

/// 账面日序第 0 日 = 2026-01-01（VE-V 域冻结基准日，写死口径——所有冻结/
/// 起算/到期日期都用日序，不用时钟：内核无钟，日序是账）。
pub const FROZEN_EPOCH_NOTE: &str = "账面日序第 0 日 = 2026-01-01（冻结基准日）";

/// 冻结种子条目（const 表——冻结面在编译期就定死，运行期只读账不造账）。
#[derive(Clone, Copy, Debug)]
pub struct FrozenApiSeed {
    /// API id（1..=9 密集——门面按 id 直查 O(1) 的前提）。
    pub api_id: u16,
    /// API 名（签名指纹输入之一）。
    pub name: &'static str,
    /// 类别（三类闭集之一）。
    pub class: ApiClass,
    /// 门面符号（插件可见；内部符号不在其中）。
    pub facade_symbol: &'static str,
    /// 冻结时语义版本。
    pub version: SemVer,
    /// 冻结日（账面日序）——承诺期起算日。
    pub frozen_day: u32,
}

/// 冻结种子表：三类 API（显示/色彩/通用 UI）各三条，共九条逐一冻结。
/// 版本与冻结日为示范账（与锚点「三类 API 逐一冻结+语义版本」口径一致，
/// 真实面以此表为准扩充，id 密集不变式必须保住）。
pub const FROZEN_API_SEED: [FrozenApiSeed; 9] = [
    FrozenApiSeed {
        api_id: 1,
        name: "display.present_frame",
        class: ApiClass::Display,
        facade_symbol: "vev09_disp_present",
        version: SemVer::new(1, 4),
        frozen_day: 0,
    },
    FrozenApiSeed {
        api_id: 2,
        name: "display.query_modes",
        class: ApiClass::Display,
        facade_symbol: "vev09_disp_modes",
        version: SemVer::new(1, 4),
        frozen_day: 0,
    },
    FrozenApiSeed {
        api_id: 3,
        name: "display.vsync_policy",
        class: ApiClass::Display,
        facade_symbol: "vev09_disp_vsync",
        version: SemVer::new(1, 2),
        frozen_day: 12,
    },
    FrozenApiSeed {
        api_id: 4,
        name: "color.convert_cs",
        class: ApiClass::Color,
        facade_symbol: "vev09_color_conv",
        version: SemVer::new(2, 1),
        frozen_day: 0,
    },
    FrozenApiSeed {
        api_id: 5,
        name: "color.query_gamut",
        class: ApiClass::Color,
        facade_symbol: "vev09_color_gamut",
        version: SemVer::new(2, 1),
        frozen_day: 0,
    },
    FrozenApiSeed {
        api_id: 6,
        name: "color.tone_map",
        class: ApiClass::Color,
        facade_symbol: "vev09_color_tone",
        version: SemVer::new(2, 3),
        frozen_day: 30,
    },
    FrozenApiSeed {
        api_id: 7,
        name: "ui.layout_measure",
        class: ApiClass::Ui,
        facade_symbol: "vev09_ui_measure",
        version: SemVer::new(1, 0),
        frozen_day: 0,
    },
    FrozenApiSeed {
        api_id: 8,
        name: "ui.input_route",
        class: ApiClass::Ui,
        facade_symbol: "vev09_ui_input",
        version: SemVer::new(1, 1),
        frozen_day: 21,
    },
    FrozenApiSeed {
        api_id: 9,
        name: "ui.a11y_announce",
        class: ApiClass::Ui,
        facade_symbol: "vev09_ui_a11y",
        version: SemVer::new(1, 0),
        frozen_day: 0,
    },
];

/// 冻结 API 数（种子表长度——id 密集 1..=9，判据按字面量重算 9）。
pub const API_COUNT: usize = 9;
/// 门面 id 基（id 密集从 1 起——直查下标 = id - 1）。
pub const FACADE_ID_BASE: u16 = 1;

/// 冻结 API 条目（登记簿一行：id/名/类/门面符号/版本/签名指纹/冻结日/承诺态）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FrozenApi {
    /// API id（唯一、密集）。
    pub api_id: u16,
    /// API 名。
    pub name: &'static str,
    /// 类别。
    pub class: ApiClass,
    /// 门面符号（插件可见）。
    pub facade_symbol: &'static str,
    /// 当前语义版本（MINOR 只增兼容；MAJOR 前进必走评审）。
    pub version: SemVer,
    /// 冻结日（承诺期起算日）。
    pub frozen_day: u32,
    /// 签名指纹（登记时算定——签名变指纹变，可检测）。
    pub sig_fnv: u32,
    /// 承诺态（最近一次到期检查的落账态）。
    pub commitment: CommitmentPhase,
}

/// 冻结面总指纹：全 API 面（id/名/门面符号/版本/签名/冻结日）按账序折叠。
/// 任何冻结面变更 → 指纹必变 = 变更可检测（锚点判据的机制化）。
pub fn surface_fingerprint(apis: &[FrozenApi]) -> u32 {
    let mut h = FNV_OFFSET;
    let mut i = 0usize;
    while i < apis.len() {
        let a = &apis[i];
        h = fnv_fold_u16(h, a.api_id);
        h = fnv_fold_str(h, a.name);
        h = fnv_fold_str(h, a.facade_symbol);
        h = fnv_fold_u16(h, a.version.major);
        h = fnv_fold_u16(h, a.version.minor);
        h = fnv_fold_u32(h, a.sig_fnv);
        h = fnv_fold_u32(h, a.frozen_day);
        i += 1;
    }
    h
}

// ---------------------------------------------------------------------------
// 七、API 门面（const 注册 + 内部符号表隔离 + 外泄检测）
// ---------------------------------------------------------------------------

/// 门面条目（插件可见面一行：id ↔ 门面符号 ↔ 类 ↔ 名）。
/// 内部实现符号**不在**本表——门面隔离即「插件只见门面不见内部实现」。
#[derive(Clone, Copy, Debug)]
pub struct ApiFacadeEntry {
    /// API id。
    pub api_id: u16,
    /// 门面符号（插件唯一可见调用面）。
    pub facade_symbol: &'static str,
    /// 类别。
    pub class: ApiClass,
    /// API 名。
    pub name: &'static str,
}

/// 门面表（const 注册——按 id 密集排列，直查 O(1)）。
pub const FACADE_TABLE: [ApiFacadeEntry; 9] = [
    ApiFacadeEntry { api_id: 1, facade_symbol: "vev09_disp_present", class: ApiClass::Display, name: "display.present_frame" },
    ApiFacadeEntry { api_id: 2, facade_symbol: "vev09_disp_modes", class: ApiClass::Display, name: "display.query_modes" },
    ApiFacadeEntry { api_id: 3, facade_symbol: "vev09_disp_vsync", class: ApiClass::Display, name: "display.vsync_policy" },
    ApiFacadeEntry { api_id: 4, facade_symbol: "vev09_color_conv", class: ApiClass::Color, name: "color.convert_cs" },
    ApiFacadeEntry { api_id: 5, facade_symbol: "vev09_color_gamut", class: ApiClass::Color, name: "color.query_gamut" },
    ApiFacadeEntry { api_id: 6, facade_symbol: "vev09_color_tone", class: ApiClass::Color, name: "color.tone_map" },
    ApiFacadeEntry { api_id: 7, facade_symbol: "vev09_ui_measure", class: ApiClass::Ui, name: "ui.layout_measure" },
    ApiFacadeEntry { api_id: 8, facade_symbol: "vev09_ui_input", class: ApiClass::Ui, name: "ui.input_route" },
    ApiFacadeEntry { api_id: 9, facade_symbol: "vev09_ui_a11y", class: ApiClass::Ui, name: "ui.a11y_announce" },
];

/// 内部符号表（**不在门面导出**——命中任一即外泄，拒绝带码）。
/// 与门面表刻意无交集：门面隔离的负样本面。
pub const INTERNAL_SYMBOLS: [&str; 4] = [
    "vev09_internal_arena_resolve",
    "vev09_internal_cs_pipeline",
    "vev09_internal_framegraph",
    "vev09_internal_gate_core",
];

/// 门面探针裁决。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProbeVerdict {
    /// 门面符号——放行（插件正路）。
    Allowed {
        /// 命中的 API id。
        api_id: u16,
    },
    /// 不在冻结面（门面表和内部表都未命中）——按未知 API 拒。
    Unknown,
}

/// 门面符号探针（拦截 O(API 数) 枚举 / O(1) 按 id 直查——探针走符号枚举，
/// O(符号数)；插件运行路径走 [`facade_lookup`] 直查 O(1)）。
///
/// * 门面符号命中 → [`ProbeVerdict::Allowed`]（正路放行）；
/// * 内部符号命中 → `Err(FacadeLeak)`（外泄→拒绝，锚点降级矩阵第一行）；
/// * 双表皆未命中 → [`ProbeVerdict::Unknown`]（未知面，上层按 ApiNotFound 处置）。
pub fn probe_symbol(symbol: &str) -> Result<ProbeVerdict, FreezeLayerError> {
    let mut i = 0usize;
    while i < INTERNAL_SYMBOLS.len() {
        if symbol == INTERNAL_SYMBOLS[i] {
            return Err(FreezeLayerError::FacadeLeak(INTERNAL_SYMBOLS[i]));
        }
        i += 1;
    }
    let mut j = 0usize;
    while j < FACADE_TABLE.len() {
        if symbol == FACADE_TABLE[j].facade_symbol {
            return Ok(ProbeVerdict::Allowed { api_id: FACADE_TABLE[j].api_id });
        }
        j += 1;
    }
    Ok(ProbeVerdict::Unknown)
}

/// 按 id 直查门面（**O(1)**——id 密集 1..=9 直接下标，锚点「门面 O(1)」口径）。
pub const fn facade_lookup(api_id: u16) -> Option<&'static ApiFacadeEntry> {
    if api_id < FACADE_ID_BASE {
        return None;
    }
    let idx = (api_id - FACADE_ID_BASE) as usize;
    if idx >= FACADE_TABLE.len() {
        return None;
    }
    Some(&FACADE_TABLE[idx])
}

// ---------------------------------------------------------------------------
// 八、十年承诺账（双版期：支持期 3650 日 + 宽限期 730 日，逐 API 起算）
// ---------------------------------------------------------------------------

/// 十年承诺支持期 = 3650 日（十年不变口径：365 日/年 × 10 年，写死——
/// 不闰年不插日：账面日序是承诺口径不是天文口径）。
pub const DECADE_SUPPORT_DAYS: u32 = 3650;
/// 宽限期 = 730 日（双版期第二段：支持期后两年，只修不增）。
pub const GRACE_DAYS: u32 = 730;
/// 双版期总长（支持 + 宽限 = 4380 日——判据按字面量重算 3650+730）。
pub const DECADE_TOTAL_DAYS: u32 = DECADE_SUPPORT_DAYS + GRACE_DAYS;

/// 承诺三态（账面态，逐 API 落账）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CommitmentPhase {
    /// 支持期内：完全承诺（MINOR 兼容增补照常）。
    Committed,
    /// 宽限期内：只修不增（MINOR 增补/MJOR 前进都拦，只留关键修复通道）。
    GraceOnly,
    /// 承诺过期：只读（任何增改都拦 + 事件账留痕）。
    Expired,
}

impl CommitmentPhase {
    /// 态判别值（const——判据断言用索引判等，不依赖非 const 的 PartialEq）。
    pub const fn as_index(self) -> u8 {
        match self {
            CommitmentPhase::Committed => 0,
            CommitmentPhase::GraceOnly => 1,
            CommitmentPhase::Expired => 2,
        }
    }

    /// 态名（读屏口径）。
    pub const fn as_str(self) -> &'static str {
        match self {
            CommitmentPhase::Committed => "承诺期内",
            CommitmentPhase::GraceOnly => "宽限期（只修不增）",
            CommitmentPhase::Expired => "承诺过期（只读）",
        }
    }

    /// 是否还允许任何增改（只有完全承诺期允许）。
    pub const fn allows_change(self) -> bool {
        matches!(self, CommitmentPhase::Committed)
    }
}

/// 承诺期判定（const O(1)，逐 API 按 own 起算日独立判定）：
/// `frozen_day + 3650` 前为支持期，`+ 4380` 前为宽限期，此后过期。
pub const fn commitment_phase(frozen_day: u32, day: u32) -> CommitmentPhase {
    let support_end = frozen_day.saturating_add(DECADE_SUPPORT_DAYS);
    let grace_end = support_end.saturating_add(GRACE_DAYS);
    if day < support_end {
        CommitmentPhase::Committed
    } else if day < grace_end {
        CommitmentPhase::GraceOnly
    } else {
        CommitmentPhase::Expired
    }
}

/// 承诺到期日（起算 + 双版期总长，饱和）。
pub const fn commitment_expiry(frozen_day: u32) -> u32 {
    frozen_day.saturating_add(DECADE_TOTAL_DAYS)
}

// ---------------------------------------------------------------------------
// 九、兼容性门（拦截 O(1) + 留痕）
// ---------------------------------------------------------------------------

/// 插件绑定声明（插件向门面报到：用哪个 API、声明什么版本）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PluginBinding {
    /// 目标 API id。
    pub api_id: u16,
    /// 插件名（留痕可读）。
    pub plugin_name: &'static str,
    /// 插件声明版本（针对该 API 的门面版本所写）。
    pub declared: SemVer,
}

/// 放行裁决（拦截 O(1) 的成功面——带齐判定输入，回执可审计）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GateVerdict {
    /// 目标 API id。
    pub api_id: u16,
    /// 插件声明版本。
    pub declared: SemVer,
    /// 门面当前版本。
    pub facade: SemVer,
    /// 裁决行（读屏口径）。
    pub line: &'static str,
}

/// 拦截留痕（破坏兼容即拦截 + 留痕——留痕是账，不是日志：进容量账）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GateTrace {
    /// 账面日序。
    pub day: u32,
    /// 目标 API id。
    pub api_id: u16,
    /// 插件名。
    pub plugin_name: &'static str,
    /// 插件声明版本。
    pub declared: SemVer,
    /// 门面版本（拦截时的账面值）。
    pub facade: SemVer,
    /// 诊断码。
    pub code: u16,
}

/// 门面留痕容量（满则显性计溢出，不静默丢——溢出进 health 不绿灯）。
pub const GATE_TRACE_CAP: usize = 32;

/// 兼容性门（拦截 O(1)：纯双段比较 + 定长留痕）。
#[derive(Debug)]
pub struct CompatibilityGate {
    /// 放行计数。
    pub passes: u32,
    /// 拦截留痕（定长，满则溢出计数）。
    pub blocked: Vec<GateTrace>,
    /// 留痕溢出计数（>0 即 health 不绿灯）。
    pub overflow: u32,
}

impl CompatibilityGate {
    /// 空门（const——账从零起）。
    pub const fn new() -> CompatibilityGate {
        CompatibilityGate { passes: 0, blocked: Vec::new(), overflow: 0 }
    }

    /// 门面判定（**O(1)**）：
    /// 兼容（MAJOR 相等 + 门面 MINOR ≥ 插件 MINOR）→ 放行 + 计数；
    /// 破坏兼容 → 拦截 + 留痕 + `Err(VersionBlocked)`。
    /// 门面版本由调用方传入冻结账面值——门本身不持账，只判账。
    pub fn gate(
        &mut self,
        binding: PluginBinding,
        facade: SemVer,
        day: u32,
    ) -> Result<GateVerdict, FreezeLayerError> {
        if binding.declared.is_compatible_with(facade) {
            self.passes = self.passes.saturating_add(1);
            Ok(GateVerdict {
                api_id: binding.api_id,
                declared: binding.declared,
                facade,
                line: "兼容放行（MAJOR 相等且门面 MINOR ≥ 插件 MINOR）",
            })
        } else {
            if self.blocked.len() >= GATE_TRACE_CAP {
                self.overflow = self.overflow.saturating_add(1);
            } else {
                self.blocked.push(GateTrace {
                    day,
                    api_id: binding.api_id,
                    plugin_name: binding.plugin_name,
                    declared: binding.declared,
                    facade,
                    code: CODE_VERSION_BLOCKED,
                });
            }
            Err(FreezeLayerError::VersionBlocked {
                api_id: binding.api_id,
                declared: binding.declared,
                facade,
            })
        }
    }

    /// 某 API 的拦截留痕（扫描 O(留痕数)——审计用，非热路径）。
    pub fn traces_for(&self, api_id: u16) -> Vec<GateTrace> {
        let mut out = Vec::new();
        let mut i = 0usize;
        while i < self.blocked.len() {
            if self.blocked[i].api_id == api_id {
                out.push(self.blocked[i]);
            }
            i += 1;
        }
        out
    }
}

// ---------------------------------------------------------------------------
// 十、变更评审（提案 → 影响面分析 → 四态流转；越权→冻结评审+红账）
// ---------------------------------------------------------------------------

/// 变更种类（冻结面只认这三种变更；签名破坏折算 MAJOR 前进）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ChangeKind {
    /// MINOR 兼容增补（MINOR 只增兼容——不破坏，但也必须记账）。
    MinorAdd,
    /// MAJOR 前进 / 签名破坏（破坏兼容——评审强制线）。
    MajorBump,
    /// 承诺修订（改起算日 = 改十年口径——生态敏感，评审强制线）。
    CommitmentAmend,
}

impl ChangeKind {
    /// 种类名（读屏口径）。
    pub const fn as_str(self) -> &'static str {
        match self {
            ChangeKind::MinorAdd => "MINOR 兼容增补",
            ChangeKind::MajorBump => "MAJOR 前进（破坏兼容）",
            ChangeKind::CommitmentAmend => "承诺修订",
        }
    }

    /// 是否破坏兼容（评审强制线：MAJOR 前进与承诺修订都是）。
    pub const fn is_breaking(self) -> bool {
        matches!(self, ChangeKind::MajorBump | ChangeKind::CommitmentAmend)
    }

    /// 是否必须过评审才能落账（全部都必须——MINOR 增补可免评审落账，
    /// 但本字段指「改变账面值」的强制评审线：破坏类）。
    pub const fn requires_review_to_apply(self) -> bool {
        matches!(self, ChangeKind::MajorBump | ChangeKind::CommitmentAmend)
    }
}

/// 评审四态（提交 / 评审中 / 通过 / 否决——流转只有两步，不可跳态）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReviewState {
    /// 已提交（等受理）。
    Submitted,
    /// 评审中（等裁决）。
    UnderReview,
    /// 通过（可落账，一次性消费）。
    Approved,
    /// 否决（终态——提案死账）。
    Rejected,
}

impl ReviewState {
    /// 态名（读屏口径）。
    pub const fn as_str(self) -> &'static str {
        match self {
            ReviewState::Submitted => "已提交",
            ReviewState::UnderReview => "评审中",
            ReviewState::Approved => "通过",
            ReviewState::Rejected => "否决",
        }
    }

    /// 是否终态。
    pub const fn is_terminal(self) -> bool {
        matches!(self, ReviewState::Approved | ReviewState::Rejected)
    }
}

/// 评审裁决动作。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReviewDecision {
    /// 受理（Submitted → UnderReview）。
    Open,
    /// 批（UnderReview → Approved）。
    Approve,
    /// 否（UnderReview → Rejected）。
    Reject,
}

/// 影响面分析（评审 O(变更面)——变更面 = 同类 API 计数 + 破坏性判定）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ImpactReport {
    /// 目标 API id。
    pub api_id: u16,
    /// 变更种类。
    pub kind: ChangeKind,
    /// 同类 API 计数（变更波及面：同类冻结条目全受兼容性约束）。
    pub same_class_count: u32,
    /// 是否破坏兼容（MAJOR 前进 / 承诺修订 → true）。
    pub breaking: bool,
    /// 结论行（读屏口径）。
    pub summary: String,
}

/// 变更评审单（提案 → 影响面 → 四态流转；通过后一次性消费落账）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ChangeReview {
    /// 评审单 id（全账唯一，单调递增）。
    pub review_id: u32,
    /// 目标 API id。
    pub api_id: u16,
    /// 变更种类。
    pub kind: ChangeKind,
    /// 当前态。
    pub state: ReviewState,
    /// 影响面分析（开单时算定，评审随单走）。
    pub impact: ImpactReport,
    /// 提案理由（固定文案——评审单不收自由文本，防注水）。
    pub reason: &'static str,
    /// 开单日（账面日序）。
    pub opened_day: u32,
    /// 结单日（0 = 未结）。
    pub closed_day: u32,
    /// 是否已消费落账（通过后一次性——防止同一单反复改面）。
    pub consumed: bool,
}

/// 评审账容量（满则显性拒收，不静默丢提案）。
pub const REVIEW_CAP: usize = 32;

// ---------------------------------------------------------------------------
// 十一、语义版本登记 + 事件账（定长账簿，满则溢出计数）
// ---------------------------------------------------------------------------

/// 版本登记记录（语义版本登记：从 → 到、账日、经何单评审）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct VersionRecord {
    /// 目标 API id。
    pub api_id: u16,
    /// 变更前版本。
    pub from: SemVer,
    /// 变更后版本。
    pub to: SemVer,
    /// 落账日。
    pub day: u32,
    /// 经何评审单（0 = MINOR 兼容增补免评审通道）。
    pub via_review: u32,
}

/// 版本账容量。
pub const VERSION_CAP: usize = 32;

/// 事件种类（冻结层全部要害动作——每动作必留一笔，账全可审计）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EventKind {
    /// 启动登记（九条冻结条目入账）。
    Boot,
    /// 门面放行。
    GatePass,
    /// 门面拦截（破坏兼容）。
    GateBlock,
    /// 外泄拦截。
    LeakBlock,
    /// 评审开单。
    ReviewOpened,
    /// 评审受理。
    ReviewOpenedReview,
    /// 评审通过。
    ReviewApproved,
    /// 评审否决。
    ReviewRejected,
    /// 冻结面已变更（指纹必变——账面在先，检测在后）。
    SurfaceChanged,
    /// MAJOR 前进落账。
    MajorBumped,
    /// MINOR 增补落账。
    MinorAdded,
    /// 承诺修订落账。
    CommitmentAmended,
    /// 承诺过期落账（到期检查发现）。
    CommitExpired,
    /// 越权拦截（未过评审改面 → 红账）。
    UnauthorizedBlock,
}

impl EventKind {
    /// 事件名（读屏口径）。
    pub const fn as_str(self) -> &'static str {
        match self {
            EventKind::Boot => "启动登记",
            EventKind::GatePass => "门面放行",
            EventKind::GateBlock => "门面拦截",
            EventKind::LeakBlock => "外泄拦截",
            EventKind::ReviewOpened => "评审开单",
            EventKind::ReviewOpenedReview => "评审受理",
            EventKind::ReviewApproved => "评审通过",
            EventKind::ReviewRejected => "评审否决",
            EventKind::SurfaceChanged => "冻结面变更",
            EventKind::MajorBumped => "MAJOR 前进",
            EventKind::MinorAdded => "MINOR 增补",
            EventKind::CommitmentAmended => "承诺修订",
            EventKind::CommitExpired => "承诺过期",
            EventKind::UnauthorizedBlock => "越权拦截",
        }
    }
}

/// 事件记录（定长字段——事件账不进堆字符串，账簿可拷贝可对拍）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EventRecord {
    /// 全账序号（单调递增，饱和）。
    pub seq: u32,
    /// 事件种类。
    pub kind: EventKind,
    /// 账面日序。
    pub day: u32,
    /// 目标 API id（0 = 全局面事件）。
    pub api_id: u16,
    /// 附注（固定文案）。
    pub note: &'static str,
}

/// 事件账容量。
pub const EVENT_CAP: usize = 128;

// ---------------------------------------------------------------------------
// 十二、冻结层总控（登记簿 + 门 + 评审 + 版本账 + 事件账 + 指纹基线）
// ---------------------------------------------------------------------------

/// 冻结层总控——VE-F4607 的运行体：所有纪律在账上执行，不在人心里执行。
#[derive(Debug)]
pub struct FreezeLayer {
    /// 冻结登记簿（boot 时由种子表建账，此后只按纪律改）。
    pub ledger: Vec<FrozenApi>,
    /// 兼容性门（拦截留痕在门上）。
    pub gate: CompatibilityGate,
    /// 评审账。
    pub reviews: Vec<ChangeReview>,
    /// 版本登记账。
    pub versions: Vec<VersionRecord>,
    /// 事件账。
    pub events: Vec<EventRecord>,
    /// 事件账溢出计数。
    pub event_overflow: u32,
    /// 评审账溢出计数。
    pub review_overflow: u32,
    /// 版本账溢出计数。
    pub version_overflow: u32,
    /// 红账计数（越权拦截每次 +1——红账只增不减，health 硬指标）。
    pub red_flags: u32,
    /// 冻结基线指纹（boot 时算定后**永不再写**——基线 = 已对外承诺的冻结面；
    /// 一切变更靠「指纹差异 + 事件账」检测：现值 != 基线 必须有账，无账即漂移事故）。
    pub frozen_fingerprint: u32,
    /// 全账单调序号（评审单 id 等从这取）。
    pub seq: u32,
    /// 启动日（账面日序）。
    pub boot_day: u32,
}

impl FreezeLayer {
    /// 建账（boot）：种子表逐条登记——类别闭集校验、id 去重、签名指纹算定、
    /// 基线指纹算定、启动事件落账。任何校验不过 → 建账失败（账不半建）。
    pub fn boot(day: u32) -> Result<FreezeLayer, FreezeLayerError> {
        let mut ledger: Vec<FrozenApi> = Vec::new();
        let mut i = 0usize;
        while i < FROZEN_API_SEED.len() {
            let seed = &FROZEN_API_SEED[i];
            // 类别闭集校验（闭集枚举天然在集内——from_index 反证口径仍跑一遍）。
            if ApiClass::from_index(seed.class.as_index()).is_none() {
                return Err(FreezeLayerError::ClassInvalid(seed.class.as_index()));
            }
            // id 去重（O(n²)，n=9 建账一次——账不撞）。
            let mut j = 0usize;
            while j < ledger.len() {
                if ledger[j].api_id == seed.api_id {
                    return Err(FreezeLayerError::DuplicateApi(seed.api_id));
                }
                j += 1;
            }
            ledger.push(FrozenApi {
                api_id: seed.api_id,
                name: seed.name,
                class: seed.class,
                facade_symbol: seed.facade_symbol,
                version: seed.version,
                frozen_day: seed.frozen_day,
                sig_fnv: signature_fingerprint(seed.name, seed.facade_symbol),
                commitment: CommitmentPhase::Committed,
            });
            i += 1;
        }
        let baseline = surface_fingerprint(&ledger);
        let mut layer = FreezeLayer {
            ledger,
            gate: CompatibilityGate::new(),
            reviews: Vec::new(),
            versions: Vec::new(),
            events: Vec::new(),
            event_overflow: 0,
            review_overflow: 0,
            version_overflow: 0,
            red_flags: 0,
            frozen_fingerprint: baseline,
            seq: 0,
            boot_day: day,
        };
        // 承诺初检：建账日就过期的条目直接落账过期态（账实相符，不装年轻）。
        let mut k = 0usize;
        while k < layer.ledger.len() {
            let phase = commitment_phase(layer.ledger[k].frozen_day, day);
            layer.ledger[k].commitment = phase;
            k += 1;
        }
        layer.push_event(EventKind::Boot, 0, day, "冻结登记簿建账（三类九条逐一冻结）");
        Ok(layer)
    }

    /// 序号取号（饱和——永不回绕）。
    fn take_seq(&mut self) -> u32 {
        self.seq = self.seq.saturating_add(1);
        self.seq
    }

    /// 事件落账（定长，满则溢出计数——不静默丢）。
    fn push_event(&mut self, kind: EventKind, api_id: u16, day: u32, note: &'static str) {
        if self.events.len() >= EVENT_CAP {
            self.event_overflow = self.event_overflow.saturating_add(1);
            return;
        }
        let seq = self.take_seq();
        self.events.push(EventRecord { seq, kind, day, api_id, note });
    }

    /// 登记簿定位（O(API 数)——审计路径；热路径用 facade_lookup O(1)）。
    fn ledger_index(&self, api_id: u16) -> Result<usize, FreezeLayerError> {
        let mut i = 0usize;
        while i < self.ledger.len() {
            if self.ledger[i].api_id == api_id {
                return Ok(i);
            }
            i += 1;
        }
        Err(FreezeLayerError::ApiNotFound(api_id))
    }

    /// 查冻结条目（审计读账路径）。
    pub fn query(&self, api_id: u16) -> Result<&FrozenApi, FreezeLayerError> {
        let idx = self.ledger_index(api_id)?;
        Ok(&self.ledger[idx])
    }

    /// 门面过门（插件绑定 → 判定 → 放行/拦截留痕 + 事件账）。
    /// 拦截 O(1)（锚点口径）；未知 API 先于版本判定被拒。
    pub fn admit(
        &mut self,
        binding: PluginBinding,
        day: u32,
    ) -> Result<GateVerdict, FreezeLayerError> {
        let facade_ver = self.query(binding.api_id)?.version;
        match self.gate.gate(binding, facade_ver, day) {
            Ok(v) => {
                self.push_event(EventKind::GatePass, binding.api_id, day, "门面兼容放行");
                Ok(v)
            }
            Err(e) => {
                self.push_event(EventKind::GateBlock, binding.api_id, day, "破坏兼容拦截留痕");
                Err(e)
            }
        }
    }

    /// 开评审单（评审 O(变更面)——影响面分析扫同类 API 计数）。
    pub fn open_review(
        &mut self,
        api_id: u16,
        kind: ChangeKind,
        reason: &'static str,
        day: u32,
    ) -> Result<u32, FreezeLayerError> {
        let idx = self.ledger_index(api_id)?;
        let class = self.ledger[idx].class;
        // 影响面分析：同类 API 计数（变更面 = 同类冻结条目全受兼容约束）。
        let mut same_class = 0u32;
        let mut j = 0usize;
        while j < self.ledger.len() {
            if self.ledger[j].class == class {
                same_class = same_class.saturating_add(1);
            }
            j += 1;
        }
        if self.reviews.len() >= REVIEW_CAP {
            self.review_overflow = self.review_overflow.saturating_add(1);
            return Err(FreezeLayerError::LedgerFull { cap: REVIEW_CAP, which: "评审账" });
        }
        let review_id = self.take_seq();
        let mut summary = String::new();
        summary.push_str("影响面：");
        summary.push_str(class.as_str());
        summary.push_str("类共 ");
        summary.push_str(&same_class.to_string());
        summary.push_str(" 条冻结面；变更=");
        summary.push_str(kind.as_str());
        summary.push_str(kind_break_line(kind));
        let impact = ImpactReport { api_id, kind, same_class_count: same_class, breaking: kind.is_breaking(), summary };
        self.reviews.push(ChangeReview {
            review_id,
            api_id,
            kind,
            state: ReviewState::Submitted,
            impact,
            reason,
            opened_day: day,
            closed_day: 0,
            consumed: false,
        });
        self.push_event(EventKind::ReviewOpened, api_id, day, "变更评审单开立");
        Ok(review_id)
    }

    /// 评审流转（提交 → 评审中 → 通过/否决；跳态即拒）。
    pub fn advance_review(
        &mut self,
        review_id: u32,
        decision: ReviewDecision,
        day: u32,
    ) -> Result<ReviewState, FreezeLayerError> {
        let mut idx = self.reviews.len();
        let mut i = 0usize;
        while i < self.reviews.len() {
            if self.reviews[i].review_id == review_id {
                idx = i;
            }
            i += 1;
        }
        if idx == self.reviews.len() {
            return Err(FreezeLayerError::ReviewBlocked { review_id, why: "评审单不存在" });
        }
        let cur = self.reviews[idx].state;
        // 四态状态机：只有两条合法前向边 + 两条终态边，其余全拒。
        let next = match (cur, decision) {
            (ReviewState::Submitted, ReviewDecision::Open) => ReviewState::UnderReview,
            (ReviewState::UnderReview, ReviewDecision::Approve) => ReviewState::Approved,
            (ReviewState::UnderReview, ReviewDecision::Reject) => ReviewState::Rejected,
            (s, _) if s.is_terminal() => {
                return Err(FreezeLayerError::ReviewBlocked {
                    review_id,
                    why: "评审单已终态，不可再流转",
                });
            }
            _ => {
                return Err(FreezeLayerError::ReviewBlocked {
                    review_id,
                    why: "流转跳态被拒（必须 提交→评审中→通过/否决）",
                });
            }
        };
        self.reviews[idx].state = next;
        if next.is_terminal() {
            self.reviews[idx].closed_day = day;
        }
        let ev = match next {
            ReviewState::UnderReview => EventKind::ReviewOpenedReview,
            ReviewState::Approved => EventKind::ReviewApproved,
            ReviewState::Rejected => EventKind::ReviewRejected,
            ReviewState::Submitted => EventKind::ReviewOpened,
        };
        self.push_event(ev, self.reviews[idx].api_id, day, "评审流转落账");
        Ok(next)
    }

    /// 取已批未消费的评审单（落账前置检查——一单一消费）。
    fn approved_unconsumed_index(&self, api_id: u16, kind: ChangeKind) -> Option<usize> {
        let mut found: Option<usize> = None;
        let mut i = 0usize;
        while i < self.reviews.len() {
            let r = &self.reviews[i];
            if r.api_id == api_id && r.kind == kind && r.state == ReviewState::Approved && !r.consumed {
                found = Some(i);
            }
            i += 1;
        }
        found
    }

    /// MINOR 兼容增补落账（MINOR 只增兼容——免评审通道，但必留版本账 + 事件账，
    /// 指纹必变可检测）。承诺期外（宽限/过期）一律拦。
    pub fn apply_minor_add(&mut self, api_id: u16, day: u32) -> Result<SemVer, FreezeLayerError> {
        let idx = self.ledger_index(api_id)?;
        let phase = commitment_phase(self.ledger[idx].frozen_day, day);
        if !phase.allows_change() {
            return Err(FreezeLayerError::CommitmentExpired { api_id, day });
        }
        let from = self.ledger[idx].version;
        let to = from.bump_minor();
        if self.versions.len() >= VERSION_CAP {
            self.version_overflow = self.version_overflow.saturating_add(1);
            return Err(FreezeLayerError::LedgerFull { cap: VERSION_CAP, which: "版本账" });
        }
        self.ledger[idx].version = to;
        self.versions.push(VersionRecord { api_id, from, to, day, via_review: 0 });
        self.push_event(EventKind::MinorAdded, api_id, day, "MINOR 兼容增补落账");
        self.push_event(EventKind::SurfaceChanged, api_id, day, "冻结面已变（指纹必变）");
        Ok(to)
    }

    /// MAJOR 前进落账（破坏兼容必须走评审——无已批未消费的 MajorBump 单
    /// 即拦截 + 红账；有单则消费落账，版本账留评审单号）。
    pub fn apply_major_bump(&mut self, api_id: u16, day: u32) -> Result<SemVer, FreezeLayerError> {
        let idx = self.ledger_index(api_id)?;
        let phase = commitment_phase(self.ledger[idx].frozen_day, day);
        if !phase.allows_change() {
            return Err(FreezeLayerError::CommitmentExpired { api_id, day });
        }
        let ridx = match self.approved_unconsumed_index(api_id, ChangeKind::MajorBump) {
            Some(r) => r,
            None => {
                // 越权变更 → 拦截 + 红账（锚点降级矩阵第三行）。
                self.red_flags = self.red_flags.saturating_add(1);
                self.push_event(EventKind::UnauthorizedBlock, api_id, day, "未过评审改冻结面——红账");
                return Err(FreezeLayerError::UnauthorizedChange {
                    api_id,
                    why: "MAJOR 前进必须有已通过的 MajorBump 评审单",
                });
            }
        };
        let from = self.ledger[idx].version;
        let to = from.bump_major();
        if self.versions.len() >= VERSION_CAP {
            self.version_overflow = self.version_overflow.saturating_add(1);
            return Err(FreezeLayerError::LedgerFull { cap: VERSION_CAP, which: "版本账" });
        }
        let review_id = self.reviews[ridx].review_id;
        self.reviews[ridx].consumed = true;
        self.ledger[idx].version = to;
        self.versions.push(VersionRecord { api_id, from, to, day, via_review: review_id });
        self.push_event(EventKind::MajorBumped, api_id, day, "MAJOR 前进落账（评审已过）");
        self.push_event(EventKind::SurfaceChanged, api_id, day, "冻结面已变（指纹必变）");
        Ok(to)
    }

    /// 承诺修订落账（改起算日 = 改十年口径——必须有已批 CommitmentAmend 单）。
    pub fn amend_commitment(&mut self, api_id: u16, new_frozen_day: u32, day: u32) -> Result<u32, FreezeLayerError> {
        let idx = self.ledger_index(api_id)?;
        let ridx = match self.approved_unconsumed_index(api_id, ChangeKind::CommitmentAmend) {
            Some(r) => r,
            None => {
                self.red_flags = self.red_flags.saturating_add(1);
                self.push_event(EventKind::UnauthorizedBlock, api_id, day, "未过评审改承诺期——红账");
                return Err(FreezeLayerError::UnauthorizedChange {
                    api_id,
                    why: "承诺修订必须有已通过的 CommitmentAmend 评审单",
                });
            }
        };
        let review_id = self.reviews[ridx].review_id;
        self.reviews[ridx].consumed = true;
        self.ledger[idx].frozen_day = new_frozen_day;
        self.ledger[idx].commitment = commitment_phase(new_frozen_day, day);
        self.push_event(EventKind::CommitmentAmended, api_id, day, "承诺起算日修订落账（评审已过）");
        self.push_event(EventKind::SurfaceChanged, api_id, day, "冻结面已变（指纹必变）");
        Ok(review_id)
    }

    /// 到期检查（O(API 数)）：按起算日独立判定三态，态变化才落账。
    /// 宽限期内条目保持只修不增纪律（allows_change 已拦增改）。
    pub fn expire_sweep(&mut self, day: u32) -> u32 {
        let mut newly_expired = 0u32;
        let mut i = 0usize;
        while i < self.ledger.len() {
            let frozen_day = self.ledger[i].frozen_day;
            let phase = commitment_phase(frozen_day, day);
            if phase == CommitmentPhase::Expired && self.ledger[i].commitment != CommitmentPhase::Expired {
                newly_expired = newly_expired.saturating_add(1);
                let api_id = self.ledger[i].api_id;
                self.push_event(EventKind::CommitExpired, api_id, day, "承诺到期（双版期走完）");
            }
            self.ledger[i].commitment = phase;
            i += 1;
        }
        newly_expired
    }

    /// 当前冻结面指纹（O(API 数)）。
    pub fn fingerprint(&self) -> u32 {
        surface_fingerprint(&self.ledger)
    }

    /// 冻结面审计（指纹纪律的机制化）：
    /// * 现值 == 基线 → `Stable`（承诺期内指纹不变成立）；
    /// * 现值 != 基线 → 必须有账（SurfaceChanged 事件在案）→ `ChangedWithLedger`；
    /// * 现值 != 基线 且无账 → `Err(FingerprintDrift)`（有变更无账 = 事故）。
    pub fn audit_surface(&self) -> Result<SurfaceAudit, FreezeLayerError> {
        let cur = self.fingerprint();
        if cur == self.frozen_fingerprint {
            return Ok(SurfaceAudit::Stable);
        }
        let mut i = 0usize;
        while i < self.events.len() {
            if self.events[i].kind == EventKind::SurfaceChanged {
                return Ok(SurfaceAudit::ChangedWithLedger);
            }
            i += 1;
        }
        Err(FreezeLayerError::FingerprintDrift { current: cur, baseline: self.frozen_fingerprint })
    }

    /// 健康度（O(事件数) 扫账统计——红账/溢出/过期/指纹全进表）。
    pub fn health(&self) -> HealthReport {
        let mut gate_passes = 0u32;
        let mut gate_blocks = 0u32;
        let mut leak_blocks = 0u32;
        let mut unauthorized = 0u32;
        let mut expired_events = 0u32;
        let mut i = 0usize;
        while i < self.events.len() {
            match self.events[i].kind {
                EventKind::GatePass => gate_passes = gate_passes.saturating_add(1),
                EventKind::GateBlock => gate_blocks = gate_blocks.saturating_add(1),
                EventKind::LeakBlock => leak_blocks = leak_blocks.saturating_add(1),
                EventKind::UnauthorizedBlock => unauthorized = unauthorized.saturating_add(1),
                EventKind::CommitExpired => expired_events = expired_events.saturating_add(1),
                _ => {}
            }
            i += 1;
        }
        let mut expired_apis = 0u32;
        let mut j = 0usize;
        while j < self.ledger.len() {
            if self.ledger[j].commitment == CommitmentPhase::Expired {
                expired_apis = expired_apis.saturating_add(1);
            }
            j += 1;
        }
        let surface = match self.audit_surface() {
            Ok(SurfaceAudit::Stable) => SurfaceAuditState::Stable,
            Ok(SurfaceAudit::ChangedWithLedger) => SurfaceAuditState::ChangedWithLedger,
            Err(_) => SurfaceAuditState::Unaccounted,
        };
        HealthReport {
            api_count: self.ledger.len() as u32,
            gate_passes,
            gate_blocks,
            leak_blocks,
            unauthorized,
            red_flags: self.red_flags,
            expired_apis,
            expired_events,
            event_records: self.events.len() as u32,
            event_overflow: self.event_overflow,
            review_overflow: self.review_overflow,
            version_overflow: self.version_overflow,
            gate_overflow: self.gate.overflow,
            surface,
        }
    }

    /// 健康度 digest（双跑对拍的比较面——所有要害计数都进指纹）。
    pub fn digest(&self) -> u32 {
        let h = self.health();
        let mut acc = FNV_OFFSET;
        acc = fnv_fold_u32(acc, h.api_count);
        acc = fnv_fold_u32(acc, h.gate_passes);
        acc = fnv_fold_u32(acc, h.gate_blocks);
        acc = fnv_fold_u32(acc, h.leak_blocks);
        acc = fnv_fold_u32(acc, h.unauthorized);
        acc = fnv_fold_u32(acc, h.red_flags);
        acc = fnv_fold_u32(acc, h.expired_apis);
        acc = fnv_fold_u32(acc, h.event_records);
        acc = fnv_fold_u32(acc, h.event_overflow);
        acc = fnv_fold_u32(acc, h.review_overflow);
        acc = fnv_fold_u32(acc, h.version_overflow);
        acc = fnv_fold_u32(acc, h.gate_overflow);
        acc = fnv_fold_u32(acc, h.surface as u32);
        acc
    }

    /// 签名对账（O(API 数)）：每条目现签名指纹与登记指纹比对——
    /// 登记簿被外部手改（不改指纹）也会被这项抓出来。
    pub fn audit_signatures(&self) -> Result<(), FreezeLayerError> {
        let mut i = 0usize;
        while i < self.ledger.len() {
            let a = &self.ledger[i];
            let expect = signature_fingerprint(a.name, a.facade_symbol);
            if a.sig_fnv != expect {
                return Err(FreezeLayerError::SignatureDrift { api_id: a.api_id });
            }
            i += 1;
        }
        Ok(())
    }

    // -- 注入面（三个注入各检出：外泄 / 破坏兼容 / 越权变更） ----------------

    /// 注入 1：把内部符号喂给门面探针——外泄必须被拒（FacadeLeak）。
    /// 返回 `Ok(true)` = 已检出拦截；`Ok(false)` = 未检出（防线失效）。
    pub fn inject_leak(&mut self, day: u32) -> Result<bool, FreezeLayerError> {
        match probe_symbol(INTERNAL_SYMBOLS[0]) {
            Err(FreezeLayerError::FacadeLeak(_)) => {
                self.push_event(EventKind::LeakBlock, 0, day, "注入外泄被拒（内部符号命中）");
                Ok(true)
            }
            Ok(_) => Ok(false),
            Err(e) => Err(e),
        }
    }

    /// 注入 2：声明 MAJOR 不兼容的插件版本——破坏兼容必须被拦（VersionBlocked）。
    pub fn inject_incompat(&mut self, day: u32) -> Result<bool, FreezeLayerError> {
        let binding = PluginBinding {
            api_id: FACADE_ID_BASE,
            plugin_name: "injector_incompat",
            declared: SemVer::new(9, 0),
        };
        match self.admit(binding, day) {
            Err(FreezeLayerError::VersionBlocked { .. }) => Ok(true),
            Ok(_) => Ok(false),
            Err(e) => Err(e),
        }
    }

    /// 注入 3：无评审单直接 MAJOR 前进——越权必须被拦（UnauthorizedChange + 红账）。
    pub fn inject_unauth(&mut self, day: u32) -> Result<bool, FreezeLayerError> {
        match self.apply_major_bump(FACADE_ID_BASE, day) {
            Err(FreezeLayerError::UnauthorizedChange { .. }) => Ok(true),
            Ok(_) => Ok(false),
            Err(e) => Err(e),
        }
    }

    // -- 自检与面板 -----------------------------------------------------------

    /// 自检（判据逐项跑）：门面符号全可查、内部符号全被拒、自版本全放行、
    /// 指纹有账、签名全对账。返回通过数 / 总数；任何意外错误原样上抛。
    pub fn run_self_checks(&mut self, day: u32) -> Result<SelfCheckReport, FreezeLayerError> {
        let mut checks = 0u32;
        let mut passed = 0u32;
        // 1. 门面符号全可查（九条正路全通）。
        let mut i = 0usize;
        while i < FACADE_TABLE.len() {
            checks = checks.saturating_add(1);
            if matches!(probe_symbol(FACADE_TABLE[i].facade_symbol)?, ProbeVerdict::Allowed { .. }) {
                passed = passed.saturating_add(1);
            }
            i += 1;
        }
        // 2. 内部符号全被拒（外泄零容忍）。
        let mut j = 0usize;
        while j < INTERNAL_SYMBOLS.len() {
            checks = checks.saturating_add(1);
            match probe_symbol(INTERNAL_SYMBOLS[j]) {
                Err(FreezeLayerError::FacadeLeak(_)) => passed = passed.saturating_add(1),
                Ok(_) => {}
                Err(e) => return Err(e),
            }
            j += 1;
        }
        // 3. 每条 API 用自身版本过门——全放行（门不误拦账面版本）。
        let mut k = 0usize;
        while k < self.ledger.len() {
            checks = checks.saturating_add(1);
            let b = PluginBinding {
                api_id: self.ledger[k].api_id,
                plugin_name: "self_check",
                declared: self.ledger[k].version,
            };
            if self.admit(b, day).is_ok() {
                passed = passed.saturating_add(1);
            }
            k += 1;
        }
        // 4. 指纹有账（Stable 或 ChangedWithLedger 都算有账）。
        checks = checks.saturating_add(1);
        if self.audit_surface().is_ok() {
            passed = passed.saturating_add(1);
        }
        // 5. 签名全对账。
        checks = checks.saturating_add(1);
        if self.audit_signatures().is_ok() {
            passed = passed.saturating_add(1);
        }
        Ok(SelfCheckReport { checks, passed })
    }

    /// 承诺账面板（每条 API 一行：id/名/版本/起算/到期/态——读屏口径）。
    pub fn commitment_board(&self, day: u32) -> Vec<String> {
        let mut out = Vec::new();
        let mut i = 0usize;
        while i < self.ledger.len() {
            let a = &self.ledger[i];
            let mut line = String::new();
            line.push_str("#");
            line.push_str(&a.api_id.to_string());
            line.push_str(" ");
            line.push_str(a.name);
            line.push_str(" ");
            line.push_str(&a.version.as_line());
            line.push_str(" 起算=");
            line.push_str(&a.frozen_day.to_string());
            line.push_str(" 到期=");
            line.push_str(&commitment_expiry(a.frozen_day).to_string());
            line.push_str(" 现态=");
            line.push_str(commitment_phase(a.frozen_day, day).as_str());
            out.push(line);
            i += 1;
        }
        out
    }
}

/// kind → 摘要尾注（破坏性标记固定文案）。
const fn kind_break_line(kind: ChangeKind) -> &'static str {
    if kind.is_breaking() {
        "（破坏兼容——评审强制线）"
    } else {
        "（兼容增补）"
    }
}

/// 冻结面审计结论。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SurfaceAudit {
    /// 指纹与基线一致——承诺期内指纹不变成立。
    Stable,
    /// 指纹变了但有变更账（SurfaceChanged 在案）——变更可检测且有账。
    ChangedWithLedger,
}

impl SurfaceAudit {
    /// 结论名（读屏口径）。
    pub const fn as_str(self) -> &'static str {
        match self {
            SurfaceAudit::Stable => "指纹稳定",
            SurfaceAudit::ChangedWithLedger => "指纹变更（有账）",
        }
    }
}

/// 健康度表的指纹态（三态含「有变更无账」的坏态）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SurfaceAuditState {
    /// 指纹稳定。
    Stable,
    /// 指纹变更但有账。
    ChangedWithLedger,
    /// 有变更无账（坏态——health 不绿灯）。
    Unaccounted,
}

/// 健康度报告（冻结层一屏看全：红账/溢出/过期/指纹态全在表）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct HealthReport {
    /// 冻结条目数。
    pub api_count: u32,
    /// 门面放行数。
    pub gate_passes: u32,
    /// 门面拦截数。
    pub gate_blocks: u32,
    /// 外泄拦截数。
    pub leak_blocks: u32,
    /// 越权拦截数。
    pub unauthorized: u32,
    /// 红账数（>0 即不绿）。
    pub red_flags: u32,
    /// 过期条目数。
    pub expired_apis: u32,
    /// 过期事件数。
    pub expired_events: u32,
    /// 事件账笔数。
    pub event_records: u32,
    /// 事件账溢出。
    pub event_overflow: u32,
    /// 评审账溢出。
    pub review_overflow: u32,
    /// 版本账溢出。
    pub version_overflow: u32,
    /// 留痕溢出。
    pub gate_overflow: u32,
    /// 指纹态。
    pub surface: SurfaceAuditState,
}

impl HealthReport {
    /// 绿灯判定：零红账、零溢出、指纹有账（过期条目是账实，不算坏账）。
    pub const fn ok(&self) -> bool {
        self.red_flags == 0
            && self.event_overflow == 0
            && self.review_overflow == 0
            && self.version_overflow == 0
            && self.gate_overflow == 0
            && !matches!(self.surface, SurfaceAuditState::Unaccounted)
    }

    /// 摘要行（读屏口径）。
    pub fn as_line(&self) -> String {
        let mut s = String::new();
        s.push_str("apis=");
        s.push_str(&self.api_count.to_string());
        s.push_str(" gate_pass=");
        s.push_str(&self.gate_passes.to_string());
        s.push_str(" gate_block=");
        s.push_str(&self.gate_blocks.to_string());
        s.push_str(" leak_block=");
        s.push_str(&self.leak_blocks.to_string());
        s.push_str(" unauth=");
        s.push_str(&self.unauthorized.to_string());
        s.push_str(" red=");
        s.push_str(&self.red_flags.to_string());
        s.push_str(" expired=");
        s.push_str(&self.expired_apis.to_string());
        s.push_str(" surface=");
        s.push_str(match self.surface {
            SurfaceAuditState::Stable => "稳定",
            SurfaceAuditState::ChangedWithLedger => "有账变更",
            SurfaceAuditState::Unaccounted => "无账变更",
        });
        s
    }
}

/// 自检报告。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SelfCheckReport {
    /// 总检查项。
    pub checks: u32,
    /// 通过项。
    pub passed: u32,
}

impl SelfCheckReport {
    /// 全绿判定。
    pub const fn all_green(&self) -> bool {
        self.checks > 0 && self.checks == self.passed
    }
}

// ---------------------------------------------------------------------------
// 十三、双跑确定性对拍（同脚本两实例 digest 全等）
// ---------------------------------------------------------------------------

/// 固定对拍脚本（两个实例各跑一遍——任何隐藏可变态都会让 digest 分叉）。
pub fn run_scenario(layer: &mut FreezeLayer, day: u32) -> Result<u32, FreezeLayerError> {
    // 三路注入各检出（检出为预期路径，不检出才会分叉失败）。
    let l = layer.inject_leak(day)?;
    let c = layer.inject_incompat(day)?;
    let u = layer.inject_unauth(day)?;
    // 一单评审全流转并落一笔 MINOR 兼容增补。
    let rid = layer.open_review(FACADE_ID_BASE + 1, ChangeKind::MinorAdd, "对拍脚本：MINOR 增补", day)?;
    layer.advance_review(rid, ReviewDecision::Open, day)?;
    layer.advance_review(rid, ReviewDecision::Approve, day)?;
    layer.apply_minor_add(FACADE_ID_BASE + 1, day)?;
    // 到期检查扫一遍（日序同天，应零新增过期）。
    let _ = layer.expire_sweep(day);
    // digest 折入日序与三路注入的检出结果（同日序必全等；异日序必分叉）。
    let mut acc = layer.digest();
    acc = fnv_fold_u32(acc, day);
    acc = fnv_fold_byte(acc, if l { 1 } else { 0 });
    acc = fnv_fold_byte(acc, if c { 1 } else { 0 });
    acc = fnv_fold_byte(acc, if u { 1 } else { 0 });
    Ok(acc)
}

/// 双跑对拍报告。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DualRunReport {
    /// 两实例一致的 digest。
    pub digest: u32,
    /// 实例 A 事件笔数。
    pub a_events: u32,
    /// 实例 B 事件笔数。
    pub b_events: u32,
}

/// 双跑确定性对拍：同日序建账两实例 → 同脚本 → digest 必须全等。
/// 不等 → `Err(DualRunMismatch)`（确定性是冻结承诺的地基——账必须可重放）。
pub fn dual_run_check(day: u32) -> Result<DualRunReport, FreezeLayerError> {
    let mut a = FreezeLayer::boot(day)?;
    let mut b = FreezeLayer::boot(day)?;
    let da = run_scenario(&mut a, day)?;
    let db = run_scenario(&mut b, day)?;
    if da != db {
        return Err(FreezeLayerError::DualRunMismatch { lhs: da, rhs: db });
    }
    Ok(DualRunReport {
        digest: da,
        a_events: a.events.len() as u32,
        b_events: b.events.len() as u32,
    })
}

/// 面板摘要行（域本色一行——面板/日志共用）。
pub fn screen_line() -> String {
    let mut s = String::new();
    s.push_str("VEV09-apifreeze facade=O(1)-by-id gate=O(1) review=O(face) decade=3650+730d fp=fnv1a32");
    s
}

// ---------------------------------------------------------------------------
// 十四、判据的编译期断言（const 校验——账错在编译期就炸，不进运行期）
// ---------------------------------------------------------------------------

/// 全部诊断码两两互异（const 双重循环——判据 6.1 口径）。
const fn codes_all_distinct() -> bool {
    let mut i = 0usize;
    while i < ALL_CODES.len() {
        let mut j = i + 1;
        while j < ALL_CODES.len() {
            if ALL_CODES[i] == ALL_CODES[j] {
                return false;
            }
            j += 1;
        }
        i += 1;
    }
    true
}

/// 全部诊断码落在本层独占段 0x1BC0..=0x1BCF。
const fn codes_in_segment() -> bool {
    let mut i = 0usize;
    while i < ALL_CODES.len() {
        let c = ALL_CODES[i];
        if (c & 0xFF00) != 0x1B00 || c < SEG_BASE || c >= SEG_END {
            return false;
        }
        i += 1;
    }
    true
}

/// 种子表 id 密集（1..=9 无洞——门面直查 O(1) 的前提）。
const fn seed_ids_dense() -> bool {
    let mut i = 0usize;
    while i < FROZEN_API_SEED.len() {
        if FROZEN_API_SEED[i].api_id != (FACADE_ID_BASE as usize + i) as u16 {
            return false;
        }
        i += 1;
    }
    true
}

/// 种子表与门面表逐条对齐（id/符号/名/类全同——账表一致）。
const fn seed_facade_aligned() -> bool {
    if FROZEN_API_SEED.len() != FACADE_TABLE.len() {
        return false;
    }
    let mut i = 0usize;
    while i < FROZEN_API_SEED.len() {
        let s = &FROZEN_API_SEED[i];
        let f = &FACADE_TABLE[i];
        if s.api_id != f.api_id {
            return false;
        }
        if !str_eq(s.facade_symbol, f.facade_symbol) {
            return false;
        }
        if !str_eq(s.name, f.name) {
            return false;
        }
        if s.class.as_index() != f.class.as_index() {
            return false;
        }
        i += 1;
    }
    true
}

/// 门面表与内部符号表零交集（门面隔离的负样本面不许重叠）。
const fn facade_internal_disjoint() -> bool {
    let mut i = 0usize;
    while i < FACADE_TABLE.len() {
        let mut j = 0usize;
        while j < INTERNAL_SYMBOLS.len() {
            if str_eq(FACADE_TABLE[i].facade_symbol, INTERNAL_SYMBOLS[j]) {
                return false;
            }
            j += 1;
        }
        i += 1;
    }
    true
}

/// 双版期口径（3650 + 730 = 4380——判据按字面量重算）。
const _: () = assert!(DECADE_SUPPORT_DAYS == 3650, "支持期必须是 3650 日（十年口径）");
const _: () = assert!(GRACE_DAYS == 730, "宽限期必须是 730 日（两年口径）");
const _: () = assert!(DECADE_TOTAL_DAYS == 4380, "双版期总长必须 = 3650 + 730");
/// 诊断码段判据（互异 + 段内）。
const _: () = assert!(codes_all_distinct(), "诊断码必须两两互异");
const _: () = assert!(codes_in_segment(), "诊断码必须全部落在 0x1BC0..=0x1BCF 独占段");
/// 登记簿判据（id 密集 + 账表对齐）。
const _: () = assert!(seed_ids_dense(), "种子表 id 必须密集 1..=9（直查 O(1) 前提）");
const _: () = assert!(seed_facade_aligned(), "种子表必须与门面表逐条对齐");
const _: () = assert!(API_COUNT == 9, "冻结 API 数判据按字面量重算为 9");
/// 门面隔离判据（门面/内部零交集）。
const _: () = assert!(facade_internal_disjoint(), "门面符号与内部符号必须零交集");
/// 定长账簿容量判据（都是 2 的幂或明确写死值，溢出必须显性计数）。
const _: () = assert!(EVENT_CAP == 128, "事件账容量写死 128");
const _: () = assert!(REVIEW_CAP == 32, "评审账容量写死 32");
const _: () = assert!(GATE_TRACE_CAP == 32, "留痕容量写死 32");
const _: () = assert!(VERSION_CAP == 32, "版本账容量写死 32");
/// 兼容语义判据（MAJOR 相等 + 门面 MINOR ≥ 插件 MINOR 的正反例编译期固定）。
const _: () = assert!(
    SemVer::new(1, 3).is_compatible_with(SemVer::new(1, 4)),
    "插件 1.3 vs 门面 1.4 必须兼容"
);
const _: () = assert!(
    !SemVer::new(1, 5).is_compatible_with(SemVer::new(1, 4)),
    "插件 1.5 vs 门面 1.4 必须拦截（插件 MINOR 超前）"
);
const _: () = assert!(
    !SemVer::new(2, 0).is_compatible_with(SemVer::new(1, 4)),
    "插件 2.0 vs 门面 1.4 必须拦截（MAJOR 不等 = 破坏兼容）"
);
const _: () = assert!(ChangeKind::MajorBump.is_breaking(), "MAJOR 前进必须标记破坏兼容");
const _: () = assert!(!ChangeKind::MinorAdd.is_breaking(), "MINOR 增补必须标记兼容");
const _: () = assert!(
    commitment_phase(0, DECADE_SUPPORT_DAYS - 1).as_index() == CommitmentPhase::Committed.as_index(),
    "支持期最后一日仍是承诺态"
);
const _: () = assert!(
    commitment_phase(0, DECADE_SUPPORT_DAYS).as_index() == CommitmentPhase::GraceOnly.as_index(),
    "支持期结束次日进入宽限期"
);
const _: () = assert!(
    commitment_phase(0, DECADE_TOTAL_DAYS - 1).as_index() == CommitmentPhase::GraceOnly.as_index(),
    "宽限期最后一日仍是宽限态"
);
const _: () = assert!(
    commitment_phase(0, DECADE_TOTAL_DAYS).as_index() == CommitmentPhase::Expired.as_index(),
    "宽限期结束次日承诺过期"
);
const _: () = assert!(
    commitment_phase(0, DECADE_TOTAL_DAYS).allows_change() == false,
    "过期态必须禁止一切增改"
);

// ---------------------------------------------------------------------------
// 十五、冒烟与判据测试（隔离探针专用——内核路径零测试依赖）
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    /// 冒烟：建账九条、逐条可查、类别闭集齐全。
    #[test]
    fn boot_and_query_all() {
        let l = FreezeLayer::boot(0).expect("boot");
        assert_eq!(l.ledger.len(), API_COUNT);
        for id in 1..=9u16 {
            let a = l.query(id).unwrap_or_else(|e| panic!("query {} failed: {:?}", id, e));
            assert!(ApiClass::from_index(a.class.as_index()).is_some());
        }
        assert!(l.query(10).is_err());
        // 三类各三条。
        for cls in ApiClass::ALL {
            let n = l.ledger.iter().filter(|a| a.class == cls).count();
            assert_eq!(n, 3, "类别 {} 应有三条", cls.as_str());
        }
    }

    /// 冒烟：门面按 id 直查 O(1) 路径与种子对齐。
    #[test]
    fn facade_lookup_matches_seed() {
        for s in FROZEN_API_SEED {
            let f = facade_lookup(s.api_id).unwrap_or_else(|| panic!("facade miss {}", s.api_id));
            assert_eq!(f.api_id, s.api_id);
            assert_eq!(f.facade_symbol, s.facade_symbol);
        }
        assert!(facade_lookup(0).is_none());
        assert!(facade_lookup(10).is_none());
    }

    /// 判据：版本拦截——相等放行 / 老插件放行 / 超前 MINOR 拦 / MAJOR 不等拦，
    /// 且拦截必留痕（trace 在账）。
    #[test]
    fn gate_blocks_and_traces() {
        let mut l = FreezeLayer::boot(100).unwrap();
        let facade_v = l.query(1).unwrap().version; // v1.4
        // 插件 1.4：相等放行。
        let v = l
            .admit(PluginBinding { api_id: 1, plugin_name: "p_ok", declared: SemVer::new(1, 4) }, 100)
            .unwrap();
        assert_eq!(v.api_id, 1);
        // 插件 1.2：老 MINOR 放行（门面向后兼容供应）。
        assert!(l
            .admit(PluginBinding { api_id: 1, plugin_name: "p_old", declared: SemVer::new(1, 2) }, 100)
            .is_ok());
        // 插件 1.5：超前 MINOR 拦。
        let e1 = l.admit(PluginBinding { api_id: 1, plugin_name: "p_ahead", declared: SemVer::new(1, 5) }, 100).unwrap_err();
        assert_eq!(e1.code(), CODE_VERSION_BLOCKED);
        assert!(matches!(e1, FreezeLayerError::VersionBlocked { .. }));
        // 插件 2.0：MAJOR 不等拦。
        let e2 = l.admit(PluginBinding { api_id: 1, plugin_name: "p_major", declared: SemVer::new(2, 0) }, 100).unwrap_err();
        assert_eq!(e2.code(), CODE_VERSION_BLOCKED);
        // 留痕：2 笔拦截都在门账上，且可按 API 追溯。
        assert_eq!(l.gate.blocked.len(), 2);
        assert_eq!(l.gate.traces_for(1).len(), 2);
        assert_eq!(l.gate.blocked[0].facade, facade_v);
        assert_eq!(l.gate.blocked[0].code, CODE_VERSION_BLOCKED);
        // 事件账：拦截事件在案。
        let h = l.health();
        assert_eq!(h.gate_blocks, 2);
        assert!(h.gate_passes >= 2);
    }

    /// 判据：外泄检测——内部符号拒绝带码；门面符号放行；未知符号 Unknown。
    #[test]
    fn leak_probe_rejects_internal() {
        for sym in INTERNAL_SYMBOLS {
            let e = probe_symbol(sym).unwrap_err();
            assert_eq!(e.code(), CODE_FACADE_LEAK);
            assert!(e.detail().contains("门面外泄"));
        }
        for f in FACADE_TABLE {
            match probe_symbol(f.facade_symbol).unwrap() {
                ProbeVerdict::Allowed { api_id } => assert_eq!(api_id, f.api_id),
                ProbeVerdict::Unknown => panic!("facade symbol judged unknown"),
            }
        }
        assert_eq!(probe_symbol("totally_unrelated").unwrap(), ProbeVerdict::Unknown);
    }

    /// 判据：评审四态流转——提交→评审中→通过，不可跳态，终态不可再流转。
    #[test]
    fn review_flow_four_states() {
        let mut l = FreezeLayer::boot(0).unwrap();
        let rid = l.open_review(3, ChangeKind::MajorBump, "vsync 语义重定", 5).unwrap();
        // 跳态被拒：Submitted 直接 Approve 不行。
        let e = l.advance_review(rid, ReviewDecision::Approve, 6).unwrap_err();
        assert_eq!(e.code(), CODE_REVIEW_BLOCKED);
        assert_eq!(l.advance_review(rid, ReviewDecision::Open, 6).unwrap(), ReviewState::UnderReview);
        assert_eq!(l.advance_review(rid, ReviewDecision::Approve, 7).unwrap(), ReviewState::Approved);
        // 终态不可再流转。
        let e2 = l.advance_review(rid, ReviewDecision::Reject, 8).unwrap_err();
        assert_eq!(e2.code(), CODE_REVIEW_BLOCKED);
        // 单据字段落账：结单日、态、影响面。
        let r = l.reviews.iter().find(|r| r.review_id == rid).unwrap();
        assert_eq!(r.state, ReviewState::Approved);
        assert_eq!(r.closed_day, 7);
        assert_eq!(r.impact.same_class_count, 3, "Display 类变更面 = 3");
        assert!(r.impact.breaking);
        // 不存在的单。
        let e3 = l.advance_review(999, ReviewDecision::Open, 9).unwrap_err();
        assert_eq!(e3.code(), CODE_REVIEW_BLOCKED);
    }

    /// 判据：否决路径——否决后不可消费落账（apply 仍拦 + 红账）。
    #[test]
    fn review_rejected_blocks_apply() {
        let mut l = FreezeLayer::boot(0).unwrap();
        let rid = l.open_review(1, ChangeKind::MajorBump, "乱改呈现语义", 1).unwrap();
        l.advance_review(rid, ReviewDecision::Open, 2).unwrap();
        l.advance_review(rid, ReviewDecision::Reject, 3).unwrap();
        let e = l.apply_major_bump(1, 4).unwrap_err();
        assert_eq!(e.code(), CODE_UNAUTHORIZED);
        assert_eq!(l.red_flags, 1);
        assert_eq!(l.query(1).unwrap().version, SemVer::new(1, 4), "版本不得变");
        assert!(l.versions.is_empty(), "否决不得留版本账");
    }

    /// 判据：越权变更——无评审单 MAJOR 前进被拦 + 红账 + 版本不变。
    #[test]
    fn unauthorized_major_blocked() {
        let mut l = FreezeLayer::boot(0).unwrap();
        let e = l.apply_major_bump(4, 10).unwrap_err();
        assert_eq!(e.code(), CODE_UNAUTHORIZED);
        assert_eq!(l.red_flags, 1);
        assert_eq!(l.query(4).unwrap().version, SemVer::new(2, 1));
        // 越权拦截事件在账。
        let h = l.health();
        assert_eq!(h.unauthorized, 1);
        assert!(!h.ok(), "红账之后 health 必须不绿灯");
    }

    /// 判据：合规 MAJOR——评审通过后落账成功、一次性消费、版本账留单号。
    #[test]
    fn approved_major_applies_once() {
        let mut l = FreezeLayer::boot(0).unwrap();
        let rid = l.open_review(4, ChangeKind::MajorBump, "色彩管线重构", 1).unwrap();
        l.advance_review(rid, ReviewDecision::Open, 2).unwrap();
        l.advance_review(rid, ReviewDecision::Approve, 3).unwrap();
        let v = l.apply_major_bump(4, 4).unwrap();
        assert_eq!(v, SemVer::new(3, 0));
        // 一次性消费：同单第二次落账被拦（无已批未消费的单了）。
        let e = l.apply_major_bump(4, 5).unwrap_err();
        assert_eq!(e.code(), CODE_UNAUTHORIZED);
        // 重复落账 = 无已批未消费的单 = 越权，红账 +1（纪律对重放同样成立）。
        assert_eq!(l.red_flags, 1);
        // 版本账留评审单号（可追溯）。
        assert_eq!(l.versions.len(), 1);
        assert_eq!(l.versions[0].via_review, rid);
        assert_eq!(l.versions[0].from, SemVer::new(2, 1));
        assert_eq!(l.versions[0].to, SemVer::new(3, 0));
        assert_eq!(l.versions[0].via_review, rid, "版本账必须留评审单号");
    }

    /// 判据：MINOR 只增兼容——免评审通道落账 + 版本账（via_review=0）+ 指纹有账。
    #[test]
    fn minor_add_compatible() {
        let mut l = FreezeLayer::boot(0).unwrap();
        let before = l.fingerprint();
        let v = l.apply_minor_add(1, 2).unwrap();
        assert_eq!(v, SemVer::new(1, 5));
        assert_eq!(l.versions[0].via_review, 0);
        assert_ne!(l.fingerprint(), before, "冻结面变更指纹必变");
        // 有账指纹审计通过（ChangedWithLedger）。
        match l.audit_surface().unwrap() {
            SurfaceAudit::ChangedWithLedger => {}
            other => panic!("expected ChangedWithLedger, got {:?}", other),
        }
        // 老插件 1.4 仍兼容新门面 1.5。
        assert!(SemVer::new(1, 4).is_compatible_with(l.query(1).unwrap().version));
        assert_eq!(l.red_flags, 0);
        assert!(l.health().ok());
    }

    /// 判据：宽限期只修不增——GRACE 内 MINOR 增补被拦（CommitmentExpired）。
    #[test]
    fn minor_add_blocked_in_grace() {
        // id=1 冻结于第 0 日：day = 3650（支持期结束次日）即宽限期。
        let mut l = FreezeLayer::boot(3650).unwrap();
        assert_eq!(l.query(1).unwrap().commitment, CommitmentPhase::GraceOnly);
        let e = l.apply_minor_add(1, 3650).unwrap_err();
        assert_eq!(e.code(), CODE_COMMITMENT_EXPIRED);
        assert_eq!(l.query(1).unwrap().version, SemVer::new(1, 4), "宽限期不得增");
    }

    /// 判据：十年承诺到期——过期后一切增改被拦（含 MAJOR，即使有已批单）。
    #[test]
    fn commitment_expiry_blocks_everything() {
        let mut l = FreezeLayer::boot(0).unwrap();
        // 先批好一单 MAJOR，等到期后再试落账。
        let rid = l.open_review(1, ChangeKind::MajorBump, "到期前预批", 1).unwrap();
        l.advance_review(rid, ReviewDecision::Open, 2).unwrap();
        l.advance_review(rid, ReviewDecision::Approve, 3).unwrap();
        let day = commitment_expiry(30); // 最晚冻结日 30 + 双版期 4380 = 4410
        let newly = l.expire_sweep(day);
        assert_eq!(newly, 9, "九条全到期");
        assert_eq!(l.query(1).unwrap().commitment, CommitmentPhase::Expired);
        // 已批单也拦——过期即只读。
        let e = l.apply_major_bump(1, day).unwrap_err();
        assert_eq!(e.code(), CODE_COMMITMENT_EXPIRED);
        let e2 = l.apply_minor_add(2, day).unwrap_err();
        assert_eq!(e2.code(), CODE_COMMITMENT_EXPIRED);
        assert_eq!(l.red_flags, 0, "承诺拒绝是合规拒绝，不是越权");
    }

    /// 判据：承诺期三态边界（支持期末日/宽限首日/宽限期末日/过期首日，合成账目单测）。
    #[test]
    fn commitment_phase_boundaries() {
        assert_eq!(commitment_phase(0, 0), CommitmentPhase::Committed);
        assert_eq!(commitment_phase(0, 3649), CommitmentPhase::Committed);
        assert_eq!(commitment_phase(0, 3650), CommitmentPhase::GraceOnly);
        assert_eq!(commitment_phase(0, 4379), CommitmentPhase::GraceOnly);
        assert_eq!(commitment_phase(0, 4380), CommitmentPhase::Expired);
        // 起算日推移整体平移。
        assert_eq!(commitment_phase(30, 30 + 3650), CommitmentPhase::GraceOnly);
        assert_eq!(commitment_phase(30, 30 + 4380), CommitmentPhase::Expired);
        assert_eq!(commitment_expiry(0), 4380);
        assert_eq!(commitment_expiry(30), 4410);
    }

    /// 判据：到期检查 O(API 数)——态变化才落账，重复扫不重复记。
    #[test]
    fn expire_sweep_idempotent_events() {
        let mut l = FreezeLayer::boot(0).unwrap();
        assert_eq!(l.expire_sweep(100), 0, "早期扫描零过期");
        assert_eq!(l.expire_sweep(commitment_expiry(30)), 9);
        let h1 = l.health();
        assert_eq!(h1.expired_events, 9);
        // 再扫一遍：无态变化 → 无新事件。
        assert_eq!(l.expire_sweep(commitment_expiry(30) + 1), 0);
        assert_eq!(l.health().expired_events, 9);
    }

    /// 判据：承诺修订——无单被拦红账；有单落账起算日平移。
    #[test]
    fn commitment_amend_review_gated() {
        let mut l = FreezeLayer::boot(0).unwrap();
        // 无单 → 红账。
        let e = l.amend_commitment(6, 100, 100).unwrap_err();
        assert_eq!(e.code(), CODE_UNAUTHORIZED);
        assert_eq!(l.red_flags, 1);
        // 有单 → 落账。
        let rid = l.open_review(6, ChangeKind::CommitmentAmend, "色调映射冻结基准重锚", 100).unwrap();
        l.advance_review(rid, ReviewDecision::Open, 101).unwrap();
        l.advance_review(rid, ReviewDecision::Approve, 102).unwrap();
        l.amend_commitment(6, 100, 103).unwrap();
        assert_eq!(l.query(6).unwrap().frozen_day, 100);
        assert_eq!(commitment_expiry(100), 100 + 4380);
        // 指纹已随冻结日变化且有账。
        match l.audit_surface().unwrap() {
            SurfaceAudit::ChangedWithLedger => {}
            other => panic!("expected ChangedWithLedger, got {:?}", other),
        }
    }

    /// 判据：指纹可检测——任何冻结面变更指纹必变；无账变更被审计抓出。
    #[test]
    fn fingerprint_drift_detected() {
        let mut l = FreezeLayer::boot(0).unwrap();
        // 无账手改（模拟外部改动）：指纹变 → 审计报 FingerprintDrift。
        let cur = l.fingerprint();
        l.frozen_fingerprint = cur.wrapping_add(1); // 基线被篡改 = 现值与基线脱钩
        let e = l.audit_surface().unwrap_err();
        assert_eq!(e.code(), CODE_FINGERPRINT_DRIFT);
        assert_eq!(l.health().surface, SurfaceAuditState::Unaccounted);
        assert!(!l.health().ok());
    }

    /// 判据：签名对账——登记指纹被手改即 SignatureDrift。
    #[test]
    fn signature_audit_catches_tamper() {
        let mut l = FreezeLayer::boot(0).unwrap();
        assert!(l.audit_signatures().is_ok());
        l.ledger[2].sig_fnv = 0xDEAD_BEEF;
        let e = l.audit_signatures().unwrap_err();
        assert_eq!(e.code(), CODE_SIGNATURE_DRIFT);
        assert_eq!(e, FreezeLayerError::SignatureDrift { api_id: 3 });
    }

    /// 判据：双跑确定性——同日序建账两实例、同脚本、digest 全等。
    #[test]
    fn dual_run_deterministic() {
        let r = dual_run_check(42).unwrap();
        assert_eq!(r.a_events, r.b_events);
        assert!(r.a_events > 0);
        // 再跑一遍同 digest（跨实例+跨次都稳定）。
        let r2 = dual_run_check(42).unwrap();
        assert_eq!(r.digest, r2.digest);
        // 换日序 digest 应变（账面含日序——确定性不等于恒定）。
        let r3 = dual_run_check(43).unwrap();
        assert_ne!(r.digest, r3.digest, "不同日序必须产生不同账面");
    }

    /// 判据：注入面三路全检出（外泄/破坏兼容/越权）。
    #[test]
    fn injections_all_detected() {
        let mut l = FreezeLayer::boot(0).unwrap();
        assert!(l.inject_leak(1).unwrap(), "外泄注入必须检出");
        assert!(l.inject_incompat(2).unwrap(), "破坏兼容注入必须检出");
        assert!(l.inject_unauth(3).unwrap(), "越权变更注入必须检出");
        let h = l.health();
        assert_eq!(h.leak_blocks, 1);
        assert_eq!(h.gate_blocks, 1);
        assert_eq!(h.unauthorized, 1);
        assert_eq!(h.red_flags, 1);
    }

    /// 判据：自检全绿（fresh 账）。
    #[test]
    fn self_checks_green() {
        let mut l = FreezeLayer::boot(0).unwrap();
        let r = l.run_self_checks(0).unwrap();
        assert!(r.all_green(), "checks={} passed={}", r.checks, r.passed);
        assert_eq!(r.checks, 9 + 4 + 9 + 1 + 1);
        assert!(l.health().ok());
    }

    /// 判据：诊断码互异、段内独占 0x1BC0..=0x1BCF（运行期再对拍一遍）。
    #[test]
    fn codes_distinct_in_segment() {
        for (i, &a) in ALL_CODES.iter().enumerate() {
            for b in ALL_CODES.iter().skip(i + 1) {
                assert_ne!(a, *b);
            }
            assert!((a & 0xFF00) == 0x1B00);
            assert!(a >= SEG_BASE && a < SEG_END);
        }
    }

    /// 判据：错误诊断全覆盖——12 变体每个 code 落段、detail 非空且含可操作语。
    #[test]
    fn error_details_all_actionable() {
        let errs: [FreezeLayerError; 12] = [
            FreezeLayerError::FacadeLeak(INTERNAL_SYMBOLS[0]),
            FreezeLayerError::VersionBlocked { api_id: 1, declared: SemVer::new(2, 0), facade: SemVer::new(1, 4) },
            FreezeLayerError::UnauthorizedChange { api_id: 1, why: "t" },
            FreezeLayerError::ReviewBlocked { review_id: 1, why: "t" },
            FreezeLayerError::ApiNotFound(42),
            FreezeLayerError::LedgerFull { cap: 32, which: "t" },
            FreezeLayerError::CommitmentExpired { api_id: 1, day: 9999 },
            FreezeLayerError::ClassInvalid(7),
            FreezeLayerError::SignatureDrift { api_id: 1 },
            FreezeLayerError::DuplicateApi(1),
            FreezeLayerError::DualRunMismatch { lhs: 1, rhs: 2 },
            FreezeLayerError::FingerprintDrift { current: 1, baseline: 2 },
        ];
        for e in errs {
            assert!(e.code() >= SEG_BASE && e.code() < SEG_END);
            assert!(!e.detail().is_empty());
            assert!(e.detail().len() > 20, "诊断必须可操作：{:?}", e.detail());
        }
    }

    /// 冒烟：语义版本纪律正反例（运行期再验一遍编译期断言同口径）。
    #[test]
    fn semver_discipline() {
        assert!(SemVer::new(1, 3).is_compatible_with(SemVer::new(1, 4)));
        assert!(SemVer::new(1, 4).is_compatible_with(SemVer::new(1, 4)));
        assert!(!SemVer::new(1, 5).is_compatible_with(SemVer::new(1, 4)));
        assert!(!SemVer::new(2, 0).is_compatible_with(SemVer::new(1, 9)));
        assert_eq!(SemVer::new(1, 4).bump_minor(), SemVer::new(1, 5));
        assert_eq!(SemVer::new(1, 4).bump_major(), SemVer::new(2, 0));
        assert!(SemVer::new(2, 0).breaks_from(SemVer::new(1, 9)));
        assert!(!SemVer::new(1, 9).breaks_from(SemVer::new(1, 4)));
        // 饱和：u16::MAX 不回绕。
        assert_eq!(SemVer::new(1, u16::MAX).bump_minor().minor, u16::MAX);
        assert_eq!(SemVer::new(u16::MAX, 0).bump_major().major, u16::MAX);
    }

    /// 冒烟：承诺账面板与 health 摘要行非空可读。
    #[test]
    fn panels_readable() {
        let l = FreezeLayer::boot(10).unwrap();
        let board = l.commitment_board(10);
        assert_eq!(board.len(), 9);
        assert!(board[0].contains("起算=0"));
        assert!(board[0].contains("到期=4380"));
        assert!(board[0].contains("承诺期内"));
        let h = l.health();
        let line = h.as_line();
        assert!(line.contains("apis=9"));
        assert!(line.contains("red=0"));
        let sl = screen_line();
        assert!(sl.contains("VEV09-apifreeze"));
        assert!(!sl.is_empty());
    }

    /// 冒烟：账簿溢出显性计数（事件账压满后溢出计数，不 panic 不静默丢）。
    #[test]
    fn ledger_overflow_explicit() {
        let mut l = FreezeLayer::boot(0).unwrap();
        // 用门面放行（GatePass——无定长副账，可无限供应）压满事件账。
        let mut day = 1u32;
        while l.events.len() < EVENT_CAP {
            let _ = l.admit(PluginBinding { api_id: 1, plugin_name: "flood", declared: SemVer::new(1, 1) }, day);
            day = day.wrapping_add(1);
        }
        assert_eq!(l.events.len(), EVENT_CAP);
        let before = l.event_overflow;
        // 再来一笔应溢出计数（放行照常成功，事件转溢出账——不静默丢）。
        let _ = l.admit(PluginBinding { api_id: 1, plugin_name: "flood", declared: SemVer::new(1, 1) }, day);
        assert_eq!(l.event_overflow, before + 1);
        assert!(!l.health().ok(), "溢出必须使 health 不绿灯");
    }

    /// 冒烟：评审账满显性拒收（LedgerFull 上抛，不静默丢提案）。
    #[test]
    fn review_cap_explicit_reject() {
        let mut l = FreezeLayer::boot(0).unwrap();
        let mut day = 1u32;
        while l.reviews.len() < REVIEW_CAP {
            let opened = l.open_review(1, ChangeKind::MinorAdd, "压账", day);
            assert!(opened.is_ok());
            day += 1;
        }
        let e = l.open_review(1, ChangeKind::MinorAdd, "超账", day).unwrap_err();
        assert_eq!(e.code(), CODE_LEDGER_FULL);
        assert_eq!(l.review_overflow, 1);
    }

    /// 冒烟：版本账满显性拒收。
    #[test]
    fn version_cap_explicit_reject() {
        let mut l = FreezeLayer::boot(0).unwrap();
        let mut day = 1u32;
        while l.versions.len() < VERSION_CAP {
            l.apply_minor_add(9, day).unwrap();
            day += 1;
        }
        let e = l.apply_minor_add(9, day).unwrap_err();
        assert_eq!(e.code(), CODE_LEDGER_FULL);
        assert_eq!(l.version_overflow, 1);
    }

    /// 冒烟：门面留痕满溢出计数（拦截继续被拒，留痕转溢出账）。
    #[test]
    fn gate_trace_overflow_explicit() {
        let mut l = FreezeLayer::boot(0).unwrap();
        let mut day = 1u32;
        // 对 api=1 用超前版本反复拦截，压满留痕。
        while l.gate.blocked.len() < GATE_TRACE_CAP {
            let _ = l.admit(PluginBinding { api_id: 1, plugin_name: "flood", declared: SemVer::new(9, 9) }, day);
            day += 1;
        }
        assert_eq!(l.gate.blocked.len(), GATE_TRACE_CAP);
        // 再拦一笔 → 溢出计数（错误照常上抛）。
        let e = l.admit(PluginBinding { api_id: 1, plugin_name: "flood", declared: SemVer::new(9, 9) }, day).unwrap_err();
        assert_eq!(e.code(), CODE_VERSION_BLOCKED);
        assert_eq!(l.gate.overflow, 1);
        assert!(!l.health().ok());
    }

    /// 冒烟：未知 API 全路径被拒（admit/open_review/apply 三路）。
    #[test]
    fn unknown_api_rejected_everywhere() {
        let mut l = FreezeLayer::boot(0).unwrap();
        assert_eq!(
            l.admit(PluginBinding { api_id: 99, plugin_name: "x", declared: SemVer::new(1, 0) }, 1).unwrap_err().code(),
            CODE_API_NOT_FOUND
        );
        assert_eq!(l.open_review(99, ChangeKind::MinorAdd, "x", 1).unwrap_err().code(), CODE_API_NOT_FOUND);
        assert_eq!(l.apply_minor_add(99, 1).unwrap_err().code(), CODE_API_NOT_FOUND);
        assert_eq!(l.apply_major_bump(99, 1).unwrap_err().code(), CODE_API_NOT_FOUND);
    }

    /// 冒烟：签名指纹 const 可用且区分输入（编译期算定的前提）。
    #[test]
    fn signature_fnv_discriminates() {
        const A: u32 = signature_fingerprint("display.present_frame", "vev09_disp_present");
        const B: u32 = signature_fingerprint("display.query_modes", "vev09_disp_modes");
        assert_ne!(A, B);
        // 换符号同名不同指纹。
        assert_ne!(
            signature_fingerprint("x", "sym_a"),
            signature_fingerprint("x", "sym_b")
        );
    }
}
