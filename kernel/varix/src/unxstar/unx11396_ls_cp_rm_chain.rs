//! UNX-F11396 ls/cp/rm 组合探针（0xeec0 段）
//!
//! 三件工具串成一条流水时，正确性不取决于任何单件，而取决于**清单的时间点**：
//!
//! - `ls` 先给出一份清单，它记的是「此刻目录里有哪些名字」；
//! - `cp` 按这份清单逐件复制，复制品出现在目标目录；
//! - `rm` 再按同一份清单逐件删除原件；
//! - 最后 `ls` 的清单必须恰好等于「初态 − 原件 + 复制品」。
//!
//! 这里最容易出错的是拿一份**过期清单**去驱动后续动作。目录是活的：一次删除
//! 会让目录项的槽位空出来，一次新建可能立刻复用那个槽位。若快照只记「名字」，
//! 复用后名字对上、槽位却换了人，删除就会删错对象。内核里 `readdir` 的 cookie
//! 失效问题与此同源。本模块让快照记住 (槽位, 名字) 对，并在比对时校验槽位里
//! 的名字是否仍是快照记的那个——一旦被复用，立刻判 `ESTALE`，绝不「尽力而为」
//! 地给出一个看似合理的差集。
//!
//! 于是 `diff` 有三条互斥的结果：
//!
//! - 快照下标非法 → `EINVAL`；
//! - 某个槽位的名字已变（被复用）→ `ESTALE`，清单不可再用；
//! - 全部槽位仍指向快照记的名字或已空 → 返回「快照里有、现在没有」的条数。
//!
//! 其余错误类：`ENOENT` 是源/目标不存在；`EDUP` 是目标名已存在（cp 不覆盖时）；
//! `EBUSY` 是源被挂载占住；`EROFS` 是只读；`EFULL` 是条目表满；`ENOSPC` 是
//! 复制预算（目标空间）不足。它们彼此不可合并：`ENOSPC` 与 `EFULL` 都发生在
//! 写入路径上，但一个删文件就能继续，另一个必须扩表。
//!
//! 闸门次序（`copy`）：形状 → 源存在 → 目标重名 → 源忙 → 只读 → 表满 → 预算。

use crate::alloc::format;
use crate::checks::CheckSet;

/// 段基址。
pub const SEG_BASE: u32 = 0xeec0;

/// 形状错或快照下标非法。
pub const LC_EINVAL: u32 = SEG_BASE + 0x01;
/// 源或目标不存在。
pub const LC_ENOENT: u32 = SEG_BASE + 0x02;
/// 目标名已存在。
pub const LC_EDUP: u32 = SEG_BASE + 0x03;
/// 复制预算不足。
pub const LC_ENOSPC: u32 = SEG_BASE + 0x04;
/// 快照槽位已被复用。
pub const LC_ESTALE: u32 = SEG_BASE + 0x05;
/// 源被挂载点占住。
pub const LC_EBUSY: u32 = SEG_BASE + 0x06;
/// 只读文件系统。
pub const LC_EROFS: u32 = SEG_BASE + 0x07;
/// 条目表或快照表满。
pub const LC_EFULL: u32 = SEG_BASE + 0x08;
/// 账本形状不符。
pub const LC_EBAL: u32 = SEG_BASE + 0x09;
/// 槽位乱序。
pub const LC_ESLOT: u32 = SEG_BASE + 0x0a;
/// 观测值与复算不符。
pub const LC_EVALUE: u32 = SEG_BASE + 0x0b;
/// 冻结后重放不一致。
pub const LC_ESEAL: u32 = SEG_BASE + 0x0c;
/// 观察下标越界。
pub const LC_ENOBS: u32 = SEG_BASE + 0x0d;

/// 条目表容量（即 `CAP`）。
pub const CAP: usize = 8;
/// 快照表容量。
pub const SCAP: usize = 3;
/// 账本格数。
pub const SAMPLES: usize = 14;

/// 操作码：播种。
pub const OP_ADD: u8 = 1;
/// 操作码：列清单。
pub const OP_LIST: u8 = 2;
/// 操作码：复制。
pub const OP_COPY: u8 = 3;
/// 操作码：删除。
pub const OP_REMOVE: u8 = 4;
/// 操作码：比对差集。
pub const OP_DIFF: u8 = 5;
/// 操作码：读活条目数。
pub const OP_STAT: u8 = 6;

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

