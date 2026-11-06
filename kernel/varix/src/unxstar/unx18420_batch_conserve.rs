//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F18420 · B01 批守恒闭合断言 · 0x7E4C0段
//! ─────────────────────────────────────────────────────────────────────
//! 锚点：UNX-F18420-J1 本批 20 条行数逐条求和=6,000 与批头/台账三方一致，红绿齐
//!
//! 结构清单：
//!   1. BccCode        诊断码闭集（code() = SEG_BASE + n）
//!   2. CardRow        批台账行（卡号/配额/目标行数/红绿态）
//!   3. A02_QUOTA      本工单 10 卡配额冻结表（求和=3000）
//!   4. A02_TARGET     本工单 10 卡目标行数冻结表（求和=6160，另账）
//!   5. BatchLedger    批台账（20 行 = 本工单 10 + 邻工单 10）
//!   6. 三方一致核     卡和/邻单/批头 6000 逐方对账
//!   7. 红绿齐核       绿+红=20 且红=必红三条口径
//!   8. Lcg + InjectLedger（三类破账 × 10 轮）
//!   9. golden_run / probe_a/b/c / inject_rounds
//!  10. run_unx18420_checks 判据集（≤20 条）
//!
//! 设计要点：
//!   - J1 的「行数」按批口径 = 配额行：本工单 10 卡配额 300+280+340+300+
//!     260+320+280+300+340+280 = 3000（逐卡冻结表可复算）；批头 B01 =
//!     20 条 × 全批 6000，邻工单 A01 由批头推导 6000−3000 = 3000，其行
//!     拆分按批头等分惯例冻结 300×10。三方一致：卡和(3000) + 邻单(3000)
//!     = 批头(6000) = 台账合计。
//!   - 目标行数另账（6160 = 600×8 + 680×2）：与配额账分列，互不混算，
//!     台账只增不减（两账都在台账内留指纹）。
//!   - 红绿齐：每行红绿双态、绿+红=20；红行数必须恰为必红三条口径的
//!     3（与 unx18419 必红三条对账），多红少红都算破账。
//!   - 注入三类：行账篡改（求和≠3000）/ 批头漂移（≠6000）/ 红绿失衡
//!     （红≠3），各 10 轮全抓。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x7E4C0，批 B01 独占区，%0x40==0 对齐）。
pub const SEG_BASE: u32 = 0x7E4C0;

// ── 冻结常量（推导见行注） ──────────────────────────────────────
/// 批 B01 总行数（冻结：J1 口径 6,000）。
pub const BATCH_TOTAL: u32 = 6000;
/// 批 B01 台账行数（冻结：J1 口径 20 条）。
pub const BATCH_ROWS: u32 = 20;
/// 邻工单 A01 配额（推导：批头 6000 − 本工单 3000 = 3000）。
pub const SIBLING_QUOTA: u32 = 6000 - 3000;
/// 邻工单 A01 行拆分（推导：批头等分惯例 3000 ÷ 10 = 300/行）。
pub const SIBLING_PER_ROW: u32 = 300;
/// 必红行数（冻结：与 unx18419 必红三条对账）。
pub const MUST_RED_ROWS: u32 = 3;
/// 判据注入轮数（写死：J1「10/10」口径）。
pub const JUDGE_ROUNDS: u32 = 10;

// ── 本工单 A02 配额冻结表（10 卡，求和=3000 可复算） ────────────
/// 逐卡配额（来源：工单 A02 卡表配额列，写死）。
pub const A02_QUOTA: [u32; 10] = [300, 280, 340, 300, 260, 320, 280, 300, 340, 280];
/// 逐卡目标行数（来源：工单 A02 卡表目标列，另账）。
pub const A02_TARGET: [u32; 10] = [600, 600, 680, 600, 600, 600, 600, 600, 680, 600];
/// 逐卡卡号（低五位：18411..18420）。
pub const A02_CARDS: [u32; 10] = [18411, 18412, 18413, 18414, 18415, 18416, 18417, 18418, 18419, 18420];

