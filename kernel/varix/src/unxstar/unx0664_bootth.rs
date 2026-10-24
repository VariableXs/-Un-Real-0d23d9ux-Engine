//! unx0664_bootth —— UNX-F0664 · 冷启动阈值判定账（≤3 秒硬线）。
//!
//! ═══════════════════════════════════════════════════════════════════════════
//! 任务锚点（主册原文抄录）：
//!   来源：docs/Varix/CoRun Varix STAR II · Unxreal/CoRun Varix STAR II · Unxreal.md 行 2233
//!   | UNX-F0664 | 冷启动阈值判定账（≤3 秒硬线） | 580 | 已深化 | UNX-F0664-J1 冷启动 T0→T3 ≤3,000ms 判定 50 次全过，超阈值拦截检出 100% 且账行三要素齐 |
//!
//! J 判据（UNX-F0664-J1）原文：
//!   「冷启动 T0→T3 ≤3,000ms 判定 50 次全过，超阈值拦截检出 100% 且账行三要素齐」
//!
//! 定位原文（同册 UNX-A1-B34 批引言）：
//!   「AI-01 承办｜域账累计：159650 + 本批 11480 = 171130 / 240,000｜防重：引导耗时账全域
//!    （锚点表/段分解/阈值账/五数/画像）归本批独占；F0198 时间线与 F0413 计时包为既有账面
//!    （本批只对冲不改写——改写走原批勘误）；冷启动 ≤3 秒判据为任务书验收判据 3 的执行面」
//!
//! 邻批对接（F0665 校准规程 · 阈值变更纪律唯一通道）：
//!   | UNX-F0665 | 阈值试产校准规程（校准→冻结→入基准表） | 580 | 已深化 | UNX-F0665-J1 校准规程三步（采样/统计/冻结）留痕齐，冻结后阈值表哈希封存且变更走 ADR |
//!   → 3,000ms 硬线在本模块冻结（`ThresholdLine::frozen()` 哈希封存；`attempt_direct_change`
//!     恒拒并立案）；任何变更只能经 `CalibrationGate` 走 F0665 三步（采样/统计/冻结）成型
//!     提案 + ADR 审批落地——本模块仅做对接声明（`declare_calibration`），零改写落地通道。
//!
//! 锚点口径（本模块执行面命名，与 F0661/F0662/F0678 对冲不改写邻批账面）：
//!   T0 固件交接 / T1 早传完成 / T2 内核主入口 / T3 用户面首帧（四锚）；
//!   三段分解：T0T1 固件段 / T1T2 早传段 / T2T3 内核段；
//!   与 F0661 锚点表、F0662 段分解账的字段映射对冲差异走原批勘误，本模块只落判定账。
//! ═══════════════════════════════════════════════════════════════════════════
//!
//! 子系统清单（全部整数运算，零浮点）：
//!  1. `BootSpan`        冷启动四锚点测量账：逐锚点时戳（T0..T3 微秒）+ 三段分解
//!     （T0T1/T1T2/T2T3）+ 守恒核（Σ三段 ≡ T0→T3 总时长；另设独立申报总时长对冲位，
//!     录入时强制恒等，破坏即拦截立案）+ 主导段判定（并列取最早段，确定性）。
//!  2. `ThresholdLine`   3,000ms 硬线冻结账：FNV-1a 哈希封存（来源串 + 限值双料）+
//!     完整性复核（`verify_integrity`）+ 解冻恒拒（直接改写 0 通道，变更走 F0665+ADR）。
//!  3. `ThresholdJudge`  阈值判定器：微秒精确判定（≤3,000,000us 判过，超即拦截，
//!     余量/超额毫秒整数核算）；判定账行三要素（次序号/耗时/结论）产出 + 复核
//!     （结论对耗时按硬线重判必须一致——防虚记）。
//!  4. `FiftyRunBook`    50 次判定账：逐次记录（次序号连续零跳号：首笔必 1、后笔必前笔+1；
//!     重复登记/跳号一律拒绝并立案）+ 双口径对账（账面计数 vs 逐行扫描重算）+
//!     封存守卫（满 50 自动封账，封后禁写）。
//!  5. `FiveNumSummary`  五数概括 + P95：整数最近秩分位（nearest-rank，无插值无浮点），
//!     n=50 口径 idx(25%)=12 / idx(50%)=24 / idx(75%)=37 / idx(95%)=47。
//!  6. `AttributionBook` 超阈归因账：超阈样本归因到最大段（固件/早传/内核三选一），
//!     归因分布万分比（Σ≡10000 守恒，余数补给最末非零段）+ 逐段累计超额微秒。
//!  7. `CalibrationGate` F0665 校准规程对接面：三步状态机（采样→统计→冻结提案），
//!     ≥20 样本起步、P95×4/3 整数余量公式成型提案 + 冻结哈希；落地改写恒拒
//!     （F0665-J1：冻结后阈值表哈希封存且变更走 ADR）。
//!  8. 事件账（环形 + 满员逐出落账 + 序号连续核 + 篡改审计）+ 案件账（满员逐出最老案）。
//!  9. `BootThEngine`    判定账引擎：录入流水线（机号核→锚点序核→段宽核→守恒核→
//!     判定→入账→归因→立案→事件→满 50 自动封账）+ `health()` 三色 + `verify_j1()`
//!     判据聚合核（50 次全过 / 超阈拦截检出 100% / 账行三要素齐 三断言一次出）。
//! 10. 注入面：超阈注入 / 跳号注入 / 守恒破坏注入（各注入必检出——未检出即立案红灯）。
//! 11. 双跑确定性对拍（同输入同种子两引擎指纹全等）+ 三机矩阵聚合（M0/M1/M2 三账 J1 3/3）。
//! 12. 文本报告（行数封顶、截断落账于返回结构、零静默吞行）。
//!
//! 纪律：no_std + alloc；内核路径零 unwrap/expect/panic!/运行期 assert!（仅编译期 const assert）；
//! 可失败全走 `Result<T, BootThError>`，错误枚举每变体带 `detail()` 中文可操作诊断；
//! 溢出显性 saturating/wrapping 或 checked 升宽；>64KB 结构体一律堆上；本模块完全自包含
//! （仅 `use core::…` / `use alloc::…`，零 `use crate::…`）。
//!
//! 诊断码段：0x1FF0–0x1FFF（全域核查空闲。已占邻段：0x1F00=ves495_s28gov、
//! 0x1F20=ves496_s28process、0x1F40=unx2782_ubuddymerge、0x1F50=unx0587_dupkey2、
//! 0x1F60=ves497_s28violation、0x1F80=unx2801_ubigpoolcap、0x1FA0=ves498_s28evolve、
//! 0x1FC0=ves499_s28version、0x1FE0=ves500_s28arbitrate；0x1FF0 段空闲，本模块独占）。

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

// ═══════════════════════════════════════════════════════════════════════════
// 版本与冻结常量（各常量注来源）
// ═══════════════════════════════════════════════════════════════════════════

/// 模块版本（冻结命名：unx0664-bootth-v1）。
pub const BOOTTH_VERSION: &str = "unx0664-bootth-v1";

/// 诊断段基址（0x1FF0 独占 0x10，全域核查空闲）。
pub const SEG_BASE: u16 = 0x1FF0;

/// 诊断段掩码（0x1FF0..0x1FFF 共 16 码）。
pub const SEG_MASK: u16 = 0x000F;

/// 冷启动硬线：3,000 ms。
/// 来源：UNX-F0664-J1 判据原文「T0→T3 ≤3,000ms」；任务书验收判据 3 执行面。
/// 冻结纪律：本值写死且哈希封存，变更唯一通道 = F0665 校准规程 + ADR。
pub const HARD_LIMIT_MS: u64 = 3_000;

/// 硬线微秒换算值（3,000,000 us；判定在微秒域精确到 1us，毫秒域仅作账面展示）。
pub const HARD_LIMIT_US: u64 = HARD_LIMIT_MS * 1_000;

/// 判据判定次数：50 次（J1 原文「判定 50 次全过」）。
pub const RUN_COUNT: usize = 50;

/// 四锚点数（T0/T1/T2/T3）。
pub const ANCHOR_COUNT: usize = 4;

/// 三段数（T0T1/T1T2/T2T3）。
pub const SEGMENT_COUNT: usize = 3;

/// 三机矩阵机数（M0/M1/M2；UNX 三机矩阵域口径）。
pub const MACHINE_COUNT: usize = 3;

/// 微秒→毫秒换算基数。
pub const US_PER_MS: u64 = 1_000;

/// 事件账默认容量（环形；满员逐出最老事件并落账驱逐计数）。
/// 来源：本域既定范式（邻件 unx0663 同容量档）。
pub const EVENT_CAP_DEFAULT: usize = 256;

/// 案件账默认容量（满员逐出最老案，驱逐计数入指纹）。
pub const CASE_CAP_DEFAULT: usize = 128;

/// 报告默认行数上限（截断落账于 `ReportOutcome`，零静默吞行）。
pub const REPORT_LINE_CAP_DEFAULT: usize = 80;

/// F0665 校准规程最低样本数（20；来源：F0665-J1 校准规程三步之「采样」起步档）。
pub const CALIB_MIN_SAMPLES: usize = 20;

/// F0665 校准采样容量上限（防单规程失控注入）。
pub const CALIB_SAMPLE_CAP: usize = 256;

/// 校准余量公式分子：提案 = P95 × 4 / 3（整数除法，余量 ≈33.3%）。
/// 来源：F0665 校准规程「统计」步既定余量口径（试产档 P95 上浮三分之一）。
pub const CALIB_HEADROOM_NUM: u64 = 4;

/// 校准余量公式分母（同上）。
pub const CALIB_HEADROOM_DEN: u64 = 3;

/// 校准样本单笔上限（60,000 ms = 1 分钟；超此必为采样域配置错误）。
pub const CALIB_SAMPLE_MAX_MS: u64 = 60_000;

/// 合成样本 T0 基线（微秒；≈2025-01-02T12:26:40Z，账面任取基准，写死冻结）。
pub const SYNTH_T0_BASE_US: u64 = 1_735_800_000_000_000;

/// 合成样本相邻引导步进（40 s/次；账面：连跑间隔口径，写死冻结）。
pub const SYNTH_T0_STEP_US: u64 = 40_000_000;

/// 硬线冻结来源串（入封存哈希；改此串即改变 `ThresholdLine::frozen()` 指纹）。
pub const LINE_PROVENANCE: &str = "UNX-F0664/任务书验收判据3/F0665冻结对接";

// ═══════════════════════════════════════════════════════════════════════════
// 诊断码（0x1FF0..0x1FFF · 16 码一码一义）
// ═══════════════════════════════════════════════════════════════════════════

/// 0x1FF0 状态机乱序（判定账/校准规程非合法相位迁移）。
pub const D_PHASE: u16 = SEG_BASE;
/// 0x1FF1 次序号跳号（判定账次序号不连续：首笔非 1 或后笔非前笔+1）。
pub const D_SEQ_GAP: u16 = SEG_BASE + 0x01;
/// 0x1FF2 锚点时序非单调（T0..T3 任一后锚早于前锚）。
pub const D_ANCHOR_DISORDER: u16 = SEG_BASE + 0x02;
/// 0x1FF3 守恒破坏（Σ三段 ≠ T0→T3 总时长，或申报总时长与锚点差值不符）。
pub const D_CONSERVATION: u16 = SEG_BASE + 0x03;
/// 0x1FF4 硬线解冻尝试（直接改写 3,000ms 冻结硬线被拒）。
pub const D_THAW: u16 = SEG_BASE + 0x04;
/// 0x1FF5 校准规程违规（F0665 对接面非法使用或落地改写被拒）。
pub const D_CALIB_INVALID: u16 = SEG_BASE + 0x05;
/// 0x1FF6 算术溢出（checked 运算越界，含锚点篡改后的段宽下溢）。
pub const D_OVERFLOW: u16 = SEG_BASE + 0x06;
/// 0x1FF7 账簿容量越界（判定账满 50 / 校准采样满容）。
pub const D_CAPACITY: u16 = SEG_BASE + 0x07;
/// 0x1FF8 机号不符（样本机号与账本机号不一致，或机号解码失败）。
pub const D_UNKNOWN_MACHINE: u16 = SEG_BASE + 0x08;
/// 0x1FF9 配置非法（run_count/硬线/容量域越界）。
pub const D_CONFIG_INVALID: u16 = SEG_BASE + 0x09;
/// 0x1FFA 事件账序号断链（事件窗口序号不连续）。
pub const D_EVENT_SEQ: u16 = SEG_BASE + 0x0A;
/// 0x1FFB 注入未检出（注入面复核失败，立案红灯）。
pub const D_INJECTION_UNCAUGHT: u16 = SEG_BASE + 0x0B;
/// 0x1FFC 样本不足（五数概括/校准统计最少样本未达）。
pub const D_INSUFFICIENT: u16 = SEG_BASE + 0x0C;
/// 0x1FFD 封存违规（封账后再写 / 重复封账 / 未封账即聚合判定）。
pub const D_SEAL: u16 = SEG_BASE + 0x0D;
/// 0x1FFE 账行三要素不齐（次序号缺位 / 耗时缺位 / 结论与耗时重判不一致）。
pub const D_ROW_INCOMPLETE: u16 = SEG_BASE + 0x0E;
/// 0x1FFF 重复登记（同次序号二次入账）。
pub const D_DUPLICATE: u16 = SEG_BASE + 0x0F;

// ═══════════════════════════════════════════════════════════════════════════
// 编译期断言（判据常量冻结核 + 合成画像安全带核）
// ═══════════════════════════════════════════════════════════════════════════

const _: () = assert!(SEG_BASE == 0x1FF0);
const _: () = assert!(SEG_BASE + SEG_MASK == 0x1FFF);
const _: () = assert!(HARD_LIMIT_MS == 3_000);
const _: () = assert!(HARD_LIMIT_US == 3_000_000);
const _: () = assert!(RUN_COUNT == 50);
const _: () = assert!(ANCHOR_COUNT == 4);
const _: () = assert!(SEGMENT_COUNT == 3);
const _: () = assert!(MACHINE_COUNT == 3);
const _: () = assert!(US_PER_MS == 1_000);
const _: () = assert!(CALIB_MIN_SAMPLES == 20);
const _: () = assert!(CALIB_HEADROOM_NUM == 4);
const _: () = assert!(CALIB_HEADROOM_DEN == 3);
const _: () = assert!(CALIB_HEADROOM_NUM > CALIB_HEADROOM_DEN);
const _: () = assert!(EVENT_CAP_DEFAULT >= 64);
const _: () = assert!(CASE_CAP_DEFAULT >= 32);
const _: () = assert!(REPORT_LINE_CAP_DEFAULT >= 22, "报告上限须容得下头尾固定行+8 行账行");
// 名义档合成画像：最坏总耗时（Σ基线 + 3×抖动）必须 < 硬线（安全带 ≥ 80ms）。
const _: () = assert!(SpanProfile::NominalTight.worst_total_ms() < HARD_LIMIT_MS);
const _: () = assert!(SpanProfile::NominalMid.worst_total_ms() < HARD_LIMIT_MS);
const _: () = assert!(SpanProfile::NominalHigh.worst_total_ms() < HARD_LIMIT_MS);
const _: () = assert!(SpanProfile::NominalHigh.worst_total_ms() <= HARD_LIMIT_MS - 80);
// 超阈档合成画像：最乐观总耗时（Σ基线 − 3×抖动）必须 > 硬线（必超阈）。
const _: () = assert!(SpanProfile::OverFirmware.floor_total_ms() > HARD_LIMIT_MS);
const _: () = assert!(SpanProfile::OverEarlyXfer.floor_total_ms() > HARD_LIMIT_MS);
const _: () = assert!(SpanProfile::OverKernel.floor_total_ms() > HARD_LIMIT_MS);
// 超阈档主导段确定性：主导段下界 > 任一非主导段上界（抖动不改变归因结论）。
const _: () = assert!(SpanProfile::OverFirmware.dominance_guaranteed());
const _: () = assert!(SpanProfile::OverEarlyXfer.dominance_guaranteed());
const _: () = assert!(SpanProfile::OverKernel.dominance_guaranteed());

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

/// 从标准偏移基起算（普通 fn 封装）。
pub fn fnv1a64(data: &[u8]) -> u64 {
    fnv1a64_from(FNV_OFFSET, data)
}

// ═══════════════════════════════════════════════════════════════════════════
// 确定性 LCG（合成采样源；同种子同序列——双跑对拍前提）
// ═══════════════════════════════════════════════════════════════════════════

/// 确定性线性同余发生器（Numerical Recipes 常数）。
pub struct Lcg(u64);

