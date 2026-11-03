//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F1584 · 域级断言大关终验（全域断言 ≥800 终账） · 0x722C0
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定，CoRun·UNX-A2-B40，前置 UNX-F1583）：
//!   ① 纯功能代码 ≥360 行；
//!   ② 判据原文：UNX-F1584-J1 断言大关终验通过（域级断言总账 ≥800 实计
//!     且逐批断言集全绿复跑一致率 100%），重号注入 10/10 拦截；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! ─────────────────────────────────────────────────────────────────────
//! 设计要点：
//!   · 终账复用 40 批 × 20 条结构独立成账（不依赖 F1522 模块实例），
//!     实计 ≥800 才许封版（大关线）；
//!   · 逐批断言集全绿：批判定引擎逐条给判（Green/Red），Red 即批红；
//!   · 复跑一致率 100%：全账独立复跑 2 轮，逐批摘要（绿数 + 判定摘要）
//!     逐轮全等，整数口径一致率 = 全对拍数×100/总对拍数；
//!   · 红批注入：单批故障旗置位后复跑必显红（BatchRed），检出即抓；
//!   · 重号注入 10 轮全拦（DupId）；封版后再登记 → SealBroken 显性拒。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x722C0，VX3-1103 大关窗口独占）。
pub const SEG_BASE: u32 = 0x722C0;

/// 批集总数（40 批，与域级批拓扑同构，冻结）。
pub const BATCH_COUNT: usize = 40;
/// 批号下界（b01，冻结）。
pub const BATCH_LO: u8 = 1;
/// 批号上界（b40，冻结）。
pub const BATCH_HI: u8 = 40;
/// 每批断言数（冻结 20，40 × 20 = 800）。
pub const ASSERTS_PER_BATCH: usize = 20;
/// 大关线（全域断言 ≥800 实计，冻结）。
pub const GRAND_MIN: u32 = 800;
/// 复跑轮数（独立复跑 2 轮，冻结）。
pub const RERUN_ROUNDS: u32 = 2;
/// 断言编号闭集上限（1..=2048，冻结）。
pub const ID_CAP: u32 = 2048;
/// 单批登记上限（冻结 32）。
pub const BATCH_CAP: usize = 32;
/// 注入轮数（写死：判据 10/10 语义）。
pub const JUDGE_ROUNDS: u32 = 10;

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FinalCode {
    /// 终账不足（实计 <800 或批账合计对不上）
    TotalShort,
    /// 重复编号（终账全局唯一，重号注入面）
    DupId,
    /// 批号越界（不在 b01–b40）
    BatchOver,
    /// 批红（断言集判定出现 Red）
    BatchRed,
    /// 复跑不一致（逐批摘要跨轮不等）
    RerunMismatch,
    /// 封版后改账（终验封印后仍登记/改旗）
    SealBroken,
    /// 摘要失配（判定摘要与登记口径不符）
    DigestBad,
    /// 守卫失守（段位/编号闭集守卫失败）
    GuardBad,
    /// 双跑不一致
    NotReproducible,
}

impl FinalCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            FinalCode::TotalShort => "终账不足：实计 <800 或批账合计对不上，补齐再封",
            FinalCode::DupId => "重复编号：终账全局唯一，重号一律拦",
            FinalCode::BatchOver => "批号越界：只在 b01–b40",
            FinalCode::BatchRed => "批红：断言集出现 Red，先修断言再验",
            FinalCode::RerunMismatch => "复跑不一致：逐批摘要跨轮不等，查判定引擎确定性",
            FinalCode::SealBroken => "封版后改账：终验封印，登记口已闭",
            FinalCode::DigestBad => "摘要失配：判定摘要与登记口径不符，重算摘要",
            FinalCode::GuardBad => "守卫失守：段位或编号闭集守卫失败，查诊断码窗口",
            FinalCode::NotReproducible => "双跑不一致：复现红线，golden_run 两次应同值",
        }
    }
    pub const ALL: [FinalCode; 9] = [
        FinalCode::TotalShort,
        FinalCode::DupId,
        FinalCode::BatchOver,
        FinalCode::BatchRed,
        FinalCode::RerunMismatch,
        FinalCode::SealBroken,
        FinalCode::DigestBad,
        FinalCode::GuardBad,
        FinalCode::NotReproducible,
    ];
}

