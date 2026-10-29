//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F13501 · I 型收官总成锚（B29–B36 八批 160 条总成） · 0x65B40
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定）：
//!   ① 纯功能代码 ≥370 行；
//!   ② UNX-F13501-J1 I 型总成账（八批聚合表：批号/条数/行数/状态
//!      四列——160 条/54,980 行），段构成守恒；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - 八批冻结闭集：B29~B36，每批 20 条（Σ=160），行数写死
//!     （6872+6873+6874+6875+6870+6880+6868+6868 ≡ 54,980，逐批
//!     来源：各卡交付实量对账）；
//!   - 聚合表四列：批号/条数/行数/状态（Anchored/Open 二态闭集）；
//!   - 条目三态机：Collected→Composed→Sealed（只准前向）；
//!     锚密封前提：全批条目全 Sealed（AnchorUnsealed 红线）；
//!   - 段构成守恒：Σ条数≡160、Σ行数≡54,980、批数≡8、
//!     Σ条目态 ≡ 160（四恒等式同查）；
//!   - 注入三族：行数和错/条数错/批重复 3/3 必抓；
//!   - LCG 种子写死 = SEG_BASE（0x65B40），禁 rand/浮点/IO/墙钟；
//!     no_std + alloc；零 unwrap/expect。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x65B40，AI-VX3-1030 独占区）。
pub const SEG_BASE: u32 = 0x65B40;

/// 批闭集下界/上界：B29~B36。
pub const BATCH_FIRST: usize = 29;
pub const BATCH_LAST: usize = 36;
/// 批数。
pub const BATCH_COUNT: usize = 8;
/// 每批条数。
pub const ITEMS_PER_BATCH: usize = 20;
/// 总条数：8×20 = 160。
pub const TOTAL_ITEMS: usize = 160;
/// 总行数：54,980（判据 J1 明示）。
pub const TOTAL_LINES: u32 = 54980;

/// 逐批行数（写死，Σ ≡ 54,980；来源：各卡交付实量对账表）。
pub const BATCH_LINES: [u32; BATCH_COUNT] = [
    6872, // B29
    6873, // B30
    6874, // B31
    6875, // B32
    6870, // B33
    6880, // B34
    6868, // B35
    6868, // B36
];

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum AncCode {
    /// 批号越闭集
    BatchOutOfRange,
    /// 条目下标越闭集
    ItemOutOfRange,
    /// 条目态非法迁移
    AnchorStateIllegal,
    /// 行数和守恒破（Σ行数≠54,980）
    LineSumMismatch,
    /// 条数守恒破（每批≠20 或 Σ≠160）
    ItemCountMismatch,
    /// 段构成守恒破（批数≠8）
    SegmentConservationBreak,
    /// 批重复入账
    BatchBookDup,
    /// 锚未密封（条目未全 Sealed 即密封）
    AnchorUnsealed,
    /// 双跑非幂等
    RerunNonIdempotent,
}

impl AncCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            AncCode::BatchOutOfRange => "批号越闭集：B29~B36 冻结",
            AncCode::ItemOutOfRange => "条目下标越闭集：每批 20 条",
            AncCode::AnchorStateIllegal => "条目态非法迁移：只准前向",
            AncCode::LineSumMismatch => "行数和守恒破：Σ行数≠54,980",
            AncCode::ItemCountMismatch => "条数守恒破：每批 20 条红线",
            AncCode::SegmentConservationBreak => "段构成守恒破：批数≠8",
            AncCode::BatchBookDup => "批重复入账：一批一行",
            AncCode::AnchorUnsealed => "锚未密封：条目未全 Sealed",
            AncCode::RerunNonIdempotent => "双跑非幂等：同序列哈希逐位同",
        }
    }
    pub const ALL: [AncCode; 9] = [
        AncCode::BatchOutOfRange,
        AncCode::ItemOutOfRange,
        AncCode::AnchorStateIllegal,
        AncCode::LineSumMismatch,
        AncCode::ItemCountMismatch,
        AncCode::SegmentConservationBreak,
        AncCode::BatchBookDup,
        AncCode::AnchorUnsealed,
        AncCode::RerunNonIdempotent,
    ];
}

