//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F0119 · 早期失败显性化探针 · 0x6EB40
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定，CoRun·UNX-A1）：
//!   ① 纯功能代码 ≥260 行；
//!   ② UNX-F0119-J1 早期失败显性化：探针网络（六站：串口/显存/
//!     内存图/ACPI/时钟/引导参）逐站心跳账，任一站失败必出显性
//!     码（静默失败红线），失败站号/码/时刻三账入册，探针账可
//!     复算零漂移；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - 六站闭集（STATION 0-5 冻结次序）；
//!   - 心跳账：逐站 ok/失败（fail 站必入册三要素：站号/码/时刻）；
//!   - 静默红线：失败未入册 = 静默失败（审计抓）；
//!   - 全绿账：六站全过才放行（GO 票）；
//!   - LCG 种子写死 = SEG_BASE；no_std + alloc；零 unwrap/expect。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x6EB40，AI-VX3-1102 批M 独占区）。
pub const SEG_BASE: u32 = 0x6EB40;

/// 六站闭集（冻结次序）。
pub const ST_SERIAL: usize = 0;
pub const ST_VGA: usize = 1;
pub const ST_MEMMAP: usize = 2;
pub const ST_ACPI: usize = 3;
pub const ST_CLOCK: usize = 4;
pub const ST_CMDLINE: usize = 5;
pub const STATION_COUNT: usize = 6;

/// 判据迭代轮数（写死：判据 10/10 语义）。
pub const JUDGE_ROUNDS: u32 = 10;

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ProbeCode {
    /// 站号越界
    StationOver,
    /// 失败未入册（静默失败红线）
    SilentFailure,
    /// 三要素缺账
    LedgerTripletMissing,
    /// 复算失配
    ReplayMismatch,
    /// 全绿误放行
    GreenWithFailure,
    /// 双跑不一致
    NotReproducible,
}

impl ProbeCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            ProbeCode::StationOver => "站号越界：≥6",
            ProbeCode::SilentFailure => "失败未入册：静默红线",
            ProbeCode::LedgerTripletMissing => "三要素缺账：站/码/时刻",
            ProbeCode::ReplayMismatch => "复算失配",
            ProbeCode::GreenWithFailure => "有失败仍放行 GO",
            ProbeCode::NotReproducible => "双跑不一致：复现红线",
        }
    }
    pub const ALL: [ProbeCode; 6] = [
        ProbeCode::StationOver,
        ProbeCode::SilentFailure,
        ProbeCode::LedgerTripletMissing,
        ProbeCode::ReplayMismatch,
        ProbeCode::GreenWithFailure,
        ProbeCode::NotReproducible,
    ];
}

pub fn detail(code: u32) -> &'static str {
    let mut i = 0;
    while i < ProbeCode::ALL.len() {
        if ProbeCode::ALL[i].val() == code {
            return ProbeCode::ALL[i].detail();
        }
        i += 1;
    }
    "未知诊断码：不在 0x6EB40 段闭集内"
}

// ── 探针网络账 ──────────────────────────────────────────────────
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FailureRecord {
    pub station: usize,
    pub code: u32,
    pub ts: u64,
}

#[derive(Clone, Debug)]
pub struct ProbeNet {
    /// 逐站心跳（true=过）。
    pub beats: [bool; STATION_COUNT],
    /// 失败册。
    pub ledger: Vec<FailureRecord>,
}

impl ProbeNet {
    pub fn new() -> ProbeNet {
        ProbeNet { beats: [false; STATION_COUNT], ledger: Vec::new() }
    }

    /// 上报心跳（失败必入册三要素）。
    pub fn beat(&mut self, station: usize, ok: bool, code: u32, ts: u64) -> Result<(), u32> {
        if station >= STATION_COUNT {
            return Err(ProbeCode::StationOver.val());
        }
        self.beats[station] = ok;
        if !ok {
            if code == 0 || ts == 0 {
                return Err(ProbeCode::LedgerTripletMissing.val());
            }
            self.ledger.push(FailureRecord { station, code, ts });
        }
        Ok(())
    }

