//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F1683 · 时间片 tick 消费与到期判据 · 0x72400
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定，CoRun·UNX-A3-B05，前置 UNX-F1682）：
//!   ① 纯功能代码 ≥320 行；
//!   ② 判据原文：UNX-F1683-J1 每 tick 剩余片递减账闭合，到期判据（剩余
//!     ≤0）确定，tick 丢失注入零静默；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! ─────────────────────────────────────────────────────────────────────
//! 设计要点：
//!   · 消费账链：每 tick 一条 TickRecord（tick_no/delta/before/after），
//!     账闭合 = 首条 before==预算 ∧ 逐条 delta==1 ∧ after==before-delta
//!     ∧ 逐条 before==前条 after ∧ 末条 after==剩余 ∧ 发出 tick 数==
//!     入账条数（记录被静默丢弃即断链，零静默）；
//!   · 到期判据：判据只读剩余值——剩余≤EXPIRE_FLOOR 即到期，与 expired
//!     标志必须一致，不一致即判据不定（ExpireAmbig）；
//!   · 状态机 Armed→Running→Expired 单调推进，平移/倒退显性拒；续片
//!     （rearm）只允许从 Expired 出发，slice_seq 严格递增；
//!   · 失真注入 10 路：删记录/重记录/改末尾 after/翻到期标志/超扣/改
//!     预算/乱序/伪造 tick_no/零增量/贴地板续扣，闭合核验 10/10 必抓；
//!   · 双跑一致：golden_run 两次同值（复现红线），段位守卫窗口 0x40。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x72400，VX3-1103 时间片窗口独占）。
pub const SEG_BASE: u32 = 0x72400;
/// 受控任务数（冻结 64）。
pub const TASK_N: usize = 64;
/// 单片预算（tick 数，冻结 8）。
pub const SLICE_BUDGET: u64 = 8;
/// 每任务续片轮数（冻结 3 片/任务）。
pub const SLICES_PER_TASK: u32 = 3;
/// 调度 tick 流长度（冻结 1536 = 64×3×8）。
pub const TICK_STREAM: u64 = TASK_N as u64 * SLICES_PER_TASK as u64 * SLICE_BUDGET;
/// 到期地板（剩余≤0 即到期，冻结 0）。
pub const EXPIRE_FLOOR: i64 = 0;
/// 判据轮数（判据 10/10 语义，冻结）。
pub const JUDGE_ROUNDS: u32 = 10;

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum QuantaCode {
    /// tick 丢失：发出 tick 数与入账条数对不上，静默丢失必抓
    TickLost,
    /// 账链断裂：before/after 链或 tick_no 序不闭合，账面被动过
    ChainBroken,
    /// 剩余片倒退：非续片路径剩余增加或 slice_seq 回退
    SliceBack,
    /// 到期判据不定：剩余≤0 与 expired 标志不一致
    ExpireAmbig,
    /// 超额消费：单 tick 扣减≠1，或贴着地板/到期后再扣
    TickOverrun,
    /// 预算被改：首条 before 与片预算对不上
    BudgetTampered,
    /// 空任务表：无任务可调度却收到消费请求
    EmptyTable,
    /// 双跑不一致：golden_run 两次应同值
    NotReproducible,
}

impl QuantaCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            QuantaCode::TickLost => "tick 丢失：发出数≠入账数，逐 tick 补账并查丢账路径",
            QuantaCode::ChainBroken => "账链断裂：before/after 链不闭合，重放 tick 流复账",
            QuantaCode::SliceBack => "剩余片倒退：剩余只减不增，续片必须走 rearm 新片",
            QuantaCode::ExpireAmbig => "到期判据不定：剩余≤0 与 expired 标志不一致，以剩余值为准校正",
            QuantaCode::TickOverrun => "超额消费：单 tick 只扣 1，到期后禁止再扣",
            QuantaCode::BudgetTampered => "预算被改：首条 before≠预算，重出片并锁定预算寄存",
            QuantaCode::EmptyTable => "空任务表：先建表再发 tick",
            QuantaCode::NotReproducible => "双跑不一致：复现红线，golden_run 两次应同值",
        }
    }
    pub const ALL: [QuantaCode; 8] = [
        QuantaCode::TickLost,
        QuantaCode::ChainBroken,
        QuantaCode::SliceBack,
        QuantaCode::ExpireAmbig,
        QuantaCode::TickOverrun,
        QuantaCode::BudgetTampered,
        QuantaCode::EmptyTable,
        QuantaCode::NotReproducible,
    ];
}

