//! UNX-F11312 线程退出 TLS 清账（0xe970 段）
//!
//! 线程退出必须把创建时借走的一切原样还清：块交回池、DTV 条目作废、槽位
//! 转入“已死”态等待复用。少还一块就是一整块线程区泄漏；多还一次就是重复
//! 释放，二者都会让池的账目在长时间运行后失真。模型用三项计数守这条线：
//! `used`（活块字节数）、`reclaimed`（累计归还条目数）与 `live`（活块数），
//! 退出路径每一步都要让三者与块表保持一致。
//!
//! 退出走五道闸门：槽位越界或空闲、槽位已死、代号过期、仍被借用、块无法
//! 归还。顺序刻意固定——空闲槽位优先报“未知线程”，已死槽位报“重复退出”，
//! 代号比较排在状态之后，这样重复退出即使代号陈旧也报重复，而不是过期，
//! 与装载器“先看状态再看票”的习惯一致。
//!
//! 千线程循环是这张账的终极判据：同一槽位被创建-退出反复复用一千次后，
//! 活块数、占用字节数必须归零，归还条目数必须恰为千倍的 DTV 条目数，
//! 代号计数必须单调递增一千次。账本十四格中八格成功（三次创建、四次退出、
//! 一次槽位复用），六格拒绝分别命中重复退出、未知线程、借用未解、槽满、
//! 池尽与代号过期。

use crate::alloc::format;
use crate::checks::CheckSet;

/// 段基址。
pub const SEG_BASE: u32 = 0xe970;

/// 编号非法。
pub const EX_EID: u32 = SEG_BASE + 0x01;
/// 登记顺序错误。
pub const EX_EORDER: u32 = SEG_BASE + 0x02;
/// 重复登记。
pub const EX_EDUP: u32 = SEG_BASE + 0x03;
/// 已冻结。
pub const EX_EFROZEN: u32 = SEG_BASE + 0x04;
/// 观测不存在的格。
pub const EX_ENOBS: u32 = SEG_BASE + 0x05;
/// 结论不符。
pub const EX_EMISMATCH: u32 = SEG_BASE + 0x06;
/// 账本失衡。
pub const EX_EBAL: u32 = SEG_BASE + 0x07;
/// 未知线程。
pub const EX_EUNKNOWN: u32 = SEG_BASE + 0x08;
/// 重复退出。
pub const EX_EDOUBLE: u32 = SEG_BASE + 0x09;
/// 代号过期。
pub const EX_ESTALE: u32 = SEG_BASE + 0x0a;
/// 仍被借用。
pub const EX_EBUSY: u32 = SEG_BASE + 0x0b;
/// 线程槽已满。
pub const EX_EFULL: u32 = SEG_BASE + 0x0c;
/// 池容量不足。
pub const EX_ENOMEM: u32 = SEG_BASE + 0x0d;
/// 尺寸为零。
pub const EX_ESIZE: u32 = SEG_BASE + 0x0e;
/// 块不在池中。
pub const EX_EFREE: u32 = SEG_BASE + 0x0f;
/// 票号不一致。
pub const EX_ESEAL: u32 = SEG_BASE + 0x10;
/// 对齐非法。
pub const EX_EALIGN: u32 = SEG_BASE + 0x11;

/// 判决：成功。
pub const V_OK: u32 = 0;

/// 槽位：空闲。
pub const ST_FREE: u8 = 0;
/// 槽位：活。
pub const ST_LIVE: u8 = 1;
/// 槽位：已死。
pub const ST_DEAD: u8 = 2;

/// 操作：创建。
pub const OP_CREATE: u8 = 1;
/// 操作：退出。
pub const OP_EXIT: u8 = 2;

/// TCB 头长度。
pub const TCB: u32 = 48;
/// 线程槽数。
pub const SLOTS: usize = 3;
/// 池块数。
pub const BLOCKS: usize = 6;
/// 池容量。
pub const ARENA_CAP: usize = 1024;
/// 样本格数。
pub const SAMPLES: usize = 14;

/// FNV 偏移基。
pub const FNV_OFF: u64 = 0xcbf2_9ce4_8422_2325;
/// FNV 质数。
pub const FNV_PRIME: u64 = 0x100_0000_01b3;

/// 池内一块。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Block {
    pub base: u32,
    pub size: u32,
    pub live: bool,
}

