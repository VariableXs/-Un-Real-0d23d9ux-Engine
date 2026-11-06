//! UNX-F50125 热插拔事件账（接入/拔出任务保全语义）（0x43bc0 段）
//!
//! 锚点：UNX-F50125-J1 拔出 ×10 任务状态归位零半空；判据锚定 体验三 状态机。
//!
//! 热拔出的危险不是「设备不见了」，而是「任务还挂在设备上」：一个 Active 任务
//! 指向一条已经不存在的管道，既不会完成也不会失败，队列里就留下一个永远不被
//! 回收的空壳——本模块把这种状态叫**半空**（half-empty），并要求 `audit()`
//! 在任何一次操作之后都报 0。
//!
//! 归位规则（任务保全）：
//! * Active / Quiescing / Parked → `Cancelled`，并把已写字节搬到 `salvaged`
//!   （保全：重插后 `requeue` 可从断点继续，进度不丢）；
//! * Queued → `Cancelled`，`salvaged` 沿用该任务已有进度（requeue 过的任务
//!   仍保有上一轮的断点）；
//! * Done 不动——已经完成的任务不因拔出而回退；
//! * 设备侧状态只有 Present / Absent 两态，拔出后立即归 Absent，不允许停在
//!   「正在移除」这种没人推进的中间态。
//!
//! 数据结构：
//! * `DeviceKey` / `DevSlot` —— 设备表（去重、代数 generation 递增）；
//! * `Task` / `TaskState` —— 任务表（7 态机：Queued/Active/Quiescing/Parked/
//!   Done/Cancelled + 显式非法转移拒绝）；
//! * `HotLedger` —— 设备表 + 任务表 + 热插拔事件流水 + 计数；
//! * `Audit` —— 半空/悬空扫描结果（J1 的 0 值就是验收线）。
//!
//! 错误闭集：`SEG_BASE + n`（0x43bc0 段），所有码值都被真实构造。

use crate::alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x43bc0 独占 0x20）。
pub const SEG_BASE: u32 = 0x43bc0;

pub const E_EMPTY: u16 = 1;
pub const E_NO_DEVICE: u16 = 2;
pub const E_DUP_DEVICE: u16 = 3;
pub const E_NO_TASK: u16 = 4;
pub const E_BAD_STATE: u16 = 5;
pub const E_NOT_CANCELLABLE: u16 = 6;
pub const E_CAPACITY: u16 = 7;
pub const E_EVENT_FULL: u16 = 8;

/// 热插拔错误闭集。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HotError {
    Empty,
    NoDevice,
    DupDevice,
    NoTask,
    BadState,
    NotCancellable,
    Capacity,
    EventFull,
}

impl HotError {
    pub fn code(self) -> u32 {
        let n = match self {
            HotError::Empty => E_EMPTY,
            HotError::NoDevice => E_NO_DEVICE,
            HotError::DupDevice => E_DUP_DEVICE,
            HotError::NoTask => E_NO_TASK,
            HotError::BadState => E_BAD_STATE,
            HotError::NotCancellable => E_NOT_CANCELLABLE,
            HotError::Capacity => E_CAPACITY,
            HotError::EventFull => E_EVENT_FULL,
        };
        SEG_BASE + n as u32
    }

    pub fn message(self) -> &'static str {
        match self {
            HotError::Empty => "device key all-zero",
            HotError::NoDevice => "device not present",
            HotError::DupDevice => "device already present",
            HotError::NoTask => "task id unknown",
            HotError::BadState => "task transition illegal in current state",
            HotError::NotCancellable => "requeue requires a cancelled task",
            HotError::Capacity => "device/task table full",
            HotError::EventFull => "hotplug event log full",
        }
    }
}

/// 设备键。serial 用 32 位摘要即可——同一 vid:pid 下串号只用于区分同型号多台。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DeviceKey {
    pub vid: u16,
    pub pid: u16,
    pub serial: u32,
}

impl DeviceKey {
    pub const fn new(vid: u16, pid: u16, serial: u32) -> DeviceKey {
        DeviceKey { vid, pid, serial }
    }

    /// 全零键不是合法设备（对应「没有枚举到任何东西」）。
    pub fn valid(&self) -> bool {
        self.vid != 0 || self.pid != 0
    }

    pub fn digest(&self) -> u64 {
        let mut h: u64 = 0xcbf2_9ce4_8422_2325;
        h = fnv_u16(h, self.vid);
        h = fnv_u16(h, self.pid);
        fnv_u32(h, self.serial)
    }
}

/// 设备在表里的状态。只有两态：在/不在。没有「移除中」——拔出是同步完成的，
/// 允许外部观察到「移除中」就等于允许半空存在。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DevState {
    Present,
    Absent,
}

impl DevState {
    pub fn label(self) -> &'static str {
        match self {
            DevState::Present => "present",
            DevState::Absent => "absent",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DevSlot {
    pub key: DeviceKey,
    pub state: DevState,
    /// 每接入一次 +1，用于区分「同一台设备的第几次接入」。
    pub generation: u16,
}

/// 任务状态机。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TaskState {
    Queued,
    Active,
    Quiescing,
    Parked,
    Done,
    Cancelled,
}

impl TaskState {
    /// 终态：不会再被拔出改写，也不需要 requeue。
    pub fn is_terminal(self) -> bool {
        matches!(self, TaskState::Done | TaskState::Cancelled)
    }

