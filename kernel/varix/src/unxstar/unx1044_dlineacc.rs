//! unx1044_dlineacc —— UNX-F1044 TSC-deadline 精度账（deadline 模式误差分解）。
//! 锚点（CoRun Varix STAR II · Unxreal 主册 UNX-A2-B13 第 4 条）：
//!   UNX-F1044 | TSC-deadline 精度账（deadline 模式误差分解） | 300 | 已深化 |
//!   UNX-F0953→F1044-J1 精度达标（deadline 全链 p99 ≤10µs——F1014 预算表 LAPIC 行兑现），
//!   与分频模式对比账（精度提升量化），分解和≈全链（<5%）
//! 定位原文（主册 B13 批注）：AI-02 承办｜嫁接源：Intel SDM Vol.3 §10.5.4（LAPIC
//!   定时器/TSC-deadline）与 Linux clocksource 仲裁语义，只跟随｜上游：F1026 候选面、
//!   F1002 特性账、F0836 定时器双计数器｜防重：对 F0035 纲领与 F0828/F0836（LAPIC
//!   定时器寄存器面）分层——寄存器面在 B02，本批做模式语义、仲裁与切换。
//! 判据三铁面：
//!   J1-a 精度达标：deadline 全链 p99 ≤ 10µs（10000ns 整数硬线——F1014 预算表
//!        LAPIC 定时器 deadline 行兑现；分频档对照线 ≤50µs，F1053 分行口径）；
//!        达标/失守二态判定，失守时定位到最大误差段。
//!   J1-b 对比账：deadline 模式 vs TCR 分频模式同负载精度对比，提升量化 =
//!        两者全链 p99 差值（ns）与相对提升 ppm，对比账双列呈现。
//!   J1-c 分解对账：五段分解和 ≈ 全链实测，逐轮比值以 ppm 记账，越界（≥5%
//!        = 50000ppm）立案；正常档差 <5% 判据兑现。
//! 五段分解（全整数 ns 账，每段 600 环 + 段级 P95/P99）：
//!   S0 写 MSR 延迟（wrmsr TSC_DEADLINE MSR 0x6B0 落针→退役）
//!   S1 投递延迟（deadline 到点→LAPIC 发出定时器中断向量）
//!   S2 中断退出延迟（向量投递→iretq 返回被打断上下文前的退出路径）
//!   S3 处理入口延迟（iretq 前后进入定时器处理函数首条指令）
//!   S4 重装延迟（处理函数内再武装：换算+写 MSR 完成）
//! 职责定位：F1043（TSC-deadline 模式语义）之上的精度记账层——不碰寄存器面
//!   （F0828/F0836 在 B02），只做误差分解、对账、达标判定与模式对比。
//! 硬件行为全部为确定性模型注入（LCG 种子驱动），逻辑 TSC 单调推进，零墙钟、
//!   零浮点、全 u64 饱和算术；同种子同序列恒同账（双跑指纹对拍为证）。
//! 诊断码段：0x1410~0x141F（全域 grep 确认独占：0x1414 仅作为 PCI 厂商 ID
//!   字符串与分页常量出现，非诊断码；0x1420 段已被 ves442_s24flow 占用故不取）。
//! 纪律：no_std + alloc；内核路径零 unwrap/expect/panic!/运行期 assert!（编译期
//!   const assert 可）；可失败全走 Result<T, DlineAccError>；溢出全 saturating；
//!   >64KB 结构体不上栈（全部采样环走 Vec 堆分配）；const fn 无 slice::get/
//!   Vec 索引/Ord::min/for 循环（while+索引）；零 crate:: 依赖，完全自包含。
//! 性能逐项分解：
//!   record O(SEG_COUNT)=O(5)（五段推环 + 一次对账除法）；
//!   percentile O(N log N)（排序快照，N=600 环封顶，重排摊销）；
//!   run_rounds O(n)；指纹 O(账目字节数)（<1KB）；对拍 O(2n)。
//! ---------------------------------------------------------------------------

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 常量（来源写死：Intel SDM Vol.3 §10.5.4 / F1014 预算表 / F1053 分行口径）
// ---------------------------------------------------------------------------

/// 五段分解段数：写 MSR / 投递 / 中断退出 / 处理入口 / 重装。
pub const SEG_COUNT: usize = 5;
/// 段索引 S0：写 MSR 延迟。
pub const SEG_WRITE_MSR: usize = 0;
/// 段索引 S1：投递延迟。
pub const SEG_DELIVERY: usize = 1;
/// 段索引 S2：中断退出延迟。
pub const SEG_IRQ_EXIT: usize = 2;
/// 段索引 S3：处理入口延迟。
pub const SEG_ENTRY: usize = 3;
/// 段索引 S4：重装延迟。
pub const SEG_REARM: usize = 4;

/// 采样环容量：600 环（判据点名"600 环+段级 P95/P99"，F1044 判据原文）。
pub const RING_CAP: usize = 600;
/// deadline 全链 p99 预算：10µs = 10000ns（F1014 预算表 LAPIC deadline 行）。
pub const CHAIN_BUDGET_NS: u64 = 10_000;
/// 分频模式全链 p99 预算：50µs = 50000ns（F1053 分频档分行口径）。
pub const DIVIDER_BUDGET_NS: u64 = 50_000;
/// 分解对账容差：<5%，以 ppm 记账即 <50000ppm（判据"分解和≈全链（<5%）"）。
pub const RECONCILE_TOL_PPM: u64 = 50_000;
/// ppm 换算基数：1_000_000（百万分比）。
pub const PPM_BASE: u64 = 1_000_000;

/// 模型注入档：S0 写 MSR 基线 40ns（QEMU/KVM wrmsr 量级档，F1043 注入细分）。
pub const WRITE_MSR_BASE_NS: u64 = 40;
/// 模型注入档：S0 写 MSR 抖动幅 0..25ns。
pub const WRITE_MSR_JITTER_NS: u64 = 25;
/// 模型注入档：S1 投递基线 120ns、抖动 0..80ns。
pub const DELIVERY_BASE_NS: u64 = 120;
pub const DELIVERY_JITTER_NS: u64 = 80;
/// 模型注入档：S2 中断退出基线 200ns、抖动 0..150ns。
pub const IRQ_EXIT_BASE_NS: u64 = 200;
pub const IRQ_EXIT_JITTER_NS: u64 = 150;
/// 模型注入档：S3 处理入口基线 150ns、抖动 0..120ns。
pub const ENTRY_BASE_NS: u64 = 150;
pub const ENTRY_JITTER_NS: u64 = 120;
/// 模型注入档：S4 重装基线 60ns、抖动 0..60ns。
pub const REARM_BASE_NS: u64 = 60;
pub const REARM_JITTER_NS: u64 = 60;
/// 模型注入档：跨段杂项噪声占全链百分比 0..2%（对账差源，<5% 门内）。
pub const CROSS_NOISE_PCT_MAX: u64 = 2;
/// 模型注入档：分频模式量化误差幅 0..40000ns（分频周期尾差，F1053 分频档）。
pub const DIVIDER_QUANT_NS: u64 = 40_000;
/// 段级超差天花板（检测带宽，正常注入档上界再加裕量；单位 ns）：
/// S0 ≤200 / S1 ≤1000 / S2 ≤2000 / S3 ≤2000 / S4 ≤1000。
pub const SEGMENT_CEIL_NS: [u64; SEG_COUNT] = [200, 1_000, 2_000, 2_000, 1_000];
/// 超差注入幅下界：20000ns（=预算 2 倍起，必越全链 10µs 线）。
pub const OUTLIER_MIN_NS: u64 = 20_000;
/// 超差注入幅宽：0..40000ns。
pub const OUTLIER_SPAN_NS: u64 = 40_000;
/// 立案簿容量：64 条（越界案例封顶，防账目无界）。
pub const CASE_CAP: usize = 64;
/// 事件环容量：128 条（事件账封顶，满则丢旧入账）。
pub const EVENT_CAP: usize = 128;
/// 逻辑 TSC 步进基线：50000 tick（≈50µs 一轮，QEMU TSC 1GHz 近似档）。
pub const TSC_STRIDE_NS: u64 = 50_000;
/// 逻辑 TSC 步进抖动幅：0..30000 tick。
pub const TSC_STRIDE_JITTER: u64 = 30_000;
/// 直方图 24 桶 × 100ns/桶（覆盖 0..2400ns，全链正常档全落桶内）。
pub const HIST_BINS: usize = 24;
pub const HIST_BIN_NS: u64 = 100;
/// 检测命中簿容量：256 条（注入面账目封顶）。
pub const HIT_CAP: usize = 256;

/// LCG 常量（Numerical Recipes Knuth MMIX 64 位档；确定性注入源）。
const LCG_A: u64 = 6_364_136_223_846_793_005;
const LCG_C: u64 = 1_442_695_040_888_963_407;
/// LCG 种子混入金枢（避免全零种子退化为常序列）。
const LCG_GOLDEN: u64 = 0x9E37_79B9_7F4A_7C15;
/// FNV-1a 64 位偏移基（标准常量 0xcbf29ce484222325）。
pub const FNV_OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
/// FNV-1a 64 位素数（标准常量 0x100000001b3 = 2^40 + 2^8 + 179）。
pub const FNV_PRIME: u64 = 0x0000_0100_0000_01b3;
/// 指纹域分隔标签（对拍账防串域）。
pub const FINGERPRINT_TAG: u64 = 0x1410_4404_DEAD_1ACC;

// 编译期不变量（允许的 const assert；来源随常量注释）。
const _: () = assert!(SEG_COUNT == 5, "五段分解段数铁定 5");
const _: () = assert!(RING_CAP == 600, "判据点名 600 环");
const _: () = assert!(CHAIN_BUDGET_NS == 10_000, "F1014 LAPIC deadline 行 10µs");
const _: () = assert!(DIVIDER_BUDGET_NS == 50_000, "F1053 分频档 50µs");
const _: () = assert!(RECONCILE_TOL_PPM == 50_000, "对账门 <5% = <50000ppm");
const _: () = assert!(RECONCILE_TOL_PPM < PPM_BASE, "ppm 门必须小于基数");
const _: () = assert!(HIST_BINS > 0 && HIST_BIN_NS > 0, "直方图参数非零");
const _: () = assert!(OUTLIER_MIN_NS > CHAIN_BUDGET_NS, "超差注入必越全链预算");

// ---------------------------------------------------------------------------
// 诊断码段 0x1410~0x141F（闭集；全域独占，先入库者占段）
// ---------------------------------------------------------------------------

/// 0x1410 采样序倒退：TSC 时间戳非严格单调，样本拒收入账。
pub const C_TSC_RETROGRADE: u16 = 0x1410;
/// 0x1411 全链 p99 超预算：> 10µs（F1014 LAPIC 行红线）。
pub const C_CHAIN_P99_MISS: u16 = 0x1411;
/// 0x1412 分解对账失衡：|Σ段−全链|/全链 ≥ 5%，立案。
pub const C_RECON_BREACH: u16 = 0x1412;
/// 0x1413 段级超差：单段样本越出段级天花板带宽。
pub const C_SEG_OUTLIER: u16 = 0x1413;
/// 0x1414 段样本丢失：五段账出现缺段（结构完整门）。
pub const C_SEG_LOSS: u16 = 0x1414;
/// 0x1415 全链零值：full_ns=0 非法样本，拒收。
pub const C_ZERO_CHAIN: u16 = 0x1415;
/// 0x1416 空环取分位：环内零样本请求 P95/P99。
pub const C_EMPTY_RING: u16 = 0x1416;
/// 0x1417 分频档 p99 超其预算：> 50µs（F1053 分频档红线）。
pub const C_DIVIDER_MISS: u16 = 0x1417;
/// 0x1418 事件环溢出：事件账满，丢旧入账计数。
pub const C_EVENT_OVERFLOW: u16 = 0x1418;
/// 0x1419 立案簿满：越界案例超出 64 条封顶，仅计数不落簿。
pub const C_CASE_CAP_FULL: u16 = 0x1419;
/// 0x141A 注入未检出：注入面发现一笔注入未被任何探测器命中（红账）。
pub const C_INJECT_UNDETECTED: u16 = 0x141A;
/// 0x141B 指纹不一致：双跑确定性对拍失败。
pub const C_FINGERPRINT_MISMATCH: u16 = 0x141B;
/// 0x141C 分位参数非法：percentile 入参 >100。
pub const C_BAD_PERCENTILE: u16 = 0x141C;
/// 0x141D 段索引越界：访问不存在的分解段。
pub const C_BAD_SEG_INDEX: u16 = 0x141D;
/// 0x141E 样本不足：判据要求的轮次未跑满。
pub const C_ROUNDS_SHORT: u16 = 0x141E;
/// 0x141F 误报非零：清洁样本被探测器误命中（注入面红账）。
pub const C_FALSE_POSITIVE: u16 = 0x141F;

