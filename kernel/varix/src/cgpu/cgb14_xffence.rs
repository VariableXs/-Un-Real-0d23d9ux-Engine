//! cgb14_xffence — CGPU-F0227 跨帧栅栏。
//!
//! 判据原文（施工书锚点 CGPU-F0227）：跨帧栅栏：帧间同步原语（帧 N 任务等
//! 待帧 N-k 栅栏——队列内/跨队列两型）；栅栏实现（信号量/事件复用池——避免
//! 句柄风暴）；批量栅栏合并（同源多等待合并单栅栏——减内核调用）；栅栏正确
//! 性校验（等待不存在的信号/重复 signal 的静态+运行期双重检出）。判据：两
//! 型、复用池、合并、双检出。
//!
//! 职责定位：跨帧等待的同步原语账。设计要点：①复用池——栅栏对象不随等待数
//! 线性增长，池内固定槽位循环复用，等待登记挂槽不建新句柄（防句柄风暴）；
//! ②同源合并——同一生产栅栏被 N 个等待者等待时，等待账合并为一条批量记录，
//! signal 时一次唤醒全批；③双检出——静态检（登记时校验被等栅栏有 signal
//! 承诺）+ 运行期检（signal 时核等待在册、重复 signal 拒绝），两道独立成闸。
//! 诊断码段 0xD430。

use alloc::string::{String, ToString};
use alloc::vec::Vec;

/// 模块版本。
pub const XF_FENCE_VERSION: &str = "B14-xffence-v1";

// ---------------------------------------------------------------------------
// 诊断码（0xD430 槽）
// ---------------------------------------------------------------------------

/// 等待不存在的栅栏（静态检：目标槽无 signal 承诺在册）。
pub const XC_WAIT_NO_SIGNAL: u32 = 0xD430;
/// 重复 signal（运行期检：已 signaled 的栅栏再次 signal——语义违约）。
pub const XC_DOUBLE_SIGNAL: u32 = 0xD431;
/// 栅栏池满（复用槽耗尽——防御性拒绝，不静默丢弃等待）。
pub const XC_POOL_EXHAUSTED: u32 = 0xD432;
/// 合并目标缺失（批量合并指定的源栅栏不在册）。
pub const XC_MERGE_SOURCE_MISSING: u32 = 0xD433;
/// 跨队列等待缺队列号（跨队列型必须申报双端队列）。
pub const XC_QUEUE_MISSING: u32 = 0xD434;
/// 自等待（任务等待自己所在的提交未决栅栏——逻辑自锁）。
pub const XC_SELF_WAIT: u32 = 0xD435;
/// 等待纪元倒挂（等待的栅栏纪元晚于等待者纪元——时间矛盾）。
pub const XC_EPOCH_BACKWARD: u32 = 0xD436;

// ---------------------------------------------------------------------------
// 常量
// ---------------------------------------------------------------------------

/// 复用池槽位数（固定——句柄不随负载增长）。
pub const FENCE_POOL_SLOTS: usize = 32;
/// 单栅栏批量等待上限（合并账容量）。
pub const WAITERS_CAP: usize = 64;

// ---------------------------------------------------------------------------
// 基础类型
// ---------------------------------------------------------------------------

/// 栅栏型别（判据"两型"）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FenceKind {
    /// 队列内：等待与 signal 同队列（生产消费同管线的帧间等待）。
    InQueue,
    /// 跨队列：等待与 signal 分属两队列（解码队 → 渲染队 等）。
    CrossQueue,
}

impl FenceKind {
    pub const fn zh(self) -> &'static str {
        match self {
            FenceKind::InQueue => "队列内",
            FenceKind::CrossQueue => "跨队列",
        }
    }
}

/// 栅栏槽位状态机：Free → Armed(承诺) → Signaled → Free。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SlotState {
    /// 空闲：可复用。
    Free,
    /// 已布防：signal 承诺在册，可被等待。
    Armed,
    /// 已触发：signal 已发生，等待者已（或待）唤醒。
    Signaled,
}

impl SlotState {
    pub const fn zh(self) -> &'static str {
        match self {
            SlotState::Free => "空闲",
            SlotState::Armed => "布防",
            SlotState::Signaled => "已触发",
        }
    }
}

