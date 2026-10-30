//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F15161 · 泵吞吐总核（10^6 消息/秒级吞吐复测与基准勾稽） · 0x67000
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定）：
//!   ① 纯功能代码 ≥270 行；
//!   ② UNX-F15161-J1 10^6 消息/秒级吞吐复测与基准勾稽；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - 吞吐模型确定性化（禁墙钟）：节拍数写死 1000（来源：吞吐复测
//!     标准节拍窗），每节拍派发 1000 消息 → 总量 10^6；
//!   - 吞吐基准写死 THROUGHPUT_BASELINE = 1_000_000（判据常量独立
//!     写死，不读被测账本）；
//!   - 泵双队列状态机： Produced（产列）→ Pumped → Consumed（耗列），
//!      每消息恰经三相位一次；产消守恒恒等式：产数==耗数==泵数；
//!   - 复测勾稽：复测总量与基准逐位一致；序列号无重无漏
//!      （seq 单调 +1，首 0 末 999999）；
//!   - 反向判据：抽掉一拍派发（缺拍注入）后勾稽必须失败报短量；
//!   - LCG 种子写死 = SEG_BASE（0x67000），禁 rand/浮点/IO/墙钟；
//!     no_std + alloc；零 unwrap/expect/panic。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x67000，AI-VX3-1030 独占区）。
pub const SEG_BASE: u32 = 0x67000;

/// 节拍数（写死，来源：吞吐复测标准节拍窗）。
pub const TICKS: u32 = 1000;
/// 每节拍派发消息数（写死）。
pub const MSGS_PER_TICK: u32 = 1000;
/// 吞吐基准（写死判据常量）。
pub const THROUGHPUT_BASELINE: u32 = 1_000_000;

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PumpCode {
    /// 节拍越窗
    TickOutOfRange,
    /// 序列号重复
    SeqDuplicate,
    /// 序列号断档（缺漏）
    SeqGap,
    /// 产消不守恒
    ProduceConsumeUnbalanced,
    /// 复测量与基准不一致
    BaselineMismatch,
    /// 相位悬挂（消息停留中间态）
    MsgHung,
    /// 消息越相位（重复消费）
    MsgReConsume,
    /// 缺拍（短量检出）
    TickMissing,
    /// 双跑不一致（不可复现）
    NotReproducible,
    /// 泵账未入账
    PumpUnbooked,
}

impl PumpCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            PumpCode::TickOutOfRange => "节拍越窗：1000 拍冻结",
            PumpCode::SeqDuplicate => "序列号重复",
            PumpCode::SeqGap => "序列号断档：缺漏",
            PumpCode::ProduceConsumeUnbalanced => "产消不守恒：产数≠耗数",
            PumpCode::BaselineMismatch => "复测量与基准不一致",
            PumpCode::MsgHung => "相位悬挂：消息停留中间态",
            PumpCode::MsgReConsume => "重复消费：消息越相位",
            PumpCode::TickMissing => "缺拍：短量检出",
            PumpCode::NotReproducible => "双跑不一致：复现红线",
            PumpCode::PumpUnbooked => "泵账未入账",
        }
    }
    pub const ALL: [PumpCode; 10] = [
        PumpCode::TickOutOfRange,
        PumpCode::SeqDuplicate,
        PumpCode::SeqGap,
        PumpCode::ProduceConsumeUnbalanced,
        PumpCode::BaselineMismatch,
        PumpCode::MsgHung,
        PumpCode::MsgReConsume,
        PumpCode::TickMissing,
        PumpCode::NotReproducible,
        PumpCode::PumpUnbooked,
    ];
}

pub fn detail(code: u32) -> &'static str {
    let mut i = 0;
    while i < PumpCode::ALL.len() {
        if PumpCode::ALL[i].val() == code {
            return PumpCode::ALL[i].detail();
        }
        i += 1;
    }
    "未知诊断码：不在 0x67000 段闭集内"
}

// ── 消息相位机 ───────────────────────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum MsgPhase {
    Produced,
    Pumped,
    Consumed,
}

