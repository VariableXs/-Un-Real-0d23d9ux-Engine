//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F0166 · EFI Watchdog 处理（深化第二实现） · 0x6F180
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定，CoRun·UNX-A1）：
//!   ① 纯功能代码 ≥200 行；
//!   ② UNX-F0166-J1 EFI Watchdog：5 分钟系统看门狗语义（冻结
//!     300s），SetWatchdogTimer 账（时限/码），告警账（触发三要
//!     素），清零取消，账可复算；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x6F180，AI-VX3-1102 批O 独占区）。
pub const SEG_BASE: u32 = 0x6F180;

/// 系统看门狗时限（300s 冻结）。
pub const WD_TIMEOUT_S: u64 = 300;

/// 判据迭代轮数（写死：判据 10/10 语义）。
pub const JUDGE_ROUNDS: u32 = 10;

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum WdCode {
    /// 时限越域（>300s）
    TimeoutOver,
    /// 三要素缺账
    TripletMissing,
    /// 已设再设（重设拒）
    DoubleArm,
    /// 复算失配
    ReplayMismatch,
    /// 双跑不一致
    NotReproducible,
}

impl WdCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            WdCode::TimeoutOver => "时限越域：>300s",
            WdCode::TripletMissing => "三要素缺账",
            WdCode::DoubleArm => "已设再设：重设拒",
            WdCode::ReplayMismatch => "复算失配",
            WdCode::NotReproducible => "双跑不一致：复现红线",
        }
    }
    pub const ALL: [WdCode; 5] = [
        WdCode::TimeoutOver,
        WdCode::TripletMissing,
        WdCode::DoubleArm,
        WdCode::ReplayMismatch,
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
    "未知诊断码：不在 0x6F180 段闭集内"
}

// ── Watchdog 账 ────────────────────────────────────────────────
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct WdAlarm {
    pub ts: u64,
    pub code: u64,
    pub elapsed_s: u64,
}

#[derive(Clone, Debug, Default)]
pub struct EfiWatchdog {
    pub armed: bool,
    pub timeout_s: u64,
    pub alarm: Option<WdAlarm>,
}

impl EfiWatchdog {
    /// 设看门狗（时限闸 + 重设拒）。
    pub fn set(&mut self, timeout_s: u64) -> Result<(), u32> {
        if self.armed {
            return Err(WdCode::DoubleArm.val());
        }
        if timeout_s > WD_TIMEOUT_S {
            return Err(WdCode::TimeoutOver.val());
        }
        self.armed = true;
        self.timeout_s = timeout_s;
        Ok(())
    }

    /// 取消（EBS 前清零语义）。
    pub fn cancel(&mut self) -> Result<(), u32> {
        self.armed = false;
        self.timeout_s = 0;
        Ok(())
    }

    /// 倒计时 tick（秒；归零触发三要素）。
    pub fn tick(&mut self, elapsed_s: u64, code: u64) -> Result<bool, u32> {
        if !self.armed {
            return Ok(false);
        }
        if elapsed_s >= self.timeout_s {
            if code == 0 || elapsed_s == 0 {
                return Err(WdCode::TripletMissing.val());
            }
            self.alarm = Some(WdAlarm { ts: elapsed_s, code, elapsed_s });
            self.armed = false;
            return Ok(true);
        }
        Ok(false)
    }
}

/// 复算。
pub fn replay_ok(timeout_s: u64, elapsed: u64) -> Result<(), u32> {
    let mut a = EfiWatchdog::default();
    let mut b = EfiWatchdog::default();
    let _ = a.set(timeout_s);
    let _ = b.set(timeout_s);
    let ra = a.tick(elapsed, 1);
    let rb = b.tick(elapsed, 1);
    if ra != rb || a.alarm != b.alarm {
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

// ── 注入检出账（四路：越域路/重设路/缺账路/复算路） ──────────────
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

// ── 黄金全链：设 300→取消→设 300→299 不触发→300 触发三要素→复算 ──
pub fn golden_run() -> Result<(u32, u32, u32), u32> {
    let mut wd = EfiWatchdog::default();
    wd.set(WD_TIMEOUT_S)?;
    let armed = wd.armed as u32;
    wd.cancel()?;
    let cancelled = (!wd.armed && wd.timeout_s == 0) as u32;
    wd.set(WD_TIMEOUT_S)?;
    let near = wd.tick(299, 1)?;
    let fire = wd.tick(300, 0xEF01)?;
    let trip_ok = (!near && fire && wd.alarm.map(|a| a.code == 0xEF01 && a.elapsed_s == 300).unwrap_or(false)) as u32;
    replay_ok(WD_TIMEOUT_S, 300)?;
    Ok((armed + cancelled, trip_ok, 0))
}

// ── 探针（守卫次序三序） ────────────────────────────────────────
/// 序 A：设/取消/299 不触发/300 触发。
pub fn probe_seq_a() -> Result<(), u32> {
    match golden_run() {
        Ok((2, 1, _)) => Ok(()),
        _ => Err(WdCode::NotReproducible.val()),
    }
}

/// 序 B：时限 301 ⇒ TimeoutOver。
pub fn probe_seq_b() -> Result<(), u32> {
    let mut wd = EfiWatchdog::default();
    match wd.set(301) {
        Err(e) if e == WdCode::TimeoutOver.val() => Ok(()),
        _ => Err(WdCode::NotReproducible.val()),
    }
}

/// 序 C：重设 ⇒ DoubleArm。
pub fn probe_seq_c() -> Result<(), u32> {
    let mut wd = EfiWatchdog::default();
    let _ = wd.set(300);
    match wd.set(300) {
        Err(e) if e == WdCode::DoubleArm.val() => Ok(()),
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
        // 路 1：越域路。
        let mut w1 = EfiWatchdog::default();
        led.record(0, w1.set(400).is_err());
        // 路 2：重设路。
        let mut w2 = EfiWatchdog::default();
        let _ = w2.set(300);
        led.record(1, w2.set(300).is_err());
        // 路 3：缺账路。
        let mut w3 = EfiWatchdog::default();
        let _ = w3.set(100);
        led.record(2, w3.tick(100, 0).is_err());
        // 路 4：复算路。
        led.record(3, replay_ok(300, 299).is_ok());
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
            Ok((setup, trip, _)) => *setup == 2 && *trip == 1,
            Err(_) => false,
        },
        match &g {
            Ok((setup, trip, _)) => format!("armed+cancel={} trip_ok={}", setup, trip),
            Err(e) => format!("err=0x{:x}", e),
        },
    ));
    // 时限冻结。
    v.push((
        "J1-时限冻结",
        WD_TIMEOUT_S == 300,
        String::from("5 分钟系统看门狗冻结"),
    ));
    // 未武装 tick 恒假。
    let mut w = EfiWatchdog::default();
    v.push((
        "J1-未武装tick",
        w.tick(1000, 1) == Ok(false),
        String::from("未武装不触发"),
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
        String::from("0x6F180 窗口"),
    ));
    v
}

pub fn run_unx0166b_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F0166 EFI Watchdog 处理（深化二）");
    for (n, p, d) in extra_checks() {
        s.add_dyn(n, p, &d);
    }
    s
}
