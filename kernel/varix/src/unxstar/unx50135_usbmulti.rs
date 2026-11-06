//! UNX-F50135 多设备并发账（双 USB 打印机并发零串扰）（0x43d00 段）
//!
//! 锚点：UNX-F50135-J1 并发 ×10 零串扰断言；判据锚定 §AI-63 §2 专题八。
//!
//! 两台打印机同时打印时，最危险的不是慢，是**串**：A 的块被写进 B 的队列、
//! B 的序号覆盖了 A 的游标。这类 bug 在单设备回归里永远不出现，在真机上却
//! 表现为「偶尔印出别人文档的半页」。
//!
//! 本模块把并发拆成三层可审计的机制：
//! 1. **块头自洽**：每个块自带 (dev, seq, len) 的 CRC；块被写坏先于路由被拒。
//! 2. **路由对账**：`submit(queue_dev, chunk)` 显式分开「后端选定的队列」与
//!    「块自带标签」，两者不一致立刻记串扰并拒绝——这正是串扰的唯一入口。
//! 3. **调度复核**：`pump` 交替从两个队列取块（严格轮转），取块时校验设备内
//!    序号连续；`seal` 再扫一遍事件账，确认设备内序号严格递增、总量守恒。
//!
//! 「零串扰」因此不是一句口号，而是三个可断言量的合取：crosstalk == 0、
//! 每设备事件标签一致、每设备字节数与文档声明相等。
//!
//! 数据结构：
//! * `Chunk` —— 带自校验头的数据块；
//! * `DeviceQueue` —— 单设备 FIFO（游标、期望序号、已交付字节）；
//! * `MultiLedger` —— 双队列 + 事件账（串扰/乱序/跳号计数）；
//! * `Report` / `MultiRound` —— 单次封账与 ×10 复跑聚合。
//!
//! 错误路径：`MultiError::code()` 返回 `SEG_BASE + n`，段内闭集；全部码值都在
//! 判据或注入核里被真实构造。

use crate::alloc::format;
use crate::alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x43d00 独占 0x20）。
pub const SEG_BASE: u32 = 0x43d00;

pub const E_NO_DEVICE: u16 = 1;
pub const E_DUP_DEVICE: u16 = 2;
pub const E_CAPACITY: u16 = 3;
pub const E_EMPTY_DOC: u16 = 4;
pub const E_CROSSTALK: u16 = 5;
pub const E_REORDER: u16 = 6;
pub const E_SEQ_GAP: u16 = 7;
pub const E_SHORT_DELIVERY: u16 = 8;
pub const E_DEVICE_MIX: u16 = 9;
pub const E_DIGEST_DRIFT: u16 = 10;
pub const E_CHUNK_EMPTY: u16 = 11;
pub const E_CHUNK_CRC: u16 = 12;
pub const E_UNKNOWN: u16 = 13;
pub const E_ROUNDS: u16 = 14;

/// 并发设备数（「双 USB 打印机」是两个，不是「至少两个」）。
pub const DEVICES: usize = 2;

/// 每台设备的文档分块数。8 块足以让交替调度跨过偶数/奇数边界。
pub const CHUNKS_PER_DEVICE: u16 = 8;

/// 块大小：512 字节 = 高速 bulk 单包满包。
pub const CHUNK_LEN: u16 = 512;

/// 事件账容量（两设备 × 8 块 = 16，留一倍余量给注入核）。
pub const MAX_EVENTS: usize = 32;

/// 复跑轮数上限。
pub const MAX_ROUNDS: u8 = 16;

/// J1 规定的复跑轮数：×10。
pub const TEN_ROUNDS: u8 = 10;

/// 并发账错误闭集。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MultiError {
    NoDevice,
    DupDevice,
    Capacity,
    EmptyDoc,
    Crosstalk,
    Reorder,
    SeqGap,
    ShortDelivery,
    DeviceMix,
    DigestDrift,
    ChunkEmpty,
    ChunkCrc,
    Unknown,
    Rounds,
}

impl MultiError {
    /// 诊断码：段基址 + 段内序号（闭集，全仓唯一）。
    pub fn code(self) -> u32 {
        let n = match self {
            MultiError::NoDevice => E_NO_DEVICE,
            MultiError::DupDevice => E_DUP_DEVICE,
            MultiError::Capacity => E_CAPACITY,
            MultiError::EmptyDoc => E_EMPTY_DOC,
            MultiError::Crosstalk => E_CROSSTALK,
            MultiError::Reorder => E_REORDER,
            MultiError::SeqGap => E_SEQ_GAP,
            MultiError::ShortDelivery => E_SHORT_DELIVERY,
            MultiError::DeviceMix => E_DEVICE_MIX,
            MultiError::DigestDrift => E_DIGEST_DRIFT,
            MultiError::ChunkEmpty => E_CHUNK_EMPTY,
            MultiError::ChunkCrc => E_CHUNK_CRC,
            MultiError::Unknown => E_UNKNOWN,
            MultiError::Rounds => E_ROUNDS,
        };
        SEG_BASE + n as u32
    }

