//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F1486 · 补全执行四（账本与回归段 B31–B34 密度均衡） · 0x75B40段
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（分派表逐字锚定，AI-VX7-1104 批4-J）：
//!   4 批密度比红线（行数/配额 ≥1.5）40/40 批通过，均衡断言入总账
//!   只增不减，均衡漏项注入 10/10 必抓
//! 设计要点：
//!   1. 批闭集：账本与回归段冻结四批 B31~B34，判据配额闭集
//!      （6/5/6/5），每批断言配额冻结 10 行，4×10 = 40/40 口径；
//!   2. 密度比 = 行数/配额（×10000 定点整数，红线 15000 即 ≥1.5），
//!      四批逐批过线才叫 40/40 通过；
//!   3. 三段单向状态机（Draft→Balanced→Sealed）：密度过线才许进
//!      Balanced，行数足额才许封 Sealed，倒退显性拒；
//!   4. 均衡断言 append-only 入总账（唯一语料键 批-序），只增不减
//!      （长度 + 前缀稳定双闸审计）；
//!   5. 均衡漏项注入 10/10：每轮漏一行，行数足额闸/密度线必抓；
//!   6. LCG 种子写死 = SEG_BASE；no_std + alloc；整数运算零浮点。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x75B40，AI-VX7-1104 批4-J 独占槽）。
pub const SEG_BASE: u32 = 0x75B40;

// ── 批闭集与冻结配额 ────────────────────────────────────────────
/// 四批批名闭集（账本与回归段，F1482 冻结序）。
pub const DENSITY_BATCHES: [&str; 4] = ["B31", "B32", "B33", "B34"];
/// 每批判据配额闭集（密度比分母口径）。
pub const BATCH_QUOTAS: [u64; 4] = [6, 5, 6, 5];
/// 每批断言行配额（冻结 10，4×10 = 40/40）。
pub const ROWS_PER_BATCH: usize = 10;
/// 密度红线（×10000 定点：≥1.5 → 15000）。
pub const DENSITY_FLOOR: u64 = 15_000;
/// 定点倍率。
pub const DENSITY_SCALE: u64 = 10_000;
/// 40/40 总断言大关。
pub const TOTAL_GATE: usize = 40;

/// 判据迭代轮数（写死：判据 10/10 语义）。
pub const JUDGE_ROUNDS: u32 = 10;

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DnCode {
    /// 批号越闭集
    BatchOutOfRange,
    /// 空断言行（语料为空）
    EmptyAssert,
    /// 行数不足配额（<10）
    QuotaShort,
    /// 密度比越线（<1.5）
    DensityBelowLine,
    /// 批阶段倒退
    StageRegress,
    /// 总账删除（append-only 红线）
    LedgerDeletion,
    /// 40/40 大关未过
    GateShort,
    /// 漏项注入漏抓
    MissingLeaked,
    /// 双跑不一致
    NotReproducible,
}

impl DnCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            DnCode::BatchOutOfRange => "批号越闭集：账本与回归段仅冻结 B31~B34 四批，建议核对分派批序",
            DnCode::EmptyAssert => "空断言行：语料不得为空，建议每行带批名-序号可对账键",
            DnCode::QuotaShort => "行数不足配额：每批冻结 10 行，建议补齐漏行后再封批",
            DnCode::DensityBelowLine => "密度比越线：行数/配额 <1.5，建议按配额补写断言或合并判据",
            DnCode::StageRegress => "批阶段倒退：单向状态机红线，建议按 Draft→Balanced→Sealed 单向推进",
            DnCode::LedgerDeletion => "总账删除：append-only 红线，建议只追加不回撤，漏行走重跑",
            DnCode::GateShort => "40/40 大关未过：四批 ×10 行未齐，建议逐批对账补齐后复验",
            DnCode::MissingLeaked => "漏项注入漏抓：复验未检出漏行，建议复核足额谓词方向",
            DnCode::NotReproducible => "双跑不一致：复现红线，建议排查非确定性来源（随机/墙钟/迭代序）",
        }
    }
    pub const ALL: [DnCode; 9] = [
        DnCode::BatchOutOfRange,
        DnCode::EmptyAssert,
        DnCode::QuotaShort,
        DnCode::DensityBelowLine,
        DnCode::StageRegress,
        DnCode::LedgerDeletion,
        DnCode::GateShort,
        DnCode::MissingLeaked,
        DnCode::NotReproducible,
    ];
}

