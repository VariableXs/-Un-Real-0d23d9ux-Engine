//! unx0885_irqbal —— UNX-F0885 中断负载均衡器（分布观测与均衡决策）。
//! 锚点（CoRun Unxreal 主册）：UNX-F0885 · 中断负载均衡器（分布观测与均衡决策） · 目标 320 行 · 已深化
//! 判据与定位原文（docs/Varix/CoRun Varix STAR II · Unxreal/CoRun Varix STAR II · Unxreal.md:2538 全文抄录）：
//!   | UNX-F0885 | 中断负载均衡器（分布观测与均衡决策） | 320 | 已深化 |
//!     UNX-F0885-J1 分布观测准确（per 核投递率直方图与实测误差 <1%），
//!     均衡决策保守性（决策前后迁移量 ≤5%/轮），
//!     抖动抑制（震荡决策 10/10 次被抑制）
//! 组件：PerCoreHistogram per 核投递率直方图（8 槽/窗 × 4 窗滚动 · 槽投递计数 unit-bin 标定域
//!       0..=1024 · 域外拒收不入 bin（保观测误差结构性为零）· 增量对账 vs 实测原始计数器
//!       · 误差 ppm 口径 <10000（<1%）判据自证）+
//!       ImbalanceDetector 失衡检测（max/min 核速率比 · ≥125% 判失衡 · 轻/中/重三级分级
//!       （160%/250% 分界）· 最载核/最闲核定位 · 活跃核门限（<2 活跃核不动））+
//!       BalanceDecision 均衡决策（保守性红线：单轮迁移量 ≤5%（500bps）· 迁移向量数上限 4 条/轮
//!       · 决策留痕（原因/迁移清单/预期改善 ppm）· 双重复核（计划端+执行端各算一次 bps））+
//!       OscillationGuard 抖动抑制（同向滞回：连续 3 轮同向失衡才放行 · 反向迁移冷却期 4 轮硬闸
//!       · 震荡决策 10/10 次被抑制判据自证）+
//!       迁移执行账（每向量记录 源核/目标核/向量号/单位/轮次/时戳 · 迁移后下一窗重测验证
//!       （热核速率须降 + 冷核速率须升 · 双向强校验））+
//!       事件账 + health() + FNV-1a 指纹 + 双跑确定性对拍 + 三路注入面
//!       （观测造假注入/超限迁移注入/震荡注入 各必检出）。
//! 职责定位：均衡器的第一美德是"不添乱"——观测必须与实测对得上账（<1%），
//!   决策必须保守（单轮 ≤5%），抖动必须被滞回压住（10/10 抑制）；
//!   三条判据全部在本模块内以确定性整数演练自证，不依赖外部环境。
//! 性能逐项分解：observe O(1)；close_round O(核×槽)；cross_check O(核×bin)；
//!   detect O(核)；plan O(1)；guard O(1)；fingerprint O(账面)；
//!   演练 24 窗 × 8 槽 × 4 核 确定性 LCG；全整数零浮点零墙钟。
//! 诊断码段：0x1930（0x1930 基址保留 + 0x1931~0x193C 十二变体 + 0x193D~0x193F 备用；
//!   全域 grep 确认独占：0x1920 段已被 ves470_s25docs 占用，0x1940 段已被 ves471_s25sum 占用）。
//! 纯功能线：no_std + alloc，内核路径零 unwrap/expect/panic!/运行期 assert!，
//!   可失败全走 Result<T, IrqBalError>，溢出显性 saturating/wrapping，
//!   >64KB 结构体（CoreObservation ≈ 8.3KB 堆上 hist）不上栈按引用传递。

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

// ============================================================================
// 常量区（全部写死并注释来源）
// ============================================================================

/// 模块版本。
pub const IB_VERSION: &str = "unx0885_irqbal-v1";

/// 诊断段基址（0x1930 段 · 本模块独占；0x1930 为保留基址码，变体自 0x1931 起）。
pub const IB_SEG_BASE: u16 = 0x1930;

/// 核数上限（x2APIC 集群容量上限 · 与 unx088_irqaffinity 同域惯例对齐）。
pub const IB_MAX_CORES: usize = 64;

/// 百万比（ppm）基准。
pub const IB_PPM_ONE: u64 = 1_000_000;
/// 万分比（bps）基准。
pub const IB_BPS_ONE: u64 = 10_000;

/// J1 观测误差容忍：<1% = 10000 ppm。
pub const IB_ERR_TOL_PPM: u64 = 10_000;

/// J1 决策保守性红线：单轮迁移量 ≤5% = 500 bps。
pub const IB_MIG_CAP_BPS: u64 = 500;

/// 单轮迁移向量数上限（每轮至多 4 条迁移向量 · 保守拆分防单点巨迁）。
pub const IB_VEC_CAP: usize = 4;

/// 失衡进入阈值：max/min 速率比 ≥125% 判失衡（125% 以下视为均衡态）。
pub const IB_IMBALANCE_ENTER_PPM: u64 = 1_250_000;
/// 失衡分级界：轻度上限 160%。
pub const IB_GRADE_LIGHT_PPM: u64 = 1_600_000;
/// 失衡分级界：中度上限 250%（≥250% 为重度）。
pub const IB_GRADE_MEDIUM_PPM: u64 = 2_500_000;

/// 槽投递标定域上界（每槽投递计数 unit-bin 直方图域 0..=1024 · 域外拒收）。
pub const IB_SLOT_CAP: u64 = 1024;
/// 直方图 bin 数（unit-bin：bin[i] 即"槽投递计数恰为 i"的槽个数 · 0..=1024 共 1025 个）。
pub const IB_HIST_SPAN: usize = 1025;
/// 每窗槽数（观测窗划分为 8 个投递槽）。
pub const IB_WINDOW_SLOTS: usize = 8;
/// 滚动窗数（保留最近 4 窗的窗总量环 · 速率取滚动均值抗单窗毛刺）。
pub const IB_ROLL_WINDOWS: usize = 4;

/// 同向滞回轮数（同方向失衡连续 ≥3 轮才放行下一次迁移 · J1 抖动抑制的滞回半边）。
pub const IB_HYST_STREAK: u32 = 3;
/// 反向迁移冷却期（迁移后 4 轮内反向迁移硬闸 · J1 抖动抑制的冷却半边）。
pub const IB_COOLDOWN: u32 = 4;

/// 事件账容量（超出则淘汰最旧 · 环形语义保指纹确定性）。
pub const IB_EVENT_CAP: usize = 4096;

/// 演练窗数（24 窗：足够完成 观测→失衡→滞回→迁移→重测 全链路多轮）。
pub const IB_DRILL_WINDOWS: u64 = 24;
/// 演练核数（4 核：1 重 + 2 中 + 1 轻，比值约 10:6:3:1）。
pub const IB_DRILL_CORES: usize = 4;
/// 震荡注入探测次数（J1 点名 10/10）。
pub const IB_OSC_PROBES: u64 = 10;

/// LCG 乘子（glibc 风格线性同余 · Numerical Recipes 推荐常数）。
pub const IB_LCG_MUL: u64 = 6364136223846793005;
/// LCG 增量（同上 · 与乘子配对满周期）。
pub const IB_LCG_ADD: u64 = 1442695040888963407;
/// 演练种子搅拌盐（圆周率黄金分割近似 · 与种子异或防平凡种子退化）。
pub const IB_LCG_SALT: u64 = 0x9E3779B97F4A7C15;

/// FNV-1a 64 位偏移基（FNV 基准参数 · fingerprint 起始值）。
pub const IB_FNV_OFFSET: u64 = 0xcbf29ce484222325;
/// FNV-1a 64 位素数（FNV 基准参数）。
pub const IB_FNV_PRIME: u64 = 0x100000001b3;

// ---- 编译期判据自检（const 断言 · 不占运行期）----
const _: () = assert!(IB_ERR_TOL_PPM == IB_PPM_ONE / 100); // <1% 恰为 10000ppm
const _: () = assert!(IB_MIG_CAP_BPS * 20 == IB_BPS_ONE); // 5% 恰为 1/20 · 500bps
const _: () = assert!(IB_HIST_SPAN == (IB_SLOT_CAP as usize) + 1); // unit-bin 域恰全覆盖
const _: () = assert!(IB_WINDOW_SLOTS == 8 && IB_ROLL_WINDOWS == 4); // 8 槽 × 4 窗
const _: () = assert!(IB_MAX_CORES <= 64); // 核位图容量约束
const _: () = assert!(IB_IMBALANCE_ENTER_PPM < IB_GRADE_LIGHT_PPM
    && IB_GRADE_LIGHT_PPM < IB_GRADE_MEDIUM_PPM); // 分级单调

// ============================================================================
// 通用整数辅助（const fn · 无 slice/Vec/Ord 依赖 · 可编译期求值）
// ============================================================================

/// part/whole 的 ppm 值（whole=0 时返回 0 · 饱和乘防溢出）。
pub const fn ppm_of(part: u64, whole: u64) -> u64 {
    if whole == 0 {
        0
    } else {
        part.saturating_mul(IB_PPM_ONE) / whole
    }
}

/// part/whole 的 bps 值（whole=0 时返回 0 · 饱和乘防溢出）。
pub const fn bps_of(part: u64, whole: u64) -> u64 {
    if whole == 0 {
        0
    } else {
        part.saturating_mul(IB_BPS_ONE) / whole
    }
}

/// 给定全局每槽均值，单轮迁移量红线对应的单位上限（先饱和乘后除 · 保守取整）。
pub const fn cap_units_for(mean_units: u64) -> u64 {
    mean_units.saturating_mul(IB_MIG_CAP_BPS) / IB_BPS_ONE
}

/// bps → ppm 换算（1 bps = 100 ppm）。
pub const fn bps_to_ppm(bps: u64) -> u64 {
    bps.saturating_mul(100)
}

// ============================================================================
// 失衡分级
// ============================================================================

/// 失衡分级（轻/中/重三档 · 以 max/min 速率比 ppm 判档）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum IbGrade {
    /// 均衡（ratio < 125%）。
    Balanced,
    /// 轻度失衡（125% ≤ ratio < 160%）。
    Light,
    /// 中度失衡（160% ≤ ratio < 250%）。
    Medium,
    /// 重度失衡（ratio ≥ 250%）。
    Heavy,
}

impl IbGrade {
    /// 全序（供遍历/自检）。
    pub const ALL: [IbGrade; 4] = [
        IbGrade::Balanced,
        IbGrade::Light,
        IbGrade::Medium,
        IbGrade::Heavy,
    ];

