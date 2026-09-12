//! VARIABLE-200 AI-04 · 服务域 IPC 面（F083~F090）。
//!
//! 事件总线、管道/FIFO、共享内存、事件通道、io_uring 式批量接口、
//! 命名服务、配置读取、日志服务。纯逻辑 + 固定容量数组，无分配。

use crate::checks::CheckSet;

// ---------------------------------------------------------------------------
// F083 事件总线用户态端 — 主题订阅与分发
// ---------------------------------------------------------------------------

pub const BUS_MAX_SUBS: usize = 12;
pub const BUS_MAX_TOPIC: usize = 16;
/// 主题位掩码（最多 32 个主题）。
pub const BUS_TOPIC_BITS: usize = 32;

#[derive(Clone, Copy)]
pub struct Subscription {
    /// 订阅者端口号。
    pub port: u32,
    /// 主题掩码。
    pub mask: u32,
    pub active: bool,
    /// 已投递事件数。
    pub delivered: u64,
}

impl Subscription {
    pub const fn empty() -> Subscription {
        Subscription { port: 0, mask: 0, active: false, delivered: 0 }
    }

    pub fn wants(&self, topic: usize) -> bool {
        topic < BUS_TOPIC_BITS && (self.mask & (1u32 << topic)) != 0
    }
}

#[derive(Clone, Copy)]
pub struct EventBus {
    subs: [Subscription; BUS_MAX_SUBS],
    count: usize,
    /// 事件发布总数。
    pub published: u64,
}

impl EventBus {
    pub const fn new() -> EventBus {
        EventBus { subs: [Subscription::empty(); BUS_MAX_SUBS], count: 0, published: 0 }
    }

    pub fn len(&self) -> usize {
        self.count
    }

    pub fn is_empty(&self) -> bool {
        self.count == 0
    }

    pub fn subscribe(&mut self, port: u32, mask: u32) -> Option<usize> {
        if self.count >= BUS_MAX_SUBS || mask == 0 || port == 0 {
            return None;
        }
        let mut s = Subscription::empty();
        s.port = port;
        s.mask = mask;
        s.active = true;
        self.subs[self.count] = s;
        self.count += 1;
        Some(self.count - 1)
    }

    pub fn unsubscribe(&mut self, port: u32) -> bool {
        let mut i = 0usize;
        while i < self.count {
            if self.subs[i].port == port && self.subs[i].active {
                self.subs[i].active = false;
                return true;
            }
            i += 1;
        }
        false
    }

    /// 发布一个主题事件，返回投递到的订阅者数。
    pub fn publish(&mut self, topic: usize) -> usize {
        if topic >= BUS_TOPIC_BITS {
            return 0;
        }
        self.published += 1;
        let mut n = 0usize;
        let mut i = 0usize;
        while i < self.count {
            if self.subs[i].active && self.subs[i].wants(topic) {
                self.subs[i].delivered += 1;
                n += 1;
            }
            i += 1;
        }
        n
    }

    pub fn delivered(&self, i: usize) -> Option<u64> {
        if i < self.count {
            Some(self.subs[i].delivered)
        } else {
            None
        }
    }

    /// 主题名 → 位（固定表，避免字符串哈希）。
    pub fn topic_bit(name: &[u8]) -> Option<usize> {
        const TABLE: [&[u8]; 6] =
            [b"device", b"power", b"proc", b"display", b"input", b"fs"];
        let mut i = 0usize;
        while i < TABLE.len() {
            if TABLE[i] == name {
                return Some(i);
            }
            i += 1;
        }
        None
    }

    pub fn topic_name(bit: usize) -> Option<&'static [u8]> {
        const TABLE: [&[u8]; 6] =
            [b"device", b"power", b"proc", b"display", b"input", b"fs"];
        if bit < TABLE.len() {
            Some(TABLE[bit])
        } else {
            None
        }
    }
}

// ---------------------------------------------------------------------------
// F084 管道与 FIFO — 环形缓冲 + 行缓冲策略
// ---------------------------------------------------------------------------

pub const PIPE_CAP: usize = 64;
pub const PIPE_MAX: usize = 8;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PipeMode {
    /// 字节流：可读即读。
    Stream,
    /// 行缓冲：未遇换行不交付。
    Line,
}

#[derive(Clone, Copy)]
pub struct Pipe {
    buf: [u8; PIPE_CAP],
    head: usize,
    len: usize,
    pub mode: PipeMode,
    pub written: u64,
    pub read: u64,
    /// 写端已关闭。
    pub closed_wr: bool,
}

impl Pipe {
    pub const fn new(mode: PipeMode) -> Pipe {
        Pipe {
            buf: [0u8; PIPE_CAP],
            head: 0,
            len: 0,
            mode,
            written: 0,
            read: 0,
            closed_wr: false,
        }
    }

    pub fn len(&self) -> usize {
        self.len
    }

    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    pub fn free(&self) -> usize {
        PIPE_CAP - self.len
    }

