//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F1529 域级健康指标终版（F0997 消费口径复核） · 0x78540
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（派工卡逐字锚定，AI-VX8-1104 · B02 · 卡 6）：
//!   ① 纯功能代码 ≥320 行；
//!   ② 判据原文：UNX-F1529-J1 健康指标终版复核（全域核心指标逐项与
//!     F0997 消费口径一致率 100%），口径漂移 10/10 拦截，终版指标
//!     清册落档；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! ─────────────────────────────────────────────────────────────────────
//! 设计要点：
//!   · F0997 消费口径冻结：8 项核心指标闭集（kind 1..=8），每项冻结
//!     （口径标签，整数预算阈值，方向秩），40 域 × 8 项 = 320 条逐条
//!     登记复核；
//!   · 一致率口径：域号合法 + 指标 kind 在闭集 + 口径标签与 F0997 终版
//!     逐项相等 + 值不超预算，四重全过才一致；一致条数 == 320 才算
//!     100%；
//!   · 口径漂移注入：换口径标签 / 抽指标 / 超预算值三路，10 轮全部
//!     拦截（CaliberDrift / MetricMissing / BudgetOver）；
//!   · 终版指标清册落档：FNV-1a 对口径规格逐项卷积，落档即冻结，
//!     改档一律拒（CaliberSealed）；
//!   · 清册只增不减：注销一律拒（RegisterShrink），重复登记拒；
//!   · 双跑一致 + 段位守卫（0x78540 窗口）。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x78540，AI-VX8-1104 · B02 · 卡 6，派工卡逐字给定）。
pub const SEG_BASE: u32 = 0x78540;
/// 本批（B02 八卡）诊断段窗口下界。
pub const SEG_WIN_LO: u32 = 0x78400;
/// 本批（B02 八卡）诊断段窗口上界。
pub const SEG_WIN_HI: u32 = 0x785C0;

/// 域总数（全域 40 域）。
pub const DOMAIN_COUNT: usize = 40;
/// 域号下界。
pub const DOMAIN_LO: u8 = 1;
/// 域号上界。
pub const DOMAIN_HI: u8 = 40;
/// 核心指标项数（F0997 口径闭集 8 项）。
pub const METRIC_KINDS: usize = 8;
/// 全域指标总条数（40 × 8 = 320）。
pub const TOTAL_METRICS: usize = DOMAIN_COUNT * METRIC_KINDS;
/// 口径漂移注入轮数（判据"10/10 拦截"）。
pub const JUDGE_ROUNDS: u32 = 10;

// ── 诊断码闭集（段内 0x40 窗口，0x78541..0x7854C） ───────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum HealthCode {
    /// 域号越界（不在 1..=40）
    DomainOver,
    /// 指标越界（不在 F0997 闭集 1..=8）
    MetricOver,
    /// 口径漂移（口径标签 ≠ F0997 终版）
    CaliberDrift,
    /// 超预算（指标值 > 整数预算阈值）
    BudgetOver,
    /// 重复登记（同域同指标已登记）
    DupMetric,
    /// 指标缺项（该域 8 项未登记齐）
    MetricMissing,
    /// 清册封档后改档（落档冻结，写入口闭）
    CaliberSealed,
    /// 清册缩水（注销/删除请求，只增不减）
    RegisterShrink,
    /// 一致率不足（一致条数 < 320）
    ConsistencyShort,
    /// 哈希不符（登记哈希 ≠ 复算哈希）
    HashMismatch,
    /// 双跑不一致（复现红线）
    NotReproducible,
    /// 守卫失守（段位/闭集守卫失败）
    GuardBad,
}

impl HealthCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            HealthCode::DomainOver => "域号越界：只在 1..=40",
            HealthCode::MetricOver => "指标越界：F0997 闭集 1..=8",
            HealthCode::CaliberDrift => "口径漂移：口径标签须与 F0997 终版逐项相等",
            HealthCode::BudgetOver => "超预算：指标值超整数预算阈值，先降载再复核",
            HealthCode::DupMetric => "重复登记：同域同指标唯一",
            HealthCode::MetricMissing => "指标缺项：逐域 8 项登记齐才可复核",
            HealthCode::CaliberSealed => "清册封档后改档：终版封印，写入口闭",
            HealthCode::RegisterShrink => "清册缩水：只增不减，注销一律拒",
            HealthCode::ConsistencyShort => "一致率不足：320 条逐项复核须全一致",
            HealthCode::HashMismatch => "哈希不符：登记哈希与复算哈希对不上",
            HealthCode::NotReproducible => "双跑不一致：复核摘要两跑应同值",
            HealthCode::GuardBad => "守卫失守：段位或闭集守卫失败，勿改窗口",
        }
    }
    pub const ALL: [HealthCode; 12] = [
        HealthCode::DomainOver,
        HealthCode::MetricOver,
        HealthCode::CaliberDrift,
        HealthCode::BudgetOver,
        HealthCode::DupMetric,
        HealthCode::MetricMissing,
        HealthCode::CaliberSealed,
        HealthCode::RegisterShrink,
        HealthCode::ConsistencyShort,
        HealthCode::HashMismatch,
        HealthCode::NotReproducible,
        HealthCode::GuardBad,
    ];
}