impl Lcg {
    /// 以种子构造（同种子同序列）。
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
// 整数工具（插入排序 + 最近秩分位索引）
// ═══════════════════════════════════════════════════════════════════════════

/// u64 升序插入排序（确定性；五数/校准统计专用，n≤256 规模足够）。
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

/// 最近秩分位索引：idx(pct) = ceil(pct%×n) − 1，钳入 [0, n−1]（n≥1）。
/// 整数实现：ceil(pct*n/100) = (pct*n + 99) / 100。
/// n=50 口径：p25→12 / p50→24 / p75→37 / p95→47。
pub fn nearest_rank_index(n: usize, pct: u64) -> usize {
    if n == 0 {
        return 0;
    }
    let np = pct.saturating_mul(n as u64);
    let mut rank = (np + 99) / 100;
    if rank == 0 {
        rank = 1;
    }
    let idx = (rank - 1) as usize;
    if idx >= n {
        n - 1
    } else {
        idx
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// 错误枚举（16 变体 = 16 诊断码 · 每变体中文可操作诊断）
// ═══════════════════════════════════════════════════════════════════════════

/// 本模块统一错误（零 unwrap 纪律：一切可失败路径的返回载体）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum BootThError {
    /// 状态机乱序（期望相位 vs 实际相位）。
    PhaseViolation {
        /// 期望相位说明。
        expect: &'static str,
        /// 实际相位说明。
        got: &'static str,
    },
    /// 判定账次序号跳号（期望 vs 实到）。
    SeqGap {
        /// 期望的下一笔次序号。
        expect: u32,
        /// 实到次序号。
        got: u32,
    },
    /// 锚点时序非单调。
    AnchorDisorder {
        /// 违例锚点位（1=T1、2=T2、3=T3）。
        anchor: u8,
        /// 前锚时戳（us）。
        prev_us: u64,
        /// 违例锚时戳（us）。
        got_us: u64,
    },
    /// 守恒破坏（锚点差值总长 vs 申报总长）。
    ConservationBroken {
        /// 锚点差值总长（us）。
        computed_us: u64,
        /// 申报总长（us）。
        declared_us: u64,
    },
    /// 硬线解冻尝试被拒。
    ThawAttempt {
        /// 冻结硬线（ms）。
        frozen_ms: u64,
        /// 企图改写值（ms）。
        attempted_ms: u64,
    },
    /// 校准规程违规（F0665 对接面）。
    CalibInvalid {
        /// 违规步说明。
        step: &'static str,
    },
    /// 算术溢出/下溢（checked 运算拦截）。
    Overflow {
        /// 运算说明。
        op: &'static str,
    },
    /// 账簿容量越界。
    CapacityFull {
        /// 账簿名。
        what: &'static str,
        /// 容量。
        cap: usize,
    },
    /// 机号不符/未知机号。
    UnknownMachine {
        /// 实到机号码。
        code: u8,
    },
    /// 配置非法。
    ConfigInvalid {
        /// 字段名。
        field: &'static str,
        /// 原因。
        reason: &'static str,
    },
    /// 事件账序号断链。
    EventSeqBroken {
        /// 期望序号。
        expect: u64,
        /// 实到序号。
        got: u64,
    },
    /// 注入未检出（注入面复核失败）。
    InjectionUncaught {
        /// 注入种类说明。
        kind: &'static str,
    },
    /// 样本不足。
    InsufficientSamples {
        /// 现有样本数。
        have: usize,
        /// 需要样本数。
        need: usize,
    },
    /// 封存违规（封后写/重复封/未封聚合）。
    SealViolation {
        /// 相关次序号（0=非逐笔场景）。
        run_seq: u32,
    },
    /// 判定账行三要素不齐。
    RowIncomplete {
        /// 相关次序号。
        run_seq: u32,
        /// 缺失要素说明。
        missing: &'static str,
    },
    /// 重复登记（同次序号二次入账）。
    DuplicateRun {
        /// 已在账次序号。
        have: u32,
    },
}

impl BootThError {
    /// 中文可操作诊断（逐变体）。
    pub fn detail(&self) -> &'static str {
        match self {
            BootThError::PhaseViolation { .. } => {
                "相位乱序：判定账须按 开账→录入(满 50 自动封)→聚合核 顺序；校准须按 \
                 采样→统计→冻结提案 顺序。请检查调用序列，勿跳步或回退。"
            }
            BootThError::SeqGap { .. } => {
                "次序号跳号：判定账首笔次序号必须为 1，此后每笔必须前笔+1。\
                 请按次序逐笔补录，勿跳号勿漏号。"
            }
            BootThError::AnchorDisorder { .. } => {
                "锚点乱序：T0..T3 四锚时戳必须非降。请回查固件交接/早传完成/内核主入口/\
                 用户面首帧四锚采样点，修正后重录。"
            }
            BootThError::ConservationBroken { .. } => {
                "守恒破坏：Σ三段(T0T1+T1T2+T2T3) 必须恒等于 T0→T3 总时长，且与申报总长\
                 （F0198 时间线对冲位）一致。请核对锚点时戳与申报来源，差异走原批勘误。"
            }
            BootThError::ThawAttempt { .. } => {
                "硬线解冻被拒：3,000ms 为冻结硬线，禁止直接改写。变更唯一通道 = \
                 F0665 校准规程三步成型提案 + ADR 审批。本操作已立案。"
            }
            BootThError::CalibInvalid { .. } => {
                "校准规程违规：F0665 对接面须 采样(≥20 笔)→统计(P95×4/3 提案)→冻结提案\
                 依序推进；且提案只可经 ADR 落地，本模块不提供任何改写通道。"
            }
            BootThError::Overflow { .. } => {
                "算术溢出：时戳/耗时运算越界（多因锚点被篡改导致段宽下溢）。\
                 请回查样本完整性，必要时重建测量账。"
            }
            BootThError::CapacityFull { .. } => {
                "账簿满容：判定账固定 50 笔（判据口径），校准采样上限 256 笔。\
                 满账请走聚合核收口，勿超录。"
            }
            BootThError::UnknownMachine { .. } => {
                "机号不符：样本机号与账本机号不一致，或机号码非 0..2。\
                 UNX 三机矩阵仅 M0/M1/M2，请核对机号后重投。"
            }
            BootThError::ConfigInvalid { .. } => {
                "配置非法：run_count 必须恰为 50（判据冻结）、hard_limit_ms 必须恰为 \
                 3,000（硬线冻结）、容量须在许可域。请改用 BootThConfig::standard()。"
            }
            BootThError::EventSeqBroken { .. } => {
                "事件账断链：事件窗口序号必须逐笔连续。请审计事件账是否被外部篡改，\
                 篡改即账面完整性红灯。"
            }
            BootThError::InjectionUncaught { .. } => {
                "注入未检出：注入面复核失败（注入未产生预期拦截/立案）。\
                 请立刻复盘判定流水线，此为判据可信性红灯。"
            }
            BootThError::InsufficientSamples { .. } => {
                "样本不足：五数概括需 ≥1 笔、校准统计需 ≥20 笔。\
                 请先补齐样本再重试统计。"
            }
            BootThError::SealViolation { .. } => {
                "封存违规：满 50 笔自动封账，封账后禁写；未封账禁走聚合核。\
                 请检查账本相位后重试。"
            }
            BootThError::RowIncomplete { .. } => {
                "账行三要素不齐：次序号/耗时/结论三要素必须齐备，且结论必须与耗时\
                 按硬线重判一致。请回查账行生成路径，防虚记。"
            }
            BootThError::DuplicateRun { .. } => {
                "重复登记：同一次序号已入账，禁止二次入账。若需重测请使用新的\
                 引导次序号续录。"
            }
        }
    }

    /// 诊断码（0x1FF0..0x1FFF 段内一码一义）。
    pub fn diag_code(&self) -> u16 {
        match self {
            BootThError::PhaseViolation { .. } => D_PHASE,
            BootThError::SeqGap { .. } => D_SEQ_GAP,
            BootThError::AnchorDisorder { .. } => D_ANCHOR_DISORDER,
            BootThError::ConservationBroken { .. } => D_CONSERVATION,
            BootThError::ThawAttempt { .. } => D_THAW,
            BootThError::CalibInvalid { .. } => D_CALIB_INVALID,
            BootThError::Overflow { .. } => D_OVERFLOW,
            BootThError::CapacityFull { .. } => D_CAPACITY,
            BootThError::UnknownMachine { .. } => D_UNKNOWN_MACHINE,
            BootThError::ConfigInvalid { .. } => D_CONFIG_INVALID,
            BootThError::EventSeqBroken { .. } => D_EVENT_SEQ,
            BootThError::InjectionUncaught { .. } => D_INJECTION_UNCAUGHT,
            BootThError::InsufficientSamples { .. } => D_INSUFFICIENT,
            BootThError::SealViolation { .. } => D_SEAL,
            BootThError::RowIncomplete { .. } => D_ROW_INCOMPLETE,
            BootThError::DuplicateRun { .. } => D_DUPLICATE,
        }
    }

    /// 短名（日志/报告紧凑用）。
    pub fn short(&self) -> &'static str {
        match self {
            BootThError::PhaseViolation { .. } => "phase",
            BootThError::SeqGap { .. } => "seq_gap",
            BootThError::AnchorDisorder { .. } => "anchor_disorder",
            BootThError::ConservationBroken { .. } => "conservation",
            BootThError::ThawAttempt { .. } => "thaw",
            BootThError::CalibInvalid { .. } => "calib",
            BootThError::Overflow { .. } => "overflow",
            BootThError::CapacityFull { .. } => "capacity",
            BootThError::UnknownMachine { .. } => "machine",
            BootThError::ConfigInvalid { .. } => "config",
            BootThError::EventSeqBroken { .. } => "event_seq",
            BootThError::InjectionUncaught { .. } => "inj_uncaught",
            BootThError::InsufficientSamples { .. } => "few_samples",
            BootThError::SealViolation { .. } => "seal",
            BootThError::RowIncomplete { .. } => "row_incomplete",
            BootThError::DuplicateRun { .. } => "duplicate",
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// 机号 / 锚点 / 段种 / 判定结论 / 健康 / 相位（基础枚举族）
// ═══════════════════════════════════════════════════════════════════════════

/// 三机矩阵机号。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MachineId {
    /// 机 0。
    M0,
    /// 机 1。
    M1,
    /// 机 2。
    M2,
}

impl MachineId {
    /// 机号码（0/1/2）。
    pub fn code(self) -> u8 {
        match self {
            MachineId::M0 => 0,
            MachineId::M1 => 1,
            MachineId::M2 => 2,
        }
    }

    /// 按下标解机号（0..2 外得 `None`）。
    pub fn from_index(i: usize) -> Option<MachineId> {
        match i {
            0 => Some(MachineId::M0),
            1 => Some(MachineId::M1),
            2 => Some(MachineId::M2),
            _ => Option::None,
        }
    }

    /// 按机号码解机号（非法码得 `None` → `UnknownMachine`）。
    pub fn from_code(code: u8) -> Option<MachineId> {
        MachineId::from_index(code as usize)
    }

    /// 中文标签。
    pub fn label(self) -> &'static str {
        match self {
            MachineId::M0 => "机 0",
            MachineId::M1 => "机 1",
            MachineId::M2 => "机 2",
        }
    }
}

/// 冷启动四锚点（T0..T3）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BootAnchor {
    /// T0 固件交接。
    T0,
    /// T1 早传完成。
    T1,
    /// T2 内核主入口。
    T2,
    /// T3 用户面首帧。
    T3,
}

impl BootAnchor {
    /// 锚点下标（0..3）。
    pub fn index(self) -> usize {
        match self {
            BootAnchor::T0 => 0,
            BootAnchor::T1 => 1,
            BootAnchor::T2 => 2,
            BootAnchor::T3 => 3,
        }
    }

    /// 按下标解锚点。
    pub fn from_index(i: usize) -> Option<BootAnchor> {
        match i {
            0 => Some(BootAnchor::T0),
            1 => Some(BootAnchor::T1),
            2 => Some(BootAnchor::T2),
            3 => Some(BootAnchor::T3),
            _ => Option::None,
        }
    }

    /// 中文全名（本模块执行面命名）。
    pub fn label(self) -> &'static str {
        match self {
            BootAnchor::T0 => "T0 固件交接",
            BootAnchor::T1 => "T1 早传完成",
            BootAnchor::T2 => "T2 内核主入口",
            BootAnchor::T3 => "T3 用户面首帧",
        }
    }

    /// 短名。
    pub fn short(self) -> &'static str {
        match self {
            BootAnchor::T0 => "T0",
            BootAnchor::T1 => "T1",
            BootAnchor::T2 => "T2",
            BootAnchor::T3 => "T3",
        }
    }
}

/// 三段种（T0T1/T1T2/T2T3）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SegmentKind {
    /// 固件段 T0→T1。
    FirmwareT0T1,
    /// 早传段 T1→T2。
    EarlyXferT1T2,
    /// 内核段 T2→T3。
    KernelT2T3,
}

impl SegmentKind {
    /// 段下标（0..2）。
    pub fn index(self) -> usize {
        match self {
            SegmentKind::FirmwareT0T1 => 0,
            SegmentKind::EarlyXferT1T2 => 1,
            SegmentKind::KernelT2T3 => 2,
        }
    }

    /// 按下标解段种。
    pub fn from_index(i: usize) -> Option<SegmentKind> {
        match i {
            0 => Some(SegmentKind::FirmwareT0T1),
            1 => Some(SegmentKind::EarlyXferT1T2),
            2 => Some(SegmentKind::KernelT2T3),
            _ => Option::None,
        }
    }

    /// 中文标签（归因类名）。
    pub fn label(self) -> &'static str {
        match self {
            SegmentKind::FirmwareT0T1 => "固件段 T0T1",
            SegmentKind::EarlyXferT1T2 => "早传段 T1T2",
            SegmentKind::KernelT2T3 => "内核段 T2T3",
        }
    }

    /// 短名。
    pub fn short(self) -> &'static str {
        match self {
            SegmentKind::FirmwareT0T1 => "T0T1",
            SegmentKind::EarlyXferT1T2 => "T1T2",
            SegmentKind::KernelT2T3 => "T2T3",
        }
    }
}

/// 判定结论（通过带余量 / 拦截带超额，毫秒整数核算）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Verdict {
    /// 通过（距硬线余量 ms）。
    Pass {
        /// 余量（ms）。
        slack_ms: u64,
    },
    /// 拦截（超硬线额度 ms，向上取整到 ms）。
    Intercept {
        /// 超额（ms）。
        over_ms: u64,
    },
}

impl Verdict {
    /// 是否通过。
    pub fn is_pass(self) -> bool {
        match self {
            Verdict::Pass { .. } => true,
            Verdict::Intercept { .. } => false,
        }
    }

    /// 中文标签。
    pub fn label(self) -> &'static str {
        match self {
            Verdict::Pass { .. } => "通过",
            Verdict::Intercept { .. } => "拦截",
        }
    }

    /// 余量（拦截时为 0）。
    pub fn slack_ms(self) -> u64 {
        match self {
            Verdict::Pass { slack_ms } => slack_ms,
            Verdict::Intercept { .. } => 0,
        }
    }

    /// 超额（通过时为 0）。
    pub fn over_ms(self) -> u64 {
        match self {
            Verdict::Pass { .. } => 0,
            Verdict::Intercept { over_ms } => over_ms,
        }
    }

    /// 指纹折叠（变体码 + 量值）。
    pub fn fold(self, h: u64) -> u64 {
        match self {
            Verdict::Pass { slack_ms } => {
                fnv1a64_from(fnv1a64_from(h, &[0x01]), &slack_ms.to_le_bytes())
            }
            Verdict::Intercept { over_ms } => {
                fnv1a64_from(fnv1a64_from(h, &[0x02]), &over_ms.to_le_bytes())
            }
        }
    }
}

/// 三色健康度。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Health {
    /// 绿：封账 + 50/50 全过 + 零案件 + 账面完整。
    Green,
    /// 黄：未封账 / 存在非红灯案件。
    Amber,
    /// 红：守恒破坏 / 解冻尝试 / 注入未检出 / 事件断链 / 硬线完整性失守。
    Red,
}

impl Health {
    /// 等级（0=绿 1=黄 2=红）。
    pub fn rank(self) -> u8 {
        match self {
            Health::Green => 0,
            Health::Amber => 1,
            Health::Red => 2,
        }
    }

    /// 中文标签。
    pub fn label(self) -> &'static str {
        match self {
            Health::Green => "绿",
            Health::Amber => "黄",
            Health::Red => "红",
        }
    }
}

/// 引擎相位（开账即录入；满 50 自动封）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EnginePhase {
    /// 录入中。
    Recording,
    /// 已封账（满 50）。
    Sealed,
}

impl EnginePhase {
    /// 相位码。
    pub fn code(self) -> u8 {
        match self {
            EnginePhase::Recording => 1,
            EnginePhase::Sealed => 2,
        }
    }

    /// 中文标签。
    pub fn label(self) -> &'static str {
        match self {
            EnginePhase::Recording => "录入中",
            EnginePhase::Sealed => "已封账",
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// 子系统 1：BootSpan —— 冷启动四锚点测量账（段分解 + 守恒核 + 主导段）
// ═══════════════════════════════════════════════════════════════════════════

/// 冷启动 T0→T3 测量账：四锚点微秒时戳 + 三段分解 + 申报总长对冲位。
///
/// 守恒恒等式（结构性）：`seg(T0T1) + seg(T1T2) + seg(T2T3) ≡ T3 − T0`
/// （三段由相邻锚点差直接给出，代数恒等）；另设 `declared_total_us` 独立申报位
/// （来源：F0198 时间线对冲字段），录入时由引擎强制 `total_us() == declared_total_us`，
/// 不符即 `ConservationBroken` 拦截立案——对冲核注入面即篡改此位。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BootSpan {
    machine: MachineId,
    run_seq: u32,
    /// 四锚时戳（us，构造时强制非降）。
    t_us: [u64; ANCHOR_COUNT],
    /// 独立申报总时长（us；F0198 对冲位）。
    declared_total_us: u64,
}

impl BootSpan {
    /// 构造（锚点非降核 + 次序号在位核；守恒对冲留待录入引擎强制）。
    pub fn new(
        machine: MachineId,
        run_seq: u32,
        t_us: [u64; ANCHOR_COUNT],
        declared_total_us: u64,
    ) -> Result<BootSpan, BootThError> {
        if run_seq == 0 {
            return Err(BootThError::RowIncomplete {
                run_seq,
                missing: "次序号缺位（必须 ≥1）",
            });
        }
        let mut i = 1usize;
        while i < ANCHOR_COUNT {
            if t_us[i] < t_us[i - 1] {
                return Err(BootThError::AnchorDisorder {
                    anchor: i as u8,
                    prev_us: t_us[i - 1],
                    got_us: t_us[i],
                });
            }
            i += 1;
        }
        Ok(BootSpan {
            machine,
            run_seq,
            t_us,
            declared_total_us,
        })
    }

    /// 机号。
    pub fn machine(&self) -> MachineId {
        self.machine
    }

    /// 次序号。
    pub fn run_seq(&self) -> u32 {
        self.run_seq
    }

    /// 四锚时戳副本。
    pub fn anchors_us(&self) -> [u64; ANCHOR_COUNT] {
        self.t_us
    }

    /// 单锚时戳。
    pub fn anchor_us(&self, a: BootAnchor) -> u64 {
        self.t_us[a.index()]
    }

    /// 段耗时（us；锚点非降不变式下差值非负，显式 saturating 防篡改后下溢静默）。
    pub fn seg_us(&self, k: SegmentKind) -> u64 {
        let i = k.index();
        self.t_us[i + 1].saturating_sub(self.t_us[i])
    }

    /// 段耗时（us；checked 版——锚点被篡改致下溢时显式 `Overflow`）。
    pub fn seg_us_strict(&self, k: SegmentKind) -> Result<u64, BootThError> {
        let i = k.index();
        match self.t_us[i + 1].checked_sub(self.t_us[i]) {
            Some(v) => Ok(v),
            Option::None => Err(BootThError::Overflow {
                op: "段耗时 = 上锚 − 下锚（下溢）",
            }),
        }
    }

    /// 三段耗时（us；checked 三连——录入流水线用）。
    pub fn seg_triple_strict(&self) -> Result<[u64; SEGMENT_COUNT], BootThError> {
        Ok([
            self.seg_us_strict(SegmentKind::FirmwareT0T1)?,
            self.seg_us_strict(SegmentKind::EarlyXferT1T2)?,
            self.seg_us_strict(SegmentKind::KernelT2T3)?,
        ])
    }

    /// 段耗时（ms，账面展示口径：us/1000 向下取整）。
    pub fn seg_ms(&self, k: SegmentKind) -> u64 {
        self.seg_us(k) / US_PER_MS
    }

    /// T0→T3 总时长（us；非降不变式下 saturating）。
    pub fn total_us(&self) -> u64 {
        self.t_us[3].saturating_sub(self.t_us[0])
    }

    /// T0→T3 总时长（ms，账面展示口径）。
    pub fn total_ms(&self) -> u64 {
        self.total_us() / US_PER_MS
    }

    /// 申报总时长（us；F0198 对冲位）。
    pub fn declared_total_us(&self) -> u64 {
        self.declared_total_us
    }

    /// 锚点非降复核（防御纵深：构造核之外可再验，锚点篡改即失守）。
    pub fn anchors_monotonic(&self) -> bool {
        let mut i = 1usize;
        while i < ANCHOR_COUNT {
            if self.t_us[i] < self.t_us[i - 1] {
                return false;
            }
            i += 1;
        }
        true
    }

    /// 守恒核：锚点差值总长 ≡ 申报总长（对冲恒等）。
    pub fn conservation_ok(&self) -> bool {
        self.total_us() == self.declared_total_us
    }

    /// 三段求和守恒展示值（结构性恒等于 `total_us()`；供对账渲染）。
    pub fn seg_sum_us(&self) -> u64 {
        self.seg_us(SegmentKind::FirmwareT0T1)
            .saturating_add(self.seg_us(SegmentKind::EarlyXferT1T2))
            .saturating_add(self.seg_us(SegmentKind::KernelT2T3))
    }

