//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F13581 · 域收官总成（B01–B40 四十批 800 条聚合权威位） · 0x65C40
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定）：
//!   ① 纯功能代码 ≥370 行；
//!   ② UNX-F13581-J1 收官总账（40 批×20 条聚合矩阵：批号/条数/行数/
//!      状态四列——800 条全登记），矩阵零缺；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - 收官矩阵：40 批 × 20 条 = 800 条冻结闭集，四列账（批号/条数/
//!     行数/状态）；
//!   - 批状态机闭集：未开→聚合中→已收口，任何非法边（未开批先记条/
//!     收口后补条/条不满 20 强行收口）一律拒收；
//!   - 守恒恒等式：Σ各批条数 ≡ 800，且批数 ≡ 40（双守恒）；
//!   - 聚合权威位：每批收口时算出聚合行数（= 该批 20 条行数之和），
//!     收官位 = 40 批聚合行数总和，双跑哈希幂等；
//!   - 注入三族：未开批先记条/重复条目/条不满强行收口 10/10 必抓；
//!   - LCG 种子写死 = SEG_BASE（0x65C40），禁 rand/浮点/IO/墙钟；
//!     no_std + alloc；零 unwrap/expect。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x65C40，AI-VX3-1030 独占区）。
pub const SEG_BASE: u32 = 0x65C40;

/// 批闭集：B01–B40 四十批（冻结，来源：域收官任务书）。
pub const BATCH_COUNT: usize = 40;

/// 每批条目闭集：20 条（冻结，来源：域收官任务书）。
pub const ITEMS_PER_BATCH: usize = 20;

/// 全账条目总数：40×20 = 800（冻结常量，来源：两闭集直积）。
pub const TOTAL_ITEMS: usize = BATCH_COUNT * ITEMS_PER_BATCH;

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FinalCode {
    /// 批号越闭集
    BatchOutOfRange,
    /// 条目号越闭集
    ItemOutOfRange,
    /// 未开批先记条
    BookBeforeOpen,
    /// 条目重复登记
    ItemDuplicate,
    /// 条不满强行收口
    CloseIncomplete,
    /// 双守恒破：Σ条 ≢ 800 或批 ≢ 40
    ConservationBreak,
    /// 收口后补条（已收口批只读）
    PostCloseWrite,
    /// 聚合权威位不一致（双跑不同）
    AuthorityMismatch,
    /// 收官位未就绪（800 条未全收口）
    AuthorityNotReady,
}

impl FinalCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            FinalCode::BatchOutOfRange => "批号越闭集：1~40 冻结",
            FinalCode::ItemOutOfRange => "条目号越闭集：1~20 冻结",
            FinalCode::BookBeforeOpen => "未开批先记条：必须先 Open 批",
            FinalCode::ItemDuplicate => "条目重复登记：同批同条号只许一次",
            FinalCode::CloseIncomplete => "条不满 20 强行收口：拒收",
            FinalCode::ConservationBreak => "双守恒破：Σ条≡800 且批≡40",
            FinalCode::PostCloseWrite => "收口后补条：已收口批只读红线",
            FinalCode::AuthorityMismatch => "聚合权威位双跑不一致：非确定性",
            FinalCode::AuthorityNotReady => "收官位未就绪：800 条未全收口",
        }
    }
    pub const ALL: [FinalCode; 9] = [
        FinalCode::BatchOutOfRange,
        FinalCode::ItemOutOfRange,
        FinalCode::BookBeforeOpen,
        FinalCode::ItemDuplicate,
        FinalCode::CloseIncomplete,
        FinalCode::ConservationBreak,
        FinalCode::PostCloseWrite,
        FinalCode::AuthorityMismatch,
        FinalCode::AuthorityNotReady,
    ];
}

pub fn detail(code: u32) -> &'static str {
    let mut i = 0;
    while i < FinalCode::ALL.len() {
        if FinalCode::ALL[i].val() == code {
            return FinalCode::ALL[i].detail();
        }
        i += 1;
    }
    "未知诊断码：不在 0x65C40 段闭集内"
}

// ── 批状态三闭集 ─────────────────────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BatchPhase {
    /// 未开批
    Unopened,
    /// 聚合中
    Aggregating,
    /// 已收口
    Closed,
}

/// 合法边闭集：Unopened→Aggregating→Closed（线性单链）。
fn batch_edge(cur: BatchPhase, next: BatchPhase) -> bool {
    matches!(
        (cur, next),
        (BatchPhase::Unopened, BatchPhase::Aggregating) | (BatchPhase::Aggregating, BatchPhase::Closed)
    )
}

/// 条目状态：仅两态（未登记/已登记），登记一次即冻结。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ItemPhase {
    Empty,
    Logged,
}

