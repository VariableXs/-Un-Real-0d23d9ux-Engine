//! UNX-F11365 dlsym 的 RTLD_DEFAULT 与 RTLD_NEXT 特型句柄（0xecd0 段）
//!
//! `RTLD_DEFAULT` 与 `RTLD_NEXT` 不是真正的句柄，而是两个**伪句柄**，它们改变
//! 的是查找的**起点**：
//!
//! - `RTLD_DEFAULT` 从链接序（link map）头部开始，等价于「在全局可见域里找」。
//!   它看到的是装载顺序里第一个导出该符号的对象。
//! - `RTLD_NEXT` 从**调用者所在对象的下一个**开始，跳过调用者和它之前的一切。
//!   这是包装函数（interposer）的立身之本：包装层要找到「被自己包住的那个
//!   实现」，而不是又找到自己。
//!
//! 因此同一个符号在同一个进程里，`DEFAULT` 与 `NEXT` 可以给出完全不同的答案。
//! 本模块把链接序做成显式的下标序（装载即追加），并保证三件事：
//!
//! 1. **链接序不因卸载而重排**：`dlclose` 只把槽位标死，后面的对象序号不变。
//!    重排会让 `NEXT` 的语义随无关对象的卸载而漂移，这是不可接受的。
//! 2. **「没有后继」与「后继都不导出」分开**：前者是 `ENEXT`（调用者已是链尾），
//!    后者是 `ENOMATCH`（后继存在但符号不在其中）。glibc 两者都返回 NULL，
//!    但 `dlerror` 文案不同，诊断时必须能区分。
//! 3. **已卸载句柄与野句柄分开**：前者 `ESCOPED`（这个号曾经合法），后者
//!    `EBADH`（从来不存在）。混码会把 use-after-dlclose 误诊成野指针。
//!
//! 闸门次序：
//! - `load`：零键 → 无此映像 → 已装载 → 表满；
//! - `dlsym_default`：符号号 ≥32 → 顺序扫描（跳过死槽）→ `ENOSYM`；
//! - `dlsym_next`：符号号/零调用者 → 调用者句柄 → 调用者已卸载 → 无活后继
//!   （`ENEXT`）→ 逐个后继（`ENOMATCH`）。

use crate::alloc::format;
use crate::checks::CheckSet;

/// 段基址。
pub const SEG_BASE: u32 = 0xecd0;

/// 符号号非法或调用者为零。
pub const DS_EINVAL: u32 = SEG_BASE + 0x01;
/// 全局域没有对象导出该符号。
pub const DS_ENOSYM: u32 = SEG_BASE + 0x02;
/// 调用者已是链尾，其后没有任何活对象。
pub const DS_ENEXT: u32 = SEG_BASE + 0x03;
/// 句柄从未分配。
pub const DS_EBADH: u32 = SEG_BASE + 0x04;
/// 同一映像重复装载。
pub const DS_EDUP: u32 = SEG_BASE + 0x05;
/// 链接表已满。
pub const DS_EFULL: u32 = SEG_BASE + 0x06;
/// 句柄曾合法，但对象已卸载。
pub const DS_ESCOPED: u32 = SEG_BASE + 0x07;
/// 后继存在，但没有任何一个导出该符号。
pub const DS_ENOMATCH: u32 = SEG_BASE + 0x08;
/// 账本形状不符。
pub const DS_EBAL: u32 = SEG_BASE + 0x09;
/// 槽位乱序。
pub const DS_EORDER: u32 = SEG_BASE + 0x0a;
/// 观测值与复算不符。
pub const DS_EMISMATCH: u32 = SEG_BASE + 0x0b;
/// 冻结后重放不一致。
pub const DS_ESEAL: u32 = SEG_BASE + 0x0c;
/// 观察下标越界。
pub const DS_ENOBS: u32 = SEG_BASE + 0x0d;

/// 目录容量。
pub const ICAP: usize = 8;
/// 链接表容量。
pub const CAP: usize = 7;
/// 账本格数。
pub const SAMPLES: usize = 14;

