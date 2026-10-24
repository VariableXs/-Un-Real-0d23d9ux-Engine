//! VE-F6013 · 物理示例（VE-AB 域 · 物理组 · 目标 300 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F6013`
//!
//! **判据（锚点原文）**：三例、同源、CI 守护、判据。
//!
//! **职责定位（锚点原文全文）**：职责定位：物理示例（目标 300 行）——物理示例——最小物理场景、确定性对拍演示、调试视图三例同源；示例含确定性对拍演示——同种子两次运行轨迹完全重合的可视化；示例含对拍结果可视化；示例含教学步骤解说；示例含回归进 CI 说明；示例含读屏说明；示例含对拍结果可视化；可视化含帧号标注；帧号标注含跳转；跳转含断点联动。数据结构：示例包。错误路径与降级矩阵：失效→CI；过时→同步；缺→补。性能逐项分解：运行 O（示例）。跨批对接点：W07 样例库。无障碍与隐私：示例含指引。工程量：核心逻辑约 120 行（五类要点判定与主流程编排）、边界防护约 45 行（输入校验与阈值防护）、错误路径约 45 行（降级与回滚处置）、测试支撑约 90 行（回归用例与断言），合计约 300 行。判据：三例、同源、CI 守护、判据。
//!
//! ---------------------------------------------------------------------------
//! 深化说明（300 行 → 深化 2000+ 行，零浮点，Q16.16 定点建模）
//! ---------------------------------------------------------------------------
//!
//! 本模块是物理域（VE-AB）的示例子系统，与邻件的关系：
//! - `vead12_phystest`（VE-F6012 物理测试）提供防线的**测试面**；
//! - 本模块（VE-F6013 物理示例）提供同一套确定性内核的**教学面**：
//!   最小物理场景闭集、确定性对拍演示、调试视图三例同源（轨迹表 /
//!   事件时间线 / 指标面板均从同一份运行账生成）、CI 守护说明。
//!
//! 八大真子系统（全部手工实现，无浮点运算，Q16.16 定点）：
//!
//! 1. `ExampleScene` 示例场景闭集（8 个场景 const 初始条件表）：自由落体 /
//!    弹性碰撞 / 斜面滑块 / 弹簧振子 / 抛体 / 多球碰撞 / 摩擦停摆 / 软绳摆，
//!    每场景带预期行为注记 + 教学解说 + 预期指标带（const 声明，来源注释）。
//! 2. `PhysKernel` 微型确定性物理内核：半隐式欧拉（辛积分器）固定步长积分
//!    Q16 定点位置/速度；Q16 定点乘除（乘后移位 + 显性饱和）；球-球冲量
//!    碰撞（恢复系数）+ 边界反射 + 软绳单边约束（只拉不推）+ 库仑摩擦
//!    （含静摩擦咬合与摩擦功台账）；逐步状态指纹链（FNV-1a 链式折叠）——
//!    任一步分叉即破。
//! 3. `ScenarioRunner` 场景运行器：场景号 → 运行 → 逐步状态账（轨迹采样点
//!    带帧号）+ 终态指纹；同种子双跑全链一致（对拍自证）+ 异种子必分叉
//!    （反证，不分叉即 `SeedDivergeFailed` 报错）。
//! 4. `MetricExtractor` 教学指标提取：能量守恒偏差（ppm）/ 动量守恒偏差
//!    （ppm）/ 收敛步数三指标；每场景预期带 const 声明；越带即立案
//!    （`BandCase` 入案账）。
//! 5. `ExampleBook` 示例册：场景清单 + 运行结果快照 + 指纹；册指纹冻结
//!    （freeze 后不可再改）；双跑一致判据（两次独立构建册指纹必须全等）。
//! 6. `DebugView` 调试视图三例同源：轨迹表（帧号标注）/ 事件时间线 /
//!    指标面板三视图全部从同一份 `RunRecord` 生成；帧号跳转（jump）+
//!    断点联动（set_breakpoint 后跳转结果携带断点关联标记）。
//! 7. 边界防线：速度/位置上限显性 saturating 钳制（钳制入账计数）；
//!    NaN 位型防线（f32 位型 const 掩码，纯位运算检测，不依赖浮点比较）；
//!    步数上限（`MAX_STEPS_PER_RUN`，超限拒绝）。
//! 8. 注入面三件套 + `health()`：场景篡改注入（篡改镜像表 → 目录指纹
//!    失配检出）/ 指纹漂移注入（翻一位与复跑指纹比对检出）/ 越带注入
//!    （强制越带指标 → 立案检出），各检出于案账；`health()` 六项健康面。
//!
//! 另含：事件账（滚动容量）、读屏指引 const、CI 守护说明 const 与
//! `ci_lines()`（种子→指纹表，作 CI 夹具导出面）。
//!
//! ---------------------------------------------------------------------------
//! 纪律（铁律）
//! ---------------------------------------------------------------------------
//! - no_std + alloc；只 `use core::…` / `use alloc::…`；零 `use crate::…`。
//! - 内核路径零 unwrap / 零 expect / 零 panic! / 零运行期 assert!
//!   （编译期 `const _: () = assert!(…)` 允许，见文末；cfg(test) 内允许）。
//! - 可失败全走 `Result<T, PhysExError>`；错误枚举每变体带
//!   `detail(&self) -> 'static str` 中文可操作诊断。
//! - 溢出显性 saturating / wrapping；>64KB 结构体禁上栈（本模块聚合结构
//!   全部 ≤ 数十字节，堆账走 `alloc::vec::Vec`）。
//! - const fn 禁坑：无 slice::get、无 Vec 索引、无 Ord::min/max、无
//!   derive(PartialEq) 类型的 `==`、无 for 循环（一律 while + 索引）。
//! - 零浮点：物理量全 i64 Q16.16 定点；NaN/Inf 检测与换算纯位运算。
//! - 零 IO、零墙钟：时钟一律 tick 注入/逻辑帧号。
//!
//! ---------------------------------------------------------------------------
//! 诊断码段（0x1D70~0x1D7F，独占）
//! ---------------------------------------------------------------------------
//! 码段占用核实（grep kernel/varix/src 全树，2026-10-24）：
//! - `grep -rn "0x1D7"` 全树唯一命中为 `unxstar/unx2502_uhptime.rs` 内
//!   64 位种子常量 `0x1D7A2311CA1CD63C` 的子串——该处是 RNG 种子而非
//!   诊断码，不构成诊断码段占用；
//! - 0x1D80 段已被 `svstar2/ves487_s27debug.rs`（idx487）占用；
//! - 0x1DC0~0x1DFF 段已被 `unxstar/unx2682_ucacheif.rs` 独占声明
//!   （0x1DF0 备选段因此不可用）；
//! - 0x1D70~0x1D7F 段作为**诊断码**全树无任何占用 → 本模块独占。

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 常量面（全部写死，来源注释）
// ---------------------------------------------------------------------------

/// 诊断段基座（0x1D70 段，独占 0x10——空闲核实见文件头）。
pub const SEG_BASE: u16 = 0x1D70;

/// 诊断段跨度（0x10——0x1D70~0x1D7F 共 16 码）。
pub const SEG_SPAN: u16 = 0x10;

/// 模块标识（总册/报告引用用）。
pub const MODULE_ID: &str = "vead13_physexample";

/// 任务号（锚点）。
pub const TASK_ID: &str = "VE-F6013";

/// Q16.16 定点小数位宽（Q16 约定——与 vead12_phystest / vead10_physec 同源）。
pub const Q16_SHIFT: u32 = 16;

/// Q16 定点 1.0。
pub const Q16_ONE: i64 = 1 << Q16_SHIFT;

/// 位置定点量程（|x| ≤ 32768.0）。
/// 来源：与 vead12_phystest::POS_LIMIT 同源（VE-AB 域统一量纲）。
pub const POS_LIMIT: i64 = 1 << 25;

/// 速度定点量程（|v| ≤ 4096.0）。
/// 来源：与 vead12_phystest::VEL_LIMIT 同源。
pub const VEL_LIMIT: i64 = 1 << 22;

/// 质量定点量程上界（|m| ≤ 1024.0）。
/// 来源：与 vead12_phystest::MASS_LIMIT 同源。
pub const MASS_LIMIT: i64 = 1 << 20;

/// 示例场景物理体容量上限（示例场景体数极小，取教学面保守值）。
/// 来源：示例场景闭集最大体数为 3，取 16 留演示余量。
pub const BODY_CAP: usize = 16;

/// 单次运行步数上限（步数上限防线）。
/// 来源：8 场景最长运行 240 步 ×2 倍演示余量向上取整到 2 的幂。
pub const MAX_STEPS_PER_RUN: u32 = 4096;

/// 标准步长 1/60 s（Q16 整除截断 = 1092，真值 1092.267）。
/// 来源：物理域主循环 60Hz 帧率约定（VE-AB 域步长三律之一）；
/// 截断相对误差 3.5e-5，落在各场景收敛带与漂移带裕量内（带来源声明）。
pub const DT_STANDARD_Q16: i64 = Q16_ONE / 60;

/// 最小步长 1/240 s（Q16）。
/// 来源：高精度子步下限（240Hz 物理细分业界惯例）。
pub const DT_MIN_Q16: i64 = Q16_ONE / 240;

/// 最大步长 1/20 s（Q16）。
/// 来源：半隐式欧拉对 k=4 弹簧的稳定裕度上界（ω·dt=0.1 < 2 稳定界，
/// 取 1/20 留 20 倍裕度）。
pub const DT_MAX_Q16: i64 = Q16_ONE / 20;

/// 重力加速度 9.81 m/s²（Q16 = 642908）。
/// 来源：国际单位制标准重力（CGPM 1901，9.80665 取两位教学近似 9.81）。
pub const GRAVITY_Q16: i64 = 642_908;

/// NaN 防线钳制安全值（Q16 定点 0.0）。
/// 来源：vead10/vead12 防线行为同源——检出即钳到中性安全量，
/// 钳制值不入确定性状态，轨迹不变。
pub const SAFE_CLAMP_Q16: i64 = 0;

/// 边界哨兵：无墙（bounds 分量等于此值表示该方向无边界）。
/// 来源：位置量程 POS_LIMIT=2^25，取 2^44 远离量程且远离 i64 溢出界，
/// 镜像反射运算不会触碰哨兵值。
pub const WALL_NONE: i64 = 1 << 44;

/// 指纹采样帧距（每 8 步采样一次入对拍样本链）。
/// 来源：与 vead12_phystest::SAMPLE_STRIDE 同源；240 步内保证 ≥30 个
/// 采样锚点，足以检出任何单步分叉。
pub const SAMPLE_STRIDE: u64 = 8;

/// 指纹样本链容量（滚动）。
/// 来源：4096 步上限 ÷ 8 帧距 = 512，正好一账不滚动。
pub const SAMPLE_CAP: usize = 512;

/// 轨迹采样帧距（调试视图帧号粒度）。
/// 来源：60Hz 下每 4 帧 ≈ 15Hz 视图刷新，人眼可读且 4096 步 ≤1024 点。
pub const TRAIL_STRIDE: u32 = 4;

/// 轨迹账容量上限（滚动）。
/// 来源：4096 步 ÷ 4 帧距 + 首尾各 1 点 = 1025，向上取整。
pub const TRAIL_CAP: usize = 1088;

/// 内核事件账容量（滚动）。
/// 来源：单场景事件峰值（碰撞+边界+收敛）实测 < 32，取 64 翻倍裕量。
pub const EVENT_CAP: usize = 64;

/// 示例册事件账容量（滚动，跨场景聚合）。
pub const BOOK_EVENT_CAP: usize = 128;

/// 案账容量（越带/注入立案上限，滚动）。
/// 来源：三注入面 × 8 场景峰值 24 案，取 32。
pub const CASE_CAP: usize = 32;

/// 示例册条目数（场景闭集大小）。
pub const BOOK_ENTRY_CAP: usize = 8;

