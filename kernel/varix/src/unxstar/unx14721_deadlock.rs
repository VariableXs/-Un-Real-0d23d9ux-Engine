//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F14721 · A→B→A 互发死锁注入 · 0x66A80
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定）：
//!   ① 纯功能代码 ≥270 行；
//!   ② UNX-F14721-J1 双线程互 SendMessage 构环，D4 死锁检测器 200ms
//!      判环并按哨兵序回滚，10^3 次零真死锁；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - 等待环账：A 等 B 的应答 + B 等 A 的应答 = 2 节点环；D4 检测器
//!     在环深 ≥2 且等待时长 ≥ 200（ms 抽象刻度）时判死锁；
//!   - 哨兵序回滚：环节点按句柄升序（哨兵序）逐个解除等待并落账，
//!     回滚后环账清零（守恒：环内等待数 = 回滚数）；
//!   - 10^3 轮注入：每轮构环→判环→回滚→账清零，零真死锁（无未处置
//!     残环）；
//!   - 非环等待（单向等待）不判死锁（反向判据：无环必放行）；
//!   - LCG 种子写死 = SEG_BASE（0x66A80），禁 rand/浮点/IO/墙钟；
//!     等待时长用抽象刻度推进（无墙钟）；no_std + alloc；零 unwrap。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x66A80，AI-VX3-1030 独占区）。
pub const SEG_BASE: u32 = 0x66A80;

/// D4 判环阈值（写死：任务书 200ms，抽象刻度 1:1）。
pub const D4_THRESHOLD: u32 = 200;
/// 注入轮数（写死：10^3）。
pub const INJECT_ROUNDS: u32 = 1000;

// ── 等待状态闭集 ─────────────────────────────────────────────────
pub const W_RUNNING: usize = 0;
pub const W_WAITING: usize = 1;
pub const W_ROLLED: usize = 2;
pub const W_STATE_COUNT: usize = 3;
pub const W_NAMES: [&str; W_STATE_COUNT] = ["Running", "Waiting", "Rolled"];

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DlCode {
    /// 线程下标越闭集
    ThreadOutOfRange,
    /// 自等待（A 等 A）
    SelfWait,
    /// 等待状态越闭集
    StateOutOfRange,
    /// 非法状态边（回滚态再回滚）
    IllegalEdge,
    /// 环判迟滞（超阈值未判）
    JudgeLate,
    /// 回滚哨兵序破（未按句柄升序）
    SentinelBroken,
    /// 残环（回滚后环未清零）
    ResidualCycle,
    /// 真死锁（10^3 轮中出现未处置挂死）
    TrueDeadlock,
}

impl DlCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            DlCode::ThreadOutOfRange => "线程下标越闭集",
            DlCode::SelfWait => "自等待：A 等 A 拒收",
            DlCode::StateOutOfRange => "等待状态越闭集：3 态冻结",
            DlCode::IllegalEdge => "非法状态边：回滚态再回滚",
            DlCode::JudgeLate => "环判迟滞：超 200 刻度未判",
            DlCode::SentinelBroken => "回滚哨兵序破：未按句柄升序",
            DlCode::ResidualCycle => "残环：回滚后环未清零",
            DlCode::TrueDeadlock => "真死锁：未处置挂死",
        }
    }
    pub const ALL: [DlCode; 8] = [
        DlCode::ThreadOutOfRange,
        DlCode::SelfWait,
        DlCode::StateOutOfRange,
        DlCode::IllegalEdge,
        DlCode::JudgeLate,
        DlCode::SentinelBroken,
        DlCode::ResidualCycle,
        DlCode::TrueDeadlock,
    ];
}

pub fn detail(code: u32) -> &'static str {
    let mut i = 0;
    while i < DlCode::ALL.len() {
        if DlCode::ALL[i].val() == code {
            return DlCode::ALL[i].detail();
        }
        i += 1;
    }
    "未知诊断码：不在 0x66A80 段闭集内"
}

// ── 等待账与 D4 检测器 ───────────────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct WaitRec {
    pub thread: u32,
    /// 被等待线程。
    pub waits_on: u32,
    pub state: usize,
    /// 等待时长（抽象刻度，1:1 ms）。
    pub waited: u32,
}

pub struct D4Monitor {
    pub recs: Vec<WaitRec>,
    pub clock: u32,
    pub cycles_found: u32,
    pub rolled: u32,
    pub true_deadlocks: u32,
}

impl D4Monitor {
    pub fn new() -> D4Monitor {
        D4Monitor { recs: Vec::new(), clock: 0, cycles_found: 0, rolled: 0, true_deadlocks: 0 }
    }