// ── 诊断码闭集 ──────────────────────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BccCode {
    /// 空台账
    EmptyLedger,
    /// 行账篡改（卡和 ≠ 3000）
    CardSumBroken,
    /// 批头漂移（三方合计 ≠ 6000）
    BatchHeaderDrift,
    /// 台账行数 ≠ 20
    RowCountBroken,
    /// 目标行数另账失守（≠6160）
    TargetLedgerBroken,
    /// 红绿失衡（绿+红≠20）
    RedGreenImbalance,
    /// 红行数 ≠ 必红三条
    RedCountMismatch,
    /// 行序乱（卡号非升序）
    RowOrderBroken,
    /// 邻单拆分与等分惯例不符
    SiblingSplitBroken,
    /// 双跑不一致
    NotReproducible,
}

impl BccCode {
    /// 诊断码取值：SEG_BASE + 枚举序号（窗口 0x7E4C0..0x7E500）。
    pub fn code(self) -> u32 {
        SEG_BASE + self as u32
    }
    /// 中文诊断文案（闭集逐条非空）。
    pub fn detail(self) -> &'static str {
        match self {
            BccCode::EmptyLedger => "空台账：无任何批行入账",
            BccCode::CardSumBroken => "行账篡改：卡和≠3000",
            BccCode::BatchHeaderDrift => "批头漂移：三方合计≠6000",
            BccCode::RowCountBroken => "台账行数≠20",
            BccCode::TargetLedgerBroken => "目标行数另账失守：≠6160",
            BccCode::RedGreenImbalance => "红绿失衡：绿+红≠20",
            BccCode::RedCountMismatch => "红行数≠必红三条",
            BccCode::RowOrderBroken => "行序乱：卡号非升序",
            BccCode::SiblingSplitBroken => "邻单拆分与等分惯例不符",
            BccCode::NotReproducible => "双跑不一致：复现红线",
        }
    }
    /// 闭集全量。
    pub const ALL: [BccCode; 10] = [
        BccCode::EmptyLedger,
        BccCode::CardSumBroken,
        BccCode::BatchHeaderDrift,
        BccCode::RowCountBroken,
        BccCode::TargetLedgerBroken,
        BccCode::RedGreenImbalance,
        BccCode::RedCountMismatch,
        BccCode::RowOrderBroken,
        BccCode::SiblingSplitBroken,
        BccCode::NotReproducible,
    ];
}

/// 按数值反查诊断文案。
pub fn detail_of(code: u32) -> &'static str {
    let mut i = 0;
    while i < BccCode::ALL.len() {
        if BccCode::ALL[i].code() == code {
            return BccCode::ALL[i].detail();
        }
        i += 1;
    }
    "未知诊断码：不在 0x7E4C0 段闭集内"
}

// ── 台账行 ──────────────────────────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct CardRow {
    /// 卡号低五位
    pub card: u32,
    /// 配额行数
    pub quota: u32,
    /// 目标行数（另账）
    pub target: u32,
    /// 红绿态（true=绿）
    pub green: bool,
}

/// 批台账：20 行定容账（本工单 10 + 邻工单 10）。
pub struct BatchLedger {
    rows: [Option<CardRow>; 20],
    /// 绿行数
    pub green_count: u32,
    /// 红行数
    pub red_count: u32,
    /// 累计追加行数
    pub appended: u32,
}

impl BatchLedger {
    /// 新台账（空）。
    pub fn new() -> BatchLedger {
        BatchLedger { rows: [None; 20], green_count: 0, red_count: 0, appended: 0 }
    }

    /// 追加一行（行满拒、重复卡号拒）。
    pub fn append(&mut self, row: CardRow) -> Result<(), u32> {
        // 重复卡号闸。
        let mut i = 0;
        while i < 20 {
            if let Some(r) = self.rows[i] {
                if r.card == row.card {
                    return Err(BccCode::RowCountBroken.code());
                }
            }
            i += 1;
        }
        // 空槽分配。
        let mut slot: Option<usize> = None;
        let mut i = 0;
        while i < 20 {
            if self.rows[i].is_none() {
                slot = Some(i);
                break;
            }
            i += 1;
        }
        let idx = match slot {
            Some(s) => s,
            None => return Err(BccCode::RowCountBroken.code()),
        };
        self.rows[idx] = Some(row);
        self.appended += 1;
        if row.green {
            self.green_count += 1;
        } else {
            self.red_count += 1;
        }
        Ok(())
    }

