//! UNX-F11408 mv 跨设备复制-删除回退
//!
//! 跨设备移动不可能靠一次 `rename` 完成：两个设备各有自己的 inode 号空间与
//! 目录项表，内核只能退化成「先复制、后删源」的两段式。两段之间没有任何原子
//! 性可言，因此**每一个中间状态都必须被显式建模**：
//!
//! 1. **复制前失败**：目标已存在且未 `-f`、目标只读、目标空间不足、源不可读、
//!    源被钉住——这些失败一行数据都没写，目标目录干净如初；
//! 2. **复制中失败**：短拷贝（读到的字节少于源大小）必须把已经写出的半截目标
//!    撤回。回退账记 `rolled`，且目标**不允许**留在目录里；
//! 3. **复制后失败**：副本已经完整落盘，删源却因源目录只读被拒。这时正确的
//!    做法是保留副本、如实报 `EROFS`，让用户看到「两份都在」，而不是偷偷删掉
//!    副本假装什么都没发生。
//!
//! ## `run` 的闸门次序
//! 1. 源是目录 → `EISDIR`（跨设备目录回退不在本账范围）；
//! 2. 目标目录不存在 → `ENOENT`；源被钉住 → `EBUSY`；
//! 3. 目标已有同名且未强制 → `EEXIST`（先于读源，失败不碰数据）；
//! 4. 源不可读 → `EIO`；目标只读 → `EROFS`；空间不足 → `ENOSPC`；
//! 5. 短拷贝 → 记 `copied` 与 `rolled`，报 `EIO`，目标不落盘；
//! 6. 完整复制 → 目标落盘、扣减空间；删源被拒 → `EROFS`（副本保留）；
//! 7. 删源成功 → 返回搬运字节数。
//!
//! ## 账本契约
//! 十四格：八格成功（四次完整搬运、空文件、部分回退后的状态、删源失败后的
//! 状态、剩余空间、设备对），六格失败（同设备、目标缺失、目标已存在、钉住、
//! 空间不足、源是目录）。`audit`/`seal`/`replay` 与前册同构。

use crate::alloc::format;
use crate::checks::CheckSet;

/// 段基址。
pub const SEG_BASE: u32 = 0xef80;

/// 参数非法（同设备、零 inode、零设备号）。
pub const XD_EINVAL: u32 = SEG_BASE + 0x01;
/// 目标目录不存在。
pub const XD_ENOENT: u32 = SEG_BASE + 0x02;
/// 目标已有同名且未强制覆盖。
pub const XD_EEXIST: u32 = SEG_BASE + 0x03;
/// 源被钉住。
pub const XD_EBUSY: u32 = SEG_BASE + 0x04;
/// 源是目录，拒绝跨设备回退。
pub const XD_EISDIR: u32 = SEG_BASE + 0x05;
/// 读源失败或短拷贝。
pub const XD_EIO: u32 = SEG_BASE + 0x06;
/// 目标空间不足。
pub const XD_ENOSPC: u32 = SEG_BASE + 0x07;
/// 目标或源目录只读。
pub const XD_EROFS: u32 = SEG_BASE + 0x08;
/// 账本形状不符。
pub const XD_EBAL: u32 = SEG_BASE + 0x09;
/// 槽位乱序。
pub const XD_ESLOT: u32 = SEG_BASE + 0x0a;
/// 观测值与复算不符。
pub const XD_EVALUE: u32 = SEG_BASE + 0x0b;
/// 冻结后重放不一致。
pub const XD_ESEAL: u32 = SEG_BASE + 0x0c;
/// 观察下标越界。
pub const XD_ENOBS: u32 = SEG_BASE + 0x0d;

/// 账本格数。
pub const SAMPLES: usize = 14;
/// 一次的搬运上限（4 MiB）。
pub const MOVE_MAX: u32 = 0x40_0000;

/// 操作码：执行搬运。
pub const OP_RUN: u8 = 1;
/// 操作码：观察中间状态位。
pub const OP_STATE: u8 = 2;
/// 操作码：观察目标剩余空间。
pub const OP_FREE: u8 = 3;
/// 操作码：观察设备对。
pub const OP_DEV: u8 = 4;

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

