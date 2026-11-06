//! UNX-F50116 直通性能账（大文件零整载流式透传）（0x43aa0 段）
//!
//! 锚点：UNX-F50116-J1 100MB 级流式内存峰值恒定断言；判据锚定 §AI-63 §5。
//!
//! 直通大文件不允许「先读全再转发」：一旦整载，内存峰值随文件大小线性上涨。
//! 本模块把透传建模成**定长环 + 分块游标**：每个块按索引现场生成（不分配
//! 载荷），环里最多驻留 `ring` 个块元数据；因此峰值只与 `chunk × ring` 有关，
//! 与 `total` 无关。100 MiB 与 1 MiB 在相同参数下峰值必须逐位相等——这就是
//! J1 的可断言形式。
//!
//! 结构清单：
//! * 流规格 `StreamSpec`（总字节 / 块大小 / 环深）与块元数据 `ChunkMeta`；
//! * 定容块环 `Ring`（8 槽 Option 数组，满则背压，空则 Empty，不扩容）；
//! * 泵状态机 `Pump`：Idle → Streaming → Done；push/pop/step/run，
//!   背压、空环、终态后操作都显式报错；
//! * 性能账 `PerfLedger`：容量 16，序号单调，拒绝乱序与不自洽样本；
//! * 注入核：规格坏样本 6 类 + 泵坏样本 3 类 + 账坏样本 3 类，共 12 类。
//!
//! 错误纪律：诊断码闭集 `0x43aa0 + 1..=11`；内核路径零 panic——环槽访问
//! 先 `get`/`get_mut`，除零风险（`chunk` 为 0）在构造期即被拒绝。

use crate::alloc::format;
use crate::alloc::string::ToString;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段（0x43aa0 独占 0x20）。
pub const SEG_BASE: u32 = 0x43aa0;

pub const E_ZERO: u16 = 1;
pub const E_CHUNK: u16 = 2;
pub const E_RING: u16 = 3;
pub const E_STATE: u16 = 4;
pub const E_BACKPRESSURE: u16 = 5;
pub const E_ORDER: u16 = 6;
pub const E_CAPACITY: u16 = 7;
pub const E_RANGE: u16 = 8;
pub const E_EMPTY: u16 = 9;

/// 性能账错误。`code()` 返回 `SEG_BASE + n`。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PerfErr {
    Zero,
    Chunk,
    Ring,
    State,
    Backpressure,
    Order,
    Capacity,
    Range,
    Empty,
}

impl PerfErr {
    /// 诊断码（段基址 + 闭集内偏移）。
    pub fn code(self) -> u32 {
        let n = match self {
            PerfErr::Zero => E_ZERO,
            PerfErr::Chunk => E_CHUNK,
            PerfErr::Ring => E_RING,
            PerfErr::State => E_STATE,
            PerfErr::Backpressure => E_BACKPRESSURE,
            PerfErr::Order => E_ORDER,
            PerfErr::Capacity => E_CAPACITY,
            PerfErr::Range => E_RANGE,
            PerfErr::Empty => E_EMPTY,
        };
        SEG_BASE + n as u32
    }

    /// 稳定文案。
    pub fn message(self) -> &'static str {
        match self {
            PerfErr::Zero => "total byte count is zero",
            PerfErr::Chunk => "chunk size outside 4KiB..=1MiB",
            PerfErr::Ring => "ring depth outside 2..=8",
            PerfErr::State => "pump operation illegal in current state",
            PerfErr::Backpressure => "ring is full",
            PerfErr::Order => "sample stamp is not monotonic",
            PerfErr::Capacity => "performance ledger full",
            PerfErr::Range => "value outside legal range",
            PerfErr::Empty => "ring is empty",
        }
    }
}

const FNV_BASIS: u64 = 0xcbf2_9ce4_8422_2325;
const FNV_PRIME: u64 = 0x0000_0100_0000_01b3;

fn fnv_u64(h: u64, x: u64) -> u64 {
    let mut acc = h;
    let mut shift = 0u32;
    while shift < 64 {
        let b = ((x >> shift) & 0xff) as u8;
        acc ^= b as u64;
        acc = acc.wrapping_mul(FNV_PRIME);
        shift += 8;
    }
    acc
}

