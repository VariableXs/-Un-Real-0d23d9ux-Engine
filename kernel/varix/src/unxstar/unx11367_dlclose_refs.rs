//! UNX-F11367 dlclose 引用计数语义（0xecf0 段）
//!
//! `dlopen` 同一个映像 N 次得到的是**同一个**装载对象，引用计数加 N；`dlclose`
//! 每次只减一，只有减到零才真正卸载。于是「多开单关」必须存活，「同数对消」
//! 必须归零并卸载，这两条判据是 dlclose 语义的骨架。
//!
//! 本模块在此之上补了三个真实链接器必须处理的边角，并把它们做成互不混淆的错误类：
//!
//! 1. **句柄代际（ESTALE）**：卸载后槽位会被后来的 `dlopen` 复用。若只拿槽位下标
//!    当句柄，一个陈旧的 `dlclose` 会关掉**后来者**——这是最危险的 use-after-close。
//!    因此句柄 = 代际 << 8 | 槽位，卸载即推进代际；旧代句柄一律 `ESTALE`。
//! 2. **未复活即关闭（EREFS）**：槽位已卸载但代际已推进到「下一任」，此时用下一任
//!    的句柄来 close，代际对得上但对象是死的。这与 `ESTALE`（代际对不上）必须分开：
//!    前者是调用方提前使用了**将来**的句柄，后者是握着**过去**的句柄。
//! 3. **引用饱和（EDRIFT）**：计数是 16 位，饱和前必须拒收，不能默默回绕成 0 然后
//!    在下一次 close 时把还在使用的映像卸掉。
//!
//! 还有 `force_unload`（相当于 `RTLD_NOLOAD` 之外的强制路径）：只要计数仍大于一，
//! 就以 `EHELD` 拒收，绝不代替调用方吞掉别人的引用。
//!
//! 闸门次序（关闭）：空句柄 → 槽位越界 → 代际不符 → 已卸载 → 递减/卸载。
//! 闸门次序（装载）：空键 → 目录无此件 → 计数饱和 → 活件加计/死槽复活/新槽/表满。
//! 代际检查必须排在「已卸载」之前：先确认这个号属于当下，再谈它是否还活着。

use crate::alloc::format;
use crate::checks::CheckSet;

/// 段基址。
pub const SEG_BASE: u32 = 0xecf0;

/// 键或句柄为零。
pub const RC_EINVAL: u32 = SEG_BASE + 0x01;
/// 目录里没有这个映像键。
pub const RC_ENOENT: u32 = SEG_BASE + 0x02;
/// 句柄槽位越界。
pub const RC_EBADH: u32 = SEG_BASE + 0x03;
/// 句柄代际过期（指向曾经的装载）。
pub const RC_ESTALE: u32 = SEG_BASE + 0x04;
/// 槽位已卸载但代际为当下（提前使用将来句柄）。
pub const RC_EREFS: u32 = SEG_BASE + 0x05;
/// 装载表已满。
pub const RC_EFULL: u32 = SEG_BASE + 0x06;
/// 强制卸载时仍被他人持有。
pub const RC_EHELD: u32 = SEG_BASE + 0x07;
/// 引用计数已达上限。
pub const RC_EDRIFT: u32 = SEG_BASE + 0x08;
/// 账本形状不符。
pub const RC_EBAL: u32 = SEG_BASE + 0x09;
/// 槽位乱序。
pub const RC_EORDER: u32 = SEG_BASE + 0x0a;
/// 观测值与复算不符。
pub const RC_EMISMATCH: u32 = SEG_BASE + 0x0b;
/// 冻结后重放不一致。
pub const RC_ESEAL: u32 = SEG_BASE + 0x0c;
/// 观察下标越界。
pub const RC_ENOBS: u32 = SEG_BASE + 0x0d;

/// 装载表容量。
pub const CAP: usize = 6;
/// 目录容量。
pub const ICAP: usize = 8;
/// 账本格数。
pub const SAMPLES: usize = 14;

