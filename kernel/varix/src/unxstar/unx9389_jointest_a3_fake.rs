//! UNX-F9389 阻塞唤醒原语联测（A3 fake × C4 等待队列） · 0x7CEC0段
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（卡面 UNX-F9389-J1 逐字）：
//!   锚点：UNX-F9389-J1 原语联测判据过，A3 fake 原语与 C4 等待队列
//!   对接可观测，Schema 先行声明格过
//!
//! 数据段 SEG_BASE = 0x7CEC0：诊断码闭集 +0x01..+0x0B（11 码，
//! 9389 独占窗口，ALL 闭集必填，detail() 全中文非空）。
//!
//! 结构清单：
//!   - Schema 先行声明：PRIM_SCHEMA（版本/等待队列容量/唤醒语义）
//!     必须在联测前登记，联测逐项比对，无声明即拒绝；
//!   - 状态机：A3 fake 原语 Unlocked→Locked→Released 单向迁移，
//!     销毁后引用显性拒绝（UseDestroyed）；
//!   - 队列：C4 等待队列 FIFO 定容，满/空/重复入队逐边界处理；
//!   - 账本：JointLedger（入队=唤醒=回收三段守恒）+ 注入账；
//!   - 注入核：5 类坏样本（未声明 Schema / 销毁后使用 / 满队入队 /
//!     空队唤醒 / 重复入队）逐类显性拒绝，逃逸=0；
//!   - 对拍：Linux errno 对照（EINVAL/EBADF/EAGAIN/EINTR/ENOENT）；
//!   - 指纹：FNV-1a 联测事件序列双跑指纹。
//! ─────────────────────────────────────────────────────────────────────

use alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（F9389 独占 0x40 窗口）。
pub const SEG_BASE: u32 = 0x7CEC0;

/// Schema 版本号（先行声明闭集：联测只认此版本）。
pub const SCHEMA_VERSION: u32 = 1;

/// C4 等待队列定容（8 等待者）。
pub const QUEUE_CAP: usize = 8;

/// A3 fake 联测轮数。
pub const JOINT_ROUNDS: u32 = 64;

/// 注入坏样本类别数。
pub const BAD_CLASSES: u32 = 5;

/// Linux errno 数值（正数口径）。
pub const ENOENT: i32 = 2;
pub const EINTR: i32 = 4;
pub const EBADF: i32 = 9;
pub const EAGAIN: i32 = 11;
pub const EINVAL: i32 = 22;

// ── 诊断码闭集 ────────────────────────────────────────────────────────
/// 本模块诊断码（SEG_BASE + n，n ∈ 1..=11 闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PrimCode {
    /// Schema 未声明即联测
    SchemaMissing,
    /// Schema 版本不匹配
    SchemaVersionMismatch,
    /// 原语已销毁仍被引用
    UseDestroyed,
    /// 等待队列已满仍入队
    QueueFull,
    /// 空队列唤醒
    WakeEmpty,
    /// 重复入队：同一原语已在队列中
    DuplicateWait,
    /// 唤醒者与等待者不配对
    WakeWaiterMismatch,
    /// 迁移非法：Locked 上重复加锁
    DoubleLock,
    /// 账本倒退
    LedgerRegress,
    /// 表外错误码
    UnknownErrno,
    /// 双跑不一致
    NotReproducible,
}

impl PrimCode {
    /// 闭集数组（判据遍历）。
    pub const ALL: [PrimCode; 11] = [
        PrimCode::SchemaMissing,
        PrimCode::SchemaVersionMismatch,
        PrimCode::UseDestroyed,
        PrimCode::QueueFull,
        PrimCode::WakeEmpty,
        PrimCode::DuplicateWait,
        PrimCode::WakeWaiterMismatch,
        PrimCode::DoubleLock,
        PrimCode::LedgerRegress,
        PrimCode::UnknownErrno,
        PrimCode::NotReproducible,
    ];

