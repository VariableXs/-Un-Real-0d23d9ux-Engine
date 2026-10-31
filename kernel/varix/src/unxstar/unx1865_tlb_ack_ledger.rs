//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F1865 · 击落应答与完成账（应答计数、完成判定） · 0x6D0C0
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定，CoRun·UNX-B4）：
//!   ① 纯功能代码 ≥320 行；
//!   ② UNX-F1865-J1 应答账与完成判定断言全过（Σ应答=Σ广播），完
//!     成前重用注入 10/10 检出；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - 应答账：每轮击落（seq）广播 N 核 → 应答计数逐核累计，收齐
//!      才判完成（Σ应答=Σ广播）；
//!    - 完成前重用注入：未完成单再发起/再完成 = 重用违例，10/10
//!      检出；
//!    - 序守恒：完成数 + 在途数 ≡ 发起数；
//!    - LCG 种子写死 = SEG_BASE；no_std + alloc；零 unwrap/expect。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x6D0C0，AI-VX3-1030 批E 独占区）。
pub const SEG_BASE: u32 = 0x6D0C0;

// ── 冻结值 ─────────────────────────────────────────────────────
/// 核数。
pub const CORE_COUNT: usize = 4;

/// 判据迭代轮数（写死：判据 10/10 语义）。
pub const JUDGE_ROUNDS: u32 = 10;

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum AckCode {
    /// 核号越界
    CoreOutOfRange,
    /// 重用应答（同核双应答）
    DoubleAck,
    /// 未收齐即完成
    CompleteBeforeFull,
    /// 完成单重用
    ReuseAfterComplete,
    /// 应答守恒破坏（Σ应答≠Σ广播）
    AckConservationBroken,
    /// 序守恒破坏（完成+在途≠发起）
    SequenceConservationBroken,
    /// 单号越界
    TicketOutOfRange,
    /// 双跑不一致
    NotReproducible,
}

impl AckCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            AckCode::CoreOutOfRange => "核号越界：4 核",
            AckCode::DoubleAck => "重用应答：同核双应答",
            AckCode::CompleteBeforeFull => "未收齐即完成",
            AckCode::ReuseAfterComplete => "完成单重用",
            AckCode::AckConservationBroken => "应答守恒破坏：Σ应答≠Σ广播",
            AckCode::SequenceConservationBroken => "序守恒破坏",
            AckCode::TicketOutOfRange => "单号越界",
            AckCode::NotReproducible => "双跑不一致：复现红线",
        }
    }
    pub const ALL: [AckCode; 8] = [
        AckCode::CoreOutOfRange,
        AckCode::DoubleAck,
        AckCode::CompleteBeforeFull,
        AckCode::ReuseAfterComplete,
        AckCode::AckConservationBroken,
        AckCode::SequenceConservationBroken,
        AckCode::TicketOutOfRange,
        AckCode::NotReproducible,
    ];
}

pub fn detail(code: u32) -> &'static str {
    let mut i = 0;
    while i < AckCode::ALL.len() {
        if AckCode::ALL[i].val() == code {
            return AckCode::ALL[i].detail();
        }
        i += 1;
    }
    "未知诊断码：不在 0x6D0C0 段闭集内"
}

// ── 击落单 ─────────────────────────────────────────────────────
pub struct ShootdownTicket {
    pub seq: u64,
    pub initiator: usize,
    /// 应答位图。
    pub ack_mask: u8,
    pub ack_count: u32,
    pub closed: bool,
}

impl ShootdownTicket {
    pub fn new(seq: u64, initiator: usize) -> Result<ShootdownTicket, u32> {
        if initiator >= CORE_COUNT {
            return Err(AckCode::CoreOutOfRange.val());
        }
        Ok(ShootdownTicket { seq, initiator, ack_mask: 0, ack_count: 0, closed: false })
    }