    pub fn label(self) -> &'static str {
        match self {
            TaskState::Queued => "queued",
            TaskState::Active => "active",
            TaskState::Quiescing => "quiescing",
            TaskState::Parked => "parked",
            TaskState::Done => "done",
            TaskState::Cancelled => "cancelled",
        }
    }

    pub fn code(self) -> u8 {
        match self {
            TaskState::Queued => 1,
            TaskState::Active => 2,
            TaskState::Quiescing => 3,
            TaskState::Parked => 4,
            TaskState::Done => 5,
            TaskState::Cancelled => 6,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Task {
    pub id: u32,
    pub dev: DeviceKey,
    pub state: TaskState,
    /// 已写字节（跨 requeue 保留）。
    pub written: u32,
    /// 拔出时保全的断点（仅 Cancelled 有意义）。
    pub salvaged: u32,
}

/// 热插拔事件流水。每一条成功改变设备/任务归属的操作都必须留痕——
/// 没有留痕的拔出等于「设备消失但没人知道是热拔出」。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EvRec {
    pub seq: u32,
    pub kind: u8,
    pub dev: DeviceKey,
    pub task: u32,
}

pub const EV_ATTACH: u8 = 1;
pub const EV_DETACH: u8 = 2;
pub const EV_RESET: u8 = 3;
pub const EV_REQUEUE: u8 = 4;

impl EvRec {
    pub fn kind_label(self) -> &'static str {
        match self.kind {
            EV_ATTACH => "attach",
            EV_DETACH => "detach",
            EV_RESET => "reset",
            EV_REQUEUE => "requeue",
            _ => "unknown",
        }
    }
}

/// 审计结果。`half` 是 J1 的核心指标，`dangling` 是更严重的「设备键根本不在表里」。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Audit {
    pub tasks: u16,
    pub present_devices: u16,
    pub half: u16,
    pub dangling: u16,
    pub active: u16,
    pub cancelled: u16,
    pub done: u16,
}

impl Audit {
    pub fn clean(&self) -> bool {
        self.half == 0 && self.dangling == 0
    }
}

/// 一轮拔出的结果（判据直接读这些字段，避免重新推导）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RoundReport {
    pub round: u8,
    pub tasks: u16,
    pub resolved: u16,
    pub done_kept: u16,
    pub salvage_before: u32,
    pub salvage_after: u32,
    pub half: u16,
    pub dangling: u16,
    pub generation: u16,
}

/// 热插拔账本：设备表 + 任务表 + 事件流水，全部定容。
pub struct HotLedger {
    devices: Vec<DevSlot>,
    tasks: Vec<Task>,
    events: Vec<EvRec>,
    dev_cap: usize,
    task_cap: usize,
    ev_cap: usize,
    attaches: u16,
    detaches: u16,
    resets: u16,
    requeues: u16,
    next_task: u32,
}

impl HotLedger {
    /// 建账。三个容量都必须 ≥1 且不超过 64（热插拔是低频事件，64 足够覆盖
    /// 一次介质插拔风暴；更大的表应交给上层注册表，不在这里堆）。
    pub fn new(dev_cap: usize, task_cap: usize, ev_cap: usize) -> Result<HotLedger, HotError> {
        if dev_cap == 0 || dev_cap > 64 || task_cap == 0 || task_cap > 64 || ev_cap == 0 || ev_cap > 64 {
            return Err(HotError::Capacity);
        }
        Ok(HotLedger {
            devices: Vec::with_capacity(dev_cap),
            tasks: Vec::with_capacity(task_cap),
            events: Vec::with_capacity(ev_cap),
            dev_cap,
            task_cap,
            ev_cap,
            attaches: 0,
            detaches: 0,
            resets: 0,
            requeues: 0,
            next_task: 1,
        })
    }

    pub fn device_len(&self) -> usize {
        self.devices.len()
    }

    pub fn task_len(&self) -> usize {
        self.tasks.len()
    }

    pub fn event_len(&self) -> usize {
        self.events.len()
    }

    pub fn attaches(&self) -> u16 {
        self.attaches
    }

    pub fn detaches(&self) -> u16 {
        self.detaches
    }

    pub fn resets(&self) -> u16 {
        self.resets
    }

    pub fn requeues(&self) -> u16 {
        self.requeues
    }

    pub fn device(&self, key: DeviceKey) -> Option<&DevSlot> {
        let i = self.dev_index(key)?;
        self.devices.get(i)
    }

    pub fn task(&self, id: u32) -> Option<&Task> {
        let i = self.task_index(id)?;
        self.tasks.get(i)
    }

    /// 按表内序取任务（判据遍历用；越界返回 None）。
    pub fn task_at(&self, i: usize) -> Option<&Task> {
        self.tasks.get(i)
    }

    /// 按流水序取事件（越界返回 None）。
    pub fn event_at(&self, i: usize) -> Option<&EvRec> {
        self.events.get(i)
    }

