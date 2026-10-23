//! CGPU-F2085 · 效果与原生 API 映射（CGPU-N 域 · 原生 API 组 · 目标 320 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/CGPU Varix STAR II · 总纲与施工书.md#CGPU-F2085`
//!
//! **判据（锚点原文）**：映射表、红线复用、两组、判据。
//!
//! **职责定位（锚点原文摘录）**：API 映射（原生 API 映射（SetWindowCompositionAttribute 等映射表——映射表版本化——版本化；调用（安全调用路径（硬件红线复用——红线复用；测试（映射/安全两组）。
//!
//! --------------------------------------------------------------------------
//!
//! ## 〇、本模块全景（八个子系统，全部真实实现，无桩）
//!
//! 本模块把 VE 图形效果（模糊/亚克力/云母/阴影/圆角/透明度/动画曲线……）
//! 映射到原生系统 API 调用（SetWindowCompositionAttribute /
//! DwmSetWindowAttribute / DwmEnableBlurBehindWindow 族）。八个子系统：
//!
//! 1. **效果闭集**（[`EffectKind`]）：16 种效果闭集枚举，未知效果名显性
//!    拒绝不猜（[`EffectKind::from_name`] 走名字表逐字比对，表外即拒）。
//! 2. **原生 API 描述表**（[`NativeApiDesc`] + [`API_TABLE`]）：API 名/
//!    模块/属性码/参数布局/最低系统版本逐条写死并注释来源（Windows SDK
//!    头文件与 Microsoft 文档）。
//! 3. **版本化映射表**（[`EffMapTable`] + [`MapEntry`]）：const 基表
//!    [`BASE_TABLE`] 构建 + `lookup()` + FNV-1a 表指纹 +
//!    **只增不改**演进纪律（改写旧条目拒绝带码 0x12D2 + 审计留痕）。
//! 4. **硬件红线卫兵**（[`RedLineGuard`]）：直调内核 API / 越权句柄 /
//!    未声明能力 / 安全面四条红线（[`REDLINE_RULES`] 清单），命中即拒 +
//!    拒绝留痕（[`DenyTrace`]）+ 降级建议路径。
//! 5. **安全调用计划器**（[`SafeCallPlanner`]）：效果 → API 阶段链 →
//!    前置检查（OS 版本/红线/能力）→ 失败回退链（A 失败退 B 退软件合成
//!    ——回退链 const 表 [`BASE_TABLE`] 内含 + 实测计数）。
//! 6. **参数编解码**（[`encode_call_params`] / [`decode_call_params`]）：
//!    32 字节定长二进制帧，小端显性编解码 + 校验和 + 编码往返自证。
//! 7. **事件账**（[`MapEventLedger`]）：查表命中/红线拒绝/回退触发三类
//!    计数 + 定长环形账（128 槽，满则覆写最旧并计 dropped）。
//! 8. **引擎门面**（[`EffMapEngine`]）：幂等双跑对拍（同效果重复映射
//!    同一计划）+ `health()` 体检 + 自检报告文本。
//!
//! ## 〇.一、纪律落实
//!
//! - **零 panic 面**：内核路径零 `unwrap`/`expect`/`panic!`/运行期
//!   `assert!`；可失败全走 [`Result`]<T, [`EffMapError`]>
//!   （16 变体闭集，码段 0x12D0~0x12DF 独占，每变体
//!   [`EffMapError::detail()`] 中文可操作诊断）。
//! - **no_std + alloc**：只依赖 `core` 与 `alloc::{vec::Vec, format,
//!   string::String}`；零 IO、零墙钟、零线程。
//! - **溢出显性**：全部计数器走 `saturating_add`/`wrapping_add`。
//! - **栈上禁区**：>64KB 结构体禁止上栈——本模块最大类型
//!   [`EffMapEngine`] 约 2KB，编译期 `const` 断言逐类型封顶。
//! - **const fn 禁坑**：const fn 内不用 `slice::get`/`Ord::min/max`/
//!   derive 相等比较/`for` 循环——一律 `while` + 下标，枚举比较一律
//!   经 `as_u8()` 整型化。
//! - **API 常量来源**：凡取自 Windows SDK 头文件的常量逐条注释来源
//!   （winuser.h / dwmapi.h / wingdi.h）；SetWindowCompositionAttribute
//!   为 user32.dll 公开导出但未收入官方头文件，其 ACCENT_STATE 取值以
//!   Microsoft 文档与社区考证为准并显性标注，列为能力受控 API。
//!
//! **对接**：F2084（效果重建管线——捕获/重建的上游）、F2083（重建原则
//! ——验证口径）、M 域（合成——软件合成兜底 A00 的执行端）、F 域
//! （能力声明——[`DeclaredCaps`] 位集的颁发方）。零 panic 面、零 IO、
//! 零墙钟、no_std。

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 一、规格常量
// ---------------------------------------------------------------------------

/// 本项版本（N03 组 · 效果与原生 API 映射）。
pub const EFFMAP_VERSION: &str = "N03-effmap-v1";

/// 映射表 schema 版本（V1 起步；演进只增不改——schema 号只允许追加新
/// 版本号，不允许改写旧版本语义）。
pub const MAP_TABLE_SCHEMA_V1: u32 = 1;

/// 映射表容量（基表 14 条 + 演进追加余量 10 条）。
pub const EFFMAP_TABLE_CAP: usize = 24;

/// 演进审计账容量（追加/拒绝两类行；满则置溢出旗标并只计数不落账）。
pub const EFFMAP_AUDIT_CAP: usize = 64;

/// 红线拒绝留痕容量（环形挤出最旧）。
pub const GUARD_TRACE_CAP: usize = 32;

/// 事件账定长容量（环形账）。
pub const LEDGER_CAP: usize = 128;

// ---------------------------------------------------------------------------
// 二、诊断码（0x12D0~0x12DF 段独占：F2085 效果与原生 API 映射面）
// ---------------------------------------------------------------------------

/// 诊断码闭集（0x12D0~0x12DF 段独占，共 16 变体；每变体配
/// [`EffMapError::code()`] 与 [`EffMapError::detail()`]）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EffMapError {
    /// 未知效果名（表外名字显性拒绝不猜）。
    UnknownEffect,
    /// 映射表 schema 版本不符（拒绝挂载）。
    TableSchemaMismatch,
    /// 只增不改红线：改写既有映射条目被拒绝。
    EntryRewriteDenied,
    /// 映射表已满。
    TableFull,
    /// 硬件红线命中（直调被拒绝；留痕见 [`DenyTrace`]）。
    RedLineViolation,
    /// 安全调用计划失败（全链不可用，含软件兜底）。
    PlanFailed,
    /// 参数编码溢出（字段超界或布局未知）。
    ParamEncodeOverflow,
    /// 参数解码校验不符（校验和/保留域）。
    ParamDecodeMismatch,
    /// 事件账满进入环形覆写。
    LedgerFull,
    /// 演进审计账满。
    AuditLedgerFull,
    /// 表指纹不符（疑似越权改写）。
    FingerprintMismatch,
    /// API 序号不在表内或未为该效果定义取值。
    ApiNotFound,
    /// 计划尺寸越限（>64KB 栈上禁区防护）。
    PlanSizeOverLimit,
    /// 映射请求非法（句柄域未知/系统版本缺失）。
    InvalidRequest,
    /// 幂等漂移（同请求两次计划不一致）。
    IdempotencyDrift,
    /// 体检降级（存在红灯/黄灯项）。
    HealthDegraded,
}

impl EffMapError {
    /// 诊断码（0x12D0~0x12DF 段独占）。
    pub const fn code(&self) -> u16 {
        match self {
            EffMapError::UnknownEffect => 0x12D0,
            EffMapError::TableSchemaMismatch => 0x12D1,
            EffMapError::EntryRewriteDenied => 0x12D2,
            EffMapError::TableFull => 0x12D3,
            EffMapError::RedLineViolation => 0x12D4,
            EffMapError::PlanFailed => 0x12D5,
            EffMapError::ParamEncodeOverflow => 0x12D6,
            EffMapError::ParamDecodeMismatch => 0x12D7,
            EffMapError::LedgerFull => 0x12D8,
            EffMapError::AuditLedgerFull => 0x12D9,
            EffMapError::FingerprintMismatch => 0x12DA,
            EffMapError::ApiNotFound => 0x12DB,
            EffMapError::PlanSizeOverLimit => 0x12DC,
            EffMapError::InvalidRequest => 0x12DD,
            EffMapError::IdempotencyDrift => 0x12DE,
            EffMapError::HealthDegraded => 0x12DF,
        }
    }

    /// 中文可操作诊断（每变体一句：说清"错在哪、下一步怎么办"）。
    pub const fn detail(&self) -> &'static str {
        match self {
            EffMapError::UnknownEffect => {
                "未知效果名：只接受效果闭集内的名字。请用 EffectKind::name() 拼写核对，勿猜测别名或自造效果名"
            }
            EffMapError::TableSchemaMismatch => {
                "映射表 schema 版本不符：加载的表由更高 schema 生成。请先升级内核映射模块（EFFMAP_VERSION），再挂载该表"
            }
            EffMapError::EntryRewriteDenied => {
                "只增不改红线：改写既有映射条目被拒绝。请新增修订条目并按版本发布流程演进；历史条目与审计账不允许原地改写"
            }
            EffMapError::TableFull => {
                "映射表已满：当前容量见 EFFMAP_TABLE_CAP。请清理未挂载的实验条目，或扩容后重编（扩容本身也要走演进审计）"
            }
            EffMapError::RedLineViolation => {
                "硬件红线命中：直调被拒绝。请按拒绝留痕（DenyTrace.degrade_hint）改走保守降级路径，勿绕过卫兵直调"
            }
            EffMapError::PlanFailed => {
                "安全调用计划失败：效果存在但全部阶段（含软件合成兜底）不可用。请检查能力声明位集与系统版本是否满足任一阶段"
            }
            EffMapError::ParamEncodeOverflow => {
                "参数编码溢出：字段超界或布局未知。请核对 LAYOUT_* 布局表与各字段宽度，确认未传入未注册的布局号"
            }
            EffMapError::ParamDecodeMismatch => {
                "参数解码校验不符：校验和或保留域不匹配。参数帧可能被截断或篡改，请整帧丢弃并重新编码，勿带病下发"
            }
            EffMapError::LedgerFull => {
                "事件账已满进入环形覆写：最旧事件被挤出（dropped 计数）。如需全量留存请先调用 summary/快照导出再继续"
            }
            EffMapError::AuditLedgerFull => {
                "演进审计账已满：后续审计事件只计数不落账（audit_overflow 旗标）。请先归档审计账再继续演进操作"
            }
            EffMapError::FingerprintMismatch => {
                "表指纹不符：表内容与记录指纹不一致，可能被越权改写。请拒绝挂载该表并走演进审计追责"
            }
            EffMapError::ApiNotFound => {
                "API 序号不在表内或未为该效果定义取值：请核对 API_TABLE 与映射条目回退链定义，链条里不得出现未注册 API"
            }
            EffMapError::PlanSizeOverLimit => {
                "计划尺寸越限：超过 64KB 栈上禁区。请缩减阶段数或把大块负载改走堆上参数帧"
            }
            EffMapError::InvalidRequest => {
                "映射请求非法：句柄域取值不在 HandleScope 闭集内，或系统版本为 0。请用闭集取值并携带 os_pack 打包版本"
            }
            EffMapError::IdempotencyDrift => {
                "幂等漂移：同请求两次计划不一致。请排查是否引入了非确定性输入（环境探测/时钟/随机数）并冻结环境后重跑对拍"
            }
            EffMapError::HealthDegraded => {
                "体检降级：存在红灯/黄灯项。请按 EffMapHealth.issues 列表逐项处置后再恢复映射服务"
            }
        }
    }

    /// 带码诊断行（ alloc 文本；供日志总线消费）。
    pub fn render(&self) -> String {
        format!("[EFFMAP-0x{:04X}] {}", self.code(), self.detail())
    }
}

// ---------------------------------------------------------------------------
// 三、效果闭集（16 种；闭集纪律：表外名字显性拒绝不猜）
// ---------------------------------------------------------------------------

/// VE 图形效果闭集。判据要求 ≥12 种；此处 16 种，其中 14 种在基表
/// [`BASE_TABLE`] 有原生映射，2 种（`LuminosityBlend`/`VibranceBoost`）
/// 预留为演进追加位——验证"只增不改"的追加路径真实可用。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum EffectKind {
    /// 高斯背景模糊（窗后内容模糊）。
    BlurGaussian = 1,
    /// 亚克力材质（模糊 + 噪点 + 着色）。
    AcrylicMaterial = 2,
    /// 云母材质（取样桌面壁纸的低频模糊）。
    MicaMaterial = 3,
    /// 云母 Alt（标签页窗体变体）。
    MicaAltTab = 4,
    /// 窗体投影（DWM 边框延伸产生的阴影）。
    DropShadow = 5,
    /// 窗体圆角（Win11 顶点偏好）。
    RoundedCorner = 6,
    /// 整窗透明度（分层窗口 alpha）。
    AlphaTransparency = 7,
    /// 色键透明（colorkey 抠色）。
    ColorKeyTransparency = 8,
    /// 暗色表面（沉浸式暗色标题栏）。
    DarkSurface = 9,
    /// 淡入淡出动画曲线（AnimateWindow 混合）。
    AnimationFade = 10,
    /// 边框着色（窗框颜色覆写）。
    BorderTint = 11,
    /// 渐变背景（纯渐变 accent，无模糊）。
    GradientBackdrop = 12,
    /// 磨砂叠层（模糊 + 着色玻璃感）。
    FrostOverlay = 13,
    /// 宿主背景底衬（host backdrop brush）。
    HostBackdrop = 14,
    /// 亮度混合（演进预留：未入基表）。
    LuminosityBlend = 15,
    /// 鲜艳度增强（演进预留：无原生等价物，软件直发）。
    VibranceBoost = 16,
}

/// 效果闭集数量。
pub const EFFECT_KIND_COUNT: usize = 16;

/// 基表映射条目数（其余 2 种为演进追加位）。
pub const BASE_ENTRY_COUNT: usize = 14;

impl EffectKind {
    /// 判别值（const fn；枚举比较一律经整型化，绕开 const 语境禁坑）。
    pub const fn as_u8(&self) -> u8 {
        *self as u8
    }

    /// 判别值 → 效果（0 与 17.. 均无效果：显性 None 不猜）。
    pub const fn from_u8(v: u8) -> Option<EffectKind> {
        match v {
            1 => Some(EffectKind::BlurGaussian),
            2 => Some(EffectKind::AcrylicMaterial),
            3 => Some(EffectKind::MicaMaterial),
            4 => Some(EffectKind::MicaAltTab),
            5 => Some(EffectKind::DropShadow),
            6 => Some(EffectKind::RoundedCorner),
            7 => Some(EffectKind::AlphaTransparency),
            8 => Some(EffectKind::ColorKeyTransparency),
            9 => Some(EffectKind::DarkSurface),
            10 => Some(EffectKind::AnimationFade),
            11 => Some(EffectKind::BorderTint),
            12 => Some(EffectKind::GradientBackdrop),
            13 => Some(EffectKind::FrostOverlay),
            14 => Some(EffectKind::HostBackdrop),
            15 => Some(EffectKind::LuminosityBlend),
            16 => Some(EffectKind::VibranceBoost),
            _ => None,
        }
    }

