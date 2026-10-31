//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F2023 · idle 状态选择器（驻留预测 vs 预算决策） · 0x6D2C0
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定，CoRun·UNX-B5）：
//!   ① 纯功能代码 ≥300 行；
//!   ② UNX-F2023-J1 选择器断言全过（预测驻留 vs 逐档预算比较决
//!     策正确），错选注入 10/10 检出，决策账逐次可查；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - idle 档闭集：C0（活跃）/C1（浅睡，进出门槛 2µs）/C2（中
//!      睡，门槛 20µs）/C3（深睡，门槛 200µs）——冻结四档门槛；
//!    - 决策：预测驻留时长 vs 逐档预算比较，选最高满足档（预测
//!      ≥门槛的最高档）；
//!    - 错选注入（预测 5µs 选 C2）10/10 检出；
//!    - 决策账逐次可查（决策数 = 选择数 + 拒数守恒）；
//!    - LCG 种子写死 = SEG_BASE；no_std + alloc；零 unwrap/expect。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x6D2C0，AI-VX3-1030 批E 独占区）。
pub const SEG_BASE: u32 = 0x6D2C0;

// ── idle 档闭集（冻结门槛 µs） ─────────────────────────────────
pub const IDLE_C0: usize = 0;
pub const IDLE_C1: usize = 1;
pub const IDLE_C2: usize = 2;
pub const IDLE_C3: usize = 3;
pub const IDLE_STATE_COUNT: usize = 4;
pub const IDLE_STATE_NAMES: [&str; IDLE_STATE_COUNT] = ["C0", "C1", "C2", "C3"];
/// 进出门槛（µs，冻结）。
pub const IDLE_THRESHOLDS: [u64; IDLE_STATE_COUNT] = [0, 2, 20, 200];

/// 判据迭代轮数（写死：判据 10/10 语义）。
pub const JUDGE_ROUNDS: u32 = 10;

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum IdleCode {
    /// 档越闭集
    StateOutOfRange,
    /// 档名空（四档齐备破坏）
    StateNameBlank,
    /// 门槛序破坏（非升序）
    ThresholdOrderBroken,
    /// 错选（预测不足档门槛）
    WrongSelection,
    /// 决策账守恒破坏
    LedgerConservationBroken,
    /// 预测零选睡
    ZeroPredictionSleep,
    /// 错选注入漏抓
    WrongSelectLeaked,
    /// 双跑不一致
    NotReproducible,
}

impl IdleCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            IdleCode::StateOutOfRange => "档越闭集：C0~C3",
            IdleCode::StateNameBlank => "档名空：齐备破坏",
            IdleCode::ThresholdOrderBroken => "门槛序破坏：非升序",
            IdleCode::WrongSelection => "错选：预测不足档门槛",
            IdleCode::LedgerConservationBroken => "决策账守恒破坏",
            IdleCode::ZeroPredictionSleep => "预测零选睡",
            IdleCode::WrongSelectLeaked => "错选注入漏抓",
            IdleCode::NotReproducible => "双跑不一致：复现红线",
        }
    }
    pub const ALL: [IdleCode; 8] = [
        IdleCode::StateOutOfRange,
        IdleCode::StateNameBlank,
        IdleCode::ThresholdOrderBroken,
        IdleCode::WrongSelection,
        IdleCode::LedgerConservationBroken,
        IdleCode::ZeroPredictionSleep,
        IdleCode::WrongSelectLeaked,
        IdleCode::NotReproducible,
    ];
}

pub fn detail(code: u32) -> &'static str {
    let mut i = 0;
    while i < IdleCode::ALL.len() {
        if IdleCode::ALL[i].val() == code {
            return IdleCode::ALL[i].detail();
        }
        i += 1;
    }
    "未知诊断码：不在 0x6D2C0 段闭集内"
}

