//! UNX-F11276 musl 文件族消费判据（0x1f4c0 段）
//!
//! 本模块把 musl 文件族五个入口 openat / read / write / lseek / fstat（外加
//! close）建模成两件真东西：一张**定容 fd 表**（真分配、真推移偏移、真截断
//! 读），以及一条**纯判据函数**，输入一条 Case（op + fd 号 + 偏移 + 长度 +
//! 状态位），输出 Linux errno 原值。
//! 状态位不是装饰：F_FD_OPEN 缺席即 EBADF，F_FD_DIR 命中即 EISDIR，
//! F_FD_RDONLY 命中即写侧 EBADF，F_FD_PIPE 命中即 lseek 的 ESPIPE，
//! F_PARENT_NOX 即打开路径的 EACCES，F_BUF_NULL 即 EFAULT。
//! 判据面要求 10 类错误码逐类透传，故 ERRNO_CLASSES 固定列出 10 个 errno，
//! 账本为每类开一个直方槽：登记时按判据结果落槽，replay 要求
//! 「零判据数 + 直方总和 == 登记条数」，漏计一类即 EBALANCE。
//! 拒绝面只有四种：id 越界、登记序倒退、同 id 重复、封册后写账；
//! 判据函数本身**不拒收**任何样本——越界长度（如 0x8000_0000 读长、
//! 超过磁盘上限的写末址）是故意喂进规则里产生 EINVAL/ENOSPC 的。

use crate::alloc::format;
use crate::checks::CheckSet;
use alloc::string::String;

/// 诊断段（0x1f4c0 独占 0x10）。
pub const SEG_BASE: u32 = 0x1f4c0;

/// 条目 id 越界。
pub const FS_EID: u32 = SEG_BASE + 0x01;
/// 登记序倒退。
pub const FS_EORDER: u32 = SEG_BASE + 0x02;
/// 同 id 重复登记。
pub const FS_EDUP: u32 = SEG_BASE + 0x03;
/// 条目表已满。
pub const FS_EFULL: u32 = SEG_BASE + 0x04;
/// 封册后写账。
pub const FS_EFROZEN: u32 = SEG_BASE + 0x05;
/// 观测了未登记的 id。
pub const FS_EUNSEEN: u32 = SEG_BASE + 0x06;
/// 复算 errno 与登记时不符。
pub const FS_EMISMATCH: u32 = SEG_BASE + 0x07;
/// 账未观测满。
pub const FS_EUNOBS: u32 = SEG_BASE + 0x08;
/// 同一条目重复观测。
pub const FS_EDOBS: u32 = SEG_BASE + 0x09;
/// 直方/尝试表不平。
pub const FS_EBALANCE: u32 = SEG_BASE + 0x0a;

/// 透传的 Linux errno 原值（不重新编号）。
pub const ENOENT: u32 = 2;
pub const EBADF: u32 = 9;
pub const EACCES: u32 = 13;
pub const EFAULT: u32 = 14;
pub const ENOTDIR: u32 = 20;
pub const EISDIR: u32 = 21;
pub const EINVAL: u32 = 22;
pub const EMFILE: u32 = 24;
pub const ENOSPC: u32 = 28;
pub const ESPIPE: u32 = 29;
pub const ELOOP: u32 = 40;
pub const EOVERFLOW: u32 = 75;

/// 十个必须逐类透传的错误码（判据面的「10 类」）。
pub const ERRNO_CLASSES: [u32; 10] = [
    EBADF, EFAULT, EINVAL, ENOENT, EACCES, EISDIR, ENOTDIR, ESPIPE, ENOSPC, EOVERFLOW,
];

/// fd 表定容。
pub const FD_CAP: usize = 8;
/// 条目表定容。
pub const CAP: usize = 12;
/// 单次读长度上限（Linux 对 read 的 cap）。
pub const RD_MAX: u32 = 0x7fff_f000;
/// 模拟磁盘上限 1 MiB。
pub const DISK_MAX: u64 = 0x10_0000;
/// lseek 目标偏移上限。
pub const LSEEK_MAX: u64 = 0x7fff_ffff_ffff;
/// 条目 id 合法域。
pub const ID_MIN: u32 = 1;
pub const ID_MAX: u32 = 4096;

/// 操作码。
pub const OP_OPENAT: u32 = 1;
pub const OP_READ: u32 = 2;
pub const OP_WRITE: u32 = 3;
pub const OP_LSEEK: u32 = 4;
pub const OP_FSTAT: u32 = 5;
pub const OP_CLOSE: u32 = 6;

/// whence 取值（Linux 原值）。
pub const SEEK_SET: u32 = 0;
pub const SEEK_CUR: u32 = 1;
pub const SEEK_END: u32 = 2;

/// fd 处于打开态（缺席即 EBADF）。
pub const F_FD_OPEN: u32 = 1 << 0;
/// fd 指向目录。
pub const F_FD_DIR: u32 = 1 << 1;
/// fd 是管道。
pub const F_FD_PIPE: u32 = 1 << 2;
/// fd 只读。
pub const F_FD_RDONLY: u32 = 1 << 3;
/// 父目录无搜索权限。
pub const F_PARENT_NOX: u32 = 1 << 4;
/// openat 带 O_DIRECTORY。
pub const F_O_DIRECTORY: u32 = 1 << 5;
/// openat 带 O_NOFOLLOW。
pub const F_O_NOFOLLOW: u32 = 1 << 6;
/// 路径为空串。
pub const F_PATH_EMPTY: u32 = 1 << 7;
/// 目标为符号链接。
pub const F_SYMLINK: u32 = 1 << 8;
/// 缓冲区指针为空。
pub const F_BUF_NULL: u32 = 1 << 9;
/// whence 非法。
pub const F_WHENCE_BAD: u32 = 1 << 10;
/// 合法状态位并集。
pub const FLAG_MASK: u32 =
    F_FD_OPEN | F_FD_DIR | F_FD_PIPE | F_FD_RDONLY | F_PARENT_NOX | F_O_DIRECTORY
        | F_O_NOFOLLOW | F_PATH_EMPTY | F_SYMLINK | F_BUF_NULL | F_WHENCE_BAD;