/// 诊断码查表（段外码返回统一文案）。
pub fn detail(code: u32) -> &'static str {
    let mut i = 0;
    while i < QuantaCode::ALL.len() {
        if QuantaCode::ALL[i].val() == code {
            return QuantaCode::ALL[i].detail();
        }
        i += 1;
    }
    "未知诊断码：不在 0x72400 段闭集内"
}

// ── 片状态机（Armed→Running→Expired，单调推进） ─────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum QuantaState {
    /// 备片：新片已出，一 tick 未扣。
    Armed,
    /// 消费中：至少扣了一 tick，片未到期。
    Running,
    /// 已到期：剩余≤地板，本片禁止再扣。
    Expired,
}

impl QuantaState {
    /// 单调序值：Armed=0 < Running=1 < Expired=2。
    pub fn rank(self) -> u8 {
        match self {
            QuantaState::Armed => 0,
            QuantaState::Running => 1,
            QuantaState::Expired => 2,
        }
    }
    /// 只允许向前推进；平移与倒退一律 false。
    pub fn can_advance(self, to: QuantaState) -> bool {
        to.rank() > self.rank()
    }
}

// ── 域模型 ──────────────────────────────────────────────────────
/// 单 tick 消费记录：链式账的一节（before-delta=after）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TickRecord {
    /// 全局 tick 序号（任务内严格递增）。
    pub tick_no: u64,
    /// 本 tick 扣减量（判据冻结恒为 1）。
    pub delta: u64,
    /// 扣前剩余。
    pub before: i64,
    /// 扣后剩余。
    pub after: i64,
}

/// 单片运行账：预算 + 剩余 + 到期标志 + 消费记录链。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SliceRun {
    /// 片序号（任务内严格递增，续片加一）。
    pub slice_seq: u64,
    /// 片预算（tick 数）。
    pub budget: u64,
    /// 剩余片（判据输入，唯一权威）。
    pub remaining: i64,
    /// 到期标志（必须与 remaining≤0 一致）。
    pub expired: bool,
    /// 消费记录链（逐 tick 追加，不许删改）。
    pub records: Vec<TickRecord>,
}

/// 受控任务的片状态机 + 当前片账。
#[derive(Clone, Debug)]
pub struct TaskQuanta {
    pub tid: u32,
    pub state: QuantaState,
    pub run: SliceRun,
    /// 历史续片次数（单调递增）。
    pub rearms: u32,
    /// 最后一次发出的片序号。
    pub last_slice_seq: u64,
}

/// 出新片（确定性：预算固定，剩余=预算，未到期）。
pub fn new_slice(slice_seq: u64, budget: u64) -> SliceRun {
    SliceRun {
        slice_seq,
        budget,
        remaining: budget as i64,
        expired: false,
        records: Vec::new(),
    }
}

/// 建任务表（确定性：tid 由下标派生，首片序号 = tid+1）。
pub fn build_table(n: usize) -> Vec<TaskQuanta> {
    let mut v: Vec<TaskQuanta> = Vec::new();
    let mut i = 0usize;
    while i < n {
        v.push(TaskQuanta {
            tid: i as u32,
            state: QuantaState::Armed,
            run: new_slice((i + 1) as u64, SLICE_BUDGET),
            rearms: 0,
            last_slice_seq: (i + 1) as u64,
        });
        i += 1;
    }
    v
}