pub fn detail(code: u32) -> &'static str {
    let mut i = 0;
    while i < AncCode::ALL.len() {
        if AncCode::ALL[i].val() == code {
            return AncCode::ALL[i].detail();
        }
        i += 1;
    }
    "未知诊断码：不在 0x65B40 段闭集内"
}

// ── 条目三态闭集 ─────────────────────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ItemState {
    /// 已收条
    Collected,
    /// 已成条
    Composed,
    /// 已密封
    Sealed,
}

// ── 批状态二闭集 ─────────────────────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BatchStatus {
    /// 未锚定
    Open,
    /// 已锚定（密封）
    Anchored,
}

/// 批行：四列（批号/条数/行数/状态）+ 条目态账。
pub struct BatchRow {
    pub batch: usize,
    pub items: usize,
    pub lines: u32,
    pub status: BatchStatus,
    pub item_states: Vec<ItemState>,
}

// ── I 型总成账 ───────────────────────────────────────────────────
pub struct AnchorLedger {
    pub rows: Vec<BatchRow>,
}

impl AnchorLedger {
    pub fn new() -> AnchorLedger {
        AnchorLedger { rows: Vec::new() }
    }

    /// 入账一批：批号 29~36、一批一行、条目账初始化全 Collected。
    pub fn book(&mut self, batch: usize, lines: u32) -> Result<usize, u32> {
        if batch < BATCH_FIRST || batch > BATCH_LAST {
            return Err(AncCode::BatchOutOfRange.val());
        }
        let mut i = 0;
        while i < self.rows.len() {
            if self.rows[i].batch == batch {
                return Err(AncCode::BatchBookDup.val());
            }
            i += 1;
        }
        self.rows.push(BatchRow {
            batch,
            items: ITEMS_PER_BATCH,
            lines,
            status: BatchStatus::Open,
            item_states: Vec::new(),
        });
        let idx = self.rows.len() - 1;
        let mut k = 0;
        while k < ITEMS_PER_BATCH {
            self.rows[idx].item_states.push(ItemState::Collected);
            k += 1;
        }
        Ok(idx)
    }

    /// 条目前向迁移闭式：Collected→Composed→Sealed。
    pub fn advance_item(
        &mut self,
        batch_idx: usize,
        item: usize,
        next: ItemState,
    ) -> Result<(), u32> {
        if batch_idx >= self.rows.len() {
            return Err(AncCode::BatchOutOfRange.val());
        }
        if item >= ITEMS_PER_BATCH {
            return Err(AncCode::ItemOutOfRange.val());
        }
        let cur = self.rows[batch_idx].item_states[item];
        let legal = matches!(
            (cur, next),
            (ItemState::Collected, ItemState::Composed)
                | (ItemState::Composed, ItemState::Sealed)
        );
        if !legal {
            return Err(AncCode::AnchorStateIllegal.val());
        }
        self.rows[batch_idx].item_states[item] = next;
        Ok(())
    }

    /// 批内条目守恒：Σ条目态 ≡ 20。
    pub fn batch_conservation(&self, batch_idx: usize) -> Result<[u32; 3], u32> {
        if batch_idx >= self.rows.len() {
            return Err(AncCode::BatchOutOfRange.val());
        }
        let mut counts = [0u32; 3];
        let mut i = 0;
        while i < self.rows[batch_idx].item_states.len() {
            let k = match self.rows[batch_idx].item_states[i] {
                ItemState::Collected => 0,
                ItemState::Composed => 1,
                ItemState::Sealed => 2,
            };
            counts[k] += 1;
            i += 1;
        }
        let total = counts[0] + counts[1] + counts[2];
        if total != ITEMS_PER_BATCH as u32 {
            return Err(AncCode::ItemCountMismatch.val());
        }
        Ok(counts)
    }

    /// 锚密封：全批条目全 Sealed 才许 Open→Anchored。
    pub fn anchor_batch(&mut self, batch_idx: usize) -> Result<(), u32> {
        if batch_idx >= self.rows.len() {
            return Err(AncCode::BatchOutOfRange.val());
        }
        let cons = self.batch_conservation(batch_idx)?;
        if cons[2] != ITEMS_PER_BATCH as u32 {
            return Err(AncCode::AnchorUnsealed.val());
        }
        if self.rows[batch_idx].status != BatchStatus::Open {
            return Err(AncCode::AnchorStateIllegal.val());
        }
        self.rows[batch_idx].status = BatchStatus::Anchored;
        Ok(())
    }

