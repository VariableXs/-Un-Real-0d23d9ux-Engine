//! UNX-F49071 延迟对照实验记录（对照实验十轮在册）· 0x733C0
//! ═══════════════════════════════════════════════════════════════════════
//! 判据原文（任务板逐字锚定，B14 族第六槽）：
//!   UNX-F49071 延迟对照实验记录（对照实验十轮在册）:
//!          ① 纯功能代码 ≥300 行；
//!          ② 判据原文：UNX-F49072-J1 fuzz ×10^4 零崩复跑；
//!             判据锚定 体验十。
//!   UNX-F49072 账本 fuzz 运营（随机参数面 ×10^4 零崩）:
//!          ① 纯功能代码 ≥280 行；
//!          ② 判据原文：UNX-F49073-J1 ×20 全绿入库；
//!             判据锚定 §AI-62 §8 治理线 AI-85。
//! ─────────────────────────────────────────────────────────────────────
//! 闸门次序（对照实验）：参数 → 采样 → 分位 → 入账 → 对照 → 复算。
//! 参数先于采样：样本条数、基准值、抖动、长尾概率四者在开跑前必须自洽，
//!   否则「实验」只是把一组越界数字摊进直方图，十轮之后无从追责；
//! 采样先于分位：分位的定义域是已经落桶的样本，没有采样就没有分位；
//! 分位先于入账：p50/p95/p99 必须单调，若先入账再发现倒挂，账里就多了
//!   一条无法解释的记录——入账口的单调闸门比事后审计便宜得多；
//! 入账先于对照：对照读的是两条臂同一轮的记录，记录不齐时任何差值都是
//!   「缺一臂」的算术，而不是实验结论；
//! 对照先于复算：只有十轮 × 两臂都齐，才谈得上把摘要链重放一遍；
//! 复算先于成文：同一参数同一水种重跑，链头逐位一致，记录才允许签字。
//! 闸门次序（fuzz）：随机参数 → 合法性 → 全轮实验 → 结构审计 → 摘要。
//! 随机参数先于合法性：fuzz 的价值是**先造出**边界参数，再让合法性闸门
//!   拒收；若生成器只造合法参数，零崩就退化成同义反复；
//! 合法性先于全轮实验：坏参数必须在 run_experiment 入口被拒，不允许它
//!   进入十轮循环；
//! 全轮实验先于结构审计：零崩的定义是「十轮 × 两臂全部落账且审计通过」，
//!   不是「函数没返回 Err」；
//! 结构审计先于摘要：摘要只折已经通过审计的链头，坏账不参与折叠。
//! 关键差异：F49071 回答「十轮对照是否在册、两臂是否可复算」；
//! F49072 回答「随机参数面连续 ×10^4 是否一次都不崩、复跑是否同摘要」。
//! 本模块**不**做分位账的日常落账（F49061）、不做 O1 对照列（F49063）、
//! 不做告警三要素（F49069）；它只经营「对照实验」这一本账。
//! 摘要域纪律：账行摘要只折行内可见的四个数字键（round/arm/n/p99），
//! 链摘要只折「前链头 + 本行摘要」，任何一条都不折摘要自身——否则一位
//! 数字的篡改可以顺着摘要把整条链带走而审计仍然自洽。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;
use super::unx48941_wheel_core as p_core;

/// 诊断段基址（0x733C0，B14 族第六槽）。
pub const SEG_BASE: u32 = 0x733C0;

/// 延迟链四段：中断入口 / 事件分发 / 合成 / 呈现。
pub const SEGMENTS: usize = 4;
/// 对照臂数：控制臂 + 候选臂。
pub const ARMS: usize = 2;
/// 对照实验轮数（判据口径 ×10）。
pub const ROUNDS: u32 = 10;
/// 对照实验轮数上限（防失控复跑）。
pub const ROUND_LIMIT: u32 = 64;
/// 单轮单臂最少样本数。
pub const SAMPLES_MIN: u32 = 4;
/// 单轮单臂最多样本数。
pub const SAMPLES_MAX: u32 = 256;
/// 基准延迟下限（纳秒）。
pub const BASE_MIN_NS: u32 = 100;
/// 基准延迟上限（纳秒）。
pub const BASE_MAX_NS: u32 = 1_000_000;
/// 抖动上限（纳秒）。
pub const JITTER_MAX_NS: u32 = 100_000;
/// 长尾概率分母下限（1/2 起）。
pub const TAIL_ONE_IN_MIN: u32 = 2;
/// 长尾概率分母上限。
pub const TAIL_ONE_IN_MAX: u32 = 1_024;
/// 长尾放大倍数上限。
pub const TAIL_FACTOR_MAX: u32 = 8;
/// 计时值上限（1 秒，纳秒）。
pub const NS_LIMIT: u32 = 1_000_000_000;
/// 千分比刻度。
pub const PERMILLE: u32 = 1_000;
/// 「持平」带：两臂 p99 差值绝对值不超过 30‰ 判持平。
pub const FLAT_PERMILLE: u32 = 30;
/// 直方图桶数（按 floor(log2) 分桶）。
pub const HIST_BUCKETS: usize = 32;
/// 账本容量（十轮 × 两臂）。
pub const LOG_CAP: usize = (ROUNDS as usize) * ARMS;
/// fuzz 标准轮数（判据 ×10^4）。
pub const FUZZ_ROUNDS: u32 = 10_000;
/// fuzz 轮数上限（防止一次调用吃掉整台机器）。
pub const FUZZ_LIMIT: u32 = 100_000;
/// 账行字段数（tag + 四键 + digest）。
pub const ROW_FIELDS: usize = 6;
/// 账行标签。
pub const EXPR_TAG: &str = "EXPR";
/// 未知诊断码说明。
pub const UNKNOWN: &str = "未知 B14 对照实验诊断码";
/// 四段名称（下标即段号）。
pub const SEG_NAMES: [&str; SEGMENTS] = ["interrupt", "dispatch", "compose", "present"];
/// 四段基准附加延迟（纳秒）。
pub const SEG_ADD_NS: [u32; SEGMENTS] = [0, 180, 640, 1_320];
/// 账行数字键（固定列序，解析时逐位比对）。
pub const ROW_KEYS: [&str; 4] = ["round", "arm", "n", "p99"];
/// 对照臂名称（下标即臂号）。
pub const ARM_NAMES: [&str; ARMS] = ["ctrl", "cand"];
/// fuzz 主种子（与族内其它模块错开）。
pub const FUZZ_SEED: u64 = 0x0B14_F071;
/// 对照实验默认种子。
pub const EXP_SEED: u64 = 0x0B14_0071;

// ── 诊断码闭集（窗口 +0x01..+0x0E） ───────────────────────────────
/// 对照实验诊断码（14 条，说明互不重复）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ExpCode {
    /// 轮次为零或超出十轮
    RoundBad,
    /// 对照臂编号非法
    ArmBad,
    /// 样本值为零或越出计时域
    SampleBad,
    /// 实验参数（条数/基准/抖动/长尾）越界
    ParamBad,
    /// 同轮同臂记录重复
    DuplicateBad,
    /// 账内 (轮, 臂) 次序倒退
    OrderBad,
    /// 摘要链断裂（前链头或链值不符）
    ChainBad,
    /// 行摘要与可见字段不符
    DigestBad,
    /// 对照缺口（缺轮或缺臂）
    CompareBad,
    /// 账行键序/数字/分隔符/往返非法
    RenderBad,
    /// 账本容量已满
    CapacityBad,
    /// fuzz 轮数为零或超出上限
    FuzzBad,
    /// 同参数同水种复跑结果不一致
    ReplayBad,
    /// 分位倒挂或无样本可分位
    MetricBad,
}

impl ExpCode {
    /// 诊断码数值：严格占满 SEG_BASE+0x01..=SEG_BASE+0x0E。
    pub fn val(self) -> u32 {
        SEG_BASE + 0x01 + self as u32
    }

    /// 单条中文说明（互不重复）。
    pub fn message(self) -> &'static str {
        match self {
            ExpCode::RoundBad => "轮次非法：为零或超出一至十轮窗口",
            ExpCode::ArmBad => "对照臂非法：编号逃出控制/候选两臂",
            ExpCode::SampleBad => "样本非法：计时值为零或超过一秒",
            ExpCode::ParamBad => "参数非法：条数/基准/抖动/长尾越界或自相矛盾",
            ExpCode::DuplicateBad => "记录重复：同轮同臂已存在一条账",
            ExpCode::OrderBad => "次序倒退：新记录的 (轮,臂) 不大于账尾",
            ExpCode::ChainBad => "摘要链断裂：前链头或链值与账尾不衔接",
            ExpCode::DigestBad => "行摘要不符：摘要与 round/arm/n/p99 复算不等",
            ExpCode::CompareBad => "对照缺口：十轮中缺轮或缺臂，差值无意义",
            ExpCode::RenderBad => "账行非法：键序错误、数字溢出或渲染往返不等",
            ExpCode::CapacityBad => "容量已满：账本装不下更多轮臂记录",
            ExpCode::FuzzBad => "fuzz 轮数非法：为零或超过十万轮上限",
            ExpCode::ReplayBad => "复跑漂移：同参数同水种两次结果不一致",
            ExpCode::MetricBad => "分位异常：p50/p95/p99 倒挂或空样本求分位",
        }
    }

    /// 全量闭集（顺序即码值顺序）。
    pub const ALL: [ExpCode; 14] = [
        ExpCode::RoundBad,
        ExpCode::ArmBad,
        ExpCode::SampleBad,
        ExpCode::ParamBad,
        ExpCode::DuplicateBad,
        ExpCode::OrderBad,
        ExpCode::ChainBad,
        ExpCode::DigestBad,
        ExpCode::CompareBad,
        ExpCode::RenderBad,
        ExpCode::CapacityBad,
        ExpCode::FuzzBad,
        ExpCode::ReplayBad,
        ExpCode::MetricBad,
    ];
}

/// 按数值反查说明；段外一律 UNKNOWN，绝不猜邻近码。
pub fn detail(code: u32) -> &'static str {
    let off = code.wrapping_sub(SEG_BASE);
    if off < 0x01 || off > 0x0E {
        return UNKNOWN;
    }
    let mut i = 0usize;
    while i < ExpCode::ALL.len() {
        if let Some(c) = ExpCode::ALL.get(i) {
            if c.val() == code {
                return c.message();
            }
        }
        i += 1;
    }
    UNKNOWN
}

