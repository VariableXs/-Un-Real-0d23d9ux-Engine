//! UNX-F11307 静态 TLS 剩余空间 surplus（0xe920 段）
//!
//! 静态 TLS 窗口在启动期按初始模块集一次性定尺，运行期 dlopen 的模块只能
//! 从窗口尾部预留的一段 surplus 里挤位置。这段空间有三个特征：总量固定、
//! 只能顺序推进或复用已释放的空洞、从不做碎片整理。装载器因此必须把每一
//! 字节的归属记清楚：活跃块、空洞、以及对齐填充。
//!
//! 模型把 surplus 记账成三元恒等式：`used + free + wasted == top`。
//! `used` 是活跃块尺寸之和，`free` 是空洞尺寸之和，`wasted` 是对齐前导
//! 填充——它既不能被后续分配使用，也不随释放回收，属于永久沉没成本。
//! 这条恒等式在每一格样本上都必须成立，任何一字节去向不明即视为账本失衡。
//!
//! 分配走两段式首次适配：先在空洞里找起点已对齐且容量足够的洞，再退一步
//! 找需要前导填充的洞，两者都失败才推进顶沿。空洞不做相邻合并——这是本
//! 模型刻意的保守策略：两个相邻小洞合计够大，但单独都不够，请求仍会推进
//! 顶沿，从而把“碎片化代价”显式写进轨迹，而不是靠合并掩盖。
//!
//! 释放只认活跃块起始偏移，重复释放与未知偏移一律拒绝；拒绝路径不改任何
//! 计数、不动顶沿、不产生空洞。账本十四格：九格成功（七次分配、两次释放）、
//! 五格拒绝（预算耗尽、尺寸为零、对齐非法两格、释放未知块），逐格记录
//! 分配结果与三元组状态，用于验证记账恒等式与首次适配顺序。

use crate::alloc::format;
use crate::checks::CheckSet;

/// 段基址。
pub const SEG_BASE: u32 = 0xe920;

/// 编号非法。
pub const SP_EID: u32 = SEG_BASE + 0x01;
/// 登记顺序错误。
pub const SP_EORDER: u32 = SEG_BASE + 0x02;
/// 重复登记。
pub const SP_EDUP: u32 = SEG_BASE + 0x03;
/// 已冻结。
pub const SP_EFROZEN: u32 = SEG_BASE + 0x04;
/// 观测不存在的格。
pub const SP_ENOBS: u32 = SEG_BASE + 0x05;
/// 结论不符。
pub const SP_EMISMATCH: u32 = SEG_BASE + 0x06;
/// 账本失衡。
pub const SP_EBAL: u32 = SEG_BASE + 0x07;
/// 尺寸为零。
pub const SP_ESIZE: u32 = SEG_BASE + 0x08;
/// 对齐非法。
pub const SP_EALIGN: u32 = SEG_BASE + 0x09;
/// 预算耗尽。
pub const SP_EFULL: u32 = SEG_BASE + 0x0a;
/// 释放未知块。
pub const SP_ENOBLK: u32 = SEG_BASE + 0x0b;
/// 空洞表满。
pub const SP_EHOLE: u32 = SEG_BASE + 0x0c;
/// 票号不一致。
pub const SP_ESEAL: u32 = SEG_BASE + 0x0d;

/// 判决：成功。
pub const V_OK: u32 = 0;

/// surplus 总预算。
pub const SURPLUS_CAP: u32 = 512;
/// 活跃块上限。
pub const LIVE_CAP: usize = 6;
/// 空洞上限。
pub const HOLE_CAP: usize = 4;
/// 样本格数。
pub const SAMPLES: usize = 14;

/// 操作：分配。
pub const OP_RESERVE: u8 = 1;
/// 操作：释放。
pub const OP_RELEASE: u8 = 2;

/// FNV 偏移基。
pub const FNV_OFF: u64 = 0xcbf2_9ce4_8422_2325;
/// FNV 质数。
pub const FNV_PRIME: u64 = 0x100_0000_01b3;