    /// SEG_BASE + 段内序号（1 起）。
    pub fn val(self) -> u32 {
        SEG_BASE
            + match self {
                PrimCode::SchemaMissing => 1,
                PrimCode::SchemaVersionMismatch => 2,
                PrimCode::UseDestroyed => 3,
                PrimCode::QueueFull => 4,
                PrimCode::WakeEmpty => 5,
                PrimCode::DuplicateWait => 6,
                PrimCode::WakeWaiterMismatch => 7,
                PrimCode::DoubleLock => 8,
                PrimCode::LedgerRegress => 9,
                PrimCode::UnknownErrno => 10,
                PrimCode::NotReproducible => 11,
            }
    }

    /// 非空中文处置建议。
    pub fn detail(self) -> &'static str {
        match self {
            PrimCode::SchemaMissing => "联测前必须先声明 Schema，无声明一律拒绝",
            PrimCode::SchemaVersionMismatch => "Schema 版本闭集只认 v1，版本漂移即拒绝",
            PrimCode::UseDestroyed => "fake 原语销毁后引用是未定义行为，必须显性拒绝",
            PrimCode::QueueFull => "等待队列定容 8，满队入队须回 EAGAIN 口径",
            PrimCode::WakeEmpty => "空队列唤醒无对象，必须显性拒绝不得静默",
            PrimCode::DuplicateWait => "同一原语不允许重复入队，防假唤醒叠加",
            PrimCode::WakeWaiterMismatch => "唤醒令牌必须与队首等待者配对",
            PrimCode::DoubleLock => "Locked 上重复加锁是死锁候选，显性拒绝",
            PrimCode::LedgerRegress => "联测账只增不减，倒退即审计失败",
            PrimCode::UnknownErrno => "错误码必须落在 errno 对照表闭集内",
            PrimCode::NotReproducible => "同输入双跑指纹必须逐位一致",
        }
    }
}

// ── Schema 先行声明 ───────────────────────────────────────────────────
/// A3 fake 原语 Schema（联测前逐字段登记）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct PrimSchema {
    /// 版本号（只认 SCHEMA_VERSION）
    pub version: u32,
    /// 等待队列容量（必须与 QUEUE_CAP 一致）
    pub queue_cap: usize,
    /// 唤醒语义：true = 唤醒一个（FIFO 队首）
    pub wake_one_fifo: bool,
}

/// Schema 声明核验：三字段全对才算「先行声明成立」。
pub fn declare_schema(s: PrimSchema) -> Result<(), PrimCode> {
    if s.version != SCHEMA_VERSION {
        return Err(PrimCode::SchemaVersionMismatch);
    }
    if s.queue_cap != QUEUE_CAP {
        return Err(PrimCode::SchemaVersionMismatch);
    }
    if !s.wake_one_fifo {
        return Err(PrimCode::SchemaVersionMismatch);
    }
    Ok(())
}

// ── 状态机 ────────────────────────────────────────────────────────────
/// A3 fake 原语状态：Unlocked→Locked→Released 单向，销毁为旁路终态。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PrimState {
    /// 未锁：可加锁
    Unlocked,
    /// 已锁：等待队列可入队
    Locked,
    /// 已释放：唤醒等待者后进入
    Released,
    /// 已销毁：任何引用显性拒绝
    Destroyed,
}

impl PrimState {
    /// 单向迁移核验（销毁可从任意活跃态进入）。
    pub fn advance(self, want: PrimState) -> Option<PrimState> {
        let legal = match (self, want) {
            (PrimState::Unlocked, PrimState::Locked) => true,
            (PrimState::Locked, PrimState::Released) => true,
            (PrimState::Unlocked, PrimState::Destroyed) => true,
            (PrimState::Locked, PrimState::Destroyed) => true,
            _ => false,
        };
        if legal { Some(want) } else { None }
    }
}

// ── C4 等待队列 ───────────────────────────────────────────────────────
/// 定容 FIFO 等待队列（等待者 = 原语槽位号 + 1，0 表空）。
pub struct WaitQueue {
    slots: [u32; QUEUE_CAP],
    head: usize,
    tail: usize,
    len: usize,
    /// 累计入队数（账本）
    pub enqueued: u32,
    /// 累计唤醒数（账本）
    pub woken: u32,
    /// 累计拒绝数（账本）
    pub rejected: u32,
}