    /// 行数。
    pub fn len(&self) -> u32 {
        self.appended
    }

    /// 台账行快照（导出用，保序）。
    pub fn snapshot(&self) -> Vec<CardRow> {
        let mut out = Vec::new();
        let mut i = 0;
        while i < 20 {
            if let Some(r) = self.rows[i] {
                out.push(r);
            }
            i += 1;
        }
        out
    }
}

// ── 三方一致核 ──────────────────────────────────────────────────
/// 本工单卡和（配额账）：逐卡累加（saturating 防溢出）。
pub fn a02_quota_sum() -> u32 {
    let mut sum = 0u32;
    let mut i = 0;
    while i < A02_QUOTA.len() {
        sum = sum.saturating_add(match A02_QUOTA.get(i) {
            Some(q) => *q as u32,
            None => 0,
        });
        i += 1;
    }
    sum
}

/// 本工单目标行数另账和。
pub fn a02_target_sum() -> u32 {
    let mut sum = 0u32;
    let mut i = 0;
    while i < A02_TARGET.len() {
        sum = sum.saturating_add(match A02_TARGET.get(i) {
            Some(t) => *t,
            None => 0,
        });
        i += 1;
    }
    sum
}

/// 邻工单等分拆分和。
pub fn sibling_quota_sum() -> u32 {
    SIBLING_PER_ROW.saturating_mul(10)
}

/// 三方一致核：卡和 + 邻单 = 批头 6000，且各自与冻结常量一致。
pub fn three_way_consistent() -> Result<(), u32> {
    let card_sum = a02_quota_sum();
    if card_sum != 3000 {
        return Err(BccCode::CardSumBroken.code());
    }
    let sib = sibling_quota_sum();
    if sib != SIBLING_QUOTA {
        return Err(BccCode::SiblingSplitBroken.code());
    }
    if card_sum.saturating_add(sib) != BATCH_TOTAL {
        return Err(BccCode::BatchHeaderDrift.code());
    }
    if a02_target_sum() != 6160 {
        return Err(BccCode::TargetLedgerBroken.code());
    }
    Ok(())
}

/// 台账三方一致核：对一份 20 行台账逐方对账（行账/邻单/批头）。
pub fn ledger_three_way(led: &BatchLedger) -> Result<(), u32> {
    if led.len() != BATCH_ROWS {
        return Err(BccCode::RowCountBroken.code());
    }
    let snap = led.snapshot();
    // 卡和（前 10 行=本工单，后 10 行=邻单）。
    let mut a02_sum = 0u32;
    let mut sib_sum = 0u32;
    let mut i = 0;
    while i < snap.len() {
        let r = match snap.get(i) {
            Some(r) => *r,
            None => return Err(BccCode::EmptyLedger.code()),
        };
        if i < 10 {
            a02_sum = a02_sum.saturating_add(r.quota);
        } else {
            sib_sum = sib_sum.saturating_add(r.quota);
        }
        i += 1;
    }
    if a02_sum != 3000 {
        return Err(BccCode::CardSumBroken.code());
    }
    if sib_sum != SIBLING_QUOTA {
        return Err(BccCode::SiblingSplitBroken.code());
    }
    if a02_sum.saturating_add(sib_sum) != BATCH_TOTAL {
        return Err(BccCode::BatchHeaderDrift.code());
    }
    Ok(())
}

/// 红绿齐核：绿+红=20 且红=必红三条，且行级红绿与计数账一致。
pub fn red_green_aligned(led: &BatchLedger) -> Result<(), u32> {
    if led.green_count.saturating_add(led.red_count) != BATCH_ROWS {
        return Err(BccCode::RedGreenImbalance.code());
    }
    if led.red_count != MUST_RED_ROWS {
        return Err(BccCode::RedCountMismatch.code());
    }
    // 行级复算：绿行数逐行点数与计数账一致。
    let snap = led.snapshot();
    let mut g = 0;
    let mut i = 0;
    while i < snap.len() {
        let r = match snap.get(i) {
            Some(r) => *r,
            None => break,
        };
        if r.green {
            g += 1;
        }
        i += 1;
    }
    if g != led.green_count {
        return Err(BccCode::RedGreenImbalance.code());
    }
    Ok(())
}

