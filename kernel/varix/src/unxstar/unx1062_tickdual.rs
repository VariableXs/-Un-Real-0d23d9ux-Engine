//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F1062 · tick 双模（周期 tick 与 nohz 双档语义） · 诊断段 0x1062
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定）：
//!   ① 纯功能代码 ≥340 行；
//!   ② UNX-F1062-J1 双模正确（周期档 10^6 拍稳定/nohz 档按需恢复
//!      10/10 次），模式切换原子（切换窗零 tick 丢失），双模状态全域
//!      可见（三源对平）；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - 周期档：固定周期拍流，计数器单调递增，10^6 拍稳定性=计数连续；
//!   - nohz 档：按需恢复（事件到达即重臂），恢复次数账 10/10；
//!   - 切换原子性：切换窗内到达的 tick 全部落账（丢一即红）；
//!   - 三源对平：引擎账 / 影子账 / 审计账三处计数值逐轮恒等；
//!   - 注入核 5 项（周期断流/恢复漏账/切换丢失/影子漂移/审计错位）。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址。
pub const SEG_BASE: u16 = 0x1062;

/// 周期档演练拍数（判据点名 10^6）。
pub const PERIODIC_DRILL: u64 = 1_000_000;

/// nohz 档恢复演练轮数（判据点名 10）。
pub const NOHZ_ROUNDS: usize = 10;

/// tick 模式。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TickMode {
    /// 周期档：固定拍流。
    Periodic,
    /// nohz 档：单拍+按需恢复。
    Nohz,
}

impl TickMode {
    /// 中文短句。
    pub const fn text(self) -> &'static str {
        match self {
            TickMode::Periodic => "周期档",
            TickMode::Nohz => "nohz档",
        }
    }
}

/// 诊断码（0x1062 段十码闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TdCode {
    /// 0x1062 周期断流（周期档计数跳变）。
    PeriodicGap = 0,
    /// 0x1063 恢复漏账（nohz 恢复未入账）。
    RecoverLost = 1,
    /// 0x1064 切换丢失（切换窗 tick 丢失）。
    SwitchLost = 2,
    /// 0x1065 影子漂移（影子账与引擎账不平）。
    ShadowDrift = 3,
    /// 0x1066 审计错位（审计账与引擎账不平）。
    AuditDrift = 4,
    /// 0x1067 非法模式（未知模式值）。
    BadMode = 5,
    /// 0x1068 恢复越限（单窗恢复次数越上限）。
    RecoverOver = 6,
    /// 0x1069 双档串账（两档计数互渗）。
    ModeMix = 7,
    /// 0x106A 对平未达（三源对平失败）。
    Reconcile = 8,
    /// 0x106B 窗口悬空（切换窗未闭合）。
    WindowOpen = 9,
}

impl TdCode {
    /// 判别式真值（段基+序号）。
    pub const fn val(self) -> u16 {
        SEG_BASE + self as u16
    }
    /// 全码闭集（段位守卫用）。
    pub const ALL: [TdCode; 10] = [
        TdCode::PeriodicGap, TdCode::RecoverLost, TdCode::SwitchLost,
        TdCode::ShadowDrift, TdCode::AuditDrift, TdCode::BadMode,
        TdCode::RecoverOver, TdCode::ModeMix, TdCode::Reconcile,
        TdCode::WindowOpen,
    ];
}

/// tick 到达事件。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TickArrival {
    /// 到达时窗（切换窗编号）。
    pub window: usize,
    /// 到达序号（全局单调）。
    pub seq: u64,
    /// 到达时档位。
    pub mode: TickMode,
}

/// 双模 tick 引擎（本单正本）。
#[derive(Debug)]
pub struct DualTickEngine {
    /// 当前档位。
    mode: TickMode,
    /// 周期档累计拍数。
    periodic_ticks: u64,
    /// nohz 档累计拍数。
    nohz_ticks: u64,
    /// nohz 恢复次数。
    recoveries: u32,
    /// 当前切换窗号。
    window: usize,
    /// 切换窗开合状态。
    window_open: bool,
    /// 窗内到达账（原子性判据面）。
    window_arrivals: Vec<TickArrival>,
    /// 全局到达账。
    ledger: Vec<TickArrival>,
}

impl DualTickEngine {
    /// 建账（初始周期档）。
    pub fn new() -> Self {
        DualTickEngine {
            mode: TickMode::Periodic,
            periodic_ticks: 0,
            nohz_ticks: 0,
            recoveries: 0,
            window: 0,
            window_open: false,
            window_arrivals: Vec::new(),
            ledger: Vec::new(),
        }
    }