const FNV_OFF: u64 = 0xcbf2_9ce4_8422_2325;
const FNV_PRIME: u64 = 0x100_0000_01b3;
const TAG: u64 = 0xb04_76;

/// 一条文件族样本：id + 操作码 + fd 号 + 偏移 + 长度 + 状态位。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Case {
    pub id: u32,
    pub op: u32,
    pub fd: u32,
    pub off: u64,
    pub len: u32,
    pub flags: u32,
}

impl Case {
    pub const fn blank() -> Case {
        Case { id: 0, op: 0, fd: 0, off: 0, len: 0, flags: 0 }
    }

    pub fn render(&self) -> String {
        format!("C{} op{} fd{} off={:x} len={} fl={:x}",
            self.id, self.op, self.fd, self.off, self.len, self.flags)
    }
}

/// 纯判据：一条文件族样本的期望 errno。判定序按 Linux 入口固定：
/// 打开先看路径与权限，读写先看 fd 有效性，lseek 先看管道，再看待查表。
pub fn file_verdict(c: &Case) -> u32 {
    match c.op {
        OP_OPENAT => {
            if c.flags & F_PATH_EMPTY != 0 {
                return ENOENT;
            }
            if c.flags & F_PARENT_NOX != 0 {
                return EACCES;
            }
            if c.flags & F_O_DIRECTORY != 0 && c.flags & F_FD_DIR == 0 {
                return ENOTDIR;
            }
            if c.flags & F_O_NOFOLLOW != 0 && c.flags & F_SYMLINK != 0 {
                return ELOOP;
            }
            0
        }
        OP_READ => {
            if c.flags & F_FD_OPEN == 0 {
                return EBADF;
            }
            if c.flags & F_BUF_NULL != 0 {
                return EFAULT;
            }
            if c.flags & F_FD_DIR != 0 {
                return EISDIR;
            }
            if c.len > RD_MAX {
                return EINVAL;
            }
            0
        }
        OP_WRITE => {
            if c.flags & F_FD_OPEN == 0 {
                return EBADF;
            }
            if c.flags & F_FD_RDONLY != 0 {
                return EBADF;
            }
            if c.off.wrapping_add(c.len as u64) > DISK_MAX {
                return ENOSPC;
            }
            0
        }
        OP_LSEEK => {
            if c.flags & F_FD_OPEN == 0 {
                return EBADF;
            }
            if c.flags & F_FD_PIPE != 0 {
                return ESPIPE;
            }
            if c.flags & F_WHENCE_BAD != 0 {
                return EINVAL;
            }
            if c.off > LSEEK_MAX {
                return EOVERFLOW;
            }
            0
        }
        OP_FSTAT => {
            if c.flags & F_FD_OPEN == 0 {
                return EBADF;
            }
            if c.flags & F_BUF_NULL != 0 {
                return EFAULT;
            }
            0
        }
        OP_CLOSE => {
            if c.flags & F_FD_OPEN == 0 {
                return EBADF;
            }
            0
        }
        _ => EINVAL,
    }
}

/// fd 槽：打开态 + 偏移 + 尺寸 + 三个类型位。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Fd {
    pub used: bool,
    pub offset: u64,
    pub size: u64,
    pub dir: bool,
    pub pipe: bool,
    pub readonly: bool,
}

impl Fd {
    pub const fn blank() -> Fd {
        Fd { used: false, offset: 0, size: 0, dir: false, pipe: false, readonly: false }
    }
}

/// 定容 fd 表：真分配、真推移、真截断。读到头返回 0（EOF 不是错误）。
#[derive(Clone, Copy, Debug)]
pub struct FdTable {
    fds: [Fd; FD_CAP],
    pub opened: u32,
    pub closed: u32,
    pub reads: u32,
    pub writes: u32,
}

impl FdTable {
    pub const fn new() -> FdTable {
        FdTable { fds: [Fd::blank(); FD_CAP], opened: 0, closed: 0, reads: 0, writes: 0 }
    }

    fn at(&self, fd: usize) -> Result<Fd, u32> {
        match self.fds.get(fd) {
            Some(f) if f.used => Ok(*f),
            _ => Err(EBADF),
        }
    }

    fn install(&mut self, k: usize, dir: bool, pipe: bool, readonly: bool, size: u64) {
        if let Some(s) = self.fds.get_mut(k) {
            *s = Fd { used: true, offset: 0, size, dir, pipe, readonly };
        }
        self.opened = self.opened.wrapping_add(1);
    }