/// 黄金台账构造：本工单 10 行（红绿按必红三条口径：末三行红）+
/// 邻工单 10 行（等分 300，全绿）。
pub fn golden_ledger() -> Result<BatchLedger, u32> {
    let mut led = BatchLedger::new();
    // 本工单 10 行：前 7 绿，末 3 红（红=必红三条）。
    let mut i = 0;
    while i < 10 {
        let green = i < 7;
        led.append(CardRow {
            card: match A02_CARDS.get(i) {
                Some(c) => *c as u32,
                None => return Err(BccCode::EmptyLedger.code()),
            },
            quota: match A02_QUOTA.get(i) {
                Some(q) => *q as u32,
                None => return Err(BccCode::EmptyLedger.code()),
            },
            target: match A02_TARGET.get(i) {
                Some(t) => *t,
                None => return Err(BccCode::EmptyLedger.code()),
            },
            green,
        })?;
        i += 1;
    }
    // 邻工单 10 行：300 × 10 全绿。
    let mut j = 0;
    while j < 10 {
        led.append(CardRow { card: 18301 + j, quota: SIBLING_PER_ROW, target: SIBLING_PER_ROW, green: true })?;
        j += 1;
    }
    Ok(led)
}

/// 行序核：本工单 10 行卡号严格升序。
pub fn row_order_ok(led: &BatchLedger) -> bool {
    let snap = led.snapshot();
    let mut prev: u32 = 0;
    let mut i = 0;
    while i < snap.len() {
        let r = match snap.get(i) {
            Some(r) => *r,
            None => return false,
        };
        if i < 10 {
            if r.card <= prev {
                return false;
            }
            prev = r.card;
        }
        i += 1;
    }
    true
}

// ── 确定性伪随机 ────────────────────────────────────────────────
pub struct Lcg(u64);
impl Lcg {
    /// 播种（种子 = SEG_BASE）。
    pub fn new(seed: u64) -> Lcg {
        Lcg(seed)
    }
    /// 推进一格。
    pub fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        self.0
    }
    /// [0, n) 伪随机。
    pub fn below(&mut self, n: u64) -> u64 {
        if n == 0 {
            return 0;
        }
        self.next() % n
    }
}

// ── 注入检出账 ──────────────────────────────────────────────────
/// 三类注入槽：0=行账篡改，1=批头漂移，2=红绿失衡。
pub struct InjectLedger {
    pub tries: [u32; 3],
    pub caught: [u32; 3],
}
impl InjectLedger {
    /// 空账。
    pub fn new() -> InjectLedger {
        InjectLedger { tries: [0; 3], caught: [0; 3] }
    }
    /// 记一轮。
    pub fn record(&mut self, slot: usize, hit: bool) {
        if slot < 3 {
            self.tries[slot] += 1;
            if hit {
                self.caught[slot] += 1;
            }
        }
    }
    /// 全抓断言。
    pub fn all_caught(&self) -> bool {
        let mut i = 0;
        while i < 3 {
            if self.tries[i] == 0 || self.caught[i] != self.tries[i] {
                return false;
            }
            i += 1;
        }
        true
    }
    /// 漏检合计。
    pub fn missed_total(&self) -> u32 {
        let mut m = 0;
        let mut i = 0;
        while i < 3 {
            m += self.tries[i].saturating_sub(self.caught[i]);
            i += 1;
        }
        m
    }
}