/// 诊断码查表（闭集返回，未知码显性文案）。
pub fn detail(code: u32) -> &'static str {
    let mut i = 0;
    while i < DnCode::ALL.len() {
        if let Some(c) = DnCode::ALL.get(i) {
            if c.val() == code {
                return c.detail();
            }
        }
        i += 1;
    }
    "未知诊断码：不在 0x75B40 段闭集内，建议核对诊断码表"
}

// ── 批阶段：单向状态机 ──────────────────────────────────────────
/// 批三段：草稿→均衡→封批（单向推进，倒退显性拒）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DnStage {
    /// 草稿（行数未足）
    Draft,
    /// 均衡已过线（密度 ≥1.5）
    Balanced,
    /// 封批（行数足额且已复验）
    Sealed,
}

impl DnStage {
    pub fn rank(self) -> u8 {
        match self {
            DnStage::Draft => 0,
            DnStage::Balanced => 1,
            DnStage::Sealed => 2,
        }
    }
}

// ── 批密度账 ────────────────────────────────────────────────────
/// 单批密度账：批名 + 判据配额 + 断言行 + 阶段。
#[derive(Clone, Debug)]
pub struct DensityBatch {
    pub name: String,
    pub quota: u64,
    pub stage: DnStage,
    pub rows: Vec<String>,
}

impl DensityBatch {
    fn new(name: &str, quota: u64) -> DensityBatch {
        DensityBatch {
            name: String::from(name),
            quota,
            stage: DnStage::Draft,
            rows: Vec::new(),
        }
    }

    /// 行数。
    pub fn row_count(&self) -> usize {
        self.rows.len()
    }

    /// 密度比（×10000 定点：行数 ×10000 ÷ 配额；配额 0 视为 0）。
    pub fn density(&self) -> u64 {
        if self.quota == 0 {
            return 0;
        }
        (self.rows.len() as u64) * DENSITY_SCALE / self.quota
    }

    /// 密度线谓词。
    pub fn density_ok(&self) -> bool {
        self.density() >= DENSITY_FLOOR
    }

    /// 阶段推进（倒退显性拒）。
    pub fn advance(&mut self, target: DnStage) -> Result<DnStage, u32> {
        if target.rank() < self.stage.rank() {
            return Err(DnCode::StageRegress.val());
        }
        self.stage = target;
        Ok(self.stage)
    }
}

// ── 均衡总账 ────────────────────────────────────────────────────
/// 四批均衡总账（append-only）。
#[derive(Clone, Debug)]
pub struct DensityLedger {
    pub batches: Vec<DensityBatch>,
    /// 总账（均衡断言逐行追加，只增不减）。
    pub ledger: Vec<String>,
}

impl DensityLedger {
    pub fn new() -> DensityLedger {
        let mut batches = Vec::new();
        let mut i = 0;
        while i < DENSITY_BATCHES.len() {
            let name = match DENSITY_BATCHES.get(i) {
                Some(n) => *n,
                None => "",
            };
            let quota = match BATCH_QUOTAS.get(i) {
                Some(q) => *q,
                None => 0,
            };
            batches.push(DensityBatch::new(name, quota));
            i += 1;
        }
        DensityLedger { batches, ledger: Vec::new() }
    }