impl WaitQueue {
    /// 建空队列（全零初始化，不用裸下标写入非零）。
    pub fn new() -> WaitQueue {
        WaitQueue {
            slots: [0; QUEUE_CAP],
            head: 0,
            tail: 0,
            len: 0,
            enqueued: 0,
            woken: 0,
            rejected: 0,
        }
    }

    /// 当前排队长度。
    pub fn waiters(&self) -> usize {
        self.len
    }

    /// 入队：满/重复/非法号显性拒绝。
    pub fn enqueue(&mut self, waiter: u32) -> Result<(), PrimCode> {
        if waiter == 0 {
            self.rejected += 1;
            return Err(PrimCode::WakeWaiterMismatch);
        }
        if self.len >= QUEUE_CAP {
            self.rejected += 1;
            return Err(PrimCode::QueueFull);
        }
        // 重复入队检测：FIFO 内同号只允许一份
        let mut i = 0usize;
        while i < self.len {
            let idx = (self.head + i) % QUEUE_CAP;
            if let Some(&v) = self.slots.get(idx) {
                if v == waiter {
                    self.rejected += 1;
                    return Err(PrimCode::DuplicateWait);
                }
            }
            i += 1;
        }
        if let Some(s) = self.slots.get_mut(self.tail) {
            *s = waiter;
        }
        self.tail = (self.tail + 1) % QUEUE_CAP;
        self.len += 1;
        self.enqueued += 1;
        Ok(())
    }

    /// 唤醒一个（FIFO 队首）：空队显性拒绝。
    pub fn wake_one(&mut self) -> Result<u32, PrimCode> {
        if self.len == 0 {
            self.rejected += 1;
            return Err(PrimCode::WakeEmpty);
        }
        let v = match self.slots.get(self.head) {
            Some(&v) => v,
            None => {
                self.rejected += 1;
                return Err(PrimCode::WakeEmpty);
            }
        };
        if let Some(s) = self.slots.get_mut(self.head) {
            *s = 0;
        }
        self.head = (self.head + 1) % QUEUE_CAP;
        self.len -= 1;
        self.woken += 1;
        Ok(v)
    }
}

// ── 联测账本 ──────────────────────────────────────────────────────────
/// 联测账本：加锁=入队=唤醒=回收 四段守恒。
#[derive(Clone, Copy)]
pub struct JointLedger {
    pub locked: u32,
    pub enqueued: u32,
    pub woken: u32,
    pub reclaimed: u32,
}

impl JointLedger {
    /// 零账初值。
    pub fn new() -> JointLedger {
        JointLedger { locked: 0, enqueued: 0, woken: 0, reclaimed: 0 }
    }

    /// 四段守恒。
    pub fn conserved(&self) -> bool {
        self.locked == self.enqueued
            && self.enqueued == self.woken
            && self.woken == self.reclaimed
    }
}

/// 注入账（同 9388 口径：注入=检出+逃逸，逃逸恒 0）。
#[derive(Clone, Copy)]
pub struct JointInjectLedger {
    pub injected: u32,
    pub detected: u32,
    pub escaped: u32,
}

impl JointInjectLedger {
    /// 零账初值。
    pub fn new() -> JointInjectLedger {
        JointInjectLedger { injected: 0, detected: 0, escaped: 0 }
    }

    /// 记一笔。
    pub fn record(&mut self, caught: bool) {
        self.injected += 1;
        if caught { self.detected += 1; } else { self.escaped += 1; }
    }

    /// 守恒。
    pub fn conserved(&self) -> bool {
        self.injected == self.detected + self.escaped && self.escaped == 0
    }
}

// ── errno 对照表 ──────────────────────────────────────────────────────
/// 本模块 errno 闭集。
pub const ERRNO_TABLE: [i32; 5] = [ENOENT, EINTR, EBADF, EAGAIN, EINVAL];

/// 表内码逐码对拍。
pub fn agrees_with_linux(code: i32) -> bool {
    if code < 0 {
        return ERRNO_TABLE.contains(&-code);
    }
    ERRNO_TABLE.contains(&code)
}