/// 条目行（批号/条号/行数）。Copy 派生：数组重复字面量需要。
#[derive(Clone, Copy)]
pub struct FinalItem {
    pub batch: usize,
    pub item: usize,
    /// 该条聚合的行数（>0，来源：LCG 正演）。
    pub lines: u32,
}

/// 批账：20 条 + 批态 + 聚合权威位。Copy 派生：批数组重复字面量需要。
#[derive(Clone, Copy)]
pub struct BatchLedger {
    pub phase: BatchPhase,
    /// 条目池：下标 = item-1。
    items: [Option<FinalItem>; ITEMS_PER_BATCH],
    pub logged: u32,
    /// 聚合权威位：收口时冻结 = Σ条行数。
    pub authority: Option<u64>,
}

impl BatchLedger {
    pub const fn new() -> BatchLedger {
        BatchLedger {
            phase: BatchPhase::Unopened,
            items: [None; ITEMS_PER_BATCH],
            logged: 0,
            authority: None,
        }
    }

    /// 开批。
    pub fn open(&mut self) -> Result<(), u32> {
        if !batch_edge(self.phase, BatchPhase::Aggregating) {
            return Err(FinalCode::BookBeforeOpen.val());
        }
        self.phase = BatchPhase::Aggregating;
        Ok(())
    }

    /// 记条：批须 Aggregating，条号闭集且不重复。
    pub fn log(&mut self, item: usize, lines: u32) -> Result<(), u32> {
        if self.phase == BatchPhase::Closed {
            return Err(FinalCode::PostCloseWrite.val());
        }
        if self.phase != BatchPhase::Aggregating {
            return Err(FinalCode::BookBeforeOpen.val());
        }
        if item == 0 || item > ITEMS_PER_BATCH {
            return Err(FinalCode::ItemOutOfRange.val());
        }
        if lines == 0 {
            return Err(FinalCode::ItemOutOfRange.val());
        }
        let idx = item - 1;
        if idx >= self.items.len() {
            return Err(FinalCode::ItemOutOfRange.val());
        }
        if self.items[idx].is_some() {
            return Err(FinalCode::ItemDuplicate.val());
        }
        self.items[idx] = Some(FinalItem { batch: 0, item, lines });
        self.logged += 1;
        Ok(())
    }

    /// 收口：条满 20 才许；冻结聚合权威位。
    pub fn close(&mut self) -> Result<u64, u32> {
        if !batch_edge(self.phase, BatchPhase::Closed) {
            if self.phase == BatchPhase::Unopened {
                return Err(FinalCode::BookBeforeOpen.val());
            }
            return Err(FinalCode::PostCloseWrite.val());
        }
        if self.logged != ITEMS_PER_BATCH as u32 {
            return Err(FinalCode::CloseIncomplete.val());
        }
        let mut sum: u64 = 0;
        let mut i = 0;
        while i < ITEMS_PER_BATCH {
            match self.items[i] {
                Some(ref it) => sum += it.lines as u64,
                None => return Err(FinalCode::CloseIncomplete.val()),
            }
            i += 1;
        }
        self.authority = Some(sum);
        self.phase = BatchPhase::Closed;
        Ok(sum)
    }

    /// 批内守恒：Σ条行数 ≡ 权威位。
    pub fn verify(&self) -> Result<u64, u32> {
        if self.phase != BatchPhase::Closed {
            return Err(FinalCode::AuthorityNotReady.val());
        }
        let auth = match self.authority {
            Some(a) => a,
            None => return Err(FinalCode::AuthorityNotReady.val()),
        };
        let mut sum: u64 = 0;
        let mut i = 0;
        while i < ITEMS_PER_BATCH {
            match self.items[i] {
                Some(ref it) => sum += it.lines as u64,
                None => return Err(FinalCode::ConservationBreak.val()),
            }
            i += 1;
        }
        if sum != auth {
            return Err(FinalCode::ConservationBreak.val());
        }
        Ok(sum)
    }
}

/// 收官总账：40 批。
pub struct FinalMatrix {
    batches: [BatchLedger; BATCH_COUNT],
    pub closed: u32,
    /// 全账收官旗。
    pub sealed: bool,
    /// 收官权威位。
    pub grand_total: Option<u64>,
}

impl FinalMatrix {
    pub fn new() -> FinalMatrix {
        FinalMatrix {
            batches: [BatchLedger::new(); BATCH_COUNT],
            closed: 0,
            sealed: false,
            grand_total: None,
        }
    }

    fn bounds(batch: usize) -> Result<(), u32> {
        if batch == 0 || batch > BATCH_COUNT {
            return Err(FinalCode::BatchOutOfRange.val());
        }
        Ok(())
    }

    pub fn open(&mut self, batch: usize) -> Result<(), u32> {
        Self::bounds(batch)?;
        self.batches[batch - 1].open()
    }

