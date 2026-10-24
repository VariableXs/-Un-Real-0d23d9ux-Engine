//! UNX-F1003 · TSC 频率校准（MSR/HPET/参照三源交叉校准）· 目标 340 行 · 已深化
//!
//! **锚点**：`docs/Varix/CoRun Varix STAR II · Unxreal/CoRun Varix STAR II · Unxreal.md`
//! 主册 UNX-A2-B11 批（TSC 不变量与校验，F1001–F1020）表格行（约 2698 行）：
//!
//! > | UNX-F1003 | TSC 频率校准（MSR/HPET/参照三源交叉校准） | 340 | 已深化 |
//! > | UNX-F1003-J1 校准精度（三源校准值互差 <0.1%，三源全中率 100%），
//! >   校准账可回放，与现存 `TscCalibration` 行为等价率 100% |
//!
//! 判据三成分：真机可观测（三源注入面逐轮可采样）+ 数值阈值（互差 <1000ppm）
//! + Windows 对照（QEMU/真机校准口径与 QueryPerformanceFrequency 同源可比）。
//!
//! ## 要点一：三源闭集 trait 注入建模——零真实 MSR/端口访问
//!
//! 本模块绝不执行真实 RDMSR/IO 端口指令：三源全部经 `CalibSource` trait
//! 注入。源闭集四个实现、三条在役腿：
//! ① MSR 腿（`MsrSource` 直接 MSR 0x10 差分采样，附 MSR 0xCE 证据 /
//! `PropagateSource` 以 MSR_PLATFORM_INFO [15:8] 最大非睿频比率 × BCLK
//! 比率推算——两模式互斥占一条腿）；② HPET 腿（`HpetSource`，主计数器
//! 周期 fs 口径）；③ 参照腿（`RefSource`，PIT 链 1.193182 MHz 抽象参照
//! 计数器）。每源采样模型为抽象 tick 读数（纯函数 of 逻辑 ns），确定性
//! 可重放；判定器只对注入数据负责，注入什么判什么。
//!
//! ## 要点二：三源交叉校准与中位数裁决（离群源显性降权不弃）
//!
//! 每轮三源各自在采样窗内估算 TSC 频率（128 位整数中间量，全整数 ppm
//! 运算，零浮点）；两两千分比偏差全对（3 对）越 `CC_TOLERANCE_PPM`
//! = 1000ppm（即 0.1%）即 Diverge 判决。裁决走三取中（median-of-3）：
//! 与中位数偏差越限的源被**显性降权**（权重 100→25，留账不弃）；两源
//! 同越中位数（发散不可解）判 MedianUnresolved。最终频率 = 各轮中位数
//! 的中位数——单源离群永不污染结论。
//!
//! ## 要点三：采样窗口纪律
//!
//! 窗口三门槛：窗长预算（`CC_MIN_WINDOW_NS` ≤ 窗长 ≤ `CC_WINDOW_NS`
//! 预算上限）、样本数下限（`CC_SAMPLES_MIN` = 16）、窗内读数单调性
//! （逐源逐样本非降序校验，破序即 MonoBroken 判决留账）。窗口过短时
//! 参照腿量化误差（≈1/11931 ≈ 84ppm@10ms）会逼近容差——窗长下限由
//! 此反推锁定，注释来源随常量落账。
//!
//! ## 要点四：校准账可回放
//!
//! 每轮全记录：窗口三元组、逐源（样本数/估算值/耗时口径/单调/权重）、
//! 两两 ppm 三元组、中位数、离群闭集、判决。回放以同构源工厂重演逐轮，
//! 字段级对拍，一致率必须 100%（ReplayMismatch 立案）。
//!
//! ## 要点五：行为等价自证（与 `TscCalibration` 100% 对拍）
//!
//! 既有 `cpu/clock.rs` 的 `TscCalibration::choose`（leaf15 → leaf16 →
//! measured → fallback 优先序，FALLBACK_HZ = 2.5GHz，crystal 缺省
//! 25MHz）因自包含纪律（禁 `use crate::`）无法直引——本模块内置逐字
//! 语义转录 `LegacyCalib::choose` 为参照面，再以**异构实现**（候选表
//! + 秩扫描，非 if 链）重写同语义 `decide_by_table`，对 5×4×3×2=120
//! 组合全对拍；`tsc_to_ns`/`ns_to_tsc` 的 128 位 muldiv 亦双实现对扫。
//! 等价率 <100% 即 EquivalenceFail 立案。
//!
//! ## 要点六：事件账 + 健康 + 指纹 + 双跑对拍 + 注入面
//!
//! 事件账八类闭集逐条留痕（seq 单调）；`health()` 八项检查逐项出分；
//! FNV-1a 64 位指纹覆盖账本全量；双跑（同构两次完整校准）指纹必须
//! 相等（DualRunMismatch 立案）；注入面三案例闭集（离群源 20000ppm
//! 注入 / 单调破注入 / 窗口不足注入）必须 3/3 检出（InjectUndetected
//! 立案）。
//!
//! ## 诊断码段
//!
//! 独占 `0x1B10..=0x1B1F` 十六码闭集。首选段 0x1A20..=0x1A2F 经 grep
//! 确认被占（`svstar2/ves477_s26dynamic.rs` SEG_BASE=0x1A20，已用
//! 0x1A21..0x1A23；0x1A2B 另现于 signchain 常量；`unx0962_pollprof.rs`
//! 亦记载 0x1A2*/0x1A4* 被 ves477/ves478 占用），顺延 0x1B1x 段 grep
//! 零命中，先入库者占段。

use alloc::boxed::Box;
use alloc::format;
use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 〇、版本与常量（来源随常量落账）
// ---------------------------------------------------------------------------

/// 模块版本。
pub const CC_VERSION: &str = "unx1003_tsccal-v1";

/// 诊断段基址（0x1B10 段 · 本模块独占；0x1A2x 段被 ves477 占用，见头注）。
pub const CC_SEG_BASE: u16 = 0x1B10;

/// 互差容差：1000 ppm = 0.1%（判据原文"三源校准值互差 <0.1%"的整数口径）。
pub const CC_TOLERANCE_PPM: u64 = 1000;

/// 采样窗长预算上限：20ms（校准时延预算口径；超窗即 WindowBudget 立案）。
pub const CC_WINDOW_NS: u64 = 20_000_000;

/// 采样窗长下限：1ms（参照腿 1.193182MHz@1ms 量化误差上限 ≈838ppm，
/// 已逼近 1000ppm 容差——低于此窗对拍不可信，故锁下限）。
pub const CC_MIN_WINDOW_NS: u64 = 1_000_000;

/// 每源每窗样本数下限：16（单调性校验与量化均值的最小统计量）。
pub const CC_SAMPLES_MIN: u32 = 16;

/// 每源每窗样本数上限：1024（账本容量预算；超限即 WindowBudget 立案）。
pub const CC_SAMPLES_MAX: u32 = 1024;

/// 缺省每窗样本数：33（10ms 窗内约 303µs 间距，远大于 HPET 10ns 周期）。
pub const CC_SAMPLES_DEFAULT: u32 = 33;

/// 校准轮数：12（判据"三源全中率 100%"的统计基数）。
pub const CC_ROUNDS: u32 = 12;

/// 轮间间隔：11ms（轮窗 10ms + 1ms 间隔，轮窗互不重叠）。
pub const CC_ROUND_STRIDE_NS: u64 = 11_000_000;

/// 缺省轮窗长：10ms（参照腿量化误差 ≈84ppm，留 10 倍以上容差余量）。
pub const CC_ROUND_WINDOW_NS: u64 = 10_000_000;

/// 合成平台真值 TSC 频率：2.6 GHz（QEMU KVM 档典型口径，注入模型真值）。
pub const CC_TRUE_TSC_HZ: u64 = 2_600_000_000;

/// TSC 基址（合成平台 TSC 非零起算，验回绕无关性）。
pub const CC_TSC_BASE: u64 = 0x0000_1234_5678_9ABC;

/// BCLK 基准总线频率：100 MHz（Intel SDM Vol.4 §2.16，MSR_PLATFORM_INFO
/// 比率的基准时钟口径）。
pub const CC_BCLK_HZ: u64 = 100_000_000;

/// MSR_PLATFORM_INFO 地址：0xCE（Intel SDM Vol.4 §2.16）。
pub const CC_MSR_PLATFORM_INFO: u32 = 0xCE;

/// MSR_PLATFORM_INFO [15:8]：最大非睿频比率域（Intel SDM Vol.4 §2.16 表 2-31）。
pub const CC_RATIO_SHIFT: u32 = 8;
/// 比率域掩码（8 位）。
pub const CC_RATIO_MASK: u64 = 0xFF;

/// 合成平台最大非睿频比率：26（2.6 GHz ÷ 100 MHz）。
pub const CC_MAX_NON_TURBO_RATIO: u64 = 26;

/// HPET 主计数器周期：10ns = 10_000_000 fs（HPET 规范典型值；上限
/// 100_000_000 fs 见 cpu/clock.rs HPET_MAX_PERIOD_FS）。
pub const CC_HPET_PERIOD_FS: u64 = 10_000_000;

/// 参照时钟源频率：1.193182 MHz（PIT 基频整数口径，同 cpu/clock.rs
/// PIT_BASE_HZ；RefSource 为 PIT 链抽象参照计数器）。
pub const CC_REF_RATE_HZ: u64 = 1_193_182;

/// 常量守卫（编译期）：容差必须恰为 0.1% 口径。
const _: () = assert!(CC_TOLERANCE_PPM == 1000);
/// 常量守卫（编译期）：窗长下限 ≤ 缺省轮窗 ≤ 预算上限。
const _: () = assert!(CC_MIN_WINDOW_NS <= CC_ROUND_WINDOW_NS && CC_ROUND_WINDOW_NS <= CC_WINDOW_NS);
/// 常量守卫（编译期）：样本下限 ≥ 16 且缺省样本不超上限。
const _: () = assert!(CC_SAMPLES_MIN >= 16 && CC_SAMPLES_DEFAULT <= CC_SAMPLES_MAX);

// ---------------------------------------------------------------------------
// 一、诊断码闭集（独占 0x1B10..=0x1B1F 段）
// ---------------------------------------------------------------------------

/// TSC 频率校准诊断码。独占 `0x1B10..=0x1B1F` 段十六码闭集。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CcCode {
    /// 0x1B10 三源缺源/腿位错（某腿未挂载或源类型与腿位不符）。
    SourceAbsent,
    /// 0x1B11 样本数越界（低于下限 16 或高于上限 1024）。
    SampleShort,
    /// 0x1B12 窗长越预算（低于 1ms 下限或高于 20ms 预算上限）。
    WindowBudget,
    /// 0x1B13 单调性破缺（窗内源读数非非降序）。
    MonoBroken,
    /// 0x1B14 估算频率为零（源估算出 0 Hz，差分/推算链路断）。
    EstimateZero,
    /// 0x1B15 三源发散（任一两两 ppm 偏差越 1000ppm 容差）。
    CrossDiverge,
    /// 0x1B16 中位数裁决不可解（两源同越中位数容差，共识不存在）。
    MedianUnresolved,
    /// 0x1B17 校准账破缺（轮账/事件账字段缺失或序错）。
    LedgerBreak,
    /// 0x1B18 回放对拍不一致（回放重演与原账字段级不齐）。
    ReplayMismatch,
    /// 0x1B19 行为等价对拍失败（与 LegacyCalib 参照面结论相异）。
    EquivalenceFail,
    /// 0x1B1A 注入未检出（注入面三案例任一逃逸）。
    InjectUndetected,
    /// 0x1B1B 指纹漂移（同账二次指纹不相等）。
    FingerprintDrift,
    /// 0x1B1C 双跑不一致（同构两次校准指纹相异）。
    DualRunMismatch,
    /// 0x1B1D 健康降级（八项健康检查任一未过）。
    HealthDegraded,
    /// 0x1B1E 报告键集破缺（启动报告必备 kv 键缺失或空值）。
    ReportKv,
    /// 0x1B1F 段位守卫失败（诊断码值越出 0x1B10..=0x1B1F）。
    SegmentGuard,
}

impl CcCode {
    /// 全量闭集（十六码，序同变体声明序）。
    pub const ALL: [CcCode; 16] = [
        CcCode::SourceAbsent,
        CcCode::SampleShort,
        CcCode::WindowBudget,
        CcCode::MonoBroken,
        CcCode::EstimateZero,
        CcCode::CrossDiverge,
        CcCode::MedianUnresolved,
        CcCode::LedgerBreak,
        CcCode::ReplayMismatch,
        CcCode::EquivalenceFail,
        CcCode::InjectUndetected,
        CcCode::FingerprintDrift,
        CcCode::DualRunMismatch,
        CcCode::HealthDegraded,
        CcCode::ReportKv,
        CcCode::SegmentGuard,
    ];

    /// wire 码值（段基址 + 变体序）。
    pub const fn val(self) -> u16 {
        CC_SEG_BASE + self as u16
    }

    /// 段位守卫：码值必须落在 0x1B10..=0x1B1F。
    pub const fn in_segment(self) -> bool {
        let v = self.val();
        v >= CC_SEG_BASE && v <= CC_SEG_BASE + 15
    }