/// 一个目录项。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Ent {
    /// 名字。
    pub name: u32,
    /// 字节数。
    pub size: u32,
    /// 是否挂载点。
    pub busy: bool,
    /// 是否只读。
    pub ro: bool,
    /// 是否存活。
    pub live: bool,
}

impl Ent {
    /// 空槽。
    pub const fn blank() -> Ent {
        Ent { name: 0, size: 0, busy: false, ro: false, live: false }
    }
}

/// 一份 (槽位, 名字) 快照。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Snap {
    /// 槽位号。
    pub slot: [u8; CAP],
    /// 槽位当时的名字。
    pub name: [u32; CAP],
    /// 记录条数。
    pub n: usize,
    /// 建立时的变更代数。
    pub epoch: u32,
    /// 槽位是否被使用。
    pub live: bool,
}

impl Snap {
    /// 空快照。
    pub const fn blank() -> Snap {
        Snap { slot: [0u8; CAP], name: [0u32; CAP], n: 0, epoch: 0, live: false }
    }
}

/// 三工具流水模型。
#[derive(Clone, Copy, Debug)]
pub struct Chain {
    e: [Ent; CAP],
    s: [Snap; SCAP],
    budget: u32,
    used: u32,
    epoch: u32,
    lists: u32,
    copies: u32,
    removes: u32,
}

impl Chain {
    /// 空模型，默认预算 64 字节。
    pub const fn new() -> Chain {
        Chain {
            e: [Ent::blank(); CAP],
            s: [Snap::blank(); SCAP],
            budget: 64,
            used: 0,
            epoch: 0,
            lists: 0,
            copies: 0,
            removes: 0,
        }
    }

    /// 指定复制预算。
    pub fn with_budget(budget: u32) -> Chain {
        let mut c = Chain::new();
        c.budget = budget;
        c
    }

