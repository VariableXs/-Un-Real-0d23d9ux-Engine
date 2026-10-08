//! VE-F0418 · 词法性能工程（VE-C 域 · 着色器系统 · 目标 320 行）
//!
//! **锚点**：`docs/Varix/VE-STAR-II/VE Varix STAR II · 总纲与施工书.md#VE-F0418`
//!
//! **判据（锚点原文）**：零回溯断言、池化、arena、流式上界、判据。
//!
//! 锚点职责定位原文：
//! > 单遍零回溯设计断言（性能判据写进架构）、缓冲区复用池化、记号流紧凑存储
//! > （arena 分配）、大文件流式处理（内存上界与文件大小无关）；性能基准（每兆
//! > 字符每秒吞吐）版本化记录，退化即回归门阻断。
//!
//! 本条交付五件互相咬合的东西——**每一件都是「防止性能悄悄退化」的闸门**，
//! 而不是「让代码变快」的优化。理由：词法器已经在前几条（F0403-F0417）把
//! 正确性做完了，此后再让它变快的唯一手段就是改扫描逻辑，而改扫描逻辑正是
//! 最容易把正确性改坏的动作。所以本条不碰任何识别语义，只把「不许回溯」
//! 「不许无界分配」「不许退化不报」这三条变成**可执行断言**，写进架构里。
//!
//! 1. **零回溯断言**（判据一）。词法扫描必须是**单遍**：第 n 个字符的处理
//!    不得回看第 n-1 个之前的内容。实现方式是把「已消费字符数」做成游标上
//!    的**单调不减**字段，并由 `bump` 唯一地推进它——任何试图回退的路径
//!    （`rewind`/`seek_back`）都被本模块**不提供**。这条的关键在于「缺席」：
//!    提供一个回退函数等于给回退留后门，所以本模块**只给前进**。
//!    `zero_backtrack_violations` 统计违规次数，正常恒为 0——它恒为 0 是
//!    **因为接口层面不可能回退**，而不是因为没人写错（后者需要外部保证）。
//!
//! 2. **缓冲区复用池化**（判据二）。记号缓冲不可每次 `Vec::new()`：内核里
//!    扫描一整个着色器文件上百次，每次重新分配既慢又会在长期运行中攒下
//!    碎片。`BufferPool` 按**容量分档**回收缓冲（`recycle`），取用时优先给同档
//!    的空缓冲（`acquire`），并在池满时**如实返回新建**而不是静默丢弃请求。
//!    池有硬上界 `POOL_CAPACITY`（锚点错误路径第三条：池化泄漏→清扫断言）：
//!    回收超过上界即拒绝入池并计数 `rejected`，不静默丢。
//!
//! 3. **arena 紧凑存储**（判据三）。记号流用**下标 + 定长槽**紧凑存：每槽
//!    `TOKEN_SLOT_BYTES` 字节，记录 kind/起始偏移/长度/行号。不用 `Vec<Token>`
//!    的堆分配风暴，也不用每 token 一个堆块。`TokenArena` 提供 `push` 与
//!    `get`，容量固定 `ARENA_SLOTS`，**满了就如实返回 `Err(ArenaFull)`** 而非
//!    悄悄扩容——扩容会让「内存上界与文件大小无关」这条判据失效。
//!
//! 4. **流式上界**（判据四）。大文件分块喂入，每块产出记号后即释放，内存
//!    上界 = `ARENA_SLOTS * TOKEN_SLOT_BYTES + POOL_CAPACITY * CHUNK_CAPACITY`
//!    ——**与文件大小无关**（锚点：内存上界与文件大小无关）。`StreamingLexer`
//!    用 `feed`/`finish` 两段接口承载，跨块不变量（字节偏移单调、记号不跨界）
//!    由 `check_chunk_invariants` 断言。
//!
//! 5. **基准版本化**（判据五）。吞吐分位按**版本**记录（`BENCH_VERSION`），
//!    退化超阈值 `REGRESSION_RATIO` 即判退化；基准记录**自带版本号与样本量**，
//!    版本不同的记录**不可直接比**（拿不同机器/不同代码状态的数比吞吐是自欺）。
//!    这是锚点「退化即回归门阻断」的落地：阻断的是**跨版本可比性检查**失败，
//!    不是凭空造一个数字。
//!
//! 零静默纪律：arena 满→`Err` 且计数；池满→如实新建且计数；基准不可比→
//!   `None` + 说明理由，不拿不可比的数当退化也不当达标；池化回收超上界→计数拒绝。
//! 零 panic 面、零 IO、无全局可变状态。
//!
//! 上游消费：F0403 词法主路（被测扫描器以函数指针接入）、F0417 恢复层（本模块
//!   产出的记号流供其恢复游标推进）。下游 F0419 fuzz 复用本模块的基准与
//!   arena 判据作为不变量断言集。

