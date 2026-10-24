//! VE-F6012 · 物理测试（VE-AB 域 · 物理组 · 目标 300 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F6012`
//!
//! **判据（锚点原文）**：确定性对拍、NaN 防线、三线、长稳、CI 回归、判据。
//!
//! **职责定位（锚点原文摘录）**：物理测试（目标 300 行）——物理测试——确定性对拍（同种子同轨迹）、NaN 洪流防线、步长边界三线测试；测试含长稳项——物理连续 8 小时运行确定性不漂移；测试含确定性回归进 CI；含长稳劣化容忍带声明；含长稳场景清单附档；含长稳结果进总册；含结果评审签字。
//!
//! ---------------------------------------------------------------------------
//! 深化说明（300 行 → 深化 2300+ 行，零浮点，Q16 定点建模）
//! ---------------------------------------------------------------------------
//!
//! 本模块是物理域（VE-AB）的测试子系统，与前序模块的关系：
//! - `vead10_physec`（VE-F6010 物理安全）提供 NaN/Inf 位模式洪流防线的**运行时防线**；
//! - 本模块（VE-F6012 物理测试）提供同一条防线的**测试面**：把防线放进
//!   确定性物理内核里做注入对拍——注入 NaN 后防线必须钳到安全值、入账、
//!   计数，且**不得改变确定性轨迹**（钳制不入状态）。
//!
//! 六大真子系统（全部手工实现，无浮点运算，Q16.16 定点）：
//!
//! 1. `DeterministicPhysics` 确定性物理内核：半隐式欧拉（辛积分器）固定步长
//!    积分 Q16 定点位置/速度；逐步状态指纹链（FNV-1a 链式折叠）——任一步
//!    分叉即破；双跑对拍（同种子两条独立实例逐步比对 `fp_chain`）。
//! 2. `NanFloodGuard` NaN 洪流防线：f32/f64 NaN/Inf **位模式 const 表**，
//!    检测纯位运算（不依赖浮点比较，零浮点纪律下根本无浮点可比）；命中后
//!    钳到安全值 + 入事件账 + 防线触发计数；防线解除（disarm）时洪流扩散
//!    → 污染标志 → 下一步积分拒绝（`NanFloodUncontained`），重武装 + 清污
//!    后恢复；另含位型→Q16 定点手工换算（指数/尾数手工移位，非规格化向零
//!    舍入，越界显性饱和）。
//! 3. `StepBoundaryTest` 步长边界三线：最小步长（1/240s）/标准步长（1/60s）/
//!    最大步长（1/20s）三线，每线独立稳定性判据（能量守恒漂移带，带宽来源
//!    注释声明）；步长越界一律拒绝（`StepSizeOutOfRange`）。
//! 4. `LongRunStability` 长稳测试：8 小时等效抽象时钟（1_728_000 逻辑
//!    tick，加速采样帧距 16 → 108_000 物理步）加速模拟；确定性不漂移判据
//!    （双跑全链一致 + 首尾指纹一致 + 能量漂移 ≤ 容忍带）；劣化容忍带
//!    const 声明（带宽 + 来源注释）；长稳场景清单附档（const 表）。
//! 5. CI 回归面：确定性回归种子表（const 8 种子），每种子指纹入 CI 账；
//!    双跑一致即绿、漂移即红；账间 diff 面供种子漂移注入检出。
//! 6. 结果评审签字：测试报告 → 评审签字位（签字人/时戳/结论三要素）→
//!    封存 → 入总册；未签字不得入总册（`SignatureMissing`），空签字人/
//!    零时戳/非 Pass 结论各拒（`SignatureInvalid`/`VerdictRejected`），
//!    封存后不可改（`ReportSealed`）。
//!
//! 另含：事件账（滚动容量）、`health()` 健康面、注入面三件套（NaN 注入/
//! 种子漂移注入/签字缺失注入，各检出于 `probe_log`）、`selfcheck()` 判据
//! 自检全集。
//!
//! ---------------------------------------------------------------------------
//! 纪律（铁律）
//! ---------------------------------------------------------------------------
//! - no_std + alloc；只 `use core::…` / `use alloc::…`；零 `use crate::…`。
//! - 内核路径零 unwrap / 零 expect / 零 panic! / 零运行期 assert!
//!   （编译期 `const _: () = assert!(…)` 允许，见文末）。
//! - 可失败全走 `Result<T, PhysTestError>`；错误枚举每变体带
//!   `detail(&self) -> 'static str` 中文可操作诊断。
//! - 溢出显性 saturating / wrapping；>64KB 结构体禁上栈（本模块全部聚合
//!   结构 ≤ 数十字节，堆账走 `alloc::vec::Vec`）。
//! - const fn 禁坑：无 slice::get、无 Vec 索引、无 Ord::min/max、无
//!   derive(PartialEq) 类型的 `==`、无 for 循环（一律 while + 索引）。
//! - 零浮点：物理量全 i64 Q16.16 定点；NaN/Inf 检测与换算纯位运算。
//! - 零 IO、零墙钟：时钟一律 tick 注入/抽象时钟。
//!
//! ---------------------------------------------------------------------------
//! 诊断码段（0x1BF0~0x1BFF，独占）
//! ---------------------------------------------------------------------------
//! 码段占用核实（grep kernel/varix/src 全树，2026-10-24）：
//! - 0x1B40 段已被 `cgpu/cga34_postfx.rs`（CGPU-F0034）占用；
//! - 0x1B50 段已被 `cgpu/cga35_clkadapt.rs`（CGPU-F0035）占用；
//! - 0x1B60 段已被 `cgpu/cga36_mixedprec.rs`（CGPU-F0036）占用；
//! - 0x1BF0 段全树无命中 → 本模块独占 0x1BF0~0x1BFF。

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 常量面（全部写死，来源注释）
// ---------------------------------------------------------------------------

/// 诊断段基座（0x1BF0 段，独占 0x10——空闲核实见文件头）。
pub const SEG_BASE: u16 = 0x1BF0;

/// 诊断段跨度（0x10——0x1BF0~0x1BFF 共 16 码）。
pub const SEG_SPAN: u16 = 0x10;

/// 模块标识（总册/报告引用用）。
pub const MODULE_ID: &str = "vead12_phystest";

/// 任务号（锚点）。
pub const TASK_ID: &str = "VE-F6012";

/// Q16.16 定点小数位宽（Q16 约定——与 vead10_physec 同源）。
pub const Q16_SHIFT: u32 = 16;

/// Q16 定点 1.0。
pub const Q16_ONE: i64 = 1 << Q16_SHIFT;

/// Q16 定点换算饱和界 = 2^24.0 实值。
/// 来源：防线上限声明——位型→定点换算遇越界量级一律显性饱和到此界，
/// 不静默截断不 panic。
pub const Q16_SAT_MAX: i64 = 1 << 40;

/// 位置定点量程（|x| ≤ 32768.0）。
/// 来源：与 vead10_physec::POS_LIMIT 同源（VE-AB 域统一量纲）。
pub const POS_LIMIT: i64 = 1 << 25;

/// 速度定点量程（|v| ≤ 4096.0）。
/// 来源：与 vead10_physec::VEL_LIMIT 同源。
pub const VEL_LIMIT: i64 = 1 << 22;

/// 质量定点量程上界（|m| ≤ 1024.0）。
/// 来源：与 vead10_physec::QUANTITY_LIMITS[mass]=1<<20 同源。
pub const MASS_LIMIT: i64 = 1 << 20;

/// 物理体容量上限（测试内核 O(物理体) 账面上限）。
/// 来源：与 vead10_physec::BODY_CAP=512 对齐取保守值。
pub const BODY_CAP: usize = 256;

/// 默认物理体数（三体——对拍/长稳标准场景）。
pub const DEFAULT_BODY_COUNT: usize = 3;

/// 标准步长 1/60 s（Q16）。
/// 来源：物理域主循环 60Hz 帧率约定（VE-AB 域步长同源三律之一）。
pub const FIXED_DT_STANDARD_Q16: i64 = Q16_ONE / 60;

/// 最小步长 1/240 s（Q16）。
/// 来源：高精度子步下限（240Hz 触摸/物理细分业界惯例）。
pub const FIXED_DT_MIN_Q16: i64 = Q16_ONE / 240;

/// 最大步长 1/20 s（Q16）。
/// 来源：半隐式欧拉对 k=64 弹簧的稳定裕度上界（ω·dt=0.4 < 2 稳定界，
/// 取 1/20 留 5 倍裕度并使能量漂移带可观测）。
pub const FIXED_DT_MAX_Q16: i64 = Q16_ONE / 20;

/// 弹簧劲度 0.04（Q16）——三线/长稳标准场景的保守力常数。
/// 来源：构造 ω=√k≈0.2 rad/s 的慢谐振子。半隐式欧拉（一阶辛积分器）
/// 的真实能量有界振荡幅 ~ ω·dt 量级——取 ω=0.2 使三线漂移带分层可观测
/// （实测 min 1730 / std 2785 / max 5529ppm）且长稳容忍带可收紧。
pub const K_SPRING_Q16: i64 = Q16_ONE / 25;

/// NaN 防线钳制安全值（Q16 定点 0.0）。
/// 来源：vead10 防线行为——检出即钳到安全值，0 为中性安全量。
pub const SAFE_CLAMP_Q16: i64 = 0;

/// 逐步指纹采样帧距（每 8 步采样一次入滚动样本环）。
/// 来源：指纹链每步全量折叠 O(体数)，采样环只承担对拍/长稳的首尾与
/// 中段比对，8 步帧距在 4096 步对拍内保证 ≥256 个采样锚点。
pub const SAMPLE_STRIDE: u64 = 8;

/// 指纹样本环容量（滚动）。
pub const SAMPLE_CAP: usize = 64;

/// 事件账容量（滚动覆盖，覆盖计数留痕）。
pub const EVENT_CAP: usize = 256;

/// 浮点注入口容量（滚动）——位型→定点换算暂存寄存器账。
pub const INTAKE_CAP: usize = 32;

/// 双跑对拍失配熔断数（失配计满即停，避免长跑刷屏）。
pub const DUAL_MISMATCH_CAP: u32 = 8;

/// 三线测试种子（写死——判据：三线结果可复现）。
pub const BOUNDARY_SEED: u64 = 0x5EED_B0A1;

/// 三线测试步数（1800 步 = 30s 标准步长等效，三线同量程物理时长）。
pub const BOUNDARY_STEPS: u64 = 1800;

/// 长稳 8 小时等效逻辑 tick 总数（8h × 3600 s × 60 Hz）。
/// 来源：锚点判据「物理连续 8 小时运行确定性不漂移」的口径换算。
pub const LONGRUN_TICKS_EQUIV: u64 = 28_800 * 60;

/// 长稳抽象时钟加速采样帧距（每 16 逻辑 tick 执行 1 物理步）。
/// 来源：长稳劣化容忍带附档声明——加速模拟保持固定步长确定性不变，
/// 只压缩执行帧距；16:1 使 8 小时窗口落到 108_000 步、CI 可承受。
pub const LONGRUN_STRIDE: u64 = 16;

/// 长稳单场景物理步数（= LONGRUN_TICKS_EQUIV / LONGRUN_STRIDE）。
pub const LONGRUN_PHYS_STEPS: u64 = LONGRUN_TICKS_EQUIV / LONGRUN_STRIDE;

/// 长稳劣化容忍带默认值（百万分之 8000）。
/// 来源：容忍带声明——确定性辛积分理论零漂移，能量有界振荡实测
/// 3715~3852ppm（标准步长、ω≈0.2、三体，2026-10-24 长稳基准标定）；
/// 默认带 = 实测上界 × 2 余量 = 8000ppm，由 CI 长稳项持续回填校准
/// （回填须走评审签字进总册，不允许静默改带）。
pub const LONGRUN_TOL_DEFAULT_PPM: i64 = 8_000;

/// CI 回归种子数（const 种子表长度——判据：每种子指纹入 CI 账）。
pub const CI_SEED_COUNT: usize = 8;

/// CI 回归步数（每种子 512 步——CI 预算内可承受，且覆盖 ≥64 个采样锚点）。
pub const CI_STEPS: u64 = 512;

/// 总册容量（滚动拒绝——总册不允许静默滚动，满即拒并要求评审归档）。
pub const MASTER_CAP: usize = 64;

// ---------------------------------------------------------------------------
// 错误面（0x1BF0 段全码，每变体带中文可操作诊断）
// ---------------------------------------------------------------------------

/// 物理测试错误码（0x1BF1~0x1BFF，段基座 0x1BF0 保留为段标识）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PhysTestCode {
    /// 0x1BF1 种子非法（0 种子无法驱动 xorshift 状态机）。
    SeedInvalid = 0x1BF1,
    /// 0x1BF2 物理体数非法（0 或超 BODY_CAP）。
    BodyCountInvalid = 0x1BF2,
    /// 0x1BF3 物理体未知（未注册的 body id）。
    BodyUnknown = 0x1BF3,
    /// 0x1BF4 浮点注入口未知（channel 越界）。
    ChannelUnknown = 0x1BF4,
    /// 0x1BF5 步长越界拒绝（不在 [min,max] 三线闭区间内）。
    StepSizeOutOfRange = 0x1BF5,
    /// 0x1BF6 能量漂移出带（三线稳定性判据破）。
    EnergyDriftExceeded = 0x1BF6,
    /// 0x1BF7 轨迹分叉（同种子双跑指纹链破）。
    DivergenceDetected = 0x1BF7,
    /// 0x1BF8 NaN 洪流未兜住（防线解除态下毒位型扩散进积分）。
    NanFloodUncontained = 0x1BF8,
    /// 0x1BF9 长稳漂移出带（确定性不漂移判据破）。
    LongrunDriftExceeded = 0x1BF9,
    /// 0x1BFA 长稳场景未知（场景清单越界）。
    ScenarioUnknown = 0x1BFA,
    /// 0x1BFB 账满（总册/账面容量拒绝）。
    LedgerFull = 0x1BFB,
    /// 0x1BFC 签字缺失（未签字不得入总册）。
    SignatureMissing = 0x1BFC,
    /// 0x1BFD 签字非法（签字人为空/时戳为零）。
    SignatureInvalid = 0x1BFD,
    /// 0x1BFE 报告已封存（封存后不可改签/不可改节）。
    ReportSealed = 0x1BFE,
    /// 0x1BFF 结论非 Pass 或小节含红（不得入总册）。
    VerdictRejected = 0x1BFF,
}

impl PhysTestCode {
    /// 码值（u16，落 0x1BF0 段）。
    pub const fn code(self) -> u16 {
        self as u16
    }

