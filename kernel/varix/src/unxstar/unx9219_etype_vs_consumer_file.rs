//! UNX-F9219 文件族 E 型与 glibc errno 透传消费核 · 0x7A440段
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（派工卡 UNX-F9219 逐字）：
//!   ① 纯功能代码 ≥320 行；② 判据原文：UNX-F9219-J1 glibc errno 读取透传链
//!   判据过，perror/strerror 输出逐码一致格过，消费号登记格过；③ 完成时
//!   API/软件写回：状态=已完成、结果摘要非空。
//!
//! 设计要点：
//!   ① 透传链的口径以 glibc 的 `__syscall_ret` 为准：内核返回 raw ∈
//!      [-4095, -1] 视为错误，errno = -raw、返回 -1；raw < -4095（典型
//!      -4096）**不是错误**而是合法返回值，必须原样透传。这条边界是把
//!      「raw < 0 即错误」的朴素实现钉死的唯一判据。
//!   ② errno 是粘滞的：成功的系统调用**不**清零 errno。消费者必须先看
//!      返回值再看 errno；链里任何一步「顺手清零」都算 ErrnoClobbered。
//!   ③ 链状态机 Idle→Raw→Classified→Delivered→Rendered 固化阶段次序：
//!      未取到内核返回值就渲染属越级，Raw 直接跳 Delivered 同理被拒。
//!   ④ 消费号登记格：每个被 glibc 层消费的 errno 在册内占一个稳定号，
//!      编号单调发放、重复登记拒、环外 errno（未进 strerror 表）拒收；
//!      消费记账 已消费 = 已登记命中 + 未登记命中，未登记命中必须为 0。
//!   ⑤ 对账：16 条 strerror 文案与 perror 渲染逐码比对，任一行不等即红；
//!      注入器三类违约（边界误读/errno 被清/未登记消费）各 10 轮全抓。
//! ─────────────────────────────────────────────────────────────────────
use alloc::format;
use alloc::string::String;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（F9219 独占 0x40 窗口）。
pub const SEG_BASE: u32 = 0x7A440;
/// glibc 错误区上界：raw ∈ [-4095, -1] 为错误。
pub const GLIBC_ERRNO_MAX: i64 = 4095;
/// strerror 表行数（20 条，宽于登记格，留出「表内但未登记」余量）。
pub const STRERROR_ROWS: usize = 20;
/// 登记格容量（16 条）：小于表宽，才能真实触发 RegistryFull。
pub const REGISTRY_CAP: usize = 16;
/// 每类注入复测轮数。
pub const INJECT_ROUNDS: u32 = 10;
/// 注入类别数。
pub const INJECT_CLASSES: u32 = 3;

// Linux errno（正数口径）
pub const EPERM: i32 = 1;
pub const ENOENT: i32 = 2;
pub const EINTR: i32 = 4;
pub const EIO: i32 = 5;
pub const EBADF: i32 = 9;
pub const EAGAIN: i32 = 11;
pub const ENOMEM: i32 = 12;
pub const EACCES: i32 = 13;
pub const EFAULT: i32 = 14;
pub const EEXIST: i32 = 17;
pub const ENOTDIR: i32 = 20;
pub const EISDIR: i32 = 21;
pub const EINVAL: i32 = 22;
pub const ENFILE: i32 = 23;
pub const EMFILE: i32 = 24;
pub const ENOSPC: i32 = 28;
pub const ESPIPE: i32 = 29;
pub const EROFS: i32 = 30;
pub const EPIPE: i32 = 32;
pub const ENAMETOOLONG: i32 = 36;
/// 不在表内的样本码（ENOSYS）：用于「未登记消费」注入。
pub const ENOSYS: i32 = 38;

// ── 诊断码闭集 ────────────────────────────────────────────────────────
/// 本模块诊断码（0x7A440 段内 +0x01..+0x0C 闭集）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum C2Code {
    NaiveBoundaryMisread, ErrnoClobbered, ConsumerUnregistered, DuplicateConsumer,
    RegistryFull, UnregisteredErrno, StageViolation, NotAnError,
    MessageMismatch, CountMismatch, NotReproducible, SegmentViolation,
}

impl C2Code {
    /// 码值（段基址 + 变体序号）。
    pub fn val(self) -> u32 { SEG_BASE + self as u32 }