/// 环槽数上限（与 `Ring` 定容数组一致）。
pub const RING_MAX: usize = 8;
/// 单块下限 4 KiB：再小则每字节的泵调度开销压过透传本体的意义。
pub const CHUNK_MIN: u32 = 4096;
/// 单块上限 1 MiB：块越大越省调度，但峰值随块 линейно上涨。
pub const CHUNK_MAX: u32 = 1 << 20;
/// 总长上限 1 TiB（超过属于归档迁移，不走打印直通）。
pub const TOTAL_MAX: u64 = 1u64 << 40;

/// 流规格。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct StreamSpec {
    pub total: u64,
    pub chunk: u32,
    pub ring: u8,
}

impl StreamSpec {
    /// 构造并校验。
    pub fn new(total: u64, chunk: u32, ring: u8) -> Result<StreamSpec, PerfErr> {
        let s = StreamSpec { total, chunk, ring };
        s.validate()?;
        Ok(s)
    }

    /// 校验顺序：total → chunk → ring → 总长上限。除零因此不可能发生。
    pub fn validate(&self) -> Result<(), PerfErr> {
        if self.total == 0 {
            return Err(PerfErr::Zero);
        }
        if self.chunk < CHUNK_MIN || self.chunk > CHUNK_MAX {
            return Err(PerfErr::Chunk);
        }
        if self.ring < 2 || self.ring as usize > RING_MAX {
            return Err(PerfErr::Ring);
        }
        if self.total > TOTAL_MAX {
            return Err(PerfErr::Range);
        }
        Ok(())
    }

    /// 块数 = ceil(total / chunk)。
    pub fn chunks(&self) -> u32 {
        let c = self.chunk as u64;
        ((self.total + c - 1) / c) as u32
    }

    /// 第 index 块的字节数（尾块可以短于 chunk）。
    pub fn chunk_len(&self, index: u32) -> u32 {
        let off = index as u64 * self.chunk as u64;
        if off >= self.total {
            return 0;
        }
        let left = self.total - off;
        if left > self.chunk as u64 {
            self.chunk
        } else {
            left as u32
        }
    }

    /// 最坏驻留字节 = chunk × ring（与 total 无关，这是 J1 的核心量）。
    pub fn peak_budget(&self) -> u64 {
        self.chunk as u64 * self.ring as u64
    }
}

/// 块元数据：索引 + 字节数 + 内容摘要。载荷本身不驻留。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ChunkMeta {
    pub index: u32,
    pub len: u32,
    pub digest: u64,
}

/// 现场生成第 x 个字节（线性同余 + 右移混合，无随机源、无时钟）。
fn stream_byte(x: u64) -> u8 {
    ((x.wrapping_mul(0x9E37_79B1)) ^ (x >> 7)) as u8
}

/// 计算第 index 块摘要：逐字节扫描该块（只在元数据里落 64 位摘要）。
pub fn chunk_digest(spec: &StreamSpec, index: u32) -> u64 {
    let len = spec.chunk_len(index) as u64;
    let off = index as u64 * spec.chunk as u64;
    let mut h = FNV_BASIS;
    let mut i = 0u64;
    while i < len {
        h ^= stream_byte(off + i) as u64;
        h = h.wrapping_mul(FNV_PRIME);
        i += 1;
    }
    h
}

/// 泵状态。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PumpState {
    Idle,
    Streaming,
    Done,
}

impl PumpState {
    /// 稳定短名。
    pub fn label(self) -> &'static str {
        match self {
            PumpState::Idle => "idle",
            PumpState::Streaming => "streaming",
            PumpState::Done => "done",
        }
    }
}

/// 定容块环：队列语义（head + len），满则背压，不扩容。
#[derive(Clone, Debug)]
pub struct Ring {
    slots: [Option<ChunkMeta>; RING_MAX],
    head: usize,
    len: usize,
    cap: usize,
}

