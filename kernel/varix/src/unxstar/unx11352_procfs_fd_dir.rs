//! UNX-F11352 /proc/self/fd 与描述符呈现（0xec00 段）
//!
//! `/proc/self/fd` 把进程的描述符表投影成一个目录，目录项的名字就是描述符号，
//! 每个名字是一个符号链接，链接目标描述该描述符此刻指向什么：
//!
//! - 普通文件指向路径；
//! - 管道指向 `pipe:[inode]`；
//! - 目录流指向它自己。
//!
//! 这份投影有三条容易写错、但对诊断工具必须成立的语义：
//!
//! 1. **最小号复用**：新开描述符总是拿到当时最小的空闲号。用户态工具靠这一点
//!    推断「上一轮关掉的号是不是被新一轮拿走了」。
//! 2. **目录自现**：`opendir("/proc/self/fd")` 自己也要占一个号，并且这个号会
//!    出现在正在读的列表里（读者能看到自己）。
//! 3. **消失即缺失**：描述符号关闭后，对应的目录项不是「坏描述符」，而是整条
//!    链接不存在——读它得到 `ENOENT`，而不是 `EBADF`。二者的区别正是诊断工具
//!    区分「本来就没人打开」与「我拿错了号」的依据。
//!
//! 另外两条边界：
//!
//! - `exec` 只清除带 `CLOEXEC` 的描述符，其余原样保留；清除掉的那些在新映像里
//!   连链接都不存在。
//! - 扫描是快照：一次 `opendir` 只能配一次 `scan`，读完再读必须重新 `opendir`；
//!   扫描进行中把目录流自己的号关掉，等于读一半把书合上，必须拒绝
//!   （`EDIR`）；同样，扫描进行中执行 `exec` 也会使快照失效（`EBUSY`）。
//!
//! 闸门次序：
//! - `open`：类非法 → 目标为零 → 标志非法 → 表满；
//! - `close`：号越界 → 未打开 → 扫描中关目录流；
//! - `opendir`：已有未读目录流 → 标志非法 → 表满；
//! - `scan`：没有目录流（或已读过）；
//! - `exec`：扫描进行中。

use crate::alloc::format;
use crate::checks::CheckSet;

/// 段基址。
pub const SEG_BASE: u32 = 0xec00;

/// 参数非法。
pub const FD_EINVAL: u32 = SEG_BASE + 0x01;
/// 描述符号坏（从未打开或已关闭）。
pub const FD_EBADF: u32 = SEG_BASE + 0x02;
/// 链接不存在（对应的号已经不在表里）。
pub const FD_ENOENT: u32 = SEG_BASE + 0x03;
/// 描述符表满。
pub const FD_EFULL: u32 = SEG_BASE + 0x04;
/// 目录流已读或未开。
pub const FD_ERUN: u32 = SEG_BASE + 0x05;
/// 扫描进行中关目录流。
pub const FD_EDIR: u32 = SEG_BASE + 0x06;
/// 观察下标越界。
pub const FD_ENOBS: u32 = SEG_BASE + 0x07;
/// 列表未严格升序。
pub const FD_ESEQ: u32 = SEG_BASE + 0x08;
/// 账本形状不符。
pub const FD_EBAL: u32 = SEG_BASE + 0x09;
/// 槽位乱序。
pub const FD_EORDER: u32 = SEG_BASE + 0x0a;
/// 观测值与复算不符。
pub const FD_EMISMATCH: u32 = SEG_BASE + 0x0b;
/// 冻结后重放不一致。
pub const FD_ESEAL: u32 = SEG_BASE + 0x0c;
/// 扫描进行中执行 exec。
pub const FD_EBUSY: u32 = SEG_BASE + 0x0d;

/// 单表容量。
pub const CAP: usize = 6;
/// 账本格数。
pub const SAMPLES: usize = 14;

/// 操作码：打开。
pub const OP_OPEN: u8 = 1;
/// 操作码：关闭。
pub const OP_CLOSE: u8 = 2;
/// 操作码：开目录流。
pub const OP_OPENDIR: u8 = 3;
/// 操作码：扫描。
pub const OP_SCAN: u8 = 4;
/// 操作码：读链接。
pub const OP_LINK: u8 = 5;
/// 操作码：执行。
pub const OP_EXEC: u8 = 6;

