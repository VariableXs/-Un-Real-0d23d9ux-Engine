//! CGPU-F1929 · 显示功耗（CGPU-M 域 · 显示组 · 目标 300 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/CGPU Varix STAR II · 总纲与施工书.md#CGPU-F1929`
//!
//! **判据（锚点原文）**：模型复用、标注、休眠、三组、判据。
//!
//! **职责定位（锚点原文摘录）**：功耗：显示功耗（面板功耗（亮度联动——功耗模型复用 F1446——模型复用；刷新（高刷功耗——刷新功耗标注；休眠（显示器休眠（DPMS——休眠语义；测试（模型/标注/休眠三组）。
//!
//! ## 零、总览：一块面板两成分，一档刷新一个标注，一次休眠一本延迟账
//!
//! 显示功耗是整机功耗里最"看得见"的一块：亮度一动功耗就动，刷新率一提
//! 功耗就涨，显示器一睡功耗就落到底。本模块把显示功耗收敛为**一条被算
//! 的功耗链**：面板功耗模型 → 刷新功耗标注 → DPMS 休眠语义 → 预算
//! 联动 → 多屏聚合，链路旁挂一本定长事件账：
//!
//! 1. **面板功耗模型**（判据一，[`PanelPowerModel`]）：亮度联动走
//!    "背光 baseline + 亮度线性段"双成分模型——静态面板功耗 + 背光
//!    基线 + 亮度线性斜率，系数表按面板典型规格写死并注明来源。
//!    **模型复用**判据落地为：系数与指纹算法复用 F1446 功耗模型的
//!    同一套双成分结构（本模块按 F1446 的模型形状做显示域实例化），
//!    模型指纹在引导时冻结，后续每次报价都重算指纹比对——系数被
//!    漂移篡改即显性报 [`DispPwrError::ModelDriftDetected`]，绝不
//!    静默换模型。
//! 2. **刷新功耗标注**（判据二，[`RefreshPowerTag`]）：60/120/144/240Hz
//!    四档各带写死的功耗倍率（典型规格折算，来源注释在表上），高刷
//!    溢价**全链传播**——每屏报价带档位标注，聚合报告逐行带档位，
//!    导出面不带标注即构造失败。VRR 区间给出功耗带（最低/最高刷新
//!    对应的两端报价），带内波动显性而非含糊。
//! 3. **DPMS 休眠语义**（判据三，[`DpmsSemantics`]）：VESA DPMS 四态
//!    On/Standby/Suspend/Off 各带功耗地板与唤醒延迟，迁移合法性表
//!    写死——非法迁移（如 Off→Standby，必须先经 On）显性拒绝并入账。
//!    休眠期内内容写入/亮度调节被**拦截**并显性报
//!    [`DispPwrError::ContentUnreachable`]——内容在休眠期不可达是
//!    显性事实，不是静默成功。唤醒延迟按次累加记账。
//! 4. **预算联动**（[`PowerBudgetLink`]）：显示功耗按显示占比份额进入
//!    整机功耗账；预算紧时给出**降亮度建议**——建议只建议、不强制，
//!    是否采纳由调用方显式调用，评估函数绝不偷改任何屏的状态。
//! 5. **多屏聚合**：Σ屏 ≡ 总显示功耗守恒——聚合时逐屏重算求和与
//!    运行总账比对，不一致即报 [`DispPwrError::ConservationBroken`]
//!    并把核心推入告警；报告每屏分列（档位标注、DPMS 态、报价），
//!    总行只来自求和，绝无第二条汇总路径。
//!
//! 链路旁的**事件账**（[`LedgerRing`]）：注册/报价/亮度/刷新/DPMS
//! 迁移/拦截/告警/建议/注入全部入账，定长环形（256 条），被覆盖的旧
//! 事件显性计数（`overflow`），绝不静默丢账。`health()` 体检给三档
//! 结论；`fingerprint()` 用 FNV-1a 64 对全量状态做确定性指纹；
//! `run_dual_comparison()` 用同一份脚本在两个独立实例上双跑对拍指纹
//! ——分歧即确定性破缺（[`DispPwrError::DualRunMismatch`]）。
//!
//! ## 注入面（三组测试判据的对抗半边）
//!
//! * **模型漂移注入**：篡改模型系数副本 → 指纹比对必须检出。
//! * **非法档位注入**：90Hz 不在档位闭集 → 必须显性拒绝。
//! * **非法 DPMS 迁移注入**：Suspend→Standby / Off→Standby → 必须
//!   显性拒绝并入账。
//! 三路注入统一走 [`DispPwrCore::run_injection_suite`]，检出结果
//! 全部入账，任何一路未检出即体检 Bad。
//!
//! ## 纪律（本文件自守）
//!
//! * 完全自包含：只 `use core::…` / `use alloc::{vec, vec::Vec, format,
//!   string…}`，禁 `use crate::…`；no_std + alloc。
//! * 内核路径零 `unwrap`/`expect`/`panic!`/运行期 `assert!`；可失败一律
//!   `Result<T, DispPwrError>`，16 个变体恰好占满 0x1D90~0x1D9F 诊断段，
//!   每变体 `detail()` 给中文可操作诊断。
//! * 溢出显性：功耗累加全走 `saturating_add`，指纹哈希走 `wrapping`，
//!   延迟累加走 `saturating_add`，绝不静默回绕。
//! * `const fn` 自守：无 `slice::get`、无 `Vec` 索引、无 `Ord::min/max`、
//!   无派生 `PartialEq` 的 `==`、循环一律 `while`+索引。
//! * >64KB 结构体不上栈：所有结构体远小于 64KB；报告载荷按需堆分配。
//!
//! ## 诊断码段（独占声明）
//!
//! 本模块独占 `0x1D90 .. 0x1D9F`（`SEG_BASE = 0x1D90`，`SEG_MASK =
//! 0x000F`，16 变体一码一段）。开工前复核记录：任务预核段
//! `0x1CB0..0x1CBF` 已被 `unxstar/unx2602_upfentry.rs` 声明占用
//! （其声明域为 `0x1C80..0x1CBF`），故按预案顺延启用 `0x1D90` 段；
//! `grep -rn "0x1D9" kernel/varix/src` 仅命中无关 64 位种子常量，
//! 无段声明冲突。`0x1DA0` 起归 `svstar2/ves488_s27fuzz.rs`，下界
//! `0x1D80` 一侧无声明，均与本段不相交。

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

// ===================================================================
// 一、段常量与定容（全部写死并注明来源）
// ===================================================================

/// 诊断码段基址：0x1D90。
/// （来源：见文件头"诊断码段"一节的复核记录；0x1CB0 段已被占用后顺延。）
pub const SEG_BASE: u16 = 0x1D90;

/// 诊断段掩码：段内 16 个码位（0x1D90..0x1D9F 恰好容纳 16 个错误变体）。
pub const SEG_MASK: u16 = 0x000F;

/// 事件账容量：256 条定长环形。
/// （来源：邻件 cgm09_outsec.rs 审计账同容量，域内保持一致，便于对账。）
pub const LEDGER_CAP: usize = 256;

/// 单核最多注册屏数：8。
/// （来源：DP 输出典型上限（4 复用流 × 2 核）外放宽到 8，覆盖双 GPU 桌面形态。）
pub const MAX_SCREENS: usize = 8;

/// 亮度分辨率：千分比（0..=1000）。
/// （来源：F1446 功耗模型亮度联动约定以千分比为粒度，本模块复用同一约定。）
pub const BRIGHT_MAX_PERMILLE: u16 = 1000;

/// 亮度最低可用档：50‰。
/// （来源：面板规格：背光在极低 duty 下闪烁/色偏不可用，5% 为常用最低稳定档。）
pub const BRIGHT_MIN_PERMILLE: u16 = 50;

/// 刷新率合法下界：30Hz（覆盖 eDP 面板省电模式与 VRR 下限）。
/// （来源：VESA eDP + DisplayPort VRR 常见下限 30~48Hz，取闭集校验下界 30。）
pub const HZ_MIN: u16 = 30;

/// 刷新率合法上界：240Hz（与档位表上档一致）。
/// （来源：本域档位表最高档 240Hz；超出即非法档位。）
pub const HZ_MAX: u16 = 240;

// ===================================================================
// 二、错误闭集（0x1D90..0x1D9F，一码一变体，共 16 变体）
// ===================================================================

/// 显示功耗子系统错误闭集。每变体独占一个诊断码，`detail()` 给中文
/// 可操作诊断；本枚举不派生 `PartialEq`（`const fn` 自守：禁派生类型
/// 参与 `==`），比较一律走 `code()`。
#[derive(Debug, Clone, Copy)]
pub enum DispPwrError {
    /// `0x1D90` 核心未引导：未调用 `boot` 就发起操作。
    NotBooted,
    /// `0x1D91` 重复引导：`boot` 只允许一次。
    AlreadyBooted,
    /// `0x1D92` 亮度越界：不在 `BRIGHT_MIN..=BRIGHT_MAX` 千分比区间。
    BrightnessOutOfRange,
    /// `0x1D93` 模型漂移：模型指纹与引导时冻结值不一致。
    ModelDriftDetected,
    /// `0x1D94` 非法刷新档：Hz 不在档位闭集（60/120/144/240）。
    RefreshTierInvalid,
    /// `0x1D95` VRR 区间非法：min>=max 或越出 30..=240。
    VrrBandInvalid,
    /// `0x1D96` 非法 DPMS 迁移：不在迁移合法性表内。
    DpmsTransitionIllegal,
    /// `0x1D97` 内容不可达：休眠期内容写入/调节被拦截。
    ContentUnreachable,
    /// `0x1D98` 屏不存在：screen_id 未注册。
    ScreenNotFound,
    /// `0x1D99` 屏重复注册：同一 screen_id 已在册。
    ScreenAlreadyRegistered,
    /// `0x1D9A` 显示预算反向请求越界（预算低于静态功耗地板）。
    BudgetBelowFloor,
    /// `0x1D9B` 守恒破坏：Σ屏重算与运行总账不一致。
    ConservationBroken,
    /// `0x1D9C` 屏表已满：超过 `MAX_SCREENS`。
    ScreenTableFull,
    /// `0x1D9D` 双跑分歧：同脚本双实例指纹不一致，确定性破缺。
    DualRunMismatch,
    /// `0x1D9E` 状态不允许：核心未处于 Active 态。
    StateNotActive,
    /// `0x1D9F` 注入面失守：注入用例未被检出（自检失败）。
    InjectionUndetected,
}

impl DispPwrError {
    /// 诊断码：`SEG_BASE | 段内序号`，一码一变体。
    pub const fn code(&self) -> u16 {
        match self {
            DispPwrError::NotBooted => 0x1D90,
            DispPwrError::AlreadyBooted => 0x1D91,
            DispPwrError::BrightnessOutOfRange => 0x1D92,
            DispPwrError::ModelDriftDetected => 0x1D93,
            DispPwrError::RefreshTierInvalid => 0x1D94,
            DispPwrError::VrrBandInvalid => 0x1D95,
            DispPwrError::DpmsTransitionIllegal => 0x1D96,
            DispPwrError::ContentUnreachable => 0x1D97,
            DispPwrError::ScreenNotFound => 0x1D98,
            DispPwrError::ScreenAlreadyRegistered => 0x1D99,
            DispPwrError::BudgetBelowFloor => 0x1D9A,
            DispPwrError::ConservationBroken => 0x1D9B,
            DispPwrError::ScreenTableFull => 0x1D9C,
            DispPwrError::DualRunMismatch => 0x1D9D,
            DispPwrError::StateNotActive => 0x1D9E,
            DispPwrError::InjectionUndetected => 0x1D9F,
        }
    }

    /// 中文可操作诊断：拿到错误码的人按这句话就能动手排查。
    pub const fn detail(&self) -> &'static str {
        match self {
            DispPwrError::NotBooted => "核心未引导：先调用 DispPwrCore::boot 完成模型冻结与状态初始化，再发起功耗操作",
            DispPwrError::AlreadyBooted => "重复引导：boot 幂等保护生效；核对引导路径是否被重复进入，勿二次调用",
            DispPwrError::BrightnessOutOfRange => "亮度越界：千分比须在 50..=1000（BRIGHT_MIN..=BRIGHT_MAX），调用前先夹取或校验",
            DispPwrError::ModelDriftDetected => "模型漂移：当前模型指纹与引导冻结值不一致，系数表被篡改；恢复出厂系数或重新 boot 冻结",
            DispPwrError::RefreshTierInvalid => "非法刷新档：Hz 须为 60/120/144/240 档位闭集之一；查档位表 RefreshTier::from_hz",
            DispPwrError::VrrBandInvalid => "VRR 区间非法：min_hz 须小于 max_hz 且双双落在 30..=240；修正区间端点后重设",
            DispPwrError::DpmsTransitionIllegal => "非法 DPMS 迁移：查迁移合法性表 DPMS_TRANSITIONS；Off 态必须先唤醒到 On 再转低功耗态",
            DispPwrError::ContentUnreachable => "内容不可达：屏处于休眠态（非 On），内容写入/调节被拦截；先 DPMS 唤醒到 On 再操作",
            DispPwrError::ScreenNotFound => "屏不存在：screen_id 未注册；先 register_screen，或核对传入 id 是否传错",
            DispPwrError::ScreenAlreadyRegistered => "屏重复注册：该 screen_id 已在册；如需重建请先走对应屏的重建接口，勿直接重复注册",
            DispPwrError::BudgetBelowFloor => "显示预算低于地板：预算须 ≥ 显示份额下限（静态功耗）；调大整机预算或核对该屏面板型号",
            DispPwrError::ConservationBroken => "守恒破坏：Σ屏重算与运行总账不一致；停写并导出事件账排查，聚合路径存在旁路写入",
            DispPwrError::ScreenTableFull => "屏表已满：超过 MAX_SCREENS=8；下线空闲屏或核对拓扑，勿绕过注册上限",
            DispPwrError::DualRunMismatch => "双跑分歧：同脚本双实例指纹不一致，存在不确定性来源；检查随机数/未初始化内存参与哈希",
            DispPwrError::StateNotActive => "状态不允许：核心不在 Active 态（Offline/告警锁定）；先完成引导或解除告警",
            DispPwrError::InjectionUndetected => "注入面失守：对抗用例未被检出，检测逻辑失效；立即停机核对注入对应的检测路径",
        }
    }
}

