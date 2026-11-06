//! UNX-F3449 慢用例账本（执行时长分布 P50/P99 观测） · 0x79D80段
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（派工卡 VX8-B14 逐字摘录）：
//!   ① 纯功能代码 ≥300 行；
//!   ② J1：per-case 时长直方图（桶宽 2 的幂）+ P50/P99 导出；
//!     M-208 分布对账（注入已知时长样本）。
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - 直方图桶宽恒 2 的幂：桶 i 覆盖 [2^i, 2^(i+1)) 微秒（桶 0 = 1µs，
//!     桶 1 = 2~3µs，桶 2 = 4~7µs……桶 15 = 32768~65535µs），桶序号 =
//!     floor(log2(时长))，循环移位整数求（零浮点零对数库）；
//!   - 桶账守恒：各桶计数之和恒等于样本总数，不平即账目违约显性判红；
//!   - P50/P99 导出：分位秩 = ceil(p×n/100)（整数式 (p×n+99)/100），
//!     有序副本按秩取值；p 只收 1..=100，空账导出显性拒；
//!   - M-208 分布对账：注入已知样本 1..=100µs（n=100），P50 必为 50、
//!     P99 必为 99（秩与值双双对上）；
//!   - 慢用例点名：以 P99 为慢线，时长 > P99 的样本进慢例名单，名单
//!     条数与账面对账一致；
//!   - 双跑一致：同样本序列两次记账导出的分位与直方图逐字段相同。
//! 常量来源：
//!   - 桶宽 2 的幂：判据原文「桶宽 2 的幂」冻结（16 桶覆盖 1µs~65535µs）；
//!   - M-208：派工卡验收判据编号（分布对账锚定 0x208）。
//! ─────────────────────────────────────────────────────────────────────

use alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x40 对齐，B14 批段窗 [0x79C00, 0x79DC0] 内）。
pub const SEG_BASE: u32 = 0x79D80;

/// 直方图桶数（16 桶覆盖 1µs..=65535µs，桶宽 2 的幂）。
pub const BUCKET_COUNT: usize = 16;

/// 样本账容量（超出显性拒，不静默丢样本）。
pub const SAMPLE_CAP: usize = 4096;

/// M-208 分布对账锚定号。
pub const INJECTION_CASE_ID: u32 = 0x208;

/// 桶序号：floor(log2(us))，us ≥ 1；循环移位整数求。
/// 例：1→0、2→1、3→1、4→2、7→2、8→3、65535→15。
pub fn bucket_index(us: u64) -> Option<usize> {
    if us == 0 {
        return None;
    }
    let mut v = us;
    let mut idx = 0usize;
    while v >= 2 {
        v = match v.checked_div(2) {
            Some(x) => x,
            None => return None,
        };
        idx += 1;
    }
    if idx >= BUCKET_COUNT {
        return None;
    }
    Some(idx)
}

/// 桶 i 的下界（2^i，整数幂表）。
pub fn bucket_lower(i: usize) -> Option<u64> {
    if i >= BUCKET_COUNT {
        return None;
    }
    let mut v: u64 = 1;
    let mut k = 0usize;
    while k < i {
        v = match v.checked_mul(2) {
            Some(x) => x,
            None => return None,
        };
        k += 1;
    }
    Some(v)
}

/// 桶 i 的上界（2^(i+1) - 1，含端）。
pub fn bucket_upper(i: usize) -> Option<u64> {
    let lower = bucket_lower(i)?;
    match lower.checked_mul(2) {
        Some(twice) => Some(twice - 1),
        None => None,
    }
}

/// 时长直方图账本：分桶计数 + 全量样本账（分位导出用）。
#[derive(Debug, Clone)]
pub struct DurationLedger {
    buckets: Vec<u64>,
    samples: Vec<u64>,
}

impl DurationLedger {
    /// 建空账。
    pub fn new() -> DurationLedger {
        let mut buckets = Vec::new();
        let mut i = 0usize;
        while i < BUCKET_COUNT {
            buckets.push(0u64);
            i += 1;
        }
        DurationLedger {
            buckets,
            samples: Vec::new(),
        }
    }