    /// 主导段（最大段；并列取最早段——确定性归因规则）。
    pub fn dominant_segment(&self) -> SegmentKind {
        let a = self.seg_us(SegmentKind::FirmwareT0T1);
        let b = self.seg_us(SegmentKind::EarlyXferT1T2);
        let c = self.seg_us(SegmentKind::KernelT2T3);
        if a >= b && a >= c {
            SegmentKind::FirmwareT0T1
        } else if b >= c {
            SegmentKind::EarlyXferT1T2
        } else {
            SegmentKind::KernelT2T3
        }
    }

    /// 全量指纹（机号/次序号/四锚/申报位链式 FNV）。
    pub fn fingerprint(&self) -> u64 {
        let mut h = fnv1a64_from(FNV_OFFSET, BOOTTH_VERSION.as_bytes());
        h = fnv1a64_from(h, &[self.machine.code()]);
        h = fnv1a64_from(h, &self.run_seq.to_le_bytes());
        let mut i = 0usize;
        while i < ANCHOR_COUNT {
            h = fnv1a64_from(h, &self.t_us[i].to_le_bytes());
            i += 1;
        }
        fnv1a64_from(h, &self.declared_total_us.to_le_bytes())
    }

    /// 注入面专用：篡改申报总长（破坏守恒对冲位——必被录入流水线检出）。
    pub fn tamper_declared_total(&mut self, new_total_us: u64) {
        self.declared_total_us = new_total_us;
    }

    /// 注入面专用：篡改单锚时戳（破坏非降序——必被锚点序核检出）。
    pub fn tamper_anchor(&mut self, a: BootAnchor, new_us: u64) {
        self.t_us[a.index()] = new_us;
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// 合成画像（确定性采样源 · 常量冻结 + 编译期安全带核）
// ═══════════════════════════════════════════════════════════════════════════

/// 合成引导画像（六档：三名义档全在硬线内、三超阈档各主导一段）。
///
/// 段基线（ms）与抖动幅（±ms）写死冻结；名义档最坏和 < 硬线（安全带 ≥80ms）、
/// 超阈档最乐观和 > 硬线、超阈档主导段下界 > 非主导段上界——三者皆编译期断言锁定。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SpanProfile {
    /// 名义快档：基线 [340, 290, 520]，抖动 ±120。
    NominalTight,
    /// 名义中档：基线 [610, 520, 880]，抖动 ±130。
    NominalMid,
    /// 名义高档：基线 [850, 760, 1130]，抖动 ±60（安全带 80ms）。
    NominalHigh,
    /// 超阈·固件主导：基线 [4180, 520, 830]，抖动 ±80。
    OverFirmware,
    /// 超阈·早传主导：基线 [700, 3890, 770]，抖动 ±80。
    OverEarlyXfer,
    /// 超阈·内核主导：基线 [720, 610, 4470]，抖动 ±80。
    OverKernel,
}

impl SpanProfile {
    /// 画像码（1..6）。
    pub fn code(self) -> u8 {
        match self {
            SpanProfile::NominalTight => 1,
            SpanProfile::NominalMid => 2,
            SpanProfile::NominalHigh => 3,
            SpanProfile::OverFirmware => 4,
            SpanProfile::OverEarlyXfer => 5,
            SpanProfile::OverKernel => 6,
        }
    }

    /// 中文标签。
    pub fn label(self) -> &'static str {
        match self {
            SpanProfile::NominalTight => "名义快档",
            SpanProfile::NominalMid => "名义中档",
            SpanProfile::NominalHigh => "名义高档",
            SpanProfile::OverFirmware => "超阈·固件主导",
            SpanProfile::OverEarlyXfer => "超阈·早传主导",
            SpanProfile::OverKernel => "超阈·内核主导",
        }
    }

    /// 是否超阈档。
    pub fn is_over(self) -> bool {
        match self {
            SpanProfile::NominalTight | SpanProfile::NominalMid | SpanProfile::NominalHigh => false,
            SpanProfile::OverFirmware | SpanProfile::OverEarlyXfer | SpanProfile::OverKernel => true,
        }
    }

    /// 主导段下标（超阈档用）。
    pub const fn dominant_index(self) -> usize {
        match self {
            SpanProfile::OverFirmware => 0,
            SpanProfile::OverEarlyXfer => 1,
            SpanProfile::OverKernel => 2,
            _ => 0,
        }
    }

    /// 段基线（ms；const fn · match 直构数组）。
    pub const fn base_seg_ms(self) -> [u64; SEGMENT_COUNT] {
        match self {
            SpanProfile::NominalTight => [340, 290, 520],
            SpanProfile::NominalMid => [610, 520, 880],
            SpanProfile::NominalHigh => [850, 760, 1130],
            SpanProfile::OverFirmware => [4180, 520, 830],
            SpanProfile::OverEarlyXfer => [700, 3890, 770],
            SpanProfile::OverKernel => [720, 610, 4470],
        }
    }

    /// 抖动幅（±ms；const fn）。
    pub const fn jitter_ms(self) -> u64 {
        match self {
            SpanProfile::NominalTight => 120,
            SpanProfile::NominalMid => 130,
            SpanProfile::NominalHigh => 60,
            SpanProfile::OverFirmware | SpanProfile::OverEarlyXfer | SpanProfile::OverKernel => 80,
        }
    }

    /// 最坏总耗时（Σ基线 + 3×抖动；const fn · 逐段直加）。
    pub const fn worst_total_ms(self) -> u64 {
        let b = self.base_seg_ms();
        let j = self.jitter_ms();
        b[0] + b[1] + b[2] + 3 * j
    }

    /// 最乐观总耗时（Σ基线 − 3×抖动；const fn）。
    pub const fn floor_total_ms(self) -> u64 {
        let b = self.base_seg_ms();
        let j = self.jitter_ms();
        b[0] + b[1] + b[2] - 3 * j
    }

    /// 主导段确定性核（主导段下界 > 任一非主导段上界；const fn · while+索引）。
    pub const fn dominance_guaranteed(self) -> bool {
        let b = self.base_seg_ms();
        let j = self.jitter_ms();
        let d = self.dominant_index();
        let mut k = 0usize;
        let mut ok = true;
        while k < 3 {
            if k != d {
                if b[d] - j <= b[k] + j {
                    ok = false;
                }
            }
            k += 1;
        }
        ok
    }
}

/// 合成单次引导测量账（同画像同 RNG 状态同输出——确定性零随机外泄）。
///
/// 段耗时 = 基线 ± 抖动（LCG 决定，逐段独立）；T0 基线 + 次序号×步进错峰；
/// 申报总长 = 锚点差值（对冲位初值自洽）。
pub fn synth_span(
    machine: MachineId,
    run_seq: u32,
    profile: SpanProfile,
    rng: &mut Lcg,
) -> Result<BootSpan, BootThError> {
    let base = profile.base_seg_ms();
    let jmax = profile.jitter_ms();
    let mut segs = [0u64; SEGMENT_COUNT];
    let mut i = 0usize;
    while i < SEGMENT_COUNT {
        let d = rng.below(jmax * 2 + 1);
        let jit = d as i64 - jmax as i64;
        let v = base[i] as i64 + jit;
        // 基线远大于抖动，理论不触钳；显式钳 1ms 防御性保正。
        let v2 = if v < 1 { 1 } else { v };
        segs[i] = (v2 as u64).saturating_mul(US_PER_MS);
        i += 1;
    }
    let t0 = SYNTH_T0_BASE_US.saturating_add((run_seq as u64).saturating_mul(SYNTH_T0_STEP_US));
    let t1 = t0.saturating_add(segs[0]);
    let t2 = t1.saturating_add(segs[1]);
    let t3 = t2.saturating_add(segs[2]);
    let declared = t3.saturating_sub(t0);
    BootSpan::new(machine, run_seq, [t0, t1, t2, t3], declared)
}

// ═══════════════════════════════════════════════════════════════════════════
// 子系统 2：ThresholdLine —— 3,000ms 硬线冻结账（哈希封存 + 解冻恒拒）
// ═══════════════════════════════════════════════════════════════════════════

/// 硬线冻结账：限值 + 来源串双料 FNV 封存哈希。
///
/// 变更纪律（F0665-J1）：冻结后阈值表哈希封存且变更走 ADR——本类型不提供任何
/// 改值通道；`attempt_direct_change` 恒拒；落地改写归 F0665 校准规程（本模块
/// 仅 `CalibrationGate` 对接声明）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ThresholdLine {
    limit_ms: u64,
    frozen_hash: u64,
    provenance: &'static str,
}

impl ThresholdLine {
    /// 冻结硬线（const 构造；封存哈希 = FNV(来源串 ‖ 限值 LE 字节)）。
    pub const fn frozen() -> ThresholdLine {
        let h0 = fnv1a64_from(FNV_OFFSET, LINE_PROVENANCE.as_bytes());
        let h1 = fnv1a64_from(h0, &HARD_LIMIT_MS.to_le_bytes());
        ThresholdLine {
            limit_ms: HARD_LIMIT_MS,
            frozen_hash: h1,
            provenance: LINE_PROVENANCE,
        }
    }

    /// 限值（ms）。
    pub fn limit_ms(&self) -> u64 {
        self.limit_ms
    }

    /// 限值（us，判定域）。
    pub fn limit_us(&self) -> u64 {
        self.limit_ms.saturating_mul(US_PER_MS)
    }

    /// 封存哈希。
    pub fn frozen_hash(&self) -> u64 {
        self.frozen_hash
    }

    /// 来源串。
    pub fn provenance(&self) -> &'static str {
        self.provenance
    }

    /// 完整性复核（重算封存哈希比对——篡改限值/来源即失守）。
    pub fn verify_integrity(&self) -> bool {
        let h0 = fnv1a64_from(FNV_OFFSET, self.provenance.as_bytes());
        let h1 = fnv1a64_from(h0, &self.limit_ms.to_le_bytes());
        h1 == self.frozen_hash
    }

    /// 指纹（即封存哈希）。
    pub fn fingerprint(&self) -> u64 {
        self.frozen_hash
    }

    /// 直接改写恒拒（解冻纪律 0 通道；调用方应同时落 ThawAttempt 案）。
    pub fn attempt_direct_change(&self, new_ms: u64) -> Result<ThresholdLine, BootThError> {
        let _ = new_ms;
        Err(BootThError::ThawAttempt {
            frozen_ms: self.limit_ms,
            attempted_ms: new_ms,
        })
    }

    /// 注入面专用：篡改限值（完整性复核必失守 → 健康度红灯）。
    pub fn tamper_limit(&mut self, new_ms: u64) {
        self.limit_ms = new_ms;
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// 子系统 3：ThresholdJudge + JudgeRow —— 判定器与账行三要素
// ═══════════════════════════════════════════════════════════════════════════

/// 阈值判定器（纯函数式：只读硬线，无内部状态；判定域 = 微秒）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ThresholdJudge {
    limit_ms: u64,
}

impl ThresholdJudge {
    /// 从冻结硬线构建（限值必须恰为 HARD_LIMIT_MS——防夹带私改线）。
    pub fn from_line(line: &ThresholdLine) -> Result<ThresholdJudge, BootThError> {
        if line.limit_ms != HARD_LIMIT_MS {
            return Err(BootThError::ConfigInvalid {
                field: "judge.limit_ms",
                reason: "判定器只接受冻结硬线 3,000ms（变更走 F0665 校准规程+ADR）",
            });
        }
        if !line.verify_integrity() {
            return Err(BootThError::ConfigInvalid {
                field: "judge.line",
                reason: "硬线封存哈希复核失败（疑似篡改）——禁用该判定器",
            });
        }
        Ok(ThresholdJudge {
            limit_ms: line.limit_ms,
        })
    }

    /// 硬线（ms）。
    pub fn limit_ms(&self) -> u64 {
        self.limit_ms
    }

    /// 硬线（us）。
    pub fn limit_us(&self) -> u64 {
        self.limit_ms.saturating_mul(US_PER_MS)
    }

    /// 微秒域判定（≤ 硬线 us → 通过带余量；超 → 拦截带超额，超额向上取整到 ms）。
    pub fn judge_us(&self, total_us: u64) -> Verdict {
        let limit_us = self.limit_us();
        if total_us <= limit_us {
            Verdict::Pass {
                slack_ms: (limit_us - total_us) / US_PER_MS,
            }
        } else {
            let over_us = total_us - limit_us;
            Verdict::Intercept {
                over_ms: (over_us + US_PER_MS - 1) / US_PER_MS,
            }
        }
    }

    /// 测量账判定。
    pub fn judge_span(&self, span: &BootSpan) -> Verdict {
        self.judge_us(span.total_us())
    }

    /// 账行三要素产出（次序号 / 耗时 ms / 结论）。
    pub fn row_for(&self, span: &BootSpan) -> JudgeRow {
        JudgeRow {
            run_seq: span.run_seq(),
            total_ms: span.total_ms(),
            verdict: self.judge_span(span),
        }
    }

    /// 拦截检出率（万分比；分母 0 得 10,000 约定空账全过）。
    pub fn interception_rate_bp(passes: u64, intercepts: u64) -> u64 {
        let total = passes.saturating_add(intercepts);
        if total == 0 {
            return 10_000;
        }
        intercepts.saturating_mul(10_000) / total
    }

    /// 拦截检出 100% 判据核（纯样本复核：样本集内每一笔 > 硬线的必须判拦截）。
    pub fn verify_intercept_full(&self, samples_us: &[u64]) -> bool {
        let limit_us = self.limit_us();
        let mut i = 0usize;
        while i < samples_us.len() {
            let over = samples_us[i] > limit_us;
            let judged_intercept = !self.judge_us(samples_us[i]).is_pass();
            if over != judged_intercept {
                return false;
            }
            i += 1;
        }
        true
    }
}

/// 判定账行三要素（次序号 / 耗时 / 结论）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct JudgeRow {
    /// 次序号（≥1 连续）。
    pub run_seq: u32,
    /// T0→T3 耗时（ms，账面展示口径）。
    pub total_ms: u64,
    /// 结论（通过/拦截）。
    pub verdict: Verdict,
}

impl JudgeRow {
    /// 三要素在位核（次序号 ≥1 且耗时 ≥1ms；结论为值类型必在位）。
    pub fn triple_complete(&self) -> bool {
        self.run_seq >= 1 && self.total_ms >= 1
    }

    /// 结论复核核：按硬线对耗时重判，必须与账载结论一致（防虚记）。
    /// `total_us` 为该笔精确微秒值（账行只存 ms，复核须用 us 精确域）。
    pub fn verify_against_us(&self, limit_ms: u64, total_us: u64) -> Result<(), BootThError> {
        if self.run_seq == 0 {
            return Err(BootThError::RowIncomplete {
                run_seq: 0,
                missing: "次序号缺位",
            });
        }
        if self.total_ms == 0 {
            return Err(BootThError::RowIncomplete {
                run_seq: self.run_seq,
                missing: "耗时缺位（0ms 非法）",
            });
        }
        let limit_us = limit_ms.saturating_mul(US_PER_MS);
        let rejudged = if total_us <= limit_us {
            Verdict::Pass {
                slack_ms: (limit_us - total_us) / US_PER_MS,
            }
        } else {
            Verdict::Intercept {
                over_ms: (total_us - limit_us + US_PER_MS - 1) / US_PER_MS,
            }
        };
        if rejudged != self.verdict {
            return Err(BootThError::RowIncomplete {
                run_seq: self.run_seq,
                missing: "结论与耗时按硬线重判不一致（疑似虚记）",
            });
        }
        Ok(())
    }

    /// 指纹。
    pub fn fingerprint(&self) -> u64 {
        let mut h = fnv1a64_from(FNV_OFFSET, &[0xA1]);
        h = fnv1a64_from(h, &self.run_seq.to_le_bytes());
        h = fnv1a64_from(h, &self.total_ms.to_le_bytes());
        self.verdict.fold(h)
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// 子系统 4：RunRecord + FiftyRunBook —— 50 次判定账
// ═══════════════════════════════════════════════════════════════════════════

/// 逐次记录（账行三要素 + 段分解 + 精确耗时 + 守恒旗标 + 归因位）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RunRecord {
    /// 判定账行（三要素）。
    pub row: JudgeRow,
    /// 机号。
    pub machine: MachineId,
    /// 三段耗时（us，精确域）。
    pub seg_us: [u64; SEGMENT_COUNT],
    /// T0→T3 精确耗时（us——结论复核域）。
    pub total_us_exact: u64,
    /// 申报总长（us，对冲位留痕）。
    pub declared_total_us: u64,
    /// 测量账指纹。
    pub span_fingerprint: u64,
    /// 守恒核结果（录入时已强制 true 方可入账）。
    pub conservation_ok: bool,
    /// 超阈归因位（通过为 `None`；拦截为主导段）。
    pub attributed: Option<SegmentKind>,
}

impl RunRecord {
    /// 从测量账 + 账行 + 归因位组装（段宽 checked 三连，下溢即 `Overflow`）。
    pub fn from_span(
        span: &BootSpan,
        row: JudgeRow,
        attributed: Option<SegmentKind>,
    ) -> Result<RunRecord, BootThError> {
        let segs = span.seg_triple_strict()?;
        Ok(RunRecord {
            row,
            machine: span.machine(),
            seg_us: segs,
            total_us_exact: span.total_us(),
            declared_total_us: span.declared_total_us(),
            span_fingerprint: span.fingerprint(),
            conservation_ok: span.conservation_ok(),
            attributed,
        })
    }

    /// 指纹。
    pub fn fingerprint(&self) -> u64 {
        let mut h = self.row.fingerprint();
        h = fnv1a64_from(h, &[self.machine.code()]);
        h = fnv1a64_from(h, &self.seg_us[0].to_le_bytes());
        h = fnv1a64_from(h, &self.seg_us[1].to_le_bytes());
        h = fnv1a64_from(h, &self.seg_us[2].to_le_bytes());
        h = fnv1a64_from(h, &self.total_us_exact.to_le_bytes());
        h = fnv1a64_from(h, &self.declared_total_us.to_le_bytes());
        h = fnv1a64_from(h, &self.span_fingerprint.to_le_bytes());
        let cons_bit: u8 = if self.conservation_ok { 1 } else { 0 };
        h = fnv1a64_from(h, &[cons_bit]);
        match self.attributed {
            Option::None => fnv1a64_from(h, &[0x00]),
            Option::Some(k) => fnv1a64_from(h, &[0x01, k.index() as u8 + 1]),
        }
    }
}

/// 50 次判定账（次序号连续零跳号 + 双口径对账 + 封存守卫）。
pub struct FiftyRunBook {
    machine: MachineId,
    cap: usize,
    records: Vec<RunRecord>,
    sealed: bool,
}

impl FiftyRunBook {
    /// 构造（cap ≥1）。
    pub fn new(machine: MachineId, cap: usize) -> Result<FiftyRunBook, BootThError> {
        if cap == 0 {
            return Err(BootThError::ConfigInvalid {
                field: "book.cap",
                reason: "判定账容量必须 ≥1",
            });
        }
        Ok(FiftyRunBook {
            machine,
            cap,
            records: Vec::new(),
            sealed: false,
        })
    }

    /// 账本机号。
    pub fn machine(&self) -> MachineId {
        self.machine
    }

    /// 容量。
    pub fn cap(&self) -> usize {
        self.cap
    }

    /// 在账笔数。
    pub fn len(&self) -> usize {
        self.records.len()
    }

    /// 空账？
    pub fn is_empty(&self) -> bool {
        self.records.is_empty()
    }

    /// 满账？
    pub fn is_complete(&self) -> bool {
        self.records.len() >= self.cap
    }

    /// 已封账？
    pub fn is_sealed(&self) -> bool {
        self.sealed
    }

    /// 全部记录（只读）。
    pub fn records(&self) -> &[RunRecord] {
        &self.records
    }