/// 2 的幂判定。
pub fn is_pow2(v: u32) -> bool {
    v != 0 && (v & (v - 1)) == 0
}

/// 上取整对齐。
pub fn align_up(v: u32, a: u32) -> Result<u32, u32> {
    if !is_pow2(a) {
        return Err(SP_EALIGN);
    }
    let mask = a - 1;
    match v.checked_add(mask) {
        Some(x) => Ok(x & !mask),
        None => Err(SP_ESIZE),
    }
}

/// 一段区间：起始偏移与长度。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Region {
    pub off: u32,
    pub size: u32,
}

impl Region {
    /// 构造。
    pub const fn make(off: u32, size: u32) -> Region {
        Region { off, size }
    }
}

/// surplus 分配器。
#[derive(Clone, Copy, Debug)]
pub struct Surplus {
    cap: u32,
    top: u32,
    holes: [Region; HOLE_CAP],
    nholes: usize,
    live: [Region; LIVE_CAP],
    nlive: usize,
    wasted: u32,
    releases: u32,
}

impl Surplus {
    /// 新建。
    pub const fn new(cap: u32) -> Surplus {
        Surplus {
            cap,
            top: 0,
            holes: [Region::make(0, 0); HOLE_CAP],
            nholes: 0,
            live: [Region::make(0, 0); LIVE_CAP],
            nlive: 0,
            wasted: 0,
            releases: 0,
        }
    }

    /// 分配一段，返回起始偏移。
    ///
    /// 顺序：已对齐空洞 → 需填充空洞 → 顶沿推进。
    pub fn reserve(&mut self, size: u32, align: u32) -> Result<u32, u32> {
        if size == 0 {
            return Err(SP_ESIZE);
        }
        if !is_pow2(align) {
            return Err(SP_EALIGN);
        }
        if self.nlive >= LIVE_CAP {
            return Err(SP_EFULL);
        }
        let mut i = 0;
        while i < self.nholes {
            match self.holes.get(i).copied() {
                Some(h) => {
                    if h.off % align == 0 && h.size >= size {
                        return self.take_hole(i, size, 0);
                    }
                }
                None => return Err(SP_ENOBS),
            }
            i += 1;
        }
        let mut j = 0;
        while j < self.nholes {
            let h = match self.holes.get(j).copied() {
                Some(v) => v,
                None => return Err(SP_ENOBS),
            };
            let start = align_up(h.off, align)?;
            let pad = start - h.off;
            let need = match pad.checked_add(size) {
                Some(v) => v,
                None => return Err(SP_ESIZE),
            };
            if h.size >= need {
                return self.take_hole(j, size, pad);
            }
            j += 1;
        }
        let start = align_up(self.top, align)?;
        let end = match start.checked_add(size) {
            Some(v) => v,
            None => return Err(SP_ESIZE),
        };
        if end > self.cap {
            return Err(SP_EFULL);
        }
        self.wasted += start - self.top;
        self.top = end;
        match self.live.get_mut(self.nlive) {
            Some(s) => *s = Region::make(start, size),
            None => return Err(SP_EFULL),
        }
        self.nlive += 1;
        Ok(start)
    }

    fn take_hole(&mut self, idx: usize, size: u32, pad: u32) -> Result<u32, u32> {
        let h = match self.holes.get(idx).copied() {
            Some(v) => v,
            None => return Err(SP_ENOBS),
        };
        let start = match h.off.checked_add(pad) {
            Some(v) => v,
            None => return Err(SP_ESIZE),
        };
        let new_off = match start.checked_add(size) {
            Some(v) => v,
            None => return Err(SP_ESIZE),
        };
        let hole_end = match h.off.checked_add(h.size) {
            Some(v) => v,
            None => return Err(SP_ESIZE),
        };
        self.wasted += pad;
        if new_off == hole_end {
            let mut k = idx;
            while k + 1 < self.nholes {
                match (self.holes.get(k + 1).copied(), self.holes.get_mut(k)) {
                    (Some(nx), Some(dst)) => *dst = nx,
                    _ => return Err(SP_ENOBS),
                }
                k += 1;
            }
            self.nholes -= 1;
        } else {
            match self.holes.get_mut(idx) {
                Some(s) => *s = Region::make(new_off, hole_end - new_off),
                None => return Err(SP_ENOBS),
            }
        }
        match self.live.get_mut(self.nlive) {
            Some(s) => *s = Region::make(start, size),
            None => return Err(SP_EFULL),
        }
        self.nlive += 1;
        Ok(start)
    }

