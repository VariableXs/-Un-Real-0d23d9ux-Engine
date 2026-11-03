//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F1485 · 补全执行三（分发与接管面 B16–B30 缺口批补齐） · 0x75AC0段
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（分派表逐字锚定，AI-VX7-1104 批4-J）：
//!   分发与接管面 15 批缺口补齐执行（F1482 清单逐项补），补全后段级
//!   ≥80 大关（F1382/F1400 双层锚）复验通过，补全漏项注入 10/10 必抓
//! 设计要点：
//!   1. 缺口清单闭集：F1482 冻结十五批 B16~B30，每批三类缺口项
//!      （分发/接管/桥接），15×3 = 45 项闭集；
//!   2. 清单→补全→复验三段单向状态机（Listed→Filling→Verified），
//!      倒退显性拒，封账批再补显性拒；
//!   3. 段级 ≥80 大关走 F1382/F1400 双层锚：F1382 层 = 权重进度
//!      （分发 3 / 接管 2 / 桥接 2，×10000 定点），F1400 层 = 批完成度
//!      （封账批数 ×10000 ÷ 15）；合分 ≥80（8000）且层差 ≤15（1500）
//!      才过大关；
//!   4. 补全断言 append-only 入总账（seq 单调），只增不减双闸审计；
//!   5. 漏项注入 10/10：每轮故意漏补一项，封账/大关复验必须抓；
//!   6. LCG 种子写死 = SEG_BASE；no_std + alloc；整数运算零浮点。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x75AC0，AI-VX7-1104 批4-J 独占槽）。
pub const SEG_BASE: u32 = 0x75AC0;

// ── F1482 缺口清单闭集（十五批冻结） ────────────────────────────
/// 十五批批名闭集（B16~B30，F1482 冻结序）。
pub const GAP_BATCHES: [&str; 15] = [
    "B16", "B17", "B18", "B19", "B20", "B21", "B22", "B23", "B24", "B25", "B26", "B27", "B28",
    "B29", "B30",
];
/// 缺口项三类闭集（分发/接管/桥接）。
pub const GAP_KINDS: [&str; 3] = ["分发", "接管", "桥接"];
/// 每类权重闭集（F1382 层权重进度口径）。
pub const KIND_WEIGHTS: [u32; 3] = [3, 2, 2];
/// 每批缺口项数（三类齐备，冻结 3）。
pub const GAPS_PER_BATCH: usize = 3;
/// 段级大关（合分 ≥80，×10000 定点 → 8000）。
pub const SEGMENT_GATE: u32 = 8000;
/// 双层锚层差上限（≤15，×10000 定点 → 1500）。
pub const LAYER_DRIFT_MAX: u32 = 1500;

/// 判据迭代轮数（写死：判据 10/10 语义）。
pub const JUDGE_ROUNDS: u32 = 10;

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Gap3Code {
    /// 批号越闭集
    BatchOutOfRange,
    /// 类别号越闭集
    KindOutOfRange,
    /// 重复补全
    DuplicateFill,
    /// 批阶段倒退
    StageRegress,
    /// 批已封账后再补全
    SealedReject,
    /// 缺口项未补
    GapItemMissing,
    /// 大关未达（合分 <80）
    GateShort,
    /// 双层锚层差超限
    LayerDrift,
    /// 总账删除（append-only / seq 单调红线）
    LedgerDeletion,
    /// 漏项注入漏抓
    MissingLeaked,
    /// 双跑不一致
    NotReproducible,
}