// ── tick 发放（消费账追加口，唯一写路径） ───────────────────────
/// 发一个 tick：Armed→Running 推进；剩余减一；贴地板即置到期。
/// 到期后再发 / 状态倒退一律拒。
pub fn issue_tick(task: &mut TaskQuanta, tick_no: u64) -> Result<TickRecord, u32> {
    if task.state == QuantaState::Expired {
        return Err(QuantaCode::TickOverrun.val());
    }
    if task.run.remaining <= EXPIRE_FLOOR {
        return Err(QuantaCode::TickOverrun.val());
    }
    if task.state == QuantaState::Armed {
        if !task.state.can_advance(QuantaState::Running) {
            return Err(QuantaCode::SliceBack.val());
        }
        task.state = QuantaState::Running;
    }
    // tick_no 必须与账内序严格对位（片基址 0x1000 + 账内下标），防伪造/重放。
    let want_no = (task.run.slice_seq as u64).saturating_mul(0x1000)
        + task.run.records.len() as u64;
    if tick_no != want_no {
        return Err(QuantaCode::ChainBroken.val());
    }
    let before = task.run.remaining;
    let delta = 1u64;
    let after = before - delta as i64;
    let rec = TickRecord { tick_no, delta, before, after };
    task.run.records.push(rec);
    task.run.remaining = after;
    task.run.expired = task.run.remaining <= EXPIRE_FLOOR;
    if task.run.expired {
        if !task.state.can_advance(QuantaState::Expired) {
            return Err(QuantaCode::SliceBack.val());
        }
        task.state = QuantaState::Expired;
    }
    Ok(rec)
}

/// 续片：只允许从 Expired 出发；片序号严格递增；倒退显性拒。
pub fn rearm(task: &mut TaskQuanta, budget: u64) -> Result<(), u32> {
    if task.state != QuantaState::Expired {
        return Err(QuantaCode::SliceBack.val());
    }
    let seq = task.last_slice_seq
        .checked_add(1)
        .ok_or(QuantaCode::SliceBack.val())?;
    if seq <= task.run.slice_seq {
        return Err(QuantaCode::SliceBack.val());
    }
    task.run = new_slice(seq, budget);
    task.state = QuantaState::Armed;
    task.last_slice_seq = seq;
    task.rearms = task.rearms.saturating_add(1);
    Ok(())
}

// ── 账闭合核验（判据主口） ──────────────────────────────────────
/// 每 tick 剩余片递减账闭合：
///   ① 链首 before==预算；② 逐条 delta==1 且 before>地板；
///   ③ after==before-delta 且逐条 before==前条 after；
///   ④ tick_no 严格递增；⑤ 末条 after==剩余；⑥ 发出数==入账数；
///   ⑦ expired 与 remaining≤0 一致。
pub fn close_run(run: &SliceRun, issued: u64) -> Result<(), u32> {
    if run.records.is_empty() {
        if issued == 0 && run.remaining == run.budget as i64 && !run.expired {
            return Ok(());
        }
        if issued != 0 {
            return Err(QuantaCode::TickLost.val());
        }
        return Err(QuantaCode::ChainBroken.val());
    }
    let first = match run.records.first() {
        Some(r) => *r,
        None => return Err(QuantaCode::ChainBroken.val()),
    };
    if first.before != run.budget as i64 {
        return Err(QuantaCode::BudgetTampered.val());
    }
    let mut prev = first;
    let mut i = 1usize;
    while i < run.records.len() {
        let cur = match run.records.get(i) {
            Some(r) => *r,
            None => return Err(QuantaCode::ChainBroken.val()),
        };
        if cur.tick_no <= prev.tick_no {
            return Err(QuantaCode::ChainBroken.val());
        }
        if cur.before != prev.after {
            return Err(QuantaCode::ChainBroken.val());
        }
        prev = cur;
        i += 1;
    }
    let mut i = 0usize;
    while i < run.records.len() {
        let r = match run.records.get(i) {
            Some(r) => *r,
            None => return Err(QuantaCode::ChainBroken.val()),
        };
        if r.delta != 1 {
            return Err(QuantaCode::TickOverrun.val());
        }
        if r.before <= EXPIRE_FLOOR {
            return Err(QuantaCode::TickOverrun.val());
        }
        if r.before - r.delta as i64 != r.after {
            return Err(QuantaCode::ChainBroken.val());
        }
        i += 1;
    }
    let last = match run.records.last() {
        Some(r) => *r,
        None => return Err(QuantaCode::ChainBroken.val()),
    };
    if last.after != run.remaining {
        return Err(QuantaCode::ChainBroken.val());
    }
    if run.records.len() as u64 != issued {
        return Err(QuantaCode::TickLost.val());
    }
    if run.expired != (run.remaining <= EXPIRE_FLOOR) {
        return Err(QuantaCode::ExpireAmbig.val());
    }
    Ok(())
}

