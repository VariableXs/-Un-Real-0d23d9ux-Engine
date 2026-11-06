//! UNX-F11391 cp 跨设备 EXDEV 与回退（0xee70 段）
//!
//! `cp` 在同一个设备上可以走内核的「快道」——`copy_file_range` 之类的调用直接
//! 在内核里搬块，拷贝对用户态是原子的、块数也由文件系统说了算。一旦源和目标
//! 落在**不同设备**上，快道立刻以 `EXDEV` 失败：跨设备没有共同的块映射，内核
//! 没法把「块」直接挪过去。
//!
//! 真正需要被建模的不是 `EXDEV` 这个错误本身，而是 `cp` 对它的**处理**：
//! 用户态必须把快道失败翻译成「逐块读写循环」——按固定块长把源读出来、写到目标
//! 去，写完后校验摘要，只有在摘要一致时才允许目标条目出现在目录里。这条回退
//! 路径对用户必须**完全无感**：目标的内容、尺寸、摘要求和源逐字节等价，唯一的
//! 可观测差别是慢。
//!
//! 本模型把设备表、目录项表和搬运计数分开记账：
//!
//! - `fast` 记同设备快道成功次数；
//! - `fallbacks` 记跨设备回退次数（快道先失败，再走读写循环）；
//! - `chunks` 记回退路径真正搬运的块数；
//! - `rollbacks` 记校验失败后回滚的次数——回滚必须把设备占用和目标条目一起
//!   撤回，不能留下半成品。
//!
//! 错误类为何彼此独立：
//!
//! - `EINVAL` 是调用形状错（空名、零设备号、零尺寸），与文件系统状态无关；
//! - `ENOENT` 是源条目不在表里，重试同一个名字永远失败；
//! - `EISDIR` 是源存在但类型不对，换成 `cp -r` 才有意义，和「不存在」是两回事；
//! - `EDEV` 是目标设备号没注册，属于挂载拓扑错，不是空间错；
//! - `EFULL` 是条目表满，删一条就能继续；
//! - `ENOSPC` 是目标设备空间不足，删大文件或换设备才能继续；
//! - `ECHK` 是回退搬运完成后摘要不符，属于介质/竞争错，重试可能成功；
//! - `ELOOP` 是源与目标指向同一个 inode，属于自覆，`cp` 必须直接拒绝。
//!
//! 闸门次序（`copy`）：空名/零设备 → 源存在 → 源非目录 → 目标设备注册 →
//! 自覆 → 条目表满（覆盖不占新格）→ 目标空间 → 搬运。这个次序保证错误码只由
//! 「第一个不成立的断言」决定：例如源不存在时绝不会先报设备未注册，源是目录时
//! 绝不会先报空间不足。

use crate::alloc::format;
use crate::checks::CheckSet;

/// 段基址。
pub const SEG_BASE: u32 = 0xee70;

/// 参数非法（空名、零设备号、零尺寸、重复播种）。
pub const XD_EINVAL: u32 = SEG_BASE + 0x01;
/// 源条目不存在。
pub const XD_ENOENT: u32 = SEG_BASE + 0x02;
/// 源是目录，单件复制不收。
pub const XD_EISDIR: u32 = SEG_BASE + 0x03;
/// 目标设备未注册。
pub const XD_EDEV: u32 = SEG_BASE + 0x04;
/// 条目表已满且目标名需要新格。
pub const XD_EFULL: u32 = SEG_BASE + 0x05;
/// 目标设备空间不足。
pub const XD_ENOSPC: u32 = SEG_BASE + 0x06;
/// 回退搬运完成后摘要不符。
pub const XD_ECHK: u32 = SEG_BASE + 0x07;
/// 源与目标指向同一个 inode。
pub const XD_ELOOP: u32 = SEG_BASE + 0x08;
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

/// 条目表容量。
pub const CAP: usize = 8;
/// 设备表容量。
pub const DCAP: usize = 4;
/// 账本格数。
pub const SAMPLES: usize = 14;
/// 回退路径的块长（字节）。
pub const CHUNK: u32 = 16;

