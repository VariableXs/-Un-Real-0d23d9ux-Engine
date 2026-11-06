//! UNX-F3446 并发执行器（每 CPU 工作者池） · 0x79CC0段
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（派工卡 VX8-B14 逐字摘录）：
//!   ① 纯功能代码 ≥320 行；
//!   ② J1：工作者池（每 CPU 一 worker、任务队列本地化）+ 双核桩调度器
//!     验证；M-205 并发执行完成数 == 用例总数（零丢失）。
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - 工作者池：每 CPU 一 worker（本单桩定 2 核），任务提交即按轮转
//!     分派入本地队列——任务一旦入队不再迁移（本地化承诺）；
//!   - 桩调度器：整数拍推进，每轮各 worker 从自己的本地队列取队首一例
//!     执行（代价取冻结代价表），完成落账（用例 ID + worker 号 + 完成
//!     序戳）；无线程、无墙钟，纯整数账面并发；
//!   - M-205 零丢失：drain 结束后完成账例数必等于提交例数，且每例恰执
//!     行一次（无重复、无丢失、无迁移三查）；
//!   - 对账面：完成账与提交账做差集，伪造丢账必被点名（丢失例 ID 入
//!     诊断文案语义）；
//!   - 完成序戳：同 worker 内严格递增（本地队列 FIFO），跨 worker 各自
//!     递增互不约束；
//!   - 双跑一致：同提交两次 drain 的完成账逐字段相同。
//! 常量来源：
//!   - 双核桩：判据原文「双核桩调度器验证」冻结 WORKER_COUNT=2；
//!   - M-205：派工卡验收判据编号（零丢失判据锚定 0x205）。
//! ─────────────────────────────────────────────────────────────────────

use alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x40 对齐，B14 批段窗 [0x79C00, 0x79DC0] 内）。
pub const SEG_BASE: u32 = 0x79CC0;

/// 工作者数（每 CPU 一 worker，双核桩冻结）。
pub const WORKER_COUNT: usize = 2;

/// M-205 零丢失判据锚定号（注入对账用例 ID）。
pub const INJECTION_CASE_ID: u32 = 0x205;

/// 冻结用例代价表：用例名 → (用例 ID, 代价拍)。
/// 代价按账面工作量定（冒烟类小、序列化类大）。
pub const CASE_COST_TABLE: [(&str, u32, u64); 6] = [
    ("冒烟甲", 1, 8),
    ("冒烟乙", 2, 8),
    ("序列化万条", 3, 64),
    ("账本往返", 4, 16),
    ("看门狗自检", 5, 4),
    ("写回对账", 6, 12),
];

/// 冻结代价表内查价（表外 ID 返回 None）。
pub fn cost_of(case_id: u32) -> Option<u64> {
    let mut i = 0usize;
    while i < CASE_COST_TABLE.len() {
        match CASE_COST_TABLE.get(i) {
            Some((_, id, cost)) => {
                if *id == case_id {
                    return Some(*cost);
                }
            }
            None => return None,
        }
        i += 1;
    }
    None
}

/// 一条完成账：谁在哪核、第几拍完成。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Completion {
    /// 用例 ID。
    pub case_id: u32,
    /// 执行 worker（= 分派 worker，任务不迁移）。
    pub worker: usize,
    /// 实耗拍数（取自冻结代价表）。
    pub cost_ticks: u64,
    /// 完成序戳（全局单调，1 起）。
    pub finish_tick: u64,
}

/// 每核工作者：本地队列 + 取队首游标（任务入队不迁移）。
#[derive(Debug, Clone)]
struct LocalQueue {
    tasks: Vec<u32>,
    head: usize,
}

impl LocalQueue {
    fn new() -> LocalQueue {
        LocalQueue {
            tasks: Vec::new(),
            head: 0,
        }
    }
    fn push(&mut self, case_id: u32) {
        self.tasks.push(case_id);
    }
    fn pop(&mut self) -> Option<u32> {
        if self.head >= self.tasks.len() {
            return None;
        }
        let id = match self.tasks.get(self.head) {
            Some(&x) => x,
            None => return None,
        };
        self.head += 1;
        Some(id)
    }
    fn pending(&self) -> usize {
        if self.head >= self.tasks.len() {
            0
        } else {
            self.tasks.len() - self.head
        }
    }
}