// ── 失真注入（10 路逐字失真，tick 丢失零静默） ──────────────────
/// 对已闭合的单片账注入第 kind 路失真，返回坏账（值域全整数，安全）。
pub fn distort(run: &SliceRun, kind: usize) -> SliceRun {
    let mut bad = run.clone();
    let n = bad.records.len();
    match kind % 10 {
        0 => {
            // 删首条：发出数不变而入账少一条（静默丢 tick）。
            if n > 0 {
                bad.records.remove(0);
            }
        }
        1 => {
            // 重记录：首条复制插到第二位（链断裂）。
            if n > 0 {
                let dup = bad.records[0];
                bad.records.insert(1.min(n), dup);
            }
        }
        2 => {
            // 改末条 after：末条与剩余对不上。
            if let Some(l) = bad.records.last_mut() {
                l.after = l.after.saturating_add(1);
            }
        }
        3 => {
            // 翻到期标志：剩余>0 却标已到期（判据不定）。
            bad.expired = !bad.expired;
        }
        4 => {
            // 超扣：中部一条 delta 抬成 2。
            let mid = n / 2;
            if let Some(r) = bad.records.get_mut(mid) {
                r.delta = 2;
            }
        }
        5 => {
            // 改预算：首条 before 与预算对不上。
            bad.budget = bad.budget.saturating_add(1);
        }
        6 => {
            // 乱序：前两条对调（tick_no 不再递增）。
            if n >= 2 {
                let a = bad.records[0];
                bad.records[0] = bad.records[1];
                bad.records[1] = a;
            }
        }
        7 => {
            // 伪造 tick_no：第二条序号改成与首条相同（重放）。
            if n >= 2 {
                let t = bad.records[0].tick_no;
                bad.records[1].tick_no = t;
            }
        }
        8 => {
            // 零增量：首条 delta 抹成 0（空转过账）。
            if let Some(r) = bad.records.first_mut() {
                r.delta = 0;
            }
        }
        _ => {
            // 贴地板续扣：追加一条 before=0 的越界记录。
            let last_no = match bad.records.last() {
                Some(r) => r.tick_no,
                None => 0,
            };
            bad.records.push(TickRecord { tick_no: last_no + 1, delta: 1, before: 0, after: -1 });
        }
    }
    bad
}

/// 注入检出账。
pub struct InjectLedger {
    pub tries: u32,
    pub caught: u32,
    /// 漏抓的失真路号（诊断用）。
    pub miss_kinds: Vec<usize>,
}

impl InjectLedger {
    pub fn new() -> InjectLedger {
        InjectLedger { tries: 0, caught: 0, miss_kinds: Vec::new() }
    }
    pub fn record(&mut self, kind: usize, hit: bool) {
        self.tries += 1;
        if hit {
            self.caught += 1;
        } else {
            self.miss_kinds.push(kind);
        }
    }
    pub fn all_caught(&self) -> bool {
        self.tries > 0 && self.caught == self.tries
    }
}