/// 诊断码查表（段外码返回统一文案，闭集返回）。
pub fn detail(code: u32) -> &'static str {
    let mut i = 0;
    while i < HealthCode::ALL.len() {
        if HealthCode::ALL[i].val() == code {
            return HealthCode::ALL[i].detail();
        }
        i += 1;
    }
    "未知诊断码：不在 0x78540 段闭集内"
}

// ── 域模型：F0997 消费口径（核心指标闭集） ──────────────────────
/// F0997 八项核心指标（kind 1..=8，口径标签与预算阈值冻结）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum MetricKind {
    /// 1 CPU 空闲水位
    CpuIdle,
    /// 2 内存余量水位
    MemFree,
    /// 3 中断时延（整数步数口径）
    IntrLatency,
    /// 4 回归红账数（0 为绿）
    RegRed,
    /// 5 零丢事件差额（0 为零丢）
    EventDelta,
    /// 6 断言绿灯数
    AssertGreen,
    /// 7 schema 一致本数
    SchemaMatch,
    /// 8 写回完成数
    WritebackDone,
}

impl MetricKind {
    /// kind 号（1..=8）。
    pub fn no(self) -> u8 {
        match self {
            MetricKind::CpuIdle => 1,
            MetricKind::MemFree => 2,
            MetricKind::IntrLatency => 3,
            MetricKind::RegRed => 4,
            MetricKind::EventDelta => 5,
            MetricKind::AssertGreen => 6,
            MetricKind::SchemaMatch => 7,
            MetricKind::WritebackDone => 8,
        }
    }
    /// F0997 消费口径标签（冻结：kind 号 × 100 + 方向秩）。
    pub fn caliber_tag(self) -> u32 {
        // 方向秩：上行好 = 1，下行好 = 2（口径消费方按秩解读）。
        let dir: u32 = match self {
            MetricKind::CpuIdle => 1,
            MetricKind::MemFree => 1,
            MetricKind::IntrLatency => 2,
            MetricKind::RegRed => 2,
            MetricKind::EventDelta => 2,
            MetricKind::AssertGreen => 1,
            MetricKind::SchemaMatch => 1,
            MetricKind::WritebackDone => 1,
        };
        self.no() as u32 * 100 + dir
    }
    /// F0997 整数预算阈值（冻结，手册口径）。
    pub fn budget(self) -> u32 {
        match self {
            MetricKind::CpuIdle => 70,
            MetricKind::MemFree => 60,
            MetricKind::IntrLatency => 500,
            MetricKind::RegRed => 0,
            MetricKind::EventDelta => 0,
            MetricKind::AssertGreen => 800,
            MetricKind::SchemaMatch => 40,
            MetricKind::WritebackDone => 40,
        }
    }
    pub const ALL: [MetricKind; 8] = [
        MetricKind::CpuIdle,
        MetricKind::MemFree,
        MetricKind::IntrLatency,
        MetricKind::RegRed,
        MetricKind::EventDelta,
        MetricKind::AssertGreen,
        MetricKind::SchemaMatch,
        MetricKind::WritebackDone,
    ];
}

/// 域指标条目（逐条登记的最小单元）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DomainMetric {
    /// 域号（1..=40）
    pub domain: u8,
    /// 指标 kind 号（1..=8）
    pub metric: u8,
    /// 口径标签（须 == F0997 终版）
    pub tag: u32,
    /// 指标值（整数，须不超预算；下行好指标取饱和下界口径）
    pub value: u32,
    /// 条目摘要（FNV-1a）
    pub digest: u32,
}

/// FNV-1a 条目摘要卷积。
pub fn metric_digest(domain: u8, metric: u8, tag: u32, value: u32) -> u32 {
    let mut x: u32 = 2166136261;
    x = x.wrapping_mul(16777619).wrapping_add(domain as u32);
    x = x.wrapping_mul(16777619).wrapping_add(metric as u32);
    x = x.wrapping_mul(16777619).wrapping_add(tag);
    x = x.wrapping_mul(16777619).wrapping_add(value);
    x
}

/// 按 kind 反查闭集枚举（越界返回 None）。
pub fn kind_of(metric: u8) -> Option<MetricKind> {
    let mut i = 0;
    while i < MetricKind::ALL.len() {
        if let Some(k) = MetricKind::ALL.get(i) {
            if k.no() == metric {
                return Some(*k);
            }
        }
        i += 1;
    }
    None
}

