//! UNX-F11546 sync 缓冲冲刷
//!
//! `sync` 家族的语义核心不是「写文件」，而是「把已标脏的页与元数据交给下层」。
//! 一次冲刷有两个可观测量：脏页的**数量**与元数据的**代次**。本模块把两者分开
//! 记账，因为 `fdatasync` 只保证数据落盘、不保证元数据落盘——若把两者混成一个
//! 计数器，`fdatasync` 与 `fsync` 就再也分不出来了。
//!
//! 每个卷处于四种状态之一：
//!
//! - `CLEAN`：无脏页，可接受冲刷；
//! - `DIRTY`：有脏页，冲刷后转 `CLEAN`；
//! - `ERROR`：下层报错。**任何**覆盖它的冲刷请求整体失败，且不留半程效果——
//!   全部卷要么一起冲刷，要么一个都不动；
//! - `RO`：只读卷。带脏页时冲刷是矛盾的（数据无处可写），必须拒绝；无脏页时
//!   是空操作。
//!
//! 原子性是本模块的第一约束。多卷冲刷先做一遍全量检查，检查通过才落账；否则
//! 中途失败会留下「前三个卷已刷、后两个没刷」的不一致状态，而这种状态没有任何
//! 外部手段可以复原。
//!
//! 冲刷顺序必须按卷序号递增。区间冲刷给了闭区间 `[from, to]`，`to < from` 是
//! 参数错误而不是空区间——静默接受会把「写反了」伪装成「无事发生」。
//!
//! 闸门次序：
//!
//! - `add`：名字非法 → 状态非法 → 重名 → 卷表满；
//! - `mark_dirty`：未知名 → 卷状态（只读/错误）→ 页数为零 → 计数溢出；
//! - `sync_one`：模式非法 → 未知名 → 卷状态；
//! - `sync_all` / `sync_range`：区间反序 → 端点非法 → 错误卷（整体拒绝）→
//!   只读卷带脏页。

use crate::checks::CheckSet;

/// 段基址。
pub const SEG_BASE: u32 = 0xf330;

/// 名字或页数非法。
pub const SY_EINVAL: u32 = SEG_BASE + 0x01;
/// 卷名重复。
pub const SY_EEXIST: u32 = SEG_BASE + 0x02;
/// 卷不存在。
pub const SY_ENOENT: u32 = SEG_BASE + 0x03;
/// 只读卷。
pub const SY_EROFS: u32 = SEG_BASE + 0x04;
/// 下层错误。
pub const SY_EBUSY: u32 = SEG_BASE + 0x05;
/// 卷表或脏页计数满。
pub const SY_EFULL: u32 = SEG_BASE + 0x06;
/// 区间反序。
pub const SY_EORDER: u32 = SEG_BASE + 0x07;
/// 模式非法。
pub const SY_EMODE: u32 = SEG_BASE + 0x08;
/// 账本形状不符。
pub const SY_EBAL: u32 = SEG_BASE + 0x09;
/// 槽位乱序。
pub const SY_ESLOT: u32 = SEG_BASE + 0x0a;
/// 观测值与复算不符。
pub const SY_EVALUE: u32 = SEG_BASE + 0x0b;
/// 冻结后重放不一致。
pub const SY_ESEAL: u32 = SEG_BASE + 0x0c;
/// 观察下标越界。
pub const SY_ENOBS: u32 = SEG_BASE + 0x0d;

/// 卷表容量。
pub const VOL_CAP: usize = 8;
/// 卷名上限。
pub const NAMEMAX: usize = 12;
/// 单次冲刷页数上限。
pub const PAGEMAX: u32 = 4096;
/// 账本格数。
pub const SAMPLES: usize = 14;

/// 卷状态：干净。
pub const VOL_CLEAN: u8 = 1;
/// 卷状态：脏。
pub const VOL_DIRTY: u8 = 2;
/// 卷状态：错误。
pub const VOL_ERROR: u8 = 3;
/// 卷状态：只读。
pub const VOL_RO: u8 = 4;

/// 冲刷模式：数据与元数据。
pub const MODE_FULL: u8 = 1;
/// 冲刷模式：仅数据。
pub const MODE_DATA: u8 = 2;

