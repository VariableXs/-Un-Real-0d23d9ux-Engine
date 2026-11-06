//! UNX-F11351 maps 与 dl_iterate_phdr 交叉一致（0xebf0 段）
//!
//! 同一个进程地址空间有两份各自独立的「账」：
//!
//! - `/proc/self/maps` 列出每一段映射（起止、保护位、所属对象）；
//! - `dl_iterate_phdr` 列出每一个已加载对象（加载偏置、段长）。
//!
//! 两份账必须能对上：每个对象的加载偏置都应当等于它某一条可执行映射的起点；
//! 每条映射的归属对象都应当在对象表里存在。对不上的三种情形含义完全不同：
//!
//! - 有对象没映射（`ENOMAP`）：对象说自己加载了，地址空间里找不到它；
//! - 偏置不一致（`EBIAS`）：映射在，但起点和对象自报的偏置差了，说明有重定位
//!   或写入时序出错；
//! - 有映射没对象（`EORPHAN`）：地址空间里多出一段不属于任何已知对象的可执行
//!   映射，通常意味着注入或残留。
//!
//! 交叉只允许做一次（快照语义）：第一次对完之后再对，读到的已经不是同一时点
//! 的两份账，必须拒绝。
//!
//! 闸门次序：
//! - `obj`：零号 → 已交叉 → 表满 → 重复；
//! - `map`：零号归属 → 已交叉 → 表满 → 区间倒置；
//! - `cross`：已交叉 → 对象缺映射 → 偏置不一致 → 孤儿映射。

use crate::alloc::format;
use crate::checks::CheckSet;

/// 段基址。
pub const SEG_BASE: u32 = 0xebf0;

/// 对象号为零。
pub const CR_EINVAL: u32 = SEG_BASE + 0x01;
/// 同号对象重复。
pub const CR_EDUP: u32 = SEG_BASE + 0x02;
/// 表满。
pub const CR_EFULL: u32 = SEG_BASE + 0x03;
/// 已经交叉过。
pub const CR_ERUN: u32 = SEG_BASE + 0x04;
/// 对象没有对应映射。
pub const CR_ENOMAP: u32 = SEG_BASE + 0x05;
/// 加载偏置与映射起点不符。
pub const CR_EBIAS: u32 = SEG_BASE + 0x06;
/// 映射缺少可执行保护。
pub const CR_ENOPROT: u32 = SEG_BASE + 0x07;
/// 映射没有归属对象。
pub const CR_EORPHAN: u32 = SEG_BASE + 0x08;
/// 账本形状不符。
pub const CR_EBAL: u32 = SEG_BASE + 0x09;
/// 槽位乱序。
pub const CR_EORDER: u32 = SEG_BASE + 0x0a;
/// 观测值与复算不符。
pub const CR_EMISMATCH: u32 = SEG_BASE + 0x0b;
/// 冻结后重放不一致。
pub const CR_ESEAL: u32 = SEG_BASE + 0x0c;
/// 观察下标越界。
pub const CR_ENOBS: u32 = SEG_BASE + 0x0d;

/// 单表容量。
pub const CAP: usize = 6;
/// 账本格数。
pub const SAMPLES: usize = 14;

/// 操作码：收对象。
pub const OP_OBJ: u8 = 1;
/// 操作码：收映射。
pub const OP_MAP: u8 = 2;
/// 操作码：交叉。
pub const OP_CROSS: u8 = 3;
/// 操作码：读结果。
pub const OP_OBS: u8 = 4;

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

/// 一个已加载对象。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Obj {
    /// 对象号。
    pub id: u32,
    /// 加载偏置。
    pub bias: u32,
    /// 正文长度。
    pub len: u32,
}

/// 一段映射。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Map {
    /// 起点。
    pub start: u32,
    /// 终点。
    pub end: u32,
    /// 是否可执行。
    pub exec: bool,
    /// 归属对象号。
    pub id: u32,
}

impl Map {
    /// 区间长度。
    pub fn span(&self) -> u32 {
        self.end.saturating_sub(self.start)
    }
}

/// 交叉器。
#[derive(Clone, Copy, Debug)]
pub struct Cross {
    o: [Obj; CAP],
    on: usize,
    m: [Map; CAP],
    mn: usize,
    ran: bool,
    matched: [u32; CAP],
    matched_len: usize,
    unpaired: u32,
}

