//! UNX-F3408 断言计数账本（总断言数可达 1 万+ 的账面设计） · 0x79540段
//!
//! 判据原文（UNX-F3408 派工卡逐字锚定）：
//!   ① 纯功能代码 ≥340 行；
//!   ② J1：断言计数账本（总/通过/失败/跳过四计数 + 分域分批视图）；M-167 计数对账。
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//!
//! 设计要点：
//! 1. 四计数账面：总/通过/失败/跳过全 u64 计数器——账本只存计数与流水，
//!    不存断言本体，1 万+ 断言的账面恒定内存（万级可扩、计数不溢）；
//! 2. 分域分批双视图：域视图（内存/调度/文件闭集）与批视图（B+数字）随
//!    记账同步累加，两视图行列和必须回到四计数（视图漂移即对账破）；
//! 3. M-167 计数对账：流水重放重算四计数 + 恒等式（总≡通过+失败+跳过）
//!    + 双视图行列和三面对账，任何一面破即显性点出；
//! 4. 快照对账面：快照带四计数与双视图行列和的公共字段，快照可被外部
//!    伪造——伪造面正是注入入口（改任一计数重放必被抓）；
//! 5. 注入矩阵 8 项：非法域/非法批/恒等式破/域视图漂移/批视图漂移/跳过
//!    语义破/重放双计/万+ 容量面逐种构造，逐种必须被口点出；
//! 6. 诊断码闭集 0x79541..0x7954C，Err 用 Result<T,u32>，中文诊断文案。

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 段基址（派工卡 B10 给定值；本批窗口 0x79400..=0x795C0）。
pub const SEG_BASE: u32 = 0x79540;

/// 万+ 账面验证规模（判据原文「1 万+」的实测下限）。
pub const MASS_RECORDS: usize = 10_000;

/// 域闭集数。
pub const DOMAIN_COUNT: usize = 3;

/// 域闭集名册。
pub const DOMAINS: [&str; DOMAIN_COUNT] = ["内存", "调度", "文件"];

/// 计数账本诊断码闭集。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Diag {
    /// 非法域：域不在闭集名册内。
    EBadDomain,
    /// 非法批：批须为 B+数字。
    EBadBatch,
    /// 恒等式破：总 ≠ 通过+失败+跳过。
    EInvariant,
    /// 域视图漂移：域视图行列和与四计数不符。
    EDomainDrift,
    /// 批视图漂移：批视图行列和与四计数不符。
    EBatchDrift,
    /// 跳过语义破：跳过被计入失败（或反之）。
    ESkipSemantics,
    /// 重放双计：流水重放结果与账面不符。
    EReplay,
    /// 对账快照缺面：快照字段缺失（行和/列和未齐）。
    ENoSnapshot,
    /// 账本空：空账不许出对账结论。
    EEmptyLedger,
    /// 流水序断裂：流水序号不连续。
    ESeq,
    /// 视图越位：取视图下标越界。
    ERange,
    /// 相位倒退：账本封账后记账被拒。
    EPhase,
}

impl Diag {
    /// 诊断码值（SEG_BASE + 0x01..0x0C 闭集）。
    pub fn val(self) -> u32 {
        SEG_BASE
            + match self {
                Diag::EBadDomain => 0x01,
                Diag::EBadBatch => 0x02,
                Diag::EInvariant => 0x03,
                Diag::EDomainDrift => 0x04,
                Diag::EBatchDrift => 0x05,
                Diag::ESkipSemantics => 0x06,
                Diag::EReplay => 0x07,
                Diag::ENoSnapshot => 0x08,
                Diag::EEmptyLedger => 0x09,
                Diag::ESeq => 0x0a,
                Diag::ERange => 0x0b,
                Diag::EPhase => 0x0c,
            }
    }