/// 操作码：登记卷。
pub const OP_ADD: u8 = 1;
/// 操作码：标脏。
pub const OP_DIRTY: u8 = 2;
/// 操作码：冲刷。
pub const OP_SYNC: u8 = 3;
/// 操作码：统计。
pub const OP_STAT: u8 = 4;

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

/// 一个卷。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Volume {
    name: [u8; NAMEMAX],
    nlen: u8,
    state: u8,
    dirty: u32,
    meta: u32,
    flushed: u32,
}

impl Volume {
    /// 空卷。
    pub const fn blank() -> Volume {
        Volume { name: [0; NAMEMAX], nlen: 0, state: VOL_CLEAN, dirty: 0, meta: 0, flushed: 0 }
    }

    /// 名字字节。
    pub fn name(&self) -> &[u8] {
        self.name.get(..self.nlen as usize).unwrap_or(&[])
    }

    /// 名字是否等于 `other`。
    pub fn name_is(&self, other: &[u8]) -> bool {
        self.name() == other
    }

    /// 状态。
    pub fn state(&self) -> u8 {
        self.state
    }

    /// 脏页数。
    pub fn dirty(&self) -> u32 {
        self.dirty
    }

    /// 元数据代次。
    pub fn meta(&self) -> u32 {
        self.meta
    }

    /// 已冲刷页数。
    pub fn flushed(&self) -> u32 {
        self.flushed
    }
}

/// 卷池。
#[derive(Clone, Copy, Debug)]
pub struct Pool {
    vols: [Volume; VOL_CAP],
    n: usize,
    flushes: u32,
    pages: u32,
    rejects: u32,
}

impl Pool {
    /// 空池。
    pub const fn new() -> Pool {
        Pool {
            vols: [Volume::blank(); VOL_CAP],
            n: 0,
            flushes: 0,
            pages: 0,
            rejects: 0,
        }
    }

    /// 卷数。
    pub fn len(&self) -> usize {
        self.n
    }

    /// 是否空池。
    pub fn is_empty(&self) -> bool {
        self.n == 0
    }

    /// 冲刷次数。
    pub fn flushes(&self) -> u32 {
        self.flushes
    }

    /// 累计冲刷页数。
    pub fn pages(&self) -> u32 {
        self.pages
    }

    /// 拒绝计数。
    pub fn rejects(&self) -> u32 {
        self.rejects
    }

    /// 第 i 个卷。
    pub fn vol(&self, i: usize) -> Option<Volume> {
        if i >= self.n {
            return None;
        }
        self.vols.get(i).copied()
    }

    /// 按名取下标。
    pub fn index_of(&self, name: &[u8]) -> Option<usize> {
        let mut i = 0usize;
        while i < self.n {
            if let Some(v) = self.vols.get(i) {
                if v.name() == name {
                    return Some(i);
                }
            }
            i += 1;
        }
        None
    }

    /// 登记卷。
    pub fn add(&mut self, name: &[u8], state: u8) -> Result<usize, u32> {
        if name.is_empty() || name.len() > NAMEMAX {
            self.rejects = self.rejects.saturating_add(1);
            return Err(SY_EINVAL);
        }
        let mut i = 0usize;
        while i < name.len() {
            match name.get(i) {
                Some(0x21..=0x7e) => {}
                _ => {
                    self.rejects = self.rejects.saturating_add(1);
                    return Err(SY_EINVAL);
                }
            }
            i += 1;
        }
        if state != VOL_CLEAN && state != VOL_DIRTY && state != VOL_ERROR && state != VOL_RO {
            self.rejects = self.rejects.saturating_add(1);
            return Err(SY_EMODE);
        }
        if self.index_of(name).is_some() {
            self.rejects = self.rejects.saturating_add(1);
            return Err(SY_EEXIST);
        }
        if self.n >= VOL_CAP {
            self.rejects = self.rejects.saturating_add(1);
            return Err(SY_EFULL);
        }
        let mut v = Volume::blank();
        let mut k = 0usize;
        while k < name.len() {
            if let (Some(dst), Some(src)) = (v.name.get_mut(k), name.get(k)) {
                *dst = *src;
            }
            k += 1;
        }
        v.nlen = name.len() as u8;
        v.state = state;
        if let Some(dst) = self.vols.get_mut(self.n) {
            *dst = v;
        }
        self.n += 1;
        Ok(self.n - 1)
    }

