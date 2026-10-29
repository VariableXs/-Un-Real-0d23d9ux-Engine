//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F1745 · idle 账目结算（时长账与 idle 位联动）· 0x364C0
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定）：
//!   ① 纯功能代码 ≥280 行；
//!   ② UNX-F1745-J1 Σ(idle)+Σ(busy)=上线总时长守恒断言全过，idle 位
//!      与 F1666 发现账双源一致；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - 双账模型：每核 idle_ticks 与 busy_ticks 两账，进入 idle 位清/
//!     置联动（idle_enter 置位 + 结算；idle_exit 清位 + busy 起算）；
//!   - 守恒恒等式：任意时刻 Σ(idle)+Σ(busy) == 上线总时长（uptime
//!     单调基准）；结算粒度 tick 全整数；
//!   - idle 位双源一致：源 A = 本账 idle 位；源 B = F1666 发现账建
//!     模副本（发现账按事件推导 idle 态）——两源逐核逐时刻对拍全同；
//!   - 越界防线：无上线先结算拒 / idle 双重进入拒 / 退出未进入拒 /
//!     uptime 回卷拒；
//!   - 注入面：守恒破账（外部篡改一账）/ 双重进入 / 未进退出 / 回
//!     卷 / 双源分歧 五族 10/10 必抓；
//!   - LCG 种子写死 = SEG_BASE（0x364C0），禁 rand/浮点/IO/墙钟；
//!     no_std + alloc；零 unwrap/expect。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x364C0，全仓独占）。
pub const SEG_BASE: u32 = 0x364C0;

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum IdleCode {
    /// 守恒破账：Σ(idle)+Σ(busy) ≠ uptime
    ConservationBroken,
    /// idle 双重进入
    DoubleEnter,
    /// 未进入就退出
    ExitWithoutEnter,
    /// uptime 回卷
    UptimeRegress,
    /// 双源分歧（本账 vs 发现账）
    DualSourceDiverge,
    /// 无上线先结算
    SettleBeforeOnline,
    /// 核号越界
    CoreOutOfRange,
    /// 负时长（end < start）
    NegativeDuration,
}

impl IdleCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            IdleCode::ConservationBroken => "守恒破账：Σ(idle)+Σ(busy) 必须恰等于上线总时长",
            IdleCode::DoubleEnter => "idle 双重进入：已处 idle 态再进入",
            IdleCode::ExitWithoutEnter => "未进先出：busy 结算无 idle 进入对应",
            IdleCode::UptimeRegress => "uptime 回卷：上线时长单调",
            IdleCode::DualSourceDiverge => "双源分歧：本账 idle 位与发现账不一致",
            IdleCode::SettleBeforeOnline => "无上线先结算：上线事件必须先行",
            IdleCode::CoreOutOfRange => "核号越界",
            IdleCode::NegativeDuration => "负时长：end < start",
        }
    }
    pub const ALL: [IdleCode; 8] = [
        IdleCode::ConservationBroken,
        IdleCode::DoubleEnter,
        IdleCode::ExitWithoutEnter,
        IdleCode::UptimeRegress,
        IdleCode::DualSourceDiverge,
        IdleCode::SettleBeforeOnline,
        IdleCode::CoreOutOfRange,
        IdleCode::NegativeDuration,
    ];
}

pub fn detail(code: u32) -> &'static str {
    for c in IdleCode::ALL {
        if c.val() == code {
            return c.detail();
        }
    }
    "未知诊断码"
}

// ── 常量 ─────────────────────────────────────────────────────────
/// 核数（消费 A1 拓扑 8 核建模）。
pub const CORES: usize = 8;

/// 每核双账实体。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct CoreIdle {
    pub core: usize,
    pub idle_ticks: u64,
    pub busy_ticks: u64,
    /// idle 位（源 A）。
    pub idle_flag: bool,
    /// 进入 idle 的时刻（None = 非 idle）。
    pub idle_since: Option<u64>,
    /// 上线时刻（None = 未上线）。
    pub online_at: Option<u64>,
}