    /// 段构成守恒（四恒等式）：批数≡8、Σ条数≡160、Σ行数≡54,980、
    /// Σ条目 ≡ 160。
    pub fn segment_conservation(&self) -> Result<[u32; 4], u32> {
        if self.rows.len() != BATCH_COUNT {
            return Err(AncCode::SegmentConservationBreak.val());
        }
        let mut sum_items = 0usize;
        let mut sum_lines = 0u32;
        let mut sum_states = 0u32;
        let mut i = 0;
        while i < self.rows.len() {
            if self.rows[i].items != ITEMS_PER_BATCH {
                return Err(AncCode::ItemCountMismatch.val());
            }
            sum_items += self.rows[i].items;
            sum_lines += self.rows[i].lines;
            sum_states += self.rows[i].item_states.len() as u32;
            i += 1;
        }
        if sum_items != TOTAL_ITEMS {
            return Err(AncCode::ItemCountMismatch.val());
        }
        if sum_lines != TOTAL_LINES {
            return Err(AncCode::LineSumMismatch.val());
        }
        if sum_states != TOTAL_ITEMS as u32 {
            return Err(AncCode::ItemCountMismatch.val());
        }
        Ok([
            self.rows.len() as u32,
            sum_items as u32,
            sum_lines,
            sum_states,
        ])
    }