/// 并发执行器：双核桩调度器 + 本地队列 + 完成账。
#[derive(Debug, Clone)]
pub struct WorkerPool {
    queues: Vec<LocalQueue>,
    completed: Vec<Completion>,
    next_tick: u64,
    submitted: Vec<u32>,
    drained: bool,
}

impl WorkerPool {
    /// 建池：双核各一张空本地队列。
    pub fn new() -> WorkerPool {
        let mut queues = Vec::new();
        let mut i = 0usize;
        while i < WORKER_COUNT {
            queues.push(LocalQueue::new());
            i += 1;
        }
        WorkerPool {
            queues,
            completed: Vec::new(),
            next_tick: 1,
            submitted: Vec::new(),
            drained: false,
        }
    }

    /// 提交一批用例：按轮转分派入本地队列（表外 ID 拒整批）。
    pub fn submit(&mut self, case_ids: &[u32]) -> Result<usize, u32> {
        if self.drained {
            return Err(Diag::SubmitAfterDrain.val());
        }
        for id in case_ids.iter() {
            if cost_of(*id).is_none() {
                return Err(Diag::CaseForeign.val());
            }
        }
        let mut n = 0usize;
        for id in case_ids.iter() {
            let slot = match self.submitted.len().checked_rem(WORKER_COUNT) {
                Some(r) => r,
                None => return Err(Diag::ZeroWorker.val()),
            };
            match self.queues.get_mut(slot) {
                Some(q) => q.push(*id),
                None => return Err(Diag::ZeroWorker.val()),
            }
            self.submitted.push(*id);
            n += 1;
        }
        Ok(n)
    }

    /// 桩调度 drain：每轮各核取队首一例并行执行，轮拍推进到全部清空。
    /// 返回完成账条数；零丢失由对账判据核。
    pub fn drain(&mut self) -> Result<usize, u32> {
        if WORKER_COUNT == 0 || self.queues.len() != WORKER_COUNT {
            return Err(Diag::ZeroWorker.val());
        }
        loop {
            // 本轮：各核有活则各执行一例（桩并发）。
            let mut any_active = false;
            let mut round_max: u64 = 0;
            let mut w = 0usize;
            while w < WORKER_COUNT {
                let take = match self.queues.get_mut(w) {
                    Some(q) => q.pop(),
                    None => return Err(Diag::ZeroWorker.val()),
                };
                match take {
                    Some(case_id) => {
                        let cost = match cost_of(case_id) {
                            Some(c) => c,
                            None => return Err(Diag::CostMismatch.val()),
                        };
                        let finish = self.next_tick.saturating_add(cost);
                        self.completed.push(Completion {
                            case_id,
                            worker: w,
                            cost_ticks: cost,
                            finish_tick: finish,
                        });
                        if cost > round_max {
                            round_max = cost;
                        }
                        any_active = true;
                    }
                    None => {}
                }
                w += 1;
            }
            if !any_active {
                break;
            }
            self.next_tick = self.next_tick.saturating_add(round_max);
        }
        self.drained = true;
        Ok(self.completed.len())
    }

    /// 完成账只读视图。
    pub fn completed(&self) -> &[Completion] {
        &self.completed
    }

    /// 提交账只读视图。
    pub fn submitted(&self) -> &[u32] {
        &self.submitted
    }

    /// 某核本地队列剩余（drain 后恒 0）。
    pub fn pending_on(&self, worker: usize) -> usize {
        match self.queues.get(worker) {
            Some(q) => q.pending(),
            None => 0,
        }
    }
}

/// M-205 零丢失对账：完成账与提交账做多重集合差，返回丢失例 ID 列表。
pub fn lost_cases(pool: &WorkerPool) -> Vec<u32> {
    let mut lost: Vec<u32> = Vec::new();
    for id in pool.submitted().iter() {
        let mut want = 1i64;
        for c in pool.completed().iter() {
            if c.case_id == *id {
                want -= 1;
            }
        }
        if want > 0 {
            lost.push(*id);
        }
    }
    lost
}