/// 描述符类：普通文件。
pub const K_FILE: u8 = 1;
/// 描述符类：管道。
pub const K_PIPE: u8 = 2;
/// 描述符类：目录流。
pub const K_DIR: u8 = 3;

/// 标志：可读。
pub const F_READ: u8 = 1;
/// 标志：可写。
pub const F_WRITE: u8 = 2;
/// 标志：执行时关闭。
pub const F_CLOEXEC: u8 = 4;
/// 合法标志掩码。
pub const F_VALID: u8 = F_READ | F_WRITE | F_CLOEXEC;

/// 目录流自指的链接目标。
pub const T_DIR: u32 = 0xffff_fffd;

/// FNV 偏移基。
pub const FNV_OFF: u64 = 0xcbf2_9ce4_8422_2325;
/// FNV 质数。
pub const FNV_PRIME: u64 = 0x100_0000_01b3;

/// 折入一个 64 位字。
pub fn fold(h: u64, v: u64) -> u64 {
    let mut x = h;
    let mut n = v;
    let mut i = 0u32;
    while i < 8 {
        x ^= n & 0xff;
        x = x.wrapping_mul(FNV_PRIME);
        n >>= 8;
        i += 1;
    }
    x
}

/// 一个描述符。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Fd {
    /// 描述符号。
    pub num: u32,
    /// 描述符类。
    pub kind: u8,
    /// 链接目标。
    pub target: u32,
    /// 标志位。
    pub flags: u8,
}

impl Fd {
    /// 是否带执行时关闭。
    pub fn cloexec(&self) -> bool {
        self.flags & F_CLOEXEC != 0
    }

    /// 是否可读。
    pub fn readable(&self) -> bool {
        self.flags & F_READ != 0
    }

    /// 是否可写。
    pub fn writable(&self) -> bool {
        self.flags & F_WRITE != 0
    }

    /// 链接目标是否指向目录流自身。
    pub fn self_dir(&self) -> bool {
        self.kind == K_DIR && self.target == T_DIR
    }
}

/// 描述符表投影。
#[derive(Clone, Copy, Debug)]
pub struct FdDir {
    slots: [Fd; CAP],
    n: usize,
    used: u64,
    self_fd: u32,
    scanning: bool,
    scanned: bool,
    listed: [u32; CAP + 1],
    listed_len: usize,
    opens: usize,
    closes: usize,
    kills: u32,
}

impl FdDir {
    /// 空表：没有任何描述符（0、1、2 由调用方显式打开）。
    pub const fn new() -> FdDir {
        FdDir {
            slots: [Fd { num: 0, kind: 0, target: 0, flags: 0 }; CAP],
            n: 0,
            used: 0,
            self_fd: 0,
            scanning: false,
            scanned: false,
            listed: [0; CAP + 1],
            listed_len: 0,
            opens: 0,
            closes: 0,
            kills: 0,
        }
    }

    /// 打开的描述符条数。
    pub fn count(&self) -> usize {
        self.n
    }

    /// 打开次数。
    pub fn opens(&self) -> usize {
        self.opens
    }

    /// 关闭次数。
    pub fn closes(&self) -> usize {
        self.closes
    }

    /// 执行时被清除的条数。
    pub fn kills(&self) -> u32 {
        self.kills
    }

    /// 当前目录流自己的号（未开为 0）。
    pub fn self_fd(&self) -> u32 {
        self.self_fd
    }

    /// 是否正在扫描。
    pub fn scanning(&self) -> bool {
        self.scanning
    }

    /// 是否已扫描过。
    pub fn scanned(&self) -> bool {
        self.scanned
    }

    /// 列表长度。
    pub fn listed_len(&self) -> usize {
        self.listed_len
    }

    /// 号是否在表里。
    pub fn is_open(&self, num: u32) -> bool {
        if num >= 64 {
            return false;
        }
        self.used & (1u64 << num) != 0
    }

    /// 第 i 个槽。
    pub fn slot_at(&self, i: usize) -> Option<Fd> {
        if i >= self.n {
            return None;
        }
        self.slots.get(i).copied()
    }

