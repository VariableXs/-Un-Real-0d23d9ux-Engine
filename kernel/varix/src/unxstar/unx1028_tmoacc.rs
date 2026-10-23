//! unx1028_tmoacc —— UNX-F1028 超时器精度账（到期误差分解与达标）。
//!
//! 锚点（CoRun Unxreal 主册）：UNX-F1028 · 超时器精度账（到期误差分解与达标） · 目标 320 行 · 已深化
//!
//! 判据原文（docs/Varix/CoRun Varix STAR II · Unxreal/CoRun Varix STAR II · Unxreal.md
//! · UNX-A2-B12 判据表 L2730，逐字抄录）：
//! ```text
//! | UNX-F1028 | 超时器精度账（到期误差分解与达标） | 320 | 已深化 | UNX-F0953→F1028-J1 误差分解齐备（设定-到期-执行三段），达标断言（全链 p99 ≤50µs QEMU 档——F1014 预算表兑现），分解和≈全链（残差 <5%） |
//! ```
//!
//! 定位原文（同册邻行，锚定上下游消费关系，逐字抄录）：
//! ```text
//! L2729 | UNX-F1027 | 超时器到期回调分发（到期→回调的调度链） | 320 | 已深化 | UNX-F1027-J1 分发正确（到期回调全部执行 10 万次——零丢失零重复），分发延迟账（到期到回调执行差值直方图），回调违禁检测 10/10 次（F0905 契约） |
//! L2731 | UNX-F1029 | 超时器取消与重设（cancel/reset 语义） | 300 | 已深化 | UNX-F1029-J1 取消正确（取消后零回调 10/10 次——竞态取消安全），重设正确（重设后新时刻到期 10/10 次），取消竞态（到期瞬间取消）10/10 次正确处理 |
//! L2712 | UNX-F1014 | 时间误差预算表（计时面 SLO 分解契约） | 300 | 已深化 | UNX-F0953→F1014-J1 预算表冻结（计时面误差源七项预算+责任条目），达标率 ≥99% 断言（F0949 同型账），版本化机制 10/10 次 |
//! ```
//!
//! 职责定位：超时器链路（F1022 单次底座 → F1023 tick → F1027 到期分发）之上
//! 的唯一权威精度账面——把「请求→回调入口」的全链到期误差拆成三段可追责的
//! 误差源，逐段落 600 样本环账，对 F1014 预算表做逐行兑现审计。
//!
//! 组件清单（全手工真实子系统，整数运算零浮点）：
//!   1. `ErrorTriplet` 三段误差分解账：
//!      设定段（请求→入轮）/ 到期段（理论到期→实际到期）/ 执行段（实际到期→回调入口），
//!      每段 600 样本环 + 段级 P50/P90/P95/P99 整数分位（最近邻秩法）。
//!   2. 全链 p99 达标面：≤50µs=50000ns QEMU 档硬线（F1014 预算表超时器行兑现判据），
//!      达标/失守二态 + 失守定位最大段 + 达标率（≥99% 同 F1014-J1 口径）。
//!   3. 分解和≈全链：Σ三段 ≈ 全链实测（外测 attest 口径），残差 ppm <5% 判据，
//!      越界自动立案（CaseFile 账）。
//!   4. 提前到期红线：到期绝不早于理论时刻——early 采样即红账，容忍窗=0。
//!   5. 精度画像：四泳道分位数曲线 + 8 桶 ns 直方图 + EWMA 误差趋势
//!      （α=1/8 移位实现）+ 基线漂移检出（连续越限计 streak）。
//!   6. 事件账 + `health()` 健康面 + FNV-1a 32 位指纹冻结 + 双跑确定性对拍
//!      + 注入核（提前到期注入 / 段丢失注入 / 残差漂移注入，各 10/10 必抓）。
//!   7. F1014 预算表冻结面：三段预算 + 全链硬线逐行审计（预算和=硬线，编译期断言）。
//!
//! 诊断码段：0x1A30~0x1A3F（本模块独占闭集；已核验空闲——
//!   0x1A00..0x1A04、0x1A2*、0x1A4*（ves477/ves478）均被占，0x1A3* 全仓 grep 零命中）。
//! 纯功能线：no_std + alloc，完全自包含（只 use core/alloc，不依赖 crate 内其他模块），
//!   内核路径零 unwrap/expect/panic/运行期 assert，整数算术显性 saturating/wrapping，
//!   >64KB 结构体一律落堆（环账/日志全 Vec 承载），确定性可对拍。
//!
//! 测试档例外说明：文末 `#[cfg(test)]` 冒烟测试使用测试Harness 的 assert!，
//! 仅在测试二进制中编译，内核路径不含。

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

/// 模块版本。
pub const TA_VERSION: &str = "unx1028_tmoacc-v1";

/// 诊断段基址（0x1A30 段 · 本模块独占 · 已核验全仓空闲）。
pub const TA_SEG_BASE: u16 = 0x1A30;

/// XcCode 闭集（0x1A31~0x1A3F，严格递增互异，全部落在 0x1A30 段）。
pub const XC_EARLY_FIRE: u32 = 0x1A31;
pub const XC_P99_BREACH: u32 = 0x1A32;
pub const XC_RESIDUAL_OOB: u32 = 0x1A33;
pub const XC_SEG_LOST: u32 = 0x1A34;
pub const XC_DRIFT_DETECTED: u32 = 0x1A35;
pub const XC_FINGER_DRIFT: u32 = 0x1A36;
pub const XC_DUALRUN_MISMATCH: u32 = 0x1A37;
pub const XC_INJECT_MISSED: u32 = 0x1A38;
pub const XC_EVENT_OVF: u32 = 0x1A39;
pub const XC_SEALED: u32 = 0x1A3A;
pub const XC_CONGESTED: u32 = 0x1A3B;
pub const XC_PARAM_BAD: u32 = 0x1A3C;
pub const XC_LEDGER_BROKEN: u32 = 0x1A3D;
pub const XC_PROFILE_EMPTY: u32 = 0x1A3E;
pub const XC_HEALTH_DOWN: u32 = 0x1A3F;

/// 编译期闭集断言：诊断码段必须严丝合缝落在 0x1A30~0x1A3F。
const _: () = assert!(
    XC_HEALTH_DOWN == (TA_SEG_BASE as u32) + 0x0F,
    "UNX-F1028：诊断码闭集溢出 0x1A30~0x1A3F 段"
);

/// 全链 p99 硬线：50µs = 50000ns（QEMU 档；来源 UNX-F1028-J1「全链 p99 ≤50µs」，
/// 与 F1022-J1「到期误差 p99 ≤50µs（QEMU 档）」同值，F1014 预算表超时器行兑现线）。
pub const CHAIN_P99_LIMIT_NS: u64 = 50_000;

/// 设定段预算：5000ns（来源：F1014 预算表超时器行三段分配——本模块冻结执行档；
/// 分配原则：入轮为纯软件路径取最小份额）。
pub const SEG_BUDGET_SETUP_NS: u64 = 5_000;

/// 到期段预算：30000ns（来源：F1014 预算表超时器行三段分配——HPET 比较器
/// 写回+中断投递占大头，取最大份额）。
pub const SEG_BUDGET_EXPIRY_NS: u64 = 30_000;

/// 执行段预算：15000ns（来源：F1014 预算表超时器行三段分配——回调分发链）。
pub const SEG_BUDGET_EXEC_NS: u64 = 15_000;

/// 编译期断言：三段预算和必须严丝合缝等于全链硬线（分解可加性是精度账的根基）。
const _: () = assert!(
    SEG_BUDGET_SETUP_NS + SEG_BUDGET_EXPIRY_NS + SEG_BUDGET_EXEC_NS == CHAIN_P99_LIMIT_NS,
    "UNX-F1028：三段预算和 ≠ 全链 50µs 硬线，F1014 预算表失配"
);

/// 达标率下限：99%（来源 UNX-F1014-J1「达标率 ≥99% 断言」，ppm 口径 990000/1000000）。
pub const ATTAIN_FLOOR_PPM: u64 = 990_000;

/// 残差上限：5%（来源 UNX-F1028-J1「分解和≈全链（残差 <5%）」，ppm 口径）。
pub const RESIDUAL_LIMIT_PPM: u64 = 50_000;

/// 段级/全链样本环容量：600（来源 UNX-F1028-J1「每段样本账 600 环」）。
pub const RING_CAP: usize = 600;

/// 段泳道号：0 设定段。
pub const LANE_SETUP: usize = 0;
/// 段泳道号：1 到期段。
pub const LANE_EXPIRY: usize = 1;
/// 段泳道号：2 执行段。
pub const LANE_EXEC: usize = 2;
/// 段泳道号：3 全链。
pub const LANE_CHAIN: usize = 3;
/// 泳道总数。
pub const LANE_N: usize = 4;

/// ns 直方图桶界（8 桶闭集：0/5k/10k/20k/40k/80k/160k，尾桶 320k+；
/// 桶界覆盖三段预算与硬线，使越线样本必落入独立桶可定位）。
pub const HIST_BOUNDS_NS: [u64; 7] = [5_000, 10_000, 20_000, 40_000, 80_000, 160_000, 320_000];

/// ns 落桶（const fn：while + 索引，无 slice::get / Ord::min / for）。
pub const fn bucket_of(v: u64) -> u8 {
    let mut b = 0u8;
    while b < 7 && v > HIST_BOUNDS_NS[b as usize] {
        b += 1;
    }
    b
}

/// EWMA 平滑系数移位：α = 1/8（右移 3 位；整数实现零浮点零除法）。
pub const EWMA_SHIFT: u32 = 3;

/// 基线冻结样本数：前 100 样本用于冻结 EWMA 基线（暖机窗）。
pub const WARMUP_SAMPLES: u64 = 100;

/// 漂移检出阈值：EWMA 相对基线偏移 >10000ns 计越限。
pub const DRIFT_LIMIT_NS: i64 = 10_000;

/// 漂移检出连续长度：连续 32 次越限才立案（防瞬时抖动误报）。
pub const DRIFT_STREAK_LEN: u32 = 32;

/// 趋势环快照窗：每 100 个样本落一拍 EWMA 趋势。
pub const TREND_WINDOW: u64 = 100;

/// 趋势环容量：64 拍（6400 样本的趋势窗）。
pub const TREND_CAP: usize = 64;

/// 事件账容量。
pub const EVENTS_CAP: usize = 256;
/// 立案账容量。
pub const CASES_CAP: usize = 64;
/// 提前红账容量。
pub const REDS_CAP: usize = 64;
/// 最近三元组账容量（ forensic 近窗）。
pub const RECENT_CAP: usize = 128;
/// 在途表容量（并发超时器上限）。
pub const INFLIGHT_CAP: usize = 4096;

/// 在途状态：已请求未入轮。
pub const IF_REQ: u8 = 1;
/// 在途状态：已入轮未到期。
pub const IF_ENQ: u8 = 2;
/// 在途状态：已到期待回调。
pub const IF_FIRED: u8 = 3;

/// 中文诊断文本（码 → 文案/建议，闭集逐码两要素齐备）。
pub fn diag_text(code: u32) -> (&'static str, &'static str) {
    match code {
        XC_EARLY_FIRE => (
            "提前到期红账：到期早于理论时刻（容忍窗=0）",
            "检查 HPET 比较器写序与回读口径，禁倒序写",
        ),
        XC_P99_BREACH => (
            "全链 p99 越线：>50µs QEMU 档硬线",
            "按失守最大段定位 F1014 预算超标项",
        ),
        XC_RESIDUAL_OOB => (
            "残差越界立案：分解和与全链实测偏差 >5%",
            "检查外测时钟口径与三段采样点是否同源",
        ),
        XC_SEG_LOST => (
            "段账缺失：设定/到期/执行三段出现断链",
            "排查 F1027 分发链与超窗在途项",
        ),
        XC_DRIFT_DETECTED => (
            "EWMA 漂移检出：全链误差趋势持续越限",
            "检查时钟源降级（F1041 仲裁）与温漂",
        ),
        XC_FINGER_DRIFT => (
            "指纹漂移：封账指纹与重算值不一致",
            "精度账被外力改写，回滚或重建账本",
        ),
        XC_DUALRUN_MISMATCH => (
            "双跑对拍不一致：同输入不同指纹",
            "排查非确定性路径（未初始化读/迭代序）",
        ),
        XC_INJECT_MISSED => (
            "注入未捕获：注入核存在漏抓",
            "三类注入（提前/段丢失/残差）必须 10/10 必抓",
        ),
        XC_EVENT_OVF => (
            "事件账溢出：事件环滚动丢弃",
            "调大 EVENTS_CAP 或及时消费快照",
        ),
        XC_SEALED => (
            "精度账已封账：封账后禁止任何写入",
            "新建账本或导出指纹后重建",
        ),
        XC_CONGESTED => (
            "在途表满：并发超时器超 4096 上限",
            "先 sweep_lost 回收弃账或提高在途预算",
        ),
        XC_PARAM_BAD => (
            "参数非法：越出闭集约束（如分位数须 1..=100）",
            "核对调用参数与常量闭集",
        ),
        XC_LEDGER_BROKEN => (
            "账结构破坏：段序倒置或重复闭段",
            "检查调用序：请求→入轮→到期→回调",
        ),
        XC_PROFILE_EMPTY => (
            "画像空账：无完整三段样本",
            "先喂入完整三段流水再取画像",
        ),
        XC_HEALTH_DOWN => (
            "精度账健康降级：红账/越线/立案/漂移任一命中",
            "按 health() 判据逐项排查",
        ),
        _ => ("未知诊断码", "查本模块 XcCode 闭集 0x1A31~0x1A3F"),
    }
}