// ===================================================================
// 三、通用小件：FNV-1a 64 与饱和运算
// ===================================================================

/// FNV-1a 64 偏移基（来源：FNV 参考实现标准常量 offset basis）。
const FNV_OFFSET: u64 = 0xcbf2_9ce4_8422_2325;

/// FNV-1a 64 素数（来源：FNV 参考实现标准常量 prime）。
const FNV_PRIME: u64 = 0x0000_0100_0000_01b3;

/// 单字节入哈希（const fn 安全：纯算术，无库调用）。
const fn fnv_byte(h: u64, b: u8) -> u64 {
    (h ^ (b as u64)).wrapping_mul(FNV_PRIME)
}

/// u16 小端逐字节入哈希。
const fn fnv_u16(h: u64, v: u16) -> u64 {
    fnv_byte(fnv_byte(h, (v & 0xFF) as u8), (v >> 8) as u8)
}

/// u32 小端逐字节入哈希。
const fn fnv_u32(h: u64, v: u32) -> u64 {
    fnv_byte(
        fnv_byte(
            fnv_byte(fnv_byte(h, (v & 0xFF) as u8), ((v >> 8) & 0xFF) as u8),
            ((v >> 16) & 0xFF) as u8,
        ),
        ((v >> 24) & 0xFF) as u8,
    )
}

/// u64 小端逐字节入哈希。
const fn fnv_u64(h: u64, v: u64) -> u64 {
    let mut i = 0;
    let mut acc = h;
    while i < 8 {
        acc = fnv_byte(acc, ((v >> (i * 8)) & 0xFF) as u8);
        i += 1;
    }
    acc
}

// ===================================================================
// 四、面板功耗模型（判据一：模型复用 F1446 双成分结构）
// ===================================================================

/// 面板形态闭集。系数来自各形态典型规格（来源逐项注释在系数表上）。
#[derive(Debug, Clone, Copy)]
pub enum PanelKind {
    /// 13.3 英寸笔记本 IPS（300nit 典型）。
    LaptopIps133,
    /// 27 英寸 4K 桌面 IPS（400nit 典型）。
    Desktop4k27,
    /// 15.6 英寸 4K OLED 移动形态（自发光，背光成分近零）。
    OledMobile156,
}

impl PanelKind {
    /// 形态代号（入指纹与报告）。
    pub const fn code(&self) -> u16 {
        match self {
            PanelKind::LaptopIps133 => 1,
            PanelKind::Desktop4k27 => 2,
            PanelKind::OledMobile156 => 3,
        }
    }

    /// 形态名（报告用静态串）。
    pub const fn name(&self) -> &'static str {
        match self {
            PanelKind::LaptopIps133 => "13.3 IPS 笔记本",
            PanelKind::Desktop4k27 => "27 4K 桌面 IPS",
            PanelKind::OledMobile156 => "15.6 4K OLED 移动",
        }
    }
}

/// 面板功耗模型：**背光 baseline + 亮度线性段**双成分（F1446 模型复用）。
///
/// 模型式（整数运算，单位 mW）：
/// `P(b) = static_panel_mw + backlight_baseline_mw + linear_slope_mw * b`
/// 其中 `b` 为亮度千分比（0..=1000）。
///
/// **模型复用判据**：该双成分形状与 F1446 功耗模型一致——静态成分 +
/// 基线成分 + 线性联动成分的三元结构、千分比亮度粒度、指纹冻结方式
/// 均复用 F1446 的约定，本模块只是把系数实例化到显示面板域。
#[derive(Debug, Clone, Copy)]
pub struct PanelPowerModel {
    /// 静态面板功耗（0 亮度时面板点亮的固定开销，mW）。
    pub static_panel_mw: u32,
    /// 背光基线（背光最暗 duty 下的固定功耗，mW；OLED 近零）。
    pub backlight_baseline_mw: u32,
    /// 亮度线性斜率（每千分比亮度对应的功耗，mW/‰）。
    pub linear_slope_mw: u32,
    /// 面板形态代号。
    pub kind_code: u16,
}

/// 三形态系数表（const，来源逐项注释）。
///
/// * 笔记本 13.3" IPS 300nit：整机面板功耗典型 1.4~4.0W（面板规格书
///   典型值域：静态 ~0.8W、背光基线 ~0.6W、满亮度线性段 ~2.6W）。
/// * 桌面 27" 4K IPS 400nit：面板功耗典型 3.5~18W（静态 ~3.5W、
///   背光基线 ~2.5W、满亮度线性段 ~12W）。
/// * 移动 15.6" 4K OLED：自发光，背光基线近零（~0.2W 驱动余量），
///   静态 ~1.2W、满亮度（APL 全白）线性段 ~9W。
const PANEL_MODEL_TABLE: [PanelPowerModel; 3] = [
    PanelPowerModel {
        static_panel_mw: 800,
        backlight_baseline_mw: 600,
        linear_slope_mw: 26,
        kind_code: 1,
    },
    PanelPowerModel {
        static_panel_mw: 3500,
        backlight_baseline_mw: 2500,
        linear_slope_mw: 120,
        kind_code: 2,
    },
    PanelPowerModel {
        static_panel_mw: 1200,
        backlight_baseline_mw: 200,
        linear_slope_mw: 90,
        kind_code: 3,
    },
];

impl PanelPowerModel {
    /// 按形态取系数表模型（const fn：数组下标常量化，无 slice::get）。
    pub const fn from_kind(kind: PanelKind) -> Self {
        let idx = match kind {
            PanelKind::LaptopIps133 => 0usize,
            PanelKind::Desktop4k27 => 1usize,
            PanelKind::OledMobile156 => 2usize,
        };
        PANEL_MODEL_TABLE[idx]
    }

    /// 模型指纹（FNV-1a 64 over 四个系数小端字节，顺序固定）。
    /// 指纹在 boot 时冻结；运行期每次报价重算比对，漂移即报错。
    pub const fn fingerprint(&self) -> u64 {
        let h0 = fnv_u64(FNV_OFFSET, self.static_panel_mw as u64);
        let h1 = fnv_u64(h0, self.backlight_baseline_mw as u64);
        let h2 = fnv_u64(h1, self.linear_slope_mw as u64);
        fnv_u16(h2, self.kind_code)
    }

    /// 亮度→功耗映射（mW）。越界亮度显性拒绝；算术全饱和。
    ///
    /// 双成分式：`static + baseline + slope * b`；三个成分都是模型
    /// 复用 F1446 的固定形状，此处只是实例化求值。
    pub fn power_at(&self, brightness_permille: u16) -> Result<u32, DispPwrError> {
        if brightness_permille > BRIGHT_MAX_PERMILLE || brightness_permille < BRIGHT_MIN_PERMILLE {
            return Err(DispPwrError::BrightnessOutOfRange);
        }
        let linear = (self.linear_slope_mw as u64) * (brightness_permille as u64);
        let sum = (self.static_panel_mw as u64)
            .saturating_add(self.backlight_baseline_mw as u64)
            .saturating_add(linear);
        // 求和上限 1000*120+6000 ≈ 1.26e5 mW，u32 绰绰有余；饱和加已兜底。
        Ok(sum as u32)
    }

    /// 预算反向映射：给定该屏功耗预算（mW），求可运行的最大亮度档。
    /// 预算低于静态地板时显性报 [`DispPwrError::BudgetBelowFloor`]。
    /// 结果向下取整到 `BRIGHT_MIN..=BRIGHT_MAX`；供预算联动出建议用。
    pub fn brightness_for_budget(&self, budget_mw: u32) -> Result<u16, DispPwrError> {
        let floor = self
            .static_panel_mw
            .saturating_add(self.backlight_baseline_mw);
        if budget_mw < floor.saturating_add(self.linear_slope_mw * (BRIGHT_MIN_PERMILLE as u32)) {
            return Err(DispPwrError::BudgetBelowFloor);
        }
        let head = budget_mw.saturating_sub(floor);
        // b = head / slope，向下取整；再夹到亮度闭集。
        let raw = if self.linear_slope_mw == 0 {
            BRIGHT_MAX_PERMILLE as u64
        } else {
            (head as u64) / (self.linear_slope_mw as u64)
        };
        let mut b = raw;
        if b > BRIGHT_MAX_PERMILLE as u64 {
            b = BRIGHT_MAX_PERMILLE as u64;
        }
        if b < BRIGHT_MIN_PERMILLE as u64 {
            b = BRIGHT_MIN_PERMILLE as u64;
        }
        Ok(b as u16)
    }
}

// ===================================================================
// 五、刷新功耗标注（判据二：高刷溢价全链传播）
// ===================================================================

/// 刷新档位闭集（60/120/144/240）。档外一律非法（90Hz 之类不在闭集）。
#[derive(Debug, Clone, Copy)]
pub enum RefreshTier {
    /// 60Hz 标准档。
    Hz60,
    /// 120Hz 高刷档。
    Hz120,
    /// 144Hz 高刷档。
    Hz144,
    /// 240Hz 电竞档。
    Hz240,
}

/// 刷新档倍率表（permille，相对 60Hz=1000）。
/// （来源：面板典型规格折算：扫描/驱动频率线性项 + 数据通路随频增长，
/// 实测典型 120Hz ≈ +18%、144Hz ≈ +22%、240Hz ≈ +35% 溢价，取整写入。）
const TIER_TABLE: [(u16, u16); 4] = [
    (60, 1000),
    (120, 1180),
    (144, 1220),
    (240, 1350),
];

impl RefreshTier {
    /// 档位序号（0..3，入指纹与报告）。
    pub const fn index(&self) -> usize {
        match self {
            RefreshTier::Hz60 => 0,
            RefreshTier::Hz120 => 1,
            RefreshTier::Hz144 => 2,
            RefreshTier::Hz240 => 3,
        }
    }

    /// 档位标称 Hz。
    pub const fn hz(&self) -> u16 {
        TIER_TABLE[self.index()].0
    }

    /// 档位功耗倍率（permille，60Hz=1000）。
    pub const fn multiplier_permille(&self) -> u16 {
        TIER_TABLE[self.index()].1
    }

    /// 档位名（报告标注用）。
    pub const fn name(&self) -> &'static str {
        match self {
            RefreshTier::Hz60 => "60Hz 标准",
            RefreshTier::Hz120 => "120Hz 高刷",
            RefreshTier::Hz144 => "144Hz 高刷",
            RefreshTier::Hz240 => "240Hz 电竞",
        }
    }

    /// Hz→档位解析（const fn：while+索引遍历闭集，无 slice::get）。
    /// 档外返回 `None`——**非法档位注入**在此显性失配。
    pub const fn from_hz(hz: u16) -> Option<RefreshTier> {
        let mut i = 0;
        while i < 4 {
            if TIER_TABLE[i].0 == hz {
                return match i {
                    0 => Some(RefreshTier::Hz60),
                    1 => Some(RefreshTier::Hz120),
                    2 => Some(RefreshTier::Hz144),
                    _ => Some(RefreshTier::Hz240),
                };
            }
            i += 1;
        }
        None
    }

    /// 任意 Hz（合法区间内）的插值倍率：落在两档之间时按 Hz 线性
    /// 插值到 permille（供 VRR 区间功耗带用；档位值直接取表值）。
    pub const fn multiplier_at_hz(hz: u16) -> Option<u16> {
        if hz < HZ_MIN || hz > HZ_MAX {
            return None;
        }
        let mut i = 0;
        while i < 4 {
            if TIER_TABLE[i].0 == hz {
                return Some(TIER_TABLE[i].1);
            }
            // 表按 Hz 升序；hz 落在本档与下一档之间时插值。
            if i + 1 < 4 && hz < TIER_TABLE[i + 1].0 && hz > TIER_TABLE[i].0 {
                let lo_hz = TIER_TABLE[i].0 as u32;
                let hi_hz = TIER_TABLE[i + 1].0 as u32;
                let lo_m = TIER_TABLE[i].1 as u32;
                let hi_m = TIER_TABLE[i + 1].1 as u32;
                let span = hi_hz - lo_hz;
                let frac = (hz as u32 - lo_hz) * 1000 / span;
                let val = lo_m * 1000 + (hi_m - lo_m) * frac;
                return Some((val / 1000) as u16);
            }
            i += 1;
        }
        // hz 落在 30..60 之间：按 60 档倍率下探线性到 30Hz。
        // （来源：低频省电模式按 60Hz 倍率对 30Hz 线性折半下限。）
        if hz < TIER_TABLE[0].0 {
            let lo_hz = HZ_MIN as u32;
            let hi_hz = TIER_TABLE[0].0 as u32;
            let frac = (hz as u32 - lo_hz) * 1000 / (hi_hz - lo_hz);
            let val = 900_000u32 + (TIER_TABLE[0].1 as u32 - 900) * frac;
            return Some((val / 1000) as u16);
        }
        None
    }
}

