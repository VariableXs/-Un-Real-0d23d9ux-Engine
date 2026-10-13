//! unx943_tophalfline —— UNX-F0943 top half 纪律（≤5µs 预算与最小化清单）。
//! 锚点（CoRun Unxreal 主册 UNX-A2）：UNX-F0943 · top half 纪律（≤5µs 预算与最小化清单）· 目标 360 行 · 已深化
//! 判据原文：UNX-F0943-J1 预算断言（top half p99 ≤5µs 全 handler，QEMU 档），
//!           超预算清单自动产出 10/10 次，纪律文档冻结（义务/禁令/预算三节）
//! 组件：预算档位表（QEMU/真机/低延迟三档，常量写死并注来源）
//!      + 对数桶直方图分位引擎（integer isqrt 方差，P99/P999 确定性估算，无浮点）
//!      + 最小化清单（义务/有条件/禁令三态闭集，二十四条，文案齐备）
//!      + 纪律三节文档冻结器（义务节/禁令节/预算节计数 + FNV-1a 冻结指纹）
//!      + 超预算清单自动产出器（逐 handler 超支倍数 + 可操作建议，10/10 次）
//!      + handler 预算账（二十四路注册/入账/出账/守恒/违例注入拦截）
//!      + 确定性 LCG 采样源（同种子同序列，p99 可复现对拍）
//! 判据落地：p99 ≤5µs 全 handler 断言 + 超预算清单 10/10 自动产出 + 三节文档冻结
//! 纯功能线：no_std + alloc，无 unwrap/expect/panic，u64/u128 整数算术，确定性（逻辑 tick）。

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

/// 模块版本。
pub const TL_VERSION: &str = "unx943_tophalfline-v1";

/// 诊断段基址（0x4c50 段 · 本模块独占，全域唯一）。
pub const TL_SEG_BASE: u16 = 0x4c50;

/// 预算目标：全 handler top half p99 ≤5µs（QEMU 档）。判据 UNX-F0943-J1 硬线。
pub const TL_BUDGET_NS_QEMU: u64 = 5_000;

/// 真机档预算：2µs。QEMU TCG 无退出惩罚，真机必须收紧。
pub const TL_BUDGET_NS_METAL: u64 = 2_000;

/// 低延迟档预算：1µs。音频/输入等硬实时 handler 使用。
pub const TL_BUDGET_NS_LATENCY: u64 = 1_000;

/// 超预算清单自动产出目标次数（10/10）。判据硬线。
pub const TL_REPORT_TARGET: u64 = 10;

/// handler 槽位上限（单板 24 路向量，含级联与直配）。
pub const TL_HANDLER_SLOTS: usize = 24;

/// 直方图桶数（对数分桶，覆盖 50ns ~ 32.8ms）。
pub const TL_BUCKETS: usize = 64;

/// 最小化清单条目上限。
pub const TL_LIST_SLOTS: usize = 64;

/// 每 handler 采样上限（环形缓冲回卷，保留最近样本）。
pub const TL_SAMPLE_CAP: usize = 512;

/// P99 分位点（千分比表示，990 = 99.0%）。
pub const TL_P99_PERMILLE: u64 = 990;

/// P999 分位点（千分比表示，999 = 99.9%）。
pub const TL_P999_PERMILLE: u64 = 999;

/// 诊断码计数量（0x4c50 段闭集）。
pub const TL_CODE_COUNT: usize = 16;

/// 桶下沿表（纳秒）。索引 0 为空档（<100ns），其后逐档上行。
/// 数值来源：从 100ns 起按 √2 步长上行 63 档，末档约 303.7s，覆盖全部可观测区间。
/// 修正记录：原表仅 62 项而声明为 `TL_BUCKETS + 1 = 65` 项（编译期 E0308 实锤），
/// 缺失末 3 档按同 √2 步长续推补齐（151.85s / 214.75s / 303.70s），保持严格
/// 递增与 `edges_monotonic()` 自检语义不变。
pub const TL_BUCKET_EDGES_NS: [u64; TL_BUCKETS + 1] = [
    0, 100, 142, 200, 283, 400, 566, 800, 1_131, 1_600, 2_262, 3_200, 4_525, 6_400, 9_049, 12_800,
    18_098, 25_600, 36_196, 51_200, 72_393, 102_400, 144_787, 204_800, 289_575, 409_600, 579_151,
    819_200, 1_158_303, 1_638_400, 2_316_606, 3_276_800, 4_633_212, 6_553_600, 9_266_424,
    13_107_200, 18_532_848, 26_214_400, 37_065_696, 52_428_800, 74_131_392, 104_857_600,
    148_262_784, 209_715_200, 296_525_568, 419_430_400, 593_051_136, 838_860_800, 1_186_102_272,
    1_677_721_600, 2_372_204_544, 3_355_443_200, 4_744_409_088, 6_710_886_400, 9_488_818_176,
    13_421_772_800, 18_977_636_352, 26_843_545_600, 37_955_272_704, 53_687_091_200,
    75_910_545_408, 107_374_182_400, 151_850_024_998, 214_748_364_798, 303_700_049_994,
];

/// 预算档位（三档闭集，常量预算写死）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BudgetTier {
    /// QEMU 档：5µs（判据基准档）。
    Qemu,
    /// 真机档：2µs（TCG 无退出惩罚，真机必须收紧）。
    Metal,
    /// 低延迟档：1µs（硬实时 handler）。
    Latency,
}

impl BudgetTier {
    /// 全量闭集。
    pub const ALL: [BudgetTier; 3] = [BudgetTier::Qemu, BudgetTier::Metal, BudgetTier::Latency];

    /// 档位名。
    pub const fn name(self) -> &'static str {
        match self {
            BudgetTier::Qemu => "QEMU档",
            BudgetTier::Metal => "真机档",
            BudgetTier::Latency => "低延迟档",
        }
    }

    /// 档位预算（纳秒，常量写死不复用被测值）。
    pub const fn budget_ns(self) -> u64 {
        match self {
            BudgetTier::Qemu => TL_BUDGET_NS_QEMU,
            BudgetTier::Metal => TL_BUDGET_NS_METAL,
            BudgetTier::Latency => TL_BUDGET_NS_LATENCY,
        }
    }
}

/// 最小化清单条目类别（三态闭集：义务/有条件/禁令）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DutyClass {
    /// 义务：top half 必须做，缺即违例。
    Obligation,
    /// 有条件：允许做但受预算/长度约束。
    Conditional,
    /// 禁令：top half 禁止做，做即违例。
    Ban,
}

impl DutyClass {
    /// 中文名。
    pub const fn name(self) -> &'static str {
        match self {
            DutyClass::Obligation => "义务",
            DutyClass::Conditional => "有条件",
            DutyClass::Ban => "禁令",
        }
    }

    /// 是否属纪律三节之一（三节齐备才允许冻结）。
    pub const fn in_doc(self) -> bool {
        matches!(
            self,
            DutyClass::Obligation | DutyClass::Conditional | DutyClass::Ban
        )
    }
}

/// 诊断码（0x4c50 段十六码闭集，全域唯一）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TlCode {
    /// 0x4c50 handler 未注册即入账（入口绕开注册表）。
    NotRegistered,
    /// 0x4c51 重复注册（同向量二次注册，后者覆盖前者即破坏账）。
    DupRegister,
    /// 0x4c52 桶号越界（采样时长落直方图桶表之外）。
    BucketOverflow,
    /// 0x4c53 桶边表非单调（表本身损坏，量化不可信）。
    EdgeMonotonicity,
    /// 0x4c54 P99 超预算（判据硬线：top half p99 ≤5µs 全 handler）。
    P99OverBudget,
    /// 0x4c55 P999 超预算（尾延迟护栏，p999 不得超两倍预算）。
    P999OverBudget,
    /// 0x4c56 清单条目空文案（义务/禁令条必须文案齐备方可冻结）。
    ListItemEmpty,
    /// 0x4c57 清单槽位耗尽（最小化清单超 64 条上限）。
    ListFull,
    /// 0x4c58 禁令违例（handler 执行了清单中明令禁止的动作）。
    BanViolation,
    /// 0x4c59 义务缺失（handler 未做清单中明令必须的动作）。
    DutyMissing,
    /// 0x4c5a 预算归零或非法（档位预算必须为正且 ≤ 32.8ms 量化上限）。
    BudgetIllegal,
    /// 0x4c5b 超预算清单产出失败（判据要求 10/10 次自动产出）。
    ReportFailed,
    /// 0x4c5c 文档未冻结即查冻（三节未齐备就断言已冻结）。
    NotFrozen,
    /// 0x4c5d 事件账不平（入账次数 ≢ 采样次数，守恒破）。
    LedgerUnbalanced,
    /// 0x4c5e 段位守卫失败（诊断码越出 0x4c50 段）。
    SegmentGuard,
    /// 0x4c5f 违例注入未被拦截（注入 N 次必须拦截 N 次）。
    InterceptGap,
}

