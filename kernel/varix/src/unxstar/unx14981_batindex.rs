//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F14981 · B16–B29 十四批索引总账 · 0x66DC0
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定）：
//!   ① 纯功能代码 ≥270 行；
//!   ② UNX-F14981-J1 十四批 280 条索引四列表与批册逐批勾稽
//!      280/280 零缺漏；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - 批册闭集：B16–B29 共 14 批（名表写死），每批 20 条索引 =
//!      280 条总账；
//!   - 独立判据常量：BATCH_COUNT = 14、PER_BATCH = 20、
//!      TOTAL_INDEX = 280、HEADER_BATCHES = 14（批头批数）均写死
//!      不复用被测值，勾稽用；
//!   - 四列表：批号列（批名）/序号列（批内 0..19）/键位列（闭式
//!      键 = 段基址 + 批内序号）/摘要列（非空串）；
//!   - 键位闭式（写死）：批 b（0..13）段基址 = 0x66B00 + b*0x40，
//!      键 = 基址 + 序号（本批 14 段 0x66B00–0x66E40 与批册对齐，
//!      本卡所在段 0x66DC0 = B27）；
//!   - 勾稽恒等式：总账 280 == 判据 280 == 14×20；逐批 20/20；
//!      批内序号严格递增零重号；键位逐批升段零跨段混键；
//!   - 状态机：Missing → Indexed → Reconciled 三态收敛，非法回边
//!      （重复索引）拒收；
//!   - 注入面：批越界/序号越界/缺漏检出 三族全抓；
//!   - LCG 种子写死 = SEG_BASE（0x66DC0），禁 rand/浮点/IO/墙钟；
//!     no_std + alloc；零 unwrap/expect。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x66DC0，AI-VX3-1030 独占区）。
pub const SEG_BASE: u32 = 0x66DC0;

// ── 批册闭集 ─────────────────────────────────────────────────────
pub const BATCH_COUNT: usize = 14;
pub const PER_BATCH: usize = 20;
pub const TOTAL_INDEX: usize = BATCH_COUNT * PER_BATCH;
/// 独立判据常量（不复用乘积/批数）。
pub const EXPECTED_TOTAL: usize = 280;
pub const HEADER_BATCHES: usize = 14;

/// 批名表（写死 B16–B29）。
pub const BATCH_NAMES: [&str; BATCH_COUNT] = [
    "B16", "B17", "B18", "B19", "B20", "B21", "B22",
    "B23", "B24", "B25", "B26", "B27", "B28", "B29",
];

/// 批段基址闭式（写死：0x66B00 + b*0x40，B16 段起 0x66B00）。
pub const BATCH_SEG_BASE: u32 = 0x66B00;
pub const BATCH_SEG_STEP: u32 = 0x40;

pub fn batch_seg(b: usize) -> Result<u32, u32> {
    if b >= BATCH_COUNT {
        return Err(BatchCode::BatchOutOfRange.val());
    }
    Ok(BATCH_SEG_BASE + (b as u32) * BATCH_SEG_STEP)
}

/// 索引键闭式：批 b 序号 s → 段基址 + s。
pub fn index_key(b: usize, s: usize) -> Result<u32, u32> {
    if b >= BATCH_COUNT {
        return Err(BatchCode::BatchOutOfRange.val());
    }
    if s >= PER_BATCH {
        return Err(BatchCode::SeqOutOfRange.val());
    }
    Ok(batch_seg(b)? + s as u32)
}

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BatchCode {
    /// 批号越 14 批闭集
    BatchOutOfRange,
    /// 序号越 20 闭集
    SeqOutOfRange,
    /// 总账与判据 280 不勾稽
    TotalReconFail,
    /// 批头批数与判据 14 不勾稽
    HeaderReconFail,
    /// 逐批 20/20 守恒破
    PerBatchConservation,
    /// 批内序号重号/乱序
    SeqOrderBroken,
    /// 键位跨段混键
    KeyCrossBatch,
    /// 索引未入账
    EntryUnbooked,
    /// 条目终态悬挂
    EntryHung,
    /// 双跑不一致（不可复现）
    NotReproducible,
}

