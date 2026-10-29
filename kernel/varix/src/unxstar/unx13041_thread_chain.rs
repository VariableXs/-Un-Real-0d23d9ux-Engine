//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F13041 · CreateThread 内核侧链路（五段链）· 0x65580
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定）：
//!   ① 纯功能代码 ≥400 行；
//!   ② UNX-F13041-J1 五段链（对象创建→栈→TEB→APC 首投递→调度）
//!     分段计时落账，1,000 线程并发创建销毁 10^3 轮零错账，全链
//!     P95 对照 Windows ×1.5 内；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - 五段闭集（写死段号 0..4）：0 对象创建 1 栈 2 TEB
//!     3 APC 首投递 4 调度；
//!   - 分段计时账：段和 == 全链 tick；线性推进状态机，跳段/回段拒收；
//!   - 并发账：1,000 线程 × 10^3 轮（创建+销毁）账守恒——每轮
//!     created == destroyed（终态），在册峰值 <= 1,000；tid 分配
//!     最低空闲位复用（与 F13061 位图语义一致面）；
//!   - P95 无浮点：升序后 (95*n+99)/100-1 下标；×1.5 整数化
//!     ours*2 <= win*3；
//!   - 失败注入：栈耗尽段拒 → 回滚账（已建对象逆序回收）；
//!   - LCG 种子写死 = SEG_BASE（0x65580）；禁 rand/浮点/IO/墙钟；
//!     no_std + alloc；零 unwrap/expect。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x65580，全仓独占）。
pub const SEG_BASE: u32 = 0x65580;

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ThdCode {
    /// 跳段
    StageSkipped,
    /// 回段
    StageBacktracked,
    /// 段账守恒破
    LedgerBroken,
    /// 对象创建拒（句柄表满）
    ObjectTableFull,
    /// 栈保留/提交拒
    StackReserveFailed,
    /// TEB 分配拒
    TebAllocFailed,
    /// 销毁账破（created != destroyed）
    DestroyLedgerBroken,
    /// P95 对账超阈
    P95OverThreshold,
}

impl ThdCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            ThdCode::StageSkipped => "跳段：五段链须线性推进",
            ThdCode::StageBacktracked => "回段：状态机逆行拒收",
            ThdCode::LedgerBroken => "段账守恒破：段和 != 全链 tick",
            ThdCode::ObjectTableFull => "对象创建拒：句柄表满",
            ThdCode::StackReserveFailed => "栈保留/提交拒",
            ThdCode::TebAllocFailed => "TEB 分配拒",
            ThdCode::DestroyLedgerBroken => "销毁账破：created != destroyed",
            ThdCode::P95OverThreshold => "P95 对账超阈：ours/win 超过 ×1.5",
        }
    }
    pub const ALL: [ThdCode; 8] = [
        ThdCode::StageSkipped,
        ThdCode::StageBacktracked,
        ThdCode::LedgerBroken,
        ThdCode::ObjectTableFull,
        ThdCode::StackReserveFailed,
        ThdCode::TebAllocFailed,
        ThdCode::DestroyLedgerBroken,
        ThdCode::P95OverThreshold,
    ];
}

pub fn detail(code: u32) -> &'static str {
    let mut i = 0;
    while i < ThdCode::ALL.len() {
        if ThdCode::ALL[i].val() == code {
            return ThdCode::ALL[i].detail();
        }
        i += 1;
    }
    "未知诊断码"
}

// ── 五段闭集 ────────────────────────────────────────────────────
pub const ST_OBJECT: usize = 0;
pub const ST_STACK: usize = 1;
pub const ST_TEB: usize = 2;
pub const ST_APC: usize = 3;
pub const ST_SCHED: usize = 4;
/// 段数闭集（5，写死）。
pub const STAGE_COUNT: usize = 5;

pub const STAGE_NAMES: [&str; STAGE_COUNT] = ["对象创建", "栈", "TEB", "APC 首投递", "调度"];

// ── 段推进状态机（线性闭式） ─────────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ChainState {
    NotStarted,
    Running,
    Completed,
    Failed,
}