// ---------------------------------------------------------------------------
// 错误枚举（每变体 detail() 中文可操作诊断；错误码映射 0x141x 段）
// ---------------------------------------------------------------------------

/// DlineAcc 可失败路径统一错误。
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum DlineAccError {
    /// 全链样本为零值（full_ns=0），无法对账。
    ZeroChainSample,
    /// 空环取分位：tag 指明是哪条环（段名或 chain/divider/ppm）。
    EmptyRing { tag: &'static str },
    /// 分位参数非法（>100）。
    BadPercentile(u64),
    /// 段索引越界（>=SEG_COUNT）。
    BadSegmentIndex(usize),
    /// 样本不足：判据要求至少 have_needed 轮，实有 have_now 轮。
    RoundsShort { have_needed: u64, have_now: u64 },
    /// 双跑指纹不一致（确定性破约）。
    FingerprintMismatch { first: u64, second: u64 },
    /// 分频档 p99 超其预算（对比账红账）。
    DividerP99Miss { p99_ns: u64 },
}

impl DlineAccError {
    /// 中文可操作诊断（'static str，零分配）。
    pub fn detail(&self) -> &'static str {
        match self {
            DlineAccError::ZeroChainSample => {
                "全链样本 full_ns=0 非法：检查模型注入链是否漏算跨段噪声项，full_ns 必须为正"
            }
            DlineAccError::EmptyRing { .. } => {
                "采样环为空：先调用 run_rounds 跑满至少 1 轮再取分位；判据档需 600 环"
            }
            DlineAccError::BadPercentile(_) => {
                "分位参数非法：percentile 只接受 0..=100，检查调用点是否把 0.99 误传成 99.9"
            }
            DlineAccError::BadSegmentIndex(_) => {
                "段索引越界：合法段为 0..5（写MSR/投递/退出/入口/重装），检查段常量使用处"
            }
            DlineAccError::RoundsShort { .. } => {
                "样本不足：判据档需 600 环，增大 run_rounds 轮数后重取分位与判定"
            }
            DlineAccError::FingerprintMismatch { .. } => {
                "双跑指纹不一致：同种子同序列必须同账，检查是否有墙钟/未初始化内存混入账目"
            }
            DlineAccError::DividerP99Miss { .. } => {
                "分频档 p99 超 50µs 预算：核对 DIVIDER_QUANT_NS 模型档与 F1053 分频档口径"
            }
        }
    }

    /// 诊断码（0x1410~0x141F 段）。
    pub fn code(&self) -> u16 {
        match self {
            DlineAccError::ZeroChainSample => C_ZERO_CHAIN,
            DlineAccError::EmptyRing { .. } => C_EMPTY_RING,
            DlineAccError::BadPercentile(_) => C_BAD_PERCENTILE,
            DlineAccError::BadSegmentIndex(_) => C_BAD_SEG_INDEX,
            DlineAccError::RoundsShort { .. } => C_ROUNDS_SHORT,
            DlineAccError::FingerprintMismatch { .. } => C_FINGERPRINT_MISMATCH,
            DlineAccError::DividerP99Miss { .. } => C_DIVIDER_MISS,
        }
    }
}

// ---------------------------------------------------------------------------
// 确定性随机源（LCG；零墙钟、同种子恒同序列）
// ---------------------------------------------------------------------------

/// 64 位 LCG（Knuth MMIX 常量档）；new 为 const fn（仅异或，无禁坑操作）。
pub struct Lcg {
    state: u64,
}

impl Lcg {
    /// 构造：种子异或金枢防退化。const fn 合规（仅整型异或）。
    pub const fn new(seed: u64) -> Self {
        Lcg {
            state: seed ^ LCG_GOLDEN,
        }
    }

    /// 推进一拍并输出原始 64 位。
    pub fn next_u64(&mut self) -> u64 {
        self.state = self.state.wrapping_mul(LCG_A).wrapping_add(LCG_C);
        self.state
    }

    /// 均匀取 [0, bound)：右移 16 位截头部降低位偏差；bound=0 恒返 0。
    pub fn next_range(&mut self, bound: u64) -> u64 {
        if bound == 0 {
            return 0;
        }
        (self.next_u64() >> 16) % bound
    }
}

// ---------------------------------------------------------------------------
// FNV-1a 指纹与规范化字节流
// ---------------------------------------------------------------------------

/// FNV-1a 64 位指纹（逐字节 xor-mul，while 循环）。
pub fn fnv64(data: &[u8], seed: u64) -> u64 {
    let mut h = seed;
    let mut i = 0usize;
    while i < data.len() {
        h ^= data[i] as u64;
        h = h.wrapping_mul(FNV_PRIME);
        i += 1;
    }
    h
}

/// 追加一个 u64（小端 8 字节）到规范化字节流。
fn push_u64_le(buf: &mut Vec<u8>, v: u64) {
    let mut i = 0u64;
    while i < 8 {
        buf.push(((v >> (i * 8)) & 0xFF) as u8);
        i += 1;
    }
}

/// 追加一个 u16（小端 2 字节）。
fn push_u16_le(buf: &mut Vec<u8>, v: u16) {
    buf.push((v & 0xFF) as u8);
    buf.push((v >> 8) as u8);
}

/// 追加字符串字节（长度前置，防拼接歧义）。
fn push_str_tag(buf: &mut Vec<u8>, s: &str) {
    push_u64_le(buf, s.len() as u64);
    let bytes = s.as_bytes();
    let mut i = 0usize;
    while i < bytes.len() {
        buf.push(bytes[i]);
        i += 1;
    }
}

// ---------------------------------------------------------------------------
// 采样环（600 封顶 FIFO；分位/均值/极值全整数）
// ---------------------------------------------------------------------------

/// 定容采样环：满后覆写最旧样本；账目计数不丢。
pub struct SampleRing {
    cap: usize,
    buf: Vec<u64>,
    head: usize,
    pushed: u64,
    overwritten: u64,
}

impl SampleRing {
    /// 构造（cap 固定 600 由调用方传入；堆分配，不上栈）。
    pub fn new(cap: usize) -> Self {
        SampleRing {
            cap,
            buf: Vec::new(),
            head: 0,
            pushed: 0,
            overwritten: 0,
        }
    }

    /// 推入一个样本；满则覆写最旧并计溢出。
    pub fn push(&mut self, v: u64) {
        if self.buf.len() < self.cap {
            self.buf.push(v);
        } else {
            self.buf[self.head] = v;
            self.head = (self.head + 1) % self.cap;
            self.overwritten = self.overwritten.wrapping_add(1);
        }
        self.pushed = self.pushed.wrapping_add(1);
    }

    /// 环内现存量。
    pub fn count(&self) -> usize {
        self.buf.len()
    }

    /// 累计推入数（含覆写）。
    pub fn total_pushed(&self) -> u64 {
        self.pushed
    }

    /// 覆写数（环满后丢旧次数）。
    pub fn total_overwritten(&self) -> u64 {
        self.overwritten
    }

    /// 时序快照（最旧在前）；分配新 Vec，调用方持有。
    pub fn snapshot(&self) -> Vec<u64> {
        if self.buf.len() < self.cap {
            let mut out = Vec::with_capacity(self.buf.len());
            let mut i = 0usize;
            while i < self.buf.len() {
                out.push(self.buf[i]);
                i += 1;
            }
            out
        } else {
            let mut out = Vec::with_capacity(self.cap);
            let mut i = 0usize;
            while i < self.cap {
                out.push(self.buf[(self.head + i) % self.cap]);
                i += 1;
            }
            out
        }
    }

    /// 排序快照（升序；分位底座）。O(N log N)，N≤600 摊销。
    pub fn snapshot_sorted(&self) -> Vec<u64> {
        let mut v = self.snapshot();
        v.sort_unstable();
        v
    }

    /// 饱和求和。
    pub fn sum_saturating(&self) -> u64 {
        let mut acc = 0u64;
        let mut i = 0usize;
        while i < self.buf.len() {
            acc = acc.saturating_add(self.buf[i]);
            i += 1;
        }
        acc
    }

    /// 极大值；空环返 0（取分位请走 percentile 的 Result 路径）。
    pub fn max_value(&self) -> u64 {
        let mut m = 0u64;
        let mut i = 0usize;
        while i < self.buf.len() {
            if self.buf[i] > m {
                m = self.buf[i];
            }
            i += 1;
        }
        m
    }

    /// 均值（饱和和/存量；空环返 0）。
    pub fn mean(&self) -> u64 {
        let n = self.buf.len();
        if n == 0 {
            return 0;
        }
        self.sum_saturating() / n as u64
    }

    /// 整数分位：升序序位 (count*pct)/100，钳到末位；空环/非法参走 Err。
    pub fn percentile(&self, pct: u64) -> Result<u64, DlineAccError> {
        if pct > 100 {
            return Err(DlineAccError::BadPercentile(pct));
        }
        let sorted = self.snapshot_sorted();
        let n = sorted.len();
        if n == 0 {
            return Err(DlineAccError::EmptyRing { tag: "ring" });
        }
        let mut idx = (n as u64).saturating_mul(pct) / 100;
        if idx >= n as u64 {
            idx = (n as u64).saturating_sub(1);
        }
        Ok(sorted[idx as usize])
    }
}

// ---------------------------------------------------------------------------
// 段名、段统计与直方图
// ---------------------------------------------------------------------------