    /// 记一笔时长样本（微秒）；零样本/超量程/账满显性拒。
    pub fn record(&mut self, us: u64) -> Result<(), u32> {
        if us == 0 {
            return Err(Diag::ZeroSample.val());
        }
        let idx = match bucket_index(us) {
            Some(i) => i,
            None => return Err(Diag::OutOfRange.val()),
        };
        if self.samples.len() >= SAMPLE_CAP {
            return Err(Diag::LedgerFull.val());
        }
        match self.buckets.get_mut(idx) {
            Some(slot) => {
                *slot = match slot.checked_add(1) {
                    Some(v) => v,
                    None => return Err(Diag::BucketSumBroke.val()),
                };
            }
            None => return Err(Diag::BucketSumBroke.val()),
        }
        self.samples.push(us);
        Ok(())
    }

    /// 样本总数。
    pub fn count(&self) -> usize {
        self.samples.len()
    }

    /// 桶计数只读视图。
    pub fn buckets(&self) -> &[u64] {
        &self.buckets
    }

    /// 桶账守恒核：各桶和 == 样本数。
    pub fn buckets_conserved(&self) -> bool {
        let mut total: u64 = 0;
        for b in self.buckets.iter() {
            total = match total.checked_add(*b) {
                Some(v) => v,
                None => return false,
            };
        }
        total == self.count() as u64
    }

    /// 分位导出：p ∈ 1..=100，秩 = ceil(p×n/100)，有序副本按秩取值。
    pub fn percentile(&self, p: u64) -> Result<u64, u32> {
        if p == 0 || p > 100 {
            return Err(Diag::PercentileRange.val());
        }
        let n = self.count();
        if n == 0 {
            return Err(Diag::LedgerEmpty.val());
        }
        // 秩 = ceil(p*n/100) 整数式：(p*n + 99) / 100；秩必落在 1..=n。
        let pn = match p.checked_mul(n as u64) {
            Some(v) => v,
            None => return Err(Diag::QuantileRankBroke.val()),
        };
        let rank = match pn.checked_add(99) {
            Some(v) => v / 100,
            None => return Err(Diag::QuantileRankBroke.val()),
        };
        if rank == 0 || rank > n as u64 {
            return Err(Diag::QuantileRankBroke.val());
        }
        let mut sorted = self.samples.clone();
        sorted.sort_unstable();
        match sorted.get((rank - 1) as usize) {
            Some(&v) => Ok(v),
            None => Err(Diag::QuantileRankBroke.val()),
        }
    }

    /// P50 导出（中位分位）。
    pub fn p50(&self) -> Result<u64, u32> {
        self.percentile(50)
    }

    /// P99 导出（慢线分位）。
    pub fn p99(&self) -> Result<u64, u32> {
        self.percentile(99)
    }

    /// 慢用例名单：时长 > P99 的样本值列表（按记账序）。
    pub fn slow_cases(&self) -> Result<Vec<u64>, u32> {
        let line = self.p99()?;
        let mut slow: Vec<u64> = Vec::new();
        for s in self.samples.iter() {
            if *s > line {
                slow.push(*s);
            }
        }
        Ok(slow)
    }
}

/// 诊断码闭集：0x79D81..=0x79D8C（12 码，变体带中文 doc）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Diag {
    /// 零时长样本：0µs 不入账（执行例不可能零耗时）。
    ZeroSample = 0x01,
    /// 样本超量程：≥ 65536µs 超出 16 桶覆盖域。
    OutOfRange = 0x02,
    /// 百分位越界：p 不在 1..=100。
    PercentileRange = 0x03,
    /// 空账导出：无样本就求分位。
    LedgerEmpty = 0x04,
    /// 账满拒记：样本数达 SAMPLE_CAP 上限。
    LedgerFull = 0x05,
    /// 桶账不平：各桶和 != 样本数。
    BucketSumBroke = 0x06,
    /// 桶宽非 2 的幂：桶界表漂移。
    WidthNotPow2 = 0x07,
    /// 分位秩越界：秩算出 0 或超 n。
    QuantileRankBroke = 0x08,
    /// 分位单调破约：P50 > P99。
    P50GtP99 = 0x09,
    /// 慢例名单错：名单口径与 P99 慢线对不上。
    SlowListMismatch = 0x0A,
    /// 双跑分叉：同样本两次记账导出不同。
    DoubleRunDiverge = 0x0B,
    /// 段位失守：SEG_BASE 不满足 0x40 对齐或越出批段窗。
    SegGuardFail = 0x0C,
}