    /// 列表第 i 项。
    pub fn readdir_at(&self, i: usize) -> Result<u32, u32> {
        if i >= self.listed_len {
            return Err(FD_ENOBS);
        }
        match self.listed.get(i) {
            Some(v) => Ok(*v),
            None => Err(FD_ENOBS),
        }
    }

    /// 按号找槽。
    fn find(&self, num: u32) -> Option<usize> {
        let mut i = 0usize;
        while i < self.n {
            match self.slots.get(i) {
                Some(f) => {
                    if f.num == num {
                        return Some(i);
                    }
                }
                None => return None,
            }
            i += 1;
        }
        None
    }

    /// 取最小空闲号。
    fn lowest_free(&self) -> Option<u32> {
        let mut b = 0u32;
        while b < 64 {
            if self.used & (1u64 << b) == 0 {
                return Some(b);
            }
            b += 1;
        }
        None
    }

    /// 放下一个描述符。
    fn install(&mut self, kind: u8, target: u32, flags: u8) -> Result<usize, u32> {
        let num = match self.lowest_free() {
            Some(v) => v,
            None => return Err(FD_EFULL),
        };
        if let Some(slot) = self.slots.get_mut(self.n) {
            *slot = Fd { num, kind, target, flags };
        }
        self.n += 1;
        self.used |= 1u64 << num;
        self.opens += 1;
        Ok(num as usize)
    }

    /// 打开一个描述符：最小空闲号。
    pub fn open(&mut self, kind: u8, target: u32, flags: u8) -> Result<usize, u32> {
        if kind == 0 || kind > K_DIR {
            return Err(FD_EINVAL);
        }
        if target == 0 {
            return Err(FD_EINVAL);
        }
        if flags & !F_VALID != 0 {
            return Err(FD_EINVAL);
        }
        if self.n >= CAP {
            return Err(FD_EFULL);
        }
        self.install(kind, target, flags)
    }

    /// 关闭一个描述符。
    pub fn close(&mut self, num: u32) -> Result<usize, u32> {
        if num >= 64 || self.used & (1u64 << num) == 0 {
            return Err(FD_EBADF);
        }
        if self.scanning && self.self_fd == num {
            return Err(FD_EDIR);
        }
        let at = match self.find(num) {
            Some(v) => v,
            None => return Err(FD_EBADF),
        };
        let mut i = at;
        while i + 1 < self.n {
            match self.slots.get(i + 1).copied() {
                Some(next) => {
                    if let Some(dst) = self.slots.get_mut(i) {
                        *dst = next;
                    }
                }
                None => break,
            }
            i += 1;
        }
        self.n -= 1;
        self.used &= !(1u64 << num);
        self.closes += 1;
        Ok(num as usize)
    }

    /// 读链接：号不在表里就是整条链接不存在。
    pub fn readlink(&self, num: u32) -> Result<u32, u32> {
        if !self.is_open(num) {
            return Err(FD_ENOENT);
        }
        match self.find(num) {
            Some(at) => match self.slots.get(at) {
                Some(f) => Ok(f.target),
                None => Err(FD_ENOENT),
            },
            None => Err(FD_ENOENT),
        }
    }

    /// 打开目录流：自己占一个号，并进入扫描状态。
    pub fn opendir(&mut self, flags: u8) -> Result<usize, u32> {
        if self.scanning {
            return Err(FD_ERUN);
        }
        if flags & !F_VALID != 0 {
            return Err(FD_EINVAL);
        }
        if self.n >= CAP {
            return Err(FD_EFULL);
        }
        let num = self.install(K_DIR, T_DIR, flags)?;
        self.self_fd = num as u32;
        self.scanning = true;
        self.scanned = false;
        self.listed_len = 0;
        Ok(num)
    }

    /// 扫描列表：升序，且必然包含目录流自己的号。
    pub fn scan(&mut self) -> Result<usize, u32> {
        if !self.scanning {
            return Err(FD_ERUN);
        }
        let mut b = 0u32;
        let mut k = 0usize;
        while b < 64 {
            if self.used & (1u64 << b) != 0 {
                if let Some(slot) = self.listed.get_mut(k) {
                    *slot = b;
                }
                k += 1;
            }
            b += 1;
        }
        self.listed_len = k;
        self.scanning = false;
        self.scanned = true;
        self.self_fd = 0;
        Ok(self.listed_len)
    }

