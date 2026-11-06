//! UNX-F9308 flock/lockf EINTR 矩阵（锁等待中断） · 0x7BA80段
//! ─────────────────────────────────────────────────────────────
//! 判据原文（任务板 F9308 逐字）：
//!   ① 纯功能代码 ≥320 行；
//!   ② 判据原文：UNX-F9308-J1 锁等待中断注入判据过，全 EINTR 格过，
//!      锁状态零破坏格过，与 B16 记录锁挂点对账格过；
//!   ③ 完成时 API/软件写回：状态=已完成、结果摘要非空。
//!
//! 设计要点：
//!   ① 模型是「记录锁区间表」而不是等待队列：每个条目持有 (inode, fd,
//!      [start, end), kind)。flock 整文件锁用 [0, MAX_RANGE) 表示，lockf
//!      按字节区间加锁，两者共用同一张表，判据因此同时覆盖两套系统调用。
//!   ② 兼容性只由 LockKind 决定：Read/Read 兼容，Write 与任何已持锁冲突；
//!      同 fd 落在同一区间则走「转换」而非「新增」，读升写、写降读都记一笔
//!      converted，绝不产生两个同 fd 同区间条目。
//!   ③ 判定次序冻结：坏 fd (EBADF) → 区间溢出/越界 (EINVAL) → 解锁（幂等，
//!      恒成功）→ 冲突扫描 → 非阻塞 (EAGAIN) → 死锁 (EDEADLK) → 信号
//!      (EINTR / 静默重启) → 继续阻塞。次序每错一格都会改变可观测 errno。
//!   ④ 锁状态零破坏：EINTR 分支必须在任何写表动作之前返回。判据对每个
//!      EINTR 格取「调用前表摘要」与「调用后表摘要」逐字节比对，100 轮内
//!      摘要必须恒定；同时 entry 数、granted/converted 计数均不得变动。
//!   ⑤ F_GETLK 探针（getlk）只读：返回首个冲突持有者的 fd / kind / 区间，
//!      无冲突返回 Free；探针调用前后表摘要必须相同——探针污染是独立诊断码。
//!   ⑥ 与 B16 记录锁挂点对账：冻结五行挂点表（inode, fd, start, kind），
//!      脚本跑完后逐行在表内查找并比对，且表内活动条目数必须恰等于行数。
//!   ⑦ 注入账 injected == detected + escaped，escaped 恒 0；诊断码 12 个闭集。
//! ─────────────────────────────────────────────────────────────

use alloc::format;
use alloc::vec::Vec;

use crate::checks::CheckSet;

/// 诊断段基址（F9308 独占 0x7BA80..=0x7BABF）。
pub const SEG_BASE: u32 = 0x7BA80;
/// 段窗口宽度。
pub const SEG_WINDOW: u32 = 0x40;
/// 记录锁条目容量。
pub const LOCK_SLOTS: usize = 6;
/// 兼容性转换矩阵格数（3×3）。
pub const CONVERT_CELLS: usize = 9;
/// 冲突矩阵格数。
pub const CONFLICT_CELLS: usize = 10;
/// 单格注入轮数。
pub const INJECT_ROUNDS: u32 = 10;
/// B16 对账行数。
pub const B16_ROWS: usize = 4;
/// 语料区间上界（1 MiB，模拟稀疏文件尾）。
pub const MAX_RANGE: u64 = 1_048_576;

/// Linux errno（正数口径）。
pub const EBADF: i32 = 9;
/// 非阻塞面冲突。
pub const EAGAIN: i32 = 11;
/// 阻塞等待被信号打断。
pub const EINTR: i32 = 4;
/// 区间/参数非法。
pub const EINVAL: i32 = 22;
/// 升级环导致死锁。
pub const EDEADLK: i32 = 35;
/// 锁表耗尽（无锁可用）。
pub const ENOLCK: i32 = 37;
/// 目标条目不存在。
pub const ESRCH: i32 = 3;

// ── 诊断码闭集 ────────────────────────────────────────────────────────
/// 本模块诊断码（段内 +0x01..=+0x0C 闭集）。
#[repr(u16)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum LockCode {
    /// 冲突判反
    ConflictInverted,
    /// EINTR 路径改了表
    LostOnInterrupt,
    /// 升级把旧条目留下
    UpgradeResidue,
    /// 同 fd 同区间双授权
    DoubleGrant,
    /// 区间溢出未拒
    RangeOverflow,
    /// 满表未回 ENOLCK
    TableFull,
    /// 双升级未报死锁
    DeadlockMissed,
    /// 只读探针污染表
    ProbeMutated,
    /// B16 挂点对不上
    B16Skew,
    /// 注入账倒退
    LedgerRegress,
    /// 双跑指纹漂移
    FingerprintDrift,
    /// 段位越界
    SegmentViolation,
}

impl LockCode {
    /// 码值 = 段基址 + 1 + 变体序号（u32 量程，段值超 u16）。
    pub fn code(self) -> u32 {
        SEG_BASE + 1 + self as u32
    }

    /// 中文诊断一句话（闭集返回，禁止空串）。
    pub fn detail(self) -> &'static str {
        match self {
            LockCode::ConflictInverted => "冲突判反：Read/Read 兼容被判阻塞；请先判共享兼容再判独占",
            LockCode::LostOnInterrupt => "EINTR 路径改了锁表：请在任何写表动作之前返回中断",
            LockCode::UpgradeResidue => "升级残留：同 fd 同区间出现两个条目；请原地改 kind 不新增",
            LockCode::DoubleGrant => "同 fd 同区间被双授权：转换必须复用既有条目",
            LockCode::RangeOverflow => "区间溢出未拒：start+len 必须 checked_add 且不越语料上界",
            LockCode::TableFull => "锁表满未回 ENOLCK：六槽用尽必须显性拒绝新增",
            LockCode::DeadlockMissed => "双升级未报 EDEADLK：见到 upgrading 持有者必须先报死锁",
            LockCode::ProbeMutated => "F_GETLK 探针污染锁表：只读查询不得改任何字段",
            LockCode::B16Skew => "B16 挂点对账失败：冻结行必须逐行命中且无多余条目",
            LockCode::LedgerRegress => "注入账倒退：三格必须同步推进且只增不减",
            LockCode::FingerprintDrift => "双跑指纹不一致：判定路径混入非确定状态",
            LockCode::SegmentViolation => "段位越界：本模块必须独占 0x7BA80 的 0x40 窗口",
        }
    }

    /// 闭集全量。
    pub const ALL: &[LockCode] = &[
        LockCode::ConflictInverted, LockCode::LostOnInterrupt, LockCode::UpgradeResidue,
        LockCode::DoubleGrant, LockCode::RangeOverflow, LockCode::TableFull,
        LockCode::DeadlockMissed, LockCode::ProbeMutated, LockCode::B16Skew,
        LockCode::LedgerRegress, LockCode::FingerprintDrift, LockCode::SegmentViolation,
    ];
}