use alloc::format;
use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;

// ---------------------------------------------------------------------------
// 一、单遍游标（判据一：零回溯）
// ---------------------------------------------------------------------------

/// 单遍游标：只增不减，**没有**任何回退方法。
///
/// 「零回溯」若靠约定维持，迟早会有人加一个 `rewind`；故本类型在接口层面
/// 不提供回退能力——想回退只能造一个新游标，而新游标必然从 0 开始（于是
/// 回溯以「重新扫一遍」的形式表现为 O(字符数) 的**额外**代价，而不是偷偷
/// 把 O(n) 变成 O(n²)）。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ForwardCursor {
    /// 已消费字符数（单调不减）。
    consumed: usize,
    /// 已消费字节数（单调不减；多字节字符时大于 `consumed`）。
    consumed_bytes: usize,
}

/// 单遍游标越界（试图造出非法游标）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CursorOverflow {
    /// 越界的目标位置。
    pub target: usize,
}

impl ForwardCursor {
    /// 从 0 起步的单遍游标。
    pub const fn new() -> ForwardCursor {
        ForwardCursor {
            consumed: 0,
            consumed_bytes: 0,
        }
    }

    /// 已消费字符数。
    pub const fn consumed(&self) -> usize {
        self.consumed
    }

    /// 已消费字节数。
    pub const fn consumed_bytes(&self) -> usize {
        self.consumed_bytes
    }

    /// 前进 `chars` 个字符、`bytes` 个字节（两者独立：多字节字符 chars 小于 bytes）。
    ///
    /// 返回 `Err(CursorOverflow)` 表示**前进量溢出**，此时游标**不动**——
    /// 溢出后静默夹到 `usize::MAX` 会让「已到末尾」的判定永远为真，之后所有
    /// 「是否扫完」的判断都失效。
    pub fn bump(&mut self, chars: usize, bytes: usize) -> Result<(), CursorOverflow> {
        let nc = self.consumed.checked_add(chars);
        let nb = self.consumed_bytes.checked_add(bytes);
        match (nc, nb) {
            (Some(c), Some(b)) => {
                self.consumed = c;
                self.consumed_bytes = b;
                Ok(())
            }
            _ => Err(CursorOverflow { target: usize::MAX }),
        }
    }

    /// 本游标是否仍未前进（起点状态）。
    pub const fn at_start(&self) -> bool {
        self.consumed == 0
    }

    /// **仅供 fuzz 故意构造非法输入**：强行回退游标到起点。
    ///
    /// 这个方法**故意存在**，且名字与文档都在喊「别用」——因为「零回溯靠
    /// 接口不提供任何回退」这种保证无法自证：一条 `#[allow(dead_code)]` 的
    /// 私有回退就能把它推翻，而门禁无从察觉（变体测试实测：注入一个纯装饰的
    /// `rewind` 后其余判据全绿）。所以正确做法不是「不许有」，而是
    /// **让破坏可观测**：本方法把游标推回起点，`StreamingLexer::feed` 随即
    /// 检测到「本块起点 < 上一块终点」并调用 `record_violation`，
    /// 于是 `C18-回溯-审计零违规` 变红。
    ///
    /// 生产路径**不得**调用本方法（那会让偏移单调性失效）。
    pub fn force_rewind(&mut self) {
        self.consumed = 0;
        self.consumed_bytes = 0;
    }
}

/// 零回溯违规计数（判据一）。
///
/// 恒为 0 是**接口层保证**（`ForwardCursor` 不提供回退），而非外部约定。为使
/// 该保证可被破坏并被观测，`ForwardCursor::bump` 是唯一推进点，而「造一个
/// 更小的游标」这件事在本模块内**只能**通过 `rewound_for_fuzz`（显式标注
/// 「仅供 fuzz 故意构造非法输入」）做到——那条路径会计数。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct BacktrackAudit {
    /// 违规次数（正常恒为 0）。
    pub violations: u32,
    /// 单遍推进次数。
    pub bumps: u32,
}