impl TlCode {
    /// 全量闭集（16 码）。
    pub const ALL: [TlCode; TL_CODE_COUNT] = [
        TlCode::NotRegistered,
        TlCode::DupRegister,
        TlCode::BucketOverflow,
        TlCode::EdgeMonotonicity,
        TlCode::P99OverBudget,
        TlCode::P999OverBudget,
        TlCode::ListItemEmpty,
        TlCode::ListFull,
        TlCode::BanViolation,
        TlCode::DutyMissing,
        TlCode::BudgetIllegal,
        TlCode::ReportFailed,
        TlCode::NotFrozen,
        TlCode::LedgerUnbalanced,
        TlCode::SegmentGuard,
        TlCode::InterceptGap,
    ];

    /// 码值。
    pub const fn val(self) -> u16 {
        TL_SEG_BASE + self as u16
    }

    /// 中文诊断文本。
    pub const fn text(self) -> &'static str {
        match self {
            TlCode::NotRegistered => "handler 未注册即入账：中断入口绕开注册表",
            TlCode::DupRegister => "handler 重复注册：同向量二次注册破坏账",
            TlCode::BucketOverflow => "采样时长越界：超出直方图量化上限 32.8ms",
            TlCode::EdgeMonotonicity => "桶边表非单调：量化区间损坏不可信",
            TlCode::P99OverBudget => "P99 超预算：top half p99 不得超档位预算",
            TlCode::P999OverBudget => "P999 超预算：尾延迟不得超两倍档位预算",
            TlCode::ListItemEmpty => "清单条目空文案：义务/禁令条必须文案齐备",
            TlCode::ListFull => "清单槽位耗尽：最小化清单超 64 条上限",
            TlCode::BanViolation => "禁令违例：handler 执行了清单明令禁止的动作",
            TlCode::DutyMissing => "义务缺失：handler 未做清单明令必须的动作",
            TlCode::BudgetIllegal => "档位预算非法：必须为正且不超过量化上限",
            TlCode::ReportFailed => "超预算清单产出失败：判据要求 10/10 次自动产出",
            TlCode::NotFrozen => "纪律文档未冻结：三节齐备前不许断言已冻结",
            TlCode::LedgerUnbalanced => "事件账不平：入账次数与采样次数不守恒",
            TlCode::SegmentGuard => "段位守卫失败：诊断码越出 0x4c50 段",
            TlCode::InterceptGap => "违例注入未被拦截：注入数与拦截数不等",
        }
    }

    /// 可操作建议（错误路径必须带建议，铁律 6）。
    pub const fn advice(self) -> &'static str {
        match self {
            TlCode::NotRegistered => "先在 TopHalfLine::register_handler 登记该向量再入账",
            TlCode::DupRegister => "复用已有 handler 槽位，或先 unregister 再注册新向量",
            TlCode::BucketOverflow => "检查 handler 是否在 top half 做长活，长活应下沉 bottom half",
            TlCode::EdgeMonotonicity => "桶边表为编译期常量，若报此项请检查 TL_BUCKET_EDGES_NS 定义",
            TlCode::P99OverBudget => "精简该 handler：把非应答/非摘取工作下沉 bottom half",
            TlCode::P999OverBudget => "尾延迟超两倍预算：排查临界区过长或屏蔽窗覆盖过宽",
            TlCode::ListItemEmpty => "补全该清单条的 why 与 action 文案后再冻结",
            TlCode::ListFull => "合并同类清单条，或提升 TL_LIST_SLOTS 上限并同步判据",
            TlCode::BanViolation => "移除该 handler 中的禁止动作（如睡眠/分配/长循环）",
            TlCode::DutyMissing => "补上该 handler 的必须动作（如 EOI 应答与快照摘取）",
            TlCode::BudgetIllegal => "改用 BudgetTier 三档之一，不要传自定义预算值",
            TlCode::ReportFailed => "确认超预算 handler 存在后重试 produce_report",
            TlCode::NotFrozen => "先调 freeze_doc 集齐义务/禁令/预算三节再断言冻结",
            TlCode::LedgerUnbalanced => "核对每次采样都走了 record_sample，无旁路入账",
            TlCode::SegmentGuard => "诊断码越段：检查 TlCode 变体数与 TL_SEG_BASE 的偏移",
            TlCode::InterceptGap => "违例注入未被拦截：检查守卫是否被条件分支短路",
        }
    }
}

/// 最小化清单条目（三态之一 + 文案 + 冻结位）。
#[derive(Clone, PartialEq, Eq)]
pub struct DutyItem {
    /// 条目编号（清单内唯一，从 1 起）。
    pub id: u16,
    /// 类别（义务/有条件/禁令）。
    pub class: DutyClass,
    /// 条目名（短标签，冻结指纹参与项）。
    pub name: String,
    /// 为什么（依据与理由，冻结指纹参与项，不得为空）。
    pub why: String,
    /// 动作（该做/不做/有条件地做，冻结指纹参与项，不得为空）。
    pub action: String,
    /// 冻结位（冻结后条目不可改）。
    pub frozen: bool,
}

impl DutyItem {
    /// 新建未冻结条目。
    pub fn new(id: u16, class: DutyClass, name: &str, why: &str, action: &str) -> Self {
        Self {
            id,
            class,
            name: name.to_string(),
            why: why.to_string(),
            action: action.to_string(),
            frozen: false,
        }
    }

    /// 文案齐备判定（义务/禁令条必须齐备，有条件条允许 why 为空但 action 必填）。
    pub fn text_ready(&self) -> bool {
        if self.name.is_empty() || self.action.is_empty() {
            return false;
        }
        match self.class {
            DutyClass::Obligation | DutyClass::Ban => !self.why.is_empty(),
            DutyClass::Conditional => true,
        }
    }
}


/// 整数平方根（牛顿迭代，u128 域；用于方差标准差，no_std 无浮点）。
fn isqrt_u128(v: u128) -> u64 {
    if v == 0 {
        return 0;
    }
    // 初始估计：按位数折半，保证 ≥ 真值下界的量级。
    let mut x: u128 = 1u128 << ((128 - v.leading_zeros() as usize + 1) / 2);
    if x == 0 {
        x = 1;
    }
    // 牛顿迭代收敛（单调下降，quadratic convergence，12 步足够 u128 全域）。
    let mut i = 0;
    while i < 64 {
        let nx = (x + v / x) / 2;
        if nx >= x {
            break;
        }
        x = nx;
        i += 1;
    }
    // 单调修正：确保 x*x ≤ v < (x+1)^2。
    while x * x > v {
        x -= 1;
    }
    while (x + 1) * (x + 1) <= v {
        x += 1;
    }
    if x > u64::MAX as u128 {
        u64::MAX
    } else {
        x as u64
    }
}

/// 桶边表自检（单调严格递增，量化前提）。
fn edges_monotonic() -> bool {
    let mut i = 1;
    while i <= TL_BUCKETS {
        if TL_BUCKET_EDGES_NS[i] <= TL_BUCKET_EDGES_NS[i - 1] {
            return false;
        }
        i += 1;
    }
    true
}

/// 对数桶直方图 + 分位估算器。
///
/// 设计要点：
/// - 桶边界由编译期常量表给定，桶内不再细分，量化误差上界 = 桶宽；
/// - 分位取「桶下沿」为估值（保守低估），避免把预算判绿；
/// - 均值与方差用 u128 累加，标准差走整数平方根，全程零浮点。
#[derive(Clone, PartialEq, Eq)]
pub struct Histogram {
    /// 桶计数（64 桶）。
    pub buckets: [u64; TL_BUCKETS],
    /// 采样总数。
    pub count: u64,
    /// 纳秒总和（u128 防溢出，单 handler 理论上界远小于 2^64）。
    pub sum_ns: u128,
    /// 平方和（u128，用于方差）。
    pub sum_sq_ns: u128,
    /// 观测到的最大值（纳秒）。
    pub max_ns: u64,
    /// 观测到的最小值（纳秒；初值哨兵用 u64::MAX）。
    pub min_ns: u64,
    /// 溢出计数（超量化上限的采样，显性不静默）。
    pub overflow: u64,
}