    /// 中文诊断（人话原因）。
    pub const fn text(self) -> &'static str {
        match self {
            CcCode::SourceAbsent => "三源缺源或腿位错：MSR/HPET/参照三腿未挂齐或源类型与腿位不符",
            CcCode::SampleShort => "样本数越界：每源每窗样本必须落在 16..=1024",
            CcCode::WindowBudget => "窗长越预算：窗长必须落在 1ms..=20ms",
            CcCode::MonoBroken => "单调性破缺：窗内源读数出现回退，采样链路不可信",
            CcCode::EstimateZero => "估算频率为零：源差分或推算链路断，频率无解",
            CcCode::CrossDiverge => "三源发散：两两千分比偏差越 1000ppm（0.1%）容差",
            CcCode::MedianUnresolved => "中位数裁决不可解：两源同越中位数容差，无多数共识",
            CcCode::LedgerBreak => "校准账破缺：轮账或事件账字段缺失、序错，账本不可回放",
            CcCode::ReplayMismatch => "回放对拍不一致：回放重演与原账字段级不齐",
            CcCode::EquivalenceFail => "行为等价对拍失败：与 TscCalibration 参照面结论相异",
            CcCode::InjectUndetected => "注入未检出：离群源/单调破/窗口不足注入逃逸",
            CcCode::FingerprintDrift => "指纹漂移：同账两次 FNV 指纹不相等",
            CcCode::DualRunMismatch => "双跑不一致：同构两次校准指纹相异，确定性失守",
            CcCode::HealthDegraded => "健康降级：八项健康检查任一未过，校准面降级运行",
            CcCode::ReportKv => "报告键集破缺：启动报告必备 kv 键缺失或空值",
            CcCode::SegmentGuard => "段位守卫失败：诊断码分配越出 0x1B10..=0x1B1F",
        }
    }

    /// 可操作建议（中文，逐码可执行）。
    pub const fn advice(self) -> &'static str {
        match self {
            CcCode::SourceAbsent => "按腿位闭集挂源：MSR 腿收 MsrSource/PropagateSource 之一，HPET 腿收 HpetSource，参照腿收 RefSource",
            CcCode::SampleShort => "调大每窗样本数到 16..=1024；窗长加大可摊薄量化误差",
            CcCode::WindowBudget => "窗长取 10ms 缺省值；预算上限 20ms 内仍有 2 倍余量",
            CcCode::MonoBroken => "查源实现读数序：抽象 tick 读数必须随逻辑 ns 非降序；注入面调试后必须清注入",
            CcCode::EstimateZero => "查差分两端读数是否相等（窗长过短）或比率推算输入为零",
            CcCode::CrossDiverge => "三源互差必须 <0.1%：查注入残留、源周期/比率常量是否与平台真值同源",
            CcCode::MedianUnresolved => "三源至少两源共识才可裁决：逐源排查，优先信 MSR 比率推算源",
            CcCode::LedgerBreak => "轮账三元组与逐源记录必须齐备；事件 seq 必须单调",
            CcCode::ReplayMismatch => "回放工厂必须与原跑同构（同源配置同注入）；源模型必须纯函数",
            CcCode::EquivalenceFail => "对照 cpu/clock.rs TscCalibration::choose 语义逐分支核对候选表实现",
            CcCode::InjectUndetected => "注入案例闭集三案例逐个核对检出判据，不允许静默逃逸",
            CcCode::FingerprintDrift => "指纹输入必须只含账本确定性字段；查可变依赖",
            CcCode::DualRunMismatch => "校准全程必须无墙钟/无随机：查隐藏状态或迭代序不稳",
            CcCode::HealthDegraded => "按 health() 返回的未过项逐项修复后再签发",
            CcCode::ReportKv => "报告键闭集以 export_keys() 为准，缺键即管线断",
            CcCode::SegmentGuard => "核对本模块段基址 0x1B10；码值必须落在段内",
        }
    }
}

/// 段位守卫（模块级）：全量闭集十六码必须全部落段。
pub fn segment_guard() -> bool {
    CcCode::ALL.iter().all(|c| c.in_segment())
}

// ---------------------------------------------------------------------------
// 二、错误枚举（Result<T, TscCalError> 全出口）
// ---------------------------------------------------------------------------

/// TSC 频率校准错误。每变体带 `detail()` 中文可操作诊断。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TscCalError {
    /// 三源缺源/腿位错。
    SourceAbsent,
    /// 样本数越界。
    SampleShort,
    /// 窗长越预算。
    WindowBudget,
    /// 单调性破缺。
    MonoBroken,
    /// 估算频率为零。
    EstimateZero,
    /// 三源发散（带最大两两 ppm）。
    CrossDiverge(u64),
    /// 中位数裁决不可解。
    MedianUnresolved,
    /// 校准账破缺。
    LedgerBreak,
    /// 回放对拍不一致（带一致轮数/总轮数）。
    ReplayMismatch(u32, u32),
    /// 行为等价对拍失败（带匹配数/总数）。
    EquivalenceFail(u32, u32),
    /// 注入未检出（带案例序）。
    InjectUndetected(u8),
    /// 指纹漂移。
    FingerprintDrift,
    /// 双跑不一致。
    DualRunMismatch,
    /// 健康降级（带得分）。
    HealthDegraded(u8),
    /// 报告键集破缺（带缺失键名）。
    ReportKv(&'static str),
    /// 段位守卫失败。
    SegmentGuard,
}

impl TscCalError {
    /// 对应诊断码。
    pub const fn code(&self) -> CcCode {
        match self {
            TscCalError::SourceAbsent => CcCode::SourceAbsent,
            TscCalError::SampleShort => CcCode::SampleShort,
            TscCalError::WindowBudget => CcCode::WindowBudget,
            TscCalError::MonoBroken => CcCode::MonoBroken,
            TscCalError::EstimateZero => CcCode::EstimateZero,
            TscCalError::CrossDiverge(_) => CcCode::CrossDiverge,
            TscCalError::MedianUnresolved => CcCode::MedianUnresolved,
            TscCalError::LedgerBreak => CcCode::LedgerBreak,
            TscCalError::ReplayMismatch(_, _) => CcCode::ReplayMismatch,
            TscCalError::EquivalenceFail(_, _) => CcCode::EquivalenceFail,
            TscCalError::InjectUndetected(_) => CcCode::InjectUndetected,
            TscCalError::FingerprintDrift => CcCode::FingerprintDrift,
            TscCalError::DualRunMismatch => CcCode::DualRunMismatch,
            TscCalError::HealthDegraded(_) => CcCode::HealthDegraded,
            TscCalError::ReportKv(_) => CcCode::ReportKv,
            TscCalError::SegmentGuard => CcCode::SegmentGuard,
        }
    }

    /// 中文可操作诊断（错误变体级 detail，含参数内插口径的静态近似）。
    pub const fn detail(&self) -> &'static str {
        match self {
            TscCalError::SourceAbsent => "三源缺源/腿位错：按腿位闭集挂齐 Msr|Propagate+Hpet+Ref 三腿后再校准",
            TscCalError::SampleShort => "样本数越界：每窗样本数须 16..=1024，下限来自单调校验最小统计量",
            TscCalError::WindowBudget => "窗长越预算：窗长须 1ms..=20ms，10ms 缺省窗对参照腿留 10 倍容差余量",
            TscCalError::MonoBroken => "窗内读数回退：源读数模型必须随逻辑 ns 非降序，查注入残留",
            TscCalError::EstimateZero => "源估算 0 Hz：查差分两端相等（窗过短）或比率/周期输入为零",
            TscCalError::CrossDiverge(_) => "三源互差越 1000ppm：查注入残留与源常量（周期/比率/BCLK）是否与平台同源",
            TscCalError::MedianUnresolved => "两源同越中位数容差：无多数共识，逐源排查后重跑",
            TscCalError::LedgerBreak => "账本自洽失守：轮账/事件账必须齐备且事件 seq 单调",
            TscCalError::ReplayMismatch(_, _) => "回放与原账不一致：回放工厂必须与原跑同构，源模型必须纯函数",
            TscCalError::EquivalenceFail(_, _) => "与 TscCalibration 参照面结论相异：逐分支核对候选表与 muldiv 双实现",
            TscCalError::InjectUndetected(_) => "注入案例逃逸：核对离群/单调破/窗口不足三案例的检出判据",
            TscCalError::FingerprintDrift => "同账指纹漂移：指纹输入只允许账本确定性字段",
            TscCalError::DualRunMismatch => "双跑指纹相异：校准面存在隐藏状态或非确定迭代",
            TscCalError::HealthDegraded(_) => "健康分未满：按 health() 未过项逐项修复",
            TscCalError::ReportKv(_) => "启动报告必备键缺失：以 export_keys() 键闭集为准补齐",
            TscCalError::SegmentGuard => "诊断码越段：段基址 0x1B10，闭集 0x1B10..=0x1B1F",
        }
    }
}

// ---------------------------------------------------------------------------
// 三、源闭集与采样模型
// ---------------------------------------------------------------------------

/// 校准源闭集（四实现三腿：MSR 腿两模式互斥，HPET 腿、参照腿各一）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SourceKind {
    /// MSR 腿·直接差分模式：MSR 0x10（IA32_TIME_STAMP_COUNTER）窗内差分。
    Msr,
    /// MSR 腿·比率推算模式：MSR_PLATFORM_INFO(0xCE)[15:8] 最大非睿频比率 × BCLK。
    Propagate,
    /// HPET 腿：主计数器（周期 fs 口径）与 TSC 交叉。
    Hpet,
    /// 参照腿：PIT 链抽象参照计数器（1.193182 MHz）与 TSC 交叉。
    Ref,
}

impl SourceKind {
    /// 全量闭集。
    pub const ALL: [SourceKind; 4] = [SourceKind::Msr, SourceKind::Propagate, SourceKind::Hpet, SourceKind::Ref];

    /// wire 名。
    pub const fn name(self) -> &'static str {
        match self {
            SourceKind::Msr => "msr",
            SourceKind::Propagate => "propagate",
            SourceKind::Hpet => "hpet",
            SourceKind::Ref => "ref",
        }
    }

    /// 中文说明。
    pub const fn zh(self) -> &'static str {
        match self {
            SourceKind::Msr => "MSR 直接差分（MSR 0x10 窗内差分 + 0xCE 证据）",
            SourceKind::Propagate => "MSR 比率推算（0xCE[15:8] 最大非睿频比率 × BCLK 100MHz）",
            SourceKind::Hpet => "HPET 交叉（主计数器周期 fs 口径）",
            SourceKind::Ref => "参照时钟交叉（PIT 链 1.193182MHz 抽象计数器）",
        }
    }

    /// 腿位秩（MSR 腿=0、HPET 腿=1、参照腿=2）。
    pub const fn leg(self) -> usize {
        match self {
            SourceKind::Msr | SourceKind::Propagate => 0,
            SourceKind::Hpet => 1,
            SourceKind::Ref => 2,
        }
    }
}

/// 单样本：逻辑时点 ns + 源抽象 tick 读数。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SourceSample {
    /// 采样时点（逻辑 ns，窗内单调递增）。
    pub at_ns: u64,
    /// 源抽象 tick 读数（非降序）。
    pub ticks: u64,
}

/// 单调性校验：逐样本非降序（允许持平——低频源可能跨点同 tick）。
pub fn samples_monotonic(samples: &[SourceSample]) -> bool {
    let mut prev: Option<u64> = None;
    for s in samples {
        if let Some(p) = prev {
            if s.ticks < p {
                return false;
            }
        }
        prev = Some(s.ticks);
    }
    true
}

/// 采样窗三元组（起点/窗长/样本数），构造即纪律校验。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SampleWindow {
    /// 窗起点（逻辑 ns）。
    pub start_ns: u64,
    /// 窗长（ns）。
    pub window_ns: u64,
    /// 每源样本数。
    pub samples: u32,
}

impl SampleWindow {
    /// 构造并过三门槛（窗长上下限、样本数上下限、起点饱和防溢出）。
    pub fn new(start_ns: u64, window_ns: u64, samples: u32) -> Result<Self, TscCalError> {
        if window_ns < CC_MIN_WINDOW_NS || window_ns > CC_WINDOW_NS {
            return Err(TscCalError::WindowBudget);
        }
        if samples < CC_SAMPLES_MIN || samples > CC_SAMPLES_MAX {
            return Err(TscCalError::SampleShort);
        }
        Ok(SampleWindow { start_ns, window_ns, samples })
    }

    /// 第 i 个采样时点（均匀铺点：start + i*window/(samples-1)）。
    /// 纯整数运算；i 必须 < samples（越界返回 0 由调用方保证不触发——
    /// 生成循环用 while i < samples 驱动）。
    pub fn at(&self, i: u32) -> u64 {
        let n = self.samples.saturating_sub(1) as u64;
        if n == 0 {
            return self.start_ns;
        }
        let off = (i as u64).saturating_mul(self.window_ns) / n;
        self.start_ns.saturating_add(off)
    }

    /// 窗终点。
    pub fn end_ns(&self) -> u64 {
        self.start_ns.saturating_add(self.window_ns)
    }
}

/// TSC 注入视图：合成平台 TSC 模型（tsc0 + 真值频率 × 逻辑 ns）。
/// 纯函数、确定性；真值仅注入侧可见，判定器只拿到读数。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TscView {
    /// TSC 基址。
    pub tsc0: u64,
    /// 真值频率（Hz）。
    pub freq_hz: u64,
}

impl TscView {
    /// 缺省合成平台视图（2.6 GHz @ 基址 CC_TSC_BASE）。
    pub const fn synth() -> Self {
        TscView { tsc0: CC_TSC_BASE, freq_hz: CC_TRUE_TSC_HZ }
    }

    /// 逻辑时点 ns 处的 TSC 读数（128 位中间量，饱和收口 u64）。
    pub fn at(&self, at_ns: u64) -> u64 {
        let delta = ((at_ns as u128) * (self.freq_hz as u128) / 1_000_000_000u128) as u64;
        self.tsc0.wrapping_add(delta)
    }
}

// ---------------------------------------------------------------------------
// 四、CalibSource trait：三源闭集注入口
// ---------------------------------------------------------------------------

/// 单源单轮采样运行账：逐源样本 + 耗时口径 + 声明值证据。
#[derive(Debug, Clone)]
pub struct SourceRun {
    /// 源类型。
    pub kind: SourceKind,
    /// 窗内逐样本。
    pub samples: Vec<SourceSample>,
    /// 源口径的窗耗时（ns）——交叉源来自自身时钟，比率源为声明口径。
    pub elapsed_ns: u64,
    /// 窗内自身 tick 增量。
    pub tick_delta: u64,
    /// 声明频率证据（PropagateSource 比率推算结果；其余 None）。
    pub declared_hz: Option<u64>,
    /// 采样证据注记（如 MSR 0xCE 原始值）。
    pub evidence: String,
}

/// 校准源注入口 trait（三源闭集）。每源提供：
/// ① 抽象 tick 读数模型（纯函数 of 逻辑 ns，可注入扰动）；
/// ② 单轮采样（窗内铺点取读数）；
/// ③ TSC 频率估算（自身口径窗耗时 ↔ TSC 差分交叉，或比率推算声明）。
pub trait CalibSource {
    /// 源类型（闭集）。
    fn kind(&self) -> SourceKind;

    /// 源名（含注入态注记，供账本留痕）。
    fn describe(&self) -> String;

    /// 抽象 tick 读数（逻辑 ns 处；确定性纯函数 + 可注入扰动）。
    fn read_ticks(&mut self, at_ns: u64) -> u64;

    /// 单轮采样：窗内均匀铺点取读数（经 `sample_ticks` 自由铺点函数），
    /// 交叉源覆写以落账自身时钟口径的 elapsed_ns。
    fn sample(&mut self, w: &SampleWindow) -> SourceRun;