/// 段种类（三段 + 全链泳道的类型化句柄）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SegKind {
    /// 设定段：请求→入轮。
    Setup,
    /// 到期段：理论到期→实际到期。
    Expiry,
    /// 执行段：实际到期→回调入口。
    Exec,
}

impl SegKind {
    /// 泳道号（const fn：match 枚举合法）。
    pub const fn lane(self) -> usize {
        match self {
            SegKind::Setup => LANE_SETUP,
            SegKind::Expiry => LANE_EXPIRY,
            SegKind::Exec => LANE_EXEC,
        }
    }
    /// 中文名。
    pub const fn name(self) -> &'static str {
        match self {
            SegKind::Setup => "设定段(请求→入轮)",
            SegKind::Expiry => "到期段(理论到期→实际到期)",
            SegKind::Exec => "执行段(实际到期→回调入口)",
        }
    }
}

/// 泳道号 → 段种类。
pub const fn seg_from_lane(lane: usize) -> SegKind {
    if lane == LANE_EXPIRY {
        SegKind::Expiry
    } else if lane == LANE_EXEC {
        SegKind::Exec
    } else {
        SegKind::Setup
    }
}

/// F1014 预算表冻结行（超时器行兑现面：预算值 + 达标率下限 + 责任条目）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct BudgetRow {
    /// 泳道号（0..2 段，3 全链）。
    pub lane: u8,
    /// 预算值（ns）。
    pub budget_ns: u64,
    /// 达标率下限（ppm）。
    pub attain_floor_ppm: u64,
    /// 责任条目（F1014 冻结口径）。
    pub owner: &'static str,
}

/// F1014 预算表超时器行（冻结面，const fn 可编译期取用）。
pub const fn budget_table() -> [BudgetRow; 4] {
    [
        BudgetRow {
            lane: LANE_SETUP as u8,
            budget_ns: SEG_BUDGET_SETUP_NS,
            attain_floor_ppm: ATTAIN_FLOOR_PPM,
            owner: "F1014·超时器设定段（请求→入轮）",
        },
        BudgetRow {
            lane: LANE_EXPIRY as u8,
            budget_ns: SEG_BUDGET_EXPIRY_NS,
            attain_floor_ppm: ATTAIN_FLOOR_PPM,
            owner: "F1014·超时器到期段（理论到期→实际到期）",
        },
        BudgetRow {
            lane: LANE_EXEC as u8,
            budget_ns: SEG_BUDGET_EXEC_NS,
            attain_floor_ppm: ATTAIN_FLOOR_PPM,
            owner: "F1014·超时器执行段（实际到期→回调入口）",
        },
        BudgetRow {
            lane: LANE_CHAIN as u8,
            budget_ns: CHAIN_P99_LIMIT_NS,
            attain_floor_ppm: ATTAIN_FLOOR_PPM,
            owner: "F1014·超时器全链（QEMU 档硬线 50µs）",
        },
    ]
}

/// 精度账错误枚举：每变体携带诊断码 + 中文可操作 detail。
/// 内核路径零 panic——一切可失败面走本枚举。
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum TmoAccError {
    /// 封账后写入拒绝。
    Sealed,
    /// 参数非法（参数名，实际值）。
    ParamBad(&'static str, u64),
    /// 在途表满。
    Congested,
    /// 提前到期红账（提前量 ns）。
    EarlyFire(u64),
    /// 段账缺失（首个缺失段号：0 设定 / 1 到期 / 2 执行）。
    SegLoss(u8),
    /// 残差越界立案（实测残差 ppm）。
    ResidualOob(u64),
    /// 全链 p99 越线（实测 p99 ns）。
    P99Breach(u64),
    /// EWMA 漂移检出（相对基线偏移 ns，带符号）。
    DriftDetected(i64),
    /// 指纹漂移（冻结值，重算值）。
    FingerDrift(u32, u32),
    /// 双跑对拍不一致（账 A 指纹，账 B 指纹）。
    DualRunMismatch(u32, u32),
    /// 账结构破坏（原因）。
    LedgerBroken(&'static str),
    /// 画像空账。
    ProfileEmpty,
    /// 事件账溢出（已丢弃数）。
    EventOvf(u64),
    /// 注入未捕获（注入类号）。
    InjectMissed(u32),
}

impl TmoAccError {
    /// 诊断码（0x1A30 段闭集）。
    pub const fn code(&self) -> u32 {
        match self {
            TmoAccError::Sealed => XC_SEALED,
            TmoAccError::ParamBad(_, _) => XC_PARAM_BAD,
            TmoAccError::Congested => XC_CONGESTED,
            TmoAccError::EarlyFire(_) => XC_EARLY_FIRE,
            TmoAccError::SegLoss(_) => XC_SEG_LOST,
            TmoAccError::ResidualOob(_) => XC_RESIDUAL_OOB,
            TmoAccError::P99Breach(_) => XC_P99_BREACH,
            TmoAccError::DriftDetected(_) => XC_DRIFT_DETECTED,
            TmoAccError::FingerDrift(_, _) => XC_FINGER_DRIFT,
            TmoAccError::DualRunMismatch(_, _) => XC_DUALRUN_MISMATCH,
            TmoAccError::LedgerBroken(_) => XC_LEDGER_BROKEN,
            TmoAccError::ProfileEmpty => XC_PROFILE_EMPTY,
            TmoAccError::EventOvf(_) => XC_EVENT_OVF,
            TmoAccError::InjectMissed(_) => XC_INJECT_MISSED,
        }
    }
    /// 中文可操作诊断（每变体必非空，两要素：现象 + 处置）。
    pub const fn detail(&self) -> &'static str {
        match self {
            TmoAccError::Sealed => "精度账已封账，禁止任何写入；请导出指纹后新建账本",
            TmoAccError::ParamBad(_, _) => "参数越出闭集约束（分位数须 1..=100 等）；请核对调用参数",
            TmoAccError::Congested => "在途表满 4096；先 sweep_lost 回收弃账或提高在途预算",
            TmoAccError::EarlyFire(_) => "到期早于理论时刻且容忍窗=0，必为红账；检查比较器写序与倒序写",
            TmoAccError::SegLoss(_) => "三段序断链；按缺失段号排查 F1027 分发链与在途项",
            TmoAccError::ResidualOob(_) => "分解和与全链实测偏差超 5%；检查外测时钟口径与采样点同源性",
            TmoAccError::P99Breach(_) => "全链 p99 超 50µs QEMU 档硬线；按失守最大段定位预算超标项",
            TmoAccError::DriftDetected(_) => "EWMA 相对基线持续越限；检查时钟源降级与温漂",
            TmoAccError::FingerDrift(_, _) => "封账指纹与重算不一致，账被外力改写；回滚",
            TmoAccError::DualRunMismatch(_, _) => "同输入双跑指纹不一致，存在非确定性路径；排查迭代序",
            TmoAccError::LedgerBroken(_) => "段序倒置或重复闭段；调用序须为 请求→入轮→到期→回调",
            TmoAccError::ProfileEmpty => "无完整三段样本；先喂入流水再取画像",
            TmoAccError::EventOvf(_) => "事件环已滚动丢弃；调大 EVENTS_CAP 或及时消费快照",
            TmoAccError::InjectMissed(_) => "注入核存在漏抓；三类注入必须 10/10 必抓",
        }
    }
}

/// u64 → i64 显性钳制（防 i64::MAX 以上静默翻转）。
pub const fn u64_to_i64_clamped(v: u64) -> i64 {
    if v > i64::MAX as u64 {
        i64::MAX
    } else {
        v as i64
    }
}

/// FNV-1a 32 位（const fn：while + 索引 + 显性 wrapping）。
pub const fn fnv1a32(bytes: &[u8]) -> u32 {
    let mut h: u32 = 0x811C_9DC5;
    let mut i = 0;
    while i < bytes.len() {
        h ^= bytes[i] as u32;
        h = h.wrapping_mul(0x0100_0193);
        i += 1;
    }
    h
}

/// LCG 确定性发生器（对拍与注入脚本的唯一随机源；同种子同序列）。
pub struct Lcg(u64);

impl Lcg {
    /// 新发生器（种子预混，防零种子锁死）。
    pub const fn new(seed: u64) -> Self {
        Lcg(seed
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407))
    }
    /// 下一原始值。
    pub fn next(&mut self) -> u64 {
        self.0 = self
            .0
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        self.0 >> 11
    }
    /// [0, n) 均匀取值。
    pub fn below(&mut self, n: u64) -> u64 {
        self.next() % n.max(1)
    }
}

/// 600 样本环账（每段样本账本体；滚动覆盖最老样本，和值显性 wrapping 累计）。
/// Vec 承载落堆——栈上结构体仅 ~48B，远小于 64KB 栈限。
#[derive(Clone, Debug)]
pub struct Ring600 {
    cap: usize,
    buf: Vec<u64>,
    head: usize,
    count: usize,
    /// 样本和（wrapping 累计；分位数/极值按需重扫，和值仅作趋势快照）。
    sum: u64,
}

impl Ring600 {
    /// 新环（const fn：Vec::new 为 const-stable）。
    pub const fn new() -> Self {
        Ring600 {
            cap: RING_CAP,
            buf: Vec::new(),
            head: 0,
            count: 0,
            sum: 0,
        }
    }
    /// 入环（满则覆盖最老）。
    pub fn push(&mut self, v: u64) {
        if self.count < self.cap {
            self.buf.push(v);
            self.count += 1;
        } else {
            // 满：head 指向最老槽位，覆盖后前移（显性取模回绕）。
            let slot = self.head;
            if slot < self.buf.len() {
                self.buf[slot] = v;
            }
            self.head = (self.head + 1) % self.cap;
        }
        self.sum = self.sum.wrapping_add(v);
    }
    /// 样本数。
    pub const fn count(&self) -> usize {
        self.count
    }
    /// 是否为空。
    pub const fn is_empty(&self) -> bool {
        self.count == 0
    }
    /// wrapping 和值快照。
    pub const fn sum_wrapping(&self) -> u64 {
        self.sum
    }
    /// 按最老→最新序导出（指纹与外部审计共用同序，保证对拍确定）。
    pub fn snapshot(&self) -> Vec<u64> {
        let mut out: Vec<u64> = Vec::with_capacity(self.count);
        let mut i = 0usize;
        while i < self.count {
            let slot = if self.count == self.cap {
                (self.head + i) % self.cap
            } else {
                i
            };
            if slot < self.buf.len() {
                out.push(self.buf[slot]);
            }
            i += 1;
        }
        out
    }
    /// 整数分位（最近邻秩法：idx = ⌈N·p/100⌉ − 1；插入排序，600 元素上限确定）。
    pub fn percentile(&self, p: u32) -> Result<u64, TmoAccError> {
        if p == 0 || p > 100 {
            return Err(TmoAccError::ParamBad("分位数 p 须在 1..=100", p as u64));
        }
        if self.count == 0 {
            return Err(TmoAccError::ProfileEmpty);
        }
        let mut v = self.snapshot();
        sort_asc(&mut v);
        let idx = (((self.count as u64) * (p as u64) + 99) / 100) as usize - 1;
        if idx >= v.len() {
            return Err(TmoAccError::LedgerBroken("分位索引越界"));
        }
        Ok(v[idx])
    }
    /// 极值（按需重扫，避免覆盖式 min/max 的失真维护）。
    pub fn min_max(&self) -> Result<(u64, u64), TmoAccError> {
        if self.count == 0 {
            return Err(TmoAccError::ProfileEmpty);
        }
        let mut mn = u64::MAX;
        let mut mx = 0u64;
        let mut i = 0usize;
        while i < self.buf.len() {
            let v = self.buf[i];
            if i < self.count {
                if v < mn {
                    mn = v;
                }
                if v > mx {
                    mx = v;
                }
            }
            i += 1;
        }
        Ok((mn, mx))
    }
}

/// 插入排序升序（确定性；600 元素上限下 O(n²) 可接受且无递归栈险）。
fn sort_asc(v: &mut [u64]) {
    let n = v.len();
    let mut i = 1usize;
    while i < n {
        let mut j = i;
        while j > 0 && v[j - 1] > v[j] {
            v.swap(j - 1, j);
            j -= 1;
        }
        i += 1;
    }
}