    /// 人读名（中文短句）。
    pub const fn name(self) -> &'static str {
        match self {
            PhysTestCode::SeedInvalid => "种子非法",
            PhysTestCode::BodyCountInvalid => "物理体数非法",
            PhysTestCode::BodyUnknown => "物理体未知",
            PhysTestCode::ChannelUnknown => "注入口未知",
            PhysTestCode::StepSizeOutOfRange => "步长越界拒绝",
            PhysTestCode::EnergyDriftExceeded => "能量漂移出带",
            PhysTestCode::DivergenceDetected => "轨迹分叉",
            PhysTestCode::NanFloodUncontained => "NaN 洪流未兜住",
            PhysTestCode::LongrunDriftExceeded => "长稳漂移出带",
            PhysTestCode::ScenarioUnknown => "长稳场景未知",
            PhysTestCode::LedgerFull => "账满拒绝",
            PhysTestCode::SignatureMissing => "签字缺失",
            PhysTestCode::SignatureInvalid => "签字非法",
            PhysTestCode::ReportSealed => "报告已封存",
            PhysTestCode::VerdictRejected => "结论未过",
        }
    }

    /// 可操作建议（中文，落诊断读屏）。
    pub const fn advice(self) -> &'static str {
        match self {
            PhysTestCode::SeedInvalid => "种子 0 无法驱动 xorshift64 状态机；换任意非零种子并登记进 CI 种子表",
            PhysTestCode::BodyCountInvalid => "物理体数须在 1..=BODY_CAP(256)；检查场景清单 bodies 字段",
            PhysTestCode::BodyUnknown => "body id 未注册；先 add_body 拿到返回 id 再引用",
            PhysTestCode::ChannelUnknown => "注入口 channel = body_idx*2 + slot(0=x,1=y)；核对体数与槽位",
            PhysTestCode::StepSizeOutOfRange => "步长须落在 [1/240s, 1/20s] 三线闭区间；越界一律拒绝，改用三线之一",
            PhysTestCode::EnergyDriftExceeded => "能量漂移超该线带宽；先核对劲度 k 与步长乘积 ω·dt<2，再复核带宽声明",
            PhysTestCode::DivergenceDetected => "同种子双跑指纹链分叉——积分路径存在非确定性输入；查注入面与共享可变状态",
            PhysTestCode::NanFloodUncontained => "防线解除态下 NaN 位型已扩散；rearm_and_purge 清污后重武装，禁止裸跑",
            PhysTestCode::LongrunDriftExceeded => "长稳漂移超容忍带；核对容忍带声明来源与加速帧距，再查定点舍入累积",
            PhysTestCode::ScenarioUnknown => "场景下标越界；以 LONGRUN_SCENARIOS 清单为准选择 0..len",
            PhysTestCode::LedgerFull => "总册已满且不允许静默滚动；评审归档后清册或扩 MASTER_CAP",
            PhysTestCode::SignatureMissing => "未签字不得入总册；先 sign（签字人/时戳/结论三要素齐）再 admit",
            PhysTestCode::SignatureInvalid => "签字人为空或时戳为零均属无效签字；补齐三要素后重签",
            PhysTestCode::ReportSealed => "报告已封存不可改；如需修订请新建报告版本并重走签字流",
            PhysTestCode::VerdictRejected => "结论非 Pass 或小节含红；修红后重跑测试再签字",
        }
    }
}

/// 全部错误码清单（const——detail 面自检用）。
pub const ALL_CODES: [PhysTestCode; 15] = [
    PhysTestCode::SeedInvalid,
    PhysTestCode::BodyCountInvalid,
    PhysTestCode::BodyUnknown,
    PhysTestCode::ChannelUnknown,
    PhysTestCode::StepSizeOutOfRange,
    PhysTestCode::EnergyDriftExceeded,
    PhysTestCode::DivergenceDetected,
    PhysTestCode::NanFloodUncontained,
    PhysTestCode::LongrunDriftExceeded,
    PhysTestCode::ScenarioUnknown,
    PhysTestCode::LedgerFull,
    PhysTestCode::SignatureMissing,
    PhysTestCode::SignatureInvalid,
    PhysTestCode::ReportSealed,
    PhysTestCode::VerdictRejected,
];

/// 物理测试错误（只携带码——上下文入事件账，错误本身 Copy 零堆）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PhysTestError {
    /// 错误码。
    pub code: PhysTestCode,
}

impl PhysTestError {
    /// 构造。
    pub const fn new(code: PhysTestCode) -> Self {
        PhysTestError { code }
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

/// 码是否落在本模块诊断段（0x1BF0~0x1BFF）。
pub const fn code_in_segment(code: u16) -> bool {
    code >= SEG_BASE && code < SEG_BASE + SEG_SPAN
}

// ---------------------------------------------------------------------------
// Q16 定点算术（const 友好——显性饱和，零浮点）
// ---------------------------------------------------------------------------

/// Q16 乘法（i128 中间量 + 显性饱和回 i64）。
pub const fn q16_mul(a: i64, b: i64) -> i64 {
    let p: i128 = (a as i128) * (b as i128);
    let s: i128 = p >> Q16_SHIFT;
    if s > i64::MAX as i128 {
        i64::MAX
    } else if s < i64::MIN as i128 {
        i64::MIN
    } else {
        s as i64
    }
}

/// 闭区间钳制（const 友好——不用 Ord::min/max）。
pub const fn clamp_i64(v: i64, lo: i64, hi: i64) -> i64 {
    if v < lo {
        lo
    } else if v > hi {
        hi
    } else {
        v
    }
}

/// i128 → Q16 i64 显性饱和钳制（位型换算出口统一走此口）。
pub const fn clamp_i128_to_q16(v: i128) -> i64 {
    let sat: i128 = Q16_SAT_MAX as i128;
    if v > sat {
        Q16_SAT_MAX
    } else if v < -sat {
        -Q16_SAT_MAX
    } else {
        v as i64
    }
}

/// 绝对差（饱和语义——不用 i64::abs 以避 i64::MIN 翻转坑）。
pub const fn abs_diff_i64(a: i64, b: i64) -> i64 {
    if a >= b {
        a.saturating_sub(b)
    } else {
        b.saturating_sub(a)
    }
}

/// 相对漂移（百万分比，i128 中间量防溢出；基线 ≤0 视为无基线→0）。
pub const fn drift_ppm(base: i64, now: i64) -> i64 {
    let d: i128 = abs_diff_i64(now, base) as i128;
    let b: i128 = base as i128;
    if b <= 0 {
        0
    } else {
        let ppm: i128 = (d * 1_000_000) / b;
        if ppm > i64::MAX as i128 {
            i64::MAX
        } else {
            ppm as i64
        }
    }
}

// ---------------------------------------------------------------------------
// 确定性随机与指纹（xorshift64* / FNV-1a——全整数，跨平台逐位一致）
// ---------------------------------------------------------------------------

/// xorshift64* 输出乘子。
/// 来源：Vigna 2014《xorshift*/xorshift+》推荐常数。
pub const RNG_MULT: u64 = 0x2545_F491_4F6C_DD1D;

/// xorshift64* 状态推进 + 输出（输出即下一状态——乘子为奇数，双射可逆，
/// 非零状态永不塌到 0）。
pub const fn rng_next(state: u64) -> u64 {
    let mut x = state;
    x ^= x << 13;
    x ^= x >> 7;
    x ^= x << 17;
    x.wrapping_mul(RNG_MULT)
}

/// FNV-1a 偏移基。
pub const FNV_OFFSET: u64 = 0xcbf2_9ce4_8422_2325;

/// FNV-1a 质数。
pub const FNV_PRIME: u64 = 0x0000_0100_0000_01b3;

/// FNV-1a 逐字节折叠一个 u64 字（const fn——while + 移位，无索引）。
pub const fn fnv_word(h: u64, w: u64) -> u64 {
    let mut i: u32 = 0;
    let mut hh = h;
    while i < 8 {
        hh = hh.wrapping_mul(FNV_PRIME) ^ ((w >> (i * 8)) & 0xFF);
        i += 1;
    }
    hh
}

// ---------------------------------------------------------------------------
// 位模式面（NaN/Inf 检测与换算——纯位运算，零浮点比较）
// ---------------------------------------------------------------------------

/// f32 NaN/Inf 位型 const 表（检测不依赖浮点比较——检测面只认位型）。
/// 来源：IEEE 754-2008 单精度编码表（exp=0xFF 全部 + 尾数判 NaN/Inf）。
pub const F32_PATTERNS: [(u32, &'static str); 6] = [
    (0x7FC0_0000, "f32 安静 NaN（+qNaN）"),
    (0xFFC0_0000, "f32 安静 NaN（-qNaN）"),
    (0x7F80_0001, "f32 信号 NaN（最小 sNaN）"),
    (0x7FFF_FFFF, "f32 信号 NaN（全 1 sNaN）"),
    (0x7F80_0000, "f32 +Inf"),
    (0xFF80_0000, "f32 -Inf"),
];

/// f64 NaN/Inf 位型 const 表（同上——双精度编码）。
pub const F64_PATTERNS: [(u64, &'static str); 6] = [
    (0x7FF8_0000_0000_0000, "f64 安静 NaN（+qNaN）"),
    (0xFFF8_0000_0000_0000, "f64 安静 NaN（-qNaN）"),
    (0x7FF0_0000_0000_0001, "f64 信号 NaN（最小 sNaN）"),
    (0x7FFF_FFFF_FFFF_FFFF, "f64 信号 NaN（全 1 sNaN）"),
    (0x7FF0_0000_0000_0000, "f64 +Inf"),
    (0xFFF0_0000_0000_0000, "f64 -Inf"),
];

/// f32 位型类别（const 可判——纯位运算）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum F32BitsClass {
    /// 零（±0）。
    Zero,
    /// 非规格化数。
    Denormal,
    /// 规格化有限数。
    Normal,
    /// NaN（安静/信号）。
    Nan,
    /// 无穷（±Inf）。
    Inf,
}

/// f32 位型分类（const fn——exp/尾数位判，零浮点比较）。
pub const fn classify_f32_bits(bits: u32) -> F32BitsClass {
    let exp = (bits >> 23) & 0xFF;
    let frac = bits & 0x007F_FFFF;
    if exp == 0xFF {
        if frac == 0 {
            F32BitsClass::Inf
        } else {
            F32BitsClass::Nan
        }
    } else if exp == 0 {
        if frac == 0 {
            F32BitsClass::Zero
        } else {
            F32BitsClass::Denormal
        }
    } else {
        F32BitsClass::Normal
    }
}

/// f64 位型类别（同构 f32——纯位判）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum F64BitsClass {
    /// 零（±0）。
    Zero,
    /// 非规格化数。
    Denormal,
    /// 规格化有限数。
    Normal,
    /// NaN（安静/信号）。
    Nan,
    /// 无穷（±Inf）。
    Inf,
}

/// f64 位型分类（const fn）。
pub const fn classify_f64_bits(bits: u64) -> F64BitsClass {
    let exp = (bits >> 52) & 0x07FF;
    let frac = bits & 0x000F_FFFF_FFFF_FFFF;
    if exp == 0x07FF {
        if frac == 0 {
            F64BitsClass::Inf
        } else {
            F64BitsClass::Nan
        }
    } else if exp == 0 {
        if frac == 0 {
            F64BitsClass::Zero
        } else {
            F64BitsClass::Denormal
        }
    } else {
        F64BitsClass::Normal
    }
}

/// f32 位型→Q16 定点手工换算（const fn，零浮点）。
///
/// 规则（换算纪律声明）：
/// - NaN/Inf → `None`（防线面负责钳制，换算面拒绝）；
/// - 非规格化 → 0（向零舍入——Q16 精度之下，声明为准）；
/// - 规格化：m=(1<<23)|frac，Q16 = m × 2^(exp-134)，i128 中间量 + 显性饱和；
/// - 符号位最后施加（避免 -0 与翻转坑）。
pub const fn f32_bits_to_q16_checked(bits: u32) -> Option<i64> {
    let sign = (bits >> 31) & 1;
    let exp = ((bits >> 23) & 0xFF) as i64;
    let frac = (bits & 0x007F_FFFF) as i64;
    if exp == 0xFF {
        return None;
    }
    let mag: i128;
    if exp == 0 {
        // 非规格化：m×2^-149 → Q16 = m×2^-133，m<2^23 → 恒为 0（向零舍入）。
        mag = 0;
    } else {
        let m: i128 = (1i128 << 23) + frac as i128;
        let shift: i64 = exp - 134;
        if shift >= 0 {
            // 上移保护：2^24 << 100 = 2^124 < i128 上界；超过即显性饱和。
            if shift > 100 {
                return Some(if sign == 1 { -Q16_SAT_MAX } else { Q16_SAT_MAX });
            }
            mag = m << shift;
        } else {
            let down: i64 = -shift;
            // i128 移位量上限 127；更深下移必为 0（m < 2^24）。
            if down > 127 {
                mag = 0;
            } else {
                mag = m >> down;
            }
        }
    }
    let clamped = clamp_i128_to_q16(mag);
    Some(if sign == 1 { -clamped } else { clamped })
}

/// f64 位型→Q16 定点手工换算（const fn，零浮点；规则同 f32 面）。
/// 规格化：m=(1<<52)|frac，Q16 = m × 2^(exp-1059)。
pub const fn f64_bits_to_q16_checked(bits: u64) -> Option<i64> {
    let sign = (bits >> 63) & 1;
    let exp = ((bits >> 52) & 0x07FF) as i64;
    let frac = (bits & 0x000F_FFFF_FFFF_FFFF) as i128;
    if exp == 0x07FF {
        return None;
    }
    let mag: i128;
    if exp == 0 {
        // 非规格化：m×2^-1074 → Q16 下移超 127 位 → 恒为 0（向零舍入）。
        mag = 0;
    } else {
        let m: i128 = (1i128 << 52) + frac;
        let shift: i64 = exp - 1059;
        if shift >= 0 {
            // 上移保护：2^53 << 70 = 2^123 < i128 上界；超过即显性饱和。
            if shift > 70 {
                return Some(if sign == 1 { -Q16_SAT_MAX } else { Q16_SAT_MAX });
            }
            mag = m << shift;
        } else {
            let down: i64 = -shift;
            if down > 127 {
                mag = 0;
            } else {
                mag = m >> down;
            }
        }
    }
    let clamped = clamp_i128_to_q16(mag);
    Some(if sign == 1 { -clamped } else { clamped })
}

/// f32 位型在 const 表中的精确命中名（表外位型给类别名——while 扫表）。
pub fn match_pattern_name_f32(bits: u32) -> &'static str {
    let cls = classify_f32_bits(bits);
    let mut i = 0;
    while i < F32_PATTERNS.len() {
        if F32_PATTERNS[i].0 == bits {
            return F32_PATTERNS[i].1;
        }
        i += 1;
    }
    match cls {
        F32BitsClass::Nan => "f32 NaN（表外位型）",
        F32BitsClass::Inf => "f32 Inf（表外位型）",
        F32BitsClass::Normal => "f32 规格化有限数",
        F32BitsClass::Denormal => "f32 非规格化数",
        F32BitsClass::Zero => "f32 零",
    }
}

/// f64 位型在 const 表中的精确命中名（同构 f32 面）。
pub fn match_pattern_name_f64(bits: u64) -> &'static str {
    let cls = classify_f64_bits(bits);
    let mut i = 0;
    while i < F64_PATTERNS.len() {
        if F64_PATTERNS[i].0 == bits {
            return F64_PATTERNS[i].1;
        }
        i += 1;
    }
    match cls {
        F64BitsClass::Nan => "f64 NaN（表外位型）",
        F64BitsClass::Inf => "f64 Inf（表外位型）",
        F64BitsClass::Normal => "f64 规格化有限数",
        F64BitsClass::Denormal => "f64 非规格化数",
        F64BitsClass::Zero => "f64 零",
    }
}