    /// 一行可读原因（诊断/日志共用）。
    pub fn message(self) -> &'static str {
        match self {
            MultiError::NoDevice => "device queue not registered",
            MultiError::DupDevice => "device registered twice",
            MultiError::Capacity => "chunk buffer or event ledger full",
            MultiError::EmptyDoc => "no device or no chunk to seal",
            MultiError::Crosstalk => "chunk dev tag disagrees with target queue",
            MultiError::Reorder => "chunk sequence went backwards",
            MultiError::SeqGap => "chunk sequence skipped a number",
            MultiError::ShortDelivery => "seal attempted before full delivery",
            MultiError::DeviceMix => "device id outside 1..=2",
            MultiError::DigestDrift => "two rounds produced different ledgers",
            MultiError::ChunkEmpty => "zero-length chunk",
            MultiError::ChunkCrc => "chunk header crc mismatch",
            MultiError::Unknown => "device not in ledger",
            MultiError::Rounds => "round count outside 1..=16",
        }
    }
}

fn fnv_u8(mut h: u64, b: u8) -> u64 {
    h ^= b as u64;
    h.wrapping_mul(0x100_0000_01b3)
}

/// 数据块。`crc` 覆盖块头的三个字段：串扰最典型的形态就是「头被另一个设备
/// 的字段覆盖」，所以块头必须自带校验，而不是只校验载荷。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Chunk {
    pub dev: u8,
    pub seq: u16,
    pub len: u16,
    pub crc: u32,
}

impl Chunk {
    fn compute(&self) -> u32 {
        let mut h: u32 = 0x811c_9dc5;
        h ^= self.dev as u32;
        h = h.wrapping_mul(0x0100_0193);
        h ^= (self.seq & 0xff) as u32;
        h = h.wrapping_mul(0x0100_0193);
        h ^= (self.seq >> 8) as u32;
        h = h.wrapping_mul(0x0100_0193);
        h ^= (self.len & 0xff) as u32;
        h = h.wrapping_mul(0x0100_0193);
        h ^= (self.len >> 8) as u32;
        h.wrapping_mul(0x0100_0193)
    }

    /// 签发一个自洽的块（crc 现算）。
    pub fn sealed(dev: u8, seq: u16, len: u16) -> Chunk {
        let mut c = Chunk {
            dev,
            seq,
            len,
            crc: 0,
        };
        c.crc = c.compute();
        c
    }

    /// 原始构造（只供坏 CRC 注入；正常路径请走 `sealed`）。
    pub fn raw(dev: u8, seq: u16, len: u16, crc: u32) -> Chunk {
        Chunk {
            dev,
            seq,
            len,
            crc,
        }
    }

    /// 块头是否与 CRC 相符。
    pub fn crc_ok(&self) -> bool {
        self.crc == self.compute()
    }
}

/// 一次交付事件（事件账的单元，`order` 是全局交付序号）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Event {
    pub dev: u8,
    pub seq: u16,
    pub len: u16,
    pub order: u16,
}

/// 单设备队列。
#[derive(Clone, Debug)]
pub struct DeviceQueue {
    pub dev: u8,
    chunks: Vec<Chunk>,
    cursor: usize,
    next_seq: u16,
    delivered: u32,
}

impl DeviceQueue {
    pub fn dev(&self) -> u8 {
        self.dev
    }

    pub fn pending(&self) -> usize {
        self.chunks.len().saturating_sub(self.cursor)
    }

    pub fn queued(&self) -> usize {
        self.chunks.len()
    }

    pub fn cursor(&self) -> usize {
        self.cursor
    }

    pub fn delivered_bytes(&self) -> u32 {
        self.delivered
    }

    pub fn drained(&self) -> bool {
        self.cursor == self.chunks.len()
    }
}

/// 双设备并发账。
pub struct MultiLedger {
    queues: Vec<DeviceQueue>,
    events: Vec<Event>,
    crosstalk: u16,
    reorder: u16,
    seqgap: u16,
    rounds: u8,
}

impl MultiLedger {
    pub fn new() -> MultiLedger {
        MultiLedger {
            queues: Vec::with_capacity(DEVICES),
            events: Vec::with_capacity(MAX_EVENTS),
            crosstalk: 0,
            reorder: 0,
            seqgap: 0,
            rounds: 0,
        }
    }

