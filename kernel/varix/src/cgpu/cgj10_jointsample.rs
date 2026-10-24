//! CGPU-F1451 · 功耗与温度联合采样（CGPU-J 域 · 功耗域 · 目标 300 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/CGPU Varix STAR II · 总纲与施工书.md#CGPU-F1451`
//!
//! **判据（锚点原文）**：同拍对齐、联合画像、交叉规则、优先级表、开销合并、缺失降级、测试、判据。
//!
//! **职责定位（锚点原文摘录）**：联合采样：同步采样（功耗+温度+频率三通道同拍对齐——单次轮询批量读——时序偏差<1ms）；联合画像（功耗-温度散点画像——热模型 F1473 输入：功耗→温升响应曲线）；交叉验证（功耗骤升+温度缓升→正常；功耗平+温度骤升→散热异常——交叉规则表）；传感器优先级（温度传感多源——取 GPU 结温优先、VRM 次之——优先级表）；采样开销合并（联合采样单次开销≤0.3ms——分立采样开销对比归档）；缺失对齐（某通道缺失→单通道模式+降级标注——不造假补齐）；测试（对齐精度/交叉规则/缺失降级三组）。
//!
//! 本模块真实语义密度（全部纯功能，零 IO 零墙钟零浮点）：
//!
//! 1. **同拍三元组与对齐账**（判据一）：[`JointSample`]——功耗 mW / 温度
//!    m°C / 频率 kHz 三通道定长编码 + 同拍时戳（取三通道读取时戳之最晚者）
//!    + 时序偏差账（偏差 = max−min，T01 阈 1000µs = 1ms 判据自证）；偏差
//!    落入 T02 预警带（≥800µs）显性预警、超 T01 的拍 [`JointSample::aligned`]
//!    = false（[`JointSampCode::BeatSkewed`] 留痕），绝不冒充同拍入画像。
//!    [`build_joint_sample`] 单次轮询批量收帧：三通道一次进参、一次出样。
//! 2. **联合画像**（判据二）：[`JointProfiler`]——16×16 功耗×温度散点桶
//!    矩阵（T08/T09）+ 每功耗桶温度直方图导出分位数带（P25/P50/P75 整数
//!    分位）→ [`ResponseCurve`] 功耗→温升响应曲线；单调性审计
//!    （[`JointProfiler::audit_monotonic`]——相邻非空功耗桶的中位温度桶倒挂
//!    超 T20 容忍即违约立案）+ [`HotModelInput`] F1473 热模型输入接口声明。
//! 3. **交叉规则表**（判据三）：[`CrossRuleEngine`] + [`CROSS_RULES`]
//!    const 四规则——功耗骤升+温度缓升→正常（热滞后）/功耗平+温度骤升→
//!    散热异常/双骤升→瞬时过载/双平→稳态；斜率分类闭集（[`PowerClass`]/
//!    [`TempClass`]，T12~T15 阈值表）；未匹配组合显性 Unknown 立案不猜；
//!    判定留痕 [`VerdictRecord`]（规则 id/斜率/类别/痕迹全程可追溯）。
//! 4. **传感器优先级**（判据四）：[`SENSOR_PRIORITY`] const 表——GPU 结温
//!    （rank3）> VRM（rank2）> 环境温度（rank1）；同拍多源消解取最高优先
//!    级源（[`resolve_temp_source`]）+ 源切换账 [`SensorSwitchLedger`]
//!    （切换/降源回退/逐源选用计数）。
//! 5. **开销合并账**（判据五）：[`OverheadLedger`]——联合单次开销 ≤0.3ms
//!    = 300µs（T03）逐拍判据 + 分立三次采样开销对比归档（3×单通道 130µs
//!    + 分发 45µs = 435µs vs 联合 270µs——节省倍率账 permille 归档导出）。
//! 6. **缺失降级**（判据六）：[`MissingDegradation`]——连续 T16 宽限拍内
//!    缺通道仅计数，超宽限进入单通道模式（[`DegradeState::SingleChannel`]）
//!    + 降级标注沿画像/规则/报告全链传播；**不造假补齐红线**：哨兵值
//!    （T21）注入一律拒收（[`JointSampError::FakeFillSentinel`]）并立案；
//!    连续 T12 恢复拍自动回双通道。
//! 7. **事件账与对拍**（判据七）：[`EventLedger`] 事件账（容量 T18、半量
//!    让位、seq 连续性自证）+ `health()` 账体检 + FNV-1a 指纹 + 双跑确定性
//!    对拍（[`JointSampStation::determinism_pair`]）+ 注入面
//!    [`InjectRig`]（超偏差注入/通道缺失注入/交叉规则违例注入 + 干净序列
//!    误报率 ppm 门禁 T19）。
//!
//! 诊断码段：**0x1C20~0x1C2F**（本文件独占，开工前 grep 确认空闲）。
//!
//! 序依赖契约：本模块输出供 **F1473**（热模型，[`HotModelInput`] 消费方）
//! 与 F1448（事件总线）下游消费；上游与 F1450（功耗异常检测）平级同源
//! （同读 F1441 采样节拍），交叉规则的"功耗骤升"分类与 F1450 斜率分类
//! 阈值同表（T12/T13），保证两域判定口径一致。缺失/超偏差拍不做交叉判定
//! ——判定留痕里 degraded/aligned 双标志位自证，绝不静默放行。

#![forbid(unsafe_code)]

use alloc::format;
use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 常量区（数值全部写死，注释标注来源）
// ---------------------------------------------------------------------------

/// 同拍时序偏差判据：1000µs（= 1ms）。
/// 来源：锚点原文"三通道同拍对齐……时序偏差<1ms"。
pub const ALIGN_TOLERANCE_US: u32 = 1_000; // T01

/// 时序偏差预警带下界：800µs（判据内、但接近越界的拍显性预警）。
/// 来源：F1451 施工细则——对齐精度判据需提前预警档，取判据值的 80%。
pub const SKEW_WARN_US: u32 = 800; // T02

/// 联合采样单次开销预算：300µs（= 0.3ms）。
/// 来源：锚点原文"联合采样单次开销≤0.3ms"。
pub const JOINT_BUDGET_US: u32 = 300; // T03

/// 分立采样单通道读数开销：130µs/通道。
/// 来源：F1441 采样节拍标定——单通道寄存器轮询+换算典型 130µs；
/// 分立三次采样 = 3×130 + 分发开销（T05）。
pub const DISCRETE_PER_CHANNEL_US: u32 = 130; // T04

/// 分立采样每次分发/上下文开销：45µs。
/// 来源：F1441 节拍标定——三次独立分发共 3 次，每次 45µs。
pub const DISCRETE_DISPATCH_US: u32 = 45; // T05

/// 联合采样单次分发开销：60µs。
/// 来源：锚点"单次轮询批量读"——一次分发 60µs（略高于单通道分发，
/// 因需装帧三通道缓冲，但仅付一次）。
pub const JOINT_DISPATCH_US: u32 = 60; // T06

/// 联合采样批量读数开销：210µs。
/// 来源：锚点"单次轮询批量读"——三通道连读 210µs（< 3×130=390，
/// 批量化节省来自页寄存器驻留，不重复走通道切换）。
pub const JOINT_BATCH_READ_US: u32 = 210; // T07

/// 功耗桶数：16（散点画像横轴）。
/// 来源：锚点原文"功耗-温度散点画像"——16×16 桶矩阵（施工细则定档）。
pub const BUCKET_P: usize = 16; // T08

/// 温度桶数：16（散点画像纵轴，兼作每功耗桶内温度直方图档数）。
/// 来源：同 T08——16×16 桶矩阵。
pub const BUCKET_T: usize = 16; // T09

/// 功耗量程：0~500_000 mW（500W）。
/// 来源：F1450 功耗域同量程（cap 标定 300W 典型 + 1.67 倍裕度）。
pub const P_RANGE_MW: u32 = 500_000; // T10

/// 温度量程：0~150_000 m°C（150°C）。
/// 来源：GPU 结温告警上限 105°C + 关机阈值 120°C，量程取 150°C 覆盖。
pub const T_RANGE_MDEGC: u32 = 150_000; // T11

/// 功耗"平"斜率上界：1500 mW/ms（≤ 此值视为平/降）。
/// 来源：与 F1450 T05 同源——典型调频爬坡 ~1.5W/ms 不算骤升。
pub const PWR_FLAT_SLOPE_MW_PER_MS: i64 = 1_500; // T12

/// 功耗"骤升"斜率下界：15000 mW/ms（≥ 此值视为骤升）。
/// 来源：与 F1450 T04 同表——帧间突变档 15W/ms。
pub const PWR_STEEP_SLOPE_MW_PER_MS: i64 = 15_000; // T13

/// 温度"平"斜率上界：2 m°C/ms（= 2°C/s）。
/// 来源：芯片自然升温典型 0.5~2°C/s；≤2 m°C/ms 视为平。
pub const TEMP_FLAT_SLOPE_MDEGC_PER_MS: i64 = 2; // T14

/// 温度"骤升"斜率下界：12 m°C/ms（= 12°C/s）。
/// 来源：散热失效（风扇停转/风道堵）时结温爬升典型 ≥10°C/s，取 12
/// 为骤升下界——正常热滞后（功耗骤升温度仅缓升）不落入此档。
pub const TEMP_STEEP_SLOPE_MDEGC_PER_MS: i64 = 12; // T15

/// 缺通道宽限拍数：连续 3 拍缺失才进入单通道模式。
/// 来源：锚点"某通道缺失→单通道模式"——瞬时读失败（总线重试）不立即
/// 降级，连续 3 拍（@10ms = 30ms）缺失才认定通道不可用。
pub const MISSING_GRACE_BEATS: u32 = 3; // T16

/// 恢复确认拍数：连续 8 拍全通道在位才回双通道。
/// 来源：锚点"恢复自动回双通道"——恢复需 80ms 稳定在位防抖动往返。
pub const RECOVERY_CONFIRM_BEATS: u32 = 8; // T17

/// 事件账容量：512 条，满后半量让位。
/// 来源：与 F1450 账面同级（内核路径禁动态膨胀，容量写死）。
pub const LEDGER_CAP: usize = 512; // T18

/// 误报率门禁：10000 ppm（= 1%）。
/// 来源：锚点测试判据——F1450 同款门禁口径（误报率 <1%）。
pub const FP_PPM_GATE: u64 = 10_000; // T19

/// 响应曲线单调性容忍：相邻非空功耗桶中位温度桶最多倒挂 1 桶。
/// 来源：量化噪声容忍——桶宽 9375m°C，单桶倒挂属量化抖动，≥2 桶为真违约。
pub const MONO_TOLERANCE_BUCKETS: i64 = 1; // T20

/// 造假补齐哨兵值：任何通道读到此值视为造假注入，一律拒收。
/// 来源：锚点红线"不造假补齐"——内核侧约定哨兵常量，采样帧中出现即立案。
pub const FAKE_FILL_SENTINEL: u32 = 0xFFFF_FFFF; // T21

/// FNV-1a 偏移基。
/// 来源：FNV-1a 64bit 标准参数。
pub const FNV_OFFSET: u64 = 0xcbf2_9ce4_8422_2325; // T22

/// FNV-1a 素数。
/// 来源：同 T22。
pub const FNV_PRIME: u64 = 0x0000_0100_0000_01b3; // T23

/// 分立三次采样总开销（const 判据自证）：3×130 + 45 = 435µs。
pub const fn discrete_total_us() -> u32 {
    DISCRETE_PER_CHANNEL_US * 3 + DISCRETE_DISPATCH_US
}

/// 联合采样名义开销（const 判据自证）：60 + 210 = 270µs ≤ 300µs 预算。
pub const fn joint_nominal_us() -> u32 {
    JOINT_DISPATCH_US + JOINT_BATCH_READ_US
}

// 编译期判据自证：联合名义开销必须落在预算内，且必须真省于分立。
const _: () = assert!(JOINT_BUDGET_US == 300);
const _: () = assert!(ALIGN_TOLERANCE_US == 1_000);
const _: () = assert!(joint_nominal_us() <= JOINT_BUDGET_US);
const _: () = assert!(discrete_total_us() > joint_nominal_us());
const _: () = assert!(BUCKET_P == 16 && BUCKET_T == 16);
const _: () = assert!(LEDGER_CAP >= 64 && (LEDGER_CAP & 1) == 0);
const _: () = assert!(SKEW_WARN_US < ALIGN_TOLERANCE_US);

// ---------------------------------------------------------------------------
// 通道与采样帧
// ---------------------------------------------------------------------------

/// 采样通道三元组。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Channel {
    /// 功耗通道（mW）。
    Power = 0,
    /// 温度通道（m°C，多源，按 [`SENSOR_PRIORITY`] 消解）。
    Temp = 1,
    /// 频率通道（kHz）。
    Freq = 2,
}

impl Channel {
    /// 通道在在位掩码中的位。
    pub const fn mask_bit(self) -> u8 {
        1u8 << (self as u8)
    }

    /// 通道下标（0/1/2）。
    pub const fn index(self) -> usize {
        self as usize
    }

    /// 通道名（诊断用）。
    pub fn label(self) -> &'static str {
        match self {
            Channel::Power => "功耗(mW)",
            Channel::Temp => "温度(m°C)",
            Channel::Freq => "频率(kHz)",
        }
    }
}

/// 全通道在位掩码（功耗|温度|频率 = 0b111）。
pub const FULL_MASK: u8 = 0b111;

/// 通道优先级消解后的温度源 id 常量（与 [`SENSOR_PRIORITY`] 一一对应）。
pub const SRC_GPU_DIE: u8 = 0;
/// VRM 温度源 id。
pub const SRC_VRM: u8 = 1;
/// 环境温度源 id。
pub const SRC_AMBIENT: u8 = 2;

/// 在位掩码是否含某通道。
pub fn mask_has(mask: u8, ch: Channel) -> bool {
    mask & ch.mask_bit() != 0
}

/// 缺失掩码（在位掩码取反后的全通道域）。
pub fn mask_missing(mask: u8) -> u8 {
    !mask & FULL_MASK
}

/// 在位掩码的可读标签，如 "P+T+F" / "P+F"。
pub fn mask_label(mask: u8) -> String {
    let mut s = String::new();
    if mask_has(mask, Channel::Power) {
        s.push('P');
    }
    if mask_has(mask, Channel::Temp) {
        if !s.is_empty() {
            s.push('+');
        }
        s.push('T');
    }
    if mask_has(mask, Channel::Freq) {
        if !s.is_empty() {
            s.push('+');
        }
        s.push('F');
    }
    if s.is_empty() {
        s.push_str("空");
    }
    s
}

/// 单通道读数（联合采样的进参原子）。
///
/// `source_id` 仅温度通道有意义（0=GPU 结温 / 1=VRM / 2=环境温度），
/// 其余通道恒填 0。`tsc_us` 为本通道读取时戳——三通道时戳之差即同拍
/// 时序偏差账的原始输入。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ChannelReading {
    /// 所属通道。
    pub channel: Channel,
    /// 温度源 id（非温度通道恒 0）。
    pub source_id: u8,
    /// 通道值（mW / m°C / kHz，定长 u32 编码）。
    pub value_u32: u32,
    /// 本通道读取时戳（µs，平台单调计数）。
    pub tsc_us: u64,
}

impl ChannelReading {
    /// 功耗读数。
    pub fn power_mw(value_mw: u32, tsc_us: u64) -> Self {
        ChannelReading { channel: Channel::Power, source_id: 0, value_u32: value_mw, tsc_us }
    }

    /// 温度读数（带源 id）。
    pub fn temp(source_id: u8, value_mdegc: u32, tsc_us: u64) -> Self {
        ChannelReading { channel: Channel::Temp, source_id, value_u32: value_mdegc, tsc_us }
    }

    /// 频率读数。
    pub fn freq_khz(value_khz: u32, tsc_us: u64) -> Self {
        ChannelReading { channel: Channel::Freq, source_id: 0, value_u32: value_khz, tsc_us }
    }
}

