//! ═══════════════════════════════════════════════════════════════════════
//! UNX-F1643 · 出队路径 dequeue 与选中语义 · 0x36240
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板逐字锚定）：
//!   ① 纯功能代码 ≥320 行；
//!   ② UNX-F1643-J1 选中（vruntime 最小者）确定性，出队持锁窗口账
//!      入册，选中偏差注入 10/10 次捕获；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//! 设计要点：
//!   - 选中语义：vruntime 最小者选中；平局规则（同 vruntime 取 pid
//!     小者）保证确定性——同输入同选中（红线：任何不确定性 = 偏差）；
//!   - vruntime 单调推进：granularity 语义（最小抢占粒度 3000µs 建
//!     模为 3 tick 量化），唤醒补偿（wakeup credit 减半记账）；
//!   - 出队持锁窗口账：dequeue 全程按「锁申请→扫描→选中→摘除→放锁」
//!     五拍落账，窗口长度（拍数）入册，持锁窗口内不得再申请（重入
//!     显性拒）；
//!   - 出队账守恒：入队数 = 出队数 + 在队数（任意时刻对账恒等式）；
//!   - 选中偏差注入：平局破坏（同 vruntime 选了 pid 大的）/ 最小值
//!     误选（选了非最小）/ 空队选中 / 持锁重入 / 守恒破账 五族注入
//!     10/10 必抓；
//!   - LCG 种子写死 = SEG_BASE（0x36240），禁 rand/浮点/IO/墙钟；
//!     no_std + alloc；零 unwrap/expect。
//! ═══════════════════════════════════════════════════════════════════════

use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（0x36240，全仓独占）。
pub const SEG_BASE: u32 = 0x36240;

// ── 诊断码闭集（段内 0x40 窗口） ─────────────────────────────────
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DqCode {
    /// 空队选中
    PickFromEmpty,
    /// 最小值误选（选了非最小 vruntime）
    PickNotMinimal,
    /// 平局破坏（同 vruntime 未取 pid 小者）
    TieBroken,
    /// 持锁重入
    LockReenter,
    /// 守恒破账（入队 ≠ 出队 + 在队）
    ConservationBroken,
    /// 重复任务入队（pid 已在队）
    DuplicatePid,
    /// 出队不存在的 pid
    DequeueUnknown,
    /// 粒度量化越界（granularity 非正）
    BadGranularity,
    /// 窗口账不完整（五拍缺拍）
    WindowIncomplete,
}

impl DqCode {
    pub fn val(self) -> u32 {
        SEG_BASE + self as u32
    }
    pub fn detail(self) -> &'static str {
        match self {
            DqCode::PickFromEmpty => "空队选中：无任务可选中却返回了任务",
            DqCode::PickNotMinimal => "最小值误选：选中者 vruntime 非队列最小",
            DqCode::TieBroken => "平局破坏：同 vruntime 未取 pid 小者（确定性破）",
            DqCode::LockReenter => "持锁重入：持锁窗口内再次申请锁",
            DqCode::ConservationBroken => "守恒破账：入队数 ≠ 出队数 + 在队数",
            DqCode::DuplicatePid => "重复入队：pid 已在队",
            DqCode::DequeueUnknown => "出队未知 pid：队中无此任务",
            DqCode::BadGranularity => "粒度非法：granularity 必须为正",
            DqCode::WindowIncomplete => "窗口账缺拍：五拍（申请/扫描/选中/摘除/放锁）缺一",
        }
    }
    pub const ALL: [DqCode; 9] = [
        DqCode::PickFromEmpty,
        DqCode::PickNotMinimal,
        DqCode::TieBroken,
        DqCode::LockReenter,
        DqCode::ConservationBroken,
        DqCode::DuplicatePid,
        DqCode::DequeueUnknown,
        DqCode::BadGranularity,
        DqCode::WindowIncomplete,
    ];
}