    /// 追加一行断言（批界/空语料两闸 + 阶段推进 Draft→Balanced）。
    pub fn add_row(&mut self, batch: usize, text: &str) -> Result<usize, u32> {
        if batch >= self.batches.len() {
            return Err(DnCode::BatchOutOfRange.val());
        }
        if text.is_empty() {
            return Err(DnCode::EmptyAssert.val());
        }
        let stage = match self.batches.get(batch) {
            Some(b) => b.stage,
            None => return Err(DnCode::BatchOutOfRange.val()),
        };
        if stage == DnStage::Sealed {
            return Err(DnCode::StageRegress.val());
        }
        // 语料键：批名-序号（可对账唯一键）。
        let bname = match self.batches.get(batch) {
            Some(b) => b.name.clone(),
            None => return Err(DnCode::BatchOutOfRange.val()),
        };
        let seq = match self.batches.get(batch) {
            Some(b) => b.row_count(),
            None => 0,
        };
        let line = format!("{}-{:02} · {} · 密度均衡口径", bname, seq, text);
        {
            let b = match self.batches.get_mut(batch) {
                Some(x) => x,
                None => return Err(DnCode::BatchOutOfRange.val()),
            };
            b.rows.push(line.clone());
            // 密度过线即升 Balanced（同阶/升阶放行）。
            if b.density_ok() {
                let _ = b.advance(DnStage::Balanced);
            }
        }
        self.ledger.push(line);
        Ok(self.ledger.len() - 1)
    }

    /// 批封批：行数足额 + 密度过线才许 Balanced→Sealed。
    pub fn seal_batch(&mut self, batch: usize) -> Result<u64, u32> {
        if batch >= self.batches.len() {
            return Err(DnCode::BatchOutOfRange.val());
        }
        let (rows, ok, stage) = match self.batches.get(batch) {
            Some(b) => (b.row_count(), b.density_ok(), b.stage),
            None => return Err(DnCode::BatchOutOfRange.val()),
        };
        if rows < ROWS_PER_BATCH {
            return Err(DnCode::QuotaShort.val());
        }
        if !ok {
            return Err(DnCode::DensityBelowLine.val());
        }
        if stage == DnStage::Sealed {
            return Ok(match self.batches.get(batch) {
                Some(b) => b.density(),
                None => 0,
            });
        }
        match self.batches.get_mut(batch) {
            Some(b) => b.advance(DnStage::Sealed).map(|_| b.density()),
            None => Err(DnCode::BatchOutOfRange.val()),
        }
    }

    /// 批阶段设置（倒退显性拒——注入面用）。
    pub fn force_stage(&mut self, batch: usize, target: DnStage) -> Result<(), u32> {
        match self.batches.get_mut(batch) {
            Some(b) => b.advance(target).map(|_| ()),
            None => Err(DnCode::BatchOutOfRange.val()),
        }
    }

    /// 40/40 大关：总账 40 行 + 四批全 Sealed + 逐批过线。
    pub fn gate_4040(&self) -> Result<usize, u32> {
        if self.ledger.len() < TOTAL_GATE {
            return Err(DnCode::GateShort.val());
        }
        let mut b = 0;
        while b < self.batches.len() {
            match self.batches.get(b) {
                Some(x) => {
                    if x.stage != DnStage::Sealed || !x.density_ok() || x.row_count() < ROWS_PER_BATCH {
                        return Err(DnCode::GateShort.val());
                    }
                }
                None => return Err(DnCode::BatchOutOfRange.val()),
            }
            b += 1;
        }
        Ok(self.ledger.len())
    }