/// 完整单片账（黄金基准）：SLICE_BUDGET 个 tick 逐条入账。
pub fn golden_slice(slice_seq: u64) -> Result<SliceRun, u32> {
    let mut run = new_slice(slice_seq, SLICE_BUDGET);
    let base = slice_seq * 0x1000;
    let mut t = 0u64;
    while t < SLICE_BUDGET {
        let before = run.remaining;
        let after = before - 1;
        run.records.push(TickRecord {
            tick_no: base + t,
            delta: 1,
            before,
            after,
        });
        run.remaining = after;
        t += 1;
    }
    run.expired = run.remaining <= EXPIRE_FLOOR;
    close_run(&run, SLICE_BUDGET)?;
    Ok(run)
}

/// 注入轮：每轮对新鲜黄金账注入一路失真，闭合核验必须抓到
/// （黄金账本身必须核验通过，防注入器污染基准）。
pub fn inject_rounds(rounds: u32) -> Result<InjectLedger, u32> {
    let mut led = InjectLedger::new();
    let mut r = 0u32;
    while r < rounds {
        let run = golden_slice(0x10 + r as u64)?;
        let kind = (r % 10) as usize;
        led.record(kind, close_run(&distort(&run, kind), SLICE_BUDGET).is_err());
        r += 1;
    }
    Ok(led)
}

// ── 调度流（tick 逐 tick 发放 + 逐片闭合） ─────────────────────
/// 确定性调度：每任务 SLICES_PER_TASK 片，每片 SLICE_BUDGET tick，
/// 逐片闭合；返回 (消费总数, 到期片数)。
pub fn schedule_all(tasks: &mut Vec<TaskQuanta>) -> Result<(u64, u64), u32> {
    if tasks.is_empty() {
        return Err(QuantaCode::EmptyTable.val());
    }
    let mut consumed: u64 = 0;
    let mut expired_slices: u64 = 0;
    let mut i = 0usize;
    while i < tasks.len() {
        let mut s = 0u32;
        while s < SLICES_PER_TASK {
            if s > 0 {
                rearm(&mut tasks[i], SLICE_BUDGET)?;
            }
            let base = tasks[i].run.slice_seq * 0x1000;
            let mut t = 0u64;
            while t < SLICE_BUDGET {
                issue_tick(&mut tasks[i], base + t)?;
                t += 1;
            }
            let issued = SLICE_BUDGET;
            close_run(&tasks[i].run, issued)?;
            if !tasks[i].run.expired {
                return Err(QuantaCode::ExpireAmbig.val());
            }
            expired_slices += 1;
            consumed += issued;
            s += 1;
        }
        i += 1;
    }
    Ok((consumed, expired_slices))
}

