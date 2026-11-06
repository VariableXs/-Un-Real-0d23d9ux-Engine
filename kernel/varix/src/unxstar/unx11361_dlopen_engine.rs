//! UNX-F11361 dlopen 引擎消费面（0xec90 段）
//!
//! F11205 把 `dlopen` 的行为锚在了「句柄非空 / 对象已映入 / 符号可达」这三个
//! 字段上。本模块是这条顶层锚在 B08 批里的**批级实例化**：把目录（映像清单）、
//! 对象表（已装载实例）与符号可达性三件东西落到同一张表里，让每个字段都能被
//! 独立观测，而不是靠一句「返回非 NULL」概括。
//!
//! 装载是**原子的**：一条映像的段表要么整段映入（映射段数、字节数、基址全部
//! 落账），要么一格都不写。段表为空（`ENOSEG`）与段区间重叠（`ERELOC`）都在
//! 分配对象槽**之前**被拦下，因此失败不会留下半个对象——这一点用「失败后对象
//! 数不变、游标不前进」来验证，而不是靠错误码本身。
//!
//! 同一映像重复 `dlopen` 不新建对象，而是**复用同一句柄**并把引用计数加一；
//! 句柄因此是「对象身份」而不是「装载次数」。卸载是引用计数的逆运算：计数
//! 归零才真正释放槽位，释放后同一个句柄再卸载得到的是 `EREFS`（这个句柄曾经
//! 合法、只是对象已经走了），而从未分配过的句柄得到 `EBADH`。二者必须分开：
//! 前者是「用得太晚」，后者是「拿错了号」，混成一个码会让诊断工具把程序 bug
//! 和内存踩踏当成同一类。
//!
//! 符号可达性按对象的符号位图判定：位图为 1 才是该对象导出的符号，位为 0 的
//! 查询是 `ENOSYM`，不是「返回 0」。符号地址 = 对象基址 + 符号序号 × 槽宽，
//! 基址按 64 KiB 递增，故地址只增不叠。
//!
//! 闸门次序：
//! - `load`：零键 → 目录无此映像 → 已装载（复用）→ 段表为空 → 段区间重叠 →
//!   对象表满；
//! - `lookup`：句柄为零 → 无此句柄 → 位图不含该符号；
//! - `unload`：句柄为零 → 从未分配（`EBADH`）→ 已释放（`EREFS`）→ 计数减一
//!   或释放槽位。

use crate::alloc::format;
use crate::checks::CheckSet;

/// 段基址。
pub const SEG_BASE: u32 = 0xec90;

/// 映像键非法（零键或重复登记）。
pub const DE_EINVAL: u32 = SEG_BASE + 0x01;
/// 目录中没有这条映像。
pub const DE_ENOENT: u32 = SEG_BASE + 0x02;
/// 对象表已满。
pub const DE_EFULL: u32 = SEG_BASE + 0x03;
/// 映像段表为空，无从映入。
pub const DE_ENOSEG: u32 = SEG_BASE + 0x04;
/// 段区间重叠，不可重定位。
pub const DE_ERELOC: u32 = SEG_BASE + 0x05;
/// 符号不在该对象的导出位图内。
pub const DE_ENOSYM: u32 = SEG_BASE + 0x06;
/// 句柄从未分配（为零或越过分配水位）。
pub const DE_EBADH: u32 = SEG_BASE + 0x07;
/// 句柄曾合法，但其对象已完全卸载。
pub const DE_EREFS: u32 = SEG_BASE + 0x08;
/// 账本形状不符。
pub const DE_EBAL: u32 = SEG_BASE + 0x09;
/// 槽位乱序。
pub const DE_EORDER: u32 = SEG_BASE + 0x0a;
/// 观测值与复算不符。
pub const DE_EMISMATCH: u32 = SEG_BASE + 0x0b;
/// 冻结后重放不一致。
pub const DE_ESEAL: u32 = SEG_BASE + 0x0c;
/// 观察下标越界。
pub const DE_ENOBS: u32 = SEG_BASE + 0x0d;

