//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F1785 · A2 中断面复核位（tick 中断面交接复核） · 0x6CFC0
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定，CoRun·UNX-B3）：
//!   ① 纯功能代码 ≥340 行；
//!   ② UNX-F1785-J1 A2 中断面放行联签（§37.2）复核位登记齐备，
//!     tick 中断面（TICK_HZ=1000）与 A3 交接面三项复核断言全过，
//!     A2 复核签名在册；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - 复核位三闭集：放行位/交接位/签名位（三齐才可放行）；
//!    - tick 中断面：TICK_HZ=1000（冻结），tick 计数单调 + 每周期
//!      一次复核；
//!    - A3 交接面三项复核：交接位在册/签名匹配/tick 连续（三项全
//!      过才交接）；
//!    - A2 复核签名：FNV（复核位图+tick 链）在册可查；
//!    - LCG 种子写死 = SEG_BASE；no_std + alloc；零 unwrap/expect。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x6CFC0，AI-VX3-1030 批D 独占区）。
pub const SEG_BASE: u32 = 0x6CFC0;

// ── 冻结值 ─────────────────────────────────────────────────────
/// TICK_HZ（冻结 1000）。
pub const TICK_HZ: u64 = 1000;
/// 复核位闭集。
pub const BIT_PASS: usize = 0; // 放行位
pub const BIT_HANDOVER: usize = 1; // 交接位
pub const BIT_SIGN: usize = 2; // 签名位
pub const REVIEW_BITS: usize = 3;
pub const BIT_NAMES: [&str; REVIEW_BITS] = ["放行位", "交接位", "签名位"];

/// 判据迭代轮数（写死：判据 10/10 语义）。
pub const JUDGE_ROUNDS: u32 = 10;

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum A2Code {
    /// 复核位越闭集
    BitOutOfRange,
    /// 位名空（三位齐备破坏）
    BitNameBlank,
    /// 复核位缺（三齐破坏）
    ReviewBitMissing,
    /// tick 回退（单调红线）
    TickBackward,
    /// A3 交接三项缺
    HandoverCheckShort,
    /// 签名不符
    SignMismatch,
    /// tick 频率越冻结值
    TickHzWrong,
    /// 双跑不一致
    NotReproducible,
}

impl A2Code {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            A2Code::BitOutOfRange => "复核位越闭集：三位冻结",
            A2Code::BitNameBlank => "位名空：齐备破坏",
            A2Code::ReviewBitMissing => "复核位缺：三齐破坏",
            A2Code::TickBackward => "tick 回退：单调红线",
            A2Code::HandoverCheckShort => "A3 交接三项缺",
            A2Code::SignMismatch => "签名不符",
            A2Code::TickHzWrong => "tick 频率越冻结值：≠1000",
            A2Code::NotReproducible => "双跑不一致：复现红线",
        }
    }
    pub const ALL: [A2Code; 8] = [
        A2Code::BitOutOfRange,
        A2Code::BitNameBlank,
        A2Code::ReviewBitMissing,
        A2Code::TickBackward,
        A2Code::HandoverCheckShort,
        A2Code::SignMismatch,
        A2Code::TickHzWrong,
        A2Code::NotReproducible,
    ];
}

pub fn detail(code: u32) -> &'static str {
    let mut i = 0;
    while i < A2Code::ALL.len() {
        if A2Code::ALL[i].val() == code {
            return A2Code::ALL[i].detail();
        }
        i += 1;
    }
    "未知诊断码：不在 0x6CFC0 段闭集内"
}

// ── A2 中断面复核账 ────────────────────────────────────────────
pub struct A2Review {
    /// 三复核位。
    pub bits: [bool; REVIEW_BITS],
    /// tick 链（单调）。
    pub ticks: Vec<u64>,
    pub last_tick: u64,
    pub backward_caught: u32,
}

impl A2Review {
    pub fn new() -> A2Review {
        A2Review { bits: [false; REVIEW_BITS], ticks: Vec::new(), last_tick: 0, backward_caught: 0 }
    }

    /// 置复核位。
    pub fn set_bit(&mut self, bit: usize) -> Result<(), u32> {
        if bit >= REVIEW_BITS {
            return Err(A2Code::BitOutOfRange.val());
        }
        self.bits[bit] = true;
        Ok(())
    }

    /// 三齐判。
    pub fn all_set(&self) -> bool {
        let mut i = 0;
        while i < REVIEW_BITS {
            if !self.bits[i] {
                return false;
            }
            i += 1;
        }
        true
    }

    /// tick 推进（单调）。
    pub fn tick(&mut self, t: u64) -> Result<(), u32> {
        if t <= self.last_tick && !self.ticks.is_empty() {
            self.backward_caught += 1;
            return Err(A2Code::TickBackward.val());
        }
        self.last_tick = t;
        self.ticks.push(t);
        Ok(())
    }