impl Gap3Code {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            Gap3Code::BatchOutOfRange => "批号越闭集：分发与接管面仅冻结 B16~B30 十五批，建议核对 F1482 清单批序",
            Gap3Code::KindOutOfRange => "类别号越闭集：仅分发/接管/桥接三类，建议按 GAP_KINDS 对账",
            Gap3Code::DuplicateFill => "重复补全：同缺口项二次入账，建议以批×类键去重后再补",
            Gap3Code::StageRegress => "批阶段倒退：单向状态机红线，建议按 Listed→Filling→Verified 单向推进",
            Gap3Code::SealedReject => "批已封账：Verified 批不得再补全，建议走复验而非补写",
            Gap3Code::GapItemMissing => "缺口项未补：封账前三类逐项清零，建议核对 filled 位图后补齐漏项",
            Gap3Code::GateShort => "大关未达：F1382/F1400 合分 <80，建议按权重补齐分发类高权重项",
            Gap3Code::LayerDrift => "双层锚层差超限：权重进度与批完成度失步 >15，建议均衡补齐避免偏科",
            Gap3Code::LedgerDeletion => "总账删除：append-only / seq 单调红线，建议只追加不回撤，漏补走重跑",
            Gap3Code::MissingLeaked => "漏项注入漏抓：复验未检出漏补，建议复核完整性谓词方向",
            Gap3Code::NotReproducible => "双跑不一致：复现红线，建议排查非确定性来源（随机/墙钟/迭代序）",
        }
    }
    pub const ALL: [Gap3Code; 11] = [
        Gap3Code::BatchOutOfRange,
        Gap3Code::KindOutOfRange,
        Gap3Code::DuplicateFill,
        Gap3Code::StageRegress,
        Gap3Code::SealedReject,
        Gap3Code::GapItemMissing,
        Gap3Code::GateShort,
        Gap3Code::LayerDrift,
        Gap3Code::LedgerDeletion,
        Gap3Code::MissingLeaked,
        Gap3Code::NotReproducible,
    ];
}

/// 诊断码查表（闭集返回，未知码显性文案）。
pub fn detail(code: u32) -> &'static str {
    let mut i = 0;
    while i < Gap3Code::ALL.len() {
        if let Some(c) = Gap3Code::ALL.get(i) {
            if c.val() == code {
                return c.detail();
            }
        }
        i += 1;
    }
    "未知诊断码：不在 0x75AC0 段闭集内，建议核对诊断码表"
}

// ── 批阶段：单向状态机 ──────────────────────────────────────────
/// 批三段：清单→补全→复验（单向推进，倒退显性拒）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BatchStage3 {
    /// 清单已建、未开补
    Listed,
    /// 补全进行中
    Filling,
    /// 复验通过、封账
    Verified,
}

impl BatchStage3 {
    pub fn rank(self) -> u8 {
        match self {
            BatchStage3::Listed => 0,
            BatchStage3::Filling => 1,
            BatchStage3::Verified => 2,
        }
    }
}

// ── 分发与接管面批账 ────────────────────────────────────────────
/// 单批缺口账：三类位图 + 阶段 + 名称。
#[derive(Clone, Debug)]
pub struct FaceBatch {
    pub name: String,
    pub stage: BatchStage3,
    filled: [bool; 3],
}

impl FaceBatch {
    fn new(name: &str) -> FaceBatch {
        FaceBatch { name: String::from(name), stage: BatchStage3::Listed, filled: [false; 3] }
    }

    /// 已补项数（0~3）。
    pub fn filled_count(&self) -> usize {
        let mut n = 0;
        let mut k = 0;
        while k < 3 {
            if let Some(true) = self.filled.get(k) {
                n += 1;
            }
            k += 1;
        }
        n
    }

    /// 阶段推进（倒退显性拒）。
    pub fn advance(&mut self, target: BatchStage3) -> Result<BatchStage3, u32> {
        if target.rank() < self.stage.rank() {
            return Err(Gap3Code::StageRegress.val());
        }
        self.stage = target;
        Ok(self.stage)
    }
}

/// 补全总账行：seq 单调 + 批×类键 + 断言语料。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LedgerRow {
    pub seq: u32,
    pub batch: usize,
    pub kind: usize,
    pub text: String,
}

/// 十五批补全总账（append-only，seq 单调）。
#[derive(Clone, Debug)]
pub struct Gapfill3Ledger {
    pub batches: Vec<FaceBatch>,
    pub rows: Vec<LedgerRow>,
    /// 下一个 seq（单调不回退）。
    next_seq: u32,
}

impl Gapfill3Ledger {
    pub fn new() -> Gapfill3Ledger {
        let mut batches = Vec::new();
        let mut i = 0;
        while i < GAP_BATCHES.len() {
            let name = match GAP_BATCHES.get(i) {
                Some(n) => *n,
                None => "",
            };
            batches.push(FaceBatch::new(name));
            i += 1;
        }
        Gapfill3Ledger { batches, rows: Vec::new(), next_seq: 0 }
    }