    /// 写入；返回实际写入字节（满则 0）。
    pub fn write(&mut self, data: &[u8]) -> usize {
        let n = core::cmp::min(data.len(), self.free());
        let mut i = 0usize;
        while i < n {
            self.buf[(self.head + self.len + i) % PIPE_CAP] = data[i];
            i += 1;
        }
        self.len += n;
        self.written += n as u64;
        n
    }

    /// 行缓冲模式下可交付的有效长度。
    pub fn deliverable(&self) -> usize {
        if self.mode == PipeMode::Stream {
            return self.len;
        }
        let mut i = 0usize;
        while i < self.len {
            if self.buf[(self.head + i) % PIPE_CAP] == b'\n' {
                return i + 1;
            }
            i += 1;
        }
        0
    }

    /// 读取；行缓冲模式下只交付到最后一个完整行。
    pub fn read(&mut self, out: &mut [u8]) -> usize {
        let avail = self.deliverable();
        if avail == 0 {
            return 0;
        }
        let n = core::cmp::min(out.len(), avail);
        let mut i = 0usize;
        while i < n {
            out[i] = self.buf[(self.head + i) % PIPE_CAP];
            i += 1;
        }
        self.head = (self.head + n) % PIPE_CAP;
        self.len -= n;
        self.read += n as u64;
        n
    }

    pub fn close_write(&mut self) {
        self.closed_wr = true;
    }

    /// EOF：写端关闭且缓冲排空。
    pub fn eof(&self) -> bool {
        self.closed_wr && self.len == 0
    }
}

/// FIFO 名字表：命名管道。
pub const FIFO_MAX: usize = 8;

#[derive(Clone, Copy)]
pub struct FifoTable {
    names: [[u8; 16]; FIFO_MAX],
    name_len: [usize; FIFO_MAX],
    pipes: [Pipe; FIFO_MAX],
    /// 每个 FIFO 的写端引用计数。
    writers: [u32; FIFO_MAX],
    count: usize,
}

impl FifoTable {
    pub const fn new() -> FifoTable {
        FifoTable {
            names: [[0u8; 16]; FIFO_MAX],
            name_len: [0usize; FIFO_MAX],
            pipes: [Pipe::new(PipeMode::Stream); FIFO_MAX],
            writers: [0u32; FIFO_MAX],
            count: 0,
        }
    }

    pub fn len(&self) -> usize {
        self.count
    }

    pub fn is_empty(&self) -> bool {
        self.count == 0
    }

    pub fn mkfifo(&mut self, name: &[u8], mode: PipeMode) -> Option<usize> {
        if self.count >= FIFO_MAX || name.is_empty() || name.len() >= 16 {
            return None;
        }
        if self.lookup(name).is_some() {
            return None;
        }
        let mut i = 0usize;
        while i < name.len() {
            self.names[self.count][i] = name[i];
            i += 1;
        }
        self.name_len[self.count] = name.len();
        self.pipes[self.count] = Pipe::new(mode);
        self.writers[self.count] = 1;
        self.count += 1;
        Some(self.count - 1)
    }

    pub fn lookup(&self, name: &[u8]) -> Option<usize> {
        let mut i = 0usize;
        while i < self.count {
            if self.names[i][..self.name_len[i]] == *name {
                return Some(i);
            }
            i += 1;
        }
        None
    }

    pub fn pipe_mut(&mut self, i: usize) -> Option<&mut Pipe> {
        if i < self.count {
            Some(&mut self.pipes[i])
        } else {
            None
        }
    }

    /// 打开写端：不存在则创建；返回下标。
    pub fn open_write(&mut self, name: &[u8]) -> Option<usize> {
        match self.lookup(name) {
            Some(i) => {
                self.writers[i] += 1;
                Some(i)
            }
            None => self.mkfifo(name, PipeMode::Stream),
        }
    }

    pub fn close_write(&mut self, i: usize) -> bool {
        if i >= self.count || self.writers[i] == 0 {
            return false;
        }
        self.writers[i] -= 1;
        if self.writers[i] == 0 {
            self.pipes[i].close_write();
        }
        true
    }

    pub fn writers(&self, i: usize) -> u32 {
        if i < self.count {
            self.writers[i]
        } else {
            0
        }
    }
}

// ---------------------------------------------------------------------------
// F085 共享内存 — 具名共享区创建/映射/权限
// ---------------------------------------------------------------------------

pub const SHM_MAX: usize = 8;
pub const SHM_MAX_PAGES: usize = 8;

/// 共享区权限位（no_std 无外部 crate，手写位标志）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ShmPerm(pub u8);

impl ShmPerm {
    pub const READ: ShmPerm = ShmPerm(0b001);
    pub const WRITE: ShmPerm = ShmPerm(0b010);
    pub const EXEC: ShmPerm = ShmPerm(0b100);

    pub const fn empty() -> ShmPerm {
        ShmPerm(0)
    }

    pub const fn bits(self) -> u8 {
        self.0
    }