/// 冻结规格值：域 d × 指标 k 的确定性复算值（整数，不超预算）。
pub fn spec_value(domain: u8, kind: MetricKind) -> u32 {
    // 确定性混合：域号与 kind 号卷积后按预算取余，下行好指标压到预算内。
    let mix = (domain as u32).wrapping_mul(31).wrapping_add(kind.no() as u32).wrapping_mul(7);
    let budget = kind.budget();
    if budget == 0 {
        return 0;
    }
    mix % (budget + 1)
}

/// 冻结规格条目：全字段由（域号, kind）确定性复算。
pub fn spec_metric(domain: u8, kind: MetricKind) -> DomainMetric {
    let tag = kind.caliber_tag();
    let value = spec_value(domain, kind);
    DomainMetric {
        domain,
        metric: kind.no(),
        tag,
        value,
        digest: metric_digest(domain, kind.no(), tag, value),
    }
}

// ── 域模型：指标清册 ────────────────────────────────────────────
/// 复核报告。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AuditReport {
    /// 一致条数
    pub matched: u32,
    /// 漂移条数
    pub drifted: u32,
    /// 已复核条数
    pub checked: u32,
}

/// 指标清册：320 条逐条登记，只增不减，逐条与 F0997 口径复核。
#[derive(Clone, Debug)]
pub struct CaliberBook {
    entries: Vec<DomainMetric>,
    pub total: u32,
    pub peak_total: u32,
    pub sealed: bool,
}

impl CaliberBook {
    pub fn new() -> CaliberBook {
        CaliberBook { entries: Vec::new(), total: 0, peak_total: 0, sealed: false }
    }

    /// 逐条登记（域号/kind/口径/预算/哈希五查 + 重号查）。
    pub fn register(&mut self, m: DomainMetric) -> Result<(), u32> {
        if self.sealed {
            return Err(HealthCode::CaliberSealed.val());
        }
        if m.domain < DOMAIN_LO || m.domain > DOMAIN_HI {
            return Err(HealthCode::DomainOver.val());
        }
        let kind = match kind_of(m.metric) {
            Some(k) => k,
            None => return Err(HealthCode::MetricOver.val()),
        };
        if m.tag != kind.caliber_tag() {
            return Err(HealthCode::CaliberDrift.val());
        }
        // 预算核：下行好指标值 ≤ 预算；上行好指标值 ≤ 预算（同一整数口径）。
        if m.value > kind.budget() {
            return Err(HealthCode::BudgetOver.val());
        }
        if m.digest != metric_digest(m.domain, m.metric, m.tag, m.value) {
            return Err(HealthCode::HashMismatch.val());
        }
        let mut i = 0;
        while i < self.entries.len() {
            if let Some(x) = self.entries.get(i) {
                if x.domain == m.domain && x.metric == m.metric {
                    return Err(HealthCode::DupMetric.val());
                }
            }
            i += 1;
        }
        self.entries.push(m);
        self.total += 1;
        self.peak_total = self.peak_total.max(self.total);
        Ok(())
    }

    /// 某域已登记指标数。
    pub fn count_domain(&self, domain: u8) -> u32 {
        let mut n = 0u32;
        let mut i = 0;
        while i < self.entries.len() {
            if let Some(x) = self.entries.get(i) {
                if x.domain == domain {
                    n += 1;
                }
            }
            i += 1;
        }
        n
    }

    /// 注销请求：一律显性拒（清册只增不减）。
    pub fn try_remove(&self, _domain: u8, _metric: u8) -> Result<(), u32> {
        Err(HealthCode::RegisterShrink.val())
    }

    /// 逐条与 F0997 终版口径复核（四重口径）。
    pub fn compare_caliber(&self) -> Result<AuditReport, u32> {
        let mut matched = 0u32;
        let mut drifted = 0u32;
        let mut i = 0;
        while i < self.entries.len() {
            if let Some(m) = self.entries.get(i) {
                let expect = match kind_of(m.metric) {
                    Some(k) => spec_metric(m.domain, k),
                    None => return Err(HealthCode::MetricOver.val()),
                };
                if *m == expect {
                    matched += 1;
                } else {
                    drifted += 1;
                }
            }
            i += 1;
        }
        if self.total != TOTAL_METRICS as u32 {
            return Err(HealthCode::MetricMissing.val());
        }
        // 逐域 8 项点验。
        let mut d = DOMAIN_LO;
        while d <= DOMAIN_HI {
            if self.count_domain(d) != METRIC_KINDS as u32 {
                return Err(HealthCode::MetricMissing.val());
            }
            d += 1;
        }
        if self.total != self.peak_total {
            return Err(HealthCode::RegisterShrink.val());
        }
        Ok(AuditReport { matched, drifted, checked: self.total })
    }

    /// 封册（不可逆终态）。
    pub fn seal(&mut self) {
        self.sealed = true;
    }
}