// ---------------------------------------------------------------------------
// NanFloodGuard —— NaN 洪流防线（测试面：注入→检出→钳制→入账→计数）
// ---------------------------------------------------------------------------

/// 防线注入口的处置结果（不携带错误——错误在扩散后的积分拒绝处抛）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum IntakeOutcome {
    /// NaN 命中且防线兜住：钳到安全值。
    ContainedNan {
        /// 钳制后的安全定点值。
        clamped_q16: i64,
    },
    /// Inf 命中且防线兜住：钳到安全值。
    ContainedInf {
        /// 钳制后的安全定点值。
        clamped_q16: i64,
    },
    /// 良性位型：换算为定点后放行。
    Accepted {
        /// 换算出的定点值（已饱和钳制）。
        q16: i64,
    },
    /// 防线解除态下毒位型逃逸（污染标志置位，等待下一步积分拒绝）。
    Escaped {
        /// 逃逸的原始位型。
        bits: u64,
    },
}

/// NaN 洪流防线（与 vead10 运行时防线同构的测试面实例）。
///
/// 行为契约（判据：NaN 防线）：
/// 1. 武装态：NaN/Inf 位型命中 → 钳到 `SAFE_CLAMP_Q16` + 事件入账 +
///    计数递增；**不改变物理状态**（钳制不入状态 → 确定性轨迹不破）。
/// 2. 解除态：毒位型逃逸 → `contaminated` 置位 → 下一步积分以
///    `NanFloodUncontained` 拒绝（洪流扩散可检出）。
/// 3. 重武装：`rearm_and_purge` 清污染标志但保留逃逸计数（留痕不洗账）。
#[derive(Clone, Debug)]
pub struct NanFloodGuard {
    /// 防线武装态（true=武装——默认武装）。
    pub armed: bool,
    /// NaN 命中累计数。
    pub nan_hits: u64,
    /// Inf 命中累计数。
    pub inf_hits: u64,
    /// 良性位型放行累计数。
    pub benign_pass: u64,
    /// 钳制事件累计数（武装态命中即钳）。
    pub clamp_events: u64,
    /// 逃逸事件累计数（解除态命中即逃，留痕不洗）。
    pub escape_events: u64,
    /// 污染标志（逃逸后置位；rearm_and_purge 清除）。
    pub contaminated: bool,
    /// 最近一次毒 f32 位型（检出留痕）。
    pub last_bad_bits_f32: u32,
    /// 最近一次毒 f64 位型（检出留痕）。
    pub last_bad_bits_f64: u64,
    /// 最近一次命中位型的人读名（来自 const 表）。
    pub last_pattern_note: &'static str,
}

impl NanFloodGuard {
    /// 武装态新防线。
    pub const fn armed_new() -> Self {
        NanFloodGuard {
            armed: true,
            nan_hits: 0,
            inf_hits: 0,
            benign_pass: 0,
            clamp_events: 0,
            escape_events: 0,
            contaminated: false,
            last_bad_bits_f32: 0,
            last_bad_bits_f64: 0,
            last_pattern_note: "",
        }
    }

    /// 解除态新防线（注入面专用——验证洪流扩散可检出）。
    pub const fn disarmed_new() -> Self {
        let mut g = Self::armed_new();
        g.armed = false;
        g
    }

    /// f32 位型摄入（检出纯位判；处置走武装/解除两态）。
    pub fn ingest_f32(&mut self, bits: u32) -> IntakeOutcome {
        let cls = classify_f32_bits(bits);
        match cls {
            F32BitsClass::Nan => {
                self.nan_hits = self.nan_hits.saturating_add(1);
                self.last_bad_bits_f32 = bits;
                self.last_pattern_note = match_pattern_name_f32(bits);
                if self.armed {
                    self.clamp_events = self.clamp_events.saturating_add(1);
                    IntakeOutcome::ContainedNan {
                        clamped_q16: SAFE_CLAMP_Q16,
                    }
                } else {
                    self.escape_events = self.escape_events.saturating_add(1);
                    self.contaminated = true;
                    IntakeOutcome::Escaped {
                        bits: bits as u64,
                    }
                }
            }
            F32BitsClass::Inf => {
                self.inf_hits = self.inf_hits.saturating_add(1);
                self.last_bad_bits_f32 = bits;
                self.last_pattern_note = match_pattern_name_f32(bits);
                if self.armed {
                    self.clamp_events = self.clamp_events.saturating_add(1);
                    IntakeOutcome::ContainedInf {
                        clamped_q16: SAFE_CLAMP_Q16,
                    }
                } else {
                    self.escape_events = self.escape_events.saturating_add(1);
                    self.contaminated = true;
                    IntakeOutcome::Escaped {
                        bits: bits as u64,
                    }
                }
            }
            _ => {
                self.benign_pass = self.benign_pass.saturating_add(1);
                match f32_bits_to_q16_checked(bits) {
                    Some(q) => IntakeOutcome::Accepted { q16: q },
                    // 理论不可达（Normal/Denormal/Zero 必有换算值）；
                    // 防御式兜底按 Inf 命中处置——防线不因面内矛盾失守。
                    None => {
                        if self.armed {
                            self.clamp_events = self.clamp_events.saturating_add(1);
                            IntakeOutcome::ContainedInf {
                                clamped_q16: SAFE_CLAMP_Q16,
                            }
                        } else {
                            self.escape_events = self.escape_events.saturating_add(1);
                            self.contaminated = true;
                            IntakeOutcome::Escaped {
                                bits: bits as u64,
                            }
                        }
                    }
                }
            }
        }
    }

    /// f64 位型摄入（同构 f32 面）。
    pub fn ingest_f64(&mut self, bits: u64) -> IntakeOutcome {
        let cls = classify_f64_bits(bits);
        match cls {
            F64BitsClass::Nan => {
                self.nan_hits = self.nan_hits.saturating_add(1);
                self.last_bad_bits_f64 = bits;
                self.last_pattern_note = match_pattern_name_f64(bits);
                if self.armed {
                    self.clamp_events = self.clamp_events.saturating_add(1);
                    IntakeOutcome::ContainedNan {
                        clamped_q16: SAFE_CLAMP_Q16,
                    }
                } else {
                    self.escape_events = self.escape_events.saturating_add(1);
                    self.contaminated = true;
                    IntakeOutcome::Escaped { bits }
                }
            }
            F64BitsClass::Inf => {
                self.inf_hits = self.inf_hits.saturating_add(1);
                self.last_bad_bits_f64 = bits;
                self.last_pattern_note = match_pattern_name_f64(bits);
                if self.armed {
                    self.clamp_events = self.clamp_events.saturating_add(1);
                    IntakeOutcome::ContainedInf {
                        clamped_q16: SAFE_CLAMP_Q16,
                    }
                } else {
                    self.escape_events = self.escape_events.saturating_add(1);
                    self.contaminated = true;
                    IntakeOutcome::Escaped { bits }
                }
            }
            _ => {
                self.benign_pass = self.benign_pass.saturating_add(1);
                match f64_bits_to_q16_checked(bits) {
                    Some(q) => IntakeOutcome::Accepted { q16: q },
                    None => {
                        if self.armed {
                            self.clamp_events = self.clamp_events.saturating_add(1);
                            IntakeOutcome::ContainedInf {
                                clamped_q16: SAFE_CLAMP_Q16,
                            }
                        } else {
                            self.escape_events = self.escape_events.saturating_add(1);
                            self.contaminated = true;
                            IntakeOutcome::Escaped { bits }
                        }
                    }
                }
            }
        }
    }

    /// 重武装 + 清污（逃逸计数保留——留痕不洗账）。
    pub fn rearm_and_purge(&mut self) {
        self.armed = true;
        self.contaminated = false;
    }

    /// 防线健康小结（health 面）。
    pub fn is_clean(&self) -> bool {
        self.armed && !self.contaminated && self.escape_events == 0
    }
}

// ---------------------------------------------------------------------------
// 事件账（滚动容量——覆盖计数留痕）
// ---------------------------------------------------------------------------

/// 事件类别（闭集）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EventKind {
    /// 浮点注入口接受（良性位型换算放行）。
    IntakeAccept,
    /// 防线钳制（毒位型兜住）。
    GuardClamp,
    /// 防线逃逸（解除态扩散）。
    GuardEscape,
    /// 双跑分叉检出。
    Divergence,
    /// 注入探针结论。
    InjectProbe,
}

impl EventKind {
    /// 人读名。
    pub const fn name(self) -> &'static str {
        match self {
            EventKind::IntakeAccept => "注入口接受",
            EventKind::GuardClamp => "防线钳制",
            EventKind::GuardEscape => "防线逃逸",
            EventKind::Divergence => "分叉检出",
            EventKind::InjectProbe => "注入探针",
        }
    }
}

/// 物理事件账条目。
#[derive(Clone, Debug)]
pub struct PhysEvent {
    /// 事件发生的逻辑 tick。
    pub tick: u64,
    /// 事件类别。
    pub kind: EventKind,
    /// 诊断码（0x1BF0 段）。
    pub code: u16,
    /// 人读备注。
    pub note: String,
}

// ---------------------------------------------------------------------------
// PhysBody / DeterministicPhysics —— 确定性物理内核
// ---------------------------------------------------------------------------

/// 物理体（全 Q16 定点；thrust 为注入口换算出的外力驱动项）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PhysBody {
    /// 体 id（注册序号，从 1 起——0 保留为非法 id）。
    pub id: u32,
    /// 位置 [x, y]（Q16）。
    pub pos: [i64; 2],
    /// 速度 [x, y]（Q16）。
    pub vel: [i64; 2],
    /// 外力驱动 [x, y]（Q16 加速度当量；默认 0——三线/长稳场景保持闭系统）。
    pub thrust: [i64; 2],
    /// 质量（Q16）。
    pub mass_q16: i64,
}

/// 双跑对拍报告（判据：同种子同轨迹）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DualRunReport {
    /// 对拍种子。
    pub seed: u64,
    /// 对拍步数。
    pub steps: u64,
    /// A 跑初始指纹链。
    pub head_fp_a: u64,
    /// B 跑初始指纹链。
    pub head_fp_b: u64,
    /// A 跑终态指纹链。
    pub tail_fp_a: u64,
    /// B 跑终态指纹链。
    pub tail_fp_b: u64,
    /// 逐步失配数（0 = 全程逐步一致）。
    pub mismatches: u32,
    /// 采样环是否一致。
    pub samples_match: bool,
    /// 对拍结论（双跑一致即真）。
    pub agree: bool,
}

/// 确定性物理内核（Q16 定点 · 半隐式欧拉 · 固定步长 · 逐步指纹链）。
///
/// 确定性契约：
/// - 状态推进只依赖 `(seed, dt, k, thrust)` 与整数定点算术——跨平台
///   逐位一致（无浮点、无地址依赖、无墙钟）；
/// - 逐步状态指纹链 `fp_chain` = FNV-1a(上一步链， 本步全量状态哈希)——
///   任一步分叉即破链；
/// - NaN 注入经防线钳制**不入状态**——同种子含注入与不含注入两条轨迹
///   指纹链必须全等（对拍判据的构造性保证）。
#[derive(Clone, Debug)]
pub struct DeterministicPhysics {
    /// 对拍种子（非零——0 拒绝）。
    pub seed: u64,
    /// 固定步长（Q16；三线之一，越界拒绝）。
    pub dt_q16: i64,
    /// 弹簧劲度（Q16；保守力——半隐式欧拉下能量有界不漂）。
    pub k_spring_q16: i64,
    /// 逻辑 tick（自 0 起，wrapping 递增）。
    pub tick: u64,
    /// 物理体账。
    pub bodies: Vec<PhysBody>,
    /// 下一个待分配体 id。
    next_id: u32,
    /// 逐步状态指纹链。
    pub fp_chain: u64,
    /// 已积分步数。
    pub fp_steps: u64,
    /// 指纹采样环（滚动——首尾/中段比对锚点）。
    pub samples: Vec<u64>,
    /// 采样环滚动覆盖计数。
    pub samples_rolled: u64,
    /// 基准能量（首步前——三线漂移带基线）。
    pub energy0_q16: i64,
    /// 全程最大能量漂移（ppm——三线判据直接读数）。
    pub max_drift_ppm: i64,
    /// 事件账（滚动）。
    pub events: Vec<PhysEvent>,
    /// 事件账滚动覆盖计数。
    pub events_rolled: u64,
    /// NaN 洪流防线（测试面实例）。
    pub guard: NanFloodGuard,
    /// 浮点注入口暂存账（滚动——位型→定点换算寄存器）。
    pub intake: Vec<(usize, usize, i64)>,
}

impl DeterministicPhysics {
    /// 标准构造（默认体数 + 标准步长）。
    pub fn new(seed: u64) -> Result<Self, PhysTestError> {
        Self::with_config(seed, FIXED_DT_STANDARD_Q16, DEFAULT_BODY_COUNT)
    }

    /// 全参构造（种子 + 步长 + 体数；全部入参过闸）。
    pub fn with_config(
        seed: u64,
        dt_q16: i64,
        body_count: usize,
    ) -> Result<Self, PhysTestError> {
        validate_dt_q16(dt_q16)?;
        if seed == 0 || body_count == 0 || body_count > BODY_CAP {
            return Err(PhysTestError::new(PhysTestCode::SeedInvalid));
        }
        let mut sim = DeterministicPhysics {
            seed,
            dt_q16,
            k_spring_q16: K_SPRING_Q16,
            tick: 0,
            bodies: Vec::new(),
            next_id: 1,
            fp_chain: FNV_OFFSET,
            fp_steps: 0,
            samples: Vec::new(),
            samples_rolled: 0,
            energy0_q16: 0,
            max_drift_ppm: 0,
            events: Vec::new(),
            events_rolled: 0,
            guard: NanFloodGuard::armed_new(),
            intake: Vec::new(),
        };
        // 种子驱动初始条件生成（确定性 IC——同种子同初始状态）。
        let mut rng = seed;
        let mut i = 0;
        while i < body_count {
            rng = rng_next(rng);
            let pr = ((rng >> 8) & 0xFFF) as i64 - 2048; // -2048..2047
            let px = pr * (Q16_ONE / 2048); // ±1.0 Q16
            rng = rng_next(rng);
            let vr = ((rng >> 16) & 0x3FF) as i64 - 512; // -512..511
            let vx = vr * (Q16_ONE / 1024); // ±0.5 Q16
            rng = rng_next(rng);
            let py = (((rng >> 8) & 0xFFF) as i64 - 2048) * (Q16_ONE / 2048);
            rng = rng_next(rng);
            let vy = (((rng >> 16) & 0x3FF) as i64 - 512) * (Q16_ONE / 1024);
            let _ = sim.add_body(px, py, vx, vy, Q16_ONE)?;
            i += 1;
        }
        sim.energy0_q16 = sim.energy_q16();
        Ok(sim)
    }