// ── FNV-1a 指纹 ───────────────────────────────────────────────────────
/// FNV-1a 32 位哈希。
pub fn fnv1a(bytes: &[u8]) -> u32 {
    let mut h: u32 = 0x811C_9DC5;
    for &b in bytes {
        h ^= b as u32;
        h = h.wrapping_mul(0x0100_0193);
    }
    h
}

/// 联测事件序列指纹。
pub fn joint_fingerprint(events: &[u32]) -> u32 {
    let mut buf: Vec<u8> = Vec::new();
    for e in events {
        buf.extend_from_slice(&e.to_le_bytes());
    }
    fnv1a(&buf)
}

// ── A3 fake × C4 联测主循环 ───────────────────────────────────────────
/// 联测主循环：Schema 先行声明后，每轮 加锁→入队→唤醒→回收。
///
/// 返回 (联测账, 事件序列)；事件序列用于双跑指纹对拍。
pub fn run_a3_c4_jointest() -> (JointLedger, Vec<u32>) {
    // Schema 先行声明：版本/容量/语义逐字段对拍，失败则联测不启动
    let schema = PrimSchema { version: SCHEMA_VERSION, queue_cap: QUEUE_CAP, wake_one_fifo: true };
    let schema_ok = declare_schema(schema).is_ok();

    let mut ledger = JointLedger::new();
    let mut events: Vec<u32> = Vec::new();
    if !schema_ok {
        return (ledger, events);
    }

    let mut queue = WaitQueue::new();
    let mut round = 0u32;
    while round < JOINT_ROUNDS {
        // 每轮一个新 A3 fake 原语槽位（单向闭集内建模，不复用终态槽）
        let mut state = PrimState::Unlocked;
        // A3 fake：Unlocked→Locked
        if let Some(next) = state.advance(PrimState::Locked) {
            state = next;
            ledger.locked += 1;
        }
        // C4：入队（等待者号 = 轮次 + 1，保证唯一不重复）
        let waiter = round + 1;
        if queue.enqueue(waiter).is_ok() {
            ledger.enqueued += 1;
        }
        // C4：唤醒队首
        if let Ok(w) = queue.wake_one() {
            ledger.woken += 1;
            events.push(w);
        }
        // A3 fake：Locked→Released 后回收；终态标记入事件流供指纹
        if let Some(next) = state.advance(PrimState::Released) {
            state = next;
            ledger.reclaimed += 1;
            events.push(state as u32 + 0x1000);
        }
        round += 1;
    }
    (ledger, events)
}

// ── 注入核 ────────────────────────────────────────────────────────────
/// 5 类坏样本逐类注入，返回各类是否都被显性拒绝。
pub fn run_injection_core() -> ([bool; 5], JointInjectLedger) {
    let mut ok = [false; 5];
    let mut ledger = JointInjectLedger::new();

    // 类 1：Schema 未声明（版本为 0）必须拒绝
    let no_schema = declare_schema(PrimSchema { version: 0, queue_cap: QUEUE_CAP, wake_one_fifo: true })
        == Err(PrimCode::SchemaVersionMismatch);
    if no_schema {
        ledger.record(true);
        ok[0] = true;
    } else {
        ledger.record(false);
    }

    // 类 2：销毁后引用——Destroyed 不迁移任何态
    let dead = PrimState::Destroyed;
    let dead_ref = dead.advance(PrimState::Locked).is_none()
        && dead.advance(PrimState::Released).is_none()
        && dead.advance(PrimState::Destroyed).is_none();
    if dead_ref {
        ledger.record(true);
        ok[1] = true;
    } else {
        ledger.record(false);
    }

    // 类 3：满队入队
    let mut q = WaitQueue::new();
    let mut i = 1u32;
    while i <= QUEUE_CAP as u32 {
        let _ = q.enqueue(i);
        i += 1;
    }
    let full = q.enqueue(QUEUE_CAP as u32 + 1) == Err(PrimCode::QueueFull);
    if full {
        ledger.record(true);
        ok[2] = true;
    } else {
        ledger.record(false);
    }

    // 类 4：空队唤醒
    let mut q2 = WaitQueue::new();
    let empty_wake = q2.wake_one() == Err(PrimCode::WakeEmpty);
    if empty_wake {
        ledger.record(true);
        ok[3] = true;
    } else {
        ledger.record(false);
    }

    // 类 5：重复入队 + 非法等待者号
    let mut q3 = WaitQueue::new();
    let _ = q3.enqueue(3);
    let dup = q3.enqueue(3) == Err(PrimCode::DuplicateWait)
        && q3.enqueue(0) == Err(PrimCode::WakeWaiterMismatch);
    if dup {
        ledger.record(true);
        ok[4] = true;
    } else {
        ledger.record(false);
    }

    (ok, ledger)
}