// ── 黄金全链 ────────────────────────────────────────────────────
/// 全链：调度全表逐片闭合 → 注入 10/10 → 四拒探（空表/运行中续片/
/// 到期再扣/丢 tick）。
/// 返回 (正旗数, 拒旗数, 消费总数, 到期片数)。
pub fn golden_run() -> Result<(u32, u32, u64, u64), u32> {
    let mut tasks = build_table(TASK_N);
    let (consumed, expired_slices) = schedule_all(&mut tasks)?;
    // 到期确定性：每片恰好扣满预算贴地板，末条 after==0。
    let mut t0 = 0usize;
    while t0 < tasks.len() {
        let last = match tasks[t0].run.records.last() {
            Some(r) => *r,
            None => return Err(QuantaCode::ChainBroken.val()),
        };
        if last.after != EXPIRE_FLOOR || tasks[t0].run.remaining != EXPIRE_FLOOR {
            return Err(QuantaCode::ExpireAmbig.val());
        }
        t0 += 1;
    }
    // 注入 10 路全抓。
    let inj = inject_rounds(JUDGE_ROUNDS)?;
    let inj_ok = inj.all_caught();
    // 四拒探。
    let mut probe = build_table(1);
    let empty_rej = schedule_all(&mut Vec::new()).is_err();
    // 运行中续片拒：扣一 tick（Running）就 rearm。
    let base = probe[0].run.slice_seq * 0x1000;
    let _ = issue_tick(&mut probe[0], base)?;
    let running_rej = rearm(&mut probe[0], SLICE_BUDGET).is_err();
    // 把片扣到到期（首 tick 已扣，从 base+1 续扣），再扣拒。
    let mut t = 1u64;
    while t < SLICE_BUDGET {
        let _ = issue_tick(&mut probe[0], base + t)?;
        t += 1;
    }
    let after_expire_rej = issue_tick(&mut probe[0], base + SLICE_BUDGET).is_err();
    // 丢 tick 拒：发出数与入账数不一致。
    let lost_rej = close_run(&probe[0].run, SLICE_BUDGET + 1).is_err();
    let flags = (consumed == TICK_STREAM) as u32
        + (expired_slices == TASK_N as u64 * SLICES_PER_TASK as u64) as u32
        + inj_ok as u32
        + (tasks.len() == TASK_N) as u32;
    let rejects = empty_rej as u32 + running_rej as u32 + after_expire_rej as u32 + lost_rej as u32;
    Ok((flags, rejects, consumed, expired_slices))
}

// ── 探针三序 ────────────────────────────────────────────────────
/// 序 A：黄金全链四正四拒全中。
pub fn probe_seq_a() -> Result<(), u32> {
    match golden_run() {
        Ok((4, 4, _, _)) => Ok(()),
        _ => Err(QuantaCode::NotReproducible.val()),
    }
}

/// 序 B：失真注入 10 路全抓。
pub fn probe_seq_b() -> Result<(), u32> {
    match inject_rounds(JUDGE_ROUNDS) {
        Ok(l) if l.all_caught() => Ok(()),
        _ => Err(QuantaCode::TickLost.val()),
    }
}

/// 序 C：冻结常量与段位窗口。
pub fn probe_seq_c() -> Result<(), u32> {
    let in_window = SEG_BASE == 0x72400 && QuantaCode::NotReproducible.val() < SEG_BASE + 0x40;
    let ok = TASK_N == 64 && SLICE_BUDGET == 8 && SLICES_PER_TASK == 3
        && TICK_STREAM == 1536 && EXPIRE_FLOOR == 0 && JUDGE_ROUNDS == 10 && in_window;
    if ok {
        Ok(())
    } else {
        Err(QuantaCode::NotReproducible.val())
    }
}

// ── 双跑一致（复现红线） ────────────────────────────────────────
pub fn repro_check() -> Result<(), u32> {
    let a = golden_run()?;
    let b = golden_run()?;
    if a != b {
        return Err(QuantaCode::NotReproducible.val());
    }
    Ok(())
}

// ── 段位守卫 ────────────────────────────────────────────────────
pub fn segment_guard() -> bool {
    let mut i = 0;
    while i < QuantaCode::ALL.len() {
        let v = QuantaCode::ALL[i].val();
        if v < SEG_BASE || v >= SEG_BASE + 0x40 {
            return false;
        }
        i += 1;
    }
    true
}