    /// 效果规范名（映射请求只认这组名字；表外名字显性拒绝）。
    pub const fn name(&self) -> &'static str {
        match self {
            EffectKind::BlurGaussian => "blur.gaussian",
            EffectKind::AcrylicMaterial => "acrylic.material",
            EffectKind::MicaMaterial => "mica.material",
            EffectKind::MicaAltTab => "mica.alt",
            EffectKind::DropShadow => "drop.shadow",
            EffectKind::RoundedCorner => "round.corner",
            EffectKind::AlphaTransparency => "alpha.transparency",
            EffectKind::ColorKeyTransparency => "color.key",
            EffectKind::DarkSurface => "dark.surface",
            EffectKind::AnimationFade => "anim.fade",
            EffectKind::BorderTint => "border.tint",
            EffectKind::GradientBackdrop => "gradient.backdrop",
            EffectKind::FrostOverlay => "frost.overlay",
            EffectKind::HostBackdrop => "host.backdrop",
            EffectKind::LuminosityBlend => "luminosity.blend",
            EffectKind::VibranceBoost => "vibrance.boost",
        }
    }

    /// 效果中文名（诊断与账本呈现用）。
    pub const fn zh_name(&self) -> &'static str {
        match self {
            EffectKind::BlurGaussian => "高斯背景模糊",
            EffectKind::AcrylicMaterial => "亚克力材质",
            EffectKind::MicaMaterial => "云母材质",
            EffectKind::MicaAltTab => "云母Alt",
            EffectKind::DropShadow => "窗体投影",
            EffectKind::RoundedCorner => "窗体圆角",
            EffectKind::AlphaTransparency => "整窗透明度",
            EffectKind::ColorKeyTransparency => "色键透明",
            EffectKind::DarkSurface => "暗色表面",
            EffectKind::AnimationFade => "淡入淡出动画",
            EffectKind::BorderTint => "边框着色",
            EffectKind::GradientBackdrop => "渐变背景",
            EffectKind::FrostOverlay => "磨砂叠层",
            EffectKind::HostBackdrop => "宿主背景底衬",
            EffectKind::LuminosityBlend => "亮度混合",
            EffectKind::VibranceBoost => "鲜艳度增强",
        }
    }

    /// 效果名 → 效果（表外名字显性拒绝带码 0x12D0，不猜不模糊匹配）。
    pub fn from_name(s: &str) -> Result<EffectKind, EffMapError> {
        let mut i = 0;
        while i < EFFECT_KIND_COUNT {
            if EFFECT_KIND_NAMES[i] == s {
                // 名字表与闭集枚举一一对应，下标换算判别值后走闭集构造。
                let v = (i + 1) as u8;
                return match EffectKind::from_u8(v) {
                    Some(k) => Ok(k),
                    None => Err(EffMapError::UnknownEffect),
                };
            }
            i += 1;
        }
        Err(EffMapError::UnknownEffect)
    }
}

/// 效果名表（与 [`EffectKind`] 判别值一一对应：名字表下标 + 1 = 判别值）。
pub const EFFECT_KIND_NAMES: [&str; EFFECT_KIND_COUNT] = [
    "blur.gaussian",
    "acrylic.material",
    "mica.material",
    "mica.alt",
    "drop.shadow",
    "round.corner",
    "alpha.transparency",
    "color.key",
    "dark.surface",
    "anim.fade",
    "border.tint",
    "gradient.backdrop",
    "frost.overlay",
    "host.backdrop",
    "luminosity.blend",
    "vibrance.boost",
];

// ---------------------------------------------------------------------------
// 四、参数布局 / 能力位 / 系统版本打包
// ---------------------------------------------------------------------------

/// 参数布局：软件合成兜底（Varix 自有路径，无系统参数帧）。
pub const LAYOUT_SW_COMPOSITE: u16 = 0;
/// 参数布局：ACCENT_POLICY（16 字节：AccentState/AccentFlags/GradientColor/
/// AnimationId，各 4 字节）——来源：SetWindowCompositionAttribute 配套
/// 结构，见 winuser.h WCA_ACCENT_POLICY 与 Microsoft 文档考证。
pub const LAYOUT_ACCENT_POLICY: u16 = 1;
/// 参数布局：单 DWORD 属性值（4 字节）——DwmSetWindowAttribute 的
/// 标准属性值形态（dwmapi.h）。
pub const LAYOUT_DWORD: u16 = 2;
/// 参数布局：DWM_BLURBEHIND（12 字节：dwFlags/BOOL fEnable/HRGN hRgnBlur）
/// ——来源：dwmapi.h，Windows Vista SDK。
pub const LAYOUT_BLURBEHIND: u16 = 3;
/// 参数布局：MARGINS（16 字节：cxLeftWidth/cxRightWidth/cyTopHeight/
/// cyBottomHeight）——来源：dwmapi.h，Windows Vista SDK。
pub const LAYOUT_MARGINS: u16 = 4;
/// 参数布局：SetLayeredWindowAttributes 参数组（12 字节：COLORREF crKey/
/// BYTE bAlpha/dwFlags）——来源：winuser.h。
pub const LAYOUT_LAYERED_ATTRS: u16 = 5;
/// 参数布局：AnimateWindow 参数组（8 字节：dwTime/dwFlags）——来源：
/// winuser.h。
pub const LAYOUT_ANIMATE: u16 = 6;
/// 参数布局：SetWindowRgn 参数组（16 字节：区域类型标记 + 保留）——
/// 来源：winuser.h/wingdi.h；区域句柄由 Varix 侧构造，帧内只带类型标记。
pub const LAYOUT_WINDOW_RGN: u16 = 7;

/// 已注册布局闭集数量（编码器按闭集校验，表外布局显性拒绝）。
pub const LAYOUT_KIND_COUNT: u16 = 8;

/// 能力位：dwmapi.dll 调用权（DwmSetWindowAttribute 族）。
pub const CAP_DWM_API: u32 = 1 << 0;
/// 能力位：user32 SetWindowCompositionAttribute（非公开契约）调用权。
pub const CAP_SWCA: u32 = 1 << 1;
/// 能力位：分层窗口（WS_EX_LAYERED + SetLayeredWindowAttributes）。
pub const CAP_LAYERED: u32 = 1 << 2;
/// 能力位：AnimateWindow 动画。
pub const CAP_ANIMATION: u32 = 1 << 3;
/// 能力位：SetWindowRgn 窗体区域。
pub const CAP_REGION: u32 = 1 << 4;
/// 能力位：软件合成兜底（回退链终点必须声明）。
pub const CAP_SOFTWARE: u32 = 1 << 5;

/// 全能力位集（体检样本请求使用）。
pub const CAP_ALL: u32 = CAP_DWM_API | CAP_SWCA | CAP_LAYERED | CAP_ANIMATION | CAP_REGION | CAP_SOFTWARE;

/// 系统版本打包：`major(8bit) << 24 | minor(4bit) << 20 | build(20bit)`。
/// 例：Windows 10 build 14393 → 0x0A00_3839。打包后可直接整型比较。
pub const fn os_pack(major: u32, minor: u32, build: u32) -> u32 {
    (major << 24) | ((minor & 0xF) << 20) | (build & 0xFFFFF)
}

/// Windows 2000（build 2195）——SetLayeredWindowAttributes/AnimateWindow/
/// SetWindowRgn 的最低版本（winuser.h，平台 SDK 文档）。
pub const OS_WIN2K: u32 = os_pack(5, 0, 2195);
/// Windows Vista RTM（build 6000）——DwmSetWindowAttribute /
/// DwmEnableBlurBehindWindow / DwmExtendFrameIntoClientArea 的最低版本
/// （dwmapi.h，Windows Vista SDK）。
pub const OS_VISTA: u32 = os_pack(6, 0, 6000);
/// Windows 10 1507（build 10240）——SetWindowCompositionAttribute
/// ACCENT_STATE 生效起点（Microsoft 文档/社区考证）。
pub const OS_WIN10_1507: u32 = os_pack(10, 0, 10240);
/// Windows 10 1703（build 15063）——ACCENT_ENABLE_ACRYLICBLURBEHIND
/// 稳定可用起点；DWMWA_USE_HOSTBACKDROPBRUSH 之外的亚克力回退窗。
pub const OS_WIN10_1703: u32 = os_pack(10, 0, 15063);
/// Windows 10 1903（build 18362）——DWMWA_USE_IMMERSIVE_DARK_MODE 属性
/// 码 20（初名 19）与 DWMWA_USE_HOSTBACKDROPBRUSH 属性码 17 生效起点
/// （dwmapi.h，Windows 10 1903 SDK）。
pub const OS_WIN10_1903: u32 = os_pack(10, 0, 18362);
/// Windows 11 21H2（build 22000）——DWMWA_WINDOW_CORNER_PREFERENCE
/// （属性码 33）与 DWMWA_BORDER_COLOR（属性码 34）生效起点
/// （dwmapi.h，Windows 11 SDK）。
pub const OS_WIN11_21H2: u32 = os_pack(10, 0, 22000);
/// Windows 11 22H2（build 22621）——DWMWA_SYSTEMBACKDROP_TYPE
/// （属性码 38：云母/亚克力/云母 Alt 系统底衬）生效起点
/// （dwmapi.h，Windows 11 22H2 SDK）。
pub const OS_WIN11_22H2: u32 = os_pack(10, 0, 22621);

/// 体检样本系统版本（Windows 11 24H2 build 26100——样本用当前主流版本）。
pub const OS_HEALTH_SAMPLE: u32 = os_pack(10, 0, 26100);

// ---------------------------------------------------------------------------
// 五、原生 API 描述表（API 名/模块/属性码/参数布局/最低系统版本——逐条注释来源）
// ---------------------------------------------------------------------------

/// API 表下标类型。
pub type ApiIndex = u8;

/// 空回退位（回退链 4 槽定长，未用槽填 0xFF）。
pub const API_NONE: ApiIndex = 0xFF;

/// A00：Varix 软件合成兜底（回退链终点；非系统 API）。
pub const API_SOFTWARE: ApiIndex = 0;
/// A01：user32!SetWindowCompositionAttribute。
pub const API_SWCA: ApiIndex = 1;
/// A02：dwmapi!DwmSetWindowAttribute。
pub const API_DWMAPI_ATTR: ApiIndex = 2;
/// A03：dwmapi!DwmEnableBlurBehindWindow。
pub const API_DWM_BLURBEHIND: ApiIndex = 3;
/// A04：dwmapi!DwmExtendFrameIntoClientArea。
pub const API_DWM_MARGINS: ApiIndex = 4;
/// A05：user32!SetLayeredWindowAttributes。
pub const API_LAYERED_ATTRS: ApiIndex = 5;
/// A06：user32!AnimateWindow。
pub const API_ANIMATE: ApiIndex = 6;
/// A07：user32!SetWindowRgn。
pub const API_WINDOW_RGN: ApiIndex = 7;
/// A08：win32k!NtUserSetWindowCompositionAttribute（内核直调面——红线）。
pub const API_NT_SWCA: ApiIndex = 8;

/// 原生 API 表条目数（A00~A08 共 9 条）。
pub const API_TABLE_LEN: usize = 9;

/// 红线分类（API 条目的 `redline_class` 字段）：
pub const RLC_SAFE: u8 = 0;
/// 红线分类 0：公开文档化用户态 API，卫兵只做能力校验。
/// 红线分类 1：能力受控 API（非公开契约，随系统更新可能失效）。
pub const RLC_CAP_GATED: u8 = 1;
/// 红线分类 2：硬件红线——禁止直调，命中即拒（0x12D4）+ 降级。
pub const RLC_REDLINE: u8 = 2;

/// 原生 API 描述（const 表逐条写死，注释来源）。
#[derive(Debug, Clone, Copy)]
pub struct NativeApiDesc {
    /// API 表下标（A00~A08）。
    pub api_index: ApiIndex,
    /// API 导出名（写死，供留痕与计划呈现）。
    pub api_name: &'static str,
    /// 宿主模块（user32.dll / dwmapi.dll / win32k.sys / varix 内部）。
    pub module: &'static str,
    /// 属性码/入口常量（0 表示按调用传参，见条目注释）。
    pub attr_code: u32,
    /// 参数布局号（[`LAYOUT_*`] 闭集）。
    pub layout: u16,
    /// 布局自然尺寸（字节；A00 无帧为 0）。
    pub layout_bytes: u16,
    /// 最低系统版本（[`os_pack`] 打包，整型比较）。
    pub min_os: u32,
    /// 需声明能力位集（全满足才可调用；红线 API 填 0——永不放行）。
    pub required_cap: u32,
    /// 红线分类（[`RLC_*`]）。
    pub redline_class: u8,
    /// 来源注记（SDK 头文件/文档；写死进指纹）。
    pub note: &'static str,
}