/// 趋势环（EWMA 趋势快照账；i64 承载，EWMA 对非负样本恒非负——
/// 算术右移向 −inf 取整的性质保证 cur + ((s−cur)>>3) ≥ 0）。
#[derive(Clone, Debug)]
pub struct TrendRing {
    cap: usize,
    buf: Vec<i64>,
    head: usize,
    count: usize,
}

impl TrendRing {
    /// 新趋势环。
    pub fn new() -> Self {
        TrendRing {
            cap: TREND_CAP,
            buf: Vec::new(),
            head: 0,
            count: 0,
        }
    }
    /// 入环（满则覆盖最老）。
    pub fn push(&mut self, v: i64) {
        if self.count < self.cap {
            self.buf.push(v);
            self.count += 1;
        } else {
            let slot = self.head;
            if slot < self.buf.len() {
                self.buf[slot] = v;
            }
            self.head = (self.head + 1) % self.cap;
        }
    }
    /// 拍数。
    pub const fn count(&self) -> usize {
        self.count
    }
    /// 最老→最新导出。
    pub fn snapshot(&self) -> Vec<i64> {
        let mut out: Vec<i64> = Vec::with_capacity(self.count);
        let mut i = 0usize;
        while i < self.count {
            let slot = if self.count == self.cap {
                (self.head + i) % self.cap
            } else {
                i
            };
            if slot < self.buf.len() {
                out.push(self.buf[slot]);
            }
            i += 1;
        }
        out
    }
}

/// 有界流水账（事件/红账/立案/近窗三元组共用的 append-only 滚动账；
/// 满则覆盖最老并计丢弃数——丢弃本身是可审计事件）。
#[derive(Clone, Debug)]
pub struct BoundedLog<T: Clone> {
    cap: usize,
    buf: Vec<T>,
    head: usize,
    count: usize,
    dropped: u64,
}

impl<T: Clone> BoundedLog<T> {
    /// 定容新账。
    pub fn with_cap(cap: usize) -> Self {
        BoundedLog {
            cap,
            buf: Vec::new(),
            head: 0,
            count: 0,
            dropped: 0,
        }
    }
    /// 入账（满则覆盖最老并计丢弃）。
    pub fn push(&mut self, v: T) {
        if self.count < self.cap {
            self.buf.push(v);
            self.count += 1;
        } else {
            let slot = self.head;
            if slot < self.buf.len() {
                self.buf[slot] = v;
            }
            self.head = (self.head + 1) % self.cap;
            self.dropped = self.dropped.wrapping_add(1);
        }
    }
    /// 账内条数。
    pub const fn count(&self) -> usize {
        self.count
    }
    /// 历史丢弃总数。
    pub const fn dropped(&self) -> u64 {
        self.dropped
    }
    /// 最老→最新导出。
    pub fn snapshot(&self) -> Vec<T> {
        let mut out: Vec<T> = Vec::with_capacity(self.count);
        let mut i = 0usize;
        while i < self.count {
            let slot = if self.count == self.cap {
                (self.head + i) % self.cap
            } else {
                i
            };
            if slot < self.buf.len() {
                out.push(self.buf[slot].clone());
            }
            i += 1;
        }
        out
    }
}

/// 三段误差分解三元组（精度账的核心记录；一次完整超时器生命周期的误差切片）。
/// 字段语义（全 ns，单调时钟域，来源 F1008 ktime 单调面）：
///   setup  = 入轮时刻 − 请求时刻           （设定段：软件登记路径延迟）
///   expiry = 实际到期 − 理论到期           （到期段：比较器/中断投递过冲；提前即红账）
///   exec   = 回调入口 − 实际到期           （执行段：F1027 分发链延迟）
///   model_sum_ns = saturating(setup+expiry+exec) —— 分解和（模型面全链误差）
///   chain_ns      = 全链实测（外测 attest 值；未 attest 时回退分解和）
///   residual_ns   = chain − model_sum（带符号残差）
///   residual_ppm  = |residual| × 10⁶ ÷ chain（ppm 口径，<50000 即 <5% 达标）
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct ErrorTriplet {
    /// 生命周期序号（账内唯一单调递增）。
    pub seq: u64,
    /// 超时器号（F1022 底座登记号）。
    pub tmo_id: u32,
    /// 设定段 ns。
    pub setup_ns: u64,
    /// 到期段 ns。
    pub expiry_ns: u64,
    /// 执行段 ns。
    pub exec_ns: u64,
    /// 分解和（模型面全链误差）。
    pub model_sum_ns: u64,
    /// 全链实测（attest 值或模型和回退）。
    pub chain_ns: u64,
    /// 带符号残差（chain − 分解和）。
    pub residual_ns: i64,
    /// 残差 ppm。
    pub residual_ppm: u64,
    /// 是否经外测 attest（未 attest 的三元组不入残差统计）。
    pub attested: bool,
    /// 残差越界标记（越界即已立案）。
    pub oob: bool,
}

impl ErrorTriplet {
    /// 按段种类取段值。
    pub const fn seg_ns(&self, kind: SegKind) -> u64 {
        match kind {
            SegKind::Setup => self.setup_ns,
            SegKind::Expiry => self.expiry_ns,
            SegKind::Exec => self.exec_ns,
        }
    }
    /// 分解和（模型面）。
    pub const fn sum_ns(&self) -> u64 {
        self.model_sum_ns
    }
    /// 残差幅值（|chain − sum|）。
    pub const fn residual_mag(&self) -> u64 {
        if self.residual_ns < 0 {
            // i64::MIN 已被钳制路径排除（residual 由两个 ≤i64::MAX 的钳制值相减），
            // wrapping_neg 在此域安全；仍显性标注。
            self.residual_ns.wrapping_neg() as u64
        } else {
            self.residual_ns as u64
        }
    }
}

/// 精度账事件（append-only 滚动账条目）。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct TaEvent {
    /// 关联生命周期序号（无关联填 0）。
    pub seq: u64,
    /// 诊断码（0x1A30 段闭集）。
    pub code: u32,
    /// 事件时刻（单调 ns）。
    pub at_ns: u64,
    /// 辅助载荷（如提前量/缺失段号/丢弃数）。
    pub aux: u64,
    /// 中文短注。
    pub note: &'static str,
}

/// 提前到期红账条目（容忍窗=0：actual < theory 即红）。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct RedEntry {
    /// 生命周期序号。
    pub seq: u64,
    /// 超时器号。
    pub tmo_id: u32,
    /// 理论到期 ns。
    pub theory_ns: u64,
    /// 实际到期 ns。
    pub actual_ns: u64,
    /// 提前量 ns（theory − actual > 0）。
    pub early_ns: u64,
}

/// 立案账条目（残差越界/p99 越线的正式立案记录）。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct CaseFile {
    /// 立案类码。
    pub kind: u32,
    /// 关联序号。
    pub seq: u64,
    /// 实测值。
    pub value: u64,
    /// 限额。
    pub limit: u64,
}

/// 在途表条目（三段流水推进的状态机本体）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
struct InFlight {
    seq: u64,
    tmo_id: u32,
    req_ns: u64,
    enq_ns: Option<u64>,
    theory_due_ns: Option<u64>,
    fire_ns: Option<u64>,
    state: u8,
}

/// 按超时器号的聚合画像（失守定位到具体超时器号）。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct TmoAgg {
    /// 超时器号。
    pub tmo_id: u32,
    /// 近窗样本数。
    pub samples: u32,
    /// 近窗最大全链 ns。
    pub max_chain_ns: u64,
    /// 近窗越线样本数。
    pub breaches: u32,
}

/// 分位曲线泳道（画像行）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct QuantileLane {
    /// 泳道名。
    pub name: &'static str,
    /// 样本数。
    pub count: u64,
    /// P50。
    pub p50: u64,
    /// P90。
    pub p90: u64,
    /// P95。
    pub p95: u64,
    /// P99。
    pub p99: u64,
    /// 极大值。
    pub max: u64,
    /// 8 桶 ns 直方图。
    pub hist: [u32; 8],
}

/// 精度画像（分位数曲线 + EWMA 趋势 + 漂移态 + 达标率的一次性快照）。
#[derive(Clone, Debug)]
pub struct PrecisionProfile {
    /// 四泳道分位曲线（0 设定 / 1 到期 / 2 执行 / 3 全链）。
    pub lanes: [QuantileLane; LANE_N],
    /// 四泳道 EWMA（ns；unset 为 −1 哨兵）。
    pub ewma_ns: [i64; LANE_N],
    /// EWMA 趋势拍（全链泳道）。
    pub trend: Vec<i64>,
    /// 漂移激活态。
    pub drift_active: bool,
    /// 当前漂移偏移（相对基线，ns 带符号）。
    pub drift_offset_ns: i64,
    /// 漂移越限连续计数。
    pub drift_streak: u32,
    /// 样本总数。
    pub samples: u64,
    /// 全链达标率（ppm）。
    pub attain_rate_ppm: u64,
}

/// 达标裁决（F1014 兑现面输出）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ComplianceVerdict {
    /// p99 达标（≤50µs QEMU 档）。
    pub compliant: bool,
    /// 全链 p99 实测 ns。
    pub chain_p99_ns: u64,
    /// 硬线 ns。
    pub limit_ns: u64,
    /// 失守最大段（达标时为 p99 最大段）。
    pub worst_seg: SegKind,
    /// 最大段 p99 ns。
    pub worst_seg_p99_ns: u64,
    /// 全链达标率 ppm。
    pub attain_rate_ppm: u64,
    /// 达标率达标（≥99%）。
    pub attain_ok: bool,
    /// 总裁决 = p99 达标 ∧ 达标率达标。
    pub overall: bool,
}

/// 健康面快照。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct TaHealth {
    /// 样本总数。
    pub samples: u64,
    /// 健康上电态（全部红面为零且 p99 达标）。
    pub up: bool,
    /// 诊断码（健康 0；降级 XC_HEALTH_DOWN）。
    pub code: u32,
    /// 全链 p99（空账为 0）。
    pub chain_p99_ns: u64,
    /// p99 达标态。
    pub compliant: bool,
    /// 提前红账总数。
    pub early_reds: u64,
    /// 段丢失总数。
    pub seg_losses: u64,
    /// 残差立案总数。
    pub residual_oob: u64,
    /// 漂移激活态。
    pub drift_active: bool,
    /// 事件账条数。
    pub events: usize,
    /// 事件丢弃总数。
    pub dropped_events: u64,
    /// 中文裁决。
    pub verdict: &'static str,
}

/// 双跑对拍报告。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct DualRunReport {
    /// 账 A 指纹。
    pub fp_a: u32,
    /// 账 B 指纹。
    pub fp_b: u32,
    /// 同输入指纹一致。
    pub equal: bool,
    /// 扰动账指纹。
    pub perturbed_fp: u32,
    /// 扰动账指纹确实不同（指纹灵敏度面）。
    pub perturbed_differs: bool,
    /// 对拍迭代数。
    pub iters: u32,
}

/// 注入核报告（三类注入各自的 (抓到, 总数)）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct InjectReport {
    /// 提前到期注入。
    pub early: (u32, u32),
    /// 段丢失注入·缺到期段（回调入口直呼路径）。
    pub segloss_fire: (u32, u32),
    /// 段丢失注入·弃账扫出（sweep 路径）。
    pub segloss_sweep: (u32, u32),
    /// 残差漂移注入。
    pub resid: (u32, u32),
}

impl InjectReport {
    /// 三类全部 10/10 必抓。
    pub const fn all_caught(&self) -> bool {
        let (ec, et) = self.early;
        let (fc, ft) = self.segloss_fire;
        let (sc, st) = self.segloss_sweep;
        let (rc, rt) = self.resid;
        ec == et && fc == ft && sc == st && rc == rt
    }
    /// 总抓获数。
    pub const fn total_caught(&self) -> u32 {
        self.early.0 + self.segloss_fire.0 + self.segloss_sweep.0 + self.resid.0
    }
    /// 总注入数。
    pub const fn total_injected(&self) -> u32 {
        self.early.1 + self.segloss_fire.1 + self.segloss_sweep.1 + self.resid.1
    }
}