    /// 故障注入入口：只把设备标 Absent，**不归位任务**。
    ///
    /// 存在的意义是让「半空可检」这条判据有真实坏样本可抓——正常路径永远
    /// 走 `detach`，这里专门模拟「拔出路径漏了归位」的错误实现。
    pub fn force_absent(&mut self, key: DeviceKey) -> bool {
        let i = match self.dev_index(key) {
            Some(v) => v,
            None => return false,
        };
        if let Some(slot) = self.devices.get_mut(i) {
            slot.state = DevState::Absent;
            return true;
        }
        false
    }

    /// 事件流水摘要（逐条含 seq/kind/dev/task）。
    pub fn event_digest(&self) -> u64 {
        let mut h: u64 = 0xcbf2_9ce4_8422_2325 ^ self.events.len() as u64;
        let mut i = 0usize;
        while i < self.events.len() {
            if let Some(e) = self.events.get(i) {
                h = fnv_u32(h, e.seq);
                h = fnv_u8(h, e.kind);
                h ^= e.dev.digest();
                h = h.wrapping_mul(0x100_0000_01b3);
                h = fnv_u32(h, e.task);
            }
            i += 1;
        }
        h
    }

    fn dev_index(&self, key: DeviceKey) -> Option<usize> {
        let mut i = 0usize;
        while i < self.devices.len() {
            if let Some(s) = self.devices.get(i) {
                if s.key == key {
                    return Some(i);
                }
            }
            i += 1;
        }
        None
    }

    fn task_index(&self, id: u32) -> Option<usize> {
        let mut i = 0usize;
        while i < self.tasks.len() {
            if let Some(t) = self.tasks.get(i) {
                if t.id == id {
                    return Some(i);
                }
            }
            i += 1;
        }
        None
    }

    /// 追加事件。容量满时**先失败再改状态**：没有留痕的操作不执行。
    fn push_event(&mut self, kind: u8, dev: DeviceKey, task: u32) -> Result<u32, HotError> {
        if self.events.len() >= self.ev_cap {
            return Err(HotError::EventFull);
        }
        let seq = self.events.len() as u32;
        self.events.push(EvRec { seq, kind, dev, task });
        Ok(seq)
    }

    /// 接入。已在表里且 Present → 重复接入；曾经拔出（Absent）→ 代数 +1 复活。
    pub fn attach(&mut self, key: DeviceKey) -> Result<u16, HotError> {
        if !key.valid() {
            return Err(HotError::Empty);
        }
        let mut idx: Option<usize> = None;
        let mut present = false;
        let mut gen = 0u16;
        let mut i = 0usize;
        while i < self.devices.len() {
            if let Some(s) = self.devices.get(i) {
                if s.key == key {
                    idx = Some(i);
                    present = s.state == DevState::Present;
                    gen = s.generation;
                    break;
                }
            }
            i += 1;
        }
        if present {
            return Err(HotError::DupDevice);
        }
        match idx {
            Some(at) => {
                self.push_event(EV_ATTACH, key, 0)?;
                if let Some(slot) = self.devices.get_mut(at) {
                    slot.state = DevState::Present;
                    slot.generation = gen.saturating_add(1);
                    gen = slot.generation;
                }
            }
            None => {
                if self.devices.len() >= self.dev_cap {
                    return Err(HotError::Capacity);
                }
                self.push_event(EV_ATTACH, key, 0)?;
                gen = 1;
                self.devices.push(DevSlot { key, state: DevState::Present, generation: 1 });
            }
        }
        self.attaches = self.attaches.saturating_add(1);
        Ok(gen)
    }

    /// 开一个任务。设备必须 Present——给不存在的设备挂任务正是「半空」的源头，
    /// 必须在入口拦住。
    pub fn task_open(&mut self, key: DeviceKey, written: u32) -> Result<u32, HotError> {
        if !key.valid() {
            return Err(HotError::Empty);
        }
        let i = match self.dev_index(key) {
            Some(v) => v,
            None => return Err(HotError::NoDevice),
        };
        let mut present = false;
        if let Some(s) = self.devices.get(i) {
            present = s.state == DevState::Present;
        }
        if !present {
            return Err(HotError::NoDevice);
        }
        if self.tasks.len() >= self.task_cap {
            return Err(HotError::Capacity);
        }
        let id = self.next_task;
        self.next_task = self.next_task.saturating_add(1);
        self.tasks.push(Task {
            id,
            dev: key,
            state: TaskState::Active,
            written,
            salvaged: 0,
        });
        Ok(id)
    }

    /// 写进度。只有 Active/Quiescing 能推进；Done/Cancelled 上的写入是
    /// 生命周期错误，不能默默累加。
    pub fn task_progress(&mut self, id: u32, bytes: u32) -> Result<u32, HotError> {
        let i = match self.task_index(id) {
            Some(v) => v,
            None => return Err(HotError::NoTask),
        };
        let mut ok = false;
        if let Some(t) = self.tasks.get(i) {
            ok = matches!(t.state, TaskState::Active | TaskState::Quiescing);
        }
        if !ok {
            return Err(HotError::BadState);
        }
        let mut out = 0u32;
        if let Some(t) = self.tasks.get_mut(i) {
            t.written = t.written.saturating_add(bytes);
            out = t.written;
        }
        Ok(out)
    }