/// 线程槽。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Slot {
    pub state: u8,
    pub gen: u32,
    pub tp: u32,
    pub base: u32,
    pub size: u32,
    pub dtv: u32,
    pub busy: bool,
}

impl Slot {
    /// 空槽。
    pub const fn empty() -> Slot {
        Slot {
            state: ST_FREE,
            gen: 0,
            tp: 0,
            base: 0,
            size: 0,
            dtv: 0,
            busy: false,
        }
    }
}

/// 线程账：槽表与块池共用一本账。
#[derive(Clone, Copy, Debug)]
pub struct Threads {
    slots: [Slot; SLOTS],
    blocks: [Block; BLOCKS],
    frees: [usize; BLOCKS],
    nblocks: usize,
    nfree: usize,
    cursor: u32,
    used: u32,
    reclaimed: u32,
    next_gen: u32,
}

impl Threads {
    /// 空账。
    pub const fn new() -> Threads {
        Threads {
            slots: [Slot::empty(); SLOTS],
            blocks: [Block {
                base: 0,
                size: 0,
                live: false,
            }; BLOCKS],
            frees: [0; BLOCKS],
            nblocks: 0,
            nfree: 0,
            cursor: 0,
            used: 0,
            reclaimed: 0,
            next_gen: 1,
        }
    }

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