    /// 下一笔期望次序号（空账为 1；否则末笔+1）。
    pub fn next_expected_seq(&self) -> u32 {
        match self.records.last() {
            Option::None => 1,
            Option::Some(r) => r.row.run_seq.saturating_add(1),
        }
    }

    /// 入账（封后禁写；首笔必 1；后笔必前笔+1；满容拒收）。
    pub fn record(&mut self, rec: RunRecord) -> Result<(), BootThError> {
        if self.sealed {
            return Err(BootThError::SealViolation {
                run_seq: rec.row.run_seq,
            });
        }
        if self.records.len() >= self.cap {
            return Err(BootThError::CapacityFull {
                what: "判定账（50 次判据口径）",
                cap: self.cap,
            });
        }
        let expect = self.next_expected_seq();
        let got = rec.row.run_seq;
        if got < expect {
            return Err(BootThError::DuplicateRun { have: got });
        }
        if got > expect {
            return Err(BootThError::SeqGap { expect, got });
        }
        self.records.push(rec);
        Ok(())
    }

    /// 手动封账（重复封 → `SealViolation`；引擎侧满 50 自动封不走此口）。
    pub fn seal(&mut self) -> Result<(), BootThError> {
        if self.sealed {
            return Err(BootThError::SealViolation { run_seq: 0 });
        }
        self.sealed = true;
        Ok(())
    }

    /// 引擎内部封账置位（不重复报错——只在未封时置位）。
    pub(crate) fn seal_quiet(&mut self) {
        self.sealed = true;
    }

    /// 次序号连续核（逐行扫描重算：首笔 1、逐笔 +1——防账面缓存漂移）。
    pub fn seq_continuous(&self) -> bool {
        let mut i = 0usize;
        while i < self.records.len() {
            let expect = if i == 0 { 1 } else { self.records[i - 1].row.run_seq + 1 };
            if self.records[i].row.run_seq != expect {
                return false;
            }
            i += 1;
        }
        true
    }

    /// 通过笔数（扫描重算口径）。
    pub fn pass_count(&self) -> u64 {
        let mut n = 0u64;
        let mut i = 0usize;
        while i < self.records.len() {
            if self.records[i].row.verdict.is_pass() {
                n = n.saturating_add(1);
            }
            i += 1;
        }
        n
    }

    /// 拦截笔数（扫描重算口径）。
    pub fn intercept_count(&self) -> u64 {
        let mut n = 0u64;
        let mut i = 0usize;
        while i < self.records.len() {
            if !self.records[i].row.verdict.is_pass() {
                n = n.saturating_add(1);
            }
            i += 1;
        }
        n
    }

    /// 50 次全过核（满 50 且零拦截）。
    pub fn all_pass(&self) -> bool {
        self.records.len() == self.cap && self.intercept_count() == 0
    }

    /// 耗时样本集（ms 口径，堆上新 Vec——五数概括输入）。
    pub fn ms_values(&self) -> Vec<u64> {
        let mut v = Vec::with_capacity(self.records.len());
        let mut i = 0usize;
        while i < self.records.len() {
            v.push(self.records[i].row.total_ms);
            i += 1;
        }
        v
    }

    /// 五数概括（空账 → `InsufficientSamples`）。
    pub fn summary(&self) -> Result<FiveNumSummary, BootThError> {
        FiveNumSummary::from_ms_samples(&self.ms_values())
    }

    /// 指纹（机号/容量/封旗 + 逐笔链式）。
    pub fn fingerprint(&self) -> u64 {
        let mut h = fnv1a64_from(FNV_OFFSET, &[self.machine.code()]);
        h = fnv1a64_from(h, &(self.cap as u64).to_le_bytes());
        let s: u8 = if self.sealed { 1 } else { 0 };
        h = fnv1a64_from(h, &[s]);
        let mut i = 0usize;
        while i < self.records.len() {
            h = fnv1a64_from(h, &self.records[i].fingerprint().to_le_bytes());
            i += 1;
        }
        h
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// 子系统 5：FiveNumSummary —— 五数概括 + P95（整数最近秩分位）
// ═══════════════════════════════════════════════════════════════════════════

/// 五数概括（min/P25/中位/P75/max）+ P95（全整数 ms，最近秩法）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FiveNumSummary {
    /// 样本数。
    pub n: usize,
    /// 最小值（ms）。
    pub min_ms: u64,
    /// P25（ms）。
    pub p25_ms: u64,
    /// 中位数（ms）。
    pub median_ms: u64,
    /// P75（ms）。
    pub p75_ms: u64,
    /// 最大值（ms）。
    pub max_ms: u64,
    /// P95（ms）。
    pub p95_ms: u64,
}

impl FiveNumSummary {
    /// 从 ms 样本集统计（n≥1；内部复制到堆排序，零原地改写调用方数据）。
    pub fn from_ms_samples(samples: &[u64]) -> Result<FiveNumSummary, BootThError> {
        let n = samples.len();
        if n == 0 {
            return Err(BootThError::InsufficientSamples { have: 0, need: 1 });
        }
        let mut v: Vec<u64> = Vec::with_capacity(n);
        let mut i = 0usize;
        while i < n {
            v.push(samples[i]);
            i += 1;
        }
        sort_u64_asc(&mut v);
        Ok(FiveNumSummary {
            n,
            min_ms: v[0],
            p25_ms: v[nearest_rank_index(n, 25)],
            median_ms: v[nearest_rank_index(n, 50)],
            p75_ms: v[nearest_rank_index(n, 75)],
            max_ms: v[n - 1],
            p95_ms: v[nearest_rank_index(n, 95)],
        })
    }

    /// 极差（max − min）。
    pub fn spread_ms(&self) -> u64 {
        self.max_ms.saturating_sub(self.min_ms)
    }

    /// 四分位距（P75 − P25）。
    pub fn iqr_ms(&self) -> u64 {
        self.p75_ms.saturating_sub(self.p25_ms)
    }

    /// 全样本在硬线内？（max ≤ limit——判据「50 次全过」的分布面复核）。
    pub fn within_limit(&self, limit_ms: u64) -> bool {
        self.max_ms <= limit_ms
    }

    /// 单行渲染（账面口径：ms）。
    pub fn render_line(&self) -> String {
        format!(
            "n={} min={} / p25={} / 中位={} / p75={} / max={} / p95={} (ms)",
            self.n, self.min_ms, self.p25_ms, self.median_ms, self.p75_ms, self.max_ms, self.p95_ms
        )
    }

    /// 指纹。
    pub fn fingerprint(&self) -> u64 {
        let mut h = fnv1a64_from(FNV_OFFSET, &(self.n as u64).to_le_bytes());
        h = fnv1a64_from(h, &self.min_ms.to_le_bytes());
        h = fnv1a64_from(h, &self.p25_ms.to_le_bytes());
        h = fnv1a64_from(h, &self.median_ms.to_le_bytes());
        h = fnv1a64_from(h, &self.p75_ms.to_le_bytes());
        h = fnv1a64_from(h, &self.max_ms.to_le_bytes());
        fnv1a64_from(h, &self.p95_ms.to_le_bytes())
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// 子系统 6：AttributionBook —— 超阈归因账（最大段归因 + 分布守恒）
// ═══════════════════════════════════════════════════════════════════════════

/// 超阈归因账：超阈样本归因最大段（固件/早传/内核）+ 分布万分比（Σ≡10000）。
#[derive(Clone, Debug)]
pub struct AttributionBook {
    /// 逐段归因笔数（下标 = `SegmentKind::index`）。
    counts: [u64; SEGMENT_COUNT],
    /// 逐段累计超额（us）。
    over_us_sum: [u64; SEGMENT_COUNT],
    /// 归因总笔数。
    total_intercepts: u64,
}

impl AttributionBook {
    /// 空账构造。
    pub fn new() -> AttributionBook {
        AttributionBook {
            counts: [0; SEGMENT_COUNT],
            over_us_sum: [0; SEGMENT_COUNT],
            total_intercepts: 0,
        }
    }

    /// 归因总笔数。
    pub fn total(&self) -> u64 {
        self.total_intercepts
    }

    /// 逐段归因笔数。
    pub fn counts(&self) -> [u64; SEGMENT_COUNT] {
        self.counts
    }

    /// 逐段累计超额（us）。
    pub fn over_us_sum(&self) -> [u64; SEGMENT_COUNT] {
        self.over_us_sum
    }

    /// 单笔归因入账（仅超阈样本入账；归因 = 主导段；返回归因段）。
    /// `limit_ms` 为硬线（ms）；超额 = 总时长 − 硬线（us 域）。
    pub fn apply(&mut self, span: &BootSpan, limit_ms: u64) -> Option<SegmentKind> {
        let limit_us = limit_ms.saturating_mul(US_PER_MS);
        let total_us = span.total_us();
        if total_us <= limit_us {
            return Option::None;
        }
        let d = span.dominant_segment();
        let i = d.index();
        self.counts[i] = self.counts[i].saturating_add(1);
        let over_us = total_us.saturating_sub(limit_us);
        self.over_us_sum[i] = self.over_us_sum[i].saturating_add(over_us);
        self.total_intercepts = self.total_intercepts.saturating_add(1);
        Option::Some(d)
    }

    /// 归因分布（万分比；Σ≡10000——余数补给最末非零段，守恒）。
    pub fn distribution_bp(&self) -> [u64; SEGMENT_COUNT] {
        let t = self.total_intercepts;
        if t == 0 {
            return [0; SEGMENT_COUNT];
        }
        let mut bp = [0u64; SEGMENT_COUNT];
        let mut assigned: u64 = 0;
        let mut i = 0usize;
        while i < SEGMENT_COUNT {
            bp[i] = self.counts[i].saturating_mul(10_000) / t;
            assigned = assigned.saturating_add(bp[i]);
            i += 1;
        }
        let rem = 10_000u64.saturating_sub(assigned);
        let mut j = SEGMENT_COUNT;
        while j > 0 {
            j -= 1;
            if self.counts[j] > 0 {
                bp[j] = bp[j].saturating_add(rem);
                break;
            }
        }
        bp
    }

    /// 分布守恒核（Σbp ≡ 10000，空账除外）。
    pub fn distribution_sum_ok(&self) -> bool {
        if self.total_intercepts == 0 {
            return true;
        }
        let bp = self.distribution_bp();
        bp[0].saturating_add(bp[1]).saturating_add(bp[2]) == 10_000
    }

    /// 主导归因类（并列取最早段；空账 → `None`）。
    pub fn dominant_class(&self) -> Option<SegmentKind> {
        if self.total_intercepts == 0 {
            return Option::None;
        }
        if self.counts[0] >= self.counts[1] && self.counts[0] >= self.counts[2] {
            Option::Some(SegmentKind::FirmwareT0T1)
        } else if self.counts[1] >= self.counts[2] {
            Option::Some(SegmentKind::EarlyXferT1T2)
        } else {
            Option::Some(SegmentKind::KernelT2T3)
        }
    }

    /// 单行渲染。
    pub fn render_line(&self) -> String {
        let bp = self.distribution_bp();
        format!(
            "固件 {} 笔 ({}bp) / 早传 {} 笔 ({}bp) / 内核 {} 笔 ({}bp) · 计 {} 笔",
            self.counts[0], bp[0], self.counts[1], bp[1], self.counts[2], bp[2], self.total_intercepts
        )
    }

    /// 指纹。
    pub fn fingerprint(&self) -> u64 {
        let mut h = fnv1a64_from(FNV_OFFSET, &self.counts[0].to_le_bytes());
        h = fnv1a64_from(h, &self.counts[1].to_le_bytes());
        h = fnv1a64_from(h, &self.counts[2].to_le_bytes());
        h = fnv1a64_from(h, &self.over_us_sum[0].to_le_bytes());
        h = fnv1a64_from(h, &self.over_us_sum[1].to_le_bytes());
        h = fnv1a64_from(h, &self.over_us_sum[2].to_le_bytes());
        fnv1a64_from(h, &self.total_intercepts.to_le_bytes())
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// 子系统 7：CalibrationGate —— F0665 校准规程对接面（采样→统计→冻结提案）
// ═══════════════════════════════════════════════════════════════════════════

/// 校准规程相位（F0665 三步：采样/统计/冻结提案）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CalibPhase {
    /// 空闲。
    Idle,
    /// 采样步。
    Sampling,
    /// 统计步（内部瞬态——`conclude_statistics` 后即 Proposed）。
    Statistic,
    /// 冻结提案步（提案成型，只可声明对接，不可落地）。
    Proposed,
}

impl CalibPhase {
    /// 中文标签。
    pub fn label(self) -> &'static str {
        match self {
            CalibPhase::Idle => "空闲",
            CalibPhase::Sampling => "采样",
            CalibPhase::Statistic => "统计",
            CalibPhase::Proposed => "冻结提案",
        }
    }

    /// 相位码。
    pub fn code(self) -> u8 {
        match self {
            CalibPhase::Idle => 0,
            CalibPhase::Sampling => 1,
            CalibPhase::Statistic => 2,
            CalibPhase::Proposed => 3,
        }
    }
}

/// 校准冻结提案（F0665 产物：只可经 ADR 落地，本模块零改写通道）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CalibrationProposal {
    /// 提案限值（ms = P95 × 4/3 整数除法）。
    pub proposed_ms: u64,
    /// 样本数。
    pub n_samples: usize,
    /// 样本 P95（ms）。
    pub p95_ms: u64,
    /// 冻结哈希（FNV(提案限值 ‖ 样本数 ‖ P95)）。
    pub freeze_hash: u64,
    /// ADR 引用（F0665-J1：变更走 ADR）。
    pub adr_ref: &'static str,
}

impl CalibrationProposal {
    /// 指纹（即冻结哈希）。
    pub fn fingerprint(&self) -> u64 {
        self.freeze_hash
    }
}

/// 校准规程步留痕。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CalibStepRecord {
    /// 步相位。
    pub step: CalibPhase,
    /// 附属量（样本数/提案哈希等）。
    pub aux: u64,
}

/// F0665 校准规程对接面（三步状态机；落地改写恒拒）。
pub struct CalibrationGate {
    phase: CalibPhase,
    samples: Vec<u64>,
    steps: Vec<CalibStepRecord>,
    proposal: Option<CalibrationProposal>,
}

impl CalibrationGate {
    /// 空闲构造。
    pub fn new() -> CalibrationGate {
        CalibrationGate {
            phase: CalibPhase::Idle,
            samples: Vec::new(),
            steps: Vec::new(),
            proposal: Option::None,
        }
    }

    /// 当前相位。
    pub fn phase(&self) -> CalibPhase {
        self.phase
    }

    /// 已采样本数。
    pub fn sample_count(&self) -> usize {
        self.samples.len()
    }

    /// 步留痕（只读）。
    pub fn steps(&self) -> &[CalibStepRecord] {
        &self.steps
    }

    /// 成型提案（只读）。
    pub fn proposal(&self) -> Option<CalibrationProposal> {
        self.proposal
    }

    /// 开采样步（Idle→Sampling；重复开 → `PhaseViolation`）。
    pub fn begin_sampling(&mut self) -> Result<(), BootThError> {
        if self.phase != CalibPhase::Idle {
            return Err(BootThError::PhaseViolation {
                expect: "Idle（空闲）起步",
                got: self.phase.label(),
            });
        }
        self.phase = CalibPhase::Sampling;
        self.steps.push(CalibStepRecord {
            step: CalibPhase::Sampling,
            aux: 0,
        });
        Ok(())
    }

    /// 采样入笔（仅采样步；1ms..60,000ms 域；满容拒收）。
    pub fn push_sample(&mut self, ms: u64) -> Result<(), BootThError> {
        if self.phase != CalibPhase::Sampling {
            return Err(BootThError::PhaseViolation {
                expect: "Sampling（采样步）",
                got: self.phase.label(),
            });
        }
        if ms == 0 || ms > CALIB_SAMPLE_MAX_MS {
            return Err(BootThError::ConfigInvalid {
                field: "calib.sample_ms",
                reason: "校准样本须在 1..=60000ms 域（0 或越界即采样域配置错误）",
            });
        }
        if self.samples.len() >= CALIB_SAMPLE_CAP {
            return Err(BootThError::CapacityFull {
                what: "校准采样账",
                cap: CALIB_SAMPLE_CAP,
            });
        }
        self.samples.push(ms);
        Ok(())
    }

    /// 统计步收口（≥20 样本；提案 = P95×4/3；相位 → Proposed）。
    pub fn conclude_statistics(&mut self) -> Result<CalibrationProposal, BootThError> {
        if self.phase != CalibPhase::Sampling {
            return Err(BootThError::PhaseViolation {
                expect: "Sampling（采样步收口）",
                got: self.phase.label(),
            });
        }
        if self.samples.len() < CALIB_MIN_SAMPLES {
            return Err(BootThError::InsufficientSamples {
                have: self.samples.len(),
                need: CALIB_MIN_SAMPLES,
            });
        }
        self.phase = CalibPhase::Statistic;
        let sum = FiveNumSummary::from_ms_samples(&self.samples)?;
        let p95 = sum.p95_ms;
        let proposed = p95
            .saturating_mul(CALIB_HEADROOM_NUM)
            / CALIB_HEADROOM_DEN;
        let n = self.samples.len() as u64;
        let mut h = fnv1a64_from(FNV_OFFSET, &proposed.to_le_bytes());
        h = fnv1a64_from(h, &n.to_le_bytes());
        h = fnv1a64_from(h, &p95.to_le_bytes());
        let prop = CalibrationProposal {
            proposed_ms: proposed,
            n_samples: self.samples.len(),
            p95_ms: p95,
            freeze_hash: h,
            adr_ref: "ADR-UNX-F0665",
        };
        self.steps.push(CalibStepRecord {
            step: CalibPhase::Statistic,
            aux: h,
        });
        self.phase = CalibPhase::Proposed;
        self.proposal = Option::Some(prop);
        Ok(prop)
    }

    /// 落地改写恒拒（F0665-J1：冻结后阈值表哈希封存且变更走 ADR——
    /// 本模块仅对接声明，不提供任何改写通道）。
    pub fn apply_to_line(
        &self,
        _proposal: &CalibrationProposal,
        _line: &ThresholdLine,
    ) -> Result<(), BootThError> {
        Err(BootThError::CalibInvalid {
            step: "冻结落地（freeze）",
        })
    }

    /// 复位（Proposed→Idle；步留痕保留，样本清空——可重跑规程）。
    pub fn reset(&mut self) -> Result<(), BootThError> {
        if self.phase != CalibPhase::Proposed {
            return Err(BootThError::PhaseViolation {
                expect: "Proposed（冻结提案步复位）",
                got: self.phase.label(),
            });
        }
        self.phase = CalibPhase::Idle;
        self.samples.clear();
        self.steps.push(CalibStepRecord {
            step: CalibPhase::Idle,
            aux: 0,
        });
        Ok(())
    }