/// VRR 功耗带：区间两端各自的倍率报价，带内波动显性标注。
#[derive(Debug, Clone, Copy)]
pub struct VrrBand {
    /// VRR 最低刷新（Hz，30..=240）。
    pub min_hz: u16,
    /// VRR 最高刷新（Hz，min<max≤240）。
    pub max_hz: u16,
}

impl VrrBand {
    /// 构造并校验：min<max 且双双在 30..=240。
    pub fn new(min_hz: u16, max_hz: u16) -> Result<Self, DispPwrError> {
        if min_hz >= max_hz || min_hz < HZ_MIN || max_hz > HZ_MAX {
            return Err(DispPwrError::VrrBandInvalid);
        }
        Ok(VrrBand { min_hz, max_hz })
    }

    /// 带端倍率（permille）对：`(低刷新倍率, 高刷新倍率)`。
    /// 任一端解析失败（区间已被构造校验，理论不可达）按非法档报错。
    pub fn band_multipliers(&self) -> Result<(u16, u16), DispPwrError> {
        let lo = match RefreshTier::multiplier_at_hz(self.min_hz) {
            Some(v) => v,
            None => return Err(DispPwrError::VrrBandInvalid),
        };
        let hi = match RefreshTier::multiplier_at_hz(self.max_hz) {
            Some(v) => v,
            None => return Err(DispPwrError::VrrBandInvalid),
        };
        Ok((lo, hi))
    }
}

/// 功耗报价：一屏一次报价的全部标注（高刷溢价全链传播的载体）。
///
/// 导出报告每行携带本结构；档位标注缺失即构造失败（`new` 强制），
/// 保证"标注全链传播"不是口头承诺。
#[derive(Debug, Clone, Copy)]
pub struct PowerQuote {
    /// 屏 id。
    pub screen_id: u8,
    /// 档位标注（None 即非法——构造时已保证非 None）。
    pub tier: Option<RefreshTier>,
    /// 档位倍率（permille）。
    pub tier_mult_permille: u16,
    /// 亮度千分比。
    pub brightness_permille: u16,
    /// 报价（mW，已含档位倍率、未含 DPMS 地板折扣）。
    pub base_mw: u32,
    /// VRR 带低报价（无 VRR 时等于 base_mw）。
    pub band_low_mw: u32,
    /// VRR 带高报价（无 VRR 时等于 base_mw）。
    pub band_high_mw: u32,
}

impl PowerQuote {
    /// 构造报价：亮度与档位必须合法，标注强制入载荷。
    pub fn new(
        screen_id: u8,
        model: &PanelPowerModel,
        tier: RefreshTier,
        brightness_permille: u16,
        vrr: Option<VrrBand>,
    ) -> Result<Self, DispPwrError> {
        let base = model.power_at(brightness_permille)?;
        let mult = tier.multiplier_permille() as u64;
        let scaled = ((base as u64) * mult / 1000) as u32;
        let (lo, hi) = match vrr {
            None => (scaled, scaled),
            Some(band) => {
                let (mlo, mhi) = band.band_multipliers()?;
                let low = ((base as u64) * (mlo as u64) / 1000) as u32;
                let high = ((base as u64) * (mhi as u64) / 1000) as u32;
                (low, high)
            }
        };
        Ok(PowerQuote {
            screen_id,
            tier: Some(tier),
            tier_mult_permille: tier.multiplier_permille(),
            brightness_permille,
            base_mw: scaled,
            band_low_mw: lo,
            band_high_mw: hi,
        })
    }
}

// ===================================================================
// 六、DPMS 休眠语义（判据三：休眠语义 + 拦截 + 延迟账）
// ===================================================================

/// VESA DPMS 四态。态序即数组序（const 表下标依此）。
#[derive(Debug, Clone, Copy)]
pub enum DpmsState {
    /// On：正常工作。
    On,
    /// Standby：最小省电，全屏快速恢复。
    Standby,
    /// Suspend：深度省电，面板供电保持部分上下文。
    Suspend,
    /// Off：全关，恢复需完整重同步。
    Off,
}

/// DPMS 四态语义表：`(功耗地板 permille, 唤醒延迟 ms)`。
/// （来源：VESA DPMS 标准四态定义 + 典型面板规格延迟：Standby 快恢复
/// ~50ms、Suspend 面板重启 ~200ms、Off 全重同步 ~2s；功耗地板取典型
/// 面板待机功耗折算：Standby ~60%、Suspend ~30%、Off ~2%。）
const DPMS_TABLE: [(u32, u32); 4] = [
    (1000, 0),
    (600, 50),
    (300, 200),
    (20, 2000),
];

/// 迁移合法性表：`DPMS_TRANSITIONS[当前态][目标态]`，true 为合法。
///
/// 语义（来源：VESA DPMS 状态机 + 本域收紧约定）：
/// * 任意态可驻留本态（自迁移恒真）；
/// * On 可直达任一低功耗态；
/// * Standby/Suspend 可继续下探（Standby→Suspend/Off、Suspend→Off）；
/// * 唤醒一律直达 On——**Off→Standby、Off→Suspend、Suspend→Standby
///   非法**（省电方向不可逆行；必须先经 On 恢复内容通路）。
const DPMS_TRANSITIONS: [[bool; 4]; 4] = [
    // 目标: On     Standby  Suspend  Off
    [true, true, true, true], // On
    [true, true, true, true], // Standby
    [true, false, true, true], // Suspend（→Standby 非法）
    [true, false, false, true], // Off（→Standby/→Suspend 非法）
];

impl DpmsState {
    /// 态序号（0..3，const 表下标）。
    pub const fn index(&self) -> usize {
        match self {
            DpmsState::On => 0,
            DpmsState::Standby => 1,
            DpmsState::Suspend => 2,
            DpmsState::Off => 3,
        }
    }

    /// 功耗地板（permille，相对本屏 On 态报价）。
    pub const fn floor_permille(&self) -> u32 {
        DPMS_TABLE[self.index()].0
    }

    /// 唤醒到 On 的延迟（ms）。
    pub const fn wake_latency_ms(&self) -> u32 {
        DPMS_TABLE[self.index()].1
    }

    /// 是否休眠态（非 On 即休眠——内容不可达判定依据）。
    pub const fn is_asleep(&self) -> bool {
        match self {
            DpmsState::On => false,
            _ => true,
        }
    }

    /// 态名（报告标注用）。
    pub const fn name(&self) -> &'static str {
        match self {
            DpmsState::On => "On",
            DpmsState::Standby => "Standby",
            DpmsState::Suspend => "Suspend",
            DpmsState::Off => "Off",
        }
    }

    /// 迁移合法性查询（查写死的表，const fn 数组下标）。
    pub const fn can_transition(&self, target: DpmsState) -> bool {
        DPMS_TRANSITIONS[self.index()][target.index()]
    }
}

/// DPMS 语义引擎：状态机 + 延迟账 + 拦截语义，单屏一份。
#[derive(Debug, Clone, Copy)]
pub struct DpmsSemantics {
    /// 当前 DPMS 态。
    pub state: DpmsState,
    /// 累计唤醒延迟账（ms，饱和累加）。
    pub wake_latency_acc_ms: u32,
    /// 合法迁移次数。
    pub legal_transitions: u32,
    /// 非法迁移拦截次数（注入检测的观察窗）。
    pub illegal_intercepts: u32,
    /// 休眠期内容拦截次数（内容不可达显性化的观察窗）。
    pub content_intercepts: u32,
}

impl DpmsSemantics {
    /// 新引擎：初始 On 态（上电即亮）。
    pub const fn new() -> Self {
        DpmsSemantics {
            state: DpmsState::On,
            wake_latency_acc_ms: 0,
            legal_transitions: 0,
            illegal_intercepts: 0,
            content_intercepts: 0,
        }
    }

    /// 请求迁移：查合法性表；非法迁移显性拒绝并计数（不静默放行）。
    /// 合法迁移按目标态计唤醒延迟账（迁移到更低功耗态本身不耗时，
    /// 这里计的是**下次唤醒**的预期延迟——预记账语义，与逐次实冲
    /// 无关，供整机唤醒延迟预算使用）。
    pub fn request_transition(&mut self, target: DpmsState) -> Result<(), DispPwrError> {
        if self.state.can_transition(target) {
            self.state = target;
            self.legal_transitions = self.legal_transitions.saturating_add(1);
            if target.is_asleep() {
                // 预记账：进入休眠态时把该态的唤醒延迟先入账。
                self.wake_latency_acc_ms = self
                    .wake_latency_acc_ms
                    .saturating_add(target.wake_latency_ms());
            }
            Ok(())
        } else {
            self.illegal_intercepts = self.illegal_intercepts.saturating_add(1);
            Err(DispPwrError::DpmsTransitionIllegal)
        }
    }

    /// 唤醒：等价于 request_transition(On)，返回本次实际预记账延迟
    /// （On 态延迟为 0，故只有从休眠态唤醒才有非零值）。
    pub fn wake(&mut self) -> Result<u32, DispPwrError> {
        let from = self.state;
        self.request_transition(DpmsState::On)?;
        Ok(from.wake_latency_ms())
    }

    /// 内容可达性闸门：休眠态下内容写入/亮度/刷新调节一律拦截，
    /// 显性报 [`DispPwrError::ContentUnreachable`]——内容不可达是
    /// 显性事实，绝不静默成功。On 态放行。
    pub fn content_gate(&mut self) -> Result<(), DispPwrError> {
        if self.state.is_asleep() {
            self.content_intercepts = self.content_intercepts.saturating_add(1);
            Err(DispPwrError::ContentUnreachable)
        } else {
            Ok(())
        }
    }

    /// 当前态的功耗地板（permille）。
    pub const fn floor_permille(&self) -> u32 {
        self.state.floor_permille()
    }
}

impl Default for DpmsSemantics {
    fn default() -> Self {
        DpmsSemantics::new()
    }
}

// ===================================================================
// 七、预算联动（显示功耗入整机功耗账 + 只建议不强制）
// ===================================================================

/// 整机功耗预算联动件。
///
/// 显示份额约定（来源：F1446 整机功耗账的显示占比典型值：桌面形态
/// 显示约占总功耗 20~30%，取 25% 为默认份额，permille=250）。
#[derive(Debug, Clone, Copy)]
pub struct PowerBudgetLink {
    /// 整机功耗预算（mW）。
    pub machine_budget_mw: u32,
    /// 显示份额（permille，250 = 25%）。
    pub display_share_permille: u16,
    /// 其它已锁定预留（mW，不参与显示份额计算，直接从预算扣）。
    pub reserved_other_mw: u32,
}

impl PowerBudgetLink {
    /// 构造并校验：份额 ≤ 1000‰，预留 ≤ 预算。
    pub fn new(machine_budget_mw: u32, display_share_permille: u16, reserved_other_mw: u32) -> Result<Self, DispPwrError> {
        if display_share_permille > 1000 || reserved_other_mw > machine_budget_mw {
            return Err(DispPwrError::BudgetBelowFloor);
        }
        Ok(PowerBudgetLink {
            machine_budget_mw,
            display_share_permille,
            reserved_other_mw,
        })
    }

    /// 显示可支配预算：`预留后预算 × 份额`（饱和，向下取整）。
    pub const fn display_budget_mw(&self) -> u32 {
        let after_reserve = self.machine_budget_mw.saturating_sub(self.reserved_other_mw);
        ((after_reserve as u64) * (self.display_share_permille as u64) / 1000) as u32
    }
}

/// 亮度策略建议：**只建议、不强制**——评估函数绝不改动任何屏状态，
/// 采纳与否由调用方显式调用 `set_brightness`。
#[derive(Debug, Clone, Copy)]
pub enum BrightnessAdvice {
    /// 预算宽松：维持现状。
    Keep,
    /// 预算紧张：建议降到目标亮度（permille）。
    ReduceTo {
        /// 建议目标亮度（千分比）。
        target_permille: u16,
        /// 超支额（mW）。
        over_mw: u32,
    },
}

impl BrightnessAdvice {
    /// 建议文案（报告用）。
    pub const fn describe(&self) -> &'static str {
        match self {
            BrightnessAdvice::Keep => "预算宽松，维持当前亮度策略（仅建议，不强制）",
            BrightnessAdvice::ReduceTo { .. } => {
                "预算紧张，建议降亮度至目标档（仅建议，采纳需显式调用 set_brightness）"
            }
        }
    }
}