// ── 锁类型 ────────────────────────────────────────────────────────────
/// 记录锁类型（Free 同时充当「探测无冲突」的返回值）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum LockKind {
    /// 未持锁
    Free,
    /// 共享读锁（LOCK_SH / F_RDLCK）
    Read,
    /// 独占写锁（LOCK_EX / F_WRLCK）
    Write,
}

impl LockKind {
    /// 闭集编码 0/1/2。
    pub fn tag(self) -> u8 {
        match self { LockKind::Free => 0, LockKind::Read => 1, LockKind::Write => 2 }
    }

    /// 兼容性：本类型与另一类型能否共存于同一区间（不同 fd）。
    pub fn compatible_with(self, other: LockKind) -> bool {
        match (self, other) {
            (LockKind::Free, _) => true,
            (_, LockKind::Free) => true,
            (LockKind::Read, LockKind::Read) => true,
            _ => false,
        }
    }
}

// ── 调用与判决 ────────────────────────────────────────────────────────
/// 一次 flock/lockf 调用。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct LockCall {
    /// 发起者 fd（0 视为坏 fd）
    pub fd: u32,
    /// 目标 inode
    pub inode: u32,
    /// 区间起点
    pub start: u64,
    /// 区间长度；0 表示到文件尾（flock 语义）
    pub len: u64,
    /// 请求类型（Free 表示解锁）
    pub want: LockKind,
    /// LOCK_NB / F_SETLK 非阻塞
    pub nonblock: bool,
    /// 等待期间是否有待决信号
    pub sig_pending: bool,
    /// 处置是否带 SA_RESTART
    pub sa_restart: bool,
}

impl LockCall {
    /// 阻塞式整文件 flock 的构造器（区间 [0, MAX_RANGE)）。
    pub fn flock(fd: u32, inode: u32, want: LockKind) -> LockCall {
        LockCall {
            fd,
            inode,
            start: 0,
            len: MAX_RANGE,
            want,
            nonblock: false,
            sig_pending: false,
            sa_restart: false,
        }
    }

    /// 记录锁区间构造器。
    pub fn range(fd: u32, inode: u32, start: u64, len: u64, want: LockKind) -> LockCall {
        LockCall { fd, inode, start, len, want, nonblock: false, sig_pending: false, sa_restart: false }
    }

    /// 派生：加非阻塞位。
    pub fn with_nonblock(self) -> LockCall {
        LockCall { nonblock: true, ..self }
    }

    /// 派生：等待期间有待决信号。
    pub fn with_signal(self, restart: bool) -> LockCall {
        LockCall { sig_pending: true, sa_restart: restart, ..self }
    }
}

/// 调用判决。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum LockVerdict {
    /// 授权成功；bool 表示是否新建条目（false = 已持同类锁）
    Granted(bool),
    /// 同 fd 同区间原地换类型
    Converted,
    /// 解锁（幂等，恒成功）
    Unlocked,
    /// 非阻塞冲突
    WouldBlock,
    /// 阻塞等待（无信号）
    Blocked,
    /// 被信号打断（表不得改变）
    Interrupted,
    /// 坏 fd
    BadFd,
    /// 参数/区间非法
    Invalid,
    /// 升级环死锁
    Deadlock,
    /// 锁表耗尽
    NoLock,
}

impl LockVerdict {
    /// 闭集编码：0 成功类、1 等待类、2 错误类（指纹分段用）。
    pub fn tag(self) -> u8 {
        match self {
            LockVerdict::Granted(_) | LockVerdict::Converted | LockVerdict::Unlocked => 0,
            LockVerdict::Blocked => 1,
            _ => 2,
        }
    }

    /// 对外 errno（成功/等待面为 0）。
    pub fn errno(self) -> i32 {
        match self {
            LockVerdict::WouldBlock => EAGAIN,
            LockVerdict::Interrupted => EINTR,
            LockVerdict::BadFd => EBADF,
            LockVerdict::Invalid => EINVAL,
            LockVerdict::Deadlock => EDEADLK,
            LockVerdict::NoLock => ENOLCK,
            _ => 0,
        }
    }
}

// ── 记录锁条目与锁表 ──────────────────────────────────────────────────
/// 一条记录锁：[start, end) 由 resolve_end 规整，绝不存 len。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct LockEntry {
    pub used: bool,
    pub inode: u32,
    pub fd: u32,
    pub start: u64,
    pub end: u64,
    pub kind: LockKind,
    /// 该持有者正等待读升写（死锁检测的唯一依据）
    pub upgrading: bool,
}

impl LockEntry {
    /// 空条目常量。
    pub const EMPTY: LockEntry = LockEntry {
        used: false,
        inode: 0,
        fd: 0,
        start: 0,
        end: 0,
        kind: LockKind::Free,
        upgrading: false,
    };

    /// 与查询区间是否相交（半开区间）。
    pub fn spans(&self, start: u64, end: u64) -> bool {
        self.start < end && start < self.end
    }
}

/// 定容记录锁表：六槽 + 五类计数。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct LockTable {
    entries: [LockEntry; LOCK_SLOTS],
    granted: u32,
    converted: u32,
    interrupted: u32,
    blocked: u32,
    released: u32,
    rejects: u32,
}