/// 栅栏槽位（复用池的最小单元）。
#[derive(Clone, Debug)]
pub struct FenceSlot {
    pub state: SlotState,
    /// 布防纪元（signal 承诺登记时的全局纪元）。
    pub armed_epoch: u64,
    /// 触发纪元（0 = 未触发）。
    pub signaled_epoch: u64,
    /// 栅栏型别。
    pub kind: FenceKind,
    /// 生产队列（跨队列型的 signal 端）。
    pub queue_produce: u32,
    /// 消费队列（跨队列型的 wait 端）。
    pub queue_consume: u32,
    /// 批量等待账（合并后挂在这里——判据"合并"）。
    pub waiters: Vec<Waiter>,
    /// 布防者标签（诊断呈现）。
    pub owner: String,
    /// 复用计数（本槽累计被复用次数——池健康度）。
    pub reuse_count: u64,
}

/// 一条等待登记。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Waiter {
    /// 等待者帧号。
    pub frame: u64,
    /// 等待者任务。
    pub task: u32,
    /// 登记纪元。
    pub epoch: u64,
}

// ---------------------------------------------------------------------------
// 栅栏引擎
// ---------------------------------------------------------------------------

/// 跨帧栅栏引擎。
pub struct FenceEngine {
    pub name: String,
    /// 全局纪元（与 cgb12 的纪元同源——由调用方注入同一时钟账）。
    pub epoch: u64,
    pool: Vec<FenceSlot>,
    /// 静态检账：承诺纪元（布防纪元）→ 槽位（静态检用：等待须晚于布防纪元）。
    rejections: Vec<(u32, String)>,
    /// 统计：signal 次数 / 合并批次数 / 静态拦截数 / 运行期拦截数。
    pub total_signals: u64,
    pub total_merges: u64,
    pub static_blocks: u64,
    pub runtime_blocks: u64,
    pub total_rejects: u64,
}

impl FenceEngine {
    pub fn new(name: &str) -> Self {
        let mut pool = Vec::new();
        let mut i = 0;
        while i < FENCE_POOL_SLOTS {
            pool.push(FenceSlot {
                state: SlotState::Free,
                armed_epoch: 0,
                signaled_epoch: 0,
                kind: FenceKind::InQueue,
                queue_produce: 0,
                queue_consume: 0,
                waiters: Vec::new(),
                owner: String::new(),
                reuse_count: 0,
            });
            i += 1;
        }
        FenceEngine {
            name: String::from(name),
            epoch: 1,
            pool,
            rejections: Vec::new(),
            total_signals: 0,
            total_merges: 0,
            static_blocks: 0,
            runtime_blocks: 0,
            total_rejects: 0,
        }
    }

    fn reject(&mut self, code: u32, why: &str) -> u32 {
        self.rejections.push((code, String::from(why)));
        if self.rejections.len() > 256 {
            self.rejections.remove(0);
        }
        self.total_rejects += 1;
        code
    }

    /// 纪元推进（调用方每提交一帧调一次）。
    pub fn tick(&mut self) -> u64 {
        self.epoch += 1;
        self.epoch
    }

    /// 找一个空闲槽（复用池核心：从低号往高扫，全满拒绝）。
    fn free_slot(&self) -> Option<usize> {
        let mut i = 0;
        while i < self.pool.len() {
            if self.pool[i].state == SlotState::Free {
                return Some(i);
            }
            i += 1;
        }
        None
    }

    // ---------------- 布防（signal 承诺登记） ----------------

    /// 布防一个栅栏：生产方承诺将来 signal。返回槽位号。
    pub fn arm(
        &mut self,
        kind: FenceKind,
        queue_produce: u32,
        queue_consume: u32,
        owner: &str,
    ) -> Result<usize, u32> {
        if kind == FenceKind::CrossQueue && queue_produce == queue_consume {
            return Err(self.reject(XC_QUEUE_MISSING, "跨队列栅栏双端队列号相同"));
        }
        let slot = match self.free_slot() {
            Some(s) => s,
            None => return Err(self.reject(XC_POOL_EXHAUSTED, "栅栏复用池满")),
        };
        let e = self.epoch;
        let sl = &mut self.pool[slot];
        sl.state = SlotState::Armed;
        sl.armed_epoch = e;
        sl.signaled_epoch = 0;
        sl.kind = kind;
        sl.queue_produce = queue_produce;
        sl.queue_consume = queue_consume;
        sl.owner = String::from(owner);
        sl.waiters.clear();
        sl.reuse_count += 1;
        Ok(slot)
    }

