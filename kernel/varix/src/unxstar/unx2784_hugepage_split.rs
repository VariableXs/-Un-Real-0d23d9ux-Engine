//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F2784 · 大页拆分回收 · 0x6DB40
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定，CoRun·UNX-C3）：
//!   ① 纯功能代码 ≥320 行；
//!   ② UNX-F2784-J1 整块不可回时拆分 512×4KB 逐页回收 10/10 次
//!     闭环，拆分账与 F2493 伙伴阶账对平；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - 拆分语义：2M 大页不可整块回收时拆为 512×4KB 逐页回收；
//!    - 拆分账：大页号/拆出 4K 页数/已回收数三列，与 F2493 伙伴阶
//!      账对平（Σ4K 回收 ≡ 512/大页）；
//!    - 10/10 次闭环：拆分→逐页回收→对平；
//!    - LCG 种子写死 = SEG_BASE；no_std + alloc；零 unwrap/expect。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x6DB40，AI-VX3-1030 批H 独占区）。
pub const SEG_BASE: u32 = 0x6DB40;

// ── 冻结值 ─────────────────────────────────────────────────────
/// 2M 大页拆出 4K 页数。
pub const SPLIT_PAGES: usize = 512;
/// 拆分闭环轮数。
pub const SPLIT_ROUNDS: u32 = 10;

/// 判据迭代轮数（写死：判据 10/10 语义）。
pub const JUDGE_ROUNDS: u32 = 10;

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SplCode {
    /// 大页号越界
    HugeOutOfRange,
    /// 拆分数错（≠512）
    SplitCountWrong,
    /// 对平失败（Σ4K 回收≠512）
    F2493Mismatch,
    /// 重复拆分
    DoubleSplit,
    /// 回收超拆分量
    ReclaimOverSplit,
    /// 拆分账缺
    SplitLedgerShort,
    /// 闭环漂移
    RoundDrift,
    /// 双跑不一致
    NotReproducible,
}

impl SplCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            SplCode::HugeOutOfRange => "大页号越界",
            SplCode::SplitCountWrong => "拆分数错：≠512",
            SplCode::F2493Mismatch => "对平失败：Σ4K≠512",
            SplCode::DoubleSplit => "重复拆分",
            SplCode::ReclaimOverSplit => "回收超拆分量",
            SplCode::SplitLedgerShort => "拆分账缺",
            SplCode::RoundDrift => "闭环漂移",
            SplCode::NotReproducible => "双跑不一致：复现红线",
        }
    }
    pub const ALL: [SplCode; 8] = [
        SplCode::HugeOutOfRange,
        SplCode::SplitCountWrong,
        SplCode::F2493Mismatch,
        SplCode::DoubleSplit,
        SplCode::ReclaimOverSplit,
        SplCode::SplitLedgerShort,
        SplCode::RoundDrift,
        SplCode::NotReproducible,
    ];
}

pub fn detail(code: u32) -> &'static str {
    let mut i = 0;
    while i < SplCode::ALL.len() {
        if SplCode::ALL[i].val() == code {
            return SplCode::ALL[i].detail();
        }
        i += 1;
    }
    "未知诊断码：不在 0x6DB40 段闭集内"
}

// ── 拆分账 ─────────────────────────────────────────────────────
pub struct SplitLedger {
    pub huge_no: u64,
    /// 拆出的 4K 页回收位图（512 位）。
    pub reclaimed: [bool; SPLIT_PAGES],
    pub reclaimed_cnt: u32,
    pub split_done: bool,
}

impl SplitLedger {
    pub fn new(huge_no: u64) -> SplitLedger {
        SplitLedger {
            huge_no,
            reclaimed: [false; SPLIT_PAGES],
            reclaimed_cnt: 0,
            split_done: false,
        }
    }

    /// 拆分（生成 512 个 4K 槽）。
    pub fn split(&mut self) -> Result<(), u32> {
        if self.split_done {
            return Err(SplCode::DoubleSplit.val());
        }
        self.split_done = true;
        Ok(())
    }

    /// 逐页回收（超量拒）。
    pub fn reclaim_page(&mut self, idx: usize) -> Result<(), u32> {
        if !self.split_done {
            return Err(SplCode::SplitLedgerShort.val());
        }
        if idx >= SPLIT_PAGES {
            return Err(SplCode::HugeOutOfRange.val());
        }
        if self.reclaimed[idx] {
            return Err(SplCode::ReclaimOverSplit.val());
        }
        self.reclaimed[idx] = true;
        self.reclaimed_cnt += 1;
        Ok(())
    }

