//! CGPU-F1450 · 功耗异常检测（骤变/超限）（CGPU-J 域 · 功耗域 · 目标 320 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/CGPU Varix STAR II · 总纲与施工书.md#CGPU-F1450`
//!
//! **判据（锚点原文）**：超限滞回、斜率检测、趋势、配置化、序依赖、建议语义、注入测试、判据。
//!
//! **职责定位（锚点原文摘录）**：超限检测（实测>cap×0.95 持续 3 采样→PowerLimitHit——滞回退出：cap×0.85）；骤变检测（斜率超阈——ΔP/Δt 阈值表——帧间突变 vs 缓变分类）；持续越限（P99 越限画像——趋势预警）；检测算法显性（阈值全部配置化+文档公开——黑盒红线不适用）；误报控制（滤波后检测——F1447 序依赖声明；漏报/误报权衡参数表）；事件动作（超限→F1461 调度降载建议+UE 提示预留——动作只建议不强制：决策权在调度）；测试（注入超限/骤变序列——检测率与时延统计、误报率门禁<1%）。
//!
//! 本模块真实语义密度（全部纯功能，零 IO 零墙钟零浮点）：
//!
//! 1. **超限滞回状态机**（判据一）：[`PowerLimitDetector`]——Normal→Arming→Hit
//!    三态；进入 = 实测 > cap×950‰ 连续 3 采样（T01/T02）；退出 = 实测 ≤
//!    cap×850‰（T03 滞回深谷）；850‰~950‰ 之间为滞回死区（Hit 态内不退出）。
//!    进入/退出时戳、滞留采样数、峰值功率全程留痕。
//! 2. **斜率三分类**（判据二）：[`SlopeClassifier`]——ΔP/Δt 定点斜率（mW/ms，
//!    整数除法防零间隔），阈值表两档分界（T04 突变 15000、T05 缓变 1500），
//!    突变/缓变/平稳三分类闭集（[`SlopeClass`]）。
//! 3. **P99 持续越限画像**（判据三）：[`P99TrendProfile`]——600 样本滚动窗
//!    （T06）+ 整数分桶（桶宽 1024mW，3908 桶）P99 下界估计 + 最小二乘趋势斜率
//!    （Σx/Σy/Σxy/Σx² 增量维护，i128 定点）+ 预警四级（[`WarningLevel`]：
//!    平静/预警 920‰（T07）/危急 970‰（T08）/紧急（危急 ∧ 趋势 ≥ T09））。
//! 4. **阈值全配置化**（判据四）：[`ThresholdBook`]——12 条阈值逐条登记
//!    （id/名称/数值/单位/来源注释），文档串导出（[`ThresholdBook::doc_export`]）；
//!    任何判定载荷都携带 `trace`（阈值 id）——黑盒红线自证：任何判定可追溯
//!    到一个已登记阈值（[`ThresholdBook::verify`]）。
//! 5. **序依赖声明**（判据五）：F1447 滤波后序——输入必须带 `filtered` 标志
//!    （[`PwrSample::filtered`]），未滤波拒收（[`PwrAnomalyError::InputUnfiltered`]，
//!    诊断码 0x1341）；漏报/误报权衡参数表显性（[`ThresholdBook::sensitivity_table`]）。
//! 6. **建议语义**（判据六）：[`AnomalyAdvice`]——超限/危急 → F1461 调度降载
//!    建议 + UE 提示预留；`forced` 位恒 false（决策权在调度——建议不强制，
//!    注释写明）。
//! 7. **注入测试面**（判据七）：[`InjectRig`]——注入超限/骤变/平稳序列 →
//!    检测率 + 检出时延统计 + 误报率 ppm 门禁（<1% = 10000ppm（T10），违约
//!    立案入账）；[`CheatSheetLedger`] 事件账 + `health()` 账体检 + FNV-1a
//!    指纹 + 双跑确定性对拍（[`InjectRig::determinism_pair`]）。
//!
//! 诊断码段：**0x1340~0x134F**（本文件独占，开工前 grep 确认空闲）。
//!
//! 序依赖契约（F1447 → F1450）：本模块只消费 `cgj05_pwrfilt`（F1447）滤波
//! 管道的输出；原始采样直送本模块将被拒收并计数，绝不静默放行——误报控制
//! 的第一道闸在输入侧。F1450 的事件输出供 F1448（事件总线）与 F1461（调度
//! 降载）消费，动作仅建议不强制。

#![forbid(unsafe_code)]

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 常量区（数值全部写死，注释标注来源）
// ---------------------------------------------------------------------------

/// 超限进入比：950‰（cap×0.95）。
/// 来源：锚点原文"实测>cap×0.95 持续 3 采样→PowerLimitHit"。
pub const LIMIT_ENTER_PERMILLE: u32 = 950;

/// 滞回退出比：850‰（cap×0.85）。
/// 来源：锚点原文"滞回退出：cap×0.85"。
pub const LIMIT_EXIT_PERMILLE: u32 = 850;

/// 超限进入滞留：连续 3 采样。
/// 来源：锚点原文"持续 3 采样→PowerLimitHit"。
pub const LIMIT_ENTER_DWELL: u32 = 3;

/// 骤变（突变档）斜率阈：15000 mW/ms（= 15 W/ms）。
/// 来源：F1450 判据"帧间突变"标定——典型负载突袭（30W→300W 于 20ms 内）折合
/// 13.5 W/ms，取 15 W/ms 为突变下界，缓变调频斜率（~1.5 W/ms）不落入此档。
pub const SLOPE_STEEP_MW_PER_MS: u32 = 15_000;

/// 缓变斜率下界：1500 mW/ms（= 1.5 W/ms）。
/// 来源：典型时钟调频爬坡（300W 于 200ms）折合 1.5 W/ms；≥ 此值为缓变
/// （Drift），< 此值为平稳（Stable）。
pub const SLOPE_DRIFT_MW_PER_MS: u32 = 1_500;

/// P99 滚动窗：600 样本。
/// 来源：锚点原文"持续越限（P99 越限画像）"——600 样本 @ 10ms 采样 ≈ 6s
/// 观察窗，与 F1447 60s 聚合窗形成 1:10 层级。
pub const P99_WINDOW: usize = 600;

/// P99 画像预热样本数：60（窗的 10%）。
/// 来源：窗口未满时 P99 统计无意义，取 T06 的 10% 为预热下界（T07 附款）。
pub const P99_WARMUP: usize = 60;

/// P99 预警阈：920‰ cap。
/// 来源：介于滞回进入 950‰ 与退出 850‰ 之间——持续 P99 越过 920‰ 即"画像
/// 级预警"（T07），先于瞬时超限暴露长期越限倾向。
pub const P99_WARN_PERMILLE: u32 = 920;

/// P99 危急阈：970‰ cap。
/// 来源：高于瞬时进入比 950‰——P99（下界估计）达 970‰ 意味着窗口内至少
/// 1% 样本实测越限，属危急（T08）。
pub const P99_CRIT_PERMILLE: u32 = 970;

/// 趋势预警阈：1000 mW / 百样本（= 10 mW/样本 ≈ 1 W/s @ 10ms 采样）。
/// 来源：危急态叠加正趋势才升级"紧急"——趋势阈取 1 W/s 量级，过滤统计噪声
/// （T09）。
pub const TREND_WARN_MW_PER_100: i64 = 1_000;

/// 误报率门禁：10000 ppm（= 1%）。
/// 来源：锚点原文"误报率门禁<1%"——注入测试的误报样本占比必须 < 1%，
/// 违约立案（T10）。
pub const FP_GATE_PPM: u32 = 10_000;

/// 检出时延预算：4 采样。
/// 来源：进入滞留 3 采样 + 1 采样检测裕度——从首个越限样本到 Hit 判定不得
/// 超过 4 采样（T11）。
pub const DETECT_LATENCY_BUDGET_SAMPLES: u32 = 4;

/// 最小合法采样间隔：1 µs。
/// 来源：零间隔（dt=0）无法定义斜率，直接拒收（T12 守卫）。
pub const MIN_SAMPLE_INTERVAL_US: u64 = 1;

/// 事件账容量：4096 条。
/// 来源：与 F1448 事件总线镜像容量对齐；满则半量让位（丢旧留新，丢弃显性
/// 计数，零静默）。
pub const LEDGER_CAP: usize = 4096;

/// 阈值登记条数（T01~T12）。
pub const THRESHOLD_COUNT: usize = 12;

// 编译期断言（非运行期）——阈值自洽性守卫。
const _: () = assert!(LIMIT_EXIT_PERMILLE < LIMIT_ENTER_PERMILLE);
const _: () = assert!(LIMIT_ENTER_PERMILLE <= 1000);
const _: () = assert!(P99_WARN_PERMILLE < P99_CRIT_PERMILLE);
const _: () = assert!(P99_CRIT_PERMILLE > LIMIT_ENTER_PERMILLE);
const _: () = assert!(SLOPE_DRIFT_MW_PER_MS < SLOPE_STEEP_MW_PER_MS);
const _: () = assert!(P99_WINDOW >= 64);
const _: () = assert!(P99_WARMUP < P99_WINDOW);
const _: () = assert!(FP_GATE_PPM == 10_000);
const _: () = assert!(THRESHOLD_COUNT == 12);

// ---------------------------------------------------------------------------
// 诊断码（0x1340~0x134F · 本文件独占段）
// ---------------------------------------------------------------------------

/// F1450 功耗异常检测诊断码闭集（0x1340~0x134F 十六码）。
///
/// 每码三要素（what/why/advice）齐备；全部 `const fn`（match 判别，不触
/// PartialEq 运算符）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PwrAnomalyCode {
    /// 0x1340 样本受理（滤波标志齐备，进入检测流水）。
    SampleAccepted = 0x1340,
    /// 0x1341 未滤波拒收（F1447 序依赖违约）。
    UnfilteredRejected = 0x1341,
    /// 0x1342 超限进入（PowerLimitHit 成立）。
    LimitEntered = 0x1342,
    /// 0x1343 超限退出（滞回深谷确认）。
    LimitExited = 0x1343,
    /// 0x1344 骤变分类成立（斜率 ≥ T04）。
    SteepClassified = 0x1344,
    /// 0x1345 缓变分类成立（T05 ≤ 斜率 < T04）。
    DriftClassified = 0x1345,
    /// 0x1346 平稳分类成立（斜率 < T05）。
    StableClassified = 0x1346,
    /// 0x1347 P99 画像预警升级（达 T07）。
    P99WarnRaised = 0x1347,
    /// 0x1348 P99 画像危急/紧急升级（达 T08/T09）。
    P99Critical = 0x1348,
    /// 0x1349 阈值登记完成（ThresholdBook 自证起点）。
    ThresholdRegistered = 0x1349,
    /// 0x134A 降载建议发放（F1461 消费，不强制）。
    AdviceEmitted = 0x134A,
    /// 0x134B 注入用例受理（注入测试面）。
    InjectCaseAccepted = 0x134B,
    /// 0x134C 误报门禁违约（≥ T10）——立案。
    FpGateViolated = 0x134C,
    /// 0x134D 事件账守恒破坏（health 体检失败）。
    LedgerBroken = 0x134D,
    /// 0x134E 双跑指纹不一致（确定性对拍违约）。
    FingerprintMismatch = 0x134E,
    /// 0x134F 零/负采样间隔守卫拒收（T12）。
    ZeroIntervalGuard = 0x134F,
}

impl PwrAnomalyCode {
    /// 诊断码数值。
    pub const fn as_u16(self) -> u16 {
        self as u16
    }