/// 同拍三元组样本（判据一的载体）。
///
/// 定长编码：功耗 mW / 温度 m°C / 频率 kHz 各 u32；`beat_tsc_us` 为同拍
/// 时戳（取三通道读取时戳之最晚者——以最晚通道为准，保证无通道"先于拍"
/// 被记）；`skew_us` 为三通道时戳极差（时序偏差账）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct JointSample {
    /// 拍序号（站内单调递增）。
    pub beat_seq: u64,
    /// 同拍时戳（三通道时戳最大值，µs）。
    pub beat_tsc_us: u64,
    /// 功耗（mW）。
    pub power_mw: u32,
    /// 温度（m°C，优先级消解后的单值）。
    pub temp_mdegc: u32,
    /// 频率（kHz）。
    pub freq_khz: u32,
    /// 通道在位掩码（bit0 功耗 / bit1 温度 / bit2 频率）。
    pub present: u8,
    /// 温度选中源 id（无温度通道时 None）。
    pub temp_source_id: Option<u8>,
    /// 时序偏差（三通道时戳极差，µs）。
    pub skew_us: u32,
    /// 同拍对齐判定：skew_us ≤ T01 才为 true（超偏差拍显性标注不冒充同拍）。
    pub aligned: bool,
    /// 降级标注：任一通道缺失即为 true（全链传播）。
    pub degraded: bool,
}

impl JointSample {
    /// 是否含某通道。
    pub fn has(&self, ch: Channel) -> bool {
        mask_has(self.present, ch)
    }

    /// 缺失通道掩码。
    pub fn missing_mask(&self) -> u8 {
        mask_missing(self.present)
    }

    /// 同拍且全通道——只有这种拍才有资格做交叉判定与入画像。
    pub fn is_same_beat(&self) -> bool {
        self.aligned && !self.degraded
    }

    /// 缺失通道的可读说明（降级标注用）。
    pub fn missing_note(&self) -> String {
        let miss = self.missing_mask();
        if miss == 0 {
            return String::from("无缺失");
        }
        let mut s = format!("缺失[{}]", mask_label(miss));
        if mask_has(miss, Channel::Temp) {
            s.push_str("→单通道模式");
        }
        s
    }
}

/// 单次轮询批量收帧：三通道读数一次进参、一次出样。
///
/// 判据一的核心路径：
/// - 温度多源先按 [`SENSOR_PRIORITY`] 消解为单值（同拍只允许一个温度值）；
/// - 三通道时戳极差即时序偏差账（超 u32 域显性报错，不静默截断）；
/// - 哨兵值（T21）一律拒收——不造假补齐红线的第一道闸；
/// - 对齐判定 skew ≤ T01；降级判定 present ≠ 全通道。
pub fn build_joint_sample(
    beat_seq: u64,
    readings: &[ChannelReading],
) -> Result<JointSample, JointSampError> {
    if readings.is_empty() {
        return Err(JointSampError::EmptyBeat);
    }
    // 红线闸：哨兵值（造假补齐注入）在任何通道出现都直接拒收并立案。
    for r in readings {
        if r.value_u32 == FAKE_FILL_SENTINEL {
            return Err(JointSampError::FakeFillSentinel { channel: r.channel as u8 });
        }
    }
    // 温度多源消解：同拍多源同采，只取优先级最高源的单值。
    let mut temp_candidates: Vec<(u8, u32, u64)> = Vec::new();
    for r in readings {
        if r.channel == Channel::Temp {
            temp_candidates.push((r.source_id, r.value_u32, r.tsc_us));
        }
    }
    let mut present = 0u8;
    let mut power = 0u32;
    let mut temp = 0u32;
    let mut freq = 0u32;
    let mut temp_source = None;
    let mut min_tsc = u64::MAX;
    let mut max_tsc = 0u64;
    match resolve_temp_source(&temp_candidates) {
        Some((src, value, tsc)) => {
            temp = value;
            temp_source = Some(src.id);
            present |= Channel::Temp.mask_bit();
            if tsc < min_tsc {
                min_tsc = tsc;
            }
            if tsc > max_tsc {
                max_tsc = tsc;
            }
        }
        // 无温度源或全部来源未登记：温度通道按缺失处理，不造假补齐。
        None => {}
    }
    for r in readings {
        if r.channel == Channel::Temp {
            continue; // 已由优先级消解处理
        }
        match r.channel {
            Channel::Power => {
                power = r.value_u32;
                present |= Channel::Power.mask_bit();
            }
            Channel::Freq => {
                freq = r.value_u32;
                present |= Channel::Freq.mask_bit();
            }
            Channel::Temp => {}
        }
        if r.tsc_us < min_tsc {
            min_tsc = r.tsc_us;
        }
        if r.tsc_us > max_tsc {
            max_tsc = r.tsc_us;
        }
    }
    if present == 0 {
        return Err(JointSampError::EmptyBeat);
    }
    let span = max_tsc - min_tsc; // present != 0 ⇒ min/max 均已落值
    if span > u32::MAX as u64 {
        return Err(JointSampError::SkewOverflow { span_us: span });
    }
    let skew = span as u32;
    Ok(JointSample {
        beat_seq,
        beat_tsc_us: max_tsc,
        power_mw: power,
        temp_mdegc: temp,
        freq_khz: freq,
        present,
        temp_source_id: temp_source,
        skew_us: skew,
        aligned: skew <= ALIGN_TOLERANCE_US,
        degraded: present != FULL_MASK,
    })
}

/// 桶下标换算（const fn，纯整数）：value * buckets / range，越界落末桶。
///
/// 用 u64 中间量防 (500_000×16) 级乘法溢出；显性 saturating 语义——
/// 超量程值计入末桶而非丢弃（画像不丢点）。
pub const fn bucket_of(value: u32, range: u32, buckets: usize) -> usize {
    let v = value as u64;
    let r = range as u64;
    let b = buckets as u64;
    let idx = v.saturating_mul(b) / r;
    if idx >= b {
        (b - 1) as usize
    } else {
        idx as usize
    }
}

/// 温度桶中心值换算（m°C）：桶下标 → 桶中心。
/// 桶宽 = 150000/16 = 9375 m°C；中心 = idx×9375 + 半桶 4687。
pub const fn temp_bucket_center(idx: usize) -> u32 {
    let width = T_RANGE_MDEGC as u64 / BUCKET_T as u64;
    let half = T_RANGE_MDEGC as u64 / (BUCKET_T as u64 * 2);
    (idx as u64 * width + half) as u32
}

// ---------------------------------------------------------------------------
// 诊断码（0x1C20~0x1C2F，本文件独占）与错误枚举
// ---------------------------------------------------------------------------

/// F1451 诊断事件码（16 个，闭集，段 0x1C20~0x1C2F 独占）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum JointSampCode {
    /// 拍对齐入账（偏差 ≤ T01）。
    BeatAligned = 0x1C20,
    /// 拍偏差预警档（T02 ≤ skew < T01）或超差（skew ≥ T01，note 区分）。
    BeatSkewed = 0x1C21,
    /// 拍内通道缺失（逐拍计数，宽限期内仅计数不降级）。
    ChannelMissing = 0x1C22,
    /// 进入单通道模式（连续 T16 拍缺失）。
    DegradedSingleChannel = 0x1C23,
    /// 恢复双通道（连续 T17 拍全通道在位）。
    RecoveredDualChannel = 0x1C24,
    /// 交叉规则命中（R1~R4 之一，aux = 规则 id）。
    CrossRuleMatched = 0x1C25,
    /// 交叉规则未匹配（类别组合不在表内，显性 Unknown 不猜）。
    CrossRuleUnmatched = 0x1C26,
    /// 温度源切换（aux = 新源 id）。
    SensorSourceSwitched = 0x1C27,
    /// 温度降源回退（最高优先级源缺失、启用次级源）。
    SensorFallbackUsed = 0x1C28,
    /// 画像桶首次落点（桶坐标入 aux 高低位）。
    ProfileBucketUpdated = 0x1C29,
    /// 响应曲线单调性违约（中位温度桶倒挂超 T20 容忍）。
    CurveMonotonicViolated = 0x1C2A,
    /// 联合开销在预算内（预算恢复/首拍入账，逐拍不刷账）。
    OverheadBudgetOk = 0x1C2B,
    /// 联合开销超预算（> T03，逐拍立案）。
    OverheadBudgetExceeded = 0x1C2C,
    /// 造假补齐注入被拒收（哨兵值红线立案）。
    FakeFillRejected = 0x1C2D,
    /// 双跑指纹不一致（确定性破约）。
    FingerprintMismatch = 0x1C2E,
    /// 账面自证失败（seq 断链/计数不守恒）。
    LedgerCorrupted = 0x1C2F,
}

impl JointSampCode {
    /// 诊断码数值（闭集自证：0x1C20 + 序）。
    pub const fn as_u16(self) -> u16 {
        self as u16
    }

    /// 是什么。
    pub fn what(self) -> &'static str {
        match self {
            JointSampCode::BeatAligned => "拍同拍对齐入账",
            JointSampCode::BeatSkewed => "拍时序偏差预警或超差",
            JointSampCode::ChannelMissing => "拍内通道缺失计数",
            JointSampCode::DegradedSingleChannel => "进入单通道降级模式",
            JointSampCode::RecoveredDualChannel => "恢复双通道模式",
            JointSampCode::CrossRuleMatched => "交叉规则命中",
            JointSampCode::CrossRuleUnmatched => "交叉规则未匹配",
            JointSampCode::SensorSourceSwitched => "温度源切换",
            JointSampCode::SensorFallbackUsed => "温度降源回退",
            JointSampCode::ProfileBucketUpdated => "画像桶首次落点",
            JointSampCode::CurveMonotonicViolated => "响应曲线单调性违约",
            JointSampCode::OverheadBudgetOk => "联合开销在预算内",
            JointSampCode::OverheadBudgetExceeded => "联合开销超预算",
            JointSampCode::FakeFillRejected => "造假补齐注入被拒收",
            JointSampCode::FingerprintMismatch => "双跑指纹不一致",
            JointSampCode::LedgerCorrupted => "账面自证失败",
        }
    }

    /// 为什么。
    pub fn why(self) -> &'static str {
        match self {
            JointSampCode::BeatAligned => "三通道时戳极差 ≤ 1ms 判据",
            JointSampCode::BeatSkewed => "三通道时戳极差进入预警带或超判据",
            JointSampCode::ChannelMissing => "该拍轮询未收到某通道读数",
            JointSampCode::DegradedSingleChannel => "连续 3 拍缺失，通道认定不可用",
            JointSampCode::RecoveredDualChannel => "连续 8 拍全通道在位，防抖确认通过",
            JointSampCode::CrossRuleMatched => "功耗/温度类别组合命中四规则之一",
            JointSampCode::CrossRuleUnmatched => "类别组合不在规则表内，不猜",
            JointSampCode::SensorSourceSwitched => "同拍选中源与上拍不同",
            JointSampCode::SensorFallbackUsed => "最高优先级源缺失，启用次级源",
            JointSampCode::ProfileBucketUpdated => "散点桶从空变非空",
            JointSampCode::CurveMonotonicViolated => "相邻功耗桶中位温度倒挂超容忍",
            JointSampCode::OverheadBudgetOk => "联合单次开销 ≤ 300µs 判据",
            JointSampCode::OverheadBudgetExceeded => "联合单次开销 > 300µs 判据",
            JointSampCode::FakeFillRejected => "通道读数含造假补齐哨兵值",
            JointSampCode::FingerprintMismatch => "同输入双跑状态指纹不等",
            JointSampCode::LedgerCorrupted => "seq 断链或计数不守恒",
        }
    }

    /// 怎么办。
    pub fn advice(self) -> &'static str {
        match self {
            JointSampCode::BeatAligned => "无需处理",
            JointSampCode::BeatSkewed => "检查总线仲裁与轮询顺序；超差拍不入画像",
            JointSampCode::ChannelMissing => "核对通道寄存器就绪位与重试策略",
            JointSampCode::DegradedSingleChannel => "按单通道模式消费：缺失值不得伪造",
            JointSampCode::RecoveredDualChannel => "恢复正常联合消费",
            JointSampCode::CrossRuleMatched => "按规则动作语义消费（建议不强制）",
            JointSampCode::CrossRuleUnmatched => "扩充规则表前先归档该组合样本",
            JointSampCode::SensorSourceSwitched => "核对多源读数帧装配顺序",
            JointSampCode::SensorFallbackUsed => "检查最高优先级源链路；降源期画像带降级标注",
            JointSampCode::ProfileBucketUpdated => "无需处理",
            JointSampCode::CurveMonotonicViolated => "核查散热路径与传感器标定后再入 F1473",
            JointSampCode::OverheadBudgetOk => "无需处理",
            JointSampCode::OverheadBudgetExceeded => "压缩批量读窗口或降拍频，合并前先归档开销",
            JointSampCode::FakeFillRejected => "删除补齐逻辑：缺失走降级标注，不造假",
            JointSampCode::FingerprintMismatch => "排查隐式时钟/随机性泄漏进状态机",
            JointSampCode::LedgerCorrupted => "停用账面并复盘写入路径",
        }
    }
}

/// F1451 可失败路径错误（每变体带中文可操作诊断）。
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum JointSampError {
    /// 空拍：一帧读数一个通道都没有。
    EmptyBeat,
    /// 造假补齐哨兵值拒收（红线立案）。
    FakeFillSentinel {
        /// 携带哨兵值的通道（Channel as u8）。
        channel: u8,
    },
    /// 时序偏差超出 u32 可表达域（不静默截断）。
    SkewOverflow {
        /// 实际时戳跨度（µs）。
        span_us: u64,
    },
    /// 同拍时戳无进展（dt=0，斜率不可算）。
    ZeroInterval {
        /// 发生拍序号。
        beat: u64,
    },
    /// 拍序号断链（喂入序号 ≠ 站内期望）。
    SeqGap {
        /// 站内期望序号。
        expected: u64,
        /// 实际喂入序号。
        got: u64,
    },
    /// 开销账溢出（累计 µs 超 u64，理论不可达但显性防呆）。
    OverheadOverflow,
    /// 账面/计数自证失败。
    LedgerCorrupted,
    /// 双跑确定性破约。
    DeterminismBroken,
    /// 注入用例空集。
    InjectionCaseEmpty,
}

impl JointSampError {
    /// 对应诊断码。
    pub fn code(&self) -> JointSampCode {
        match self {
            JointSampError::EmptyBeat => JointSampCode::ChannelMissing,
            JointSampError::FakeFillSentinel { .. } => JointSampCode::FakeFillRejected,
            JointSampError::SkewOverflow { .. } => JointSampCode::BeatSkewed,
            JointSampError::ZeroInterval { .. } => JointSampCode::BeatSkewed,
            JointSampError::SeqGap { .. } => JointSampCode::LedgerCorrupted,
            JointSampError::OverheadOverflow => JointSampCode::OverheadBudgetExceeded,
            JointSampError::LedgerCorrupted => JointSampCode::LedgerCorrupted,
            JointSampError::DeterminismBroken => JointSampCode::FingerprintMismatch,
            JointSampError::InjectionCaseEmpty => JointSampCode::CrossRuleUnmatched,
        }
    }

    /// 中文可操作诊断（≥10 字，说清原因与下一步）。
    pub fn detail(&self) -> &'static str {
        match self {
            JointSampError::EmptyBeat => "空拍拒收：帧内无任何通道读数，请检查采样轮询是否整体失败",
            JointSampError::FakeFillSentinel { .. } => {
                "造假补齐拒收：读到哨兵值，删除补齐逻辑并改走缺失降级标注"
            }
            JointSampError::SkewOverflow { .. } => {
                "时序偏差溢出：通道时戳跨度超 32 位，请核对时钟源同步与计数位宽"
            }
            JointSampError::ZeroInterval { .. } => {
                "时戳无进展：同拍时戳未前进，斜率不可算，请核对拍时戳单调性"
            }
            JointSampError::SeqGap { .. } => {
                "拍序断链：喂入序号与站内期望不符，请按站内 next_seq 顺序喂拍"
            }
            JointSampError::OverheadOverflow => {
                "开销账溢出：累计 µs 超 64 位，请停用并归档当前账面"
            }
            JointSampError::LedgerCorrupted => {
                "账面损坏：seq 断链或计数不守恒，请停用账面并复盘写入路径"
            }
            JointSampError::DeterminismBroken => {
                "确定性破约：同输入双跑指纹不一致，排查随机性/时钟泄漏进状态机"
            }
            JointSampError::InjectionCaseEmpty => {
                "注入用例空集：至少提供一个注入用例再跑台面"
            }
        }
    }
}

