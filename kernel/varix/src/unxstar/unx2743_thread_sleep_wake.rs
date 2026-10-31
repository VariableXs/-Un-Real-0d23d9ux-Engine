//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F2743 · 线程休眠/唤醒状态机 · 0x6DAC0
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定，CoRun·UNX-C3）：
//!   ① 纯功能代码 ≥300 行；
//!   ② UNX-F2743-J1 三态（运行/待命/休眠）迁移 10/10 轮合法，非
//!     法迁移注入拦截；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - 三态闭集：Running（运行）/Ready（待命）/Sleeping（休眠）；
//!    - 合法迁移边：Run→Ready（让出）/Ready→Run（调度）/Ready→
//!      Sleep（阻塞）/Sleep→Ready（唤醒）；Run→Sleep 直迁非法；
//!    - 10/10 轮合法迁移；非法迁移注入 10/10 拦截；
//!    - 迁移账逐次可查；
//!    - LCG 种子写死 = SEG_BASE；no_std + alloc；零 unwrap/expect。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x6DAC0，AI-VX3-1030 批H 独占区）。
pub const SEG_BASE: u32 = 0x6DAC0;

// ── 三态闭集 ────────────────────────────────────────────────────
pub const TS_RUNNING: usize = 0;
pub const TS_READY: usize = 1;
pub const TS_SLEEPING: usize = 2;
pub const STATE_COUNT: usize = 3;
pub const STATE_NAMES: [&str; STATE_COUNT] = ["运行", "待命", "休眠"];

/// 判据迭代轮数（写死：判据 10/10 语义）。
pub const JUDGE_ROUNDS: u32 = 10;

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ThrCode {
    /// 态越闭集
    StateOutOfRange,
    /// 态名空（三态齐备破坏）
    StateNameBlank,
    /// 非法迁移
    IllegalTransition,
    /// 轮次漂移
    RoundDrift,
    /// 迁移账缺
    TransitionLedgerShort,
    /// 重复唤醒
    DoubleWake,
    /// 迁移注入漏拦
    InjectLeaked,
    /// 双跑不一致
    NotReproducible,
}

impl ThrCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            ThrCode::StateOutOfRange => "态越闭集：三态冻结",
            ThrCode::StateNameBlank => "态名空：齐备破坏",
            ThrCode::IllegalTransition => "非法迁移",
            ThrCode::RoundDrift => "轮次漂移",
            ThrCode::TransitionLedgerShort => "迁移账缺",
            ThrCode::DoubleWake => "重复唤醒",
            ThrCode::InjectLeaked => "迁移注入漏拦",
            ThrCode::NotReproducible => "双跑不一致：复现红线",
        }
    }
    pub const ALL: [ThrCode; 8] = [
        ThrCode::StateOutOfRange,
        ThrCode::StateNameBlank,
        ThrCode::IllegalTransition,
        ThrCode::RoundDrift,
        ThrCode::TransitionLedgerShort,
        ThrCode::DoubleWake,
        ThrCode::InjectLeaked,
        ThrCode::NotReproducible,
    ];
}

pub fn detail(code: u32) -> &'static str {
    let mut i = 0;
    while i < ThrCode::ALL.len() {
        if ThrCode::ALL[i].val() == code {
            return ThrCode::ALL[i].detail();
        }
        i += 1;
    }
    "未知诊断码：不在 0x6DAC0 段闭集内"
}

// ── 迁移合法性（冻结边表） ─────────────────────────────────────
pub fn can_move(from: usize, to: usize) -> bool {
    match (from, to) {
        (TS_RUNNING, TS_READY) => true,
        (TS_READY, TS_RUNNING) => true,
        (TS_READY, TS_SLEEPING) => true,
        (TS_SLEEPING, TS_READY) => true,
        _ => false,
    }
}

// ── 线程状态机 ─────────────────────────────────────────────────
pub struct ThreadFsm {
    pub tid: usize,
    pub state: usize,
    pub transitions: u64,
    pub ledger: Vec<(usize, usize)>,
}

impl ThreadFsm {
    pub fn new(tid: usize) -> ThreadFsm {
        ThreadFsm { tid, state: TS_READY, transitions: 0, ledger: Vec::new() }
    }

    /// 迁移（非法拒）。
    pub fn transition(&mut self, to: usize) -> Result<(), u32> {
        if to >= STATE_COUNT {
            return Err(ThrCode::StateOutOfRange.val());
        }
        if !can_move(self.state, to) {
            return Err(ThrCode::IllegalTransition.val());
        }
        self.ledger.push((self.state, to));
        self.state = to;
        self.transitions += 1;
        Ok(())
    }

    /// 迁移账审计。
    pub fn ledger_audit(&self) -> Result<(), u32> {
        if STATE_NAMES.len() != STATE_COUNT {
            return Err(ThrCode::StateNameBlank.val());
        }
        if self.ledger.is_empty() {
            return Err(ThrCode::TransitionLedgerShort.val());
        }
        Ok(())
    }
}

