//! UNX-F11362 RTLD_LAZY 与 RTLD_NOW 绑定模式（0xeca0 段）
//!
//! 两种绑定模式的差别不在「绑不绑」，而在**什么时候必须绑完**：
//!
//! - `RTLD_NOW` 在 `dlopen` 返回之前把对象的重定位表全部解析掉。只要有一个
//!   符号在作用域里找不到，装载当场失败——`dlopen` 返回 NULL，对象一个字节都
//!   不留在进程里。好处是错误在装载点暴露，坏处是启动慢。
//! - `RTLD_LAZY` 只做必要的搬移，把符号解析推迟到**首次调用**该槽位。装载一定
//!   成功（哪怕表里全是未定义符号），第一调用到缺失符号的那条路径才失败，
//!   失败被记在「首次调用」这一点上，而不是装载点。
//!
//! 因此同一条映像、同一个缺失符号，在 NOW 下是「装载失败」，在 LAZY 下是
//! 「装载成功 + 首调失败」。本模块把这两个失败点分别记账（`now_fails` /
//! `lazy_fails`），并保证：
//!
//! 1. **NOW 原子**：装载失败不占对象槽、不推进句柄水位、不增加已绑定计数；
//! 2. **LAZY 幂等**：同一槽位首次调用失败后，槽位被标记为「已尝试且失败」，
//!    再次调用返回同一个 `EUNSYM`，而不是反复重试（重试会把诊断日志刷爆）；
//! 3. **绑定只发生一次**：成功绑定的槽位 `bound = true`，后续调用直接取地址，
//!    绑定计数不随调用次数增长。
//!
//! 闸门次序：
//! - `load_now` / `load_lazy`：零键 → 无此映像 → 同映像已装载 → 无重定位槽 →
//!   NOW 全绑定（遇缺即回滚）→ 表满；
//! - `call`：句柄为零 → 无此句柄 → 无此槽号 → 已绑定直接返回 → 惰性首绑定；
//! - `bound_of`：句柄 → 槽号，再读绑定态（未绑定时 `EUNBOUND`，不是 0）。

use crate::alloc::format;
use crate::checks::CheckSet;

/// 段基址。
pub const SEG_BASE: u32 = 0xeca0;

/// 映像键或槽号非法。
pub const BM_EINVAL: u32 = SEG_BASE + 0x01;
/// 目录中没有这条映像。
pub const BM_ENOENT: u32 = SEG_BASE + 0x02;
/// 同一映像重复装载。
pub const BM_EDUP: u32 = SEG_BASE + 0x03;
/// 对象表已满。
pub const BM_EFULL: u32 = SEG_BASE + 0x04;
/// 映像没有重定位槽。
pub const BM_ENOSLOT: u32 = SEG_BASE + 0x05;
/// 符号未定义（NOW 在装载期、LAZY 在首调期触发）。
pub const BM_EUNSYM: u32 = SEG_BASE + 0x06;
/// 句柄无效。
pub const BM_EBADH: u32 = SEG_BASE + 0x07;
/// 槽位尚未绑定。
pub const BM_EUNBOUND: u32 = SEG_BASE + 0x08;
/// 账本形状不符。
pub const BM_EBAL: u32 = SEG_BASE + 0x09;
/// 槽位乱序。
pub const BM_EORDER: u32 = SEG_BASE + 0x0a;
/// 观测值与复算不符。
pub const BM_EMISMATCH: u32 = SEG_BASE + 0x0b;
/// 冻结后重放不一致。
pub const BM_ESEAL: u32 = SEG_BASE + 0x0c;
/// 观察下标越界。
pub const BM_ENOBS: u32 = SEG_BASE + 0x0d;

/// 目录容量。
pub const ICAP: usize = 6;
/// 对象容量。
pub const CAP: usize = 5;
/// 单映像重定位槽数。
pub const SLOTS: usize = 4;
/// 账本格数。
pub const SAMPLES: usize = 14;
/// 基址起点。
pub const BASE_START: u64 = 0x5000_0000;
/// 基址跨度。
pub const BASE_STRIDE: u64 = 0x1_0000;
/// 符号槽宽。
pub const SYM_STRIDE: u32 = 0x40;