/// 区间规整：len==0 视作到文件尾；溢出或越界返回 None。
pub fn resolve_end(start: u64, len: u64) -> Option<u64> {
    let end = if len == 0 {
        MAX_RANGE
    } else {
        match start.checked_add(len) {
            Some(v) => v,
            None => return None,
        }
    };
    if start >= end || end > MAX_RANGE {
        None
    } else {
        Some(end)
    }
}

impl LockTable {
    /// 空表。
    pub const fn new() -> LockTable {
        LockTable {
            entries: [LockEntry::EMPTY; LOCK_SLOTS],
            granted: 0,
            converted: 0,
            interrupted: 0,
            blocked: 0,
            released: 0,
            rejects: 0,
        }
    }

    /// 活动条目数。
    pub fn count(&self) -> u32 {
        self.entries.iter().filter(|e| e.used).count() as u32
    }

    /// 条目视图。
    pub fn entry(&self, i: usize) -> Option<&LockEntry> { self.entries.get(i) }
    /// 授权计数。
    pub fn granted(&self) -> u32 { self.granted }
    /// 转换计数。
    pub fn converted(&self) -> u32 { self.converted }
    /// 中断计数。
    pub fn interrupted(&self) -> u32 { self.interrupted }
    /// 阻塞计数。
    pub fn blocked(&self) -> u32 { self.blocked }
    /// 释放计数。
    pub fn released(&self) -> u32 { self.released }
    /// 拒绝计数。
    pub fn rejects(&self) -> u32 { self.rejects }

    /// 条目字节流：只含槽位，不含计数（零破坏比对必须用它）。
    fn encode_entries(&self, bytes: &mut Vec<u8>) {
        for e in self.entries.iter() {
            bytes.push(if e.used { 1 } else { 0 });
            bytes.push(e.kind.tag());
            bytes.push(if e.upgrading { 1 } else { 0 });
            bytes.extend_from_slice(&e.inode.to_le_bytes());
            bytes.extend_from_slice(&e.fd.to_le_bytes());
            bytes.extend_from_slice(&e.start.to_le_bytes());
            bytes.extend_from_slice(&e.end.to_le_bytes());
        }
    }

    /// 纯槽位摘要：EINTR 零破坏与探针零污染都拿它前后对比，故不含任何计数。
    pub fn entries_digest(&self) -> u32 {
        let mut bytes: Vec<u8> = Vec::new();
        self.encode_entries(&mut bytes);
        fnv1a(&bytes)
    }

    /// 全量摘要：槽位 + 六类计数，用于最终指纹。
    pub fn digest(&self) -> u32 {
        let mut bytes: Vec<u8> = Vec::new();
        self.encode_entries(&mut bytes);
        bytes.extend_from_slice(&self.granted.to_le_bytes());
        bytes.extend_from_slice(&self.converted.to_le_bytes());
        bytes.extend_from_slice(&self.interrupted.to_le_bytes());
        bytes.extend_from_slice(&self.blocked.to_le_bytes());
        bytes.extend_from_slice(&self.released.to_le_bytes());
        bytes.extend_from_slice(&self.rejects.to_le_bytes());
        fnv1a(&bytes)
    }

    /// 覆盖点查询：返回覆盖 at 的该 fd 条目类型（B16 对账按挂点地址查）。
    pub fn covering(&self, inode: u32, fd: u32, at: u64) -> LockKind {
        let mut kind = LockKind::Free;
        for e in self.entries.iter() {
            if e.used && e.inode == inode && e.fd == fd && e.start <= at && at < e.end {
                kind = e.kind;
            }
        }
        kind
    }

    /// 同 fd 同 inode 同区间的精确条目下标。
    fn find_exact(&self, inode: u32, fd: u32, start: u64, end: u64) -> Option<usize> {
        self.entries.iter().position(|e| {
            e.used && e.inode == inode && e.fd == fd && e.start == start && e.end == end
        })
    }

    /// 他 fd 冲突条目（首个）。
    fn find_conflict(&self, inode: u32, fd: u32, start: u64, end: u64, want: LockKind) -> Option<usize> {
        self.entries.iter().position(|e| {
            e.used && e.inode == inode && e.fd != fd && e.spans(start, end)
                && !e.kind.compatible_with(want)
        })
    }

    /// 是否存在「他 fd 正在升级」的读锁（升级环的另一半）。
    fn has_upgrading_peer(&self, inode: u32, fd: u32, start: u64, end: u64) -> bool {
        self.entries.iter().any(|e| {
            e.used && e.inode == inode && e.fd != fd && e.upgrading
                && matches!(e.kind, LockKind::Read) && e.spans(start, end)
        })
    }

    /// 把自己在同区间的读锁标记为「升级等待者」。
    fn mark_upgrading(&mut self, inode: u32, fd: u32, start: u64, end: u64) {
        for e in self.entries.iter_mut() {
            if e.used && e.inode == inode && e.fd == fd && e.spans(start, end)
                && matches!(e.kind, LockKind::Read)
            {
                e.upgrading = true;
            }
        }
    }

    /// 释放同 fd 在该 inode 上所有相交条目；返回释放条数。
    fn release(&mut self, inode: u32, fd: u32, start: u64, end: u64) -> u32 {
        let mut n = 0u32;
        for e in self.entries.iter_mut() {
            if e.used && e.inode == inode && e.fd == fd && e.spans(start, end) {
                e.used = false;
                e.kind = LockKind::Free;
                e.upgrading = false;
                n = n.saturating_add(1);
            }
        }
        n
    }

    /// 新增条目；满表返回 false（调用方转 ENOLCK）。
    fn insert(&mut self, inode: u32, fd: u32, start: u64, end: u64, kind: LockKind) -> bool {
        for e in self.entries.iter_mut() {
            if !e.used {
                e.used = true;
                e.inode = inode;
                e.fd = fd;
                e.start = start;
                e.end = end;
                e.kind = kind;
                e.upgrading = false;
                return true;
            }
        }
        false
    }

    /// 错误分支统一出口：记一笔拒绝并回传判决。
    fn deny(&mut self, v: LockVerdict) -> LockVerdict {
        self.rejects = self.rejects.saturating_add(1);
        v
    }

