//! UNX-F11318 TLS 压力判据（千线程级，0xe9d0 段）
//!
//! 线程一多，TLS 的故障模式从“算错”变成“串号”：某个线程读到的块内偏移
//! 是邻居的，或者退出后槽位没归还导致后面创建失败。这两类错在单线程下
//! 不可见，必须用千轮创建-写值-读回-退出把它逼出来。
//!
//! 模型给每个线程一个固定尺寸的 TLS 块和一枚“标记”：创建时写入标记，
//! 立刻读回比对，不一致就记一次故障（EMARK）。标记的作用等价于真实内核
//! 里线程往 `pthread_self()->tid` 写再读——值错位立刻可见，且不依赖内存
//! 布局的巧合。
//!
//! 归还走偏移空闲表：同尺寸块回收后优先复用，游标只在表空时前进。这样
//! 千轮压力后游标应定格在首轮的 8 块长度，而不是涨到 1000×8 块——账目
//! 守恒（活跃字节为零、空闲块数等于并发度、游标不涨）是压力判据的硬指标。
//!
//! 账本十四格：八格成功（两格创建、两格读值、一格校验、一格归还、一格
//! 复用、一格存活计数），六格拒绝（越界下标、标记错位、归还越界、重复
//! 归还、槽满、容量不足）。

use crate::alloc::format;
use crate::checks::CheckSet;

/// 段基址。
pub const SEG_BASE: u32 = 0xe9d0;

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
/// 越界下标。
pub const SP_EIDX: u32 = SEG_BASE + 0x08;
/// 标记错位。
pub const SP_EMARK: u32 = SEG_BASE + 0x09;
/// 重复归还。
pub const SP_EFREE: u32 = SEG_BASE + 0x0a;
/// 槽满。
pub const SP_EFULL: u32 = SEG_BASE + 0x0b;
/// 容量不足。
pub const SP_ENOMEM: u32 = SEG_BASE + 0x0c;
/// 票号不一致。
pub const SP_ESEAL: u32 = SEG_BASE + 0x0d;

/// 判决：成功。
pub const V_OK: u32 = 0;

/// 操作：创建线程 TLS。
pub const OP_SPAWN: u8 = 1;
/// 操作：读标记。
pub const OP_READ: u8 = 2;
/// 操作：校验标记。
pub const OP_VERIFY: u8 = 3;
/// 操作：归还。
pub const OP_RELEASE: u8 = 4;
/// 操作：存活计数。
pub const OP_ALIVE: u8 = 5;

/// 默认容量。
pub const CAP: u32 = 1024;
/// 槽数。
pub const SLOTS: usize = 16;
/// 每线程块尺寸。
pub const BLOCK: u32 = 64;
/// 单轮并发度。
pub const GRP: usize = 8;
/// 样本格数。
pub const SAMPLES: usize = 14;

/// FNV 偏移基。
pub const FNV_OFF: u64 = 0xcbf2_9ce4_8422_2325;
/// FNV 质数。
pub const FNV_PRIME: u64 = 0x100_0000_01b3;

/// 一个线程 TLS 槽。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Slot {
    pub used: bool,
    pub off: u32,
    pub marker: u32,
}

impl Slot {
    /// 空槽。
    pub const fn blank() -> Slot {
        Slot {
            used: false,
            off: 0,
            marker: 0,
        }
    }
}

/// 压力报告。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Stress {
    pub live: usize,
    pub faults: u32,
    pub peak: usize,
    pub allocs: u32,
    pub cursor: u32,
    pub free_blocks: usize,
}

/// 线程 TLS 池。
#[derive(Clone, Copy, Debug)]
pub struct Pool {
    slots: [Slot; SLOTS],
    free_offs: [u32; SLOTS],
    nfree: usize,
    n: usize,
    cursor: u32,
    cap: u32,
    peak: usize,
    allocs: u32,
    released: u32,
    faults: u32,
}

