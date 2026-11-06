//! UNX-F11364 dlsym 句柄查找语义（0xecc0 段）
//!
//! `dlsym(handle, name)` 的查找范围不是「这个对象自己」，而是**这个对象的
//! 依赖闭包**：glibc 为每个句柄维护一张作用域表，表的构造是**广度优先**遍历
//! 依赖图（先本体，再一层依赖，再二层依赖……），同层按链接顺序。于是当两层
//! 依赖都导出同名符号时，浅层的那个胜出——这是 dlsym 最容易被写成深度优先的
//! 地方，也是本模块把 BFS 与 DFS 的差别做成可观测判据的原因。
//!
//! 查找命中返回**导出者句柄**（不是地址）：调用方后续要拿这个句柄去查绑定态，
//! 句柄比地址更能说明「符号是谁给的」。全链都没有时返回 `ENOSYM`，与「句柄
//! 本身无效」（`EBADH`）严格分开：前者是符号问题，后者是调用方拿错了句柄。
//!
//! 依赖挂接有三条硬约束：
//!
//! 1. 依赖必须比本体先装载（装载序即句柄序），否则 `EDEP`；
//! 2. 不能自依赖、不能重复挂接同一条依赖，否则 `EDEP`；
//! 3. 依赖链深度不得超过 `MAX_DEPTH`。深度在挂接时增量计算（本体深度 =
//!    1 + 所有依赖深度的最大值），超限直接拒绝，因此表里任何时刻都不存在
//!    一条超深链——不是查询时才截断。
//!
//! 闸门次序：
//! - `load`：零键 → 无此映像 → 已装载 → 表满；
//! - `link`：本体句柄 → 依赖句柄 → 自依赖 → 逆序 → 重复 → 超深；
//! - `dlsym`：句柄为零/失效 → 符号号 ≥32 → BFS（本体 → 各层依赖）。

use crate::alloc::format;
use crate::checks::CheckSet;

/// 段基址。
pub const SEG_BASE: u32 = 0xecc0;

/// 映像键为零或符号号非法。
pub const DH_EINVAL: u32 = SEG_BASE + 0x01;
/// 目录中没有这条映像。
pub const DH_ENOENT: u32 = SEG_BASE + 0x02;
/// 依赖闭包内没有导出该符号。
pub const DH_ENOSYM: u32 = SEG_BASE + 0x03;
/// 句柄无效。
pub const DH_EBADH: u32 = SEG_BASE + 0x04;
/// 对象表已满。
pub const DH_EFULL: u32 = SEG_BASE + 0x05;
/// 依赖关系非法（自依赖 / 逆序 / 重复）。
pub const DH_EDEP: u32 = SEG_BASE + 0x06;
/// 同一映像重复装载。
pub const DH_EDUP: u32 = SEG_BASE + 0x07;
/// 依赖链超过深度上限。
pub const DH_EDEPTH: u32 = SEG_BASE + 0x08;
/// 账本形状不符。
pub const DH_EBAL: u32 = SEG_BASE + 0x09;
/// 槽位乱序。
pub const DH_EORDER: u32 = SEG_BASE + 0x0a;
/// 观测值与复算不符。
pub const DH_EMISMATCH: u32 = SEG_BASE + 0x0b;
/// 冻结后重放不一致。
pub const DH_ESEAL: u32 = SEG_BASE + 0x0c;
/// 观察下标越界。
pub const DH_ENOBS: u32 = SEG_BASE + 0x0d;

/// 目录容量。
pub const ICAP: usize = 8;
/// 对象容量。
pub const CAP: usize = 6;
/// 单对象依赖上限。
pub const DEPS: usize = 3;
/// 依赖链深度上限（本体算第一层）。
pub const MAX_DEPTH: u8 = 4;
/// 账本格数。
pub const SAMPLES: usize = 14;