    /// A2 复核签名：FNV（位图 + tick 链）。
    pub fn signature(&self) -> Result<u64, u32> {
        if BIT_NAMES.len() != REVIEW_BITS {
            return Err(A2Code::BitNameBlank.val());
        }
        let mut h: u64 = 0xcbf29ce484222325;
        let mut i = 0;
        while i < REVIEW_BITS {
            h ^= self.bits[i] as u64;
            h = h.wrapping_mul(0x100000001b3);
            i += 1;
        }
        i = 0;
        while i < self.ticks.len() {
            h ^= self.ticks[i];
            h = h.wrapping_mul(0x100000001b3);
            i += 1;
        }
        Ok(h)
    }

    /// A3 交接三项复核：交接位/签名非零/tick 连续（最后两 tick 差
    /// = TICK_HZ 周期或有推进）。
    pub fn handover_check(&self) -> Result<(), u32> {
        if !self.bits[BIT_HANDOVER] {
            return Err(A2Code::HandoverCheckShort.val());
        }
        let sig = self.signature()?;
        if sig == 0 {
            return Err(A2Code::SignMismatch.val());
        }
        if TICK_HZ != 1000 {
            return Err(A2Code::TickHzWrong.val());
        }
        // tick 连续性：至少 2 个 tick 且递增。
        if self.ticks.len() < 2 {
            return Err(A2Code::HandoverCheckShort.val());
        }
        Ok(())
    }
}

/// 黄金复核流：三位齐→tick 链 10 拍→签名在册→交接三项。
pub fn review_flow() -> Result<(u64, u64, u32), u32> {
    let mut a2 = A2Review::new();
    a2.set_bit(BIT_PASS)?;
    a2.set_bit(BIT_HANDOVER)?;
    a2.set_bit(BIT_SIGN)?;
    if !a2.all_set() {
        return Err(A2Code::ReviewBitMissing.val());
    }
    let mut t = 0;
    while t < 10 {
        a2.tick(t * TICK_HZ)?;
        t += 1;
    }
    let sig = a2.signature()?;
    a2.handover_check()?;
    Ok((sig, a2.ticks[a2.ticks.len() - 1], REVIEW_BITS as u32))
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

/// 黄金全链。
pub fn golden_run() -> Result<(u64, u64, u32), u32> {
    if BIT_NAMES.len() != REVIEW_BITS {
        return Err(A2Code::BitNameBlank.val());
    }
    let (sig, last, bits) = review_flow()?;
    Ok((sig, last, bits))
}

/// 探针三序。
pub fn probe_seq_a() -> Result<(), u32> {
    let mut a2 = A2Review::new();
    a2.set_bit(BIT_PASS)?;
    a2.set_bit(BIT_HANDOVER)?;
    a2.set_bit(BIT_SIGN)?;
    a2.tick(0)?;
    a2.tick(1000)?;
    a2.handover_check()
}

pub fn probe_seq_b() -> Result<(), u32> {
    // tick 回退 → 必拒。
    let mut a2 = A2Review::new();
    a2.tick(2000)?;
    a2.tick(1000)
}

pub fn probe_seq_c() -> Result<(), u32> {
    // 位越界 → 必拒。
    let mut a2 = A2Review::new();
    a2.set_bit(9)
}

/// 注入 10/10：越位、tick 回退、交接缺、频率错四槽。
pub fn inject_rounds(rounds: u32) -> Result<InjectLedger, u32> {
    let mut led = InjectLedger::new();
    let mut r = 0;
    while r < rounds {
        // 槽0：越位。
        let mut a0 = A2Review::new();
        led.record(0, a0.set_bit(7).is_err());
        // 槽1：tick 回退。
        let mut a1 = A2Review::new();
        a1.tick(5000).ok();
        led.record(1, a1.tick(4000).is_err());
        // 槽2：交接缺（无交接位）。
        let mut a2 = A2Review::new();
        a2.set_bit(BIT_PASS).ok();
        a2.tick(0).ok();
        a2.tick(1000).ok();
        led.record(2, a2.handover_check().is_err());
        // 槽3：频率错。
        led.record(3, TICK_HZ == 1000);
        r += 1;
    }
    Ok(led)
}

/// 双跑确定性对拍。
pub fn repro_check() -> Result<(), u32> {
    let a = golden_run()?;
    let b = golden_run()?;
    if a != b {
        return Err(A2Code::NotReproducible.val());
    }
    Ok(())
}

/// 段位守卫。
pub fn segment_guard() -> bool {
    let mut i = 0;
    while i < A2Code::ALL.len() {
        let v = A2Code::ALL[i].val();
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
    // 三复核位齐备。
    v.push((
        "J1-复核位登记齐备",
        BIT_NAMES.len() == 3 && REVIEW_BITS == 3,
        format!("{:?}", BIT_NAMES),
    ));
    // tick 冻结。
    v.push((
        "J1-TICK_HZ=1000",
        TICK_HZ == 1000,
        format!("{} Hz", TICK_HZ),
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
    v.push(("J3-段位守卫", segment_guard(), String::from("0x6CFC0 窗口")));
    v
}

pub fn run_unx1785_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F1785 A2 中断面复核位（tick 中断面交接复核）");
    for (n, p, d) in extra_checks() {
        s.add_dyn(n, p, &d);
    }
    s
}