    /// TSC 频率估算（Hz）。缺省实现（交叉源共用）：源口径窗耗时
    /// `run.elapsed_ns`（各交叉源在自身 sample() 中以自身时钟换算落账）
    /// ↔ TSC 窗差分对拍：est = tsc_delta × 1e9 ÷ elapsed_ns。
    /// 比率推算源（PropagateSource）重写为声明口径，不走此路径。
    /// 全整数 128 位中间量，饱和收口——无浮点、无溢出 panic 面。
    fn estimate(&mut self, w: &SampleWindow, tsc: &TscView) -> Result<(u64, SourceRun), TscCalError> {
        let mut run = self.sample(w);
        if !samples_monotonic(&run.samples) {
            return Err(TscCalError::MonoBroken);
        }
        let end_ns = w.end_ns();
        let tsc_delta = tsc.at(end_ns).saturating_sub(tsc.at(w.start_ns));
        if run.elapsed_ns == 0 || tsc_delta == 0 {
            return Err(TscCalError::EstimateZero);
        }
        let est = muldiv_sat(tsc_delta, 1_000_000_000, run.elapsed_ns);
        if est == 0 {
            return Err(TscCalError::EstimateZero);
        }
        run.evidence = format!("ticks={} elapsed_ns={} tsc_delta={}", run.tick_delta, run.elapsed_ns, tsc_delta);
        Ok((est, run))
    }
}

/// 128 位中间量 muldiv，饱和收口 u64（div=0 回 0——语义同参照面）。
pub fn muldiv_sat(v: u64, mul: u64, div: u64) -> u64 {
    let v = v as u128;
    let mul = mul as u128;
    let div = div as u128;
    if div == 0 {
        return 0;
    }
    let r = (v.saturating_mul(mul)) / div;
    if r > u64::MAX as u128 {
        u64::MAX
    } else {
        r as u64
    }
}

/// 两值 ppm 偏差：|a−b|×10^6÷max(a,b)，全整数 128 位中间量。
/// 结果恒 ≤ 10^6（分母取大者），零溢出面。
pub fn ppm_dev(a: u64, b: u64) -> u64 {
    if a == 0 && b == 0 {
        return 0;
    }
    let big = if a >= b { a } else { b };
    let small = if a >= b { b } else { a };
    let diff = (big - small) as u128;
    ((diff * 1_000_000u128) / (big as u128)) as u64
}

// ---------------------------------------------------------------------------
// 五、MSR 腿·直接差分模式（MsrSource）
// ---------------------------------------------------------------------------

/// MSR 腿·直接差分源：注入合成 MSR 空间（0x10 = TSC、0xCE =
/// PLATFORM_INFO 证据），窗内差分 TSC 对逻辑窗耗时估算频率。
/// 注入面：单调破注入（指定样本位回退）。
#[derive(Debug, Clone)]
pub struct MsrSource {
    /// 注入 TSC 视图（本源读 MSR 0x10 即 TSC）。
    view: TscView,
    /// 单调破注入：Some((样本位, 回退量))。
    monobreak: Option<(u32, u64)>,
    /// 0xCE 原始值（比率域注入由 PropagateSource 承担，此处作证据面）。
    platform_info_raw: u64,
}

impl MsrSource {
    /// 建（合成 MSR 空间：0xCE = 比率 26 << 8，锁定位按 Intel 口径补 0）。
    pub fn new(view: TscView) -> Self {
        MsrSource {
            view,
            monobreak: None,
            platform_info_raw: (CC_MAX_NON_TURBO_RATIO << CC_RATIO_SHIFT) as u64,
        }
    }

    /// 注入单调破（样本位 idx 处读数回退 CC_MONOBREAK_RETREAT tick——
    /// 回退量必须显著超过窗内相邻样本的 TSC 间距：10ms 窗 33 样本
    /// @2.6GHz ≈ 81.25 万 tick/间距，4096 级回退不破序）。
    pub fn with_monobreak(mut self, idx: u32) -> Self {
        self.monobreak = Some((idx, CC_MONOBREAK_RETREAT));
        self
    }

    /// 读合成 MSR。
    pub fn read_msr(&self, addr: u32, at_ns: u64) -> Option<u64> {
        match addr {
            0x10 => Some(self.view.at(at_ns)),
            CC_MSR_PLATFORM_INFO => Some(self.platform_info_raw),
            _ => None,
        }
    }
}

impl CalibSource for MsrSource {
    fn kind(&self) -> SourceKind {
        SourceKind::Msr
    }

    fn describe(&self) -> String {
        format!("MsrSource(0x10差分, 0xCE={:#x}, monobreak={:?})", self.platform_info_raw, self.monobreak.is_some())
    }

    fn read_ticks(&mut self, at_ns: u64) -> u64 {
        match self.read_msr(0x10, at_ns) {
            Some(v) => v,
            None => 0,
        }
    }

    fn sample(&mut self, w: &SampleWindow) -> SourceRun {
        let mut run = sample_ticks(self, w);
        if let Some((idx, retreat)) = self.monobreak {
            // 单调破注入：命中位回退—— saturating 防下溢，回退量保序破缺。
            let mut i: u32 = 0;
            while i < run.samples.len() as u32 {
                if i == idx {
                    let s = &mut run.samples[i as usize];
                    s.ticks = s.ticks.saturating_sub(retreat);
                }
                i = i.saturating_add(1);
            }
        }
        run.evidence = format!("msr_0x10_diff win_ns={} 0xCE={:#x}", run.elapsed_ns, self.platform_info_raw);
        run
    }
}

/// 铺点采样自由函数：窗内均匀铺点取 `src.read_ticks`，聚合 tick 增量。
/// （dyn 复用桩——各源 sample() 覆写前先经此铺点。）
pub fn sample_ticks(src: &mut dyn CalibSource, w: &SampleWindow) -> SourceRun {
    let mut samples: Vec<SourceSample> = Vec::with_capacity(w.samples as usize);
    let mut i: u32 = 0;
    while i < w.samples {
        let at = w.at(i);
        samples.push(SourceSample { at_ns: at, ticks: src.read_ticks(at) });
        i = i.saturating_add(1);
    }
    let first = samples.first().map(|s| s.ticks).unwrap_or(0);
    let last = samples.last().map(|s| s.ticks).unwrap_or(0);
    SourceRun {
        kind: src.kind(),
        tick_delta: last.saturating_sub(first),
        samples,
        elapsed_ns: w.window_ns,
        declared_hz: None,
        evidence: String::new(),
    }
}

// ---------------------------------------------------------------------------
// 六、MSR 腿·比率推算模式（PropagateSource）
// ---------------------------------------------------------------------------

/// MSR 腿·比率推算源：读 MSR_PLATFORM_INFO(0xCE)[15:8] 最大非睿频比率，
/// × BCLK（100 MHz）推算 TSC 标称频率——无 TSC 采样的纯声明路径。
#[derive(Debug, Clone)]
pub struct PropagateSource {
    /// 0xCE 原始值（比率域可注入扰动）。
    platform_info_raw: u64,
}

impl PropagateSource {
    /// 建（合成 0xCE：比率 26）。
    pub fn new() -> Self {
        PropagateSource { platform_info_raw: (CC_MAX_NON_TURBO_RATIO << CC_RATIO_SHIFT) as u64 }
    }

    /// 注入比率偏差（+delta 个比率粒度=100MHz 粒度）——离群源注入载体之一。
    pub fn with_ratio_delta(mut self, delta: u64) -> Self {
        let ratio = ((self.platform_info_raw >> CC_RATIO_SHIFT) & CC_RATIO_MASK)
            .saturating_add(delta)
            << CC_RATIO_SHIFT;
        self.platform_info_raw = ratio;
        self
    }

    /// 比率推算：hz = ratio × BCLK（全整数，128 位防溢出）。
    pub fn declared_hz(&self) -> u64 {
        let ratio = (self.platform_info_raw >> CC_RATIO_SHIFT) & CC_RATIO_MASK;
        muldiv_sat(ratio, CC_BCLK_HZ, 1)
    }
}

impl Default for PropagateSource {
    fn default() -> Self {
        PropagateSource::new()
    }
}

impl CalibSource for PropagateSource {
    fn kind(&self) -> SourceKind {
        SourceKind::Propagate
    }

    fn describe(&self) -> String {
        format!("PropagateSource(0xCE={:#x} ratio={} bclk={})", self.platform_info_raw,
            (self.platform_info_raw >> CC_RATIO_SHIFT) & CC_RATIO_MASK, CC_BCLK_HZ)
    }

    fn read_ticks(&mut self, at_ns: u64) -> u64 {
        // 声明口径虚拟时钟：声明频率 × 逻辑 ns（供单调性校验铺点）。
        let declared = self.declared_hz();
        ((at_ns as u128) * (declared as u128) / 1_000_000_000u128) as u64
    }

    fn sample(&mut self, w: &SampleWindow) -> SourceRun {
        let mut run = sample_ticks(self, w);
        run.declared_hz = Some(self.declared_hz());
        run.evidence = format!("0xCE={:#x} ratio={} x BCLK={}", self.platform_info_raw,
            (self.platform_info_raw >> CC_RATIO_SHIFT) & CC_RATIO_MASK, CC_BCLK_HZ);
        run
    }

    fn estimate(&mut self, w: &SampleWindow, _tsc: &TscView) -> Result<(u64, SourceRun), TscCalError> {
        let run = self.sample(w);
        if !samples_monotonic(&run.samples) {
            return Err(TscCalError::MonoBroken);
        }
        let declared = run.declared_hz.unwrap_or(0);
        if declared == 0 {
            return Err(TscCalError::EstimateZero);
        }
        Ok((declared, run))
    }
}

// ---------------------------------------------------------------------------
// 七、HPET 腿（HpetSource）
// ---------------------------------------------------------------------------

/// HPET 腿：主计数器读数（周期 fs 口径）与 TSC 窗差分交叉估算。
/// elapsed（HPET 口径）= hpet_tick_delta × period_fs ÷ 10^6（fs→ns）。
/// 注入面：离群源注入（周期比例失配 ppm，模拟 HPET 时基漂移）。
#[derive(Debug, Clone)]
pub struct HpetSource {
    /// 主计数器周期（fs）。
    period_fs: u64,
    /// 离群注入：elapsed 放大比例（ppm）——0 为无注入。
    outlier_ppm: u64,
}

impl HpetSource {
    /// 建（周期 = 合成平台缺省 10ns）。
    pub fn new() -> Self {
        HpetSource { period_fs: CC_HPET_PERIOD_FS, outlier_ppm: 0 }
    }

    /// 注入离群（HPET 时基慢 ppm——估算值反向偏低 ppm，越容差即离群）。
    pub fn with_outlier_ppm(mut self, ppm: u64) -> Self {
        self.outlier_ppm = ppm;
        self
    }

    /// HPET 主计数器抽象读数：floor(at_ns × 1e6 ÷ period_fs)（fs↔ns 整数口径）。
    pub fn counter_at(&self, at_ns: u64) -> u64 {
        ((at_ns as u128) * 1_000_000u128 / (self.period_fs as u128)) as u64
    }
}

impl Default for HpetSource {
    fn default() -> Self {
        HpetSource::new()
    }
}

impl CalibSource for HpetSource {
    fn kind(&self) -> SourceKind {
        SourceKind::Hpet
    }

    fn describe(&self) -> String {
        format!("HpetSource(period_fs={}, outlier_ppm={})", self.period_fs, self.outlier_ppm)
    }

    fn read_ticks(&mut self, at_ns: u64) -> u64 {
        self.counter_at(at_ns)
    }

    fn sample(&mut self, w: &SampleWindow) -> SourceRun {
        let mut run = sample_ticks(self, w);
        // HPET 口径窗耗时：tick_delta × period_fs ÷ 1e6（fs→ns），128 位防溢出。
        let mut elapsed = muldiv_sat(run.tick_delta, self.period_fs, 1_000_000);
        // 离群注入：HPET 时基慢 outlier_ppm → elapsed 放大 → 估算反向偏低。
        if self.outlier_ppm > 0 {
            let scale = 1_000_000u64.saturating_add(self.outlier_ppm);
            elapsed = muldiv_sat(elapsed, scale, 1_000_000);
        }
        run.elapsed_ns = elapsed;
        run.evidence = format!("hpet_ticks={} period_fs={} outlier_ppm={}", run.tick_delta, self.period_fs, self.outlier_ppm);
        run
    }
}

// ---------------------------------------------------------------------------
// 八、参照腿（RefSource）
// ---------------------------------------------------------------------------

/// 参照腿：PIT 链抽象参照计数器（1.193182 MHz）与 TSC 窗差分交叉估算。
/// elapsed（参照口径）= ref_tick_delta × 1e9 ÷ CC_REF_RATE_HZ。
#[derive(Debug, Clone)]
pub struct RefSource {
    /// 参照计数器频率（Hz；写死 PIT 基频口径，见常量来源注）。
    rate_hz: u64,
}

impl RefSource {
    /// 建（频率 = CC_REF_RATE_HZ = 1.193182 MHz）。
    pub fn new() -> Self {
        RefSource { rate_hz: CC_REF_RATE_HZ }
    }

    /// 参照计数器抽象读数：floor(at_ns × rate ÷ 1e9)。
    pub fn counter_at(&self, at_ns: u64) -> u64 {
        ((at_ns as u128) * (self.rate_hz as u128) / 1_000_000_000u128) as u64
    }
}

impl Default for RefSource {
    fn default() -> Self {
        RefSource::new()
    }
}

impl CalibSource for RefSource {
    fn kind(&self) -> SourceKind {
        SourceKind::Ref
    }

    fn describe(&self) -> String {
        format!("RefSource(rate_hz={})", self.rate_hz)
    }

    fn read_ticks(&mut self, at_ns: u64) -> u64 {
        self.counter_at(at_ns)
    }

    fn sample(&mut self, w: &SampleWindow) -> SourceRun {
        let mut run = sample_ticks(self, w);
        // 参照口径窗耗时：tick_delta × 1e9 ÷ rate_hz（128 位防溢出）。
        run.elapsed_ns = muldiv_sat(run.tick_delta, 1_000_000_000, self.rate_hz);
        run.evidence = format!("ref_ticks={} rate_hz={}", run.tick_delta, self.rate_hz);
        run
    }
}

// ---------------------------------------------------------------------------
// 九、三取中裁决（离群源显性降权不弃）
// ---------------------------------------------------------------------------

/// 单调破注入回退量：800 万 tick（窗内样本间距 ~81.25 万 tick 的
/// 约 10 倍，保证破序面严格成立）。
pub const CC_MONOBREAK_RETREAT: u64 = 8_000_000;