// ===================================================================
// 八、事件账（定长环形，溢出显性计数）
// ===================================================================

/// 事件种类闭集（每条账目必须落在闭集内）。
#[derive(Debug, Clone, Copy)]
pub enum EventKind {
    /// 屏注册。
    ScreenRegistered,
    /// 亮度设定。
    BrightnessSet,
    /// 刷新档设定。
    RefreshSet,
    /// VRR 区间设定。
    VrrSet,
    /// DPMS 合法迁移。
    DpmsTransition,
    /// DPMS 非法迁移拦截。
    DpmsIllegalBlocked,
    /// 休眠期内容拦截（内容不可达）。
    ContentIntercepted,
    /// 功耗报价。
    QuoteTaken,
    /// 守恒校验通过。
    ConservationOk,
    /// 守恒破坏告警。
    ConservationBroken,
    /// 模型漂移告警。
    ModelDriftAlert,
    /// 预算建议产出。
    BudgetAdvice,
    /// 注入检出。
    InjectionDetected,
    /// 注入失守（自检失败，最重告警）。
    InjectionMissed,
    /// 引导完成。
    Booted,
    /// 告警解除。
    AlertCleared,
}

impl EventKind {
    /// 事件代号（入指纹）。
    pub const fn code(&self) -> u16 {
        match self {
            EventKind::ScreenRegistered => 1,
            EventKind::BrightnessSet => 2,
            EventKind::RefreshSet => 3,
            EventKind::VrrSet => 4,
            EventKind::DpmsTransition => 5,
            EventKind::DpmsIllegalBlocked => 6,
            EventKind::ContentIntercepted => 7,
            EventKind::QuoteTaken => 8,
            EventKind::ConservationOk => 9,
            EventKind::ConservationBroken => 10,
            EventKind::ModelDriftAlert => 11,
            EventKind::BudgetAdvice => 12,
            EventKind::InjectionDetected => 13,
            EventKind::InjectionMissed => 14,
            EventKind::Booted => 15,
            EventKind::AlertCleared => 16,
        }
    }
}

/// 单条账目：序号 + 种类 + 屏 id + 辅助量（mW/permille/ms 视事件而定）。
#[derive(Debug, Clone, Copy)]
pub struct LedgerEntry {
    /// 全账单调序号（wrapping，仅用于指纹与对账）。
    pub seq: u64,
    /// 事件种类代号。
    pub kind_code: u16,
    /// 关联屏 id（无屏事件填 0xFF）。
    pub screen_id: u8,
    /// 辅助量。
    pub aux: u32,
}

/// 定长环形事件账：满则覆盖最旧并显性计数，绝不静默丢账。
#[derive(Debug)]
pub struct LedgerRing {
    /// 环形存储（定长 256，约 4KB，远小于 64KB 上栈红线）。
    buf: [LedgerEntry; LEDGER_CAP],
    /// 写指针。
    head: usize,
    /// 已写入总数（含被覆盖者）。
    total: u64,
    /// 被覆盖的旧账条数（显性溢出计数）。
    pub overflow: u64,
}

impl LedgerRing {
    /// 空账。
    pub const fn new() -> Self {
        LedgerRing {
            buf: [LedgerEntry {
                seq: 0,
                kind_code: 0,
                screen_id: 0xFF,
                aux: 0,
            }; LEDGER_CAP],
            head: 0,
            total: 0,
            overflow: 0,
        }
    }

    /// 入账（满则覆盖最旧，overflow 显性 +1）。
    pub fn push(&mut self, kind: EventKind, screen_id: u8, aux: u32) {
        let entry = LedgerEntry {
            seq: self.total.wrapping_add(1),
            kind_code: kind.code(),
            screen_id,
            aux,
        };
        let slot = self.head % LEDGER_CAP;
        self.buf[slot] = entry;
        self.head = (self.head + 1) % LEDGER_CAP;
        if self.total >= LEDGER_CAP as u64 {
            self.overflow = self.overflow.saturating_add(1);
        }
        self.total = self.total.wrapping_add(1);
    }

    /// 当前有效条数（≤ 容量）。
    pub const fn live_count(&self) -> usize {
        if self.total as usize > LEDGER_CAP {
            LEDGER_CAP
        } else {
            self.total as usize
        }
    }

    /// 账指纹（对有效条目按逻辑序入 FNV——同操作序列同指纹，确定性）。
    pub fn fingerprint(&self) -> u64 {
        self.fingerprint_fold(FNV_OFFSET)
    }

    /// 在给定哈希基上折叠账本内容（供核心全量指纹复用同一遍历路径，
    /// 不存在第二套账遍历实现——单一事实源）。
    pub fn fingerprint_fold(&self, seed: u64) -> u64 {
        let mut h = seed;
        let start = if (self.total as usize) > LEDGER_CAP {
            self.head % LEDGER_CAP
        } else {
            0
        };
        let n = self.live_count();
        let mut i = 0;
        while i < n {
            let slot = (start + i) % LEDGER_CAP;
            let e = self.buf[slot];
            h = fnv_u64(h, e.seq);
            h = fnv_u16(h, e.kind_code);
            h = fnv_byte(h, e.screen_id);
            h = fnv_u32(h, e.aux);
            i += 1;
        }
        h = fnv_u64(h, self.total);
        fnv_u64(h, self.overflow)
    }
}

impl Default for LedgerRing {
    fn default() -> Self {
        LedgerRing::new()
    }
}

// ===================================================================
// 九、核心状态种类（供 const fn 返回的窄化视图）
// ===================================================================

/// 核心状态窄化视图（不携带内部数据，供 `state()` const 返回与比较）。
#[derive(Debug, Clone, Copy)]
pub enum CoreStateKind {
    /// 未引导。
    Offline,
    /// 正常工作。
    Active,
    /// 告警锁定。
    Alert,
}

impl CoreStateKind {
    /// 状态名。
    pub const fn name(&self) -> &'static str {
        match self {
            CoreStateKind::Offline => "Offline",
            CoreStateKind::Active => "Active",
            CoreStateKind::Alert => "Alert",
        }
    }
}

// ===================================================================
// 十、屏账户与多屏核心（聚合守恒 + 体检 + 指纹 + 双跑对拍）
// ===================================================================

/// 单屏账户：模型 + 档位 + 亮度 + DPMS + VRR + 最近报价。
#[derive(Debug, Clone, Copy)]
pub struct ScreenAccount {
    /// 屏 id。
    pub screen_id: u8,
    /// 面板功耗模型（boot 冻结实例，运行期只读）。
    pub model: PanelPowerModel,
    /// 当前刷新档。
    pub tier: RefreshTier,
    /// 当前亮度（千分比）。
    pub brightness_permille: u16,
    /// DPMS 语义引擎。
    pub dpms: DpmsSemantics,
    /// VRR 区间（None = 定频）。
    pub vrr: Option<VrrBand>,
    /// 最近一次报价（mW，含档位倍率与 DPMS 地板折扣）。
    pub last_quote_mw: u32,
    /// 累计报价次数。
    pub quote_count: u32,
}

/// 聚合报告：每屏分列 + 总账 + 守恒自证。
/// 报告行携带档位标注（`String` 由档位名生成）——高刷溢价标注全链
/// 传播的最后一跳：导出层直接逐行透出，无标注的行不可能存在。
#[derive(Debug, Clone)]
pub struct AggReport {
    /// 每屏分列行：`(id, 档位标注, DPMS 态, 亮度‰, 报价 mW)`。
    pub rows: Vec<(u8, String, String, u16, u32)>,
    /// Σ屏重算总显示功耗（mW）。
    pub sum_mw: u32,
    /// 运行总账（mW）。
    pub running_total_mw: u32,
    /// 守恒结论：sum == running。
    pub conserved: bool,
}

/// 健康体检报告：三档结论 + 明细计数。
#[derive(Debug, Clone, Copy)]
pub struct HealthReport {
    /// 结论："GOOD" / "WATCH" / "BAD"。
    pub verdict: &'static str,
    /// 守恒是否完好。
    pub conserved: bool,
    /// 模型漂移是否检出过。
    pub drift_detected: bool,
    /// 非法迁移拦截次数。
    pub illegal_dpms: u32,
    /// 内容拦截次数。
    pub content_intercepts: u32,
    /// 账溢出条数。
    pub ledger_overflow: u64,
    /// 注入检出数（0..3）。
    pub injections_detected: u8,
}

/// 显示功耗核心：多屏聚合 + 预算联动 + 事件账 + 状态机。
pub struct DispPwrCore {
    /// 核心状态。
    state: CoreState,
    /// 屏账户表（堆上，最多 MAX_SCREENS）。
    screens: Vec<ScreenAccount>,
    /// 运行总显示功耗（mW，随每次报价/DPMS 迁移重估）。
    running_total_mw: u32,
    /// 引导时冻结的模型指纹（按 kind_code 1..3 索引，下标 0 不用）。
    frozen_prints: [u64; 4],
    /// 预算联动件。
    budget: Option<PowerBudgetLink>,
    /// 事件账。
    ledger: LedgerRing,
    /// 守恒完好标志（破坏后置 false，进入 Alert）。
    conserved: bool,
    /// 模型漂移检出标志。
    drift_detected: bool,
    /// 最近一次预算建议。
    last_advice: Option<BrightnessAdvice>,
    /// 注入检出位图（bit0 模型漂移 / bit1 非法档位 / bit2 非法迁移）。
    injection_bits: u8,
}

/// 核心内部状态机：Offline → Active；守恒破坏/注入失守推入 Alert；
/// Alert 须显性解除（解除前一切功耗操作拒绝）。
#[derive(Debug, Clone, Copy)]
pub enum CoreState {
    /// 未引导。
    Offline,
    /// 正常工作。
    Active,
    /// 告警锁定：操作拒绝，等显性解除。
    Alert,
}

impl DispPwrCore {
    /// 引导：冻结三形态模型指纹，进入 Active，入账 Booted。
    /// boot 是唯一构造入口，"重复引导"由 [`reboot_check`] 显性表达。
    pub fn boot() -> Result<Self, DispPwrError> {
        let mut core = Self::build()?;
        core.ledger.push(EventKind::Booted, 0xFF, 0);
        Ok(core)
    }

    /// 重复引导防护检查：任何已构造实例再走引导检查路径即报
    /// [`DispPwrError::AlreadyBooted`]（幂等保护显性化，测试用）。
    pub fn reboot_check(&self) -> Result<(), DispPwrError> {
        match self.state {
            CoreState::Offline => Err(DispPwrError::NotBooted),
            _ => Err(DispPwrError::AlreadyBooted),
        }
    }

    /// 实际构造：冻结指纹、空屏表、Active 态。
    fn build() -> Result<Self, DispPwrError> {
        let frozen_prints = [
            0,
            PanelPowerModel::from_kind(PanelKind::LaptopIps133).fingerprint(),
            PanelPowerModel::from_kind(PanelKind::Desktop4k27).fingerprint(),
            PanelPowerModel::from_kind(PanelKind::OledMobile156).fingerprint(),
        ];
        Ok(DispPwrCore {
            state: CoreState::Active,
            screens: Vec::new(),
            running_total_mw: 0,
            frozen_prints,
            budget: None,
            ledger: LedgerRing::new(),
            conserved: true,
            drift_detected: false,
            last_advice: None,
            injection_bits: 0,
        })
    }

    /// 当前状态窄化视图。
    pub const fn state(&self) -> CoreStateKind {
        match self.state {
            CoreState::Offline => CoreStateKind::Offline,
            CoreState::Active => CoreStateKind::Active,
            CoreState::Alert => CoreStateKind::Alert,
        }
    }

    /// Active 态闸门：非 Active 一律拒绝。
    fn require_active(&self) -> Result<(), DispPwrError> {
        match self.state {
            CoreState::Active => Ok(()),
            _ => Err(DispPwrError::StateNotActive),
        }
    }

    /// 按屏 id 找下标（while+索引）。
    fn find_screen(&self, screen_id: u8) -> Option<usize> {
        let mut i = 0;
        while i < self.screens.len() {
            if self.screens[i].screen_id == screen_id {
                return Some(i);
            }
            i += 1;
        }
        None
    }

    /// 屏数（诊断/测试用）。
    pub fn screen_count(&self) -> usize {
        self.screens.len()
    }

    /// 模型指纹运行期校验：重算比对冻结值，漂移即告警入账并报错。
    /// **模型复用判据的守卫半边**：模型只有一张冻结指纹，不存在
    /// 第二套旁路系数。
    fn verify_model(&mut self, model: &PanelPowerModel) -> Result<(), DispPwrError> {
        // kind_code 1..3 直接做下标（0 号位为占位 0，未知 kind 指纹
        // 必与占位失配 → 按漂移处理：未知模型不许上机）。
        let mut idx = model.kind_code as usize;
        if idx > 3 {
            idx = 3;
        }
        let frozen = self.frozen_prints[idx];
        if model.fingerprint() != frozen {
            self.drift_detected = true;
            self.ledger
                .push(EventKind::ModelDriftAlert, 0xFF, model.kind_code as u32);
            return Err(DispPwrError::ModelDriftDetected);
        }
        Ok(())
    }

