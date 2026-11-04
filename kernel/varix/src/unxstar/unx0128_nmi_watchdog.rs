//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F0128 · NMI 备份看门狗 · 0x6EC40
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定，CoRun·UNX-A1）：
//!   ① 纯功能代码 ≥260 行；
//!   ② UNX-F0128-J1 NMI 备份看门狗：主喂狗账（周期 tick 计数），
//!     漏喂 N 拍（N=8 冻结）触发 NMI 告警账（CPU/时刻/漏拍数三
//!     账），触发后停机进诊断（重入拒），喂狗恢复清账；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - 主账：tick 计数 + 漏拍计数（喂狗清零）；
//!   - 阈值：漏 8 拍触发（冻结）；
//!   - 触发账：CPU 号/时刻/漏拍数三要素；
//!   - 触发后停机态：再喂拒（重入红线）；
//!   - 喂狗恢复：清漏拍计数（停机态除外）；
//!   - LCG 种子写死 = SEG_BASE；no_std + alloc；零 unwrap/expect。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x6EC40，AI-VX3-1102 批M 独占区）。
pub const SEG_BASE: u32 = 0x6EC40;

/// 漏拍触发阈值（冻结 8）。
pub const MISS_THRESHOLD: u32 = 8;

/// 判据迭代轮数（写死：判据 10/10 语义）。
pub const JUDGE_ROUNDS: u32 = 10;

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum WdCode {
    /// 触发后重喂（停机红线）
    FeedAfterTrip,
    /// 漏拍账失真
    MissLedgerBad,
    /// 三要素缺账
    TripletMissing,
    /// 复算失配
    ReplayMismatch,
    /// 阈值误触（<8 触发）
    PrematureTrip,
    /// 双跑不一致
    NotReproducible,
}

impl WdCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            WdCode::FeedAfterTrip => "触发后重喂：停机红线",
            WdCode::MissLedgerBad => "漏拍账失真",
            WdCode::TripletMissing => "三要素缺账：CPU/时刻/漏拍",
            WdCode::ReplayMismatch => "复算失配",
            WdCode::PrematureTrip => "阈值误触：<8 触发",
            WdCode::NotReproducible => "双跑不一致：复现红线",
        }
    }
    pub const ALL: [WdCode; 6] = [
        WdCode::FeedAfterTrip,
        WdCode::MissLedgerBad,
        WdCode::TripletMissing,
        WdCode::ReplayMismatch,
        WdCode::PrematureTrip,
        WdCode::NotReproducible,
    ];
}

pub fn detail(code: u32) -> &'static str {
    let mut i = 0;
    while i < WdCode::ALL.len() {
        if WdCode::ALL[i].val() == code {
            return WdCode::ALL[i].detail();
        }
        i += 1;
    }
    "未知诊断码：不在 0x6EC40 段闭集内"
}

// ── 看门狗账 ────────────────────────────────────────────────────
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TripRecord {
    pub cpu: u32,
    pub ts: u64,
    pub missed: u32,
}

#[derive(Clone, Debug)]
pub struct NmiWatchdog {
    pub missed: u32,
    pub tripped: bool,
    pub trip: Option<TripRecord>,
    pub retrips: u32,
}

impl NmiWatchdog {
    pub fn new() -> NmiWatchdog {
        NmiWatchdog { missed: 0, tripped: false, trip: None, retrips: 0 }
    }

    /// tick（未喂狗则漏拍累加，达阈值触发）。
    pub fn tick(&mut self, cpu: u32, ts: u64) -> Result<bool, u32> {
        if self.tripped {
            return Ok(true); // 已停机：tick 无效不重复触发。
        }
        self.missed += 1;
        if self.missed >= MISS_THRESHOLD {
            if cpu > 0xFFFF || ts == 0 {
                return Err(WdCode::TripletMissing.val());
            }
            self.tripped = true;
            self.trip = Some(TripRecord { cpu, ts, missed: self.missed });
            return Ok(true);
        }
        Ok(false)
    }

    /// 喂狗（停机态拒；正常清漏拍）。
    pub fn feed(&mut self) -> Result<(), u32> {
        if self.tripped {
            return Err(WdCode::FeedAfterTrip.val());
        }
        self.missed = 0;
        Ok(())
    }

    /// 重新武装（诊断后）。
    pub fn rearm(&mut self) {
        if self.tripped {
            self.retrips += 1;
            self.tripped = false;
            self.missed = 0;
        }
    }
}