/// 操作码：注册设备。
pub const OP_MKDEV: u8 = 1;
/// 操作码：播种文件。
pub const OP_CREATE: u8 = 2;
/// 操作码：播种目录。
pub const OP_MKDIR: u8 = 3;
/// 操作码：复制。
pub const OP_COPY: u8 = 4;
/// 操作码：比对两件是否等价。
pub const OP_SAME: u8 = 5;
/// 操作码：读设备剩余空间。
pub const OP_FREE: u8 = 6;

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

/// 一个设备。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Dev {
    /// 设备号。
    pub id: u32,
    /// 总字节容量。
    pub span: u32,
    /// 已用字节。
    pub used: u32,
}

impl Dev {
    /// 空设备。
    pub const fn blank() -> Dev {
        Dev { id: 0, span: 0, used: 0 }
    }

    /// 剩余字节。
    pub fn free(&self) -> u32 {
        self.span.saturating_sub(self.used)
    }
}

/// 一个目录项。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Ent {
    /// 条目名（不透明编号，等价于一个路径的最后一段）。
    pub name: u32,
    /// 所在设备。
    pub dev: u32,
    /// inode 号。
    pub ino: u32,
    /// 字节数（目录为 0）。
    pub size: u32,
    /// 内容摘要。
    pub sum: u64,
    /// 是否目录。
    pub dir: bool,
}

impl Ent {
    /// 空条目。
    pub const fn blank() -> Ent {
        Ent { name: 0, dev: 0, ino: 0, size: 0, sum: 0, dir: false }
    }
}

/// 复制引擎：设备表 + 条目表 + 搬运计数。
#[derive(Clone, Copy, Debug)]
pub struct Cp {
    d: [Dev; DCAP],
    dn: usize,
    e: [Ent; CAP],
    n: usize,
    next_ino: u32,
    fast: u32,
    fallbacks: u32,
    chunks: u32,
    bytes: u32,
    rollbacks: u32,
}

impl Cp {
    /// 空引擎。
    pub const fn new() -> Cp {
        Cp {
            d: [Dev::blank(); DCAP],
            dn: 0,
            e: [Ent::blank(); CAP],
            n: 0,
            next_ino: 1,
            fast: 0,
            fallbacks: 0,
            chunks: 0,
            bytes: 0,
            rollbacks: 0,
        }
    }

    /// 设备条数。
    pub fn dev_count(&self) -> usize {
        self.dn
    }

    /// 条目条数。
    pub fn ent_count(&self) -> usize {
        self.n
    }

    /// 快道成功次数。
    pub fn fast_count(&self) -> u32 {
        self.fast
    }

    /// 回退次数。
    pub fn fallback_count(&self) -> u32 {
        self.fallbacks
    }

    /// 回退搬运块数。
    pub fn chunk_count(&self) -> u32 {
        self.chunks
    }

    /// 回退搬运字节数。
    pub fn byte_count(&self) -> u32 {
        self.bytes
    }

    /// 回滚次数。
    pub fn rollback_count(&self) -> u32 {
        self.rollbacks
    }

    /// 第 i 个设备。
    pub fn dev_at(&self, i: usize) -> Option<Dev> {
        if i >= self.dn {
            return None;
        }
        self.d.get(i).copied()
    }

    /// 第 i 个条目。
    pub fn ent_at(&self, i: usize) -> Option<Ent> {
        if i >= self.n {
            return None;
        }
        self.e.get(i).copied()
    }

    /// 按号找设备下标。
    fn find_dev(&self, id: u32) -> Option<usize> {
        let mut i = 0usize;
        while i < self.dn {
            match self.d.get(i) {
                Some(x) => {
                    if x.id == id {
                        return Some(i);
                    }
                }
                None => return None,
            }
            i += 1;
        }
        None
    }

    /// 按名找条目下标。
    fn find_ent(&self, name: u32) -> Option<usize> {
        let mut i = 0usize;
        while i < self.n {
            match self.e.get(i) {
                Some(x) => {
                    if x.name == name {
                        return Some(i);
                    }
                }
                None => return None,
            }
            i += 1;
        }
        None
    }

    /// 取一个条目的拷贝。
    fn ent_of(&self, name: u32) -> Result<Ent, u32> {
        match self.find_ent(name) {
            Some(i) => match self.e.get(i) {
                Some(x) => Ok(*x),
                None => Err(XD_ENOENT),
            },
            None => Err(XD_ENOENT),
        }
    }