    /// 标脏。
    pub fn mark_dirty(&mut self, name: &[u8], pages: u32) -> Result<u32, u32> {
        let idx = match self.index_of(name) {
            Some(i) => i,
            None => {
                self.rejects = self.rejects.saturating_add(1);
                return Err(SY_ENOENT);
            }
        };
        let state = match self.vols.get(idx) {
            Some(v) => v.state,
            None => return Err(SY_ENOENT),
        };
        if state == VOL_RO {
            self.rejects = self.rejects.saturating_add(1);
            return Err(SY_EROFS);
        }
        if state == VOL_ERROR {
            self.rejects = self.rejects.saturating_add(1);
            return Err(SY_EBUSY);
        }
        if pages == 0 {
            self.rejects = self.rejects.saturating_add(1);
            return Err(SY_EINVAL);
        }
        let v = match self.vols.get_mut(idx) {
            Some(v) => v,
            None => return Err(SY_ENOENT),
        };
        let total = match v.dirty.checked_add(pages) {
            Some(t) => t,
            None => {
                self.rejects = self.rejects.saturating_add(1);
                return Err(SY_EFULL);
            }
        };
        if total > PAGEMAX {
            self.rejects = self.rejects.saturating_add(1);
            return Err(SY_EFULL);
        }
        v.dirty = total;
        v.meta = v.meta.saturating_add(1);
        v.state = VOL_DIRTY;
        Ok(v.dirty)
    }

    /// 冲刷单卷。
    pub fn sync_one(&mut self, name: &[u8], mode: u8) -> Result<u32, u32> {
        if mode != MODE_FULL && mode != MODE_DATA {
            self.rejects = self.rejects.saturating_add(1);
            return Err(SY_EMODE);
        }
        let idx = match self.index_of(name) {
            Some(i) => i,
            None => {
                self.rejects = self.rejects.saturating_add(1);
                return Err(SY_ENOENT);
            }
        };
        let state = match self.vols.get(idx) {
            Some(v) => v.state,
            None => return Err(SY_ENOENT),
        };
        if state == VOL_ERROR {
            self.rejects = self.rejects.saturating_add(1);
            return Err(SY_EBUSY);
        }
        if state == VOL_RO {
            let dirty = match self.vols.get(idx) {
                Some(v) => v.dirty,
                None => 0,
            };
            if dirty > 0 {
                self.rejects = self.rejects.saturating_add(1);
                return Err(SY_EROFS);
            }
            return Ok(0);
        }
        let v = match self.vols.get_mut(idx) {
            Some(v) => v,
            None => return Err(SY_ENOENT),
        };
        let moved = v.dirty;
        v.dirty = 0;
        if mode == MODE_FULL {
            v.meta = 0;
        }
        v.flushed = v.flushed.saturating_add(moved);
        if moved > 0 {
            v.state = VOL_CLEAN;
        }
        self.flushes = self.flushes.saturating_add(1);
        self.pages = self.pages.saturating_add(moved);
        Ok(moved)
    }

    /// 冲刷全部卷。
    pub fn sync_all(&mut self) -> Result<u32, u32> {
        let mut i = 0usize;
        while i < self.n {
            let v = match self.vols.get(i) {
                Some(v) => *v,
                None => return Err(SY_ENOENT),
            };
            if v.state == VOL_ERROR {
                self.rejects = self.rejects.saturating_add(1);
                return Err(SY_EBUSY);
            }
            if v.state == VOL_RO && v.dirty > 0 {
                self.rejects = self.rejects.saturating_add(1);
                return Err(SY_EROFS);
            }
            i += 1;
        }
        let mut moved = 0u32;
        let mut k = 0usize;
        while k < self.n {
            if let Some(v) = self.vols.get_mut(k) {
                let take = v.dirty;
                moved = moved.saturating_add(take);
                v.dirty = 0;
                v.meta = 0;
                v.flushed = v.flushed.saturating_add(take);
                if v.state == VOL_DIRTY {
                    v.state = VOL_CLEAN;
                }
            }
            k += 1;
        }
        if moved > 0 {
            self.flushes = self.flushes.saturating_add(1);
            self.pages = self.pages.saturating_add(moved);
        }
        Ok(moved)
    }

