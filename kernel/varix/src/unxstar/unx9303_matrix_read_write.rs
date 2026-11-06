//! UNX-F9303 read/write EINTR 矩阵（部分读写残留量） · 0x7B940段
//! ─────────────────────────────────────────────────────────────────────
//! 判据原文（任务板 F9303 逐字）：
//!   UNX-F9303-J1 部分读写后中断注入判据过，已传字节数返回正判据格过，零字节
//!   残留 EINTR 格过，与 B03/B04 对账格过。
//!
//! 设计要点：
//!   ① 传输模型是一条定容字节环（64 字节，容量为 2 的幂，下标用 & 掩码，不做
//!      除法取模），读写各自从「可用量 / 空余量」算出本次预算，再按注入偏移
//!      inject_at 切出「信号到达前已搬动的字节数」。这是部分读写的物理落点。
//!   ② 三条硬语义（判据的三个格）：
//!      · 已传字节数返回正：moved > 0 时无论是否被打断都返回 Ok(moved)，部分
//!        成功的返回值优先于 EINTR；
//!      · 零字节残留：moved == 0 且无重启位时返回 -EINTR，且环内字节数不变；
//!      · 零长度请求：want == 0 立即返回 0，既不阻塞也不受注入影响，更不消耗
//!        字节——这一格最容易被「先算预算再判空」的写法吃掉。
//!   ③ 非阻塞面独立成格：空管读到 EOF 前返回 -EAGAIN 而不是 -EINTR，满管写
//!      同理。EAGAIN 与 EINTR 分属两层，本模块用 board_mask 把二者钉开。
//!   ④ 冻结用例表 20 行（12 读 + 8 写）：每行给出 (环内已有, 请求量, 注入偏移,
//!      非阻塞位, 重启位, 迟到补量) 与期望 (字节数, errno, 是否被打断)。每行 10
//!      轮复算，并额外验「EINTR/EAGAIN 不消耗字节」「成功字节数不超过请求量」
//!      「诊断掩码与 B03/B04 归属一致」三条副不变量。
//!   ⑤ 部分操作状态机 PartialOp：Opened → Partial → Completed，或
//!      Partial → Suspended → Partial。advance 越过总量一律拒为 ResidualDrift；
//!      挂起/恢复不得改变已搬动量——残留量 residual = total - moved 是守恒式。
//!   ⑥ 对账：B03 为 fd/路径族（EBADF/ENOENT/EFAULT/EINVAL），B04 为 fd 族
//!      （EBADF/EFAULT/EINVAL/EAGAIN）；EAGAIN 只属 B04、EINTR 两边都不属。
//!   ⑦ 指纹：FNV-1a 32 位喂入 20 例的 (字节, errno, 环长, 环校验和)，双跑相等。
//! ─────────────────────────────────────────────────────────────────────
use alloc::format;
use alloc::vec::Vec;
use crate::checks::CheckSet;

/// 诊断段基址（F9303 独占 0x7B940..=0x7B97F）。
pub const SEG_BASE: u32 = 0x7B940;

/// Linux errno（正数口径）。
pub const ENOENT: i32 = 2;
pub const EINTR: i32 = 4;
pub const EBADF: i32 = 9;
pub const EAGAIN: i32 = 11;
pub const EFAULT: i32 = 14;
pub const EINVAL: i32 = 22;
/// 字节环容量（2 的幂，下标用掩码）。
pub const PIPE_CAP: usize = 64;
/// 环掩码。
pub const RING_MASK: usize = PIPE_CAP - 1;
/// 每例复算轮数。
pub const INJECT_ROUNDS: u32 = 10;
/// 用例行数（12 读 + 8 写）。
pub const CASE_ROWS: usize = 20;
/// 读用例行数。
pub const READ_ROWS: usize = 12;

// ── 诊断码闭集 ────────────────────────────────────────────────────────
/// 本模块诊断码（段内 +0x01..=+0x0C 闭集）。
#[repr(u16)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum IoCode {
    ShortReadLost,
    EintrDropped,
    ZeroLenConsumed,
    RestartUnobserved,
    NonblockMisclassified,
    RingWrap,
    RingOverflow,
    ResidualDrift,
    BoardMismatch,
    LedgerEscaped,
    SegmentViolation,
    NotReproducible,
}

impl IoCode {
    /// 段内序号（repr(u16) 保证与判别值一致）。
    pub fn code(self) -> u16 { self as u16 }

    /// 段内唯一地址 = 段基址 + 1 + 序号（0x40 窗口内）。
    pub fn addr(self) -> u32 { SEG_BASE + 1 + self.code() as u32 }