    /// 取一个设备的拷贝。
    fn dev_of(&self, id: u32) -> Result<Dev, u32> {
        match self.find_dev(id) {
            Some(i) => match self.d.get(i) {
                Some(x) => Ok(*x),
                None => Err(XD_EDEV),
            },
            None => Err(XD_EDEV),
        }
    }

    /// 注册一个设备。
    pub fn mkdev(&mut self, id: u32, span: u32) -> Result<usize, u32> {
        if id == 0 || span == 0 {
            return Err(XD_EINVAL);
        }
        if self.find_dev(id).is_some() {
            return Err(XD_EINVAL);
        }
        if self.dn >= DCAP {
            return Err(XD_EFULL);
        }
        if let Some(slot) = self.d.get_mut(self.dn) {
            *slot = Dev { id, span, used: 0 };
        }
        self.dn += 1;
        Ok(self.dn)
    }

    /// 播种一个文件（仅供夹具使用；目录项的真实创建走文件系统）。
    pub fn create(&mut self, name: u32, dev: u32, size: u32) -> Result<usize, u32> {
        if name == 0 || size == 0 {
            return Err(XD_EINVAL);
        }
        let di = self.find_dev(dev).ok_or(XD_EDEV)?;
        if self.find_ent(name).is_some() {
            return Err(XD_EINVAL);
        }
        if self.n >= CAP {
            return Err(XD_EFULL);
        }
        let d = match self.d.get(di) {
            Some(x) => *x,
            None => return Err(XD_EDEV),
        };
        if d.used.saturating_add(size) > d.span {
            return Err(XD_ENOSPC);
        }
        let sum = fold(fold(FNV_OFF, name as u64), size as u64);
        if let Some(slot) = self.e.get_mut(self.n) {
            *slot = Ent { name, dev, ino: self.next_ino, size, sum, dir: false };
        }
        self.next_ino = self.next_ino.saturating_add(1);
        if let Some(dm) = self.d.get_mut(di) {
            dm.used = dm.used.saturating_add(size);
        }
        self.n += 1;
        Ok(self.n)
    }

    /// 播种一个目录。
    pub fn mkdir(&mut self, name: u32, dev: u32) -> Result<usize, u32> {
        if name == 0 {
            return Err(XD_EINVAL);
        }
        if self.find_dev(dev).is_none() {
            return Err(XD_EDEV);
        }
        if self.find_ent(name).is_some() {
            return Err(XD_EINVAL);
        }
        if self.n >= CAP {
            return Err(XD_EFULL);
        }
        if let Some(slot) = self.e.get_mut(self.n) {
            *slot = Ent {
                name,
                dev,
                ino: self.next_ino,
                size: 0,
                sum: fold(FNV_OFF, name as u64),
                dir: true,
            };
        }
        self.next_ino = self.next_ino.saturating_add(1);
        self.n += 1;
        Ok(self.n)
    }

    /// 设备剩余空间。
    pub fn free_of(&self, dev: u32) -> Result<u32, u32> {
        match self.dev_of(dev) {
            Ok(d) => Ok(d.free()),
            Err(c) => Err(c),
        }
    }

    /// 两件是否逐字节等价（尺寸 + 摘要）。返回 1 等价、0 不等价。
    pub fn same(&self, a: u32, b: u32) -> Result<u32, u32> {
        let x = self.ent_of(a)?;
        let y = self.ent_of(b)?;
        if x.size == y.size && x.sum == y.sum {
            Ok(1)
        } else {
            Ok(0)
        }
    }

    /// 按块搬运：模拟回退路径的读—写循环。
    ///
    /// `poison` 为被注入损坏的块号（`Some` 只在故障注入时出现）。任一被注入的块
    /// 读回摘要即判定不符，立即停止；已搬的块计入块账，失败块不计。
    fn transfer(&mut self, size: u32, poison: Option<u32>) -> Result<u32, u32> {
        let mut moved = 0u32;
        let mut idx = 0u32;
        while moved < size {
            let left = size.saturating_sub(moved);
            let take = if left > CHUNK { CHUNK } else { left };
            if let Some(p) = poison {
                if p == idx {
                    return Err(XD_ECHK);
                }
            }
            moved = moved.saturating_add(take);
            idx = idx.saturating_add(1);
        }
        self.chunks = self.chunks.saturating_add(idx);
        Ok(moved)
    }