// ---------------------------------------------------------------------------
// FNV-1a 指纹与事件账
// ---------------------------------------------------------------------------

/// FNV-1a 单步（const fn）。
pub const fn fnv1a_step(hash: u64, byte: u8) -> u64 {
    (hash ^ byte as u64).wrapping_mul(FNV_PRIME)
}

/// FNV-1a 连续混入字节串。
pub fn fnv1a_bytes(mut hash: u64, data: &[u8]) -> u64 {
    let mut i = 0;
    while i < data.len() {
        hash = fnv1a_step(hash, data[i]);
        i += 1;
    }
    hash
}

/// u64 按小端逐字节混入（指纹用，避免内存布局依赖）。
pub fn mix_u64(mut hash: u64, v: u64) -> u64 {
    let mut i = 0;
    while i < 8 {
        hash = fnv1a_step(hash, ((v >> (i * 8)) & 0xFF) as u8);
        i += 1;
    }
    hash
}

/// 事件账条目。
#[derive(Clone, Debug)]
pub struct LedgerEntry {
    /// 全局单调序号（含被让位淘汰的条目）。
    pub seq: u64,
    /// 诊断码。
    pub code: JointSampCode,
    /// 关联拍序号。
    pub beat: u64,
    /// 辅助量（skew µs / 规则 id / 源 id / 桶坐标等，语义随 code）。
    pub aux: u64,
    /// 备注（区分预警/超差等细分语义）。
    pub note: String,
}

/// 事件账（容量 T18 写死；满后半量让位；seq 连续性自证）。
pub struct EventLedger {
    entries: Vec<LedgerEntry>,
    next_seq: u64,
    dropped: u64,
}

impl EventLedger {
    /// 新账。
    pub fn new() -> Self {
        EventLedger { entries: Vec::new(), next_seq: 0, dropped: 0 }
    }

    /// 在账条目数。
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// 账是否为空。
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// 累计让位淘汰数。
    pub fn dropped(&self) -> u64 {
        self.dropped
    }

    /// 在账条目（只读）。
    pub fn entries(&self) -> &[LedgerEntry] {
        &self.entries
    }

    /// 入账（满后半量让位，seq 单调递增不回绕语义用 wrapping 计数）。
    pub fn push(&mut self, code: JointSampCode, beat: u64, aux: u64, note: &str) {
        if self.entries.len() >= LEDGER_CAP {
            let half = LEDGER_CAP / 2;
            self.entries.drain(0..half);
            self.dropped = self.dropped.saturating_add(half as u64);
        }
        let seq = self.next_seq;
        self.next_seq = self.next_seq.wrapping_add(1);
        self.entries.push(LedgerEntry { seq, code, beat, aux, note: String::from(note) });
    }

    /// 某码在账计数。
    pub fn count_of(&self, code: JointSampCode) -> u32 {
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

    /// 账面自证：在账条目 seq 必须连续，且首条 seq + 条数 = next_seq。
    pub fn health(&self) -> Result<(), JointSampError> {
        let mut i = 1;
        while i < self.entries.len() {
            if self.entries[i].seq != self.entries[i - 1].seq.wrapping_add(1) {
                return Err(JointSampError::LedgerCorrupted);
            }
            i += 1;
        }
        if let Some(first) = self.entries.first() {
            if first.seq.saturating_add(self.entries.len() as u64) != self.next_seq {
                return Err(JointSampError::LedgerCorrupted);
            }
        }
        Ok(())
    }

    /// 账面指纹（混入全部在账条目；让位淘汰数一并混入，保证双跑可对拍）。
    pub fn fingerprint_seed(&self, mut h: u64) -> u64 {
        h = mix_u64(h, self.next_seq);
        h = mix_u64(h, self.dropped);
        let mut i = 0;
        while i < self.entries.len() {
            let e = &self.entries[i];
            h = mix_u64(h, e.seq);
            h = mix_u64(h, e.code.as_u16() as u64);
            h = mix_u64(h, e.beat);
            h = mix_u64(h, e.aux);
            h = fnv1a_bytes(h, e.note.as_bytes());
            i += 1;
        }
        h
    }
}

// ---------------------------------------------------------------------------
// 联合画像：16×16 散点桶矩阵 + 功耗→温升响应曲线（F1473 输入接口）
// ---------------------------------------------------------------------------

/// 单功耗桶的温度分位数带（整数分位，从桶内温度直方图导出）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct TempBand {
    /// 桶内样本数。
    pub count: u32,
    /// 中位温度所在桶下标。
    pub t_bucket: usize,
    /// P25 温度（m°C，桶中心值）。
    pub p25_mdegc: u32,
    /// P50 温度（m°C，桶中心值）。
    pub p50_mdegc: u32,
    /// P75 温度（m°C，桶中心值）。
    pub p75_mdegc: u32,
}

/// 功耗→温升响应曲线（联合画像导出物）。
#[derive(Clone, Debug)]
pub struct ResponseCurve {
    /// 每功耗桶的分位数带（None = 该桶无样本）。
    pub bands: Vec<Option<TempBand>>,
    /// 非空功耗桶数。
    pub occupied: u32,
    /// 画像覆盖率（‰，非空桶 / 16）。
    pub coverage_permille: u32,
    /// 单调性违约计数（最近一次审计值）。
    pub violations: u32,
}

/// F1473 热模型输入接口声明（判据二落点）。
///
/// F1473（热模型）消费 [`ResponseCurve`] 的功耗→温升响应带作为输入；
/// 本结构只声明接口与质量标志，不做热模型计算（职责分离）。
#[derive(Clone, Debug)]
pub struct HotModelInput {
    /// 功耗→温升响应曲线。
    pub curve: ResponseCurve,
    /// 入画像拍数（同拍对齐且双通道在位的拍）。
    pub beats_used: u64,
    /// 是否仅同拍对齐样本入像（恒 true——超偏差拍不冒充同拍）。
    pub aligned_only: bool,
    /// 消费方声明。
    pub sink: &'static str,
    /// 质量门：覆盖率 ≥ 3/16 且样本 ≥ 64 拍才建议入模型。
    pub quality_ok: bool,
}

/// 联合画像器（判据二）。
///
/// 16×16 散点桶矩阵：行 = 功耗桶（T08），列 = 温度桶（T09）；矩阵同时
/// 就是每功耗桶的温度直方图——响应曲线分位数带直接从行直方图导出，
/// 无需第二份存储。仅"同拍对齐 + 功耗/温度双在位"的拍入像。
pub struct JointProfiler {
    matrix: [[u32; BUCKET_T]; BUCKET_P],
    beats_total: u64,
    beats_used: u64,
    beats_skipped_missing: u64,
    beats_skewed_excluded: u64,
    mono_violations: u32,
    reported_violations: u32,
}

/// 单拍画像落点报告。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ProfileDelta {
    /// 是否入像。
    pub recorded: bool,
    /// 因超偏差被排除（不冒充同拍）。
    pub excluded_skewed: bool,
    /// 因通道缺失被跳过（不造假补齐）。
    pub skipped_missing: bool,
    /// 功耗桶下标（入像时有效）。
    pub power_bucket: usize,
    /// 温度桶下标（入像时有效）。
    pub temp_bucket: usize,
}

impl JointProfiler {
    /// 新画像器（1KB 矩阵置零）。
    pub fn new() -> Self {
        JointProfiler {
            matrix: [[0u32; BUCKET_T]; BUCKET_P],
            beats_total: 0,
            beats_used: 0,
            beats_skipped_missing: 0,
            beats_skewed_excluded: 0,
            mono_violations: 0,
            reported_violations: 0,
        }
    }

    /// 入像拍数。
    pub fn beats_used(&self) -> u64 {
        self.beats_used
    }

    /// 总收拍数。
    pub fn beats_total(&self) -> u64 {
        self.beats_total
    }

    /// 因缺失被跳过的拍数。
    pub fn beats_skipped_missing(&self) -> u64 {
        self.beats_skipped_missing
    }

    /// 因超偏差被排除的拍数。
    pub fn beats_skewed_excluded(&self) -> u64 {
        self.beats_skewed_excluded
    }

    /// 单调性违约计数（最近一次审计值）。
    pub fn mono_violations(&self) -> u32 {
        self.mono_violations
    }

    /// 单拍入像（判据自证：超偏差拍与缺通道拍不入像，分类计数留痕）。
    pub fn record(&mut self, s: &JointSample, ledger: &mut EventLedger) -> ProfileDelta {
        self.beats_total = self.beats_total.saturating_add(1);
        // 超偏差拍不冒充同拍：显性排除并入账分类计数。
        if !s.aligned {
            self.beats_skewed_excluded = self.beats_skewed_excluded.saturating_add(1);
            return ProfileDelta {
                recorded: false,
                excluded_skewed: true,
                skipped_missing: false,
                power_bucket: 0,
                temp_bucket: 0,
            };
        }
        // 缺失通道不入像：不造假补齐（缺功耗或缺温度都无法落散点）。
        if !s.has(Channel::Power) || !s.has(Channel::Temp) {
            self.beats_skipped_missing = self.beats_skipped_missing.saturating_add(1);
            return ProfileDelta {
                recorded: false,
                excluded_skewed: false,
                skipped_missing: true,
                power_bucket: 0,
                temp_bucket: 0,
            };
        }
        let p = bucket_of(s.power_mw, P_RANGE_MW, BUCKET_P);
        let t = bucket_of(s.temp_mdegc, T_RANGE_MDEGC, BUCKET_T);
        let before = self.matrix[p][t];
        self.matrix[p][t] = before.saturating_add(1);
        self.beats_used = self.beats_used.saturating_add(1);
        if before == 0 {
            // 桶首次落点才入账（逐拍刷账会淹没 512 容量）。
            let aux = (p as u64) << 8 | t as u64;
            ledger.push(JointSampCode::ProfileBucketUpdated, s.beat_seq, aux, "散点桶首次落点");
        }
        ProfileDelta {
            recorded: true,
            excluded_skewed: false,
            skipped_missing: false,
            power_bucket: p,
            temp_bucket: t,
        }
    }

    /// 导出功耗→温升响应曲线（纯读：分位数带从行直方图导出）。
    pub fn response_curve(&self) -> ResponseCurve {
        let mut bands = Vec::new();
        let mut occupied = 0u32;
        let mut p = 0usize;
        while p < BUCKET_P {
            let row = &self.matrix[p];
            let mut count = 0u64;
            let mut t = 0usize;
            while t < BUCKET_T {
                count = count.saturating_add(row[t] as u64);
                t += 1;
            }
            if count == 0 {
                bands.push(None);
            } else {
                occupied = occupied.saturating_add(1);
                let count32 = count as u32;
                let b25 = hist_quantile_bucket(row, count32, 250);
                let b50 = hist_quantile_bucket(row, count32, 500);
                let b75 = hist_quantile_bucket(row, count32, 750);
                bands.push(Some(TempBand {
                    count: count32,
                    t_bucket: b50,
                    p25_mdegc: temp_bucket_center(b25),
                    p50_mdegc: temp_bucket_center(b50),
                    p75_mdegc: temp_bucket_center(b75),
                }));
            }
            p += 1;
        }
        ResponseCurve {
            bands,
            occupied,
            coverage_permille: occupied.saturating_mul(1000) / BUCKET_P as u32,
            violations: self.mono_violations,
        }
    }

    /// 单调性审计（判据二自证）：功耗升、温升也应升——相邻非空功耗桶的
    /// 中位温度桶倒挂超 T20 容忍即违约。审计增量入账（违约数上涨才立案）。
    pub fn audit_monotonic(&mut self, ledger: &mut EventLedger, beat: u64) -> u32 {
        // 行直方图中位桶序列（只取非空行）。
        let mut medians: Vec<(usize, i64)> = Vec::new();
        let mut p = 0usize;
        while p < BUCKET_P {
            let row = &self.matrix[p];
            let mut count = 0u64;
            let mut t = 0usize;
            while t < BUCKET_T {
                count = count.saturating_add(row[t] as u64);
                t += 1;
            }
            if count > 0 {
                let b50 = hist_quantile_bucket(row, count as u32, 500);
                medians.push((p, b50 as i64));
            }
            p += 1;
        }
        let mut violations = 0u32;
        let mut last_violation = None;
        let mut i = 1;
        while i < medians.len() {
            let drop = medians[i - 1].1 - medians[i].1; // 正值 = 倒挂
            if drop > MONO_TOLERANCE_BUCKETS {
                violations = violations.saturating_add(1);
                last_violation = Some((medians[i - 1].0, medians[i].0));
            }
            i += 1;
        }
        self.mono_violations = violations;
        if violations > self.reported_violations {
            let aux = match last_violation {
                Some((a, b)) => (a as u64) << 8 | b as u64,
                None => 0,
            };
            ledger.push(
                JointSampCode::CurveMonotonicViolated,
                beat,
                aux,
                "中位温度桶倒挂超容忍——核查散热与标定",
            );
        }
        self.reported_violations = violations;
        violations
    }

    /// F1473 热模型输入接口（判据二落点声明）。
    pub fn hot_model_input(&self) -> HotModelInput {
        let curve = self.response_curve();
        let quality_ok = curve.coverage_permille >= 1000 / BUCKET_P as u32 * 3
            && self.beats_used >= 64;
        HotModelInput {
            curve,
            beats_used: self.beats_used,
            aligned_only: true,
            sink: "F1473 热模型（功耗→温升响应曲线输入）",
            quality_ok,
        }
    }

    /// 状态指纹混入（双跑对拍用）。
    pub fn fingerprint_state(&self, mut h: u64) -> u64 {
        h = mix_u64(h, self.beats_total);
        h = mix_u64(h, self.beats_used);
        h = mix_u64(h, self.beats_skipped_missing);
        h = mix_u64(h, self.beats_skewed_excluded);
        h = mix_u64(h, self.mono_violations as u64);
        let mut p = 0usize;
        while p < BUCKET_P {
            let mut t = 0usize;
            while t < BUCKET_T {
                h = mix_u64(h, self.matrix[p][t] as u64);
                t += 1;
            }
            p += 1;
        }
        h
    }
}

/// 行直方图整数分位桶下标：permille ∈ {250, 500, 750}。
///
/// 累积计数超过 target（count×permille/1000）的首个桶即分位桶；
/// target 为 0 时落在首个非零桶（直方图无插值，桶级精度）。
fn hist_quantile_bucket(row: &[u32; BUCKET_T], count: u32, permille: u32) -> usize {
    let target = (count as u64).saturating_mul(permille as u64) / 1000;
    let mut cum: u64 = 0;
    let mut i = 0usize;
    while i < BUCKET_T {
        cum = cum.saturating_add(row[i] as u64);
        if cum > target {
            return i;
        }
        i += 1;
    }
    BUCKET_T - 1
}

// ---------------------------------------------------------------------------
// 交叉规则表（判据三）
// ---------------------------------------------------------------------------

/// 功耗斜率类别（闭集三分类）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PowerClass {
    /// 平/降：斜率 ≤ T12。
    Flat,
    /// 缓升：T12 < 斜率 < T13。
    SlowRise,
    /// 骤升：斜率 ≥ T13。
    SuddenRise,
}

impl PowerClass {
    /// 类别名。
    pub fn label(self) -> &'static str {
        match self {
            PowerClass::Flat => "功耗平",
            PowerClass::SlowRise => "功耗缓升",
            PowerClass::SuddenRise => "功耗骤升",
        }
    }
}