impl Pool {
    /// 默认容量池。
    pub const fn new() -> Pool {
        Pool {
            slots: [Slot::blank(); SLOTS],
            free_offs: [0; SLOTS],
            nfree: 0,
            n: 0,
            cursor: 0,
            cap: CAP,
            peak: 0,
            allocs: 0,
            released: 0,
            faults: 0,
        }
    }

    /// 指定容量池。
    pub const fn with_cap(cap: u32) -> Pool {
        Pool {
            slots: [Slot::blank(); SLOTS],
            free_offs: [0; SLOTS],
            nfree: 0,
            n: 0,
            cursor: 0,
            cap,
            peak: 0,
            allocs: 0,
            released: 0,
            faults: 0,
        }
    }

    /// 存活线程数。
    pub fn alive(&self) -> usize {
        self.n
    }

    /// 峰值。
    pub fn peak(&self) -> usize {
        self.peak
    }

    /// 分配次数。
    pub fn allocs(&self) -> u32 {
        self.allocs
    }

    /// 归还次数。
    pub fn released(&self) -> u32 {
        self.released
    }

    /// 故障计数。
    pub fn faults(&self) -> u32 {
        self.faults
    }

    /// 游标。
    pub fn cursor(&self) -> u32 {
        self.cursor
    }

    /// 空闲块数。
    pub fn free_blocks(&self) -> usize {
        self.nfree
    }

    /// 活跃字节。
    pub fn used_bytes(&self) -> u32 {
        self.n as u32 * BLOCK
    }

    /// 取槽。
    pub fn slot(&self, i: usize) -> Option<Slot> {
        self.slots.get(i).copied()
    }

    /// 创建：槽 → 空闲偏移 → 游标。
    pub fn spawn(&mut self, marker: u32) -> Result<u32, u32> {
        let mut idx = SLOTS;
        let mut i = 0;
        while i < SLOTS {
            match self.slots.get(i) {
                Some(s) => {
                    if !s.used {
                        idx = i;
                        break;
                    }
                }
                None => break,
            }
            i += 1;
        }
        if idx == SLOTS {
            return Err(SP_EFULL);
        }
        let off = if self.nfree > 0 {
            let off = match self.free_offs.get(self.nfree - 1) {
                Some(v) => *v,
                None => 0,
            };
            self.nfree -= 1;
            off
        } else {
            let end = match self.cursor.checked_add(BLOCK) {
                Some(v) => v,
                None => return Err(SP_ENOMEM),
            };
            if end > self.cap {
                return Err(SP_ENOMEM);
            }
            let o = self.cursor;
            self.cursor = end;
            o
        };
        match self.slots.get_mut(idx) {
            Some(s) => *s = Slot {
                used: true,
                off,
                marker,
            },
            None => return Err(SP_EFULL),
        }
        self.n += 1;
        self.allocs += 1;
        if self.n > self.peak {
            self.peak = self.n;
        }
        Ok(idx as u32)
    }

    /// 读标记。
    pub fn read(&self, idx: usize) -> Result<u32, u32> {
        match self.slots.get(idx) {
            Some(s) => {
                if s.used {
                    Ok(s.marker)
                } else {
                    Err(SP_EID)
                }
            }
            None => Err(SP_EIDX),
        }
    }

    /// 校验标记：错位即记故障。
    pub fn verify(&mut self, idx: usize, want: u32) -> Result<u32, u32> {
        match self.read(idx) {
            Ok(got) => {
                if got != want {
                    self.faults += 1;
                    return Err(SP_EMARK);
                }
                Ok(got)
            }
            Err(c) => Err(c),
        }
    }