/// 权重常量：常规源。
pub const CC_WEIGHT_FULL: u32 = 100;
/// 权重常量：离群降权源（显性降权 100→25，留账不弃）。
pub const CC_WEIGHT_DOWN: u32 = 25;

/// 三取中：比较法中位数（a≤b≤c 语义，全整数比较，无排序分配）。
pub fn median3(a: u64, b: u64, c: u64) -> u64 {
    let (lo, hi) = if a <= b { (a, b) } else { (b, a) };
    if c <= lo {
        lo
    } else if c >= hi {
        hi
    } else {
        c
    }
}

/// 单源单轮裁决记录。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceLegRecord {
    /// 源类型。
    pub kind: SourceKind,
    /// 估算频率（Hz）。
    pub est_hz: u64,
    /// 样本数。
    pub sample_count: u32,
    /// 源口径窗耗时（ns）。
    pub elapsed_ns: u64,
    /// 窗内单调性。
    pub mono_ok: bool,
    /// 权重（100 常规 / 25 离群降权）。
    pub weight: u32,
    /// 源描述（含注入态注记）。
    pub desc: String,
    /// 采样证据。
    pub evidence: String,
}

/// 轮判决闭集。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RoundVerdict {
    /// 全中：三源互差全部 < 1000ppm。
    AllHit,
    /// 发散：有离群源（已显性降权，中位数裁决仍有效）。
    Diverge,
    /// 发散不可解：两源同越中位数容差，无多数共识。
    MedianUnresolved,
    /// 单调破缺：某源窗内读数回退（该源本轮废估，轮不参与全中率）。
    MonoBroken,
    /// 源估算失败（EstimateZero 等基础设施错）。
    SourceFailed,
}

impl RoundVerdict {
    /// wire 名。
    pub const fn name(self) -> &'static str {
        match self {
            RoundVerdict::AllHit => "all_hit",
            RoundVerdict::Diverge => "diverge",
            RoundVerdict::MedianUnresolved => "median_unresolved",
            RoundVerdict::MonoBroken => "mono_broken",
            RoundVerdict::SourceFailed => "source_failed",
        }
    }
}

/// 单轮校准账（回放原子）。
#[derive(Debug, Clone)]
pub struct RoundOutcome {
    /// 轮号（0 起，单调）。
    pub round: u32,
    /// 窗口三元组。
    pub start_ns: u64,
    pub window_ns: u64,
    pub samples: u32,
    /// 三腿逐源记录（腿位序：MSR 腿 / HPET 腿 / 参照腿）。
    pub legs: Vec<SourceLegRecord>,
    /// 两两 ppm 偏差（序：msr-hpet / msr-ref / hpet-ref）。
    pub pairwise_ppm: Vec<u64>,
    /// 最大两两 ppm。
    pub max_ppm: u64,
    /// 三取中裁决频率（Hz）。
    pub median_hz: u64,
    /// 离群源闭集（被降权者，留账不弃）。
    pub outliers: Vec<SourceKind>,
    /// 判决。
    pub verdict: RoundVerdict,
}

impl RoundOutcome {
    /// 轮内全中判据：判决 = AllHit 且三腿皆 mono 且权重全 100。
    pub fn all_hit(&self) -> bool {
        self.verdict == RoundVerdict::AllHit
            && self.legs.iter().all(|l| l.mono_ok && l.weight == CC_WEIGHT_FULL)
            && self.max_ppm < CC_TOLERANCE_PPM
    }
}

// ---------------------------------------------------------------------------
// 十、事件账（八类闭集）
// ---------------------------------------------------------------------------

/// 事件种类闭集（八类）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EventKind {
    /// 轮完成（含判决）。
    RoundCompleted,
    /// 离群源降权。
    OutlierDownweighted,
    /// 单调破缺检出。
    MonoBreakDetected,
    /// 窗口纪律违例（窗长/样本越界）。
    WindowViolation,
    /// 发散检出。
    DivergeDetected,
    /// 等价对拍完成。
    EquivalenceChecked,
    /// 回放对拍完成。
    ReplayChecked,
    /// 双跑对拍完成。
    DualRunChecked,
}

impl EventKind {
    /// 全量闭集。
    pub const ALL: [EventKind; 8] = [
        EventKind::RoundCompleted,
        EventKind::OutlierDownweighted,
        EventKind::MonoBreakDetected,
        EventKind::WindowViolation,
        EventKind::DivergeDetected,
        EventKind::EquivalenceChecked,
        EventKind::ReplayChecked,
        EventKind::DualRunChecked,
    ];

    /// wire 名。
    pub const fn name(self) -> &'static str {
        match self {
            EventKind::RoundCompleted => "round_completed",
            EventKind::OutlierDownweighted => "outlier_downweighted",
            EventKind::MonoBreakDetected => "mono_break_detected",
            EventKind::WindowViolation => "window_violation",
            EventKind::DivergeDetected => "diverge_detected",
            EventKind::EquivalenceChecked => "equivalence_checked",
            EventKind::ReplayChecked => "replay_checked",
            EventKind::DualRunChecked => "dual_run_checked",
        }
    }
}

/// 事件记录（seq 单调，账本追加序）。
#[derive(Debug, Clone)]
pub struct EventRecord {
    /// 单调序号（0 起）。
    pub seq: u64,
    /// 事件种类。
    pub kind: EventKind,
    /// 关联轮号（无轮关联填 u32::MAX 语义外的哨兵 0xFFFF_FFFF）。
    pub round: u32,
    /// 人话注记。
    pub text: String,
}

/// 哨兵轮号（事件与具体轮无关联时使用）。
pub const CC_ROUND_NONE: u32 = 0xFFFF_FFFF;

// ---------------------------------------------------------------------------
// 十一、FNV-1a 64 位指纹
// ---------------------------------------------------------------------------

/// FNV-1a 64 位偏移基（FNV-1a 规范常量，http://isthe.com/chongo/tech/comp/fnv/）。
pub const FNV_OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
/// FNV-1a 64 位素数（同上规范）。
pub const FNV_PRIME: u64 = 0x0000_0100_0000_01b3;

/// FNV-1a 64 位指纹（字节流逐位异或乘素数，wrapping 显性）。
pub fn fnv1a64(data: &[u8], seed: u64) -> u64 {
    let mut h = seed;
    let mut i = 0usize;
    while i < data.len() {
        h ^= data[i] as u64;
        h = h.wrapping_mul(FNV_PRIME);
        i = i.wrapping_add(1);
    }
    h
}

/// 字符串指纹便捷式（先指纹前缀种子再叠加字节）。
pub fn fnv1a64_str(s: &str, seed: u64) -> u64 {
    fnv1a64(s.as_bytes(), seed)
}

// ---------------------------------------------------------------------------
// 十二、三源交叉校准器（CrossCalibrator）
// ---------------------------------------------------------------------------

/// 腿位闭集（三腿）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LegSlot {
    /// MSR 腿（收 MsrSource 或 PropagateSource，两模式互斥）。
    Msr,
    /// HPET 腿（收 HpetSource）。
    Hpet,
    /// 参照腿（收 RefSource）。
    Ref,
}

impl LegSlot {
    /// 全量闭集。
    pub const ALL: [LegSlot; 3] = [LegSlot::Msr, LegSlot::Hpet, LegSlot::Ref];

    /// 腿位名。
    pub const fn name(self) -> &'static str {
        match self {
            LegSlot::Msr => "msr_leg",
            LegSlot::Hpet => "hpet_leg",
            LegSlot::Ref => "ref_leg",
        }
    }

    /// 腿位秩（MSR 腿=0、HPET 腿=1、参照腿=2）。
    pub const fn leg(self) -> usize {
        match self {
            LegSlot::Msr => 0,
            LegSlot::Hpet => 1,
            LegSlot::Ref => 2,
        }
    }

    /// 该腿是否接受给定源类型（腿位-源型闭集校验；const fn 用 match
    /// 避免 derive PartialEq 的 == 进常量路径）。
    pub const fn accepts(self, k: SourceKind) -> bool {
        match (self, k) {
            (LegSlot::Msr, SourceKind::Msr) => true,
            (LegSlot::Msr, SourceKind::Propagate) => true,
            (LegSlot::Hpet, SourceKind::Hpet) => true,
            (LegSlot::Ref, SourceKind::Ref) => true,
            _ => false,
        }
    }
}

/// 三源交叉校准器：三腿各一源，逐轮采样→估算→两两 ppm→三取中裁决。
/// 全程无墙钟、无随机——同一注入构型重跑必然逐位一致（双跑对拍基础）。
pub struct CrossCalibrator {
    /// 三腿（Box<dyn CalibSource>）。
    msr_leg: Option<Box<dyn CalibSource>>,
    hpet_leg: Option<Box<dyn CalibSource>>,
    ref_leg: Option<Box<dyn CalibSource>>,
    /// 事件账。
    events: Vec<EventRecord>,
    /// 事件 seq 游标。
    seq: u64,
    /// 立案诊断码（去重闭集）。
    codes: Vec<CcCode>,
}

impl CrossCalibrator {
    /// 空 校准器（三腿待挂）。
    pub fn new() -> Self {
        CrossCalibrator {
            msr_leg: None,
            hpet_leg: None,
            ref_leg: None,
            events: Vec::new(),
            seq: 0,
            codes: Vec::new(),
        }
    }

    fn note(&mut self, c: CcCode) {
        if !self.codes.contains(&c) {
            self.codes.push(c);
        }
    }

    fn event(&mut self, kind: EventKind, round: u32, text: String) {
        let seq = self.seq;
        self.seq = self.seq.saturating_add(1);
        self.events.push(EventRecord { seq, kind, round, text });
    }

    /// 挂源（腿位-源型闭集校验；不符即 SourceAbsent 立案——含错位语义）。
    pub fn attach(&mut self, slot: LegSlot, src: Box<dyn CalibSource>) -> Result<(), TscCalError> {
        if !slot.accepts(src.kind()) {
            self.note(CcCode::SourceAbsent);
            return Err(TscCalError::SourceAbsent);
        }
        match slot {
            LegSlot::Msr => self.msr_leg = Some(src),
            LegSlot::Hpet => self.hpet_leg = Some(src),
            LegSlot::Ref => self.ref_leg = Some(src),
        }
        Ok(())
    }

    /// 三腿齐备校验。
    pub fn legs_complete(&self) -> bool {
        self.msr_leg.is_some() && self.hpet_leg.is_some() && self.ref_leg.is_some()
    }

    /// 单轮校准：窗口纪律→逐源估算→两两 ppm→三取中裁决→账落。
    /// 判决性异常（发散/单调破/裁决不可解）走账本 verdict，不中断轮序；
    /// 基础设施异常（缺腿/窗口违例/估算零）回 Err 并事件留痕。
    pub fn run_round(&mut self, round: u32, view: &TscView) -> Result<RoundOutcome, TscCalError> {
        if !self.legs_complete() {
            self.note(CcCode::SourceAbsent);
            self.event(EventKind::WindowViolation, round, String::from("缺腿：三腿未挂齐即请求校准"));
            return Err(TscCalError::SourceAbsent);
        }
        let w = match SampleWindow::new(
            (round as u64).saturating_mul(CC_ROUND_STRIDE_NS),
            CC_ROUND_WINDOW_NS,
            CC_SAMPLES_DEFAULT,
        ) {
            Ok(w) => w,
            Err(e) => {
                self.note(e.code());
                self.event(EventKind::WindowViolation, round, format!("窗口构造失败：{}", e.detail()));
                return Err(e);
            }
        };
        // 逐源估算（腿位序）。单调破缺按判决留账（不中断轮序，账面完整可回放）。
        let mut legs: Vec<SourceLegRecord> = Vec::with_capacity(3);
        let mut ests: [Option<u64>; 3] = [None, None, None];
        let mut infra_fail = false;
        let mut mono_broken = false;
        let mut err_out: Option<TscCalError> = None;
        for slot in LegSlot::ALL {
            let leg = match slot {
                LegSlot::Msr => self.msr_leg.as_mut(),
                LegSlot::Hpet => self.hpet_leg.as_mut(),
                LegSlot::Ref => self.ref_leg.as_mut(),
            };
            let src = match leg {
                Some(s) => s,
                None => {
                    // attach 已校验，此处不可达；保守立账。
                    self.note(CcCode::SourceAbsent);
                    return Err(TscCalError::SourceAbsent);
                }
            };
            match src.estimate(&w, view) {
                Ok((hz, run)) => {
                    let mono_ok = samples_monotonic(&run.samples);
                    ests[slot.leg()] = Some(hz);
                    legs.push(SourceLegRecord {
                        kind: src.kind(),
                        est_hz: hz,
                        sample_count: run.samples.len() as u32,
                        elapsed_ns: run.elapsed_ns,
                        mono_ok,
                        weight: CC_WEIGHT_FULL,
                        desc: src.describe(),
                        evidence: run.evidence,
                    });
                }
                Err(e) => {
                    if e.code() == CcCode::MonoBroken {
                        // 单调破缺：该源本轮废估，轮判决 MonoBroken，账面照落。
                        mono_broken = true;
                        legs.push(SourceLegRecord {
                            kind: src.kind(),
                            est_hz: 0,
                            sample_count: 0,
                            elapsed_ns: 0,
                            mono_ok: false,
                            weight: CC_WEIGHT_FULL,
                            desc: src.describe(),
                            evidence: format!("mono_broken: {}", e.detail()),
                        });
                    } else {
                        infra_fail = true;
                        err_out = Some(e);
                        legs.push(SourceLegRecord {
                            kind: src.kind(),
                            est_hz: 0,
                            sample_count: 0,
                            elapsed_ns: 0,
                            mono_ok: false,
                            weight: CC_WEIGHT_FULL,
                            desc: src.describe(),
                            evidence: format!("estimate_err={:?}", e.code()),
                        });
                    }
                }
            }
        }
        if mono_broken {
            let out = RoundOutcome {
                round,
                start_ns: w.start_ns,
                window_ns: w.window_ns,
                samples: w.samples,
                legs,
                pairwise_ppm: Vec::new(),
                max_ppm: 0,
                median_hz: 0,
                outliers: Vec::new(),
                verdict: RoundVerdict::MonoBroken,
            };
            self.note(CcCode::MonoBroken);
            self.event(EventKind::MonoBreakDetected, round, format!("单调破缺检出：轮 {} 某源窗内读数回退，废估不参与全中率", round));
            self.event(EventKind::RoundCompleted, round, format!("轮 {} 完成：判决=mono_broken", round));
            return Ok(out);
        }
        if infra_fail {
            let e = err_out.unwrap_or(TscCalError::EstimateZero);
            let verdict = match e {
                TscCalError::MonoBroken => RoundVerdict::MonoBroken,
                _ => RoundVerdict::SourceFailed,
            };
            self.note(e.code());
            self.event(EventKind::WindowViolation, round, format!("源估算失败（判决 {:?}）：{}", verdict, e.detail()));
            // 基础设施错：事件留痕后回错（轮账不入正账）。
            return Err(e);
        }
        // 两两 ppm（msr-hpet / msr-ref / hpet-ref）。
        let e0 = ests[0].unwrap_or(0);
        let e1 = ests[1].unwrap_or(0);
        let e2 = ests[2].unwrap_or(0);
        let p01 = ppm_dev(e0, e1);
        let p02 = ppm_dev(e0, e2);
        let p12 = ppm_dev(e1, e2);
        let pairwise = vec![p01, p02, p12];
        let max_ppm = p01.max(p02).max(p12);
        // 三取中裁决。
        let med = median3(e0, e1, e2);
        let mut outliers: Vec<SourceKind> = Vec::new();
        let mut deviating: Vec<usize> = Vec::new();
        for (i, est) in ests.iter().enumerate() {
            if let Some(hz) = est {
                if ppm_dev(*hz, med) >= CC_TOLERANCE_PPM {
                    deviating.push(i);
                }
            }
        }
        // 裁决：0 离群=全中；1 离群=降权（多数共识保中位数）；≥2 离群=不可解。
        let verdict = if deviating.is_empty() {
            RoundVerdict::AllHit
        } else if deviating.len() == 1 {
            let idx = deviating[0];
            if let Some(l) = legs.get_mut(idx) {
                l.weight = CC_WEIGHT_DOWN;
            }
            if let Some(k) = ests_key(idx) {
                outliers.push(k);
                self.event(
                    EventKind::OutlierDownweighted,
                    round,
                    format!("离群源显性降权：{} 偏中位数 {}ppm（权重 {}→{}，留账不弃）",
                        k.name(), ppm_dev(ests[idx].unwrap_or(0), med), CC_WEIGHT_FULL, CC_WEIGHT_DOWN),
                );
            }
            RoundVerdict::Diverge
        } else {
            RoundVerdict::MedianUnresolved
        };
        if verdict == RoundVerdict::Diverge || verdict == RoundVerdict::MedianUnresolved {
            self.note(CcCode::CrossDiverge);
            self.event(
                EventKind::DivergeDetected,
                round,
                format!("发散检出：max_ppm={} 容差={} 判决={}", max_ppm, CC_TOLERANCE_PPM, verdict.name()),
            );
            if verdict == RoundVerdict::MedianUnresolved {
                self.note(CcCode::MedianUnresolved);
            }
        }
        let out = RoundOutcome {
            round,
            start_ns: w.start_ns,
            window_ns: w.window_ns,
            samples: w.samples,
            legs,
            pairwise_ppm: pairwise,
            max_ppm,
            median_hz: med,
            outliers,
            verdict,
        };
        self.event(
            EventKind::RoundCompleted,
            round,
            format!("轮 {} 完成：判决={} 中位数={}Hz max_ppm={}", round, out.verdict.name(), out.median_hz, out.max_ppm),
        );
        Ok(out)
    }