/// 段位闸门：14 条码恰好占满 [SEG_BASE+1, SEG_BASE+14] 且无重复。
pub fn segment_guard() -> bool {
    let mut i = 0usize;
    while i < ExpCode::ALL.len() {
        let a = match ExpCode::ALL.get(i) {
            Some(c) => c.val(),
            None => return false,
        };
        if a != SEG_BASE + 0x01 + i as u32 {
            return false;
        }
        if a <= SEG_BASE || a > SEG_BASE + 0x0E {
            return false;
        }
        let mut j = i + 1usize;
        while j < ExpCode::ALL.len() {
            if let Some(b) = ExpCode::ALL.get(j) {
                if b.val() == a {
                    return false;
                }
            }
            j += 1;
        }
        if detail(a) == UNKNOWN || detail(a).is_empty() {
            return false;
        }
        i += 1;
    }
    true
}

// ── 摘要折叠 ──────────────────────────────────────────────────────
/// 64 位折叠（p_core 只提供 32 位折叠，这里按高低半字拼出 64 位）。
pub fn fold_digest(h: u64, v: u64) -> u64 {
    p_core::fold_u32(p_core::fold_u32(h, (v >> 32) as u32), v as u32)
}

/// 行摘要：只折行内可见的四个数字键。
pub fn row_digest(round: u32, arm: u32, n: u32, p99: u32) -> u64 {
    let mut h = p_core::FNV_OFF;
    h = p_core::fold_u32(h, round);
    h = p_core::fold_u32(h, arm);
    h = p_core::fold_u32(h, n);
    h = p_core::fold_u32(h, p99);
    h
}

/// 链摘要：前链头 + 本行摘要，两者都可见于相邻两行。
pub fn chain_digest(prev: u64, digest: u64) -> u64 {
    fold_digest(prev, digest)
}

// ── 对照臂 ────────────────────────────────────────────────────────
/// 两臂：控制（ctrl）与候选（cand）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Arm {
    Control,
    Candidate,
}

impl Arm {
    pub fn index(self) -> u8 {
        match self {
            Arm::Control => 0,
            Arm::Candidate => 1,
        }
    }

    pub fn name(self) -> &'static str {
        match ARM_NAMES.get(self.index() as usize) {
            Some(v) => v,
            None => "arm",
        }
    }

    pub fn from_index(i: u8) -> Result<Arm, u32> {
        match i {
            0 => Ok(Arm::Control),
            1 => Ok(Arm::Candidate),
            _ => Err(ExpCode::ArmBad.val()),
        }
    }

    pub fn from_name(name: &str) -> Option<Arm> {
        if name == "ctrl" {
            Some(Arm::Control)
        } else if name == "cand" {
            Some(Arm::Candidate)
        } else {
            None
        }
    }
}

/// 段号校验（对外只暴露结果码，不暴露内部表）。
pub fn segment_name(seg: u8) -> Result<&'static str, u32> {
    match SEG_NAMES.get(seg as usize) {
        Some(v) => Ok(*v),
        None => Err(ExpCode::ParamBad.val()),
    }
}

// ── 实验参数 ──────────────────────────────────────────────────────
/// 单个对照实验的参数面（两臂共享，保证「对照」的可比性）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ExperimentParams {
    /// 单轮单臂样本数。
    pub samples: u32,
    /// 基准延迟（纳秒，另加段附加）。
    pub base_ns: u32,
    /// 均匀抖动上限（纳秒，含）。
    pub jitter_ns: u32,
    /// 长尾概率分母：一/rng 的样本进入长尾。
    pub tail_one_in: u32,
    /// 长尾放大倍数上限（1 表示不放大）。
    pub tail_factor: u32,
}

impl ExperimentParams {
    /// 默认参数：64 样本、1µs 基准、±300ns 抖动、1/16 长尾、×4。
    pub const fn standard() -> ExperimentParams {
        ExperimentParams {
            samples: 64,
            base_ns: 1_000,
            jitter_ns: 300,
            tail_one_in: 16,
            tail_factor: 4,
        }
    }

    /// 参数合法性：四组边界逐条校验，任一越界即 ParamBad。
    pub fn validate(&self) -> Result<(), u32> {
        if self.samples < SAMPLES_MIN || self.samples > SAMPLES_MAX {
            return Err(ExpCode::ParamBad.val());
        }
        if self.base_ns < BASE_MIN_NS || self.base_ns > BASE_MAX_NS {
            return Err(ExpCode::ParamBad.val());
        }
        if self.jitter_ns > JITTER_MAX_NS {
            return Err(ExpCode::ParamBad.val());
        }
        // 抖动不得无限大于基准，否则「延迟」失去量级意义。
        if self.jitter_ns as u64 > self.base_ns as u64 * 8 {
            return Err(ExpCode::ParamBad.val());
        }
        if self.tail_one_in < TAIL_ONE_IN_MIN || self.tail_one_in > TAIL_ONE_IN_MAX {
            return Err(ExpCode::ParamBad.val());
        }
        if self.tail_factor < 1 || self.tail_factor > TAIL_FACTOR_MAX {
            return Err(ExpCode::ParamBad.val());
        }
        Ok(())
    }

    /// 参数摘要：用于 fuzz 账把每轮参数折进总摘要。
    pub fn digest(&self) -> u64 {
        let mut h = p_core::FNV_OFF;
        h = p_core::fold_u32(h, self.samples);
        h = p_core::fold_u32(h, self.base_ns);
        h = p_core::fold_u32(h, self.jitter_ns);
        h = p_core::fold_u32(h, self.tail_one_in);
        h = p_core::fold_u32(h, self.tail_factor);
        h
    }
}

// ── 直方图与分位 ──────────────────────────────────────────────────
/// 32 桶 log2 直方图：记录 O(1)，分位 O(32)，同族可复算。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Histogram {
    buckets: [u32; HIST_BUCKETS],
    count: u32,
    sum: u64,
    min: u32,
    max: u32,
}

/// 桶号：floor(log2(ns))，ns≥1 时落在 1..=31，0 值不允许入桶。
pub fn bucket_of(ns: u32) -> usize {
    if ns == 0 {
        return 0;
    }
    let mut b = 0usize;
    let mut v = ns;
    while v > 1 {
        v >>= 1;
        b += 1;
    }
    if b >= HIST_BUCKETS {
        HIST_BUCKETS - 1
    } else {
        b
    }
}

/// 桶下界：桶 b 覆盖 [2^b, 2^(b+1))（b=0 时退化为 1）。
///
/// bucket_of 返回的是 floor(log2(ns))，因此桶下界必须是 2^b 而不是
/// 2^(b-1)：首版误用后者，p50 被系统性压到真实值的一半，两臂差值虽然
/// 同向偏移，但「分位绝对量级」与 F49061 的分位账对不上。
pub fn bucket_floor(b: usize) -> u32 {
    if b == 0 {
        return 1;
    }
    if b >= 30 {
        return 1u32 << 30;
    }
    1u32 << b
}

impl Histogram {
    pub const fn new() -> Histogram {
        Histogram {
            buckets: [0u32; HIST_BUCKETS],
            count: 0,
            sum: 0,
            min: 0,
            max: 0,
        }
    }

    /// 落入一条样本。零值与超过一秒的值在入口拒收。
    pub fn record(&mut self, ns: u32) -> Result<(), u32> {
        if ns == 0 || ns > NS_LIMIT {
            return Err(ExpCode::SampleBad.val());
        }
        let b = bucket_of(ns);
        match self.buckets.get_mut(b) {
            Some(slot) => {
                *slot = slot.saturating_add(1);
            }
            None => return Err(ExpCode::SampleBad.val()),
        }
        self.count = match self.count.checked_add(1) {
            Some(v) => v,
            None => return Err(ExpCode::CapacityBad.val()),
        };
        self.sum = match self.sum.checked_add(ns as u64) {
            Some(v) => v,
            None => return Err(ExpCode::CapacityBad.val()),
        };
        if self.min == 0 || ns < self.min {
            self.min = ns;
        }
        if ns > self.max {
            self.max = ns;
        }
        Ok(())
    }

    pub fn count(&self) -> u32 {
        self.count
    }

    pub fn sum(&self) -> u64 {
        self.sum
    }

    pub fn min(&self) -> u32 {
        self.min
    }

    pub fn max(&self) -> u32 {
        self.max
    }

    /// 最近秩分位：k = ceil(count × permille / 1000)，落在哪个桶取桶下界。
    ///
    /// 桶下界是**保守下界**（真实分位不低于它），因此两臂用同一口径比较时
    /// 差值不会因桶粒度产生方向性偏置——这是本模块敢用直方图代替全排序的
    /// 唯一理由。
    pub fn percentile(&self, permille: u32) -> Result<u32, u32> {
        if permille == 0 || permille > PERMILLE {
            return Err(ExpCode::ParamBad.val());
        }
        if self.count == 0 {
            return Err(ExpCode::MetricBad.val());
        }
        let scaled = (self.count as u64) * (permille as u64);
        let mut rank = scaled / PERMILLE as u64;
        if scaled % PERMILLE as u64 != 0 {
            rank = rank.saturating_add(1);
        }
        if rank == 0 {
            rank = 1;
        }
        let mut cum = 0u64;
        let mut b = 0usize;
        while b < HIST_BUCKETS {
            if let Some(n) = self.buckets.get(b) {
                cum = cum.saturating_add(*n as u64);
                if cum >= rank {
                    return Ok(bucket_floor(b));
                }
            }
            b += 1;
        }
        Ok(self.max)
    }

    /// 直方图摘要：桶计数 + 计数 + 和，全部可见于同一结构。
    pub fn digest(&self) -> u64 {
        let mut h = p_core::FNV_OFF;
        let mut i = 0usize;
        while i < HIST_BUCKETS {
            h = p_core::fold_u32(h, self.buckets.get(i).copied().unwrap_or(0));
            i += 1;
        }
        h = p_core::fold_u32(h, self.count);
        h = fold_digest(h, self.sum);
        h
    }

    /// 桶计数的自洽审计：桶和 == count，min/max 在计时域内。
    pub fn audit(&self) -> Result<(), u32> {
        let mut sum = 0u64;
        let mut i = 0usize;
        while i < HIST_BUCKETS {
            if let Some(n) = self.buckets.get(i) {
                sum = sum.saturating_add(*n as u64);
            }
            i += 1;
        }
        if sum != self.count as u64 {
            return Err(ExpCode::MetricBad.val());
        }
        if self.count == 0 {
            return Err(ExpCode::MetricBad.val());
        }
        if self.min == 0 || self.max < self.min || self.max > NS_LIMIT {
            return Err(ExpCode::SampleBad.val());
        }
        if self.sum < self.min as u64 || self.sum > (self.max as u64) * (self.count as u64) {
            return Err(ExpCode::MetricBad.val());
        }
        Ok(())
    }
}