    /// flock/lockf 主判定。写表动作全部集中在成功分支尾部，保证 EINTR 零破坏。
    pub fn call(&mut self, c: LockCall) -> LockVerdict {
        if c.fd == 0 {
            return self.deny(LockVerdict::BadFd);
        }
        let end = match resolve_end(c.start, c.len) {
            Some(v) => v,
            None => return self.deny(LockVerdict::Invalid),
        };
        // 解锁：幂等，未持锁也返回成功，不计入 granted。
        if matches!(c.want, LockKind::Free) {
            let n = self.release(c.inode, c.fd, c.start, end);
            self.released = self.released.saturating_add(n);
            return LockVerdict::Unlocked;
        }
        // 冲突面：只读扫描，不写表。
        if self.find_conflict(c.inode, c.fd, c.start, end, c.want).is_some() {
            if c.nonblock {
                return self.deny(LockVerdict::WouldBlock);
            }
            if matches!(c.want, LockKind::Write) && self.has_upgrading_peer(c.inode, c.fd, c.start, end) {
                return self.deny(LockVerdict::Deadlock);
            }
            if c.sig_pending && !c.sa_restart {
                // 关键：中断分支在 mark_upgrading 之前返回，表摘要必须逐字节不变。
                self.interrupted = self.interrupted.saturating_add(1);
                return LockVerdict::Interrupted;
            }
            if matches!(c.want, LockKind::Write) {
                self.mark_upgrading(c.inode, c.fd, c.start, end);
            }
            self.blocked = self.blocked.saturating_add(1);
            return LockVerdict::Blocked;
        }
        // 无冲突：先看同 fd 同区间是否已持锁（转换面）。
        if let Some(idx) = self.find_exact(c.inode, c.fd, c.start, end) {
            let cur = match self.entries.get(idx) {
                Some(e) => e.kind,
                None => LockKind::Free,
            };
            if cur == c.want {
                self.granted = self.granted.saturating_add(1);
                return LockVerdict::Granted(false);
            }
            if let Some(slot) = self.entries.get_mut(idx) {
                slot.kind = c.want;
                slot.upgrading = false;
            }
            self.converted = self.converted.saturating_add(1);
            return LockVerdict::Converted;
        }
        if !self.insert(c.inode, c.fd, c.start, end, c.want) {
            return self.deny(LockVerdict::NoLock);
        }
        self.granted = self.granted.saturating_add(1);
        LockVerdict::Granted(true)
    }

    /// 当前同 fd 同区间的持锁类型（判据复算用）。
    pub fn state_of(&self, inode: u32, fd: u32, start: u64, len: u64) -> LockKind {
        let end = match resolve_end(start, len) {
            Some(v) => v,
            None => return LockKind::Free,
        };
        match self.find_exact(inode, fd, start, end) {
            Some(i) => match self.entries.get(i) {
                Some(e) => e.kind,
                None => LockKind::Free,
            },
            None => LockKind::Free,
        }
    }
}

// ── F_GETLK 探针 ──────────────────────────────────────────────────────
/// 探针结果：blocked 为真时携带持有者信息。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct LockProbe {
    pub blocked: bool,
    pub holder_fd: u32,
    pub holder_kind: LockKind,
    pub start: u64,
    pub end: u64,
}

/// F_GETLK/F_TESTLK 语义：只读扫描，返回首个冲突持有者。
pub fn getlk(t: &LockTable, inode: u32, fd: u32, start: u64, len: u64, want: LockKind) -> LockProbe {
    let end = match resolve_end(start, len) {
        Some(v) => v,
        None => {
            return LockProbe { blocked: false, holder_fd: 0, holder_kind: LockKind::Free, start: 0, end: 0 }
        }
    };
    match t.find_conflict(inode, fd, start, end, want) {
        Some(i) => match t.entry(i) {
            Some(e) => LockProbe {
                blocked: true,
                holder_fd: e.fd,
                holder_kind: e.kind,
                start: e.start,
                end: e.end,
            },
            None => LockProbe { blocked: false, holder_fd: 0, holder_kind: LockKind::Free, start: 0, end: 0 },
        },
        None => LockProbe { blocked: false, holder_fd: 0, holder_kind: LockKind::Free, start: 0, end: 0 },
    }
}

// ── 三×三转换矩阵 ─────────────────────────────────────────────────────
/// 一行：(已持类型, 请求类型, 期望判决)。
pub const CONVERT_MATRIX: [(LockKind, LockKind, LockVerdict); CONVERT_CELLS] = [
    (LockKind::Free, LockKind::Free, LockVerdict::Unlocked),
    (LockKind::Free, LockKind::Read, LockVerdict::Granted(true)),
    (LockKind::Free, LockKind::Write, LockVerdict::Granted(true)),
    (LockKind::Read, LockKind::Read, LockVerdict::Granted(false)),
    (LockKind::Read, LockKind::Write, LockVerdict::Converted),
    (LockKind::Read, LockKind::Free, LockVerdict::Unlocked),
    (LockKind::Write, LockKind::Read, LockVerdict::Converted),
    (LockKind::Write, LockKind::Write, LockVerdict::Granted(false)),
    (LockKind::Write, LockKind::Free, LockVerdict::Unlocked),
];

/// 逐格复算转换矩阵：返回 (命中数, 转换后类型正确数)。
pub fn run_convert_matrix() -> (u32, u32) {
    let mut hits = 0u32;
    let mut states = 0u32;
    let mut i = 0usize;
    while i < CONVERT_MATRIX.len() {
        if let Some((held, want, expect)) = CONVERT_MATRIX.get(i).copied() {
            let mut t = LockTable::new();
            let seed_ok = if matches!(held, LockKind::Free) {
                true
            } else {
                t.call(LockCall::range(1, 7, 0, 4096, held)) == LockVerdict::Granted(true)
            };
            let got = t.call(LockCall::range(1, 7, 0, 4096, want));
            let after = t.state_of(7, 1, 0, 4096);
            let state_ok = if matches!(want, LockKind::Free) {
                matches!(after, LockKind::Free)
            } else {
                after == want
            };
            if seed_ok && got == expect {
                hits = hits.saturating_add(1);
            }
            if seed_ok && state_ok {
                states = states.saturating_add(1);
            }
        }
        i += 1;
    }
    (hits, states)
}