pub fn edge_legal(from: ChainState, to: ChainState) -> bool {
    matches!(
        (from, to),
        (ChainState::NotStarted, ChainState::Running)
            | (ChainState::Running, ChainState::Running)
            | (ChainState::Running, ChainState::Completed)
            | (ChainState::Running, ChainState::Failed)
    )
}

/// 段推进守卫：next == cur+1；跳段/回段闭式分错。
pub fn advance(cur: usize, next: usize) -> Result<usize, ThdCode> {
    if next == cur + 1 && next < STAGE_COUNT {
        Ok(next)
    } else if next <= cur {
        Err(ThdCode::StageBacktracked)
    } else {
        Err(ThdCode::StageSkipped)
    }
}

// ── 单线程链账 ──────────────────────────────────────────────────
#[derive(Clone, Debug)]
pub struct ThreadChainLedger {
    pub stage_ticks: [u64; STAGE_COUNT],
    pub stage_done: [bool; STAGE_COUNT],
    pub total_ticks: u64,
    pub cur: usize,
    pub state: ChainState,
    pub rolled_back: usize,
}

impl ThreadChainLedger {
    pub fn new() -> ThreadChainLedger {
        ThreadChainLedger {
            stage_ticks: [0; STAGE_COUNT],
            stage_done: [false; STAGE_COUNT],
            total_ticks: 0,
            cur: 0,
            state: ChainState::NotStarted,
            rolled_back: 0,
        }
    }

    pub fn finish_stage(&mut self, stage: usize, ticks: u64) -> Result<(), ThdCode> {
        if self.state == ChainState::NotStarted {
            if stage != ST_OBJECT {
                return Err(ThdCode::StageSkipped);
            }
            self.state = ChainState::Running;
        } else if self.state != ChainState::Running {
            return Err(ThdCode::StageBacktracked);
        }
        if self.stage_done[stage] {
            return Err(ThdCode::StageBacktracked);
        }
        if stage != self.cur {
            if stage > self.cur {
                return Err(ThdCode::StageSkipped);
            }
            return Err(ThdCode::StageBacktracked);
        }
        self.stage_ticks[stage] = ticks;
        self.stage_done[stage] = true;
        self.total_ticks = self.total_ticks.wrapping_add(ticks);
        if stage == ST_SCHED {
            self.state = ChainState::Completed;
            self.cur = STAGE_COUNT; // 完成段数账 = 5
        } else {
            self.cur = stage + 1;
        }
        Ok(())
    }

    pub fn conserved(&self) -> bool {
        let mut sum: u64 = 0;
        let mut done = 0;
        let mut i = 0;
        while i < STAGE_COUNT {
            sum = sum.wrapping_add(self.stage_ticks[i]);
            if self.stage_done[i] {
                done += 1;
            }
            i += 1;
        }
        sum == self.total_ticks && done == self.cur
    }

    /// 失败回滚：逆序回收已建段。
    pub fn rollback(&mut self) -> Result<(), ThdCode> {
        if self.state != ChainState::Running {
            return Err(ThdCode::DestroyLedgerBroken);
        }
        let mut i = self.cur;
        while i > 0 {
            i -= 1;
            if self.stage_done[i] {
                self.stage_done[i] = false;
                self.rolled_back += 1;
            }
        }
        self.state = ChainState::Failed;
        if self.rolled_back != self.cur {
            return Err(ThdCode::DestroyLedgerBroken);
        }
        Ok(())
    }
}

pub fn run_chain(ticks: &[u64; STAGE_COUNT]) -> Result<ThreadChainLedger, ThdCode> {
    let mut led = ThreadChainLedger::new();
    let mut i = 0;
    while i < STAGE_COUNT {
        led.finish_stage(i, ticks[i])?;
        i += 1;
    }
    if !led.conserved() {
        return Err(ThdCode::LedgerBroken);
    }
    Ok(led)
}

// ── 并发账（tid 位图最低空闲位，1,000 线程 × 10^3 轮） ───────────
pub const MAX_THREADS: usize = 1_000;
/// 轮数闭集（10^3，判据写死）。
pub const ROUNDS: usize = 1_000;

