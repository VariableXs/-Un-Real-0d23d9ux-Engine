//! UNX-F11392 rm 单件删除与输出语义（0xee80 段）
//!
//! `rm` 最容易被误读的一点是「它什么都不说」。删除成功时工具**必须彻底静默**：
//! 不打印文件名、不打印计数、不打印换行——脚本靠「无声即成功」判断，任何多余
//! 输出都会污染下游管道。删除失败时则相反：必须往 stderr 写一行可读的诊断，
//! 并让退出码非零。本模块把这两条输出语义做成可观测的字节账（`out_bytes`）与
//! 错误码账（`last_code`），成功路径一个字节都不写。
//!
//! 真正复杂的是「目录项消失」与「链接计数下降」这两件事的先后与一致性。Linux
//! 的 `unlink` 做的是：
//!
//! 1. 摘掉目录项（此后按名字再也找不到它）；
//! 2. inode 的 `nlink` 减一；
//! 3. 只有当 `nlink == 0` **且** 没有进程还持有打开的描述符时，inode 才被回收。
//!
//! 第 3 条制造出「已删除但仍占空间」的中间态：文件从目录里消失了，可写它的进程
//! 还在写。本模块用 `deferred` 精确记录这个中间态，并要求 `unlink` 在递延场景
//! 返回 0（未回收），在 `close` 把最后一个描述符放掉时返回 1（此刻才回收）。
//!
//! 硬链接让 `nlink` 有意义：`link` 增加一个目录项并把计数加一，`unlink` 只是把
//! 计数减一，只有当计数归零才谈得上回收。不变式因此可以写成一条硬等式：
//! **每个 inode 的 `nlink` 等于引用它的活目录项条数**（已 unlink 但仍有打开的
//! 情形除外，此时 nlink 为 0 而 inode 仍活）。
//!
//! 错误类为何彼此独立：
//!
//! - `EINVAL` 是调用形状错（空名、零号、非法/已关句柄），与文件系统内容无关；
//! - `ENOENT` 是名字此刻不存在，硬链接计数与目录项都无需变动；
//! - `EISDIR` 是名字存在但类型是目录，`rm` 不带 `-r` 时必须拒绝而不是「尽力删」；
//! - `EACCES` 是父目录不可写，权限模型拒绝，和文件本身权限无关；
//! - `EPERM` 是 sticky 目录里既非目录属主也非文件属主的删除，属于策略拒绝；
//! - `EBUSY` 是目录项是挂载点，删掉它等于把整棵子树抽走，内核必须拒绝；
//! - `EROFS` 是文件系统只读，任何删除都无从谈起，优先于「条目是否存在」；
//! - `EFULL` 是表满（条目表/inode 表/描述符表），删一条或关一个即可继续。
//!
//! 闸门次序（`unlink`）：空名/零调用者 → 目录存在 → 只读 → 条目存在 → 挂载点 →
//! 类型 → sticky 属主 → 父目录写权限 → 摘项。次序固定，因此同一个坏名字在只读
//! 文件系统上得到 `EROFS`，在可写文件系统上得到 `ENOENT`，两者不会互相掩盖。

use crate::alloc::format;
use crate::checks::CheckSet;

/// 段基址。
pub const SEG_BASE: u32 = 0xee80;

/// 调用形状错（空名、零号、非法句柄）。
pub const RM_EINVAL: u32 = SEG_BASE + 0x01;
/// 名字不存在。
pub const RM_ENOENT: u32 = SEG_BASE + 0x02;
/// 名字存在但类型是目录。
pub const RM_EISDIR: u32 = SEG_BASE + 0x03;
/// sticky 目录属主策略拒绝。
pub const RM_EPERM: u32 = SEG_BASE + 0x04;
/// 父目录不可写。
pub const RM_EACCES: u32 = SEG_BASE + 0x05;
/// 目录项是挂载点。
pub const RM_EBUSY: u32 = SEG_BASE + 0x06;
/// 文件系统只读。
pub const RM_EROFS: u32 = SEG_BASE + 0x07;
/// 表满（条目/inode/描述符）。
pub const RM_EFULL: u32 = SEG_BASE + 0x08;
/// 账本形状不符。
pub const RM_EBAL: u32 = SEG_BASE + 0x09;
/// 槽位乱序。
pub const RM_ESLOT: u32 = SEG_BASE + 0x0a;
/// 观测值与复算不符。
pub const RM_EVALUE: u32 = SEG_BASE + 0x0b;
/// 冻结后重放不一致。
pub const RM_ESEAL: u32 = SEG_BASE + 0x0c;
/// 观察下标越界。
pub const RM_ENOBS: u32 = SEG_BASE + 0x0d;

/// 目录表容量。
pub const DCAP: usize = 4;
/// inode 表容量。
pub const ICAP: usize = 8;
/// 目录项表容量（即 `CAP`）。
pub const CAP: usize = 10;
/// 描述符表容量。
pub const FCAP: usize = 6;
/// 账本格数。
pub const SAMPLES: usize = 14;
/// 一条诊断信息的字节数。
pub const MSG_LEN: u32 = 24;