/// 五段名（顺序与 SEG_* 索引铁定对齐）。
pub const SEGMENT_NAMES: [&'static str; SEG_COUNT] =
    ["写MSR", "投递", "中断退出", "处理入口", "重装"];

/// 段级统计（一次算齐：均值/极值/P95/P99，全整数 ns）。
pub struct SegmentStats {
    /// 段索引 0..5。
    pub idx: usize,
    /// 段名。
    pub name: &'static str,
    /// 环内存量（≤600）。
    pub count: usize,
    /// 均值 ns。
    pub mean_ns: u64,
    /// 极大 ns。
    pub max_ns: u64,
    /// P95 ns。
    pub p95_ns: u64,
    /// P99 ns。
    pub p99_ns: u64,
}

impl SegmentStats {
    /// 从一条段环算统计；空环走 Err（调用方保证已跑轮）。
    pub fn from_ring(idx: usize, ring: &SampleRing) -> Result<SegmentStats, DlineAccError> {
        if idx >= SEG_COUNT {
            return Err(DlineAccError::BadSegmentIndex(idx));
        }
        if ring.count() == 0 {
            return Err(DlineAccError::EmptyRing {
                tag: SEGMENT_NAMES[idx],
            });
        }
        Ok(SegmentStats {
            idx,
            name: SEGMENT_NAMES[idx],
            count: ring.count(),
            mean_ns: ring.mean(),
            max_ns: ring.max_value(),
            p95_ns: ring.percentile(95)?,
            p99_ns: ring.percentile(99)?,
        })
    }

    /// 单行人话化（对账表用）。
    pub fn line(&self) -> String {
        format!(
            "S{} {} 环{} 均值{}ns 极大{}ns P95={}ns P99={}ns",
            self.idx, self.name, self.count, self.mean_ns, self.max_ns, self.p95_ns, self.p99_ns
        )
    }
}

/// 线性直方图：24 桶 × 100ns，溢出桶单列（覆盖率=桶内/总量）。
pub struct SegmentHistogram {
    bins: [u64; HIST_BINS],
    overflow: u64,
    total: u64,
}

impl SegmentHistogram {
    /// 空直方图。
    pub const fn new() -> Self {
        SegmentHistogram {
            bins: [0; HIST_BINS],
            overflow: 0,
            total: 0,
        }
    }

    /// 从样本序列构建。
    pub fn from_samples(samples: &[u64]) -> SegmentHistogram {
        let mut h = SegmentHistogram::new();
        let mut i = 0usize;
        while i < samples.len() {
            h.push(samples[i]);
            i += 1;
        }
        h
    }

    /// 推入一个样本（饱和计数）。
    pub fn push(&mut self, v: u64) {
        let idx = v / HIST_BIN_NS;
        if idx >= HIST_BINS as u64 {
            self.overflow = self.overflow.saturating_add(1);
        } else {
            self.bins[idx as usize] = self.bins[idx as usize].saturating_add(1);
        }
        self.total = self.total.saturating_add(1);
    }

    /// 众数桶（并列取最低桶；空图返 0）。
    pub fn mode_bin(&self) -> usize {
        let mut best = 0usize;
        let mut i = 0usize;
        while i < HIST_BINS {
            if self.bins[i] > self.bins[best] {
                best = i;
            }
            i += 1;
        }
        best
    }

    /// 桶内覆盖率（percent，总量为 0 时返 0）。
    pub fn coverage_pct(&self) -> u64 {
        if self.total == 0 {
            return 0;
        }
        let in_bins = self.total.saturating_sub(self.overflow);
        in_bins.saturating_mul(100) / self.total
    }

    /// 溢出计数。
    pub fn overflow_count(&self) -> u64 {
        self.overflow
    }

    /// 总计数。
    pub fn total_count(&self) -> u64 {
        self.total
    }
}

// ---------------------------------------------------------------------------
// 全链样本与缺失掩码
// ---------------------------------------------------------------------------

/// 缺失掩码位：S0 写 MSR 丢失。
pub const MASK_WRITE: u8 = 1 << SEG_WRITE_MSR;
/// 缺失掩码位：S1 投递丢失。
pub const MASK_DELIVERY: u8 = 1 << SEG_DELIVERY;
/// 缺失掩码位：S2 退出丢失。
pub const MASK_IRQ_EXIT: u8 = 1 << SEG_IRQ_EXIT;
/// 缺失掩码位：S3 入口丢失。
pub const MASK_ENTRY: u8 = 1 << SEG_ENTRY;
/// 缺失掩码位：S4 重装丢失。
pub const MASK_REARM: u8 = 1 << SEG_REARM;

/// 一轮全链样本：逻辑 TSC 时间戳 + 五段 ns 账 + 全链实测 + 缺失掩码。
#[derive(Clone, Copy)]
pub struct ChainSample {
    /// 逻辑 TSC（tick；模型档 1 tick ≈ 1ns，QEMU TSC 1GHz 近似档）。
    pub tsc: u64,
    /// 五段误差账（ns；丢失段对应位掩码置 1，其值置 0）。
    pub seg: [u64; SEG_COUNT],
    /// 全链实测 ns（含跨段噪声；丢失段仍计入实测——硬件发生了，只是没采到）。
    pub full_ns: u64,
    /// 缺失掩码（bit i = 段 i 丢失；正常样本恒 0）。
    pub missing_mask: u8,
}

impl ChainSample {
    /// 已采段饱和求和（丢失段不计）。
    pub fn sum_known(&self) -> u64 {
        let mut acc = 0u64;
        let mut i = 0usize;
        while i < SEG_COUNT {
            if self.missing_mask & (1 << i) == 0 {
                acc = acc.saturating_add(self.seg[i]);
            }
            i += 1;
        }
        acc
    }

    /// 结构完整（五段齐备）。
    pub fn is_complete(&self) -> bool {
        self.missing_mask == 0
    }

    /// 对账差 ppm：|Σ已知段−全链|*1e6/全链（全链为 0 返回饱和上限）。
    pub fn reconcile_ppm(&self) -> u64 {
        if self.full_ns == 0 {
            return u64::MAX;
        }
        let diff = self.sum_known().abs_diff(self.full_ns);
        diff.saturating_mul(PPM_BASE) / self.full_ns
    }
}

// ---------------------------------------------------------------------------
// 模型注入源（确定性硬件行为档；全部来源注释见常量区）
// ---------------------------------------------------------------------------

/// 确定性模型源：五段/全链/分频/注入样本全由 LCG 驱动。
pub struct ModelGen {
    lcg: Lcg,
}

impl ModelGen {
    /// 构造（const fn：仅包一层 Lcg::new）。
    pub const fn new(seed: u64) -> Self {
        ModelGen {
            lcg: Lcg::new(seed),
        }
    }

    /// 生成一轮正常全链样本（TSC 由调用方单调推进注入）。
    pub fn next_chain(&mut self, tsc: u64) -> ChainSample {
        let s0 = WRITE_MSR_BASE_NS + self.lcg.next_range(WRITE_MSR_JITTER_NS + 1);
        let s1 = DELIVERY_BASE_NS + self.lcg.next_range(DELIVERY_JITTER_NS + 1);
        let s2 = IRQ_EXIT_BASE_NS + self.lcg.next_range(IRQ_EXIT_JITTER_NS + 1);
        let s3 = ENTRY_BASE_NS + self.lcg.next_range(ENTRY_JITTER_NS + 1);
        let s4 = REARM_BASE_NS + self.lcg.next_range(REARM_JITTER_NS + 1);
        let sum = s0.saturating_add(s1).saturating_add(s2).saturating_add(s3).saturating_add(s4);
        // 跨段噪声：占全链 0..2%（锁存对账差在 <5% 门内；噪声本身是
        // 五段账之外的上下文保存/恢复等杂项，判据"分解和≈全链"的差源）。
        let noise_pct = self.lcg.next_range(CROSS_NOISE_PCT_MAX + 1);
        let full = sum.saturating_add(sum.saturating_mul(noise_pct) / 100);
        ChainSample {
            tsc,
            seg: [s0, s1, s2, s3, s4],
            full_ns: full,
            missing_mask: 0,
        }
    }

    /// 分频模式同负载全链实测：deadline 链 + 分频量化尾差（0..40000ns）。
    pub fn next_divider_full(&mut self, deadline_chain_ns: u64) -> u64 {
        let quant = self.lcg.next_range(DIVIDER_QUANT_NS + 1);
        deadline_chain_ns.saturating_add(quant)
    }

    /// 注入超差样本：第 seg 段注入 20000..60000ns 的越差值（全链随之爆线）。
    pub fn inject_outlier(&mut self, tsc: u64, seg: usize) -> ChainSample {
        let mut s = self.next_chain(tsc);
        if seg < SEG_COUNT {
            s.seg[seg] = OUTLIER_MIN_NS + self.lcg.next_range(OUTLIER_SPAN_NS + 1);
            // 全链实测重算：硬件真的花了这么久，全链同步爆线。
            s.full_ns = s.sum_known().saturating_add(s.sum_known() / 100);
        }
        s
    }

    /// 注入段丢失：随机丢一段（实测全链保留原值，账面缺段）。
    pub fn inject_loss(&mut self, tsc: u64) -> ChainSample {
        let mut s = self.next_chain(tsc);
        let lost = self.lcg.next_range(SEG_COUNT as u64) as usize;
        s.seg[lost] = 0;
        s.missing_mask |= 1 << lost;
        s
    }

    /// 注入对账漂移：全链实测多记 10..19% 的跨段开销（五段账外的
    /// 上下文切换尖刺等），Σ段 与全链差越 5% 门——立案簿/越界路径的唯一
    /// 自然触发面（正常档噪声 0..2% 恒在门内）。
    pub fn inject_recon_drift(&mut self, tsc: u64) -> ChainSample {
        let mut s = self.next_chain(tsc);
        let sum = s.sum_known();
        let drift_pct = 10 + self.lcg.next_range(10);
        s.full_ns = sum.saturating_add(sum.saturating_mul(drift_pct) / 100);
        s
    }
}

// ---------------------------------------------------------------------------
// 事件账（定容环；满则丢旧入账）
// ---------------------------------------------------------------------------

/// 事件账条目：诊断码 + 中文备注。
pub struct EventRing {
    buf: Vec<(u16, String)>,
    cap: usize,
    overwritten: u64,
}

impl EventRing {
    /// 构造（EVENT_CAP 封顶）。
    pub fn new(cap: usize) -> Self {
        EventRing {
            buf: Vec::new(),
            cap,
            overwritten: 0,
        }
    }

    /// 推入事件；满则丢最旧并计溢出（溢出本身即 0x1418 账目事实）。
    pub fn push(&mut self, code: u16, note: String) {
        if self.buf.len() >= self.cap {
            self.buf.remove(0);
            self.overwritten = self.overwritten.wrapping_add(1);
        }
        self.buf.push((code, note));
    }

    /// 现存事件数。
    pub fn count(&self) -> usize {
        self.buf.len()
    }

    /// 累计丢旧数。
    pub fn total_overwritten(&self) -> u64 {
        self.overwritten
    }

    /// 按序号取事件（越界返 None）。
    pub fn get(&self, i: usize) -> Option<(u16, &str)> {
        if i < self.buf.len() {
            Some((self.buf[i].0, self.buf[i].1.as_str()))
        } else {
            None
        }
    }

    /// 全事件人话化逐行（含码段十六进制）。
    pub fn render(&self) -> Vec<String> {
        let mut out = Vec::new();
        let mut i = 0usize;
        while i < self.buf.len() {
            out.push(format!("0x{:04X} {}", self.buf[i].0, self.buf[i].1));
            i += 1;
        }
        out
    }
}

// ---------------------------------------------------------------------------
// 立案簿（对账越界案例）
// ---------------------------------------------------------------------------

/// 一桩对账越界立案（|Σ段−全链|/全链 ≥ 5%）。
pub struct ReconcileCase {
    /// 案发逻辑 TSC。
    pub tsc: u64,
    /// 越界比值（ppm，≥50000）。
    pub ppm: u64,
    /// Σ已知段 ns。
    pub sum_ns: u64,
    /// 全链实测 ns。
    pub full_ns: u64,
    /// 案发轮最大段索引（定位首个嫌疑段）。
    pub max_seg_idx: usize,
}

/// 立案簿渲染单行。
impl ReconcileCase {
    pub fn line(&self) -> String {
        format!(
            "立案@tsc={} ppm={} Σ段={}ns 全链={}ns 首嫌段=S{} {}",
            self.tsc,
            self.ppm,
            self.sum_ns,
            self.full_ns,
            self.max_seg_idx,
            SEGMENT_NAMES[self.max_seg_idx]
        )
    }
}

// ---------------------------------------------------------------------------
// 误差五段分解器（判据 J1-c 核心）
// ---------------------------------------------------------------------------

/// 记录结果：接受/拒收 + 对账账目。
pub struct RecordOutcome {
    /// 是否入环。
    pub accepted: bool,
    /// 拒收/接受因由。
    pub reason: AcceptReason,
    /// 全链实测 ns（拒收轮仍回带，供调用方查）。
    pub full_ns: u64,
    /// 本轮对账 ppm（拒收轮为 0）。
    pub ppm: u64,
}

/// 记录因由。
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum AcceptReason {
    /// 正常入环。
    Ok,
    /// TSC 倒退/持平（严格单调校验不过），拒收。
    Retrograde,
    /// 段缺失（结构完整门不过），拒收。
    Incomplete,
}

impl AcceptReason {
    /// 人话化。
    pub fn as_str(&self) -> &'static str {
        match self {
            AcceptReason::Ok => "入环",
            AcceptReason::Retrograde => "拒收：采样序倒退",
            AcceptReason::Incomplete => "拒收：段缺失",
        }
    }
}

/// 五段误差分解器：五条 600 段环 + 全链环 + 对账 ppm 环 + 立案簿 + 事件账。
pub struct ErrorDecomposer {
    seg_rings: [SampleRing; SEG_COUNT],
    chain_ring: SampleRing,
    ppm_ring: SampleRing,
    last_tsc: u64,
    have_last_tsc: bool,
    rounds_recorded: u64,
    rounds_rejected_retro: u64,
    rounds_rejected_loss: u64,
    reconcile_rounds: u64,
    reconcile_max_ppm: u64,
    reconcile_breaches: u64,
    cases: Vec<ReconcileCase>,
    cases_overflow: u64,
    events: EventRing,
}

impl ErrorDecomposer {
    /// 构造：六条环全部堆分配（600×8B×6=28.8KB 不上栈）。
    pub fn new() -> Self {
        ErrorDecomposer {
            seg_rings: [
                SampleRing::new(RING_CAP),
                SampleRing::new(RING_CAP),
                SampleRing::new(RING_CAP),
                SampleRing::new(RING_CAP),
                SampleRing::new(RING_CAP),
            ],
            chain_ring: SampleRing::new(RING_CAP),
            ppm_ring: SampleRing::new(RING_CAP),
            last_tsc: 0,
            have_last_tsc: false,
            rounds_recorded: 0,
            rounds_rejected_retro: 0,
            rounds_rejected_loss: 0,
            reconcile_rounds: 0,
            reconcile_max_ppm: 0,
            reconcile_breaches: 0,
            cases: Vec::new(),
            cases_overflow: 0,
            events: EventRing::new(EVENT_CAP),
        }
    }

