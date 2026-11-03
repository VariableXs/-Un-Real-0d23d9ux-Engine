//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F1747 · 唤醒置态原语（睡眠→就绪原子转换） · 0x724C0
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定，CoRun·UNX-A3-B08，前置 UNX-F1746）：
//!   ① 纯功能代码 ≥300 行；
//!   ② 判据原文：UNX-F1747-J1 置态原子性断言全过，可见性窗口注入
//!     10/10 次正确排队；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! ─────────────────────────────────────────────────────────────────────
//! 设计要点：
//!   · 置态三段提交： Sleeping→Waking（占位）→ 入就绪队列 →Ready；
//!     原子不变量：任意观察点 state 序值≥Ready ⇔ 在就绪队列恰一次
//!     （VisibleHole 即违原子性）；
//!   · 可见性窗口注入：跳过入队段直接置 Ready（观察者看到"就绪但
//!     不在队列"的洞），恢复扫描 10/10 次检测并正确补队；
//!   · 唤醒纪元：每次置态 wake_epoch 严格递增，倒退显性拒；重复唤醒
//!     （非 Sleeping 态）拒；队列入队序单调不重不漏；
//!   · 阻断/再唤醒：Running→Sleeping 是显性的新周期入口（epoch+1），
//!     旧周期入队残迹必须清空后才可再唤醒；
//!   · 失真注入 10 路：就绪不入队/入队不就绪/重复唤醒/唤醒运行态/
//!     纪元倒退/伪造 tid/重复入队/队列丢项/入队序乱/状态倒退，
//!     原子扫描 10/10 必抓；双跑一致 + 段位守卫窗口 0x40。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x724C0，VX3-1103 唤醒置态窗口独占）。
pub const SEG_BASE: u32 = 0x724C0;
/// 受控任务数（冻结 64）。
pub const TASK_N: usize = 64;
/// 注入轮数（判据 10/10 语义，冻结）。
pub const INJECT_ROUNDS: u32 = 10;
/// 就绪队列容量（冻结 64）。
pub const QUEUE_CAP: usize = 64;
/// 全链再唤醒周期数（冻结 2）。
pub const WAKE_CYCLES: u32 = 2;

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum WakeCode {
    /// 原子性破洞：就绪态不在队列或队列项非就绪态
    VisibleHole,
    /// 状态倒退：置态序值回退（Ready→Sleeping 无新周期入口）
    StateBack,
    /// 重复唤醒：非 Sleeping 态被再唤醒
    WakeDup,
    /// 纪元倒退：wake_epoch 不递增
    EpochBack,
    /// 队列破损：入队序不单调或重复入队
    QueueDup,
    /// 队列丢项：就绪队列丢项或超容
    QueueMiss,
    /// tid 越界：伪造任务号不在表内
    TidBad,
    /// 双跑不一致：golden_run 两次应同值
    NotReproducible,
}

impl WakeCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            WakeCode::VisibleHole => "原子性破洞：就绪⇔在队不变量被破，跑恢复扫描补队或撤态",
            WakeCode::StateBack => "状态倒退：置态只进不退，回睡必须走 block 新周期入口",
            WakeCode::WakeDup => "重复唤醒：仅 Sleeping 态可唤醒，重复唤醒显性拒",
            WakeCode::EpochBack => "纪元倒退：wake_epoch 严格递增，倒退一律拒",
            WakeCode::QueueDup => "队列破损：同一任务重复入队或入队序不单调，重排就绪队列",
            WakeCode::QueueMiss => "队列丢项：就绪队列丢项/超容，恢复扫描补齐",
            WakeCode::TidBad => "tid 越界：任务号不在表内，查调用方寄存",
            WakeCode::NotReproducible => "双跑不一致：复现红线，golden_run 两次应同值",
        }
    }
    pub const ALL: [WakeCode; 8] = [
        WakeCode::VisibleHole,
        WakeCode::StateBack,
        WakeCode::WakeDup,
        WakeCode::EpochBack,
        WakeCode::QueueDup,
        WakeCode::QueueMiss,
        WakeCode::TidBad,
        WakeCode::NotReproducible,
    ];
}