// ── 状态机单调探针 ──────────────────────────────────────────────
/// Armed<Running<Expired 全序 + 平移/倒退拒 + 续片序号递增。
pub fn probe_state_machine() -> bool {
    let ranks = QuantaState::Armed.rank() < QuantaState::Running.rank()
        && QuantaState::Running.rank() < QuantaState::Expired.rank();
    let flat1 = !QuantaState::Armed.can_advance(QuantaState::Armed);
    let back1 = !QuantaState::Expired.can_advance(QuantaState::Running);
    let back2 = !QuantaState::Running.can_advance(QuantaState::Armed);
    let fwd1 = QuantaState::Armed.can_advance(QuantaState::Running);
    let fwd2 = QuantaState::Running.can_advance(QuantaState::Expired);
    // 续片序号：Expired 可续，Armed/Running 拒，且 seq 递增。
    let mut t = build_table(1);
    let seq0 = t[0].run.slice_seq;
    let base0 = t[0].run.slice_seq * 0x1000;
    let mut drove_ok = true;
    let mut k = 0u64;
    while k < SLICE_BUDGET {
        if issue_tick(&mut t[0], base0 + k).is_err() {
            drove_ok = false;
        }
        k += 1;
    }
    let mut t2 = build_table(1);
    let base2 = t2[0].run.slice_seq * 0x1000;
    let _ = issue_tick(&mut t2[0], base2);
    let rearm_running_rej = rearm(&mut t2[0], SLICE_BUDGET).is_err();
    let ok_rearm = rearm(&mut t[0], SLICE_BUDGET).is_ok();
    let seq_up = t[0].run.slice_seq == seq0 + 1 && t[0].rearms == 1;
    ranks && flat1 && back1 && back2 && fwd1 && fwd2 && rearm_running_rej && drove_ok && ok_rearm && seq_up
}

// ── 判据确定性探针：到期只由剩余值决定 ─────────────────────────
/// 到期边界三查：剩余 1 未到期、剩余 0 到期、剩余 -1（越界扣后）到期。
pub fn probe_expiry_judge() -> bool {
    let mut t = build_table(1);
    let base = t[0].run.slice_seq * 0x1000;
    // 扣 7 tick：剩 1，未到期。
    let mut k = 0u64;
    while k < SLICE_BUDGET - 1 {
        if issue_tick(&mut t[0], base + k).is_err() {
            return false;
        }
        k += 1;
    }
    let not_yet = t[0].run.remaining == 1 && !t[0].run.expired && t[0].state == QuantaState::Running;
    // 第 8 tick：剩 0，到期。
    let last = issue_tick(&mut t[0], base + SLICE_BUDGET - 1);
    let now = t[0].run.remaining == 0 && t[0].run.expired && t[0].state == QuantaState::Expired && last.is_ok();
    // 到期后再扣必拒。
    let rej = issue_tick(&mut t[0], base + SLICE_BUDGET).is_err();
    // 判据单向：剩余≤0 ⇒ expired（golden_slice 全量复核）。
    let g = match golden_slice(0x77) {
        Ok(r) => r.expired && r.remaining == 0,
        Err(_) => false,
    };
    not_yet && now && rej && g
}

// ── 消费记录确定性探针 ─────────────────────────────────────────
/// 记录链逐字段确定性：tick_no 对位、before/after 首尾锚定。
pub fn probe_record_chain() -> bool {
    let run = match golden_slice(0x33) {
        Ok(r) => r,
        Err(_) => return false,
    };
    if run.records.len() as u64 != SLICE_BUDGET {
        return false;
    }
    let f = match run.records.first() {
        Some(r) => *r,
        None => return false,
    };
    let l = match run.records.last() {
        Some(r) => *r,
        None => return false,
    };
    let head_ok = f.before == SLICE_BUDGET as i64 && f.tick_no == 0x33 * 0x1000;
    let tail_ok = l.after == 0 && l.tick_no == 0x33 * 0x1000 + SLICE_BUDGET - 1;
    // 中段逐条抽验：before == 上条 after，after == before-1。
    let mut mid_ok = true;
    let mut i = 1usize;
    while i < run.records.len() {
        let a = run.records[i - 1];
        let b = run.records[i];
        if b.before != a.after || b.after != b.before - 1 {
            mid_ok = false;
        }
        i += 1;
    }
    head_ok && tail_ok && mid_ok
}