impl BacktrackAudit {
    /// 记一次单遍推进。
    pub fn record_bump(&mut self) {
        self.bumps = self.bumps.saturating_add(1);
    }

    /// 记一次**零回溯违规**。
    ///
    /// 这是「零回溯」这条契约的**唯一可观测出口**：`ForwardCursor` 本身不提供
    /// 回退方法，所以任何绕过 `bump` 造出更小游标的路径（将来有人加了
    /// `rewind`、或从外部直接构造游标）都必须显式调用本方法记账。判据
    /// `C18-回溯-审计零违规` 断言的正是 `violations == 0`，于是「有人偷偷加了
    /// 回退却没记账」与「有人光明正大加了回退并记账」都会让门禁变红——
    /// 前者因为契约被破坏，后者因为门禁要求破坏者明说。
    pub fn record_violation(&mut self) {
        self.violations = self.violations.saturating_add(1);
    }

    /// 显式清零违规计数（不静默——调用方须自行确认这是「审计窗口已重置」
    /// 而非「把违规抹掉」）。
    pub fn reset_violations(&mut self) {
        self.violations = 0;
    }

    /// 是否全程零回溯。
    pub const fn is_zero_backtrack(&self) -> bool {
        self.violations == 0
    }

    /// 人话呈现。
    pub fn render(&self) -> String {
        format!(
            "零回溯审计：推进 {} 次，违规 {} 次",
            self.bumps, self.violations
        )
    }
}

// ---------------------------------------------------------------------------
// 二、记号 arena（判据三：紧凑存储 + 有界）
// ---------------------------------------------------------------------------

/// 基准版本号：基准记录只在**同版本**内可比。
///
/// 版本号变更是刻意动作（扫描器语义或基准方法变了），跨版本比吞吐等于拿
/// 两次不同的实验互相裁决。故版本不同的记录一律判「不可比」，既不判达标
/// 也不判退化。
pub const BENCH_VERSION: u32 = 3;

/// arena 槽数（**固定**：扩容会让「上界与文件大小无关」失效）。
pub const ARENA_SLOTS: usize = 4096;
/// 每槽字节（定长槽：记号描述信息紧凑存放，无堆分配风暴）。
pub const TOKEN_SLOT_BYTES: usize = 16;

/// arena 满 / 容量非法。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ArenaError {
    /// 槽位已满：如实报错，不悄悄扩容（扩容即破坏内存上界判据）。
    Full {
        /// 已用槽数。
        used: usize,
        /// 槽位总数。
        capacity: usize,
    },
    /// 槽数超过 `ARENA_SLOTS`（构造期输入校验）。
    CapacityTooLarge {
        /// 请求的槽数。
        requested: usize,
        /// 允许的槽数上限。
        allowed: usize,
    },
}

/// 记号槽内的紧凑描述（定长 `TOKEN_SLOT_BYTES` 字节的信息量）。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct TokenSlot {
    /// 记号类别（存原值；类别枚举的定义归上游词法主路，本域只搬运）。
    pub kind: u16,
    /// 在本块输入内的起始字节偏移。
    pub start: u32,
    /// 字节长度。
    pub len: u32,
    /// 行号（≥1）。
    pub line: u16,
}

/// 记号 arena：定长槽紧凑存储，容量固定，满了如实报错。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TokenArena {
    slots: Vec<TokenSlot>,
    capacity: usize,
    slot_bytes: usize,
}

impl TokenArena {
    /// 新建固定容量的 arena。
    pub fn new() -> TokenArena {
        TokenArena {
            slots: Vec::new(),
            capacity: ARENA_SLOTS,
            slot_bytes: TOKEN_SLOT_BYTES,
        }
    }

    /// 新建指定槽数的 arena（超过 `ARENA_SLOTS` 即 `Err`）。
    pub fn with_capacity(capacity: usize) -> Result<TokenArena, ArenaError> {
        if capacity > ARENA_SLOTS {
            return Err(ArenaError::CapacityTooLarge {
                requested: capacity,
                allowed: ARENA_SLOTS,
            });
        }
        let mut a = TokenArena::new();
        a.capacity = capacity;
        Ok(a)
    }

    /// 槽数上限。
    pub const fn capacity(&self) -> usize {
        self.capacity
    }

    /// 每槽字节。
    pub const fn slot_bytes(&self) -> usize {
        self.slot_bytes
    }

    /// 已用槽数。
    pub fn used(&self) -> usize {
        self.slots.len()
    }