impl Ring {
    /// 按环深建环（环深已在 `StreamSpec` 校验过，这里再夹一次上限）。
    pub fn new(depth: u8) -> Ring {
        let cap = if depth as usize > RING_MAX {
            RING_MAX
        } else if depth < 2 {
            2
        } else {
            depth as usize
        };
        Ring {
            slots: [None; RING_MAX],
            head: 0,
            len: 0,
            cap,
        }
    }

    /// 当前驻留块数。
    pub fn len(&self) -> usize {
        self.len
    }

    /// 环是否为空。
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// 环是否已满（满时再推即背压）。
    pub fn is_full(&self) -> bool {
        self.len >= self.cap
    }

    /// 容量。
    pub fn cap(&self) -> usize {
        self.cap
    }

    /// 入环：满则 `Backpressure`，成功返回新驻留数。
    pub fn push(&mut self, m: ChunkMeta) -> Result<usize, PerfErr> {
        if self.is_full() {
            return Err(PerfErr::Backpressure);
        }
        let idx = (self.head + self.len) % RING_MAX;
        match self.slots.get_mut(idx) {
            Some(cell) => {
                *cell = Some(m);
                self.len += 1;
                Ok(self.len)
            }
            None => Err(PerfErr::Range),
        }
    }

    /// 出环：空则 `Empty`。
    pub fn pop(&mut self) -> Result<ChunkMeta, PerfErr> {
        if self.len == 0 {
            return Err(PerfErr::Empty);
        }
        let idx = self.head % RING_MAX;
        let got = match self.slots.get_mut(idx) {
            Some(cell) => cell.take(),
            None => None,
        };
        match got {
            Some(m) => {
                self.head = (self.head + 1) % RING_MAX;
                self.len -= 1;
                Ok(m)
            }
            None => Err(PerfErr::Empty),
        }
    }
}

/// 泵统计（一次完整流式透传的可审计快照）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PumpStats {
    pub total: u64,
    pub chunks: u32,
    pub peak_held: usize,
    pub peak_bytes: u64,
    pub bytes: u64,
    pub digest: u64,
    pub ticks: u64,
}

impl PumpStats {
    /// 自洽性：字节守恒、块数守恒、峰值不超预算。
    pub fn consistent(&self, spec: &StreamSpec) -> bool {
        self.total == spec.total
            && self.chunks == spec.chunks()
            && self.bytes == self.total
            && self.peak_held <= spec.ring as usize
            && self.peak_bytes == self.peak_held as u64 * spec.chunk as u64
            && self.peak_bytes <= spec.peak_budget()
            && self.ticks > 0
    }
}

/// 流式透传泵。
#[derive(Clone, Debug)]
pub struct Pump {
    spec: StreamSpec,
    ring: Ring,
    state: PumpState,
    next_index: u32,
    bytes_read: u64,
    bytes_folded: u64,
    digest: u64,
    peak_held: usize,
    ticks: u64,
}

impl Pump {
    /// 建泵（Idle，未消费任何块）。
    pub fn new(spec: StreamSpec) -> Result<Pump, PerfErr> {
        spec.validate()?;
        Ok(Pump {
            spec,
            ring: Ring::new(spec.ring),
            state: PumpState::Idle,
            next_index: 0,
            bytes_read: 0,
            bytes_folded: 0,
            digest: FNV_BASIS,
            peak_held: 0,
            ticks: 0,
        })
    }

    /// 当前状态。
    pub fn state(&self) -> PumpState {
        self.state
    }

    /// 环当前驻留数。
    pub fn held(&self) -> usize {
        self.ring.len()
    }

    /// 规格。
    pub fn spec(&self) -> StreamSpec {
        self.spec
    }

    /// 开流：只允许 Idle → Streaming。
    pub fn begin(&mut self) -> Result<(), PerfErr> {
        if self.state != PumpState::Idle {
            return Err(PerfErr::State);
        }
        self.state = PumpState::Streaming;
        Ok(())
    }