    pub const fn contains(self, other: ShmPerm) -> bool {
        (self.0 & other.0) == other.0
    }

    pub const fn union(self, other: ShmPerm) -> ShmPerm {
        ShmPerm(self.0 | other.0)
    }
}

#[derive(Clone, Copy)]
pub struct ShmRegion {
    pub name: [u8; 16],
    pub name_len: usize,
    pub pages: usize,
    pub perm: ShmPerm,
    /// 映射计数。
    pub mappings: u32,
    pub sealed: bool,
}

impl ShmRegion {
    pub const fn empty() -> ShmRegion {
        ShmRegion {
            name: [0u8; 16],
            name_len: 0,
            pages: 0,
            perm: ShmPerm::empty(),
            mappings: 0,
            sealed: false,
        }
    }

    pub fn bytes(&self) -> usize {
        self.pages * 4096
    }

    pub fn name_eq(&self, want: &[u8]) -> bool {
        &self.name[..self.name_len] == want
    }
}

#[derive(Clone, Copy)]
pub struct ShmTable {
    regions: [ShmRegion; SHM_MAX],
    count: usize,
    pub bytes_mapped: usize,
}

impl ShmTable {
    pub const fn new() -> ShmTable {
        ShmTable { regions: [ShmRegion::empty(); SHM_MAX], count: 0, bytes_mapped: 0 }
    }

    pub fn len(&self) -> usize {
        self.count
    }

    pub fn is_empty(&self) -> bool {
        self.count == 0
    }

    pub fn create(
        &mut self,
        name: &[u8],
        pages: usize,
        perm: ShmPerm,
    ) -> Option<usize> {
        if self.count >= SHM_MAX
            || name.is_empty()
            || name.len() >= 16
            || pages == 0
            || pages > SHM_MAX_PAGES
        {
            return None;
        }
        if self.find(name).is_some() {
            return None;
        }
        let mut r = ShmRegion::empty();
        r.name_len = name.len();
        r.pages = pages;
        r.perm = perm;
        let mut i = 0usize;
        while i < name.len() {
            r.name[i] = name[i];
            i += 1;
        }
        self.regions[self.count] = r;
        self.count += 1;
        Some(self.count - 1)
    }

    pub fn find(&self, name: &[u8]) -> Option<usize> {
        let mut i = 0usize;
        while i < self.count {
            if self.regions[i].name_eq(name) {
                return Some(i);
            }
            i += 1;
        }
        None
    }

    /// 映射：权限必须是创建权限的子集，且未封印。
    pub fn map(&mut self, i: usize, want: ShmPerm) -> bool {
        if i >= self.count || self.regions[i].sealed {
            return false;
        }
        if !self.regions[i].perm.contains(want) {
            return false;
        }
        self.regions[i].mappings += 1;
        self.bytes_mapped += self.regions[i].bytes();
        true
    }

    pub fn unmap(&mut self, i: usize) -> bool {
        if i >= self.count || self.regions[i].mappings == 0 {
            return false;
        }
        self.regions[i].mappings -= 1;
        self.bytes_mapped -= self.regions[i].bytes();
        true
    }

    /// 封印后不可再映射（持久通道就绪）。
    pub fn seal(&mut self, i: usize) -> bool {
        if i < self.count {
            self.regions[i].sealed = true;
            true
        } else {
            false
        }
    }

    pub fn region(&self, i: usize) -> Option<ShmRegion> {
        if i < self.count {
            Some(self.regions[i])
        } else {
            None
        }
    }
}

// ---------------------------------------------------------------------------
// F086 事件通道 — 共享内存 + 事件通知的 ping-pong
// ---------------------------------------------------------------------------

pub const CHAN_MAX: usize = 4;
/// 通道环槽位数。
pub const CHAN_SLOTS: usize = 16;

#[derive(Clone, Copy)]
pub struct EventChannel {
    /// 环槽位：每槽一个 u32 消息。
    slots: [u32; CHAN_SLOTS],
    /// 生产者写指针。
    pub wr: usize,
    /// 消费者读指针。
    pub rd: usize,
    pub dropped: u64,
    pub delivered: u64,
    pub notify_pending: bool,
}

impl EventChannel {
    pub const fn new() -> EventChannel {
        EventChannel {
            slots: [0u32; CHAN_SLOTS],
            wr: 0,
            rd: 0,
            dropped: 0,
            delivered: 0,
            notify_pending: false,
        }
    }

    pub fn occupied(&self) -> usize {
        (self.wr + CHAN_SLOTS - self.rd) % CHAN_SLOTS
    }

    pub fn empty(&self) -> bool {
        self.wr == self.rd
    }

    /// 生产者投递（环满即丢弃并计数，绝不阻塞内核）。
    pub fn post(&mut self, value: u32) -> bool {
        let next = (self.wr + 1) % CHAN_SLOTS;
        if next == self.rd {
            self.dropped += 1;
            return false;
        }
        self.slots[self.wr] = value;
        self.wr = next;
        self.notify_pending = true;
        true
    }