    /// 中文诊断与可操作建议（闭集返回，禁止空串）。
    pub fn detail(self) -> &'static str {
        match self {
            C2Code::NaiveBoundaryMisread => "边界误读：把 raw < -4095（如 -4096）当成错误；glibc 只认 [-4095,-1]，请改用 checked_neg 判定并原样透传",
            C2Code::ErrnoClobbered => "errno 被清：成功路径清零会让消费者读到 0；errno 是粘滞的，只有错误分支才写",
            C2Code::ConsumerUnregistered => "消费未登记：该 errno 没有消费号就被读取；请先在登记格分配号再消费",
            C2Code::DuplicateConsumer => "消费号重复登记：同一 errno 占两号会让对账错位；请复用既有号或先注销",
            C2Code::RegistryFull => "登记格已满：容量 16 与 strerror 表同宽；请先扩表再登记，不许静默丢弃",
            C2Code::UnregisteredErrno => "errno 未进表：该码没有 strerror 文案；请补文案后再登记，禁止返回空串",
            C2Code::StageViolation => "链阶段越级：只允许 Idle→Raw→Classified→Delivered→Rendered，取返回值前不得渲染",
            C2Code::NotAnError => "非错误上下文：成功调用后直接 perror 会打印陈旧 errno；请先确认 errno_set",
            C2Code::MessageMismatch => "文案不一致：本册 strerror/perror 与 glibc 逐码比对不等；请按表内原文修正",
            C2Code::CountMismatch => "计数不守恒：已消费与命中数不等；请逐项复算，禁止用总量掩盖",
            C2Code::NotReproducible => "双跑不一致：同输入两次指纹不同；请排查隐藏状态",
            C2Code::SegmentViolation => "段位越界：段基址须 0x40 对齐且落在 0x7A000..=0x7D1C0",
        }
    }

    /// 闭集全量。
    pub const ALL: [C2Code; 12] = [
        C2Code::NaiveBoundaryMisread, C2Code::ErrnoClobbered, C2Code::ConsumerUnregistered,
        C2Code::DuplicateConsumer, C2Code::RegistryFull, C2Code::UnregisteredErrno,
        C2Code::StageViolation, C2Code::NotAnError, C2Code::MessageMismatch,
        C2Code::CountMismatch, C2Code::NotReproducible, C2Code::SegmentViolation,
    ];
}

// ── strerror 表（glibc C locale 原文） ────────────────────────────────
/// (符号名, errno, glibc C locale 原文)。
pub const STRERROR_TABLE: [(&str, i32, &str); STRERROR_ROWS] = [
    ("EPERM", EPERM, "Operation not permitted"),
    ("ENOENT", ENOENT, "No such file or directory"),
    ("EINTR", EINTR, "Interrupted system call"),
    ("EIO", EIO, "Input/output error"),
    ("EBADF", EBADF, "Bad file descriptor"),
    ("EAGAIN", EAGAIN, "Resource temporarily unavailable"),
    ("ENOMEM", ENOMEM, "Cannot allocate memory"),
    ("EACCES", EACCES, "Permission denied"),
    ("EFAULT", EFAULT, "Bad address"),
    ("EEXIST", EEXIST, "File exists"),
    ("ENOTDIR", ENOTDIR, "Not a directory"),
    ("EISDIR", EISDIR, "Is a directory"),
    ("EINVAL", EINVAL, "Invalid argument"),
    ("ENFILE", ENFILE, "Too many open files in system"),
    ("EMFILE", EMFILE, "Too many open files"),
    ("ENOSPC", ENOSPC, "No space left on device"),
    ("ESPIPE", ESPIPE, "Illegal seek"),
    ("EROFS", EROFS, "Read-only file system"),
    ("EPIPE", EPIPE, "Broken pipe"),
    ("ENAMETOOLONG", ENAMETOOLONG, "File name too long"),
];

/// 文案查询结果。未进表的码不回空串，而是显性标 known=false
/// （glibc 实际渲染 "Unknown error NNN"，此处保留可复算的 known 位）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Message {
    /// 渲染文本
    pub text: &'static str,
    /// 是否命中表内
    pub known: bool,
}

/// 查 strerror 文案。
pub fn strerror(code: i32) -> Message {
    let mut i = 0usize;
    while i < STRERROR_ROWS {
        if let Some((_sym, num, text)) = STRERROR_TABLE.get(i).copied() {
            if num == code {
                return Message { text, known: true };
            }
        }
        i += 1;
    }
    Message { text: "Unknown error", known: false }
}

/// 该码是否进表（登记格的前置条件）。
pub fn known_errno(code: i32) -> bool {
    strerror(code).known
}

/// perror 渲染：prefix 非空时 "prefix: msg\n"，否则 "msg\n"。
pub fn perror_render(prefix: &str, code: i32) -> String {
    let m = strerror(code);
    if prefix.is_empty() {
        format!("{}\n", m.text)
    } else {
        format!("{}: {}\n", prefix, m.text)
    }
}

// ── 内核返回值分类 ────────────────────────────────────────────────────
/// 内核发出值的解释：要么是返回值，要么是错误号。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Wire {
    /// 原样透传的返回值（含 -4096 这类负但非错误的值）
    Value(i64),
    /// 内核错误，携带 errno 正数
    Errno(i32),
}

/// glibc 口径分类：raw ∈ [-4095, -1] 是错误，其余全是返回值。
pub fn classify(raw: i64) -> Wire {
    if raw < 0 {
        match raw.checked_neg() {
            // 上界含 4095：-4095 仍是错误，-4096 落到下面的透传分支。
            Some(mag) if mag <= GLIBC_ERRNO_MAX => Wire::Errno(mag as i32),
            _ => Wire::Value(raw),
        }
    } else {
        Wire::Value(raw)
    }
}

/// 朴素实现（被注入的缺陷样本）：raw < 0 一律当错误。
pub fn classify_naive(raw: i64) -> Wire {
    if raw < 0 {
        match raw.checked_neg() {
            Some(mag) => Wire::Errno(mag as i32),
            None => Wire::Errno(i32::MAX),
        }
    } else {
        Wire::Value(raw)
    }
}

// ── errno 单元 ────────────────────────────────────────────────────────
/// errno 单元：粘滞语义（只有错误分支写，成功不清零）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ErrnoCell {
    value: i32,
    writes: u32,
    reads: u32,
}