    pub fn crosstalk(&self) -> u16 {
        self.crosstalk
    }

    pub fn reorder(&self) -> u16 {
        self.reorder
    }

    pub fn seqgap(&self) -> u16 {
        self.seqgap
    }

    pub fn events(&self) -> usize {
        self.events.len()
    }

    pub fn event_at(&self, i: usize) -> Option<Event> {
        self.events.get(i).copied()
    }

    pub fn rounds(&self) -> u8 {
        self.rounds
    }

    fn index_of(&self, dev: u8) -> Option<usize> {
        let mut i = 0usize;
        while i < self.queues.len() {
            if let Some(q) = self.queues.get(i) {
                if q.dev == dev {
                    return Some(i);
                }
            }
            i += 1;
        }
        None
    }

    /// 只读取队列（未登记设备返回 `Unknown`，不返回空队列冒充）。
    pub fn queue(&self, dev: u8) -> Result<&DeviceQueue, MultiError> {
        match self.index_of(dev).and_then(|i| self.queues.get(i)) {
            Some(q) => Ok(q),
            None => Err(MultiError::Unknown),
        }
    }

    /// 登记设备。设备号只有 1/2（`DeviceMix` 拦 0 与 3+）。
    pub fn register(&mut self, dev: u8) -> Result<usize, MultiError> {
        if dev < 1 || dev as usize > DEVICES {
            return Err(MultiError::DeviceMix);
        }
        if self.index_of(dev).is_some() {
            return Err(MultiError::DupDevice);
        }
        if self.queues.len() >= DEVICES {
            return Err(MultiError::Capacity);
        }
        self.queues.push(DeviceQueue {
            dev,
            chunks: Vec::with_capacity(CHUNKS_PER_DEVICE as usize),
            cursor: 0,
            next_seq: 0,
            delivered: 0,
        });
        Ok(self.queues.len() - 1)
    }

    fn buffered(&self) -> usize {
        let mut n = 0usize;
        let mut i = 0usize;
        while i < self.queues.len() {
            if let Some(q) = self.queues.get(i) {
                n += q.pending();
            }
            i += 1;
        }
        n
    }

    /// 入队。`queue_dev` 是后端选定的目标队列，`chunk.dev` 是数据自带标签：
    /// 两者不一致就是串扰的现场证据——先把块拦下、记账，绝不「按块头改路由」
    /// （那会把串扰伪装成成功）。
    pub fn submit(&mut self, queue_dev: u8, chunk: Chunk) -> Result<usize, MultiError> {
        if chunk.len == 0 {
            return Err(MultiError::ChunkEmpty);
        }
        if chunk.dev != queue_dev {
            self.crosstalk = self.crosstalk.saturating_add(1);
            return Err(MultiError::Crosstalk);
        }
        if !chunk.crc_ok() {
            return Err(MultiError::ChunkCrc);
        }
        let qi = match self.index_of(queue_dev) {
            Some(i) => i,
            None => return Err(MultiError::NoDevice),
        };
        if self.buffered() >= MAX_EVENTS || self.events.len() + 1 > MAX_EVENTS {
            return Err(MultiError::Capacity);
        }
        let q = match self.queues.get_mut(qi) {
            Some(x) => x,
            None => return Err(MultiError::NoDevice),
        };
        if chunk.seq < q.next_seq {
            self.reorder = self.reorder.saturating_add(1);
            return Err(MultiError::Reorder);
        }
        if chunk.seq > q.next_seq {
            self.seqgap = self.seqgap.saturating_add(1);
            return Err(MultiError::SeqGap);
        }
        q.next_seq = q.next_seq.saturating_add(1);
        q.chunks.push(chunk);
        Ok(q.chunks.len() - 1)
    }

    /// 交替调度：每一轮从每个非空队列各取一块（严格轮转，无饥饿）。
    ///
    /// 取块即记账：`delivered` 累加字节、`events` 追加交付事件。轮转从设备
    /// 登记序开始，因此交付序是确定的——两轮同样内容必然得到同样事件账。
    pub fn pump(&mut self) -> Result<usize, MultiError> {
        let mut delivered = 0usize;
        let mut sweeps = 0usize;
        loop {
            let mut progressed = false;
            let mut qi = 0usize;
            while qi < self.queues.len() {
                let mut taken: Option<Chunk> = None;
                if let Some(q) = self.queues.get_mut(qi) {
                    let cursor = q.cursor;
                    if let Some(c) = q.chunks.get(cursor).copied() {
                        q.cursor = cursor + 1;
                        q.delivered = q.delivered.saturating_add(c.len as u32);
                        taken = Some(c);
                    }
                }
                if let Some(c) = taken {
                    let order = self.events.len() as u16;
                    self.events.push(Event {
                        dev: c.dev,
                        seq: c.seq,
                        len: c.len,
                        order,
                    });
                    delivered += 1;
                    progressed = true;
                }
                qi += 1;
            }
            if !progressed {
                break;
            }
            sweeps += 1;
            // 护栏：合法账本最多 32 块，扫描次数超过它只能是状态坏了。
            if sweeps > MAX_EVENTS {
                break;
            }
        }
        self.rounds = self.rounds.saturating_add(1);
        Ok(delivered)
    }