/// 目录容量。
pub const ICAP: usize = 8;
/// 对象表容量。
pub const CAP: usize = 6;
/// 单映像最多段数。
pub const SEGS: usize = 4;
/// 账本格数。
pub const SAMPLES: usize = 14;
/// 基址游标起点。
pub const BASE_START: u64 = 0x4000_0000;
/// 每个对象占用的基址跨度。
pub const BASE_STRIDE: u64 = 0x1_0000;
/// 符号槽宽。
pub const SYM_STRIDE: u32 = 0x40;

/// 操作码：装载。
pub const OP_LOAD: u8 = 1;
/// 操作码：查符号。
pub const OP_LOOKUP: u8 = 2;
/// 操作码：读映射段数。
pub const OP_SEGS: u8 = 3;
/// 操作码：读映射字节。
pub const OP_BYTES: u8 = 4;
/// 操作码：读引用计数。
pub const OP_REFS: u8 = 5;
/// 操作码：卸载。
pub const OP_UNLOAD: u8 = 6;

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
pub struct Image {
    /// 映像键。
    pub key: u32,
    /// 段数。
    pub segs: u8,
    /// 各段文件内偏移。
    pub off: [u32; SEGS],
    /// 各段字节数。
    pub len: [u32; SEGS],
    /// 导出符号位图（最多 32 个符号）。
    pub syms: u32,
}

impl Image {
    /// 空映像。
    pub const fn blank() -> Image {
        Image { key: 0, segs: 0, off: [0; SEGS], len: [0; SEGS], syms: 0 }
    }

    /// 造一条映像（段偏移按 `i × 0x1000`，段长按调用者给定）。
    pub fn make(key: u32, count: u8, syms: u32) -> Image {
        let mut img = Image { key, segs: count, off: [0; SEGS], len: [0; SEGS], syms };
        let mut i = 0usize;
        while i < SEGS {
            if let Some(slot) = img.off.get_mut(i) {
                *slot = (i as u32).saturating_mul(0x1000);
            }
            if let Some(slot) = img.len.get_mut(i) {
                *slot = if i < count as usize { 0x800 } else { 0 };
            }
            i += 1;
        }
        img
    }

    /// 改写第 i 段（偏移, 长度）。越界返回 `None`。
    pub fn with_seg(mut self, i: usize, off: u32, len: u32) -> Option<Image> {
        if i >= SEGS {
            return None;
        }
        if let Some(slot) = self.off.get_mut(i) {
            *slot = off;
        }
        if let Some(slot) = self.len.get_mut(i) {
            *slot = len;
        }
        Some(self)
    }

    /// 段总字节数。
    pub fn total(&self) -> u32 {
        let mut t = 0u32;
        let mut i = 0usize;
        while i < self.segs as usize {
            if let Some(n) = self.len.get(i) {
                t = t.saturating_add(*n);
            }
            i += 1;
        }
        t
    }

    /// 段区间是否两两不交。
    pub fn disjoint(&self) -> bool {
        let mut i = 0usize;
        while i < self.segs as usize {
            let (oi, li) = match (self.off.get(i), self.len.get(i)) {
                (Some(a), Some(b)) => (*a, *b),
                _ => return false,
            };
            if li == 0 {
                return false;
            }
            let endi = match oi.checked_add(li) {
                Some(e) => e,
                None => return false,
            };
            let mut j = i + 1;
            while j < self.segs as usize {
                let (oj, lj) = match (self.off.get(j), self.len.get(j)) {
                    (Some(a), Some(b)) => (*a, *b),
                    _ => return false,
                };
                let endj = match oj.checked_add(lj) {
                    Some(e) => e,
                    None => return false,
                };
                if oi < endj && oj < endi {
                    return false;
                }
                j += 1;
            }
            i += 1;
        }
        true
    }
}

/// 一个已装载对象。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Obj {
    /// 映像键。
    pub key: u32,
    /// 句柄。
    pub handle: u32,
    /// 引用计数。
    pub refs: u32,
    /// 已映射段数。
    pub mapped: u8,
    /// 映射字节数。
    pub bytes: u32,
    /// 映射基址。
    pub base: u64,
    /// 槽位是否在用。
    pub live: bool,
}