/// 原生 API 描述表（A00~A08）。**来源逐条注释**：
///
/// - **A00** Varix 软件合成兜底：非系统 API，M 域合成执行端；任何回退
///   链的终点（判据"失败回退链"的软件合成一级）。
/// - **A01** SetWindowCompositionAttribute（user32.dll 公开导出，未收入
///   官方头文件）：属性 `WCA_ACCENT_POLICY = 19`（Windows 10 SDK
///   winuser.h 已收录 WCA_* 枚举）；ACCENT_STATE 取值 ACCENT_ENABLE_
///   GRADIENT=1 / TRANSPARENTGRADIENT=2 / BLURBEHIND=3 / ACRYLICBLURBEHIND=4
///   / HOSTBACKDROP=5（Microsoft Docs 与社区考证）。因属非公开契约，
///   列为能力受控（RLC_CAP_GATED），要求 [`CAP_SWCA`]。
/// - **A02** DwmSetWindowAttribute（dwmapi.h）：属性码按调用传入，
///   本表使用的属性族 DWMWA_USE_IMMERSIVE_DARK_MODE=20、
///   DWMWA_USE_HOSTBACKDROPBRUSH=17、DWMWA_WINDOW_CORNER_PREFERENCE=33、
///   DWMWA_BORDER_COLOR=34、DWMWA_SYSTEMBACKDROP_TYPE=38；分属性最低
///   版本见各映射条目 min_os。
/// - **A03** DwmEnableBlurBehindWindow（dwmapi.h）：DWM_BB_ENABLE=0x1、
///   DWM_BB_BLURREGION=0x2、DWM_BB_TRANSITIONONMAXIMIZED=0x4
///   （Windows Vista SDK 起）。
/// - **A04** DwmExtendFrameIntoClientArea（dwmapi.h）：MARGINS 结构
///   （Windows Vista SDK 起）。
/// - **A05** SetLayeredWindowAttributes（winuser.h）：LWA_COLORKEY=0x1、
///   LWA_ALPHA=0x2；配合 WS_EX_LAYERED=0x00080000（Windows 2000 起）。
/// - **A06** AnimateWindow（winuser.h）：AW_BLEND=0x00080000、
///   AW_SLIDE=0x00040000、AW_HIDE=0x00010000、AW_ACTIVATE=0x00020000
///   （Windows 2000 起）。
/// - **A07** SetWindowRgn（winuser.h/wingdi.h）：区域句柄由 Varix 侧
///   构造，参数帧只携带区域类型标记（Windows 2000 起）。
/// - **A08** NtUserSetWindowCompositionAttribute（win32k.sys 内核服务
///   ——A01 的内核入口）：**硬件红线**。用户态图形子系统禁直调内核
///   服务（绕过 win32k 校验与 UIPI），命中即拒（0x12D4）并降级 A01。
pub const API_TABLE: [NativeApiDesc; API_TABLE_LEN] = [
    NativeApiDesc {
        api_index: API_SOFTWARE,
        api_name: "VarixSoftwareComposite",
        module: "varix.sw-composite",
        attr_code: 0,
        layout: LAYOUT_SW_COMPOSITE,
        layout_bytes: 0,
        min_os: 0,
        required_cap: CAP_SOFTWARE,
        redline_class: RLC_SAFE,
        note: "Varix 自有软件合成兜底——非系统 API；回退链终点",
    },
    NativeApiDesc {
        api_index: API_SWCA,
        api_name: "SetWindowCompositionAttribute",
        module: "user32.dll",
        attr_code: 19, // WCA_ACCENT_POLICY = 19（Windows 10 SDK winuser.h）
        layout: LAYOUT_ACCENT_POLICY,
        layout_bytes: 16,
        min_os: OS_WIN10_1507,
        required_cap: CAP_SWCA,
        redline_class: RLC_CAP_GATED,
        note: "非公开契约：ACCENT_STATE 见 Microsoft Docs 考证；能力受控",
    },
    NativeApiDesc {
        api_index: API_DWMAPI_ATTR,
        api_name: "DwmSetWindowAttribute",
        module: "dwmapi.dll",
        attr_code: 0, // 属性码按调用传入（DWMWA_* 族）
        layout: LAYOUT_DWORD,
        layout_bytes: 4,
        min_os: OS_VISTA,
        required_cap: CAP_DWM_API,
        redline_class: RLC_SAFE,
        note: "dwmapi.h：DWMWA_*=17/20/33/34/38（分属性最低版本见映射条目）",
    },
    NativeApiDesc {
        api_index: API_DWM_BLURBEHIND,
        api_name: "DwmEnableBlurBehindWindow",
        module: "dwmapi.dll",
        attr_code: 0x1, // DWM_BB_ENABLE = 0x1（dwmapi.h）
        layout: LAYOUT_BLURBEHIND,
        layout_bytes: 12,
        min_os: OS_VISTA,
        required_cap: CAP_DWM_API,
        redline_class: RLC_SAFE,
        note: "dwmapi.h：DWM_BB_ENABLE=0x1 DWM_BB_BLURREGION=0x2 DWM_BB_TRANSITIONONMAXIMIZED=0x4",
    },
    NativeApiDesc {
        api_index: API_DWM_MARGINS,
        api_name: "DwmExtendFrameIntoClientArea",
        module: "dwmapi.dll",
        attr_code: 0,
        layout: LAYOUT_MARGINS,
        layout_bytes: 16,
        min_os: OS_VISTA,
        required_cap: CAP_DWM_API,
        redline_class: RLC_SAFE,
        note: "dwmapi.h：MARGINS{cxLeftWidth,cxRightWidth,cyTopHeight,cyBottomHeight}",
    },
    NativeApiDesc {
        api_index: API_LAYERED_ATTRS,
        api_name: "SetLayeredWindowAttributes",
        module: "user32.dll",
        attr_code: 0,
        layout: LAYOUT_LAYERED_ATTRS,
        layout_bytes: 12,
        min_os: OS_WIN2K,
        required_cap: CAP_LAYERED,
        redline_class: RLC_SAFE,
        note: "winuser.h：LWA_COLORKEY=0x1 LWA_ALPHA=0x2；WS_EX_LAYERED=0x00080000",
    },
    NativeApiDesc {
        api_index: API_ANIMATE,
        api_name: "AnimateWindow",
        module: "user32.dll",
        attr_code: 0,
        layout: LAYOUT_ANIMATE,
        layout_bytes: 8,
        min_os: OS_WIN2K,
        required_cap: CAP_ANIMATION,
        redline_class: RLC_SAFE,
        note: "winuser.h：AW_BLEND=0x00080000 AW_SLIDE=0x00040000 AW_HIDE=0x00010000",
    },
    NativeApiDesc {
        api_index: API_WINDOW_RGN,
        api_name: "SetWindowRgn",
        module: "user32.dll",
        attr_code: 0,
        layout: LAYOUT_WINDOW_RGN,
        layout_bytes: 16,
        min_os: OS_WIN2K,
        required_cap: CAP_REGION,
        redline_class: RLC_SAFE,
        note: "winuser.h/wingdi.h：区域句柄 Varix 侧构造，帧只带类型标记",
    },
    NativeApiDesc {
        api_index: API_NT_SWCA,
        api_name: "NtUserSetWindowCompositionAttribute",
        module: "win32k.sys",
        attr_code: 19, // 与用户态 WCA_ACCENT_POLICY 同参（内核入口）
        layout: LAYOUT_ACCENT_POLICY,
        layout_bytes: 16,
        min_os: OS_WIN10_1507,
        required_cap: 0, // 红线 API：永不放行
        redline_class: RLC_REDLINE,
        note: "硬件红线：用户态禁直调 win32k 内核服务（绕过 UIPI/win32k 校验）",
    },
];

// ---------------------------------------------------------------------------
// 六、版本化映射表（const 构建 + lookup + FNV-1a 指纹 + 只增不改演进）
// ---------------------------------------------------------------------------

/// 单条映射：效果 → 主路径 API + 回退链（4 槽定长）。
///
/// 回退链纪律：主路径失败退第 1 回退，第 1 失败退第 2……终点必须是
/// 软件合成 A00（编译期校验：[`base_entries_sane`] 保证每条链含 A00
/// 或主路径本身即 A00）。未用槽填 [`API_NONE`]。
#[derive(Debug, Clone, Copy)]
pub struct MapEntry {
    /// 效果（闭集内）。
    pub kind: EffectKind,
    /// 主路径 API（A00~A08）。
    pub primary: ApiIndex,
    /// 回退链（4 槽；0xFF = 空位）。
    pub fallbacks: [ApiIndex; 4],
    /// 主步参数布局号（与主 API 布局一致；表内自检）。
    pub layout: u16,
    /// 本条映射最低系统版本（打包；主路径 API 门槛取两者较严者）。
    pub min_os: u32,
    /// 来源注记（SDK 头文件/文档；参与指纹）。
    pub source_note: &'static str,
}

/// 基表 V1（14 条）。**判据"映射表"主体**：VE 效果 → 原生 API 的映射
/// 写死于此，const 构建、指纹固定。演进一律走 [`EffMapTable::append_entry`]
/// （追加），禁止改写本表既有条目（[`EffMapTable::amend_entry`] 恒拒）。
pub const BASE_TABLE: [MapEntry; BASE_ENTRY_COUNT] = [
    // 1. 高斯背景模糊：SWCA ACCENT_ENABLE_BLURBEHIND=3；回退
    //    DwmEnableBlurBehindWindow（dwmapi.h DWM_BB_ENABLE=0x1，Vista 起）。
    MapEntry {
        kind: EffectKind::BlurGaussian,
        primary: API_SWCA,
        fallbacks: [API_DWM_BLURBEHIND, API_SOFTWARE, API_NONE, API_NONE],
        layout: LAYOUT_ACCENT_POLICY,
        min_os: OS_WIN10_1507,
        source_note: "winuser.h WCA_ACCENT_POLICY=19, ACCENT_ENABLE_BLURBEHIND=3; dwmapi.h DWM_BB_ENABLE=0x1",
    },
    // 2. 亚克力：SWCA ACCENT_ENABLE_ACRYLICBLURBEHIND=4（1703 起）；
    //    回退 DWMWA_SYSTEMBACKDROP_TYPE=38 + DWMSBT_TRANSIENTWINDOW=3（22H2 起）。
    MapEntry {
        kind: EffectKind::AcrylicMaterial,
        primary: API_SWCA,
        fallbacks: [API_DWMAPI_ATTR, API_SOFTWARE, API_NONE, API_NONE],
        layout: LAYOUT_ACCENT_POLICY,
        min_os: OS_WIN10_1703,
        source_note: "ACCENT_ENABLE_ACRYLICBLURBEHIND=4; dwmapi.h DWMWA_SYSTEMBACKDROP_TYPE=38, DWMSBT_TRANSIENTWINDOW=3",
    },
    // 3. 云母：DWMWA_SYSTEMBACKDROP_TYPE=38 + DWMSBT_MAINWINDOW=2（22H2 起）；
    //    回退亚克力 SWCA（1703 起）。
    MapEntry {
        kind: EffectKind::MicaMaterial,
        primary: API_DWMAPI_ATTR,
        fallbacks: [API_SWCA, API_SOFTWARE, API_NONE, API_NONE],
        layout: LAYOUT_DWORD,
        min_os: OS_WIN11_22H2,
        source_note: "dwmapi.h DWMWA_SYSTEMBACKDROP_TYPE=38, DWMSBT_MAINWINDOW=2; 回退 ACCENT_ENABLE_ACRYLICBLURBEHIND=4",
    },
    // 4. 云母 Alt：DWMSBT_TABBEDWINDOW=4（22H2 起）；回退亚克力。
    MapEntry {
        kind: EffectKind::MicaAltTab,
        primary: API_DWMAPI_ATTR,
        fallbacks: [API_SWCA, API_SOFTWARE, API_NONE, API_NONE],
        layout: LAYOUT_DWORD,
        min_os: OS_WIN11_22H2,
        source_note: "dwmapi.h DWMWA_SYSTEMBACKDROP_TYPE=38, DWMSBT_TABBEDWINDOW=4",
    },
    // 5. 窗体投影：DwmExtendFrameIntoClientArea（MARGINS 底边 1px）；
    //    回退 DWMWA_NCRENDERING_ENABLED=1 前置确认（dwmapi.h 属性码 1）。
    MapEntry {
        kind: EffectKind::DropShadow,
        primary: API_DWM_MARGINS,
        fallbacks: [API_DWMAPI_ATTR, API_SOFTWARE, API_NONE, API_NONE],
        layout: LAYOUT_MARGINS,
        min_os: OS_VISTA,
        source_note: "dwmapi.h MARGINS; DWMWA_NCRENDERING_ENABLED=1（属性码 1，Vista 起）",
    },
    // 6. 窗体圆角：DWMWA_WINDOW_CORNER_PREFERENCE=33 + DWMWCP_ROUND=2
    //    （Win11 22000 起）；回退 SetWindowRgn 圆角区域（Win2000 起）。
    MapEntry {
        kind: EffectKind::RoundedCorner,
        primary: API_DWMAPI_ATTR,
        fallbacks: [API_WINDOW_RGN, API_SOFTWARE, API_NONE, API_NONE],
        layout: LAYOUT_DWORD,
        min_os: OS_WIN11_21H2,
        source_note: "dwmapi.h DWMWA_WINDOW_CORNER_PREFERENCE=33, DWMWCP_ROUND=2; 回退 SetWindowRgn",
    },
    // 7. 整窗透明度：SetLayeredWindowAttributes LWA_ALPHA=0x2（Win2000 起）；
    //    回退 SWCA ACCENT_ENABLE_TRANSPARENTGRADIENT=2。
    MapEntry {
        kind: EffectKind::AlphaTransparency,
        primary: API_LAYERED_ATTRS,
        fallbacks: [API_SWCA, API_SOFTWARE, API_NONE, API_NONE],
        layout: LAYOUT_LAYERED_ATTRS,
        min_os: OS_WIN2K,
        source_note: "winuser.h LWA_ALPHA=0x2; ACCENT_ENABLE_TRANSPARENTGRADIENT=2",
    },
    // 8. 色键透明：SetLayeredWindowAttributes LWA_COLORKEY=0x1（Win2000 起）；
    //    无二级原生回退，直达软件合成。
    MapEntry {
        kind: EffectKind::ColorKeyTransparency,
        primary: API_LAYERED_ATTRS,
        fallbacks: [API_SOFTWARE, API_NONE, API_NONE, API_NONE],
        layout: LAYOUT_LAYERED_ATTRS,
        min_os: OS_WIN2K,
        source_note: "winuser.h LWA_COLORKEY=0x1；crKey 抠色由调用方给色值",
    },
    // 9. 暗色表面：DWMWA_USE_IMMERSIVE_DARK_MODE=20 + TRUE=1（1903 起）；
    //    无二级原生回退，直达软件合成。
    MapEntry {
        kind: EffectKind::DarkSurface,
        primary: API_DWMAPI_ATTR,
        fallbacks: [API_SOFTWARE, API_NONE, API_NONE, API_NONE],
        layout: LAYOUT_DWORD,
        min_os: OS_WIN10_1903,
        source_note: "dwmapi.h DWMWA_USE_IMMERSIVE_DARK_MODE=20, TRUE=1",
    },
    // 10. 淡入淡出动画：AnimateWindow AW_BLEND=0x00080000（Win2000 起）；
    //     无二级原生回退，直达软件合成（Varix 曲线自绘）。
    MapEntry {
        kind: EffectKind::AnimationFade,
        primary: API_ANIMATE,
        fallbacks: [API_SOFTWARE, API_NONE, API_NONE, API_NONE],
        layout: LAYOUT_ANIMATE,
        min_os: OS_WIN2K,
        source_note: "winuser.h AW_BLEND=0x00080000",
    },
    // 11. 边框着色：DWMWA_BORDER_COLOR=34（Win11 22000 起）；无二级
    //     原生回退，直达软件合成描边。
    MapEntry {
        kind: EffectKind::BorderTint,
        primary: API_DWMAPI_ATTR,
        fallbacks: [API_SOFTWARE, API_NONE, API_NONE, API_NONE],
        layout: LAYOUT_DWORD,
        min_os: OS_WIN11_21H2,
        source_note: "dwmapi.h DWMWA_BORDER_COLOR=34（COLORREF 形态 ARGB）",
    },
    // 12. 渐变背景：SWCA ACCENT_ENABLE_GRADIENT=1（Win10 1507 起）；
    //     无二级原生回退，直达软件合成。
    MapEntry {
        kind: EffectKind::GradientBackdrop,
        primary: API_SWCA,
        fallbacks: [API_SOFTWARE, API_NONE, API_NONE, API_NONE],
        layout: LAYOUT_ACCENT_POLICY,
        min_os: OS_WIN10_1507,
        source_note: "ACCENT_ENABLE_GRADIENT=1",
    },
    // 13. 磨砂叠层：SWCA BLURBEHIND=3 + 着色（同模糊态，帧内带染色值）；
    //     回退 DwmEnableBlurBehindWindow。
    MapEntry {
        kind: EffectKind::FrostOverlay,
        primary: API_SWCA,
        fallbacks: [API_DWM_BLURBEHIND, API_SOFTWARE, API_NONE, API_NONE],
        layout: LAYOUT_ACCENT_POLICY,
        min_os: OS_WIN10_1703,
        source_note: "ACCENT_ENABLE_BLURBEHIND=3 + GradientColor 染色；回退 DWM_BB_ENABLE=0x1",
    },
    // 14. 宿主背景底衬：SWCA ACCENT_ENABLE_HOSTBACKDROP=5（Win10 1507 起，
    //     配合 DWMWA_USE_HOSTBACKDROPBRUSH=17 使用）；无二级原生回退。
    MapEntry {
        kind: EffectKind::HostBackdrop,
        primary: API_SWCA,
        fallbacks: [API_SOFTWARE, API_NONE, API_NONE, API_NONE],
        layout: LAYOUT_ACCENT_POLICY,
        min_os: OS_WIN10_1507,
        source_note: "ACCENT_ENABLE_HOSTBACKDROP=5; dwmapi.h DWMWA_USE_HOSTBACKDROPBRUSH=17",
    },
];

/// FNV-1a 单字节（const fn；wrapping 乘法显性）。
const fn fnv_byte(mut hash: u64, b: u8) -> u64 {
    hash ^= b as u64;
    hash = hash.wrapping_mul(0x1000_0000_01B3);
    hash
}