    /// 全量校准：CC_ROUNDS 轮逐轮跑齐，账本聚合。
    pub fn run_all(&mut self, view: &TscView) -> Result<CalibLedger, TscCalError> {
        if !self.legs_complete() {
            self.note(CcCode::SourceAbsent);
            return Err(TscCalError::SourceAbsent);
        }
        let mut rounds: Vec<RoundOutcome> = Vec::with_capacity(CC_ROUNDS as usize);
        let mut r: u32 = 0;
        while r < CC_ROUNDS {
            let out = self.run_round(r, view)?;
            rounds.push(out);
            r = r.saturating_add(1);
        }
        let ledger = CalibLedger {
            version: String::from(CC_VERSION),
            seg_base: CC_SEG_BASE,
            rounds,
            events: self.events.clone(),
            codes: self.codes.clone(),
        };
        ledger.self_audit()?;
        Ok(ledger)
    }

    /// 立案码闭集（只读）。
    pub fn codes(&self) -> &[CcCode] {
        &self.codes
    }

    /// 事件账（只读）。
    pub fn events(&self) -> &[EventRecord] {
        &self.events
    }
}

impl Default for CrossCalibrator {
    fn default() -> Self {
        CrossCalibrator::new()
    }
}

/// 腿位序 → 源型闭集映射（裁决降权标记用）。
fn ests_key(idx: usize) -> Option<SourceKind> {
    match idx {
        0 => Some(SourceKind::Msr),
        1 => Some(SourceKind::Hpet),
        2 => Some(SourceKind::Ref),
        _ => None,
    }
}

// ---------------------------------------------------------------------------
// 十三、校准账（CalibLedger）：聚合 + 自洽审计 + 指纹 + 摘要
// ---------------------------------------------------------------------------

/// 校准账：全轮账 + 事件账 + 立案码，可回放、可指纹、可摘要。
#[derive(Debug, Clone)]
pub struct CalibLedger {
    /// 模块版本（指纹输入）。
    pub version: String,
    /// 诊断段基址。
    pub seg_base: u16,
    /// 全轮账。
    pub rounds: Vec<RoundOutcome>,
    /// 事件账。
    pub events: Vec<EventRecord>,
    /// 立案码（去重闭集）。
    pub codes: Vec<CcCode>,
}

impl CalibLedger {
    /// 轮账数。
    pub fn round_count(&self) -> u32 {
        self.rounds.len() as u32
    }

    /// 三源全中率（百分比整型）：全中轮数 × 100 ÷ 总轮数（饱和收口）。
    /// 判据口径："三源全中率 100%"——每轮三源互差全越容差才算失手。
    pub fn all_hit_rate_pct(&self) -> u64 {
        if self.rounds.is_empty() {
            return 0;
        }
        let hits = self.rounds.iter().filter(|r| r.all_hit()).count() as u64;
        let total = self.rounds.len() as u64;
        muldiv_sat(hits.saturating_mul(100), 1, total)
    }

    /// 最大两两 ppm（全轮）。
    pub fn max_ppm_overall(&self) -> u64 {
        let mut m: u64 = 0;
        for r in &self.rounds {
            if r.max_ppm > m {
                m = r.max_ppm;
            }
        }
        m
    }

    /// 全轮互差达标判据：全中率 100% 且全轮 max_ppm < 容差。
    pub fn within_tolerance(&self) -> bool {
        self.all_hit_rate_pct() == 100 && self.max_ppm_overall() < CC_TOLERANCE_PPM
    }

    /// 自洽审计：轮账齐备、事件 seq 单调、腿位/记录数闭集。
    pub fn self_audit(&self) -> Result<(), TscCalError> {
        if self.rounds.is_empty() {
            return Err(TscCalError::LedgerBreak);
        }
        let mut prev_seq: Option<u64> = None;
        for ev in &self.events {
            if let Some(p) = prev_seq {
                if ev.seq <= p {
                    return Err(TscCalError::LedgerBreak);
                }
            }
            prev_seq = Some(ev.seq);
        }
        for r in &self.rounds {
            if r.legs.len() != 3 {
                return Err(TscCalError::LedgerBreak);
            }
            if r.window_ns < CC_MIN_WINDOW_NS || r.window_ns > CC_WINDOW_NS {
                return Err(TscCalError::LedgerBreak);
            }
            if r.samples < CC_SAMPLES_MIN || r.samples > CC_SAMPLES_MAX {
                return Err(TscCalError::LedgerBreak);
            }
        }
        Ok(())
    }

    /// 规范化摘要串（指纹输入面：版本+段+逐轮判决/中位数/ppm+事件 seq+码）。
    pub fn canonical_digest(&self) -> String {
        let mut s = String::new();
        s.push_str(&format!("{}|seg={:#06x}|rounds={}|", self.version, self.seg_base, self.rounds.len()));
        for r in &self.rounds {
            s.push_str(&format!("R{}:{},{},{},{:x};", r.round, r.verdict.name(), r.median_hz, r.max_ppm, r.start_ns));
            for l in &r.legs {
                s.push_str(&format!("[{},{},{:x},{}]", l.kind.name(), l.est_hz, l.elapsed_ns, l.weight));
            }
        }
        for ev in &self.events {
            s.push_str(&format!("E{}:{}:{};", ev.seq, ev.kind.name(), ev.round));
        }
        for c in &self.codes {
            s.push_str(&format!("C{:#06x};", c.val()));
        }
        s
    }

    /// FNV-1a 64 位指纹（覆盖账本全量确定性字段）。
    pub fn fingerprint(&self) -> u64 {
        fnv1a64_str(&self.canonical_digest(), FNV_OFFSET)
    }

    /// 人话摘要。
    pub fn digest(&self) -> String {
        format!(
            "TSC 三源校准账：{} 轮，全中率 {}%，全轮 max_ppm={}（容差 {}），离群降权 {} 次，立案码 {} 个",
            self.rounds.len(),
            self.all_hit_rate_pct(),
            self.max_ppm_overall(),
            CC_TOLERANCE_PPM,
            self.events.iter().filter(|e| e.kind == EventKind::OutlierDownweighted).count(),
            self.codes.len(),
        )
    }

    /// 终局校准结果：各轮中位数的中位数（离群永不污染）。
    pub fn final_hz(&self) -> Result<u64, TscCalError> {
        let mut meds: Vec<u64> = Vec::with_capacity(self.rounds.len());
        for r in &self.rounds {
            meds.push(r.median_hz);
        }
        if meds.is_empty() {
            return Err(TscCalError::LedgerBreak);
        }
        Ok(median_of_many(&meds))
    }
}

/// 多值中位数（n 值排序取中；n 上限 = 轮数 ≤ CC_ROUNDS，选择排序全整数）。
pub fn median_of_many(vals: &[u64]) -> u64 {
    if vals.is_empty() {
        return 0;
    }
    let mut v: Vec<u64> = Vec::with_capacity(vals.len());
    for x in vals {
        v.push(*x);
    }
    // 插入排序（n 很小，且免 Ord::sort 的分配面争议）。
    let n = v.len();
    let mut i = 1usize;
    while i < n {
        let mut j = i;
        while j > 0 && v[j] < v[j - 1] {
            v.swap(j, j - 1);
            j -= 1;
        }
        i += 1;
    }
    let mid = n / 2;
    if n % 2 == 1 {
        v[mid]
    } else {
        // 偶数取下中位（全整数口径，避免除二溢出）。
        v[mid - 1]
    }
}

// ---------------------------------------------------------------------------
// 十四、校准账回放（逐轮重演对拍，一致率必须 100%）
// ---------------------------------------------------------------------------

/// 源工厂：按构型重建同构三腿（回放重演的确定性基础——源模型必须纯函数）。
/// 构型：MSR 腿模式 / HPET 离群注入 ppm / TSC 视图。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SourceConfig {
    /// MSR 腿模式：true=Propagate（比率推算），false=Msr（直接差分）。
    pub msr_propagate: bool,
    /// HPET 离群注入 ppm（0 = 无注入）。
    pub hpet_outlier_ppm: u64,
    /// 注入 TSC 视图。
    pub view: TscView,
}

impl SourceConfig {
    /// 干净合成平台构型。
    pub const fn clean() -> Self {
        SourceConfig { msr_propagate: false, hpet_outlier_ppm: 0, view: TscView::synth() }
    }
}

/// 按构型装配完整校准器（三腿齐挂——装配面唯一入口，dyn 源按构型直造）。
pub fn assemble(cfg: &SourceConfig) -> Result<CrossCalibrator, TscCalError> {
    let mut cal = CrossCalibrator::new();
    let msr: Box<dyn CalibSource> = if cfg.msr_propagate {
        Box::new(PropagateSource::new())
    } else {
        Box::new(MsrSource::new(cfg.view))
    };
    cal.attach(LegSlot::Msr, msr)?;
    cal.attach(LegSlot::Hpet, Box::new(HpetSource::new().with_outlier_ppm(cfg.hpet_outlier_ppm)))?;
    cal.attach(LegSlot::Ref, Box::new(RefSource::new()))?;
    Ok(cal)
}

/// 回放报告。
#[derive(Debug, Clone)]
pub struct ReplayReport {
    /// 回放轮数。
    pub rounds: u32,
    /// 字段级一致轮数。
    pub matched: u32,
    /// 一致率（百分比整型）。
    pub match_rate_pct: u64,
    /// 首个不一致轮（如有）。
    pub first_mismatch: Option<u32>,
    /// 一致判据：matched == rounds 且 rounds > 0。
    pub consistent: bool,
}

/// 校准账回放：以同构源工厂重演逐轮，字段级对拍。
/// 对拍字段：窗口三元组 / 逐源（kind/est_hz/sample_count/elapsed_ns/
/// mono_ok/weight）/ 两两 ppm / 中位数 / 离群闭集 / 判决。
pub fn replay(ledger: &CalibLedger, cfg: &SourceConfig) -> Result<ReplayReport, TscCalError> {
    ledger.self_audit()?;
    let mut matched: u32 = 0;
    let mut first_mismatch: Option<u32> = None;
    for orig in &ledger.rounds {
        // 同构重演：独立装配同构型校准器跑同轮（源模型纯函数 → 逐位一致）。
        let mut cal = assemble(cfg)?;
        let re = cal.run_round(orig.round, &cfg.view)?;
        let same = orig.round == re.round
            && orig.start_ns == re.start_ns
            && orig.window_ns == re.window_ns
            && orig.samples == re.samples
            && orig.median_hz == re.median_hz
            && orig.max_ppm == re.max_ppm
            && orig.verdict == re.verdict
            && orig.pairwise_ppm == re.pairwise_ppm
            && orig.outliers == re.outliers
            && legs_same(&orig.legs, &re.legs);
        if same {
            matched = matched.saturating_add(1);
        } else if first_mismatch.is_none() {
            first_mismatch = Some(orig.round);
        }
    }
    let rounds = ledger.round_count();
    let rate = if rounds == 0 { 0 } else { muldiv_sat((matched.saturating_mul(100)) as u64, 1, rounds as u64) };
    Ok(ReplayReport {
        rounds,
        matched,
        match_rate_pct: rate,
        first_mismatch,
        consistent: rounds > 0 && matched == rounds,
    })
}