// ── 冲突十格 ──────────────────────────────────────────────────────────
/// 一行：持有者类型 × 请求类型 × 非阻塞 × 信号 × 重定位 → 期望判决。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ConflictCell {
    pub held: LockKind,
    pub want: LockKind,
    pub nonblock: bool,
    pub sig: bool,
    pub restart: bool,
    pub expect: LockVerdict,
}

/// 十行冻结冲突矩阵（持有者固定 fd=1，请求者 fd=2，区间同为 [0,4096)）。
pub const CONFLICT_MATRIX: [ConflictCell; CONFLICT_CELLS] = [
    ConflictCell { held: LockKind::Read, want: LockKind::Read, nonblock: false, sig: false, restart: false, expect: LockVerdict::Granted(true) },
    ConflictCell { held: LockKind::Read, want: LockKind::Write, nonblock: false, sig: false, restart: false, expect: LockVerdict::Blocked },
    ConflictCell { held: LockKind::Read, want: LockKind::Write, nonblock: true, sig: false, restart: false, expect: LockVerdict::WouldBlock },
    ConflictCell { held: LockKind::Read, want: LockKind::Write, nonblock: false, sig: true, restart: false, expect: LockVerdict::Interrupted },
    ConflictCell { held: LockKind::Read, want: LockKind::Write, nonblock: false, sig: true, restart: true, expect: LockVerdict::Blocked },
    ConflictCell { held: LockKind::Write, want: LockKind::Read, nonblock: false, sig: false, restart: false, expect: LockVerdict::Blocked },
    ConflictCell { held: LockKind::Write, want: LockKind::Read, nonblock: true, sig: false, restart: false, expect: LockVerdict::WouldBlock },
    ConflictCell { held: LockKind::Write, want: LockKind::Read, nonblock: false, sig: true, restart: false, expect: LockVerdict::Interrupted },
    ConflictCell { held: LockKind::Write, want: LockKind::Write, nonblock: false, sig: true, restart: false, expect: LockVerdict::Interrupted },
    ConflictCell { held: LockKind::Write, want: LockKind::Write, nonblock: true, sig: false, restart: false, expect: LockVerdict::WouldBlock },
];

/// 注入账：与 F9307 同名但独立定义（模块自包含，不跨单引用）。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct LockLedger {
    pub injected: u32,
    pub detected: u32,
    pub escaped: u32,
}

impl LockLedger {
    /// 零账。
    pub const fn zero() -> LockLedger { LockLedger { injected: 0, detected: 0, escaped: 0 } }

    /// 记一次并回传守恒结论。
    pub fn charge(&mut self, caught: bool) -> bool {
        self.injected = self.injected.saturating_add(1);
        if caught { self.detected = self.detected.saturating_add(1); } else { self.escaped = self.escaped.saturating_add(1); }
        self.conserved()
    }

    /// 守恒。
    pub fn conserved(&self) -> bool {
        self.detected.saturating_add(self.escaped) == self.injected
    }
}

/// 冲突矩阵复算报告。
#[derive(Clone, Copy, Debug)]
pub struct FlockReport {
    pub per_cell: [u32; CONFLICT_CELLS],
    pub ledger: LockLedger,
    pub eintr_cells: u32,
    pub eintr_cells_ok: u32,
    /// EINTR 格上表摘要被改动的次数（必须为 0）。
    pub corruption: u32,
    pub conserved: bool,
}

/// 单格单轮：新表 → 持有者加锁 → 请求者调用 → 记录摘要是否漂移。
fn simulate_conflict(cell: ConflictCell) -> (LockVerdict, bool, bool) {
    let mut t = LockTable::new();
    let seed = LockCall::range(1, 7, 0, 4096, cell.held);
    let seeded = t.call(seed).errno() == 0;
    let before = t.entries_digest();
    let mut req = LockCall::range(2, 7, 0, 4096, cell.want);
    req.nonblock = cell.nonblock;
    req.sig_pending = cell.sig;
    req.sa_restart = cell.restart;
    let got = t.call(req);
    let after = t.entries_digest();
    let drift = before != after;
    // 对 Interrupted 格：摘要必须不变；其他格允许按判决改表。
    let eintr_violation = matches!(cell.expect, LockVerdict::Interrupted) && drift;
    (got, seeded, eintr_violation)
}

/// 十格 × 十轮：逐格命中、EINTR 覆盖、零破坏计数、注入账。
pub fn run_conflict_matrix() -> FlockReport {
    let mut report = FlockReport {
        per_cell: [0u32; CONFLICT_CELLS],
        ledger: LockLedger::zero(),
        eintr_cells: 0,
        eintr_cells_ok: 0,
        corruption: 0,
        conserved: true,
    };
    let mut ci = 0usize;
    while ci < CONFLICT_MATRIX.len() {
        if let Some(cell) = CONFLICT_MATRIX.get(ci).copied() {
            if matches!(cell.expect, LockVerdict::Interrupted) {
                report.eintr_cells = report.eintr_cells.saturating_add(1);
            }
            let mut round = 0u32;
            let mut all_ok = true;
            while round < INJECT_ROUNDS {
                let (got, seeded, violation) = simulate_conflict(cell);
                let caught = got == cell.expect && seeded && !violation;
                if !report.ledger.charge(caught) {
                    report.conserved = false;
                }
                if violation {
                    report.corruption = report.corruption.saturating_add(1);
                }
                if caught {
                    if let Some(slot) = report.per_cell.get_mut(ci) {
                        *slot = slot.saturating_add(1);
                    }
                } else {
                    all_ok = false;
                }
                round += 1;
            }
            if matches!(cell.expect, LockVerdict::Interrupted) && all_ok {
                report.eintr_cells_ok = report.eintr_cells_ok.saturating_add(1);
            }
        }
        ci += 1;
    }
    report
}