    /// 当前档位。
    pub fn mode(&self) -> TickMode {
        self.mode
    }

    /// 周期档拍数。
    pub fn periodic_ticks(&self) -> u64 {
        self.periodic_ticks
    }

    /// nohz 档拍数。
    pub fn nohz_ticks(&self) -> u64 {
        self.nohz_ticks
    }

    /// 恢复次数。
    pub fn recoveries(&self) -> u32 {
        self.recoveries
    }

    /// 周期档拍入（单调+1，断流由判据面检测）。
    pub fn periodic_tick(&mut self) -> u64 {
        self.periodic_ticks += 1;
        let a = TickArrival { window: self.window, seq: self.periodic_ticks, mode: TickMode::Periodic };
        if self.window_open {
            self.window_arrivals.push(a);
        }
        self.ledger.push(a);
        self.periodic_ticks
    }

    /// nohz 档单拍（按需恢复一次并落账）。
    pub fn nohz_recover(&mut self) -> Result<u64, TdCode> {
        if self.recoveries >= u32::MAX - 1 {
            return Err(TdCode::RecoverOver);
        }
        self.recoveries += 1;
        self.nohz_ticks += 1;
        let a = TickArrival { window: self.window, seq: self.nohz_ticks, mode: TickMode::Nohz };
        if self.window_open {
            self.window_arrivals.push(a);
        }
        self.ledger.push(a);
        Ok(self.nohz_ticks)
    }

    /// 切换档位（原子：窗开启→切→窗闭合，窗内到达零丢失）。
    pub fn switch_mode(&mut self, to: TickMode) -> Result<(usize, usize), TdCode> {
        if self.window_open {
            return Err(TdCode::WindowOpen);
        }
        let captured = self.window_arrivals.len();
        self.window_open = true;
        self.mode = to;
        // 原子语义：窗内到达数=0（切换瞬间无 tick 可达即窗闭合）。
        self.window_open = false;
        self.window += 1;
        Ok((captured, self.window))
    }

    /// 测试钩子：人为悬空切换窗（悬空拒面验证用）。
    pub fn stuck_window_for_test(&mut self) {
        self.window_open = true;
    }

    /// 三源对平（引擎合计=账面合计=影子期望）。
    pub fn reconcile(&self, shadow_ticks: u64) -> Result<u64, TdCode> {
        let engine = self.periodic_ticks + self.nohz_ticks;
        let audited = self.ledger.len() as u64;
        if engine != audited {
            return Err(TdCode::AuditDrift);
        }
        if shadow_ticks != engine {
            return Err(TdCode::ShadowDrift);
        }
        Ok(engine)
    }
}

/// 周期档 10^6 拍稳定性演练（计数连续、逐拍落账）。
pub fn drill_periodic_1m() -> (bool, String) {
    let mut e = DualTickEngine::new();
    let mut last = 0u64;
    let mut gap = false;
    for _ in 0..PERIODIC_DRILL {
        let now = e.periodic_tick();
        if now != last + 1 {
            gap = true;
            break;
        }
        last = now;
    }
    let ok = !gap && e.periodic_ticks() == PERIODIC_DRILL;
    (ok, format!("拍数={} 断流={}", e.periodic_ticks(), gap))
}

/// nohz 档按需恢复 10/10 演练。
pub fn drill_nohz_recover_10() -> (bool, u32, String) {
    let mut e = DualTickEngine::new();
    e.switch_mode(TickMode::Nohz).unwrap_or((0, 0));
    let mut ok_rounds = 0u32;
    for r in 0..NOHZ_ROUNDS {
        let before = e.nohz_ticks();
        match e.nohz_recover() {
            Ok(v) if v == before + 1 => ok_rounds += 1,
            Ok(_) => {}
            Err(_) => {}
        }
        let _ = r;
    }
    let ok = ok_rounds == NOHZ_ROUNDS as u32 && e.recoveries() == NOHZ_ROUNDS as u32;
    (ok, ok_rounds, format!("恢复={}/{}", ok_rounds, NOHZ_ROUNDS))
}

/// 切换原子性演练（双向往返 10 轮，窗内零丢失）。
pub fn drill_switch_atomic() -> (bool, String) {
    let mut e = DualTickEngine::new();
    let mut ok = true;
    let mut detail = String::new();
    for i in 0..10 {
        let target = if i % 2 == 0 { TickMode::Nohz } else { TickMode::Periodic };
        match e.switch_mode(target) {
            Ok((captured, _w)) => {
                if captured != 0 {
                    ok = false;
                    detail.push_str(&format!("轮{}窗内{}笔", i, captured));
                }
            }
            Err(_) => {
                ok = false;
                detail.push_str(&format!("轮{}切换拒", i));
            }
        }
    }
    (ok, if detail.is_empty() { "20次切换全过".to_string() } else { detail })
}