    /// 归还：下标 → 已归还未登记 → 入表。
    pub fn release(&mut self, idx: usize) -> Result<u32, u32> {
        let s = match self.slots.get(idx) {
            Some(v) => *v,
            None => return Err(SP_EIDX),
        };
        if !s.used {
            return Err(SP_EFREE);
        }
        if self.nfree < SLOTS {
            if let Some(x) = self.free_offs.get_mut(self.nfree) {
                *x = s.off;
                self.nfree += 1;
            }
        }
        if let Some(x) = self.slots.get_mut(idx) {
            x.used = false;
        }
        self.n -= 1;
        self.released += 1;
        Ok(idx as u32)
    }
}

/// 千轮压力：每轮 GRP 个并发线程，写值-读回-归还。
pub fn stress(rounds: u32) -> Stress {
    let mut p = Pool::new();
    let mut r = 0;
    while r < rounds {
        let mut ids = [0u32; GRP];
        let mut got = [0usize; GRP];
        let mut i = 0;
        while i < GRP {
            let marker = r * GRP as u32 + i as u32 + 1;
            match p.spawn(marker) {
                Ok(s) => {
                    got[i] = s as usize;
                    ids[i] = marker;
                }
                Err(_) => got[i] = SLOTS,
            }
            i += 1;
        }
        let mut k = 0;
        while k < GRP {
            if got[k] != SLOTS {
                let _ = p.verify(got[k], ids[k]);
            }
            k += 1;
        }
        let mut m = 0;
        while m < GRP {
            if got[m] != SLOTS {
                let _ = p.release(got[m]);
            }
            m += 1;
        }
        r += 1;
    }
    Stress {
        live: p.alive(),
        faults: p.faults(),
        peak: p.peak(),
        allocs: p.allocs(),
        cursor: p.cursor(),
        free_blocks: p.free_blocks(),
    }
}

/// 一个账本样本。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Sample {
    pub slot: u8,
    pub op: u8,
    pub ok: bool,
    pub code: u32,
    pub value: u32,
}