// ── 死锁探针 ──────────────────────────────────────────────────────────
/// 双升级环：两 fd 先共持读锁，再各自升写；后者必须 EDEADLK，
/// 且前者停留在 Blocked（upgrading 标记已落表但不改锁类型）。
pub fn deadlock_probe() -> [LockVerdict; 4] {
    let mut t = LockTable::new();
    let mut out = [LockVerdict::BadFd; 4];
    let a = t.call(LockCall::range(1, 9, 0, 8192, LockKind::Read));
    let b = t.call(LockCall::range(2, 9, 0, 8192, LockKind::Read));
    let c = t.call(LockCall::range(1, 9, 0, 8192, LockKind::Write));
    let d = t.call(LockCall::range(2, 9, 0, 8192, LockKind::Write));
    if let Some(slot) = out.get_mut(0) { *slot = a; }
    if let Some(slot) = out.get_mut(1) { *slot = b; }
    if let Some(slot) = out.get_mut(2) { *slot = c; }
    if let Some(slot) = out.get_mut(3) { *slot = d; }
    out
}

// ── B16 挂点对账 ──────────────────────────────────────────────────────
/// B16 冻结挂点：(inode, fd, start, kind)，由 B16 记录锁挂点表抄录。
pub const B16_CHECKPOINT: [(u32, u32, u64, LockKind); B16_ROWS] = [
    (7, 1, 0, LockKind::Write),
    (7, 2, 4096, LockKind::Write),
    (9, 3, 0, LockKind::Read),
    (11, 4, 0, LockKind::Write),
];

/// 对账结果：(命中行数, 表内活动条目数, 是否无多余条目, 脚本五步是否全部成功)。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct B16Report {
    pub hits: u32,
    pub live: u32,
    pub exact: bool,
    pub scripted: bool,
}

/// 脚本：建表 → 逐行核对。
pub fn reconcile_b16() -> B16Report {
    let mut t = LockTable::new();
    let scripted = t.call(LockCall::range(1, 7, 0, 4096, LockKind::Read)).errno() == 0
        && t.call(LockCall::range(2, 7, 4096, 4096, LockKind::Write)).errno() == 0
        && t.call(LockCall::range(3, 9, 0, 16384, LockKind::Read)).errno() == 0
        && t.call(LockCall::range(1, 7, 0, 4096, LockKind::Write)).errno() == 0
        && t.call(LockCall::range(4, 11, 0, 1024, LockKind::Write)).errno() == 0;
    let mut hits = 0u32;
    let mut i = 0usize;
    while i < B16_CHECKPOINT.len() {
        if let Some((inode, fd, start, kind)) = B16_CHECKPOINT.get(i).copied() {
            if t.covering(inode, fd, start) == kind {
                hits = hits.saturating_add(1);
            }
        }
        i += 1;
    }
    let live = t.count();
    B16Report { hits, live, exact: live == B16_ROWS as u32, scripted }
}

// ── Linux 逐码对照 ────────────────────────────────────────────────────
/// (名称, 数值, 触发面)。只收锁族，表外为 0。
pub const ERRNO_TABLE: &[(&str, i32, &str)] = &[
    ("EINTR", EINTR, "锁等待被信号打断"),
    ("EAGAIN", EAGAIN, "非阻塞面冲突"),
    ("EBADF", EBADF, "fd 为 0 或无效"),
    ("EINVAL", EINVAL, "区间越界或参数非法"),
    ("EDEADLK", EDEADLK, "升级环死锁"),
    ("ENOLCK", ENOLCK, "锁表耗尽"),
];

/// 按名查码；表外返回 0。
pub fn errno_by_name(name: &str) -> i32 {
    let mut i = 0usize;
    while i < ERRNO_TABLE.len() {
        if let Some(row) = ERRNO_TABLE.get(i) {
            if row.0 == name {
                return row.1;
            }
        }
        i += 1;
    }
    0
}

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

/// 规范语料指纹：正常授权、共享共存、冲突阻塞、EINTR、转换、解锁、满表。
pub fn corpus_fingerprint() -> u32 {
    let mut bytes: Vec<u8> = Vec::new();
    let mut t = LockTable::new();
    let ops: [(u32, u32, u64, u64, LockKind); 8] = [
        (1, 7, 0, 4096, LockKind::Read),
        (2, 7, 0, 4096, LockKind::Read),
        (1, 7, 0, 4096, LockKind::Write),
        (3, 7, 0, 4096, LockKind::Read),
        (2, 7, 0, 4096, LockKind::Free),
        (1, 7, 0, 4096, LockKind::Read),
        (0, 7, 0, 4096, LockKind::Read),
        (1, 7, 0, 0, LockKind::Write),
    ];
    let mut i = 0usize;
    while i < ops.len() {
        if let Some((fd, inode, start, len, want)) = ops.get(i).copied() {
            let v = t.call(LockCall::range(fd, inode, start, len, want));
            bytes.push(v.tag());
            bytes.push((v.errno() & 0x7F) as u8);
        }
        i += 1;
    }
    bytes.extend_from_slice(&t.digest().to_le_bytes());
    fnv1a(&bytes)
}