    /// 设备内序号严格递增且从 0 连续（事件账口径的复核）。
    pub fn order_ok(&self) -> bool {
        let mut last = [-1i32; DEVICES];
        let mut i = 0usize;
        while i < self.events.len() {
            if let Some(e) = self.events.get(i) {
                let slot = (e.dev as usize).wrapping_sub(1);
                if slot >= DEVICES {
                    return false;
                }
                let want = match last.get(slot) {
                    Some(v) => *v + 1,
                    None => return false,
                };
                if e.seq as i32 != want {
                    return false;
                }
                if let Some(cell) = last.get_mut(slot) {
                    *cell = want;
                }
            }
            i += 1;
        }
        true
    }

    /// 交付序是否严格交替（双设备等量文档下应恒为真）。
    pub fn alternating(&self) -> bool {
        if self.events.len() < 2 {
            return true;
        }
        let mut i = 1usize;
        while i < self.events.len() {
            let a = match self.events.get(i - 1) {
                Some(e) => e.dev,
                None => return false,
            };
            let b = match self.events.get(i) {
                Some(e) => e.dev,
                None => return false,
            };
            if a == b {
                return false;
            }
            i += 1;
        }
        true
    }

    /// 事件账摘要（按交付序折叠 dev/seq/len）。
    pub fn digest(&self) -> u64 {
        let mut h: u64 = 0xcbf2_9ce4_8422_2325 ^ self.events.len() as u64;
        let mut i = 0usize;
        while i < self.events.len() {
            if let Some(e) = self.events.get(i) {
                h = fnv_u8(h, e.dev);
                h = fnv_u8(h, (e.seq & 0xff) as u8);
                h = fnv_u8(h, (e.seq >> 8) as u8);
                h = fnv_u8(h, (e.len & 0xff) as u8);
                h = fnv_u8(h, (e.len >> 8) as u8);
            }
            i += 1;
        }
        h
    }

    /// 封账：全部块必须已交付、串扰必须为零、事件数必须与块数相等。
    pub fn seal(&self) -> Result<Report, MultiError> {
        if self.queues.is_empty() {
            return Err(MultiError::EmptyDoc);
        }
        if self.crosstalk != 0 {
            return Err(MultiError::Crosstalk);
        }
        let mut chunks: u16 = 0;
        let mut bytes: u32 = 0;
        let mut delivered = [0u16; DEVICES];
        let mut per_device_bytes = [0u32; DEVICES];
        let mut i = 0usize;
        while i < self.queues.len() {
            let q = match self.queues.get(i) {
                Some(x) => x,
                None => return Err(MultiError::EmptyDoc),
            };
            if !q.drained() {
                return Err(MultiError::ShortDelivery);
            }
            chunks = chunks.saturating_add(q.chunks.len() as u16);
            bytes = bytes.saturating_add(q.delivered);
            let slot = (q.dev as usize).wrapping_sub(1);
            if slot < DEVICES {
                if let Some(cell) = delivered.get_mut(slot) {
                    *cell = q.chunks.len() as u16;
                }
                if let Some(cell) = per_device_bytes.get_mut(slot) {
                    *cell = q.delivered;
                }
            }
            i += 1;
        }
        if chunks == 0 {
            return Err(MultiError::EmptyDoc);
        }
        if self.events.len() != chunks as usize {
            return Err(MultiError::ShortDelivery);
        }
        Ok(Report {
            devices: self.queues.len(),
            chunks,
            bytes,
            events: self.events.len(),
            crosstalk: self.crosstalk,
            reorder: self.reorder,
            seqgap: self.seqgap,
            delivered,
            per_device_bytes,
            digest: self.digest(),
            alternating: self.alternating(),
            order_ok: self.order_ok(),
        })
    }
}