/// 温度斜率类别（闭集三分类）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TempClass {
    /// 平/降：斜率 ≤ T14。
    Flat,
    /// 缓升：T14 < 斜率 < T15。
    SlowRise,
    /// 骤升：斜率 ≥ T15。
    SuddenRise,
}

impl TempClass {
    /// 类别名。
    pub fn label(self) -> &'static str {
        match self {
            TempClass::Flat => "温度平",
            TempClass::SlowRise => "温度缓升",
            TempClass::SuddenRise => "温度骤升",
        }
    }
}

/// 交叉判定结论（闭集五分类：四规则 + 显性 Unknown）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RuleVerdict {
    /// 正常（功耗骤升 + 温度缓升：热滞后，R1）。
    Normal,
    /// 散热异常（功耗平 + 温度骤升，R2）。
    CoolingAbnormal,
    /// 瞬时过载（双骤升，R3）。
    TransientOverload,
    /// 稳态（双平，R4）。
    SteadyState,
    /// 未判定（类别组合不在表内 / 拍无效——显性不猜）。
    Unknown,
}

impl RuleVerdict {
    /// 结论名。
    pub fn label(self) -> &'static str {
        match self {
            RuleVerdict::Normal => "正常（热滞后）",
            RuleVerdict::CoolingAbnormal => "散热异常",
            RuleVerdict::TransientOverload => "瞬时过载",
            RuleVerdict::SteadyState => "稳态",
            RuleVerdict::Unknown => "未判定",
        }
    }
}

/// 交叉规则条目（const 表，锚点四规则逐一登记）。
pub struct CrossRule {
    /// 规则 id（R1~R4，判定留痕用）。
    pub id: u8,
    /// 规则名（锚点原文措辞）。
    pub name: &'static str,
    /// 功耗类别侧。
    pub power: PowerClass,
    /// 温度类别侧。
    pub temp: TempClass,
    /// 结论。
    pub verdict: RuleVerdict,
    /// 动作建议（建议不强制：决策权在调度——与 F1461 契约一致）。
    pub advice: &'static str,
}

/// 交叉规则表（const，判据三）。
///
/// 来源：锚点原文"功耗骤升+温度缓升→正常；功耗平+温度骤升→散热异常"
/// + 补全对称规则（双骤升→瞬时过载、双平→稳态，施工细则定档）。
/// 未登记组合（如功耗骤升+温度平）显性 Unknown 立案，不猜结论。
pub const CROSS_RULES: [CrossRule; 4] = [
    CrossRule {
        id: 1,
        name: "功耗骤升+温度缓升→正常",
        power: PowerClass::SuddenRise,
        temp: TempClass::SlowRise,
        verdict: RuleVerdict::Normal,
        advice: "热滞后正常物理响应，无需动作",
    },
    CrossRule {
        id: 2,
        name: "功耗平+温度骤升→散热异常",
        power: PowerClass::Flat,
        temp: TempClass::SuddenRise,
        verdict: RuleVerdict::CoolingAbnormal,
        advice: "建议核查风扇/风道（建议 F1461，不强制）",
    },
    CrossRule {
        id: 3,
        name: "双骤升→瞬时过载",
        power: PowerClass::SuddenRise,
        temp: TempClass::SuddenRise,
        verdict: RuleVerdict::TransientOverload,
        advice: "建议观察拍后回落，持续则交 F1450 超限链路",
    },
    CrossRule {
        id: 4,
        name: "双平→稳态",
        power: PowerClass::Flat,
        temp: TempClass::Flat,
        verdict: RuleVerdict::SteadyState,
        advice: "无需动作，画像继续累积",
    },
];

/// 功耗斜率分类（dt>0 由调用方保证；降斜率归 Flat——锚点只定义升侧语义）。
pub fn classify_power(slope_mw_per_ms: i64) -> PowerClass {
    if slope_mw_per_ms >= PWR_STEEP_SLOPE_MW_PER_MS {
        PowerClass::SuddenRise
    } else if slope_mw_per_ms <= PWR_FLAT_SLOPE_MW_PER_MS {
        PowerClass::Flat
    } else {
        PowerClass::SlowRise
    }
}

/// 温度斜率分类（dt>0 由调用方保证；降斜率归 Flat）。
pub fn classify_temp(slope_mdegc_per_ms: i64) -> TempClass {
    if slope_mdegc_per_ms >= TEMP_STEEP_SLOPE_MDEGC_PER_MS {
        TempClass::SuddenRise
    } else if slope_mdegc_per_ms <= TEMP_FLAT_SLOPE_MDEGC_PER_MS {
        TempClass::Flat
    } else {
        TempClass::SlowRise
    }
}

/// 交叉判定留痕（判定可追溯：规则 id / 双斜率 / 双类别 / 降级与对齐标志）。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct VerdictRecord {
    /// 判定拍序号。
    pub beat: u64,
    /// 结论。
    pub verdict: RuleVerdict,
    /// 命中规则 id（Unknown 时 None）。
    pub matched_rule: Option<u8>,
    /// 功耗类别。
    pub power_class: PowerClass,
    /// 温度类别。
    pub temp_class: TempClass,
    /// 功耗斜率（mW/ms，整数）。
    pub power_slope_mw_per_ms: i64,
    /// 温度斜率（m°C/ms，整数）。
    pub temp_slope_mdegc_per_ms: i64,
    /// 当前拍是否降级（降级拍不判定）。
    pub degraded: bool,
    /// 当前拍是否同拍对齐（超偏差拍不判定）。
    pub aligned: bool,
    /// 判定痕迹（规则名 / 不判定原因）。
    pub trace: String,
}

/// 交叉规则引擎（判据三执行侧）。
pub struct CrossRuleEngine {
    matched: u32,
    unmatched: u32,
    skipped_invalid: u32,
    per_rule: [u32; 4],
    last: Option<VerdictRecord>,
}

impl CrossRuleEngine {
    /// 新引擎。
    pub fn new() -> Self {
        CrossRuleEngine { matched: 0, unmatched: 0, skipped_invalid: 0, per_rule: [0; 4], last: None }
    }

    /// 累计命中数。
    pub fn matched(&self) -> u32 {
        self.matched
    }

    /// 累计未匹配数。
    pub fn unmatched(&self) -> u32 {
        self.unmatched
    }

    /// 因拍无效跳过判定数（缺失/超偏差——不判定的拍显性计数）。
    pub fn skipped_invalid(&self) -> u32 {
        self.skipped_invalid
    }

    /// 逐规则命中计数。
    pub fn per_rule(&self) -> [u32; 4] {
        self.per_rule
    }

    /// 最近一次判定留痕。
    pub fn last(&self) -> Option<&VerdictRecord> {
        self.last.as_ref()
    }

    /// 对当前拍做交叉判定。
    ///
    /// - 无前拍：返回 None（首拍无从谈斜率，不算异常）；
    /// - 前拍或当前拍无效（超偏差/降级/缺双通道）：Unknown + 留痕，计入
    ///   skipped——缺失降级沿规则链传播的落点；
    /// - 正常路径：双斜率 → 双类别 → const 表匹配 → 命中/未匹配入账。
    pub fn evaluate(
        &mut self,
        prev: Option<&JointSample>,
        cur: &JointSample,
        ledger: &mut EventLedger,
    ) -> Option<VerdictRecord> {
        let pv = match prev {
            None => return None,
            Some(p) => p,
        };
        let prev_ok = pv.aligned && !pv.degraded && pv.has(Channel::Power) && pv.has(Channel::Temp);
        let cur_ok = cur.aligned && !cur.degraded && cur.has(Channel::Power) && cur.has(Channel::Temp);
        if !prev_ok || !cur_ok {
            self.skipped_invalid = self.skipped_invalid.saturating_add(1);
            let mut rec = VerdictRecord {
                beat: cur.beat_seq,
                verdict: RuleVerdict::Unknown,
                matched_rule: None,
                power_class: PowerClass::Flat,
                temp_class: TempClass::Flat,
                power_slope_mw_per_ms: 0,
                temp_slope_mdegc_per_ms: 0,
                degraded: cur.degraded,
                aligned: cur.aligned,
                trace: String::new(),
            };
            if !cur_ok {
                rec.trace = format!(
                    "当前拍无效（{}）——不判定",
                    if cur.aligned { "降级/缺通道" } else { "超偏差" }
                );
            } else {
                rec.trace = String::from("前拍无效（超偏差/降级/缺通道）——不判定");
            }
            self.last = Some(rec.clone());
            return Some(rec);
        }
        let dt_us = cur.beat_tsc_us.saturating_sub(pv.beat_tsc_us);
        if dt_us == 0 {
            self.skipped_invalid = self.skipped_invalid.saturating_add(1);
            let rec = VerdictRecord {
                beat: cur.beat_seq,
                verdict: RuleVerdict::Unknown,
                matched_rule: None,
                power_class: PowerClass::Flat,
                temp_class: TempClass::Flat,
                power_slope_mw_per_ms: 0,
                temp_slope_mdegc_per_ms: 0,
                degraded: cur.degraded,
                aligned: cur.aligned,
                trace: String::from("时间戳无进展（dt=0）——斜率不可算，不判定"),
            };
            self.last = Some(rec.clone());
            return Some(rec);
        }
        // 整数斜率：Δ×1000/dt(µs) = 每 ms 变化量；i64 域内 (Δ≤1.5e5)×1000 无溢出。
        let dt = dt_us as i64;
        let p_slope = (cur.power_mw as i64 - pv.power_mw as i64) * 1000 / dt;
        let t_slope = (cur.temp_mdegc as i64 - pv.temp_mdegc as i64) * 1000 / dt;
        let pc = classify_power(p_slope);
        let tc = classify_temp(t_slope);
        let mut hit: Option<usize> = None;
        let mut i = 0usize;
        while i < CROSS_RULES.len() {
            if CROSS_RULES[i].power == pc && CROSS_RULES[i].temp == tc {
                hit = Some(i);
                break;
            }
            i += 1;
        }
        let rec = match hit {
            Some(idx) => {
                let r = &CROSS_RULES[idx];
                self.matched = self.matched.saturating_add(1);
                self.per_rule[idx] = self.per_rule[idx].saturating_add(1);
                ledger.push(
                    JointSampCode::CrossRuleMatched,
                    cur.beat_seq,
                    r.id as u64,
                    r.name,
                );
                VerdictRecord {
                    beat: cur.beat_seq,
                    verdict: r.verdict,
                    matched_rule: Some(r.id),
                    power_class: pc,
                    temp_class: tc,
                    power_slope_mw_per_ms: p_slope,
                    temp_slope_mdegc_per_ms: t_slope,
                    degraded: cur.degraded,
                    aligned: cur.aligned,
                    trace: format!("规则 R{}：{}｜建议：{}", r.id, r.name, r.advice),
                }
            }
            None => {
                self.unmatched = self.unmatched.saturating_add(1);
                ledger.push(
                    JointSampCode::CrossRuleUnmatched,
                    cur.beat_seq,
                    0,
                    "类别组合不在表内",
                );
                VerdictRecord {
                    beat: cur.beat_seq,
                    verdict: RuleVerdict::Unknown,
                    matched_rule: None,
                    power_class: pc,
                    temp_class: tc,
                    power_slope_mw_per_ms: p_slope,
                    temp_slope_mdegc_per_ms: t_slope,
                    degraded: cur.degraded,
                    aligned: cur.aligned,
                    trace: format!(
                        "未匹配组合：{}+{}——不猜结论，先归档样本",
                        pc.label(),
                        tc.label()
                    ),
                }
            }
        };
        self.last = Some(rec.clone());
        Some(rec)
    }

    /// 状态指纹混入。
    pub fn fingerprint_state(&self, mut h: u64) -> u64 {
        h = mix_u64(h, self.matched as u64);
        h = mix_u64(h, self.unmatched as u64);
        h = mix_u64(h, self.skipped_invalid as u64);
        let mut i = 0;
        while i < 4 {
            h = mix_u64(h, self.per_rule[i] as u64);
            i += 1;
        }
        h
    }
}

// ---------------------------------------------------------------------------
// 传感器优先级表（判据四）
// ---------------------------------------------------------------------------

/// 温度传感器源（const 优先级表条目）。
pub struct SensorSource {
    /// 源 id（与 SRC_* 常量一致）。
    pub id: u8,
    /// 源名。
    pub name: &'static str,
    /// 优先级秩（3 最高）。
    pub rank: u8,
    /// 来源注记。
    pub note: &'static str,
}

/// 温度源优先级表（const，判据四）。
///
/// 来源：锚点原文"温度传感多源——取 GPU 结温优先、VRM 次之"；
/// 环境温度为兜底源（rank1，仅当更高秩源全缺时启用）。
pub const SENSOR_PRIORITY: [SensorSource; 3] = [
    SensorSource {
        id: SRC_GPU_DIE,
        name: "GPU 结温",
        rank: 3,
        note: "最高优先级（锚点：取 GPU 结温优先）",
    },
    SensorSource {
        id: SRC_VRM,
        name: "VRM 温度",
        rank: 2,
        note: "次优先级（锚点：VRM 次之）",
    },
    SensorSource {
        id: SRC_AMBIENT,
        name: "环境温度",
        rank: 1,
        note: "兜底源（仅更高秩源全缺时启用）",
    },
];

/// 按 id 查源表。
pub fn sensor_source_by_id(id: u8) -> Option<&'static SensorSource> {
    let mut i = 0usize;
    while i < SENSOR_PRIORITY.len() {
        if SENSOR_PRIORITY[i].id == id {
            return Some(&SENSOR_PRIORITY[i]);
        }
        i += 1;
    }
    None
}

/// 同拍多源温度消解：取优先级最高（rank 最大）且已登记的源。
///
/// 候选元组为 (source_id, value_mdegc, tsc_us)；未登记 id（≥3）忽略——
/// 未登记源可能是新硬件尚未进表，不猜其秩，宁缺毋滥。
/// 返回 (选中源, 值, 时戳)。
pub fn resolve_temp_source(
    candidates: &[(u8, u32, u64)],
) -> Option<(&'static SensorSource, u32, u64)> {
    let mut best: Option<(&'static SensorSource, u32, u64)> = None;
    let mut i = 0usize;
    while i < candidates.len() {
        let (id, value, tsc) = candidates[i];
        if let Some(src) = sensor_source_by_id(id) {
            let take = match best {
                None => true,
                Some((b, _, _)) => src.rank > b.rank,
            };
            if take {
                best = Some((src, value, tsc));
            }
        }
        i += 1;
    }
    best
}

/// 温度源切换账（判据四的"源切换账"）。
pub struct SensorSwitchLedger {
    last: Option<u8>,
    switches: u32,
    fallbacks: u32,
    selections: u64,
    per_source: [u32; 3],
}

impl SensorSwitchLedger {
    /// 新账。
    pub fn new() -> Self {
        SensorSwitchLedger { last: None, switches: 0, fallbacks: 0, selections: 0, per_source: [0; 3] }
    }

    /// 上拍选中源。
    pub fn last(&self) -> Option<u8> {
        self.last
    }

    /// 累计切换数。
    pub fn switches(&self) -> u32 {
        self.switches
    }

    /// 累计降源回退数（最高秩源缺失、启用次级源）。
    pub fn fallbacks(&self) -> u32 {
        self.fallbacks
    }

    /// 累计选用拍数。
    pub fn selections(&self) -> u64 {
        self.selections
    }

    /// 逐源选用计数。
    pub fn per_source(&self) -> [u32; 3] {
        self.per_source
    }

    /// 记一笔选用：返回是否发生源切换。
    ///
    /// `gpu_present` = 本拍温度候选中是否含 GPU 结温源——不含即降源回退。
    pub fn observe(
        &mut self,
        chosen: &'static SensorSource,
        gpu_present: bool,
        beat: u64,
        ledger: &mut EventLedger,
    ) -> bool {
        self.selections = self.selections.saturating_add(1);
        let idx = chosen.id as usize;
        if idx < 3 {
            self.per_source[idx] = self.per_source[idx].saturating_add(1);
        }
        let switched = match self.last {
            // 首次选用不算切换（无从谈起"从哪切来"）。
            None => false,
            Some(prev) => prev != chosen.id,
        };
        if switched {
            self.switches = self.switches.saturating_add(1);
            ledger.push(
                JointSampCode::SensorSourceSwitched,
                beat,
                chosen.id as u64,
                chosen.name,
            );
        }
        if !gpu_present {
            self.fallbacks = self.fallbacks.saturating_add(1);
            ledger.push(
                JointSampCode::SensorFallbackUsed,
                beat,
                chosen.id as u64,
                "最高秩源缺失，降源回退",
            );
        }
        self.last = Some(chosen.id);
        switched
    }

    /// 状态指纹混入。
    pub fn fingerprint_state(&self, mut h: u64) -> u64 {
        h = mix_u64(h, self.last.map(|v| v as u64).unwrap_or(0xFF));
        h = mix_u64(h, self.switches as u64);
        h = mix_u64(h, self.fallbacks as u64);
        h = mix_u64(h, self.selections);
        let mut i = 0;
        while i < 3 {
            h = mix_u64(h, self.per_source[i] as u64);
            i += 1;
        }
        h
    }
}

// ---------------------------------------------------------------------------
// 开销合并账（判据五）
// ---------------------------------------------------------------------------

/// 单拍开销判定报告。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct OverheadReport {
    /// 本拍是否超预算（> T03）。
    pub budget_hit: bool,
    /// 相对分立三次采样的节省率（‰，整数）。
    pub savings_permille: u32,
    /// 截至本拍的均值开销（µs，整数除法）。
    pub avg_us: u32,
}