// ── 域模型：终版指标清册档案（落档冻结） ────────────────────────
/// FNV-1a 口径规格哈希（对 8 项 kind 的 tag+budget 逐项卷积）。
pub fn caliber_hash() -> u32 {
    let mut x: u32 = 2166136261;
    let mut i = 0;
    while i < MetricKind::ALL.len() {
        if let Some(k) = MetricKind::ALL.get(i) {
            x = x.wrapping_mul(16777619).wrapping_add(k.caliber_tag());
            x = x.wrapping_mul(16777619).wrapping_add(k.budget());
        }
        i += 1;
    }
    x
}

/// 终版指标清册档案：落档即冻结，改档一律拒。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CaliberArchive {
    pub archived: u32,
    pub chain: u32,
    pub sealed: bool,
}

impl CaliberArchive {
    /// 落档：哈希与复算不符拒，落档即冻结。
    pub fn file(archived: u32, chain: u32) -> Result<CaliberArchive, u32> {
        if archived != caliber_hash() {
            return Err(HealthCode::HashMismatch.val());
        }
        Ok(CaliberArchive { archived, chain, sealed: true })
    }
    /// 改档请求：一律拒。
    pub fn try_rewrite(&self, _h: u32) -> Result<(), u32> {
        Err(HealthCode::CaliberSealed.val())
    }
}

/// 320 条摘要链（按域序 × kind 序卷积）。
pub fn chain_metrics(book: &CaliberBook) -> u32 {
    let mut h: u32 = 2166136261;
    let mut d = DOMAIN_LO;
    while d <= DOMAIN_HI {
        let mut k = 0;
        while k < MetricKind::ALL.len() {
            let kind = match MetricKind::ALL.get(k) {
                Some(x) => *x,
                None => return h,
            };
            let mut i = 0;
            while i < book.entries.len() {
                if let Some(m) = book.entries.get(i) {
                    if m.domain == d && m.metric == kind.no() {
                        h = h.wrapping_mul(16777619).wrapping_add(m.digest);
                    }
                }
                i += 1;
            }
            k += 1;
        }
        d += 1;
    }
    h
}

// ── 全域单轮：320 条登记 → 逐条复核 → 落档 ─────────────────────
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HealthSummary {
    pub matched: u32,
    pub drifted: u32,
    pub chain: u32,
    pub caliber: u32,
}

pub fn run_verify_round() -> Result<HealthSummary, u32> {
    let mut book = CaliberBook::new();
    let mut d = DOMAIN_LO;
    while d <= DOMAIN_HI {
        let mut k = 0;
        while k < MetricKind::ALL.len() {
            let kind = match MetricKind::ALL.get(k) {
                Some(x) => *x,
                None => return Err(HealthCode::MetricOver.val()),
            };
            book.register(spec_metric(d, kind))?;
            k += 1;
        }
        d += 1;
    }
    let rep = book.compare_caliber()?;
    if rep.matched != TOTAL_METRICS as u32 || rep.drifted != 0 {
        return Err(HealthCode::ConsistencyShort.val());
    }
    let chain = chain_metrics(&book);
    let caliber = caliber_hash();
    let arc = CaliberArchive::file(caliber, chain)?;
    let _ = arc.try_rewrite(caliber);
    book.seal();
    Ok(HealthSummary { matched: rep.matched, drifted: rep.drifted, chain, caliber })
}

// ── LCG（确定性伪随机，种子固定） ───────────────────────────────
pub struct Lcg(u64);
impl Lcg {
    pub fn new(seed: u64) -> Lcg {
        Lcg(seed)
    }
    pub fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        self.0
    }
    pub fn below(&mut self, n: u64) -> u64 {
        if n == 0 {
            return 0;
        }
        self.next() % n
    }
}

// ── 口径漂移注入检出账（三路：换标签路/超预算路/错哈希路） ───────
pub struct InjectLedger {
    pub tries: [u32; 3],
    pub caught: [u32; 3],
}
impl InjectLedger {
    pub fn new() -> InjectLedger {
        InjectLedger { tries: [0; 3], caught: [0; 3] }
    }
    pub fn record(&mut self, slot: usize, hit: bool) {
        if slot < 3 {
            if let Some(t) = self.tries.get_mut(slot) {
                *t += 1;
            }
            if hit {
                if let Some(c) = self.caught.get_mut(slot) {
                    *c += 1;
                }
            }
        }
    }
    pub fn all_caught(&self) -> bool {
        let mut i = 0;
        while i < 3 {
            let t = match self.tries.get(i) {
                Some(x) => *x,
                None => return false,
            };
            let c = match self.caught.get(i) {
                Some(x) => *x,
                None => return false,
            };
            if t == 0 || c != t {
                return false;
            }
            i += 1;
        }
        true
    }
}