    /// 执行新映像：清除带执行时关闭的描述符。
    pub fn exec(&mut self) -> Result<usize, u32> {
        if self.scanning {
            return Err(FD_EBUSY);
        }
        let mut killed = 0u32;
        let mut i = 0usize;
        while i < self.n {
            let drop = match self.slots.get(i) {
                Some(f) => f.cloexec(),
                None => false,
            };
            if drop {
                let num = match self.slots.get(i) {
                    Some(f) => f.num,
                    None => 0,
                };
                let mut j = i;
                while j + 1 < self.n {
                    match self.slots.get(j + 1).copied() {
                        Some(next) => {
                            if let Some(dst) = self.slots.get_mut(j) {
                                *dst = next;
                            }
                        }
                        None => break,
                    }
                    j += 1;
                }
                self.n -= 1;
                self.used &= !(1u64 << num);
                killed = killed.saturating_add(1);
            } else {
                i += 1;
            }
        }
        self.kills = killed;
        self.listed_len = 0;
        self.scanned = false;
        self.self_fd = 0;
        Ok(killed as usize)
    }

    /// 不变式自检。
    pub fn consistent(&self) -> bool {
        let mut bits = 0usize;
        let mut b = 0u32;
        while b < 64 {
            if self.used & (1u64 << b) != 0 {
                bits += 1;
            }
            b += 1;
        }
        if bits != self.n || self.n > CAP {
            return false;
        }
        let mut i = 0usize;
        while i < self.n {
            let f = match self.slots.get(i) {
                Some(v) => *v,
                None => return false,
            };
            if f.num >= 64 || self.used & (1u64 << f.num) == 0 {
                return false;
            }
            let mut j = i + 1;
            while j < self.n {
                match self.slots.get(j) {
                    Some(g) => {
                        if g.num == f.num {
                            return false;
                        }
                    }
                    None => return false,
                }
                j += 1;
            }
            i += 1;
        }
        let mut k = 0usize;
        while k < self.listed_len {
            let cur = match self.listed.get(k) {
                Some(v) => *v,
                None => return false,
            };
            if cur >= 64 {
                return false;
            }
            if k > 0 {
                match self.listed.get(k - 1) {
                    Some(prev) => {
                        if *prev >= cur {
                            return false;
                        }
                    }
                    None => return false,
                }
            }
            k += 1;
        }
        if self.scanning {
            if self.self_fd >= 64 || self.used & (1u64 << self.self_fd) == 0 {
                return false;
            }
            if self.find(self.self_fd).is_none() {
                return false;
            }
        }
        if self.scanned && self.scanning {
            return false;
        }
        true
    }

    /// 校验列表严格升序，返回真正的问题码。
    pub fn check_order(&self) -> Result<(), u32> {
        let mut k = 1usize;
        while k < self.listed_len {
            let a = match self.listed.get(k - 1) {
                Some(v) => *v,
                None => return Err(FD_EBAL),
            };
            let b = match self.listed.get(k) {
                Some(v) => *v,
                None => return Err(FD_EBAL),
            };
            if a >= b {
                return Err(FD_ESEQ);
            }
            k += 1;
        }
        Ok(())
    }
}

/// 一个账本样本。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Sample {
    /// 槽位。
    pub slot: u8,
    /// 操作码。
    pub op: u8,
    /// 是否成功。
    pub ok: bool,
    /// 错误码。
    pub code: u32,
    /// 观测值。
    pub value: u32,
}

impl Sample {
    /// 空白格。
    pub const fn blank() -> Sample {
        Sample { slot: 0, op: 0, ok: false, code: 0, value: 0 }
    }
}

/// `Result<usize, u32>` → 样本。
fn cell(slot: u8, op: u8, r: Result<usize, u32>) -> Sample {
    match r {
        Ok(v) => Sample { slot, op, ok: true, code: 0, value: v as u32 },
        Err(c) => Sample { slot, op, ok: false, code: c, value: 0 },
    }
}

/// `Result<u32, u32>` → 样本。
fn word_cell(slot: u8, op: u8, r: Result<u32, u32>) -> Sample {
    match r {
        Ok(v) => Sample { slot, op, ok: true, code: 0, value: v },
        Err(c) => Sample { slot, op, ok: false, code: c, value: 0 },
    }
}