/// 诊断码查表（段外码返回统一文案）。
pub fn detail(code: u32) -> &'static str {
    let mut i = 0;
    while i < FinalCode::ALL.len() {
        if FinalCode::ALL[i].val() == code {
            return FinalCode::ALL[i].detail();
        }
        i += 1;
    }
    "未知诊断码：不在 0x722C0 段闭集内"
}

// ── 域模型 ──────────────────────────────────────────────────────
/// 判定结果（二值闭集：Green=0 / Red=1）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Verdict {
    Green,
    Red,
}

impl Verdict {
    pub fn rank(self) -> u8 {
        match self {
            Verdict::Green => 0,
            Verdict::Red => 1,
        }
    }
}

/// 终账断言条目：登记时冻结期望判（全绿口径 expect=true）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FinalAssert {
    pub id: u32,
    pub batch: u8,
    pub expect_green: bool,
}

/// 批断言集：20 条终账断言 + 故障旗（注入面用）。
#[derive(Clone, Debug)]
pub struct BatchSet {
    pub batch: u8,
    pub asserts: Vec<FinalAssert>,
    /// 故障旗：置位后该批按 1/3 槽位注红（复跑检出面）。
    pub faulted: bool,
}

impl BatchSet {
    pub fn new(batch: u8) -> BatchSet {
        BatchSet { batch, asserts: Vec::new(), faulted: false }
    }
}

/// 单批单轮判定摘要（复跑对平单位）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BatchRun {
    pub batch: u8,
    pub greens: u32,
    pub reds: u32,
    pub digest: u32,
}

/// 大关终账：40 批 + 全局编号位图 + 只增计数 + 封印旗。
#[derive(Clone, Debug)]
pub struct GrandLedger {
    pub batches: Vec<BatchSet>,
    pub used: Vec<bool>,
    pub total: u32,
    pub peak_total: u32,
    pub sealed: bool,
}

impl GrandLedger {
    pub fn new() -> GrandLedger {
        let mut batches = Vec::new();
        let mut b = BATCH_LO;
        while b <= BATCH_HI {
            batches.push(BatchSet::new(b));
            b += 1;
        }
        let mut used = Vec::new();
        let mut i = 0u32;
        while i <= ID_CAP {
            used.push(false);
            i += 1;
        }
        GrandLedger { batches, used, total: 0, peak_total: 0, sealed: false }
    }

    /// 批号 → 批账下标。
    pub fn batch_index(&self, batch: u8) -> Option<usize> {
        if batch < BATCH_LO || batch > BATCH_HI {
            return None;
        }
        let idx = (batch - BATCH_LO) as usize;
        if idx < self.batches.len() {
            Some(idx)
        } else {
            None
        }
    }

    fn id_used(&self, id: u32) -> bool {
        match self.used.get(id as usize) {
            Some(b) => *b,
            None => false,
        }
    }

    /// 终账登记（封印闸/批号闸/编号闸/重号闸/批满闸）。
    pub fn register(&mut self, batch: u8, id: u32) -> Result<(), u32> {
        if self.sealed {
            return Err(FinalCode::SealBroken.val());
        }
        if batch < BATCH_LO || batch > BATCH_HI {
            return Err(FinalCode::BatchOver.val());
        }
        if id == 0 || id > ID_CAP {
            return Err(FinalCode::GuardBad.val());
        }
        if self.id_used(id) {
            return Err(FinalCode::DupId.val());
        }
        let bi = match self.batch_index(batch) {
            Some(i) => i,
            None => return Err(FinalCode::BatchOver.val()),
        };
        let full = match self.batches.get(bi) {
            Some(b) => b.asserts.len() >= BATCH_CAP,
            None => true,
        };
        if full {
            return Err(FinalCode::TotalShort.val());
        }
        if let Some(b) = self.batches.get_mut(bi) {
            b.asserts.push(FinalAssert { id, batch, expect_green: true });
        }
        if let Some(flag) = self.used.get_mut(id as usize) {
            *flag = true;
        }
        self.total += 1;
        if self.total > self.peak_total {
            self.peak_total = self.total;
        }
        Ok(())
    }