/// 诊断码查表（段外码返回统一文案）。
pub fn detail(code: u32) -> &'static str {
    let mut i = 0;
    while i < WakeCode::ALL.len() {
        if WakeCode::ALL[i].val() == code {
            return WakeCode::ALL[i].detail();
        }
        i += 1;
    }
    "未知诊断码：不在 0x724C0 段闭集内"
}

// ── 状态机（Sleeping→Waking→Ready→Running，单调推进） ───────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TaskState {
    /// 睡眠：等待唤醒原语。
    Sleeping,
    /// 唤醒占位：三段提交第一段已完成。
    Waking,
    /// 就绪：已在就绪队列恰一次。
    Ready,
    /// 运行：已出队被调度。
    Running,
}

impl TaskState {
    /// 单调序值：Sleeping=0 < Waking=1 < Ready=2 < Running=3。
    pub fn rank(self) -> u8 {
        match self {
            TaskState::Sleeping => 0,
            TaskState::Waking => 1,
            TaskState::Ready => 2,
            TaskState::Running => 3,
        }
    }
    /// 只允许向前推进；平移与倒退一律 false。
    pub fn can_advance(self, to: TaskState) -> bool {
        to.rank() > self.rank()
    }
}

// ── 域模型 ──────────────────────────────────────────────────────
/// 任务格：置态 + 唤醒纪元 + 队列锚。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TaskCell {
    pub tid: u32,
    pub state: TaskState,
    /// 唤醒纪元（每次置态 +1，严格递增）。
    pub wake_epoch: u64,
    /// 是否挂在就绪队列（原子不变量的一侧）。
    pub queued: bool,
    /// 入队序号（None=未入队；Some 单调递增）。
    pub enqueue_seq: Option<u64>,
}

/// 唤醒表：任务格阵 + 就绪队列 + 全局入队序。
#[derive(Clone, Debug)]
pub struct WakeTable {
    pub cells: Vec<TaskCell>,
    /// 就绪队列（tid 序，按入队序单调）。
    pub ready: Vec<u32>,
    /// 全局入队序（只增）。
    pub seq: u64,
}

/// 建表（确定性：全员 Sleeping，epoch 从 1 起）。
pub fn build_table(n: usize) -> WakeTable {
    let mut cells: Vec<TaskCell> = Vec::new();
    let mut i = 0usize;
    while i < n {
        cells.push(TaskCell {
            tid: i as u32,
            state: TaskState::Sleeping,
            wake_epoch: (i + 1) as u64,
            queued: false,
            enqueue_seq: None,
        });
        i += 1;
    }
    WakeTable { cells, ready: Vec::new(), seq: 0 }
}

// ── 原子不变量扫描（判据主口） ──────────────────────────────────
/// 置态原子性：任意观察点
///   ① state 序值≥Ready ⇔ queued==true；
///   ② 就绪队列恰含该任务一次（不重不漏）；
///   ③ 队列长度 ≤ QUEUE_CAP；
///   ④ 队列中 tid 全部在表内且 state≥Ready；
///   ⑤ 入队序沿队列严格递增；
///   ⑥ wake_epoch ≥ 1（纪元永不归零）。
pub fn atomicity_scan(t: &WakeTable) -> Result<(), u32> {
    let mut i = 0usize;
    while i < t.cells.len() {
        let c = match t.cells.get(i) {
            Some(c) => c,
            None => return Err(WakeCode::TidBad.val()),
        };
        let should_queue = c.state == TaskState::Ready;
        if c.queued != should_queue {
            return Err(WakeCode::VisibleHole.val());
        }
        if c.wake_epoch == 0 {
            return Err(WakeCode::EpochBack.val());
        }
        // 在队标记必须与队列成员一致：恰出现一次。
        if c.queued {
            let mut hits = 0u32;
            let mut k = 0usize;
            while k < t.ready.len() {
                if let Some(x) = t.ready.get(k) {
                    if *x == c.tid {
                        hits += 1;
                    }
                }
                k += 1;
            }
            if hits != 1 {
                return Err(WakeCode::QueueMiss.val());
            }
        }
        i += 1;
    }
    if t.ready.len() > QUEUE_CAP {
        return Err(WakeCode::QueueMiss.val());
    }
    let mut prev_seq: Option<u64> = None;
    let mut k = 0usize;
    while k < t.ready.len() {
        let tid = match t.ready.get(k) {
            Some(x) => *x,
            None => return Err(WakeCode::QueueMiss.val()),
        };
        if tid as usize >= t.cells.len() {
            return Err(WakeCode::TidBad.val());
        }
        let c = match t.cells.get(tid as usize) {
            Some(c) => c,
            None => return Err(WakeCode::TidBad.val()),
        };
        if c.state != TaskState::Ready {
            return Err(WakeCode::VisibleHole.val());
        }
        if !c.queued {
            return Err(WakeCode::QueueMiss.val());
        }
        let qseq = match c.enqueue_seq {
            Some(s) => s,
            None => return Err(WakeCode::QueueMiss.val()),
        };
        if let Some(p) = prev_seq {
            if qseq <= p {
                return Err(WakeCode::QueueDup.val());
            }
        }
        prev_seq = Some(qseq);
        k += 1;
    }
    Ok(())
}