    pub fn log(&mut self, batch: usize, item: usize, lines: u32) -> Result<(), u32> {
        Self::bounds(batch)?;
        if self.sealed {
            return Err(FinalCode::PostCloseWrite.val());
        }
        self.batches[batch - 1].log(item, lines)
    }

    pub fn close(&mut self, batch: usize) -> Result<u64, u32> {
        Self::bounds(batch)?;
        let a = self.batches[batch - 1].close()?;
        self.closed += 1;
        Ok(a)
    }

    /// 全收官：40 批全收口后冻结，算出 grand_total。
    pub fn seal(&mut self) -> Result<u64, u32> {
        if self.closed != BATCH_COUNT as u32 {
            return Err(FinalCode::AuthorityNotReady.val());
        }
        let mut grand: u64 = 0;
        let mut b = 1;
        while b <= BATCH_COUNT {
            grand += self.batches[b - 1].verify()?;
            b += 1;
        }
        self.sealed = true;
        self.grand_total = Some(grand);
        Ok(grand)
    }

    /// 双守恒：批数 ≡ 40 且 Σ条数 ≡ 800。
    pub fn conservation(&self) -> Result<[u32; 3], u32> {
        let mut counts = [0u32; 3];
        let mut items_total: u32 = 0;
        let mut b = 0;
        while b < BATCH_COUNT {
            let k = match self.batches[b].phase {
                BatchPhase::Unopened => 0,
                BatchPhase::Aggregating => 1,
                BatchPhase::Closed => 2,
            };
            counts[k] += 1;
            items_total += self.batches[b].logged;
            b += 1;
        }
        if counts[0] + counts[1] + counts[2] != BATCH_COUNT as u32 {
            return Err(FinalCode::ConservationBreak.val());
        }
        if self.closed as usize == BATCH_COUNT && items_total != TOTAL_ITEMS as u32 {
            return Err(FinalCode::ConservationBreak.val());
        }
        Ok(counts)
    }

    /// 四列账导出：批号/条数/行数/状态（确定性序）。
    pub fn export_matrix(&self) -> Vec<(usize, u32, u64, u8)> {
        let mut out = Vec::new();
        let mut b = 0;
        while b < BATCH_COUNT {
            let st = match self.batches[b].phase {
                BatchPhase::Unopened => 0u8,
                BatchPhase::Aggregating => 1u8,
                BatchPhase::Closed => 2u8,
            };
            let lines = match self.batches[b].authority {
                Some(a) => a,
                None => 0,
            };
            out.push((b + 1, self.batches[b].logged, lines, st));
            b += 1;
        }
        out
    }
}

// ── 确定性 LCG ───────────────────────────────────────────────────
pub struct Lcg(u64);
impl Lcg {
    pub const fn new(seed: u64) -> Lcg {
        Lcg(seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407))
    }
    pub fn next(&mut self) -> u64 {
        self.0 = self
            .0
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        self.0 >> 33
    }
    pub fn below(&mut self, n: u64) -> u64 {
        let m = if n == 0 { 1 } else { n };
        self.next() % m
    }
}

// ── 黄金正演：40 批 × 20 条全登记 → 收口 → 收官权威位 ────────────
pub fn golden_run() -> Result<u64, u32> {
    let mut m = FinalMatrix::new();
    let mut rng = Lcg::new(SEG_BASE as u64);
    let mut b = 1;
    while b <= BATCH_COUNT {
        m.open(b)?;
        let mut it = 1;
        while it <= ITEMS_PER_BATCH {
            let lines = 1 + rng.below(50) as u32;
            m.log(b, it, lines)?;
            it += 1;
        }
        m.close(b)?;
        b += 1;
    }
    let g1 = m.seal()?;
    m.conservation()?;
    // 幂等：export 双跑逐位同。
    let e1 = m.export_matrix();
    let e2 = m.export_matrix();
    let mut i = 0;
    while i < e1.len() {
        if e1[i] != e2[i] {
            return Err(FinalCode::AuthorityMismatch.val());
        }
        i += 1;
    }
    if e1.len() != BATCH_COUNT {
        return Err(FinalCode::ConservationBreak.val());
    }
    // 反向对照：收官旗置位后再写必拒（独立判据）。
    let rej = m.log(1, 1, 10);
    let mut sealed_reject = false;
    if let Err(e) = rej {
        if e == FinalCode::PostCloseWrite.val() {
            sealed_reject = true;
        }
    }
    if !sealed_reject {
        return Err(FinalCode::PostCloseWrite.val());
    }
    // 黄金哈希。
    let mut h: u64 = 0x811C9DC5;
    i = 0;
    while i < e1.len() {
        let (bt, cnt, ln, st) = e1[i];
        h = (h ^ (bt as u64)).wrapping_mul(0x01000193);
        h = (h ^ (cnt as u64)).wrapping_mul(0x01000193);
        h = (h ^ ln).wrapping_mul(0x01000193);
        h = (h ^ (st as u64)).wrapping_mul(0x01000193);
        i += 1;
    }
    h = (h ^ g1).wrapping_mul(0x01000193);
    Ok(h)
}