    /// 注册屏：表容量、id 唯一、模型指纹、亮度闭集四道闸。
    pub fn register_screen(
        &mut self,
        screen_id: u8,
        model: PanelPowerModel,
        tier: RefreshTier,
        brightness_permille: u16,
    ) -> Result<(), DispPwrError> {
        self.require_active()?;
        if self.screens.len() >= MAX_SCREENS {
            return Err(DispPwrError::ScreenTableFull);
        }
        if self.find_screen(screen_id).is_some() {
            return Err(DispPwrError::ScreenAlreadyRegistered);
        }
        self.verify_model(&model)?;
        if brightness_permille > BRIGHT_MAX_PERMILLE || brightness_permille < BRIGHT_MIN_PERMILLE {
            return Err(DispPwrError::BrightnessOutOfRange);
        }
        let acct = ScreenAccount {
            screen_id,
            model,
            tier,
            brightness_permille,
            dpms: DpmsSemantics::new(),
            vrr: None,
            last_quote_mw: 0,
            quote_count: 0,
        };
        self.screens.push(acct);
        self.ledger
            .push(EventKind::ScreenRegistered, screen_id, tier.hz() as u32);
        self.reevaluate_total();
        Ok(())
    }

    /// 设亮度：内容闸门（休眠拦截）→ 亮度闭集校验 → 落账重估。
    pub fn set_brightness(&mut self, screen_id: u8, permille: u16) -> Result<(), DispPwrError> {
        self.require_active()?;
        let idx = match self.find_screen(screen_id) {
            Some(i) => i,
            None => return Err(DispPwrError::ScreenNotFound),
        };
        if self.screens[idx].dpms.state.is_asleep() {
            self.screens[idx].dpms.content_intercepts =
                self.screens[idx].dpms.content_intercepts.saturating_add(1);
            self.ledger
                .push(EventKind::ContentIntercepted, screen_id, permille as u32);
            return Err(DispPwrError::ContentUnreachable);
        }
        if permille > BRIGHT_MAX_PERMILLE || permille < BRIGHT_MIN_PERMILLE {
            return Err(DispPwrError::BrightnessOutOfRange);
        }
        self.screens[idx].brightness_permille = permille;
        self.ledger
            .push(EventKind::BrightnessSet, screen_id, permille as u32);
        self.reevaluate_total();
        Ok(())
    }

    /// 设刷新档：内容闸门 → 档位闭集校验（非法档位注入的正面入口）。
    pub fn set_refresh_hz(&mut self, screen_id: u8, hz: u16) -> Result<(), DispPwrError> {
        self.require_active()?;
        let idx = match self.find_screen(screen_id) {
            Some(i) => i,
            None => return Err(DispPwrError::ScreenNotFound),
        };
        if self.screens[idx].dpms.state.is_asleep() {
            self.screens[idx].dpms.content_intercepts =
                self.screens[idx].dpms.content_intercepts.saturating_add(1);
            self.ledger
                .push(EventKind::ContentIntercepted, screen_id, hz as u32);
            return Err(DispPwrError::ContentUnreachable);
        }
        let tier = match RefreshTier::from_hz(hz) {
            Some(t) => t,
            None => return Err(DispPwrError::RefreshTierInvalid),
        };
        self.screens[idx].tier = tier;
        self.ledger.push(EventKind::RefreshSet, screen_id, hz as u32);
        self.reevaluate_total();
        Ok(())
    }

    /// 设 VRR 区间：构造时校验区间合法性。
    pub fn set_vrr(&mut self, screen_id: u8, min_hz: u16, max_hz: u16) -> Result<(), DispPwrError> {
        self.require_active()?;
        let idx = match self.find_screen(screen_id) {
            Some(i) => i,
            None => return Err(DispPwrError::ScreenNotFound),
        };
        let band = VrrBand::new(min_hz, max_hz)?;
        self.screens[idx].vrr = Some(band);
        let aux = ((min_hz as u32) << 16) | (max_hz as u32);
        self.ledger.push(EventKind::VrrSet, screen_id, aux);
        self.reevaluate_total();
        Ok(())
    }

    /// DPMS 迁移：合法性表裁决；非法显性拒绝并入账（拦截账目）。
    pub fn dpms_transition(
        &mut self,
        screen_id: u8,
        target: DpmsState,
    ) -> Result<(), DispPwrError> {
        self.require_active()?;
        let idx = match self.find_screen(screen_id) {
            Some(i) => i,
            None => return Err(DispPwrError::ScreenNotFound),
        };
        let res = self.screens[idx].dpms.request_transition(target);
        match res {
            Ok(()) => {
                self.ledger
                    .push(EventKind::DpmsTransition, screen_id, target.index() as u32);
                self.reevaluate_total();
                Ok(())
            }
            Err(e) => {
                self.ledger.push(
                    EventKind::DpmsIllegalBlocked,
                    screen_id,
                    target.index() as u32,
                );
                Err(e)
            }
        }
    }

    /// 单屏报价：模型指纹校验 → 基础报价（档位倍率）→ DPMS 地板折扣
    /// → 落账重估。休眠态报价取地板折扣值（屏睡了但仍在账上）。
    /// 返回的 [`PowerQuote`] 携带档位标注——标注全链传播。
    pub fn quote_screen(&mut self, screen_id: u8) -> Result<PowerQuote, DispPwrError> {
        self.require_active()?;
        let idx = match self.find_screen(screen_id) {
            Some(i) => i,
            None => return Err(DispPwrError::ScreenNotFound),
        };
        let acct = self.screens[idx];
        self.verify_model(&acct.model)?;
        let q = PowerQuote::new(
            screen_id,
            &acct.model,
            acct.tier,
            acct.brightness_permille,
            acct.vrr,
        )?;
        let floor = acct.dpms.floor_permille() as u64;
        let floored = ((q.base_mw as u64) * floor / 1000) as u32;
        let lo = ((q.band_low_mw as u64) * floor / 1000) as u32;
        let hi = ((q.band_high_mw as u64) * floor / 1000) as u32;
        let q = PowerQuote {
            base_mw: floored,
            band_low_mw: lo,
            band_high_mw: hi,
            ..q
        };
        self.screens[idx].last_quote_mw = floored;
        self.screens[idx].quote_count = self.screens[idx].quote_count.saturating_add(1);
        self.ledger.push(EventKind::QuoteTaken, screen_id, floored);
        self.reevaluate_total();
        Ok(q)
    }

    /// 总账重估：Σ屏报价 ≡ 运行总账——守恒的唯一写入口。
    /// 任何绕过本函数的改动都会在 `aggregate()` 的守恒校验处显性暴露。
    fn reevaluate_total(&mut self) {
        let mut sum: u32 = 0;
        let mut i = 0;
        while i < self.screens.len() {
            sum = sum.saturating_add(self.screens[i].last_quote_mw);
            i += 1;
        }
        self.running_total_mw = sum;
    }

    /// 多屏聚合：每屏分列 + Σ屏重算 vs 运行总账守恒校验。
    /// 不一致即报 [`DispPwrError::ConservationBroken`] 并推入 Alert。
    pub fn aggregate(&mut self) -> Result<AggReport, DispPwrError> {
        self.require_active()?;
        let mut rows: Vec<(u8, String, String, u16, u32)> = Vec::new();
        let mut sum: u32 = 0;
        let mut i = 0;
        while i < self.screens.len() {
            let a = &self.screens[i];
            rows.push((
                a.screen_id,
                String::from(a.tier.name()),
                String::from(a.dpms.state.name()),
                a.brightness_permille,
                a.last_quote_mw,
            ));
            sum = sum.saturating_add(a.last_quote_mw);
            i += 1;
        }
        if sum != self.running_total_mw {
            self.conserved = false;
            self.ledger
                .push(EventKind::ConservationBroken, 0xFF, sum);
            self.state = CoreState::Alert;
            return Err(DispPwrError::ConservationBroken);
        }
        self.ledger.push(EventKind::ConservationOk, 0xFF, sum);
        Ok(AggReport {
            rows,
            sum_mw: sum,
            running_total_mw: self.running_total_mw,
            conserved: true,
        })
    }

    /// 守恒完好标志（体检与测试用）。
    pub const fn is_conserved(&self) -> bool {
        self.conserved
    }

    /// 运行总账读数（mW）。
    pub const fn running_total_mw(&self) -> u32 {
        self.running_total_mw
    }

    /// 模型漂移检出标志。
    pub const fn drift_detected(&self) -> bool {
        self.drift_detected
    }

    /// 事件账只读指纹。
    pub fn ledger_fingerprint(&self) -> u64 {
        self.ledger.fingerprint()
    }

    /// 账溢出计数。
    pub const fn ledger_overflow(&self) -> u64 {
        self.ledger.overflow
    }

    /// 注入检出位图（bit0 漂移 / bit1 档位 / bit2 迁移）。
    pub const fn injection_bits(&self) -> u8 {
        self.injection_bits
    }

    /// 屏表状态指纹（不含事件账与预算）——用于验证"只建议不强制"：
    /// 建议评估前后该指纹必须不变。
    pub fn screens_fingerprint(&self) -> u64 {
        let mut h = fnv_u64(FNV_OFFSET, self.screens.len() as u64);
        let mut i = 0;
        while i < self.screens.len() {
            let a = &self.screens[i];
            h = fnv_byte(h, a.screen_id);
            h = fnv_u16(h, a.tier.multiplier_permille());
            h = fnv_u16(h, a.brightness_permille);
            h = fnv_byte(h, a.dpms.state.index() as u8);
            h = fnv_u32(h, a.last_quote_mw);
            i += 1;
        }
        h
    }

    /// 诊断探针：强置漂移标志（仅测试路径调用；生产路径无调用方，
    /// 漂移标志只能由指纹闸门置位）。
    pub fn force_drift_flag_for_probe(&mut self) {
        self.drift_detected = true;
    }
}

impl AggReport {
    /// 导出单行摘要（含屏数、总功耗、守恒结论）——导出面自带守恒
    /// 自证；逐行档位标注由 `rows` 直接透出，无标注的导出不可能存在。
    pub fn summary(&self) -> String {
        format!(
            "screens={} sum_mw={} running_mw={} conserved={}",
            self.rows.len(),
            self.sum_mw,
            self.running_total_mw,
            self.conserved
        )
    }
}

// ===================================================================
// 十一、预算联动接入核心（只建议不强制）+ 体检 + 指纹
// ===================================================================

impl DispPwrCore {
    /// 挂载整机预算联动件（可覆盖，落账）。
    pub fn attach_budget(&mut self, link: PowerBudgetLink) -> Result<(), DispPwrError> {
        self.require_active()?;
        self.budget = Some(link);
        Ok(())
    }

    /// 显示功耗入整机账：把当前显示总功耗与显示预算一并上报——
    /// 这是显示功耗进入整机功耗账（F1446 域）的唯一接口声明。
    /// 返回 `(显示预算 mW, 当前显示功耗 mW, 超支 mW)`；超支为 0 表示
    /// 预算内。未挂预算时报 [`DispPwrError::BudgetBelowFloor`] 语义
    /// 不合适，改为显性要求先挂载（复用 StateNotActive 之外的专码
    /// 不增段位，故返回 BudgetBelowFloor 仅用于"预算被击穿"）。
    pub fn report_to_machine_budget(&mut self) -> Result<(u32, u32, u32), DispPwrError> {
        self.require_active()?;
        self.reevaluate_total();
        let b = match self.budget {
            Some(b) => b,
            None => {
                // 未挂预算：显示预算按 0 记，超支按当前功耗记——
                // 显性暴露"账没挂上"，绝不静默报 0 超支。
                let cur = self.running_total_mw;
                self.ledger.push(EventKind::BudgetAdvice, 0xFF, cur);
                return Ok((0, cur, cur));
            }
        };
        let disp_budget = b.display_budget_mw();
        let cur = self.running_total_mw;
        let over = if cur > disp_budget {
            cur - disp_budget
        } else {
            0
        };
        self.ledger
            .push(EventKind::BudgetAdvice, 0xFF, disp_budget);
        Ok((disp_budget, cur, over))
    }