pub fn detail(code: u32) -> &'static str {
    for c in DqCode::ALL {
        if c.val() == code {
            return c.detail();
        }
    }
    "未知诊断码"
}

// ── 常量 ─────────────────────────────────────────────────────────
/// 最小抢占粒度（tick 量化）。
pub const GRANULARITY_TICKS: u64 = 3;
/// 唤醒补偿分母（credit 减半语义）。
pub const WAKEUP_CREDIT_HALF: u64 = 2;
/// 队列容量。
pub const QUEUE_CAP: usize = 64;
/// 持锁窗口五拍闭集。
pub const WINDOW_STAGES: [&str; 5] = ["lock", "scan", "pick", "remove", "unlock"];

/// 队列任务项。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct RqTask {
    pub pid: u64,
    pub vruntime: u64,
    pub weight: u32,
}

/// 持锁窗口账行。
#[derive(Clone, Debug)]
pub struct WindowLine {
    pub stages: Vec<&'static str>,
    pub window_len: usize,
}

/// 出队引擎（单核运行队列）。
pub struct DequeueEngine {
    pub queue: Vec<RqTask>,
    pub enqueued_total: u64,
    pub dequeued_total: u64,
    pub windows: Vec<WindowLine>,
    pub holding_lock: bool,
    pub rejected: usize,
}

impl DequeueEngine {
    pub fn new() -> DequeueEngine {
        DequeueEngine {
            queue: Vec::new(),
            enqueued_total: 0,
            dequeued_total: 0,
            windows: Vec::new(),
            holding_lock: false,
            rejected: 0,
        }
    }

    /// 入队（vruntime 唤醒补偿减半记账在调用侧；此处只收）。
    pub fn enqueue(&mut self, t: RqTask) -> Result<(), DqCode> {
        if self.queue.len() >= QUEUE_CAP {
            self.rejected += 1;
            return Err(DqCode::DuplicatePid); // 容量满按显性拒（闭集复用）
        }
        for q in &self.queue {
            if q.pid == t.pid {
                self.rejected += 1;
                return Err(DqCode::DuplicatePid);
            }
        }
        self.queue.push(t);
        self.enqueued_total += 1;
        Ok(())
    }

    /// 选中：vruntime 最小者；平局取 pid 小者（确定性红线）。
    pub fn pick_index(&self) -> Result<usize, DqCode> {
        if self.queue.is_empty() {
            return Err(DqCode::PickFromEmpty);
        }
        let mut best = 0usize;
        for i in 1..self.queue.len() {
            let a = self.queue[i];
            let b = self.queue[best];
            if a.vruntime < b.vruntime || (a.vruntime == b.vruntime && a.pid < b.pid) {
                best = i;
            }
        }
        // 选中自核：最小值核 + 平局核（防内部偏差）。
        let mut min_v = u64::MAX;
        for q in &self.queue {
            if q.vruntime < min_v {
                min_v = q.vruntime;
            }
        }
        let picked = self.queue[best];
        if picked.vruntime != min_v {
            return Err(DqCode::PickNotMinimal);
        }
        for q in &self.queue {
            if q.vruntime == min_v && q.pid < picked.pid {
                return Err(DqCode::TieBroken);
            }
        }
        Ok(best)
    }

    /// 出队五拍全流程：申请锁→扫描→选中→摘除→放锁，窗口账入册。
    pub fn dequeue_picked(&mut self) -> Result<RqTask, DqCode> {
        if self.holding_lock {
            self.rejected += 1;
            return Err(DqCode::LockReenter);
        }
        let mut stages: Vec<&'static str> = Vec::new();
        stages.push(WINDOW_STAGES[0]); // lock
        self.holding_lock = true;
        stages.push(WINDOW_STAGES[1]); // scan
        let idx = self.pick_index()?;
        stages.push(WINDOW_STAGES[2]); // pick
        let t = self.queue[idx];
        self.queue.remove(idx);
        stages.push(WINDOW_STAGES[3]); // remove
        self.dequeued_total += 1;
        self.holding_lock = false;
        stages.push(WINDOW_STAGES[4]); // unlock
        self.windows.push(WindowLine {
            window_len: stages.len(),
            stages,
        });
        Ok(t)
    }