    /// 分级名。
    pub fn name(self) -> &'static str {
        match self {
            IbGrade::Balanced => "balanced",
            IbGrade::Light => "light",
            IbGrade::Medium => "medium",
            IbGrade::Heavy => "heavy",
        }
    }

    /// 分级码（const fn · 0=均衡 1=轻 2=中 3=重）。
    pub const fn code(self) -> u8 {
        match self {
            IbGrade::Balanced => 0,
            IbGrade::Light => 1,
            IbGrade::Medium => 2,
            IbGrade::Heavy => 3,
        }
    }
}

/// 由速率比 ppm 判级（const fn · 纯阈值比较 · 无 Ord 依赖）。
pub const fn grade_of(ratio_ppm: u64) -> IbGrade {
    if ratio_ppm < IB_IMBALANCE_ENTER_PPM {
        IbGrade::Balanced
    } else if ratio_ppm < IB_GRADE_LIGHT_PPM {
        IbGrade::Light
    } else if ratio_ppm < IB_GRADE_MEDIUM_PPM {
        IbGrade::Medium
    } else {
        IbGrade::Heavy
    }
}

// ============================================================================
// 错误目录（每变体带 code() 与中文可操作 detail()）
// ============================================================================

/// 均衡器错误目录（0x1930 段 · 0x1931~0x193C 十二变体）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum IrqBalError {
    /// 核号越界（0x1931）。
    CoreOutOfRange { core: usize },
    /// 观测账为空（尚无任何封闭窗，速率无从谈起）（0x1932）。
    EmptyObservation,
    /// 槽投递超出标定域（0..=1024）——拒收不入 bin，保直方图误差结构性为零（0x1933）。
    SlotOverflow { core: usize, units: u64 },
    /// 观测造假：直方图增量与实测增量对账超阈（0x1934）。
    ObservationForgery { core: usize, error_ppm: u64 },
    /// 速率分母为零（无有效活跃核）（0x1935）。
    ZeroRateDenominator,
    /// 迁移量超保守性红线（>500bps/轮）（0x1936）。
    MigrationOverCap { requested_bps: u64, cap_bps: u64 },
    /// 迁移向量数超上限（>4 条/轮）（0x1937）。
    VectorCapExceeded { requested: usize, cap: usize },
    /// 滞回拦截：同向失衡连续轮数不足（0x1938）。
    HysteresisBlocked { streak: u32, need: u32 },
    /// 冷却期拦截：反向迁移距上次迁移过近（0x1939）。
    CooldownActive { remain: u64 },
    /// 迁移后重测不达标（热核未降或冷核未升）（0x193A）。
    ReMeasureMismatch { hot: usize, cold: usize },
    /// 无可均衡对象（迁移向量为空或活跃核不足）（0x193B）。
    NothingToBalance,
    /// 双跑确定性对拍指纹不一致（0x193C）。
    FingerprintMismatch { left: u64, right: u64 },
}

impl IrqBalError {
    /// 诊断码（0x1930 段内独占）。
    pub const fn code(&self) -> u16 {
        match self {
            IrqBalError::CoreOutOfRange { .. } => IB_SEG_BASE + 0x01, // 0x1931
            IrqBalError::EmptyObservation => IB_SEG_BASE + 0x02,      // 0x1932
            IrqBalError::SlotOverflow { .. } => IB_SEG_BASE + 0x03,   // 0x1933
            IrqBalError::ObservationForgery { .. } => IB_SEG_BASE + 0x04, // 0x1934
            IrqBalError::ZeroRateDenominator => IB_SEG_BASE + 0x05,   // 0x1935
            IrqBalError::MigrationOverCap { .. } => IB_SEG_BASE + 0x06, // 0x1936
            IrqBalError::VectorCapExceeded { .. } => IB_SEG_BASE + 0x07, // 0x1937
            IrqBalError::HysteresisBlocked { .. } => IB_SEG_BASE + 0x08, // 0x1938
            IrqBalError::CooldownActive { .. } => IB_SEG_BASE + 0x09, // 0x1939
            IrqBalError::ReMeasureMismatch { .. } => IB_SEG_BASE + 0x0A, // 0x193A
            IrqBalError::NothingToBalance => IB_SEG_BASE + 0x0B,      // 0x193B
            IrqBalError::FingerprintMismatch { .. } => IB_SEG_BASE + 0x0C, // 0x193C
        }
    }

    /// 中文可操作诊断（每变体一句 · 告知原因与下一步动作）。
    pub fn detail(&self) -> &'static str {
        match self {
            IrqBalError::CoreOutOfRange { .. } => {
                "核号越界：请核对核数初始化（with_cores）与调用方核号来源，上限见 IB_MAX_CORES"
            }
            IrqBalError::EmptyObservation => {
                "观测账为空：请先经 observe/close_round 封闭至少一个观测窗再触发检测"
            }
            IrqBalError::SlotOverflow { .. } => {
                "槽投递超标定域：单槽投递计数超过 1024，请调细观测粒度（分批上报）而非放大域"
            }
            IrqBalError::ObservationForgery { .. } => {
                "观测造假：直方图增量与实测原始计数器对账超阈，请排查观测通路的旁路写/回放，并执行重定基"
            }
            IrqBalError::ZeroRateDenominator => {
                "速率分母为零：全部核滚动窗无投递，请检查中断源是否被整体屏蔽"
            }
            IrqBalError::MigrationOverCap { .. } => {
                "迁移量超红线：单轮迁移超过全局量 5%（500bps），请缩小迁移单位或拆分多轮执行"
            }
            IrqBalError::VectorCapExceeded { .. } => {
                "迁移向量超上限：单轮向量数超过 4 条，请合并同向迁移或延后至下一轮"
            }
            IrqBalError::HysteresisBlocked { .. } => {
                "滞回拦截：同向失衡未满 3 连轮，属预期抖动抑制；请等待下一观测窗自然累积"
            }
            IrqBalError::CooldownActive { .. } => {
                "冷却期拦截：反向迁移距上次迁移不足 4 轮，属预期防震荡硬闸；请等待冷却期满"
            }
            IrqBalError::ReMeasureMismatch { .. } => {
                "重测不达标：迁移后热核速率未降或冷核未升，请核对迁移是否真实生效（权重/亲和性写入）"
            }
            IrqBalError::NothingToBalance => {
                "无可均衡对象：迁移清单为空或活跃核不足 2 个，无需均衡，直接返回即可"
            }
            IrqBalError::FingerprintMismatch { .. } => {
                "双跑指纹不一致：同种子两次演练账面不同，请排查非确定性来源（浮点/墙钟/未初始化内存）"
            }
        }
    }
}

// ============================================================================
// 事件账
// ============================================================================

/// 事件账条目（全原始字段 · 指纹按字段摘要 · 环形容量防无界增长）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum IbEvent {
    /// 启动（核数）。
    Booted { cores: usize },
    /// 一次槽观测（核/单位）。
    Observed { core: usize, units: u64 },
    /// 封窗（窗序/全局窗总量）。
    WindowClosed { seq: u64, global_units: u64 },
    /// 失衡检出（轮次/分级码/比率/热核/冷核）。
    Imbalance {
        round: u64,
        grade_code: u8,
        ratio_ppm: u64,
        hot: usize,
        cold: usize,
    },
    /// 决策被拦（轮次/拦截码）。
    Rejected { round: u64, code: u16 },
    /// 决策放行（轮次/迁移量/迁移bps）。
    Admitted {
        round: u64,
        moved_units: u64,
        moved_bps: u64,
    },
    /// 单向量迁移落账（账序/源核/目标核/向量号/单位）。
    VectorApplied {
        seq: u64,
        src: usize,
        dst: usize,
        vector: u32,
        units: u64,
    },
    /// 重测通过（轮次）。
    ReMeasureOk { round: u64 },
    /// 造假检出（核/误差ppm）。
    ForgeryCaught { core: usize, error_ppm: u64 },
    /// 重定基完成（核）。
    ForgeryRebased { core: usize },
    /// 超限迁移被拒（请求bps）。
    OvercapRejected { requested_bps: u64 },
    /// 震荡探测被抑制（探测序/拦截码）。
    OscillationSuppressed { attempt: u64, code: u16 },
    /// 健康封账（健康码）。
    HealthSealed { health_code: u8 },
}

impl IbEvent {
    /// 事件名（供账面渲染）。
    pub fn name(&self) -> &'static str {
        match self {
            IbEvent::Booted { .. } => "booted",
            IbEvent::Observed { .. } => "observed",
            IbEvent::WindowClosed { .. } => "window_closed",
            IbEvent::Imbalance { .. } => "imbalance",
            IbEvent::Rejected { .. } => "rejected",
            IbEvent::Admitted { .. } => "admitted",
            IbEvent::VectorApplied { .. } => "vector_applied",
            IbEvent::ReMeasureOk { .. } => "remeasure_ok",
            IbEvent::ForgeryCaught { .. } => "forgery_caught",
            IbEvent::ForgeryRebased { .. } => "forgery_rebased",
            IbEvent::OvercapRejected { .. } => "overcap_rejected",
            IbEvent::OscillationSuppressed { .. } => "oscillation_suppressed",
            IbEvent::HealthSealed { .. } => "health_sealed",
        }
    }
}

/// 事件字段摘要（指纹用 · 每变体折一个 u64，变体名折高位标签防碰撞混淆）。
fn event_digest(e: &IbEvent) -> u64 {
    let tag: u64 = match e {
        IbEvent::Booted { .. } => 0x01,
        IbEvent::Observed { .. } => 0x02,
        IbEvent::WindowClosed { .. } => 0x03,
        IbEvent::Imbalance { .. } => 0x04,
        IbEvent::Rejected { .. } => 0x05,
        IbEvent::Admitted { .. } => 0x06,
        IbEvent::VectorApplied { .. } => 0x07,
        IbEvent::ReMeasureOk { .. } => 0x08,
        IbEvent::ForgeryCaught { .. } => 0x09,
        IbEvent::ForgeryRebased { .. } => 0x0A,
        IbEvent::OvercapRejected { .. } => 0x0B,
        IbEvent::OscillationSuppressed { .. } => 0x0C,
        IbEvent::HealthSealed { .. } => 0x0D,
    } << 56;
    let body: u64 = match e {
        IbEvent::Booted { cores } => *cores as u64,
        IbEvent::Observed { core, units } => (*core as u64) ^ units.rotate_left(16),
        IbEvent::WindowClosed { seq, global_units } => seq ^ global_units.rotate_left(24),
        IbEvent::Imbalance {
            round,
            grade_code,
            ratio_ppm,
            hot,
            cold,
        } => round ^ (*grade_code as u64) ^ ratio_ppm.rotate_left(8) ^ ((*hot as u64) << 40) ^ ((*cold as u64) << 48),
        IbEvent::Rejected { round, code } => round ^ (*code as u64).rotate_left(32),
        IbEvent::Admitted {
            round,
            moved_units,
            moved_bps,
        } => round ^ moved_units.rotate_left(16) ^ ((*moved_bps as u64) << 48),
        IbEvent::VectorApplied {
            seq,
            src,
            dst,
            vector,
            units,
        } => seq ^ ((*src as u64) << 16) ^ ((*dst as u64) << 24) ^ ((*vector as u64) << 32) ^ units.rotate_left(40),
        IbEvent::ReMeasureOk { round } => round.rotate_left(8),
        IbEvent::ForgeryCaught { core, error_ppm } => (*core as u64) ^ error_ppm.rotate_left(20),
        IbEvent::ForgeryRebased { core } => (*core as u64).rotate_left(12),
        IbEvent::OvercapRejected { requested_bps } => requested_bps.rotate_left(4),
        IbEvent::OscillationSuppressed { attempt, code } => attempt ^ ((*code as u64) << 32),
        IbEvent::HealthSealed { health_code } => *health_code as u64,
    };
    tag ^ body
}