/// 操作码：装载。
pub const OP_LOAD: u8 = 1;
/// 操作码：DEFAULT 查找。
pub const OP_DEFAULT: u8 = 2;
/// 操作码：NEXT 查找。
pub const OP_NEXT: u8 = 3;
/// 操作码：读链接序。
pub const OP_ORDER: u8 = 4;
/// 操作码：卸载。
pub const OP_UNLOAD: u8 = 5;

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

    /// 是否导出 `sym`。
    pub fn exports(&self, sym: u32) -> bool {
        sym < 32 && (self.exports & (1u32 << sym)) != 0
    }
}

/// 链接序里的一个对象。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Obj {
    /// 映像键。
    pub key: u32,
    /// 句柄（= 链接序 + 1）。
    pub handle: u32,
    /// 是否存活。
    pub live: bool,
}

impl Obj {
    /// 空槽。
    pub const fn blank() -> Obj {
        Obj { key: 0, handle: 0, live: false }
    }
}

/// 链接表。
#[derive(Clone, Copy, Debug)]
pub struct LinkMap {
    cat: [Img; ICAP],
    ncat: usize,
    obj: [Obj; CAP],
    n: usize,
    defaults: u32,
    nexts: u32,
    scans: u32,
}

impl LinkMap {
    /// 空表。
    pub const fn new() -> LinkMap {
        LinkMap {
            cat: [Img::make(0, 0); ICAP],
            ncat: 0,
            obj: [Obj::blank(); CAP],
            n: 0,
            defaults: 0,
            nexts: 0,
            scans: 0,
        }
    }

    /// 内置目录：A 导出 s0；B 导出 s1；C 导出 s0,s1；D 只导出 s3。
    pub fn seeded() -> LinkMap {
        let mut m = LinkMap::new();
        let _ = m.add_img(Img::make(0x10, 0b0001));
        let _ = m.add_img(Img::make(0x11, 0b0010));
        let _ = m.add_img(Img::make(0x12, 0b0011));
        let _ = m.add_img(Img::make(0x13, 0b1000));
        m
    }