    /// 冲刷闭区间 `[from, to]`。
    pub fn sync_range(&mut self, from: usize, to: usize) -> Result<u32, u32> {
        if to < from {
            self.rejects = self.rejects.saturating_add(1);
            return Err(SY_EORDER);
        }
        if to >= self.n {
            self.rejects = self.rejects.saturating_add(1);
            return Err(SY_EINVAL);
        }
        let mut i = from;
        while i <= to {
            let v = match self.vols.get(i) {
                Some(v) => *v,
                None => return Err(SY_EINVAL),
            };
            if v.state == VOL_ERROR {
                self.rejects = self.rejects.saturating_add(1);
                return Err(SY_EBUSY);
            }
            if v.state == VOL_RO && v.dirty > 0 {
                self.rejects = self.rejects.saturating_add(1);
                return Err(SY_EROFS);
            }
            i += 1;
        }
        let mut moved = 0u32;
        let mut k = from;
        while k <= to {
            if let Some(v) = self.vols.get_mut(k) {
                let take = v.dirty;
                moved = moved.saturating_add(take);
                v.dirty = 0;
                v.meta = 0;
                v.flushed = v.flushed.saturating_add(take);
                if v.state == VOL_DIRTY {
                    v.state = VOL_CLEAN;
                }
            }
            k += 1;
        }
        if moved > 0 {
            self.flushes = self.flushes.saturating_add(1);
            self.pages = self.pages.saturating_add(moved);
        }
        Ok(moved)
    }

    /// 全池待写页数。
    pub fn pending(&self) -> u32 {
        let mut sum = 0u32;
        let mut i = 0usize;
        while i < self.n {
            if let Some(v) = self.vols.get(i) {
                sum = sum.saturating_add(v.dirty);
            }
            i += 1;
        }
        sum
    }