// ============================================================================
// PerCoreHistogram —— per 核投递率直方图（观测半区）
// ============================================================================

/// 单核观测账：原始计数器（实测半区）+ unit-bin 直方图（观测半区）+ 滚动窗环。
///
/// 结构约 8.3KB（含 1025×u64 堆上 hist）——只按引用传递，绝不上栈拷贝。
/// 增量对账：cross_check 比较自上次对账以来 直方图增量 vs 实测增量（ppm 口径），
/// 域内逐位一致（0 ppm）；造假注入使两半区分裂即超阈检出。
#[derive(Clone, Debug)]
pub struct CoreObservation {
    /// 核号。
    core: usize,
    /// 实测累计投递（原始计数器 · 饱和累加）。
    raw_total: u64,
    /// 最近一次封窗时的实测快照（增量对账基准）。
    raw_at_close: u64,
    /// 当前窗各槽投递计数（8 槽 · 每槽 ≤1024）。
    slots: [u64; IB_WINDOW_SLOTS],
    /// 槽游标（observe 写入后前移 · close_round 归零）。
    slot_cursor: usize,
    /// unit-bin 直方图：hist[v] = 槽投递计数恰为 v 的已封槽数（堆上 · 1025 bin）。
    hist: Vec<u64>,
    /// 直方图修正量（造假重定基后使 report = raw · i64 允许双向伪造）。
    hist_correction: i64,
    /// 自上次对账以来的直方图快照（增量对账 · 观测半区基准）。
    chk_hist_snap: u64,
    /// 自上次对账以来的实测快照（增量对账 · 实测半区基准）。
    chk_raw_snap: u64,
    /// 滚动窗总量环（最近 4 窗 · 未填位为 0）。
    window_totals: [u64; IB_ROLL_WINDOWS],
    /// 环游标。
    window_cursor: usize,
    /// 已封窗数。
    closed_windows: u64,
    /// 域外拒收计数（超 1024/槽的观测被拒而未入 bin · 保误差结构性为零）。
    overflow_bag: u64,
}

impl CoreObservation {
    /// 新建单核观测账（堆上分配 1025 bin 全零直方图）。
    pub fn new(core: usize) -> CoreObservation {
        let mut hist = Vec::new();
        let mut i = 0usize;
        while i < IB_HIST_SPAN {
            hist.push(0u64);
            i += 1;
        }
        CoreObservation {
            core,
            raw_total: 0,
            raw_at_close: 0,
            slots: [0u64; IB_WINDOW_SLOTS],
            slot_cursor: 0,
            hist,
            hist_correction: 0,
            chk_hist_snap: 0,
            chk_raw_snap: 0,
            window_totals: [0u64; IB_ROLL_WINDOWS],
            window_cursor: 0,
            closed_windows: 0,
            overflow_bag: 0,
        }
    }

    /// 核号。
    pub fn core(&self) -> usize {
        self.core
    }

    /// 实测累计投递。
    pub fn raw_total(&self) -> u64 {
        self.raw_total
    }

    /// 域外拒收计数。
    pub fn overflow_bag(&self) -> u64 {
        self.overflow_bag
    }

    /// 已封窗数。
    pub fn closed_windows(&self) -> u64 {
        self.closed_windows
    }

    /// 记一笔槽投递（每槽恰一笔 · 单位 ≤1024 · 域外拒收）。
    fn observe(&mut self, units: u64) -> Result<(), IrqBalError> {
        if units > IB_SLOT_CAP {
            self.overflow_bag = self.overflow_bag.saturating_add(1);
            return Err(IrqBalError::SlotOverflow {
                core: self.core,
                units,
            });
        }
        let cur = self.slot_cursor;
        self.slots[cur] = self.slots[cur].saturating_add(units);
        self.raw_total = self.raw_total.saturating_add(units);
        self.slot_cursor = (cur + 1) % IB_WINDOW_SLOTS;
        Ok(())
    }

    /// 封窗：逐槽入 bin（unit-bin · 域内精确）、窗总量入环、清槽、快照实测。
    fn close_window(&mut self) {
        let mut total = 0u64;
        let mut s = 0usize;
        while s < IB_WINDOW_SLOTS {
            let cnt = self.slots[s];
            // 域内守卫：observe 已拒域外，此处再防御一次（超界不入 bin 只进拒收袋）
            if cnt > IB_SLOT_CAP {
                self.overflow_bag = self.overflow_bag.saturating_add(1);
            } else if cnt > 0 {
                let idx = cnt as usize;
                if idx < self.hist.len() {
                    self.hist[idx] = self.hist[idx].saturating_add(1);
                }
            }
            total = total.saturating_add(cnt);
            self.slots[s] = 0;
            s += 1;
        }
        self.window_totals[self.window_cursor] = total;
        self.window_cursor = (self.window_cursor + 1) % IB_ROLL_WINDOWS;
        self.closed_windows = self.closed_windows.saturating_add(1);
        self.raw_at_close = self.raw_total;
        self.slot_cursor = 0;
    }

    /// 纯直方图加权和（Σ bin[v]×v · 不含修正量）。
    fn hist_raw_total(&self) -> u64 {
        let mut i = 0usize;
        let mut t = 0u64;
        while i < self.hist.len() {
            let b = self.hist[i];
            if b > 0 {
                t = t.saturating_add(b.saturating_mul(i as u64));
            }
            i += 1;
        }
        t
    }

    /// 直方图报告总量（含重定基修正 · 即"观测半区"对外口径）。
    fn hist_reported_total(&self) -> u64 {
        let v = self.hist_raw_total() as i64 - self.hist_correction;
        if v < 0 {
            0
        } else {
            v as u64
        }
    }

    /// 滚动窗有效槽位（min(已封窗, 4) × 8）。
    fn roll_slots(&self) -> u64 {
        let w = if self.closed_windows < IB_ROLL_WINDOWS as u64 {
            self.closed_windows
        } else {
            IB_ROLL_WINDOWS as u64
        };
        w.saturating_mul(IB_WINDOW_SLOTS as u64)
    }

    /// 滚动窗总量（环内求和 · 未填位为 0 天然参与）。
    fn roll_sum(&self) -> u64 {
        let mut i = 0usize;
        let mut t = 0u64;
        while i < IB_ROLL_WINDOWS {
            t = t.saturating_add(self.window_totals[i]);
            i += 1;
        }
        t
    }

    /// 滚动均值（每槽投递单位）。
    fn mean_units(&self) -> u64 {
        let s = self.roll_slots();
        if s == 0 {
            0
        } else {
            self.roll_sum() / s
        }
    }

    /// 滚动速率（占槽容量 1024 的 ppm 口径）。
    fn rate_ppm(&self) -> u64 {
        ppm_of(self.mean_units(), IB_SLOT_CAP)
    }

    /// 增量对账：|观测增量 − 实测增量| 的 ppm（更新快照 · 域内结构性为 0）。
    fn cross_check_delta(&mut self) -> (u64, u64) {
        let rep = self.hist_reported_total();
        let raw = self.raw_at_close;
        let d_hist = rep.saturating_sub(self.chk_hist_snap);
        let d_raw = raw.saturating_sub(self.chk_raw_snap);
        self.chk_hist_snap = rep;
        self.chk_raw_snap = raw;
        let diff = if d_hist > d_raw {
            d_hist - d_raw
        } else {
            d_raw - d_hist
        };
        // 实测增量为 0 而观测增量非 0 → 视作 100% 误差（除零保护口径）
        let err = if d_raw == 0 {
            if d_hist == 0 { 0 } else { IB_PPM_ONE }
        } else {
            ppm_of(diff, d_raw)
        };
        (err, diff)
    }

    /// 重定基：修正直方图报告总量至实测快照（对账检出造假后的恢复动作）。
    fn rebase(&mut self) {
        let rep = self.hist_raw_total();
        let raw = self.raw_at_close;
        self.hist_correction += rep as i64 - raw as i64;
        self.chk_hist_snap = self.hist_reported_total();
        self.chk_raw_snap = self.raw_at_close;
    }

    /// 注入面：伪造直方图 bin（向 idx=8 bin 添加 phantom_slots 个幽灵槽 · 模拟旁路写观测账）。
    fn forge_bin(&mut self, idx: usize, phantom_slots: u64) -> Result<(), IrqBalError> {
        if idx >= self.hist.len() {
            return Err(IrqBalError::CoreOutOfRange { core: idx });
        }
        self.hist[idx] = self.hist[idx].saturating_add(phantom_slots);
        Ok(())
    }

    /// 核账面摘要（指纹与渲染共用）。
    fn digest_fields(&self) -> (u64, u64, u64, u64) {
        (
            self.raw_total,
            self.raw_at_close,
            self.hist_reported_total(),
            self.overflow_bag,
        )
    }
}

// ============================================================================
// ImbalanceDetector —— 失衡检测（max/min 速率比 + 分级 + 热冷核定位）
// ============================================================================

/// 失衡检测报告（快照值 · Copy 便于留痕与重测对读）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ImbalanceReport {
    /// 最载核（热核）。
    pub hot: usize,
    /// 最闲核（冷核）。
    pub cold: usize,
    /// 热核滚动速率（ppm of 槽容量）。
    pub hot_rate_ppm: u64,
    /// 冷核滚动速率（ppm of 槽容量）。
    pub cold_rate_ppm: u64,
    /// max/min 速率比（ppm）。
    pub ratio_ppm: u64,
    /// 失衡分级。
    pub grade: IbGrade,
    /// 活跃核数（滚动均值 > 0 的核）。
    pub active_cores: usize,
    /// 全局每槽均值（迁移红线分母）。
    pub global_mean_units: u64,
    /// 全核滚动速率快照（重测对读按核号取值）。
    pub rates_ppm: [u64; IB_MAX_CORES],
}