/// 开销合并账（判据五）：联合单次开销逐拍对 T03（300µs）判据 + 分立
/// 三次采样开销对比归档（3×单通道 vs 1×联合，节省倍率账 permille）。
pub struct OverheadLedger {
    beats: u64,
    total_us: u64,
    max_us: u32,
    budget_ok: u32,
    budget_exceeded: u32,
    last_exceeded: bool,
}

impl OverheadLedger {
    /// 新账。
    pub fn new() -> Self {
        OverheadLedger {
            beats: 0,
            total_us: 0,
            max_us: 0,
            budget_ok: 0,
            budget_exceeded: 0,
            last_exceeded: false,
        }
    }

    /// 记账拍数。
    pub fn beats(&self) -> u64 {
        self.beats
    }

    /// 累计开销（µs）。
    pub fn total_us(&self) -> u64 {
        self.total_us
    }

    /// 峰值开销（µs）。
    pub fn max_us(&self) -> u32 {
        self.max_us
    }

    /// 预算内拍数。
    pub fn budget_ok(&self) -> u32 {
        self.budget_ok
    }

    /// 超预算拍数。
    pub fn budget_exceeded(&self) -> u32 {
        self.budget_exceeded
    }

    /// 单拍记账：预算判定 + 事件入账（超差逐拍立案；预算内仅首拍与
    /// 恢复时入账，防 512 容量被正常拍刷爆）。
    pub fn record(
        &mut self,
        measured_us: u32,
        beat: u64,
        ledger: &mut EventLedger,
    ) -> Result<OverheadReport, JointSampError> {
        let total = match self.total_us.checked_add(measured_us as u64) {
            Some(t) => t,
            None => return Err(JointSampError::OverheadOverflow),
        };
        self.total_us = total;
        self.beats = self.beats.saturating_add(1);
        if measured_us > self.max_us {
            self.max_us = measured_us;
        }
        let hit = measured_us > JOINT_BUDGET_US;
        if hit {
            self.budget_exceeded = self.budget_exceeded.saturating_add(1);
            ledger.push(
                JointSampCode::OverheadBudgetExceeded,
                beat,
                measured_us as u64,
                "联合开销超 300µs 判据",
            );
        } else {
            self.budget_ok = self.budget_ok.saturating_add(1);
            if self.budget_ok == 1 {
                ledger.push(JointSampCode::OverheadBudgetOk, beat, measured_us as u64, "联合开销入预算（首拍）");
            } else if self.last_exceeded {
                ledger.push(JointSampCode::OverheadBudgetOk, beat, measured_us as u64, "预算恢复");
            }
        }
        self.last_exceeded = hit;
        // 节省率 = (分立总开销 − 联合实测) × 1000 / 分立总开销（‰）。
        let discrete = discrete_total_us();
        let savings = discrete.saturating_sub(measured_us) as u64 * 1000 / discrete as u64;
        let avg = if self.beats == 0 { 0 } else { (self.total_us / self.beats) as u32 };
        Ok(OverheadReport { budget_hit: hit, savings_permille: savings as u32, avg_us: avg })
    }

    /// 均值开销（µs）。
    pub fn avg_us(&self) -> u32 {
        if self.beats == 0 {
            0
        } else {
            (self.total_us / self.beats) as u32
        }
    }

    /// 分立 vs 联合对比归档（判据五落点：节省倍率账导出）。
    pub fn overhead_doc(&self) -> String {
        let discrete = discrete_total_us();
        let joint = joint_nominal_us();
        let nominal_savings = (discrete - joint) as u64 * 1000 / discrete as u64;
        format!(
            "开销合并账：分立三次采样 = 3×{}µs(单通道) + {}µs(分发) = {}µs；\
             联合单次 = {}µs(分发) + {}µs(批量读) = {}µs ≤ 预算 {}µs；\
             名义节省 {}‰；实测均值 {}µs / 峰值 {}µs；\
             预算内 {} 拍 / 超预算 {} 拍",
            DISCRETE_PER_CHANNEL_US,
            DISCRETE_DISPATCH_US,
            discrete,
            JOINT_DISPATCH_US,
            JOINT_BATCH_READ_US,
            joint,
            JOINT_BUDGET_US,
            nominal_savings,
            self.avg_us(),
            self.max_us,
            self.budget_ok,
            self.budget_exceeded,
        )
    }

    /// 状态指纹混入。
    pub fn fingerprint_state(&self, mut h: u64) -> u64 {
        h = mix_u64(h, self.beats);
        h = mix_u64(h, self.total_us);
        h = mix_u64(h, self.max_us as u64);
        h = mix_u64(h, self.budget_ok as u64);
        h = mix_u64(h, self.budget_exceeded as u64);
        h
    }
}

// ---------------------------------------------------------------------------
// 缺失降级（判据六）
// ---------------------------------------------------------------------------

/// 降级状态机状态。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DegradeState {
    /// 双（全）通道模式。
    DualChannel,
    /// 单通道模式：missing 为缺失通道掩码。
    SingleChannel {
        /// 缺失通道掩码。
        missing: u8,
    },
}

/// 状态迁移记录。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct DegradeTransition {
    /// 迁移前状态。
    pub before: DegradeState,
    /// 迁移后状态。
    pub after: DegradeState,
}

/// 缺失降级状态机（判据六）。
///
/// - 宽限：连续 T16 拍内缺失只计数（瞬时总线重试不降级）；
/// - 降级：超宽限进入单通道模式，降级标注沿样本/画像/规则/报告全链传播；
/// - 红线：缺失值一律不补齐——哨兵值注入在采样帧层直接拒收；
/// - 恢复：连续 T17 拍全通道在位自动回双通道（防抖确认）。
pub struct MissingDegradation {
    state: DegradeState,
    miss_streak: u32,
    good_streak: u32,
    missing_counts: [u32; 3],
    degraded_beats: u64,
    fake_fill_rejected: u32,
}

impl MissingDegradation {
    /// 新状态机。
    pub fn new() -> Self {
        MissingDegradation {
            state: DegradeState::DualChannel,
            miss_streak: 0,
            good_streak: 0,
            missing_counts: [0; 3],
            degraded_beats: 0,
            fake_fill_rejected: 0,
        }
    }

    /// 当前状态。
    pub fn state(&self) -> DegradeState {
        self.state
    }

    /// 当前连缺拍数。
    pub fn miss_streak(&self) -> u32 {
        self.miss_streak
    }

    /// 逐通道累计缺失拍数（下标同 Channel::index）。
    pub fn missing_counts(&self) -> [u32; 3] {
        self.missing_counts
    }

    /// 降级期拍数（任一通道缺失的拍）。
    pub fn degraded_beats(&self) -> u64 {
        self.degraded_beats
    }

    /// 造假补齐拒收计数（红线立案数）。
    pub fn fake_fill_rejected(&self) -> u32 {
        self.fake_fill_rejected
    }

    /// 红线立案登记（由站点在拒收哨兵值时调用）。
    pub fn note_fake_fill_rejected(&mut self) {
        self.fake_fill_rejected = self.fake_fill_rejected.saturating_add(1);
    }

    /// 单拍观察：按在位掩码推进状态机，迁移事件入账。
    pub fn observe(
        &mut self,
        present: u8,
        beat: u64,
        ledger: &mut EventLedger,
    ) -> DegradeTransition {
        let before = self.state;
        if present == FULL_MASK {
            // 全通道：连缺清零，恢复确认计数前进。
            self.miss_streak = 0;
            self.good_streak = self.good_streak.saturating_add(1);
            if self.state != DegradeState::DualChannel
                && self.good_streak >= RECOVERY_CONFIRM_BEATS
            {
                self.state = DegradeState::DualChannel;
                ledger.push(
                    JointSampCode::RecoveredDualChannel,
                    beat,
                    self.good_streak as u64,
                    "连续恢复拍达标，自动回双通道",
                );
            }
        } else {
            // 有缺失：恢复计数清零，逐通道缺失计数前进。
            self.good_streak = 0;
            self.miss_streak = self.miss_streak.saturating_add(1);
            self.degraded_beats = self.degraded_beats.saturating_add(1);
            let missing = mask_missing(present);
            let mut ch_idx = 0usize;
            while ch_idx < 3 {
                if missing & (1u8 << ch_idx) != 0 {
                    self.missing_counts[ch_idx] = self.missing_counts[ch_idx].saturating_add(1);
                }
                ch_idx += 1;
            }
            ledger.push(
                JointSampCode::ChannelMissing,
                beat,
                missing as u64,
                &format!("缺失[{}]——计数不补齐", mask_label(missing)),
            );
            match self.state {
                DegradeState::DualChannel => {
                    if self.miss_streak >= MISSING_GRACE_BEATS {
                        self.state = DegradeState::SingleChannel { missing };
                        ledger.push(
                            JointSampCode::DegradedSingleChannel,
                            beat,
                            missing as u64,
                            "宽限超限，进入单通道模式——缺失值不造假补齐",
                        );
                    }
                }
                DegradeState::SingleChannel { missing: ref mut prev_missing } => {
                    // 已在降级中：缺失掩码随实际在位情况更新（不重复立案）。
                    *prev_missing = missing;
                }
            }
        }
        DegradeTransition { before, after: self.state }
    }

    /// 状态指纹混入。
    pub fn fingerprint_state(&self, mut h: u64) -> u64 {
        let state_v = match self.state {
            DegradeState::DualChannel => 0u64,
            DegradeState::SingleChannel { missing } => 1u64 | (missing as u64) << 8,
        };
        h = mix_u64(h, state_v);
        h = mix_u64(h, self.miss_streak as u64);
        h = mix_u64(h, self.good_streak as u64);
        h = mix_u64(h, self.degraded_beats);
        h = mix_u64(h, self.fake_fill_rejected as u64);
        let mut i = 0;
        while i < 3 {
            h = mix_u64(h, self.missing_counts[i] as u64);
            i += 1;
        }
        h
    }
}

// ---------------------------------------------------------------------------
// 联合采样站（facade）：喂拍 → 全链判定 → 事件账/指纹/对拍
// ---------------------------------------------------------------------------

/// 单拍喂入报告（全链结果快照）。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct FeedReport {
    /// 拍序号。
    pub beat_seq: u64,
    /// 同拍对齐判定。
    pub aligned: bool,
    /// 时序偏差（µs）。
    pub skew_us: u32,
    /// 降级标注（任一通道缺失）。
    pub degraded: bool,
    /// 降级状态机迁移后状态。
    pub degrade_after: DegradeState,
    /// 交叉判定留痕（首拍为 None）。
    pub verdict: Option<VerdictRecord>,
    /// 温度选中源名（无温度通道时 None）。
    pub temp_source: Option<&'static str>,
    /// 是否入画像。
    pub profile_recorded: bool,
    /// 本拍是否超开销预算。
    pub overhead_budget_hit: bool,
}

/// 回放脚本条目（对拍/注入用）：读数组 + 声明的开销。
#[derive(Clone, Debug)]
pub struct BeatScript {
    /// 本拍通道读数。
    pub readings: Vec<ChannelReading>,
    /// 本拍联合采样开销（µs，平台实测值声明）。
    pub overhead_us: u32,
}

impl BeatScript {
    /// 全通道脚本条目（功耗+GPU 结温+频率，时戳错开 40µs 内）。
    pub fn full(power_mw: u32, temp_mdegc: u32, tsc_us: u64) -> Self {
        BeatScript {
            readings: vec![
                ChannelReading::power_mw(power_mw, tsc_us),
                ChannelReading::temp(SRC_GPU_DIE, temp_mdegc, tsc_us + 20),
                ChannelReading::freq_khz(1_800_000, tsc_us + 40),
            ],
            overhead_us: joint_nominal_us(),
        }
    }
}

/// 健康报告。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct HealthReport {
    /// 总体健康。
    pub ok: bool,
    /// 已喂拍数（含被拒收）。
    pub beats_fed: u64,
    /// 受理拍数。
    pub beats_accepted: u64,
    /// 拒收拍数。
    pub beats_rejected: u64,
    /// 降级期拍数。
    pub degraded_beats: u64,
    /// 超开销预算拍数。
    pub budget_exceeded: u32,
    /// 曲线单调性违约数。
    pub mono_violations: u32,
    /// 账面在账条目数。
    pub ledger_len: usize,
    /// 账面让位淘汰数。
    pub ledger_dropped: u64,
}

/// 联合采样站：把七个子系统串成一条喂拍流水线。
///
/// 喂拍全链：序号守卫 → 收帧（哨兵红线）→ 时戳单调守卫 → 对齐/偏差账 →
/// 缺失降级状态机 → 温度源优先级与切换账 → 联合画像 → 交叉规则 →
/// 开销账 → 前拍更新。
pub struct JointSampStation {
    seed: u64,
    profiler: JointProfiler,
    rules: CrossRuleEngine,
    switches: SensorSwitchLedger,
    overhead: OverheadLedger,
    degrade: MissingDegradation,
    ledger: EventLedger,
    prev: Option<JointSample>,
    next_seq: u64,
    beats_accepted: u64,
    beats_rejected: u64,
}

impl JointSampStation {
    /// 新站（seed 参与指纹，用于双跑对拍分档）。
    pub fn new(seed: u64) -> Result<Self, JointSampError> {
        Ok(JointSampStation {
            seed,
            profiler: JointProfiler::new(),
            rules: CrossRuleEngine::new(),
            switches: SensorSwitchLedger::new(),
            overhead: OverheadLedger::new(),
            degrade: MissingDegradation::new(),
            ledger: EventLedger::new(),
            prev: None,
            next_seq: 0,
            beats_accepted: 0,
            beats_rejected: 0,
        })
    }

    /// 下一期望拍序号（喂拍顺序守卫用）。
    pub fn next_seq(&self) -> u64 {
        self.next_seq
    }

    /// 受理拍数。
    pub fn beats_accepted(&self) -> u64 {
        self.beats_accepted
    }

    /// 拒收拍数。
    pub fn beats_rejected(&self) -> u64 {
        self.beats_rejected
    }

    /// 降级状态。
    pub fn degrade_state(&self) -> DegradeState {
        self.degrade.state()
    }

    /// 事件账（只读）。
    pub fn ledger(&self) -> &EventLedger {
        &self.ledger
    }

    /// 联合画像器（只读）。
    pub fn profiler(&self) -> &JointProfiler {
        &self.profiler
    }

    /// 交叉规则引擎（只读）。
    pub fn rules(&self) -> &CrossRuleEngine {
        &self.rules
    }