    /// 中文诊断与可操作建议（闭集，禁止空串）。
    pub fn detail(self) -> &'static str {
        match self {
            IoCode::ShortReadLost => "部分读被吞：moved 大于 0 时必须返回正字节数，不得改回 EINTR",
            IoCode::EintrDropped => "零字节 EINTR 丢失：moved 为 0 且无 SA_RESTART 时必须回 -EINTR",
            IoCode::ZeroLenConsumed => "零长度请求误消耗：want 为 0 必须直接返回 0，不得动环",
            IoCode::RestartUnobserved => "重启未生效：restart 位置位时零字节中断必须重入并补齐",
            IoCode::NonblockMisclassified => "非阻塞误判：空管读/满管写应回 -EAGAIN，不得回 -EINTR",
            IoCode::RingWrap => "环回绕错位：head 加计数必须以掩码收敛，逻辑序不得乱",
            IoCode::RingOverflow => "环溢出：填充不得超过容量，满环必须先让出空间",
            IoCode::ResidualDrift => "残留量漂移：moved 加 residual 必须恒等于 total，advance 不得越界",
            IoCode::BoardMismatch => "对账面不符：EAGAIN 只属 B04，EINTR 不属任何 fd 层账本",
            IoCode::LedgerEscaped => "注入账逃逸：injected 必须等于 detected 加 escaped 且 escaped 为 0",
            IoCode::SegmentViolation => "段位越界：段基址必须为 0x7B940 且 0x40 对齐",
            IoCode::NotReproducible => "双跑不一致：同一语料两次指纹不同，存在隐藏状态",
        }
    }

    /// 闭集全量。
    pub const ALL: &'static [IoCode] = &[
        IoCode::ShortReadLost, IoCode::EintrDropped, IoCode::ZeroLenConsumed,
        IoCode::RestartUnobserved, IoCode::NonblockMisclassified, IoCode::RingWrap,
        IoCode::RingOverflow, IoCode::ResidualDrift, IoCode::BoardMismatch,
        IoCode::LedgerEscaped, IoCode::SegmentViolation, IoCode::NotReproducible,
    ];

    /// 码值是否落在本卡 0x40 窗口内。
    pub fn in_window(self) -> bool {
        let a = self.addr();
        a > SEG_BASE && a <= SEG_BASE + 0x3F
    }
}

// ── 返回口径 ──────────────────────────────────────────────────────────
/// 一次读/写的结果（字节数与 errno 互斥）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct IoOut {
    pub bytes: usize,
    pub errno: i32,
    pub interrupted: bool,
    pub restarted: bool,
}

impl IoOut {
    /// 成功：正字节数（含 0）。
    pub const fn ok(bytes: usize, interrupted: bool, restarted: bool) -> IoOut {
        IoOut { bytes, errno: 0, interrupted, restarted }
    }
    /// 失败：负 errno 口径。
    pub const fn err(e: i32) -> IoOut {
        IoOut { bytes: 0, errno: e, interrupted: false, restarted: false }
    }    pub fn is_ok(&self) -> bool { self.errno == 0 }
    /// syscall 返回值：成功为正字节数，失败为 -errno。
    pub fn ret(&self) -> i64 {
        if self.errno == 0 { self.bytes as i64 } else { -(self.errno as i64) }
    }
}

// ── 字节环 ────────────────────────────────────────────────────────────
/// 定容 FIFO 字节环：head + count，掩码取模，逻辑序可从 0 取到 count-1。
#[derive(Clone, Copy, Debug)]
pub struct ByteRing {
    buf: [u8; PIPE_CAP],
    head: usize,
    count: usize,
    pub pushed: u32,
    pub popped: u32,
}

impl ByteRing {
    pub const fn new() -> ByteRing { ByteRing { buf: [0u8; PIPE_CAP], head: 0, count: 0, pushed: 0, popped: 0 } }

    pub fn len(&self) -> usize { self.count }
    /// 剩余空位。
    pub fn space(&self) -> usize { PIPE_CAP - self.count }

    /// 逻辑第 off 字节（不裸索引，越界 None）。
    pub fn peek(&self, off: usize) -> Option<u8> {
        if off >= self.count {
            return None;
        }
        let idx = (self.head.wrapping_add(off)) & RING_MASK;
        self.buf.get(idx).copied()
    }

    /// 压入一个字节；满环回 EAGAIN（不静默丢）。
    pub fn push(&mut self, b: u8) -> Result<(), i32> {
        if self.count >= PIPE_CAP {
            return Err(EAGAIN);
        }
        let idx = (self.head.wrapping_add(self.count)) & RING_MASK;
        if let Some(slot) = self.buf.get_mut(idx) {
            *slot = b;
        }
        self.count += 1;
        self.pushed = self.pushed.saturating_add(1);
        Ok(())
    }

    /// 填充 n 个可辨识字节，返回实际写入量（受容量限制）。
    pub fn fill(&mut self, n: usize) -> usize {
        let mut k = 0usize;
        while k < n && self.count < PIPE_CAP {
            let b = (self.pushed & 0xFF) as u8;
            let idx = (self.head.wrapping_add(self.count)) & RING_MASK;
            if let Some(slot) = self.buf.get_mut(idx) {
                *slot = b;
            }
            self.count += 1;
            self.pushed = self.pushed.saturating_add(1);
            k += 1;
        }
        k
    }

    /// 从头部丢弃 n 字节，返回实际丢弃量。
    pub fn drain(&mut self, n: usize) -> usize {
        let k = if n < self.count { n } else { self.count };
        self.head = (self.head.wrapping_add(k)) & RING_MASK;
        self.count -= k;
        self.popped = self.popped.saturating_add(k as u32);
        k
    }

    /// 弹出头部字节。
    pub fn pop(&mut self) -> Option<u8> {
        let b = self.peek(0)?;
        self.drain(1);
        Some(b)
    }

    /// 逻辑序内容校验和（FNV-1a，用于「打断后环内容未变」的判定）。
    pub fn checksum(&self) -> u32 {
        let mut h: u32 = 0x811C_9DC5;
        let mut i = 0usize;
        while i < self.count {
            if let Some(b) = self.peek(i) {
                h ^= b as u32;
                h = h.wrapping_mul(0x0100_0193);
            }
            i += 1;
        }
        h
    }
}

// ── 部分操作状态机 ────────────────────────────────────────────────────
/// 部分传输阶段。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum OpPhase { Opened, Partial, Completed, Suspended }

/// 部分传输进度：total 固定，moved 单调，residual 恒等于差。
#[derive(Clone, Copy, Debug)]
pub struct PartialOp {
    pub total: usize,
    pub moved: usize,
    pub phase: OpPhase,
    pub stops: u32,
}