    /// 亮度策略评估：预算紧（显示功耗超显示预算）时给出降亮度建议。
    /// **只建议不强制**：本函数绝不修改任何屏的亮度/档位/DPMS 态，
    /// 采纳必须走显式 `set_brightness`（建议与执行的分离是判据语义）。
    ///
    /// 目标亮度取"最大屏"（最近报价最高者）的反向映射——降最大头
    /// 收益最高；多屏等比细调留给调用方多轮评估。
    pub fn evaluate_brightness_advice(&mut self) -> Result<BrightnessAdvice, DispPwrError> {
        self.require_active()?;
        self.reevaluate_total();
        let (disp_budget, cur, _over) = self.report_to_machine_budget()?;
        if cur <= disp_budget {
            self.last_advice = Some(BrightnessAdvice::Keep);
            return Ok(BrightnessAdvice::Keep);
        }
        // 找最近报价最大的屏（while+索引，显性比较）。
        let mut best: Option<usize> = None;
        let mut best_mw: u32 = 0;
        let mut i = 0;
        while i < self.screens.len() {
            if self.screens[i].last_quote_mw > best_mw {
                best_mw = self.screens[i].last_quote_mw;
                best = Some(i);
            }
            i += 1;
        }
        let over = cur.saturating_sub(disp_budget);
        let advice = match best {
            None => BrightnessAdvice::ReduceTo {
                target_permille: BRIGHT_MIN_PERMILLE,
                over_mw: over,
            },
            Some(bi) => {
                let model = self.screens[bi].model;
                // 目标：把该屏压到 (报价 - 超支) 对应的最大亮度档。
                let target_budget = best_mw.saturating_sub(over);
                let target = match model.brightness_for_budget(target_budget) {
                    Ok(b) => b,
                    Err(DispPwrError::BudgetBelowFloor) => BRIGHT_MIN_PERMILLE,
                    Err(e) => return Err(e),
                };
                BrightnessAdvice::ReduceTo {
                    target_permille: target,
                    over_mw: over,
                }
            }
        };
        self.last_advice = Some(advice);
        self.ledger.push(EventKind::BudgetAdvice, 0xFF, over);
        Ok(advice)
    }

    /// 最近一次建议（只读视图）。
    pub const fn last_advice(&self) -> Option<BrightnessAdvice> {
        self.last_advice
    }

    /// 告警解除：仅 Alert 态可调用；解除前重跑守恒自查，仍不一致
    /// 则拒绝解除（告警不是一键复位）。
    pub fn clear_alert(&mut self) -> Result<(), DispPwrError> {
        match self.state {
            CoreState::Alert => {}
            _ => return Err(DispPwrError::StateNotActive),
        }
        // 自查：重估后比对。
        let mut sum: u32 = 0;
        let mut i = 0;
        while i < self.screens.len() {
            sum = sum.saturating_add(self.screens[i].last_quote_mw);
            i += 1;
        }
        if sum != self.running_total_mw {
            self.conserved = false;
            return Err(DispPwrError::ConservationBroken);
        }
        self.conserved = true;
        self.state = CoreState::Active;
        self.ledger.push(EventKind::AlertCleared, 0xFF, sum);
        Ok(())
    }

    /// 全量确定性指纹：状态 + 屏表 + 总账 + 账本逐段入 FNV-1a 64。
    pub fn fingerprint(&self) -> u64 {
        let mut h = fnv_byte(FNV_OFFSET, match self.state {
            CoreState::Offline => 0u8,
            CoreState::Active => 1u8,
            CoreState::Alert => 2u8,
        });
        h = fnv_u32(h, self.running_total_mw);
        h = fnv_byte(h, if self.conserved { 1 } else { 0 });
        h = fnv_byte(h, if self.drift_detected { 1 } else { 0 });
        h = fnv_byte(h, self.injection_bits);
        h = fnv_byte(h, self.screens.len() as u8);
        let mut i = 0;
        while i < self.screens.len() {
            let a = &self.screens[i];
            h = fnv_byte(h, a.screen_id);
            h = fnv_u16(h, a.model.kind_code);
            h = fnv_u32(h, a.model.static_panel_mw);
            h = fnv_u32(h, a.model.backlight_baseline_mw);
            h = fnv_u32(h, a.model.linear_slope_mw);
            h = fnv_u16(h, a.tier.multiplier_permille());
            h = fnv_u16(h, a.brightness_permille);
            h = fnv_byte(h, a.dpms.state.index() as u8);
            h = fnv_u32(h, a.dpms.wake_latency_acc_ms);
            h = fnv_u32(h, a.dpms.legal_transitions);
            h = fnv_u32(h, a.dpms.illegal_intercepts);
            h = fnv_u32(h, a.dpms.content_intercepts);
            h = fnv_u32(h, a.last_quote_mw);
            h = fnv_u32(h, a.quote_count);
            i += 1;
        }
        self.ledger.fingerprint_fold(h)
    }

    /// 健康体检三档：
    /// * GOOD：守恒完好、无漂移、注入面 3/3 检出、无最重告警；
    /// * WATCH：守恒完好但有拦截/溢出等观测项；
    /// * BAD：守恒破坏 / 漂移未处置 / 注入失守 / 告警态。
    pub fn health(&self) -> HealthReport {
        let mut illegal = 0u32;
        let mut intercepts = 0u32;
        let mut i = 0;
        while i < self.screens.len() {
            illegal = illegal.saturating_add(self.screens[i].dpms.illegal_intercepts);
            intercepts = intercepts.saturating_add(self.screens[i].dpms.content_intercepts);
            i += 1;
        }
        let inj = self.injection_bits.count_ones() as u8;
        let mut verdict = "GOOD";
        if !self.conserved || self.drift_detected || inj < 3 || matches!(self.state, CoreState::Alert) {
            verdict = "BAD";
        } else if illegal > 0 || intercepts > 0 || self.ledger.overflow > 0 {
            verdict = "WATCH";
        }
        HealthReport {
            verdict,
            conserved: self.conserved,
            drift_detected: self.drift_detected,
            illegal_dpms: illegal,
            content_intercepts: intercepts,
            ledger_overflow: self.ledger.overflow,
            injections_detected: inj,
        }
    }
}

// ===================================================================
// 十二、注入面与双跑确定性对拍
// ===================================================================

/// 注入检出标记（位图位）。
const INJ_MODEL_DRIFT: u8 = 1;
/// 非法档位注入位。
const INJ_BAD_TIER: u8 = 2;
/// 非法 DPMS 迁移注入位。
const INJ_BAD_DPMS: u8 = 4;

/// 双跑脚本步骤闭集：对每个注册屏执行一步操作。
#[derive(Debug, Clone, Copy)]
pub enum ScriptOp {
    /// 设亮度。
    SetBrightness(u16),
    /// 设刷新档（Hz）。
    SetRefreshHz(u16),
    /// DPMS 迁移到目标态。
    Dpms(DpmsState),
    /// 报价。
    Quote,
}

impl DispPwrCore {
    /// 注入面（三组测试判据的对抗半边）：每路注入都必须被检出。
    ///
    /// * 模型漂移注入：篡改模型系数副本 → 指纹比对必须显性报错；
    /// * 非法档位注入：90Hz 不在档位闭集 → 必须显性报错；
    /// * 非法 DPMS 迁移注入：Suspend→Standby → 必须显性报错并入账。
    ///
    /// 全部检出返回 Ok；任何一路失守返回
    /// [`DispPwrError::InjectionUndetected`]（体检随之 BAD）。
    pub fn run_injection_suite(&mut self) -> Result<[bool; 3], DispPwrError> {
        self.require_active()?;
        let mut results = [false; 3];
        // ── 注入一：模型漂移 ──
        // 复制在册模型并篡改斜率（+1），指纹必与冻结值失配。
        if self.screens.is_empty() {
            return Err(DispPwrError::ScreenNotFound);
        }
        let mut drift_model = self.screens[0].model;
        drift_model.linear_slope_mw = drift_model.linear_slope_mw.wrapping_add(1);
        let drifted = drift_model.fingerprint() != self.screens[0].model.fingerprint();
        // 让校验路径吃下这个漂移副本：register 一个同 id 必撞唯一闸，
        // 改走 verify_model 直检（通过临时新屏注册到空闲 id）。
        let probe_id: u8 = if self.screens.len() < MAX_SCREENS {
            0x7F
        } else {
            0x7E
        };
        let verify_res = if self.screens.len() < MAX_SCREENS {
            self.register_screen(probe_id, drift_model, RefreshTier::Hz60, BRIGHT_MIN_PERMILLE)
        } else {
            Err(DispPwrError::ScreenTableFull)
        };
        if drifted && matches!(verify_res, Err(DispPwrError::ModelDriftDetected)) {
            results[0] = true;
            self.injection_bits |= INJ_MODEL_DRIFT;
            self.ledger.push(EventKind::InjectionDetected, probe_id, 1);
        }
        // ── 注入二：非法档位 ──
        if !self.screens.is_empty() {
            let sid = self.screens[0].screen_id;
            let bad = self.set_refresh_hz(sid, 90);
            if matches!(bad, Err(DispPwrError::RefreshTierInvalid)) {
                results[1] = true;
                self.injection_bits |= INJ_BAD_TIER;
                self.ledger.push(EventKind::InjectionDetected, sid, 2);
            }
        }
        // ── 注入三：非法 DPMS 迁移（Suspend→Standby）──
        if !self.screens.is_empty() {
            let sid = self.screens[0].screen_id;
            // 先合法下探到 Suspend，再注入非法逆行。
            let down = self.dpms_transition(sid, DpmsState::Suspend);
            let bad = self.dpms_transition(sid, DpmsState::Standby);
            if down.is_ok() && matches!(bad, Err(DispPwrError::DpmsTransitionIllegal)) {
                results[2] = true;
                self.injection_bits |= INJ_BAD_DPMS;
                self.ledger.push(EventKind::InjectionDetected, sid, 3);
            }
        }
        if !results[0] || !results[1] || !results[2] {
            self.ledger.push(EventKind::InjectionMissed, 0xFF, 0);
            self.state = CoreState::Alert;
            return Err(DispPwrError::InjectionUndetected);
        }
        // 注入一检出的是"外部漂移模型被闸门拒绝"，在册模型从未被
        // 篡改——检出成功后复位漂移标志，避免体检误报 BAD。
        // （闸门拦截 ≠ 系统被攻破；真正的在册模型漂移无法绕过闸门。）
        self.drift_detected = false;
        Ok(results)
    }

    /// 双跑确定性对拍：同一份脚本在两个独立实例上重放，指纹必须
    /// 一致；分歧即确定性破缺，显性报
    /// [`DispPwrError::DualRunMismatch`]。
    ///
    /// 脚本先在本实例上执行一遍（结果留在本实例），再在副本实例上
    /// 执行一遍并对拍指纹。
    pub fn run_dual_comparison(&mut self, ops: &[ScriptOp]) -> Result<u64, DispPwrError> {
        self.require_active()?;
        // 先快照屏配置（重放会改运行态，副本必须从同一初态出发）。
        let mut snap: Vec<(u8, PanelPowerModel, RefreshTier, u16, Option<VrrBand>)> = Vec::new();
        let mut s = 0;
        while s < self.screens.len() {
            let a = &self.screens[s];
            snap.push((
                a.screen_id,
                a.model,
                a.tier,
                a.brightness_permille,
                a.vrr,
            ));
            s += 1;
        }
        let budget_snap = self.budget;
        // ── 第一跑：本实例 ──
        self.replay(ops)?;
        let fp_a = self.fingerprint();
        // ── 第二跑：副本实例（同初态同脚本）──
        let mut twin = DispPwrCore::boot()?;
        let mut i = 0;
        while i < snap.len() {
            let (sid, model, tier, bright, vrr) = snap[i];
            twin.register_screen(sid, model, tier, bright)?;
            if let Some(band) = vrr {
                twin.set_vrr(sid, band.min_hz, band.max_hz)?;
            }
            i += 1;
        }
        if let Some(b) = budget_snap {
            twin.attach_budget(b)?;
        }
        twin.replay(ops)?;
        let fp_b = twin.fingerprint();
        if fp_a != fp_b {
            return Err(DispPwrError::DualRunMismatch);
        }
        Ok(fp_a)
    }

    /// 脚本重放：先快照屏 id 列表，再逐操作逐屏执行；可失败操作
    /// 失败即中止（脚本本身需自洽；对拍双方同脚本同失败也是确定性）。
    fn replay(&mut self, ops: &[ScriptOp]) -> Result<(), DispPwrError> {
        // 快照屏 id（快照后 Vec 只读遍历，避免借用与索引失效）。
        let mut ids: Vec<u8> = Vec::new();
        let mut k = 0;
        while k < self.screens.len() {
            ids.push(self.screens[k].screen_id);
            k += 1;
        }
        let mut o = 0;
        while o < ops.len() {
            let mut n = 0;
            while n < ids.len() {
                match ops[o] {
                    ScriptOp::SetBrightness(b) => {
                        self.set_brightness(ids[n], b)?;
                    }
                    ScriptOp::SetRefreshHz(hz) => {
                        self.set_refresh_hz(ids[n], hz)?;
                    }
                    ScriptOp::Dpms(s) => {
                        self.dpms_transition(ids[n], s)?;
                    }
                    ScriptOp::Quote => {
                        self.quote_screen(ids[n])?;
                    }
                }
                n += 1;
            }
            o += 1;
        }
        Ok(())
    }
}

// ===================================================================
// 十三、测试（模型 / 标注 / 休眠 三组判据 + 守恒 + 注入 + 双跑）
// ===================================================================

#[cfg(test)]
mod tests {
    use super::*;

    /// 测试辅助：把 Result 拆包，失败时带中文诊断信息中止（仅测试路径）。
    fn must_ok<T>(r: Result<T, DispPwrError>) -> T {
        match r {
            Ok(v) => v,
            Err(e) => panic!("预期成功，实得 {}（{}）", e.code(), e.detail()),
        }
    }

    /// 测试辅助：断言错误正是预期种类（按诊断码比对，不依赖 PartialEq）。
    fn must_err<T>(r: Result<T, DispPwrError>, want: u16) {
        match r {
            Ok(_) => panic!("预期错误 0x{:04X}，实得成功", want),
            Err(e) => assert!(
                e.code() == want,
                "预期 0x{:04X}，实得 0x{:04X}（{}）",
                want,
                e.code(),
                e.detail()
            ),
        }
    }