    /// 指纹（相位码 + 样本链 + 步留痕链 + 提案哈希）。
    pub fn fingerprint(&self) -> u64 {
        let mut h = fnv1a64_from(FNV_OFFSET, &[self.phase.code()]);
        let mut i = 0usize;
        while i < self.samples.len() {
            h = fnv1a64_from(h, &self.samples[i].to_le_bytes());
            i += 1;
        }
        let mut j = 0usize;
        while j < self.steps.len() {
            h = fnv1a64_from(h, &[self.steps[j].step.code()]);
            h = fnv1a64_from(h, &self.steps[j].aux.to_le_bytes());
            j += 1;
        }
        match self.proposal {
            Option::None => fnv1a64_from(h, &[0x00]),
            Option::Some(p) => fnv1a64_from(h, &p.freeze_hash.to_le_bytes()),
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// 子系统 8：事件账 + 案件账（环形 · 序号连续核 · 满员逐出落账）
// ═══════════════════════════════════════════════════════════════════════════

/// 事件种类。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EventKind {
    /// 开账。
    BookOpen,
    /// 逐笔入账。
    RunRecorded,
    /// 判过。
    Pass,
    /// 拦截。
    Intercept,
    /// 超阈归因。
    Attribute,
    /// 跳号检出。
    SeqGapCaught,
    /// 重复登记检出。
    DuplicateCaught,
    /// 守恒破坏检出。
    ConservationBroken,
    /// 锚点乱序检出。
    AnchorDisorderCaught,
    /// 机号不符检出。
    MachineMismatch,
    /// 段宽下溢检出。
    SegUnderflow,
    /// 解冻尝试被拦。
    ThawBlocked,
    /// 校准对接声明。
    CalibDeclared,
    /// 封账。
    Sealed,
    /// 注入检出。
    InjectionCaught,
    /// 注入未检出（红灯）。
    InjectionUncaught,
}

impl EventKind {
    /// 中文标签。
    pub fn label(self) -> &'static str {
        match self {
            EventKind::BookOpen => "开账",
            EventKind::RunRecorded => "入账",
            EventKind::Pass => "判过",
            EventKind::Intercept => "拦截",
            EventKind::Attribute => "归因",
            EventKind::SeqGapCaught => "跳号检出",
            EventKind::DuplicateCaught => "重复检出",
            EventKind::ConservationBroken => "守恒破坏检出",
            EventKind::AnchorDisorderCaught => "锚点乱序检出",
            EventKind::MachineMismatch => "机号不符",
            EventKind::SegUnderflow => "段宽下溢",
            EventKind::ThawBlocked => "解冻拦截",
            EventKind::CalibDeclared => "校准对接声明",
            EventKind::Sealed => "封账",
            EventKind::InjectionCaught => "注入检出",
            EventKind::InjectionUncaught => "注入未检出",
        }
    }

    /// 事件码。
    pub fn code(self) -> u8 {
        match self {
            EventKind::BookOpen => 1,
            EventKind::RunRecorded => 2,
            EventKind::Pass => 3,
            EventKind::Intercept => 4,
            EventKind::Attribute => 5,
            EventKind::SeqGapCaught => 6,
            EventKind::DuplicateCaught => 7,
            EventKind::ConservationBroken => 8,
            EventKind::AnchorDisorderCaught => 9,
            EventKind::MachineMismatch => 10,
            EventKind::SegUnderflow => 11,
            EventKind::ThawBlocked => 12,
            EventKind::CalibDeclared => 13,
            EventKind::Sealed => 14,
            EventKind::InjectionCaught => 15,
            EventKind::InjectionUncaught => 16,
        }
    }
}

/// 事件账行。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BootThEvent {
    /// 事件序号（账内连续，自 1 起）。
    pub seq: u64,
    /// 种类。
    pub kind: EventKind,
    /// 相关引导次序号（0=非逐笔）。
    pub run_seq: u32,
    /// 机号。
    pub machine: MachineId,
    /// 附属量（耗时/超额/哈希等）。
    pub aux: u64,
}

/// 事件账（环形：满员逐出最老事件，驱逐计数入指纹——零静默吞事件）。
pub struct EventLedger {
    cap: usize,
    events: Vec<BootThEvent>,
    evicted: usize,
    next_seq: u64,
}

impl EventLedger {
    /// 构造（cap ≥ 8）。
    pub fn new(cap: usize) -> Result<EventLedger, BootThError> {
        if cap < 8 {
            return Err(BootThError::ConfigInvalid {
                field: "events.cap",
                reason: "事件账容量必须 ≥8",
            });
        }
        Ok(EventLedger {
            cap,
            events: Vec::new(),
            evicted: 0,
            next_seq: 1,
        })
    }

    /// 容量。
    pub fn cap(&self) -> usize {
        self.cap
    }

    /// 窗口内事件数。
    pub fn len(&self) -> usize {
        self.events.len()
    }

    /// 空账？
    pub fn is_empty(&self) -> bool {
        self.events.is_empty()
    }

    /// 累计驱逐数。
    pub fn evicted(&self) -> usize {
        self.evicted
    }

    /// 窗口事件（只读）。
    pub fn events(&self) -> &[BootThEvent] {
        &self.events
    }

    /// 入账（环形逐出 + 序号自增——序号连续由构造保证）。
    pub fn push(&mut self, kind: EventKind, run_seq: u32, machine: MachineId, aux: u64) {
        if self.events.len() >= self.cap {
            self.events.remove(0);
            self.evicted = self.evicted.saturating_add(1);
        }
        let seq = self.next_seq;
        self.next_seq = self.next_seq.saturating_add(1);
        self.events.push(BootThEvent {
            seq,
            kind,
            run_seq,
            machine,
            aux,
        });
    }

    /// 窗口序号连续核（bool 版）。
    pub fn seq_ok(&self) -> bool {
        let mut i = 1usize;
        while i < self.events.len() {
            if self.events[i].seq != self.events[i - 1].seq.saturating_add(1) {
                return false;
            }
            i += 1;
        }
        true
    }

    /// 窗口序号连续审计（`Result` 版——断链点定位）。
    pub fn audit(&self) -> Result<(), BootThError> {
        let mut i = 1usize;
        while i < self.events.len() {
            let expect = self.events[i - 1].seq.saturating_add(1);
            if self.events[i].seq != expect {
                return Err(BootThError::EventSeqBroken {
                    expect,
                    got: self.events[i].seq,
                });
            }
            i += 1;
        }
        Ok(())
    }

    /// 某类事件计数（窗口内）。
    pub fn count_of(&self, kind: EventKind) -> u64 {
        let mut n = 0u64;
        let mut i = 0usize;
        while i < self.events.len() {
            if self.events[i].kind == kind {
                n = n.saturating_add(1);
            }
            i += 1;
        }
        n
    }

    /// 注入面专用：篡改窗口内某事件序号（审计必断链）。
    pub fn tamper_seq(&mut self, idx: usize, seq: u64) {
        if idx < self.events.len() {
            self.events[idx].seq = seq;
        }
    }

    /// 指纹（容量/驱逐/窗口逐行链式）。
    pub fn fingerprint(&self) -> u64 {
        let mut h = fnv1a64_from(FNV_OFFSET, &(self.cap as u64).to_le_bytes());
        h = fnv1a64_from(h, &(self.evicted as u64).to_le_bytes());
        h = fnv1a64_from(h, &self.next_seq.to_le_bytes());
        let mut i = 0usize;
        while i < self.events.len() {
            h = fnv1a64_from(h, &self.events[i].seq.to_le_bytes());
            h = fnv1a64_from(h, &[self.events[i].kind.code()]);
            h = fnv1a64_from(h, &self.events[i].run_seq.to_le_bytes());
            h = fnv1a64_from(h, &[self.events[i].machine.code()]);
            h = fnv1a64_from(h, &self.events[i].aux.to_le_bytes());
            i += 1;
        }
        h
    }
}

/// 案件种类。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CaseKind {
    /// 超阈拦截案。
    OverThreshold,
    /// 次序号跳号案。
    SeqGap,
    /// 重复登记案。
    Duplicate,
    /// 守恒破坏案（红灯级）。
    Conservation,
    /// 锚点乱序案。
    AnchorDisorder,
    /// 硬线解冻尝试案（红灯级）。
    Thaw,
    /// 校准违规案。
    CalibBlocked,
    /// 注入未检出案（红灯级）。
    InjectionUncaught,
}

impl CaseKind {
    /// 中文标签。
    pub fn label(self) -> &'static str {
        match self {
            CaseKind::OverThreshold => "超阈拦截",
            CaseKind::SeqGap => "次序号跳号",
            CaseKind::Duplicate => "重复登记",
            CaseKind::Conservation => "守恒破坏",
            CaseKind::AnchorDisorder => "锚点乱序",
            CaseKind::Thaw => "硬线解冻尝试",
            CaseKind::CalibBlocked => "校准违规",
            CaseKind::InjectionUncaught => "注入未检出",
        }
    }

    /// 案件码。
    pub fn code(self) -> u8 {
        match self {
            CaseKind::OverThreshold => 1,
            CaseKind::SeqGap => 2,
            CaseKind::Duplicate => 3,
            CaseKind::Conservation => 4,
            CaseKind::AnchorDisorder => 5,
            CaseKind::Thaw => 6,
            CaseKind::CalibBlocked => 7,
            CaseKind::InjectionUncaught => 8,
        }
    }

    /// 红灯级案件？（守恒/解冻/注入未检出）。
    pub fn is_red_grade(self) -> bool {
        match self {
            CaseKind::Conservation | CaseKind::Thaw | CaseKind::InjectionUncaught => true,
            _ => false,
        }
    }
}

/// 案件账行。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CaseRecord {
    /// 案件序号（连续自 1）。
    pub case_seq: u64,
    /// 种类。
    pub kind: CaseKind,
    /// 相关引导次序号（0=非逐笔）。
    pub run_seq: u32,
    /// 机号。
    pub machine: MachineId,
    /// 证据指针（静态串）。
    pub evidence: &'static str,
}

/// 案件账（满员逐出最老案，驱逐计数入指纹——摘要不失）。
pub struct CaseBook {
    cap: usize,
    cases: Vec<CaseRecord>,
    next_seq: u64,
    evicted: usize,
}

impl CaseBook {
    /// 构造（cap ≥ 4）。
    pub fn new(cap: usize) -> Result<CaseBook, BootThError> {
        if cap < 4 {
            return Err(BootThError::ConfigInvalid {
                field: "cases.cap",
                reason: "案件账容量必须 ≥4",
            });
        }
        Ok(CaseBook {
            cap,
            cases: Vec::new(),
            next_seq: 1,
            evicted: 0,
        })
    }

    /// 容量。
    pub fn cap(&self) -> usize {
        self.cap
    }

    /// 在案数。
    pub fn len(&self) -> usize {
        self.cases.len()
    }

    /// 空案？
    pub fn is_empty(&self) -> bool {
        self.cases.is_empty()
    }

    /// 累计驱逐数。
    pub fn evicted(&self) -> usize {
        self.evicted
    }

    /// 立案（环形逐出；返回案件序号）。
    pub fn file(&mut self, kind: CaseKind, run_seq: u32, machine: MachineId, evidence: &'static str) -> u64 {
        if self.cases.len() >= self.cap {
            self.cases.remove(0);
            self.evicted = self.evicted.saturating_add(1);
        }
        let seq = self.next_seq;
        self.next_seq = self.next_seq.saturating_add(1);
        self.cases.push(CaseRecord {
            case_seq: seq,
            kind,
            run_seq,
            machine,
            evidence,
        });
        seq
    }

    /// 某类案件计数。
    pub fn count_of(&self, kind: CaseKind) -> u64 {
        let mut n = 0u64;
        let mut i = 0usize;
        while i < self.cases.len() {
            if self.cases[i].kind == kind {
                n = n.saturating_add(1);
            }
            i += 1;
        }
        n
    }

    /// 存在某类案件？
    pub fn has(&self, kind: CaseKind) -> bool {
        self.count_of(kind) > 0
    }

    /// 存在任一红灯级案件？
    pub fn has_red_grade(&self) -> bool {
        let mut i = 0usize;
        while i < self.cases.len() {
            if self.cases[i].kind.is_red_grade() {
                return true;
            }
            i += 1;
        }
        false
    }

    /// 案件窗口（只读）。
    pub fn cases(&self) -> &[CaseRecord] {
        &self.cases
    }

    /// 指纹。
    pub fn fingerprint(&self) -> u64 {
        let mut h = fnv1a64_from(FNV_OFFSET, &(self.cap as u64).to_le_bytes());
        h = fnv1a64_from(h, &(self.evicted as u64).to_le_bytes());
        h = fnv1a64_from(h, &self.next_seq.to_le_bytes());
        let mut i = 0usize;
        while i < self.cases.len() {
            h = fnv1a64_from(h, &self.cases[i].case_seq.to_le_bytes());
            h = fnv1a64_from(h, &[self.cases[i].kind.code()]);
            h = fnv1a64_from(h, &self.cases[i].run_seq.to_le_bytes());
            h = fnv1a64_from(h, &[self.cases[i].machine.code()]);
            i += 1;
        }
        h
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// 配置
// ═══════════════════════════════════════════════════════════════════════════

/// 引擎配置（判据冻结域：run_count 恰 50、hard_limit_ms 恰 3,000）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BootThConfig {
    /// 判定笔数（必须 == `RUN_COUNT`）。
    pub run_count: usize,
    /// 硬线（ms；必须 == `HARD_LIMIT_MS`——防夹带私改线）。
    pub hard_limit_ms: u64,
    /// 事件账容量（64..=4096）。
    pub event_cap: usize,
    /// 案件账容量（32..=1024）。
    pub case_cap: usize,
    /// 报告行数上限（22..=1024；≥ 头尾固定行 + 8 行账行）。
    pub report_line_cap: usize,
}

impl BootThConfig {
    /// 标准配置（判据口径写死）。
    pub const fn standard() -> BootThConfig {
        BootThConfig {
            run_count: RUN_COUNT,
            hard_limit_ms: HARD_LIMIT_MS,
            event_cap: EVENT_CAP_DEFAULT,
            case_cap: CASE_CAP_DEFAULT,
            report_line_cap: REPORT_LINE_CAP_DEFAULT,
        }
    }

    /// 校验（越界即 `ConfigInvalid`，逐字段中文原因）。
    pub fn validate(&self) -> Result<(), BootThError> {
        if self.run_count != RUN_COUNT {
            return Err(BootThError::ConfigInvalid {
                field: "run_count",
                reason: "判定笔数必须恰为 50（J1 判据冻结口径）",
            });
        }
        if self.hard_limit_ms != HARD_LIMIT_MS {
            return Err(BootThError::ConfigInvalid {
                field: "hard_limit_ms",
                reason: "硬线必须恰为 3,000ms（冻结；变更走 F0665 校准规程+ADR）",
            });
        }
        if self.event_cap < 64 || self.event_cap > 4096 {
            return Err(BootThError::ConfigInvalid {
                field: "event_cap",
                reason: "事件账容量须在 64..=4096",
            });
        }
        if self.case_cap < 32 || self.case_cap > 1024 {
            return Err(BootThError::ConfigInvalid {
                field: "case_cap",
                reason: "案件账容量须在 32..=1024",
            });
        }
        if self.report_line_cap < 22 || self.report_line_cap > 1024 {
            return Err(BootThError::ConfigInvalid {
                field: "report_line_cap",
                reason: "报告行数上限须在 22..=1024（防截断吞掉全部账行）",
            });
        }
        Ok(())
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// 子系统 9a：注入面类型
// ═══════════════════════════════════════════════════════════════════════════

/// 注入种类。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InjectionKind {
    /// 超阈注入（带主导段）。
    Overrun(SegmentKind),
    /// 跳号注入。
    SeqGap,
    /// 守恒破坏注入。
    ConservationBreak,
}

impl InjectionKind {
    /// 中文标签。
    pub fn label(self) -> &'static str {
        match self {
            InjectionKind::Overrun(_) => "超阈注入",
            InjectionKind::SeqGap => "跳号注入",
            InjectionKind::ConservationBreak => "守恒破坏注入",
        }
    }
}

/// 注入结果（caught=false 即判据可信性红灯）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct InjectionOutcome {
    /// 注入种类。
    pub kind: InjectionKind,
    /// 是否检出。
    pub caught: bool,
    /// 证据说明。
    pub evidence: &'static str,
}

/// 注入结果集全检出核。
pub fn outcomes_all_caught(outcomes: &[InjectionOutcome]) -> bool {
    let mut i = 0usize;
    while i < outcomes.len() {
        if !outcomes[i].caught {
            return false;
        }
        i += 1;
    }
    true
}

// ═══════════════════════════════════════════════════════════════════════════
// 子系统 9b：J1Report —— 判据聚合核报告
// ═══════════════════════════════════════════════════════════════════════════

/// UNX-F0664-J1 聚合核报告（三断言一次出）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct J1Report {
    /// 机号。
    pub machine: MachineId,
    /// 在账笔数。
    pub runs: usize,
    /// 通过笔数。
    pub pass_count: u64,
    /// 拦截笔数。
    pub intercept_count: u64,
    /// 超阈见账笔数（引擎双口径之一）。
    pub over_limit_seen: u64,
    /// 断言 1：50 次全过。
    pub all_pass: bool,
    /// 断言 2：超阈拦截检出 100%（三口径对齐：拦截笔数 == 超阈见账 == 超阈立案）。
    pub intercept_detected_all: bool,
    /// 断言 3：账行三要素齐（逐行在位 + 结论对耗时重判一致 + 守恒旗标全真）。
    pub rows_triple_complete: bool,
    /// 次序号连续零跳号（逐行扫描复核）。
    pub seq_continuous: bool,
    /// 守恒全平（逐行旗标复核）。
    pub conservation_all: bool,
    /// 五数概括 + P95。
    pub summary: FiveNumSummary,
    /// J1 总判（三断言 ∧ 次序连续 ∧ 守恒全平 ∧ 恰 50 笔）。
    pub j1: bool,
    /// 报告指纹。
    pub fingerprint: u64,
}