    /// Active|Quiescing → Parked（保留进度，等设备恢复）。
    pub fn task_park(&mut self, id: u32) -> Result<(), HotError> {
        self.transition(id, &[TaskState::Active, TaskState::Quiescing], TaskState::Parked)
    }

    /// Active → Quiescing。设备已拔但数据还在路上时进入静默，随后由 detach 取消。
    pub fn task_quiesce(&mut self, id: u32) -> Result<(), HotError> {
        self.transition(id, &[TaskState::Active], TaskState::Quiescing)
    }

    /// Active → Done。任务完成不搬运 salvaged（它从未被取消）。
    pub fn task_complete(&mut self, id: u32) -> Result<(), HotError> {
        self.transition(id, &[TaskState::Active], TaskState::Done)
    }

    /// Queued|Parked → Active（恢复执行/重插后启动）。
    pub fn task_start(&mut self, id: u32) -> Result<(), HotError> {
        self.transition(id, &[TaskState::Queued, TaskState::Parked], TaskState::Active)
    }

    fn transition(&mut self, id: u32, from: &[TaskState], to: TaskState) -> Result<(), HotError> {
        let i = match self.task_index(id) {
            Some(v) => v,
            None => return Err(HotError::NoTask),
        };
        let mut allow = false;
        if let Some(t) = self.tasks.get(i) {
            let mut k = 0usize;
            while k < from.len() {
                if let Some(f) = from.get(k) {
                    if t.state == *f {
                        allow = true;
                    }
                }
                k += 1;
            }
        }
        if !allow {
            return Err(HotError::BadState);
        }
        if let Some(t) = self.tasks.get_mut(i) {
            t.state = to;
        }
        Ok(())
    }

    /// 拔出。所有非终态任务当场归位为 Cancelled 并保全断点，Done 保持不动。
    /// 返回被归位的任务数。设备同一时刻转 Absent——不存在「半拔出」。
    pub fn detach(&mut self, key: DeviceKey) -> Result<u16, HotError> {
        if !key.valid() {
            return Err(HotError::Empty);
        }
        let idx = match self.dev_index(key) {
            Some(v) => v,
            None => return Err(HotError::NoDevice),
        };
        let mut present = false;
        if let Some(s) = self.devices.get(idx) {
            present = s.state == DevState::Present;
        }
        if !present {
            return Err(HotError::NoDevice);
        }
        self.push_event(EV_DETACH, key, 0)?;
        let mut resolved = 0u16;
        let mut i = 0usize;
        while i < self.tasks.len() {
            let mut act: Option<(TaskState, u32)> = None;
            if let Some(t) = self.tasks.get(i) {
                if t.dev == key {
                    act = Some((t.state, t.written));
                }
            }
            if let Some((st, written)) = act {
                if !st.is_terminal() {
                    if let Some(t) = self.tasks.get_mut(i) {
                        t.state = TaskState::Cancelled;
                        t.salvaged = written;
                    }
                    resolved = resolved.saturating_add(1);
                }
            }
            i += 1;
        }
        if let Some(slot) = self.devices.get_mut(idx) {
            slot.state = DevState::Absent;
        }
        self.detaches = self.detaches.saturating_add(1);
        Ok(resolved)
    }

    /// 设备复位（非拔出）：代数 +1，在途任务转 Parked 等重新调度。
    /// 返回转入 Parked 的任务数。
    pub fn reset(&mut self, key: DeviceKey) -> Result<u16, HotError> {
        if !key.valid() {
            return Err(HotError::Empty);
        }
        let idx = match self.dev_index(key) {
            Some(v) => v,
            None => return Err(HotError::NoDevice),
        };
        let mut present = false;
        if let Some(s) = self.devices.get(idx) {
            present = s.state == DevState::Present;
        }
        if !present {
            return Err(HotError::NoDevice);
        }
        self.push_event(EV_RESET, key, 0)?;
        if let Some(slot) = self.devices.get_mut(idx) {
            slot.generation = slot.generation.saturating_add(1);
        }
        let mut moved = 0u16;
        let mut i = 0usize;
        while i < self.tasks.len() {
            let mut act: Option<(TaskState, u32)> = None;
            if let Some(t) = self.tasks.get(i) {
                if t.dev == key {
                    act = Some((t.state, t.written));
                }
            }
            if let Some((st, written)) = act {
                if matches!(st, TaskState::Active | TaskState::Quiescing) {
                    if let Some(t) = self.tasks.get_mut(i) {
                        t.state = TaskState::Parked;
                        t.salvaged = written;
                    }
                    moved = moved.saturating_add(1);
                }
            }
            i += 1;
        }
        self.resets = self.resets.saturating_add(1);
        Ok(moved)
    }