/// FNV-1a 64 表指纹（const fn：const 切片入参 + while + 下标，无 get/
/// 无 for/无 derive 比较）。条目按"判别值→主路径→回退链→min_os(LE)→
/// 布局(LE)→来源注记字节→分隔符"规范序列哈希——内容变一字节，指纹变。
pub const fn fnv1a_entries(entries: &[MapEntry]) -> u64 {
    let mut hash: u64 = 0xCBF2_9CE4_8422_2325;
    let mut i = 0;
    while i < entries.len() {
        let e = &entries[i];
        hash = fnv_byte(hash, e.kind.as_u8());
        hash = fnv_byte(hash, e.primary);
        let mut j = 0;
        while j < 4 {
            hash = fnv_byte(hash, e.fallbacks[j]);
            j += 1;
        }
        let os = e.min_os;
        hash = fnv_byte(hash, (os & 0xFF) as u8);
        hash = fnv_byte(hash, ((os >> 8) & 0xFF) as u8);
        hash = fnv_byte(hash, ((os >> 16) & 0xFF) as u8);
        hash = fnv_byte(hash, ((os >> 24) & 0xFF) as u8);
        hash = fnv_byte(hash, (e.layout & 0xFF) as u8);
        hash = fnv_byte(hash, (e.layout >> 8) as u8);
        let note = e.source_note.as_bytes();
        let mut k = 0;
        while k < note.len() {
            hash = fnv_byte(hash, note[k]);
            k += 1;
        }
        hash = fnv_byte(hash, 0x1F); // 条目分隔符
        i += 1;
    }
    hash
}

/// 基表 V1 指纹（编译期固定；任何对基表的改动都会被指纹与审计同时抓到）。
pub const BASE_FINGERPRINT: u64 = fnv1a_entries(&BASE_TABLE);

/// 回退链是否含软件合成终点（const fn）。
const fn chain_has_software(chain: [u8; 4]) -> bool {
    let mut i = 0;
    while i < 4 {
        if chain[i] == API_SOFTWARE {
            return true;
        }
        i += 1;
    }
    false
}

/// 基表合法性编译期自检：主路径/回退下标均需为已注册 API（或空位），
/// 每条回退链必须收敛到软件合成（或主路径本身即软件合成），效果无重复。
const fn base_entries_sane(entries: &[MapEntry]) -> bool {
    let mut i = 0;
    while i < entries.len() {
        let e = &entries[i];
        if (e.primary as usize) >= API_TABLE_LEN {
            return false;
        }
        if !chain_has_software(e.fallbacks) && e.primary != API_SOFTWARE {
            return false;
        }
        let mut j = 0;
        while j < 4 {
            let fb = e.fallbacks[j];
            if fb != API_NONE && (fb as usize) >= API_TABLE_LEN {
                return false;
            }
            j += 1;
        }
        let mut k = i + 1;
        while k < entries.len() {
            if entries[k].kind.as_u8() == e.kind.as_u8() {
                return false;
            }
            k += 1;
        }
        i += 1;
    }
    true
}

/// 演进审计动作：追加（合法演进）/ 拒绝改写（违规企图留痕）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum AuditAction {
    /// 追加新条目（合法演进；revision 递增）。
    Append = 1,
    /// 改写旧条目被拒绝（只增不改红线；表内容不变，企图留痕）。
    DenyRewrite = 2,
}

/// 演进审计行（定长小结构；`source_note` 级别粒度）。
#[derive(Debug, Clone, Copy)]
pub struct AuditRow {
    /// 动作后的表 revision。
    pub revision: u32,
    /// 动作（追加/拒绝改写）。
    pub action: AuditAction,
    /// 涉事效果判别值。
    pub kind: u8,
    /// 动作后的表指纹（拒绝改写时与动作前相同）。
    pub fingerprint_after: u64,
    /// 注记（写死字符串，不分配）。
    pub note: &'static str,
}

/// 版本化映射表：const 基表 + 追加式演进 + FNV-1a 指纹 + 审计账。
///
/// **只增不改纪律**：`amend_entry` 恒拒（带码 0x12D2 + 审计留痕）；
/// `append_entry` 只接受效果闭集内尚未映射的效果（如预留的
/// LuminosityBlend/VibranceBoost），每追加一次 revision 递增、指纹
/// 重算、审计落一行。历史条目内容永远不变（审计可回放）。
#[derive(Debug)]
pub struct EffMapTable {
    schema: u32,
    revision: u32,
    entries: Vec<MapEntry>,
    fingerprint: u64,
    audit: Vec<AuditRow>,
    audit_overflow: bool,
    deny_rewrite_total: u64,
}

impl EffMapTable {
    /// 按 schema 版本挂载表：schema 不符显性拒绝（0x12D1）。
    pub fn load_with_schema(schema: u32) -> Result<Self, EffMapError> {
        if schema != MAP_TABLE_SCHEMA_V1 {
            return Err(EffMapError::TableSchemaMismatch);
        }
        // 编译期自检兜底之外，运行期仍防表满（基表 14 ≤ 容量 24）。
        if BASE_ENTRY_COUNT > EFFMAP_TABLE_CAP {
            return Err(EffMapError::TableFull);
        }
        let mut entries = Vec::new();
        let mut i = 0;
        while i < BASE_ENTRY_COUNT {
            entries.push(BASE_TABLE[i]);
            i += 1;
        }
        Ok(EffMapTable {
            schema,
            revision: 1,
            fingerprint: BASE_FINGERPRINT,
            entries,
            audit: Vec::new(),
            audit_overflow: false,
            deny_rewrite_total: 0,
        })
    }

    /// 以当前 schema 挂载（常规入口）。
    pub fn new() -> Result<Self, EffMapError> {
        Self::load_with_schema(MAP_TABLE_SCHEMA_V1)
    }

    /// schema 版本。
    pub fn schema(&self) -> u32 {
        self.schema
    }

    /// 表 revision（基表=1；每次合法追加 +1）。
    pub fn revision(&self) -> u32 {
        self.revision
    }

    /// 当前表指纹。
    pub fn fingerprint(&self) -> u64 {
        self.fingerprint
    }

    /// 条目数。
    pub fn entry_count(&self) -> usize {
        self.entries.len()
    }

    /// 条目切片（只读）。
    pub fn entries(&self) -> &[MapEntry] {
        &self.entries
    }

    /// 审计行切片（只读）。
    pub fn audit_rows(&self) -> &[AuditRow] {
        &self.audit
    }

    /// 审计账溢出旗标（满后只计数不落账）。
    pub fn audit_overflow(&self) -> bool {
        self.audit_overflow
    }

    /// 历史拒绝改写总数（含溢出未落账的）。
    pub fn deny_rewrite_total(&self) -> u64 {
        self.deny_rewrite_total
    }

    /// 查表：按效果取映射条目（while + 下标线性扫——表容量 24，可接受；
    /// const 语境禁坑不约束本运行期方法，但风格统一用 while）。
    pub fn lookup(&self, kind: EffectKind) -> Option<MapEntry> {
        let target = kind.as_u8();
        let mut i = 0;
        while i < self.entries.len() {
            if self.entries[i].kind.as_u8() == target {
                return Some(self.entries[i]);
            }
            i += 1;
        }
        None
    }

    /// 追加新条目（合法演进路径）：效果未映射且表未满才放行；
    /// 效果已映射 = 改写企图 → 拒绝带码 0x12D2 + 审计留痕；
    /// 表满 → 拒绝带码 0x12D3 + 审计留痕。
    pub fn append_entry(&mut self, entry: MapEntry) -> Result<(), EffMapError> {
        if self.lookup(entry.kind).is_some() {
            self.record_audit(AuditAction::DenyRewrite, entry.kind, "追加被拒：效果已有映射（只增不改）");
            return Err(EffMapError::EntryRewriteDenied);
        }
        if self.entries.len() >= EFFMAP_TABLE_CAP {
            self.record_audit(AuditAction::DenyRewrite, entry.kind, "追加被拒：表满（EFFMAP_TABLE_CAP）");
            return Err(EffMapError::TableFull);
        }
        if (entry.primary as usize) >= API_TABLE_LEN {
            self.record_audit(AuditAction::DenyRewrite, entry.kind, "追加被拒：主路径 API 未注册");
            return Err(EffMapError::ApiNotFound);
        }
        self.entries.push(entry);
        self.revision = self.revision.saturating_add(1);
        self.fingerprint = fnv1a_entries(&self.entries);
        self.record_audit(AuditAction::Append, entry.kind, "追加成功：新效果映射入表");
        Ok(())
    }

    /// 改写既有条目：**恒拒**（只增不改红线）。带码 0x12D2 +
    /// 审计留痕 + 降级建议（见诊断详情）。表内容保证不变。
    pub fn amend_entry(&mut self, kind: EffectKind, _new_primary: ApiIndex) -> Result<(), EffMapError> {
        self.deny_rewrite_total = self.deny_rewrite_total.saturating_add(1);
        self.record_audit(
            AuditAction::DenyRewrite,
            kind,
            "改写企图被拒：只增不改——如需修订请新增条目并走版本发布流程",
        );
        Err(EffMapError::EntryRewriteDenied)
    }

    /// 指纹自检：重算当前条目集指纹并与记录值比对。
    pub fn verify_fingerprint(&self) -> bool {
        self.fingerprint == fnv1a_entries(&self.entries)
    }

    /// 审计落账（账满则置溢出旗标，只计数不落账）。
    fn record_audit(&mut self, action: AuditAction, kind: EffectKind, note: &'static str) {
        if self.audit.len() >= EFFMAP_AUDIT_CAP {
            self.audit_overflow = true;
            return;
        }
        let row = AuditRow {
            revision: self.revision,
            action,
            kind: kind.as_u8(),
            fingerprint_after: self.fingerprint,
            note,
        };
        self.audit.push(row);
    }
}

// ---------------------------------------------------------------------------
// 七、硬件红线卫兵（红线清单 + 命中即拒 + 留痕 + 降级建议）
// ---------------------------------------------------------------------------

/// 句柄域闭集（请求携带判别值；闭集外显性拒绝 0x12DD）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum HandleScope {
    /// 自有窗口（本进程本会话创建的 HWND）——唯一全放行域。
    OwnWindow = 1,
    /// 同会话（同桌面会话内他人窗口）——受控放行（仅公开 API）。
    SameSession = 2,
    /// 跨进程——越权句柄红线。
    CrossProcess = 3,
    /// 系统域（SYSTEM/服务会话窗口）——越权句柄红线。
    System = 4,
}

/// 红线规则判别值（卫兵按规则逐条扫，命中留痕）。
pub const RL_KERNEL_DIRECT: u8 = 1;
pub const RL_PRIVILEGED_HANDLE: u8 = 2;
pub const RL_UNDECLARED_CAP: u8 = 3;
pub const RL_SECURE_SURFACE: u8 = 4;

/// 红线规则数量。
pub const REDLINE_RULE_COUNT: usize = 4;

/// 硬件红线清单（判据"红线清单+拒绝留痕"主体）：规则名 + 降级建议
/// 路径，const 写死。命中任一规则 → 该 API 直调被拒，留痕
/// [`DenyTrace`] 并给出降级建议。
pub const REDLINE_RULES: [(&'static str, &'static str); REDLINE_RULE_COUNT] = [
    (
        // 规则 1：直调内核（win32k.sys 系统调用族）。用户态图形子系统
        // 必须走公开用户态入口，绕过 win32k 校验与 UIPI 一律拒绝。
        "kernel.direct_call",
        "降级建议：改走 A01 SetWindowCompositionAttribute 公开用户态入口（能力受控），或软件合成 A00",
    ),
    (
        // 规则 2：越权句柄（跨进程/系统域句柄）。窗体组合属性只允许
        // 施加于自有窗口句柄；跨进程句柄属越权面。
        "handle.privileged",
        "降级建议：把句柄域收敛到自有窗口（HandleScope::OwnWindow）后重试",
    ),
    (
        // 规则 3：未声明能力。API 表每条目声明 required_cap，调用方
        // 位集不满足即拒——能力是颁发出来的，不是默认有的。
        "capability.undeclared",
        "降级建议：先声明缺失能力位（见 API_TABLE.required_cap），或走软件合成 A00",
    ),
    (
        // 规则 4：安全表面（安全桌面/锁屏/UAC 窗）。任何原生组合属性
        // 都不上安全表面；软件合成路径（A00）不受此规则限制。
        "surface.secure",
        "降级建议：安全表面一律软件合成 A00，并禁用任何原语捕获",
    ),
];

/// 红线规则名（按判别值取）。
pub const fn rule_name(rule: u8) -> &'static str {
    match rule {
        RL_KERNEL_DIRECT => "kernel.direct_call",
        RL_PRIVILEGED_HANDLE => "handle.privileged",
        RL_UNDECLARED_CAP => "capability.undeclared",
        RL_SECURE_SURFACE => "surface.secure",
        _ => "unknown.rule",
    }
}

/// 红线降级建议（按判别值取；命中留痕里必须带建议路径）。
pub const fn rule_degrade_hint(rule: u8) -> &'static str {
    match rule {
        RL_KERNEL_DIRECT => REDLINE_RULES[0].1,
        RL_PRIVILEGED_HANDLE => REDLINE_RULES[1].1,
        RL_UNDECLARED_CAP => REDLINE_RULES[2].1,
        RL_SECURE_SURFACE => REDLINE_RULES[3].1,
        _ => "降级建议：未知规则——冻结该调用并上报审计",
    }
}

/// 红线卫兵扫描裁决：allowed = 未命中任何规则；命中则带规则判别值
/// 列表与首个命中的诊断码。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GuardVerdict {
    /// 是否放行。
    pub allowed: bool,
    /// 命中规则判别值（定长 4 槽，未用为 0）。
    pub hits: [u8; 4],
    /// 命中数。
    pub hit_count: usize,
    /// 首命中诊断码（命中恒为 0x12D4；未命中 0）。
    pub first_code: u16,
}

/// 拒绝留痕行（卫兵环形账；满则挤出最旧）。
#[derive(Debug, Clone, Copy)]
pub struct DenyTrace {
    /// 留痕序号（单调）。
    pub seq: u64,
    /// 命中规则判别值。
    pub rule: u8,
    /// 被拒 API 下标。
    pub api_index: ApiIndex,
    /// 涉事效果判别值。
    pub effect: u8,
    /// 诊断码（恒 0x12D4）。
    pub code: u16,
    /// 降级建议路径（写死字符串）。
    pub degrade_hint: &'static str,
}

/// API 名安全取值（越界给占位名，不留 panic 面）。
pub fn api_name_of(api_index: ApiIndex) -> &'static str {
    if (api_index as usize) < API_TABLE_LEN {
        API_TABLE[api_index as usize].api_name
    } else {
        "未知API"
    }
}

impl DenyTrace {
    /// 带码留痕行（alloc 文本；供日志总线消费）。
    pub fn render(&self) -> String {
        format!(
            "[红线-{}] api={}({}) effect={} code=0x{:04X} {}",
            rule_name(self.rule),
            api_name_of(self.api_index),
            self.api_index,
            self.effect,
            self.code,
            self.degrade_hint
        )
    }
}

/// 硬件红线卫兵：四条规则（[`REDLINE_RULES`]）逐条扫 + 拒绝留痕 +
/// 规则级实测计数。卫兵只裁决不规划——裁决结果交给
/// [`SafeCallPlanner`] 决定降级路径。
pub struct RedLineGuard {
    traces: Vec<DenyTrace>,
    next_seq: u64,
    scan_total: u64,
    deny_total: u64,
    rule_counts: [u64; REDLINE_RULE_COUNT + 1], // 下标 0 弃用，1..=4 按规则
}

impl RedLineGuard {
    /// 新卫兵。
    pub fn new() -> Self {
        RedLineGuard {
            traces: Vec::new(),
            next_seq: 1,
            scan_total: 0,
            deny_total: 0,
            rule_counts: [0; REDLINE_RULE_COUNT + 1],
        }
    }

    /// 扫描总次数（实测计数）。
    pub fn scan_total(&self) -> u64 {
        self.scan_total
    }

    /// 拒绝总次数（实测计数）。
    pub fn deny_total(&self) -> u64 {
        self.deny_total
    }

    /// 按规则计数（rule 取 1..=4；越界返回 0）。
    pub fn rule_count(&self, rule: u8) -> u64 {
        if (rule as usize) < self.rule_counts.len() {
            self.rule_counts[rule as usize]
        } else {
            0
        }
    }