    /// 只增不减审计：总账长度不减 + 前缀逐行稳定 + 语料全非空唯一。
    pub fn append_only_audit(&self, prev: &[String]) -> Result<(), u32> {
        if self.ledger.len() < prev.len() {
            return Err(DnCode::LedgerDeletion.val());
        }
        let mut i = 0;
        while i < prev.len() {
            match (prev.get(i), self.ledger.get(i)) {
                (Some(a), Some(b)) => {
                    if a != b {
                        return Err(DnCode::LedgerDeletion.val());
                    }
                }
                _ => return Err(DnCode::LedgerDeletion.val()),
            }
            i += 1;
        }
        let mut j = 0;
        while j < self.ledger.len() {
            let a = match self.ledger.get(j) {
                Some(x) => x,
                None => return Err(DnCode::LedgerDeletion.val()),
            };
            if a.is_empty() {
                return Err(DnCode::EmptyAssert.val());
            }
            let mut k = j + 1;
            while k < self.ledger.len() {
                if let Some(b) = self.ledger.get(k) {
                    if a == b {
                        return Err(DnCode::LedgerDeletion.val());
                    }
                }
                k += 1;
            }
            j += 1;
        }
        Ok(())
    }
}

impl Default for DensityLedger {
    fn default() -> DensityLedger {
        DensityLedger::new()
    }
}

// ── 断言语料键闭集（账本与回归段判据点名） ──────────────────────
pub const CORPUS_KEYS: [&str; 10] = [
    "账本位图账平",
    "回归红批显红",
    "只增不减审计",
    "密度线复验",
    "漏项注入必抓",
    "批状态单调",
    "语料键唯一",
    "双跑复现一致",
    "段位窗口守卫",
    "总账行数对账",
];

// ── 全量均衡（四批 × 10 行 → 40/40） ────────────────────────────
pub fn balance_all() -> Result<DensityLedger, u32> {
    if DENSITY_BATCHES.len() != 4 || BATCH_QUOTAS.len() != 4 {
        return Err(DnCode::BatchOutOfRange.val());
    }
    let mut led = DensityLedger::new();
    let mut b = 0;
    while b < DENSITY_BATCHES.len() {
        let mut k = 0;
        while k < ROWS_PER_BATCH {
            let key = match CORPUS_KEYS.get(k) {
                Some(x) => *x,
                None => return Err(DnCode::EmptyAssert.val()),
            };
            led.add_row(b, key)?;
            k += 1;
        }
        led.seal_batch(b)?;
        b += 1;
    }
    led.gate_4040()?;
    Ok(led)
}

// ── 均衡漏项注入 10/10（每轮漏一行，足额/大关必抓） ─────────────
pub fn missing_inject_10() -> Result<u32, u32> {
    let mut caught = 0u32;
    let mut k = 0u32;
    while k < 10 {
        let mut led = DensityLedger::new();
        let mut b = 0;
        while b < DENSITY_BATCHES.len() {
            let mut r = 0;
            while r < ROWS_PER_BATCH {
                // 第 k 轮漏批 0 的第 k 行（k<10 恒落批 0）。
                let skip = b == 0 && r == k as usize;
                if !skip {
                    let key = match CORPUS_KEYS.get(r) {
                        Some(x) => *x,
                        None => "",
                    };
                    let _ = led.add_row(b, key);
                }
                r += 1;
            }
            if led.seal_batch(b).is_err() {
                caught += 1;
            }
            b += 1;
        }
        if led.gate_4040().is_err() {
            caught += 1;
        }
        k += 1;
    }
    let rounds_hit = caught / 2;
    if rounds_hit != 10 {
        return Err(DnCode::MissingLeaked.val());
    }
    Ok(rounds_hit)
}

// ── LCG（种子写死 = SEG_BASE） ──────────────────────────────────
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

// ── 注入检出账（四槽：越批/空语料/漏行/阶段倒退） ───────────────
pub struct InjectLedger {
    pub tries: [u32; 4],
    pub caught: [u32; 4],
}
impl InjectLedger {
    pub fn new() -> InjectLedger {
        InjectLedger { tries: [0; 4], caught: [0; 4] }
    }
    pub fn record(&mut self, slot: usize, hit: bool) {
        if slot < 4 {
            self.tries[slot] += 1;
            if hit {
                self.caught[slot] += 1;
            }
        }
    }
    pub fn all_caught(&self) -> bool {
        let mut i = 0;
        while i < 4 {
            let t = match self.tries.get(i) {
                Some(v) => *v,
                None => 0,
            };
            let c = match self.caught.get(i) {
                Some(v) => *v,
                None => 0,
            };
            if t == 0 || c != t {
                return false;
            }
            i += 1;
        }
        true
    }
}