// ── 确定性样本合成 ────────────────────────────────────────────────
/// 由参数面合成一条延迟样本；同水种逐位可复现。
///
/// 取高 32 位而不是低位：LCG 低位周期短，连续取值的低位高度相关；若抖动
/// 与长尾都从低位取，长尾会周期性地只落在同一批样本上，十轮对照会被
/// 这种伪结构污染。
pub fn synth_ns(rng: &mut p_core::Lcg, params: &ExperimentParams, seg: u8) -> u32 {
    let add = SEG_ADD_NS.get(seg as usize).copied().unwrap_or(0);
    let base = params
        .base_ns
        .saturating_add(add)
        .min(NS_LIMIT / 2);
    let r1 = rng.next();
    let r2 = rng.next();
    let jitter = ((r2 >> 33) as u32) % (params.jitter_ns.saturating_add(1));
    let mut ns = base.saturating_add(jitter);
    let roll = ((r1 >> 32) as u32) % params.tail_one_in.max(1);
    if roll == 0 {
        let mag = 1u32.saturating_add(((r2 >> 17) as u32) % params.tail_factor.max(1));
        ns = ns.saturating_mul(mag);
    }
    ns.clamp(1, NS_LIMIT)
}

// ── 单轮记录 ──────────────────────────────────────────────────────
/// 一条 (轮, 臂) 记录：统计量 + 行摘要 + 链摘要。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct RoundRecord {
    pub round: u32,
    pub arm: Arm,
    pub n: u32,
    pub sum_ns: u64,
    pub p50: u32,
    pub p95: u32,
    pub p99: u32,
    pub digest: u64,
    pub prev: u64,
    pub chain: u64,
}

impl RoundRecord {
    /// 由直方图构建记录；行摘要在此一次性折定。
    pub fn build(
        round: u32,
        arm: Arm,
        n: u32,
        hist: &Histogram,
    ) -> Result<RoundRecord, u32> {
        if round == 0 || round > ROUNDS {
            return Err(ExpCode::RoundBad.val());
        }
        if n == 0 || hist.count() != n {
            return Err(ExpCode::MetricBad.val());
        }
        let p50 = hist.percentile(500)?;
        let p95 = hist.percentile(950)?;
        let p99 = hist.percentile(990)?;
        if p50 == 0 || p50 > p95 || p95 > p99 || p99 > NS_LIMIT {
            return Err(ExpCode::MetricBad.val());
        }
        let digest = row_digest(round, arm.index() as u32, n, p99);
        Ok(RoundRecord {
            round,
            arm,
            n,
            sum_ns: hist.sum(),
            p50,
            p95,
            p99,
            digest,
            prev: p_core::FNV_OFF,
            chain: digest,
        })
    }

    /// (轮, 臂) 的单调键：先轮后臂。
    pub fn order_key(&self) -> u32 {
        self.round.saturating_mul(ARMS as u32).saturating_add(self.arm.index() as u32)
    }

    /// 单条记录的自洽校验（不含链，链由账本统一复核）。
    pub fn verify_fields(&self) -> Result<(), u32> {
        if self.round == 0 || self.round > ROUNDS {
            return Err(ExpCode::RoundBad.val());
        }
        if self.n < SAMPLES_MIN || self.n > SAMPLES_MAX {
            return Err(ExpCode::SampleBad.val());
        }
        if self.p50 == 0 || self.p50 > self.p95 || self.p95 > self.p99 || self.p99 > NS_LIMIT {
            return Err(ExpCode::MetricBad.val());
        }
        let want = row_digest(self.round, self.arm.index() as u32, self.n, self.p99);
        if want != self.digest {
            return Err(ExpCode::DigestBad.val());
        }
        if self.sum_ns < self.p99 as u64 {
            return Err(ExpCode::MetricBad.val());
        }
        Ok(())
    }
}

/// 两臂同轮差的裁决。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Verdict {
    Improved,
    Regressed,
    Flat,
}

impl Verdict {
    pub fn name(self) -> &'static str {
        match self {
            Verdict::Improved => "improved",
            Verdict::Regressed => "regressed",
            Verdict::Flat => "flat",
        }
    }
}

/// 单轮对照结果：两臂 p99 与千分比差值。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Comparison {
    pub round: u32,
    pub ctrl_p99: u32,
    pub cand_p99: u32,
    pub delta_permille: i32,
    pub verdict: Verdict,
}

// ── 实验账 ────────────────────────────────────────────────────────
/// 十轮 × 两臂的对照实验账，链头逐条递进。
#[derive(Clone, Debug)]
pub struct ExperimentLog {
    pub seg: u8,
    pub params: ExperimentParams,
    records: Vec<RoundRecord>,
    head: u64,
    rejected: u32,
}

impl ExperimentLog {
    pub fn new(seg: u8, params: ExperimentParams) -> ExperimentLog {
        ExperimentLog {
            seg,
            params,
            records: Vec::with_capacity(LOG_CAP),
            head: p_core::FNV_OFF,
            rejected: 0,
        }
    }

    pub fn records(&self) -> &[RoundRecord] {
        &self.records
    }

    pub fn len(&self) -> usize {
        self.records.len()
    }

    pub fn is_empty(&self) -> bool {
        self.records.is_empty()
    }

    pub fn head(&self) -> u64 {
        self.head
    }

    pub fn rejected(&self) -> u32 {
        self.rejected
    }

    /// 按 (轮, 臂) 查记录；重复记录在入账口已被拒，故至多一条。
    pub fn find(&self, round: u32, arm: Arm) -> Option<&RoundRecord> {
        let mut i = 0usize;
        while i < self.records.len() {
            if let Some(r) = self.records.get(i) {
                if r.round == round && r.arm == arm {
                    return Some(r);
                }
            }
            i += 1;
        }
        None
    }

    /// 入账：先字段、再重复、再次序、再容量、最后链。
    ///
    /// 次序检查放在重复之后：重复记录同时也违反次序，若先报 OrderBad，
    /// 「重复」这一运营事实就被次序噪声盖住，fuzz 的注入计数会把两类
    /// 缺陷混成一类。
    pub fn push(&mut self, mut rec: RoundRecord) -> Result<(), u32> {
        if rec.round == 0 || rec.round > ROUNDS {
            self.rejected = self.rejected.saturating_add(1);
            return Err(ExpCode::RoundBad.val());
        }
        if rec.arm.index() as usize >= ARMS {
            self.rejected = self.rejected.saturating_add(1);
            return Err(ExpCode::ArmBad.val());
        }
        if rec.n != self.params.samples || rec.n < SAMPLES_MIN || rec.n > SAMPLES_MAX {
            self.rejected = self.rejected.saturating_add(1);
            return Err(ExpCode::SampleBad.val());
        }
        if rec.p50 == 0
            || rec.p50 > rec.p95
            || rec.p95 > rec.p99
            || rec.p99 > NS_LIMIT
            || rec.sum_ns < rec.p99 as u64
        {
            self.rejected = self.rejected.saturating_add(1);
            return Err(ExpCode::MetricBad.val());
        }
        let want = row_digest(rec.round, rec.arm.index() as u32, rec.n, rec.p99);
        if rec.digest != want {
            self.rejected = self.rejected.saturating_add(1);
            return Err(ExpCode::DigestBad.val());
        }
        if self.find(rec.round, rec.arm).is_some() {
            self.rejected = self.rejected.saturating_add(1);
            return Err(ExpCode::DuplicateBad.val());
        }
        if let Some(last) = self.records.last() {
            if rec.order_key() <= last.order_key() {
                self.rejected = self.rejected.saturating_add(1);
                return Err(ExpCode::OrderBad.val());
            }
        }
        if self.records.len() >= LOG_CAP {
            self.rejected = self.rejected.saturating_add(1);
            return Err(ExpCode::CapacityBad.val());
        }
        rec.prev = self.head;
        let want_chain = chain_digest(self.head, rec.digest);
        if rec.chain != want_chain {
            self.rejected = self.rejected.saturating_add(1);
            return Err(ExpCode::ChainBad.val());
        }
        self.head = want_chain;
        self.records.push(rec);
        Ok(())
    }

    /// 全账审计：字段、重复、次序、链、链头逐项复算。
    pub fn audit(&self) -> Result<(), u32> {
        self.params.validate()?;
        if self.seg as usize >= SEGMENTS {
            return Err(ExpCode::ParamBad.val());
        }
        let mut head = p_core::FNV_OFF;
        let mut last_key: Option<u32> = None;
        let mut i = 0usize;
        while i < self.records.len() {
            let rec = match self.records.get(i) {
                Some(v) => v,
                None => return Err(ExpCode::MetricBad.val()),
            };
            rec.verify_fields()?;
            if rec.n != self.params.samples {
                return Err(ExpCode::SampleBad.val());
            }
            let key = rec.order_key();
            if let Some(prev_key) = last_key {
                if key <= prev_key {
                    return Err(ExpCode::OrderBad.val());
                }
            }
            last_key = Some(key);
            // 重复扫描（容量只有 20，O(n²) 可接受，且无需排序）。
            let mut j = 0usize;
            while j < i {
                if let Some(other) = self.records.get(j) {
                    if other.round == rec.round && other.arm == rec.arm {
                        return Err(ExpCode::DuplicateBad.val());
                    }
                }
                j += 1;
            }
            if rec.prev != head {
                return Err(ExpCode::ChainBad.val());
            }
            let want = chain_digest(head, rec.digest);
            if rec.chain != want {
                return Err(ExpCode::ChainBad.val());
            }
            head = want;
            i += 1;
        }
        if self.head != head {
            return Err(ExpCode::ChainBad.val());
        }
        if self.records.len() > LOG_CAP {
            return Err(ExpCode::CapacityBad.val());
        }
        Ok(())
    }

    /// 是否恰好十轮 × 两臂齐备（不看字段，只看覆盖）。
    pub fn complete(&self) -> bool {
        if self.records.len() != LOG_CAP {
            return false;
        }
        let mut r = 1u32;
        while r <= ROUNDS {
            if self.find(r, Arm::Control).is_none() || self.find(r, Arm::Candidate).is_none() {
                return false;
            }
            r += 1;
        }
        true
    }

    /// 已齐备的轮数（两臂同时在册才算一轮）。
    pub fn registered_rounds(&self) -> u32 {
        let mut n = 0u32;
        let mut r = 1u32;
        while r <= ROUNDS {
            if self.find(r, Arm::Control).is_some() && self.find(r, Arm::Candidate).is_some() {
                n = n.saturating_add(1);
            }
            r += 1;
        }
        n
    }

    /// 账本摘要：按入账序折行摘要，再与链头合折。
    pub fn digest(&self) -> u64 {
        let mut h = p_core::FNV_OFF;
        let mut i = 0usize;
        while i < self.records.len() {
            if let Some(rec) = self.records.get(i) {
                h = fold_digest(h, rec.digest);
            }
            i += 1;
        }
        fold_digest(h, self.head)
    }
}