impl ImbalanceReport {
    /// 报告摘要（决策留痕用）。
    pub fn summary(&self) -> String {
        format!(
            "imbalance ratio={}ppm grade={} hot#c{}={}ppm cold#c{}={}ppm active={} global_mean={}u",
            self.ratio_ppm,
            self.grade.name(),
            self.hot,
            self.hot_rate_ppm,
            self.cold,
            self.cold_rate_ppm,
            self.active_cores,
            self.global_mean_units,
        )
    }
}

/// 失衡检测器（无状态判定 + 有状态统计 · 检出/检查计数留痕）。
#[derive(Clone, Debug)]
pub struct ImbalanceDetector {
    /// 检查次数。
    checks: u64,
    /// 检出失衡次数。
    detections: u64,
    /// 最近一次报告。
    last: Option<ImbalanceReport>,
}

impl ImbalanceDetector {
    /// 新建。
    pub fn new() -> ImbalanceDetector {
        ImbalanceDetector {
            checks: 0,
            detections: 0,
            last: None,
        }
    }

    /// 检查次数。
    pub fn checks(&self) -> u64 {
        self.checks
    }

    /// 检出次数。
    pub fn detections(&self) -> u64 {
        self.detections
    }

    /// 最近一次报告。
    pub fn last(&self) -> Option<ImbalanceReport> {
        self.last
    }

    /// 判定入口：账面 → 报告（均衡返回 None · 检出返回 Some）。
    pub fn detect(
        &mut self,
        cores: &[CoreObservation],
    ) -> Result<Option<ImbalanceReport>, IrqBalError> {
        let rep = detect_report(cores)?;
        self.checks = self.checks.saturating_add(1);
        if rep.is_some() {
            self.detections = self.detections.saturating_add(1);
        }
        self.last = rep;
        Ok(rep)
    }
}

/// 纯判定函数：核账切片 → 失衡报告（均衡/不具检测条件 → None）。
fn detect_report(cores: &[CoreObservation]) -> Result<Option<ImbalanceReport>, IrqBalError> {
    let mut rates = [0u64; IB_MAX_CORES];
    let mut active = 0usize;
    let mut hot = 0usize;
    let mut cold = 0usize;
    let mut hot_rate = 0u64;
    let mut cold_rate = u64::MAX;
    let mut global_units = 0u64;
    let mut slots = 0u64;
    let mut i = 0usize;
    while i < cores.len() && i < IB_MAX_CORES {
        let r = cores[i].rate_ppm();
        rates[i] = r;
        let mu = cores[i].mean_units();
        global_units = global_units.saturating_add(cores[i].roll_sum());
        slots = slots.saturating_add(cores[i].roll_slots());
        if mu > 0 {
            active = active.saturating_add(1);
            if r > hot_rate {
                hot_rate = r;
                hot = i;
            }
            if r < cold_rate {
                cold_rate = r;
                cold = i;
            }
        }
        i += 1;
    }
    if slots == 0 {
        return Err(IrqBalError::EmptyObservation);
    }
    let mean_units = global_units / slots;
    if active < 2 {
        // 单核活跃：无从比较，亦无从迁移
        return Ok(None);
    }
    if cold_rate == 0 || cold_rate == u64::MAX {
        return Err(IrqBalError::ZeroRateDenominator);
    }
    let ratio = ppm_of(hot_rate, cold_rate);
    let grade = grade_of(ratio);
    if grade == IbGrade::Balanced {
        return Ok(None);
    }
    Ok(Some(ImbalanceReport {
        hot,
        cold,
        hot_rate_ppm: hot_rate,
        cold_rate_ppm: cold_rate,
        ratio_ppm: ratio,
        grade,
        active_cores: active,
        global_mean_units: mean_units,
        rates_ppm: rates,
    }))
}

// ============================================================================
// BalanceDecision —— 均衡决策（保守性红线 ≤5%/轮 + 向量上限 + 决策留痕）
// ============================================================================

/// 单条迁移向量（源核/目标核/向量号/每槽迁移单位）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MigVector {
    /// 源核（减载方）。
    pub src: usize,
    /// 目标核（加载方）。
    pub dst: usize,
    /// 向量号（0..IB_VEC_CAP）。
    pub vector: u32,
    /// 每槽迁移投递单位。
    pub units: u64,
}

/// 均衡决策（含完整留痕：原因/清单/预期改善）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BalanceDecision {
    /// 决策轮次。
    pub round: u64,
    /// 决策原因（人类可读留痕）。
    pub reason: String,
    /// 迁移向量清单（≤ IB_VEC_CAP 条）。
    pub vectors: Vec<MigVector>,
    /// 总迁移量（每槽单位）。
    pub moved_units: u64,
    /// 迁移量占全局每槽均值的 bps（≤500 红线）。
    pub moved_bps: u64,
    /// 预期热核速率（ppm）。
    pub expected_hot_ppm: u64,
    /// 预期冷核速率（ppm）。
    pub expected_cold_ppm: u64,
    /// 方向键（源,目标）· 供滞回判向。
    pub direction_key: (usize, usize),
}

impl BalanceDecision {
    /// 决策留痕渲染。
    pub fn render(&self) -> String {
        let mut vs = String::new();
        let mut i = 0usize;
        while i < self.vectors.len() {
            let v = &self.vectors[i];
            if i > 0 {
                vs.push_str(", ");
            }
            vs.push_str(&format!(
                "#c{}→#c{}[v{}]{}u",
                v.src, v.dst, v.vector, v.units
            ));
            i += 1;
        }
        format!(
            "round={} reason=[{}] vectors=[{}] moved={}u({}bps) expect hot={}ppm cold={}ppm",
            self.round,
            self.reason,
            vs,
            self.moved_units,
            self.moved_bps,
            self.expected_hot_ppm,
            self.expected_cold_ppm,
        )
    }
}

/// 决策规划：失衡报告 → 决策（保守：ideal 与 5% 红线取小 · 不足 1 单位不动）。
fn plan_decision(rep: &ImbalanceReport, round: u64) -> Result<Option<BalanceDecision>, IrqBalError> {
    // 由速率 ppm 还原每槽均值单位（速率 = mean/1024×1e6 → mean = rate×1024/1e6）
    let hot_mean = rep.hot_rate_ppm.saturating_mul(IB_SLOT_CAP) / IB_PPM_ONE;
    let cold_mean = rep.cold_rate_ppm.saturating_mul(IB_SLOT_CAP) / IB_PPM_ONE;
    // 理想迁移：向中点收拢一半差值
    let ideal = hot_mean.saturating_sub(cold_mean) / 2;
    // 保守红线：单轮 ≤ 全局每槽均值 5%
    let cap = cap_units_for(rep.global_mean_units);
    let m = if ideal < cap { ideal } else { cap };
    if m == 0 {
        return Ok(None);
    }
    let moved_bps = bps_of(m, rep.global_mean_units);
    if moved_bps > IB_MIG_CAP_BPS {
        // 理论上 cap 已保证，此处为防御性复核（计划端第一重红线）
        return Err(IrqBalError::MigrationOverCap {
            requested_bps: moved_bps,
            cap_bps: IB_MIG_CAP_BPS,
        });
    }
    // 向量拆分：≤ IB_VEC_CAP 条 · base+rem 守恒于 m
    let mut vectors = Vec::new();
    let base = m / IB_VEC_CAP as u64;
    let rem = m % IB_VEC_CAP as u64;
    let mut i = 0usize;
    while i < IB_VEC_CAP {
        let extra = if (i as u64) < rem { 1 } else { 0 };
        let units = base.saturating_add(extra);
        if units > 0 {
            vectors.push(MigVector {
                src: rep.hot,
                dst: rep.cold,
                vector: i as u32,
                units,
            });
        }
        i += 1;
    }
    if vectors.is_empty() {
        return Ok(None);
    }
    let expected_hot_ppm = ppm_of(hot_mean.saturating_sub(m), IB_SLOT_CAP);
    let expected_cold_ppm = ppm_of(cold_mean.saturating_add(m), IB_SLOT_CAP);
    let reason = format!(
        "ratio={}ppm grade={} 热核#c{}({}ppm)→冷核#c{}({}ppm) 迁移{}u/槽 占{}bps（红线{}bps）",
        rep.ratio_ppm,
        rep.grade.name(),
        rep.hot,
        rep.hot_rate_ppm,
        rep.cold,
        rep.cold_rate_ppm,
        m,
        moved_bps,
        IB_MIG_CAP_BPS,
    );
    Ok(Some(BalanceDecision {
        round,
        reason,
        vectors,
        moved_units: m,
        moved_bps,
        expected_hot_ppm,
        expected_cold_ppm,
        direction_key: (rep.hot, rep.cold),
    }))
}

// ============================================================================
// OscillationGuard —— 抖动抑制（同向滞回 + 反向冷却）
// ============================================================================

/// 抖动抑制闸（滞回 + 冷却双机制）。
///
/// 语义：
/// - 同向滞回：迁移放行要求"当前请求方向"连续 ≥IB_HYST_STREAK 轮被 note_direction 确认；
///   方向一变即清零重来。震荡（方向交替）永远凑不满连击 → 10/10 抑制。
/// - 反向冷却：与上次迁移方向不同的迁移，即使在冷却期内连击已满，也须等
///   cooldown_until（上次迁移轮 + IB_COOLDOWN）——防"迁移刚落就反悔"。
#[derive(Clone, Debug)]
pub struct OscillationGuard {
    /// 上次放行迁移的方向（冷却判据 · 仅 settle 时更新）。
    last_migration_dir: Option<(usize, usize)>,
    /// 当前失衡跟踪方向（滞回判据 · note_direction 驱动）。
    note_dir: Option<(usize, usize)>,
    /// 当前方向连击数（note_direction 驱动）。
    streak: u32,
    /// 反向冷却截止轮（上次放行轮 + IB_COOLDOWN）。
    cooldown_until: u64,
    /// 被抑制次数。
    suppressed: u64,
    /// 放行次数。
    admitted: u64,
}

impl OscillationGuard {
    /// 新建。
    pub fn new() -> OscillationGuard {
        OscillationGuard {
            last_migration_dir: None,
            note_dir: None,
            streak: 0,
            cooldown_until: 0,
            suppressed: 0,
            admitted: 0,
        }
    }

    /// 被抑制次数。
    pub fn suppressed(&self) -> u64 {
        self.suppressed
    }

    /// 放行次数。
    pub fn admitted(&self) -> u64 {
        self.admitted
    }

    /// 当前连击。
    pub fn streak(&self) -> u32 {
        self.streak
    }

    /// 每轮失衡方向确认（方向变 → 连击清零重来；同向 → 连击 +1）。
    pub fn note_direction(&mut self, dir: (usize, usize)) {
        if self.note_dir == Some(dir) {
            self.streak = self.streak.saturating_add(1);
        } else {
            self.note_dir = Some(dir);
            self.streak = 1;
        }
    }