    /// 记录一轮样本：严格单调校验 → 结构完整门 → 入环 → 对账（ppm+越界立案）。
    pub fn record(&mut self, s: &ChainSample) -> Result<RecordOutcome, DlineAccError> {
        // 结构完整门：缺段样本不入账（硬件全链实测仍发生了，故只拒账面）。
        if !s.is_complete() {
            self.rounds_rejected_loss = self.rounds_rejected_loss.wrapping_add(1);
            self.events.push(
                C_SEG_LOSS,
                format!("tsc={} 缺段掩码=0x{:02X}", s.tsc, s.missing_mask),
            );
            return Ok(RecordOutcome {
                accepted: false,
                reason: AcceptReason::Incomplete,
                full_ns: s.full_ns,
                ppm: 0,
            });
        }
        // 全链零值门：full_ns=0 无法对账，直接 Err。
        if s.full_ns == 0 {
            self.events
                .push(C_ZERO_CHAIN, format!("tsc={} 全链零值拒收", s.tsc));
            return Err(DlineAccError::ZeroChainSample);
        }
        // 严格单调校验：tsc 必须严格大于上一轮（持平也算倒退）。
        if self.have_last_tsc && s.tsc <= self.last_tsc {
            self.rounds_rejected_retro = self.rounds_rejected_retro.wrapping_add(1);
            self.events.push(
                C_TSC_RETROGRADE,
                format!(
                    "tsc={} ≤ 上轮 tsc={}，样本拒收",
                    s.tsc, self.last_tsc
                ),
            );
            return Ok(RecordOutcome {
                accepted: false,
                reason: AcceptReason::Retrograde,
                full_ns: s.full_ns,
                ppm: 0,
            });
        }
        // 入环：五段 + 全链。
        let mut i = 0usize;
        while i < SEG_COUNT {
            self.seg_rings[i].push(s.seg[i]);
            i += 1;
        }
        self.chain_ring.push(s.full_ns);
        self.last_tsc = s.tsc;
        self.have_last_tsc = true;
        self.rounds_recorded = self.rounds_recorded.wrapping_add(1);
        // 对账：|Σ段−全链| 以 ppm 记账；越界（≥5%）立案。
        let sum = s.sum_known();
        let diff = sum.abs_diff(s.full_ns);
        let ppm = diff.saturating_mul(PPM_BASE) / s.full_ns;
        self.ppm_ring.push(ppm);
        self.reconcile_rounds = self.reconcile_rounds.wrapping_add(1);
        if ppm > self.reconcile_max_ppm {
            self.reconcile_max_ppm = ppm;
        }
        if ppm >= RECONCILE_TOL_PPM {
            self.reconcile_breaches = self.reconcile_breaches.wrapping_add(1);
            // 首嫌段：本轮最大段值（粗定位，精定位由 verdict 段级 P99 承担）。
            let mut max_seg = 0usize;
            let mut k = 1usize;
            while k < SEG_COUNT {
                if s.seg[k] > s.seg[max_seg] {
                    max_seg = k;
                }
                k += 1;
            }
            if self.cases.len() < CASE_CAP {
                self.cases.push(ReconcileCase {
                    tsc: s.tsc,
                    ppm,
                    sum_ns: sum,
                    full_ns: s.full_ns,
                    max_seg_idx: max_seg,
                });
            } else {
                self.cases_overflow = self.cases_overflow.wrapping_add(1);
                self.events.push(
                    C_CASE_CAP_FULL,
                    format!("tsc={} 立案簿满，本案仅计数 ppm={}", s.tsc, ppm),
                );
            }
            self.events.push(
                C_RECON_BREACH,
                format!(
                    "tsc={} Σ段={}ns 全链={}ns ppm={} ≥ 门 {}",
                    s.tsc, sum, s.full_ns, ppm, RECONCILE_TOL_PPM
                ),
            );
        }
        Ok(RecordOutcome {
            accepted: true,
            reason: AcceptReason::Ok,
            full_ns: s.full_ns,
            ppm,
        })
    }

    /// 全链 p99（空环走 Err）。
    pub fn chain_p99(&self) -> Result<u64, DlineAccError> {
        if self.chain_ring.count() == 0 {
            return Err(DlineAccError::EmptyRing { tag: "chain" });
        }
        self.chain_ring.percentile(99)
    }

    /// 全链环时序快照（直方图底座；空环走 Err）。
    pub fn chain_snapshot(&self) -> Result<Vec<u64>, DlineAccError> {
        if self.chain_ring.count() == 0 {
            return Err(DlineAccError::EmptyRing { tag: "chain" });
        }
        Ok(self.chain_ring.snapshot())
    }

    /// 全链 P95。
    pub fn chain_p95(&self) -> Result<u64, DlineAccError> {
        if self.chain_ring.count() == 0 {
            return Err(DlineAccError::EmptyRing { tag: "chain" });
        }
        self.chain_ring.percentile(95)
    }

    /// 段级统计表（五段一次算齐）。
    pub fn seg_stats(&self) -> Result<Vec<SegmentStats>, DlineAccError> {
        let mut out = Vec::with_capacity(SEG_COUNT);
        let mut i = 0usize;
        while i < SEG_COUNT {
            out.push(SegmentStats::from_ring(i, &self.seg_rings[i])?);
            i += 1;
        }
        Ok(out)
    }

    /// 段级 P99 最大段（失守定位用）。
    pub fn max_seg_by_p99(&self) -> Result<(usize, u64), DlineAccError> {
        let stats = self.seg_stats()?;
        let mut best = 0usize;
        let mut best_p99 = 0u64;
        let mut i = 0usize;
        while i < stats.len() {
            if stats[i].p99_ns > best_p99 {
                best_p99 = stats[i].p99_ns;
                best = i;
            }
            i += 1;
        }
        Ok((best, best_p99))
    }

    /// 对账最大 ppm。
    pub fn reconcile_max_ppm(&self) -> u64 {
        self.reconcile_max_ppm
    }

    /// 对账越界立案数（含仅计数的溢出案）。
    pub fn reconcile_breaches(&self) -> u64 {
        self.reconcile_breaches
    }

    /// 立案簿现存案数。
    pub fn case_count(&self) -> usize {
        self.cases.len()
    }

    /// 按序号取立案（越界返 None）。
    pub fn case_at(&self, i: usize) -> Option<&ReconcileCase> {
        if i < self.cases.len() {
            Some(&self.cases[i])
        } else {
            None
        }
    }

    /// 已入账轮数。
    pub fn rounds_recorded(&self) -> u64 {
        self.rounds_recorded
    }

    /// 倒退拒收轮数。
    pub fn retro_rejects(&self) -> u64 {
        self.rounds_rejected_retro
    }

    /// 缺段拒收轮数。
    pub fn loss_rejects(&self) -> u64 {
        self.rounds_rejected_loss
    }

    /// 事件账（渲染用）。
    pub fn events(&self) -> &EventRing {
        &self.events
    }

    /// 对账 ppm 环统计行。
    pub fn reconcile_line(&self) -> String {
        format!(
            "对账轮{} 最大ppm={} 门{}ppm(<5%) 越界立案{}（簿内{} 溢出{}）",
            self.reconcile_rounds,
            self.reconcile_max_ppm,
            RECONCILE_TOL_PPM,
            self.reconcile_breaches,
            self.cases.len(),
            self.cases_overflow
        )
    }

    /// 规范化字节流（指纹底座；双跑确定性逐字节可复算）。
    pub fn push_canonical(&self, buf: &mut Vec<u8>) {
        push_u64_le(buf, FINGERPRINT_TAG);
        push_u64_le(buf, self.rounds_recorded);
        push_u64_le(buf, self.rounds_rejected_retro);
        push_u64_le(buf, self.rounds_rejected_loss);
        push_u64_le(buf, self.reconcile_rounds);
        push_u64_le(buf, self.reconcile_max_ppm);
        push_u64_le(buf, self.reconcile_breaches);
        push_u64_le(buf, self.chain_ring.sum_saturating());
        push_u64_le(buf, self.ppm_ring.sum_saturating());
        let mut i = 0usize;
        while i < SEG_COUNT {
            push_u64_le(buf, self.seg_rings[i].sum_saturating());
            i += 1;
        }
        push_u64_le(buf, self.cases.len() as u64);
        push_u64_le(buf, self.cases_overflow);
        push_u64_le(buf, self.events.count() as u64);
        push_u64_le(buf, self.events.total_overwritten());
        let mut e = 0usize;
        while e < self.events.count() {
            if let Some((code, note)) = self.events.get(e) {
                push_u16_le(buf, code);
                push_str_tag(buf, note);
            }
            e += 1;
        }
    }

    /// 指纹（FNV-1a over 规范化账目）。
    pub fn fingerprint(&self) -> u64 {
        let mut buf = Vec::new();
        self.push_canonical(&mut buf);
        fnv64(&buf, FNV_OFFSET)
    }
}

// ---------------------------------------------------------------------------
// 精度达标判定（判据 J1-a：p99 ≤10µs 硬线 + 失守定位）
// ---------------------------------------------------------------------------

/// 达标判定书（二态 + 失守定位 + 对账门状态）。
pub struct AccuracyVerdict {
    /// 二态：true=达标（全链 p99 ≤ 10µs）。
    pub pass: bool,
    /// 全链 P99 ns。
    pub full_p99_ns: u64,
    /// 预算硬线 ns（10000）。
    pub budget_ns: u64,
    /// 余量 ns（预算−P99，饱和；失守时为 0）。
    pub headroom_ns: u64,
    /// 是否失守（与 pass 互补，便于匹配诊断码）。
    pub failing: bool,
    /// 最大误差段索引。
    pub max_seg_idx: usize,
    /// 最大误差段名。
    pub max_seg_name: &'static str,
    /// 最大段 P99 ns。
    pub max_seg_p99_ns: u64,
    /// 对账最大 ppm。
    pub reconcile_max_ppm: u64,
    /// 对账门内（<5%）。
    pub reconcile_within: bool,
    /// 入账轮数。
    pub rounds: u64,
    /// 判定书人话化备注。
    pub note: String,
}

impl AccuracyVerdict {
    /// 从分解器判定（J1-a 硬线 + 失守定位到最大段）。
    pub fn assess(d: &ErrorDecomposer) -> Result<AccuracyVerdict, DlineAccError> {
        if d.rounds_recorded() < 1 {
            return Err(DlineAccError::RoundsShort {
                have_needed: 1,
                have_now: d.rounds_recorded(),
            });
        }
        let p99 = d.chain_p99()?;
        let pass = p99 <= CHAIN_BUDGET_NS;
        let (max_idx, max_p99) = d.max_seg_by_p99()?;
        let max_ppm = d.reconcile_max_ppm();
        let within = max_ppm < RECONCILE_TOL_PPM;
        let note = if pass {
            format!(
                "达标：全链 P99={}ns ≤ 预算{}ns（F1014 LAPIC 行兑现），余量{}ns；最大段=S{} {}({}ns)",
                p99, CHAIN_BUDGET_NS, CHAIN_BUDGET_NS.saturating_sub(p99), max_idx,
                SEGMENT_NAMES[max_idx], max_p99
            )
        } else {
            format!(
                "失守（0x{:04X}）：全链 P99={}ns > 预算{}ns，定位最大段=S{} {}({}ns)——先查该段模型档与注入抖动",
                C_CHAIN_P99_MISS, p99, CHAIN_BUDGET_NS, max_idx, SEGMENT_NAMES[max_idx], max_p99
            )
        };
        Ok(AccuracyVerdict {
            pass,
            full_p99_ns: p99,
            budget_ns: CHAIN_BUDGET_NS,
            headroom_ns: CHAIN_BUDGET_NS.saturating_sub(p99),
            failing: !pass,
            max_seg_idx: max_idx,
            max_seg_name: SEGMENT_NAMES[max_idx],
            max_seg_p99_ns: max_p99,
            reconcile_max_ppm: max_ppm,
            reconcile_within: within,
            rounds: d.rounds_recorded(),
            note,
        })
    }
}

// ---------------------------------------------------------------------------
// 分频模式对比账（判据 J1-b：提升量化，双列呈现）
// ---------------------------------------------------------------------------

/// 模式对比账（deadline vs 分频，同负载）。
pub struct ModeComparison {
    /// deadline 模式全链 P99 ns。
    pub deadline_p99_ns: u64,
    /// 分频模式全链 P99 ns。
    pub divider_p99_ns: u64,
    /// 提升量化 = 分频 P99 − deadline P99（ns，饱和）。
    pub improvement_ns: u64,
    /// 相对提升 ppm（提升量/分频 P99）。
    pub improvement_ppm: u64,
    /// deadline 档达标（≤10µs）。
    pub deadline_within_budget: bool,
    /// 分频档达标（≤50µs，F1053 分行口径）。
    pub divider_within_budget: bool,
    /// 双列对比表（人话化）。
    pub columns: String,
}

impl ModeComparison {
    /// 构建对比账（两档 P99 由调用方算好传入；分频 P99=0 非法走 Err）。
    pub fn build(deadline_p99: u64, divider_p99: u64) -> Result<ModeComparison, DlineAccError> {
        if divider_p99 == 0 {
            return Err(DlineAccError::EmptyRing { tag: "divider" });
        }
        let improvement = divider_p99.saturating_sub(deadline_p99);
        let improvement_ppm = improvement.saturating_mul(PPM_BASE) / divider_p99;
        let dl_ok = deadline_p99 <= CHAIN_BUDGET_NS;
        let dv_ok = divider_p99 <= DIVIDER_BUDGET_NS;
        let columns = format!(
            "| 指标 | deadline 档 | 分频档 |\n\
             |---|---|---|\n\
             | 全链 P99 | {} ns | {} ns |\n\
             | 预算硬线 | {} ns | {} ns |\n\
             | 达标 | {} | {} |\n\
             | 提升量化 | 分频→deadline：{} ns（{}ppm） | —— |",
            deadline_p99,
            divider_p99,
            CHAIN_BUDGET_NS,
            DIVIDER_BUDGET_NS,
            if dl_ok { "达标" } else { "失守" },
            if dv_ok { "达标" } else { "失守" },
            improvement,
            improvement_ppm
        );
        Ok(ModeComparison {
            deadline_p99_ns: deadline_p99,
            divider_p99_ns: divider_p99,
            improvement_ns: improvement,
            improvement_ppm,
            deadline_within_budget: dl_ok,
            divider_within_budget: dv_ok,
            columns,
        })
    }
}