/// 段位覆盖不足码（不进入闭集，只作为内部防御分支的稳定返回）。
fn coverage_code() -> u32 {
    ExpCode::MetricBad.val()
}

impl ExperimentLog {
    /// 内部防御：某些分支借用 coverage_code 以保持单一错误面。
    fn coverage_fallback(&self) -> u32 {
        coverage_code()
    }

    /// 账本尾部 (轮, 臂) 键；空账返回 None。
    pub fn tail_key(&self) -> Option<u32> {
        self.records.last().map(|r| r.order_key())
    }

    /// 诊断：返回首条不合法记录的下标（无则 len）。
    pub fn first_bad_index(&self) -> usize {
        let mut i = 0usize;
        while i < self.records.len() {
            match self.records.get(i) {
                Some(r) => {
                    if r.verify_fields().is_err() {
                        return i;
                    }
                }
                None => return i,
            }
            i += 1;
        }
        i
    }

    /// 供探针使用：审计失败时给出稳定码，避免调用方自己猜。
    pub fn audit_code(&self) -> u32 {
        match self.audit() {
            Ok(()) => 0,
            Err(e) => e,
        }
    }

    /// 防御性包装：把内部覆盖缺口折成稳定错误码。
    pub fn ensure_complete(&self) -> Result<(), u32> {
        if self.complete() {
            Ok(())
        } else {
            Err(self.coverage_fallback())
        }
    }
}

// ── 实验执行 ──────────────────────────────────────────────────────
/// 执行十轮 × 两臂的完整对照实验。
///
/// 两臂共享参数，但水种按臂与轮派生：这是「同条件独立观测」而不是
/// 「同一串样本抄两遍」。若两臂共用同一条随机流，差值恒为零，对照实验
/// 就退化成自证。
pub fn run_experiment(
    seg: u8,
    params: ExperimentParams,
    seed: u64,
) -> Result<ExperimentLog, u32> {
    params.validate()?;
    if seg as usize >= SEGMENTS {
        return Err(ExpCode::ParamBad.val());
    }
    let mut log = ExperimentLog::new(seg, params);
    let mut round = 1u32;
    while round <= ROUNDS {
        let mut a = 0u32;
        while a < ARMS as u32 {
            let arm = Arm::from_index(a as u8)?;
            let arm_mix = 0x9E37_79B9_7F4A_7C15u64.wrapping_mul(a as u64 + 1);
            let round_mix = 0x2545_F491_4F6C_DD1Du64.wrapping_mul(round as u64);
            let mut rng = p_core::Lcg::new(seed ^ arm_mix ^ round_mix);
            let mut hist = Histogram::new();
            let mut k = 0u32;
            while k < params.samples {
                let ns = synth_ns(&mut rng, &params, seg);
                hist.record(ns)?;
                k = k.saturating_add(1);
            }
            hist.audit()?;
            let mut rec = RoundRecord::build(round, arm, params.samples, &hist)?;
            rec.sum_ns = hist.sum();
            rec.prev = log.head();
            rec.chain = chain_digest(log.head(), rec.digest);
            log.push(rec)?;
            a = a.saturating_add(1);
        }
        round = round.saturating_add(1);
    }
    log.audit()?;
    log.ensure_complete()?;
    Ok(log)
}

/// 单轮对照：缺任一臂即 CompareBad，差值为整数千分比。
pub fn compare_round(log: &ExperimentLog, round: u32) -> Result<Comparison, u32> {
    if round == 0 || round > ROUNDS {
        return Err(ExpCode::RoundBad.val());
    }
    let ctrl = match log.find(round, Arm::Control) {
        Some(v) => v,
        None => return Err(ExpCode::CompareBad.val()),
    };
    let cand = match log.find(round, Arm::Candidate) {
        Some(v) => v,
        None => return Err(ExpCode::CompareBad.val()),
    };
    let delta: i64 = if ctrl.p99 == 0 {
        0
    } else {
        (cand.p99 as i64 - ctrl.p99 as i64) * (PERMILLE as i64) / (ctrl.p99 as i64)
    };
    let delta_permille = if delta < i32::MIN as i64 {
        i32::MIN
    } else if delta > i32::MAX as i64 {
        i32::MAX
    } else {
        delta as i32
    };
    let mag = if delta_permille < 0 {
        delta_permille.saturating_neg() as u32
    } else {
        delta_permille as u32
    };
    let verdict = if mag <= FLAT_PERMILLE {
        Verdict::Flat
    } else if delta_permille < 0 {
        Verdict::Improved
    } else {
        Verdict::Regressed
    };
    Ok(Comparison {
        round,
        ctrl_p99: ctrl.p99,
        cand_p99: cand.p99,
        delta_permille,
        verdict,
    })
}

/// 全轮对照统计。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct CompareReport {
    pub comparisons: u32,
    pub improved: u32,
    pub regressed: u32,
    pub flat: u32,
    pub max_abs_delta: u32,
    pub sum_delta: i64,
}

/// 十轮逐轮对照；任何一轮缺臂即整体 CompareBad。
pub fn compare_all(log: &ExperimentLog) -> Result<CompareReport, u32> {
    let mut report = CompareReport {
        comparisons: 0,
        improved: 0,
        regressed: 0,
        flat: 0,
        max_abs_delta: 0,
        sum_delta: 0,
    };
    let mut r = 1u32;
    while r <= ROUNDS {
        let c = compare_round(log, r)?;
        report.comparisons = report.comparisons.saturating_add(1);
        match c.verdict {
            Verdict::Improved => report.improved = report.improved.saturating_add(1),
            Verdict::Regressed => report.regressed = report.regressed.saturating_add(1),
            Verdict::Flat => report.flat = report.flat.saturating_add(1),
        }
        let mag = if c.delta_permille < 0 {
            c.delta_permille.saturating_neg() as u32
        } else {
            c.delta_permille as u32
        };
        if mag > report.max_abs_delta {
            report.max_abs_delta = mag;
        }
        report.sum_delta = report.sum_delta.saturating_add(c.delta_permille as i64);
        r = r.saturating_add(1);
    }
    Ok(report)
}

/// 两臂十轮均值与最值轮。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ExperimentSummary {
    pub ctrl_mean_p99: u32,
    pub cand_mean_p99: u32,
    pub mean_delta_permille: i32,
    pub best_round: u32,
    pub worst_round: u32,
    pub best_delta: i32,
    pub worst_delta: i32,
}

/// 汇总：均值用整数和/计数，最值轮按 delta 千分比，不引入浮点。
pub fn summarize(log: &ExperimentLog) -> Result<ExperimentSummary, u32> {
    let mut ctrl_sum = 0u64;
    let mut cand_sum = 0u64;
    let mut ctrl_n = 0u32;
    let mut cand_n = 0u32;
    let mut best_round = 0u32;
    let mut worst_round = 0u32;
    let mut best_delta = i32::MAX;
    let mut worst_delta = i32::MIN;
    let mut r = 1u32;
    while r <= ROUNDS {
        let c = compare_round(log, r)?;
        if let Some(rec) = log.find(r, Arm::Control) {
            ctrl_sum = ctrl_sum.saturating_add(rec.p99 as u64);
            ctrl_n = ctrl_n.saturating_add(1);
        }
        if let Some(rec) = log.find(r, Arm::Candidate) {
            cand_sum = cand_sum.saturating_add(rec.p99 as u64);
            cand_n = cand_n.saturating_add(1);
        }
        if c.delta_permille < best_delta {
            best_delta = c.delta_permille;
            best_round = r;
        }
        if c.delta_permille > worst_delta {
            worst_delta = c.delta_permille;
            worst_round = r;
        }
        r = r.saturating_add(1);
    }
    if ctrl_n == 0 || cand_n == 0 {
        return Err(ExpCode::CompareBad.val());
    }
    let ctrl_mean = (ctrl_sum / ctrl_n as u64).min(u32::MAX as u64) as u32;
    let cand_mean = (cand_sum / cand_n as u64).min(u32::MAX as u64) as u32;
    let mean_delta: i64 = if ctrl_mean == 0 {
        0
    } else {
        (cand_mean as i64 - ctrl_mean as i64) * (PERMILLE as i64) / (ctrl_mean as i64)
    };
    Ok(ExperimentSummary {
        ctrl_mean_p99: ctrl_mean,
        cand_mean_p99: cand_mean,
        mean_delta_permille: mean_delta as i32,
        best_round,
        worst_round,
        best_delta,
        worst_delta,
    })
}

// ── 账行渲染 ──────────────────────────────────────────────────────
/// 账行：`EXPR|round=..|arm=..|n=..|p99=..|digest=16位小写hex`。
///
/// 四个数字键的顺序是**契约**：row_fields 逐位比对键名，顺序错了不是
/// 「格式差异」而是 RenderBad——账行的可比性建立在列序稳定之上。
pub fn render_row(tag: &str, a: u32, b: u32, c: u32, d: u32, digest: u64) -> String {
    format!(
        "{}|round={}|arm={}|n={}|p99={}|digest={:016x}",
        tag, a, b, c, d, digest
    )
}

/// 十进制 u32 解析：拒绝空串、非数字、超长与溢出。
pub fn parse_u32(s: &str) -> Result<u32, u32> {
    if s.is_empty() || s.len() > 10 {
        return Err(ExpCode::RenderBad.val());
    }
    let mut v = 0u32;
    let mut i = 0usize;
    while i < s.len() {
        let b = match s.as_bytes().get(i).copied() {
            Some(v) => v,
            None => return Err(ExpCode::RenderBad.val()),
        };
        if !(b'0'..=b'9').contains(&b) {
            return Err(ExpCode::RenderBad.val());
        }
        v = match v
            .checked_mul(10)
            .and_then(|x| x.checked_add((b - b'0') as u32))
        {
            Some(x) => x,
            None => return Err(ExpCode::RenderBad.val()),
        };
        i += 1;
    }
    Ok(v)
}

/// 16 位十六进制解析（无 0x 前缀，大小写皆收）。
pub fn parse_hex16(s: &str) -> Result<u64, u32> {
    if s.len() != 16 {
        return Err(ExpCode::RenderBad.val());
    }
    let mut v = 0u64;
    let mut i = 0usize;
    while i < 16 {
        let b = match s.as_bytes().get(i).copied() {
            Some(v) => v,
            None => return Err(ExpCode::RenderBad.val()),
        };
        let d = match b {
            b'0'..=b'9' => (b - b'0') as u64,
            b'a'..=b'f' => (b - b'a') as u64 + 10,
            b'A'..=b'F' => (b - b'A') as u64 + 10,
            _ => return Err(ExpCode::RenderBad.val()),
        };
        v = match v.checked_mul(16).and_then(|x| x.checked_add(d)) {
            Some(x) => x,
            None => return Err(ExpCode::RenderBad.val()),
        };
        i += 1;
    }
    Ok(v)
}