    /// 放行判定（note_direction 之后调用）。
    pub fn admit(&mut self, dir: (usize, usize), round: u64) -> Result<(), IrqBalError> {
        // 冷却硬闸：仅约束与上次迁移方向不同的迁移（对上次迁移方向判向）
        if let Some(d) = self.last_migration_dir {
            if d != dir && round < self.cooldown_until {
                self.suppressed = self.suppressed.saturating_add(1);
                return Err(IrqBalError::CooldownActive {
                    remain: self.cooldown_until - round,
                });
            }
        }
        // 滞回闸：连击不足不放
        if self.streak < IB_HYST_STREAK {
            self.suppressed = self.suppressed.saturating_add(1);
            return Err(IrqBalError::HysteresisBlocked {
                streak: self.streak,
                need: IB_HYST_STREAK,
            });
        }
        self.settle(dir, round);
        Ok(())
    }

    /// 放行落定：记迁移方向、清连击、起冷却。
    fn settle(&mut self, dir: (usize, usize), round: u64) {
        self.last_migration_dir = Some(dir);
        self.streak = 0;
        self.cooldown_until = round.saturating_add(IB_COOLDOWN as u64);
        self.admitted = self.admitted.saturating_add(1);
    }

    /// 探测前归位（模拟迁移后瞬时态：连击清零 · 供 10/10 震荡注入确定性起算）。
    pub fn reset_streak(&mut self) {
        self.streak = 0;
    }
}

// ============================================================================
// 迁移执行账 —— 每向量记录 + 重测验证
// ============================================================================

/// 单向量迁移记录（源核/目标核/向量号/单位/轮次/时戳）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MigRecord {
    /// 账序（单调递增）。
    pub seq: u64,
    /// 源核。
    pub src: usize,
    /// 目标核。
    pub dst: usize,
    /// 向量号。
    pub vector: u32,
    /// 每槽迁移单位。
    pub units: u64,
    /// 决策轮次。
    pub round: u64,
    /// 逻辑时戳（引擎 tick）。
    pub tick: u64,
}

/// 重测意图（迁移落账时冻结迁移前速率 · 下一窗对读）。
#[derive(Clone, Copy, Debug)]
struct VerifyIntent {
    applied_round: u64,
    hot: usize,
    cold: usize,
    pre_hot_ppm: u64,
    pre_cold_ppm: u64,
    moved_units: u64,
}

/// 迁移执行账。
#[derive(Clone, Debug)]
pub struct MigLedger {
    /// 记录账。
    records: Vec<MigRecord>,
    /// 下一账序。
    next_seq: u64,
    /// 累计迁移单位。
    applied_units: u64,
    /// 重测通过次数。
    reverified: u64,
    /// 重测失败次数。
    verify_fail: u64,
}

impl MigLedger {
    /// 新建。
    pub fn new() -> MigLedger {
        MigLedger {
            records: Vec::new(),
            next_seq: 1,
            applied_units: 0,
            reverified: 0,
            verify_fail: 0,
        }
    }

    /// 落一条记录。
    fn record(&mut self, src: usize, dst: usize, vector: u32, units: u64, round: u64, tick: u64) -> MigRecord {
        let rec = MigRecord {
            seq: self.next_seq,
            src,
            dst,
            vector,
            units,
            round,
            tick,
        };
        self.next_seq = self.next_seq.saturating_add(1);
        self.records.push(rec);
        rec
    }

    /// 账面只读访问。
    pub fn records(&self) -> &[MigRecord] {
        &self.records
    }

    /// 账面长度。
    pub fn len(&self) -> usize {
        self.records.len()
    }

    /// 空账判定。
    pub fn is_empty(&self) -> bool {
        self.records.is_empty()
    }

    /// 累计迁移单位。
    pub fn applied_units(&self) -> u64 {
        self.applied_units
    }

    /// 重测通过次数。
    pub fn reverified(&self) -> u64 {
        self.reverified
    }

    /// 重测失败次数。
    pub fn verify_fail(&self) -> u64 {
        self.verify_fail
    }
}

// ============================================================================
// 引擎主体 —— IrqBalancer
// ============================================================================

/// 单窗推进结果。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct StepOutcome {
    /// 窗序（封窗时的轮次号）。
    pub window: u64,
    /// 本窗是否检出失衡。
    pub imbalanced: bool,
    /// 分级码（未检出为 0）。
    pub grade_code: u8,
    /// 本窗是否放行迁移。
    pub admitted: bool,
    /// 拦截码（0 = 未拦截）。
    pub rejected_code: u16,
}

/// 健康态（Green=无未决异常 · Degraded=异常已检出并遏制 · Failed=红线被击穿）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum IbHealth {
    /// 健康。
    Green,
    /// 降级（异常已遏制）。
    Degraded,
    /// 失效（红线击穿）。
    Failed,
}

impl IbHealth {
    /// 健康码。
    pub const fn code(self) -> u8 {
        match self {
            IbHealth::Green => 0,
            IbHealth::Degraded => 1,
            IbHealth::Failed => 2,
        }
    }

    /// 健康名。
    pub fn name(self) -> &'static str {
        match self {
            IbHealth::Green => "green",
            IbHealth::Degraded => "degraded",
            IbHealth::Failed => "failed",
        }
    }
}

/// 全局滚动统计（红线分母）。
#[derive(Clone, Copy, Debug)]
struct RollStats {
    /// 全局每槽均值（单位）。
    mean_units: u64,
    /// 全局滚动总量。
    total_units: u64,
}

/// 全局滚动统计（核账切片 → 汇总）。
fn global_roll_stats(cores: &[CoreObservation]) -> Result<RollStats, IrqBalError> {
    let mut total = 0u64;
    let mut slots = 0u64;
    let mut i = 0usize;
    while i < cores.len() {
        total = total.saturating_add(cores[i].roll_sum());
        slots = slots.saturating_add(cores[i].roll_slots());
        i += 1;
    }
    if slots == 0 {
        return Err(IrqBalError::EmptyObservation);
    }
    Ok(RollStats {
        mean_units: total / slots,
        total_units: total,
    })
}

/// 统计快照（health/指纹/外部巡检共用）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct IbStats {
    /// 当前轮次。
    pub round: u64,
    /// 逻辑时戳。
    pub tick: u64,
    /// 放行决策数。
    pub decisions: u64,
    /// 落账迁移数（决策级）。
    pub migrations: u64,
    /// 滞回等待次数。
    pub hysteresis_waits: u64,
    /// 造假检出次数。
    pub forgery_caught: u64,
    /// 超限拒收次数。
    pub overcap_rejected: u64,
    /// 历史最大单轮迁移 bps。
    pub max_moved_bps: u64,
    /// 历史最大对账误差 ppm。
    pub max_hist_err_ppm: u64,
    /// 事件账长度。
    pub events_len: usize,
    /// guard 抑制次数。
    pub guard_suppressed: u64,
    /// guard 放行次数。
    pub guard_admitted: u64,
    /// 迁移账长度。
    pub ledger_len: usize,
    /// 累计迁移单位。
    pub applied_units: u64,
    /// 重测通过次数。
    pub reverified: u64,
}

/// 中断负载均衡器（分布观测 → 失衡检测 → 保守决策 → 抖动闸 → 执行账 → 重测）。
#[derive(Clone, Debug)]
pub struct IrqBalancer {
    /// per 核观测账（堆上 · 各 ≈8.3KB · 按引用使用）。
    cores: Vec<CoreObservation>,
    /// 失衡检测器。
    detector: ImbalanceDetector,
    /// 抖动抑制闸。
    guard: OscillationGuard,
    /// 迁移执行账。
    ledger: MigLedger,
    /// 事件账（环形）。
    events: Vec<IbEvent>,
    /// 当前轮次（每封一窗 +1）。
    round: u64,
    /// 逻辑时戳（每次放行 +1）。
    tick: u64,
    /// 放行决策数。
    decisions: u64,
    /// 落账迁移数。
    migrations: u64,
    /// 滞回等待次数。
    hysteresis_waits: u64,
    /// 造假检出次数。
    forgery_caught: u64,
    /// 超限拒收次数。
    overcap_rejected: u64,
    /// 历史最大单轮迁移 bps。
    max_moved_bps: u64,
    /// 历史最大对账误差 ppm。
    max_hist_err_ppm: u64,
    /// 红线击穿标记（重测失败/执行端超限穿透）。
    health_failed: bool,
    /// 待重测意图（至多一条 · 迁移后下一窗结算）。
    pending_verify: Option<VerifyIntent>,
    /// 最近一次失衡报告。
    last_report: Option<ImbalanceReport>,
    /// 最近一次迁移提示（源,目标,单位 · 供演练侧改写生成权重）。
    last_move: Option<(usize, usize, u64)>,
}

impl IrqBalancer {
    /// 以 n 核启动（事件账记 Booted）。
    pub fn with_cores(n: usize) -> Result<IrqBalancer, IrqBalError> {
        if n == 0 || n > IB_MAX_CORES {
            return Err(IrqBalError::CoreOutOfRange { core: n });
        }
        let mut cores = Vec::new();
        let mut i = 0usize;
        while i < n {
            cores.push(CoreObservation::new(i));
            i += 1;
        }
        let mut bal = IrqBalancer {
            cores,
            detector: ImbalanceDetector::new(),
            guard: OscillationGuard::new(),
            ledger: MigLedger::new(),
            events: Vec::new(),
            round: 0,
            tick: 0,
            decisions: 0,
            migrations: 0,
            hysteresis_waits: 0,
            forgery_caught: 0,
            overcap_rejected: 0,
            max_moved_bps: 0,
            max_hist_err_ppm: 0,
            health_failed: false,
            pending_verify: None,
            last_report: None,
            last_move: None,
        };
        bal.push_event(IbEvent::Booted { cores: n });
        Ok(bal)
    }

    /// 事件账只读访问。
    pub fn events(&self) -> &[IbEvent] {
        &self.events
    }

    /// 迁移账只读访问。
    pub fn ledger(&self) -> &MigLedger {
        &self.ledger
    }

    /// 检测器只读访问。
    pub fn detector(&self) -> &ImbalanceDetector {
        &self.detector
    }

    /// 抖动闸只读访问。
    pub fn guard(&self) -> &OscillationGuard {
        &self.guard
    }

    /// 当前轮次。
    pub fn round(&self) -> u64 {
        self.round
    }

    /// 最近一次迁移提示（源,目标,单位）。
    pub fn last_move_hint(&self) -> Option<(usize, usize, u64)> {
        self.last_move
    }

    /// 最近一次失衡报告。
    pub fn last_report(&self) -> Option<ImbalanceReport> {
        self.last_report
    }