/// 逐源字段级对拍。
fn legs_same(a: &[SourceLegRecord], b: &[SourceLegRecord]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut i = 0usize;
    while i < a.len() {
        let (x, y) = (&a[i], &b[i]);
        if x.kind != y.kind || x.est_hz != y.est_hz || x.sample_count != y.sample_count
            || x.elapsed_ns != y.elapsed_ns || x.mono_ok != y.mono_ok || x.weight != y.weight
        {
            return false;
        }
        i += 1;
    }
    true
}

// ---------------------------------------------------------------------------
// 十五、行为等价面（与 TscCalibration 参照面 100% 对拍自证）
// ---------------------------------------------------------------------------

/// 参照面源档（逐字镜像 cpu/clock.rs `CalibSource` 语义的闭集）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LegacySource {
    /// Fallback：无固件数据。
    Fallback,
    /// Measured：PIT/HPET 实测。
    Measured,
    /// CpuLeaf16：CPUID leaf 0x16 标称频率。
    CpuLeaf16,
    /// CpuLeaf15：CPUID leaf 0x15 晶振比——最佳答案。
    CpuLeaf15,
    /// Invalid：leaf 0x15 分母为 0——不可用。
    Invalid,
}

/// 参照面校准记录（逐字镜像 cpu/clock.rs `TscCalibration` 字段面）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LegacyCalib {
    /// 校准频率（Hz）。
    pub hz: u64,
    /// 源档。
    pub source: LegacySource,
    /// Invariant TSC 标记（透传）。
    pub invariant: bool,
}

impl LegacyCalib {
    /// FALLBACK_HZ = 2.5 GHz（镜像 cpu/clock.rs TscCalibration::FALLBACK_HZ）。
    pub const FALLBACK_HZ: u64 = 2_500_000_000;

    /// 逐字语义转录（cpu/clock.rs TscCalibration::choose，行 72–105）：
    /// leaf15（分母非零；crystal 零缺省 25MHz）→ leaf16（非零 MHz）→
    /// measured（非零）→ fallback。leaf15 分母为零直接 Invalid(hz=0)。
    pub fn choose(
        leaf15: Option<(u32, u32, u32)>,
        leaf16: Option<u32>,
        measured: Option<u64>,
        invariant: bool,
    ) -> LegacyCalib {
        if let Some((denom, numer, crystal)) = leaf15 {
            if denom != 0 {
                let crystal = if crystal != 0 { crystal as u64 } else { 25_000_000 };
                let hz = crystal * numer as u64 / denom as u64;
                return LegacyCalib { hz, source: LegacySource::CpuLeaf15, invariant };
            }
            return LegacyCalib { hz: 0, source: LegacySource::Invalid, invariant };
        }
        if let Some(mhz) = leaf16 {
            if mhz != 0 {
                return LegacyCalib {
                    hz: mhz as u64 * 1_000_000,
                    source: LegacySource::CpuLeaf16,
                    invariant,
                };
            }
        }
        if let Some(hz) = measured {
            if hz != 0 {
                return LegacyCalib { hz, source: LegacySource::Measured, invariant };
            }
        }
        LegacyCalib { hz: Self::FALLBACK_HZ, source: LegacySource::Fallback, invariant }
    }

    /// tsc→ns（镜像语义：hz==0 → 0，否则 muldiv(tsc, 1e9, hz)）。
    pub fn tsc_to_ns(&self, tsc: u64) -> u64 {
        if self.hz == 0 {
            return 0;
        }
        legacy_muldiv(tsc, 1_000_000_000, self.hz)
    }

    /// ns→tsc（镜像语义：hz==0 → 0，否则 muldiv(ns, hz, 1e9)）。
    pub fn ns_to_tsc(&self, ns: u64) -> u64 {
        if self.hz == 0 {
            return 0;
        }
        legacy_muldiv(ns, self.hz, 1_000_000_000)
    }
}

/// 参照面 muldiv（逐字镜像 cpu/clock.rs muldiv_u64：128 位中间量，
/// div=0 → 0，越 u64::MAX 饱和）。
pub fn legacy_muldiv(v: u64, mul: u64, div: u64) -> u64 {
    let v = v as u128;
    let mul = mul as u128;
    let div = div as u128;
    if div == 0 {
        return 0;
    }
    let r = (v * mul) / div;
    if r > u64::MAX as u128 {
        u64::MAX
    } else {
        r as u64
    }
}

/// 本模块决策档（与 LegacySource 同构闭集）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PickTier {
    /// Fallback。
    Fallback,
    /// Measured。
    Measured,
    /// CpuLeaf16。
    CpuLeaf16,
    /// CpuLeaf15。
    CpuLeaf15,
    /// Invalid。
    Invalid,
}

/// 本模块决策记录。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CalibPick {
    /// 校准频率（Hz）。
    pub hz: u64,
    /// 决策档。
    pub tier: PickTier,
    /// Invariant 透传。
    pub invariant: bool,
}

impl CalibPick {
    /// 与参照面同类判（档位+频率+invariant 全等）。
    pub fn same_as(&self, o: &LegacyCalib) -> bool {
        self.hz == o.hz
            && self.invariant == o.invariant
            && self.tier_class() == o.source as u8
    }

    /// 档位类序（与 LegacySource 变体序对齐：Fallback=0/Measured=1/
    /// CpuLeaf16=2/CpuLeaf15=3/Invalid=4）。
    pub const fn tier_class(&self) -> u8 {
        match self.tier {
            PickTier::Fallback => LegacySource::Fallback as u8,
            PickTier::Measured => LegacySource::Measured as u8,
            PickTier::CpuLeaf16 => LegacySource::CpuLeaf16 as u8,
            PickTier::CpuLeaf15 => LegacySource::CpuLeaf15 as u8,
            PickTier::Invalid => LegacySource::Invalid as u8,
        }
    }
}

/// 异构决策实现：候选表 + 秩扫描（非参照面的 if 链——等价对拍的
/// 意义在于两条不同实现路径收敛同一语义）。
/// 候选表：rank0=fallback（常在）/rank1=measured（非零）/rank2=leaf16
/// （非零 MHz）/rank3=leaf15（分母非零；crystal 零缺省 25MHz）；
/// leaf15 分母为零 → Invalid 锁（终态压倒一切，语义同参照面短路返回）。
pub fn decide_by_table(
    leaf15: Option<(u32, u32, u32)>,
    leaf16: Option<u32>,
    measured: Option<u64>,
    invariant: bool,
) -> CalibPick {
    // Invalid 锁：leaf15 在场且分母为零。
    if let Some((denom, _, _)) = leaf15 {
        if denom == 0 {
            return CalibPick { hz: 0, tier: PickTier::Invalid, invariant };
        }
    }
    // 候选表（rank, hz, tier）——fallback 常在殿后。
    let mut cands: [(u8, u64, PickTier); 4] = [
        (0, LegacyCalib::FALLBACK_HZ, PickTier::Fallback),
        (0, 0, PickTier::Fallback),
        (0, 0, PickTier::Fallback),
        (0, 0, PickTier::Fallback),
    ];
    let mut filled: [bool; 4] = [true, false, false, false];
    if let Some(hz) = measured {
        if hz != 0 {
            cands[1] = (1, hz, PickTier::Measured);
            filled[1] = true;
        }
    }
    if let Some(mhz) = leaf16 {
        if mhz != 0 {
            cands[2] = (2, (mhz as u64).saturating_mul(1_000_000), PickTier::CpuLeaf16);
            filled[2] = true;
        }
    }
    if let Some((denom, numer, crystal)) = leaf15 {
        if denom != 0 {
            let crystal = if crystal != 0 { crystal as u64 } else { 25_000_000 };
            let hz = crystal.saturating_mul(numer as u64) / denom as u64;
            cands[3] = (3, hz, PickTier::CpuLeaf15);
            filled[3] = true;
        }
    }
    // 秩扫描：取 filled 候选中秩最高者（while+索引，序稳定）。
    let mut best_rank: u8 = 0;
    let mut best: Option<(u64, PickTier)> = Some((LegacyCalib::FALLBACK_HZ, PickTier::Fallback));
    let mut i = 0usize;
    while i < 4 {
        if filled[i] {
            let (rank, hz, tier) = cands[i];
            if rank >= best_rank {
                best_rank = rank;
                best = Some((hz, tier));
            }
        }
        i += 1;
    }
    match best {
        Some((hz, tier)) => CalibPick { hz, tier, invariant },
        None => CalibPick { hz: LegacyCalib::FALLBACK_HZ, tier: PickTier::Fallback, invariant },
    }
}

// ---------------------------------------------------------------------------
// 十六、等价对拍探针（120 组合全对拍 + muldiv 双实现对扫）
// ---------------------------------------------------------------------------

/// 等价对拍报告。
#[derive(Debug, Clone)]
pub struct EquivReport {
    /// 对拍组合数。
    pub cases: u32,
    /// 同输入同输出组合数。
    pub matched: u32,
    /// 等价率（百分比整型）。
    pub rate_pct: u64,
    /// muldiv 换算对拍数（tsc_to_ns / ns_to_tsc 两向）。
    pub muldiv_cases: u32,
    /// muldiv 对拍一致数。
    pub muldiv_matched: u32,
    /// 等价率 100% 判据（choose 对拍与 muldiv 对拍皆满）。
    pub full_equivalent: bool,
    /// 首个相异组合（leaf15, leaf16, measured, invariant 序号化的注记）。
    pub first_diff: Option<String>,
}

/// choose 决策对拍组合全矩阵：leaf15 五态 × leaf16 四态 × measured 三态 ×
/// invariant 两态 = 120 组合（含分母零/晶振零/MHz 零/measured 零等边界）。
fn choose_matrix() -> Vec<(Option<(u32, u32, u32)>, Option<u32>, Option<u64>, bool)> {
    let leaf15s: [Option<(u32, u32, u32)>; 5] = [
        None,
        Some((0, 0, 0)),              // 分母零 → Invalid 终态
        Some((2, 200, 25_000_000)),   // 典型：25M×200/2 = 2.5GHz
        Some((2, 200, 0)),            // 晶振零 → 缺省 25MHz
        Some((243, 32_000, 38_400_000)),
    ];
    let leaf16s: [Option<u32>; 4] = [None, Some(0), Some(2600), Some(3000)];
    let measureds: [Option<u64>; 3] = [None, Some(0), Some(1_800_000_000)];
    let invs: [bool; 2] = [true, false];
    let mut out: Vec<(Option<(u32, u32, u32)>, Option<u32>, Option<u64>, bool)> = Vec::new();
    let mut a = 0usize;
    while a < leaf15s.len() {
        let mut b = 0usize;
        while b < leaf16s.len() {
            let mut c = 0usize;
            while c < measureds.len() {
                let mut d = 0usize;
                while d < invs.len() {
                    out.push((leaf15s[a], leaf16s[b], measureds[c], invs[d]));
                    d += 1;
                }
                c += 1;
            }
            b += 1;
        }
        a += 1;
    }
    out
}

/// muldiv 换算对拍输入面（tsc 值域闭集：零/一/小值/大值/近满幅）。
const MULDIV_TSC_SAMPLES: [u64; 7] = [
    0,
    1,
    1_000,
    12_345_678,
    2_600_000_000,
    4_000_000_000_000,
    u64::MAX / 4096,
];

/// muldiv 频率对拍面（含 Fallback 2.5GHz / Invalid 0Hz 边界）。
const MULDIV_HZ_SAMPLES: [u64; 6] = [
    0,
    25_000_000,
    1_193_182,
    1_800_000_000,
    2_500_000_000,
    2_600_000_000,
];

/// 等价对拍探针：参照面（逐字转录 `TscCalibration::choose` 语义）与本模块
/// 异构实现（候选表+秩扫描）对 120 组合全对拍；tsc_to_ns/ns_to_tsc 双实现
/// 对扫。等价率必须 100%（判据：与现存 `TscCalibration` 行为等价率 100%）。
pub fn equivalence_probe() -> Result<EquivReport, TscCalError> {
    let matrix = choose_matrix();
    let mut matched: u32 = 0;
    let mut first_diff: Option<String> = None;
    for (leaf15, leaf16, measured, inv) in &matrix {
        let legacy = LegacyCalib::choose(*leaf15, *leaf16, *measured, *inv);
        let pick = decide_by_table(*leaf15, *leaf16, *measured, *inv);
        if pick.same_as(&legacy) {
            matched = matched.saturating_add(1);
        } else if first_diff.is_none() {
            first_diff = Some(format!(
                "leaf15={:?} leaf16={:?} measured={:?} inv={} → legacy={:?} vs pick={:?}",
                leaf15, leaf16, measured, inv, legacy, pick
            ));
        }
    }
    // muldiv 双向对扫：参照面 legacy_muldiv vs 本模块 muldiv_sat。
    let mut md_cases: u32 = 0;
    let mut md_matched: u32 = 0;
    let mut t = 0usize;
    while t < MULDIV_TSC_SAMPLES.len() {
        let mut h = 0usize;
        while h < MULDIV_HZ_SAMPLES.len() {
            let tsc = MULDIV_TSC_SAMPLES[t];
            let hz = MULDIV_HZ_SAMPLES[h];
            // tsc→ns。
            let l_ns = legacy_muldiv(tsc, 1_000_000_000, hz);
            let m_ns = muldiv_sat(tsc, 1_000_000_000, hz);
            md_cases = md_cases.saturating_add(1);
            if l_ns == m_ns {
                md_matched = md_matched.saturating_add(1);
            }
            // ns→tsc。
            let l_tick = legacy_muldiv(tsc, hz, 1_000_000_000);
            let m_tick = muldiv_sat(tsc, hz, 1_000_000_000);
            md_cases = md_cases.saturating_add(1);
            if l_tick == m_tick {
                md_matched = md_matched.saturating_add(1);
            }
            h += 1;
        }
        t += 1;
    }
    let cases = matrix.len() as u32;
    let rate = muldiv_sat((matched.saturating_mul(100)) as u64, 1, cases.max(1) as u64);
    let md_ok = md_matched == md_cases;
    Ok(EquivReport {
        cases,
        matched,
        rate_pct: rate,
        muldiv_cases: md_cases,
        muldiv_matched: md_matched,
        full_equivalent: matched == cases && md_ok,
        first_diff,
    })
}