    /// 重插后把已取消的任务放回队列。必须：任务已 Cancelled、设备已重新 Present。
    /// 进度（written/salvaged）原样保留，这就是「任务保全」的兑现。
    pub fn requeue(&mut self, id: u32) -> Result<u32, HotError> {
        let i = match self.task_index(id) {
            Some(v) => v,
            None => return Err(HotError::NoTask),
        };
        let mut st = TaskState::Queued;
        let mut dev = DeviceKey::new(0, 0, 0);
        if let Some(t) = self.tasks.get(i) {
            st = t.state;
            dev = t.dev;
        }
        if st != TaskState::Cancelled {
            return Err(HotError::NotCancellable);
        }
        let di = match self.dev_index(dev) {
            Some(v) => v,
            None => return Err(HotError::NoDevice),
        };
        let mut present = false;
        if let Some(s) = self.devices.get(di) {
            present = s.state == DevState::Present;
        }
        if !present {
            return Err(HotError::NoDevice);
        }
        self.push_event(EV_REQUEUE, dev, id)?;
        let mut written = 0u32;
        if let Some(t) = self.tasks.get_mut(i) {
            t.state = TaskState::Queued;
            written = t.written;
        }
        self.requeues = self.requeues.saturating_add(1);
        Ok(written)
    }

    /// 半空/悬空扫描。`half` 只统计「设备 Absent 但任务非终态」，
    /// `dangling` 统计「设备键完全不在表里」。两者都必须是 0。
    pub fn audit(&self) -> Audit {
        let mut tasks = 0u16;
        let mut active = 0u16;
        let mut cancelled = 0u16;
        let mut done = 0u16;
        let mut half = 0u16;
        let mut dangling = 0u16;
        let mut present_devices = 0u16;
        let mut i = 0usize;
        while i < self.devices.len() {
            if let Some(s) = self.devices.get(i) {
                if s.state == DevState::Present {
                    present_devices = present_devices.saturating_add(1);
                }
            }
            i += 1;
        }
        let mut k = 0usize;
        while k < self.tasks.len() {
            if let Some(t) = self.tasks.get(k) {
                tasks = tasks.saturating_add(1);
                match t.state {
                    TaskState::Active | TaskState::Quiescing => active = active.saturating_add(1),
                    TaskState::Cancelled => cancelled = cancelled.saturating_add(1),
                    TaskState::Done => done = done.saturating_add(1),
                    _ => {}
                }
                let mut state: Option<DevState> = None;
                let mut j = 0usize;
                while j < self.devices.len() {
                    if let Some(s) = self.devices.get(j) {
                        if s.key == t.dev {
                            state = Some(s.state);
                            break;
                        }
                    }
                    j += 1;
                }
                match state {
                    None => {
                        dangling = dangling.saturating_add(1);
                        if !t.state.is_terminal() {
                            half = half.saturating_add(1);
                        }
                    }
                    Some(DevState::Absent) => {
                        if !t.state.is_terminal() {
                            half = half.saturating_add(1);
                        }
                    }
                    Some(DevState::Present) => {}
                }
            }
            k += 1;
        }
        Audit { tasks, present_devices, half, dangling, active, cancelled, done }
    }

    /// 第 i 轮拔出演练（i < 10）。
    ///
    /// 每轮：接入 → 开 2~4 个任务（分别落在 Active/Parked/Quiescing/Done 四种
    /// 起始形态）→ 拔出 → 审计。J1 的「×10」就是这 10 轮。
    pub fn round(&mut self, i: usize) -> Result<RoundReport, HotError> {
        if i >= 10 {
            return Err(HotError::BadState);
        }
        let key = DeviceKey::new(0x0400 + i as u16, 0x5000 + i as u16, i as u32 + 1);
        let generation = self.attach(key)?;
        let n = 2 + (i % 3);
        let mut ids: Vec<u32> = Vec::with_capacity(n);
        let mut t = 0usize;
        while t < n {
            let progress = 100 + (t as u32) * 50 + i as u32;
            let id = self.task_open(key, progress)?;
            t += 1;
            // 形态分布：0=Active、1=Parked/Quiescing 交替、2=Done、3=Parked。
            match t {
                2 => {
                    if i % 2 == 0 {
                        self.task_park(id)?;
                    } else {
                        self.task_quiesce(id)?;
                    }
                }
                3 => {
                    self.task_complete(id)?;
                }
                4 => {
                    self.task_park(id)?;
                }
                _ => {}
            }
            ids.push(id);
        }
        // 拔出前的断点总量：所有非终态任务的已写字节。
        let mut salvage_before = 0u32;
        let mut done_kept = 0u16;
        let mut a = 0usize;
        while a < ids.len() {
            if let Some(id) = ids.get(a) {
                if let Some(task) = self.task(*id) {
                    if task.state == TaskState::Done {
                        done_kept = done_kept.saturating_add(1);
                    } else {
                        salvage_before = salvage_before.saturating_add(task.written);
                    }
                }
            }
            a += 1;
        }
        let resolved = self.detach(key)?;
        let mut salvage_after = 0u32;
        let mut b = 0usize;
        while b < ids.len() {
            if let Some(id) = ids.get(b) {
                if let Some(task) = self.task(*id) {
                    salvage_after = salvage_after.saturating_add(task.salvaged);
                }
            }
            b += 1;
        }
        let audit = self.audit();
        Ok(RoundReport {
            round: i as u8,
            tasks: n as u16,
            resolved,
            done_kept,
            salvage_before,
            salvage_after,
            half: audit.half,
            dangling: audit.dangling,
            generation,
        })
    }
}