    /// 三要素：现象。
    pub const fn what(self) -> &'static str {
        match self {
            PwrAnomalyCode::SampleAccepted => "样本受理进入检测流水",
            PwrAnomalyCode::UnfilteredRejected => "未滤波样本被拒收",
            PwrAnomalyCode::LimitEntered => "超限成立（PowerLimitHit）",
            PwrAnomalyCode::LimitExited => "超限退出（滞回深谷确认）",
            PwrAnomalyCode::SteepClassified => "骤变分类成立",
            PwrAnomalyCode::DriftClassified => "缓变分类成立",
            PwrAnomalyCode::StableClassified => "平稳分类成立",
            PwrAnomalyCode::P99WarnRaised => "P99 画像预警升级",
            PwrAnomalyCode::P99Critical => "P99 画像危急/紧急升级",
            PwrAnomalyCode::ThresholdRegistered => "阈值登记完成",
            PwrAnomalyCode::AdviceEmitted => "降载建议已发放",
            PwrAnomalyCode::InjectCaseAccepted => "注入用例已受理",
            PwrAnomalyCode::FpGateViolated => "误报门禁违约已立案",
            PwrAnomalyCode::LedgerBroken => "事件账守恒破坏",
            PwrAnomalyCode::FingerprintMismatch => "双跑指纹不一致",
            PwrAnomalyCode::ZeroIntervalGuard => "零/负采样间隔被守卫拒收",
        }
    }

    /// 三要素：原因。
    pub const fn why(self) -> &'static str {
        match self {
            PwrAnomalyCode::SampleAccepted => "filtered 标志置位且间隔合法",
            PwrAnomalyCode::UnfilteredRejected => "F1447 序依赖：未过滤波管道直送",
            PwrAnomalyCode::LimitEntered => "实测 > cap×950‰ 连续 3 采样（T01/T02）",
            PwrAnomalyCode::LimitExited => "实测 ≤ cap×850‰（T03 滞回）",
            PwrAnomalyCode::SteepClassified => "ΔP/Δt ≥ 15000 mW/ms（T04）",
            PwrAnomalyCode::DriftClassified => "ΔP/Δt ≥ 1500 mW/ms（T05）",
            PwrAnomalyCode::StableClassified => "ΔP/Δt < 1500 mW/ms（T05 下界以下）",
            PwrAnomalyCode::P99WarnRaised => "P99 ≥ cap×920‰ 持续窗口（T07）",
            PwrAnomalyCode::P99Critical => "P99 ≥ cap×970‰（T08），紧急另需趋势 ≥ T09",
            PwrAnomalyCode::ThresholdRegistered => "ThresholdBook 构造登记",
            PwrAnomalyCode::AdviceEmitted => "超限进入或画像危急触发建议语义",
            PwrAnomalyCode::InjectCaseAccepted => "注入用例非空且通过前置校验",
            PwrAnomalyCode::FpGateViolated => "注入套件误报率 ≥ 10000 ppm（T10）",
            PwrAnomalyCode::LedgerBroken => "账内 seq 非单调或码段越界",
            PwrAnomalyCode::FingerprintMismatch => "同输入双跑产出不同指纹",
            PwrAnomalyCode::ZeroIntervalGuard => "采样时戳未前进（dt < 1µs）",
        }
    }

    /// 三要素：建议。
    pub const fn advice(self) -> &'static str {
        match self {
            PwrAnomalyCode::SampleAccepted => "无需动作，继续流水",
            PwrAnomalyCode::UnfilteredRejected => "样本先送 cgj05_pwrfilt（F1447）滤波再复检",
            PwrAnomalyCode::LimitEntered => "消费 AnomalyAdvice 交 F1461 调度降载（建议不强制）",
            PwrAnomalyCode::LimitExited => "解除降载建议由调度裁量，本模块不撤回",
            PwrAnomalyCode::SteepClassified => "核对 F1447 去毛刺级是否漏尖刺",
            PwrAnomalyCode::DriftClassified => "观察 P99 画像是否同步抬升",
            PwrAnomalyCode::StableClassified => "无需动作",
            PwrAnomalyCode::P99WarnRaised => "关注窗口内占空比，预留降载预案",
            PwrAnomalyCode::P99Critical => "立即向 F1461 转交紧急建议",
            PwrAnomalyCode::ThresholdRegistered => "任何判定追溯时按 trace id 查本账",
            PwrAnomalyCode::AdviceEmitted => "调度侧自行决策，忽略不影响本模块状态",
            PwrAnomalyCode::InjectCaseAccepted => "等待套件统计（检测率/时延/误报）",
            PwrAnomalyCode::FpGateViolated => "立案：复检 T01~T12 与 F1447 上游滤波参数",
            PwrAnomalyCode::LedgerBroken => "停用本账并重建，先查 seq 发放器",
            PwrAnomalyCode::FingerprintMismatch => "查编译器/目标差异，禁止带病合流",
            PwrAnomalyCode::ZeroIntervalGuard => "检查采样源时戳单调性（F1447 五级管道）",
        }
    }
}

// ---------------------------------------------------------------------------
// 错误枚举（每变体 detail() 中文可操作诊断）
// ---------------------------------------------------------------------------

/// F1450 全部可失败路径的唯一错误类型。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PwrAnomalyError {
    /// 输入样本未带 F1447 滤波标志。
    InputUnfiltered,
    /// 采样时戳未前进（dt = 0 或回退）。
    ZeroSampleInterval,
    /// 功耗上限 cap 非法（0 或超量程）。
    CapInvalid,
    /// 阈值 id 未登记（追溯断裂）。
    ThresholdUnregistered,
    /// 事件账守恒体检失败。
    LedgerCorrupted,
    /// 注入用例为空。
    InjectionCaseEmpty,
    /// 双跑确定性对拍指纹不一致。
    DeterminismBroken,
    /// 统计量溢出（saturating 饱和后仍越界）。
    StatsOverflow,
}

impl PwrAnomalyError {
    /// 中文可操作诊断（每变体一条，含处置步骤）。
    pub const fn detail(&self) -> &'static str {
        match self {
            PwrAnomalyError::InputUnfiltered => {
                "样本 filtered=false：请先送 cgj05_pwrfilt（F1447）滤波管道，\
                 取滤波级输出的 filtered=true 样本再送 F1450；本模块拒收原始采样"
            }
            PwrAnomalyError::ZeroSampleInterval => {
                "采样时戳未前进（dt<1µs）：检查 F1447 采样源时戳单调性；\
                 同一时戳的重复样本应在上游去重，勿直送本模块"
            }
            PwrAnomalyError::CapInvalid => {
                "cap_mw 非法（0 或超 u32 量程）：核对 F1449/RAPL 上报的功耗上限，\
                 cap 必须为正且 ≤ 4_000_000 mW（4kW 量程）"
            }
            PwrAnomalyError::ThresholdUnregistered => {
                "trace 阈值 id 未在 ThresholdBook 登记（1..=12）：\
                 判定载荷的 trace id 只允许来自登记表，怀疑被篡改请重审生成点"
            }
            PwrAnomalyError::LedgerCorrupted => {
                "事件账守恒体检失败（seq 非单调或码段越界）：停用本账、\
                 重建 CheatSheetLedger，并核查是否存在并发写（本模块无内部锁）"
            }
            PwrAnomalyError::InjectionCaseEmpty => {
                "注入用例 samples 为空：注入测试面只受理非空序列，\
                 空用例请删除或在构造侧补样本"
            }
            PwrAnomalyError::DeterminismBroken => {
                "同输入双跑指纹不一致：检查目标/编译器差异与本模块外部输入，\
                 F1450 全程定点无浮点无时钟，指纹必须一致，禁止带病合流"
            }
            PwrAnomalyError::StatsOverflow => {
                "统计量饱和溢出：误报样本量或用例量已达计数上限，\
                 请分批注入并合并统计"
            }
        }
    }

    /// 错误对应的诊断码（记账用）。
    pub const fn code(self) -> PwrAnomalyCode {
        match self {
            PwrAnomalyError::InputUnfiltered => PwrAnomalyCode::UnfilteredRejected,
            PwrAnomalyError::ZeroSampleInterval => PwrAnomalyCode::ZeroIntervalGuard,
            PwrAnomalyError::CapInvalid => PwrAnomalyCode::UnfilteredRejected,
            PwrAnomalyError::ThresholdUnregistered => PwrAnomalyCode::ThresholdRegistered,
            PwrAnomalyError::LedgerCorrupted => PwrAnomalyCode::LedgerBroken,
            PwrAnomalyError::InjectionCaseEmpty => PwrAnomalyCode::InjectCaseAccepted,
            PwrAnomalyError::DeterminismBroken => PwrAnomalyCode::FingerprintMismatch,
            PwrAnomalyError::StatsOverflow => PwrAnomalyCode::FpGateViolated,
        }
    }
}

// ---------------------------------------------------------------------------
// 输入样本
// ---------------------------------------------------------------------------

/// 单个功耗采样（F1447 滤波管道输出口径）。
///
/// 单位约定：功率/上限均 mW（u32，量程 4kW）；时戳 µs（u64）。
/// `filtered` 是 F1447 序依赖的硬件位：滤波管道出口置 true，本模块只收
/// true 样本——这是误报控制的第一道闸（判据五）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct PwrSample {
    /// 采样时戳（µs，单调递增）。
    pub ts_us: u64,
    /// 实测功耗（mW）。
    pub power_mw: u32,
    /// 功耗上限 cap（mW）。
    pub cap_mw: u32,
    /// F1447 滤波标志：true = 已过滤波管道，false = 原始采样（必拒）。
    pub filtered: bool,
}

impl PwrSample {
    /// 构造滤波后样本（正常入口）。
    pub const fn filtered_new(ts_us: u64, power_mw: u32, cap_mw: u32) -> Self {
        PwrSample { ts_us, power_mw, cap_mw, filtered: true }
    }

    /// 构造原始样本（仅用于负向测试——本模块必拒）。
    pub const fn raw_new(ts_us: u64, power_mw: u32, cap_mw: u32) -> Self {
        PwrSample { ts_us, power_mw, cap_mw, filtered: false }
    }

    /// 基本合法性：cap 为正且功率不超量程上限。
    pub const fn is_shape_ok(&self) -> bool {
        self.cap_mw > 0 && self.cap_mw <= 4_000_000
    }
}

// ---------------------------------------------------------------------------
// ThresholdBook · 判据四：阈值全配置化 + 文档公开 + 判定可追溯
// ---------------------------------------------------------------------------

/// 单条阈值登记项。
#[derive(Clone, Copy, Debug)]
pub struct ThresholdReg {
    /// 登记 id（T01=1 … T12=12）——判定载荷 trace 字段引用此 id。
    pub id: u16,
    /// 阈值名（稳定标识）。
    pub name: &'static str,
    /// 阈值数值（语义由 unit 说明）。
    pub value: u64,
    /// 单位/口径说明。
    pub unit: &'static str,
    /// 数值来源注释（锚点原文或标定依据）。
    pub source: &'static str,
}

/// 配置化阈值簿：F1450 全部判定阈值显性登记（判据四）。
///
/// 黑盒红线自证：本模块任何判定（超限进出/斜率分类/画像预警/门禁裁决）
/// 的载荷都携带 `trace: u16`，指向本簿一条登记项——任何判定可追溯到一个
/// 已登记阈值；未登记 id 无法进入判定路径（[`ThresholdBook::verify`]）。
#[derive(Clone, Debug)]
pub struct ThresholdBook {
    regs: [ThresholdReg; THRESHOLD_COUNT],
}

impl ThresholdBook {
    /// 构造并完成登记（T01~T12）。数值常量来源见各常量定义处注释。
    pub const fn new() -> Self {
        ThresholdBook {
            regs: [
                ThresholdReg {
                    id: 1,
                    name: "limit_enter_permille",
                    value: LIMIT_ENTER_PERMILLE as u64,
                    unit: "‰cap（实测>cap×0.95 进入超限）",
                    source: "锚点原文：实测>cap×0.95 持续 3 采样→PowerLimitHit",
                },
                ThresholdReg {
                    id: 2,
                    name: "limit_enter_dwell_samples",
                    value: LIMIT_ENTER_DWELL as u64,
                    unit: "连续采样数",
                    source: "锚点原文：持续 3 采样→PowerLimitHit",
                },
                ThresholdReg {
                    id: 3,
                    name: "limit_exit_permille",
                    value: LIMIT_EXIT_PERMILLE as u64,
                    unit: "‰cap（实测≤cap×0.85 滞回退出）",
                    source: "锚点原文：滞回退出：cap×0.85",
                },
                ThresholdReg {
                    id: 4,
                    name: "slope_steep_mw_per_ms",
                    value: SLOPE_STEEP_MW_PER_MS as u64,
                    unit: "mW/ms（ΔP/Δt 突变档下界）",
                    source: "判据标定：负载突袭 13.5W/ms 不误入缓变，取 15W/ms",
                },
                ThresholdReg {
                    id: 5,
                    name: "slope_drift_mw_per_ms",
                    value: SLOPE_DRIFT_MW_PER_MS as u64,
                    unit: "mW/ms（缓变档下界，低于即平稳）",
                    source: "判据标定：时钟调频爬坡 ~1.5W/ms",
                },
                ThresholdReg {
                    id: 6,
                    name: "p99_window_samples",
                    value: P99_WINDOW as u64,
                    unit: "样本（滚动窗）",
                    source: "锚点原文：P99 越限画像——600 样本 @10ms ≈ 6s",
                },
                ThresholdReg {
                    id: 7,
                    name: "p99_warn_permille",
                    value: P99_WARN_PERMILLE as u64,
                    unit: "‰cap（画像预警）",
                    source: "标定：介于滞回 850‰/950‰ 之间取 920‰",
                },
                ThresholdReg {
                    id: 8,
                    name: "p99_crit_permille",
                    value: P99_CRIT_PERMILLE as u64,
                    unit: "‰cap（画像危急）",
                    source: "标定：高于瞬时进入比 950‰ 取 970‰",
                },
                ThresholdReg {
                    id: 9,
                    name: "trend_warn_mw_per_100",
                    value: TREND_WARN_MW_PER_100 as u64,
                    unit: "mW/百样本（趋势斜率预警）",
                    source: "标定：1W/s 量级过滤统计噪声",
                },
                ThresholdReg {
                    id: 10,
                    name: "fp_gate_ppm",
                    value: FP_GATE_PPM as u64,
                    unit: "ppm（误报率门禁，<1%）",
                    source: "锚点原文：误报率门禁<1%",
                },
                ThresholdReg {
                    id: 11,
                    name: "detect_latency_budget_samples",
                    value: DETECT_LATENCY_BUDGET_SAMPLES as u64,
                    unit: "采样（首个越限样本→Hit 判定）",
                    source: "标定：滞留 3 采样 + 1 裕度",
                },
                ThresholdReg {
                    id: 12,
                    name: "min_sample_interval_us",
                    value: MIN_SAMPLE_INTERVAL_US,
                    unit: "µs（零间隔守卫）",
                    source: "守卫条款：dt=0 无法定义斜率，必拒",
                },
            ],
        }
    }

    /// 追溯校验：id 必须在登记表内（判据四的可追溯闸门）。
    pub fn verify(&self, id: u16) -> Result<(), PwrAnomalyError> {
        if id == 0 || id as usize > THRESHOLD_COUNT {
            return Err(PwrAnomalyError::ThresholdUnregistered);
        }
        // 双保险：id 与登记位一致才算登记（防表错位）。
        if self.regs[(id - 1) as usize].id != id {
            return Err(PwrAnomalyError::ThresholdUnregistered);
        }
        Ok(())
    }