// ── 黄金全链：均衡→40/40→只增审计→漏项 10/10 ───────────────────
pub fn golden_run() -> Result<(usize, u64, u32), u32> {
    let led = balance_all()?;
    let gate = led.gate_4040()?;
    // 密度全表（四批逐批过线）。
    let mut min_density = u64::MAX;
    let mut b = 0;
    while b < led.batches.len() {
        if let Some(x) = led.batches.get(b) {
            if x.density() < min_density {
                min_density = x.density();
            }
        }
        b += 1;
    }
    if min_density < DENSITY_FLOOR {
        return Err(DnCode::DensityBelowLine.val());
    }
    // 只增审计：半账前缀稳定。
    let mut half = DensityLedger::new();
    let mut hb = 0;
    while hb < 2 {
        let mut hk = 0;
        while hk < ROWS_PER_BATCH {
            let key = match CORPUS_KEYS.get(hk) {
                Some(x) => *x,
                None => "",
            };
            half.add_row(hb, key)?;
            hk += 1;
        }
        hb += 1;
    }
    led.append_only_audit(&half.ledger)?;
    let caught = missing_inject_10()?;
    Ok((gate, min_density, caught))
}

// ── 判据谓词（私有） ────────────────────────────────────────────
/// 批闭集谓词：四批齐备 + 配额非零 + 每批 10 行下密度全过线。
fn closed_set_ok() -> bool {
    if DENSITY_BATCHES.len() != 4 || BATCH_QUOTAS.len() != 4 || ROWS_PER_BATCH != 10 {
        return false;
    }
    let mut i = 0;
    while i < DENSITY_BATCHES.len() {
        let name = match DENSITY_BATCHES.get(i) {
            Some(n) => *n,
            None => return false,
        };
        let quota = match BATCH_QUOTAS.get(i) {
            Some(q) => *q,
            None => return false,
        };
        if name.is_empty() || quota == 0 {
            return false;
        }
        // 冻结行数下密度必须过线（否则清单自相矛盾）。
        let d = (ROWS_PER_BATCH as u64) * DENSITY_SCALE / quota;
        if d < DENSITY_FLOOR {
            return false;
        }
        i += 1;
    }
    true
}

/// 密度算术谓词：定点比与手算一致（B31：10/6 = 16666，B32：10/5 = 20000）。
fn density_math_ok() -> bool {
    let mut b = DensityBatch::new("B31", 6);
    let mut i = 0;
    while i < ROWS_PER_BATCH {
        b.rows.push(format!("行{}", i));
        i += 1;
    }
    if b.density() != 16_666 {
        return false;
    }
    let mut c = DensityBatch::new("B32", 5);
    let mut j = 0;
    while j < ROWS_PER_BATCH {
        c.rows.push(format!("行{}", j));
        j += 1;
    }
    if c.density() != 20_000 {
        return false;
    }
    true
}

/// 状态机单调谓词：任何倒退必被拒。
fn stage_monotonic_ok() -> bool {
    let mut led = DensityLedger::new();
    if led.force_stage(0, DnStage::Sealed).is_err() {
        return false;
    }
    if led.force_stage(0, DnStage::Balanced).is_ok() {
        return false;
    }
    if led.force_stage(0, DnStage::Draft).is_ok() {
        return false;
    }
    if led.force_stage(1, DnStage::Balanced).is_err() {
        return false;
    }
    if led.force_stage(1, DnStage::Draft).is_ok() {
        return false;
    }
    true
}

// ── 探针三序 ────────────────────────────────────────────────────
/// 序 A（正向）：单行入账 + 空前缀只增审计。
pub fn probe_seq_a() -> Result<(), u32> {
    let mut led = DensityLedger::new();
    led.add_row(0, "探针行")?;
    led.append_only_audit(&[])
}