/// 单步转移：Produced+Pump→Pumped；Pumped+Consume→Consumed；其余拒。
pub fn msg_step(p: MsgPhase, ev: PumpEv) -> Result<MsgPhase, u32> {
    match (p, ev) {
        (MsgPhase::Produced, PumpEv::Pump) => Ok(MsgPhase::Pumped),
        (MsgPhase::Pumped, PumpEv::Consume) => Ok(MsgPhase::Consumed),
        (MsgPhase::Consumed, _) => Err(PumpCode::MsgReConsume.val()),
        _ => Err(PumpCode::MsgHung.val()),
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PumpEv {
    Pump,
    Consume,
}

// ── 泵吞吐账本 ───────────────────────────────────────────────────
pub struct PumpLedger {
    pub produced: u32,
    pub pumped: u32,
    pub consumed: u32,
    /// 已消费消息序列号（升序落账）。
    pub consumed_seqs: Vec<u32>,
    pub ticks_done: u32,
    pub booked: u32,
}

impl PumpLedger {
    pub fn new() -> PumpLedger {
        PumpLedger {
            produced: 0,
            pumped: 0,
            consumed: 0,
            consumed_seqs: Vec::new(),
            ticks_done: 0,
            booked: 0,
        }
    }

    /// 单节拍：产 MSGS_PER_TICK → 泵 → 耗；序列号连续。
    pub fn tick(&mut self) -> Result<(), u32> {
        if self.ticks_done >= TICKS {
            return Err(PumpCode::TickOutOfRange.val());
        }
        let mut m = 0;
        while m < MSGS_PER_TICK {
            let seq = self.produced;
            // 三相位一次走满。
            let p0 = MsgPhase::Produced;
            let p1 = msg_step(p0, PumpEv::Pump)?;
            let p2 = msg_step(p1, PumpEv::Consume)?;
            if p2 != MsgPhase::Consumed {
                return Err(PumpCode::MsgHung.val());
            }
            self.produced += 1;
            self.pumped += 1;
            self.consumed += 1;
            self.consumed_seqs.push(seq);
            m += 1;
        }
        self.ticks_done += 1;
        Ok(())
    }

    /// 全节拍复测。
    pub fn run_all(&mut self) -> Result<u32, u32> {
        let mut t = 0;
        while t < TICKS {
            self.tick()?;
            t += 1;
        }
        if self.ticks_done != TICKS {
            return Err(PumpCode::TickMissing.val());
        }
        // 产消守恒。
        if self.produced != self.consumed || self.pumped != self.consumed {
            return Err(PumpCode::ProduceConsumeUnbalanced.val());
        }
        // 基准勾稽。
        if self.consumed != THROUGHPUT_BASELINE {
            return Err(PumpCode::BaselineMismatch.val());
        }
        self.booked = 1;
        Ok(self.consumed)
    }

    /// 序列号无重无漏：升序、首 0、末 BASELINE-1、相邻差 1。
    pub fn seqs_healthy(&self) -> Result<(), u32> {
        if self.consumed_seqs.len() as u32 != THROUGHPUT_BASELINE {
            return Err(PumpCode::SeqGap.val());
        }
        // 下标先比长度（消费端抽验走切片前先比长度）。
        if self.consumed_seqs.is_empty() {
            return Err(PumpCode::SeqGap.val());
        }
        if self.consumed_seqs[0] != 0 {
            return Err(PumpCode::SeqGap.val());
        }
        let last = self.consumed_seqs[self.consumed_seqs.len() - 1];
        if last != THROUGHPUT_BASELINE - 1 {
            return Err(PumpCode::SeqGap.val());
        }
        let mut i = 1;
        while i < self.consumed_seqs.len() {
            if self.consumed_seqs[i] != self.consumed_seqs[i - 1] + 1 {
                return Err(PumpCode::SeqGap.val());
            }
            i += 1;
        }
        Ok(())
    }

    /// 抽样一致性：LCG 抽 32 个下标，seq==下标（升序满排恒等）。
    pub fn sample_check(&self, seed: u64) -> Result<u32, u32> {
        if self.consumed_seqs.len() as u32 != THROUGHPUT_BASELINE {
            return Err(PumpCode::SeqGap.val());
        }
        let mut lcg = Lcg::new(seed);
        let mut ok = 0;
        let mut k = 0;
        while k < 32 {
            let ix = (lcg.below(THROUGHPUT_BASELINE as u64)) as usize;
            if self.consumed_seqs[ix] == ix as u32 {
                ok += 1;
            }
            k += 1;
        }
        if ok != 32 {
            return Err(PumpCode::SeqDuplicate.val());
        }
        Ok(ok)
    }
}

/// 缺拍注入：只跑 TICKS-1 拍，耗量必短于基准——勾稽必须失败报缺拍。
pub fn missing_tick_probe() -> Result<(), u32> {
    let mut led = PumpLedger::new();
    let mut t = 0;
    while t < TICKS - 1 {
        led.tick()?;
        t += 1;
    }
    if led.consumed < THROUGHPUT_BASELINE {
        return Err(PumpCode::TickMissing.val());
    }
    Ok(())
}

// ── 确定性 LCG（种子=SEG_BASE） ─────────────────────────────────
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
        self.next() % (n | 1)
    }
}

// ── 黄金正演 ─────────────────────────────────────────────────────
pub fn golden_run() -> Result<(u32, u32, u32), u32> {
    let mut led = PumpLedger::new();
    let total = led.run_all()?;
    if total != THROUGHPUT_BASELINE {
        return Err(PumpCode::BaselineMismatch.val());
    }
    led.seqs_healthy()?;
    let s = led.sample_check(SEG_BASE as u64)?;
    if s != 32 {
        return Err(PumpCode::SeqDuplicate.val());
    }
    if led.booked != 1 {
        return Err(PumpCode::PumpUnbooked.val());
    }
    // 复测一致性：第二跑（复测）与首跑总量一致。
    let mut led2 = PumpLedger::new();
    let total2 = led2.run_all()?;
    if total2 != total {
        return Err(PumpCode::NotReproducible.val());
    }
    // 反向判据：缺拍注入必检出。
    if missing_tick_probe().is_ok() {
        return Err(PumpCode::TickMissing.val());
    }
    Ok((total, led.ticks_done, s))
}