    /// 打开：先到先得，表满 EMFILE（不覆盖已用槽）。
    pub fn open(&mut self, dir: bool, readonly: bool, size: u64) -> Result<usize, u32> {
        let mut k = 0usize;
        while k < FD_CAP {
            let free = match self.fds.get(k) {
                Some(f) => !f.used,
                None => false,
            };
            if free {
                self.install(k, dir, false, readonly, size);
                return Ok(k);
            }
            k = k.wrapping_add(1);
        }
        Err(EMFILE)
    }

    /// 开一条管道（lseek 必 ESPIPE）。
    pub fn open_pipe(&mut self) -> Result<usize, u32> {
        let mut k = 0usize;
        while k < FD_CAP {
            let free = match self.fds.get(k) {
                Some(f) => !f.used,
                None => false,
            };
            if free {
                self.install(k, false, true, false, 0);
                return Ok(k);
            }
            k = k.wrapping_add(1);
        }
        Err(EMFILE)
    }

    /// 读：目录 EISDIR，超上限 EINVAL，否则截断到剩余长度。
    pub fn read(&mut self, fd: usize, want: u32) -> Result<u32, u32> {
        let f = self.at(fd)?;
        if f.dir {
            return Err(EISDIR);
        }
        if want == 0 {
            return Ok(0);
        }
        if want > RD_MAX {
            return Err(EINVAL);
        }
        let avail = f.size.saturating_sub(f.offset);
        let n = if (want as u64) < avail { want as u64 } else { avail };
        if let Some(s) = self.fds.get_mut(fd) {
            s.offset = s.offset.wrapping_add(n);
        }
        self.reads = self.reads.wrapping_add(1);
        Ok(n as u32)
    }

    /// 写：只读 fd 记 EBADF，越过磁盘上限记 ENOSPC。
    pub fn write(&mut self, fd: usize, bytes: u32) -> Result<u32, u32> {
        let f = self.at(fd)?;
        if f.dir {
            return Err(EISDIR);
        }
        if f.readonly {
            return Err(EBADF);
        }
        let end = f.offset.wrapping_add(bytes as u64);
        if end > DISK_MAX {
            return Err(ENOSPC);
        }
        if let Some(s) = self.fds.get_mut(fd) {
            s.offset = end;
            if end > s.size {
                s.size = end;
            }
        }
        self.writes = self.writes.wrapping_add(1);
        Ok(bytes)
    }

    /// 定位：管道 ESPIPE，whence 非法 EINVAL，负目标 EINVAL，超 32 位 EOVERFLOW。
    pub fn lseek(&mut self, fd: usize, off: i64, whence: u32) -> Result<u64, u32> {
        let f = self.at(fd)?;
        if f.pipe {
            return Err(ESPIPE);
        }
        let base: u64 = match whence {
            SEEK_SET => 0,
            SEEK_CUR => f.offset,
            SEEK_END => f.size,
            _ => return Err(EINVAL),
        };
        let target = base as i128 + off as i128;
        if target < 0 {
            return Err(EINVAL);
        }
        if target > u32::MAX as i128 {
            return Err(EOVERFLOW);
        }
        let t = target as u64;
        if let Some(s) = self.fds.get_mut(fd) {
            s.offset = t;
        }
        Ok(t)
    }

    /// 取状态三元组 (size, offset, is_dir)。
    pub fn fstat(&self, fd: usize) -> Result<(u64, u64, bool), u32> {
        let f = self.at(fd)?;
        Ok((f.size, f.offset, f.dir))
    }

    /// 关闭：槽位复位，二次关闭 EBADF。
    pub fn close(&mut self, fd: usize) -> Result<(), u32> {
        self.at(fd)?;
        if let Some(s) = self.fds.get_mut(fd) {
            *s = Fd::blank();
        }
        self.closed = self.closed.wrapping_add(1);
        Ok(())
    }

    pub fn live(&self) -> u32 {
        let mut n = 0u32;
        let mut k = 0usize;
        while k < FD_CAP {
            if let Some(f) = self.fds.get(k) {
                if f.used {
                    n = n.wrapping_add(1);
                }
            }
            k = k.wrapping_add(1);
        }
        n
    }
}

/// 错误码 → 直方槽下标；不在十类表内返回 None（不进直方，但仍是合法判据）。
pub fn class_index(errno: u32) -> Option<usize> {
    let mut k = 0usize;
    while k < ERRNO_CLASSES.len() {
        if let Some(c) = ERRNO_CLASSES.get(k) {
            if *c == errno {
                return Some(k);
            }
        }
        k = k.wrapping_add(1);
    }
    None
}

/// 文件族账本：条目 + 判据 + 观测 + 十类直方。
#[derive(Clone, Copy, Debug)]
pub struct FileLedger {
    cases: [Case; CAP],
    verdicts: [u32; CAP],
    seen: [bool; CAP],
    hist: [u32; 10],
    len: usize,
    attempts: u32,
    accepts: u32,
    rejects: u32,
    observed: u32,
    matched: u32,
    zero: u32,
    frozen: bool,
    root: Option<u64>,
}

impl FileLedger {
    pub const fn new() -> FileLedger {
        FileLedger {
            cases: [Case::blank(); CAP],
            verdicts: [0u32; CAP],
            seen: [false; CAP],
            hist: [0u32; 10],
            len: 0,
            attempts: 0,
            accepts: 0,
            rejects: 0,
            observed: 0,
            matched: 0,
            zero: 0,
            frozen: false,
            root: None,
        }
    }

    fn fail(&mut self, code: u32) -> u32 {
        self.rejects = self.rejects.wrapping_add(1);
        code
    }

