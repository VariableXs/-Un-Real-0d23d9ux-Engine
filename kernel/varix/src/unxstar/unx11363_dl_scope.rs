//! UNX-F11363 RTLD_GLOBAL 与 RTLD_LOCAL 命名空间语义（0xecb0 段）
//!
//! `dlopen` 的 `RTLD_GLOBAL` / `RTLD_LOCAL` 决定的不是「能不能被找到」，而是
//! **这个对象的导出符号进不进全局域**：
//!
//! - `RTLD_GLOBAL`：对象装载后，其导出符号进入进程的全局符号域，此后装载的
//!   对象都能解析到它；
//! - `RTLD_LOCAL`：导出符号只在本对象的**私有组**（自己 + 它的依赖）里可见，
//!   全局域看不到它，后装对象自然也看不到。
//!
//! 解析顺序因此是两段式：先查私有组（自己，再按依赖挂接序），再查全局域
//! （按装载序，**先入者胜**）。这带来一条容易被误认为 bug 的规则：两个全局
//! 对象导出同名符号时，后装的那个**不会**覆盖先装的——符号解析用的是装载序
//! 里第一个导出者，而不是最后一个。
//!
//! 作用域是装载时确定的，之后不能就地改写：同一映像已经以 LOCAL 装载，再以
//! GLOBAL 装载一次，得到的是 `ESCOPE` 而不是「提升为全局」。要让后来的对象
//! 看见它，只能卸载后按新作用域重装（本模块的 `promote` 模拟的就是显式提升
//! 这条路，它与 `load` 的作用域闸门是两件事）。
//!
//! 私有组通过 `link_dep` 显式挂接，挂接约束：依赖必须比本体先装载（装载序即
//! 句柄序）、不能自依赖、不能重复挂接同一条依赖。挂接失败一律 `EDEP`，因为
//! 这四种情形都是「依赖关系本身不合法」，与句柄是否有效（`EBADH`）不是一类。
//!
//! 闸门次序：
//! - `load`：零键 → 非法作用域 → 无此映像 → 已装载（同域复用 / 异域 `ESCOPE`）
//!   → 表满；
//! - `link_dep`：本体句柄 → 依赖句柄 → 自依赖 → 依赖不在本体之前 → 重复挂接；
//! - `resolve`：句柄 → 私有组（本体 + 依赖）→ 全局域 → `ESYM`。

use crate::alloc::format;
use crate::checks::CheckSet;

/// 段基址。
pub const SEG_BASE: u32 = 0xecb0;

/// 映像键为零或作用域取值非法。
pub const NS_EINVAL: u32 = SEG_BASE + 0x01;
/// 目录中没有这条映像。
pub const NS_ENOENT: u32 = SEG_BASE + 0x02;
/// 同一映像重复装载。
pub const NS_EDUP: u32 = SEG_BASE + 0x03;
/// 对象表已满。
pub const NS_EFULL: u32 = SEG_BASE + 0x04;
/// 句柄无效。
pub const NS_EBADH: u32 = SEG_BASE + 0x05;
/// 私有组与全局域都没有导出该符号。
pub const NS_ESYM: u32 = SEG_BASE + 0x06;
/// 同映像的装载作用域与既有对象冲突。
pub const NS_ESCOPE: u32 = SEG_BASE + 0x07;
/// 依赖关系非法（自依赖 / 逆序 / 重复 / 成环）。
pub const NS_EDEP: u32 = SEG_BASE + 0x08;
/// 账本形状不符。
pub const NS_EBAL: u32 = SEG_BASE + 0x09;
/// 槽位乱序。
pub const NS_EORDER: u32 = SEG_BASE + 0x0a;
/// 观测值与复算不符。
pub const NS_EMISMATCH: u32 = SEG_BASE + 0x0b;
/// 冻结后重放不一致。
pub const NS_ESEAL: u32 = SEG_BASE + 0x0c;
/// 观察下标越界。
pub const NS_ENOBS: u32 = SEG_BASE + 0x0d;

/// 目录容量。
pub const ICAP: usize = 8;
/// 对象容量。
pub const CAP: usize = 6;
/// 单对象依赖上限。
pub const DEPS: usize = 3;
/// 账本格数。
pub const SAMPLES: usize = 14;

/// 作用域：全局。
pub const S_GLOBAL: u8 = 1;
/// 作用域：私有。
pub const S_LOCAL: u8 = 2;