/// 操作码：打开。
pub const OP_OPEN: u8 = 1;
/// 操作码：关闭。
pub const OP_CLOSE: u8 = 2;
/// 操作码：强制卸载。
pub const OP_FORCE: u8 = 3;
/// 操作码：读引用计数。
pub const OP_REFS: u8 = 4;

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

/// 取句柄；失败记 0，便于在检查里继续推进。
fn hnd(r: Result<u32, u32>) -> u32 {
    match r {
        Ok(v) => v,
        Err(_) => 0,
    }
}

/// 目录条目：映像键与它是否存在。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Entry {
    /// 映像键。
    pub key: u32,
    /// 是否存在。
    pub present: bool,
}

impl Entry {
    /// 造条目。
    pub const fn make(key: u32, present: bool) -> Entry {
        Entry { key, present }
    }
}

/// 装载槽位。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Slot {
    /// 映像键。
    pub key: u32,
    /// 代际。
    pub gen: u8,
    /// 引用计数。
    pub refs: u16,
    /// 是否已装载。
    pub live: bool,
}

impl Slot {
    /// 空槽。
    pub const fn empty() -> Slot {
        Slot { key: 0, gen: 0, refs: 0, live: false }
    }
}

/// 引用计数装载器。
#[derive(Clone, Copy, Debug)]
pub struct Loader {
    dir: [Entry; ICAP],
    slots: [Slot; CAP],
    n: usize,
    opens: u32,
    closes: u32,
    unloads: u32,
}

impl Loader {
    /// 目录：0x11..=0x18 存在，其余键都不存在。
    pub const fn seeded() -> Loader {
        let mut dir = [Entry::make(0, false); ICAP];
        let mut k = 0usize;
        while k < ICAP {
            dir[k] = Entry::make(0x11 + k as u32, true);
            k += 1;
        }
        Loader {
            dir,
            slots: [Slot::empty(); CAP],
            n: 0,
            opens: 0,
            closes: 0,
            unloads: 0,
        }
    }

    /// 空目录。
    pub const fn new() -> Loader {
        Loader {
            dir: [Entry::make(0, false); ICAP],
            slots: [Slot::empty(); CAP],
            n: 0,
            opens: 0,
            closes: 0,
            unloads: 0,
        }
    }

    /// 是否登记过该键。
    fn known(&self, key: u32) -> bool {
        let mut i = 0usize;
        while i < ICAP {
            if let Some(e) = self.dir.get(i) {
                if e.present && e.key == key {
                    return true;
                }
            }
            i += 1;
        }
        false
    }

    /// 句柄编码。
    fn encode(idx: usize, gen: u8) -> u32 {
        ((gen as u32) << 8) | ((idx as u32) + 1)
    }

    /// 句柄拆解：返回槽位与代际。
    fn decode(&self, handle: u32) -> Result<(usize, u8), u32> {
        let low = handle & 0xff;
        if low == 0 {
            return Err(RC_EBADH);
        }
        let idx = (low as usize) - 1;
        if idx >= self.n || idx >= CAP {
            return Err(RC_EBADH);
        }
        Ok((idx, (handle >> 8) as u8))
    }

    /// 当下活槽位的句柄。
    pub fn handle_of(&self, key: u32) -> Option<u32> {
        let mut i = 0usize;
        while i < self.n {
            if let Some(s) = self.slots.get(i) {
                if s.live && s.key == key {
                    return Some(Self::encode(i, s.gen));
                }
            }
            i += 1;
        }
        None
    }