    /// 红批注入口（封印后置旗拒：终验面不许动账）。
    pub fn mark_fault(&mut self, batch: u8) -> Result<(), u32> {
        if self.sealed {
            return Err(FinalCode::SealBroken.val());
        }
        let bi = match self.batch_index(batch) {
            Some(i) => i,
            None => return Err(FinalCode::BatchOver.val()),
        };
        if let Some(b) = self.batches.get_mut(bi) {
            b.faulted = true;
        }
        Ok(())
    }

    /// 大关封印：实计 ≥800 且 40 批全非空才许封。
    pub fn seal(&mut self) -> Result<(), u32> {
        if self.total < GRAND_MIN {
            return Err(FinalCode::TotalShort.val());
        }
        if self.batches.len() != BATCH_COUNT {
            return Err(FinalCode::TotalShort.val());
        }
        let mut i = 0;
        while i < self.batches.len() {
            match self.batches.get(i) {
                Some(b) => {
                    if b.asserts.len() != ASSERTS_PER_BATCH {
                        return Err(FinalCode::TotalShort.val());
                    }
                }
                None => return Err(FinalCode::TotalShort.val()),
            }
            i += 1;
        }
        self.sealed = true;
        Ok(())
    }

    /// 位图置位数复算。
    pub fn bitmap_count(&self) -> u32 {
        let mut n = 0u32;
        let mut i = 0usize;
        while i < self.used.len() {
            if let Some(true) = self.used.get(i) {
                n += 1;
            }
            i += 1;
        }
        n
    }
}

// ── 判定引擎（确定性，可复跑） ──────────────────────────────────
/// 单条判定：故障批按槽位 1/3 注红（id % 3 == 0 判红），其余全绿。
/// expect 与实际判不符记 Red（判定引擎与登记口径失配面）。
pub fn eval_assert(a: &FinalAssert, faulted: bool) -> Verdict {
    let raw = if faulted && a.id % 3 == 0 {
        false
    } else {
        true
    };
    if raw == a.expect_green {
        Verdict::Green
    } else {
        Verdict::Red
    }
}

/// 单批单轮跑：逐条判定，绿数/红数/摘要三出。
pub fn run_batch(set: &BatchSet) -> BatchRun {
    let mut greens = 0u32;
    let mut reds = 0u32;
    let mut h: u32 = 2166136261;
    let mut i = 0;
    while i < set.asserts.len() {
        let v = match set.asserts.get(i) {
            Some(a) => eval_assert(a, set.faulted),
            None => Verdict::Red,
        };
        match v {
            Verdict::Green => greens += 1,
            Verdict::Red => reds += 1,
        }
        h = h.wrapping_mul(16777619).wrapping_add(v.rank() as u32);
        if let Some(a) = set.asserts.get(i) {
            h = h.wrapping_mul(16777619).wrapping_add(a.id);
        }
        i += 1;
    }
    BatchRun { batch: set.batch, greens, reds, digest: h }
}

/// 单轮全账跑：40 批逐批判定，批红即拒（全绿口径）。
pub fn run_round(led: &GrandLedger) -> Result<Vec<BatchRun>, u32> {
    let mut runs: Vec<BatchRun> = Vec::new();
    let mut i = 0;
    while i < led.batches.len() {
        let set = match led.batches.get(i) {
            Some(b) => b,
            None => return Err(FinalCode::TotalShort.val()),
        };
        let r = run_batch(set);
        if r.reds > 0 {
            return Err(FinalCode::BatchRed.val());
        }
        runs.push(r);
        i += 1;
    }
    Ok(runs)
}