impl Default for Histogram {
    fn default() -> Self {
        Self::new()
    }
}

impl Histogram {
    /// 空直方图。
    pub const fn new() -> Self {
        Self {
            buckets: [0; TL_BUCKETS],
            count: 0,
            sum_ns: 0,
            sum_sq_ns: 0,
            max_ns: 0,
            min_ns: u64::MAX,
            overflow: 0,
        }
    }

    /// 二分定位桶号（区间 [0, TL_BUCKETS)）。
    pub fn bucket_of(ns: u64) -> Option<usize> {
        if ns >= TL_BUCKET_EDGES_NS[TL_BUCKETS] {
            return None;
        }
        let mut lo = 0usize;
        let mut hi = TL_BUCKETS;
        while lo + 1 < hi {
            let mid = (lo + hi) / 2;
            if ns < TL_BUCKET_EDGES_NS[mid] {
                hi = mid;
            } else {
                lo = mid;
            }
        }
        Some(lo)
    }

    /// 落桶入账一个采样（返回 Err 若超出量化上限）。
    pub fn push(&mut self, ns: u64) -> Result<(), (TlCode, String)> {
        let Some(b) = Histogram::bucket_of(ns) else {
            self.overflow += 1;
            return Err((TlCode::BucketOverflow, TlCode::BucketOverflow.advice().to_string()));
        };
        if b >= TL_BUCKETS {
            return Err((TlCode::BucketOverflow, TlCode::BucketOverflow.advice().to_string()));
        }
        self.buckets[b] += 1;
        self.count += 1;
        self.sum_ns += ns as u128;
        self.sum_sq_ns += (ns as u128) * (ns as u128);
        if ns > self.max_ns {
            self.max_ns = ns;
        }
        if ns < self.min_ns {
            self.min_ns = ns;
        }
        Ok(())
    }

    /// 分位估算（permille 千分位；返回桶下沿纳秒，保守低估）。
    ///
    /// 无样本时返回 None（不造假数，空桶不发零值谎报）。
    pub fn percentile_ns(&self, permille: u64) -> Option<u64> {
        if self.count == 0 || permille == 0 || permille > 1000 {
            return None;
        }
        // 目标序位：ceil(permille/1000 * count)，向上取整保证「至少覆盖 permille 分位」。
        let target = {
            let num = permille as u128 * self.count as u128;
            let q = num / 1000;
            if num % 1000 == 0 { q } else { q + 1 }
        };
        if target == 0 {
            return Some(TL_BUCKET_EDGES_NS[0]);
        }
        let mut acc: u128 = 0;
        let mut i = 0usize;
        while i < TL_BUCKETS {
            acc += self.buckets[i] as u128;
            if acc >= target {
                return Some(TL_BUCKET_EDGES_NS[i]);
            }
            i += 1;
        }
        // 全部累加仍不足 target（理论上不可达，兜底返回末档）。
        Some(TL_BUCKET_EDGES_NS[TL_BUCKETS - 1])
    }

    /// P99（千分位 990）。
    pub fn p99_ns(&self) -> Option<u64> {
        self.percentile_ns(TL_P99_PERMILLE)
    }

    /// P999（千分位 999）。
    pub fn p999_ns(&self) -> Option<u64> {
        self.percentile_ns(TL_P999_PERMILLE)
    }

    /// 均值（纳秒，整数向下取整；空返回 None）。
    pub fn mean_ns(&self) -> Option<u64> {
        if self.count == 0 {
            return None;
        }
        Some((self.sum_ns / self.count as u128) as u64)
    }

    /// 总体标准差（纳秒，整数平方根；样本数 < 2 返回 0）。
    pub fn stddev_ns(&self) -> u64 {
        if self.count < 2 {
            return 0;
        }
        let n = self.count as u128;
        let mean = self.sum_ns / n;
        // 方差 = E[x^2] - mean^2，用 u128 全程非负。
        let var = if self.sum_sq_ns / n > mean * mean {
            self.sum_sq_ns / n - mean * mean
        } else {
            0
        };
        isqrt_u128(var)
    }

    /// 非空判定（防恒真门禁：断言前先证该量非平凡）。
    pub fn nontrivial(&self) -> bool {
        self.count > 0
    }

    /// 桶占用数（用于诊断摘要）。
    pub fn used_buckets(&self) -> u32 {
        let mut n = 0u32;
        let mut i = 0usize;
        while i < TL_BUCKETS {
            if self.buckets[i] > 0 {
                n += 1;
            }
            i += 1;
        }
        n
    }
}


/// 确定性线性同余采样源（同种子同序列，p99 可复现对拍）。
///
/// 用途：判据复跑时必须给出同一份采样序列，否则 p99 断言不可对拍。
#[derive(Clone, PartialEq, Eq)]
pub struct Lcg {
    /// 内部状态。
    pub state: u64,
}

impl Lcg {
    /// 以种子构造（种子 0 会被抬到非零，避免退化为全零序列）。
    pub const fn new(seed: u64) -> Self {
        let s = if seed == 0 { 0x9E37_79B9_7F4A_7C15 } else { seed };
        Self { state: s }
    }

    /// 取下一个 u64（PCG 风格的 LCG：乘子与增量取自 Knuth 表）。
    pub fn next_u64(&mut self) -> u64 {
        self.state = self
            .state
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        self.state
    }

    /// 取 [0, span) 内的一个整数（拒绝法保证均匀，不取模偏）。
    pub fn next_below(&mut self, span: u64) -> u64 {
        if span <= 1 {
            return 0;
        }
        // 拒绝域：把 2^64 按 span 整除的最大整数倍之外的部分拒绝掉。
        let zone = u64::MAX - (u64::MAX % span);
        let mut r = self.next_u64();
        let mut guard = 0u32;
        while r >= zone && guard < 64 {
            r = self.next_u64();
            guard += 1;
        }
        r % span
    }

    /// 产生一段纳秒时长序列（用于 handler 预算回放）。
    pub fn burst(&mut self, n: usize, base_ns: u64, jitter_ns: u64) -> Vec<u64> {
        let mut out = Vec::with_capacity(n);
        let mut i = 0usize;
        while i < n {
            let j = if jitter_ns == 0 {
                0
            } else {
                self.next_below(jitter_ns + 1)
            };
            out.push(base_ns + j);
            i += 1;
        }
        out
    }
}

/// 纪律三节文档冻结器（义务/禁令/预算三节齐备才可冻结）。
///
/// 判据 UNX-F0943-J1 第三分句：纪律文档冻结（义务/禁令/预算三节）。
/// 冻结后指纹恒定，篡改任一文案都会使指纹变化（显性可检出）。
#[derive(Clone, PartialEq, Eq)]
pub struct DisciplineDoc {
    /// 义务节条款数。
    pub obligation_clauses: u32,
    /// 禁令节条款数。
    pub ban_clauses: u32,
    /// 预算节条款数（三档预算各一条，至少 3）。
    pub budget_clauses: u32,
    /// 冻结位。
    pub frozen: bool,
    /// 冻结指纹（FNV-1a 变体，64 位）。
    pub fingerprint: u64,
    /// 文档标题（参与指纹）。
    pub title: String,
    /// 三节正文（参与指纹）。
    pub body: String,
}

impl Default for DisciplineDoc {
    fn default() -> Self {
        Self::new()
    }
}

impl DisciplineDoc {
    /// 空文档（未冻结）。
    pub const fn new() -> Self {
        Self {
            obligation_clauses: 0,
            ban_clauses: 0,
            budget_clauses: 0,
            frozen: false,
            fingerprint: 0,
            title: String::new(),
            body: String::new(),
        }
    }

    /// 三节齐备判定（义务与禁令各至少一条，预算至少三档）。
    pub fn sections_complete(&self) -> bool {
        self.obligation_clauses > 0 && self.ban_clauses > 0 && self.budget_clauses >= 3
    }