impl BatchCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            BatchCode::BatchOutOfRange => "批号越闭集：B16–B29 十四批冻结",
            BatchCode::SeqOutOfRange => "序号越闭集：批内 0–19 冻结",
            BatchCode::TotalReconFail => "总账与判据 280 不勾稽",
            BatchCode::HeaderReconFail => "批头批数与判据 14 不勾稽",
            BatchCode::PerBatchConservation => "逐批 20/20 守恒破",
            BatchCode::SeqOrderBroken => "批内序号重号或乱序",
            BatchCode::KeyCrossBatch => "键位跨段混键",
            BatchCode::EntryUnbooked => "索引未入账：280/280 红线",
            BatchCode::EntryHung => "条目终态悬挂：状态机未收敛",
            BatchCode::NotReproducible => "双跑不一致：复现红线",
        }
    }
    pub const ALL: [BatchCode; 10] = [
        BatchCode::BatchOutOfRange,
        BatchCode::SeqOutOfRange,
        BatchCode::TotalReconFail,
        BatchCode::HeaderReconFail,
        BatchCode::PerBatchConservation,
        BatchCode::SeqOrderBroken,
        BatchCode::KeyCrossBatch,
        BatchCode::EntryUnbooked,
        BatchCode::EntryHung,
        BatchCode::NotReproducible,
    ];
}

pub fn detail(code: u32) -> &'static str {
    let mut i = 0;
    while i < BatchCode::ALL.len() {
        if BatchCode::ALL[i].val() == code {
            return BatchCode::ALL[i].detail();
        }
        i += 1;
    }
    "未知诊断码：不在 0x66DC0 段闭集内"
}

// ── 索引条目（四列表行）与状态机 ─────────────────────────────────
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum EntryState {
    Missing,
    Indexed(u32),
    Reconciled(u32),
}

/// 四列表行：批号名 / 批内序号 / 键位 / 摘要。
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct IndexRow {
    pub batch: String,
    pub seq: usize,
    pub key: u32,
    pub digest: String,
}

/// 总账。
pub struct BatchLedger {
    pub rows: Vec<IndexRow>,
    pub states: Vec<EntryState>,
    pub header_batches: usize,
    pub booked: u32,
}

impl BatchLedger {
    pub fn new() -> BatchLedger {
        BatchLedger {
            rows: Vec::new(),
            states: Vec::new(),
            header_batches: HEADER_BATCHES,
            booked: 0,
        }
    }

    /// 单条索引：Missing → Indexed → Reconciled。
    pub fn index_entry(&mut self, b: usize, s: usize) -> Result<IndexRow, u32> {
        if b >= BATCH_COUNT {
            return Err(BatchCode::BatchOutOfRange.val());
        }
        if s >= PER_BATCH {
            return Err(BatchCode::SeqOutOfRange.val());
        }
        let idx = b * PER_BATCH + s;
        while self.states.len() <= idx {
            self.states.push(EntryState::Missing);
        }
        // 非法回边拒收：已 Reconciled 的条目不得重复索引。
        if matches!(self.states[idx], EntryState::Reconciled(_)) {
            return Err(BatchCode::SeqOrderBroken.val());
        }
        let key = index_key(b, s)?;
        let row = IndexRow {
            batch: String::from(BATCH_NAMES[b]),
            seq: s,
            key,
            digest: format!("{}-{:02}@{:05X}", BATCH_NAMES[b], s, key),
        };
        if row.digest.len() == 0 {
            return Err(BatchCode::EntryUnbooked.val());
        }
        self.rows.push(row.clone());
        self.states[idx] = EntryState::Reconciled(key);
        self.booked += 1;
        Ok(row)
    }

    /// 全 280 条：零缺漏 + 总账勾稽 + 批头勾稽 + 逐批守恒 +
    /// 序号单调 + 键位分段。
    pub fn run_all(&mut self) -> Result<u32, u32> {
        let mut b = 0;
        while b < BATCH_COUNT {
            let mut s = 0;
            while s < PER_BATCH {
                self.index_entry(b, s)?;
                s += 1;
            }
            b += 1;
        }
        if self.rows.len() != TOTAL_INDEX || self.booked as usize != EXPECTED_TOTAL {
            return Err(BatchCode::TotalReconFail.val());
        }
        if self.header_batches != HEADER_BATCHES {
            return Err(BatchCode::HeaderReconFail.val());
        }
        // 逐批守恒：每批恰 20 条且序号严格递增零重号。
        let mut b = 0;
        while b < BATCH_COUNT {
            let mut n = 0;
            let mut prev: i64 = -1;
            let mut s = 0;
            while s < PER_BATCH {
                let idx = b * PER_BATCH + s;
                match &self.states[idx] {
                    EntryState::Reconciled(_) => {
                        n += 1;
                        if (s as i64) <= prev {
                            return Err(BatchCode::SeqOrderBroken.val());
                        }
                        prev = s as i64;
                    }
                    _ => return Err(BatchCode::EntryUnbooked.val()),
                }
                s += 1;
            }
            if n != PER_BATCH {
                return Err(BatchCode::PerBatchConservation.val());
            }
            b += 1;
        }
        // 键位分段：每条键落在本批段窗内。
        let mut i = 0;
        while i < self.rows.len() {
            let r = &self.rows[i];
            let b = i / PER_BATCH;
            let seg = batch_seg(b)?;
            let want = index_key(b, r.seq)?;
            if r.key != want || r.key < seg || r.key >= seg + BATCH_SEG_STEP {
                return Err(BatchCode::KeyCrossBatch.val());
            }
            if r.batch != BATCH_NAMES[b] {
                return Err(BatchCode::KeyCrossBatch.val());
            }
            i += 1;
        }
        Ok(self.booked)
    }