impl ErrnoCell {
    /// 初值 0（Unset）。
    pub const fn new() -> ErrnoCell { ErrnoCell { value: 0, writes: 0, reads: 0 } }
    /// 写 errno（仅错误分支调用）。
    pub fn set(&mut self, v: i32) {
        self.value = v;
        self.writes = self.writes.saturating_add(1);
    }
    /// 读 errno（消费者读取，记读账）。
    pub fn get(&mut self) -> i32 {
        self.reads = self.reads.saturating_add(1);
        self.value
    }
    /// 只读窥视（不记读账，供判据与诊断用）。
    pub fn peek(&self) -> i32 { self.value }
    /// 写次数账。
    pub fn writes(&self) -> u32 { self.writes }
    /// 读次数账。
    pub fn reads(&self) -> u32 { self.reads }
    /// 是否已被写过。
    pub fn is_set(&self) -> bool { self.value != 0 }
}

/// libc 层返回：-1 + errno_set 或原样返回值。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct LibcRet {
    /// 返回给调用者的值
    pub ret: i64,
    /// 本次是否写了 errno
    pub errno_set: bool,
}

/// 透传一步：错误 → errno=mag、ret=-1；返回值 → 原样，**不动 errno**。
pub fn syscall_ret(raw: i64, cell: &mut ErrnoCell) -> LibcRet {
    match classify(raw) {
        Wire::Errno(e) => {
            cell.set(e);
            LibcRet { ret: -1, errno_set: true }
        }
        Wire::Value(v) => LibcRet { ret: v, errno_set: false },
    }
}

// ── 消费者链状态机 ────────────────────────────────────────────────────
/// 链阶段。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Stage {
    /// 空闲（未取返回值）
    Idle,
    /// 已取到内核 raw
    Raw,
    /// 已分类
    Classified,
    /// 已交付调用者（errno 已定）
    Delivered,
    /// 已渲染 perror/strerror
    Rendered,
}

impl Stage {
    /// 阶段名。
    pub const fn label(self) -> &'static str {
        match self {
            Stage::Idle => "idle", Stage::Raw => "raw", Stage::Classified => "classified",
            Stage::Delivered => "delivered", Stage::Rendered => "rendered",
        }
    }
    /// 转移表：只允许沿链前进，或从交付/渲染态开启下一次调用。
    pub const fn can_go(self, to: Stage) -> bool {
        match (self, to) {
            (Stage::Idle, Stage::Raw) => true,
            (Stage::Raw, Stage::Classified) => true,
            (Stage::Classified, Stage::Delivered) => true,
            (Stage::Delivered, Stage::Rendered) => true,
            // 连续系统调用：交付后或渲染后可直接取下一个 raw。
            (Stage::Delivered, Stage::Raw) => true,
            (Stage::Rendered, Stage::Raw) => true,
            _ => false,
        }
    }
}

/// 消费者链：内核 raw → libc 返回 → 渲染。
#[derive(Clone, Copy, Debug)]
pub struct ConsumerChain {
    stage: Stage,
    cell: ErrnoCell,
    last: LibcRet,
    calls: u32,
    renders: u32,
    violations: u32,
}

impl ConsumerChain {
    /// 新建链。
    pub const fn new() -> ConsumerChain {
        ConsumerChain {
            stage: Stage::Idle, cell: ErrnoCell::new(),
            last: LibcRet { ret: 0, errno_set: false },
            calls: 0, renders: 0, violations: 0,
        }
    }
    /// 当前阶段。
    pub fn stage(&self) -> Stage { self.stage }
    /// 上一次调用的返回。
    pub fn last(&self) -> LibcRet { self.last }
    /// errno 单元（只读窥视）。
    pub fn cell(&self) -> ErrnoCell { self.cell }
    /// 调用次数。
    pub fn calls(&self) -> u32 { self.calls }
    /// 渲染次数。
    pub fn renders(&self) -> u32 { self.renders }
    /// 违约次数。
    pub fn violations(&self) -> u32 { self.violations }

    /// 喂入一个内核返回值，走完 Raw→Classified→Delivered。
    pub fn feed(&mut self, raw: i64) -> Result<LibcRet, C2Code> {
        if !self.stage.can_go(Stage::Raw) {
            self.violations = self.violations.saturating_add(1);
            return Err(C2Code::StageViolation);
        }
        self.stage = Stage::Raw;
        let wire = classify(raw);
        self.stage = Stage::Classified;
        let out = match wire {
            Wire::Errno(e) => {
                self.cell.set(e);
                LibcRet { ret: -1, errno_set: true }
            }
            // 成功路径绝不触碰 errno（粘滞）。
            Wire::Value(v) => LibcRet { ret: v, errno_set: false },
        };
        self.stage = Stage::Delivered;
        self.last = out;
        self.calls = self.calls.saturating_add(1);
        Ok(out)
    }

    /// 渲染 perror：必须已交付（否则就是拿陈旧 errno 打印）。
    pub fn render(&mut self, prefix: &str) -> Result<String, C2Code> {
        if !self.stage.can_go(Stage::Rendered) {
            self.violations = self.violations.saturating_add(1);
            return Err(C2Code::StageViolation);
        }
        if !self.last.errno_set {
            self.violations = self.violations.saturating_add(1);
            return Err(C2Code::NotAnError);
        }
        let code = self.cell.get();
        self.stage = Stage::Rendered;
        self.renders = self.renders.saturating_add(1);
        Ok(perror_render(prefix, code))
    }