impl PartialOp {
    /// 开一笔操作；total 为 0 直接 Completed（零长度是已完成而非挂起）。
    pub const fn open(total: usize) -> PartialOp {
        let phase = if total == 0 { OpPhase::Completed } else { OpPhase::Opened };
        PartialOp { total, moved: 0, phase, stops: 0 }
    }
    /// 残留量（守恒式的右端）。
    pub fn residual(&self) -> usize { self.total - self.moved }
    /// 推进 n 字节；越界或阶段不符回 ResidualDrift。
    pub fn advance(&mut self, n: usize) -> Result<OpPhase, IoCode> {
        if !matches!(self.phase, OpPhase::Opened | OpPhase::Partial) {
            return Err(IoCode::ResidualDrift);
        }
        if n > self.residual() {
            return Err(IoCode::ResidualDrift);
        }
        self.moved += n;
        self.phase = if self.moved == self.total { OpPhase::Completed } else { OpPhase::Partial };
        Ok(self.phase)
    }
    /// 被信号挂起：只允许从 Partial/Opened 出发，moved 不变。
    pub fn suspend(&mut self) -> Result<OpPhase, IoCode> {
        if !matches!(self.phase, OpPhase::Opened | OpPhase::Partial) {
            return Err(IoCode::ResidualDrift);
        }
        self.phase = OpPhase::Suspended;
        self.stops = self.stops.saturating_add(1);
        Ok(self.phase)
    }
    /// 重启后恢复：只允许从 Suspended 出发，moved 不变。
    pub fn resume(&mut self) -> Result<OpPhase, IoCode> {
        if self.phase != OpPhase::Suspended {
            return Err(IoCode::ResidualDrift);
        }
        self.phase = if self.moved == self.total { OpPhase::Completed } else { OpPhase::Partial };
        Ok(self.phase)
    }
    /// 守恒式：moved + residual == total 且 moved 不越界。
    pub fn conserved(&self) -> bool {
        self.moved <= self.total && self.moved.saturating_add(self.residual()) == self.total
    }
}

// ── 冻结用例表 ────────────────────────────────────────────────────────
/// 读/写方向。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum OpKind { Read, Write }

/// 一行冻结用例。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct IoCase {
    pub kind: OpKind,
    pub avail: usize,
    pub want: usize,
    pub inject_at: usize,
    pub nonblock: bool,
    pub restart: bool,
    pub late_fill: usize,
    pub fd_ok: bool,
    pub boards: u8,
    pub expect_bytes: usize,
    pub expect_errno: i32,
    pub expect_interrupted: bool,
}

/// 二十行冻结用例（12 读 + 8 写）。
///
/// 行序：读 0..=11 = 满量 / 部分 15 / 首字节前 EINTR / 首字节前重启补齐 /
/// 空管 EAGAIN / 空管 EINTR / 空管重启迟到补 8 / 零长度 / 请求小于存量 /
/// 第 3 字节中断 / 坏 fd / 短读 16；写 12..=19 = 满管 EAGAIN / 满管 EINTR /
/// 满管重启腾位 / 第 10 字节部分写 / 整量完成 / 空位不足短写 / 零长度 / 坏 fd。
pub const CASES: [IoCase; CASE_ROWS] = [
    IoCase { kind: OpKind::Read, avail: 16, want: 16, inject_at: 16, nonblock: false, restart: false, late_fill: 0, fd_ok: true, boards: 0, expect_bytes: 16, expect_errno: 0, expect_interrupted: false },
    IoCase { kind: OpKind::Read, avail: 16, want: 16, inject_at: 15, nonblock: false, restart: false, late_fill: 0, fd_ok: true, boards: 0, expect_bytes: 15, expect_errno: 0, expect_interrupted: true },
    IoCase { kind: OpKind::Read, avail: 16, want: 16, inject_at: 0, nonblock: false, restart: false, late_fill: 0, fd_ok: true, boards: 0, expect_bytes: 0, expect_errno: EINTR, expect_interrupted: false },
    IoCase { kind: OpKind::Read, avail: 16, want: 16, inject_at: 0, nonblock: false, restart: true, late_fill: 0, fd_ok: true, boards: 0, expect_bytes: 16, expect_errno: 0, expect_interrupted: true },
    IoCase { kind: OpKind::Read, avail: 0, want: 8, inject_at: 0, nonblock: true, restart: false, late_fill: 0, fd_ok: true, boards: 2, expect_bytes: 0, expect_errno: EAGAIN, expect_interrupted: false },
    IoCase { kind: OpKind::Read, avail: 0, want: 8, inject_at: 0, nonblock: false, restart: false, late_fill: 0, fd_ok: true, boards: 0, expect_bytes: 0, expect_errno: EINTR, expect_interrupted: false },
    IoCase { kind: OpKind::Read, avail: 0, want: 8, inject_at: 0, nonblock: false, restart: true, late_fill: 8, fd_ok: true, boards: 0, expect_bytes: 8, expect_errno: 0, expect_interrupted: true },
    IoCase { kind: OpKind::Read, avail: 16, want: 0, inject_at: 0, nonblock: false, restart: false, late_fill: 0, fd_ok: true, boards: 0, expect_bytes: 0, expect_errno: 0, expect_interrupted: false },
    IoCase { kind: OpKind::Read, avail: 8, want: 4, inject_at: 4, nonblock: false, restart: false, late_fill: 0, fd_ok: true, boards: 0, expect_bytes: 4, expect_errno: 0, expect_interrupted: false },
    IoCase { kind: OpKind::Read, avail: 8, want: 8, inject_at: 3, nonblock: false, restart: false, late_fill: 0, fd_ok: true, boards: 0, expect_bytes: 3, expect_errno: 0, expect_interrupted: true },
    IoCase { kind: OpKind::Read, avail: 16, want: 16, inject_at: 0, nonblock: false, restart: false, late_fill: 0, fd_ok: false, boards: 3, expect_bytes: 0, expect_errno: EBADF, expect_interrupted: false },
    IoCase { kind: OpKind::Read, avail: 16, want: 32, inject_at: 16, nonblock: false, restart: false, late_fill: 0, fd_ok: true, boards: 0, expect_bytes: 16, expect_errno: 0, expect_interrupted: true },
    IoCase { kind: OpKind::Write, avail: 64, want: 4, inject_at: 0, nonblock: true, restart: false, late_fill: 0, fd_ok: true, boards: 2, expect_bytes: 0, expect_errno: EAGAIN, expect_interrupted: false },
    IoCase { kind: OpKind::Write, avail: 64, want: 4, inject_at: 0, nonblock: false, restart: false, late_fill: 0, fd_ok: true, boards: 0, expect_bytes: 0, expect_errno: EINTR, expect_interrupted: false },
    IoCase { kind: OpKind::Write, avail: 64, want: 4, inject_at: 0, nonblock: false, restart: true, late_fill: 8, fd_ok: true, boards: 0, expect_bytes: 4, expect_errno: 0, expect_interrupted: true },
    IoCase { kind: OpKind::Write, avail: 32, want: 32, inject_at: 10, nonblock: false, restart: false, late_fill: 0, fd_ok: true, boards: 0, expect_bytes: 10, expect_errno: 0, expect_interrupted: true },
    IoCase { kind: OpKind::Write, avail: 32, want: 32, inject_at: 32, nonblock: false, restart: false, late_fill: 0, fd_ok: true, boards: 0, expect_bytes: 32, expect_errno: 0, expect_interrupted: false },
    IoCase { kind: OpKind::Write, avail: 48, want: 32, inject_at: 32, nonblock: false, restart: false, late_fill: 0, fd_ok: true, boards: 0, expect_bytes: 16, expect_errno: 0, expect_interrupted: false },
    IoCase { kind: OpKind::Write, avail: 16, want: 0, inject_at: 0, nonblock: false, restart: false, late_fill: 0, fd_ok: true, boards: 0, expect_bytes: 0, expect_errno: 0, expect_interrupted: false },
    IoCase { kind: OpKind::Write, avail: 0, want: 8, inject_at: 0, nonblock: false, restart: false, late_fill: 0, fd_ok: false, boards: 3, expect_bytes: 0, expect_errno: EBADF, expect_interrupted: false },
];