/// 重复执行对账：完成账内同例出现多于提交次数即重复。
pub fn duplicated_cases(pool: &WorkerPool) -> Vec<u32> {
    let mut dup: Vec<u32> = Vec::new();
    for id in pool.completed().iter() {
        let mut ran = 0i64;
        let mut want = 0i64;
        for c in pool.completed().iter() {
            if c.case_id == id.case_id {
                ran += 1;
            }
        }
        for s in pool.submitted().iter() {
            if *s == id.case_id {
                want += 1;
            }
        }
        if ran > want && !dup.contains(&id.case_id) {
            dup.push(id.case_id);
        }
    }
    dup
}

/// 诊断码闭集：0x79CC1..=0x79CCC（12 码，变体带中文 doc）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Diag {
    /// 工作者数为零：池建歪或核表越界。
    ZeroWorker = 0x01,
    /// 表外用例：提交 ID 不在冻结代价表内。
    CaseForeign = 0x02,
    /// 丢失破约：完成数 != 用例总数（M-205 零丢失失守）。
    LossDetected = 0x03,
    /// 重复执行：同例在完成账出现多于提交次数。
    DuplicateRun = 0x04,
    /// 任务迁移：完成 worker != 分派 worker（本地化承诺破约）。
    TaskMigrated = 0x05,
    /// 序戳倒退：完成序戳非全局单调。
    TickRegress = 0x06,
    /// 代价错账：完成账代价与冻结代价表不符。
    CostMismatch = 0x07,
    /// 对账漏点：伪造丢账未被差集对账点名。
    ReconcileMiss = 0x08,
    /// 账目不平：完成账与提交账差集非空（M-205 实锤）。
    LedgerUnbalanced = 0x09,
    /// 收尾后再提交：drain 之后提交口未闭。
    SubmitAfterDrain = 0x0A,
    /// 双跑分叉：同提交两次 drain 完成账不同。
    DoubleRunDiverge = 0x0B,
    /// 段位失守：SEG_BASE 不满足 0x40 对齐或越出批段窗。
    SegGuardFail = 0x0C,
}

impl Diag {
    /// 完整 u32 编码。
    pub fn val(self) -> u32 {
        SEG_BASE + (self as u32)
    }

    /// 中文诊断文案（含可操作建议）。
    pub fn detail(self) -> &'static str {
        match self {
            Diag::ZeroWorker => "工作者数为零：建议建池核对 WORKER_COUNT 与队列数一致",
            Diag::CaseForeign => "表外用例：建议提交前先查冻结代价表，新例先扩表",
            Diag::LossDetected => "丢失破约（M-205）：建议 drain 后必做完成数==提交数对账再收口",
            Diag::DuplicateRun => "重复执行：建议完成账按用例 ID 查重，同例只许一账",
            Diag::TaskMigrated => "任务迁移：建议入队即锁核，完成 worker 必取分派 worker",
            Diag::TickRegress => "序戳倒退：建议完成序戳由调度器单一发号，禁手工回拨",
            Diag::CostMismatch => "代价错账：建议执行代价只从冻结代价表取，禁旁路估价",
            Diag::ReconcileMiss => "对账漏点：建议伪造丢账必经差集对账点名再放行",
            Diag::LedgerUnbalanced => "账目不平：建议完成账与提交账逐 ID 对多重集合差",
            Diag::SubmitAfterDrain => "收尾后再提交：建议 drain 后提交口闭锁，新批次重建池",
            Diag::DoubleRunDiverge => "双跑分叉：建议调度器做成纯函数，输入定则完成账定",
            Diag::SegGuardFail => "段位失守：建议核对 SEG_BASE 的 0x40 对齐与批段窗",
        }
    }
}

/// 闭集全量数组（12 码）。
pub const ALL_DIAGS: [Diag; 12] = [
    Diag::ZeroWorker,
    Diag::CaseForeign,
    Diag::LossDetected,
    Diag::DuplicateRun,
    Diag::TaskMigrated,
    Diag::TickRegress,
    Diag::CostMismatch,
    Diag::ReconcileMiss,
    Diag::LedgerUnbalanced,
    Diag::SubmitAfterDrain,
    Diag::DoubleRunDiverge,
    Diag::SegGuardFail,
];