    /// 生产一块入环（满则 Backpressure，不覆盖、不丢块）。
    pub fn push(&mut self) -> Result<usize, PerfErr> {
        if self.state != PumpState::Streaming {
            return Err(PerfErr::State);
        }
        if self.next_index >= self.spec.chunks() {
            return Err(PerfErr::State);
        }
        let idx = self.next_index;
        let meta = ChunkMeta {
            index: idx,
            len: self.spec.chunk_len(idx),
            digest: chunk_digest(&self.spec, idx),
        };
        let n = self.ring.push(meta)?;
        self.next_index += 1;
        self.bytes_read += meta.len as u64;
        if self.ring.len() > self.peak_held {
            self.peak_held = self.ring.len();
        }
        Ok(n)
    }

    /// 消费一块：折叠摘要与字节数（这是「落盘/转发」的替身）。
    pub fn pop(&mut self) -> Result<ChunkMeta, PerfErr> {
        if self.state != PumpState::Streaming && self.state != PumpState::Done {
            return Err(PerfErr::State);
        }
        let m = self.ring.pop()?;
        self.digest = fnv_u64(self.digest, m.digest);
        self.bytes_folded += m.len as u64;
        Ok(m)
    }

    /// 走一步：环未满先生产，满了先消费；收尾后转 Done。
    pub fn step(&mut self) -> Result<(), PerfErr> {
        if self.state != PumpState::Streaming {
            return Err(PerfErr::State);
        }
        let n = self.spec.chunks();
        if self.next_index >= n && self.ring.is_empty() {
            self.state = PumpState::Done;
            self.ticks = self.ticks.wrapping_add(1);
            return Ok(());
        }
        if self.ring.is_full() {
            let _ = self.pop()?;
        } else if self.next_index < n {
            let _ = self.push()?;
        } else {
            let _ = self.pop()?;
        }
        self.ticks = self.ticks.wrapping_add(1);
        Ok(())
    }

    /// 跑完整条流（从 Idle 起）。
    pub fn run(&mut self) -> Result<PumpStats, PerfErr> {
        if self.state != PumpState::Idle {
            return Err(PerfErr::State);
        }
        self.begin()?;
        // 上界：每块至少两步（入+出）+ 终态一步；超出即状态异常，显式报错。
        let budget = self.spec.chunks() as u64 * 2 + 4;
        while self.state != PumpState::Done {
            if self.ticks > budget {
                return Err(PerfErr::State);
            }
            self.step()?;
        }
        Ok(self.stats())
    }

    /// 统计快照（任何状态可取，但只有 Done 后 `consistent` 才应为真）。
    pub fn stats(&self) -> PumpStats {
        PumpStats {
            total: self.spec.total,
            chunks: self.spec.chunks(),
            peak_held: self.peak_held,
            peak_bytes: self.peak_held as u64 * self.spec.chunk as u64,
            bytes: self.bytes_folded,
            digest: self.digest,
            ticks: self.ticks,
        }
    }
}

/// 一条性能样本（账本条目）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PerfSample {
    pub spec: StreamSpec,
    pub chunks: u32,
    pub peak_held: usize,
    pub peak_bytes: u64,
    pub bytes: u64,
    pub digest: u64,
    pub ticks: u64,
}

impl PerfSample {
    /// 由统计快照落样本；调用方保证统计已完成（Done）。
    pub fn from_stats(spec: StreamSpec, st: &PumpStats) -> PerfSample {
        PerfSample {
            spec,
            chunks: st.chunks,
            peak_held: st.peak_held,
            peak_bytes: st.peak_bytes,
            bytes: st.bytes,
            digest: st.digest,
            ticks: st.ticks,
        }
    }

    /// 自洽性：字节守恒 + 块数守恒 + 峰值不超预算。
    pub fn consistent(&self) -> bool {
        self.spec.validate().is_ok()
            && self.chunks == self.spec.chunks()
            && self.bytes == self.spec.total
            && self.peak_held <= self.spec.ring as usize
            && self.peak_bytes == self.peak_held as u64 * self.spec.chunk as u64
            && self.peak_bytes <= self.spec.peak_budget()
            && self.ticks > 0
    }

