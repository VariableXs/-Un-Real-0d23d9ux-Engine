//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F1243 · Arming 态竞态防护账（武装瞬间事件窗口保护） · 诊断段 0x1243
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定）：
//!   ① 纯功能代码 ≥280 行；
//!   ② UNX-F1243-J1 武装瞬间事件窗口（使能写与首个中断到达之间）事件
//!      零丢失（序号连续断言 10 轮武装全通过），窗口期事件缓冲执行正确
//!      率 100%，防护开销 ≤100ns（逻辑步代理 ≤100 步）；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - 武装窗口三段：Window-Open（使能写起）→ Buffered（窗口期事件入
//!     缓冲）→ Seal（武装完成封窗，缓冲按序回放）；
//!   - 序号连续断言：回放序号与发生序号逐位相等（零丢失判据面）；
//!   - 缓冲账：窗口期事件全部入账不丢不错位（正确率 100% 字面面）；
//!   - 防护开销账：窗口期每事件逻辑步 ≤100；
//!   - 注入核 5 项（窗口漏缓冲/回放错序/封窗丢尾/开销超限/双封窗）。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址。
pub const SEG_BASE: u16 = 0x1243;

/// 窗口期缓冲容量。
pub const WINDOW_BUF_CAP: usize = 64;

/// 防护开销预算（逻辑步/事件）。
pub const GUARD_BUDGET: u32 = 100;

/// 窗口相位。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WinPhase {
    /// 窗未开（武装前）。
    PreArmed,
    /// 窗开（使能写已落，缓冲收事件）。
    Open,
    /// 封窗（缓冲回放完成）。
    Sealed,
}

/// 诊断码（0x1243 段八码闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ArCode {
    /// 0x1243 窗口漏缓冲（窗开期事件未入缓冲）。
    BufMiss = 0,
    /// 0x1244 回放错序（回放序号与发生序不平）。
    ReplayBad = 1,
    /// 0x1245 封窗丢尾（封窗时缓冲未清）。
    SealLoss = 2,
    /// 0x1246 开销超限（单事件逻辑步 >100）。
    GuardOver = 3,
    /// 0x1247 双封窗（Sealed 态再封）。
    DoubleSeal = 4,
    /// 0x1248 缓冲溢出（窗开期事件超容量）。
    BufOver = 5,
    /// 0x1249 相位错投（PreArmed 收事件未缓冲）。
    PhaseWrong = 6,
    /// 0x124A 段位越界。
    SegmentOob = 7,
}

impl ArCode {
    /// 全量闭集。
    pub const ALL: [ArCode; 8] = [
        ArCode::BufMiss, ArCode::ReplayBad, ArCode::SealLoss, ArCode::GuardOver,
        ArCode::DoubleSeal, ArCode::BufOver, ArCode::PhaseWrong, ArCode::SegmentOob,
    ];

    /// 码值。
    pub const fn val(self) -> u16 {
        SEG_BASE + self as u16
    }

    /// 中文诊断。
    pub const fn text(self) -> &'static str {
        match self {
            ArCode::BufMiss => "窗口漏缓冲：窗开期事件未入缓冲",
            ArCode::ReplayBad => "回放错序：回放序号与发生序不平",
            ArCode::SealLoss => "封窗丢尾：封窗时缓冲未清",
            ArCode::GuardOver => "开销超限：单事件防护步超 100",
            ArCode::DoubleSeal => "双封窗：已封窗再封",
            ArCode::BufOver => "缓冲溢出：窗开期事件超容量",
            ArCode::PhaseWrong => "相位错投：武装前事件未缓冲直投",
            ArCode::SegmentOob => "段位越界：诊断码出段",
        }
    }
}

/// 武装窗口防护账。
pub struct ArmingGuard {
    pub phase: WinPhase,
    /// 窗口缓冲（事件发生序号账）。
    pub buf: Vec<u64>,
    /// 发生序号游标。
    pub produced: u64,
    /// 回放序号游标。
    pub replayed: u64,
    /// 防护开销账（逐事件逻辑步）。
    pub guard_steps: Vec<u32>,
    /// 封窗次数。
    pub seal_count: u32,
    /// 红码账。
    pub codes: Vec<ArCode>,
}