impl Cross {
    /// 空交叉器。
    pub const fn new() -> Cross {
        Cross {
            o: [Obj { id: 0, bias: 0, len: 0 }; CAP],
            on: 0,
            m: [Map { start: 0, end: 0, exec: false, id: 0 }; CAP],
            mn: 0,
            ran: false,
            matched: [0; CAP],
            matched_len: 0,
            unpaired: 0,
        }
    }

    /// 对象条数。
    pub fn obj_count(&self) -> usize {
        self.on
    }

    /// 映射条数。
    pub fn map_count(&self) -> usize {
        self.mn
    }

    /// 是否已交叉。
    pub fn is_ran(&self) -> bool {
        self.ran
    }

    /// 匹配上的对象条数。
    pub fn matched_count(&self) -> usize {
        self.matched_len
    }

    /// 无归属的映射条数。
    pub fn unpaired(&self) -> u32 {
        self.unpaired
    }

    /// 第 i 个匹配上的对象号。
    pub fn matched_id(&self, i: usize) -> Result<u32, u32> {
        if i >= self.matched_len {
            return Err(CR_ENOBS);
        }
        match self.matched.get(i) {
            Some(v) => Ok(*v),
            None => Err(CR_ENOBS),
        }
    }

    /// 第 i 个对象。
    pub fn obj_at(&self, i: usize) -> Option<Obj> {
        if i >= self.on {
            return None;
        }
        self.o.get(i).copied()
    }

    /// 第 i 段映射。
    pub fn map_at(&self, i: usize) -> Option<Map> {
        if i >= self.mn {
            return None;
        }
        self.m.get(i).copied()
    }

    /// 按号查对象。
    fn find_obj(&self, id: u32) -> Option<usize> {
        let mut i = 0usize;
        while i < self.on {
            match self.o.get(i) {
                Some(v) => {
                    if v.id == id {
                        return Some(i);
                    }
                }
                None => return None,
            }
            i += 1;
        }
        None
    }

    /// 找归属对象为 `id` 的可执行映射。
    fn find_exec_map(&self, id: u32) -> Option<Map> {
        let mut i = 0usize;
        while i < self.mn {
            match self.m.get(i) {
                Some(v) => {
                    if v.id == id && v.exec {
                        return Some(*v);
                    }
                }
                None => return None,
            }
            i += 1;
        }
        None
    }

    /// 收一个对象。
    pub fn obj(&mut self, id: u32, bias: u32, len: u32) -> Result<usize, u32> {
        if id == 0 {
            return Err(CR_EINVAL);
        }
        if self.ran {
            return Err(CR_ERUN);
        }
        if len == 0 {
            return Err(CR_EINVAL);
        }
        if self.on >= CAP {
            return Err(CR_EFULL);
        }
        if self.find_obj(id).is_some() {
            return Err(CR_EDUP);
        }
        if let Some(slot) = self.o.get_mut(self.on) {
            *slot = Obj { id, bias, len };
        }
        self.on += 1;
        Ok(self.on)
    }

    /// 收一段映射。
    pub fn map(&mut self, start: u32, end: u32, exec: bool, id: u32) -> Result<usize, u32> {
        if id == 0 {
            return Err(CR_EINVAL);
        }
        if self.ran {
            return Err(CR_ERUN);
        }
        if self.mn >= CAP {
            return Err(CR_EFULL);
        }
        if end <= start {
            return Err(CR_EINVAL);
        }
        if let Some(slot) = self.m.get_mut(self.mn) {
            *slot = Map { start, end, exec, id };
        }
        self.mn += 1;
        Ok(self.mn)
    }

    /// 交叉两份账：先按对象查映射，再扫孤儿映射。
    pub fn cross(&mut self) -> Result<usize, u32> {
        if self.ran {
            return Err(CR_ERUN);
        }
        self.ran = true;
        let mut k = 0usize;
        while k < self.on {
            let o = match self.o.get(k) {
                Some(v) => *v,
                None => return Err(CR_EBAL),
            };
            let m = match self.find_exec_map(o.id) {
                Some(m) => m,
                None => return Err(CR_ENOMAP),
            };
            if m.start != o.bias {
                return Err(CR_EBIAS);
            }
            if m.span() < o.len {
                return Err(CR_ENOPROT);
            }
            if let Some(slot) = self.matched.get_mut(self.matched_len) {
                *slot = o.id;
            }
            self.matched_len += 1;
            k += 1;
        }
        let mut i = 0usize;
        let mut orphans = 0u32;
        while i < self.mn {
            let m = match self.m.get(i) {
                Some(v) => *v,
                None => return Err(CR_EBAL),
            };
            if self.find_obj(m.id).is_none() {
                orphans = orphans.saturating_add(1);
            }
            i += 1;
        }
        if orphans != 0 {
            self.unpaired = orphans;
            return Err(CR_EORPHAN);
        }
        Ok(self.matched_len)
    }