/// 超时器精度账（本模块唯一权威账本；一次账本覆盖一段审计窗）。
/// 全部大容量数据落堆（环账/日志/在途表皆 Vec），栈上结构 <1KB。
#[derive(Clone, Debug)]
pub struct TmoAccLedger {
    /// 三段样本环（600 环 ×3）。
    rings: [Ring600; 3],
    /// 全链样本环（600 环）。
    chain_ring: Ring600,
    /// 四泳道 8 桶 ns 直方图。
    hist: [[u32; 8]; LANE_N],
    /// 四泳道 EWMA（−1 哨兵 = 未初始化）。
    ewma: [i64; LANE_N],
    /// 冻结基线（暖机窗末的全链 EWMA）。
    baseline: Option<i64>,
    /// 漂移越限连续计数。
    drift_streak: u32,
    /// 漂移激活态。
    drift_active: bool,
    /// 当前漂移偏移。
    drift_offset: i64,
    /// EWMA 趋势环。
    trend: TrendRing,
    /// 近窗三元组账。
    recent: BoundedLog<ErrorTriplet>,
    /// 事件账。
    events: BoundedLog<TaEvent>,
    /// 提前红账。
    reds: BoundedLog<RedEntry>,
    /// 立案账。
    cases: BoundedLog<CaseFile>,
    /// 在途表。
    in_flight: Vec<InFlight>,
    /// 样本总数（完整三段闭环数）。
    samples: u64,
    /// attest 样本数。
    attested_count: u64,
    /// 残差立案总数（含已滚出立案账的）。
    oob_total: u64,
    /// 历史|残差|最大 ppm（全 attest 样本）。
    max_resid_ppm: u64,
    /// 立案样本中的最大 ppm。
    max_oob_ppm: u64,
    /// 提前红账总数。
    early_red_total: u64,
    /// 段丢失总数。
    seg_loss_total: u64,
    /// 下一序号（0 保留为无效值）。
    next_seq: u64,
    /// 封账态。
    sealed: bool,
    /// 封账冻结指纹。
    frozen_fp: Option<u32>,
}

impl TmoAccLedger {
    /// 新账本。
    pub fn new() -> Self {
        TmoAccLedger {
            rings: [Ring600::new(), Ring600::new(), Ring600::new()],
            chain_ring: Ring600::new(),
            hist: [[0u32; 8]; LANE_N],
            ewma: [-1i64; LANE_N],
            baseline: None,
            drift_streak: 0,
            drift_active: false,
            drift_offset: 0,
            trend: TrendRing::new(),
            recent: BoundedLog::with_cap(RECENT_CAP),
            events: BoundedLog::with_cap(EVENTS_CAP),
            reds: BoundedLog::with_cap(REDS_CAP),
            cases: BoundedLog::with_cap(CASES_CAP),
            in_flight: Vec::new(),
            samples: 0,
            attested_count: 0,
            oob_total: 0,
            max_resid_ppm: 0,
            max_oob_ppm: 0,
            early_red_total: 0,
            seg_loss_total: 0,
            next_seq: 1,
            sealed: false,
            frozen_fp: None,
        }
    }

    // ————— 读面（无副作用快照） —————

    /// 完整三段闭环样本总数。
    pub const fn samples(&self) -> u64 {
        self.samples
    }
    /// attest 样本数。
    pub const fn attested_count(&self) -> u64 {
        self.attested_count
    }
    /// 残差立案总数。
    pub const fn oob_case_total(&self) -> u64 {
        self.oob_total
    }
    /// 全 attest 样本的 |残差| 最大 ppm。
    pub const fn max_resid_ppm(&self) -> u64 {
        self.max_resid_ppm
    }
    /// 立案样本最大 ppm。
    pub const fn max_oob_ppm(&self) -> u64 {
        self.max_oob_ppm
    }
    /// 提前红账总数。
    pub const fn early_red_total(&self) -> u64 {
        self.early_red_total
    }
    /// 段丢失总数。
    pub const fn seg_loss_total(&self) -> u64 {
        self.seg_loss_total
    }
    /// 在途表长度。
    pub fn inflight_len(&self) -> usize {
        self.in_flight.len()
    }
    /// 封账态。
    pub const fn sealed(&self) -> bool {
        self.sealed
    }
    /// 指定泳道环长。
    pub fn ring_len(&self, lane: usize) -> usize {
        if lane == LANE_CHAIN {
            self.chain_ring.count()
        } else if lane < LANE_CHAIN {
            self.rings[lane].count()
        } else {
            0
        }
    }
    /// 指定泳道 EWMA 读数（−1 哨兵 = 未初始化）。
    pub fn ewma_lane(&self, lane: usize) -> i64 {
        if lane < LANE_N {
            self.ewma[lane]
        } else {
            -1
        }
    }
    /// EWMA 趋势拍导出（全链泳道，最老→最新）。
    pub fn trend_snapshot(&self) -> Vec<i64> {
        self.trend.snapshot()
    }
    /// 指定段 P99。
    pub fn seg_p99(&self, kind: SegKind) -> Result<u64, TmoAccError> {
        self.rings[kind.lane()].percentile(99)
    }
    /// 全链 P99。
    pub fn chain_p99(&self) -> Result<u64, TmoAccError> {
        self.chain_ring.percentile(99)
    }
    /// 全链达标率（ppm；空账 0）。
    pub fn attain_rate_ppm(&self) -> u64 {
        let snap = self.chain_ring.snapshot();
        let total = snap.len();
        if total == 0 {
            return 0;
        }
        let mut ok = 0u64;
        let mut i = 0usize;
        while i < total {
            if snap[i] <= CHAIN_P99_LIMIT_NS {
                ok += 1;
            }
            i += 1;
        }
        ok.saturating_mul(1_000_000) / (total as u64)
    }
    /// 事件快照（最老→最新）。
    pub fn event_snapshot(&self) -> Vec<TaEvent> {
        self.events.snapshot()
    }
    /// 事件丢弃总数。
    pub const fn event_dropped(&self) -> u64 {
        self.events.dropped()
    }
    /// 红账快照。
    pub fn red_snapshot(&self) -> Vec<RedEntry> {
        self.reds.snapshot()
    }
    /// 立案快照。
    pub fn case_snapshot(&self) -> Vec<CaseFile> {
        self.cases.snapshot()
    }
    /// 近窗三元组快照。
    pub fn recent_snapshot(&self) -> Vec<ErrorTriplet> {
        self.recent.snapshot()
    }

    // ————— 写面（三段流水推进） —————

    /// 段 0 起点：请求登记（返回生命周期序号）。
    pub fn begin_request(&mut self, tmo_id: u32, now_ns: u64) -> Result<u64, TmoAccError> {
        if self.sealed {
            return Err(TmoAccError::Sealed);
        }
        if self.in_flight.len() >= INFLIGHT_CAP {
            self.push_event(
                XC_CONGESTED,
                0,
                now_ns,
                INFLIGHT_CAP as u64,
                "在途表满：并发超限拒绝登记",
            );
            return Err(TmoAccError::Congested);
        }
        let seq = self.next_seq;
        // 显性回绕：seq 空间 2^64；0 保留为无效值，回绕到 0 时跳到 1。
        self.next_seq = self.next_seq.wrapping_add(1);
        if self.next_seq == 0 {
            self.next_seq = 1;
        }
        self.in_flight.push(InFlight {
            seq,
            tmo_id,
            req_ns: now_ns,
            enq_ns: None,
            theory_due_ns: None,
            fire_ns: None,
            state: IF_REQ,
        });
        Ok(seq)
    }

    /// 段 0 闭环：入轮（设定段样本在此落定）。
    pub fn enqueued(&mut self, seq: u64, now_ns: u64) -> Result<(), TmoAccError> {
        if self.sealed {
            return Err(TmoAccError::Sealed);
        }
        let i = match self.find_inflight(seq) {
            Some(i) => i,
            None => {
                self.seg_loss_total += 1;
                self.push_event(
                    XC_SEG_LOST,
                    seq,
                    now_ns,
                    LANE_SETUP as u64,
                    "入轮失败：序号不在途（设定段缺失）",
                );
                return Err(TmoAccError::SegLoss(LANE_SETUP as u8));
            }
        };
        if self.in_flight[i].state != IF_REQ {
            self.push_event(
                XC_LEDGER_BROKEN,
                seq,
                now_ns,
                self.in_flight[i].state as u64,
                "入轮段重复或段序倒置",
            );
            return Err(TmoAccError::LedgerBroken("入轮段重复或段序倒置"));
        }
        self.in_flight[i].enq_ns = Some(now_ns);
        self.in_flight[i].state = IF_ENQ;
        Ok(())
    }

    /// 段 1 闭环：到期观测（比较器回读口径；提前即红账，容忍窗=0）。
    /// theory_due_ns = 入轮时刻 + 设定延迟（F1022 底座编程值）。
    pub fn fired(
        &mut self,
        seq: u64,
        theory_due_ns: u64,
        actual_fire_ns: u64,
    ) -> Result<(), TmoAccError> {
        if self.sealed {
            return Err(TmoAccError::Sealed);
        }
        let i = match self.find_inflight(seq) {
            Some(i) => i,
            None => {
                self.seg_loss_total += 1;
                self.push_event(
                    XC_SEG_LOST,
                    seq,
                    actual_fire_ns,
                    LANE_SETUP as u64,
                    "到期观测失败：序号不在途",
                );
                return Err(TmoAccError::SegLoss(LANE_SETUP as u8));
            }
        };
        let state = self.in_flight[i].state;
        if state == IF_REQ {
            // 未入轮即报到期：设定段缺失。
            self.seg_loss_total += 1;
            self.push_event(
                XC_SEG_LOST,
                seq,
                actual_fire_ns,
                LANE_SETUP as u64,
                "到期观测失败：未入轮（设定段缺失）",
            );
            self.in_flight.remove(i);
            return Err(TmoAccError::SegLoss(LANE_SETUP as u8));
        }
        if state != IF_ENQ {
            self.push_event(
                XC_LEDGER_BROKEN,
                seq,
                actual_fire_ns,
                state as u64,
                "重复到期观测",
            );
            return Err(TmoAccError::LedgerBroken("重复到期观测"));
        }
        if actual_fire_ns < theory_due_ns {
            // 提前到期红线：容忍窗=0，一律红账 + 立即断流（该生命周期不再进回调）。
            let early = theory_due_ns.saturating_sub(actual_fire_ns);
            let tmo_id = self.in_flight[i].tmo_id;
            self.early_red_total += 1;
            self.reds.push(RedEntry {
                seq,
                tmo_id,
                theory_ns: theory_due_ns,
                actual_ns: actual_fire_ns,
                early_ns: early,
            });
            self.push_event(
                XC_EARLY_FIRE,
                seq,
                actual_fire_ns,
                early,
                "提前到期红账：到期早于理论时刻（容忍窗=0）",
            );
            self.in_flight.remove(i);
            return Err(TmoAccError::EarlyFire(early));
        }
        self.in_flight[i].theory_due_ns = Some(theory_due_ns);
        self.in_flight[i].fire_ns = Some(actual_fire_ns);
        self.in_flight[i].state = IF_FIRED;
        Ok(())
    }

    /// 段 2 闭环：回调入口观测 → 产出三元组并全面落账。
    /// ext_chain_err_ns：全链实测误差（外测口径，如 F1008 ktime 面交叉读：
    /// cb_entry − req − armed_delay）。Some 走 attest 残差审计；None 回退模型和。
    pub fn callback_entered(
        &mut self,
        seq: u64,
        now_ns: u64,
        ext_chain_err_ns: Option<u64>,
    ) -> Result<ErrorTriplet, TmoAccError> {
        if self.sealed {
            return Err(TmoAccError::Sealed);
        }
        let i = match self.find_inflight(seq) {
            Some(i) => i,
            None => {
                self.seg_loss_total += 1;
                self.push_event(
                    XC_SEG_LOST,
                    seq,
                    now_ns,
                    LANE_SETUP as u64,
                    "回调观测失败：序号不在途",
                );
                return Err(TmoAccError::SegLoss(LANE_SETUP as u8));
            }
        };
        let state = self.in_flight[i].state;
        if state == IF_REQ {
            self.seg_loss_total += 1;
            self.push_event(
                XC_SEG_LOST,
                seq,
                now_ns,
                LANE_SETUP as u64,
                "回调观测失败：设定段缺失（未入轮）",
            );
            self.in_flight.remove(i);
            return Err(TmoAccError::SegLoss(LANE_SETUP as u8));
        }
        if state == IF_ENQ {
            self.seg_loss_total += 1;
            self.push_event(
                XC_SEG_LOST,
                seq,
                now_ns,
                LANE_EXPIRY as u64,
                "回调观测失败：到期段缺失（未观测到期）",
            );
            self.in_flight.remove(i);
            return Err(TmoAccError::SegLoss(LANE_EXPIRY as u8));
        }
        if state != IF_FIRED {
            self.push_event(
                XC_LEDGER_BROKEN,
                seq,
                now_ns,
                state as u64,
                "在途状态机非法态",
            );
            return Err(TmoAccError::LedgerBroken("在途状态机非法态"));
        }
        // IF_FIRED 态保证三源齐备（防御式回退仅作账面自洽兜底）。
        let req = self.in_flight[i].req_ns;
        let enq = match self.in_flight[i].enq_ns {
            Some(v) => v,
            None => req,
        };
        let theory = match self.in_flight[i].theory_due_ns {
            Some(v) => v,
            None => now_ns,
        };
        let fire = match self.in_flight[i].fire_ns {
            Some(v) => v,
            None => now_ns,
        };
        let tmo_id = self.in_flight[i].tmo_id;
        // 三段（显性 saturating：单调域内不回退，兜底钳 0）。
        let setup = enq.saturating_sub(req);
        let expiry = fire.saturating_sub(theory);
        let exec = now_ns.saturating_sub(fire);
        let model_sum = setup
            .saturating_add(expiry)
            .saturating_add(exec);
        let (chain, attested) = match ext_chain_err_ns {
            Some(e) => (e, true),
            None => (model_sum, false),
        };
        // 残差 = 全链实测 − 分解和（带符号；两侧先钳 i64 域防翻转）。
        let residual = u64_to_i64_clamped(chain).saturating_sub(u64_to_i64_clamped(model_sum));
        // ppm 幅值：|residual| × 10⁶ ÷ chain（chain≥1 防零除）。
        let diff: u64 = if chain > model_sum {
            chain - model_sum
        } else {
            model_sum - chain
        };
        let ppm = diff.saturating_mul(1_000_000) / chain.max(1);
        let oob = attested && ppm > RESIDUAL_LIMIT_PPM;
        let t = ErrorTriplet {
            seq,
            tmo_id,
            setup_ns: setup,
            expiry_ns: expiry,
            exec_ns: exec,
            model_sum_ns: model_sum,
            chain_ns: chain,
            residual_ns: residual,
            residual_ppm: ppm,
            attested,
            oob,
        };
        self.in_flight.remove(i);
        self.record_triplet(t.clone());
        Ok(t)
    }