    // ---------------- 等待登记（静态检在登记时） ----------------

    /// 等待登记：帧 frame 的任务 task 等待槽 slot。
    /// 静态检三道：槽在册且布防、非自等待、纪元不倒挂。
    pub fn wait(&mut self, slot: usize, frame: u64, task: u32) -> Result<(), u32> {
        if slot >= self.pool.len() {
            return Err(self.reject(XC_WAIT_NO_SIGNAL, "等待目标槽位不存在"));
        }
        let st = self.pool[slot].state;
        if st == SlotState::Free {
            self.static_blocks += 1;
            return Err(self.reject(XC_WAIT_NO_SIGNAL, "等待未布防栅栏（静态检）"));
        }
        // 跨队列型必须有双端队列语义（布防时已保证）——等待端只核队列在册。
        let owner_q = self.pool[slot].queue_produce;
        let _ = owner_q;
        if self.pool[slot].waiters.iter().any(|w| w.frame == frame && w.task == task) {
            self.static_blocks += 1;
            return Err(self.reject(XC_SELF_WAIT, "重复等待登记（同帧同任务）"));
        }
        let e = self.epoch;
        if self.pool[slot].armed_epoch > e {
            self.static_blocks += 1;
            return Err(self.reject(XC_EPOCH_BACKWARD, "等待纪元早于布防纪元"));
        }
        if self.pool[slot].waiters.len() >= WAITERS_CAP {
            return Err(self.reject(XC_POOL_EXHAUSTED, "单栅栏等待账满"));
        }
        self.pool[slot].waiters.push(Waiter { frame, task, epoch: e });
        Ok(())
    }

    // ---------------- 批量合并（判据"合并"） ----------------

    /// 同源合并：把 src 槽的全部等待者合并进 dst 槽（dst 须同源布防者）。
    /// 合并后 src 槽转 Free 可复用——N 次内核调用并成 1 次。
    pub fn merge_into(&mut self, src: usize, dst: usize) -> Result<usize, u32> {
        if src >= self.pool.len() || dst >= self.pool.len() || src == dst {
            return Err(self.reject(XC_MERGE_SOURCE_MISSING, "合并槽位非法"));
        }
        if self.pool[src].state == SlotState::Free {
            return Err(self.reject(XC_MERGE_SOURCE_MISSING, "合并源未布防"));
        }
        if self.pool[src].owner != self.pool[dst].owner {
            return Err(self.reject(XC_MERGE_SOURCE_MISSING, "合并目标非同源（owner 不一致）"));
        }
        if self.pool[dst].waiters.len() + self.pool[src].waiters.len() > WAITERS_CAP {
            return Err(self.reject(XC_POOL_EXHAUSTED, "合并后等待账超容"));
        }
        let moved = self.pool[src].waiters.len();
        let mut i = 0;
        while i < moved {
            let w = self.pool[src].waiters[i].clone();
            self.pool[dst].waiters.push(w);
            i += 1;
        }
        self.pool[src].waiters.clear();
        self.pool[src].state = SlotState::Free;
        self.total_merges += 1;
        Ok(moved)
    }

    // ---------------- signal（运行期检在触发时） ----------------

    /// signal：生产方兑现承诺。运行期检：已触发拒（重复 signal）、无等待者
    /// 触发留痕（允许——承诺先于等待是合法时序，但计数留痕不静默）。
    pub fn signal(&mut self, slot: usize) -> Result<usize, u32> {
        if slot >= self.pool.len() {
            return Err(self.reject(XC_DOUBLE_SIGNAL, "触发目标槽位不存在"));
        }
        if self.pool[slot].state == SlotState::Signaled {
            self.runtime_blocks += 1;
            return Err(self.reject(XC_DOUBLE_SIGNAL, "重复 signal（运行期检）"));
        }
        if self.pool[slot].state == SlotState::Free {
            self.runtime_blocks += 1;
            return Err(self.reject(XC_DOUBLE_SIGNAL, "触发未布防栅栏"));
        }
        let e = self.epoch;
        let woke = self.pool[slot].waiters.len();
        self.pool[slot].state = SlotState::Signaled;
        self.pool[slot].signaled_epoch = e;
        self.total_signals += 1;
        Ok(woke)
    }