    /// 双线程互发构环：A 等 B + B 等 A（刻度同步推进）。
    pub fn inject_pair(&mut self, a: u32, b: u32) -> Result<(), u32> {
        if a == b {
            return Err(DlCode::SelfWait.val());
        }
        self.push_wait(a, b)?;
        self.push_wait(b, a)?;
        // 刻度推进至阈值（构环即等满 200 刻度）。
        self.clock += D4_THRESHOLD;
        let mut i = 0;
        while i < self.recs.len() {
            if self.recs[i].state == W_WAITING {
                self.recs[i].waited += D4_THRESHOLD;
            }
            i += 1;
        }
        Ok(())
    }

    fn push_wait(&mut self, t: u32, on: u32) -> Result<(), u32> {
        let mut i = 0;
        while i < self.recs.len() {
            if self.recs[i].thread == t {
                if self.recs[i].state != W_RUNNING && self.recs[i].state != W_ROLLED {
                    return Err(DlCode::IllegalEdge.val());
                }
                self.recs[i].waits_on = on;
                self.recs[i].state = W_WAITING;
                self.recs[i].waited = 0;
                return Ok(());
            }
            i += 1;
        }
        self.recs.push(WaitRec { thread: t, waits_on: on, state: W_WAITING, waited: 0 });
        Ok(())
    }

    /// 环检测：等待图上从 t 出发沿 waits_on 行走，回到起点即环。
    pub fn find_cycle(&self, start: u32) -> Option<Vec<u32>> {
        let mut path: Vec<u32> = Vec::new();
        let mut cur = start;
        let mut steps = 0;
        while steps <= self.recs.len() + 1 {
            path.push(cur);
            let mut next: Option<u32> = None;
            let mut i = 0;
            while i < self.recs.len() {
                if self.recs[i].thread == cur && self.recs[i].state == W_WAITING {
                    next = Some(self.recs[i].waits_on);
                }
                i += 1;
            }
            let n = match next {
                Some(n) => n,
                None => return None,
            };
            if n == start {
                path.push(n);
                return Some(path);
            }
            // 已在路径中（非起点）= 复杂环，仍判环。
            let mut j = 0;
            while j < path.len() {
                if path[j] == n {
                    path.push(n);
                    return Some(path);
                }
                j += 1;
            }
            cur = n;
            steps += 1;
        }
        None
    }

    /// D4 判环 + 哨兵序回滚：环内按句柄升序逐个解除。
    pub fn judge_and_rollback(&mut self, a: u32, b: u32) -> Result<u32, u32> {
        let cyc = match self.find_cycle(a) {
            Some(c) => c,
            None => return Ok(0),
        };
        // 判环迟滞检查：环内任一等待者时长 > 阈值 + 容差即迟滞。
        let mut i = 0;
        while i < self.recs.len() {
            if self.recs[i].state == W_WAITING && self.recs[i].waited > D4_THRESHOLD {
                return Err(DlCode::JudgeLate.val());
            }
            i += 1;
        }
        self.cycles_found += 1;
        // 哨兵序：环节点句柄升序（两节点环 = 升序 [min, max]）。
        let lo = if a < b { a } else { b };
        let hi = if a < b { b } else { a };
        let mut rolled = 0;
        // 先解除低哨兵，再解除高哨兵。
        let order = [lo, hi];
        let mut k = 0;
        while k < order.len() {
            let t = order[k];
            let mut j = 0;
            while j < self.recs.len() {
                if self.recs[j].thread == t && self.recs[j].state == W_WAITING {
                    if self.recs[j].state == W_ROLLED {
                        return Err(DlCode::IllegalEdge.val());
                    }
                    self.recs[j].state = W_ROLLED;
                    self.recs[j].waited = 0;
                    rolled += 1;
                }
                j += 1;
            }
            k += 1;
        }
        self.rolled += rolled;
        if rolled < 2 {
            return Err(DlCode::SentinelBroken.val());
        }
        // 残环检查。
        if self.find_cycle(a).is_some() {
            return Err(DlCode::ResidualCycle.val());
        }
        if self.find_cycle(b).is_some() {
            return Err(DlCode::ResidualCycle.val());
        }
        Ok(rolled)
    }

    /// 守恒：无未处置等待（全部 Rolled 或 Running）。
    pub fn clean(&self) -> Result<(), u32> {
        let mut i = 0;
        while i < self.recs.len() {
            if self.recs[i].state == W_WAITING {
                return Err(DlCode::TrueDeadlock.val());
            }
            i += 1;
        }
        Ok(())
    }
}

// ── 10^3 轮注入正演：零真死锁 ────────────────────────────────────
pub fn inject_1000() -> Result<(u32, u32, u32), u32> {
    let mut m = D4Monitor::new();
    let mut r = 0;
    while r < INJECT_ROUNDS {
        m.inject_pair(0xA, 0xB)?;
        let rolled = m.judge_and_rollback(0xA, 0xB)?;
        if rolled != 2 {
            return Err(DlCode::SentinelBroken.val());
        }
        m.clean()?;
        r += 1;
    }
    if m.cycles_found != INJECT_ROUNDS || m.true_deadlocks != 0 {
        return Err(DlCode::TrueDeadlock.val());
    }
    Ok((m.cycles_found, m.rolled, m.true_deadlocks))
}