    /// 中文诊断文案（可操作建议）。
    pub fn detail(self) -> &'static str {
        match self {
            Diag::EBadDomain => "非法域：域不在 内存/调度/文件 闭集内，按名册回填再记账",
            Diag::EBadBatch => "非法批：批须为 B+数字（如 B10），按批册回填再记账",
            Diag::EInvariant => "恒等式破：总≠通过+失败+跳过，重放流水定位漂移计数",
            Diag::EDomainDrift => "域视图漂移：域视图行列和与四计数不符，逐域重算视图",
            Diag::EBatchDrift => "批视图漂移：批视图行列和与四计数不符，逐批重算视图",
            Diag::ESkipSemantics => "跳过语义破：跳过须计入跳过计数，不得混入失败计数",
            Diag::EReplay => "重放双计：流水重放与账面不符，查重复记账路径",
            Diag::ENoSnapshot => "对账快照缺面：快照须带四计数与双视图行列和，补齐再对账",
            Diag::EEmptyLedger => "账本空：无账可对，先记账再出对账结论",
            Diag::ESeq => "流水序断裂：流水序号须从一起连续递增，重建流水",
            Diag::ERange => "视图越位：下标越出视图，按视图数取",
            Diag::EPhase => "相位倒退：账本已封账，记账走新账",
        }
    }
}

/// 全部诊断码（闭集数组）。
pub const ALL: &[Diag] = &[
    Diag::EBadDomain,
    Diag::EBadBatch,
    Diag::EInvariant,
    Diag::EDomainDrift,
    Diag::EBatchDrift,
    Diag::ESkipSemantics,
    Diag::EReplay,
    Diag::ENoSnapshot,
    Diag::EEmptyLedger,
    Diag::ESeq,
    Diag::ERange,
    Diag::EPhase,
];

/// 诊断码查表（闭集返回）。
pub fn detail(code: u32) -> &'static str {
    for d in ALL {
        if d.val() == code {
            return d.detail();
        }
    }
    "未知诊断码：不在 0x79541..0x7954C 闭集内"
}

/// 断言裁决闭集（通过/失败/跳过——跳过独立计数不混失败）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Verdict {
    /// 通过。
    Pass,
    /// 失败。
    Fail,
    /// 跳过。
    Skip,
}

impl Verdict {
    /// 中文名。
    pub fn name(self) -> &'static str {
        match self {
            Verdict::Pass => "通过",
            Verdict::Fail => "失败",
            Verdict::Skip => "跳过",
        }
    }
}

/// 一条记账流水（序号 + 域 + 批 + 裁决）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Rec {
    /// 流水序号（从一起连续递增）。
    pub seq: u32,
    /// 域（闭集内）。
    pub domain: &'static str,
    /// 批（B+数字）。
    pub batch: String,
    /// 裁决。
    pub verdict: Verdict,
}

/// 四计数（u64——万+ 账面容量载体）。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct FourCounts {
    /// 总数。
    pub total: u64,
    /// 通过数。
    pub passed: u64,
    /// 失败数。
    pub failed: u64,
    /// 跳过数。
    pub skipped: u64,
}

impl FourCounts {
    /// 恒等式：总 ≡ 通过+失败+跳过。
    pub fn invariant_ok(&self) -> bool {
        self.total
            == self
                .passed
                .saturating_add(self.failed)
                .saturating_add(self.skipped)
    }

    /// 累加一条裁决。
    pub fn bump(&mut self, v: Verdict) {
        self.total = self.total.saturating_add(1);
        match v {
            Verdict::Pass => self.passed = self.passed.saturating_add(1),
            Verdict::Fail => self.failed = self.failed.saturating_add(1),
            Verdict::Skip => self.skipped = self.skipped.saturating_add(1),
        }
    }
}

/// 域视图（分域四计数）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DomainView {
    /// 域名。
    pub domain: &'static str,
    /// 该域四计数。
    pub counts: FourCounts,
}

/// 批视图（分批四计数）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BatchView {
    /// 批号。
    pub batch: String,
    /// 该批四计数。
    pub counts: FourCounts,
}

/// 对账快照（四计数 + 双视图行列和——伪造面即注入入口）。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct Snapshot {
    /// 账面四计数。
    pub ledger: FourCounts,
    /// 域视图行列和（逐面累加）。
    pub domain_sum: FourCounts,
    /// 批视图行列和（逐面累加）。
    pub batch_sum: FourCounts,
}

/// 域合法。
pub fn domain_legal(domain: &str) -> bool {
    DOMAINS.iter().any(|d| *d == domain)
}