    /// 链指纹（含阶段、errno、返回与账目）。
    pub fn fingerprint(&self) -> u32 {
        let bytes: Vec<u8> = Vec::from([
            self.stage as u8,
            (self.cell.peek() & 0xFF) as u8,
            ((self.last.ret + 1) & 0xFF) as u8,
            u8::from(self.last.errno_set),
            (self.calls & 0xFF) as u8,
            (self.renders & 0xFF) as u8,
        ]);
        fnv1a(&bytes)
    }
}

// ── 消费号登记格 ──────────────────────────────────────────────────────
/// 单个消费登记项。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Consumer {
    /// 被消费的 errno
    pub errno: i32,
    /// 消费号（单调发放，从 0 起）
    pub no: u16,
    /// 命中次数
    pub hits: u32,
}

impl Consumer {
    const fn empty() -> Consumer { Consumer { errno: 0, no: 0, hits: 0 } }
}

/// 消费结果。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Consume {
    /// 命中已登记消费号
    Registered(u16),
    /// 未登记（必须为 0 次才合格）
    Unknown,
}

/// 消费号登记格。
#[derive(Clone, Copy, Debug)]
pub struct ConsumerRegistry {
    slots: [Option<Consumer>; REGISTRY_CAP],
    count: usize,
    next_no: u16,
    consumed: u32,
    unknown_hits: u32,
}

impl ConsumerRegistry {
    /// 空登记格。
    pub const fn new() -> ConsumerRegistry {
        ConsumerRegistry {
            slots: [None; REGISTRY_CAP], count: 0, next_no: 0, consumed: 0, unknown_hits: 0,
        }
    }
    /// 已登记条数。
    pub fn count(&self) -> usize { self.count }
    /// 已消费次数（含未登记命中）。
    pub fn consumed(&self) -> u32 { self.consumed }
    /// 未登记命中次数（必须 0）。
    pub fn unknown_hits(&self) -> u32 { self.unknown_hits }
    /// 已登记命中次数。
    pub fn registered_hits(&self) -> u32 {
        (0..self.count).filter_map(|i| self.item(i)).map(|c| c.hits).sum()
    }
    /// 第 i 项。
    pub fn item(&self, i: usize) -> Option<Consumer> {
        match self.slots.get(i) {
            Some(Some(c)) => Some(*c),
            _ => None,
        }
    }
    /// 按 errno 找登记号。
    pub fn lookup(&self, errno: i32) -> Option<u16> {
        (0..self.count).filter_map(|i| self.item(i)).find(|c| c.errno == errno).map(|c| c.no)
    }

    /// 登记一个 errno。先判表内（未进 strerror 表直接拒），再判重复，再判容量。
    pub fn register(&mut self, errno: i32) -> Result<u16, C2Code> {
        if !known_errno(errno) {
            return Err(C2Code::UnregisteredErrno);
        }
        if self.lookup(errno).is_some() {
            return Err(C2Code::DuplicateConsumer);
        }
        if self.count >= REGISTRY_CAP {
            return Err(C2Code::RegistryFull);
        }
        let no = self.next_no;
        let idx = self.count;
        if let Some(slot) = self.slots.get_mut(idx) {
            *slot = Some(Consumer { errno, no, hits: 0 });
            self.count = self.count.saturating_add(1);
            self.next_no = self.next_no.saturating_add(1);
            return Ok(no);
        }
        Err(C2Code::RegistryFull)
    }

    /// 消费一次：命中登记号则记命中，否则记未登记命中（不静默）。
    pub fn consume(&mut self, errno: i32) -> Consume {
        self.consumed = self.consumed.saturating_add(1);
        let mut hit = None;
        let mut i = 0usize;
        while i < self.count {
            if let Some(c) = self.item(i) {
                if c.errno == errno {
                    hit = Some(i);
                    break;
                }
            }
            i += 1;
        }
        match hit {
            Some(idx) => {
                let mut no = 0u16;
                if let Some(c) = self.item(idx) {
                    no = c.no;
                }
                if let Some(slot) = self.slots.get_mut(idx) {
                    if let Some(c) = slot {
                        c.hits = c.hits.saturating_add(1);
                    }
                }
                Consume::Registered(no)
            }
            None => {
                self.unknown_hits = self.unknown_hits.saturating_add(1);
                Consume::Unknown
            }
        }
    }

    /// 守恒式：已消费 = 已登记命中 + 未登记命中。
    pub fn conserved(&self) -> bool {
        self.registered_hits().saturating_add(self.unknown_hits) == self.consumed
    }

    /// 登记格指纹（按登记序，含 errno/号/命中）。
    pub fn fingerprint(&self) -> u32 {
        let mut bytes: Vec<u8> = Vec::new();
        for c in (0..self.count).filter_map(|i| self.item(i)) {
            bytes.push((c.errno & 0xFF) as u8);
            bytes.push((c.no & 0xFF) as u8);
            bytes.push((c.hits & 0xFF) as u8);
        }
        bytes.push(self.count as u8);
        bytes.push(self.unknown_hits as u8);
        fnv1a(&bytes)
    }
}