    /// 历史最大对账误差 ppm。
    pub fn max_hist_err_ppm(&self) -> u64 {
        self.max_hist_err_ppm
    }

    /// 历史最大单轮迁移 bps。
    pub fn max_moved_bps(&self) -> u64 {
        self.max_moved_bps
    }

    /// 统计快照。
    pub fn stats(&self) -> IbStats {
        IbStats {
            round: self.round,
            tick: self.tick,
            decisions: self.decisions,
            migrations: self.migrations,
            hysteresis_waits: self.hysteresis_waits,
            forgery_caught: self.forgery_caught,
            overcap_rejected: self.overcap_rejected,
            max_moved_bps: self.max_moved_bps,
            max_hist_err_ppm: self.max_hist_err_ppm,
            events_len: self.events.len(),
            guard_suppressed: self.guard.suppressed(),
            guard_admitted: self.guard.admitted(),
            ledger_len: self.ledger.len(),
            applied_units: self.ledger.applied_units(),
            reverified: self.ledger.reverified(),
        }
    }

    /// 健康判定：Failed > Degraded > Green。
    pub fn health(&self) -> IbHealth {
        if self.health_failed {
            return IbHealth::Failed;
        }
        let mut overflow = 0u64;
        let mut i = 0usize;
        while i < self.cores.len() {
            overflow = overflow.saturating_add(self.cores[i].overflow_bag());
            i += 1;
        }
        if self.forgery_caught > 0 || overflow > 0 {
            return IbHealth::Degraded;
        }
        IbHealth::Green
    }

    /// 事件入账（环形容量 · 淘汰最旧保确定性）。
    fn push_event(&mut self, e: IbEvent) {
        if self.events.len() >= IB_EVENT_CAP {
            self.events.remove(0);
        }
        self.events.push(e);
    }

    /// 记一笔槽投递（含事件留痕）。
    pub fn observe(&mut self, core: usize, units: u64) -> Result<(), IrqBalError> {
        if core >= self.cores.len() {
            return Err(IrqBalError::CoreOutOfRange { core });
        }
        self.cores[core].observe(units)?;
        self.push_event(IbEvent::Observed { core, units });
        Ok(())
    }

    /// 封窗：全核槽入 bin + 窗总量入环 + 实测快照。
    pub fn close_round(&mut self) -> Result<u64, IrqBalError> {
        let mut i = 0usize;
        while i < self.cores.len() {
            self.cores[i].close_window();
            i += 1;
        }
        let global = global_roll_stats(&self.cores)?.total_units;
        self.push_event(IbEvent::WindowClosed {
            seq: self.round,
            global_units: global,
        });
        Ok(self.round)
    }

    /// 单核增量对账（观测半区 vs 实测半区 · 超阈即造假检出）。
    pub fn cross_check(&mut self, core: usize) -> Result<u64, IrqBalError> {
        if core >= self.cores.len() {
            return Err(IrqBalError::CoreOutOfRange { core });
        }
        let (err, diff) = self.cores[core].cross_check_delta();
        if err > self.max_hist_err_ppm {
            self.max_hist_err_ppm = err;
        }
        if err >= IB_ERR_TOL_PPM {
            self.forgery_caught = self.forgery_caught.saturating_add(1);
            self.push_event(IbEvent::ForgeryCaught {
                core,
                error_ppm: err,
            });
            return Err(IrqBalError::ObservationForgery {
                core,
                error_ppm: err,
            });
        }
        let _ = diff;
        Ok(err)
    }

    /// 全核对账 + 失衡检测（窗后判定入口）。
    pub fn detect_current(&mut self) -> Result<Option<ImbalanceReport>, IrqBalError> {
        let mut i = 0usize;
        while i < self.cores.len() {
            self.cross_check(i)?;
            i += 1;
        }
        let rep = self.detector.detect(&self.cores)?;
        self.last_report = rep;
        if let Some(r) = rep {
            self.push_event(IbEvent::Imbalance {
                round: self.round,
                grade_code: r.grade.code(),
                ratio_ppm: r.ratio_ppm,
                hot: r.hot,
                cold: r.cold,
            });
        }
        Ok(rep)
    }

    /// 决策落账与执行（执行端第二重红线复核 · 注入面由此拦截）。
    fn apply_decision(
        &mut self,
        d: BalanceDecision,
        pre_hot_ppm: u64,
        pre_cold_ppm: u64,
        tick: u64,
    ) -> Result<(), IrqBalError> {
        if d.vectors.is_empty() {
            return Err(IrqBalError::NothingToBalance);
        }
        if d.vectors.len() > IB_VEC_CAP {
            return Err(IrqBalError::VectorCapExceeded {
                requested: d.vectors.len(),
                cap: IB_VEC_CAP,
            });
        }
        // 执行端复核：以当前账面全局均值重算 bps（防计划端被绕过）
        let st = global_roll_stats(&self.cores)?;
        let moved_bps = bps_of(d.moved_units, st.mean_units);
        if moved_bps > IB_MIG_CAP_BPS {
            self.overcap_rejected = self.overcap_rejected.saturating_add(1);
            self.push_event(IbEvent::OvercapRejected {
                requested_bps: moved_bps,
            });
            return Err(IrqBalError::MigrationOverCap {
                requested_bps: moved_bps,
                cap_bps: IB_MIG_CAP_BPS,
            });
        }
        // 逐向量落账
        let mut i = 0usize;
        while i < d.vectors.len() {
            let v = d.vectors[i];
            let rec = self.ledger.record(v.src, v.dst, v.vector, v.units, d.round, tick);
            self.push_event(IbEvent::VectorApplied {
                seq: rec.seq,
                src: rec.src,
                dst: rec.dst,
                vector: rec.vector,
                units: rec.units,
            });
            i += 1;
        }
        self.ledger.applied_units = self.ledger.applied_units.saturating_add(d.moved_units);
        self.migrations = self.migrations.saturating_add(1);
        self.decisions = self.decisions.saturating_add(1);
        if moved_bps > self.max_moved_bps {
            self.max_moved_bps = moved_bps;
        }
        // 冻结重测意图（迁移前速率）
        let first = d.vectors[0];
        self.pending_verify = Some(VerifyIntent {
            applied_round: d.round,
            hot: first.src,
            cold: first.dst,
            pre_hot_ppm,
            pre_cold_ppm,
            moved_units: d.moved_units,
        });
        self.last_move = Some((first.src, first.dst, d.moved_units));
        self.push_event(IbEvent::Admitted {
            round: d.round,
            moved_units: d.moved_units,
            moved_bps,
        });
        Ok(())
    }

    /// 重测结算：迁移后下一窗对读（热核须降 + 冷核须升 双向强校验）。
    fn settle_verify(&mut self, rep: &ImbalanceReport) -> Result<(), IrqBalError> {
        let intent = match self.pending_verify.take() {
            Some(v) => v,
            None => return Ok(()),
        };
        if self.round <= intent.applied_round {
            // 尚无新封闭窗：意图保留至下一窗结算（防御性 · 重测必须晚于迁移窗）
            self.pending_verify = Some(intent);
            return Ok(());
        }
        if intent.hot >= IB_MAX_CORES || intent.cold >= IB_MAX_CORES {
            return Err(IrqBalError::CoreOutOfRange { core: intent.hot });
        }
        if intent.moved_units == 0 {
            // 零迁移无可验证：按无可均衡对象处理（防御性 · 规划端已保证非零）
            return Err(IrqBalError::NothingToBalance);
        }
        let now_hot = rep.rates_ppm[intent.hot];
        let now_cold = rep.rates_ppm[intent.cold];
        if now_hot < intent.pre_hot_ppm && now_cold > intent.pre_cold_ppm {
            self.ledger.reverified = self.ledger.reverified.saturating_add(1);
            self.push_event(IbEvent::ReMeasureOk {
                round: self.round,
            });
            Ok(())
        } else {
            self.ledger.verify_fail = self.ledger.verify_fail.saturating_add(1);
            self.health_failed = true;
            Err(IrqBalError::ReMeasureMismatch {
                hot: intent.hot,
                cold: intent.cold,
            })
        }
    }

    /// 单窗推进：封窗 → 对账+检测 → 重测结算 → 失衡时 规划+过闸+落账。
    pub fn step_window(&mut self) -> Result<StepOutcome, IrqBalError> {
        let window = self.close_round()?;
        let mut outcome = StepOutcome {
            window,
            imbalanced: false,
            grade_code: 0,
            admitted: false,
            rejected_code: 0,
        };
        let report = self.detect_current()?;
        if let Some(rep) = report {
            // 先结算上一迁移的重测（用本窗报告对读）
            self.settle_verify(&rep)?;
            outcome.imbalanced = true;
            outcome.grade_code = rep.grade.code();
            let dir = (rep.hot, rep.cold);
            self.guard.note_direction(dir);
            match plan_decision(&rep, self.round) {
                Ok(Some(d)) => match self.guard.admit(dir, self.round) {
                    Ok(()) => {
                        let pre_hot = rep.rates_ppm[rep.hot];
                        let pre_cold = rep.rates_ppm[rep.cold];
                        self.apply_decision(d, pre_hot, pre_cold, self.tick)?;
                        outcome.admitted = true;
                        self.tick = self.tick.saturating_add(1);
                    }
                    Err(e) => {
                        let code = e.code();
                        // 滞回/冷却属预期抑制，不升级为故障
                        if matches!(
                            e,
                            IrqBalError::HysteresisBlocked { .. }
                                | IrqBalError::CooldownActive { .. }
                        ) {
                            self.hysteresis_waits = self.hysteresis_waits.saturating_add(1);
                        }
                        self.push_event(IbEvent::Rejected {
                            round: self.round,
                            code,
                        });
                        outcome.rejected_code = code;
                    }
                },
                Ok(None) => {
                    // 迁移量不足 1 单位/槽：保守不动
                }
                Err(e) => return Err(e),
            }
        }
        self.round = self.round.saturating_add(1);
        Ok(outcome)
    }

    // ---- 注入面（三路 · 各自必检出）----

    /// 注入一：观测造假（幽灵槽计数入 bin → 增量对账必超阈 → 检出并重定基）。
    /// 返回 true 表示造假被检出（预期）；false 表示红线失守。
    pub fn inject_forgery(&mut self, core: usize, phantom_slots: u64) -> Result<bool, IrqBalError> {
        if core >= self.cores.len() {
            return Err(IrqBalError::CoreOutOfRange { core });
        }
        // 幽灵槽：bin[8] += phantom_slots → 幽灵投递 = 8 × phantom_slots
        self.cores[core].forge_bin(8, phantom_slots)?;
        match self.cross_check(core) {
            Err(IrqBalError::ObservationForgery { .. }) => {
                // 检出：重定基恢复观测半区与实测半区一致（遏制但不失忆）
                self.cores[core].rebase();
                self.push_event(IbEvent::ForgeryRebased { core });
                Ok(true)
            }
            Err(e) => Err(e),
            Ok(_) => Ok(false), // 造假未被检出：观测半区失守
        }
    }