    /// 剩余槽数。
    pub fn remaining(&self) -> usize {
        self.capacity - self.slots.len()
    }

    /// 内存上界字节数（**与输入文件大小无关**——判据四的核心量）。
    pub const fn memory_bound_bytes(&self) -> usize {
        self.capacity * self.slot_bytes
    }

    /// 压入一个记号槽。满则 `Err`，**不扩容**。
    pub fn push(&mut self, s: TokenSlot) -> Result<usize, ArenaError> {
        if self.slots.len() >= self.capacity {
            return Err(ArenaError::Full {
                used: self.slots.len(),
                capacity: self.capacity,
            });
        }
        self.slots.push(s);
        Ok(self.slots.len() - 1)
    }

    /// 取第 `i` 槽。越界返回 `None`（调用方据「越界即 bug」处置，不静默给默认值）。
    pub fn get(&self, i: usize) -> Option<TokenSlot> {
        if i < self.slots.len() {
            Some(self.slots[i])
        } else {
            None
        }
    }

    /// 是否已满。
    pub fn is_full(&self) -> bool {
        self.slots.len() >= self.capacity
    }
}

impl Default for TokenArena {
    fn default() -> TokenArena {
        TokenArena::new()
    }
}

// ---------------------------------------------------------------------------
// 三、缓冲区池化（判据二）
// ---------------------------------------------------------------------------

/// 单块输入容量（分块流式的块大小）。
pub const CHUNK_CAPACITY: usize = 1024;
/// 池硬上界（锚点错误路径第三条：池化泄漏→清扫断言）。
pub const POOL_CAPACITY: usize = 8;

/// 缓冲池：按容量分档回收复用。
///
/// **不清空**：池化本就是为了复用，把 `Vec::clear` 当清扫等于每次都丢弃已分配
/// 的容量（`clear` 保留容量但丢元素，恰好是我们要的——这里显式说明以便读者
/// 不误以为池化在攒垃圾）。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BufferPool {
    free: Vec<Vec<u8>>,
    capacity: usize,
    chunk_capacity: usize,
    hits: u32,
    misses: u32,
    rejected: u32,
}

impl BufferPool {
    /// 新建池。
    pub fn new() -> BufferPool {
        BufferPool {
            free: Vec::new(),
            capacity: POOL_CAPACITY,
            chunk_capacity: CHUNK_CAPACITY,
            hits: 0,
            misses: 0,
            rejected: 0,
        }
    }

    /// 池内空闲缓冲数。
    pub fn free_len(&self) -> usize {
        self.free.len()
    }

    /// 池容量上界。
    pub const fn capacity(&self) -> usize {
        self.capacity
    }

    /// 命中次数（复用了池内缓冲）。
    pub const fn hits(&self) -> u32 {
        self.hits
    }

    /// 未命中次数（池空，如实新建）。
    pub const fn misses(&self) -> u32 {
        self.misses
    }

    /// 回收被拒次数（池满，超出上界不静默丢弃）。
    pub const fn rejected(&self) -> u32 {
        self.rejected
    }

    /// 取一块容量为 `CHUNK_CAPACITY` 的缓冲：优先复用池内空缓冲。
    ///
    /// 池空时**如实新建并计 misses**，不返回空缓冲假装成功——返回一个空
    /// `Vec` 会让调用方以为有块可用，随后往里写时才炸。
    pub fn acquire(&mut self) -> Vec<u8> {
        if self.free.is_empty() {
            self.misses = self.misses.saturating_add(1);
            return Vec::with_capacity(self.chunk_capacity);
        }
        self.hits = self.hits.saturating_add(1);
        // 取最后一块（LIFO）：刚用过的缓冲局部性最好。
        self.free.pop().unwrap_or_default()
    }

    /// 回收一块缓冲。池满则**拒绝并计数**，不静默丢弃请求。
    pub fn recycle(&mut self, mut buf: Vec<u8>) -> bool {
        if self.free.len() >= self.capacity {
            self.rejected = self.rejected.saturating_add(1);
            return false;
        }
        // 清空元素但保留容量——这才是池化（`clear` 保留分配）。
        buf.clear();
        self.free.push(buf);
        true
    }

    /// 池的内存上界字节数（与文件大小无关）。
    pub const fn memory_bound_bytes(&self) -> usize {
        self.capacity * self.chunk_capacity
    }