    /// 登记一条样本：只有 id 域、重复、顺序、定容、封册五道闸。
    /// 状态位与长度一律照单全收——越界样本要进规则产出 errno。
    pub fn register(&mut self, c: Case) -> Result<usize, u32> {
        self.attempts = self.attempts.wrapping_add(1);
        if self.frozen {
            return Err(self.fail(FS_EFROZEN));
        }
        if c.id < ID_MIN || c.id > ID_MAX {
            return Err(self.fail(FS_EID));
        }
        let mut k = 0usize;
        while k < self.len {
            if let Some(x) = self.cases.get(k) {
                if x.id == c.id {
                    return Err(self.fail(FS_EDUP));
                }
            }
            k = k.wrapping_add(1);
        }
        if self.len > 0 {
            if let Some(last) = self.cases.get(self.len - 1) {
                if c.id <= last.id {
                    return Err(self.fail(FS_EORDER));
                }
            }
        }
        if self.len >= CAP {
            return Err(self.fail(FS_EFULL));
        }
        if let Some(slot) = self.cases.get_mut(self.len) {
            *slot = c;
        }
        self.len = self.len.wrapping_add(1);
        self.accepts = self.accepts.wrapping_add(1);
        Ok(self.len - 1)
    }

    /// 登记并当场复算判据，把结果落进 verdicts 与直方。
    pub fn register_ruled(&mut self, c: Case) -> Result<(usize, u32), u32> {
        let idx = self.register(c)?;
        let v = file_verdict(&c);
        if let Some(slot) = self.verdicts.get_mut(idx) {
            *slot = v;
        }
        if v == 0 {
            self.zero = self.zero.wrapping_add(1);
        } else if let Some(ci) = class_index(v) {
            if let Some(h) = self.hist.get_mut(ci) {
                *h = h.wrapping_add(1);
            }
        }
        Ok((idx, v))
    }

    /// 观测一条实测 errno；与登记判据一致返回 true（不计拒绝）。
    pub fn observe(&mut self, id: u32, seen_code: u32) -> Result<bool, u32> {
        if self.frozen {
            return Err(FS_EFROZEN);
        }
        let mut k = 0usize;
        let mut hit: Option<usize> = None;
        while k < self.len {
            if let Some(x) = self.cases.get(k) {
                if x.id == id {
                    hit = Some(k);
                }
            }
            k = k.wrapping_add(1);
        }
        let idx = match hit {
            Some(v) => v,
            None => return Err(FS_EUNSEEN),
        };
        match self.seen.get(idx) {
            Some(true) => return Err(FS_EDOBS),
            _ => {}
        }
        if let Some(slot) = self.seen.get_mut(idx) {
            *slot = true;
        }
        self.observed = self.observed.wrapping_add(1);
        let expect = match self.verdicts.get(idx) {
            Some(v) => *v,
            None => 0,
        };
        if seen_code == expect {
            self.matched = self.matched.wrapping_add(1);
            Ok(true)
        } else {
            Ok(false)
        }
    }

    pub fn len(&self) -> usize {
        self.len
    }
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }
    pub fn attempts(&self) -> u32 {
        self.attempts
    }
    pub fn accepts(&self) -> u32 {
        self.accepts
    }
    pub fn rejects(&self) -> u32 {
        self.rejects
    }
    pub fn observed(&self) -> u32 {
        self.observed
    }
    pub fn matched(&self) -> u32 {
        self.matched
    }
    pub fn zero_verdicts(&self) -> u32 {
        self.zero
    }
    pub fn hist_at(&self, i: usize) -> u32 {
        match self.hist.get(i) {
            Some(v) => *v,
            None => 0,
        }
    }
    pub fn hist_sum(&self) -> u32 {
        let mut s = 0u32;
        let mut k = 0usize;
        while k < 10 {
            if let Some(v) = self.hist.get(k) {
                s = s.wrapping_add(*v);
            }
            k = k.wrapping_add(1);
        }
        s
    }
    pub fn case_at(&self, i: usize) -> Option<Case> {
        self.cases.get(i).copied()
    }
    pub fn verdict_at(&self, i: usize) -> Option<u32> {
        self.verdicts.get(i).copied()
    }
    pub fn is_frozen(&self) -> bool {
        self.frozen
    }
    pub fn root(&self) -> Option<u64> {
        self.root
    }

    /// FNV-1a 覆盖条目、判据、观测位、直方与全部计数器。
    pub fn fold(&self) -> u64 {
        let mut h = FNV_OFF ^ TAG;
        h = (h ^ self.len as u64).wrapping_mul(FNV_PRIME);
        h = (h ^ self.attempts as u64).wrapping_mul(FNV_PRIME);
        h = (h ^ self.accepts as u64).wrapping_mul(FNV_PRIME);
        h = (h ^ self.rejects as u64).wrapping_mul(FNV_PRIME);
        h = (h ^ self.observed as u64).wrapping_mul(FNV_PRIME);
        h = (h ^ self.matched as u64).wrapping_mul(FNV_PRIME);
        h = (h ^ self.zero as u64).wrapping_mul(FNV_PRIME);
        h = (h ^ self.frozen as u64).wrapping_mul(FNV_PRIME);
        let mut k = 0usize;
        while k < self.len {
            if let Some(c) = self.cases.get(k) {
                h = (h ^ c.id as u64).wrapping_mul(FNV_PRIME);
                h = (h ^ c.op as u64).wrapping_mul(FNV_PRIME);
                h = (h ^ c.fd as u64).wrapping_mul(FNV_PRIME);
                h = (h ^ c.off).wrapping_mul(FNV_PRIME);
                h = (h ^ c.len as u64).wrapping_mul(FNV_PRIME);
                h = (h ^ c.flags as u64).wrapping_mul(FNV_PRIME);
            }
            if let Some(v) = self.verdicts.get(k) {
                h = (h ^ *v as u64).wrapping_mul(FNV_PRIME);
            }
            if let Some(b) = self.seen.get(k) {
                h = (h ^ *b as u64).wrapping_mul(FNV_PRIME);
            }
            k = k.wrapping_add(1);
        }
        let mut j = 0usize;
        while j < 10 {
            if let Some(v) = self.hist.get(j) {
                h = (h ^ *v as u64).wrapping_mul(FNV_PRIME);
            }
            j = j.wrapping_add(1);
        }
        h
    }

    /// 复账：全登记、全观测、全复算一致、两张表平衡。
    pub fn replay(&self) -> Result<u64, u32> {
        if self.len == 0 {
            return Err(FS_EUNOBS);
        }
        if self.observed as usize != self.len || self.matched as usize != self.len {
            return Err(FS_EUNOBS);
        }
        if self.accepts.wrapping_add(self.rejects) != self.attempts {
            return Err(FS_EBALANCE);
        }
        if self.zero.wrapping_add(self.hist_sum()) != self.len as u32 {
            return Err(FS_EBALANCE);
        }
        let mut k = 0usize;
        while k < self.len {
            let c = match self.cases.get(k) {
                Some(v) => *v,
                None => return Err(FS_EUNOBS),
            };
            let v = match self.verdicts.get(k) {
                Some(x) => *x,
                None => return Err(FS_EMISMATCH),
            };
            if file_verdict(&c) != v {
                return Err(FS_EMISMATCH);
            }
            match self.seen.get(k) {
                Some(true) => {}
                _ => return Err(FS_EUNOBS),
            }
            k = k.wrapping_add(1);
        }
        Ok(self.fold())
    }

    /// 封册：先复账再上锁；二次封册即 EFROZEN。
    pub fn freeze(&mut self) -> Result<u64, u32> {
        if self.frozen {
            return Err(FS_EFROZEN);
        }
        let h = self.replay()?;
        self.frozen = true;
        self.root = Some(h);
        Ok(h)
    }
}

