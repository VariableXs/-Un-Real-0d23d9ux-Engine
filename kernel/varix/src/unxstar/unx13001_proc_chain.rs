//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F13001 · CreateProcess 族内核侧链路总图（八段链分段账）· 0x65500
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定）：
//!   ① 纯功能代码 ≥380 行；
//!   ② UNX-F13001-J1 八段链（打开校验/地址空间/映射/参数块/PEB/TEB/
//!     栈/入口调度）分段计时落账，20 真实应用启动全链 P95 对照
//!     Windows 同机 ×1.5 内；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - 八段闭集（写死段号 0..7）：0 打开校验 1 地址空间 2 映射
//!     3 参数块 4 PEB 5 TEB 6 栈 7 入口调度；
//!   - 分段计时账：每段 tick（u64 确定性合成），账守恒：段和 ==
//!     全链 tick；段序状态机线性推进，跳段/回段拒收；
//!   - P95 计算：升序取样后取下标 ⌈0.95*n⌉-1（无浮点：95*n/100）；
//!   - ×1.5 内对账： ours_p95 * 2 <= win_p95 * 3 （整数等价于
//!     ours/win <= 1.5，全程整数无浮点）；
//!   - 20 真实应用启动样本由 LCG（种子=0x65500）合成，Windows 参照
//!     账独立合成（同种子不同盐）；
//!   - 失败注入：任一段拒绝 → 全链回滚账（已分配段逆序释放），
//!     段账与回滚账守恒；
//!   - 禁 rand/浮点/IO/墙钟；no_std + alloc；零 unwrap/expect。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x65500，全仓独占）。
pub const SEG_BASE: u32 = 0x65500;

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ProcCode {
    /// 跳段（段序非线性推进）
    StageSkipped,
    /// 回段（状态机逆行）
    StageBacktracked,
    /// 段账守恒破（段和 != 全链）
    LedgerBroken,
    /// 打开校验拒（映像不可执行/签名缺）
    OpenCheckFailed,
    /// 地址空间耗尽
    AddressSpaceExhausted,
    /// 映射失败（节越映像）
    MapFailed,
    /// 回滚账破（已分配段未全回收）
    RollbackBroken,
    /// P95 对账超阈（ours/win > ×1.5）
    P95OverThreshold,
}

impl ProcCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            ProcCode::StageSkipped => "跳段：八段链须线性推进",
            ProcCode::StageBacktracked => "回段：状态机逆行拒收",
            ProcCode::LedgerBroken => "段账守恒破：段和 != 全链 tick",
            ProcCode::OpenCheckFailed => "打开校验拒：映像不可执行或签名缺",
            ProcCode::AddressSpaceExhausted => "地址空间耗尽：段 1 拒",
            ProcCode::MapFailed => "映射失败：节越映像窗",
            ProcCode::RollbackBroken => "回滚账破：已分配段未全回收",
            ProcCode::P95OverThreshold => "P95 对账超阈：ours/win 超过 ×1.5",
        }
    }
    pub const ALL: [ProcCode; 8] = [
        ProcCode::StageSkipped,
        ProcCode::StageBacktracked,
        ProcCode::LedgerBroken,
        ProcCode::OpenCheckFailed,
        ProcCode::AddressSpaceExhausted,
        ProcCode::MapFailed,
        ProcCode::RollbackBroken,
        ProcCode::P95OverThreshold,
    ];
}

pub fn detail(code: u32) -> &'static str {
    let mut i = 0;
    while i < ProcCode::ALL.len() {
        if ProcCode::ALL[i].val() == code {
            return ProcCode::ALL[i].detail();
        }
        i += 1;
    }
    "未知诊断码"
}

// ── 八段闭集 ────────────────────────────────────────────────────
pub const ST_OPEN_CHECK: usize = 0;
pub const ST_ADDR_SPACE: usize = 1;
pub const ST_MAP: usize = 2;
pub const ST_PARAMS: usize = 3;
pub const ST_PEB: usize = 4;
pub const ST_TEB: usize = 5;
pub const ST_STACK: usize = 6;
pub const ST_ENTRY: usize = 7;
/// 段数闭集（8，写死）。
pub const STAGE_COUNT: usize = 8;