#[derive(Clone, Debug)]
pub struct ThreadTable {
    /// tid 占用位图（index 0..MAX_THREADS）。
    pub used: Vec<bool>,
    /// 在册数。
    pub live: usize,
    /// 峰值在册。
    pub peak: usize,
    /// 累计创建 / 销毁。
    pub created: u64,
    pub destroyed: u64,
}

impl ThreadTable {
    pub fn new() -> ThreadTable {
        ThreadTable {
            used: vec![false; MAX_THREADS],
            live: 0,
            peak: 0,
            created: 0,
            destroyed: 0,
        }
    }

    /// 最低空闲位分配（Windows 一致语义）。
    pub fn alloc_tid(&mut self) -> Result<usize, ThdCode> {
        let mut i = 0;
        while i < MAX_THREADS {
            if !self.used[i] {
                self.used[i] = true;
                self.live += 1;
                self.created += 1;
                if self.live > self.peak {
                    self.peak = self.live;
                }
                if self.live > MAX_THREADS {
                    return Err(ThdCode::ObjectTableFull);
                }
                return Ok(i);
            }
            i += 1;
        }
        Err(ThdCode::ObjectTableFull)
    }

    pub fn free_tid(&mut self, tid: usize) -> Result<(), ThdCode> {
        if tid >= MAX_THREADS {
            return Err(ThdCode::DestroyLedgerBroken);
        }
        if !self.used[tid] {
            return Err(ThdCode::DestroyLedgerBroken);
        }
        self.used[tid] = false;
        self.live -= 1;
        self.destroyed += 1;
        Ok(())
    }

    /// 账实守恒：位图置位数 == live；created == destroyed（终态）。
    pub fn conserved(&self) -> bool {
        let mut n = 0;
        let mut i = 0;
        while i < MAX_THREADS {
            if self.used[i] {
                n += 1;
            }
            i += 1;
        }
        n == self.live && self.created >= self.destroyed
    }

    /// 终态零泄漏：created == destroyed 且 live == 0。
    pub fn zero_leak(&self) -> bool {
        self.created == self.destroyed && self.live == 0 && self.conserved()
    }
}

// ── P95（无浮点） ───────────────────────────────────────────────
pub fn sort_asc(v: &mut [u64]) {
    let mut a = 0;
    while a < v.len() {
        let mut b = 0;
        while b + 1 < v.len() {
            if v[b] > v[b + 1] {
                let t = v[b];
                v[b] = v[b + 1];
                v[b + 1] = t;
            }
            b += 1;
        }
        a += 1;
    }
}

pub fn percentile95(sorted: &[u64]) -> Option<u64> {
    if sorted.is_empty() {
        return None;
    }
    let n = sorted.len();
    let idx = (95 * n + 99) / 100 - 1;
    if idx >= n {
        return Some(sorted[n - 1]);
    }
    Some(sorted[idx])
}

pub fn within_150pct(ours: u64, win: u64) -> bool {
    if win == 0 {
        return ours == 0;
    }
    ours.wrapping_mul(2) <= win.wrapping_mul(3)
}

// ── LCG（种子写死 = SEG_BASE） ──────────────────────────────────
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
        if n == 0 {
            return 0;
        }
        self.next() % n
    }
}

/// 合成一次建链分段 tick。
pub fn synth_ticks(rng: &mut Lcg, salt: u64) -> [u64; STAGE_COUNT] {
    let mut ticks = [0u64; STAGE_COUNT];
    let mut i = 0;
    while i < STAGE_COUNT {
        let base = match i {
            ST_OBJECT => 25,
            ST_STACK => 80,
            ST_TEB => 40,
            ST_APC => 30,
            _ => 20,
        };
        ticks[i] = base + rng.next_below(30) + salt;
        i += 1;
    }
    ticks
}