/// 单次封账报告。`delivered`/`per_device_bytes` 按设备号 -1 落槽。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Report {
    pub devices: usize,
    pub chunks: u16,
    pub bytes: u32,
    pub events: usize,
    pub crosstalk: u16,
    pub reorder: u16,
    pub seqgap: u16,
    pub delivered: [u16; DEVICES],
    pub per_device_bytes: [u32; DEVICES],
    pub digest: u64,
    pub alternating: bool,
    pub order_ok: bool,
}

/// ×10 复跑聚合。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MultiRound {
    pub rounds: u8,
    pub identical: bool,
    pub crosstalk: u32,
    pub bytes: u32,
    pub delivered: [u16; DEVICES],
    pub digest: u64,
}

/// 两轮报告比对：字段级相等，不等即 `DigestDrift`。
pub fn compare_rounds(a: &Report, b: &Report) -> Result<(), MultiError> {
    if a.digest == b.digest
        && a.bytes == b.bytes
        && a.chunks == b.chunks
        && a.delivered == b.delivered
        && a.per_device_bytes == b.per_device_bytes
    {
        Ok(())
    } else {
        Err(MultiError::DigestDrift)
    }
}

fn submit_document(led: &mut MultiLedger, pattern: u8, len: u16) -> Result<(), MultiError> {
    match pattern % 3 {
        0 => {
            let mut i = 0u16;
            while i < CHUNKS_PER_DEVICE {
                led.submit(1, Chunk::sealed(1, i, len))?;
                led.submit(2, Chunk::sealed(2, i, len))?;
                i += 1;
            }
        }
        1 => {
            let mut i = 0u16;
            while i < CHUNKS_PER_DEVICE {
                led.submit(2, Chunk::sealed(2, i, len))?;
                led.submit(1, Chunk::sealed(1, i, len))?;
                i += 1;
            }
        }
        _ => {
            // 分组提交：先把 A 的 8 块全部压入，再压 B 的 8 块。
            // 提交次序不该影响交付次序——这是「零串扰」之外的另一条独立性。
            let mut i = 0u16;
            while i < CHUNKS_PER_DEVICE {
                led.submit(1, Chunk::sealed(1, i, len))?;
                i += 1;
            }
            let mut k = 0u16;
            while k < CHUNKS_PER_DEVICE {
                led.submit(2, Chunk::sealed(2, k, len))?;
                k += 1;
            }
        }
    }
    Ok(())
}

/// 跑一次双设备并发场景（提交 → 交替调度 → 封账）。
pub fn run_scenario_with(pattern: u8, len: u16) -> Result<Report, MultiError> {
    let mut led = MultiLedger::new();
    led.register(1)?;
    led.register(2)?;
    submit_document(&mut led, pattern, len)?;
    led.pump()?;
    led.seal()
}

/// 跑一次默认块大小的场景。
pub fn run_scenario(pattern: u8) -> Result<Report, MultiError> {
    run_scenario_with(pattern, CHUNK_LEN)
}

/// ×N 复跑：每轮换一种提交次序，要求报告逐字段一致、串扰累计为零。
pub fn run_rounds(rounds: u8) -> Result<MultiRound, MultiError> {
    if rounds == 0 || rounds > MAX_ROUNDS {
        return Err(MultiError::Rounds);
    }
    let mut first: Option<Report> = None;
    let mut identical = true;
    let mut crosstalk: u32 = 0;
    let mut r = 0u8;
    while r < rounds {
        let rep = run_scenario(r)?;
        crosstalk += rep.crosstalk as u32;
        match first {
            None => first = Some(rep),
            Some(f) => {
                if compare_rounds(&f, &rep).is_err() {
                    identical = false;
                }
            }
        }
        r += 1;
    }
    match first {
        Some(f) => Ok(MultiRound {
            rounds,
            identical,
            crosstalk,
            bytes: f.bytes,
            delivered: f.delivered,
            digest: f.digest,
        }),
        None => Err(MultiError::EmptyDoc),
    }
}

/// 诊断渲染：一行说清「几台、几块、几字节、串扰/乱序/跳号」。
pub fn render(r: &Report) -> String {
    format!(
        "multi devs={} chunks={} bytes={} events={} xtalk={} reorder={} gap={} alt={} order={}",
        r.devices,
        r.chunks,
        r.bytes,
        r.events,
        r.crosstalk,
        r.reorder,
        r.seqgap,
        r.alternating,
        r.order_ok
    )
}

// ---------------------------------------------------------------------------
// 注入核：七类坏并发必须被显式拒绝
// ---------------------------------------------------------------------------