    /// 样本摘要（账根折叠单元）。
    pub fn digest_entry(&self) -> u64 {
        let mut h = fnv_u64(FNV_BASIS, self.spec.total);
        h = fnv_u64(h, self.spec.chunk as u64);
        h = fnv_u64(h, self.spec.ring as u64);
        h = fnv_u64(h, self.peak_bytes);
        h = fnv_u64(h, self.digest);
        fnv_u64(h, self.ticks)
    }
}

/// 性能账容量：16 条覆盖 1MiB..100MiB × 两种块 × 两种环深的关键组合。
pub const PERF_CAP: usize = 16;

/// 性能账：序号单调，峰值累计取最大。
#[derive(Clone, Debug)]
pub struct PerfLedger {
    samples: Vec<PerfSample>,
    root: u64,
    seq: u32,
    peak: u64,
}

impl PerfLedger {
    /// 空账。
    pub fn new() -> PerfLedger {
        PerfLedger {
            samples: Vec::with_capacity(PERF_CAP),
            root: FNV_BASIS,
            seq: 0,
            peak: 0,
        }
    }

    /// 已落账条数。
    pub fn len(&self) -> usize {
        self.samples.len()
    }

    /// 是否为空。
    pub fn is_empty(&self) -> bool {
        self.samples.is_empty()
    }

    /// 账根。
    pub fn root(&self) -> u64 {
        self.root
    }

    /// 历史最大峰值字节（账的单调量）。
    pub fn peak(&self) -> u64 {
        self.peak
    }

    /// 只读样本切片。
    pub fn samples(&self) -> &[PerfSample] {
        &self.samples
    }

    /// 落账：自洽 → 序号（必须等于账当前序号）→ 容量。
    /// 拒绝路径不改根、不改峰值。
    pub fn record(&mut self, s: PerfSample, stamp: u32) -> Result<u64, PerfErr> {
        if !s.consistent() {
            return Err(PerfErr::Range);
        }
        if stamp != self.seq {
            return Err(PerfErr::Order);
        }
        if self.samples.len() >= PERF_CAP {
            return Err(PerfErr::Capacity);
        }
        let d = s.digest_entry();
        self.samples.push(s);
        self.root = fnv_u64(self.root, d);
        if s.peak_bytes > self.peak {
            self.peak = s.peak_bytes;
        }
        self.seq = self.seq.wrapping_add(1);
        Ok(d)
    }
}

/// 一次性跑完并返回统计（判据与外部调用共用）。
pub fn pump_run(spec: StreamSpec) -> Result<PumpStats, PerfErr> {
    let mut p = Pump::new(spec)?;
    p.run()
}

const MIB: u64 = 1024 * 1024;

fn spec_of(total: u64, chunk: u32, ring: u8) -> StreamSpec {
    match StreamSpec::new(total, chunk, ring) {
        Ok(s) => s,
        Err(_) => StreamSpec {
            total: 4096,
            chunk: 4096,
            ring: 2,
        },
    }
}

fn sample_of(total: u64, chunk: u32, ring: u8) -> Result<PerfSample, PerfErr> {
    let spec = StreamSpec::new(total, chunk, ring)?;
    let st = pump_run(spec)?;
    Ok(PerfSample::from_stats(spec, &st))
}