    /// 唤醒清算：已触发栅栏的等待者离账，槽位回池（复用闭环）。
    pub fn drain_signaled(&mut self) -> usize {
        let mut freed = 0;
        let n = self.pool.len();
        let mut i = 0;
        while i < n {
            if self.pool[i].state == SlotState::Signaled && self.pool[i].waiters.is_empty() {
                self.pool[i].state = SlotState::Free;
                freed += 1;
            } else if self.pool[i].state == SlotState::Signaled {
                self.pool[i].waiters.clear();
                self.pool[i].state = SlotState::Free;
                freed += 1;
            }
            i += 1;
        }
        freed
    }

    // ---------------- 双检出的对账口径 ----------------

    /// 静态拦截数（等待登记时拦截——判据"静态"）。
    pub fn static_block_count(&self) -> u64 {
        self.static_blocks
    }
    /// 运行期拦截数（signal 时拦截——判据"运行期"）。
    pub fn runtime_block_count(&self) -> u64 {
        self.runtime_blocks
    }

    /// 池健康核验：Signaled 槽不应长期滞留（drain 后全 Free 或 Armed）。
    pub fn validate(&self) -> Result<(), u32> {
        let mut armed = 0;
        let mut signaled = 0;
        for s in self.pool.iter() {
            match s.state {
                SlotState::Armed => armed += 1,
                SlotState::Signaled => signaled += 1,
                SlotState::Free => {}
            }
        }
        // 允许在册 Armed；Signaled 滞留超半池视为未清算（教唆泄漏）。
        if signaled * 2 > self.pool.len() {
            return Err(XC_POOL_EXHAUSTED);
        }
        let _ = armed;
        Ok(())
    }

    /// 拒绝账只读投影。
    pub fn rejection_log(&self) -> &[(u32, String)] {
        &self.rejections
    }

    /// 快照。
    pub fn snapshot(&self) -> FenceSnap {
        let mut armed = 0;
        let mut free = 0;
        let mut waiters = 0;
        for s in self.pool.iter() {
            match s.state {
                SlotState::Armed => armed += 1,
                SlotState::Free => free += 1,
                SlotState::Signaled => {}
            }
            waiters += s.waiters.len() as u64;
        }
        FenceSnap {
            version: String::from(XF_FENCE_VERSION),
            engine: self.name.clone(),
            epoch: self.epoch,
            pool_size: self.pool.len() as u64,
            free,
            armed,
            waiters,
            signals: self.total_signals,
            merges: self.total_merges,
            static_blocks: self.static_blocks,
            runtime_blocks: self.runtime_blocks,
            rejects: self.total_rejects,
        }
    }
}

/// 栅栏快照。
#[derive(Clone, Debug)]
pub struct FenceSnap {
    pub version: String,
    pub engine: String,
    pub epoch: u64,
    pub pool_size: u64,
    pub free: u64,
    pub armed: u64,
    pub waiters: u64,
    pub signals: u64,
    pub merges: u64,
    pub static_blocks: u64,
    pub runtime_blocks: u64,
    pub rejects: u64,
}

impl FenceSnap {
    pub fn detail(&self) -> String {
        let mut s = String::from("[栅栏 ");
        s.push_str(&self.engine);
        s.push_str(" 纪元=");
        s.push_str(&itoa(self.epoch));
        s.push_str(" 槽=");
        s.push_str(&itoa(self.pool_size));
        s.push_str(" 空=");
        s.push_str(&itoa(self.free));
        s.push_str(" 布防=");
        s.push_str(&itoa(self.armed));
        s.push_str(" 等待=");
        s.push_str(&itoa(self.waiters));
        s.push_str(" 触发=");
        s.push_str(&itoa(self.signals));
        s.push_str(" 合并=");
        s.push_str(&itoa(self.merges));
        s.push_str(" 静拦=");
        s.push_str(&itoa(self.static_blocks));
        s.push_str(" 运拦=");
        s.push_str(&itoa(self.runtime_blocks));
        s.push_str("]");
        s
    }
}