/// 便捷构造：一条样本的最小字段集（id 由调用方补）。
pub fn probe(op: u32, flags: u32, len: u32, off: u64) -> Case {
    Case { id: 1, op, fd: 0, off, len, flags }
}

/// 十类错误码的触发样本：第 i 槽的规则输出必须等于 ERRNO_CLASSES[i]。
/// 每槽独立构造，不共享状态；样本 id 只作占位（不登记进账本）。
pub fn class_probe(i: usize) -> (Case, u32) {
    match i {
        0 => (probe(OP_READ, 0, 8, 0), EBADF),
        1 => (probe(OP_READ, F_FD_OPEN | F_BUF_NULL, 8, 0), EFAULT),
        2 => (probe(OP_LSEEK, F_FD_OPEN | F_WHENCE_BAD, 0, 0), EINVAL),
        3 => (probe(OP_OPENAT, F_PATH_EMPTY, 0, 0), ENOENT),
        4 => (probe(OP_OPENAT, F_PARENT_NOX, 0, 0), EACCES),
        5 => (probe(OP_READ, F_FD_OPEN | F_FD_DIR, 8, 0), EISDIR),
        6 => (probe(OP_OPENAT, F_O_DIRECTORY, 0, 0), ENOTDIR),
        7 => (probe(OP_LSEEK, F_FD_OPEN | F_FD_PIPE, 0, 0), ESPIPE),
        8 => (probe(OP_WRITE, F_FD_OPEN, 0x200, 0x0f_ff00), ENOSPC),
        9 => (probe(OP_LSEEK, F_FD_OPEN, 0, 0x8000_0000_0000), EOVERFLOW),
        _ => (Case::blank(), EINVAL),
    }
}

/// 十二条规格样本：一条零判据 + 十一条覆盖十类错误码（EBADF 两条）。
pub fn spec_of(id: usize) -> (Case, u32) {
    match id {
        1 => (probe(OP_OPENAT, 0, 0, 0), 0),
        2 => (probe(OP_OPENAT, F_PATH_EMPTY, 0, 0), ENOENT),
        3 => (probe(OP_OPENAT, F_PARENT_NOX, 0, 0), EACCES),
        4 => (probe(OP_OPENAT, F_O_DIRECTORY, 0, 0), ENOTDIR),
        5 => (probe(OP_READ, 0, 8, 0), EBADF),
        6 => (probe(OP_READ, F_FD_OPEN | F_FD_DIR, 8, 0), EISDIR),
        7 => (probe(OP_READ, F_FD_OPEN | F_BUF_NULL, 8, 0), EFAULT),
        8 => (probe(OP_READ, F_FD_OPEN, 0x8000_0000, 0), EINVAL),
        9 => (probe(OP_WRITE, F_FD_OPEN | F_FD_RDONLY, 4, 0), EBADF),
        10 => (probe(OP_WRITE, F_FD_OPEN, 0x200, 0x0f_ff00), ENOSPC),
        11 => (probe(OP_LSEEK, F_FD_OPEN | F_FD_PIPE, 0, 0), ESPIPE),
        12 => (probe(OP_LSEEK, F_FD_OPEN, 0, 0x8000_0000_0000), EOVERFLOW),
        _ => (Case::blank(), FS_EID),
    }
}