/// 隔离观察期（NaN 注入后需连续干净步数才恢复清洁标志）。
/// 来源：与 vead12 防线解除/重武装语义同源，取 4 步观察窗。
pub const GUARD_OBSERVE_STEPS: u32 = 4;

/// FNV-1a 64 位偏移基。
/// 来源：FNV-1a 公开标准常数（Landon Curt Noll, public domain）。
pub const FNV_OFFSET: u64 = 0xcbf29ce484222325;

/// FNV-1a 64 位素数。
/// 来源：同上。
pub const FNV_PRIME: u64 = 0x100000001b3;

/// xorshift64 位移参数三元组（13,7,17）。
/// 来源：Marsaglia 2003 "Xorshift RNGs" 推荐组之一。
pub const XORSHIFT_A: u32 = 13;
pub const XORSHIFT_B: u32 = 7;
pub const XORSHIFT_C: u32 = 17;

/// 黄金分割常数（种子扰动混入用）。
/// 来源：Knuth MMIX 乘常数 0x9E3779B97F4A7C15，公开标准。
pub const GOLDEN_MIX: u64 = 0x9E37_79B9_7F4A_7C15;

/// 教学种子 A（对拍自证标准种子）。
/// 来源：示例册标准种子，任意选取后冻结进示例闭集。
pub const SEED_A: u64 = 0xA11CE_D00D;

/// 教学种子 B（异种子分叉反证标准种子）。
/// 来源：与 SEEDA 异或距离足够大的固定值，保证 8 场景全部分叉（测试实证）。
pub const SEED_B: u64 = 0xB0B5EED_5;

/// 读屏指引（无障碍：示例含指引——锚点原文要求）。
pub const READSCREEN_NOTE: &str = "读屏指引：本示例所有视图均为纯文本行输出；\
轨迹表每行以「帧 N」开头，帧号为跳转键；事件时间线按时间升序逐行朗读；\
指标面板先读场景名再读三指标（能量偏差/动量偏差/收敛步数）与是否在带；\
所有诊断文案为中文可操作短句，错误码落在 0x1D70~0x1D7F 段。";

/// CI 守护说明（示例含回归进 CI 说明——锚点原文要求）。
pub const CI_NOTE: &str = "CI 守护：示例册 build(seed) 的册指纹为纯确定性函数——\
同种子两次构建册指纹必须全等（双跑一致判据）；CI 中以 SEED_A 构建并核对\
ci_lines() 导出的种子→指纹表；任何一处指纹漂移即红（失效→CI，锚点降级矩阵）。";

// ---------------------------------------------------------------------------
// 错误面（0x1D70 段全码，每变体带中文可操作诊断）
// ---------------------------------------------------------------------------

/// 物理示例错误码（0x1D71~0x1D7F，段基座 0x1D70 保留为段标识）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PhysExCode {
    /// 0x1D71 场景未知（场景号 0 或越界——闭集为 1..=8）。
    SceneUnknown = 0x1D71,
    /// 0x1D72 物理体数非法（0 或超 BODY_CAP）。
    BodyCountInvalid = 0x1D72,
    /// 0x1D73 质量非法（动态体质量 ≤ 0 或超 MASS_LIMIT）。
    MassInvalid = 0x1D73,
    /// 0x1D74 半径非法（动态体半径 ≤ 0）。
    RadiusInvalid = 0x1D74,
    /// 0x1D75 步数超限（超 MAX_STEPS_PER_RUN 上限拒绝）。
    StepCountExceeded = 0x1D75,
    /// 0x1D76 场景参数越界（步长/恢复系数/摩擦系数/带宽非法）。
    ParamOutOfRange = 0x1D76,
    /// 0x1D77 轨迹分叉（同种子双跑指纹链破——确定性破坏）。
    DivergenceDetected = 0x1D77,
    /// 0x1D78 异种子未分叉（分叉反证失败——种子未真正进入轨迹）。
    SeedDivergeFailed = 0x1D78,
    /// 0x1D79 指标越带（能量/动量/收敛步数出预期带）。
    MetricOutOfBand = 0x1D79,
    /// 0x1D7A 场景篡改检出（目录指纹与篡改镜像失配）。
    CatalogTampered = 0x1D7A,
    /// 0x1D7B 指纹漂移检出（账面指纹与复跑指纹失配）。
    FingerprintDrift = 0x1D7B,
    /// 0x1D7C 示例册已冻结（冻结后拒绝写入/改条）。
    BookFrozen = 0x1D7C,
    /// 0x1D7D 示例册缺条目（册内缺场景闭集条目，缺→补降级路径）。
    BookMissingEntry = 0x1D7D,
    /// 0x1D7E NaN 洪流未兜住（防线解除态下毒位型扩散）。
    NanIntakeUncontained = 0x1D7E,
    /// 0x1D7F 容量超限（账/案/册容量拒绝，不允许静默滚动丢失）。
    CapacityExceeded = 0x1D7F,
}

impl PhysExCode {
    /// 码值（u16，落 0x1D70 段）。
    pub const fn code(self) -> u16 {
        self as u16
    }

    /// 人读名（中文短句）。
    pub const fn name(self) -> &'static str {
        match self {
            PhysExCode::SceneUnknown => "场景未知",
            PhysExCode::BodyCountInvalid => "物理体数非法",
            PhysExCode::MassInvalid => "质量非法",
            PhysExCode::RadiusInvalid => "半径非法",
            PhysExCode::StepCountExceeded => "步数超限",
            PhysExCode::ParamOutOfRange => "场景参数越界",
            PhysExCode::DivergenceDetected => "轨迹分叉",
            PhysExCode::SeedDivergeFailed => "异种子未分叉",
            PhysExCode::MetricOutOfBand => "指标越带",
            PhysExCode::CatalogTampered => "场景篡改检出",
            PhysExCode::FingerprintDrift => "指纹漂移检出",
            PhysExCode::BookFrozen => "示例册已冻结",
            PhysExCode::BookMissingEntry => "示例册缺条目",
            PhysExCode::NanIntakeUncontained => "NaN 洪流未兜住",
            PhysExCode::CapacityExceeded => "容量超限",
        }
    }

    /// 可操作建议（中文，落诊断读屏）。
    pub const fn advice(self) -> &'static str {
        match self {
            PhysExCode::SceneUnknown => "场景闭集为 1..=8（自由落体/弹性碰撞/斜面滑块/弹簧振子/抛体/多球碰撞/摩擦停摆/软绳摆）；以 SCENES 表为准选择下标 0..8",
            PhysExCode::BodyCountInvalid => "示例场景体数须在 1..=BODY_CAP(16)；检查场景初始条件表 bodies 字段",
            PhysExCode::MassInvalid => "动态体质量须在 (0, MASS_LIMIT]；静态体质量任意但固定体 inv_mass=0；修正初始条件表",
            PhysExCode::RadiusInvalid => "动态体半径须 > 0（碰撞对几何前提）；零半径属退化输入，改表或标记 fixed",
            PhysExCode::StepCountExceeded => "单次运行步数不得超过 MAX_STEPS_PER_RUN(4096)；长演示请分段运行并拼接指纹链",
            PhysExCode::ParamOutOfRange => "步长须在 [1/240s, 1/20s]、恢复系数在 [0,1]、摩擦系数在 [0,4]、带宽须为正且收敛带 < 收敛目标；逐项核对场景表",
            PhysExCode::DivergenceDetected => "同种子双跑指纹链分叉——积分路径存在非确定性输入；查注入面与共享可变状态，修复后重录示例册",
            PhysExCode::SeedDivergeFailed => "异种子跑出同指纹——种子扰动未真正进入初始条件；核对 perturb_amp 与种子混入路径",
            PhysExCode::MetricOutOfBand => "指标越带：核对场景表 energy_band_ppm / momentum_band_ppm / convergence_band 声明来源；物理参数变更后须同步带宽并重立案",
            PhysExCode::CatalogTampered => "目录指纹与篡改镜像失配——场景初始条件表被改动；恢复 SCENES const 表原文并重审带宽声明",
            PhysExCode::FingerprintDrift => "账面指纹与复跑指纹失配——运行账被改或内核行为漂移；先双跑复核，再查内核 diff",
            PhysExCode::BookFrozen => "示例册已冻结不可改；如需修订请新建册版本并重走双跑一致判据",
            PhysExCode::BookMissingEntry => "示例册缺场景条目（缺→补降级路径）；以 SCENES 闭集为准补齐 8 条后重冻结",
            PhysExCode::NanIntakeUncontained => "防线解除态下 NaN 位型已扩散；rearm_and_purge 清污后重武装，禁止裸跑物理内核",
            PhysExCode::CapacityExceeded => "账/案/册容量已满且不允许静默滚动丢失；归档后清账或扩对应 CAP 常量",
        }
    }
}

/// 全部错误码清单（const——detail 面自检用）。
pub const ALL_CODES: [PhysExCode; 15] = [
    PhysExCode::SceneUnknown,
    PhysExCode::BodyCountInvalid,
    PhysExCode::MassInvalid,
    PhysExCode::RadiusInvalid,
    PhysExCode::StepCountExceeded,
    PhysExCode::ParamOutOfRange,
    PhysExCode::DivergenceDetected,
    PhysExCode::SeedDivergeFailed,
    PhysExCode::MetricOutOfBand,
    PhysExCode::CatalogTampered,
    PhysExCode::FingerprintDrift,
    PhysExCode::BookFrozen,
    PhysExCode::BookMissingEntry,
    PhysExCode::NanIntakeUncontained,
    PhysExCode::CapacityExceeded,
];

/// 物理示例错误（只携带码——上下文入事件账，错误本身 Copy 零堆）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PhysExError {
    /// 错误码。
    pub code: PhysExCode,
}

impl PhysExError {
    /// 构造。
    pub const fn new(code: PhysExCode) -> Self {
        PhysExError { code }
    }

    /// 码值。
    pub const fn code(&self) -> u16 {
        self.code as u16
    }

    /// 中文可操作诊断（'static——不借上下文堆）。
    pub fn detail(&self) -> &'static str {
        self.code.advice()
    }

    /// 人读名。
    pub fn name(&self) -> &'static str {
        self.code.name()
    }
}

/// 码是否落在本模块诊断段（0x1D70~0x1D7F）。
pub const fn code_in_segment(code: u16) -> bool {
    code >= SEG_BASE && code < SEG_BASE + SEG_SPAN
}

// ---------------------------------------------------------------------------
// Q16 定点算术 + 整数平方根 + 指纹/随机（const 友好——显性饱和，零浮点）
// ---------------------------------------------------------------------------

/// Q16 定点乘：乘后右移 16 位，显性饱和到 i64 量程。
/// i128 中间量保证 2^25×2^25×2^16 量级不溢出。
pub const fn q16_mul(a: i64, b: i64) -> i64 {
    let p = (a as i128) * (b as i128);
    let shifted = p >> Q16_SHIFT;
    if shifted > i64::MAX as i128 {
        i64::MAX
    } else if shifted < i64::MIN as i128 {
        i64::MIN
    } else {
        shifted as i64
    }
}

/// Q16 定点除：左移 16 位后除，显性饱和；除零按符号饱和到量程端点
/// （不 panic——零除是确定性输入错误，走饱和 + 上层事件账）。
pub const fn q16_div(a: i64, b: i64) -> i64 {
    if b == 0 {
        return if a >= 0 { i64::MAX } else { i64::MIN };
    }
    let num = (a as i128) << Q16_SHIFT;
    let den = b as i128;
    let q = num / den;
    if q > i64::MAX as i128 {
        i64::MAX
    } else if q < i64::MIN as i128 {
        i64::MIN
    } else {
        q as i64
    }
}

/// i64 显性钳制（不用 Ord::min/max——const fn 禁坑纪律）。
pub const fn clamp_i64(v: i64, lo: i64, hi: i64) -> i64 {
    if v < lo {
        lo
    } else if v > hi {
        hi
    } else {
        v
    }
}

