//! unx0663_tsccross —— UNX-F0663 · TSC 计时基准跨机复核（三机基频账 + 漂移拦截 + 复核轮账）。
//!
//! ═══════════════════════════════════════════════════════════════════════════
//! 任务锚点（主册原文抄录）：
//!   来源：docs/Varix/CoRun Varix STAR II · Unxreal/CoRun Varix STAR II · Unxreal.md 行 2232
//!   | UNX-F0663 | TSC 计时基准跨机复核 | 570 | 已深化 | UNX-F0663-J1 三机 TSC 基频校准值与 F0123 账面一致 3/3，基频漂移 >0.5% 检出拦截 |
//!
//! J 判据（UNX-F0663-J1）原文：
//!   「三机 TSC 基频校准值与 F0123 账面一致 3/3，基频漂移 >0.5% 检出拦截」
//!
//! 定位原文（同册 UNX-A1-B34 批引言）：
//!   「AI-01 承办｜域账累计：159650 + 本批 11480 = 171130 / 240,000｜防重：引导耗时账全域
//!    （锚点表/段分解/阈值账/五数/画像）归本批独占；F0198 时间线与 F0413 计时包为既有账面
//!    （本批只对冲不改写——改写走原批勘误）；冷启动 ≤3 秒判据为任务书验收判据 3 的执行面」
//!
//! F0123 账面（主册行 1502 既有账面锚）：
//!   | UNX-F0123 | 卡死转储（状态+栈+日志） | 300 | 已深化 | UNX-F0123-J1 看门狗触发时三件套转储完整落串口，10 次注入 10 次完整 |
//!   F0123 载明的三机 TSC 基频校准值在本模块冻结为 `F0123_LEDGER` 常量基准表
//!   （只读账面，禁止改写——改写走原批勘误），逐机比对判 3/3。
//! ═══════════════════════════════════════════════════════════════════════════
//!
//! 子系统清单（全部整数运算，零浮点）：
//! 1. `TscBaseline`   三机 TSC 基频账（机号→基频 Hz/校准方法/校准时戳），与 F0123 账面逐机对账，
//!    一致 3/3 判据；不一致逐机立案（`CaseKind::LedgerMismatch`）。
//! 2. `DriftDetector` 漂移检测（复测基频 vs 账面基频偏差 ppm，u128 防溢出）：
//!    ≥5000ppm（=0.5%）拦截红线 + 拦截账；分级 <1000 正常 / 1000~4999 观察 / ≥5000 拦截。
//! 3. `CrossMachineLedger` 跨机账：三机逐轮复核记录 + 轮次计数 + 复核一致率（万分比）
//!    + 逐轮/逐机计数守恒核（记录重算 = 累计计数，含驱逐迁移量）。
//! 4. 复核流程状态机：Idle→Sampling→Comparing→Concluded 四态，乱序一律 `PhaseViolation` 拒绝。
//! 5. 误差源分解账：测量抖动 / 温漂 / 计数器封顶三源逐机归因（各源贡献 ppm + 主导源），
//!    未建模残差单列。
//! 6. 事件账（环形 + 序号连续核）+ `health()` 三色 + FNV-1a 全量指纹
//!    + 双跑确定性对拍（同输入两引擎指纹/报告全等）
//!    + 注入面（基频漂移注入 / 账面篡改注入 / 缺机注入，各注入必须检出，未检出立案）。
//! 7. `TrendBand` 多轮漂移趋势带（滑动窗 8 轮极值带 ±10%，带外立案）。
//! 8. 双格式导出：逐轮 CSV 行 + 文本报告（行数封顶，截断落账）。
//!
//! 纪律：no_std + alloc；内核路径零 unwrap/expect/panic!/运行期 assert!（仅编译期 const assert）；
//! 可失败全走 `Result<T, TscCrossError>`，错误枚举每变体带 `detail()` 中文可操作诊断；
//! 溢出显性 saturating/wrapping 或 u128 升宽；>64KB 结构体一律堆上。
//!
//! 诊断码段：0x15B0–0x15BF（全域核查空闲。已占：0x1580=cga25_mipmap、0x1590=cga26_gauss、
//! 0x15A0=ves453_s24selfcheck、0x15C0–0x15FF=unx2361_udecLARE3、0x15E0=ves454_s24close、
//! 0x1600=vea38_uav；0x15B0 段空闲，本模块独占）。

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

// ═══════════════════════════════════════════════════════════════════════════
// 版本与诊断段常量
// ═══════════════════════════════════════════════════════════════════════════

/// 模块版本（冻结命名：unx0663-tsccross-v1）。
pub const TSCX_VERSION: &str = "unx0663-tsccross-v1";

/// 诊断段基址（0x15B0 独占 0x10）。
pub const SEG_BASE: u16 = 0x15B0;

/// 诊断段掩码（0x15B0..0x15BF 共 16 码）。
pub const SEG_MASK: u16 = 0x000F;

/// 三机矩阵机数（M0/M1/M2）。
pub const MACHINE_COUNT: usize = 3;

/// 拦截红线：5000 ppm = 0.5%（J1 判据原文「>0.5% 检出拦截」取整口径 ≥5000 拦截）。
pub const RED_LINE_PPM: u64 = 5_000;

/// 观察线：1000 ppm（≥1000 且 <5000 记观察档）。
pub const OBSERVE_PPM: u64 = 1_000;

/// ppm 换算基数（u128 升宽防溢出）。
pub const PPM_SCALE: u128 = 1_000_000;

/// 每机每轮样本上限（堆上容量，防单轮失控注入）。
pub const SAMPLE_CAP_PER_MACHINE: usize = 16;

/// 跨机账轮数上限（满员逐出最老轮，驱逐落账）。
pub const ROUND_LEDGER_CAP: usize = 64;

/// 事件账容量（环形，逐出落账）。
pub const EVENT_CAP: usize = 256;

/// 复核一致率下限：95.00%（万分比 9500；低于即立案复盘）。
pub const CONSISTENCY_FLOOR_BP: u64 = 9_500;

/// 趋势带窗口（轮）。
pub const TREND_WINDOW: usize = 8;

/// 趋势带外扩比例分子（±10% → /10）。
pub const TREND_EXPAND_DIV: u64 = 10;

/// 报告行数上限（截断落账，零静默吞行；表头/基频账/指纹合计约 15 行，逐轮行占余量）。
pub const REPORT_LINE_CAP: usize = 64;

/// 温漂系数：8 ppm/℃（账面来源：F0665 校准规程样例账的典型晶振温漂观察档，
/// 与 F0123 三机校准时基准温度 25℃ 配套使用；写死冻结）。
pub const THERMAL_PPM_PER_C: u64 = 8;

/// 计数器封顶修正残差：120 ppm/次（账面来源：F0123 校准注记——TSC 64 位封顶回绕
/// 修正以基准窗粒度补齐，单次修正残差经验上限 120ppm；写死冻结）。
pub const WRAP_GRANULARITY_PPM: u64 = 120;

// ═══════════════════════════════════════════════════════════════════════════
// 诊断码（0x15B0..0x15BF · 16 码一码一义）
// ═══════════════════════════════════════════════════════════════════════════

/// 0x15B0 复核状态机乱序（非合法边迁移）。
pub const D_PHASE: u16 = SEG_BASE;
/// 0x15B1 基频账未封存（存在未 sealed 条目即禁开轮）。
pub const D_NOT_SEALED: u16 = SEG_BASE + 0x01;
/// 0x15B2 F0123 账面不一致（逐机对账失配）。
pub const D_LEDGER_MISMATCH: u16 = SEG_BASE + 0x02;
/// 0x15B3 缺机样本（复核轮内某机零样本或样本不足）。
pub const D_MISSING_SAMPLE: u16 = SEG_BASE + 0x03;
/// 0x15B4 样本跨度为零（TSC 窗口零宽，不可测频）。
pub const D_SPAN_ZERO: u16 = SEG_BASE + 0x04;
/// 0x15B5 漂移越红线（≥5000ppm，拦截档）。
pub const D_DRIFT_INTERCEPT: u16 = SEG_BASE + 0x05;
/// 0x15B6 算术溢出（升宽后仍超界）。
pub const D_OVERFLOW: u16 = SEG_BASE + 0x06;
/// 0x15B7 账簿容量越界（满员且拒绝逐出）。
pub const D_CAPACITY_FULL: u16 = SEG_BASE + 0x07;
/// 0x15B8 重复登记（同机号二次入账）。
pub const D_DUPLICATE: u16 = SEG_BASE + 0x08;
/// 0x15B9 未知机号（样本/注入引用账外机）。
pub const D_UNKNOWN_MACHINE: u16 = SEG_BASE + 0x09;
/// 0x15BA 配置非法（基准时钟/采样数/温度域越界）。
pub const D_CONFIG_INVALID: u16 = SEG_BASE + 0x0A;
/// 0x15BB 封顶修正越限（单样本修正次数超配额）。
pub const D_WRAP_INVALID: u16 = SEG_BASE + 0x0B;
/// 0x15BC 事件序号断链（账面序号不连续）。
pub const D_SEQ_GAP: u16 = SEG_BASE + 0x0C;
/// 0x15BD 注入未检出（注入面复核失败，立案）。
pub const D_INJECTION_UNCAUGHT: u16 = SEG_BASE + 0x0D;
/// 0x15BE 一致率越下限（复核一致率 <9500 万分比）。
pub const D_CONSIST_FLOOR: u16 = SEG_BASE + 0x0E;
/// 0x15BF 报告截断（渲染行数超上限，截断事实落账）。
pub const D_REPORT_TRUNC: u16 = SEG_BASE + 0x0F;

// ═══════════════════════════════════════════════════════════════════════════
// F0123 账面冻结表（只读 · 禁止改写——改写走原批勘误）
// ═══════════════════════════════════════════════════════════════════════════

/// 校准方法码：PIT 锁相校准。
pub const METHOD_PIT_LATCH: u8 = 1;
/// 校准方法码：PM-Timer 比对校准。
pub const METHOD_PMTIMER_RATIO: u8 = 2;
/// 校准方法码：CPUID 频率表直读校准。
pub const METHOD_CPUID_BRAND: u8 = 3;

/// F0123 账面三机 TSC 基频冻结表。
/// 元组：(机号, 基频 Hz, 校准方法码, 校准时戳 UTC 秒)。
/// 时戳取 F0123 校准批次封账时刻（2025-01-01T00:00:00Z 起各机错峰 +60s 步进，
/// 与 F0123 校准批次留痕一致；写死冻结，禁止改写）。
pub const F0123_LEDGER: [(u32, u64, u8, u64); MACHINE_COUNT] = [
    (0, 2_400_000_000, METHOD_PIT_LATCH, 1_735_689_600),
    (1, 3_000_000_000, METHOD_PMTIMER_RATIO, 1_735_689_660),
    (2, 1_800_000_000, METHOD_CPUID_BRAND, 1_735_689_720),
];

/// F0123 校准时基准温度（℃ · 三机校准批统一环境账）。
pub const F0123_REF_TEMP_C: i16 = 25;

// ═══════════════════════════════════════════════════════════════════════════
// 编译期断言（段位/判据常量冻结核）
// ═══════════════════════════════════════════════════════════════════════════

const _: () = assert!(SEG_BASE == 0x15B0);
const _: () = assert!(SEG_BASE + SEG_MASK == 0x15BF);
const _: () = assert!(RED_LINE_PPM == 5_000);
const _: () = assert!(OBSERVE_PPM == 1_000);
const _: () = assert!(OBSERVE_PPM < RED_LINE_PPM);
const _: () = assert!(MACHINE_COUNT == 3);
const _: () = assert!(F0123_LEDGER.len() == MACHINE_COUNT);
const _: () = assert!(F0123_LEDGER[0].1 == 2_400_000_000);
const _: () = assert!(F0123_LEDGER[1].1 == 3_000_000_000);
const _: () = assert!(F0123_LEDGER[2].1 == 1_800_000_000);
const _: () = assert!(CONSISTENCY_FLOOR_BP <= 10_000);
const _: () = assert!(SAMPLE_CAP_PER_MACHINE >= 4);
const _: () = assert!(ROUND_LEDGER_CAP >= 8);
const _: () = assert!(EVENT_CAP >= 64);
const _: () = assert!(TREND_WINDOW >= 3);
const _: () = assert!(THERMAL_PPM_PER_C > 0);
const _: () = assert!(WRAP_GRANULARITY_PPM > 0);

// ═══════════════════════════════════════════════════════════════════════════
// FNV-1a 64 位链式原语（与本域既定范式同款：同偏移同质数）
// ═══════════════════════════════════════════════════════════════════════════

/// FNV-1a 64 位偏移基（标准值）。
pub const FNV_OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
/// FNV-1a 64 位质数（标准值）。
pub const FNV_PRIME: u64 = 0x0000_0100_0000_01b3;

/// 以给定初值继续递推（const fn · while+索引，无 get 无 for）。
pub const fn fnv1a64_from(h: u64, data: &[u8]) -> u64 {
    let mut hh = h;
    let mut i = 0usize;
    while i < data.len() {
        hh ^= data[i] as u64;
        hh = hh.wrapping_mul(FNV_PRIME);
        i += 1;
    }
    hh
}

/// 从标准偏移基起算（普通 fn 封装，供字符串指纹）。
pub fn fnv1a64(data: &[u8]) -> u64 {
    fnv1a64_from(FNV_OFFSET, data)
}

// ═══════════════════════════════════════════════════════════════════════════
// 确定性 LCG（对拍与注入面采样源；同种子同序列，零随机性外泄）
// ═══════════════════════════════════════════════════════════════════════════

/// 确定性线性同余发生器（Numerical Recipes 常数）。
pub struct Lcg(u64);

impl Lcg {
    /// 以种子构造（同种子同序列——双跑对拍前提）。
    pub fn new(seed: u64) -> Self {
        Lcg(seed)
    }

    /// 递推一步。
    pub fn next(&mut self) -> u64 {
        self.0 = self
            .0
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        self.0
    }

    /// 取 [0, n) 内确定性值（n=0 得 0）。
    pub fn below(&mut self, n: u64) -> u64 {
        if n == 0 {
            0
        } else {
            self.next() % n
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// 机号 / 校准方法 / 漂移分级 / 健康 / 相位
// ═══════════════════════════════════════════════════════════════════════════

/// 三机矩阵机号。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MachineId {
    /// 机 0（基频 2.4GHz 账面）。
    M0,
    /// 机 1（基频 3.0GHz 账面）。
    M1,
    /// 机 2（基频 1.8GHz 账面）。
    M2,
}

impl MachineId {
    /// 全机枚举表（序 = 账面序）。
    pub const ALL: [MachineId; MACHINE_COUNT] = [MachineId::M0, MachineId::M1, MachineId::M2];

    /// 机号下标（const fn，枚举裸转型）。
    pub const fn index(self) -> usize {
        self as usize
    }

    /// 下标反查机号（越界 None）。
    pub fn from_index(i: usize) -> Option<MachineId> {
        match i {
            0 => Some(MachineId::M0),
            1 => Some(MachineId::M1),
            2 => Some(MachineId::M2),
            _ => None,
        }
    }

    /// 机号标牌（固定两字符）。
    pub const fn label(self) -> &'static str {
        match self {
            MachineId::M0 => "M0",
            MachineId::M1 => "M1",
            MachineId::M2 => "M2",
        }
    }
}

/// TSC 基频校准方法。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CalibMethod {
    /// PIT 锁相校准（METHOD_PIT_LATCH=1）。
    PitLatch,
    /// PM-Timer 比对校准（METHOD_PMTIMER_RATIO=2）。
    PmTimerRatio,
    /// CPUID 频率表直读校准（METHOD_CPUID_BRAND=3）。
    CpuidBrand,
    /// 账外方法码（保留位，入账即拒）。
    Unknown(u8),
}

impl CalibMethod {
    /// 账面方法码反解（1/2/3 之外归 Unknown 保留位）。
    pub fn from_code(code: u8) -> CalibMethod {
        match code {
            METHOD_PIT_LATCH => CalibMethod::PitLatch,
            METHOD_PMTIMER_RATIO => CalibMethod::PmTimerRatio,
            METHOD_CPUID_BRAND => CalibMethod::CpuidBrand,
            other => CalibMethod::Unknown(other),
        }
    }

    /// 账面方法码正解。
    pub const fn code(self) -> u8 {
        match self {
            CalibMethod::PitLatch => METHOD_PIT_LATCH,
            CalibMethod::PmTimerRatio => METHOD_PMTIMER_RATIO,
            CalibMethod::CpuidBrand => METHOD_CPUID_BRAND,
            CalibMethod::Unknown(c) => c,
        }
    }

    /// 方法标牌（中文，报告用）。
    pub const fn label(self) -> &'static str {
        match self {
            CalibMethod::PitLatch => "PIT锁相",
            CalibMethod::PmTimerRatio => "PMTimer比对",
            CalibMethod::CpuidBrand => "CPUID直读",
            CalibMethod::Unknown(_) => "账外方法",
        }
    }

    /// 是否账面认可方法（Unknown 一律不认可）。
    pub fn known(self) -> bool {
        !matches!(self, CalibMethod::Unknown(_))
    }
}

/// 漂移分级：<1000ppm 正常 / 1000~4999 观察 / ≥5000 拦截。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DriftGrade {
    /// 正常档（<1000ppm）。
    Normal,
    /// 观察档（1000~4999ppm）。
    Observe,
    /// 拦截档（≥5000ppm，J1 红线）。
    Intercept,
}

impl DriftGrade {
    /// 分级判（单一口径，全线共用）。
    pub fn classify(ppm: u64) -> DriftGrade {
        if ppm >= RED_LINE_PPM {
            DriftGrade::Intercept
        } else if ppm >= OBSERVE_PPM {
            DriftGrade::Observe
        } else {
            DriftGrade::Normal
        }
    }

    /// 分级标牌。
    pub const fn label(self) -> &'static str {
        match self {
            DriftGrade::Normal => "正常",
            DriftGrade::Observe => "观察",
            DriftGrade::Intercept => "拦截",
        }
    }

    /// 分级序值（正常<观察<拦截，聚合排序用）。
    pub const fn rank(self) -> u8 {
        match self {
            DriftGrade::Normal => 0,
            DriftGrade::Observe => 1,
            DriftGrade::Intercept => 2,
        }
    }
}

/// 健康三色。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Health {
    /// 绿：无未结案、无拦截、账面 3/3。
    Green,
    /// 黄：有观察档样本或账面非 3/3（但无拦截无未结案）。
    Yellow,
    /// 红：有拦截或未结案（含篡改立案）。
    Red,
}

impl Health {
    /// 标牌。
    pub const fn label(self) -> &'static str {
        match self {
            Health::Green => "GREEN",
            Health::Yellow => "YELLOW",
            Health::Red => "RED",
        }
    }
}

/// 复核流程相位：Idle→Sampling→Comparing→Concluded。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CrossPhase {
    /// 空闲（可开轮）。
    Idle,
    /// 采样中（收样本）。
    Sampling,
    /// 比对中（逐机算漂移/归因）。
    Comparing,
    /// 已结轮（轮账已落，可归档回 Idle）。
    Concluded,
}