/// 操作码：建目录。
pub const OP_MKDIR: u8 = 1;
/// 操作码：建文件。
pub const OP_MKFILE: u8 = 2;
/// 操作码：硬链接。
pub const OP_LINK: u8 = 3;
/// 操作码：打开。
pub const OP_OPEN: u8 = 4;
/// 操作码：删除。
pub const OP_UNLINK: u8 = 5;
/// 操作码：关闭。
pub const OP_CLOSE: u8 = 6;
/// 操作码：读活条目数。
pub const OP_STAT: u8 = 7;

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

/// 一个目录。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Dir {
    /// 目录号。
    pub id: u32,
    /// 属主用户。
    pub owner: u32,
    /// 属主是否可写。
    pub writable: bool,
    /// 是否 sticky。
    pub sticky: bool,
    /// 是否只读挂载。
    pub ro: bool,
}

impl Dir {
    /// 空目录槽。
    pub const fn blank() -> Dir {
        Dir { id: 0, owner: 0, writable: false, sticky: false, ro: false }
    }
}

/// 一个 inode。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Inode {
    /// inode 号。
    pub no: u32,
    /// 链接计数。
    pub nlink: u32,
    /// 打开计数。
    pub opens: u32,
    /// 属主。
    pub uid: u32,
    /// 是否目录。
    pub dir: bool,
    /// 是否存活。
    pub live: bool,
}

impl Inode {
    /// 空 inode 槽。
    pub const fn blank() -> Inode {
        Inode { no: 0, nlink: 0, opens: 0, uid: 0, dir: false, live: false }
    }
}

/// 一个目录项。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DEnt {
    /// 所在目录。
    pub dir: u32,
    /// 名字。
    pub name: u32,
    /// 指向的 inode 号。
    pub ino: u32,
    /// 是否挂载点。
    pub mount: bool,
    /// 是否存活。
    pub live: bool,
}

impl DEnt {
    /// 空目录项槽。
    pub const fn blank() -> DEnt {
        DEnt { dir: 0, name: 0, ino: 0, mount: false, live: false }
    }
}

/// 一个描述符。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Fd {
    /// 指向的 inode 号。
    pub ino: u32,
    /// 是否打开。
    pub live: bool,
}

impl Fd {
    /// 空描述符槽。
    pub const fn blank() -> Fd {
        Fd { ino: 0, live: false }
    }
}

/// 单件删除模型。
#[derive(Clone, Copy, Debug)]
pub struct Rm {
    d: [Dir; DCAP],
    dn: usize,
    i: [Inode; ICAP],
    e: [DEnt; CAP],
    f: [Fd; FCAP],
    next_ino: u32,
    out_bytes: u32,
    last_code: u32,
    unlinks: u32,
    deferred: u32,
}

impl Rm {
    /// 空模型。
    pub const fn new() -> Rm {
        Rm {
            d: [Dir::blank(); DCAP],
            dn: 0,
            i: [Inode::blank(); ICAP],
            e: [DEnt::blank(); CAP],
            f: [Fd::blank(); FCAP],
            next_ino: 1,
            out_bytes: 0,
            last_code: 0,
            unlinks: 0,
            deferred: 0,
        }
    }

    /// 目录条数。
    pub fn dir_count(&self) -> usize {
        self.dn
    }

    /// 活 inode 条数（含 nlink 已零但仍有打开的）。
    pub fn ino_live(&self) -> usize {
        let mut n = 0usize;
        let mut k = 0usize;
        while k < ICAP {
            if let Some(x) = self.i.get(k) {
                if x.live {
                    n += 1;
                }
            }
            k += 1;
        }
        n
    }

    /// 活目录项条数。
    pub fn ent_live(&self) -> usize {
        let mut n = 0usize;
        let mut k = 0usize;
        while k < CAP {
            if let Some(x) = self.e.get(k) {
                if x.live {
                    n += 1;
                }
            }
            k += 1;
        }
        n
    }

    /// 打开的描述符条数。
    pub fn open_fds(&self) -> usize {
        let mut n = 0usize;
        let mut k = 0usize;
        while k < FCAP {
            if let Some(x) = self.f.get(k) {
                if x.live {
                    n += 1;
                }
            }
            k += 1;
        }
        n
    }

    /// 累计诊断输出字节。
    pub fn out_bytes(&self) -> u32 {
        self.out_bytes
    }

    /// 最近一次错误码（成功不改写）。
    pub fn last_code(&self) -> u32 {
        self.last_code
    }

    /// 成功删除次数。
    pub fn unlink_count(&self) -> u32 {
        self.unlinks
    }

    /// 递延回收次数。
    pub fn defer_count(&self) -> u32 {
        self.deferred
    }

    /// 第 k 个目录。
    pub fn dir_at(&self, k: usize) -> Option<Dir> {
        if k >= self.dn {
            return None;
        }
        self.d.get(k).copied()
    }