// ── 完成写回（判据③：状态=已完成、结果摘要非空） ────────────────
pub fn writeback_summary() -> Result<String, u32> {
    let mut tasks = build_table(TASK_N);
    let (consumed, expired_slices) = schedule_all(&mut tasks)?;
    let inj = inject_rounds(JUDGE_ROUNDS)?;
    if !inj.all_caught() {
        return Err(QuantaCode::TickLost.val());
    }
    Ok(format!(
        "状态=已完成 tasks={} slices={} consumed={} expired={} inject={}/{}",
        tasks.len(), expired_slices, consumed, expired_slices, inj.caught, inj.tries
    ))
}

// ── 判据面 ──────────────────────────────────────────────────────
pub fn extra_checks() -> Vec<(&'static str, bool, String)> {
    let mut v: Vec<(&'static str, bool, String)> = Vec::new();
    // J1 主判据：黄金全链（四正四拒）。
    let g = golden_run();
    let (gp, gd) = match &g {
        Ok((f, r, c, e)) => (*f == 4 && *r == 4, format!("flags={} rejects={} consumed={} expired={}", f, r, c, e)),
        Err(e) => (false, format!("err=0x{:x} {}", e, detail(*e))),
    };
    v.push(("J1-黄金全链", gp, gd));
    // J1 每 tick 递减账闭合：全表逐片闭合 + 记录链抽验。
    let mut tasks = build_table(TASK_N);
    let closed = schedule_all(&mut tasks).is_ok();
    let mut chain_ok = true;
    let mut ti = 0usize;
    while ti < tasks.len() {
        if close_run(&tasks[ti].run, SLICE_BUDGET).is_err() {
            chain_ok = false;
        }
        ti += 1;
    }
    v.push((
        "J1-递减账闭合",
        closed && chain_ok && probe_record_chain(),
        format!("tasks={} slices/任务={} 全闭合={}", TASK_N, SLICES_PER_TASK, chain_ok),
    ));
    // J1 到期判据确定。
    v.push(("J1-到期判据确定", probe_expiry_judge(), String::from("剩余≤0 即到期；剩1未到期；到期再扣必拒")));
    // J1 tick 丢失注入零静默。
    let inj = inject_rounds(JUDGE_ROUNDS);
    let (ip, id) = match &inj {
        Ok(l) => (l.all_caught(), format!("t={} c={} miss={:?}", l.tries, l.caught, l.miss_kinds)),
        Err(e) => (false, format!("err=0x{:x}", e)),
    };
    v.push(("J1-tick丢失零静默", ip, id));
    // J2 状态机单调 + 续片守卫。
    v.push(("J2-状态机单调", probe_state_machine(), String::from("Armed<Running<Expired；平移/倒退/运行中续片全拒")));
    // J2 空表守卫。
    let empty_probe = schedule_all(&mut Vec::new());
    let (empty_rej, empty_code) = match empty_probe {
        Err(c) => (true, c),
        Ok(_) => (false, 0u32),
    };
    v.push((
        "J2-空表守卫",
        empty_rej && empty_code == QuantaCode::EmptyTable.val(),
        format!("空表拒={} 码=0x{:x}", empty_rej, empty_code),
    ));
    // J2 双跑一致。
    v.push(("J2-双跑一致", repro_check().is_ok(), String::from("golden_run 两跑同值")));
    // J2 冻结常量。
    v.push((
        "J2-冻结常量",
        probe_seq_c().is_ok(),
        format!("TASK_N={} SLICE_BUDGET={} TICK_STREAM={}", TASK_N, SLICE_BUDGET, TICK_STREAM),
    ));
    // J3 写回摘要非空。
    let wb = writeback_summary();
    let (wp, wd) = match &wb {
        Ok(s) => (!s.is_empty(), s.clone()),
        Err(e) => (false, format!("err=0x{:x}", e)),
    };
    v.push(("J3-写回摘要", wp, wd));
    // J4 段位守卫。
    v.push(("J4-段位守卫", segment_guard(), String::from("0x72400 窗口")));
    v
}

pub fn run_unx1683_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F1683 时间片 tick 消费与到期判据");
    for (n, p, d) in extra_checks() {
        s.add_dyn(n, p, &d);
    }
    s
}