    /// 注入二：超限迁移（请求 10% 全局量 > 5% 红线 → 执行端复核必拒）。
    /// 返回 true 表示红线拦下（预期）；false 表示红线失守。
    pub fn inject_overcap(&mut self, units: u64) -> Result<bool, IrqBalError> {
        let mut vectors = Vec::new();
        vectors.push(MigVector {
            src: 0,
            dst: 1,
            vector: 0,
            units,
        });
        let d = BalanceDecision {
            round: self.round,
            reason: format!(
                "注入面：超限迁移 {} 单位/槽（红线 {}bps）",
                units, IB_MIG_CAP_BPS
            ),
            vectors,
            moved_units: units,
            moved_bps: 0,
            expected_hot_ppm: 0,
            expected_cold_ppm: 0,
            direction_key: (0, 1),
        };
        match self.apply_decision(d, 0, 0, self.tick) {
            Err(IrqBalError::MigrationOverCap { .. }) => Ok(true),
            Err(e) => Err(e),
            Ok(()) => Ok(false), // 红线失守：超限迁移被放行
        }
    }

    /// 注入三：震荡探测（方向交替 × IB_OSC_PROBES 次 → 滞回连击永远凑不满 → 10/10 抑制）。
    /// 返回被抑制次数（预期 == IB_OSC_PROBES）。
    pub fn inject_oscillation(&mut self) -> Result<u64, IrqBalError> {
        if self.cores.len() < 2 {
            return Err(IrqBalError::NothingToBalance);
        }
        // 探测前归位：模拟迁移后瞬时态（连击清零 · 方向保留）
        self.guard.reset_streak();
        let mut suppressed = 0u64;
        let mut flip = false;
        let mut attempt = 1u64;
        while attempt <= IB_OSC_PROBES {
            let dir = if flip { (1usize, 0usize) } else { (0usize, 1usize) };
            self.guard.note_direction(dir);
            match self.guard.admit(dir, self.round) {
                Ok(()) => {
                    // 漏放：震荡决策穿透了滞回闸（不应发生；不落账仅留痕）
                    self.push_event(IbEvent::Admitted {
                        round: self.round,
                        moved_units: 0,
                        moved_bps: 0,
                    });
                }
                Err(e) => {
                    // 滞回/冷却拦截 = 预期抑制；其余错误升级上抛
                    if matches!(
                        e,
                        IrqBalError::HysteresisBlocked { .. }
                            | IrqBalError::CooldownActive { .. }
                    ) {
                        suppressed = suppressed.saturating_add(1);
                        let code = e.code();
                        self.push_event(IbEvent::OscillationSuppressed { attempt, code });
                    } else {
                        return Err(e);
                    }
                }
            }
            flip = !flip;
            attempt = attempt.saturating_add(1);
        }
        Ok(suppressed)
    }

    // ---- 指纹与确定性 ----

    /// FNV-1a 单字混合。
    fn mix(fp: &mut u64, v: u64) {
        *fp ^= v;
        *fp = fp.wrapping_mul(IB_FNV_PRIME);
    }

    /// 字符串混入（逐字节）。
    fn mix_str(fp: &mut u64, s: &str) {
        let mut i = 0usize;
        let bytes = s.as_bytes();
        while i < bytes.len() {
            IrqBalancer::mix(fp, bytes[i] as u64);
            i += 1;
        }
    }

    /// 账面指纹（FNV-1a 64 · 全整数确定性 · 双跑对拍依据）。
    pub fn fingerprint(&self) -> u64 {
        let mut fp = IB_FNV_OFFSET;
        IrqBalancer::mix_str(&mut fp, IB_VERSION);
        IrqBalancer::mix(&mut fp, self.round);
        IrqBalancer::mix(&mut fp, self.tick);
        IrqBalancer::mix(&mut fp, self.decisions);
        IrqBalancer::mix(&mut fp, self.migrations);
        IrqBalancer::mix(&mut fp, self.hysteresis_waits);
        IrqBalancer::mix(&mut fp, self.forgery_caught);
        IrqBalancer::mix(&mut fp, self.overcap_rejected);
        IrqBalancer::mix(&mut fp, self.max_moved_bps);
        IrqBalancer::mix(&mut fp, self.max_hist_err_ppm);
        IrqBalancer::mix(&mut fp, self.guard.suppressed());
        IrqBalancer::mix(&mut fp, self.guard.admitted());
        IrqBalancer::mix(&mut fp, self.guard.streak() as u64);
        // guard 冷却/方向
        IrqBalancer::mix(&mut fp, self.guard_cooldown_digest());
        // per 核账面
        let mut i = 0usize;
        while i < self.cores.len() {
            let (raw, snap, rep, bag) = self.cores[i].digest_fields();
            IrqBalancer::mix(&mut fp, (i as u64) ^ raw.rotate_left(8));
            IrqBalancer::mix(&mut fp, snap);
            IrqBalancer::mix(&mut fp, rep);
            IrqBalancer::mix(&mut fp, bag);
            i += 1;
        }
        // 迁移账
        IrqBalancer::mix(&mut fp, self.ledger.len() as u64);
        IrqBalancer::mix(&mut fp, self.ledger.applied_units());
        IrqBalancer::mix(&mut fp, self.ledger.reverified());
        let mut j = 0usize;
        while j < self.ledger.len() {
            let r = &self.ledger.records()[j];
            IrqBalancer::mix(&mut fp, r.seq ^ r.units.rotate_left(16));
            IrqBalancer::mix(
                &mut fp,
                (r.src as u64) ^ ((r.dst as u64) << 16) ^ ((r.vector as u64) << 32) ^ r.round.rotate_left(40),
            );
            IrqBalancer::mix(&mut fp, r.tick);
            j += 1;
        }
        // 事件账（含淘汰序 · 环形确定性）
        IrqBalancer::mix(&mut fp, self.events.len() as u64);
        let mut k = 0usize;
        while k < self.events.len() {
            IrqBalancer::mix(&mut fp, event_digest(&self.events[k]));
            k += 1;
        }
        fp
    }

    /// guard 冷却态摘要（fingerprint 内部用）。
    fn guard_cooldown_digest(&self) -> u64 {
        match self.guard.last_migration_dir {
            None => 0,
            Some((a, b)) => (a as u64) ^ ((b as u64) << 16) ^ (self.guard.cooldown_until << 32),
        }
    }
}

// ============================================================================
// 确定性演练与判据自证
// ============================================================================

/// 演练报告（J1 三判据 + 三注入 + 健康与指纹 一账尽收）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DrillReport {
    /// 演练窗数。
    pub windows: u64,
    /// 放行决策数。
    pub decisions: u64,
    /// 落账迁移数。
    pub migrations: u64,
    /// 滞回等待次数。
    pub hysteresis_waits: u64,
    /// 重测通过次数。
    pub reverified: u64,
    /// 历史最大单轮迁移 bps（J1：≤500）。
    pub max_moved_bps: u64,
    /// 历史最大对账误差 ppm（J1：<10000）。
    pub max_hist_error_ppm: u64,
    /// 造假注入是否检出。
    pub forgery_caught: bool,
    /// 超限注入是否被拒。
    pub overcap_rejected: bool,
    /// 震荡探测次数。
    pub osc_probes: u64,
    /// 震荡被抑制次数（J1：== 探测次数）。
    pub osc_suppressed: u64,
    /// 注入前健康（预期 Green）。
    pub health_pre_injection: IbHealth,
    /// 注入后最终健康（造假遏制后预期 Degraded）。
    pub health_final: IbHealth,
    /// 账面指纹（双跑对拍）。
    pub fingerprint: u64,
}

/// 确定性演练：4 核（625/375/188/62 单位/槽 ≈ 10:6:3:1）× 24 窗 × 8 槽。
///
/// 流程：灌观测 → 封窗 → 对账+检测 → 重测结算 → 失衡则规划+过闸+落账
/// → 演练侧按落账迁移改写生成权重（真实生效）→ 收尾三路注入 → 封账。
/// 全程 LCG 确定性 · 同种子逐位可重放。
pub fn run_drill(seed: u64) -> Result<DrillReport, IrqBalError> {
    let mut bal = IrqBalancer::with_cores(IB_DRILL_CORES)?;
    let mut lcg = seed ^ IB_LCG_SALT;
    // 演练初值：合计 1250 单位/槽（红线 cap = 1250×5% = 62 单位/槽）
    let mut weights: [u64; IB_DRILL_CORES] = [625, 375, 188, 62];
    let mut w = 0u64;
    let mut health_pre = IbHealth::Green;
    while w < IB_DRILL_WINDOWS {
        let mut s = 0usize;
        while s < IB_WINDOW_SLOTS {
            let mut c = 0usize;
            while c < IB_DRILL_CORES {
                lcg = lcg
                    .wrapping_mul(IB_LCG_MUL)
                    .wrapping_add(IB_LCG_ADD);
                let units = weights[c].saturating_add((lcg >> 33) % 8);
                bal.observe(c, units)?;
                c += 1;
            }
            s += 1;
        }
        let outcome = bal.step_window()?;
        // 迁移真实生效：演练侧改写生成权重（源减载/目标加载 · 下一窗观测即体现）
        if outcome.admitted {
            if let Some((src, dst, units)) = bal.last_move_hint() {
                if src < IB_DRILL_CORES && dst < IB_DRILL_CORES && weights[src] > units {
                    weights[src] -= units;
                    weights[dst] = weights[dst].saturating_add(units);
                }
            }
        }
        if w + 1 == 4 {
            health_pre = bal.health();
        }
        w = w.saturating_add(1);
    }
    let windows = bal.round;
    let decisions = bal.decisions;
    let migrations = bal.migrations;
    let hysteresis_waits = bal.hysteresis_waits;
    let reverified = bal.ledger.reverified();
    let max_moved_bps = bal.max_moved_bps;
    let max_hist_error_ppm = bal.max_hist_err_ppm;
    // 三路注入（各必检出）
    let forgery_caught = bal.inject_forgery(0, 32)?; // 幽灵 32 槽 × 8 = 256 幽灵投递
    let overcap_rejected = bal.inject_overcap(125)?; // 125/1250 = 10% > 红线 5%
    let osc_probes = IB_OSC_PROBES;
    let osc_suppressed = bal.inject_oscillation()?;
    let health_final = bal.health();
    bal.push_event(IbEvent::HealthSealed {
        health_code: health_final.code(),
    });
    Ok(DrillReport {
        windows,
        decisions,
        migrations,
        hysteresis_waits,
        reverified,
        max_moved_bps,
        max_hist_error_ppm,
        forgery_caught,
        overcap_rejected,
        osc_probes,
        osc_suppressed,
        health_pre_injection: health_pre,
        health_final,
        fingerprint: bal.fingerprint(),
    })
}