/// 操作码：装载。
pub const OP_LOAD: u8 = 1;
/// 操作码：解析符号。
pub const OP_RESOLVE: u8 = 2;
/// 操作码：读作用域。
pub const OP_SCOPE: u8 = 3;
/// 操作码：读全局域条数。
pub const OP_GSCOPE: u8 = 4;
/// 操作码：挂接依赖。
pub const OP_LINK: u8 = 5;

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

/// 一条目录映像。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Img {
    /// 映像键。
    pub key: u32,
    /// 导出符号位图。
    pub exports: u32,
}

impl Img {
    /// 造一条映像。
    pub const fn make(key: u32, exports: u32) -> Img {
        Img { key, exports }
    }

    /// 是否导出 `sym`。
    pub fn exports(&self, sym: u32) -> bool {
        sym < 32 && (self.exports & (1u32 << sym)) != 0
    }
}

/// 一个已装载对象。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Obj {
    /// 映像键。
    pub key: u32,
    /// 句柄。
    pub handle: u32,
    /// 作用域。
    pub scope: u8,
    /// 引用计数。
    pub refs: u32,
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
        Obj { key: 0, handle: 0, scope: 0, refs: 0, deps: [0; DEPS], ndeps: 0, live: false }
    }

    /// 是否全局。
    pub fn is_global(&self) -> bool {
        self.scope == S_GLOBAL
    }

    /// 是否已挂接某条依赖。
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

/// 命名空间。
#[derive(Clone, Copy, Debug)]
pub struct Ns {
    cat: [Img; ICAP],
    ncat: usize,
    obj: [Obj; CAP],
    gscope: [u32; CAP],
    nglobal: usize,
    next_handle: u32,
    resolves: u32,
    private_hits: u32,
    global_hits: u32,
}

impl Ns {
    /// 空命名空间。
    pub const fn new() -> Ns {
        Ns {
            cat: [Img::make(0, 0); ICAP],
            ncat: 0,
            obj: [Obj::blank(); CAP],
            gscope: [0; CAP],
            nglobal: 0,
            next_handle: 1,
            resolves: 0,
            private_hits: 0,
            global_hits: 0,
        }
    }

    /// 内置目录：A/B 导出 s0,s1；C 导出 s2；D 只导出 s3。
    pub fn seeded() -> Ns {
        let mut n = Ns::new();
        let _ = n.add_img(Img::make(0x11, 0b0011));
        let _ = n.add_img(Img::make(0x22, 0b0110));
        let _ = n.add_img(Img::make(0x33, 0b0100));
        let _ = n.add_img(Img::make(0x44, 0b1000));
        n
    }