    /// 打开（引用计数加一）。
    pub fn open(&mut self, key: u32) -> Result<u32, u32> {
        if key == 0 {
            return Err(RC_EINVAL);
        }
        if !self.known(key) {
            return Err(RC_ENOENT);
        }
        let mut i = 0usize;
        let mut dead: Option<usize> = None;
        while i < self.n {
            let s = match self.slots.get(i) {
                Some(v) => *v,
                None => {
                    i += 1;
                    continue;
                }
            };
            if s.live {
                if s.key == key {
                    if s.refs == u16::MAX {
                        return Err(RC_EDRIFT);
                    }
                    let next = s.refs.saturating_add(1);
                    if let Some(dst) = self.slots.get_mut(i) {
                        dst.refs = next;
                    }
                    self.opens = self.opens.saturating_add(1);
                    return Ok(Self::encode(i, s.gen));
                }
            } else if dead.is_none() && s.key == key {
                dead = Some(i);
            }
            i += 1;
        }
        let idx = match dead {
            Some(v) => {
                let s = match self.slots.get(v) {
                    Some(w) => *w,
                    None => return Err(RC_EBADH),
                };
                if let Some(dst) = self.slots.get_mut(v) {
                    dst.refs = 1;
                    dst.live = true;
                    dst.gen = s.gen;
                }
                v
            }
            None => {
                if self.n >= CAP {
                    return Err(RC_EFULL);
                }
                if let Some(dst) = self.slots.get_mut(self.n) {
                    *dst = Slot { key, gen: 0, refs: 1, live: true };
                }
                self.n += 1;
                self.n - 1
            }
        };
        self.opens = self.opens.saturating_add(1);
        let g = match self.slots.get(idx) {
            Some(s) => s.gen,
            None => return Err(RC_EBADH),
        };
        Ok(Self::encode(idx, g))
    }

    /// 关闭（引用计数减一；归零即卸载）。
    pub fn close(&mut self, handle: u32) -> Result<usize, u32> {
        if handle == 0 {
            return Err(RC_EINVAL);
        }
        let (idx, gen) = self.decode(handle)?;
        let s = match self.slots.get(idx) {
            Some(v) => *v,
            None => return Err(RC_EBADH),
        };
        if s.gen != gen {
            return Err(RC_ESTALE);
        }
        if !s.live {
            return Err(RC_EREFS);
        }
        self.closes = self.closes.saturating_add(1);
        if s.refs > 1 {
            let next = s.refs.saturating_sub(1);
            if let Some(dst) = self.slots.get_mut(idx) {
                dst.refs = next;
            }
            return Ok(next as usize);
        }
        if let Some(dst) = self.slots.get_mut(idx) {
            dst.refs = 0;
            dst.live = false;
            dst.gen = s.gen.wrapping_add(1);
        }
        self.unloads = self.unloads.saturating_add(1);
        Ok(0)
    }

    /// 强制卸载：仍有他人持有时拒收。
    pub fn force_unload(&mut self, handle: u32) -> Result<usize, u32> {
        if handle == 0 {
            return Err(RC_EINVAL);
        }
        let (idx, gen) = self.decode(handle)?;
        let s = match self.slots.get(idx) {
            Some(v) => *v,
            None => return Err(RC_EBADH),
        };
        if s.gen != gen {
            return Err(RC_ESTALE);
        }
        if !s.live {
            return Err(RC_EREFS);
        }
        if s.refs > 1 {
            return Err(RC_EHELD);
        }
        self.close(handle)
    }

    /// 读引用计数。
    pub fn refs_of(&self, handle: u32) -> Result<usize, u32> {
        if handle == 0 {
            return Err(RC_EINVAL);
        }
        let (idx, gen) = self.decode(handle)?;
        let s = match self.slots.get(idx) {
            Some(v) => *v,
            None => return Err(RC_EBADH),
        };
        if s.gen != gen {
            return Err(RC_ESTALE);
        }
        if !s.live {
            return Err(RC_EREFS);
        }
        Ok(s.refs as usize)
    }

    /// 按已知计数重建句柄（恢复/测试场景）。
    pub fn refs_seed(&mut self, key: u32, refs: u16) -> Option<u32> {
        if key == 0 || refs == 0 || !self.known(key) || self.n >= CAP {
            return None;
        }
        let mut i = 0usize;
        while i < self.n {
            let s = match self.slots.get(i) {
                Some(v) => *v,
                None => {
                    i += 1;
                    continue;
                }
            };
            if s.key == key {
                if let Some(dst) = self.slots.get_mut(i) {
                    dst.refs = refs;
                    dst.live = true;
                }
                return Some(Self::encode(i, s.gen));
            }
            i += 1;
        }
        if let Some(dst) = self.slots.get_mut(self.n) {
            *dst = Slot { key, gen: 0, refs, live: true };
        }
        self.n += 1;
        Some(Self::encode(self.n - 1, 0))
    }