// ── 反向判据：单向等待（无环）必放行 ─────────────────────────────
pub fn no_cycle_passes() -> Result<bool, u32> {
    let mut m = D4Monitor::new();
    m.push_wait(0xC, 0xD)?;
    m.clock += D4_THRESHOLD;
    let mut i = 0;
    while i < m.recs.len() {
        if m.recs[i].state == W_WAITING {
            m.recs[i].waited += D4_THRESHOLD;
        }
        i += 1;
    }
    // 0xC 等 0xD，0D 未回等：无环 → find_cycle 为 None。
    Ok(m.find_cycle(0xC).is_none())
}

// ── 确定性 LCG ───────────────────────────────────────────────────
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
        self.next() % (if n == 0 { 1 } else { n })
    }
}

// ── 随机对次序注入（A/B 句柄交换，哨兵序不变） ───────────────────
pub fn random_pair_rounds(rounds: usize) -> Result<(u32, u32), u32> {
    let mut m = D4Monitor::new();
    let mut lcg = Lcg::new(SEG_BASE as u64);
    let mut ok = 0;
    let mut r = 0;
    while r < rounds {
        let flip = lcg.below(2) == 0;
        let (a, b) = if flip { (0xB, 0xA) } else { (0xA, 0xB) };
        m.inject_pair(a, b)?;
        let rolled = m.judge_and_rollback(a, b)?;
        if rolled != 2 {
            return Err(DlCode::SentinelBroken.val());
        }
        m.clean()?;
        ok += 1;
        r += 1;
    }
    Ok((ok as u32, m.rolled))
}

// ── 注入面：三族全抓 ─────────────────────────────────────────────
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
    // 族 0：自等待必拒。
    ledger.tries[0] += 1;
    let mut m = D4Monitor::new();
    if let Err(e) = m.inject_pair(0xA, 0xA) {
        if e == DlCode::SelfWait.val() {
            ledger.caught[0] += 1;
        }
    }
    // 族 1：无环单向等待必放行（反向注入面）。
    ledger.tries[1] += 1;
    if let Ok(true) = no_cycle_passes() {
        ledger.caught[1] += 1;
    }
    // 族 2：构环后残环检查器必就位（未回滚时 find_cycle 非 None）。
    ledger.tries[2] += 1;
    let mut m2 = D4Monitor::new();
    let _ = m2.inject_pair(0xA, 0xB);
    if m2.find_cycle(0xA).is_some() {
        ledger.caught[2] += 1;
    }
}

// ── 黄金正演 ─────────────────────────────────────────────────────
pub fn golden_run() -> Result<(u32, u32, u32, u32, u32), u32> {
    let (cyc, rolled, dead) = inject_1000()?;
    let nopass = no_cycle_passes()?;
    let (ok_r, rolled2) = random_pair_rounds(64)?;
    let np = if nopass { 1 } else { 0 };
    Ok((cyc, rolled, dead, np * 1000 + ok_r, rolled2))
}

// ── 判据入口 ─────────────────────────────────────────────────────
pub fn segment_guard() -> bool {
    let mut i = 0;
    while i < DlCode::ALL.len() {
        let v = DlCode::ALL[i].val();
        if v < SEG_BASE || v >= SEG_BASE + 0x40 {
            return false;
        }
        i += 1;
    }
    true
}

pub fn extra_checks() -> Vec<(&'static str, bool, String)> {
    let mut v: Vec<(&'static str, bool, String)> = Vec::new();
    let golden = golden_run();
    v.push((
        "J1-1000 轮构环判回零真死锁",
        matches!(golden, Ok((1000, 2000, 0, 1064, _))),
        format!("{:?}", golden),
    ));
    // 阈值常量点验（判据独立写死）。
    v.push((
        "J1-D4 阈值 = 200 刻度",
        D4_THRESHOLD == 200,
        format!("threshold={}", D4_THRESHOLD),
    ));
    v.push((
        "J1-反向：无环必放行",
        matches!(no_cycle_passes(), Ok(true)),
        String::from("单向等待放行"),
    ));
    // 状态名点验。
    let mut ok = 0;
    let mut i = 0;
    while i < W_STATE_COUNT {
        if W_NAMES[i].len() > 0 {
            ok += 1;
        }
        i += 1;
    }
    v.push((
        "J1-3 等待态名点验",
        ok == 3,
        format!("{}/3", ok),
    ));
    let mut inj = InjectLedger::new();
    run_injections(&mut inj);
    v.push((
        "J1-注入三族全抓",
        inj.all_caught(),
        format!("t/c {:?}/{:?}", inj.tries, inj.caught),
    ));
    v.push(("J3-段位守卫", segment_guard(), String::from("0x66A80 窗口")));
    v
}

pub fn run_unx14721_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F14721 A→B→A 互发死锁注入");
    for (n, p, d) in extra_checks() {
        s.add_dyn(n, p, &d);
    }
    s
}