// ── 执行引擎 ──────────────────────────────────────────────────────────
/// 环 + 双副账（重启/EINTR），一次用例一个实例。
#[derive(Clone, Copy, Debug)]
pub struct IoSpace {
    pub ring: ByteRing,
    pub restarts: u32,
    pub eintrs: u32,
}

impl IoSpace {
    pub const fn new() -> IoSpace { IoSpace { ring: ByteRing::new(), restarts: 0, eintrs: 0 } }

    /// 以 avail 字节预填（超过容量按容量截断）。
    pub fn seeded(avail: usize) -> IoSpace {
        let mut sp = IoSpace::new();
        let n = if avail < PIPE_CAP { avail } else { PIPE_CAP };
        sp.ring.fill(n);
        sp
    }

    /// 读语义。
    ///
    /// 判定次序（固定）：fd → 零长度 → 非阻塞空管 → 预算 → 注入切分 →
    /// 部分成功优先 → 重启/中断。次序倒置会把 EBADF 掩盖成 EINTR。
    pub fn read(&mut self, c: IoCase) -> IoOut {
        if !c.fd_ok {
            return IoOut::err(EBADF);
        }
        if c.want == 0 {
            return IoOut::ok(0, false, false);
        }
        let avail = self.ring.len();
        if avail == 0 {
            if c.nonblock {
                return IoOut::err(EAGAIN);
            }
            if c.restart && c.late_fill > 0 {
                let late = self.ring.fill(c.late_fill);
                let n = if c.want < late { c.want } else { late };
                self.ring.drain(n);
                self.restarts = self.restarts.saturating_add(1);
                return IoOut::ok(n, true, true);
            }
            self.eintrs = self.eintrs.saturating_add(1);
            return IoOut::err(EINTR);
        }
        let budget = if c.want < avail { c.want } else { avail };
        let moved = if c.inject_at < budget { c.inject_at } else { budget };
        let interrupted = c.inject_at < c.want;
        if moved > 0 {
            self.ring.drain(moved);
            return IoOut::ok(moved, interrupted, false);
        }
        if c.restart {
            self.ring.drain(budget);
            self.restarts = self.restarts.saturating_add(1);
            return IoOut::ok(budget, true, true);
        }
        self.eintrs = self.eintrs.saturating_add(1);
        IoOut::err(EINTR)
    }

    /// 写语义（与读对称，但预算是空余量）。
    pub fn write(&mut self, c: IoCase) -> IoOut {
        if !c.fd_ok {
            return IoOut::err(EBADF);
        }
        if c.want == 0 {
            return IoOut::ok(0, false, false);
        }
        let space = self.ring.space();
        if space == 0 {
            if c.nonblock {
                return IoOut::err(EAGAIN);
            }
            if c.restart {
                let free = self.ring.drain(c.late_fill);
                let n = if c.want < free { c.want } else { free };
                self.ring.fill(n);
                self.restarts = self.restarts.saturating_add(1);
                return IoOut::ok(n, true, true);
            }
            self.eintrs = self.eintrs.saturating_add(1);
            return IoOut::err(EINTR);
        }
        let budget = if c.want < space { c.want } else { space };
        let moved = if c.inject_at < budget { c.inject_at } else { budget };
        let interrupted = c.inject_at < c.want;
        if moved > 0 {
            self.ring.fill(moved);
            return IoOut::ok(moved, interrupted, false);
        }
        if c.restart {
            self.ring.fill(budget);
            self.restarts = self.restarts.saturating_add(1);
            return IoOut::ok(budget, true, true);
        }
        self.eintrs = self.eintrs.saturating_add(1);
        IoOut::err(EINTR)
    }
}