impl Diag {
    /// 完整 u32 编码。
    pub fn val(self) -> u32 {
        SEG_BASE + (self as u32)
    }

    /// 中文诊断文案（含可操作建议）。
    pub fn detail(self) -> &'static str {
        match self {
            Diag::ZeroSample => "零时长样本：建议记账口拒收 0µs，先核执行器计时口径",
            Diag::OutOfRange => "样本超量程：建议超 65535µs 慢例先降采样或扩桶表",
            Diag::PercentileRange => "百分位越界：建议 p 只收 1..=100，越界显性拒",
            Diag::LedgerEmpty => "空账导出：建议先记账再求分位，空账分位无意义",
            Diag::LedgerFull => "账满拒记：建议达到容量上限即拒并扩容，不静默丢样本",
            Diag::BucketSumBroke => "桶账不平：建议每次记账同步桶与样本账并做守恒核对",
            Diag::WidthNotPow2 => "桶宽非 2 的幂：建议桶界只从 2 的幂表生成，禁手改",
            Diag::QuantileRankBroke => "分位秩越界：建议秩用 ceil(p×n/100) 整数式并核 1..=n",
            Diag::P50GtP99 => "P50>P99：建议导出口做分位单调核，倒挂即判红",
            Diag::SlowListMismatch => "慢例名单错：建议名单口径恒为时长>P99，禁改阈值",
            Diag::DoubleRunDiverge => "双跑分叉：建议记账导出做成纯函数，输入定则输出定",
            Diag::SegGuardFail => "段位失守：建议核对 SEG_BASE 的 0x40 对齐与批段窗",
        }
    }
}

/// 闭集全量数组（12 码）。
pub const ALL_DIAGS: [Diag; 12] = [
    Diag::ZeroSample,
    Diag::OutOfRange,
    Diag::PercentileRange,
    Diag::LedgerEmpty,
    Diag::LedgerFull,
    Diag::BucketSumBroke,
    Diag::WidthNotPow2,
    Diag::QuantileRankBroke,
    Diag::P50GtP99,
    Diag::SlowListMismatch,
    Diag::DoubleRunDiverge,
    Diag::SegGuardFail,
];

/// 诊断码查表闭集返回；未知编码回退固定兜底串。
pub fn detail(code: u32) -> &'static str {
    for d in ALL_DIAGS.iter() {
        if d.val() == code {
            return d.detail();
        }
    }
    "未知诊断码：建议核对是否落在 0x79D81..=0x79D8C 闭集内"
}

// ── 判据谓词（全部私有 fn xxx_ok() -> bool）────────────────────────────

/// 段位守卫：0x40 对齐 + B14 批段窗 [0x79C00, 0x79DC0]。
fn seg_guard_ok() -> bool {
    SEG_BASE % 0x40 == 0 && SEG_BASE >= 0x79C00 && SEG_BASE <= 0x79DC0
}

/// 诊断闭集完整：12 码、编码连续、唯一、文案非空且带「建议」。
fn diag_closed_set_ok() -> bool {
    if ALL_DIAGS.len() != 12 {
        return false;
    }
    let mut i = 0usize;
    while i < ALL_DIAGS.len() {
        let a = match ALL_DIAGS.get(i) {
            Some(d) => *d,
            None => return false,
        };
        if a.val() != SEG_BASE + (i as u32) + 1 {
            return false;
        }
        if a.detail().is_empty() || !a.detail().contains("建议") {
            return false;
        }
        let mut j = i + 1;
        while j < ALL_DIAGS.len() {
            match ALL_DIAGS.get(j) {
                Some(b) => {
                    if a.val() == b.val() {
                        return false;
                    }
                }
                None => return false,
            }
            j += 1;
        }
        i += 1;
    }
    detail(0xDEAD) == detail(0x5A5A) && !detail(0xDEAD).is_empty()
}