    /// 留痕行切片（只读）。
    pub fn traces(&self) -> &[DenyTrace] {
        &self.traces
    }

    /// 扫描一次（命中即拒 + 留痕）。**前置条件**：api_index 已由调用方
    /// 校验过在 API_TABLE 闭集内（计划器先行校验；越界属调用方契约违例，
    /// 卫兵不做越界兜底——契约违例在请求合法性关卡就应被拦下）。
    ///
    /// 规则次序：安全面 → 内核直调 → 越权句柄 → 未声明能力（全命中全留痕）。
    pub fn scan(&mut self, api_index: ApiIndex, effect: EffectKind, req: &MapRequest) -> GuardVerdict {
        self.scan_total = self.scan_total.saturating_add(1);
        let api = API_TABLE[api_index as usize];
        let mut hits: [u8; 4] = [0; 4];
        let mut hit_count = 0usize;

        // 规则 4：安全表面——原生 API 全拒；软件合成（A00，无系统调用）
        // 不受限，否则安全面上连兜底都不可用。
        if req.secure_surface && api_index != API_SOFTWARE {
            hits[hit_count] = RL_SECURE_SURFACE;
            hit_count += 1;
        }
        // 规则 1：内核直调（红线分类 2）。
        if api.redline_class == RLC_REDLINE {
            hits[hit_count] = RL_KERNEL_DIRECT;
            hit_count += 1;
        }
        // 规则 2：越权句柄（跨进程/系统域；软件合成无句柄不受限）。
        if api_index != API_SOFTWARE && (req.handle_scope == HandleScope::CrossProcess as u8 || req.handle_scope == HandleScope::System as u8)
        {
            hits[hit_count] = RL_PRIVILEGED_HANDLE;
            hit_count += 1;
        }
        // 规则 3：未声明能力（位集不满足 required_cap）。
        if (api.required_cap & req.caps) != api.required_cap {
            hits[hit_count] = RL_UNDECLARED_CAP;
            hit_count += 1;
        }

        if hit_count == 0 {
            return GuardVerdict {
                allowed: true,
                hits,
                hit_count: 0,
                first_code: 0,
            };
        }

        // 命中即拒 + 按规则计数 + 留痕（每条命中规则各留一行）。
        self.deny_total = self.deny_total.saturating_add(1);
        let mut i = 0;
        while i < hit_count {
            let rule = hits[i];
            let slot = rule as usize;
            if slot < self.rule_counts.len() {
                self.rule_counts[slot] = self.rule_counts[slot].saturating_add(1);
            }
            if self.traces.len() >= GUARD_TRACE_CAP {
                self.traces.remove(0); // 环形挤出最旧
            }
            self.traces.push(DenyTrace {
                seq: self.next_seq,
                rule,
                api_index,
                effect: effect.as_u8(),
                code: EffMapError::RedLineViolation.code(),
                degrade_hint: rule_degrade_hint(rule),
            });
            self.next_seq = self.next_seq.wrapping_add(1);
            i += 1;
        }
        GuardVerdict {
            allowed: false,
            hits,
            hit_count,
            first_code: EffMapError::RedLineViolation.code(),
        }
    }

    /// 直调许可裁决（"这个 API 能不能直调"的显性问询）：放行 Ok；
    /// 命中红线 Err(0x12D4)，留痕与降级建议见 [`RedLineGuard::traces`]。
    pub fn enforce_direct(&mut self, api_index: ApiIndex, effect: EffectKind, req: &MapRequest) -> Result<(), EffMapError> {
        let verdict = self.scan(api_index, effect, req);
        if verdict.allowed {
            Ok(())
        } else {
            Err(EffMapError::RedLineViolation)
        }
    }
}

// ---------------------------------------------------------------------------
// 八、映射请求与安全调用计划器（效果 → API 序列 → 前置检查 → 回退链）
// ---------------------------------------------------------------------------

/// 映射请求（效果名 + 句柄域 + 能力位集 + 安全面旗标 + 打包系统版本）。
#[derive(Debug, Clone, Copy)]
pub struct MapRequest<'a> {
    /// 效果规范名（[`EffectKind::name()`] 闭集；表外显性拒绝）。
    pub effect_name: &'a str,
    /// 句柄域判别值（[`HandleScope`] 闭集 1..=4）。
    pub handle_scope: u8,
    /// 已声明能力位集（[`CAP_*`] 位或）。
    pub caps: u32,
    /// 是否安全表面（安全桌面/锁屏——原生 API 全拒）。
    pub secure_surface: bool,
    /// 打包系统版本（[`os_pack`]；0 视为非法请求）。
    pub os_build: u32,
}

/// 计划步状态（闭集）。
pub const STEP_PENDING: u8 = 0;
/// 计划步状态：选中（执行头）。
pub const STEP_SELECTED: u8 = 1;
/// 计划步状态：红线拒绝（卫兵命中）。
pub const STEP_REDLINE_DENIED: u8 = 2;
/// 计划步状态：系统版本不足跳过。
pub const STEP_OS_SKIP: u8 = 3;
/// 计划步状态：该效果未定义此 API 取值（链条与取值表失配）。
pub const STEP_VALUE_UNDEF: u8 = 4;

/// 单个计划步：API + 属性码 + 负载 + 布局 + 状态。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PlanStep {
    /// API 下标（A00~A08）。
    pub api_index: ApiIndex,
    /// 属性码（A02 的 DWMWA_* / A01 的 WCA_*；0 表无属性码语义）。
    pub attr_id: u32,
    /// 负载值（效果 × API 取值表 [`stage_spec`] 给出）。
    pub payload: u32,
    /// 参数布局号。
    pub layout: u16,
    /// 步状态（[`STEP_*`] 闭集）。
    pub status: u8,
}

impl PlanStep {
    /// 空步（const fn）。
    pub const fn empty() -> Self {
        PlanStep {
            api_index: API_NONE,
            attr_id: 0,
            payload: 0,
            layout: 0,
            status: STEP_PENDING,
        }
    }
}

/// 计划步槽数：5 链位（主路径 + 4 回退）+ 1 强制兜底位。
pub const PLAN_STEP_SLOTS: usize = 6;

/// 安全调用计划（效果 → API 序列的裁决产物）。
///
/// `steps` 覆盖整条被评估的链：每个被评估阶段都有终态
/// （选中/红线拒绝/版本跳过/取值未定义），`head` 指向执行头（首个
/// 可用阶段）。`degrade_stage` = 头步之前被跳过的阶段数（0 = 主路径
/// 直发；>0 = 实际发生了降级；强制软件兜底时再 +1 以区分链内回退与
/// 链外兜底）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CallPlan {
    /// 效果。
    pub effect: EffectKind,
    /// 计划步（6 槽定长；未用槽为空步）。
    pub steps: [PlanStep; PLAN_STEP_SLOTS],
    /// 执行头步下标。
    pub head: u8,
    /// 降级深度（头步前被跳过的阶段数；强制兜底再 +1）。
    pub degrade_stage: u8,
    /// 红线命中计数（实测；saturating）。
    pub redline_hits: u16,
    /// 计划时的打包系统版本。
    pub os_build: u32,
    /// 计划指纹（FNV-1a 规范序列；幂等对拍用）。
    pub plan_hash: u64,
}

/// 效果 × API 的取值表（属性码 + 负载）。
///
/// **显性闭集**：只覆盖基表各链条里真实出现的 (效果, API) 组合；
/// 组合外显性拒绝（0x12DB）——链条与取值表失配会在计划期被抓到
/// （步状态 STEP_VALUE_UNDEF），而不是编出默认值蒙混下发。
///
/// 取值来源（逐行注释）：
/// - A01（WCA_ACCENT_POLICY=19）：负载 = ACCENT_STATE 取值。
/// - A02：属性码 = DWMWA_*（dwmapi.h），负载 = 属性值。
/// - A03：属性码 = DWM_BB_ENABLE=0x1（dwmapi.h），负载 = fEnable(BOOL)。
/// - A04：负载 = MARGINS 打包（cyBottomHeight=1 → 1，底边延伸 1px）。
/// - A05：负载低 16 位 = LWA_* 旗标，高 16 位 = 参数（alpha/色值低位段）。
/// - A06：负载 = AW_* 旗标（winuser.h）。
/// - A07：负载 = 区域类型标记（2 = 圆角矩形，Varix 侧构造句柄）。
/// - A00：软件合成兜底，(0, 0)。
pub fn stage_spec(kind: EffectKind, api: ApiIndex) -> Result<(u32, u32), EffMapError> {
    let k = kind.as_u8();
    match (k, api) {
        // 高斯背景模糊
        (1, API_SWCA) => Ok((19, 3)),          // WCA_ACCENT_POLICY=19; ACCENT_ENABLE_BLURBEHIND=3
        (1, API_DWM_BLURBEHIND) => Ok((0x1, 1)), // DWM_BB_ENABLE=0x1; fEnable=TRUE
        // 亚克力
        (2, API_SWCA) => Ok((19, 4)),          // ACCENT_ENABLE_ACRYLICBLURBEHIND=4
        (2, API_DWMAPI_ATTR) => Ok((38, 3)),   // DWMWA_SYSTEMBACKDROP_TYPE=38; DWMSBT_TRANSIENTWINDOW=3
        // 云母
        (3, API_DWMAPI_ATTR) => Ok((38, 2)),   // DWMSBT_MAINWINDOW=2
        (3, API_SWCA) => Ok((19, 4)),          // 回退亚克力态
        // 云母 Alt
        (4, API_DWMAPI_ATTR) => Ok((38, 4)),   // DWMSBT_TABBEDWINDOW=4
        (4, API_SWCA) => Ok((19, 4)),          // 回退亚克力态
        // 窗体投影
        (5, API_DWM_MARGINS) => Ok((0, 1)),    // MARGINS: cyBottomHeight=1（底边延伸 1px）
        (5, API_DWMAPI_ATTR) => Ok((1, 1)),    // DWMWA_NCRENDERING_ENABLED=1; TRUE（前置确认）
        // 窗体圆角
        (6, API_DWMAPI_ATTR) => Ok((33, 2)),   // DWMWA_WINDOW_CORNER_PREFERENCE=33; DWMWCP_ROUND=2
        (6, API_WINDOW_RGN) => Ok((0, 2)),     // 区域类型标记 2 = 圆角矩形
        // 整窗透明度：LWA_ALPHA=0x2 | (alpha=200 << 16)
        (7, API_LAYERED_ATTRS) => Ok((0, (200 << 16) | 0x2)),
        (7, API_SWCA) => Ok((19, 2)),          // ACCENT_ENABLE_TRANSPARENTGRADIENT=2
        // 色键透明：LWA_COLORKEY=0x1 | (crKey=0x00FF00FF 品红 << 16)
        (8, API_LAYERED_ATTRS) => Ok((0, (0x00FF_00FF << 16) | 0x1)),
        // 暗色表面：DWMWA_USE_IMMERSIVE_DARK_MODE=20; TRUE
        (9, API_DWMAPI_ATTR) => Ok((20, 1)),
        // 淡入淡出动画：AW_BLEND=0x00080000
        (10, API_ANIMATE) => Ok((0, 0x0008_0000)),
        // 边框着色：DWMWA_BORDER_COLOR=34; ARGB 0xFF202020
        (11, API_DWMAPI_ATTR) => Ok((34, 0xFF20_2020)),
        // 渐变背景：ACCENT_ENABLE_GRADIENT=1
        (12, API_SWCA) => Ok((19, 1)),
        // 磨砂叠层：ACCENT_ENABLE_BLURBEHIND=3 + 染色（帧 extra 带色值）
        (13, API_SWCA) => Ok((19, 3)),
        (13, API_DWM_BLURBEHIND) => Ok((0x1, 1)),
        // 宿主背景底衬：ACCENT_ENABLE_HOSTBACKDROP=5
        (14, API_SWCA) => Ok((19, 5)),
        // 软件合成兜底：所有效果 × A00 一律 (0, 0)
        (_, API_SOFTWARE) => Ok((0, 0)),
        // 演进预留位的取值（追加映射时同源给值）
        (15, API_DWMAPI_ATTR) => Ok((17, 1)), // DWMWA_USE_HOSTBACKDROPBRUSH=17; TRUE
        // 其余组合显性拒绝：不猜默认值
        _ => Err(EffMapError::ApiNotFound),
    }
}

/// 幂等对拍报告（同请求双跑计划的比对结论）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct IdemReport {
    /// 双跑是否一致。
    pub consistent: bool,
    /// 第一次计划指纹。
    pub hash_first: u64,
    /// 第二次计划指纹。
    pub hash_second: u64,
}

/// 计划指纹（FNV-1a 规范序列：效果/头步/降级/命中/逐步四元组）。
fn compute_plan_hash(plan: &CallPlan) -> u64 {
    let mut h: u64 = 0xCBF2_9CE4_8422_2325;
    h = fnv_byte(h, plan.effect.as_u8());
    h = fnv_byte(h, plan.head);
    h = fnv_byte(h, plan.degrade_stage);
    h = fnv_byte(h, (plan.redline_hits & 0xFF) as u8);
    h = fnv_byte(h, ((plan.redline_hits >> 8) & 0xFF) as u8);
    let mut i = 0;
    while i < PLAN_STEP_SLOTS {
        let s = &plan.steps[i];
        h = fnv_byte(h, s.api_index);
        h = fnv_byte(h, s.status);
        h = fnv_byte(h, (s.layout & 0xFF) as u8);
        h = fnv_byte(h, (s.layout >> 8) as u8);
        let mut j = 0;
        while j < 4 {
            h = fnv_byte(h, ((s.attr_id >> (j * 8)) & 0xFF) as u8);
            h = fnv_byte(h, ((s.payload >> (j * 8)) & 0xFF) as u8);
            j += 1;
        }
        h = fnv_byte(h, 0x1F);
        i += 1;
    }
    h
}

/// 安全调用计划器（判据"安全调用路径"主体）：效果 → API 阶段链 →
/// 前置检查（请求合法性 → 系统版本 → 红线卫兵 → 取值表）→ 失败回退链
/// → 全链失败强制软件兜底 → 仍失败显性报错。计划器无状态（卫兵的实测
/// 计数不影响裁决），同输入必同输出——幂等对拍据此成立。
pub struct SafeCallPlanner;