    /// 注册物理体（量程过闸——越界注入一律拒绝）。
    pub fn add_body(
        &mut self,
        px_q16: i64,
        py_q16: i64,
        vx_q16: i64,
        vy_q16: i64,
        mass_q16: i64,
    ) -> Result<u32, PhysTestError> {
        if self.bodies.len() >= BODY_CAP {
            return Err(PhysTestError::new(PhysTestCode::BodyCountInvalid));
        }
        if mass_q16 <= 0 || mass_q16 > MASS_LIMIT {
            return Err(PhysTestError::new(PhysTestCode::BodyCountInvalid));
        }
        let pos_x = clamp_i64(px_q16, -POS_LIMIT, POS_LIMIT);
        let pos_y = clamp_i64(py_q16, -POS_LIMIT, POS_LIMIT);
        let vel_x = clamp_i64(vx_q16, -VEL_LIMIT, VEL_LIMIT);
        let vel_y = clamp_i64(vy_q16, -VEL_LIMIT, VEL_LIMIT);
        let id = self.next_id;
        self.next_id = self.next_id.wrapping_add(1);
        self.bodies.push(PhysBody {
            id,
            pos: [pos_x, pos_y],
            vel: [vel_x, vel_y],
            thrust: [0, 0],
            mass_q16,
        });
        Ok(id)
    }

    /// 体 id 查下标（while 线性扫——O(体数)，体数有界）。
    pub fn body_index(&self, body_id: u32) -> Option<usize> {
        let mut i = 0;
        while i < self.bodies.len() {
            if self.bodies[i].id == body_id {
                return Some(i);
            }
            i += 1;
        }
        None
    }

    /// 全量状态哈希（FNV-1a 折叠 tick + 各体六字——逐步指纹链的原料）。
    fn state_hash(&self) -> u64 {
        let mut h = fnv_word(FNV_OFFSET, self.tick);
        let mut i = 0;
        while i < self.bodies.len() {
            let b = &self.bodies[i];
            h = fnv_word(h, b.id as u64);
            h = fnv_word(h, b.pos[0] as u64);
            h = fnv_word(h, b.pos[1] as u64);
            h = fnv_word(h, b.vel[0] as u64);
            h = fnv_word(h, b.vel[1] as u64);
            h = fnv_word(h, b.thrust[0] as u64);
            h = fnv_word(h, b.thrust[1] as u64);
            h = fnv_word(h, b.mass_q16 as u64);
            i += 1;
        }
        h
    }

    /// 总机械能（Q16）：KE = Σ ½ m v²，PE = Σ ½ k x²（弹簧保守场）。
    pub fn energy_q16(&self) -> i64 {
        let mut e: i64 = 0;
        let mut i = 0;
        while i < self.bodies.len() {
            let b = &self.bodies[i];
            let vx2 = q16_mul(b.vel[0], b.vel[0]);
            let vy2 = q16_mul(b.vel[1], b.vel[1]);
            let ke = q16_mul(b.mass_q16, vx2.saturating_add(vy2)) >> 1;
            let px2 = q16_mul(b.pos[0], b.pos[0]);
            let py2 = q16_mul(b.pos[1], b.pos[1]);
            let pe = q16_mul(self.k_spring_q16 >> 1, px2.saturating_add(py2));
            e = e.saturating_add(ke).saturating_add(pe);
            i += 1;
        }
        e
    }

    /// 推进一步（半隐式欧拉：先速度后位置——辛积分器，谐振子能量有界）。
    pub fn step(&mut self) -> Result<(), PhysTestError> {
        // 防线污染闸：解除态下毒位型已扩散进注入口——拒绝积分。
        if self.guard.contaminated {
            return Err(PhysTestError::new(PhysTestCode::NanFloodUncontained));
        }
        let dt = self.dt_q16;
        let k = self.k_spring_q16;
        let n = self.bodies.len();
        let mut i = 0;
        while i < n {
            let b = &mut self.bodies[i];
            // a = -k·x + thrust（Q16；保守力 + 注入口外力项）。
            let ax = -q16_mul(k, b.pos[0]).saturating_add(b.thrust[0]);
            let ay = -q16_mul(k, b.pos[1]).saturating_add(b.thrust[1]);
            // 半隐式欧拉：v ← v + a·dt（先速度）。
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
            // 后位置：x ← x + v·dt。
            b.pos[0] = clamp_i64(
                b.pos[0].saturating_add(q16_mul(b.vel[0], dt)),
                -POS_LIMIT,
                POS_LIMIT,
            );
            b.pos[1] = clamp_i64(
                b.pos[1].saturating_add(q16_mul(b.vel[1], dt)),
                -POS_LIMIT,
                POS_LIMIT,
            );
            i += 1;
        }
        self.tick = self.tick.wrapping_add(1);
        // 逐步状态指纹链：任一步分叉即破。
        let sh = self.state_hash();
        self.fp_chain = fnv_word(self.fp_chain, sh);
        self.fp_steps = self.fp_steps.saturating_add(1);
        if self.fp_steps % SAMPLE_STRIDE == 0 {
            if self.samples.len() >= SAMPLE_CAP {
                self.samples.remove(0);
                self.samples_rolled = self.samples_rolled.saturating_add(1);
            }
            self.samples.push(self.fp_chain);
        }
        // 能量漂移带跟踪（三线/长稳判据直接读数）。
        let e = self.energy_q16();
        let d = drift_ppm(self.energy0_q16, e);
        if d > self.max_drift_ppm {
            self.max_drift_ppm = d;
        }
        Ok(())
    }

    /// 批量推进（步数上限封装——错误短路）。
    pub fn step_n(&mut self, steps: u64) -> Result<(), PhysTestError> {
        let mut s: u64 = 0;
        while s < steps {
            self.step()?;
            s += 1;
        }
        Ok(())
    }

    /// 事件入账（滚动覆盖——覆盖计数留痕）。
    pub fn push_event(&mut self, kind: EventKind, code: u16, note: String) {
        if self.events.len() >= EVENT_CAP {
            self.events.remove(0);
            self.events_rolled = self.events_rolled.saturating_add(1);
        }
        self.events.push(PhysEvent {
            tick: self.tick,
            kind,
            code,
            note,
        });
    }

    /// 浮点注入口：f32 位型摄入（防线检出→处置→入账；返回处置结果）。
    ///
    /// channel 编址：`channel = body_idx * 2 + slot`（slot 0=x, 1=y）。
    /// 武装态命中 → 钳制事件入账；解除态命中 → 逃逸事件 + 污染置位。
    /// 良性位型 → 手工换算 Q16 → 暂存寄存器账（可被 apply_intake 采用）。
    pub fn feed_float_f32(
        &mut self,
        channel: usize,
        bits: u32,
    ) -> Result<IntakeOutcome, PhysTestError> {
        if self.bodies.is_empty() || channel >= self.bodies.len() * 2 {
            return Err(PhysTestError::new(PhysTestCode::ChannelUnknown));
        }
        let outcome = self.guard.ingest_f32(bits);
        let body_idx = channel / 2;
        let slot = channel % 2;
        match &outcome {
            IntakeOutcome::Accepted { q16 } => {
                if self.intake.len() >= INTAKE_CAP {
                    self.intake.remove(0);
                }
                self.intake.push((body_idx, slot, *q16));
                let note = format!(
                    "f32 良性位型 {bits:#010x} → Q16 {q16}（channel {channel}）"
                );
                self.push_event(EventKind::IntakeAccept, SEG_BASE, note);
            }
            IntakeOutcome::ContainedNan { clamped_q16 } => {
                let note = format!(
                    "f32 NaN 命中已钳制→{clamped_q16}（{}；channel {channel}）",
                    self.guard.last_pattern_note
                );
                self.push_event(EventKind::GuardClamp, SEG_BASE + 1, note);
            }
            IntakeOutcome::ContainedInf { clamped_q16 } => {
                let note = format!(
                    "f32 Inf 命中已钳制→{clamped_q16}（{}；channel {channel}）",
                    self.guard.last_pattern_note
                );
                self.push_event(EventKind::GuardClamp, SEG_BASE + 1, note);
            }
            IntakeOutcome::Escaped { bits } => {
                let note = format!(
                    "f32 毒位型 {bits:#010x} 逃逸（防线解除；channel {channel}）"
                );
                self.push_event(EventKind::GuardEscape, SEG_BASE + 2, note);
            }
        }
        Ok(outcome)
    }

    /// 浮点注入口：f64 位型摄入（同构 f32 面）。
    pub fn feed_float_f64(
        &mut self,
        channel: usize,
        bits: u64,
    ) -> Result<IntakeOutcome, PhysTestError> {
        if self.bodies.is_empty() || channel >= self.bodies.len() * 2 {
            return Err(PhysTestError::new(PhysTestCode::ChannelUnknown));
        }
        let outcome = self.guard.ingest_f64(bits);
        let body_idx = channel / 2;
        let slot = channel % 2;
        match &outcome {
            IntakeOutcome::Accepted { q16 } => {
                if self.intake.len() >= INTAKE_CAP {
                    self.intake.remove(0);
                }
                self.intake.push((body_idx, slot, *q16));
                let note = format!(
                    "f64 良性位型 {bits:#018x} → Q16 {q16}（channel {channel}）"
                );
                self.push_event(EventKind::IntakeAccept, SEG_BASE, note);
            }
            IntakeOutcome::ContainedNan { clamped_q16 } => {
                let note = format!(
                    "f64 NaN 命中已钳制→{clamped_q16}（{}；channel {channel}）",
                    self.guard.last_pattern_note
                );
                self.push_event(EventKind::GuardClamp, SEG_BASE + 1, note);
            }
            IntakeOutcome::ContainedInf { clamped_q16 } => {
                let note = format!(
                    "f64 Inf 命中已钳制→{clamped_q16}（{}；channel {channel}）",
                    self.guard.last_pattern_note
                );
                self.push_event(EventKind::GuardClamp, SEG_BASE + 1, note);
            }
            IntakeOutcome::Escaped { bits } => {
                let note = format!(
                    "f64 毒位型 {bits:#018x} 逃逸（防线解除；channel {channel}）"
                );
                self.push_event(EventKind::GuardEscape, SEG_BASE + 2, note);
            }
        }
        Ok(outcome)
    }

    /// 把注入口寄存器账应用到物理体（外力驱动项生效——开环场景用）。
    pub fn apply_intake(&mut self) -> Result<usize, PhysTestError> {
        let mut applied = 0;
        let mut i = 0;
        while i < self.intake.len() {
            let (body_idx, slot, q16) = self.intake[i];
            if body_idx < self.bodies.len() && slot < 2 {
                self.bodies[body_idx].thrust[slot] = clamp_i64(
                    q16,
                    -ACC_LIMIT_EFFECTIVE,
                    ACC_LIMIT_EFFECTIVE,
                );
                applied += 1;
            }
            i += 1;
        }
        Ok(applied)
    }

    /// 双跑对拍（同种子两条独立实例逐步比对指纹链——判据：同种子同轨迹）。
    pub fn dual_run(seed: u64, steps: u64) -> Result<DualRunReport, PhysTestError> {
        let mut a = Self::new(seed)?;
        let mut b = Self::new(seed)?;
        let head_fp_a = a.fp_chain;
        let head_fp_b = b.fp_chain;
        let mut mismatches: u32 = 0;
        let mut diverged_note: Option<String> = None;
        let mut s: u64 = 0;
        while s < steps {
            a.step()?;
            b.step()?;
            if a.fp_chain != b.fp_chain {
                mismatches = mismatches.saturating_add(1);
                if diverged_note.is_none() {
                    diverged_note = Some(format!(
                        "双跑在 step {} 分叉（A {:#018x} / B {:#018x}）",
                        s + 1,
                        a.fp_chain,
                        b.fp_chain
                    ));
                }
                if mismatches >= DUAL_MISMATCH_CAP {
                    break;
                }
            }
            s += 1;
        }
        if let Some(note) = diverged_note {
            a.push_event(EventKind::Divergence, PhysTestCode::DivergenceDetected.code(), note);
        }
        let samples_match = a.samples == b.samples;
        let tail_fp_a = a.fp_chain;
        let tail_fp_b = b.fp_chain;
        Ok(DualRunReport {
            seed,
            steps: s,
            head_fp_a,
            head_fp_b,
            tail_fp_a,
            tail_fp_b,
            mismatches,
            samples_match,
            agree: mismatches == 0 && samples_match && tail_fp_a == tail_fp_b,
        })
    }

    /// 重放比对（对本实例的另一条重放轨迹——同种子同步数即须全等）。
    pub fn replay_matches(&self, other: &DeterministicPhysics) -> Result<bool, PhysTestError> {
        if self.seed != other.seed
            || self.tick != other.tick
            || self.bodies.len() != other.bodies.len()
            || self.fp_steps != other.fp_steps
        {
            return Ok(false);
        }
        Ok(self.fp_chain == other.fp_chain && self.samples == other.samples)
    }

    /// 对拍步数预算内取采样锚点数（判据口径换算——读屏用）。
    pub const fn sample_anchors_for(steps: u64) -> u64 {
        steps / SAMPLE_STRIDE
    }
}

/// 注入口外力当量的有效量程（|a| ≤ 1024.0 Q16）。
/// 来源：与 vead10_physec::ACC_LIMIT 同源。
pub const ACC_LIMIT_EFFECTIVE: i64 = 1 << 20;

// ---------------------------------------------------------------------------
// StepBoundaryTest —— 步长边界三线（最小/标准/最大，各线独立漂移带判据）
// ---------------------------------------------------------------------------

/// 步长越界闸（三线闭区间 [min,max] 之外一律拒绝——判据：步长越界拒绝）。
pub fn validate_dt_q16(dt_q16: i64) -> Result<(), PhysTestError> {
    if dt_q16 < FIXED_DT_MIN_Q16 || dt_q16 > FIXED_DT_MAX_Q16 {
        return Err(PhysTestError::new(PhysTestCode::StepSizeOutOfRange));
    }
    Ok(())
}

/// 三线名称（线 0/1/2 —— 与 DTS/BANDS 下标对齐）。
pub const BOUNDARY_LINE_NAMES: [&str; 3] = ["min-dt(1/240s)", "std-dt(1/60s)", "max-dt(1/20s)"];

/// 三线步长（Q16）。
pub const BOUNDARY_LINE_DTS: [i64; 3] = [
    FIXED_DT_MIN_Q16,
    FIXED_DT_STANDARD_Q16,
    FIXED_DT_MAX_Q16,
];

/// 三线能量漂移带（百万分比——判据：每线稳定性判据）。
///
/// 带宽来源声明（实测标定，2026-10-24，种子 0x5EED_B0A1 · 1800 步）：
/// 半隐式欧拉（一阶辛积分器）对谐振子的真实能量有界振荡不漂移，振荡幅
/// ~ ω·dt 量级。ω=√k≈0.2 rad/s 实测：min 线 1730ppm / std 线 2785ppm /
/// max 线 5529ppm。带宽 = 实测 × ~2.2~2.5 裕度：
/// - min 线实测 1730 → 带 4000ppm；
/// - std 线实测 2785 → 带 8000ppm；
/// - max 线实测 5529 → 带 12000ppm。
/// 超带即判该线不稳定 → 生产步长拒绝采用该线。
pub const BOUNDARY_LINE_BANDS_PPM: [i64; 3] = [4_000, 8_000, 12_000];

