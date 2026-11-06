//! UNX-F11311 线程创建 TLS 块分配（0xe960 段）
//!
//! `pthread_create` 为每个新线程准备一段线程局部存储：先按镜像的
//! `tdata + tbss` 加 TCB 头长度向池里申请块，再把 `tdata` 拷进块尾、
//! 把 `tbss` 清零，最后把 TP 指向块首加 TCB 头。三步必须原子成立——
//! 拷镜像失败要回滚块，设 TP 失败也要回滚，否则池会被半成品永久占住。
//!
//! 池有两条供给线：空闲链表与游标。空闲链表优先，且只接受尺寸完全一致
//! 的块，这模拟 glibc 对同规格线程块的复用；游标向上对齐到镜像要求的
//! 边界，越过容量即 `ENOMEM`。错误码在分配失败时必须原样透传给调用者，
//! 不能被后续步骤覆盖——这正是 `ENOMEM` 透传判据要守的性质。
//!
//! 账本十四格：八格成功覆盖六次连续申请、一次释放后复用与一次独立池；
//! 六格拒绝分别命中池满、池容量不足、镜像超长、对齐非法、TP 失配与
//! 尺寸为零。成功样本记录三步完成数与 TP，拒绝样本记录失败所在步数，
//! 使回滚路径在账本里可数。

use crate::alloc::format;
use crate::checks::CheckSet;

/// 段基址。
pub const SEG_BASE: u32 = 0xe960;

/// 编号非法。
pub const TH_EID: u32 = SEG_BASE + 0x01;
/// 登记顺序错误。
pub const TH_EORDER: u32 = SEG_BASE + 0x02;
/// 重复登记。
pub const TH_EDUP: u32 = SEG_BASE + 0x03;
/// 已冻结。
pub const TH_EFROZEN: u32 = SEG_BASE + 0x04;
/// 观测不存在的格。
pub const TH_ENOBS: u32 = SEG_BASE + 0x05;
/// 结论不符。
pub const TH_EMISMATCH: u32 = SEG_BASE + 0x06;
/// 账本失衡。
pub const TH_EBAL: u32 = SEG_BASE + 0x07;
/// 池容量不足。
pub const TH_ENOMEM: u32 = SEG_BASE + 0x08;
/// 镜像超过块尾。
pub const TH_EIMG: u32 = SEG_BASE + 0x09;
/// 对齐非法。
pub const TH_EALIGN: u32 = SEG_BASE + 0x0a;
/// TP 失配。
pub const TH_ETP: u32 = SEG_BASE + 0x0b;
/// 尺寸为零。
pub const TH_ESIZE: u32 = SEG_BASE + 0x0c;
/// 释放未持有的块。
pub const TH_EFREE: u32 = SEG_BASE + 0x0d;
/// 票号不一致。
pub const TH_ESEAL: u32 = SEG_BASE + 0x0e;

/// 判决：成功。
pub const V_OK: u32 = 0;

/// 操作：整体创建。
pub const OP_SPAWN: u8 = 1;
/// 操作：仅分配。
pub const OP_ALLOC: u8 = 2;
/// 操作：仅拷镜像。
pub const OP_COPY: u8 = 3;
/// 操作：仅设 TP。
pub const OP_TP: u8 = 4;

/// TCB 头长度。
pub const TCB: u32 = 48;
/// 池块槽数。
pub const POOL: usize = 6;
/// 线程池容量（字节）。
pub const ARENA_CAP: usize = 1024;
/// 镜像数据容量。
pub const IMG_CAP: usize = 64;
/// 毒化填充字节。
pub const POISON: u8 = 0xa5;
/// 样本格数。
pub const SAMPLES: usize = 14;

/// FNV 偏移基。
pub const FNV_OFF: u64 = 0xcbf2_9ce4_8422_2325;
/// FNV 质数。
pub const FNV_PRIME: u64 = 0x100_0000_01b3;

/// TLS 初始化镜像。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Image {
    pub tdata: [u8; IMG_CAP],
    pub tlen: u32,
    pub tbss: u32,
    pub align: u32,
}