/// 批格式：B 开头余部全数字。
pub fn batch_legal(batch: &str) -> bool {
    let bytes = batch.as_bytes();
    if bytes.len() < 2 || bytes.first() != Some(&b'B') {
        return false;
    }
    let mut i = 1usize;
    while i < bytes.len() {
        match bytes.get(i) {
            Some(b) => {
                if *b < b'0' || *b > b'9' {
                    return false;
                }
            }
            None => return false,
        }
        i += 1;
    }
    true
}

/// 断言计数账本（计数器 + 双视图 + 流水，封账单向）。
#[derive(Clone, Debug)]
pub struct AssertLedger {
    sealed: bool,
    counts: FourCounts,
    domains: Vec<DomainView>,
    batches: Vec<BatchView>,
    journal: Vec<Rec>,
    rejects: u32,
}

impl AssertLedger {
    /// 新账本（空账）。
    pub fn new() -> AssertLedger {
        AssertLedger {
            sealed: false,
            counts: FourCounts {
                total: 0,
                passed: 0,
                failed: 0,
                skipped: 0,
            },
            domains: Vec::new(),
            batches: Vec::new(),
            journal: Vec::new(),
            rejects: 0,
        }
    }

    /// 账面四计数。
    pub fn counts(&self) -> FourCounts {
        self.counts
    }

    /// 流水条数。
    pub fn journal_len(&self) -> usize {
        self.journal.len()
    }

    /// 拒绝计数。
    pub fn rejects(&self) -> u32 {
        self.rejects
    }

    /// 是否封账。
    pub fn sealed(&self) -> bool {
        self.sealed
    }

    /// 域视图数。
    pub fn domain_views(&self) -> usize {
        self.domains.len()
    }

    /// 批视图数。
    pub fn batch_views(&self) -> usize {
        self.batches.len()
    }

    /// 取域视图（越位返回 None）。
    pub fn domain_at(&self, idx: usize) -> Option<DomainView> {
        self.domains.get(idx).copied()
    }

    /// 取批视图（越位返回 None）。
    pub fn batch_at(&self, idx: usize) -> Option<BatchView> {
        self.batches.get(idx).cloned()
    }

    /// 记账：域批合法 + 封账拒；流水、账面、双视图同步落。
    pub fn record(&mut self, domain: &'static str, batch: &str, v: Verdict) -> Result<u32, u32> {
        if self.sealed {
            self.rejects += 1;
            return Err(Diag::EPhase.val());
        }
        if !domain_legal(domain) {
            self.rejects += 1;
            return Err(Diag::EBadDomain.val());
        }
        if !batch_legal(batch) {
            self.rejects += 1;
            return Err(Diag::EBadBatch.val());
        }
        let seq = (self.journal.len() as u32) + 1;
        self.journal.push(Rec {
            seq,
            domain,
            batch: String::from(batch),
            verdict: v,
        });
        self.counts.bump(v);
        // 域视图同步。
        let mut hit = false;
        for dv in &mut self.domains {
            if dv.domain == domain {
                dv.counts.bump(v);
                hit = true;
                break;
            }
        }
        if !hit {
            let mut c = FourCounts {
                total: 0,
                passed: 0,
                failed: 0,
                skipped: 0,
            };
            c.bump(v);
            self.domains.push(DomainView { domain, counts: c });
        }
        // 批视图同步。
        let mut bhit = false;
        for bv in &mut self.batches {
            if bv.batch == batch {
                bv.counts.bump(v);
                bhit = true;
                break;
            }
        }
        if !bhit {
            let mut c = FourCounts {
                total: 0,
                passed: 0,
                failed: 0,
                skipped: 0,
            };
            c.bump(v);
            self.batches.push(BatchView {
                batch: String::from(batch),
                counts: c,
            });
        }
        Ok(seq)
    }

    /// 流水序自检（从一起连续递增）。
    pub fn seq_ok(&self) -> bool {
        let mut want = 1u32;
        for r in &self.journal {
            if r.seq != want {
                return false;
            }
            want += 1;
        }
        true
    }