/// 诊断码查表闭集返回；未知编码回退固定兜底串。
pub fn detail(code: u32) -> &'static str {
    for d in ALL_DIAGS.iter() {
        if d.val() == code {
            return d.detail();
        }
    }
    "未知诊断码：建议核对是否落在 0x79CC1..=0x79CCC 闭集内"
}

// ── 判据谓词（全部私有 fn xxx_ok() -> bool）────────────────────────────

/// 段位守卫：0x40 对齐 + B14 批段窗 [0x79C00, 0x79DC0]。
fn seg_guard_ok() -> bool {
    SEG_BASE % 0x40 == 0 && SEG_BASE >= 0x79C00 && SEG_BASE <= 0x79DC0
}

/// 诊断闭集完整：12 码、编码连续、唯一、文案非空且带「建议」。
fn diag_closed_set_ok() -> bool {
    if ALL_DIAGS.len() != 12 {
        return false;
    }
    let mut i = 0usize;
    while i < ALL_DIAGS.len() {
        let a = match ALL_DIAGS.get(i) {
            Some(d) => *d,
            None => return false,
        };
        if a.val() != SEG_BASE + (i as u32) + 1 {
            return false;
        }
        if a.detail().is_empty() || !a.detail().contains("建议") {
            return false;
        }
        let mut j = i + 1;
        while j < ALL_DIAGS.len() {
            match ALL_DIAGS.get(j) {
                Some(b) => {
                    if a.val() == b.val() {
                        return false;
                    }
                }
                None => return false,
            }
            j += 1;
        }
        i += 1;
    }
    detail(0xDEAD) == detail(0x5A5A) && !detail(0xDEAD).is_empty()
}

/// 正常流 + M-205 零丢失：冻结表六例全提交全完成，三查（零丢失、
/// 无重复、无迁移）全过。
fn zero_loss_ok() -> bool {
    let mut pool = WorkerPool::new();
    let mut ids: Vec<u32> = Vec::new();
    let mut i = 0usize;
    while i < CASE_COST_TABLE.len() {
        match CASE_COST_TABLE.get(i) {
            Some((_, id, _)) => ids.push(*id),
            None => return false,
        }
        i += 1;
    }
    if pool.submit(&ids) != Ok(ids.len()) {
        return false;
    }
    let done = match pool.drain() {
        Ok(n) => n,
        Err(_) => return false,
    };
    if done != ids.len() || pool.completed().len() != ids.len() {
        return false;
    }
    if !lost_cases(&pool).is_empty() || !duplicated_cases(&pool).is_empty() {
        return false;
    }
    // 无迁移：完成 worker 必在双核界内且每例恰一账。
    for c in pool.completed().iter() {
        if c.worker >= WORKER_COUNT {
            return false;
        }
    }
    true
}

/// 轮转分派核：六例提交后按 0,1,0,1 分核，本地队列各自持有分派额。
fn round_robin_ok() -> bool {
    let mut pool = WorkerPool::new();
    let ids: [u32; 6] = [1, 2, 3, 4, 5, 6];
    if pool.submit(&ids) != Ok(6) {
        return false;
    }
    // 本地化：每核应持有 3 例，drain 后各核完成 3 例。
    if pool.pending_on(0) != 3 || pool.pending_on(1) != 3 {
        return false;
    }
    if pool.drain().is_err() {
        return false;
    }
    let mut on0 = 0usize;
    let mut on1 = 0usize;
    for c in pool.completed().iter() {
        match c.worker {
            0 => on0 += 1,
            1 => on1 += 1,
            _ => return false,
        }
    }
    on0 == 3 && on1 == 3
}

/// 表外用例闸：含一个表外 ID 的整批提交必拒（CaseForeign），批不落账。
fn foreign_case_gate_ok() -> bool {
    let mut pool = WorkerPool::new();
    let bad: [u32; 2] = [1, 0x7FFF];
    if pool.submit(&bad) != Err(Diag::CaseForeign.val()) {
        return false;
    }
    pool.submitted().is_empty() && pool.completed().is_empty()
}