impl SafeCallPlanner {
    /// 生成计划。
    pub fn plan(table: &EffMapTable, guard: &mut RedLineGuard, req: &MapRequest) -> Result<CallPlan, EffMapError> {
        // 前置检查 0：请求合法性（句柄域闭集 + 版本非零）。
        if req.handle_scope < HandleScope::OwnWindow as u8 || req.handle_scope > HandleScope::System as u8 {
            return Err(EffMapError::InvalidRequest);
        }
        if req.os_build == 0 {
            return Err(EffMapError::InvalidRequest);
        }
        // 前置检查 1：效果名闭集（未知显性拒绝不猜，0x12D0）。
        let kind = EffectKind::from_name(req.effect_name)?;
        // 前置检查 2：查表（闭集效果必映射；查不到 = 表损坏）。
        let entry = match table.lookup(kind) {
            Some(e) => e,
            None => return Err(EffMapError::PlanFailed),
        };
        if !table.verify_fingerprint() {
            return Err(EffMapError::FingerprintMismatch);
        }

        // 阶段链 = 主路径 + 4 回退槽。
        let stages: [ApiIndex; 5] = [
            entry.primary,
            entry.fallbacks[0],
            entry.fallbacks[1],
            entry.fallbacks[2],
            entry.fallbacks[3],
        ];
        let mut steps: [PlanStep; PLAN_STEP_SLOTS] = [PlanStep::empty(); PLAN_STEP_SLOTS];
        let mut head: u8 = 0xFF;
        let mut skipped: u8 = 0;
        let mut redline_hits: u16 = 0;
        // 条目级版本门槛：主路径另受映射条目自身 min_os 约束（如云母
        // 主路径 22H2 才有意义，低版本直接跳过主路径进回退链）。
        let primary_os_gate = req.os_build < entry.min_os;

        let mut i = 0usize;
        while i < 5 {
            let api_i = stages[i];
            if api_i == API_NONE {
                break;
            }
            if (api_i as usize) >= API_TABLE_LEN {
                return Err(EffMapError::ApiNotFound);
            }
            let api = API_TABLE[api_i as usize];
            let status;
            if (i == 0 && primary_os_gate) || req.os_build < api.min_os {
                // 前置检查 3：系统版本门槛（API 条目 min_os 整型比较）。
                status = STEP_OS_SKIP;
            } else {
                // 前置检查 4：红线卫兵（四规则）。
                let verdict = guard.scan(api_i, kind, req);
                if !verdict.allowed {
                    status = STEP_REDLINE_DENIED;
                    redline_hits = redline_hits.saturating_add(1);
                } else {
                    // 前置检查 5：取值表（组合外显性拒绝）。
                    match stage_spec(kind, api_i) {
                        Ok((attr, payload)) => {
                            steps[i] = PlanStep {
                                api_index: api_i,
                                attr_id: attr,
                                payload,
                                layout: api.layout,
                                status: STEP_SELECTED,
                            };
                            head = i as u8;
                            break;
                        }
                        Err(_) => {
                            status = STEP_VALUE_UNDEF;
                        }
                    }
                }
            }
            steps[i] = PlanStep {
                api_index: api_i,
                attr_id: 0,
                payload: 0,
                layout: api.layout,
                status,
            };
            skipped = skipped.saturating_add(1);
            i += 1;
        }

        if head == 0xFF {
            // 全链失败 → 强制软件合成兜底（回退链终点纪律的兜底之兜底）。
            let verdict = guard.scan(API_SOFTWARE, kind, req);
            if !verdict.allowed {
                return Err(EffMapError::PlanFailed);
            }
            let (attr, payload) = match stage_spec(kind, API_SOFTWARE) {
                Ok(v) => v,
                Err(_) => return Err(EffMapError::PlanFailed),
            };
            let api = API_TABLE[API_SOFTWARE as usize];
            steps[5] = PlanStep {
                api_index: API_SOFTWARE,
                attr_id: attr,
                payload,
                layout: api.layout,
                status: STEP_SELECTED,
            };
            head = 5;
            skipped = skipped.saturating_add(1);
        }

        if skipped > 200 {
            // 计划尺寸防线（语义上 skipped ≤ 6；此分支永不应达——
            // 到达即结构异常，显性拒绝而非继续）。
            return Err(EffMapError::PlanSizeOverLimit);
        }

        let draft = CallPlan {
            effect: kind,
            steps,
            head,
            degrade_stage: skipped,
            redline_hits,
            os_build: req.os_build,
            plan_hash: 0,
        };
        let plan = CallPlan {
            plan_hash: compute_plan_hash(&draft),
            ..draft
        };
        Ok(plan)
    }

    /// 幂等对拍：同请求双跑计划，比对计划指纹与结构相等性。
    pub fn verify_idempotency(
        table: &EffMapTable,
        guard: &mut RedLineGuard,
        req: &MapRequest,
    ) -> Result<IdemReport, EffMapError> {
        let a = Self::plan(table, guard, req)?;
        let b = Self::plan(table, guard, req)?;
        Ok(IdemReport {
            consistent: a == b && a.plan_hash == b.plan_hash,
            hash_first: a.plan_hash,
            hash_second: b.plan_hash,
        })
    }
}

// ---------------------------------------------------------------------------
// 九、参数编解码（32 字节定长帧；小端显性；校验和；往返自证）
// ---------------------------------------------------------------------------

/// 参数帧定长（字节）。
pub const PARAM_FRAME_LEN: usize = 32;

/// 参数帧布局（**小端显性**——逐字节手编，与宿主机字节序无关）：
///
/// ```text
/// [0..2)   u16 layout_tag   LE  布局号
/// [2..3)   u8  api_index        API 下标
/// [3..4)   u8  zero             保留 0
/// [4..8)   u32 attr_id       LE  属性码
/// [8..12)  u32 payload       LE  负载
/// [12..16) u32 extra0        LE  扩展字段 0
/// [16..20) u32 extra1        LE  扩展字段 1
/// [20..28) 8×0x00               保留域（解码校验必须为 0）
/// [28..32) u32 checksum      LE  FNV-1a(0..28) 截断 u32
/// ```
pub fn encode_call_params(
    api_index: u8,
    layout: u16,
    attr_id: u32,
    payload: u32,
    extra: [u32; 2],
) -> Result<[u8; PARAM_FRAME_LEN], EffMapError> {
    // 布局闭集校验（表外布局显性拒绝 0x12D6）。
    if layout >= LAYOUT_KIND_COUNT {
        return Err(EffMapError::ParamEncodeOverflow);
    }
    if (api_index as usize) >= API_TABLE_LEN {
        return Err(EffMapError::ApiNotFound);
    }
    let mut frame = [0u8; PARAM_FRAME_LEN];
    frame[0] = (layout & 0xFF) as u8;
    frame[1] = (layout >> 8) as u8;
    frame[2] = api_index;
    frame[3] = 0;
    frame[4] = (attr_id & 0xFF) as u8;
    frame[5] = ((attr_id >> 8) & 0xFF) as u8;
    frame[6] = ((attr_id >> 16) & 0xFF) as u8;
    frame[7] = ((attr_id >> 24) & 0xFF) as u8;
    frame[8] = (payload & 0xFF) as u8;
    frame[9] = ((payload >> 8) & 0xFF) as u8;
    frame[10] = ((payload >> 16) & 0xFF) as u8;
    frame[11] = ((payload >> 24) & 0xFF) as u8;
    frame[12] = (extra[0] & 0xFF) as u8;
    frame[13] = ((extra[0] >> 8) & 0xFF) as u8;
    frame[14] = ((extra[0] >> 16) & 0xFF) as u8;
    frame[15] = ((extra[0] >> 24) & 0xFF) as u8;
    frame[16] = (extra[1] & 0xFF) as u8;
    frame[17] = ((extra[1] >> 8) & 0xFF) as u8;
    frame[18] = ((extra[1] >> 16) & 0xFF) as u8;
    frame[19] = ((extra[1] >> 24) & 0xFF) as u8;
    // 20..28 保留域已由零值数组覆盖。
    let sum = param_checksum(&frame);
    frame[28] = (sum & 0xFF) as u8;
    frame[29] = ((sum >> 8) & 0xFF) as u8;
    frame[30] = ((sum >> 16) & 0xFF) as u8;
    frame[31] = ((sum >> 24) & 0xFF) as u8;
    Ok(frame)
}

/// FNV-1a 截断 u32 校验和（对帧前 28 字节）。
fn param_checksum(frame: &[u8; PARAM_FRAME_LEN]) -> u32 {
    let mut h: u64 = 0xCBF2_9CE4_8422_2325;
    let mut i = 0;
    while i < 28 {
        h = fnv_byte(h, frame[i]);
        i += 1;
    }
    (h & 0xFFFF_FFFF) as u32
}

/// 解码产物（字段还原）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DecodedParams {
    /// 布局号。
    pub layout: u16,
    /// API 下标。
    pub api_index: u8,
    /// 属性码。
    pub attr_id: u32,
    /// 负载。
    pub payload: u32,
    /// 扩展字段。
    pub extra: [u32; 2],
}

/// 解码参数帧：长度必须恰为 32、保留域必须为 0、校验和必须匹配；
/// 任一不符 → 0x12D7（帧可能被截断或篡改，整帧丢弃重编）。
pub fn decode_call_params(frame: &[u8]) -> Result<DecodedParams, EffMapError> {
    if frame.len() != PARAM_FRAME_LEN {
        return Err(EffMapError::ParamDecodeMismatch);
    }
    let mut buf = [0u8; PARAM_FRAME_LEN];
    let mut i = 0;
    while i < PARAM_FRAME_LEN {
        buf[i] = frame[i];
        i += 1;
    }
    // 保留域校验（3..4 单字节 + 20..28 八字节）。
    if buf[3] != 0 {
        return Err(EffMapError::ParamDecodeMismatch);
    }
    let mut j = 20;
    while j < 28 {
        if buf[j] != 0 {
            return Err(EffMapError::ParamDecodeMismatch);
        }
        j += 1;
    }
    let stored = (buf[28] as u32)
        | ((buf[29] as u32) << 8)
        | ((buf[30] as u32) << 16)
        | ((buf[31] as u32) << 24);
    if stored != param_checksum(&buf) {
        return Err(EffMapError::ParamDecodeMismatch);
    }
    Ok(DecodedParams {
        layout: (buf[0] as u16) | ((buf[1] as u16) << 8),
        api_index: buf[2],
        attr_id: (buf[4] as u32)
            | ((buf[5] as u32) << 8)
            | ((buf[6] as u32) << 16)
            | ((buf[7] as u32) << 24),
        payload: (buf[8] as u32)
            | ((buf[9] as u32) << 8)
            | ((buf[10] as u32) << 16)
            | ((buf[11] as u32) << 24),
        extra: [
            (buf[12] as u32)
                | ((buf[13] as u32) << 8)
                | ((buf[14] as u32) << 16)
                | ((buf[15] as u32) << 24),
            (buf[16] as u32)
                | ((buf[17] as u32) << 8)
                | ((buf[18] as u32) << 16)
                | ((buf[19] as u32) << 24),
        ],
    })
}

/// 编码往返自证：编码 → 解码 → 字段级全等比对；任一字段漂移 →
/// 0x12D7（往返失配）。健康体检与测试均复用本函数。
pub fn verify_param_roundtrip(
    api_index: u8,
    layout: u16,
    attr_id: u32,
    payload: u32,
    extra: [u32; 2],
) -> Result<(), EffMapError> {
    let frame = encode_call_params(api_index, layout, attr_id, payload, extra)?;
    let back = decode_call_params(&frame)?;
    if back.layout != layout
        || back.api_index != api_index
        || back.attr_id != attr_id
        || back.payload != payload
        || back.extra[0] != extra[0]
        || back.extra[1] != extra[1]
    {
        return Err(EffMapError::ParamDecodeMismatch);
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// 十、事件账（查表命中 / 红线拒绝 / 回退触发 三类计数 + 定长环形账）
// ---------------------------------------------------------------------------

/// 事件类别：查表命中。
pub const EVENT_LOOKUP_HIT: u8 = 1;
/// 事件类别：红线拒绝。
pub const EVENT_REDLINE_REJECT: u8 = 2;
/// 事件类别：回退触发（链内降级或强制兜底）。
pub const EVENT_FALLBACK: u8 = 3;

/// 单条事件（定长小结构）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MapEvent {
    /// 事件序号（单调）。
    pub seq: u32,
    /// 事件类别（1/2/3）。
    pub class: u8,
    /// 效果判别值。
    pub effect: u8,
    /// API 下标。
    pub api: u8,
    /// 关联诊断码（无则 0）。
    pub code: u16,
}

/// 事件账摘要。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LedgerSummary {
    /// 累计入账总数。
    pub recorded_total: u64,
    /// 查表命中累计。
    pub hits: u64,
    /// 红线拒绝累计。
    pub redlines: u64,
    /// 回退触发累计。
    pub fallbacks: u64,
    /// 被环形覆写挤出的旧事件数。
    pub dropped: u64,
    /// 账面占用（千分比）。
    pub fill_permille: u16,
    /// 自检结论（计数守恒 + 账面合法）。
    pub consistent: bool,
}

/// 定长事件账（128 槽环形）：满后新事件覆写最旧槽并计 dropped；
/// 三类计数独立累加（saturating），`selfcheck()` 校验计数守恒。
pub struct MapEventLedger {
    events: [MapEvent; LEDGER_CAP],
    len: usize,
    next_seq: u32,
    recorded_total: u64,
    hits: u64,
    redlines: u64,
    fallbacks: u64,
    dropped: u64,
}

impl MapEventLedger {
    /// 新账。
    pub fn new() -> Self {
        MapEventLedger {
            events: [MapEvent {
                seq: 0,
                class: 0,
                effect: 0,
                api: 0,
                code: 0,
            }; LEDGER_CAP],
            len: 0,
            next_seq: 0, // 序号从 0 起：首笔事件落槽 0，账面无零值脏行
            recorded_total: 0,
            hits: 0,
            redlines: 0,
            fallbacks: 0,
            dropped: 0,
        }
    }

    /// 记一笔（类别闭集外静默忽略——账不收脏数据）。
    pub fn record(&mut self, class: u8, effect: u8, api: u8, code: u16) {
        if class < EVENT_LOOKUP_HIT || class > EVENT_FALLBACK {
            return;
        }
        let seq = self.next_seq;
        let slot = (seq as usize) % LEDGER_CAP;
        self.events[slot] = MapEvent {
            seq,
            class,
            effect,
            api,
            code,
        };
        self.next_seq = self.next_seq.wrapping_add(1);
        self.recorded_total = self.recorded_total.saturating_add(1);
        if self.len < LEDGER_CAP {
            self.len += 1;
        } else {
            self.dropped = self.dropped.saturating_add(1);
        }
        match class {
            EVENT_LOOKUP_HIT => self.hits = self.hits.saturating_add(1),
            EVENT_REDLINE_REJECT => self.redlines = self.redlines.saturating_add(1),
            _ => self.fallbacks = self.fallbacks.saturating_add(1),
        }
    }

    /// 账面行数。
    pub fn len(&self) -> usize {
        self.len
    }

    /// 账面是否为空。
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// 按槽位取行（越界 None）。
    pub fn row(&self, index: usize) -> Option<MapEvent> {
        if index < self.len {
            Some(self.events[index])
        } else {
            None
        }
    }

    /// 三类计数（只读快照）。
    pub fn counters(&self) -> (u64, u64, u64) {
        (self.hits, self.redlines, self.fallbacks)
    }

    /// 自检：三类计数之和 == 入账总数；账面行数 == min(总数, 容量)；
    /// 账面每行类别合法。
    pub fn selfcheck(&self) -> bool {
        let sum = self.hits.saturating_add(self.redlines).saturating_add(self.fallbacks);
        if sum != self.recorded_total {
            return false;
        }
        let expect_len = if self.recorded_total as usize <= LEDGER_CAP {
            self.recorded_total as usize
        } else {
            LEDGER_CAP
        };
        if self.len != expect_len {
            return false;
        }
        let mut i = 0;
        while i < self.len {
            let ev = self.events[i];
            if ev.class < EVENT_LOOKUP_HIT || ev.class > EVENT_FALLBACK {
                return false;
            }
            i += 1;
        }
        true
    }

    /// 摘要快照。
    pub fn summary(&self) -> LedgerSummary {
        LedgerSummary {
            recorded_total: self.recorded_total,
            hits: self.hits,
            redlines: self.redlines,
            fallbacks: self.fallbacks,
            dropped: self.dropped,
            fill_permille: (((self.len as u32) * 1000 / (LEDGER_CAP as u32)) as u16),
            consistent: self.selfcheck(),
        }
    }
}

// ---------------------------------------------------------------------------
// 十一、引擎门面（映射总入口 + 幂等双跑 + health 体检）
// ---------------------------------------------------------------------------

/// 体检样本请求（health() 幂等自检固定用此样本，保证体检本身可复现）。
pub const SAMPLE_HEALTH_REQUEST: MapRequest<'static> = MapRequest {
    effect_name: "blur.gaussian",
    handle_scope: 1, // OwnWindow
    caps: CAP_ALL,
    secure_surface: false,
    os_build: OS_HEALTH_SAMPLE,
};