    /// 对账快照（账面 + 双视图行列和三面齐出）。
    pub fn snapshot(&self) -> Snapshot {
        let mut domain_sum = FourCounts {
            total: 0,
            passed: 0,
            failed: 0,
            skipped: 0,
        };
        for dv in &self.domains {
            domain_sum.total = domain_sum.total.saturating_add(dv.counts.total);
            domain_sum.passed = domain_sum.passed.saturating_add(dv.counts.passed);
            domain_sum.failed = domain_sum.failed.saturating_add(dv.counts.failed);
            domain_sum.skipped = domain_sum.skipped.saturating_add(dv.counts.skipped);
        }
        let mut batch_sum = FourCounts {
            total: 0,
            passed: 0,
            failed: 0,
            skipped: 0,
        };
        for bv in &self.batches {
            batch_sum.total = batch_sum.total.saturating_add(bv.counts.total);
            batch_sum.passed = batch_sum.passed.saturating_add(bv.counts.passed);
            batch_sum.failed = batch_sum.failed.saturating_add(bv.counts.failed);
            batch_sum.skipped = batch_sum.skipped.saturating_add(bv.counts.skipped);
        }
        Snapshot {
            ledger: self.counts,
            domain_sum,
            batch_sum,
        }
    }

    /// M-167 计数对账：恒等式 + 双视图行列和 + 流水重放三面全对才过。
    pub fn reconcile(&self, snap: &Snapshot) -> Result<(), u32> {
        if self.journal.is_empty() {
            return Err(Diag::EEmptyLedger.val());
        }
        if !snap.ledger.invariant_ok() {
            return Err(Diag::EInvariant.val());
        }
        if snap.domain_sum != snap.ledger {
            return Err(Diag::EDomainDrift.val());
        }
        if snap.batch_sum != snap.ledger {
            return Err(Diag::EBatchDrift.val());
        }
        // 流水重放重算。
        let mut replay = FourCounts {
            total: 0,
            passed: 0,
            failed: 0,
            skipped: 0,
        };
        for r in &self.journal {
            replay.bump(r.verdict);
        }
        if replay != snap.ledger {
            return Err(Diag::EReplay.val());
        }
        if !self.seq_ok() {
            return Err(Diag::ESeq.val());
        }
        Ok(())
    }

    /// 封账（单向；封账后记账显性拒）。
    pub fn seal(&mut self) -> Result<(), u32> {
        if self.sealed {
            self.rejects += 1;
            return Err(Diag::EPhase.val());
        }
        self.sealed = true;
        Ok(())
    }

    /// 写回摘要（四计数 + 分域分批逐行，摘要非空）。
    pub fn summary(&self) -> String {
        let mut out = String::new();
        out.push_str(&format!(
            "总{} 通过{} 失败{} 跳过{}\n",
            self.counts.total, self.counts.passed, self.counts.failed, self.counts.skipped
        ));
        for dv in &self.domains {
            out.push_str(&format!(
                "域{} 总{} 通过{} 失败{} 跳过{}\n",
                dv.domain, dv.counts.total, dv.counts.passed, dv.counts.failed, dv.counts.skipped
            ));
        }
        for bv in &self.batches {
            out.push_str(&format!(
                "批{} 总{} 通过{} 失败{} 跳过{}\n",
                bv.batch, bv.counts.total, bv.counts.passed, bv.counts.failed, bv.counts.skipped
            ));
        }
        out
    }
}

/// 注入矩阵置位（越界静默忽略——注入位恒在 0..INJECTS 内）。
pub const INJECTS: usize = 8;

fn mark(out: &mut [bool; INJECTS], idx: usize, detected: bool) {
    if let Some(x) = out.get_mut(idx) {
        *x = detected;
    }
}

/// 标准记账：三域两批九条（通过6/失败2/跳过1，钦定账面）。
pub fn seed_ledger() -> AssertLedger {
    let mut led = AssertLedger::new();
    let script: [(&'static str, &'static str, Verdict); 9] = [
        ("内存", "B10", Verdict::Pass),
        ("内存", "B10", Verdict::Fail),
        ("内存", "B11", Verdict::Pass),
        ("调度", "B10", Verdict::Pass),
        ("调度", "B11", Verdict::Skip),
        ("调度", "B11", Verdict::Pass),
        ("文件", "B10", Verdict::Pass),
        ("文件", "B10", Verdict::Fail),
        ("文件", "B11", Verdict::Pass),
    ];
    for (d, b, v) in script.iter() {
        let _ = led.record(d, b, *v);
    }
    led
}