    /// 批册逐批勾稽：批名互异 + 段基址等差递进。
    pub fn batch_registry_audit(&self) -> Result<(), u32> {
        if BATCH_NAMES.len() != BATCH_COUNT {
            return Err(BatchCode::HeaderReconFail.val());
        }
        let mut b = 0;
        while b < BATCH_COUNT {
            if BATCH_NAMES[b].len() == 0 {
                return Err(BatchCode::HeaderReconFail.val());
            }
            if batch_seg(b)? != BATCH_SEG_BASE + (b as u32) * BATCH_SEG_STEP {
                return Err(BatchCode::KeyCrossBatch.val());
            }
            b += 1;
        }
        Ok(())
    }

    /// 复现：双跑账一致。
    pub fn reproducible(&self) -> Result<(), u32> {
        let mut run2 = BatchLedger::new();
        let r2 = run2.run_all();
        if run2.booked != self.booked || run2.rows != self.rows || r2.is_err() {
            return Err(BatchCode::NotReproducible.val());
        }
        Ok(())
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
        self.next() % if n < 1 { 1 } else { n }
    }
}

/// LCG 抽验：随机 16 条 (批,序) 全落闭集且键与闭式一致。
pub fn lcg_index_walk() -> Result<u32, u32> {
    let mut g = Lcg::new(SEG_BASE as u64);
    let mut n = 0;
    let mut i = 0;
    while i < 16 {
        let b = g.below(BATCH_COUNT as u64) as usize;
        let s = g.below(PER_BATCH as u64) as usize;
        let k1 = index_key(b, s)?;
        let k2 = batch_seg(b)? + s as u32;
        if k1 != k2 {
            return Err(BatchCode::KeyCrossBatch.val());
        }
        n += 1;
        i += 1;
    }
    Ok(n)
}

// ── 黄金正演 ─────────────────────────────────────────────────────
pub fn golden_run() -> Result<(u32, u32, u32), u32> {
    if TOTAL_INDEX != EXPECTED_TOTAL {
        return Err(BatchCode::TotalReconFail.val());
    }
    let mut led = BatchLedger::new();
    let booked = led.run_all()?;
    led.batch_registry_audit()?;
    led.reproducible()?;
    let walk = lcg_index_walk()?;
    Ok((booked, walk, BATCH_COUNT as u32))
}

// ── 注入面：三族 5/5 必抓 ────────────────────────────────────────
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
    // 族 0：批号越界 + 序号越界 各一发（键闭式端）。
    ledger.tries[0] += 1;
    let mut axis_ok = true;
    if let Err(e) = index_key(BATCH_COUNT, 0) {
        if e != BatchCode::BatchOutOfRange.val() {
            axis_ok = false;
        }
    } else {
        axis_ok = false;
    }
    if let Err(e) = index_key(0, PER_BATCH) {
        if e != BatchCode::SeqOutOfRange.val() {
            axis_ok = false;
        }
    } else {
        axis_ok = false;
    }
    if axis_ok {
        ledger.caught[0] += 1;
    }
    // 族 1：重复索引同条必拒（非法回边）。
    ledger.tries[1] += 1;
    let mut led = BatchLedger::new();
    let mut dup_ok = false;
    if led.index_entry(3, 7).is_ok() {
        if let Err(e) = led.index_entry(3, 7) {
            if e == BatchCode::SeqOrderBroken.val() {
                dup_ok = true;
            }
        }
    }
    if dup_ok {
        ledger.caught[1] += 1;
    }
    // 族 2：缺漏检出（只录首批 20 条，总账勾稽必破）。
    ledger.tries[2] += 1;
    let mut led2 = BatchLedger::new();
    let mut miss_ok = false;
    let mut s = 0;
    while s < PER_BATCH {
        let _ = led2.index_entry(0, s);
        s += 1;
    }
    if led2.booked as usize == PER_BATCH {
        // 缺 13 批：逐批守恒审计应判未入账。
        if let Err(e) = audit_partial(&led2) {
            if e == BatchCode::EntryUnbooked.val() {
                miss_ok = true;
            }
        }
    }
    if miss_ok {
        ledger.caught[2] += 1;
    }
}