/// 样本账本。
#[derive(Clone, Copy, Debug)]
pub struct Ledger {
    rows: [Sample; SAMPLES],
    len: usize,
    root: u64,
    frozen: bool,
    rejects: u32,
}

impl Ledger {
    /// 空账本。
    pub const fn new() -> Ledger {
        Ledger { rows: [Sample::blank(); SAMPLES], len: 0, root: 0, frozen: false, rejects: 0 }
    }

    /// 登记一格。
    pub fn register(&mut self, s: Sample) -> Result<(), u32> {
        if self.frozen || (s.slot as usize) != self.len || (s.slot as usize) >= SAMPLES {
            self.rejects += 1;
            return Err(FD_EORDER);
        }
        if let Some(dst) = self.rows.get_mut(self.len) {
            *dst = s;
        }
        self.len += 1;
        Ok(())
    }

    /// 格数。
    pub fn len(&self) -> usize {
        self.len
    }

    /// 是否空账。
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// 第 i 格。
    pub fn sample(&self, i: usize) -> Option<Sample> {
        if i >= self.len {
            return None;
        }
        self.rows.get(i).copied()
    }

    /// 成功格数。
    pub fn ok_count(&self) -> usize {
        let mut n = 0usize;
        let mut i = 0usize;
        while i < self.len {
            if let Some(s) = self.rows.get(i) {
                if s.ok {
                    n += 1;
                }
            }
            i += 1;
        }
        n
    }

    /// 失败格数。
    pub fn err_count(&self) -> usize {
        self.len - self.ok_count()
    }

    /// 对账。
    pub fn audit(&self) -> Result<(), u32> {
        if self.len != SAMPLES || self.ok_count() != 8 || self.err_count() != 6 {
            return Err(FD_EBAL);
        }
        const VALS: [u32; 8] = [0, 1, 2, 3, 4, 3, 11, 1];
        const CODES: [u32; 6] =
            [FD_EINVAL, FD_EBADF, FD_ENOENT, FD_ERUN, FD_EFULL, FD_EDIR];
        let mut vi = 0usize;
        let mut ci = 0usize;
        let mut i = 0usize;
        while i < self.len {
            let s = match self.rows.get(i) {
                Some(v) => *v,
                None => return Err(FD_EBAL),
            };
            if s.slot as usize != i {
                return Err(FD_EORDER);
            }
            if s.ok {
                if s.code != 0 {
                    return Err(FD_EMISMATCH);
                }
                match VALS.get(vi) {
                    Some(w) => {
                        if s.value != *w {
                            return Err(FD_EMISMATCH);
                        }
                    }
                    None => return Err(FD_EBAL),
                }
                vi += 1;
            } else {
                match CODES.get(ci) {
                    Some(w) => {
                        if s.code != *w {
                            return Err(FD_EMISMATCH);
                        }
                    }
                    None => return Err(FD_EBAL),
                }
                ci += 1;
            }
            i += 1;
        }
        if vi != 8 || ci != 6 {
            return Err(FD_EBAL);
        }
        Ok(())
    }

    /// 指纹。
    pub fn seal(&self) -> u64 {
        let mut h = FNV_OFF;
        let mut i = 0usize;
        while i < self.len {
            if let Some(s) = self.rows.get(i) {
                h = fold(h, s.slot as u64);
                h = fold(h, s.op as u64);
                h = fold(h, s.ok as u64);
                h = fold(h, s.code as u64);
                h = fold(h, s.value as u64);
            }
            i += 1;
        }
        fold(h, self.rejects as u64)
    }

    /// 冻结。
    pub fn freeze(&mut self) -> Result<u64, u32> {
        if self.frozen {
            self.rejects += 1;
            return Err(FD_EBAL);
        }
        self.root = self.seal();
        self.frozen = true;
        Ok(self.root)
    }

    /// 重放。
    pub fn replay(&self) -> Result<u64, u32> {
        let fresh = build_ledger();
        if fresh.seal() == self.seal() {
            Ok(fresh.seal())
        } else {
            Err(FD_ESEAL)
        }
    }

    /// 冻结指纹。
    pub fn root(&self) -> u64 {
        self.root
    }