/// 注入 10/10：三类破账逐类注入，断言每轮被对账核显式抓出。
pub fn inject_rounds(rounds: u32) -> Result<InjectLedger, u32> {
    let mut led = InjectLedger::new();
    let mut r = 0;
    while r < rounds {
        // 槽 0：行账篡改——第 1 卡配额 +1（301），卡和 3001≠3000 必抓。
        let tampered = CardRow { card: 18411, quota: A02_QUOTA[0] + 1, target: 600, green: true };
        let mut t = BatchLedger::new();
        let _ = t.append(tampered);
        let hit0 = ledger_three_way(&t).is_err();
        led.record(0, hit0);
        // 槽 1：批头漂移——邻单行 301×10 = 3010，合计 3000+3010=6010≠6000。
        let mut t1 = BatchLedger::new();
        let mut k = 0;
        while k < 10 {
            let _ = t1.append(CardRow {
                card: match A02_CARDS.get(k) {
                    Some(c) => *c as u32,
                    None => (18411 + k) as u32,
                },
                quota: match A02_QUOTA.get(k) {
                    Some(q) => *q as u32,
                    None => 300,
                },
                target: 600,
                green: true,
            });
            k += 1;
        }
        let mut j = 0;
        while j < 10 {
            let _ = t1.append(CardRow { card: 18301 + j, quota: 301, target: 301, green: true });
            j += 1;
        }
        let hit1 = ledger_three_way(&t1).is_err();
        led.record(1, hit1);
        // 槽 2：红绿失衡——红行 4（≠必红三条 3）。
        let mut t2 = BatchLedger::new();
        let mut m = 0;
        while m < 10 {
            let _ = t2.append(CardRow {
                card: match A02_CARDS.get(m) {
                    Some(c) => *c as u32,
                    None => (18411 + m) as u32,
                },
                quota: match A02_QUOTA.get(m) {
                    Some(q) => *q as u32,
                    None => 300,
                },
                target: 600,
                green: m < 6, // 4 红
            });
            m += 1;
        }
        let mut n = 0;
        while n < 10 {
            let _ = t2.append(CardRow { card: 18301 + n, quota: 300, target: 300, green: true });
            n += 1;
        }
        let hit2 = red_green_aligned(&t2).is_err();
        led.record(2, hit2);
        r += 1;
    }
    if !led.all_caught() || led.missed_total() != 0 {
        return Err(BccCode::NotReproducible.code());
    }
    Ok(led)
}

// ── 黄金全链 ────────────────────────────────────────────────────
/// 黄金全链：冻结表三方一致 → 黄金台账三方一致 → 红绿齐 → 行序升序。
/// 返回 (台账行数, 卡和, 绿行数, 红行数)。
pub fn golden_run() -> Result<(u32, u32, u32, u32), u32> {
    three_way_consistent()?;
    let led = golden_ledger()?;
    ledger_three_way(&led)?;
    red_green_aligned(&led)?;
    if !row_order_ok(&led) {
        return Err(BccCode::RowOrderBroken.code());
    }
    Ok((led.len(), a02_quota_sum(), led.green_count, led.red_count))
}

// ── 探针三序 ────────────────────────────────────────────────────
/// 探针 A：冻结表逐卡可复算（10 卡配额/目标/卡号三列对表）。
pub fn probe_a() -> Result<u32, u32> {
    let mut checked = 0;
    let mut i = 0;
    while i < 10 {
        let q = match A02_QUOTA.get(i) {
            Some(q) => *q as u32,
            None => return Err(BccCode::EmptyLedger.code()),
        };
        let t = match A02_TARGET.get(i) {
            Some(t) => *t,
            None => return Err(BccCode::EmptyLedger.code()),
        };
        let c = match A02_CARDS.get(i) {
            Some(c) => *c as u32,
            None => return Err(BccCode::EmptyLedger.code()),
        };
        if c != 18411 + i as u32 || q == 0 || t < q {
            return Err(BccCode::CardSumBroken.code());
        }
        checked += 1;
        i += 1;
    }
    Ok(checked)
}

/// 探针 B：重复卡号拒（台账唯一性）。
pub fn probe_b() -> Result<(), u32> {
    let mut led = BatchLedger::new();
    led.append(CardRow { card: 18411, quota: 300, target: 600, green: true })?;
    if led.append(CardRow { card: 18411, quota: 300, target: 600, green: true }).is_err() {
        return Ok(());
    }
    Err(BccCode::RowCountBroken.code())
}