impl Obj {
    /// 空槽。
    pub const fn blank() -> Obj {
        Obj { key: 0, handle: 0, refs: 0, mapped: 0, bytes: 0, base: 0, live: false }
    }
}

/// 装载器：目录 + 对象表。
#[derive(Clone, Copy, Debug)]
pub struct Loader {
    cat: [Image; ICAP],
    ncat: usize,
    obj: [Obj; CAP],
    cursor: u64,
    next_handle: u32,
    maps: u32,
    relocs: u32,
    unloads: u32,
}

impl Loader {
    /// 空装载器。
    pub const fn new() -> Loader {
        Loader {
            cat: [Image::blank(); ICAP],
            ncat: 0,
            obj: [Obj::blank(); CAP],
            cursor: BASE_START,
            next_handle: 1,
            maps: 0,
            relocs: 0,
            unloads: 0,
        }
    }

    /// 内置目录：libc / libssl / libzero（零段）/ liboverlap（重叠段）。
    pub fn seeded() -> Loader {
        let mut l = Loader::new();
        let _ = l.add_image(Image::make(0x11, 3, 0b1011));
        let _ = l.add_image(Image::make(0x22, 2, 0b0110));
        let _ = l.add_image(Image::make(0x33, 0, 0));
        let overlap = match Image::make(0x44, 2, 0b1).with_seg(1, 0x400, 0x200) {
            Some(i) => i,
            None => Image::make(0x44, 2, 0b1),
        };
        let _ = l.add_image(overlap);
        l
    }

    /// 登记一条目录映像。
    pub fn add_image(&mut self, img: Image) -> Result<usize, u32> {
        if img.key == 0 {
            return Err(DE_EINVAL);
        }
        if self.find_image(img.key).is_some() {
            return Err(DE_EINVAL);
        }
        if self.ncat >= ICAP {
            return Err(DE_EFULL);
        }
        if let Some(dst) = self.cat.get_mut(self.ncat) {
            *dst = img;
        }
        self.ncat += 1;
        Ok(self.ncat)
    }

    /// 目录条数。
    pub fn catalog_len(&self) -> usize {
        self.ncat
    }