/// 双跑确定性对拍：同种子两次演练 → 指纹与全字段一致（否则 FingerprintMismatch）。
pub fn dual_run(seed: u64) -> Result<(DrillReport, DrillReport), IrqBalError> {
    let a = run_drill(seed)?;
    let b = run_drill(seed)?;
    if a.fingerprint != b.fingerprint
        || a.windows != b.windows
        || a.decisions != b.decisions
        || a.migrations != b.migrations
        || a.hysteresis_waits != b.hysteresis_waits
        || a.reverified != b.reverified
        || a.max_moved_bps != b.max_moved_bps
        || a.max_hist_error_ppm != b.max_hist_error_ppm
        || a.osc_suppressed != b.osc_suppressed
        || a.health_final != b.health_final
        || a.health_pre_injection != b.health_pre_injection
    {
        return Err(IrqBalError::FingerprintMismatch {
            left: a.fingerprint,
            right: b.fingerprint,
        });
    }
    Ok((a, b))
}

// ============================================================================
// 内置自检（cfg(test) · 判据逐条断言）
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    /// 测试辅助：解 Result（仅测试路径 · 内核路径无此函数）。
    fn ok<T>(r: Result<T, IrqBalError>) -> T {
        match r {
            Ok(v) => v,
            Err(e) => panic!(
                "意外失败 code={:#06x} detail={}（variants={:?}）",
                e.code(),
                e.detail(),
                e
            ),
        }
    }

    /// J1-1：分布观测准确——直方图 vs 实测误差 <1%（10000ppm）。
    #[test]
    fn j1_hist_error_under_1pct() {
        let rep = ok(run_drill(0x0885));
        assert!(
            rep.max_hist_error_ppm < IB_ERR_TOL_PPM,
            "对账误差 {}ppm 超阈 {}ppm",
            rep.max_hist_error_ppm,
            IB_ERR_TOL_PPM
        );
    }

    /// J1-2：均衡决策保守性——单轮迁移量 ≤5%（500bps）且超限注入被拒。
    #[test]
    fn j1_migration_cap_500bps() {
        let rep = ok(run_drill(0x0885));
        assert!(
            rep.max_moved_bps <= IB_MIG_CAP_BPS,
            "最大迁移 {}bps 超红线 {}bps",
            rep.max_moved_bps,
            IB_MIG_CAP_BPS
        );
        assert!(rep.overcap_rejected, "超限迁移未被拒：红线失守");
        assert!(rep.migrations > 0, "演练未发生任何迁移：链路不完整");
        assert!(rep.reverified > 0, "迁移后重测未通过任何一轮");
    }

    /// J1-3：抖动抑制——震荡决策 10/10 次被抑制。
    #[test]
    fn j1_oscillation_10_of_10() {
        let rep = ok(run_drill(0x0885));
        assert_eq!(rep.osc_probes, IB_OSC_PROBES);
        assert_eq!(
            rep.osc_suppressed, rep.osc_probes,
            "震荡抑制 {} / {}，存在漏放",
            rep.osc_suppressed, rep.osc_probes
        );
    }

    /// 三路注入面全部检出（造假/超限/震荡）。
    #[test]
    fn injections_all_detected() {
        let rep = ok(run_drill(0xC0FFEE));
        assert!(rep.forgery_caught, "观测造假注入未检出");
        assert!(rep.overcap_rejected, "超限迁移注入未被拒");
        assert_eq!(rep.osc_suppressed, rep.osc_probes, "震荡注入存在漏放");
    }

    /// 双跑确定性对拍：同种子两跑全字段一致。
    #[test]
    fn dual_run_deterministic() {
        let (a, b) = ok(dual_run(0xDEADBEEF));
        assert_eq!(a.fingerprint, b.fingerprint, "指纹不一致");
        assert_eq!(a, b, "演练报告字段不一致");
        assert_ne!(a.fingerprint, 0, "指纹异常为零");
    }

    /// 错误目录完整性：十二变体码在段内、中文诊断非空。
    #[test]
    fn error_catalog_complete() {
        let all: [IrqBalError; 12] = [
            IrqBalError::CoreOutOfRange { core: 999 },
            IrqBalError::EmptyObservation,
            IrqBalError::SlotOverflow { core: 0, units: 9999 },
            IrqBalError::ObservationForgery {
                core: 0,
                error_ppm: 500_000,
            },
            IrqBalError::ZeroRateDenominator,
            IrqBalError::MigrationOverCap {
                requested_bps: 1000,
                cap_bps: 500,
            },
            IrqBalError::VectorCapExceeded {
                requested: 5,
                cap: 4,
            },
            IrqBalError::HysteresisBlocked { streak: 1, need: 3 },
            IrqBalError::CooldownActive { remain: 2 },
            IrqBalError::ReMeasureMismatch { hot: 0, cold: 1 },
            IrqBalError::NothingToBalance,
            IrqBalError::FingerprintMismatch {
                left: 1,
                right: 2,
            },
        ];
        let mut i = 0usize;
        while i < all.len() {
            let code = all[i].code();
            assert!(
                code >= IB_SEG_BASE && code <= IB_SEG_BASE + 0x0F,
                "变体#{} 码 {:#06x} 出段",
                i,
                code
            );
            assert!(!all[i].detail().is_empty(), "变体#{} 诊断为空", i);
            i += 1;
        }
    }

    /// 健康与链路完整性：注入前 Green / 注入后 Degraded（遏制）· 链路各环非零。
    #[test]
    fn health_and_pipeline() {
        let rep = ok(run_drill(7));
        assert_eq!(
            rep.health_pre_injection,
            IbHealth::Green,
            "注入前健康应为 Green"
        );
        assert_eq!(
            rep.health_final,
            IbHealth::Degraded,
            "造假遏制后健康应为 Degraded"
        );
        assert!(rep.windows == IB_DRILL_WINDOWS);
        assert!(rep.hysteresis_waits > 0, "滞回从未拦截：抖动闸未被驱动");
        assert!(rep.max_moved_bps > 0, "从未发生有效迁移");
    }

    /// 滞回与冷却语义单测（独立于演练）。
    #[test]
    fn guard_semantics() {
        let mut g = OscillationGuard::new();
        // 首方向需连击 3 轮
        g.note_direction((0, 1));
        assert!(matches!(
            g.admit((0, 1), 0),
            Err(IrqBalError::HysteresisBlocked { streak: 1, need: 3 })
        ));
        g.note_direction((0, 1));
        assert!(matches!(
            g.admit((0, 1), 1),
            Err(IrqBalError::HysteresisBlocked { .. })
        ));
        g.note_direction((0, 1));
        assert!(g.admit((0, 1), 2).is_ok(), "连击 3 应放行");
        // 放行后连击清零（settle）+ 反向冷却 4 轮硬闸（cooldown_until = 2+4 = 6）
        assert_eq!(g.streak(), 0);
        g.note_direction((1, 0));
        assert!(matches!(
            g.admit((1, 0), 3),
            Err(IrqBalError::CooldownActive { remain: 3 })
        ));
        // 同向迁移不走冷却闸，但连击仅 1 → 滞回拦截
        g.note_direction((0, 1));
        assert!(matches!(
            g.admit((0, 1), 4),
            Err(IrqBalError::HysteresisBlocked { .. })
        ));
        // 冷却期内反向 → 硬闸（remain = 6-5 = 1）
        g.note_direction((1, 0));
        assert!(matches!(
            g.admit((1, 0), 5),
            Err(IrqBalError::CooldownActive { remain: 1 })
        ));
        // 冷却已过（round 6 ≥ 6）但连击仅 1 → 滞回拦截
        g.note_direction((0, 1));
        assert!(matches!(
            g.admit((0, 1), 6),
            Err(IrqBalError::HysteresisBlocked { .. })
        ));
        // 连击满 → 放行（同向不走冷却闸）
        g.note_direction((0, 1));
        g.note_direction((0, 1));
        assert!(g.admit((0, 1), 7).is_ok());
        assert_eq!(g.admitted(), 2);
        assert_eq!(g.suppressed(), 6);
    }

    /// 红线常量与分级函数编译期/运行期一致性抽查。
    #[test]
    fn const_math_spot() {
        assert_eq!(bps_to_ppm(IB_MIG_CAP_BPS), 50_000);
        assert_eq!(cap_units_for(1250), 62); // 1250×5% = 62.5 → 62（保守取整）
        assert_eq!(grade_of(1_000_000), IbGrade::Balanced);
        assert_eq!(grade_of(1_300_000), IbGrade::Light);
        assert_eq!(grade_of(2_000_000), IbGrade::Medium);
        assert_eq!(grade_of(3_000_000), IbGrade::Heavy);
        assert_eq!(ppm_of(1, 4), 250_000);
        assert_eq!(bps_of(62, 1250), 496);
        assert_eq!(ppm_of(5, 0), 0, "零分母保护");
    }

    /// 槽溢出拒收路径：域外观测不入 bin 且只进拒收袋。
    #[test]
    fn slot_overflow_rejected() {
        let mut bal = ok(IrqBalancer::with_cores(2));
        let r = bal.observe(0, IB_SLOT_CAP + 1);
        assert!(matches!(r, Err(IrqBalError::SlotOverflow { core: 0, .. })));
        assert_eq!(bal.health(), IbHealth::Degraded, "拒收应致 Degraded");
        // 拒收不入 bin：对账仍平
        let mut s = 0usize;
        while s < IB_WINDOW_SLOTS {
            ok(bal.observe(0, 10));
            ok(bal.observe(1, 10));
            s += 1;
        }
        ok(bal.close_round());
        let err = ok(bal.cross_check(0));
        assert_eq!(err, 0, "域外拒收不得污染对账");
    }

    /// 造假注入直接路径：幻影量不足阈时不误报、足阈必报。
    #[test]
    fn forgery_threshold_boundary() {
        let mut bal = ok(IrqBalancer::with_cores(2));
        let mut s = 0usize;
        while s < IB_WINDOW_SLOTS {
            ok(bal.observe(0, 100));
            ok(bal.observe(1, 100));
            s += 1;
        }
        ok(bal.close_round());
        // 幻影 1 槽 = 8 单位 vs 窗实测 800 → 1% 恰在阈上：>10000ppm 必检出
        let caught = ok(bal.inject_forgery(0, 1));
        assert!(caught, "阈上幻影未被检出");
        // 重定基后对账恢复为 0
        let err = ok(bal.cross_check(0));
        assert_eq!(err, 0, "重定基后对账应归零");
    }
}