/// 体检状态。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum HealthStatus {
    /// 绿灯：全项通过。
    Green = 1,
    /// 黄灯：高水位/审计溢出等需关注项。
    Amber = 2,
    /// 红灯：指纹/账本/幂等等结构性不符。
    Red = 3,
}

/// 体检报告。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EffMapHealth {
    /// 总体状态。
    pub status: HealthStatus,
    /// 表条目数。
    pub table_entries: usize,
    /// 表 revision。
    pub table_revision: u32,
    /// 当前表指纹。
    pub table_fingerprint: u64,
    /// 指纹自检（重算比对）。
    pub fingerprint_ok: bool,
    /// 事件账摘要。
    pub ledger: LedgerSummary,
    /// 卫兵留痕行数。
    pub guard_traces: usize,
    /// 卫兵拒绝总数。
    pub guard_denies: u64,
    /// 审计行数。
    pub audit_rows: usize,
    /// 审计账溢出旗标。
    pub audit_overflow: bool,
    /// 幂等自检结论。
    pub idempotency_ok: bool,
    /// 计划器运行总数。
    pub planner_runs: u64,
    /// 问题清单（≤4 条；未用槽为空串）。
    pub issues: [&'static str; 4],
    /// 问题条数。
    pub issue_count: usize,
}

/// 效果映射引擎门面：表 + 卫兵 + 计划器 + 事件账 + 实测计数的总装。
///
/// 判据"测试（映射/安全两组）"在模块尾以两个测试组落实；本门面的
/// `map_effect()` 是唯一映射入口——映射必须过卫兵、必须留账。
pub struct EffMapEngine {
    table: EffMapTable,
    guard: RedLineGuard,
    ledger: MapEventLedger,
    planner_runs: u64,
    unknown_rejects: u64,
    idempotency_checks: u64,
}

impl EffMapEngine {
    /// 新引擎（基表 V1 挂载）。
    pub fn new() -> Result<Self, EffMapError> {
        Ok(EffMapEngine {
            table: EffMapTable::new()?,
            guard: RedLineGuard::new(),
            ledger: MapEventLedger::new(),
            planner_runs: 0,
            unknown_rejects: 0,
            idempotency_checks: 0,
        })
    }

    /// 映射表（只读）。
    pub fn table(&self) -> &EffMapTable {
        &self.table
    }

    /// 红线卫兵（只读；留痕可查）。
    pub fn guard(&self) -> &RedLineGuard {
        &self.guard
    }

    /// 事件账（只读；摘要可查）。
    pub fn ledger(&self) -> &MapEventLedger {
        &self.ledger
    }

    /// 计划器运行总数。
    pub fn planner_runs(&self) -> u64 {
        self.planner_runs
    }

    /// 未知效果拒绝总数。
    pub fn unknown_rejects(&self) -> u64 {
        self.unknown_rejects
    }

    /// 幂等对拍运行总数。
    pub fn idempotency_checks(&self) -> u64 {
        self.idempotency_checks
    }

    /// 映射总入口：计划 → 留账（命中/红线/回退三类事件）。
    pub fn map_effect(&mut self, req: &MapRequest) -> Result<CallPlan, EffMapError> {
        self.planner_runs = self.planner_runs.saturating_add(1);
        match SafeCallPlanner::plan(&self.table, &mut self.guard, req) {
            Ok(plan) => {
                let head_api = plan.steps[plan.head as usize].api_index;
                self.ledger
                    .record(EVENT_LOOKUP_HIT, plan.effect.as_u8(), head_api, 0);
                // 每个被红线拒绝的阶段各记一笔红线事件。
                let mut s = 0;
                while s < PLAN_STEP_SLOTS {
                    if plan.steps[s].status == STEP_REDLINE_DENIED {
                        self.ledger.record(
                            EVENT_REDLINE_REJECT,
                            plan.effect.as_u8(),
                            plan.steps[s].api_index,
                            EffMapError::RedLineViolation.code(),
                        );
                    }
                    s += 1;
                }
                if plan.degrade_stage > 0 {
                    self.ledger
                        .record(EVENT_FALLBACK, plan.effect.as_u8(), head_api, 0);
                }
                Ok(plan)
            }
            Err(e) => {
                if e.code() == EffMapError::RedLineViolation.code() {
                    self.ledger.record(
                        EVENT_REDLINE_REJECT,
                        0,
                        API_NONE,
                        e.code(),
                    );
                } else {
                    self.unknown_rejects = self.unknown_rejects.saturating_add(1);
                }
                Err(e)
            }
        }
    }

    /// 直调许可问询（"这个 API 能不能直调"）。
    pub fn check_direct_call(
        &mut self,
        api_index: ApiIndex,
        effect: EffectKind,
        req: &MapRequest,
    ) -> Result<(), EffMapError> {
        if (api_index as usize) >= API_TABLE_LEN {
            return Err(EffMapError::ApiNotFound);
        }
        self.guard.enforce_direct(api_index, effect, req)
    }

    /// 幂等对拍（双跑同请求，比对计划指纹）。
    pub fn run_idempotency_check(&mut self, req: &MapRequest) -> Result<IdemReport, EffMapError> {
        self.idempotency_checks = self.idempotency_checks.saturating_add(1);
        self.planner_runs = self.planner_runs.saturating_add(2);
        SafeCallPlanner::verify_idempotency(&self.table, &mut self.guard, req)
    }

    /// 体检（判据"判据"的运行期落点）：指纹自检 + 账本自检 + 幂等样本
    /// 自检 + 高水位审视；红灯项 → [`HealthStatus::Red`]，黄灯项 →
    /// [`HealthStatus::Amber`]，全过 → [`HealthStatus::Green`]。
    pub fn health(&mut self) -> EffMapHealth {
        let mut issues: [&'static str; 4] = ["", "", "", ""];
        let mut issue_count = 0usize;

        let fingerprint_ok = self.table.verify_fingerprint();
        if !fingerprint_ok {
            issues[issue_count] = "表指纹不符（红灯）：疑似越权改写，拒绝服务并走审计";
            issue_count += 1;
        }

        let led = self.ledger.summary();
        if !led.consistent {
            issues[issue_count] = "事件账自检不符（红灯）：计数不守恒或账面脏行";
            issue_count += 1;
        }

        let idem = self.run_idempotency_check(&SAMPLE_HEALTH_REQUEST);
        let idempotency_ok = matches!(idem, Ok(r) if r.consistent);
        if !idempotency_ok {
            issues[issue_count] = "幂等自检漂移（红灯）：同请求双跑计划不一致";
            issue_count += 1;
        }

        let audit_overflow = self.table.audit_overflow();
        if audit_overflow {
            issues[issue_count] = "审计账溢出（黄灯）：先归档审计账再继续演进";
            issue_count += 1;
        }

        let mut status = HealthStatus::Green;
        let mut i = 0;
        while i < issue_count {
            // 红灯关键词在 issues 前三条（结构项），黄灯在审计项。
            if issues[i].starts_with("表指纹") || issues[i].starts_with("事件账") || issues[i].starts_with("幂等") {
                status = HealthStatus::Red;
            } else if status == HealthStatus::Green {
                status = HealthStatus::Amber;
            }
            i += 1;
        }
        if led.fill_permille >= 900 && status == HealthStatus::Green {
            status = HealthStatus::Amber;
            if issue_count < 4 {
                issues[issue_count] = "事件账高水位（黄灯）：即将进入环形覆写";
                issue_count += 1;
            }
        }

        EffMapHealth {
            status,
            table_entries: self.table.entry_count(),
            table_revision: self.table.revision(),
            table_fingerprint: self.table.fingerprint(),
            fingerprint_ok,
            ledger: led,
            guard_traces: self.guard.traces().len(),
            guard_denies: self.guard.deny_total(),
            audit_rows: self.table.audit_rows().len(),
            audit_overflow,
            idempotency_ok,
            planner_runs: self.planner_runs,
            issues,
            issue_count,
        }
    }

    /// 体检达标断言（红灯即 Err(0x12DF)，供外层服务门禁消费）。
    pub fn assert_healthy(&mut self) -> Result<EffMapHealth, EffMapError> {
        let h = self.health();
        if h.status == HealthStatus::Red {
            Err(EffMapError::HealthDegraded)
        } else {
            Ok(h)
        }
    }

    /// 自检报告文本（alloc；供日志总线/诊断面板消费）。
    pub fn selfcheck_report(&mut self) -> String {
        let h = self.health();
        format!(
            "EFFMAP {} 表 {}条 rev{} 指纹=0x{:016X}{} 账 {}/{} 命中{} 红线{} 回退{} 溢出{} 幂等={} 卫兵拒{} 审计{}行 状态={}",
            EFFMAP_VERSION,
            h.table_entries,
            h.table_revision,
            h.table_fingerprint,
            if h.fingerprint_ok { "√" } else { "×" },
            h.ledger.recorded_total,
            LEDGER_CAP,
            h.ledger.hits,
            h.ledger.redlines,
            h.ledger.fallbacks,
            h.ledger.dropped,
            if h.idempotency_ok { "√" } else { "×" },
            h.guard_denies,
            h.audit_rows,
            match h.status {
                HealthStatus::Green => "绿",
                HealthStatus::Amber => "黄",
                HealthStatus::Red => "红",
            }
        )
    }
}

// ---------------------------------------------------------------------------
// 十二、编译期断言（const 级纪律封顶；运行期零 assert）
// ---------------------------------------------------------------------------

const _: () = assert!(PARAM_FRAME_LEN == 32, "参数帧必须 32 字节定长");
const _: () = assert!(API_TABLE_LEN == 9, "API 表必须 9 条（A00~A08）");
const _: () = assert!(EFFECT_KIND_COUNT == 16, "效果闭集必须 16 种");
const _: () = assert!(BASE_TABLE.len() == BASE_ENTRY_COUNT, "基表条目数必须与常量一致");
const _: () = assert!(base_entries_sane(&BASE_TABLE), "基表自检：链条收敛软件合成且无重复效果");
const _: () = assert!(BASE_FINGERPRINT != 0, "基表指纹不得为 0");
const _: () = assert!(0x12DF - 0x12D0 == 15, "诊断码段必须为 0x12D0~0x12DF 闭集");
const _: () = assert!(LEDGER_CAP == 128, "事件账定长 128 槽");
const _: () = assert!(EFFMAP_TABLE_CAP >= BASE_ENTRY_COUNT, "表容量必须容纳基表");
const _: () = assert!(CAP_ALL == 0x3F, "能力位集必须占满低 6 位");
const _: () = assert!(core::mem::size_of::<MapEntry>() <= 64, "MapEntry 栈上封顶 64B");
const _: () = assert!(core::mem::size_of::<NativeApiDesc>() <= 128, "NativeApiDesc 栈上封顶 128B");
const _: () = assert!(core::mem::size_of::<CallPlan>() <= 512, "CallPlan 栈上封顶 512B");
const _: () = assert!(core::mem::size_of::<MapEvent>() <= 16, "MapEvent 定长封顶 16B");
const _: () = assert!(core::mem::size_of::<MapEventLedger>() <= 4096, "事件账整体封顶 4KB");
const _: () = assert!(core::mem::size_of::<EffMapEngine>() <= 64 * 1024, "引擎 >64KB 禁上栈红线");

// ---------------------------------------------------------------------------
// 十三、测试（判据"映射/安全两组"）
// ---------------------------------------------------------------------------

#[cfg(test)]
mod map_group_tests {
    use super::*;

    #[test]
    fn base_lookup_all_entries() {
        let t = EffMapTable::new().unwrap();
        let mut i = 0;
        while i < BASE_ENTRY_COUNT {
            let kind = EffectKind::from_u8((i + 1) as u8).unwrap();
            let entry = t.lookup(kind).unwrap_or_else(|| panic!("基表缺 {}", kind.name()));
            assert!((entry.primary as usize) < API_TABLE_LEN);
            // 每条链必须收敛到软件合成（或主路径即软件合成）。
            let mut has_sw = entry.primary == API_SOFTWARE;
            let mut j = 0;
            while j < 4 {
                if entry.fallbacks[j] == API_SOFTWARE {
                    has_sw = true;
                }
                j += 1;
            }
            assert!(has_sw, "{} 回退链未收敛软件合成", kind.name());
            i += 1;
        }
        assert_eq!(t.entry_count(), BASE_ENTRY_COUNT);
        assert_eq!(t.revision(), 1);
        assert_eq!(t.fingerprint(), BASE_FINGERPRINT);
    }

    #[test]
    fn unknown_effect_explicitly_rejected() {
        // 名字层：表外名字显性拒绝（0x12D0）。
        assert_eq!(EffectKind::from_name("vapor.wave").unwrap_err().code(), 0x12D0);
        assert_eq!(EffectKind::from_name("").unwrap_err().code(), 0x12D0);
        assert_eq!(EffectKind::from_name("blur.gauss").unwrap_err().code(), 0x12D0); // 近似名也不猜
        // 计划层：未知效果名同样拒绝。
        let mut e = EffMapEngine::new().unwrap();
        let req = MapRequest {
            effect_name: "vapor.wave",
            handle_scope: 1,
            caps: CAP_ALL,
            secure_surface: false,
            os_build: OS_HEALTH_SAMPLE,
        };
        assert_eq!(e.map_effect(&req).unwrap_err().code(), 0x12D0);
        assert_eq!(e.unknown_rejects(), 1);
    }

    #[test]
    fn from_u8_roundtrip() {
        let mut v = 1;
        while v <= EFFECT_KIND_COUNT as u8 {
            let k = EffectKind::from_u8(v).unwrap_or_else(|| panic!("判别值 {} 缺失", v));
            assert_eq!(k.as_u8(), v);
            assert_eq!(EffectKind::from_name(k.name()).unwrap().as_u8(), v);
            v += 1;
        }
        assert!(EffectKind::from_u8(0).is_none());
        assert!(EffectKind::from_u8(17).is_none());
    }

    #[test]
    fn stage_spec_covers_all_base_chains() {
        let t = EffMapTable::new().unwrap();
        let mut i = 0;
        while i < BASE_ENTRY_COUNT {
            let entry = t.lookup(EffectKind::from_u8((i + 1) as u8).unwrap()).unwrap();
            let chain = [entry.primary, entry.fallbacks[0], entry.fallbacks[1], entry.fallbacks[2], entry.fallbacks[3]];
            let mut j = 0;
            while j < 5 {
                let api = chain[j];
                if api == API_NONE {
                    break;
                }
                let kind = EffectKind::from_u8((i + 1) as u8).unwrap();
                assert!(
                    stage_spec(kind, api).is_ok(),
                    "{} × A{} 无取值定义",
                    kind.name(),
                    api
                );
                j += 1;
            }
            i += 1;
        }
    }

    #[test]
    fn append_only_evolution_and_fingerprint() {
        let mut t = EffMapTable::new().unwrap();
        let fp_before = t.fingerprint();
        // 追加预留效果 LuminosityBlend（合法演进）。
        let new_entry = MapEntry {
            kind: EffectKind::LuminosityBlend,
            primary: API_DWMAPI_ATTR,
            fallbacks: [API_SOFTWARE, API_NONE, API_NONE, API_NONE],
            layout: LAYOUT_DWORD,
            min_os: OS_WIN10_1903,
            source_note: "dwmapi.h DWMWA_USE_HOSTBACKDROPBRUSH=17（10.0.18362）——演进 v2 追加",
        };
        assert!(t.append_entry(new_entry).is_ok());
        assert_eq!(t.revision(), 2);
        assert_ne!(t.fingerprint(), fp_before);
        assert!(t.verify_fingerprint());
        assert!(t.lookup(EffectKind::LuminosityBlend).is_some());
        // 审计账应有 1 行 Append。
        assert_eq!(t.audit_rows().len(), 1);
        assert_eq!(t.audit_rows()[0].action, AuditAction::Append);
        // 指纹稳定：同内容两表指纹一致。
        let t2 = EffMapTable::new().unwrap();
        assert_eq!(t2.fingerprint(), BASE_FINGERPRINT);
        assert_eq!(BASE_FINGERPRINT, fnv1a_entries(&BASE_TABLE));
    }