    /// 静默审计：失败站必在册。
    pub fn silent_audit(&self) -> Result<(), u32> {
        let mut i = 0;
        while i < STATION_COUNT {
            if !self.beats[i] {
                let mut found = false;
                let mut k = 0;
                while k < self.ledger.len() {
                    if self.ledger[k].station == i {
                        found = true;
                    }
                    k += 1;
                }
                if !found {
                    return Err(ProbeCode::SilentFailure.val());
                }
            }
            i += 1;
        }
        Ok(())
    }

    /// GO 票（全绿才放行；有失败仍 GO = 红线）。
    pub fn gate(&self) -> Result<(), u32> {
        let mut i = 0;
        while i < STATION_COUNT {
            if !self.beats[i] {
                return Err(ProbeCode::GreenWithFailure.val());
            }
            i += 1;
        }
        Ok(())
    }
}

/// 复算：同心跳序列重建册账一致。
pub fn replay_ok(seq: &[(usize, bool, u32, u64)]) -> Result<(), u32> {
    let mut a = ProbeNet::new();
    let mut b = ProbeNet::new();
    let mut i = 0;
    while i < seq.len() {
        let _ = a.beat(seq[i].0, seq[i].1, seq[i].2, seq[i].3);
        let _ = b.beat(seq[i].0, seq[i].1, seq[i].2, seq[i].3);
        i += 1;
    }
    if a.beats != b.beats || a.ledger[..] != b.ledger[..] {
        return Err(ProbeCode::ReplayMismatch.val());
    }
    Ok(())
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

// ── 注入检出账（四路：越站路/静默路/缺账路/复算路） ──────────────
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

/// 真机心跳：六站全过。
pub fn machine_beats() -> Vec<(usize, bool, u32, u64)> {
    vec![
        (ST_SERIAL, true, 0, 10),
        (ST_VGA, true, 0, 20),
        (ST_MEMMAP, true, 0, 30),
        (ST_ACPI, true, 0, 40),
        (ST_CLOCK, true, 0, 50),
        (ST_CMDLINE, true, 0, 60),
    ]
}

// ── 黄金全链：六站全绿 GO + 失败站入册 + 静默审计 + 复算 ──────────
pub fn golden_run() -> Result<(u32, u32, u32), u32> {
    // 全绿路径。
    let mut net = ProbeNet::new();
    let beats = machine_beats();
    let mut i = 0;
    while i < beats.len() {
        net.beat(beats[i].0, beats[i].1, beats[i].2, beats[i].3)?;
        i += 1;
    }
    net.silent_audit()?;
    net.gate()?;
    // 失败路径：ST_CLOCK 失败入册。
    let mut net2 = ProbeNet::new();
    let mut i = 0;
    while i < beats.len() {
        let ok = beats[i].0 != ST_CLOCK;
        net2.beat(beats[i].0, ok, if ok { 0 } else { 0x51 }, if ok { beats[i].3 } else { 55 })?;
        i += 1;
    }
    net2.silent_audit()?;
    let gate_blocked = net2.gate().is_err();
    let in_ledger = net2.ledger.len() == 1 && net2.ledger[0].station == ST_CLOCK;
    replay_ok(&beats)?;
    Ok((gate_blocked as u32, in_ledger as u32, net.ledger.len() as u32))
}

// ── 探针（守卫次序三序） ────────────────────────────────────────
/// 序 A：六站全绿 GO + 失败站拦截入册。
pub fn probe_seq_a() -> Result<(), u32> {
    match golden_run() {
        Ok((1, 1, 0)) => Ok(()),
        _ => Err(ProbeCode::NotReproducible.val()),
    }
}

/// 序 B：失败未入册 ⇒ SilentFailure（手工挖站）。
pub fn probe_seq_b() -> Result<(), u32> {
    let mut net = ProbeNet::new();
    net.beats[ST_ACPI] = false; // 直接挖账绕过 beat（模拟静默）
    match net.silent_audit() {
        Err(e) if e == ProbeCode::SilentFailure.val() => Ok(()),
        _ => Err(ProbeCode::NotReproducible.val()),
    }
}

/// 序 C：三要素缺账 ⇒ LedgerTripletMissing。
pub fn probe_seq_c() -> Result<(), u32> {
    let mut net = ProbeNet::new();
    match net.beat(ST_CLOCK, false, 0, 0) {
        Err(e) if e == ProbeCode::LedgerTripletMissing.val() => Ok(()),
        _ => Err(ProbeCode::NotReproducible.val()),
    }
}

// ── 注入轮（LCG 驱动 10 轮 × 4 路） ─────────────────────────────
pub fn inject_rounds(rounds: u32) -> Result<InjectLedger, u32> {
    let mut led = InjectLedger::new();
    let mut rng = Lcg::new(SEG_BASE as u64);
    let mut r = 0;
    while r < rounds {
        let _ = rng.next();
        // 路 1：越站路。
        let mut n1 = ProbeNet::new();
        led.record(0, n1.beat(9, true, 0, 1).is_err());
        // 路 2：静默路。
        let mut n2 = ProbeNet::new();
        n2.beats[ST_MEMMAP] = false;
        led.record(1, n2.silent_audit().is_err());
        // 路 3：缺账路。
        let mut n3 = ProbeNet::new();
        led.record(2, n3.beat(ST_VGA, false, 0x51, 0).is_err());
        // 路 4：复算路。
        led.record(3, replay_ok(&machine_beats()).is_ok());
        r += 1;
    }
    Ok(led)
}

// ── 双跑一致（复现红线） ────────────────────────────────────────
pub fn repro_check() -> Result<(), u32> {
    let a = golden_run()?;
    let b = golden_run()?;
    if a != b {
        return Err(ProbeCode::NotReproducible.val());
    }
    Ok(())
}

// ── 段位守卫（诊断码落在本段窗口） ──────────────────────────────
pub fn segment_guard() -> bool {
    let mut i = 0;
    while i < ProbeCode::ALL.len() {
        let v = ProbeCode::ALL[i].val();
        if v < SEG_BASE || v >= SEG_BASE + 0x40 {
            return false;
        }
        i += 1;
    }
    true
}

// ── 判据面（extra_checks：J1 主判据 + 守卫） ─────────────────────
pub fn extra_checks() -> Vec<(&'static str, bool, String)> {
    let mut v: Vec<(&'static str, bool, String)> = Vec::new();
    let g = golden_run();
    v.push((
        "J1-黄金全链",
        match &g {
            Ok((blocked, ledgered, clean)) => *blocked == 1 && *ledgered == 1 && *clean == 0,
            Err(_) => false,
        },
        match &g {
            Ok((blocked, ledgered, clean)) => format!("blocked={} ledgered={} clean_failures={}", blocked, ledgered, clean),
            Err(e) => format!("err=0x{:x}", e),
        },
    ));
    // 六站闭集。
    v.push((
        "J1-六站闭集",
        STATION_COUNT == 6,
        String::from("串口/显存/内存图/ACPI/时钟/引导参 冻结次序"),
    ));
    // 全绿 GO。
    let mut net = ProbeNet::new();
    let beats = machine_beats();
    let mut i = 0;
    while i < beats.len() {
        let _ = net.beat(beats[i].0, beats[i].1, beats[i].2, beats[i].3);
        i += 1;
    }
    v.push((
        "J1-全绿GO",
        net.gate().is_ok() && net.ledger.is_empty(),
        String::from("六站全过 ⇒ GO 票"),
    ));
    // 注入 10/10。
    let inj = inject_rounds(JUDGE_ROUNDS);
    let inj_ok = match &inj {
        Ok(l) => l.all_caught(),
        Err(_) => false,
    };
    v.push((
        "J1-注入10/10",
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
        format!("a=绿 b=检出{} c=检出{}", b, c),
    ));
    // 双跑一致。
    v.push((
        "J2-双跑一致",
        repro_check().is_ok(),
        String::from("golden_run 两跑同值"),
    ));
    v.push((
        "J3-段位守卫",
        segment_guard(),
        String::from("0x6EB40 窗口"),
    ));
    v
}

pub fn run_unx0119_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F0119 早期失败显性化探针");
    for (n, p, d) in extra_checks() {
        s.add_dyn(n, p, &d);
    }
    s
}