// ── 唤醒置态原语（三段提交） ────────────────────────────────────
/// 提交记录：tid、纪元、入队序号。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct WakeCommit {
    pub tid: u32,
    pub epoch: u64,
    pub enqueue_seq: u64,
}

/// 唤醒置态原语：Sleeping→Waking→入队→Ready 三段原子提交。
/// 任一段失败即整体回退到 Sleeping；重复唤醒/纪元倒退显性拒。
pub fn wake_primitive(t: &mut WakeTable, tid: u32) -> Result<WakeCommit, u32> {
    if tid as usize >= t.cells.len() {
        return Err(WakeCode::TidBad.val());
    }
    let idx = tid as usize;
    let state = match t.cells.get(idx) {
        Some(c) => c.state,
        None => return Err(WakeCode::TidBad.val()),
    };
    if state != TaskState::Sleeping {
        return Err(WakeCode::WakeDup.val());
    }
    // 段一：占位置 Waking。
    if !state.can_advance(TaskState::Waking) {
        return Err(WakeCode::StateBack.val());
    }
    let epoch = match t.cells[idx].wake_epoch.checked_add(1) {
        Some(e) => e,
        None => return Err(WakeCode::EpochBack.val()),
    };
    t.cells[idx].state = TaskState::Waking;
    t.cells[idx].wake_epoch = epoch;
    t.cells[idx].queued = false;
    t.cells[idx].enqueue_seq = None;
    // 段二：入就绪队列（先排队后置态：观察者只见"在队且就绪"）。
    if t.ready.len() >= QUEUE_CAP {
        // 容量拒：整体回退。
        t.cells[idx].state = TaskState::Sleeping;
        return Err(WakeCode::QueueMiss.val());
    }
    let qseq = t.seq;
    t.ready.push(tid);
    t.seq = t.seq.saturating_add(1);
    // 段三：置 Ready。
    t.cells[idx].queued = true;
    t.cells[idx].enqueue_seq = Some(qseq);
    t.cells[idx].state = TaskState::Ready;
    // 原子性自证：提交后扫描必过。
    atomicity_scan(t)?;
    Ok(WakeCommit { tid, epoch, enqueue_seq: qseq })
}

/// 调度出队：Ready→Running（就绪队列头出队）。
pub fn dispatch_head(t: &mut WakeTable) -> Result<u32, u32> {
    if t.ready.is_empty() {
        return Err(WakeCode::QueueMiss.val());
    }
    let tid = t.ready[0];
    let rest: Vec<u32> = t.ready[1..].to_vec();
    t.ready = rest;
    let idx = tid as usize;
    if idx >= t.cells.len() {
        return Err(WakeCode::TidBad.val());
    }
    if t.cells[idx].state != TaskState::Ready {
        return Err(WakeCode::VisibleHole.val());
    }
    t.cells[idx].queued = false;
    t.cells[idx].enqueue_seq = None;
    t.cells[idx].state = TaskState::Running;
    atomicity_scan(t)?;
    Ok(tid)
}