    /// 第 k 个 inode。
    pub fn ino_at(&self, k: usize) -> Option<Inode> {
        self.i.get(k).copied()
    }

    /// 第 k 个目录项。
    pub fn ent_at(&self, k: usize) -> Option<DEnt> {
        self.e.get(k).copied()
    }

    /// 第 k 个描述符。
    pub fn fd_at(&self, k: usize) -> Option<Fd> {
        self.f.get(k).copied()
    }

    /// 找目录下标。
    fn find_dir(&self, id: u32) -> Option<usize> {
        let mut k = 0usize;
        while k < self.dn {
            match self.d.get(k) {
                Some(x) => {
                    if x.id == id {
                        return Some(k);
                    }
                }
                None => return None,
            }
            k += 1;
        }
        None
    }

    /// 按 inode 号找下标。
    fn find_ino(&self, no: u32) -> Option<usize> {
        let mut k = 0usize;
        while k < ICAP {
            match self.i.get(k) {
                Some(x) => {
                    if x.live && x.no == no {
                        return Some(k);
                    }
                }
                None => return None,
            }
            k += 1;
        }
        None
    }

    /// 找活目录项下标。
    fn find_ent(&self, dir: u32, name: u32) -> Option<usize> {
        let mut k = 0usize;
        while k < CAP {
            match self.e.get(k) {
                Some(x) => {
                    if x.live && x.dir == dir && x.name == name {
                        return Some(k);
                    }
                }
                None => return None,
            }
            k += 1;
        }
        None
    }

    /// 找一个空目录项槽。
    fn free_ent(&self) -> Option<usize> {
        let mut k = 0usize;
        while k < CAP {
            match self.e.get(k) {
                Some(x) => {
                    if !x.live {
                        return Some(k);
                    }
                }
                None => return None,
            }
            k += 1;
        }
        None
    }

    /// 找一个空 inode 槽。
    fn free_ino(&self) -> Option<usize> {
        let mut k = 0usize;
        while k < ICAP {
            match self.i.get(k) {
                Some(x) => {
                    if !x.live {
                        return Some(k);
                    }
                }
                None => return None,
            }
            k += 1;
        }
        None
    }

    /// 失败：记一次诊断输出并返回错误。
    fn fail(&mut self, code: u32) -> Result<usize, u32> {
        self.out_bytes = self.out_bytes.saturating_add(MSG_LEN);
        self.last_code = code;
        Err(code)
    }

    /// 建目录。
    pub fn mkdir(&mut self, id: u32, owner: u32, writable: bool, sticky: bool) -> Result<usize, u32> {
        if id == 0 || owner == 0 {
            return self.fail(RM_EINVAL);
        }
        if self.find_dir(id).is_some() {
            return self.fail(RM_EINVAL);
        }
        if self.dn >= DCAP {
            return self.fail(RM_EFULL);
        }
        if let Some(slot) = self.d.get_mut(self.dn) {
            *slot = Dir { id, owner, writable, sticky, ro: false };
        }
        self.dn += 1;
        Ok(self.dn)
    }

    /// 调整目录的写权限位（模拟 chmod 改父目录）。
    pub fn set_write(&mut self, id: u32, allowed: bool) -> Result<usize, u32> {
        match self.find_dir(id) {
            Some(k) => {
                if let Some(d) = self.d.get_mut(k) {
                    d.writable = allowed;
                }
                Ok(k + 1)
            }
            None => self.fail(RM_ENOENT),
        }
    }

    /// 把目录标记为只读挂载。
    pub fn set_ro(&mut self, id: u32) -> Result<usize, u32> {
        match self.find_dir(id) {
            Some(k) => {
                if let Some(d) = self.d.get_mut(k) {
                    d.ro = true;
                }
                Ok(k + 1)
            }
            None => self.fail(RM_ENOENT),
        }
    }

    /// 建文件，返回活条目数。
    pub fn mkfile(&mut self, dir: u32, name: u32, uid: u32) -> Result<usize, u32> {
        if dir == 0 || name == 0 || uid == 0 {
            return self.fail(RM_EINVAL);
        }
        let di = match self.find_dir(dir) {
            Some(k) => k,
            None => return self.fail(RM_ENOENT),
        };
        let d = match self.d.get(di) {
            Some(x) => *x,
            None => return self.fail(RM_ENOENT),
        };
        if d.ro {
            return self.fail(RM_EROFS);
        }
        if !d.writable {
            return self.fail(RM_EACCES);
        }
        if self.find_ent(dir, name).is_some() {
            return self.fail(RM_EINVAL);
        }
        let es = match self.free_ent() {
            Some(k) => k,
            None => return self.fail(RM_EFULL),
        };
        let is = match self.free_ino() {
            Some(k) => k,
            None => return self.fail(RM_EFULL),
        };
        let no = self.next_ino;
        self.next_ino = self.next_ino.saturating_add(1);
        if let Some(slot) = self.i.get_mut(is) {
            *slot = Inode { no, nlink: 1, opens: 0, uid, dir: false, live: true };
        }
        if let Some(slot) = self.e.get_mut(es) {
            *slot = DEnt { dir, name, ino: no, mount: false, live: true };
        }
        Ok(self.ent_live())
    }

