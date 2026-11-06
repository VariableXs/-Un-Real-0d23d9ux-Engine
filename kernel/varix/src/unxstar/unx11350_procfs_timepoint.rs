//! UNX-F11350 procfs 读取时点语义（快照 vs 动态）（0xebe0 段）
//!
//! `/proc` 里的文件分两类，混为一谈就会读出「时间旅行」的数据：
//!
//! - **快照型**（如 `/proc/self/status` 的多数行）：`open` 那一刻钉住时点，
//!   同一句柄后续反复读取都返回钉住时点的内容，外部状态再怎么变也不动；
//! - **动态型**（如 `/proc/uptime`）：每次读取都反映当下的时钟，同一句柄两次
//!   读取之间时钟前进，值就必须跟着前进。
//!
//! 这两条规则互相矛盾却共存于同一个目录，所以本模块把它们放在一张表里按条目
//! 分账：每次读取都记下返回值，快照项的值必须恒等于打开时钉住的时点，动态项
//! 的值必须恒等于读取当下时点。任何一次「快照项跟着时钟走」或「动态项被钉住」
//! 都会在读数表里露馅。
//!
//! 闸门次序：
//! - `add`：零号 → 未知种类 → 表满 → 重复；
//! - `read`：未打开 → 无此条目；
//! - `close`：未打开。

use crate::alloc::format;
use crate::checks::CheckSet;

/// 段基址。
pub const SEG_BASE: u32 = 0xebe0;

/// 条目号为零。
pub const TS_EINVAL: u32 = SEG_BASE + 0x01;
/// 同号条目重复。
pub const TS_EDUP: u32 = SEG_BASE + 0x02;
/// 条目表满。
pub const TS_EFULL: u32 = SEG_BASE + 0x03;
/// 无此条目。
pub const TS_ENOENT: u32 = SEG_BASE + 0x04;
/// 句柄尚未打开。
pub const TS_ENOPEN: u32 = SEG_BASE + 0x05;
/// 句柄已关闭。
pub const TS_ECLOSED: u32 = SEG_BASE + 0x06;
/// 账本形状不符。
pub const TS_EBAL: u32 = SEG_BASE + 0x07;
/// 槽位乱序。
pub const TS_EORDER: u32 = SEG_BASE + 0x08;
/// 观测值与复算不符。
pub const TS_EMISMATCH: u32 = SEG_BASE + 0x09;
/// 冻结后重放不一致。
pub const TS_ESEAL: u32 = SEG_BASE + 0x0a;
/// 观察下标越界。
pub const TS_ENOBS: u32 = SEG_BASE + 0x0b;
/// 未知条目种类。
pub const TS_EKIND: u32 = SEG_BASE + 0x0c;
/// 时点回退。
pub const TS_EPIN: u32 = SEG_BASE + 0x0d;

/// 条目容量。
pub const CAP: usize = 8;
/// 账本格数。
pub const SAMPLES: usize = 14;

/// 快照型：打开时钉住。
pub const K_STATIC: u8 = 1;
/// 动态型：每次读取取当下。
pub const K_DYNAMIC: u8 = 2;

/// 操作码：加条目。
pub const OP_ADD: u8 = 1;
/// 操作码：打开。
pub const OP_OPEN: u8 = 2;
/// 操作码：推进时钟。
pub const OP_TICK: u8 = 3;
/// 操作码：读取。
pub const OP_READ: u8 = 4;
/// 操作码：关闭。
pub const OP_CLOSE: u8 = 5;

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

/// 一个条目。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Entry {
    /// 条目号。
    pub id: u32,
    /// 快照型或动态型。
    pub kind: u8,
}

/// 一锅两味的视图。
#[derive(Clone, Copy, Debug)]
pub struct View {
    e: [Entry; CAP],
    n: usize,
    tick: u32,
    pin: u32,
    open: bool,
    ever: bool,
    reads: u32,
    seen_id: [u32; CAP],
    seen_val: [u32; CAP],
    seen_pin: [u32; CAP],
    seen_len: usize,
}