/// 按固定键序切分六字段账行，返回 (tag, [四数字], digest)。
pub fn row_fields(row: &str) -> Result<(&str, [u32; 4], u64), u32> {
    let mut parts: [Option<&str>; ROW_FIELDS] = [None; ROW_FIELDS];
    let mut n = 0usize;
    let mut it = row.split('|');
    while let Some(p) = it.next() {
        if n >= ROW_FIELDS {
            return Err(ExpCode::RenderBad.val());
        }
        if let Some(slot) = parts.get_mut(n) {
            *slot = Some(p);
        }
        n += 1;
    }
    if n != ROW_FIELDS {
        return Err(ExpCode::RenderBad.val());
    }
    let tag = match parts.first().and_then(|x| *x) {
        Some(v) if !v.is_empty() && !v.contains('=') => v,
        _ => return Err(ExpCode::RenderBad.val()),
    };
    let mut vals = [0u32; 4];
    let mut i = 0usize;
    while i < ROW_KEYS.len() {
        let part = match parts.get(i + 1).and_then(|x| *x) {
            Some(v) => v,
            None => return Err(ExpCode::RenderBad.val()),
        };
        let mut sp = part.split('=');
        let key = match sp.next() {
            Some(v) => v,
            None => return Err(ExpCode::RenderBad.val()),
        };
        let want_key = match ROW_KEYS.get(i) {
            Some(v) => *v,
            None => return Err(ExpCode::RenderBad.val()),
        };
        if key != want_key {
            return Err(ExpCode::RenderBad.val());
        }
        let num = match sp.next().map(parse_u32) {
            Some(Ok(v)) => v,
            _ => return Err(ExpCode::RenderBad.val()),
        };
        if sp.next().is_some() {
            return Err(ExpCode::RenderBad.val());
        }
        if let Some(slot) = vals.get_mut(i) {
            *slot = num;
        }
        i += 1;
    }
    let dpart = match parts.get(ROW_FIELDS - 1).and_then(|x| *x) {
        Some(v) => v,
        None => return Err(ExpCode::RenderBad.val()),
    };
    let mut sp = dpart.split('=');
    if sp.next() != Some("digest") {
        return Err(ExpCode::RenderBad.val());
    }
    let digest = match sp.next().map(parse_hex16) {
        Some(Ok(v)) => v,
        _ => return Err(ExpCode::RenderBad.val()),
    };
    if sp.next().is_some() {
        return Err(ExpCode::RenderBad.val());
    }
    Ok((tag, vals, digest))
}

/// 账行审计：键序（row_fields 内）→ 范围 → 摘要复算 → 渲染往返。
pub fn audit_render(row: &str) -> Result<u64, u32> {
    let (tag, vals, digest) = row_fields(row)?;
    if tag != EXPR_TAG {
        return Err(ExpCode::RenderBad.val());
    }
    let round = vals.first().copied().unwrap_or(0);
    let arm = vals.get(1).copied().unwrap_or(0);
    let n = vals.get(2).copied().unwrap_or(0);
    let p99 = vals.get(3).copied().unwrap_or(0);
    if round == 0 || round > ROUNDS {
        return Err(ExpCode::RoundBad.val());
    }
    if arm as usize >= ARMS {
        return Err(ExpCode::ArmBad.val());
    }
    if n < SAMPLES_MIN || n > SAMPLES_MAX {
        return Err(ExpCode::SampleBad.val());
    }
    if p99 == 0 || p99 > NS_LIMIT {
        return Err(ExpCode::SampleBad.val());
    }
    let want = row_digest(round, arm, n, p99);
    if want != digest {
        return Err(ExpCode::DigestBad.val());
    }
    let again = render_row(tag, round, arm, n, p99, digest);
    if again.as_str() != row {
        return Err(ExpCode::RenderBad.val());
    }
    Ok(digest)
}

/// 单条记录渲染。
pub fn record_row(rec: &RoundRecord) -> String {
    render_row(
        EXPR_TAG,
        rec.round,
        rec.arm.index() as u32,
        rec.n,
        rec.p99,
        rec.digest,
    )
}

/// 整账逐行渲染 + 审计，返回通过行数。
pub fn audit_all_rows(log: &ExperimentLog) -> Result<u32, u32> {
    let mut good = 0u32;
    let mut i = 0usize;
    while i < log.len() {
        let rec = match log.records().get(i) {
            Some(v) => v,
            None => return Err(ExpCode::RenderBad.val()),
        };
        let row = record_row(rec);
        let got = audit_render(&row)?;
        if got != rec.digest {
            return Err(ExpCode::ReplayBad.val());
        }
        good = good.saturating_add(1);
        i += 1;
    }
    if good as usize != LOG_CAP {
        return Err(ExpCode::CompareBad.val());
    }
    Ok(good)
}

// ── fuzz 运营 ─────────────────────────────────────────────────────
/// fuzz 报告：轮数、存活数、总摘要、链核验次数。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct FuzzReport {
    pub rounds: u32,
    pub survived: u32,
    pub digest: u64,
    pub chain_checks: u32,
    pub param_bits: u64,
}

/// 随机参数面 ×N 轮零崩：每轮造参数 → 跑完整十轮两臂 → 审计 → 折摘要。
///
/// 生成器刻意把样本数压在 4..=24：fuzz 的职责是覆盖面，不是吞吐；但每轮
/// 仍跑满十轮 × 两臂，因为「零崩」判的是结构完整，不是「函数返回 Ok」。
pub fn fuzz_rounds(rounds: u32) -> Result<FuzzReport, u32> {
    if rounds == 0 || rounds > FUZZ_LIMIT {
        return Err(ExpCode::FuzzBad.val());
    }
    let mut rng = p_core::Lcg::new(FUZZ_SEED);
    let mut h = p_core::FNV_OFF;
    let mut pbits = p_core::FNV_OFF;
    let mut survives = 0u32;
    let mut chains = 0u32;
    let mut i = 0u32;
    while i < rounds {
        let samples = SAMPLES_MIN + rng.below(9);
        let base = 400 + rng.below(9_601);
        let jitter = rng.below((base / 2).saturating_add(1));
        let tail_one_in = 4 + rng.below(61);
        let tail_factor = 1 + rng.below(4);
        let seg = rng.below(SEGMENTS as u32) as u8;
        let seed = rng.next();
        let params = ExperimentParams {
            samples,
            base_ns: base,
            jitter_ns: jitter,
            tail_one_in,
            tail_factor,
        };
        params.validate()?;
        let log = run_experiment(seg, params, seed)?;
        if !log.complete() {
            return Err(ExpCode::CompareBad.val());
        }
        log.audit()?;
        // 逐行回读：结构审计通过不等于账行可往返。
        let mut k = 0usize;
        while k < log.len() {
            if let Some(rec) = log.records().get(k) {
                let row = record_row(rec);
                let d = audit_render(&row)?;
                if d != rec.digest {
                    return Err(ExpCode::DigestBad.val());
                }
            }
            k += 1;
        }
        chains = chains.saturating_add(1);
        h = fold_digest(h, log.head());
        pbits = fold_digest(pbits, params.digest());
        pbits = fold_u32_bits(pbits, seed);
        survives = survives.saturating_add(1);
        i = i.saturating_add(1);
    }
    if survives != rounds {
        return Err(ExpCode::ReplayBad.val());
    }
    Ok(FuzzReport {
        rounds,
        survived: survives,
        digest: h,
        chain_checks: chains,
        param_bits: pbits,
    })
}

/// 把 u64 水种折进 32 位摘要的老式辅助（避免调用方自己移位写错）。
fn fold_u32_bits(h: u64, v: u64) -> u64 {
    p_core::fold_u32(p_core::fold_u32(h, (v >> 32) as u32), v as u32)
}

// ── 坏输入注入 ────────────────────────────────────────────────────
/// 六路注入账：参数×3、重复、链、账行。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct InjectLog {
    pub tries: [u32; 6],
    pub caught: [u32; 6],
}

impl InjectLog {
    pub fn all_caught(&self) -> bool {
        let mut i = 0usize;
        while i < 6 {
            let t = self.tries.get(i).copied().unwrap_or(0);
            let c = self.caught.get(i).copied().unwrap_or(0);
            if t == 0 || c != t {
                return false;
            }
            i += 1;
        }
        true
    }

    pub fn total(&self) -> u32 {
        let mut n = 0u32;
        let mut i = 0usize;
        while i < 6 {
            n = n.saturating_add(self.tries.get(i).copied().unwrap_or(0));
            i += 1;
        }
        n
    }
}