/// idle 账总账。
pub struct IdleLedger {
    pub cores: Vec<CoreIdle>,
    /// 上线总时长（全局单调）。
    pub uptime: u64,
    pub rejected: usize,
}

impl IdleLedger {
    pub fn new() -> IdleLedger {
        let mut cores = Vec::new();
        for c in 0..CORES {
            cores.push(CoreIdle {
                core: c,
                idle_ticks: 0,
                busy_ticks: 0,
                idle_flag: false,
                idle_since: None,
                online_at: None,
            });
        }
        IdleLedger {
            cores,
            uptime: 0,
            rejected: 0,
        }
    }

    /// 上线事件（每核一次）。
    pub fn online(&mut self, core: usize, at: u64) -> Result<(), IdleCode> {
        if core >= CORES {
            return Err(IdleCode::CoreOutOfRange);
        }
        if at < self.uptime {
            return Err(IdleCode::UptimeRegress);
        }
        self.uptime = at;
        self.cores[core].online_at = Some(at);
        Ok(())
    }

    /// tick 推进：按 idle 位分流双账（结算核）。
    pub fn tick(&mut self, at: u64) -> Result<(), IdleCode> {
        if at < self.uptime {
            return Err(IdleCode::UptimeRegress);
        }
        let delta = at - self.uptime;
        if delta == 0 {
            return Ok(());
        }
        for c in &mut self.cores {
            match c.online_at {
                None => {} // 未上线不分账（上线前时长不计）
                Some(_) => {
                    if c.idle_flag {
                        c.idle_ticks += delta;
                    } else {
                        c.busy_ticks += delta;
                    }
                }
            }
        }
        self.uptime = at;
        Ok(())
    }

    /// 进入 idle（置位联动）。
    pub fn idle_enter(&mut self, core: usize, at: u64) -> Result<(), IdleCode> {
        if core >= CORES {
            return Err(IdleCode::CoreOutOfRange);
        }
        if self.cores[core].online_at.is_none() {
            self.rejected += 1;
            return Err(IdleCode::SettleBeforeOnline);
        }
        if self.cores[core].idle_flag {
            self.rejected += 1;
            return Err(IdleCode::DoubleEnter);
        }
        if at < self.uptime {
            return Err(IdleCode::UptimeRegress);
        }
        self.cores[core].idle_flag = true;
        self.cores[core].idle_since = Some(at);
        Ok(())
    }

    /// 退出 idle（清位 + busy 恢复）。
    pub fn idle_exit(&mut self, core: usize, at: u64) -> Result<(), IdleCode> {
        if core >= CORES {
            return Err(IdleCode::CoreOutOfRange);
        }
        if !self.cores[core].idle_flag {
            self.rejected += 1;
            return Err(IdleCode::ExitWithoutEnter);
        }
        match self.cores[core].idle_since {
            Some(s) if at >= s => {}
            _ => {
                self.rejected += 1;
                return Err(IdleCode::NegativeDuration);
            }
        }
        self.cores[core].idle_flag = false;
        self.cores[core].idle_since = None;
        Ok(())
    }

    /// 守恒恒等式：Σ(idle)+Σ(busy) == Σ核上线时长（逐核口径：
    /// uptime - online_at ≥ idle+busy，结算边界内相等）。
    pub fn conservation(&self) -> bool {
        let mut sum = 0u64;
        for c in &self.cores {
            sum += c.idle_ticks + c.busy_ticks;
        }
        // 上线总时长 = Σ核 (uptime - online_at)。
        let mut expect = 0u64;
        for c in &self.cores {
            if let Some(o) = c.online_at {
                expect += self.uptime - o;
            }
        }
        sum == expect
    }

    /// 双源一致：本账 idle 位 vs F1666 发现账（事件推导副本）。
    pub fn dual_source_check(&self, discovery_flags: &[bool; CORES]) -> bool {
        for (i, c) in self.cores.iter().enumerate() {
            if c.idle_flag != discovery_flags[i] {
                return false;
            }
        }
        true
    }