// ---------------------------------------------------------------------------
// 健康账（F0899 口径对齐体例）
// ---------------------------------------------------------------------------

/// 健康三态。
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum HealthStatus {
    /// 良好（分 ≥90）。
    Good,
    /// 降级（60..89）。
    Degrade,
    /// 异常（<60）。
    Bad,
}

impl HealthStatus {
    /// 人话化。
    pub fn as_str(&self) -> &'static str {
        match self {
            HealthStatus::Good => "良好",
            HealthStatus::Degrade => "降级",
            HealthStatus::Bad => "异常",
        }
    }
}

/// 健康报告：状态 + 评分（0..100）+ 分项行。
pub struct HealthReport {
    /// 三态。
    pub status: HealthStatus,
    /// 评分。
    pub score: u8,
    /// 分项行（每行一条评分依据）。
    pub lines: Vec<String>,
}

// ---------------------------------------------------------------------------
// 顶层账本（UNX-F1044 门面）
// ---------------------------------------------------------------------------

/// 一批轮次的运行摘要。
pub struct RoundSummary {
    /// 请求轮数。
    pub requested: u64,
    /// 入账轮数。
    pub accepted: u64,
    /// 倒退拒收轮数。
    pub retro_rejected: u64,
    /// 缺段拒收轮数。
    pub loss_rejected: u64,
    /// 本批对账最大 ppm。
    pub batch_max_ppm: u64,
}

/// UNX-F1044 精度账账本：分解器 + 分频对照环 + 模型源，一体门面。
pub struct DlineAccLedger {
    decomp: ErrorDecomposer,
    divider_ring: SampleRing,
    gen: ModelGen,
    tsc_cursor: u64,
}

impl DlineAccLedger {
    /// 构造（种子驱动；零墙钟）。
    pub fn new(seed: u64) -> Self {
        DlineAccLedger {
            decomp: ErrorDecomposer::new(),
            divider_ring: SampleRing::new(RING_CAP),
            gen: ModelGen::new(seed),
            tsc_cursor: seed.wrapping_add(0x1410_4404),
        }
    }

    /// 跑 n 轮：逻辑 TSC 单调推进 → 正常链入分解器 → 分频同负载入对照环。
    pub fn run_rounds(&mut self, n: u64) -> Result<RoundSummary, DlineAccError> {
        let mut accepted = 0u64;
        let mut retro = 0u64;
        let mut loss = 0u64;
        let mut batch_max_ppm = 0u64;
        let mut i = 0u64;
        while i < n {
            // 逻辑 TSC 步进：50000..80000 tick，严格递增（模型档注释见常量区）。
            self.tsc_cursor = self
                .tsc_cursor
                .saturating_add(TSC_STRIDE_NS + self.gen.lcg_raw_range(TSC_STRIDE_JITTER + 1));
            let chain = self.gen.next_chain(self.tsc_cursor);
            let out = self.decomp.record(&chain)?;
            if out.accepted {
                accepted = accepted.wrapping_add(1);
                if out.ppm > batch_max_ppm {
                    batch_max_ppm = out.ppm;
                }
            } else if out.reason == AcceptReason::Retrograde {
                retro = retro.wrapping_add(1);
            } else {
                loss = loss.wrapping_add(1);
            }
            // 分频模式同负载：同一轮负载下分频档全链实测（deadline 链 + 量化尾差）。
            let divider_full = self.gen.next_divider_full(chain.full_ns);
            self.divider_ring.push(divider_full);
            i += 1;
        }
        Ok(RoundSummary {
            requested: n,
            accepted,
            retro_rejected: retro,
            loss_rejected: loss,
            batch_max_ppm,
        })
    }

    /// 精度达标判定（J1-a）。
    pub fn verdict(&self) -> Result<AccuracyVerdict, DlineAccError> {
        AccuracyVerdict::assess(&self.decomp)
    }

    /// 分频对照 P99（J1-b 左列底座；空环走 Err）。
    pub fn divider_p99(&self) -> Result<u64, DlineAccError> {
        if self.divider_ring.count() == 0 {
            return Err(DlineAccError::EmptyRing { tag: "divider" });
        }
        self.divider_ring.percentile(99)
    }

    /// 分频对照 P99 + 达标门（F1053 分频档 ≤50µs）。
    pub fn divider_check(&self) -> Result<(u64, bool), DlineAccError> {
        let p99 = self.divider_p99()?;
        let ok = p99 <= DIVIDER_BUDGET_NS;
        Ok((p99, ok))
    }

    /// 模式对比账（J1-b）。
    pub fn comparison(&self) -> Result<ModeComparison, DlineAccError> {
        let dl = self.decomp.chain_p99()?;
        let dv = self.divider_p99()?;
        let cmp = ModeComparison::build(dl, dv)?;
        if !cmp.divider_within_budget {
            return Err(DlineAccError::DividerP99Miss { p99_ns: dv });
        }
        Ok(cmp)
    }

    /// 段级统计表。
    pub fn segment_table(&self) -> Result<Vec<SegmentStats>, DlineAccError> {
        self.decomp.seg_stats()
    }

    /// 五段直方图摘要（每段众数桶 + 桶内覆盖率）。
    pub fn histogram_lines(&self) -> Result<Vec<String>, DlineAccError> {
        let mut out = Vec::with_capacity(SEG_COUNT + 1);
        let mut i = 0usize;
        while i < SEG_COUNT {
            let snap = self.decomp_seg_snapshot(i)?;
            let h = SegmentHistogram::from_samples(&snap);
            out.push(format!(
                "S{} {} 众数桶={}（{}..{}ns） 桶内覆盖{}% 溢出{}笔",
                i,
                SEGMENT_NAMES[i],
                h.mode_bin(),
                h.mode_bin() as u64 * HIST_BIN_NS,
                (h.mode_bin() as u64 + 1) * HIST_BIN_NS,
                h.coverage_pct(),
                h.overflow_count()
            ));
            i += 1;
        }
        let chain_snap = self.decomp.chain_snapshot()?;
        let hc = SegmentHistogram::from_samples(&chain_snap);
        out.push(format!(
            "全链 众数桶={}（{}..{}ns） 桶内覆盖{}% 溢出{}笔",
            hc.mode_bin(),
            hc.mode_bin() as u64 * HIST_BIN_NS,
            (hc.mode_bin() as u64 + 1) * HIST_BIN_NS,
            hc.coverage_pct(),
            hc.overflow_count()
        ));
        Ok(out)
    }

    /// 健康账（F0899 口径）：评分五项——p99 达标 40 / 对账门内 25 / 有账 10 /
    /// 零立案 15 / 事件环零溢出 10。
    pub fn health(&self) -> Result<HealthReport, DlineAccError> {
        let mut score: u64 = 0;
        let mut lines: Vec<String> = Vec::new();
        let v = self.verdict()?;
        if v.pass {
            score += 40;
            lines.push(format!("+40 p99 达标：{}ns ≤ {}ns", v.full_p99_ns, CHAIN_BUDGET_NS));
        } else {
            lines.push(format!("+0 p99 失守：{}ns > {}ns", v.full_p99_ns, CHAIN_BUDGET_NS));
        }
        if v.reconcile_within {
            score += 25;
            lines.push(format!("+25 对账门内：最大 {}ppm < {}ppm", v.reconcile_max_ppm, RECONCILE_TOL_PPM));
        } else {
            lines.push(format!("+0 对账越界：最大 {}ppm ≥ {}ppm", v.reconcile_max_ppm, RECONCILE_TOL_PPM));
        }
        if self.decomp.rounds_recorded() > 0 {
            score += 10;
            lines.push(format!("+10 有账：{} 轮入环", self.decomp.rounds_recorded()));
        } else {
            lines.push(String::from("+0 无账：未跑任何轮次"));
        }
        if self.decomp.case_count() == 0 {
            score += 15;
            lines.push(String::from("+15 零立案：对账越界案 0"));
        } else {
            lines.push(format!("+0 有立案：{} 桩对账越界", self.decomp.case_count()));
        }
        if self.decomp.events().total_overwritten() == 0 {
            score += 10;
            lines.push(String::from("+10 事件环零溢出"));
        } else {
            lines.push(format!("+0 事件环溢出 {} 笔", self.decomp.events().total_overwritten()));
        }
        let status = if score >= 90 {
            HealthStatus::Good
        } else if score >= 60 {
            HealthStatus::Degrade
        } else {
            HealthStatus::Bad
        };
        lines.push(format!(
            "总分{} 状态:{}（p99={}ns 对账ppm={} 倒退拒收{} 缺段拒收{}）",
            score,
            status.as_str(),
            v.full_p99_ns,
            v.reconcile_max_ppm,
            self.decomp.retro_rejects(),
            self.decomp.loss_rejects()
        ));
        Ok(HealthReport {
            status,
            score: score as u8,
            lines,
        })
    }

    /// 全量快照（人话化多段报告）。
    pub fn snapshot(&self) -> Result<String, DlineAccError> {
        let v = self.verdict()?;
        let cmp = self.comparison()?;
        let mut s = String::new();
        s.push_str("== UNX-F1044 TSC-deadline 精度账 ==\n");
        // 判定书（J1-a）。
        s.push_str("[判定]\n");
        s.push_str(&v.note);
        s.push('\n');
        // 五段表。
        s.push_str("[五段分解（600 环）]\n");
        let stats = self.segment_table()?;
        let mut i = 0usize;
        while i < stats.len() {
            s.push_str(&stats[i].line());
            s.push('\n');
            i += 1;
        }
        // 全链 P95/P99。
        s.push_str("[全链]\n");
        s.push_str(&format!(
            "P95={}ns P99={}ns 预算={}ns 余量={}ns\n",
            self.decomp.chain_p95()?,
            v.full_p99_ns,
            CHAIN_BUDGET_NS,
            v.headroom_ns
        ));
        // 对账（J1-c）。
        s.push_str("[分解对账（Σ段 vs 全链，<5% 门）]\n");
        s.push_str(&self.decomp.reconcile_line());
        s.push('\n');
        // 对比账（J1-b）双列。
        s.push_str("[模式对比账（deadline vs 分频）]\n");
        s.push_str(&cmp.columns);
        s.push('\n');
        // 直方图。
        s.push_str("[直方图]\n");
        let hlines = self.histogram_lines()?;
        let mut h = 0usize;
        while h < hlines.len() {
            s.push_str(&hlines[h]);
            s.push('\n');
            h += 1;
        }
        // 事件账。
        s.push_str("[事件账]\n");
        let evs = self.decomp.events().render();
        if evs.is_empty() {
            s.push_str("（零事件——全部轮次干净入环）\n");
        } else {
            let mut e = 0usize;
            while e < evs.len() {
                s.push_str(&evs[e]);
                s.push('\n');
                e += 1;
            }
        }
        // 指纹。
        s.push_str("[指纹]\n");
        s.push_str(&format!("FNV-1a64 = 0x{:016X}\n", self.fingerprint()));
        Ok(s)
    }

    /// 指纹（分解器账 + 分频环账合成）。
    pub fn fingerprint(&self) -> u64 {
        let mut buf = Vec::new();
        self.decomp.push_canonical(&mut buf);
        push_u64_le(&mut buf, self.divider_ring.sum_saturating());
        push_u64_le(&mut buf, self.divider_ring.total_pushed());
        push_u64_le(&mut buf, self.divider_ring.max_value());
        push_u64_le(&mut buf, self.tsc_cursor);
        fnv64(&buf, FNV_OFFSET)
    }

    /// 取段 i 的时序快照（内部转发，索引越界走 Err）。
    fn decomp_seg_snapshot(&self, i: usize) -> Result<Vec<u64>, DlineAccError> {
        if i >= SEG_COUNT {
            return Err(DlineAccError::BadSegmentIndex(i));
        }
        Ok(self.decomp.seg_rings[i].snapshot())
    }

    /// 分解器只读访问（对账/事件/立案审计用）。
    pub fn decomposer(&self) -> &ErrorDecomposer {
        &self.decomp
    }
}