    /// 温度源切换账（只读）。
    pub fn switches(&self) -> &SensorSwitchLedger {
        &self.switches
    }

    /// 开销账（只读）。
    pub fn overhead(&self) -> &OverheadLedger {
        &self.overhead
    }

    /// 喂一拍（全链）。
    pub fn feed(
        &mut self,
        beat_seq: u64,
        readings: &[ChannelReading],
        overhead_us: u32,
    ) -> Result<FeedReport, JointSampError> {
        // 序号守卫：必须严格按站内期望序号喂拍（断链拒收，不静默对齐）。
        if beat_seq != self.next_seq {
            return Err(JointSampError::SeqGap { expected: self.next_seq, got: beat_seq });
        }
        self.next_seq = self.next_seq.saturating_add(1);
        // 收帧：哨兵值红线在此拒收（缺失走降级，不走补齐）。
        let s = match build_joint_sample(beat_seq, readings) {
            Ok(s) => s,
            Err(e) => {
                if let JointSampError::FakeFillSentinel { .. } = e {
                    self.degrade.note_fake_fill_rejected();
                    self.ledger.push(
                        JointSampCode::FakeFillRejected,
                        beat_seq,
                        0,
                        "哨兵值红线立案：缺失走降级，不走补齐",
                    );
                }
                self.beats_rejected = self.beats_rejected.saturating_add(1);
                return Err(e);
            }
        };
        // 时戳单调守卫：dt 必须为正（否则斜率不可算，拒收不判定）。
        if let Some(p) = &self.prev {
            if s.beat_tsc_us <= p.beat_tsc_us {
                self.beats_rejected = self.beats_rejected.saturating_add(1);
                return Err(JointSampError::ZeroInterval { beat: beat_seq });
            }
        }
        // 对齐/偏差账：偏差账逐拍留痕（预警档在判据内显性预警）。
        if s.aligned {
            self.ledger.push(JointSampCode::BeatAligned, beat_seq, s.skew_us as u64, "");
            if s.skew_us >= SKEW_WARN_US {
                self.ledger.push(
                    JointSampCode::BeatSkewed,
                    beat_seq,
                    s.skew_us as u64,
                    "预警档（判据内，接近越界）",
                );
            }
        } else {
            self.ledger.push(
                JointSampCode::BeatSkewed,
                beat_seq,
                s.skew_us as u64,
                "超差——不冒充同拍，不入画像不判定",
            );
        }
        // 温度源优先级：选中源切换账（多源同采取最高优先级）。
        let mut temp_source_name = None;
        if let Some(src) = s.temp_source_id.and_then(sensor_source_by_id) {
            let gpu_present = readings
                .iter()
                .any(|r| r.channel == Channel::Temp && r.source_id == SRC_GPU_DIE);
            self.switches.observe(src, gpu_present, beat_seq, &mut self.ledger);
            temp_source_name = Some(src.name);
        }
        // 缺失降级状态机（降级标注由此向下游传播）。
        let transition = self.degrade.observe(s.present, beat_seq, &mut self.ledger);
        // 联合画像（超偏差/缺通道拍在画像器内显性排除）。
        let delta = self.profiler.record(&s, &mut self.ledger);
        // 交叉规则（前拍/当前拍无效时不判定并留痕）。
        let verdict = self.rules.evaluate(self.prev.as_ref(), &s, &mut self.ledger);
        // 开销账（判据五逐拍判定；溢出走拒收计数，保持守恒自证）。
        let oh = match self.overhead.record(overhead_us, beat_seq, &mut self.ledger) {
            Ok(o) => o,
            Err(e) => {
                self.beats_rejected = self.beats_rejected.saturating_add(1);
                return Err(e);
            }
        };
        self.prev = Some(s);
        self.beats_accepted = self.beats_accepted.saturating_add(1);
        Ok(FeedReport {
            beat_seq,
            aligned: s.aligned,
            skew_us: s.skew_us,
            degraded: s.degraded,
            degrade_after: transition.after,
            verdict,
            temp_source: temp_source_name,
            profile_recorded: delta.recorded,
            overhead_budget_hit: oh.budget_hit,
        })
    }

    /// 单调性审计转发（画像器曲线倒挂检查，增量入账）。
    pub fn audit_monotonic(&mut self) -> u32 {
        let beat = self.next_seq.saturating_sub(1);
        self.profiler.audit_monotonic(&mut self.ledger, beat)
    }

    /// 账体检：seq 连续性 + 计数守恒 + 画像器与受理计数对账。
    pub fn health(&self) -> Result<HealthReport, JointSampError> {
        self.ledger.health()?;
        if self.profiler.beats_total() != self.beats_accepted {
            return Err(JointSampError::LedgerCorrupted);
        }
        if self.beats_accepted.saturating_add(self.beats_rejected) != self.next_seq {
            return Err(JointSampError::LedgerCorrupted);
        }
        Ok(HealthReport {
            ok: true,
            beats_fed: self.next_seq,
            beats_accepted: self.beats_accepted,
            beats_rejected: self.beats_rejected,
            degraded_beats: self.degrade.degraded_beats(),
            budget_exceeded: self.overhead.budget_exceeded(),
            mono_violations: self.profiler.mono_violations(),
            ledger_len: self.ledger.len(),
            ledger_dropped: self.ledger.dropped(),
        })
    }

    /// 全站指纹（FNV-1a，跨子系统状态混入）。
    pub fn fingerprint(&self) -> u64 {
        let mut h = FNV_OFFSET;
        h = mix_u64(h, self.seed);
        h = mix_u64(h, self.next_seq);
        h = mix_u64(h, self.beats_accepted);
        h = mix_u64(h, self.beats_rejected);
        h = self.profiler.fingerprint_state(h);
        h = self.rules.fingerprint_state(h);
        h = self.switches.fingerprint_state(h);
        h = self.overhead.fingerprint_state(h);
        h = self.degrade.fingerprint_state(h);
        h = self.ledger.fingerprint_seed(h);
        h
    }

    /// 双跑确定性对拍：同脚本在两台同种子新站上重放，指纹必须一致。
    pub fn determinism_pair(
        &self,
        script: &[BeatScript],
    ) -> Result<(u64, u64), JointSampError> {
        let mut a = JointSampStation::new(self.seed)?;
        let mut b = JointSampStation::new(self.seed)?;
        let mut i = 0usize;
        while i < script.len() {
            let item = &script[i];
            a.feed(a.next_seq(), &item.readings, item.overhead_us)?;
            b.feed(b.next_seq(), &item.readings, item.overhead_us)?;
            i += 1;
        }
        let fa = a.fingerprint();
        let fb = b.fingerprint();
        if fa != fb {
            return Err(JointSampError::DeterminismBroken);
        }
        Ok((fa, fb))
    }
}

// ---------------------------------------------------------------------------
// 注入面（判据七）：超偏差注入 / 通道缺失注入 / 交叉规则违例注入
// ---------------------------------------------------------------------------

/// 注入统计与门禁。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct InjectStats {
    /// 超偏差注入是否检出（aligned=false 显性标注）。
    pub skewed_detected: bool,
    /// 交叉规则违例注入是否检出（R2 散热异常结论）。
    pub rule_violation_detected: bool,
    /// 通道缺失注入是否检出（进入单通道模式）。
    pub missing_detected: bool,
    /// 恢复路径是否走通（连续 T17 拍回双通道）。
    pub recovery_ok: bool,
    /// 干净序列拍数。
    pub clean_beats: u32,
    /// 干净序列误报事件数。
    pub clean_fp_events: u32,
    /// 干净序列误报率（ppm）。
    pub fp_ppm: u64,
    /// 总门禁（三类注入全检出 + 恢复走通 + 误报率 ≤ T19）。
    pub gate_ok: bool,
}

/// 注入台面（判据七）：三类注入 + 恢复路径 + 干净序列误报门禁。
pub struct InjectRig;

impl InjectRig {
    /// 超偏差注入：单拍三通道时戳拉距 5000µs（> T01 判据）。
    pub fn case_skewed() -> Vec<BeatScript> {
        vec![BeatScript {
            readings: vec![
                ChannelReading::power_mw(250_000, 1_000),
                ChannelReading::temp(SRC_GPU_DIE, 45_000, 6_000),
                ChannelReading::freq_khz(1_800_000, 1_010),
            ],
            overhead_us: joint_nominal_us(),
        }]
    }

    /// 交叉规则违例注入：功耗平 + 温度骤升 → 应判 R2 散热异常。
    /// （基线拍 + 触发拍，间隔 10ms；温升 8000m°C/10ms = 800 m°C/ms ≥ T15。）
    pub fn case_rule_violation() -> Vec<BeatScript> {
        vec![
            BeatScript::full(250_000, 45_000, 20_000),
            BeatScript::full(250_000, 53_000, 30_000),
        ]
    }

    /// 通道缺失注入：连续 T16+1 拍缺温度通道（功耗+频率仍在）。
    pub fn case_missing() -> Vec<BeatScript> {
        let mut out = Vec::new();
        let mut i = 0u32;
        while i <= MISSING_GRACE_BEATS {
            let tsc = 40_000u64 + i as u64 * 10_000;
            out.push(BeatScript {
                readings: vec![
                    ChannelReading::power_mw(200_000, tsc),
                    ChannelReading::freq_khz(1_800_000, tsc + 10),
                ],
                overhead_us: 250,
            });
            i += 1;
        }
        out
    }

    /// 恢复路径：连续 T17 拍全通道在位（常值平稳序列）。
    pub fn case_recovery() -> Vec<BeatScript> {
        let mut out = Vec::new();
        let mut i = 0u32;
        while i < RECOVERY_CONFIRM_BEATS {
            let tsc = 80_000u64 + i as u64 * 10_000;
            out.push(BeatScript::full(200_000, 45_000, tsc));
            i += 1;
        }
        out
    }

    /// 干净序列：6 拍缓变平稳（功耗斜率 200mW/ms = Flat，温度斜率
    /// 1.5m°C/ms = Flat）→ 全部应判 R4 稳态，零误报。
    pub fn case_clean() -> Vec<BeatScript> {
        let mut out = Vec::new();
        let mut i = 0u32;
        while i < 6 {
            let tsc = 160_000u64 + i as u64 * 10_000;
            out.push(BeatScript::full(202_000 + i * 2_000, 45_015 + i * 15, tsc));
            i += 1;
        }
        out
    }

    /// 台面回放：按站内序号顺序喂入脚本。
    fn play(
        station: &mut JointSampStation,
        scripts: &[BeatScript],
    ) -> Result<Vec<FeedReport>, JointSampError> {
        let mut out = Vec::new();
        let mut i = 0usize;
        while i < scripts.len() {
            let seq = station.next_seq();
            let s = &scripts[i];
            out.push(station.feed(seq, &s.readings, s.overhead_us)?);
            i += 1;
        }
        Ok(out)
    }

    /// 全量注入：超偏差 → 交叉违例 → 缺失 → 恢复 → 干净序列门禁。
    pub fn run_all(station: &mut JointSampStation) -> Result<InjectStats, JointSampError> {
        // 1. 超偏差注入：本拍 aligned 必须为 false（不冒充同拍）。
        let skewed_reports = Self::play(station, &Self::case_skewed())?;
        let skewed_detected = skewed_reports.iter().any(|r| !r.aligned);
        // 2. 交叉规则违例注入：功耗平 + 温度骤升 → R2 散热异常。
        let rule_reports = Self::play(station, &Self::case_rule_violation())?;
        let rule_violation_detected = rule_reports.iter().any(|r| {
            matches!(r.verdict, Some(ref v) if v.verdict == RuleVerdict::CoolingAbnormal)
        });
        // 3. 通道缺失注入：连续缺失必须进入单通道模式。
        let _missing_reports = Self::play(station, &Self::case_missing())?;
        let missing_detected = station.degrade_state() != DegradeState::DualChannel;
        // 4. 恢复路径：连续 T17 拍全通道在位自动回双通道。
        let _recovery_reports = Self::play(station, &Self::case_recovery())?;
        let recovery_ok = station.degrade_state() == DegradeState::DualChannel;
        // 5. 干净序列：稳态判定 + 误报率 ppm 门禁（T19）。
        let clean_reports = Self::play(station, &Self::case_clean())?;
        let clean_beats = clean_reports.len() as u32;
        let mut fp = 0u32;
        let mut i = 0usize;
        while i < clean_reports.len() {
            let r = &clean_reports[i];
            let adverse = !r.aligned
                || r.degraded
                || match &r.verdict {
                    Some(v) => {
                        v.verdict != RuleVerdict::Normal && v.verdict != RuleVerdict::SteadyState
                    }
                    None => true,
                };
            if adverse {
                fp = fp.saturating_add(1);
            }
            i += 1;
        }
        let fp_ppm = if clean_beats == 0 {
            0
        } else {
            fp as u64 * 1_000_000 / clean_beats as u64
        };
        let gate_ok = skewed_detected
            && rule_violation_detected
            && missing_detected
            && recovery_ok
            && fp_ppm <= FP_PPM_GATE;
        Ok(InjectStats {
            skewed_detected,
            rule_violation_detected,
            missing_detected,
            recovery_ok,
            clean_beats,
            clean_fp_events: fp,
            fp_ppm,
            gate_ok,
        })
    }
}

// ---------------------------------------------------------------------------
// 文档导出（阈值全显性——黑盒红线自证）
// ---------------------------------------------------------------------------

/// 常量文档：T01~T23 逐条导出（数值 + 来源），判定可追溯。
pub fn constants_doc() -> String {
    let mut s = String::new();
    s.push_str(&format!("T01 align_tolerance_us = {}\n", ALIGN_TOLERANCE_US));
    s.push_str(&format!("T02 skew_warn_us = {}\n", SKEW_WARN_US));
    s.push_str(&format!("T03 joint_budget_us = {}\n", JOINT_BUDGET_US));
    s.push_str(&format!("T04 discrete_per_channel_us = {}\n", DISCRETE_PER_CHANNEL_US));
    s.push_str(&format!("T05 discrete_dispatch_us = {}\n", DISCRETE_DISPATCH_US));
    s.push_str(&format!("T06 joint_dispatch_us = {}\n", JOINT_DISPATCH_US));
    s.push_str(&format!("T07 joint_batch_read_us = {}\n", JOINT_BATCH_READ_US));
    s.push_str(&format!("T08 bucket_p = {}\n", BUCKET_P));
    s.push_str(&format!("T09 bucket_t = {}\n", BUCKET_T));
    s.push_str(&format!("T10 p_range_mw = {}\n", P_RANGE_MW));
    s.push_str(&format!("T11 t_range_mdegc = {}\n", T_RANGE_MDEGC));
    s.push_str(&format!("T12 pwr_flat_slope_mw_per_ms = {}\n", PWR_FLAT_SLOPE_MW_PER_MS));
    s.push_str(&format!("T13 pwr_steep_slope_mw_per_ms = {}\n", PWR_STEEP_SLOPE_MW_PER_MS));
    s.push_str(&format!("T14 temp_flat_slope_mdegc_per_ms = {}\n", TEMP_FLAT_SLOPE_MDEGC_PER_MS));
    s.push_str(&format!("T15 temp_steep_slope_mdegc_per_ms = {}\n", TEMP_STEEP_SLOPE_MDEGC_PER_MS));
    s.push_str(&format!("T16 missing_grace_beats = {}\n", MISSING_GRACE_BEATS));
    s.push_str(&format!("T17 recovery_confirm_beats = {}\n", RECOVERY_CONFIRM_BEATS));
    s.push_str(&format!("T18 ledger_cap = {}\n", LEDGER_CAP));
    s.push_str(&format!("T19 fp_ppm_gate = {}\n", FP_PPM_GATE));
    s.push_str(&format!("T20 mono_tolerance_buckets = {}\n", MONO_TOLERANCE_BUCKETS));
    s.push_str(&format!(
        "T21 fake_fill_sentinel = 0x{:08X}\n",
        FAKE_FILL_SENTINEL
    ));
    s.push_str(&format!(
        "discrete_total_us = {} / joint_nominal_us = {}\n",
        discrete_total_us(),
        joint_nominal_us()
    ));
    s
}