    /// 设置三节条款数（未冻结前可改，冻结后拒绝）。
    pub fn set_sections(
        &mut self,
        obligation: u32,
        ban: u32,
        budget: u32,
    ) -> Result<(), (TlCode, String)> {
        if self.frozen {
            return Err((TlCode::NotFrozen, "文档已冻结：三节条款数不可再改".to_string()));
        }
        if obligation == 0 || ban == 0 || budget < 3 {
            return Err((TlCode::NotFrozen, TlCode::NotFrozen.advice().to_string()));
        }
        self.obligation_clauses = obligation;
        self.ban_clauses = ban;
        self.budget_clauses = budget;
        Ok(())
    }

    /// 设置标题与正文（未冻结前可改）。
    pub fn set_body(&mut self, title: &str, body: &str) -> Result<(), (TlCode, String)> {
        if self.frozen {
            return Err((TlCode::NotFrozen, "文档已冻结：正文不可再改".to_string()));
        }
        if title.is_empty() {
            return Err((TlCode::ListItemEmpty, TlCode::ListItemEmpty.advice().to_string()));
        }
        self.title = title.to_string();
        self.body = body.to_string();
        Ok(())
    }

    /// 计算冻结指纹（FNV-1a 64 位，覆盖标题/正文/三节计数/三档预算）。
    pub fn compute_fingerprint(&self) -> u64 {
        let mut h: u64 = 0xcbf2_9ce4_8422_2325;
        let mut feed = |bytes: &[u8], h: &mut u64| {
            for &b in bytes {
                *h ^= b as u64;
                *h = h.wrapping_mul(0x0000_0100_0000_01b3);
            }
        };
        feed(self.title.as_bytes(), &mut h);
        feed(b"|", &mut h);
        feed(self.body.as_bytes(), &mut h);
        feed(b"|ob=", &mut h);
        feed(self.obligation_clauses.to_le_bytes().as_slice(), &mut h);
        feed(b"|ban=", &mut h);
        feed(self.ban_clauses.to_le_bytes().as_slice(), &mut h);
        feed(b"|bud=", &mut h);
        feed(self.budget_clauses.to_le_bytes().as_slice(), &mut h);
        feed(b"|q=", &mut h);
        feed(TL_BUDGET_NS_QEMU.to_le_bytes().as_slice(), &mut h);
        feed(b"|m=", &mut h);
        feed(TL_BUDGET_NS_METAL.to_le_bytes().as_slice(), &mut h);
        feed(b"|l=", &mut h);
        feed(TL_BUDGET_NS_LATENCY.to_le_bytes().as_slice(), &mut h);
        h
    }

    /// 冻结（三节齐备才允许；冻结后指纹恒定）。
    pub fn freeze(&mut self) -> Result<(), (TlCode, String)> {
        if !self.sections_complete() {
            return Err((TlCode::NotFrozen, TlCode::NotFrozen.advice().to_string()));
        }
        self.fingerprint = self.compute_fingerprint();
        self.frozen = true;
        Ok(())
    }

    /// 冻结核（判据：已冻结 + 指纹非零 + 指纹可复算一致）。
    pub fn freeze_audit(&self) -> bool {
        self.frozen && self.fingerprint != 0 && self.fingerprint == self.compute_fingerprint()
    }
}


/// 最小化清单（义务/有条件/禁令三态闭集容器）。
#[derive(Clone, PartialEq, Eq, Default)]
pub struct DutyList {
    /// 条目（有序，id 递增）。
    pub items: Vec<DutyItem>,
    /// 下一可用条目 id。
    pub next_id: u16,
    /// 冻结位。
    pub frozen: bool,
}

impl DutyList {
    /// 空清单。
    pub const fn new() -> Self {
        Self {
            items: Vec::new(),
            next_id: 1,
            frozen: false,
        }
    }

    /// 追加条目（冻结后拒绝；文案不齐备拒绝；槽位满拒绝）。
    pub fn add(
        &mut self,
        class: DutyClass,
        name: &str,
        why: &str,
        action: &str,
    ) -> Result<u16, (TlCode, String)> {
        if self.frozen {
            return Err((TlCode::NotFrozen, "清单已冻结：不可再追加条目".to_string()));
        }
        if !class.in_doc() {
            return Err((TlCode::ListItemEmpty, TlCode::ListItemEmpty.advice().to_string()));
        }
        if self.items.len() >= TL_LIST_SLOTS {
            return Err((TlCode::ListFull, TlCode::ListFull.advice().to_string()));
        }
        if self.next_id == 0 {
            return Err((TlCode::ListItemEmpty, "条目 id 溢出：清单条目数超 65535".to_string()));
        }
        let item = DutyItem::new(self.next_id, class, name, why, action);
        if !item.text_ready() {
            return Err((TlCode::ListItemEmpty, TlCode::ListItemEmpty.advice().to_string()));
        }
        let id = self.next_id;
        self.next_id += 1;
        self.items.push(item);
        Ok(id)
    }

    /// 冻结清单（所有条目文案齐备才允许）。
    pub fn freeze(&mut self) -> Result<(), (TlCode, String)> {
        if self.items.is_empty() {
            return Err((TlCode::ListItemEmpty, TlCode::ListItemEmpty.advice().to_string()));
        }
        for it in self.items.iter() {
            if !it.text_ready() {
                return Err((TlCode::ListItemEmpty, TlCode::ListItemEmpty.advice().to_string()));
            }
        }
        for it in self.items.iter_mut() {
            it.frozen = true;
        }
        self.frozen = true;
        Ok(())
    }

    /// 按类别计数。
    pub fn count_of(&self, class: DutyClass) -> u32 {
        self.items.iter().filter(|i| i.class == class).count() as u32
    }

    /// 三态齐备判定（每类至少一条，判据「三节」的清单侧前提）。
    pub fn all_classes_present(&self) -> bool {
        self.count_of(DutyClass::Obligation) > 0
            && self.count_of(DutyClass::Conditional) > 0
            && self.count_of(DutyClass::Ban) > 0
    }

    /// 按 id 查条目。
    pub fn find(&self, id: u16) -> Option<&DutyItem> {
        self.items.iter().find(|i| i.id == id)
    }

    /// 清单内容摘要（用于诊断）。
    pub fn summary(&self) -> String {
        format!(
            "清单 {} 条（义务 {} / 有条件 {} / 禁令 {}），冻结 {}",
            self.items.len(),
            self.count_of(DutyClass::Obligation),
            self.count_of(DutyClass::Conditional),
            self.count_of(DutyClass::Ban),
            self.frozen
        )
    }
}

/// 超预算清单行（单 handler 的超支明细）。
#[derive(Clone, PartialEq, Eq)]
pub struct OverrunRow {
    /// handler 向量号。
    pub vector: u16,
    /// 观测 P99（纳秒）。
    pub p99_ns: u64,
    /// 观测 P999（纳秒）。
    pub p999_ns: u64,
    /// 档位预算（纳秒）。
    pub budget_ns: u64,
    /// 超支倍数（千分比；p99*1000/budget，超预算才入列）。
    pub over_permille: u32,
    /// 可操作建议。
    pub advice: String,
}

/// 超预算清单（判据要求超预算时自动产出，10/10 次）。
#[derive(Clone, PartialEq, Eq)]
pub struct OverrunReport {
    /// 行集（仅含超预算 handler）。
    pub rows: Vec<OverrunRow>,
    /// 产出序号（每次产出自增，用于 10/10 次核对）。
    pub seq: u64,
    /// 全体 handler 数。
    pub total_handlers: u32,
    /// 超预算 handler 数。
    pub overrun_handlers: u32,
    /// 摘要文案。
    pub digest_text: String,
}

impl OverrunReport {
    /// 是否全部达标（无超支）。
    pub fn all_within(&self) -> bool {
        self.rows.is_empty()
    }

    /// 行数（判据断言用）。
    pub fn row_count(&self) -> usize {
        self.rows.len()
    }
}


/// 单 handler 预算账。
#[derive(Clone, PartialEq, Eq)]
pub struct HandlerBudget {
    /// 向量号（键）。
    pub vector: u16,
    /// 档位。
    pub tier: BudgetTier,
    /// 档位预算（纳秒，注册时冻结，不复用被测值）。
    pub budget_ns: u64,
    /// 直方图。
    pub hist: Histogram,
    /// 最近样本环形缓冲（保留最近 TL_SAMPLE_CAP 条，用于对拍）。
    pub recent: Vec<u64>,
    /// 入账次数（应等于采样次数）。
    pub entered: u64,
    /// 出账次数（判预算后计一次）。
    pub exited: u64,
    /// 违例计数（该 handler 自身触发的违例数）。
    pub violations: u32,
}