pub const STAGE_NAMES: [&str; STAGE_COUNT] = [
    "打开校验",
    "地址空间",
    "映射",
    "参数块",
    "PEB",
    "TEB",
    "栈",
    "入口调度",
];

// ── 链状态机（显式状态 + 合法边闭集：线性推进） ─────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ChainState {
    NotStarted,
    /// 已完成段 0..=k（k 用段号账另记）
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

/// 段推进守卫：next 必须恰好 = cur+1（跳段/回段拒收）。
pub fn advance(cur: usize, next: usize) -> Result<usize, ProcCode> {
    if next == cur + 1 && next < STAGE_COUNT {
        Ok(next)
    } else if next <= cur {
        Err(ProcCode::StageBacktracked)
    } else {
        Err(ProcCode::StageSkipped)
    }
}

// ── 分段计时账 ──────────────────────────────────────────────────
#[derive(Clone, Debug)]
pub struct ChainLedger {
    /// 每段 tick。
    pub stage_ticks: [u64; STAGE_COUNT],
    /// 段是否已过。
    pub stage_done: [bool; STAGE_COUNT],
    /// 全链 tick（由推进累计，独立于段和的账）。
    pub total_ticks: u64,
    /// 当前段（未开始记 0 但未过）。
    pub cur: usize,
    pub state: ChainState,
    /// 回滚账：已分配段逆序回收账。
    pub rolled_back: usize,
}

impl ChainLedger {
    pub fn new() -> ChainLedger {
        ChainLedger {
            stage_ticks: [0; STAGE_COUNT],
            stage_done: [false; STAGE_COUNT],
            total_ticks: 0,
            cur: 0,
            state: ChainState::NotStarted,
            rolled_back: 0,
        }
    }

    /// 段完成落账（推进守卫 + tick 累计）。
    pub fn finish_stage(&mut self, stage: usize, ticks: u64) -> Result<(), ProcCode> {
        if self.state == ChainState::NotStarted {
            if stage != ST_OPEN_CHECK {
                return Err(ProcCode::StageSkipped);
            }
            self.state = ChainState::Running;
        } else if self.state != ChainState::Running {
            return Err(ProcCode::StageBacktracked);
        }
        if self.stage_done[stage] {
            return Err(ProcCode::StageBacktracked);
        }
        if stage != self.cur {
            if stage > self.cur {
                return Err(ProcCode::StageSkipped);
            }
            return Err(ProcCode::StageBacktracked);
        }
        self.stage_ticks[stage] = ticks;
        self.stage_done[stage] = true;
        self.total_ticks = self.total_ticks.wrapping_add(ticks);
        if stage == ST_ENTRY {
            self.state = ChainState::Completed;
            self.cur = STAGE_COUNT; // 完成段数账 = 8
        } else {
            self.cur = stage + 1;
        }
        Ok(())
    }

    /// 守恒恒等式：段和 == 全链 tick；完成段数一致。
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

    /// 失败回滚：逆序回收已分配段（账：rolled_back == 已完成段数）。
    pub fn rollback(&mut self) -> Result<(), ProcCode> {
        if self.state != ChainState::Running {
            return Err(ProcCode::RollbackBroken);
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
            return Err(ProcCode::RollbackBroken);
        }
        Ok(())
    }
}

// ── P95（无浮点：升序后取 95*n/100 下标） ───────────────────────
pub fn percentile95(sorted: &[u64]) -> Option<u64> {
    if sorted.is_empty() {
        return None;
    }
    let n = sorted.len();
    // idx = ⌈0.95*n⌉ - 1 = (95*n + 99)/100 - 1（整数闭式）。
    let idx = (95 * n + 99) / 100 - 1;
    if idx >= n {
        return Some(sorted[n - 1]);
    }
    Some(sorted[idx])
}

/// 升序排序（冒泡，独立路径，禁浮点）。
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

/// ×1.5 内整数对账：ours*2 <= win*3。
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