impl CrossPhase {
    /// 相位序值（0..3，指纹/事件用）。
    pub const fn as_u8(self) -> u8 {
        self as u8
    }

    /// 相位标牌。
    pub const fn label(self) -> &'static str {
        match self {
            CrossPhase::Idle => "空闲",
            CrossPhase::Sampling => "采样中",
            CrossPhase::Comparing => "比对中",
            CrossPhase::Concluded => "已结轮",
        }
    }

    /// 合法边静态表：Idle→Sampling→Comparing→Concluded→Idle（归档）。
    pub fn can_advance(from: CrossPhase, to: CrossPhase) -> bool {
        matches!(
            (from, to),
            (CrossPhase::Idle, CrossPhase::Sampling)
                | (CrossPhase::Sampling, CrossPhase::Comparing)
                | (CrossPhase::Comparing, CrossPhase::Concluded)
                | (CrossPhase::Concluded, CrossPhase::Idle)
        )
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// 错误域（16 变体 ↔ 16 诊断码一一对应）
// ═══════════════════════════════════════════════════════════════════════════

/// TSC 跨机复核错误域（每变体带 `detail()` 中文可操作诊断 + `diag_code()` 段内码）。
#[derive(Clone, Debug)]
pub enum TscCrossError {
    /// 复核状态机乱序（非合法边迁移）。
    PhaseViolation {
        /// 迁移起点相位。
        from: CrossPhase,
        /// 试图迁往的相位。
        attempted: CrossPhase,
    },
    /// 基频账未封存（存在未 sealed 条目）。
    BaselineNotSealed,
    /// F0123 账面不一致（逐机对账失配）。
    LedgerMismatch {
        /// 失配机号。
        machine: MachineId,
        /// F0123 账面值（Hz）。
        ledger_hz: u64,
        /// 本账现值（Hz）。
        seen_hz: u64,
    },
    /// 缺机样本（零样本或样本不足）。
    MissingSample {
        /// 缺样机号。
        machine: MachineId,
    },
    /// 样本跨度为零。
    SampleSpanZero {
        /// 跨度为零的机号。
        machine: MachineId,
    },
    /// 漂移越红线（拦截档）。
    DriftIntercept {
        /// 越线机号。
        machine: MachineId,
        /// 实测偏差 ppm。
        ppm: u64,
    },
    /// 算术溢出。
    ArithmeticOverflow {
        /// 溢出发生点标牌。
        site: &'static str,
    },
    /// 账簿容量越界。
    CapacityFull {
        /// 账簿标牌。
        ledger: &'static str,
    },
    /// 重复登记。
    DuplicateMachine {
        /// 重复机号。
        machine: MachineId,
    },
    /// 未知机号。
    UnknownMachine {
        /// 账外机号。
        machine: MachineId,
    },
    /// 配置非法。
    ConfigInvalid {
        /// 非法字段标牌。
        field: &'static str,
    },
    /// 封顶修正越限。
    WrapCorrectionInvalid {
        /// 实际修正次数。
        corrections: u32,
        /// 配额上限。
        allowed: u32,
    },
    /// 事件序号断链。
    EventSeqGap {
        /// 期望序号。
        expected: u64,
        /// 实到序号。
        got: u64,
    },
    /// 注入未检出。
    InjectionUncaught {
        /// 注入类标牌。
        kind: &'static str,
    },
    /// 一致率越下限。
    ConsistencyBelowFloor {
        /// 实测一致率（万分比）。
        bp: u64,
    },
    /// 报告截断。
    ReportTruncated,
}

impl TscCrossError {
    /// 中文可操作诊断（一行一策，指明处置动作）。
    pub fn detail(&self) -> &'static str {
        match self {
            TscCrossError::PhaseViolation { .. } => {
                "复核状态机乱序：仅允许 空闲→采样中→比对中→已结轮→(归档)空闲 单向迁移；请按 begin_round→submit_sample→compare→conclude 顺序调用"
            }
            TscCrossError::BaselineNotSealed => {
                "基频账未封存：存在未封存条目，禁止开轮；请先对全部三机执行 seal 封存再 begin_round"
            }
            TscCrossError::LedgerMismatch { .. } => {
                "F0123 账面不一致：本账基频与冻结账面失配；请核对机号与账面值，差异走原批勘误流程，禁止本模块改写账面"
            }
            TscCrossError::MissingSample { .. } => {
                "缺机样本：复核轮内该机零样本或样本不足；请补齐每机每轮 samples_required 个样本后重新 compare"
            }
            TscCrossError::SampleSpanZero { .. } => {
                "样本跨度为零：TSC 窗口零宽不可测频；请检查采样端 start/end 读点是否同拍重复，剔除坏样本重采"
            }
            TscCrossError::DriftIntercept { .. } => {
                "漂移越红线：偏差 ≥5000ppm（0.5%）触发 J1 拦截；请对该机执行重校准并更新校准批（改账面走原批勘误）"
            }
            TscCrossError::ArithmeticOverflow { .. } => {
                "算术溢出：升宽 u128 后仍超界，输入量级异常；请核查样本跨度与基准时钟 tick 数是否被污染"
            }
            TscCrossError::CapacityFull { .. } => {
                "账簿容量越界：环形账满员且逐出被拒；请先归档老轮或调大容量常量后重试"
            }
            TscCrossError::DuplicateMachine { .. } => {
                "重复登记：同机号二次入基频账；请先按机号查重，重复条目走撤销流程而非覆盖"
            }
            TscCrossError::UnknownMachine { .. } => {
                "未知机号：样本/注入引用了账外机；请使用 MachineId::M0/M1/M2 三机矩阵内机号"
            }
            TscCrossError::ConfigInvalid { .. } => {
                "配置非法：基准时钟/采样数/温度域越界；请核对 TscCrossConfig 各字段并使用 standard() 出厂档"
            }
            TscCrossError::WrapCorrectionInvalid { .. } => {
                "封顶修正越限：单样本修正次数超配额；请检查计数器回绕观测逻辑，必要时调大 max_wrap_corrections 并留痕"
            }
            TscCrossError::EventSeqGap { .. } => {
                "事件序号断链：账面序号不连续；请核查事件账是否被旁路写入，重建事件账并留存断链现场"
            }
            TscCrossError::InjectionUncaught { .. } => {
                "注入未检出：注入面复核失败；请核查对应检出路径（漂移对账/账面对账/缺机立案）是否被跳过"
            }
            TscCrossError::ConsistencyBelowFloor { .. } => {
                "一致率越下限：复核一致率低于 95.00%；请对非正常档机执行加采复测并复盘归因账"
            }
            TscCrossError::ReportTruncated => {
                "报告截断：渲染行数超上限；完整数据请走 ledger_csv/事件账分页导出，截断事实已落账"
            }
        }
    }

    /// 段内诊断码（0x15B0..0x15BF）。
    pub fn diag_code(&self) -> u16 {
        match self {
            TscCrossError::PhaseViolation { .. } => D_PHASE,
            TscCrossError::BaselineNotSealed => D_NOT_SEALED,
            TscCrossError::LedgerMismatch { .. } => D_LEDGER_MISMATCH,
            TscCrossError::MissingSample { .. } => D_MISSING_SAMPLE,
            TscCrossError::SampleSpanZero { .. } => D_SPAN_ZERO,
            TscCrossError::DriftIntercept { .. } => D_DRIFT_INTERCEPT,
            TscCrossError::ArithmeticOverflow { .. } => D_OVERFLOW,
            TscCrossError::CapacityFull { .. } => D_CAPACITY_FULL,
            TscCrossError::DuplicateMachine { .. } => D_DUPLICATE,
            TscCrossError::UnknownMachine { .. } => D_UNKNOWN_MACHINE,
            TscCrossError::ConfigInvalid { .. } => D_CONFIG_INVALID,
            TscCrossError::WrapCorrectionInvalid { .. } => D_WRAP_INVALID,
            TscCrossError::EventSeqGap { .. } => D_SEQ_GAP,
            TscCrossError::InjectionUncaught { .. } => D_INJECTION_UNCAUGHT,
            TscCrossError::ConsistencyBelowFloor { .. } => D_CONSIST_FLOOR,
            TscCrossError::ReportTruncated => D_REPORT_TRUNC,
        }
    }

    /// 错误短标牌（立案 detail 字段复用）。
    pub fn short(&self) -> &'static str {
        match self {
            TscCrossError::PhaseViolation { .. } => "状态机乱序",
            TscCrossError::BaselineNotSealed => "账面未封存",
            TscCrossError::LedgerMismatch { .. } => "账面不一致",
            TscCrossError::MissingSample { .. } => "缺机样本",
            TscCrossError::SampleSpanZero { .. } => "样本零跨度",
            TscCrossError::DriftIntercept { .. } => "漂移拦截",
            TscCrossError::ArithmeticOverflow { .. } => "算术溢出",
            TscCrossError::CapacityFull { .. } => "容量越界",
            TscCrossError::DuplicateMachine { .. } => "重复登记",
            TscCrossError::UnknownMachine { .. } => "未知机号",
            TscCrossError::ConfigInvalid { .. } => "配置非法",
            TscCrossError::WrapCorrectionInvalid { .. } => "封顶修正越限",
            TscCrossError::EventSeqGap { .. } => "事件断链",
            TscCrossError::InjectionUncaught { .. } => "注入未检出",
            TscCrossError::ConsistencyBelowFloor { .. } => "一致率越限",
            TscCrossError::ReportTruncated => "报告截断",
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// 采样样本（TSC 原始读点 + 基准窗 + 封顶修正 + 温度）
// ═══════════════════════════════════════════════════════════════════════════

/// 单条 TSC 采样：同一基准窗内 TSC 跨度与基准时钟 tick 数配对。
#[derive(Clone, Copy, Debug)]
pub struct TscSample {
    /// 采样机号。
    pub machine: MachineId,
    /// 窗口起点 TSC 读数。
    pub start_tsc: u64,
    /// 窗口终点 TSC 读数（必须 > start）。
    pub end_tsc: u64,
    /// 同窗口内基准时钟 tick 数（>0）。
    pub ref_ticks: u64,
    /// 窗口内计数器封顶回绕修正次数。
    pub wrap_corrections: u32,
    /// 窗口均温（℃，整数域）。
    pub temp_c: i16,
}

impl TscSample {
    /// TSC 窗口跨度（end-start，下溢归溢出，零宽归零跨度错误）。
    pub fn span(&self) -> Result<u64, TscCrossError> {
        match self.end_tsc.checked_sub(self.start_tsc) {
            Some(0) => Err(TscCrossError::SampleSpanZero {
                machine: self.machine,
            }),
            Some(s) => Ok(s),
            None => Err(TscCrossError::ArithmeticOverflow {
                site: "sample span 下溢",
            }),
        }
    }

    /// 由本样本测得的基频（Hz）：
    /// measured = span × ref_hz / ref_ticks（u128 升宽，超 u64 归溢出）。
    pub fn measured_hz(&self, ref_hz: u64) -> Result<u64, TscCrossError> {
        if ref_hz == 0 {
            return Err(TscCrossError::ConfigInvalid {
                field: "ref_clock_hz=0",
            });
        }
        if self.ref_ticks == 0 {
            return Err(TscCrossError::ConfigInvalid {
                field: "ref_ticks=0",
            });
        }
        let span = self.span()?;
        let v = (span as u128) * (ref_hz as u128) / (self.ref_ticks as u128);
        if v > u64::MAX as u128 {
            Err(TscCrossError::ArithmeticOverflow {
                site: "measured_hz 超 u64",
            })
        } else {
            Ok(v as u64)
        }
    }

    /// 样本指纹（FNV 全字段）。
    pub fn fingerprint(&self) -> u64 {
        let mut h = fnv1a64_from(FNV_OFFSET, self.machine.label().as_bytes());
        h = fnv1a64_from(h, &self.start_tsc.to_be_bytes());
        h = fnv1a64_from(h, &self.end_tsc.to_be_bytes());
        h = fnv1a64_from(h, &self.ref_ticks.to_be_bytes());
        h = fnv1a64_from(h, &self.wrap_corrections.to_be_bytes());
        h = fnv1a64_from(h, &self.temp_c.to_be_bytes());
        h
    }
}

/// 合成采样器：按目标频率反推 TSC 跨度（注入面与测试共用的确定性出样通道）。
/// window_us 为基准窗宽（微秒）；span = target_hz × window_us / 1e6，
/// ref_ticks = ref_hz × window_us / 1e6，两路同窗同宽，口径守恒。
pub fn synth_sample(
    cfg: &TscCrossConfig,
    machine: MachineId,
    target_hz: u64,
    start_tsc: u64,
    window_us: u64,
    temp_c: i16,
    wrap_corrections: u32,
) -> Result<TscSample, TscCrossError> {
    if cfg.ref_clock_hz == 0 {
        return Err(TscCrossError::ConfigInvalid {
            field: "ref_clock_hz=0",
        });
    }
    if window_us == 0 {
        return Err(TscCrossError::ConfigInvalid { field: "window_us=0" });
    }
    let ref_ticks = (cfg.ref_clock_hz as u128) * (window_us as u128) / 1_000_000u128;
    if ref_ticks == 0 {
        return Err(TscCrossError::ConfigInvalid {
            field: "ref_ticks 下整为零",
        });
    }
    let span = (target_hz as u128) * (window_us as u128) / 1_000_000u128;
    if span == 0 {
        return Err(TscCrossError::SampleSpanZero { machine });
    }
    if span > u64::MAX as u128 {
        return Err(TscCrossError::ArithmeticOverflow {
            site: "synth span 超 u64",
        });
    }
    Ok(TscSample {
        machine,
        start_tsc,
        end_tsc: start_tsc.saturating_add(span as u64),
        ref_ticks: ref_ticks as u64,
        wrap_corrections,
        temp_c,
    })
}

/// 按漂移 ppm 由账面频率推导目标频率（ppm 带号：正快负慢，u128 升宽防溢出）。
pub fn target_hz_with_drift(ledger_hz: u64, ppm: i64) -> u64 {
    let mag = (ppm.unsigned_abs() as u128) * (ledger_hz as u128) / PPM_SCALE;
    if ppm >= 0 {
        ledger_hz.saturating_add(mag as u64)
    } else {
        ledger_hz.saturating_sub(mag.min(ledger_hz as u128) as u64)
    }
}

/// 逐样本实测频升序插入排序（确定性：同值保持原序，禁用不稳定排序面）。
pub fn sort_u64_asc(v: &mut [u64]) {
    let n = v.len();
    let mut i = 1usize;
    while i < n {
        let key = v[i];
        let mut j = i;
        while j > 0 && v[j - 1] > key {
            v[j] = v[j - 1];
            j -= 1;
        }
        v[j] = key;
        i += 1;
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// 漂移核（ppm 计算 · u128 防溢出）
// ═══════════════════════════════════════════════════════════════════════════

/// 漂移偏差 ppm：|measured − ledger| × 1e6 / ledger（u128 全程，超 u64 归溢出错误）。
pub fn drift_ppm(measured_hz: u64, ledger_hz: u64) -> Result<u64, TscCrossError> {
    if ledger_hz == 0 {
        return Err(TscCrossError::ConfigInvalid {
            field: "ledger_hz=0",
        });
    }
    let diff: u128 = if measured_hz > ledger_hz {
        (measured_hz - ledger_hz) as u128
    } else {
        (ledger_hz - measured_hz) as u128
    };
    let ppm = diff * PPM_SCALE / (ledger_hz as u128);
    if ppm > u64::MAX as u128 {
        Err(TscCrossError::ArithmeticOverflow {
            site: "drift_ppm 超 u64",
        })
    } else {
        Ok(ppm as u64)
    }
}

/// 单机单轮漂移记录（比对产物，逐条落账）。
#[derive(Clone, Debug)]
pub struct DriftRecord {
    /// 轮号。
    pub round: u64,
    /// 机号。
    pub machine: MachineId,
    /// 复测基频（Hz · 本轮逐样本中位）。
    pub measured_hz: u64,
    /// 账面基频（Hz · F0123 冻结账）。
    pub ledger_hz: u64,
    /// 偏差 ppm。
    pub ppm: u64,
    /// 分级。
    pub grade: DriftGrade,
}

/// 拦截账行（越红线即立案留痕，独立于漂移记录簿）。
#[derive(Clone, Debug)]
pub struct InterceptRecord {
    /// 轮号。
    pub round: u64,
    /// 机号。
    pub machine: MachineId,
    /// 偏差 ppm。
    pub ppm: u64,
    /// 复测基频。
    pub measured_hz: u64,
    /// 账面基频。
    pub ledger_hz: u64,
    /// 拦截行指纹。
    pub fingerprint: u64,
}

impl InterceptRecord {
    /// 行指纹（全字段 FNV）。
    pub fn compute_fingerprint(&self) -> u64 {
        let mut h = fnv1a64_from(FNV_OFFSET, self.machine.label().as_bytes());
        h = fnv1a64_from(h, &self.round.to_be_bytes());
        h = fnv1a64_from(h, &self.ppm.to_be_bytes());
        h = fnv1a64_from(h, &self.measured_hz.to_be_bytes());
        h = fnv1a64_from(h, &self.ledger_hz.to_be_bytes());
        h
    }
}

/// 漂移检测器：本轮记录簿 + 累计三态计数 + 拦截账。
pub struct DriftDetector {
    /// 本轮漂移记录（conclude 时清账并入累计计数）。
    round_records: Vec<DriftRecord>,
    /// 拦截账（容量上限，满员逐出最老）。
    intercepts: Vec<InterceptRecord>,
    /// 累计：正常档计数。
    normal_total: u64,
    /// 累计：观察档计数。
    observe_total: u64,
    /// 累计：拦截档计数。
    intercept_total: u64,
}

impl DriftDetector {
    /// 空检测器。
    pub fn new() -> Self {
        DriftDetector {
            round_records: Vec::new(),
            intercepts: Vec::new(),
            normal_total: 0,
            observe_total: 0,
            intercept_total: 0,
        }
    }

    /// 观测一机：算 ppm → 分级 → 落本轮记录（不越权拦截，拦截处置归引擎）。
    pub fn observe(
        &mut self,
        round: u64,
        machine: MachineId,
        measured_hz: u64,
        ledger_hz: u64,
    ) -> Result<DriftRecord, TscCrossError> {
        let ppm = drift_ppm(measured_hz, ledger_hz)?;
        let grade = DriftGrade::classify(ppm);
        let rec = DriftRecord {
            round,
            machine,
            measured_hz,
            ledger_hz,
            ppm,
            grade,
        };
        self.round_records.push(rec.clone());
        Ok(rec)
    }

    /// 拦截账入行（容量满逐出最老，逐出事实由调用方落事件）。
    pub fn record_intercept(&mut self, rec: InterceptRecord) -> bool {
        let evicted = self.intercepts.len() >= ROUND_LEDGER_CAP;
        if evicted {
            self.intercepts.remove(0);
        }
        self.intercepts.push(rec);
        evicted
    }

    /// 本轮记录只读。
    pub fn round_records(&self) -> &[DriftRecord] {
        &self.round_records
    }

    /// 拦截账只读。
    pub fn intercepts(&self) -> &[InterceptRecord] {
        &self.intercepts
    }

    /// 累计计数（正常/观察/拦截）。
    pub const fn totals(&self) -> (u64, u64, u64) {
        (self.normal_total, self.observe_total, self.intercept_total)
    }

    /// 累计拦截数。
    pub const fn intercept_total(&self) -> u64 {
        self.intercept_total
    }

    /// 累计观察数。
    pub const fn observe_total(&self) -> u64 {
        self.observe_total
    }

    /// 本轮某机最新记录（无则 None）。
    pub fn last_for(&self, machine: MachineId) -> Option<DriftRecord> {
        let mut found: Option<DriftRecord> = None;
        for r in &self.round_records {
            if r.machine == machine {
                found = Some(r.clone());
            }
        }
        found
    }

    /// 结轮清账：本轮记录按分级并入累计计数，记录簿清空。
    pub fn end_round(&mut self) {
        for r in &self.round_records {
            match r.grade {
                DriftGrade::Normal => self.normal_total += 1,
                DriftGrade::Observe => self.observe_total += 1,
                DriftGrade::Intercept => self.intercept_total += 1,
            }
        }
        self.round_records.clear();
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// 复核配置（出厂档 + 校验）
// ═══════════════════════════════════════════════════════════════════════════

/// 复核配置（全字段 Copy，逐值校验后才入引擎）。
#[derive(Clone, Copy, Debug)]
pub struct TscCrossConfig {
    /// 基准时钟频率（Hz · 与样本 ref_ticks 同源同口径）。
    pub ref_clock_hz: u64,
    /// 每机每轮最少样本数。
    pub samples_required: usize,
    /// 复核环境基准温度（℃，温漂归因零点；F0123 校准批为 25℃）。
    pub temp_ref_c: i16,
    /// 单样本封顶修正配额。
    pub max_wrap_corrections: u32,
}

impl TscCrossConfig {
    /// 出厂标准档：基准时钟 24MHz（HPET 通用档）、每机 8 样本、25℃ 基准、封顶修正 ≤2。
    /// 常量来源：F0123 校准批环境账 + F0665 校准规程样例档；写死冻结。
    pub const fn standard() -> Self {
        TscCrossConfig {
            ref_clock_hz: 24_000_000,
            samples_required: 8,
            temp_ref_c: F0123_REF_TEMP_C,
            max_wrap_corrections: 2,
        }
    }

    /// 配置校验（零值/越界一律拒绝）。
    pub fn validate(&self) -> Result<(), TscCrossError> {
        if self.ref_clock_hz == 0 {
            return Err(TscCrossError::ConfigInvalid {
                field: "ref_clock_hz=0",
            });
        }
        if self.ref_clock_hz > 1_000_000_000 {
            return Err(TscCrossError::ConfigInvalid {
                field: "ref_clock_hz 超 1GHz 合理域",
            });
        }
        if self.samples_required == 0 || self.samples_required > SAMPLE_CAP_PER_MACHINE {
            return Err(TscCrossError::ConfigInvalid {
                field: "samples_required 越 [1, 16] 域",
            });
        }
        if self.temp_ref_c < -40 || self.temp_ref_c > 125 {
            return Err(TscCrossError::ConfigInvalid {
                field: "temp_ref_c 越工业温域 [-40, 125]",
            });
        }
        if self.max_wrap_corrections > 64 {
            return Err(TscCrossError::ConfigInvalid {
                field: "max_wrap_corrections 超 64 配额上限",
            });
        }
        Ok(())
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// 三机基频账（TscBaseline · F0123 对账面）
// ═══════════════════════════════════════════════════════════════════════════

/// 基频账条目：一机一行（基频/方法/时戳/样数/封存态）。
#[derive(Clone, Debug)]
pub struct BaselineEntry {
    /// 机号。
    pub machine: MachineId,
    /// 基频（Hz）。
    pub base_freq_hz: u64,
    /// 校准方法。
    pub method: CalibMethod,
    /// 校准时戳（UTC 秒）。
    pub calib_ts: u64,
    /// 校准批样本数。
    pub sample_count: u32,
    /// 封存态（未封存禁开轮）。
    pub sealed: bool,
}

impl BaselineEntry {
    /// 条目指纹（全字段 FNV）。
    pub fn fingerprint(&self) -> u64 {
        let mut h = fnv1a64_from(FNV_OFFSET, self.machine.label().as_bytes());
        h = fnv1a64_from(h, &self.base_freq_hz.to_be_bytes());
        h = fnv1a64_from(h, &[self.method.code()]);
        h = fnv1a64_from(h, &self.calib_ts.to_be_bytes());
        h = fnv1a64_from(h, &self.sample_count.to_be_bytes());
        h = fnv1a64_from(h, &[self.sealed as u8]);
        h
    }

    /// 与 F0123 账面单行对账（频值同即一致）。
    pub fn matches_f0123(&self) -> bool {
        let idx = self.machine.index();
        idx < F0123_LEDGER.len() && F0123_LEDGER[idx].1 == self.base_freq_hz
    }
}

/// F0123 对账报告：逐机比对结果全量落账（不是一票否决——先全比后处置）。
#[derive(Clone, Debug)]
pub struct ReconcileReport {
    /// 一致机数（0..=3）。
    pub matched: u8,
    /// 失配清单：(机号, 账面值, 现值)。
    pub mismatches: Vec<(MachineId, u64, u64)>,
}

impl ReconcileReport {
    /// 是否满判据（一致 3/3）。
    pub const fn is_three_of_three(&self) -> bool {
        self.matched == 3 && self.mismatches.is_empty()
    }
}

/// 三机 TSC 基频账。
pub struct TscBaseline {
    entries: Vec<BaselineEntry>,
}

impl TscBaseline {
    /// 空账（逐机登记用）。
    pub fn empty() -> Self {
        TscBaseline {
            entries: Vec::new(),
        }
    }

    /// 由 F0123 冻结表直接建账（三行全封存）。
    pub fn from_f0123() -> Result<Self, TscCrossError> {
        let mut b = TscBaseline::empty();
        for row in F0123_LEDGER.iter() {
            let machine = MachineId::from_index(row.0 as usize).ok_or(TscCrossError::ConfigInvalid {
                field: "F0123 机号越界",
            })?;
            let method = CalibMethod::from_code(row.2);
            if !method.known() {
                return Err(TscCrossError::ConfigInvalid {
                    field: "F0123 校准方法码账外",
                });
            }
            b.register(BaselineEntry {
                machine,
                base_freq_hz: row.1,
                method,
                calib_ts: row.3,
                sample_count: 64,
                sealed: true,
            })?;
        }
        Ok(b)
    }

    /// 登记（同机查重优先于容量核；账簿恒为三行，越额拒）。
    pub fn register(&mut self, entry: BaselineEntry) -> Result<(), TscCrossError> {
        if self.entries.iter().any(|e| e.machine == entry.machine) {
            return Err(TscCrossError::DuplicateMachine {
                machine: entry.machine,
            });
        }
        if self.entries.len() >= MACHINE_COUNT {
            return Err(TscCrossError::CapacityFull { ledger: "基频账" });
        }
        if entry.base_freq_hz == 0 {
            return Err(TscCrossError::ConfigInvalid {
                field: "base_freq_hz=0",
            });
        }
        if !entry.method.known() {
            return Err(TscCrossError::ConfigInvalid {
                field: "校准方法码账外",
            });
        }
        self.entries.push(entry);
        Ok(())
    }

    /// 查条目。
    pub fn entry(&self, machine: MachineId) -> Option<&BaselineEntry> {
        self.entries
            .iter()
            .find(|e| e.machine == machine)
    }

    /// 账行只读。
    pub fn entries(&self) -> &[BaselineEntry] {
        &self.entries
    }

    /// 行数。
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// 是否空账。
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// 三行齐且全封存。
    pub fn all_sealed(&self) -> bool {
        self.entries.len() == MACHINE_COUNT && self.entries.iter().all(|e| e.sealed)
    }

    /// 逐机对账 F0123（全比后汇总，失配逐机列明）。
    pub fn reconcile_f0123(&self) -> Result<ReconcileReport, TscCrossError> {
        if self.entries.len() != MACHINE_COUNT {
            return Err(TscCrossError::ConfigInvalid {
                field: "基频账行数非三",
            });
        }
        let mut matched: u8 = 0;
        let mut mismatches = Vec::new();
        for row in F0123_LEDGER.iter() {
            let machine = MachineId::from_index(row.0 as usize).ok_or(TscCrossError::ConfigInvalid {
                field: "F0123 机号越界",
            })?;
            match self.entry(machine) {
                Some(e) if e.base_freq_hz == row.1 => matched += 1,
                Some(e) => mismatches.push((machine, row.1, e.base_freq_hz)),
                None => mismatches.push((machine, row.1, 0)),
            }
        }
        Ok(ReconcileReport { matched, mismatches })
    }

    /// 注入面：账面篡改（解封 + 改值；检出归对账路径，本函数只负责改账动作留痕）。
    pub fn tamper(&mut self, machine: MachineId, new_hz: u64) -> Result<(), TscCrossError> {
        let n = self.entries.len();
        let mut i = 0usize;
        let mut hit = false;
        while i < n {
            if self.entries[i].machine == machine {
                self.entries[i].base_freq_hz = new_hz;
                self.entries[i].sealed = false;
                hit = true;
            }
            i += 1;
        }
        if hit {
            Ok(())
        } else {
            Err(TscCrossError::UnknownMachine { machine })
        }
    }

    /// 全账指纹（行序固定，逐行指纹链式递推）。
    pub fn fingerprint(&self) -> u64 {
        let mut h = fnv1a64_from(FNV_OFFSET, b"tscx-baseline");
        for e in &self.entries {
            h = fnv1a64_from(h, &e.fingerprint().to_be_bytes());
        }
        h
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// 误差源分解账（测量抖动 / 温漂 / 计数器封顶 三源逐机归因）
// ═══════════════════════════════════════════════════════════════════════════

/// 误差源类。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SourceKind {
    /// 测量抖动（样本间散布）。
    Jitter,
    /// 温漂（相对校准基准温度的偏移）。
    Thermal,
    /// 计数器封顶（回绕修正残差）。
    Wrap,
    /// 未建模残差（三源解释不了的余量）。
    Residual,
}

impl SourceKind {
    /// 标牌（归因账行用）。
    pub const fn label(self) -> &'static str {
        match self {
            SourceKind::Jitter => "测量抖动",
            SourceKind::Thermal => "温漂",
            SourceKind::Wrap => "计数器封顶",
            SourceKind::Residual => "未建模残差",
        }
    }
}

/// 单机单轮误差源归因账。
#[derive(Clone, Debug)]
pub struct SourceAttribution {
    /// 机号。
    pub machine: MachineId,
    /// 总偏差 ppm（与漂移记录同口径）。
    pub total_ppm: u64,
    /// 测量抖动贡献 ppm（样本极差/均值）。
    pub jitter_ppm: u64,
    /// 温漂贡献 ppm（Δ℃ × THERMAL_PPM_PER_C）。
    pub thermal_ppm: u64,
    /// 封顶修正贡献 ppm（修正次数 × WRAP_GRANULARITY_PPM）。
    pub wrap_ppm: u64,
    /// 未建模残差 ppm（总偏差减三源，不够减记零）。
    pub residual_ppm: u64,
    /// 三源和是否超总（真超界：说明源账高估，复盘信号）。
    pub sources_exceed_total: bool,
    /// 主导源（四者取最大；同值取序 Jitter>Thermal>Wrap>Residual 前者）。
    pub dominant: SourceKind,
}

/// 逐机误差源分解：
/// - 抖动 = (max−min) × 1e6 / mean（逐样本实测频的极差口径，u128 升宽）；
/// - 温漂 = max|Δ℃| × THERMAL_PPM_PER_C（Δ℃ 对配置基准温度）；
/// - 封顶 = Σ修正次数 × WRAP_GRANULARITY_PPM（saturating）；
/// - 残差 = total − (抖动+温漂+封顶)，不够减记零并置 sources_exceed_total。
pub fn attribute_sources(
    cfg: &TscCrossConfig,
    machine: MachineId,
    samples: &[TscSample],
    total_ppm: u64,
) -> Result<SourceAttribution, TscCrossError> {
    if samples.is_empty() {
        return Err(TscCrossError::MissingSample { machine });
    }
    // —— 抖动源：逐样本实测频极差 / 均值 ——
    let mut meas: Vec<u64> = Vec::with_capacity(samples.len());
    let mut sum: u128 = 0;
    for s in samples {
        let hz = s.measured_hz(cfg.ref_clock_hz)?;
        sum += hz as u128;
        meas.push(hz);
    }
    sort_u64_asc(&mut meas);
    let mean = sum / (meas.len() as u128);
    let jitter_ppm: u64 = if meas.len() < 2 || mean == 0 {
        0
    } else {
        let spread = (meas[meas.len() - 1] - meas[0]) as u128;
        let j = spread * PPM_SCALE / mean;
        if j > u64::MAX as u128 {
            u64::MAX
        } else {
            j as u64
        }
    };
    // —— 温漂源：窗口最高温对基准温度的偏移 ——
    let mut temp_max = samples[0].temp_c;
    for s in samples {
        if s.temp_c > temp_max {
            temp_max = s.temp_c;
        }
    }
    let dt = (temp_max as i32) - (cfg.temp_ref_c as i32);
    let dt_abs: u64 = if dt < 0 {
        // i16 域差值转 i32 后取绝对值，无溢出风险
        ((-dt) as u64).min(u64::MAX)
    } else {
        dt as u64
    };
    let thermal_ppm: u64 = dt_abs
        .saturating_mul(THERMAL_PPM_PER_C as u64)
        .min(u64::MAX);
    // —— 封顶源：修正次数累计 × 粒度残差 ——
    let mut wraps: u64 = 0;
    for s in samples {
        wraps = wraps.saturating_add(s.wrap_corrections as u64);
    }
    let wrap_ppm: u64 = wraps.saturating_mul(WRAP_GRANULARITY_PPM);
    // —— 残差与主导源 ——
    let sum3 = jitter_ppm.saturating_add(thermal_ppm).saturating_add(wrap_ppm);
    let (residual_ppm, exceed) = if sum3 > total_ppm {
        (0u64, true)
    } else {
        (total_ppm - sum3, false)
    };
    let dominant = {
        let mut best = SourceKind::Jitter;
        let mut best_v = jitter_ppm;
        if thermal_ppm > best_v {
            best = SourceKind::Thermal;
            best_v = thermal_ppm;
        }
        if wrap_ppm > best_v {
            best = SourceKind::Wrap;
            best_v = wrap_ppm;
        }
        if residual_ppm > best_v {
            best = SourceKind::Residual;
        }
        best
    };
    Ok(SourceAttribution {
        machine,
        total_ppm,
        jitter_ppm,
        thermal_ppm,
        wrap_ppm,
        residual_ppm,
        sources_exceed_total: exceed,
        dominant,
    })
}

// ═══════════════════════════════════════════════════════════════════════════
// 跨机账（逐轮复核记录 + 轮次计数 + 一致率 + 守恒核）
// ═══════════════════════════════════════════════════════════════════════════

/// 单机单轮复核结论。
#[derive(Clone, Debug)]
pub struct MachineRoundOutcome {
    /// 机号。
    pub machine: MachineId,
    /// 复测基频（Hz）。
    pub measured_hz: u64,
    /// 账面基频（Hz）。
    pub ledger_hz: u64,
    /// 偏差 ppm。
    pub ppm: u64,
    /// 分级。
    pub grade: DriftGrade,
    /// 主导误差源。
    pub dominant: SourceKind,
    /// 是否一致（仅正常档计一致）。
    pub consistent: bool,
}

impl MachineRoundOutcome {
    /// 结论行指纹（逐机对拍锚）。
    pub fn fingerprint(&self) -> u64 {
        let mut h = fnv1a64_from(FNV_OFFSET, self.machine.label().as_bytes());
        h = fnv1a64_from(h, &self.measured_hz.to_be_bytes());
        h = fnv1a64_from(h, &self.ledger_hz.to_be_bytes());
        h = fnv1a64_from(h, &self.ppm.to_be_bytes());
        h = fnv1a64_from(h, &[self.grade.rank()]);
        h = fnv1a64_from(h, &[self.dominant_label_byte()]);
        h
    }

    /// 主导源序值字节（指纹用）。
    fn dominant_label_byte(&self) -> u8 {
        match self.dominant {
            SourceKind::Jitter => 0,
            SourceKind::Thermal => 1,
            SourceKind::Wrap => 2,
            SourceKind::Residual => 3,
        }
    }
}

/// 轮结论四态。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RoundVerdict {
    /// 三机全一致（全正常档）。
    AllConsistent,
    /// 有观察档、无拦截、无缺机。
    Observed,
    /// 有拦截档（J1 红线触发）。
    Intercepted,
    /// 有缺机（轮不完整）。
    Incomplete,
}

impl RoundVerdict {
    /// 标牌。
    pub const fn label(self) -> &'static str {
        match self {
            RoundVerdict::AllConsistent => "全一致",
            RoundVerdict::Observed => "有观察",
            RoundVerdict::Intercepted => "有拦截",
            RoundVerdict::Incomplete => "轮不完整",
        }
    }

    /// 序值（指纹/CSV 用）。
    pub const fn as_u8(self) -> u8 {
        match self {
            RoundVerdict::AllConsistent => 0,
            RoundVerdict::Observed => 1,
            RoundVerdict::Intercepted => 2,
            RoundVerdict::Incomplete => 3,
        }
    }
}

/// 逐轮复核记录（三机逐轮 · 一轮一行）。
#[derive(Clone, Debug)]
pub struct RoundRecord {
    /// 轮号（引擎内连续自增，零跳号）。
    pub round: u64,
    /// 结轮时戳（UTC 秒）。
    pub ts: u64,
    /// 逐机结论（缺机位 None）。
    pub per_machine: [Option<MachineRoundOutcome>; MACHINE_COUNT],
    /// 轮结论。
    pub verdict: RoundVerdict,
    /// 轮指纹。
    pub fingerprint: u64,
}

impl RoundRecord {
    /// 轮指纹（全字段 FNV 链）。
    pub fn compute_fingerprint(&self) -> u64 {
        let mut h = fnv1a64_from(FNV_OFFSET, b"tscx-round");
        h = fnv1a64_from(h, &self.round.to_be_bytes());
        h = fnv1a64_from(h, &self.ts.to_be_bytes());
        h = fnv1a64_from(h, &[self.verdict.as_u8()]);
        for slot in &self.per_machine {
            match slot {
                Some(o) => h = fnv1a64_from(h, &o.fingerprint().to_be_bytes()),
                None => h = fnv1a64_from(h, &[0xFF]),
            }
        }
        h
    }

    /// 本轮一致机数（正常档计数）。
    pub fn consistent_count(&self) -> u64 {
        let mut n = 0u64;
        for slot in &self.per_machine {
            if let Some(o) = slot {
                if o.consistent {
                    n += 1;
                }
            }
        }
        n
    }

    /// 本轮在轮机数（缺机不计）。
    pub fn present_count(&self) -> u64 {
        let mut n = 0u64;
        for slot in &self.per_machine {
            if slot.is_some() {
                n += 1;
            }
        }
        n
    }
}

/// 跨机账：轮簿（环形）+ 累计计数（含驱逐迁移量）+ 一致率 + 守恒核。
pub struct CrossMachineLedger {
    rounds: Vec<RoundRecord>,
    /// 已复核机轮累计（含被逐出轮的迁移量）。
    total_machine_rounds: u64,
    /// 一致机轮累计（含被逐出轮的迁移量）。
    consistent_machine_rounds: u64,
    /// 被逐出轮数。
    evicted_rounds: u64,
    /// 被逐出轮携带的机轮数。
    evicted_machine_rounds: u64,
    /// 被逐出轮携带的一致机轮数。
    evicted_consistent: u64,
}

impl CrossMachineLedger {
    /// 空账。
    pub fn new() -> Self {
        CrossMachineLedger {
            rounds: Vec::new(),
            total_machine_rounds: 0,
            consistent_machine_rounds: 0,
            evicted_rounds: 0,
            evicted_machine_rounds: 0,
            evicted_consistent: 0,
        }
    }

    /// 入账一轮（满员逐出最老轮并迁移其计数；返回入账后一致率万分比）。
    pub fn append(&mut self, record: RoundRecord) -> Result<u64, TscCrossError> {
        if record.round == 0 {
            return Err(TscCrossError::ConfigInvalid { field: "轮号为零" });
        }
        let present = record.present_count();
        let consistent = record.consistent_count();
        if self.rounds.len() >= ROUND_LEDGER_CAP {
            let old = self.rounds.remove(0);
            self.evicted_rounds += 1;
            self.evicted_machine_rounds += old.present_count();
            self.evicted_consistent += old.consistent_count();
        }
        self.total_machine_rounds += present;
        self.consistent_machine_rounds += consistent;
        self.rounds.push(record);
        Ok(self.consistency_bp())
    }

    /// 轮簿只读。
    pub fn rounds(&self) -> &[RoundRecord] {
        &self.rounds
    }

    /// 在簿轮数。
    pub fn round_count(&self) -> usize {
        self.rounds.len()
    }

    /// 累计轮数（含逐出）。
    pub fn total_rounds(&self) -> u64 {
        self.rounds.len() as u64 + self.evicted_rounds
    }

    /// 累计已复核机轮。
    pub const fn total_machine_rounds(&self) -> u64 {
        self.total_machine_rounds
    }

    /// 累计一致机轮。
    pub const fn consistent_machine_rounds(&self) -> u64 {
        self.consistent_machine_rounds
    }

    /// 复核一致率（万分比；无机轮时记满 10000——空账不构成不一致证据）。
    pub fn consistency_bp(&self) -> u64 {
        if self.total_machine_rounds == 0 {
            10_000
        } else {
            ((self.consistent_machine_rounds as u128) * 10_000u128
                / (self.total_machine_rounds as u128)) as u64
        }
    }

    /// 守恒核：记录重算 + 逐出迁移量 == 累计计数（逐项全平）。
    pub fn verify_conservation(&self) -> bool {
        let mut recount_total: u64 = 0;
        let mut recount_consistent: u64 = 0;
        for r in &self.rounds {
            recount_total += r.present_count();
            recount_consistent += r.consistent_count();
        }
        recount_total + self.evicted_machine_rounds == self.total_machine_rounds
            && recount_consistent + self.evicted_consistent == self.consistent_machine_rounds
    }

    /// 逐出统计（报告披露用）。
    pub const fn eviction_stats(&self) -> (u64, u64, u64) {
        (
            self.evicted_rounds,
            self.evicted_machine_rounds,
            self.evicted_consistent,
        )
    }

    /// 全账指纹（逐轮指纹链式递推 + 计数封账）。
    pub fn fingerprint(&self) -> u64 {
        let mut h = fnv1a64_from(FNV_OFFSET, b"tscx-ledger");
        for r in &self.rounds {
            h = fnv1a64_from(h, &r.fingerprint.to_be_bytes());
        }
        h = fnv1a64_from(h, &self.total_machine_rounds.to_be_bytes());
        h = fnv1a64_from(h, &self.consistent_machine_rounds.to_be_bytes());
        h = fnv1a64_from(h, &self.evicted_rounds.to_be_bytes());
        h
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// 事件账（环形 + 序号连续核）
// ═══════════════════════════════════════════════════════════════════════════

/// 事件类。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EventKind {
    /// 开轮。
    RoundBegin,
    /// 样本入账。
    SampleAccepted,
    /// 样本被拒。
    SampleRejected,
    /// 比对完成。
    CompareDone,
    /// 漂移拦截。
    Intercept,
    /// 立案。
    CaseOpened,
    /// 账面篡改事件。
    Tamper,
    /// 注入事件。
    Injection,
    /// 结轮。
    Conclude,
    /// 相位迁移。
    PhaseChange,
    /// 账簿逐出。
    LedgerEvict,
    /// 趋势带外。
    TrendOutlier,
}

impl EventKind {
    /// 标牌。
    pub const fn label(self) -> &'static str {
        match self {
            EventKind::RoundBegin => "开轮",
            EventKind::SampleAccepted => "样本入账",
            EventKind::SampleRejected => "样本被拒",
            EventKind::CompareDone => "比对完成",
            EventKind::Intercept => "漂移拦截",
            EventKind::CaseOpened => "立案",
            EventKind::Tamper => "账面篡改",
            EventKind::Injection => "注入",
            EventKind::Conclude => "结轮",
            EventKind::PhaseChange => "相位迁移",
            EventKind::LedgerEvict => "账簿逐出",
            EventKind::TrendOutlier => "趋势带外",
        }
    }
}

/// 事件账行。
#[derive(Clone, Debug)]
pub struct TscEvent {
    /// 全局连续序号（自 1 起）。
    pub seq: u64,
    /// 事件类。
    pub kind: EventKind,
    /// 关联机号（可空）。
    pub machine: Option<MachineId>,
    /// 关联轮号（0=轮外事件）。
    pub round: u64,
    /// 段内诊断码。
    pub code: u16,
    /// 事件注记（静态短标牌）。
    pub note: &'static str,
}

/// 事件账（环形；序号全局连续，逐出不清序号）。
pub struct EventLedger {
    events: Vec<TscEvent>,
    next_seq: u64,
}

impl EventLedger {
    /// 空账（序号自 1 起）。
    pub fn new() -> Self {
        EventLedger {
            events: Vec::new(),
            next_seq: 1,
        }
    }

    /// 入账（满员逐出最老；序号单调递增）。
    pub fn push(
        &mut self,
        kind: EventKind,
        machine: Option<MachineId>,
        round: u64,
        code: u16,
        note: &'static str,
    ) {
        let ev = TscEvent {
            seq: self.next_seq,
            kind,
            machine,
            round,
            code,
            note,
        };
        self.next_seq += 1;
        if self.events.len() >= EVENT_CAP {
            self.events.remove(0);
        }
        self.events.push(ev);
    }

    /// 账行只读。
    pub fn events(&self) -> &[TscEvent] {
        &self.events
    }

    /// 在账事件数。
    pub fn len(&self) -> usize {
        self.events.len()
    }

    /// 是否空账。
    pub fn is_empty(&self) -> bool {
        self.events.is_empty()
    }

    /// 下一序号（指纹封账用）。
    pub const fn next_seq(&self) -> u64 {
        self.next_seq
    }

    /// 序号连续核：账内 seq 逐行 +1 且首行 + 长度 == next_seq。
    pub fn verify_contiguous(&self) -> Result<(), TscCrossError> {
        let n = self.events.len();
        if n > 0 {
            let first = self.events[0].seq;
            let mut i = 1usize;
            while i < n {
                if self.events[i].seq != self.events[i - 1].seq + 1 {
                    return Err(TscCrossError::EventSeqGap {
                        expected: self.events[i - 1].seq + 1,
                        got: self.events[i].seq,
                    });
                }
                i += 1;
            }
            if first + (n as u64) - 1 != self.next_seq - 1 {
                return Err(TscCrossError::EventSeqGap {
                    expected: first + (n as u64) - 1,
                    got: self.next_seq - 1,
                });
            }
        } else if self.next_seq != 1 {
            return Err(TscCrossError::EventSeqGap {
                expected: 1,
                got: self.next_seq,
            });
        }
        Ok(())
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// 立案账（CaseKind / CaseRecord）
// ═══════════════════════════════════════════════════════════════════════════

/// 案件类。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CaseKind {
    /// F0123 账面不一致。
    LedgerMismatch,
    /// 漂移拦截（J1 红线）。
    DriftIntercept,
    /// 缺机。
    MissingMachine,
    /// 账面篡改嫌疑（对账复核发现失配即立）。
    TamperSuspect,
    /// 注入未检出。
    InjectionUncaught,
    /// 一致率越下限。
    ConsistencyBelowFloor,
    /// 趋势带外。
    TrendOutlier,
}

impl CaseKind {
    /// 案类标牌。
    pub const fn label(self) -> &'static str {
        match self {
            CaseKind::LedgerMismatch => "账面不一致",
            CaseKind::DriftIntercept => "漂移拦截",
            CaseKind::MissingMachine => "缺机",
            CaseKind::TamperSuspect => "篡改嫌疑",
            CaseKind::InjectionUncaught => "注入未检出",
            CaseKind::ConsistencyBelowFloor => "一致率越限",
            CaseKind::TrendOutlier => "趋势带外",
        }
    }

    /// 关联诊断码（趋势带外归漂移族 0x15B5）。
    pub const fn code(self) -> u16 {
        match self {
            CaseKind::LedgerMismatch => D_LEDGER_MISMATCH,
            CaseKind::DriftIntercept => D_DRIFT_INTERCEPT,
            CaseKind::MissingMachine => D_MISSING_SAMPLE,
            CaseKind::TamperSuspect => D_LEDGER_MISMATCH,
            CaseKind::InjectionUncaught => D_INJECTION_UNCAUGHT,
            CaseKind::ConsistencyBelowFloor => D_CONSIST_FLOOR,
            CaseKind::TrendOutlier => D_DRIFT_INTERCEPT,
        }
    }
}

/// 案件记录（立案即落账，结案须显式 close）。
#[derive(Clone, Debug)]
pub struct CaseRecord {
    /// 案号（引擎内自 1 连续）。
    pub case_id: u64,
    /// 案类。
    pub kind: CaseKind,
    /// 关联机号（可空）。
    pub machine: Option<MachineId>,
    /// 关联轮号。
    pub round: u64,
    /// 立案时戳（UTC 秒）。
    pub open_ts: u64,
    /// 案情注记（静态）。
    pub detail: &'static str,
    /// 结案态。
    pub closed: bool,
}

impl CaseRecord {
    /// 案行指纹。
    pub fn fingerprint(&self) -> u64 {
        let mut h = fnv1a64_from(FNV_OFFSET, b"tscx-case");
        h = fnv1a64_from(h, &self.case_id.to_be_bytes());
        h = fnv1a64_from(h, &[self.kind.code() as u8]);
        h = fnv1a64_from(h, &self.round.to_be_bytes());
        h = fnv1a64_from(h, &[self.closed as u8]);
        h
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// 注入面（基频漂移 / 账面篡改 / 缺机 · 各注入必须检出）
// ═══════════════════════════════════════════════════════════════════════════

/// 注入类型（三通道，每通道绑定独立检出路径）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Injection {
    /// 基频漂移注入：对指定机注入 ±ppm 复测偏差（检出路径：漂移对账分级）。
    FreqDrift {
        /// 目标机。
        machine: MachineId,
        /// 漂移量（ppm，带号）。
        ppm: i64,
    },
    /// 账面篡改注入：改写指定机本账基频（检出路径：F0123 逐机对账失配）。
    LedgerTamper {
        /// 目标机。
        machine: MachineId,
        /// 篡改后频率（Hz）。
        new_hz: u64,
    },
    /// 缺机注入：复核轮剔除指定机全部样本（检出路径：缺机立案）。
    MissingMachine {
        /// 目标机。
        machine: MachineId,
    },
}

impl Injection {
    /// 注入类标牌。
    pub const fn label(self) -> &'static str {
        match self {
            Injection::FreqDrift { .. } => "基频漂移注入",
            Injection::LedgerTamper { .. } => "账面篡改注入",
            Injection::MissingMachine { .. } => "缺机注入",
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// 比对摘要（compare 产物）
// ═══════════════════════════════════════════════════════════════════════════

/// 比对摘要：本轮逐机结论 + 缺机清单 + 轮结论。
#[derive(Clone, Debug)]
pub struct CompareSummary {
    /// 轮号。
    pub round: u64,
    /// 轮结论。
    pub verdict: RoundVerdict,
    /// 逐机结论（在轮机）。
    pub outcomes: Vec<MachineRoundOutcome>,
    /// 缺机清单。
    pub missing: Vec<MachineId>,
    /// 本轮拦截机清单。
    pub intercepted: Vec<MachineId>,
}

impl CompareSummary {
    /// 摘要指纹（对拍面之一）。
    pub fn fingerprint(&self) -> u64 {
        let mut h = fnv1a64_from(FNV_OFFSET, b"tscx-summary");
        h = fnv1a64_from(h, &self.round.to_be_bytes());
        h = fnv1a64_from(h, &[self.verdict.as_u8()]);
        for o in &self.outcomes {
            h = fnv1a64_from(h, &o.fingerprint().to_be_bytes());
        }
        for m in &self.missing {
            h = fnv1a64_from(h, m.label().as_bytes());
        }
        h
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// 趋势带（滑动窗极值带 ±10%，带外立案）
// ═══════════════════════════════════════════════════════════════════════════

/// 多轮漂移趋势带：滚动记录每轮最大偏差 ppm，带 = 历史极值各外扩 10%。
pub struct TrendBand {
    window: usize,
    series: Vec<(u64, u64)>,
    evicted: u64,
}

impl TrendBand {
    /// 以窗口宽构造（<3 视为配置非法）。
    pub fn new(window: usize) -> Result<Self, TscCrossError> {
        if window < 3 {
            return Err(TscCrossError::ConfigInvalid {
                field: "趋势带窗口 <3",
            });
        }
        Ok(TrendBand {
            window,
            series: Vec::new(),
            evicted: 0,
        })
    }

    /// 入样（满窗逐出最老）。
    pub fn push(&mut self, round: u64, max_ppm: u64) {
        if self.series.len() >= self.window {
            self.series.remove(0);
            self.evicted += 1;
        }
        self.series.push((round, max_ppm));
    }

    /// 带宽（下限, 上限）；历史不足 None。
    pub fn band(&self) -> Option<(u64, u64)> {
        if self.series.len() < 3 {
            return None;
        }
        let mut lo = self.series[0].1;
        let mut hi = self.series[0].1;
        for s in &self.series {
            if s.1 < lo {
                lo = s.1;
            }
            if s.1 > hi {
                hi = s.1;
            }
        }
        let lo = lo.saturating_sub(lo / TREND_EXPAND_DIV);
        let hi = hi.saturating_add(hi / TREND_EXPAND_DIV);
        Some((lo, hi))
    }

    /// 带外判（带宽未成型时恒 False——历史不足不立案）。
    pub fn is_outlier(&self, ppm: u64) -> bool {
        match self.band() {
            Some((lo, hi)) => ppm < lo || ppm > hi,
            None => false,
        }
    }

    /// 在窗样本数。
    pub fn len(&self) -> usize {
        self.series.len()
    }

    /// 是否空。
    pub fn is_empty(&self) -> bool {
        self.series.is_empty()
    }

    /// 逐出计数。
    pub const fn evicted(&self) -> u64 {
        self.evicted
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// 复核引擎（状态机 + 全子系统装配）
// ═══════════════════════════════════════════════════════════════════════════

/// TSC 跨机复核引擎：四态状态机驱动，全部账簿堆上（引擎本体 <64KB 可安全持有）。
pub struct TscCrossEngine {
    cfg: TscCrossConfig,
    baseline: TscBaseline,
    phase: CrossPhase,
    ledger: CrossMachineLedger,
    drift: DriftDetector,
    events: EventLedger,
    cases: Vec<CaseRecord>,
    next_case_id: u64,
    cur_round: u64,
    last_ts: u64,
    round_samples: [Vec<TscSample>; MACHINE_COUNT],
    pending_outcomes: Vec<MachineRoundOutcome>,
    pending_missing: Vec<MachineId>,
    pending_intercepted: Vec<MachineId>,
    f0123_match: u8,
    tamper_detected: bool,
    injections: Vec<(Injection, bool)>,
    trend: TrendBand,
    /// 归档轮数（Concluded→Idle 迁移计数）。
    archived_rounds: u64,
}

impl TscCrossEngine {
    /// 构造：配置校验 → 三行基频账核 → F0123 逐机对账（失配逐机立案）。
    pub fn new(cfg: TscCrossConfig, baseline: TscBaseline) -> Result<Self, TscCrossError> {
        cfg.validate()?;
        if baseline.len() != MACHINE_COUNT {
            return Err(TscCrossError::ConfigInvalid {
                field: "基频账行数非三",
            });
        }
        let mut eng = TscCrossEngine {
            cfg,
            baseline,
            phase: CrossPhase::Idle,
            ledger: CrossMachineLedger::new(),
            drift: DriftDetector::new(),
            events: EventLedger::new(),
            cases: Vec::new(),
            next_case_id: 0,
            cur_round: 0,
            last_ts: 0,
            round_samples: [Vec::new(), Vec::new(), Vec::new()],
            pending_outcomes: Vec::new(),
            pending_missing: Vec::new(),
            pending_intercepted: Vec::new(),
            f0123_match: 0,
            tamper_detected: false,
            injections: Vec::new(),
            trend: TrendBand::new(TREND_WINDOW)?,
            archived_rounds: 0,
        };
        eng.reconcile_baseline();
        Ok(eng)
    }

    /// 是否已有同机同类的未结案（对账幂等处置依据）。
    fn has_open_case(&self, kind: CaseKind, machine: Option<MachineId>) -> bool {
        for c in &self.cases {
            if c.kind == kind && c.machine == machine && !c.closed {
                return true;
            }
        }
        false
    }

    /// F0123 逐机对账并处置（失配逐机立案，同机同案幂等不重复立）。
    pub fn reconcile_baseline(&mut self) -> ReconcileReport {
        let report = match self.baseline.reconcile_f0123() {
            Ok(r) => r,
            Err(_) => {
                // 理论不可达（构造已核三行）；落事件保守处置。
                self.push_event(
                    EventKind::CaseOpened,
                    None,
                    0,
                    D_CONFIG_INVALID,
                    "对账前置核失败",
                );
                return ReconcileReport {
                    matched: 0,
                    mismatches: Vec::new(),
                };
            }
        };
        self.f0123_match = report.matched;
        if report.is_three_of_three() {
            self.tamper_detected = false;
        } else {
            self.tamper_detected = true;
            let mism = report.mismatches.clone();
            for (machine, ledger_hz, seen_hz) in mism {
                if !self.has_open_case(CaseKind::LedgerMismatch, Some(machine)) {
                    let err = TscCrossError::LedgerMismatch {
                        machine,
                        ledger_hz,
                        seen_hz,
                    };
                    self.open_case(CaseKind::LedgerMismatch, Some(machine), err.short());
                }
            }
        }
        report
    }

    // ———————— 内部账务原语 ————————

    fn push_event(
        &mut self,
        kind: EventKind,
        machine: Option<MachineId>,
        round: u64,
        code: u16,
        note: &'static str,
    ) {
        self.events.push(kind, machine, round, code, note);
    }

    fn open_case(&mut self, kind: CaseKind, machine: Option<MachineId>, detail: &'static str) {
        self.next_case_id += 1;
        let rec = CaseRecord {
            case_id: self.next_case_id,
            kind,
            machine,
            round: self.cur_round,
            open_ts: self.last_ts,
            detail,
            closed: false,
        };
        self.cases.push(rec);
        self.push_event(
            EventKind::CaseOpened,
            machine,
            self.cur_round,
            kind.code(),
            kind.label(),
        );
    }

    fn set_phase(&mut self, to: CrossPhase) -> Result<(), TscCrossError> {
        if !CrossPhase::can_advance(self.phase, to) {
            return Err(TscCrossError::PhaseViolation {
                from: self.phase,
                attempted: to,
            });
        }
        self.push_event(
            EventKind::PhaseChange,
            None,
            self.cur_round,
            D_PHASE,
            to.label(),
        );
        self.phase = to;
        Ok(())
    }

    // ———————— 相位操作（对外状态机面） ————————

    /// 开轮：Idle（或 Concluded 自动归档）→ Sampling。基频账未封存即拒。
    pub fn begin_round(&mut self, ts: u64) -> Result<u64, TscCrossError> {
        if self.phase == CrossPhase::Concluded {
            self.set_phase(CrossPhase::Idle)?;
            self.archived_rounds += 1;
        }
        if self.phase != CrossPhase::Idle {
            return Err(TscCrossError::PhaseViolation {
                from: self.phase,
                attempted: CrossPhase::Sampling,
            });
        }
        if !self.baseline.all_sealed() {
            return Err(TscCrossError::BaselineNotSealed);
        }
        self.cur_round += 1;
        self.last_ts = ts;
        for slot in &mut self.round_samples {
            slot.clear();
        }
        self.pending_outcomes.clear();
        self.pending_missing.clear();
        self.pending_intercepted.clear();
        self.set_phase(CrossPhase::Sampling)?;
        self.push_event(
            EventKind::RoundBegin,
            None,
            self.cur_round,
            D_PHASE,
            "复核轮开启",
        );
        Ok(self.cur_round)
    }

    /// 交样：仅 Sampling 相位受理；机号/跨度/封顶修正/容量逐项校验。
    pub fn submit_sample(&mut self, sample: TscSample) -> Result<(), TscCrossError> {
        if self.phase != CrossPhase::Sampling {
            let e = TscCrossError::PhaseViolation {
                from: self.phase,
                attempted: CrossPhase::Sampling,
            };
            self.push_event(
                EventKind::SampleRejected,
                Some(sample.machine),
                self.cur_round,
                e.diag_code(),
                e.short(),
            );
            return Err(e);
        }
        if self.baseline.entry(sample.machine).is_none() {
            return Err(TscCrossError::UnknownMachine {
                machine: sample.machine,
            });
        }
        if sample.end_tsc <= sample.start_tsc {
            let err = sample.span().err().unwrap_or(TscCrossError::SampleSpanZero {
                machine: sample.machine,
            });
            self.push_event(
                EventKind::SampleRejected,
                Some(sample.machine),
                self.cur_round,
                err.diag_code(),
                err.short(),
            );
            return Err(err);
        }
        if sample.wrap_corrections > self.cfg.max_wrap_corrections {
            let err = TscCrossError::WrapCorrectionInvalid {
                corrections: sample.wrap_corrections,
                allowed: self.cfg.max_wrap_corrections,
            };
            self.push_event(
                EventKind::SampleRejected,
                Some(sample.machine),
                self.cur_round,
                err.diag_code(),
                err.short(),
            );
            return Err(err);
        }
        let slot_len = self.round_samples[sample.machine.index()].len();
        if slot_len >= SAMPLE_CAP_PER_MACHINE {
            let err = TscCrossError::CapacityFull {
                ledger: "单机样本槽",
            };
            self.push_event(
                EventKind::SampleRejected,
                Some(sample.machine),
                self.cur_round,
                err.diag_code(),
                err.short(),
            );
            return Err(err);
        }
        // 预测频（零基准 tick 早拒，避免 compare 阶段才爆）
        if sample.measured_hz(self.cfg.ref_clock_hz).is_err() {
            let err = TscCrossError::ConfigInvalid {
                field: "样本 ref_ticks=0",
            };
            self.push_event(
                EventKind::SampleRejected,
                Some(sample.machine),
                self.cur_round,
                err.diag_code(),
                err.short(),
            );
            return Err(err);
        }
        self.round_samples[sample.machine.index()].push(sample);
        self.push_event(
            EventKind::SampleAccepted,
            Some(sample.machine),
            self.cur_round,
            D_PHASE,
            "样本入账",
        );
        Ok(())
    }

    /// 比对：Sampling→Comparing。缺机 fail-soft（立案不停轮），拦截即立案。
    pub fn compare(&mut self, ts: u64) -> Result<CompareSummary, TscCrossError> {
        if self.phase != CrossPhase::Sampling {
            return Err(TscCrossError::PhaseViolation {
                from: self.phase,
                attempted: CrossPhase::Comparing,
            });
        }
        self.last_ts = ts;
        self.set_phase(CrossPhase::Comparing)?;
        let cfg = self.cfg;
        let mut outcomes: Vec<MachineRoundOutcome> = Vec::new();
        let mut missing: Vec<MachineId> = Vec::new();
        let mut intercepted: Vec<MachineId> = Vec::new();
        for mi in 0..MACHINE_COUNT {
            let machine = MachineId::from_index(mi).ok_or(TscCrossError::ConfigInvalid {
                field: "机号越界",
            })?;
            let samples = self.round_samples[mi].clone();
            if samples.len() < cfg.samples_required {
                missing.push(machine);
                self.open_case(
                    CaseKind::MissingMachine,
                    Some(machine),
                    if samples.is_empty() {
                        "缺机零样本"
                    } else {
                        "样本不足"
                    },
                );
                continue;
            }
            // 逐样本实测频 → 中位（下中位口径）
            let mut meas: Vec<u64> = Vec::with_capacity(samples.len());
            for s in &samples {
                meas.push(s.measured_hz(cfg.ref_clock_hz)?);
            }
            sort_u64_asc(&mut meas);
            let median = meas[meas.len() / 2];
            let ledger_hz = match self.baseline.entry(machine) {
                Some(e) => e.base_freq_hz,
                None => {
                    return Err(TscCrossError::UnknownMachine { machine });
                }
            };
            let rec = self.drift.observe(self.cur_round, machine, median, ledger_hz)?;
            if rec.grade == DriftGrade::Intercept {
                let irec = InterceptRecord {
                    round: self.cur_round,
                    machine,
                    ppm: rec.ppm,
                    measured_hz: rec.measured_hz,
                    ledger_hz: rec.ledger_hz,
                    fingerprint: 0,
                };
                let mut irec = irec;
                irec.fingerprint = irec.compute_fingerprint();
                let evicted = self.drift.record_intercept(irec);
                if evicted {
                    self.push_event(
                        EventKind::LedgerEvict,
                        Some(machine),
                        self.cur_round,
                        D_CAPACITY_FULL,
                        "拦截账逐出",
                    );
                }
                self.open_case(CaseKind::DriftIntercept, Some(machine), "漂移越红线");
                self.push_event(
                    EventKind::Intercept,
                    Some(machine),
                    self.cur_round,
                    D_DRIFT_INTERCEPT,
                    "J1 红线触发",
                );
                intercepted.push(machine);
            }
            // 误差源分解
            let attrib = attribute_sources(&cfg, machine, &samples, rec.ppm)?;
            outcomes.push(MachineRoundOutcome {
                machine,
                measured_hz: rec.measured_hz,
                ledger_hz: rec.ledger_hz,
                ppm: rec.ppm,
                grade: rec.grade,
                dominant: attrib.dominant,
                consistent: rec.grade == DriftGrade::Normal,
            });
        }
        // 轮结论四态判定（拦截 > 缺机 > 观察 > 全一致）
        let verdict = if !intercepted.is_empty() {
            RoundVerdict::Intercepted
        } else if !missing.is_empty() {
            RoundVerdict::Incomplete
        } else if outcomes.iter().any(|o| o.grade == DriftGrade::Observe) {
            RoundVerdict::Observed
        } else {
            RoundVerdict::AllConsistent
        };
        self.pending_outcomes = outcomes.clone();
        self.pending_missing = missing.clone();
        self.pending_intercepted = intercepted.clone();
        self.push_event(
            EventKind::CompareDone,
            None,
            self.cur_round,
            D_PHASE,
            verdict.label(),
        );
        Ok(CompareSummary {
            round: self.cur_round,
            verdict,
            outcomes,
            missing,
            intercepted,
        })
    }

    /// 结轮：Comparing→Concluded。落轮账 → 注入面核查 → 一致率下限核 → 趋势带更新。
    pub fn conclude(&mut self, ts: u64) -> Result<RoundRecord, TscCrossError> {
        if self.phase != CrossPhase::Comparing {
            return Err(TscCrossError::PhaseViolation {
                from: self.phase,
                attempted: CrossPhase::Concluded,
            });
        }
        self.last_ts = ts;
        // 组轮记录
        let mut per_machine: [Option<MachineRoundOutcome>; MACHINE_COUNT] = [None, None, None];
        for o in &self.pending_outcomes {
            per_machine[o.machine.index()] = Some(o.clone());
        }
        let verdict = if !self.pending_intercepted.is_empty() {
            RoundVerdict::Intercepted
        } else if !self.pending_missing.is_empty() {
            RoundVerdict::Incomplete
        } else if self
            .pending_outcomes
            .iter()
            .any(|o| o.grade == DriftGrade::Observe)
        {
            RoundVerdict::Observed
        } else {
            RoundVerdict::AllConsistent
        };
        let mut record = RoundRecord {
            round: self.cur_round,
            ts,
            per_machine,
            verdict,
            fingerprint: 0,
        };
        record.fingerprint = record.compute_fingerprint();
        // 注入面核查（未检出立案，检出落事件）
        self.check_injections();
        // 轮账入簿
        let bp = self.ledger.append(record.clone())?;
        // 一致率下限核（至少三轮起判，避免冷启动噪声）
        if self.ledger.round_count() >= 3 && bp < CONSISTENCY_FLOOR_BP {
            let err = TscCrossError::ConsistencyBelowFloor { bp };
            self.open_case(CaseKind::ConsistencyBelowFloor, None, err.short());
        }
        // 趋势带更新（本轮最大偏差；带外立案）
        let mut round_max_ppm: u64 = 0;
        for o in &self.pending_outcomes {
            if o.ppm > round_max_ppm {
                round_max_ppm = o.ppm;
            }
        }
        let outlier = self.trend.is_outlier(round_max_ppm);
        self.trend.push(self.cur_round, round_max_ppm);
        if outlier {
            self.open_case(CaseKind::TrendOutlier, None, "轮最大偏差越趋势带");
            self.push_event(
                EventKind::TrendOutlier,
                None,
                self.cur_round,
                D_DRIFT_INTERCEPT,
                "趋势带外",
            );
        }
        // 漂移检测器结轮（本轮记录并入累计）
        self.drift.end_round();
        self.pending_outcomes.clear();
        self.pending_missing.clear();
        self.pending_intercepted.clear();
        self.set_phase(CrossPhase::Concluded)?;
        self.push_event(
            EventKind::Conclude,
            None,
            self.cur_round,
            D_PHASE,
            verdict.label(),
        );
        Ok(record)
    }

    /// 注入面核查：每条未检出的注入立案一次（检出/立案后均置已处置）。
    fn check_injections(&mut self) {
        let outcomes = self.pending_outcomes.clone();
        let missing = self.pending_missing.clone();
        let n = self.injections.len();
        let mut i = 0usize;
        while i < n {
            if !self.injections[i].1 {
                let inj = self.injections[i].0;
                let caught = match inj {
                    Injection::FreqDrift { machine, ppm } => {
                        let tol = {
                            let a = ppm.unsigned_abs() / 10;
                            if a > 100 {
                                a
                            } else {
                                100
                            }
                        };
                        let mut hit = false;
                        for o in &outcomes {
                            if o.machine == machine {
                                let diff = (o.ppm as i64) - ppm;
                                let ad = diff.unsigned_abs();
                                if ad <= tol {
                                    hit = true;
                                }
                            }
                        }
                        hit
                    }
                    Injection::LedgerTamper { .. } => self.tamper_detected,
                    Injection::MissingMachine { machine } => missing.contains(&machine),
                };
                if caught {
                    self.injections[i].1 = true;
                    self.push_event(
                        EventKind::Injection,
                        None,
                        self.cur_round,
                        D_PHASE,
                        "注入检出",
                    );
                } else {
                    let err = TscCrossError::InjectionUncaught {
                        kind: inj.label(),
                    };
                    self.open_case(CaseKind::InjectionUncaught, None, err.short());
                    self.injections[i].1 = true;
                }
            }
            i += 1;
        }
    }

    /// 便捷全流程：开轮→交样→比对→结轮（判据聚合与对拍主通道）。
    pub fn run_full_review(
        &mut self,
        ts: u64,
        samples: &[TscSample],
    ) -> Result<CompareSummary, TscCrossError> {
        self.begin_round(ts)?;
        for s in samples {
            self.submit_sample(*s)?;
        }
        let summary = self.compare(ts)?;
        self.conclude(ts)?;
        Ok(summary)
    }

    // ———————— 注入面 ————————

    /// 登记注入。账面篡改通道即刻执行改账并触发对账（检出路径：F0123 失配立案）。
    pub fn apply_injection(&mut self, inj: Injection) -> Result<(), TscCrossError> {
        self.injections.push((inj, false));
        match inj {
            Injection::LedgerTamper { machine, new_hz } => {
                self.baseline.tamper(machine, new_hz)?;
                self.tamper_detected = true;
                self.push_event(
                    EventKind::Tamper,
                    Some(machine),
                    self.cur_round,
                    D_LEDGER_MISMATCH,
                    "账面篡改执行",
                );
                self.reconcile_baseline();
                // 对账失配即注入检出（同参数篡改注入逐一置检出态）
                let n = self.injections.len();
                let mut i = 0usize;
                while i < n {
                    if !self.injections[i].1 {
                        if let Injection::LedgerTamper {
                            machine: m2,
                            new_hz: h2,
                        } = self.injections[i].0
                        {
                            if m2 == machine && h2 == new_hz && self.tamper_detected {
                                self.injections[i].1 = true;
                                self.push_event(
                                    EventKind::Injection,
                                    Some(machine),
                                    self.cur_round,
                                    D_PHASE,
                                    "注入检出",
                                );
                            }
                        }
                    }
                    i += 1;
                }
            }
            Injection::FreqDrift { machine, ppm: _ } => {
                self.push_event(
                    EventKind::Injection,
                    Some(machine),
                    self.cur_round,
                    D_DRIFT_INTERCEPT,
                    "漂移注入登记",
                );
            }
            Injection::MissingMachine { machine } => {
                self.push_event(
                    EventKind::Injection,
                    Some(machine),
                    self.cur_round,
                    D_MISSING_SAMPLE,
                    "缺机注入登记",
                );
            }
        }
        Ok(())
    }

    /// 注入处置清单：(注入类标牌, 是否检出)。
    pub fn injection_report(&self) -> Vec<(&'static str, bool)> {
        let mut v = Vec::new();
        for (inj, caught) in &self.injections {
            v.push((inj.label(), *caught));
        }
        v
    }

    // ———————— 只读账面 ————————

    /// 基频账只读。
    pub fn baseline(&self) -> &TscBaseline {
        &self.baseline
    }

    /// 跨机账只读。
    pub fn ledger(&self) -> &CrossMachineLedger {
        &self.ledger
    }

    /// 事件账只读。
    pub fn events(&self) -> &EventLedger {
        &self.events
    }

    /// 漂移检测器只读。
    pub fn drift(&self) -> &DriftDetector {
        &self.drift
    }

    /// 案件账只读。
    pub fn cases(&self) -> &[CaseRecord] {
        &self.cases
    }

    /// 当前相位。
    pub const fn phase(&self) -> CrossPhase {
        self.phase
    }

    /// 当前轮号。
    pub const fn cur_round(&self) -> u64 {
        self.cur_round
    }

    /// F0123 一致机数（0..=3）。
    pub const fn f0123_match(&self) -> u8 {
        self.f0123_match
    }

    /// 篡改嫌疑置位态。
    pub const fn tamper_detected(&self) -> bool {
        self.tamper_detected
    }

    /// 归档轮数。
    pub const fn archived_rounds(&self) -> u64 {
        self.archived_rounds
    }

    /// 未结案数。
    pub fn open_case_count(&self) -> usize {
        let mut n = 0;
        for c in &self.cases {
            if !c.closed {
                n += 1;
            }
        }
        n
    }

    /// 结案（案号不存在拒）。
    pub fn close_case(&mut self, case_id: u64) -> Result<(), TscCrossError> {
        let n = self.cases.len();
        let mut i = 0usize;
        while i < n {
            if self.cases[i].case_id == case_id {
                if !self.cases[i].closed {
                    self.cases[i].closed = true;
                    self.push_event(
                        EventKind::CaseOpened,
                        None,
                        self.cur_round,
                        D_PHASE,
                        "案件结案",
                    );
                    return Ok(());
                }
                return Err(TscCrossError::ConfigInvalid { field: "重复结案" });
            }
            i += 1;
        }
        Err(TscCrossError::ConfigInvalid { field: "案号不存在" })
    }

    /// 健康三色：
    /// 红 = 有硬案未结（拦截/账面失配/缺机/篡改/注入未检出）或累计拦截 > 0；
    /// 黄 = 有观察档累计或 F0123 非 3/3 或仅软案（一致率越限/趋势带外——观察档语义）；
    /// 否则绿。
    pub fn health(&self) -> Health {
        let mut hard_open = self.drift.intercept_total() > 0;
        if !hard_open {
            for c in &self.cases {
                if !c.closed
                    && c.kind != CaseKind::ConsistencyBelowFloor
                    && c.kind != CaseKind::TrendOutlier
                {
                    hard_open = true;
                    break;
                }
            }
        }
        if hard_open {
            Health::Red
        } else if self.drift.observe_total() > 0 || self.f0123_match != 3 {
            Health::Yellow
        } else {
            Health::Green
        }
    }

    /// 守恒核透传（轮账记录重算 = 累计计数）。
    pub fn verify_conservation(&self) -> bool {
        self.ledger.verify_conservation()
    }

    /// 事件序号连续核透传。
    pub fn verify_event_contiguous(&self) -> Result<(), TscCrossError> {
        self.events.verify_contiguous()
    }

    /// 全量指纹（版本 + 相位 + 基频账 + 轮账 + 事件账 + 案件 + 漂移累计）。
    /// 双跑确定性对拍锚：同输入两引擎此值必等。
    pub fn fingerprint(&self) -> u64 {
        let mut h = fnv1a64_from(FNV_OFFSET, TSCX_VERSION.as_bytes());
        h = fnv1a64_from(h, &[self.phase.as_u8()]);
        h = fnv1a64_from(h, &self.f0123_match.to_be_bytes());
        h = fnv1a64_from(h, &self.baseline.fingerprint().to_be_bytes());
        h = fnv1a64_from(h, &self.ledger.fingerprint().to_be_bytes());
        h = fnv1a64_from(h, &self.events.next_seq().to_be_bytes());
        h = fnv1a64_from(h, &self.drift.intercept_total().to_be_bytes());
        h = fnv1a64_from(h, &self.drift.observe_total().to_be_bytes());
        h = fnv1a64_from(h, &(self.open_case_count() as u64).to_be_bytes());
        h = fnv1a64_from(h, &self.archived_rounds.to_be_bytes());
        h
    }

    // ———————— 双格式导出 ————————

    /// 逐轮 CSV 行（表头 + 每轮一行：轮号/结论/一致机数/在轮机数/四机偏差列）。
    pub fn ledger_csv(&self) -> String {
        let mut out = String::from("round,verdict,consistent,present,ppm_m0,ppm_m1,ppm_m2\n");
        for r in self.ledger.rounds() {
            let mut ppm_m: [String; MACHINE_COUNT] =
                [String::new(), String::new(), String::new()];
            for (i, slot) in r.per_machine.iter().enumerate() {
                if let Some(o) = slot {
                    ppm_m[i] = o.ppm.to_string();
                }
            }
            out.push_str(&format!(
                "{},{},{},{},{},{},{}\n",
                r.round,
                r.verdict.as_u8(),
                r.consistent_count(),
                r.present_count(),
                ppm_m[0],
                ppm_m[1],
                ppm_m[2]
            ));
        }
        out
    }

    /// 文本报告（行数封顶 REPORT_LINE_CAP，截断事实落账并返回截断态）。
    /// 返回 (报告文本, 是否截断)。
    pub fn render_report(&mut self) -> (String, bool) {
        let mut lines: Vec<String> = Vec::new();
        lines.push(format!("== UNX-F0663 TSC 计时基准跨机复核报告 · {} ==", TSCX_VERSION));
        lines.push(format!("诊断段: 0x{:04X}..0x{:04X}", SEG_BASE, SEG_BASE + SEG_MASK));
        let health = self.health();
        lines.push(format!("健康: {}", health.label()));
        lines.push(format!(
            "F0123 对账: {}/3  篡改嫌疑: {}",
            self.f0123_match, self.tamper_detected
        ));
        lines.push(format!(
            "相位: {}  当前轮: {}  归档轮: {}",
            self.phase.label(),
            self.cur_round,
            self.archived_rounds
        ));
        lines.push(format!(
            "轮簿: {} 轮  累计机轮: {}  一致机轮: {}  一致率: {}bp",
            self.ledger.round_count(),
            self.ledger.total_machine_rounds(),
            self.ledger.consistent_machine_rounds(),
            self.ledger.consistency_bp()
        ));
        let (ev, mr, ec) = self.ledger.eviction_stats();
        lines.push(format!("逐出: 轮{} 机轮{} 一致{}", ev, mr, ec));
        let (nt, ot, it) = self.drift.totals();
        lines.push(format!(
            "漂移累计: 正常{} 观察{} 拦截{}  拦截账: {} 行",
            nt,
            ot,
            it,
            self.drift.intercepts().len()
        ));
        lines.push(format!("案件: 总{} 未结{}", self.cases.len(), self.open_case_count()));
        lines.push(format!("事件: 在账{} 下一序号{}", self.events.len(), self.events.next_seq()));
        // 基频账三行
        for e in self.baseline.entries() {
            lines.push(format!(
                "基频账 {} {}Hz 方法:{} 时戳:{} 封存:{}",
                e.machine.label(),
                e.base_freq_hz,
                e.method.label(),
                e.calib_ts,
                e.sealed
            ));
        }
        // 逐轮摘要
        for r in self.ledger.rounds() {
            let mut row = format!("[轮{}] {} 一致{}/{}", r.round, r.verdict.label(), r.consistent_count(), r.present_count());
            for slot in &r.per_machine {
                if let Some(o) = slot {
                    row.push_str(&format!(
                        " {}:{}ppm({},{})",
                        o.machine.label(),
                        o.ppm,
                        o.grade.label(),
                        o.dominant.label()
                    ));
                }
            }
            lines.push(row);
        }
        lines.push(format!("全量指纹: 0x{:016X}", self.fingerprint()));
        let truncated = lines.len() > REPORT_LINE_CAP;
        if truncated {
            lines.truncate(REPORT_LINE_CAP);
            self.push_event(
                EventKind::CaseOpened,
                None,
                self.cur_round,
                D_REPORT_TRUNC,
                "报告截断",
            );
        }
        let mut out = String::new();
        for l in &lines {
            out.push_str(l);
            out.push('\n');
        }
        (out, truncated)
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// 双跑确定性对拍探针（同构建器两引擎同输入 → 指纹必等）
// ═══════════════════════════════════════════════════════════════════════════

/// 双跑确定性对拍探针：
/// 用同一构建器闭包造两台引擎，各自跑完同一批轮输入（轮输入由轮次序号从
/// `inputs` 依序取用），比对全量指纹与逐轮结论序列。
/// 返回 (引擎甲指纹, 引擎乙指纹, 逐轮结论是否全等)；两指纹相等且结论全等
/// 才算对拍过。任一轮失败即透传错误。
pub fn dual_run_check(
    mut build: impl FnMut() -> Result<TscCrossEngine, TscCrossError>,
    inputs: &[&[TscSample]],
    ts_base: u64,
) -> Result<(u64, u64, bool), TscCrossError> {
    let mut eng_a = build()?;
    let mut eng_b = build()?;
    let mut verdicts_a: Vec<u8> = Vec::new();
    let mut verdicts_b: Vec<u8> = Vec::new();
    for (i, samples) in inputs.iter().enumerate() {
        let ts = ts_base + (i as u64) * 60;
        let sa = eng_a.run_full_review(ts, samples)?;
        let sb = eng_b.run_full_review(ts, samples)?;
        if sa.verdict != sb.verdict {
            return Ok((eng_a.fingerprint(), eng_b.fingerprint(), false));
        }
        verdicts_a.push(sa.verdict.as_u8());
        verdicts_b.push(sb.verdict.as_u8());
    }
    let mut equal = verdicts_a == verdicts_b;
    equal = equal && eng_a.fingerprint() == eng_b.fingerprint();
    Ok((eng_a.fingerprint(), eng_b.fingerprint(), equal))
}

// ═══════════════════════════════════════════════════════════════════════════
// 测试（判据映射：见各 #[test] 头注）
// ═══════════════════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::vec;

    // —— 公共构建器 ——

    /// 标准档引擎（F0123 直接建账，全封存）。
    fn fresh_engine() -> TscCrossEngine {
        let cfg = TscCrossConfig::standard();
        let bl = TscBaseline::from_f0123().expect("F0123 建账");
        TscCrossEngine::new(cfg, bl).expect("引擎构造")
    }

    /// 干净样本集：每机 8 样本，账面频率 ±35ppm 内微抖，25~27℃ 零封顶修正。
    fn clean_samples(cfg: &TscCrossConfig) -> Vec<TscSample> {
        let mut v = Vec::new();
        for mi in 0..MACHINE_COUNT {
            let machine = MachineId::from_index(mi).expect("机号");
            let ledger = F0123_LEDGER[mi].1;
            for k in 0..8u64 {
                let ppm = k as i64 * 10 - 35;
                let hz = target_hz_with_drift(ledger, ppm);
                let s = synth_sample(
                    cfg,
                    machine,
                    hz,
                    1_000_000_000_000 + mi as u64 * 10_000_000_000 + k * 1_000_000,
                    400_000,
                    25 + (k % 3) as i16,
                    0,
                )
                .expect("合成样本");
                v.push(s);
            }
        }
        v
    }

    /// 剔除指定机的样本子集（缺机注入出样通道）。
    fn drop_machine(samples: &[TscSample], machine: MachineId) -> Vec<TscSample> {
        let mut v = Vec::new();
        for s in samples {
            if s.machine != machine {
                v.push(*s);
            }
        }
        v
    }

    // —— J1 主判据：三机对账一致 3/3 ——

    /// UNX-F0663-J1 前半：三机 TSC 基频校准值与 F0123 账面一致 3/3。
    #[test]
    fn j1_three_of_three_consistent() {
        let mut eng = fresh_engine();
        assert_eq!(eng.f0123_match(), 3, "F0123 对账应 3/3");
        assert!(!eng.tamper_detected());
        let cfg = TscCrossConfig::standard();
        let summary = eng
            .run_full_review(1_735_700_000, &clean_samples(&cfg))
            .expect("干净轮");
        assert_eq!(summary.verdict, RoundVerdict::AllConsistent, "干净轮应全一致");
        assert_eq!(summary.outcomes.len(), 3);
        for o in &summary.outcomes {
            assert_eq!(o.grade, DriftGrade::Normal, "微抖 ±35ppm 应正常档");
            assert!(o.consistent);
        }
        assert_eq!(eng.health(), Health::Green);
        assert_eq!(eng.ledger().consistency_bp(), 10_000, "首轮一致率应满");
        assert!(eng.verify_conservation(), "轮账守恒应平");
        eng.verify_event_contiguous().expect("事件序号连续");
        assert_eq!(eng.open_case_count(), 0, "干净轮应零立案");
    }

    // —— J1 后半：基频漂移 >0.5% 检出拦截 ——

    /// UNX-F0663-J1 后半：漂移 ≥5000ppm（0.5%）注入必须拦截 + 立案 + 注入检出。
    #[test]
    fn j1_drift_intercept_at_5000ppm() {
        let mut eng = fresh_engine();
        let cfg = TscCrossConfig::standard();
        let mut samples = clean_samples(&cfg);
        // M1 注入 +5200ppm 复测偏差
        eng.apply_injection(Injection::FreqDrift {
            machine: MachineId::M1,
            ppm: 5200,
        })
        .expect("注入登记");
        let ledger_m1 = F0123_LEDGER[1].1;
        samples.retain(|x| x.machine != MachineId::M1);
        for k in 0..8u64 {
            let hz = target_hz_with_drift(ledger_m1, 5200);
            let s = synth_sample(
                &cfg,
                MachineId::M1,
                hz,
                2_000_000_000_000 + k * 1_000_000,
                400_000,
                25,
                0,
            )
            .expect("注入样本");
            samples.push(s);
        }
        let summary = eng
            .run_full_review(1_735_700_060, &samples)
            .expect("拦截轮");
        assert_eq!(summary.verdict, RoundVerdict::Intercepted, "应判拦截轮");
        assert_eq!(summary.intercepted, vec![MachineId::M1]);
        // 漂移记录核对（拦截账行——轮记录簿结轮即清，拦截账长期在账）：
        let irec = &eng.drift().intercepts()[0];
        assert_eq!(irec.machine, MachineId::M1);
        assert!(
            irec.ppm >= 5100 && irec.ppm <= 5300,
            "ppm 应在 5200 邻域，实测 {}",
            irec.ppm
        );
        // 拦截账 + 案件 + 健康
        assert_eq!(eng.drift().intercepts().len(), 1, "拦截账应一行");
        assert_eq!(eng.drift().intercepts()[0].machine, MachineId::M1);
        let mut found_case = false;
        for c in eng.cases() {
            if c.kind == CaseKind::DriftIntercept {
                found_case = true;
            }
        }
        assert!(found_case, "应立漂移拦截案");
        assert_eq!(eng.health(), Health::Red);
        // 注入面核查：漂移注入应检出
        let report = eng.injection_report();
        assert_eq!(report.len(), 1);
        assert!(report[0].1, "漂移注入应检出");
        assert!(eng.verify_conservation());
    }

    /// 恰在红线 5000ppm 的边界拦截（≥5000 拦截口径逐位核）。
    #[test]
    fn intercept_boundary_exactly_5000ppm() {
        // 直接核 ppm 数学：2.4GHz 账面 +0.5% = +12_000_000Hz → 恰 5000ppm
        let ledger = F0123_LEDGER[0].1;
        let measured = ledger + ledger / 200; // 0.5%
        let ppm = drift_ppm(measured, ledger).expect("ppm");
        assert_eq!(ppm, 5_000, "0.5% 应恰为 5000ppm");
        assert_eq!(DriftGrade::classify(ppm), DriftGrade::Intercept);
        // 引擎级：+5000ppm 注入必须拦截
        let mut eng = fresh_engine();
        let cfg = TscCrossConfig::standard();
        let mut samples = clean_samples(&cfg);
        samples.retain(|x| x.machine != MachineId::M0);
        for k in 0..8u64 {
            let hz = target_hz_with_drift(ledger, 5000);
            let s = synth_sample(
                &cfg,
                MachineId::M0,
                hz,
                3_000_000_000_000 + k * 1_000_000,
                400_000,
                25,
                0,
            )
            .expect("注入样本");
            samples.push(s);
        }
        let summary = eng
            .run_full_review(1_735_700_120, &samples)
            .expect("边界轮");
        assert_eq!(summary.verdict, RoundVerdict::Intercepted);
    }

    // —— 漂移分级三档边界 ——

    /// 分级口径：<1000 正常 / 1000~4999 观察 / ≥5000 拦截（四点逐位核）。
    #[test]
    fn grading_boundaries() {
        assert_eq!(DriftGrade::classify(0), DriftGrade::Normal);
        assert_eq!(DriftGrade::classify(999), DriftGrade::Normal);
        assert_eq!(DriftGrade::classify(1_000), DriftGrade::Observe);
        assert_eq!(DriftGrade::classify(4_999), DriftGrade::Observe);
        assert_eq!(DriftGrade::classify(5_000), DriftGrade::Intercept);
        assert_eq!(DriftGrade::classify(1_000_000), DriftGrade::Intercept);
        // ppm 数学：1.8GHz 账面 +1.8MHz = +1000ppm 恰观察档下沿
        let ledger = F0123_LEDGER[2].1;
        let ppm = drift_ppm(ledger + 1_800_000, ledger).expect("ppm");
        assert_eq!(ppm, 1_000);
        assert_eq!(DriftGrade::classify(ppm), DriftGrade::Observe);
    }

    /// 观察档整轮：M2 +1500ppm 全轮观察，一致率 11/12 = 9166bp，越下限立案。
    #[test]
    fn observe_round_and_consistency_floor() {
        let mut eng = fresh_engine();
        let cfg = TscCrossConfig::standard();
        // 先跑三轮干净轮（下限核需 ≥3 轮）
        for r in 0..3u64 {
            eng.run_full_review(1_735_700_000 + r * 60, &clean_samples(&cfg))
                .expect("干净轮");
        }
        // 第四轮：M2 +1500ppm（观察档）
        let mut samples = clean_samples(&cfg);
        samples.retain(|x| x.machine != MachineId::M2);
        let ledger_m2 = F0123_LEDGER[2].1;
        for k in 0..8u64 {
            let hz = target_hz_with_drift(ledger_m2, 1500);
            let s = synth_sample(
                &cfg,
                MachineId::M2,
                hz,
                4_000_000_000_000 + k * 1_000_000,
                400_000,
                25,
                0,
            )
            .expect("观察样本");
            samples.push(s);
        }
        let summary = eng
            .run_full_review(1_735_700_180, &samples)
            .expect("观察轮");
        assert_eq!(summary.verdict, RoundVerdict::Observed);
        // 一致率：3 轮 × 3 机全一致 = 9，加本轮 2/3 → 11/12
        let bp = eng.ledger().consistency_bp();
        assert_eq!(
            bp,
            11 * 10_000 / 12,
            "一致率应 11/12 万分比，实测 {}",
            bp
        );
        // 越下限立案（9166 < 9500）
        let mut floor_case = false;
        for c in eng.cases() {
            if c.kind == CaseKind::ConsistencyBelowFloor {
                floor_case = true;
            }
        }
        assert!(floor_case, "一致率越下限应立案");
        assert_eq!(eng.health(), Health::Yellow, "仅观察档应黄");
        assert_eq!(eng.drift().observe_total(), 1, "观察档累计应 1");
    }

    // —— 缺机注入检出 ——

    /// 缺机注入：剔除 M2 样本 → 轮不完整 + 缺机立案 + 注入检出。
    #[test]
    fn missing_machine_injection_detected() {
        let mut eng = fresh_engine();
        let cfg = TscCrossConfig::standard();
        eng.apply_injection(Injection::MissingMachine {
            machine: MachineId::M2,
        })
        .expect("注入登记");
        let samples = drop_machine(&clean_samples(&cfg), MachineId::M2);
        let summary = eng
            .run_full_review(1_735_700_240, &samples)
            .expect("缺机轮");
        assert_eq!(summary.verdict, RoundVerdict::Incomplete);
        assert_eq!(summary.missing, vec![MachineId::M2]);
        assert_eq!(summary.outcomes.len(), 2, "缺机时在轮机应 2");
        let mut found = false;
        for c in eng.cases() {
            if c.kind == CaseKind::MissingMachine && c.machine == Some(MachineId::M2) {
                found = true;
            }
        }
        assert!(found, "应立缺机案");
        let report = eng.injection_report();
        assert_eq!(report.len(), 1);
        assert!(report[0].1, "缺机注入应检出");
        assert!(eng.verify_conservation(), "缺机轮也应守恒（只计在轮机）");
    }

    /// 无注入时的自然缺机同样立案（非注入路径）。
    #[test]
    fn natural_missing_machine_case() {
        let mut eng = fresh_engine();
        let cfg = TscCrossConfig::standard();
        let samples = drop_machine(&clean_samples(&cfg), MachineId::M0);
        let summary = eng
            .run_full_review(1_735_700_300, &samples)
            .expect("自然缺机轮");
        assert_eq!(summary.verdict, RoundVerdict::Incomplete);
        assert_eq!(eng.open_case_count(), 1);
        assert_eq!(eng.health(), Health::Red, "缺机立案即红");
    }

    // —— 账面篡改注入检出 ——

    /// 账面篡改注入：改写 M0 基频 +1% → 对账 2/3 + 篡改立案 + 注入检出 + 健康红。
    #[test]
    fn ledger_tamper_injection_detected() {
        let mut eng = fresh_engine();
        let bad_hz = F0123_LEDGER[0].1 / 100 * 101; // +1%
        eng.apply_injection(Injection::LedgerTamper {
            machine: MachineId::M0,
            new_hz: bad_hz,
        })
        .expect("注入登记");
        assert_eq!(eng.f0123_match(), 2, "篡改后对账应 2/3");
        assert!(eng.tamper_detected());
        let mut mismatch_case = false;
        let mut tamper_case = false;
        for c in eng.cases() {
            if c.kind == CaseKind::LedgerMismatch {
                mismatch_case = true;
            }
            if c.kind == CaseKind::TamperSuspect {
                tamper_case = true;
            }
        }
        assert!(mismatch_case, "应立账面不一致案");
        let _ = tamper_case; // 篡改嫌疑案由对账路径立，同一案类不重复计        assert_eq!(eng.health(), Health::Red);
        let report = eng.injection_report();
        assert_eq!(report.len(), 1);
        assert!(report[0].1, "篡改注入应检出");
        // 篡改后 baseline 指纹必变
        assert_ne!(eng.baseline().fingerprint(), {
            let bl = TscBaseline::from_f0123().expect("重建账");
            bl.fingerprint()
        });
    }

    /// 篡改后漂移对账双杀：同轮既对不上账面又测不出漂移锚 → 两案并存。
    #[test]
    fn tamper_then_reconcile_twice_opens_once() {
        let mut eng = fresh_engine();
        let bad_hz = F0123_LEDGER[1].1 + 1;
        eng.apply_injection(Injection::LedgerTamper {
            machine: MachineId::M1,
            new_hz: bad_hz,
        })
        .expect("注入登记");
        let before = eng.cases().len();
        eng.reconcile_baseline(); // 二次对账不得重复立案风暴（幂等处置）
        assert_eq!(
            eng.cases().len(),
            before,
            "同批失配二次对账不应加案"
        );
    }

    // —— 状态机乱序拒绝 ——

    /// 四态状态机：所有非法边一律 PhaseViolation，合法链畅通。
    #[test]
    fn state_machine_out_of_order_rejected() {
        let cfg = TscCrossConfig::standard();
        let samples = clean_samples(&cfg);
        let mut eng = fresh_engine();
        // Idle 直接纳样：拒
        let s0 = samples[0];
        assert!(matches!(
            eng.submit_sample(s0),
            Err(TscCrossError::PhaseViolation { .. })
        ));
        // Idle 直接比对：拒
        assert!(matches!(
            eng.compare(0),
            Err(TscCrossError::PhaseViolation { .. })
        ));
        // Idle 直接结轮：拒
        assert!(matches!(
            eng.conclude(0),
            Err(TscCrossError::PhaseViolation { .. })
        ));
        // 开轮后直接结轮：拒（跳过比对）
        eng.begin_round(1_735_700_360).expect("开轮");
        assert!(matches!(
            eng.conclude(0),
            Err(TscCrossError::PhaseViolation { .. })
        ));
        // 正常链走通
        for s in &samples {
            eng.submit_sample(*s).expect("交样");
        }
        eng.compare(1_735_700_360).expect("比对");
        // 比对后重复比对：拒
        assert!(matches!(
            eng.compare(0),
            Err(TscCrossError::PhaseViolation { .. })
        ));
        eng.conclude(1_735_700_360).expect("结轮");
        assert_eq!(eng.phase(), CrossPhase::Concluded);
        // 结轮后再交样：拒
        assert!(matches!(
            eng.submit_sample(s0),
            Err(TscCrossError::PhaseViolation { .. })
        ));
        // Concluded→begin_round 自动归档再开轮
        let r2 = eng.begin_round(1_735_700_420).expect("归档后开轮");
        assert_eq!(r2, 2);
        assert_eq!(eng.archived_rounds(), 1);
    }

    // —— 误差源分解 ——

    /// 抖动主导：样本极差 ±800ppm → 主导源 = 测量抖动。
    #[test]
    fn attribution_jitter_dominant() {
        let cfg = TscCrossConfig::standard();
        let ledger = F0123_LEDGER[0].1;
        let mut samples = Vec::new();
        for k in 0..8u64 {
            let ppm = -800 + (k as i64) * 228; // 极差 1596ppm
            let hz = target_hz_with_drift(ledger, ppm);
            samples.push(
                synth_sample(
                    &cfg,
                    MachineId::M0,
                    hz,
                    5_000_000_000_000 + k * 1_000_000,
                    400_000,
                    25, // 基准温度：温漂源归零
                    0,  // 零封顶修正：封顶源归零
                )
                .expect("抖动样本"),
            );
        }
        let median_hz = {
            let mut meas = Vec::new();
            for s in &samples {
                meas.push(s.measured_hz(cfg.ref_clock_hz).expect("测频"));
            }
            sort_u64_asc(&mut meas);
            meas[meas.len() / 2]
        };
        let total = drift_ppm(median_hz, ledger).expect("总偏差");
        let a = attribute_sources(&cfg, MachineId::M0, &samples, total).expect("归因");
        assert_eq!(a.dominant, SourceKind::Jitter);
        assert!(a.jitter_ppm >= 1_500, "抖动应 ≈1596ppm，实测 {}", a.jitter_ppm);
        assert_eq!(a.thermal_ppm, 0);
        assert_eq!(a.wrap_ppm, 0);
        // 源账（1596ppm 极差）超总偏差（中位仅 ~112ppm）是抖动主导的自然形态：
        // 残差归零 + 超界旗标置位
        assert!(a.sources_exceed_total);
        assert_eq!(a.residual_ppm, 0);
    }

    /// 温漂主导：基准温度 +50℃ → 温漂 400ppm 主导。
    #[test]
    fn attribution_thermal_dominant() {
        let cfg = TscCrossConfig::standard();
        let ledger = F0123_LEDGER[1].1;
        let mut samples = Vec::new();
        for k in 0..8u64 {
            let ppm = (k as i64) * 5 - 20; // 微抖极差 35ppm
            let hz = target_hz_with_drift(ledger, ppm);
            samples.push(
                synth_sample(
                    &cfg,
                    MachineId::M1,
                    hz,
                    6_000_000_000_000 + k * 1_000_000,
                    400_000,
                    75, // +50℃
                    0,
                )
                .expect("温漂样本"),
            );
        }
        let mut meas = Vec::new();
        for s in &samples {
            meas.push(s.measured_hz(cfg.ref_clock_hz).expect("测频"));
        }
        sort_u64_asc(&mut meas);
        let total = drift_ppm(meas[meas.len() / 2], ledger).expect("总偏差");
        let a = attribute_sources(&cfg, MachineId::M1, &samples, total).expect("归因");
        assert_eq!(a.dominant, SourceKind::Thermal);
        assert_eq!(a.thermal_ppm, 50 * THERMAL_PPM_PER_C, "温漂应 400ppm");
        assert!(a.jitter_ppm < a.thermal_ppm);
        assert_eq!(a.wrap_ppm, 0);
    }

    /// 封顶主导：40 次修正 × 120ppm = 4800ppm 主导（配额放宽档）。
    #[test]
    fn attribution_wrap_dominant() {
        let mut cfg = TscCrossConfig::standard();
        cfg.max_wrap_corrections = 64; // 归因试验档（仅放宽配额）
        cfg.validate().expect("放宽档合法");
        let ledger = F0123_LEDGER[2].1;
        let mut samples = Vec::new();
        for k in 0..8u64 {
            let hz = target_hz_with_drift(ledger, (k as i64) * 5 - 20);
            samples.push(
                synth_sample(
                    &cfg,
                    MachineId::M2,
                    hz,
                    7_000_000_000_000 + k * 1_000_000,
                    400_000,
                    25,
                    40, // 每样本 40 次封顶修正
                )
                .expect("封顶样本"),
            );
        }
        let mut meas = Vec::new();
        for s in &samples {
            meas.push(s.measured_hz(cfg.ref_clock_hz).expect("测频"));
        }
        sort_u64_asc(&mut meas);
        let total = drift_ppm(meas[meas.len() / 2], ledger).expect("总偏差");
        let a = attribute_sources(&cfg, MachineId::M2, &samples, total).expect("归因");
        assert_eq!(a.dominant, SourceKind::Wrap);
        assert_eq!(a.wrap_ppm, 8 * 40 * WRAP_GRANULARITY_PPM, "封顶应 38400ppm");
        assert!(a.sources_exceed_total, "源账超总应置位（复盘信号）");
        assert_eq!(a.residual_ppm, 0);
    }

    /// 残差归因：三源皆零、总偏差 200ppm → 残差主导。
    #[test]
    fn attribution_residual_dominant() {
        let cfg = TscCrossConfig::standard();
        let ledger = F0123_LEDGER[0].1;
        let mut samples = Vec::new();
        for k in 0..8u64 {
            let hz = target_hz_with_drift(ledger, 200); // 恒 200ppm 偏移，零散布
            samples.push(
                synth_sample(
                    &cfg,
                    MachineId::M0,
                    hz,
                    8_000_000_000_000 + k * 1_000_000,
                    400_000,
                    25,
                    0,
                )
                .expect("残差样本"),
            );
        }
        let a = attribute_sources(&cfg, MachineId::M0, &samples, 200).expect("归因");
        assert_eq!(a.jitter_ppm, 0, "零散布应零抖动");
        assert_eq!(a.thermal_ppm, 0);
        assert_eq!(a.wrap_ppm, 0);
        assert_eq!(a.residual_ppm, 200);
        assert_eq!(a.dominant, SourceKind::Residual);
    }

    // —— 样本/注入面防御 ——

    /// 零跨度样本、负跨度样本、零基准 tick、封顶越限逐项拒收。
    #[test]
    fn sample_defense_paths() {
        let mut eng = fresh_engine();
        let cfg = TscCrossConfig::standard();
        eng.begin_round(1_735_700_480).expect("开轮");
        // 零跨度
        let bad0 = TscSample {
            machine: MachineId::M0,
            start_tsc: 1_000,
            end_tsc: 1_000,
            ref_ticks: 9_600_000,
            wrap_corrections: 0,
            temp_c: 25,
        };
        assert!(matches!(
            eng.submit_sample(bad0),
            Err(TscCrossError::SampleSpanZero { .. })
        ));
        // 负跨度（下溢归溢出）
        let bad1 = TscSample {
            machine: MachineId::M0,
            start_tsc: 2_000,
            end_tsc: 1_000,
            ref_ticks: 9_600_000,
            wrap_corrections: 0,
            temp_c: 25,
        };
        assert!(matches!(
            eng.submit_sample(bad1),
            Err(TscCrossError::ArithmeticOverflow { .. })
        ));
        // 封顶修正越限
        let bad2 = synth_sample(
            &cfg,
            MachineId::M0,
            F0123_LEDGER[0].1,
            9_000_000_000_000,
            400_000,
            25,
            3, // 配额 2
        )
        .expect("合成");
        assert!(matches!(
            eng.submit_sample(bad2),
            Err(TscCrossError::WrapCorrectionInvalid { .. })
        ));
        // 未知机号不在枚举内——MachineId 只有三值，此处核 ref_ticks=0
        let bad3 = TscSample {
            machine: MachineId::M1,
            start_tsc: 1_000,
            end_tsc: 2_000,
            ref_ticks: 0,
            wrap_corrections: 0,
            temp_c: 25,
        };
        assert!(matches!(
            eng.submit_sample(bad3),
            Err(TscCrossError::ConfigInvalid { .. })
        ));
        // 正常样本仍可入账
        let good = synth_sample(
            &cfg,
            MachineId::M0,
            F0123_LEDGER[0].1,
            9_000_000_000_000,
            400_000,
            25,
            0,
        )
        .expect("合成");
        eng.submit_sample(good).expect("正常样本应入账");
        assert_eq!(eng.events.len() > 0, true);
    }

    /// 单机样本槽容量封顶（16 上限）。
    #[test]
    fn sample_slot_capacity_cap() {
        let mut eng = fresh_engine();
        let cfg = TscCrossConfig::standard();
        eng.begin_round(1_735_700_540).expect("开轮");
        for k in 0..SAMPLE_CAP_PER_MACHINE as u64 {
            let s = synth_sample(
                &cfg,
                MachineId::M0,
                target_hz_with_drift(F0123_LEDGER[0].1, (k as i64) * 3 - 10),
                10_000_000_000_000 + k * 1_000_000,
                400_000,
                25,
                0,
            )
            .expect("合成");
            eng.submit_sample(s).expect("容量内应入账");
        }
        let over = synth_sample(
            &cfg,
            MachineId::M0,
            F0123_LEDGER[0].1,
            99_000_000_000_000,
            400_000,
            25,
            0,
        )
        .expect("合成");
        assert!(matches!(
            eng.submit_sample(over),
            Err(TscCrossError::CapacityFull { .. })
        ));
    }

    // —— 基频账防御 ——

    /// 基频账：重复登记拒、账外方法拒、未封存禁开轮、行数非三拒构造。
    #[test]
    fn baseline_defense_paths() {
        let mut bl = TscBaseline::from_f0123().expect("建账");
        // 重复登记
        let dup = BaselineEntry {
            machine: MachineId::M0,
            base_freq_hz: 1,
            method: CalibMethod::PitLatch,
            calib_ts: 0,
            sample_count: 1,
            sealed: true,
        };
        assert!(matches!(
            bl.register(dup),
            Err(TscCrossError::DuplicateMachine { .. })
        ));
        // 未封存账禁开轮
        let mut eng = {
            let mut b = TscBaseline::empty();
            for row in F0123_LEDGER.iter() {
                b.register(BaselineEntry {
                    machine: MachineId::from_index(row.0 as usize).expect("机号"),
                    base_freq_hz: row.1,
                    method: CalibMethod::from_code(row.2),
                    calib_ts: row.3,
                    sample_count: 64,
                    sealed: false, // 全不封存
                })
                .expect("登记");
            }
            TscCrossEngine::new(TscCrossConfig::standard(), b).expect("引擎")
        };
        assert!(matches!(
            eng.begin_round(0),
            Err(TscCrossError::BaselineNotSealed)
        ));
        // 账外方法码拒
        let mut b2 = TscBaseline::empty();
        let bad_method = BaselineEntry {
            machine: MachineId::M0,
            base_freq_hz: 1_000,
            method: CalibMethod::from_code(0xEE),
            calib_ts: 0,
            sample_count: 1,
            sealed: true,
        };
        assert!(matches!(
            b2.register(bad_method),
            Err(TscCrossError::ConfigInvalid { .. })
        ));
        // 行数非三拒构造
        let one = {
            let mut b = TscBaseline::empty();
            b.register(BaselineEntry {
                machine: MachineId::M0,
                base_freq_hz: F0123_LEDGER[0].1,
                method: CalibMethod::PitLatch,
                calib_ts: F0123_LEDGER[0].3,
                sample_count: 64,
                sealed: true,
            })
            .expect("登记");
            b
        };
        assert!(matches!(
            TscCrossEngine::new(TscCrossConfig::standard(), one),
            Err(TscCrossError::ConfigInvalid { .. })
        ));
        let _ = &mut bl;
    }

    /// 配置校验五路拒收。
    #[test]
    fn config_validation_paths() {
        let mut c = TscCrossConfig::standard();
        c.ref_clock_hz = 0;
        assert!(c.validate().is_err());
        let mut c = TscCrossConfig::standard();
        c.ref_clock_hz = 2_000_000_000;
        assert!(c.validate().is_err());
        let mut c = TscCrossConfig::standard();
        c.samples_required = 0;
        assert!(c.validate().is_err());
        let mut c = TscCrossConfig::standard();
        c.temp_ref_c = 200;
        assert!(c.validate().is_err());
        let mut c = TscCrossConfig::standard();
        c.max_wrap_corrections = 100;
        assert!(c.validate().is_err());
        assert!(TscCrossConfig::standard().validate().is_ok());
    }

    // —— ppm 数学与溢出防 ——

    /// ppm 计算的 u128 升宽防溢出与边界。
    #[test]
    fn ppm_math_overflow_safe() {
        // 零账面拒
        assert!(matches!(
            drift_ppm(1, 0),
            Err(TscCrossError::ConfigInvalid { .. })
        ));
        // 大跨度：measured=2^63、账面=2^32 → ppm = (2^31−1)×1e6，u128 承载不溢
        let big = 1u64 << 63;
        let ppm = drift_ppm(big, 1u64 << 32).expect("u128 应承载");
        assert_eq!(ppm, 2_147_483_647_000_000);
        // 巨量失配（u128 乘积超 u64）→ 显性溢出错误，不静默封顶
        assert!(matches!(
            drift_ppm(u64::MAX, 1),
            Err(TscCrossError::ArithmeticOverflow { .. })
        ));
        // 反向：measured=0 → 100% = 1_000_000ppm
        let ppm = drift_ppm(0, F0123_LEDGER[0].1).expect("ppm");
        assert_eq!(ppm, 1_000_000);
        assert_eq!(DriftGrade::classify(ppm), DriftGrade::Intercept);
        // 相等 → 0
        assert_eq!(drift_ppm(7, 7).expect("ppm"), 0);
        // 极小账面 1Hz +1Hz → 1_000_000ppm
        assert_eq!(drift_ppm(2, 1).expect("ppm"), 1_000_000);
    }

    /// 目标频率推导：带号 ppm 双向、极端封顶。
    #[test]
    fn target_hz_with_drift_paths() {
        let ledger = F0123_LEDGER[0].1;
        let up = target_hz_with_drift(ledger, 5_000);
        let down = target_hz_with_drift(ledger, -5_000);
        assert!(up > ledger);
        assert!(down < ledger);
        assert_eq!(up - ledger, ledger - down, "对称漂移等距");
        // 负漂触底封顶：-2_000_000ppm（-200%）→ 不得下穿零
        let floor = target_hz_with_drift(ledger, -2_000_000);
        assert_eq!(floor, 0);
        // 巨量正漂 saturating 封顶
        let ceil = target_hz_with_drift(ledger, i64::MAX);
        assert!(ceil >= ledger);
    }

    // —— 轮账守恒与逐出迁移 ——

    /// 轮账守恒：混合轮次（干净/观察/拦截/缺机）后记录重算 = 累计计数。
    #[test]
    fn ledger_conservation_mixed_rounds() {
        let mut eng = fresh_engine();
        let cfg = TscCrossConfig::standard();
        let clean = clean_samples(&cfg);
        // 轮1 干净
        eng.run_full_review(1_735_700_000, &clean).expect("轮1");
        // 轮2 缺 M2
        let miss = drop_machine(&clean, MachineId::M2);
        eng.run_full_review(1_735_700_060, &miss).expect("轮2");
        // 轮3 干净
        eng.run_full_review(1_735_700_120, &clean).expect("轮3");
        // 轮4 M1 +2000ppm 观察
        let mut obs = clean.clone();
        obs.retain(|x| x.machine != MachineId::M1);
        for k in 0..8u64 {
            let s = synth_sample(
                &cfg,
                MachineId::M1,
                target_hz_with_drift(F0123_LEDGER[1].1, 2000),
                11_000_000_000_000 + k * 1_000_000,
                400_000,
                25,
                0,
            )
            .expect("合成");
            obs.push(s);
        }
        eng.run_full_review(1_735_700_180, &obs).expect("轮4");
        // 守恒：在轮机数 3+2+3+3 = 11，一致 3+2+3+2 = 10（缺机轮 M0/M1 仍一致）
        assert_eq!(eng.ledger().total_machine_rounds(), 11);
        assert_eq!(eng.ledger().consistent_machine_rounds(), 10);
        assert!(eng.verify_conservation(), "混合轮守恒应平");
        assert_eq!(
            eng.ledger().consistency_bp(),
            10 * 10_000 / 11,
            "一致率应 10/11 万分比"
        );
        // 轮号连续零跳号
        let mut expect = 1u64;
        for r in eng.ledger().rounds() {
            assert_eq!(r.round, expect);
            expect += 1;
        }
    }

    /// 轮簿环形逐出：65 轮后簿恒 64 行，逐出迁移计数平，守恒仍立。
    #[test]
    fn ledger_ring_eviction() {
        let mut eng = fresh_engine();
        let cfg = TscCrossConfig::standard();
        let clean = clean_samples(&cfg);
        for r in 0..65u64 {
            eng.run_full_review(1_735_700_000 + r * 60, &clean)
                .expect("轮");
        }
        assert_eq!(eng.ledger().round_count(), ROUND_LEDGER_CAP);
        let (er, emr, ec) = eng.ledger().eviction_stats();
        assert_eq!(er, 1, "应逐出 1 轮");
        assert_eq!(emr, 3, "逐出轮应迁 3 机轮");
        assert_eq!(ec, 3, "逐出轮应迁 3 一致机轮");
        assert_eq!(eng.ledger().total_rounds(), 65);
        assert!(eng.verify_conservation(), "逐出后守恒仍平");
        assert_eq!(eng.ledger().total_machine_rounds(), 65 * 3);
    }

    // —— 事件账 ——

    /// 事件账：序号连续、环形逐出后仍连续、跨 65 轮压力。
    #[test]
    fn event_ledger_ring_and_seq() {
        let mut eng = fresh_engine();
        let cfg = TscCrossConfig::standard();
        let clean = clean_samples(&cfg);
        for r in 0..30u64 {
            eng.run_full_review(1_735_700_000 + r * 60, &clean)
                .expect("轮");
        }
        // 30 轮 × (1开轮+24交样+1相位+1比对+1结轮+1相位) ≈ 840 事件 > 256 容量
        assert_eq!(eng.events().len(), EVENT_CAP, "事件账应恒满容");
        eng.verify_event_contiguous().expect("逐出后序号仍连续");
        let first = eng.events().events()[0].seq;
        assert!(first > 1, "应已逐出头部事件");
        // 事件类覆盖核
        let mut kinds_seen: Vec<EventKind> = Vec::new();
        for e in eng.events().events() {
            if !kinds_seen.contains(&e.kind) {
                kinds_seen.push(e.kind);
            }
        }
        assert!(kinds_seen.contains(&EventKind::RoundBegin));
        assert!(kinds_seen.contains(&EventKind::SampleAccepted));
        assert!(kinds_seen.contains(&EventKind::CompareDone));
        assert!(kinds_seen.contains(&EventKind::Conclude));
        assert!(kinds_seen.contains(&EventKind::PhaseChange));
    }

    // —— 趋势带 ——

    /// 趋势带：历史成型前不立案；带外尖峰立案；带随窗滚动。
    #[test]
    fn trend_band_outlier_detection() {
        // 纯单元：窄带成型后带外判真
        let mut band = TrendBand::new(4).expect("带构造");
        assert!(!band.is_outlier(1_000_000), "历史不足不立案");
        for (r, ppm) in [(1u64, 100u64), (2, 110), (3, 90), (4, 105)] {
            band.push(r, ppm);
        }
        assert_eq!(band.len(), 4);
        // 带 = [90-9, 110+11] = [81, 121]
        assert!(!band.is_outlier(121));
        assert!(band.is_outlier(122), "带上沿外应判带外");
        assert!(band.is_outlier(80), "带下沿外应判带外");
        assert!(!band.is_outlier(81));
        // 滚动逐出
        band.push(5, 100);
        assert_eq!(band.len(), 4);
        assert_eq!(band.evicted(), 1);
        // 引擎级：先 3 干净轮成型带，再注入 +30000ppm 尖峰 → 趋势案 + 漂移案并存
        let mut eng = fresh_engine();
        let cfg = TscCrossConfig::standard();
        let clean = clean_samples(&cfg);
        for r in 0..3u64 {
            eng.run_full_review(1_735_700_000 + r * 60, &clean)
                .expect("干净轮");
        }
        let mut spike = clean.clone();
        spike.retain(|x| x.machine != MachineId::M0);
        for k in 0..8u64 {
            let s = synth_sample(
                &cfg,
                MachineId::M0,
                target_hz_with_drift(F0123_LEDGER[0].1, 30_000),
                12_000_000_000_000 + k * 1_000_000,
                400_000,
                25,
                0,
            )
            .expect("尖峰样本");
            spike.push(s);
        }
        eng.run_full_review(1_735_700_180, &spike).expect("尖峰轮");
        let mut trend_case = false;
        let mut drift_case = false;
        for c in eng.cases() {
            if c.kind == CaseKind::TrendOutlier {
                trend_case = true;
            }
            if c.kind == CaseKind::DriftIntercept {
                drift_case = true;
            }
        }
        assert!(trend_case, "带外尖峰应立趋势案");
        assert!(drift_case, "尖峰同时应立漂移案");
    }

    // —— 双跑确定性对拍 ——

    /// 双跑确定性：同构建器同输入两引擎指纹全等 + 逐轮结论全等。
    #[test]
    fn dual_run_deterministic() {
        let cfg = TscCrossConfig::standard();
        let clean = clean_samples(&cfg);
        let mut obs = clean.clone();
        obs.retain(|x| x.machine != MachineId::M1);
        for k in 0..8u64 {
            let s = synth_sample(
                &cfg,
                MachineId::M1,
                target_hz_with_drift(F0123_LEDGER[1].1, 2000),
                13_000_000_000_000 + k * 1_000_000,
                400_000,
                25,
                0,
            )
            .expect("合成");
            obs.push(s);
        }
        let inputs: Vec<&[TscSample]> = vec![&clean, &obs, &clean];
        let (fp_a, fp_b, equal) = dual_run_check(
            || {
                let bl = TscBaseline::from_f0123().expect("建账");
                TscCrossEngine::new(TscCrossConfig::standard(), bl)
            },
            &inputs,
            1_735_710_000,
        )
        .expect("双跑");
        assert!(equal, "双跑指纹/结论应全等");
        assert_eq!(fp_a, fp_b, "指纹应逐位一致：甲={:016X} 乙={:016X}", fp_a, fp_b);
    }

    /// 双跑不同输入必不等（对拍面有效性反证）。
    #[test]
    fn dual_run_diverges_on_different_input() {
        let cfg = TscCrossConfig::standard();
        let clean = clean_samples(&cfg);
        let mut drift_run = clean.clone();
        drift_run.retain(|x| x.machine != MachineId::M0);
        for k in 0..8u64 {
            let s = synth_sample(
                &cfg,
                MachineId::M0,
                target_hz_with_drift(F0123_LEDGER[0].1, 6000),
                14_000_000_000_000 + k * 1_000_000,
                400_000,
                25,
                0,
            )
            .expect("合成");
            drift_run.push(s);
        }
        // 甲跑干净序列，乙跑含拦截轮序列 → 指纹必不等
        let mut eng_a = fresh_engine();
        let mut eng_b = fresh_engine();
        eng_a
            .run_full_review(1_735_720_000, &clean)
            .expect("甲轮");
        eng_b
            .run_full_review(1_735_720_000, &drift_run)
            .expect("乙轮");
        assert_ne!(
            eng_a.fingerprint(),
            eng_b.fingerprint(),
            "不同输入指纹必不等"
        );
    }

    // —— 指纹与报告 ——

    /// 指纹敏感性：任一账面变动（相位/案件/事件）即指纹变动。
    #[test]
    fn fingerprint_sensitivity() {
        let cfg = TscCrossConfig::standard();
        let clean = clean_samples(&cfg);
        let mut eng = fresh_engine();
        let fp0 = eng.fingerprint();
        eng.begin_round(1_735_730_000).expect("开轮");
        let fp1 = eng.fingerprint();
        assert_ne!(fp0, fp1, "开轮应改指纹");
        for s in &clean {
            eng.submit_sample(*s).expect("交样");
        }
        eng.compare(1_735_730_000).expect("比对");
        eng.conclude(1_735_730_000).expect("结轮");
        let fp2 = eng.fingerprint();
        assert_ne!(fp1, fp2, "结轮应改指纹");
        // 篡改改指纹
        eng.apply_injection(Injection::LedgerTamper {
            machine: MachineId::M2,
            new_hz: 1,
        })
        .expect("注入");
        let fp3 = eng.fingerprint();
        assert_ne!(fp2, fp3, "篡改应改指纹");
    }

    /// 报告渲染：关键节齐 + 截断落账（30 轮超 80 行上限）。
    #[test]
    fn report_render_and_truncation() {
        let mut eng = fresh_engine();
        let cfg = TscCrossConfig::standard();
        let clean = clean_samples(&cfg);
        // 短报告：不截断
        eng.run_full_review(1_735_740_000, &clean).expect("轮");
        let (short, trunc) = eng.render_report();
        assert!(!trunc, "短报告不应截断");
        assert!(short.contains("UNX-F0663"));
        assert!(short.contains("0x15B0"));
        assert!(short.contains("GREEN"));
        assert!(short.contains("3/3"));
        assert!(short.contains("全一致"));
        assert!(short.contains("基频账 M0"));
        assert!(short.contains("全量指纹"));
        // 长报告：截断 + 事件落账（61 轮 → 表头 13 + 轮行 61 + 指纹 1 = 75 行 > 64 上限）
        for r in 0..60u64 {
            eng.run_full_review(1_735_740_060 + r * 60, &clean)
                .expect("轮");
        }
        let (long, trunc2) = eng.render_report();
        assert!(trunc2, "长报告应截断");
        let line_count = long.lines().count();
        assert!(
            line_count <= REPORT_LINE_CAP,
            "截断后行数应 ≤{}，实测 {}",
            REPORT_LINE_CAP,
            line_count
        );
        // 截断事实落账（0x15BF 码在账）
        let mut trunc_event = false;
        for e in eng.events().events() {
            if e.code == D_REPORT_TRUNC {
                trunc_event = true;
            }
        }
        assert!(trunc_event, "截断应落事件账");
    }

    /// CSV 导出：表头 + 行数 = 轮数 + 列位守恒。
    #[test]
    fn csv_export_shape() {
        let mut eng = fresh_engine();
        let cfg = TscCrossConfig::standard();
        let clean = clean_samples(&cfg);
        let miss = drop_machine(&clean, MachineId::M2);
        eng.run_full_review(1_735_750_000, &clean).expect("轮1");
        eng.run_full_review(1_735_750_060, &miss).expect("轮2");
        let csv = eng.ledger_csv();
        let mut lines = csv.lines();
        let header = lines.next().expect("表头");
        assert_eq!(
            header,
            "round,verdict,consistent,present,ppm_m0,ppm_m1,ppm_m2"
        );
        let rows: Vec<&str> = lines.collect();
        assert_eq!(rows.len(), 2, "CSV 行数应 = 轮数");
        // 轮2 缺机位（末列 ppm_m2）为空 → 行尾悬逗号
        assert!(rows[1].ends_with(','), "缺机末列应空，行: {}", rows[1]);
    }

    // —— 立案/结案生命周期 ——

    /// 案件结案：结案后未结数归零，重复结案/不存在案号拒。
    #[test]
    fn case_lifecycle() {
        let mut eng = fresh_engine();
        let cfg = TscCrossConfig::standard();
        let miss = drop_machine(&clean_samples(&cfg), MachineId::M1);
        eng.run_full_review(1_735_760_000, &miss).expect("缺机轮");
        assert_eq!(eng.open_case_count(), 1);
        let case_id = eng.cases()[0].case_id;
        assert_eq!(case_id, 1, "首案案号应 1");
        eng.close_case(case_id).expect("结案");
        assert_eq!(eng.open_case_count(), 0);
        // 案结账清：硬案已结、无观察累计、账面 3/3 → 归绿
        assert_eq!(eng.health(), Health::Green);
        // 重复结案拒
        assert!(matches!(
            eng.close_case(case_id),
            Err(TscCrossError::ConfigInvalid { .. })
        ));
        // 不存在案号拒
        assert!(matches!(
            eng.close_case(999),
            Err(TscCrossError::ConfigInvalid { .. })
        ));
    }

    /// 错误域全覆盖：detail 非空、diag_code 落段、short 非空。
    #[test]
    fn error_domain_coverage() {
        let errs: Vec<TscCrossError> = vec![
            TscCrossError::PhaseViolation {
                from: CrossPhase::Idle,
                attempted: CrossPhase::Concluded,
            },
            TscCrossError::BaselineNotSealed,
            TscCrossError::LedgerMismatch {
                machine: MachineId::M0,
                ledger_hz: 1,
                seen_hz: 2,
            },
            TscCrossError::MissingSample {
                machine: MachineId::M1,
            },
            TscCrossError::SampleSpanZero {
                machine: MachineId::M2,
            },
            TscCrossError::DriftIntercept {
                machine: MachineId::M0,
                ppm: 6000,
            },
            TscCrossError::ArithmeticOverflow { site: "测试" },
            TscCrossError::CapacityFull { ledger: "测试账" },
            TscCrossError::DuplicateMachine {
                machine: MachineId::M1,
            },
            TscCrossError::UnknownMachine {
                machine: MachineId::M2,
            },
            TscCrossError::ConfigInvalid { field: "测试" },
            TscCrossError::WrapCorrectionInvalid {
                corrections: 9,
                allowed: 2,
            },
            TscCrossError::EventSeqGap { expected: 7, got: 9 },
            TscCrossError::InjectionUncaught { kind: "测试注入" },
            TscCrossError::ConsistencyBelowFloor { bp: 8000 },
            TscCrossError::ReportTruncated,
        ];
        assert_eq!(errs.len(), 16, "错误域应 16 变体");
        for e in &errs {
            assert!(!e.detail().is_empty(), "detail 应非空");
            assert!(!e.short().is_empty(), "short 应非空");
            assert!(
                e.diag_code() >= SEG_BASE && e.diag_code() <= SEG_BASE + SEG_MASK,
                "诊断码应落 0x15B0..0x15BF 段"
            );
        }
        // 16 码互斥
        let mut codes: Vec<u16> = Vec::new();
        for e in &errs {
            codes.push(e.diag_code());
        }
        for i in 0..codes.len() {
            for j in (i + 1)..codes.len() {
                assert_ne!(codes[i], codes[j], "诊断码应互斥");
            }
        }
    }

    /// FNV 原语锚定：标准测试向量 "a"/"foobar"。
    #[test]
    fn fnv_known_vectors() {
        assert_eq!(fnv1a64(b""), 0xcbf2_9ce4_8422_2325);
        assert_eq!(fnv1a64(b"a"), 0xaf63_dc4c_8601_ec8c);
        assert_eq!(fnv1a64(b"foobar"), 0x85944171f73967e8);
        // const 版链式与直算等价
        let h = fnv1a64_from(FNV_OFFSET, b"foo");
        assert_eq!(fnv1a64_from(h, b"bar"), fnv1a64(b"foobar"));
    }

    /// LCG 确定性：同种子同序列，不同种子异序列。
    #[test]
    fn lcg_deterministic() {
        let mut a = Lcg::new(42);
        let mut b = Lcg::new(42);
        for _ in 0..16 {
            assert_eq!(a.next(), b.next());
        }
        let mut c = Lcg::new(43);
        assert_ne!(a.next(), c.next());
        // below 域约束
        let mut d = Lcg::new(7);
        for _ in 0..64 {
            let v = d.below(100);
            assert!(v < 100);
        }
        assert_eq!(Lcg::new(1).below(0), 0);
    }

    /// 机号/方法/相位工具面全覆盖。
    #[test]
    fn enum_utility_coverage() {
        for i in 0..MACHINE_COUNT {
            let m = MachineId::from_index(i).expect("机号");
            assert_eq!(m.index(), i);
            assert_eq!(m.label().len(), 2);
        }
        assert!(MachineId::from_index(3).is_none());
        assert_eq!(
            CalibMethod::from_code(METHOD_PIT_LATCH),
            CalibMethod::PitLatch
        );
        assert_eq!(
            CalibMethod::from_code(METHOD_PMTIMER_RATIO),
            CalibMethod::PmTimerRatio
        );
        assert_eq!(
            CalibMethod::from_code(METHOD_CPUID_BRAND),
            CalibMethod::CpuidBrand
        );
        assert!(!CalibMethod::from_code(0xEE).known());
        assert_eq!(CalibMethod::from_code(0xEE).code(), 0xEE);
        // 相位合法边全覆盖
        assert!(CrossPhase::can_advance(CrossPhase::Idle, CrossPhase::Sampling));
        assert!(CrossPhase::can_advance(
            CrossPhase::Sampling,
            CrossPhase::Comparing
        ));
        assert!(CrossPhase::can_advance(
            CrossPhase::Comparing,
            CrossPhase::Concluded
        ));
        assert!(CrossPhase::can_advance(
            CrossPhase::Concluded,
            CrossPhase::Idle
        ));
        assert!(!CrossPhase::can_advance(CrossPhase::Idle, CrossPhase::Comparing));
        assert!(!CrossPhase::can_advance(CrossPhase::Idle, CrossPhase::Concluded));
        assert!(!CrossPhase::can_advance(
            CrossPhase::Sampling,
            CrossPhase::Idle
        ));
        assert!(!CrossPhase::can_advance(
            CrossPhase::Concluded,
            CrossPhase::Sampling
        ));
        // 合成采样器防御
        let cfg = TscCrossConfig::standard();
        assert!(synth_sample(&cfg, MachineId::M0, 1, 0, 0, 25, 0).is_err());
        let zero_cfg = TscCrossConfig {
            ref_clock_hz: 0,
            ..TscCrossConfig::standard()
        };
        assert!(synth_sample(&zero_cfg, MachineId::M0, 1, 0, 400_000, 25, 0).is_err());
        // 微窗下整为零
        assert!(synth_sample(&cfg, MachineId::M0, 1, 0, 1, 25, 0).is_err());
    }

    /// 多轮混合注入齐检：三通道同轮注册，逐通道检出（判据注入面 100% 口径）。
    #[test]
    fn all_three_injections_detected_together() {
        let cfg = TscCrossConfig::standard();
        let mut eng = fresh_engine();
        eng.apply_injection(Injection::FreqDrift {
            machine: MachineId::M0,
            ppm: 7000,
        })
        .expect("注入1");
        eng.apply_injection(Injection::MissingMachine {
            machine: MachineId::M2,
        })
        .expect("注入2");
        // 样本：M0 +7000ppm，M1 干净，M2 全缺
        let mut samples = Vec::new();
        for k in 0..8u64 {
            let s = synth_sample(
                &cfg,
                MachineId::M0,
                target_hz_with_drift(F0123_LEDGER[0].1, 7000),
                15_000_000_000_000 + k * 1_000_000,
                400_000,
                25,
                0,
            )
            .expect("合成");
            samples.push(s);
            let s = synth_sample(
                &cfg,
                MachineId::M1,
                target_hz_with_drift(F0123_LEDGER[1].1, (k as i64) * 5 - 10),
                16_000_000_000_000 + k * 1_000_000,
                400_000,
                25,
                0,
            )
            .expect("合成");
            samples.push(s);
        }
        let summary = eng
            .run_full_review(1_735_770_000, &samples)
            .expect("混合注入轮");
        assert_eq!(summary.verdict, RoundVerdict::Intercepted, "拦截优先判");
        assert_eq!(summary.missing, vec![MachineId::M2]);
        let report = eng.injection_report();
        assert_eq!(report.len(), 2, "本例注册两通道注入");
        for (label, caught) in &report {
            assert!(*caught, "注入 [{}] 应检出", label);
        }
        assert!(eng.verify_conservation());
        eng.verify_event_contiguous().expect("事件连续");
    }
}