    #[test]
    fn amend_rewrite_always_denied_with_code() {
        let mut t = EffMapTable::new().unwrap();
        let fp_before = t.fingerprint();
        let rev_before = t.revision();
        // 改写旧条目：恒拒 + 带码 + 留痕 + 表内容不变。
        let err = t.amend_entry(EffectKind::BlurGaussian, API_NT_SWCA).unwrap_err();
        assert_eq!(err.code(), 0x12D2);
        assert_eq!(t.fingerprint(), fp_before);
        assert_eq!(t.revision(), rev_before);
        assert_eq!(t.deny_rewrite_total(), 1);
        let rows = t.audit_rows();
        assert_eq!(rows[rows.len() - 1].action, AuditAction::DenyRewrite);
        // 对不存在效果同样拒（只增不改与存在性无关）。
        assert_eq!(t.amend_entry(EffectKind::VibranceBoost, API_SOFTWARE).unwrap_err().code(), 0x12D2);
        // 重复追加同效果 = 改写企图，同拒。
        let dup = MapEntry {
            kind: EffectKind::BlurGaussian,
            primary: API_SOFTWARE,
            fallbacks: [API_NONE; 4],
            layout: LAYOUT_SW_COMPOSITE,
            min_os: 0,
            source_note: "重复追加企图",
        };
        assert_eq!(t.append_entry(dup).unwrap_err().code(), 0x12D2);
        assert_eq!(t.entry_count(), BASE_ENTRY_COUNT);
    }

    #[test]
    fn schema_gate_and_plan_happy_path() {
        // schema 门禁：高版本表拒绝挂载（0x12D1）。
        assert_eq!(EffMapTable::load_with_schema(99).unwrap_err().code(), 0x12D1);
        // 幸福路径：全能力 + 自有窗口 → 模糊主路径 A01 直发。
        let mut e = EffMapEngine::new().unwrap();
        let req = MapRequest {
            effect_name: "blur.gaussian",
            handle_scope: 1,
            caps: CAP_ALL,
            secure_surface: false,
            os_build: OS_HEALTH_SAMPLE,
        };
        let plan = e.map_effect(&req).unwrap();
        assert_eq!(plan.effect, EffectKind::BlurGaussian);
        assert_eq!(plan.head, 0);
        assert_eq!(plan.degrade_stage, 0);
        assert_eq!(plan.steps[0].api_index, API_SWCA);
        assert_eq!(plan.steps[0].attr_id, 19); // WCA_ACCENT_POLICY
        assert_eq!(plan.steps[0].payload, 3); // ACCENT_ENABLE_BLURBEHIND
        assert!(plan.plan_hash != 0);
    }

    #[test]
    fn param_roundtrip_all_layouts() {
        // 8 种布局逐一往返自证。
        assert!(verify_param_roundtrip(API_SWCA, LAYOUT_ACCENT_POLICY, 19, 3, [0xFF00FF00, 0]).is_ok());
        assert!(verify_param_roundtrip(API_DWMAPI_ATTR, LAYOUT_DWORD, 38, 2, [0, 0]).is_ok());
        assert!(verify_param_roundtrip(API_DWM_BLURBEHIND, LAYOUT_BLURBEHIND, 0x1, 1, [0xDEAD, 0xBEEF]).is_ok());
        assert!(verify_param_roundtrip(API_DWM_MARGINS, LAYOUT_MARGINS, 0, 1, [0, 0]).is_ok());
        assert!(verify_param_roundtrip(API_LAYERED_ATTRS, LAYOUT_LAYERED_ATTRS, 0, (200 << 16) | 2, [0, 0]).is_ok());
        assert!(verify_param_roundtrip(API_ANIMATE, LAYOUT_ANIMATE, 0, 0x0008_0000, [0, 0]).is_ok());
        assert!(verify_param_roundtrip(API_WINDOW_RGN, LAYOUT_WINDOW_RGN, 0, 2, [0, 0]).is_ok());
        assert!(verify_param_roundtrip(API_SOFTWARE, LAYOUT_SW_COMPOSITE, 0, 0, [0, 0]).is_ok());
        // 表外布局显性拒绝（0x12D6）。
        assert_eq!(encode_call_params(1, 99, 0, 0, [0; 2]).unwrap_err().code(), 0x12D6);
        // 未注册 API 拒绝（0x12DB）。
        assert_eq!(encode_call_params(200, 1, 0, 0, [0; 2]).unwrap_err().code(), 0x12DB);
        // 篡改检测：改一字节后解码必拒（0x12D7）。
        let mut frame = encode_call_params(1, 1, 19, 3, [0; 2]).unwrap();
        frame[9] ^= 0x01;
        assert_eq!(decode_call_params(&frame).unwrap_err().code(), 0x12D7);
        // 截断检测：长度不符必拒。
        assert_eq!(decode_call_params(&frame[..16]).unwrap_err().code(), 0x12D7);
    }
}

#[cfg(test)]
mod safety_group_tests {
    use super::*;

    fn req_full(effect: &'static str) -> MapRequest<'static> {
        MapRequest {
            effect_name: effect,
            handle_scope: 1,
            caps: CAP_ALL,
            secure_surface: false,
            os_build: OS_HEALTH_SAMPLE,
        }
    }

    #[test]
    fn kernel_direct_call_redline() {
        let mut e = EffMapEngine::new().unwrap();
        let req = req_full("blur.gaussian");
        // A08 win32k 内核直调：命中红线（0x12D4）+ 留痕 + 降级建议。
        assert_eq!(e.check_direct_call(API_NT_SWCA, EffectKind::BlurGaussian, &req).unwrap_err().code(), 0x12D4);
        let traces = e.guard().traces();
        assert_eq!(traces.len(), 1);
        assert_eq!(traces[0].rule, RL_KERNEL_DIRECT);
        assert!(traces[0].degrade_hint.contains("A01"), "降级建议必须指路 A01");
        // 同请求计划：A08 不在链内，主路径 A01 直发不受影响。
        let plan = e.map_effect(&req).unwrap();
        assert_eq!(plan.steps[0].api_index, API_SWCA);
        assert_eq!(e.guard().rule_count(RL_KERNEL_DIRECT), 1);
    }

    #[test]
    fn privileged_handle_redline() {
        let mut e = EffMapEngine::new().unwrap();
        // 跨进程句柄 → 越权红线。
        let req_x = MapRequest {
            handle_scope: 3,
            ..req_full("dark.surface")
        };
        assert_eq!(e.check_direct_call(API_DWMAPI_ATTR, EffectKind::DarkSurface, &req_x).unwrap_err().code(), 0x12D4);
        assert_eq!(e.guard().traces()[0].rule, RL_PRIVILEGED_HANDLE);
        // 计划层：跨进程 → 链上原生阶段全拒 → 强制软件兜底（A00）。
        let plan = e.map_effect(&req_x).unwrap();
        assert_eq!(plan.steps[plan.head as usize].api_index, API_SOFTWARE);
        assert!(plan.degrade_stage > 0);
    }

    #[test]
    fn undeclared_capability_degrades_chain() {
        let mut e = EffMapEngine::new().unwrap();
        // 缺 CAP_SWCA：模糊链 A01 被拒 → A03 接管（链内降级）。
        let req = MapRequest {
            caps: CAP_DWM_API | CAP_SOFTWARE,
            ..req_full("blur.gaussian")
        };
        let plan = e.map_effect(&req).unwrap();
        assert_eq!(plan.head, 1);
        assert_eq!(plan.steps[1].api_index, API_DWM_BLURBEHIND);
        assert_eq!(plan.steps[1].attr_id, 0x1); // DWM_BB_ENABLE
        assert_eq!(plan.degrade_stage, 1);
        assert_eq!(plan.redline_hits, 1);
        // 事件账：1 命中 + 1 红线 + 1 回退。
        let (hits, redlines, fallbacks) = e.ledger().counters();
        assert_eq!((hits, redlines, fallbacks), (1, 1, 1));
        assert!(e.ledger().selfcheck());
    }

    #[test]
    fn secure_surface_forces_software() {
        let mut e = EffMapEngine::new().unwrap();
        // 安全面：原生 API 全拒，软件合成（A00）不受安全面规则限制。
        let req = MapRequest {
            secure_surface: true,
            ..req_full("mica.material")
        };
        let plan = e.map_effect(&req).unwrap();
        assert_eq!(plan.steps[plan.head as usize].api_index, API_SOFTWARE);
        assert_eq!(plan.redline_hits, 2); // 链上 A02、A01 两个原生阶段被拒
        // 全能力缺失（含软件兜底）→ 计划失败 0x12D5。
        let e2 = EffMapEngine::new().unwrap();
        let mut e2 = e2;
        let req_no_caps = MapRequest {
            caps: 0,
            ..req_full("blur.gaussian")
        };
        assert_eq!(e2.map_effect(&req_no_caps).unwrap_err().code(), 0x12D5);
    }

    #[test]
    fn os_version_gate_skips_stage() {
        let mut e = EffMapEngine::new().unwrap();
        // 旧系统（Vista）：云母主路径 A02(38) 是 22H2 特性 → 版本跳过 →
        // 回退亚克力 SWCA（1703 起也够不上）→ 版本跳过 → 软件兜底。
        let req = MapRequest {
            os_build: OS_VISTA,
            caps: CAP_DWM_API | CAP_SWCA | CAP_SOFTWARE,
            ..req_full("mica.material")
        };
        let plan = e.map_effect(&req).unwrap();
        assert_eq!(plan.steps[plan.head as usize].api_index, API_SOFTWARE);
        assert_eq!(plan.steps[0].status, STEP_OS_SKIP);
        assert_eq!(plan.steps[1].status, STEP_OS_SKIP);
        // 请求版本为 0 → 非法请求 0x12DD；句柄域越闭集同拒。
        let e2 = EffMapEngine::new().unwrap();
        let mut e2 = e2;
        assert_eq!(e2.map_effect(&MapRequest { os_build: 0, ..req_full("blur.gaussian") }).unwrap_err().code(), 0x12DD);
        assert_eq!(e2.map_effect(&MapRequest { handle_scope: 9, ..req_full("blur.gaussian") }).unwrap_err().code(), 0x12DD);
    }

    #[test]
    fn idempotency_dual_run_compare() {
        let mut e1 = EffMapEngine::new().unwrap();
        let mut e2 = EffMapEngine::new().unwrap();
        let req = req_full("blur.gaussian");
        // 引擎 2 先跑两次无关映射（卫兵带历史），计划裁决仍须一致。
        let _ = e2.map_effect(&req_full("drop.shadow")).unwrap();
        let _ = e2.map_effect(&req_full("drop.shadow")).unwrap();
        let r1 = e1.run_idempotency_check(&req).unwrap();
        let r2 = e2.run_idempotency_check(&req).unwrap();
        assert!(r1.consistent);
        assert!(r2.consistent);
        assert_eq!(r1.hash_first, r1.hash_second);
        // 跨引擎同请求同计划（幂等不受历史计数影响）。
        assert_eq!(r1.hash_first, r2.hash_first);
        assert_eq!(e1.idempotency_checks(), 1);
    }

    #[test]
    fn ledger_ring_and_selfcheck() {
        let mut led = MapEventLedger::new();
        let mut i = 0;
        while i < (LEDGER_CAP + 10) {
            let class = match i % 3 {
                0 => EVENT_LOOKUP_HIT,
                1 => EVENT_REDLINE_REJECT,
                _ => EVENT_FALLBACK,
            };
            led.record(class, 1, API_SWCA, 0);
            i += 1;
        }
        assert_eq!(led.len(), LEDGER_CAP);
        let s = led.summary();
        assert_eq!(s.recorded_total, (LEDGER_CAP + 10) as u64);
        assert_eq!(s.dropped, 10);
        assert!(s.consistent);
        // 类别闭集外静默拒收。
        led.record(9, 1, 1, 0);
        assert_eq!(led.summary().recorded_total, s.recorded_total);
    }

    #[test]
    fn health_green_and_report() {
        let mut e = EffMapEngine::new().unwrap();
        // 预热：几笔正常映射 + 一笔红线直调问询。
        let _ = e.map_effect(&req_full("blur.gaussian")).unwrap();
        let req_missing = MapRequest {
            caps: CAP_DWM_API | CAP_SOFTWARE,
            ..req_full("frost.overlay")
        };
        let _ = e.map_effect(&req_missing).unwrap();
        let _ = e.check_direct_call(API_NT_SWCA, EffectKind::BlurGaussian, &req_full("blur.gaussian"));
        let h = e.health();
        assert_eq!(h.status, HealthStatus::Green);
        assert!(h.fingerprint_ok);
        assert!(h.idempotency_ok);
        assert!(h.ledger.consistent);
        assert!(h.audit_rows <= EFFMAP_AUDIT_CAP);
        assert!(e.assert_healthy().is_ok());
        let text = e.selfcheck_report();
        assert!(text.contains("EFFMAP"));
        assert!(text.contains("绿"));
    }

    #[test]
    fn error_codes_and_details_complete() {
        let all: [EffMapError; 16] = [
            EffMapError::UnknownEffect,
            EffMapError::TableSchemaMismatch,
            EffMapError::EntryRewriteDenied,
            EffMapError::TableFull,
            EffMapError::RedLineViolation,
            EffMapError::PlanFailed,
            EffMapError::ParamEncodeOverflow,
            EffMapError::ParamDecodeMismatch,
            EffMapError::LedgerFull,
            EffMapError::AuditLedgerFull,
            EffMapError::FingerprintMismatch,
            EffMapError::ApiNotFound,
            EffMapError::PlanSizeOverLimit,
            EffMapError::InvalidRequest,
            EffMapError::IdempotencyDrift,
            EffMapError::HealthDegraded,
        ];
        let mut i = 0;
        while i < all.len() {
            let e = &all[i];
            assert_eq!(e.code(), (0x12D0 + i) as u16, "变体 {} 码错位", i);
            let d = e.detail();
            assert!(!d.is_empty());
            assert!(d.chars().count() > 10, "变体 {} 诊断过短", i);
            let r = e.render();
            assert!(r.starts_with("[EFFMAP-0x12D"));
            i += 1;
        }
    }

    #[test]
    fn api_table_and_redline_rules_const_shape() {
        assert_eq!(API_TABLE.len(), API_TABLE_LEN);
        // A08 是唯一红线 API；A01 是唯一能力受控 API。
        let mut redline_count = 0;
        let mut gated_count = 0;
        let mut i = 0;
        while i < API_TABLE_LEN {
            match API_TABLE[i].redline_class {
                RLC_REDLINE => redline_count += 1,
                RLC_CAP_GATED => gated_count += 1,
                _ => {}
            }
            // 每条 API 的来源注记与模块名非空。
            assert!(!API_TABLE[i].note.is_empty());
            assert!(!API_TABLE[i].module.is_empty());
            i += 1;
        }
        assert_eq!(redline_count, 1);
        assert_eq!(gated_count, 1);
        assert_eq!(REDLINE_RULES.len(), REDLINE_RULE_COUNT);
        let mut j = 0;
        while j < REDLINE_RULE_COUNT {
            assert!(!REDLINE_RULES[j].0.is_empty());
            assert!(REDLINE_RULES[j].1.contains("降级建议"));
            j += 1;
        }
    }
}