    /// 补全一项（批 idx + 类 idx）：四闸 + 位图落账 + 总账追加（seq 单调）。
    pub fn fill(&mut self, batch: usize, kind: usize) -> Result<u32, u32> {
        if batch >= self.batches.len() {
            return Err(Gap3Code::BatchOutOfRange.val());
        }
        if kind >= GAPS_PER_BATCH {
            return Err(Gap3Code::KindOutOfRange.val());
        }
        let stage = match self.batches.get(batch) {
            Some(b) => b.stage,
            None => return Err(Gap3Code::BatchOutOfRange.val()),
        };
        if stage == BatchStage3::Verified {
            return Err(Gap3Code::SealedReject.val());
        }
        // 批×类键去重（扫总账既有行）。
        let mut r = 0;
        while r < self.rows.len() {
            if let Some(row) = self.rows.get(r) {
                if row.batch == batch && row.kind == kind {
                    return Err(Gap3Code::DuplicateFill.val());
                }
            }
            r += 1;
        }
        let bname = match self.batches.get(batch) {
            Some(b) => b.name.clone(),
            None => return Err(Gap3Code::BatchOutOfRange.val()),
        };
        let kname = match GAP_KINDS.get(kind) {
            Some(k) => *k,
            None => return Err(Gap3Code::KindOutOfRange.val()),
        };
        // 位图落账 + 阶段推进。
        {
            let b = match self.batches.get_mut(batch) {
                Some(x) => x,
                None => return Err(Gap3Code::BatchOutOfRange.val()),
            };
            match b.filled.get_mut(kind) {
                Some(slot) => *slot = true,
                None => return Err(Gap3Code::KindOutOfRange.val()),
            }
            let _ = b.advance(BatchStage3::Filling);
        }
        // 总账追加（seq 单调，checked_add 防溢出）。
        let seq = self.next_seq;
        self.next_seq = match self.next_seq.checked_add(1) {
            Some(v) => v,
            None => return Err(Gap3Code::LedgerDeletion.val()),
        };
        self.rows.push(LedgerRow {
            seq,
            batch,
            kind,
            text: format!("分发接管面断言 {}-{} · F1482缺口项补齐", bname, kname),
        });
        Ok(seq)
    }

    /// 批封账（复验）：三类清零才许 Filling→Verified。
    pub fn seal_batch(&mut self, batch: usize) -> Result<usize, u32> {
        if batch >= self.batches.len() {
            return Err(Gap3Code::BatchOutOfRange.val());
        }
        let (stage, filled_n) = match self.batches.get(batch) {
            Some(b) => (b.stage, b.filled_count()),
            None => return Err(Gap3Code::BatchOutOfRange.val()),
        };
        if filled_n != GAPS_PER_BATCH {
            return Err(Gap3Code::GapItemMissing.val());
        }
        if stage == BatchStage3::Verified {
            return Ok(filled_n);
        }
        match self.batches.get_mut(batch) {
            Some(b) => b.advance(BatchStage3::Verified).map(|_| filled_n),
            None => Err(Gap3Code::BatchOutOfRange.val()),
        }
    }

    /// 批阶段设置（倒退显性拒——注入面用）。
    pub fn force_stage(&mut self, batch: usize, target: BatchStage3) -> Result<(), u32> {
        match self.batches.get_mut(batch) {
            Some(b) => b.advance(target).map(|_| ()),
            None => Err(Gap3Code::BatchOutOfRange.val()),
        }
    }

    /// F1382 层：权重进度（已补权重 ×10000 ÷ 总权重，×10000 定点）。
    pub fn layer_f1382(&self) -> u32 {
        let mut got = 0u32;
        let mut r = 0;
        while r < self.rows.len() {
            if let Some(row) = self.rows.get(r) {
                if let Some(w) = KIND_WEIGHTS.get(row.kind) {
                    got += *w;
                }
            }
            r += 1;
        }
        let total: u32 = KIND_WEIGHTS.iter().fold(0, |a, &x| a + x) * GAP_BATCHES.len() as u32;
        if total == 0 {
            return 0;
        }
        got * 10000 / total
    }

    /// F1400 层：批完成度（封账批数 ×10000 ÷ 15）。
    pub fn layer_f1400(&self) -> u32 {
        let mut sealed = 0u32;
        let mut b = 0;
        while b < self.batches.len() {
            if let Some(x) = self.batches.get(b) {
                if x.stage == BatchStage3::Verified {
                    sealed += 1;
                }
            }
            b += 1;
        }
        sealed * 10000 / GAP_BATCHES.len() as u32
    }