/// 按 id 1..=CAP 逐条登记 + 观测，判据取自已登记的 verdict。
pub fn build_ledger() -> FileLedger {
    let mut l = FileLedger::new();
    let mut i = 1usize;
    while i <= CAP {
        let (mut c, expect) = spec_of(i);
        c.id = i as u32;
        if l.register_ruled(c).is_ok() {
            let _ = l.observe(c.id, expect);
        }
        i = i.wrapping_add(1);
    }
    l
}

/// 四条独立拒绝：顺序、重复、越界、封册。
pub fn inject_four() -> [bool; 4] {
    let mut a = FileLedger::new();
    let (mut c1, _) = spec_of(1);
    c1.id = 2;
    let (_c2, _) = spec_of(2);
    let mut c0 = spec_of(1).0;
    c0.id = 1;
    let _ = a.register(c1);
    let bad_a = a.register(c0).err() == Some(FS_EORDER);

    let mut b = FileLedger::new();
    let _ = b.register(spec_of(1).0);
    let bad_b = b.register(spec_of(1).0).err() == Some(FS_EDUP);

    let mut c = FileLedger::new();
    let mut wide = spec_of(1).0;
    wide.id = ID_MAX + 1;
    let bad_c = c.register(wide).err() == Some(FS_EID);

    let mut d = build_ledger();
    let _ = d.freeze();
    let mut late = spec_of(1).0;
    late.id = 13;
    let bad_d = d.register(late).err() == Some(FS_EFROZEN);
    [bad_a, bad_b, bad_c, bad_d]
}