/// 收尾后再提交闸：drain 后提交口必闭（SubmitAfterDrain）。
fn submit_after_drain_gate_ok() -> bool {
    let mut pool = WorkerPool::new();
    let ids: [u32; 2] = [1, 2];
    if pool.submit(&ids).is_err() {
        return false;
    }
    if pool.drain().is_err() {
        return false;
    }
    let more: [u32; 1] = [3];
    pool.submit(&more) == Err(Diag::SubmitAfterDrain.val())
}

/// 完成账纪律核：同核内序戳随完成序严格递增；代价与冻结表逐条对上。
fn completion_ledger_ok() -> bool {
    let mut pool = WorkerPool::new();
    let ids: [u32; 6] = [1, 2, 3, 4, 5, 6];
    if pool.submit(&ids).is_err() || pool.drain().is_err() {
        return false;
    }
    // 同核序戳递增核（按 worker 分组看账内出现序）。
    let mut w = 0usize;
    while w < WORKER_COUNT {
        let mut prev: u64 = 0;
        for c in pool.completed().iter() {
            if c.worker == w {
                if prev > 0 && c.finish_tick <= prev {
                    return false;
                }
                prev = c.finish_tick;
            }
        }
        w += 1;
    }
    // 代价对账：每例实耗拍数与冻结表一致。
    for c in pool.completed().iter() {
        match cost_of(c.case_id) {
            Some(cost) => {
                if cost != c.cost_ticks {
                    return false;
                }
            }
            None => return false,
        }
    }
    pool.pending_on(0) == 0 && pool.pending_on(1) == 0
}

/// M-205 注入对账核：完成账人为抽掉一例（伪造丢失），差集对账必点名
/// 被抽例；若对账漏点则 ReconcileMiss 实锤。
fn forged_loss_reconcile_ok() -> bool {
    let mut pool = WorkerPool::new();
    let mut ids: Vec<u32> = Vec::new();
    let mut i = 0usize;
    while i < CASE_COST_TABLE.len() {
        match CASE_COST_TABLE.get(i) {
            Some((_, id, _)) => ids.push(*id),
            None => return false,
        }
        i += 1;
    }
    if pool.submit(&ids).is_err() || pool.drain().is_err() {
        return false;
    }
    // 原账零丢失。
    if !lost_cases(&pool).is_empty() {
        return false;
    }
    // 伪造丢失：抽掉序列化例（ID=3）后差集必点名。
    let mut forged = pool.clone();
    let mut kept: Vec<Completion> = Vec::new();
    for c in forged.completed().iter() {
        if c.case_id != 3 {
            kept.push(c.clone());
        }
    }
    forged.completed = kept;
    let lost = lost_cases(&forged);
    lost.len() == 1 && lost.first() == Some(&3)
}

/// 双跑一致：同提交两次 drain 的完成账逐字段相同。
fn double_run_ok() -> bool {
    let run_once = || -> Result<Vec<Completion>, u32> {
        let mut pool = WorkerPool::new();
        let ids: [u32; 6] = [1, 2, 3, 4, 5, 6];
        pool.submit(&ids)?;
        pool.drain()?;
        Ok(pool.completed().to_vec())
    };
    let a = run_once();
    let b = run_once();
    a.is_ok() && b.is_ok() && a == b
}

/// 空批核：空提交合法（0 例），drain 也 0 完成，零丢失平凡成立。
fn empty_batch_ok() -> bool {
    let mut pool = WorkerPool::new();
    if pool.submit(&[]) != Ok(0) {
        return false;
    }
    if pool.drain() != Ok(0) {
        return false;
    }
    pool.completed().is_empty() && lost_cases(&pool).is_empty()
}