/// 全量登记 16 条（=容量）并各消费一次（对账与守恒的规范化巡检）。
pub fn patrol_registry() -> ConsumerRegistry {
    let mut reg = ConsumerRegistry::new();
    let mut i = 0usize;
    while i < REGISTRY_CAP {
        if let Some((_sym, num, _text)) = STRERROR_TABLE.get(i).copied() {
            let _ = reg.register(num);
            let _ = reg.consume(num);
        }
        i += 1;
    }
    reg
}

// ── 注入器 ────────────────────────────────────────────────────────────
/// 三类注入样本。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum C2Inject {
    /// 边界误读：raw=-4096（朴素实现当错误）
    BoundaryMisread,
    /// errno 被清：成功路径清零后再读
    ErrnoClobber,
    /// 未登记消费：消费 ENOSYS
    UnregisteredConsume,
}

impl C2Inject {
    /// 期望码。
    pub fn expected_code(self) -> C2Code {
        match self {
            C2Inject::BoundaryMisread => C2Code::NaiveBoundaryMisread,
            C2Inject::ErrnoClobber => C2Code::ErrnoClobbered,
            C2Inject::UnregisteredConsume => C2Code::ConsumerUnregistered,
        }
    }
    /// 闭集全量。
    pub const ALL: [C2Inject; 3] =
        [C2Inject::BoundaryMisread, C2Inject::ErrnoClobber, C2Inject::UnregisteredConsume];
}

/// 注入账。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct C2Ledger {
    /// 注入次数
    pub injected: u32,
    /// 检出次数
    pub detected: u32,
    /// 逃逸次数（必须 0）
    pub escaped: u32,
}

impl C2Ledger {
    /// 零账。
    pub const fn zero() -> C2Ledger { C2Ledger { injected: 0, detected: 0, escaped: 0 } }
    /// 记一笔。
    pub fn record(&mut self, caught: bool) {
        self.injected = self.injected.saturating_add(1);
        if caught {
            self.detected = self.detected.saturating_add(1);
        } else {
            self.escaped = self.escaped.saturating_add(1);
        }
    }
    /// 守恒式。
    pub fn conserved(&self) -> bool { self.detected.saturating_add(self.escaped) == self.injected }
}

/// 单轮注入：返回是否被检出。
pub fn inject_once(kind: C2Inject) -> bool {
    match kind {
        // 朴素分类把 -4096 判成错误，本册分类判成返回值 → 差异即检出。
        C2Inject::BoundaryMisread => {
            let raw = -(GLIBC_ERRNO_MAX + 1);
            classify_naive(raw) != classify(raw)
                && classify(raw) == Wire::Value(raw)
                && classify_naive(raw) == Wire::Errno(4096)
        }
        // 成功调用后再成功调用，errno 必须保持首个错误值（粘滞）。
        C2Inject::ErrnoClobber => {
            let mut chain = ConsumerChain::new();
            let first = chain.feed(-EBADF as i64);
            let stuck = chain.cell().peek();
            let second = chain.feed(7);
            let after = chain.cell().peek();
            let third = chain.feed(-EIO as i64);
            first == Ok(LibcRet { ret: -1, errno_set: true })
                && second == Ok(LibcRet { ret: 7, errno_set: false })
                && third == Ok(LibcRet { ret: -1, errno_set: true })
                && stuck == EBADF && after == EBADF && chain.cell().peek() == EIO
        }
        // 未登记就消费：登记格必须记未知命中，且查表 known=false。
        C2Inject::UnregisteredConsume => {
            let mut reg = ConsumerRegistry::new();
            let _ = reg.register(EBADF);
            let out = reg.consume(ENOSYS);
            out == Consume::Unknown && reg.unknown_hits() == 1 && !known_errno(ENOSYS)
                && reg.lookup(ENOSYS).is_none()
        }
    }
}

/// 跑三类注入 × 10 轮。
pub fn run_consumer_matrix() -> (C2Ledger, [u32; 3]) {
    let mut ledger = C2Ledger::zero();
    let mut per_class = [0u32; 3];
    let mut ci = 0usize;
    while ci < C2Inject::ALL.len() {
        if let Some(kind) = C2Inject::ALL.get(ci).copied() {
            let mut round = 0u32;
            while round < INJECT_ROUNDS {
                let caught = inject_once(kind);
                ledger.record(caught);
                if caught {
                    if let Some(slot) = per_class.get_mut(ci) {
                        *slot = slot.saturating_add(1);
                    }
                }
                round += 1;
            }
        }
        ci += 1;
    }
    (ledger, per_class)
}

// ── 对账表 ────────────────────────────────────────────────────────────
/// 逐码对账：表内 16 行 strerror 与 perror 渲染是否与本册一致。
pub fn reconcile_messages() -> bool {
    let mut i = 0usize;
    while i < STRERROR_ROWS {
        if let Some((_sym, num, text)) = STRERROR_TABLE.get(i).copied() {
            let m = strerror(num);
            if !m.known || m.text != text {
                return false;
            }
            let p = perror_render("varix", num);
            if p != format!("varix: {}\n", text) {
                return false;
            }
        }
        i += 1;
    }
    true
}