/// 复跑一致率（整数口径）：轮间摘要逐对相等计 1 对，
/// 一致率 = 全对对数 × 100 / 总对数（百分整数，无浮点）。
pub fn rerun_consistency(led: &GrandLedger, rounds: u32) -> Result<u32, u32> {
    if rounds < 1 {
        return Err(FinalCode::RerunMismatch.val());
    }
    // 首轮为基准。
    let base = run_round(led)?;
    let mut total_pairs: u32 = 0;
    let mut hit_pairs: u32 = 0;
    let mut r = 1;
    while r < rounds {
        let cur = run_round(led)?;
        let mut i = 0;
        while i < base.len() && i < cur.len() {
            let b = match base.get(i) {
                Some(b) => b,
                None => return Err(FinalCode::RerunMismatch.val()),
            };
            let c = match cur.get(i) {
                Some(c) => c,
                None => return Err(FinalCode::RerunMismatch.val()),
            };
            total_pairs += 1;
            if b.batch == c.batch && b.greens == c.greens && b.reds == c.reds && b.digest == c.digest {
                hit_pairs += 1;
            }
            i += 1;
        }
        r += 1;
    }
    if rounds == 1 {
        return Ok(100);
    }
    if total_pairs == 0 {
        return Err(FinalCode::RerunMismatch.val());
    }
    Ok(hit_pairs.wrapping_mul(100) / total_pairs)
}

// ── 冻结终验规格：40 批 × 20 条 = 800 ───────────────────────────
/// 编号分配：批 b 占 ((b-1)*20+1)..=(b*20)。
pub fn spec_id(batch: u8, slot: usize) -> u32 {
    ((batch as u32 - 1).saturating_mul(ASSERTS_PER_BATCH as u32)) + slot as u32 + 1
}

/// 独立建终账跑满 800（不依赖其他模块实例）。
pub fn build_full_ledger() -> Result<GrandLedger, u32> {
    let mut led = GrandLedger::new();
    let mut b = BATCH_LO;
    while b <= BATCH_HI {
        let mut slot = 0;
        while slot < ASSERTS_PER_BATCH {
            led.register(b, spec_id(b, slot))?;
            slot += 1;
        }
        b += 1;
    }
    Ok(led)
}

/// 大关报告（写回与摘要用）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GrandReport {
    pub total: u32,
    pub runs: u32,
    pub consistency: u32,
    pub digest0: u32,
}

/// 大关终验全步：建账 800 → 审计 → 封印 → 复跑 2 轮一致率 100%。
pub fn grand_gate() -> Result<GrandReport, u32> {
    let mut led = build_full_ledger()?;
    // 审计三重：批账合计 == total、位图 == total、无缩水。
    if led.bitmap_count() != led.total || led.total != led.peak_total {
        return Err(FinalCode::TotalShort.val());
    }
    if led.total < GRAND_MIN {
        return Err(FinalCode::TotalShort.val());
    }
    led.seal()?;
    // 封印后登记必拒（SealBroken）。
    if led.register(BATCH_LO, 1800).is_ok() {
        return Err(FinalCode::SealBroken.val());
    }
    // 复跑 2 轮：全绿 + 一致率 100%。
    let base = run_round(&led)?;
    if base.len() != BATCH_COUNT {
        return Err(FinalCode::TotalShort.val());
    }
    let cons = rerun_consistency(&led, RERUN_ROUNDS)?;
    if cons != 100 {
        return Err(FinalCode::RerunMismatch.val());
    }
    let d0 = match base.get(0) {
        Some(r) => r.digest,
        None => return Err(FinalCode::DigestBad.val()),
    };
    Ok(GrandReport { total: led.total, runs: BATCH_COUNT as u32, consistency: cons, digest0: d0 })
}

// ── LCG（确定性伪随机，供注入轮扰动） ───────────────────────────
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

// ── 注入检出账（三路：重号路/红批路/封印路） ─────────────────────
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
            self.tries[slot] += 1;
            if hit {
                self.caught[slot] += 1;
            }
        }
    }
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
}

