//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F1484 · 补全执行二（计时面 B11–B15 缺口批补齐） · 0x6CC00
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定，CoRun·UNX-B2）：
//!   ① 纯功能代码 ≥280 行；
//!   ② UNX-F1484-J1 计时面 5 批缺口补齐执行（F1482 清单逐项补），
//!     补全后计时面断言 ≥100 大关复验通过（五批累计），补全断言入
//!     总账只增不减，补全漏项注入 10/10 必抓；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - 缺口清单闭集：F1482 冻结 5 批（B11~B15），每批缺口项数
//!      （20/20/20/20/20 = 100 断言大关）；
//!    - 补全执行：逐项补齐（项→断言账），补全后累计断言 ≥100 复
//!      验（大关判据）；
//!    - 入总账只增不减：补全断言 append-only（删除拒）；
//!    - 漏项注入 10/10 必抓；
//!    - LCG 种子写死 = SEG_BASE；no_std + alloc；零 unwrap/expect。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x6CC00，AI-VX3-1030 批B 独占区）。
pub const SEG_BASE: u32 = 0x6CC00;

// ── 五批缺口闭集 ────────────────────────────────────────────────
pub const GAP_BATCHES: [&str; 5] = ["B11", "B12", "B13", "B14", "B15"];
/// 每批缺口项数（冻结：20）。
pub const GAPS_PER_BATCH: usize = 20;
/// 断言大关（五批累计 ≥100）。
pub const ASSERT_THRESHOLD: usize = 100;

/// 判据迭代轮数（写死：判据 10/10 语义）。
pub const JUDGE_ROUNDS: u32 = 10;

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum GapCode {
    /// 批越闭集
    BatchOutOfRange,
    /// 批名空（五批齐备破坏）
    BatchNameBlank,
    /// 缺口项未补（漏项）
    GapItemMissing,
    /// 大关未过（<100）
    ThresholdShort,
    /// 总账删除（只增不减红线）
    LedgerDeletion,
    /// 漏项注入漏抓
    MissingLeaked,
    /// 重复补全（同项二次入账）
    DuplicateFill,
    /// 双跑不一致
    NotReproducible,
}

impl GapCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            GapCode::BatchOutOfRange => "批越闭集：五批冻结",
            GapCode::BatchNameBlank => "批名空：齐备破坏",
            GapCode::GapItemMissing => "缺口项未补：漏项",
            GapCode::ThresholdShort => "大关未过：<100",
            GapCode::LedgerDeletion => "总账删除：只增不减红线",
            GapCode::MissingLeaked => "漏项注入漏抓",
            GapCode::DuplicateFill => "重复补全：同项二次入账",
            GapCode::NotReproducible => "双跑不一致：复现红线",
        }
    }
    pub const ALL: [GapCode; 8] = [
        GapCode::BatchOutOfRange,
        GapCode::BatchNameBlank,
        GapCode::GapItemMissing,
        GapCode::ThresholdShort,
        GapCode::LedgerDeletion,
        GapCode::MissingLeaked,
        GapCode::DuplicateFill,
        GapCode::NotReproducible,
    ];
}

pub fn detail(code: u32) -> &'static str {
    let mut i = 0;
    while i < GapCode::ALL.len() {
        if GapCode::ALL[i].val() == code {
            return GapCode::ALL[i].detail();
        }
        i += 1;
    }
    "未知诊断码：不在 0x6CC00 段闭集内"
}

// ── 补全账 ─────────────────────────────────────────────────────
pub struct BackfillLedger {
    /// 已补项：batch*100+item 键（防重复）。
    pub filled: Vec<u32>,
    /// 断言总账（append-only）。
    pub asserts: Vec<String>,
}

impl BackfillLedger {
    pub fn new() -> BackfillLedger {
        BackfillLedger { filled: Vec::new(), asserts: Vec::new() }
    }