    /// 按键查目录。
    pub fn find_image(&self, key: u32) -> Option<Image> {
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

    /// 累计映射段数。
    pub fn map_count(&self) -> u32 {
        self.maps
    }

    /// 累计重定位失败次数（重叠段）。
    pub fn reloc_fail_count(&self) -> u32 {
        self.relocs
    }

    /// 累计卸载次数。
    pub fn unload_count(&self) -> u32 {
        self.unloads
    }

    /// 基址游标（下一个对象将拿到的基址）。
    pub fn cursor(&self) -> u64 {
        self.cursor
    }

    /// 句柄水位（下一个将分配的句柄）。
    pub fn handle_high_water(&self) -> u32 {
        self.next_handle
    }

    /// 按句柄查对象下标。
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

    /// 按映像键查在用对象下标。
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

    /// 第一个空槽。
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

    /// 句柄是否在本装载器里分发过。
    pub fn handle_issued(&self, handle: u32) -> bool {
        handle != 0 && handle < self.next_handle
    }

    /// 句柄当前是否仍对应一个在用对象。
    pub fn handle_live(&self, handle: u32) -> bool {
        self.slot_of(handle).is_some()
    }

    /// 装载一条映像，返回句柄（非零）。重复装载复用句柄并加引用。
    pub fn load(&mut self, key: u32) -> Result<usize, u32> {
        if key == 0 {
            return Err(DE_EINVAL);
        }
        let img = match self.find_image(key) {
            Some(i) => i,
            None => return Err(DE_ENOENT),
        };
        if let Some(i) = self.slot_by_key(key) {
            if let Some(o) = self.obj.get_mut(i) {
                o.refs = o.refs.saturating_add(1);
                return Ok(o.handle as usize);
            }
            return Err(DE_EBADH);
        }
        if img.segs == 0 {
            return Err(DE_ENOSEG);
        }
        if !img.disjoint() {
            self.relocs = self.relocs.saturating_add(1);
            return Err(DE_ERELOC);
        }
        let slot = match self.free_slot() {
            Some(i) => i,
            None => return Err(DE_EFULL),
        };
        let handle = self.next_handle;
        let base = self.cursor;
        if let Some(o) = self.obj.get_mut(slot) {
            *o = Obj {
                key,
                handle,
                refs: 1,
                mapped: img.segs,
                bytes: img.total(),
                base,
                live: true,
            };
        }
        self.next_handle = self.next_handle.saturating_add(1);
        self.cursor = self.cursor.saturating_add(BASE_STRIDE);
        self.maps = self.maps.saturating_add(img.segs as u32);
        Ok(handle as usize)
    }

    /// 读对象字段。
    pub fn object_of(&self, handle: u32) -> Result<Obj, u32> {
        if handle == 0 {
            return Err(DE_EBADH);
        }
        match self.slot_of(handle) {
            Some(i) => match self.obj.get(i) {
                Some(o) => Ok(*o),
                None => Err(DE_EBADH),
            },
            None => {
                if self.handle_issued(handle) {
                    Err(DE_EREFS)
                } else {
                    Err(DE_EBADH)
                }
            }
        }
    }

    /// 符号地址 = 基址 + 序号 × 槽宽。
    pub fn lookup(&self, handle: u32, sym: u32) -> Result<u32, u32> {
        if handle == 0 {
            return Err(DE_EBADH);
        }
        let o = self.object_of(handle)?;
        let img = match self.find_image(o.key) {
            Some(i) => i,
            None => return Err(DE_ENOENT),
        };
        if sym >= 32 {
            return Err(DE_ENOSYM);
        }
        let bit = 1u32 << sym;
        if img.syms & bit == 0 {
            return Err(DE_ENOSYM);
        }
        let off = sym.saturating_mul(SYM_STRIDE);
        Ok((o.base as u32).wrapping_add(off))
    }

    /// 已映射段数。
    pub fn seg_count(&self, handle: u32) -> Result<usize, u32> {
        let o = self.object_of(handle)?;
        Ok(o.mapped as usize)
    }

    /// 已映射字节数。
    pub fn mapped_bytes(&self, handle: u32) -> Result<u32, u32> {
        let o = self.object_of(handle)?;
        Ok(o.bytes)
    }

    /// 引用计数。
    pub fn refs(&self, handle: u32) -> Result<u32, u32> {
        let o = self.object_of(handle)?;
        Ok(o.refs)
    }

    /// 卸载一次引用。计数归零则释放槽位，返回剩余在用对象数。
    pub fn unload(&mut self, handle: u32) -> Result<usize, u32> {
        if handle == 0 {
            return Err(DE_EBADH);
        }
        let slot = match self.slot_of(handle) {
            Some(i) => i,
            None => {
                if self.handle_issued(handle) {
                    return Err(DE_EREFS);
                }
                return Err(DE_EBADH);
            }
        };
        let refs = match self.obj.get(slot) {
            Some(o) => o.refs,
            None => return Err(DE_EBADH),
        };
        if refs > 1 {
            if let Some(o) = self.obj.get_mut(slot) {
                o.refs = o.refs.saturating_sub(1);
            }
            self.unloads = self.unloads.saturating_add(1);
            return Ok(self.object_count());
        }
        if let Some(o) = self.obj.get_mut(slot) {
            o.live = false;
            o.refs = 0;
        }
        self.unloads = self.unloads.saturating_add(1);
        Ok(self.object_count())
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
            return Err(DE_EORDER);
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
            return Err(DE_EBAL);
        }
        const VALS: [u32; 8] = [1, 2, 1, 0x4000_0040, 0x4001_0040, 3, 4096, 2];
        const CODES: [u32; 6] =
            [DE_EINVAL, DE_ENOENT, DE_ENOSYM, DE_EBADH, DE_EFULL, DE_ENOSEG];
        let mut vi = 0usize;
        let mut ci = 0usize;
        let mut i = 0usize;
        while i < self.len {
            let s = match self.rows.get(i) {
                Some(v) => *v,
                None => return Err(DE_EBAL),
            };
            if s.slot as usize != i {
                return Err(DE_EORDER);
            }
            if s.ok {
                if s.code != 0 {
                    return Err(DE_EMISMATCH);
                }
                match VALS.get(vi) {
                    Some(w) => {
                        if s.value != *w {
                            return Err(DE_EMISMATCH);
                        }
                    }
                    None => return Err(DE_EBAL),
                }
                vi += 1;
            } else {
                match CODES.get(ci) {
                    Some(w) => {
                        if s.code != *w {
                            return Err(DE_EMISMATCH);
                        }
                    }
                    None => return Err(DE_EBAL),
                }
                ci += 1;
            }
            i += 1;
        }
        if vi != 8 || ci != 6 {
            return Err(DE_EBAL);
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
            return Err(DE_EBAL);
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
            Err(DE_ESEAL)
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

/// 十四格账本：两次装载 → 复用 → 查符号 → 段/字节/引用 → 六类拒绝。
pub fn build_ledger() -> Ledger {
    let mut l = Ledger::new();
    let mut ld = Loader::seeded();

    let _ = l.register(cell(0, OP_LOAD, ld.load(0x11)));
    let _ = l.register(cell(1, OP_LOAD, ld.load(0x22)));
    let _ = l.register(cell(2, OP_LOAD, ld.load(0x11)));
    let _ = l.register(word_cell(3, OP_LOOKUP, ld.lookup(1, 1)));
    let _ = l.register(word_cell(4, OP_LOOKUP, ld.lookup(2, 1)));
    let _ = l.register(cell(5, OP_SEGS, ld.seg_count(1)));
    let _ = l.register(word_cell(6, OP_BYTES, ld.mapped_bytes(2)));
    let _ = l.register(word_cell(7, OP_REFS, ld.refs(1)));

    let _ = l.register(cell(8, OP_LOAD, ld.load(0)));

    let mut n = Loader::seeded();
    let _ = l.register(cell(9, OP_LOAD, n.load(0x99)));

    let _ = l.register(word_cell(10, OP_LOOKUP, ld.lookup(2, 0)));

    let _ = l.register(word_cell(11, OP_LOOKUP, ld.lookup(0xFFFF, 1)));

    let mut f = Loader::new();
    let mut i = 0u32;
    while (i as usize) < ICAP {
        let _ = f.add_image(Image::make(0xA0 + i, 1, 1));
        i += 1;
    }
    let mut k = 0u32;
    while (k as usize) < CAP {
        let _ = f.load(0xA0 + k);
        k += 1;
    }
    let _ = l.register(cell(12, OP_LOAD, f.load(0xA0 + (CAP as u32))));

    let mut z = Loader::seeded();
    let _ = l.register(cell(13, OP_LOAD, z.load(0x33)));

    l
}

/// 四路注入。
pub fn inject_four() -> [bool; 4] {
    let mut out = [false; 4];

    let l = build_ledger();
    let rep = l.replay();
    let root = l.seal();
    out[0] = l.len() == SAMPLES && l.audit().is_ok() && rep == Ok(root) && root != 0;

    // 一路：正常装载三字段（句柄非空 / 已映入 / 符号可达）逐字段成立。
    let mut a = Loader::seeded();
    let h = a.load(0x11);
    let handle = match h {
        Ok(v) => v,
        Err(_) => 0,
    };
    let obj = a.object_of(handle as u32);
    let sym = a.lookup(handle as u32, 1);
    let again = a.load(0x11);
    let refs = a.refs(handle as u32);
    let un1 = a.unload(handle as u32);
    let un2 = a.unload(handle as u32);
    let after = a.object_of(handle as u32);
    out[1] = h == Ok(1)
        && handle != 0
        && obj.map(|o| o.mapped).unwrap_or(0) == 3
        && obj.map(|o| o.bytes).unwrap_or(0) == 0x1800
        && obj.map(|o| o.base).unwrap_or(0) == BASE_START
        && sym == Ok(0x4000_0040)
        && again == Ok(handle)
        && refs == Ok(2)
        && un1 == Ok(1)
        && un2 == Ok(0)
        && after == Err(DE_EREFS)
        && a.object_count() == 0
        && a.unload_count() == 2;

    // 二路：装载失败是原子的——对象数与游标不变。
    let mut b = Loader::seeded();
    let before_n = b.object_count();
    let before_c = b.cursor();
    let r1 = b.load(0x44);
    let r2 = b.load(0x33);
    let after_n = b.object_count();
    let after_c = b.cursor();
    out[2] = r1 == Err(DE_ERELOC)
        && r2 == Err(DE_ENOSEG)
        && before_n == after_n
        && before_c == after_c
        && b.reloc_fail_count() == 1
        && b.handle_high_water() == 1
        && b.object_of(1) == Err(DE_EBADH);

    // 三路：句柄面四类失败互相可辨（零号 / 已释 / 未导出 / 野号）。
    let mut c = Loader::seeded();
    let h = match c.load(0x22) {
        Ok(v) => v as u32,
        Err(_) => 0,
    };
    let e1 = c.lookup(0, 1);
    let e3 = c.lookup(h, 0);
    let refs = c.refs(h);
    let segs = c.seg_count(h);
    let bytes = c.mapped_bytes(h);
    let _ = c.unload(h);
    let e2 = c.lookup(h, 1);
    let e4 = c.unload(0);
    out[3] = e1 == Err(DE_EBADH)
        && e2 == Err(DE_EREFS)
        && e3 == Err(DE_ENOSYM)
        && e4 == Err(DE_EBADH)
        && refs == Ok(1)
        && segs == Ok(2)
        && bytes == Ok(0x1000)
        && c.object_count() == 0;

    out
}

/// 十三项判据。
pub fn run_unx11361_checks() -> CheckSet {
    let mut s = CheckSet::new("unx11361.dlopen_engine");
    let l = build_ledger();

    s.add_dyn(
        "DE1-十四格八允六拒",
        l.len() == SAMPLES && l.ok_count() == 8 && l.err_count() == 6 && l.audit().is_ok(),
        &format!("n={} ok={} err={}", l.len(), l.ok_count(), l.err_count()),
    );

    s.add(
        "DE2-两次装载句柄从一递增",
        l.sample(0).map(|x| x.value) == Some(1)
            && l.sample(1).map(|x| x.value) == Some(2),
        "static",
    );

    s.add(
        "DE3-同映像重装复用句柄",
        l.sample(2).map(|x| x.value) == Some(1),
        "static",
    );

    s.add(
        "DE4-符号地址按基址加槽宽",
        l.sample(3).map(|x| x.value) == Some(0x4000_0040)
            && l.sample(4).map(|x| x.value) == Some(0x4001_0040),
        "static",
    );

    s.add(
        "DE5-段数与字节对账",
        l.sample(5).map(|x| x.value) == Some(3)
            && l.sample(6).map(|x| x.value) == Some(0x1000),
        "static",
    );

    s.add(
        "DE6-重载使引用计数为二",
        l.sample(7).map(|x| x.value) == Some(2),
        "static",
    );

    s.add(
        "DE7-零键装载被拒",
        l.sample(8).map(|x| x.code) == Some(DE_EINVAL),
        "static",
    );

    s.add(
        "DE8-目录无此映像",
        l.sample(9).map(|x| x.code) == Some(DE_ENOENT),
        "static",
    );

    s.add(
        "DE9-未导出符号不可达",
        l.sample(10).map(|x| x.code) == Some(DE_ENOSYM),
        "static",
    );

    s.add(
        "DE10-野句柄与对象表满",
        l.sample(11).map(|x| x.code) == Some(DE_EBADH)
            && l.sample(12).map(|x| x.code) == Some(DE_EFULL),
        "static",
    );

    s.add(
        "DE11-零段映像被拒",
        l.sample(13).map(|x| x.code) == Some(DE_ENOSEG),
        "static",
    );

    s.add(
        "DE12-重叠段回滚与双卸可辨",
        {
            let mut b = Loader::seeded();
            let n0 = b.object_count();
            let c0 = b.cursor();
            let reloc = b.load(0x44);
            let zero = b.load(0x33);
            let mut c = Loader::seeded();
            let h = match c.load(0x22) {
                Ok(v) => v as u32,
                Err(_) => 0,
            };
            let once = c.unload(h);
            let twice = c.unload(h);
            reloc == Err(DE_ERELOC)
                && zero == Err(DE_ENOSEG)
                && b.object_count() == n0
                && b.cursor() == c0
                && once == Ok(0)
                && twice == Err(DE_EREFS)
                && c.unload(0x999) == Err(DE_EBADH)
        },
        "static",
    );

    s.add(
        "DE13-冻结重放四注入",
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
        let s = run_unx11361_checks();
        assert_eq!(s.len(), 13);
        assert!(s.all_passed());
    }

    #[test]
    fn reload_shares_handle() {
        let mut l = Loader::seeded();
        assert_eq!(l.load(0x11), Ok(1));
        assert_eq!(l.load(0x11), Ok(1));
        assert_eq!(l.refs(1), Ok(2));
        assert_eq!(l.object_count(), 1);
        assert_eq!(l.handle_high_water(), 2);
    }

    #[test]
    fn unload_once_keeps_object() {
        let mut l = Loader::seeded();
        let _ = l.load(0x11);
        let _ = l.load(0x11);
        assert_eq!(l.unload(1), Ok(1));
        assert_eq!(l.refs(1), Ok(1));
        assert_eq!(l.seg_count(1), Ok(3));
        assert_eq!(l.unload(1), Ok(0));
        assert_eq!(l.object_of(1), Err(DE_EREFS));
    }

    #[test]
    fn overlap_is_atomic() {
        let mut l = Loader::seeded();
        let c0 = l.cursor();
        assert_eq!(l.load(0x44), Err(DE_ERELOC));
        assert_eq!(l.object_count(), 0);
        assert_eq!(l.cursor(), c0);
        assert_eq!(l.reloc_fail_count(), 1);
    }

    #[test]
    fn symbol_gate_order() {
        let mut l = Loader::seeded();
        let _ = l.load(0x22);
        assert_eq!(l.lookup(0, 1), Err(DE_EBADH));
        assert_eq!(l.lookup(1, 0), Err(DE_ENOSYM));
        assert_eq!(l.lookup(1, 32), Err(DE_ENOSYM));
        assert_eq!(l.lookup(1, 1), Ok(0x4000_0040));
    }

    #[test]
    fn capacity_is_exact() {
        let mut l = Loader::new();
        let mut i = 0u32;
        while (i as usize) < ICAP {
            assert!(l.add_image(Image::make(0xB0 + i, 1, 1)).is_ok());
            i += 1;
        }
        let mut k = 0u32;
        while (k as usize) < CAP {
            assert!(l.load(0xB0 + k).is_ok());
            k += 1;
        }
        assert_eq!(l.load(0xB0 + (CAP as u32)), Err(DE_EFULL));
        assert_eq!(l.object_count(), CAP);
    }

    #[test]
    fn catalog_rejects_zero_and_duplicate() {
        let mut l = Loader::new();
        assert_eq!(l.add_image(Image::make(0, 1, 0)), Err(DE_EINVAL));
        assert!(l.add_image(Image::make(7, 1, 0)).is_ok());
        assert_eq!(l.add_image(Image::make(7, 2, 0)), Err(DE_EINVAL));
        assert_eq!(l.catalog_len(), 1);
        assert_eq!(l.find_image(7).map(|i| i.segs), Some(1));
        assert!(l.find_image(8).is_none());
    }
}