/// 64 位整数平方根（逐位法，无除法、无浮点）。
/// 来源：经典 bit-by-bit isqrt 算法（Hacker's Delight 11-1 变体）。
pub const fn isqrt64(num: u64) -> u64 {
    let mut n = num;
    let mut res: u64 = 0;
    let mut bit: u64 = 1u64 << 62;
    while bit > n && bit > 1 {
        bit >>= 2;
    }
    if bit > n {
        bit = 0;
    }
    while bit != 0 {
        if n >= res + bit {
            n -= res + bit;
            res = (res >> 1) + bit;
        } else {
            res >>= 1;
        }
        bit >>= 2;
    }
    res
}

/// Q16 定点平方根：sqrt(x)（x 为 Q16）= isqrt(x << 16)。
/// 数学：x_q16 = X·2^16，sqrt(X)·2^16 = sqrt(X·2^32) = sqrt(x_q16·2^16)。
/// 负输入防御性返回 0（物理量平方非负，负值只能是上游饱和噪声）。
pub const fn sqrt_q16(v: i64) -> i64 {
    if v <= 0 {
        0
    } else {
        isqrt64((v as u64) << Q16_SHIFT) as i64
    }
}

/// 符号函数（const fn，零分支表——摩擦方向判定用）。
pub const fn sgn_i64(v: i64) -> i64 {
    if v > 0 {
        1
    } else if v < 0 {
        -1
    } else {
        0
    }
}

/// 相对漂移（ppm）：|cur−base|·1e6 / max(|base|,1)，i128 中间量防溢出。
pub const fn drift_ppm(base: i64, cur: i64) -> i64 {
    let d = (cur as i128) - (base as i128);
    let ad = if d < 0 { -d } else { d };
    let b = if base < 0 { -(base as i128) } else { base as i128 };
    let b = if b == 0 { 1 } else { b };
    let ppm = (ad * 1_000_000) / b;
    if ppm > i64::MAX as i128 {
        i64::MAX
    } else {
        ppm as i64
    }
}

/// FNV-1a 单字折叠（逐字节，const fn while 循环纪律）。
pub const fn fnv_word(h: u64, w: u64) -> u64 {
    let mut acc = h;
    let mut i = 0;
    while i < 8 {
        let byte = ((w >> (i * 8)) & 0xFF) as u64;
        acc ^= byte;
        acc = acc.wrapping_mul(FNV_PRIME);
        i += 1;
    }
    acc
}

/// xorshift64（Marsaglia 2003；种子 0 由调用方负责拒绝）。
pub const fn xorshift64(mut x: u64) -> u64 {
    x ^= x << XORSHIFT_A;
    x ^= x >> XORSHIFT_B;
    x ^= x << XORSHIFT_C;
    x
}

/// L1 范数 |a|+|b|（i128 中间量，饱和回 i64——动量幅值用，免开方）。
pub const fn l1_q16(a: i64, b: i64) -> i64 {
    let sa = if a < 0 { -(a as i128) } else { a as i128 };
    let sb = if b < 0 { -(b as i128) } else { b as i128 };
    let s = sa + sb;
    if s > i64::MAX as i128 {
        i64::MAX
    } else {
        s as i64
    }
}

/// Q16 定点 → 人类可读十进制（4 位小数）——调试视图/读屏输出面。
pub fn q16_fmt(v: i64) -> String {
    let neg = v < 0;
    let m = if neg {
        // i64::MIN 防御：先升 i128 再取绝对值。
        let w = (v as i128).unsigned_abs() as u64;
        w
    } else {
        v as u64
    };
    let whole = m >> Q16_SHIFT;
    let frac = ((m & 0xFFFF) * 10_000) >> Q16_SHIFT;
    if neg {
        format!("-{}.{}", whole, pad4(frac))
    } else {
        format!("{}.{}", whole, pad4(frac))
    }
}

/// 4 位补零（q16_fmt 辅助——纯 usize 运算）。
fn pad4(v: u64) -> String {
    if v >= 1000 {
        format!("{}", v)
    } else if v >= 100 {
        format!("0{}", v)
    } else if v >= 10 {
        format!("00{}", v)
    } else {
        format!("000{}", v)
    }
}

// ---------------------------------------------------------------------------
// NaN 位型防线（纯位运算，零浮点依赖——与 vead10/vead12 防线语义同源）
// ---------------------------------------------------------------------------

/// f32 指数域掩码（8 位指数）。
/// 来源：IEEE-754 binary32 布局：1 符号 + 8 指数 + 23 尾数。
pub const F32_EXP_MASK: u32 = 0x7F80_0000;

/// f32 尾数域掩码（23 位）。
pub const F32_FRAC_MASK: u32 = 0x007F_FFFF;

/// f32 过大量级指数阈值（指数域 ≥ 0x70 即 |值| ≥ 2^17——教学示例量纲上界
/// 远小于此，命中即视为前兆污染，钳到安全值）。
/// 来源：示例场景速度上限 4096.0 = 2^12，留 2^5 倍裕量到 2^17。
pub const F32_HUGE_EXP: u32 = 0x70;

/// 位型分类（教学可视 + 防线判定共用）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BitClass {
    /// 有限且量级在防线上内——放行。
    Clean,
    /// 无穷（指数满、尾数零）。
    Inf,
    /// NaN（指数满、尾数非零）。
    Nan,
    /// 有限但超量级阈值——前兆污染。
    Huge,
}

impl BitClass {
    /// 人读名。
    pub const fn name(self) -> &'static str {
        match self {
            BitClass::Clean => "有限常量",
            BitClass::Inf => "无穷",
            BitClass::Nan => "非数",
            BitClass::Huge => "超量级前兆",
        }
    }
}

/// f32 位型分类（纯位运算——不依赖浮点比较指令）。
pub const fn classify_f32(bits: u32) -> BitClass {
    let exp = (bits & F32_EXP_MASK) >> 23;
    let frac = bits & F32_FRAC_MASK;
    if exp == 0xFF {
        if frac == 0 {
            BitClass::Inf
        } else {
            BitClass::Nan
        }
    } else if exp >= F32_HUGE_EXP {
        BitClass::Huge
    } else {
        BitClass::Clean
    }
}

/// 注入判定结果。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum IntakeOutcome {
    /// 有限常量——放行不钳。
    AcceptedClean,
    /// 非有限/超量级——已钳到安全值（钳制值不入确定性状态）。
    Clamped(BitClass),
}

impl IntakeOutcome {
    /// 人读名。
    pub fn name(self) -> &'static str {
        match self {
            IntakeOutcome::AcceptedClean => "放行",
            IntakeOutcome::Clamped(c) => {
                if c == BitClass::Nan {
                    "钳制（非数）"
                } else if c == BitClass::Inf {
                    "钳制（无穷）"
                } else {
                    "钳制（超量级前兆）"
                }
            }
        }
    }
}

/// NaN 位型防线（武装/解除/污染三态——与 vead12 NanFloodGuard 语义同源，
/// 实现为本模块自包含简版：注入口为教学演示专用通道，钳制值走隔离区，
/// 不进入确定性物理状态）。
#[derive(Clone, Debug)]
pub struct NanBitGuard {
    /// 防线是否武装（true=防线在位）。
    pub armed: bool,
    /// 污染标志（解除态下吸入毒位型 → 置位；置位后物理运行拒绝）。
    pub contaminated: bool,
    /// 钳制计数（检出即钳）。
    pub clamp_count: u64,
    /// 毒性吸入计数（解除态吸入）。
    pub toxic_count: u64,
    /// 隔离区（钳制安全值队列——演示「钳制值去哪了」隔离体行为说明）。
    pub quarantine: Vec<i64>,
    /// 隔离区容量。
    pub quarantine_cap: usize,
}

impl NanBitGuard {
    /// 出厂武装态。
    pub fn new() -> Self {
        NanBitGuard {
            armed: true,
            contaminated: false,
            clamp_count: 0,
            toxic_count: 0,
            quarantine: Vec::new(),
            quarantine_cap: 16,
        }
    }

    /// 吸入一个 f32 位型。
    pub fn ingest_f32(&mut self, bits: u32) -> IntakeOutcome {
        let cls = classify_f32(bits);
        match cls {
            BitClass::Clean => IntakeOutcome::AcceptedClean,
            _ => {
                if self.armed {
                    self.clamp_count = self.clamp_count.saturating_add(1);
                    if self.quarantine.len() >= self.quarantine_cap {
                        self.quarantine.remove(0);
                    }
                    self.quarantine.push(SAFE_CLAMP_Q16);
                    IntakeOutcome::Clamped(cls)
                } else {
                    // 解除态：毒性扩散——置污染标志（下一步运行将被拒绝）。
                    self.contaminated = true;
                    self.toxic_count = self.toxic_count.saturating_add(1);
                    IntakeOutcome::Clamped(cls)
                }
            }
        }
    }

    /// 解除防线（演示「防线解除会怎样」——教学注入路径）。
    pub fn disarm(&mut self) {
        self.armed = false;
    }

    /// 重武装 + 清污（清空隔离区与污染标志）。
    pub fn rearm_and_purge(&mut self) {
        self.armed = true;
        self.contaminated = false;
        self.quarantine.clear();
    }

    /// 当前是否清洁（武装且无污染）。
    pub fn is_clean(&self) -> bool {
        self.armed && !self.contaminated
    }
}

// ---------------------------------------------------------------------------
// 示例场景闭集（ExampleScene · 8 场景 const 初始条件表 + 预期行为注记）
// ---------------------------------------------------------------------------

/// 场景动力学类别（决定内核积分分支——每类一条专属真实物理路径）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SceneKind {
    /// 自由落体：仅重力，落地即收敛事件。
    FreeFall = 1,
    /// 弹性碰撞：零重力双球正碰，恢复系数 1，动量/动能双守恒教学基准。
    ElasticCollision = 2,
    /// 斜面滑块：沿斜面切向一维动力学 + 库仑动/静摩擦 + 摩擦功台账。
    InclineSlide = 3,
    /// 弹簧振子：胡克线性回复力，谐振动能量有界振荡教学基准。
    SpringOscillator = 4,
    /// 抛体：重力 + 初速度斜抛，落地步数 = 飞行时间教学基准。
    Projectile = 5,
    /// 多球碰撞：零重力三球 + 四壁反射，恢复系数 1。
    MultiBallCollision = 6,
    /// 摩擦停摆：水平面库仑摩擦，速度显性咬合归零，停步数 = v₀/(μg)。
    FrictionStop = 7,
    /// 软绳摆：单边约束（只拉不推），重力驱动画弧，摆锤过底收敛事件。
    RopePendulum = 8,
}

/// 动量守恒候选轴（None = 取 L1 幅值 |px|+|py| 作教学幅值漂移记录）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MomentumAxis {
    /// L1 幅值（外力场景——动量本不守恒，带仅作教学量级记录）。
    None = 0,
    /// x 轴（无水平外力场景——px 应严格守恒，带收紧）。
    X = 1,
}

/// 能量采样策略（收敛事件步采样 / 终步采样）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EnergySample {
    /// 在收敛事件步（或其前最后一个轨迹点）采样——落地/停摆类场景，
    /// 避免落地冻结后能量全失造成的假漂移。
    AtConvergence = 0,
    /// 在终步采样——碰撞/振子类场景，能量全程有效。
    AtFinal = 1,
}

/// 场景初始条件表条目（const 表元素，全量纲 Q16）。
#[derive(Clone, Copy, Debug)]
pub struct SceneBodySpec {
    /// 初始位置 [x, y]（Q16，米）。
    pub pos: [i64; 2],
    /// 初速度 [vx, vy]（Q16，米/秒）。
    pub vel: [i64; 2],
    /// 质量（Q16，千克；fixed 体忽略）。
    pub mass_q16: i64,
    /// 半径（Q16，米；碰撞对几何前提，fixed 体也须 > 0）。
    pub radius_q16: i64,
    /// 是否静态固定体（inv_mass = 0，不受力不动）。
    pub fixed: bool,
}