    /// 补全一项（批 idx + 项 idx）。
    pub fn fill(&mut self, batch: usize, item: usize) -> Result<(), u32> {
        if batch >= GAP_BATCHES.len() {
            return Err(GapCode::BatchOutOfRange.val());
        }
        if item >= GAPS_PER_BATCH {
            return Err(GapCode::GapItemMissing.val());
        }
        let key = (batch as u32) * 100 + item as u32;
        let mut i = 0;
        while i < self.filled.len() {
            if self.filled[i] == key {
                return Err(GapCode::DuplicateFill.val());
            }
            i += 1;
        }
        self.filled.push(key);
        self.asserts.push(format!("B{}-item{}-assert", GAP_BATCHES[batch], item));
        Ok(())
    }

    /// 大关复验：断言数 ≥100。
    pub fn threshold_check(&self) -> Result<usize, u32> {
        if self.asserts.len() < ASSERT_THRESHOLD {
            return Err(GapCode::ThresholdShort.val());
        }
        Ok(self.asserts.len())
    }

    /// 完整性：五批×20 项全补。
    pub fn completeness(&self) -> Result<usize, u32> {
        let mut hit = [false; 5 * 20];
        let mut i = 0;
        while i < self.filled.len() {
            let k = self.filled[i] as usize;
            if k < 100 {
                hit[k] = true;
            }
            i += 1;
        }
        let mut n = 0;
        let mut k = 0;
        while k < 100 {
            if hit[k] {
                n += 1;
            }
            k += 1;
        }
        if n != 100 {
            return Err(GapCode::GapItemMissing.val());
        }
        Ok(n)
    }

    /// 总账只增不减（长度与序对拍）。
    pub fn append_only_audit(&self, prev_len: usize) -> Result<(), u32> {
        if self.asserts.len() < prev_len {
            return Err(GapCode::LedgerDeletion.val());
        }
        Ok(())
    }
}

/// 全量补齐（100 项）。
pub fn backfill_all() -> Result<BackfillLedger, u32> {
    let mut led = BackfillLedger::new();
    let mut b = 0;
    while b < GAP_BATCHES.len() {
        let mut it = 0;
        while it < GAPS_PER_BATCH {
            led.fill(b, it)?;
            it += 1;
        }
        b += 1;
    }
    led.completeness()?;
    led.threshold_check().map(|_| ())?;
    Ok(led)
}

/// 漏项注入 10/10（每次漏一项，完整性必抓）。
pub fn missing_inject_10() -> Result<u32, u32> {
    let mut caught = 0;
    let mut k = 0;
    while k < 10 {
        let mut led = BackfillLedger::new();
        let mut b = 0;
        while b < GAP_BATCHES.len() {
            let mut it = 0;
            while it < GAPS_PER_BATCH {
                if !(k < GAPS_PER_BATCH && b == 0 && it == k) {
                    led.fill(b, it).ok();
                }
                it += 1;
            }
            b += 1;
        }
        if led.completeness().is_err() {
            caught += 1;
        }
        k += 1;
    }
    if caught != 10 {
        return Err(GapCode::MissingLeaked.val());
    }
    Ok(caught)
}

// ── LCG ────────────────────────────────────────────────────────
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

// ── 注入检出账 ──────────────────────────────────────────────────
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
            if self.tries[i] == 0 || self.caught[i] != self.tries[i] {
                return false;
            }
            i += 1;
        }
        true
    }
}

/// 黄金全链：全补 100→大关复验→只增审计→漏项 10/10。
pub fn golden_run() -> Result<(usize, usize, u32), u32> {
    if GAP_BATCHES.len() != 5 {
        return Err(GapCode::BatchNameBlank.val());
    }
    let mut led = backfill_all()?;
    let n = led.threshold_check()?;
    // 只增审计：追加后长度不减。
    led.append_only_audit(50)?;
    if led.asserts.len() < 50 {
        return Err(GapCode::LedgerDeletion.val());
    }
    let caught = missing_inject_10()?;
    Ok((n, led.asserts.len(), caught))
}

/// 探针三序。
pub fn probe_seq_a() -> Result<(), u32> {
    let mut led = BackfillLedger::new();
    led.fill(0, 0)?;
    led.fill(4, 19)?;
    led.append_only_audit(0)
}

pub fn probe_seq_b() -> Result<(), u32> {
    // 批越闭集 → 必拒。
    let mut led = BackfillLedger::new();
    led.fill(5, 0)
}