// ── 检查入口 ──────────────────────────────────────────────────────────
/// F9308 判据集（add ≥12；add_dyn ≤8）。
pub fn run_unx9308_checks() -> CheckSet {
    let mut s = CheckSet::new("unx9308");

    // 段位守卫。
    let seg_ok = SEG_BASE % 0x40 == 0 && SEG_BASE == 0x7BA80
        && (0x7B000..=0x7D1C0).contains(&SEG_BASE)
        && SEG_BASE + SEG_WINDOW <= 0x7BAC0;
    s.add("段位守卫：0x7BA80 独占 0x40 窗口", seg_ok,
          "段基址必须 0x40 对齐且不覆盖 F9309 段");

    // 正常流：空表授权读锁并新建条目。
    let mut t0 = LockTable::new();
    let g = t0.call(LockCall::flock(1, 5, LockKind::Read));
    s.add("正常流：整文件读锁新建条目",
          g == LockVerdict::Granted(true) && t0.count() == 1
              && t0.state_of(5, 1, 0, 0) == LockKind::Read,
          "flock 整文件锁必须以 len=0 规整到文件尾并成一条");

    // 共享兼容：两 fd 同区间读锁共存。
    let mut t1 = LockTable::new();
    let a = t1.call(LockCall::range(1, 7, 0, 4096, LockKind::Read));
    let b = t1.call(LockCall::range(2, 7, 0, 4096, LockKind::Read));
    s.add("兼容：Read/Read 同区间两 fd 共存",
          a == LockVerdict::Granted(true) && b == LockVerdict::Granted(true) && t1.count() == 2,
          "共享锁互不排斥，兼容性必须先于冲突判定");

    // 冲突面三判决：阻塞 / 非阻塞 EAGAIN / 信号 EINTR。
    let mut t2 = LockTable::new();
    let seed_ok = t2.call(LockCall::range(1, 7, 0, 4096, LockKind::Read)) == LockVerdict::Granted(true);
    let blocked = t2.call(LockCall::range(2, 7, 0, 4096, LockKind::Write));
    let nb = t2.call(LockCall::range(2, 7, 0, 4096, LockKind::Write).with_nonblock());
    let intr = t2.call(LockCall::range(2, 7, 0, 4096, LockKind::Write).with_signal(false));
    s.add("冲突三判决：Blocked / EAGAIN / EINTR",
          seed_ok && blocked == LockVerdict::Blocked && nb == LockVerdict::WouldBlock
              && intr == LockVerdict::Interrupted && nb.errno() == EAGAIN
              && intr.errno() == EINTR,
          "同一冲突面必须由 nonblock 与信号位分出三种可观测结果");

    // 十格冲突矩阵 + 全 EINTR 格 + 零破坏。
    let report = run_conflict_matrix();
    let cells_ok = report.per_cell.iter().all(|n| *n == INJECT_ROUNDS);
    s.add("矩阵：十格 × 十轮全命中", cells_ok && report.eintr_cells == 3,
          "每格十轮逐轮复算，格数必须与冻结表一致");
    let eintr_ok = report.eintr_cells == 3 && report.eintr_cells_ok == 3
        && LockVerdict::Interrupted.errno() == EINTR;
    s.add("全 EINTR 格：三格每轮均回 4", eintr_ok,
          "有信号且无重定位位的冲突格必须每轮都回 EINTR");
    let ledger_ok = report.conserved && report.ledger.conserved()
        && report.ledger.escaped == 0
        && report.ledger.injected == CONFLICT_CELLS as u32 * INJECT_ROUNDS;
    s.add("注入账：100 注入 100 检出 0 逃逸", ledger_ok,
          "矩阵注入数与检出数必须逐笔对平");

    // 零破坏：100 轮内 EINTR 格表摘要恒不变。
    s.add("零破坏：EINTR 格表摘要零漂移", report.corruption == 0,
          "中断路径若先写表再返回，摘要必漂移并被此格抓住");

    // 转换矩阵 9 格。
    let (conv_hits, conv_states) = run_convert_matrix();
    s.add("转换：3×3 九格与终态全对",
          conv_hits == CONVERT_CELLS as u32 && conv_states == CONVERT_CELLS as u32,
          "读升写/写降读必须原地改类型，终态不得残留旧类型");

    // 死锁：双升级环。
    let dl = deadlock_probe();
    let dl_ok = dl.get(0) == Some(&LockVerdict::Granted(true))
        && dl.get(1) == Some(&LockVerdict::Granted(true))
        && dl.get(2) == Some(&LockVerdict::Blocked)
        && dl.get(3) == Some(&LockVerdict::Deadlock)
        && LockVerdict::Deadlock.errno() == EDEADLK;
    s.add("死锁：双升级后者 EDEADLK 前者仍阻塞", dl_ok,
          "见到 upgrading 持有者必须先报死锁，不得无限等待");

    // F_GETLK 探针：只读，且报告持有者。
    let mut t3 = LockTable::new();
    let wrote = t3.call(LockCall::range(1, 7, 0, 4096, LockKind::Write)) == LockVerdict::Granted(true);
    let before = t3.entries_digest();
    let probe = getlk(&t3, 7, 2, 0, 4096, LockKind::Read);
    let free = getlk(&t3, 7, 2, 8192, 4096, LockKind::Read);
    s.add("探针：F_GETLK 报持有者且不改表",
          wrote && probe.blocked && probe.holder_fd == 1 && probe.holder_kind == LockKind::Write
              && !free.blocked && free.holder_kind == LockKind::Free && t3.entries_digest() == before,
          "只读探针污染表会让后续判决全错，必须逐字节比对摘要");

    // 解锁幂等：未持锁释放也成功，表不变。
    let mut t4 = LockTable::new();
    let held = t4.call(LockCall::range(1, 7, 0, 4096, LockKind::Read)) == LockVerdict::Granted(true);
    let no_lock = LockCall::range(4, 7, 0, 4096, LockKind::Free);
    let d_before = t4.entries_digest();
    let unlocked = t4.call(no_lock);
    s.add("解锁幂等：未持锁释放成功且表不变",
          held && unlocked == LockVerdict::Unlocked && t4.entries_digest() == d_before && t4.count() == 1,
          "flock LOCK_UN 恒成功；不得因未持锁改记账");

    // 容量：六槽用尽后第七条 NoLock。
    let mut t5 = LockTable::new();
    let mut made = 0u32;
    while made < LOCK_SLOTS as u32 {
        let start = made as u64 * 4096;
        if t5.call(LockCall::range(1, 7, start, 4096, LockKind::Read)).errno() == 0 {
            made = made.saturating_add(1);
        } else {
            break;
        }
    }
    let seventh = t5.call(LockCall::range(1, 7, 4096 * 6, 4096, LockKind::Read));
    s.add("容量：六槽满后新增回 ENOLCK",
          made == LOCK_SLOTS as u32 && seventh == LockVerdict::NoLock
              && seventh.errno() == ENOLCK && t5.count() == LOCK_SLOTS as u32,
          "全表新增失败必须显性回 37，不得覆盖既有条目");

    // 错态：坏 fd 与区间越界。
    let mut t6 = LockTable::new();
    let bad = t6.call(LockCall::flock(0, 5, LockKind::Read));
    let ovf = t6.call(LockCall::range(1, 5, MAX_RANGE - 1, 4096, LockKind::Read));
    let zero_len_tail = t6.call(LockCall::range(1, 5, MAX_RANGE, 0, LockKind::Read));
    s.add("错态：EBADF 与区间溢出 EINVAL",
          bad == LockVerdict::BadFd && bad.errno() == EBADF
              && ovf == LockVerdict::Invalid && zero_len_tail == LockVerdict::Invalid,
          "坏 fd 必须先于区间判定，越界不得被规整成合法区间");

    // B16 对账。
    let b16 = reconcile_b16();
    s.add("B16 对账：四行挂点全命中且无多余条目",
          b16.scripted && b16.hits == B16_ROWS as u32 && b16.live == B16_ROWS as u32 && b16.exact,
          "记录锁挂点冻结表必须与实装表逐行一致");

    // Linux 逐码。
    let linux_ok = errno_by_name("EINTR") == 4 && errno_by_name("EAGAIN") == 11
        && errno_by_name("EBADF") == 9 && errno_by_name("EINVAL") == 22
        && errno_by_name("EDEADLK") == 35 && errno_by_name("ENOLCK") == 37
        && errno_by_name("ENOSYS") == 0;
    s.add("Linux 逐码：锁族六码一致", linux_ok,
          "错误码必须按 Linux 数值，表外不得伪造");

    // 双跑指纹。
    let fp_a = corpus_fingerprint();
    let fp_b = corpus_fingerprint();
    s.add("双跑一致：规范语料指纹相等", fp_a == fp_b && fp_a != 0,
          "同输入两次必须逐字节相同");

    // 诊断码唯一 + 文案。
    let mut uniq = true;
    let mut i = 0usize;
    while i < LockCode::ALL.len() {
        let mut j = i + 1;
        while j < LockCode::ALL.len() {
            if let (Some(x), Some(y)) = (LockCode::ALL.get(i), LockCode::ALL.get(j)) {
                if x.code() == y.code() { uniq = false; }
            }
            j += 1;
        }
        i += 1;
    }
    let text_ok = LockCode::ALL.iter().all(|c| !c.detail().is_empty())
        && LockCode::ALL.len() == 12 && uniq;
    s.add("诊断码：12 码唯一且文案非空", text_ok,
          "段内闭集码重复会让判据无法定位");

    s.add_dyn("注入账快照", ledger_ok,
              &format!("inj={} det={} esc={}", report.ledger.injected,
                       report.ledger.detected, report.ledger.escaped));
    s.add_dyn("零破坏快照", report.corruption == 0,
              &format!("drift={} cells={}", report.corruption, report.eintr_cells));
    s.add_dyn("指纹快照", fp_a == fp_b, &format!("fnv=0x{:08X}", fp_a));

    s
}