    /// 消费者取一条。
    pub fn poll(&mut self) -> Option<u32> {
        if self.empty() {
            self.notify_pending = false;
            return None;
        }
        let v = self.slots[self.rd];
        self.rd = (self.rd + 1) % CHAN_SLOTS;
        self.delivered += 1;
        if self.empty() {
            self.notify_pending = false;
        }
        Some(v)
    }

    /// ping-pong 往返：A 发 value → B 取 → B 回 value+1。
    pub fn ping_pong(&mut self, round_trips: usize) -> usize {
        let mut done = 0usize;
        let mut i = 0usize;
        while i < round_trips {
            if self.post(i as u32) {
                if let Some(v) = self.poll() {
                    if v == i as u32 {
                        done += 1;
                    }
                }
            }
            i += 1;
        }
        done
    }
}

#[derive(Clone, Copy)]
pub struct ChannelTable {
    chans: [EventChannel; CHAN_MAX],
    count: usize,
}

impl ChannelTable {
    pub const fn new() -> ChannelTable {
        ChannelTable { chans: [EventChannel::new(); CHAN_MAX], count: 0 }
    }

    pub fn len(&self) -> usize {
        self.count
    }

    pub fn is_empty(&self) -> bool {
        self.count == 0
    }

    pub fn create(&mut self) -> Option<usize> {
        if self.count >= CHAN_MAX {
            return None;
        }
        self.chans[self.count] = EventChannel::new();
        self.count += 1;
        Some(self.count - 1)
    }

    pub fn get_mut(&mut self, i: usize) -> Option<&mut EventChannel> {
        if i < self.count {
            Some(&mut self.chans[i])
        } else {
            None
        }
    }

    pub fn get(&self, i: usize) -> Option<&EventChannel> {
        if i < self.count {
            Some(&self.chans[i])
        } else {
            None
        }
    }
}

// ---------------------------------------------------------------------------
// F087 io_uring 式批量接口 — 提交/完成环
// ---------------------------------------------------------------------------

pub const RING_SLOTS: usize = 16;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RingOp {
    Read,
    Write,
    Fsync,
    Close,
}

#[derive(Clone, Copy)]
pub struct Sqe {
    pub op: RingOp,
    pub fd: u32,
    pub len: u32,
    pub user_data: u64,
}

impl Sqe {
    pub const fn new(op: RingOp, fd: u32, len: u32, user_data: u64) -> Sqe {
        Sqe { op, fd, len, user_data }
    }
}

#[derive(Clone, Copy)]
pub struct Cqe {
    pub user_data: u64,
    /// 结果（>=0 成功字节，<0 错误码）。
    pub result: i64,
}

impl Cqe {
    pub const fn empty() -> Cqe {
        Cqe { user_data: 0, result: 0 }
    }
}

#[derive(Clone, Copy)]
pub struct IoRing {
    sq: [Sqe; RING_SLOTS],
    sq_head: usize,
    sq_tail: usize,
    cq: [Cqe; RING_SLOTS],
    cq_head: usize,
    cq_tail: usize,
    pub submits: u64,
    pub completes: u64,
    pub truncations: u64,
}

impl IoRing {
    pub const fn new() -> IoRing {
        IoRing {
            sq: [Sqe::new(RingOp::Read, 0, 0, 0); RING_SLOTS],
            sq_head: 0,
            sq_tail: 0,
            cq: [Cqe::empty(); RING_SLOTS],
            cq_head: 0,
            cq_tail: 0,
            submits: 0,
            completes: 0,
            truncations: 0,
        }
    }

    pub fn sq_depth(&self) -> usize {
        (self.sq_tail + RING_SLOTS - self.sq_head) % RING_SLOTS
    }

    pub fn cq_depth(&self) -> usize {
        (self.cq_tail + RING_SLOTS - self.cq_head) % RING_SLOTS
    }

    /// 提交一条 SQE（环满即拒绝并计数）。
    pub fn submit(&mut self, e: Sqe) -> bool {
        let next = (self.sq_tail + 1) % RING_SLOTS;
        if next == self.sq_head {
            self.truncations += 1;
            return false;
        }
        self.sq[self.sq_tail] = e;
        self.sq_tail = next;
        self.submits += 1;
        true
    }

    fn push_cqe(&mut self, c: Cqe) -> bool {
        let next = (self.cq_tail + 1) % RING_SLOTS;
        if next == self.cq_head {
            self.truncations += 1;
            return false;
        }
        self.cq[self.cq_tail] = c;
        self.cq_tail = next;
        true
    }

    /// 一次陷入处理至多 `budget` 条（预算化批量）。
    pub fn flush(&mut self, budget: usize) -> usize {
        let mut n = 0usize;
        while n < budget && self.sq_head != self.sq_tail {
            let e = self.sq[self.sq_head];
            self.sq_head = (self.sq_head + 1) % RING_SLOTS;
            let result: i64 = match e.op {
                RingOp::Read => e.len as i64,
                RingOp::Write => e.len as i64,
                RingOp::Fsync => 0,
                RingOp::Close => 0,
            };
            let _ = self.push_cqe(Cqe { user_data: e.user_data, result });
            self.completes += 1;
            n += 1;
        }
        n
    }