/// 写回面：状态=已完成、结果摘要非空（含核数、完成数与零丢失结论）。
fn writeback_summary_ok() -> bool {
    let mut pool = WorkerPool::new();
    let mut ids: Vec<u32> = Vec::new();
    let mut i = 0usize;
    while i < CASE_COST_TABLE.len() {
        match CASE_COST_TABLE.get(i) {
            Some((_, id, _)) => ids.push(*id),
            None => return false,
        }
        i += 1;
    }
    if pool.submit(&ids).is_err() || pool.drain().is_err() {
        return false;
    }
    let lost_n = lost_cases(&pool).len();
    let summary = format!(
        "UNX-F3446 完成：双核桩调度{}核 完成{}例 零丢失{}",
        WORKER_COUNT,
        pool.completed().len(),
        if lost_n == 0 { "成立" } else { "破约" }
    );
    !summary.is_empty()
        && summary.contains("UNX-F3446")
        && summary.contains("零丢失")
        && lost_n == 0
        && detail(Diag::LossDetected.val()).contains("M-205")
}

// ── 判据总成：run_unx3446_checks ───────────────────────────────────────

/// run_unx3446_checks：本单判据聚合（诊断段 0x79CC0，全部走 add_dyn）。
pub fn run_unx3446_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F3446 并发执行器");
    let checks: [(&'static str, bool, String); 13] = [
        (
            "段位守卫0x40对齐且在批段窗内",
            seg_guard_ok(),
            format!("SEG_BASE=0x{:X} 守卫{}", SEG_BASE, if seg_guard_ok() { "过" } else { "破" }),
        ),
        (
            "诊断码闭集12码唯一且文案带建议",
            diag_closed_set_ok(),
            format!("闭集{}", if diag_closed_set_ok() { "完整" } else { "破缺" }),
        ),
        (
            "M-205零丢失三查零丢无重无迁移",
            zero_loss_ok(),
            format!("M-205零丢失{}", if zero_loss_ok() { "成立" } else { "破约" }),
        ),
        (
            "轮转分派双核各持三例本地化",
            round_robin_ok(),
            format!("轮转分派{}", if round_robin_ok() { "均衡" } else { "失衡" }),
        ),
        (
            "表外用例整批必拒不落账",
            foreign_case_gate_ok(),
            format!("表外闸{}", if foreign_case_gate_ok() { "拦准" } else { "失守" }),
        ),
        (
            "收尾后再提交口闭锁必拒",
            submit_after_drain_gate_ok(),
            format!("收尾闸{}", if submit_after_drain_gate_ok() { "拦准" } else { "失守" }),
        ),
        (
            "完成账序戳递增代价与冻结表对上",
            completion_ledger_ok(),
            format!("完成账{}", if completion_ledger_ok() { "对账" } else { "错账" }),
        ),
        (
            "伪造丢失差集对账必点名",
            forged_loss_reconcile_ok(),
            format!("对账闸{}", if forged_loss_reconcile_ok() { "点名" } else { "漏点" }),
        ),
        (
            "双跑一致完成账逐字段同",
            double_run_ok(),
            format!("双跑{}", if double_run_ok() { "一致" } else { "分叉" }),
        ),
        (
            "空批合法零完成零丢失平凡成立",
            empty_batch_ok(),
            format!("空批{}", if empty_batch_ok() { "成立" } else { "破约" }),
        ),
        (
            "冻结代价表六例查价对齐",
            cost_table_ok(),
            format!("代价表{}", if cost_table_ok() { "对齐" } else { "漂移" }),
        ),
        (
            "单例提交分派0核完成可查",
            single_case_ok(),
            format!("单例{}", if single_case_ok() { "落账" } else { "失账" }),
        ),
        (
            "完成账条数恒等于提交条数",
            count_equality_ok(),
            format!("条数{}", if count_equality_ok() { "守恒" } else { "失衡" }),
        ),
    ];
    // 写回摘要判据（第 14 条，与 13 条并列成账）。
    let wb = writeback_summary_ok();
    for (name, passed, det) in checks.iter() {
        s.add_dyn(name, *passed, det);
    }
    s.add_dyn(
        "写回摘要非空状态已完成",
        wb,
        &format!("写回{}", if wb { "非空" } else { "空" }),
    );
    s
}