pub fn injection_core() -> (usize, usize) {
    let mut caught = 0usize;
    let total = 7usize;

    // 1 串扰：B 的块塞进 A 的队列
    let mut led = MultiLedger::new();
    let _ = led.register(1);
    let _ = led.register(2);
    let xt = led.submit(1, Chunk::sealed(2, 0, CHUNK_LEN));
    if xt.err() == Some(MultiError::Crosstalk) && led.crosstalk() == 1 {
        caught += 1;
    }
    // 2 乱序：0 之后再交 0
    let ok0 = led.submit(1, Chunk::sealed(1, 0, CHUNK_LEN));
    let rev = led.submit(1, Chunk::sealed(1, 0, CHUNK_LEN));
    if ok0.is_ok() && rev.err() == Some(MultiError::Reorder) {
        caught += 1;
    }
    // 3 跳号：直接交 seq=5
    let gap = led.submit(2, Chunk::sealed(2, 5, CHUNK_LEN));
    if gap.err() == Some(MultiError::SeqGap) {
        caught += 1;
    }
    // 4 空块
    if led.submit(1, Chunk::sealed(1, 1, 0)).err() == Some(MultiError::ChunkEmpty) {
        caught += 1;
    }
    // 5 块头 CRC 被改
    let mut bad = Chunk::sealed(1, 1, CHUNK_LEN);
    bad.len = 256;
    if led.submit(1, bad).err() == Some(MultiError::ChunkCrc) {
        caught += 1;
    }
    // 6 未交付就封账（新账本，避免前五项留下的串扰计数抢先报错）
    let mut half = MultiLedger::new();
    let _ = half.register(1);
    let _ = half.submit(1, Chunk::sealed(1, 0, CHUNK_LEN));
    if half.seal().err() == Some(MultiError::ShortDelivery) {
        caught += 1;
    }
    // 7 两轮不一致：块大小不同 → 摘要/字节必不同
    let a = run_scenario_with(0, CHUNK_LEN);
    let b = run_scenario_with(0, CHUNK_LEN / 2);
    let drift = match (a, b) {
        (Ok(x), Ok(y)) => compare_rounds(&x, &y).err(),
        _ => None,
    };
    if drift == Some(MultiError::DigestDrift) {
        caught += 1;
    }

    (caught, total)
}

// ---------------------------------------------------------------------------
// 判据
// ---------------------------------------------------------------------------