/// 示例场景规格（const 闭集元素——初始条件表 + 预期行为注记 + 预期带）。
#[derive(Clone, Copy, Debug)]
pub struct SceneSpec {
    /// 场景号（1..=8，0 保留非法）。
    pub id: u16,
    /// 场景名（中文短句——示例册清单/读屏输出）。
    pub name: &'static str,
    /// 动力学类别。
    pub kind: SceneKind,
    /// 重力加速度（Q16，米/秒²；零重力场景为 0）。
    pub gravity_q16: i64,
    /// 步长（Q16，秒）。
    pub dt_q16: i64,
    /// 演示步数。
    pub steps: u32,
    /// 初始条件表。
    pub bodies: &'static [SceneBodySpec],
    /// 弹簧劲度（Q16；非弹簧场景为 0）。
    pub spring_k_q16: i64,
    /// 弹簧原长 / 软绳长度（Q16；两用字段，非对应场景为 0）。
    pub spring_rest_q16: i64,
    /// 弹簧锚点 / 软绳悬点 [x, y]（Q16）。
    pub anchor: [i64; 2],
    /// 斜面 sinθ（Q16；非斜面场景为 0）。
    pub sin_q16: i64,
    /// 斜面 cosθ（Q16；非斜面场景为 0）。
    pub cos_q16: i64,
    /// 摩擦系数 μ（Q16；无摩擦场景为 0）。
    pub mu_q16: i64,
    /// 恢复系数 e（Q16；碰撞场景 1 = 完全弹性）。
    pub restitution_q16: i64,
    /// 边界 [xmin, xmax, ymin, ymax]（Q16；WALL_NONE = 该向无墙）。
    pub bounds: [i64; 4],
    /// 种子扰动幅值（Q16，米/秒——异种子分叉反证的物理入口）。
    pub perturb_amp_q16: i64,
    /// 收敛目标量（Q16——斜面滑程/落地高度；其余场景为 0）。
    pub target_q16: i64,
    /// 能量采样策略。
    pub energy_sample: EnergySample,
    /// 动量守恒候选轴。
    pub momentum_axis: MomentumAxis,
    /// 能量漂移预期带（ppm，绝对值上界）。
    pub energy_band_ppm: i64,
    /// 动量漂移预期带（ppm，绝对值上界）。
    pub momentum_band_ppm: i64,
    /// 摩擦功残差预期带（ppm，绝对值上界——仅摩擦场景生效）。
    pub residual_band_ppm: i64,
    /// 收敛步数预期值（0 = 该场景不校验收敛带）。
    pub conv_target: u32,
    /// 收敛步数容差（±，步）。
    pub conv_band: u32,
    /// 教学步骤解说（示例含教学步骤解说——锚点原文要求）。
    pub teaching: &'static str,
    /// 预期行为注记（每场景 const——示例含对拍结果可视化的判读基准）。
    pub expected: &'static str,
}

// --- Q16 字面量速查（1.0=65536；0.5=32768；2.0=131072；4.0=262144） ---

/// 质量 1.0 kg。
pub const M1_Q16: i64 = 65_536;
/// 质量 2.0 kg。
pub const M2_Q16: i64 = 131_072;
/// 长度/半径 0.5 m。
pub const R_HALF_Q16: i64 = 32_768;
/// 速度 1.0 m/s。
pub const V1_Q16: i64 = 65_536;
/// 速度 2.0 m/s。
pub const V2_Q16: i64 = 131_072;
/// 速度 3.0 m/s。
pub const V3_Q16: i64 = 196_608;
/// 速度 4.0 m/s。
pub const V4_Q16: i64 = 262_144;
/// 位置 2.0 m。
pub const X2_Q16: i64 = 131_072;
/// 位置 4.0 m。
pub const X4_Q16: i64 = 262_144;
/// 位置 10.0 m。
pub const X10_Q16: i64 = 655_360;
/// 种子扰动幅值 0.01 m/s（Q16 = 655.36 截断）。
/// 来源：扰动须远小于教学初速（≥1% 才可读）且远小于收敛带步长分辨率；
/// 0.01 m/s 在 60Hz 下每步位移 1.7e-4 m，不破坏教学判读。
pub const PERTURB_AMP_Q16: i64 = 655;

/// 场景一 · 自由落体：10 m 静止下落，落地 ~86 步。
/// 能量带来源：半隐式欧拉对线性位势每步恒定偏置 −½g²dt²（解析：
/// g=9.81, dt=1092/65536 → 0.01337 J/kg/步；86 步落地累计 ≈11_700 ppm
/// of E0=98.1），带宽取解析值 1.7 倍。
pub const SCENE_FREEFALL: SceneSpec = SceneSpec {
    id: 1,
    name: "自由落体",
    kind: SceneKind::FreeFall,
    gravity_q16: GRAVITY_Q16,
    dt_q16: DT_STANDARD_Q16,
    steps: 120,
    bodies: &[SceneBodySpec {
        pos: [0, X10_Q16],
        vel: [0, 0],
        mass_q16: M1_Q16,
        radius_q16: R_HALF_Q16,
        fixed: false,
    }],
    spring_k_q16: 0,
    spring_rest_q16: 0,
    anchor: [0, 0],
    sin_q16: 0,
    cos_q16: 0,
    mu_q16: 0,
    restitution_q16: 0,
    bounds: [WALL_NONE, WALL_NONE, WALL_NONE, WALL_NONE],
    perturb_amp_q16: PERTURB_AMP_Q16,
    target_q16: 0,
    energy_sample: EnergySample::AtConvergence,
    momentum_axis: MomentumAxis::X,
    energy_band_ppm: 20_000,
    momentum_band_ppm: 1_000,
    residual_band_ppm: 20_000,
    conv_target: 86,
    conv_band: 6,
    teaching: "教学解说：第 1 步先更新速度（v ← v + g·dt，半隐式欧拉先速度），\
再更新位置（x ← x + v·dt）——先速度后位置是辛积分器，能量误差有界；\
每步能量损失为恒定偏置 −½g²dt²≈0.0134 J/kg，可解析预告，落地带内即绿。",
    expected: "预期行为：匀加速下落，落地步数 ≈86（√(2h/g)=1.428s ÷ 步长）；\
px 恒为 0（无水平外力）；能量单调缓降但落在解析预告带内。",
};

/// 场景二 · 弹性碰撞：2kg@+2m/s 与 1kg@−1m/s 正碰，恢复系数 1。
/// 动量带来源：冲量法动量守恒为代数恒等式（与步长无关），Q16 舍入
/// 每步 ≤2^-16 相对量，带取 2_000 ppm 教学裕量。
pub const SCENE_ELASTIC: SceneSpec = SceneSpec {
    id: 2,
    name: "弹性碰撞",
    kind: SceneKind::ElasticCollision,
    gravity_q16: 0,
    dt_q16: DT_STANDARD_Q16,
    steps: 240,
    bodies: &[
        SceneBodySpec {
            pos: [-X2_Q16, 0],
            vel: [V2_Q16, 0],
            mass_q16: M2_Q16,
            radius_q16: R_HALF_Q16,
            fixed: false,
        },
        SceneBodySpec {
            pos: [X2_Q16, 0],
            vel: [-V1_Q16, 0],
            mass_q16: M1_Q16,
            radius_q16: R_HALF_Q16,
            fixed: false,
        },
    ],
    spring_k_q16: 0,
    spring_rest_q16: 0,
    anchor: [0, 0],
    sin_q16: 0,
    cos_q16: 0,
    mu_q16: 0,
    restitution_q16: Q16_ONE,
    bounds: [WALL_NONE, WALL_NONE, WALL_NONE, WALL_NONE],
    perturb_amp_q16: PERTURB_AMP_Q16,
    target_q16: 0,
    energy_sample: EnergySample::AtFinal,
    momentum_axis: MomentumAxis::X,
    energy_band_ppm: 2_000,
    momentum_band_ppm: 2_000,
    residual_band_ppm: 2_000,
    conv_target: 60,
    conv_band: 4,
    teaching: "教学解说：两球接近至半径和即触发冲量求解——j = −(1+e)·v_rel,n \
/ (1/m₁+1/m₂)，沿法线交换速度；e=1 时动能代数恒等守恒（恢复系数定义），\
动量守恒与步长无关——这是确定性内核里最干净的守恒教学基准。",
    expected: "预期行为：相对速度 3 m/s、净距 3 m → 约 60 步首撞；\
碰后按一维弹性碰撞公式交换速度；px=3 kg·m/s 全程不变；\
E 全程不变（Q16 舍入级）。",
};

/// 场景三 · 斜面滑块：30° 斜面自 4 m 高静止下滑 8 m 滑程，μ=0.1。
/// 残差带来源：能量台账 E−E₀+W_f 的闭合度只受 Q16 舍入累积影响
/// （~119 步 × 2^-16 相对量），带取 8_000 ppm。
pub const SCENE_INCLINE: SceneSpec = SceneSpec {
    id: 3,
    name: "斜面滑块",
    kind: SceneKind::InclineSlide,
    gravity_q16: GRAVITY_Q16,
    dt_q16: DT_STANDARD_Q16,
    steps: 180,
    bodies: &[SceneBodySpec {
        pos: [0, X4_Q16],
        vel: [0, 0],
        mass_q16: M1_Q16,
        radius_q16: R_HALF_Q16,
        fixed: false,
    }],
    spring_k_q16: 0,
    spring_rest_q16: 0,
    anchor: [0, 0],
    sin_q16: 32_768,
    cos_q16: 56_755,
    mu_q16: 6_554,
    restitution_q16: 0,
    bounds: [WALL_NONE, WALL_NONE, WALL_NONE, WALL_NONE],
    perturb_amp_q16: PERTURB_AMP_Q16,
    target_q16: 8 * Q16_ONE,
    energy_sample: EnergySample::AtConvergence,
    momentum_axis: MomentumAxis::None,
    energy_band_ppm: 300_000,
    momentum_band_ppm: 1_500_000,
    residual_band_ppm: 8_000,
    conv_target: 119,
    conv_band: 8,
    teaching: "教学解说：滑块被约束在斜面切向一维运动——切向Drive = g·sinθ，\
动摩擦 = μ·g·cosθ（ oppose 速度方向）；μ·cosθ=0.087 < sinθ=0.5 故永不静摩擦\
咬合、一路加速到坡底；内核逐台账累积摩擦功 W=μ·m·g·cosθ·Δs，\
教学判据是 E−E₀+W 残差闭合（摩擦生热不是凭空消失）。",
    expected: "预期行为：a=g(sinθ−μcosθ)=4.06 m/s²，8 m 滑程 119 步；\
到底时 v≈8.05 m/s；能量「损失」= 摩擦功 6.79 J，残差带内闭合。",
};