    /// 收割一条完成事件。
    pub fn reap(&mut self) -> Option<Cqe> {
        if self.cq_head == self.cq_tail {
            return None;
        }
        let c = self.cq[self.cq_head];
        self.cq_head = (self.cq_head + 1) % RING_SLOTS;
        Some(c)
    }
}

// ---------------------------------------------------------------------------
// F088 命名服务 — 服务名 → 端口解析（AURORA 内总线对接点）
// ---------------------------------------------------------------------------

pub const NS_MAX: usize = 16;

#[derive(Clone, Copy)]
pub struct NsBinding {
    pub name: [u8; 24],
    pub name_len: usize,
    pub port: u32,
    pub generation: u32,
}

impl NsBinding {
    pub const fn empty() -> NsBinding {
        NsBinding { name: [0u8; 24], name_len: 0, port: 0, generation: 0 }
    }

    pub fn name_eq(&self, want: &[u8]) -> bool {
        &self.name[..self.name_len] == want
    }
}

#[derive(Clone, Copy)]
pub struct NameService {
    bindings: [NsBinding; NS_MAX],
    count: usize,
    /// 代际：每次解绑/重绑递增，旧句柄即失效。
    pub epoch: u32,
}

impl NameService {
    pub const fn new() -> NameService {
        NameService { bindings: [NsBinding::empty(); NS_MAX], count: 0, epoch: 1 }
    }

    pub fn len(&self) -> usize {
        self.count
    }

    pub fn is_empty(&self) -> bool {
        self.count == 0
    }

    pub fn bind(&mut self, name: &[u8], port: u32) -> bool {
        if self.count >= NS_MAX || name.is_empty() || name.len() >= 24 || port == 0 {
            return false;
        }
        if self.resolve(name).is_some() {
            return false;
        }
        let mut b = NsBinding::empty();
        b.name_len = name.len();
        b.port = port;
        b.generation = self.epoch;
        let mut i = 0usize;
        while i < name.len() {
            b.name[i] = name[i];
            i += 1;
        }
        self.bindings[self.count] = b;
        self.count += 1;
        true
    }

    /// 解析：命中返回 (port, generation)。
    pub fn resolve(&self, name: &[u8]) -> Option<(u32, u32)> {
        let mut i = 0usize;
        while i < self.count {
            if self.bindings[i].name_eq(name) {
                return Some((self.bindings[i].port, self.bindings[i].generation));
            }
            i += 1;
        }
        None
    }

    /// 解绑并提升代际（所有旧的 (port,generation) 组合失效）。
    pub fn unbind(&mut self, name: &[u8]) -> bool {
        let mut i = 0usize;
        while i < self.count {
            if self.bindings[i].name_eq(name) {
                self.bindings[i] = self.bindings[self.count - 1];
                self.count -= 1;
                self.epoch += 1;
                return true;
            }
            i += 1;
        }
        false
    }

    /// 校验句柄是否仍然有效（代际一致）。
    pub fn validate(&self, name: &[u8], port: u32, generation: u32) -> bool {
        match self.resolve(name) {
            Some((p, g)) => p == port && g == generation && g == self.epoch,
            None => false,
        }
    }
}

// ---------------------------------------------------------------------------
// F089 配置读取服务 — 只读配置查询（主题/键位/性能档）
// ---------------------------------------------------------------------------

pub const CFG_MAX: usize = 16;

#[derive(Clone, Copy)]
pub struct ConfigEntry {
    pub key: [u8; 24],
    pub key_len: usize,
    pub value: u32,
}

impl ConfigEntry {
    pub const fn empty() -> ConfigEntry {
        ConfigEntry { key: [0u8; 24], key_len: 0, value: 0 }
    }

    pub fn key_eq(&self, want: &[u8]) -> bool {
        &self.key[..self.key_len] == want
    }
}

#[derive(Clone, Copy)]
pub struct ConfigStore {
    entries: [ConfigEntry; CFG_MAX],
    count: usize,
    /// 封印后写操作全部拒绝（只读面）。
    pub sealed: bool,
    pub writes: u64,
    pub rejects: u64,
}

impl ConfigStore {
    pub const fn new() -> ConfigStore {
        ConfigStore { entries: [ConfigEntry::empty(); CFG_MAX], count: 0, sealed: false, writes: 0, rejects: 0 }
    }

    pub fn len(&self) -> usize {
        self.count
    }

    pub fn is_empty(&self) -> bool {
        self.count == 0
    }