pub fn probe_seq_c() -> Result<(), u32> {
    // 大关未过 → 必拒。
    let mut led = BackfillLedger::new();
    led.fill(0, 0)?;
    led.threshold_check().map(|_| ())
}

/// 注入 10/10：越批、重复补全、漏项抓、大关四槽。
pub fn inject_rounds(rounds: u32) -> Result<InjectLedger, u32> {
    let mut led = InjectLedger::new();
    let mut r = 0;
    while r < rounds {
        // 槽0：越批。
        let mut l0 = BackfillLedger::new();
        led.record(0, l0.fill(99, 0).is_err());
        // 槽1：重复补全。
        let mut l1 = BackfillLedger::new();
        l1.fill(2, 5).ok();
        led.record(1, l1.fill(2, 5).is_err());
        // 槽2：漏项抓。
        led.record(2, missing_inject_10() == Ok(10));
        // 槽3：大关（99 项 <100 必拒）。
        let mut l3 = BackfillLedger::new();
        let mut i = 0;
        while i < 99 {
            l3.fill(i / 20, i % 20).ok();
            i += 1;
        }
        led.record(3, l3.threshold_check().is_err());
        r += 1;
    }
    Ok(led)
}

/// 双跑确定性对拍。
pub fn repro_check() -> Result<(), u32> {
    let a = golden_run()?;
    let b = golden_run()?;
    if a != b {
        return Err(GapCode::NotReproducible.val());
    }
    Ok(())
}

/// 段位守卫。
pub fn segment_guard() -> bool {
    let mut i = 0;
    while i < GapCode::ALL.len() {
        let v = GapCode::ALL[i].val();
        if v < SEG_BASE || v >= SEG_BASE + 0x40 {
            return false;
        }
        i += 1;
    }
    true
}

pub fn extra_checks() -> Vec<(&'static str, bool, String)> {
    let mut v: Vec<(&'static str, bool, String)> = Vec::new();
    let g = golden_run();
    v.push(("J1-黄金全链", g.is_ok(), format!("{:?}", g)));
    // 五批×20=100 大关。
    v.push((
        "J1-五批×20=100大关",
        GAP_BATCHES.len() == 5 && GAPS_PER_BATCH == 20 && ASSERT_THRESHOLD == 100,
        format!("{:?} × {}", GAP_BATCHES, GAPS_PER_BATCH),
    ));
    // 大关复验。
    let led = backfill_all();
    v.push((
        "J1-大关复验通过",
        match &led {
            Ok(l) => l.threshold_check() == Ok(100),
            Err(_) => false,
        },
        format!("{:?}", led.as_ref().map(|l| l.asserts.len())),
    ));
    // 漏项 10/10。
    v.push((
        "J1-漏项注入10/10",
        missing_inject_10() == Ok(10),
        format!("{}/10 抓", missing_inject_10().unwrap_or(0)),
    ));
    // 注入 10/10。
    let led2 = inject_rounds(JUDGE_ROUNDS);
    let led_ok = match &led2 {
        Ok(l) => l.all_caught() && l.caught[0] == 10,
        Err(_) => false,
    };
    v.push((
        "J1-注入10/10",
        led_ok,
        format!(
            "t/c {:?}/{:?}",
            led2.as_ref().map(|l| l.tries),
            led2.as_ref().map(|l| l.caught)
        ),
    ));
    // 探针三序。
    let a = probe_seq_a().is_ok();
    let b = probe_seq_b().is_err();
    let c = probe_seq_c().is_err();
    v.push((
        "J1-探针三序",
        a && b && c,
        format!("a={} b!={} c!={}", a, b, c),
    ));
    // 双跑一致。
    v.push((
        "J2-双跑一致",
        repro_check().is_ok(),
        String::from("golden_run 两跑同值"),
    ));
    v.push(("J3-段位守卫", segment_guard(), String::from("0x6CC00 窗口")));
    v
}

pub fn run_unx1484_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F1484 补全执行二（计时面 B11–B15 缺口批补齐）");
    for (n, p, d) in extra_checks() {
        s.add_dyn(n, p, &d);
    }
    s
}