    /// 在父目录下建一个子目录项（inode 标记为目录）。
    pub fn subdir(&mut self, dir: u32, name: u32, uid: u32) -> Result<usize, u32> {
        if dir == 0 || name == 0 || uid == 0 {
            return self.fail(RM_EINVAL);
        }
        let di = match self.find_dir(dir) {
            Some(k) => k,
            None => return self.fail(RM_ENOENT),
        };
        let d = match self.d.get(di) {
            Some(x) => *x,
            None => return self.fail(RM_ENOENT),
        };
        if d.ro {
            return self.fail(RM_EROFS);
        }
        if !d.writable {
            return self.fail(RM_EACCES);
        }
        if self.find_ent(dir, name).is_some() {
            return self.fail(RM_EINVAL);
        }
        let es = match self.free_ent() {
            Some(k) => k,
            None => return self.fail(RM_EFULL),
        };
        let is = match self.free_ino() {
            Some(k) => k,
            None => return self.fail(RM_EFULL),
        };
        let no = self.next_ino;
        self.next_ino = self.next_ino.saturating_add(1);
        if let Some(slot) = self.i.get_mut(is) {
            *slot = Inode { no, nlink: 1, opens: 0, uid, dir: true, live: true };
        }
        if let Some(slot) = self.e.get_mut(es) {
            *slot = DEnt { dir, name, ino: no, mount: false, live: true };
        }
        Ok(self.ent_live())
    }

    /// 把某个目录项标记为挂载点。
    pub fn mark_mount(&mut self, dir: u32, name: u32) -> Result<usize, u32> {
        match self.find_ent(dir, name) {
            Some(k) => {
                if let Some(x) = self.e.get_mut(k) {
                    x.mount = true;
                }
                Ok(k + 1)
            }
            None => self.fail(RM_ENOENT),
        }
    }

    /// 硬链接：在 `dst_dir/dst_name` 再挂一个名字指向同一 inode。
    pub fn link(&mut self, src_dir: u32, src_name: u32, dst_dir: u32, dst_name: u32) -> Result<usize, u32> {
        if src_dir == 0 || src_name == 0 || dst_dir == 0 || dst_name == 0 {
            return self.fail(RM_EINVAL);
        }
        let si = match self.find_ent(src_dir, src_name) {
            Some(k) => k,
            None => return self.fail(RM_ENOENT),
        };
        let se = match self.e.get(si) {
            Some(x) => *x,
            None => return self.fail(RM_ENOENT),
        };
        let ii = match self.find_ino(se.ino) {
            Some(k) => k,
            None => return self.fail(RM_ENOENT),
        };
        let ino = match self.i.get(ii) {
            Some(x) => *x,
            None => return self.fail(RM_ENOENT),
        };
        if ino.dir {
            return self.fail(RM_EISDIR);
        }
        let di = match self.find_dir(dst_dir) {
            Some(k) => k,
            None => return self.fail(RM_ENOENT),
        };
        let d = match self.d.get(di) {
            Some(x) => *x,
            None => return self.fail(RM_ENOENT),
        };
        if d.ro {
            return self.fail(RM_EROFS);
        }
        if !d.writable {
            return self.fail(RM_EACCES);
        }
        if self.find_ent(dst_dir, dst_name).is_some() {
            return self.fail(RM_EINVAL);
        }
        let es = match self.free_ent() {
            Some(k) => k,
            None => return self.fail(RM_EFULL),
        };
        if let Some(slot) = self.e.get_mut(es) {
            *slot = DEnt { dir: dst_dir, name: dst_name, ino: se.ino, mount: false, live: true };
        }
        if let Some(x) = self.i.get_mut(ii) {
            x.nlink = x.nlink.saturating_add(1);
        }
        Ok(self.ent_live())
    }

    /// 打开一个名字，返回描述符号（1 基）。
    pub fn open(&mut self, dir: u32, name: u32) -> Result<usize, u32> {
        if dir == 0 || name == 0 {
            return self.fail(RM_EINVAL);
        }
        let ei = match self.find_ent(dir, name) {
            Some(k) => k,
            None => return self.fail(RM_ENOENT),
        };
        let e = match self.e.get(ei) {
            Some(x) => *x,
            None => return self.fail(RM_ENOENT),
        };
        let ii = match self.find_ino(e.ino) {
            Some(k) => k,
            None => return self.fail(RM_ENOENT),
        };
        let mut fs = FCAP;
        let mut k = 0usize;
        while k < FCAP {
            match self.f.get(k) {
                Some(x) => {
                    if !x.live {
                        fs = k;
                        break;
                    }
                }
                None => break,
            }
            k += 1;
        }
        if fs >= FCAP {
            return self.fail(RM_EFULL);
        }
        if let Some(slot) = self.f.get_mut(fs) {
            *slot = Fd { ino: e.ino, live: true };
        }
        if let Some(x) = self.i.get_mut(ii) {
            x.opens = x.opens.saturating_add(1);
        }
        Ok(fs + 1)
    }