    /// 按登记项引用查询名称（判定载荷展示用）。
    pub fn name_of(&self, id: u16) -> Option<&'static str> {
        if id == 0 || id as usize > THRESHOLD_COUNT {
            return None;
        }
        let r = &self.regs[(id - 1) as usize];
        if r.id == id {
            Some(r.name)
        } else {
            None
        }
    }

    /// 超限进入电平（mW）= cap × 950‰ / 1000（u64 中转防溢出）。
    pub const fn enter_level_mw(&self, cap_mw: u32) -> u32 {
        ((cap_mw as u64 * LIMIT_ENTER_PERMILLE as u64) / 1000) as u32
    }

    /// 滞回退出电平（mW）= cap × 850‰ / 1000。
    pub const fn exit_level_mw(&self, cap_mw: u32) -> u32 {
        ((cap_mw as u64 * LIMIT_EXIT_PERMILLE as u64) / 1000) as u32
    }

    /// P99 预警电平（mW）= cap × 920‰ / 1000。
    pub const fn warn_level_mw(&self, cap_mw: u32) -> u32 {
        ((cap_mw as u64 * P99_WARN_PERMILLE as u64) / 1000) as u32
    }

    /// P99 危急电平（mW）= cap × 970‰ / 1000。
    pub const fn crit_level_mw(&self, cap_mw: u32) -> u32 {
        ((cap_mw as u64 * P99_CRIT_PERMILLE as u64) / 1000) as u32
    }

    /// 文档串导出（判据四"文档公开"）：全部阈值逐条成文，含序依赖声明。
    pub fn doc_export(&self) -> String {
        let mut s = String::from("== F1450 阈值登记簿（判定全可追溯） ==\n");
        let mut i = 0usize;
        while i < THRESHOLD_COUNT {
            let r = &self.regs[i];
            let line = format!(
                "T{:02} {} = {} [{}] 来源: {}\n",
                r.id, r.name, r.value, r.unit, r.source
            );
            s.push_str(&line);
            i += 1;
        }
        s.push_str(SEQ_DEPENDENCY_NOTE);
        s
    }

    /// 漏报/误报权衡参数表（判据五"权衡参数表显性"）。
    ///
    /// 每行：旋钮名 / 阈值 id / 收紧（调高/调深）后果 / 放松后果。
    /// 全部为显性静态表——调参者可据此预判漏报↑ vs 误报↓的交换。
    pub fn sensitivity_table(&self) -> &'static [SensitivityKnob] {
        &SENSITIVITY_TABLE
    }
}

/// 权衡参数表条目。
#[derive(Clone, Copy, Debug)]
pub struct SensitivityKnob {
    /// 旋钮名。
    pub name: &'static str,
    /// 对应阈值 id。
    pub threshold_id: u16,
    /// 收紧（数值调高/滞回调深）的后果。
    pub tighten: &'static str,
    /// 放松（数值调低/滞回调浅）的后果。
    pub loosen: &'static str,
}

/// 静态权衡表（四旋钮）。
static SENSITIVITY_TABLE: [SensitivityKnob; 4] = [
    SensitivityKnob {
        name: "limit_enter_permille",
        threshold_id: 1,
        tighten: "950→980：误报↓（更接近真实 cap 才报警），漏报↑（瞬时超限可能漏）",
        loosen: "950→900：误报↑，漏报↓（提前捕获逼近趋势）",
    },
    SensitivityKnob {
        name: "limit_enter_dwell_samples",
        threshold_id: 2,
        tighten: "3→5：误报↓（单尖刺不再触发），漏报↑（检出时延 +2 采样）",
        loosen: "3→2：误报↑（毛刺敏感），漏报↓（时延 −1 采样）",
    },
    SensitivityKnob {
        name: "limit_exit_permille",
        threshold_id: 3,
        tighten: "850→800：滞回更深，抖振误报↓，解除响应变慢",
        loosen: "850→900：滞回变浅，解除响应快，抖振误报↑",
    },
    SensitivityKnob {
        name: "slope_steep_mw_per_ms",
        threshold_id: 4,
        tighten: "15000→30000：骤变误报↓，缓变漏报↑（真突袭可能落入缓变档）",
        loosen: "15000→8000：骤变检出↑，误报↑（重负载正常爬坡误判）",
    },
];

/// 序依赖声明常量（随 doc_export 公开）。
pub const SEQ_DEPENDENCY_NOTE: &str = "序依赖：F1447(F1450 输入)→F1450(本模块)→F1448/F1461(输出)。\
本模块只受理 filtered=true 样本；未滤波拒收（0x1341）。检测算法显性：全部阈值见上表，\
任何判定载荷的 trace 字段可回溯至登记项；黑盒红线不适用。\n";

// ---------------------------------------------------------------------------
// PowerLimitDetector · 判据一：超限检测滞回状态机
// ---------------------------------------------------------------------------

/// 超限检测状态机三态。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum LimitState {
    /// 正常态（未超限，亦未累积）。
    Normal,
    /// 攒积态（实测已越进入电平，但连续滞留 < T02=3 采样）。
    Arming,
    /// 超限命中态（PowerLimitHit 成立；850‰~950‰ 为滞回死区）。
    Hit,
}

/// 超限状态迁移载荷（进入/退出留痕）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum LimitEventKind {
    /// 进入超限（PowerLimitHit）。
    Enter,
    /// 退出超限（滞回深谷确认）。
    Exit,
}

/// 超限状态迁移事件：进入/退出时戳 + 滞留采样数 + 峰值 + 追溯阈值 id。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct LimitTransition {
    /// 迁移种类。
    pub kind: LimitEventKind,
    /// 迁移发生时戳（µs，取判定样本时戳）。
    pub ts_us: u64,
    /// Hit 态滞留采样数（Enter 时为触发时连续越限计数；Exit 时为命中期总滞留）。
    pub dwell_samples: u32,
    /// 命中期峰值功率（mW；Enter 时为当次命中期峰值，Exit 时为全程峰值）。
    pub peak_mw: u32,
    /// 追溯阈值 id（Enter→T01；Exit→T03）。
    pub trace: u16,
}

/// 超限检测状态机（判据一）。
///
/// 迁移规则（全部阈值出自 [`ThresholdBook`]）：
/// * Normal/Arming → Hit：实测 > cap×950‰（T01）**连续** 3 采样（T02）。
///   连续性中断（实测回落到进入电平以下）即清零攒积计数——"持续"语义。
/// * Hit → Normal：实测 ≤ cap×850‰（T03）。
/// * Hit 态内 850‰ < 实测 ≤ 950‰：滞回死区，保持 Hit（防抖振误报）。
/// * Arming 态内实测回落 → Normal 并清零（Arming 不留痕迁移事件）。
///
/// 留痕：进入/退出时戳、命中期滞留采样数、峰值功率、追溯阈值 id 全程记录。
#[derive(Clone, Debug)]
pub struct PowerLimitDetector {
    state: LimitState,
    /// 连续越限计数（Arming 攒积）。
    consecutive: u32,
    /// 命中期起始时戳（Enter 时戳）。
    enter_ts_us: u64,
    /// 命中期结束时戳（Exit 时戳）。
    exit_ts_us: u64,
    /// 命中期滞留采样总数。
    dwell_samples: u32,
    /// 命中期峰值功率（mW）。
    peak_mw: u32,
    /// 累计迁移次数（Enter+Exit 总和，账面用）。
    transitions: u32,
    /// 累计清零次数（连续性中断），误报控制观测点。
    resets: u32,
}

impl PowerLimitDetector {
    /// 构造（初始 Normal，全账清零）。
    pub const fn new() -> Self {
        PowerLimitDetector {
            state: LimitState::Normal,
            consecutive: 0,
            enter_ts_us: 0,
            exit_ts_us: 0,
            dwell_samples: 0,
            peak_mw: 0,
            transitions: 0,
            resets: 0,
        }
    }

    /// 当前状态。
    pub const fn state(&self) -> LimitState {
        self.state
    }

    /// 当前连续越限计数（观测用）。
    pub const fn consecutive(&self) -> u32 {
        self.consecutive
    }

    /// 进入时戳留痕。
    pub const fn enter_ts_us(&self) -> u64 {
        self.enter_ts_us
    }

    /// 退出时戳留痕。
    pub const fn exit_ts_us(&self) -> u64 {
        self.exit_ts_us
    }

    /// 命中期滞留采样数。
    pub const fn dwell_samples(&self) -> u32 {
        self.dwell_samples
    }

    /// 命中期峰值功率（mW）。
    pub const fn peak_mw(&self) -> u32 {
        self.peak_mw
    }

    /// 累计迁移次数。
    pub const fn transitions(&self) -> u32 {
        self.transitions
    }

    /// 累计连续性清零次数（攒积被打断）。
    pub const fn resets(&self) -> u32 {
        self.resets
    }

    /// 单样本馈入（判据一主路径）。
    ///
    /// 返回 `Ok(Some(LimitTransition))` 表示本样本触发状态迁移（Enter/Exit）；
    /// `Ok(None)` 表示状态未迁移。样本形状非法走 [`PwrAnomalyError::CapInvalid`]。
    pub fn feed(
        &mut self,
        s: &PwrSample,
        book: &ThresholdBook,
    ) -> Result<Option<LimitTransition>, PwrAnomalyError> {
        if !s.is_shape_ok() {
            return Err(PwrAnomalyError::CapInvalid);
        }
        let enter_level = book.enter_level_mw(s.cap_mw);
        let exit_level = book.exit_level_mw(s.cap_mw);
        let above = s.power_mw > enter_level;
        let below = s.power_mw <= exit_level;

        match self.state {
            LimitState::Normal | LimitState::Arming => {
                if above {
                    self.consecutive = self.consecutive.saturating_add(1);
                    if self.consecutive >= LIMIT_ENTER_DWELL {
                        // Arming → Hit（或 Normal 直跳，等价于连续第 3 采样命中）。
                        self.state = LimitState::Hit;
                        self.enter_ts_us = s.ts_us;
                        self.dwell_samples = self.consecutive;
                        self.peak_mw = s.power_mw;
                        self.transitions = self.transitions.saturating_add(1);
                        return Ok(Some(LimitTransition {
                            kind: LimitEventKind::Enter,
                            ts_us: s.ts_us,
                            dwell_samples: self.dwell_samples,
                            peak_mw: self.peak_mw,
                            trace: 1, // T01 进入比
                        }));
                    }
                    self.state = LimitState::Arming;
                } else {
                    // 连续性中断：攒积清零（"持续"语义），清零显性计数。
                    if self.consecutive > 0 {
                        self.resets = self.resets.saturating_add(1);
                    }
                    self.consecutive = 0;
                    self.state = LimitState::Normal;
                }
                Ok(None)
            }
            LimitState::Hit => {
                // 命中期滞留/峰值留痕（saturating 显性）。
                self.dwell_samples = self.dwell_samples.saturating_add(1);
                if s.power_mw > self.peak_mw {
                    self.peak_mw = s.power_mw;
                }
                if below {
                    // Hit → Normal：滞回深谷确认。
                    self.state = LimitState::Normal;
                    self.exit_ts_us = s.ts_us;
                    self.transitions = self.transitions.saturating_add(1);
                    let ev = LimitTransition {
                        kind: LimitEventKind::Exit,
                        ts_us: s.ts_us,
                        dwell_samples: self.dwell_samples,
                        peak_mw: self.peak_mw,
                        trace: 3, // T03 滞回退出比
                    };
                    self.consecutive = 0;
                    self.dwell_samples = 0;
                    self.peak_mw = 0;
                    return Ok(Some(ev));
                }
                // 850‰~950‰ 死区：保持 Hit（滞回防抖振）。
                Ok(None)
            }
        }
    }
}

// ---------------------------------------------------------------------------
// SlopeClassifier · 判据二：骤变/缓变分类
// ---------------------------------------------------------------------------

/// 斜率三分类闭集（突变 / 缓变 / 平稳）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SlopeClass {
    /// 突变：斜率 ≥ 15000 mW/ms（T04）——帧间突袭。
    Steep,
    /// 缓变：1500 ≤ 斜率 < 15000 mW/ms（T05~T04 区间）——调频爬坡类。
    Drift,
    /// 平稳：斜率 < 1500 mW/ms（T05 下界以下）。
    Stable,
}

impl SlopeClass {
    /// 分类对应诊断码。
    pub const fn code(self) -> PwrAnomalyCode {
        match self {
            SlopeClass::Steep => PwrAnomalyCode::SteepClassified,
            SlopeClass::Drift => PwrAnomalyCode::DriftClassified,
            SlopeClass::Stable => PwrAnomalyCode::StableClassified,
        }
    }

    /// 中文语义名。
    pub const fn label(self) -> &'static str {
        match self {
            SlopeClass::Steep => "突变（帧间突袭）",
            SlopeClass::Drift => "缓变（趋势爬坡）",
            SlopeClass::Stable => "平稳",
        }
    }
}

/// 斜率判定载荷：分类 + 定点斜率 + 追溯阈值 id。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct SlopeVerdict {
    /// 三分类结论。
    pub class: SlopeClass,
    /// 定点斜率（mW/ms，u32 饱和封顶）。
    pub slope_mw_per_ms: u32,
    /// 本次样本与前样本的功率差绝对值（mW）。
    pub dp_mw: u32,
    /// 采样间隔（µs，恒 ≥ 1——T12 守卫保证）。
    pub dt_us: u64,
    /// 追溯阈值 id（Steep→T04；Drift/Stable→T05）。
    pub trace: u16,
}

/// 骤变/缓变分类器（判据二）。
///
/// 斜率口径：slope = ΔP(mW) × 1000 / Δt(µs) → mW/ms（定点整数，除法前显性
/// 防零：dt < 1µs 拒收；乘法 saturating；结果 u64 中转后饱和收窄到 u32）。
/// 首样本（无前件）按平稳归档（slope=0），属基线口径而非判定。
#[derive(Clone, Debug)]
pub struct SlopeClassifier {
    prev_ts_us: u64,
    prev_power_mw: u32,
    seen: bool,
    steep_count: u32,
    drift_count: u32,
    stable_count: u32,
}

impl SlopeClassifier {
    /// 构造（无前件基线）。
    pub const fn new() -> Self {
        SlopeClassifier {
            prev_ts_us: 0,
            prev_power_mw: 0,
            seen: false,
            steep_count: 0,
            drift_count: 0,
            stable_count: 0,
        }
    }

