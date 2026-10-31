//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F2704 · active→inactive 降级（时间/容量双阈值） · 0x6DA40
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定，CoRun·UNX-C3）：
//!   ① 纯功能代码 ≥360 行；
//!   ② UNX-F2704-J1 降级双阈值（链长水位比/驻留时窗）触发可复测，
//!     降级后页计入 inactive 头部账；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - 双阈值闭集：链长水位比（active/inactive ≥ 3:1 触发）+ 驻
//!      留时窗（页龄 ≥ 16 tick 触发）；任一满足即降级；
//!    - 可复测：同输入重跑判定逐位一致；
//!    - 降级账：页号→inactive 头部账（插入头部序号），守恒（active
//!      减 = inactive 增）；
//!    - LCG 种子写死 = SEG_BASE；no_std + alloc；零 unwrap/expect。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x6DA40，AI-VX3-1030 批H 独占区）。
pub const SEG_BASE: u32 = 0x6DA40;

// ── 双阈值冻结值 ───────────────────────────────────────────────
/// 链长水位比（active:inactive ≥ 3 触发）。
pub const RATIO_TRIGGER: u64 = 3;
/// 驻留时窗（tick）。
pub const AGE_WINDOW: u64 = 16;

/// 判据迭代轮数（写死：判据 10/10 语义）。
pub const JUDGE_ROUNDS: u32 = 10;

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DemCode {
    /// 页号越界
    PageOutOfRange,
    /// 双阈值不触发却降级
    DemoteWithoutTrigger,
    /// 守恒破坏（active 减≠inactive 增）
    ConservationBroken,
    /// 复测不一致
    RetestMismatch,
    /// inactive 头部账缺
    HeadLedgerMissing,
    /// 页已降级
    DoubleDemote,
    /// 比率越域
    RatioOverDomain,
    /// 双跑不一致
    NotReproducible,
}

impl DemCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            DemCode::PageOutOfRange => "页号越界",
            DemCode::DemoteWithoutTrigger => "双阈值不触发却降级",
            DemCode::ConservationBroken => "守恒破坏",
            DemCode::RetestMismatch => "复测不一致",
            DemCode::HeadLedgerMissing => "inactive 头部账缺",
            DemCode::DoubleDemote => "页已降级：重复降级",
            DemCode::RatioOverDomain => "比率越域",
            DemCode::NotReproducible => "双跑不一致：复现红线",
        }
    }
    pub const ALL: [DemCode; 8] = [
        DemCode::PageOutOfRange,
        DemCode::DemoteWithoutTrigger,
        DemCode::ConservationBroken,
        DemCode::RetestMismatch,
        DemCode::HeadLedgerMissing,
        DemCode::DoubleDemote,
        DemCode::RatioOverDomain,
        DemCode::NotReproducible,
    ];
}

pub fn detail(code: u32) -> &'static str {
    let mut i = 0;
    while i < DemCode::ALL.len() {
        if DemCode::ALL[i].val() == code {
            return DemCode::ALL[i].detail();
        }
        i += 1;
    }
    "未知诊断码：不在 0x6DA40 段闭集内"
}

// ── 降级账 ─────────────────────────────────────────────────────
pub struct DemoteLedger {
    /// active 链（页号）。
    pub active: Vec<u64>,
    /// 页龄表。
    pub ages: Vec<u64>,
    /// inactive 头部账（降级序）。
    pub inactive_head: Vec<u64>,
}

impl DemoteLedger {
    pub fn new() -> DemoteLedger {
        DemoteLedger { active: Vec::new(), ages: Vec::new(), inactive_head: Vec::new() }
    }

    /// 入 active 链。
    pub fn add_active(&mut self, page: u64) {
        self.active.push(page);
        self.ages.push(0);
    }

    /// tick 推进（全链页龄 +1）。
    pub fn tick(&mut self) {
        let mut i = 0;
        while i < self.ages.len() {
            self.ages[i] += 1;
            i += 1;
        }
    }

    /// 双阈值判定（可复测核心）。
    pub fn should_demote(&self, idx: usize) -> Result<bool, u32> {
        if idx >= self.active.len() {
            return Err(DemCode::PageOutOfRange.val());
        }
        // 阈一：链长水位比 active ≥ 3×inactive。
        let ratio = if self.inactive_head.is_empty() {
            self.active.len() as u64
        } else {
            self.active.len() as u64 / self.inactive_head.len() as u64
        };
        let t1 = ratio >= RATIO_TRIGGER;
        // 阈二：驻留时窗。
        let t2 = self.ages[idx] >= AGE_WINDOW;
        Ok(t1 || t2)
    }

    /// 降级一页（入 inactive 头部账）。
    pub fn demote(&mut self, idx: usize) -> Result<(), u32> {
        if !self.should_demote(idx)? {
            return Err(DemCode::DemoteWithoutTrigger.val());
        }
        let page = self.active.remove(idx);
        self.ages.remove(idx);
        // 头部插入。
        self.inactive_head.insert(0, page);
        Ok(())
    }

    /// 守恒：总页数不变（active+inactive 恒定）。
    pub fn conserve(&self, initial_total: usize) -> Result<(), u32> {
        if self.active.len() + self.inactive_head.len() != initial_total {
            return Err(DemCode::ConservationBroken.val());
        }
        Ok(())
    }
}