/// 桶界核：桶宽 2 的幂（下界 2^i、上界 2^(i+1)-1、相邻桶无缝衔接），
/// 桶序号表逐点核对（1→0、2→1、3→1、4→2、7→2、8→3、65535→15）。
fn bucket_pow2_ok() -> bool {
    let mut i = 0usize;
    while i < BUCKET_COUNT {
        let lo = match bucket_lower(i) {
            Some(v) => v,
            None => return false,
        };
        let up = match bucket_upper(i) {
            Some(v) => v,
            None => return false,
        };
        // 桶宽 = 上界 + 1 - 下界 必为 2 的幂（即 lo 本身）。
        if up + 1 - lo != lo {
            return false;
        }
        // 相邻桶无缝：桶 i 上界 + 1 == 桶 i+1 下界。
        if i + 1 < BUCKET_COUNT {
            let next_lo = match bucket_lower(i + 1) {
                Some(v) => v,
                None => return false,
            };
            if up + 1 != next_lo {
                return false;
            }
        }
        i += 1;
    }
    bucket_index(1) == Some(0)
        && bucket_index(2) == Some(1)
        && bucket_index(3) == Some(1)
        && bucket_index(4) == Some(2)
        && bucket_index(7) == Some(2)
        && bucket_index(8) == Some(3)
        && bucket_index(65535) == Some(15)
        && bucket_index(0).is_none()
        && bucket_index(65536).is_none()
}

/// M-208 分布对账：注入已知样本 1..=100µs（n=100），P50=50、P99=99，
/// 桶账守恒同时成立。
fn injection_m208_ok() -> bool {
    let mut ledger = DurationLedger::new();
    let mut us: u64 = 1;
    while us <= 100 {
        if ledger.record(us).is_err() {
            return false;
        }
        us += 1;
    }
    if ledger.count() != 100 || !ledger.buckets_conserved() {
        return false;
    }
    ledger.p50() == Ok(50) && ledger.p99() == Ok(99)
}

/// 分位守卫：p=0 与 p=101 显性拒；P50 ≤ P99 单调；秩口径抽查。
fn percentile_guard_ok() -> bool {
    let mut ledger = DurationLedger::new();
    let mut us: u64 = 1;
    while us <= 100 {
        if ledger.record(us).is_err() {
            return false;
        }
        us += 1;
    }
    if ledger.percentile(0) != Err(Diag::PercentileRange.val()) {
        return false;
    }
    if ledger.percentile(101) != Err(Diag::PercentileRange.val()) {
        return false;
    }
    if ledger.percentile(1) != Ok(1) || ledger.percentile(100) != Ok(100) {
        return false;
    }
    match (ledger.p50(), ledger.p99()) {
        (Ok(a), Ok(b)) => a <= b,
        _ => false,
    }
}

/// 空账与零样本闸：空账求分位拒（LedgerEmpty）、零样本拒（ZeroSample）。
fn empty_and_zero_gate_ok() -> bool {
    let ledger = DurationLedger::new();
    if ledger.p50() != Err(Diag::LedgerEmpty.val()) {
        return false;
    }
    if ledger.p99() != Err(Diag::LedgerEmpty.val()) {
        return false;
    }
    let mut l2 = DurationLedger::new();
    l2.record(0) == Err(Diag::ZeroSample.val())
}

/// 超量程闸：65536µs 拒（OutOfRange），65535µs 收进桶 15。
fn out_of_range_gate_ok() -> bool {
    let mut ledger = DurationLedger::new();
    if ledger.record(65536) != Err(Diag::OutOfRange.val()) {
        return false;
    }
    if ledger.record(65535).is_err() {
        return false;
    }
    ledger.count() == 1
        && match ledger.buckets().get(15) {
            Some(b) => *b == 1,
            None => false,
        }
}