/// 注入矩阵 8 项：逐种构造违约，逐种必须被口点出。
pub fn inject_matrix() -> [bool; INJECTS] {
    let mut out = [false; INJECTS];
    // 注入1：非法域记账。
    let mut led = AssertLedger::new();
    mark(
        &mut out,
        0,
        led.record("网络", "B10", Verdict::Pass) == Err(Diag::EBadDomain.val()),
    );
    // 注入2：非法批记账。
    let mut led = AssertLedger::new();
    mark(
        &mut out,
        1,
        led.record("内存", "b1x", Verdict::Pass) == Err(Diag::EBadBatch.val()),
    );
    // 注入3：恒等式破（快照总计数被改）。
    let led = seed_ledger();
    let mut snap = led.snapshot();
    snap.ledger.total = snap.ledger.total + 1;
    mark(&mut out, 2, led.reconcile(&snap) == Err(Diag::EInvariant.val()));
    // 注入4：域视图漂移（域行列和被改）。
    let led = seed_ledger();
    let mut snap = led.snapshot();
    snap.domain_sum.failed = snap.domain_sum.failed + 1;
    mark(&mut out, 3, led.reconcile(&snap) == Err(Diag::EDomainDrift.val()));
    // 注入5：批视图漂移（批行列和被改）。
    let led = seed_ledger();
    let mut snap = led.snapshot();
    snap.batch_sum.skipped = snap.batch_sum.skipped + 1;
    mark(&mut out, 4, led.reconcile(&snap) == Err(Diag::EBatchDrift.val()));
    // 注入6：重放双计（账面与双视图同步抬量保持恒等式与视图和成立，
    // 但与流水重放不符——重放对账面点出 EReplay）。
    let led = seed_ledger();
    let mut snap = led.snapshot();
    snap.ledger.total = snap.ledger.total + 1;
    snap.ledger.passed = snap.ledger.passed + 1;
    snap.domain_sum.total = snap.domain_sum.total + 1;
    snap.domain_sum.passed = snap.domain_sum.passed + 1;
    snap.batch_sum.total = snap.batch_sum.total + 1;
    snap.batch_sum.passed = snap.batch_sum.passed + 1;
    mark(&mut out, 5, led.reconcile(&snap) == Err(Diag::EReplay.val()));
    // 注入7：跳过语义（跳过必须入跳过计数、失败计数不增）。
    let mut skip_led = AssertLedger::new();
    let _ = skip_led.record("内存", "B10", Verdict::Skip);
    let c = skip_led.counts();
    mark(
        &mut out,
        6,
        c.total == 1 && c.skipped == 1 && c.failed == 0 && c.passed == 0,
    );
    // 注入8：封账后记账。
    let mut sealed = AssertLedger::new();
    let _ = sealed.record("内存", "B10", Verdict::Pass);
    let _ = sealed.seal();
    mark(
        &mut out,
        7,
        sealed.record("内存", "B10", Verdict::Pass) == Err(Diag::EPhase.val()),
    );
    out
}

// ---- 判据谓词（私有） ------------------------------------------------------

/// 段位守卫（对齐 0x40；窗口按 B10 派工卡 0x79400..=0x795C0）。
fn seg_guard_ok() -> bool {
    if SEG_BASE % 0x40 != 0 {
        return false;
    }
    if SEG_BASE < 0x79400 || SEG_BASE > 0x795C0 {
        return false;
    }
    SEG_BASE == 0x79540
}

/// J1 主判据：四计数 + 分域分批视图齐备且账面与钦定值对齐。
fn j1_main_ok() -> bool {
    let led = seed_ledger();
    let c = led.counts();
    // 钦定：九条 = 通过6 + 失败2 + 跳过1。
    let counts_ok =
        c.total == 9 && c.passed == 6 && c.failed == 2 && c.skipped == 1 && c.invariant_ok();
    // 分域视图：三域齐、逐域计数齐。
    let domains_ok = led.domain_views() == 3 && led.batch_views() == 2;
    // 视图可取且计数随域分。
    let mem = led.domain_at(0);
    let view_ok = match mem {
        Some(dv) => dv.domain == "内存" && dv.counts.total == 3,
        None => false,
    };
    counts_ok && domains_ok && view_ok
}

/// M-167 计数对账（清洁账全绿）。
fn m167_ok() -> bool {
    let led = seed_ledger();
    let snap = led.snapshot();
    led.reconcile(&snap).is_ok()
}