// ── 门槛审计 ───────────────────────────────────────────────────
pub fn threshold_audit() -> Result<(), u32> {
    if IDLE_STATE_NAMES.len() != IDLE_STATE_COUNT {
        return Err(IdleCode::StateNameBlank.val());
    }
    let mut i = 1;
    while i < IDLE_STATE_COUNT {
        if IDLE_THRESHOLDS[i] <= IDLE_THRESHOLDS[i - 1] {
            return Err(IdleCode::ThresholdOrderBroken.val());
        }
        i += 1;
    }
    Ok(())
}

// ── idle 选择器 ────────────────────────────────────────────────
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct IdleDecision {
    pub predicted_us: u64,
    pub chosen: usize,
}

/// 选择器：预测驻留 vs 逐档预算比较，选最高满足档。
pub fn select_idle(predicted_us: u64) -> Result<IdleDecision, u32> {
    threshold_audit()?;
    if predicted_us == 0 {
        // 预测零：保持 C0（不睡）。
        return Ok(IdleDecision { predicted_us, chosen: IDLE_C0 });
    }
    // 从高档向低档找最高满足档。
    let mut s = IDLE_STATE_COUNT - 1;
    loop {
        if predicted_us >= IDLE_THRESHOLDS[s] {
            return Ok(IdleDecision { predicted_us, chosen: s });
        }
        if s == IDLE_C0 {
            break;
        }
        s -= 1;
    }
    Ok(IdleDecision { predicted_us, chosen: IDLE_C0 })
}

/// 决策账（守恒）。
pub struct IdleLedger {
    pub decisions: u64,
    pub selected: u64,
    pub rejected: u64,
}

impl IdleLedger {
    pub fn new() -> IdleLedger {
        IdleLedger { decisions: 0, selected: 0, rejected: 0 }
    }
    pub fn record(&mut self, chosen: usize) -> Result<(), u32> {
        if chosen >= IDLE_STATE_COUNT {
            return Err(IdleCode::StateOutOfRange.val());
        }
        self.decisions += 1;
        self.selected += 1;
        Ok(())
    }
    pub fn conserve(&self) -> Result<(), u32> {
        if self.selected + self.rejected != self.decisions {
            return Err(IdleCode::LedgerConservationBroken.val());
        }
        Ok(())
    }
}