/// 操作码：装载。
pub const OP_LOAD: u8 = 1;
/// 操作码：挂接依赖。
pub const OP_LINK: u8 = 2;
/// 操作码：查符号。
pub const OP_DLSYM: u8 = 3;
/// 操作码：读依赖条数。
pub const OP_DEPS: u8 = 4;
/// 操作码：读链深。
pub const OP_DEPTH: u8 = 5;

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

/// 目录映像。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Img {
    /// 映像键。
    pub key: u32,
    /// 导出位图。
    pub exports: u32,
}

impl Img {
    /// 造映像。
    pub const fn make(key: u32, exports: u32) -> Img {
        Img { key, exports }
    }

    /// 是否导出 sym。
    pub fn exports(&self, sym: u32) -> bool {
        sym < 32 && (self.exports & (1u32 << sym)) != 0
    }
}

/// 已装载对象。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Obj {
    /// 映像键。
    pub key: u32,
    /// 句柄。
    pub handle: u32,
    /// 依赖句柄。
    pub deps: [u32; DEPS],
    /// 依赖条数。
    pub ndeps: u8,
    /// 是否在用。
    pub live: bool,
}

impl Obj {
    /// 空对象。
    pub const fn blank() -> Obj {
        Obj { key: 0, handle: 0, deps: [0; DEPS], ndeps: 0, live: false }
    }

    /// 是否已挂接某依赖。
    pub fn has_dep(&self, h: u32) -> bool {
        let mut i = 0usize;
        while i < self.ndeps as usize {
            if let Some(d) = self.deps.get(i) {
                if *d == h {
                    return true;
                }
            }
            i += 1;
        }
        false
    }
}

/// 句柄表。
#[derive(Clone, Copy, Debug)]
pub struct Handles {
    cat: [Img; ICAP],
    ncat: usize,
    obj: [Obj; CAP],
    next_handle: u32,
    searches: u32,
    hops: u32,
}

impl Handles {
    /// 空表。
    pub const fn new() -> Handles {
        Handles {
            cat: [Img::make(0, 0); ICAP],
            ncat: 0,
            obj: [Obj::blank(); CAP],
            next_handle: 1,
            searches: 0,
            hops: 0,
        }
    }

    /// 内置目录：A 导出 s2；B 导出 s0；C 导出 s1；D 导出 s0,s1。
    pub fn seeded() -> Handles {
        let mut h = Handles::new();
        let _ = h.add_img(Img::make(0x10, 0b0100));
        let _ = h.add_img(Img::make(0x11, 0b0001));
        let _ = h.add_img(Img::make(0x12, 0b0010));
        let _ = h.add_img(Img::make(0x13, 0b0011));
        h
    }

    /// 登记目录。
    pub fn add_img(&mut self, img: Img) -> Result<usize, u32> {
        if img.key == 0 {
            return Err(DH_EINVAL);
        }
        if self.find_img(img.key).is_some() {
            return Err(DH_EDUP);
        }
        if self.ncat >= ICAP {
            return Err(DH_EFULL);
        }
        if let Some(dst) = self.cat.get_mut(self.ncat) {
            *dst = img;
        }
        self.ncat += 1;
        Ok(self.ncat)
    }

    /// 查目录。
    pub fn find_img(&self, key: u32) -> Option<Img> {
        let mut i = 0usize;
        while i < self.ncat {
            match self.cat.get(i) {
                Some(img) => {
                    if img.key == key {
                        return Some(*img);
                    }
                }
                None => return None,
            }
            i += 1;
        }
        None
    }

    /// 查找次数。
    pub fn search_count(&self) -> u32 {
        self.searches
    }

    /// 遍历过的节点总数（BFS 跳数）。
    pub fn hop_count(&self) -> u32 {
        self.hops
    }

    /// 句柄水位。
    pub fn handle_high_water(&self) -> u32 {
        self.next_handle
    }

    /// 在用对象数。
    pub fn object_count(&self) -> usize {
        let mut k = 0usize;
        let mut i = 0usize;
        while i < CAP {
            if let Some(o) = self.obj.get(i) {
                if o.live {
                    k += 1;
                }
            }
            i += 1;
        }
        k
    }