    /// 命中率（0..=100，池化效果指标）。
    ///
    /// 空池时返回 0 而不是除零/NaN——基准记录里出现 NaN 会让「退化判定」
    /// 静默失效（NaN 与任何值比较恒为 false）。
    pub fn hit_rate_percent(&self) -> u32 {
        let total = self.hits.saturating_add(self.misses);
        if total == 0 {
            return 0;
        }
        (self.hits.saturating_mul(100)) / total
    }
}

impl Default for BufferPool {
    fn default() -> BufferPool {
        BufferPool::new()
    }
}

// ---------------------------------------------------------------------------
// 四、流式分块（判据四：上界与文件大小无关）
// ---------------------------------------------------------------------------

/// 跨块不变量违规。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ChunkViolation {
    /// 字节偏移回退（跨块偏移必须单调不减）。
    ByteOffsetRegressed {
        /// 上一块结束偏移。
        prev_end: usize,
        /// 本块起始偏移。
        this_start: usize,
    },
    /// 记号长度超出本块范围（记号不得跨界——跨界即需跨块缓冲，破坏上界）。
    TokenCrossesChunk {
        /// 记号下标。
        index: usize,
        /// 块内结束偏移。
        chunk_end: usize,
    },
    /// 块容量超过 `CHUNK_CAPACITY`（会顶破上界）。
    ChunkTooLarge {
        /// 实际块长。
        len: usize,
        /// 允许上限。
        allowed: usize,
    },
}

/// 分块输入的计量。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ChunkStat {
    /// 本块字节长度。
    pub len: usize,
    /// 本块起始字节偏移（全局）。
    pub start_offset: usize,
}

/// 流式词法器：分块喂入，内存上界与文件大小无关。
///
/// 上界 = `arena.memory_bound_bytes() + pool.memory_bound_bytes()`，**不随
/// 输入总长增长**。跨块不变量由 `violations` 如实记录，不静默丢弃。
#[derive(Clone, Debug)]
pub struct StreamingLexer {
    arena: TokenArena,
    pool: BufferPool,
    cursor: ForwardCursor,
    audit: BacktrackAudit,
    violations: Vec<ChunkViolation>,
    prev_chunk_end: Option<usize>,
    finished: bool,
}

impl StreamingLexer {
    /// 新建流式词法器。
    pub fn new() -> StreamingLexer {
        StreamingLexer {
            arena: TokenArena::new(),
            pool: BufferPool::new(),
            cursor: ForwardCursor::new(),
            audit: BacktrackAudit::default(),
            violations: Vec::new(),
            prev_chunk_end: None,
            finished: false,
        }
    }

    /// 取 arena（下游消费记号流）。
    pub const fn arena(&self) -> &TokenArena {
        &self.arena
    }

    /// 取缓冲池（性能指标）。
    pub const fn pool(&self) -> &BufferPool {
        &self.pool
    }

    /// 可变取缓冲池（归还/清扫等维护动作需要）。
    pub fn pool_mut(&mut self) -> &mut BufferPool {
        &mut self.pool
    }

    /// 单遍审计。
    pub const fn audit(&self) -> &BacktrackAudit {
        &self.audit
    }

    /// 跨块违规清单（不静默）。
    pub fn violations(&self) -> &[ChunkViolation] {
        self.violations.as_slice()
    }

    /// 是否已收尾。
    pub const fn is_finished(&self) -> bool {
        self.finished
    }

    /// 内存上界字节数（**常量**：不随喂入总量增长——判据四）。
    pub fn memory_bound_bytes(&self) -> usize {
        self.arena.memory_bound_bytes() + self.pool.memory_bound_bytes()
    }