    /// 弃账扫出：超窗未推进的在途项按缺失段立案（段丢失的第二检出路径）。
    /// 执行段由回调闭环无悬挂态，故缺失只会是设定段(0)/到期段(1)。
    pub fn sweep_lost(&mut self, now_ns: u64, stale_after_ns: u64) -> usize {
        if self.sealed {
            return 0;
        }
        let mut lost = 0usize;
        let mut i = 0usize;
        while i < self.in_flight.len() {
            let stale_at = self.in_flight[i].req_ns.saturating_add(stale_after_ns);
            let state = self.in_flight[i].state;
            if now_ns > stale_at && state != IF_FIRED {
                let seq = self.in_flight[i].seq;
                let missing = if state == IF_REQ { 0u8 } else { 1u8 };
                self.seg_loss_total += 1;
                self.push_event(
                    XC_SEG_LOST,
                    seq,
                    now_ns,
                    missing as u64,
                    "段账缺失：在途超时器超窗未推进（扫账立案）",
                );
                self.in_flight.remove(i);
                lost += 1;
            } else {
                i += 1;
            }
        }
        lost
    }

    /// 在途查找（线性扫描；容量 4096 上界确定）。
    fn find_inflight(&self, seq: u64) -> Option<usize> {
        let mut i = 0usize;
        while i < self.in_flight.len() {
            if self.in_flight[i].seq == seq {
                return Some(i);
            }
            i += 1;
        }
        None
    }

    /// 三元组全面落账（环账/直方图/EWMA/漂移/趋势/近窗/计数器）。
    fn record_triplet(&mut self, t: ErrorTriplet) {
        self.rings[LANE_SETUP].push(t.setup_ns);
        self.rings[LANE_EXPIRY].push(t.expiry_ns);
        self.rings[LANE_EXEC].push(t.exec_ns);
        self.chain_ring.push(t.chain_ns);
        self.hist[LANE_SETUP][bucket_of(t.setup_ns) as usize] += 1;
        self.hist[LANE_EXPIRY][bucket_of(t.expiry_ns) as usize] += 1;
        self.hist[LANE_EXEC][bucket_of(t.exec_ns) as usize] += 1;
        self.hist[LANE_CHAIN][bucket_of(t.chain_ns) as usize] += 1;
        // EWMA 四泳道（α=1/8 移位实现；−1 哨兵初始化为首个样本）。
        let s0 = u64_to_i64_clamped(t.setup_ns);
        let s1 = u64_to_i64_clamped(t.expiry_ns);
        let s2 = u64_to_i64_clamped(t.exec_ns);
        let s3 = u64_to_i64_clamped(t.chain_ns);
        self.ewma[LANE_SETUP] = ewma_step(self.ewma[LANE_SETUP], s0);
        self.ewma[LANE_EXPIRY] = ewma_step(self.ewma[LANE_EXPIRY], s1);
        self.ewma[LANE_EXEC] = ewma_step(self.ewma[LANE_EXEC], s2);
        self.ewma[LANE_CHAIN] = ewma_step(self.ewma[LANE_CHAIN], s3);
        self.samples += 1;
        if t.attested {
            self.attested_count += 1;
            if t.residual_ppm > self.max_resid_ppm {
                self.max_resid_ppm = t.residual_ppm;
            }
        }
        if t.oob {
            self.oob_total += 1;
            if t.residual_ppm > self.max_oob_ppm {
                self.max_oob_ppm = t.residual_ppm;
            }
            self.cases.push(CaseFile {
                kind: XC_RESIDUAL_OOB,
                seq: t.seq,
                value: t.residual_ppm,
                limit: RESIDUAL_LIMIT_PPM,
            });
            self.push_event(
                XC_RESIDUAL_OOB,
                t.seq,
                t.chain_ns,
                t.residual_ppm,
                "残差越界立案：分解和与全链实测偏差 >5%",
            );
        }
        self.recent.push(t.clone());
        // 基线冻结（暖机窗末）。
        if self.samples == WARMUP_SAMPLES && self.baseline.is_none() {
            self.baseline = Some(self.ewma[LANE_CHAIN]);
        }
        // 漂移检出：连续 32 拍越限才激活（防瞬时抖动误报）。
        if let Some(b) = self.baseline {
            let off = self.ewma[LANE_CHAIN].saturating_sub(b);
            self.drift_offset = off;
            let mag = if off == i64::MIN {
                i64::MAX
            } else if off < 0 {
                -off
            } else {
                off
            };
            if mag > DRIFT_LIMIT_NS {
                self.drift_streak = self.drift_streak.saturating_add(1);
                if self.drift_streak >= DRIFT_STREAK_LEN && !self.drift_active {
                    self.drift_active = true;
                    self.push_event(
                        XC_DRIFT_DETECTED,
                        t.seq,
                        t.chain_ns,
                        mag as u64,
                        "EWMA 漂移检出：全链误差趋势持续越限",
                    );
                }
            } else {
                self.drift_streak = 0;
                self.drift_active = false;
            }
        }
        // 趋势落拍。
        if self.samples % TREND_WINDOW == 0 {
            self.trend.push(self.ewma[LANE_CHAIN]);
        }
    }

    /// 事件入账。
    fn push_event(&mut self, code: u32, seq: u64, at_ns: u64, aux: u64, note: &'static str) {
        self.events.push(TaEvent {
            seq,
            code,
            at_ns,
            aux,
            note,
        });
    }

    // ————— 达标裁决与预算审计 —————

    /// 全链 p99 达标裁决（F1014 兑现面：p99 硬线 + 达标率 + 失守最大段定位）。
    pub fn verdict(&self) -> Result<ComplianceVerdict, TmoAccError> {
        let p99 = self.chain_ring.percentile(99)?;
        let rate = self.attain_rate_ppm();
        let mut worst_lane = LANE_SETUP;
        let mut worst_p99 = 0u64;
        let mut lane = 0usize;
        while lane < 3 {
            let p = self.rings[lane].percentile(99)?;
            if p >= worst_p99 {
                worst_p99 = p;
                worst_lane = lane;
            }
            lane += 1;
        }
        let compliant = p99 <= CHAIN_P99_LIMIT_NS;
        let attain_ok = rate >= ATTAIN_FLOOR_PPM;
        Ok(ComplianceVerdict {
            compliant,
            chain_p99_ns: p99,
            limit_ns: CHAIN_P99_LIMIT_NS,
            worst_seg: seg_from_lane(worst_lane),
            worst_seg_p99_ns: worst_p99,
            attain_rate_ppm: rate,
            attain_ok,
            overall: compliant && attain_ok,
        })
    }

    /// p99 达标闸（断言面：越线即 Err，供判据直接消费）。
    pub fn p99_gate(&self) -> Result<u64, TmoAccError> {
        let p99 = self.chain_ring.percentile(99)?;
        if p99 > CHAIN_P99_LIMIT_NS {
            Err(TmoAccError::P99Breach(p99))
        } else {
            Ok(p99)
        }
    }

    /// 残差闸（断言面：任一立案即 Err）。
    pub fn residual_gate(&self) -> Result<(u64, u64), TmoAccError> {
        if self.oob_total > 0 {
            Err(TmoAccError::ResidualOob(self.max_oob_ppm))
        } else {
            Ok((self.attested_count, self.max_resid_ppm))
        }
    }

    /// 漂移闸（断言面：漂移激活即 Err）。
    pub fn drift_gate(&self) -> Result<(), TmoAccError> {
        if self.drift_active {
            Err(TmoAccError::DriftDetected(self.drift_offset))
        } else {
            Ok(())
        }
    }

    /// 事件闸（断言面：存在丢弃即 Err）。
    pub fn event_gate(&self) -> Result<usize, TmoAccError> {
        let d = self.events.dropped();
        if d > 0 {
            Err(TmoAccError::EventOvf(d))
        } else {
            Ok(self.events.count())
        }
    }

    /// F1014 预算表逐行审计：四泳道 p99 对预算行（含达标率下限复核）。
    /// 返回逐行通过位（true = 该行兑现）。
    pub fn budget_audit(&self) -> Result<[bool; LANE_N], TmoAccError> {
        let table = budget_table();
        let mut out = [false; LANE_N];
        let mut i = 0usize;
        while i < LANE_N {
            let row = table[i];
            let p99 = if row.lane as usize == LANE_CHAIN {
                self.chain_ring.percentile(99)?
            } else {
                self.rings[row.lane as usize].percentile(99)?
            };
            let rate_ok = if row.lane as usize == LANE_CHAIN {
                self.attain_rate_ppm() >= row.attain_floor_ppm
            } else {
                true // 段级只审 p99 预算；达标率口径定义在全链
            };
            out[i] = p99 <= row.budget_ns && rate_ok;
            i += 1;
        }
        Ok(out)
    }

    // ————— 精度画像 —————

    /// 精度画像快照（分位曲线 + 直方图 + EWMA 趋势 + 漂移态 + 达标率）。
    pub fn profile(&self) -> Result<PrecisionProfile, TmoAccError> {
        if self.samples == 0 {
            return Err(TmoAccError::ProfileEmpty);
        }
        let lanes = [
            self.lane_profile(LANE_SETUP, "设定段(请求→入轮)")?,
            self.lane_profile(LANE_EXPIRY, "到期段(理论到期→实际到期)")?,
            self.lane_profile(LANE_EXEC, "执行段(实际到期→回调入口)")?,
            self.lane_profile(LANE_CHAIN, "全链(请求→回调入口)")?,
        ];
        Ok(PrecisionProfile {
            lanes,
            ewma_ns: self.ewma,
            trend: self.trend.snapshot(),
            drift_active: self.drift_active,
            drift_offset_ns: self.drift_offset,
            drift_streak: self.drift_streak,
            samples: self.samples,
            attain_rate_ppm: self.attain_rate_ppm(),
        })
    }

    /// 单泳道画像行。
    fn lane_profile(&self, lane: usize, name: &'static str) -> Result<QuantileLane, TmoAccError> {
        let ring = if lane == LANE_CHAIN {
            &self.chain_ring
        } else {
            &self.rings[lane]
        };
        let (mn, mx) = ring.min_max()?;
        let _ = mn;
        let mut hist = [0u32; 8];
        let mut b = 0usize;
        while b < 8 {
            hist[b] = self.hist[lane][b];
            b += 1;
        }
        Ok(QuantileLane {
            name,
            count: ring.count() as u64,
            p50: ring.percentile(50)?,
            p90: ring.percentile(90)?,
            p95: ring.percentile(95)?,
            p99: ring.percentile(99)?,
            max: mx,
            hist,
        })
    }

    /// 按超时器号聚合（近窗三元组；失守定位到具体超时器号）。
    /// 返回按最大全链降序的前 top_n 项。
    pub fn per_tmo_top(&self, top_n: usize) -> Vec<TmoAgg> {
        let rec = self.recent.snapshot();
        let mut aggs: Vec<TmoAgg> = Vec::new();
        let mut k = 0usize;
        while k < rec.len() {
            let t = &rec[k];
            let mut found = false;
            let mut j = 0usize;
            while j < aggs.len() {
                if aggs[j].tmo_id == t.tmo_id {
                    aggs[j].samples += 1;
                    if t.chain_ns > aggs[j].max_chain_ns {
                        aggs[j].max_chain_ns = t.chain_ns;
                    }
                    if t.chain_ns > CHAIN_P99_LIMIT_NS {
                        aggs[j].breaches += 1;
                    }
                    found = true;
                    j = aggs.len();
                } else {
                    j += 1;
                }
            }
            if !found {
                aggs.push(TmoAgg {
                    tmo_id: t.tmo_id,
                    samples: 1,
                    max_chain_ns: t.chain_ns,
                    breaches: if t.chain_ns > CHAIN_P99_LIMIT_NS { 1 } else { 0 },
                });
            }
            k += 1;
        }
        // 选择排序：按 max_chain 降序取前 top_n。
        let n = aggs.len();
        let mut i = 0usize;
        while i < n {
            let mut best = i;
            let mut j = i + 1;
            while j < n {
                if aggs[j].max_chain_ns > aggs[best].max_chain_ns {
                    best = j;
                }
                j += 1;
            }
            if best != i {
                aggs.swap(i, best);
            }
            i += 1;
        }
        if top_n < aggs.len() {
            aggs.truncate(top_n);
        }
        aggs
    }