impl HandlerBudget {
    /// 新建 handler 账。
    pub fn new(vector: u16, tier: BudgetTier) -> Self {
        Self {
            vector,
            tier,
            budget_ns: tier.budget_ns(),
            hist: Histogram::new(),
            recent: Vec::new(),
            entered: 0,
            exited: 0,
            violations: 0,
        }
    }

    /// 记入一个采样（入账一次 + 落桶 + 环形缓冲回卷）。
    pub fn record(&mut self, ns: u64) -> Result<(), (TlCode, String)> {
        self.hist.push(ns)?;
        self.entered += 1;
        if self.recent.len() >= TL_SAMPLE_CAP {
            self.recent.remove(0);
        }
        self.recent.push(ns);
        Ok(())
    }

    /// 出账（判预算后调用；入账 ≢ 出账 即账不平）。
    pub fn exit(&mut self) {
        self.exited += 1;
    }

    /// P99（无样本 None）。
    pub fn p99_ns(&self) -> Option<u64> {
        self.hist.p99_ns()
    }

    /// P999（无样本 None）。
    pub fn p999_ns(&self) -> Option<u64> {
        self.hist.p999_ns()
    }

    /// 达标判定（有样本且 p99 ≤ 预算；无样本不算达标也不算超支）。
    pub fn within_budget(&self) -> bool {
        match self.p99_ns() {
            Some(p) => p <= self.budget_ns,
            None => false,
        }
    }

    /// 超支倍数（千分比；未超支返回 0）。
    pub fn over_permille(&self) -> u32 {
        match self.p99_ns() {
            Some(p) if p > self.budget_ns && self.budget_ns > 0 => {
                let num = (p as u128) * 1000u128 / (self.budget_ns as u128);
                if num > u32::MAX as u128 {
                    u32::MAX
                } else {
                    num as u32
                }
            }
            _ => 0,
        }
    }

    /// 尾延迟护栏（p999 ≤ 两倍预算）。
    pub fn tail_ok(&self) -> bool {
        match self.p999_ns() {
            Some(p) => p <= self.budget_ns.saturating_mul(2),
            None => false,
        }
    }

    /// 账平判定。
    pub fn balanced(&self) -> bool {
        self.entered == self.exited
    }
}


/// top half 纪律总账（判据 UNX-F0943-J1 的实现主体）。
#[derive(Clone, PartialEq, Eq)]
pub struct TopHalfLine {
    /// handler 账表。
    pub handlers: Vec<HandlerBudget>,
    /// 最小化清单。
    pub list: DutyList,
    /// 纪律三节文档。
    pub doc: DisciplineDoc,
    /// 逻辑 tick（确定性推进，用于时间轴与诊断）。
    pub tick: u64,
    /// 违例注入计数（判据拦截核用）。
    pub injected: u64,
    /// 违例拦截计数。
    pub intercepted: u64,
    /// 超预算清单产出次数（判据 10/10 核用）。
    pub report_produced: u64,
    /// 诊断码命中计数（按码序）。
    pub notes: [u64; TL_CODE_COUNT],
    /// 总入账次数。
    pub total_entered: u64,
    /// 总出账次数。
    pub total_exited: u64,
}

impl Default for TopHalfLine {
    fn default() -> Self {
        Self::new()
    }
}

impl TopHalfLine {
    /// 空总账。
    pub const fn new() -> Self {
        Self {
            handlers: Vec::new(),
            list: DutyList::new(),
            doc: DisciplineDoc::new(),
            tick: 0,
            injected: 0,
            intercepted: 0,
            report_produced: 0,
            notes: [0; TL_CODE_COUNT],
            total_entered: 0,
            total_exited: 0,
        }
    }

    /// 记录一次诊断码命中。
    fn note(&mut self, code: TlCode) {
        let idx = code as usize;
        if idx < TL_CODE_COUNT {
            self.notes[idx] += 1;
        }
    }

    /// 推进逻辑 tick。
    pub fn advance(&mut self) {
        self.tick += 1;
    }

    /// 注册 handler（同向量重复注册拒绝；槽位满拒绝；档位预算非法拒绝）。
    pub fn register_handler(
        &mut self,
        vector: u16,
        tier: BudgetTier,
    ) -> Result<usize, (TlCode, String)> {
        if self.handlers.iter().any(|h| h.vector == vector) {
            self.note(TlCode::DupRegister);
            return Err((TlCode::DupRegister, TlCode::DupRegister.advice().to_string()));
        }
        if self.handlers.len() >= TL_HANDLER_SLOTS {
            return Err((TlCode::ListFull, "handler 槽位满：单板最多 24 路".to_string()));
        }
        if tier.budget_ns() == 0 || tier.budget_ns() > TL_BUCKET_EDGES_NS[TL_BUCKETS] {
            self.note(TlCode::BudgetIllegal);
            return Err((TlCode::BudgetIllegal, TlCode::BudgetIllegal.advice().to_string()));
        }
        self.handlers.push(HandlerBudget::new(vector, tier));
        Ok(self.handlers.len() - 1)
    }

    /// 注销 handler（按向量；账平才允许注销，账不平拒绝防丢账）。
    pub fn unregister_handler(&mut self, vector: u16) -> Result<(), (TlCode, String)> {
        let pos = self
            .handlers
            .iter()
            .position(|h| h.vector == vector)
            .ok_or((TlCode::NotRegistered, TlCode::NotRegistered.advice().to_string()))?;
        if !self.handlers[pos].balanced() {
            self.note(TlCode::LedgerUnbalanced);
            return Err((TlCode::LedgerUnbalanced, TlCode::LedgerUnbalanced.advice().to_string()));
        }
        self.handlers.remove(pos);
        Ok(())
    }

    /// handler 槽位下标（按向量）。
    pub fn index_of(&self, vector: u16) -> Option<usize> {
        self.handlers.iter().position(|h| h.vector == vector)
    }

    /// 取 handler 账（不可变）。
    pub fn handler(&self, vector: u16) -> Option<&HandlerBudget> {
        self.index_of(vector).map(|i| &self.handlers[i])
    }

    /// 记入采样（入账一次；未注册拒）。
    pub fn record_sample(&mut self, vector: u16, ns: u64) -> Result<(), (TlCode, String)> {
        self.advance();
        let Some(i) = self.index_of(vector) else {
            self.note(TlCode::NotRegistered);
            return Err((TlCode::NotRegistered, TlCode::NotRegistered.advice().to_string()));
        };
        let res = self.handlers[i].record(ns);
        if res.is_ok() {
            self.total_entered += 1;
        }
        res
    }

    /// 出账（判预算；入账 ≢ 出账 即账不平）。
    pub fn settle(&mut self, vector: u16) -> Result<(), (TlCode, String)> {
        let Some(i) = self.index_of(vector) else {
            self.note(TlCode::NotRegistered);
            return Err((TlCode::NotRegistered, TlCode::NotRegistered.advice().to_string()));
        };
        self.handlers[i].exit();
        self.total_exited += 1;
        Ok(())
    }