    pub fn summary_line(&self) -> String {
        let mut idle_sum = 0u64;
        let mut busy_sum = 0u64;
        for c in &self.cores {
            idle_sum += c.idle_ticks;
            busy_sum += c.busy_ticks;
        }
        format!(
            "idle 结算: Σidle={} Σbusy={} 守恒={} uptime={}",
            idle_sum,
            busy_sum,
            self.conservation(),
            self.uptime
        )
    }
}

// ── LCG 注入核（种子写死 = SEG_BASE） ───────────────────────────
pub struct Lcg {
    state: u64,
}

impl Lcg {
    pub fn new(seed: u64) -> Lcg {
        Lcg { state: seed }
    }
    pub fn next(&mut self) -> u64 {
        self.state = self
            .state
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        self.state
    }
    pub fn next_below(&mut self, n: u64) -> u64 {
        self.next() % n
    }
}

// ── 守恒断言（判据 J1 主面） ─────────────────────────────────────
pub fn drill_conservation_10() -> (bool, String) {
    let mut lcg = Lcg::new(SEG_BASE as u64);
    let mut caught = 0;
    for round in 0..10u32 {
        let mut led = IdleLedger::new();
        let mut ok = true;
        // 全核上线。
        for c in 0..CORES {
            ok = ok && led.online(c, 10).is_ok();
        }
        // 随机 idle/exit 循环 + tick 推进。
        let mut t = 10u64;
        for _ in 0..16 {
            let c = (lcg.next_below(CORES as u64)) as usize;
            t += 1 + lcg.next_below(5);
            if led.cores[c].idle_flag {
                ok = ok && led.idle_exit(c, t).is_ok();
            } else {
                ok = ok && led.idle_enter(c, t).is_ok();
            }
            let _ = round;
            ok = ok && led.tick(t + 1).is_ok();
        }
        ok = ok && led.conservation();
        if ok {
            caught += 1;
        }
    }
    let ok = caught == 10;
    (ok, format!("守恒断言 {}/10 轮", caught))
}

fn drill_double_enter_10() -> (bool, String) {
    let mut caught = 0;
    for _ in 0..10 {
        let mut led = IdleLedger::new();
        for c in 0..CORES {
            let _ = led.online(c, 0);
        }
        let _ = led.idle_enter(0, 1);
        let r = led.idle_enter(0, 2);
        if r == Err(IdleCode::DoubleEnter) {
            caught += 1;
        }
    }
    let ok = caught == 10;
    (ok, format!("双重进入注入 {}/10 必抓", caught))
}

fn drill_exit_without_enter_10() -> (bool, String) {
    let mut caught = 0;
    for i in 0..10u32 {
        let mut led = IdleLedger::new();
        for c in 0..CORES {
            let _ = led.online(c, 0);
        }
        let r = led.idle_exit(i as usize % CORES, 1);
        if r == Err(IdleCode::ExitWithoutEnter) {
            caught += 1;
        }
    }
    let ok = caught == 10;
    (ok, format!("未进先出注入 {}/10 必抓", caught))
}

fn drill_uptime_regress_10() -> (bool, String) {
    let mut caught = 0;
    for _ in 0..10 {
        let mut led = IdleLedger::new();
        let _ = led.online(0, 100);
        let r1 = led.online(1, 99); // 回卷
        let r2 = led.tick(50); // tick 回卷
        if r1 == Err(IdleCode::UptimeRegress) && r2 == Err(IdleCode::UptimeRegress) {
            caught += 1;
        }
    }
    let ok = caught == 10;
    (ok, format!("uptime 回卷注入 {}/10 必抓", caught))
}