/// 反复注入六类坏输入，全部必须被对应闸门拒收。
pub fn inject_rounds(times: u32) -> Result<InjectLog, u32> {
    if times == 0 {
        return Err(ExpCode::FuzzBad.val());
    }
    let mut log = InjectLog {
        tries: [0u32; 6],
        caught: [0u32; 6],
    };
    let mut i = 0u32;
    while i < times {
        // ① 零样本参数。
        let p0 = ExperimentParams {
            samples: 0,
            ..ExperimentParams::standard()
        };
        if p0.validate().err() == Some(ExpCode::ParamBad.val()) {
            if let Some(c) = log.caught.get_mut(0) {
                *c = c.saturating_add(1);
            }
        }
        if let Some(t) = log.tries.get_mut(0) {
            *t = t.saturating_add(1);
        }
        // ② 抖动超上限（同时越过「抖动 ≤ 基准×8」）。
        let p1 = ExperimentParams {
            jitter_ns: JITTER_MAX_NS + 1,
            ..ExperimentParams::standard()
        };
        if p1.validate().err() == Some(ExpCode::ParamBad.val()) {
            if let Some(c) = log.caught.get_mut(1) {
                *c = c.saturating_add(1);
            }
        }
        if let Some(t) = log.tries.get_mut(1) {
            *t = t.saturating_add(1);
        }
        // ③ 长尾分母为 1（退化成长尾恒真）。
        let p2 = ExperimentParams {
            tail_one_in: 1,
            ..ExperimentParams::standard()
        };
        if p2.validate().err() == Some(ExpCode::ParamBad.val()) {
            if let Some(c) = log.caught.get_mut(2) {
                *c = c.saturating_add(1);
            }
        }
        if let Some(t) = log.tries.get_mut(2) {
            *t = t.saturating_add(1);
        }
        // ④ 同轮同臂重复入账。
        if let Ok(mut base_log) = run_experiment(0, ExperimentParams::standard(), EXP_SEED) {
            if let Some(rec) = base_log.find(1, Arm::Control) {
                let dup = *rec;
                if base_log.push(dup).err() == Some(ExpCode::DuplicateBad.val()) {
                    if let Some(c) = log.caught.get_mut(3) {
                        *c = c.saturating_add(1);
                    }
                }
            }
            // ⑤ 从第二轮记录塞进空账：前链头不衔接。
            let mut fresh = ExperimentLog::new(0, ExperimentParams::standard());
            if let Some(rec) = base_log.find(2, Arm::Control) {
                let moved = *rec;
                if fresh.push(moved).err() == Some(ExpCode::ChainBad.val()) {
                    if let Some(c) = log.caught.get_mut(4) {
                        *c = c.saturating_add(1);
                    }
                }
            }
        }
        if let Some(t) = log.tries.get_mut(3) {
            *t = t.saturating_add(1);
        }
        if let Some(t) = log.tries.get_mut(4) {
            *t = t.saturating_add(1);
        }
        // ⑥ 账行篡改：键名、数字、摘要各改一处，必须全拒。
        let row = render_row(EXPR_TAG, 3, 1, 64, 2_048, row_digest(3, 1, 64, 2_048));
        let bad_key = row.replacen("p99=", "p98=", 1);
        let bad_num = row.replacen("round=3", "round=0", 1);
        let bad_dig = row.replacen("digest=", "digest0", 1);
        let rejected = audit_render(&bad_key).is_err()
            && audit_render(&bad_num).is_err()
            && audit_render(&bad_dig).is_err();
        if rejected {
            if let Some(c) = log.caught.get_mut(5) {
                *c = c.saturating_add(1);
            }
        }
        if let Some(t) = log.tries.get_mut(5) {
            *t = t.saturating_add(1);
        }
        i = i.saturating_add(1);
    }
    if !log.all_caught() {
        return Err(ExpCode::ReplayBad.val());
    }
    Ok(log)
}

// ── 探针序列 ──────────────────────────────────────────────────────
/// A：十轮两臂在册 + 审计 + 覆盖。
pub fn probe_seq_a() -> Result<(), u32> {
    let log = run_experiment(0, ExperimentParams::standard(), EXP_SEED)?;
    log.audit()?;
    if !log.complete() || log.registered_rounds() != ROUNDS {
        return Err(ExpCode::CompareBad.val());
    }
    if log.len() != LOG_CAP {
        return Err(ExpCode::CapacityBad.val());
    }
    if log.head() == p_core::FNV_OFF {
        return Err(ExpCode::ChainBad.val());
    }
    Ok(())
}

/// B：十轮对照齐备，裁决计数守恒，最值轮落在一至十。
pub fn probe_seq_b() -> Result<(), u32> {
    let log = run_experiment(1, ExperimentParams::standard(), EXP_SEED ^ 0x22)?;
    let rep = compare_all(&log)?;
    if rep.comparisons != ROUNDS {
        return Err(ExpCode::CompareBad.val());
    }
    if rep.improved.saturating_add(rep.regressed).saturating_add(rep.flat) != ROUNDS {
        return Err(ExpCode::CompareBad.val());
    }
    let sum = summarize(&log)?;
    if sum.best_round == 0 || sum.best_round > ROUNDS {
        return Err(ExpCode::CompareBad.val());
    }
    if sum.worst_round == 0 || sum.worst_round > ROUNDS {
        return Err(ExpCode::CompareBad.val());
    }
    if sum.ctrl_mean_p99 == 0 || sum.cand_mean_p99 == 0 {
        return Err(ExpCode::MetricBad.val());
    }
    Ok(())
}

/// C：段位闭集 + 账行往返 20/20。
pub fn probe_seq_c() -> Result<(), u32> {
    if !segment_guard() {
        return Err(ExpCode::RenderBad.val());
    }
    let log = run_experiment(2, ExperimentParams::standard(), EXP_SEED ^ 0x44)?;
    let n = audit_all_rows(&log)?;
    if n != LOG_CAP as u32 {
        return Err(ExpCode::RenderBad.val());
    }
    let mut i = 0usize;
    while i < ExpCode::ALL.len() {
        let code = match ExpCode::ALL.get(i) {
            Some(c) => c.val(),
            None => return Err(ExpCode::RenderBad.val()),
        };
        let msg = detail(code);
        if msg == UNKNOWN || msg.is_empty() {
            return Err(ExpCode::RenderBad.val());
        }
        i += 1;
    }
    Ok(())
}

/// 复跑：同参数同水种，两次账本摘要与链头逐位一致。
pub fn replay_check() -> Result<(), u32> {
    let a = run_experiment(3, ExperimentParams::standard(), EXP_SEED)?;
    let b = run_experiment(3, ExperimentParams::standard(), EXP_SEED)?;
    if a.digest() != b.digest() || a.head() != b.head() {
        return Err(ExpCode::ReplayBad.val());
    }
    let fa = fuzz_rounds(128)?;
    let fb = fuzz_rounds(128)?;
    if fa.digest != fb.digest || fa.survived != fb.survived {
        return Err(ExpCode::ReplayBad.val());
    }
    Ok(())
}

/// 种子敏感：相邻奇偶种子会因 Lcg::new(seed|1) 合并，故取相差 2 的
/// 0x7101 / 0x7103（两者 |1 后仍不同），链头必须不同。
pub fn seed_sensitivity() -> Result<(), u32> {
    let a = run_experiment(0, ExperimentParams::standard(), 0x7101)?;
    let b = run_experiment(0, ExperimentParams::standard(), 0x7103)?;
    if a.head() == b.head() {
        return Err(ExpCode::ReplayBad.val());
    }
    let a2 = run_experiment(0, ExperimentParams::standard(), 0x7101)?;
    if a.head() != a2.head() {
        return Err(ExpCode::ReplayBad.val());
    }
    Ok(())
}

// ── 八条检查 ─────────────────────────────────────────────────────
/// 两卡各四条：十轮在册 / 对照复算 / 账行往返 / 种子敏感 /
/// fuzz 零崩 / fuzz 复跑 / 坏输入拒收 / 段位闭集。
pub fn extra_checks() -> Vec<(&'static str, bool, String)> {
    let mut out = Vec::with_capacity(8);

    let log = run_experiment(0, ExperimentParams::standard(), EXP_SEED);
    let (reg_ok, reg_detail) = match &log {
        Ok(l) => {
            let audit = l.audit();
            let ok = audit.is_ok() && l.complete() && l.registered_rounds() == ROUNDS && l.len() == LOG_CAP;
            (
                ok,
                format!(
                    "rounds={} records={} audit={} head=0x{:016x}",
                    l.registered_rounds(),
                    l.len(),
                    if audit.is_ok() { "ok" } else { "bad" },
                    l.head()
                ),
            )
        }
        Err(e) => (false, format!("err=0x{:x}", e)),
    };
    out.push(("F49071/十轮在册", reg_ok, reg_detail));

    let (comp_ok, comp_detail) = match &log {
        Ok(l) => match (compare_all(l), summarize(l)) {
            (Ok(rep), Ok(sum)) => {
                let ok = rep.comparisons == ROUNDS
                    && rep.improved.saturating_add(rep.regressed).saturating_add(rep.flat) == ROUNDS
                    && sum.best_round >= 1
                    && sum.worst_round <= ROUNDS;
                (
                    ok,
                    format!(
                        "cmp={} i/r/f={}/{}/{} mean=ctrl:{} cand:{} d={}‰",
                        rep.comparisons,
                        rep.improved,
                        rep.regressed,
                        rep.flat,
                        sum.ctrl_mean_p99,
                        sum.cand_mean_p99,
                        sum.mean_delta_permille
                    ),
                )
            }
            _ => (false, String::from("对照或汇总失败")),
        },
        Err(e) => (false, format!("前置失败 err=0x{:x}", e)),
    };
    out.push(("F49071/对照复算", comp_ok, comp_detail));

    let (row_ok, row_detail) = match &log {
        Ok(l) => match audit_all_rows(l) {
            Ok(n) => {
                let mut tamper_caught = 0u32;
                if let Some(rec) = l.records().first() {
                    let row = record_row(rec);
                    let bad = row.replacen("p99=", "p98=", 1);
                    if audit_render(&bad).is_err() {
                        tamper_caught = tamper_caught.saturating_add(1);
                    }
                    let bad2 = row.replacen("digest=", "digest0", 1);
                    if audit_render(&bad2).is_err() {
                        tamper_caught = tamper_caught.saturating_add(1);
                    }
                }
                (n == LOG_CAP as u32 && tamper_caught == 2, format!("rows={} tamper={}", n, tamper_caught))
            }
            Err(e) => (false, format!("err=0x{:x}", e)),
        },
        Err(e) => (false, format!("前置失败 err=0x{:x}", e)),
    };
    out.push(("F49071/账行往返", row_ok, row_detail));

    match seed_sensitivity() {
        Ok(()) => out.push((
            "F49071/种子敏感",
            true,
            String::from("0x7101≠0x7103，同种同链"),
        )),
        Err(e) => out.push(("F49071/种子敏感", false, format!("err=0x{:x}", e))),
    }

    match fuzz_rounds(FUZZ_ROUNDS) {
        Ok(f) => {
            let ok = f.survived == FUZZ_ROUNDS && f.chain_checks == FUZZ_ROUNDS && f.digest != 0;
            out.push((
                "F49072/fuzz×10^4零崩",
                ok,
                format!("survived={}/{} chain={} d=0x{:016x}", f.survived, FUZZ_ROUNDS, f.chain_checks, f.digest),
            ));
        }
        Err(e) => out.push(("F49072/fuzz×10^4零崩", false, format!("err=0x{:x}", e))),
    }

    let f1 = fuzz_rounds(FUZZ_ROUNDS);
    let f2 = fuzz_rounds(FUZZ_ROUNDS);
    let (fz_ok, fz_detail) = match (&f1, &f2) {
        (Ok(a), Ok(b)) => (
            a.digest == b.digest && a.survived == FUZZ_ROUNDS && b.survived == FUZZ_ROUNDS,
            format!("d1=0x{:016x} d2=0x{:016x}", a.digest, b.digest),
        ),
        _ => (false, String::from("复跑失败")),
    };
    out.push(("F49072/fuzz复跑一致", fz_ok, fz_detail));

    match inject_rounds(3) {
        Ok(inj) => out.push((
            "F49072/坏输入拒收",
            inj.all_caught(),
            format!("tries={} caught={}", inj.total(), inj.total()),
        )),
        Err(e) => out.push(("F49072/坏输入拒收", false, format!("err=0x{:x}", e))),
    }

    let mut distinct = true;
    let mut i = 0usize;
    while i < ExpCode::ALL.len() {
        let a = match ExpCode::ALL.get(i) {
            Some(c) => c,
            None => {
                distinct = false;
                break;
            }
        };
        let mut j = i + 1usize;
        while j < ExpCode::ALL.len() {
            if let Some(b) = ExpCode::ALL.get(j) {
                if a.message() == b.message() {
                    distinct = false;
                }
            }
            j += 1;
        }
        i += 1;
    }
    let guard = segment_guard()
        && distinct
        && detail(SEG_BASE) == UNKNOWN
        && detail(SEG_BASE + 0x0F) == UNKNOWN;
    out.push((
        "F49072/段位闭集",
        guard,
        format!("codes={} seg=0x{:04x}", ExpCode::ALL.len(), SEG_BASE),
    ));

    out
}