impl Sample {
    /// 空白格。
    pub const fn blank() -> Sample {
        Sample {
            slot: 0,
            op: 0,
            ok: false,
            code: 0,
            value: 0,
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
            return Err(SP_EFROZEN);
        }
        if (row.slot as usize) != self.len {
            return Err(SP_EORDER);
        }
        if self.len >= SAMPLES {
            return Err(SP_EID);
        }
        let mut i = 0;
        while i < self.len {
            match self.rows.get(i) {
                Some(x) => {
                    if x.slot == row.slot {
                        return Err(SP_EDUP);
                    }
                }
                None => return Err(SP_ENOBS),
            }
            i += 1;
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
        if self.ok_count() != 8 || self.err_count() != 6 {
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
                        if s.code != V_OK {
                            return Err(SP_EMISMATCH);
                        }
                    } else if s.code == V_OK || s.value != 0 {
                        return Err(SP_EMISMATCH);
                    }
                }
                None => return Err(SP_ENOBS),
            }
            i += 1;
        }
        let vals = [0u32, 1, 7, 8, 8, 1, 1, 2];
        let mut k = 0;
        while k < vals.len() {
            match (self.sample(k), vals.get(k)) {
                (Some(s), Some(w)) => {
                    if !s.ok || s.value != *w {
                        return Err(SP_EMISMATCH);
                    }
                }
                _ => return Err(SP_EBAL),
            }
            k += 1;
        }
        let codes = [SP_EID, SP_EMARK, SP_EIDX, SP_EFREE, SP_EFULL, SP_ENOMEM];
        let mut c = 0;
        let mut j = 8;
        while j < self.len {
            match self.rows.get(j) {
                Some(s) => match codes.get(c) {
                    Some(w) => {
                        if s.code != *w {
                            return Err(SP_EMISMATCH);
                        }
                    }
                    None => return Err(SP_EBAL),
                },
                None => return Err(SP_ENOBS),
            }
            c += 1;
            j += 1;
        }
        if c != codes.len() {
            return Err(SP_EBAL);
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
                h = fold(h, s.value as u64);
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

/// 由结果生成一格。
pub fn cell(slot: u8, op: u8, r: Result<u32, u32>) -> Sample {
    match r {
        Ok(v) => Sample {
            slot,
            op,
            ok: true,
            code: V_OK,
            value: v,
        },
        Err(c) => Sample {
            slot,
            op,
            ok: false,
            code: c,
            value: 0,
        },
    }
}

/// 构建十四格账本。
pub fn build_ledger() -> Ledger {
    let mut l = Ledger::new();
    let mut p = Pool::new();

    let _ = l.register(cell(0, OP_SPAWN, p.spawn(7)));
    let _ = l.register(cell(1, OP_SPAWN, p.spawn(8)));
    let _ = l.register(cell(2, OP_READ, p.read(0)));
    let _ = l.register(cell(3, OP_READ, p.read(1)));
    let _ = l.register(cell(4, OP_VERIFY, p.verify(1, 8)));
    let _ = l.register(cell(5, OP_RELEASE, p.release(1)));
    let _ = l.register(cell(6, OP_SPAWN, p.spawn(9)));
    let _ = l.register(Sample {
        slot: 7,
        op: OP_ALIVE,
        ok: true,
        code: V_OK,
        value: p.alive() as u32,
    });

    let _ = l.register(cell(8, OP_READ, p.read(5)));
    let _ = l.register(cell(9, OP_VERIFY, p.verify(0, 99)));
    let _ = l.register(cell(10, OP_RELEASE, p.release(SLOTS)));
    let _ = p.release(1);
    let _ = l.register(cell(11, OP_RELEASE, p.release(1)));

    let mut full = Pool::new();
    let mut i = 0;
    while i < SLOTS {
        let _ = full.spawn(i as u32 + 1);
        i += 1;
    }
    let _ = l.register(cell(12, OP_SPAWN, full.spawn(99)));

    let mut tiny = Pool::with_cap(BLOCK);
    let _ = tiny.spawn(1);
    let _ = l.register(cell(13, OP_SPAWN, tiny.spawn(2)));

    l
}

/// 四路注入：账本重放、千轮压力、账目守恒、闸门齐全。
pub fn inject_four() -> [bool; 4] {
    let mut out = [false; 4];

    let l = build_ledger();
    let base = l.len() == SAMPLES && l.audit().is_ok();
    let rep = l.replay();
    let root = l.seal();
    out[0] = base && rep == Ok(root) && root != 0;

    let s = stress(1000);
    out[1] = s.live == 0
        && s.faults == 0
        && s.peak == GRP
        && s.allocs == 8000
        && s.cursor == BLOCK * GRP as u32
        && s.free_blocks == GRP;

    out[2] = {
        let mut p = Pool::new();
        let mut i = 0;
        let mut good = true;
        while i < 100 {
            match p.spawn(i + 1) {
                Ok(slot) => {
                    let want = i + 1;
                    if p.verify(slot as usize, want) != Ok(want) {
                        good = false;
                    }
                    if p.release(slot as usize) != Ok(slot) {
                        good = false;
                    }
                }
                Err(_) => good = false,
            }
            i += 1;
        }
        good && p.alive() == 0
            && p.used_bytes() == 0
            && p.cursor() == BLOCK
            && p.free_blocks() == 1
            && p.allocs() == 100
            && p.released() == 100
            && p.faults() == 0
    };

    out[3] = pool_gates();

    out
}

/// 六道闸门逐一验证。
pub fn pool_gates() -> bool {
    let mut p = Pool::new();
    if p.read(0) != Err(SP_EID) {
        return false;
    }
    if p.read(SLOTS) != Err(SP_EIDX) {
        return false;
    }
    if p.release(0) != Err(SP_EFREE) {
        return false;
    }
    if p.spawn(1) != Ok(0) {
        return false;
    }
    if p.verify(0, 2) != Err(SP_EMARK) || p.faults() != 1 {
        return false;
    }
    if p.verify(0, 1) != Ok(1) {
        return false;
    }
    if p.release(0) != Ok(0) {
        return false;
    }
    if p.release(0) != Err(SP_EFREE) {
        return false;
    }
    let mut full = Pool::new();
    let mut i = 0;
    while i < SLOTS {
        if full.spawn(i as u32 + 1) != Ok(i as u32) {
            return false;
        }
        i += 1;
    }
    if full.spawn(1) != Err(SP_EFULL) {
        return false;
    }
    let mut tiny = Pool::with_cap(BLOCK);
    if tiny.spawn(1) != Ok(0) {
        return false;
    }
    if tiny.spawn(2) != Err(SP_ENOMEM) {
        return false;
    }
    true
}

/// 十三项判据。
pub fn run_unx11318_checks() -> CheckSet {
    let mut s = CheckSet::new("unx11318.tlsstress");
    let l = build_ledger();

    s.add(
        "SP1-十四格齐备",
        l.len() == SAMPLES && !l.is_empty() && l.sample(0).map(|x| x.op) == Some(OP_SPAWN),
        "static",
    );

    s.add_dyn(
        "SP2-八允六拒分布正确",
        l.ok_count() == 8 && l.err_count() == 6 && l.audit().is_ok(),
        &format!("ok={} err={}", l.ok_count(), l.err_count()),
    );

    s.add(
        "SP3-槽位与标记",
        l.sample(0).map(|x| x.value) == Some(0)
            && l.sample(1).map(|x| x.value) == Some(1)
            && l.sample(2).map(|x| x.value) == Some(7)
            && l.sample(3).map(|x| x.value) == Some(8),
        "static",
    );

    s.add(
        "SP4-校验成功回读",
       {
            let mut p = Pool::new();
            let a = p.spawn(41);
            let b = p.verify(0, 41);
            a == Ok(0) && b == Ok(41) && p.faults() == 0 && p.slot(0).map(|x| x.off) == Some(0)
                && p.slot(0).map(|x| x.marker) == Some(41)
        },
        "static",
    );

    s.add(
        "SP5-归还后槽位复用",
        {
            let mut p = Pool::new();
            let _ = p.spawn(1);
            let _ = p.spawn(2);
            let rel = p.release(1);
            let again = p.spawn(3);
            rel == Ok(1)
                && again == Ok(1)
                && p.slot(1).map(|x| x.off) == Some(BLOCK)
                && p.free_blocks() == 0
        },
        "static",
    );

    s.add(
        "SP6-下标闸门",
        l.sample(8).map(|x| x.code) == Some(SP_EID)
            && l.sample(10).map(|x| x.code) == Some(SP_EIDX)
            && {
                let p = Pool::new();
                p.read(SLOTS) == Err(SP_EIDX) && p.read(SLOTS + 3) == Err(SP_EIDX)
            },
        "static",
    );

    s.add(
        "SP7-标记错位闸门",
        l.sample(9).map(|x| x.code) == Some(SP_EMARK)
            && {
                let mut p = Pool::new();
                let _ = p.spawn(11);
                let _ = p.spawn(12);
                p.verify(0, 12) == Err(SP_EMARK)
                    && p.faults() == 1
                    && p.verify(1, 12) == Ok(12)
                    && p.faults() == 1
            },
        "static",
    );

    s.add(
        "SP8-重复归还闸门",
        l.sample(11).map(|x| x.code) == Some(SP_EFREE)
            && {
                let mut p = Pool::new();
                let _ = p.spawn(5);
                p.release(0) == Ok(0) && p.release(0) == Err(SP_EFREE) && p.released() == 1
            },
        "static",
    );

    s.add(
        "SP9-槽满闸门",
        l.sample(12).map(|x| x.code) == Some(SP_EFULL)
            && {
                let mut p = Pool::new();
                let mut i = 0;
                let mut all = true;
                while i < SLOTS {
                    if p.spawn(1) != Ok(i as u32) {
                        all = false;
                    }
                    i += 1;
                }
                all && p.alive() == SLOTS && p.peak() == SLOTS && p.spawn(1) == Err(SP_EFULL)
            },
        "static",
    );

    s.add(
        "SP10-容量闸门",
        l.sample(13).map(|x| x.code) == Some(SP_ENOMEM)
            && {
                let mut p = Pool::with_cap(BLOCK * 2);
                p.spawn(1) == Ok(0) && p.spawn(2) == Ok(1) && p.spawn(3) == Err(SP_ENOMEM)
                    && p.cursor() == BLOCK * 2
            },
        "static",
    );

    s.add(
        "SP11-千轮压力判据",
        {
            let s = stress(1000);
            s.live == 0
                && s.faults == 0
                && s.peak == GRP
                && s.allocs == 8000
                && s.cursor == BLOCK * GRP as u32
                && s.free_blocks == GRP
        },
        "static",
    );

    s.add(
        "SP12-账目守恒",
        {
            let s = stress(64);
            s.live == 0
                && s.allocs == 64 * GRP as u32
                && s.cursor == BLOCK * GRP as u32
                && s.free_blocks == GRP
                && stress(0).allocs == 0
                && stress(0).cursor == 0
        },
        "static",
    );

    s.add(
        "SP13-冻结重放与四路注入",
        {
            let mut f = build_ledger();
            let rep = f.replay();
            let one = f.freeze();
            let two = f.freeze();
            let inj = inject_four();
            one.is_ok()
                && one == rep
                && two == Err(SP_EFROZEN)
                && f.rejects() == 1
                && f.is_frozen()
                && f.root() != 0
                && inj == [true, true, true, true]
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
        let s = run_unx11318_checks();
        assert_eq!(s.len(), 13);
        assert!(s.all_passed());
    }

    #[test]
    fn spawn_reuses_offsets() {
        let mut p = Pool::new();
        assert_eq!(p.spawn(1), Ok(0));
        assert_eq!(p.spawn(2), Ok(1));
        assert_eq!(p.release(0), Ok(0));
        assert_eq!(p.spawn(3), Ok(0));
        assert_eq!(p.cursor(), BLOCK * 2);
        assert_eq!(p.free_blocks(), 0);
    }

    #[test]
    fn peak_tracks_high_water() {
        let mut p = Pool::new();
        let mut i = 0;
        while i < 5 {
            assert!(p.spawn(i + 1).is_ok());
            i += 1;
        }
        assert_eq!(p.peak(), 5);
        let _ = p.release(0);
        let _ = p.release(1);
        assert_eq!(p.peak(), 5);
        assert_eq!(p.alive(), 3);
    }

    #[test]
    fn marker_mismatch_counts_fault() {
        let mut p = Pool::new();
        let _ = p.spawn(1);
        assert_eq!(p.verify(0, 2), Err(SP_EMARK));
        assert_eq!(p.faults(), 1);
        assert_eq!(p.verify(0, 1), Ok(1));
        assert_eq!(p.faults(), 1);
    }

    #[test]
    fn stress_leaves_no_residue() {
        let s = stress(200);
        assert_eq!(s.live, 0);
        assert_eq!(s.faults, 0);
        assert_eq!(s.peak, GRP);
        assert_eq!(s.allocs, 1600);
        assert_eq!(s.cursor, BLOCK * GRP as u32);
        assert_eq!(s.free_blocks, GRP);
    }

    #[test]
    fn empty_stress_is_identity() {
        let s = stress(0);
        assert_eq!(s.live, 0);
        assert_eq!(s.allocs, 0);
        assert_eq!(s.cursor, 0);
        assert_eq!(s.free_blocks, 0);
    }

    #[test]
    fn freeze_then_replay_matches() {
        let mut l = build_ledger();
        let root = l.seal();
        assert_eq!(l.freeze(), Ok(root));
        assert_eq!(l.replay(), Ok(root));
        assert_eq!(l.freeze(), Err(SP_EFROZEN));
    }
}