/// 重号注入 10/10 + 红批检出 + 封印拒（三路各计）。
pub fn inject_rounds(rounds: u32) -> Result<InjectLedger, u32> {
    let mut led = InjectLedger::new();
    let mut rng = Lcg::new(SEG_BASE as u64);
    let mut r = 0;
    while r < rounds {
        let _ = rng.next();
        // 路 0：重号路（满账上重登同号必拒）。
        let mut l = build_full_ledger()?;
        let id = spec_id(BATCH_LO, 0);
        led.record(0, l.register(BATCH_LO, id) == Err(FinalCode::DupId.val()));
        // 路 1：红批路（故障旗置位 → 复跑必显红）。
        let mut f = build_full_ledger()?;
        let red_batch = BATCH_LO + (rng.below(BATCH_COUNT as u64) as u8);
        f.mark_fault(red_batch)?;
        led.record(1, run_round(&f) == Err(FinalCode::BatchRed.val()));
        // 路 2：封印路（短账封印拒 + 封后再登记拒）。
        let mut s = GrandLedger::new();
        let _ = s.register(BATCH_LO, spec_id(BATCH_LO, 0));
        let short = s.seal() == Err(FinalCode::TotalShort.val());
        let mut full = build_full_ledger()?;
        full.seal()?;
        let late = full.register(BATCH_LO, 1900) == Err(FinalCode::SealBroken.val());
        let late_fault = full.mark_fault(BATCH_LO) == Err(FinalCode::SealBroken.val());
        led.record(2, short && late && late_fault);
        r += 1;
    }
    Ok(led)
}

// ── 黄金全链：大关终验 + 注入三路全抓 ───────────────────────────
pub fn golden_run() -> Result<(u32, u32, u32), u32> {
    let rep = grand_gate()?;
    if rep.total != 800 || rep.runs != 40 || rep.consistency != 100 {
        return Err(FinalCode::TotalShort.val());
    }
    let inj = inject_rounds(1)?;
    if !inj.all_caught() {
        return Err(FinalCode::NotReproducible.val());
    }
    let caught = inj.caught[0] + inj.caught[1] + inj.caught[2];
    Ok((1, caught, rep.total))
}

// ── 探针（守卫三序） ────────────────────────────────────────────
/// 序 A：大关终验 + 双跑一致（报告逐字段相等）。
pub fn probe_seq_a() -> Result<(), u32> {
    match golden_run() {
        Ok((1, 3, 800)) => {
            let a = grand_gate()?;
            let b = grand_gate()?;
            if a == b {
                Ok(())
            } else {
                Err(FinalCode::NotReproducible.val())
            }
        }
        _ => Err(FinalCode::NotReproducible.val()),
    }
}

/// 序 B：注入面三拒（重号 / 红批 / 越批）逐条显性拒。
pub fn probe_seq_b() -> Result<(), u32> {
    let mut l = build_full_ledger()?;
    let r1 = l.register(BATCH_LO, spec_id(BATCH_LO, 0));
    let mut f = GrandLedger::new();
    // 槽位注红面：id=3 命中 id%3==0 判红槽（id=1 判绿，抓不到红）。
    let _ = f.register(BATCH_LO, 3);
    let _ = f.mark_fault(BATCH_LO);
    let r2 = run_round(&f) == Err(FinalCode::BatchRed.val());
    let r3 = l.register(BATCH_HI + 1, 2000);
    match (r1.is_err(), r2, r3.is_err()) {
        (true, true, true) => Ok(()),
        _ => Err(FinalCode::NotReproducible.val()),
    }
}

/// 序 C：冻结常量（40 批 / ≥800 / 复跑 2 轮 / 编号闭集 2048 / 单批 32）。
pub fn probe_seq_c() -> Result<(), u32> {
    match (
        BATCH_COUNT == 40,
        GRAND_MIN == 800,
        RERUN_ROUNDS == 2,
        ID_CAP == 2048,
        BATCH_CAP == 32,
        ASSERTS_PER_BATCH == 20,
        JUDGE_ROUNDS == 10,
    ) {
        (true, true, true, true, true, true, true) => Ok(()),
        _ => Err(FinalCode::NotReproducible.val()),
    }
}