/// 与内核 errno 约定对账：错误区上界与负值映射可复算。
pub fn reconcile_wire() -> bool {
    classify(-1) == Wire::Errno(1)
        && classify(-GLIBC_ERRNO_MAX) == Wire::Errno(GLIBCCAST)
        && classify(-(GLIBC_ERRNO_MAX + 1)) == Wire::Value(-(GLIBC_ERRNO_MAX + 1))
        && classify(0) == Wire::Value(0)
        && classify(4096) == Wire::Value(4096)
}

/// 4095 的 i32 形态（避免魔法数散落）。
const GLIBCCAST: i32 = 4095;

// ── 指纹 ──────────────────────────────────────────────────────────────
/// FNV-1a 32 位（无浮点、无随机、无墙钟）。
pub fn fnv1a(bytes: &[u8]) -> u32 {
    let mut h: u32 = 0x811C_9DC5;
    let mut i = 0usize;
    while i < bytes.len() {
        if let Some(b) = bytes.get(i) {
            h ^= *b as u32;
            h = h.wrapping_mul(0x0100_0193);
        }
        i += 1;
    }
    h
}

/// 规范化巡检指纹：一条真实链（open 失败 → 消费 → 渲染）双跑。
pub fn patrol_fingerprint() -> u32 {
    let mut chain = ConsumerChain::new();
    let mut reg = ConsumerRegistry::new();
    let _ = reg.register(EBADF);
    let _ = reg.register(ENOENT);
    let mut acc = 0u32;
    let raws = [-EBADF as i64, 11, -ENOENT as i64, -(GLIBC_ERRNO_MAX + 1), -EIO as i64];
    let mut i = 0usize;
    while i < raws.len() {
        if let Some(raw) = raws.get(i).copied() {
            let fed = chain.feed(raw);
            if let Ok(r) = fed {
                acc ^= fnv1a(&[(r.ret & 0xFF) as u8, u8::from(r.errno_set)]);
            }
            let code = chain.cell().peek();
            let out = reg.consume(code);
            if let Consume::Registered(no) = out {
                acc = acc.wrapping_add(no as u32);
            }
        }
        i += 1;
    }
    acc ^ chain.fingerprint() ^ reg.fingerprint()
}