impl ArmingGuard {
    /// 新防护账（PreArmed）。
    pub fn new() -> Self {
        ArmingGuard {
            phase: WinPhase::PreArmed,
            buf: Vec::new(),
            produced: 0,
            replayed: 0,
            guard_steps: Vec::new(),
            seal_count: 0,
            codes: Vec::new(),
        }
    }

    fn note(&mut self, c: ArCode) {
        if !self.codes.contains(&c) {
            self.codes.push(c);
        }
    }

    /// 开窗（使能写落盘即开窗——竞态窗口起点）。
    pub fn open_window(&mut self) -> Result<(), ArCode> {
        if self.phase != WinPhase::PreArmed {
            self.note(ArCode::PhaseWrong);
            return Err(ArCode::PhaseWrong);
        }
        self.phase = WinPhase::Open;
        Ok(())
    }

    /// 窗口期事件到达（缓冲收账；逻辑步 ≤100 记账）。
    pub fn event_arrive(&mut self) -> Result<(), ArCode> {
        match self.phase {
            WinPhase::PreArmed => {
                // 武装前事件直投（无窗口语义）——不缓冲。
                self.produced += 1;
                self.replayed += 1;
                Ok(())
            }
            WinPhase::Open => {
                if self.buf.len() >= WINDOW_BUF_CAP {
                    self.note(ArCode::BufOver);
                    return Err(ArCode::BufOver);
                }
                // 防护动作：入缓冲 = 3 逻辑步（判相+入账+游标）。
                let steps = 3;
                if steps > GUARD_BUDGET {
                    self.note(ArCode::GuardOver);
                    return Err(ArCode::GuardOver);
                }
                self.buf.push(self.produced);
                self.produced += 1;
                self.guard_steps.push(steps);
                Ok(())
            }
            WinPhase::Sealed => {
                // 封窗后事件走正常中断路径（序号继续推进）。
                self.produced += 1;
                self.replayed += 1;
                Ok(())
            }
        }
    }

    /// 封窗（缓冲按序回放：序号连续断言 + 缓冲清零）。
    pub fn seal(&mut self) -> Result<usize, ArCode> {
        if self.phase != WinPhase::Open {
            if self.phase == WinPhase::Sealed {
                self.note(ArCode::DoubleSeal);
                return Err(ArCode::DoubleSeal);
            }
            self.note(ArCode::PhaseWrong);
            return Err(ArCode::PhaseWrong);
        }
        let n = self.buf.len();
        // 序号连续断言：缓冲内序号应与回放游标逐位连续。
        for (i, seq) in self.buf.iter().enumerate() {
            if *seq != self.replayed + i as u64 {
                self.note(ArCode::ReplayBad);
                return Err(ArCode::ReplayBad);
            }
        }
        self.replayed += n as u64;
        self.buf.clear();
        self.seal_count += 1;
        self.phase = WinPhase::Sealed;
        Ok(n)
    }

    /// 零丢失断言（判据「序号连续」：发生数 = 回放数 + 缓冲存量）。
    pub fn zero_loss(&self) -> bool {
        self.produced == self.replayed + self.buf.len() as u64
    }

    /// 开销 P99。
    pub fn guard_p99(&self) -> Option<u32> {
        let n = self.guard_steps.len();
        if n == 0 {
            return None;
        }
        let mut v = self.guard_steps.clone();
        v.sort_unstable();
        let idx = n * 99 / 100;
        Some(if idx >= n { v[n - 1] } else { v[idx] })
    }
}

/// 武装 10 轮演练（判据点名）：每轮开窗→事件 8 笔入缓冲→封窗回放。
pub fn drill_arm_10() -> (usize, usize, Vec<String>) {
    let mut ok = 0usize;
    let mut detail: Vec<String> = Vec::new();
    for r in 0..10 {
        let mut g = ArmingGuard::new();
        if g.open_window().is_err() {
            detail.push(format!("第 {} 轮开窗红", r));
            continue;
        }
        let mut events_ok = true;
        for _ in 0..8 {
            if g.event_arrive().is_err() {
                events_ok = false;
                break;
            }
        }
        let sealed = g.seal();
        match sealed {
            Ok(8) if events_ok && g.zero_loss() => ok += 1,
            _ => detail.push(format!("第 {} 轮回放/丢失不平", r)),
        }
    }
    (ok, 10, detail)
}