    /// 关闭描述符，返回是否在这一次回收了 inode。
    pub fn close(&mut self, fd: usize) -> Result<usize, u32> {
        if fd == 0 || fd > FCAP {
            return self.fail(RM_EINVAL);
        }
        let k = fd - 1;
        let f = match self.f.get(k) {
            Some(x) => *x,
            None => return self.fail(RM_EINVAL),
        };
        if !f.live {
            return self.fail(RM_EINVAL);
        }
        if let Some(slot) = self.f.get_mut(k) {
            slot.live = false;
        }
        let ii = match self.find_ino(f.ino) {
            Some(v) => v,
            None => return self.fail(RM_ENOENT),
        };
        let mut freed = 0usize;
        if let Some(x) = self.i.get_mut(ii) {
            x.opens = x.opens.saturating_sub(1);
            if x.nlink == 0 && x.opens == 0 {
                x.live = false;
                freed = 1;
            }
        }
        if freed == 1 {
            self.defer_count_back();
        }
        Ok(freed)
    }

    /// 递延计数在真正回收时归零式的「减一」（仅用于账目自洽）。
    fn defer_count_back(&mut self) {
        self.deferred = self.deferred.saturating_sub(1);
    }

    /// 删除一个名字。返回 1 表示 inode 本次被回收，0 表示递延。
    pub fn unlink(&mut self, dir: u32, name: u32, caller: u32) -> Result<usize, u32> {
        if dir == 0 || name == 0 || caller == 0 {
            return self.fail(RM_EINVAL);
        }
        let di = match self.find_dir(dir) {
            Some(k) => k,
            None => return self.fail(RM_ENOENT),
        };
        let d = match self.d.get(di) {
            Some(x) => *x,
            None => return self.fail(RM_ENOENT),
        };
        if d.ro {
            return self.fail(RM_EROFS);
        }
        let ei = match self.find_ent(dir, name) {
            Some(k) => k,
            None => return self.fail(RM_ENOENT),
        };
        let e = match self.e.get(ei) {
            Some(x) => *x,
            None => return self.fail(RM_ENOENT),
        };
        if e.mount {
            return self.fail(RM_EBUSY);
        }
        let ii = match self.find_ino(e.ino) {
            Some(k) => k,
            None => return self.fail(RM_ENOENT),
        };
        let ino = match self.i.get(ii) {
            Some(x) => *x,
            None => return self.fail(RM_ENOENT),
        };
        if ino.dir {
            return self.fail(RM_EISDIR);
        }
        if d.sticky && caller != d.owner && caller != ino.uid {
            return self.fail(RM_EPERM);
        }
        if !d.writable {
            return self.fail(RM_EACCES);
        }

        if let Some(slot) = self.e.get_mut(ei) {
            slot.live = false;
        }
        self.unlinks = self.unlinks.saturating_add(1);
        let mut freed = 0usize;
        if let Some(x) = self.i.get_mut(ii) {
            x.nlink = x.nlink.saturating_sub(1);
            if x.nlink == 0 {
                if x.opens == 0 {
                    x.live = false;
                    freed = 1;
                } else {
                    self.deferred = self.deferred.saturating_add(1);
                }
            }
        }
        Ok(freed)
    }

    /// 名字当前是否可见。
    pub fn visible(&self, dir: u32, name: u32) -> bool {
        self.find_ent(dir, name).is_some()
    }

    /// inode 链接计数。
    pub fn nlink_of(&self, no: u32) -> Option<u32> {
        match self.find_ino(no) {
            Some(k) => match self.i.get(k) {
                Some(x) => Some(x.nlink),
                None => None,
            },
            None => None,
        }
    }