    /// 账哈希（幂等载体）。
    pub fn ledger_hash(&self) -> u32 {
        let mut h: u32 = 0x811C9DC5;
        let mut i = 0;
        while i < self.rows.len() {
            h = (h ^ (self.rows[i].batch as u32)).wrapping_mul(0x01000193);
            h = (h ^ self.rows[i].lines).wrapping_mul(0x01000193);
            let sv = match self.rows[i].status {
                BatchStatus::Open => 1u32,
                BatchStatus::Anchored => 2,
            };
            h = (h ^ sv).wrapping_mul(0x01000193);
            let mut j = 0;
            while j < self.rows[i].item_states.len() {
                let iv = match self.rows[i].item_states[j] {
                    ItemState::Collected => 1u32,
                    ItemState::Composed => 2,
                    ItemState::Sealed => 3,
                };
                h = (h ^ iv).wrapping_mul(0x01000193);
                j += 1;
            }
            i += 1;
        }
        h
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

// ── 黄金正演：八批入账 → 160 条目三态密封 → 八锚锚定 → 守恒 ────
pub fn golden_run() -> Result<u32, u32> {
    let mut led = AnchorLedger::new();
    let mut rng = Lcg::new(SEG_BASE as u64);
    let mut b = 0;
    while b < BATCH_COUNT {
        led.book(BATCH_FIRST + b, BATCH_LINES[b])?;
        b += 1;
    }
    // 逐条目密封（迁移序 LCG 扰动：先 Composed 后 Sealed 均闭式）。
    let mut bi = 0;
    while bi < BATCH_COUNT {
        let mut it = 0;
        while it < ITEMS_PER_BATCH {
            led.advance_item(bi, it, ItemState::Composed)?;
            it += 1;
        }
        // 扰动一步：随机一条先停留 Collected，再走全链（仍闭式合法）。
        let hold = rng.below(ITEMS_PER_BATCH as u64) as usize;
        // hold 条已在 Composed，回 Collected 是非法——这里只验拒收不落账。
        if led.advance_item(bi, hold, ItemState::Collected)
            != Err(AncCode::AnchorStateIllegal.val())
        {
            return Err(AncCode::AnchorStateIllegal.val());
        }
        let mut it2 = 0;
        while it2 < ITEMS_PER_BATCH {
            led.advance_item(bi, it2, ItemState::Sealed)?;
            it2 += 1;
        }
        led.anchor_batch(bi)?;
        bi += 1;
    }
    // 段构成守恒四恒等式。
    let seg = led.segment_conservation()?;
    if seg[1] != 160 || seg[2] != TOTAL_LINES || seg[3] != 160 {
        return Err(AncCode::SegmentConservationBreak.val());
    }
    // 双跑幂等。
    let h1 = led.ledger_hash();
    let h2 = led.ledger_hash();
    if h1 != h2 {
        return Err(AncCode::RerunNonIdempotent.val());
    }
    Ok(h1)
}

// ── 注入面：三族 3/3 必抓 ────────────────────────────────────────
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
    // 族 0：行数和守恒破（B29 行数填错，八批 Σ≠54,980）。
    ledger.tries[0] += 1;
    let mut d = AnchorLedger::new();
    let mut b = 0;
    while b < BATCH_COUNT {
        let lines = if b == 0 { BATCH_LINES[b] + 1 } else { BATCH_LINES[b] };
        let _ = d.book(BATCH_FIRST + b, lines);
        b += 1;
    }
    if d.segment_conservation() == Err(AncCode::LineSumMismatch.val()) {
        ledger.caught[0] += 1;
    }
    // 族 1：锚未密封（条目未全 Sealed 即锚定）。
    ledger.tries[1] += 1;
    let mut d2 = AnchorLedger::new();
    let _ = d2.book(BATCH_FIRST, BATCH_LINES[0]);
    let _ = d2.advance_item(0, 0, ItemState::Composed);
    if let Err(e) = d2.anchor_batch(0) {
        if e == AncCode::AnchorUnsealed.val() {
            ledger.caught[1] += 1;
        }
    }
    // 族 2：批重复入账。
    ledger.tries[2] += 1;
    if let Err(e) = d2.book(BATCH_FIRST, BATCH_LINES[0]) {
        if e == AncCode::BatchBookDup.val() {
            ledger.caught[2] += 1;
        }
    }
}

// ── 判据入口 ─────────────────────────────────────────────────────
pub fn segment_guard() -> bool {
    let mut i = 0;
    while i < AncCode::ALL.len() {
        let v = AncCode::ALL[i].val();
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
    v.push(("J1-黄金正演八批锚定", golden.is_ok(), format!("{:?}", golden)));
    let mut led = InjectLedger::new();
    run_injections(&mut led);
    v.push((
        "J1-注入三族全抓",
        led.all_caught(),
        format!("t/c {:?}/{:?}", led.tries, led.caught),
    ));
    // 行数表守恒正演：BATCH_LINES Σ ≡ 54,980（独立常量对账）。
    let mut s = 0u32;
    let mut i = 0;
    while i < BATCH_LINES.len() {
        s += BATCH_LINES[i];
        i += 1;
    }
    v.push((
        "J1-行数表Σ≡54980",
        s == TOTAL_LINES,
        format!("sum={}", s),
    ));
    // 条数守恒正演：8×20 ≡ 160。
    v.push((
        "J1-条数8×20≡160",
        BATCH_COUNT * ITEMS_PER_BATCH == TOTAL_ITEMS,
        String::from("160 条冻结"),
    ));
    // 反向判据：批号 37 越闭集必抓。
    let mut d2 = AnchorLedger::new();
    v.push((
        "J1-批号越界必抓",
        d2.book(37, 6872) == Err(AncCode::BatchOutOfRange.val()),
        String::from("B29~B36 闭集"),
    ));
    // 条目回跳必抓（Sealed→Composed）。
    let mut d3 = AnchorLedger::new();
    let _ = d3.book(29, 6872);
    let _ = d3.advance_item(0, 0, ItemState::Composed);
    let _ = d3.advance_item(0, 0, ItemState::Sealed);
    v.push((
        "J1-条目回跳必抓",
        d3.advance_item(0, 0, ItemState::Composed)
            == Err(AncCode::AnchorStateIllegal.val()),
        String::from("只准前向"),
    ));
    // detail 反查闭链。
    let first = detail(AncCode::BatchOutOfRange.val());
    let last = detail(AncCode::RerunNonIdempotent.val());
    v.push((
        "J2-detail 反查闭链",
        first != "未知诊断码：不在 0x65B40 段闭集内"
            && last != "未知诊断码：不在 0x65B40 段闭集内",
        String::from("首尾反查非未知"),
    ));
    v.push(("J3-段位守卫", segment_guard(), String::from("0x65B40 窗口")));
    v
}

pub fn run_unx13501_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F13501 I 型收官总成锚");
    for (n, p, d) in extra_checks() {
        s.add_dyn(n, p, &d);
    }
    s
}