fn fnv_u8(mut h: u64, b: u8) -> u64 {
    h ^= b as u64;
    h.wrapping_mul(0x100_0000_01b3)
}

fn fnv_u16(h: u64, v: u16) -> u64 {
    fnv_u8(fnv_u8(h, (v & 0xff) as u8), (v >> 8) as u8)
}

fn fnv_u32(h: u64, v: u32) -> u64 {
    fnv_u16(fnv_u16(h, (v & 0xffff) as u16), (v >> 16) as u16)
}

// ---------------------------------------------------------------------------
// 注入核：八类坏热插拔操作必须全部被拒
// ---------------------------------------------------------------------------

pub fn injection_core() -> (usize, usize) {
    let mut caught = 0usize;
    let total = 8usize;
    let mut l = match HotLedger::new(2, 4, 8) {
        Ok(v) => v,
        Err(_) => return (0, total),
    };
    let k1 = DeviceKey::new(0x0403, 0x6001, 7);
    let k2 = DeviceKey::new(0x04b8, 0x0005, 8);

    // 1 全零设备键
    if l.attach(DeviceKey::new(0, 0, 0)).err() == Some(HotError::Empty) {
        caught += 1;
    }
    // 2 拔出未接入的设备
    if l.detach(k1).err() == Some(HotError::NoDevice) {
        caught += 1;
    }
    // 3 同一设备重复接入
    if l.attach(k1).is_ok() && l.attach(k1).err() == Some(HotError::DupDevice) {
        caught += 1;
    }
    // 4 给 Absent 设备挂任务
    if l.detach(k1).is_ok() && l.task_open(k1, 10).err() == Some(HotError::NoDevice) {
        caught += 1;
    }
    // 5 未知任务号
    if l.task_progress(999, 1).err() == Some(HotError::NoTask) {
        caught += 1;
    }
    // 6 非法状态转移：对已取消任务调用 park
    if l.attach(k1).is_ok() {
        let id = l.task_open(k1, 5);
        if let Ok(tid) = id {
            if l.detach(k1).is_ok() && l.task_park(tid).err() == Some(HotError::BadState) {
                caught += 1;
            }
        }
    }
    // 7 requeue 非 Cancelled 任务
    if l.attach(k1).is_ok() {
        let id = l.task_open(k1, 5);
        if let Ok(tid) = id {
            if l.requeue(tid).err() == Some(HotError::NotCancellable) {
                caught += 1;
            }
        }
    }
    // 8 设备表满（容量 2，已有 k1，加入 k2 后第三台被拒）
    if l.attach(k2).is_ok()
        && l.attach(DeviceKey::new(0x0500, 0x0001, 9)).err() == Some(HotError::Capacity)
    {
        caught += 1;
    }

    (caught, total)
}

// ---------------------------------------------------------------------------
// 判据
// ---------------------------------------------------------------------------