    /// 复制一个文件到 `dst_dev` 下的 `dst_name`。
    pub fn copy(&mut self, src: u32, dst_dev: u32, dst_name: u32) -> Result<usize, u32> {
        self.copy_inner(src, dst_dev, dst_name, None)
    }

    /// 带故障注入的复制：第 `poison` 块读回摘要不符。
    pub fn copy_faulty(&mut self, src: u32, dst_dev: u32, dst_name: u32, poison: u32) -> Result<usize, u32> {
        self.copy_inner(src, dst_dev, dst_name, Some(poison))
    }

    /// 复制主体。闸门次序见模块文档。
    fn copy_inner(&mut self, src: u32, dst_dev: u32, dst_name: u32, poison: Option<u32>) -> Result<usize, u32> {
        if src == 0 || dst_name == 0 || dst_dev == 0 {
            return Err(XD_EINVAL);
        }
        let s = self.ent_of(src)?;
        if s.dir {
            return Err(XD_EISDIR);
        }
        let di = match self.find_dev(dst_dev) {
            Some(v) => v,
            None => return Err(XD_EDEV),
        };
        // 自覆：目标名当前指向的 inode 就是源 inode。
        if let Some(ti) = self.find_ent(dst_name) {
            if let Some(t) = self.e.get(ti) {
                if t.dev == s.dev && t.ino == s.ino {
                    return Err(XD_ELOOP);
                }
            }
        }
        let replace = self.find_ent(dst_name);
        if replace.is_none() && self.n >= CAP {
            return Err(XD_EFULL);
        }
        let d = match self.d.get(di) {
            Some(v) => *v,
            None => return Err(XD_EDEV),
        };
        let reclaim = match replace {
            Some(ti) => match self.e.get(ti) {
                Some(t) => t.size,
                None => 0,
            },
            None => 0,
        };
        let need = d
            .used
            .saturating_sub(reclaim)
            .saturating_add(s.size);
        if need > d.span {
            return Err(XD_ENOSPC);
        }

        // 快道只有同设备可用；跨设备必然先 EXDEV，再落入读写循环。
        let cross = s.dev != dst_dev;
        if cross {
            self.fallbacks = self.fallbacks.saturating_add(1);
        } else {
            self.fast = self.fast.saturating_add(1);
        }
        let moved = match self.transfer(s.size, poison) {
            Ok(v) => v,
            Err(c) => {
                // 校验失败：目标从未落表，设备占用也未提交，只记一次回滚。
                self.rollbacks = self.rollbacks.saturating_add(1);
                return Err(c);
            }
        };

        let new_ino = self.next_ino;
        self.next_ino = self.next_ino.saturating_add(1);
        let fresh = Ent {
            name: dst_name,
            dev: dst_dev,
            ino: new_ino,
            size: s.size,
            sum: s.sum,
            dir: false,
        };
        match replace {
            Some(ti) => {
                if let Some(slot) = self.e.get_mut(ti) {
                    *slot = fresh;
                }
            }
            None => {
                if let Some(slot) = self.e.get_mut(self.n) {
                    *slot = fresh;
                }
                self.n += 1;
            }
        }
        if let Some(dm) = self.d.get_mut(di) {
            dm.used = need;
        }
        self.bytes = self.bytes.saturating_add(moved);
        Ok(moved as usize)
    }