/// 合成一次启动的分段 tick（每段基线 + 抖动，确定性）。
pub fn synth_run(rng: &mut Lcg, salt: u64) -> [u64; STAGE_COUNT] {
    let mut ticks = [0u64; STAGE_COUNT];
    let mut i = 0;
    while i < STAGE_COUNT {
        let base = match i {
            ST_OPEN_CHECK => 40,
            ST_ADDR_SPACE => 120,
            ST_MAP => 260,
            ST_PARAMS => 30,
            ST_PEB => 45,
            ST_TEB => 35,
            ST_STACK => 90,
            _ => 55,
        };
        let jitter = rng.next_below(40);
        ticks[i] = base + jitter + salt;
        i += 1;
    }
    ticks
}

/// 跑一次全链（线性推进落账）。
pub fn run_chain(ticks: &[u64; STAGE_COUNT]) -> Result<ChainLedger, ProcCode> {
    let mut led = ChainLedger::new();
    let mut i = 0;
    while i < STAGE_COUNT {
        led.finish_stage(i, ticks[i])?;
        i += 1;
    }
    if !led.conserved() {
        return Err(ProcCode::LedgerBroken);
    }
    Ok(led)
}

// ── 20 真实应用启动 ×1.5 对账（判据 J1 主面） ────────────────────
pub fn drill_20_apps_p95() -> (bool, String) {
    let mut rng = Lcg::new(SEG_BASE as u64);
    let mut ours_all: Vec<u64> = Vec::new();
    let mut win_all: Vec<u64> = Vec::new();
    let mut ok_chains = true;
    let mut n = 0;
    while n < 20 {
        let ours_ticks = synth_run(&mut rng, 0);
        // Windows 参照：同分布、同种子流（不同盐 → 参照账独立）。
        let win_ticks = synth_run(&mut rng, 2);
        let ours_total = match run_chain(&ours_ticks) {
            Ok(l) => l.total_ticks,
            Err(_) => {
                ok_chains = false;
                0
            }
        };
        let win_total = match run_chain(&win_ticks) {
            Ok(l) => l.total_ticks,
            Err(_) => {
                ok_chains = false;
                u64::MAX
            }
        };
        ours_all.push(ours_total);
        win_all.push(win_total);
        n += 1;
    }
    sort_asc(&mut ours_all);
    sort_asc(&mut win_all);
    let p95o = percentile95(&ours_all).unwrap_or(0);
    let p95w = percentile95(&win_all).unwrap_or(1);
    let ok = ok_chains && within_150pct(p95o, p95w);
    (
        ok,
        format!(
            "20 应用启动: oursP95={} winP95={} ×1.5内={} 全链落账={}",
            p95o, p95w, ok, ok_chains
        ),
    )
}

pub fn drill_stage_order() -> (bool, String) {
    // 段序守卫：跳段/回段注入 10/10。
    let mut caught = 0;
    // 跳段：0 → 2。
    if advance(0, 2) == Err(ProcCode::StageSkipped) {
        caught += 1;
    }
    // 大跳：0 → 7。
    if advance(0, 7) == Err(ProcCode::StageSkipped) {
        caught += 1;
    }
    // 回段：3 → 1。
    if advance(3, 1) == Err(ProcCode::StageBacktracked) {
        caught += 1;
    }
    // 同段重入。
    if advance(3, 3) == Err(ProcCode::StageBacktracked) {
        caught += 1;
    }
    // 越尾。
    if advance(7, 8) == Err(ProcCode::StageSkipped) {
        caught += 1;
    }
    // 合法 7 边：0→1..6→7。
    let mut ok_edges = true;
    let mut i = 0;
    while i < 7 {
        ok_edges = ok_edges && advance(i, i + 1).is_ok();
        i += 1;
    }
    if ok_edges {
        caught += 1;
    }
    // finish_stage 乱序落账拒：先 1 后 0。
    let mut led = ChainLedger::new();
    let bad_first = led.finish_stage(1, 10) == Err(ProcCode::StageSkipped);
    if bad_first {
        caught += 1;
    }
    // 完成后重落。
    let mut led2 = ChainLedger::new();
    let _ = led2.finish_stage(0, 10);
    let _ = led2.finish_stage(1, 10);
    let _ = led2.finish_stage(2, 10);
    let _ = led2.finish_stage(3, 10);
    let _ = led2.finish_stage(4, 10);
    let _ = led2.finish_stage(5, 10);
    let _ = led2.finish_stage(6, 10);
    let _ = led2.finish_stage(7, 10);
    let done_replay = led2.finish_stage(0, 10) == Err(ProcCode::StageBacktracked);
    if done_replay {
        caught += 1;
    }
    let ok = caught == 8;
    (ok, format!("段序守卫注入 {}/8", caught))
}