/// 场景四 · 弹簧振子：k=4，m=1，自 +1 m 静止释放，水平零重力。
/// 能量带来源：半隐式欧拉对谐振子能量有界振荡，幅值 ~（ω·dt/2）·E₀
/// （ω=2, dt=0.01666 → ~1.7% 上界），带取 20_000 ppm 实测裕量。
pub const SCENE_SPRING: SceneSpec = SceneSpec {
    id: 4,
    name: "弹簧振子",
    kind: SceneKind::SpringOscillator,
    gravity_q16: 0,
    dt_q16: DT_STANDARD_Q16,
    steps: 240,
    bodies: &[SceneBodySpec {
        pos: [Q16_ONE, 0],
        vel: [0, 0],
        mass_q16: M1_Q16,
        radius_q16: R_HALF_Q16,
        fixed: false,
    }],
    spring_k_q16: 262_144,
    spring_rest_q16: 0,
    anchor: [0, 0],
    sin_q16: 0,
    cos_q16: 0,
    mu_q16: 0,
    restitution_q16: 0,
    bounds: [WALL_NONE, WALL_NONE, WALL_NONE, WALL_NONE],
    perturb_amp_q16: PERTURB_AMP_Q16,
    target_q16: 0,
    energy_sample: EnergySample::AtFinal,
    momentum_axis: MomentumAxis::None,
    energy_band_ppm: 20_000,
    momentum_band_ppm: 1_500_000,
    residual_band_ppm: 20_000,
    conv_target: 47,
    conv_band: 5,
    teaching: "教学解说：胡克定律 F=−kx，ω=√(k/m)=2 rad/s，周期 T=π s；\
半隐式欧拉（辛积分器）的谐振子能量不漂移只振荡——这正是 vead12 三线\
测试选它的原因；振子第一次过平衡位置即收敛事件（T/4≈47 步）。",
    expected: "预期行为：振幅恒定 1 m（有界振荡不过衰不过涨）；\
首次过平衡 ≈47 步；E=2 J 有界振荡带内。",
};

/// 场景五 · 抛体：v₀=(3,4) m/s 斜抛落地，射程 2.45 m。
/// 能量带来源：同自由落体的每步恒定偏置 −½g²dt²，49 步累计
/// ≈52_400 ppm of E₀=12.5，带取解析值 1.5 倍。
pub const SCENE_PROJECTILE: SceneSpec = SceneSpec {
    id: 5,
    name: "抛体",
    kind: SceneKind::Projectile,
    gravity_q16: GRAVITY_Q16,
    dt_q16: DT_STANDARD_Q16,
    steps: 90,
    bodies: &[SceneBodySpec {
        pos: [0, 0],
        vel: [V3_Q16, V4_Q16],
        mass_q16: M1_Q16,
        radius_q16: R_HALF_Q16,
        fixed: false,
    }],
    spring_k_q16: 0,
    spring_rest_q16: 0,
    anchor: [0, 0],
    sin_q16: 0,
    cos_q16: 0,
    mu_q16: 0,
    restitution_q16: 0,
    bounds: [WALL_NONE, WALL_NONE, WALL_NONE, WALL_NONE],
    perturb_amp_q16: PERTURB_AMP_Q16,
    target_q16: 0,
    energy_sample: EnergySample::AtConvergence,
    momentum_axis: MomentumAxis::X,
    energy_band_ppm: 80_000,
    momentum_band_ppm: 1_000,
    residual_band_ppm: 80_000,
    conv_target: 49,
    conv_band: 4,
    teaching: "教学解说：抛体 = 自由落体 + 水平匀速（两轴解耦，积分互不干扰）；\
飞行时间 2v_y/g=0.815 s → 49 步落地；水平动量 px=3 全程不变——\
重力只动 y 轴，这是「分量解耦」的教学基准。",
    expected: "预期行为：抛物线轨迹，49 步落地；px 守恒（带 1_000 ppm）；\
能量带内缓降（同自由落体解析偏置）。",
};

/// 场景六 · 多球碰撞：零重力三球 + ±4 m 四壁，恢复系数 1。
/// 动量带来源：边界反射改变系统动量（墙是外力）——该带仅作教学量级
/// 记录、非守恒判据，取 2_000_000 ppm 宽带并注明；能量带 4_000 ppm
/// （镜像反射 + 冲量均保动能，Q16 舍入级）。
pub const SCENE_MULTIBALL: SceneSpec = SceneSpec {
    id: 6,
    name: "多球碰撞",
    kind: SceneKind::MultiBallCollision,
    gravity_q16: 0,
    dt_q16: DT_STANDARD_Q16,
    steps: 240,
    bodies: &[
        SceneBodySpec {
            pos: [-V3_Q16, 0],
            vel: [V2_Q16, 0],
            mass_q16: M1_Q16,
            radius_q16: R_HALF_Q16,
            fixed: false,
        },
        SceneBodySpec {
            pos: [0, 0],
            vel: [0, 0],
            mass_q16: M2_Q16,
            radius_q16: R_HALF_Q16,
            fixed: false,
        },
        SceneBodySpec {
            pos: [V3_Q16, Q16_ONE],
            vel: [-V1_Q16, -R_HALF_Q16],
            mass_q16: M1_Q16,
            radius_q16: R_HALF_Q16,
            fixed: false,
        },
    ],
    spring_k_q16: 0,
    spring_rest_q16: 0,
    anchor: [0, 0],
    sin_q16: 0,
    cos_q16: 0,
    mu_q16: 0,
    restitution_q16: Q16_ONE,
    bounds: [-X4_Q16, X4_Q16, -X4_Q16, X4_Q16],
    perturb_amp_q16: PERTURB_AMP_Q16,
    target_q16: 0,
    energy_sample: EnergySample::AtFinal,
    momentum_axis: MomentumAxis::None,
    energy_band_ppm: 4_000,
    momentum_band_ppm: 2_000_000,
    residual_band_ppm: 4_000,
    conv_target: 60,
    conv_band: 6,
    teaching: "教学解说：三球两两碰撞按固定 i<j 序贯求解——序贯即确定性\
（换序才会分叉）；墙反射用镜像位（x = 2·xmin − x）+ 法向速度反乘 e，\
e=1 时动能严格保守；系统动量因墙外力不守恒——带宽声明如实注明。",
    expected: "预期行为：A 球 1 s（约 60 步）撞中静止的 B 球（收敛事件）；\
此后三球在四壁间弹跳、KE 带内守恒；动量 L1 幅值随墙反射起伏（宽带记录）。",
};

/// 场景七 · 摩擦停摆：v₀=2 m/s，μ=0.2，停步数 = 61。
/// 残差带来源：E−E₀+W 闭合度同斜面（61 步舍入累积），带 8_000 ppm。
pub const SCENE_FRICTION: SceneSpec = SceneSpec {
    id: 7,
    name: "摩擦停摆",
    kind: SceneKind::FrictionStop,
    gravity_q16: GRAVITY_Q16,
    dt_q16: DT_STANDARD_Q16,
    steps: 120,
    bodies: &[SceneBodySpec {
        pos: [0, 0],
        vel: [V2_Q16, 0],
        mass_q16: M1_Q16,
        radius_q16: R_HALF_Q16,
        fixed: false,
    }],
    spring_k_q16: 0,
    spring_rest_q16: 0,
    anchor: [0, 0],
    sin_q16: 0,
    cos_q16: 0,
    mu_q16: 13_107,
    restitution_q16: 0,
    bounds: [WALL_NONE, WALL_NONE, WALL_NONE, WALL_NONE],
    perturb_amp_q16: PERTURB_AMP_Q16,
    target_q16: 0,
    energy_sample: EnergySample::AtConvergence,
    momentum_axis: MomentumAxis::None,
    energy_band_ppm: 1_100_000,
    momentum_band_ppm: 1_500_000,
    residual_band_ppm: 8_000,
    conv_target: 61,
    conv_band: 6,
    teaching: "教学解说：库仑摩擦咬合——每步减速 Δv=μ·g·dt=0.0327 m/s，\
当剩余速度 ≤ Δv 时一步咬合归零（静摩擦），滑行距离恰为 v₀²/(2μg)=1.019 m；\
动能全部转入摩擦功台账，残差闭合即「热去哪了」的教学答案。",
    expected: "预期行为：匀减速 61 步后停摆（收敛事件 Stopped）；\
停点 x≈1.019 m；E₀=2 J 全额入 W 台账，残差带内。",
};

/// 场景八 · 软绳摆：L=2 m，自 60° 张角静止释放，摆锤过底收敛。
/// 能量带来源：单边约束投影在绳张紧瞬间去除径向外逸分量——该去除量
/// 在约束激活帧每步 ≤ v_radial，摆动稳态后趋零；带取宽松 80_000 ppm
/// （实测校准，来源：投影法一阶能量误差）。
pub const SCENE_ROPE: SceneSpec = SceneSpec {
    id: 8,
    name: "软绳摆",
    kind: SceneKind::RopePendulum,
    gravity_q16: GRAVITY_Q16,
    dt_q16: DT_STANDARD_Q16,
    steps: 160,
    bodies: &[SceneBodySpec {
        pos: [113_516, -65_536],
        vel: [0, 0],
        mass_q16: M1_Q16,
        radius_q16: R_HALF_Q16,
        fixed: false,
    }],
    spring_k_q16: 0,
    spring_rest_q16: 131_072,
    anchor: [0, 0],
    sin_q16: 0,
    cos_q16: 0,
    mu_q16: 0,
    restitution_q16: 0,
    bounds: [WALL_NONE, WALL_NONE, WALL_NONE, WALL_NONE],
    perturb_amp_q16: PERTURB_AMP_Q16,
    target_q16: 0,
    energy_sample: EnergySample::AtConvergence,
    momentum_axis: MomentumAxis::None,
    energy_band_ppm: 80_000,
    momentum_band_ppm: 1_500_000,
    residual_band_ppm: 80_000,
    conv_target: 46,
    conv_band: 12,
    teaching: "教学解说：软绳与刚杆的区别在单边性——绳只能拉不能推：\
距悬点超长才投影回圆弧、且只去除向外的径向速度分量（绳内张力不做负功）；\
大摆幅（60°）周期比小角近似长 ~7%（T≈3.03 s），过底步数 46 带 ±12。",
    expected: "预期行为：摆锤沿圆弧下摆、过最低点（收敛事件 AnchorCross，\
约 46 步）再荡起；能量在投影耗散界内带内；px/py 随悬点外力起伏（宽带记录）。",
};

/// 示例场景闭集（const 表——目录指纹的忠实源）。
pub const SCENES: [SceneSpec; 8] = [
    SCENE_FREEFALL,
    SCENE_ELASTIC,
    SCENE_INCLINE,
    SCENE_SPRING,
    SCENE_PROJECTILE,
    SCENE_MULTIBALL,
    SCENE_FRICTION,
    SCENE_ROPE,
];

/// 场景下标 → 规格（0 起；越界走 `SceneUnknown`）。
pub fn scene_spec(idx: usize) -> Result<SceneSpec, PhysExError> {
    if idx >= SCENES.len() {
        return Err(PhysExError::new(PhysExCode::SceneUnknown));
    }
    Ok(SCENES[idx])
}