/// 模式：立即绑定。
pub const M_NOW: u8 = 1;
/// 模式：惰性绑定。
pub const M_LAZY: u8 = 2;

/// 操作码：立即装载。
pub const OP_NOW: u8 = 1;
/// 操作码：惰性装载。
pub const OP_LAZY: u8 = 2;
/// 操作码：调用。
pub const OP_CALL: u8 = 3;
/// 操作码：读绑定态。
pub const OP_BOUND: u8 = 4;
/// 操作码：读模式。
pub const OP_MODE: u8 = 5;
/// 操作码：读调用次数。
pub const OP_CALLS: u8 = 6;

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

/// 一条待装载映像。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Img {
    /// 映像键。
    pub key: u32,
    /// 重定位槽数。
    pub nslots: u8,
    /// 存在位图：bit i 为 1 表示槽 i 的符号可解析。
    pub present: u32,
}

impl Img {
    /// 造一条映像（前 `n` 槽，`present` 指明哪些符号可解析）。
    pub const fn make(key: u32, n: u8, present: u32) -> Img {
        Img { key, nslots: n, present }
    }

    /// 槽 i 的符号是否可解析。
    pub fn has(&self, i: usize) -> bool {
        if i >= self.nslots as usize {
            return false;
        }
        self.present & (1u32 << i) != 0
    }

    /// 缺失槽数。
    pub fn missing(&self) -> usize {
        let mut k = 0usize;
        let mut i = 0usize;
        while i < self.nslots as usize {
            if !self.has(i) {
                k += 1;
            }
            i += 1;
        }
        k
    }
}

/// 一个重定位槽。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Slot {
    /// 是否已绑定。
    pub bound: bool,
    /// 是否尝试过且失败。
    pub failed: bool,
    /// 该槽被调用的次数。
    pub calls: u32,
}

impl Slot {
    /// 空槽。
    pub const fn blank() -> Slot {
        Slot { bound: false, failed: false, calls: 0 }
    }
}

/// 一个已装载对象。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Obj {
    /// 映像键。
    pub key: u32,
    /// 句柄。
    pub handle: u32,
    /// 绑定模式。
    pub mode: u8,
    /// 映射基址。
    pub base: u64,
    /// 槽数。
    pub nslots: u8,
    /// 槽表。
    pub slots: [Slot; SLOTS],
    /// 是否在用。
    pub live: bool,
}

impl Obj {
    /// 空对象。
    pub const fn blank() -> Obj {
        Obj {
            key: 0,
            handle: 0,
            mode: 0,
            base: 0,
            nslots: 0,
            slots: [Slot::blank(); SLOTS],
            live: false,
        }
    }

    /// 已绑定槽数。
    pub fn bound(&self) -> usize {
        let mut k = 0usize;
        let mut i = 0usize;
        while i < self.nslots as usize {
            if let Some(s) = self.slots.get(i) {
                if s.bound {
                    k += 1;
                }
            }
            i += 1;
        }
        k
    }

    /// 累计调用次数。
    pub fn calls(&self) -> u32 {
        let mut t = 0u32;
        let mut i = 0usize;
        while i < self.nslots as usize {
            if let Some(s) = self.slots.get(i) {
                t = t.saturating_add(s.calls);
            }
            i += 1;
        }
        t
    }
}

/// 装载器。
#[derive(Clone, Copy, Debug)]
pub struct Linker {
    cat: [Img; ICAP],
    ncat: usize,
    obj: [Obj; CAP],
    cursor: u64,
    next_handle: u32,
    now_binds: u32,
    lazy_binds: u32,
    now_fails: u32,
    lazy_fails: u32,
}