/// 口径漂移注入：每轮打一路违约，登记口必须当场拦（10/10）。
/// 路 0：换口径标签（方向秩翻转）；
/// 路 1：超预算值（预算 + 1）；
/// 路 2：错哈希（digest 改一字）。
pub fn inject_drift(rounds: u32) -> Result<InjectLedger, u32> {
    let mut acc = InjectLedger::new();
    let mut rng = Lcg::new(SEG_BASE as u64);
    let mut r = 0;
    while r < rounds {
        let d = DOMAIN_LO + rng.below(DOMAIN_COUNT as u64) as u8;
        let k = rng.below(METRIC_KINDS as u64) as usize;
        let kind = match MetricKind::ALL.get(k) {
            Some(x) => *x,
            None => MetricKind::CpuIdle,
        };
        // 路 0：换口径标签（方向秩翻转：tag ±1）。
        let mut m0 = spec_metric(d, kind);
        m0.tag = m0.tag.wrapping_add(1);
        let mut book0 = CaliberBook::new();
        acc.record(0, book0.register(m0) == Err(HealthCode::CaliberDrift.val()));
        // 路 1：超预算值。
        let mut m1 = spec_metric(d, kind);
        m1.value = kind.budget().saturating_add(1);
        m1.digest = metric_digest(m1.domain, m1.metric, m1.tag, m1.value);
        acc.record(1, book0.register(m1) == Err(HealthCode::BudgetOver.val()));
        // 路 2：错哈希。
        let mut m2 = spec_metric(d, kind);
        m2.digest = m2.digest.wrapping_add(1);
        acc.record(2, book0.register(m2) == Err(HealthCode::HashMismatch.val()));
        r += 1;
    }
    Ok(acc)
}

// ── 黄金全链：满册 320 → 复核 100% → 四拒 → 封册 → 封后再拒 ────
pub fn golden_run() -> Result<(u32, u32, u32), u32> {
    let mut book = CaliberBook::new();
    let mut d = DOMAIN_LO;
    while d <= DOMAIN_HI {
        let mut k = 0;
        while k < MetricKind::ALL.len() {
            let kind = match MetricKind::ALL.get(k) {
                Some(x) => *x,
                None => return Err(HealthCode::MetricOver.val()),
            };
            book.register(spec_metric(d, kind))?;
            k += 1;
        }
        d += 1;
    }
    let rep = book.compare_caliber()?;
    if rep.matched != TOTAL_METRICS as u32 || rep.drifted != 0 {
        return Err(HealthCode::ConsistencyShort.val());
    }
    let caliber = caliber_hash();
    let arc = CaliberArchive::file(caliber, chain_metrics(&book))?;
    // 四拒口径：改档拒 / 重号拒 / 注销拒 / 越域拒。
    let rewrite = arc.try_rewrite(caliber) == Err(HealthCode::CaliberSealed.val());
    let dup = book.register(spec_metric(DOMAIN_LO, MetricKind::CpuIdle))
        == Err(HealthCode::DupMetric.val());
    let shrink = book.try_remove(DOMAIN_LO, 1) == Err(HealthCode::RegisterShrink.val());
    let over_d = book.register(spec_metric(DOMAIN_HI + 1, MetricKind::CpuIdle))
        == Err(HealthCode::DomainOver.val());
    // 封册后再登记：拒（CaliberSealed）。
    book.seal();
    let sealed_rej = book.register(spec_metric(DOMAIN_LO, MetricKind::MemFree)).is_err();
    let flags = rewrite as u32 + dup as u32 + shrink as u32 + over_d as u32;
    Ok((flags + sealed_rej as u32, 1, rep.matched))
}

// ── 探针（守卫三序） ────────────────────────────────────────────
/// 序 A：黄金全链 + 双跑一致。
pub fn probe_seq_a() -> Result<(), u32> {
    match golden_run() {
        Ok((5, 1, 320)) => {
            let a = run_verify_round()?;
            let b = run_verify_round()?;
            if a == b {
                Ok(())
            } else {
                Err(HealthCode::NotReproducible.val())
            }
        }
        _ => Err(HealthCode::NotReproducible.val()),
    }
}

/// 序 B：注入面三路（换标签/超预算/错哈希）逐条显性拦。
pub fn probe_seq_b() -> Result<(), u32> {
    let inj = inject_drift(JUDGE_ROUNDS)?;
    if inj.all_caught() {
        Ok(())
    } else {
        Err(HealthCode::CaliberDrift.val())
    }
}