impl J1Report {
    /// 单行渲染。
    pub fn render_line(&self) -> String {
        format!(
            "{}: 50 全过={} / 拦截检出100%={} / 三要素齐={} → J1={}",
            self.machine.label(),
            self.all_pass,
            self.intercept_detected_all,
            self.rows_triple_complete,
            self.j1
        )
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// 子系统 9c：BootThEngine —— 判定账引擎
// ═══════════════════════════════════════════════════════════════════════════

/// 冷启动阈值判定账引擎（单机单账；三机矩阵 = 三引擎聚合）。
pub struct BootThEngine {
    cfg: BootThConfig,
    machine: MachineId,
    opened_at: u64,
    line: ThresholdLine,
    judge: ThresholdJudge,
    book: FiftyRunBook,
    attribution: AttributionBook,
    events: EventLedger,
    cases: CaseBook,
    phase: EnginePhase,
    recorded_total: u64,
    over_limit_seen: u64,
}

impl BootThEngine {
    /// 构造（配置校验 + 开账事件）。
    pub fn new(cfg: BootThConfig, machine: MachineId, opened_at: u64) -> Result<BootThEngine, BootThError> {
        cfg.validate()?;
        let line = ThresholdLine::frozen();
        let judge = ThresholdJudge::from_line(&line)?;
        let mut events = EventLedger::new(cfg.event_cap)?;
        events.push(EventKind::BookOpen, 0, machine, opened_at);
        Ok(BootThEngine {
            cfg,
            machine,
            opened_at,
            line,
            judge,
            book: FiftyRunBook::new(machine, cfg.run_count)?,
            attribution: AttributionBook::new(),
            events,
            cases: CaseBook::new(cfg.case_cap)?,
            phase: EnginePhase::Recording,
            recorded_total: 0,
            over_limit_seen: 0,
        })
    }

    /// 按机号码构造（非法码 → `UnknownMachine`）。
    pub fn with_machine_code(
        cfg: BootThConfig,
        machine_code: u8,
        opened_at: u64,
    ) -> Result<BootThEngine, BootThError> {
        let m = match MachineId::from_code(machine_code) {
            Option::Some(m) => m,
            Option::None => {
                return Err(BootThError::UnknownMachine { code: machine_code });
            }
        };
        BootThEngine::new(cfg, m, opened_at)
    }

    /// 机号。
    pub fn machine(&self) -> MachineId {
        self.machine
    }

    /// 相位。
    pub fn phase(&self) -> EnginePhase {
        self.phase
    }

    /// 开账时戳。
    pub fn opened_at(&self) -> u64 {
        self.opened_at
    }

    /// 配置（只读）。
    pub fn config(&self) -> &BootThConfig {
        &self.cfg
    }

    /// 冻结硬线（只读）。
    pub fn line(&self) -> &ThresholdLine {
        &self.line
    }

    /// 判定器（只读）。
    pub fn judge(&self) -> &ThresholdJudge {
        &self.judge
    }

    /// 判定账（只读）。
    pub fn book(&self) -> &FiftyRunBook {
        &self.book
    }

    /// 归因账（只读）。
    pub fn attribution(&self) -> &AttributionBook {
        &self.attribution
    }

    /// 事件账（只读）。
    pub fn events(&self) -> &EventLedger {
        &self.events
    }

    /// 案件账（只读）。
    pub fn cases(&self) -> &CaseBook {
        &self.cases
    }

    /// 累计入账笔数（含拒收尝试前的计数口径：仅成功入账计）。
    pub fn recorded_total(&self) -> u64 {
        self.recorded_total
    }

    /// 超阈见账笔数（判定流水线口径）。
    pub fn over_limit_seen(&self) -> u64 {
        self.over_limit_seen
    }

    /// 录入流水线：机号核→锚点序核→段宽核→守恒核→判定→入账→归因→立案→事件
    /// （满 50 自动封账）。全部失败路径先立案落事件再返回 `Err`。
    pub fn record_span(&mut self, span: BootSpan) -> Result<JudgeRow, BootThError> {
        if self.phase == EnginePhase::Sealed {
            return Err(BootThError::SealViolation {
                run_seq: span.run_seq(),
            });
        }
        if span.machine() != self.machine {
            self.events.push(EventKind::MachineMismatch, span.run_seq(), span.machine(), 0);
            return Err(BootThError::UnknownMachine {
                code: span.machine().code(),
            });
        }
        if !span.anchors_monotonic() {
            self.cases.file(
                CaseKind::AnchorDisorder,
                span.run_seq(),
                self.machine,
                "锚点非单调（T0..T3 时戳乱序）——拒收立案",
            );
            self.events.push(EventKind::AnchorDisorderCaught, span.run_seq(), self.machine, 0);
            return Err(BootThError::AnchorDisorder {
                anchor: 0,
                prev_us: span.anchor_us(BootAnchor::T0),
                got_us: span.anchor_us(BootAnchor::T3),
            });
        }
        if span.seg_triple_strict().is_err() {
            self.cases.file(
                CaseKind::AnchorDisorder,
                span.run_seq(),
                self.machine,
                "段宽下溢（锚点差值 checked 失败）——拒收立案",
            );
            self.events.push(EventKind::SegUnderflow, span.run_seq(), self.machine, 0);
            return Err(BootThError::Overflow {
                op: "三段宽 checked 差值",
            });
        }
        if !span.conservation_ok() {
            self.cases.file(
                CaseKind::Conservation,
                span.run_seq(),
                self.machine,
                "守恒破坏（Σ三段 ≠ T0→T3 或申报总长不符）——拒收立案",
            );
            self.events.push(
                EventKind::ConservationBroken,
                span.run_seq(),
                self.machine,
                span.declared_total_us(),
            );
            return Err(BootThError::ConservationBroken {
                computed_us: span.total_us(),
                declared_us: span.declared_total_us(),
            });
        }
        let verdict = self.judge.judge_span(&span);
        let over = !verdict.is_pass();
        let row = self.judge.row_for(&span);
        // 归因预判（纯计算，不落账——入账成功后才落归因账，防拒收样本污染）。
        let attributed = if over {
            Option::Some(span.dominant_segment())
        } else {
            Option::None
        };
        let rec = RunRecord::from_span(&span, row, attributed)?;
        if let Err(e) = self.book.record(rec) {
            match e {
                BootThError::SeqGap { expect, got } => {
                    self.cases.file(
                        CaseKind::SeqGap,
                        got,
                        self.machine,
                        "次序号跳号——拒收立案",
                    );
                    self.events.push(EventKind::SeqGapCaught, got, self.machine, expect as u64);
                    return Err(e);
                }
                BootThError::DuplicateRun { have } => {
                    self.cases.file(
                        CaseKind::Duplicate,
                        have,
                        self.machine,
                        "重复登记（同次序号二次入账）——拒收立案",
                    );
                    self.events.push(EventKind::DuplicateCaught, have, self.machine, 0);
                    return Err(e);
                }
                _ => {
                    return Err(e);
                }
            }
        }
        self.events.push(
            EventKind::RunRecorded,
            row.run_seq,
            self.machine,
            span.total_us(),
        );
        if over {
            // 入账成功后落超阈计数 + 归因账 + 立案（三口径同步——检出 100% 核的对齐面）。
            self.over_limit_seen = self.over_limit_seen.saturating_add(1);
            let applied = self.attribution.apply(&span, self.line.limit_ms());
            let ev_over = verdict.over_ms();
            self.cases.file(
                CaseKind::OverThreshold,
                row.run_seq,
                self.machine,
                "T0→T3 超硬线 3,000ms——拦截并归因最大段",
            );
            self.events.push(EventKind::Intercept, row.run_seq, self.machine, ev_over);
            if let Option::Some(k) = applied {
                self.events.push(EventKind::Attribute, row.run_seq, self.machine, k.index() as u64);
            }
        } else {
            self.events.push(EventKind::Pass, row.run_seq, self.machine, verdict.slack_ms());
        }
        self.recorded_total = self.recorded_total.saturating_add(1);
        if self.book.is_complete() && !self.book.is_sealed() {
            self.book.seal_quiet();
            self.phase = EnginePhase::Sealed;
            self.events.push(EventKind::Sealed, 0, self.machine, self.book.len() as u64);
        }
        Ok(row)
    }

    /// 合成录入（按画像确定性合成下一笔并走完整流水线）。
    pub fn record_synthetic(
        &mut self,
        profile: SpanProfile,
        rng: &mut Lcg,
    ) -> Result<JudgeRow, BootThError> {
        let seq = self.book.next_expected_seq();
        let span = synth_span(self.machine, seq, profile, rng)?;
        self.record_span(span)
    }

    /// 超阈拦截检出 100% 在线核（三口径对齐：拦截笔数 == 超阈见账 == 超阈立案；
    /// 未封账亦可调用——注入面对拍用）。
    pub fn interception_full(&self) -> bool {
        self.book.intercept_count() == self.over_limit_seen
            && self.cases.count_of(CaseKind::OverThreshold) == self.over_limit_seen
    }

    /// 健康度三色。
    pub fn health(&self) -> Health {
        if !self.line.verify_integrity() {
            return Health::Red;
        }
        if self.cases.has_red_grade() {
            return Health::Red;
        }
        if !self.events.seq_ok() {
            return Health::Red;
        }
        if self.phase != EnginePhase::Sealed {
            return Health::Amber;
        }
        if !self.cases.is_empty() {
            return Health::Amber;
        }
        Health::Green
    }

    /// 五数概括（透传判定账；空账 → `InsufficientSamples`）。
    pub fn summary(&self) -> Result<FiveNumSummary, BootThError> {
        self.book.summary()
    }

    /// UNX-F0664-J1 聚合核（须已封账；三断言 + 次序连续 + 守恒全平一次出）。
    pub fn verify_j1(&self) -> Result<J1Report, BootThError> {
        if self.phase != EnginePhase::Sealed {
            return Err(BootThError::PhaseViolation {
                expect: "Sealed（满 50 自动封账后）",
                got: self.phase.label(),
            });
        }
        self.events.audit()?;
        let runs = self.book.len();
        let pass_count = self.book.pass_count();
        let intercept_count = self.book.intercept_count();
        let over_limit_seen = self.over_limit_seen;
        let over_cases = self.cases.count_of(CaseKind::OverThreshold);
        let intercept_detected_all =
            intercept_count == over_limit_seen && over_cases == over_limit_seen;
        let seq_continuous = self.book.seq_continuous();
        let mut rows_triple_complete = true;
        let mut conservation_all = true;
        let recs = self.book.records();
        let mut i = 0usize;
        while i < recs.len() {
            if !recs[i].row.triple_complete() {
                rows_triple_complete = false;
            }
            if recs[i]
                .row
                .verify_against_us(self.line.limit_ms(), recs[i].total_us_exact)
                .is_err()
            {
                rows_triple_complete = false;
            }
            if !recs[i].conservation_ok {
                conservation_all = false;
                rows_triple_complete = false;
            }
            i += 1;
        }
        let summary = self.book.summary()?;
        let all_pass = pass_count == self.cfg.run_count as u64 && intercept_count == 0;
        let j1 = runs == RUN_COUNT
            && all_pass
            && intercept_detected_all
            && rows_triple_complete
            && seq_continuous
            && conservation_all;
        let mut h = fnv1a64_from(FNV_OFFSET, &[self.machine.code()]);
        h = fnv1a64_from(h, &(runs as u64).to_le_bytes());
        h = fnv1a64_from(h, &pass_count.to_le_bytes());
        h = fnv1a64_from(h, &intercept_count.to_le_bytes());
        h = fnv1a64_from(h, &over_limit_seen.to_le_bytes());
        let a1: u8 = if all_pass { 1 } else { 0 };
        let a2: u8 = if intercept_detected_all { 1 } else { 0 };
        let a3: u8 = if rows_triple_complete { 1 } else { 0 };
        h = fnv1a64_from(h, &[a1, a2, a3]);
        h = fnv1a64_from(h, &summary.fingerprint().to_le_bytes());
        Ok(J1Report {
            machine: self.machine,
            runs,
            pass_count,
            intercept_count,
            over_limit_seen,
            all_pass,
            intercept_detected_all,
            rows_triple_complete,
            seq_continuous,
            conservation_all,
            summary,
            j1,
            fingerprint: h,
        })
    }

    /// 全量指纹（配置/硬线/相位/四账链式——双跑对拍载体）。
    pub fn fingerprint(&self) -> u64 {
        let mut h = fnv1a64_from(FNV_OFFSET, BOOTTH_VERSION.as_bytes());
        h = fnv1a64_from(h, &[self.machine.code()]);
        h = fnv1a64_from(h, &self.opened_at.to_le_bytes());
        h = fnv1a64_from(h, &(self.cfg.run_count as u64).to_le_bytes());
        h = fnv1a64_from(h, &self.cfg.hard_limit_ms.to_le_bytes());
        h = fnv1a64_from(h, &self.line.frozen_hash().to_le_bytes());
        h = fnv1a64_from(h, &[self.phase.code()]);
        h = fnv1a64_from(h, &self.recorded_total.to_le_bytes());
        h = fnv1a64_from(h, &self.over_limit_seen.to_le_bytes());
        h = fnv1a64_from(h, &self.book.fingerprint().to_le_bytes());
        h = fnv1a64_from(h, &self.attribution.fingerprint().to_le_bytes());
        h = fnv1a64_from(h, &self.events.fingerprint().to_le_bytes());
        fnv1a64_from(h, &self.cases.fingerprint().to_le_bytes())
    }

    /// 解冻尝试面（恒拒 + 立案 + 落事件——红灯级纪律面）。
    pub fn attempt_thaw(&mut self, new_ms: u64) -> Result<(), BootThError> {
        let r: Result<(), BootThError> = match self.line.attempt_direct_change(new_ms) {
            Ok(line) => {
                let _ = line;
                Ok(())
            }
            Err(e) => Err(e),
        };
        self.cases.file(
            CaseKind::Thaw,
            0,
            self.machine,
            "直接改写冻结硬线被拒——变更须走 F0665 校准规程+ADR",
        );
        self.events.push(EventKind::ThawBlocked, 0, self.machine, new_ms);
        r
    }

    /// F0665 对接声明（校准提案入事件账；零改写——限值原封不动）。
    pub fn declare_calibration(&mut self, gate: &CalibrationGate) -> Result<u64, BootThError> {
        let prop = match gate.proposal() {
            Option::Some(p) => p,
            Option::None => {
                self.cases.file(
                    CaseKind::CalibBlocked,
                    0,
                    self.machine,
                    "校准未成型（无冻结提案）即声明对接——违规立案",
                );
                return Err(BootThError::CalibInvalid {
                    step: "对接声明（declare）",
                });
            }
        };
        self.events
            .push(EventKind::CalibDeclared, 0, self.machine, prop.freeze_hash);
        Ok(prop.freeze_hash)
    }

    /// 硬线完整性复核（true = 封存哈希无篡改）。
    pub fn line_integrity_ok(&self) -> bool {
        self.line.verify_integrity()
    }

    /// 注入面专用：篡改硬线限值（健康度必转红——完整性失守路径复验）。
    pub fn tamper_line_limit(&mut self, new_ms: u64) {
        self.line.tamper_limit(new_ms);
    }

    // ── 注入面（三类 · 各必检出；未检出 = 立红灯案 + `InjectionUncaught`）──

    /// 超阈注入：合成指定主导段的超阈样本并录入——必须判拦截 + 归因 + 立案。
    pub fn inject_overrun(
        &mut self,
        seg: SegmentKind,
        rng: &mut Lcg,
    ) -> Result<InjectionOutcome, BootThError> {
        let profile = match seg {
            SegmentKind::FirmwareT0T1 => SpanProfile::OverFirmware,
            SegmentKind::EarlyXferT1T2 => SpanProfile::OverEarlyXfer,
            SegmentKind::KernelT2T3 => SpanProfile::OverKernel,
        };
        let seq = self.book.next_expected_seq();
        let span = synth_span(self.machine, seq, profile, rng)?;
        let attr_before = self.attribution.total();
        let over_cases_before = self.cases.count_of(CaseKind::OverThreshold);
        let row = self.record_span(span)?;
        let caught = !row.verdict.is_pass()
            && self.attribution.total() == attr_before.saturating_add(1)
            && self.cases.count_of(CaseKind::OverThreshold) == over_cases_before.saturating_add(1)
            && self.attribution.counts()[seg.index()] >= 1;
        if caught {
            self.events.push(EventKind::InjectionCaught, row.run_seq, self.machine, 1);
            Ok(InjectionOutcome {
                kind: InjectionKind::Overrun(seg),
                caught,
                evidence: "超阈注入被拦截：判拦截+归因最大段+立案三证齐",
            })
        } else {
            self.cases.file(
                CaseKind::InjectionUncaught,
                row.run_seq,
                self.machine,
                "超阈注入未被完整检出（判/归因/立案缺证）——红灯立案",
            );
            self.events.push(EventKind::InjectionUncaught, row.run_seq, self.machine, 1);
            Err(BootThError::InjectionUncaught {
                kind: "超阈注入（Overrun）",
            })
        }
    }

    /// 跳号注入：以 next+jump 次序号录入——必须拒收 `SeqGap` + 立案 + 落事件。
    pub fn inject_seq_gap(
        &mut self,
        jump: u32,
        rng: &mut Lcg,
    ) -> Result<InjectionOutcome, BootThError> {
        if jump == 0 {
            return Err(BootThError::ConfigInvalid {
                field: "inject.jump",
                reason: "跳号注入幅必须 ≥1",
            });
        }
        let next = self.book.next_expected_seq();
        let bad_seq = next.saturating_add(jump);
        let span = synth_span(self.machine, bad_seq, SpanProfile::NominalMid, rng)?;
        let gap_cases_before = self.cases.count_of(CaseKind::SeqGap);
        let r = self.record_span(span);
        let caught = match r {
            Err(BootThError::SeqGap { expect, got }) => {
                let _ = expect;
                let _ = got;
                self.cases.count_of(CaseKind::SeqGap) == gap_cases_before.saturating_add(1)
                    && self.events.count_of(EventKind::SeqGapCaught) >= 1
            }
            _ => false,
        };
        if caught {
            self.events.push(EventKind::InjectionCaught, bad_seq, self.machine, 2);
            Ok(InjectionOutcome {
                kind: InjectionKind::SeqGap,
                caught,
                evidence: "跳号注入被检出：拒收 SeqGap+立案+事件三证齐",
            })
        } else {
            self.cases.file(
                CaseKind::InjectionUncaught,
                bad_seq,
                self.machine,
                "跳号注入未被检出（未拒收或缺立案/事件）——红灯立案",
            );
            self.events.push(EventKind::InjectionUncaught, bad_seq, self.machine, 2);
            Err(BootThError::InjectionUncaught {
                kind: "跳号注入（SeqGap）",
            })
        }
    }

    /// 守恒破坏注入：申报总长 +delta——必须拒收 `ConservationBroken` + 立案 + 落事件。
    pub fn inject_conservation_break(
        &mut self,
        delta_us: u64,
        rng: &mut Lcg,
    ) -> Result<InjectionOutcome, BootThError> {
        if delta_us == 0 {
            return Err(BootThError::ConfigInvalid {
                field: "inject.delta_us",
                reason: "守恒破坏注入幅必须 >0",
            });
        }
        let seq = self.book.next_expected_seq();
        let mut span = synth_span(self.machine, seq, SpanProfile::NominalMid, rng)?;
        span.tamper_declared_total(span.declared_total_us().saturating_add(delta_us));
        let cons_cases_before = self.cases.count_of(CaseKind::Conservation);
        let r = self.record_span(span);
        let caught = match r {
            Err(BootThError::ConservationBroken { computed_us, declared_us }) => {
                let _ = computed_us;
                let _ = declared_us;
                self.cases.count_of(CaseKind::Conservation) == cons_cases_before.saturating_add(1)
                    && self.events.count_of(EventKind::ConservationBroken) >= 1
            }
            _ => false,
        };
        if caught {
            self.events.push(EventKind::InjectionCaught, seq, self.machine, 3);
            Ok(InjectionOutcome {
                kind: InjectionKind::ConservationBreak,
                caught,
                evidence: "守恒破坏注入被检出：拒收 ConservationBroken+立案+事件三证齐",
            })
        } else {
            self.cases.file(
                CaseKind::InjectionUncaught,
                seq,
                self.machine,
                "守恒破坏注入未被检出（未拒收或缺立案/事件）——红灯立案",
            );
            self.events.push(EventKind::InjectionUncaught, seq, self.machine, 3);
            Err(BootThError::InjectionUncaught {
                kind: "守恒破坏注入（ConservationBreak）",
            })
        }
    }

    /// 文本报告（行数封顶；截断只发生在账行段且截断数落 `ReportOutcome`——零静默吞行）。
    pub fn render_report(&self) -> ReportOutcome {
        let cap = self.cfg.report_line_cap;
        let mut lines: Vec<String> = Vec::new();
        lines.push(format!("== UNX-F0664 冷启动阈值判定账 · {} ==", BOOTTH_VERSION));
        lines.push(format!(
            "机号 {} · 相位 {} · 开账时戳 {}",
            self.machine.label(),
            self.phase.label(),
            self.opened_at
        ));
        lines.push(format!(
            "硬线 {}ms（冻结哈希 {:016x} · 完整性 {}）",
            self.line.limit_ms(),
            self.line.frozen_hash(),
            if self.line.verify_integrity() { "无篡改" } else { "失守" }
        ));
        lines.push(format!(
            "判据 UNX-F0664-J1：T0→T3 ≤{}ms 判定 {} 次全过 · 超阈拦截检出 100% · 账行三要素齐",
            HARD_LIMIT_MS, RUN_COUNT
        ));
        lines.push(format!(
            "-- 判定账行（在账 {}/{} 笔）--",
            self.book.len(),
            self.book.cap()
        ));
        // 尾段固定 8 行预留（五数/归因/事件/案件/健康/指纹/截断注/结尾）。
        let reserve_tail = 8usize;
        let row_budget = (cap.saturating_sub(lines.len() + reserve_tail)).saturating_add(1);
        let recs = self.book.records();
        let mut truncated = 0usize;
        let mut i = 0usize;
        while i < recs.len() {
            if i < row_budget {
                let r = &recs[i];
                let verdict_txt = match r.row.verdict {
                    Verdict::Pass { slack_ms } => format!("通过(余量{}ms)", slack_ms),
                    Verdict::Intercept { over_ms } => format!("拦截(超{}ms)", over_ms),
                };
                let attr_txt = match r.attributed {
                    Option::None => String::from(""),
                    Option::Some(k) => format!("→{}", k.short()),
                };
                lines.push(format!(
                    "#{:03} {}ms {}{}",
                    r.row.run_seq, r.row.total_ms, verdict_txt, attr_txt
                ));
            } else {
                truncated = truncated.saturating_add(1);
            }
            i += 1;
        }
        match self.summary() {
            Ok(s) => lines.push(format!("五数+P95: {}", s.render_line())),
            Err(_) => lines.push(String::from("五数+P95: 空账未统计")),
        }
        lines.push(format!("超阈归因: {}", self.attribution.render_line()));
        lines.push(format!(
            "事件账: 窗口 {} 条 · 驱逐 {} 条 · 序号连续 {}",
            self.events.len(),
            self.events.evicted(),
            if self.events.seq_ok() { "是" } else { "断链" }
        ));
        lines.push(format!(
            "案件账: 在案 {} 条 · 驱逐 {} 条 · 红灯级 {}",
            self.cases.len(),
            self.cases.evicted(),
            if self.cases.has_red_grade() { "有" } else { "无" }
        ));
        lines.push(format!(
            "健康度: {} · 拦截检出100%核: {}",
            self.health().label(),
            if self.interception_full() { "过" } else { "未过" }
        ));
        lines.push(format!("全量指纹: {:016x}", self.fingerprint()));
        if truncated > 0 {
            lines.push(format!(
                "[截断注] 账行超出报告行数上限 {}，截断 {} 行（截断事实落账，零静默吞行）",
                cap, truncated
            ));
        }
        lines.push(String::from("== 报告结尾 =="));
        let mut text = String::new();
        let mut j = 0usize;
        while j < lines.len() {
            if j > 0 {
                text.push('\n');
            }
            text.push_str(&lines[j]);
            j += 1;
        }
        ReportOutcome {
            text,
            total_lines: lines.len().saturating_add(truncated),
            rendered_lines: lines.len(),
            truncated_lines: truncated,
            fingerprint: self.fingerprint(),
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// 报告产物
// ═══════════════════════════════════════════════════════════════════════════

/// 文本报告产物（截断落账：truncated_lines = 被截账行数）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReportOutcome {
    /// 报告全文（\n 分行）。
    pub text: String,
    /// 总行数（含截断）。
    pub total_lines: usize,
    /// 实际渲染行数。
    pub rendered_lines: usize,
    /// 截断行数（仅账行段可截）。
    pub truncated_lines: usize,
    /// 报告指纹（= 引擎全量指纹）。
    pub fingerprint: u64,
}

// ═══════════════════════════════════════════════════════════════════════════
// 子系统 10：注入自检 + 双跑对拍 + 三机聚合（自由函数面）
// ═══════════════════════════════════════════════════════════════════════════

/// 注入自检（干净账上依次跑三注入——全部必须检出；
/// 任一未检出即以 `InjectionUncaught` 失败并已红灯立案于自检账）。
/// 种子写死：0xF0664_CAFEBABE（对拍确定性）。
pub const INJECTION_SELFTEST_SEED: u64 = 0xF0664_CAFE_BABE;

/// 注入自检（返回三注入结果；`Err` 即存在未检出注入——判据可信性红灯）。
pub fn injection_selfcheck(machine: MachineId) -> Result<Vec<InjectionOutcome>, BootThError> {
    let mut eng = BootThEngine::new(BootThConfig::standard(), machine, 1_735_800_000)?;
    let mut rng = Lcg::new(INJECTION_SELFTEST_SEED);
    // 预热两笔干净样本（账面成型后再注入）。
    let _ = eng.record_synthetic(SpanProfile::NominalMid, &mut rng)?;
    let _ = eng.record_synthetic(SpanProfile::NominalTight, &mut rng)?;
    let mut outs: Vec<InjectionOutcome> = Vec::new();
    outs.push(eng.inject_overrun(SegmentKind::KernelT2T3, &mut rng)?);
    outs.push(eng.inject_overrun(SegmentKind::FirmwareT0T1, &mut rng)?);
    outs.push(eng.inject_seq_gap(3, &mut rng)?);
    outs.push(eng.inject_conservation_break(977, &mut rng)?);
    Ok(outs)
}

/// 双跑确定性对拍（同画像序列同种子两引擎——指纹全等即确定性成立）。
pub fn dual_run_check(
    machine: MachineId,
    profiles: &[SpanProfile],
    seed: u64,
    opened_at: u64,
) -> Result<(u64, u64, bool), BootThError> {
    let cfg = BootThConfig::standard();
    let mut a = BootThEngine::new(cfg, machine, opened_at)?;
    let mut b = BootThEngine::new(cfg, machine, opened_at)?;
    let mut rng_a = Lcg::new(seed);
    let mut rng_b = Lcg::new(seed);
    let mut i = 0usize;
    while i < profiles.len() {
        let pa = profiles[i];
        let _ = a.record_synthetic(pa, &mut rng_a)?;
        let _ = b.record_synthetic(pa, &mut rng_b)?;
        i += 1;
    }
    let fa = a.fingerprint();
    let fb = b.fingerprint();
    Ok((fa, fb, fa == fb))
}

/// 三机矩阵单机 J1 摘要行。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MachineJ1Line {
    /// 机号。
    pub machine: MachineId,
    /// 50 次全过？
    pub all_pass: bool,
    /// 拦截检出 100%？
    pub intercept_full: bool,
    /// 账行三要素齐？
    pub rows_complete: bool,
    /// 单机 J1 总判。
    pub j1: bool,
    /// 该账最大耗时（ms）。
    pub max_ms: u64,
}

/// 三机矩阵 J1 聚合报告（3/3 全过核）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TripleJ1Report {
    /// 三机摘要行（按入参序）。
    pub lines: [MachineJ1Line; MACHINE_COUNT],
    /// 三机全 J1？
    pub all_j1: bool,
    /// 聚合指纹。
    pub fingerprint: u64,
}

/// 三机机号互异核（含数量恰 3）。
fn machines_distinct(engines: &[&BootThEngine]) -> bool {
    if engines.len() != MACHINE_COUNT {
        return false;
    }
    let mut i = 0usize;
    while i < engines.len() {
        let mut j = i + 1;
        while j < engines.len() {
            if engines[i].machine() == engines[j].machine() {
                return false;
            }
            j += 1;
        }
        i += 1;
    }
    true
}

/// 三机矩阵聚合（三引擎须互异机号且均已封账；J1 3/3 核）。
pub fn aggregate_triple(
    engines: &[&BootThEngine],
) -> Result<TripleJ1Report, BootThError> {
    if engines.len() != MACHINE_COUNT {
        return Err(BootThError::ConfigInvalid {
            field: "triple.engines",
            reason: "三机矩阵聚合须恰 3 台引擎（M0/M1/M2）",
        });
    }
    if !machines_distinct(engines) {
        return Err(BootThError::ConfigInvalid {
            field: "triple.machines",
            reason: "三机机号必须互异（M0/M1/M2 各一）",
        });
    }
    let mut lines: [MachineJ1Line; MACHINE_COUNT] = [
        MachineJ1Line {
            machine: MachineId::M0,
            all_pass: false,
            intercept_full: false,
            rows_complete: false,
            j1: false,
            max_ms: 0,
        },
        MachineJ1Line {
            machine: MachineId::M0,
            all_pass: false,
            intercept_full: false,
            rows_complete: false,
            j1: false,
            max_ms: 0,
        },
        MachineJ1Line {
            machine: MachineId::M0,
            all_pass: false,
            intercept_full: false,
            rows_complete: false,
            j1: false,
            max_ms: 0,
        },
    ];
    let mut all_j1 = true;
    let mut h = fnv1a64_from(FNV_OFFSET, BOOTTH_VERSION.as_bytes());
    let mut i = 0usize;
    while i < engines.len() {
        let rep = engines[i].verify_j1()?;
        lines[i] = MachineJ1Line {
            machine: rep.machine,
            all_pass: rep.all_pass,
            intercept_full: rep.intercept_detected_all,
            rows_complete: rep.rows_triple_complete,
            j1: rep.j1,
            max_ms: rep.summary.max_ms,
        };
        if !rep.j1 {
            all_j1 = false;
        }
        h = fnv1a64_from(h, &rep.fingerprint.to_le_bytes());
        i += 1;
    }
    let a: u8 = if all_j1 { 1 } else { 0 };
    h = fnv1a64_from(h, &[a]);
    Ok(TripleJ1Report {
        lines,
        all_j1,
        fingerprint: h,
    })
}

// ═══════════════════════════════════════════════════════════════════════════
// 测试（cfg(test) —— 冒烟 + 判据 + 注入 + 对拍）
// ═══════════════════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::vec::Vec;

    /// 名义画像轮换（三档循环——50 次覆盖三档）。
    fn nominal_cycle(i: usize) -> SpanProfile {
        match i % 3 {
            0 => SpanProfile::NominalTight,
            1 => SpanProfile::NominalMid,
            _ => SpanProfile::NominalHigh,
        }
    }

    /// 干净引擎（标准配置 + 机 0 + 固定开账时戳）。
    fn fresh_engine() -> BootThEngine {
        BootThEngine::new(BootThConfig::standard(), MachineId::M0, 1_735_800_000)
            .expect("引擎构造")
    }

    /// 干净账灌满 n 笔（确定性种子 0x0664）。
    fn fill_clean(eng: &mut BootThEngine, n: usize) {
        let mut rng = Lcg::new(0x0664);
        let mut i = 0usize;
        while i < n {
            let _ = eng
                .record_synthetic(nominal_cycle(i), &mut rng)
                .expect("干净样本入账");
            i += 1;
        }
    }

    // —— 冻结常量与诊断段 ——

    #[test]
    fn frozen_constants_and_diag_segment() {
        assert_eq!(SEG_BASE, 0x1FF0);
        assert_eq!(SEG_BASE + SEG_MASK, 0x1FFF);
        assert_eq!(HARD_LIMIT_MS, 3_000);
        assert_eq!(HARD_LIMIT_US, 3_000_000);
        assert_eq!(RUN_COUNT, 50);
        assert_eq!(MACHINE_COUNT, 3);
        // 16 诊断码一码一义：错误枚举逐变体映射段内互异。
        let samples = [
            BootThError::PhaseViolation { expect: "a", got: "b" },
            BootThError::SeqGap { expect: 1, got: 2 },
            BootThError::AnchorDisorder { anchor: 1, prev_us: 1, got_us: 0 },
            BootThError::ConservationBroken { computed_us: 1, declared_us: 2 },
            BootThError::ThawAttempt { frozen_ms: 3000, attempted_ms: 1 },
            BootThError::CalibInvalid { step: "s" },
            BootThError::Overflow { op: "o" },
            BootThError::CapacityFull { what: "w", cap: 1 },
            BootThError::UnknownMachine { code: 9 },
            BootThError::ConfigInvalid { field: "f", reason: "r" },
            BootThError::EventSeqBroken { expect: 1, got: 2 },
            BootThError::InjectionUncaught { kind: "k" },
            BootThError::InsufficientSamples { have: 0, need: 1 },
            BootThError::SealViolation { run_seq: 0 },
            BootThError::RowIncomplete { run_seq: 0, missing: "m" },
            BootThError::DuplicateRun { have: 1 },
        ];
        let mut i = 0usize;
        while i < samples.len() {
            assert_eq!(samples[i].diag_code(), SEG_BASE + i as u16, "变体 {} 锆位", i);
            assert!(!samples[i].detail().is_empty());
            assert!(!samples[i].short().is_empty());
            i += 1;
        }
    }

    // —— 测量账：段分解 + 守恒 + 主导段 ——

    #[test]
    fn span_segments_conservation_and_dominant() {
        // 手构：T0=1_000_000, T1=1_400_000, T2=1_900_000, T3=3_100_000（us）
        // 段：固件 400ms / 早传 500ms / 内核 1,200ms；总 2,100ms。
        let span = BootSpan::new(
            MachineId::M1,
            7,
            [1_000_000, 1_400_000, 1_900_000, 3_100_000],
            2_100_000,
        )
        .expect("手构账");
        assert_eq!(span.seg_us(SegmentKind::FirmwareT0T1), 400_000);
        assert_eq!(span.seg_us(SegmentKind::EarlyXferT1T2), 500_000);
        assert_eq!(span.seg_us(SegmentKind::KernelT2T3), 1_200_000);
        assert_eq!(span.seg_ms(SegmentKind::KernelT2T3), 1_200);
        assert_eq!(span.total_us(), 2_100_000);
        assert_eq!(span.total_ms(), 2_100);
        // 守恒恒等：Σ三段 ≡ T0→T3。
        assert_eq!(span.seg_sum_us(), span.total_us());
        assert!(span.conservation_ok());
        assert!(span.anchors_monotonic());
        // 主导段 = 内核段。
        assert_eq!(span.dominant_segment(), SegmentKind::KernelT2T3);
        // 申报位篡改 → 守恒失守。
        let mut bad = span.clone();
        bad.tamper_declared_total(2_100_001);
        assert!(!bad.conservation_ok());
        // 并列主导取最早段。
        let tie = BootSpan::new(
            MachineId::M0,
            1,
            [0, 500_000, 1_000_000, 1_500_000],
            1_500_000,
        )
        .expect("并列账");
        assert_eq!(tie.dominant_segment(), SegmentKind::FirmwareT0T1);
    }

    #[test]
    fn anchor_disorder_and_run_seq_rejected() {
        // T2 早于 T1 → 拒。
        let e = BootSpan::new(
            MachineId::M0,
            1,
            [100, 200, 150, 300],
            200,
        )
        .unwrap_err();
        assert_eq!(e.diag_code(), D_ANCHOR_DISORDER);
        // 次序号 0 → 三要素不齐。
        let e2 = BootSpan::new(MachineId::M0, 0, [100, 200, 300, 400], 300).unwrap_err();
        assert_eq!(e2.diag_code(), D_ROW_INCOMPLETE);
        // 篡改锚点后 checked 段宽下溢显性化。
        let mut s = BootSpan::new(MachineId::M0, 2, [100, 200, 300, 400], 300).expect("账");
        s.tamper_anchor(BootAnchor::T1, 500);
        assert!(!s.anchors_monotonic());
        assert!(s.seg_us_strict(SegmentKind::EarlyXferT1T2).is_err());
        assert_eq!(
            s.seg_us_strict(SegmentKind::EarlyXferT1T2)
                .unwrap_err()
                .diag_code(),
            D_OVERFLOW
        );
    }

    // —— 判定器：微秒域边界 + 账行三要素 ——

    #[test]
    fn judge_boundary_and_row_verify() {
        let line = ThresholdLine::frozen();
        assert!(line.verify_integrity());
        assert_eq!(line.limit_ms(), 3_000);
        let judge = ThresholdJudge::from_line(&line).expect("判定器");
        // 边界：恰 3,000,000us 判过（≤ 硬线含等号）；+1us 即拦截。
        assert_eq!(judge.judge_us(3_000_000), Verdict::Pass { slack_ms: 0 });
        assert_eq!(
            judge.judge_us(3_000_001),
            Verdict::Intercept { over_ms: 1 }
        );
        assert_eq!(judge.judge_us(2_000_000), Verdict::Pass { slack_ms: 1_000 });
        // 纯样本集复核：超阈全拦截（检出 100% 判据的判定器面）。
        let mut samples: Vec<u64> = Vec::new();
        let mut us = 2_900_000u64;
        while us <= 3_100_000 {
            samples.push(us);
            us += 1_000;
        }
        assert!(judge.verify_intercept_full(&samples));
        // 账行三要素：在位 + 结论对耗时重判一致。
        let span = BootSpan::new(
            MachineId::M0,
            3,
            [0, 300_000, 800_000, 2_500_000],
            2_500_000,
        )
        .expect("账");
        let row = judge.row_for(&span);
        assert!(row.triple_complete());
        assert!(row.verify_against_us(3_000, 2_500_000).is_ok());
        // 虚记复核：结论改篡后必须失守。
        let forged = JudgeRow {
            run_seq: 3,
            total_ms: 2_500,
            verdict: Verdict::Intercept { over_ms: 1 },
        };
        assert!(forged.verify_against_us(3_000, 2_500_000).is_err());
    }

    // —— 50 次全过 + J1 聚合（主判据冒烟）——

    #[test]
    fn fifty_runs_all_pass_and_j1() {
        let mut eng = fresh_engine();
        assert_eq!(eng.phase(), EnginePhase::Recording);
        fill_clean(&mut eng, RUN_COUNT);
        assert_eq!(eng.book().len(), 50);
        assert!(eng.book().is_sealed());
        assert_eq!(eng.phase(), EnginePhase::Sealed);
        let rep = eng.verify_j1().expect("J1 聚合");
        assert_eq!(rep.runs, 50);
        assert_eq!(rep.pass_count, 50);
        assert_eq!(rep.intercept_count, 0);
        assert!(rep.all_pass, "50 次全过");
        assert!(rep.intercept_detected_all, "拦截检出 100%（空超阈平凡过）");
        assert!(rep.rows_triple_complete, "账行三要素齐");
        assert!(rep.seq_continuous, "次序号连续零跳号");
        assert!(rep.conservation_all, "守恒全平");
        assert!(rep.j1, "J1 总判");
        assert!(rep.summary.within_limit(3_000), "分布面 max ≤ 硬线");
        assert_eq!(eng.health(), Health::Green);
        // 事件面：开账/入账/判过/封账齐。
        assert!(eng.events().count_of(EventKind::BookOpen) >= 1);
        assert_eq!(eng.events().count_of(EventKind::RunRecorded), 50);
        assert_eq!(eng.events().count_of(EventKind::Pass), 50);
        assert!(eng.events().count_of(EventKind::Sealed) >= 1);
        assert!(eng.events().seq_ok());
    }

    // —— 超阈拦截 100% + 归因分布 ——

    #[test]
    fn over_threshold_interception_100pct_and_attribution() {
        let mut eng = fresh_engine();
        let mut rng = Lcg::new(0xBEEF);
        let overs = [
            SpanProfile::OverFirmware,
            SpanProfile::OverEarlyXfer,
            SpanProfile::OverKernel,
        ];
        let mut i = 0usize;
        while i < overs.len() {
            let _ = eng.record_synthetic(overs[i], &mut rng).expect("超阈入账");
            let _ = eng.record_synthetic(SpanProfile::NominalMid, &mut rng).expect("名义入账");
            i += 1;
        }
        // 6 笔中 3 笔超阈：全部判拦截 + 立案 + 归因（三口径对齐 = 检出 100%）。
        assert_eq!(eng.book().len(), 6);
        assert_eq!(eng.book().intercept_count(), 3);
        assert_eq!(eng.over_limit_seen(), 3);
        assert_eq!(eng.cases().count_of(CaseKind::OverThreshold), 3);
        assert!(eng.interception_full(), "超阈拦截检出 100%");
        assert!(eng.judge().verify_intercept_full(
            &eng
                .book()
                .records()
                .iter()
                .map(|r| r.total_us_exact)
                .collect::<Vec<u64>>()
                .as_slice()
        ));
        // 归因分布：三段各 1 笔 → 3333/3333/3334（Σ=10000 守恒）。
        let counts = eng.attribution().counts();
        assert_eq!(counts, [1, 1, 1]);
        let bp = eng.attribution().distribution_bp();
        assert_eq!(bp[0] + bp[1] + bp[2], 10_000);
        assert_eq!(bp, [3333, 3333, 3334]);
        assert!(eng.attribution().distribution_sum_ok());
        // 每笔归因 = 画像主导段。
        let recs = eng.book().records();
        assert_eq!(recs[0].attributed, Option::Some(SegmentKind::FirmwareT0T1));
        assert_eq!(recs[2].attributed, Option::Some(SegmentKind::EarlyXferT1T2));
        assert_eq!(recs[4].attributed, Option::Some(SegmentKind::KernelT2T3));
        // 名义笔无归因位。
        assert_eq!(recs[1].attributed, Option::None);
    }

    // —— 五数概括（手算对照 + n=50 索引口径）——

    #[test]
    fn five_number_summary_hand_computed() {
        // n=10 等差 10..100：p25=idx2=30 / 中位=idx4=50 / p75=idx7=80 / p95=idx9=100。
        let s = FiveNumSummary::from_ms_samples(&[10, 20, 30, 40, 50, 60, 70, 80, 90, 100])
            .expect("五数");
        assert_eq!(s.n, 10);
        assert_eq!(s.min_ms, 10);
        assert_eq!(s.p25_ms, 30);
        assert_eq!(s.median_ms, 50);
        assert_eq!(s.p75_ms, 80);
        assert_eq!(s.max_ms, 100);
        assert_eq!(s.p95_ms, 100);
        assert_eq!(s.spread_ms(), 90);
        assert_eq!(s.iqr_ms(), 50);
        // 最近秩索引 n=50 口径。
        assert_eq!(nearest_rank_index(50, 25), 12);
        assert_eq!(nearest_rank_index(50, 50), 24);
        assert_eq!(nearest_rank_index(50, 75), 37);
        assert_eq!(nearest_rank_index(50, 95), 47);
        // n=50 实账五数（干净引擎满账）。
        let mut eng = fresh_engine();
        fill_clean(&mut eng, RUN_COUNT);
        let sum = eng.summary().expect("五数");
        assert_eq!(sum.n, 50);
        assert!(sum.min_ms >= 1);
        assert!(sum.min_ms <= sum.p25_ms);
        assert!(sum.p25_ms <= sum.median_ms);
        assert!(sum.median_ms <= sum.p75_ms);
        assert!(sum.p75_ms <= sum.p95_ms);
        assert!(sum.p95_ms <= sum.max_ms);
        assert!(sum.max_ms < 3_000);
        // 空账 → InsufficientSamples。
        let empty = fresh_engine();
        assert_eq!(
            empty.summary().unwrap_err().diag_code(),
            D_INSUFFICIENT
        );
    }

    // —— 注入面三检出 ——

    #[test]
    fn injections_all_caught() {
        let outs = injection_selfcheck(MachineId::M0).expect("注入自检");
        assert_eq!(outs.len(), 4);
        assert!(outcomes_all_caught(&outs), "三类注入全检出");
        // 种类覆盖：超阈×2（内核/固件）+ 跳号 + 守恒破坏。
        assert_eq!(outs[0].kind, InjectionKind::Overrun(SegmentKind::KernelT2T3));
        assert_eq!(outs[1].kind, InjectionKind::Overrun(SegmentKind::FirmwareT0T1));
        assert_eq!(outs[2].kind, InjectionKind::SeqGap);
        assert_eq!(outs[3].kind, InjectionKind::ConservationBreak);
        // 引擎级单注入面（机 1 独立账）：早传主导超阈注入。
        let mut eng = BootThEngine::new(BootThConfig::standard(), MachineId::M1, 1)
            .expect("引擎");
        let mut rng = Lcg::new(0x51);
        let out = eng
            .inject_overrun(SegmentKind::EarlyXferT1T2, &mut rng)
            .expect("超阈注入检出");
        assert!(out.caught);
        assert!(eng.interception_full());
    }

    // —— 双跑确定性对拍 ——

    #[test]
    fn dual_run_deterministic() {
        let mut profiles: Vec<SpanProfile> = Vec::new();
        let mut i = 0usize;
        while i < RUN_COUNT {
            profiles.push(nominal_cycle(i));
            i += 1;
        }
        let (fa, fb, eq) =
            dual_run_check(MachineId::M2, &profiles, 0xD0A1, 1_735_800_777).expect("对拍");
        assert!(eq, "双跑指纹全等：{:016x} vs {:016x}", fa, fb);
        // 混入超阈档对拍（拦截路径亦确定）。
        let mut mixed: Vec<SpanProfile> = profiles.clone();
        mixed[10] = SpanProfile::OverKernel;
        mixed[20] = SpanProfile::OverFirmware;
        let (fa2, fb2, eq2) =
            dual_run_check(MachineId::M0, &mixed, 0xD0A2, 1_735_800_778).expect("对拍2");
        assert!(eq2, "混超阈双跑指纹全等：{:016x} vs {:016x}", fa2, fb2);
        assert_ne!(fa, fa2, "不同输入指纹必异");
    }

    // —— 阈值变更纪律：解冻恒拒 + 校准规程 + 对接声明 ——

    #[test]
    fn thaw_blocked_and_calibration_discipline() {
        let line = ThresholdLine::frozen();
        // 直接改写恒拒。
        let e = line.attempt_direct_change(2_500).unwrap_err();
        assert_eq!(e.diag_code(), D_THAW);
        assert_eq!(line.limit_ms(), 3_000, "硬线原封不动");
        assert!(line.verify_integrity());
        // 校准三步：采样 → 统计 → 冻结提案。
        let mut gate = CalibrationGate::new();
        assert_eq!(gate.phase(), CalibPhase::Idle);
        // 未开采样即推样 → PhaseViolation。
        assert_eq!(
            gate.push_sample(1_000).unwrap_err().diag_code(),
            D_PHASE
        );
        gate.begin_sampling().expect("开采样");
        // 重复开采样 → PhaseViolation。
        assert!(gate.begin_sampling().is_err());
        // 样本不足即收口 → InsufficientSamples。
        let mut k = 0u64;
        while k < 19 {
            gate.push_sample(1_000 + k * 10).expect("采样");
            k += 1;
        }
        assert_eq!(
            gate.conclude_statistics().unwrap_err().diag_code(),
            D_INSUFFICIENT
        );
        gate.push_sample(1_000 + 19 * 10).expect("第 20 笔");
        let prop = gate.conclude_statistics().expect("统计收口");
        assert_eq!(gate.phase(), CalibPhase::Proposed);
        // 样本升序 1000..1190 步长 10（n=20）：p95 idx=(95*20+99)/100−1=18 → 1180；
        // 提案 = 1180×4/3 = 4720/3 = 1573（整数除法）。
        assert_eq!(prop.p95_ms, 1_180);
        assert_eq!(prop.proposed_ms, 1_573);
        assert_eq!(prop.n_samples, 20);
        assert_eq!(prop.adr_ref, "ADR-UNX-F0665");
        // 落地恒拒（F0665-J1：哈希封存 + 变更走 ADR）。
        assert_eq!(
            gate.apply_to_line(&prop, &line).unwrap_err().diag_code(),
            D_CALIB_INVALID
        );
        assert_eq!(line.limit_ms(), 3_000, "落地零改写");
        // 引擎对接声明：成型提案可声明（事件留痕），未成型不可声明。
        let mut eng = fresh_engine();
        assert_eq!(
            eng.declare_calibration(&gate).expect("声明").to_le_bytes().len(),
            8
        );
        assert!(eng.events().count_of(EventKind::CalibDeclared) >= 1);
        assert_eq!(eng.line().limit_ms(), 3_000, "声明后仍零改写");
        let empty_gate = CalibrationGate::new();
        assert_eq!(
            eng.declare_calibration(&empty_gate).unwrap_err().diag_code(),
            D_CALIB_INVALID
        );
        // 引擎解冻面：恒拒 + Thaw 案 + 事件 + 健康红。
        assert!(eng.attempt_thaw(2_000).is_err());
        assert!(eng.cases().has(CaseKind::Thaw));
        assert!(eng.events().count_of(EventKind::ThawBlocked) >= 1);
        assert_eq!(eng.health(), Health::Red);
        // 复位规程（Proposed → Idle，可重跑）。
        gate.reset().expect("复位");
        assert_eq!(gate.phase(), CalibPhase::Idle);
    }

    // —— 跳号 / 重复 / 封账守卫 ——

    #[test]
    fn seq_gap_duplicate_and_seal_guards() {
        let mut eng = fresh_engine();
        fill_clean(&mut eng, 5);
        let mut rng = Lcg::new(0x99);
        // 跳号：next=6，投 9 → SeqGap 拒收 + 案 + 事件。
        let span = synth_span(MachineId::M0, 9, SpanProfile::NominalMid, &mut rng)
            .expect("跳号样");
        let e = eng.record_span(span).unwrap_err();
        assert_eq!(e.diag_code(), D_SEQ_GAP);
        assert!(eng.cases().has(CaseKind::SeqGap));
        assert!(eng.events().count_of(EventKind::SeqGapCaught) >= 1);
        assert_eq!(eng.book().len(), 5, "拒收不入账");
        // 重复：投 3（已在账）→ DuplicateRun 拒收 + 案。
        let dup = synth_span(MachineId::M0, 3, SpanProfile::NominalMid, &mut rng)
            .expect("重复样");
        let e2 = eng.record_span(dup).unwrap_err();
        assert_eq!(e2.diag_code(), D_DUPLICATE);
        assert!(eng.cases().has(CaseKind::Duplicate));
        // 续满 50 → 封账；再写 → SealViolation。
        let mut i = 5usize;
        let mut rng2 = Lcg::new(0x0665);
        while i < RUN_COUNT {
            let _ = eng
                .record_synthetic(nominal_cycle(i), &mut rng2)
                .expect("续满");
            i += 1;
        }
        assert!(eng.book().is_sealed());
        let extra = synth_span(MachineId::M0, 51, SpanProfile::NominalMid, &mut rng2)
            .expect("超额样");
        let e3 = eng.record_span(extra).unwrap_err();
        assert_eq!(e3.diag_code(), D_SEAL);
        // 次序连续复核（账面 1..=50 零跳号）。
        assert!(eng.book().seq_continuous());
        let seqs: Vec<u32> = eng.book().records().iter().map(|r| r.row.run_seq).collect();
        assert_eq!(seqs.first(), Option::Some(&1));
        assert_eq!(seqs.last(), Option::Some(&50));
    }

    // —— 守恒破坏拒收（引擎面）——

    #[test]
    fn conservation_break_rejected_with_case() {
        let mut eng = fresh_engine();
        let mut rng = Lcg::new(0xCC);
        let _ = eng.record_synthetic(SpanProfile::NominalMid, &mut rng).expect("干净");
        let seq = eng.book().next_expected_seq();
        let mut bad = synth_span(MachineId::M0, seq, SpanProfile::NominalMid, &mut rng)
            .expect("样");
        bad.tamper_declared_total(bad.declared_total_us() + 1);
        let e = eng.record_span(bad).unwrap_err();
        assert_eq!(e.diag_code(), D_CONSERVATION);
        assert!(eng.cases().has(CaseKind::Conservation));
        assert!(eng.events().count_of(EventKind::ConservationBroken) >= 1);
        assert_eq!(eng.health(), Health::Red, "守恒案 = 红灯级");
        assert_eq!(eng.book().len(), 1, "破坏样本未入账");
    }

    // —— 事件账：序号连续 + 篡改审计 ——

    #[test]
    fn event_ledger_seq_and_tamper_audit() {
        let mut led = EventLedger::new(8).expect("事件账");
        let mut i = 0u64;
        while i < 12 {
            led.push(EventKind::RunRecorded, (i as u32) + 1, MachineId::M0, i);
            i += 1;
        }
        assert_eq!(led.len(), 8, "环形窗口 8");
        assert_eq!(led.evicted(), 4, "驱逐 4 落账");
        assert!(led.seq_ok());
        assert!(led.audit().is_ok());
        // 篡改序号 → 审计断链。
        led.tamper_seq(3, 999);
        assert!(!led.seq_ok());
        let e = led.audit().unwrap_err();
        assert_eq!(e.diag_code(), D_EVENT_SEQ);
        // 容量下限守卫。
        assert!(EventLedger::new(4).is_err());
    }

    // —— 报告渲染 + 截断落账 ——

    #[test]
    fn report_render_and_truncation() {
        let mut eng = fresh_engine();
        fill_clean(&mut eng, RUN_COUNT);
        let out = eng.render_report();
        assert!(out.text.contains("UNX-F0664"));
        assert!(out.text.contains("判定账行"));
        assert!(out.text.contains("五数+P95"));
        assert!(out.text.contains("超阈归因"));
        assert!(out.text.contains("健康度"));
        assert_eq!(out.truncated_lines, 0, "标准上限容得下 50 行账行");
        assert!(out.rendered_lines <= 80);
        // 窄上限配置：截断只落在账行段且截断数落账。
        let mut cfg = BootThConfig::standard();
        cfg.report_line_cap = 48;
        let mut eng2 =
            BootThEngine::new(cfg, MachineId::M2, 1).expect("窄报告引擎");
        fill_clean(&mut eng2, RUN_COUNT);
        let out2 = eng2.render_report();
        assert!(out2.truncated_lines > 0, "窄上限必截账行");
        assert!(out2.text.contains("截断注"));
        assert_eq!(
            out2.total_lines,
            out2.rendered_lines + out2.truncated_lines,
            "总行 = 渲染 + 截断（守恒）"
        );
    }

    // —— 三机矩阵聚合 ——

    #[test]
    fn triple_machine_aggregate() {
        let machines = [MachineId::M0, MachineId::M1, MachineId::M2];
        let mut engs: Vec<BootThEngine> = Vec::new();
        let mut refs: Vec<&BootThEngine> = Vec::new();
        let mut i = 0usize;
        while i < machines.len() {
            let mut e = BootThEngine::new(BootThConfig::standard(), machines[i], 100 + i as u64)
                .expect("引擎");
            fill_clean(&mut e, RUN_COUNT);
            engs.push(e);
            i += 1;
        }
        let mut j = 0usize;
        while j < engs.len() {
            refs.push(&engs[j]);
            j += 1;
        }
        let rep = aggregate_triple(&refs).expect("聚合");
        assert!(rep.all_j1, "三机 J1 3/3");
        assert_eq!(rep.lines.len(), 3);
        let mut k = 0usize;
        while k < rep.lines.len() {
            assert!(rep.lines[k].j1);
            assert!(rep.lines[k].max_ms < 3_000);
            k += 1;
        }
        // 机号重复 → 拒。
        let dup_ref = [&engs[0] as &BootThEngine, &engs[0], &engs[2]];
        assert!(aggregate_triple(&dup_ref).is_err());
        // 数量错 → 拒。
        let short = [&engs[0] as &BootThEngine, &engs[1]];
        let e = aggregate_triple(&short).unwrap_err();
        assert_eq!(e.diag_code(), D_CONFIG_INVALID);
        // 未封账 → PhaseViolation（机号取 M2 保持三机互异，先过机号核再触相位核）。
        let mut open_eng = BootThEngine::new(BootThConfig::standard(), MachineId::M2, 1)
            .expect("未封引擎");
        fill_clean(&mut open_eng, 10);
        let triple = [&engs[0] as &BootThEngine, &engs[1], &open_eng];
        let e2 = aggregate_triple(&triple).unwrap_err();
        assert_eq!(e2.diag_code(), D_PHASE);
    }

    // —— 配置校验 ——

    #[test]
    fn config_validation_rejects_out_of_domain() {
        let base = BootThConfig::standard();
        // run_count ≠ 50。
        let mut c1 = base;
        c1.run_count = 49;
        assert_eq!(c1.validate().unwrap_err().diag_code(), D_CONFIG_INVALID);
        // 硬线私改。
        let mut c2 = base;
        c2.hard_limit_ms = 2_500;
        assert!(c2.validate().is_err());
        // 容量越界。
        let mut c3 = base;
        c3.event_cap = 8;
        assert!(c3.validate().is_err());
        let mut c4 = base;
        c4.case_cap = 8;
        assert!(c4.validate().is_err());
        let mut c5 = base;
        c5.report_line_cap = 10;
        assert!(c5.validate().is_err());
        // 私改线判定器构建也拒（防夹带）。
        let mut forged_line = ThresholdLine::frozen();
        forged_line.tamper_limit(2_500);
        assert!(ThresholdJudge::from_line(&forged_line).is_err());
        // 机号解码。
        assert!(BootThEngine::with_machine_code(base, 9, 0).is_err());
        // 硬线篡改 → 完整性失守 → 健康红。
        let mut eng = fresh_engine();
        eng.tamper_line_limit(1);
        assert!(!eng.line_integrity_ok());
        assert_eq!(eng.health(), Health::Red);
    }

    // —— 健康阶梯 ——

    #[test]
    fn health_ladder() {
        // 录入中 → 黄。
        let mut mid = fresh_engine();
        fill_clean(&mut mid, 10);
        assert_eq!(mid.health(), Health::Amber);
        // 续满至 50 → 绿。
        fill_clean(&mut mid, RUN_COUNT - 10);
        assert_eq!(mid.health(), Health::Green);
        // 守恒案 → 红（未封账 scratch 引擎注入——封账后禁写故不可用已封账）。
        let mut red = BootThEngine::new(BootThConfig::standard(), MachineId::M1, 2)
            .expect("红灯引擎");
        let mut rng = Lcg::new(0xEE);
        let _ = red
            .record_synthetic(SpanProfile::NominalMid, &mut rng)
            .expect("预热");
        let _ = red
            .inject_conservation_break(500, &mut rng)
            .expect("注入检出");
        assert_eq!(red.health(), Health::Red);
        assert!(red.cases().has(CaseKind::Conservation));
        // 超阈案（非红级）→ 黄。
        let mut amber = fresh_engine();
        let mut rng2 = Lcg::new(0xEF);
        let _ = amber
            .record_synthetic(SpanProfile::OverKernel, &mut rng2)
            .expect("超阈");
        assert_eq!(amber.health(), Health::Amber);
        assert_eq!(amber.recorded_total(), 1);
    }

    // —— LCG 确定性 ——

    #[test]
    fn lcg_determinism() {
        let mut a = Lcg::new(42);
        let mut b = Lcg::new(42);
        let mut i = 0usize;
        while i < 16 {
            assert_eq!(a.next(), b.next());
            i += 1;
        }
        let mut c = Lcg::new(43);
        assert_ne!(a.next(), c.next(), "异种子必异序列");
        // 合成账确定性：同机同序号同画像同种子 → 指纹全等。
        let mut r1 = Lcg::new(7);
        let mut r2 = Lcg::new(7);
        let s1 = synth_span(MachineId::M0, 5, SpanProfile::NominalHigh, &mut r1)
            .expect("合成1");
        let s2 = synth_span(MachineId::M0, 5, SpanProfile::NominalHigh, &mut r2)
            .expect("合成2");
        assert_eq!(s1, s2);
        assert_eq!(s1.fingerprint(), s2.fingerprint());
    }
}