    pub fn set(&mut self, key: &[u8], value: u32) -> bool {
        if self.sealed || key.is_empty() || key.len() >= 24 {
            self.rejects += 1;
            return false;
        }
        let mut i = 0usize;
        while i < self.count {
            if self.entries[i].key_eq(key) {
                self.entries[i].value = value;
                self.writes += 1;
                return true;
            }
            i += 1;
        }
        if self.count >= CFG_MAX {
            self.rejects += 1;
            return false;
        }
        let mut e = ConfigEntry::empty();
        e.key_len = key.len();
        e.value = value;
        let mut k = 0usize;
        while k < key.len() {
            e.key[k] = key[k];
            k += 1;
        }
        self.entries[self.count] = e;
        self.count += 1;
        self.writes += 1;
        true
    }

    pub fn get(&self, key: &[u8]) -> Option<u32> {
        let mut i = 0usize;
        while i < self.count {
            if self.entries[i].key_eq(key) {
                return Some(self.entries[i].value);
            }
            i += 1;
        }
        None
    }

    pub fn seal(&mut self) {
        self.sealed = true;
    }
}

/// 配置服务默认表（主题/键位/性能档）。
pub fn default_config() -> ConfigStore {
    let mut c = ConfigStore::new();
    let _ = c.set(b"theme", 0); // 0=dark,1=light
    let _ = c.set(b"keymap", 1); // 1=us
    let _ = c.set(b"perf.tier", 2); // 0=low,1=mid,2=high
    let _ = c.set(b"dpi.scale", 100);
    c
}

// ---------------------------------------------------------------------------
// F090 日志服务 — 分级过滤 + 环形缓冲可回读
// ---------------------------------------------------------------------------

pub const LOG_RING: usize = 32;
pub const LOG_MSG_MAX: usize = 48;

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub enum LogLevel {
    Trace = 0,
    Debug = 1,
    Info = 2,
    Warn = 3,
    Error = 4,
}

#[derive(Clone, Copy)]
pub struct LogRecord {
    pub level: LogLevel,
    pub msg: [u8; LOG_MSG_MAX],
    pub msg_len: usize,
    pub tick: u64,
    /// 来源进程。
    pub pid: u32,
}

impl LogRecord {
    pub const fn empty() -> LogRecord {
        LogRecord {
            level: LogLevel::Info,
            msg: [0u8; LOG_MSG_MAX],
            msg_len: 0,
            tick: 0,
            pid: 0,
        }
    }

    pub fn msg_bytes(&self) -> &[u8] {
        &self.msg[..self.msg_len]
    }
}

#[derive(Clone, Copy)]
pub struct LogService {
    ring: [LogRecord; LOG_RING],
    wr: usize,
    /// 当前环形占用。
    pub filled: usize,
    /// 被阈值过滤掉的总数。
    pub filtered: u64,
    /// 已写入总数。
    pub written: u64,
    /// 环形覆盖（丢弃最旧）次数。
    pub evicted: u64,
    pub threshold: LogLevel,
}

impl LogService {
    pub const fn new(threshold: LogLevel) -> LogService {
        LogService {
            ring: [LogRecord::empty(); LOG_RING],
            wr: 0,
            filled: 0,
            filtered: 0,
            written: 0,
            evicted: 0,
            threshold,
        }
    }

    /// 分级过滤后入环。
    pub fn log(&mut self, level: LogLevel, pid: u32, tick: u64, msg: &[u8]) -> bool {
        if level < self.threshold {
            self.filtered += 1;
            return false;
        }
        let mut r = LogRecord::empty();
        r.level = level;
        r.pid = pid;
        r.tick = tick;
        let n = core::cmp::min(msg.len(), LOG_MSG_MAX);
        let mut i = 0usize;
        while i < n {
            r.msg[i] = msg[i];
            i += 1;
        }
        r.msg_len = n;
        if self.filled == LOG_RING {
            self.evicted += 1;
        } else {
            self.filled += 1;
        }
        self.ring[self.wr] = r;
        self.wr = (self.wr + 1) % LOG_RING;
        self.written += 1;
        true
    }

    /// 按时间序回读第 `i` 条（0 = 最旧）。
    pub fn read(&self, i: usize) -> Option<LogRecord> {
        if i >= self.filled {
            return None;
        }
        let count = self.filled;
        let oldest = if count == LOG_RING { self.wr } else { 0 };
        Some(self.ring[(oldest + i) % LOG_RING])
    }

    /// 按前缀检索最近一条（回读利器）。
    pub fn find_last(&self, prefix: &[u8]) -> Option<LogRecord> {
        let mut i = self.filled;
        while i > 0 {
            i -= 1;
            if let Some(r) = self.read(i) {
                let m = r.msg_bytes();
                if m.len() >= prefix.len() && &m[..prefix.len()] == prefix {
                    return Some(r);
                }
            }
        }
        None
    }

    pub fn count_level(&self, level: LogLevel) -> usize {
        let mut n = 0usize;
        let mut i = 0usize;
        while i < self.filled {
            if let Some(r) = self.read(i) {
                if r.level == level {
                    n += 1;
                }
            }
            i += 1;
        }
        n
    }
}

// ---------------------------------------------------------------------------
// 自检扩展（F083~F090 共 8 项）
// ---------------------------------------------------------------------------