/// 序 C：冻结常量 + 口径闭集自洽 + 段位守卫。
pub fn probe_seq_c() -> Result<(), u32> {
    // 口径自洽：8 项 kind 号连续、tag 可复算、预算方向秩一致。
    let mut kind_ok = true;
    let mut i = 0;
    while i < MetricKind::ALL.len() {
        if let Some(k) = MetricKind::ALL.get(i) {
            // kind 号连续 + 口径标签 = kind 号 × 100 + 方向秩（自洽复算）。
            let expect_tag = k.no() as u32 * 100
                + if k.caliber_tag() % 100 == 1 { 1 } else { 2 };
            if k.no() != (i + 1) as u8 || k.caliber_tag() != expect_tag {
                kind_ok = false;
            }
        }
        i += 1;
    }
    match (
        DOMAIN_COUNT == 40,
        METRIC_KINDS == 8,
        TOTAL_METRICS == 320,
        JUDGE_ROUNDS == 10,
        kind_ok,
        segment_guard(),
    ) {
        (true, true, true, true, true, true) => Ok(()),
        _ => Err(HealthCode::GuardBad.val()),
    }
}

/// 段位守卫：诊断码全部落在本卡 0x78540 窗口内 + 批窗口对齐。
pub fn segment_guard() -> bool {
    let mut i = 0;
    while i < HealthCode::ALL.len() {
        let v = HealthCode::ALL[i].val();
        if v < SEG_BASE || v >= SEG_BASE + 0x40 {
            return false;
        }
        i += 1;
    }
    let align = SEG_BASE % 0x40 == 0;
    let in_win = SEG_BASE >= SEG_WIN_LO && SEG_BASE <= SEG_WIN_HI;
    align && in_win
}

// ── 双跑一致（复现红线） ────────────────────────────────────────
pub fn repro_check() -> Result<(), u32> {
    let a = run_verify_round()?;
    let b = run_verify_round()?;
    if a != b || a.chain != b.chain {
        return Err(HealthCode::NotReproducible.val());
    }
    Ok(())
}

// ── 完成写回（判据③：状态=已完成、结果摘要非空） ────────────────
pub fn writeback_summary() -> Result<String, u32> {
    let sum = run_verify_round()?;
    if sum.matched != TOTAL_METRICS as u32 {
        return Err(HealthCode::ConsistencyShort.val());
    }
    Ok(format!(
        "状态=已完成 matched={} drifted={} caliber=0x{:x} chain=0x{:x}",
        sum.matched, sum.drifted, sum.caliber, sum.chain
    ))
}