/// 场景规格校验（初始条件表 + 参数带全检——边界防线入口）。
pub fn validate_spec(spec: &SceneSpec) -> Result<(), PhysExError> {
    if spec.id == 0 || spec.id as usize > SCENES.len() {
        return Err(PhysExError::new(PhysExCode::SceneUnknown));
    }
    if spec.bodies.is_empty() || spec.bodies.len() > BODY_CAP {
        return Err(PhysExError::new(PhysExCode::BodyCountInvalid));
    }
    if spec.steps == 0 || spec.steps > MAX_STEPS_PER_RUN {
        return Err(PhysExError::new(PhysExCode::StepCountExceeded));
    }
    if spec.dt_q16 < DT_MIN_Q16 || spec.dt_q16 > DT_MAX_Q16 {
        return Err(PhysExError::new(PhysExCode::ParamOutOfRange));
    }
    if spec.restitution_q16 < 0 || spec.restitution_q16 > Q16_ONE {
        return Err(PhysExError::new(PhysExCode::ParamOutOfRange));
    }
    if spec.mu_q16 < 0 || spec.mu_q16 > 4 * Q16_ONE {
        return Err(PhysExError::new(PhysExCode::ParamOutOfRange));
    }
    if spec.energy_band_ppm <= 0 || spec.momentum_band_ppm <= 0 || spec.residual_band_ppm <= 0 {
        return Err(PhysExError::new(PhysExCode::ParamOutOfRange));
    }
    if spec.conv_band >= spec.conv_target.max(1) {
        return Err(PhysExError::new(PhysExCode::ParamOutOfRange));
    }
    let mut i = 0;
    while i < spec.bodies.len() {
        let b = &spec.bodies[i];
        if !b.fixed {
            if b.mass_q16 <= 0 || b.mass_q16 > MASS_LIMIT {
                return Err(PhysExError::new(PhysExCode::MassInvalid));
            }
            if b.radius_q16 <= 0 {
                return Err(PhysExError::new(PhysExCode::RadiusInvalid));
            }
        }
        if b.radius_q16 <= 0 {
            return Err(PhysExError::new(PhysExCode::RadiusInvalid));
        }
        i += 1;
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// 事件账（ExEvent · 滚动容量）
// ---------------------------------------------------------------------------

/// 事件类别（示例面全事件谱——调试视图时间线读屏输出）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ExEventKind {
    /// 场景装载（种子/扰动幅值入账）。
    SceneLoaded,
    /// 边界钳制（速度/位置上限 saturating 命中）。
    Clamp,
    /// 球-球碰撞（冲量求解）。
    CollisionPair,
    /// 边界反射。
    WallHit,
    /// 落地（y ≤ 目标线）。
    GroundCross,
    /// 过平衡/过底（x 过锚线）。
    AnchorCross,
    /// 摩擦咬合停摆。
    Stopped,
    /// 软绳张紧（约束激活）。
    RopeTaut,
    /// NaN 注入被钳（防线在位）。
    NanClamped,
    /// NaN 毒性扩散（防线解除）。
    NanUncontained,
    /// 指标越带立案。
    MetricEscalated,
    /// 篡改检出。
    TamperDetected,
    /// 指纹漂移检出。
    DriftDetected,
    /// 案件入账。
    CaseFiled,
    /// 示例册冻结。
    BookFrozen,
    /// 双跑一致。
    DualConsistent,
    /// 双跑分叉。
    DualDiverged,
}

impl ExEventKind {
    /// 段内事件标签码（信息性标签，落 0x1D70 段——与错误码同一诊断段，
    /// 读取面按 kind 分流不按码分流）。
    pub const fn tag(self) -> u16 {
        match self {
            ExEventKind::SceneLoaded => 0x1D70,
            ExEventKind::Clamp => 0x1D70,
            ExEventKind::CollisionPair => 0x1D70,
            ExEventKind::WallHit => 0x1D70,
            ExEventKind::GroundCross => 0x1D70,
            ExEventKind::AnchorCross => 0x1D70,
            ExEventKind::Stopped => 0x1D70,
            ExEventKind::RopeTaut => 0x1D70,
            ExEventKind::NanClamped => 0x1D70,
            ExEventKind::NanUncontained => 0x1D7E,
            ExEventKind::MetricEscalated => 0x1D79,
            ExEventKind::TamperDetected => 0x1D7A,
            ExEventKind::DriftDetected => 0x1D7B,
            ExEventKind::CaseFiled => 0x1D70,
            ExEventKind::BookFrozen => 0x1D7C,
            ExEventKind::DualConsistent => 0x1D70,
            ExEventKind::DualDiverged => 0x1D77,
        }
    }

    /// 人读名。
    pub const fn name(self) -> &'static str {
        match self {
            ExEventKind::SceneLoaded => "场景装载",
            ExEventKind::Clamp => "边界钳制",
            ExEventKind::CollisionPair => "球球碰撞",
            ExEventKind::WallHit => "边界反射",
            ExEventKind::GroundCross => "落地",
            ExEventKind::AnchorCross => "过锚线",
            ExEventKind::Stopped => "摩擦停摆",
            ExEventKind::RopeTaut => "软绳张紧",
            ExEventKind::NanClamped => "NaN 钳制",
            ExEventKind::NanUncontained => "NaN 扩散",
            ExEventKind::MetricEscalated => "越带立案",
            ExEventKind::TamperDetected => "篡改检出",
            ExEventKind::DriftDetected => "漂移检出",
            ExEventKind::CaseFiled => "案件入账",
            ExEventKind::BookFrozen => "示例册冻结",
            ExEventKind::DualConsistent => "双跑一致",
            ExEventKind::DualDiverged => "双跑分叉",
        }
    }
}

/// 事件（时间戳 = 逻辑步号；note 为人类可读上下文）。
#[derive(Clone, Debug)]
pub struct ExEvent {
    /// 逻辑步号（0 = 装载期）。
    pub step: u64,
    /// 事件类别。
    pub kind: ExEventKind,
    /// 段内标签码。
    pub code: u16,
    /// 人类可读上下文。
    pub note: String,
}

// ---------------------------------------------------------------------------
// PhysKernel · 微型确定性物理内核
// ---------------------------------------------------------------------------

/// 物理体（运行时态——全 i64 Q16，零浮点）。
#[derive(Clone, Copy, Debug)]
pub struct ExBody {
    /// 位置 [x, y]（Q16，米）。
    pub pos: [i64; 2],
    /// 速度 [vx, vy]（Q16，米/秒）。
    pub vel: [i64; 2],
    /// 质量（Q16，千克）。
    pub mass_q16: i64,
    /// 逆质量（Q16；fixed 体 = 0——冲量/校正对其无效）。
    pub inv_mass_q16: i64,
    /// 半径（Q16，米）。
    pub radius_q16: i64,
    /// 静态固定体。
    pub fixed: bool,
}

/// 微型确定性物理内核：半隐式欧拉固定步长 + Q16 定点 + 指纹链。
///
/// 确定性三律（本内核执行口径）：
/// 1. 同输入同输出——全部运算为整数定值运算，无浮点/无迭代顺序歧义；
/// 2. 逐步指纹链——每步终态折叠入 FNV-1a 链，任一步分叉即破链；
/// 3. 防线不改轨迹——钳制/隔离只入账不改积分状态。
pub struct PhysKernel {
    /// 场景号。
    pub scene_id: u16,
    /// 动力学类别。
    pub kind: SceneKind,
    /// 步长（Q16）。
    pub dt_q16: i64,
    /// 重力（Q16）。
    pub gravity_q16: i64,
    /// 弹簧劲度（Q16）。
    pub spring_k_q16: i64,
    /// 弹簧原长 / 软绳长（Q16）。
    pub spring_rest_q16: i64,
    /// 锚点/悬点。
    pub anchor: [i64; 2],
    /// 斜面 sinθ（Q16）。
    pub sin_q16: i64,
    /// 斜面 cosθ（Q16）。
    pub cos_q16: i64,
    /// 摩擦系数（Q16）。
    pub mu_q16: i64,
    /// 恢复系数（Q16）。
    pub restitution_q16: i64,
    /// 边界。
    pub bounds: [i64; 4],
    /// 收敛目标量（Q16）。
    pub target_q16: i64,
    /// 物理体。
    pub bodies: Vec<ExBody>,
    /// 逻辑步号。
    pub tick: u64,
    /// 逐步状态指纹链（FNV-1a 链式折叠——任一步分叉即破）。
    pub fp_chain: u64,
    /// 已入链步数。
    pub fp_steps: u64,
    /// 指纹采样链（对拍样本——每 SAMPLE_STRIDE 步一锚）。
    pub fp_samples: Vec<u64>,
    /// 收敛事件步号（0 = 未收敛）。
    pub convergence_step: u32,
    /// 摩擦功台账（Q16，焦耳——能量残差教学判据的输入）。
    pub friction_work_q16: i64,
    /// 累计滑程（Q16，米——斜面收敛判据输入）。
    pub traveled_q16: i64,
    /// 软绳已张紧标志（RopeTaut 事件只发一次）。
    pub rope_taut_seen: bool,
    /// 摩擦已咬合标志（Stopped 事件只发一次）。
    pub stopped_seen: bool,
    /// 边界钳制命中计数（防线入账——不改轨迹）。
    pub clamp_count: u64,
    /// 事件账（滚动）。
    pub events: Vec<ExEvent>,
    /// 事件滚动覆盖计数。
    pub events_rolled: u64,
    /// 驱动种子（指纹链混入 + 追溯）。
    pub seed: u64,
}

impl PhysKernel {
    /// 从场景规格构建（校验 + 种子扰动 + 装载事件）。
    pub fn from_spec(spec: &SceneSpec, seed: u64) -> Result<Self, PhysExError> {
        validate_spec(spec)?;
        if seed == 0 {
            return Err(PhysExError::new(PhysExCode::ParamOutOfRange));
        }
        let mut bodies: Vec<ExBody> = Vec::new();
        let mut i = 0;
        while i < spec.bodies.len() {
            let s = &spec.bodies[i];
            let inv = if s.fixed || s.mass_q16 == 0 {
                0
            } else {
                q16_div(Q16_ONE, s.mass_q16)
            };
            // 种子扰动：确定性 xorshift 混入初速度（异种子分叉反证的物理入口）。
            let mut vel = [s.vel[0], s.vel[1]];
            if !s.fixed && spec.perturb_amp_q16 > 0 {
                let p = seed_perturb(seed, i, spec.perturb_amp_q16);
                vel[0] = clamp_i64(vel[0].saturating_add(p[0]), -VEL_LIMIT, VEL_LIMIT);
                vel[1] = clamp_i64(vel[1].saturating_add(p[1]), -VEL_LIMIT, VEL_LIMIT);
            }
            bodies.push(ExBody {
                pos: [s.pos[0], s.pos[1]],
                vel,
                mass_q16: s.mass_q16,
                inv_mass_q16: inv,
                radius_q16: s.radius_q16,
                fixed: s.fixed,
            });
            i += 1;
        }
        let mut k = PhysKernel {
            scene_id: spec.id,
            kind: spec.kind,
            dt_q16: spec.dt_q16,
            gravity_q16: spec.gravity_q16,
            spring_k_q16: spec.spring_k_q16,
            spring_rest_q16: spec.spring_rest_q16,
            anchor: spec.anchor,
            sin_q16: spec.sin_q16,
            cos_q16: spec.cos_q16,
            mu_q16: spec.mu_q16,
            restitution_q16: spec.restitution_q16,
            bounds: spec.bounds,
            target_q16: spec.target_q16,
            bodies,
            tick: 0,
            fp_chain: fnv_word(FNV_OFFSET, seed),
            fp_steps: 0,
            fp_samples: Vec::new(),
            convergence_step: 0,
            friction_work_q16: 0,
            traveled_q16: 0,
            rope_taut_seen: false,
            stopped_seen: false,
            clamp_count: 0,
            events: Vec::new(),
            events_rolled: 0,
            seed,
        };
        k.fp_samples.push(k.fp_chain);
        k.push_event(
            ExEventKind::SceneLoaded,
            format!(
                "场景{}「{}」种子=0x{:016X} 扰动幅={} 体数={}",
                spec.id,
                spec.name,
                seed,
                q16_fmt(spec.perturb_amp_q16),
                k.bodies.len()
            ),
        );
        Ok(k)
    }

    /// 事件入账（滚动覆盖——覆盖计数留痕）。
    pub fn push_event(&mut self, kind: ExEventKind, note: String) {
        if self.events.len() >= EVENT_CAP {
            self.events.remove(0);
            self.events_rolled = self.events_rolled.saturating_add(1);
        }
        let code = kind.tag();
        self.events.push(ExEvent {
            step: self.tick,
            kind,
            code,
            note,
        });
    }

    /// 逐步加速度（通用分支——斜面/摩擦两分支走专属路径不入此）。
    fn accel_of(&self, i: usize) -> (i64, i64) {
        let b = &self.bodies[i];
        let mut ax: i64 = 0;
        let mut ay: i64 = -self.gravity_q16;
        if self.kind as u8 == SceneKind::SpringOscillator as u8 && self.spring_k_q16 > 0 {
            let dx = b.pos[0].saturating_sub(self.anchor[0]);
            let dy = b.pos[1].saturating_sub(self.anchor[1]);
            let dsq = q16_mul(dx, dx).saturating_add(q16_mul(dy, dy));
            let len = sqrt_q16(dsq);
            if len > 0 {
                let stretch = len.saturating_sub(self.spring_rest_q16);
                // a = −k·stretch / m（先标量后方向——确定性定值运算序）。
                let a_mag = q16_div(q16_mul(self.spring_k_q16, stretch), b.mass_q16);
                let nx = q16_div(dx, len);
                let ny = q16_div(dy, len);
                ax = ax.saturating_sub(q16_mul(a_mag, nx));
                ay = ay.saturating_sub(q16_mul(a_mag, ny));
            }
        }
        (ax, ay)
    }