/// 注入核：12 类坏样本，逐类断言具体诊断码。
pub fn injection_core() -> (usize, usize) {
    let mut caught = 0usize;
    let total = 12usize;

    // 1 total 0 → Zero
    if StreamSpec::new(0, 4096, 4).err() == Some(PerfErr::Zero) {
        caught += 1;
    }
    // 2 块过小 → Chunk
    if StreamSpec::new(MIB, 1024, 4).err() == Some(PerfErr::Chunk) {
        caught += 1;
    }
    // 3 块过大 → Chunk
    if StreamSpec::new(MIB, (1 << 20) + 4096, 4).err() == Some(PerfErr::Chunk) {
        caught += 1;
    }
    // 4 环深 1 → Ring
    if StreamSpec::new(MIB, 4096, 1).err() == Some(PerfErr::Ring) {
        caught += 1;
    }
    // 5 环深 9 → Ring
    if StreamSpec::new(MIB, 4096, 9).err() == Some(PerfErr::Ring) {
        caught += 1;
    }
    // 6 总长超 1TiB → Range
    if StreamSpec::new(TOTAL_MAX + 1, 4096, 4).err() == Some(PerfErr::Range) {
        caught += 1;
    }
    // 7 Idle 上 push → State
    let mut p = match Pump::new(spec_of(MIB, 4096, 4)) {
        Ok(v) => v,
        Err(_) => return (caught, total),
    };
    if p.push().err() == Some(PerfErr::State) {
        caught += 1;
    }
    // 8 开流后空环 pop → Empty
    let _ = p.begin();
    if p.pop().err() == Some(PerfErr::Empty) {
        caught += 1;
    }
    // 9 环满再推 → Backpressure
    let mut full = match Pump::new(spec_of(64 * 4096, 4096, 2)) {
        Ok(v) => v,
        Err(_) => return (caught, total),
    };
    let _ = full.begin();
    let _ = full.push();
    let _ = full.push();
    if full.push().err() == Some(PerfErr::Backpressure) {
        caught += 1;
    }
    // 10 账本序号乱序 → Order
    let mut ledger = PerfLedger::new();
    let good = match sample_of(4096 * 8, 4096, 2) {
        Ok(v) => v,
        Err(_) => return (caught, total),
    };
    if ledger.record(good, 0).is_ok() && ledger.record(good, 0).err() == Some(PerfErr::Order) {
        caught += 1;
    }
    // 11 峰值超环预算（伪造样本）→ Range
    let mut forged = good;
    forged.peak_held = forged.spec.ring as usize + 1;
    forged.peak_bytes = forged.peak_held as u64 * forged.spec.chunk as u64;
    let mut l2 = PerfLedger::new();
    if l2.record(forged, 0).err() == Some(PerfErr::Range) {
        caught += 1;
    }
    // 12 容量 16 后第 17 条 → Capacity
    let mut cap = PerfLedger::new();
    let small = match sample_of(4096 * 4, 4096, 2) {
        Ok(v) => v,
        Err(_) => return (caught, total),
    };
    let mut i = 0u32;
    let mut acc = 0usize;
    while i < PERF_CAP as u32 {
        if cap.record(small, i).is_ok() {
            acc += 1;
        }
        i += 1;
    }
    if acc == PERF_CAP && cap.record(small, PERF_CAP as u32).err() == Some(PerfErr::Capacity) {
        caught += 1;
    }
    (caught, total)
}