// ── 判据 ──────────────────────────────────────────────────────────────
/// UNX-F9389 自检：A3 fake × C4 联测判据（约 14 条）。
pub fn run_unx9389_checks() -> CheckSet {
    let mut s = CheckSet::new("unx9389");

    // 1. Schema 先行声明成立
    let schema_ok = declare_schema(PrimSchema {
        version: SCHEMA_VERSION,
        queue_cap: QUEUE_CAP,
        wake_one_fifo: true,
    })
    .is_ok();
    s.add("Schema 先行声明：v1 + 容量 8 + FIFO 语义全对拍", schema_ok,
          "联测前逐字段登记，缺字段即拒绝");

    // 2. Schema 版本漂移拒绝
    let ver_bad = declare_schema(PrimSchema { version: 2, queue_cap: QUEUE_CAP, wake_one_fifo: true })
        == Err(PrimCode::SchemaVersionMismatch)
        && declare_schema(PrimSchema { version: SCHEMA_VERSION, queue_cap: 4, wake_one_fifo: true })
            == Err(PrimCode::SchemaVersionMismatch);
    s.add("Schema 漂移：版本/容量任一不符即拒绝", ver_bad,
          "声明闭集不允许部分匹配");

    // 3. 状态机单向迁移
    let fsm_ok = PrimState::Unlocked.advance(PrimState::Locked) == Some(PrimState::Locked)
        && PrimState::Locked.advance(PrimState::Released) == Some(PrimState::Released)
        && PrimState::Released.advance(PrimState::Locked).is_none()
        && PrimState::Unlocked.advance(PrimState::Released).is_none();
    s.add("A3 fake 状态机：Unlocked→Locked→Released 单向", fsm_ok,
          "回退与跳级全部拒绝");

    // 4. 销毁旁路：任意活跃态可销毁，销毁后零迁移
    let destroy_ok = PrimState::Locked.advance(PrimState::Destroyed) == Some(PrimState::Destroyed)
        && PrimState::Destroyed.advance(PrimState::Locked).is_none();
    s.add("销毁旁路：Destroyed 后任何引用显性拒绝", destroy_ok,
          "fake 原语销毁后不得复活");

    // 5. 联测主循环四段守恒
    let (ledger, events) = run_a3_c4_jointest();
    s.add("联测四段守恒：加锁=入队=唤醒=回收", ledger.conserved(),
          "A3 fake 与 C4 队列对接逐段记账");

    // 6. 联测轮数足额：64 轮全贯通
    s.add("联测足额：64 轮全部回收", ledger.reclaimed == JOINT_ROUNDS,
          "逐轮贯通不得中途丢段");

    // 7. FIFO 顺序：唤醒事件（值 ≤ JOINT_ROUNDS）必须按入队序递增
    let fifo_ok = {
        let mut in_order = true;
        let mut wakes = 0usize;
        let mut i = 0usize;
        while i < events.len() {
            if let Some(&v) = events.get(i) {
                // 终态标记（≥0x1000）不是唤醒事件，跳过
                if v < 0x1000 {
                    if v != (wakes as u32) + 1 {
                        in_order = false;
                    }
                    wakes += 1;
                }
            }
            i += 1;
        }
        in_order && wakes == JOINT_ROUNDS as usize
    };
    s.add("FIFO 顺序：唤醒序列与入队序逐位一致", fifo_ok,
          "wake_one_fifo 声明必须被联测证实");

    // 8. 注入核 5 类逐类拒绝
    let (inj_ok, inj_ledger) = run_injection_core();
    let all_rej = inj_ok.iter().all(|x| *x);
    s.add("注入核：5 类坏样本逐类显性拒绝", all_rej,
          "无声明/销毁引用/满队/空队/重复入队全过闸");

    // 9. 注入账守恒
    s.add("注入账守恒：注入=检出+逃逸且逃逸=0", inj_ledger.conserved(),
          "5 注入必须 5 检出、0 逃逸");

    // 10. 队列边界账：满队/空队拒绝各计一笔
    let mut q = WaitQueue::new();
    let mut i = 1u32;
    while i <= QUEUE_CAP as u32 {
        let _ = q.enqueue(i);
        i += 1;
    }
    let _ = q.enqueue(99);
    let _ = q.wake_one();
    let _ = q.wake_one();
    let mut q2 = WaitQueue::new();
    let _ = q2.wake_one();
    let bounds_ok = q.rejected == 1 && q2.rejected == 1;
    s.add("队列边界账：满队与空队拒绝各记一笔", bounds_ok,
          "拒绝不静默，全部显性入账");

    // 11. errno 逐码对拍
    let codes_ok = ERRNO_TABLE.iter().all(|c| agrees_with_linux(*c))
        && agrees_with_linux(-EAGAIN)
        && !agrees_with_linux(0)
        && !agrees_with_linux(4242);
    s.add("errno 对照：5 码表内全中、表外拒绝", codes_ok,
          "错误码闭集对拍");

    // 12. 双跑指纹一致
    let (_, ev2) = run_a3_c4_jointest();
    let fp1 = joint_fingerprint(&events);
    let fp2 = joint_fingerprint(&ev2);
    s.add("双跑指纹：联测事件 FNV-1a 两次一致", fp1 == fp2,
          "确定性联测，无隐藏状态");

    // 13. 指纹区分度：改变事件序列必须变指纹
    let mut ev3 = ev2.clone();
    if let Some(last) = ev3.last_mut() {
        *last = last.wrapping_add(1);
    }
    s.add("指纹区分度：事件改动即指纹漂移", joint_fingerprint(&ev3) != fp1,
          "指纹必须能反映联测内容变化");

    // 14. 诊断码闭集：detail 全非空 + 码值唯一 + 段界吻合
    let text_ok = PrimCode::ALL.iter().all(|c| !c.detail().is_empty());
    let mut uniq = true;
    let mut i = 0usize;
    while i < PrimCode::ALL.len() {
        let mut j = i + 1;
        while j < PrimCode::ALL.len() {
            let a = PrimCode::ALL.get(i).map(|c| c.val()).unwrap_or(0);
            let b = PrimCode::ALL.get(j).map(|c| c.val()).unwrap_or(1);
            if a == b {
                uniq = false;
            }
            j += 1;
        }
        i += 1;
    }
    let span_ok = PrimCode::ALL.first().map(|c| c.val()) == Some(SEG_BASE + 1)
        && PrimCode::ALL.last().map(|c| c.val()) == Some(SEG_BASE + 11);
    s.add("诊断码闭集：11 码唯一、文案非空、段界吻合", text_ok && uniq && span_ok,
          "SEG_BASE+1..=+11 闭集");

    // 动态明细（≤8）
    let led_text = format!(
        "锁 {} / 队 {} / 唤 {} / 收 {}",
        ledger.locked, ledger.enqueued, ledger.woken, ledger.reclaimed
    );
    s.add_dyn("联测账快照", ledger.conserved(), &led_text);
    let inj_text = format!(
        "注入 {} / 检出 {} / 逃逸 {}",
        inj_ledger.injected, inj_ledger.detected, inj_ledger.escaped
    );
    s.add_dyn("注入账快照", inj_ledger.conserved(), &inj_text);

    s
}