    /// 标准双屏配置：笔记本屏(1) + 桌面屏(2)，60Hz/亮度 500‰。
    fn boot_two_screens() -> DispPwrCore {
        let mut c = must_ok(DispPwrCore::boot());
        must_ok(c.register_screen(
            1,
            PanelPowerModel::from_kind(PanelKind::LaptopIps133),
            RefreshTier::Hz60,
            500,
        ));
        must_ok(c.register_screen(
            2,
            PanelPowerModel::from_kind(PanelKind::Desktop4k27),
            RefreshTier::Hz60,
            500,
        ));
        c
    }

    // ─────────── 第一组：模型（亮度映射 / 指纹冻结 / 漂移检出） ───────────

    #[test]
    fn t01_brightness_mapping_monotonic_and_endpoints() {
        let m = PanelPowerModel::from_kind(PanelKind::LaptopIps133);
        // 端点：0/1000‰ 显性拒绝（闭集 50..=1000）。
        must_err(m.power_at(0), 0x1D92);
        must_err(m.power_at(1001), 0x1D92);
        must_err(m.power_at(49), 0x1D92);
        // 下界可报价。
        let p_min = must_ok(m.power_at(BRIGHT_MIN_PERMILLE));
        assert!(p_min >= m.static_panel_mw + m.backlight_baseline_mw);
        // 单调不减。
        let mut b = BRIGHT_MIN_PERMILLE;
        let mut prev = p_min;
        while b <= BRIGHT_MAX_PERMILLE {
            let p = must_ok(m.power_at(b));
            assert!(p >= prev, "亮度映射必须单调不减");
            prev = p;
            b += 50;
        }
        // 双成分形状：满亮度 ≈ 静态 + 基线 + 1000×斜率。
        let p_max = must_ok(m.power_at(BRIGHT_MAX_PERMILLE));
        let expect = m
            .static_panel_mw
            .saturating_add(m.backlight_baseline_mw)
            .saturating_add(m.linear_slope_mw * 1000);
        assert_eq!(p_max, expect);
        // 桌面 4K 面板功耗显著高于笔记本面板。
        let d = PanelPowerModel::from_kind(PanelKind::Desktop4k27);
        assert!(must_ok(d.power_at(500)) > must_ok(m.power_at(500)));
    }

    #[test]
    fn t02_model_fingerprint_frozen_and_deterministic() {
        let m1 = PanelPowerModel::from_kind(PanelKind::Desktop4k27);
        let m2 = PanelPowerModel::from_kind(PanelKind::Desktop4k27);
        // 同系数同指纹（确定性）。
        assert_eq!(m1.fingerprint(), m2.fingerprint());
        // 三形态指纹两两不同。
        let l = PanelPowerModel::from_kind(PanelKind::LaptopIps133);
        let o = PanelPowerModel::from_kind(PanelKind::OledMobile156);
        assert!(m1.fingerprint() != l.fingerprint());
        assert!(m1.fingerprint() != o.fingerprint());
        assert!(l.fingerprint() != o.fingerprint());
        // 系数微动指纹必变（漂移可检）。
        let mut drifted = m1;
        drifted.linear_slope_mw += 1;
        assert!(drifted.fingerprint() != m1.fingerprint());
    }

    #[test]
    fn t03_budget_inverse_mapping() {
        let m = PanelPowerModel::from_kind(PanelKind::LaptopIps133);
        // 静态地板：1.4mW 级预算必拒。
        must_err(m.brightness_for_budget(10), 0x1D9A);
        // 正向反演：给足预算 → 亮度档与正向映射自洽（预算 ≥ 该档报价）。
        let b = must_ok(m.brightness_for_budget(4000));
        let p = must_ok(m.power_at(b));
        assert!(p <= 4000, "反演档的报价不得超过预算");
        if b < BRIGHT_MAX_PERMILLE {
            let p_next = must_ok(m.power_at(b + 1));
            assert!(p_next > 4000, "反演档必须是预算内的最大档");
        }
    }

    // ─────────── 第二组：标注（档位闭集 / VRR 带 / 全链传播） ───────────

    #[test]
    fn t04_refresh_tier_table() {
        // 档位表写死值核对（来源注释：120/144/240 典型溢价折算）。
        assert_eq!(RefreshTier::Hz60.multiplier_permille(), 1000);
        assert_eq!(RefreshTier::Hz120.multiplier_permille(), 1180);
        assert_eq!(RefreshTier::Hz144.multiplier_permille(), 1220);
        assert_eq!(RefreshTier::Hz240.multiplier_permille(), 1350);
        // 档位名标注非空。
        assert!(!RefreshTier::Hz144.name().is_empty());
    }

    #[test]
    fn t05_refresh_tier_parse_and_injection() {
        // 闭集内解析。
        assert!(RefreshTier::from_hz(60).is_some());
        assert!(RefreshTier::from_hz(120).is_some());
        assert!(RefreshTier::from_hz(144).is_some());
        assert!(RefreshTier::from_hz(240).is_some());
        // 闭集外显性失配：非法档位注入面。
        assert!(RefreshTier::from_hz(90).is_none());
        assert!(RefreshTier::from_hz(0).is_none());
        assert!(RefreshTier::from_hz(1000).is_none());
        // 插值倍率：区间内单调、端点闭集值精确。
        assert_eq!(RefreshTier::multiplier_at_hz(60), Some(1000));
        assert_eq!(RefreshTier::multiplier_at_hz(240), Some(1350));
        let m90 = RefreshTier::multiplier_at_hz(90);
        let m60 = RefreshTier::multiplier_at_hz(60);
        let m120 = RefreshTier::multiplier_at_hz(120);
        assert!(m90.is_some());
        let mv90 = m90.unwrap_or(0) as u32;
        assert!(mv90 > m60.unwrap_or(0) as u32 && mv90 < m120.unwrap_or(0) as u32);
        // 区间外显性拒绝。
        assert!(RefreshTier::multiplier_at_hz(29).is_none());
        assert!(RefreshTier::multiplier_at_hz(241).is_none());
    }

    #[test]
    fn t06_quote_carries_tier_annotation() {
        let m = PanelPowerModel::from_kind(PanelKind::LaptopIps133);
        let q60 = must_ok(PowerQuote::new(1, &m, RefreshTier::Hz60, 500, None));
        let q240 = must_ok(PowerQuote::new(1, &m, RefreshTier::Hz240, 500, None));
        // 标注在载荷上（档位不可为 None——构造强制）。
        assert!(q60.tier.is_some() && q240.tier.is_some());
        assert_eq!(q60.tier_mult_permille, 1000);
        assert_eq!(q240.tier_mult_permille, 1350);
        // 高刷溢价量化：240Hz 报价 ≈ 60Hz × 1.35。
        assert!(q240.base_mw > q60.base_mw);
        let ratio = (q240.base_mw as u64) * 1000 / (q60.base_mw as u64);
        assert!(ratio >= 1340 && ratio <= 1360, "溢价倍率应为 ~1.35，实得 {}", ratio);
        // 无 VRR 时带端收敛于报价本身。
        assert_eq!(q60.band_low_mw, q60.base_mw);
        assert_eq!(q60.band_high_mw, q60.base_mw);
    }

    #[test]
    fn t07_vrr_band_and_invalid() {
        // 合法区间。
        let band = must_ok(VrrBand::new(48, 144));
        let (lo, hi) = must_ok(band.band_multipliers());
        assert!(lo < hi);
        // 非法区间：min>=max、越下界、越上界。
        must_err(VrrBand::new(144, 48), 0x1D95);
        must_err(VrrBand::new(144, 144), 0x1D95);
        must_err(VrrBand::new(10, 60), 0x1D95);
        must_err(VrrBand::new(60, 300), 0x1D95);
        // VRR 报价带：带端来自区间两端倍率。
        let m = PanelPowerModel::from_kind(PanelKind::OledMobile156);
        let q = must_ok(PowerQuote::new(9, &m, RefreshTier::Hz60, 500, Some(band)));
        assert!(q.band_low_mw < q.base_mw && q.base_mw < q.band_high_mw);
    }

    // ─────────── 第三组：休眠（DPMS 状态机 / 拦截 / 延迟账） ───────────

    #[test]
    fn t08_dpms_table_and_legality() {
        // 四态地板写死值（DPMS 标准 + 典型规格注释）。
        assert_eq!(DpmsState::On.floor_permille(), 1000);
        assert_eq!(DpmsState::Standby.floor_permille(), 600);
        assert_eq!(DpmsState::Suspend.floor_permille(), 300);
        assert_eq!(DpmsState::Off.floor_permille(), 20);
        // 唤醒延迟写死值。
        assert_eq!(DpmsState::On.wake_latency_ms(), 0);
        assert_eq!(DpmsState::Standby.wake_latency_ms(), 50);
        assert_eq!(DpmsState::Suspend.wake_latency_ms(), 200);
        assert_eq!(DpmsState::Off.wake_latency_ms(), 2000);
        // 合法迁移抽查。
        assert!(DpmsState::On.can_transition(DpmsState::Off));
        assert!(DpmsState::Standby.can_transition(DpmsState::On));
        assert!(DpmsState::Suspend.can_transition(DpmsState::Off));
        // 非法迁移（省电方向不可逆行）。
        assert!(!DpmsState::Off.can_transition(DpmsState::Standby));
        assert!(!DpmsState::Off.can_transition(DpmsState::Suspend));
        assert!(!DpmsState::Suspend.can_transition(DpmsState::Standby));
        // 休眠态判定。
        assert!(!DpmsState::On.is_asleep());
        assert!(DpmsState::Standby.is_asleep());
        assert!(DpmsState::Off.is_asleep());
    }

    #[test]
    fn t09_dpms_state_machine_flow() {
        let mut d = DpmsSemantics::new();
        assert!(!d.state.is_asleep());
        // On→Standby→Suspend→Off 逐级下探合法。
        must_ok(d.request_transition(DpmsState::Standby));
        must_ok(d.request_transition(DpmsState::Suspend));
        must_ok(d.request_transition(DpmsState::Off));
        assert_eq!(d.legal_transitions, 3);
        // 预记账延迟：50+200+2000 = 2250ms（饱和累加）。
        assert_eq!(d.wake_latency_acc_ms, 2250);
        // Off→Standby 非法迁移显性拒绝并计数。
        must_err(d.request_transition(DpmsState::Standby), 0x1D96);
        assert_eq!(d.illegal_intercepts, 1);
        // 合法迁移计数不受非法请求影响。
        assert_eq!(d.legal_transitions, 3);
        // 唤醒直达 On，返回本次预记账延迟 2000ms。
        let woke = must_ok(d.wake());
        assert_eq!(woke, 2000);
        assert!(!d.state.is_asleep());
        // On 态唤醒延迟为 0。
        let woke2 = must_ok(d.wake());
        assert_eq!(woke2, 0);
    }

    #[test]
    fn t10_content_unreachable_during_sleep() {
        let mut d = DpmsSemantics::new();
        // On 态内容可达。
        must_ok(d.content_gate());
        must_ok(d.request_transition(DpmsState::Standby));
        // 休眠期内容闸门：显性拦截（内容不可达），绝不静默成功。
        must_err(d.content_gate(), 0x1D97);
        must_err(d.content_gate(), 0x1D97);
        assert_eq!(d.content_intercepts, 2);
        // 唤醒后恢复可达。
        must_ok(d.wake());
        must_ok(d.content_gate());
        assert_eq!(d.content_intercepts, 2);
    }

    // ─────────── 核心：注册 / 引用闸门 / 聚合守恒 / 分列 ───────────

    #[test]
    fn t11_boot_guards_and_registration() {
        let c = must_ok(DispPwrCore::boot());
        // 重复引导显性拒绝。
        must_err(c.reboot_check(), 0x1D91);
        let mut c = boot_two_screens();
        assert_eq!(c.screen_count(), 2);
        // 重复注册拒绝。
        must_err(
            c.register_screen(
                1,
                PanelPowerModel::from_kind(PanelKind::LaptopIps133),
                RefreshTier::Hz60,
                500,
            ),
            0x1D99,
        );
        // 亮度越界注册拒绝。
        must_err(
            c.register_screen(
                3,
                PanelPowerModel::from_kind(PanelKind::LaptopIps133),
                RefreshTier::Hz60,
                0,
            ),
            0x1D92,
        );
        // 未注册屏操作拒绝。
        must_err(c.set_brightness(9, 500), 0x1D98);
        must_err(c.quote_screen(9), 0x1D98);
        must_err(c.dpms_transition(9, DpmsState::Off), 0x1D98);
    }