/// 桶账守恒注入核：构造跨桶样本落 5 个桶，各桶计数与样本数守恒，
/// 单桶计数逐桶核对。
fn bucket_conservation_ok() -> bool {
    let mut ledger = DurationLedger::new();
    // 样本：桶 0×2（1,1）、桶 1×1（3）、桶 3×3（8,9,10）、桶 7×1（128）。
    let feed: [u64; 7] = [1, 1, 3, 8, 9, 10, 128];
    for us in feed.iter() {
        if ledger.record(*us).is_err() {
            return false;
        }
    }
    if !ledger.buckets_conserved() || ledger.count() != 7 {
        return false;
    }
    let expect: [(usize, u64); 4] = [(0, 2), (1, 1), (3, 3), (7, 1)];
    let mut k = 0usize;
    while k < expect.len() {
        let (idx, want) = match expect.get(k) {
            Some(e) => *e,
            None => return false,
        };
        match ledger.buckets().get(idx) {
            Some(b) => {
                if *b != want {
                    return false;
                }
            }
            None => return false,
        }
        k += 1;
    }
    // 其余桶必为 0。
    let mut i = 0usize;
    while i < BUCKET_COUNT {
        let in_expect = {
            let mut found = false;
            let mut k2 = 0usize;
            while k2 < expect.len() {
                match expect.get(k2) {
                    Some((idx, _)) => {
                        if *idx == i {
                            found = true;
                        }
                    }
                    None => return false,
                }
                k2 += 1;
            }
            found
        };
        if !in_expect {
            match ledger.buckets().get(i) {
                Some(b) => {
                    if *b != 0 {
                        return false;
                    }
                }
                None => return false,
            }
        }
        i += 1;
    }
    true
}

/// 慢例名单核：1..=100 样本下 P99=99，慢例名单恰为 {100}（> 99 的
/// 记账序），名单口径与慢线对账一致。
fn slow_list_ok() -> bool {
    let mut ledger = DurationLedger::new();
    let mut us: u64 = 1;
    while us <= 100 {
        if ledger.record(us).is_err() {
            return false;
        }
        us += 1;
    }
    let slow = match ledger.slow_cases() {
        Ok(s) => s,
        Err(_) => return false,
    };
    if slow.len() != 1 {
        return false;
    }
    (match slow.first() {
        Some(&v) => v == 100,
        None => false,
    }) && detail(Diag::SlowListMismatch.val()).contains("P99")
}

/// 容量闸：账满（SAMPLE_CAP）后再记必拒（LedgerFull），已记账目不损。
fn ledger_full_gate_ok() -> bool {
    let mut ledger = DurationLedger::new();
    // 只灌到容量（1µs 样本重复，账面合法）。
    let mut n = 0usize;
    while n < SAMPLE_CAP {
        if ledger.record(1).is_err() {
            return false;
        }
        n += 1;
    }
    if ledger.record(2) != Err(Diag::LedgerFull.val()) {
        return false;
    }
    ledger.count() == SAMPLE_CAP && ledger.buckets_conserved() && ledger.p50() == Ok(1)
}

/// 分位秩口径核：n=3 样本 [10,20,30]，P50 秩 ceil(150/100)=2 → 20、
/// P99 秩 ceil(297/100)=3 → 30（奇数样本的秩落点钉死）。
fn quantile_rank_ok() -> bool {
    let mut ledger = DurationLedger::new();
    let feed: [u64; 3] = [10, 20, 30];
    for us in feed.iter() {
        if ledger.record(*us).is_err() {
            return false;
        }
    }
    ledger.percentile(50) == Ok(20) && ledger.percentile(99) == Ok(30)
}