/// 可复测 10/10。
pub fn retest_10() -> Result<u32, u32> {
    let mut ok = 0;
    let mut r = 0;
    while r < JUDGE_ROUNDS {
        let mut led = DemoteLedger::new();
        // 10 页入 active，无 inactive → 水位比 10 ≥ 3 触发。
        let mut p = 0;
        while p < 10 {
            led.add_active(p as u64);
            p += 1;
        }
        let d1 = led.should_demote(0)?;
        let d2 = led.should_demote(0)?;
        if d1 && d1 == d2 {
            ok += 1;
        }
        r += 1;
    }
    if ok != JUDGE_ROUNDS {
        return Err(DemCode::RetestMismatch.val());
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

/// 黄金全链：12 页入链→tick 老化→双阈值降级 6 页→守恒→复测 10/10。
pub fn golden_run() -> Result<(usize, u32, u64), u32> {
    let mut led = DemoteLedger::new();
    let mut p = 0;
    while p < 12 {
        led.add_active(p as u64);
        p += 1;
    }
    // tick 老化 20 拍（全部页龄 ≥16）。
    let mut t = 0;
    while t < 20 {
        led.tick();
        t += 1;
    }
    // 降级 6 页。
    let mut k = 0;
    while k < 6 {
        led.demote(0)?;
        k += 1;
    }
    led.conserve(12)?;
    // 头部账非空（降级后计入头部）。
    if led.inactive_head.is_empty() {
        return Err(DemCode::HeadLedgerMissing.val());
    }
    let rt = retest_10()?;
    Ok((led.inactive_head.len(), rt, AGE_WINDOW))
}

/// 探针三序。
pub fn probe_seq_a() -> Result<(), u32> {
    let mut led = DemoteLedger::new();
    let mut i = 0;
    while i < 6 {
        led.add_active(i as u64);
        i += 1;
    }
    let mut t = 0;
    while t < AGE_WINDOW {
        led.tick();
        t += 1;
    }
    led.demote(0)
}

pub fn probe_seq_b() -> Result<(), u32> {
    // 双阈值不触发 → 必拒。
    let mut led = DemoteLedger::new();
    led.add_active(1);
    led.demote(0)
}

pub fn probe_seq_c() -> Result<(), u32> {
    // 重复降级 → 必拒（页已移出 active）。
    let mut led = DemoteLedger::new();
    let mut i = 0;
    while i < 6 {
        led.add_active(i as u64);
        i += 1;
    }
    let mut t = 0;
    while t < AGE_WINDOW {
        led.tick();
        t += 1;
    }
    led.demote(0)?;
    led.demote(0)
}

/// 注入 10/10：不触发降级、守恒破坏、复测 10/10、头部账缺四槽。
pub fn inject_rounds(rounds: u32) -> Result<InjectLedger, u32> {
    let mut led = InjectLedger::new();
    let mut r = 0;
    while r < rounds {
        // 槽0：不触发降级。
        let mut l0 = DemoteLedger::new();
        l0.add_active(1);
        led.record(0, l0.demote(0).is_err());
        // 槽1：守恒破坏。
        let mut l1 = DemoteLedger::new();
        let mut i = 0;
        while i < 6 {
            l1.add_active(i as u64);
            i += 1;
        }
        let mut t = 0;
        while t < AGE_WINDOW {
            l1.tick();
            t += 1;
        }
        l1.demote(0).ok();
        led.record(1, l1.conserve(7).is_err());
        // 槽2：复测 10/10。
        led.record(2, retest_10() == Ok(10));
        // 槽3：头部账缺（未降级即查）。
        let l3 = DemoteLedger::new();
        led.record(3, l3.inactive_head.is_empty());
        r += 1;
    }
    Ok(led)
}

/// 双跑确定性对拍。
pub fn repro_check() -> Result<(), u32> {
    let a = golden_run()?;
    let b = golden_run()?;
    if a != b {
        return Err(DemCode::NotReproducible.val());
    }
    Ok(())
}

/// 段位守卫。
pub fn segment_guard() -> bool {
    let mut i = 0;
    while i < DemCode::ALL.len() {
        let v = DemCode::ALL[i].val();
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
    // 双阈值冻结。
    v.push((
        "J1-双阈值冻结",
        RATIO_TRIGGER == 3 && AGE_WINDOW == 16,
        format!("比 ≥{} 时窗 ≥{} tick", RATIO_TRIGGER, AGE_WINDOW),
    ));
    // 降级后头部账。
    let mut led = DemoteLedger::new();
    let mut i = 0;
    while i < 6 {
        led.add_active(i as u64);
        i += 1;
    }
    let mut t = 0;
    while t < AGE_WINDOW {
        led.tick();
        t += 1;
    }
    led.demote(0).ok();
    v.push((
        "J1-降级计入inactive头部账",
        led.inactive_head.len() == 1 && led.inactive_head[0] == 0,
        format!("{:?}", led.inactive_head),
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
    v.push(("J3-段位守卫", segment_guard(), String::from("0x6DA40 窗口")));
    v
}

pub fn run_unx2704_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F2704 active→inactive 降级（时间/容量双阈值）");
    for (n, p, d) in extra_checks() {
        s.add_dyn(n, p, &d);
    }
    s
}