/// 跨设备搬运的两段式状态机。
#[derive(Clone, Copy, Debug)]
pub struct Xmove {
    src_ino: u16,
    size: u32,
    src_dev: u16,
    dst_dev: u16,
    free: u32,
    dst_exists: bool,
    dst_has: bool,
    force: bool,
    readable: bool,
    pinned: bool,
    src_ro: bool,
    is_dir: bool,
    dst_ro: bool,
    want: u32,
    copied: u32,
    unlinked: bool,
    dst_present: bool,
    rolled: bool,
    attempts: u32,
}

impl Xmove {
    /// 开户：源 inode、字节数、源/目标设备、目标空闲空间。
    pub fn open(ino: u16, size: u32, src_dev: u16, dst_dev: u16, free: u32) -> Result<Xmove, u32> {
        if ino == 0 || src_dev == 0 || dst_dev == 0 || src_dev == dst_dev || size > MOVE_MAX {
            return Err(XD_EINVAL);
        }
        Ok(Xmove {
            src_ino: ino,
            size,
            src_dev,
            dst_dev,
            free,
            dst_exists: true,
            dst_has: false,
            force: false,
            readable: true,
            pinned: false,
            src_ro: false,
            is_dir: false,
            dst_ro: false,
            want: size,
            copied: 0,
            unlinked: false,
            dst_present: false,
            rolled: false,
            attempts: 0,
        })
    }

    /// 源 inode。
    pub fn ino(&self) -> u16 {
        self.src_ino
    }

    /// 源字节数。
    pub fn size(&self) -> u32 {
        self.size
    }

    /// 设备对（高 16 位源设备、低 16 位目标设备）。
    pub fn dev_pair(&self) -> u32 {
        ((self.src_dev as u32) << 16) | self.dst_dev as u32
    }

    /// 已复制字节数。
    pub fn copied(&self) -> u32 {
        self.copied
    }

    /// 源是否已被删除。
    pub fn unlinked(&self) -> bool {
        self.unlinked
    }

    /// 目标是否落盘。
    pub fn dst_present(&self) -> bool {
        self.dst_present
    }

    /// 是否发生过回退。
    pub fn rolled_back(&self) -> bool {
        self.rolled
    }

    /// 源是否仍在（未删源）。
    pub fn src_present(&self) -> bool {
        !self.unlinked
    }

    /// 目标剩余空间。
    pub fn remaining(&self) -> u32 {
        self.free
    }

    /// 执行次数。
    pub fn attempts(&self) -> u32 {
        self.attempts
    }

    /// 中间状态位：bit0 目标在、bit1 已删源、bit2 已回退。
    pub fn state_bits(&self) -> u32 {
        let mut v = 0u32;
        if self.dst_present {
            v |= 1;
        }
        if self.unlinked {
            v |= 2;
        }
        if self.rolled {
            v |= 4;
        }
        v
    }

    /// 设置目标目录是否存在。
    pub fn set_dst_exists(&mut self, on: bool) {
        self.dst_exists = on;
    }

    /// 设置目标是否已有同名。
    pub fn set_dst_has(&mut self, on: bool) {
        self.dst_has = on;
    }

    /// 设置强制覆盖。
    pub fn set_force(&mut self, on: bool) {
        self.force = on;
    }

    /// 设置源可读。
    pub fn set_readable(&mut self, on: bool) {
        self.readable = on;
    }

    /// 设置源被钉住。
    pub fn set_pinned(&mut self, on: bool) {
        self.pinned = on;
    }

    /// 设置源目录只读（删源被拒）。
    pub fn set_src_ro(&mut self, on: bool) {
        self.src_ro = on;
    }

    /// 设置源是目录。
    pub fn set_src_dir(&mut self, on: bool) {
        self.is_dir = on;
    }

    /// 设置目标目录只读。
    pub fn set_dst_ro(&mut self, on: bool) {
        self.dst_ro = on;
    }

    /// 设置本次可读到的字节上限（模拟短读）。
    pub fn set_want(&mut self, n: u32) {
        self.want = if n > self.size { self.size } else { n };
    }