impl Image {
    /// 由字节切片构造，超长直接截断到零长度由调用方察觉。
    pub fn make(bytes: &[u8], tbss: u32, align: u32) -> Image {
        let mut tdata = [0u8; IMG_CAP];
        let mut i = 0;
        while i < bytes.len() && i < IMG_CAP {
            match (tdata.get_mut(i), bytes.get(i)) {
                (Some(d), Some(s)) => *d = *s,
                _ => break,
            }
            i += 1;
        }
        Image {
            tdata,
            tlen: i as u32,
            tbss,
            align,
        }
    }

    /// 块总长：头 + 数据 + 零区。
    pub fn block_size(&self) -> Option<u32> {
        TCB.checked_add(self.tlen).and_then(|v| v.checked_add(self.tbss))
    }
}

/// 池内一块。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Block {
    pub base: u32,
    pub size: u32,
    pub live: bool,
}

/// 线程块池。
#[derive(Clone, Copy, Debug)]
pub struct Pool {
    cursor: u32,
    blocks: [Block; POOL],
    n: usize,
    frees: [usize; POOL],
    nfree: usize,
}

impl Pool {
    /// 空池。
    pub const fn new() -> Pool {
        Pool {
            cursor: 0,
            blocks: [Block {
                base: 0,
                size: 0,
                live: false,
            }; POOL],
            n: 0,
            frees: [0; POOL],
            nfree: 0,
        }
    }

    /// 向上对齐，越界返回 None。
    fn align_up(v: u32, a: u32) -> Option<u32> {
        if a == 0 {
            return None;
        }
        let r = v % a;
        if r == 0 {
            Some(v)
        } else {
            v.checked_add(a - r)
        }
    }

    /// 申请一块：尺寸 → 对齐 → 空闲链 → 游标。
    pub fn alloc(&mut self, size: u32, align: u32) -> Result<u32, u32> {
        if size == 0 {
            return Err(TH_ESIZE);
        }
        if align == 0 || !align.is_power_of_two() {
            return Err(TH_EALIGN);
        }
        let mut k = 0;
        while k < self.nfree {
            let idx = match self.frees.get(k) {
                Some(i) => *i,
                None => return Err(TH_ENOMEM),
            };
            let exact = match self.blocks.get(idx) {
                Some(b) => !b.live && b.size == size && b.base % align == 0,
                None => false,
            };
            if exact {
                let base = match self.blocks.get(idx) {
                    Some(b) => b.base,
                    None => return Err(TH_ENOMEM),
                };
                if let Some(b) = self.blocks.get_mut(idx) {
                    b.live = true;
                }
                self.nfree -= 1;
                let last = self.nfree;
                if let Some(src) = self.frees.get(last).copied() {
                    if let Some(dst) = self.frees.get_mut(k) {
                        *dst = src;
                    }
                }
                return Ok(base);
            }
            k += 1;
        }
        if self.n >= POOL {
            return Err(TH_ENOMEM);
        }
        let base = match Pool::align_up(self.cursor, align) {
            Some(v) => v,
            None => return Err(TH_ENOMEM),
        };
        let end = match base.checked_add(size) {
            Some(v) => v,
            None => return Err(TH_ENOMEM),
        };
        if (end as usize) > ARENA_CAP {
            return Err(TH_ENOMEM);
        }
        if let Some(slot) = self.blocks.get_mut(self.n) {
            *slot = Block {
                base,
                size,
                live: true,
            };
        } else {
            return Err(TH_ENOMEM);
        }
        self.n += 1;
        self.cursor = end;
        Ok(base)
    }

    /// 释放：必须命中活块，否则拒绝。
    pub fn release(&mut self, base: u32) -> Result<(), u32> {
        let mut i = 0;
        while i < self.n {
            let hit = match self.blocks.get(i) {
                Some(b) => b.live && b.base == base,
                None => false,
            };
            if hit {
                if let Some(b) = self.blocks.get_mut(i) {
                    b.live = false;
                }
                if self.nfree < POOL {
                    if let Some(s) = self.frees.get_mut(self.nfree) {
                        *s = i;
                    }
                    self.nfree += 1;
                }
                return Ok(());
            }
            i += 1;
        }
        Err(TH_EFREE)
    }

    /// 游标位置。
    pub fn cursor(&self) -> u32 {
        self.cursor
    }