    /// 活条目数。
    pub fn live(&self) -> usize {
        let mut k = 0usize;
        let mut n = 0usize;
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

    /// 已用字节。
    pub fn used(&self) -> u32 {
        self.used
    }

    /// 变更代数。
    pub fn epoch(&self) -> u32 {
        self.epoch
    }

    /// 列清单次数。
    pub fn list_count(&self) -> u32 {
        self.lists
    }

    /// 复制次数。
    pub fn copy_count(&self) -> u32 {
        self.copies
    }

    /// 删除次数。
    pub fn remove_count(&self) -> u32 {
        self.removes
    }

    /// 第 k 个槽。
    pub fn ent_at(&self, k: usize) -> Option<Ent> {
        self.e.get(k).copied()
    }

    /// 第 k 份快照。
    pub fn snap_at(&self, k: usize) -> Option<Snap> {
        self.s.get(k).copied()
    }

    /// 按名找活条目。
    fn find(&self, name: u32) -> Option<usize> {
        let mut k = 0usize;
        while k < CAP {
            match self.e.get(k) {
                Some(x) => {
                    if x.live && x.name == name {
                        return Some(k);
                    }
                }
                None => return None,
            }
            k += 1;
        }
        None
    }

    /// 空条目槽。
    fn free(&self) -> Option<usize> {
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

    /// 空快照槽。
    fn free_snap(&self) -> Option<usize> {
        let mut k = 0usize;
        while k < SCAP {
            match self.s.get(k) {
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

    /// 播种。
    pub fn add(&mut self, name: u32, size: u32) -> Result<usize, u32> {
        if name == 0 || size == 0 {
            return Err(LC_EINVAL);
        }
        if self.find(name).is_some() {
            return Err(LC_EDUP);
        }
        if self.used.saturating_add(size) > self.budget {
            return Err(LC_ENOSPC);
        }
        let slot = match self.free() {
            Some(v) => v,
            None => return Err(LC_EFULL),
        };
        if let Some(x) = self.e.get_mut(slot) {
            *x = Ent { name, size, busy: false, ro: false, live: true };
        }
        self.used = self.used.saturating_add(size);
        self.epoch = self.epoch.saturating_add(1);
        Ok(self.live())
    }

    /// 标记挂载点。
    pub fn mark_busy(&mut self, name: u32) -> Result<usize, u32> {
        match self.find(name) {
            Some(k) => {
                if let Some(x) = self.e.get_mut(k) {
                    x.busy = true;
                }
                Ok(k + 1)
            }
            None => Err(LC_ENOENT),
        }
    }

    /// 标记单条只读。
    pub fn mark_ro(&mut self, name: u32) -> Result<usize, u32> {
        match self.find(name) {
            Some(k) => {
                if let Some(x) = self.e.get_mut(k) {
                    x.ro = true;
                }
                Ok(k + 1)
            }
            None => Err(LC_ENOENT),
        }
    }

    /// 列清单：记录 (槽位, 名字) 快照，返回条数。
    pub fn list(&mut self) -> Result<usize, u32> {
        let slot = match self.free_snap() {
            Some(v) => v,
            None => return Err(LC_EFULL),
        };
        let mut snap = Snap { slot: [0u8; CAP], name: [0u32; CAP], n: 0, epoch: self.epoch, live: true };
        let mut k = 0usize;
        while k < CAP {
            if let Some(x) = self.e.get(k) {
                if x.live {
                    if let Some(dst) = snap.slot.get_mut(snap.n) {
                        *dst = k as u8;
                    }
                    if let Some(dst) = snap.name.get_mut(snap.n) {
                        *dst = x.name;
                    }
                    snap.n += 1;
                }
            }
            k += 1;
        }
        if let Some(dst) = self.s.get_mut(slot) {
            *dst = snap;
        }
        self.lists = self.lists.saturating_add(1);
        Ok(self.live())
    }

    /// 复制。闸门次序见模块文档。
    pub fn copy(&mut self, src: u32, dst: u32) -> Result<usize, u32> {
        if src == 0 || dst == 0 {
            return Err(LC_EINVAL);
        }
        let si = match self.find(src) {
            Some(v) => v,
            None => return Err(LC_ENOENT),
        };
        let se = match self.e.get(si) {
            Some(v) => *v,
            None => return Err(LC_ENOENT),
        };
        if self.find(dst).is_some() {
            return Err(LC_EDUP);
        }
        if se.busy {
            return Err(LC_EBUSY);
        }
        if se.ro {
            return Err(LC_EROFS);
        }
        let slot = match self.free() {
            Some(v) => v,
            None => return Err(LC_EFULL),
        };
        if self.used.saturating_add(se.size) > self.budget {
            return Err(LC_ENOSPC);
        }
        if let Some(x) = self.e.get_mut(slot) {
            *x = Ent { name: dst, size: se.size, busy: false, ro: false, live: true };
        }
        self.used = self.used.saturating_add(se.size);
        self.epoch = self.epoch.saturating_add(1);
        self.copies = self.copies.saturating_add(1);
        Ok(self.live())
    }

    /// 删除。
    pub fn remove(&mut self, name: u32) -> Result<usize, u32> {
        if name == 0 {
            return Err(LC_EINVAL);
        }
        let k = match self.find(name) {
            Some(v) => v,
            None => return Err(LC_ENOENT),
        };
        let ent = match self.e.get(k) {
            Some(v) => *v,
            None => return Err(LC_ENOENT),
        };
        if ent.busy {
            return Err(LC_EBUSY);
        }
        if ent.ro {
            return Err(LC_EROFS);
        }
        if let Some(x) = self.e.get_mut(k) {
            x.live = false;
        }
        self.used = self.used.saturating_sub(ent.size);
        self.epoch = self.epoch.saturating_add(1);
        self.removes = self.removes.saturating_add(1);
        Ok(self.live())
    }

    /// 比对第 i 份快照与当前目录，返回「快照有、现在没有」的条数。
    pub fn diff(&self, i: usize) -> Result<usize, u32> {
        if i >= SCAP {
            return Err(LC_EINVAL);
        }
        let snap = match self.s.get(i) {
            Some(v) => *v,
            None => return Err(LC_EINVAL),
        };
        if !snap.live {
            return Err(LC_EINVAL);
        }
        let mut gone = 0usize;
        let mut k = 0usize;
        while k < snap.n {
            let slot = match snap.slot.get(k) {
                Some(v) => *v as usize,
                None => return Err(LC_ESTALE),
            };
            let name = match snap.name.get(k) {
                Some(v) => *v,
                None => return Err(LC_ESTALE),
            };
            let cur = match self.e.get(slot) {
                Some(v) => *v,
                None => return Err(LC_ESTALE),
            };
            if cur.live {
                if cur.name != name {
                    return Err(LC_ESTALE);
                }
            } else {
                gone += 1;
            }
            k += 1;
        }
        Ok(gone)
    }

    /// 不变式：活条目名唯一，用量与条目之和一致，快照条数不超过 CAP。
    pub fn consistent(&self) -> bool {
        let mut sum = 0u32;
        let mut k = 0usize;
        while k < CAP {
            let x = match self.e.get(k) {
                Some(v) => *v,
                None => return false,
            };
            if x.live {
                if x.name == 0 || x.size == 0 {
                    return false;
                }
                sum = sum.saturating_add(x.size);
                let mut j = k + 1;
                while j < CAP {
                    match self.e.get(j) {
                        Some(o) => {
                            if o.live && o.name == x.name {
                                return false;
                            }
                        }
                        None => return false,
                    }
                    j += 1;
                }
            }
            k += 1;
        }
        if sum != self.used {
            return false;
        }
        let mut s = 0usize;
        while s < SCAP {
            match self.s.get(s) {
                Some(x) => {
                    if x.live && (x.n > CAP || x.epoch > self.epoch) {
                        return false;
                    }
                }
                None => return false,
            }
            s += 1;
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
            return Err(LC_ESLOT);
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
            return Err(LC_EBAL);
        }
        const VALS: [u32; 8] = [1, 2, 2, 3, 4, 3, 2, 2];
        const CODES: [u32; 6] =
            [LC_EINVAL, LC_ENOENT, LC_EDUP, LC_ENOSPC, LC_ESTALE, LC_EBUSY];
        let mut vi = 0usize;
        let mut ci = 0usize;
        let mut i = 0usize;
        while i < self.len {
            let s = match self.rows.get(i) {
                Some(v) => *v,
                None => return Err(LC_EBAL),
            };
            if s.slot as usize != i {
                return Err(LC_ESLOT);
            }
            if s.ok {
                if s.code != 0 {
                    return Err(LC_EVALUE);
                }
                match VALS.get(vi) {
                    Some(w) => {
                        if s.value != *w {
                            return Err(LC_EVALUE);
                        }
                    }
                    None => return Err(LC_EBAL),
                }
                vi += 1;
            } else {
                match CODES.get(ci) {
                    Some(w) => {
                        if s.code != *w {
                            return Err(LC_EVALUE);
                        }
                    }
                    None => return Err(LC_EBAL),
                }
                ci += 1;
            }
            i += 1;
        }
        if vi != 8 || ci != 6 {
            return Err(LC_EBAL);
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
            return Err(LC_EBAL);
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
            Err(LC_ESEAL)
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

/// 十四格账本：两件播种 → 列清单 → 复制两件 → 删除原件 → 差集 → 六类拒绝。
pub fn build_ledger() -> Ledger {
    let mut l = Ledger::new();
    let mut c = Chain::new();

    let _ = l.register(cell(0, OP_ADD, c.add(10, 16)));
    let _ = l.register(cell(1, OP_ADD, c.add(20, 16)));
    let _ = l.register(cell(2, OP_LIST, c.list()));
    let _ = l.register(cell(3, OP_COPY, c.copy(10, 30)));
    let _ = l.register(cell(4, OP_COPY, c.copy(20, 40)));
    let _ = l.register(cell(5, OP_REMOVE, c.remove(10)));
    let _ = l.register(cell(6, OP_REMOVE, c.remove(20)));
    let _ = l.register(cell(7, OP_DIFF, c.diff(0)));

    let mut a = Chain::new();
    let _ = l.register(cell(8, OP_ADD, a.add(0, 8)));

    let mut b = Chain::new();
    let _ = l.register(cell(9, OP_COPY, b.copy(99, 31)));

    let mut d = Chain::new();
    let _ = d.add(10, 16);
    let _ = d.copy(10, 30);
    let _ = l.register(cell(10, OP_COPY, d.copy(10, 30)));

    let mut e = Chain::with_budget(24);
    let _ = e.add(50, 16);
    let _ = l.register(cell(11, OP_COPY, e.copy(50, 51)));

    let mut f = Chain::new();
    let _ = f.add(60, 8);
    let _ = f.list();
    let _ = f.remove(60);
    let _ = f.add(70, 8);
    let _ = l.register(cell(12, OP_DIFF, f.diff(0)));

    let mut g = Chain::new();
    let _ = g.add(80, 8);
    let _ = g.mark_busy(80);
    let _ = l.register(cell(13, OP_COPY, g.copy(80, 81)));

    l
}

/// 四路注入。
pub fn inject_four() -> [bool; 4] {
    let mut out = [false; 4];

    let l = build_ledger();
    let rep = l.replay();
    let root = l.seal();
    out[0] = l.len() == SAMPLES && l.audit().is_ok() && rep == Ok(root) && root != 0;

    // 一路：列举→复制→删除链，差集等于原件数。
    let mut a = Chain::new();
    let _ = a.add(1, 8);
    let _ = a.add(2, 8);
    let _ = a.add(3, 8);
    let before = a.list();
    let _ = a.copy(1, 11);
    let _ = a.copy(2, 12);
    let _ = a.copy(3, 13);
    let _ = a.remove(1);
    let _ = a.remove(2);
    let _ = a.remove(3);
    let gone = a.diff(0);
    out[1] = before == Ok(3)
        && gone == Ok(3)
        && a.live() == 3
        && a.copy_count() == 3
        && a.remove_count() == 3
        && a.used() == 24
        && a.consistent();

    // 二路：五类失败彼此可辨。
    let mut b = Chain::new();
    let _ = b.add(20, 8);
    let _ = b.mark_busy(20);
    let _ = b.add(21, 8);
    let _ = b.mark_ro(21);
    let missing = b.copy(99, 1) == Err(LC_ENOENT);
    let dup = b.copy(20, 21) == Err(LC_EDUP);
    let busy = b.copy(20, 22) == Err(LC_EBUSY);
    let ro = b.copy(21, 22) == Err(LC_EROFS);
    let mut small = Chain::with_budget(20);
    let _ = small.add(1, 16);
    let nospc = small.copy(1, 2) == Err(LC_ENOSPC);
    let mut full = Chain::new();
    let mut i = 1u32;
    while (full.live() as usize) < CAP {
        let _ = full.add(i, 1);
        i += 1;
    }
    let ef = full.add(99, 1) == Err(LC_EFULL);
    out[2] = missing && dup && busy && ro && nospc && ef && b.consistent() && full.consistent();

    // 三路：闸门次序、快照失效与容量。
    let mut q = Chain::new();
    let _ = q.add(1, 8);
    let g1 = q.copy(0, 2) == Err(LC_EINVAL);
    let g2 = q.copy(99, 2) == Err(LC_ENOENT);
    let g3 = q.copy(1, 1) == Err(LC_EDUP);
    let _ = q.list();
    let _ = q.remove(1);
    let _ = q.add(2, 8);
    let g4 = q.diff(0) == Err(LC_ESTALE);
    let g5 = q.diff(SCAP) == Err(LC_EINVAL);
    let g6 = q.consistent();
    out[3] = g1 && g2 && g3 && g4 && g5 && g6;

    out
}

/// 十三项判据。
pub fn run_unx11396_checks() -> CheckSet {
    let mut s = CheckSet::new("unx11396.ls_cp_rm_chain");
    let l = build_ledger();

    s.add_dyn(
        "LC1-十四格八允六拒",
        l.len() == SAMPLES && l.ok_count() == 8 && l.err_count() == 6 && l.audit().is_ok(),
        &format!("n={} ok={} err={}", l.len(), l.ok_count(), l.err_count()),
    );

    s.add(
        "LC2-两件播种",
        l.sample(0).map(|x| x.value) == Some(1) && l.sample(1).map(|x| x.value) == Some(2),
        "static",
    );

    s.add(
        "LC3-清单两条",
        l.sample(2).map(|x| x.value) == Some(2),
        "static",
    );

    s.add(
        "LC4-复制两件",
        l.sample(3).map(|x| x.value) == Some(3) && l.sample(4).map(|x| x.value) == Some(4),
        "static",
    );

    s.add(
        "LC5-删除原件",
        l.sample(5).map(|x| x.value) == Some(3) && l.sample(6).map(|x| x.value) == Some(2),
        "static",
    );

    s.add(
        "LC6-差集等于两条",
        l.sample(7).map(|x| x.value) == Some(2),
        "static",
    );

    s.add(
        "LC7-空名被拒",
        l.sample(8).map(|x| x.code) == Some(LC_EINVAL),
        "static",
    );

    s.add(
        "LC8-源缺失为ENOENT",
        l.sample(9).map(|x| x.code) == Some(LC_ENOENT),
        "static",
    );

    s.add(
        "LC9-目标重名为EDUP",
        l.sample(10).map(|x| x.code) == Some(LC_EDUP),
        "static",
    );

    s.add(
        "LC10-预算不足为ENOSPC",
        l.sample(11).map(|x| x.code) == Some(LC_ENOSPC),
        "static",
    );

    s.add(
        "LC11-槽位复用即STALE",
        l.sample(12).map(|x| x.code) == Some(LC_ESTALE)
            && l.sample(13).map(|x| x.code) == Some(LC_EBUSY),
        "static",
    );

    s.add(
        "LC12-链后清单为差集",
        {
            let mut c = Chain::new();
            let _ = c.add(4, 8);
            let _ = c.add(5, 8);
            let _ = c.list();
            let _ = c.copy(4, 14);
            let _ = c.copy(5, 15);
            let _ = c.remove(4);
            let _ = c.remove(5);
            let gone = c.diff(0);
            let still = c.list();
            gone == Ok(2)
                && still == Ok(2)
                && c.ent_at(2).map(|x| x.name) == Some(14)
                && c.ent_at(3).map(|x| x.name) == Some(15)
                && c.consistent()
        },
        "static",
    );

    s.add(
        "LC13-冻结重放四注入",
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
        let s = run_unx11396_checks();
        assert_eq!(s.len(), 13);
        assert!(s.all_passed());
    }

    #[test]
    fn chain_yields_difference() {
        let mut c = Chain::new();
        let _ = c.add(1, 8);
        let _ = c.add(2, 8);
        assert_eq!(c.list(), Ok(2));
        assert_eq!(c.copy(1, 11), Ok(3));
        assert_eq!(c.remove(1), Ok(2));
        assert_eq!(c.diff(0), Ok(1));
        assert_eq!(c.live(), 2);
    }

    #[test]
    fn reused_slot_is_stale() {
        let mut c = Chain::new();
        let _ = c.add(1, 8);
        let _ = c.list();
        let _ = c.remove(1);
        let _ = c.add(2, 8);
        assert_eq!(c.diff(0), Err(LC_ESTALE));
    }

    #[test]
    fn snapshots_are_bounded() {
        let mut c = Chain::new();
        let mut i = 0usize;
        while i < SCAP {
            assert!(c.list().is_ok());
            i += 1;
        }
        assert_eq!(c.list(), Err(LC_EFULL));
    }

    #[test]
    fn gate_order_is_stable() {
        let mut c = Chain::new();
        let _ = c.add(1, 8);
        let _ = c.mark_busy(1);
        assert_eq!(c.copy(0, 2), Err(LC_EINVAL));
        assert_eq!(c.copy(9, 2), Err(LC_ENOENT));
        assert_eq!(c.copy(1, 2), Err(LC_EBUSY));
        assert_eq!(c.remove(1), Err(LC_EBUSY));
    }

    #[test]
    fn budget_and_usage_agree() {
        let mut c = Chain::with_budget(20);
        assert_eq!(c.add(1, 12), Ok(1));
        assert_eq!(c.copy(1, 2), Err(LC_ENOSPC));
        assert_eq!(c.used(), 12);
        assert_eq!(c.remove(1), Ok(0));
        assert_eq!(c.used(), 0);
        assert!(c.consistent());
    }
}