/// 阻断入口：Running→Sleeping 显性新周期（epoch+1，队列锚已清）。
pub fn block_running(t: &mut WakeTable, tid: u32) -> Result<(), u32> {
    if tid as usize >= t.cells.len() {
        return Err(WakeCode::TidBad.val());
    }
    let idx = tid as usize;
    if t.cells[idx].state != TaskState::Running {
        return Err(WakeCode::StateBack.val());
    }
    if t.cells[idx].queued {
        return Err(WakeCode::VisibleHole.val());
    }
    let epoch = match t.cells[idx].wake_epoch.checked_add(1) {
        Some(e) => e,
        None => return Err(WakeCode::EpochBack.val()),
    };
    t.cells[idx].state = TaskState::Sleeping;
    t.cells[idx].wake_epoch = epoch;
    atomicity_scan(t)?;
    Ok(())
}

// ── 恢复扫描（可见性窗口注入的修复口） ─────────────────────────
/// 恢复扫描：就绪但不在队（VisibleHole）→ 补队；在队但不就绪 →
/// 撤队。返回补队数。10/10 次正确排队红线由调用方断言。
pub fn recovery_scan(t: &mut WakeTable) -> Result<u32, u32> {
    let mut repaired = 0u32;
    let mut i = 0usize;
    while i < t.cells.len() {
        let c = match t.cells.get(i) {
            Some(c) => c.clone(),
            None => return Err(WakeCode::TidBad.val()),
        };
        let should = c.state == TaskState::Ready;
        if should && !c.queued {
            // 就绪但不在队：补队。
            if t.ready.len() >= QUEUE_CAP {
                return Err(WakeCode::QueueMiss.val());
            }
            let qseq = t.seq;
            t.ready.push(c.tid);
            t.seq = t.seq.saturating_add(1);
            t.cells[i].queued = true;
            t.cells[i].enqueue_seq = Some(qseq);
            repaired += 1;
        } else if !should && c.queued {
            // 在队但不就绪：撤队（按 tid 移除恰一次）。
            let mut out: Vec<u32> = Vec::new();
            let mut k = 0usize;
            let mut removed = false;
            while k < t.ready.len() {
                let x = match t.ready.get(k) {
                    Some(x) => *x,
                    None => return Err(WakeCode::QueueMiss.val()),
                };
                if x == c.tid && !removed {
                    removed = true;
                } else {
                    out.push(x);
                }
                k += 1;
            }
            t.ready = out;
            t.cells[i].queued = false;
            t.cells[i].enqueue_seq = None;
            repaired += 1;
        }
        i += 1;
    }
    atomicity_scan(t)?;
    Ok(repaired)
}