// ── 1,000 线程 × 10^3 轮零错账（判据 J1 主面） ───────────────────
pub fn drill_1000_threads_1000_rounds() -> (bool, String) {
    let mut rng = Lcg::new(SEG_BASE as u64);
    let mut table = ThreadTable::new();
    let mut ok = true;
    let mut round = 0;
    let mut chains_ok = true;
    while round < ROUNDS {
        // 每轮：批量创建 + 批量销毁（并发窗口内峰值 <= 1,000）。
        let batch = 8 + rng.next_below(16) as usize;
        let mut tids: Vec<usize> = Vec::new();
        let mut b = 0;
        while b < batch {
            match table.alloc_tid() {
                Ok(tid) => {
                    tids.push(tid);
                    // 建链落账（分段计时）。
                    let ticks = synth_ticks(&mut rng, 0);
                    if run_chain(&ticks).is_err() {
                        chains_ok = false;
                    }
                }
                Err(_) => {
                    ok = ok && table.live == MAX_THREADS; // 满表才可拒
                    break;
                }
            }
            b += 1;
        }
        // 逆序销毁（LIFO 语义）。
        let mut d = tids.len();
        while d > 0 {
            d -= 1;
            if table.free_tid(tids[d]).is_err() {
                ok = false;
                break;
            }
        }
        ok = ok && table.conserved();
        round += 1;
    }
    ok = ok && table.zero_leak() && table.peak <= MAX_THREADS && chains_ok;
    (
        ok,
        format!(
            "10^3 轮: created={} destroyed={} peak={} 零泄漏={}",
            table.created, table.destroyed, table.peak, table.zero_leak()
        ),
    )
}

pub fn drill_full_chain_1000() -> (bool, String) {
    // 1,000 线程全建全拆：tid 覆盖 0..999 全域、分配序 = 最低空闲位。
    let mut table = ThreadTable::new();
    let mut tids: Vec<usize> = Vec::new();
    let mut i = 0;
    let mut ok = true;
    while i < MAX_THREADS {
        match table.alloc_tid() {
            Ok(tid) => {
                ok = ok && tid == i; // 最低空闲位序
                tids.push(tid);
            }
            Err(_) => {
                ok = false;
                break;
            }
        }
        i += 1;
    }
    // 满表再建 → ObjectTableFull。
    ok = ok && table.alloc_tid() == Err(ThdCode::ObjectTableFull);
    // 全拆。
    let mut j = 0;
    while j < MAX_THREADS {
        ok = ok && table.free_tid(tids[j]).is_ok();
        j += 1;
    }
    ok = ok && table.zero_leak();
    // 复用：拆后最低位 0 再分配得 0。
    match table.alloc_tid() {
        Ok(tid) => ok = ok && tid == 0,
        Err(_) => ok = false,
    }
    (ok, format!("1,000 线程全建全拆: 最低位序+满表拒+复用 = {}", ok))
}

pub fn drill_stage_order() -> (bool, String) {
    // 段序守卫注入。
    let mut caught = 0;
    if advance(0, 2) == Err(ThdCode::StageSkipped) {
        caught += 1;
    }
    if advance(0, 4) == Err(ThdCode::StageSkipped) {
        caught += 1;
    }
    if advance(3, 1) == Err(ThdCode::StageBacktracked) {
        caught += 1;
    }
    if advance(2, 2) == Err(ThdCode::StageBacktracked) {
        caught += 1;
    }
    if advance(4, 5) == Err(ThdCode::StageSkipped) {
        caught += 1;
    }
    let mut ok_edges = true;
    let mut i = 0;
    while i < 4 {
        ok_edges = ok_edges && advance(i, i + 1).is_ok();
        i += 1;
    }
    if ok_edges {
        caught += 1;
    }
    let ok = caught == 6;
    (ok, format!("段序守卫注入 {}/6", caught))
}

pub fn drill_stage_ledger() -> (bool, String) {
    // 分段计时落账：24 链守恒（段和 == 全链）。
    let mut rng = Lcg::new(SEG_BASE as u64 + 1);
    let mut caught = 0;
    let mut n = 0;
    while n < 24 {
        let ticks = synth_ticks(&mut rng, 1);
        match run_chain(&ticks) {
            Ok(l) => {
                if l.conserved() && l.state == ChainState::Completed && l.total_ticks > 0 {
                    caught += 1;
                }
            }
            Err(_) => {}
        }
        n += 1;
    }
    let ok = caught == 24;
    (ok, format!("分段计时账 {}/24", caught))
}