/// 交叉规则表文档导出（四规则 + 动作建议，判定口径公开）。
pub fn rules_doc() -> String {
    let mut s = String::new();
    let mut i = 0usize;
    while i < CROSS_RULES.len() {
        let r = &CROSS_RULES[i];
        s.push_str(&format!(
            "R{} {}｜{}+{}→{}｜建议：{}\n",
            r.id,
            r.name,
            r.power.label(),
            r.temp.label(),
            r.verdict.label(),
            r.advice
        ));
        i += 1;
    }
    s
}

// ---------------------------------------------------------------------------
// 测试（判据七：对齐精度 / 交叉规则 / 缺失降级三组 + 画像 / 开销 / 对拍）
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    const STEP: u64 = 10_000; // 10ms 拍间隔

    /// 全通道读数组（功耗 + GPU 结温 + 频率，时戳错开 ≤40µs）。
    fn full_readings(power_mw: u32, temp_mdegc: u32, tsc_us: u64) -> Vec<ChannelReading> {
        vec![
            ChannelReading::power_mw(power_mw, tsc_us),
            ChannelReading::temp(SRC_GPU_DIE, temp_mdegc, tsc_us + 20),
            ChannelReading::freq_khz(1_800_000, tsc_us + 40),
        ]
    }

    /// 仅功耗 + 频率（缺温度通道）。
    fn pf_readings(power_mw: u32, tsc_us: u64) -> Vec<ChannelReading> {
        vec![
            ChannelReading::power_mw(power_mw, tsc_us),
            ChannelReading::freq_khz(1_800_000, tsc_us + 10),
        ]
    }

    // ---- 判据一组：同拍对齐精度 ----

    #[test]
    fn joint_sample_alignment_within_tolerance() {
        // 40µs 偏差：判据（<1ms）内，aligned = true。
        let s = build_joint_sample(0, &full_readings(200_000, 45_000, 1_000)).unwrap();
        assert!(s.aligned);
        assert_eq!(s.skew_us, 40);
        assert_eq!(s.beat_tsc_us, 1_040); // 同拍时戳 = 三通道时戳最晚者
        assert!(!s.degraded);
        assert!(s.is_same_beat());
        assert_eq!(s.temp_source_id, Some(SRC_GPU_DIE));
    }

    #[test]
    fn joint_sample_skew_warn_band_stays_aligned() {
        // 850µs：落在预警带（T02 ≤ skew < T01），判据内仍 aligned，
        // 但站点必须出 BeatSkewed 预警档事件（不静默）。
        let readings = vec![
            ChannelReading::power_mw(200_000, 0),
            ChannelReading::temp(SRC_GPU_DIE, 45_000, 850),
            ChannelReading::freq_khz(1_800_000, 0),
        ];
        let s = build_joint_sample(0, &readings).unwrap();
        assert!(s.aligned);
        assert_eq!(s.skew_us, 850);
        let mut st = JointSampStation::new(1).unwrap();
        let rep = st.feed(st.next_seq(), &readings, 270).unwrap();
        assert!(rep.aligned);
        assert_eq!(st.ledger().count_of(JointSampCode::BeatSkewed), 1);
        assert_eq!(st.ledger().count_of(JointSampCode::BeatAligned), 1);
    }

    #[test]
    fn joint_sample_skew_over_tolerance_not_same_beat() {
        // 5000µs：超 T01 判据——aligned = false，绝不冒充同拍。
        let readings = vec![
            ChannelReading::power_mw(250_000, 1_000),
            ChannelReading::temp(SRC_GPU_DIE, 45_000, 6_000),
            ChannelReading::freq_khz(1_800_000, 1_010),
        ];
        let s = build_joint_sample(0, &readings).unwrap();
        assert!(!s.aligned);
        assert_eq!(s.skew_us, 5_000);
        assert!(!s.is_same_beat());
        let mut st = JointSampStation::new(1).unwrap();
        let rep = st.feed(st.next_seq(), &readings, 270).unwrap();
        assert!(!rep.aligned);
        assert!(!rep.profile_recorded); // 超偏差拍不入画像
        assert_eq!(st.profiler().beats_skewed_excluded(), 1);
        assert_eq!(st.ledger().count_of(JointSampCode::BeatSkewed), 1);
    }

    #[test]
    fn joint_sample_skew_overflow_rejected() {
        // 时戳跨度超 u32 域：显性报错，不静默截断。
        let readings = vec![
            ChannelReading::power_mw(200_000, 0),
            ChannelReading::temp(SRC_GPU_DIE, 45_000, u32::MAX as u64 + 10),
        ];
        let e = build_joint_sample(0, &readings).unwrap_err();
        assert!(matches!(e, JointSampError::SkewOverflow { .. }));
    }

    #[test]
    fn empty_beat_and_seq_gap_and_zero_interval_rejected() {
        let mut st = JointSampStation::new(1).unwrap();
        // 空拍。
        assert_eq!(st.feed(st.next_seq(), &[], 270), Err(JointSampError::EmptyBeat));
        // 序号断链。
        let first = st.next_seq();
        assert_eq!(
            st.feed(first + 5, &full_readings(200_000, 45_000, 1_000), 270),
            Err(JointSampError::SeqGap { expected: first, got: first + 5 })
        );
        // 正常首拍。
        st.feed(st.next_seq(), &full_readings(200_000, 45_000, 1_000), 270).unwrap();
        // 时戳无进展（dt = 0）：斜率不可算，拒收（此时站内序号已推进到 2）。
        assert_eq!(
            st.feed(st.next_seq(), &full_readings(200_000, 45_000, 1_000), 270),
            Err(JointSampError::ZeroInterval { beat: 2 })
        );
        // 计数守恒：受理 1 + 拒收 2（空拍 + 时戳无进展）= 已喂 3。
        assert_eq!(st.health().unwrap().beats_fed, 3);
        assert_eq!(st.health().unwrap().beats_rejected, 2);
        assert!(st.health().unwrap().ok);
    }

    // ---- 不造假补齐红线 ----

    #[test]
    fn fake_fill_sentinel_rejected_and_recorded() {
        let mut st = JointSampStation::new(7).unwrap();
        st.feed(st.next_seq(), &full_readings(200_000, 45_000, 1_000), 270).unwrap();
        // 注入哨兵值（假装温度存在）：必须拒收 + 立案 + 计数。
        let readings = vec![
            ChannelReading::power_mw(200_000, 11_000),
            ChannelReading::temp(SRC_GPU_DIE, FAKE_FILL_SENTINEL, 11_020),
            ChannelReading::freq_khz(1_800_000, 11_040),
        ];
        let e = st.feed(st.next_seq(), &readings, 270).unwrap_err();
        assert_eq!(
            e,
            JointSampError::FakeFillSentinel { channel: Channel::Temp as u8 }
        );
        assert_eq!(st.ledger().count_of(JointSampCode::FakeFillRejected), 1);
        assert_eq!(st.beats_rejected(), 1);
        // 站点拒收后序号照常推进（拒收也消耗一拍序号，账面可对拍）。
        let rep = st
            .feed(st.next_seq(), &full_readings(200_000, 45_000, 21_000), 270)
            .unwrap();
        assert!(rep.aligned);
        assert!(st.health().unwrap().ok);
    }

    // ---- 判据二组：联合画像与响应曲线 ----

    #[test]
    fn profiler_curve_monotonic_ramp() {
        let mut st = JointSampStation::new(3).unwrap();
        // 64 拍缓升：功耗 7mW/ms（Flat 档）、温度 1.5m°C/ms（Flat 档）。
        let mut i = 0u32;
        while i < 64 {
            let p = 10_000 + i * 7_000;
            let t = 30_000 + i * 15;
            let rep = st
                .feed(st.next_seq(), &full_readings(p, t, i as u64 * STEP), 270)
                .unwrap();
            assert!(rep.profile_recorded);
            i += 1;
        }
        let prof = st.profiler();
        assert_eq!(prof.beats_used(), 64);
        assert_eq!(prof.beats_skewed_excluded(), 0);
        assert_eq!(prof.beats_skipped_missing(), 0);
        let curve = prof.response_curve();
        assert!(curve.occupied >= 14); // 功耗桶 0~14 应全部非空
        assert_eq!(curve.violations, 0);
        // 中位温度桶沿功耗桶单调不减（缓升序列不倒挂）。
        let mut prev_tb: Option<usize> = None;
        for b in curve.bands.iter() {
            if let Some(band) = b {
                if let Some(pb) = prev_tb {
                    assert!(band.t_bucket >= pb);
                }
                prev_tb = Some(band.t_bucket);
                // 分位数带自洽：P25 ≤ P50 ≤ P75。
                assert!(band.p25_mdegc <= band.p50_mdegc);
                assert!(band.p50_mdegc <= band.p75_mdegc);
            }
        }
        // 单调性审计零违约，F1473 输入接口质量门通过。
        assert_eq!(st.audit_monotonic(), 0);
        let hmi = st.profiler().hot_model_input();
        assert!(hmi.aligned_only);
        assert!(hmi.sink.contains("F1473"));
        assert!(hmi.quality_ok);
        assert_eq!(hmi.beats_used, 64);
    }

    #[test]
    fn profiler_skipped_and_excluded_accounting() {
        let mut st = JointSampStation::new(3).unwrap();
        st.feed(st.next_seq(), &full_readings(200_000, 45_000, 0), 270).unwrap();
        // 缺温度拍：画像跳过（不造假补齐）。
        st.feed(st.next_seq(), &pf_readings(200_000, STEP), 270).unwrap();
        // 超偏差拍：画像排除（不冒充同拍）。
        let skewed = vec![
            ChannelReading::power_mw(200_000, 2 * STEP),
            ChannelReading::temp(SRC_GPU_DIE, 45_000, 2 * STEP + 5_000),
            ChannelReading::freq_khz(1_800_000, 2 * STEP + 10),
        ];
        st.feed(st.next_seq(), &skewed, 270).unwrap();
        let prof = st.profiler();
        assert_eq!(prof.beats_used(), 1);
        assert_eq!(prof.beats_skipped_missing(), 1);
        assert_eq!(prof.beats_skewed_excluded(), 1);
        assert_eq!(prof.beats_total(), 3);
        assert!(st.health().unwrap().ok);
    }

    #[test]
    fn profiler_monotonic_violation_detected_once() {
        let mut st = JointSampStation::new(3).unwrap();
        // 低功耗高结温 → 高功耗低结温：中位桶倒挂 13 桶 >> T20 容忍。
        st.feed(st.next_seq(), &full_readings(10_000, 140_000, 0), 270).unwrap();
        st.feed(st.next_seq(), &full_readings(480_000, 10_000, STEP), 270).unwrap();
        let v1 = st.audit_monotonic();
        assert!(v1 >= 1);
        assert_eq!(st.profiler().mono_violations(), v1);
        // 违约立案入账；重复审计不重复立案（增量语义）。
        assert_eq!(st.ledger().count_of(JointSampCode::CurveMonotonicViolated), 1);
        assert_eq!(st.audit_monotonic(), v1);
        assert_eq!(st.ledger().count_of(JointSampCode::CurveMonotonicViolated), 1);
    }

    // ---- 判据三组：交叉规则 ----

    #[test]
    fn cross_rule_four_matches_and_unmatched_end_to_end() {
        let mut st = JointSampStation::new(5).unwrap();
        // beat0：基线（首拍无判定）。
        st.feed(st.next_seq(), &full_readings(100_000, 40_000, 0), 270).unwrap();
        // R1：功耗骤升（15W/ms）+ 温度缓升（5m°C/ms，整数斜率落在缓升带）→ 正常（热滞后）。
        let r1 = st
            .feed(st.next_seq(), &full_readings(250_000, 40_050, STEP), 270)
            .unwrap();
        // R2：功耗平 + 温度骤升（800m°C/ms）→ 散热异常。
        let r2 = st
            .feed(st.next_seq(), &full_readings(250_000, 48_015, 2 * STEP), 270)
            .unwrap();
        // R3：双骤升 → 瞬时过载。
        let r3 = st
            .feed(st.next_seq(), &full_readings(400_000, 48_215, 3 * STEP), 270)
            .unwrap();
        // R4：双平 → 稳态。
        let r4 = st
            .feed(st.next_seq(), &full_readings(400_000, 48_215, 4 * STEP), 270)
            .unwrap();
        // 未匹配：功耗骤升 + 温度平 → 显性 Unknown（不猜）。
        let r5 = st
            .feed(st.next_seq(), &full_readings(550_000, 48_225, 5 * STEP), 270)
            .unwrap();
        assert_eq!(r1.verdict.as_ref().unwrap().verdict, RuleVerdict::Normal);
        assert_eq!(r1.verdict.as_ref().unwrap().matched_rule, Some(1));
        assert_eq!(r2.verdict.as_ref().unwrap().verdict, RuleVerdict::CoolingAbnormal);
        assert_eq!(r2.verdict.as_ref().unwrap().matched_rule, Some(2));
        assert_eq!(r3.verdict.as_ref().unwrap().verdict, RuleVerdict::TransientOverload);
        assert_eq!(r3.verdict.as_ref().unwrap().matched_rule, Some(3));
        assert_eq!(r4.verdict.as_ref().unwrap().verdict, RuleVerdict::SteadyState);
        assert_eq!(r4.verdict.as_ref().unwrap().matched_rule, Some(4));
        let v5 = r5.verdict.as_ref().unwrap();
        assert_eq!(v5.verdict, RuleVerdict::Unknown);
        assert_eq!(v5.matched_rule, None);
        assert!(v5.trace.contains("不猜结论"));
        assert_eq!(st.rules().matched(), 4);
        assert_eq!(st.rules().unmatched(), 1);
        assert_eq!(st.rules().per_rule(), [1, 1, 1, 1]);
        // 判定留痕：斜率全程可追溯（R1 功耗斜率恰为 15000mW/ms 判据值）。
        assert_eq!(r1.verdict.as_ref().unwrap().power_slope_mw_per_ms, 15_000);
        assert_eq!(r2.verdict.as_ref().unwrap().temp_slope_mdegc_per_ms, 796);
        assert!(r1.verdict.as_ref().unwrap().trace.contains("R1"));
        // 事件账：四命中 + 一未匹配。
        assert_eq!(st.ledger().count_of(JointSampCode::CrossRuleMatched), 4);
        assert_eq!(st.ledger().count_of(JointSampCode::CrossRuleUnmatched), 1);
    }

    #[test]
    fn cross_rule_degraded_or_skewed_beat_not_judged() {
        let mut st = JointSampStation::new(5).unwrap();
        st.feed(st.next_seq(), &full_readings(200_000, 45_000, 0), 270).unwrap();
        // 降级拍（缺温度）：不判定 + 留痕（缺失降级沿规则链传播）。
        let rep = st.feed(st.next_seq(), &pf_readings(200_000, STEP), 270).unwrap();
        let v = rep.verdict.as_ref().unwrap();
        assert_eq!(v.verdict, RuleVerdict::Unknown);
        assert!(v.degraded);
        assert!(v.trace.contains("当前拍无效"));
        assert_eq!(st.rules().skipped_invalid(), 1);
        // 超偏差拍：不判定 + 留痕。
        let skewed = vec![
            ChannelReading::power_mw(200_000, 2 * STEP),
            ChannelReading::temp(SRC_GPU_DIE, 45_000, 2 * STEP + 5_000),
            ChannelReading::freq_khz(1_800_000, 2 * STEP + 10),
        ];
        let rep2 = st.feed(st.next_seq(), &skewed, 270).unwrap();
        let v2 = rep2.verdict.as_ref().unwrap();
        assert_eq!(v2.verdict, RuleVerdict::Unknown);
        assert!(!v2.aligned);
        assert!(v2.trace.contains("当前拍无效"));
        assert_eq!(st.rules().skipped_invalid(), 2);
    }

    #[test]
    fn cross_rules_doc_export() {
        let doc = rules_doc();
        assert!(doc.contains("R1 功耗骤升+温度缓升→正常"));
        assert!(doc.contains("R2 功耗平+温度骤升→散热异常"));
        assert!(doc.contains("R3 双骤升→瞬时过载"));
        assert!(doc.contains("R4 双平→稳态"));
    }

    // ---- 判据四组：传感器优先级 ----

    #[test]
    fn sensor_priority_multi_source_resolution() {
        // 三源同拍：取 GPU 结温（rank3）。
        let readings = vec![
            ChannelReading::power_mw(200_000, 1_000),
            ChannelReading::temp(SRC_GPU_DIE, 45_000, 1_000),
            ChannelReading::temp(SRC_VRM, 60_000, 1_002),
            ChannelReading::temp(SRC_AMBIENT, 30_000, 1_004),
        ];
        let s = build_joint_sample(0, &readings).unwrap();
        assert_eq!(s.temp_mdegc, 45_000);
        assert_eq!(s.temp_source_id, Some(SRC_GPU_DIE));
        // 双源：VRM > 环境。
        let readings2 = vec![
            ChannelReading::temp(SRC_AMBIENT, 30_000, 1_000),
            ChannelReading::temp(SRC_VRM, 60_000, 1_002),
        ];
        let s2 = build_joint_sample(0, &readings2).unwrap();
        assert_eq!(s2.temp_mdegc, 60_000);
        assert_eq!(s2.temp_source_id, Some(SRC_VRM));
        // 仅环境源：兜底。
        let s3 = build_joint_sample(
            0,
            &[ChannelReading::temp(SRC_AMBIENT, 30_000, 1_000)],
        )
        .unwrap();
        assert_eq!(s3.temp_source_id, Some(SRC_AMBIENT));
        // 未登记源：忽略（宁缺毋滥）。帧内仅有未登记温度源时无任何可用
        // 通道 → 空拍拒收（诚实报错，不猜源秩）。
        assert_eq!(
            build_joint_sample(0, &[ChannelReading::temp(9, 50_000, 1_000)]),
            Err(JointSampError::EmptyBeat)
        );
        // 未登记源 + 功耗通道：温度按缺失处理，功耗正常成帧（降级标注）。
        let s4 = build_joint_sample(
            0,
            &[
                ChannelReading::power_mw(200_000, 1_000),
                ChannelReading::temp(9, 50_000, 1_002),
            ],
        )
        .unwrap();
        assert_eq!(s4.temp_source_id, None);
        assert!(!s4.has(Channel::Temp));
        assert!(s4.degraded);
    }

    #[test]
    fn sensor_switch_and_fallback_ledger() {
        let mut st = JointSampStation::new(9).unwrap();
        // 拍0：GPU 结温在位 → 正常选用。
        st.feed(st.next_seq(), &full_readings(200_000, 45_000, 0), 270).unwrap();
        assert_eq!(st.switches().per_source()[SRC_GPU_DIE as usize], 1);
        // 拍1：仅 VRM → 降源回退 + 切换各立案一次。
        let vrm_only = vec![
            ChannelReading::power_mw(200_000, STEP),
            ChannelReading::temp(SRC_VRM, 60_000, STEP + 2),
            ChannelReading::freq_khz(1_800_000, STEP + 4),
        ];
        let rep = st.feed(st.next_seq(), &vrm_only, 270).unwrap();
        assert_eq!(rep.temp_source, Some("VRM 温度"));
        assert_eq!(st.switches().fallbacks(), 1);
        assert_eq!(st.switches().switches(), 1);
        assert_eq!(st.ledger().count_of(JointSampCode::SensorFallbackUsed), 1);
        assert_eq!(st.ledger().count_of(JointSampCode::SensorSourceSwitched), 1);
        // 拍2：GPU 结温恢复 → 再次切换（回落主源）。
        st.feed(st.next_seq(), &full_readings(200_000, 45_000, 2 * STEP), 270).unwrap();
        assert_eq!(st.switches().switches(), 2);
        assert_eq!(st.switches().per_source()[SRC_VRM as usize], 1);
        assert!(st.health().unwrap().ok);
    }

    // ---- 判据五组：开销合并账 ----

    #[test]
    fn overhead_budget_and_savings_ledger() {
        let mut st = JointSampStation::new(11).unwrap();
        // 名义联合开销 270µs：预算内，节省 379‰（对 435µs 分立口径）。
        let rep = st
            .feed(st.next_seq(), &full_readings(200_000, 45_000, 0), 270)
            .unwrap();
        assert!(!rep.overhead_budget_hit);
        // 超预算拍（400µs > 300µs）：逐拍立案。
        let rep2 = st
            .feed(st.next_seq(), &full_readings(200_000, 45_000, STEP), 400)
            .unwrap();
        assert!(rep2.overhead_budget_hit);
        assert_eq!(st.overhead().budget_exceeded(), 1);
        assert_eq!(st.ledger().count_of(JointSampCode::OverheadBudgetExceeded), 1);
        // 均值（270+400)/2 = 335µs；峰值 400。
        assert_eq!(st.overhead().avg_us(), 335);
        assert_eq!(st.overhead().max_us(), 400);
        // 归档文档：分立口径与名义开销全程可追溯。
        let doc = st.overhead().overhead_doc();
        assert!(doc.contains("435"));
        assert!(doc.contains("270"));
        assert!(doc.contains("预算 300"));
        assert!(doc.contains("均值 335"));
        // const 判据自证：联合名义开销 ≤ 预算 < 分立总开销。
        assert_eq!(joint_nominal_us(), 270);
        assert_eq!(discrete_total_us(), 435);
        assert!(st.health().unwrap().ok);
    }

    // ---- 判据六组：缺失降级 ----

    #[test]
    fn missing_degradation_grace_transient_absorbed() {
        let mut st = JointSampStation::new(13).unwrap();
        st.feed(st.next_seq(), &full_readings(200_000, 45_000, 0), 270).unwrap();
        // 连缺 2 拍：仍在 T16=3 宽限内，不降级（瞬时总线重试不降档）。
        st.feed(st.next_seq(), &pf_readings(200_000, STEP), 270).unwrap();
        st.feed(st.next_seq(), &pf_readings(200_000, 2 * STEP), 270).unwrap();
        assert_eq!(st.degrade_state(), DegradeState::DualChannel);
        // 第 3 拍恢复全通道：缺失清零，永不降级。
        let rep = st
            .feed(st.next_seq(), &full_readings(200_000, 45_000, 3 * STEP), 270)
            .unwrap();
        assert!(!rep.degraded);
        assert_eq!(st.degrade_state(), DegradeState::DualChannel);
        assert_eq!(st.ledger().count_of(JointSampCode::DegradedSingleChannel), 0);
        assert_eq!(st.ledger().count_of(JointSampCode::ChannelMissing), 2);
    }

    #[test]
    fn missing_degradation_transition_and_recovery() {
        let mut st = JointSampStation::new(13).unwrap();
        st.feed(st.next_seq(), &full_readings(200_000, 45_000, 0), 270).unwrap();
        // 连缺 3 拍：超宽限 → 单通道模式（缺失值不补齐）。
        let mut i = 1u32;
        while i <= MISSING_GRACE_BEATS {
            let rep = st
                .feed(st.next_seq(), &pf_readings(200_000, i as u64 * STEP), 270)
                .unwrap();
            assert!(rep.degraded);
            i += 1;
        }
        assert_eq!(
            st.degrade_state(),
            DegradeState::SingleChannel { missing: Channel::Temp.mask_bit() }
        );
        assert_eq!(st.degrade.missing_counts()[Channel::Temp.index()], 3);
        assert_eq!(st.degrade.degraded_beats(), 3);
        assert_eq!(st.ledger().count_of(JointSampCode::DegradedSingleChannel), 1);
        // 恢复：连续 T17=8 拍全通道在位 → 自动回双通道。
        let mut j = 0u32;
        while j < RECOVERY_CONFIRM_BEATS {
            let rep = st
                .feed(
                    st.next_seq(),
                    &full_readings(200_000, 45_000, (4 + j) as u64 * STEP),
                    270,
                )
                .unwrap();
            if j + 1 < RECOVERY_CONFIRM_BEATS {
                assert_eq!(rep.degrade_after, DegradeState::SingleChannel { missing: Channel::Temp.mask_bit() });
            } else {
                assert_eq!(rep.degrade_after, DegradeState::DualChannel);
            }
            j += 1;
        }
        assert_eq!(st.degrade_state(), DegradeState::DualChannel);
        assert_eq!(st.ledger().count_of(JointSampCode::RecoveredDualChannel), 1);
        assert!(st.health().unwrap().ok);
    }

    #[test]
    fn degrade_missing_note_propagates() {
        let s = build_joint_sample(0, &pf_readings(200_000, 0)).unwrap();
        assert!(s.missing_note().contains("缺失[T]"));
        assert!(s.missing_note().contains("单通道模式"));
        assert_eq!(mask_label(FULL_MASK), "P+T+F");
        assert_eq!(mask_missing(FULL_MASK), 0);
    }

    // ---- 事件账与指纹 ----

    #[test]
    fn ledger_cap_half_drop_and_health() {
        let mut led = EventLedger::new();
        let mut i = 0u64;
        while i < LEDGER_CAP as u64 + 16 {
            led.push(JointSampCode::BeatAligned, i, i, "n");
            i += 1;
        }
        // 半量让位：在账 = CAP/2 + 16，淘汰 = CAP/2，seq 连续守恒。
        assert_eq!(led.len(), LEDGER_CAP / 2 + 16);
        assert_eq!(led.dropped(), LEDGER_CAP as u64 / 2);
        assert!(led.health().is_ok());
        // 断链注入：健康检查必须立案。
        let mut bad = EventLedger::new();
        bad.push(JointSampCode::BeatAligned, 0, 0, "a");
        bad.push(JointSampCode::BeatAligned, 1, 0, "b");
        bad.entries[1].seq = 9; // 人为断链
        assert_eq!(bad.health(), Err(JointSampError::LedgerCorrupted));
    }

    #[test]
    fn diagnostic_code_closure_and_triplets() {
        let all = [
            JointSampCode::BeatAligned,
            JointSampCode::BeatSkewed,
            JointSampCode::ChannelMissing,
            JointSampCode::DegradedSingleChannel,
            JointSampCode::RecoveredDualChannel,
            JointSampCode::CrossRuleMatched,
            JointSampCode::CrossRuleUnmatched,
            JointSampCode::SensorSourceSwitched,
            JointSampCode::SensorFallbackUsed,
            JointSampCode::ProfileBucketUpdated,
            JointSampCode::CurveMonotonicViolated,
            JointSampCode::OverheadBudgetOk,
            JointSampCode::OverheadBudgetExceeded,
            JointSampCode::FakeFillRejected,
            JointSampCode::FingerprintMismatch,
            JointSampCode::LedgerCorrupted,
        ];
        let mut i = 0usize;
        while i < all.len() {
            let v = all[i].as_u16();
            assert_eq!(v, 0x1C20 + i as u16);
            assert!(!all[i].what().is_empty());
            assert!(!all[i].why().is_empty());
            assert!(!all[i].advice().is_empty());
            i += 1;
        }
    }

    #[test]
    fn error_details_actionable_chinese() {
        let all = [
            JointSampError::EmptyBeat,
            JointSampError::FakeFillSentinel { channel: 0 },
            JointSampError::SkewOverflow { span_us: 1 },
            JointSampError::ZeroInterval { beat: 0 },
            JointSampError::SeqGap { expected: 0, got: 1 },
            JointSampError::OverheadOverflow,
            JointSampError::LedgerCorrupted,
            JointSampError::DeterminismBroken,
            JointSampError::InjectionCaseEmpty,
        ];
        let mut i = 0usize;
        while i < all.len() {
            assert!(all[i].detail().len() > 10);
            let c = all[i].code().as_u16();
            assert!(c >= 0x1C20 && c <= 0x1C2F);
            i += 1;
        }
    }

    // ---- 指纹与双跑确定性对拍 ----

    #[test]
    fn fingerprint_stability_and_determinism_pair() {
        let mut script: Vec<BeatScript> = Vec::new();
        let mut i = 0u32;
        while i < 12 {
            script.push(BeatScript::full(
                120_000 + i * 8_000,
                42_000 + i * 20,
                i as u64 * STEP,
            ));
            i += 1;
        }
        let st = JointSampStation::new(42).unwrap();
        let (fa, fb) = st.determinism_pair(&script).unwrap();
        assert_eq!(fa, fb, "同种子双跑指纹必须一致");
        // 同种子同脚本：对拍站指纹与第三站一致。
        let mut c = JointSampStation::new(42).unwrap();
        let mut k = 0usize;
        while k < script.len() {
            let s = &script[k];
            c.feed(c.next_seq(), &s.readings, s.overhead_us).unwrap();
            k += 1;
        }
        assert_eq!(c.fingerprint(), fa);
        // 不同种子：指纹必然分档（seed 混入）。
        let mut d = JointSampStation::new(43).unwrap();
        let mut k = 0usize;
        while k < script.len() {
            let s = &script[k];
            d.feed(d.next_seq(), &s.readings, s.overhead_us).unwrap();
            k += 1;
        }
        assert_ne!(d.fingerprint(), fa);
    }

    // ---- 注入台面端到端 ----

    #[test]
    fn inject_rig_end_to_end_gate() {
        let mut station = JointSampStation::new(99).unwrap();
        let stats = InjectRig::run_all(&mut station).unwrap();
        assert!(stats.skewed_detected, "超偏差注入必须检出");
        assert!(stats.rule_violation_detected, "交叉规则违例注入必须检出");
        assert!(stats.missing_detected, "通道缺失注入必须检出");
        assert!(stats.recovery_ok, "恢复路径必须走通");
        assert_eq!(stats.clean_fp_events, 0);
        assert_eq!(stats.fp_ppm, 0);
        assert!(stats.gate_ok);
        // 台面跑完账面必须自洽。
        assert!(station.health().unwrap().ok);
        // 注入留痕齐备：偏差/降级/规则/恢复四类事件各在账。
        assert!(station.ledger().count_of(JointSampCode::BeatSkewed) >= 1);
        assert!(station.ledger().count_of(JointSampCode::DegradedSingleChannel) >= 1);
        assert!(station.ledger().count_of(JointSampCode::RecoveredDualChannel) >= 1);
        assert!(station.ledger().count_of(JointSampCode::CrossRuleMatched) >= 1);
    }

    // ---- 常量文档导出 ----

    #[test]
    fn constants_doc_lists_thresholds() {
        let doc = constants_doc();
        assert!(doc.contains("T01 align_tolerance_us = 1000"));
        assert!(doc.contains("T03 joint_budget_us = 300"));
        assert!(doc.contains("T13 pwr_steep_slope_mw_per_ms = 15000"));
        assert!(doc.contains("T15 temp_steep_slope_mdegc_per_ms = 12"));
        assert!(doc.contains("T16 missing_grace_beats = 3"));
        assert!(doc.contains("T17 recovery_confirm_beats = 8"));
        assert!(doc.contains("T21 fake_fill_sentinel = 0xFFFFFFFF"));
        assert!(doc.contains("discrete_total_us = 435 / joint_nominal_us = 270"));
    }

    #[test]
    fn const_fn_helpers_and_bucket_math() {
        // 桶换算边界：0 落首桶、量程上限落末桶、超量程饱和不丢点。
        assert_eq!(bucket_of(0, P_RANGE_MW, BUCKET_P), 0);
        assert_eq!(bucket_of(500_000, P_RANGE_MW, BUCKET_P), 15);
        assert_eq!(bucket_of(600_000, P_RANGE_MW, BUCKET_P), 15);
        assert_eq!(bucket_of(31_249, P_RANGE_MW, BUCKET_P), 0);
        assert_eq!(bucket_of(31_250, P_RANGE_MW, BUCKET_P), 1);
        assert_eq!(temp_bucket_center(0), 4_687);
        assert_eq!(temp_bucket_center(15), 145_312);
    }
}