    /// 释放起始偏移所在的活跃块，返回其偏移。
    pub fn release(&mut self, off: u32) -> Result<u32, u32> {
        let mut idx: Option<usize> = None;
        let mut i = 0;
        while i < self.nlive {
            match self.live.get(i).copied() {
                Some(r) => {
                    if r.off == off {
                        idx = Some(i);
                    }
                }
                None => return Err(SP_ENOBS),
            }
            i += 1;
        }
        let at = match idx {
            Some(v) => v,
            None => return Err(SP_ENOBLK),
        };
        if self.nholes >= HOLE_CAP {
            return Err(SP_EHOLE);
        }
        let reg = match self.live.get(at).copied() {
            Some(v) => v,
            None => return Err(SP_ENOBS),
        };
        let last = self.nlive - 1;
        if at != last {
            match self.live.get(last).copied() {
                Some(v) => match self.live.get_mut(at) {
                    Some(dst) => *dst = v,
                    None => return Err(SP_ENOBS),
                },
                None => return Err(SP_ENOBS),
            }
        }
        self.nlive -= 1;
        match self.holes.get_mut(self.nholes) {
            Some(s) => *s = reg,
            None => return Err(SP_EHOLE),
        }
        self.nholes += 1;
        self.releases += 1;
        Ok(off)
    }

    /// 活跃字节数。
    pub fn used(&self) -> u32 {
        let mut n = 0u32;
        let mut i = 0;
        while i < self.nlive {
            if let Some(r) = self.live.get(i) {
                n += r.size;
            }
            i += 1;
        }
        n
    }

    /// 空洞字节数。
    pub fn free(&self) -> u32 {
        let mut n = 0u32;
        let mut i = 0;
        while i < self.nholes {
            if let Some(r) = self.holes.get(i) {
                n += r.size;
            }
            i += 1;
        }
        n
    }

    /// 对齐沉没字节数。
    pub fn wasted(&self) -> u32 {
        self.wasted
    }

    /// 顶沿。
    pub fn top(&self) -> u32 {
        self.top
    }

    /// 预算。
    pub fn cap(&self) -> u32 {
        self.cap
    }

    /// 活跃块数。
    pub fn nlive(&self) -> usize {
        self.nlive
    }

    /// 空洞数。
    pub fn nholes(&self) -> usize {
        self.nholes
    }

    /// 取活跃块。
    pub fn live(&self, i: usize) -> Option<Region> {
        if i >= self.nlive {
            return None;
        }
        self.live.get(i).copied()
    }

    /// 取空洞。
    pub fn hole(&self, i: usize) -> Option<Region> {
        if i >= self.nholes {
            return None;
        }
        self.holes.get(i).copied()
    }

    /// 成功释放次数。
    pub fn releases(&self) -> u32 {
        self.releases
    }

    /// 三元恒等式：活跃 + 空洞 + 沉没 == 顶沿。
    pub fn balanced(&self) -> bool {
        match self.used().checked_add(self.free()) {
            Some(v) => match v.checked_add(self.wasted) {
                Some(t) => t == self.top,
                None => false,
            },
            None => false,
        }
    }
}

/// 一个账本样本。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Sample {
    pub slot: u8,
    pub op: u8,
    pub ok: bool,
    pub code: u32,
    pub off: u32,
    pub size: u32,
    pub top: u32,
    pub used: u32,
    pub free: u32,
    pub wasted: u32,
}

impl Sample {
    /// 空白格。
    pub const fn blank() -> Sample {
        Sample {
            slot: 0,
            op: 0,
            ok: false,
            code: 0,
            off: 0,
            size: 0,
            top: 0,
            used: 0,
            free: 0,
            wasted: 0,
        }
    }