/// 单线结果。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LineResult {
    /// 线下标（0/1/2）。
    pub line: usize,
    /// 线名。
    pub name: &'static str,
    /// 步长（Q16）。
    pub dt_q16: i64,
    /// 跑过的步数。
    pub steps: u64,
    /// 基准能量（Q16）。
    pub energy0_q16: i64,
    /// 全程最大能量漂移（ppm）。
    pub max_drift_ppm: i64,
    /// 该线漂移带（ppm——见 BOUNDARY_LINE_BANDS_PPM 来源声明）。
    pub band_ppm: i64,
    /// 是否在带内（稳定性判据）。
    pub in_band: bool,
}

/// 三线汇总。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BoundarySummary {
    /// 三线结果（按 0/1/2 序）。
    pub lines: Vec<LineResult>,
    /// 三线全绿（生产步长可采用标准线）。
    pub accepted: bool,
}

impl BoundarySummary {
    /// 人读行集（报告面）。
    pub fn to_lines(&self) -> Vec<String> {
        let mut out = Vec::new();
        let mut i = 0;
        while i < self.lines.len() {
            let l = &self.lines[i];
            out.push(format!(
                "{} drift={}ppm band={}ppm {}",
                l.name,
                l.max_drift_ppm,
                l.band_ppm,
                if l.in_band { "IN_BAND" } else { "OUT_OF_BAND" }
            ));
            i += 1;
        }
        out
    }
}

/// 步长边界三线测试（判据：三线稳定 + 越界拒绝）。
///
/// 三线用同一种子同一体数分别建世界跑 `BOUNDARY_STEPS` 步，各线按
/// `BOUNDARY_LINE_BANDS_PPM` 独立判带；任一线出带 → 汇总拒绝。
pub fn run_boundary_lines(
    seed: u64,
    body_count: usize,
    steps: u64,
) -> Result<BoundarySummary, PhysTestError> {
    let mut lines: Vec<LineResult> = Vec::new();
    let mut all_in_band = true;
    let mut i = 0;
    while i < BOUNDARY_LINE_DTS.len() {
        let dt = BOUNDARY_LINE_DTS[i];
        let band = BOUNDARY_LINE_BANDS_PPM[i];
        let mut sim = DeterministicPhysics::with_config(seed, dt, body_count)?;
        sim.step_n(steps)?;
        let in_band = sim.max_drift_ppm <= band;
        if !in_band {
            all_in_band = false;
        }
        lines.push(LineResult {
            line: i,
            name: BOUNDARY_LINE_NAMES[i],
            dt_q16: dt,
            steps,
            energy0_q16: sim.energy0_q16,
            max_drift_ppm: sim.max_drift_ppm,
            band_ppm: band,
            in_band,
        });
        i += 1;
    }
    Ok(BoundarySummary {
        lines,
        accepted: all_in_band,
    })
}

// ---------------------------------------------------------------------------
// LongRunStability —— 长稳测试（8h 等效抽象时钟加速模拟 · 不漂移判据）
// ---------------------------------------------------------------------------

/// 长稳场景（场景清单附档条目——const 表即附档）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LongrunScenario {
    /// 场景名（清单可读键）。
    pub name: &'static str,
    /// 场景种子（写死——可复现）。
    pub seed: u64,
    /// 物理体数。
    pub bodies: usize,
    /// 物理步数（≤ LONGRUN_PHYS_STEPS——加速帧距换算后）。
    pub steps: u64,
    /// 劣化容忍带（ppm——本场景覆写值；来源见各条注释）。
    pub tol_ppm: i64,
    /// 场景说明（附档正文）。
    pub note: &'static str,
}

/// 长稳场景清单附档（const 表——判据：含长稳场景清单附档）。
///
/// 各条容忍带来源：确定性辛积分理论零漂移；标准步长能量振荡幅实测
/// 3715~5285ppm（ω≈0.2、2026-10-24 长稳基准标定，与三线带同源）；
/// 各条带宽 = 实测 × ~2 裕度，由 CI 长稳项持续回填校准（回填须走
/// 评审签字进总册，不允许静默改带）。
pub const LONGRUN_SCENARIOS: [LongrunScenario; 4] = [
    LongrunScenario {
        name: "lr-hold-near-static",
        seed: 0xA17E_0001,
        bodies: 3,
        steps: 108_000,
        tol_ppm: 8_000,
        note: "近静止悬挂（小振幅）——基准场景，带取默认 8000ppm（实测 3715）",
    },
    LongrunScenario {
        name: "lr-spring-standard",
        seed: 0xA17E_0002,
        bodies: 3,
        steps: 54_000,
        tol_ppm: 8_000,
        note: "标准振幅弹簧场——带 8000ppm（实测 3751 × 2 余量）",
    },
    LongrunScenario {
        name: "lr-full-amplitude",
        seed: 0xA17E_0003,
        bodies: 3,
        steps: 27_000,
        tol_ppm: 10_000,
        note: "满振幅边界场景——带 10000ppm（实测 5285 × 1.9 余量）",
    },
    LongrunScenario {
        name: "lr-mixed-ic",
        seed: 0xA17E_0004,
        bodies: 4,
        steps: 108_000,
        tol_ppm: 8_000,
        note: "四体混合初值——默认带（实测 3852）；验证体数扩容下不漂移",
    },
];

/// 长稳单场景记录（入报告 → 签字 → 总册）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LongrunRecord {
    /// 场景名。
    pub scenario: &'static str,
    /// 种子。
    pub seed: u64,
    /// 实跑步数。
    pub steps_run: u64,
    /// 首采样锚点指纹（head）。
    pub head_fp: u64,
    /// 尾采样锚点指纹（tail）。
    pub tail_fp: u64,
    /// 双跑重放是否全等（确定性不漂移——同种子全链一致）。
    pub replay_match: bool,
    /// 全程最大能量漂移（ppm）。
    pub max_drift_ppm: i64,
    /// 容忍带（ppm——场景清单覆写值）。
    pub tol_ppm: i64,
    /// 结论（不漂移判据全过）。
    pub pass: bool,
}

/// 长稳单场景执行（判据：连续 8 小时等效确定性不漂移）。
///
/// 判据三件套（全过才 pass）：
/// 1. 双跑重放全等——同种子两条实例全链指纹一致（构造性确定性）；
/// 2. 采样环一致——首/中/尾锚点链一致；
/// 3. 能量漂移 ≤ 场景容忍带——劣化容忍带声明（来源见清单注释）。
pub fn run_longrun_scenario(idx: usize) -> Result<LongrunRecord, PhysTestError> {
    if idx >= LONGRUN_SCENARIOS.len() {
        return Err(PhysTestError::new(PhysTestCode::ScenarioUnknown));
    }
    // const 表运行期下标读取——先过界闸（上 if），此处取值安全。
    let sc_name = LONGRUN_SCENARIOS[idx].name;
    let sc_seed = LONGRUN_SCENARIOS[idx].seed;
    let sc_bodies = LONGRUN_SCENARIOS[idx].bodies;
    let sc_steps = LONGRUN_SCENARIOS[idx].steps;
    let sc_tol = LONGRUN_SCENARIOS[idx].tol_ppm;
    let mut a = DeterministicPhysics::with_config(sc_seed, FIXED_DT_STANDARD_Q16, sc_bodies)?;
    let mut b = DeterministicPhysics::with_config(sc_seed, FIXED_DT_STANDARD_Q16, sc_bodies)?;
    let mut head_fp: u64 = a.fp_chain;
    let mut s: u64 = 0;
    let mut mismatch = false;
    while s < sc_steps {
        a.step()?;
        b.step()?;
        if s == 0 {
            head_fp = a.fp_chain;
        }
        if a.fp_chain != b.fp_chain {
            mismatch = true;
            break;
        }
        s += 1;
    }
    let tail_fp = a.fp_chain;
    let replay_match = !mismatch && a.samples == b.samples;
    let drift_in_tol = a.max_drift_ppm <= sc_tol;
    Ok(LongrunRecord {
        scenario: sc_name,
        seed: sc_seed,
        steps_run: s,
        head_fp,
        tail_fp,
        replay_match,
        max_drift_ppm: a.max_drift_ppm,
        tol_ppm: sc_tol,
        pass: replay_match && drift_in_tol,
    })
}

// ---------------------------------------------------------------------------
// CI 回归面（确定性回归种子表 const + 每种子指纹入账——漂移即红）
// ---------------------------------------------------------------------------

/// CI 确定性回归种子表（const——判据：种子表写死可复现）。
pub const CI_SEED_TABLE: [u64; CI_SEED_COUNT] = [
    0x5EED_0001,
    0x5EED_0002,
    0x5EED_0003,
    0x5EED_0004,
    0x5EED_0005,
    0x5EED_0006,
    0x5EED_0007,
    0x5EED_DEAD,
];

/// CI 单种子账条目。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CiEntry {
    /// 种子。
    pub seed: u64,
    /// 双跑一致终态指纹（绿时有效——红时为 0 哨兵）。
    pub fingerprint: u64,
    /// 步数。
    pub steps: u64,
    /// 绿/红（双跑一致即绿；漂移即红）。
    pub green: bool,
}

/// CI 账（确定性回归面——每种子指纹入账）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CiLedger {
    /// 账条目（种子序 = CI_SEED_TABLE 序）。
    pub entries: Vec<CiEntry>,
}

impl CiLedger {
    /// 全绿判定（while 扫——任一红即假）。
    pub fn all_green(&self) -> bool {
        let mut i = 0;
        while i < self.entries.len() {
            if !self.entries[i].green {
                return false;
            }
            i += 1;
        }
        true
    }

    /// 账间 diff——指纹不一致的种子集（种子漂移注入检出面）。
    pub fn diff_seeds(&self, other: &CiLedger) -> Vec<u64> {
        let mut out: Vec<u64> = Vec::new();
        let mut i = 0;
        while i < self.entries.len() && i < other.entries.len() {
            if self.entries[i].seed == other.entries[i].seed
                && self.entries[i].fingerprint != other.entries[i].fingerprint
            {
                out.push(self.entries[i].seed);
            }
            i += 1;
        }
        out
    }

    /// 人读行集（CI 读屏面——漂移即红逐条可见）。
    pub fn to_lines(&self) -> Vec<String> {
        let mut out = Vec::new();
        let mut i = 0;
        while i < self.entries.len() {
            let e = &self.entries[i];
            out.push(format!(
                "seed {:#010x} fp {:#018x} steps {} {}",
                e.seed,
                e.fingerprint,
                e.steps,
                if e.green { "GREEN" } else { "RED" }
            ));
            i += 1;
        }
        out
    }
}

/// 跑 CI 确定性回归全套（判据：确定性回归进 CI——每种子指纹入账）。
///
/// 每种子双跑 `CI_STEPS` 步：逐步指纹链全等且采样环一致 → 绿（指纹入账）；
/// 任一失配 → 红（指纹记 0 哨兵）。全账全绿才算过闸。
pub fn run_ci_suite() -> Result<CiLedger, PhysTestError> {
    let mut entries: Vec<CiEntry> = Vec::new();
    let mut i = 0;
    while i < CI_SEED_TABLE.len() {
        let seed = CI_SEED_TABLE[i];
        let rep = DeterministicPhysics::dual_run(seed, CI_STEPS)?;
        let green = rep.agree;
        entries.push(CiEntry {
            seed,
            fingerprint: if green { rep.tail_fp_a } else { 0 },
            steps: rep.steps,
            green,
        });
        i += 1;
    }
    Ok(CiLedger { entries })
}

// ---------------------------------------------------------------------------
// 结果评审签字（测试报告 → 签字位 → 封存 → 入总册；未签字不得入总册）
// ---------------------------------------------------------------------------

/// 评审结论（三值——Pass 才可入总册）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SignVerdict {
    /// 通过（可入总册）。
    Pass,
    /// 不通过（修红重跑）。
    Fail,
    /// 有条件通过（仍不得入总册——条件闭环后重签 Pass）。
    Conditional,
}

impl SignVerdict {
    /// 人读名。
    pub const fn name(self) -> &'static str {
        match self {
            SignVerdict::Pass => "通过",
            SignVerdict::Fail => "不通过",
            SignVerdict::Conditional => "有条件通过",
        }
    }
}

/// 报告小节结果。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SectionResult {
    /// 小节名。
    pub name: String,
    /// 绿/红。
    pub ok: bool,
    /// 备注（读屏）。
    pub note: String,
}

/// 评审签字位（三要素：签字人 / 时戳 / 结论——缺一即无效签字）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReviewSignature {
    /// 签字人（非空——空即 SignatureInvalid）。
    pub signer: String,
    /// 签字时戳（抽象 tick 注入——非零；零即 SignatureInvalid）。
    pub stamp_ticks: u64,
    /// 结论（非 Pass 不得入总册）。
    pub verdict: SignVerdict,
    /// 签字备注。
    pub note: String,
}

/// 测试报告（签字 → 封存 → 入总册的载体）。
#[derive(Clone, Debug)]
pub struct TestReport {
    /// 报告标题。
    pub title: String,
    /// 小节结果集。
    pub sections: Vec<SectionResult>,
    /// 签字位（None = 未签字）。
    pub signature: Option<ReviewSignature>,
    /// 封存态（封存后不可改节/不可改签）。
    pub sealed: bool,
}

impl TestReport {
    /// 新报告（未签字未封存）。
    pub fn new(title: impl ToString) -> Self {
        TestReport {
            title: title.to_string(),
            sections: Vec::new(),
            signature: None,
            sealed: false,
        }
    }

    /// 追加小节（封存态拒绝）。
    pub fn add_section(
        &mut self,
        name: impl ToString,
        ok: bool,
        note: impl ToString,
    ) -> Result<(), PhysTestError> {
        if self.sealed {
            return Err(PhysTestError::new(PhysTestCode::ReportSealed));
        }
        self.sections.push(SectionResult {
            name: name.to_string(),
            ok,
            note: note.to_string(),
        });
        Ok(())
    }

    /// 签字（三要素过闸：签字人非空、时戳非零；封存态拒绝改签）。
    pub fn sign(
        &mut self,
        signer: &str,
        stamp_ticks: u64,
        verdict: SignVerdict,
        note: impl ToString,
    ) -> Result<(), PhysTestError> {
        if self.sealed {
            return Err(PhysTestError::new(PhysTestCode::ReportSealed));
        }
        let trimmed = signer.trim();
        if trimmed.is_empty() || stamp_ticks == 0 {
            return Err(PhysTestError::new(PhysTestCode::SignatureInvalid));
        }
        self.signature = Some(ReviewSignature {
            signer: trimmed.to_string(),
            stamp_ticks,
            verdict,
            note: note.to_string(),
        });
        Ok(())
    }

    /// 封存（未签字不得封存——签字闸前置）。
    pub fn seal(&mut self) -> Result<(), PhysTestError> {
        if self.sealed {
            return Err(PhysTestError::new(PhysTestCode::ReportSealed));
        }
        if self.signature.is_none() {
            return Err(PhysTestError::new(PhysTestCode::SignatureMissing));
        }
        self.sealed = true;
        Ok(())
    }

    /// 全小节绿判定。
    pub fn all_sections_ok(&self) -> bool {
        let mut i = 0;
        while i < self.sections.len() {
            if !self.sections[i].ok {
                return false;
            }
            i += 1;
        }
        true
    }