    /// 执行复制-删除回退。
    pub fn run(&mut self) -> Result<u32, u32> {
        self.attempts = self.attempts.saturating_add(1);
        if self.is_dir {
            return Err(XD_EISDIR);
        }
        if !self.dst_exists {
            return Err(XD_ENOENT);
        }
        if self.pinned {
            return Err(XD_EBUSY);
        }
        if self.dst_has && !self.force {
            return Err(XD_EEXIST);
        }
        if !self.readable {
            return Err(XD_EIO);
        }
        if self.dst_ro {
            return Err(XD_EROFS);
        }
        if self.free < self.size {
            return Err(XD_ENOSPC);
        }
        let got = if self.want < self.size { self.want } else { self.size };
        self.copied = got;
        if got < self.size {
            // 短拷贝：写出过半截目标，随即撤回。
            self.rolled = true;
            self.dst_present = false;
            return Err(XD_EIO);
        }
        self.free = self.free.saturating_sub(self.size);
        self.dst_present = true;
        if self.src_ro {
            // 副本完整，但删源被拒：两份都留。
            return Err(XD_EROFS);
        }
        self.unlinked = true;
        Ok(self.size)
    }

    /// 不变式自检。
    pub fn consistent(&self) -> bool {
        if self.src_ino == 0 || self.src_dev == 0 || self.dst_dev == 0 {
            return false;
        }
        if self.src_dev == self.dst_dev || self.size > MOVE_MAX {
            return false;
        }
        if self.copied > self.size {
            return false;
        }
        if self.rolled && self.dst_present {
            return false;
        }
        if self.dst_present && self.copied != self.size {
            return false;
        }
        if self.unlinked && (!self.dst_present || self.copied != self.size) {
            return false;
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

/// `Result<u8, u32>` → 样本。
fn byte_cell(slot: u8, op: u8, r: Result<u8, u32>) -> Sample {
    match r {
        Ok(v) => Sample { slot, op, ok: true, code: 0, value: v as u32 },
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
    pub fn register(&mut self, row: Sample) -> Result<(), u32> {
        if self.frozen || (row.slot as usize) != self.len || (row.slot as usize) >= SAMPLES {
            self.rejects = self.rejects.saturating_add(1);
            return Err(XD_ESLOT);
        }
        if let Some(dst) = self.rows.get_mut(self.len) {
            *dst = row;
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
            if let Some(r) = self.rows.get(i) {
                if r.ok {
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
            return Err(XD_EBAL);
        }
        const VALS: [u32; 8] = [4096, 4096, 0, 4, 1, 4096, 4096, 65538];
        const CODES: [u32; 6] =
            [XD_EINVAL, XD_ENOENT, XD_EEXIST, XD_EBUSY, XD_ENOSPC, XD_EISDIR];
        let mut vi = 0usize;
        let mut ci = 0usize;
        let mut i = 0usize;
        while i < self.len {
            let r = match self.rows.get(i) {
                Some(v) => *v,
                None => return Err(XD_EBAL),
            };
            if r.slot as usize != i {
                return Err(XD_ESLOT);
            }
            if r.ok {
                if r.code != 0 {
                    return Err(XD_EVALUE);
                }
                match VALS.get(vi) {
                    Some(w) => {
                        if r.value != *w {
                            return Err(XD_EVALUE);
                        }
                    }
                    None => return Err(XD_EBAL),
                }
                vi += 1;
            } else {
                if r.value != 0 {
                    return Err(XD_EVALUE);
                }
                match CODES.get(ci) {
                    Some(w) => {
                        if r.code != *w {
                            return Err(XD_EVALUE);
                        }
                    }
                    None => return Err(XD_EBAL),
                }
                ci += 1;
            }
            i += 1;
        }
        if vi != 8 || ci != 6 {
            return Err(XD_EBAL);
        }
        Ok(())
    }

    /// 指纹。
    pub fn seal(&self) -> u64 {
        let mut h = FNV_OFF;
        let mut i = 0usize;
        while i < self.len {
            if let Some(r) = self.rows.get(i) {
                h = fold(h, r.slot as u64);
                h = fold(h, r.op as u64);
                h = fold(h, r.ok as u64);
                h = fold(h, r.code as u64);
                h = fold(h, r.value as u64);
            }
            i += 1;
        }
        fold(h, self.rejects as u64)
    }

    /// 冻结。
    pub fn freeze(&mut self) -> Result<u64, u32> {
        if self.frozen {
            self.rejects = self.rejects.saturating_add(1);
            return Err(XD_EBAL);
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
            Err(XD_ESEAL)
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

/// 十四格账本：四次完整搬运、空文件、回退状态、留副本状态、空间与设备对，
/// 再撞六类失败。
pub fn build_ledger() -> Ledger {
    let mut l = Ledger::new();

    let mut a = match Xmove::open(700, 4096, 1, 2, 8192) {
        Ok(v) => v,
        Err(_) => return l,
    };
    let _ = l.register(word_cell(0, OP_RUN, a.run()));

    let mut b = match Xmove::open(701, 4096, 1, 2, 8192) {
        Ok(v) => v,
        Err(_) => return l,
    };
    b.set_dst_has(true);
    b.set_force(true);
    let _ = l.register(word_cell(1, OP_RUN, b.run()));

    let mut e = match Xmove::open(702, 0, 1, 2, 0) {
        Ok(v) => v,
        Err(_) => return l,
    };
    let _ = l.register(word_cell(2, OP_RUN, e.run()));

    let mut p = match Xmove::open(703, 4096, 1, 2, 8192) {
        Ok(v) => v,
        Err(_) => return l,
    };
    p.set_want(1024);
    let _ = p.run();
    let _ = l.register(word_cell(3, OP_STATE, Ok(p.state_bits())));

    let mut r = match Xmove::open(704, 4096, 1, 2, 8192) {
        Ok(v) => v,
        Err(_) => return l,
    };
    r.set_src_ro(true);
    let _ = r.run();
    let _ = l.register(word_cell(4, OP_STATE, Ok(r.state_bits())));

    let mut f = match Xmove::open(705, 4096, 1, 2, 4096) {
        Ok(v) => v,
        Err(_) => return l,
    };
    f.set_dst_has(true);
    f.set_force(true);
    let _ = l.register(word_cell(5, OP_RUN, f.run()));

    let mut g = match Xmove::open(706, 4096, 1, 2, 8192) {
        Ok(v) => v,
        Err(_) => return l,
    };
    let _ = g.run();
    let _ = l.register(word_cell(6, OP_FREE, Ok(g.remaining())));

    let d = match Xmove::open(707, 4096, 1, 2, 8192) {
        Ok(v) => v,
        Err(_) => return l,
    };
    let _ = l.register(word_cell(7, OP_DEV, Ok(d.dev_pair())));

    let _ = l.register(word_cell(
        8,
        OP_RUN,
        Xmove::open(708, 4096, 3, 3, 8192).map(|m| m.dev_pair()),
    ));
    let mut n1 = match Xmove::open(709, 4096, 1, 2, 8192) {
        Ok(v) => v,
        Err(_) => return l,
    };
    n1.set_dst_exists(false);
    let _ = l.register(word_cell(9, OP_RUN, n1.run()));

    let mut n2 = match Xmove::open(710, 4096, 1, 2, 8192) {
        Ok(v) => v,
        Err(_) => return l,
    };
    n2.set_dst_has(true);
    let _ = l.register(word_cell(10, OP_RUN, n2.run()));

    let mut n3 = match Xmove::open(711, 4096, 1, 2, 8192) {
        Ok(v) => v,
        Err(_) => return l,
    };
    n3.set_pinned(true);
    let _ = l.register(word_cell(11, OP_RUN, n3.run()));

    let mut n4 = match Xmove::open(712, 4096, 1, 2, 1024) {
        Ok(v) => v,
        Err(_) => return l,
    };
    let _ = l.register(word_cell(12, OP_RUN, n4.run()));

    let mut n5 = match Xmove::open(713, 4096, 1, 2, 8192) {
        Ok(v) => v,
        Err(_) => return l,
    };
    n5.set_src_dir(true);
    let _ = l.register(word_cell(13, OP_RUN, n5.run()));

    l
}

/// 四路注入：账本身、完整搬运、八类失败、回退与闸门次序。
pub fn inject_four() -> [bool; 4] {
    let mut out = [false; 4];

    let l = build_ledger();
    let rep = l.replay();
    let root = l.seal();
    out[0] = l.len() == SAMPLES && l.audit().is_ok() && rep == Ok(root) && root != 0;

    // 一路：完整搬运，目标落盘、源删除、空间扣减。
    let mut a = match Xmove::open(800, 4096, 1, 2, 8192) {
        Ok(v) => v,
        Err(_) => return out,
    };
    let moved = a.run();
    out[1] = moved == Ok(4096)
        && a.copied() == 4096
        && a.dst_present()
        && a.unlinked()
        && !a.src_present()
        && !a.rolled_back()
        && a.remaining() == 4096
        && a.state_bits() == 3
        && a.attempts() == 1
        && a.dev_pair() == 0x0001_0002
        && a.consistent();

    // 二路：八类失败各自可辨。
    let same = Xmove::open(801, 4096, 2, 2, 8192).is_err();
    let zero_dev = Xmove::open(802, 4096, 0, 2, 8192).is_err();
    let zero_ino = Xmove::open(0, 4096, 1, 2, 8192).is_err();
    let too_big = Xmove::open(803, MOVE_MAX + 1, 1, 2, 8192).is_err();
    let mut b = match Xmove::open(804, 4096, 1, 2, 8192) {
        Ok(v) => v,
        Err(_) => return out,
    };
    b.set_dst_exists(false);
    let missing = b.run() == Err(XD_ENOENT);
    b.set_dst_exists(true);
    b.set_dst_has(true);
    let exists = b.run() == Err(XD_EEXIST);
    b.set_force(true);
    b.set_pinned(true);
    let busy = b.run() == Err(XD_EBUSY);
    b.set_pinned(false);
    b.set_dst_ro(true);
    let ro_dst = b.run() == Err(XD_EROFS);
    b.set_dst_ro(false);
    b.set_readable(false);
    let unreadable = b.run() == Err(XD_EIO);
    b.set_readable(true);
    b.set_src_dir(true);
    let dir = b.run() == Err(XD_EISDIR);
    out[2] = same
        && zero_dev
        && zero_ino
        && too_big
        && missing
        && exists
        && busy
        && ro_dst
        && unreadable
        && dir
        && b.consistent();

    // 三路：三种中间态与闸门次序。
    let mut c = match Xmove::open(805, 4096, 1, 2, 1024) {
        Ok(v) => v,
        Err(_) => return out,
    };
    let no_space = c.run() == Err(XD_ENOSPC);
    let clean = !c.dst_present() && !c.rolled_back() && c.copied() == 0;

    let mut d = match Xmove::open(806, 4096, 1, 2, 8192) {
        Ok(v) => v,
        Err(_) => return out,
    };
    d.set_want(1024);
    let short = d.run() == Err(XD_EIO);
    let half = d.copied() == 1024 && d.rolled_back() && !d.dst_present() && !d.unlinked();

    let mut e = match Xmove::open(807, 4096, 1, 2, 8192) {
        Ok(v) => v,
        Err(_) => return out,
    };
    e.set_src_ro(true);
    let kept = e.run() == Err(XD_EROFS);
    let two_copies = e.dst_present() && e.copied() == 4096 && !e.unlinked() && e.src_present();

    let mut f = match Xmove::open(808, 4096, 1, 2, 8192) {
        Ok(v) => v,
        Err(_) => return out,
    };
    f.set_dst_has(true);
    f.set_readable(false);
    let exist_before_read = f.run() == Err(XD_EEXIST);

    out[3] = no_space
        && clean
        && short
        && half
        && kept
        && two_copies
        && exist_before_read
        && c.consistent()
        && d.consistent()
        && e.consistent();

    out
}

/// 十三项判据。
pub fn run_unx11408_checks() -> CheckSet {
    let mut s = CheckSet::new("unx11408.mv_cross_device_fallback");
    let l = build_ledger();

    s.add_dyn(
        "XD1-十四格八允六拒",
        l.len() == SAMPLES && l.ok_count() == 8 && l.err_count() == 6 && l.audit().is_ok(),
        &format!("n={} ok={} err={}", l.len(), l.ok_count(), l.err_count()),
    );

    s.add(
        "XD2-完整搬运两段收",
        {
            let mut a = match Xmove::open(900, 4096, 1, 2, 8192) {
                Ok(v) => v,
                Err(_) => return s,
            };
            a.run() == Ok(4096)
                && a.unlinked()
                && a.dst_present()
                && a.remaining() == 4096
                && a.state_bits() == 3
                && l.sample(0).map(|x| x.value) == Some(4096)
        },
        "static",
    );

    s.add(
        "XD3-空文件零字节",
        l.sample(2).map(|x| x.value) == Some(0)
            && {
                let mut a = match Xmove::open(901, 0, 1, 2, 0) {
                    Ok(v) => v,
                    Err(_) => return s,
                };
                a.run() == Ok(0) && a.copied() == 0 && a.dst_present() && a.unlinked()
            },
        "static",
    );

    s.add(
        "XD4-同设备与坏参数",
        l.sample(8).map(|x| x.code) == Some(XD_EINVAL)
            && Xmove::open(902, 1, 5, 5, 8).is_err()
            && Xmove::open(903, 1, 0, 5, 8).is_err()
            && Xmove::open(0, 1, 1, 2, 8).is_err()
            && Xmove::open(904, MOVE_MAX + 1, 1, 2, 8).is_err(),
        "static",
    );

    s.add(
        "XD5-目标已存在与强制覆盖",
        {
            let mut a = match Xmove::open(905, 4096, 1, 2, 8192) {
                Ok(v) => v,
                Err(_) => return s,
            };
            a.set_dst_has(true);
            let refused = a.run() == Err(XD_EEXIST);
            a.set_force(true);
            let forced = a.run() == Ok(4096);
            refused
                && forced
                && a.dst_present()
                && a.unlinked()
                && l.sample(10).map(|x| x.code) == Some(XD_EEXIST)
        },
        "static",
    );

    s.add(
        "XD6-空间不足不落盘",
        {
            let mut a = match Xmove::open(906, 4096, 1, 2, 4095) {
                Ok(v) => v,
                Err(_) => return s,
            };
            a.run() == Err(XD_ENOSPC)
                && !a.dst_present()
                && !a.rolled_back()
                && a.copied() == 0
                && a.remaining() == 4095
                && l.sample(12).map(|x| x.code) == Some(XD_ENOSPC)
        },
        "static",
    );

    s.add(
        "XD7-短拷贝回滚",
        {
            let mut a = match Xmove::open(907, 4096, 1, 2, 8192) {
                Ok(v) => v,
                Err(_) => return s,
            };
            a.set_want(1024);
            a.run() == Err(XD_EIO)
                && a.copied() == 1024
                && a.rolled_back()
                && !a.dst_present()
                && a.src_present()
                && a.state_bits() == 4
                && a.consistent()
                && l.sample(3).map(|x| x.value) == Some(4)
        },
        "static",
    );

    s.add(
        "XD8-删源失败留副本",
        {
            let mut a = match Xmove::open(908, 4096, 1, 2, 8192) {
                Ok(v) => v,
                Err(_) => return s,
            };
            a.set_src_ro(true);
            a.run() == Err(XD_EROFS)
                && a.dst_present()
                && a.copied() == 4096
                && !a.unlinked()
                && a.src_present()
                && !a.rolled_back()
                && a.state_bits() == 1
                && a.consistent()
                && l.sample(4).map(|x| x.value) == Some(1)
        },
        "static",
    );

    s.add(
        "XD9-目录拒绝回退",
        {
            let mut a = match Xmove::open(909, 4096, 1, 2, 8192) {
                Ok(v) => v,
                Err(_) => return s,
            };
            a.set_src_dir(true);
            a.run() == Err(XD_EISDIR)
                && !a.dst_present()
                && a.copied() == 0
                && l.sample(13).map(|x| x.code) == Some(XD_EISDIR)
        },
        "static",
    );

    s.add(
        "XD10-源被钉住",
        {
            let mut a = match Xmove::open(910, 4096, 1, 2, 8192) {
                Ok(v) => v,
                Err(_) => return s,
            };
            a.set_pinned(true);
            a.run() == Err(XD_EBUSY)
                && !a.dst_present()
                && a.remaining() == 8192
                && l.sample(11).map(|x| x.code) == Some(XD_EBUSY)
        },
        "static",
    );

    s.add(
        "XD11-目标目录缺失",
        {
            let mut a = match Xmove::open(911, 4096, 1, 2, 8192) {
                Ok(v) => v,
                Err(_) => return s,
            };
            a.set_dst_exists(false);
            a.run() == Err(XD_ENOENT)
                && a.attempts() == 1
                && !a.dst_present()
                && l.sample(9).map(|x| x.code) == Some(XD_ENOENT)
        },
        "static",
    );

    s.add(
        "XD12-只读与闸门次序",
        {
            let mut a = match Xmove::open(912, 4096, 1, 2, 8192) {
                Ok(v) => v,
                Err(_) => return s,
            };
            a.set_dst_ro(true);
            let ro = a.run() == Err(XD_EROFS) && !a.dst_present() && a.copied() == 0;
            a.set_dst_ro(false);
            a.set_dst_has(true);
            a.set_readable(false);
            let exist_first = a.run() == Err(XD_EEXIST);
            a.set_force(true);
            let read_first = a.run() == Err(XD_EIO);
            a.set_readable(true);
            a.set_want(1);
            let short_before_space = a.run() == Err(XD_EIO);
            ro && exist_first && read_first && short_before_space && a.consistent()
        },
        "static",
    );

    s.add(
        "XD13-冻结重放四注入",
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

    fn xm(size: u32, free: u32) -> Xmove {
        match Xmove::open(1, size, 1, 2, free) {
            Ok(v) => v,
            Err(_) => panic!("open"),
        }
    }

    #[test]
    fn ledger_shape() {
        let l = build_ledger();
        assert_eq!(l.len(), SAMPLES);
        assert_eq!(l.ok_count(), 8);
        assert_eq!(l.err_count(), 6);
        assert_eq!(l.rejects(), 0);
        assert!(l.audit().is_ok());
        assert_ne!(l.seal(), 0);
        assert_eq!(l.replay(), Ok(l.seal()));
        let mut i = 0usize;
        while i < l.len() {
            assert_eq!(l.sample(i).map(|x| x.slot as usize), Some(i));
            i += 1;
        }
    }

    #[test]
    fn checks_pass() {
        let s = run_unx11408_checks();
        assert_eq!(s.len(), 13);
        assert!(s.all_passed());
    }

    #[test]
    fn full_move_consumes_and_unlinks() {
        let mut a = xm(4096, 8192);
        assert_eq!(a.run(), Ok(4096));
        assert_eq!(a.copied(), 4096);
        assert!(a.dst_present());
        assert!(a.unlinked());
        assert_eq!(a.remaining(), 4096);
        assert_eq!(a.state_bits(), 3);
        assert!(a.consistent());
    }

    #[test]
    fn short_copy_rolls_back() {
        let mut a = xm(4096, 8192);
        a.set_want(512);
        assert_eq!(a.run(), Err(XD_EIO));
        assert_eq!(a.copied(), 512);
        assert!(a.rolled_back());
        assert!(!a.dst_present());
        assert!(a.src_present());
        assert_eq!(a.state_bits(), 4);
        assert!(a.consistent());
    }

    #[test]
    fn unlink_failure_keeps_copy() {
        let mut a = xm(4096, 8192);
        a.set_src_ro(true);
        assert_eq!(a.run(), Err(XD_EROFS));
        assert!(a.dst_present());
        assert_eq!(a.copied(), 4096);
        assert!(!a.unlinked());
        assert!(a.src_present());
        assert_eq!(a.state_bits(), 1);
    }

    #[test]
    fn space_and_pin_refuse_cleanly() {
        let mut a = xm(4096, 1024);
        assert_eq!(a.run(), Err(XD_ENOSPC));
        assert!(!a.dst_present());
        assert_eq!(a.copied(), 0);
        let mut b = xm(4096, 8192);
        b.set_pinned(true);
        assert_eq!(b.run(), Err(XD_EBUSY));
        assert_eq!(b.remaining(), 8192);
        let mut c = xm(4096, 8192);
        c.set_dst_has(true);
        assert_eq!(c.run(), Err(XD_EEXIST));
        c.set_force(true);
        assert_eq!(c.run(), Ok(4096));
    }

    #[test]
    fn params_and_dir_guarded() {
        assert!(Xmove::open(1, 1, 4, 4, 8).is_err());
        assert!(Xmove::open(0, 1, 1, 2, 8).is_err());
        assert!(Xmove::open(1, 1, 0, 2, 8).is_err());
        let mut d = xm(4096, 8192);
        d.set_src_dir(true);
        assert_eq!(d.run(), Err(XD_EISDIR));
        assert!(!d.dst_present());
        let mut m = xm(4096, 8192);
        m.set_dst_exists(false);
        assert_eq!(m.run(), Err(XD_ENOENT));
    }

    #[test]
    fn inject_four_holds() {
        assert_eq!(inject_four(), [true; 4]);
    }
}