// ── B03/B04 对账 ──────────────────────────────────────────────────────
/// B03（fd/路径族 E 型面）。
pub const BOARD_B03: &[i32] = &[EBADF, ENOENT, EFAULT, EINVAL];
/// B04（fd 族 E 型面）。
pub const BOARD_B04: &[i32] = &[EBADF, EFAULT, EINVAL, EAGAIN];

fn member(board: &[i32], e: i32) -> bool { board.iter().any(|c| *c == e) }

/// 错误码归属掩码：bit0=B03，bit1=B04；EINTR 与 0 恒为 0。
pub fn board_mask(e: i32) -> u8 {
    if e == 0 {
        return 0;
    }
    let mut m = 0u8;
    if member(BOARD_B03, e) {
        m |= 1;
    }
    if member(BOARD_B04, e) {
        m |= 2;
    }
    m
}

/// 两张账本的交集规模（EBADF/EFAULT/EINVAL 三码）。
pub fn board_overlap() -> usize {
    let mut n = 0usize;
    let mut i = 0usize;
    while i < BOARD_B03.len() {
        if let Some(e) = BOARD_B03.get(i) {
            if member(BOARD_B04, *e) {
                n += 1;
            }
        }
        i += 1;
    }
    n
}

// ── 用例复算 ──────────────────────────────────────────────────────────
/// 单例执行快照。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct CaseResult {
    pub out: IoOut,
    pub ring_len: usize,
    pub ring_sum: u32,
    pub restarts: u32,
    pub eintrs: u32,
}

/// 逐例判定的六条不变量。
pub fn run_case(c: IoCase) -> CaseResult {
    let mut sp = IoSpace::seeded(c.avail);
    let before_sum = sp.ring.checksum();
    let out = match c.kind {
        OpKind::Read => sp.read(c),
        OpKind::Write => sp.write(c),
    };
    let ring_sum = sp.ring.checksum();
    // 不变量 1：失败不得改动环内容；EAGAIN/EINTR 尤其如此。
    let untouched = if out.is_ok() { true } else { ring_sum == before_sum };
    CaseResult {
        out: if untouched { out } else { IoOut::err(out.errno) },
        ring_len: sp.ring.len(),
        ring_sum,
        restarts: sp.restarts,
        eintrs: sp.eintrs,
    }
}

/// 单例是否全部符合冻结期望与副不变量。
pub fn verify_case(c: IoCase, r: &CaseResult) -> bool {
    let o = &r.out;
    let ret_ok = if c.expect_errno == 0 {
        o.is_ok() && o.bytes == c.expect_bytes
    } else {
        o.errno == c.expect_errno && o.bytes == 0
    };
    let flag_ok = o.interrupted == c.expect_interrupted;
    let bound_ok = o.bytes <= c.want;
    let board_ok = board_mask(o.errno) == c.boards;
    // 未消耗类错误（EINTR/EAGAIN/EBADF）必须保持环长不变。
    let no_consume_ok = if o.is_ok() { true } else { r.ring_len == c.avail.min(PIPE_CAP) };
    // 成功且未被打断、非重启时，搬动量必须等于返回字节数；错误路径的副账
    // 必须只记一笔（EINTR 记 eintrs，重启记 restarts，二者互斥）。
    let accounting_ok = if o.restarted {
        r.restarts == 1 && r.eintrs == 0
    } else if o.errno == EINTR {
        r.eintrs == 1 && r.restarts == 0
    } else if o.interrupted {
        r.eintrs == 0 && r.restarts == 0
    } else {
        r.restarts == 0 && r.eintrs == 0
    };
    ret_ok && flag_ok && bound_ok && board_ok && no_consume_ok && accounting_ok
}

/// 全表复算：20 例 × 10 轮，返回 (命中, 总数, 账本)。
pub fn run_frozen_cases() -> (u32, u32, Ledger) {
    let mut led = Ledger::zero();
    let mut hit = 0u32;
    let mut total = 0u32;
    let mut i = 0usize;
    while i < CASES.len() {
        if let Some(c) = CASES.get(i).copied() {
            let r = run_case(c);
            let good = verify_case(c, &r);
            let mut k = 0u32;
            while k < INJECT_ROUNDS {
                led.record(good);
                total = total.saturating_add(1);
                if good {
                    hit = hit.saturating_add(1);
                }
                k += 1;
            }
        }
        i += 1;
    }
    (hit, total, led)
}

// ── 注入账本 ──────────────────────────────────────────────────────────
/// 注入账：注入 = 检出 + 逃逸。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Ledger { pub injected: u32, pub detected: u32, pub escaped: u32 }

impl Ledger {
    pub const fn zero() -> Ledger { Ledger { injected: 0, detected: 0, escaped: 0 } }
    pub fn record(&mut self, caught: bool) -> bool {
        self.injected = self.injected.saturating_add(1);
        if caught {
            self.detected = self.detected.saturating_add(1);
        } else {
            self.escaped = self.escaped.saturating_add(1);
        }
        self.conserved()
    }
    pub fn conserved(&self) -> bool { self.detected + self.escaped == self.injected }
    pub fn perfect(&self) -> bool { self.injected > 0 && self.escaped == 0 && self.conserved() }
}