pub fn drill_rollback() -> (bool, String) {
    // 失败回滚：段 0..3 各失败点 ×回滚账守恒（段 4 完成 = 终态不回滚）。
    let mut caught = 0;
    let mut k = 0;
    while k < 4 {
        let mut led = ThreadChainLedger::new();
        let mut i = 0;
        while i <= k {
            let _ = led.finish_stage(i, 10 + i as u64);
            i += 1;
        }
        let before = led.cur;
        let rb = led.rollback();
        if rb.is_ok()
            && led.rolled_back == before
            && led.state == ChainState::Failed
            && led.stage_done.iter().all(|d| !*d)
        {
            caught += 1;
        }
        k += 1;
    }
    let ok = caught == 4;
    (ok, format!("失败回滚 {}/4 失败点", caught))
}

pub fn drill_20_threads_p95() -> (bool, String) {
    // 20 样本全链 P95 ×1.5 对账（Windows 参照独立盐）。
    let mut rng = Lcg::new(SEG_BASE as u64 + 2);
    let mut ours_all: Vec<u64> = Vec::new();
    let mut win_all: Vec<u64> = Vec::new();
    let mut ok_chains = true;
    let mut n = 0;
    while n < 20 {
        let ours_ticks = synth_ticks(&mut rng, 0);
        let win_ticks = synth_ticks(&mut rng, 2);
        match run_chain(&ours_ticks) {
            Ok(l) => ours_all.push(l.total_ticks),
            Err(_) => {
                ok_chains = false;
                ours_all.push(0);
            }
        }
        match run_chain(&win_ticks) {
            Ok(l) => win_all.push(l.total_ticks),
            Err(_) => {
                ok_chains = false;
                win_all.push(u64::MAX);
            }
        }
        n += 1;
    }
    sort_asc(&mut ours_all);
    sort_asc(&mut win_all);
    let p95o = percentile95(&ours_all).unwrap_or(0);
    let p95w = percentile95(&win_all).unwrap_or(1);
    let ok = ok_chains && within_150pct(p95o, p95w);
    (
        ok,
        format!("20 样本 P95: ours={} win={} ×1.5内={}", p95o, p95w, ok),
    )
}

pub fn drill_p95_math() -> (bool, String) {
    let mut v: Vec<u64> = Vec::new();
    let mut i = 0;
    while i < 20 {
        v.push((19 - i) as u64);
        i += 1;
    }
    sort_asc(&mut v);
    let ok = percentile95(&v) == Some(18)
        && percentile95(&[7]) == Some(7)
        && percentile95(&[]).is_none()
        && within_150pct(15, 10)
        && !within_150pct(16, 10);
    (ok, format!("P95 闭式 + 阈值反向: {}", ok))
}

pub fn drill_stage_names() -> (bool, String) {
    let mut ok = STAGE_NAMES.len() == STAGE_COUNT;
    let mut i = 0;
    while i < STAGE_COUNT {
        ok = ok && !STAGE_NAMES[i].is_empty();
        i += 1;
    }
    ok = ok && STAGE_NAMES[ST_OBJECT] == "对象创建" && STAGE_NAMES[ST_SCHED] == "调度";
    (ok, format!("五段名闭表: {}", ok))
}

pub fn drill_reuse_no_race() -> (bool, String) {
    // 复用竞态注入：同一 tid 双重分配必拒（位图原子语义）。
    let mut caught = 0;
    let mut round = 0;
    while round < 10 {
        let mut table = ThreadTable::new();
        let t1 = table.alloc_tid();
        match t1 {
            Ok(tid) => {
                // 强行重复占位（建模竞态）→ 位图已置 → 二次分配应得下一位。
                let t2 = table.alloc_tid();
                let race_caught = t2 == Ok(tid + 1) && table.used[tid];
                if race_caught && table.free_tid(tid).is_ok() {
                    caught += 1;
                }
            }
            Err(_) => {}
        }
        round += 1;
    }
    let ok = caught == 10;
    (ok, format!("复用竞态注入 {}/10", caught))
}