impl View {
    /// 空视图，时钟从零开始。
    pub const fn new() -> View {
        View {
            e: [Entry { id: 0, kind: 0 }; CAP],
            n: 0,
            tick: 0,
            pin: 0,
            open: false,
            ever: false,
            reads: 0,
            seen_id: [0; CAP],
            seen_val: [0; CAP],
            seen_pin: [0; CAP],
            seen_len: 0,
        }
    }

    /// 条目条数。
    pub fn count(&self) -> usize {
        self.n
    }

    /// 当前时钟。
    pub fn tick(&self) -> u32 {
        self.tick
    }

    /// 打开时钉住的时点。
    pub fn pin(&self) -> u32 {
        self.pin
    }

    /// 句柄是否打开。
    pub fn is_open(&self) -> bool {
        self.open
    }

    /// 累计读取次数。
    pub fn read_count(&self) -> u32 {
        self.reads
    }

    /// 第 i 个条目。
    pub fn entry_at(&self, i: usize) -> Option<Entry> {
        if i >= self.n {
            return None;
        }
        self.e.get(i).copied()
    }

    /// 第 i 次读取记录的条目号。
    pub fn seen_id_at(&self, i: usize) -> Result<u32, u32> {
        if i >= self.seen_len {
            return Err(TS_ENOBS);
        }
        match self.seen_id.get(i) {
            Some(v) => Ok(*v),
            None => Err(TS_ENOBS),
        }
    }

    /// 第 i 次读取记录的值。
    pub fn seen_val_at(&self, i: usize) -> Result<u32, u32> {
        if i >= self.seen_len {
            return Err(TS_ENOBS);
        }
        match self.seen_val.get(i) {
            Some(v) => Ok(*v),
            None => Err(TS_ENOBS),
        }
    }

    /// 按号查条目。
    fn entry(&self, id: u32) -> Option<Entry> {
        let mut i = 0usize;
        while i < self.n {
            match self.e.get(i) {
                Some(x) => {
                    if x.id == id {
                        return Some(*x);
                    }
                }
                None => return None,
            }
            i += 1;
        }
        None
    }

    /// 挂条目。
    pub fn add(&mut self, id: u32, kind: u8) -> Result<usize, u32> {
        if id == 0 {
            return Err(TS_EINVAL);
        }
        if kind != K_STATIC && kind != K_DYNAMIC {
            return Err(TS_EKIND);
        }
        if self.n >= CAP {
            return Err(TS_EFULL);
        }
        if self.entry(id).is_some() {
            return Err(TS_EDUP);
        }
        if let Some(slot) = self.e.get_mut(self.n) {
            *slot = Entry { id, kind };
        }
        self.n += 1;
        Ok(self.n)
    }

    /// 打开：钉住当下时点。
    pub fn open(&mut self) -> Result<u32, u32> {
        self.pin = self.tick;
        self.open = true;
        self.ever = true;
        Ok(self.pin)
    }

    /// 推进时钟。
    pub fn advance(&mut self, steps: u32) -> Result<u32, u32> {
        self.tick = self.tick.saturating_add(steps);
        Ok(self.tick)
    }

    /// 读取一条：快照型取钉住时点，动态型取当下时点。
    pub fn read(&mut self, id: u32) -> Result<u32, u32> {
        if !self.open {
            if self.ever {
                return Err(TS_ECLOSED);
            }
            return Err(TS_ENOPEN);
        }
        let ent = match self.entry(id) {
            Some(e) => e,
            None => return Err(TS_ENOENT),
        };
        let val = if ent.kind == K_STATIC { self.pin } else { self.tick };
        if self.seen_len < CAP {
            if let Some(slot) = self.seen_id.get_mut(self.seen_len) {
                *slot = id;
            }
            if let Some(slot) = self.seen_val.get_mut(self.seen_len) {
                *slot = val;
            }
            if let Some(slot) = self.seen_pin.get_mut(self.seen_len) {
                *slot = self.pin;
            }
            self.seen_len += 1;
        }
        self.reads = self.reads.saturating_add(1);
        Ok(val)
    }

    /// 关闭句柄。
    pub fn close(&mut self) -> Result<u32, u32> {
        if !self.open {
            if self.ever {
                return Err(TS_ECLOSED);
            }
            return Err(TS_ENOPEN);
        }
        self.open = false;
        Ok(0)
    }