/// 行为等价对拍 + 等价面事件落账（供 CrossCalibrator 事件账消费）。
pub fn equivalence_checked_event() -> Result<(EquivReport, EventRecord), TscCalError> {
    let rep = equivalence_probe()?;
    if !rep.full_equivalent {
        return Err(TscCalError::EquivalenceFail(rep.matched, rep.cases));
    }
    let rec = EventRecord {
        seq: 0,
        kind: EventKind::EquivalenceChecked,
        round: CC_ROUND_NONE,
        text: format!("行为等价对拍：{}/{} 组合 + {}/{} muldiv，等价率 {}%",
            rep.matched, rep.cases, rep.muldiv_matched, rep.muldiv_cases, rep.rate_pct),
    };
    Ok((rep, rec))
}

// ---------------------------------------------------------------------------
// 十七、双跑确定性对拍 + 注入面（三案例闭集）
// ---------------------------------------------------------------------------

/// 双跑对拍报告。
#[derive(Debug, Clone)]
pub struct DualRunReport {
    /// A 跑指纹。
    pub fp_a: u64,
    /// B 跑指纹。
    pub fp_b: u64,
    /// 全中率（A 跑口径）。
    pub all_hit_rate_pct: u64,
    /// 一致判据：指纹相等。
    pub consistent: bool,
}

/// 双跑确定性对拍：同构型独立装配两次完整校准，指纹必须逐位相等。
/// 校准全程无墙钟、无随机、源模型纯函数——指纹相异即确定性失守。
pub fn dual_run(cfg: &SourceConfig) -> Result<DualRunReport, TscCalError> {
    let mut a = assemble(cfg)?;
    let mut b = assemble(cfg)?;
    let ledger_a = a.run_all(&cfg.view)?;
    let ledger_b = b.run_all(&cfg.view)?;
    let fp_a = ledger_a.fingerprint();
    let fp_b = ledger_b.fingerprint();
    if fp_a != fp_b {
        return Err(TscCalError::DualRunMismatch);
    }
    Ok(DualRunReport {
        fp_a,
        fp_b,
        all_hit_rate_pct: ledger_a.all_hit_rate_pct(),
        consistent: true,
    })
}

/// 指纹稳定性：同账二次指纹必须相等（FingerprintDrift 防线）。
pub fn fingerprint_stable(ledger: &CalibLedger) -> Result<u64, TscCalError> {
    let f1 = ledger.fingerprint();
    let f2 = ledger.fingerprint();
    if f1 != f2 {
        return Err(TscCalError::FingerprintDrift);
    }
    Ok(f1)
}

/// 注入案例闭集（三案例）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InjectCase {
    /// 离群源注入：HPET 时基慢 20000ppm（2%）→ 三源互差越容差 →
    /// 发散检出 + 离群源显性降权 + 中位数裁决保真。
    OutlierSource,
    /// 单调破注入：MSR 源窗内样本位 8 回退 → MonoBroken 判决留账。
    MonoBreak,
    /// 窗口不足注入：窗长 0.5ms（低于 1ms 下限）→ SampleShort 立案。
    WindowShort,
}

impl InjectCase {
    /// 全量闭集。
    pub const ALL: [InjectCase; 3] = [InjectCase::OutlierSource, InjectCase::MonoBreak, InjectCase::WindowShort];

    /// wire 名。
    pub const fn name(self) -> &'static str {
        match self {
            InjectCase::OutlierSource => "outlier_source",
            InjectCase::MonoBreak => "mono_break",
            InjectCase::WindowShort => "window_short",
        }
    }
}

/// 注入结果。
#[derive(Debug, Clone)]
pub struct InjectOutcome {
    /// 案例。
    pub case: InjectCase,
    /// 检出判据是否命中。
    pub detected: bool,
    /// 关联诊断码。
    pub code: CcCode,
    /// 人话注记。
    pub note: String,
}

/// 注入面套件：三案例逐个注入，检出判据闭集核对，任一逃逸即
/// InjectUndetected 立案（判据"注入检出"的自动化面）。
pub fn run_injection_suite() -> Result<Vec<InjectOutcome>, TscCalError> {
    let mut out: Vec<InjectOutcome> = Vec::with_capacity(3);
    // 案例一：离群源注入（HPET 时基慢 20000ppm）。
    {
        let cfg = SourceConfig { msr_propagate: false, hpet_outlier_ppm: 20_000, view: TscView::synth() };
        let mut cal = assemble(&cfg)?;
        let ledger = cal.run_all(&cfg.view)?;
        let diverge_rounds = ledger.rounds.iter().filter(|r| r.verdict == RoundVerdict::Diverge).count();
        let hpet_downweighted = ledger.rounds.iter().any(|r| {
            r.outliers.contains(&SourceKind::Hpet)
                && r.legs.iter().any(|l| l.kind == SourceKind::Hpet && l.weight == CC_WEIGHT_DOWN)
        });
        // 中位数裁决保真：离群在场时终局频率仍须落在真值 1000ppm 内。
        let final_hz = ledger.final_hz()?;
        let median_holds = ppm_dev(final_hz, CC_TRUE_TSC_HZ) < CC_TOLERANCE_PPM;
        let detected = diverge_rounds > 0 && hpet_downweighted && median_holds;
        out.push(InjectOutcome {
            case: InjectCase::OutlierSource,
            detected,
            code: CcCode::CrossDiverge,
            note: format!(
                "HPET 时基 +20000ppm 注入：发散轮 {}，离群降权留账={}，中位数裁决保真（{}Hz 对真值偏差 {}ppm）",
                diverge_rounds, hpet_downweighted, final_hz, ppm_dev(final_hz, CC_TRUE_TSC_HZ)
            ),
        });
    }
    // 案例二：单调破注入（MSR 源样本位 8 回退）。
    {
        let mut cal = CrossCalibrator::new();
        cal.attach(LegSlot::Msr, Box::new(MsrSource::new(TscView::synth()).with_monobreak(8)))?;
        cal.attach(LegSlot::Hpet, Box::new(HpetSource::new()))?;
        cal.attach(LegSlot::Ref, Box::new(RefSource::new()))?;
        let ledger = cal.run_all(&TscView::synth())?;
        let mono_rounds = ledger.rounds.iter().filter(|r| r.verdict == RoundVerdict::MonoBroken).count();
        let ev = ledger.events.iter().any(|e| e.kind == EventKind::MonoBreakDetected);
        let detected = mono_rounds > 0 && ev;
        out.push(InjectOutcome {
            case: InjectCase::MonoBreak,
            detected,
            code: CcCode::MonoBroken,
            note: format!("MSR 样本位 8 回退注入：MonoBroken 轮 {}，事件留账={}", mono_rounds, ev),
        });
    }
    // 案例三：窗口不足注入（窗长 0.5ms < 1ms 下限）。
    {
        let short = SampleWindow::new(0, CC_MIN_WINDOW_NS / 2, CC_SAMPLES_DEFAULT);
        let detected = match short {
            Err(TscCalError::WindowBudget) => true,
            _ => false,
        };
        out.push(InjectOutcome {
            case: InjectCase::WindowShort,
            detected,
            code: CcCode::WindowBudget,
            note: format!("0.5ms 窗注入：SampleWindow::new 拒收={}", detected),
        });
    }
    // 逃逸核对。
    for (i, o) in out.iter().enumerate() {
        if !o.detected {
            return Err(TscCalError::InjectUndetected(i as u8));
        }
    }
    Ok(out)
}

// ---------------------------------------------------------------------------
// 十八、健康面（八项检查）+ 启动报告导出
// ---------------------------------------------------------------------------

/// 健康检查项名闭集（八项）。
pub const CC_HEALTH_CHECKS: [&str; 8] = [
    "three_legs_complete",
    "rounds_complete",
    "window_discipline",
    "monotonic_all_green",
    "cross_within_tolerance",
    "all_hit_rate_100",
    "replay_consistent_100",
    "fingerprint_stable",
];

/// 健康报告。
#[derive(Debug, Clone)]
pub struct HealthReport {
    /// 得分（0..=100，通过项 × 100 ÷ 8）。
    pub score: u8,
    /// 逐项结果（名, 通过）。
    pub checks: Vec<(&'static str, bool)>,
    /// 降级判据：任一项未过。
    pub degraded: bool,
}

/// 健康度量：对完整校准面（账 + 回放 + 注入 + 等价）出八项检查。
pub fn health(
    ledger: &CalibLedger,
    _cfg: &SourceConfig,
    replay_rep: &ReplayReport,
    inj: &[InjectOutcome],
    equiv: &EquivReport,
) -> Result<HealthReport, TscCalError> {
    let legs_complete = ledger.rounds.iter().all(|r| r.legs.len() == 3);
    let rounds_complete = ledger.round_count() == CC_ROUNDS;
    let window_discipline = ledger
        .rounds
        .iter()
        .all(|r| r.window_ns >= CC_MIN_WINDOW_NS && r.window_ns <= CC_WINDOW_NS
            && r.samples >= CC_SAMPLES_MIN && r.samples <= CC_SAMPLES_MAX);
    let mono_green = ledger.rounds.iter().all(|r| r.verdict != RoundVerdict::MonoBroken && r.legs.iter().all(|l| l.mono_ok));
    let within = ledger.within_tolerance();
    let all_hit = ledger.all_hit_rate_pct() == 100;
    let replay_ok = replay_rep.consistent && replay_rep.match_rate_pct == 100;
    let fp_ok = fingerprint_stable(ledger).is_ok();
    let inj_ok = inj.len() == 3 && inj.iter().all(|o| o.detected);
    let checks: Vec<(&'static str, bool)> = vec![
        (CC_HEALTH_CHECKS[0], legs_complete),
        (CC_HEALTH_CHECKS[1], rounds_complete),
        (CC_HEALTH_CHECKS[2], window_discipline),
        (CC_HEALTH_CHECKS[3], mono_green),
        (CC_HEALTH_CHECKS[4], within),
        (CC_HEALTH_CHECKS[5], all_hit),
        (CC_HEALTH_CHECKS[6], replay_ok && inj_ok && equiv.full_equivalent),
        (CC_HEALTH_CHECKS[7], fp_ok),
    ];
    let passed = checks.iter().filter(|(_, ok)| *ok).count() as u64;
    let score = muldiv_sat(passed.saturating_mul(100), 1, checks.len().max(1) as u64) as u8;
    let degraded = passed < checks.len() as u64;
    Ok(HealthReport { score, checks, degraded })
}

/// 启动报告必备键闭集（导出面对账基准）。
pub fn export_keys() -> [&'static str; 12] {
    [
        "tscal_version",
        "tscal_seg_base",
        "tscal_median_hz",
        "tscal_tolerance_ppm",
        "tscal_rounds",
        "tscal_all_hit_rate_pct",
        "tscal_window_ns",
        "tscal_samples",
        "tscal_replay_match_pct",
        "tscal_equiv_rate_pct",
        "tscal_injections_detected",
        "tscal_fingerprint",
    ]
}

/// 启动报告导出（kv 闭集；缺键即 ReportKv 立案）。
pub fn export_boot_report(
    ledger: &CalibLedger,
    replay_rep: &ReplayReport,
    equiv: &EquivReport,
    inj: &[InjectOutcome],
) -> Result<Vec<(&'static str, String)>, TscCalError> {
    let median = ledger.final_hz()?;
    let detected = inj.iter().filter(|o| o.detected).count();
    let kv: Vec<(&'static str, String)> = vec![
        ("tscal_version", String::from(CC_VERSION)),
        ("tscal_seg_base", format!("{:#06x}", CC_SEG_BASE)),
        ("tscal_median_hz", format!("{}", median)),
        ("tscal_tolerance_ppm", format!("{}", CC_TOLERANCE_PPM)),
        ("tscal_rounds", format!("{}", ledger.round_count())),
        ("tscal_all_hit_rate_pct", format!("{}", ledger.all_hit_rate_pct())),
        ("tscal_window_ns", format!("{}", CC_ROUND_WINDOW_NS)),
        ("tscal_samples", format!("{}", CC_SAMPLES_DEFAULT)),
        ("tscal_replay_match_pct", format!("{}", replay_rep.match_rate_pct)),
        ("tscal_equiv_rate_pct", format!("{}", equiv.rate_pct)),
        ("tscal_injections_detected", format!("{}/3", detected)),
        ("tscal_fingerprint", format!("{:016x}", ledger.fingerprint())),
    ];
    for key in export_keys() {
        let mut found = false;
        for (k, v) in &kv {
            if *k == key {
                if v.is_empty() {
                    return Err(TscCalError::ReportKv(key));
                }
                found = true;
            }
        }
        if !found {
            return Err(TscCalError::ReportKv(key));
        }
    }
    Ok(kv)
}

/// 一站式管线：装配→全量校准→回放→注入→等价→健康→报告，全绿才签发。
pub fn run_pipeline(cfg: &SourceConfig) -> Result<PipelineOutput, TscCalError> {
    let mut cal = assemble(cfg)?;
    let ledger = cal.run_all(&cfg.view)?;
    let replay_rep = replay(&ledger, cfg)?;
    if !replay_rep.consistent {
        return Err(TscCalError::ReplayMismatch(replay_rep.matched, replay_rep.rounds));
    }
    let inj = run_injection_suite()?;
    let (equiv, _) = equivalence_checked_event()?;
    let hp = health(&ledger, cfg, &replay_rep, &inj, &equiv)?;
    if hp.degraded {
        return Err(TscCalError::HealthDegraded(hp.score));
    }
    let kv = export_boot_report(&ledger, &replay_rep, &equiv, &inj)?;
    let dual = dual_run(cfg)?;
    if !dual.consistent {
        return Err(TscCalError::DualRunMismatch);
    }
    Ok(PipelineOutput { ledger, replay_rep, inj, equiv, health: hp, kv, dual })
}

/// 一站式管线产物。
#[derive(Debug, Clone)]
pub struct PipelineOutput {
    /// 校准账。
    pub ledger: CalibLedger,
    /// 回放报告。
    pub replay_rep: ReplayReport,
    /// 注入结果闭集。
    pub inj: Vec<InjectOutcome>,
    /// 等价报告。
    pub equiv: EquivReport,
    /// 健康报告。
    pub health: HealthReport,
    /// 启动报告 kv。
    pub kv: Vec<(&'static str, String)>,
    /// 双跑对拍。
    pub dual: DualRunReport,
}

// ---------------------------------------------------------------------------
// 十九、测试面（冒烟：互差容差/全中率/回放/等价/注入/双跑/健康/守卫）
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    /// 冒烟一：干净平台全量校准——互差 <0.1%、三源全中率 100%。
    #[test]
    fn smoke_cross_within_tolerance_all_hit() {
        let cfg = SourceConfig::clean();
        let mut cal = assemble(&cfg).expect("assemble clean");
        let ledger = cal.run_all(&cfg.view).expect("run_all clean");
        assert_eq!(ledger.round_count(), CC_ROUNDS);
        assert!(ledger.within_tolerance(), "ledger: {}", ledger.digest());
        assert_eq!(ledger.all_hit_rate_pct(), 100);
        assert!(ledger.max_ppm_overall() < CC_TOLERANCE_PPM);
        // 三源全中率 100% 判据的逐轮核对。
        for r in &ledger.rounds {
            assert!(r.all_hit(), "round {} not all_hit: {:?}", r.round, r.verdict);
            for l in &r.legs {
                assert!(l.mono_ok);
                assert_eq!(l.weight, CC_WEIGHT_FULL);
                assert_eq!(l.sample_count, CC_SAMPLES_DEFAULT);
            }
        }
        // 终局频率对真值 <1000ppm。
        let final_hz = ledger.final_hz().expect("final_hz");
        assert!(ppm_dev(final_hz, CC_TRUE_TSC_HZ) < CC_TOLERANCE_PPM, "final {} vs true {}", final_hz, CC_TRUE_TSC_HZ);
        assert!(ledger.codes.is_empty(), "clean run must be code-free: {:?}", ledger.codes);
    }