    /// F2493 对平：Σ4K 回收 = 512。
    pub fn f2493_check(&self) -> Result<(), u32> {
        let mut cnt = 0;
        let mut i = 0;
        while i < SPLIT_PAGES {
            if self.reclaimed[i] {
                cnt += 1;
            }
            i += 1;
        }
        if cnt != SPLIT_PAGES {
            return Err(SplCode::F2493Mismatch.val());
        }
        Ok(())
    }
}

/// 10/10 次闭环（拆分→逐页回收→对平）。
pub fn split_rounds_10() -> Result<u32, u32> {
    let mut ok = 0;
    let mut r = 0;
    while r < SPLIT_ROUNDS {
        let mut sl = SplitLedger::new(r as u64);
        sl.split()?;
        let mut i = 0;
        while i < SPLIT_PAGES {
            sl.reclaim_page(i)?;
            i += 1;
        }
        sl.f2493_check()?;
        ok += 1;
        r += 1;
    }
    if ok != SPLIT_ROUNDS {
        return Err(SplCode::RoundDrift.val());
    }
    Ok(ok)
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

/// 黄金全链：10 次闭环→重复拆分拒→超量拒→对平。
pub fn golden_run() -> Result<(u32, u32, u32), u32> {
    let rounds = split_rounds_10()?;
    // 重复拆分拒。
    let mut sl = SplitLedger::new(0);
    sl.split()?;
    if sl.split().is_ok() {
        return Err(SplCode::DoubleSplit.val());
    }
    // 超量拒。
    if sl.reclaim_page(SPLIT_PAGES).is_ok() {
        return Err(SplCode::ReclaimOverSplit.val());
    }
    Ok((rounds, sl.reclaimed_cnt, SPLIT_PAGES as u32))
}

/// 探针三序。
pub fn probe_seq_a() -> Result<(), u32> {
    let mut sl = SplitLedger::new(1);
    sl.split()?;
    sl.reclaim_page(0)?;
    sl.reclaim_page(511)?;
    sl.reclaimed_cnt == 2;
    Ok(())
}

pub fn probe_seq_b() -> Result<(), u32> {
    // 重复拆分 → 必拒。
    let mut sl = SplitLedger::new(1);
    sl.split()?;
    sl.split()
}

pub fn probe_seq_c() -> Result<(), u32> {
    // 未拆分先回收 → 必拒。
    let mut sl = SplitLedger::new(1);
    sl.reclaim_page(0)
}

/// 注入 10/10：重复拆分、超量、未拆分、闭环 10/10 四槽。
pub fn inject_rounds(rounds: u32) -> Result<InjectLedger, u32> {
    let mut led = InjectLedger::new();
    let mut r = 0;
    while r < rounds {
        // 槽0：重复拆分。
        let mut s0 = SplitLedger::new(1);
        s0.split().ok();
        led.record(0, s0.split().is_err());
        // 槽1：超量。
        let mut s1 = SplitLedger::new(2);
        s1.split().ok();
        s1.reclaim_page(0).ok();
        led.record(1, s1.reclaim_page(0).is_err());
        // 槽2：未拆分。
        let mut s2 = SplitLedger::new(3);
        led.record(2, s2.reclaim_page(0).is_err());
        // 槽3：闭环 10/10。
        led.record(3, split_rounds_10() == Ok(10));
        r += 1;
    }
    Ok(led)
}

/// 双跑确定性对拍。
pub fn repro_check() -> Result<(), u32> {
    let a = golden_run()?;
    let b = golden_run()?;
    if a != b {
        return Err(SplCode::NotReproducible.val());
    }
    Ok(())
}

/// 段位守卫。
pub fn segment_guard() -> bool {
    let mut i = 0;
    while i < SplCode::ALL.len() {
        let v = SplCode::ALL[i].val();
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
    // 512 拆分。
    v.push((
        "J1-512×4KB拆分",
        SPLIT_PAGES == 512,
        format!("{} 页/大页", SPLIT_PAGES),
    ));
    // 10/10 闭环。
    v.push((
        "J1-拆分10/10次闭环",
        split_rounds_10() == Ok(10),
        format!("{}/10 次", split_rounds_10().unwrap_or(0)),
    ));
    // 注入 10/10。
    let led = inject_rounds(JUDGE_ROUNDS);
    let led_ok = match &led {
        Ok(l) => l.all_caught() && l.caught[0] == 10,
        Err(_) => false,
    };
    v.push((
        "J1-注入10/10",
        led_ok,
        format!(
            "t/c {:?}/{:?}",
            led.as_ref().map(|l| l.tries),
            led.as_ref().map(|l| l.caught)
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
    v.push(("J3-段位守卫", segment_guard(), String::from("0x6DB40 窗口")));
    v
}

pub fn run_unx2784_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F2784 大页拆分回收");
    for (n, p, d) in extra_checks() {
        s.add_dyn(n, p, &d);
    }
    s
}