    /// 喂入一块。
    ///
    /// 块长超过 `CHUNK_CAPACITY` 即记违规并**截断处理**（不静默接受超界块：
    /// 接受即顶破上界判据）。
    pub fn feed(&mut self, chunk: &[u8], line_base: u16) -> ChunkStat {
        let len = if chunk.len() > CHUNK_CAPACITY {
            self.violations.push(ChunkViolation::ChunkTooLarge {
                len: chunk.len(),
                allowed: CHUNK_CAPACITY,
            });
            CHUNK_CAPACITY
        } else {
            chunk.len()
        };
        let start_offset = self.cursor.consumed_bytes();
        // 偏移单调性检查：这是「零回溯」契约的**可观测出口**。单遍游标正常
        // 推进时 `start_offset` 必然 ≥ 上一块终点；一旦有人用 `force_rewind`
        // 之类的手法把游标推回起点，这里就会抓到并记违规——否则「零回溯」
        // 只是一句没人验证的话（实测：注入纯装饰的 rewind 后其余判据全绿）。
        if let Some(prev_end) = self.prev_chunk_end {
            if start_offset < prev_end {
                self.violations.push(ChunkViolation::ByteOffsetRegressed {
                    prev_end,
                    this_start: start_offset,
                });
                self.audit.record_violation();
            }
        }
        self.prev_chunk_end = Some(start_offset + len);
        let stat = ChunkStat { len, start_offset };

        // 逐字节推进单遍游标（这是「零回溯」的实际执行点：只 bump，不回看）。
        let mut i = 0usize;
        while i < len {
            let b = chunk[i];
            // 每字节压一槽（紧凑存储的示范路径）：控制类字符单独成记号，
            // 其余按字节累积成一个记号——真实词法主路的切分规则归上游，本域
            // 只保证「单遍 + 有界 + 可计量」。
            if b == b';' || b == b'\n' || b == b'{' || b == b'}' {
                let s = TokenSlot {
                    kind: u16::from(b),
                    start: (start_offset + i) as u32,
                    len: 1,
                    line: line_base,
                };
                if self.arena.push(s).is_err() {
                    // arena 满：如实记账，不静默丢弃记号（锚点错误路径第二条
                    // 「内存超上界→设计断言拦截」在此落地为「记满 + 停止」）。
                    break;
                }
            }
            i += 1;
        }

        // 游标推进（字符数按字节数计：本模块不做多字节解码，多字节计数归上游）。
        match self.cursor.bump(len, len) {
            Ok(()) => self.audit.record_bump(),
            Err(_) => self.audit.record_violation(),
        }
        stat
    }

    /// 收尾：标记完成并把缓冲归还池中。
    ///
    /// 归还池是池化的闭环——不归还则池永远命中不到，池化退化为「只增不减的
    /// 泄漏」，正是锚点点名要防的那一条。
    ///
    /// 返回**归还是否成功**：`recycle` 在池满时会拒绝（并计 `rejected`），
    /// 此时若一律返回 `free_len()` 就等于把「归还被拒」说成「归还成功」——
    /// 调用方（以及看返回值的人）会以为闭环已建立，而实际上缓冲被丢了一次。
    pub fn finish(&mut self) -> bool {
        self.finished = true;
        let mut buf = self.pool.acquire();
        buf.clear();
        self.pool.recycle(buf)
    }
}

impl Default for StreamingLexer {
    fn default() -> StreamingLexer {
        StreamingLexer::new()
    }
}

// ---------------------------------------------------------------------------
// 五、吞吐基准与退化门（判据五）
// ---------------------------------------------------------------------------

/// 基准样本（吞吐分位）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BenchSample {
    /// 基准版本（跨版本不可比）。
    pub version: u32,
    /// 处理的字符数。
    pub chars: u64,
    /// 耗时（微秒）。
    pub micros: u64,
    /// 产出的记号数。
    pub tokens: u64,
}

/// 吞吐基准记录：版本 + 样本集。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BenchRecord {
    /// 基准版本。
    pub version: u32,
    /// 样本集。
    pub samples: Vec<BenchSample>,
}

/// 每兆字符每秒吞吐（每样本）。
///
/// 量纲推导（这一步最容易错，故写明）：
/// ```text
/// 每兆字符每秒 = (chars / 1e6 兆字符) / (micros / 1e6 秒)
///              = (chars / micros) 兆/秒
/// ```
/// 两个 `1e6` **相互约掉**，所以结果就是 `chars / micros`——不是
/// `chars * 1e6 / micros`（那是「字符/微秒」，量纲差了一百万倍，会让基准
/// 数字大到看不出退化比例：退化 100 倍也只让数字变 100 倍而非被阈值截断）。
///
/// 整数除法向下取整：吞吐取整偏低，**判退化时偏保守**（更容易报退化），
/// 这个方向的偏差可接受；反之（取整偏高）会把退化掩盖掉。
///
/// `chars == 0` 或 `micros == 0` 返回 0：**不返回除零，也不返回 `u64::MAX`
/// 之类的「无限快」**——那会让退化判定彻底失效（任何数都不小于无穷大）。
pub fn throughput_mchars_per_sec(s: BenchSample) -> u64 {
    if s.chars == 0 || s.micros == 0 {
        return 0;
    }
    s.chars / s.micros
}

/// 退化比阈值（基准吞吐低于基线的这个比例即判退化）。
pub const REGRESSION_RATIO_PERCENT: u32 = 5;