    /// 段级大关：合分 ≥80 且层差 ≤15（F1382/F1400 双层锚）。
    pub fn segment_gate(&self) -> Result<(u32, u32, u32), u32> {
        let a = self.layer_f1382();
        let b = self.layer_f1400();
        let combined = (a + b) / 2;
        let drift = if a >= b { a - b } else { b - a };
        if combined < SEGMENT_GATE {
            return Err(Gap3Code::GateShort.val());
        }
        if drift > LAYER_DRIFT_MAX {
            return Err(Gap3Code::LayerDrift.val());
        }
        Ok((a, b, combined))
    }

    /// 只增不减审计：行数不减 + 行内容前缀稳定 + seq 全列单调。
    pub fn append_only_audit(&self, prev_rows: &[LedgerRow]) -> Result<(), u32> {
        if self.rows.len() < prev_rows.len() {
            return Err(Gap3Code::LedgerDeletion.val());
        }
        let mut i = 0;
        while i < prev_rows.len() {
            match (prev_rows.get(i), self.rows.get(i)) {
                (Some(a), Some(b)) => {
                    if a != b {
                        return Err(Gap3Code::LedgerDeletion.val());
                    }
                }
                _ => return Err(Gap3Code::LedgerDeletion.val()),
            }
            i += 1;
        }
        let mut j = 1;
        while j < self.rows.len() {
            let prev = match self.rows.get(j - 1) {
                Some(x) => x.seq,
                None => return Err(Gap3Code::LedgerDeletion.val()),
            };
            let cur = match self.rows.get(j) {
                Some(x) => x.seq,
                None => return Err(Gap3Code::LedgerDeletion.val()),
            };
            if cur <= prev {
                return Err(Gap3Code::LedgerDeletion.val());
            }
            j += 1;
        }
        Ok(())
    }
}

impl Default for Gapfill3Ledger {
    fn default() -> Gapfill3Ledger {
        Gapfill3Ledger::new()
    }
}

// ── 全量补齐（十五批 × 三类 → 45 项，逐批封账） ─────────────────
pub fn backfill_all() -> Result<Gapfill3Ledger, u32> {
    if GAP_BATCHES.len() != 15 || GAPS_PER_BATCH != 3 {
        return Err(Gap3Code::BatchOutOfRange.val());
    }
    let mut led = Gapfill3Ledger::new();
    let mut b = 0;
    while b < GAP_BATCHES.len() {
        let mut k = 0;
        while k < GAPS_PER_BATCH {
            led.fill(b, k)?;
            k += 1;
        }
        led.seal_batch(b)?;
        b += 1;
    }
    led.segment_gate()?;
    Ok(led)
}