    /// 三元恒等式。
    pub fn balanced(&self) -> bool {
        match self.used.checked_add(self.free) {
            Some(v) => match v.checked_add(self.wasted) {
                Some(t) => t == self.top,
                None => false,
            },
            None => false,
        }
    }
}

/// FNV 折入。
pub fn fold(h: u64, v: u64) -> u64 {
    let mut x = h;
    let mut n = v;
    let mut i = 0;
    while i < 8 {
        x ^= n & 0xff;
        x = x.wrapping_mul(FNV_PRIME);
        n >>= 8;
        i += 1;
    }
    x
}

/// 样本账本。
#[derive(Clone, Copy, Debug)]
pub struct Ledger {
    rows: [Sample; SAMPLES],
    len: usize,
    seal: u64,
    frozen: bool,
    rejects: u32,
}

impl Ledger {
    /// 空账本。
    pub const fn new() -> Ledger {
        Ledger {
            rows: [Sample::blank(); SAMPLES],
            len: 0,
            seal: 0,
            frozen: false,
            rejects: 0,
        }
    }

    /// 登记一格：结果、尺寸与登记后的三元组状态。
    pub fn register(
        &mut self,
        slot: u8,
        op: u8,
        size: u32,
        r: Result<u32, u32>,
        s: &Surplus,
    ) -> Result<(), u32> {
        if self.frozen {
            return Err(SP_EFROZEN);
        }
        if (slot as usize) != self.len {
            return Err(SP_EORDER);
        }
        if self.len >= SAMPLES {
            return Err(SP_EID);
        }
        let mut i = 0;
        while i < self.len {
            match self.rows.get(i) {
                Some(x) => {
                    if x.slot == slot {
                        return Err(SP_EDUP);
                    }
                }
                None => return Err(SP_ENOBS),
            }
            i += 1;
        }
        let row = match r {
            Ok(off) => {
                if off >= s.top() {
                    return Err(SP_EMISMATCH);
                }
                Sample {
                    slot,
                    op,
                    ok: true,
                    code: V_OK,
                    off,
                    size,
                    top: s.top(),
                    used: s.used(),
                    free: s.free(),
                    wasted: s.wasted(),
                }
            }
            Err(c) => {
                if c == V_OK {
                    return Err(SP_EMISMATCH);
                }
                Sample {
                    slot,
                    op,
                    ok: false,
                    code: c,
                    off: 0,
                    size,
                    top: s.top(),
                    used: s.used(),
                    free: s.free(),
                    wasted: s.wasted(),
                }
            }
        };
        if !row.balanced() {
            return Err(SP_EBAL);
        }
        match self.rows.get_mut(self.len) {
            Some(x) => *x = row,
            None => return Err(SP_EID),
        }
        self.len += 1;
        Ok(())
    }

    /// 格数。
    pub fn len(&self) -> usize {
        self.len
    }

    /// 是否为空。
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// 取样本。
    pub fn sample(&self, i: usize) -> Option<Sample> {
        if i >= self.len {
            return None;
        }
        self.rows.get(i).copied()
    }