    // ————— 健康面 / 指纹 / 封账 —————

    /// 健康面（F0899 口径同型：指标齐备 + 二态裁决 + 中文裁决）。
    pub fn health(&self) -> TaHealth {
        let (p99, compliant) = match self.chain_ring.percentile(99) {
            Ok(v) => (v, v <= CHAIN_P99_LIMIT_NS),
            Err(_) => (0u64, false),
        };
        let up = compliant
            && self.early_red_total == 0
            && self.seg_loss_total == 0
            && self.oob_total == 0
            && !self.drift_active;
        let verdict = if up {
            "正常：全链 p99 达标，无提前红账/段丢失/残差立案/漂移"
        } else if !compliant {
            "降级：全链 p99 越线（>50µs QEMU 档），按失守最大段定位预算超标"
        } else if self.early_red_total > 0 {
            "降级：存在提前到期红账（容忍窗=0），检查比较器写序"
        } else if self.seg_loss_total > 0 {
            "降级：段账缺失，三段分解断链，排查 F1027 分发链"
        } else if self.oob_total > 0 {
            "降级：残差越界立案，分解和与全链实测失配"
        } else {
            "降级：EWMA 漂移检出，误差趋势持续越限"
        };
        TaHealth {
            samples: self.samples,
            up,
            code: if up { 0 } else { XC_HEALTH_DOWN },
            chain_p99_ns: p99,
            compliant,
            early_reds: self.early_red_total,
            seg_losses: self.seg_loss_total,
            residual_oob: self.oob_total,
            drift_active: self.drift_active,
            events: self.events.count(),
            dropped_events: self.events.dropped(),
            verdict,
        }
    }

    /// 账本指纹（FNV-1a 32 位；同输入同指纹——双跑对拍的本体）。
    /// 覆盖：版本 + 四泳道环账序 + 直方图 + EWMA + 趋势 + 漂移态 + 全部计数器
    /// + 事件/红账/立案/近窗账序 + 在途表序。
    pub fn fingerprint(&self) -> u32 {
        let mut b: Vec<u8> = Vec::with_capacity(4096);
        b.extend_from_slice(TA_VERSION.as_bytes());
        // 四泳道环账（最老→最新固定序）。
        let mut lane = 0usize;
        while lane < 3 {
            for v in self.rings[lane].snapshot() {
                b.extend_from_slice(&v.to_le_bytes());
            }
            lane += 1;
        }
        for v in self.chain_ring.snapshot() {
            b.extend_from_slice(&v.to_le_bytes());
        }
        // 直方图。
        let mut l = 0usize;
        while l < LANE_N {
            let mut bb = 0usize;
            while bb < 8 {
                b.extend_from_slice(&self.hist[l][bb].to_le_bytes());
                bb += 1;
            }
            l += 1;
        }
        // EWMA。
        l = 0;
        while l < LANE_N {
            b.extend_from_slice(&self.ewma[l].to_le_bytes());
            l += 1;
        }
        // 趋势。
        for v in self.trend.snapshot() {
            b.extend_from_slice(&v.to_le_bytes());
        }
        // 基线/漂移。
        let base = match self.baseline {
            Some(v) => v,
            None => i64::MIN,
        };
        b.extend_from_slice(&base.to_le_bytes());
        b.extend_from_slice(&self.drift_streak.to_le_bytes());
        b.push(if self.drift_active { 1 } else { 0 });
        b.extend_from_slice(&self.drift_offset.to_le_bytes());
        // 计数器。
        b.extend_from_slice(&self.samples.to_le_bytes());
        b.extend_from_slice(&self.attested_count.to_le_bytes());
        b.extend_from_slice(&self.oob_total.to_le_bytes());
        b.extend_from_slice(&self.max_resid_ppm.to_le_bytes());
        b.extend_from_slice(&self.max_oob_ppm.to_le_bytes());
        b.extend_from_slice(&self.early_red_total.to_le_bytes());
        b.extend_from_slice(&self.seg_loss_total.to_le_bytes());
        b.extend_from_slice(&self.next_seq.to_le_bytes());
        b.push(if self.sealed { 1 } else { 0 });
        // 四账序。
        for e in self.events.snapshot() {
            b.extend_from_slice(&e.seq.to_le_bytes());
            b.extend_from_slice(&e.code.to_le_bytes());
            b.extend_from_slice(&e.at_ns.to_le_bytes());
            b.extend_from_slice(&e.aux.to_le_bytes());
        }
        for r in self.reds.snapshot() {
            b.extend_from_slice(&r.seq.to_le_bytes());
            b.extend_from_slice(&r.theory_ns.to_le_bytes());
            b.extend_from_slice(&r.actual_ns.to_le_bytes());
            b.extend_from_slice(&r.early_ns.to_le_bytes());
        }
        for c in self.cases.snapshot() {
            b.extend_from_slice(&c.kind.to_le_bytes());
            b.extend_from_slice(&c.seq.to_le_bytes());
            b.extend_from_slice(&c.value.to_le_bytes());
            b.extend_from_slice(&c.limit.to_le_bytes());
        }
        for t in self.recent.snapshot() {
            b.extend_from_slice(&t.seq.to_le_bytes());
            b.extend_from_slice(&t.setup_ns.to_le_bytes());
            b.extend_from_slice(&t.expiry_ns.to_le_bytes());
            b.extend_from_slice(&t.exec_ns.to_le_bytes());
            b.extend_from_slice(&t.chain_ns.to_le_bytes());
            b.extend_from_slice(&t.residual_ns.to_le_bytes());
            b.extend_from_slice(&t.residual_ppm.to_le_bytes());
            b.push(if t.attested { 1 } else { 0 });
            b.push(if t.oob { 1 } else { 0 });
        }
        // 在途表。
        let mut i = 0usize;
        while i < self.in_flight.len() {
            b.extend_from_slice(&self.in_flight[i].seq.to_le_bytes());
            b.extend_from_slice(&self.in_flight[i].tmo_id.to_le_bytes());
            b.extend_from_slice(&self.in_flight[i].req_ns.to_le_bytes());
            b.push(self.in_flight[i].state);
            i += 1;
        }
        fnv1a32(&b)
    }

    /// 封账（冻结指纹；封账后一切写面拒绝）。
    /// 注意：sealed 标志入指纹哈希，故须先置位再算指纹，保证 verify_frozen 重算一致。
    pub fn seal(&mut self) -> Result<u32, TmoAccError> {
        if self.sealed {
            return Err(TmoAccError::LedgerBroken("重复封账"));
        }
        self.sealed = true;
        let fp = self.fingerprint();
        self.frozen_fp = Some(fp);
        Ok(fp)
    }

    /// 冻结指纹复核（封账后重算比对；不一致即指纹漂移事件）。
    pub fn verify_frozen(&self) -> Result<bool, TmoAccError> {
        let want = match self.frozen_fp {
            Some(v) => v,
            None => return Err(TmoAccError::ParamBad("尚未封账，无冻结指纹", 0)),
        };
        let got = self.fingerprint();
        if want != got {
            return Ok(false);
        }
        Ok(true)
    }
}

/// EWMA 单步（cur 非负域；算术右移向 −inf 取整，非负性保持）。
pub const fn ewma_step(cur: i64, sample: i64) -> i64 {
    if cur < 0 {
        // −1 哨兵：首样本直赋。
        sample
    } else {
        cur + ((sample.saturating_sub(cur)) >> EWMA_SHIFT)
    }
}

// ————— 确定性脚本面（对拍/注入/判据共用；同种子同序列） —————

/// 合规流水脚本：小偏差三段样本 + 外测 attest（噪声 ≤ 全链 2%，交叉时钟读差模型）。
/// over_bias_ns：到期段过冲偏置（对拍扰动/漂移脚本用）。
pub fn feed_compliant(
    led: &mut TmoAccLedger,
    iters: u32,
    over_bias_ns: u64,
) -> Result<(), TmoAccError> {
    feed_compliant_from(led, iters, over_bias_ns, 1_000_000)
}

/// 带起始时刻的合规流水（漂移脚本两段使用不重叠时间域，保单调观感）。
pub fn feed_compliant_from(
    led: &mut TmoAccLedger,
    iters: u32,
    over_bias_ns: u64,
    t0_ns: u64,
) -> Result<(), TmoAccError> {
    let mut rng = Lcg::new(0x1A_28_F028);
    let mut t = t0_ns;
    let mut i = 0u32;
    while i < iters {
        let setup = rng.below(800);
        let delay = 1_000 + rng.below(20_000);
        let over = over_bias_ns + rng.below(1_500);
        let exec = rng.below(2_500);
        let tmo_id = 1_000 + (i % 64);
        let seq = led.begin_request(tmo_id, t)?;
        let enq = t.saturating_add(setup);
        let theory = enq.saturating_add(delay);
        let fire = theory.saturating_add(over);
        let cb = fire.saturating_add(exec);
        led.enqueued(seq, enq)?;
        led.fired(seq, theory, fire)?;
        let model = setup.saturating_add(over).saturating_add(exec);
        // 外测噪声 ≤ 模型和/50（≈2%，恒 <5% 残差线——QEMU 档交叉时钟读差模型）。
        let noise = rng.below(model / 50 + 1);
        let ext = model.saturating_add(noise);
        let _ = led.callback_entered(seq, cb, Some(ext))?;
        t = t.saturating_add(delay).saturating_add(500);
        i += 1;
    }
    Ok(())
}

/// 执行段超重脚本：执行段 40k~60k ns → 全链 p99 必越 50µs 硬线（失守定位用）。
pub fn feed_exec_heavy(led: &mut TmoAccLedger, iters: u32) -> Result<(), TmoAccError> {
    let mut rng = Lcg::new(0x1A_28_BEEF);
    let mut t = 2_000_000u64;
    let mut i = 0u32;
    while i < iters {
        let setup = rng.below(500);
        let delay = 1_000 + rng.below(10_000);
        let over = rng.below(1_000);
        let exec = 40_000 + rng.below(20_000);
        let tmo_id = 2_000 + (i % 16);
        let seq = led.begin_request(tmo_id, t)?;
        let enq = t.saturating_add(setup);
        let theory = enq.saturating_add(delay);
        let fire = theory.saturating_add(over);
        let cb = fire.saturating_add(exec);
        led.enqueued(seq, enq)?;
        led.fired(seq, theory, fire)?;
        let model = setup.saturating_add(over).saturating_add(exec);
        let noise = rng.below(model / 50 + 1);
        let _ = led.callback_entered(seq, cb, Some(model.saturating_add(noise)))?;
        t = t.saturating_add(delay).saturating_add(500);
        i += 1;
    }
    Ok(())
}

/// 漂移脚本：前 100 样本小偏差（冻结基线）→ 后续持续大偏差（触发漂移检出）。
pub fn feed_drift(led: &mut TmoAccLedger) -> Result<(), TmoAccError> {
    feed_compliant_from(led, 100, 500, 1_000_000)?;
    feed_compliant_from(led, 220, 26_000, 8_000_000)?;
    Ok(())
}

/// 双跑确定性对拍：同脚本喂双账指纹必等；+1ns 扰动指纹必异。
pub fn dual_run_determinism() -> Result<DualRunReport, TmoAccError> {
    let iters = 240u32;
    let mut a = TmoAccLedger::new();
    let mut b = TmoAccLedger::new();
    let mut p = TmoAccLedger::new();
    feed_compliant(&mut a, iters, 0)?;
    feed_compliant(&mut b, iters, 0)?;
    feed_compliant(&mut p, iters, 1)?;
    let fa = a.fingerprint();
    let fb = b.fingerprint();
    let fp = p.fingerprint();
    Ok(DualRunReport {
        fp_a: fa,
        fp_b: fb,
        equal: fa == fb,
        perturbed_fp: fp,
        perturbed_differs: fa != fp,
        iters,
    })
}

/// 双跑对拍闸（断言面）。
pub fn dual_run_gate() -> Result<DualRunReport, TmoAccError> {
    let r = dual_run_determinism()?;
    if !r.equal {
        return Err(TmoAccError::DualRunMismatch(r.fp_a, r.fp_b));
    }
    Ok(r)
}