    /// 活块数。
    pub fn live(&self) -> usize {
        let mut n = 0;
        let mut i = 0;
        while i < self.n {
            if let Some(b) = self.blocks.get(i) {
                if b.live {
                    n += 1;
                }
            }
            i += 1;
        }
        n
    }

    /// 空闲链长度。
    pub fn free_len(&self) -> usize {
        self.nfree
    }

    /// 第 i 块。
    pub fn block(&self, i: usize) -> Option<Block> {
        self.blocks.get(i).copied()
    }

    /// 块数。
    pub fn len(&self) -> usize {
        self.n
    }

    /// 是否为空。
    pub fn is_empty(&self) -> bool {
        self.n == 0
    }
}

/// 拷镜像：毒化块体，写数据，清零区。
pub fn copy_image(mem: &mut [u8; ARENA_CAP], img: &Image, block_size: u32) -> Result<(), u32> {
    if block_size < TCB {
        return Err(TH_EIMG);
    }
    let payload = block_size - TCB;
    if img.tlen > payload || img.tbss > payload - img.tlen {
        return Err(TH_EIMG);
    }
    let mut i = 0;
    while i < payload as usize {
        if let Some(b) = mem.get_mut(TCB as usize + i) {
            *b = POISON;
        }
        i += 1;
    }
    let mut d = 0;
    while d < img.tlen as usize {
        match (mem.get_mut(TCB as usize + d), img.tdata.get(d)) {
            (Some(b), Some(s)) => *b = *s,
            _ => return Err(TH_EIMG),
        }
        d += 1;
    }
    let mut z = 0;
    while z < img.tbss as usize {
        match mem.get_mut(TCB as usize + img.tlen as usize + z) {
            Some(b) => *b = 0,
            None => return Err(TH_EIMG),
        }
        z += 1;
    }
    Ok(())
}

/// 设 TP：块首加头长度，必须十六字节对齐且落在池内。
pub fn set_tp(base: u32, head: u32) -> Result<u32, u32> {
    let tp = match base.checked_add(head) {
        Some(v) => v,
        None => return Err(TH_ETP),
    };
    if tp % 16 != 0 || (tp as usize) >= ARENA_CAP {
        return Err(TH_ETP);
    }
    Ok(tp)
}

/// 一个已建线程。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Thread {
    pub base: u32,
    pub tp: u32,
    pub size: u32,
    pub steps: u8,
}

/// 三步创建：分配 → 拷镜像 → 设 TP，失败即回滚。
pub fn spawn(mem: &mut [u8; ARENA_CAP], pool: &mut Pool, img: &Image) -> Result<Thread, u32> {
    let size = match img.block_size() {
        Some(v) => v,
        None => return Err(TH_ENOMEM),
    };
    let base = pool.alloc(size, img.align)?;
    if let Err(c) = copy_image(mem, img, size) {
        let _ = pool.release(base);
        return Err(c);
    }
    let tp = match set_tp(base, TCB) {
        Ok(v) => v,
        Err(c) => {
            let _ = pool.release(base);
            return Err(c);
        }
    };
    Ok(Thread {
        base,
        tp,
        size,
        steps: 3,
    })
}

/// 一个账本样本。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Sample {
    pub slot: u8,
    pub op: u8,
    pub ok: bool,
    pub code: u32,
    pub steps: u8,
    pub tp: u32,
}