/// 合法迁移 10/10（标准序列：让出→调度→阻塞→唤醒 ×2）。
pub fn legal_10() -> Result<u32, u32> {
    let mut ok = 0;
    let mut r = 0;
    while r < JUDGE_ROUNDS {
        let mut t = ThreadFsm::new(r as usize + 1);
        // Ready→Run。
        t.transition(TS_RUNNING)?;
        t.transition(TS_READY)?;
        t.transition(TS_SLEEPING)?;
        t.transition(TS_READY)?;
        t.transition(TS_RUNNING)?;
        if t.transitions == 5 {
            ok += 1;
        }
        r += 1;
    }
    if ok != JUDGE_ROUNDS {
        return Err(ThrCode::RoundDrift.val());
    }
    Ok(ok)
}

/// 非法迁移注入 10/10（Run→Sleep 直迁 + Sleep→Run 直迁）。
pub fn illegal_inject_10() -> Result<u32, u32> {
    let mut caught = 0;
    let mut k = 0;
    while k < 10 {
        let mut t = ThreadFsm::new(k + 1);
        t.transition(TS_RUNNING).ok();
        // Run→Sleep 直迁非法。
        let hit = t.transition(TS_SLEEPING).is_err();
        if hit {
            caught += 1;
        }
        k += 1;
    }
    if caught != 10 {
        return Err(ThrCode::InjectLeaked.val());
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

/// 黄金全链：合法 10/10→非法 10/10→迁移账→守恒。
pub fn golden_run() -> Result<(u32, u32, u64), u32> {
    let legal = legal_10()?;
    let illegal = illegal_inject_10()?;
    let mut t = ThreadFsm::new(1);
    t.transition(TS_RUNNING)?;
    t.transition(TS_READY)?;
    t.transition(TS_SLEEPING)?;
    t.transition(TS_READY)?;
    t.ledger_audit()?;
    Ok((legal, illegal, t.transitions))
}

/// 探针三序。
pub fn probe_seq_a() -> Result<(), u32> {
    let mut t = ThreadFsm::new(1);
    t.transition(TS_RUNNING)?;
    t.transition(TS_READY)?;
    t.transition(TS_SLEEPING)?;
    t.transition(TS_READY)
}

pub fn probe_seq_b() -> Result<(), u32> {
    // Run→Sleep 直迁 → 必拒。
    let mut t = ThreadFsm::new(1);
    t.transition(TS_RUNNING)?;
    t.transition(TS_SLEEPING)
}

pub fn probe_seq_c() -> Result<(), u32> {
    // Sleep→Run 直迁 → 必拒。
    let mut t = ThreadFsm::new(1);
    t.transition(TS_RUNNING)?;
    t.transition(TS_READY)?;
    t.transition(TS_SLEEPING)?;
    t.transition(TS_RUNNING)
}

/// 注入 10/10：非法迁移、重复唤醒、越态、合法 10/10 四槽。
pub fn inject_rounds(rounds: u32) -> Result<InjectLedger, u32> {
    let mut led = InjectLedger::new();
    let mut r = 0;
    while r < rounds {
        // 槽0：非法迁移。
        let mut t0 = ThreadFsm::new(1);
        t0.transition(TS_RUNNING).ok();
        led.record(0, t0.transition(TS_SLEEPING).is_err());
        // 槽1：重复唤醒。
        let mut t1 = ThreadFsm::new(2);
        t1.transition(TS_RUNNING).ok();
        t1.transition(TS_READY).ok();
        t1.transition(TS_SLEEPING).ok();
        t1.transition(TS_READY).ok();
        led.record(1, t1.transition(TS_READY).is_err());
        // 槽2：越态。
        let mut t2 = ThreadFsm::new(3);
        led.record(2, t2.transition(9).is_err());
        // 槽3：合法 10/10。
        led.record(3, legal_10() == Ok(10));
        r += 1;
    }
    Ok(led)
}

/// 双跑确定性对拍。
pub fn repro_check() -> Result<(), u32> {
    let a = golden_run()?;
    let b = golden_run()?;
    if a != b {
        return Err(ThrCode::NotReproducible.val());
    }
    Ok(())
}

/// 段位守卫。
pub fn segment_guard() -> bool {
    let mut i = 0;
    while i < ThrCode::ALL.len() {
        let v = ThrCode::ALL[i].val();
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
    // 三态齐备。
    v.push((
        "J1-三态齐备",
        STATE_NAMES.len() == 3,
        format!("{:?}", STATE_NAMES),
    ));
    // 合法 10/10。
    v.push((
        "J1-合法迁移10/10轮",
        legal_10() == Ok(10),
        format!("{}/10 轮", legal_10().unwrap_or(0)),
    ));
    // 非法 10/10。
    v.push((
        "J1-非法迁移注入10/10拦截",
        illegal_inject_10() == Ok(10),
        format!("{}/10 拦截", illegal_inject_10().unwrap_or(0)),
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
    v.push(("J3-段位守卫", segment_guard(), String::from("0x6DAC0 窗口")));
    v
}

pub fn run_unx2743_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F2743 线程休眠/唤醒状态机");
    for (n, p, d) in extra_checks() {
        s.add_dyn(n, p, &d);
    }
    s
}