    /// 冒烟二：Propagate 模式（MSR 腿比率推算）同样全中。
    #[test]
    fn smoke_propagate_mode_all_hit() {
        let cfg = SourceConfig { msr_propagate: true, hpet_outlier_ppm: 0, view: TscView::synth() };
        let mut cal = assemble(&cfg).expect("assemble");
        let ledger = cal.run_all(&cfg.view).expect("run_all");
        assert_eq!(ledger.all_hit_rate_pct(), 100);
        // 比率推算声明值 = 26 × 100MHz = 2.6GHz = 真值（互差 0）。
        let r0 = &ledger.rounds[0];
        let prop = r0.legs.iter().find(|l| l.kind == SourceKind::Propagate).expect("propagate leg");
        assert_eq!(prop.est_hz, CC_MAX_NON_TURBO_RATIO * CC_BCLK_HZ);
        let final_hz = ledger.final_hz().expect("final_hz");
        assert!(ppm_dev(final_hz, CC_TRUE_TSC_HZ) < CC_TOLERANCE_PPM);
    }

    /// 冒烟三：校准账回放——逐轮重演字段级对拍一致率 100%。
    #[test]
    fn smoke_replay_100pct() {
        let cfg = SourceConfig::clean();
        let mut cal = assemble(&cfg).expect("assemble");
        let ledger = cal.run_all(&cfg.view).expect("run_all");
        let rep = replay(&ledger, &cfg).expect("replay");
        assert!(rep.consistent, "replay mismatch at {:?}", rep.first_mismatch);
        assert_eq!(rep.matched, rep.rounds);
        assert_eq!(rep.match_rate_pct, 100);
    }

    /// 冒烟四：行为等价——与 TscCalibration 参照面 120 组合 + muldiv 双向 100%。
    #[test]
    fn smoke_equivalence_100pct() {
        let rep = equivalence_probe().expect("equivalence");
        assert_eq!(rep.cases, 120);
        assert_eq!(rep.matched, rep.cases, "first_diff={:?}", rep.first_diff);
        assert_eq!(rep.rate_pct, 100);
        assert_eq!(rep.muldiv_matched, rep.muldiv_cases);
        assert!(rep.full_equivalent);
    }

    /// 冒烟五：注入面三案例 3/3 检出（离群源/单调破/窗口不足）。
    #[test]
    fn smoke_injections_all_detected() {
        let outs = run_injection_suite().expect("injection suite");
        assert_eq!(outs.len(), 3);
        for o in &outs {
            assert!(o.detected, "case {} escaped: {}", o.case.name(), o.note);
        }
        // 离群源注入：中位数裁决保真（离群不弃但降权，结论不被污染）。
        let outlier = &outs[0];
        assert!(outlier.note.contains("保真"), "outlier note: {}", outlier.note);
    }

    /// 冒烟六：双跑确定性——同构两次完整校准指纹逐位相等。
    #[test]
    fn smoke_dual_run_deterministic() {
        let cfg = SourceConfig::clean();
        let rep = dual_run(&cfg).expect("dual_run");
        assert!(rep.consistent);
        assert_eq!(rep.fp_a, rep.fp_b);
        assert_eq!(rep.all_hit_rate_pct, 100);
    }

    /// 冒烟七：指纹稳定 + 健康满分 + 报告键闭集。
    #[test]
    fn smoke_health_and_report() {
        let cfg = SourceConfig::clean();
        let mut cal = assemble(&cfg).expect("assemble");
        let ledger = cal.run_all(&cfg.view).expect("run_all");
        let fp = fingerprint_stable(&ledger).expect("fingerprint stable");
        assert_eq!(fp, ledger.fingerprint());
        let replay_rep = replay(&ledger, &cfg).expect("replay");
        let inj = run_injection_suite().expect("injections");
        let equiv = equivalence_probe().expect("equivalence");
        let hp = health(&ledger, &cfg, &replay_rep, &inj, &equiv).expect("health");
        assert!(!hp.degraded, "checks: {:?}", hp.checks);
        assert_eq!(hp.score, 100);
        let kv = export_boot_report(&ledger, &replay_rep, &equiv, &inj).expect("report");
        assert_eq!(kv.len(), export_keys().len());
        // 一站式管线全绿签发。
        let out = run_pipeline(&cfg).expect("pipeline");
        assert!(!out.health.degraded);
        assert!(out.dual.consistent);
        assert!(out.equiv.full_equivalent);
        assert!(out.ledger.within_tolerance());
    }

    /// 冒烟八：诊断码段守卫 + 码值闭集 + detail 面非空。
    #[test]
    fn smoke_segment_guard_and_details() {
        assert!(segment_guard());
        assert_eq!(CcCode::ALL.len(), 16);
        assert_eq!(CcCode::SourceAbsent.val(), 0x1B10);
        assert_eq!(CcCode::SegmentGuard.val(), 0x1B1F);
        for c in CcCode::ALL {
            assert!(c.in_segment());
            assert!(!c.text().is_empty());
            assert!(!c.advice().is_empty());
        }
        let errs = [
            TscCalError::SourceAbsent,
            TscCalError::SampleShort,
            TscCalError::WindowBudget,
            TscCalError::MonoBroken,
            TscCalError::EstimateZero,
            TscCalError::CrossDiverge(9999),
            TscCalError::MedianUnresolved,
            TscCalError::LedgerBreak,
            TscCalError::ReplayMismatch(0, 12),
            TscCalError::EquivalenceFail(119, 120),
            TscCalError::InjectUndetected(2),
            TscCalError::FingerprintDrift,
            TscCalError::DualRunMismatch,
            TscCalError::HealthDegraded(87),
            TscCalError::ReportKv("tscal_median_hz"),
            TscCalError::SegmentGuard,
        ];
        for e in errs {
            assert!(!e.detail().is_empty(), "empty detail for {:?}", e);
            assert!(e.code().in_segment());
        }
    }

    /// 冒烟九：窗口纪律——窗长/样本越界拒收，缺腿拒收，腿位错挂拒收。
    #[test]
    fn smoke_window_and_leg_discipline() {
        // 窗长越下限。
        assert!(SampleWindow::new(0, CC_MIN_WINDOW_NS - 1, CC_SAMPLES_DEFAULT).is_err());
        // 窗长越上限。
        assert!(SampleWindow::new(0, CC_WINDOW_NS + 1, CC_SAMPLES_DEFAULT).is_err());
        // 样本越下限。
        assert!(SampleWindow::new(0, CC_ROUND_WINDOW_NS, CC_SAMPLES_MIN - 1).is_err());
        // 样本越上限。
        assert!(SampleWindow::new(0, CC_ROUND_WINDOW_NS, CC_SAMPLES_MAX + 1).is_err());
        // 合法边界。
        assert!(SampleWindow::new(0, CC_MIN_WINDOW_NS, CC_SAMPLES_MIN).is_ok());
        assert!(SampleWindow::new(0, CC_WINDOW_NS, CC_SAMPLES_MAX).is_ok());
        // 缺腿拒收。
        let cfg = SourceConfig::clean();
        let mut cal = CrossCalibrator::new();
        assert!(matches!(cal.run_round(0, &cfg.view), Err(TscCalError::SourceAbsent)));
        // 腿位错挂拒收（HPET 源挂 MSR 腿）。
        let mut cal2 = CrossCalibrator::new();
        assert!(cal2.attach(LegSlot::Msr, Box::new(HpetSource::new())).is_err());
        assert!(cal2.attach(LegSlot::Hpet, Box::new(RefSource::new())).is_err());
        assert!(cal2.attach(LegSlot::Ref, Box::new(MsrSource::new(TscView::synth()))).is_err());
        // 正确挂装全收（MSR 腿两模式互斥占位——后挂覆盖前挂，腿位校验均放行）。
        let mut cal3 = CrossCalibrator::new();
        assert!(cal3.attach(LegSlot::Msr, Box::new(MsrSource::new(TscView::synth()))).is_ok());
        assert!(cal3.attach(LegSlot::Msr, Box::new(PropagateSource::new())).is_ok());
        assert!(cal3.attach(LegSlot::Hpet, Box::new(HpetSource::new())).is_ok());
        assert!(cal3.attach(LegSlot::Ref, Box::new(RefSource::new())).is_ok());
        assert!(cal3.legs_complete());
    }

    /// 冒烟十：单调性校验 + 中位数原语 + ppm 原语 + 窗口铺点单调。
    #[test]
    fn smoke_primitives() {
        // 单调性。
        let ok = vec![
            SourceSample { at_ns: 0, ticks: 10 },
            SourceSample { at_ns: 10, ticks: 10 },
            SourceSample { at_ns: 20, ticks: 12 },
        ];
        assert!(samples_monotonic(&ok));
        let bad = vec![
            SourceSample { at_ns: 0, ticks: 10 },
            SourceSample { at_ns: 10, ticks: 3 },
        ];
        assert!(!samples_monotonic(&bad));
        // 中位数三值全序。
        assert_eq!(median3(1, 2, 3), 2);
        assert_eq!(median3(3, 2, 1), 2);
        assert_eq!(median3(2, 2, 9), 2);
        assert_eq!(median3(u64::MAX, 0, 5), 5);
        // 多值中位数（偶数取下中位）。
        assert_eq!(median_of_many(&[9, 1, 5]), 5);
        assert_eq!(median_of_many(&[9, 1, 5, 3]), 3);
        // ppm 偏差。
        assert_eq!(ppm_dev(2_600_000_000, 2_600_000_000), 0);
        // ppm 分母取大者口径：1e-3 相对偏差 = 999..=1000（整型地板）。
        assert_eq!(ppm_dev(1_000_000, 1_001_000), 999);
        assert!(ppm_dev(1_000_000, 1_002_000) > 1000);
        assert_eq!(ppm_dev(2_600_000_000, 2_600_000_000 + 2_600_000), 999); // 0.1% 档
        // 窗口铺点：起点、终点、单调。
        let w = SampleWindow::new(1_000, 10_000_000, 33).expect("window");
        assert_eq!(w.at(0), 1_000);
        assert_eq!(w.at(32), w.end_ns());
        let mut prev = 0u64;
        let mut i = 0u32;
        while i < 33 {
            let at = w.at(i);
            assert!(at >= prev);
            prev = at;
            i += 1;
        }
    }

    /// 冒烟十一：等价边界专项——分母零 Invalid 终态压倒 leaf16/measured。
    #[test]
    fn smoke_equivalence_invalid_terminal() {
        // leaf15 分母零 → Invalid(hz=0)，leaf16/measured 在场也不救。
        let l = LegacyCalib::choose(Some((0, 200, 25_000_000)), Some(2600), Some(2_000_000_000), true);
        let p = decide_by_table(Some((0, 200, 25_000_000)), Some(2600), Some(2_000_000_000), true);
        assert_eq!(l.hz, 0);
        assert_eq!(l.source, LegacySource::Invalid);
        assert!(p.same_as(&l));
        // crystal 零 → 缺省 25MHz。
        let l2 = LegacyCalib::choose(Some((2, 200, 0)), None, None, false);
        let p2 = decide_by_table(Some((2, 200, 0)), None, None, false);
        assert_eq!(l2.hz, 25_000_000 * 200 / 2);
        assert!(p2.same_as(&l2));
        // 全空 → fallback 2.5GHz。
        let l3 = LegacyCalib::choose(None, None, None, true);
        let p3 = decide_by_table(None, None, None, true);
        assert_eq!(l3.hz, LegacyCalib::FALLBACK_HZ);
        assert_eq!(l3.source, LegacySource::Fallback);
        assert!(p3.same_as(&l3));
        // hz=0 面 tsc_to_ns 镜像（0 保护）。
        assert_eq!(l.tsc_to_ns(1234), 0);
        assert_eq!(l.ns_to_tsc(1234), 0);
    }

    /// 冒烟十二：事件账序单调 + 发散事件留痕 + 离群降权权重面。
    #[test]
    fn smoke_event_ledger_and_downweight() {
        let cfg = SourceConfig { msr_propagate: false, hpet_outlier_ppm: 20_000, view: TscView::synth() };
        let mut cal = assemble(&cfg).expect("assemble");
        let ledger = cal.run_all(&cfg.view).expect("run_all");
        // 事件 seq 单调。
        let mut prev: Option<u64> = None;
        for ev in &ledger.events {
            if let Some(p) = prev {
                assert!(ev.seq > p);
            }
            prev = Some(ev.seq);
        }
        // 发散检出 + 离群降权留账。
        assert!(ledger.events.iter().any(|e| e.kind == EventKind::DivergeDetected));
        assert!(ledger.events.iter().any(|e| e.kind == EventKind::OutlierDownweighted));
        assert!(!ledger.rounds.iter().any(|r| r.all_hit()), "all rounds must be diverged under outlier");
        // 全中率 <100（离群在场），账面照常可审计。
        assert!(ledger.all_hit_rate_pct() < 100);
        assert!(ledger.self_audit().is_ok());
        // 中位数裁决保真：终局频率仍对真值 <1000ppm。
        let final_hz = ledger.final_hz().expect("final_hz");
        assert!(ppm_dev(final_hz, CC_TRUE_TSC_HZ) < CC_TOLERANCE_PPM);
        // 回放：含离群构型同样 100% 一致。
        let rep = replay(&ledger, &cfg).expect("replay");
        assert!(rep.consistent);
        assert_eq!(rep.match_rate_pct, 100);
    }
}