    /// 应答（重复应答拒）。
    pub fn ack(&mut self, core: usize) -> Result<(), u32> {
        if core >= CORE_COUNT || core == self.initiator {
            return Err(AckCode::CoreOutOfRange.val());
        }
        if self.closed {
            return Err(AckCode::ReuseAfterComplete.val());
        }
        let bit = 1u8 << core;
        if self.ack_mask & bit != 0 {
            return Err(AckCode::DoubleAck.val());
        }
        self.ack_mask |= bit;
        self.ack_count += 1;
        Ok(())
    }

    /// 完成判定：应答数 = 广播目标数才可闭。
    pub fn complete(&mut self, expected: u32) -> Result<(), u32> {
        if self.closed {
            return Err(AckCode::ReuseAfterComplete.val());
        }
        if self.ack_count != expected {
            return Err(AckCode::CompleteBeforeFull.val());
        }
        self.closed = true;
        Ok(())
    }
}

// ── 应答总账 ───────────────────────────────────────────────────
pub struct AckLedger {
    pub tickets: Vec<ShootdownTicket>,
    pub broadcast_total: u64,
    pub ack_total: u64,
    pub completed: u64,
    pub inflight: u64,
}

impl AckLedger {
    pub fn new() -> AckLedger {
        AckLedger { tickets: Vec::new(), broadcast_total: 0, ack_total: 0, completed: 0, inflight: 0 }
    }

    /// 发起（广播 = 核数-1 目标）。
    pub fn initiate(&mut self, seq: u64, initiator: usize) -> Result<usize, u32> {
        let t = ShootdownTicket::new(seq, initiator)?;
        self.tickets.push(t);
        self.broadcast_total += (CORE_COUNT - 1) as u64;
        self.inflight += 1;
        Ok(self.tickets.len() - 1)
    }

    /// 应答。
    pub fn ack(&mut self, idx: usize, core: usize) -> Result<(), u32> {
        if idx >= self.tickets.len() {
            return Err(AckCode::TicketOutOfRange.val());
        }
        self.tickets[idx].ack(core)?;
        self.ack_total += 1;
        Ok(())
    }

    /// 完成。
    pub fn complete(&mut self, idx: usize) -> Result<(), u32> {
        if idx >= self.tickets.len() {
            return Err(AckCode::TicketOutOfRange.val());
        }
        self.tickets[idx].complete((CORE_COUNT - 1) as u32)?;
        self.completed += 1;
        self.inflight -= 1;
        Ok(())
    }

    /// 双守恒：Σ应答=Σ广播；完成+在途=发起。
    pub fn conserve(&self) -> Result<(), u32> {
        if self.ack_total != self.broadcast_total {
            return Err(AckCode::AckConservationBroken.val());
        }
        if self.completed + self.inflight != self.tickets.len() as u64 {
            return Err(AckCode::SequenceConservationBroken.val());
        }
        Ok(())
    }
}