    /// 成功计数。
    pub fn ok_count(&self) -> usize {
        let mut n = 0;
        let mut i = 0;
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

    /// 拒绝计数。
    pub fn err_count(&self) -> usize {
        self.len - self.ok_count()
    }

    /// 对账。
    pub fn audit(&self) -> Result<(), u32> {
        if self.len != SAMPLES {
            return Err(SP_EBAL);
        }
        if self.ok_count() != 9 || self.err_count() != 5 {
            return Err(SP_EBAL);
        }
        let mut i = 0;
        while i < self.len {
            match self.rows.get(i) {
                Some(s) => {
                    if s.slot as usize != i {
                        return Err(SP_EORDER);
                    }
                    if s.ok {
                        if s.code != V_OK || !s.balanced() {
                            return Err(SP_EMISMATCH);
                        }
                    } else if s.code == V_OK || !s.balanced() || s.off != 0 {
                        return Err(SP_EMISMATCH);
                    }
                }
                None => return Err(SP_ENOBS),
            }
            i += 1;
        }
        let want: [(u32, u32, u32, u32, u32); SAMPLES] = [
            (0, 100, 100, 0, 0),
            (112, 162, 150, 0, 12),
            (164, 194, 180, 0, 14),
            (112, 194, 130, 50, 14),
            (112, 194, 170, 10, 14),
            (152, 194, 180, 0, 14),
            (224, 424, 380, 0, 44),
            (0, 424, 380, 0, 44),
            (0, 424, 380, 0, 44),
            (0, 424, 380, 0, 44),
            (0, 424, 380, 0, 44),
            (0, 424, 380, 0, 44),
            (164, 424, 350, 30, 44),
            (168, 424, 374, 2, 48),
        ];
        let mut k = 0;
        while k < self.len {
            match (self.rows.get(k), want.get(k)) {
                (Some(s), Some(w)) => {
                    if s.off != w.0 || s.top != w.1 || s.used != w.2 || s.free != w.3 || s.wasted != w.4 {
                        return Err(SP_EMISMATCH);
                    }
                }
                _ => return Err(SP_ENOBS),
            }
            k += 1;
        }
        let codes = [SP_EFULL, SP_ESIZE, SP_EALIGN, SP_EALIGN, SP_ENOBLK];
        let mut c = 0;
        let mut j = 0;
        while j < self.len {
            if let Some(s) = self.rows.get(j) {
                if !s.ok {
                    match codes.get(c) {
                        Some(w) => {
                            if s.code != *w {
                                return Err(SP_EMISMATCH);
                            }
                        }
                        None => return Err(SP_EBAL),
                    }
                    c += 1;
                }
            }
            j += 1;
        }
        if c != codes.len() {
            return Err(SP_EBAL);
        }
        if self.sample(3).map(|x| x.op) != Some(OP_RELEASE) {
            return Err(SP_EMISMATCH);
        }
        if self.sample(12).map(|x| x.op) != Some(OP_RELEASE) {
            return Err(SP_EMISMATCH);
        }
        Ok(())
    }

    /// 熔断票号。
    pub fn seal(&self) -> u64 {
        let mut h = FNV_OFF;
        let mut i = 0;
        while i < self.len {
            if let Some(s) = self.rows.get(i) {
                h = fold(h, s.slot as u64);
                h = fold(h, s.op as u64);
                h = fold(h, s.ok as u64);
                h = fold(h, s.code as u64);
                h = fold(h, s.off as u64);
                h = fold(h, s.size as u64);
                h = fold(h, s.top as u64);
                h = fold(h, s.used as u64);
                h = fold(h, s.free as u64);
                h = fold(h, s.wasted as u64);
            }
            i += 1;
        }
        h
    }

    /// 冻结。
    pub fn freeze(&mut self) -> Result<u64, u32> {
        if self.frozen {
            self.rejects += 1;
            return Err(SP_EFROZEN);
        }
        let s = self.seal();
        self.seal = s;
        self.frozen = true;
        Ok(s)
    }

    /// 重放。
    pub fn replay(&self) -> Result<u64, u32> {
        let fresh = build_ledger();
        if fresh.len != self.len {
            return Err(SP_EBAL);
        }
        let h = fresh.seal();
        if self.frozen && h != self.seal {
            return Err(SP_ESEAL);
        }
        Ok(h)
    }

    /// 封存票号。
    pub fn root(&self) -> u64 {
        self.seal
    }

    /// 二次冻结次数。
    pub fn rejects(&self) -> u32 {
        self.rejects
    }

    /// 是否已冻结。
    pub fn is_frozen(&self) -> bool {
        self.frozen
    }
}

/// 构建十四格账本。
pub fn build_ledger() -> Ledger {
    let mut l = Ledger::new();
    let mut s = Surplus::new(SURPLUS_CAP);

    let r0 = s.reserve(100, 8);
    let _ = l.register(0, OP_RESERVE, 100, r0, &s);
    let r1 = s.reserve(50, 16);
    let _ = l.register(1, OP_RESERVE, 50, r1, &s);
    let r2 = s.reserve(30, 4);
    let _ = l.register(2, OP_RESERVE, 30, r2, &s);

    let f3 = s.release(112);
    let _ = l.register(3, OP_RELEASE, 50, f3, &s);

    let r4 = s.reserve(40, 8);
    let _ = l.register(4, OP_RESERVE, 40, r4, &s);
    let r5 = s.reserve(10, 4);
    let _ = l.register(5, OP_RESERVE, 10, r5, &s);
    let r6 = s.reserve(200, 32);
    let _ = l.register(6, OP_RESERVE, 200, r6, &s);

    let r7 = s.reserve(100, 64);
    let _ = l.register(7, OP_RESERVE, 100, r7, &s);
    let r8 = s.reserve(0, 8);
    let _ = l.register(8, OP_RESERVE, 0, r8, &s);
    let r9 = s.reserve(16, 12);
    let _ = l.register(9, OP_RESERVE, 16, r9, &s);
    let r10 = s.reserve(16, 0);
    let _ = l.register(10, OP_RESERVE, 16, r10, &s);
    let r11 = s.release(999);
    let _ = l.register(11, OP_RELEASE, 0, r11, &s);

    let f12 = s.release(164);
    let _ = l.register(12, OP_RELEASE, 30, f12, &s);
    let r13 = s.reserve(24, 8);
    let _ = l.register(13, OP_RESERVE, 24, r13, &s);

    l
}

/// 四路注入：账本重放、首次适配、记账恒等式、拒绝零副作用。
pub fn inject_four() -> [bool; 4] {
    let mut out = [false; 4];

    let l = build_ledger();
    let base = l.len() == SAMPLES && l.audit().is_ok();
    let rep = l.replay();
    let root = l.seal();
    out[0] = base && rep == Ok(root) && root != 0;

    let mut s = Surplus::new(SURPLUS_CAP);
    let a = s.reserve(100, 8);
    let b = s.reserve(50, 16);
    let f = s.release(112);
    let c = s.reserve(40, 8);
    out[1] = a == Ok(0)
        && b == Ok(112)
        && f == Ok(112)
        && c == Ok(112)
        && s.top() == 162
        && s.wasted() == 12
        && s.free() == 10
        && s.used() == 140
        && s.balanced();

    let mut t = Surplus::new(SURPLUS_CAP);
    let mut ok = t.balanced();
    ok = ok && t.reserve(448, 32) == Ok(0) && t.balanced();
    ok = ok && t.reserve(64, 64) == Ok(448) && t.balanced();
    ok = ok && t.top() == SURPLUS_CAP && t.wasted() == 0;
    ok = ok && t.reserve(1, 1) == Err(SP_EFULL) && t.balanced();
    out[2] = ok;

    let mut d = Surplus::new(SURPLUS_CAP);
    let before = (d.top(), d.used(), d.free(), d.wasted(), d.nlive(), d.nholes());
    let z = d.reserve(0, 8);
    let m = d.reserve(16, 12);
    let u = d.release(0);
    let big = d.reserve(600, 8);
    out[3] = z == Err(SP_ESIZE)
        && m == Err(SP_EALIGN)
        && u == Err(SP_ENOBLK)
        && big == Err(SP_EFULL)
        && (d.top(), d.used(), d.free(), d.wasted(), d.nlive(), d.nholes()) == before
        && d.releases() == 0;

    out
}

/// 十三项判据。
pub fn run_unx11307_checks() -> CheckSet {
    let mut s = CheckSet::new("unx11307.surplus");
    let l = build_ledger();

    s.add(
        "SP1-十四格齐备",
        l.len() == SAMPLES && !l.is_empty() && l.sample(0).map(|x| x.op) == Some(OP_RESERVE),
        "static",
    );

    s.add_dyn(
        "SP2-九允五拒分布正确",
        l.ok_count() == 9 && l.err_count() == 5 && l.audit().is_ok(),
        &format!("ok={} err={}", l.ok_count(), l.err_count()),
    );

    s.add(
        "SP3-预算恒等式逐格成立",
        {
            let mut ok = true;
            let mut i = 0;
            while i < l.len() {
                if l.sample(i).map(|x| x.balanced()) != Some(true) {
                    ok = false;
                }
                i += 1;
            }
            ok && l.sample(13).map(|x| x.top) == Some(424)
        },
        "static",
    );

    s.add(
        "SP4-首次适配复用空洞",
        l.sample(3).map(|x| x.free) == Some(50)
            && l.sample(4).map(|x| x.off) == Some(112)
            && l.sample(4).map(|x| x.free) == Some(10)
            && l.sample(5).map(|x| x.off) == Some(152)
            && l.sample(5).map(|x| x.free) == Some(0),
        "static",
    );

    s.add(
        "SP5-相邻空洞不合并",
        {
            let mut f = Surplus::new(SURPLUS_CAP);
            let _ = f.reserve(100, 8);
            let _ = f.reserve(50, 8);
            let _ = f.release(0);
            let _ = f.release(104);
            let r = f.reserve(120, 8);
            r == Ok(160) && f.nholes() == 2 && f.wasted() == 10
        },
        "static",
    );

    s.add(
        "SP6-对齐填充沉没记账",
        l.sample(1).map(|x| x.wasted) == Some(12)
            && l.sample(2).map(|x| x.wasted) == Some(14)
            && l.sample(6).map(|x| x.wasted) == Some(44)
            && l.sample(13).map(|x| x.wasted) == Some(48),
        "static",
    );

    s.add(
        "SP7-预算耗尽拒绝",
        l.sample(7).map(|x| x.code) == Some(SP_EFULL)
            && {
                let mut f = Surplus::new(SURPLUS_CAP);
                let _ = f.reserve(448, 32);
                f.reserve(65, 64) == Err(SP_EFULL) && f.top() == 448
            },
        "static",
    );

    s.add(
        "SP8-尺寸为零拒绝",
        l.sample(8).map(|x| x.code) == Some(SP_ESIZE)
            && Surplus::new(SURPLUS_CAP).reserve(0, 1) == Err(SP_ESIZE),
        "static",
    );

    s.add(
        "SP9-对齐非法拒绝",
        l.sample(9).map(|x| x.code) == Some(SP_EALIGN)
            && l.sample(10).map(|x| x.code) == Some(SP_EALIGN)
            && Surplus::new(SURPLUS_CAP).reserve(8, 6) == Err(SP_EALIGN),
        "static",
    );

    s.add(
        "SP10-释放未知块拒绝",
        l.sample(11).map(|x| x.code) == Some(SP_ENOBLK)
            && {
                let mut f = Surplus::new(SURPLUS_CAP);
                let _ = f.reserve(32, 8);
                f.release(8) == Err(SP_ENOBLK) && f.releases() == 0 && f.balanced()
            },
        "static",
    );

    s.add(
        "SP11-重复释放拒绝",
        {
            let mut f = Surplus::new(SURPLUS_CAP);
            let _ = f.reserve(32, 8);
            let first = f.release(0);
            let second = f.release(0);
            first == Ok(0) && second == Err(SP_ENOBLK) && f.releases() == 1 && f.balanced() && f.free() == 32
        },
        "static",
    );

    s.add(
        "SP12-满容量精确落位",
        {
            let mut f = Surplus::new(SURPLUS_CAP);
            f.reserve(448, 32) == Ok(0)
                && f.reserve(64, 64) == Ok(448)
                && f.top() == SURPLUS_CAP
                && f.balanced()
                && f.nholes() == 0
        },
        "static",
    );

    let mut g = build_ledger();
    let rep = g.replay();
    let root = match g.freeze() {
        Ok(v) => v,
        Err(_) => 0,
    };
    let again = g.freeze() == Err(SP_EFROZEN);
    let inj = inject_four();
    s.add_dyn(
        "SP13-冻结重放与四路注入",
        root != 0 && rep == Ok(root) && again && g.root() == root && g.rejects() == 1 && inj == [true; 4],
        &format!("root={:016x} inject={:?}", root, inj),
    );

    s
}

/// 摘要。
pub fn summary(l: &Ledger) -> alloc::string::String {
    format!("rows={} ok={} err={}", l.len(), l.ok_count(), l.err_count())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bump_advances_with_alignment_padding() {
        let mut s = Surplus::new(SURPLUS_CAP);
        assert_eq!(s.reserve(100, 8), Ok(0));
        assert_eq!(s.reserve(50, 16), Ok(112));
        assert_eq!(s.wasted(), 12);
        assert_eq!(s.top(), 162);
        assert_eq!(s.used(), 150);
        assert!(s.balanced());
    }

    #[test]
    fn first_fit_reuses_hole() {
        let mut s = Surplus::new(SURPLUS_CAP);
        let _ = s.reserve(100, 8);
        let _ = s.reserve(50, 16);
        assert_eq!(s.release(112), Ok(112));
        assert_eq!(s.free(), 50);
        assert_eq!(s.reserve(40, 8), Ok(112));
        assert_eq!(s.free(), 10);
        assert_eq!(s.hole(0), Some(Region::make(152, 10)));
        assert!(s.balanced());
    }

    #[test]
    fn adjacent_holes_do_not_coalesce() {
        let mut s = Surplus::new(SURPLUS_CAP);
        let _ = s.reserve(100, 8);
        let _ = s.reserve(50, 8);
        let _ = s.release(0);
        let _ = s.release(104);
        assert_eq!(s.nholes(), 2);
        assert_eq!(s.reserve(120, 8), Ok(160));
        assert_eq!(s.nholes(), 2);
        assert_eq!(s.wasted(), 10);
        assert!(s.balanced());
    }

    #[test]
    fn exact_budget_fit_then_refusal() {
        let mut s = Surplus::new(SURPLUS_CAP);
        assert_eq!(s.reserve(448, 32), Ok(0));
        assert_eq!(s.reserve(64, 64), Ok(448));
        assert_eq!(s.top(), SURPLUS_CAP);
        assert_eq!(s.reserve(1, 1), Err(SP_EFULL));
        assert!(s.balanced());
    }

    #[test]
    fn zero_size_and_bad_align_are_refused() {
        let mut s = Surplus::new(SURPLUS_CAP);
        assert_eq!(s.reserve(0, 8), Err(SP_ESIZE));
        assert_eq!(s.reserve(8, 12), Err(SP_EALIGN));
        assert_eq!(s.reserve(8, 0), Err(SP_EALIGN));
        assert_eq!(s.top(), 0);
        assert_eq!(s.nlive(), 0);
    }

    #[test]
    fn double_release_is_refused() {
        let mut s = Surplus::new(SURPLUS_CAP);
        let _ = s.reserve(32, 8);
        assert_eq!(s.release(0), Ok(0));
        assert_eq!(s.release(0), Err(SP_ENOBLK));
        assert_eq!(s.releases(), 1);
        assert_eq!(s.free(), 32);
        assert!(s.balanced());
    }

    #[test]
    fn unknown_offset_is_refused() {
        let mut s = Surplus::new(SURPLUS_CAP);
        let _ = s.reserve(32, 8);
        assert_eq!(s.release(4), Err(SP_ENOBLK));
        assert_eq!(s.release(999), Err(SP_ENOBLK));
        assert_eq!(s.nholes(), 0);
        assert!(s.balanced());
    }

    #[test]
    fn ledger_seals_and_replays() {
        let mut l = build_ledger();
        assert!(l.audit().is_ok());
        let root = l.freeze().expect("freeze");
        assert_eq!(l.replay(), Ok(root));
        assert_eq!(l.freeze(), Err(SP_EFROZEN));
        assert_eq!(l.rejects(), 1);
    }

    #[test]
    fn checks_pass() {
        let s = run_unx11307_checks();
        assert_eq!(s.len(), 13);
        assert!(s.all_passed(), "{}", s.red_items_text());
    }
}