impl Sample {
    /// 空白格。
    pub const fn blank() -> Sample {
        Sample {
            slot: 0,
            op: 0,
            ok: false,
            code: 0,
            steps: 0,
            tp: 0,
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

    /// 登记一格。
    pub fn register(&mut self, row: Sample) -> Result<(), u32> {
        if self.frozen {
            return Err(TH_EFROZEN);
        }
        if (row.slot as usize) != self.len {
            return Err(TH_EORDER);
        }
        if self.len >= SAMPLES {
            return Err(TH_EID);
        }
        let mut i = 0;
        while i < self.len {
            match self.rows.get(i) {
                Some(x) => {
                    if x.slot == row.slot {
                        return Err(TH_EDUP);
                    }
                }
                None => return Err(TH_ENOBS),
            }
            i += 1;
        }
        match self.rows.get_mut(self.len) {
            Some(x) => *x = row,
            None => return Err(TH_EID),
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
            return Err(TH_EBAL);
        }
        if self.ok_count() != 8 || self.err_count() != 6 {
            return Err(TH_EBAL);
        }
        let mut i = 0;
        while i < self.len {
            match self.rows.get(i) {
                Some(s) => {
                    if s.slot as usize != i {
                        return Err(TH_EORDER);
                    }
                    if s.ok {
                        if s.code != V_OK || s.steps != 3 {
                            return Err(TH_EMISMATCH);
                        }
                    } else if s.code == V_OK || s.tp != 0 {
                        return Err(TH_EMISMATCH);
                    }
                }
                None => return Err(TH_ENOBS),
            }
            i += 1;
        }
        let tps = [48u32, 112, 192, 272, 352, 448, 192, 48];
        let mut k = 0;
        while k < tps.len() {
            match (self.sample(k), tps.get(k)) {
                (Some(s), Some(w)) => {
                    if !s.ok || s.tp != *w {
                        return Err(TH_EMISMATCH);
                    }
                }
                _ => return Err(TH_EBAL),
            }
            k += 1;
        }
        let steps = [0u8, 0, 1, 0, 2, 0];
        let mut c = 0;
        let mut j = 8;
        while j < self.len {
            if let Some(s) = self.rows.get(j) {
                match steps.get(c) {
                    Some(w) => {
                        if s.steps != *w {
                            return Err(TH_EMISMATCH);
                        }
                    }
                    None => return Err(TH_EBAL),
                }
                c += 1;
            }
            j += 1;
        }
        if c != steps.len() {
            return Err(TH_EBAL);
        }
        let codes = [
            TH_ENOMEM,
            TH_ENOMEM,
            TH_EIMG,
            TH_EALIGN,
            TH_ETP,
            TH_ESIZE,
        ];
        let mut m = 0;
        let mut d = 0;
        while m < self.len {
            if let Some(s) = self.rows.get(m) {
                if !s.ok {
                    match codes.get(d) {
                        Some(w) => {
                            if s.code != *w {
                                return Err(TH_EMISMATCH);
                            }
                        }
                        None => return Err(TH_EBAL),
                    }
                    d += 1;
                }
            }
            m += 1;
        }
        if d != codes.len() {
            return Err(TH_EBAL);
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
                h = fold(h, s.steps as u64);
                h = fold(h, s.tp as u64);
            }
            i += 1;
        }
        h
    }

    /// 冻结。
    pub fn freeze(&mut self) -> Result<u64, u32> {
        if self.frozen {
            self.rejects += 1;
            return Err(TH_EFROZEN);
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
            return Err(TH_EBAL);
        }
        let h = fresh.seal();
        if self.frozen && h != self.seal {
            return Err(TH_ESEAL);
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

/// 六次连续申请使用的镜像。
pub fn six_images() -> [Image; 6] {
    [
        Image::make(&[1, 2, 3, 4], 12, 16),
        Image::make(&[5; 28], 4, 16),
        Image::make(&[6; 20], 12, 16),
        Image::make(&[7; 4], 28, 32),
        Image::make(&[8; 36], 12, 16),
        Image::make(&[9; 12], 4, 16),
    ]
}

/// 构建十四格账本。
pub fn build_ledger() -> Ledger {
    let mut l = Ledger::new();
    let mut mem = [0u8; ARENA_CAP];
    let mut pool = Pool::new();
    let imgs = six_images();

    let mut i = 0;
    while i < imgs.len() {
        let r = match imgs.get(i) {
            Some(img) => spawn(&mut mem, &mut pool, img),
            None => Err(TH_EIMG),
        };
        let row = match r {
            Ok(t) => Sample {
                slot: i as u8,
                op: OP_SPAWN,
                ok: true,
                code: V_OK,
                steps: t.steps,
                tp: t.tp,
            },
            Err(c) => Sample {
                slot: i as u8,
                op: OP_SPAWN,
                ok: false,
                code: c,
                steps: 0,
                tp: 0,
            },
        };
        let _ = l.register(row);
        i += 1;
    }

    let _ = pool.release(144);
    let reuse = match imgs.get(2) {
        Some(img) => spawn(&mut mem, &mut pool, img),
        None => Err(TH_EIMG),
    };
    let _ = l.register(match reuse {
        Ok(t) => Sample {
            slot: 6,
            op: OP_SPAWN,
            ok: true,
            code: V_OK,
            steps: t.steps,
            tp: t.tp,
        },
        Err(c) => Sample {
            slot: 6,
            op: OP_SPAWN,
            ok: false,
            code: c,
            steps: 0,
            tp: 0,
        },
    });

    let mut mem_b = [0u8; ARENA_CAP];
    let mut pool_b = Pool::new();
    let odd = Image::make(&[0xab; 4], 4, 64);
    let _ = l.register(match spawn(&mut mem_b, &mut pool_b, &odd) {
        Ok(t) => Sample {
            slot: 7,
            op: OP_SPAWN,
            ok: true,
            code: V_OK,
            steps: t.steps,
            tp: t.tp,
        },
        Err(c) => Sample {
            slot: 7,
            op: OP_SPAWN,
            ok: false,
            code: c,
            steps: 0,
            tp: 0,
        },
    });

    let full = match imgs.get(0) {
        Some(img) => spawn(&mut mem, &mut pool, img),
        None => Err(TH_EIMG),
    };
    let _ = l.register(Sample {
        slot: 8,
        op: OP_SPAWN,
        ok: full.is_ok(),
        code: match full {
            Ok(_) => V_OK,
            Err(c) => c,
        },
        steps: 0,
        tp: 0,
    });

    let over = pool.alloc(ARENA_CAP as u32 + 1, 16);
    let _ = l.register(Sample {
        slot: 9,
        op: OP_ALLOC,
        ok: over.is_ok(),
        code: match over {
            Ok(_) => V_OK,
            Err(c) => c,
        },
        steps: 0,
        tp: 0,
    });

    let long = Image::make(&[0u8; 40], 0, 16);
    let mut scratch = [0u8; ARENA_CAP];
    let short = copy_image(&mut scratch, &long, TCB + 16);
    let _ = l.register(Sample {
        slot: 10,
        op: OP_COPY,
        ok: short.is_ok(),
        code: match short {
            Ok(_) => V_OK,
            Err(c) => c,
        },
        steps: if short.is_err() { 1 } else { 0 },
        tp: 0,
    });

    let bad_align = pool.alloc(64, 3);
    let _ = l.register(Sample {
        slot: 11,
        op: OP_ALLOC,
        ok: bad_align.is_ok(),
        code: match bad_align {
            Ok(_) => V_OK,
            Err(c) => c,
        },
        steps: 0,
        tp: 0,
    });

    let bad_tp = set_tp(64, 40);
    let _ = l.register(Sample {
        slot: 12,
        op: OP_TP,
        ok: bad_tp.is_ok(),
        code: match bad_tp {
            Ok(_) => V_OK,
            Err(c) => c,
        },
        steps: if bad_tp.is_err() { 2 } else { 0 },
        tp: 0,
    });

    let zero = pool.alloc(0, 16);
    let _ = l.register(Sample {
        slot: 13,
        op: OP_ALLOC,
        ok: zero.is_ok(),
        code: match zero {
            Ok(_) => V_OK,
            Err(c) => c,
        },
        steps: 0,
        tp: 0,
    });

    l
}

/// 四路注入：账本重放、三步痕迹、镜像拷贝、闸门优先级。
pub fn inject_four() -> [bool; 4] {
    let mut out = [false; 4];

    let l = build_ledger();
    let base = l.len() == SAMPLES && l.audit().is_ok();
    let rep = l.replay();
    let root = l.seal();
    out[0] = base && rep == Ok(root) && root != 0;

    let mut mem = [0u8; ARENA_CAP];
    let mut pool = Pool::new();
    let mut steps_ok = true;
    let mut expect_tp = TCB;
    let imgs = six_images();
    let mut i = 0;
    while i < imgs.len() {
        let r = match imgs.get(i) {
            Some(img) => spawn(&mut mem, &mut pool, img),
            None => Err(TH_EIMG),
        };
        match r {
            Ok(t) => {
                if t.steps != 3 || t.tp != t.base + TCB || t.tp < expect_tp {
                    steps_ok = false;
                }
                expect_tp = t.tp;
            }
            Err(_) => steps_ok = false,
        }
        i += 1;
    }
    out[1] = steps_ok && pool.live() == 6 && pool.cursor() == 464;

    let mut mem2 = [0u8; ARENA_CAP];
    let mut pool2 = Pool::new();
    let probe = Image::make(&[0x11, 0x22, 0x33, 0x44], 8, 16);
    let built = spawn(&mut mem2, &mut pool2, &probe);
    let mut copy_ok = built.is_ok();
    if let Ok(t) = built {
        let mut d = 0;
        while d < 4 {
            match mem2.get(TCB as usize + d) {
                Some(b) => {
                    if *b != 0x11 * (d as u8 + 1) {
                        copy_ok = false;
                    }
                }
                None => copy_ok = false,
            }
            d += 1;
        }
        let mut z = 0;
        while z < 8 {
            match mem2.get(TCB as usize + 4 + z) {
                Some(b) => {
                    if *b != 0 {
                        copy_ok = false;
                    }
                }
                None => copy_ok = false,
            }
            z += 1;
        }
        if t.tp != TCB {
            copy_ok = false;
        }
    }
    out[2] = copy_ok;

    let mut p3 = Pool::new();
    let mut m3 = [0u8; ARENA_CAP];
    let small = Image::make(&[1, 2, 3, 4], 0, 16);
    let mut full_ok = true;
    let mut k = 0;
    while k < POOL {
        if spawn(&mut m3, &mut p3, &small).is_err() {
            full_ok = false;
        }
        k += 1;
    }
    out[3] = full_ok
        && spawn(&mut m3, &mut p3, &small) == Err(TH_ENOMEM)
        && p3.alloc(ARENA_CAP as u32, 16) == Err(TH_ENOMEM)
        && copy_image(&mut m3, &Image::make(&[0u8; 40], 0, 16), TCB + 8) == Err(TH_EIMG)
        && p3.alloc(8, 3) == Err(TH_EALIGN)
        && set_tp(64, 40) == Err(TH_ETP)
        && p3.alloc(0, 16) == Err(TH_ESIZE)
        && p3.release(1023) == Err(TH_EFREE);

    out
}

/// 十三项判据。
pub fn run_unx11311_checks() -> CheckSet {
    let mut s = CheckSet::new("unx11311.tlscreate");
    let l = build_ledger();

    s.add(
        "TH1-十四格齐备",
        l.len() == SAMPLES && !l.is_empty() && l.sample(0).map(|x| x.op) == Some(OP_SPAWN),
        "static",
    );

    s.add_dyn(
        "TH2-八允六拒分布正确",
        l.ok_count() == 8 && l.err_count() == 6 && l.audit().is_ok(),
        &format!("ok={} err={}", l.ok_count(), l.err_count()),
    );

    s.add(
        "TH3-TP 轨迹",
        {
            let want = [48u32, 112, 192, 272, 352, 448, 192, 48];
            let mut ok = true;
            let mut i = 0;
            while i < want.len() {
                match (l.sample(i), want.get(i)) {
                    (Some(x), Some(w)) => {
                        if x.tp != *w {
                            ok = false;
                        }
                    }
                    _ => ok = false,
                }
                i += 1;
            }
            ok
        },
        "static",
    );

    s.add(
        "TH4-三步痕迹",
        {
            let mut ok = true;
            let mut i = 0;
            while i < 8 {
                match l.sample(i) {
                    Some(x) => {
                        if !x.ok || x.steps != 3 {
                            ok = false;
                        }
                    }
                    None => ok = false,
                }
                i += 1;
            }
            ok && l.sample(12).map(|x| x.steps) == Some(2)
                && l.sample(10).map(|x| x.steps) == Some(1)
                && l.sample(8).map(|x| x.steps) == Some(0)
        },
        "static",
    );

    s.add(
        "TH5-释放后复用同址",
        l.sample(2).map(|x| x.tp) == l.sample(6).map(|x| x.tp)
            && l.sample(2).map(|x| x.tp) == Some(192),
        "static",
    );

    s.add(
        "TH6-池满透传 ENOMEM",
        l.sample(8).map(|x| x.code) == Some(TH_ENOMEM)
            && {
                let mut pool = Pool::new();
                let mut mem = [0u8; ARENA_CAP];
                let img = Image::make(&[0u8; 4], 0, 16);
                let mut i = 0;
                while i < POOL {
                    let _ = spawn(&mut mem, &mut pool, &img);
                    i += 1;
                }
                spawn(&mut mem, &mut pool, &img) == Err(TH_ENOMEM) && pool.live() == POOL
            },
        "static",
    );

    s.add(
        "TH7-池容量不足",
        l.sample(9).map(|x| x.code) == Some(TH_ENOMEM)
            && {
                let mut pool = Pool::new();
                pool.alloc(ARENA_CAP as u32, 16).is_ok() && pool.alloc(ARENA_CAP as u32 + 1, 16) == Err(TH_ENOMEM)
            },
        "static",
    );

    s.add(
        "TH8-镜像超长",
        l.sample(10).map(|x| x.code) == Some(TH_EIMG)
            && {
                let mut mem = [0u8; ARENA_CAP];
                let long = Image::make(&[7u8; 60], 8, 16);
                copy_image(&mut mem, &long, TCB + 32) == Err(TH_EIMG)
                    && copy_image(&mut mem, &long, TCB + 60) == Err(TH_EIMG)
                    && copy_image(&mut mem, &long, TCB + 68).is_ok()
            },
        "static",
    );

    s.add(
        "TH9-对齐闸门",
        l.sample(11).map(|x| x.code) == Some(TH_EALIGN)
            && {
                let mut pool = Pool::new();
                pool.alloc(32, 0) == Err(TH_EALIGN)
                    && pool.alloc(32, 6) == Err(TH_EALIGN)
                    && pool.alloc(32, 64).is_ok()
            },
        "static",
    );

    s.add(
        "TH10-TP 闸门",
        l.sample(12).map(|x| x.code) == Some(TH_ETP)
            && set_tp(64, 40) == Err(TH_ETP)
            && set_tp(72, 32) == Err(TH_ETP)
            && set_tp(1024 - TCB, TCB) == Err(TH_ETP)
            && set_tp(64, TCB) == Ok(112),
        "static",
    );

    s.add(
        "TH11-尺寸零闸门",
        l.sample(13).map(|x| x.code) == Some(TH_ESIZE)
            && {
                let mut pool = Pool::new();
                pool.alloc(0, 16) == Err(TH_ESIZE) && pool.is_empty()
            },
        "static",
    );

    s.add(
        "TH12-镜像拷贝与毒化",
        {
            let mut pool = Pool::new();
            let mut mem = [0u8; ARENA_CAP];
            let img = Image::make(&[1, 2, 3, 4, 5, 6], 2, 16);
            let mut ok = true;
            if let Ok(t) = spawn(&mut mem, &mut pool, &img) {
                let mut i = 0;
                while i < 6 {
                    if mem.get(TCB as usize + i) != Some(&(i as u8 + 1)) {
                        ok = false;
                    }
                    i += 1;
                }
                let mut z = 0;
                while z < 2 {
                    if mem.get(TCB as usize + 6 + z) != Some(&0) {
                        ok = false;
                    }
                    z += 1;
                }
                let mut h = 0;
                while h < TCB as usize {
                    if mem.get(h) != Some(&0) {
                        ok = false;
                    }
                    h += 1;
                }
                ok = ok && t.size == TCB + 8 && t.base == 0;
            } else {
                ok = false;
            }
            let mut spare = [0u8; ARENA_CAP];
            let pad = Image::make(&[9, 8], 2, 16);
            ok = ok
                && copy_image(&mut spare, &pad, TCB + 16).is_ok()
                && spare.get(TCB as usize) == Some(&9)
                && spare.get(TCB as usize + 2) == Some(&0)
                && spare.get(TCB as usize + 4) == Some(&POISON);
            ok
        },
        "static",
    );

    let mut f = build_ledger();
    let rep = f.replay();
    let root = match f.freeze() {
        Ok(v) => v,
        Err(_) => 0,
    };
    let again = f.freeze() == Err(TH_EFROZEN);
    let inj = inject_four();
    s.add_dyn(
        "TH13-冻结重放与四路注入",
        root != 0 && rep == Ok(root) && again && f.root() == root && f.rejects() == 1 && inj == [true; 4],
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
    fn six_spawns_walk_the_pool() {
        let mut mem = [0u8; ARENA_CAP];
        let mut pool = Pool::new();
        let imgs = six_images();
        let mut tps = [0u32; 6];
        let mut i = 0;
        while i < 6 {
            let img = imgs[i];
            let t = spawn(&mut mem, &mut pool, &img).expect("spawn");
            tps[i] = t.tp;
            assert_eq!(t.steps, 3);
            assert_eq!(t.tp, t.base + TCB);
            i += 1;
        }
        assert_eq!(tps, [48, 112, 192, 272, 352, 448]);
        assert_eq!(pool.cursor(), 464);
        assert_eq!(pool.live(), 6);
    }

    #[test]
    fn release_then_reuse_same_size() {
        let mut mem = [0u8; ARENA_CAP];
        let mut pool = Pool::new();
        let imgs = six_images();
        let first = spawn(&mut mem, &mut pool, &imgs[2]).expect("first");
        assert_eq!(first.base, 0);
        assert!(pool.release(0).is_ok());
        let again = spawn(&mut mem, &mut pool, &imgs[2]).expect("again");
        assert_eq!(again.base, 0);
        assert_eq!(again.tp, 48);
        assert_eq!(pool.cursor(), 80);
    }

    #[test]
    fn pool_full_is_enomem_and_rolls_back() {
        let mut mem = [0u8; ARENA_CAP];
        let mut pool = Pool::new();
        let img = Image::make(&[0u8; 4], 0, 16);
        let mut i = 0;
        while i < POOL {
            assert!(spawn(&mut mem, &mut pool, &img).is_ok());
            i += 1;
        }
        assert_eq!(spawn(&mut mem, &mut pool, &img), Err(TH_ENOMEM));
        assert_eq!(pool.live(), POOL);
        assert_eq!(pool.free_len(), 0);
    }

    #[test]
    fn gates_are_ordered() {
        let mut pool = Pool::new();
        assert_eq!(pool.alloc(0, 3), Err(TH_ESIZE));
        assert_eq!(pool.alloc(8, 3), Err(TH_EALIGN));
        assert_eq!(pool.alloc(ARENA_CAP as u32 + 1, 16), Err(TH_ENOMEM));
        assert_eq!(pool.release(0), Err(TH_EFREE));
    }

    #[test]
    fn image_copy_poisons_and_zeroes() {
        let img = Image::make(&[9, 8, 7, 6], 6, 16);
        let mut mem = [0u8; ARENA_CAP];
        assert!(copy_image(&mut mem, &img, TCB + 10).is_ok());
        assert_eq!(mem[TCB as usize], 9);
        assert_eq!(mem[TCB as usize + 3], 6);
        assert_eq!(mem[TCB as usize + 4], 0);
        assert_eq!(mem[TCB as usize + 9], 0);
        assert_eq!(mem[TCB as usize + 10], 0);
        assert_eq!(copy_image(&mut mem, &img, TCB + 4), Err(TH_EIMG));
    }

    #[test]
    fn tp_must_be_16_aligned() {
        assert_eq!(set_tp(0, TCB), Ok(48));
        assert_eq!(set_tp(64, TCB), Ok(112));
        assert_eq!(set_tp(64, 40), Err(TH_ETP));
        assert_eq!(set_tp(1024 - TCB, TCB), Err(TH_ETP));
    }

    #[test]
    fn ledger_seals_and_replays() {
        let mut l = build_ledger();
        assert!(l.audit().is_ok());
        let root = l.freeze().expect("freeze");
        assert_eq!(l.replay(), Ok(root));
        assert_eq!(l.freeze(), Err(TH_EFROZEN));
        assert_eq!(l.rejects(), 1);
    }

    #[test]
    fn checks_pass() {
        let s = run_unx11311_checks();
        assert_eq!(s.len(), 13);
        assert!(s.all_passed(), "{}", s.red_items_text());
    }
}