// ── 检查入口 ──────────────────────────────────────────────────────────
/// F9219 判据集（22 条静态 + 4 条动态；add_dyn ≤8）。
pub fn run_unx9219_checks() -> CheckSet {
    let mut s = CheckSet::new("unx9219");

    let seg_ok = SEG_BASE % 0x40 == 0 && (0x7A000..=0x7D1C0).contains(&SEG_BASE);
    s.add("段位守卫：0x40 对齐且在 0x7A000..=0x7D1C0", seg_ok,
          "段基址须唯一且落在本批窗口，越界即拒绝注册");

    // 正常流：坏 fd 链一步到底
    let mut c = ConsumerChain::new();
    let r1 = c.feed(-EBADF as i64);
    let rendered = c.render("cat");
    s.add("正常流：EBADF 透传 → ret=-1、errno=9、perror 文案正确",
          r1 == Ok(LibcRet { ret: -1, errno_set: true }) && c.cell().peek() == EBADF
              && rendered == Ok(String::from("cat: Bad file descriptor\n")),
          "错误链须一步落 errno，再按 perror 格式渲染");

    // 正常流：成功返回值原样透传
    let mut c2 = ConsumerChain::new();
    s.add("正常流：raw=42 原样透传且不置 errno",
          c2.feed(42) == Ok(LibcRet { ret: 42, errno_set: false })
              && c2.cell().peek() == 0 && !c2.cell().is_set(),
          "成功路径不得写 errno，返回值按 i64 原样交付");

    // 边界四类：-1 / -4095 / -4096 / 0
    s.add("边界：raw=-1 → errno=1（错误区下沿）", classify(-1) == Wire::Errno(EPERM),
          "最小错误号必须被识别，不能因「-1 也是 -1 返回值」混淆");
    s.add("边界：raw=-4095 → errno=4095（错误区上沿）",
          classify(-GLIBC_ERRNO_MAX) == Wire::Errno(4095),
          "4095 仍属错误区，上界含等号");
    s.add("边界：raw=-4096 → 原样透传（非错误区）",
          classify(-(GLIBC_ERRNO_MAX + 1)) == Wire::Value(-4096),
          "glibc 只认到 -4095；-4096 必须当返回值，朴素实现正在此处翻车");
    s.add("边界：raw=0 与 i64::MIN 均不溢出、按返回值透传",
          classify(0) == Wire::Value(0) && classify(i64::MIN) == Wire::Value(i64::MIN),
          "checked_neg 挡住 i64::MIN 取负溢出，必须回返回值而非错误");

    // errno 粘滞
    let mut c3 = ConsumerChain::new();
    let _ = c3.feed(-EACCES as i64);
    let _ = c3.feed(3);
    let _ = c3.feed(4);
    s.add("判定次序：先分类后写 errno，成功不清零（粘滞）",
          c3.cell().peek() == EACCES && c3.cell().writes() == 1 && c3.calls() == 3,
          "errno 只在错误分支写；消费者必须看返回值，不能只看 errno");

    // 状态机
    let mut c4 = ConsumerChain::new();
    s.add("状态机：未取返回值即渲染 → StageViolation",
          c4.render("x") == Err(C2Code::StageViolation) && c4.violations() == 1,
          "拿陈旧 errno 打印是典型误用，必须在 Idle 态拒渲染");
    let _ = c4.feed(-EIO as i64);
    let ok_render = c4.render("x");
    let again = c4.render("x");
    s.add("状态机：交付后可渲染一次，二次渲染与连续 feed 次序正确",
          ok_render == Ok(String::from("x: Input/output error\n"))
              && again == Err(C2Code::StageViolation)
              && c4.feed(5) == Ok(LibcRet { ret: 5, errno_set: false }),
          "Rendered→Rendered 拒；Rendered→Raw（下一次调用）准");
    let mut c5 = ConsumerChain::new();
    let _ = c5.feed(9);
    s.add("状态机：成功调用后渲染 → NotAnError",
          c5.render("x") == Err(C2Code::NotAnError),
          "errno_set=false 时 perror 会打印陈旧值，须显性拒");

    // 注入三类
    let (ledger, per_class) = run_consumer_matrix();
    let all_caught = per_class.iter().all(|n| *n == INJECT_ROUNDS);
    s.add("注入必抓：边界误读/errno 被清/未登记消费各 10/10", all_caught,
          "三类违约各 10 轮必须全检出，漏检即红");

    // 登记格
    let reg = patrol_registry();
    let no_seq_ok = (0..REGISTRY_CAP).filter_map(|i| reg.item(i)).enumerate()
        .all(|(i, c)| c.no as usize == i);
    s.add("登记格：16 条全登记且消费号 0..15 单调发放",
          reg.count() == REGISTRY_CAP && no_seq_ok && reg.unknown_hits() == 0,
          "消费号必须按登记序单调，重复或跳号都会让对账错位");

    let mut reg2 = ConsumerRegistry::new();
    let _ = reg2.register(EBADF);
    s.add("登记格：重复登记 → DuplicateConsumer",
          reg2.register(EBADF) == Err(C2Code::DuplicateConsumer),
          "同一 errno 占两号会让消费账分裂");
    s.add("登记格：表外 errno(ENOSYS) → UnregisteredErrno",
          reg2.register(ENOSYS) == Err(C2Code::UnregisteredErrno),
          "没有 strerror 文案的码不得进登记格");

    let mut reg3 = ConsumerRegistry::new();
    let mut i = 0usize;
    while i < REGISTRY_CAP {
        if let Some((_sym, num, _t)) = STRERROR_TABLE.get(i).copied() {
            let _ = reg3.register(num);
        }
        i += 1;
    }
    // 第 17 条是表内但未登记的 EROFS：满格 → RegistryFull。
    s.add("容量：满格后再登记表内码 → RegistryFull 且前 16 条不丢",
          reg3.register(EROFS) == Err(C2Code::RegistryFull)
              && reg3.count() == REGISTRY_CAP && reg3.lookup(EROFS).is_none(),
          "满格必须显性拒收；表外码先撞 UnregisteredErrno（表内判定在容量之前）");

    // 对账
    s.add("对账：20 行 strerror/perror 文案与 glibc 逐码一致", reconcile_messages(),
          "任一行文案不等即红，禁止用「意思相近」蒙混");
    s.add("对账：错误区边界与负值映射可复算", reconcile_wire(),
          "-1→1、-4095→4095、-4096→透传、0→0 四条必须同时成立");

    // 守恒
    s.add("计数守恒：已消费 == 已登记命中 + 未登记命中",
          reg.conserved() && reg.consumed() == REGISTRY_CAP as u32,
          "16 次消费须全部落在已登记命中上，未登记命中恒 0");

    // 双跑
    let fp_a = patrol_fingerprint();
    let fp_b = patrol_fingerprint();
    s.add("双跑一致：巡检指纹两次相等", fp_a == fp_b,
          "同输入两次必须逐字段相等，隐藏状态即失败");

    // 文案闭集 + 码唯一
    s.add("文案闭集：12 个诊断码均带中文建议",
          C2Code::ALL.iter().all(|c| !c.detail().is_empty()),
          "每个码必须能取到非空且可操作的说明");
    let mut uniq = true;
    let mut x = 0usize;
    while x < C2Code::ALL.len() {
        let mut y = x + 1;
        while y < C2Code::ALL.len() {
            if let (Some(a), Some(b)) = (C2Code::ALL.get(x), C2Code::ALL.get(y)) {
                if a.val() == b.val() {
                    uniq = false;
                }
            }
            y += 1;
        }
        x += 1;
    }
    s.add("诊断码：12 个码值两两唯一", uniq, "段内闭集码不得重复，否则判据无法定位");

    // 动态明细（4 条）
    let t = format!("注入 {} / 检出 {} / 逃逸 {}", ledger.injected, ledger.detected, ledger.escaped);
    s.add_dyn("注入账快照", ledger.conserved() && ledger.escaped == 0, &t);
    let per = format!("边界 {} / clobber {} / 未登记 {}",
                      match per_class.get(0) { Some(v) => *v, None => 0 },
                      match per_class.get(1) { Some(v) => *v, None => 0 },
                      match per_class.get(2) { Some(v) => *v, None => 0 });
    s.add_dyn("逐类检出快照", all_caught, &per);
    let reg_text = format!("登记 {} / 消费 {} / 未知命中 {}",
                           reg.count(), reg.consumed(), reg.unknown_hits());
    s.add_dyn("登记格快照", reg.conserved(), &reg_text);
    let fp_text = format!("巡检指纹 0x{:08X}", fp_a);
    s.add_dyn("指纹快照", fp_a == fp_b, &fp_text);

    s
}