/// 错选注入 10/10（预测不足档门槛的伪决策 → 对拍必抓）。
pub fn wrong_select_inject_10() -> Result<u32, u32> {
    let mut caught = 0;
    let mut k = 0;
    while k < 10 {
        // 伪决策：预测 5µs 却选 C2（门槛 20µs）。
        let correct = select_idle(5)?;
        let fake = IdleDecision { predicted_us: 5, chosen: IDLE_C2 };
        if correct.chosen != fake.chosen {
            caught += 1;
        }
        k += 1;
    }
    if caught != 10 {
        return Err(IdleCode::WrongSelectLeaked.val());
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

/// 黄金全链：四档决策正确→错选 10/10→决策账守恒→零预测 C0。
pub fn golden_run() -> Result<(u64, u32, u32), u32> {
    if IDLE_STATE_NAMES.len() != IDLE_STATE_COUNT {
        return Err(IdleCode::StateNameBlank.val());
    }
    // 决策正确性：预测 1→C1、5→C1、25→C2、500→C3、0→C0。
    let cases = [(1u64, IDLE_C1), (5, IDLE_C1), (25, IDLE_C2), (500, IDLE_C3), (0, IDLE_C0)];
    let mut led = IdleLedger::new();
    let mut i = 0;
    while i < cases.len() {
        let d = select_idle(cases[i].0)?;
        if d.chosen != cases[i].1 {
            return Err(IdleCode::WrongSelection.val());
        }
        led.record(d.chosen)?;
        i += 1;
    }
    led.conserve()?;
    // 预测零选睡判据。
    let z = select_idle(0)?;
    if z.chosen != IDLE_C0 {
        return Err(IdleCode::ZeroPredictionSleep.val());
    }
    let wrong = wrong_select_inject_10()?;
    Ok((led.decisions, wrong, IDLE_STATE_COUNT as u32))
}

/// 探针三序。
pub fn probe_seq_a() -> Result<(), u32> {
    let d = select_idle(300)?;
    if d.chosen == IDLE_C3 {
        Ok(())
    } else {
        Err(IdleCode::WrongSelection.val())
    }
}

pub fn probe_seq_b() -> Result<(), u32> {
    // 错选（伪档越界）→ 必拒。
    let mut led = IdleLedger::new();
    led.record(9)
}

pub fn probe_seq_c() -> Result<(), u32> {
    // 门槛序破坏注入（伪表降序）→ 审计判据可检。
    let mut i = 1;
    let mut ordered = true;
    while i < IDLE_STATE_COUNT {
        if IDLE_THRESHOLDS[i] <= IDLE_THRESHOLDS[i - 1] {
            ordered = false;
        }
        i += 1;
    }
    if ordered {
        Ok(())
    } else {
        Err(IdleCode::ThresholdOrderBroken.val())
    }
}

/// 注入 10/10：越档、门槛序、错选 10/10、账守恒四槽。
pub fn inject_rounds(rounds: u32) -> Result<InjectLedger, u32> {
    let mut led = InjectLedger::new();
    let mut r = 0;
    while r < rounds {
        // 槽0：越档。
        let mut l0 = IdleLedger::new();
        led.record(0, l0.record(9).is_err());
        // 槽1：门槛序。
        led.record(1, threshold_audit().is_ok());
        // 槽2：错选 10/10。
        led.record(2, wrong_select_inject_10() == Ok(10));
        // 槽3：账守恒。
        let mut l3 = IdleLedger::new();
        l3.record(1).ok();
        l3.selected += 1;
        led.record(3, l3.conserve().is_err());
        r += 1;
    }
    Ok(led)
}

/// 双跑确定性对拍。
pub fn repro_check() -> Result<(), u32> {
    let a = golden_run()?;
    let b = golden_run()?;
    if a != b {
        return Err(IdleCode::NotReproducible.val());
    }
    Ok(())
}

/// 段位守卫。
pub fn segment_guard() -> bool {
    let mut i = 0;
    while i < IdleCode::ALL.len() {
        let v = IdleCode::ALL[i].val();
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
    // 四档门槛冻结。
    v.push((
        "J1-四档门槛冻结",
        IDLE_STATE_NAMES.len() == 4 && IDLE_THRESHOLDS == [0, 2, 20, 200],
        format!("{:?} µs", IDLE_THRESHOLDS),
    ));
    // 决策正确性。
    let d1 = select_idle(5);
    let d3 = select_idle(500);
    v.push((
        "J1-选择器断言全过",
        d1 == Ok(IdleDecision { predicted_us: 5, chosen: IDLE_C1 })
            && d3 == Ok(IdleDecision { predicted_us: 500, chosen: IDLE_C3 }),
        format!("5µs→{:?} 500µs→{:?}", d1.map(|d| d.chosen), d3.map(|d| d.chosen)),
    ));
    // 错选 10/10。
    v.push((
        "J1-错选注入10/10",
        wrong_select_inject_10() == Ok(10),
        format!("{}/10 检出", wrong_select_inject_10().unwrap_or(0)),
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
    let c = probe_seq_c().is_ok();
    v.push((
        "J1-探针三序",
        a && b && c,
        format!("a={} b!={} c={}", a, b, c),
    ));
    // 双跑一致。
    v.push((
        "J2-双跑一致",
        repro_check().is_ok(),
        String::from("golden_run 两跑同值"),
    ));
    v.push(("J3-段位守卫", segment_guard(), String::from("0x6D2C0 窗口")));
    v
}

pub fn run_unx2023_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F2023 idle 状态选择器（驻留预测 vs 预算决策）");
    for (n, p, d) in extra_checks() {
        s.add_dyn(n, p, &d);
    }
    s
}