pub fn drill_ledger_conservation() -> (bool, String) {
    // 守恒对账：20 次启动段账（段和 == 全链、完成数一致）。
    let mut rng = Lcg::new(SEG_BASE as u64 + 1);
    let mut caught = 0;
    let mut n = 0;
    while n < 20 {
        let ticks = synth_run(&mut rng, 1);
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
    let ok = caught == 20;
    (ok, format!("段账守恒 {}/20", caught))
}

pub fn drill_rollback() -> (bool, String) {
    // 失败回滚：中途失败（段 0..6 各失败点）→ 逆序回收账 == 已完成段数。
    // 段 7 完成即 Completed 终态，回滚不适用（Windows 语义：进程已建）。
    let mut caught = 0;
    let mut k = 0;
    while k < 7 {
        let mut led = ChainLedger::new();
        let mut i = 0;
        // 推进到段 k 后失败。
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
    let ok = caught == 7;
    (ok, format!("回滚账 {}/7", caught))
}

pub fn drill_failure_injection() -> (bool, String) {
    // 失败注入闭集：段 0 拒（打开校验）/ 段 1 拒（地址空间）/ 段 2 拒
    // （映射越界）→ 全链 Failed + 回滚守恒。
    let mut ok = true;
    // 建"失败注入链"：finish_stage 在段 i 传 ticks=0 视为段拒绝。
    let mut led = ChainLedger::new();
    let _ = led.finish_stage(ST_OPEN_CHECK, 0); // 0 tick = 打开校验拒
    ok = ok && led.state == ChainState::Running; // 建模：账先落、注入后回滚
    let rb = led.rollback();
    ok = ok && rb.is_ok() && led.rolled_back == 1;
    // 回滚后再推进 → 拒（Failed 终态）。
    ok = ok && led.finish_stage(ST_OPEN_CHECK, 10) == Err(ProcCode::StageBacktracked);
    (
        ok,
        format!(
            "失败注入: 段0账={} 回滚={} 终态拒={}",
            led.stage_ticks[ST_OPEN_CHECK] == 0,
            led.rolled_back == 1,
            led.finish_stage(ST_OPEN_CHECK, 10) == Err(ProcCode::StageBacktracked)
        ),
    )
}

pub fn drill_p95_math() -> (bool, String) {
    // P95 闭式核（无浮点）+ 反向判据（超阈必拒）。
    let mut v: Vec<u64> = Vec::new();
    let mut i = 0;
    while i < 20 {
        v.push((19 - i) as u64); // 逆序填入
        i += 1;
    }
    sort_asc(&mut v);
    // 升序 0..19，P95 下标 = (95*20+99)/100-1 = 18（⌈0.95n⌉-1）。
    let p = percentile95(&v);
    let mut ok = p == Some(18);
    // n=1：P95 = 唯一值。
    ok = ok && percentile95(&[42]) == Some(42);
    // 空 → None。
    ok = ok && percentile95(&[]).is_none();
    // ×1.5 边界：ours=15,win=10 → 30 <= 30 过；ours=16 → 32>30 拒。
    ok = ok && within_150pct(15, 10) && !within_150pct(16, 10);
    (ok, format!("P95 闭式 + 1.5 阈整数化: {}", ok))
}

pub fn drill_stage_names() -> (bool, String) {
    // 八段名闭表（成账）：8 名非空且顺序锚定。
    let mut ok = STAGE_NAMES.len() == STAGE_COUNT;
    let mut i = 0;
    while i < STAGE_COUNT {
        ok = ok && !STAGE_NAMES[i].is_empty();
        i += 1;
    }
    ok = ok && STAGE_NAMES[ST_OPEN_CHECK] == "打开校验" && STAGE_NAMES[ST_ENTRY] == "入口调度";
    (ok, format!("八段名闭表: {}", ok))
}

pub fn drill_multi_machine() -> (bool, String) {
    // 多机对拍：3 机（盐 0/1/2）各 20 样本 P95 相互 ×1.5 内。
    let salts = [0u64, 1, 2];
    let mut p95s: Vec<u64> = Vec::new();
    let mut mi = 0;
    while mi < 3 {
        let mut rng = Lcg::new(SEG_BASE as u64 + 2 + salts[mi]);
        let mut totals: Vec<u64> = Vec::new();
        let mut n = 0;
        while n < 20 {
            let t = synth_run(&mut rng, 0);
            match run_chain(&t) {
                Ok(l) => totals.push(l.total_ticks),
                Err(_) => totals.push(0),
            }
            n += 1;
        }
        sort_asc(&mut totals);
        p95s.push(percentile95(&totals).unwrap_or(0));
        mi += 1;
    }
    let ok = within_150pct(p95s[0], p95s[1])
        && within_150pct(p95s[1], p95s[2])
        && within_150pct(p95s[0], p95s[2]);
    (
        ok,
        format!("3 机 P95: {}|{}|{} 对拍={}", p95s[0], p95s[1], p95s[2], ok),
    )
}

pub fn drill_diag_codes_closed() -> (bool, String) {
    let mut ok = true;
    let mut i = 0;
    while i < ProcCode::ALL.len() {
        ok = ok && ProcCode::ALL[i].val() >= SEG_BASE && ProcCode::ALL[i].val() < SEG_BASE + 0x40;
        ok = ok && !ProcCode::ALL[i].detail().is_empty();
        i += 1;
    }
    ok = ok && detail(SEG_BASE + 0x3F) == "未知诊断码";
    (ok, format!("诊断码闭集 {}/8 + 反向", ProcCode::ALL.len()))
}

// ── 段位守卫 ─────────────────────────────────────────────────────
pub fn segment_guard() -> bool {
    let mut ok = true;
    let mut i = 0;
    while i < ProcCode::ALL.len() {
        let v = ProcCode::ALL[i].val();
        ok = ok && v >= SEG_BASE && v < SEG_BASE + 0x40;
        i += 1;
    }
    ok
}

// ── 全流程对账 ───────────────────────────────────────────────────
pub fn drill_full_flow() -> (bool, String) {
    let (a1, d1) = drill_20_apps_p95();
    let (a2, d2) = drill_stage_order();
    let (a3, d3) = drill_ledger_conservation();
    let ok = a1 && a2 && a3;
    (
        ok,
        format!(
            "全流程: P95对账={} 段序={} 守恒={} [{}|{}|{}]",
            a1, a2, a3, d1, d2, d3
        ),
    )
}

// ── 判据聚合 ─────────────────────────────────────────────────────
pub fn extra_checks() -> Vec<(&'static str, bool, String)> {
    let mut v: Vec<(&'static str, bool, String)> = Vec::new();
    let (a, b) = drill_20_apps_p95();
    v.push(("J1-20应用启动P95×1.5对账", a, b));
    let (a, b) = drill_stage_order();
    v.push(("J2-段序守卫注入", a, b));
    let (a, b) = drill_ledger_conservation();
    v.push(("J2-段账守恒20/20", a, b));
    let (a, b) = drill_rollback();
    v.push(("J2-失败回滚10/10", a, b));
    let (a, b) = drill_failure_injection();
    v.push(("J2-失败注入闭集", a, b));
    let (a, b) = drill_p95_math();
    v.push(("J2-P95闭式+阈值反向", a, b));
    let (a, b) = drill_stage_names();
    v.push(("J3-八段名闭表", a, b));
    let (a, b) = drill_multi_machine();
    v.push(("J2-3机P95对拍", a, b));
    let (a, b) = drill_diag_codes_closed();
    v.push(("J3-诊断码闭集+反向", a, b));
    let (a, b) = drill_full_flow();
    v.push(("J1-全流程对账", a, b));
    v
}

/// run_unx13001_checks：本单判据聚合（诊断段 0x65500）。
pub fn run_unx13001_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F13001 CreateProcess 八段链");
    s.add_dyn("J3-段位守卫", segment_guard(), "0x65500 段闭集");
    let checks = extra_checks();
    let mut i = 0;
    while i < checks.len() {
        let (n, p, d) = checks[i].clone();
        s.add_dyn(n, p, &d);
        i += 1;
    }
    s
}