/// 注入闸（断言面：任一类漏抓即 Err）。
pub fn inject_gate(rep: &InjectReport) -> Result<(), TmoAccError> {
    if !rep.all_caught() {
        // 类号：1 提前 / 2 缺到期 / 3 弃账扫出 / 4 残差。
        let kind = if rep.early.0 != rep.early.1 {
            1
        } else if rep.segloss_fire.0 != rep.segloss_fire.1 {
            2
        } else if rep.segloss_sweep.0 != rep.segloss_sweep.1 {
            3
        } else {
            4
        };
        return Err(TmoAccError::InjectMissed(kind));
    }
    Ok(())
}

/// 注入核：三类注入各 10/10 必抓（提前到期 / 段丢失双路径 / 残差漂移）。
pub fn injection_core() -> InjectReport {
    // 1) 提前到期 ×10：实际到期比理论早 1ns → 容忍窗=0 必红。
    let mut early = 0u32;
    let mut k = 0u32;
    while k < 10 {
        let mut led = TmoAccLedger::new();
        let seq = match led.begin_request(7, 1_000) {
            Ok(s) => s,
            Err(_) => 0,
        };
        let _ = led.enqueued(seq, 1_100);
        let mut hit = false;
        match led.fired(seq, 11_100, 11_099) {
            Err(TmoAccError::EarlyFire(1)) => {
                if led.early_red_total() == 1 && led.red_snapshot().len() == 1 {
                    hit = true;
                }
            }
            _ => {}
        }
        if hit {
            early += 1;
        }
        k += 1;
    }
    // 2a) 段丢失·缺到期段 ×10：入轮后直呼回调入口 → 到期段缺失必抓。
    let mut slf = 0u32;
    k = 0;
    while k < 10 {
        let mut led = TmoAccLedger::new();
        let seq = match led.begin_request(8, 1_000) {
            Ok(s) => s,
            Err(_) => 0,
        };
        let _ = led.enqueued(seq, 1_100);
        let mut hit = false;
        match led.callback_entered(seq, 12_000, None) {
            Err(TmoAccError::SegLoss(1)) => {
                if led.seg_loss_total() == 1 && led.inflight_len() == 0 {
                    hit = true;
                }
            }
            _ => {}
        }
        if hit {
            slf += 1;
        }
        k += 1;
    }
    // 2b) 段丢失·弃账扫出 ×10：仅登记不推进 → 超窗 sweep 必扫出设定段缺失。
    let mut sls = 0u32;
    k = 0;
    while k < 10 {
        let mut led = TmoAccLedger::new();
        let _s1 = match led.begin_request(9, 1_000) {
            Ok(s) => s,
            Err(_) => 0,
        };
        let lost = led.sweep_lost(1_000 + 100_000, 50_000);
        if lost == 1 && led.seg_loss_total() == 1 && led.inflight_len() == 0 {
            sls += 1;
        }
        k += 1;
    }
    // 3) 残差漂移 ×10：外测值 = 2×分解和 → ppm = 500000 > 50000 必立案。
    let mut rs = 0u32;
    k = 0;
    while k < 10 {
        let mut led = TmoAccLedger::new();
        let seq = match led.begin_request(10, 1_000) {
            Ok(s) => s,
            Err(_) => 0,
        };
        let _ = led.enqueued(seq, 1_100); // setup = 100
        let _ = led.fired(seq, 11_100, 12_100); // over = 1000
        // exec = 50（12_150 − 12_100）；model = 1150；ext = 2300。
        let mut hit = false;
        match led.callback_entered(seq, 12_150, Some(2_300)) {
            Ok(t) => {
                if t.oob && led.oob_case_total() == 1 && led.case_snapshot().len() == 1 {
                    hit = true;
                }
            }
            Err(_) => {}
        }
        if hit {
            rs += 1;
        }
        k += 1;
    }
    InjectReport {
        early: (early, 10),
        segloss_fire: (slf, 10),
        segloss_sweep: (sls, 10),
        resid: (rs, 10),
    }
}

// ————— 自包含判据面（不依赖 crate::checks，账式自检） —————

/// 单条判据。
#[derive(Clone, Debug)]
pub struct TaCheck {
    /// 判据名。
    pub name: &'static str,
    /// 通过态。
    pub ok: bool,
    /// 失败短因。
    pub msg: &'static str,
}

/// 判据集（自包含 CheckSet 同型面）。
#[derive(Clone, Debug)]
pub struct TaCheckSet {
    /// 套件名。
    pub suite: &'static str,
    /// 判据列表。
    pub checks: Vec<TaCheck>,
}

impl TaCheckSet {
    /// 新套件。
    pub fn new(suite: &'static str) -> Self {
        TaCheckSet {
            suite,
            checks: Vec::new(),
        }
    }
    /// 添判据。
    pub fn add(&mut self, name: &'static str, ok: bool, msg: &'static str) {
        self.checks.push(TaCheck { name, ok, msg });
    }
    /// 全过态。
    pub fn all_ok(&self) -> bool {
        let mut i = 0usize;
        while i < self.checks.len() {
            if !self.checks[i].ok {
                return false;
            }
            i += 1;
        }
        true
    }
    /// 通过数。
    pub fn passed(&self) -> usize {
        let mut n = 0usize;
        let mut i = 0usize;
        while i < self.checks.len() {
            if self.checks[i].ok {
                n += 1;
            }
            i += 1;
        }
        n
    }
    /// 人话报告。
    pub fn report(&self) -> String {
        let mut s = String::new();
        s.push_str(&format!(
            "套件 {}：{}/{} 过\n",
            self.suite,
            self.passed(),
            self.checks.len()
        ));
        let mut i = 0usize;
        while i < self.checks.len() {
            let c = &self.checks[i];
            if c.ok {
                s.push_str("[过] ");
            } else {
                s.push_str("[败] ");
            }
            s.push_str(c.name);
            if !c.msg.is_empty() {
                s.push_str(" —— ");
                s.push_str(c.msg);
            }
            s.push('\n');
            i += 1;
        }
        s
    }
}

/// 达标裁决的人话化（explain 对齐体例）。
pub fn explain_verdict(v: &ComplianceVerdict) -> &'static str {
    if v.overall {
        "达标：全链 p99 与达标率双双兑现 F1014 预算表（≤50µs 且 ≥99%）"
    } else if !v.compliant {
        "失守：全链 p99 超 50µs QEMU 档硬线——按失守最大段定位预算超标项"
    } else {
        "失守：p99 达标但达标率 <99%——存在长尾越线样本，查尾桶直方图"
    }
}