/// 十三条自检：前十项压判据面（规则/十类透传/真 fd 表/账本），
/// 后三项压拒绝面与封册面。
pub fn run_unx11276_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F11276");

    let layout = SEG_BASE == 0x1f4c0
        && FS_EID == SEG_BASE + 0x01
        && FS_EORDER == SEG_BASE + 0x02
        && FS_EDUP == SEG_BASE + 0x03
        && FS_EFULL == SEG_BASE + 0x04
        && FS_EFROZEN == SEG_BASE + 0x05
        && FS_EUNSEEN == SEG_BASE + 0x06
        && FS_EMISMATCH == SEG_BASE + 0x07
        && FS_EUNOBS == SEG_BASE + 0x08
        && FS_EDOBS == SEG_BASE + 0x09
        && FS_EBALANCE == SEG_BASE + 0x0a
        && ERRNO_CLASSES.len() == 10;
    s.add("S1-段布局与十类错误码表", layout,
        "0x1f4c0 段十个错误码连续，十类 errno 表定长");

    let open_ok = file_verdict(&probe(OP_OPENAT, 0, 0, 0)) == 0;
    let open_missing = file_verdict(&probe(OP_OPENAT, F_PATH_EMPTY, 0, 0)) == ENOENT;
    let open_prio = file_verdict(&probe(OP_OPENAT, F_PATH_EMPTY | F_PARENT_NOX, 0, 0)) == ENOENT;
    let open_deny = file_verdict(&probe(OP_OPENAT, F_PARENT_NOX, 0, 0)) == EACCES;
    let open_loop = file_verdict(&probe(OP_OPENAT, F_O_NOFOLLOW | F_SYMLINK, 0, 0)) == ELOOP;
    s.add("S2-openat 路径与权限判据", open_ok && open_missing && open_prio && open_deny && open_loop,
        "空路径先于权限判 ENOENT，O_NOFOLLOW 撞符号链接判 ELOOP");

    let rd_closed = file_verdict(&probe(OP_READ, 0, 8, 0)) == EBADF;
    let rd_null = file_verdict(&probe(OP_READ, F_FD_OPEN | F_BUF_NULL, 8, 0)) == EFAULT;
    let rd_dir = file_verdict(&probe(OP_READ, F_FD_OPEN | F_FD_DIR, 8, 0)) == EISDIR;
    let rd_max = file_verdict(&probe(OP_READ, F_FD_OPEN, RD_MAX, 0)) == 0;
    let rd_over = file_verdict(&probe(OP_READ, F_FD_OPEN, RD_MAX + 1, 0)) == EINVAL;
    let rd_zero = file_verdict(&probe(OP_READ, F_FD_OPEN, 0, 0)) == 0;
    s.add("S3-read 有效性、目录与长度上限", rd_closed && rd_null && rd_dir && rd_max && rd_over && rd_zero,
        "关闭 fd EBADF，空缓冲 EFAULT，目录 EISDIR，超 0x7ffff000 EINVAL");

    let wr_closed = file_verdict(&probe(OP_WRITE, 0, 4, 0)) == EBADF;
    let wr_ro = file_verdict(&probe(OP_WRITE, F_FD_OPEN | F_FD_RDONLY, 4, 0)) == EBADF;
    let wr_edge = file_verdict(&probe(OP_WRITE, F_FD_OPEN, 0x100, DISK_MAX - 0x100)) == 0;
    let wr_over = file_verdict(&probe(OP_WRITE, F_FD_OPEN, 0x101, DISK_MAX - 0x100)) == ENOSPC;
    let wr_zero = file_verdict(&probe(OP_WRITE, F_FD_OPEN, 0, 0)) == 0;
    s.add("S4-write 只读位与磁盘末址", wr_closed && wr_ro && wr_edge && wr_over && wr_zero,
        "末址恰等 1 MiB 放行，越一字节即 ENOSPC");

    let lk_pipe = file_verdict(&probe(OP_LSEEK, F_FD_OPEN | F_FD_PIPE, 0, 0)) == ESPIPE;
    let lk_whence = file_verdict(&probe(OP_LSEEK, F_FD_OPEN | F_WHENCE_BAD, 0, 0)) == EINVAL;
    let lk_max = file_verdict(&probe(OP_LSEEK, F_FD_OPEN, 0, LSEEK_MAX)) == 0;
    let lk_over = file_verdict(&probe(OP_LSEEK, F_FD_OPEN, 0, LSEEK_MAX + 1)) == EOVERFLOW;
    let lk_closed = file_verdict(&probe(OP_LSEEK, 0, 0, 0)) == EBADF;
    s.add("S5-lseek 管道、whence 与偏移溢出", lk_pipe && lk_whence && lk_max && lk_over && lk_closed,
        "管道 ESPIPE，坏 whence EINVAL，超 47 位 EOVERFLOW");

    let st_closed = file_verdict(&probe(OP_FSTAT, 0, 0, 0)) == EBADF;
    let st_null = file_verdict(&probe(OP_FSTAT, F_FD_OPEN | F_BUF_NULL, 0, 0)) == EFAULT;
    let st_ok = file_verdict(&probe(OP_FSTAT, F_FD_OPEN, 0, 0)) == 0;
    let cl_closed = file_verdict(&probe(OP_CLOSE, 0, 0, 0)) == EBADF;
    let cl_ok = file_verdict(&probe(OP_CLOSE, F_FD_OPEN, 0, 0)) == 0;
    s.add("S6-fstat 与 close 判据", st_closed && st_null && st_ok && cl_closed && cl_ok,
        "fstat 空缓冲 EFAULT，close 二次 EBADF");

    let mut classes_ok = true;
    let mut k = 0usize;
    while k < ERRNO_CLASSES.len() {
        let (c, want) = class_probe(k);
        let got = file_verdict(&c);
        let tbl = match ERRNO_CLASSES.get(k) {
            Some(v) => *v,
            None => 0,
        };
        if got != want || want != tbl {
            classes_ok = false;
        }
        k = k.wrapping_add(1);
    }
    s.add("S7-十类错误码逐类透传", classes_ok,
        "十类 errno 各有一条独立触发样本且与常量表对齐");

    let mut t = FdTable::new();
    let mut truth = true;
    let fd = match t.open(false, false, 0) {
        Ok(v) => v,
        Err(_) => 99,
    };
    truth = truth && fd == 0 && t.live() == 1;
    truth = truth && t.write(fd, 5) == Ok(5);
    truth = truth && t.fstat(fd) == Ok((5, 5, false));
    truth = truth && t.lseek(fd, 0, SEEK_SET) == Ok(0);
    truth = truth && t.read(fd, 8) == Ok(5);
    truth = truth && t.read(fd, 8) == Ok(0);
    s.add("S8-定容 fd 表真开真读写", truth,
        "open→write 5→fstat→lseek 0→read 5→EOF 0 逐值正确");

    let led = build_ledger();
    let eb = class_index(EBADF) == Some(0);
    let balance = led.len() == 12
        && led.zero_verdicts() == 1
        && led.hist_sum() == 11
        && led.hist_at(0) == 2
        && led.hist_at(8) == 1
        && eb;
    s.add("S9-登记账与十类直方平衡", balance,
        "12 条登记 = 1 条零判据 + 11 条错误，EBADF 占两槽");

    let replay_ok = led.replay().is_ok();
    s.add("S10-全观测且复算一致", replay_ok && led.observed() == 12 && led.matched() == 12,
        "replay 逐条复算 12 条判据并与观测对齐");

    let mut g = FileLedger::new();
    let mut c2 = spec_of(2).0;
    c2.id = 2;
    let _ = g.register(c2);
    let mut c1 = spec_of(1).0;
    c1.id = 1;
    let e_order = g.register(c1).err();
    let e_dup = g.register(c2).err();
    let mut wide = spec_of(1).0;
    wide.id = 0;
    let e_id = g.register(wide).err();
    let mut full = build_ledger();
    let mut extra = spec_of(1).0;
    extra.id = 13;
    let e_full = full.register(extra).err();
    s.add("S11-顺序重复越界定容四闸", e_order == Some(FS_EORDER)
        && e_dup == Some(FS_EDUP)
        && e_id == Some(FS_EID)
        && e_full == Some(FS_EFULL)
        && g.accepts() == 1
        && g.rejects() == 3
        && g.attempts() == 4,
        "倒退 EORDER、重复 EDUP、越界 EID、满表 EFULL 各自独立");

    let mut ob = FileLedger::new();
    let mut c5 = spec_of(5).0;
    c5.id = 5;
    let _ = ob.register_ruled(c5);
    let miss = ob.replay().err() == Some(FS_EUNOBS);
    let unseen = ob.observe(99, 0).err() == Some(FS_EUNSEEN);
    let wrong = ob.observe(5, 0) == Ok(false);
    let dob = ob.observe(5, EBADF).err() == Some(FS_EDOBS);
    s.add("S12-观测面三门", miss && unseen && wrong && dob,
        "未观测 EUNOBS、未登记 id EUNSEEN、错观测 false、重观测 EDOBS");

    let mut fz = build_ledger();
    let h = match fz.freeze() {
        Ok(v) => v,
        Err(_) => 0,
    };
    let again = fz.freeze();
    let mut late = spec_of(1).0;
    late.id = 13;
    let post = fz.register(late).err();
    s.add_dyn("S13-封册先复账后上锁", h != 0
        && again == Err(FS_EFROZEN)
        && post == Some(FS_EFROZEN)
        && fz.is_frozen()
        && fz.root() == Some(h)
        && fz.replay().is_ok(),
        &format!("根={:x} 二次={:?} 封后写={:?}", h, again, post));
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unx11276_ledger_flow() {
        let l = build_ledger();
        assert_eq!(l.len(), 12);
        assert_eq!(l.accepts(), 12);
        assert_eq!(l.rejects(), 0);
        assert_eq!(l.zero_verdicts(), 1);
        assert_eq!(l.hist_sum(), 11);
        assert_eq!(l.observed(), 12);
        assert_eq!(l.matched(), 12);
        assert!(l.replay().is_ok());
        assert_eq!(l.verdict_at(0), Some(0));
        assert_eq!(l.verdict_at(11), Some(EOVERFLOW));
    }

    #[test]
    fn unx11276_rule_edges() {
        let mut k = 0usize;
        while k < 10 {
            let (c, want) = class_probe(k);
            assert_eq!(file_verdict(&c), want);
            match ERRNO_CLASSES.get(k) {
                Some(v) => assert_eq!(want, *v),
                None => assert!(false),
            }
            k += 1;
        }
        assert_eq!(file_verdict(&probe(OP_OPENAT, F_O_NOFOLLOW | F_SYMLINK, 0, 0)), ELOOP);
        assert_eq!(file_verdict(&probe(OP_READ, F_FD_OPEN, RD_MAX, 0)), 0);
        assert_eq!(file_verdict(&probe(OP_READ, F_FD_OPEN, RD_MAX + 1, 0)), EINVAL);
        assert_eq!(file_verdict(&probe(OP_WRITE, F_FD_OPEN, 0x100, DISK_MAX - 0x100)), 0);
        assert_eq!(file_verdict(&probe(OP_WRITE, F_FD_OPEN, 0x101, DISK_MAX - 0x100)), ENOSPC);
        assert_eq!(file_verdict(&probe(OP_LSEEK, F_FD_OPEN, 0, LSEEK_MAX + 1)), EOVERFLOW);
    }

    #[test]
    fn unx11276_fd_table() {
        let mut t = FdTable::new();
        let fd = match t.open(false, false, 0) {
            Ok(v) => v,
            Err(_) => 99,
        };
        assert_eq!(t.write(fd, 5), Ok(5));
        assert_eq!(t.fstat(fd), Ok((5, 5, false)));
        assert_eq!(t.lseek(fd, 0, SEEK_SET), Ok(0));
        assert_eq!(t.read(fd, 3), Ok(3));
        assert_eq!(t.read(fd, 9), Ok(2));
        assert_eq!(t.read(fd, 9), Ok(0));
        assert_eq!(t.lseek(fd, -1, SEEK_CUR), Ok(4));
        assert_eq!(t.lseek(fd, -6, SEEK_SET), Err(EINVAL));
        assert_eq!(t.lseek(fd, 1 << 33, SEEK_SET), Err(EOVERFLOW));
        assert_eq!(t.lseek(fd, 0, 7), Err(EINVAL));
        let p = match t.open_pipe() {
            Ok(v) => v,
            Err(_) => 99,
        };
        assert_eq!(t.lseek(p, 0, SEEK_SET), Err(ESPIPE));
        assert_eq!(t.close(p), Ok(()));
        assert_eq!(t.close(p), Err(EBADF));
        assert_eq!(t.read(99, 4), Err(EBADF));
        let d = match t.open(true, true, 0) {
            Ok(v) => v,
            Err(_) => 99,
        };
        assert_eq!(t.read(d, 4), Err(EISDIR));
        assert_eq!(t.write(d, 4), Err(EISDIR));
        let mut full = FdTable::new();
        let mut i = 0usize;
        while i < FD_CAP {
            assert!(full.open(false, false, 0).is_ok());
            i += 1;
        }
        assert_eq!(full.open(false, false, 0), Err(EMFILE));
        assert_eq!(full.live(), FD_CAP as u32);
    }

    #[test]
    fn unx11276_refusals() {
        assert_eq!(inject_four(), [true, true, true, true]);
        let mut l = FileLedger::new();
        let mut c = spec_of(1).0;
        c.id = 7;
        assert!(l.register(c).is_ok());
        assert_eq!(l.register(c).err(), Some(FS_EDUP));
        assert_eq!(l.attempts(), 2);
        assert_eq!(l.rejects(), 1);
        assert_eq!(l.accepts(), 1);
        let mut wide = spec_of(1).0;
        wide.id = ID_MAX + 1;
        assert_eq!(l.register(wide).err(), Some(FS_EID));
    }

    #[test]
    fn unx11276_freeze_and_checks() {
        let mut l = build_ledger();
        let root = match l.freeze() {
            Ok(v) => v,
            Err(_) => 0,
        };
        assert!(root != 0);
        assert_eq!(l.root(), Some(root));
        assert_eq!(l.freeze(), Err(FS_EFROZEN));
        assert!(l.replay().is_ok());
        let s = run_unx11276_checks();
        assert!(s.all_passed());
        assert_eq!(s.red_items_text(), "");
    }
}