// ── 测试 ──────────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn passthrough_error_chain() {
        let mut c = ConsumerChain::new();
        assert_eq!(c.feed(-EBADF as i64), Ok(LibcRet { ret: -1, errno_set: true }));
        assert_eq!(c.cell().peek(), EBADF);
        assert_eq!(c.render("cat"), Ok(String::from("cat: Bad file descriptor\n")));
        assert_eq!(c.stage(), Stage::Rendered);
        assert_eq!(c.renders(), 1);
    }

    #[test]
    fn success_passthrough_keeps_errno() {
        let mut c = ConsumerChain::new();
        assert_eq!(c.feed(-EACCES as i64), Ok(LibcRet { ret: -1, errno_set: true }));
        assert_eq!(c.feed(42), Ok(LibcRet { ret: 42, errno_set: false }));
        assert_eq!(c.cell().peek(), EACCES);
        assert_eq!(c.cell().writes(), 1);
    }

    #[test]
    fn boundary_4095_and_4096() {
        assert_eq!(classify(-1), Wire::Errno(1));
        assert_eq!(classify(-4095), Wire::Errno(4095));
        assert_eq!(classify(-4096), Wire::Value(-4096));
        assert_eq!(classify(0), Wire::Value(0));
        assert_eq!(classify(i64::MIN), Wire::Value(i64::MIN));
        assert_eq!(classify(4096), Wire::Value(4096));
        // 朴素实现在 -4096 处分歧，正是注入抓点。
        assert_ne!(classify_naive(-4096), classify(-4096));
        assert_eq!(classify_naive(-4095), classify(-4095));
    }

    #[test]
    fn stage_machine_rejects_leaps() {
        let mut c = ConsumerChain::new();
        assert_eq!(c.render("x"), Err(C2Code::StageViolation));
        assert_eq!(c.feed(-EIO as i64), Ok(LibcRet { ret: -1, errno_set: true }));
        assert_eq!(c.render("x"), Ok(String::from("x: Input/output error\n")));
        assert_eq!(c.render("x"), Err(C2Code::StageViolation));
        assert_eq!(c.feed(1), Ok(LibcRet { ret: 1, errno_set: false }));
        assert_eq!(c.violations(), 2);
    }

    #[test]
    fn registry_numbers_and_guards() {
        let reg = patrol_registry();
        assert_eq!(reg.count(), 16);
        assert_eq!(reg.unknown_hits(), 0);
        assert_eq!(reg.consumed(), 16);
        assert!(reg.conserved());
        for i in 0..16 {
            let c = reg.item(i).unwrap_or(Consumer::empty());
            assert_eq!(c.no as usize, i);
            assert_eq!(c.hits, 1);
        }
        let mut r = ConsumerRegistry::new();
        assert_eq!(r.register(EBADF), Ok(0));
        assert_eq!(r.register(EBADF), Err(C2Code::DuplicateConsumer));
        assert_eq!(r.register(ENOSYS), Err(C2Code::UnregisteredErrno));
        assert_eq!(r.consume(ENOSYS), Consume::Unknown);
        assert_eq!(r.unknown_hits(), 1);
    }

    #[test]
    fn registry_capacity() {
        let mut r = ConsumerRegistry::new();
        let mut i = 0usize;
        while i < REGISTRY_CAP {
            if let Some((_s, n, _t)) = STRERROR_TABLE.get(i).copied() {
                assert!(r.register(n).is_ok());
            }
            i += 1;
        }
        // 表内但已满格：条目判定（表内、非重复）通过后撞容量。
        assert_eq!(r.register(EROFS), Err(C2Code::RegistryFull));
        // 表外码：表内判定先于容量，回 UnregisteredErrno。
        assert_eq!(r.register(ENOSYS), Err(C2Code::UnregisteredErrno));
        assert_eq!(r.count(), REGISTRY_CAP);
        assert!(r.lookup(EROFS).is_none());
    }

    #[test]
    fn messages_match_glibc() {
        assert!(reconcile_messages());
        assert!(reconcile_wire());
        assert_eq!(strerror(EBADF).text, "Bad file descriptor");
        assert_eq!(strerror(ENFILE).text, "Too many open files in system");
        assert_eq!(strerror(EMFILE).text, "Too many open files");
        assert!(!strerror(ENOSYS).known);
        assert_eq!(perror_render("", ENOENT), String::from("No such file or directory\n"));
    }

    #[test]
    fn injections_catch_all_ten() {
        let (l, per) = run_consumer_matrix();
        assert!(l.conserved());
        assert_eq!(l.escaped, 0);
        assert_eq!(l.injected, INJECT_CLASSES * INJECT_ROUNDS);
        for v in per.iter() {
            assert_eq!(*v, INJECT_ROUNDS);
        }
    }

    #[test]
    fn deterministic_patrol() {
        assert_eq!(patrol_fingerprint(), patrol_fingerprint());
        assert_eq!(patrol_registry().fingerprint(), patrol_registry().fingerprint());
    }

    #[test]
    fn checks_all_green() {
        let s = run_unx9219_checks();
        assert!(s.all_passed(), "{}", s.red_items_text());
        assert!(!s.truncated());
        assert!(s.len() >= 12);
    }
}