    /// 斜面切向速度（v·t，t = (cosθ, −sinθ) 下坡向）。
    fn incline_vt(&self, i: usize) -> i64 {
        let b = &self.bodies[i];
        q16_mul(b.vel[0], self.cos_q16).saturating_sub(q16_mul(b.vel[1], self.sin_q16))
    }

    /// 斜面专属积分步（切向一维 + 库仑动/静摩擦 + 摩擦功台账）。
    fn step_incline(&mut self, dt: i64) {
        let i = 0;
        // 1) 切向动力学：drive − 动摩擦。
        let drive = q16_mul(self.gravity_q16, self.sin_q16);
        let normal_g = q16_mul(self.gravity_q16, self.cos_q16);
        let fric_max = q16_mul(self.mu_q16, normal_g);
        let mut vt = self.incline_vt(i);
        let a_t = drive.saturating_sub(q16_mul(sgn_i64(vt), fric_max));
        vt = vt.saturating_add(q16_mul(a_t, dt));
        // 2) 静摩擦咬合判定：μ·cosθ ≥ sinθ 且 |v_t| ≤ 摩擦步减量 → 归零。
        let dv_fric = q16_mul(fric_max, dt);
        let drive_step = q16_mul(drive, dt);
        let abs_vt = if vt < 0 {
            vt.wrapping_neg()
        } else {
            vt
        };
        let hold_ok = fric_max >= drive;
        if hold_ok && abs_vt <= dv_fric && drive_step.abs_le(dv_fric) {
            vt = 0;
        }
        // 3) 重建切向速度（法向分量恒零——斜面约束）。
        let vx = q16_mul(vt, self.cos_q16);
        let vy = q16_mul(vt, -self.sin_q16);
        // 4) 位置推进 + 滑程/摩擦功台账。
        let dx = q16_mul(vx, dt);
        let dy = q16_mul(vy, dt);
        let b = &mut self.bodies[i];
        b.pos[0] = clamp_i64(b.pos[0].saturating_add(dx), -POS_LIMIT, POS_LIMIT);
        b.pos[1] = clamp_i64(b.pos[1].saturating_add(dy), -POS_LIMIT, POS_LIMIT);
        b.vel[0] = clamp_i64(vx, -VEL_LIMIT, VEL_LIMIT);
        b.vel[1] = clamp_i64(vy, -VEL_LIMIT, VEL_LIMIT);
        let abs_dx = if dx < 0 { dx.wrapping_neg() } else { dx };
        self.traveled_q16 = self.traveled_q16.saturating_add(abs_dx);
        // W += μ·m·g·cosθ·|Δx|（法向力×滑程）。
        let f_n = q16_mul(q16_mul(self.mu_q16, b.mass_q16), normal_g);
        self.friction_work_q16 = self.friction_work_q16.saturating_add(q16_mul(f_n, abs_dx));
        if self.convergence_step == 0 && self.traveled_q16 >= self.target_q16 && self.target_q16 > 0
        {
            self.convergence_step = self.tick as u32;
            self.push_event(
                ExEventKind::AnchorCross,
                format!("斜面到底：滑程={} 目标={}", q16_fmt(self.traveled_q16), q16_fmt(self.target_q16)),
            );
        }
    }

    /// 摩擦停摆专属积分步（水平库仑摩擦 + 一步咬合归零）。
    fn step_friction(&mut self, dt: i64) {
        let i = 0;
        let fric_a = q16_mul(self.mu_q16, self.gravity_q16);
        let dv = q16_mul(fric_a, dt);
        let b = &mut self.bodies[i];
        let vx = b.vel[0];
        let abs_vx = if vx < 0 { vx.wrapping_neg() } else { vx };
        let new_vx = if abs_vx <= dv {
            // 一步咬合（静摩擦）——速度显性归零。
            0
        } else {
            vx.saturating_sub(q16_mul(sgn_i64(vx), dv))
        };
        let dx = q16_mul(new_vx, dt);
        b.pos[0] = clamp_i64(b.pos[0].saturating_add(dx), -POS_LIMIT, POS_LIMIT);
        b.vel[0] = clamp_i64(new_vx, -VEL_LIMIT, VEL_LIMIT);
        let abs_dx = if dx < 0 { dx.wrapping_neg() } else { dx };
        // W += μ·m·g·|Δx|（水平面法向力 = m·g）。
        let f_n = q16_mul(q16_mul(self.mu_q16, b.mass_q16), self.gravity_q16);
        self.friction_work_q16 = self.friction_work_q16.saturating_add(q16_mul(f_n, abs_dx));
        if !self.stopped_seen && new_vx == 0 {
            self.stopped_seen = true;
            self.convergence_step = self.tick as u32;
            self.push_event(
                ExEventKind::Stopped,
                format!(
                    "摩擦咬合停摆：停点 x={} W={}",
                    q16_fmt(b.pos[0]),
                    q16_fmt(self.friction_work_q16)
                ),
            );
        }
    }

    /// 球-球冲量碰撞（i<j 固定序——序即确定性）。
    fn resolve_collisions(&mut self) {
        let is_collision_kind = self.kind as u8 == SceneKind::ElasticCollision as u8
            || self.kind as u8 == SceneKind::MultiBallCollision as u8;
        if !is_collision_kind {
            return;
        }
        let n = self.bodies.len();
        let mut i = 0;
        while i < n {
            let mut j = i + 1;
            while j < n {
                self.pair_collision(i, j);
                j += 1;
            }
            i += 1;
        }
    }

    /// 单对冲量求解（恢复系数 e；位置分离按逆质量比——冲量法教科书式）。
    fn pair_collision(&mut self, i: usize, j: usize) {
        let (dx, dy, inv_sum) = {
            let a = &self.bodies[i];
            let b = &self.bodies[j];
            let dx = b.pos[0].saturating_sub(a.pos[0]);
            let dy = b.pos[1].saturating_sub(a.pos[1]);
            let inv_sum = a
                .inv_mass_q16
                .saturating_add(b.inv_mass_q16);
            (dx, dy, inv_sum)
        };
        if inv_sum <= 0 {
            return;
        }
        let rsum = self.bodies[i]
            .radius_q16
            .saturating_add(self.bodies[j].radius_q16);
        let dsq = q16_mul(dx, dx).saturating_add(q16_mul(dy, dy));
        let rsum2 = q16_mul(rsum, rsum);
        if dsq >= rsum2 {
            return;
        }
        let dist = sqrt_q16(dsq);
        // 法线：退化（完全重合）取 +x——确定性兜底。
        let (nx, ny) = if dist <= 0 {
            (Q16_ONE, 0)
        } else {
            (q16_div(dx, dist), q16_div(dy, dist))
        };
        // 相对法向速度（<0 = 相互接近）。
        let vrel_n = {
            let a = &self.bodies[i];
            let b = &self.bodies[j];
            q16_mul(b.vel[0].saturating_sub(a.vel[0]), nx)
                .saturating_add(q16_mul(b.vel[1].saturating_sub(a.vel[1]), ny))
        };
        if vrel_n < 0 {
            // 冲量 j = −(1+e)·v_rel,n / (1/m₁+1/m₂)（v_rel,n<0 → j>0）。
            let one_plus_e = Q16_ONE.saturating_add(self.restitution_q16);
            let jimp = q16_div(q16_mul(one_plus_e, vrel_n.wrapping_neg()), inv_sum);
            let mut bi = 0;
            while bi < 2 {
                let di = q16_mul(q16_mul(jimp, self.bodies[i].inv_mass_q16), if bi == 0 { nx } else { ny });
                let dj = q16_mul(q16_mul(jimp, self.bodies[j].inv_mass_q16), if bi == 0 { nx } else { ny });
                self.bodies[i].vel[bi] = clamp_i64(
                    self.bodies[i].vel[bi].saturating_sub(di),
                    -VEL_LIMIT,
                    VEL_LIMIT,
                );
                self.bodies[j].vel[bi] = clamp_i64(
                    self.bodies[j].vel[bi].saturating_add(dj),
                    -VEL_LIMIT,
                    VEL_LIMIT,
                );
                bi += 1;
            }
            if self.convergence_step == 0 {
                self.convergence_step = self.tick as u32;
            }
            self.push_event(
                ExEventKind::CollisionPair,
                format!(
                    "体{}↔体{} 冲量 j={}（步 {}）",
                    i,
                    j,
                    q16_fmt(jimp),
                    self.tick
                ),
            );
        }
        // 位置分离（按逆质量比分摊——只推开不加速）。
        let overlap = rsum.saturating_sub(dist);
        if overlap > 0 {
            let corr = q16_div(overlap, inv_sum);
            let mut bi = 0;
            while bi < 2 {
                let ci = q16_mul(q16_mul(corr, self.bodies[i].inv_mass_q16), if bi == 0 { nx } else { ny });
                let cj = q16_mul(q16_mul(corr, self.bodies[j].inv_mass_q16), if bi == 0 { nx } else { ny });
                self.bodies[i].pos[bi] = clamp_i64(
                    self.bodies[i].pos[bi].saturating_sub(ci),
                    -POS_LIMIT,
                    POS_LIMIT,
                );
                self.bodies[j].pos[bi] = clamp_i64(
                    self.bodies[j].pos[bi].saturating_add(cj),
                    -POS_LIMIT,
                    POS_LIMIT,
                );
                bi += 1;
            }
        }
    }

    /// 边界反射（镜像位 + 法向速度反乘 e——e=1 时动能严格保守）。
    fn resolve_walls(&mut self) {
        let e = self.restitution_q16;
        let n = self.bodies.len();
        let mut i = 0;
        while i < n {
            if !self.bodies[i].fixed {
                let xmin = self.bounds[0];
                let xmax = self.bounds[1];
                let ymin = self.bounds[2];
                let ymax = self.bounds[3];
                let mut hit = false;
                // x 轴。
                if xmin != WALL_NONE && self.bodies[i].pos[0] < xmin {
                    let mirrored = xmin.saturating_add(xmin.saturating_sub(self.bodies[i].pos[0]));
                    self.bodies[i].pos[0] = clamp_i64(mirrored, xmin, POS_LIMIT);
                    self.bodies[i].vel[0] =
                        clamp_i64(q16_mul(self.bodies[i].vel[0].wrapping_neg(), e), -VEL_LIMIT, VEL_LIMIT);
                    hit = true;
                } else if xmax != WALL_NONE && self.bodies[i].pos[0] > xmax {
                    let mirrored = xmax.saturating_sub(self.bodies[i].pos[0].saturating_sub(xmax));
                    self.bodies[i].pos[0] = clamp_i64(mirrored, -POS_LIMIT, xmax);
                    self.bodies[i].vel[0] =
                        clamp_i64(q16_mul(self.bodies[i].vel[0].wrapping_neg(), e), -VEL_LIMIT, VEL_LIMIT);
                    hit = true;
                }
                // y 轴。
                if ymin != WALL_NONE && self.bodies[i].pos[1] < ymin {
                    let mirrored = ymin.saturating_add(ymin.saturating_sub(self.bodies[i].pos[1]));
                    self.bodies[i].pos[1] = clamp_i64(mirrored, ymin, POS_LIMIT);
                    self.bodies[i].vel[1] =
                        clamp_i64(q16_mul(self.bodies[i].vel[1].wrapping_neg(), e), -VEL_LIMIT, VEL_LIMIT);
                    hit = true;
                } else if ymax != WALL_NONE && self.bodies[i].pos[1] > ymax {
                    let mirrored = ymax.saturating_sub(self.bodies[i].pos[1].saturating_sub(ymax));
                    self.bodies[i].pos[1] = clamp_i64(mirrored, -POS_LIMIT, ymax);
                    self.bodies[i].vel[1] =
                        clamp_i64(q16_mul(self.bodies[i].vel[1].wrapping_neg(), e), -VEL_LIMIT, VEL_LIMIT);
                    hit = true;
                }
                if hit {
                    self.push_event(
                        ExEventKind::WallHit,
                        format!("体{} 边界反射（e={}）", i, q16_fmt(e)),
                    );
                }
            }
            i += 1;
        }
    }