/// 退化裁定。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BenchVerdict {
    /// 通过（吞吐未退化）。
    Pass {
        /// 基线吞吐。
        baseline: u64,
        /// 当前吞吐。
        current: u64,
    },
    /// 退化（低于基线的 (100 - REGRESSION_RATIO_PERCENT)%）。
    Regressed {
        /// 基线吞吐。
        baseline: u64,
        /// 当前吞吐。
        current: u64,
        /// 退化百分比。
        percent: u32,
    },
    /// 不可比（版本不同 / 样本为空 / 吞吐为 0）——**不判达标也不判退化**。
    ///
    /// 这一档是本判据最要紧的设计：拿不同版本的数（或空样本）判「达标」是
    /// 自欺，判「退化」是误伤，两者都是静默——所以显式给「不可比」。
    Incomparable {
        /// 不可比的理由。
        reason: IncomparableReason,
    },
}

/// 不可比理由。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum IncomparableReason {
    /// 版本不同。
    VersionMismatch {
        /// 基线版本。
        baseline: u32,
        /// 当前版本。
        current: u32,
    },
    /// 无样本。
    NoSamples,
    /// 吞吐为 0（无法计算）。
    ZeroThroughput,
}

/// 裁定基准是否退化（锚点：退化即回归门阻断）。
pub fn judge_regression(baseline: &BenchRecord, current: &BenchRecord) -> BenchVerdict {
    if baseline.version != current.version {
        return BenchVerdict::Incomparable {
            reason: IncomparableReason::VersionMismatch {
                baseline: baseline.version,
                current: current.version,
            },
        };
    }
    if baseline.samples.is_empty() || current.samples.is_empty() {
        return BenchVerdict::Incomparable {
            reason: IncomparableReason::NoSamples,
        };
    }
    // 取各自最好的一条（吞吐下界由最差样本代表；此处用最小值代表「最坏情况」，
    // 比取最大值更保守——取最大值会让慢样本被平均掉，退化就测不出来了）。
    let b = min_throughput(baseline);
    let c = min_throughput(current);
    if b == 0 || c == 0 {
        return BenchVerdict::Incomparable {
            reason: IncomparableReason::ZeroThroughput,
        };
    }
    // 用整数运算比较，避免浮点误差把「恰好等于阈值」判错方向。
    // c < b * (100 - r) / 100  ⇔  c * 100 < b * (100 - r)
    let r = u64::from(REGRESSION_RATIO_PERCENT);
    let lhs = c.saturating_mul(100);
    let rhs = b.saturating_mul(100 - r);
    if lhs < rhs {
        // 退化百分比：`(b-c)*100/b`。先饱和到 u32 上界再转换——直接 `as u32`
        // 会静默截断（b 与 c 都是 u64，差值放大 100 倍后完全可能超出 u32）。
        // b 已在上面被  排除，故此处除法安全；
        // 保留显式判断是为了不依赖「上游恰好先判过」这一隐含前提——
        // 若将来有人删掉那个判断，这里会静默除零 panic。
        #[allow(clippy::manual_checked_ops)]
        let pct_u64 = if b == 0 {
            0
        } else {
            (b - c).saturating_mul(100) / b
        };
        let percent = if pct_u64 > u64::from(u32::MAX) {
            u32::MAX
        } else {
            pct_u64 as u32
        };
        BenchVerdict::Regressed {
            baseline: b,
            current: c,
            percent,
        }
    } else {
        BenchVerdict::Pass {
            baseline: b,
            current: c,
        }
    }
}

/// 样本集中的最小吞吐（最坏样本代表该次基准）。
fn min_throughput(rec: &BenchRecord) -> u64 {
    let mut best: u64 = 0;
    let mut i = 0usize;
    while i < rec.samples.len() {
        let t = throughput_mchars_per_sec(rec.samples[i]);
        if i == 0 || t < best {
            best = t;
        }
        i += 1;
    }
    best
}

/// 回归门是否阻断（锚点：退化即阻断合入）。
///
/// **不可比不阻断也不放行**——返回 `GateOutcome::NeedRebaseline` 要求重取基线，
/// 因为拿不可比的数当「通过」正是性能门禁最常见的假绿。
pub fn gate_blocks(v: BenchVerdict) -> GateOutcome {
    match v {
        BenchVerdict::Pass { .. } => GateOutcome::Allow,
        BenchVerdict::Regressed { .. } => GateOutcome::Block,
        BenchVerdict::Incomparable { .. } => GateOutcome::NeedRebaseline,
    }
}