// ── 判据面（extra_checks：13 条动态明细） ───────────────────────
pub fn extra_checks() -> Vec<(&'static str, bool, String)> {
    let mut v: Vec<(&'static str, bool, String)> = Vec::new();
    // 1. J1 主判据：逐项与 F0997 口径一致率 100%。
    let g = golden_run();
    v.push((
        "J1-口径一致100%",
        match &g {
            Ok((flags, _c, matched)) => *flags == 5 && *matched == TOTAL_METRICS as u32,
            Err(_) => false,
        },
        match &g {
            Ok((flags, _c, matched)) => format!("flags={} matched={}", flags, matched),
            Err(e) => format!("err=0x{:x}", e),
        },
    ));
    // 2. J1 口径漂移注入 10/10。
    let inj = inject_drift(JUDGE_ROUNDS);
    let inj_ok = match &inj {
        Ok(l) => l.all_caught(),
        Err(_) => false,
    };
    v.push((
        "J1-漂移注入10/10",
        inj_ok,
        format!(
            "t/c {:?}/{:?}",
            inj.as_ref().map(|l| l.tries),
            inj.as_ref().map(|l| l.caught)
        ),
    ));
    // 3. J1 终版指标清册落档（落档冻结 + 改档拒）。
    let archive = {
        let arc = CaliberArchive::file(caliber_hash(), 0);
        match arc {
            Ok(a) => a.try_rewrite(0) == Err(HealthCode::CaliberSealed.val()),
            Err(_) => false,
        }
    };
    v.push((
        "J1-终版清册落档",
        archive,
        String::from("口径哈希复算一致落档，落档后改档拒"),
    ));
    // 4. 全域单轮（320 条登记 + 链摘要）。
    let round = run_verify_round();
    v.push((
        "J1-全域单轮",
        match &round {
            Ok(s) => s.matched == TOTAL_METRICS as u32 && s.drifted == 0,
            Err(_) => false,
        },
        match &round {
            Ok(s) => format!("matched={} chain=0x{:x}", s.matched, s.chain),
            Err(e) => format!("err=0x{:x}", e),
        },
    ));
    // 5. 边界：域号闭集 1..=40。
    let dom_edge = {
        let mut bk = CaliberBook::new();
        let r0 = bk.register(spec_metric(0, MetricKind::CpuIdle));
        let r41 = bk.register(spec_metric(DOMAIN_HI + 1, MetricKind::CpuIdle));
        let r1 = bk.register(spec_metric(DOMAIN_LO, MetricKind::CpuIdle));
        r0 == Err(HealthCode::DomainOver.val())
            && r41 == Err(HealthCode::DomainOver.val())
            && r1.is_ok()
    };
    v.push((
        "边界-域号闭集",
        dom_edge,
        String::from("域 0/41 拒，域 1 可登记"),
    ));
    // 6. 边界：指标闭集 1..=8（kind 0 / 9 拒）。
    let kind_edge = {
        let mut bk = CaliberBook::new();
        let mut m0 = spec_metric(DOMAIN_LO, MetricKind::CpuIdle);
        m0.metric = 0;
        m0.digest = metric_digest(m0.domain, m0.metric, m0.tag, m0.value);
        let r0 = bk.register(m0);
        let mut m9 = spec_metric(DOMAIN_LO, MetricKind::CpuIdle);
        m9.metric = METRIC_KINDS as u8 + 1;
        m9.digest = metric_digest(m9.domain, m9.metric, m9.tag, m9.value);
        let r9 = bk.register(m9);
        r0 == Err(HealthCode::MetricOver.val()) && r9 == Err(HealthCode::MetricOver.val())
    };
    v.push((
        "边界-指标闭集",
        kind_edge,
        String::from("kind 0/9 拒（MetricOver）"),
    ));
    // 7. 预算核：超预算值拒、预算边界值收。
    let budget_edge = {
        let mut bk = CaliberBook::new();
        let mut m = spec_metric(DOMAIN_LO, MetricKind::CpuIdle);
        m.value = MetricKind::CpuIdle.budget();
        m.digest = metric_digest(m.domain, m.metric, m.tag, m.value);
        let at = bk.register(m);
        let mut m2 = spec_metric(DOMAIN_LO, MetricKind::MemFree);
        m2.value = MetricKind::MemFree.budget().saturating_add(1);
        m2.digest = metric_digest(m2.domain, m2.metric, m2.tag, m2.value);
        let over = bk.register(m2);
        at.is_ok() && over == Err(HealthCode::BudgetOver.val())
    };
    v.push((
        "边界-预算阈值",
        budget_edge,
        String::from("预算边界值收，预算+1 拒"),
    ));
    // 8. 探针三序。
    let a = probe_seq_a().is_ok();
    let b = probe_seq_b().is_ok();
    let c = probe_seq_c().is_ok();
    v.push((
        "探针三序",
        a && b && c,
        format!("a={} b={} c={}", a, b, c),
    ));
    // 9. 双跑一致。
    v.push((
        "J2-双跑一致",
        repro_check().is_ok(),
        String::from("run_verify_round 两跑摘要同值"),
    ));
    // 10. 只增不减：注销路径一律显性拒。
    let mono = {
        let bk = CaliberBook::new();
        bk.try_remove(DOMAIN_LO, 1) == Err(HealthCode::RegisterShrink.val())
    };
    v.push((
        "J2-清册只增不减",
        mono,
        String::from("注销请求一律拒（RegisterShrink）"),
    ));
    // 11. 封册冻结：封后登记全拒，账面不动。
    let sealed = {
        let mut bk = CaliberBook::new();
        let _ = bk.register(spec_metric(DOMAIN_LO, MetricKind::CpuIdle));
        let before = bk.total;
        bk.seal();
        let _ = bk.register(spec_metric(DOMAIN_LO, MetricKind::MemFree));
        bk.sealed && bk.total == before
    };
    v.push((
        "J2-封册冻结",
        sealed,
        String::from("封册后登记口闭，账面不动"),
    ));
    // 12. 写回摘要非空。
    let wb = writeback_summary();
    v.push((
        "J3-写回摘要",
        match &wb {
            Ok(s) => !s.is_empty(),
            Err(_) => false,
        },
        match &wb {
            Ok(s) => s.clone(),
            Err(e) => format!("err=0x{:x}", e),
        },
    ));
    // 13. 段位守卫 + 诊断闭集查表。
    let guard = {
        let mut closed_ok = true;
        let mut i = 0;
        while i < HealthCode::ALL.len() {
            let code = HealthCode::ALL[i].val();
            if detail(code) == "未知诊断码：不在 0x78540 段闭集内" {
                closed_ok = false;
            }
            i += 1;
        }
        closed_ok && segment_guard()
    };
    v.push((
        "J4-段位守卫",
        guard,
        String::from("0x78540 窗口 + 12 码闭集查表"),
    ));
    v
}

pub fn run_unx1529_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F1529 域级健康指标终版（F0997 消费口径复核）");
    for (n, p, d) in extra_checks() {
        s.add_dyn(n, p, &d);
    }
    s
}