pub fn run_unx50125_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F50125");

    let mut l = match HotLedger::new(12, 40, 64) {
        Ok(v) => v,
        Err(_) => return s,
    };

    // J1-1 拔出 ×10：每轮任务全部归位，审计 half/dangling 全 0
    let mut reports: Vec<RoundReport> = Vec::with_capacity(10);
    let mut clean_rounds = 0usize;
    let mut i = 0usize;
    while i < 10 {
        match l.round(i) {
            Ok(r) => {
                if r.half == 0 && r.dangling == 0 && r.resolved + r.done_kept == r.tasks {
                    clean_rounds += 1;
                }
                reports.push(r);
            }
            Err(_) => {}
        }
        i += 1;
    }
    let ok = clean_rounds == 10;
    let det = format!("clean={}/10", clean_rounds);
    s.add_dyn("50125-J1-拔出10归位", ok, &det);

    // J1-2 审计守恒：任务总数 = 各轮任务数之和，活动任务 0、取消数 = 总归位数
    let audit = l.audit();
    let mut total_tasks = 0u16;
    let mut total_resolved = 0u16;
    let mut total_done = 0u16;
    let mut k = 0usize;
    while k < reports.len() {
        if let Some(r) = reports.get(k) {
            total_tasks = total_tasks.saturating_add(r.tasks);
            total_resolved = total_resolved.saturating_add(r.resolved);
            total_done = total_done.saturating_add(r.done_kept);
        }
        k += 1;
    }
    let ok = audit.tasks == total_tasks
        && audit.cancelled == total_resolved
        && audit.done == total_done
        && audit.active == 0
        && audit.half == 0
        && audit.dangling == 0;
    let det = format!(
        "tasks={} cancel={} done={} active={}",
        audit.tasks, audit.cancelled, audit.done, audit.active
    );
    s.add_dyn("50125-J1-状态守恒", ok, &det);

    // J1-3 断点保全：每轮 salvage_before == salvage_after（进度不丢）
    let mut salvage_ok = true;
    let mut total_salvage = 0u32;
    let mut j = 0usize;
    while j < reports.len() {
        if let Some(r) = reports.get(j) {
            if r.salvage_before != r.salvage_after {
                salvage_ok = false;
            }
            total_salvage = total_salvage.saturating_add(r.salvage_after);
        }
        j += 1;
    }
    let ok = salvage_ok && total_salvage > 0;
    let det = format!("kept={}", total_salvage);
    s.add_dyn("50125-J1-断点保全", ok, &det);

    // J1-4 拔出后设备侧全部 Absent、接入/拔出计数平衡
    let ok = audit.present_devices == 0
        && l.attaches() == 10
        && l.detaches() == 10
        && l.task_len() == total_tasks as usize;
    s.add("50125-J1-计数平衡", ok, if ok { "" } else { "接入/拔出/任务计数不平" });

    // J1-5 事件流水：每轮 attach+detach 各一条，共 20 条，序号从 0 严格连续
    let mut seq_ok = true;
    let mut prev: Option<u32> = None;
    let mut e = 0usize;
    while e < l.event_len() {
        if let Some(ev) = l.event_at(e) {
            match prev {
                None => {
                    if ev.seq != 0 {
                        seq_ok = false;
                    }
                }
                Some(p) => {
                    if ev.seq != p + 1 {
                        seq_ok = false;
                    }
                }
            }
            prev = Some(ev.seq);
        }
        e += 1;
    }
    let ok = l.event_len() == 20 && seq_ok && prev == Some(19);
    let det = format!("events={} seq={:?}", l.event_len(), prev);
    s.add_dyn("50125-J1-事件单调", ok, &det);

    // J1-6 重插保全：attach → requeue 归 Queued、进度保留、可继续写
    let key = DeviceKey::new(0x0700, 0x0001, 42);
    let mut r = match HotLedger::new(4, 8, 16) {
        Ok(v) => v,
        Err(_) => return s,
    };
    let ok_attach = r.attach(key);
    let id = match r.task_open(key, 300) {
        Ok(v) => v,
        Err(_) => return s,
    };
    let progress = r.task_progress(id, 50);
    let resolved = r.detach(key);
    let re = r.attach(key);
    let requeued = r.requeue(id);
    let started = r.task_start(id);
    let resumed = r.task_progress(id, 25);
    let ok = ok_attach.is_ok()
        && progress == Ok(350)
        && resolved == Ok(1)
        && re.is_ok()
        && requeued == Ok(350)
        && started.is_ok()
        && resumed == Ok(375)
        && r.requeues() == 1
        && r.audit().half == 0;
    let det = format!("requeue={:?} resume={:?}", requeued, resumed);
    s.add_dyn("50125-J1-重插续跑", ok, &det);

    // J1-7 requeue 的前置条件：未 Cancelled 拒绝；设备未重插拒绝
    let mut q = match HotLedger::new(4, 8, 16) {
        Ok(v) => v,
        Err(_) => return s,
    };
    let kq = DeviceKey::new(0x0701, 0x0002, 1);
    let _ = q.attach(kq);
    let tq = match q.task_open(kq, 10) {
        Ok(v) => v,
        Err(_) => return s,
    };
    let not_cancelled = q.requeue(tq);
    let _ = q.detach(kq);
    let absent = q.requeue(tq);
    let unknown = q.requeue(9999);
    let ok = not_cancelled.err() == Some(HotError::NotCancellable)
        && absent.err() == Some(HotError::NoDevice)
        && unknown.err() == Some(HotError::NoTask)
        && q.requeues() == 0;
    s.add("50125-J1-重插前置", ok, if ok { "" } else { "requeue 前置条件失守" });

    // J1-8 复位语义：在途任务转 Parked（非取消），设备仍在，审计零半空
    let mut z = match HotLedger::new(4, 8, 16) {
        Ok(v) => v,
        Err(_) => return s,
    };
    let kz = DeviceKey::new(0x0702, 0x0003, 1);
    let _ = z.attach(kz);
    let z1 = match z.task_open(kz, 11) {
        Ok(v) => v,
        Err(_) => return s,
    };
    let z2 = match z.task_open(kz, 22) {
        Ok(v) => v,
        Err(_) => return s,
    };
    let _ = z.task_quiesce(z2);
    let moved = z.reset(kz);
    let mut parked = 0u16;
    let mut zi = 0usize;
    while zi < z.task_len() {
        if let Some(t) = z.task_at(zi) {
            if t.id == z1 || t.id == z2 {
                if t.state == TaskState::Parked {
                    parked += 1;
                }
            }
        }
        zi += 1;
    }
    let ok = moved == Ok(2)
        && parked == 2
        && z.resets() == 1
        && z.audit().present_devices == 1
        && z.audit().half == 0
        && z.audit().active == 0;
    s.add("50125-J1-复位停车", ok, if ok { "" } else { "reset 未把在途任务停住" });

    // J1-9 非法转移：Done 不可 park/start，Done 上的写被拒
    let mut w = match HotLedger::new(4, 8, 16) {
        Ok(v) => v,
        Err(_) => return s,
    };
    let kw = DeviceKey::new(0x0703, 0x0004, 1);
    let _ = w.attach(kw);
    let tw = match w.task_open(kw, 7) {
        Ok(v) => v,
        Err(_) => return s,
    };
    let completed = w.task_complete(tw);
    let park_done = w.task_park(tw);
    let start_done = w.task_start(tw);
    let write_done = w.task_progress(tw, 1);
    let complete_again = w.task_complete(tw);
    let ok = completed.is_ok()
        && park_done.err() == Some(HotError::BadState)
        && start_done.err() == Some(HotError::BadState)
        && write_done.err() == Some(HotError::BadState)
        && complete_again.err() == Some(HotError::BadState);
    s.add("50125-J1-终态封口", ok, if ok { "" } else { "终态仍可转移" });

    // J1-10 容量：设备表/任务表/事件表三处满都显式拒绝且不改状态
    let mut c = match HotLedger::new(1, 1, 2) {
        Ok(v) => v,
        Err(_) => return s,
    };
    let kc = DeviceKey::new(0x0704, 0x0005, 1);
    let _ = c.attach(kc); // 事件 1
    let t1 = c.task_open(kc, 1);
    let t2 = c.task_open(kc, 2);
    let d1 = c.detach(kc); // 事件 2，归位 t1
    let ev_attach = c.attach(kc); // 事件表已满 → 拒绝，设备仍 Absent
    let dev_cap = c.attach(DeviceKey::new(0x0705, 0x0006, 2));
    let still_absent = c.audit().present_devices == 0;
    let ok = t1.is_ok()
        && t2.err() == Some(HotError::Capacity)
        && d1 == Ok(1)
        && ev_attach.err() == Some(HotError::EventFull)
        && dev_cap.err() == Some(HotError::Capacity)
        && still_absent
        && c.attaches() == 1
        && c.detaches() == 1
        && c.audit().half == 0
        && HotLedger::new(0, 1, 1).err() == Some(HotError::Capacity)
        && HotLedger::new(65, 1, 1).err() == Some(HotError::Capacity);
    let det = format!("ev_attach={:?}", ev_attach.err());
    s.add_dyn("50125-J1-容量边界", ok, &det);

    // J1-11 拔出不存在/全零键：NoDevice / Empty，且账本不动
    let before = l.event_digest();
    let bogus = l.detach(DeviceKey::new(0x0fff, 0x0fff, 9));
    let zero = l.detach(DeviceKey::new(0, 0, 0));
    let ok = bogus.err() == Some(HotError::NoDevice)
        && zero.err() == Some(HotError::Empty)
        && l.event_digest() == before;
    s.add("50125-J1-无设备拒", ok, if ok { "" } else { "拔出不存在设备留下了痕迹" });

    // J1-12 确定性：同一轮脚本两次执行报告一致
    let mut d1 = match HotLedger::new(4, 8, 16) {
        Ok(v) => v,
        Err(_) => return s,
    };
    let mut d2 = match HotLedger::new(4, 8, 16) {
        Ok(v) => v,
        Err(_) => return s,
    };
    let ra = d1.round(3);
    let rb = d2.round(3);
    let ok = ra == rb && d1.event_digest() == d2.event_digest() && ra.is_ok();
    s.add("50125-J1-确定性", ok, if ok { "" } else { "同脚本报告漂移" });

    // J1-13 审计口径：手工构造半空（任务挂在已拔设备上）必须被抓
    let mut h = match HotLedger::new(4, 8, 16) {
        Ok(v) => v,
        Err(_) => return s,
    };
    let kh = DeviceKey::new(0x0706, 0x0007, 3);
    let _ = h.attach(kh);
    let th = match h.task_open(kh, 9) {
        Ok(v) => v,
        Err(_) => return s,
    };
    let clean_before = h.audit().clean();
    // 绕过 detach 直接把设备标 Absent，模拟「拔出路径漏归位」的坏实现。
    h.force_absent(kh);
    let bad = h.audit();
    let ok = clean_before
        && bad.half == 1
        && bad.dangling == 0
        && bad.active == 1
        && h.task(th).map(|t| t.state == TaskState::Active) == Some(true);
    let det = format!("half={} active={}", bad.half, bad.active);
    s.add_dyn("50125-J1-半空可检", ok, &det);

    // J1-14 注入核：八类坏操作全抓
    let (caught, total) = injection_core();
    let ok = caught == total;
    let det = format!("注入 {}/{}", caught, total);
    s.add_dyn("50125-J1-注入必抓", ok, &det);

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx50125_checks_green() {
        let set = run_unx50125_checks();
        assert!(set.all_passed(), "UNX-F50125 red: {}", set.red_items_text());
    }

    #[test]
    fn unx50125_ten_rounds_zero_half() {
        let l = HotLedger::new(12, 40, 64);
        assert!(l.is_ok());
        if let Ok(mut l) = l {
            let mut i = 0usize;
            while i < 10 {
                let r = l.round(i);
                assert!(r.is_ok());
                if let Ok(r) = r {
                    assert_eq!(r.half, 0);
                    assert_eq!(r.dangling, 0);
                    assert_eq!(r.resolved + r.done_kept, r.tasks);
                    assert_eq!(r.salvage_before, r.salvage_after);
                }
                i += 1;
            }
            assert_eq!(l.audit().half, 0);
        }
    }

    #[test]
    fn unx50125_error_codes_in_segment() {
        assert_eq!(HotError::Empty.code(), SEG_BASE + 1);
        assert_eq!(HotError::EventFull.code(), SEG_BASE + 8);
    }
}