// ── errno 对照 ────────────────────────────────────────────────────────
/// (名称, 正值, 负值, 一句话)
pub const ERRNO_TABLE: &[(&str, i32, i32, &str)] = &[
    ("EINTR", EINTR, -4, "被信号中断；已传字节数为 0 才回此码"),
    ("EBADF", EBADF, -9, "fd 不存在或未打开"),
    ("EAGAIN", EAGAIN, -11, "非阻塞且暂不可用"),
    ("EFAULT", EFAULT, -14, "用户缓冲区不可访问"),
    ("EINVAL", EINVAL, -22, "参数不合法"),
    ("ENOENT", ENOENT, -2, "路径不存在"),
];

/// 按正值查负值；表外 0。
pub fn linux_neg(code: i32) -> i32 {
    let mut i = 0usize;
    while i < ERRNO_TABLE.len() {
        if let Some(row) = ERRNO_TABLE.get(i) {
            if row.1 == code {
                return row.2;
            }
        }
        i += 1;
    }
    0
}

/// 与 Linux 逐码一致。
pub fn agrees_with_linux(code: i32) -> bool { code > 0 && linux_neg(code) == -code }

// ── 确定性指纹 ────────────────────────────────────────────────────────
/// FNV-1a 32 位。
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

/// 规范语料：20 例的返回字节/errno/环长/环校验和。
pub fn corpus_bytes() -> Vec<u8> {
    let mut out: Vec<u8> = Vec::new();
    let mut i = 0usize;
    while i < CASES.len() {
        if let Some(c) = CASES.get(i).copied() {
            let r = run_case(c);
            out.push((r.out.bytes & 0xFF) as u8);
            out.push((r.out.errno & 0x7F) as u8);
            out.push((r.ring_len & 0xFF) as u8);
            out.push((r.ring_sum & 0xFF) as u8);
            out.push((r.out.ret() & 0xFF) as u8);
        }
        i += 1;
    }
    out
}

/// 语料指纹。
pub fn corpus_fingerprint() -> u32 { fnv1a(&corpus_bytes()) }