impl ModelGen {
    /// 受控随机出口（单一随机源纪律：账本 TSC 步进抖动与注入抖动全部出自
    /// 同一 LCG，保证双跑确定性逐拍可复算）。
    pub fn lcg_raw_range(&mut self, bound: u64) -> u64 {
        self.lcg.next_range(bound)
    }
}

// ---------------------------------------------------------------------------
// 注入面（超差序列 / 段丢失——检测率统计 + 误报清零）
// ---------------------------------------------------------------------------

/// 探测命中类别。
#[derive(Clone, Copy, PartialEq)]
pub enum DetectKind {
    /// 段级超差（越段级天花板）。
    SegmentOutlier,
    /// 全链预算爆线（>10µs）。
    ChainBudget,
    /// 段丢失（结构完整门）。
    SegmentLoss,
    /// 对账越界（ppm ≥5% 门）。
    ReconcileBreach,
}

impl DetectKind {
    /// 人话化。
    pub fn as_str(&self) -> &'static str {
        match self {
            DetectKind::SegmentOutlier => "段级超差",
            DetectKind::ChainBudget => "全链爆线",
            DetectKind::SegmentLoss => "段丢失",
            DetectKind::ReconcileBreach => "对账越界",
        }
    }
}

/// 一笔探测命中。
#[derive(Clone, Copy)]
pub struct DetectorHit {
    /// 案发逻辑 TSC。
    pub tsc: u64,
    /// 命中类别。
    pub kind: DetectKind,
    /// 涉事段（全链类命中为 SEG_COUNT 哨兵值）。
    pub seg_idx: usize,
    /// 涉事值 ns。
    pub value_ns: u64,
}

/// 探测命中渲染。
impl DetectorHit {
    pub fn line(&self) -> String {
        let seg_name = if self.seg_idx < SEG_COUNT {
            SEGMENT_NAMES[self.seg_idx]
        } else {
            "全链"
        };
        format!(
            "命中@tsc={} {} 段=S{}({}) 值={}ns",
            self.tsc,
            self.kind.as_str(),
            self.seg_idx,
            seg_name,
            self.value_ns
        )
    }
}

/// 检测率统计（注入/检出/漏检/误报/检出率 percent）。
pub struct DetectionStats {
    /// 注入笔数。
    pub injected: u64,
    /// 检出笔数（至少一命中即计检出）。
    pub detected: u64,
    /// 漏检笔数。
    pub missed: u64,
    /// 误报笔数（清洁样本被命中）。
    pub false_positives: u64,
    /// 检出率（percent，注入 0 笔时恒 100——无注入无从漏检）。
    pub rate_pct: u64,
}

impl DetectionStats {
    /// 由计数合成（检出率饱和防除零）。
    pub fn compose(injected: u64, detected: u64, false_positives: u64) -> DetectionStats {
        let missed = injected.saturating_sub(detected);
        let rate = if injected == 0 {
            100
        } else {
            detected.saturating_mul(100) / injected
        };
        DetectionStats {
            injected,
            detected,
            missed,
            false_positives,
            rate_pct: rate,
        }
    }

    /// 人话化单行。
    pub fn line(&self) -> String {
        format!(
            "注入{} 检出{} 漏检{} 误报{} 检出率{}%",
            self.injected, self.detected, self.missed, self.false_positives, self.rate_pct
        )
    }
}

/// 注入面台架：超差序列/段丢失双注入 + 清洁误报自查，全账封顶。
pub struct FaultInjectionBench {
    gen: ModelGen,
    tsc_cursor: u64,
    hits: Vec<DetectorHit>,
    hits_overflow: u64,
    out_injected: u64,
    out_detected: u64,
    loss_injected: u64,
    loss_detected: u64,
    clean_checked: u64,
    clean_false_positives: u64,
}

impl FaultInjectionBench {
    /// 构造（种子独立于主账本，注入面账目与主账隔离）。
    pub fn new(seed: u64) -> Self {
        FaultInjectionBench {
            gen: ModelGen::new(seed),
            tsc_cursor: seed.wrapping_add(0x1410_BEEF),
            hits: Vec::new(),
            hits_overflow: 0,
            out_injected: 0,
            out_detected: 0,
            loss_injected: 0,
            loss_detected: 0,
            clean_checked: 0,
            clean_false_positives: 0,
        }
    }

    /// 核心探测器：对一轮样本跑全部四类门（段级超差/全链爆线/段丢失/对账越界）。
    pub fn detect(&mut self, s: &ChainSample) -> Vec<DetectorHit> {
        let mut hits = Vec::new();
        // 门 1：段级超差（逐段越天花板）。
        let mut i = 0usize;
        while i < SEG_COUNT {
            let lost = s.missing_mask & (1 << i) != 0;
            if !lost && s.seg[i] > SEGMENT_CEIL_NS[i] {
                hits.push(DetectorHit {
                    tsc: s.tsc,
                    kind: DetectKind::SegmentOutlier,
                    seg_idx: i,
                    value_ns: s.seg[i],
                });
            }
            i += 1;
        }
        // 门 2：全链预算爆线。
        if s.full_ns > CHAIN_BUDGET_NS {
            hits.push(DetectorHit {
                tsc: s.tsc,
                kind: DetectKind::ChainBudget,
                seg_idx: SEG_COUNT,
                value_ns: s.full_ns,
            });
        }
        // 门 3：段丢失（结构完整门；硬件事件发生但账面缺段）。
        if !s.is_complete() {
            let mut i = 0usize;
            while i < SEG_COUNT {
                if s.missing_mask & (1 << i) != 0 {
                    hits.push(DetectorHit {
                        tsc: s.tsc,
                        kind: DetectKind::SegmentLoss,
                        seg_idx: i,
                        value_ns: 0,
                    });
                }
                i += 1;
            }
        }
        // 门 4：对账越界（缺段导致 Σ已知段 与全链失衡越 5% 门）。
        if s.full_ns > 0 && s.reconcile_ppm() >= RECONCILE_TOL_PPM {
            hits.push(DetectorHit {
                tsc: s.tsc,
                kind: DetectKind::ReconcileBreach,
                seg_idx: SEG_COUNT,
                value_ns: s.reconcile_ppm(),
            });
        }
        // 命中簿封顶入账。
        let mut h = 0usize;
        while h < hits.len() {
            if self.hits.len() < HIT_CAP {
                self.hits.push(hits[h]);
            } else {
                self.hits_overflow = self.hits_overflow.wrapping_add(1);
            }
            h += 1;
        }
        hits
    }

    /// 注入 n 笔超差序列（随机段），返回该批检测率。
    pub fn run_out_of_tolerance(&mut self, n: u64) -> DetectionStats {
        let mut detected = 0u64;
        let mut i = 0u64;
        while i < n {
            self.tsc_cursor = self
                .tsc_cursor
                .saturating_add(TSC_STRIDE_NS + self.gen.lcg_raw_range(TSC_STRIDE_JITTER + 1));
            let seg = (self.gen.lcg_raw_range(SEG_COUNT as u64)) as usize;
            let s = self.gen.inject_outlier(self.tsc_cursor, seg);
            self.out_injected = self.out_injected.wrapping_add(1);
            let hits = self.detect(&s);
            if !hits.is_empty() {
                detected = detected.wrapping_add(1);
            }
            i += 1;
        }
        self.out_detected = self.out_detected.wrapping_add(detected);
        DetectionStats::compose(n, detected, 0)
    }

    /// 注入 n 笔段丢失，返回该批检测率。
    pub fn run_segment_loss(&mut self, n: u64) -> DetectionStats {
        let mut detected = 0u64;
        let mut i = 0u64;
        while i < n {
            self.tsc_cursor = self
                .tsc_cursor
                .saturating_add(TSC_STRIDE_NS + self.gen.lcg_raw_range(TSC_STRIDE_JITTER + 1));
            let s = self.gen.inject_loss(self.tsc_cursor);
            self.loss_injected = self.loss_injected.wrapping_add(1);
            let hits = self.detect(&s);
            if !hits.is_empty() {
                detected = detected.wrapping_add(1);
            }
            i += 1;
        }
        self.loss_detected = self.loss_detected.wrapping_add(detected);
        DetectionStats::compose(n, detected, 0)
    }

    /// 清洁误报自查：n 笔干净样本过全部四门，命中即误报（判据面误报须 0）。
    pub fn run_clean_false_positive_check(&mut self, n: u64) -> u64 {
        let mut fps = 0u64;
        let mut i = 0u64;
        while i < n {
            self.tsc_cursor = self
                .tsc_cursor
                .saturating_add(TSC_STRIDE_NS + self.gen.lcg_raw_range(TSC_STRIDE_JITTER + 1));
            let s = self.gen.next_chain(self.tsc_cursor);
            self.clean_checked = self.clean_checked.wrapping_add(1);
            let hits = self.detect(&s);
            if !hits.is_empty() {
                fps = fps.wrapping_add(1);
            }
            i += 1;
        }
        self.clean_false_positives = self.clean_false_positives.wrapping_add(fps);
        fps
    }

    /// 累计检出率统计（三类各一）。
    pub fn cumulative(&self) -> (DetectionStats, DetectionStats, DetectionStats) {
        (
            DetectionStats::compose(self.out_injected, self.out_detected, 0),
            DetectionStats::compose(self.loss_injected, self.loss_detected, 0),
            DetectionStats::compose(self.clean_checked, 0, self.clean_false_positives),
        )
    }

    /// 注入面健康账：漏检与误报双红门（任一非零即异常）。
    pub fn health_line(&self) -> String {
        let (o, l, c) = self.cumulative();
        let mut verdict = String::from("注入面干净");
        if o.missed > 0 || l.missed > 0 {
            verdict = format!(
                "异常：漏检 超差{} 段丢失{}（0x{:04X}）",
                o.missed, l.missed, C_INJECT_UNDETECTED
            );
        } else if c.false_positives > 0 {
            verdict = format!("异常：清洁误报 {} 笔（0x{:04X}）", c.false_positives, C_FALSE_POSITIVE);
        }
        format!(
            "超差[{}] 段丢失[{}] 清洁自查[已查{} 误报{}] —— {}",
            o.line(),
            l.line(),
            c.injected,
            c.false_positives,
            verdict
        )
    }

    /// 命中簿渲染（封顶内的账目）。
    pub fn hits_render(&self, max_lines: usize) -> Vec<String> {
        let mut out = Vec::new();
        let mut i = 0usize;
        while i < self.hits.len() && i < max_lines {
            out.push(self.hits[i].line());
            i += 1;
        }
        out
    }

    /// 注入面指纹（对拍用；独立于主账本指纹域）。
    pub fn fingerprint(&self) -> u64 {
        let mut buf = Vec::new();
        push_u64_le(&mut buf, FINGERPRINT_TAG ^ 0x1ACE_0000_0000_0001);
        push_u64_le(&mut buf, self.out_injected);
        push_u64_le(&mut buf, self.out_detected);
        push_u64_le(&mut buf, self.loss_injected);
        push_u64_le(&mut buf, self.loss_detected);
        push_u64_le(&mut buf, self.clean_checked);
        push_u64_le(&mut buf, self.clean_false_positives);
        push_u64_le(&mut buf, self.hits.len() as u64);
        push_u64_le(&mut buf, self.hits_overflow);
        push_u64_le(&mut buf, self.tsc_cursor);
        fnv64(&buf, FNV_OFFSET)
    }
}

// ---------------------------------------------------------------------------
// 双跑确定性对拍（同种子恒同账）
// ---------------------------------------------------------------------------

/// 双跑结果：两账指纹与是否一致。
pub struct DualRunResult {
    /// 第一跑指纹。
    pub fp_first: u64,
    /// 第二跑指纹。
    pub fp_second: u64,
    /// 是否一致。
    pub identical: bool,
}

/// 双跑主账本（同种子同轮数），返回对拍结果。
pub fn dual_run_ledger(seed: u64, rounds: u64) -> Result<DualRunResult, DlineAccError> {
    let mut a = DlineAccLedger::new(seed);
    let mut b = DlineAccLedger::new(seed);
    a.run_rounds(rounds)?;
    b.run_rounds(rounds)?;
    let fa = a.fingerprint();
    let fb = b.fingerprint();
    Ok(DualRunResult {
        fp_first: fa,
        fp_second: fb,
        identical: fa == fb,
    })
}

/// 双跑对拍检查版：不一致走 Err（0x141B）。
pub fn dual_run_check(seed: u64, rounds: u64) -> Result<DualRunResult, DlineAccError> {
    let r = dual_run_ledger(seed, rounds)?;
    if !r.identical {
        return Err(DlineAccError::FingerprintMismatch {
            first: r.fp_first,
            second: r.fp_second,
        });
    }
    Ok(r)
}