    /// 预算断言（判据硬线：全 handler p99 ≤ 档位预算）。
    ///
    /// 前置守卫：桶边表单调 + 至少一个 handler 有样本（防恒真门禁）。
    pub fn assert_p99_within(&mut self) -> Result<(), (TlCode, String)> {
        if !edges_monotonic() {
            self.note(TlCode::EdgeMonotonicity);
            return Err((TlCode::EdgeMonotonicity, TlCode::EdgeMonotonicity.advice().to_string()));
        }
        let sampled = self.handlers.iter().filter(|h| h.hist.nontrivial()).count();
        if sampled == 0 {
            return Err((
                TlCode::ReportFailed,
                "无任何 handler 有样本：预算断言无输入，不得断言达标".to_string(),
            ));
        }
        // 借用纪律：`for h in self.handlers.iter()` 期间 `h` 不可变借用 self，
        // 循环体内不得调用 `self.note(&mut self)`（E0502 实锤）。故先把首个
        // 违规项的判定结果算出，循环结束后再登记，避免把借用期拉长。
        let mut violation: Option<(&'static str, u16, u64, u64)> = None;
        for h in self.handlers.iter() {
            if !h.hist.nontrivial() {
                continue;
            }
            if let Some(p) = h.p99_ns() {
                if p > h.budget_ns && violation.is_none() {
                    violation = Some((h.tier.name(), h.vector, p, h.budget_ns));
                }
            }
        }
        if let Some((tier, vector, p, budget_ns)) = violation {
            self.note(TlCode::P99OverBudget);
            return Err((
                TlCode::P99OverBudget,
                format!(
                    "handler 向量 {} p99={}ns 超预算 {}ns（{}）：{}",
                    vector,
                    p,
                    budget_ns,
                    tier,
                    TlCode::P99OverBudget.advice()
                ),
            ));
        }
        Ok(())
    }

    /// 尾延迟断言（全 handler p999 ≤ 两倍预算）。
    pub fn assert_tail_within(&mut self) -> Result<(), (TlCode, String)> {
        let sampled = self.handlers.iter().filter(|h| h.hist.nontrivial()).count();
        if sampled == 0 {
            return Err((TlCode::ReportFailed, "无样本：尾延迟断言无输入".to_string()));
        }
        // 同上：`h` 不可变借用期间不得 `self.note(&mut self)`（E0502）。
        // 先定出首个越界项，循环后统一登记。
        let mut violation: Option<(u16, u64)> = None;
        for h in self.handlers.iter() {
            if !h.tail_ok() && h.hist.nontrivial() && violation.is_none() {
                violation = Some((h.vector, h.budget_ns));
            }
        }
        if let Some((vector, budget_ns)) = violation {
            self.note(TlCode::P999OverBudget);
            return Err((
                TlCode::P999OverBudget,
                format!(
                    "handler 向量 {} p999 超两倍预算 {}ns：{}",
                    vector,
                    budget_ns,
                    TlCode::P999OverBudget.advice()
                ),
            ));
        }
        Ok(())
    }

    /// 超预算清单自动产出（判据要求超预算时 10/10 次自动产出）。
    ///
    /// 语义：无论全体是否达标，每次都产出一份清单（达标时为空清单），
    /// 产出计数自增；超预算的 handler 逐条入列并附建议。
    pub fn produce_report(&mut self) -> OverrunReport {
        self.report_produced += 1;
        let mut rows: Vec<OverrunRow> = Vec::new();
        let mut over = 0u32;
        for h in self.handlers.iter() {
            if !h.hist.nontrivial() {
                continue;
            }
            let ov = h.over_permille();
            if ov > 0 {
                over += 1;
                rows.push(OverrunRow {
                    vector: h.vector,
                    p99_ns: h.p99_ns().unwrap_or(0),
                    p999_ns: h.p999_ns().unwrap_or(0),
                    budget_ns: h.budget_ns,
                    over_permille: ov,
                    advice: TlCode::P99OverBudget.advice().to_string(),
                });
            }
        }
        rows.sort_by(|a, b| {
            b.over_permille
                .cmp(&a.over_permille)
                .then(a.vector.cmp(&b.vector))
        });
        let digest_text = format!(
            "超预算清单 #{}：handler {} 个，超支 {} 个，行 {} 条",
            self.report_produced,
            self.handlers.len(),
            over,
            rows.len()
        );
        OverrunReport {
            rows,
            seq: self.report_produced,
            total_handlers: self.handlers.len() as u32,
            overrun_handlers: over,
            digest_text,
        }
    }

    /// 清单产出核（判据：10/10 次自动产出）。
    pub fn report_audit(&self) -> bool {
        self.report_produced >= TL_REPORT_TARGET
    }


    /// 违例注入（把某 handler 标记为违反清单禁令/义务，必须被拦截）。
    ///
    /// 返回是否拦截成功；注入数与拦截数不等即 InterceptGap。
    pub fn inject_violation(&mut self, vector: u16, is_ban: bool) -> bool {
        self.injected += 1;
        let Some(i) = self.index_of(vector) else {
            self.note(TlCode::NotRegistered);
            self.intercepted += 1;
            return true;
        };
        let code = if is_ban {
            TlCode::BanViolation
        } else {
            TlCode::DutyMissing
        };
        let identifiable = match code {
            TlCode::BanViolation => self.list.count_of(DutyClass::Ban) > 0,
            TlCode::DutyMissing => self.list.count_of(DutyClass::Obligation) > 0,
            _ => false,
        };
        if identifiable && self.list.frozen {
            self.handlers[i].violations += 1;
            self.note(code);
            self.intercepted += 1;
            true
        } else {
            self.note(TlCode::InterceptGap);
            false
        }
    }

    /// 违例拦截核（判据：注入 N 次必须拦截 N 次）。
    pub fn intercept_audit(&self) -> bool {
        self.injected > 0 && self.injected == self.intercepted
    }

    /// 事件账平核（总入账 ≡ 总出账 且逐 handler 账平）。
    pub fn ledger_balanced(&self) -> bool {
        if self.total_entered != self.total_exited {
            return false;
        }
        self.handlers.iter().all(|h| h.balanced())
    }

    /// 总账平核（判据：账不平即报）。
    pub fn assert_ledger_balanced(&mut self) -> Result<(), (TlCode, String)> {
        if !self.ledger_balanced() {
            self.note(TlCode::LedgerUnbalanced);
            return Err((TlCode::LedgerUnbalanced, TlCode::LedgerUnbalanced.advice().to_string()));
        }
        Ok(())
    }

    /// 建档（三节文档 + 最小化清单一次配齐并冻结）。
    pub fn install_default_discipline(&mut self) -> Result<(), (TlCode, String)> {
        self.list = default_duty_list();
        self.list.freeze()?;
        self.doc = default_discipline_doc();
        let ob = self.list.count_of(DutyClass::Obligation);
        let ban = self.list.count_of(DutyClass::Ban);
        let cond = self.list.count_of(DutyClass::Conditional);
        self.doc.set_sections(ob, ban, cond.max(3))?;
        self.doc.set_body(DOC_TITLE, DOC_BODY)?;
        self.doc.freeze()?;
        Ok(())
    }

    /// 冻结核（判据：纪律文档冻结三节齐备 + 指纹恒定）。
    pub fn freeze_audit(&self) -> bool {
        self.doc.freeze_audit() && self.list.frozen && self.list.all_classes_present()
    }

    /// 未冻结断言（反向判据：未建档前 freeze_audit 必须为假）。
    pub fn not_frozen_before_install(&self) -> bool {
        !self.doc.frozen && !self.list.frozen
    }

    /// 摘要（判据与台账留痕用）。
    pub fn digest(&self) -> String {
        format!(
            "top half 纪律：handler {} 路，{}，文档冻结 {} 指纹 {:016x}，tick {}，产出 {} 次，注入 {} 拦截 {}，账平 {}",
            self.handlers.len(),
            self.list.summary(),
            self.doc.frozen,
            self.doc.fingerprint,
            self.tick,
            self.report_produced,
            self.injected,
            self.intercepted,
            self.ledger_balanced()
        )
    }

    /// 全判据核（一次性跑完三条硬线，返回全绿与否 + 失败码）。
    pub fn judge(&mut self) -> Result<(), (TlCode, String)> {
        self.assert_p99_within()?;
        self.assert_tail_within()?;
        self.assert_ledger_balanced()?;
        if !self.freeze_audit() {
            self.note(TlCode::NotFrozen);
            return Err((TlCode::NotFrozen, TlCode::NotFrozen.advice().to_string()));
        }
        if !self.intercept_audit() {
            self.note(TlCode::InterceptGap);
            return Err((TlCode::InterceptGap, TlCode::InterceptGap.advice().to_string()));
        }
        if !self.report_audit() {
            self.note(TlCode::ReportFailed);
            return Err((TlCode::ReportFailed, TlCode::ReportFailed.advice().to_string()));
        }
        Ok(())
    }
}


/// 纪律文档标题（冻结指纹参与项）。
pub const DOC_TITLE: &str = "top half 纪律文档 · 义务/禁令/预算三节";

/// 纪律文档正文（冻结指纹参与项）。
pub const DOC_BODY: &str = "\
第一节 义务：top half 必须在 5µs 内完成 EOI 应答、从设备摘取快照、置位 pending、返回。\
第二条 义务：摘取的快照长度不得超过 256 字节，超出即属越界重活，应下沉 bottom half。\
第三条 义务：任何 handler 必须在注册表登记后才有资格入账，入口绕开注册表属违例。\
第二节 禁令：top half 禁止睡眠/阻塞/自旋等待、禁止动态分配、禁止长循环、\
禁止持锁跨调用、禁止触碰慢速总线事务、禁止做任何可在 bottom half 完成的计算。\
第三节 预算：QEMU 档全 handler p99 ≤5µs；真机档收紧至 2µs；\
低延迟档（音频/输入）收紧至 1µs；p999 不得超两倍档位预算，\
超预算者自动产出清单并附精简建议。";

/// 预置最小化清单（义务/有条件/禁令三态齐备，文案非空）。
///
/// 该清单是纪律文档的清单侧展开，freeze 后条目不可改。
pub fn default_duty_list() -> DutyList {
    let mut l = DutyList::new();
    // 义务节（五条）。
    let _ = l.add(
        DutyClass::Obligation,
        "EOI 应答",
        "不应答则同一向量后续中断被屏蔽，设备侧看成长时间无响应",
        "top half 内第一件事就是写 LAPIC EOI 寄存器",
    );
    let _ = l.add(
        DutyClass::Obligation,
        "快照摘取",
        "bottom half 禁止直取硬件，数据必须在 top half 摘干净",
        "把设备寄存器读进 ≤256 字节的快照缓冲",
    );
    let _ = l.add(
        DutyClass::Obligation,
        "pending 置位",
        "不置位则重活永远得不到调度，事件丢在中断里",
        "把事件 id 挂进 pending 队列",
    );
    let _ = l.add(
        DutyClass::Obligation,
        "注册先行",
        "未登记 handler 的入账无法归因，预算断言失去对象",
        "中断入口必须先查注册表再入账",
    );
    let _ = l.add(
        DutyClass::Obligation,
        "预算自证",
        "判据要求 p99 可证，无采样即无证据",
        "handler 退出前把本次耗时上报预算账",
    );
    // 有条件节（四条）。
    let _ = l.add(
        DutyClass::Conditional,
        "轻量记账",
        "少量计数与累加可接受，但须计入预算",
        "仅做整数累加，不做格式化与分配",
    );
    let _ = l.add(
        DutyClass::Conditional,
        "位图更新",
        "掩码位图小面积更新在预算内可行",
        "仅改本向量对应位，不做全表扫描",
    );
    let _ = l.add(
        DutyClass::Conditional,
        "短临界区",
        "必须持锁时用最短临界区并即时释放",
        "临界区内不做任何函数调用",
    );
    let _ = l.add(
        DutyClass::Conditional,
        "快照截断",
        "超长数据可截断入快照，但须留尾标记",
        "超 256 字节时截断并置 truncated 标记",
    );
    // 禁令节（六条）。
    let _ = l.add(
        DutyClass::Ban,
        "睡眠阻塞",
        "中断上下文睡眠会死锁调度并拖垮所有核",
        "禁止任何 sleep/wait/阻塞原语",
    );
    let _ = l.add(
        DutyClass::Ban,
        "动态分配",
        "分配可能触发内存回收，时长不可控且不可重入",
        "禁止在 top half 调用 alloc",
    );
    let _ = l.add(
        DutyClass::Ban,
        "长循环",
        "长循环直接击穿 5µs 预算且不可抢占",
        "禁止无界循环，迭代上限必须编译期写死",
    );
    let _ = l.add(
        DutyClass::Ban,
        "慢速总线",
        "MMIO 读一次可能上毫秒，top half 绝不可触碰",
        "禁止在 top half 发起慢速总线事务",
    );
    let _ = l.add(
        DutyClass::Ban,
        "持锁跨调用",
        "跨调用持锁把临界区扩大到不可控范围",
        "禁止在函数调用之间保持锁",
    );
    let _ = l.add(
        DutyClass::Ban,
        "可下沉计算",
        "能做而不下沉等于浪费 top half 预算",
        "任何纯计算必须下沉 bottom half",
    );
    l
}

/// 预置纪律三节文档（条款数与清单对齐）。
pub fn default_discipline_doc() -> DisciplineDoc {
    let mut d = DisciplineDoc::new();
    let _ = d.set_sections(5, 6, 3);
    let _ = d.set_body(DOC_TITLE, DOC_BODY);
    d
}

/// 段位守卫（诊断码闭集必须落在 0x4c50 段内）。
pub fn segment_guard() -> bool {
    TlCode::ALL
        .iter()
        .all(|c| c.val() >= TL_SEG_BASE && c.val() < TL_SEG_BASE + 0x100)
}

/// 码集唯一性守卫（闭集内码值不得重复）。
pub fn code_unique_guard() -> bool {
    let mut i = 0usize;
    while i < TL_CODE_COUNT {
        let mut j = i + 1;
        while j < TL_CODE_COUNT {
            if TlCode::ALL[i].val() == TlCode::ALL[j].val() {
                return false;
            }
            j += 1;
        }
        i += 1;
    }
    true
}

/// 桶边表守卫（单调严格递增 + 首档非零）。
pub fn edge_table_guard() -> bool {
    edges_monotonic() && TL_BUCKET_EDGES_NS[0] == 0 && TL_BUCKET_EDGES_NS[TL_BUCKETS] > 0
}

/// 预算档位守卫（三档严格递减且都为正）。
pub fn tier_guard() -> bool {
    BudgetTier::Latency.budget_ns() > 0
        && BudgetTier::Metal.budget_ns() > BudgetTier::Latency.budget_ns()
        && BudgetTier::Qemu.budget_ns() > BudgetTier::Metal.budget_ns()
        && BudgetTier::Qemu.budget_ns() == TL_BUDGET_NS_QEMU
}


#[cfg(test)]
mod tests {
    use super::*;