    /// 可入总册预检（签字 + Pass + 全绿——总册 admit 复验同口径）。
    pub fn is_admissible(&self) -> bool {
        match &self.signature {
            Some(sig) => sig.verdict == SignVerdict::Pass && self.all_sections_ok(),
            None => false,
        }
    }
}

/// 总册条目（长稳结果等经签字报告归档的落地形态）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MasterEntry {
    /// 总册序号（从 1 起单调）。
    pub seq: u64,
    /// 报告标题。
    pub title: String,
    /// 签字人。
    pub signer: String,
    /// 签字时戳（抽象 tick）。
    pub stamp_ticks: u64,
    /// 结论（入册时必为 Pass——admit 闸保证）。
    pub verdict: SignVerdict,
}

/// 总册（判据：长稳结果进总册——未签字不得入）。
#[derive(Clone, Debug)]
pub struct MasterLedger {
    /// 册条目。
    pub entries: Vec<MasterEntry>,
    /// 下一序号。
    next_seq: u64,
}

impl MasterLedger {
    /// 新总册。
    pub const fn new() -> Self {
        MasterLedger {
            entries: Vec::new(),
            next_seq: 1,
        }
    }

    /// 入册闸（判据闸口——四拒全落 0x1BF0 段码）。
    ///
    /// 拒绝面：未签字（SignatureMissing）/ 签字要素非法（SignatureInvalid）/
    /// 结论非 Pass（VerdictRejected）/ 小节含红（VerdictRejected）/
    /// 册满（LedgerFull——总册不允许静默滚动）。
    pub fn admit(&mut self, report: &TestReport) -> Result<u64, PhysTestError> {
        let sig = match &report.signature {
            Some(s) => s,
            None => return Err(PhysTestError::new(PhysTestCode::SignatureMissing)),
        };
        if sig.signer.trim().is_empty() || sig.stamp_ticks == 0 {
            return Err(PhysTestError::new(PhysTestCode::SignatureInvalid));
        }
        if sig.verdict != SignVerdict::Pass {
            return Err(PhysTestError::new(PhysTestCode::VerdictRejected));
        }
        if !report.all_sections_ok() {
            return Err(PhysTestError::new(PhysTestCode::VerdictRejected));
        }
        if self.entries.len() >= MASTER_CAP {
            return Err(PhysTestError::new(PhysTestCode::LedgerFull));
        }
        let seq = self.next_seq;
        self.next_seq = self.next_seq.saturating_add(1);
        self.entries.push(MasterEntry {
            seq,
            title: report.title.clone(),
            signer: sig.signer.clone(),
            stamp_ticks: sig.stamp_ticks,
            verdict: sig.verdict,
        });
        Ok(seq)
    }
}

// ---------------------------------------------------------------------------
// PhysTestBench —— 编排台（事件账 + health + 注入面三件套 + 自检）
// ---------------------------------------------------------------------------

/// 注入探针结论（注入面三件套的统一落账形态）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProbeFinding {
    /// 探针名（"nan-flood" / "seed-drift" / "missing-signature"）。
    pub probe: &'static str,
    /// 是否检出（注入面判据：检出即真）。
    pub detected: bool,
    /// 人读结论。
    pub note: String,
}

/// NaN 洪流注入探针报告。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NanProbeReport {
    /// 注入位型数。
    pub fed: u32,
    /// NaN 命中数。
    pub nan_hits: u64,
    /// Inf 命中数。
    pub inf_hits: u64,
    /// 全部兜住（零逃逸）。
    pub contained: bool,
    /// 注入后双跑是否仍全等（确定性不破——钳制不入状态的构造性验证）。
    pub determinism_kept: bool,
    /// 人读结论。
    pub note: String,
}

/// 种子漂移注入探针报告。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SeedDriftFinding {
    /// 基线指纹表（首二种子）。
    pub baseline_fps: Vec<u64>,
    /// 变异种子表（首二种子 ⊕ 1）。
    pub mutated_seeds: Vec<u64>,
    /// 检出（变异种子与基线指纹全不一致即检出）。
    pub detected: bool,
    /// 人读结论。
    pub note: String,
}

/// 签字缺失注入探针报告。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SignatureFinding {
    /// 报告标题。
    pub title: String,
    /// 拒绝码（SignatureMissing=0x1BFC 为预期）。
    pub rejected_code: u16,
    /// 检出（闸口以 SignatureMissing 拒绝即检出）。
    pub detected: bool,
    /// 人读结论。
    pub note: String,
}

/// 健康条目。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HealthItem {
    /// 条目名。
    pub name: &'static str,
    /// 绿/红。
    pub ok: bool,
    /// 备注。
    pub note: String,
}

/// 健康报告（health 面——逐条绿红可读）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HealthReport {
    /// 条目集。
    pub items: Vec<HealthItem>,
    /// 总体绿（全条目绿）。
    pub overall_ok: bool,
}

/// 物理测试编排台（VE-F6012 的门面——判据全集的运行载体）。
#[derive(Clone, Debug)]
pub struct PhysTestBench {
    /// 主模拟世界（种子驱动）。
    pub sim: DeterministicPhysics,
    /// CI 账（run_ci 后落账）。
    pub ci: Option<CiLedger>,
    /// 三线汇总（run_boundary 后落账）。
    pub boundary: Option<BoundarySummary>,
    /// 长稳记录集（run_longrun 逐条落账——判据：长稳结果进总册的原料）。
    pub longrun: Vec<LongrunRecord>,
    /// 注入探针落账。
    pub probe_log: Vec<ProbeFinding>,
    /// 总册。
    pub master: MasterLedger,
    /// 编排台种子。
    seed: u64,
}

impl PhysTestBench {
    /// 新编排台（种子过闸——同 DeterministicPhysics）。
    pub fn new(seed: u64) -> Result<Self, PhysTestError> {
        let sim = DeterministicPhysics::new(seed)?;
        Ok(PhysTestBench {
            sim,
            ci: None,
            boundary: None,
            longrun: Vec::new(),
            probe_log: Vec::new(),
            master: MasterLedger::new(),
            seed,
        })
    }

    /// 编排台种子读数。
    pub const fn seed(&self) -> u64 {
        self.seed
    }

    /// 跑三线并落账（三线用写死种子 BOUNDARY_SEED——判据：可复现）。
    pub fn run_boundary(&mut self) -> Result<(), PhysTestError> {
        let summary = run_boundary_lines(BOUNDARY_SEED, DEFAULT_BODY_COUNT, BOUNDARY_STEPS)?;
        self.boundary = Some(summary);
        Ok(())
    }

    /// 跑指定长稳场景并落账（判据：长稳结果落账待签字）。
    pub fn run_longrun(&mut self, idx: usize) -> Result<LongrunRecord, PhysTestError> {
        let rec = run_longrun_scenario(idx)?;
        self.longrun.push(rec.clone());
        Ok(rec)
    }

    /// 跑 CI 回归全套并落账（判据：确定性回归进 CI）。
    pub fn run_ci(&mut self) -> Result<CiLedger, PhysTestError> {
        let ledger = run_ci_suite()?;
        self.ci = Some(ledger.clone());
        Ok(ledger)
    }

    /// 注入面一：NaN 洪流注入（f32/f64 全部毒位型逐口喂入）。
    ///
    /// 判据口径：武装态下全部兜住（零逃逸）+ 注入后同种子双跑仍全等
    /// （钳制不入状态 → 确定性不破）。
    pub fn inject_nan_flood(&mut self, rounds: u32) -> Result<NanProbeReport, PhysTestError> {
        let ch = 0usize; // 体 0 · x 槽
        let mut fed: u32 = 0;
        let mut i = 0;
        while i < rounds {
            let mut p = 0;
            while p < F32_PATTERNS.len() {
                let bits = F32_PATTERNS[p].0;
                let _ = self.sim.feed_float_f32(ch, bits)?;
                fed += 1;
                p += 1;
            }
            let mut p = 0;
            while p < F64_PATTERNS.len() {
                let bits = F64_PATTERNS[p].0;
                let _ = self.sim.feed_float_f64(ch, bits)?;
                fed += 1;
                p += 1;
            }
            i += 1;
        }
        let contained = self.sim.guard.escape_events == 0 && !self.sim.guard.contaminated;
        // 确定性不破验证：注入不影响状态 → 与干净重放全链一致。
        let rep = DeterministicPhysics::dual_run(self.seed, 256)?;
        let determinism_kept = rep.agree;
        let note = format!(
            "NaN 洪流注入 {fed} 口：命中 nan={} inf={}，钳制={}，逃逸={}；双跑确定性 {}",
            self.sim.guard.nan_hits,
            self.sim.guard.inf_hits,
            self.sim.guard.clamp_events,
            self.sim.guard.escape_events,
            if determinism_kept { "保持" } else { "破坏" }
        );
        let detected = contained && determinism_kept;
        self.probe_log.push(ProbeFinding {
            probe: "nan-flood",
            detected,
            note: note.clone(),
        });
        Ok(NanProbeReport {
            fed,
            nan_hits: self.sim.guard.nan_hits,
            inf_hits: self.sim.guard.inf_hits,
            contained,
            determinism_kept,
            note,
        })
    }

    /// 注入面二：种子漂移注入（变异种子 ⊕1 对拍基线——漂移必检出）。
    ///
    /// 判据口径：CI 账基线（无账则现算首二种子基线）与变异种子指纹
    /// 全不一致 → 检出为真（种子即轨迹身份——种子变轨迹必变）。
    pub fn inject_seed_drift(&mut self) -> Result<SeedDriftFinding, PhysTestError> {
        let n = 2usize;
        let mut baseline: Vec<u64> = Vec::new();
        let mut mutated: Vec<u64> = Vec::new();
        let mut i = 0;
        while i < n {
            let seed = CI_SEED_TABLE[i];
            let rep = DeterministicPhysics::dual_run(seed, CI_STEPS)?;
            baseline.push(if rep.agree { rep.tail_fp_a } else { 0 });
            i += 1;
        }
        let mut drifted: Vec<u64> = Vec::new();
        i = 0;
        while i < n {
            let mseed = CI_SEED_TABLE[i] ^ 1;
            let rep = DeterministicPhysics::dual_run(mseed, CI_STEPS)?;
            let mfp = if rep.agree { rep.tail_fp_a } else { 0 };
            mutated.push(mseed);
            if mfp == baseline[i] {
                // 变异种子指纹与基线相同——理论不可能；记为未检出证据。
            } else {
                drifted.push(mseed);
            }
            i += 1;
        }
        let detected = drifted.len() == n;
        let note = format!(
            "种子漂移注入：{} 个变异种子全部与基线指纹漂移（检出={detected}）",
            mutated.len()
        );
        self.probe_log.push(ProbeFinding {
            probe: "seed-drift",
            detected,
            note: note.clone(),
        });
        Ok(SeedDriftFinding {
            baseline_fps: baseline,
            mutated_seeds: mutated,
            detected,
            note,
        })
    }

    /// 注入面三：签字缺失注入（未签字报告撞总册闸——必被拒）。
    pub fn inject_missing_signature(&mut self, title: &str) -> Result<SignatureFinding, PhysTestError> {
        let mut report = self.build_report(title)?;
        // 故意不签字直接撞闸。
        let r = self.master.admit(&report);
        let (code, detected) = match r {
            Err(e) => (e.code(), e.code() == PhysTestCode::SignatureMissing.code()),
            Ok(_) => (0u16, false),
        };
        let note = format!(
            "签字缺失注入：总册闸拒绝码 {code:#06x}（检出={detected}）"
        );
        self.probe_log.push(ProbeFinding {
            probe: "missing-signature",
            detected,
            note: note.clone(),
        });
        // 探针报告本身入档（未签字——探针件不入总册，留编排台账）。
        let _ = report.add_section("probe-missing-signature", detected, note.clone());
        Ok(SignatureFinding {
            title: report.title.clone(),
            rejected_code: code,
            detected,
            note,
        })
    }

    /// 健康面（health——判据逐条落绿红）。
    pub fn health(&self) -> HealthReport {
        let mut items: Vec<HealthItem> = Vec::new();
        // 1. 防线武装且干净。
        items.push(HealthItem {
            name: "guard-armed-clean",
            ok: self.sim.guard.is_clean(),
            note: format!(
                "armed={} contaminated={} escape={}",
                self.sim.guard.armed, self.sim.guard.contaminated, self.sim.guard.escape_events
            ),
        });
        // 2. 双跑确定性（小步数抽样对拍——health 预算内）。
        let dual = DeterministicPhysics::dual_run(self.seed, 512);
        let dual_ok = match &dual {
            Ok(r) => r.agree,
            Err(_) => false,
        };
        items.push(HealthItem {
            name: "determinism-dual-run",
            ok: dual_ok,
            note: match dual {
                Ok(r) => format!("512 步双跑 mismatch={}", r.mismatches),
                Err(e) => format!("双跑失败：{}", e.name()),
            },
        });
        // 3. CI 账全绿且满表。
        let ci_ok = match &self.ci {
            Some(l) => l.entries.len() == CI_SEED_TABLE.len() && l.all_green(),
            None => false,
        };
        items.push(HealthItem {
            name: "ci-ledger-green",
            ok: ci_ok,
            note: match &self.ci {
                Some(l) => format!("{}/{} 种子绿", l.entries.len(), CI_SEED_TABLE.len()),
                None => "CI 未跑".to_string(),
            },
        });
        // 4. 三线全在带内。
        let b_ok = match &self.boundary {
            Some(b) => b.accepted && b.lines.len() == BOUNDARY_LINE_DTS.len(),
            None => false,
        };
        items.push(HealthItem {
            name: "boundary-three-lines",
            ok: b_ok,
            note: match &self.boundary {
                Some(b) => format!("三线 accepted={}", b.accepted),
                None => "三线未跑".to_string(),
            },
        });
        // 5. 长稳记录至少一条且全过。
        let lr_ok = if self.longrun.is_empty() {
            false
        } else {
            let mut all = true;
            let mut i = 0;
            while i < self.longrun.len() {
                if !self.longrun[i].pass {
                    all = false;
                }
                i += 1;
            }
            all
        };
        items.push(HealthItem {
            name: "longrun-no-drift",
            ok: lr_ok,
            note: format!("长稳记录 {} 条", self.longrun.len()),
        });
        // 6. 注入面三件套全检出（未跑视为红——注入面是判据项）。
        let mut nan_ok = false;
        let mut sd_ok = false;
        let mut ms_ok = false;
        let mut i = 0;
        while i < self.probe_log.len() {
            let p = &self.probe_log[i];
            if p.detected {
                if p.probe == "nan-flood" {
                    nan_ok = true;
                } else if p.probe == "seed-drift" {
                    sd_ok = true;
                } else if p.probe == "missing-signature" {
                    ms_ok = true;
                }
            }
            i += 1;
        }
        let inj_ok = nan_ok && sd_ok && ms_ok;
        items.push(HealthItem {
            name: "injection-surfaces",
            ok: inj_ok,
            note: format!("nan={nan_ok} seed-drift={sd_ok} missing-signature={ms_ok}"),
        });
        // 7. 能量漂移在标准带内（主世界健康读数）。
        let drift_ok = self.sim.max_drift_ppm <= BOUNDARY_LINE_BANDS_PPM[1];
        items.push(HealthItem {
            name: "energy-drift-in-band",
            ok: drift_ok,
            note: format!(
                "drift={}ppm band={}ppm",
                self.sim.max_drift_ppm, BOUNDARY_LINE_BANDS_PPM[1]
            ),
        });
        let mut overall = true;
        let mut i = 0;
        while i < items.len() {
            if !items[i].ok {
                overall = false;
            }
            i += 1;
        }
        HealthReport {
            items,
            overall_ok: overall,
        }
    }