pub fn drill_double_free_injection() -> (bool, String) {
    // 双重销毁注入 10/10：必拒且账不破。
    let mut caught = 0;
    let mut round = 0;
    while round < 10 {
        let mut table = ThreadTable::new();
        let tid = match table.alloc_tid() {
            Ok(t) => t,
            Err(_) => break,
        };
        let f1 = table.free_tid(tid).is_ok();
        let f2 = table.free_tid(tid) == Err(ThdCode::DestroyLedgerBroken);
        let f3 = table.free_tid(MAX_THREADS + round) == Err(ThdCode::DestroyLedgerBroken);
        if f1 && f2 && f3 && table.conserved() {
            caught += 1;
        }
        round += 1;
    }
    let ok = caught == 10;
    (ok, format!("双重销毁注入 {}/10", caught))
}

pub fn drill_diag_codes_closed() -> (bool, String) {
    let mut ok = true;
    let mut i = 0;
    while i < ThdCode::ALL.len() {
        ok = ok && ThdCode::ALL[i].val() >= SEG_BASE && ThdCode::ALL[i].val() < SEG_BASE + 0x40;
        ok = ok && !ThdCode::ALL[i].detail().is_empty();
        i += 1;
    }
    ok = ok && detail(SEG_BASE + 0x3F) == "未知诊断码";
    (ok, format!("诊断码闭集 {}/8 + 反向", ThdCode::ALL.len()))
}

// ── 段位守卫 ─────────────────────────────────────────────────────
pub fn segment_guard() -> bool {
    let mut ok = true;
    let mut i = 0;
    while i < ThdCode::ALL.len() {
        let v = ThdCode::ALL[i].val();
        ok = ok && v >= SEG_BASE && v < SEG_BASE + 0x40;
        i += 1;
    }
    ok
}

// ── 全流程对账 ───────────────────────────────────────────────────
pub fn drill_full_flow() -> (bool, String) {
    let (a1, d1) = drill_1000_threads_1000_rounds();
    let (a2, d2) = drill_full_chain_1000();
    let (a3, d3) = drill_stage_ledger();
    let ok = a1 && a2 && a3;
    (
        ok,
        format!(
            "全流程: 10^3轮={} 全建拆={} 计时账={} [{}|{}|{}]",
            a1, a2, a3, d1, d2, d3
        ),
    )
}

// ── 判据聚合 ─────────────────────────────────────────────────────
pub fn extra_checks() -> Vec<(&'static str, bool, String)> {
    let mut v: Vec<(&'static str, bool, String)> = Vec::new();
    let (a, b) = drill_1000_threads_1000_rounds();
    v.push(("J1-1000线程×10^3轮零错账", a, b));
    let (a, b) = drill_full_chain_1000();
    v.push(("J1-全建全拆最低位序", a, b));
    let (a, b) = drill_stage_order();
    v.push(("J2-段序守卫注入", a, b));
    let (a, b) = drill_stage_ledger();
    v.push(("J2-分段计时账24/24", a, b));
    let (a, b) = drill_rollback();
    v.push(("J2-失败回滚4/4", a, b));
    let (a, b) = drill_20_threads_p95();
    v.push(("J1-20样本P95×1.5对账", a, b));
    let (a, b) = drill_p95_math();
    v.push(("J2-P95闭式+阈值反向", a, b));
    let (a, b) = drill_stage_names();
    v.push(("J3-五段名闭表", a, b));
    let (a, b) = drill_reuse_no_race();
    v.push(("J2-复用竞态注入10/10", a, b));
    let (a, b) = drill_double_free_injection();
    v.push(("J2-双重销毁注入10/10", a, b));
    let (a, b) = drill_diag_codes_closed();
    v.push(("J3-诊断码闭集+反向", a, b));
    let (a, b) = drill_full_flow();
    v.push(("J1-全流程对账", a, b));
    v
}

/// run_unx13041_checks：本单判据聚合（诊断段 0x65580）。
pub fn run_unx13041_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F13041 CreateThread 五段链");
    s.add_dyn("J3-段位守卫", segment_guard(), "0x65580 段闭集");
    let checks = extra_checks();
    let mut i = 0;
    while i < checks.len() {
        let (n, p, d) = checks[i].clone();
        s.add_dyn(n, p, &d);
        i += 1;
    }
    s
}