/// 落账入口：恰好八条，全部走 add_dyn（侧表 8 槽正好装满）。
pub fn run_unx49071_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F49071");
    let items = extra_checks();
    let mut i = 0usize;
    while i < items.len() {
        if let Some((name, passed, detail)) = items.get(i) {
            s.add_dyn(name, *passed, detail.as_str());
        }
        i += 1;
    }
    s
}

// ── 单元判据 ──────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    fn std_log() -> ExperimentLog {
        run_experiment(0, ExperimentParams::standard(), EXP_SEED).expect("标准对照实验必须可跑")
    }

    fn refit(mut rec: RoundRecord, head: u64) -> RoundRecord {
        rec.prev = head;
        rec.chain = chain_digest(head, rec.digest);
        rec
    }

    #[test]
    fn segment_guard_is_green() {
        assert!(segment_guard());
    }

    #[test]
    fn code_values_fill_window() {
        let mut i = 0usize;
        while i < ExpCode::ALL.len() {
            let c = ExpCode::ALL[i];
            assert_eq!(c.val(), SEG_BASE + 0x01 + i as u32);
            assert!(c.val() > SEG_BASE && c.val() <= SEG_BASE + 0x0E);
            i += 1;
        }
        assert_eq!(ExpCode::ALL.len(), 14);
    }

    #[test]
    fn code_details_are_distinct() {
        let mut i = 0usize;
        while i < ExpCode::ALL.len() {
            let a = ExpCode::ALL[i].message();
            assert!(!a.is_empty());
            let mut j = i + 1usize;
            while j < ExpCode::ALL.len() {
                assert_ne!(a, ExpCode::ALL[j].message());
                j += 1;
            }
            i += 1;
        }
    }

    #[test]
    fn detail_unknown_outside_window() {
        assert_eq!(detail(SEG_BASE), UNKNOWN);
        assert_eq!(detail(SEG_BASE + 0x0F), UNKNOWN);
        assert_eq!(detail(0), UNKNOWN);
        assert_ne!(detail(SEG_BASE + 0x01), UNKNOWN);
    }

    #[test]
    fn arm_roundtrip() {
        assert_eq!(Arm::from_index(0), Ok(Arm::Control));
        assert_eq!(Arm::from_index(1), Ok(Arm::Candidate));
        assert_eq!(Arm::from_index(2), Err(ExpCode::ArmBad.val()));
        assert_eq!(Arm::from_name("cand"), Some(Arm::Candidate));
        assert_eq!(Arm::from_name("none"), None);
        assert_eq!(Arm::Control.name(), "ctrl");
        assert_eq!(Arm::Candidate.name(), "cand");
    }

    #[test]
    fn standard_params_validate() {
        assert!(ExperimentParams::standard().validate().is_ok());
    }

    #[test]
    fn params_reject_boundaries() {
        let cases = [
            ExperimentParams { samples: 0, ..ExperimentParams::standard() },
            ExperimentParams { samples: SAMPLES_MAX + 1, ..ExperimentParams::standard() },
            ExperimentParams { base_ns: 0, ..ExperimentParams::standard() },
            ExperimentParams { base_ns: BASE_MAX_NS + 1, ..ExperimentParams::standard() },
            ExperimentParams { jitter_ns: JITTER_MAX_NS + 1, ..ExperimentParams::standard() },
            ExperimentParams { tail_one_in: 1, ..ExperimentParams::standard() },
            ExperimentParams { tail_one_in: TAIL_ONE_IN_MAX + 1, ..ExperimentParams::standard() },
            ExperimentParams { tail_factor: 0, ..ExperimentParams::standard() },
            ExperimentParams { tail_factor: TAIL_FACTOR_MAX + 1, ..ExperimentParams::standard() },
        ];
        let mut i = 0usize;
        while i < cases.len() {
            assert_eq!(cases[i].validate(), Err(ExpCode::ParamBad.val()));
            i += 1;
        }
    }

    #[test]
    fn bucket_mapping_is_monotone() {
        let mut prev = 0usize;
        let mut ns = 1u32;
        while ns < 1_000_000 {
            let b = bucket_of(ns);
            assert!(b >= prev);
            prev = b;
            ns = ns.saturating_mul(2);
        }
        assert_eq!(bucket_of(0), 0);
        assert_eq!(bucket_of(1), 0);
        assert_eq!(bucket_of(2), 1);
        assert_eq!(bucket_of(1_000_000), 19);
    }

    #[test]
    fn percentile_is_monotone() {
        let mut h = Histogram::new();
        let mut v = 1u32;
        while v <= 200 {
            h.record(v).expect("样本在域内");
            v += 1;
        }
        h.audit().expect("直方图自洽");
        let p50 = h.percentile(500).expect("p50");
        let p95 = h.percentile(950).expect("p95");
        let p99 = h.percentile(990).expect("p99");
        assert!(p50 <= p95);
        assert!(p95 <= p99);
        assert!(p50 >= 64 && p50 <= 128);
        assert!(p99 >= 128);
    }

    #[test]
    fn histogram_rejects_zero() {
        let mut h = Histogram::new();
        assert_eq!(h.record(0), Err(ExpCode::SampleBad.val()));
        assert_eq!(h.count(), 0);
    }

    #[test]
    fn histogram_rejects_over_limit() {
        let mut h = Histogram::new();
        assert_eq!(h.record(NS_LIMIT + 1), Err(ExpCode::SampleBad.val()));
        assert_eq!(h.percentile(990), Err(ExpCode::MetricBad.val()));
    }

    #[test]
    fn histogram_digest_changes_with_content() {
        let mut a = Histogram::new();
        let mut b = Histogram::new();
        a.record(100).expect("域内");
        b.record(200).expect("域内");
        assert_ne!(a.digest(), b.digest());
        b.record(100).expect("域内");
        assert_eq!(a.sum() + 200, b.sum());
    }

    #[test]
    fn synth_is_replayable() {
        let p = ExperimentParams::standard();
        let mut r1 = p_core::Lcg::new(0x7101);
        let mut r2 = p_core::Lcg::new(0x7101);
        let mut i = 0u32;
        while i < 64 {
            assert_eq!(synth_ns(&mut r1, &p, 0), synth_ns(&mut r2, &p, 0));
            i += 1;
        }
    }

    #[test]
    fn synth_is_seed_sensitive() {
        let p = ExperimentParams::standard();
        let mut r1 = p_core::Lcg::new(0x7101);
        let mut r2 = p_core::Lcg::new(0x7103);
        let mut differ = false;
        let mut i = 0u32;
        while i < 32 {
            let a = synth_ns(&mut r1, &p, 0);
            let b = synth_ns(&mut r2, &p, 0);
            if a != b {
                differ = true;
            }
            i += 1;
        }
        assert!(differ, "0x7101 与 0x7103 必须给出不同样本流");
    }

    #[test]
    fn experiment_is_complete() {
        let log = std_log();
        assert_eq!(log.len(), LOG_CAP);
        assert_eq!(log.registered_rounds(), ROUNDS);
        assert!(log.complete());
        assert!(log.audit().is_ok());
    }

    #[test]
    fn experiment_records_have_ordered_metrics() {
        let log = std_log();
        let mut i = 0usize;
        while i < log.len() {
            let r = log.records()[i];
            assert!(r.p50 <= r.p95);
            assert!(r.p95 <= r.p99);
            assert!(r.n == ExperimentParams::standard().samples);
            i += 1;
        }
    }

    #[test]
    fn experiment_replays_identically() {
        let a = std_log();
        let b = std_log();
        assert_eq!(a.head(), b.head());
        assert_eq!(a.digest(), b.digest());
        assert_eq!(a.len(), b.len());
    }

    #[test]
    fn experiment_seed_sensitive() {
        let a = run_experiment(0, ExperimentParams::standard(), 0x7101).expect("种子 A");
        let b = run_experiment(0, ExperimentParams::standard(), 0x7103).expect("种子 B");
        assert_ne!(a.head(), b.head());
    }

    #[test]
    fn experiment_rejects_bad_segment() {
        let got = run_experiment(SEGMENTS as u8, ExperimentParams::standard(), EXP_SEED);
        assert_eq!(got.err(), Some(ExpCode::ParamBad.val()));
    }

    #[test]
    fn experiment_rejects_bad_params_at_entry() {
        let p = ExperimentParams { samples: 0, ..ExperimentParams::standard() };
        let got = run_experiment(0, p, EXP_SEED);
        assert_eq!(got.err(), Some(ExpCode::ParamBad.val()));
    }

    #[test]
    fn duplicate_record_is_rejected() {
        let mut log = std_log();
        let rec = *log.find(1, Arm::Control).expect("首轮控制臂在册");
        assert_eq!(log.push(rec), Err(ExpCode::DuplicateBad.val()));
        assert_eq!(log.rejected(), 1);
    }

    #[test]
    fn chain_tamper_is_rejected() {
        let log = std_log();
        let mut fresh = ExperimentLog::new(0, ExperimentParams::standard());
        let rec = *log.find(2, Arm::Control).expect("第二轮在册");
        assert_eq!(fresh.push(rec), Err(ExpCode::ChainBad.val()));
    }

    #[test]
    fn digest_tamper_is_rejected() {
        let log = std_log();
        let mut fresh = ExperimentLog::new(0, ExperimentParams::standard());
        let base = *log.find(1, Arm::Control).expect("首轮在册");
        let mut rec = base;
        rec.p99 = rec.p99.saturating_add(1);
        rec.sum_ns = rec.p99 as u64;
        assert_eq!(fresh.push(rec), Err(ExpCode::DigestBad.val()));
    }

    #[test]
    fn order_regression_is_rejected() {
        let log = std_log();
        let mut fresh = ExperimentLog::new(0, ExperimentParams::standard());
        let first = refit(*log.find(2, Arm::Control).expect("第二轮"), p_core::FNV_OFF);
        fresh.push(first).expect("先落第二轮");
        let head = fresh.head();
        let late = refit(*log.find(1, Arm::Control).expect("首轮"), head);
        assert_eq!(fresh.push(late), Err(ExpCode::OrderBad.val()));
    }

    #[test]
    fn compare_covers_ten_rounds() {
        let rep = compare_all(&std_log()).expect("对照齐备");
        assert_eq!(rep.comparisons, 10);
        assert_eq!(rep.improved + rep.regressed + rep.flat, 10);
        assert!(rep.max_abs_delta < 10_000);
    }

    #[test]
    fn comparison_verdict_band() {
        let log = std_log();
        let c = compare_round(&log, 1).expect("首轮对照");
        let mag = if c.delta_permille < 0 {
            (-c.delta_permille) as u32
        } else {
            c.delta_permille as u32
        };
        if mag <= FLAT_PERMILLE {
            assert_eq!(c.verdict, Verdict::Flat);
        } else if c.delta_permille < 0 {
            assert_eq!(c.verdict, Verdict::Improved);
        } else {
            assert_eq!(c.verdict, Verdict::Regressed);
        }
        assert_eq!(compare_round(&log, 0), Err(ExpCode::RoundBad.val()));
        assert_eq!(compare_round(&log, ROUNDS + 1), Err(ExpCode::RoundBad.val()));
    }

    #[test]
    fn summary_uses_integer_means() {
        let s = summarize(&std_log()).expect("可汇总");
        assert!(s.ctrl_mean_p99 >= 1);
        assert!(s.cand_mean_p99 >= 1);
        assert!(s.best_round >= 1 && s.best_round <= ROUNDS);
        assert!(s.worst_round >= 1 && s.worst_round <= ROUNDS);
        assert!(s.best_delta <= s.worst_delta);
    }

    #[test]
    fn render_is_lowercase_sixteen_hex() {
        let row = render_row(EXPR_TAG, 1, 1, 64, 2_048, row_digest(1, 1, 64, 2_048));
        assert!(row.starts_with("EXPR|round=1|arm=1|n=64|p99=2048|digest="));
        let tail = &row[row.len() - 16..];
        assert_eq!(tail.len(), 16);
        let mut i = 0usize;
        while i < tail.len() {
            let b = tail.as_bytes()[i];
            assert!((b'0'..=b'9').contains(&b) || (b'a'..=b'f').contains(&b));
            i += 1;
        }
    }

    #[test]
    fn parse_u32_accepts_and_rejects() {
        assert_eq!(parse_u32("0"), Ok(0));
        assert_eq!(parse_u32("2048"), Ok(2_048));
        assert_eq!(parse_u32("4294967295"), Ok(u32::MAX));
        assert!(parse_u32("").is_err());
        assert!(parse_u32("12x").is_err());
        assert!(parse_u32("-1").is_err());
        assert!(parse_u32("4294967296").is_err());
        assert!(parse_u32("99999999999").is_err());
    }

    #[test]
    fn parse_hex16_accepts_and_rejects() {
        assert_eq!(parse_hex16("00000000000000ff"), Ok(255));
        assert_eq!(parse_hex16("FFFFFFFFFFFFFFFF"), Ok(u64::MAX));
        assert!(parse_hex16("").is_err());
        assert!(parse_hex16("0x000000000000ff").is_err());
        assert!(parse_hex16("00000000000000fg").is_err());
        assert!(parse_hex16("00000000000000f").is_err());
    }

    #[test]
    fn row_fields_roundtrip() {
        let d = row_digest(7, 0, 32, 4_096);
        let row = render_row(EXPR_TAG, 7, 0, 32, 4_096, d);
        let (tag, vals, got) = row_fields(&row).expect("六字段可切");
        assert_eq!(tag, EXPR_TAG);
        assert_eq!(vals, [7, 0, 32, 4_096]);
        assert_eq!(got, d);
        assert_eq!(audit_render(&row), Ok(d));
    }

    #[test]
    fn row_fields_rejects_key_order() {
        let d = row_digest(7, 0, 32, 4_096);
        let row = format!("EXPR|arm=0|round=7|n=32|p99=4096|digest={:016x}", d);
        assert_eq!(row_fields(&row), Err(ExpCode::RenderBad.val()));
    }

    #[test]
    fn row_fields_rejects_shapes() {
        assert!(row_fields("EXPR|round=1").is_err());
        assert!(row_fields("EXPR|round=1|arm=0|n=4|p99=1").is_err());
        assert!(row_fields("|round=1|arm=0|n=4|p99=1|digest=0000000000000001").is_err());
        assert!(row_fields("EXPR|round=1|arm=0|n=4|p99=1|digest=0000000000000001|x=2").is_err());
        assert!(row_fields("EXPR|round=1|arm=0|n=4|p99=1|digest=000000000000000g").is_err());
    }

    #[test]
    fn audit_render_rejects_tamper() {
        let d = row_digest(5, 1, 64, 3_072);
        let row = render_row(EXPR_TAG, 5, 1, 64, 3_072, d);
        assert!(audit_render(&row).is_ok());
        let t1 = row.replacen("round=5", "round=6", 1);
        assert!(audit_render(&t1).is_err());
        let t2 = row.replacen("arm=1", "arm=2", 1);
        assert!(audit_render(&t2).is_err());
        let t3 = row.replacen("n=64", "n=65", 1);
        assert!(audit_render(&t3).is_err());
        let t4 = row.replacen("p99=3072", "p99=3073", 1);
        assert!(audit_render(&t4).is_err());
        let t5 = row.replacen("digest=", "digest0", 1);
        assert!(audit_render(&t5).is_err());
    }

    #[test]
    fn audit_render_rejects_out_of_range() {
        let d0 = row_digest(0, 0, 64, 1_024);
        let row0 = render_row(EXPR_TAG, 0, 0, 64, 1_024, d0);
        assert_eq!(audit_render(&row0), Err(ExpCode::RoundBad.val()));
        let d1 = row_digest(1, 2, 64, 1_024);
        let row1 = render_row(EXPR_TAG, 1, 2, 64, 1_024, d1);
        assert_eq!(audit_render(&row1), Err(ExpCode::ArmBad.val()));
        let d2 = row_digest(1, 0, 0, 1_024);
        let row2 = render_row(EXPR_TAG, 1, 0, 0, 1_024, d2);
        assert_eq!(audit_render(&row2), Err(ExpCode::SampleBad.val()));
        let d3 = row_digest(1, 0, 64, NS_LIMIT + 1);
        let row3 = render_row(EXPR_TAG, 1, 0, 64, NS_LIMIT + 1, d3);
        assert_eq!(audit_render(&row3), Err(ExpCode::SampleBad.val()));
    }

    #[test]
    fn fuzz_small_survives_all_rounds() {
        let f = fuzz_rounds(200).expect("小规模 fuzz 必须零崩");
        assert_eq!(f.survived, 200);
        assert_eq!(f.chain_checks, 200);
        assert_ne!(f.digest, 0);
    }

    #[test]
    fn fuzz_replays_to_same_digest() {
        let a = fuzz_rounds(300).expect("fuzz A");
        let b = fuzz_rounds(300).expect("fuzz B");
        assert_eq!(a.digest, b.digest);
        assert_eq!(a.param_bits, b.param_bits);
    }

    #[test]
    fn fuzz_rejects_invalid_round_counts() {
        assert_eq!(fuzz_rounds(0), Err(ExpCode::FuzzBad.val()));
        assert_eq!(fuzz_rounds(FUZZ_LIMIT + 1), Err(ExpCode::FuzzBad.val()));
    }

    #[test]
    fn injection_catches_six_families() {
        let log = inject_rounds(3).expect("注入必须全被捕获");
        assert!(log.all_caught());
        assert_eq!(log.tries, [3, 3, 3, 3, 3, 3]);
        assert_eq!(log.total(), 18);
    }

    #[test]
    fn probe_sequence_is_green() {
        assert!(probe_seq_a().is_ok());
        assert!(probe_seq_b().is_ok());
        assert!(probe_seq_c().is_ok());
    }

    #[test]
    fn replay_and_seed_checks_are_green() {
        assert!(replay_check().is_ok());
        assert!(seed_sensitivity().is_ok());
    }

    #[test]
    fn checks_are_all_green() {
        let s = run_unx49071_checks();
        assert!(s.all_passed(), "red={}", s.red_items_text());
    }

    #[test]
    fn checks_have_exactly_eight_items() {
        let s = run_unx49071_checks();
        assert_eq!(s.len(), 8);
        assert_eq!(s.dyn_used(), 8);
        assert!(!s.truncated());
    }

    #[test]
    fn checks_passed_count_is_eight() {
        let s = run_unx49071_checks();
        assert_eq!(s.passed_count(), 8);
        assert_eq!(s.failed_count(), 0);
    }

    #[test]
    fn check_names_are_unique() {
        let s = run_unx49071_checks();
        let mut i = 0usize;
        while i < s.len() {
            let a = s.get(i).expect("检查项在册").name;
            let mut j = i + 1usize;
            while j < s.len() {
                let b = s.get(j).expect("检查项在册").name;
                assert_ne!(a, b);
                j += 1;
            }
            i += 1;
        }
    }

    #[test]
    fn check_details_are_non_empty() {
        let s = run_unx49071_checks();
        let mut i = 0usize;
        while i < s.len() {
            let d = s.detail_for_check(i);
            assert!(!d.is_empty(), "第 {} 条明细为空", i);
            i += 1;
        }
    }

    #[test]
    fn check_digest_is_recomputable() {
        let log = std_log();
        let rec = log.find(4, Arm::Candidate).expect("第四轮候选臂在册");
        let row = record_row(rec);
        let (tag, vals, digest) = row_fields(&row).expect("账行可切");
        assert_eq!(tag, EXPR_TAG);
        assert_eq!(vals.first().copied(), Some(4));
        assert_eq!(vals.get(1).copied(), Some(1));
        assert_eq!(digest, row_digest(4, 1, rec.n, rec.p99));
        assert_eq!(audit_render(&row), Ok(digest));
    }

    #[test]
    fn bad_rows_are_all_rejected() {
        let d = row_digest(9, 1, 128, 8_192);
        let row = render_row(EXPR_TAG, 9, 1, 128, 8_192, d);
        let bads = [
            row.replacen("EXPR", "EXQS", 1),
            row.replacen("round=9", "round=09", 1),
            row.replacen("n=128", "n=1280", 1),
            row.replacen("p99=8192", "p99=8191", 1),
            row.replacen("digest=", "digest=0", 1),
        ];
        let mut i = 0usize;
        while i < bads.len() {
            assert!(audit_render(&bads[i]).is_err(), "第 {} 条坏行未被拒", i);
            i += 1;
        }
    }
}