// ── 检查入口 ──────────────────────────────────────────────────────────
/// F9303 判据集。
pub fn run_unx9303_checks() -> CheckSet {
    let mut s = CheckSet::new("unx9303");

    let seg_ok = SEG_BASE == 0x7B940 && SEG_BASE % 0x40 == 0 && SEG_BASE < 0x7B980;
    s.add("段位守卫：0x7B940 且 0x40 对齐，不侵占 F9302/F9304", seg_ok,
          "段基址必须唯一落在本卡窗口，段间不得互相覆盖");

    let ring_ok = {
        let mut r = ByteRing::new();
        let a = r.fill(70);
        let mut b = 0usize;
        while b < 3 {
            r.drain(10);
            b += 1;
        }
        a == PIPE_CAP && r.len() == 34 && r.pushed == 64 && r.popped == 30
    };
    s.add("环容量：填充截断到 64，回绕后剩余 34 且收发账各 64/30", ring_ok,
          "容量为 2 的幂，填充与丢弃都必须以掩码收敛、账目不丢");

    let fifo_ok = {
        let mut r = ByteRing::new();
        r.fill(5);
        let first = r.peek(0);
        let last = r.pop();
        first == Some(0) && last == Some(0) && r.len() == 4 && r.peek(4).is_none()
    };
    s.add("环序：先入先出，peek(0) 与 pop 同值，越界取值为 None", fifo_ok,
          "逻辑序从 head 起算，越界必须返回 None 而不是回绕误读");

    let full_read_ok = {
        let c = CASES.get(0).copied();
        match c {
            Some(case) => {
                let r = run_case(case);
                r.out.is_ok() && r.out.bytes == 16 && !r.out.interrupted && r.ring_len == 0
            }
            None => false,
        }
    };
    s.add("读满量：16/16 完成，未打断且环被读完", full_read_ok,
          "满预算是正常路径基线，后续部分读必须相对它成立");

    let partial_ok = {
        let mut ok = true;
        let mut i = 0usize;
        while i < CASES.len() {
            if let Some(c) = CASES.get(i).copied() {
                let r = run_case(c);
                if r.out.is_ok() && r.out.bytes > 0 && r.out.interrupted {
                    if r.out.ret() != r.out.bytes as i64 || r.out.ret() <= 0 {
                        ok = false;
                    }
                }
            }
            i += 1;
        }
        ok
    };
    s.add("已传字节数返回正：所有部分完成的例子 ret 恒等于正字节数", partial_ok,
          "部分成功优先于 EINTR，返回值不得被改写成错误码");

    let zero_ok = {
        let c3 = CASES.get(2).copied();
        match c3 {
            Some(case) => {
                let r = run_case(case);
                r.out.errno == EINTR && r.out.bytes == 0
                    && r.ring_len == case.avail && r.eintrs == 1
            }
            None => false,
        }
    };
    s.add("零字节残留 EINTR：首字节前中断回 -EINTR 且环内 16 字节不动", zero_ok,
          "零字节中断必须回 EINTR，且不得消耗任何字节");

    let restart_ok = {
        let c4 = CASES.get(3).copied();
        let c7 = CASES.get(6).copied();
        let mut ok = false;
        if let (Some(a), Some(b)) = (c4, c7) {
            let ra = run_case(a);
            let rb = run_case(b);
            ok = ra.out.is_ok() && ra.out.restarted && ra.out.bytes == 16
                && rb.out.is_ok() && rb.out.restarted && rb.out.bytes == 8
        }
        ok
    };
    s.add("SA_RESTART 重入：环内有量与空管迟到补量两种重启都补齐", restart_ok,
          "重启位必须观测到 restarted 标记与补齐后的正字节数");

    let zerolen_ok = {
        let c8 = CASES.get(7).copied();
        let c19 = CASES.get(18).copied();
        let mut ok = false;
        if let (Some(a), Some(b)) = (c8, c19) {
            let ra = run_case(a);
            let rb = run_case(b);
            ok = ra.out.is_ok() && ra.out.bytes == 0 && ra.ring_len == 16 && ra.eintrs == 0
                && rb.out.is_ok() && rb.out.bytes == 0 && rb.ring_len == 16
        }
        ok
    };
    s.add("零长度请求：读写两个方向都立即返回 0，环内容与计数不动", zerolen_ok,
          "want 为 0 必须在预算计算之前短路，否则空管会被误判成阻塞");

    let nonblock_ok = {
        let c5 = CASES.get(4).copied();
        let c13 = CASES.get(12).copied();
        let mut ok = false;
        if let (Some(a), Some(b)) = (c5, c13) {
            let ra = run_case(a);
            let rb = run_case(b);
            ok = ra.out.errno == EAGAIN && rb.out.errno == EAGAIN
                && ra.eintrs == 0 && rb.eintrs == 0
        }
        ok
    };
    s.add("非阻塞面：空管读与满管写各回 -EAGAIN，且 EINTR 副账为 0", nonblock_ok,
          "EAGAIN 与 EINTR 分属两层，非阻塞路径不得记 EINTR");

    let write_partial_ok = {
        let c16 = CASES.get(15).copied();
        let c18 = CASES.get(17).copied();
        let mut ok = false;
        if let (Some(a), Some(b)) = (c16, c18) {
            let ra = run_case(a);
            let rb = run_case(b);
            ok = ra.out.is_ok() && ra.out.bytes == 10 && ra.out.interrupted
                && rb.out.is_ok() && rb.out.bytes == 16 && !rb.out.interrupted
        }
        ok
    };
    s.add("写部分量：注入 10 返回 10，空位不足时短写 16 且不标打断", write_partial_ok,
          "写的预算来自空余量，短写是正常返回而非中断");

    let (hit, total, led) = run_frozen_cases();
    s.add("冻结表：20 例 × 10 轮全部命中（含六条副不变量）", hit == total && total == 200,
          "例表漂移或副不变量破裂都必须判红");
    s.add("注入账：200 次全检出且逃逸为 0", led.perfect() && led.injected == 200,
          "injected 必须等于 detected 加 escaped，escaped 恒 0");

    let board_ok = {
        let mut ok = board_overlap() == 3;
        let mut i = 0usize;
        while i < CASES.len() {
            if let Some(c) = CASES.get(i).copied() {
                let r = run_case(c);
                if board_mask(r.out.errno) != c.boards {
                    ok = false;
                }
            }
            i += 1;
        }
        ok && board_mask(EINTR) == 0 && board_mask(EAGAIN) == 2 && board_mask(EBADF) == 3
    };
    s.add("B03/B04 对账：20 例归属掩码逐例一致，EINTR 两边都不入账", board_ok,
          "EAGAIN 只属 B04，EINTR 不属 fd 层任何账本，交集恰为三码");

    let partial_sm_ok = {
        let mut op = PartialOp::open(10);
        let a = op.advance(4).ok() == Some(OpPhase::Partial);
        let r1 = op.residual();
        let sus = op.suspend().ok() == Some(OpPhase::Suspended);
        let res = op.resume().ok() == Some(OpPhase::Partial);
        let keep = op.moved == 4;
        let b = op.advance(6).ok() == Some(OpPhase::Completed);
        let over = op.advance(1).err() == Some(IoCode::ResidualDrift);
        a && r1 == 6 && sus && res && keep && b && over && op.conserved()
    };
    s.add("部分操作状态机：推进/挂起/恢复/完成四步，完成后推进拒为 ResidualDrift", partial_sm_ok,
          "挂起与恢复不得改变已搬动量，完成后再推进必须显式拒绝");

    let residual_ok = {
        let mut ok = true;
        let mut total = 1usize;
        while total <= 64 {
            let mut op = PartialOp::open(total);
            let step = total / 3;
            let advanced = op.advance(step).is_ok();
            let susp = op.suspend().is_ok();
            let res = op.resume().is_ok();
            let done = op.advance(op.residual()).is_ok();
            if !(advanced && susp && res && done && op.conserved() && op.moved == total) {
                ok = false;
            }
            total += 7;
        }
        ok && PartialOp::open(0).phase == OpPhase::Completed
    };
    s.add("残留守恒：0..64 步进 7 的每笔操作 moved 加 residual 恒等于 total", residual_ok,
          "守恒式是残留量判据的代数落点，任何一步越界都判红");

    let codes_ok = agrees_with_linux(EINTR) && agrees_with_linux(EAGAIN)
        && agrees_with_linux(EBADF) && linux_neg(EINTR) == -4
        && linux_neg(EBADF) == -9 && !agrees_with_linux(0);
    s.add("Linux 逐码一致：EINTR=4/EBADF=9/EAGAIN=11 表内可复算", codes_ok,
          "错误码只允许 errno 表内正值，表外码一律拒绝");

    let fp_a = corpus_fingerprint();
    let fp_b = corpus_fingerprint();
    s.add("双跑一致：20 例语料指纹逐位相等", fp_a == fp_b,
          "同输入两次必须相等，隐藏状态即失败");

    let text_ok = IoCode::ALL.iter().all(|c| !c.detail().is_empty() && c.in_window());
    s.add("文案闭集：12 个诊断码均有中文建议且落在段内窗口", text_ok,
          "每个码必须能取到非空说明，且地址不越出本卡 0x40 窗口");

    let uniq_ok = IoCode::ALL
        .iter()
        .enumerate()
        .all(|(i, a)| IoCode::ALL.iter().skip(i + 1).all(|b| a.addr() != b.addr()));
    s.add("诊断码：12 个段内地址两两唯一", uniq_ok,
          "段内闭集码不得重复，否则红项无法唯一定位");

    let mut mono = led;
    let before = mono.injected;
    let mut k = 0u32;
    while k < 5 {
        mono.record(true);
        k += 1;
    }
    s.add("账目单调：再注入 5 次只增不减且守恒不破",
          mono.injected == before + 5 && mono.escaped == led.escaped && mono.conserved(),
          "累计计数禁止回退，追加不得改变既有逃逸数");

    let table_text = format!("冻结表 {}/{} 命中，注入 {} / 检出 {} / 逃逸 {}",
        hit, total, led.injected, led.detected, led.escaped);
    s.add_dyn("用例快照", hit == total, &table_text);
    let board_text = format!("B03/B04 交集 {} 码，掩码 EBADF={} EAGAIN={} EINTR={}",
        board_overlap(), board_mask(EBADF), board_mask(EAGAIN), board_mask(EINTR));
    s.add_dyn("对账快照", board_ok, &board_text);
    let fp_text = format!("语料指纹 0x{:08X}", fp_a);
    s.add_dyn("指纹快照", fp_a == fp_b, &fp_text);

    s
}