    /// 干净卷数。
    pub fn clean_count(&self) -> usize {
        let mut n = 0usize;
        let mut i = 0usize;
        while i < self.n {
            if let Some(v) = self.vols.get(i) {
                if v.state == VOL_CLEAN {
                    n += 1;
                }
            }
            i += 1;
        }
        n
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

/// `Result<u32, u32>` → 样本。
fn word_cell(slot: u8, op: u8, r: Result<u32, u32>) -> Sample {
    match r {
        Ok(v) => Sample { slot, op, ok: true, code: 0, value: v },
        Err(c) => Sample { slot, op, ok: false, code: c, value: 0 },
    }
}

/// `Result<usize, u32>` → 样本。
fn count_cell(slot: u8, op: u8, r: Result<usize, u32>) -> Sample {
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
    pub fn register(&mut self, s: Sample) -> Result<(), u32> {
        if self.frozen || (s.slot as usize) != self.len || (s.slot as usize) >= SAMPLES {
            self.rejects = self.rejects.saturating_add(1);
            return Err(SY_ESLOT);
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
            return Err(SY_EBAL);
        }
        const VALS: [u32; 8] = [3, 4, 2, 6, 4, 2, 2, 0];
        const CODES: [u32; 6] = [SY_EEXIST, SY_ENOENT, SY_EROFS, SY_EORDER, SY_EMODE, SY_EBUSY];
        let mut vi = 0usize;
        let mut ci = 0usize;
        let mut i = 0usize;
        while i < self.len {
            let s = match self.rows.get(i) {
                Some(v) => *v,
                None => return Err(SY_EBAL),
            };
            if s.slot as usize != i {
                return Err(SY_ESLOT);
            }
            if s.ok {
                if s.code != 0 {
                    return Err(SY_EVALUE);
                }
                match VALS.get(vi) {
                    Some(w) => {
                        if s.value != *w {
                            return Err(SY_EVALUE);
                        }
                    }
                    None => return Err(SY_EBAL),
                }
                vi += 1;
            } else {
                match CODES.get(ci) {
                    Some(w) => {
                        if s.code != *w {
                            return Err(SY_EVALUE);
                        }
                    }
                    None => return Err(SY_EBAL),
                }
                ci += 1;
            }
            i += 1;
        }
        if vi != 8 || ci != 6 {
            return Err(SY_EBAL);
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
            return Err(SY_EBAL);
        }
        self.root = self.seal();
        self.frozen = true;
        Ok(self.root)
    }

    /// 重放。
    pub fn replay(&self) -> Result<u64, u32> {
        let fresh = build_ledger();
        let r = fresh.seal();
        if r == self.seal() {
            Ok(r)
        } else {
            Err(SY_ESEAL)
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

/// 十四格账本：登记、标脏、单卷/全池冲刷与六类拒绝。
pub fn build_ledger() -> Ledger {
    let mut l = Ledger::new();
    let mut p = Pool::new();

    let _ = p.add(b"sda", VOL_CLEAN);
    let _ = p.add(b"sdb", VOL_CLEAN);
    let _ = p.add(b"sdc", VOL_RO);
    let _ = l.register(count_cell(0, OP_ADD, Ok(p.len())));

    let _ = l.register(word_cell(1, OP_DIRTY, p.mark_dirty(b"sda", 4)));
    let _ = l.register(word_cell(2, OP_DIRTY, p.mark_dirty(b"sdb", 2)));
    let _ = l.register(word_cell(3, OP_STAT, Ok(p.pending())));
    let _ = l.register(word_cell(4, OP_SYNC, p.sync_one(b"sda", MODE_FULL)));
    let _ = l.register(word_cell(5, OP_STAT, Ok(p.pending())));
    let _ = l.register(word_cell(6, OP_SYNC, p.sync_all()));
    let _ = l.register(word_cell(7, OP_STAT, Ok(p.pending())));

    let _ = l.register(word_cell(8, OP_ADD, p.add(b"sda", VOL_CLEAN).map(|v| v as u32)));
    let _ = l.register(word_cell(9, OP_DIRTY, p.mark_dirty(b"nope", 1)));
    let _ = l.register(word_cell(10, OP_DIRTY, p.mark_dirty(b"sdc", 1)));

    let mut q = Pool::new();
    let _ = q.add(b"vda", VOL_CLEAN);
    let _ = q.add(b"vdb", VOL_CLEAN);
    let _ = l.register(word_cell(11, OP_SYNC, q.sync_range(1, 0)));

    let _ = l.register(word_cell(12, OP_SYNC, q.sync_one(b"vda", 9)));

    let mut r = Pool::new();
    let _ = r.add(b"xda", VOL_ERROR);
    let _ = l.register(word_cell(13, OP_SYNC, r.sync_all()));

    l
}

/// 四路注入。
pub fn inject_four() -> [bool; 4] {
    let mut out = [false; 4];

    let l = build_ledger();
    let rep = l.replay();
    let root = l.seal();
    out[0] = l.len() == SAMPLES && l.audit().is_ok() && rep == Ok(root) && root != 0;

    // 一路：登记、标脏与待写页数。
    let mut p = Pool::new();
    let a = p.add(b"sda", VOL_CLEAN) == Ok(0);
    let b = p.add(b"sdb", VOL_CLEAN) == Ok(1);
    let c = p.add(b"vdc", VOL_RO) == Ok(2);
    let names = p.index_of(b"sda") == Some(0) && p.index_of(b"sdb") == Some(1);
    let d1 = p.mark_dirty(b"sda", 4) == Ok(4);
    let d2 = p.mark_dirty(b"sda", 2) == Ok(6);
    let d3 = p.mark_dirty(b"sdb", 3) == Ok(3);
    let pend = p.pending() == 9 && p.len() == 3 && p.clean_count() == 0;
    out[1] = a && b && c && names && d1 && d2 && d3 && pend;

    // 二路：单卷冲刷、仅数据模式与全池冲刷。
    let moved = p.sync_one(b"sda", MODE_DATA) == Ok(6);
    let keep_meta = p.vol(0).map(|v| v.meta()) == Some(2);
    let clean = p.vol(0).map(|v| v.state()) == Some(VOL_CLEAN);
    let rest = p.pending() == 3;

    let all = p.sync_all() == Ok(3) && p.pending() == 0;
    let range = {
        let mut q = Pool::new();
        let _ = q.add(b"vda", VOL_CLEAN);
        let _ = q.add(b"vdb", VOL_CLEAN);
        let _ = q.add(b"vdc", VOL_CLEAN);
        let _ = q.mark_dirty(b"vda", 1);
        let _ = q.mark_dirty(b"vdb", 2);
        let _ = q.mark_dirty(b"vdc", 4);
        q.sync_range(0, 1) == Ok(3) && q.pending() == 4 && q.sync_range(2, 2) == Ok(4)
    };
    out[2] = moved && keep_meta && clean && rest && all && range;

    // 三路：六类拒绝与冻结重放。
    let dup = p.add(b"sda", VOL_CLEAN) == Err(SY_EEXIST);
    let none = p.mark_dirty(b"ghost", 1) == Err(SY_ENOENT);
    let ro = p.mark_dirty(b"vdc", 1) == Err(SY_EROFS);
    let zero = p.mark_dirty(b"sda", 0) == Err(SY_EINVAL);
    let mode = p.sync_one(b"sda", 7) == Err(SY_EMODE);
    let order = p.sync_range(2, 1) == Err(SY_EORDER);
    let far = p.sync_range(0, 9) == Err(SY_EINVAL);

    let mut err = Pool::new();
    let _ = err.add(b"xda", VOL_ERROR);
    let atomic = err.sync_all() == Err(SY_EBUSY) && err.sync_one(b"xda", MODE_FULL) == Err(SY_EBUSY);

    let mut led = build_ledger();
    let one = led.freeze();
    let rep = led.replay();
    let two = led.freeze();
    let reg = led.register(Sample::blank());
    out[3] = dup
        && none
        && ro
        && zero
        && mode
        && order
        && far
        && atomic
        && one.is_ok()
        && rep == one
        && two.is_err()
        && reg == Err(SY_ESLOT)
        && led.is_frozen()
        && led.rejects() == 2;

    out
}

/// 十三项判据。
pub fn run_unx11546_checks() -> CheckSet {
    let mut s = CheckSet::new("unx11546.sync_flush");

    s.add(
        "SY01-登记与查找",
        {
            let mut p = Pool::new();
            p.add(b"sda", VOL_CLEAN) == Ok(0)
                && p.add(b"sdb", VOL_DIRTY) == Ok(1)
                && p.len() == 2
                && p.index_of(b"sdb") == Some(1)
                && p.index_of(b"sdz").is_none()
                && p.vol(1).map(|v| v.name_is(b"sdb")) == Some(true)
                && p.vol(2).is_none()
                && !p.is_empty()
        },
        "static",
    );

    s.add(
        "SY02-标脏累计",
        {
            let mut p = Pool::new();
            let _ = p.add(b"sda", VOL_CLEAN);
            let a = p.mark_dirty(b"sda", 4) == Ok(4);
            let b = p.mark_dirty(b"sda", 2) == Ok(6);
            let st = p.vol(0).map(|v| (v.state(), v.dirty(), v.meta())) == Some((VOL_DIRTY, 6, 2));
            a && b && st && p.pending() == 6
        },
        "static",
    );

    s.add(
        "SY03-单卷冲刷",
        {
            let mut p = Pool::new();
            let _ = p.add(b"sda", VOL_CLEAN);
            let _ = p.add(b"sdb", VOL_CLEAN);
            let _ = p.mark_dirty(b"sda", 5);
            let _ = p.mark_dirty(b"sdb", 3);
            let m = p.sync_one(b"sda", MODE_FULL) == Ok(5);
            let st = p.vol(0).map(|v| (v.state(), v.dirty(), v.meta(), v.flushed()))
                == Some((VOL_CLEAN, 0, 0, 5));
            m && st && p.pending() == 3 && p.flushes() == 1 && p.pages() == 5
        },
        "static",
    );

    s.add(
        "SY04-仅数据冲刷保留元数据",
        {
            let mut p = Pool::new();
            let _ = p.add(b"sda", VOL_CLEAN);
            let _ = p.mark_dirty(b"sda", 3);
            let m = p.sync_one(b"sda", MODE_DATA) == Ok(3);
            let meta = p.vol(0).map(|v| v.meta()) == Some(1);
            m && meta && p.vol(0).map(|v| v.dirty()) == Some(0)
        },
        "static",
    );

    s.add(
        "SY05-全池冲刷",
        {
            let mut p = Pool::new();
            let _ = p.add(b"sda", VOL_CLEAN);
            let _ = p.add(b"sdb", VOL_CLEAN);
            let _ = p.add(b"sdc", VOL_CLEAN);
            let _ = p.mark_dirty(b"sda", 2);
            let _ = p.mark_dirty(b"sdb", 3);
            let m = p.sync_all() == Ok(5);
            m && p.pending() == 0
                && p.clean_count() == 3
                && p.flushes() == 1
                && p.pages() == 5
        },
        "static",
    );

    s.add(
        "SY06-区间冲刷",
        {
            let mut p = Pool::new();
            let _ = p.add(b"vda", VOL_CLEAN);
            let _ = p.add(b"vdb", VOL_CLEAN);
            let _ = p.add(b"vdc", VOL_CLEAN);
            let _ = p.mark_dirty(b"vda", 1);
            let _ = p.mark_dirty(b"vdb", 2);
            let _ = p.mark_dirty(b"vdc", 4);
            p.sync_range(0, 1) == Ok(3) && p.pending() == 4 && p.sync_range(2, 2) == Ok(4)
        },
        "static",
    );

    s.add(
        "SY07-重名与卷表满",
        {
            let mut p = Pool::new();
            let _ = p.add(b"sda", VOL_CLEAN);
            let dup = p.add(b"sda", VOL_DIRTY) == Err(SY_EEXIST);
            let names: [&[u8]; VOL_CAP] = [b"n0", b"n1", b"n2", b"n3", b"n4", b"n5", b"n6", b"n7"];
            let mut full = Pool::new();
            let mut i = 0usize;
            while i < VOL_CAP {
                if let Some(n) = names.get(i) {
                    let _ = full.add(n, VOL_CLEAN);
                }
                i += 1;
            }
            let cap = matches!(full.add(b"zz", VOL_CLEAN), Err(SY_EFULL));
            dup && cap && full.len() == VOL_CAP
        },
        "static",
    );

    s.add(
        "SY08-未知卷与非法参数",
        {
            let mut p = Pool::new();
            let _ = p.add(b"sda", VOL_CLEAN);
            p.mark_dirty(b"ghost", 1) == Err(SY_ENOENT)
                && p.sync_one(b"ghost", MODE_FULL) == Err(SY_ENOENT)
                && p.mark_dirty(b"sda", 0) == Err(SY_EINVAL)
                && p.sync_one(b"sda", 0) == Err(SY_EMODE)
                && p.add(b"", VOL_CLEAN) == Err(SY_EINVAL)
                && p.add(b"waytoolongname", VOL_CLEAN) == Err(SY_EINVAL)
                && p.add(b"ok", 9) == Err(SY_EMODE)
        },
        "static",
    );

    s.add(
        "SY09-只读卷",
        {
            let mut p = Pool::new();
            let _ = p.add(b"cd0", VOL_RO);
            let mark = p.mark_dirty(b"cd0", 1) == Err(SY_EROFS);
            let idle = p.sync_one(b"cd0", MODE_FULL) == Ok(0);
            let mut q = Pool::new();
            let _ = q.add(b"cd1", VOL_RO);
            let all = q.sync_all() == Ok(0);
            mark && idle && all && p.pending() == 0
        },
        "static",
    );

    s.add(
        "SY10-错误卷整体拒绝",
        {
            let mut err = Pool::new();
            let _ = err.add(b"xda", VOL_CLEAN);
            let _ = err.add(b"xdb", VOL_ERROR);
            let _ = err.mark_dirty(b"xda", 3);
            let all = err.sync_all() == Err(SY_EBUSY);
            let keep = err.pending() == 3;
            let one = err.sync_one(b"xdb", MODE_FULL) == Err(SY_EBUSY);
            let range = err.sync_range(0, 1) == Err(SY_EBUSY);
            all && keep && one && range && err.flushes() == 0
        },
        "static",
    );

    s.add(
        "SY11-账本审计",
        {
            let l = build_ledger();
            l.len() == SAMPLES
                && l.ok_count() == 8
                && l.err_count() == 6
                && l.audit().is_ok()
                && l.rejects() == 0
        },
        "static",
    );

    s.add(
        "SY12-冻结与重放",
        {
            let mut l = build_ledger();
            let a = l.freeze();
            let b = l.freeze();
            let c = l.register(Sample::blank());
            let sealed = l.seal();
            l.is_frozen()
                && a.is_ok()
                && b == Err(SY_EBAL)
                && c == Err(SY_ESLOT)
                && l.rejects() == 2
                && l.replay() == Err(SY_ESEAL)
                && sealed != 0
        },
        "static",
    );

    s.add(
        "SY13-冻结重放四注入",
        inject_four() == [true; 4] && build_ledger().audit().is_ok(),
        "static",
    );

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn register_and_find() {
        let mut p = Pool::new();
        assert_eq!(p.add(b"sda", VOL_CLEAN), Ok(0));
        assert_eq!(p.add(b"sdb", VOL_DIRTY), Ok(1));
        assert_eq!(p.index_of(b"sdb"), Some(1));
        assert_eq!(p.vol(1).map(|v| v.name_is(b"sdb")), Some(true));
        assert!(p.vol(5).is_none());
    }

    #[test]
    fn dirty_accounting() {
        let mut p = Pool::new();
        let _ = p.add(b"sda", VOL_CLEAN);
        assert_eq!(p.mark_dirty(b"sda", 4), Ok(4));
        assert_eq!(p.mark_dirty(b"sda", 2), Ok(6));
        assert_eq!(p.pending(), 6);
        assert_eq!(p.vol(0).map(|v| v.meta()), Some(2));
    }

    #[test]
    fn flush_modes() {
        let mut p = Pool::new();
        let _ = p.add(b"sda", VOL_CLEAN);
        let _ = p.mark_dirty(b"sda", 3);
        assert_eq!(p.sync_one(b"sda", MODE_DATA), Ok(3));
        assert_eq!(p.vol(0).map(|v| v.meta()), Some(1));
        let _ = p.mark_dirty(b"sda", 2);
        assert_eq!(p.sync_one(b"sda", MODE_FULL), Ok(2));
        assert_eq!(p.vol(0).map(|v| v.meta()), Some(0));
        assert_eq!(p.flushes(), 2);
    }

    #[test]
    fn all_or_nothing() {
        let mut err = Pool::new();
        let _ = err.add(b"xda", VOL_CLEAN);
        let _ = err.add(b"xdb", VOL_ERROR);
        let _ = err.mark_dirty(b"xda", 3);
        assert_eq!(err.sync_all(), Err(SY_EBUSY));
        assert_eq!(err.pending(), 3);
        assert_eq!(err.flushes(), 0);
    }

    #[test]
    fn rejections() {
        let mut p = Pool::new();
        let _ = p.add(b"sda", VOL_CLEAN);
        assert_eq!(p.add(b"sda", VOL_CLEAN), Err(SY_EEXIST));
        assert_eq!(p.add(b"", VOL_CLEAN), Err(SY_EINVAL));
        assert_eq!(p.mark_dirty(b"ghost", 1), Err(SY_ENOENT));
        assert_eq!(p.mark_dirty(b"sda", 0), Err(SY_EINVAL));
        assert_eq!(p.sync_one(b"sda", 5), Err(SY_EMODE));
        assert_eq!(p.sync_range(1, 0), Err(SY_EORDER));
        assert!(p.rejects() >= 6);
    }

    #[test]
    fn ledger_and_freeze() {
        let mut l = build_ledger();
        assert_eq!(l.len(), SAMPLES);
        assert_eq!(l.audit(), Ok(()));
        let r = l.freeze().ok().unwrap();
        assert_eq!(l.root(), r);
        assert_eq!(l.replay(), Ok(r));
        assert_eq!(l.freeze(), Err(SY_EBAL));
        assert_eq!(l.register(Sample::blank()), Err(SY_ESLOT));
        assert_eq!(l.rejects(), 2);
        assert_eq!(l.replay(), Err(SY_ESEAL));
    }

    #[test]
    fn four_routes() {
        assert_eq!(inject_four(), [true; 4]);
    }
}