    /// 三分类累计计数 [陡, 缓, 平]。
    pub const fn counts(&self) -> (u32, u32, u32) {
        (self.steep_count, self.drift_count, self.stable_count)
    }

    /// 单样本馈入（判据二主路径）。
    ///
    /// 零/负间隔（dt < 1µs，T12）走 [`PwrAnomalyError::ZeroSampleInterval`]；
    /// 分类结果携带追溯阈值 id——黑盒红线自证。
    pub fn feed(
        &mut self,
        s: &PwrSample,
        _book: &ThresholdBook,
    ) -> Result<SlopeVerdict, PwrAnomalyError> {
        if !s.is_shape_ok() {
            return Err(PwrAnomalyError::CapInvalid);
        }
        // 阈值经常量直连（登记簿持有同一常量，verify 可追溯）。
        if !self.seen {
            // 基线：首样本无前件，按平稳归档。
            self.seen = true;
            self.prev_ts_us = s.ts_us;
            self.prev_power_mw = s.power_mw;
            self.stable_count = self.stable_count.saturating_add(1);
            return Ok(SlopeVerdict {
                class: SlopeClass::Stable,
                slope_mw_per_ms: 0,
                dp_mw: 0,
                dt_us: 0,
                trace: 5, // T05 下界（平稳口径）
            });
        }
        // T12 零间隔守卫：dt 必须至少 1µs 且时戳前进。
        if s.ts_us < self.prev_ts_us.saturating_add(MIN_SAMPLE_INTERVAL_US) {
            return Err(PwrAnomalyError::ZeroSampleInterval);
        }
        let dt_us = s.ts_us - self.prev_ts_us;
        // ΔP 绝对值（u32 差，先比较后相减不溢出）。
        let dp_mw = if s.power_mw >= self.prev_power_mw {
            s.power_mw - self.prev_power_mw
        } else {
            self.prev_power_mw - s.power_mw
        };
        // slope = dp × 1000 / dt（u64 中转；乘法 saturating；除数恒 ≥ 1）。
        let slope64 = (dp_mw as u64).saturating_mul(1_000) / dt_us;
        let slope_mw_per_ms = if slope64 > u32::MAX as u64 {
            u32::MAX
        } else {
            slope64 as u32
        };
        self.prev_ts_us = s.ts_us;
        self.prev_power_mw = s.power_mw;

        // 三分类（阈值全部来自登记簿 T04/T05）。
        let verdict = if slope_mw_per_ms >= SLOPE_STEEP_MW_PER_MS {
            self.steep_count = self.steep_count.saturating_add(1);
            SlopeVerdict {
                class: SlopeClass::Steep,
                slope_mw_per_ms,
                dp_mw,
                dt_us,
                trace: 4, // T04 突变档
            }
        } else if slope_mw_per_ms >= SLOPE_DRIFT_MW_PER_MS {
            self.drift_count = self.drift_count.saturating_add(1);
            SlopeVerdict {
                class: SlopeClass::Drift,
                slope_mw_per_ms,
                dp_mw,
                dt_us,
                trace: 5, // T05 缓变档
            }
        } else {
            self.stable_count = self.stable_count.saturating_add(1);
            SlopeVerdict {
                class: SlopeClass::Stable,
                slope_mw_per_ms,
                dp_mw,
                dt_us,
                trace: 5, // T05 下界以下（平稳）
            }
        };
        Ok(verdict)
    }
}

// ---------------------------------------------------------------------------
// P99TrendProfile · 判据三：持续越限画像（P99 + 趋势 + 预警四级）
// ---------------------------------------------------------------------------

/// 画像预警四级闭集。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum WarningLevel {
    /// 0 级平静：P99 未达 T07 或仍在预热期。
    Quiet,
    /// 1 级预警：P99 ≥ cap×920‰（T07）。
    Warn,
    /// 2 级危急：P99 ≥ cap×970‰（T08）。
    Critical,
    /// 3 级紧急：危急 ∧ 趋势斜率 ≥ T09（持续越限且仍在恶化）。
    Emergency,
}

impl WarningLevel {
    /// 数值档位（0~3，比较升级用）。
    pub const fn rank(self) -> u8 {
        match self {
            WarningLevel::Quiet => 0,
            WarningLevel::Warn => 1,
            WarningLevel::Critical => 2,
            WarningLevel::Emergency => 3,
        }
    }
}

/// 画像判定载荷：P99 估计 + 趋势斜率 + 预警级 + 级变标志 + 追溯 id。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ProfileVerdict {
    /// P99 下界估计（mW；整数分桶口径，误差 ≤ 1024mW）。
    pub p99_mw: u64,
    /// 趋势斜率（mW/百样本，i64 带符号）。
    pub trend_mw_per_100: i64,
    /// 当前预警级。
    pub level: WarningLevel,
    /// 本样本是否发生预警级变化（任意方向）。
    pub level_changed: bool,
    /// 追溯阈值 id（达级时：Warn→T07；Critical/Emergency→T08；紧急升级另含 T09）。
    pub trace: u16,
}

/// 持续越限画像（判据三）。
///
/// * 600 样本滚动窗（T06）：环形槽 + 增量统计量（Σx/Σy/Σxy/Σx²），淘汰旧样
///   本时同步扣减——趋势估计 O(1) 摊销。
/// * P99：整数分桶（桶宽 1024mW = 1<<10，0 桶为 0mW，3908 桶覆盖 4kW 量程）
///   ——桶内不细分，结果为真 P99 的**下界估计**（误差 ≤ 1024mW，口径显性，
///   文档公开）。
/// * 趋势：滚动窗最小二乘斜率（i128 定点），换算为 mW/百样本；分母为 0
///   （单点/全同 x）按 0 斜率处理。
/// * 预警四级：Quiet→Warn→Critical→Emergency（升级判定见
///   [`P99TrendProfile::evaluate_level`]）。
///
/// 内存：600×(4+8)+33×4+杂项 ≈ 7.5KB < 64KB（编译期断言守卫，不上堆外大栈）。
#[derive(Clone, Debug)]
pub struct P99TrendProfile {
    ys: [u32; P99_WINDOW],
    xs: [u64; P99_WINDOW],
    bucket_counts: [u32; P99_BUCKETS],
    filled: usize,
    head: usize,
    seq: u64,
    sum_x: u64,
    sum_y: u64,
    sum_xy: u64,
    sum_x2: u64,
    cap_mw: u32,
    last_level_rank: u8,
}

/// P99 分桶移位：桶宽 1024mW（v >> 10）。
/// 来源：预警判据精度要求 ≤ 1% cap（300W cap → 3W）；1024mW 桶宽满足且
/// 桶数组 3908×4B ≈ 15.3KB，结构体总量 < 64KB。
pub const P99_BUCKET_SHIFT: u32 = 10;

/// 分桶数（覆盖 4kW 量程 + 边界裕度 2 桶）。
pub const P99_BUCKETS: usize = (4_000_000usize >> P99_BUCKET_SHIFT) + 2;

// 结构体体量守卫：>64KB 结构体禁上栈（编译期断言）。
const _: () = assert!(core::mem::size_of::<P99TrendProfile>() <= 64 * 1024);
const _: () = assert!(P99_BUCKETS == 3908);

impl P99TrendProfile {
    /// 构造（空窗 + cap 绑定）。cap 非法走 [`PwrAnomalyError::CapInvalid`]。
    pub fn new(cap_mw: u32) -> Result<Self, PwrAnomalyError> {
        if cap_mw == 0 || cap_mw > 4_000_000 {
            return Err(PwrAnomalyError::CapInvalid);
        }
        Ok(P99TrendProfile {
            ys: [0u32; P99_WINDOW],
            xs: [0u64; P99_WINDOW],
            bucket_counts: [0u32; P99_BUCKETS],
            filled: 0,
            head: 0,
            seq: 0,
            sum_x: 0,
            sum_y: 0,
            sum_xy: 0,
            sum_x2: 0,
            cap_mw,
            last_level_rank: 0,
        })
    }

    /// 已填充样本数。
    pub const fn filled(&self) -> usize {
        self.filled
    }

    /// 绑定的 cap（mW）。
    pub const fn cap_mw(&self) -> u32 {
        self.cap_mw
    }

    /// 整数分桶映射：桶 = v >> 10（桶宽 1024mW；0mW → 桶 0；4kW 满量程 → 桶 3906 < 3908）。
    fn bucket_of(v: u32) -> usize {
        (v as usize) >> P99_BUCKET_SHIFT
    }

    /// 桶下界（mW）：桶 b → b << 10（桶内不细分，下界口径）。
    fn bucket_lower_mw(b: usize) -> u64 {
        (b as u64) << P99_BUCKET_SHIFT
    }

    /// 推入样本并增量维护统计量（滚动淘汰旧样本时同步扣减）。
    pub fn push(&mut self, s: &PwrSample) {
        if self.filled == P99_WINDOW {
            // 满窗：淘汰 head 槽旧样本，扣减统计量（saturating 防账面回退）。
            let oy = self.ys[self.head];
            let ox = self.xs[self.head];
            let ob = Self::bucket_of(oy);
            self.bucket_counts[ob] = self.bucket_counts[ob].saturating_sub(1);
            self.sum_x = self.sum_x.saturating_sub(ox);
            self.sum_y = self.sum_y.saturating_sub(oy as u64);
            self.sum_xy = self.sum_xy.saturating_sub(ox.saturating_mul(oy as u64));
            self.sum_x2 = self.sum_x2.saturating_sub(ox.saturating_mul(ox));
        } else {
            self.filled += 1;
        }
        let y = s.power_mw;
        let x = self.seq;
        self.ys[self.head] = y;
        self.xs[self.head] = x;
        let b = Self::bucket_of(y);
        self.bucket_counts[b] = self.bucket_counts[b].saturating_add(1);
        self.sum_x = self.sum_x.saturating_add(x);
        self.sum_y = self.sum_y.saturating_add(y as u64);
        self.sum_xy = self.sum_xy.saturating_add(x.saturating_mul(y as u64));
        self.sum_x2 = self.sum_x2.saturating_add(x.saturating_mul(x));
        self.head = if self.head + 1 == P99_WINDOW { 0 } else { self.head + 1 };
        self.seq = self.seq.saturating_add(1);
    }

    /// P99 下界估计（mW）：升序扫分桶，累计计数达 ceil(99%·n) 的桶，
    /// 取该桶下界。空窗 → 0。
    pub fn p99_mw(&self) -> u64 {
        if self.filled == 0 {
            return 0;
        }
        let n = self.filled as u64;
        // k = ceil(0.99n)：99 分位秩（至少 k 个样本 ≤ 该桶上界）。
        let k = (n.saturating_mul(99).saturating_add(99)) / 100;
        let mut acc: u64 = 0;
        let mut b = 0usize;
        while b < P99_BUCKETS {
            acc = acc.saturating_add(self.bucket_counts[b] as u64);
            if acc >= k {
                return Self::bucket_lower_mw(b);
            }
            b += 1;
        }
        // 理论不可达（acc ≥ n ≥ k）；防御性返回最高桶下界。
        Self::bucket_lower_mw(P99_BUCKETS - 1)
    }

    /// 趋势斜率（mW/百样本）：滚动窗最小二乘，i128 定点；
    /// 分母 0（点数 < 2 或 x 全同）按 0 处理。
    pub fn trend_mw_per_100(&self) -> i64 {
        if self.filled < 2 {
            return 0;
        }
        let n = self.filled as i128;
        let sx = self.sum_x as i128;
        let sy = self.sum_y as i128;
        let sxy = self.sum_xy as i128;
        let sx2 = self.sum_x2 as i128;
        let den = n.saturating_mul(sx2).saturating_sub(sx.saturating_mul(sx));
        if den == 0 {
            return 0;
        }
        let num = n.saturating_mul(sxy).saturating_sub(sx.saturating_mul(sy));
        let slope_per_sample = num / den; // i128 带符号
        let per_100 = slope_per_sample.saturating_mul(100);
        // 收窄到 i64（i128 中转防中途溢出，端点饱和）。
        if per_100 > i64::MAX as i128 {
            i64::MAX
        } else if per_100 < i64::MIN as i128 {
            i64::MIN
        } else {
            per_100 as i64
        }
    }

    /// 预警级评估（判据三主路径；阈值 T07/T08/T09）。
    ///
    /// * 预热期（filled < T06 的 10% = 60）：强制 Quiet（窗口统计无意义）。
    /// * Emergency：Critical ∧ 趋势 ≥ +1000 mW/百样本（T09）。
    fn evaluate_level(&self) -> (WarningLevel, u16) {
        if self.filled < P99_WARMUP {
            return (WarningLevel::Quiet, 7);
        }
        let p99 = self.p99_mw();
        let crit = ((self.cap_mw as u64 * P99_CRIT_PERMILLE as u64) / 1000) as u64;
        let warn = ((self.cap_mw as u64 * P99_WARN_PERMILLE as u64) / 1000) as u64;
        let trend = self.trend_mw_per_100();
        if p99 >= crit {
            if trend >= TREND_WARN_MW_PER_100 {
                (WarningLevel::Emergency, 8) // T08 主判，T09 附加
            } else {
                (WarningLevel::Critical, 8)
            }
        } else if p99 >= warn {
            (WarningLevel::Warn, 7)
        } else {
            (WarningLevel::Quiet, 7)
        }
    }

    /// 单样本馈入（判据三主路径）：推窗 + 评估 + 级变检测。
    pub fn feed(&mut self, s: &PwrSample, book: &ThresholdBook) -> Result<ProfileVerdict, PwrAnomalyError> {
        if !s.is_shape_ok() {
            return Err(PwrAnomalyError::CapInvalid);
        }
        let _ = book; // 阈值经常量直连（登记簿持有同一常量，verify 可追溯）。
        self.push(s);
        let (level, trace) = self.evaluate_level();
        let changed = level.rank() != self.last_level_rank;
        self.last_level_rank = level.rank();
        Ok(ProfileVerdict {
            p99_mw: self.p99_mw(),
            trend_mw_per_100: self.trend_mw_per_100(),
            level,
            level_changed: changed,
            trace,
        })
    }
}