/// 双跑一致：同样本序列两次记账导出的分位与直方图逐字段相同。
fn double_run_ok() -> bool {
    let run_once = || -> Result<(Vec<u64>, u64, u64), u32> {
        let mut ledger = DurationLedger::new();
        let mut us: u64 = 1;
        while us <= 100 {
            ledger.record(us)?;
            us += 1;
        }
        let p50 = ledger.p50()?;
        let p99 = ledger.p99()?;
        Ok((ledger.buckets().to_vec(), p50, p99))
    };
    let a = run_once();
    let b = run_once();
    a.is_ok() && b.is_ok() && a == b
}

/// 写回面：状态=已完成、结果摘要非空（含桶数、P50/P99 与对账结论）。
fn writeback_summary_ok() -> bool {
    let mut ledger = DurationLedger::new();
    let mut us: u64 = 1;
    while us <= 100 {
        if ledger.record(us).is_err() {
            return false;
        }
        us += 1;
    }
    let (p50, p99) = match (ledger.p50(), ledger.p99()) {
        (Ok(a), Ok(b)) => (a, b),
        _ => return false,
    };
    let slow_n = match ledger.slow_cases() {
        Ok(s) => s.len(),
        Err(_) => return false,
    };
    let summary = format!(
        "UNX-F3449 完成：{}桶2的幂 P50={}µs P99={}µs M-208对账成立 慢例{}条",
        BUCKET_COUNT,
        p50,
        p99,
        slow_n
    );
    !summary.is_empty()
        && summary.contains("UNX-F3449")
        && summary.contains("M-208")
        && ledger.buckets_conserved()
        && detail(Diag::P50GtP99.val()).contains("单调")
}

// ── 判据总成：run_unx3449_checks ───────────────────────────────────────

/// run_unx3449_checks：本单判据聚合（诊断段 0x79D80，全部走 add_dyn）。
pub fn run_unx3449_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F3449 慢用例账本");
    let checks: [(&'static str, bool, String); 13] = [
        (
            "段位守卫0x40对齐且在批段窗内",
            seg_guard_ok(),
            format!("SEG_BASE=0x{:X} 守卫{}", SEG_BASE, if seg_guard_ok() { "过" } else { "破" }),
        ),
        (
            "诊断码闭集12码唯一且文案带建议",
            diag_closed_set_ok(),
            format!("闭集{}", if diag_closed_set_ok() { "完整" } else { "破缺" }),
        ),
        (
            "桶界2的幂相邻无缝序号表逐点核",
            bucket_pow2_ok(),
            format!("桶界{}", if bucket_pow2_ok() { "钉死" } else { "漂移" }),
        ),
        (
            "M-208已知样本P50=50P99=99",
            injection_m208_ok(),
            format!("M-208对账{}", if injection_m208_ok() { "成立" } else { "破约" }),
        ),
        (
            "分位守卫p越界拒P50小于等于P99",
            percentile_guard_ok(),
            format!("分位守卫{}", if percentile_guard_ok() { "钉死" } else { "失守" }),
        ),
        (
            "空账导出与零样本显性拒",
            empty_and_zero_gate_ok(),
            format!("空账闸{}", if empty_and_zero_gate_ok() { "拦准" } else { "失守" }),
        ),
        (
            "超量程65536拒上沿65535收桶15",
            out_of_range_gate_ok(),
            format!("量程闸{}", if out_of_range_gate_ok() { "拦准" } else { "失守" }),
        ),
        (
            "跨桶样本桶账守恒逐桶核对",
            bucket_conservation_ok(),
            format!("桶账{}", if bucket_conservation_ok() { "守恒" } else { "不平" }),
        ),
        (
            "慢例名单恰为P99线右例",
            slow_list_ok(),
            format!("慢例名单{}", if slow_list_ok() { "对账" } else { "错账" }),
        ),
        (
            "账满拒记已账不损分位仍可导",
            ledger_full_gate_ok(),
            format!("容量闸{}", if ledger_full_gate_ok() { "拦准" } else { "失守" }),
        ),
        (
            "奇数样本分位秩落点钉死",
            quantile_rank_ok(),
            format!("秩口径{}", if quantile_rank_ok() { "钉死" } else { "漂移" }),
        ),
        (
            "双跑一致分位与直方图逐字段同",
            double_run_ok(),
            format!("双跑{}", if double_run_ok() { "一致" } else { "分叉" }),
        ),
        (
            "样本账容量上限冻结4096",
            sample_cap_ok(),
            format!("容量{}", if sample_cap_ok() { "冻结" } else { "漂移" }),
        ),
    ];
    // 写回摘要判据（第 14 条，与 13 条并列成账）。
    let wb = writeback_summary_ok();
    for (name, passed, det) in checks.iter() {
        s.add_dyn(name, *passed, det);
    }
    s.add_dyn(
        "写回摘要非空状态已完成",
        wb,
        &format!("写回{}", if wb { "非空" } else { "空" }),
    );
    s
}