/// J1 判据：100MB 级流式内存峰值恒定（13 条）。
pub fn run_unx50116_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F50116");

    // 100 MiB / 64 KiB 块 / 环深 4：峰值预算 = 256 KiB。
    let big_spec = spec_of(100 * MIB, 64 * 1024, 4);
    let small_spec = spec_of(MIB, 64 * 1024, 4);
    let big = pump_run(big_spec);
    let small = pump_run(small_spec);

    // J1-1 峰值恒定：100 MiB 与 1 MiB 峰值逐位相等，且远小于文件长度。
    let ok = match (&big, &small) {
        (Ok(b), Ok(p)) => {
            b.peak_bytes == p.peak_bytes
                && b.peak_bytes == big_spec.peak_budget()
                && b.peak_bytes < b.total
                && p.peak_bytes < p.total
        }
        _ => false,
    };
    let det = match &big {
        Ok(b) => format!("peak={} total={}", b.peak_bytes, b.total),
        Err(_) => "big run failed".to_string(),
    };
    s.add_dyn("50116-J1-峰值恒定", ok, &det);

    // J1-2 字节守恒：折叠字节数 == 总长，块数 == ceil。
    let ok = match &big {
        Ok(b) => b.bytes == big_spec.total && b.chunks == big_spec.chunks() && b.consistent(&big_spec),
        Err(_) => false,
    };
    let det = match &big {
        Ok(b) => format!("folded={}/{}", b.bytes, big_spec.total),
        Err(_) => "big run failed".to_string(),
    };
    s.add_dyn("50116-J1-字节守恒", ok, &det);

    // J1-3 尾块长度精确：10 块 + 123 字节 → 第 11 块 123 字节。
    let tail_spec = spec_of(10 * 4096 + 123, 4096, 3);
    let ok = tail_spec.chunks() == 11 && tail_spec.chunk_len(10) == 123 && tail_spec.chunk_len(11) == 0;
    let det = format!("chunks={} tail={}", tail_spec.chunks(), tail_spec.chunk_len(10));
    s.add_dyn("50116-J1-尾块精确", ok, &det);

    // J1-4 环容量：驻留峰值正好等于环深（块数 ≥ 环深时），永不越界。
    let ok = match &big {
        Ok(b) => b.peak_held == big_spec.ring as usize && b.peak_held <= RING_MAX,
        Err(_) => false,
    };
    let det = match &big {
        Ok(b) => format!("peak_held={}", b.peak_held),
        Err(_) => "big run failed".to_string(),
    };
    s.add_dyn("50116-J1-环容量", ok, &det);

    // J1-5 复跑确定：同规格两次运行摘要、步数逐位一致。
    let a = pump_run(spec_of(2 * MIB, 32 * 1024, 3));
    let b = pump_run(spec_of(2 * MIB, 32 * 1024, 3));
    let ok = match (&a, &b) {
        (Ok(x), Ok(y)) => x.digest == y.digest && x.ticks == y.ticks && x.bytes == y.bytes,
        _ => false,
    };
    let det = match &a {
        Ok(x) => format!("digest={:016x} ticks={}", x.digest, x.ticks),
        Err(_) => "run failed".to_string(),
    };
    s.add_dyn("50116-J1-复跑确定", ok, &det);

    // J1-6 长度敏感：1 MiB 与 2 MiB 摘要不同（不是常量假实现）。
    let one = pump_run(spec_of(MIB, 32 * 1024, 3));
    let two = pump_run(spec_of(2 * MIB, 32 * 1024, 3));
    let ok = match (&one, &two) {
        (Ok(x), Ok(y)) => x.digest != y.digest,
        _ => false,
    };
    let det = match (&one, &two) {
        (Ok(x), Ok(y)) => format!("d1={:016x} d2={:016x}", x.digest, y.digest),
        _ => "run failed".to_string(),
    };
    s.add_dyn("50116-J1-长度敏感", ok, &det);

    // J1-7 背压纪律：环满拒绝、出环后恢复，且不丢块。
    let mut p = match Pump::new(spec_of(8 * 4096, 4096, 2)) {
        Ok(v) => v,
        Err(_) => return s,
    };
    let _ = p.begin();
    let r1 = p.push();
    let r2 = p.push();
    let r3 = p.push();
    let popped = p.pop();
    let r4 = p.push();
    let ok = r1.is_ok()
        && r2.is_ok()
        && r3.err() == Some(PerfErr::Backpressure)
        && popped.is_ok()
        && r4.is_ok()
        && p.held() == 2;
    let det = format!("held={} full_code={:x}", p.held(), r3.err().map(|e| e.code()).unwrap_or(0));
    s.add_dyn("50116-J1-背压纪律", ok, &det);

    // J1-8 状态机：Idle 上 step/pop 均 State；Done 后 push State。
    let mut q = match Pump::new(spec_of(4 * 4096, 4096, 2)) {
        Ok(v) => v,
        Err(_) => return s,
    };
    let st1 = q.step();
    let st2 = q.pop();
    let done = q.run();
    let after = q.push();
    let ok = st1.err() == Some(PerfErr::State)
        && st2.err() == Some(PerfErr::State)
        && done.is_ok()
        && q.state() == PumpState::Done
        && after.err() == Some(PerfErr::State);
    let det = format!("state={} code={:x}", q.state().label(), after.err().map(|e| e.code()).unwrap_or(0));
    s.add_dyn("50116-J1-状态机", ok, &det);

    // J1-9 账本序号单调：顺序落账成功，重复序号 Order 拒绝且根不变。
    let mut l = PerfLedger::new();
    let s1 = sample_of(4096 * 6, 4096, 2);
    let s2 = sample_of(4096 * 8, 4096, 3);
    let mut ok = false;
    if let (Ok(x), Ok(y)) = (&s1, &s2) {
        let a1 = l.record(*x, 0);
        let a2 = l.record(*y, 1);
        let root_before = l.root();
        let bad = l.record(*y, 1);
        ok = a1.is_ok()
            && a2.is_ok()
            && bad.err() == Some(PerfErr::Order)
            && l.root() == root_before
            && l.len() == 2;
    }
    let det = format!("len={} root={:016x}", l.len(), l.root());
    s.add_dyn("50116-J1-账本序号", ok, &det);

    // J1-10 账本峰值 = 样本峰值最大值（单调取大）。
    let ok = match (s1, s2) {
        (Ok(x), Ok(y)) => {
            let mut g = PerfLedger::new();
            let r1 = g.record(x, 0);
            let r2 = g.record(y, 1);
            let want = if x.peak_bytes > y.peak_bytes {
                x.peak_bytes
            } else {
                y.peak_bytes
            };
            r1.is_ok() && r2.is_ok() && g.peak() == want
        }
        _ => false,
    };
    let det = format!("peak={}", l.peak());
    s.add_dyn("50116-J1-峰值取大", ok, &det);

    // J1-11 峰值与总长解耦：同块同环下，1MiB/8MiB/64MiB 三者峰值相等。
    let mut peaks = [0u64; 3];
    let mut ok = true;
    let sizes = [MIB, 8 * MIB, 64 * MIB];
    let mut i = 0usize;
    while i < 3 {
        let sp = match sizes.get(i) {
            Some(t) => spec_of(*t, 64 * 1024, 3),
            None => big_spec,
        };
        match pump_run(sp) {
            Ok(st) => {
                if let Some(cell) = peaks.get_mut(i) {
                    *cell = st.peak_bytes;
                }
            }
            Err(_) => ok = false,
        }
        i += 1;
    }
    ok = ok && peaks[0] == peaks[1] && peaks[1] == peaks[2] && peaks[0] == 3 * 64 * 1024;
    let det = format!("p={}/{}/{}", peaks[0], peaks[1], peaks[2]);
    s.add_dyn("50116-J1-峰值解耦", ok, &det);

    // J1-12 大文件仍守恒：100MiB 记录进账后 bytes == 100MiB 且自洽。
    let mut bigled = PerfLedger::new();
    let ok = match &big {
        Ok(b) => {
            let smp = PerfSample::from_stats(big_spec, b);
            bigled.record(smp, 0).is_ok()
                && bigled.len() == 1
                && smp.bytes == 100 * MIB
                && smp.consistent()
        }
        Err(_) => false,
    };
    let det = match &big {
        Ok(b) => format!("bytes={} peak={}", b.bytes, b.peak_bytes),
        Err(_) => "big run failed".to_string(),
    };
    s.add_dyn("50116-J1-大账守恒", ok, &det);

    // J1-13 注入核 12/12。
    let (caught, total) = injection_core();
    let ok = caught == total;
    let det = format!("注入 {}/{}", caught, total);
    s.add_dyn("50116-J1-注入必抓", ok, &det);

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx50116_checks_green() {
        let set = run_unx50116_checks();
        assert!(set.all_passed(), "UNX-F50116 red: {}", set.red_items_text());
    }

    #[test]
    fn unx50116_peak_constant() {
        let a = pump_run(spec_of(4 * MIB, 64 * 1024, 4));
        let b = pump_run(spec_of(16 * MIB, 64 * 1024, 4));
        match (a, b) {
            (Ok(x), Ok(y)) => assert_eq!(x.peak_bytes, y.peak_bytes),
            _ => panic!("run failed"),
        }
    }

    #[test]
    fn unx50116_stream_conserves_bytes() {
        let spec = spec_of(10 * 4096 + 7, 4096, 3);
        match pump_run(spec) {
            Ok(st) => {
                assert_eq!(st.bytes, spec.total);
                assert_eq!(st.chunks, spec.chunks());
                assert!(st.consistent(&spec));
            }
            Err(_) => panic!("run failed"),
        }
    }
}