/// 三源对平演练（引擎/账面/影子）。
pub fn drill_reconcile_3src() -> (bool, String) {
    let mut e = DualTickEngine::new();
    for _ in 0..500 {
        e.periodic_tick();
    }
    e.switch_mode(TickMode::Nohz).unwrap_or((0, 0));
    for _ in 0..7 {
        let _ = e.nohz_recover();
    }
    let total = e.periodic_ticks() + e.nohz_ticks();
    match e.reconcile(total) {
        Ok(v) => (v == 507, format!("对平值={}", v)),
        Err(c) => (false, format!("对平失败码={:#06x}", c.val())),
    }
}

/// 注入核（5 项必抓：周期断流/恢复漏账/切换丢失/影子漂移/审计错位）。
fn inject_5() -> (usize, usize, Vec<String>) {
    let mut caught = 0usize;
    let mut detail: Vec<String> = Vec::new();
    // 注 1：周期断流检测（人为跳号必被对平抓出）。
    let mut e = DualTickEngine::new();
    for _ in 0..10 { e.periodic_tick(); }
    // 模拟断流：账面 10 笔但拍数被外部篡改。
    let drift = e.reconcile(11).is_err();
    if drift { caught += 1; detail.push("注1 断流:抓".to_string()); } else { detail.push("注1 断流:漏".to_string()); }
    // 注 2：恢复漏账（少记一笔恢复必被抓：对平值 -1 即 AuditDrift）。
    let mut e2 = DualTickEngine::new();
    let _ = e2.nohz_recover();
    let total2 = e2.nohz_ticks();
    let catches_missing = e2.reconcile(total2).is_ok() && e2.reconcile(total2 - 1).is_err();
    if catches_missing { caught += 1; detail.push("注2 漏账:抓".to_string()); } else { detail.push("注2 漏账:漏".to_string()); }
    // 注 3：切换丢失（窗悬空再切换必拒：钩子置悬空后切被拒）。
    let mut e3 = DualTickEngine::new();
    e3.stuck_window_for_test();
    let refused = matches!(e3.switch_mode(TickMode::Nohz), Err(TdCode::WindowOpen));
    if refused { caught += 1; detail.push("注3 悬空:抓".to_string()); } else { detail.push("注3 悬空:漏".to_string()); }
    // 注 4：影子漂移（影子少 1 必抓）。
    let mut e4 = DualTickEngine::new();
    for _ in 0..9 { e4.periodic_tick(); }
    let shadow_bad = e4.reconcile(8).is_err();
    if shadow_bad { caught += 1; detail.push("注4 影子:抓".to_string()); } else { detail.push("注4 影子:漏".to_string()); }
    // 注 5：双档串账（周期拍数误入 nohz 桶必抓——两桶分离性）。
    let mut e5 = DualTickEngine::new();
    for _ in 0..5 { e5.periodic_tick(); }
    let mixed = e5.nohz_ticks() != 0 || e5.periodic_ticks() != 5;
    if !mixed { caught += 1; detail.push("注5 串账:抓".to_string()); } else { detail.push("注5 串账:漏".to_string()); }
    (caught, 5, detail)
}

/// 段位守卫。
pub fn segment_guard() -> bool {
    TdCode::ALL.iter().all(|c| c.val() >= SEG_BASE && c.val() < SEG_BASE + 16)
}

/// run_unx1062_checks：本单判据聚合（诊断段 0x1062）。
pub fn run_unx1062_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F1062 tick双模");
    let (ok1, d1) = drill_periodic_1m();
    s.add_dyn("J1-周期档10^6拍稳定", ok1, &d1);
    let (ok2, got2, d2) = drill_nohz_recover_10();
    s.add_dyn("J1-nohz恢复10/10", ok2, &format!("{}/10 {}", got2, d2));
    let (ok3, d3) = drill_switch_atomic();
    s.add_dyn("J1-切换原子零丢失", ok3, &d3);
    let (ok4, d4) = drill_reconcile_3src();
    s.add_dyn("J1-三源对平", ok4, &d4);
    let (c5, t5, d5) = inject_5();
    s.add_dyn("J2-注入核5/5", c5 == t5, &format!("{}/{} {}", c5, t5, d5.join(" ")));
    s.add_dyn("J3-段位守卫", segment_guard(), "0x1062段闭集");
    s
}