    /// 不变式自检：链接计数等于活目录项条数（nlink 为零的递延 inode 除外）。
    pub fn consistent(&self) -> bool {
        if self.dn > DCAP {
            return false;
        }
        let mut k = 0usize;
        while k < self.dn {
            let d = match self.d.get(k) {
                Some(x) => *x,
                None => return false,
            };
            if d.id == 0 || d.owner == 0 {
                return false;
            }
            let mut j = k + 1;
            while j < self.dn {
                match self.d.get(j) {
                    Some(o) => {
                        if o.id == d.id {
                            return false;
                        }
                    }
                    None => return false,
                }
                j += 1;
            }
            k += 1;
        }
        let mut e = 0usize;
        while e < CAP {
            let ent = match self.e.get(e) {
                Some(x) => *x,
                None => return false,
            };
            if ent.live {
                if ent.name == 0 || ent.dir == 0 || self.find_dir(ent.dir).is_none() {
                    return false;
                }
                if self.find_ino(ent.ino).is_none() {
                    return false;
                }
            }
            e += 1;
        }
        let mut i = 0usize;
        while i < ICAP {
            let x = match self.i.get(i) {
                Some(v) => *v,
                None => return false,
            };
            if x.live {
                let mut refs = 0u32;
                let mut s = 0usize;
                while s < CAP {
                    if let Some(ent) = self.e.get(s) {
                        if ent.live && ent.ino == x.no {
                            refs = refs.saturating_add(1);
                        }
                    }
                    s += 1;
                }
                if x.nlink != refs {
                    return false;
                }
                if x.nlink == 0 && x.opens == 0 {
                    return false;
                }
                let mut t = i + 1;
                while t < ICAP {
                    match self.i.get(t) {
                        Some(o) => {
                            if o.live && o.no == x.no {
                                return false;
                            }
                        }
                        None => return false,
                    }
                    t += 1;
                }
            }
            i += 1;
        }
        let mut f = 0usize;
        while f < FCAP {
            let fd = match self.f.get(f) {
                Some(v) => *v,
                None => return false,
            };
            if fd.live && fd.ino == 0 {
                return false;
            }
            f += 1;
        }
        true
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
            self.rejects = self.rejects.saturating_add(1);
            return Err(RM_ESLOT);
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
            return Err(RM_EBAL);
        }
        const VALS: [u32; 8] = [1, 1, 2, 0, 1, 0, 1, 0];
        const CODES: [u32; 6] = [RM_EINVAL, RM_ENOENT, RM_EISDIR, RM_EROFS, RM_EBUSY, RM_EPERM];
        let mut vi = 0usize;
        let mut ci = 0usize;
        let mut i = 0usize;
        while i < self.len {
            let s = match self.rows.get(i) {
                Some(v) => *v,
                None => return Err(RM_EBAL),
            };
            if s.slot as usize != i {
                return Err(RM_ESLOT);
            }
            if s.ok {
                if s.code != 0 {
                    return Err(RM_EVALUE);
                }
                match VALS.get(vi) {
                    Some(w) => {
                        if s.value != *w {
                            return Err(RM_EVALUE);
                        }
                    }
                    None => return Err(RM_EBAL),
                }
                vi += 1;
            } else {
                match CODES.get(ci) {
                    Some(w) => {
                        if s.code != *w {
                            return Err(RM_EVALUE);
                        }
                    }
                    None => return Err(RM_EBAL),
                }
                ci += 1;
            }
            i += 1;
        }
        if vi != 8 || ci != 6 {
            return Err(RM_EBAL);
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
            self.rejects = self.rejects.saturating_add(1);
            return Err(RM_EBAL);
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
            Err(RM_ESEAL)
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

/// 十四格账本：建目录文件 → 硬链接 → 打开 → 递延删除 → 关闭回收 → 六类拒绝。
pub fn build_ledger() -> Ledger {
    let mut l = Ledger::new();
    let mut m = Rm::new();

    let _ = l.register(cell(0, OP_MKDIR, m.mkdir(1, 100, true, false)));
    let _ = l.register(cell(1, OP_MKFILE, m.mkfile(1, 10, 100)));
    let _ = l.register(cell(2, OP_LINK, m.link(1, 10, 1, 11)));
    let _ = l.register(cell(3, OP_UNLINK, m.unlink(1, 10, 100)));
    let _ = l.register(cell(4, OP_OPEN, m.open(1, 11)));
    let _ = l.register(cell(5, OP_UNLINK, m.unlink(1, 11, 100)));
    let _ = l.register(cell(6, OP_CLOSE, m.close(1)));
    let _ = l.register(cell(7, OP_STAT, Ok(m.ent_live())));

    let mut a = Rm::new();
    let _ = l.register(cell(8, OP_MKDIR, a.mkdir(0, 100, true, false)));

    let mut b = Rm::new();
    let _ = b.mkdir(1, 100, true, false);
    let _ = l.register(cell(9, OP_UNLINK, b.unlink(1, 99, 100)));

    let mut c = Rm::new();
    let _ = c.mkdir(1, 100, true, false);
    let _ = c.subdir(1, 1, 100);
    let _ = l.register(cell(10, OP_UNLINK, c.unlink(1, 1, 100)));

    let mut d = Rm::new();
    let _ = d.mkdir(1, 100, true, false);
    let _ = d.mkfile(1, 20, 100);
    let _ = d.set_ro(1);
    let _ = l.register(cell(11, OP_UNLINK, d.unlink(1, 20, 100)));

    let mut e = Rm::new();
    let _ = e.mkdir(1, 100, true, false);
    let _ = e.mkfile(1, 30, 100);
    let _ = e.mark_mount(1, 30);
    let _ = l.register(cell(12, OP_UNLINK, e.unlink(1, 30, 100)));

    let mut f = Rm::new();
    let _ = f.mkdir(1, 100, true, true);
    let _ = f.mkfile(1, 40, 200);
    let _ = l.register(cell(13, OP_UNLINK, f.unlink(1, 40, 300)));

    l
}

/// 四路注入。
pub fn inject_four() -> [bool; 4] {
    let mut out = [false; 4];

    let l = build_ledger();
    let rep = l.replay();
    let root = l.seal();
    out[0] = l.len() == SAMPLES && l.audit().is_ok() && rep == Ok(root) && root != 0;

    // 一路：成功静默、链接计数与目录项同步、递延回收。
    let mut a = Rm::new();
    let _ = a.mkdir(1, 100, true, false);
    let _ = a.mkfile(1, 10, 100);
    let _ = a.link(1, 10, 1, 11);
    let d0 = a.unlink(1, 10, 100);
    let _ = a.open(1, 11);
    let d1 = a.unlink(1, 11, 100);
    let vis1 = a.visible(1, 11);
    let d2 = a.close(1);
    out[1] = d0 == Ok(0)
        && d1 == Ok(0)
        && !vis1
        && d2 == Ok(1)
        && a.ent_live() == 0
        && a.ino_live() == 0
        && a.open_fds() == 0
        && a.out_bytes() == 0
        && a.defer_count() == 0
        && a.consistent();

    // 二路：失败一律出声，且状态不变。
    let mut b = Rm::new();
    let _ = b.mkdir(1, 100, true, false);
    let _ = b.mkdir(2, 100, true, true);
    let _ = b.mkdir(3, 100, true, false);
    let _ = b.mkfile(1, 20, 100);
    let _ = b.mkfile(2, 21, 200);
    let _ = b.mkfile(3, 22, 100);
    let _ = b.set_write(3, false);
    let _ = b.mkfile(1, 23, 100);
    let _ = b.subdir(1, 1, 100);
    let _ = b.mark_mount(1, 23);
    let before = b.ent_live();
    let e1 = b.unlink(1, 99, 100) == Err(RM_ENOENT);
    let e2 = b.unlink(1, 1, 100) == Err(RM_EISDIR);
    let e3 = b.unlink(2, 21, 300) == Err(RM_EPERM);
    let e4 = b.unlink(3, 22, 100) == Err(RM_EACCES);
    let e5 = b.unlink(1, 23, 100) == Err(RM_EBUSY);
    let e6 = b.close(1) == Err(RM_EINVAL);
    let e7 = b.close(99) == Err(RM_EINVAL);
    let bytes = b.out_bytes();
    let _ = b.set_ro(1);
    let e8 = b.unlink(1, 20, 100) == Err(RM_EROFS);
    out[2] = e1
        && e2
        && e3
        && e4
        && e5
        && e6
        && e7
        && e8
        && b.ent_live() == before
        && b.unlink_count() == 0
        && b.out_bytes() == bytes.saturating_add(MSG_LEN)
        && b.consistent();

    // 三路：闸门次序与容量边界（只读先于缺失，缺失先于类型）。
    let mut q = Rm::new();
    let _ = q.mkdir(1, 100, true, false);
    let _ = q.mkfile(1, 30, 100);
    let g1 = q.unlink(0, 30, 100) == Err(RM_EINVAL);
    let g2 = q.unlink(1, 0, 100) == Err(RM_EINVAL);
    let g3 = q.unlink(1, 30, 0) == Err(RM_EINVAL);
    let g4 = q.unlink(9, 30, 100) == Err(RM_ENOENT);
    let _ = q.set_ro(1);
    let g5 = q.unlink(1, 404, 100) == Err(RM_EROFS);
    let mut cap = Rm::new();
    let _ = cap.mkdir(1, 100, true, false);
    let mut k = 1u32;
    while (k as usize) <= ICAP {
        let _ = cap.mkfile(1, k, 100);
        k += 1;
    }
    let g6 = cap.ent_live() == ICAP;
    let _ = cap.unlink(1, 1, 100);
    let g7 = cap.mkfile(1, 99, 100) == Ok(ICAP);
    let g8 = cap.consistent();
    out[3] = g1 && g2 && g3 && g4 && g5 && g6 && g7 && g8;

    out
}

/// 十三项判据。
pub fn run_unx11392_checks() -> CheckSet {
    let mut s = CheckSet::new("unx11392.rm_single_unlink");
    let l = build_ledger();

    s.add_dyn(
        "RM1-十四格八允六拒",
        l.len() == SAMPLES && l.ok_count() == 8 && l.err_count() == 6 && l.audit().is_ok(),
        &format!("n={} ok={} err={}", l.len(), l.ok_count(), l.err_count()),
    );

    s.add(
        "RM2-建目录与建文件",
        l.sample(0).map(|x| x.value) == Some(1) && l.sample(1).map(|x| x.value) == Some(1),
        "static",
    );

    s.add(
        "RM3-硬链接计数为二",
        l.sample(2).map(|x| x.value) == Some(2),
        "static",
    );

    s.add(
        "RM4-链接减一不回收",
        l.sample(3).map(|x| x.value) == Some(0),
        "static",
    );

    s.add(
        "RM5-打开得首号且静默",
        l.sample(4).map(|x| x.value) == Some(1),
        "static",
    );

    s.add(
        "RM6-有打开者时递延不回收",
        l.sample(5).map(|x| x.value) == Some(0),
        "static",
    );

    s.add(
        "RM7-关闭即回收且目录清空",
        l.sample(6).map(|x| x.value) == Some(1) && l.sample(7).map(|x| x.value) == Some(0),
        "static",
    );

    s.add(
        "RM8-零号目录被拒",
        l.sample(8).map(|x| x.code) == Some(RM_EINVAL),
        "static",
    );

    s.add(
        "RM9-缺失名字为ENOENT",
        l.sample(9).map(|x| x.code) == Some(RM_ENOENT),
        "static",
    );

    s.add(
        "RM10-目录为EISDIR",
        l.sample(10).map(|x| x.code) == Some(RM_EISDIR),
        "static",
    );

    s.add(
        "RM11-只读先于缺失",
        l.sample(11).map(|x| x.code) == Some(RM_EROFS),
        "static",
    );

    s.add(
        "RM12-挂载点忙与sticky越权",
        l.sample(12).map(|x| x.code) == Some(RM_EBUSY)
            && l.sample(13).map(|x| x.code) == Some(RM_EPERM),
        "static",
    );

    s.add(
        "RM13-冻结重放四注入",
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
        let s = run_unx11392_checks();
        assert_eq!(s.len(), 13);
        assert!(s.all_passed());
    }

    #[test]
    fn silent_on_success() {
        let mut m = Rm::new();
        let _ = m.mkdir(1, 100, true, false);
        let _ = m.mkfile(1, 10, 100);
        assert_eq!(m.unlink(1, 10, 100), Ok(1));
        assert_eq!(m.out_bytes(), 0);
        assert!(!m.visible(1, 10));
        assert_eq!(m.ino_live(), 0);
    }

    #[test]
    fn link_count_tracks_entries() {
        let mut m = Rm::new();
        let _ = m.mkdir(1, 100, true, false);
        let _ = m.mkfile(1, 10, 100);
        let _ = m.link(1, 10, 1, 11);
        assert_eq!(m.nlink_of(1), Some(2));
        assert_eq!(m.unlink(1, 10, 100), Ok(0));
        assert_eq!(m.nlink_of(1), Some(1));
        assert!(m.visible(1, 11));
        assert_eq!(m.unlink(1, 11, 100), Ok(1));
        assert_eq!(m.ent_live(), 0);
    }

    #[test]
    fn deferred_until_last_close() {
        let mut m = Rm::new();
        let _ = m.mkdir(1, 100, true, false);
        let _ = m.mkfile(1, 10, 100);
        let fd = m.open(1, 10);
        assert_eq!(fd, Ok(1));
        assert_eq!(m.unlink(1, 10, 100), Ok(0));
        assert_eq!(m.defer_count(), 1);
        assert!(!m.visible(1, 10));
        assert_eq!(m.close(1), Ok(1));
        assert_eq!(m.defer_count(), 0);
        assert_eq!(m.ino_live(), 0);
        assert!(m.consistent());
    }

    #[test]
    fn failures_are_loud() {
        let mut m = Rm::new();
        let _ = m.mkdir(1, 100, true, false);
        assert_eq!(m.unlink(1, 7, 100), Err(RM_ENOENT));
        assert_eq!(m.out_bytes(), MSG_LEN);
        assert_eq!(m.last_code(), RM_ENOENT);
        let _ = m.subdir(1, 1, 100);
        assert_eq!(m.unlink(1, 1, 100), Err(RM_EISDIR));
        assert_eq!(m.out_bytes(), MSG_LEN * 2);
    }

    #[test]
    fn sticky_and_readonly() {
        let mut m = Rm::new();
        let _ = m.mkdir(1, 100, true, true);
        let _ = m.mkfile(1, 5, 200);
        assert_eq!(m.unlink(1, 5, 999), Err(RM_EPERM));
        assert_eq!(m.unlink(1, 5, 200), Ok(1));
        let _ = m.mkdir(2, 100, true, false);
        let _ = m.mkfile(2, 6, 100);
        let _ = m.set_ro(2);
        assert_eq!(m.unlink(2, 6, 100), Err(RM_EROFS));
    }

    #[test]
    fn inode_slots_are_reused() {
        let mut m = Rm::new();
        let _ = m.mkdir(1, 100, true, false);
        let mut k = 1u32;
        while (k as usize) <= ICAP {
            assert!(m.mkfile(1, k, 100).is_ok());
            k += 1;
        }
        assert_eq!(m.mkfile(1, 99, 100), Err(RM_EFULL));
        assert_eq!(m.unlink(1, 3, 100), Ok(1));
        assert_eq!(m.mkfile(1, 99, 100), Ok(ICAP));
        assert!(m.consistent());
    }
}