    /// 是否冻结。
    pub fn is_frozen(&self) -> bool {
        self.frozen
    }

    /// 拒绝计数。
    pub fn rejects(&self) -> u32 {
        self.rejects
    }
}

/// 十四格账本：三个描述符 → 目录流自现 → 链接 → 执行清闭包 → 各类拒绝。
pub fn build_ledger() -> Ledger {
    let mut l = Ledger::new();
    let mut d = FdDir::new();

    let _ = l.register(cell(0, OP_OPEN, d.open(K_FILE, 7, F_READ)));
    let _ = l.register(cell(1, OP_OPEN, d.open(K_PIPE, 9, F_WRITE)));
    let _ = l.register(cell(2, OP_OPEN, d.open(K_FILE, 11, F_READ | F_CLOEXEC)));
    let _ = l.register(cell(3, OP_OPENDIR, d.opendir(0)));
    let _ = l.register(cell(4, OP_SCAN, d.scan()));
    let _ = l.register(word_cell(5, OP_SCAN, d.readdir_at(3)));
    let _ = l.register(word_cell(6, OP_LINK, d.readlink(2)));
    let _ = l.register(cell(7, OP_EXEC, d.exec()));

    let mut e = FdDir::new();
    let _ = l.register(cell(8, OP_OPEN, e.open(0, 1, 0)));

    let mut b = FdDir::new();
    let _ = l.register(cell(9, OP_CLOSE, b.close(9)));

    let n = FdDir::new();
    let _ = l.register(word_cell(10, OP_LINK, n.readlink(9)));

    let mut r = FdDir::new();
    let _ = l.register(cell(11, OP_SCAN, r.scan()));

    let mut f = FdDir::new();
    let mut i = 1u32;
    while (i as usize) <= CAP {
        let _ = f.open(K_FILE, i, F_READ);
        i += 1;
    }
    let _ = l.register(cell(12, OP_OPENDIR, f.opendir(0)));

    let mut g = FdDir::new();
    let _ = g.opendir(0);
    let _ = l.register(cell(13, OP_CLOSE, g.close(0)));

    l
}

/// 四路注入。
pub fn inject_four() -> [bool; 4] {
    let mut out = [false; 4];

    let l = build_ledger();
    let rep = l.replay();
    let root = l.seal();
    out[0] = l.len() == SAMPLES && l.audit().is_ok() && rep == Ok(root) && root != 0;

    // 一路：最小号复用 + 列表升序自现。
    let mut a = FdDir::new();
    let a0 = a.open(K_FILE, 21, F_READ);
    let a1 = a.open(K_PIPE, 22, F_READ);
    let ac = a.close(0);
    let a2 = a.open(K_FILE, 23, F_WRITE);
    let ad = a.opendir(0);
    let an = a.scan();
    out[1] = a0 == Ok(0)
        && a1 == Ok(1)
        && ac == Ok(0)
        && a2 == Ok(0)
        && ad == Ok(2)
        && an == Ok(3)
        && a.readdir_at(0) == Ok(0)
        && a.readdir_at(1) == Ok(1)
        && a.readdir_at(2) == Ok(2)
        && a.readdir_at(3) == Err(FD_ENOBS)
        && a.check_order().is_ok()
        && a.consistent();

    // 二路：消失即缺失 / 坏号 / 扫描中关目录 / 执行清闭包。
    let mut f = FdDir::new();
    let _ = f.open(K_FILE, 5, F_READ);
    let _ = f.open(K_FILE, 6, F_READ | F_CLOEXEC);
    let _ = f.close(0);
    let enoent = f.readlink(0) == Err(FD_ENOENT);
    let badf = f.close(0) == Err(FD_EBADF);
    let mut g = FdDir::new();
    let _ = g.opendir(0);
    let edir = g.close(0) == Err(FD_EDIR);
    let mut h = FdDir::new();
    let _ = h.open(K_FILE, 1, F_READ);
    let _ = h.open(K_FILE, 2, F_CLOEXEC);
    let killed = h.exec() == Ok(1);
    let left = h.is_open(0) && !h.is_open(1) && h.count() == 1 && h.consistent();
    out[2] = enoent && badf && edir && killed && left;

    // 三路：闸门次序。
    let mut q = FdDir::new();
    let q1 = q.open(0, 1, 0) == Err(FD_EINVAL);
    let q2 = q.open(K_FILE, 0, 0) == Err(FD_EINVAL);
    let q3 = q.open(K_FILE, 1, 0x80) == Err(FD_EINVAL);
    let q4 = q.scan() == Err(FD_ERUN);
    let q5 = q.open(K_FILE, 3, 0) == Ok(0);
    let q6 = q.opendir(0) == Ok(1);
    let q7 = q.exec() == Err(FD_EBUSY);
    let q8 = q.scan() == Ok(2);
    let q9 = q.scan() == Err(FD_ERUN);
    let q10 = q.opendir(0) == Ok(2);
    out[3] = q1 && q2 && q3 && q4 && q5 && q6 && q7 && q8 && q9 && q10 && q.consistent();

    out
}