    /// 活件数。
    pub fn live_count(&self) -> usize {
        let mut k = 0usize;
        let mut i = 0usize;
        while i < self.n {
            if let Some(s) = self.slots.get(i) {
                if s.live {
                    k += 1;
                }
            }
            i += 1;
        }
        k
    }

    /// 槽位数。
    pub fn slot_count(&self) -> usize {
        self.n
    }

    /// 打开次数。
    pub fn opens(&self) -> u32 {
        self.opens
    }

    /// 关闭次数。
    pub fn closes(&self) -> u32 {
        self.closes
    }

    /// 卸载次数。
    pub fn unloads(&self) -> u32 {
        self.unloads
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
            return Err(RC_EORDER);
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
            return Err(RC_EBAL);
        }
        const VALS: [u32; 8] = [1, 1, 1, 0, 257, 2, 0, 1];
        const CODES: [u32; 6] =
            [RC_ESTALE, RC_EBADH, RC_EINVAL, RC_ENOENT, RC_EHELD, RC_EFULL];
        let mut vi = 0usize;
        let mut ci = 0usize;
        let mut i = 0usize;
        while i < self.len {
            let s = match self.rows.get(i) {
                Some(v) => *v,
                None => return Err(RC_EBAL),
            };
            if s.slot as usize != i {
                return Err(RC_EORDER);
            }
            if s.ok {
                if s.code != 0 {
                    return Err(RC_EMISMATCH);
                }
                match VALS.get(vi) {
                    Some(w) => {
                        if s.value != *w {
                            return Err(RC_EMISMATCH);
                        }
                    }
                    None => return Err(RC_EBAL),
                }
                vi += 1;
            } else {
                match CODES.get(ci) {
                    Some(w) => {
                        if s.code != *w {
                            return Err(RC_EMISMATCH);
                        }
                    }
                    None => return Err(RC_EBAL),
                }
                ci += 1;
            }
            i += 1;
        }
        if vi != 8 || ci != 6 {
            return Err(RC_EBAL);
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
            return Err(RC_EBAL);
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
            Err(RC_ESEAL)
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

/// 十四格账本：一装两开两关，复活、越界、空键、无件、强卸、满表各一拒。
pub fn build_ledger() -> Ledger {
    let mut l = Ledger::new();
    let mut m = Loader::seeded();

    let _ = l.register(word_cell(0, OP_OPEN, m.open(0x11)));
    let _ = l.register(word_cell(1, OP_OPEN, m.open(0x11)));
    let _ = l.register(cell(2, OP_CLOSE, m.close(1)));
    let _ = l.register(cell(3, OP_CLOSE, m.close(1)));
    let _ = l.register(word_cell(4, OP_OPEN, m.open(0x11)));
    let _ = l.register(word_cell(5, OP_OPEN, m.open(0x12)));
    let _ = l.register(cell(6, OP_CLOSE, m.close(2)));
    let _ = l.register(cell(7, OP_REFS, m.refs_of(0x101)));

    let _ = l.register(cell(8, OP_CLOSE, m.close(1)));

    let _ = l.register(cell(9, OP_CLOSE, m.close(0x105)));

    let _ = l.register(word_cell(10, OP_OPEN, m.open(0)));

    let _ = l.register(word_cell(11, OP_OPEN, m.open(0x99)));

    let _ = m.open(0x11);
    let _ = m.open(0x11);
    let _ = l.register(cell(12, OP_FORCE, m.force_unload(0x101)));

    let _ = m.open(0x12);
    let _ = m.open(0x13);
    let _ = m.open(0x14);
    let _ = m.open(0x15);
    let _ = m.open(0x16);
    let _ = l.register(word_cell(13, OP_OPEN, m.open(0x17)));

    l
}

/// 四路注入。
pub fn inject_four() -> [bool; 4] {
    let mut out = [false; 4];

    let l = build_ledger();
    let rep = l.replay();
    let root = l.seal();
    out[0] = l.len() == SAMPLES && l.audit().is_ok() && rep == Ok(root) && root != 0;

    // 一路：多开单关存活、同数对消归零。
    let mut a = Loader::seeded();
    let hw = a.open(0x13);
    let h2w = a.open(0x13);
    let h = hnd(hw);
    let h2 = hnd(h2w);
    let one = a.close(h);
    let alive = a.live_count();
    let held = a.refs_of(h);
    let zero = a.close(h2);
    let gone = a.live_count();
    let stale = a.refs_of(h);
    out[1] = hw == Ok(1)
        && h2w == Ok(1)
        && one == Ok(1)
        && alive == 1
        && held == Ok(1)
        && zero == Ok(0)
        && gone == 0
        && stale == Err(RC_ESTALE)
        && a.opens() == 2
        && a.closes() == 2
        && a.unloads() == 1;

    // 二路：七类失败互不相同。
    let mut b = Loader::seeded();
    let e1 = b.open(0);
    let e2 = b.open(0x99);
    let e3 = b.close(0);
    let e4 = b.close(0x105);
    let hb = b.open(0x11);
    let e5 = a.close(1);
    let mut c = Loader::seeded();
    let hc = hnd(c.open(0x12));
    let _ = c.close(hc);
    let e6 = c.close(0x101);
    let mut d = Loader::seeded();
    let hd = hnd(d.open(0x13));
    let _ = d.open(0x13);
    let e7 = d.force_unload(hd);
    let mut e = Loader::seeded();
    let _ = e.refs_seed(0x11, u16::MAX);
    let e8 = e.open(0x11);
    let mut f = Loader::seeded();
    let _ = f.refs_seed(0x11, 1);
    let _ = f.refs_seed(0x12, 1);
    let _ = f.refs_seed(0x13, 1);
    let _ = f.refs_seed(0x14, 1);
    let _ = f.refs_seed(0x15, 1);
    let _ = f.refs_seed(0x16, 1);
    let e9 = f.open(0x17);
    let distinct = e1.err() != e2.err()
        && e2.err() != e3.err()
        && e3.err() != e4.err()
        && e4.err() != e5.err()
        && e5.err() != e6.err()
        && e6.err() != e7.err()
        && e7.err() != e8.err();
    out[2] = e1 == Err(RC_EINVAL)
        && e2 == Err(RC_ENOENT)
        && e3 == Err(RC_EINVAL)
        && e4 == Err(RC_EBADH)
        && e5 == Err(RC_ESTALE)
        && e6 == Err(RC_EREFS)
        && e7 == Err(RC_EHELD)
        && e8 == Err(RC_EDRIFT)
        && e9 == Err(RC_EFULL)
        && hb == Ok(1)
        && distinct;

    // 三路：闸门次序与容量边界。
    let mut g = Loader::seeded();
    let g1 = g.close(0);
    let g2 = g.close(0x101);
    let h1w = g.open(0x11);
    let h1 = hnd(h1w);
    let g3 = g.force_unload(h1);
    let g4 = g.close(h1);
    let g5 = g.close(h1);
    let again = g.open(0x11);
    let mut h = Loader::seeded();
    let mut i = 0u32;
    while (i as usize) < CAP {
        let _ = h.open(0x11);
        i += 1;
    }
    let refs = h.refs_of(1);
    let slot0 = h.slot_count();
    let mut rev = Loader::seeded();
    let r1 = hnd(rev.open(0x11));
    let _ = rev.close(r1);
    let r2 = rev.open(0x11);
    let reused = rev.slot_count();
    out[3] = g1 == Err(RC_EINVAL)
        && g2 == Err(RC_EBADH)
        && h1w == Ok(1)
        && g3 == Ok(0)
        && g4 == Err(RC_ESTALE)
        && g5 == Err(RC_ESTALE)
        && again == Ok(0x101)
        && refs == Ok(CAP)
        && slot0 == 1
        && r2 == Ok(0x101)
        && reused == 1;

    out
}

/// 运行本卡检查。
pub fn run_unx11367_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F11367");
    let l = build_ledger();

    s.add_dyn(
        "RC1-十四格八允六拒",
        l.len() == SAMPLES && l.ok_count() == 8 && l.err_count() == 6 && l.audit().is_ok(),
        &format!("n={} ok={} err={} audit={:?}", l.len(), l.ok_count(), l.err_count(), l.audit()),
    );

    s.add(
        "RC2-多开单关存活",
        {
            let mut a = Loader::seeded();
            let h = hnd(a.open(0x11));
            let _ = a.open(0x11);
            let n = a.close(h);
            n == Ok(1)
                && a.live_count() == 1
                && a.refs_of(h) == Ok(1)
                && a.unloads() == 0
                && a.opens() == 2
        },
        "static",
    );

    s.add(
        "RC3-同数对消归零",
        {
            let mut a = Loader::seeded();
            let h = hnd(a.open(0x12));
            let _ = a.open(0x12);
            let one = a.close(h);
            let zero = a.close(h);
            one == Ok(1)
                && zero == Ok(0)
                && a.live_count() == 0
                && a.unloads() == 1
                && a.refs_of(h) == Err(RC_ESTALE)
        },
        "static",
    );

    s.add(
        "RC4-句柄代际防误关",
        {
            let mut a = Loader::seeded();
            let old = hnd(a.open(0x13));
            let _ = a.close(old);
            let neww = a.open(0x13);
            let new = hnd(neww);
            let stale = a.close(old);
            let fresh = a.close(new);
            stale == Err(RC_ESTALE)
                && neww == Ok(0x101)
                && fresh == Ok(0)
                && a.slot_count() == 1
                && a.unloads() == 2
        },
        "static",
    );

    s.add(
        "RC5-强制卸载守引用",
        {
            let mut a = Loader::seeded();
            let h = hnd(a.open(0x11));
            let _ = a.open(0x11);
            let held = a.force_unload(h);
            let _ = a.close(h);
            let last = a.force_unload(h);
            held == Err(RC_EHELD)
                && last == Ok(0)
                && a.live_count() == 0
                && a.unloads() == 1
        },
        "static",
    );

    s.add(
        "RC6-引用饱和拒收",
        {
            let mut a = Loader::seeded();
            let h = match a.refs_seed(0x11, u16::MAX) {
                Some(v) => v,
                None => 0,
            };
            let over = a.open(0x11);
            let held = a.refs_of(h);
            let one = a.close(h);
            over == Err(RC_EDRIFT) && held == Ok(65535) && one == Ok(65534)
        },
        "static",
    );

    s.add(
        "RC7-句柄越界",
        {
            let mut a = Loader::seeded();
            let _ = a.open(0x11);
            a.close(0x102) == Err(RC_EBADH) && a.close(0x1ff) == Err(RC_EBADH)
        },
        "static",
    );

    s.add(
        "RC8-未复活即关闭",
        {
            let mut a = Loader::seeded();
            let h = hnd(a.open(0x11));
            let _ = a.close(h);
            let future = a.handle_of(0x11);
            let bad = a.close(0x101);
            let refs = a.refs_of(0x101);
            future.is_none() && bad == Err(RC_EREFS) && refs == Err(RC_EREFS)
        },
        "static",
    );

    s.add(
        "RC9-容量精确",
        {
            let mut a = Loader::seeded();
            let _ = a.open(0x11);
            let _ = a.open(0x12);
            let _ = a.open(0x13);
            let _ = a.refs_seed(0x14, 1);
            let _ = a.refs_seed(0x15, 1);
            let _ = a.refs_seed(0x16, 1);
            let over = a.open(0x17);
            let back = a.open(0x11);
            over == Err(RC_EFULL) && back == Ok(1) && a.slot_count() == CAP
        },
        "static",
    );

    s.add(
        "RC10-账本槽序不可越",
        {
            let mut x = Ledger::new();
            let a = x.register(cell(1, OP_OPEN, Ok(1)));
            let b = x.register(cell(0, OP_OPEN, Ok(1)));
            let c = x.register(cell(0, OP_OPEN, Ok(1)));
            a == Err(RC_EORDER) && b == Ok(()) && c == Err(RC_EORDER) && x.len() == 1
        },
        "static",
    );

    s.add(
        "RC11-冻结后写拒",
        {
            let mut x = build_ledger();
            let r = x.freeze();
            let again = x.freeze();
            let w = x.register(Sample::blank());
            r.is_ok() && again.is_err() && w == Err(RC_EORDER) && x.rejects() == 2
        },
        "static",
    );

    s.add(
        "RC12-观测面边界",
        {
            let x = build_ledger();
            let a = x.sample(SAMPLES - 1);
            let b = x.sample(SAMPLES);
            a.is_some() && b.is_none() && !x.is_empty() && x.sample(0).is_some()
        },
        "static",
    );

    s.add(
        "RC13-冻结重放四注入",
        {
            let mut f = build_ledger();
            let rep = f.replay();
            let one = f.freeze();
            let two = f.freeze();
            let inj = inject_four();
            rep.is_ok()
                && one.is_ok()
                && one == rep
                && two.is_err()
                && f.is_frozen()
                && f.rejects() == 1
                && f.root() != 0
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
        let s = run_unx11367_checks();
        assert_eq!(s.len(), 13);
        assert!(s.all_passed());
    }

    #[test]
    fn many_opens_one_close_survives() {
        let mut a = Loader::seeded();
        let h = hnd(a.open(0x11));
        let _ = a.open(0x11);
        let _ = a.open(0x11);
        assert_eq!(a.close(h), Ok(2));
        assert_eq!(a.live_count(), 1);
        assert_eq!(a.refs_of(h), Ok(2));
    }

    #[test]
    fn balanced_opens_unload() {
        let mut a = Loader::seeded();
        let h = hnd(a.open(0x12));
        let _ = a.open(0x12);
        assert_eq!(a.close(h), Ok(1));
        assert_eq!(a.close(h), Ok(0));
        assert_eq!(a.live_count(), 0);
        assert_eq!(a.unloads(), 1);
        assert_eq!(a.refs_of(h), Err(RC_ESTALE));
    }

    #[test]
    fn generation_blocks_stale_close() {
        let mut a = Loader::seeded();
        let old = hnd(a.open(0x13));
        let _ = a.close(old);
        let new = hnd(a.open(0x13));
        assert!(new != old);
        assert_eq!(a.close(old), Err(RC_ESTALE));
        assert_eq!(a.refs_of(new), Ok(1));
    }

    #[test]
    fn gate_order_zero_before_range() {
        let mut a = Loader::seeded();
        assert_eq!(a.close(0), Err(RC_EINVAL));
        assert_eq!(a.close(0x105), Err(RC_EBADH));
        assert_eq!(a.force_unload(0), Err(RC_EINVAL));
        assert_eq!(a.refs_of(0), Err(RC_EINVAL));
    }

    #[test]
    fn capacity_is_exact() {
        let mut a = Loader::seeded();
        let _ = a.open(0x11);
        let _ = a.refs_seed(0x12, 1);
        let _ = a.refs_seed(0x13, 1);
        let _ = a.refs_seed(0x14, 1);
        let _ = a.refs_seed(0x15, 1);
        let _ = a.refs_seed(0x16, 1);
        assert_eq!(a.slot_count(), CAP);
        assert_eq!(a.open(0x17), Err(RC_EFULL));
        assert_eq!(a.open(0x11), Ok(1));
    }

    #[test]
    fn drift_rejects_rather_than_wraps() {
        let mut a = Loader::seeded();
        let h = match a.refs_seed(0x11, u16::MAX) {
            Some(v) => v,
            None => 0,
        };
        assert_eq!(a.open(0x11), Err(RC_EDRIFT));
        assert_eq!(a.refs_of(h), Ok(65535));
        assert_eq!(a.close(h), Ok(65534));
    }
}