fn drill_settle_before_online_10() -> (bool, String) {
    let mut caught = 0;
    for _ in 0..10 {
        let mut led = IdleLedger::new();
        let r = led.idle_enter(3, 5); // 未上线先进入
        if r == Err(IdleCode::SettleBeforeOnline) {
            caught += 1;
        }
    }
    let ok = caught == 10;
    (ok, format!("无上线先结算注入 {}/10 必抓", caught))
}

fn drill_dual_source_consistent_10() -> (bool, String) {
    // 双源一致 10/10 轮：发现账副本随事件推导，逐核对拍。
    let mut lcg = Lcg::new(SEG_BASE as u64 + 1);
    let mut caught = 0;
    for _ in 0..10 {
        let mut led = IdleLedger::new();
        let mut discovery = [false; CORES];
        let mut ok = true;
        for c in 0..CORES {
            let _ = led.online(c, 0);
        }
        for _ in 0..20 {
            let c = (lcg.next_below(CORES as u64)) as usize;
            if discovery[c] {
                let _ = led.idle_exit(c, led.uptime + 1);
            } else {
                let _ = led.idle_enter(c, led.uptime + 1);
            }
            discovery[c] = !discovery[c];
            let _ = led.tick(led.uptime + 1);
        }
        ok = ok && led.dual_source_check(&discovery);
        if ok {
            caught += 1;
        }
    }
    let ok = caught == 10;
    (ok, format!("双源一致 {}/10 轮", caught))
}

fn drill_dual_source_diverge_detected() -> (bool, String) {
    // 双源分歧注入：发现账翻转一位 → 对拍必抓。
    let mut led = IdleLedger::new();
    for c in 0..CORES {
        let _ = led.online(c, 0);
    }
    let _ = led.idle_enter(2, 1);
    let mut discovery = [false; CORES];
    discovery[2] = true;
    let ok_before = led.dual_source_check(&discovery);
    discovery[2] = false; // 注入分歧
    let caught = !led.dual_source_check(&discovery);
    (ok_before && caught, format!("双源分歧注入: 前={} 抓={}", ok_before, caught))
}

fn drill_conservation_broken_detected() -> (bool, String) {
    // 守恒破账注入：外部篡改 busy_ticks → 守恒器必抓。
    let mut led = IdleLedger::new();
    for c in 0..CORES {
        let _ = led.online(c, 0);
    }
    let _ = led.tick(100);
    let ok_before = led.conservation();
    led.cores[0].busy_ticks += 1; // 篡改
    let caught = !led.conservation();
    (ok_before && caught, format!("守恒破账注入: 前={} 抓={}", ok_before, caught))
}

fn drill_core_range_10() -> (bool, String) {
    let mut lcg = Lcg::new(SEG_BASE as u64 + 2);
    let mut caught = 0;
    for _ in 0..10 {
        let mut led = IdleLedger::new();
        let bad = CORES + (lcg.next_below(8) as usize);
        let r1 = led.idle_enter(bad, 1);
        let r2 = led.idle_exit(bad, 1);
        let r3 = led.online(bad, 1);
        if r1 == Err(IdleCode::CoreOutOfRange)
            && r2 == Err(IdleCode::CoreOutOfRange)
            && r3 == Err(IdleCode::CoreOutOfRange)
        {
            caught += 1;
        }
    }
    let ok = caught == 10;
    (ok, format!("核号越界注入 {}/10 必抓", caught))
}

fn drill_negative_duration_10() -> (bool, String) {
    let mut caught = 0;
    for _ in 0..10 {
        let mut led = IdleLedger::new();
        for c in 0..CORES {
            let _ = led.online(c, 0);
        }
        let _ = led.idle_enter(1, 100);
        let r = led.idle_exit(1, 50); // end < start
        if r == Err(IdleCode::NegativeDuration) {
            caught += 1;
        }
    }
    let ok = caught == 10;
    (ok, format!("负时长注入 {}/10 必抓", caught))
}