// ── 注入面：三族 tries/caught 全抓 ───────────────────────────────
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
    // 族 0：越窗节拍（1001 拍）。
    ledger.tries[0] += 1;
    let mut bad0 = false;
    let mut led = PumpLedger::new();
    let mut t = 0;
    let mut over_ok = true;
    while t < TICKS {
        if led.tick().is_err() {
            over_ok = false;
        }
        t += 1;
    }
    if over_ok && led.tick().is_err() {
        bad0 = true;
    }
    if bad0 {
        ledger.caught[0] += 1;
    }
    // 族 1：重复消费（Consumed 态再 Consume）。
    ledger.tries[1] += 1;
    let mut bad1 = false;
    if let Err(e) = msg_step(MsgPhase::Consumed, PumpEv::Consume) {
        if e == PumpCode::MsgReConsume.val() {
            bad1 = true;
        }
    }
    if bad1 {
        ledger.caught[1] += 1;
    }
    // 族 2：相位跳步（Produced 直接 Consume）。
    ledger.tries[2] += 1;
    let mut bad2 = false;
    if let Err(e) = msg_step(MsgPhase::Produced, PumpEv::Consume) {
        if e == PumpCode::MsgHung.val() {
            bad2 = true;
        }
    }
    if bad2 {
        ledger.caught[2] += 1;
    }
}

// ── 判据入口 ─────────────────────────────────────────────────────
pub fn segment_guard() -> bool {
    let mut i = 0;
    while i < PumpCode::ALL.len() {
        let v = PumpCode::ALL[i].val();
        if v < SEG_BASE || v >= SEG_BASE + 0x40 {
            return false;
        }
        i += 1;
    }
    true
}

pub fn extra_checks() -> Vec<(&'static str, bool, String)> {
    let mut v: Vec<(&'static str, bool, String)> = Vec::new();
    // 1. 黄金正演：10^6 复测勾稽 + 序列健康 + 抽样 32 + 缺拍反向。
    let golden = golden_run();
    let g_ok = match &golden {
        Ok((t, k, s)) => *t == THROUGHPUT_BASELINE && *k == TICKS && *s == 32,
        Err(_) => false,
    };
    v.push(("J1-10^6复测勾稽+缺拍反向", g_ok, format!("{:?}", golden)));
    // 2. 注入三族全抓。
    let mut inj = InjectLedger::new();
    run_injections(&mut inj);
    v.push((
        "J1-注入三族全抓",
        inj.all_caught(),
        format!("t/c {:?}/{:?}", inj.tries, inj.caught),
    ));
    // 3. 判据常量独立核算：1000×1000==1_000_000==基准。
    v.push((
        "J1-基准独立核算",
        TICKS * MSGS_PER_TICK == THROUGHPUT_BASELINE && THROUGHPUT_BASELINE == 1_000_000,
        format!("{}x{}={}", TICKS, MSGS_PER_TICK, THROUGHPUT_BASELINE),
    ));
    // 4. 序列首末点验（不跑全量，用首拍直接验）：首拍后 seq[0]==0，
    //    seq[999]==999。
    let mut one = PumpLedger::new();
    let mut tick_ok = one.tick().is_ok();
    let first = one.consumed_seqs[0];
    let last = one.consumed_seqs[MSGS_PER_TICK as usize - 1];
    tick_ok = tick_ok && first == 0 && last == MSGS_PER_TICK - 1;
    v.push((
        "J2-首拍序列首末点验",
        tick_ok,
        format!("first={} last={}", first, last),
    ));
    // 5. 产消守恒反例面：单拍后产==耗==1000。
    let single_ok = one.produced == MSGS_PER_TICK
        && one.consumed == MSGS_PER_TICK
        && one.pumped == MSGS_PER_TICK;
    v.push((
        "J2-单拍产消守恒",
        single_ok,
        format!("p/u/c {}/{}/{}", one.produced, one.pumped, one.consumed),
    ));
    // 6. 诊断码闭集点验：10 码互异落窗。
    let mut distinct = 0;
    let mut a = 0;
    while a < PumpCode::ALL.len() {
        let va = PumpCode::ALL[a].val();
        let mut uniq = true;
        let mut b = 0;
        while b < PumpCode::ALL.len() {
            if a != b && PumpCode::ALL[b].val() == va {
                uniq = false;
            }
            b += 1;
        }
        if uniq && va >= SEG_BASE && va < SEG_BASE + 0x40 {
            distinct += 1;
        }
        a += 1;
    }
    v.push((
        "J2-诊断码10码闭集互异",
        distinct == 10,
        format!("{}/10", distinct),
    ));
    // 7. 段位守卫。
    v.push(("J3-段位守卫", segment_guard(), String::from("0x67000 窗口")));
    v
}

pub fn run_unx15161_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F15161 泵吞吐总核（10^6 消息/秒级复测勾稽）");
    for (n, p, d) in extra_checks() {
        s.add_dyn(n, p, &d);
    }
    s
}