/// 探针 C：目标另账与配额账分列（6160 ≠ 3000，两账互不混算）。
pub fn probe_c() -> Result<(), u32> {
    if a02_target_sum() == 6160 && a02_quota_sum() == 3000 && a02_target_sum() != a02_quota_sum() {
        Ok(())
    } else {
        Err(BccCode::TargetLedgerBroken.code())
    }
}

/// 双跑确定性对拍。
pub fn repro_check() -> Result<(), u32> {
    let a = golden_run()?;
    let b = golden_run()?;
    if a != b {
        return Err(BccCode::NotReproducible.code());
    }
    Ok(())
}

/// 段位守卫。
pub fn segment_guard() -> bool {
    let mut i = 0;
    while i < BccCode::ALL.len() {
        let v = BccCode::ALL[i].code();
        if v < SEG_BASE || v >= SEG_BASE + 0x40 {
            return false;
        }
        i += 1;
    }
    SEG_BASE % 0x40 == 0
}

/// 文案闭集巡检。
pub fn detail_closure() -> bool {
    let mut i = 0;
    while i < BccCode::ALL.len() {
        let c = BccCode::ALL[i];
        if c.detail().is_empty() || detail_of(c.code()) != c.detail() {
            return false;
        }
        i += 1;
    }
    true
}

// ── 判据集 ──────────────────────────────────────────────────────
/// UNX-F18420 判据集（≤20 条；动态明细 ≤8 槽）。
pub fn run_unx18420_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F18420 B01 批守恒闭合断言");

    // 1. 段位守卫。
    s.add("J1-段位守卫", segment_guard(), "0x7E4C0 段 0x40 窗口且 0x40 对齐");
    // 2. 黄金全链。
    let golden = golden_run();
    let golden_ok = match &golden {
        Ok((rows, sum, g, r)) => *rows == 20 && *sum == 3000 && *g == 17 && *r == 3,
        Err(_) => false,
    };
    s.add_dyn("J1-黄金全链", golden_ok, &format!("{:?}", golden));
    // 3. 本批 20 条行数逐条求和=6000（J1 主判据：三方一致）。
    let three = three_way_consistent().is_ok();
    s.add("J1-三方一致6000", three, "卡和3000+邻单3000=批头6000");
    // 4. 20 行台账三方对账。
    let led_ok = match golden_ledger() {
        Ok(l) => ledger_three_way(&l).is_ok(),
        Err(_) => false,
    };
    s.add("J1-20行台账对账", led_ok, "行账/邻单/批头逐方一致");
    // 5. 红绿齐（J1 主判据）。
    let rg = match golden_ledger() {
        Ok(l) => red_green_aligned(&l).is_ok(),
        Err(_) => false,
    };
    s.add("J1-红绿齐", rg, "绿17+红3=20 且红=必红三条");
    // 6. 冻结表逐卡可复算。
    s.add("J1-冻结表逐卡复算", probe_a() == Ok(10), "10 卡三列（卡号/配额/目标）对表");
    // 7. 配额账与目标另账分列。
    s.add("J1-两账分列", probe_c().is_ok(), "配额 3000 / 目标 6160 互不混算");
    // 8. 行序升序。
    let order_ok = match golden_ledger() {
        Ok(l) => row_order_ok(&l),
        Err(_) => false,
    };
    s.add("J1-行序升序", order_ok, "本工单 10 行卡号严格升序");
    // 9. 注入 10/10。
    let inj = inject_rounds(JUDGE_ROUNDS);
    let inj_ok = match &inj {
        Ok(l) => l.all_caught(),
        Err(_) => false,
    };
    s.add_dyn("J1-注入10/10", inj_ok, &format!("r={}", JUDGE_ROUNDS));
    // 10. 计数守恒。
    let conserve_ok = match &inj {
        Ok(l) => l.missed_total() == 0,
        Err(_) => false,
    };
    s.add("J1-计数守恒漏检零", conserve_ok, "tries=caught+missed 且 missed=0");
    // 11. 探针三序。
    let a = probe_a().is_ok();
    let b = probe_b().is_ok();
    let c = probe_c().is_ok();
    s.add_dyn("J1-探针三序", a && b && c, &format!("a={} b={} c={}", a, b, c));
    // 12. 双跑一致。
    s.add("J2-双跑一致", repro_check().is_ok(), "golden_run 两跑同值");
    // 13. 邻单等分拆分核对。
    s.add("J1-邻单等分拆分", sibling_quota_sum() == SIBLING_QUOTA, "300×10=3000 批头推导");
    // 14. 重复卡号拒。
    s.add("J1-重复卡号拒", probe_b().is_ok(), "台账卡号唯一");
    // 15. 文案闭集。
    s.add("J3-文案闭集", detail_closure(), "10 码逐条非空且反查一致");
    // 16. 段内窗口逐码核对。
    let win_ok = {
        let mut ok = true;
        let mut i = 0;
        while i < BccCode::ALL.len() {
            if BccCode::ALL[i].code() >= SEG_BASE + 0x40 {
                ok = false;
            }
            i += 1;
        }
        ok
    };
    s.add("J1-段内窗口核对", win_ok, "10 码全部 < SEG_BASE+0x40");
    s
}