    /// 登记目录。
    pub fn add_img(&mut self, img: Img) -> Result<usize, u32> {
        if img.key == 0 {
            return Err(NS_EINVAL);
        }
        if self.find_img(img.key).is_some() {
            return Err(NS_EDUP);
        }
        if self.ncat >= ICAP {
            return Err(NS_EFULL);
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

    /// 全局域条数。
    pub fn global_len(&self) -> usize {
        self.nglobal
    }

    /// 私有组命中次数。
    pub fn private_hit_count(&self) -> u32 {
        self.private_hits
    }

    /// 全局域命中次数。
    pub fn global_hit_count(&self) -> u32 {
        self.global_hits
    }

    /// 解析总次数。
    pub fn resolve_count(&self) -> u32 {
        self.resolves
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

    /// 装载一条映像，返回句柄。
    pub fn load(&mut self, key: u32, scope: u8) -> Result<usize, u32> {
        if key == 0 || (scope != S_GLOBAL && scope != S_LOCAL) {
            return Err(NS_EINVAL);
        }
        if self.find_img(key).is_none() {
            return Err(NS_ENOENT);
        }
        if let Some(i) = self.slot_by_key(key) {
            let same = match self.obj.get(i) {
                Some(o) => o.scope == scope,
                None => false,
            };
            if !same {
                return Err(NS_ESCOPE);
            }
            if let Some(o) = self.obj.get_mut(i) {
                o.refs = o.refs.saturating_add(1);
                return Ok(o.handle as usize);
            }
            return Err(NS_EBADH);
        }
        let slot = match self.free_slot() {
            Some(i) => i,
            None => return Err(NS_EFULL),
        };
        let handle = self.next_handle;
        if let Some(o) = self.obj.get_mut(slot) {
            *o = Obj {
                key,
                handle,
                scope,
                refs: 1,
                deps: [0; DEPS],
                ndeps: 0,
                live: true,
            };
        }
        self.next_handle = self.next_handle.saturating_add(1);
        if scope == S_GLOBAL && self.nglobal < CAP {
            if let Some(dst) = self.gscope.get_mut(self.nglobal) {
                *dst = handle;
            }
            self.nglobal += 1;
        }
        Ok(handle as usize)
    }

    /// 挂接依赖（依赖必须比本体先装载）。
    pub fn link_dep(&mut self, handle: u32, dep: u32) -> Result<usize, u32> {
        let hi = match self.slot_of(handle) {
            Some(i) => i,
            None => return Err(NS_EBADH),
        };
        let di = match self.slot_of(dep) {
            Some(i) => i,
            None => return Err(NS_EBADH),
        };
        if handle == dep {
            return Err(NS_EDEP);
        }
        if dep >= handle {
            return Err(NS_EDEP);
        }
        let already = match self.obj.get(hi) {
            Some(o) => o.has_dep(dep),
            None => return Err(NS_EBADH),
        };
        if already {
            return Err(NS_EDEP);
        }
        let n = match self.obj.get(hi) {
            Some(o) => o.ndeps as usize,
            None => return Err(NS_EBADH),
        };
        if n >= DEPS {
            return Err(NS_EDEP);
        }
        if let Some(o) = self.obj.get_mut(hi) {
            if let Some(dst) = o.deps.get_mut(n) {
                *dst = dep;
            }
            o.ndeps = o.ndeps.saturating_add(1);
        }
        let _ = di;
        match self.obj.get(hi) {
            Some(o) => Ok(o.ndeps as usize),
            None => Err(NS_EBADH),
        }
    }

    /// 读对象。
    pub fn object_of(&self, handle: u32) -> Result<Obj, u32> {
        if handle == 0 {
            return Err(NS_EBADH);
        }
        match self.slot_of(handle) {
            Some(i) => match self.obj.get(i) {
                Some(o) => Ok(*o),
                None => Err(NS_EBADH),
            },
            None => Err(NS_EBADH),
        }
    }

    /// 对象作用域。
    pub fn scope_of(&self, handle: u32) -> Result<u32, u32> {
        let o = self.object_of(handle)?;
        Ok(o.scope as u32)
    }

    /// 对象引用计数。
    pub fn refs_of(&self, handle: u32) -> Result<u32, u32> {
        let o = self.object_of(handle)?;
        Ok(o.refs)
    }

    /// 解析符号：先私有组，后全局域，返回导出者句柄。
    pub fn resolve(&mut self, from: u32, sym: u32) -> Result<u32, u32> {
        if sym >= 32 {
            return Err(NS_ESYM);
        }
        let o = self.object_of(from)?;
        self.resolves = self.resolves.saturating_add(1);
        if let Some(img) = self.find_img(o.key) {
            if img.exports(sym) {
                self.private_hits = self.private_hits.saturating_add(1);
                return Ok(from);
            }
        }
        let mut i = 0usize;
        while i < o.ndeps as usize {
            let d = match o.deps.get(i) {
                Some(v) => *v,
                None => break,
            };
            let dok = match self.object_of(d) {
                Ok(v) => v,
                Err(_) => {
                    i += 1;
                    continue;
                }
            };
            if let Some(img) = self.find_img(dok.key) {
                if img.exports(sym) {
                    self.private_hits = self.private_hits.saturating_add(1);
                    return Ok(d);
                }
            }
            i += 1;
        }
        let mut g = 0usize;
        while g < self.nglobal {
            let h = match self.gscope.get(g) {
                Some(v) => *v,
                None => break,
            };
            let gok = match self.object_of(h) {
                Ok(v) => v,
                Err(_) => {
                    g += 1;
                    continue;
                }
            };
            if let Some(img) = self.find_img(gok.key) {
                if img.exports(sym) {
                    self.global_hits = self.global_hits.saturating_add(1);
                    return Ok(h);
                }
            }
            g += 1;
        }
        Err(NS_ESYM)
    }

    /// 从某对象视角，符号是否可见。
    pub fn visible_from(&mut self, from: u32, sym: u32) -> bool {
        self.resolve(from, sym).is_ok()
    }

    /// 显式把 LOCAL 对象提升为 GLOBAL（模拟 RTLD_NOLOAD|RTLD_GLOBAL）。
    pub fn promote(&mut self, handle: u32) -> Result<usize, u32> {
        let i = match self.slot_of(handle) {
            Some(v) => v,
            None => return Err(NS_EBADH),
        };
        let global = match self.obj.get(i) {
            Some(o) => o.is_global(),
            None => return Err(NS_EBADH),
        };
        if global {
            return Err(NS_ESCOPE);
        }
        if let Some(o) = self.obj.get_mut(i) {
            o.scope = S_GLOBAL;
        }
        if self.nglobal < CAP {
            if let Some(dst) = self.gscope.get_mut(self.nglobal) {
                *dst = handle;
            }
            self.nglobal += 1;
        }
        Ok(self.global_len())
    }

    /// 全局域第 i 个句柄。
    pub fn global_at(&self, i: usize) -> Option<u32> {
        if i >= self.nglobal {
            return None;
        }
        self.gscope.get(i).copied()
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
            return Err(NS_EORDER);
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
            return Err(NS_EBAL);
        }
        const VALS: [u32; 8] = [1, 2, 3, 2, 1, 1, 2, 1];
        const CODES: [u32; 6] =
            [NS_EINVAL, NS_ENOENT, NS_ESCOPE, NS_EBADH, NS_ESYM, NS_EFULL];
        let mut vi = 0usize;
        let mut ci = 0usize;
        let mut i = 0usize;
        while i < self.len {
            let s = match self.rows.get(i) {
                Some(v) => *v,
                None => return Err(NS_EBAL),
            };
            if s.slot as usize != i {
                return Err(NS_EORDER);
            }
            if s.ok {
                if s.code != 0 {
                    return Err(NS_EMISMATCH);
                }
                match VALS.get(vi) {
                    Some(w) => {
                        if s.value != *w {
                            return Err(NS_EMISMATCH);
                        }
                    }
                    None => return Err(NS_EBAL),
                }
                vi += 1;
            } else {
                match CODES.get(ci) {
                    Some(w) => {
                        if s.code != *w {
                            return Err(NS_EMISMATCH);
                        }
                    }
                    None => return Err(NS_EBAL),
                }
                ci += 1;
            }
            i += 1;
        }
        if vi != 8 || ci != 6 {
            return Err(NS_EBAL);
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
            return Err(NS_EBAL);
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
            Err(NS_ESEAL)
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

/// 十四格账本：全局/私有装载 → 两段解析 → 全局域计数 → 六类拒绝。
pub fn build_ledger() -> Ledger {
    let mut l = Ledger::new();
    let mut n = Ns::seeded();

    let _ = l.register(cell(0, OP_LOAD, n.load(0x11, S_GLOBAL)));
    let _ = l.register(cell(1, OP_LOAD, n.load(0x22, S_LOCAL)));
    let _ = l.register(cell(2, OP_LOAD, n.load(0x33, S_GLOBAL)));
    let _ = l.register(word_cell(3, OP_RESOLVE, n.resolve(2, 1)));
    let _ = l.register(word_cell(4, OP_RESOLVE, n.resolve(3, 1)));
    let _ = l.register(word_cell(5, OP_SCOPE, n.scope_of(1)));
    let _ = l.register(cell(6, OP_GSCOPE, Ok(n.global_len())));
    let _ = l.register(word_cell(7, OP_RESOLVE, n.resolve(1, 0)));

    let _ = l.register(cell(8, OP_LOAD, n.load(0, S_GLOBAL)));

    let mut m = Ns::seeded();
    let _ = l.register(cell(9, OP_LOAD, m.load(0x99, S_LOCAL)));

    let _ = l.register(cell(10, OP_LOAD, n.load(0x11, S_LOCAL)));

    let _ = l.register(word_cell(11, OP_RESOLVE, n.resolve(0, 1)));

    let _ = l.register(word_cell(12, OP_RESOLVE, n.resolve(3, 30)));

    let mut f = Ns::new();
    let mut i = 0u32;
    while (i as usize) < ICAP {
        let _ = f.add_img(Img::make(0xA0 + i, 1));
        i += 1;
    }
    let mut k = 0u32;
    while (k as usize) < CAP {
        let _ = f.load(0xA0 + k, S_LOCAL);
        k += 1;
    }
    let _ = l.register(cell(13, OP_LOAD, f.load(0xA0 + (CAP as u32), S_LOCAL)));

    l
}

/// 四路注入。
pub fn inject_four() -> [bool; 4] {
    let mut out = [false; 4];

    let l = build_ledger();
    let rep = l.replay();
    let root = l.seal();
    out[0] = l.len() == SAMPLES && l.audit().is_ok() && rep == Ok(root) && root != 0;

    // 一路：GLOBAL 入全局域、LOCAL 只在私有组；后装者看得见前者，看不见 LOCAL。
    let mut a = Ns::seeded();
    let ga = a.load(0x11, S_GLOBAL);
    let lb = a.load(0x22, S_LOCAL);
    let mut f = Ns::seeded();
    let _ = f.load(0x11, S_GLOBAL);
    let _ = f.load(0x22, S_LOCAL);
    let late = f.load(0x44, S_LOCAL);
    out[1] = ga == Ok(1)
        && lb == Ok(2)
        && a.global_len() == 1
        && a.scope_of(1) == Ok(S_GLOBAL as u32)
        && a.scope_of(2) == Ok(S_LOCAL as u32)
        && a.resolve(2, 1) == Ok(2)
        && a.resolve(1, 1) == Ok(1)
        && late == Ok(3)
        && f.resolve(3, 1) == Ok(1)
        && f.visible_from(3, 0)
        && f.visible_from(2, 2)
        && !f.visible_from(3, 2);

    // 二路：全局域先入者胜，后装同名导出者不覆盖。
    let mut b = Ns::seeded();
    let _ = b.load(0x11, S_GLOBAL);
    let _ = b.load(0x22, S_GLOBAL);
    let first = b.resolve(1, 0);
    let from_second = b.resolve(2, 0);
    let mut c = Ns::seeded();
    let _ = c.load(0x22, S_GLOBAL);
    let only_second = c.resolve(1, 1);
    out[2] = first == Ok(1)
        && from_second == Ok(1)
        && only_second == Ok(1)
        && b.global_len() == 2
        && b.global_at(0) == Some(1)
        && b.global_at(1) == Some(2)
        && b.resolve(1, 5).is_err();

    // 三路：作用域冲突/依赖四类非法/提升语义。
    let mut c = Ns::seeded();
    let _ = c.load(0x11, S_LOCAL);
    let conflict = c.load(0x11, S_GLOBAL);
    let self_dep = c.link_dep(1, 1);
    let mut d = Ns::seeded();
    let _ = d.load(0x22, S_LOCAL);
    let _ = d.load(0x11, S_GLOBAL);
    let reverse = d.link_dep(1, 2);
    let _ = d.link_dep(2, 1);
    let dup_dep = d.link_dep(2, 1);
    let promote_local = d.promote(1);
    let promote_global = d.promote(2);
    let promote_again = d.promote(1);
    out[3] = conflict == Err(NS_ESCOPE)
        && self_dep == Err(NS_EDEP)
        && reverse == Err(NS_EDEP)
        && dup_dep == Err(NS_EDEP)
        && promote_local == Ok(2)
        && promote_global == Err(NS_ESCOPE)
        && promote_again == Err(NS_ESCOPE)
        && d.global_len() == 2;

    out
}

/// 十三项判据。
pub fn run_unx11363_checks() -> CheckSet {
    let mut s = CheckSet::new("unx11363.dl_scope");
    let l = build_ledger();

    s.add_dyn(
        "NS1-十四格八允六拒",
        l.len() == SAMPLES && l.ok_count() == 8 && l.err_count() == 6 && l.audit().is_ok(),
        &format!("n={} ok={} err={}", l.len(), l.ok_count(), l.err_count()),
    );

    s.add(
        "NS2-句柄按装载序递增",
        l.sample(0).map(|x| x.value) == Some(1)
            && l.sample(1).map(|x| x.value) == Some(2)
            && l.sample(2).map(|x| x.value) == Some(3),
        "static",
    );

    s.add(
        "NS3-私有组优先于全局域",
        l.sample(3).map(|x| x.value) == Some(2),
        "static",
    );

    s.add(
        "NS4-全局域先入者胜",
        l.sample(4).map(|x| x.value) == Some(1),
        "static",
    );

    s.add(
        "NS5-作用域可回读",
        l.sample(5).map(|x| x.value) == Some(S_GLOBAL as u32),
        "static",
    );

    s.add(
        "NS6-全局域只收全局对象",
        l.sample(6).map(|x| x.value) == Some(2),
        "static",
    );

    s.add(
        "NS7-本体自身导出可解析",
        l.sample(7).map(|x| x.value) == Some(1),
        "static",
    );

    s.add(
        "NS8-零键装载被拒",
        l.sample(8).map(|x| x.code) == Some(NS_EINVAL),
        "static",
    );

    s.add(
        "NS9-无此映像被拒",
        l.sample(9).map(|x| x.code) == Some(NS_ENOENT),
        "static",
    );

    s.add(
        "NS10-作用域冲突被拒",
        l.sample(10).map(|x| x.code) == Some(NS_ESCOPE),
        "static",
    );

    s.add(
        "NS11-野句柄与符号缺失",
        l.sample(11).map(|x| x.code) == Some(NS_EBADH)
            && l.sample(12).map(|x| x.code) == Some(NS_ESYM),
        "static",
    );

    s.add(
        "NS12-表满与依赖四非法",
        {
            let full = l.sample(13).map(|x| x.code) == Some(NS_EFULL);
            let mut d = Ns::seeded();
            let _ = d.load(0x22, S_LOCAL);
            let _ = d.load(0x11, S_GLOBAL);
            let a = d.link_dep(1, 1) == Err(NS_EDEP);
            let b = d.link_dep(1, 2) == Err(NS_EDEP);
            let c = d.link_dep(2, 1) == Ok(1);
            let e = d.link_dep(2, 1) == Err(NS_EDEP);
            let f = d.link_dep(2, 9) == Err(NS_EBADH);
            full && a && b && c && e && f
        },
        "static",
    );

    s.add(
        "NS13-冻结重放四注入",
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
        let s = run_unx11363_checks();
        assert_eq!(s.len(), 13);
        assert!(s.all_passed());
    }

    #[test]
    fn local_is_invisible_to_late_objects() {
        let mut n = Ns::seeded();
        let _ = n.load(0x11, S_GLOBAL);
        let _ = n.load(0x22, S_LOCAL);
        let _ = n.load(0x44, S_LOCAL);
        assert_eq!(n.resolve(3, 0), Ok(1));
        assert!(!n.visible_from(3, 2));
        assert!(n.visible_from(2, 2));
        assert_eq!(n.resolve(2, 1), Ok(2));
        assert_eq!(n.global_len(), 1);
    }

    #[test]
    fn first_global_wins() {
        let mut n = Ns::seeded();
        let _ = n.load(0x22, S_GLOBAL);
        let _ = n.load(0x11, S_GLOBAL);
        assert_eq!(n.resolve(1, 0), Ok(2));
        assert_eq!(n.resolve(2, 0), Ok(2));
        assert_eq!(n.resolve(1, 1), Ok(1));
        assert_eq!(n.global_at(0), Some(1));
    }

    #[test]
    fn scope_conflict_on_reload() {
        let mut n = Ns::seeded();
        assert_eq!(n.load(0x11, S_LOCAL), Ok(1));
        assert_eq!(n.load(0x11, S_GLOBAL), Err(NS_ESCOPE));
        assert_eq!(n.load(0x11, S_LOCAL), Ok(1));
        assert_eq!(n.refs_of(1), Ok(2));
        assert_eq!(n.load(0x11, 9), Err(NS_EINVAL));
    }

    #[test]
    fn dependency_gates() {
        let mut n = Ns::seeded();
        let _ = n.load(0x22, S_LOCAL);
        let _ = n.load(0x11, S_GLOBAL);
        assert_eq!(n.link_dep(1, 1), Err(NS_EDEP));
        assert_eq!(n.link_dep(1, 2), Err(NS_EDEP));
        assert_eq!(n.link_dep(2, 1), Ok(1));
        assert_eq!(n.link_dep(2, 1), Err(NS_EDEP));
        assert_eq!(n.link_dep(2, 7), Err(NS_EBADH));
    }

    #[test]
    fn promote_moves_scope() {
        let mut n = Ns::seeded();
        let _ = n.load(0x11, S_LOCAL);
        let _ = n.load(0x44, S_LOCAL);
        assert_eq!(n.scope_of(1), Ok(S_LOCAL as u32));
        assert_eq!(n.promote(1), Ok(1));
        assert_eq!(n.scope_of(1), Ok(S_GLOBAL as u32));
        assert_eq!(n.promote(1), Err(NS_ESCOPE));
        assert_eq!(n.resolve(2, 0), Ok(1));
    }
}