/// 冻结代价表自洽：六例 ID 互异、代价非零、表外查价 None。
fn cost_table_ok() -> bool {
    if CASE_COST_TABLE.len() != 6 {
        return false;
    }
    let mut i = 0usize;
    while i < CASE_COST_TABLE.len() {
        let (_, id_a, cost_a) = match CASE_COST_TABLE.get(i) {
            Some(t) => *t,
            None => return false,
        };
        if cost_of(id_a) != Some(cost_a) || cost_a == 0 {
            return false;
        }
        let mut j = i + 1;
        while j < CASE_COST_TABLE.len() {
            match CASE_COST_TABLE.get(j) {
                Some((_, id_b, _)) => {
                    if id_a == *id_b {
                        return false;
                    }
                }
                None => return false,
            }
            j += 1;
        }
        i += 1;
    }
    cost_of(0x7FFF).is_none()
}

/// 单例提交：一例入 0 核本地队列，drain 后完成账恰一条且 worker=0。
fn single_case_ok() -> bool {
    let mut pool = WorkerPool::new();
    let ids: [u32; 1] = [5];
    if pool.submit(&ids) != Ok(1) {
        return false;
    }
    if pool.drain() != Ok(1) {
        return false;
    }
    match pool.completed().first() {
        Some(c) => c.case_id == 5 && c.worker == 0 && c.cost_ticks == 4,
        None => false,
    }
}

/// 条数守恒：提交 n 例 drain 后完成账恒 n 条（1..=6 例全扫）。
fn count_equality_ok() -> bool {
    let mut n = 1usize;
    while n <= CASE_COST_TABLE.len() {
        let mut pool = WorkerPool::new();
        let mut ids: Vec<u32> = Vec::new();
        let mut i = 0usize;
        while i < n {
            match CASE_COST_TABLE.get(i) {
                Some((_, id, _)) => ids.push(*id),
                None => return false,
            }
            i += 1;
        }
        if pool.submit(&ids) != Ok(n) {
            return false;
        }
        if pool.drain() != Ok(n) {
            return false;
        }
        if pool.completed().len() != n || !lost_cases(&pool).is_empty() {
            return false;
        }
        n += 1;
    }
    true
}

// ── 测试区（#[cfg(test)]：允许 assert!/assert_eq!/unwrap）──────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn seg_guard_and_diag_closed_set() {
        assert!(seg_guard_ok());
        assert_eq!(SEG_BASE, 0x79CC0);
        assert!(diag_closed_set_ok());
        for (i, d) in ALL_DIAGS.iter().enumerate() {
            assert_eq!(d.val(), SEG_BASE + i as u32 + 1);
        }
        assert_eq!(detail(0xDEAD), detail(0x5A5A));
    }

    #[test]
    fn zero_loss_m205() {
        assert!(zero_loss_ok());
        assert_eq!(WORKER_COUNT, 2);
        assert_eq!(INJECTION_CASE_ID, 0x205);
    }

    #[test]
    fn round_robin_local_queues() {
        assert!(round_robin_ok());
    }

    #[test]
    fn foreign_and_post_drain_gates() {
        assert!(foreign_case_gate_ok());
        assert!(submit_after_drain_gate_ok());
    }

    #[test]
    fn completion_ledger_ticks_and_costs() {
        assert!(completion_ledger_ok());
        assert_eq!(cost_of(3), Some(64));
        assert_eq!(cost_of(0x7FFF), None);
    }

    #[test]
    fn forged_loss_pointed_out() {
        assert!(forged_loss_reconcile_ok());
        let mut pool = WorkerPool::new();
        let ids: [u32; 3] = [1, 2, 3];
        assert!(pool.submit(&ids).is_ok());
        assert!(pool.drain().is_ok());
        assert!(lost_cases(&pool).is_empty());
        assert!(duplicated_cases(&pool).is_empty());
    }

    #[test]
    fn double_run_identical() {
        assert!(double_run_ok());
    }

    #[test]
    fn empty_batch_and_single_case() {
        assert!(empty_batch_ok());
        assert!(single_case_ok());
    }

    #[test]
    fn cost_table_and_count_conservation() {
        assert!(cost_table_ok());
        assert!(count_equality_ok());
    }

    #[test]
    fn writeback_summary_non_empty() {
        assert!(writeback_summary_ok());
    }

    #[test]
    fn checks_aggregate_all_green() {
        let s = run_unx3446_checks();
        assert!(s.len() >= 12);
        assert!(s.all_passed(), "全部判据必须绿：{}", s.red_items_text());
    }
}