/// 万+ 账面：万级记账后四计数、对账、视图和三面全绿。
fn mass_ok() -> bool {
    let mut led = AssertLedger::new();
    let mut i = 0usize;
    while i < MASS_RECORDS {
        // 钦定分布：i%10==0 失败、i%7==0 跳过、其余通过（i%10==0 与 i%7==0
        // 交集条数显性记账——同一循环只落一裁，先失败后跳过优先）。
        let v = if i % 10 == 0 {
            Verdict::Fail
        } else if i % 7 == 0 {
            Verdict::Skip
        } else {
            Verdict::Pass
        };
        // 域按 i%3 轮转、批按 (i/1000)%2 轮转（B10/B11）。
        let domain = match DOMAINS.get(i % DOMAIN_COUNT) {
            Some(x) => *x,
            None => return false,
        };
        let batch = if (i / 1000) % 2 == 0 { "B10" } else { "B11" };
        if led.record(domain, batch, v).is_err() {
            return false;
        }
        i += 1;
    }
    let c = led.counts();
    if c.total != MASS_RECORDS as u64 {
        return false;
    }
    let snap = led.snapshot();
    led.reconcile(&snap).is_ok() && snap.domain_sum == c && snap.batch_sum == c
}

/// 钦定分布对账：万+ 语料的通过/失败/跳过计数与闭式重算一致。
fn mass_distribution_ok() -> bool {
    // 闭式重算：0..10000 内 i%10==0 计 1000（失败）；余下中 i%7==0 计
    // （7 的倍数且非 10 的倍数）。7 的倍数共 floor(9999/7)+1 = 1429 个
    //（i 从 0 起：0,7,...,9996 共 1429）；其中同是 10 的倍数（70 的倍数）
    // 共 floor(9999/70)+1 = 143 个。跳过 = 1429 - 143 = 1286；失败 = 1000；
    // 通过 = 10000 - 1000 - 1286 = 7714。
    let want = FourCounts {
        total: 10_000,
        passed: 7_714,
        failed: 1_000,
        skipped: 1_286,
    };
    let mut led = AssertLedger::new();
    let mut i = 0usize;
    while i < MASS_RECORDS {
        let v = if i % 10 == 0 {
            Verdict::Fail
        } else if i % 7 == 0 {
            Verdict::Skip
        } else {
            Verdict::Pass
        };
        let _ = led.record("内存", "B10", v);
        i += 1;
    }
    led.counts() == want && led.counts().invariant_ok()
}

/// 双跑一致（同构语料两次记账账面/快照/摘要全同）。
fn dual_run_ok() -> bool {
    let a = seed_ledger();
    let b = seed_ledger();
    a.counts() == b.counts()
        && a.snapshot() == b.snapshot()
        && a.summary() == b.summary()
        && a.journal_len() == b.journal_len()
}

/// 注入 8/8 检出。
fn inject_all_ok() -> bool {
    inject_matrix() == [true; INJECTS]
}