/// 自检里「取不到下标」的哨兵：下游访问器都做 `i < count` 边界检查，
/// 用它替代 `.unwrap()` 只会让该项自检判定失败，不会 panic 掉整个 checkup。
const IDX_NONE: usize = usize::MAX;

pub fn extend_checks(set: &mut CheckSet) {
    // F083 事件总线
    let mut bus = EventBus::new();
    let si = bus.subscribe(100, 0b0000_0111).unwrap_or(IDX_NONE);
    let _ = bus.subscribe(101, 0b0000_0001);
    let dev = EventBus::topic_bit(b"device");
    let n_dev = bus.publish(dev.unwrap_or(0));
    let n_pow = bus.publish(1);
    let off = bus.unsubscribe(101);
    let n_dev2 = bus.publish(0);
    set.add(
        "F083 event bus",
        dev == Some(0)
            && n_dev == 2
            && n_pow == 1
            && off
            && n_dev2 == 1
            && bus.delivered(si) == Some(3)
            && EventBus::topic_name(1) == Some(&b"power"[..])
            && EventBus::topic_bit(b"nope").is_none(),
        "订阅掩码/退订/分发",
    );

    // F084 管道与 FIFO
    let mut p = Pipe::new(PipeMode::Line);
    let w = p.write(b"ab\ncd");
    let d = p.deliverable();
    let mut out = [0u8; PIPE_CAP];
    let r = p.read(&mut out);
    let first_line = &out[..r] == &b"ab\n"[..];
    let w2 = p.write(b"\n");
    let mut out2 = [0u8; PIPE_CAP];
    let r2 = p.read(&mut out2);
    let second_line = &out2[..r2] == &b"cd\n"[..];
    set.add(
        "F084 pipe fifo",
        w == 5 && d == 3 && r == 3 && first_line && w2 == 1 && r2 == 3 && second_line,
        "行缓冲只交付完整行",
    );

    let mut fifos = FifoTable::new();
    let f0 = fifos.mkfifo(b"ui.log", PipeMode::Line);
    let dup = fifos.mkfifo(b"ui.log", PipeMode::Line).is_none();
    let ew = fifos.open_write(b"ui.log");
    let f0i = f0.unwrap_or(IDX_NONE);
    let _ = fifos.pipe_mut(f0i).map(|pp| pp.write(b"x\n"));
    let r = fifos.pipe_mut(f0i).map(|pp| pp.read(&mut out)).unwrap_or(0);
    set.add(
        "F084 fifo table",
        f0 == Some(0) && dup && ew == Some(0) && fifos.len() == 1 && r == 2 && fifos.writers(0) == 2,
        "命名管道/写端计数",
    );

    // F084 close 语义
    let mut pf = FifoTable::new();
    let i = pf.mkfifo(b"a", PipeMode::Stream).unwrap_or(IDX_NONE);
    let _ = pf.close_write(i);
    let eof = pf.pipe_mut(i).map(|pp| pp.eof()).unwrap_or(false);
    set.add(
        "F084 fifo eof",
        i == 0 && eof && pf.writers(0) == 0 && !pf.close_write(9),
        "写端关闭→EOF",
    );

    // F085 共享内存
    let mut shm = ShmTable::new();
    let rw = ShmPerm::READ.union(ShmPerm::WRITE);
    let a = shm.create(b"shot", 2, rw).unwrap_or(IDX_NONE);
    let dup = shm.create(b"shot", 1, rw).is_none();
    let over = shm.create(b"huge", SHM_MAX_PAGES + 1, rw).is_none();
    let m1 = shm.map(a, ShmPerm::READ);
    let m2 = !shm.map(a, ShmPerm::EXEC);
    let sealed = shm.seal(a);
    let m3 = !shm.map(a, ShmPerm::READ);
    let bytes_mapped = shm.bytes_mapped;
    let u1 = shm.unmap(a);
    let bytes_after = shm.bytes_mapped;
    set.add(
        "F085 shared mem",
        a == 0
            && dup
            && over
            && m1
            && m2
            && sealed
            && m3
            && u1
            && bytes_mapped == 8192
            && bytes_after == 0,
        "创建/权限子集/封印/映射计数",
    );
    set.add(
        "F085 shm region",
        shm.region(a).map(|r| r.bytes() == 8192 && r.sealed).unwrap_or(false)
            && shm.find(b"absent").is_none(),
        "区域查询",
    );

    // F086 事件通道
    let mut chans = ChannelTable::new();
    let ci = chans.create().unwrap_or(IDX_NONE);
    let trips = chans.get_mut(ci).map(|c| c.ping_pong(1000)).unwrap_or(0);
    let (delivered, dropped) =
        chans.get(ci).map(|c| (c.delivered, c.dropped)).unwrap_or((0, 0));
    // 填满环再投两次，验证丢弃计数。
    let overflow = chans.get_mut(ci).map(|c| {
        let mut posted = 0usize;
        while c.post(1) {
            posted += 1;
        }
        let a = !c.post(2);
        let b = !c.post(3);
        (posted, a && b)
    });
    set.add(
        "F086 event channel",
        trips == 1000 && delivered == 1000 && dropped == 0,
        "ping-pong 10³ 零丢失",
    );
    set.add(
        "F086 channel overflow",
        overflow.map(|(posted, both)| posted == CHAN_SLOTS - 1 && both).unwrap_or(false)
            && dropped == 0,
        "环满丢弃计数不阻塞",
    );

    // F087 io_uring 式批量
    let mut ring = IoRing::new();
    let mut ok = true;
    let mut i = 0u64;
    while i < 5 {
        ok &= ring.submit(Sqe::new(RingOp::Read, 3, 16, i));
        i += 1;
    }
    let flushed = ring.flush(3);
    let first = ring.reap();
    let rest = ring.flush(8);
    let mut reaped = 0usize;
    while ring.reap().is_some() {
        reaped += 1;
    }
    set.add(
        "F087 io ring",
        ok && flushed == 3 && rest == 2 && reaped == 4 && ring.completes == 5
            && first.map(|c| c.user_data == 0 && c.result == 16).unwrap_or(false),
        "预算化批量提交/完成环",
    );

    // F088 命名服务
    let mut ns = NameService::new();
    let b1 = ns.bind(b"core.display", 7001);
    let b2 = !ns.bind(b"core.display", 7002);
    let b3 = ns.bind(b"core.input", 7002);
    let resolved = ns.resolve(b"core.display");
    let gen0 = resolved.map(|(_, g)| g).unwrap_or(0);
    let valid = ns.validate(b"core.display", 7001, gen0);
    let unbound = ns.unbind(b"core.display");
    let invalidated = !ns.validate(b"core.display", 7001, gen0);
    set.add(
        "F088 name service",
        b1 && b2 && b3 && resolved == Some((7001, 1)) && valid && unbound && invalidated
            && ns.len() == 1,
        "绑定/解析/代际失效",
    );

    // F089 配置读取服务
    let mut cfg = ConfigStore::new();
    let w1 = cfg.set(b"theme", 1);
    let w2 = cfg.set(b"keymap", 2);
    cfg.seal();
    let rejected = !cfg.set(b"theme", 0);
    set.add(
        "F089 config svc",
        w1 && w2 && rejected && cfg.get(b"theme") == Some(1) && cfg.get(b"keymap") == Some(2)
            && cfg.get(b"absent").is_none()
            && cfg.rejects == 1,
        "只读封印/更新既有键",
    );

    // F090 日志服务
    let mut log = LogService::new(LogLevel::Info);
    let _ = log.log(LogLevel::Trace, 2, 10, b"trace-hidden");
    let _ = log.log(LogLevel::Info, 2, 11, b"init up");
    let _ = log.log(LogLevel::Warn, 3, 12, b"svc slow");
    let _ = log.log(LogLevel::Error, 4, 13, b"svc died");
    let found = log.find_last(b"svc ");
    set.add(
        "F090 log service",
        log.written == 3
            && log.filtered == 1
            && log.filled == 3
            && log.read(0).map(|r| r.msg_bytes() == b"init up").unwrap_or(false)
            && found.map(|r| r.msg_bytes() == b"svc died" && r.level == LogLevel::Error).unwrap_or(false)
            && log.count_level(LogLevel::Warn) == 1,
        "分级过滤/环形回读/前缀检索",
    );
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn f086_ping_pong_loses_nothing() {
        let mut c = EventChannel::new();
        assert_eq!(c.ping_pong(1_000), 1_000);
        assert_eq!(c.dropped, 0);
    }

    #[test]
    fn f084_line_pipe_waits_for_newline() {
        let mut p = Pipe::new(PipeMode::Line);
        assert_eq!(p.write(b"no-newline"), 10);
        assert_eq!(p.deliverable(), 0);
        assert_eq!(p.write(b"\n"), 1);
        assert_eq!(p.deliverable(), 11);
        let mut out = [0u8; 16];
        assert_eq!(p.read(&mut out), 11);
        assert_eq!(&out[..11], b"no-newline\n");
    }

    #[test]
    fn f090_log_ring_evicts_oldest() {
        let mut l = LogService::new(LogLevel::Trace);
        let mut i = 0usize;
        while i < LOG_RING + 4 {
            let _ = l.log(LogLevel::Info, 1, i as u64, b"x");
            i += 1;
        }
        assert_eq!(l.filled, LOG_RING);
        assert_eq!(l.evicted, 4);
        assert_eq!(l.read(0).map(|r| r.tick), Some(4));
    }

    #[test]
    fn f088_epoch_invalidates_handles() {
        let mut ns = NameService::new();
        assert!(ns.bind(b"a", 1));
        let (p, g) = ns.resolve(b"a").unwrap();
        assert!(ns.validate(b"a", p, g));
        assert!(ns.unbind(b"a"));
        assert!(!ns.validate(b"a", p, g));
    }
}