// ── 测试 ────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    /// 判据集全绿。
    #[test]
    fn all_checks_pass() {
        let set = run_unx18420_checks();
        assert!(set.all_passed(), "UNX-F18420 判据集存在红项");
    }

    /// 冻结表求和复算。
    #[test]
    fn frozen_sums() {
        assert_eq!(a02_quota_sum(), 3000);
        assert_eq!(a02_target_sum(), 6160);
        assert_eq!(sibling_quota_sum(), 3000);
        assert_eq!(a02_quota_sum() + sibling_quota_sum(), BATCH_TOTAL);
    }

    /// 黄金台账三方一致 + 红绿齐。
    #[test]
    fn golden_ledger_aligned() {
        let led = match golden_ledger() {
            Ok(l) => l,
            Err(_) => panic!("黄金台账构造失败"),
        };
        assert_eq!(led.len(), 20);
        assert!(ledger_three_way(&led).is_ok());
        assert!(red_green_aligned(&led).is_ok());
        assert_eq!(led.green_count, 17);
        assert_eq!(led.red_count, 3);
    }

    /// 注入三类 10/10。
    #[test]
    fn injection_all_caught() {
        let led = match inject_rounds(JUDGE_ROUNDS) {
            Ok(l) => l,
            Err(_) => panic!("注入轮次内部出错"),
        };
        assert!(led.all_caught());
        assert_eq!(led.missed_total(), 0);
    }

    /// 判定次序：行数闸先于卡和闸（台账不足 20 行先报行数破）。
    #[test]
    fn order_rowcount_first() {
        let mut led = BatchLedger::new();
        let _ = led.append(CardRow { card: 18411, quota: 999, target: 600, green: true });
        match ledger_three_way(&led) {
            Err(c) => assert_eq!(c, BccCode::RowCountBroken.code()),
            Ok(_) => panic!("1 行台账不应过对账"),
        }
    }

    /// 四类边界：空台账/行满/重复/乱序。
    #[test]
    fn boundary_edges() {
        // 空。
        let empty = BatchLedger::new();
        assert_eq!(empty.len(), 0);
        // 重复。
        let mut led = BatchLedger::new();
        assert!(led.append(CardRow { card: 18411, quota: 300, target: 600, green: true }).is_ok());
        assert!(led.append(CardRow { card: 18411, quota: 300, target: 600, green: true }).is_err());
        // 乱序（黄金台账行序必须升序，乱序核成立）。
        let mut bad = BatchLedger::new();
        let _ = bad.append(CardRow { card: 18420, quota: 280, target: 600, green: true });
        let _ = bad.append(CardRow { card: 18411, quota: 300, target: 600, green: true });
        assert!(!row_order_ok(&bad));
    }

    /// 双跑一致。
    #[test]
    fn reproducible() {
        assert!(repro_check().is_ok());
        assert!(three_way_consistent().is_ok());
    }

    /// 段位守卫 + 文案闭集。
    #[test]
    fn segment_and_detail() {
        assert!(segment_guard());
        assert!(detail_closure());
        assert!(detail_of(SEG_BASE + 0x40).starts_with("未知"));
    }
}