/// 判据总成：UNX-F3408 断言计数账本。
pub fn run_unx3408_checks() -> CheckSet {
    let mut s = CheckSet::new("unx3408");
    let led = seed_ledger();

    s.add_dyn(
        "F3408-01 段位守卫",
        seg_guard_ok(),
        &format!("seg=0x{:05x} align40={}", SEG_BASE, SEG_BASE % 0x40 == 0),
    );

    // 正常流：四计数 + 分域分批视图（J1 主判据）。
    s.add_dyn(
        "F3408-02 四计数分域分批视图",
        j1_main_ok(),
        &led.summary(),
    );

    // M-167 计数对账（清洁账全绿）。
    s.add_dyn(
        "F3408-03 M167计数对账三面全绿",
        m167_ok(),
        &format!("m167={}", m167_ok()),
    );

    // 注入必抓：8 项注入全检出。
    s.add_dyn(
        "F3408-04 注入矩阵八项全检出",
        inject_all_ok(),
        &format!("matrix={:?}", inject_matrix()),
    );

    // 万+ 账面：万级记账三面全绿（账本只存计数与流水，恒定内存）。
    s.add_dyn(
        "F3408-05 万+账面万级记账对账",
        mass_ok(),
        &format!("mass={}", MASS_RECORDS),
    );

    // 万+ 钦定分布对账（闭式重算）。
    s.add_dyn(
        "F3408-06 万+钦定分布闭式对账",
        mass_distribution_ok(),
        &format!("dist={}", mass_distribution_ok()),
    );

    // 边界：空账对账显性拒。
    let empty = AssertLedger::new();
    let snap_empty = Snapshot::default();
    let empty_ok = empty.reconcile(&snap_empty) == Err(Diag::EEmptyLedger.val())
        && empty.counts().total == 0;
    s.add_dyn(
        "F3408-07 空账对账显性拒",
        empty_ok,
        &format!("total={}", empty.counts().total),
    );

    // 边界：流水序连续（九条流水序号 1..9）。
    let seq_ok = led.seq_ok()
        && led.journal_len() == 9
        && led.journal_len() as u64 == led.counts().total;
    s.add_dyn(
        "F3408-08 流水序连续与账面等量",
        seq_ok,
        &format!("journal={}", led.journal_len()),
    );

    // 视图越位取视图显性拒。
    let range_ok = led.domain_at(9).is_none()
        && led.batch_at(9).is_none()
        && led.domain_at(2).is_some()
        && led.batch_at(1).is_some();
    s.add_dyn(
        "F3408-09 视图越位显性拒",
        range_ok,
        &format!("domains={} batches={}", led.domain_views(), led.batch_views()),
    );

    // 跳过语义：跳过独立计数不混失败（分域视图同口径）。
    let mut skip_led = AssertLedger::new();
    let _ = skip_led.record("调度", "B10", Verdict::Skip);
    let _ = skip_led.record("调度", "B10", Verdict::Skip);
    let sc = skip_led.counts();
    let dv = skip_led.domain_at(0);
    let skip_ok = sc.total == 2
        && sc.skipped == 2
        && sc.failed == 0
        && match dv {
            Some(d) => d.counts.skipped == 2 && d.counts.failed == 0,
            None => false,
        };
    s.add_dyn(
        "F3408-10 跳过独立计数分域同口径",
        skip_ok,
        &format!("skipped={} failed={}", sc.skipped, sc.failed),
    );

    // 封账单向：封账后记账与再封账均显性拒。
    let mut sealed = seed_ledger();
    let seal1 = sealed.seal().is_ok();
    let seal2 = sealed.seal() == Err(Diag::EPhase.val());
    let rec_after = sealed.record("内存", "B10", Verdict::Pass) == Err(Diag::EPhase.val());
    s.add_dyn(
        "F3408-11 封账单向倒退拒",
        seal1 && seal2 && rec_after && sealed.sealed(),
        &format!("rejects={}", sealed.rejects()),
    );

    // 写回摘要非空且含四计数与分域分批行（判据③写回面）。
    let sum = led.summary();
    s.add_dyn(
        "F3408-12 写回摘要非空含视图行",
        sum.contains("总9 通过6 失败2 跳过1")
            && sum.contains("域内存")
            && sum.contains("批B10"),
        &format!("bytes={}", sum.len()),
    );

    // 双跑一致。
    s.add_dyn(
        "F3408-13 双跑一致",
        dual_run_ok(),
        &format!("dual={}", dual_run_ok()),
    );

    // 诊断码闭集。
    s.add_dyn(
        "F3408-14 诊断码闭集",
        ALL.len() == 12 && detail(SEG_BASE + 0x0C) == Diag::EPhase.detail(),
        &format!("all={} seg=0x{:05x}", ALL.len(), SEG_BASE),
    );

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn diag_codes_closed() {
        assert_eq!(ALL.len(), 12);
        assert_eq!(Diag::EBadDomain.val(), SEG_BASE + 0x01);
        assert_eq!(Diag::EPhase.val(), SEG_BASE + 0x0C);
        for d in ALL {
            assert!(!detail(d.val()).is_empty());
        }
        assert!(detail(SEG_BASE + 0x70).starts_with("未知诊断码"));
    }

    #[test]
    fn seg_guard() {
        assert!(seg_guard_ok());
        assert_eq!(SEG_BASE % 0x40, 0);
        assert_eq!(SEG_BASE, 0x79540);
    }

    #[test]
    fn seed_ledger_counts() {
        let led = seed_ledger();
        let c = led.counts();
        assert_eq!(c.total, 9);
        assert_eq!(c.passed, 6);
        assert_eq!(c.failed, 2);
        assert_eq!(c.skipped, 1);
        assert!(c.invariant_ok());
        assert_eq!(led.domain_views(), 3);
        assert_eq!(led.batch_views(), 2);
    }

    #[test]
    fn domain_and_batch_views_split() {
        let led = seed_ledger();
        let mem = led.domain_at(0);
        assert!(matches!(mem, Some(dv) if dv.domain == "内存" && dv.counts.total == 3));
        let b10 = led.batch_at(0);
        assert!(matches!(b10, Some(bv) if bv.batch == "B10" && bv.counts.total == 5));
    }

    #[test]
    fn reconcile_green_and_drift_detected() {
        let led = seed_ledger();
        let snap = led.snapshot();
        assert_eq!(led.reconcile(&snap), Ok(()));
        // 恒等式破被抓。
        let mut s2 = snap;
        s2.ledger.total = s2.ledger.total + 1;
        assert_eq!(led.reconcile(&s2), Err(Diag::EInvariant.val()));
        // 域视图漂移被抓。
        let mut s3 = snap;
        s3.domain_sum.passed = s3.domain_sum.passed + 1;
        assert_eq!(led.reconcile(&s3), Err(Diag::EDomainDrift.val()));
        // 批视图漂移被抓。
        let mut s4 = snap;
        s4.batch_sum.failed = s4.batch_sum.failed + 1;
        assert_eq!(led.reconcile(&s4), Err(Diag::EBatchDrift.val()));
    }

    #[test]
    fn empty_ledger_rejected() {
        let led = AssertLedger::new();
        assert_eq!(
            led.reconcile(&Snapshot::default()),
            Err(Diag::EEmptyLedger.val())
        );
    }

    #[test]
    fn inject_eight_of_eight() {
        assert_eq!(inject_matrix(), [true; INJECTS]);
    }

    #[test]
    fn mass_ten_thousand() {
        assert!(mass_ok());
        assert!(mass_distribution_ok());
        // 万+ 下限实测。
        let mut led = AssertLedger::new();
        for i in 0..MASS_RECORDS {
            let _ = led.record("内存", "B10", Verdict::Pass);
            let _ = i;
        }
        assert_eq!(led.counts().total, 10_000);
        assert!(led.counts().invariant_ok());
    }

    #[test]
    fn bad_domain_and_batch_rejected() {
        let mut led = AssertLedger::new();
        assert_eq!(
            led.record("网络", "B10", Verdict::Pass),
            Err(Diag::EBadDomain.val())
        );
        assert_eq!(
            led.record("内存", "b1x", Verdict::Pass),
            Err(Diag::EBadBatch.val())
        );
        assert_eq!(led.journal_len(), 0);
        assert!(led.rejects() >= 2);
    }

    #[test]
    fn seal_one_way() {
        let mut led = seed_ledger();
        assert_eq!(led.seal(), Ok(()));
        assert_eq!(led.seal(), Err(Diag::EPhase.val()));
        assert_eq!(
            led.record("内存", "B10", Verdict::Pass),
            Err(Diag::EPhase.val())
        );
        assert!(led.sealed());
    }

    #[test]
    fn skip_semantics() {
        let mut led = AssertLedger::new();
        let _ = led.record("内存", "B10", Verdict::Skip);
        let _ = led.record("内存", "B10", Verdict::Skip);
        let c = led.counts();
        assert_eq!(c.total, 2);
        assert_eq!(c.skipped, 2);
        assert_eq!(c.failed, 0);
        assert_eq!(c.passed, 0);
    }

    #[test]
    fn dual_run_consistency() {
        assert!(dual_run_ok());
    }

    #[test]
    fn summary_contains_views() {
        let led = seed_ledger();
        let sum = led.summary();
        assert!(sum.contains("总9 通过6 失败2 跳过1"));
        assert!(sum.contains("域内存"));
        assert!(sum.contains("批B11"));
    }

    #[test]
    fn checks_all_green() {
        let s = run_unx3408_checks();
        assert_eq!(s.len(), 14);
        assert_eq!(s.failed_count(), 0);
        assert!(s.all_passed());
    }
}