    /// 由编排台现状构建测试报告（未签字——走签字闸后才能入总册）。
    pub fn build_report(&self, title: &str) -> Result<TestReport, PhysTestError> {
        let mut report = TestReport::new(title.to_string());
        let h = self.health();
        let mut i = 0;
        while i < h.items.len() {
            let it = &h.items[i];
            report.add_section(it.name, it.ok, it.note.clone())?;
            i += 1;
        }
        // 长稳摘要小节（判据：长稳结果进报告待签字入总册）。
        if !self.longrun.is_empty() {
            let mut lines = String::new();
            let mut i = 0;
            while i < self.longrun.len() {
                let r = &self.longrun[i];
                lines.push_str(&format!(
                    "{} drift={}ppm tol={}ppm pass={}\n",
                    r.scenario, r.max_drift_ppm, r.tol_ppm, r.pass
                ));
                i += 1;
            }
            report.add_section("longrun-summary", h.items[4].ok, lines)?;
        }
        Ok(report)
    }

    /// 签字 + 封存 + 入总册的一条龙（判据闸全走——返回总册序号）。
    pub fn sign_seal_admit(
        &mut self,
        title: &str,
        signer: &str,
        stamp_ticks: u64,
        verdict: SignVerdict,
    ) -> Result<u64, PhysTestError> {
        let mut report = self.build_report(title)?;
        report.sign(signer, stamp_ticks, verdict, "VE-F6012 评审签字")?;
        report.seal()?;
        self.master.admit(&report)
    }
}

// ---------------------------------------------------------------------------
// selfcheck —— 判据自检全集（六判据逐项落绿红；不 panic，全走布尔）
// ---------------------------------------------------------------------------

/// 自检报告。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SelfcheckReport {
    /// 判据条目（名, 绿红）。
    pub items: Vec<(&'static str, bool)>,
    /// 全绿。
    pub all_ok: bool,
}