/// 序 B（越批必拒）：批号 4 出闭集。
pub fn probe_seq_b() -> Result<(), u32> {
    let mut led = DensityLedger::new();
    led.add_row(4, "越批行").map(|_| ())
}

/// 序 C（足额必拒）：1 行即封批。
pub fn probe_seq_c() -> Result<(), u32> {
    let mut led = DensityLedger::new();
    led.add_row(0, "孤行")?;
    led.seal_batch(0).map(|_| ())
}

// ── 注入轮（LCG 驱动 rounds × 四槽） ────────────────────────────
pub fn inject_rounds(rounds: u32) -> Result<InjectLedger, u32> {
    let mut led = InjectLedger::new();
    let mut rng = Lcg::new(SEG_BASE as u64);
    let mut r = 0;
    while r < rounds {
        let _ = rng.next();
        // 槽0：越批。
        let mut l0 = DensityLedger::new();
        led.record(0, l0.add_row(99, "越批").is_err());
        // 槽1：空语料。
        let mut l1 = DensityLedger::new();
        led.record(1, l1.add_row(0, "").is_err());
        // 槽2：漏行抓。
        led.record(2, missing_inject_10() == Ok(10));
        // 槽3：阶段倒退 + 封批拒写。
        let mut l3 = DensityLedger::new();
        let _ = l3.force_stage(2, DnStage::Sealed);
        let regress = l3.add_row(2, "封后行").is_err();
        let mut l3b = DensityLedger::new();
        let _ = l3b.add_row(2, "先行");
        let _ = l3b.force_stage(2, DnStage::Balanced);
        let regress2 = l3b.force_stage(2, DnStage::Draft).is_err();
        led.record(3, regress && regress2);
        r += 1;
    }
    Ok(led)
}

// ── 双跑一致 ────────────────────────────────────────────────────
pub fn repro_check() -> Result<(), u32> {
    let a = golden_run()?;
    let b = golden_run()?;
    if a != b {
        return Err(DnCode::NotReproducible.val());
    }
    Ok(())
}

// ── 段位守卫（窗口 + 对齐 + 区间三闸） ──────────────────────────
pub fn segment_guard() -> bool {
    if SEG_BASE % 0x40 != 0 || SEG_BASE < 0x75000 || SEG_BASE > 0x781C0 {
        return false;
    }
    let mut i = 0;
    while i < DnCode::ALL.len() {
        if let Some(c) = DnCode::ALL.get(i) {
            let v = c.val();
            if v < SEG_BASE || v >= SEG_BASE + 0x40 {
                return false;
            }
        }
        i += 1;
    }
    true
}