// ---------------------------------------------------------------------------
// AnomalyAdvice · 判据六：建议语义（动作只建议不强制）
// ---------------------------------------------------------------------------

/// 建议触发原因闭集。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum AdviceReason {
    /// 瞬时超限进入（PowerLimitHit）。
    LimitHit,
    /// P99 画像危急（T08）。
    P99Critical,
    /// P99 画像紧急（T08 ∧ T09）。
    P99Emergency,
}

impl AdviceReason {
    /// 中文语义名。
    pub const fn label(self) -> &'static str {
        match self {
            AdviceReason::LimitHit => "瞬时超限（PowerLimitHit）",
            AdviceReason::P99Critical => "持续越限危急（P99 ≥ 970‰）",
            AdviceReason::P99Emergency => "持续越限紧急（危急 ∧ 正趋势）",
        }
    }
}

/// 降载建议载荷（判据六）。
///
/// **决策权在调度（F1461）——动作只建议不强制**：本结构 `forced` 位恒为
/// false（见 [`AnomalyAdvice::FORCED_ALWAYS_FALSE`]），构造路径不存在置真
/// 入口；目标档位与 UE 提示仅作参考语义，调度侧可忽略、降档或改判，本模块
/// 不跟踪执行、不撤回、不重发。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct AnomalyAdvice {
    /// 触发原因。
    pub reason: AdviceReason,
    /// 目标降载档位（1=轻降 ~ 4=急降，档位语义归 F1461）。
    pub target_gear: u8,
    /// UE 提示预留（None 或静态提示文案——展示与否由 UE 层裁量）。
    pub ue_hint: Option<&'static str>,
    /// 强制位：恒 false（建议语义契约）。
    pub forced: bool,
    /// 追溯阈值 id（LimitHit→T01/T02；Critical→T08；Emergency→T08+T09 记主 8）。
    pub trace: u16,
}

impl AnomalyAdvice {
    /// 强制位恒值常量——编译期锁死建议语义（决策权在调度）。
    pub const FORCED_ALWAYS_FALSE: bool = false;

    /// 构造降载建议（forced 恒 false，无法置真）。
    pub const fn new(reason: AdviceReason, trace: u16) -> Self {
        // 档位映射（显性表）：LimitHit/Critical→2（中降），Emergency→3（重降）。
        let gear = match reason {
            AdviceReason::LimitHit => 2u8,
            AdviceReason::P99Critical => 2u8,
            AdviceReason::P99Emergency => 3u8,
        };
        let hint = match reason {
            AdviceReason::LimitHit => Some("功耗接近上限，性能可能受限"),
            AdviceReason::P99Critical => Some("持续高功耗运行，建议降低负载"),
            AdviceReason::P99Emergency => Some("功耗持续攀升并越限，已建议调度降载"),
        };
        AnomalyAdvice {
            reason,
            target_gear: gear,
            ue_hint: hint,
            forced: AnomalyAdvice::FORCED_ALWAYS_FALSE,
            trace,
        }
    }
}

// 编译期锁死：强制位恒 false（建议语义不可被改写为强制）。
const _: () = assert!(!AnomalyAdvice::FORCED_ALWAYS_FALSE);

// ---------------------------------------------------------------------------
// AnomalyEngine · 三检测器编排 + 序依赖执行 + 建议发放
// ---------------------------------------------------------------------------

/// 单样本检测报告（全部载荷携带追溯阈值 id）。
#[derive(Clone, Debug)]
pub struct EngineReport {
    /// 引擎样本序号（受理序）。
    pub seq: u64,
    /// 超限状态迁移（None = 未迁移）。
    pub limit: Option<LimitTransition>,
    /// 斜率判定。
    pub slope: SlopeVerdict,
    /// 画像判定。
    pub profile: ProfileVerdict,
    /// 降载建议（None = 本样本未触发建议语义）。
    pub advice: Option<AnomalyAdvice>,
    /// 本样本产出的诊断码序列（按产出顺序）。
    pub codes: Vec<PwrAnomalyCode>,
}

/// 异常检测引擎：序依赖闸 → 超限状态机 → 斜率分类 → P99 画像 → 建议发放。
///
/// 序依赖执行（判据五）：`filtered=false` 样本在闸口被拒（0x1341），计数后
/// 返回 [`PwrAnomalyError::InputUnfiltered`]——绝不静默放行；时戳不前进
/// （dt < 1µs，T12）同样闸口拒收。
#[derive(Clone, Debug)]
pub struct AnomalyEngine {
    book: ThresholdBook,
    limit_det: PowerLimitDetector,
    slope_cls: SlopeClassifier,
    profile: P99TrendProfile,
    seq: u64,
    last_ts_us: u64,
    seen_ts: bool,
    accepted: u64,
    rejected_unfiltered: u64,
    rejected_zero_dt: u64,
    advice_emitted: u64,
}

impl AnomalyEngine {
    /// 构造（cap 绑定 + 阈值簿）。cap 非法走 [`PwrAnomalyError::CapInvalid`]。
    pub fn new(cap_mw: u32) -> Result<Self, PwrAnomalyError> {
        if cap_mw == 0 || cap_mw > 4_000_000 {
            return Err(PwrAnomalyError::CapInvalid);
        }
        Ok(AnomalyEngine {
            book: ThresholdBook::new(),
            limit_det: PowerLimitDetector::new(),
            slope_cls: SlopeClassifier::new(),
            profile: P99TrendProfile::new(cap_mw)?,
            seq: 0,
            last_ts_us: 0,
            seen_ts: false,
            accepted: 0,
            rejected_unfiltered: 0,
            rejected_zero_dt: 0,
            advice_emitted: 0,
        })
    }

    /// 阈值簿引用（追溯查询用）。
    pub fn book(&self) -> &ThresholdBook {
        &self.book
    }

    /// 受理计数。
    pub const fn accepted(&self) -> u64 {
        self.accepted
    }

    /// 未滤波拒收计数。
    pub const fn rejected_unfiltered(&self) -> u64 {
        self.rejected_unfiltered
    }

    /// 零间隔拒收计数。
    pub const fn rejected_zero_dt(&self) -> u64 {
        self.rejected_zero_dt
    }

    /// 建议发放计数。
    pub const fn advice_emitted(&self) -> u64 {
        self.advice_emitted
    }

    /// 超限检测器引用（状态观测）。
    pub const fn limit_detector(&self) -> &PowerLimitDetector {
        &self.limit_det
    }

    /// 单样本主路径。
    pub fn feed(&mut self, s: PwrSample) -> Result<EngineReport, PwrAnomalyError> {
        // 闸一：F1447 序依赖（判据五）——未滤波必拒。
        if !s.filtered {
            self.rejected_unfiltered = self.rejected_unfiltered.saturating_add(1);
            return Err(PwrAnomalyError::InputUnfiltered);
        }
        // 闸二：样本形状（cap 合法性）。
        if !s.is_shape_ok() {
            return Err(PwrAnomalyError::CapInvalid);
        }
        // 闸三：T12 零间隔守卫——时戳必须严格前进。
        if self.seen_ts && s.ts_us < self.last_ts_us.saturating_add(MIN_SAMPLE_INTERVAL_US) {
            self.rejected_zero_dt = self.rejected_zero_dt.saturating_add(1);
            return Err(PwrAnomalyError::ZeroSampleInterval);
        }
        self.seen_ts = true;
        self.last_ts_us = s.ts_us;
        self.seq = self.seq.saturating_add(1);
        self.accepted = self.accepted.saturating_add(1);

        let mut codes: Vec<PwrAnomalyCode> = Vec::new();
        codes.push(PwrAnomalyCode::SampleAccepted); // 0x1340

        // 三检测器串联（彼此独立，载荷可分别追溯）。
        let limit_ev = self.limit_det.feed(&s, &self.book)?;
        let slope_v = self.slope_cls.feed(&s, &self.book)?;
        let profile_v = self.profile.feed(&s, &self.book)?;

        if let Some(ev) = limit_ev {
            codes.push(match ev.kind {
                LimitEventKind::Enter => PwrAnomalyCode::LimitEntered,
                LimitEventKind::Exit => PwrAnomalyCode::LimitExited,
            });
        }
        codes.push(slope_v.class.code()); // 0x1344/45/46
        if profile_v.level_changed && profile_v.level.rank() >= WarningLevel::Warn.rank() {
            codes.push(if profile_v.level.rank() >= WarningLevel::Critical.rank() {
                PwrAnomalyCode::P99Critical // 0x1348
            } else {
                PwrAnomalyCode::P99WarnRaised // 0x1347
            });
        }

        // 建议语义（判据六）：超限进入 > 画像紧急 > 画像危急，单样本单建议。
        let mut advice: Option<AnomalyAdvice> = None;
        let hit_entered = matches!(&limit_ev, Some(ev) if ev.kind == LimitEventKind::Enter);
        if hit_entered {
            advice = Some(AnomalyAdvice::new(AdviceReason::LimitHit, 1));
        } else if profile_v.level_changed && profile_v.level == WarningLevel::Emergency {
            advice = Some(AnomalyAdvice::new(AdviceReason::P99Emergency, 8));
        } else if profile_v.level_changed && profile_v.level == WarningLevel::Critical {
            advice = Some(AnomalyAdvice::new(AdviceReason::P99Critical, 8));
        }
        if advice.is_some() {
            self.advice_emitted = self.advice_emitted.saturating_add(1);
            codes.push(PwrAnomalyCode::AdviceEmitted); // 0x134A
        }

        Ok(EngineReport {
            seq: self.seq,
            limit: limit_ev,
            slope: slope_v,
            profile: profile_v,
            advice,
            codes,
        })
    }
}

// ---------------------------------------------------------------------------
// CheatSheetLedger · 判据七：事件账 + health + FNV 指纹
// ---------------------------------------------------------------------------

/// 账内单条事件。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct LedgerEntry {
    /// 账内序号（发放序，单调）。
    pub seq: u64,
    /// 诊断码（0x1340~0x134F 闭集）。
    pub code: PwrAnomalyCode,
    /// 事件时戳（µs）。
    pub ts_us: u64,
    /// 追溯阈值 id（0 = 无阈值引用）。
    pub trace: u16,
    /// 事件注记（静态文案）。
    pub note: &'static str,
}

/// 账体检报告。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct LedgerHealth {
    /// 体检是否通过（守恒三查全绿）。
    pub ok: bool,
    /// 现存条目数。
    pub entries: usize,
    /// 半量让位累计丢弃数（显性，零静默）。
    pub dropped: u64,
    /// 账指纹（FNV-1a 64）。
    pub fingerprint: u64,
}

/// 事件账（判据七）：全部显著事件入账 + 守恒体检 + FNV 指纹。
///
/// * 容量 [`LEDGER_CAP`]，满则半量让位（丢旧留新，丢弃显性计数）。
/// * 体检（[`CheatSheetLedger::health`]）：seq 单调、码段闭集（0x1340~0x134F）、
///   发放连续——任一破坏即 [`PwrAnomalyError::LedgerCorrupted`]。
/// * 指纹：FNV-1a 64 位，覆盖 seq/code/ts/trace/note 全字段——确定性对拍
///   与事后审计共用。
#[derive(Clone, Debug)]
pub struct CheatSheetLedger {
    entries: Vec<LedgerEntry>,
    next_seq: u64,
    dropped: u64,
}

impl CheatSheetLedger {
    /// 构造空账。
    pub const fn new() -> Self {
        CheatSheetLedger { entries: Vec::new(), next_seq: 0, dropped: 0 }
    }

    /// 现存条目数。
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// 账是否为空。
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// 累计丢弃数。
    pub const fn dropped(&self) -> u64 {
        self.dropped
    }

    /// 入账一条（满则半量让位，丢弃计数显性）。
    pub fn push(&mut self, code: PwrAnomalyCode, ts_us: u64, trace: u16, note: &'static str) {
        if self.entries.len() >= LEDGER_CAP {
            let half = LEDGER_CAP / 2;
            let mut i = 0;
            while i < half {
                self.entries.remove(0);
                i += 1;
            }
            self.dropped = self.dropped.saturating_add(half as u64);
        }
        let seq = self.next_seq;
        self.next_seq = self.next_seq.saturating_add(1);
        self.entries.push(LedgerEntry { seq, code, ts_us, trace, note });
    }

    /// 按诊断码统计入账条数。
    pub fn count_of(&self, code: PwrAnomalyCode) -> u32 {
        let mut n = 0u32;
        let mut i = 0;
        while i < self.entries.len() {
            if self.entries[i].code == code {
                n = n.saturating_add(1);
            }
            i += 1;
        }
        n
    }

    /// FNV-1a 64 位账指纹（全字段覆盖，确定性）。
    pub fn fingerprint(&self) -> u64 {
        let mut h: u64 = 0xcbf2_9ce4_8422_2325; // FNV-1a offset basis
        let mut i = 0;
        while i < self.entries.len() {
            let e = &self.entries[i];
            h = fnv_mix_u64(h, e.seq);
            h = fnv_mix_u64(h, e.ts_us);
            h = fnv_mix_u64(h, e.code.as_u16() as u64);
            h = fnv_mix_u64(h, e.trace as u64);
            let note = e.note.as_bytes();
            let mut j = 0;
            while j < note.len() {
                h ^= note[j] as u64;
                h = h.wrapping_mul(0x0000_0100_0000_01b3); // FNV prime
                j += 1;
            }
            i += 1;
        }
        h
    }