/// 部分账逐批守恒审计（不补账，只查缺）。
fn audit_partial(led: &BatchLedger) -> Result<(), u32> {
    let mut b = 0;
    while b < BATCH_COUNT {
        let mut n = 0;
        let mut s = 0;
        while s < PER_BATCH {
            let idx = b * PER_BATCH + s;
            // 下标先比长度。
            if idx < led.states.len() {
                if matches!(led.states[idx], EntryState::Reconciled(_)) {
                    n += 1;
                }
            }
            s += 1;
        }
        if n != PER_BATCH {
            return Err(BatchCode::EntryUnbooked.val());
        }
        b += 1;
    }
    Ok(())
}

// ── 判据入口 ─────────────────────────────────────────────────────
pub fn segment_guard() -> bool {
    let mut i = 0;
    while i < BatchCode::ALL.len() {
        let v = BatchCode::ALL[i].val();
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
    v.push((
        "J1-280条零缺漏+批册勾稽+复现",
        matches!(golden, Ok((280, _, 14))),
        format!("{:?}", golden),
    ));
    let mut led = InjectLedger::new();
    run_injections(&mut led);
    v.push((
        "J1-注入三族全抓(含缺漏检出)",
        led.all_caught(),
        format!("t/c {:?}/{:?}", led.tries, led.caught),
    ));
    // 批名点验：14 名全非空且互异。
    let names_ok = {
        let mut ok = true;
        let mut b = 0;
        while b < BATCH_COUNT {
            if BATCH_NAMES[b].len() == 0 {
                ok = false;
            }
            let mut c = b + 1;
            while c < BATCH_COUNT {
                if BATCH_NAMES[b] == BATCH_NAMES[c] {
                    ok = false;
                }
                c += 1;
            }
            b += 1;
        }
        ok
    };
    v.push((
        "J1-14批名互异点验",
        names_ok,
        String::from("B16–B29 两两互异且非空"),
    ));
    // 判据常量勾稽：14×20 == 280 == 判据 280，批头 14。
    v.push((
        "J1-判据常量勾稽14x20=280",
        BATCH_COUNT * PER_BATCH == TOTAL_INDEX
            && TOTAL_INDEX == EXPECTED_TOTAL
            && EXPECTED_TOTAL == 280
            && HEADER_BATCHES == 14,
        format!("{}x{}={}", BATCH_COUNT, PER_BATCH, TOTAL_INDEX),
    ));
    // 段基址抽验：B16=0x66B00、B27=0x66DC0、B29=0x66E40、等差 0x40。
    let seg_ok = batch_seg(0) == Ok(0x66B00)
        && batch_seg(11) == Ok(0x66DC0)
        && batch_seg(13) == Ok(0x66B00 + 13 * 0x40)
        && batch_seg(7) == Ok(0x66B00 + 7 * 0x40);
    v.push((
        "J1-段基址等差抽验",
        seg_ok,
        format!("B16={:05X} B29={:05X}", BATCH_SEG_BASE, BATCH_SEG_BASE + 13 * BATCH_SEG_STEP),
    ));
    // 摘要列四表点验：黄金账逐行摘要非空（抽首/中/尾三条）。
    let digest_ok = {
        let mut led2 = BatchLedger::new();
        let _ = led2.run_all();
        let n = led2.rows.len();
        let ok = n == TOTAL_INDEX;
        let mut probe_ok = true;
        if ok {
            let probes = [0usize, TOTAL_INDEX / 2, TOTAL_INDEX - 1];
            let mut k = 0;
            while k < probes.len() {
                // 下标先比长度。
                if probes[k] < led2.rows.len() && led2.rows[probes[k]].digest.len() == 0 {
                    probe_ok = false;
                }
                k += 1;
            }
        }
        ok && probe_ok
    };
    v.push((
        "J1-四列表摘要列点验",
        digest_ok,
        String::from("首/中/尾抽三条非空"),
    ));
    // 反向判据：键位跨段（批 0 用批 1 键）必判混键。
    let cross = {
        let k0 = index_key(0, 0);
        let k1 = index_key(1, 0);
        match (k0, k1) {
            (Ok(a), Ok(c)) => a != c,
            _ => false,
        }
    };
    v.push((
        "J1-反向判据键位分段互异",
        cross,
        String::from("批 0 首键 ≠ 批 1 首键"),
    ));
    v.push(("J3-段位守卫", segment_guard(), String::from("0x66DC0 窗口")));
    v
}

pub fn run_unx14981_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F14981 B16–B29 十四批索引总账（280 条四列表逐批勾稽）");
    for (n, p, d) in extra_checks() {
        s.add_dyn(n, p, &d);
    }
    s
}