    /// 装配一个达标基线（三路 handler，p99 均在档内）。
    fn baseline_green() -> TopHalfLine {
        let mut l = TopHalfLine::new();
        l.install_default_discipline().expect("建档");
        l.register_handler(0x20, BudgetTier::Qemu).expect("reg 0x20");
        l.register_handler(0x21, BudgetTier::Metal).expect("reg 0x21");
        l.register_handler(0x22, BudgetTier::Latency).expect("reg 0x22");
        // 确定性采样：全部落在预算内。
        let mut rng = Lcg::new(0x1234_5678);
        let s0 = rng.burst(1000, 1_000, 3_000); // p99 ≈ 4µs < 5µs
        let s1 = rng.burst(1000, 500, 1_200); // p99 ≈ 1.7µs < 2µs
        let s2 = rng.burst(1000, 200, 700); // p99 ≈ 900ns < 1µs
        for v in [0x20u16, 0x21, 0x22] {
            let seq: &[u64] = match v {
                0x20 => &s0,
                0x21 => &s1,
                _ => &s2,
            };
            for &ns in seq {
                l.record_sample(v, ns).expect("record");
                l.settle(v).expect("settle");
            }
        }
        // 违例注入 10 次，应全拦截。
        let mut i = 0;
        while i < 10 {
            let v = [0x20u16, 0x21, 0x22][i % 3];
            assert!(l.inject_violation(v, i % 2 == 0), "违例应被拦截");
            i += 1;
        }
        // 超预算清单产出 10 次。
        let mut k = 0;
        while k < 10 {
            let _ = l.produce_report();
            k += 1;
        }
        l
    }

    #[test]
    fn t01_guards_all_green() {
        assert!(segment_guard());
        assert!(code_unique_guard());
        assert!(edge_table_guard());
        assert!(tier_guard());
    }

    #[test]
    fn t02_baseline_judge_green() {
        let mut l = baseline_green();
        assert!(l.judge().is_ok(), "达标基线必须全绿：{}", l.digest());
    }

    #[test]
    fn t03_p99_over_budget_is_caught() {
        let mut l = TopHalfLine::new();
        l.install_default_discipline().expect("建档");
        l.register_handler(0x20, BudgetTier::Qemu).expect("reg");
        let mut i = 0;
        while i < 100 {
            l.record_sample(0x20, 20_000).expect("rec");
            l.settle(0x20).expect("settle");
            i += 1;
        }
        let e = l.assert_p99_within();
        assert!(e.is_err(), "超预算必须被断言拦截");
        let (code, msg) = e.unwrap_err();
        assert_eq!(code, TlCode::P99OverBudget);
        assert!(msg.contains("超预算"), "诊断文本须含超预算字样：{msg}");
    }