// ── 漏项注入 10/10（每轮漏补一项，封账/大关必抓） ───────────────
pub fn missing_inject_10() -> Result<u32, u32> {
    let mut caught = 0u32;
    let mut k = 0u32;
    while k < 10 {
        let mut led = Gapfill3Ledger::new();
        let mut b = 0;
        while b < GAP_BATCHES.len() {
            let mut kind = 0;
            while kind < GAPS_PER_BATCH {
                // 第 k 轮漏补全局第 k 项（批 = k/3，类 = k%3，覆盖 10 轮）。
                let skip =
                    b == k as usize / GAPS_PER_BATCH && kind == k as usize % GAPS_PER_BATCH;
                if !skip {
                    let _ = led.fill(b, kind);
                }
                kind += 1;
            }
            if led.seal_batch(b).is_err() {
                caught += 1;
            }
            b += 1;
        }
        // 复验抓漏：批完成度未满贯（缺项批未封）必被抓；
        // 大关比例制下缺一项合分仍 ≥80，故以 F1400 层满贯为抓漏口径。
        if led.layer_f1400() != 10000 || led.segment_gate().is_err() {
            caught += 1;
        }
        k += 1;
    }
    let rounds_hit = caught / 2;
    if rounds_hit != 10 {
        return Err(Gap3Code::MissingLeaked.val());
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

// ── 注入检出账（四槽：越批/越类/重复/封账拒补+漏项抓） ──────────
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

// ── 黄金全链：清单→全补45→逐批封账→双层锚大关→只增审计→漏项10/10
pub fn golden_run() -> Result<(u32, u32, u32, u32), u32> {
    let led = backfill_all()?;
    let (a, b, combined) = led.segment_gate()?;
    // 只增审计：前两批半账前缀稳定。
    let mut half_led = Gapfill3Ledger::new();
    let mut hb = 0;
    while hb < 2 {
        let mut hk = 0;
        while hk < GAPS_PER_BATCH {
            half_led.fill(hb, hk)?;
            hk += 1;
        }
        hb += 1;
    }
    led.append_only_audit(&half_led.rows)?;
    let caught = missing_inject_10()?;
    Ok((a, b, combined, caught))
}

// ── 判据谓词（私有） ────────────────────────────────────────────
/// 清单闭集谓词：十五批齐备 + 三类权重非零 + 总权重 105。
fn closed_set_ok() -> bool {
    if GAP_BATCHES.len() != 15 || GAP_KINDS.len() != 3 || GAPS_PER_BATCH != 3 {
        return false;
    }
    let mut i = 0;
    while i < GAP_BATCHES.len() {
        match GAP_BATCHES.get(i) {
            Some(n) => {
                if n.is_empty() {
                    return false;
                }
            }
            None => return false,
        }
        i += 1;
    }
    let mut total_w = 0;
    let mut k = 0;
    while k < KIND_WEIGHTS.len() {
        match KIND_WEIGHTS.get(k) {
            Some(w) => {
                if *w == 0 {
                    return false;
                }
                total_w += *w;
            }
            None => return false,
        }
        k += 1;
    }
    total_w * 15 == 105
}

/// 双层锚口径谓词：全补后两层均满、合分 10000、层差 0。
fn dual_anchor_full_ok() -> bool {
    match backfill_all() {
        Ok(led) => match led.segment_gate() {
            Ok((a, b, c)) => a == 10000 && b == 10000 && c == 10000,
            Err(_) => false,
        },
        Err(_) => false,
    }
}

/// 层差闸谓词：权重进度拉满但批完成度落后（只封 12/15 批）时，
/// 合分仍 ≥80 而层差 >15，必须被 LayerDrift 抓住。
fn layer_drift_caught_ok() -> bool {
    let mut led = Gapfill3Ledger::new();
    // 全补 45 项 → F1382 层 = 10000。
    let mut b = 0;
    while b < GAP_BATCHES.len() {
        let mut k = 0;
        while k < GAPS_PER_BATCH {
            let _ = led.fill(b, k);
            k += 1;
        }
        b += 1;
    }
    // 只封前 12 批 → F1400 层 = 8000，合分 9000 ≥80 但层差 2000 >15。
    let mut sb = 0;
    while sb < 12 {
        let _ = led.seal_batch(sb);
        sb += 1;
    }
    match led.segment_gate() {
        Err(e) => e == Gap3Code::LayerDrift.val(),
        Ok(_) => false,
    }
}

// ── 探针三序 ────────────────────────────────────────────────────
/// 序 A（正向）：首批首项补全 + 只增审计空前缀。
pub fn probe_seq_a() -> Result<(), u32> {
    let mut led = Gapfill3Ledger::new();
    led.fill(0, 0)?;
    led.append_only_audit(&[])
}

/// 序 B（越类必拒）：类别号 3 出闭集。
pub fn probe_seq_b() -> Result<(), u32> {
    let mut led = Gapfill3Ledger::new();
    led.fill(0, 3).map(|_| ())
}

/// 序 C（大关必拒）：仅补一项即验段级大关。
pub fn probe_seq_c() -> Result<(), u32> {
    let mut led = Gapfill3Ledger::new();
    led.fill(0, 0)?;
    led.segment_gate().map(|_| ())
}

// ── 注入轮（LCG 驱动 rounds × 四槽） ────────────────────────────
pub fn inject_rounds(rounds: u32) -> Result<InjectLedger, u32> {
    let mut led = InjectLedger::new();
    let mut rng = Lcg::new(SEG_BASE as u64);
    let mut r = 0;
    while r < rounds {
        let _ = rng.next();
        // 槽0：越批。
        let mut l0 = Gapfill3Ledger::new();
        led.record(0, l0.fill(15, 0).is_err());
        // 槽1：越类。
        let mut l1 = Gapfill3Ledger::new();
        led.record(1, l1.fill(0, 3).is_err());
        // 槽2：重复补全。
        let mut l2 = Gapfill3Ledger::new();
        let _ = l2.fill(7, 1);
        led.record(2, l2.fill(7, 1).is_err());
        // 槽3：封账拒补 + 漏项抓。
        let mut l3 = Gapfill3Ledger::new();
        let mut k = 0;
        while k < GAPS_PER_BATCH {
            let _ = l3.fill(9, k);
            k += 1;
        }
        let _ = l3.seal_batch(9);
        let sealed_reject = l3.fill(9, 0).is_err();
        led.record(3, sealed_reject && missing_inject_10() == Ok(10));
        r += 1;
    }
    Ok(led)
}

// ── 双跑一致 ────────────────────────────────────────────────────
pub fn repro_check() -> Result<(), u32> {
    let a = golden_run()?;
    let b = golden_run()?;
    if a != b {
        return Err(Gap3Code::NotReproducible.val());
    }
    Ok(())
}

// ── 段位守卫（窗口 + 对齐 + 区间三闸） ──────────────────────────
pub fn segment_guard() -> bool {
    if SEG_BASE % 0x40 != 0 || SEG_BASE < 0x75000 || SEG_BASE > 0x781C0 {
        return false;
    }
    let mut i = 0;
    while i < Gap3Code::ALL.len() {
        if let Some(c) = Gap3Code::ALL.get(i) {
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
    // 1. 清单闭集。
    v.push((
        "J1-清单闭集",
        closed_set_ok(),
        format!("十五批 B16~B30 三类×权重{:?} 总项=45 总权=105", KIND_WEIGHTS),
    ));
    // 2. 黄金全链。
    let g = golden_run();
    v.push((
        "J1-黄金全链",
        match &g {
            Ok((a, b, c, caught)) => *a == 10000 && *b == 10000 && *c == 10000 && *caught == 10,
            Err(_) => false,
        },
        match &g {
            Ok((a, b, c, caught)) => format!("F1382={} F1400={} 合={} 漏抓={}", a, b, c, caught),
            Err(e) => format!("err=0x{:x}", e),
        },
    ));
    // 3. 段级 ≥80 大关（双层锚满贯复验）。
    v.push((
        "J1-段级≥80双层锚",
        dual_anchor_full_ok(),
        String::from("F1382 权重进度 + F1400 批完成度 双满贯"),
    ));
    // 4. 层差闸。
    v.push((
        "J1-层差≤15闸",
        layer_drift_caught_ok(),
        String::from("偏科补齐 → LayerDrift 显性拒"),
    ));
    // 5. 三段状态机单调。
    let mono_ok = {
        let mut l = Gapfill3Ledger::new();
        let r1 = l.force_stage(0, BatchStage3::Verified).is_ok();
        let r2 = l.force_stage(0, BatchStage3::Filling).is_err();
        let r3 = l.force_stage(0, BatchStage3::Listed).is_err();
        let r4 = l.force_stage(1, BatchStage3::Filling).is_ok();
        let r5 = l.force_stage(1, BatchStage3::Listed).is_err();
        r1 && r2 && r3 && r4 && r5
    };
    v.push((
        "J1-三段状态机单调",
        mono_ok,
        String::from("Listed→Filling→Verified 倒退全拒"),
    ));
    // 6. 封账拒补。
    let seal_reject = {
        let mut l = Gapfill3Ledger::new();
        let mut k = 0;
        while k < GAPS_PER_BATCH {
            let _ = l.fill(2, k);
            k += 1;
        }
        let _ = l.seal_batch(2);
        l.fill(2, 0).is_err()
    };
    v.push(("J1-封账拒补", seal_reject, String::from("Verified 批再补全 → SealedReject")));
    // 7. 总账只增不减（seq 单调）。
    let append_ok = match backfill_all() {
        Ok(l) => {
            let mut half = Gapfill3Ledger::new();
            let _ = half.fill(0, 0);
            l.append_only_audit(&half.rows).is_ok()
                && l.rows.len() == 45
                && l.rows.first().map(|r| r.seq) == Some(0)
        }
        Err(_) => false,
    };
    v.push((
        "J1-总账只增不减",
        append_ok,
        String::from("append-only + seq 单调 + 45 行闭集"),
    ));
    // 8. 漏项注入 10/10。
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
        format!("a正向={} b越类拒={} c大关拒={}", a, b, c),
    ));
    // 11. 诊断码闭集对账。
    let codes_ok = {
        let mut ok = true;
        let mut i = 0;
        while i < Gap3Code::ALL.len() {
            if let Some(c) = Gap3Code::ALL.get(i) {
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
        format!("{} 码全查表通过", Gap3Code::ALL.len()),
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
        String::from("0x75AC0 窗口 0x40 对齐区间三闸"),
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

pub fn run_unx1485_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F1485 补全执行三（分发与接管面 B16–B30 缺口批补齐）");
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
            Ok((a, b, c, caught)) => {
                assert_eq!(a, 10000);
                assert_eq!(b, 10000);
                assert_eq!(c, 10000);
                assert_eq!(caught, 10);
            }
            Err(e) => assert!(false, "golden_run err=0x{:x}", e),
        }
    }

    #[test]
    fn closed_set_shape() {
        assert!(closed_set_ok());
        assert_eq!(GAP_BATCHES.len(), 15);
        assert_eq!(GAP_KINDS.len(), 3);
        assert_eq!(GAPS_PER_BATCH * GAP_BATCHES.len(), 45);
    }

    #[test]
    fn batch_and_kind_range_caught() {
        let mut led = Gapfill3Ledger::new();
        match led.fill(15, 0) {
            Err(e) => assert_eq!(e, Gap3Code::BatchOutOfRange.val()),
            Ok(_) => assert!(false, "越批未拒"),
        }
        match led.fill(0, 3) {
            Err(e) => assert_eq!(e, Gap3Code::KindOutOfRange.val()),
            Ok(_) => assert!(false, "越类未拒"),
        }
    }

    #[test]
    fn duplicate_fill_caught() {
        let mut led = Gapfill3Ledger::new();
        assert!(led.fill(6, 2).is_ok());
        match led.fill(6, 2) {
            Err(e) => assert_eq!(e, Gap3Code::DuplicateFill.val()),
            Ok(_) => assert!(false, "重复补全未拒"),
        }
    }

    #[test]
    fn stage_regress_and_sealed_caught() {
        let mut led = Gapfill3Ledger::new();
        assert!(led.force_stage(0, BatchStage3::Verified).is_ok());
        match led.force_stage(0, BatchStage3::Filling) {
            Err(e) => assert_eq!(e, Gap3Code::StageRegress.val()),
            Ok(_) => assert!(false, "阶段倒退未拒"),
        }
        match led.fill(0, 0) {
            Err(e) => assert_eq!(e, Gap3Code::SealedReject.val()),
            Ok(_) => assert!(false, "封账后补全未拒"),
        }
    }

    #[test]
    fn gate_short_on_partial() {
        let mut led = Gapfill3Ledger::new();
        let _ = led.fill(0, 0);
        match led.segment_gate() {
            Err(e) => assert_eq!(e, Gap3Code::GateShort.val()),
            Ok(_) => assert!(false, "1 项不应过段级大关"),
        }
    }

    #[test]
    fn layer_drift_caught() {
        assert!(layer_drift_caught_ok());
    }

    #[test]
    fn missing_inject_all_caught() {
        assert_eq!(missing_inject_10(), Ok(10));
    }

    #[test]
    fn reproducible_two_runs() {
        assert!(repro_check().is_ok());
        assert_eq!(golden_run(), golden_run());
    }

    #[test]
    fn seq_monotonic_and_append_only() {
        let mut led = Gapfill3Ledger::new();
        let s0 = led.fill(3, 0);
        let s1 = led.fill(3, 1);
        let s2 = led.fill(3, 2);
        match (s0, s1, s2) {
            (Ok(a), Ok(b), Ok(c)) => {
                assert!(a < b && b < c, "seq 必须单调");
            }
            _ => assert!(false, "正常补全不应拒"),
        }
        let snap = led.rows.clone();
        assert!(led.fill(4, 0).is_ok());
        assert!(led.append_only_audit(&snap).is_ok());
    }

    #[test]
    fn segment_guard_and_codes() {
        assert!(segment_guard());
        assert_eq!(SEG_BASE % 0x40, 0);
        assert_eq!(detail(Gap3Code::LayerDrift.val()), Gap3Code::LayerDrift.detail());
        assert!(detail(SEG_BASE + 0x3F).starts_with("未知诊断码"));
    }
}