impl Linker {
    /// 空装载器。
    pub const fn new() -> Linker {
        Linker {
            cat: [Img { key: 0, nslots: 0, present: 0 }; ICAP],
            ncat: 0,
            obj: [Obj::blank(); CAP],
            cursor: BASE_START,
            next_handle: 1,
            now_binds: 0,
            lazy_binds: 0,
            now_fails: 0,
            lazy_fails: 0,
        }
    }

    /// 内置目录：两条全解析映像 / 空槽映像 / 缺首符号映像。
    pub fn seeded() -> Linker {
        let mut l = Linker::new();
        let _ = l.add_img(Img::make(0x11, 3, 0b111));
        let _ = l.add_img(Img::make(0x22, 3, 0b111));
        let _ = l.add_img(Img::make(0x33, 0, 0));
        let _ = l.add_img(Img::make(0x44, 3, 0b110));
        l
    }

    /// 登记映像。
    pub fn add_img(&mut self, img: Img) -> Result<usize, u32> {
        if img.key == 0 {
            return Err(BM_EINVAL);
        }
        if self.find_img(img.key).is_some() {
            return Err(BM_EDUP);
        }
        if self.ncat >= ICAP {
            return Err(BM_EFULL);
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

    /// 立即绑定计数。
    pub fn now_bind_count(&self) -> u32 {
        self.now_binds
    }

    /// 惰性绑定计数。
    pub fn lazy_bind_count(&self) -> u32 {
        self.lazy_binds
    }

    /// 装载期失败次数。
    pub fn now_fail_count(&self) -> u32 {
        self.now_fails
    }

    /// 首调期失败次数。
    pub fn lazy_fail_count(&self) -> u32 {
        self.lazy_fails
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

    /// 按映像键取在用对象句柄。
    pub fn handle_of(&self, key: u32) -> Option<u32> {
        match self.slot_by_key(key) {
            Some(i) => self.obj.get(i).map(|o| o.handle),
            None => None,
        }
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

    /// 按模式装载。`now = true` 走立即绑定。
    pub fn load(&mut self, key: u32, now: bool) -> Result<usize, u32> {
        if key == 0 {
            return Err(BM_EINVAL);
        }
        let img = match self.find_img(key) {
            Some(i) => i,
            None => return Err(BM_ENOENT),
        };
        if self.slot_by_key(key).is_some() {
            return Err(BM_EDUP);
        }
        if img.nslots == 0 {
            return Err(BM_ENOSLOT);
        }
        if now && img.missing() != 0 {
            self.now_fails = self.now_fails.saturating_add(1);
            return Err(BM_EUNSYM);
        }
        let slot = match self.free_slot() {
            Some(i) => i,
            None => return Err(BM_EFULL),
        };
        let mut slots = [Slot::blank(); SLOTS];
        let mut i = 0usize;
        while i < img.nslots as usize {
            if now {
                if let Some(s) = slots.get_mut(i) {
                    s.bound = true;
                }
            }
            i += 1;
        }
        let handle = self.next_handle;
        let base = self.cursor;
        if let Some(o) = self.obj.get_mut(slot) {
            *o = Obj {
                key,
                handle,
                mode: if now { M_NOW } else { M_LAZY },
                base,
                nslots: img.nslots,
                slots,
                live: true,
            };
        }
        self.next_handle = self.next_handle.saturating_add(1);
        self.cursor = self.cursor.saturating_add(BASE_STRIDE);
        if now {
            self.now_binds = self.now_binds.saturating_add(img.nslots as u32);
            Ok(img.nslots as usize)
        } else {
            Ok(0)
        }
    }

    /// 立即绑定装载。
    pub fn load_now(&mut self, key: u32) -> Result<usize, u32> {
        self.load(key, true)
    }

    /// 惰性绑定装载。
    pub fn load_lazy(&mut self, key: u32) -> Result<usize, u32> {
        self.load(key, false)
    }

    /// 读对象。
    pub fn object_of(&self, handle: u32) -> Result<Obj, u32> {
        if handle == 0 {
            return Err(BM_EBADH);
        }
        match self.slot_of(handle) {
            Some(i) => match self.obj.get(i) {
                Some(o) => Ok(*o),
                None => Err(BM_EBADH),
            },
            None => Err(BM_EBADH),
        }
    }

    /// 绑定态：1 已绑 / 0 未绑。未绑定返回 `EUNBOUND` 而不是伪装成 0。
    pub fn bound_of(&self, handle: u32, i: usize) -> Result<u32, u32> {
        let o = self.object_of(handle)?;
        if i >= o.nslots as usize {
            return Err(BM_ENOSLOT);
        }
        match o.slots.get(i) {
            Some(s) => {
                if s.bound {
                    Ok(1)
                } else if s.failed {
                    Err(BM_EUNSYM)
                } else {
                    Err(BM_EUNBOUND)
                }
            }
            None => Err(BM_ENOSLOT),
        }
    }

    /// 调用槽 i：已绑定直接返回地址；惰性未绑则现场解析。
    pub fn call(&mut self, handle: u32, i: usize) -> Result<u32, u32> {
        if handle == 0 {
            return Err(BM_EBADH);
        }
        let slot = match self.slot_of(handle) {
            Some(s) => s,
            None => return Err(BM_EBADH),
        };
        let (key, base, nslots) = match self.obj.get(slot) {
            Some(o) => (o.key, o.base, o.nslots),
            None => return Err(BM_EBADH),
        };
        if i >= nslots as usize {
            return Err(BM_ENOSLOT);
        }
        let img = match self.find_img(key) {
            Some(v) => v,
            None => return Err(BM_ENOENT),
        };
        let (bound, failed) = match self.obj.get(slot) {
            Some(o) => match o.slots.get(i) {
                Some(s) => (s.bound, s.failed),
                None => return Err(BM_ENOSLOT),
            },
            None => return Err(BM_EBADH),
        };
        if bound {
            if let Some(o) = self.obj.get_mut(slot) {
                if let Some(s) = o.slots.get_mut(i) {
                    s.calls = s.calls.saturating_add(1);
                }
            }
            return Ok((base as u32).wrapping_add((i as u32).saturating_mul(SYM_STRIDE)));
        }
        if failed {
            if let Some(o) = self.obj.get_mut(slot) {
                if let Some(s) = o.slots.get_mut(i) {
                    s.calls = s.calls.saturating_add(1);
                }
            }
            self.lazy_fails = self.lazy_fails.saturating_add(1);
            return Err(BM_EUNSYM);
        }
        if !img.has(i) {
            if let Some(o) = self.obj.get_mut(slot) {
                if let Some(s) = o.slots.get_mut(i) {
                    s.failed = true;
                    s.calls = s.calls.saturating_add(1);
                }
            }
            self.lazy_fails = self.lazy_fails.saturating_add(1);
            return Err(BM_EUNSYM);
        }
        if let Some(o) = self.obj.get_mut(slot) {
            if let Some(s) = o.slots.get_mut(i) {
                s.bound = true;
                s.calls = s.calls.saturating_add(1);
            }
        }
        self.lazy_binds = self.lazy_binds.saturating_add(1);
        Ok((base as u32).wrapping_add((i as u32).saturating_mul(SYM_STRIDE)))
    }

    /// 对象模式。
    pub fn mode_of(&self, handle: u32) -> Result<u32, u32> {
        let o = self.object_of(handle)?;
        Ok(o.mode as u32)
    }

    /// 已绑定槽数。
    pub fn bound_count(&self, handle: u32) -> Result<usize, u32> {
        let o = self.object_of(handle)?;
        Ok(o.bound())
    }

    /// 累计调用次数。
    pub fn call_count(&self, handle: u32) -> Result<u32, u32> {
        let o = self.object_of(handle)?;
        Ok(o.calls())
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
            return Err(BM_EORDER);
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
            return Err(BM_EBAL);
        }
        const VALS: [u32; 8] = [3, 0, 0x5000_0040, 0x5001_0000, 0x5001_0000, 1, 2, 2];
        const CODES: [u32; 6] =
            [BM_EINVAL, BM_ENOENT, BM_EUNSYM, BM_EBADH, BM_EDUP, BM_EFULL];
        let mut vi = 0usize;
        let mut ci = 0usize;
        let mut i = 0usize;
        while i < self.len {
            let s = match self.rows.get(i) {
                Some(v) => *v,
                None => return Err(BM_EBAL),
            };
            if s.slot as usize != i {
                return Err(BM_EORDER);
            }
            if s.ok {
                if s.code != 0 {
                    return Err(BM_EMISMATCH);
                }
                match VALS.get(vi) {
                    Some(w) => {
                        if s.value != *w {
                            return Err(BM_EMISMATCH);
                        }
                    }
                    None => return Err(BM_EBAL),
                }
                vi += 1;
            } else {
                match CODES.get(ci) {
                    Some(w) => {
                        if s.code != *w {
                            return Err(BM_EMISMATCH);
                        }
                    }
                    None => return Err(BM_EBAL),
                }
                ci += 1;
            }
            i += 1;
        }
        if vi != 8 || ci != 6 {
            return Err(BM_EBAL);
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
            return Err(BM_EBAL);
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
            Err(BM_ESEAL)
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

/// 十四格账本：两模式装载 → 调用绑定 → 绑定态/模式/调用数 → 六类拒绝。
pub fn build_ledger() -> Ledger {
    let mut l = Ledger::new();
    let mut lk = Linker::seeded();

    let _ = l.register(cell(0, OP_NOW, lk.load_now(0x11)));
    let _ = l.register(cell(1, OP_LAZY, lk.load_lazy(0x22)));
    let _ = l.register(word_cell(2, OP_CALL, lk.call(1, 1)));
    let _ = l.register(word_cell(3, OP_CALL, lk.call(2, 0)));
    let _ = l.register(word_cell(4, OP_CALL, lk.call(2, 0)));
    let _ = l.register(cell(5, OP_BOUND, lk.bound_count(2)));
    let _ = l.register(word_cell(6, OP_MODE, lk.mode_of(2)));
    let _ = l.register(word_cell(7, OP_CALLS, lk.call_count(2)));

    let _ = l.register(cell(8, OP_NOW, lk.load_now(0)));

    let mut n = Linker::seeded();
    let _ = l.register(cell(9, OP_NOW, n.load_now(0x77)));

    let mut m = Linker::seeded();
    let _ = l.register(cell(10, OP_NOW, m.load_now(0x44)));

    let _ = l.register(word_cell(11, OP_CALL, lk.call(0, 0)));

    let _ = l.register(cell(12, OP_NOW, lk.load_now(0x11)));

    let mut f = Linker::new();
    let _ = f.add_img(Img::make(0xB0, 1, 1));
    let _ = f.add_img(Img::make(0xB1, 1, 1));
    let _ = f.add_img(Img::make(0xB2, 1, 1));
    let _ = f.add_img(Img::make(0xB3, 1, 1));
    let _ = f.add_img(Img::make(0xB4, 1, 1));
    let _ = f.add_img(Img::make(0xB5, 1, 1));
    let _ = f.load_now(0xB0);
    let _ = f.load_now(0xB1);
    let _ = f.load_now(0xB2);
    let _ = f.load_now(0xB3);
    let _ = f.load_now(0xB4);
    let _ = l.register(cell(13, OP_NOW, f.load_now(0xB5)));

    l
}

/// 四路注入。
pub fn inject_four() -> [bool; 4] {
    let mut out = [false; 4];

    let l = build_ledger();
    let rep = l.replay();
    let root = l.seal();
    out[0] = l.len() == SAMPLES && l.audit().is_ok() && rep == Ok(root) && root != 0;

    // 一路：同一缺符号映像，NOW 装载即败、LAZY 装载成功而首调才败。
    let mut a = Linker::seeded();
    let now = a.load_now(0x44);
    let now_objs = a.object_count();
    let now_hw = a.handle_high_water();
    let lazy = a.load_lazy(0x44);
    let h = match a.handle_of(0x44) {
        Some(v) => v,
        None => 0,
    };
    let first = a.call(h, 0);
    let second = a.call(h, 0);
    let good = a.call(h, 1);
    let bound1 = a.bound_count(h);
    out[1] = now == Err(BM_EUNSYM)
        && now_objs == 0
        && now_hw == 1
        && lazy == Ok(0)
        && first == Err(BM_EUNSYM)
        && second == Err(BM_EUNSYM)
        && good == Ok(0x5000_0040)
        && bound1 == Ok(1)
        && a.lazy_fail_count() == 2
        && a.lazy_bind_count() == 1
        && a.mode_of(h) == Ok(M_LAZY as u32);

    // 二路：NOW 全绑定，绑定计数不随调用增长；只绑一次。
    let mut b = Linker::seeded();
    let _ = b.load_now(0x11);
    let hb = match b.handle_of(0x11) {
        Some(v) => v,
        None => 0,
    };
    let _ = b.call(hb, 0);
    let _ = b.call(hb, 0);
    let _ = b.call(hb, 2);
    out[2] = b.bound_count(hb) == Ok(3)
        && b.now_bind_count() == 3
        && b.lazy_bind_count() == 0
        && b.call_count(hb) == Ok(3)
        && b.bound_of(hb, 0) == Ok(1)
        && b.mode_of(hb) == Ok(M_NOW as u32)
        && b.now_fail_count() == 0;

    // 三路：四条闸门边界（零键 / 无映像 / 空槽映像 / 越界槽）。
    let mut c = Linker::seeded();
    let g1 = c.load_now(0);
    let g2 = c.load_lazy(0x99);
    let g3 = c.load_now(0x33);
    let _ = c.load_lazy(0x11);
    let hc = match c.handle_of(0x11) {
        Some(v) => v,
        None => 0,
    };
    let g4 = c.call(hc, 9);
    let g5 = c.bound_of(hc, 0);
    let g6 = c.bound_of(hc, 9);
    out[3] = g1 == Err(BM_EINVAL)
        && g2 == Err(BM_ENOENT)
        && g3 == Err(BM_ENOSLOT)
        && g4 == Err(BM_ENOSLOT)
        && g5 == Err(BM_EUNBOUND)
        && g6 == Err(BM_ENOSLOT)
        && c.lazy_bind_count() == 0
        && c.now_bind_count() == 0;

    out
}

/// 十三项判据。
pub fn run_unx11362_checks() -> CheckSet {
    let mut s = CheckSet::new("unx11362.dl_bindmode");
    let l = build_ledger();

    s.add_dyn(
        "BM1-十四格八允六拒",
        l.len() == SAMPLES && l.ok_count() == 8 && l.err_count() == 6 && l.audit().is_ok(),
        &format!("n={} ok={} err={}", l.len(), l.ok_count(), l.err_count()),
    );

    s.add(
        "BM2-NOW装载即全绑定",
        l.sample(0).map(|x| x.value) == Some(3),
        "static",
    );

    s.add(
        "BM3-LAZY装载零绑定",
        l.sample(1).map(|x| x.value) == Some(0),
        "static",
    );

    s.add(
        "BM4-两模式符号地址一致",
        l.sample(2).map(|x| x.value) == Some(0x5000_0040)
            && l.sample(3).map(|x| x.value) == Some(0x5001_0000),
        "static",
    );

    s.add(
        "BM5-重复调用地址稳定",
        l.sample(4).map(|x| x.value) == Some(0x5001_0000),
        "static",
    );

    s.add(
        "BM6-惰性首调只绑一槽",
        l.sample(5).map(|x| x.value) == Some(1),
        "static",
    );

    s.add(
        "BM7-模式可回读",
        l.sample(6).map(|x| x.value) == Some(M_LAZY as u32),
        "static",
    );

    s.add(
        "BM8-调用次数累计",
        l.sample(7).map(|x| x.value) == Some(2),
        "static",
    );

    s.add(
        "BM9-零键装载被拒",
        l.sample(8).map(|x| x.code) == Some(BM_EINVAL),
        "static",
    );

    s.add(
        "BM10-无此映像与NOW缺符号",
        l.sample(9).map(|x| x.code) == Some(BM_ENOENT)
            && l.sample(10).map(|x| x.code) == Some(BM_EUNSYM),
        "static",
    );

    s.add(
        "BM11-野句柄调用被拒",
        l.sample(11).map(|x| x.code) == Some(BM_EBADH),
        "static",
    );

    s.add(
        "BM12-重装与表满各自成类",
        {
            let l1 = l.sample(12).map(|x| x.code) == Some(BM_EDUP);
            let l2 = l.sample(13).map(|x| x.code) == Some(BM_EFULL);
            let mut a = Linker::seeded();
            let now = a.load_now(0x44);
            let atomic = a.object_count() == 0 && a.handle_high_water() == 1;
            l1 && l2 && now == Err(BM_EUNSYM) && atomic && a.now_fail_count() == 1
        },
        "static",
    );

    s.add(
        "BM13-冻结重放四注入",
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
        let s = run_unx11362_checks();
        assert_eq!(s.len(), 13);
        assert!(s.all_passed());
    }

    #[test]
    fn now_fails_at_load() {
        let mut l = Linker::seeded();
        assert_eq!(l.load_now(0x44), Err(BM_EUNSYM));
        assert_eq!(l.object_count(), 0);
        assert_eq!(l.handle_high_water(), 1);
        assert_eq!(l.now_fail_count(), 1);
    }

    #[test]
    fn lazy_fails_at_first_call() {
        let mut l = Linker::seeded();
        assert_eq!(l.load_lazy(0x44), Ok(0));
        assert_eq!(l.bound_count(1), Ok(0));
        assert_eq!(l.call(1, 0), Err(BM_EUNSYM));
        assert_eq!(l.call(1, 0), Err(BM_EUNSYM));
        assert_eq!(l.lazy_fail_count(), 2);
        assert_eq!(l.bound_count(1), Ok(0));
        assert_eq!(l.call(1, 1), Ok(0x5000_0040));
        assert_eq!(l.bound_count(1), Ok(1));
    }

    #[test]
    fn bind_once_only() {
        let mut l = Linker::seeded();
        let _ = l.load_lazy(0x11);
        assert_eq!(l.call(1, 0), Ok(0x5000_0000));
        assert_eq!(l.call(1, 0), Ok(0x5000_0000));
        assert_eq!(l.lazy_bind_count(), 1);
        assert_eq!(l.call_count(1), Ok(2));
        assert_eq!(l.bound_of(1, 1), Err(BM_EUNBOUND));
    }

    #[test]
    fn gates_and_empty_slot_image() {
        let mut l = Linker::seeded();
        assert_eq!(l.load_now(0), Err(BM_EINVAL));
        assert_eq!(l.load_now(0x99), Err(BM_ENOENT));
        assert_eq!(l.load_now(0x33), Err(BM_ENOSLOT));
        assert_eq!(l.call(1, 0), Err(BM_EBADH));
        assert_eq!(l.bound_of(0, 0), Err(BM_EBADH));
    }

    #[test]
    fn capacity_is_exact() {
        let mut l = Linker::new();
        let mut i = 0u32;
        while (i as usize) < ICAP {
            let _ = l.add_img(Img::make(0xC0 + i, 1, 1));
            i += 1;
        }
        let mut k = 0u32;
        while (k as usize) < CAP {
            assert!(l.load_now(0xC0 + k).is_ok());
            k += 1;
        }
        assert_eq!(l.load_now(0xC0 + (CAP as u32)), Err(BM_EFULL));
        assert_eq!(l.object_count(), CAP);
    }

    #[test]
    fn missing_count_is_exact() {
        let i = Img::make(9, 3, 0b101);
        assert_eq!(i.missing(), 1);
        assert!(i.has(0));
        assert!(!i.has(1));
        assert!(!i.has(9));
        assert_eq!(Img::make(8, 0, 0).missing(), 0);
    }
}