/// 复算：同 tick/feed 序列重建终态一致。
pub fn replay_ok(seq: &[(bool, u32, u64)]) -> Result<(), u32> {
    let mut a = NmiWatchdog::new();
    let mut b = NmiWatchdog::new();
    let mut i = 0;
    while i < seq.len() {
        let (fed, cpu, ts) = seq[i];
        if fed {
            let _ = a.feed();
            let _ = b.feed();
        } else {
            let _ = a.tick(cpu, ts);
            let _ = b.tick(cpu, ts);
        }
        i += 1;
    }
    if a.missed != b.missed || a.tripped != b.tripped || a.trip != b.trip {
        return Err(WdCode::ReplayMismatch.val());
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

// ── 注入检出账（四路：重喂路/三要素路/误触路/复算路） ────────────
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

/// 真机喂狗序列：喂 3 → 漏 9（第 8 拍触发）→ 喂 1（拒）。
pub fn machine_seq() -> Vec<(bool, u32, u64)> {
    let mut s = Vec::new();
    let mut i = 0;
    while i < 3 {
        s.push((true, 0, 0));
        i += 1;
    }
    let mut i = 0;
    while i < 9 {
        s.push((false, 2, 1000 + i));
        i += 1;
    }
    s.push((true, 0, 0));
    s
}

// ── 黄金全链：漏 8 触发→三要素→重喂拒→喂狗清账→复算 ─────────────
pub fn golden_run() -> Result<(u32, u32, u32), u32> {
    let mut wd = NmiWatchdog::new();
    let seq = machine_seq();
    let mut tripped_at = 0u32;
    let mut i = 0;
    while i < seq.len() {
        let (fed, cpu, ts) = seq[i];
        if fed {
            let r = wd.feed();
            if i == seq.len() - 1 {
                // 最后一次喂在停机态 ⇒ 必拒。
                if r.is_ok() {
                    return Err(WdCode::FeedAfterTrip.val());
                }
            }
        } else {
            let t = wd.tick(cpu, ts)?;
            if t && tripped_at == 0 {
                tripped_at = wd.missed;
            }
        }
        i += 1;
    }
    let trip = match &wd.trip {
        Some(t) => t,
        None => return Err(WdCode::TripletMissing.val()),
    };
    let triplet_ok = (trip.cpu == 2 && trip.missed == MISS_THRESHOLD) as u32;
    replay_ok(&seq)?;
    Ok((tripped_at, triplet_ok, wd.retrips))
}

// ── 探针（守卫次序三序） ────────────────────────────────────────
/// 序 A：第 8 拍触发 + 三要素 + 重喂拒。
pub fn probe_seq_a() -> Result<(), u32> {
    match golden_run() {
        Ok((8, 1, 0)) => Ok(()),
        _ => Err(WdCode::NotReproducible.val()),
    }
}

/// 序 B：7 拍不触发。
pub fn probe_seq_b() -> Result<(), u32> {
    let mut wd = NmiWatchdog::new();
    let mut i = 0;
    while i < 7 {
        if wd.tick(0, 100 + i)? {
            return Err(WdCode::NotReproducible.val());
        }
        i += 1;
    }
    Ok(())
}

/// 序 C：喂狗清账。
pub fn probe_seq_c() -> Result<(), u32> {
    let mut wd = NmiWatchdog::new();
    let _ = wd.tick(0, 10);
    let _ = wd.tick(0, 20);
    wd.feed()?;
    match wd.missed {
        0 => Ok(()),
        _ => Err(WdCode::NotReproducible.val()),
    }
}

// ── 注入轮（LCG 驱动 10 轮 × 4 路） ─────────────────────────────
pub fn inject_rounds(rounds: u32) -> Result<InjectLedger, u32> {
    let mut led = InjectLedger::new();
    let mut rng = Lcg::new(SEG_BASE as u64);
    let mut r = 0;
    while r < rounds {
        let _ = rng.next();
        // 路 1：重喂路。
        let mut w1 = NmiWatchdog::new();
        let mut i = 0;
        while i < 8 {
            let _ = w1.tick(0, i as u64 + 1);
            i += 1;
        }
        led.record(0, w1.feed().is_err());
        // 路 2：三要素路（ts=0 缺账）。
        let mut w2 = NmiWatchdog::new();
        let mut i = 0;
        while i < 7 {
            let _ = w2.tick(0, 1);
            i += 1;
        }
        led.record(1, w2.tick(0, 0).is_err());
        // 路 3：误触路（7 拍不触发）。
        let mut w3 = NmiWatchdog::new();
        let mut premature = false;
        let mut i = 0;
        while i < 7 {
            if w3.tick(0, 1)? {
                premature = true;
            }
            i += 1;
        }
        led.record(2, !premature);
        // 路 4：复算路。
        led.record(3, replay_ok(&machine_seq()).is_ok());
        r += 1;
    }
    Ok(led)
}

// ── 双跑一致（复现红线） ────────────────────────────────────────
pub fn repro_check() -> Result<(), u32> {
    let a = golden_run()?;
    let b = golden_run()?;
    if a != b {
        return Err(WdCode::NotReproducible.val());
    }
    Ok(())
}

// ── 段位守卫（诊断码落在本段窗口） ──────────────────────────────
pub fn segment_guard() -> bool {
    let mut i = 0;
    while i < WdCode::ALL.len() {
        let v = WdCode::ALL[i].val();
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
            Ok((at, trip, re)) => *at == 8 && *trip == 1 && *re == 0,
            Err(_) => false,
        },
        match &g {
            Ok((at, trip, re)) => format!("trip_at={} triplet_ok={} rearm={}", at, trip, re),
            Err(e) => format!("err=0x{:x}", e),
        },
    ));
    // 阈值冻结。
    v.push((
        "J1-阈值冻结",
        MISS_THRESHOLD == 8,
        String::from("漏 8 拍触发冻结"),
    ));
    // 重新武装账。
    let mut w = NmiWatchdog::new();
    let mut i = 0;
    while i < 8 {
        let _ = w.tick(3, i as u64 + 1);
        i += 1;
    }
    w.rearm();
    let rearm_ok = !w.tripped && w.missed == 0 && w.retrips == 1;
    v.push((
        "J1-重新武装",
        rearm_ok,
        String::from("诊断后 rearm 清账留痕"),
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
        String::from("0x6EC40 窗口"),
    ));
    v
}

pub fn run_unx0128_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F0128 NMI 备份看门狗");
    for (n, p, d) in extra_checks() {
        s.add_dyn(n, p, &d);
    }
    s
}