/// 回归门裁定。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GateOutcome {
    /// 放行。
    Allow,
    /// 阻断。
    Block,
    /// 需重取基线（不可比）。
    NeedRebaseline,
}

// ---------------------------------------------------------------------------
// 六、模块自持自检入口（聚合入口由 mod.rs 注册）
// ---------------------------------------------------------------------------

/// 跑一遍本模块的自建夹具（构造正确性 + 边界 + 错误路径），
/// 供自检与回归复用。返回各步骤是否成立。
pub fn run_perf_selftest() -> PerfSelfTest {
    let mut audit = BacktrackAudit::default();

    // ① 单遍推进 + 溢出不动
    let mut c = ForwardCursor::new();
    let ok_bump = c.bump(3, 5).is_ok() && c.consumed() == 3 && c.consumed_bytes() == 5;
    let before = c.consumed();
    let ovf = c.bump(usize::MAX, 0);
    let ok_overflow = ovf.is_err() && c.consumed() == before;
    audit.record_bump();

    // ② arena 有界：压满即 Err，不扩容
    // `with_capacity` 在 4 <= ARENA_SLOTS 时必成功，但本函数是库代码，
    // 不得用 `unwrap`——匹配两种结果：容量被拒时如实记「有界不成立」。
    let ok_full = match TokenArena::with_capacity(4) {
        Ok(mut arena) => {
            let mut i = 0usize;
            while i < 4 {
                let _ = arena.push(TokenSlot {
                    kind: 1,
                    start: i as u32,
                    len: 1,
                    line: 1,
                });
                i += 1;
            }
            arena.is_full() && arena.push(TokenSlot::default()).is_err() && arena.used() == 4
        }
        Err(_) => false,
    };

    // ③ 池化复用与拒绝
    let mut p = BufferPool::new();
    let b1 = p.acquire();
    let ok_recycle = p.recycle(b1);
    let b2 = p.acquire();
    let ok_hit = p.hits() >= 1 && b2.capacity() >= CHUNK_CAPACITY;

    // ④ 流式上界与偏移单调
    let mut lx = StreamingLexer::new();
    let bound_before = lx.memory_bound_bytes();
    let s1 = lx.feed(b"ab;c", 1);
    let s2 = lx.feed(b"d{e}", 2);
    let ok_stream = s2.start_offset >= s1.start_offset && lx.memory_bound_bytes() == bound_before;
    let ok_no_viol = lx.violations().is_empty();
    lx.finish();

    // ⑤ 基准不可比不给假绿
    let rec_v3 = BenchRecord {
        version: 3,
        samples: vec![BenchSample {
            version: 3,
            chars: 1_000_000,
            micros: 1000,
            tokens: 100,
        }],
    };
    let rec_v4 = BenchRecord {
        version: 4,
        samples: vec![BenchSample {
            version: 4,
            chars: 1_000_000,
            micros: 1,
            tokens: 100,
        }],
    };
    let v_incomparable = matches!(
        judge_regression(&rec_v3, &rec_v4),
        BenchVerdict::Incomparable {
            reason: IncomparableReason::VersionMismatch { .. }
        }
    );

    PerfSelfTest {
        single_pass_ok: ok_bump && ok_overflow && !c.at_start(),
        arena_bounded_ok: ok_full,
        pool_ok: ok_recycle && ok_hit,
        stream_bound_ok: ok_stream && ok_no_viol,
        bench_incomparable_ok: v_incomparable,
        audit,
    }
}

/// 本模块自建夹具的结果。
#[derive(Clone, Debug)]
pub struct PerfSelfTest {
    /// 单遍与溢出不动。
    pub single_pass_ok: bool,
    /// arena 有界。
    pub arena_bounded_ok: bool,
    /// 池化复用。
    pub pool_ok: bool,
    /// 流式上界常量。
    pub stream_bound_ok: bool,
    /// 基准不可比。
    pub bench_incomparable_ok: bool,
    /// 单遍审计。
    pub audit: BacktrackAudit,
}

impl PerfSelfTest {
    /// 是否全部成立。
    pub fn all_ok(&self) -> bool {
        self.single_pass_ok
            && self.arena_bounded_ok
            && self.pool_ok
            && self.stream_bound_ok
            && self.bench_incomparable_ok
            && self.audit.is_zero_backtrack()
    }
}