    fn slot_of(&self, handle: u32) -> Option<usize> {
        let mut i = 0usize;
        while i < CAP {
            match self.obj.get(i) {
                Some(o) => {
                    if o.live && o.handle == handle {
                        return Some(i);
                    }
                }
                None => return None,
            }
            i += 1;
        }
        None
    }

    fn slot_by_key(&self, key: u32) -> Option<usize> {
        let mut i = 0usize;
        while i < CAP {
            match self.obj.get(i) {
                Some(o) => {
                    if o.live && o.key == key {
                        return Some(i);
                    }
                }
                None => return None,
            }
            i += 1;
        }
        None
    }

    fn free_slot(&self) -> Option<usize> {
        let mut i = 0usize;
        while i < CAP {
            match self.obj.get(i) {
                Some(o) => {
                    if !o.live {
                        return Some(i);
                    }
                }
                None => return None,
            }
            i += 1;
        }
        None
    }

    /// 装载映像。
    pub fn load(&mut self, key: u32) -> Result<usize, u32> {
        if key == 0 {
            return Err(DH_EINVAL);
        }
        if self.find_img(key).is_none() {
            return Err(DH_ENOENT);
        }
        if self.slot_by_key(key).is_some() {
            return Err(DH_EDUP);
        }
        let slot = match self.free_slot() {
            Some(i) => i,
            None => return Err(DH_EFULL),
        };
        let handle = self.next_handle;
        if let Some(o) = self.obj.get_mut(slot) {
            *o = Obj { key, handle, deps: [0; DEPS], ndeps: 0, live: true };
        }
        self.next_handle = self.next_handle.saturating_add(1);
        Ok(handle as usize)
    }

    /// 挂接依赖，返回本体深度。
    pub fn link(&mut self, handle: u32, dep: u32) -> Result<usize, u32> {
        let hi = match self.slot_of(handle) {
            Some(i) => i,
            None => return Err(DH_EBADH),
        };
        if self.slot_of(dep).is_none() {
            return Err(DH_EBADH);
        }
        if handle == dep {
            return Err(DH_EDEP);
        }
        if dep >= handle {
            return Err(DH_EDEP);
        }
        let existing = match self.obj.get(hi) {
            Some(o) => o.has_dep(dep),
            None => return Err(DH_EBADH),
        };
        if existing {
            return Err(DH_EDEP);
        }
        let n = match self.obj.get(hi) {
            Some(o) => o.ndeps as usize,
            None => return Err(DH_EBADH),
        };
        if n >= DEPS {
            return Err(DH_EDEP);
        }
        let ddep = self.depth_from(dep);
        if ddep.saturating_add(1) > MAX_DEPTH {
            return Err(DH_EDEPTH);
        }
        if let Some(o) = self.obj.get_mut(hi) {
            if let Some(dst) = o.deps.get_mut(n) {
                *dst = dep;
            }
            o.ndeps = o.ndeps.saturating_add(1);
        }
        match self.obj.get(hi) {
            Some(o) => Ok(o.ndeps as usize),
            None => Err(DH_EBADH),
        }
    }

    /// 读对象。
    pub fn object_of(&self, handle: u32) -> Result<Obj, u32> {
        if handle == 0 {
            return Err(DH_EBADH);
        }
        match self.slot_of(handle) {
            Some(i) => match self.obj.get(i) {
                Some(o) => Ok(*o),
                None => Err(DH_EBADH),
            },
            None => Err(DH_EBADH),
        }
    }

    /// 依赖条数。
    pub fn dep_count(&self, handle: u32) -> Result<usize, u32> {
        let o = self.object_of(handle)?;
        Ok(o.ndeps as usize)
    }

    /// 依赖闭包的最大层数（本体算第一层），按当前依赖图现算。
    pub fn depth_of(&self, handle: u32) -> Result<u32, u32> {
        let _ = self.object_of(handle)?;
        Ok(self.depth_from(handle) as u32)
    }