    /// 匹配表是否按对象表顺序排列。
    pub fn consistent(&self) -> bool {
        let mut i = 0usize;
        while i < self.matched_len {
            let id = match self.matched.get(i) {
                Some(v) => *v,
                None => return false,
            };
            match self.o.get(i) {
                Some(o) => {
                    if o.id != id {
                        return false;
                    }
                }
                None => return false,
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
            return Err(CR_EORDER);
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
            return Err(CR_EBAL);
        }
        const VALS: [u32; 8] = [1, 2, 1, 2, 2, 1, 2, 0];
        const CODES: [u32; 6] =
            [CR_EINVAL, CR_EDUP, CR_EFULL, CR_ERUN, CR_ENOMAP, CR_EBIAS];
        let mut vi = 0usize;
        let mut ci = 0usize;
        let mut i = 0usize;
        while i < self.len {
            let s = match self.rows.get(i) {
                Some(v) => *v,
                None => return Err(CR_EBAL),
            };
            if s.slot as usize != i {
                return Err(CR_EORDER);
            }
            if s.ok {
                if s.code != 0 {
                    return Err(CR_EMISMATCH);
                }
                match VALS.get(vi) {
                    Some(w) => {
                        if s.value != *w {
                            return Err(CR_EMISMATCH);
                        }
                    }
                    None => return Err(CR_EBAL),
                }
                vi += 1;
            } else {
                match CODES.get(ci) {
                    Some(w) => {
                        if s.code != *w {
                            return Err(CR_EMISMATCH);
                        }
                    }
                    None => return Err(CR_EBAL),
                }
                ci += 1;
            }
            i += 1;
        }
        if vi != 8 || ci != 6 {
            return Err(CR_EBAL);
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
            return Err(CR_EBAL);
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
            Err(CR_ESEAL)
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

/// 十四格账本：两个对象 + 两段可执行映射 → 交叉 → 读匹配表 → 各类拒绝与失配。
pub fn build_ledger() -> Ledger {
    let mut l = Ledger::new();
    let mut c = Cross::new();

    let _ = l.register(cell(0, OP_OBJ, c.obj(1, 0x40_0000, 0x900)));
    let _ = l.register(cell(1, OP_OBJ, c.obj(2, 0x7f00_0000, 0x1200)));
    let _ = l.register(cell(2, OP_MAP, c.map(0x40_0000, 0x40_1000, true, 1)));
    let _ = l.register(cell(3, OP_MAP, c.map(0x7f00_0000, 0x7f00_2000, true, 2)));
    let _ = l.register(cell(4, OP_CROSS, c.cross()));
    let _ = l.register(word_cell(5, OP_OBS, c.matched_id(0)));
    let _ = l.register(word_cell(6, OP_OBS, c.matched_id(1)));
    let _ = l.register(word_cell(7, OP_OBS, Ok(c.unpaired())));

    let mut e = Cross::new();
    let _ = l.register(cell(8, OP_OBJ, e.obj(0, 0x1000, 0x100)));

    let mut d = Cross::new();
    let _ = d.obj(3, 0x1000, 0x100);
    let _ = l.register(cell(9, OP_OBJ, d.obj(3, 0x2000, 0x100)));

    let mut full = Cross::new();
    let mut i = 1u32;
    while (i as usize) <= CAP {
        let _ = full.obj(i, i * 0x1000, 0x100);
        i += 1;
    }
    let _ = l.register(cell(10, OP_OBJ, full.obj(0x100, 0x1000, 0x100)));

    let _ = l.register(cell(11, OP_CROSS, c.cross()));

    let mut nm = Cross::new();
    let _ = nm.obj(4, 0x1000, 0x100);
    let _ = l.register(cell(12, OP_CROSS, nm.cross()));

    let mut bi = Cross::new();
    let _ = bi.obj(5, 0x1000, 0x100);
    let _ = bi.map(0x2000, 0x3000, true, 5);
    let _ = l.register(cell(13, OP_CROSS, bi.cross()));

    l
}

/// 四路注入。
pub fn inject_four() -> [bool; 4] {
    let mut out = [false; 4];

    let l = build_ledger();
    let rep = l.replay();
    let root = l.seal();
    out[0] = l.len() == SAMPLES && l.audit().is_ok() && rep == Ok(root) && root != 0;

    // 一路：全配对齐，匹配表按对象顺序。
    let mut a = Cross::new();
    let _ = a.obj(11, 0x1000, 0x200);
    let _ = a.obj(12, 0x9000, 0x400);
    let _ = a.map(0x1000, 0x2000, true, 11);
    let _ = a.map(0x9000, 0xa000, true, 12);
    let _ = a.map(0x9000, 0xa000, false, 12);
    let r = a.cross();
    out[1] = r == Ok(2)
        && a.matched_count() == 2
        && a.matched_id(0) == Ok(11)
        && a.matched_id(1) == Ok(12)
        && a.matched_id(2) == Err(CR_ENOBS)
        && a.unpaired() == 0
        && a.consistent();

    // 二路：三种失配各自可辨（缺映射 / 偏置不符 / 孤儿映射）。
    let mut b = Cross::new();
    let _ = b.obj(21, 0x1000, 0x100);
    let miss = b.cross();
    let mut c2 = Cross::new();
    let _ = c2.obj(22, 0x1000, 0x100);
    let _ = c2.map(0x2000, 0x3000, true, 22);
    let bias = c2.cross();
    let mut c3 = Cross::new();
    let _ = c3.obj(23, 0x1000, 0x100);
    let _ = c3.map(0x1000, 0x2000, true, 23);
    let _ = c3.map(0x5000, 0x6000, true, 77);
    let orphan = c3.cross();
    out[2] = miss == Err(CR_ENOMAP)
        && bias == Err(CR_EBIAS)
        && orphan == Err(CR_EORPHAN)
        && c3.unpaired() == 1;

    // 三路：闸门（零号 / 重复 / 表满 / 交叉后封闭）。
    let mut g = Cross::new();
    let g1 = g.obj(0, 0x1000, 0x100) == Err(CR_EINVAL);
    let g2 = g.map(0x1000, 0x2000, true, 0) == Err(CR_EINVAL);
    let g3 = g.obj(1, 0x1000, 0x100) == Ok(1);
    let g4 = g.obj(1, 0x2000, 0x100) == Err(CR_EDUP);
    let g5 = g.map(0x1000, 0x2000, true, 1) == Ok(1);
    let g6 = g.cross() == Ok(1);
    let g7 = g.obj(2, 0x3000, 0x100) == Err(CR_ERUN);
    let g8 = g.map(0x3000, 0x4000, true, 2) == Err(CR_ERUN);
    let g9 = g.cross() == Err(CR_ERUN);
    out[3] = g1 && g2 && g3 && g4 && g5 && g6 && g7 && g8 && g9;

    out
}

/// 十三项判据。
pub fn run_unx11351_checks() -> CheckSet {
    let mut s = CheckSet::new("unx11351.maps_phdr_cross");
    let l = build_ledger();

    s.add_dyn(
        "CR1-十四格八允六拒",
        l.len() == SAMPLES && l.ok_count() == 8 && l.err_count() == 6 && l.audit().is_ok(),
        &format!("n={} ok={} err={}", l.len(), l.ok_count(), l.err_count()),
    );

    s.add(
        "CR2-两对象两映射",
        l.sample(0).map(|x| x.value) == Some(1)
            && l.sample(1).map(|x| x.value) == Some(2)
            && l.sample(2).map(|x| x.value) == Some(1)
            && l.sample(3).map(|x| x.value) == Some(2),
        "static",
    );

    s.add(
        "CR3-交叉匹配两条",
        l.sample(4).map(|x| x.value) == Some(2),
        "static",
    );

    s.add(
        "CR4-匹配表按对象序",
        l.sample(5).map(|x| x.value) == Some(1) && l.sample(6).map(|x| x.value) == Some(2),
        "static",
    );

    s.add(
        "CR5-零孤儿",
        l.sample(7).map(|x| x.value) == Some(0),
        "static",
    );

    s.add(
        "CR6-零号对象被拒",
        l.sample(8).map(|x| x.code) == Some(CR_EINVAL),
        "static",
    );

    s.add(
        "CR7-重复与表满",
        l.sample(9).map(|x| x.code) == Some(CR_EDUP)
            && l.sample(10).map(|x| x.code) == Some(CR_EFULL),
        "static",
    );

    s.add(
        "CR8-重复交叉被拒",
        l.sample(11).map(|x| x.code) == Some(CR_ERUN),
        "static",
    );

    s.add(
        "CR9-缺映射与偏置不符",
        l.sample(12).map(|x| x.code) == Some(CR_ENOMAP)
            && l.sample(13).map(|x| x.code) == Some(CR_EBIAS),
        "static",
    );

    s.add(
        "CR10-孤儿映射被点名",
        {
            let mut a = Cross::new();
            let _ = a.obj(1, 0x1000, 0x100);
            let _ = a.map(0x1000, 0x2000, true, 1);
            let _ = a.map(0x8000, 0x9000, false, 9);
            let r = a.cross();
            r == Err(CR_EORPHAN) && a.unpaired() == 1
        },
        "static",
    );

    s.add(
        "CR11-不可执行映射不算数",
        {
            let mut a = Cross::new();
            let _ = a.obj(1, 0x1000, 0x100);
            let _ = a.map(0x1000, 0x2000, false, 1);
            a.cross() == Err(CR_ENOMAP)
        },
        "static",
    );

    s.add(
        "CR12-区间倒置被拒",
        {
            let mut a = Cross::new();
            let _ = a.obj(1, 0x1000, 0x100);
            let bad = a.map(0x3000, 0x3000, true, 1);
            let short = a.map(0x1000, 0x1080, true, 1);
            bad == Err(CR_EINVAL) && short == Ok(1) && a.cross() == Err(CR_ENOPROT)
        },
        "static",
    );

    s.add(
        "CR13-冻结重放四注入",
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
        let s = run_unx11351_checks();
        assert_eq!(s.len(), 13);
        assert!(s.all_passed());
    }

    #[test]
    fn matching_pair_passes() {
        let mut c = Cross::new();
        let _ = c.obj(1, 0x1000, 0x100);
        let _ = c.map(0x1000, 0x2000, true, 1);
        assert_eq!(c.cross(), Ok(1));
        assert_eq!(c.matched_id(0), Ok(1));
        assert_eq!(c.unpaired(), 0);
        assert!(c.consistent());
    }

    #[test]
    fn missing_map_is_named() {
        let mut c = Cross::new();
        let _ = c.obj(1, 0x1000, 0x100);
        assert_eq!(c.cross(), Err(CR_ENOMAP));
        assert!(c.is_ran());
    }

    #[test]
    fn bias_mismatch_is_named() {
        let mut c = Cross::new();
        let _ = c.obj(1, 0x1000, 0x100);
        let _ = c.map(0x2000, 0x3000, true, 1);
        assert_eq!(c.cross(), Err(CR_EBIAS));
    }

    #[test]
    fn orphan_is_counted() {
        let mut c = Cross::new();
        let _ = c.obj(1, 0x1000, 0x100);
        let _ = c.map(0x1000, 0x2000, true, 1);
        let _ = c.map(0x4000, 0x5000, true, 4);
        assert_eq!(c.cross(), Err(CR_EORPHAN));
        assert_eq!(c.unpaired(), 1);
    }

    #[test]
    fn gates() {
        let mut c = Cross::new();
        assert_eq!(c.obj(0, 0x1000, 0x100), Err(CR_EINVAL));
        assert_eq!(c.obj(1, 0x1000, 0), Err(CR_EINVAL));
        assert_eq!(c.map(0x1000, 0x1000, true, 1), Err(CR_EINVAL));
        assert_eq!(c.obj(1, 0x1000, 0x100), Ok(1));
        assert_eq!(c.obj(1, 0x2000, 0x100), Err(CR_EDUP));
        assert_eq!(c.map(0x1000, 0x2000, true, 1), Ok(1));
        assert_eq!(c.map(0x1000, 0x2000, true, 1), Ok(2));
        assert_eq!(c.cross(), Ok(1));
        assert_eq!(c.obj(2, 0x3000, 0x100), Err(CR_ERUN));
    }

    #[test]
    fn capacity_is_per_table() {
        let mut c = Cross::new();
        let mut i = 1u32;
        while (i as usize) <= CAP {
            assert!(c.obj(i, i * 0x1000, 0x100).is_ok());
            i += 1;
        }
        assert_eq!(c.obj(99, 0x1000, 0x100), Err(CR_EFULL));
        let mut m = Cross::new();
        let mut k = 1u32;
        while (k as usize) <= CAP {
            assert!(m.map(k * 0x1000, k * 0x1000 + 0x1000, true, k).is_ok());
            k += 1;
        }
        assert_eq!(m.map(0x1000, 0x2000, true, 1), Err(CR_EFULL));
    }

    #[test]
    fn freeze_blocks_writes() {
        let mut l = build_ledger();
        assert!(l.freeze().is_ok());
        assert!(l.register(Sample::blank()).is_err());
        assert_eq!(l.rejects(), 1);
    }
}