// ── 测试 ──────────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn schema_declaration_first() {
        assert!(declare_schema(PrimSchema {
            version: SCHEMA_VERSION,
            queue_cap: QUEUE_CAP,
            wake_one_fifo: true,
        })
        .is_ok());
        assert_eq!(
            declare_schema(PrimSchema { version: 9, queue_cap: QUEUE_CAP, wake_one_fifo: true }),
            Err(PrimCode::SchemaVersionMismatch)
        );
    }

    #[test]
    fn prim_state_one_way() {
        assert_eq!(PrimState::Unlocked.advance(PrimState::Locked), Some(PrimState::Locked));
        assert_eq!(PrimState::Locked.advance(PrimState::Released), Some(PrimState::Released));
        assert_eq!(PrimState::Released.advance(PrimState::Locked), None);
        assert_eq!(PrimState::Destroyed.advance(PrimState::Unlocked), None);
    }

    #[test]
    fn queue_fifo_order() {
        let mut q = WaitQueue::new();
        let _ = q.enqueue(5);
        let _ = q.enqueue(6);
        let _ = q.enqueue(7);
        assert_eq!(q.wake_one().unwrap_or(0), 5);
        assert_eq!(q.wake_one().unwrap_or(0), 6);
        assert_eq!(q.waiters(), 1);
    }

    #[test]
    fn queue_full_rejects() {
        let mut q = WaitQueue::new();
        let mut i = 1u32;
        while i <= QUEUE_CAP as u32 {
            assert!(q.enqueue(i).is_ok());
            i += 1;
        }
        assert_eq!(q.enqueue(100), Err(PrimCode::QueueFull));
        assert_eq!(q.rejected, 1);
    }

    #[test]
    fn queue_empty_wake_rejects() {
        let mut q = WaitQueue::new();
        assert_eq!(q.wake_one(), Err(PrimCode::WakeEmpty));
    }

    #[test]
    fn duplicate_wait_rejects() {
        let mut q = WaitQueue::new();
        assert!(q.enqueue(4).is_ok());
        assert_eq!(q.enqueue(4), Err(PrimCode::DuplicateWait));
        assert_eq!(q.enqueue(0), Err(PrimCode::WakeWaiterMismatch));
    }

    #[test]
    fn jointest_conserved_and_fifo() {
        let (l, ev) = run_a3_c4_jointest();
        assert!(l.conserved());
        assert_eq!(l.reclaimed, JOINT_ROUNDS);
        // 唤醒事件（<0x1000）逐个按入队序出现
        let wakes: Vec<u32> = ev.iter().copied().filter(|v| *v < 0x1000).collect();
        assert_eq!(wakes.len(), JOINT_ROUNDS as usize);
        assert_eq!(wakes.first().copied(), Some(1));
        assert_eq!(wakes.last().copied(), Some(JOINT_ROUNDS));
    }

    #[test]
    fn injection_core_all_rejected() {
        let (ok, l) = run_injection_core();
        for x in ok.iter() {
            assert!(*x);
        }
        assert!(l.conserved());
        assert_eq!(l.injected, BAD_CLASSES);
    }

    #[test]
    fn errno_table_agreement() {
        assert!(agrees_with_linux(EAGAIN));
        assert!(agrees_with_linux(-EINVAL));
        assert!(!agrees_with_linux(0));
        assert!(!agrees_with_linux(9876));
    }

    #[test]
    fn double_run_fingerprint() {
        let (_, e1) = run_a3_c4_jointest();
        let (_, e2) = run_a3_c4_jointest();
        assert_eq!(joint_fingerprint(&e1), joint_fingerprint(&e2));
    }

    #[test]
    fn codes_closed_set() {
        assert_eq!(PrimCode::ALL.len(), 11);
        for c in PrimCode::ALL.iter() {
            assert!(!c.detail().is_empty());
            assert!(c.val() > SEG_BASE && c.val() <= SEG_BASE + 11);
        }
    }

    #[test]
    fn unx9389_checks_green() {
        let set = run_unx9389_checks();
        assert!(set.all_passed(), "UNX-F9389 red: {}", set.red_items_text());
    }
}