// ── 判据面（extra_checks：≥12 条 add_dyn） ──────────────────────
pub fn extra_checks() -> Vec<(&'static str, bool, String)> {
    let mut v: Vec<(&'static str, bool, String)> = Vec::new();
    // 1. 批闭集与配额。
    v.push((
        "J1-批闭集配额",
        closed_set_ok(),
        format!(
            "四批={:?} 配额={:?} 每批={} 行",
            DENSITY_BATCHES, BATCH_QUOTAS, ROWS_PER_BATCH
        ),
    ));
    // 2. 密度算术（定点对拍）。
    v.push((
        "J1-密度定点算术",
        density_math_ok(),
        String::from("10/6=16666 · 10/5=20000（×10000 定点）"),
    ));
    // 3. 黄金全链。
    let g = golden_run();
    v.push((
        "J1-黄金全链",
        match &g {
            Ok((gate, min_d, caught)) => *gate == 40 && *min_d >= DENSITY_FLOOR && *caught == 10,
            Err(_) => false,
        },
        match &g {
            Ok((gate, min_d, caught)) => format!("总行={} 最低密度={} 漏抓={}", gate, min_d, caught),
            Err(e) => format!("err=0x{:x}", e),
        },
    ));
    // 4. 40/40 大关。
    let led = balance_all();
    v.push((
        "J1-40/40大关",
        match &led {
            Ok(l) => l.gate_4040() == Ok(40),
            Err(_) => false,
        },
        match &led {
            Ok(l) => format!("总账={} 大关={}", l.ledger.len(), TOTAL_GATE),
            Err(e) => format!("err=0x{:x}", e),
        },
    ));
    // 5. 四批密度全过线（≥1.5）。
    let line_ok = match &led {
        Ok(l) => {
            let mut ok = true;
            let mut b = 0;
            while b < l.batches.len() {
                if let Some(x) = l.batches.get(b) {
                    if !x.density_ok() {
                        ok = false;
                    }
                }
                b += 1;
            }
            ok && l.batches.len() == 4
        }
        Err(_) => false,
    };
    v.push((
        "J1-密度红线≥1.5全过",
        line_ok,
        String::from("四批 ×10000 定点 ≥15000"),
    ));
    // 6. 三段状态机单调。
    v.push((
        "J1-三段状态机单调",
        stage_monotonic_ok(),
        String::from("Draft→Balanced→Sealed 倒退全拒"),
    ));
    // 7. 均衡断言只增不减。
    let append_ok = match &led {
        Ok(l) => l.append_only_audit(&[]).is_ok() && l.ledger.len() == 40,
        Err(_) => false,
    };
    v.push((
        "J1-总账只增不减",
        append_ok,
        String::from("append-only + 前缀稳定 + 语料唯一"),
    ));
    // 8. 均衡漏项注入 10/10。
    v.push((
        "J1-漏项注入10/10",
        missing_inject_10() == Ok(10),
        format!("{}/10 轮抓", match missing_inject_10() {
            Ok(n) => n,
            Err(_) => 0,
        }),
    ));
    // 9. 注入四槽 10/10。
    let inj = inject_rounds(JUDGE_ROUNDS);
    v.push((
        "J1-注入四槽10/10",
        match &inj {
            Ok(l) => l.all_caught(),
            Err(_) => false,
        },
        match &inj {
            Ok(l) => format!("t={:?} c={:?}", l.tries, l.caught),
            Err(e) => format!("err=0x{:x}", e),
        },
    ));
    // 10. 探针三序。
    let a = probe_seq_a().is_ok();
    let b = probe_seq_b().is_err();
    let c = probe_seq_c().is_err();
    v.push((
        "J1-探针三序",
        a && b && c,
        format!("a正向={} b越批拒={} c足额拒={}", a, b, c),
    ));
    // 11. 诊断码闭集对账。
    let codes_ok = {
        let mut ok = true;
        let mut i = 0;
        while i < DnCode::ALL.len() {
            if let Some(c) = DnCode::ALL.get(i) {
                if detail(c.val()) != c.detail() {
                    ok = false;
                }
            }
            i += 1;
        }
        ok && detail(SEG_BASE + 0x3F).starts_with("未知诊断码")
    };
    v.push((
        "J1-诊断码闭集对账",
        codes_ok,
        format!("{} 码全查表通过", DnCode::ALL.len()),
    ));
    // 12. 双跑一致。
    v.push((
        "J2-双跑一致",
        repro_check().is_ok(),
        String::from("golden_run 两跑同值"),
    ));
    // 13. 段位守卫。
    v.push((
        "J3-段位守卫",
        segment_guard(),
        String::from("0x75B40 窗口 0x40 对齐区间三闸"),
    ));
    // 14. LCG 确定性。
    let lcg_ok = {
        let mut x = Lcg::new(SEG_BASE as u64);
        let mut y = Lcg::new(SEG_BASE as u64);
        let mut ok = true;
        let mut i = 0;
        while i < 16 {
            if x.next() != y.next() {
                ok = false;
            }
            i += 1;
        }
        ok
    };
    v.push(("J2-LCG确定性", lcg_ok, String::from("同种子 16 步逐位一致")));
    v
}

pub fn run_unx1486_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F1486 补全执行四（账本与回归段 B31–B34 密度均衡）");
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
    fn golden_run_green() {
        match golden_run() {
            Ok((gate, min_d, caught)) => {
                assert_eq!(gate, 40);
                assert!(min_d >= DENSITY_FLOOR);
                assert_eq!(caught, 10);
            }
            Err(e) => assert!(false, "golden_run err=0x{:x}", e),
        }
    }

    #[test]
    fn closed_set_shape() {
        assert!(closed_set_ok());
        assert!(density_math_ok());
        assert_eq!(DENSITY_BATCHES.len(), 4);
        assert_eq!(ROWS_PER_BATCH * DENSITY_BATCHES.len(), 40);
        assert_eq!(DENSITY_FLOOR, 15_000);
    }

    #[test]
    fn batch_range_and_empty_caught() {
        let mut led = DensityLedger::new();
        match led.add_row(4, "越批") {
            Err(e) => assert_eq!(e, DnCode::BatchOutOfRange.val()),
            Ok(_) => assert!(false, "越批未拒"),
        }
        match led.add_row(0, "") {
            Err(e) => assert_eq!(e, DnCode::EmptyAssert.val()),
            Ok(_) => assert!(false, "空语料未拒"),
        }
    }

    #[test]
    fn quota_short_blocks_seal() {
        let mut led = DensityLedger::new();
        let _ = led.add_row(1, "孤行");
        match led.seal_batch(1) {
            Err(e) => assert_eq!(e, DnCode::QuotaShort.val()),
            Ok(_) => assert!(false, "1 行不应封批"),
        }
    }

    #[test]
    fn density_line_enforced() {
        // 批 0 配额 6：9 行密度 15000（恰过线），行数闸先拒封批；
        // 构造低密度批：配额闭集最小 5，10 行即 20000——用 force 场景
        // 验证 DensityBelowLine 语义（单批手工低配额账）。
        let mut low = DensityBatch::new("低配", 9);
        let mut i = 0;
        while i < 10 {
            low.rows.push(format!("行{}", i));
            i += 1;
        }
        match low.density() {
            11_111 => {}
            d => assert!(false, "低密度手算失配 d={}", d),
        }
        assert!(!low.density_ok(), "10/9=11111 应低于红线");
    }

    #[test]
    fn stage_regress_caught() {
        assert!(stage_monotonic_ok());
        let mut led = DensityLedger::new();
        assert!(led.force_stage(0, DnStage::Sealed).is_ok());
        match led.force_stage(0, DnStage::Draft) {
            Err(e) => assert_eq!(e, DnCode::StageRegress.val()),
            Ok(_) => assert!(false, "阶段倒退未拒"),
        }
        match led.add_row(0, "封后行") {
            Err(e) => assert_eq!(e, DnCode::StageRegress.val()),
            Ok(_) => assert!(false, "封批后写入未拒"),
        }
    }

    #[test]
    fn missing_inject_all_caught() {
        assert_eq!(missing_inject_10(), Ok(10));
    }

    #[test]
    fn probe_sequences() {
        assert!(probe_seq_a().is_ok());
        assert!(probe_seq_b().is_err());
        assert!(probe_seq_c().is_err());
    }

    #[test]
    fn reproducible_two_runs() {
        assert!(repro_check().is_ok());
        assert_eq!(golden_run(), golden_run());
    }

    #[test]
    fn append_only_and_guard() {
        let mut led = DensityLedger::new();
        assert!(led.add_row(3, "首行").is_ok());
        let snap = led.ledger.clone();
        assert!(led.add_row(3, "次行").is_ok());
        assert!(led.append_only_audit(&snap).is_ok());
        assert!(segment_guard());
        assert_eq!(SEG_BASE % 0x40, 0);
        assert_eq!(detail(DnCode::DensityBelowLine.val()), DnCode::DensityBelowLine.detail());
        assert!(detail(SEG_BASE + 0x3F).starts_with("未知诊断码"));
    }
}