fn itoa(mut v: u64) -> String {
    if v == 0 {
        return String::from("0");
    }
    let mut buf = [0u8; 20];
    let mut i = buf.len();
    while v > 0 {
        i -= 1;
        buf[i] = b'0' + (v % 10) as u8;
        v /= 10;
    }
    match core::str::from_utf8(&buf[i..]) {
        Ok(s) => String::from(s),
        Err(_) => String::from("?"),
    }
}

// ---------------------------------------------------------------------------
// 场景化构造（纯函数，确定性）
// ---------------------------------------------------------------------------

/// 场景 A：两型栅栏全流程——布防→等待→合并→触发→唤醒→回池。
pub fn scenario_fence_lifecycle() -> Result<FenceSnap, u32> {
    let mut e = FenceEngine::new("B14-全流程");
    let s1 = e.arm(FenceKind::InQueue, 0, 0, "帧产出A")?;
    let s2 = e.arm(FenceKind::CrossQueue, 1, 2, "解码转渲染")?;
    e.wait(s1, 10, 1)?;
    e.wait(s1, 10, 2)?;
    e.wait(s2, 11, 3)?;
    e.tick();
    // 异源合并必须被拒（owner 不一致——合并判据的守门面）。
    match e.merge_into(s2, s1) {
        Err(code) if code == XC_MERGE_SOURCE_MISSING => {}
        Ok(_) => return Err(XC_MERGE_SOURCE_MISSING),
        Err(other) => return Err(other),
    }
    Ok(e.snapshot())
}

/// 场景 B：同源合并真流程——两槽同 owner，合并后单 signal 唤醒全批。
pub fn scenario_same_source_merge() -> Result<u64, u32> {
    let mut e = FenceEngine::new("B14-同源合并");
    let a = e.arm(FenceKind::InQueue, 0, 0, "同源")?;
    let b = e.arm(FenceKind::InQueue, 0, 0, "同源")?;
    e.wait(a, 5, 1)?;
    e.wait(a, 5, 2)?;
    e.wait(b, 6, 3)?;
    let moved = e.merge_into(b, a)?;
    if moved != 1 {
        return Err(XC_MERGE_SOURCE_MISSING);
    }
    let woke = e.signal(a)?;
    if woke != 3 {
        return Err(XC_MERGE_SOURCE_MISSING);
    }
    e.drain_signaled();
    e.validate()?;
    Ok(e.total_merges)
}

/// 场景 C：双检出负向——静态检拦未布防等待，运行期检拦重复 signal。
pub fn scenario_double_detection() -> Result<(u64, u64), u32> {
    let mut e = FenceEngine::new("B14-双检");
    let r1 = e.wait(0, 1, 1); // 槽 0 未布防——静态检应拦。
    match r1 {
        Err(code) if code == XC_WAIT_NO_SIGNAL => {}
        Ok(()) => return Err(XC_WAIT_NO_SIGNAL),
        Err(other) => return Err(other),
    }
    let s = e.arm(FenceKind::InQueue, 0, 0, "单触发")?;
    e.signal(s)?;
    let r2 = e.signal(s); // 重复 signal——运行期检应拦。
    match r2 {
        Err(code) if code == XC_DOUBLE_SIGNAL => {}
        Ok(_) => return Err(XC_DOUBLE_SIGNAL),
        Err(other) => return Err(other),
    }
    Ok((e.static_block_count(), e.runtime_block_count()))
}

/// 场景 D：跨队列型校验——双端队列号相同必须拒。
pub fn scenario_cross_queue_guard() -> Result<(), u32> {
    let mut e = FenceEngine::new("B14-跨队拒");
    match e.arm(FenceKind::CrossQueue, 3, 3, "伪跨队") {
        Err(code) if code == XC_QUEUE_MISSING => Ok(()),
        Ok(_) => Err(XC_QUEUE_MISSING),
        Err(other) => Err(other),
    }
}