    /// 分配块：尺寸 → 对齐 → 空闲链 → 游标。
    pub fn alloc_block(&mut self, size: u32, align: u32) -> Result<u32, u32> {
        if size == 0 {
            return Err(EX_ESIZE);
        }
        if align == 0 || !align.is_power_of_two() {
            return Err(EX_EALIGN);
        }
        let mut k = 0;
        while k < self.nfree {
            let idx = match self.frees.get(k) {
                Some(i) => *i,
                None => return Err(EX_ENOMEM),
            };
            let exact = match self.blocks.get(idx) {
                Some(b) => !b.live && b.size == size && b.base % align == 0,
                None => false,
            };
            if exact {
                let base = match self.blocks.get(idx) {
                    Some(b) => b.base,
                    None => return Err(EX_ENOMEM),
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
                self.used += size;
                return Ok(base);
            }
            k += 1;
        }
        if self.nblocks >= BLOCKS {
            return Err(EX_ENOMEM);
        }
        let base = match Threads::align_up(self.cursor, align) {
            Some(v) => v,
            None => return Err(EX_ENOMEM),
        };
        let end = match base.checked_add(size) {
            Some(v) => v,
            None => return Err(EX_ENOMEM),
        };
        if (end as usize) > ARENA_CAP {
            return Err(EX_ENOMEM);
        }
        if let Some(slot) = self.blocks.get_mut(self.nblocks) {
            *slot = Block {
                base,
                size,
                live: true,
            };
        } else {
            return Err(EX_ENOMEM);
        }
        self.nblocks += 1;
        self.cursor = end;
        self.used += size;
        Ok(base)
    }

    /// 归还块。
    pub fn release_block(&mut self, base: u32) -> Result<(), u32> {
        let mut i = 0;
        while i < self.nblocks {
            let hit = match self.blocks.get(i) {
                Some(b) => b.live && b.base == base,
                None => false,
            };
            if hit {
                let size = match self.blocks.get(i) {
                    Some(b) => b.size,
                    None => return Err(EX_EFREE),
                };
                if let Some(b) = self.blocks.get_mut(i) {
                    b.live = false;
                }
                if self.nfree < BLOCKS {
                    if let Some(s) = self.frees.get_mut(self.nfree) {
                        *s = i;
                    }
                    self.nfree += 1;
                }
                self.used = self.used.saturating_sub(size);
                return Ok(());
            }
            i += 1;
        }
        Err(EX_EFREE)
    }

    /// 创建线程：找槽 → 借块 → 记账。
    pub fn create(&mut self, size: u32, align: u32, dtv: u32) -> Result<(u8, u32, u32), u32> {
        if dtv == 0 {
            return Err(EX_ESIZE);
        }
        let mut idx = SLOTS;
        let mut i = 0;
        while i < SLOTS {
            if let Some(s) = self.slots.get(i) {
                if s.state == ST_FREE {
                    idx = i;
                    break;
                }
            }
            i += 1;
        }
        if idx == SLOTS {
            let mut j = 0;
            while j < SLOTS {
                if let Some(s) = self.slots.get(j) {
                    if s.state == ST_DEAD {
                        idx = j;
                        break;
                    }
                }
                j += 1;
            }
        }
        if idx == SLOTS {
            return Err(EX_EFULL);
        }
        let base = self.alloc_block(size, align)?;
        let tp = match base.checked_add(TCB) {
            Some(v) => v,
            None => return Err(EX_ENOMEM),
        };
        let gen = self.next_gen;
        if let Some(s) = self.slots.get_mut(idx) {
            *s = Slot {
                state: ST_LIVE,
                gen,
                tp,
                base,
                size,
                dtv,
                busy: false,
            };
        }
        self.next_gen = gen.saturating_add(1);
        Ok((idx as u8, gen, tp))
    }

    /// 退出线程：状态 → 代号 → 借用 → 归还 → 记账。
    pub fn exit(&mut self, tid: u8, gen: u32) -> Result<u32, u32> {
        let s = match self.slots.get(tid as usize) {
            Some(v) => *v,
            None => return Err(EX_EUNKNOWN),
        };
        if s.state == ST_FREE {
            return Err(EX_EUNKNOWN);
        }
        if s.state == ST_DEAD {
            return Err(EX_EDOUBLE);
        }
        if s.gen != gen {
            return Err(EX_ESTALE);
        }
        if s.busy {
            return Err(EX_EBUSY);
        }
        self.release_block(s.base)?;
        if let Some(slot) = self.slots.get_mut(tid as usize) {
            slot.state = ST_DEAD;
            slot.busy = false;
        }
        self.reclaimed = self.reclaimed.saturating_add(s.dtv);
        Ok(s.dtv)
    }

    /// 借用：阻止退出。
    pub fn pin(&mut self, tid: u8) -> Result<(), u32> {
        match self.slots.get_mut(tid as usize) {
            Some(s) => {
                if s.state != ST_LIVE {
                    return Err(EX_EUNKNOWN);
                }
                s.busy = true;
                Ok(())
            }
            None => Err(EX_EUNKNOWN),
        }
    }

    /// 解借。
    pub fn unpin(&mut self, tid: u8) -> Result<(), u32> {
        match self.slots.get_mut(tid as usize) {
            Some(s) => {
                if s.state != ST_LIVE {
                    return Err(EX_EUNKNOWN);
                }
                s.busy = false;
                Ok(())
            }
            None => Err(EX_EUNKNOWN),
        }
    }

    /// 槽。
    pub fn slot(&self, i: usize) -> Option<Slot> {
        self.slots.get(i).copied()
    }

    /// 占用字节。
    pub fn used(&self) -> u32 {
        self.used
    }

    /// 累计归还条目。
    pub fn reclaimed(&self) -> u32 {
        self.reclaimed
    }

    /// 活块数。
    pub fn live(&self) -> usize {
        let mut n = 0;
        let mut i = 0;
        while i < self.nblocks {
            if let Some(b) = self.blocks.get(i) {
                if b.live {
                    n += 1;
                }
            }
            i += 1;
        }
        n
    }

    /// 代号计数。
    pub fn gens(&self) -> u32 {
        self.next_gen
    }

    /// 注入检查：块表长度。
    pub fn blocks(&self) -> usize {
        self.nblocks
    }
}

/// 一个账本样本。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Sample {
    pub slot: u8,
    pub op: u8,
    pub ok: bool,
    pub code: u32,
    pub tid: u8,
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
            tid: 0,
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
            return Err(EX_EFROZEN);
        }
        if (row.slot as usize) != self.len {
            return Err(EX_EORDER);
        }
        if self.len >= SAMPLES {
            return Err(EX_EID);
        }
        let mut i = 0;
        while i < self.len {
            match self.rows.get(i) {
                Some(x) => {
                    if x.slot == row.slot {
                        return Err(EX_EDUP);
                    }
                }
                None => return Err(EX_ENOBS),
            }
            i += 1;
        }
        match self.rows.get_mut(self.len) {
            Some(x) => *x = row,
            None => return Err(EX_EID),
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

    /// 对账：分布、TP 轨迹、归还轨迹、拒绝码序列。
    pub fn audit(&self) -> Result<(), u32> {
        if self.len != SAMPLES {
            return Err(EX_EBAL);
        }
        if self.ok_count() != 8 || self.err_count() != 6 {
            return Err(EX_EBAL);
        }
        let mut i = 0;
        while i < self.len {
            match self.rows.get(i) {
                Some(s) => {
                    if s.slot as usize != i {
                        return Err(EX_EORDER);
                    }
                    if s.ok {
                        if s.code != V_OK {
                            return Err(EX_EMISMATCH);
                        }
                    } else if s.code == V_OK || s.value != 0 {
                        return Err(EX_EMISMATCH);
                    }
                }
                None => return Err(EX_ENOBS),
            }
            i += 1;
        }
        let vals = [
            (0usize, 48u32),
            (1, 112),
            (2, 176),
            (3, 2),
            (4, 3),
            (5, 48),
            (6, 4),
            (7, 1),
        ];
        let mut k = 0;
        while k < vals.len() {
            match vals.get(k) {
                Some((i, w)) => match self.sample(*i) {
                    Some(s) => {
                        if !s.ok || s.value != *w {
                            return Err(EX_EMISMATCH);
                        }
                    }
                    None => return Err(EX_ENOBS),
                },
                None => return Err(EX_EBAL),
            }
            k += 1;
        }
        let codes = [
            EX_EDOUBLE,
            EX_EUNKNOWN,
            EX_EBUSY,
            EX_EFULL,
            EX_ENOMEM,
            EX_ESTALE,
        ];
        let mut c = 0;
        let mut j = 8;
        while j < self.len {
            if let Some(s) = self.rows.get(j) {
                match codes.get(c) {
                    Some(w) => {
                        if s.code != *w {
                            return Err(EX_EMISMATCH);
                        }
                    }
                    None => return Err(EX_EBAL),
                }
                c += 1;
            }
            j += 1;
        }
        if c != codes.len() {
            return Err(EX_EBAL);
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
                h = fold(h, s.tid as u64);
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
            return Err(EX_EFROZEN);
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
            return Err(EX_EBAL);
        }
        let h = fresh.seal();
        if self.frozen && h != self.seal {
            return Err(EX_ESEAL);
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

/// 创建/退出一格。
fn created(slot: u8, tid: u8, r: Result<(u8, u32, u32), u32>) -> Sample {
    match r {
        Ok((_, _, tp)) => Sample {
            slot,
            op: OP_CREATE,
            ok: true,
            code: V_OK,
            tid,
            value: tp,
        },
        Err(c) => Sample {
            slot,
            op: OP_CREATE,
            ok: false,
            code: c,
            tid,
            value: 0,
        },
    }
}

fn exited(slot: u8, tid: u8, r: Result<u32, u32>) -> Sample {
    match r {
        Ok(v) => Sample {
            slot,
            op: OP_EXIT,
            ok: true,
            code: V_OK,
            tid,
            value: v,
        },
        Err(c) => Sample {
            slot,
            op: OP_EXIT,
            ok: false,
            code: c,
            tid,
            value: 0,
        },
    }
}

/// 构建十四格账本。
pub fn build_ledger() -> Ledger {
    let mut l = Ledger::new();
    let mut t = Threads::new();

    let dtvs = [2u32, 3, 1];
    let mut g = [0u32; 4];
    let mut i = 0;
    while i < 3 {
        let d = match dtvs.get(i) {
            Some(v) => *v,
            None => 1,
        };
        let r = t.create(64, 16, d);
        if let Ok((tid, gen, _)) = r {
            if let Some(x) = g.get_mut(tid as usize) {
                *x = gen;
            }
        }
        let _ = l.register(created(i as u8, i as u8, r));
        i += 1;
    }

    let _ = l.register(exited(3, 0, t.exit(0, g[0])));
    let _ = l.register(exited(4, 1, t.exit(1, g[1])));

    let reuse = t.create(64, 16, 4);
    if let Ok((tid, gen, _)) = reuse {
        if let Some(x) = g.get_mut(tid as usize) {
            *x = gen;
        }
    }
    let _ = l.register(created(5, 0, reuse));

    let _ = l.register(exited(6, 0, t.exit(0, g[0])));
    let _ = l.register(exited(7, 2, t.exit(2, g[2])));

    let _ = l.register(exited(8, 0, t.exit(0, g[0])));
    let _ = l.register(exited(9, 9, t.exit(9, 0)));

    let mut busy = Threads::new();
    let (bid, bgen, _) = match busy.create(64, 16, 2) {
        Ok(v) => v,
        Err(_) => (0, 0, 0),
    };
    let _ = busy.pin(bid);
    let _ = l.register(exited(10, bid, busy.exit(bid, bgen)));

    let mut full = Threads::new();
    let mut f = 0;
    while f < SLOTS {
        let _ = full.create(64, 16, 2);
        f += 1;
    }
    let _ = l.register(created(11, 0, full.create(64, 16, 2)));

    let mut tight = Threads::new();
    let _ = tight.create(600, 16, 2);
    let _ = l.register(created(12, 0, tight.create(600, 16, 2)));

    let mut stale = Threads::new();
    let _ = stale.create(64, 16, 2);
    let _ = l.register(exited(13, 0, stale.exit(0, 99)));

    l
}

/// 千线程循环：返回（活块数偏置, 占用, 归还, 代号）。
pub fn stress(rounds: u32) -> (u32, u32, u32, u32) {
    let mut t = Threads::new();
    let mut i = 0;
    while i < rounds {
        let (tid, gen, _) = match t.create(64, 16, 3) {
            Ok(v) => v,
            Err(_) => return (999, t.used(), t.reclaimed(), t.gens()),
        };
        if t.exit(tid, gen).is_err() {
            return (998, t.used(), t.reclaimed(), t.gens());
        }
        i += 1;
    }
    (t.live() as u32, t.used(), t.reclaimed(), t.gens())
}

/// 四路注入：账本重放、往返记账、千线程零泄漏、闸门优先级。
pub fn inject_four() -> [bool; 4] {
    let mut out = [false; 4];

    let l = build_ledger();
    let base = l.len() == SAMPLES && l.audit().is_ok();
    let rep = l.replay();
    let root = l.seal();
    out[0] = base && rep == Ok(root) && root != 0;

    let mut t = Threads::new();
    let mut ok = t.used() == 0 && t.live() == 0;
    let mut i = 0;
    while i < 5 {
        let (tid, gen, tp) = match t.create(128, 32, 2) {
            Ok(v) => v,
            Err(_) => return out,
        };
        if tp != 48 || t.used() != 128 || t.live() != 1 {
            ok = false;
        }
        if t.exit(tid, gen) != Ok(2) || t.used() != 0 || t.live() != 0 {
            ok = false;
        }
        i += 1;
    }
    out[1] = ok && t.reclaimed() == 10 && t.gens() == 6;

    let (live, used, reclaimed, gens) = stress(1000);
    out[2] = live == 0 && used == 0 && reclaimed == 3000 && gens == 1001;

    let mut p = Threads::new();
    let (tid, gen, _) = match p.create(64, 16, 2) {
        Ok(v) => v,
        Err(_) => return out,
    };
    let _ = p.pin(tid);
    let busy = p.exit(tid, gen);
    let _ = p.unpin(tid);
    let stale = p.exit(tid, gen + 7);
    let after = p.exit(tid, gen);
    let unknown = p.exit(tid, gen);
    let mut full = Threads::new();
    let mut k = 0;
    while k < SLOTS {
        let _ = full.create(64, 16, 1);
        k += 1;
    }
    out[3] = busy == Err(EX_EBUSY)
        && stale == Err(EX_ESTALE)
        && after == Ok(2)
        && unknown == Err(EX_EDOUBLE)
        && full.create(64, 16, 1) == Err(EX_EFULL)
        && p.exit(9, 1) == Err(EX_EUNKNOWN);

    out
}

/// 十三项判据。
pub fn run_unx11312_checks() -> CheckSet {
    let mut s = CheckSet::new("unx11312.tlsexit");
    let l = build_ledger();

    s.add(
        "EX1-十四格齐备",
        l.len() == SAMPLES && !l.is_empty() && l.sample(0).map(|x| x.op) == Some(OP_CREATE),
        "static",
    );

    s.add_dyn(
        "EX2-八允六拒分布正确",
        l.ok_count() == 8 && l.err_count() == 6 && l.audit().is_ok(),
        &format!("ok={} err={}", l.ok_count(), l.err_count()),
    );

    s.add(
        "EX3-创建 TP 轨迹",
        l.sample(0).map(|x| x.value) == Some(48)
            && l.sample(1).map(|x| x.value) == Some(112)
            && l.sample(2).map(|x| x.value) == Some(176),
        "static",
    );

    s.add(
        "EX4-退出归还条目",
        l.sample(3).map(|x| x.value) == Some(2)
            && l.sample(4).map(|x| x.value) == Some(3)
            && l.sample(6).map(|x| x.value) == Some(4)
            && l.sample(7).map(|x| x.value) == Some(1),
        "static",
    );

    s.add(
        "EX5-槽位与块复用",
        l.sample(5).map(|x| x.value) == Some(48)
            && {
                let mut t = Threads::new();
                let (a, ga, _) = match t.create(64, 16, 2) {
                    Ok(v) => v,
                    Err(_) => (0u8, 0, 0),
                };
                let (_b, gb, _) = match t.create(64, 16, 2) {
                    Ok(v) => v,
                    Err(_) => (0u8, 0, 0),
                };
                let c_ok = t.create(64, 16, 2).is_ok();
                let x1 = t.exit(a, ga).is_ok();
                let x2 = t.exit(1, gb).is_ok();
                let (d, _gd, tp) = match t.create(64, 16, 4) {
                    Ok(v) => v,
                    Err(_) => (99u8, 0, 0),
                };
                let (e, _ge, _) = match t.create(64, 16, 4) {
                    Ok(v) => v,
                    Err(_) => (99u8, 0, 0),
                };
                c_ok && x1 && x2 && e == 1 && d == 0 && tp == 48 && t.blocks() == 3 && t.live() == 3
            },
        "static",
    );

    s.add(
        "EX6-重复退出闸门",
        l.sample(8).map(|x| x.code) == Some(EX_EDOUBLE)
            && {
                let mut t = Threads::new();
                let (tid, gen, _) = match t.create(64, 16, 2) {
                    Ok(v) => v,
                    Err(_) => (0u8, 0, 0),
                };
                t.exit(tid, gen).is_ok() && t.exit(tid, gen) == Err(EX_EDOUBLE)
            },
        "static",
    );

    s.add(
        "EX7-未知线程闸门",
        l.sample(9).map(|x| x.code) == Some(EX_EUNKNOWN)
            && {
                let mut t = Threads::new();
                t.exit(0, 1) == Err(EX_EUNKNOWN) && t.exit(200, 1) == Err(EX_EUNKNOWN)
            },
        "static",
    );

    s.add(
        "EX8-借用未解闸门",
        l.sample(10).map(|x| x.code) == Some(EX_EBUSY)
            && {
                let mut t = Threads::new();
                let (tid, gen, _) = match t.create(64, 16, 2) {
                    Ok(v) => v,
                    Err(_) => (0u8, 0, 0),
                };
                t.pin(tid).is_ok()
                    && t.exit(tid, gen) == Err(EX_EBUSY)
                    && t.unpin(tid).is_ok()
                    && t.exit(tid, gen) == Ok(2)
            },
        "static",
    );

    s.add(
        "EX9-槽满闸门",
        l.sample(11).map(|x| x.code) == Some(EX_EFULL)
            && {
                let mut t = Threads::new();
                let mut i = 0;
                let mut all = true;
                while i < SLOTS {
                    if t.create(64, 16, 1).is_err() {
                        all = false;
                    }
                    i += 1;
                }
                all && t.create(64, 16, 1) == Err(EX_EFULL)
            },
        "static"
    );

    s.add(
        "EX10-池尽闸门",
        l.sample(12).map(|x| x.code) == Some(EX_ENOMEM)
            && {
                let mut t = Threads::new();
                t.create(600, 16, 2).is_ok() && t.create(600, 16, 2) == Err(EX_ENOMEM)
            },
        "static",
    );

    s.add(
        "EX11-代号过期闸门",
        l.sample(13).map(|x| x.code) == Some(EX_ESTALE)
            && {
                let mut t = Threads::new();
                let (tid, gen, _) = match t.create(64, 16, 2) {
                    Ok(v) => v,
                    Err(_) => (0u8, 0, 0),
                };
                t.exit(tid, gen + 1) == Err(EX_ESTALE)
                    && t.slot(tid as usize).map(|s| s.state) == Some(ST_LIVE)
                    && t.exit(tid, gen) == Ok(2)
            },
        "static",
    );

    s.add(
        "EX12-千线程零泄漏与账目恒等",
        {
            let (live, used, reclaimed, gens) = stress(1000);
            let mut t = Threads::new();
            let mut ok = live == 0 && used == 0 && reclaimed == 3000 && gens == 1001;
            let mut i = 0;
            while i < 6 {
                let (tid, gen, _) = match t.create(96, 32, 2) {
                    Ok(v) => v,
                    Err(_) => break,
                };
                if t.live() != 1 || t.used() != 96 {
                    ok = false;
                }
                if t.exit(tid, gen) != Ok(2) || t.live() != 0 || t.used() != 0 {
                    ok = false;
                }
                i += 1;
            }
            ok && t.reclaimed() == 12 && t.gens() == 7 && t.blocks() == 1
        },
        "static",
    );

    let mut f = build_ledger();
    let rep = f.replay();
    let root = match f.freeze() {
        Ok(v) => v,
        Err(_) => 0,
    };
    let again = f.freeze() == Err(EX_EFROZEN);
    let inj = inject_four();
    s.add_dyn(
        "EX13-冻结重放与四路注入",
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
    fn create_and_exit_roundtrip() {
        let mut t = Threads::new();
        let (tid, gen, tp) = t.create(64, 16, 2).expect("c");
        assert_eq!(tp, 48);
        assert_eq!(t.used(), 64);
        assert_eq!(t.live(), 1);
        assert_eq!(t.exit(tid, gen), Ok(2));
        assert_eq!(t.used(), 0);
        assert_eq!(t.live(), 0);
        assert_eq!(t.reclaimed(), 2);
    }

    #[test]
    fn slot_reuse_after_death() {
        let mut t = Threads::new();
        let (a, ga, _) = t.create(64, 16, 2).expect("a");
        let _ = t.create(64, 16, 2).expect("b");
        let _ = t.create(64, 16, 2).expect("c");
        assert_eq!(t.create(64, 16, 2), Err(EX_EFULL));
        assert!(t.exit(a, ga).is_ok());
        let (b, _, tp) = t.create(64, 16, 3).expect("reuse");
        assert_eq!(a, b);
        assert_eq!(tp, 48);
        assert_eq!(t.slot(a as usize).map(|s| s.gen), Some(4));
    }

    #[test]
    fn exit_gates_are_ordered() {
        let mut t = Threads::new();
        assert_eq!(t.exit(0, 1), Err(EX_EUNKNOWN));
        let (tid, gen, _) = t.create(64, 16, 2).expect("c");
        assert_eq!(t.exit(tid, gen + 1), Err(EX_ESTALE));
        t.pin(tid).expect("pin");
        assert_eq!(t.exit(tid, gen), Err(EX_EBUSY));
        t.unpin(tid).expect("unpin");
        assert_eq!(t.exit(tid, gen), Ok(2));
        assert_eq!(t.exit(tid, gen), Err(EX_EDOUBLE));
    }

    #[test]
    fn table_full_and_pool_exhausted() {
        let mut t = Threads::new();
        let mut i = 0;
        while i < SLOTS {
            assert!(t.create(32, 16, 1).is_ok());
            i += 1;
        }
        assert_eq!(t.create(32, 16, 1), Err(EX_EFULL));
        let mut u = Threads::new();
        assert!(u.create(600, 16, 1).is_ok());
        assert_eq!(u.create(600, 16, 1), Err(EX_ENOMEM));
        assert_eq!(u.alloc_block(0, 16), Err(EX_ESIZE));
    }

    #[test]
    fn stress_is_leak_free() {
        let (live, used, reclaimed, gens) = stress(1000);
        assert_eq!((live, used, reclaimed, gens), (0, 0, 3000, 1001));
    }

    #[test]
    fn ledger_seals_and_replays() {
        let mut l = build_ledger();
        assert!(l.audit().is_ok());
        let root = l.freeze().expect("freeze");
        assert_eq!(l.replay(), Ok(root));
        assert_eq!(l.freeze(), Err(EX_EFROZEN));
        assert_eq!(l.rejects(), 1);
    }

    #[test]
    fn checks_pass() {
        let s = run_unx11312_checks();
        assert_eq!(s.len(), 13);
        assert!(s.all_passed(), "{}", s.red_items_text());
    }
}