fn drill_offline_not_counted() -> (bool, String) {
    // 未上线核时长不计入（上线前零分账）。
    let mut led = IdleLedger::new();
    let _ = led.online(0, 10);
    let _ = led.tick(50);
    let c0 = &led.cores[0];
    let c1 = &led.cores[1];
    let ok = c0.busy_ticks == 40 && c1.busy_ticks == 0 && c1.idle_ticks == 0 && led.conservation();
    (ok, format!("未上线零分账: 核0busy={} 核1零账={} 守恒={}", c0.busy_ticks, c1.busy_ticks, led.conservation()))
}

fn drill_idle_cycle_accounting() -> (bool, String) {
    // idle 周期记账语义：进入后 tick 分流 idle 账，退出后回 busy。
    let mut led = IdleLedger::new();
    for c in 0..CORES {
        let _ = led.online(c, 0);
    }
    let _ = led.idle_enter(5, 10);
    let _ = led.tick(30); // 20 ticks idle
    let _ = led.idle_exit(5, 30);
    let _ = led.tick(40); // 10 ticks busy
    let c5 = &led.cores[5];
    let ok = c5.idle_ticks == 20 && c5.busy_ticks == 10 && led.conservation();
    (ok, format!("idle 周期记账: idle={} busy={} 守恒={}", c5.idle_ticks, c5.busy_ticks, led.conservation()))
}

// ── 段位守卫 ─────────────────────────────────────────────────────
pub fn segment_guard() -> bool {
    IdleCode::ALL.iter().all(|c| c.val() >= SEG_BASE && c.val() < SEG_BASE + 16)
}

// ── 全流程对账 ───────────────────────────────────────────────────
pub fn drill_full_flow() -> (bool, String) {
    let (a1, d1) = drill_conservation_10();
    let (a2, d2) = drill_dual_source_consistent_10();
    let (a3, d3) = drill_idle_cycle_accounting();
    let ok = a1 && a2 && a3;
    (ok, format!("全流程: 守恒={} 双源={} 周期记账={} [{}|{}|{}]", a1, a2, a3, d1, d2, d3))
}

// ── 判据聚合 ─────────────────────────────────────────────────────
pub fn extra_checks() -> Vec<(&'static str, bool, String)> {
    let mut v: Vec<(&'static str, bool, String)> = Vec::new();
    let (a, b) = drill_conservation_10();
    v.push(("J1-守恒断言10/10", a, b));
    let (a, b) = drill_double_enter_10();
    v.push(("J2-双重进入注入10/10", a, b));
    let (a, b) = drill_exit_without_enter_10();
    v.push(("J2-未进先出注入10/10", a, b));
    let (a, b) = drill_uptime_regress_10();
    v.push(("J2-uptime回卷注入10/10", a, b));
    let (a, b) = drill_settle_before_online_10();
    v.push(("J2-无上线先结算注入10/10", a, b));
    let (a, b) = drill_dual_source_consistent_10();
    v.push(("J1-双源一致10/10", a, b));
    let (a, b) = drill_dual_source_diverge_detected();
    v.push(("J2-双源分歧注入抓", a, b));
    let (a, b) = drill_conservation_broken_detected();
    v.push(("J2-守恒破账注入抓", a, b));
    let (a, b) = drill_core_range_10();
    v.push(("J2-核号越界注入10/10", a, b));
    let (a, b) = drill_negative_duration_10();
    v.push(("J2-负时长注入10/10", a, b));
    let (a, b) = drill_offline_not_counted();
    v.push(("J3-未上线零分账", a, b));
    let (a, b) = drill_idle_cycle_accounting();
    v.push(("J3-idle周期记账", a, b));
    let (a, b) = drill_full_flow();
    v.push(("J1-全流程对账", a, b));
    v
}

/// run_unx1745_checks：本单判据聚合（诊断段 0x364C0）。
pub fn run_unx1745_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F1745 idle 账目结算");
    s.add_dyn("J3-段位守卫", segment_guard(), "0x364C0 段闭集");
    for (n, p, d) in extra_checks() {
        s.add_dyn(n, p, &d);
    }
    s
}