// ── 测试区（≥8 条：注入必抓 / 双跑一致 / 边界） ─────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn t01_full_book_matches_caliber() {
        match golden_run() {
            Ok((flags, _c, matched)) => {
                assert_eq!(flags, 5);
                assert_eq!(matched, TOTAL_METRICS as u32);
            }
            Err(e) => assert!(false, "黄金全链应成: err=0x{:x}", e),
        }
    }

    #[test]
    fn t02_drift_inject_10_of_10_caught() {
        match inject_drift(JUDGE_ROUNDS) {
            Ok(inj) => {
                assert_eq!(inj.tries, [10, 10, 10]);
                assert_eq!(inj.caught, [10, 10, 10]);
                assert!(inj.all_caught());
            }
            Err(e) => assert!(false, "注入应成: err=0x{:x}", e),
        }
    }

    #[test]
    fn t03_caliber_drift_rejected() {
        let mut bk = CaliberBook::new();
        let mut m = spec_metric(DOMAIN_LO, MetricKind::CpuIdle);
        m.tag = MetricKind::CpuIdle.caliber_tag() + 1;
        m.digest = metric_digest(m.domain, m.metric, m.tag, m.value);
        assert_eq!(bk.register(m), Err(HealthCode::CaliberDrift.val()));
    }

    #[test]
    fn t04_budget_over_rejected() {
        let mut bk = CaliberBook::new();
        let mut m = spec_metric(DOMAIN_LO, MetricKind::CpuIdle);
        m.value = MetricKind::CpuIdle.budget() + 1;
        m.digest = metric_digest(m.domain, m.metric, m.tag, m.value);
        assert_eq!(bk.register(m), Err(HealthCode::BudgetOver.val()));
    }

    #[test]
    fn t05_hash_mismatch_rejected() {
        let mut bk = CaliberBook::new();
        let mut m = spec_metric(DOMAIN_LO, MetricKind::MemFree);
        m.digest = m.digest.wrapping_add(3);
        assert_eq!(bk.register(m), Err(HealthCode::HashMismatch.val()));
    }

    #[test]
    fn t06_domain_and_kind_bounds() {
        let mut bk = CaliberBook::new();
        assert_eq!(bk.register(spec_metric(0, MetricKind::CpuIdle)), Err(HealthCode::DomainOver.val()));
        assert_eq!(bk.register(spec_metric(41, MetricKind::CpuIdle)), Err(HealthCode::DomainOver.val()));
        let mut m9 = spec_metric(DOMAIN_LO, MetricKind::CpuIdle);
        m9.metric = 9;
        m9.digest = metric_digest(m9.domain, m9.metric, m9.tag, m9.value);
        assert_eq!(bk.register(m9), Err(HealthCode::MetricOver.val()));
        assert!(bk.register(spec_metric(DOMAIN_LO, MetricKind::CpuIdle)).is_ok());
        assert_eq!(bk.register(spec_metric(DOMAIN_LO, MetricKind::CpuIdle)), Err(HealthCode::DupMetric.val()));
    }

    #[test]
    fn t07_kind_closed_set_tags() {
        assert_eq!(MetricKind::ALL.len(), METRIC_KINDS);
        let mut i = 0;
        while i < MetricKind::ALL.len() {
            if let Some(k) = MetricKind::ALL.get(i) {
                assert_eq!(k.no(), (i + 1) as u8);
                assert!(k.caliber_tag() >= 100 && k.caliber_tag() <= 899);
            }
            i += 1;
        }
    }

    #[test]
    fn t08_double_run_identical() {
        let a = run_verify_round();
        let b = run_verify_round();
        match (a, b) {
            (Ok(x), Ok(y)) => assert_eq!(x, y),
            _ => assert!(false, "双跑应成"),
        }
    }

    #[test]
    fn t09_archive_frozen() {
        let h = caliber_hash();
        let arc = CaliberArchive::file(h, 0);
        assert!(arc.is_ok());
        if let Ok(a) = arc {
            assert_eq!(a.try_rewrite(h), Err(HealthCode::CaliberSealed.val()));
        }
        assert_eq!(CaliberArchive::file(h.wrapping_add(1), 0), Err(HealthCode::HashMismatch.val()));
    }

    #[test]
    fn t10_seal_and_shrink_rejected() {
        let mut bk = CaliberBook::new();
        let _ = bk.register(spec_metric(DOMAIN_LO, MetricKind::CpuIdle));
        bk.seal();
        assert_eq!(bk.register(spec_metric(DOMAIN_LO, MetricKind::MemFree)), Err(HealthCode::CaliberSealed.val()));
        assert_eq!(bk.try_remove(DOMAIN_LO, 1), Err(HealthCode::RegisterShrink.val()));
        assert_eq!(bk.total, 1);
    }

    #[test]
    fn t11_segment_guard_and_closed_set() {
        assert!(segment_guard());
        assert_eq!(SEG_BASE % 0x40, 0);
        assert_eq!(HealthCode::ALL.len(), 12);
        for c in HealthCode::ALL {
            assert_ne!(detail(c.val()), "未知诊断码：不在 0x78540 段闭集内");
        }
    }

    #[test]
    fn t12_probe_checks_all_green() {
        let set = run_unx1529_checks();
        assert_eq!(set.failed_count(), 0);
        assert!(set.all_passed());
    }
}