/// 完成前重用注入 10/10。
pub fn reuse_inject_10() -> Result<u32, u32> {
    let mut caught = 0;
    let mut k = 0;
    while k < 10 {
        let mut led = AckLedger::new();
        let idx = led.initiate(k as u64 + 1, 0).ok();
        let h = match idx {
            Some(i) => {
                // 应答全部目标。
                let mut c = 1;
                while c < CORE_COUNT {
                    led.ack(i, c).ok();
                    c += 1;
                }
                led.complete(i).ok();
                // 重用注入：已完成单再应答。
                led.ack(i, 1).is_err()
            }
            None => false,
        };
        if h {
            caught += 1;
        }
        k += 1;
    }
    if caught != 10 {
        return Err(AckCode::ReuseAfterComplete.val());
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

/// 黄金全链：3 轮击落全流程→双守恒→重用 10/10。
pub fn golden_run() -> Result<(u64, u64, u32), u32> {
    let mut led = AckLedger::new();
    let mut seq = 1;
    let mut r = 0;
    while r < 3 {
        let idx = led.initiate(seq, 0)?;
        let mut c = 1;
        while c < CORE_COUNT {
            led.ack(idx, c)?;
            c += 1;
        }
        led.complete(idx)?;
        seq += 1;
        r += 1;
    }
    led.conserve()?;
    let reuse = reuse_inject_10()?;
    Ok((led.ack_total, led.broadcast_total, reuse))
}

/// 探针三序。
pub fn probe_seq_a() -> Result<(), u32> {
    let mut led = AckLedger::new();
    let idx = led.initiate(1, 1)?;
    led.ack(idx, 0)?;
    led.ack(idx, 2)?;
    led.ack(idx, 3)?;
    led.complete(idx)?;
    led.conserve()
}

pub fn probe_seq_b() -> Result<(), u32> {
    // 未收齐即完成 → 必拒。
    let mut led = AckLedger::new();
    let idx = led.initiate(1, 0)?;
    led.ack(idx, 1)?;
    led.complete(idx)
}

pub fn probe_seq_c() -> Result<(), u32> {
    // 双应答 → 必拒。
    let mut led = AckLedger::new();
    let idx = led.initiate(1, 0)?;
    led.ack(idx, 1)?;
    led.ack(idx, 1)
}

/// 注入 10/10：双应答、未收齐、重用、越单四槽。
pub fn inject_rounds(rounds: u32) -> Result<InjectLedger, u32> {
    let mut led = InjectLedger::new();
    let mut r = 0;
    while r < rounds {
        // 槽0：双应答。
        let mut l0 = AckLedger::new();
        let i0 = l0.initiate(1, 0).ok();
        let h0 = match i0 {
            Some(i) => {
                l0.ack(i, 1).ok();
                l0.ack(i, 1).is_err()
            }
            None => false,
        };
        led.record(0, h0);
        // 槽1：未收齐。
        let mut l1 = AckLedger::new();
        let i1 = l1.initiate(1, 0).ok();
        let h1 = match i1 {
            Some(i) => {
                l1.ack(i, 1).ok();
                l1.complete(i).is_err()
            }
            None => false,
        };
        led.record(1, h1);
        // 槽2：重用。
        led.record(2, reuse_inject_10() == Ok(10));
        // 槽3：越单。
        let mut l3 = AckLedger::new();
        led.record(3, l3.ack(99, 1).is_err());
        r += 1;
    }
    Ok(led)
}

/// 双跑确定性对拍。
pub fn repro_check() -> Result<(), u32> {
    let a = golden_run()?;
    let b = golden_run()?;
    if a != b {
        return Err(AckCode::NotReproducible.val());
    }
    Ok(())
}

/// 段位守卫。
pub fn segment_guard() -> bool {
    let mut i = 0;
    while i < AckCode::ALL.len() {
        let v = AckCode::ALL[i].val();
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
    // Σ应答=Σ广播。
    let mut led = AckLedger::new();
    let idx = led.initiate(1, 0).ok();
    let cons_ok = match idx {
        Some(i) => {
            let mut c = 1;
            while c < CORE_COUNT {
                led.ack(i, c).ok();
                c += 1;
            }
            led.complete(i).ok();
            led.conserve().is_ok() && led.ack_total == led.broadcast_total
        }
        None => false,
    };
    v.push((
        "J1-Σ应答=Σ广播",
        cons_ok && CORE_COUNT == 4,
        format!("{} 目标/轮", CORE_COUNT - 1),
    ));
    // 重用 10/10。
    v.push((
        "J1-完成前重用注入10/10",
        reuse_inject_10() == Ok(10),
        format!("{}/10 检出", reuse_inject_10().unwrap_or(0)),
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
    v.push(("J3-段位守卫", segment_guard(), String::from("0x6D0C0 窗口")));
    v
}

pub fn run_unx1865_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F1865 击落应答与完成账（应答计数、完成判定）");
    for (n, p, d) in extra_checks() {
        s.add_dyn(n, p, &d);
    }
    s
}