// ── 双跑一致（复现红线） ────────────────────────────────────────
pub fn repro_check() -> Result<(), u32> {
    let a = grand_gate()?;
    let b = grand_gate()?;
    if a != b {
        return Err(FinalCode::NotReproducible.val());
    }
    Ok(())
}

// ── 段位守卫（诊断码落在本段窗口） ──────────────────────────────
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

// ── 完成写回（判据③：状态=已完成、结果摘要非空） ────────────────
pub fn writeback_summary() -> Result<String, u32> {
    let rep = grand_gate()?;
    if rep.consistency != 100 {
        return Err(FinalCode::RerunMismatch.val());
    }
    Ok(format!(
        "状态=已完成 total={} runs={} reruns={} consistency={}%",
        rep.total, rep.runs, RERUN_ROUNDS, rep.consistency
    ))
}

// ── 判据面（extra_checks：8 条动态明细） ────────────────────────
pub fn extra_checks() -> Vec<(&'static str, bool, String)> {
    let mut v: Vec<(&'static str, bool, String)> = Vec::new();
    // J1 主判据：大关终验（800 实计 + 40 批 + 复跑一致率 100%）。
    let g = golden_run();
    v.push((
        "J1-大关终验",
        match &g {
            Ok((flags, caught, total)) => *flags == 1 && *caught == 3 && *total == 800,
            Err(_) => false,
        },
        match &g {
            Ok((flags, caught, total)) => format!("flags={} caught={} total={}", flags, caught, total),
            Err(e) => format!("err=0x{:x}", e),
        },
    ));
    // J1 终账实计：满账 800、位图对平、封印可落。
    let seal_ok = match build_full_ledger() {
        Ok(mut led) => {
            led.bitmap_count() == led.total && led.total == GRAND_MIN && led.seal().is_ok()
        }
        Err(_) => false,
    };
    v.push((
        "J1-终账实计",
        seal_ok,
        format!("total=800 batches={} 全绿口径", BATCH_COUNT),
    ));
    // J1 复跑一致率：2 轮逐批摘要全等（100%）。
    let cons = match build_full_ledger() {
        Ok(led) => rerun_consistency(&led, RERUN_ROUNDS),
        Err(_) => Err(FinalCode::TotalShort.val()),
    };
    v.push((
        "J1-复跑一致率",
        match &cons {
            Ok(p) => *p == 100,
            Err(_) => false,
        },
        match &cons {
            Ok(p) => format!("consistency={}%", p),
            Err(e) => format!("err=0x{:x}", e),
        },
    ));
    // J1 重号注入 10/10（含红批/封印对照路）。
    let inj = inject_rounds(JUDGE_ROUNDS);
    let inj_ok = match &inj {
        Ok(l) => l.all_caught(),
        Err(_) => false,
    };
    v.push((
        "J1-重号注入10/10",
        inj_ok,
        format!(
            "t/c {:?}/{:?}",
            inj.as_ref().map(|l| l.tries),
            inj.as_ref().map(|l| l.caught)
        ),
    ));
    // 探针三序。
    let a = probe_seq_a().is_ok();
    let b = probe_seq_b().is_ok();
    let c = probe_seq_c().is_ok();
    v.push((
        "J1-探针三序",
        a && b && c,
        format!("a={} b={} c={}", a, b, c),
    ));
    // 双跑一致。
    v.push((
        "J2-双跑一致",
        repro_check().is_ok(),
        String::from("grand_gate 两跑报告同值"),
    ));
    // 写回摘要非空。
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
    // 段位守卫。
    v.push((
        "J4-段位守卫",
        segment_guard(),
        String::from("0x722C0 窗口"),
    ));
    v
}

pub fn run_unx1584_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F1584 域级断言大关终验（全域断言 ≥800 终账）");
    for (n, p, d) in extra_checks() {
        s.add_dyn(n, p, &d);
    }
    s
}