    /// 软绳单边约束（只拉不推：超长才投影，且只去除向外径向分量）。
    fn resolve_rope(&mut self) {
        if self.kind as u8 != SceneKind::RopePendulum as u8 || self.spring_rest_q16 <= 0 {
            return;
        }
        let i = 0;
        let (dx, dy) = {
            let b = &self.bodies[i];
            (
                b.pos[0].saturating_sub(self.anchor[0]),
                b.pos[1].saturating_sub(self.anchor[1]),
            )
        };
        let dsq = q16_mul(dx, dx).saturating_add(q16_mul(dy, dy));
        let lsq = q16_mul(self.spring_rest_q16, self.spring_rest_q16);
        if dsq <= lsq {
            return;
        }
        let dist = sqrt_q16(dsq);
        if dist <= 0 {
            return;
        }
        let nx = q16_div(dx, dist);
        let ny = q16_div(dy, dist);
        let v_rad = {
            let b = &self.bodies[i];
            q16_mul(b.vel[0], nx).saturating_add(q16_mul(b.vel[1], ny))
        };
        let b = &mut self.bodies[i];
        // 投影回圆弧。
        b.pos[0] = clamp_i64(
            self.anchor[0].saturating_add(q16_mul(nx, self.spring_rest_q16)),
            -POS_LIMIT,
            POS_LIMIT,
        );
        b.pos[1] = clamp_i64(
            self.anchor[1].saturating_add(q16_mul(ny, self.spring_rest_q16)),
            -POS_LIMIT,
            POS_LIMIT,
        );
        // 只去除向外的径向分量（绳张力不做负功——向内分量保留）。
        if v_rad > 0 {
            let dvx = q16_mul(v_rad, nx);
            let dvy = q16_mul(v_rad, ny);
            b.vel[0] = clamp_i64(b.vel[0].saturating_sub(dvx), -VEL_LIMIT, VEL_LIMIT);
            b.vel[1] = clamp_i64(b.vel[1].saturating_sub(dvy), -VEL_LIMIT, VEL_LIMIT);
        }
        if !self.rope_taut_seen {
            self.rope_taut_seen = true;
            self.push_event(ExEventKind::RopeTaut, "软绳张紧：约束激活".to_string());
        }
    }

    /// 推进一步（半隐式欧拉 + 分支约束 + 收敛检查 + 指纹链）。
    pub fn step(&mut self) -> Result<(), PhysExError> {
        if self.tick >= MAX_STEPS_PER_RUN as u64 {
            return Err(PhysExError::new(PhysExCode::StepCountExceeded));
        }
        let dt = self.dt_q16;
        let kind = self.kind;
        match kind {
            SceneKind::InclineSlide => self.step_incline(dt),
            SceneKind::FrictionStop => self.step_friction(dt),
            _ => {
                // 通用路径：速度 → 位置 → 绳/墙/碰撞约束。
                let n = self.bodies.len();
                let mut i = 0;
                while i < n {
                    if !self.bodies[i].fixed {
                        let (ax, ay) = self.accel_of(i);
                        let b = &mut self.bodies[i];
                        let old_vx = b.vel[0];
                        let old_vy = b.vel[1];
                        b.vel[0] = clamp_i64(
                            b.vel[0].saturating_add(q16_mul(ax, dt)),
                            -VEL_LIMIT,
                            VEL_LIMIT,
                        );
                        b.vel[1] = clamp_i64(
                            b.vel[1].saturating_add(q16_mul(ay, dt)),
                            -VEL_LIMIT,
                            VEL_LIMIT,
                        );
                        if b.vel[0] != clamp_i64(old_vx, -VEL_LIMIT, VEL_LIMIT)
                            || b.vel[1] != clamp_i64(old_vy, -VEL_LIMIT, VEL_LIMIT)
                        {
                            self.clamp_count = self.clamp_count.saturating_add(1);
                        }
                    }
                    i += 1;
                }
                i = 0;
                while i < n {
                    if !self.bodies[i].fixed {
                        let vx = self.bodies[i].vel[0];
                        let vy = self.bodies[i].vel[1];
                        let b = &mut self.bodies[i];
                        b.pos[0] = clamp_i64(
                            b.pos[0].saturating_add(q16_mul(vx, dt)),
                            -POS_LIMIT,
                            POS_LIMIT,
                        );
                        b.pos[1] = clamp_i64(
                            b.pos[1].saturating_add(q16_mul(vy, dt)),
                            -POS_LIMIT,
                            POS_LIMIT,
                        );
                    }
                    i += 1;
                }
                self.resolve_rope();
                self.resolve_walls();
                self.resolve_collisions();
            }
        }
        self.tick = self.tick.wrapping_add(1);
        self.check_convergence();
        // 逐步状态指纹链：任一步分叉即破。
        let sh = self.state_hash();
        self.fp_chain = fnv_word(self.fp_chain, sh);
        self.fp_steps = self.fp_steps.saturating_add(1);
        if self.fp_steps % SAMPLE_STRIDE == 0 {
            if self.fp_samples.len() >= SAMPLE_CAP {
                self.fp_samples.remove(0);
            }
            self.fp_samples.push(self.fp_chain);
        }
        Ok(())
    }

    /// 收敛事件检查（每类一条专属判据——事件只发一次）。
    fn check_convergence(&mut self) {
        if self.convergence_step != 0 {
            return;
        }
        let kind = self.kind;
        match kind {
            SceneKind::FreeFall | SceneKind::Projectile => {
                let y = self.bodies[0].pos[1];
                if y <= self.target_q16 {
                    self.convergence_step = self.tick as u32;
                    self.push_event(
                        ExEventKind::GroundCross,
                        format!("落地：y={} 步 {}", q16_fmt(y), self.tick),
                    );
                }
            }
            SceneKind::SpringOscillator => {
                let x = self.bodies[0].pos[0];
                if x <= self.anchor[0] {
                    self.convergence_step = self.tick as u32;
                    self.push_event(
                        ExEventKind::AnchorCross,
                        format!("过平衡点：x={} 步 {}", q16_fmt(x), self.tick),
                    );
                }
            }
            SceneKind::RopePendulum => {
                let x = self.bodies[0].pos[0];
                let y = self.bodies[0].pos[1];
                if x < 0 && y < 0 {
                    self.convergence_step = self.tick as u32;
                    self.push_event(
                        ExEventKind::AnchorCross,
                        format!("过底：x={} 步 {}", q16_fmt(x), self.tick),
                    );
                }
            }
            _ => {}
        }
    }

    /// 逐步状态哈希（全量折叠：tick + 每体 pos/vel——指纹链原料）。
    fn state_hash(&self) -> u64 {
        let mut h = fnv_word(FNV_OFFSET, self.tick);
        let mut i = 0;
        while i < self.bodies.len() {
            let b = &self.bodies[i];
            h = fnv_word(h, b.pos[0] as u64);
            h = fnv_word(h, b.pos[1] as u64);
            h = fnv_word(h, b.vel[0] as u64);
            h = fnv_word(h, b.vel[1] as u64);
            i += 1;
        }
        h
    }

    /// 动能 + 位能（重力位 + 弹性位——教学能量账）。
    pub fn energy_q16(&self) -> i64 {
        let mut e: i64 = 0;
        let mut i = 0;
        while i < self.bodies.len() {
            let b = &self.bodies[i];
            if !b.fixed {
                let vx2 = q16_mul(b.vel[0], b.vel[0]);
                let vy2 = q16_mul(b.vel[1], b.vel[1]);
                let ke = q16_mul(b.mass_q16, vx2.saturating_add(vy2)) >> 1;
                e = e.saturating_add(ke);
                if self.gravity_q16 != 0 {
                    let pe = q16_mul(b.mass_q16, q16_mul(self.gravity_q16, b.pos[1]));
                    e = e.saturating_add(pe);
                }
            }
            i += 1;
        }
        if self.spring_k_q16 > 0 && !self.bodies.is_empty() {
            let b = &self.bodies[0];
            let dx = b.pos[0].saturating_sub(self.anchor[0]);
            let dy = b.pos[1].saturating_sub(self.anchor[1]);
            let dsq = q16_mul(dx, dx).saturating_add(q16_mul(dy, dy));
            let len = sqrt_q16(dsq);
            let stretch = len.saturating_sub(self.spring_rest_q16);
            let s2 = q16_mul(stretch, stretch);
            let pe = q16_mul(self.spring_k_q16, s2) >> 1;
            e = e.saturating_add(pe);
        }
        e
    }

    /// 系统动量 (px, py)（Q16——守恒教学基准读数）。
    pub fn momentum_q16(&self) -> (i64, i64) {
        let mut px: i64 = 0;
        let mut py: i64 = 0;
        let mut i = 0;
        while i < self.bodies.len() {
            let b = &self.bodies[i];
            if !b.fixed {
                px = px.saturating_add(q16_mul(b.mass_q16, b.vel[0]));
                py = py.saturating_add(q16_mul(b.mass_q16, b.vel[1]));
            }
            i += 1;
        }
        (px, py)
    }

    /// 轨迹采样点（调试视图帧号原料——位置/速度/能量/指纹五元组）。
    pub fn trail_point(&self, step: u32) -> TrailPoint {
        let b = &self.bodies[0];
        TrailPoint {
            step,
            x: b.pos[0],
            y: b.pos[1],
            vx: b.vel[0],
            vy: b.vel[1],
            energy_q16: self.energy_q16(),
            fp: self.fp_chain,
        }
    }
}

/// i64 绝对值比较辅助（|a| ≤ b，i128 中间量——i64::MIN 安全）。
trait AbsLe {
    fn abs_le(self, other: i64) -> bool;
}

impl AbsLe for i64 {
    fn abs_le(self, other: i64) -> bool {
        let a = (self as i128).unsigned_abs();
        let b = (other as i128).unsigned_abs();
        a <= b
    }
}

/// 种子扰动（确定性 xorshift 双哈希 → [−amp, +amp] 整数格点）。
pub fn seed_perturb(seed: u64, body_idx: usize, amp_q16: i64) -> [i64; 2] {
    let h1 = xorshift64(
        seed ^ (body_idx as u64)
            .wrapping_mul(GOLDEN_MIX)
            .wrapping_add(0xA5A5_5A5A),
    );
    let h2 = xorshift64(h1 ^ 0xDEAD_BEEF_CAFE_F00D);
    let span = (2u64)
        .wrapping_mul(amp_q16 as u64)
        .wrapping_add(1);
    let d1 = (h1 % span) as i64 - amp_q16;
    let d2 = (h2 % span) as i64 - amp_q16;
    [d1, d2]
}

/// 轨迹采样点（调试视图帧号标注的最小单元）。
#[derive(Clone, Copy, Debug)]
pub struct TrailPoint {
    /// 帧号（逻辑步号）。
    pub step: u32,
    /// 0 号体位置 x（Q16）。
    pub x: i64,
    /// 0 号体位置 y（Q16）。
    pub y: i64,
    /// 0 号体速度 vx（Q16）。
    pub vx: i64,
    /// 0 号体速度 vy（Q16）。
    pub vy: i64,
    /// 系统总能量（Q16）。
    pub energy_q16: i64,
    /// 该步指纹链值。
    pub fp: u64,
}