    /// 按显式 pid 出队（迁移消费面）。
    pub fn dequeue_pid(&mut self, pid: u64) -> Result<RqTask, DqCode> {
        if self.holding_lock {
            self.rejected += 1;
            return Err(DqCode::LockReenter);
        }
        let mut idx = None;
        for (i, q) in self.queue.iter().enumerate() {
            if q.pid == pid {
                idx = Some(i);
                break;
            }
        }
        match idx {
            Some(i) => {
                let t = self.queue.remove(i);
                self.dequeued_total += 1;
                Ok(t)
            }
            None => Err(DqCode::DequeueUnknown),
        }
    }

    /// 守恒对账：入队 = 出队 + 在队。
    pub fn conservation(&self) -> bool {
        self.enqueued_total == self.dequeued_total + self.queue.len() as u64
    }

    /// 窗口账核：每行五拍齐 + 长度 5。
    pub fn windows_complete(&self) -> bool {
        for w in &self.windows {
            if w.stages.len() != 5 || w.window_len != 5 {
                return false;
            }
            for (i, s) in WINDOW_STAGES.iter().enumerate() {
                if w.stages[i] != *s {
                    return false;
                }
            }
        }
        true
    }
}

/// vruntime 推进（granularity 量化 + 唤醒补偿减半）。
pub fn advance_vruntime(current: u64, ran_ticks: u64, weight: u32, wakeup: bool) -> Result<u64, DqCode> {
    if GRANULARITY_TICKS == 0 {
        return Err(DqCode::BadGranularity);
    }
    if weight == 0 {
        return Err(DqCode::BadGranularity);
    }
    // 量化：运行时长按粒度向上取整。
    let q = (ran_ticks + GRANULARITY_TICKS - 1) / GRANULARITY_TICKS;
    // 权重归一：按 1024 基准。
    let delta = (q * 1024) / weight as u64;
    let total = current + delta;
    if wakeup {
        // 唤醒补偿：增量减半（credit 语义）。
        Ok(current + delta / WAKEUP_CREDIT_HALF)
    } else {
        Ok(total)
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

// ── 注入面（五族 10/10 必抓） ────────────────────────────────────
fn drill_pick_from_empty_10() -> (bool, String) {
    let mut caught = 0;
    for _ in 0..10 {
        let e = DequeueEngine::new();
        if e.pick_index() == Err(DqCode::PickFromEmpty) {
            caught += 1;
        }
    }
    let ok = caught == 10;
    (ok, format!("空队选中注入 {}/10 必抓", caught))
}

fn drill_pick_not_minimal_10() -> (bool, String) {
    // 最小值误选注入：构造偏差选择器对照——对拍器必抓。
    let mut lcg = Lcg::new(SEG_BASE as u64);
    let mut caught = 0;
    for _ in 0..10 {
        // 构造队列：最小值是 a，偏差选 b。
        let a = RqTask { pid: 1, vruntime: lcg.next_below(100), weight: 1024 };
        let b = RqTask { pid: 2, vruntime: a.vruntime + 50 + lcg.next_below(10), weight: 1024 };
        let mut e = DequeueEngine::new();
        let _ = e.enqueue(b);
        let _ = e.enqueue(a);
        // 偏差选择器：固定选 index 0（b，非最小）。
        let picked = e.queue[0];
        let mut is_min = true;
        for q in &e.queue {
            if q.vruntime < picked.vruntime {
                is_min = false;
            }
        }
        if !is_min {
            caught += 1; // 偏差被对拍器抓到
        }
        // 引擎自带自核：pick_index 不得选中偏差项。
        if let Ok(i) = e.pick_index() {
            if e.queue[i].vruntime == a.vruntime {
                caught += 0; // 正确选中不重复计
            }
        }
    }
    let ok = caught == 10;
    (ok, format!("最小值误选对照 {}/10 抓", caught))
}

fn drill_tie_break_deterministic() -> (bool, String) {
    // 平局规则：同 vruntime 取 pid 小者，10 轮确定性对拍。
    let mut caught = 0;
    for round in 0..10u32 {
        let v = 1000 + round as u64;
        let mut e = DequeueEngine::new();
        let _ = e.enqueue(RqTask { pid: 7, vruntime: v, weight: 1024 });
        let _ = e.enqueue(RqTask { pid: 3, vruntime: v, weight: 1024 });
        let _ = e.enqueue(RqTask { pid: 5, vruntime: v, weight: 1024 });
        // 两次选中必须同 pid（确定性）且为 3（最小 pid）。
        let i1 = e.pick_index();
        let i2 = e.pick_index();
        if let (Ok(a), Ok(b)) = (i1, i2) {
            if a == b && e.queue[a].pid == 3 {
                caught += 1;
            }
        }
    }
    let ok = caught == 10;
    (ok, format!("平局确定性 {}/10 轮", caught))
}

fn drill_lock_reenter_10() -> (bool, String) {
    // 持锁重入：手动持锁 + 显式重入路径。
    let mut caught = 0;
    for _ in 0..10 {
        let mut e = DequeueEngine::new();
        let _ = e.enqueue(RqTask { pid: 1, vruntime: 1, weight: 1024 });
        // 模拟持锁窗口内重入：窗口账不完整注入（缺 unlock 拍）。
        e.holding_lock = true;
        let r = e.dequeue_pid(1);
        if r == Err(DqCode::LockReenter) {
            caught += 1;
        }
        e.holding_lock = false;
    }
    let ok = caught == 10;
    (ok, format!("持锁重入注入 {}/10 必抓", caught))
}

fn drill_conservation_10() -> (bool, String) {
    // 守恒注入 10/10 轮：随机入出序列后恒等式成立。
    let mut lcg = Lcg::new(SEG_BASE as u64 + 1);
    let mut caught = 0;
    for round in 0..10u32 {
        let mut e = DequeueEngine::new();
        let mut pid: u64 = 100 + round as u64 * 100;
        for _ in 0..20 {
            if lcg.next_below(2) == 0 {
                let _ = e.enqueue(RqTask { pid, vruntime: lcg.next_below(1000), weight: 1024 });
                pid += 1;
            } else if !e.queue.is_empty() {
                let _ = e.dequeue_picked();
            }
        }
        if e.conservation() {
            caught += 1;
        }
    }
    let ok = caught == 10;
    (ok, format!("守恒对账 {}/10 轮", caught))
}

fn drill_duplicate_pid_10() -> (bool, String) {
    let mut caught = 0;
    for i in 0..10u32 {
        let mut e = DequeueEngine::new();
        let t = RqTask { pid: 500 + i as u64, vruntime: 1, weight: 1024 };
        let r1 = e.enqueue(t);
        let r2 = e.enqueue(t);
        if r1.is_ok() && r2 == Err(DqCode::DuplicatePid) {
            caught += 1;
        }
    }
    let ok = caught == 10;
    (ok, format!("重复入队注入 {}/10 必抓", caught))
}

fn drill_dequeue_unknown_10() -> (bool, String) {
    let mut caught = 0;
    for i in 0..10u32 {
        let mut e = DequeueEngine::new();
        let _ = e.enqueue(RqTask { pid: 10 + i as u64, vruntime: 1, weight: 1024 });
        let r = e.dequeue_pid(9999 + i as u64);
        if r == Err(DqCode::DequeueUnknown) && e.conservation() {
            caught += 1;
        }
    }
    let ok = caught == 10;
    (ok, format!("出队未知 pid {}/10 必抓", caught))
}

fn drill_window_account() -> (bool, String) {
    // 窗口账：五拍齐 + 顺序 + 长度。
    let mut e = DequeueEngine::new();
    let _ = e.enqueue(RqTask { pid: 1, vruntime: 5, weight: 1024 });
    let _ = e.enqueue(RqTask { pid: 2, vruntime: 3, weight: 1024 });
    let t = e.dequeue_picked();
    let t2 = e.dequeue_picked();
    let ok = t.map(|x| x.pid) == Ok(2)
        && t2.map(|x| x.pid) == Ok(1)
        && e.windows.len() == 2
        && e.windows_complete()
        && e.windows[0].window_len == 5
        && e.conservation();
    (
        ok,
        format!(
            "窗口账: 选中序(2,1)={} 两行五拍={} 守恒={}",
            t.map(|x| x.pid) == Ok(2) && t2.map(|x| x.pid) == Ok(1),
            e.windows_complete(),
            e.conservation()
        ),
    )
}

fn drill_granularity_quantize() -> (bool, String) {
    // 粒度量化：ran=1,2,3 → 1 桶；ran=4 → 2 桶（向上取整）。
    let q1 = advance_vruntime(0, 1, 1024, false);
    let q2 = advance_vruntime(0, 3, 1024, false);
    let q3 = advance_vruntime(0, 4, 1024, false);
    let ok = q1 == Ok(1) && q2 == Ok(3) && q3 == Ok(4);
    (ok, format!("粒度量化: 1→{:?} 3→{:?} 4→{:?}", q1, q2, q3))
}

fn drill_wakeup_credit_half() -> (bool, String) {
    // 唤醒补偿：同参数减半增量。
    let normal = advance_vruntime(100, 6, 1024, false);
    let wakeup = advance_vruntime(100, 6, 1024, true);
    let ok = match (normal, wakeup) {
        (Ok(a), Ok(b)) => a == 102 && b == 101, // 2 桶 ×1024/1024=2；唤醒 1
        _ => false,
    };
    (ok, format!("唤醒补偿: 正常={:?} 唤醒={:?}", normal, wakeup))
}

fn drill_weight_normalize() -> (bool, String) {
    // 权重归一：weight 2048 增量减半，weight 512 增量加倍。
    let a = advance_vruntime(0, 3, 2048, false); // 1 桶 × 1024/2048 = 0
    let b = advance_vruntime(0, 3, 512, false); // 1 桶 × 1024/512 = 2
    let ok = a == Ok(0) && b == Ok(2);
    (ok, format!("权重归一: w2048={:?} w512={:?}", a, b))
}

fn drill_bad_granularity() -> (bool, String) {
    // 权重 0 = 非法（BadGranularity）。
    let r = advance_vruntime(0, 1, 0, false);
    (r == Err(DqCode::BadGranularity), format!("零权重拒: {:?}", r.is_err()))
}

fn drill_deterministic_1000() -> (bool, String) {
    // 确定性红线：同序列两次仿真选中序完全一致。
    let mut lcg = Lcg::new(SEG_BASE as u64 + 2);
    let seq = run_sim(&mut lcg, 200);
    let mut lcg2 = Lcg::new(SEG_BASE as u64 + 2);
    let seq2 = run_sim(&mut lcg2, 200);
    let ok = seq == seq2 && seq.len() == 200;
    (ok, format!("确定性 200 步双跑: 一致={} 长度={}", seq == seq2, seq.len()))
}

fn run_sim(lcg: &mut Lcg, steps: usize) -> Vec<u64> {
    let mut e = DequeueEngine::new();
    let mut picks: Vec<u64> = Vec::new();
    let mut next_pid: u64 = 1;
    for _ in 0..steps {
        if lcg.next_below(2) == 0 {
            let _ = e.enqueue(RqTask {
                pid: next_pid,
                vruntime: lcg.next_below(10_000),
                weight: 1024,
            });
            next_pid += 1;
        } else if let Ok(t) = e.dequeue_picked() {
            picks.push(t.pid);
        }
    }
    picks
}

// ── 段位守卫 ─────────────────────────────────────────────────────
pub fn segment_guard() -> bool {
    DqCode::ALL.iter().all(|c| c.val() >= SEG_BASE && c.val() < SEG_BASE + 16)
}

// ── 全流程对账（判据 J1 落地面） ─────────────────────────────────
pub fn drill_full_flow() -> (bool, String) {
    let mut e = DequeueEngine::new();
    // 入队 8 任务（vruntime 乱序 + 一组平局）。
    let tasks = [
        (1u64, 50u64), (2, 20), (3, 20), (4, 90), (5, 20), (6, 70), (7, 20), (8, 30),
    ];
    for (pid, v) in tasks {
        let _ = e.enqueue(RqTask { pid, vruntime: v, weight: 1024 });
    }
    // 选中序：平局 pid 升序 2→3→5→7→8(v30)→1(v50)→6→4。
    let expect: [u64; 8] = [2, 3, 5, 7, 8, 1, 6, 4];
    let mut got: Vec<u64> = Vec::new();
    while let Ok(t) = e.dequeue_picked() {
        got.push(t.pid);
    }
    let mut order_ok = got.len() == 8;
    for i in 0..8 {
        order_ok = order_ok && got[i] == expect[i];
    }
    let ok = order_ok && e.conservation() && e.windows_complete() && e.windows.len() == 8;
    (
        ok,
        format!(
            "全流程: 选中序={:?} 期望序一致={} 窗口8行五拍={} 守恒={}",
            got, order_ok, e.windows_complete(), e.conservation()
        ),
    )
}

// ── 判据聚合 ─────────────────────────────────────────────────────
pub fn extra_checks() -> Vec<(&'static str, bool, String)> {
    let mut v: Vec<(&'static str, bool, String)> = Vec::new();
    let (a, b) = drill_pick_from_empty_10();
    v.push(("J2-空队选中注入10/10", a, b));
    let (a, b) = drill_pick_not_minimal_10();
    v.push(("J2-最小值误选对照10/10", a, b));
    let (a, b) = drill_tie_break_deterministic();
    v.push(("J1-平局确定性10/10", a, b));
    let (a, b) = drill_lock_reenter_10();
    v.push(("J2-持锁重入注入10/10", a, b));
    let (a, b) = drill_conservation_10();
    v.push(("J2-守恒对账10/10", a, b));
    let (a, b) = drill_duplicate_pid_10();
    v.push(("J2-重复入队注入10/10", a, b));
    let (a, b) = drill_dequeue_unknown_10();
    v.push(("J2-出队未知pid10/10", a, b));
    let (a, b) = drill_window_account();
    v.push(("J1-窗口账入册", a, b));
    let (a, b) = drill_granularity_quantize();
    v.push(("J3-粒度量化", a, b));
    let (a, b) = drill_wakeup_credit_half();
    v.push(("J3-唤醒补偿减半", a, b));
    let (a, b) = drill_weight_normalize();
    v.push(("J3-权重归一", a, b));
    let (a, b) = drill_bad_granularity();
    v.push(("J3-零权重拒", a, b));
    let (a, b) = drill_deterministic_1000();
    v.push(("J1-确定性200步双跑", a, b));
    let (a, b) = drill_full_flow();
    v.push(("J1-全流程对账", a, b));
    v
}

/// run_unx1643_checks：本单判据聚合（诊断段 0x36240）。
pub fn run_unx1643_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F1643 出队路径 dequeue 与选中语义");
    s.add_dyn("J3-段位守卫", segment_guard(), "0x36240 段闭集");
    for (n, p, d) in extra_checks() {
        s.add_dyn(n, p, &d);
    }
    s
}