// ── 测试 ──────────────────────────────────────────────────────────────
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn t01_resolve_end_rules() {
        assert_eq!(resolve_end(0, 0), Some(MAX_RANGE));
        assert_eq!(resolve_end(0, 4096), Some(4096));
        assert_eq!(resolve_end(MAX_RANGE - 1, 4096), None);
        assert_eq!(resolve_end(MAX_RANGE, 0), None);
    }

    #[test]
    fn t02_flock_and_shared() {
        let mut t = LockTable::new();
        assert_eq!(t.call(LockCall::flock(1, 5, LockKind::Write)), LockVerdict::Granted(true));
        assert_eq!(t.call(LockCall::flock(2, 5, LockKind::Read)), LockVerdict::Blocked);
        assert_eq!(t.call(LockCall::flock(2, 5, LockKind::Free)), LockVerdict::Unlocked);
        // fd2 未持锁，幂等解锁不动 fd1 的写锁；释放持有者后表才清空。
        assert_eq!(t.count(), 1);
        assert_eq!(t.call(LockCall::flock(1, 5, LockKind::Free)), LockVerdict::Unlocked);
        assert_eq!(t.count(), 0);
    }

    #[test]
    fn t03_interrupt_keeps_table() {
        let mut t = LockTable::new();
        let seeded = t.call(LockCall::range(1, 7, 0, 4096, LockKind::Read)) == LockVerdict::Granted(true);
        assert_eq!(seeded, true);
        let before = t.entries_digest();
        let v = t.call(LockCall::range(2, 7, 0, 4096, LockKind::Write).with_signal(false));
        assert_eq!(v, LockVerdict::Interrupted);
        assert_eq!(t.entries_digest(), before);
        assert_eq!(t.count(), 1);
    }

    #[test]
    fn t04_convert_matrix_all() {
        let (hits, states) = run_convert_matrix();
        assert_eq!(hits, 9);
        assert_eq!(states, 9);
    }

    #[test]
    fn t05_deadlock() {
        let dl = deadlock_probe();
        assert_eq!(dl.get(2), Some(&LockVerdict::Blocked));
        assert_eq!(dl.get(3), Some(&LockVerdict::Deadlock));
    }

    #[test]
    fn t06_probe_readonly() {
        let mut t = LockTable::new();
        let wrote = t.call(LockCall::range(1, 7, 0, 4096, LockKind::Write)) == LockVerdict::Granted(true);
        assert_eq!(wrote, true);
        let d = t.entries_digest();
        let p = getlk(&t, 7, 2, 0, 4096, LockKind::Read);
        assert_eq!(p.blocked, true);
        assert_eq!(t.entries_digest(), d);
    }

    #[test]
    fn t07_conflict_matrix_ok() {
        let r = run_conflict_matrix();
        assert_eq!(r.per_cell.iter().all(|n| *n == INJECT_ROUNDS), true);
        assert_eq!(r.eintr_cells, 3);
        assert_eq!(r.eintr_cells_ok, 3);
        assert_eq!(r.corruption, 0);
        assert_eq!(r.ledger.escaped, 0);
    }

    #[test]
    fn t08_b16_reconcile() {
        let b = reconcile_b16();
        assert_eq!(b.hits, 4);
        assert_eq!(b.live, 4);
        assert_eq!(b.exact, true);
    }

    #[test]
    fn t09_fingerprint_double() {
        let a = corpus_fingerprint();
        assert_eq!(a, corpus_fingerprint());
        assert_eq!(a != 0, true);
    }

    #[test]
    fn t10_checks_all_green() {
        let s = run_unx9308_checks();
        assert_eq!(s.all_passed(), true, "{}", s.red_items_text());
        assert_eq!(s.len() >= 12, true);
    }
}