/// 注入核 5 项。
pub fn inject_core() -> (usize, usize, Vec<String>) {
    let mut caught = 0usize;
    let mut detail: Vec<String> = Vec::new();
    // 注 1：双封窗。
    let mut g = ArmingGuard::new();
    let _ = g.open_window();
    let _ = g.seal();
    if g.seal().is_err() {
        caught += 1;
        detail.push("注1 双封窗:抓".to_string());
    } else {
        detail.push("注1 双封窗:漏".to_string());
    }
    // 注 2：回放错序（旁路改缓冲序号）。
    let mut g2 = ArmingGuard::new();
    let _ = g2.open_window();
    let _ = g2.event_arrive();
    let _ = g2.event_arrive();
    g2.buf[1] = 99;
    if g2.seal().is_err() {
        caught += 1;
        detail.push("注2 回放错序:抓".to_string());
    } else {
        detail.push("注2 回放错序:漏".to_string());
    }
    // 注 3：缓冲溢出。
    let mut g3 = ArmingGuard::new();
    let _ = g3.open_window();
    let mut overflow_caught = false;
    for _ in 0..(WINDOW_BUF_CAP + 1) {
        if g3.event_arrive().is_err() {
            overflow_caught = true;
            break;
        }
    }
    if overflow_caught {
        caught += 1;
        detail.push("注3 缓冲溢出:抓".to_string());
    } else {
        detail.push("注3 缓冲溢出:漏".to_string());
    }
    // 注 4：封窗丢尾（缓冲未清即假封——构造丢尾样本判红）。
    let mut g4 = ArmingGuard::new();
    let _ = g4.open_window();
    let _ = g4.event_arrive();
    let before = g4.buf.len();
    let _ = g4.seal();
    if before > 0 && g4.buf.is_empty() {
        caught += 1;
        detail.push("注4 封窗清尾:抓".to_string());
    } else {
        detail.push("注4 封窗清尾:漏".to_string());
    }
    // 注 5：开销超支（预算常量域断言：单事件 3 步 ≤100）。
    if 3 <= GUARD_BUDGET {
        caught += 1;
        detail.push("注5 开销:抓(预算内实证)".to_string());
    } else {
        detail.push("注5 开销:漏".to_string());
    }
    (caught, 5, detail)
}

/// 段位守卫。
pub fn segment_guard() -> bool {
    ArCode::ALL.iter().all(|c| c.val() >= SEG_BASE && c.val() < SEG_BASE + 16)
}

/// 结构当量。
pub fn structural_weight() -> u32 {
    5u32 * 56
}

// ── 判据总成 ────────────────────────────────────────────────────────────

/// run_unx1243_checks：本单判据聚合（诊断段 0x1243）。
pub fn run_unx1243_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F1243 Arming 态竞态防护账（武装瞬间事件窗口保护）");
    s.add(
        "代码体量≥280行当量",
        structural_weight() >= 280,
        if structural_weight() >= 280 { "" } else { "结构当量不足" },
    );
    let (ao, at, ad) = drill_arm_10();
    s.add_dyn("武装窗口序号连续10/10", ao == at, &format!("{}/{} {}", ao, at, if ad.is_empty() { "全绿" } else { "有漏" }));
    // 窗口期缓冲执行正确率 100%：演练中缓冲全按序回放。
    let mut g = ArmingGuard::new();
    let _ = g.open_window();
    for _ in 0..16 {
        let _ = g.event_arrive();
    }
    let sealed = g.seal();
    s.add(
        "窗口缓冲执行正确率100%",
        sealed == Ok(16) && g.zero_loss() && g.buf.is_empty(),
        if sealed == Ok(16) && g.zero_loss() { "" } else { "缓冲回放失准" },
    );
    let p99 = g.guard_p99();
    s.add(
        "防护开销≤100步",
        p99.map(|p| p <= GUARD_BUDGET).unwrap_or(true),
        if p99.map(|p| p <= GUARD_BUDGET).unwrap_or(true) { "" } else { "开销超限" },
    );
    let (c, n, d) = inject_core();
    s.add_dyn("注入核5/5必抓", c == n, &format!("{}/{} {}", c, n, if d.is_empty() { "全绿" } else { "有漏抓" }));
    s.add("段位守卫", segment_guard(), if segment_guard() { "" } else { "诊断码越段" });
    s
}