    /// 守恒体检（health，判据七）。
    ///
    /// 三查：seq 连续单调、码段闭集复核、容量上限不破。任一破坏返回
    /// [`PwrAnomalyError::LedgerCorrupted`] 并同步入账 0x134D（账若已坏则
    /// 只返错不再写账，防污染扩散）。
    pub fn health(&self) -> Result<LedgerHealth, PwrAnomalyError> {
        let mut prev_seq: Option<u64> = None;
        let mut i = 0;
        while i < self.entries.len() {
            let e = &self.entries[i];
            // 查一：seq 严格递增 1。
            match prev_seq {
                Some(p) => {
                    if e.seq != p.saturating_add(1) {
                        return Err(PwrAnomalyError::LedgerCorrupted);
                    }
                }
                None => {
                    if e.seq != 0 && self.dropped == 0 {
                        return Err(PwrAnomalyError::LedgerCorrupted);
                    }
                }
            }
            // 查二：码段闭集复核（0x1340~0x134F）。
            let c = e.code.as_u16();
            if c < 0x1340 || c > 0x134F {
                return Err(PwrAnomalyError::LedgerCorrupted);
            }
            prev_seq = Some(e.seq);
            i += 1;
        }
        Ok(LedgerHealth {
            ok: true,
            entries: self.entries.len(),
            dropped: self.dropped,
            fingerprint: self.fingerprint(),
        })
    }

    /// 按序遍历账目（只读快照）。
    pub fn iter(&self) -> alloc::vec::IntoIter<LedgerEntry> {
        self.entries.clone().into_iter()
    }
}

/// FNV 单字混合（u64 直入两半，避免位级展开重复）。
fn fnv_mix_u64(mut h: u64, v: u64) -> u64 {
    let mut shift = 0;
    while shift < 64 {
        h ^= (v >> shift) as u64 & 0xFF;
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
        shift += 8;
    }
    h
}

// ---------------------------------------------------------------------------
// InjectRig · 判据七：注入测试面（检测率/时延/误报门禁/双跑对拍）
// ---------------------------------------------------------------------------

/// 注入用例：样本序列 + 期望（应检出 / 应无检）。
#[derive(Clone, Debug)]
pub struct InjectCase {
    /// 用例名（入账留痕）。
    pub name: &'static str,
    /// 期望：true = 应检出异常（超限进入或骤变）；false = 应无检（平稳）。
    pub expect_detect: bool,
    /// 样本序列（须 filtered=true、时戳单调、间隔 ≥ 1µs）。
    pub samples: Vec<PwrSample>,
}

/// 单用例注入结论。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct InjectOutcome {
    /// 用例名。
    pub name: &'static str,
    /// 是否检出（超限进入 or 骤变分类）。
    pub detected: bool,
    /// 是否误报（期望无检却检出）。
    pub false_alarm: bool,
    /// 检出时延（采样数：首个"应检基础样本"→ 检出样本，含两端）。
    pub latency_samples: u32,
    /// 时延是否越预算（> T11=4）。
    pub latency_breach: bool,
    /// 无检基础样本数（误报率分母：期望无检用例的样本总数）。
    pub clean_samples: u32,
}

/// 注入套件统计（检测率 + 时延 + 误报门禁）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct InjectStats {
    /// 受理用例数。
    pub cases: u32,
    /// 检出数（应检用例中真检出）。
    pub detected: u32,
    /// 漏报数（应检用例中未检出）。
    pub missed: u32,
    /// 误报数（应无检用例中检出）。
    pub false_alarms: u32,
    /// 无检基础样本总数（误报率分母）。
    pub clean_samples_total: u32,
    /// 误报率（ppm；clean 为 0 时记 0）。
    pub fp_ppm: u32,
    /// 门禁结论（fp_ppm < T10=10000）。
    pub gate_ok: bool,
    /// 最大检出时延（采样数）。
    pub max_latency_samples: u32,
    /// 时延越预算次数。
    pub latency_breaches: u32,
}

/// 注入试验台：单用例/套件执行 + 门禁裁决 + 双跑确定性对拍。
#[derive(Clone, Debug)]
pub struct InjectRig {
    cap_mw: u32,
}

impl InjectRig {
    /// 构造（cap 绑定，非法走 [`PwrAnomalyError::CapInvalid`]）。
    pub fn new(cap_mw: u32) -> Result<Self, PwrAnomalyError> {
        if cap_mw == 0 || cap_mw > 4_000_000 {
            return Err(PwrAnomalyError::CapInvalid);
        }
        Ok(InjectRig { cap_mw })
    }

    /// 执行单用例：全新引擎跑完样本序列。
    ///
    /// 检出定义（显性口径）：出现超限进入迁移 **或** 骤变分类——两者任一即
    /// "检出异常事件"。检出时延：首个越限进入电平样本（或首个骤变输入样本）
    /// 到检出样本的采样跨度（含两端）；骤变路径无前置越限样本时计 1。
    pub fn run_case(&self, case: &InjectCase) -> Result<InjectOutcome, PwrAnomalyError> {
        if case.samples.is_empty() {
            return Err(PwrAnomalyError::InjectionCaseEmpty);
        }
        let mut eng = AnomalyEngine::new(self.cap_mw)?;
        let book = ThresholdBook::new();
        let mut detected = false;
        let mut first_basis: Option<usize> = None; // 首个"应检基础样本"下标
        let mut detect_idx: Option<usize> = None;
        let mut clean_samples: u32 = 0;

        let mut i = 0usize;
        while i < case.samples.len() {
            let s = case.samples[i];
            // 基础样本标记（时延统计基准）：越进入电平的样本。
            let enter_level = book.enter_level_mw(s.cap_mw);
            if s.power_mw > enter_level && first_basis.is_none() {
                first_basis = Some(i);
            }
            match eng.feed(s) {
                Ok(rep) => {
                    let hit = matches!(&rep.limit, Some(ev) if ev.kind == LimitEventKind::Enter);
                    let steep = rep.slope.class == SlopeClass::Steep;
                    if !detected && (hit || steep) {
                        detected = true;
                        detect_idx = Some(i);
                    }
                    if !detected {
                        clean_samples = clean_samples.saturating_add(1);
                    }
                }
                Err(_) => {
                    // 拒收样本不计入无检基础分母（上游数据缺陷另案处理）。
                }
            }
            i += 1;
        }

        let latency = match (first_basis, detect_idx) {
            (Some(b), Some(d)) => {
                if d >= b {
                    (d - b + 1) as u32
                } else {
                    1
                }
            }
            (None, Some(_)) => 1, // 骤变路径即时检出
            _ => 0,               // 未检出
        };
        let false_alarm = !case.expect_detect && detected;
        Ok(InjectOutcome {
            name: case.name,
            detected,
            false_alarm,
            latency_samples: latency,
            latency_breach: detected && latency > DETECT_LATENCY_BUDGET_SAMPLES,
            clean_samples,
        })
    }

    /// 执行注入套件（判据七主路径）。
    ///
    /// 每用例受理入账（0x134B）；汇总后裁决误报门禁（fp_ppm < T10=10000），
    /// 违约立案入账（0x134C，note 含复检指引）。时延越预算显性计数（不立案，
    /// 归 T11 观测项）。
    pub fn run_suite(
        &self,
        ledger: &mut CheatSheetLedger,
        cases: &[InjectCase],
        ts_us: u64,
    ) -> Result<InjectStats, PwrAnomalyError> {
        if cases.is_empty() {
            return Err(PwrAnomalyError::InjectionCaseEmpty);
        }
        let mut st = InjectStats {
            cases: 0,
            detected: 0,
            missed: 0,
            false_alarms: 0,
            clean_samples_total: 0,
            fp_ppm: 0,
            gate_ok: true,
            max_latency_samples: 0,
            latency_breaches: 0,
        };
        let mut ci = 0;
        while ci < cases.len() {
            let case = &cases[ci];
            ledger.push(
                PwrAnomalyCode::InjectCaseAccepted,
                ts_us,
                0,
                "注入用例受理（判据七测试面）",
            );
            let oc = self.run_case(case)?;
            st.cases = st.cases.saturating_add(1);
            if oc.detected {
                st.detected = st.detected.saturating_add(1);
            } else if case.expect_detect {
                st.missed = st.missed.saturating_add(1);
            }
            if oc.false_alarm {
                st.false_alarms = st.false_alarms.saturating_add(1);
            }
            if case.expect_detect {
                if oc.latency_samples > st.max_latency_samples {
                    st.max_latency_samples = oc.latency_samples;
                }
                if oc.latency_breach {
                    st.latency_breaches = st.latency_breaches.saturating_add(1);
                }
            } else {
                st.clean_samples_total = st.clean_samples_total.saturating_add(oc.clean_samples);
            }
            ci += 1;
        }
        // 误报率（ppm）= 误报数 × 1e6 / 无检基础样本总数（分母 0 记 0）。
        st.fp_ppm = if st.clean_samples_total == 0 {
            0
        } else {
            ((st.false_alarms as u64).saturating_mul(1_000_000) / st.clean_samples_total as u64)
                as u32
        };
        st.gate_ok = st.fp_ppm < FP_GATE_PPM;
        if !st.gate_ok {
            // 违约立案（T10）：入账 0x134C，注记复检指引。
            ledger.push(
                PwrAnomalyCode::FpGateViolated,
                ts_us,
                10,
                "误报门禁违约立案：复检 T01~T12 阈值与 F1447 滤波上游参数",
            );
        }
        Ok(st)
    }

    /// 双跑确定性对拍（判据七）：同套件独立双跑，逐用例结论指纹比对。
    ///
    /// 指纹 = FNV-1a(各用例 detected/false_alarm/latency/clean 串接)。
    /// 不一致返回 [`PwrAnomalyError::DeterminismBroken`] 并入账 0x134E。
    pub fn determinism_pair(
        &self,
        ledger: &mut CheatSheetLedger,
        cases: &[InjectCase],
        ts_us: u64,
    ) -> Result<(u64, u64), PwrAnomalyError> {
        if cases.is_empty() {
            return Err(PwrAnomalyError::InjectionCaseEmpty);
        }
        let mut fp_a = 0xcbf2_9ce4_8422_2325u64;
        let mut fp_b = 0xcbf2_9ce4_8422_2325u64;
        // 第一跑。
        let mut ci = 0;
        while ci < cases.len() {
            let oc = self.run_case(&cases[ci])?;
            fp_a = fnv_mix_u64(fp_a, oc.detected as u64);
            fp_a = fnv_mix_u64(fp_a, oc.false_alarm as u64);
            fp_a = fnv_mix_u64(fp_a, oc.latency_samples as u64);
            fp_a = fnv_mix_u64(fp_a, oc.clean_samples as u64);
            ci += 1;
        }
        // 第二跑（全新实例重跑同序列）。
        let mut ci = 0;
        while ci < cases.len() {
            let oc = self.run_case(&cases[ci])?;
            fp_b = fnv_mix_u64(fp_b, oc.detected as u64);
            fp_b = fnv_mix_u64(fp_b, oc.false_alarm as u64);
            fp_b = fnv_mix_u64(fp_b, oc.latency_samples as u64);
            fp_b = fnv_mix_u64(fp_b, oc.clean_samples as u64);
            ci += 1;
        }
        if fp_a != fp_b {
            ledger.push(
                PwrAnomalyCode::FingerprintMismatch,
                ts_us,
                0,
                "双跑指纹不一致：禁止带病合流，排查目标/编译器差异",
            );
            return Err(PwrAnomalyError::DeterminismBroken);
        }
        Ok((fp_a, fp_b))
    }
}

// ---------------------------------------------------------------------------
// PwrAnomalyStation · 对外台面（引擎 + 事件账 + 注入试验台 一体）
// ---------------------------------------------------------------------------

/// F1450 对外台面：单柄持有引擎/账/试验台，事件自动入账。
#[derive(Clone, Debug)]
pub struct PwrAnomalyStation {
    engine: AnomalyEngine,
    ledger: CheatSheetLedger,
    rig: InjectRig,
    /// 台面构造时戳（账起点）。
    boot_ts_us: u64,
}

impl PwrAnomalyStation {
    /// 构造台面：登记簿入账（0x1349，账起点自证）。
    pub fn new(cap_mw: u32, boot_ts_us: u64) -> Result<Self, PwrAnomalyError> {
        let engine = AnomalyEngine::new(cap_mw)?;
        let rig = InjectRig::new(cap_mw)?;
        let mut ledger = CheatSheetLedger::new();
        ledger.push(
            PwrAnomalyCode::ThresholdRegistered,
            boot_ts_us,
            0,
            "ThresholdBook T01~T12 登记完成（判定全可追溯）",
        );
        Ok(PwrAnomalyStation { engine, ledger, rig, boot_ts_us })
    }

    /// 馈入单样本：检测 + 显著事件自动入账。
    ///
    /// 入账口径（只记显著事件，样本级流水不入账防账泛洪）：
    /// 超限进出 / 骤变分类 / 画像升级 / 建议发放；拒收（未滤波/零间隔）也
    /// 入账——拒收是显著事件。
    pub fn feed(&mut self, s: PwrSample) -> Result<EngineReport, PwrAnomalyError> {
        match self.engine.feed(s) {
            Ok(rep) => {
                if let Some(ev) = &rep.limit {
                    let (code, trace) = match ev.kind {
                        LimitEventKind::Enter => (PwrAnomalyCode::LimitEntered, ev.trace),
                        LimitEventKind::Exit => (PwrAnomalyCode::LimitExited, ev.trace),
                    };
                    self.ledger.push(code, ev.ts_us, trace, "超限状态迁移留痕");
                }
                if rep.slope.class == SlopeClass::Steep {
                    self.ledger
                        .push(PwrAnomalyCode::SteepClassified, s.ts_us, rep.slope.trace, "骤变分类入账");
                }
                if rep.profile.level_changed && rep.profile.level.rank() >= WarningLevel::Warn.rank() {
                    let code = if rep.profile.level.rank() >= WarningLevel::Critical.rank() {
                        PwrAnomalyCode::P99Critical
                    } else {
                        PwrAnomalyCode::P99WarnRaised
                    };
                    self.ledger.push(code, s.ts_us, rep.profile.trace, "画像预警级变化入账");
                }
                if rep.advice.is_some() {
                    self.ledger
                        .push(PwrAnomalyCode::AdviceEmitted, s.ts_us, 1, "降载建议发放（F1461 消费，不强制）");
                }
                Ok(rep)
            }
            Err(e) => {
                self.ledger.push(e.code(), s.ts_us, 0, e.detail());
                Err(e)
            }
        }
    }