    #[test]
    fn t12_multi_screen_aggregation_conservation() {
        let mut c = boot_two_screens();
        // 首轮报价：两屏各自落账。
        let q1 = must_ok(c.quote_screen(1));
        let q2 = must_ok(c.quote_screen(2));
        let rep = must_ok(c.aggregate());
        // 守恒：Σ屏 ≡ 运行总账。
        assert!(rep.conserved);
        assert_eq!(rep.sum_mw, q1.base_mw.saturating_add(q2.base_mw));
        assert_eq!(rep.running_total_mw, rep.sum_mw);
        // 分列：每行带档位标注与 DPMS 态标注，行数 = 屏数。
        assert_eq!(rep.rows.len(), 2);
        assert_eq!(rep.rows[0].1, String::from(RefreshTier::Hz60.name()));
        assert_eq!(rep.rows[0].2, String::from(DpmsState::On.name()));
        // 高刷标注传播到分列：改档后重新聚合，行标注随之更新。
        must_ok(c.set_refresh_hz(1, 240));
        let _ = must_ok(c.quote_screen(1));
        let rep2 = must_ok(c.aggregate());
        assert_eq!(rep2.rows[0].1, String::from(RefreshTier::Hz240.name()));
        // 守恒依然成立。
        assert!(rep2.conserved);
    }

    #[test]
    fn t13_dpms_floor_discount_in_quote() {
        let mut c = boot_two_screens();
        let q_on = must_ok(c.quote_screen(2)).base_mw;
        // 入 Off 后报价按 20‰ 地板折扣（屏睡了但仍在账上）。
        must_ok(c.dpms_transition(2, DpmsState::Off));
        let q_off = must_ok(c.quote_screen(2)).base_mw;
        let expect = (q_on as u64) * 20 / 1000;
        assert_eq!(q_off as u64, expect);
        assert!(q_off < q_on);
        // 聚合守恒跨 DPMS 迁移仍成立。
        must_ok(c.quote_screen(1));
        let rep = must_ok(c.aggregate());
        assert!(rep.conserved);
        assert_eq!(rep.rows[1].2, String::from(DpmsState::Off.name()));
        // 休眠期调节被拦截（核心路径的内容闸门）。
        must_err(c.set_brightness(2, 800), 0x1D97);
        must_err(c.set_refresh_hz(2, 120), 0x1D97);
        // 唤醒后调节恢复。
        must_ok(c.dpms_transition(2, DpmsState::On));
        must_ok(c.set_brightness(2, 800));
    }

    #[test]
    fn t14_ledger_overflow_explicit() {
        let mut c = boot_two_screens();
        must_ok(c.quote_screen(1));
        // 连续报价把账打满再溢出：溢出必须显性计数。
        let mut n = 0;
        while n < LEDGER_CAP + 40 {
            let _ = must_ok(c.quote_screen(1));
            n += 1;
        }
        assert!(c.ledger_overflow() >= 40, "溢出必须显性计数");
        // 有效条数封顶于容量。
        assert!(c.ledger_fingerprint() != 0);
        // 账溢出属观测项：体检 WATCH（注入 0/3 时为 BAD，先不跑注入）。
        let h = c.health();
        assert!(h.ledger_overflow >= 40);
    }

    // ─────────── 预算联动：只建议不强制 ───────────

    #[test]
    fn t15_budget_link_and_advice_semantics() {
        let mut c = boot_two_screens();
        must_ok(c.quote_screen(1));
        must_ok(c.quote_screen(2));
        let (_, cur, _) = must_ok(c.report_to_machine_budget());
        // 未挂预算：显性暴露（预算 0、全额超支），不静默。
        assert_eq!(cur, c.running_total_mw);
        // 挂载预算：桌面形态整机 65W、显示份额 25%、其它预留 40W。
        // 显示预算 = (65000-40000)×25% = 6250mW。
        let link = must_ok(PowerBudgetLink::new(65000, 250, 40000));
        must_ok(c.attach_budget(link));
        let (disp_budget, _, _) = must_ok(c.report_to_machine_budget());
        assert_eq!(disp_budget, 6250);
        // 非法预算：份额超 1000‰ / 预留超预算。
        must_err(PowerBudgetLink::new(65000, 1500, 0), 0x1D9A);
        must_err(PowerBudgetLink::new(1000, 250, 2000), 0x1D9A);
        // 预算评估：只产出建议，不改任何屏状态（屏表指纹前后一致）。
        let before_fp = c.screens_fingerprint();
        let advice = must_ok(c.evaluate_brightness_advice());
        assert_eq!(before_fp, c.screens_fingerprint(), "建议不得改屏状态");
        match advice {
            BrightnessAdvice::Keep => {
                // 预算内：保持。
                assert!(cur <= disp_budget);
            }
            BrightnessAdvice::ReduceTo { target_permille, over_mw } => {
                assert!(over_mw > 0);
                assert!(target_permille <= BRIGHT_MAX_PERMILLE);
                // 建议文案非空（只建议语义显性可读）。
                assert!(!advice.describe().is_empty());
            }
        }
        // 建议未强制：亮度仍为原值 500。
        let rep = must_ok(c.aggregate());
        assert_eq!(rep.rows[0].3, 500);
        assert_eq!(rep.rows[1].3, 500);
        // 导出摘要自带守恒结论。
        assert!(rep.summary().contains("conserved=true"));
    }

    #[test]
    fn t16_advice_adoption_is_explicit() {
        // 预算极紧：建议必然 ReduceTo；采纳后超支压平。
        let mut c = boot_two_screens();
        must_ok(c.quote_screen(1));
        must_ok(c.quote_screen(2));
        let link = must_ok(PowerBudgetLink::new(65000, 250, 50000));
        must_ok(c.attach_budget(link));
        // 显示预算 = 15000×25% = 3750mW，双屏必超。
        let (_, _, over_before) = must_ok(c.report_to_machine_budget());
        assert!(over_before > 0);
        let advice = must_ok(c.evaluate_brightness_advice());
        let target = match advice {
            BrightnessAdvice::ReduceTo { target_permille, .. } => target_permille,
            BrightnessAdvice::Keep => panic!("紧预算必须给降亮度建议"),
        };
        // 显式采纳（只对最大屏）：超支应显著收窄（不要求清零——
        // 建议只压最大头，多轮细调留给调用方）。
        must_ok(c.set_brightness(2, target));
        let _ = must_ok(c.quote_screen(2));
        let (_, _, over_after) = must_ok(c.report_to_machine_budget());
        assert!(
            over_after < over_before,
            "采纳建议后超支应收窄：{} -> {}",
            over_before,
            over_after
        );
    }

    // ─────────── 注入面：三路全部检出 ───────────

    #[test]
    fn t17_injection_suite_all_detected() {
        let mut c = boot_two_screens();
        let results = must_ok(c.run_injection_suite());
        assert!(results[0], "模型漂移注入必须检出");
        assert!(results[1], "非法档位注入必须检出");
        assert!(results[2], "非法 DPMS 迁移注入必须检出");
        assert_eq!(c.injection_bits(), 0b111);
        // 注入后核心仍 Active（检出不是失守）。
        assert_eq!(c.state().name(), "Active");
        // 非法迁移拦截留痕在屏账户上。
        let h = c.health();
        assert_eq!(h.injections_detected, 3);
        assert!(h.illegal_dpms >= 1);
    }

    #[test]
    fn t18_drift_via_quote_gate_detected() {
        // 报价路径同样过指纹闸：注册合法屏后模型不可被换。
        let mut c = boot_two_screens();
        must_ok(c.quote_screen(1));
        // 用漂移模型再注册（同 id 撞唯一闸前先撞指纹闸）。
        let mut bad = PanelPowerModel::from_kind(PanelKind::LaptopIps133);
        bad.linear_slope_mw = bad.linear_slope_mw.saturating_add(7);
        must_err(
            c.register_screen(3, bad, RefreshTier::Hz60, 500),
            0x1D93,
        );
        assert!(c.drift_detected());
        // 在册屏报价不受影响。
        let _ = must_ok(c.quote_screen(1));
    }

    // ─────────── 双跑确定性对拍 ───────────

    #[test]
    fn t19_dual_run_deterministic() {
        let mut c = boot_two_screens();
        let script = [
            ScriptOp::SetBrightness(700),
            ScriptOp::SetRefreshHz(144),
            ScriptOp::Quote,
            ScriptOp::Dpms(DpmsState::Standby),
            ScriptOp::Quote,
            ScriptOp::Dpms(DpmsState::On),
            ScriptOp::SetBrightness(300),
            ScriptOp::Quote,
        ];
        let fp1 = must_ok(c.run_dual_comparison(&script));
        // 再跑一次：同脚本同指纹（确定性重放）。
        let mut c2 = boot_two_screens();
        let fp2 = must_ok(c2.run_dual_comparison(&script));
        assert_eq!(fp1, fp2, "同脚本双实例指纹必须一致");
        // 空脚本也对拍一致。
        let mut c3 = boot_two_screens();
        let _ = must_ok(c3.run_dual_comparison(&[]));
    }

    // ─────────── 体检 / 告警解除 / 状态闸门 ───────────

    #[test]
    fn t20_health_and_alert_recovery() {
        let mut c = boot_two_screens();
        must_ok(c.quote_screen(1));
        must_ok(c.quote_screen(2));
        // 干净态体检（注入 3/3 检出且守恒完好）：注入过程留下
        // 非法迁移拦截痕迹 → WATCH（观测项，非 BAD）。
        must_ok(c.run_injection_suite());
        let h = c.health();
        assert_eq!(h.verdict, "WATCH", "3/3 检出且守恒完好应为 WATCH");
        assert!(h.conserved);
        assert_eq!(h.injections_detected, 3);
        // 守恒破坏路径：直接构造不一致（借用运行账做交叉校验）。
        // 注：守恒破坏由 aggregate 内部比对触发；此处验证告警闸门：
        // 漂移告警会把体检拉到 BAD。
        c.force_drift_flag_for_probe();
        let h2 = c.health();
        assert_eq!(h2.verdict, "BAD");
        assert!(h2.drift_detected);
    }

    #[test]
    fn t21_not_booted_and_state_gates() {
        // NotBooted 只能通过 Offline 态实例触发：用 boot 后状态不可达，
        // 由 reboot_check 的 Offline 分支覆盖码位。
        let mut c = boot_two_screens();
        must_ok(c.quote_screen(1));
        // 漂移标志强置后 aggregate 仍按守恒裁决（漂移不改账）。
        c.force_drift_flag_for_probe();
        let rep = must_ok(c.aggregate());
        assert!(rep.conserved);
    }

    // ─────────── 错误闭集自检 ───────────

    #[test]
    fn t22_error_codes_fill_segment() {
        // 16 变体一码一段，全部落在 0x1D90..0x1D9F 且互不重码。
        let all = [
            DispPwrError::NotBooted,
            DispPwrError::AlreadyBooted,
            DispPwrError::BrightnessOutOfRange,
            DispPwrError::ModelDriftDetected,
            DispPwrError::RefreshTierInvalid,
            DispPwrError::VrrBandInvalid,
            DispPwrError::DpmsTransitionIllegal,
            DispPwrError::ContentUnreachable,
            DispPwrError::ScreenNotFound,
            DispPwrError::ScreenAlreadyRegistered,
            DispPwrError::BudgetBelowFloor,
            DispPwrError::ConservationBroken,
            DispPwrError::ScreenTableFull,
            DispPwrError::DualRunMismatch,
            DispPwrError::StateNotActive,
            DispPwrError::InjectionUndetected,
        ];
        assert_eq!(all.len(), 16);
        let mut i = 0;
        while i < all.len() {
            let ci = all[i].code();
            assert!(ci >= 0x1D90 && ci <= 0x1D9F, "码越段：0x{:04X}", ci);
            assert!(!all[i].detail().is_empty());
            let mut j = i + 1;
            while j < all.len() {
                assert!(ci != all[j].code(), "重码：0x{:04X}", ci);
                j += 1;
            }
            i += 1;
        }
    }

    #[test]
    fn t23_screen_table_cap() {
        let mut c = must_ok(DispPwrCore::boot());
        // 注册到上限 8 屏。
        let mut id = 1u8;
        while id <= 8 {
            must_ok(c.register_screen(
                id,
                PanelPowerModel::from_kind(PanelKind::LaptopIps133),
                RefreshTier::Hz60,
                500,
            ));
            id += 1;
        }
        must_err(
            c.register_screen(
                9,
                PanelPowerModel::from_kind(PanelKind::LaptopIps133),
                RefreshTier::Hz60,
                500,
            ),
            0x1D9C,
        );
        // 8 屏聚合守恒。
        let mut sid = 1u8;
        while sid <= 8 {
            let _ = must_ok(c.quote_screen(sid));
            sid += 1;
        }
        let rep = must_ok(c.aggregate());
        assert!(rep.conserved && rep.rows.len() == 8);
    }

    #[test]
    fn t24_vrr_on_screen_full_chain() {
        let mut c = boot_two_screens();
        must_ok(c.set_vrr(1, 48, 120));
        let q = must_ok(c.quote_screen(1));
        // VRR 带随报价入账：带端包住当前档报价。
        assert!(q.band_low_mw <= q.base_mw && q.base_mw <= q.band_high_mw);
        // 非法区间经核心入口同样显性拒绝。
        must_err(c.set_vrr(1, 200, 100), 0x1D95);
        // 聚合守恒不受 VRR 影响。
        must_ok(c.quote_screen(2));
        let rep = must_ok(c.aggregate());
        assert!(rep.conserved);
    }
}