/// 双跑注入面（同种子），对拍注入面指纹。
pub fn dual_run_bench(seed: u64, rounds: u64) -> Result<DualRunResult, DlineAccError> {
    let mut a = FaultInjectionBench::new(seed);
    let mut b = FaultInjectionBench::new(seed);
    a.run_out_of_tolerance(rounds);
    a.run_segment_loss(rounds);
    b.run_out_of_tolerance(rounds);
    b.run_segment_loss(rounds);
    let fa = a.fingerprint();
    let fb = b.fingerprint();
    Ok(DualRunResult {
        fp_first: fa,
        fp_second: fb,
        identical: fa == fb,
    })
}

// ---------------------------------------------------------------------------
// 测试（宿主档；冒烟判据：五段分解/对账 5% 门/p99 达标/对比账/倒退拒收/
// 注入检测率/双跑一致/错误诊断/健康快照）
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    /// 判据 600 环满档轮数。
    const FULL_ROUNDS: u64 = RING_CAP as u64;

    #[test]
    fn five_segment_decomposition_and_percentiles() {
        let mut led = DlineAccLedger::new(0x1410_0001);
        let sum = led.run_rounds(FULL_ROUNDS).unwrap_or_else(|e| {
            panic!("run_rounds 失败: {}", e.detail())
        });
        assert_eq!(sum.accepted, FULL_ROUNDS);
        assert_eq!(sum.retro_rejected, 0);
        assert_eq!(sum.loss_rejected, 0);
        let stats = led.segment_table().unwrap_or_else(|e| panic!("段表失败: {}", e.detail()));
        assert_eq!(stats.len(), SEG_COUNT);
        let mut i = 0usize;
        while i < stats.len() {
            let st = &stats[i];
            assert_eq!(st.count, RING_CAP, "段 {} 环未满", i);
            assert!(st.mean_ns > 0, "段 {} 均值非正", i);
            assert!(st.p95_ns <= st.p99_ns, "段 {} P95>P99", i);
            assert!(st.p99_ns <= st.max_ns, "段 {} P99>极大", i);
            assert!(st.p99_ns <= SEGMENT_CEIL_NS[i], "段 {} P99 越段级天花板", i);
            i += 1;
        }
        // 段均值与模型档对位（允许抖动内浮动）。
        assert!(stats[SEG_WRITE_MSR].mean_ns >= WRITE_MSR_BASE_NS);
        assert!(stats[SEG_WRITE_MSR].mean_ns <= WRITE_MSR_BASE_NS + WRITE_MSR_JITTER_NS);
        assert!(stats[SEG_DELIVERY].mean_ns >= DELIVERY_BASE_NS);
        assert!(stats[SEG_IRQ_EXIT].mean_ns >= IRQ_EXIT_BASE_NS);
        assert!(stats[SEG_ENTRY].mean_ns >= ENTRY_BASE_NS);
        assert!(stats[SEG_REARM].mean_ns >= REARM_BASE_NS);
    }

    #[test]
    fn reconcile_gate_within_5pct() {
        let mut led = DlineAccLedger::new(0x1410_0002);
        let sum = led.run_rounds(FULL_ROUNDS).unwrap_or_else(|e| panic!("run 失败: {}", e.detail()));
        // 判据 J1-c：最大对账 ppm 必须严格小于 5% 门。
        assert!(
            sum.batch_max_ppm < RECONCILE_TOL_PPM,
            "批内最大 ppm {} 越门 {}",
            sum.batch_max_ppm,
            RECONCILE_TOL_PPM
        );
        let d = led.decomposer();
        assert_eq!(d.case_count(), 0, "不应有立案");
        assert_eq!(d.reconcile_breaches(), 0);
        assert!(d.reconcile_max_ppm() < RECONCILE_TOL_PPM);
        assert!(d.reconcile_max_ppm() <= CROSS_NOISE_PCT_MAX * 10_000, "对账差不应超模型噪声档");
        assert!(d.events().count() == 0, "干净档不应有事件");
    }

    #[test]
    fn p99_pass_verdict_hard_line() {
        let mut led = DlineAccLedger::new(0x1410_0003);
        led.run_rounds(FULL_ROUNDS).unwrap_or_else(|e| panic!("run 失败: {}", e.detail()));
        let v = led.verdict().unwrap_or_else(|e| panic!("判定失败: {}", e.detail()));
        assert!(v.pass, "判定应达标: {}", v.note);
        assert!(!v.failing);
        assert!(v.full_p99_ns <= CHAIN_BUDGET_NS);
        assert!(v.headroom_ns > 0);
        assert_eq!(v.budget_ns, 10_000);
        assert!(v.rounds == FULL_ROUNDS);
        assert!(v.reconcile_within);
        assert!(v.note.contains("达标"));
    }

    #[test]
    fn p99_miss_locates_max_segment() {
        // 注入面验证失守定位：人工构造超预算样本喂给分解器，判定应转失守
        // 且定位到最大段。直接用分解器 + ModelGen，不经账本门。
        let mut gen = ModelGen::new(0x1410_0004);
        let mut d = ErrorDecomposer::new();
        let mut tsc = 1_000u64;
        let mut i = 0u64;
        while i < 120 {
            tsc += TSC_STRIDE_NS;
            let s = gen.inject_outlier(tsc, SEG_DELIVERY);
            let out = d.record(&s).unwrap_or_else(|e| panic!("record 失败: {}", e.detail()));
            assert!(out.accepted);
            i += 1;
        }
        let v = AccuracyVerdict::assess(&d).unwrap_or_else(|e| panic!("判定失败: {}", e.detail()));
        assert!(!v.pass, "超差注入后判定必须失守: {}", v.note);
        assert!(v.failing);
        assert_eq!(v.max_seg_idx, SEG_DELIVERY, "失守应定位到投递段");
        assert_eq!(v.max_seg_name, SEGMENT_NAMES[SEG_DELIVERY]);
        assert!(v.headroom_ns == 0);
        assert!(v.note.contains("失守"));
        // 对账漂移注入：越 5% 门必立案（案发轮最大段为粗定位）。
        let mut gen2 = ModelGen::new(0x1410_0004B);
        let mut d2 = ErrorDecomposer::new();
        let mut tsc2 = 50_000u64;
        let mut j = 0u64;
        while j < 40 {
            tsc2 += TSC_STRIDE_NS;
            let s = gen2.inject_recon_drift(tsc2);
            let out = d2.record(&s).unwrap_or_else(|e| panic!("record 失败: {}", e.detail()));
            assert!(out.accepted);
            assert!(out.ppm >= RECONCILE_TOL_PPM, "漂移注入 ppm={} 必越门", out.ppm);
            j += 1;
        }
        assert_eq!(d2.reconcile_breaches(), 40);
        assert_eq!(d2.case_count(), 40, "40 桩全部落簿（<64 封顶）");
        assert_eq!(d2.reconcile_max_ppm() >= RECONCILE_TOL_PPM, true);
        let case = d2.case_at(0).unwrap_or_else(|| panic!("立案缺失"));
        assert!(case.ppm >= RECONCILE_TOL_PPM);
        assert!(case.line().contains("立案"));
        assert!(d2.events().count() >= 40, "每桩都应有越界事件");
    }

    #[test]
    fn mode_comparison_quantifies_improvement() {
        let mut led = DlineAccLedger::new(0x1410_0005);
        led.run_rounds(FULL_ROUNDS).unwrap_or_else(|e| panic!("run 失败: {}", e.detail()));
        let cmp = led.comparison().unwrap_or_else(|e| panic!("对比账失败: {}", e.detail()));
        assert!(cmp.improvement_ns > 0, "deadline 必须比分频档提升");
        assert!(cmp.improvement_ppm > 0);
        assert!(cmp.divider_p99_ns > cmp.deadline_p99_ns);
        assert!(cmp.deadline_within_budget);
        assert!(cmp.divider_within_budget, "分频档 P99={} 应 ≤50000ns", cmp.divider_p99_ns);
        assert!(cmp.columns.contains("deadline"));
        assert!(cmp.columns.contains("分频档"));
        assert!(cmp.columns.contains("提升量化"));
        // 分频档对照门（F1053 分行口径）。
        let (dv_p99, dv_ok) = led.divider_check().unwrap_or_else(|e| panic!("分频门失败: {}", e.detail()));
        assert!(dv_ok);
        assert!(dv_p99 == cmp.divider_p99_ns);
    }

    #[test]
    fn retrograde_rejection_strict_monotonic() {
        let mut d = ErrorDecomposer::new();
        let mut gen = ModelGen::new(0x1410_0006);
        // 第一笔 tsc=1000 正常入环。
        let s1 = gen.next_chain(1_000);
        let o1 = d.record(&s1).unwrap_or_else(|e| panic!("record 失败: {}", e.detail()));
        assert!(o1.accepted);
        assert_eq!(o1.reason, AcceptReason::Ok);
        // 持平 tsc=1000：严格单调校验应拒收（持平也算倒退）。
        let s2 = gen.next_chain(1_000);
        let o2 = d.record(&s2).unwrap_or_else(|e| panic!("record 失败: {}", e.detail()));
        assert!(!o2.accepted);
        assert_eq!(o2.reason, AcceptReason::Retrograde);
        assert_eq!(o2.full_ns, s2.full_ns, "拒收轮仍回带全链实测");
        assert_eq!(o2.ppm, 0);
        // 倒退 tsc=999：拒收。
        let s3 = gen.next_chain(999);
        let o3 = d.record(&s3).unwrap_or_else(|e| panic!("record 失败: {}", e.detail()));
        assert!(!o3.accepted);
        assert_eq!(o3.reason, AcceptReason::Retrograde);
        // 恢复 tsc=1001：入环。
        let s4 = gen.next_chain(1_001);
        let o4 = d.record(&s4).unwrap_or_else(|e| panic!("record 失败: {}", e.detail()));
        assert!(o4.accepted);
        assert_eq!(d.retro_rejects(), 2);
        assert_eq!(d.rounds_recorded(), 2);
        // 事件账应有两笔倒退事件（码 0x1410）。
        let mut found = 0u64;
        let mut e = 0usize;
        while e < d.events().count() {
            if let Some((code, _)) = d.events().get(e) {
                if code == C_TSC_RETROGRADE {
                    found += 1;
                }
            }
            e += 1;
        }
        assert_eq!(found, 2);
    }

    #[test]
    fn incomplete_sample_rejected_structurally() {
        let mut d = ErrorDecomposer::new();
        let mut gen = ModelGen::new(0x1410_0007);
        let mut s = gen.next_chain(5_000);
        s.seg[SEG_REARM] = 0;
        s.missing_mask |= MASK_REARM;
        let o = d.record(&s).unwrap_or_else(|e| panic!("record 失败: {}", e.detail()));
        assert!(!o.accepted);
        assert_eq!(o.reason, AcceptReason::Incomplete);
        assert_eq!(d.loss_rejects(), 1);
        assert_eq!(d.rounds_recorded(), 0);
        assert_eq!(d.chain_p99(), Err(DlineAccError::EmptyRing { tag: "chain" }));
    }

    #[test]
    fn zero_chain_is_hard_error() {
        let mut d = ErrorDecomposer::new();
        let s = ChainSample {
            tsc: 7_000,
            seg: [0; SEG_COUNT],
            full_ns: 0,
            missing_mask: 0,
        };
        match d.record(&s) {
            Err(e) => {
                assert_eq!(e.code(), C_ZERO_CHAIN);
                assert!(e.detail().contains("full_ns"));
            }
            Ok(_) => panic!("全链零值必须 Err"),
        }
        // 零值事件入账。
        assert!(d.events().count() >= 1);
    }

    #[test]
    fn injection_detection_rates_and_false_positives() {
        let mut bench = FaultInjectionBench::new(0x1410_0008);
        // 超差序列注入：检测率必须 100%。
        let o = bench.run_out_of_tolerance(120);
        assert_eq!(o.injected, 120);
        assert_eq!(o.detected, 120, "超差注入检出率必须 100%: {}", o.line());
        assert_eq!(o.rate_pct, 100);
        assert_eq!(o.missed, 0);
        // 段丢失注入：结构完整门必抓，检测率 100%。
        let l = bench.run_segment_loss(120);
        assert_eq!(l.detected, 120, "段丢失检出率必须 100%: {}", l.line());
        assert_eq!(l.rate_pct, 100);
        // 清洁误报自查：误报必须 0。
        let fps = bench.run_clean_false_positive_check(300);
        assert_eq!(fps, 0, "清洁样本误报必须 0");
        let (co, cl, cc) = bench.cumulative();
        assert_eq!(co.rate_pct, 100);
        assert_eq!(cl.rate_pct, 100);
        assert_eq!(cc.false_positives, 0);
        assert!(bench.health_line().contains("干净"));
        assert!(bench.health_line().contains("检出率100%"));
        // 命中簿有账且不越封顶。
        assert!(!bench.hits_render(10).is_empty());
        assert!(bench.fingerprint() != 0);
    }

    #[test]
    fn out_of_tolerance_fires_all_expected_gates() {
        let mut bench = FaultInjectionBench::new(0x1410_0009);
        let mut gen = ModelGen::new(0x1410_000A);
        let s = gen.inject_outlier(9_000, SEG_IRQ_EXIT);
        let hits = bench.detect(&s);
        // 期望同时命中：段级超差 + 全链爆线（对账噪声比例制，超差不引发越界，
        // 越界路径由 recon_drift 注入单独承担——见 p99_miss 测试后半段）。
        let mut has_seg = false;
        let mut has_chain = false;
        let mut h = 0usize;
        while h < hits.len() {
            match hits[h].kind {
                DetectKind::SegmentOutlier => has_seg = true,
                DetectKind::ChainBudget => has_chain = true,
                _ => {}
            }
            h += 1;
        }
        assert!(has_seg, "应命中段级超差");
        assert!(has_chain, "应命中全链爆线");
    }

    #[test]
    fn dual_run_determinism_identical_fingerprints() {
        // 主账本双跑对拍。
        let r = dual_run_check(0x1410_00BB, FULL_ROUNDS)
            .unwrap_or_else(|e| panic!("双跑失败: {}", e.detail()));
        assert!(r.identical);
        assert_eq!(r.fp_first, r.fp_second);
        assert!(r.fp_first != 0);
        // 注入面双跑对拍。
        let rb = dual_run_bench(0x1410_00CC, 40)
            .unwrap_or_else(|e| panic!("注入面双跑失败: {}", e.detail()));
        assert!(rb.identical);
        // 异种子指纹必须不同（防指纹退化为常量）。
        let ra = dual_run_ledger(0x1410_00D1, 200).unwrap_or_else(|e| panic!("双跑失败: {}", e.detail()));
        let rd = dual_run_ledger(0x1410_00D2, 200).unwrap_or_else(|e| panic!("双跑失败: {}", e.detail()));
        assert!(ra.fp_first != rd.fp_first);
    }

    #[test]
    fn error_variants_details_and_codes() {
        let cases: [DlineAccError; 7] = [
            DlineAccError::ZeroChainSample,
            DlineAccError::EmptyRing { tag: "chain" },
            DlineAccError::BadPercentile(101),
            DlineAccError::BadSegmentIndex(9),
            DlineAccError::RoundsShort { have_needed: 600, have_now: 0 },
            DlineAccError::FingerprintMismatch { first: 1, second: 2 },
            DlineAccError::DividerP99Miss { p99_ns: 60_000 },
        ];
        let mut i = 0usize;
        while i < cases.len() {
            assert!(!cases[i].detail().is_empty(), "变体 {} detail 为空", i);
            let c = cases[i].code();
            assert!(c >= 0x1410 && c <= 0x141F, "变体 {} 诊断码 0x{:04X} 越段", i, c);
            i += 1;
        }
        // 具体码对位。
        assert_eq!(cases[0].code(), C_ZERO_CHAIN);
        assert_eq!(cases[1].code(), C_EMPTY_RING);
        assert_eq!(cases[2].code(), C_BAD_PERCENTILE);
        assert_eq!(cases[3].code(), C_BAD_SEG_INDEX);
        assert_eq!(cases[4].code(), C_ROUNDS_SHORT);
        assert_eq!(cases[5].code(), C_FINGERPRINT_MISMATCH);
        assert_eq!(cases[6].code(), C_DIVIDER_MISS);
    }

    #[test]
    fn percentile_and_ring_edges() {
        let mut ring = SampleRing::new(4);
        // 空环分位 → Err(EmptyRing)。
        assert_eq!(
            ring.percentile(99),
            Err(DlineAccError::EmptyRing { tag: "ring" })
        );
        // 非法分位 → Err(BadPercentile)。
        ring.push(10);
        assert_eq!(ring.percentile(101), Err(DlineAccError::BadPercentile(101)));
        // 覆写语义：cap=4 推 6 笔，最旧两笔被覆写。
        ring.push(20);
        ring.push(30);
        ring.push(40);
        ring.push(50);
        ring.push(60);
        assert_eq!(ring.count(), 4);
        assert_eq!(ring.total_pushed(), 6);
        assert_eq!(ring.total_overwritten(), 2);
        let snap = ring.snapshot();
        assert_eq!(snap, alloc_vec_from(&[30, 40, 50, 60]));
        // 分位插值边界：P0=极小，P100=极大。
        assert_eq!(ring.percentile(0).unwrap_or_else(|e| panic!("p0: {}", e.detail())), 30);
        assert_eq!(ring.percentile(100).unwrap_or_else(|e| panic!("p100: {}", e.detail())), 60);
        assert_eq!(ring.percentile(50).unwrap_or_else(|e| panic!("p50: {}", e.detail())), 50);
        assert_eq!(ring.max_value(), 60);
        assert_eq!(ring.sum_saturating(), 180);
        assert_eq!(ring.mean(), 45);
    }

    /// 测试辅助：切片转 Vec（避免在测试外引 vec! 宏）。
    fn alloc_vec_from(src: &[u64]) -> Vec<u64> {
        let mut out = Vec::new();
        let mut i = 0usize;
        while i < src.len() {
            out.push(src[i]);
            i += 1;
        }
        out
    }

    #[test]
    fn histogram_binning_and_coverage() {
        let mut h = SegmentHistogram::new();
        h.push(0);
        h.push(99);
        h.push(100);
        h.push(HIST_BIN_NS * (HIST_BINS as u64 - 1));
        h.push(HIST_BIN_NS * HIST_BINS as u64); // 溢出桶
        assert_eq!(h.total_count(), 5);
        assert_eq!(h.overflow_count(), 1);
        assert_eq!(h.coverage_pct(), 80);
        assert!(h.mode_bin() < HIST_BINS);
        // 空图覆盖率 0。
        let e = SegmentHistogram::new();
        assert_eq!(e.coverage_pct(), 0);
        assert_eq!(e.total_count(), 0);
    }

    #[test]
    fn health_and_snapshot_consistency() {
        let mut led = DlineAccLedger::new(0x1410_00E1);
        led.run_rounds(FULL_ROUNDS).unwrap_or_else(|e| panic!("run 失败: {}", e.detail()));
        let hp = led.health().unwrap_or_else(|e| panic!("健康账失败: {}", e.detail()));
        assert_eq!(hp.status, HealthStatus::Good, "干净档应为良好: {}", hp.lines.last().unwrap_or(&String::new()));
        assert!(hp.score >= 90);
        assert_eq!(hp.lines.len(), 6);
        let snap = led.snapshot().unwrap_or_else(|e| panic!("快照失败: {}", e.detail()));
        assert!(snap.contains("UNX-F1044"));
        assert!(snap.contains("判定"));
        assert!(snap.contains("五段分解"));
        assert!(snap.contains("分解对账"));
        assert!(snap.contains("模式对比账"));
        assert!(snap.contains("直方图"));
        assert!(snap.contains("事件账"));
        assert!(snap.contains("指纹"));
        assert!(snap.contains("零事件"));
        // 直方图行数 = 五段 + 全链。
        let hlines = led.histogram_lines().unwrap_or_else(|e| panic!("直方图失败: {}", e.detail()));
        assert_eq!(hlines.len(), SEG_COUNT + 1);
        // 快照指纹行与 fingerprint() 一致。
        assert!(snap.contains(&format!("0x{:016X}", led.fingerprint())));
    }

    #[test]
    fn rounds_short_guard_and_empty_verdict() {
        let d = ErrorDecomposer::new();
        match AccuracyVerdict::assess(&d) {
            Err(DlineAccError::RoundsShort { have_needed, have_now }) => {
                assert_eq!(have_needed, 1);
                assert_eq!(have_now, 0);
            }
            _ => panic!("空分解器判定必须 RoundsShort"),
        }
        // 快照在空账上必须 Err 而非 panic。
        let led = DlineAccLedger::new(0x1410_00F1);
        assert!(led.snapshot().is_err());
        assert!(led.divider_p99().is_err());
        assert!(led.comparison().is_err());
    }

    #[test]
    fn saturating_arithmetic_and_modulo_bias_guard() {
        // LCG 边界：bound=0 恒 0，不 panic。
        let mut l = Lcg::new(0);
        assert_eq!(l.next_range(0), 0);
        // 极值饱和：ppm 计算不溢出。
        let s = ChainSample {
            tsc: 1,
            seg: [u64::MAX; SEG_COUNT],
            full_ns: 1,
            missing_mask: 0,
        };
        let ppm = s.reconcile_ppm();
        assert_eq!(ppm, u64::MAX, "饱和路径应可达");
        // 全零链对账除零保护。
        let z = ChainSample { tsc: 2, seg: [0; SEG_COUNT], full_ns: 0, missing_mask: 0 };
        assert_eq!(z.reconcile_ppm(), u64::MAX);
        // LCG 同种子恒同序列。
        let mut a = Lcg::new(42);
        let mut b = Lcg::new(42);
        let mut i = 0u64;
        while i < 64 {
            assert_eq!(a.next_u64(), b.next_u64());
            i += 1;
        }
    }

    #[test]
    fn fnv64_known_vectors() {
        // FNV-1a 64 空串基：fnv64(&[], FNV_OFFSET) == FNV_OFFSET。
        assert_eq!(fnv64(&[], FNV_OFFSET), FNV_OFFSET);
        // 单字节 'a'（0x61）标准向量：af63dc4c8601ec8c。
        assert_eq!(fnv64(&[0x61], FNV_OFFSET), 0xaf63_dc4c_8601_ec8c);
        // 三字节 "foo"（0x66 0x6f 0x6f）标准向量：dcb27518fed9d577。
        assert_eq!(fnv64(&[0x66, 0x6f, 0x6f], FNV_OFFSET), 0xdcb2_7518_fed9_d577);
    }

    #[test]
    fn event_ring_cap_and_overflow_accounting() {
        let mut ev = EventRing::new(8);
        let mut i = 0u16;
        while i < 12 {
            ev.push(C_SEG_OUTLIER, format!("事件{}", i));
            i += 1;
        }
        assert_eq!(ev.count(), 8);
        assert_eq!(ev.total_overwritten(), 4);
        // 最旧被丢：现存首条为事件 4。
        let first = ev.get(0).unwrap_or_else(|| panic!("事件缺失"));
        assert_eq!(first.0, C_SEG_OUTLIER);
        assert!(first.1.contains("事件4"));
        let rendered = ev.render();
        assert_eq!(rendered.len(), 8);
        assert!(rendered[0].contains("0x1413"));
    }

    #[test]
    fn accept_reason_and_detect_kind_labels() {
        assert_eq!(AcceptReason::Ok.as_str(), "入环");
        assert!(AcceptReason::Retrograde.as_str().contains("倒退"));
        assert!(AcceptReason::Incomplete.as_str().contains("缺失"));
        assert_eq!(AcceptReason::Ok, AcceptReason::Ok);
        assert_ne!(AcceptReason::Ok, AcceptReason::Retrograde);
        assert!(DetectKind::SegmentOutlier.as_str().contains("超差"));
        assert!(DetectKind::ChainBudget.as_str().contains("爆线"));
        assert!(DetectKind::SegmentLoss.as_str().contains("丢失"));
        assert!(DetectKind::ReconcileBreach.as_str().contains("越界"));
        // HealthStatus 三态标签。
        assert_eq!(HealthStatus::Good.as_str(), "良好");
        assert_eq!(HealthStatus::Degrade.as_str(), "降级");
        assert_eq!(HealthStatus::Bad.as_str(), "异常");
    }

    #[test]
    fn batch_summary_fields_flow_through() {
        let mut led = DlineAccLedger::new(0x1410_0101);
        let s = led.run_rounds(100).unwrap_or_else(|e| panic!("run 失败: {}", e.detail()));
        assert_eq!(s.requested, 100);
        assert_eq!(s.accepted, 100);
        assert_eq!(s.retro_rejected + s.loss_rejected, 0);
        assert!(s.batch_max_ppm <= CROSS_NOISE_PCT_MAX * 10_000);
        // 连跑两批：环仍 600 封顶（后 600 覆盖前 100+500）。
        let s2 = led.run_rounds(600).unwrap_or_else(|e| panic!("run2 失败: {}", e.detail()));
        assert_eq!(s2.accepted, 600);
        let d = led.decomposer();
        assert_eq!(d.rounds_recorded(), 700);
        assert_eq!(d.chain_p99().unwrap_or_else(|e| panic!("p99: {}", e.detail())) > 0, true);
        assert_eq!(led.divider_p99().unwrap_or_else(|e| panic!("dv: {}", e.detail())) > 0, true);
        // 指纹两次取值一致（只读稳定）。
        let f1 = led.fingerprint();
        let f2 = led.fingerprint();
        assert_eq!(f1, f2);
    }
}