/// UNX-F1028 判据入口（自包含账式自检；C28-01 ~ C28-20）。
pub fn run_unx1028_checks() -> TaCheckSet {
    let mut s = TaCheckSet::new("unxstar-unx1028");

    // C28-01 三段分解守恒：手工数字全链流水 → 三段值/分解和/残差逐字段精确。
    {
        let mut led = TmoAccLedger::new();
        let seq = match led.begin_request(42, 1_000) {
            Ok(v) => v,
            Err(_) => 0,
        };
        let enq_ok = led.enqueued(seq, 1_250).is_ok(); // setup = 250
        let fire_ok = led.fired(seq, 11_250, 11_340).is_ok(); // expiry = 90
        let t = led.callback_entered(seq, 11_700, Some(710)); // exec = 360
        let ok = match t {
            Ok(t) => {
                enq_ok
                    && fire_ok
                    && t.setup_ns == 250
                    && t.expiry_ns == 90
                    && t.exec_ns == 360
                    && t.model_sum_ns == 700
                    && t.chain_ns == 710
                    && t.residual_ns == 10
                    && t.residual_ppm == 10_000_000u64 / 710
                    && t.attested
                    && !t.oob
                    && t.residual_mag() == 10
                    && t.seg_ns(SegKind::Setup) == 250
                    && led.samples() == 1
                    && led.inflight_len() == 0
            }
            Err(_) => false,
        };
        s.add("C28-01-三段分解守恒", ok, if ok { "" } else { "三段切片/残差算术失真" });
    }

    // C28-02 分位数引擎：600 样本已知分布 → 最近邻秩 P50/P90/P95/P99 精确。
    {
        let mut r = Ring600::new();
        let mut v = 1u64;
        while v <= 600 {
            r.push(v);
            v += 1;
        }
        let p50 = r.percentile(50);
        let p90 = r.percentile(90);
        let p95 = r.percentile(95);
        let p99 = r.percentile(99);
        let mm = r.min_max();
        let ok = p50 == Ok(300)
            && p90 == Ok(540)
            && p95 == Ok(570)
            && p99 == Ok(594)
            && mm == Ok((1, 600));
        s.add("C28-02-分位数引擎", ok, if ok { "" } else { "整数分位最近邻秩漂移" });
    }

    // C28-03 全链 p99 达标（J1 主面）：合规流水 → p99 ≤50µs 且达标率 100%。
    {
        let mut led = TmoAccLedger::new();
        let fed = feed_compliant(&mut led, 300, 0).is_ok();
        let v = led.verdict();
        let ok = match v {
            Ok(v) => {
                fed
                    && v.compliant
                    && v.chain_p99_ns <= CHAIN_P99_LIMIT_NS
                    && v.attain_ok
                    && v.attain_rate_ppm == 1_000_000
                    && v.overall
                    && led.p99_gate().is_ok()
                    && !explain_verdict(&v).is_empty()
            }
            Err(_) => false,
        };
        s.add("C28-03-全链p99达标", ok, if ok { "" } else { "QEMU 档 50µs 硬线失守" });
    }

    // C28-04 失守定位：执行段超重 → 越线二态 + 最大段=执行段 + 定位到超时器号。
    {
        let mut led = TmoAccLedger::new();
        let fed = feed_exec_heavy(&mut led, 300).is_ok();
        let v = led.verdict();
        let ok = match v {
            Ok(v) => {
                fed
                    && !v.compliant
                    && v.chain_p99_ns > CHAIN_P99_LIMIT_NS
                    && v.worst_seg == SegKind::Exec
                    && v.worst_seg_p99_ns > CHAIN_P99_LIMIT_NS
                    && !v.attain_ok
                    && !v.overall
            }
            Err(_) => false,
        };
        let gate = match led.p99_gate() {
            Err(TmoAccError::P99Breach(p)) => p > CHAIN_P99_LIMIT_NS,
            _ => false,
        };
        let top = led.per_tmo_top(4);
        let located = !top.is_empty() && top[0].breaches > 0;
        s.add(
            "C28-04-失守定位最大段",
            ok && gate && located,
            if ok && gate && located { "" } else { "越线二态/最大段定位/超时器号定位失灵" },
        );
    }

    // C28-05 残差 <5%（J1 主面）：外测噪声 ≤2% 模型 → 零立案且 |残差| 上界 <5%。
    {
        let mut led = TmoAccLedger::new();
        let fed = feed_compliant(&mut led, 300, 0).is_ok();
        let gate = led.residual_gate();
        let ok = fed
            && led.attested_count() == 300
            && led.oob_case_total() == 0
            && led.max_resid_ppm() <= RESIDUAL_LIMIT_PPM
            && gate.is_ok();
        s.add("C28-05-残差小于5%", ok, if ok { "" } else { "残差越界或 attest 断链" });
    }

    // C28-06 提前到期红账：容忍窗=0（早 1ns 必红；恰好理论时刻必过）。
    {
        let mut led = TmoAccLedger::new();
        let s1 = match led.begin_request(1, 1_000) {
            Ok(v) => v,
            Err(_) => 0,
        };
        let _ = led.enqueued(s1, 1_100);
        let r_early = led.fired(s1, 11_100, 11_099); // 早 1ns → 红
        let s2 = match led.begin_request(2, 2_000) {
            Ok(v) => v,
            Err(_) => 0,
        };
        let _ = led.enqueued(s2, 2_100);
        let r_exact = led.fired(s2, 12_100, 12_100); // 恰好 → 过
        let reds = led.red_snapshot();
        let ok = match r_early {
            Err(TmoAccError::EarlyFire(1)) => true,
            _ => false,
        } && r_exact.is_ok()
            && led.early_red_total() == 1
            && reds.len() == 1
            && reds[0].early_ns == 1
            && reds[0].seq == s1;
        s.add("C28-06-提前到期红账", ok, if ok { "" } else { "容忍窗≠0 或红账漏记" });
    }

    // C28-07 段级预算审计：合规流水 → 四泳道逐行兑现 F1014 预算表。
    {
        let mut led = TmoAccLedger::new();
        let fed = feed_compliant(&mut led, 300, 0).is_ok();
        let audit = led.budget_audit();
        let ok = match audit {
            Ok(rows) => {
                let setup_p99 = match led.seg_p99(SegKind::Setup) {
                    Ok(v) => v,
                    Err(_) => u64::MAX,
                };
                let expiry_p99 = match led.seg_p99(SegKind::Expiry) {
                    Ok(v) => v,
                    Err(_) => u64::MAX,
                };
                let exec_p99 = match led.seg_p99(SegKind::Exec) {
                    Ok(v) => v,
                    Err(_) => u64::MAX,
                };
                fed && rows[0] && rows[1] && rows[2] && rows[3]
                    && setup_p99 <= SEG_BUDGET_SETUP_NS
                    && expiry_p99 <= SEG_BUDGET_EXPIRY_NS
                    && exec_p99 <= SEG_BUDGET_EXEC_NS
            }
            Err(_) => false,
        };
        s.add("C28-07-F1014预算逐行兑现", ok, if ok { "" } else { "预算表逐行审计失守" });
    }

    // C28-08 EWMA 误差趋势：漂移脚本 → 趋势拍递增（误差趋势被跟踪）。
    {
        let mut led = TmoAccLedger::new();
        let fed = feed_drift(&mut led).is_ok();
        let trend = led.trend_snapshot();
        let ok = fed
            && trend.len() >= 3
            && trend[2] > trend[0]
            && led.ewma_lane(LANE_CHAIN) > trend[0];
        s.add("C28-08-EWMA趋势跟踪", ok, if ok { "" } else { "趋势拍缺失或不增" });
    }

    // C28-09 漂移检出：持续大偏差 → 漂移激活 + 事件 + 闸拒。
    {
        let mut led = TmoAccLedger::new();
        let fed = feed_drift(&mut led).is_ok();
        let gate = led.drift_gate();
        let mut has_event = false;
        for e in led.event_snapshot() {
            if e.code == XC_DRIFT_DETECTED {
                has_event = true;
            }
        }
        let ok = fed && led.drift_active && has_event && gate.is_err();
        s.add("C28-09-漂移检出", ok, if ok { "" } else { "漂移漏检或误报" });
    }

    // C28-10 事件账滚动：300 红账事件 → 256 容量滚动 + 丢弃计数 + 事件闸拒。
    {
        let mut led = TmoAccLedger::new();
        let mut k = 0u32;
        while k < 300 {
            let seq = match led.begin_request(3, 1_000 + k as u64) {
                Ok(v) => v,
                Err(_) => 0,
            };
            let _ = led.enqueued(seq, 1_100 + k as u64);
            let _ = led.fired(seq, 11_100 + k as u64, 11_000 + k as u64); // 每次红账 1 事件
            k += 1;
        }
        let ev = led.event_snapshot();
        let gate = match led.event_gate() {
            Err(TmoAccError::EventOvf(d)) => d == 44,
            _ => false,
        };
        let ok = led.early_red_total() == 300
            && ev.len() == EVENTS_CAP
            && led.event_dropped() == 44
            && gate;
        s.add("C28-10-事件账滚动丢弃", ok, if ok { "" } else { "事件环容量/丢弃计数失真" });
    }

    // C28-11 健康面：健康账 up；红账账降级（二态 + 中文裁决非空）。
    {
        let mut good = TmoAccLedger::new();
        let _ = feed_compliant(&mut good, 150, 0);
        let hg = good.health();
        let mut bad = TmoAccLedger::new();
        let sq = match bad.begin_request(5, 1_000) {
            Ok(v) => v,
            Err(_) => 0,
        };
        let _ = bad.enqueued(sq, 1_100);
        let _ = bad.fired(sq, 11_100, 11_050); // 早 50ns → 红
        let hb = bad.health();
        let ok = hg.up
            && hg.code == 0
            && hg.compliant
            && hg.early_reds == 0
            && !hg.verdict.is_empty()
            && !hb.up
            && hb.code == XC_HEALTH_DOWN
            && hb.early_reds == 1
            && !hb.verdict.is_empty();
        s.add("C28-11-健康二态", ok, if ok { "" } else { "健康面二态失灵" });
    }

    // C28-12 指纹冻结：同输入双账指纹等；+1ns 扰动异；封账冻结可复核。
    {
        let mut a = TmoAccLedger::new();
        let mut b = TmoAccLedger::new();
        let mut c = TmoAccLedger::new();
        let _ = feed_compliant(&mut a, 200, 0);
        let _ = feed_compliant(&mut b, 200, 0);
        let _ = feed_compliant(&mut c, 200, 1);
        let fa = a.fingerprint();
        let equal = fa == b.fingerprint();
        let differs = fa != c.fingerprint();
        let seal = a.seal();
        let verify = a.verify_frozen();
        let write_after = a.begin_request(1, 9_999_999);
        let ok = equal
            && differs
            && seal.is_ok()
            && verify == Ok(true)
            && write_after == Err(TmoAccError::Sealed);
        s.add("C28-12-指纹冻结", ok, if ok { "" } else { "指纹灵敏度/封账冻结失灵" });
    }

    // C28-13 双跑确定性对拍（J1 对拍面）：双账等指纹 + 扰动异指纹 + 闸过。
    {
        let r = dual_run_gate();
        let ok = match r {
            Ok(r) => r.equal && r.perturbed_differs && r.iters == 240,
            Err(_) => false,
        };
        s.add("C28-13-双跑对拍", ok, if ok { "" } else { "双跑非确定性或指纹钝感" });
    }

    // C28-14 注入核（J1 注入面）：三类注入全 10/10 必抓。
    {
        let rep = injection_core();
        let ok = rep.all_caught()
            && rep.early == (10, 10)
            && rep.segloss_fire == (10, 10)
            && rep.segloss_sweep == (10, 10)
            && rep.resid == (10, 10)
            && rep.total_caught() == 40
            && rep.total_injected() == 40
            && inject_gate(&rep).is_ok();
        s.add("C28-14-注入核40-40", ok, if ok { "" } else { "注入核漏抓" });
    }

    // C28-15 诊断码闭集：0x1A31~0x1A3F 严格递增 + 段位正确 + 逐码两要素。
    {
        let codes = [
            XC_EARLY_FIRE,
            XC_P99_BREACH,
            XC_RESIDUAL_OOB,
            XC_SEG_LOST,
            XC_DRIFT_DETECTED,
            XC_FINGER_DRIFT,
            XC_DUALRUN_MISMATCH,
            XC_INJECT_MISSED,
            XC_EVENT_OVF,
            XC_SEALED,
            XC_CONGESTED,
            XC_PARAM_BAD,
            XC_LEDGER_BROKEN,
            XC_PROFILE_EMPTY,
            XC_HEALTH_DOWN,
        ];
        let mut ok = codes.len() == 15;
        let mut i = 0usize;
        while i < codes.len() {
            if i > 0 && codes[i] <= codes[i - 1] {
                ok = false;
            }
            if codes[i] <= TA_SEG_BASE as u32 || codes[i] > (TA_SEG_BASE as u32) + 0x0F {
                ok = false;
            }
            let (t, a) = diag_text(codes[i]);
            if t.is_empty() || a.is_empty() {
                ok = false;
            }
            i += 1;
        }
        s.add("C28-15-诊断码闭集", ok, if ok { "" } else { "诊断码段漂移" });
    }

    // C28-16 错误面：全变体 detail 非空 + code 落段。
    {
        let errs = [
            TmoAccError::Sealed,
            TmoAccError::ParamBad("p", 0),
            TmoAccError::Congested,
            TmoAccError::EarlyFire(1),
            TmoAccError::SegLoss(0),
            TmoAccError::ResidualOob(1),
            TmoAccError::P99Breach(1),
            TmoAccError::DriftDetected(1),
            TmoAccError::FingerDrift(1, 2),
            TmoAccError::DualRunMismatch(1, 2),
            TmoAccError::LedgerBroken("x"),
            TmoAccError::ProfileEmpty,
            TmoAccError::EventOvf(1),
            TmoAccError::InjectMissed(1),
        ];
        let mut ok = errs.len() == 14;
        let mut i = 0usize;
        while i < errs.len() {
            if errs[i].detail().is_empty() {
                ok = false;
            }
            if errs[i].code() <= TA_SEG_BASE as u32 || errs[i].code() > (TA_SEG_BASE as u32) + 0x0F {
                ok = false;
            }
            i += 1;
        }
        s.add("C28-16-错误面诊断", ok, if ok { "" } else { "错误变体诊断缺失" });
    }

    // C28-17 600 环滚动：700 推入 → 容量锁定 600、最老被覆盖、分位随之滚动。
    {
        let mut r = Ring600::new();
        let mut v = 1u64;
        while v <= 700 {
            r.push(v);
            v += 1;
        }
        // 覆盖后内容为 101..700；P50 = 第 300 小 = 400；P99 = 第 594 小 = 694。
        let ok = r.count() == 600
            && r.min_max() == Ok((101, 700))
            && r.percentile(50) == Ok(400)
            && r.percentile(99) == Ok(694);
        s.add("C28-17-600环滚动", ok, if ok { "" } else { "环覆盖语义失真" });
    }

    // C28-18 封账拒写：封账后 enqueued/sweep 全拒且 sweep 零扫出。
    {
        let mut led = TmoAccLedger::new();
        let seq = match led.begin_request(1, 1_000) {
            Ok(v) => v,
            Err(_) => 0,
        };
        let _ = led.seal();
        let w1 = led.enqueued(seq, 1_100);
        let swept = led.sweep_lost(999_999, 1);
        let w2 = led.begin_request(2, 2_000);
        let ok = w1 == Err(TmoAccError::Sealed)
            && swept == 0
            && w2 == Err(TmoAccError::Sealed)
            && led.sealed();
        s.add("C28-18-封账拒写", ok, if ok { "" } else { "封账后写面未拒" });
    }

    // C28-19 段丢失扫账：3 弃登记 + 2 弃入轮 → sweep 5 全扫出且缺失段号正确。
    {
        let mut led = TmoAccLedger::new();
        let mut k = 0u32;
        while k < 3 {
            let _ = led.begin_request(20 + k, 1_000);
            k += 1;
        }
        k = 0;
        while k < 2 {
            let sq = match led.begin_request(30 + k, 1_000) {
                Ok(v) => v,
                Err(_) => 0,
            };
            let _ = led.enqueued(sq, 1_100);
            k += 1;
        }
        let lost = led.sweep_lost(101_000, 50_000);
        let mut n_seg0 = 0usize;
        let mut n_seg1 = 0usize;
        for e in led.event_snapshot() {
            if e.code == XC_SEG_LOST && e.aux == 0 {
                n_seg0 += 1;
            }
            if e.code == XC_SEG_LOST && e.aux == 1 {
                n_seg1 += 1;
            }
        }
        let ok = lost == 5
            && led.seg_loss_total() == 5
            && led.inflight_len() == 0
            && n_seg0 == 3
            && n_seg1 == 2;
        s.add("C28-19-段丢失扫账", ok, if ok { "" } else { "扫账漏扫或段号错位" });
    }

    // C28-20 预算常量冻结：三段和=硬线=50µs、达标率线 99%、残差线 5%、环 600。
    {
        let table = budget_table();
        let ok = SEG_BUDGET_SETUP_NS + SEG_BUDGET_EXPIRY_NS + SEG_BUDGET_EXEC_NS
            == CHAIN_P99_LIMIT_NS
            && CHAIN_P99_LIMIT_NS == 50_000
            && ATTAIN_FLOOR_PPM == 990_000
            && RESIDUAL_LIMIT_PPM == 50_000
            && RING_CAP == 600
            && table.len() == 4
            && table[3].budget_ns == CHAIN_P99_LIMIT_NS
            && table[0].budget_ns == SEG_BUDGET_SETUP_NS
            && !table[3].owner.is_empty();
        s.add("C28-20-预算常量冻结", ok, if ok { "" } else { "F1014 预算常量漂移" });
    }

    s
}

/// 测试档（仅测试二进制编译；内核路径零 assert/panic）。
#[cfg(test)]
mod tests {
    use super::*;

    /// 冒烟：全判据通过。
    #[test]
    fn smoke_all_checks_pass() {
        let cs = run_unx1028_checks();
        assert!(cs.all_ok(), "unx1028 冒烟未全过：\n{}", cs.report());
    }

    /// 冒烟：三段分解 + p99 达标 + 残差 + 提前红账。
    #[test]
    fn smoke_triplet_and_gates() {
        let mut led = TmoAccLedger::new();
        let seq = match led.begin_request(1, 100) {
            Ok(v) => v,
            Err(_) => 0,
        };
        let _ = led.enqueued(seq, 150);
        let _ = led.fired(seq, 1_150, 1_180);
        let t = led.callback_entered(seq, 1_400, Some(290));
        match t {
            Ok(t) => {
                assert_eq!(t.setup_ns, 50);
                assert_eq!(t.expiry_ns, 30);
                assert_eq!(t.exec_ns, 220);
                assert_eq!(t.model_sum_ns, 300);
            }
            Err(e) => panic!("三元组产出失败：{}", e.detail()),
        }
        assert!(led.p99_gate().is_ok());
        assert!(led.residual_gate().is_ok());
        assert!(led.drift_gate().is_ok());
    }

    /// 冒烟：双跑一致 + 注入全抓。
    #[test]
    fn smoke_dual_and_inject() {
        let dr = match dual_run_gate() {
            Ok(r) => r,
            Err(e) => panic!("双跑对拍失败：{}", e.detail()),
        };
        assert!(dr.equal && dr.perturbed_differs);
        let rep = injection_core();
        assert!(rep.all_caught(), "注入核漏抓：{:?}", rep);
    }
}