/// 十三项判据。
pub fn run_unx11352_checks() -> CheckSet {
    let mut s = CheckSet::new("unx11352.procfs_fd_dir");
    let l = build_ledger();

    s.add_dyn(
        "FD1-十四格八允六拒",
        l.len() == SAMPLES && l.ok_count() == 8 && l.err_count() == 6 && l.audit().is_ok(),
        &format!("n={} ok={} err={}", l.len(), l.ok_count(), l.err_count()),
    );

    s.add(
        "FD2-最小号依次分配",
        l.sample(0).map(|x| x.value) == Some(0)
            && l.sample(1).map(|x| x.value) == Some(1)
            && l.sample(2).map(|x| x.value) == Some(2),
        "static",
    );

    s.add(
        "FD3-目录流自现于列表",
        l.sample(3).map(|x| x.value) == Some(3)
            && l.sample(4).map(|x| x.value) == Some(4)
            && l.sample(5).map(|x| x.value) == Some(3),
        "static",
    );

    s.add(
        "FD4-链接指向目标",
        l.sample(6).map(|x| x.value) == Some(11),
        "static",
    );

    s.add(
        "FD5-执行仅清闭包",
        l.sample(7).map(|x| x.value) == Some(1),
        "static",
    );

    s.add(
        "FD6-零类被拒",
        l.sample(8).map(|x| x.code) == Some(FD_EINVAL),
        "static",
    );

    s.add(
        "FD7-关坏号被拒",
        l.sample(9).map(|x| x.code) == Some(FD_EBADF),
        "static",
    );

    s.add(
        "FD8-缺失链接为ENOENT",
        l.sample(10).map(|x| x.code) == Some(FD_ENOENT),
        "static",
    );

    s.add(
        "FD9-重扫被拒",
        l.sample(11).map(|x| x.code) == Some(FD_ERUN),
        "static",
    );

    s.add(
        "FD10-满员拒开目录流",
        l.sample(12).map(|x| x.code) == Some(FD_EFULL),
        "static",
    );

    s.add(
        "FD11-扫描中关目录被拒",
        l.sample(13).map(|x| x.code) == Some(FD_EDIR),
        "static",
    );

    s.add(
        "FD12-复用与升序自洽",
        {
            let mut a = FdDir::new();
            let one = a.open(K_FILE, 31, F_READ);
            let two = a.open(K_FILE, 32, F_READ);
            let gone = a.close(0);
            let again = a.open(K_PIPE, 33, F_READ);
            let dir = a.opendir(0);
            let n = a.scan();
            one == Ok(0)
                && two == Ok(1)
                && gone == Ok(0)
                && again == Ok(0)
                && dir == Ok(2)
                && n == Ok(3)
                && a.readdir_at(0) == Ok(0)
                && a.readdir_at(1) == Ok(1)
                && a.readdir_at(2) == Ok(2)
                && a.readdir_at(3) == Err(FD_ENOBS)
                && a.check_order().is_ok()
                && a.consistent()
        },
        "static",
    );

    s.add(
        "FD13-冻结重放四注入",
        {
            let mut f = build_ledger();
            let rep = f.replay();
            let one = f.freeze();
            let two = f.freeze();
            let inj = inject_four();
            one.is_ok()
                && one == rep
                && two.is_err()
                && f.is_frozen()
                && f.root() != 0
                && f.rejects() == 1
                && inj == [true; 4]
        },
        "static",
    );

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ledger_shape() {
        let l = build_ledger();
        assert_eq!(l.len(), SAMPLES);
        assert_eq!(l.ok_count(), 8);
        assert_eq!(l.err_count(), 6);
        assert!(l.audit().is_ok());
    }

    #[test]
    fn checks_pass() {
        let s = run_unx11352_checks();
        assert_eq!(s.len(), 13);
        assert!(s.all_passed());
    }

    #[test]
    fn lowest_free_reuse() {
        let mut a = FdDir::new();
        assert_eq!(a.open(K_FILE, 1, F_READ), Ok(0));
        assert_eq!(a.open(K_FILE, 2, F_READ), Ok(1));
        assert_eq!(a.open(K_FILE, 3, F_READ), Ok(2));
        assert_eq!(a.close(1), Ok(1));
        assert_eq!(a.open(K_PIPE, 4, F_READ), Ok(1));
        assert_eq!(a.close(0), Ok(0));
        assert_eq!(a.open(K_PIPE, 5, F_WRITE), Ok(0));
        assert!(a.consistent());
    }

    #[test]
    fn scan_includes_self_sorted() {
        let mut a = FdDir::new();
        assert_eq!(a.open(K_FILE, 10, F_READ), Ok(0));
        assert_eq!(a.open(K_FILE, 11, F_READ), Ok(1));
        assert_eq!(a.opendir(0), Ok(2));
        assert!(a.scanning());
        assert_eq!(a.scan(), Ok(3));
        assert_eq!(a.readdir_at(0), Ok(0));
        assert_eq!(a.readdir_at(1), Ok(1));
        assert_eq!(a.readdir_at(2), Ok(2));
        assert_eq!(a.readdir_at(3), Err(FD_ENOBS));
        assert!(a.scanned());
        assert!(!a.scanning());
        assert!(a.check_order().is_ok());
    }

    #[test]
    fn missing_link_is_enoent() {
        let mut a = FdDir::new();
        assert_eq!(a.readlink(7), Err(FD_ENOENT));
        assert_eq!(a.open(K_FILE, 77, F_READ), Ok(0));
        assert_eq!(a.readlink(0), Ok(77));
        assert_eq!(a.close(0), Ok(0));
        assert_eq!(a.readlink(0), Err(FD_ENOENT));
        assert_eq!(a.close(0), Err(FD_EBADF));
        assert_eq!(a.close(64), Err(FD_EBADF));
    }

    #[test]
    fn exec_drops_cloexec_only() {
        let mut a = FdDir::new();
        assert_eq!(a.open(K_FILE, 1, F_READ), Ok(0));
        assert_eq!(a.open(K_FILE, 2, F_READ | F_CLOEXEC), Ok(1));
        assert_eq!(a.open(K_PIPE, 3, F_WRITE | F_CLOEXEC), Ok(2));
        assert_eq!(a.exec(), Ok(2));
        assert_eq!(a.count(), 1);
        assert!(a.is_open(0));
        assert!(!a.is_open(1));
        assert!(!a.is_open(2));
        assert_eq!(a.kills(), 2);
        assert!(a.consistent());
    }

    #[test]
    fn capacity_is_exact() {
        let mut a = FdDir::new();
        let mut i = 0u32;
        while (i as usize) < CAP {
            assert_eq!(a.open(K_FILE, i + 1, F_READ), Ok(i as usize));
            i += 1;
        }
        assert_eq!(a.open(K_FILE, 99, F_READ), Err(FD_EFULL));
        assert_eq!(a.opendir(0), Err(FD_EFULL));
        assert!(a.consistent());
    }

    #[test]
    fn edir_and_busy_while_scanning() {
        let mut a = FdDir::new();
        assert_eq!(a.open(K_FILE, 1, F_READ), Ok(0));
        assert_eq!(a.opendir(0), Ok(1));
        assert_eq!(a.close(1), Err(FD_EDIR));
        assert_eq!(a.exec(), Err(FD_EBUSY));
        assert_eq!(a.scan(), Ok(2));
        assert_eq!(a.exec(), Ok(0));
        assert_eq!(a.close(1), Ok(1));
        assert!(a.consistent());
    }
}