    /// 登记目录。
    pub fn add_img(&mut self, img: Img) -> Result<usize, u32> {
        if img.key == 0 {
            return Err(DS_EINVAL);
        }
        if self.find_img(img.key).is_some() {
            return Err(DS_EDUP);
        }
        if self.ncat >= ICAP {
            return Err(DS_EFULL);
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

    /// 链接序长度（含死槽）。
    pub fn len(&self) -> usize {
        self.n
    }

    /// 是否空表。
    pub fn is_empty(&self) -> bool {
        self.n == 0
    }

    /// 存活对象数。
    pub fn live_count(&self) -> usize {
        let mut k = 0usize;
        let mut i = 0usize;
        while i < self.n {
            if let Some(o) = self.obj.get(i) {
                if o.live {
                    k += 1;
                }
            }
            i += 1;
        }
        k
    }

    /// DEFAULT 命中次数。
    pub fn default_hits(&self) -> u32 {
        self.defaults
    }

    /// NEXT 命中次数。
    pub fn next_hits(&self) -> u32 {
        self.nexts
    }

    /// 扫描过的槽数。
    pub fn scan_count(&self) -> u32 {
        self.scans
    }

    fn idx_of(&self, handle: u32) -> Option<usize> {
        let i = handle.checked_sub(1)? as usize;
        if i >= self.n {
            return None;
        }
        match self.obj.get(i) {
            Some(o) => {
                if o.live {
                    Some(i)
                } else {
                    None
                }
            }
            None => None,
        }
    }

    fn idx_by_key(&self, key: u32) -> Option<usize> {
        let mut i = 0usize;
        while i < self.n {
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

    /// 装载一条映像，追加到链接序尾部。
    pub fn load(&mut self, key: u32) -> Result<usize, u32> {
        if key == 0 {
            return Err(DS_EINVAL);
        }
        if self.find_img(key).is_none() {
            return Err(DS_ENOSYM);
        }
        if self.idx_by_key(key).is_some() {
            return Err(DS_EDUP);
        }
        if self.n >= CAP {
            return Err(DS_EFULL);
        }
        let handle = (self.n as u32).saturating_add(1);
        if let Some(dst) = self.obj.get_mut(self.n) {
            *dst = Obj { key, handle, live: true };
        }
        self.n += 1;
        Ok(handle as usize)
    }

    /// 句柄的链接序下标。
    pub fn order_of(&self, handle: u32) -> Result<u32, u32> {
        if handle == 0 {
            return Err(DS_EINVAL);
        }
        match self.idx_of(handle) {
            Some(i) => Ok(i as u32),
            None => {
                if (handle as usize) <= self.n {
                    Err(DS_ESCOPED)
                } else {
                    Err(DS_EBADH)
                }
            }
        }
    }

    /// 句柄的映像键。
    pub fn key_of(&self, handle: u32) -> Result<u32, u32> {
        let i = self.order_of(handle)?;
        match self.obj.get(i as usize) {
            Some(o) => Ok(o.key),
            None => Err(DS_EBADH),
        }
    }

    /// `RTLD_DEFAULT`：从链接序头部扫描。
    pub fn dlsym_default(&mut self, sym: u32) -> Result<u32, u32> {
        if sym >= 32 {
            return Err(DS_EINVAL);
        }
        let mut i = 0usize;
        while i < self.n {
            self.scans = self.scans.saturating_add(1);
            let o = match self.obj.get(i) {
                Some(v) => *v,
                None => break,
            };
            if o.live {
                if let Some(img) = self.find_img(o.key) {
                    if img.exports(sym) {
                        self.defaults = self.defaults.saturating_add(1);
                        return Ok(o.handle);
                    }
                }
            }
            i += 1;
        }
        Err(DS_ENOSYM)
    }

    /// `RTLD_NEXT`：从调用者之后扫描。
    pub fn dlsym_next(&mut self, caller: u32, sym: u32) -> Result<u32, u32> {
        if caller == 0 || sym >= 32 {
            return Err(DS_EINVAL);
        }
        let ci = match self.idx_of(caller) {
            Some(i) => i,
            None => {
                if (caller as usize) <= self.n {
                    return Err(DS_ESCOPED);
                }
                return Err(DS_EBADH);
            }
        };
        let mut i = ci + 1;
        let mut alive = false;
        while i < self.n {
            let o = match self.obj.get(i) {
                Some(v) => *v,
                None => break,
            };
            if o.live {
                alive = true;
                self.scans = self.scans.saturating_add(1);
                if let Some(img) = self.find_img(o.key) {
                    if img.exports(sym) {
                        self.nexts = self.nexts.saturating_add(1);
                        return Ok(o.handle);
                    }
                }
            }
            i += 1;
        }
        if alive {
            Err(DS_ENOMATCH)
        } else {
            Err(DS_ENEXT)
        }
    }

    /// 卸载：只标死，不重排。
    pub fn unload(&mut self, handle: u32) -> Result<usize, u32> {
        if handle == 0 {
            return Err(DS_EINVAL);
        }
        let i = match self.idx_of(handle) {
            Some(v) => v,
            None => {
                if (handle as usize) <= self.n {
                    return Err(DS_ESCOPED);
                }
                return Err(DS_EBADH);
            }
        };
        if let Some(o) = self.obj.get_mut(i) {
            o.live = false;
        }
        Ok(self.live_count())
    }

    /// 从链接序尾部回填（仅测试与检查用来构造边界）。
    pub fn push_raw(&mut self, key: u32, exports_bits: u32) -> Option<u32> {
        if self.n >= CAP || key == 0 {
            return None;
        }
        if self.find_img(key).is_none() {
            let _ = self.add_img(Img::make(key, exports_bits));
        }
        let handle = (self.n as u32).saturating_add(1);
        if let Some(dst) = self.obj.get_mut(self.n) {
            *dst = Obj { key, handle, live: true };
        }
        self.n += 1;
        Some(handle)
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
            return Err(DS_EORDER);
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
            return Err(DS_EBAL);
        }
        const VALS: [u32; 8] = [1, 2, 3, 1, 2, 3, 3, 2];
        const CODES: [u32; 6] =
            [DS_ENOSYM, DS_ENEXT, DS_EINVAL, DS_EBADH, DS_EDUP, DS_EFULL];
        let mut vi = 0usize;
        let mut ci = 0usize;
        let mut i = 0usize;
        while i < self.len {
            let s = match self.rows.get(i) {
                Some(v) => *v,
                None => return Err(DS_EBAL),
            };
            if s.slot as usize != i {
                return Err(DS_EORDER);
            }
            if s.ok {
                if s.code != 0 {
                    return Err(DS_EMISMATCH);
                }
                match VALS.get(vi) {
                    Some(w) => {
                        if s.value != *w {
                            return Err(DS_EMISMATCH);
                        }
                    }
                    None => return Err(DS_EBAL),
                }
                vi += 1;
            } else {
                match CODES.get(ci) {
                    Some(w) => {
                        if s.code != *w {
                            return Err(DS_EMISMATCH);
                        }
                    }
                    None => return Err(DS_EBAL),
                }
                ci += 1;
            }
            i += 1;
        }
        if vi != 8 || ci != 6 {
            return Err(DS_EBAL);
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
            return Err(DS_EBAL);
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
            Err(DS_ESEAL)
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

/// 十四格账本：三装载 → DEFAULT/NEXT 两查 → 链接序 → 六类拒绝。
pub fn build_ledger() -> Ledger {
    let mut l = Ledger::new();
    let mut m = LinkMap::seeded();

    let _ = l.register(cell(0, OP_LOAD, m.load(0x10)));
    let _ = l.register(cell(1, OP_LOAD, m.load(0x11)));
    let _ = l.register(cell(2, OP_LOAD, m.load(0x12)));
    let _ = l.register(word_cell(3, OP_DEFAULT, m.dlsym_default(0)));
    let _ = l.register(word_cell(4, OP_DEFAULT, m.dlsym_default(1)));
    let _ = l.register(word_cell(5, OP_NEXT, m.dlsym_next(1, 0)));
    let _ = l.register(word_cell(6, OP_NEXT, m.dlsym_next(2, 1)));
    let _ = l.register(word_cell(7, OP_ORDER, m.order_of(3)));

    let _ = l.register(word_cell(8, OP_DEFAULT, m.dlsym_default(30)));

    let _ = l.register(word_cell(9, OP_NEXT, m.dlsym_next(3, 0)));

    let _ = l.register(word_cell(10, OP_NEXT, m.dlsym_next(0, 0)));

    let _ = l.register(word_cell(11, OP_NEXT, m.dlsym_next(0xFF, 0)));

    let _ = l.register(cell(12, OP_LOAD, m.load(0x10)));

    let mut f = LinkMap::new();
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

    // 一路：同一符号 DEFAULT 取头部、NEXT 取后继，二者不同。
    let mut a = LinkMap::seeded();
    let _ = a.load(0x10);
    let _ = a.load(0x12);
    let _ = a.load(0x11);
    let d0 = a.dlsym_default(0);
    let n0 = a.dlsym_next(1, 0);
    let n1 = a.dlsym_next(2, 1);
    out[1] = d0 == Ok(1)
        && n0 == Ok(2)
        && n1 == Ok(3)
        && a.default_hits() == 1
        && a.next_hits() == 2
        && a.live_count() == 3
        && a.order_of(1) == Ok(0)
        && a.order_of(3) == Ok(2)
        && a.key_of(2) == Ok(0x12);

    // 二路：无后继与后继不匹配分开；卸载后句柄仍占序。
    let mut b = LinkMap::seeded();
    let _ = b.load(0x10);
    let _ = b.load(0x13);
    let no_match = b.dlsym_next(1, 0);
    let tail = b.dlsym_next(2, 0);
    let un = b.unload(2);
    let gone = b.dlsym_next(2, 0);
    let order_kept = b.len();
    let mut c = LinkMap::seeded();
    let _ = c.load(0x10);
    let _ = c.load(0x11);
    let _ = c.load(0x12);
    let _ = c.unload(1);
    let skip_dead = c.dlsym_default(0);
    let next_skips = c.dlsym_next(2, 1);
    out[2] = no_match == Err(DS_ENOMATCH)
        && tail == Err(DS_ENEXT)
        && un == Ok(1)
        && gone == Err(DS_ESCOPED)
        && order_kept == 2
        && skip_dead == Ok(3)
        && next_skips == Ok(3)
        && c.order_of(3) == Ok(2);

    // 三路：闸门（野号/零号/大符号号/重装/满表）。
    let mut d = LinkMap::seeded();
    let _ = d.load(0x10);
    let g1 = d.dlsym_default(40);
    let g2 = d.dlsym_next(0, 0);
    let g3 = d.dlsym_next(9, 0);
    let g4 = d.load(0x10);
    let g5 = d.unload(0);
    let g6 = d.order_of(0);
    let mut e = LinkMap::new();
    let mut i = 0u32;
    while (i as usize) < ICAP {
        let _ = e.add_img(Img::make(0xB0 + i, 1));
        i += 1;
    }
    let mut k = 0u32;
    while (k as usize) < CAP {
        let _ = e.load(0xB0 + k);
        k += 1;
    }
    let g7 = e.load(0xB0 + (CAP as u32));
    out[3] = g1 == Err(DS_EINVAL)
        && g2 == Err(DS_EINVAL)
        && g3 == Err(DS_EBADH)
        && g4 == Err(DS_EDUP)
        && g5 == Err(DS_EINVAL)
        && g6 == Err(DS_EINVAL)
        && g7 == Err(DS_EFULL)
        && e.live_count() == CAP;

    out
}

/// 十三项判据。
pub fn run_unx11365_checks() -> CheckSet {
    let mut s = CheckSet::new("unx11365.dlsym_special");
    let l = build_ledger();

    s.add_dyn(
        "DS1-十四格八允六拒",
        l.len() == SAMPLES && l.ok_count() == 8 && l.err_count() == 6 && l.audit().is_ok(),
        &format!("n={} ok={} err={}", l.len(), l.ok_count(), l.err_count()),
    );

    s.add(
        "DS2-装载即追加链接序",
        l.sample(0).map(|x| x.value) == Some(1)
            && l.sample(1).map(|x| x.value) == Some(2)
            && l.sample(2).map(|x| x.value) == Some(3),
        "static",
    );

    s.add(
        "DS3-DEFAULT从头部取首个导出者",
        l.sample(3).map(|x| x.value) == Some(1)
            && l.sample(4).map(|x| x.value) == Some(2),
        "static",
    );

    s.add(
        "DS4-NEXT跳过调用者",
        l.sample(5).map(|x| x.value) == Some(3)
            && l.sample(6).map(|x| x.value) == Some(3),
        "static",
    );

    s.add(
        "DS5-链接序可回读",
        l.sample(7).map(|x| x.value) == Some(2),
        "static",
    );

    s.add(
        "DS6-全域无符号被拒",
        l.sample(8).map(|x| x.code) == Some(DS_ENOSYM),
        "static",
    );

    s.add(
        "DS7-链尾取后继被拒",
        l.sample(9).map(|x| x.code) == Some(DS_ENEXT),
        "static",
    );

    s.add(
        "DS8-零调用者被拒",
        l.sample(10).map(|x| x.code) == Some(DS_EINVAL),
        "static",
    );

    s.add(
        "DS9-野句柄取后继被拒",
        l.sample(11).map(|x| x.code) == Some(DS_EBADH),
        "static",
    );

    s.add(
        "DS10-重复装载被拒",
        l.sample(12).map(|x| x.code) == Some(DS_EDUP),
        "static",
    );

    s.add(
        "DS11-链接表满被拒",
        l.sample(13).map(|x| x.code) == Some(DS_EFULL),
        "static",
    );

    s.add(
        "DS12-无后继与不匹配可辨",
        {
            let mut b = LinkMap::seeded();
            let _ = b.load(0x10);
            let _ = b.load(0x13);
            let nm = b.dlsym_next(1, 0);
            let tail = b.dlsym_next(2, 0);
            let mut c = LinkMap::seeded();
            let _ = c.load(0x10);
            let _ = c.load(0x11);
            let _ = c.load(0x12);
            let _ = c.unload(1);
            let dead = c.dlsym_next(1, 0);
            let kept = c.order_of(3);
            nm == Err(DS_ENOMATCH)
                && tail == Err(DS_ENEXT)
                && nm != tail
                && dead == Err(DS_ESCOPED)
                && kept == Ok(2)
                && c.len() == 3
        },
        "static",
    );

    s.add(
        "DS13-冻结重放四注入",
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
        let s = run_unx11365_checks();
        assert_eq!(s.len(), 13);
        assert!(s.all_passed());
    }

    #[test]
    fn default_and_next_differ() {
        let mut m = LinkMap::seeded();
        let _ = m.load(0x10);
        let _ = m.load(0x12);
        assert_eq!(m.dlsym_default(0), Ok(1));
        assert_eq!(m.dlsym_next(1, 0), Ok(2));
        assert_eq!(m.dlsym_next(2, 0), Err(DS_ENEXT));
    }

    #[test]
    fn no_match_is_not_tail() {
        let mut m = LinkMap::seeded();
        let _ = m.load(0x10);
        let _ = m.load(0x13);
        assert_eq!(m.dlsym_next(1, 0), Err(DS_ENOMATCH));
        assert_eq!(m.dlsym_next(2, 0), Err(DS_ENEXT));
        assert_ne!(DS_ENOMATCH, DS_ENEXT);
    }

    #[test]
    fn unload_keeps_link_order() {
        let mut m = LinkMap::seeded();
        let _ = m.load(0x10);
        let _ = m.load(0x11);
        let _ = m.load(0x12);
        assert_eq!(m.unload(1), Ok(2));
        assert_eq!(m.len(), 3);
        assert_eq!(m.order_of(3), Ok(2));
        assert_eq!(m.dlsym_default(0), Ok(3));
        assert_eq!(m.dlsym_next(2, 1), Ok(3));
        assert_eq!(m.order_of(1), Err(DS_ESCOPED));
        assert_eq!(m.order_of(9), Err(DS_EBADH));
    }

    #[test]
    fn gates_are_ordered() {
        let mut m = LinkMap::seeded();
        let _ = m.load(0x10);
        assert_eq!(m.dlsym_default(32), Err(DS_EINVAL));
        assert_eq!(m.dlsym_next(0, 0), Err(DS_EINVAL));
        assert_eq!(m.dlsym_next(0, 40), Err(DS_EINVAL));
        assert_eq!(m.dlsym_next(7, 0), Err(DS_EBADH));
        assert_eq!(m.dlsym_next(7, 40), Err(DS_EINVAL));
        assert_eq!(m.load(0x10), Err(DS_EDUP));
        assert_eq!(m.load(0), Err(DS_EINVAL));
        assert_eq!(m.load(0x99), Err(DS_ENOSYM));
    }

    #[test]
    fn capacity_is_exact() {
        let mut m = LinkMap::new();
        let mut i = 0u32;
        while (i as usize) < ICAP {
            let _ = m.add_img(Img::make(0xC0 + i, 1));
            i += 1;
        }
        let mut k = 0u32;
        while (k as usize) < CAP {
            assert!(m.load(0xC0 + k).is_ok());
            k += 1;
        }
        assert_eq!(m.load(0xC0 + (CAP as u32)), Err(DS_EFULL));
        assert_eq!(m.len(), CAP);
        assert_eq!(m.live_count(), CAP);
    }

    #[test]
    fn push_raw_extends_securely() {
        let mut m = LinkMap::new();
        assert_eq!(m.push_raw(0x30, 0b1), Some(1));
        assert_eq!(m.push_raw(0x31, 0b10), Some(2));
        assert_eq!(m.push_raw(0, 0b1), None);
        assert_eq!(m.dlsym_default(0), Ok(1));
        assert_eq!(m.dlsym_default(1), Ok(2));
        assert_eq!(m.len(), 2);
    }
}