    /// 读数表与条目种类是否自洽。
    pub fn consistent(&self) -> bool {
        let mut i = 0usize;
        while i < self.seen_len {
            let id = match self.seen_id.get(i) {
                Some(v) => *v,
                None => return false,
            };
            let val = match self.seen_val.get(i) {
                Some(v) => *v,
                None => return false,
            };
            let ent = match self.entry(id) {
                Some(e) => e,
                None => return false,
            };
            let pin = match self.seen_pin.get(i) {
                Some(v) => *v,
                None => return false,
            };
            if ent.kind == K_STATIC && val != pin {
                return false;
            }
            if ent.kind == K_DYNAMIC && val < pin {
                return false;
            }
            i += 1;
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
            self.rejects += 1;
            return Err(TS_EORDER);
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
            return Err(TS_EBAL);
        }
        const VALS: [u32; 8] = [1, 2, 0, 5, 0, 5, 0, 5];
        const CODES: [u32; 6] =
            [TS_EINVAL, TS_EDUP, TS_EFULL, TS_ENOENT, TS_ENOPEN, TS_ECLOSED];
        let mut vi = 0usize;
        let mut ci = 0usize;
        let mut i = 0usize;
        while i < self.len {
            let s = match self.rows.get(i) {
                Some(v) => *v,
                None => return Err(TS_EBAL),
            };
            if s.slot as usize != i {
                return Err(TS_EORDER);
            }
            if s.ok {
                if s.code != 0 {
                    return Err(TS_EMISMATCH);
                }
                match VALS.get(vi) {
                    Some(w) => {
                        if s.value != *w {
                            return Err(TS_EMISMATCH);
                        }
                    }
                    None => return Err(TS_EBAL),
                }
                vi += 1;
            } else {
                match CODES.get(ci) {
                    Some(w) => {
                        if s.code != *w {
                            return Err(TS_EMISMATCH);
                        }
                    }
                    None => return Err(TS_EBAL),
                }
                ci += 1;
            }
            i += 1;
        }
        if vi != 8 || ci != 6 {
            return Err(TS_EBAL);
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
            return Err(TS_EBAL);
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
            Err(TS_ESEAL)
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

/// 十四格账本：两条目 → 打开 → 推进时钟 → 快照项不动/动态项跟随 → 重开钉新点 → 各类拒绝。
pub fn build_ledger() -> Ledger {
    let mut l = Ledger::new();
    let mut v = View::new();

    let _ = l.register(cell(0, OP_ADD, v.add(1, K_STATIC)));
    let _ = l.register(cell(1, OP_ADD, v.add(2, K_DYNAMIC)));
    let _ = l.register(word_cell(2, OP_OPEN, v.open()));
    let _ = l.register(word_cell(3, OP_TICK, v.advance(5)));
    let _ = l.register(word_cell(4, OP_READ, v.read(1)));
    let _ = l.register(word_cell(5, OP_READ, v.read(2)));
    let _ = l.register(word_cell(6, OP_CLOSE, v.close()));
    let _ = l.register(word_cell(7, OP_OPEN, v.open()));

    let mut e = View::new();
    let _ = l.register(cell(8, OP_ADD, e.add(0, K_STATIC)));

    let mut d = View::new();
    let _ = d.add(5, K_STATIC);
    let _ = l.register(cell(9, OP_ADD, d.add(5, K_STATIC)));

    let mut full = View::new();
    let mut i = 1u32;
    while (i as usize) <= CAP {
        let _ = full.add(i, K_STATIC);
        i += 1;
    }
    let _ = l.register(cell(10, OP_ADD, full.add(0x100, K_STATIC)));

    let _ = l.register(word_cell(11, OP_READ, v.read(99)));

    let mut n = View::new();
    let _ = l.register(word_cell(12, OP_READ, n.read(1)));

    let mut c = View::new();
    let _ = c.open();
    let _ = c.close();
    let _ = l.register(word_cell(13, OP_READ, c.read(1)));

    l
}

/// 四路注入。
pub fn inject_four() -> [bool; 4] {
    let mut out = [false; 4];

    let l = build_ledger();
    let rep = l.replay();
    let root = l.seal();
    out[0] = l.len() == SAMPLES && l.audit().is_ok() && rep == Ok(root) && root != 0;

    // 一路：同一句柄内，快照项不动、动态项随时钟前进。
    let mut a = View::new();
    let _ = a.add(1, K_STATIC);
    let _ = a.add(2, K_DYNAMIC);
    let p = a.open();
    let s1 = a.read(1);
    let d1 = a.read(2);
    let _ = a.advance(7);
    let s2 = a.read(1);
    let d2 = a.read(2);
    out[1] = p == Ok(0)
        && s1 == Ok(0)
        && d1 == Ok(0)
        && s2 == Ok(0)
        && d2 == Ok(7)
        && a.tick() == 7
        && a.pin() == 0
        && a.consistent();

    // 二路：重新打开把快照项钉到新时点。
    let mut b = View::new();
    let _ = b.add(1, K_STATIC);
    let _ = b.open();
    let _ = b.read(1);
    let _ = b.close();
    let _ = b.advance(3);
    let p2 = b.open();
    let s = b.read(1);
    out[2] = p2 == Ok(3)
        && s == Ok(3)
        && b.pin() == 3
        && b.seen_val_at(0) == Ok(0)
        && b.seen_val_at(1) == Ok(3)
        && b.seen_id_at(1) == Ok(1)
        && b.consistent();

    // 三路：闸门（未打开 / 无条目 / 未知种类 / 重复关闭）。
    let mut g = View::new();
    let g1 = g.read(1) == Err(TS_ENOPEN);
    let g2 = g.close() == Err(TS_ENOPEN);
    let g3 = g.add(0, K_STATIC) == Err(TS_EINVAL);
    let g4 = g.add(1, 9) == Err(TS_EKIND);
    let g5 = g.add(1, K_STATIC) == Ok(1);
    let g6 = g.add(1, K_STATIC) == Err(TS_EDUP);
    let g7 = g.open() == Ok(0);
    let g8 = g.read(2) == Err(TS_ENOENT);
    let g9 = g.close() == Ok(0);
    let g10 = g.read(1) == Err(TS_ECLOSED) && g.close() == Err(TS_ECLOSED);
    out[3] = g1 && g2 && g3 && g4 && g5 && g6 && g7 && g8 && g9 && g10;

    out
}

/// 十三项判据。
pub fn run_unx11350_checks() -> CheckSet {
    let mut s = CheckSet::new("unx11350.procfs_timepoint");
    let l = build_ledger();

    s.add_dyn(
        "TS1-十四格八允六拒",
        l.len() == SAMPLES && l.ok_count() == 8 && l.err_count() == 6 && l.audit().is_ok(),
        &format!("n={} ok={} err={}", l.len(), l.ok_count(), l.err_count()),
    );

    s.add(
        "TS2-两条目挂接",
        l.sample(0).map(|x| x.value) == Some(1) && l.sample(1).map(|x| x.value) == Some(2),
        "static",
    );

    s.add(
        "TS3-打开钉住零时点",
        l.sample(2).map(|x| x.value) == Some(0),
        "static",
    );

    s.add(
        "TS4-时钟推进到五",
        l.sample(3).map(|x| x.value) == Some(5),
        "static",
    );

    s.add(
        "TS5-快照项停在钉住时点",
        l.sample(4).map(|x| x.value) == Some(0),
        "static",
    );

    s.add(
        "TS6-动态项取当下时点",
        l.sample(5).map(|x| x.value) == Some(5),
        "static",
    );

    s.add(
        "TS7-关闭与重开钉新点",
        l.sample(6).map(|x| x.value) == Some(0) && l.sample(7).map(|x| x.value) == Some(5),
        "static",
    );

    s.add(
        "TS8-零号条目被拒",
        l.sample(8).map(|x| x.code) == Some(TS_EINVAL),
        "static",
    );

    s.add(
        "TS9-重号与表满",
        l.sample(9).map(|x| x.code) == Some(TS_EDUP)
            && l.sample(10).map(|x| x.code) == Some(TS_EFULL),
        "static",
    );

    s.add(
        "TS10-无条目与未打开",
        l.sample(11).map(|x| x.code) == Some(TS_ENOENT)
            && l.sample(12).map(|x| x.code) == Some(TS_ENOPEN),
        "static",
    );

    s.add(
        "TS11-关闭后读取被拒",
        l.sample(13).map(|x| x.code) == Some(TS_ECLOSED),
        "static",
    );

    s.add(
        "TS12-读数表与种类自洽",
        {
            let mut a = View::new();
            let _ = a.add(1, K_STATIC);
            let _ = a.add(2, K_DYNAMIC);
            let _ = a.open();
            let _ = a.read(1);
            let _ = a.advance(4);
            let _ = a.read(1);
            let _ = a.read(2);
            a.seen_val_at(0) == Ok(0)
                && a.seen_val_at(1) == Ok(0)
                && a.seen_val_at(2) == Ok(4)
                && a.read_count() == 3
                && a.consistent()
        },
        "static",
    );

    s.add(
        "TS13-冻结重放四注入",
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
        let s = run_unx11350_checks();
        assert_eq!(s.len(), 13);
        assert!(s.all_passed());
    }

    #[test]
    fn static_entry_is_pinned() {
        let mut v = View::new();
        let _ = v.add(1, K_STATIC);
        let _ = v.open();
        assert_eq!(v.read(1), Ok(0));
        let _ = v.advance(9);
        assert_eq!(v.read(1), Ok(0));
        assert_eq!(v.tick(), 9);
    }

    #[test]
    fn dynamic_entry_follows_clock() {
        let mut v = View::new();
        let _ = v.add(2, K_DYNAMIC);
        let _ = v.open();
        assert_eq!(v.read(2), Ok(0));
        let _ = v.advance(3);
        assert_eq!(v.read(2), Ok(3));
        let _ = v.advance(4);
        assert_eq!(v.read(2), Ok(7));
    }

    #[test]
    fn reopen_repins() {
        let mut v = View::new();
        let _ = v.add(1, K_STATIC);
        let _ = v.open();
        let _ = v.advance(2);
        let _ = v.close();
        assert_eq!(v.pin(), 0);
        assert_eq!(v.open(), Ok(2));
        assert_eq!(v.read(1), Ok(2));
    }

    #[test]
    fn gates() {
        let mut v = View::new();
        assert_eq!(v.read(1), Err(TS_ENOPEN));
        assert_eq!(v.close(), Err(TS_ENOPEN));
        assert_eq!(v.add(0, K_STATIC), Err(TS_EINVAL));
        assert_eq!(v.add(1, 7), Err(TS_EKIND));
        assert_eq!(v.add(1, K_DYNAMIC), Ok(1));
        assert_eq!(v.add(1, K_STATIC), Err(TS_EDUP));
        assert_eq!(v.open(), Ok(0));
        assert_eq!(v.read(2), Err(TS_ENOENT));
        assert_eq!(v.close(), Ok(0));
        assert_eq!(v.read(1), Err(TS_ECLOSED));
        assert_eq!(v.close(), Err(TS_ECLOSED));
    }

    #[test]
    fn capacity_is_exact() {
        let mut v = View::new();
        let mut i = 1u32;
        while (i as usize) <= CAP {
            assert!(v.add(i, K_STATIC).is_ok());
            i += 1;
        }
        assert_eq!(v.add(0xFFFF, K_STATIC), Err(TS_EFULL));
        assert_eq!(v.count(), CAP);
    }

    #[test]
    fn read_log_is_bounded() {
        let mut v = View::new();
        let _ = v.add(1, K_DYNAMIC);
        let _ = v.open();
        let _ = v.read(1);
        assert_eq!(v.seen_id_at(0), Ok(1));
        assert_eq!(v.seen_val_at(1), Err(TS_ENOBS));
    }

    #[test]
    fn freeze_blocks_writes() {
        let mut l = build_ledger();
        assert!(l.freeze().is_ok());
        assert!(l.register(Sample::blank()).is_err());
        assert_eq!(l.rejects(), 1);
    }
}