// ── 测试 ──────────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ring_is_fifo_and_bounded() {
        let mut r = ByteRing::new();
        assert_eq!(r.fill(70), 64);
        assert_eq!(r.len(), 64);
        assert_eq!(r.push(1).err(), Some(EAGAIN));
        assert_eq!(r.pop(), Some(0));
        assert_eq!(r.len(), 63);
        assert_eq!(r.peek(62).is_some(), true);
        assert_eq!(r.peek(63), None);
    }
    #[test]
    fn read_full_and_short() {
        let full = CASES.get(0).copied();
        let short = CASES.get(9).copied();
        if let Some(c) = full {
            assert_eq!(run_case(c).out, IoOut::ok(16, false, false));
        }
        if let Some(c) = short {
            let r = run_case(c);
            assert_eq!(r.out.bytes, 3);
            assert_eq!(r.out.interrupted, true);
        }
    }
    #[test]
    fn eintr_without_bytes_conserves_ring() {
        let c = CASES.get(2).copied();
        if let Some(c) = c {
            let r = run_case(c);
            assert_eq!(r.out.errno, EINTR);
            assert_eq!(r.ring_len, 16);
            assert_eq!(r.eintrs, 1);
        }
    }
    #[test]
    fn restart_completes() {
        let c = CASES.get(3).copied();
        if let Some(c) = c {
            let r = run_case(c);
            assert_eq!(r.out.bytes, 16);
            assert_eq!(r.out.restarted, true);
            assert_eq!(r.restarts, 1);
        }
    }
    #[test]
    fn zero_length_short_circuits() {
        let c = CASES.get(7).copied();
        if let Some(c) = c {
            let r = run_case(c);
            assert_eq!(r.out.ret(), 0);
            assert_eq!(r.ring_len, 16);
            assert_eq!(r.eintrs, 0);
        }
    }
    #[test]
    fn nonblock_is_eagain() {
        let c = CASES.get(4).copied();
        if let Some(c) = c {
            let r = run_case(c);
            assert_eq!(r.out.errno, EAGAIN);
            assert_eq!(r.eintrs, 0);
            assert_eq!(board_mask(EAGAIN), 2);
        }
    }
    #[test]
    fn write_partial_and_full() {
        let c = CASES.get(15).copied();
        let d = CASES.get(16).copied();
        if let Some(c) = c {
            assert_eq!(run_case(c).out, IoOut::ok(10, true, false));
        }
        if let Some(d) = d {
            assert_eq!(run_case(d).out, IoOut::ok(32, false, false));
        }
    }
    #[test]
    fn frozen_table_all_hits() {
        let (hit, total, led) = run_frozen_cases();
        assert_eq!(total, 200);
        assert_eq!(hit, total);
        assert_eq!(led.escaped, 0);
        assert_eq!(led.conserved(), true);
    }
    #[test]
    fn partial_op_state_machine() {
        let mut op = PartialOp::open(9);
        assert_eq!(op.advance(4), Ok(OpPhase::Partial));
        assert_eq!(op.residual(), 5);
        assert_eq!(op.suspend(), Ok(OpPhase::Suspended));
        assert_eq!(op.resume(), Ok(OpPhase::Partial));
        assert_eq!(op.moved, 4);
        assert_eq!(op.advance(5), Ok(OpPhase::Completed));
        assert_eq!(op.advance(1), Err(IoCode::ResidualDrift));
        assert_eq!(op.conserved(), true);
    }
    #[test]
    fn board_masks_reconcile() {
        assert_eq!(board_overlap(), 3);
        assert_eq!(board_mask(EBADF), 3);
        assert_eq!(board_mask(EAGAIN), 2);
        assert_eq!(board_mask(EINTR), 0);
        assert_eq!(board_mask(0), 0);
    }
    #[test]
    fn fingerprint_is_stable() {
        assert_eq!(corpus_fingerprint(), corpus_fingerprint());
        assert_eq!(corpus_bytes().len(), CASE_ROWS * 5);
    }
    #[test]
    fn checks_all_green() {
        let s = run_unx9303_checks();
        assert_eq!(s.all_passed(), true, "{}", s.red_items_text());
    }
}