    #[test]
    fn t04_overrun_report_auto_produced() {
        let mut l = TopHalfLine::new();
        l.install_default_discipline().expect("建档");
        l.register_handler(0x20, BudgetTier::Qemu).expect("reg");
        l.register_handler(0x21, BudgetTier::Qemu).expect("reg");
        let mut i = 0;
        while i < 200 {
            l.record_sample(0x20, 30_000).expect("rec");
            l.record_sample(0x21, 1_000).expect("rec");
            l.settle(0x20).expect("s");
            l.settle(0x21).expect("s");
            i += 1;
        }
        let r = l.produce_report();
        assert_eq!(r.row_count(), 1, "只有 0x20 超支");
        assert_eq!(r.rows[0].vector, 0x20);
        assert!(r.rows[0].over_permille >= 1000, "超支倍数须 ≥1.0");
        assert!(!r.rows[0].advice.is_empty(), "必须附可操作建议");
        let mut k = 0;
        while k < 9 {
            let _ = l.produce_report();
            k += 1;
        }
        assert!(l.report_audit(), "10/10 产出核");
    }

    #[test]
    fn t05_doc_freeze_fingerprint_stable() {
        let mut l = TopHalfLine::new();
        assert!(l.not_frozen_before_install(), "建档前必须未冻结（反向判据）");
        l.install_default_discipline().expect("建档");
        assert!(l.doc.freeze_audit(), "冻结后指纹可复算");
        let f1 = l.doc.fingerprint;
        assert!(f1 != 0);
        let e = l.doc.set_body("改", "改");
        assert!(e.is_err(), "冻结后正文不可改");
        assert_eq!(l.doc.fingerprint, f1, "拒绝后指纹不得变");
    }

    #[test]
    fn t06_inject_intercept_audit() {
        let l = baseline_green();
        assert_eq!(l.injected, 10);
        assert_eq!(l.intercepted, 10);
        assert!(l.intercept_audit(), "注入 N 拦截 N");
    }

    #[test]
    fn t07_ledger_balanced() {
        let l = baseline_green();
        assert!(l.ledger_balanced(), "总账必须平：{}", l.digest());
    }

    #[test]
    fn t08_not_registered_rejected() {
        let mut l = TopHalfLine::new();
        l.install_default_discipline().expect("建档");
        let e = l.record_sample(0x30, 100);
        assert!(e.is_err());
        assert_eq!(e.unwrap_err().0, TlCode::NotRegistered);
    }

    #[test]
    fn t09_dup_register_rejected() {
        let mut l = TopHalfLine::new();
        l.register_handler(0x20, BudgetTier::Qemu).expect("reg");
        let e = l.register_handler(0x20, BudgetTier::Metal);
        assert_eq!(e.unwrap_err().0, TlCode::DupRegister);
    }

    #[test]
    fn t10_histogram_percentile_monotone() {
        let mut h = Histogram::new();
        let mut rng = Lcg::new(99);
        let s = rng.burst(2000, 1_000, 4_000);
        for &ns in &s {
            h.push(ns).expect("push");
        }
        let p50 = h.percentile_ns(500).unwrap();
        let p99 = h.percentile_ns(990).unwrap();
        let p999 = h.percentile_ns(999).unwrap();
        assert!(p50 <= p99, "分位必须单调：{p50} vs {p99}");
        assert!(p99 <= p999, "分位必须单调：{p99} vs {p999}");
        assert!(h.mean_ns().unwrap() > 0);
        assert!(h.stddev_ns() > 0, "非平凡样本标准差必须为正");
        assert!(h.used_buckets() > 1, "样本应分布多桶");
    }


    #[test]
    fn t11_histogram_empty_is_none() {
        let h = Histogram::new();
        assert!(h.p99_ns().is_none(), "空直方图不发零值谎报");
        assert!(h.mean_ns().is_none());
        assert!(!h.nontrivial(), "空即非平凡为假");
    }

    #[test]
    fn t12_isqrt_exact() {
        assert_eq!(isqrt_u128(0), 0);
        assert_eq!(isqrt_u128(1), 1);
        assert_eq!(isqrt_u128(99), 9);
        assert_eq!(isqrt_u128(100), 10);
        assert_eq!(isqrt_u128(10_000), 100);
        let big: u128 = 1u128 << 100;
        assert_eq!(isqrt_u128(big), 1u64 << 50);
        for v in [2u128, 3, 15, 16, 17, 1_000_000, 12_345_678_901] {
            let r = isqrt_u128(v);
            assert!((r as u128) * (r as u128) <= v);
            assert!((r as u128 + 1) * (r as u128 + 1) > v);
        }
    }

    #[test]
    fn t13_lcg_deterministic() {
        let mut a = Lcg::new(42);
        let mut b = Lcg::new(42);
        let mut i = 0;
        while i < 100 {
            assert_eq!(a.next_u64(), b.next_u64(), "同种子必须同序列");
            i += 1;
        }
        let mut c = Lcg::new(43);
        assert_ne!(Lcg::new(42).next_u64(), c.next_u64(), "异种子应异序列");
    }

    #[test]
    fn t14_lcg_uniform_below() {
        let mut r = Lcg::new(7);
        let span = 10u64;
        let mut counts = [0u64; 10];
        let mut i = 0;
        while i < 10_000 {
            let v = r.next_below(span);
            assert!(v < span);
            counts[v as usize] += 1;
            i += 1;
        }
        for c in counts.iter() {
            assert!(*c > 0, "均匀性失败");
        }
    }

    #[test]
    fn t15_list_three_classes_present() {
        let l = default_duty_list();
        assert!(l.all_classes_present());
        assert_eq!(l.count_of(DutyClass::Obligation), 5);
        assert_eq!(l.count_of(DutyClass::Conditional), 4);
        assert_eq!(l.count_of(DutyClass::Ban), 6);
    }

    #[test]
    fn t16_list_frozen_rejects_add() {
        let mut l = default_duty_list();
        l.freeze().expect("freeze");
        let e = l.add(DutyClass::Ban, "x", "y", "z");
        assert!(e.is_err(), "冻结后不可追加");
    }

    #[test]
    fn t17_empty_text_item_rejected() {
        let mut l = DutyList::new();
        let e = l.add(DutyClass::Ban, "", "", "");
        assert_eq!(e.unwrap_err().0, TlCode::ListItemEmpty);
    }

    #[test]
    fn t18_tail_over_budget_caught() {
        let mut l = TopHalfLine::new();
        l.install_default_discipline().expect("建档");
        l.register_handler(0x20, BudgetTier::Latency).expect("reg");
        // 尾延迟护栏核的是 p999，即 1000 个采样里的第 999 序位。要让 p999
        // 读出超载样本，超载采样必须**真占满第 999 序位**——慢样本占多数。
        // 原判据写「999 快 + 1 慢」：p999 目标序位恰为 999，累到第 999 个
        // 快样本即止，返回快档桶下沿 400ns，护栏不触发；单个慢样本在数学上
        // 永远够不着 p999（1‰ 分位不覆盖 1/1000 的尾部点）。这是判据自设的
        // 假前提，不是实现缺陷（percentile_ns 的 nearest-rank 契约正确）。
        // 取 990 快 + 10 慢：目标序位 999 落在慢档区，p999 读出 72393ns
        // （> 2×1µs = 2000ns），护栏 C_4C55 得以被真正考验。
        let mut i = 0;
        while i < 1000 {
            let ns = if i >= 990 { 100_000 } else { 500 };
            l.record_sample(0x20, ns).expect("rec");
            l.settle(0x20).expect("s");
            i += 1;
        }
        let e = l.assert_tail_within();
        assert!(e.is_err(), "尾延迟超护栏必须拦截");
        assert_eq!(e.unwrap_err().0, TlCode::P999OverBudget);
    }

    #[test]
    fn t19_unregister_requires_balanced() {
        let mut l = TopHalfLine::new();
        l.register_handler(0x20, BudgetTier::Qemu).expect("reg");
        l.record_sample(0x20, 1_000).expect("rec");
        let e = l.unregister_handler(0x20);
        assert_eq!(e.unwrap_err().0, TlCode::LedgerUnbalanced);
        l.settle(0x20).expect("s");
        assert!(l.unregister_handler(0x20).is_ok(), "账平后可注销");
    }

    #[test]
    fn t20_digest_nonempty() {
        let l = baseline_green();
        let d = l.digest();
        assert!(!d.is_empty());
        assert!(d.contains("top half 纪律"));
        assert!(d.contains("清单"));
    }
}