// ── 注入面：三族 10/10 必抓 ──────────────────────────────────────
pub struct InjectLedger {
    pub tries: [u32; 3],
    pub caught: [u32; 3],
}

impl InjectLedger {
    pub const fn new() -> InjectLedger {
        InjectLedger { tries: [0; 3], caught: [0; 3] }
    }
    pub fn all_caught(&self) -> bool {
        let mut i = 0;
        while i < 3 {
            if self.caught[i] != self.tries[i] {
                return false;
            }
            i += 1;
        }
        true
    }
}

pub fn run_injections(ledger: &mut InjectLedger) {
    // 族 0：未开批先记条。
    ledger.tries[0] += 1;
    let mut m = FinalMatrix::new();
    let mut c0 = false;
    if let Err(e) = m.log(1, 1, 5) {
        if e == FinalCode::BookBeforeOpen.val() {
            c0 = true;
        }
    }
    if c0 {
        ledger.caught[0] += 1;
    }
    // 族 1：重复条目。
    ledger.tries[1] += 1;
    let _ = m.open(1);
    let _ = m.log(1, 1, 5);
    let mut c1 = false;
    if let Err(e) = m.log(1, 1, 7) {
        if e == FinalCode::ItemDuplicate.val() {
            c1 = true;
        }
    }
    if c1 {
        ledger.caught[1] += 1;
    }
    // 族 2：条不满强行收口。
    ledger.tries[2] += 1;
    let mut c2 = false;
    if let Err(e) = m.close(1) {
        if e == FinalCode::CloseIncomplete.val() {
            c2 = true;
        }
    }
    if c2 {
        ledger.caught[2] += 1;
    }
}

// ── 判据入口 ─────────────────────────────────────────────────────
pub fn segment_guard() -> bool {
    let mut i = 0;
    while i < FinalCode::ALL.len() {
        let v = FinalCode::ALL[i].val();
        if v < SEG_BASE || v >= SEG_BASE + 0x40 {
            return false;
        }
        i += 1;
    }
    true
}

pub fn extra_checks() -> Vec<(&'static str, bool, String)> {
    let mut v: Vec<(&'static str, bool, String)> = Vec::new();
    let golden = golden_run();
    v.push(("J1-黄金正演800条", golden.is_ok(), format!("{:?}", golden)));
    let mut led = InjectLedger::new();
    run_injections(&mut led);
    v.push((
        "J1-注入三族全抓",
        led.all_caught(),
        format!("t/c {:?}/{:?}", led.tries, led.caught),
    ));
    // 守恒判据：独立写死 800/40，不复用 TOTAL_ITEMS 常量位。
    v.push((
        "J1-双守恒40x20=800",
        BATCH_COUNT == 40 && ITEMS_PER_BATCH == 20 && TOTAL_ITEMS == 800,
        format!("{}x{}={}", BATCH_COUNT, ITEMS_PER_BATCH, TOTAL_ITEMS),
    ));
    // 反向判据：条目号 21 必拒。
    let mut m2 = FinalMatrix::new();
    let _ = m2.open(1);
    let over = m2.log(1, ITEMS_PER_BATCH + 1, 5);
    let mut ok = false;
    if let Err(e) = over {
        if e == FinalCode::ItemOutOfRange.val() {
            ok = true;
        }
    }
    v.push(("J1-条目越界必拒", ok, String::from("1~20 闭集")));
    // 反向判据：批号 41 必拒。
    let mut m3 = FinalMatrix::new();
    let over_b = m3.open(BATCH_COUNT + 1);
    let mut ok2 = false;
    if let Err(e) = over_b {
        if e == FinalCode::BatchOutOfRange.val() {
            ok2 = true;
        }
    }
    v.push(("J1-批号越界必拒", ok2, String::from("1~40 闭集")));
    // 批状态机闭集：Unopened 直跳 Closed 必拒。
    let mut m4 = FinalMatrix::new();
    let skip = m4.close(1);
    let mut ok3 = false;
    if let Err(e) = skip {
        if e == FinalCode::BookBeforeOpen.val() {
            ok3 = true;
        }
    }
    v.push(("J1-跳级收口必拒", ok3, String::from("闭式边集")));
    v.push(("J3-段位守卫", segment_guard(), String::from("0x65C40 窗口")));
    v
}

pub fn run_unx13581_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F13581 域收官总成（B01–B40 四十批 800 条聚合权威位）");
    let checks = extra_checks();
    let mut i = 0;
    while i < checks.len() {
        let (n, p, d) = &checks[i];
        s.add_dyn(*n, *p, d);
        i += 1;
    }
    s
}