    /// 广度优先数层：本体 1，一层依赖 2，依此类推。
    fn depth_from(&self, handle: u32) -> u8 {
        let mut queue = [(0u32, 1u8); CAP];
        let mut visited = [false; CAP + 1];
        let mut head = 0usize;
        let mut tail = 0usize;
        let mut max = 0u8;
        if let Some(dst) = queue.get_mut(tail) {
            *dst = (handle, 1);
        }
        tail += 1;
        if let Some(v) = visited.get_mut(handle as usize) {
            *v = true;
        }
        while head < tail {
            let (cur, level) = match queue.get(head) {
                Some(v) => *v,
                None => break,
            };
            head += 1;
            if level > max {
                max = level;
            }
            let ob = match self.object_of(cur) {
                Ok(v) => v,
                Err(_) => continue,
            };
            let mut d = 0usize;
            while d < ob.ndeps as usize {
                let dep = match ob.deps.get(d) {
                    Some(v) => *v,
                    None => break,
                };
                let seen = match visited.get(dep as usize) {
                    Some(v) => *v,
                    None => true,
                };
                if !seen && level < MAX_DEPTH.saturating_add(1) {
                    if let Some(slot) = visited.get_mut(dep as usize) {
                        *slot = true;
                    }
                    if let Some(dst) = queue.get_mut(tail) {
                        *dst = (dep, level.saturating_add(1));
                    }
                    tail += 1;
                }
                d += 1;
            }
        }
        max
    }

    /// 广度优先查找：本体 → 一层依赖 → 二层依赖……同层按链接序。
    pub fn dlsym(&mut self, handle: u32, sym: u32) -> Result<u32, u32> {
        let root = self.object_of(handle)?;
        if sym >= 32 {
            return Err(DH_EINVAL);
        }
        self.searches = self.searches.saturating_add(1);
        let mut queue = [0u32; CAP];
        let mut visited = [false; CAP + 1];
        let mut head = 0usize;
        let mut tail = 0usize;
        if let Some(dst) = queue.get_mut(tail) {
            *dst = handle;
        }
        tail += 1;
        if let Some(v) = visited.get_mut(handle as usize) {
            *v = true;
        }
        let _ = root;
        while head < tail {
            let cur = match queue.get(head) {
                Some(v) => *v,
                None => break,
            };
            head += 1;
            self.hops = self.hops.saturating_add(1);
            let ob = match self.object_of(cur) {
                Ok(v) => v,
                Err(_) => continue,
            };
            if let Some(img) = self.find_img(ob.key) {
                if img.exports(sym) {
                    return Ok(cur);
                }
            }
            let mut d = 0usize;
            while d < ob.ndeps as usize {
                let dep = match ob.deps.get(d) {
                    Some(v) => *v,
                    None => break,
                };
                let seen = match visited.get(dep as usize) {
                    Some(v) => *v,
                    None => true,
                };
                if !seen {
                    if let Some(slot) = visited.get_mut(dep as usize) {
                        *slot = true;
                    }
                    if let Some(dst) = queue.get_mut(tail) {
                        *dst = dep;
                    }
                    tail += 1;
                }
                d += 1;
            }
        }
        Err(DH_ENOSYM)
    }