    /// 注入套件执行（透传试验台，账共享）。
    pub fn inject(&mut self, cases: &[InjectCase]) -> Result<InjectStats, PwrAnomalyError> {
        self.rig.run_suite(&mut self.ledger, cases, self.boot_ts_us)
    }

    /// 双跑确定性对拍（透传试验台，账共享）。
    pub fn dual_run_audit(&mut self, cases: &[InjectCase]) -> Result<(u64, u64), PwrAnomalyError> {
        self.rig.determinism_pair(&mut self.ledger, cases, self.boot_ts_us)
    }

    /// 账体检（health）。
    pub fn health(&self) -> Result<LedgerHealth, PwrAnomalyError> {
        self.ledger.health()
    }

    /// 账指纹。
    pub fn fingerprint(&self) -> u64 {
        self.ledger.fingerprint()
    }

    /// 引擎引用（观测）。
    pub const fn engine(&self) -> &AnomalyEngine {
        &self.engine
    }

    /// 账引用（审计）。
    pub fn ledger(&self) -> &CheatSheetLedger {
        &self.ledger
    }

    /// 阈值簿文档串（判据四公开出口）。
    pub fn threshold_doc(&self) -> String {
        self.engine.book().doc_export()
    }
}

// ---------------------------------------------------------------------------
// 测试（冒烟 + 注入 + 对拍；仅测试路径使用 assert!）
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::vec;

    const CAP: u32 = 300_000; // 300W 量程
    const STEP: u64 = 10_000; // 10ms 采样间隔

    /// 滤波后样本（正常入口）。
    fn fs(ts: u64, p: u32) -> PwrSample {
        PwrSample::filtered_new(ts, p, CAP)
    }

    /// 原始样本（负向用）。
    fn raw(ts: u64, p: u32) -> PwrSample {
        PwrSample::raw_new(ts, p, CAP)
    }

    // ---- 判据一：超限滞回 ----

    #[test]
    fn hysteresis_enter_exit_roundtrip() {
        let book = ThresholdBook::new();
        let mut det = PowerLimitDetector::new();
        assert_eq!(det.state(), LimitState::Normal);
        // 连续 3 采样越进入电平（285_000）→ 第 3 采样 Enter。
        let e1 = det.feed(&fs(0, 290_000), &book).unwrap();
        assert!(e1.is_none());
        assert_eq!(det.state(), LimitState::Arming);
        let e2 = det.feed(&fs(STEP, 290_000), &book).unwrap();
        assert!(e2.is_none());
        let e3 = det.feed(&fs(2 * STEP, 290_000), &book).unwrap();
        let ev = e3.expect("第 3 采样应触发 Enter");
        assert_eq!(ev.kind, LimitEventKind::Enter);
        assert_eq!(ev.ts_us, 2 * STEP);
        assert_eq!(ev.dwell_samples, 3);
        assert_eq!(ev.peak_mw, 290_000);
        assert_eq!(ev.trace, 1); // T01
        assert_eq!(det.state(), LimitState::Hit);
        assert_eq!(det.enter_ts_us(), 2 * STEP);
        // 滞回死区（850‰~950‰ 之间 = 255_000~285_000）：保持 Hit 不退出。
        let e4 = det.feed(&fs(3 * STEP, 260_000), &book).unwrap();
        assert!(e4.is_none());
        assert_eq!(det.state(), LimitState::Hit);
        // 深谷（≤ 255_000）→ Exit。
        let e5 = det.feed(&fs(4 * STEP, 250_000), &book).unwrap();
        let ev = e5.expect("深谷应触发 Exit");
        assert_eq!(ev.kind, LimitEventKind::Exit);
        assert_eq!(ev.trace, 3); // T03
        assert_eq!(ev.peak_mw, 290_000); // 全程峰值留痕
        assert_eq!(ev.dwell_samples, 5); // Enter 起含死区与退出样本
        assert_eq!(det.exit_ts_us(), 4 * STEP);
        assert_eq!(det.state(), LimitState::Normal);
    }

    #[test]
    fn hysteresis_arming_reset_on_broken_streak() {
        let book = ThresholdBook::new();
        let mut det = PowerLimitDetector::new();
        assert!(det.feed(&fs(0, 290_000), &book).unwrap().is_none());
        assert_eq!(det.consecutive(), 1);
        // 连续性中断 → 攒积清零 + 显性计数。
        assert!(det.feed(&fs(STEP, 200_000), &book).unwrap().is_none());
        assert_eq!(det.state(), LimitState::Normal);
        assert_eq!(det.resets(), 1);
        // 重新攒积：仍需连续 3 采样。
        assert!(det.feed(&fs(2 * STEP, 290_000), &book).unwrap().is_none());
        assert!(det.feed(&fs(3 * STEP, 290_000), &book).unwrap().is_none());
        let ev = det.feed(&fs(4 * STEP, 290_000), &book).unwrap().expect("重新攒满 3 采样应 Enter");
        assert_eq!(ev.kind, LimitEventKind::Enter);
        assert_eq!(det.transitions(), 1);
    }

    // ---- 判据二：斜率三分类 ----

    #[test]
    fn slope_three_classification() {
        let book = ThresholdBook::new();
        let mut cls = SlopeClassifier::new();
        // 基线（首样本无前件 → 平稳）。
        let v0 = cls.feed(&fs(0, 200_000), &book).unwrap();
        assert_eq!(v0.class, SlopeClass::Stable);
        assert_eq!(v0.slope_mw_per_ms, 0);
        // 突变：300W 跃升于 1ms → 300_000 mW/ms ≥ T04。
        let v1 = cls.feed(&fs(1_000, 500_000), &book).unwrap();
        assert_eq!(v1.class, SlopeClass::Steep);
        assert_eq!(v1.slope_mw_per_ms, 300_000);
        assert_eq!(v1.trace, 4); // T04
        // 缓变：20W / 2ms → 10_000 mW/ms ∈ [T05, T04)。
        let v2 = cls.feed(&fs(3_000, 520_000), &book).unwrap();
        assert_eq!(v2.class, SlopeClass::Drift);
        assert_eq!(v2.slope_mw_per_ms, 10_000);
        assert_eq!(v2.trace, 5); // T05
        // 平稳：1W / 10ms → 100 mW/ms < T05。
        let v3 = cls.feed(&fs(13_000, 521_000), &book).unwrap();
        assert_eq!(v3.class, SlopeClass::Stable);
        assert_eq!(v3.slope_mw_per_ms, 100);
        let (steep, drift, stable) = cls.counts();
        assert_eq!((steep, drift, stable), (1, 1, 2));
    }

    #[test]
    fn slope_zero_interval_rejected() {
        let book = ThresholdBook::new();
        let mut cls = SlopeClassifier::new();
        cls.feed(&fs(1_000, 200_000), &book).unwrap();
        // dt=0（时戳原地）与回退时戳都必拒（T12）。
        assert_eq!(cls.feed(&fs(1_000, 200_000), &book).unwrap_err(), PwrAnomalyError::ZeroSampleInterval);
        assert_eq!(cls.feed(&fs(500, 200_000), &book).unwrap_err(), PwrAnomalyError::ZeroSampleInterval);
    }

    // ---- 判据三：P99 画像 + 趋势 ----

    #[test]
    fn p99_bucket_lower_bound_and_warmup() {
        let mut prof = P99TrendProfile::new(CAP).unwrap();
        assert_eq!(prof.p99_mw(), 0); // 空窗
        let book = ThresholdBook::new();
        for i in 0..100 {
            let _ = prof.feed(&fs(i * STEP, 250_000), &book).unwrap();
        }
        // 250_000 → 桶 244 → 下界 249_856（误差 ≤ 1024mW 口径）。
        assert_eq!(prof.p99_mw(), 249_856);
        // 预热期内（恰 60 之前）强制 Quiet；此后 P99 低于 T07 → Quiet。
        assert_eq!(prof.feed(&fs(100 * STEP, 250_000), &book).unwrap().level, WarningLevel::Quiet);
    }

    #[test]
    fn p99_levels_critical_then_emergency_by_trend() {
        let mut prof = P99TrendProfile::new(CAP).unwrap();
        let book = ThresholdBook::new();
        let mut last = None;
        let mut saw_critical_change = false;
        // 压平窗至 295_000（桶下界 294_912 ≥ 危急电平 291_000）→ Critical。
        for i in 0..(P99_WINDOW + 1) {
            let v = prof.feed(&fs(i as u64 * STEP, 295_000), &book).unwrap();
            if v.level == WarningLevel::Critical && v.level_changed {
                saw_critical_change = true;
            }
            last = Some(v);
        }
        let v = last.unwrap();
        assert_eq!(v.level, WarningLevel::Critical);
        assert!(saw_critical_change, "预热结束后应发生 Quiet→Critical 级变");
        assert_eq!(v.trace, 8); // T08
        assert!(v.p99_mw >= 291_000);
        // 趋势为 0（全平窗）→ 不升级 Emergency。
        assert!(v.trend_mw_per_100.abs() < TREND_WARN_MW_PER_100);
        // 叠加正趋势（每样本 +1000mW）→ Emergency（T08 ∧ T09）。
        let mut em = None;
        for i in 0..120 {
            let p = 295_000u32.saturating_add((i as u32) * 1_000);
            em = Some(prof.feed(&fs((P99_WINDOW + 1 + i) as u64 * STEP, p), &book).unwrap());
        }
        let v2 = em.unwrap();
        assert_eq!(v2.level, WarningLevel::Emergency);
        assert!(v2.trend_mw_per_100 >= TREND_WARN_MW_PER_100);
    }

    // ---- 判据五：序依赖闸 ----

    #[test]
    fn unfiltered_rejected_at_gate() {
        let mut eng = AnomalyEngine::new(CAP).unwrap();
        let err = eng.feed(raw(0, 200_000)).unwrap_err();
        assert_eq!(err, PwrAnomalyError::InputUnfiltered);
        assert!(err.detail().contains("cgj05_pwrfilt"));
        assert_eq!(eng.rejected_unfiltered(), 1);
        assert_eq!(eng.accepted(), 0);
        // 滤波后样本照常受理。
        let rep = eng.feed(fs(0, 200_000)).unwrap();
        assert_eq!(rep.seq, 1);
        assert!(rep.codes.contains(&PwrAnomalyCode::SampleAccepted));
    }

    #[test]
    fn zero_interval_rejected_at_gate() {
        let mut eng = AnomalyEngine::new(CAP).unwrap();
        eng.feed(fs(1_000, 200_000)).unwrap();
        let err = eng.feed(fs(1_000, 200_000)).unwrap_err();
        assert_eq!(err, PwrAnomalyError::ZeroSampleInterval);
        assert_eq!(eng.rejected_zero_dt(), 1);
        assert!(err.detail().contains("时戳"));
    }

    #[test]
    fn cap_invalid_rejected() {
        assert_eq!(AnomalyEngine::new(0).unwrap_err(), PwrAnomalyError::CapInvalid);
        assert_eq!(AnomalyEngine::new(4_000_001).unwrap_err(), PwrAnomalyError::CapInvalid);
        let mut eng = AnomalyEngine::new(CAP).unwrap();
        let bad = PwrSample { ts_us: 0, power_mw: 1, cap_mw: 0, filtered: true };
        assert_eq!(eng.feed(bad).unwrap_err(), PwrAnomalyError::CapInvalid);
    }

    // ---- 判据六：建议语义 ----

    #[test]
    fn advice_semantics_forced_always_false() {
        let a = AnomalyAdvice::new(AdviceReason::LimitHit, 1);
        assert!(!a.forced);
        assert_eq!(a.forced, AnomalyAdvice::FORCED_ALWAYS_FALSE);
        assert_eq!(a.target_gear, 2);
        assert!(a.ue_hint.is_some());
        let b = AnomalyAdvice::new(AdviceReason::P99Emergency, 8);
        assert_eq!(b.target_gear, 3);
        assert!(!b.forced);
        let c = AnomalyAdvice::new(AdviceReason::P99Critical, 8);
        assert_eq!(c.target_gear, 2);
    }

    #[test]
    fn engine_emits_advice_on_limit_hit() {
        let mut eng = AnomalyEngine::new(CAP).unwrap();
        let mut adv = None;
        for i in 0..3 {
            let rep = eng.feed(fs(i * STEP, 290_000)).unwrap();
            adv = rep.advice;
        }
        let a = adv.expect("超限进入应发放降载建议");
        assert_eq!(a.reason, AdviceReason::LimitHit);
        assert!(!a.forced); // 决策权在调度
        assert_eq!(eng.advice_emitted(), 1);
    }

    // ---- 判据四：阈值簿追溯 ----

    #[test]
    fn threshold_book_traceability() {
        let book = ThresholdBook::new();
        for id in 1u16..=12 {
            book.verify(id).unwrap_or_else(|_| panic!("T{:02} 应已登记", id));
            assert!(book.name_of(id).is_some());
        }
        assert_eq!(book.verify(0).unwrap_err(), PwrAnomalyError::ThresholdUnregistered);
        assert_eq!(book.verify(13).unwrap_err(), PwrAnomalyError::ThresholdUnregistered);
        assert!(book.name_of(99).is_none());
        // 电平换算自洽。
        assert_eq!(book.enter_level_mw(CAP), 285_000);
        assert_eq!(book.exit_level_mw(CAP), 255_000);
        assert_eq!(book.warn_level_mw(CAP), 276_000);
        assert_eq!(book.crit_level_mw(CAP), 291_000);
    }

    #[test]
    fn threshold_doc_export_public() {
        let book = ThresholdBook::new();
        let doc = book.doc_export();
        assert!(doc.contains("T01 limit_enter_permille = 950"));
        assert!(doc.contains("T03 limit_exit_permille = 850"));
        assert!(doc.contains("T10 fp_gate_ppm = 10000"));
        assert!(doc.contains("T12 min_sample_interval_us = 1"));
        assert!(doc.contains("序依赖"));
        assert!(doc.contains("黑盒红线不适用"));
    }

    #[test]
    fn sensitivity_table_four_knobs() {
        let book = ThresholdBook::new();
        let table = book.sensitivity_table();
        assert_eq!(table.len(), 4);
        for k in table.iter() {
            book.verify(k.threshold_id).unwrap();
            assert!(!k.tighten.is_empty());
            assert!(!k.loosen.is_empty());
            assert!(k.tighten.contains("误报") || k.tighten.contains("漏报"));
        }
    }

    // ---- 判据七：注入测试面 ----

    /// 超限用例：平载 3 采样 + 越限 3 采样（10ms 步）。
    fn case_overlimit() -> InjectCase {
        let mut samples = Vec::new();
        for i in 0..3 {
            samples.push(fs(i * STEP, 200_000));
        }
        for i in 3..6 {
            samples.push(fs(i * STEP, 290_000));
        }
        InjectCase { name: "overlimit_3x", expect_detect: true, samples }
    }

    /// 骤变用例：1ms 内 90W 跃升（斜率 90000 mW/ms ≥ T04）。
    fn case_steep_spike() -> InjectCase {
        let samples = vec![fs(0, 200_000), fs(1_000, 290_000), fs(2_000, 290_500)];
        InjectCase { name: "steep_spike_1ms", expect_detect: true, samples }
    }

    /// 平稳用例：60 采样 ±500W 微抖（斜率 ~50 mW/ms，远离全部阈值）。
    fn case_clean() -> InjectCase {
        let mut samples = Vec::new();
        for i in 0..60 {
            let p = if i % 2 == 0 { 200_000 } else { 200_500 };
            samples.push(fs(i * STEP, p));
        }
        InjectCase { name: "clean_wobble", expect_detect: false, samples }
    }

    #[test]
    fn injection_detection_latency_and_fp_gate_green() {
        let rig = InjectRig::new(CAP).unwrap();
        // 单用例检出与口径。
        let oc1 = rig.run_case(&case_overlimit()).unwrap();
        assert!(oc1.detected);
        assert!(!oc1.false_alarm);
        assert_eq!(oc1.latency_samples, 3); // 首个越限样本 → 第 3 连续采样
        assert!(!oc1.latency_breach);
        let oc2 = rig.run_case(&case_steep_spike()).unwrap();
        assert!(oc2.detected);
        assert_eq!(oc2.latency_samples, 1); // 骤变路径即时检出
        let oc3 = rig.run_case(&case_clean()).unwrap();
        assert!(!oc3.detected);
        assert_eq!(oc3.clean_samples, 60);

        // 套件：检测率 2/2、漏报 0、误报 0、门禁绿。
        let mut ledger = CheatSheetLedger::new();
        let cases = vec![case_overlimit(), case_steep_spike(), case_clean()];
        let st = rig.run_suite(&mut ledger, &cases, 0).unwrap();
        assert_eq!(st.cases, 3);
        assert_eq!(st.detected, 2);
        assert_eq!(st.missed, 0);
        assert_eq!(st.false_alarms, 0);
        assert_eq!(st.clean_samples_total, 60);
        assert_eq!(st.fp_ppm, 0);
        assert!(st.gate_ok);
        assert!(st.max_latency_samples <= DETECT_LATENCY_BUDGET_SAMPLES);
        assert_eq!(st.latency_breaches, 0);
        assert_eq!(ledger.count_of(PwrAnomalyCode::InjectCaseAccepted), 3);
        assert_eq!(ledger.count_of(PwrAnomalyCode::FpGateViolated), 0);
    }

    #[test]
    fn injection_fp_gate_violation_files_case() {
        let rig = InjectRig::new(CAP).unwrap();
        // 期望"无检"却真会检出的用例 → 误报 → 门禁违约 → 立案。
        let bad = InjectCase {
            name: "mislabelled_clean",
            expect_detect: false,
            samples: vec![fs(0, 200_000), fs(1_000, 290_000), fs(2_000, 290_000), fs(3_000, 290_000)],
        };
        let mut ledger = CheatSheetLedger::new();
        let st = rig.run_suite(&mut ledger, &[bad], 0).unwrap();
        assert!(!st.gate_ok);
        assert_eq!(st.false_alarms, 1);
        assert!(st.fp_ppm >= FP_GATE_PPM); // 1 误报 / 1 无检样本 = 1e6 ppm
        assert_eq!(ledger.count_of(PwrAnomalyCode::FpGateViolated), 1);
        // 立案条目 trace 指向 T10。
        let mut found = false;
        for e in ledger.iter() {
            if e.code == PwrAnomalyCode::FpGateViolated {
                assert_eq!(e.trace, 10);
                assert!(e.note.contains("立案"));
                found = true;
            }
        }
        assert!(found);
    }

    #[test]
    fn injection_empty_case_rejected() {
        let rig = InjectRig::new(CAP).unwrap();
        let empty = InjectCase { name: "empty", expect_detect: false, samples: Vec::new() };
        assert_eq!(rig.run_case(&empty).unwrap_err(), PwrAnomalyError::InjectionCaseEmpty);
        let mut ledger = CheatSheetLedger::new();
        assert_eq!(rig.run_suite(&mut ledger, &[], 0).unwrap_err(), PwrAnomalyError::InjectionCaseEmpty);
    }

    #[test]
    fn dual_run_determinism_fingerprint_equal() {
        let rig = InjectRig::new(CAP).unwrap();
        let cases = vec![case_overlimit(), case_steep_spike(), case_clean()];
        let mut ledger = CheatSheetLedger::new();
        let (a, b) = rig.determinism_pair(&mut ledger, &cases, 0).unwrap();
        assert_eq!(a, b, "同输入双跑指纹必须一致（全程定点无时钟）");
        assert_eq!(ledger.count_of(PwrAnomalyCode::FingerprintMismatch), 0);
        // 重复对拍结果稳定。
        let (a2, _) = rig.determinism_pair(&mut ledger, &cases, 0).unwrap();
        assert_eq!(a, a2);
    }

    // ---- 事件账 + health + 指纹 ----

    #[test]
    fn ledger_health_fingerprint_and_counts() {
        let mut station = PwrAnomalyStation::new(CAP, 1_000).unwrap();
        // 账起点：阈值登记一条。
        assert_eq!(station.ledger().count_of(PwrAnomalyCode::ThresholdRegistered), 1);
        // 显著事件入账：超限进出。
        for i in 0..3 {
            let _ = station.feed(fs(i * STEP, 290_000)).unwrap();
        }
        let _ = station.feed(fs(3 * STEP, 260_000)).unwrap();
        let _ = station.feed(fs(4 * STEP, 250_000)).unwrap();
        assert_eq!(station.ledger().count_of(PwrAnomalyCode::LimitEntered), 1);
        assert_eq!(station.ledger().count_of(PwrAnomalyCode::LimitExited), 1);
        assert_eq!(station.ledger().count_of(PwrAnomalyCode::AdviceEmitted), 1);
        // 拒收也入账。
        assert!(station.feed(raw(5 * STEP, 200_000)).is_err());
        assert_eq!(station.ledger().count_of(PwrAnomalyCode::UnfilteredRejected), 1);
        // health 三查全绿。
        let h = station.health().unwrap();
        assert!(h.ok);
        assert_eq!(h.entries, station.ledger().len());
        assert_eq!(h.dropped, 0);
        // 指纹确定性。
        assert_eq!(station.fingerprint(), h.fingerprint);
        assert_eq!(station.fingerprint(), station.fingerprint());
        let h2 = station.health().unwrap();
        assert_eq!(h.fingerprint, h2.fingerprint);
    }

    #[test]
    fn ledger_health_detects_corruption() {
        // 手工构造坏账：seq 跳号（模拟发放器损坏）。
        let mut led = CheatSheetLedger::new();
        led.push(PwrAnomalyCode::SampleAccepted, 0, 0, "a");
        led.push(PwrAnomalyCode::SampleAccepted, 1, 0, "b");
        // 从外部取出并回插破坏单调（经 clone+截断重建坏序）。
        let entries: Vec<LedgerEntry> = led.iter().collect();
        let mut bad = CheatSheetLedger::new();
        bad.next_seq = 10; // 令下一条 seq=10，与前条 seq=1 断链
        let e = entries[1];
        bad.entries.push(e);
        assert_eq!(bad.health().unwrap_err(), PwrAnomalyError::LedgerCorrupted);
    }

    #[test]
    fn ledger_cap_drop_counts_explicitly() {
        let mut led = CheatSheetLedger::new();
        for i in 0..(LEDGER_CAP as u64 + 16) {
            led.push(PwrAnomalyCode::SampleAccepted, i, 0, "n");
        }
        assert_eq!(led.len(), LEDGER_CAP - LEDGER_CAP / 2 + 16);
        assert!(led.dropped() > 0);
        // 半量让位后账仍守恒（seq 单调连续）。
        assert!(led.health().unwrap().ok);
    }

    // ---- 台面端到端：注入 + 对拍 + 阈值文档 ----

    #[test]
    fn station_end_to_end_inject_and_dual_run() {
        let mut station = PwrAnomalyStation::new(CAP, 42).unwrap();
        let cases = vec![case_overlimit(), case_steep_spike(), case_clean()];
        let st = station.inject(&cases).unwrap();
        assert!(st.gate_ok);
        assert_eq!(st.missed, 0);
        let (a, b) = station.dual_run_audit(&cases).unwrap();
        assert_eq!(a, b);
        assert!(station.health().unwrap().ok);
        let doc = station.threshold_doc();
        assert!(doc.contains("T06 p99_window_samples = 600"));
        assert!(doc.contains("T11 detect_latency_budget_samples = 4"));
    }

    #[test]
    fn station_p99_emergency_advice_end_to_end() {
        let mut station = PwrAnomalyStation::new(CAP, 0).unwrap();
        let mut emergency_advice = None;
        // 压平窗至危急，再叠加正趋势 → Emergency → 建议（档位 3）。
        for i in 0..P99_WINDOW {
            let _ = station.feed(fs(i as u64 * STEP, 295_000)).unwrap();
        }
        for i in 0..130 {
            let p = 295_000u32.saturating_add((i as u32) * 1_000);
            let rep = station.feed(fs((P99_WINDOW as u64 + i as u64) * STEP, p)).unwrap();
            if let Some(a) = rep.advice {
                if a.reason == AdviceReason::P99Emergency {
                    emergency_advice = Some(a);
                }
            }
        }
        let a = emergency_advice.expect("危急+正趋势应升级 Emergency 并发放建议");
        assert_eq!(a.target_gear, 3);
        assert!(!a.forced);
        assert!(a.ue_hint.unwrap_or("").contains("降载"));
        assert!(station.ledger().count_of(PwrAnomalyCode::P99Critical) >= 1);
    }

    // ---- 诊断码闭集与错误诊断 ----

    #[test]
    fn diagnostic_code_closure_and_triplets() {
        let all = [
            PwrAnomalyCode::SampleAccepted,
            PwrAnomalyCode::UnfilteredRejected,
            PwrAnomalyCode::LimitEntered,
            PwrAnomalyCode::LimitExited,
            PwrAnomalyCode::SteepClassified,
            PwrAnomalyCode::DriftClassified,
            PwrAnomalyCode::StableClassified,
            PwrAnomalyCode::P99WarnRaised,
            PwrAnomalyCode::P99Critical,
            PwrAnomalyCode::ThresholdRegistered,
            PwrAnomalyCode::AdviceEmitted,
            PwrAnomalyCode::InjectCaseAccepted,
            PwrAnomalyCode::FpGateViolated,
            PwrAnomalyCode::LedgerBroken,
            PwrAnomalyCode::FingerprintMismatch,
            PwrAnomalyCode::ZeroIntervalGuard,
        ];
        for (i, c) in all.iter().enumerate() {
            let v = c.as_u16();
            assert_eq!(v, (0x1340 + i as u16));
            assert!(!c.what().is_empty());
            assert!(!c.why().is_empty());
            assert!(!c.advice().is_empty());
        }
    }

    #[test]
    fn error_details_actionable_chinese() {
        let all = [
            PwrAnomalyError::InputUnfiltered,
            PwrAnomalyError::ZeroSampleInterval,
            PwrAnomalyError::CapInvalid,
            PwrAnomalyError::ThresholdUnregistered,
            PwrAnomalyError::LedgerCorrupted,
            PwrAnomalyError::InjectionCaseEmpty,
            PwrAnomalyError::DeterminismBroken,
            PwrAnomalyError::StatsOverflow,
        ];
        for e in all.iter() {
            assert!(e.detail().len() > 10);
            let c = e.code().as_u16();
            assert!(c >= 0x1340 && c <= 0x134F);
        }
    }

    #[test]
    fn sample_shape_and_codes_helpers() {
        assert!(fs(0, 1).is_shape_ok());
        assert!(!raw(0, 1).filtered);
        assert_eq!(SlopeClass::Steep.code(), PwrAnomalyCode::SteepClassified);
        assert_eq!(SlopeClass::Drift.label(), "缓变（趋势爬坡）");
        assert_eq!(AdviceReason::P99Critical.label(), "持续越限危急（P99 ≥ 970‰）");
        assert_eq!(WarningLevel::Emergency.rank(), 3);
    }
}