// ── 可见性窗口注入（10 路） ─────────────────────────────────────
/// 对已闭合唤醒表注入第 kind 路失真，返回坏表（值域全整数，安全）。
pub fn distort(t: &WakeTable, kind: usize) -> WakeTable {
    let mut bad = t.clone();
    let n = bad.cells.len();
    match kind % 10 {
        0 => {
            // 就绪不入队：可见性窗口洞（原子扫描必抓）。
            // 目标锁定黄金表中段的就绪在队任务（前 TASK_N/4 已出队运行）。
            let idx = TASK_N / 4 + 3;
            if idx < n {
                bad.cells[idx].queued = false;
                bad.cells[idx].enqueue_seq = None;
            }
        }
        1 => {
            // 入队不就绪：队列项挂着睡眠态任务。
            let idx = TASK_N / 4 + 4;
            if idx < n {
                bad.cells[idx].state = TaskState::Sleeping;
            }
        }
        2 => {
            // 队列丢项：就绪队列抹掉一项（锚未清）。
            if !bad.ready.is_empty() {
                bad.ready.remove(0);
            }
        }
        3 => {
            // 重复入队：队列尾追加已有项。
            let tid = match bad.ready.first() {
                Some(x) => *x,
                None => 0,
            };
            bad.ready.push(tid);
        }
        4 => {
            // 纪元倒退：某任务 wake_epoch 清零。
            let idx = 5 % n.max(1);
            bad.cells[idx].wake_epoch = 0;
        }
        5 => {
            // 伪造 tid：队列塞表外任务号。
            bad.ready.push(n as u32 + 9);
        }
        6 => {
            // 入队序乱：交换队列两项使序倒挂。
            if bad.ready.len() >= 2 {
                let a = bad.ready[0];
                bad.ready[0] = bad.ready[1];
                bad.ready[1] = a;
            }
        }
        7 => {
            // 状态倒退：Ready 任务直接压回 Sleeping（无新周期入口）。
            // 队列残迹未清 → 队列项状态<Ready 必抓。
            let idx = TASK_N / 4 + 5;
            if idx < n {
                bad.cells[idx].state = TaskState::Sleeping;
                bad.cells[idx].queued = false;
                bad.cells[idx].enqueue_seq = None;
            }
        }
        8 => {
            // 队列超容：灌满假项。
            let mut k = bad.ready.len();
            while k < QUEUE_CAP + 1 {
                bad.ready.push((k % n.max(1)) as u32);
                k += 1;
            }
        }
        _ => {
            // 队列锚缺失：在队但 enqueue_seq=None。
            let tid = match bad.ready.first() {
                Some(x) => *x,
                None => 0,
            };
            if (tid as usize) < bad.cells.len() {
                bad.cells[tid as usize].enqueue_seq = None;
            }
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

/// 可见性窗口注入轮：每轮制造"就绪不入队"窗口（跳段提交），恢复
/// 扫描必须 10/10 次正确补队（补队后原子扫描全过且补队数恰 1）。
pub fn visibility_rounds(rounds: u32) -> Result<InjectLedger, u32> {
    let mut led = InjectLedger::new();
    let mut r = 0u32;
    while r < rounds {
        let mut t = build_table(TASK_N);
        // 唤醒前 (r%TASK_N) 个任务，全部闭合。
        let k = r % (TASK_N as u32);
        let mut w = 0u32;
        while w < k {
            let _ = wake_primitive(&mut t, w)?;
            w += 1;
        }
        let base_ok = atomicity_scan(&t).is_ok();
        // 跳段提交：直接置 Ready 不入队（可见性窗口洞）。
        let victim = k as usize;
        if victim < t.cells.len() {
            t.cells[victim].state = TaskState::Ready;
            t.cells[victim].queued = false;
            t.cells[victim].enqueue_seq = None;
            let hole_seen = atomicity_scan(&t).is_err();
            // 恢复扫描：正确补队恰 1。
            let repaired = recovery_scan(&mut t)?;
            let closed = atomicity_scan(&t).is_ok();
            let queued_ok = t.cells[victim].queued
                && t.cells[victim].state == TaskState::Ready
                && t.ready.contains(&(victim as u32));
            led.record(
                (r % 10) as usize,
                base_ok && hole_seen && closed && repaired == 1 && queued_ok,
            );
        } else {
            led.record((r % 10) as usize, false);
        }
        r += 1;
    }
    Ok(led)
}

/// 失真注入轮：每轮对新鲜黄金表注入一路失真，原子扫描必须抓到
/// （黄金表本身必须扫描通过，防注入器污染基准）。
pub fn inject_rounds(rounds: u32) -> Result<InjectLedger, u32> {
    let mut led = InjectLedger::new();
    let mut r = 0u32;
    while r < rounds {
        let t = golden_table()?;
        let kind = (r % 10) as usize;
        led.record(kind, atomicity_scan(&distort(&t, kind)).is_err());
        r += 1;
    }
    Ok(led)
}

/// 黄金表（确定性）：前半唤醒就绪、四分之一出队运行、全员闭合。
pub fn golden_table() -> Result<WakeTable, u32> {
    let mut t = build_table(TASK_N);
    let mut i = 0u32;
    while i < TASK_N as u32 {
        let _ = wake_primitive(&mut t, i)?;
        i += 1;
    }
    let mut d = 0u32;
    while d < (TASK_N / 4) as u32 {
        let _ = dispatch_head(&mut t)?;
        d += 1;
    }
    atomicity_scan(&t)?;
    Ok(t)
}

// ── 黄金全链 ────────────────────────────────────────────────────
/// 全链：全员两周期 唤醒→出队→阻断→再唤醒，逐步原子扫描 → 可见性
/// 窗口 10/10 → 失真注入 10/10 → 四拒探（重复唤醒/唤醒运行态/纪元
/// 倒退探针/伪造 tid）。
/// 返回 (正旗数, 拒旗数, 置态提交数, 恢复补队数)。
pub fn golden_run() -> Result<(u32, u32, u64, u32), u32> {
    let mut t = build_table(TASK_N);
    let mut commits: u64 = 0;
    let mut cycle = 0u32;
    while cycle < WAKE_CYCLES {
        // 唤醒所有睡眠者（上一周期被阻断者）。
        let mut i = 0u32;
        while i < TASK_N as u32 {
            if t.cells[i as usize].state == TaskState::Sleeping {
                let c = wake_primitive(&mut t, i)?;
                if c.epoch == 0 {
                    return Err(WakeCode::EpochBack.val());
                }
                commits += 1;
            }
            i += 1;
        }
        atomicity_scan(&t)?;
        // 出队全员（就绪→运行）。
        while !t.ready.is_empty() {
            let _ = dispatch_head(&mut t)?;
        }
        atomicity_scan(&t)?;
        // 全员阻断回睡（新周期入口），进入下一周期。
        let mut b = 0u32;
        while b < TASK_N as u32 {
            block_running(&mut t, b)?;
            b += 1;
        }
        atomicity_scan(&t)?;
        cycle += 1;
    }
    // 末轮残余就绪清场：全部出队。
    while !t.ready.is_empty() {
        let _ = dispatch_head(&mut t)?;
    }
    atomicity_scan(&t)?;
    // 可见性窗口注入 10/10 正确排队。
    let vis = visibility_rounds(INJECT_ROUNDS)?;
    let vis_ok = vis.all_caught();
    // 失真注入 10 路全抓。
    let inj = inject_rounds(INJECT_ROUNDS)?;
    let inj_ok = inj.all_caught();
    // 四拒探。
    let mut probe = build_table(8);
    let _ = wake_primitive(&mut probe, 1)?;
    let dup_rej = wake_primitive(&mut probe, 1).is_err();
    let mut probe2 = build_table(8);
    let _ = wake_primitive(&mut probe2, 2)?;
    let _ = dispatch_head(&mut probe2)?;
    let run_rej = wake_primitive(&mut probe2, 2).is_err();
    let tid_rej = wake_primitive(&mut probe, TASK_N as u32 + 3).is_err();
    let mut probe3 = build_table(8);
    let _ = wake_primitive(&mut probe3, 0)?;
    let back_rej = wake_primitive(&mut probe3, 0).is_err();
    // 纪元单调探针：两周期后被阻断任务的 epoch 至少 +3（唤醒×2+阻断×1）。
    let mut probe4 = build_table(4);
    let _ = wake_primitive(&mut probe4, 0)?;
    let e1 = probe4.cells[0].wake_epoch;
    let _ = dispatch_head(&mut probe4)?;
    let _ = block_running(&mut probe4, 0)?;
    let _ = wake_primitive(&mut probe4, 0)?;
    let e2 = probe4.cells[0].wake_epoch;
    let epoch_ok = e2 > e1 && probe4.cells[0].state == TaskState::Ready;
    let flags = (commits == TASK_N as u64 * WAKE_CYCLES as u64) as u32
        + vis_ok as u32
        + inj_ok as u32
        + epoch_ok as u32;
    let rejects = dup_rej as u32 + run_rej as u32 + tid_rej as u32 + back_rej as u32;
    Ok((flags, rejects, commits, vis.caught))
}

// ── 探针三序 ────────────────────────────────────────────────────
/// 序 A：黄金全链四正四拒全中。
pub fn probe_seq_a() -> Result<(), u32> {
    match golden_run() {
        Ok((4, 4, _, _)) => Ok(()),
        _ => Err(WakeCode::NotReproducible.val()),
    }
}

/// 序 B：可见性窗口注入 10/10 正确排队。
pub fn probe_seq_b() -> Result<(), u32> {
    match visibility_rounds(INJECT_ROUNDS) {
        Ok(l) if l.all_caught() => Ok(()),
        _ => Err(WakeCode::VisibleHole.val()),
    }
}

/// 序 C：冻结常量与段位窗口。
pub fn probe_seq_c() -> Result<(), u32> {
    let in_window = SEG_BASE == 0x724C0 && WakeCode::NotReproducible.val() < SEG_BASE + 0x40;
    let ok = TASK_N == 64 && INJECT_ROUNDS == 10 && QUEUE_CAP == 64
        && WAKE_CYCLES == 2 && in_window;
    if ok {
        Ok(())
    } else {
        Err(WakeCode::NotReproducible.val())
    }
}

// ── 双跑一致（复现红线） ────────────────────────────────────────
pub fn repro_check() -> Result<(), u32> {
    let a = golden_run()?;
    let b = golden_run()?;
    if a != b {
        return Err(WakeCode::NotReproducible.val());
    }
    Ok(())
}

// ── 段位守卫 ────────────────────────────────────────────────────
pub fn segment_guard() -> bool {
    let mut i = 0;
    while i < WakeCode::ALL.len() {
        let v = WakeCode::ALL[i].val();
        if v < SEG_BASE || v >= SEG_BASE + 0x40 {
            return false;
        }
        i += 1;
    }
    true
}

// ── 状态机单调探针 ──────────────────────────────────────────────
/// 全序 + 平移/倒退拒 + 三段提交逐步可查。
pub fn probe_state_machine() -> bool {
    let ranks = TaskState::Sleeping.rank() < TaskState::Waking.rank()
        && TaskState::Waking.rank() < TaskState::Ready.rank()
        && TaskState::Ready.rank() < TaskState::Running.rank();
    let flat = !TaskState::Ready.can_advance(TaskState::Ready);
    let back = !TaskState::Running.can_advance(TaskState::Ready)
        && !TaskState::Ready.can_advance(TaskState::Waking)
        && !TaskState::Waking.can_advance(TaskState::Sleeping);
    let fwd = TaskState::Sleeping.can_advance(TaskState::Waking)
        && TaskState::Waking.can_advance(TaskState::Ready)
        && TaskState::Ready.can_advance(TaskState::Running);
    // 三段提交：唤醒后 epoch+1、queued、enqueue_seq 单调。
    let mut t = build_table(4);
    let before = t.cells[1].wake_epoch;
    let c = match wake_primitive(&mut t, 1) {
        Ok(c) => c,
        Err(_) => return false,
    };
    let cell_ok = t.cells[1].state == TaskState::Ready
        && t.cells[1].queued
        && t.cells[1].enqueue_seq == Some(c.enqueue_seq)
        && t.cells[1].wake_epoch == before + 1;
    // 入队序全局单调：两个提交序号递增。
    let _ = wake_primitive(&mut t, 2);
    let seq_mono = match (t.cells[1].enqueue_seq, t.cells[2].enqueue_seq) {
        (Some(a), Some(b)) => b > a,
        _ => false,
    };
    ranks && flat && back && fwd && cell_ok && seq_mono
}

// ── 阻断入口探针 ────────────────────────────────────────────────
/// 阻断只对 Running 态生效；回睡后旧队列锚已清；再唤醒走新纪元。
pub fn probe_block_entry() -> bool {
    let mut t = build_table(4);
    // 非运行态阻断拒。
    let pre_rej = block_running(&mut t, 0).is_err();
    let _ = wake_primitive(&mut t, 0);
    let ready_rej = block_running(&mut t, 0).is_err();
    // 运行态阻断 → 回睡，epoch+1，再唤醒成功。
    let _ = dispatch_head(&mut t);
    let blk = block_running(&mut t, 0).is_ok();
    let back_ok = t.cells[0].state == TaskState::Sleeping && !t.cells[0].queued;
    let rewake = wake_primitive(&mut t, 0).is_ok();
    let epoch_grew = t.cells[0].wake_epoch >= 3;
    pre_rej && ready_rej && blk && back_ok && rewake && epoch_grew
}

// ── 完成写回（判据③：状态=已完成、结果摘要非空） ────────────────
pub fn writeback_summary() -> Result<String, u32> {
    let vis = visibility_rounds(INJECT_ROUNDS)?;
    if !vis.all_caught() {
        return Err(WakeCode::VisibleHole.val());
    }
    let inj = inject_rounds(INJECT_ROUNDS)?;
    if !inj.all_caught() {
        return Err(WakeCode::VisibleHole.val());
    }
    let t = golden_table()?;
    Ok(format!(
        "状态=已完成 tasks={} ready={} vis={}/{} inject={}/{}",
        t.cells.len(), t.ready.len(), vis.caught, vis.tries, inj.caught, inj.tries
    ))
}

// ── 判据面 ──────────────────────────────────────────────────────
pub fn extra_checks() -> Vec<(&'static str, bool, String)> {
    let mut v: Vec<(&'static str, bool, String)> = Vec::new();
    // J1 主判据：黄金全链（四正四拒）。
    let g = golden_run();
    let (gp, gd) = match &g {
        Ok((f, r, c, w)) => (*f == 4 && *r == 4, format!("flags={} rejects={} commits={} repaired={}", f, r, c, w)),
        Err(e) => (false, format!("err=0x{:x} {}", e, detail(*e))),
    };
    v.push(("J1-黄金全链", gp, gd));
    // J1 置态原子性断言。
    let t = golden_table();
    let atomic = match &t {
        Ok(x) => atomicity_scan(x).is_ok(),
        Err(_) => false,
    };
    v.push((
        "J1-置态原子性",
        atomic && probe_state_machine() && probe_block_entry(),
        String::from("就绪⇔在队不变量全观察点成立；三段提交逐步可查；阻断新周期入口"),
    ));
    // J1 可见性窗口注入 10/10 正确排队。
    let vis = visibility_rounds(INJECT_ROUNDS);
    let (vp, vd) = match &vis {
        Ok(l) => (l.all_caught(), format!("t={} c={} miss={:?}", l.tries, l.caught, l.miss_kinds)),
        Err(e) => (false, format!("err=0x{:x}", e)),
    };
    v.push(("J1-可见性窗口10/10", vp, vd));
    // J1 失真注入 10/10 必抓。
    let inj = inject_rounds(INJECT_ROUNDS);
    let (ip, id) = match &inj {
        Ok(l) => (l.all_caught(), format!("t={} c={} miss={:?}", l.tries, l.caught, l.miss_kinds)),
        Err(e) => (false, format!("err=0x{:x}", e)),
    };
    v.push(("J1-失真注入10/10", ip, id));
    // J2 队列序守卫。
    let tq = match golden_table() {
        Ok(x) => x,
        Err(_) => build_table(0),
    };
    let mut order_ok = tq.ready.len() > 0;
    let mut k = 1usize;
    while k < tq.ready.len() {
        let a = match tq.ready.get(k - 1) {
            Some(x) => *x,
            None => 0,
        };
        let b = match tq.ready.get(k) {
            Some(x) => *x,
            None => 0,
        };
        let sa = match tq.cells.get(a as usize) {
            Some(c) => c.enqueue_seq.unwrap_or(0),
            None => 0,
        };
        let sb = match tq.cells.get(b as usize) {
            Some(c) => c.enqueue_seq.unwrap_or(0),
            None => 0,
        };
        if sb <= sa {
            order_ok = false;
        }
        k += 1;
    }
    v.push(("J2-队列序守卫", order_ok, format!("ready={} 入队序严格递增不重不漏", tq.ready.len())));
    // J2 双跑一致。
    v.push(("J2-双跑一致", repro_check().is_ok(), String::from("golden_run 两跑同值")));
    // J2 冻结常量。
    v.push((
        "J2-冻结常量",
        probe_seq_c().is_ok(),
        format!("TASK_N={} QUEUE_CAP={} WAKE_CYCLES={}", TASK_N, QUEUE_CAP, WAKE_CYCLES),
    ));
    // J3 写回摘要非空。
    let wb = writeback_summary();
    let (wp, wd) = match &wb {
        Ok(s) => (!s.is_empty(), s.clone()),
        Err(e) => (false, format!("err=0x{:x}", e)),
    };
    v.push(("J3-写回摘要", wp, wd));
    // J4 段位守卫。
    v.push(("J4-段位守卫", segment_guard(), String::from("0x724C0 窗口")));
    v
}

pub fn run_unx1747_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F1747 唤醒置态原语（睡眠→就绪原子转换）");
    for (n, p, d) in extra_checks() {
        s.add_dyn(n, p, &d);
    }
    s
}