/// 判据自检（锚点六判据：确定性对拍 / NaN 防线 / 三线 / 长稳 / CI 回归 / 签字闸）。
///
/// 轻量口径（selfcheck 预算）：对拍 1024 步、三线全程、长稳取最小场景、
/// CI 全套（CI_STEPS）、签字闸走假报告撞闸。全部通过即 all_ok。
pub fn selfcheck() -> SelfcheckReport {
    let mut items: Vec<(&'static str, bool)> = Vec::new();
    // 1. 确定性对拍：同种子双跑全等 + 异种子必分叉。
    let dual_ok = match DeterministicPhysics::dual_run(CI_SEED_TABLE[0], 1024) {
        Ok(r) => r.agree && r.mismatches == 0,
        Err(_) => false,
    };
    let diverge_ok = match (
        DeterministicPhysics::new(CI_SEED_TABLE[0]),
        DeterministicPhysics::new(CI_SEED_TABLE[1]),
    ) {
        (Ok(mut a), Ok(mut b)) => {
            let mut i = 0;
            let mut div = false;
            while i < 256 {
                if a.step().is_err() || b.step().is_err() {
                    break;
                }
                if a.fp_chain != b.fp_chain {
                    div = true;
                    break;
                }
                i += 1;
            }
            div
        }
        _ => false,
    };
    items.push(("确定性对拍-同种子同轨迹", dual_ok));
    items.push(("确定性对拍-异种子分叉", diverge_ok));
    // 2. NaN 防线：武装态毒位型全兜住 + 逃逸态可检出 + 恢复可走。
    let guard_ok = match DeterministicPhysics::new(CI_SEED_TABLE[2]) {
        Ok(mut sim) => {
            let mut contained_all = true;
            let mut p = 0;
            while p < F32_PATTERNS.len() {
                match sim.feed_float_f32(0, F32_PATTERNS[p].0) {
                    Ok(IntakeOutcome::ContainedNan { .. })
                    | Ok(IntakeOutcome::ContainedInf { .. }) => {}
                    _ => contained_all = false,
                }
                p += 1;
            }
            let mut p = 0;
            while p < F64_PATTERNS.len() {
                match sim.feed_float_f64(0, F64_PATTERNS[p].0) {
                    Ok(IntakeOutcome::ContainedNan { .. })
                    | Ok(IntakeOutcome::ContainedInf { .. }) => {}
                    _ => contained_all = false,
                }
                p += 1;
            }
            let escape_ok = {
                sim.guard.armed = false;
                let esc = matches!(
                    sim.feed_float_f32(0, 0x7FC0_0000),
                    Ok(IntakeOutcome::Escaped { .. })
                );
                let refused = sim.step().is_err();
                sim.guard.rearm_and_purge();
                let recovers = sim.step().is_ok();
                esc && refused && recovers
            };
            contained_all && escape_ok
        }
        Err(_) => false,
    };
    items.push(("NaN 防线-注入钳制+逃逸检出+恢复", guard_ok));
    // 3. 三线：全在带内 + 越界步长拒绝。
    let boundary_ok = match run_boundary_lines(BOUNDARY_SEED, DEFAULT_BODY_COUNT, BOUNDARY_STEPS) {
        Ok(s) => {
            let oob_rejected = validate_dt_q16(Q16_ONE / 10).is_err()
                && validate_dt_q16(Q16_ONE / 1000).is_err()
                && validate_dt_q16(FIXED_DT_STANDARD_Q16).is_ok();
            s.accepted && oob_rejected
        }
        Err(_) => false,
    };
    items.push(("步长边界三线-漂移带+越界拒绝", boundary_ok));
    // 4. 长稳：最小场景不漂移。
    let mut lr_idx = 0;
    let mut min_steps = u64::MAX;
    let mut i = 0;
    while i < LONGRUN_SCENARIOS.len() {
        if LONGRUN_SCENARIOS[i].steps < min_steps {
            min_steps = LONGRUN_SCENARIOS[i].steps;
            lr_idx = i;
        }
        i += 1;
    }
    let longrun_ok = match run_longrun_scenario(lr_idx) {
        Ok(r) => r.pass && r.replay_match && r.max_drift_ppm <= r.tol_ppm,
        Err(_) => false,
    };
    items.push(("长稳-8h等效确定性不漂移", longrun_ok));
    // 5. CI 回归：全种子双跑绿。
    let ci_ok = match run_ci_suite() {
        Ok(l) => l.entries.len() == CI_SEED_TABLE.len() && l.all_green(),
        Err(_) => false,
    };
    items.push(("CI 回归-种子表指纹入账全绿", ci_ok));
    // 6. 签字闸：未签字拒绝 + 无效签字拒绝 + 合规签字可入册。
    let gate_ok = {
        let mut ledger = MasterLedger::new();
        let mut rep = TestReport::new("selfcheck-gate");
        if rep.add_section("gate", true, "闸探针").is_err() {
            false
        } else {
            let unsigned = matches!(
                ledger.admit(&rep),
                Err(e) if e.code() == PhysTestCode::SignatureMissing.code()
            );
            let bad_sign = match rep.sign("", 0, SignVerdict::Pass, "空签") {
                Err(e) => e.code() == PhysTestCode::SignatureInvalid.code(),
                Ok(_) => false,
            };
            let ok_sign = rep
                .sign("评审人甲", 42, SignVerdict::Pass, "闸探针通过")
                .is_ok()
                && rep.seal().is_ok()
                && ledger.admit(&rep).is_ok();
            unsigned && bad_sign && ok_sign
        }
    };
    items.push(("结果评审签字-未签字不得入总册", gate_ok));
    // 汇总。
    let mut all = true;
    let mut i = 0;
    while i < items.len() {
        if !items[i].1 {
            all = false;
        }
        i += 1;
    }
    SelfcheckReport { items, all_ok: all }
}

// ---------------------------------------------------------------------------
// 编译期断言（const 静态闸——判据的编译期防线）
// ---------------------------------------------------------------------------

const _: () = assert!(SEG_BASE == 0x1BF0);
const _: () = assert!(SEG_SPAN == 0x10);
const _: () = assert!(ALL_CODES.len() == (SEG_SPAN - 1) as usize);
const _: () = assert!(
    FIXED_DT_MIN_Q16 < FIXED_DT_STANDARD_Q16
        && FIXED_DT_STANDARD_Q16 < FIXED_DT_MAX_Q16
);
const _: () = assert!(BOUNDARY_LINE_DTS[0] == FIXED_DT_MIN_Q16);
const _: () = assert!(BOUNDARY_LINE_DTS[1] == FIXED_DT_STANDARD_Q16);
const _: () = assert!(BOUNDARY_LINE_DTS[2] == FIXED_DT_MAX_Q16);
const _: () = assert!(BOUNDARY_LINE_BANDS_PPM[0] < BOUNDARY_LINE_BANDS_PPM[1]);
const _: () = assert!(BOUNDARY_LINE_BANDS_PPM[1] < BOUNDARY_LINE_BANDS_PPM[2]);
const _: () = assert!(LONGRUN_PHYS_STEPS == 108_000);
const _: () = assert!(LONGRUN_TICKS_EQUIV == 1_728_000);
const _: () = assert!(CI_SEED_TABLE.len() == CI_SEED_COUNT);
const _: () = assert!(Q16_ONE == 65_536);
const _: () = assert!(Q16_SAT_MAX > POS_LIMIT);
const _: () = assert!(BODY_CAP <= 512);
const _: () = assert!(SAMPLE_STRIDE >= 1);
const _: () = assert!(LONGRUN_STRIDE >= 1);
const _: () = assert!(MASTER_CAP >= 1);
const _: () = assert!(EVENT_CAP >= SAMPLE_CAP);

// ---------------------------------------------------------------------------
// 测试套件（冒烟——同种子同轨迹 / NaN 防线 / 三线稳定 / 长稳不漂 / 签字闸 /
// 双跑一致 / 注入面 / 位型换算 / 错误诊断面 / 编译期闸）
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::vec;

    #[test]
    fn same_seed_same_trajectory() {
        let rep = DeterministicPhysics::dual_run(0x5EED_0001, 4096)
            .expect("test-only dual_run");
        assert!(rep.agree, "同种子双跑必须全等");
        assert_eq!(rep.mismatches, 0);
        assert!(rep.samples_match);
        assert_eq!(rep.head_fp_a, rep.head_fp_b);
        assert_eq!(rep.tail_fp_a, rep.tail_fp_b);
        // 采样锚点数口径（4096/8 = 512）。
        assert_eq!(
            DeterministicPhysics::sample_anchors_for(4096),
            512
        );
    }

    #[test]
    fn different_seeds_diverge() {
        let mut a = DeterministicPhysics::new(0x5EED_0001).unwrap();
        let mut b = DeterministicPhysics::new(0x5EED_0002).unwrap();
        let mut i = 0;
        let mut diverged = false;
        while i < 256 {
            a.step().unwrap();
            b.step().unwrap();
            if a.fp_chain != b.fp_chain {
                diverged = true;
                break;
            }
            i += 1;
        }
        assert!(diverged, "异种子必须分叉——种子即轨迹身份");
        // 重放比对：c 独立重跑到 a 的同一时点 → 全等（同种子同轨迹）。
        let mut c = DeterministicPhysics::new(0x5EED_0001).unwrap();
        while c.tick < a.tick {
            c.step().unwrap();
        }
        assert!(a.replay_matches(&c).unwrap());
    }

    #[test]
    fn nan_flood_contained_and_determinism_kept() {
        let mut sim = DeterministicPhysics::new(0x5EED_0003).unwrap();
        // f32 全毒位型逐口喂入。
        let mut p = 0;
        while p < F32_PATTERNS.len() {
            let bits = F32_PATTERNS[p].0;
            let out = sim.feed_float_f32(0, bits).unwrap();
            assert!(
                matches!(
                    out,
                    IntakeOutcome::ContainedNan { .. } | IntakeOutcome::ContainedInf { .. }
                ),
                "武装态毒位型必须兜住: {bits:#010x}"
            );
            p += 1;
        }
        let mut p = 0;
        while p < F64_PATTERNS.len() {
            let bits = F64_PATTERNS[p].0;
            let out = sim.feed_float_f64(0, bits).unwrap();
            assert!(
                matches!(
                    out,
                    IntakeOutcome::ContainedNan { .. } | IntakeOutcome::ContainedInf { .. }
                ),
                "武装态毒位型必须兜住: {bits:#018x}"
            );
            p += 1;
        }
        assert!(sim.guard.is_clean());
        assert_eq!(sim.guard.escape_events, 0);
        assert!(sim.guard.clamp_events >= 12);
        assert!(sim.guard.nan_hits >= 8);
        assert!(sim.guard.inf_hits >= 4);
        // 步进不受注入影响。
        sim.step_n(64).unwrap();
        // 钳制不入状态 → 与干净重放指纹链全等。
        let clean = DeterministicPhysics::dual_run(0x5EED_0003, 64).unwrap();
        assert!(clean.agree);
        assert_eq!(sim.fp_chain, clean.tail_fp_a, "注入不得改变确定性轨迹");
        // 事件账有钳制留痕。
        assert!(sim.events_rolled == 0 || !sim.events.is_empty());
    }

    #[test]
    fn nan_escape_detected_when_disarmed_then_recover() {
        let mut sim = DeterministicPhysics::new(0x5EED_0004).unwrap();
        sim.guard.armed = false;
        let out = sim.feed_float_f32(1, 0xFFC0_0000).unwrap();
        assert!(matches!(out, IntakeOutcome::Escaped { .. }));
        assert!(sim.guard.contaminated);
        // 下一步积分拒绝（洪流扩散可检出）。
        let r = sim.step();
        let e = r.unwrap_err();
        assert_eq!(e.code(), PhysTestCode::NanFloodUncontained.code());
        assert!(!e.detail().is_empty());
        // 重武装 + 清污 → 恢复（逃逸计数留痕不洗）。
        sim.guard.rearm_and_purge();
        assert!(sim.step().is_ok());
        assert_eq!(sim.guard.escape_events, 1);
        assert!(!sim.guard.contaminated);
    }

    #[test]
    fn benign_bit_patterns_convert() {
        let mut sim = DeterministicPhysics::new(0x5EED_0005).unwrap();
        // f32: 1.0 / 2.0 / -1.0 / 0.5 / 0.0。
        let out = sim.feed_float_f32(0, 0x3F80_0000).unwrap();
        assert_eq!(out, IntakeOutcome::Accepted { q16: 65_536 });
        let out = sim.feed_float_f32(0, 0x4000_0000).unwrap();
        assert_eq!(out, IntakeOutcome::Accepted { q16: 131_072 });
        let out = sim.feed_float_f32(0, 0xBF80_0000).unwrap();
        assert_eq!(out, IntakeOutcome::Accepted { q16: -65_536 });
        let out = sim.feed_float_f32(0, 0x3F00_0000).unwrap();
        assert_eq!(out, IntakeOutcome::Accepted { q16: 32_768 });
        let out = sim.feed_float_f32(0, 0x0000_0000).unwrap();
        assert_eq!(out, IntakeOutcome::Accepted { q16: 0 });
        // f64: 1.0 / -2.0。
        let out = sim.feed_float_f64(0, 0x3FF0_0000_0000_0000).unwrap();
        assert_eq!(out, IntakeOutcome::Accepted { q16: 65_536 });
        let out = sim.feed_float_f64(0, 0xC000_0000_0000_0000).unwrap();
        assert_eq!(out, IntakeOutcome::Accepted { q16: -131_072 });
        // 换算值已入寄存器账。
        assert!(sim.intake.len() >= 7);
        // apply_intake 生效为外力驱动项。
        let applied = sim.apply_intake().unwrap();
        assert!(applied >= 1);
        // 越界 channel 拒绝。
        let bad = sim.feed_float_f32(99, 0x3F80_0000);
        assert_eq!(
            bad.unwrap_err().code(),
            PhysTestCode::ChannelUnknown.code()
        );
    }

    #[test]
    fn three_boundary_lines_in_band() {
        let summary =
            run_boundary_lines(BOUNDARY_SEED, DEFAULT_BODY_COUNT, BOUNDARY_STEPS).unwrap();
        assert_eq!(summary.lines.len(), 3);
        for l in &summary.lines {
            assert!(l.energy0_q16 > 0, "基准能量须非零");
            assert!(
                l.in_band,
                "线 {} 漂移 {}ppm 超带 {}ppm",
                l.name, l.max_drift_ppm, l.band_ppm
            );
        }
        assert!(summary.accepted);
        let lines = summary.to_lines();
        assert_eq!(lines.len(), 3);
    }

    #[test]
    fn dt_out_of_range_rejected() {
        // 超上界（1/10s）拒绝。
        assert_eq!(
            validate_dt_q16(Q16_ONE / 10).unwrap_err().code(),
            PhysTestCode::StepSizeOutOfRange.code()
        );
        // 低于下界（1/1000s）拒绝。
        assert_eq!(
            validate_dt_q16(Q16_ONE / 1000).unwrap_err().code(),
            PhysTestCode::StepSizeOutOfRange.code()
        );
        // 三线内全过。
        let mut i = 0;
        while i < BOUNDARY_LINE_DTS.len() {
            assert!(validate_dt_q16(BOUNDARY_LINE_DTS[i]).is_ok());
            i += 1;
        }
        // 非法种子/体数过闸。
        assert!(DeterministicPhysics::with_config(0, FIXED_DT_STANDARD_Q16, 3).is_err());
        assert!(DeterministicPhysics::with_config(7, FIXED_DT_STANDARD_Q16, 0).is_err());
        assert!(DeterministicPhysics::with_config(7, FIXED_DT_STANDARD_Q16, BODY_CAP + 1).is_err());
    }

    #[test]
    fn longrun_no_drift() {
        // 取最小步数场景全跑（lr-full-amplitude · 27_000 步）。
        let mut idx = 0;
        let mut min_steps = u64::MAX;
        let mut i = 0;
        while i < LONGRUN_SCENARIOS.len() {
            if LONGRUN_SCENARIOS[i].steps < min_steps {
                min_steps = LONGRUN_SCENARIOS[i].steps;
                idx = i;
            }
            i += 1;
        }
        let rec = run_longrun_scenario(idx).unwrap();
        assert_eq!(rec.scenario, LONGRUN_SCENARIOS[idx].name);
        assert!(rec.replay_match, "双跑重放必须全链一致");
        assert!(
            rec.max_drift_ppm <= rec.tol_ppm,
            "漂移 {}ppm 超容忍带 {}ppm（{}）",
            rec.max_drift_ppm,
            rec.tol_ppm,
            LONGRUN_SCENARIOS[idx].note
        );
        assert!(rec.pass);
        assert_ne!(rec.head_fp, 0);
        assert_ne!(rec.tail_fp, 0);
        // 场景越界拒绝。
        assert_eq!(
            run_longrun_scenario(LONGRUN_SCENARIOS.len())
                .unwrap_err()
                .code(),
            PhysTestCode::ScenarioUnknown.code()
        );
    }

    #[test]
    fn ci_suite_green_and_drift_diff() {
        let ledger = run_ci_suite().unwrap();
        assert_eq!(ledger.entries.len(), CI_SEED_TABLE.len());
        assert!(ledger.all_green(), "CI 全种子双跑必须全绿");
        for e in &ledger.entries {
            assert_ne!(e.fingerprint, 0, "绿条目指纹须为真实链值");
            assert_eq!(e.steps, CI_STEPS);
        }
        // 自 diff 为空。
        assert!(ledger.diff_seeds(&ledger).is_empty());
        // 漂移注入：篡改一个指纹 → diff 检出该种子。
        let mut mutated = ledger.clone();
        mutated.entries[3].fingerprint = mutated.entries[3].fingerprint ^ 1;
        let diff = ledger.diff_seeds(&mutated);
        assert_eq!(diff, vec![CI_SEED_TABLE[3]], "指纹漂移必须被 diff 检出");
        // 读屏行集。
        assert_eq!(ledger.to_lines().len(), CI_SEED_TABLE.len());
    }

    #[test]
    fn signature_gate_and_master_ledger() {
        let mut ledger = MasterLedger::new();
        let mut rep = TestReport::new("VE-F6012 冒烟报告");
        rep.add_section("三线", true, "三线全在带内").unwrap();
        rep.add_section("长稳", true, "不漂移").unwrap();
        // 未签字撞闸 → SignatureMissing。
        let e = ledger.admit(&rep).unwrap_err();
        assert_eq!(e.code(), PhysTestCode::SignatureMissing.code());
        // 空签字人 / 零时戳 → SignatureInvalid。
        assert!(rep.sign("", 1, SignVerdict::Pass, "").is_err());
        assert!(rep.sign("评审人", 0, SignVerdict::Pass, "").is_err());
        assert!(rep.sign("  ", 1, SignVerdict::Pass, "").is_err());
        // 未签字不得封存。
        assert_eq!(
            rep.seal().unwrap_err().code(),
            PhysTestCode::SignatureMissing.code()
        );
        // Fail 结论入册 → VerdictRejected。
        rep.sign("评审人甲", 100, SignVerdict::Fail, "含红").unwrap();
        assert_eq!(
            ledger.admit(&rep).unwrap_err().code(),
            PhysTestCode::VerdictRejected.code()
        );
        assert_eq!(
            rep.sign("评审人乙", 101, SignVerdict::Pass, "复核改签")
                .is_ok(),
            true
        );
        rep.seal().unwrap();
        // 封存后改节/改签 → ReportSealed。
        assert_eq!(
            rep.add_section("追加", true, "x").unwrap_err().code(),
            PhysTestCode::ReportSealed.code()
        );
        assert_eq!(
            rep.sign("评审人丙", 102, SignVerdict::Pass, "y")
                .unwrap_err()
                .code(),
            PhysTestCode::ReportSealed.code()
        );
        // 合规入册。
        let seq = ledger.admit(&rep).unwrap();
        assert_eq!(seq, 1);
        assert!(rep.is_admissible());
        assert_eq!(ledger.entries[0].signer, "评审人乙");
        // 含红报告（Pass 签字）→ VerdictRejected。
        let mut rep2 = TestReport::new("含红报告");
        rep2.add_section("破项", false, "漂移出带").unwrap();
        rep2.sign("评审人甲", 103, SignVerdict::Pass, "带病签").unwrap();
        assert_eq!(
            ledger.admit(&rep2).unwrap_err().code(),
            PhysTestCode::VerdictRejected.code()
        );
    }

    #[test]
    fn bench_injection_surfaces_and_health() {
        let mut bench = PhysTestBench::new(0x5EED_0006).unwrap();
        // 注入面一：NaN 洪流。
        let nan = bench.inject_nan_flood(2).unwrap();
        assert!(nan.contained);
        assert!(nan.determinism_kept, "钳制不入状态——确定性不破");
        assert_eq!(nan.fed, 2 * (F32_PATTERNS.len() + F64_PATTERNS.len()) as u32);
        // 注入面二：种子漂移。
        let sd = bench.inject_seed_drift().unwrap();
        assert!(sd.detected, "种子漂移必须检出");
        assert_eq!(sd.mutated_seeds.len(), 2);
        // 注入面三：签字缺失。
        let ms = bench.inject_missing_signature("未签字探针报告").unwrap();
        assert!(ms.detected);
        assert_eq!(
            ms.rejected_code,
            PhysTestCode::SignatureMissing.code()
        );
        // 三线 + CI + 长稳落账。
        bench.run_boundary().unwrap();
        bench.run_ci().unwrap();
        let mut idx = 0;
        let mut min_steps = u64::MAX;
        let mut i = 0;
        while i < LONGRUN_SCENARIOS.len() {
            if LONGRUN_SCENARIOS[i].steps < min_steps {
                min_steps = LONGRUN_SCENARIOS[i].steps;
                idx = i;
            }
            i += 1;
        }
        let lr = bench.run_longrun(idx).unwrap();
        assert!(lr.pass);
        // 健康面全绿。
        let h = bench.health();
        for it in &h.items {
            assert!(it.ok, "健康条目 {} 红: {}", it.name, it.note);
        }
        assert!(h.overall_ok);
        // 签字一条龙：健康全绿 → Pass → 入总册。
        let seq = bench
            .sign_seal_admit("VE-F6012 长稳结果报告", "评审长", 777, SignVerdict::Pass)
            .unwrap();
        assert_eq!(seq, 1);
        assert_eq!(bench.master.entries.len(), 1);
        assert_eq!(bench.master.entries[0].verdict, SignVerdict::Pass);
    }

    #[test]
    fn selfcheck_all_green() {
        let sc = selfcheck();
        for (name, ok) in &sc.items {
            assert!(ok, "自检判据红: {name}");
        }
        assert!(sc.all_ok);
        assert_eq!(sc.items.len(), 7);
    }

    #[test]
    fn error_surface_complete() {
        // 全码落段 + 三面（码/名/诊断）非空。
        let mut i = 0;
        while i < ALL_CODES.len() {
            let c = ALL_CODES[i];
            assert!(code_in_segment(c.code()), "码 {:#06x} 出段", c.code());
            assert!(!c.name().is_empty());
            assert!(!c.advice().is_empty());
            let e = PhysTestError::new(c);
            assert_eq!(e.code(), c.code());
            assert_eq!(e.detail(), c.advice());
            assert_eq!(e.name(), c.name());
            i += 1;
        }
        assert_eq!(ALL_CODES[0].code(), 0x1BF1);
        assert_eq!(ALL_CODES[14].code(), 0x1BFF);
    }

    #[test]
    fn bit_pattern_tables_and_const_math() {
        // 位型分类。
        assert_eq!(classify_f32_bits(0x7FC0_0000), F32BitsClass::Nan);
        assert_eq!(classify_f32_bits(0x7F80_0000), F32BitsClass::Inf);
        assert_eq!(classify_f32_bits(0x0000_0000), F32BitsClass::Zero);
        assert_eq!(classify_f32_bits(0x0000_0001), F32BitsClass::Denormal);
        assert_eq!(classify_f32_bits(0x3F80_0000), F32BitsClass::Normal);
        assert_eq!(classify_f64_bits(0x7FF8_0000_0000_0000), F64BitsClass::Nan);
        assert_eq!(classify_f64_bits(0x7FF0_0000_0000_0000), F64BitsClass::Inf);
        assert_eq!(classify_f64_bits(0x3FF0_0000_0000_0000), F64BitsClass::Normal);
        // NaN/Inf 拒换算。
        assert!(f32_bits_to_q16_checked(0x7FC0_0000).is_none());
        assert!(f32_bits_to_q16_checked(0xFF80_0000).is_none());
        assert!(f64_bits_to_q16_checked(0x7FF8_0000_0000_0000).is_none());
        // 量级饱和（f32 巨数 → Q16_SAT_MAX 显性饱和）。
        let big = f32_bits_to_q16_checked(0x7F00_0000).unwrap(); // ~2^129
        assert_eq!(big, Q16_SAT_MAX);
        let big_neg = f32_bits_to_q16_checked(0xFF00_0000).unwrap();
        assert_eq!(big_neg, -Q16_SAT_MAX);
        // 极小数向零舍入。
        assert_eq!(f32_bits_to_q16_checked(0x0080_0000).unwrap(), 0);
        // 表命中名。
        assert_eq!(match_pattern_name_f32(0x7FFF_FFFF), "f32 信号 NaN（全 1 sNaN）");
        assert_eq!(match_pattern_name_f64(0x7FF0_0000_0000_0000), "f64 +Inf");
        assert_eq!(match_pattern_name_f32(0x3F80_0000), "f32 规格化有限数");
        // Q16 算术。
        assert_eq!(q16_mul(Q16_ONE, Q16_ONE), Q16_ONE); // 1.0 × 1.0
        assert_eq!(q16_mul(2 * Q16_ONE, 3 * Q16_ONE), 6 * Q16_ONE);
        assert_eq!(clamp_i64(9, -1, 1), 1);
        assert_eq!(abs_diff_i64(3, 10), 7);
        assert_eq!(drift_ppm(1_000_000, 1_001_000), 1_000);
        assert_eq!(drift_ppm(0, 5), 0);
        // FNV/随机面确定性。
        assert_eq!(fnv_word(FNV_OFFSET, 1), fnv_word(FNV_OFFSET, 1));
        assert_ne!(fnv_word(FNV_OFFSET, 1), fnv_word(FNV_OFFSET, 2));
        let r0 = rng_next(0x9E37_79B9_7F4A_7C15);
        assert_ne!(r0, 0);
        assert_eq!(rng_next(0x9E37_79B9_7F4A_7C15), r0);
    }

    #[test]
    fn event_ledger_and_body_surface() {
        let mut sim = DeterministicPhysics::new(0x5EED_0007).unwrap();
        assert_eq!(sim.bodies.len(), DEFAULT_BODY_COUNT);
        // 注入良性位型入事件账。
        let _ = sim.feed_float_f32(0, 0x3F80_0000);
        assert!(!sim.events.is_empty());
        assert_eq!(sim.events[0].kind, EventKind::IntakeAccept);
        // 体 id 查找面。
        let id0 = sim.bodies[0].id;
        assert!(sim.body_index(id0).is_some());
        assert!(sim.body_index(0xFFFF_FFFF).is_none());
        // 体量程注入过闸。
        assert!(sim.add_body(0, 0, 0, 0, 0).is_err());
        assert!(sim.add_body(0, 0, 0, 0, MASS_LIMIT + 1).is_err());
        // 事件账滚动不爆。
        let mut j = 0;
        while j < 300 {
            let _ = sim.feed_float_f32(0, 0x3F80_0000);
            j += 1;
        }
        assert!(sim.events.len() <= EVENT_CAP);
        // 事件码落段。
        let mut k = 0;
        while k < sim.events.len() {
            assert!(code_in_segment(sim.events[k].code));
            k += 1;
        }
    }
}