    /// 查找时是否只看自己（调试用：单节点作用域）。
    pub fn self_exports(&self, handle: u32, sym: u32) -> bool {
        match self.object_of(handle) {
            Ok(o) => match self.find_img(o.key) {
                Some(img) => img.exports(sym),
                None => false,
            },
            Err(_) => false,
        }
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
            return Err(DH_EORDER);
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
            return Err(DH_EBAL);
        }
        const VALS: [u32; 8] = [1, 2, 3, 1, 2, 3, 1, 2];
        const CODES: [u32; 6] =
            [DH_EINVAL, DH_ENOSYM, DH_EDEP, DH_EBADH, DH_EDUP, DH_EFULL];
        let mut vi = 0usize;
        let mut ci = 0usize;
        let mut i = 0usize;
        while i < self.len {
            let s = match self.rows.get(i) {
                Some(v) => *v,
                None => return Err(DH_EBAL),
            };
            if s.slot as usize != i {
                return Err(DH_EORDER);
            }
            if s.ok {
                if s.code != 0 {
                    return Err(DH_EMISMATCH);
                }
                match VALS.get(vi) {
                    Some(w) => {
                        if s.value != *w {
                            return Err(DH_EMISMATCH);
                        }
                    }
                    None => return Err(DH_EBAL),
                }
                vi += 1;
            } else {
                match CODES.get(ci) {
                    Some(w) => {
                        if s.code != *w {
                            return Err(DH_EMISMATCH);
                        }
                    }
                    None => return Err(DH_EBAL),
                }
                ci += 1;
            }
            i += 1;
        }
        if vi != 8 || ci != 6 {
            return Err(DH_EBAL);
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
            return Err(DH_EBAL);
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
            Err(DH_ESEAL)
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

/// 十四格账本：三装载 → 两挂接 → 自身/一层/二层符号 → 六类拒绝。
pub fn build_ledger() -> Ledger {
    let mut l = Ledger::new();
    let mut h = Handles::seeded();

    let _ = l.register(cell(0, OP_LOAD, h.load(0x11)));
    let _ = l.register(cell(1, OP_LOAD, h.load(0x12)));
    let _ = l.register(cell(2, OP_LOAD, h.load(0x10)));
    let _ = l.register(cell(3, OP_LINK, h.link(3, 1)));
    let _ = l.register(cell(4, OP_LINK, h.link(3, 2)));
    let _ = l.register(word_cell(5, OP_DLSYM, h.dlsym(3, 2)));
    let _ = l.register(word_cell(6, OP_DLSYM, h.dlsym(3, 0)));
    let _ = l.register(word_cell(7, OP_DLSYM, h.dlsym(3, 1)));

    let _ = l.register(cell(8, OP_LOAD, h.load(0)));

    let _ = l.register(word_cell(9, OP_DLSYM, h.dlsym(3, 30)));

    let _ = l.register(cell(10, OP_LINK, h.link(3, 3)));

    let _ = l.register(word_cell(11, OP_DLSYM, h.dlsym(0, 0)));

    let _ = l.register(cell(12, OP_LOAD, h.load(0x11)));

    let mut f = Handles::new();
    let mut i = 0u32;
    while (i as usize) < ICAP {
        let _ = f.add_img(Img::make(0xA0 + i, 1));
        i += 1;
    }
    let mut k = 0u32;
    while (k as usize) < CAP {
        let _ = f.load(0xA0 + k);
        k += 1;
    }
    let _ = l.register(cell(13, OP_LOAD, f.load(0xA0 + (CAP as u32))));

    l
}

/// 四路注入。
pub fn inject_four() -> [bool; 4] {
    let mut out = [false; 4];

    let l = build_ledger();
    let rep = l.replay();
    let root = l.seal();
    out[0] = l.len() == SAMPLES && l.audit().is_ok() && rep == Ok(root) && root != 0;

    // 一路：本体 → 一层 → 二层，逐层可达。
    let mut a = Handles::seeded();
    let _ = a.load(0x11);
    let _ = a.load(0x12);
    let _ = a.load(0x10);
    let _ = a.link(3, 1);
    let _ = a.link(3, 2);
    out[1] = a.dlsym(3, 2) == Ok(3)
        && a.dlsym(3, 0) == Ok(1)
        && a.dlsym(3, 1) == Ok(2)
        && a.dep_count(3) == Ok(2)
        && a.depth_of(3) == Ok(2)
        && a.dlsym(2, 0) == Err(DH_ENOSYM)
        && a.object_count() == 3;

    // 二路：广度优先——浅层依赖胜过深层依赖（深度优先会错取 2）。
    let mut b = Handles::seeded();
    let _ = b.add_img(Img::make(0x16, 0));
    let _ = b.load(0x12);
    let _ = b.load(0x13);
    let _ = b.load(0x11);
    let _ = b.load(0x16);
    let _ = b.link(4, 3);
    let _ = b.link(4, 1);
    let _ = b.link(3, 2);
    let breadth = b.dlsym(4, 1);
    let shallow = b.dlsym(4, 0);
    out[2] = breadth == Ok(1)
        && shallow == Ok(3)
        && b.depth_of(4) == Ok(3)
        && b.depth_of(3) == Ok(2)
        && b.depth_of(2) == Ok(1)
        && b.search_count() == 2
        && b.hop_count() == 5;

    // 三路：作用域隔离与闸门（自查找不算依赖）。
    let mut c = Handles::seeded();
    let _ = c.load(0x10);
    let _ = c.load(0x13);
    let self_only = c.dlsym(1, 0);
    let bad = c.dlsym(9, 0);
    let zero = c.dlsym(0, 0);
    let wide = c.dlsym(1, 40);
    out[3] = self_only == Err(DH_ENOSYM)
        && bad == Err(DH_EBADH)
        && zero == Err(DH_EBADH)
        && wide == Err(DH_EINVAL)
        && c.link(1, 1) == Err(DH_EDEP)
        && c.link(1, 2) == Err(DH_EDEP)
        && c.link(2, 1) == Ok(1)
        && c.link(2, 1) == Err(DH_EDEP);

    out
}

/// 十三项判据。
pub fn run_unx11364_checks() -> CheckSet {
    let mut s = CheckSet::new("unx11364.dlsym_handle");
    let l = build_ledger();

    s.add_dyn(
        "DH1-十四格八允六拒",
        l.len() == SAMPLES && l.ok_count() == 8 && l.err_count() == 6 && l.audit().is_ok(),
        &format!("n={} ok={} err={}", l.len(), l.ok_count(), l.err_count()),
    );

    s.add(
        "DH2-句柄按装载序递增",
        l.sample(0).map(|x| x.value) == Some(1)
            && l.sample(1).map(|x| x.value) == Some(2)
            && l.sample(2).map(|x| x.value) == Some(3),
        "static",
    );

    s.add(
        "DH3-依赖挂接条数递增",
        l.sample(3).map(|x| x.value) == Some(1)
            && l.sample(4).map(|x| x.value) == Some(2),
        "static",
    );

    s.add(
        "DH4-本体导出优先命中",
        l.sample(5).map(|x| x.value) == Some(3),
        "static",
    );

    s.add(
        "DH5-一层依赖按链接序命中",
        l.sample(6).map(|x| x.value) == Some(1)
            && l.sample(7).map(|x| x.value) == Some(2),
        "static",
    );

    s.add(
        "DH6-零键装载被拒",
        l.sample(8).map(|x| x.code) == Some(DH_EINVAL),
        "static",
    );

    s.add(
        "DH7-全链无此符号",
        l.sample(9).map(|x| x.code) == Some(DH_ENOSYM),
        "static",
    );

    s.add(
        "DH8-自依赖被拒",
        l.sample(10).map(|x| x.code) == Some(DH_EDEP),
        "static",
    );

    s.add(
        "DH9-野句柄查找被拒",
        l.sample(11).map(|x| x.code) == Some(DH_EBADH),
        "static",
    );

    s.add(
        "DH10-重复装载被拒",
        l.sample(12).map(|x| x.code) == Some(DH_EDUP),
        "static",
    );

    s.add(
        "DH11-对象表满被拒",
        l.sample(13).map(|x| x.code) == Some(DH_EFULL),
        "static",
    );

    s.add(
        "DH12-广度优先与深度上限",
        {
            let mut b = Handles::seeded();
            let _ = b.add_img(Img::make(0x16, 0));
            let _ = b.load(0x12);
            let _ = b.load(0x13);
            let _ = b.load(0x11);
            let _ = b.load(0x16);
            let _ = b.link(4, 3);
            let _ = b.link(4, 1);
            let _ = b.link(3, 2);
            let bfs = b.dlsym(4, 1);
            let mut d = Handles::seeded();
            let _ = d.add_img(Img::make(0x15, 0b1000));
            let _ = d.load(0x10);
            let _ = d.load(0x11);
            let _ = d.load(0x12);
            let _ = d.load(0x13);
            let _ = d.load(0x15);
            let _ = d.link(2, 1);
            let _ = d.link(3, 2);
            let _ = d.link(4, 3);
            let deep = d.link(5, 4);
            bfs == Ok(1)
                && b.dlsym(4, 0) == Ok(3)
                && deep == Err(DH_EDEPTH)
                && d.depth_of(4) == Ok(4)
                && d.dep_count(5) == Ok(0)
                && d.object_count() == 5
        },
        "static",
    );

    s.add(
        "DH13-冻结重放四注入",
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
        let s = run_unx11364_checks();
        assert_eq!(s.len(), 13);
        assert!(s.all_passed());
    }

    #[test]
    fn breadth_beats_depth() {
        let mut h = Handles::seeded();
        let _ = h.add_img(Img::make(0x16, 0));
        let _ = h.load(0x12);
        let _ = h.load(0x13);
        let _ = h.load(0x11);
        let _ = h.load(0x16);
        let _ = h.link(4, 3);
        let _ = h.link(4, 1);
        let _ = h.link(3, 2);
        assert_eq!(h.dlsym(4, 1), Ok(1));
        assert_eq!(h.dlsym(4, 0), Ok(3));
        assert_eq!(h.hop_count(), 5);
        assert_eq!(h.depth_of(4), Ok(3));
    }

    #[test]
    fn depth_cap_is_incremental() {
        let mut h = Handles::seeded();
        let _ = h.add_img(Img::make(0x15, 0b1000));
        let _ = h.load(0x10);
        let _ = h.load(0x11);
        let _ = h.load(0x12);
        let _ = h.load(0x13);
        let _ = h.load(0x15);
        assert_eq!(h.link(2, 1), Ok(1));
        assert_eq!(h.depth_of(2), Ok(2));
        assert_eq!(h.link(3, 2), Ok(1));
        assert_eq!(h.depth_of(3), Ok(3));
        assert_eq!(h.link(4, 3), Ok(1));
        assert_eq!(h.depth_of(4), Ok(4));
        assert_eq!(h.link(5, 4), Err(DH_EDEPTH));
        assert_eq!(h.dep_count(5), Ok(0));
    }

    #[test]
    fn dependency_gates() {
        let mut h = Handles::seeded();
        let _ = h.load(0x10);
        let _ = h.load(0x11);
        assert_eq!(h.link(1, 1), Err(DH_EDEP));
        assert_eq!(h.link(1, 2), Err(DH_EDEP));
        assert_eq!(h.link(2, 1), Ok(1));
        assert_eq!(h.link(2, 1), Err(DH_EDEP));
        assert_eq!(h.link(2, 8), Err(DH_EBADH));
        assert_eq!(h.link(0, 1), Err(DH_EBADH));
    }

    #[test]
    fn lookup_gates() {
        let mut h = Handles::seeded();
        let _ = h.load(0x10);
        assert_eq!(h.dlsym(0, 0), Err(DH_EBADH));
        assert_eq!(h.dlsym(1, 32), Err(DH_EINVAL));
        assert_eq!(h.dlsym(1, 0), Err(DH_ENOSYM));
        assert_eq!(h.dlsym(2, 0), Err(DH_EBADH));
        assert!(h.self_exports(1, 2));
        assert!(!h.self_exports(1, 0));
    }

    #[test]
    fn capacity_is_exact() {
        let mut h = Handles::new();
        let mut i = 0u32;
        while (i as usize) < ICAP {
            let _ = h.add_img(Img::make(0xB0 + i, 1));
            i += 1;
        }
        let mut k = 0u32;
        while (k as usize) < CAP {
            assert!(h.load(0xB0 + k).is_ok());
            k += 1;
        }
        assert_eq!(h.load(0xB0 + (CAP as u32)), Err(DH_EFULL));
        assert_eq!(h.object_count(), CAP);
    }
}