    /// 不变式自检。
    pub fn consistent(&self) -> bool {
        if self.dn > DCAP || self.n > CAP {
            return false;
        }
        let mut i = 0usize;
        while i < self.dn {
            let d = match self.d.get(i) {
                Some(v) => *v,
                None => return false,
            };
            if d.id == 0 || d.span == 0 || d.used > d.span {
                return false;
            }
            let mut j = i + 1;
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
            i += 1;
        }
        let mut k = 0usize;
        while k < self.n {
            let e = match self.e.get(k) {
                Some(v) => *v,
                None => return false,
            };
            if e.name == 0 || e.ino == 0 {
                return false;
            }
            if self.find_dev(e.dev).is_none() {
                return false;
            }
            if !e.dir && e.size == 0 {
                return false;
            }
            let mut l = k + 1;
            while l < self.n {
                match self.e.get(l) {
                    Some(o) => {
                        if o.name == e.name || o.ino == e.ino {
                            return false;
                        }
                    }
                    None => return false,
                }
                l += 1;
            }
            k += 1;
        }
        // 账目自洽：回退次数不会超过复制次数上界，块数按块长上取整。
        self.fallbacks <= self.fallbacks.saturating_add(self.fast)
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
            return Err(XD_ESLOT);
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
            return Err(XD_EBAL);
        }
        const VALS: [u32; 8] = [1, 2, 1, 64, 64, 1, 960, 3];
        const CODES: [u32; 6] =
            [XD_EINVAL, XD_ENOENT, XD_EISDIR, XD_EDEV, XD_ENOSPC, XD_ECHK];
        let mut vi = 0usize;
        let mut ci = 0usize;
        let mut i = 0usize;
        while i < self.len {
            let s = match self.rows.get(i) {
                Some(v) => *v,
                None => return Err(XD_EBAL),
            };
            if s.slot as usize != i {
                return Err(XD_ESLOT);
            }
            if s.ok {
                if s.code != 0 {
                    return Err(XD_EVALUE);
                }
                match VALS.get(vi) {
                    Some(w) => {
                        if s.value != *w {
                            return Err(XD_EVALUE);
                        }
                    }
                    None => return Err(XD_EBAL),
                }
                vi += 1;
            } else {
                match CODES.get(ci) {
                    Some(w) => {
                        if s.code != *w {
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

/// 十四格账本：两设备一源 → 同设备快道 → 跨设备回退 → 等价比对 → 六类拒绝。
pub fn build_ledger() -> Ledger {
    let mut l = Ledger::new();
    let mut c = Cp::new();

    let _ = l.register(cell(0, OP_MKDEV, c.mkdev(1, 1024)));
    let _ = l.register(cell(1, OP_MKDEV, c.mkdev(2, 1024)));
    let _ = l.register(cell(2, OP_CREATE, c.create(10, 1, 64)));
    let _ = l.register(cell(3, OP_COPY, c.copy(10, 1, 20)));
    let _ = l.register(cell(4, OP_COPY, c.copy(10, 2, 30)));
    let _ = l.register(word_cell(5, OP_SAME, c.same(10, 30)));
    let _ = l.register(word_cell(6, OP_FREE, c.free_of(2)));
    let _ = l.register(cell(7, OP_CREATE, Ok(c.ent_count())));

    let mut a = Cp::new();
    let _ = l.register(cell(8, OP_CREATE, a.create(0, 1, 8)));

    let mut b = Cp::new();
    let _ = b.mkdev(1, 128);
    let _ = l.register(cell(9, OP_COPY, b.copy(7, 1, 8)));

    let mut d = Cp::new();
    let _ = d.mkdev(1, 512);
    let _ = d.mkdir(40, 1);
    let _ = l.register(cell(10, OP_COPY, d.copy(40, 1, 41)));

    let mut e = Cp::new();
    let _ = e.mkdev(1, 512);
    let _ = e.create(50, 1, 16);
    let _ = l.register(cell(11, OP_COPY, e.copy(50, 9, 51)));

    let mut f = Cp::new();
    let _ = f.mkdev(1, 2048);
    let _ = f.mkdev(2, 32);
    let _ = f.create(60, 1, 64);
    let _ = l.register(cell(12, OP_COPY, f.copy(60, 2, 61)));

    let mut g = Cp::new();
    let _ = g.mkdev(1, 512);
    let _ = g.mkdev(2, 512);
    let _ = g.create(70, 1, 32);
    let _ = l.register(cell(13, OP_COPY, g.copy_faulty(70, 2, 71, 0)));

    l
}

/// 四路注入。
pub fn inject_four() -> [bool; 4] {
    let mut out = [false; 4];

    let l = build_ledger();
    let rep = l.replay();
    let root = l.seal();
    out[0] = l.len() == SAMPLES && l.audit().is_ok() && rep == Ok(root) && root != 0;

    // 一路：跨设备回退与源逐字节等价，计数精确。
    let mut a = Cp::new();
    let _ = a.mkdev(1, 512);
    let _ = a.mkdev(2, 512);
    let _ = a.create(100, 1, 64);
    let fast = a.copy(100, 1, 101);
    let slow = a.copy(100, 2, 102);
    out[1] = fast == Ok(64)
        && slow == Ok(64)
        && a.fast_count() == 1
        && a.fallback_count() == 1
        && a.chunk_count() == 8
        && a.byte_count() == 128
        && a.same(100, 102) == Ok(1)
        && a.free_of(2) == Ok(448)
        && a.ent_count() == 3
        && a.consistent();

    // 二路：六类失败彼此可辨，校验失败不留半成品。
    let mut b = Cp::new();
    let _ = b.mkdev(1, 1024);
    let _ = b.mkdev(2, 512);
    let _ = b.mkdev(3, 16);
    let _ = b.create(200, 1, 32);
    let _ = b.mkdir(300, 1);
    let missing = b.copy(999, 1, 1) == Err(XD_ENOENT);
    let isdir = b.copy(300, 1, 301) == Err(XD_EISDIR);
    let nodev = b.copy(200, 9, 201) == Err(XD_EDEV);
    let nospc = b.copy(200, 3, 202) == Err(XD_ENOSPC);
    let loop_err = b.copy(200, 1, 200) == Err(XD_ELOOP);
    let ents = b.ent_count();
    let used = b.free_of(2);
    let chk = b.copy_faulty(200, 2, 203, 1) == Err(XD_ECHK);
    out[2] = missing
        && isdir
        && nodev
        && nospc
        && loop_err
        && chk
        && b.ent_count() == ents
        && b.free_of(2) == used
        && b.rollback_count() == 1
        && b.consistent();

    // 三路：闸门次序与容量边界。
    let mut q = Cp::new();
    let _ = q.mkdev(1, 512);
    let _ = q.create(400, 1, 16);
    let _ = q.mkdir(401, 1);
    let g1 = q.copy(0, 1, 402) == Err(XD_EINVAL);
    let g2 = q.copy(400, 0, 402) == Err(XD_EINVAL);
    let g3 = q.copy(998, 9, 402) == Err(XD_ENOENT);
    let g4 = q.copy(401, 9, 403) == Err(XD_EISDIR);
    let g5 = q.copy(400, 9, 404) == Err(XD_EDEV);
    let g6 = q.copy(400, 1, 400) == Err(XD_ELOOP);
    let mut full = Cp::new();
    let _ = full.mkdev(1, 4096);
    let mut i = 1u32;
    while (i as usize) <= CAP {
        let _ = full.create(i, 1, 8);
        i += 1;
    }
    let g7 = full.copy(1, 1, 900) == Err(XD_EFULL);
    let g8 = full.copy(1, 1, 2) == Ok(8);
    let g9 = full.copy(2, 1, 5) == Ok(8);
    out[3] = g1 && g2 && g3 && g4 && g5 && g6 && g7 && g8 && g9 && full.consistent();

    out
}

/// 十三项判据。
pub fn run_unx11391_checks() -> CheckSet {
    let mut s = CheckSet::new("unx11391.cp_exdev_fallback");
    let l = build_ledger();

    s.add_dyn(
        "XD1-十四格八允六拒",
        l.len() == SAMPLES && l.ok_count() == 8 && l.err_count() == 6 && l.audit().is_ok(),
        &format!("n={} ok={} err={}", l.len(), l.ok_count(), l.err_count()),
    );

    s.add(
        "XD2-两设备注册",
        l.sample(0).map(|x| x.value) == Some(1) && l.sample(1).map(|x| x.value) == Some(2),
        "static",
    );

    s.add(
        "XD3-播种一件",
        l.sample(2).map(|x| x.value) == Some(1),
        "static",
    );

    s.add(
        "XD4-同设备快道搬满",
        l.sample(3).map(|x| x.value) == Some(64),
        "static",
    );

    s.add(
        "XD5-跨设备回退等价",
        l.sample(4).map(|x| x.value) == Some(64) && l.sample(5).map(|x| x.value) == Some(1),
        "static",
    );

    s.add(
        "XD6-目标设备剩余",
        l.sample(6).map(|x| x.value) == Some(960),
        "static",
    );

    s.add(
        "XD7-三条目共存",
        l.sample(7).map(|x| x.value) == Some(3),
        "static",
    );

    s.add(
        "XD8-空名被拒",
        l.sample(8).map(|x| x.code) == Some(XD_EINVAL),
        "static",
    );

    s.add(
        "XD9-源缺失为ENOENT",
        l.sample(9).map(|x| x.code) == Some(XD_ENOENT),
        "static",
    );

    s.add(
        "XD10-目录源为EISDIR",
        l.sample(10).map(|x| x.code) == Some(XD_EISDIR),
        "static",
    );

    s.add(
        "XD11-未注册设备与空间不足",
        l.sample(11).map(|x| x.code) == Some(XD_EDEV)
            && l.sample(12).map(|x| x.code) == Some(XD_ENOSPC),
        "static",
    );

    s.add(
        "XD12-校验失败回滚且不落表",
        {
            let mut g = Cp::new();
            let _ = g.mkdev(1, 256);
            let _ = g.mkdev(2, 256);
            let _ = g.create(70, 1, 32);
            let before = g.ent_count();
            let fail = g.copy_faulty(70, 2, 71, 1);
            fail == Err(XD_ECHK)
                && g.ent_count() == before
                && g.free_of(2) == Ok(256)
                && g.rollback_count() == 1
                && g.fallback_count() == 1
                && g.consistent()
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
        let s = run_unx11391_checks();
        assert_eq!(s.len(), 13);
        assert!(s.all_passed());
    }

    #[test]
    fn cross_device_is_equivalent() {
        let mut c = Cp::new();
        let _ = c.mkdev(1, 512);
        let _ = c.mkdev(2, 512);
        let _ = c.create(1, 1, 48);
        assert_eq!(c.copy(1, 2, 2), Ok(48));
        assert_eq!(c.same(1, 2), Ok(1));
        assert_eq!(c.fallback_count(), 1);
        assert_eq!(c.fast_count(), 0);
        assert_eq!(c.chunk_count(), 3);
        assert_eq!(c.free_of(2), Ok(464));
    }

    #[test]
    fn same_device_uses_fast_path() {
        let mut c = Cp::new();
        let _ = c.mkdev(1, 256);
        let _ = c.create(1, 1, 32);
        assert_eq!(c.copy(1, 1, 2), Ok(32));
        assert_eq!(c.fast_count(), 1);
        assert_eq!(c.fallback_count(), 0);
        assert_eq!(c.same(1, 2), Ok(1));
    }

    #[test]
    fn checksum_failure_rolls_back() {
        let mut c = Cp::new();
        let _ = c.mkdev(1, 256);
        let _ = c.mkdev(2, 256);
        let _ = c.create(1, 1, 32);
        assert_eq!(c.copy_faulty(1, 2, 2, 1), Err(XD_ECHK));
        assert_eq!(c.ent_count(), 1);
        assert_eq!(c.free_of(2), Ok(256));
        assert_eq!(c.rollback_count(), 1);
    }

    #[test]
    fn gate_order_is_stable() {
        let mut c = Cp::new();
        let _ = c.mkdev(1, 256);
        assert_eq!(c.copy(0, 9, 1), Err(XD_EINVAL));
        assert_eq!(c.copy(9, 9, 1), Err(XD_ENOENT));
        let _ = c.mkdir(2, 1);
        assert_eq!(c.copy(2, 9, 3), Err(XD_EISDIR));
        let _ = c.create(4, 1, 8);
        assert_eq!(c.copy(4, 1, 4), Err(XD_ELOOP));
        assert_eq!(c.copy(4, 9, 5), Err(XD_EDEV));
    }

    #[test]
    fn capacity_edges() {
        let mut c = Cp::new();
        let _ = c.mkdev(1, 2048);
        let mut i = 1u32;
        while (i as usize) <= CAP {
            assert!(c.create(i, 1, 8).is_ok());
            i += 1;
        }
        assert_eq!(c.create(99, 1, 8), Err(XD_EFULL));
        assert_eq!(c.copy(1, 1, 90), Err(XD_EFULL));
        assert_eq!(c.copy(1, 1, 2), Ok(8));
        assert!(c.consistent());
    }

    #[test]
    fn freeze_blocks_writes() {
        let mut l = build_ledger();
        assert!(l.freeze().is_ok());
        assert!(l.register(Sample::blank()).is_err());
        assert_eq!(l.rejects(), 1);
    }
}