pub fn run_unx50135_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F50135");

    // J1-1 双设备登记：1/2 成功，重号与越界号必拒
    let mut reg = MultiLedger::new();
    let r1 = reg.register(1);
    let r2 = reg.register(2);
    let dup = reg.register(2);
    let zero = reg.register(0);
    let three = reg.register(3);
    // 未登记设备号不可投递：块标签合法但队列不存在，必须是 NoDevice，
    // 不能借「自动建队列」把错路由洗成成功。
    let mut lone = MultiLedger::new();
    let _ = lone.register(1);
    let ghost = lone.submit(2, Chunk::sealed(2, 0, CHUNK_LEN));
    let ok = r1.is_ok()
        && r2.is_ok()
        && dup.err() == Some(MultiError::DupDevice)
        && zero.err() == Some(MultiError::DeviceMix)
        && three.err() == Some(MultiError::DeviceMix)
        && ghost.err() == Some(MultiError::NoDevice)
        && reg.queue(2).is_ok()
        && reg.queue(9).err() == Some(MultiError::Unknown);
    s.add("50135-J1-双设备登记", ok, if ok { "" } else { "设备号闭集失守" });

    // J1-2 ×10 复跑：报告逐字段一致、串扰累计为零
    let ten = match run_rounds(TEN_ROUNDS) {
        Ok(x) => x,
        Err(_) => {
            s.add("50135-J1-×10复跑", false, "十轮复跑失败");
            return s;
        }
    };
    let ok = ten.rounds == TEN_ROUNDS
        && ten.identical
        && ten.crosstalk == 0
        && ten.bytes == 2 * CHUNKS_PER_DEVICE as u32 * CHUNK_LEN as u32
        && ten.delivered == [CHUNKS_PER_DEVICE, CHUNKS_PER_DEVICE];
    let det = format!(
        "rounds={} same={} xtalk={} bytes={}",
        ten.rounds, ten.identical, ten.crosstalk, ten.bytes
    );
    s.add_dyn("50135-J1-×10零串扰", ok, &det);

    // 基准单轮报告（后续性质都以它为准）
    let rep = match run_scenario(0) {
        Ok(r) => r,
        Err(_) => return s,
    };

    // J1-3 零串扰逐设备：每设备交付块数/字节数与文档声明相等
    let want_chunks = CHUNKS_PER_DEVICE;
    let want_bytes = CHUNKS_PER_DEVICE as u32 * CHUNK_LEN as u32;
    let ok = rep.crosstalk == 0
        && rep.reorder == 0
        && rep.seqgap == 0
        && rep.delivered[0] == want_chunks
        && rep.delivered[1] == want_chunks
        && rep.per_device_bytes[0] == want_bytes
        && rep.per_device_bytes[1] == want_bytes;
    let det = format!(
        "d={:?} b={:?} xtalk={}",
        rep.delivered, rep.per_device_bytes, rep.crosstalk
    );
    s.add_dyn("50135-J1-逐设备勾稽", ok, &det);

    // J1-4 总量守恒：块 × 大小 == 字节账；事件数 == 块数
    let ok = rep.chunks == 2 * CHUNKS_PER_DEVICE
        && rep.bytes == 2 * want_bytes
        && rep.events == rep.chunks as usize
        && rep.devices == DEVICES;
    let det = format!("chunks={} bytes={} events={}", rep.chunks, rep.bytes, rep.events);
    s.add_dyn("50135-J1-总量守恒", ok, &det);

    // J1-5 设备内序：每设备序号从 0 连续递增（交错不破坏设备内顺序）
    let ok = rep.order_ok;
    s.add("50135-J1-设备内序", ok, if ok { "" } else { "设备内序号被交错打乱" });

    // J1-6 交替调度：双设备等量时交付序严格交替
    let ok = rep.alternating && rep.events == 16;
    s.add("50135-J1-交替调度", ok, if ok { "" } else { "调度未严格轮转" });

    // J1-7 无饥饿：两队列全部抽干（pending 归零）
    let mut led = MultiLedger::new();
    let _ = led.register(1);
    let _ = led.register(2);
    let _ = submit_document(&mut led, 0, CHUNK_LEN);
    let before = match (led.queue(1), led.queue(2)) {
        (Ok(a), Ok(b)) => (a.pending(), b.pending()),
        _ => return s,
    };
    let pumped = led.pump();
    let after = match (led.queue(1), led.queue(2)) {
        (Ok(a), Ok(b)) => (a.pending(), b.pending()),
        _ => return s,
    };
    let ok = before == (8, 8) && pumped == Ok(16) && after == (0, 0);
    let det = format!("before={:?} after={:?} pumped={:?}", before, after, pumped.ok());
    s.add_dyn("50135-J1-无饥饿", ok, &det);

    // J1-8 提交序无关：三种提交次序得到同一事件账
    let p0 = run_scenario(0);
    let p1 = run_scenario(1);
    let p2 = run_scenario(2);
    let ok = match (p0, p1, p2) {
        (Ok(a), Ok(b), Ok(c)) => {
            compare_rounds(&a, &b).is_ok()
                && compare_rounds(&a, &c).is_ok()
                && a.digest == c.digest
                && a.digest != 0
        }
        _ => false,
    };
    s.add("50135-J1-提交序无关", ok, if ok { "" } else { "提交次序影响交付账" });

    // J1-9 误投必拒：串扰块被拒且队列长度不变、计数 +1
    let mut xt = MultiLedger::new();
    let _ = xt.register(1);
    let _ = xt.register(2);
    let len_before = match xt.queue(1) {
        Ok(q) => q.queued(),
        Err(_) => return s,
    };
    let bad = xt.submit(1, Chunk::sealed(2, 0, CHUNK_LEN));
    let len_after = match xt.queue(1) {
        Ok(q) => q.queued(),
        Err(_) => return s,
    };
    let ok = bad.err() == Some(MultiError::Crosstalk)
        && xt.crosstalk() == 1
        && len_before == len_after;
    s.add("50135-J1-误投必拒", ok, if ok { "" } else { "误投块被收下" });

    // J1-10 乱序/跳号必拒：回退与跳号各有独立计数
    let mut seq = MultiLedger::new();
    let _ = seq.register(1);
    let _ = seq.register(2);
    let s0 = seq.submit(1, Chunk::sealed(1, 0, CHUNK_LEN));
    let back = seq.submit(1, Chunk::sealed(1, 0, CHUNK_LEN));
    let jump = seq.submit(1, Chunk::sealed(1, 4, CHUNK_LEN));
    let ok = s0.is_ok()
        && back.err() == Some(MultiError::Reorder)
        && jump.err() == Some(MultiError::SeqGap)
        && seq.reorder() == 1
        && seq.seqgap() == 1;
    s.add("50135-J1-乱序跳号", ok, if ok { "" } else { "序号纪律失守" });

    // J1-11 空块与坏块头必拒；容量满必拒
    let mut cap = MultiLedger::new();
    let _ = cap.register(1);
    let _ = cap.register(2);
    let empty = cap.submit(1, Chunk::sealed(1, 0, 0));
    let mut forged = Chunk::sealed(1, 0, CHUNK_LEN);
    forged.seq = 7;
    let crc_bad = cap.submit(1, forged);
    let mut filled = 0usize;
    let mut capacity_hit = false;
    let mut i = 0u16;
    while i < 40 {
        match cap.submit(1, Chunk::sealed(1, i, CHUNK_LEN)) {
            Ok(_) => filled += 1,
            Err(e) => {
                if e == MultiError::Capacity {
                    capacity_hit = true;
                }
                break;
            }
        }
        i += 1;
    }
    let ok = empty.err() == Some(MultiError::ChunkEmpty)
        && crc_bad.err() == Some(MultiError::ChunkCrc)
        && capacity_hit
        && filled < 40;
    let det = format!("filled={} capacity={}", filled, capacity_hit);
    s.add_dyn("50135-J1-坏块与容量", ok, &det);

    // J1-12 未交付不得封账：空账本 / 半途都必拒
    let empty_led = MultiLedger::new();
    let ed = empty_led.seal();
    let mut half = MultiLedger::new();
    let _ = half.register(1);
    let _ = half.register(2);
    let _ = submit_document(&mut half, 0, CHUNK_LEN);
    let hd = half.seal();
    let ok = ed.err() == Some(MultiError::EmptyDoc) && hd.err() == Some(MultiError::ShortDelivery);
    s.add("50135-J1-封账门槛", ok, if ok { "" } else { "未交付即封账" });

    // J1-13 轮数边界：0 与 17 必拒
    let z = run_rounds(0);
    let big = run_rounds(MAX_ROUNDS + 1);
    let ok = z.err() == Some(MultiError::Rounds) && big.err() == Some(MultiError::Rounds);
    s.add("50135-J1-轮数边界", ok, if ok { "" } else { "轮数越界被放行" });

    // J1-14 错误码闭集：14 个码互异且在段内
    let codes = [
        MultiError::NoDevice,
        MultiError::DupDevice,
        MultiError::Capacity,
        MultiError::EmptyDoc,
        MultiError::Crosstalk,
        MultiError::Reorder,
        MultiError::SeqGap,
        MultiError::ShortDelivery,
        MultiError::DeviceMix,
        MultiError::DigestDrift,
        MultiError::ChunkEmpty,
        MultiError::ChunkCrc,
        MultiError::Unknown,
        MultiError::Rounds,
    ];
    let mut distinct = true;
    let mut in_seg = true;
    let mut x = 0usize;
    while x < codes.len() {
        let cx = codes[x].code();
        if cx < SEG_BASE + 1 || cx > SEG_BASE + 14 {
            in_seg = false;
        }
        let mut y = x + 1;
        while y < codes.len() {
            if codes[y].code() == cx {
                distinct = false;
            }
            y += 1;
        }
        x += 1;
    }
    let ok = distinct && in_seg;
    s.add("50135-J1-错误码闭集", ok, if ok { "" } else { "码值重复或越段" });

    // J1-15 注入核：七类坏并发全抓
    let (caught, total) = injection_core();
    let ok = caught == total;
    let det = format!("注入 {}/{}", caught, total);
    s.add_dyn("50135-J1-注入必抓", ok, &det);

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx50135_checks_green() {
        let set = run_unx50135_checks();
        assert!(set.all_passed(), "UNX-F50135 red: {}", set.red_items_text());
    }

    #[test]
    fn unx50135_ten_rounds_zero_crosstalk() {
        let r = run_rounds(TEN_ROUNDS);
        assert!(r.is_ok());
        if let Ok(x) = r {
            assert_eq!(x.rounds, 10);
            assert!(x.identical);
            assert_eq!(x.crosstalk, 0);
            assert_eq!(x.bytes, 8192);
        }
    }

    #[test]
    fn unx50135_crosstalk_is_refused() {
        let mut led = MultiLedger::new();
        assert!(led.register(1).is_ok());
        assert!(led.register(2).is_ok());
        let bad = led.submit(1, Chunk::sealed(2, 0, CHUNK_LEN));
        assert_eq!(bad.err(), Some(MultiError::Crosstalk));
        assert_eq!(led.crosstalk(), 1);
    }

    #[test]
    fn unx50135_device_order_survives_interleave() {
        let rep = run_scenario(1);
        assert!(rep.is_ok());
        if let Ok(r) = rep {
            assert!(r.alternating);
            assert!(r.order_ok);
            assert_eq!(r.delivered, [8, 8]);
        }
    }
}