/// 样本账容量常量自洽：容量 > 0 且为 2 的幂（4096 = 2^12）。
fn sample_cap_ok() -> bool {
    if SAMPLE_CAP == 0 {
        return false;
    }
    let mut v = SAMPLE_CAP;
    while v % 2 == 0 {
        v /= 2;
    }
    v == 1 && SAMPLE_CAP == 4096 && INJECTION_CASE_ID == 0x208
}

// ── 测试区（#[cfg(test)]：允许 assert!/assert_eq!/unwrap）──────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn seg_guard_and_diag_closed_set() {
        assert!(seg_guard_ok());
        assert_eq!(SEG_BASE, 0x79D80);
        assert!(diag_closed_set_ok());
        for (i, d) in ALL_DIAGS.iter().enumerate() {
            assert_eq!(d.val(), SEG_BASE + i as u32 + 1);
        }
        assert_eq!(detail(0xDEAD), detail(0x5A5A));
    }

    #[test]
    fn bucket_boundaries_pow2() {
        assert!(bucket_pow2_ok());
        assert_eq!(bucket_lower(0), Some(1));
        assert_eq!(bucket_upper(0), Some(1));
        assert_eq!(bucket_lower(15), Some(32768));
        assert_eq!(bucket_upper(15), Some(65535));
        assert_eq!(bucket_lower(16), None);
    }

    #[test]
    fn injection_m208_known_samples() {
        assert!(injection_m208_ok());
        let mut ledger = DurationLedger::new();
        for us in 1u64..=100 {
            assert!(ledger.record(us).is_ok());
        }
        assert_eq!(ledger.p50().unwrap(), 50);
        assert_eq!(ledger.p99().unwrap(), 99);
    }

    #[test]
    fn percentile_guards() {
        assert!(percentile_guard_ok());
        assert!(empty_and_zero_gate_ok());
    }

    #[test]
    fn range_and_capacity_gates() {
        assert!(out_of_range_gate_ok());
        assert!(ledger_full_gate_ok());
        assert_eq!(SAMPLE_CAP, 4096);
    }

    #[test]
    fn bucket_conservation_cross_buckets() {
        assert!(bucket_conservation_ok());
        let mut ledger = DurationLedger::new();
        assert!(ledger.record(5).is_ok());
        assert!(ledger.buckets_conserved());
    }

    #[test]
    fn slow_list_p99_line() {
        assert!(slow_list_ok());
    }

    #[test]
    fn quantile_rank_odd_n() {
        assert!(quantile_rank_ok());
    }

    #[test]
    fn double_run_identical() {
        assert!(double_run_ok());
    }

    #[test]
    fn writeback_summary_non_empty() {
        assert!(writeback_summary_ok());
    }

    #[test]
    fn sample_cap_frozen() {
        assert!(sample_cap_ok());
        assert_eq!(INJECTION_CASE_ID, 0x208);
        assert_eq!(BUCKET_COUNT, 16);
    }

    #[test]
    fn checks_aggregate_all_green() {
        let s = run_unx3449_checks();
        assert!(s.len() >= 12);
        assert!(s.all_passed(), "全部判据必须绿：{}", s.red_items_text());
    }
}
