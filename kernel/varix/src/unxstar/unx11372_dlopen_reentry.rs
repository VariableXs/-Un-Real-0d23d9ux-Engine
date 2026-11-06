//! UNX-F11372 dlopen 线程安全与重入语义
//!
//! `dlopen` 必须可重入：同一线程递归取装载锁要能继续，跨线程装卸要互斥，
//! 读并发与写独占的升级必须显式拒绝。本模块把装载锁做成可判定的门：
//!
//! 1. **归属闸门**：写锁已有主时，其他线程一律 `EBUSY`；读锁可由多线程
//!    共享，但写者进场前必须无读者。
//! 2. **升级闸门**：单读者原地升级为写者会与自身的读锁互锁，必须显式
//!    `EUPGRADE`，先放读再取写。
//! 3. **重入闸门**：同一写者的递归深度有上限，超过上限即 `ERECUR`，防止
//!    递归装载把栈吃穿。
//!
//! 闸门次序：
//! - `acquire`：线程号非法 → 写者递归深度 → 他线程持锁 → 读升写 → 读者
//!   名额；
//! - `release`：线程号非法 → 非持有者（含深度已归零）；
//! - `wait`：线程号非法 → 重复等待 → 当前无持有者 → 等待表满；
//! - `finish`：仍有持有者或等待者。

use crate::alloc::format;
use crate::checks::CheckSet;

/// 段基址。
pub const SEG_BASE: u32 = 0xed40;

/// 线程号非法。
pub const TS_EINVAL: u32 = SEG_BASE + 0x01;
/// 他线程持有写锁。
pub const TS_EBUSY: u32 = SEG_BASE + 0x02;
/// 读锁原地升级为写锁。
pub const TS_EUPGRADE: u32 = SEG_BASE + 0x03;
/// 递归深度超限。
pub const TS_ERECUR: u32 = SEG_BASE + 0x04;
/// 读者名额已满。
pub const TS_EFULL: u32 = SEG_BASE + 0x07;
/// 释放者不是持有者。
pub const TS_EORDER: u32 = SEG_BASE + 0x05;
/// 收工时仍有持有者。
pub const TS_ESTUCK: u32 = SEG_BASE + 0x06;
/// 同一线程重复排队。
pub const TS_EDUP: u32 = SEG_BASE + 0x08;
/// 账本形状不符。
pub const TS_EBAL: u32 = SEG_BASE + 0x09;
/// 槽位乱序。
pub const TS_ESLOT: u32 = SEG_BASE + 0x0a;
/// 观测值与复算不符。
pub const TS_EVALUE: u32 = SEG_BASE + 0x0b;
/// 冻结后重放不一致。
pub const TS_ESEAL: u32 = SEG_BASE + 0x0c;
/// 观察下标越界。
pub const TS_ENOBS: u32 = SEG_BASE + 0x0d;

/// 读者/等待者名额。
pub const CAP: usize = 4;
/// 写者递归深度上限。
pub const DEPTH_MAX: u32 = 4;
/// 线程号上界。
pub const TID_MAX: u32 = 4096;
/// 账本格数。
pub const SAMPLES: usize = 14;

/// 操作码：取锁。
pub const OP_ACQ: u8 = 1;
/// 操作码：放锁。
pub const OP_REL: u8 = 2;
/// 操作码：排队等待。
pub const OP_WAIT: u8 = 3;
/// 操作码：宣称干净收工。
pub const OP_FINISH: u8 = 4;

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

/// 装载锁的可重入门。
#[derive(Clone, Copy, Debug)]
pub struct Gate {
    writer: u32,
    depth: u32,
    readers: [u32; CAP],
    nr: usize,
    waiters: [u32; CAP],
    nw: usize,
    contends: u32,
}

impl Gate {
    /// 无主门。
    pub const fn new() -> Gate {
        Gate {
            writer: 0,
            depth: 0,
            readers: [0; CAP],
            nr: 0,
            waiters: [0; CAP],
            nw: 0,
            contends: 0,
        }
    }

    /// 当前写者（0 表示无）。
    pub fn writer(&self) -> u32 {
        self.writer
    }

    /// 写者递归深度。
    pub fn depth(&self) -> u32 {
        self.depth
    }

    /// 读者数。
    pub fn reader_count(&self) -> usize {
        self.nr
    }

    /// 等待者数。
    pub fn waiter_count(&self) -> usize {
        self.nw
    }

    /// 被拒的跨线程争用次数。
    pub fn contends(&self) -> u32 {
        self.contends
    }

    fn is_reader(&self, tid: u32) -> Option<usize> {
        let mut i = 0usize;
        while i < self.nr {
            match self.readers.get(i) {
                Some(v) => {
                    if *v == tid {
                        return Some(i);
                    }
                }
                None => return None,
            }
            i += 1;
        }
        None
    }

    fn is_waiting(&self, tid: u32) -> bool {
        let mut i = 0usize;
        while i < self.nw {
            match self.waiters.get(i) {
                Some(v) => {
                    if *v == tid {
                        return true;
                    }
                }
                None => return false,
            }
            i += 1;
        }
        false
    }

    /// 取锁：`write` 为真取写锁，否则取读锁。
    pub fn acquire(&mut self, tid: u32, write: bool) -> Result<u32, u32> {
        if tid == 0 || tid > TID_MAX {
            return Err(TS_EINVAL);
        }
        if self.writer == tid {
            if self.depth >= DEPTH_MAX {
                return Err(TS_ERECUR);
            }
            self.depth = self.depth.saturating_add(1);
            return Ok(self.depth);
        }
        if self.writer != 0 {
            self.contends = self.contends.saturating_add(1);
            return Err(TS_EBUSY);
        }
        if write {
            if self.nr > 0 {
                self.contends = self.contends.saturating_add(1);
                if self.nr == 1 {
                    match self.readers.get(0) {
                        Some(v) => {
                            if *v == tid {
                                return Err(TS_EUPGRADE);
                            }
                        }
                        None => return Err(TS_EBUSY),
                    }
                }
                return Err(TS_EBUSY);
            }
            self.writer = tid;
            self.depth = 1;
            return Ok(1);
        }
        if self.is_reader(tid).is_some() {
            return Ok(self.nr as u32);
        }
        if self.nr >= CAP {
            return Err(TS_EFULL);
        }
        if let Some(slot) = self.readers.get_mut(self.nr) {
            *slot = tid;
        }
        self.nr += 1;
        Ok(self.nr as u32)
    }

    /// 放锁：写者按深度回退，读者摘除自己的名额。
    pub fn release(&mut self, tid: u32) -> Result<u32, u32> {
        if tid == 0 || tid > TID_MAX {
            return Err(TS_EINVAL);
        }
        if self.writer == tid {
            if self.depth == 0 {
                return Err(TS_EORDER);
            }
            self.depth -= 1;
            if self.depth == 0 {
                self.writer = 0;
            }
            return Ok(self.depth);
        }
        match self.is_reader(tid) {
            Some(at) => {
                let mut i = at;
                while i + 1 < self.nr {
                    let next = match self.readers.get(i + 1) {
                        Some(v) => *v,
                        None => 0,
                    };
                    if let Some(slot) = self.readers.get_mut(i) {
                        *slot = next;
                    }
                    i += 1;
                }
                self.nr -= 1;
                Ok(self.nr as u32)
            }
            None => Err(TS_EORDER),
        }
    }

    /// 排队等待：只有当前确有持有者时才允许排队。
    pub fn wait(&mut self, tid: u32) -> Result<u32, u32> {
        if tid == 0 || tid > TID_MAX {
            return Err(TS_EINVAL);
        }
        if self.is_waiting(tid) {
            return Err(TS_EDUP);
        }
        if self.writer == 0 && self.nr == 0 {
            return Err(TS_EORDER);
        }
        if self.nw >= CAP {
            return Err(TS_EFULL);
        }
        if let Some(slot) = self.waiters.get_mut(self.nw) {
            *slot = tid;
        }
        self.nw += 1;
        Ok(self.nw as u32)
    }

    /// 宣称干净收工：不许有持有者或等待者残留。
    pub fn finish(&self) -> Result<u32, u32> {
        if self.writer != 0 || self.nr != 0 || self.nw != 0 {
            return Err(TS_ESTUCK);
        }
        Ok(0)
    }

    /// 指纹：只覆盖持有关系，不含争用计数。
    pub fn seal(&self) -> u64 {
        let mut h = FNV_OFF;
        h = fold(h, self.writer as u64);
        h = fold(h, self.depth as u64);
        let mut i = 0usize;
        while i < self.nr {
            if let Some(v) = self.readers.get(i) {
                h = fold(h, *v as u64);
            }
            i += 1;
        }
        let mut j = 0usize;
        while j < self.nw {
            if let Some(v) = self.waiters.get(j) {
                h = fold(h, *v as u64);
            }
            j += 1;
        }
        h
    }

    /// 不变式自检。
    pub fn consistent(&self) -> bool {
        if self.nr > CAP || self.nw > CAP || self.depth > DEPTH_MAX {
            return false;
        }
        if (self.writer == 0) != (self.depth == 0) {
            return false;
        }
        if self.writer != 0 {
            if self.writer > TID_MAX {
                return false;
            }
            if self.nr != 0 {
                return false;
            }
        }
        let mut i = 0usize;
        while i < self.nr {
            match self.readers.get(i) {
                Some(v) => {
                    if *v == 0 || *v > TID_MAX || *v == self.writer {
                        return false;
                    }
                    let mut j = 0usize;
                    while j < i {
                        match self.readers.get(j) {
                            Some(p) => {
                                if p == v {
                                    return false;
                                }
                            }
                            None => return false,
                        }
                        j += 1;
                    }
                }
                None => return false,
            }
            i += 1;
        }
        let mut k = 0usize;
        while k < self.nw {
            match self.waiters.get(k) {
                Some(v) => {
                    if *v == 0 || *v > TID_MAX {
                        return false;
                    }
                    let mut m = 0usize;
                    while m < k {
                        match self.waiters.get(m) {
                            Some(p) => {
                                if p == v {
                                    return false;
                                }
                            }
                            None => return false,
                        }
                        m += 1;
                    }
                }
                None => return false,
            }
            k += 1;
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
            return Err(TS_ESLOT);
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
            return Err(TS_EBAL);
        }
        const VALS: [u32; 8] = [1, 2, 3, 2, 1, 0, 1, 2];
        const CODES: [u32; 6] = [TS_EBUSY, TS_EINVAL, TS_EUPGRADE, TS_EFULL, TS_EORDER, TS_ESTUCK];
        let mut vi = 0usize;
        let mut ci = 0usize;
        let mut i = 0usize;
        while i < self.len {
            let s = match self.rows.get(i) {
                Some(v) => *v,
                None => return Err(TS_EBAL),
            };
            if s.slot as usize != i {
                return Err(TS_ESLOT);
            }
            if s.ok {
                if s.code != 0 {
                    return Err(TS_EVALUE);
                }
                match VALS.get(vi) {
                    Some(w) => {
                        if s.value != *w {
                            return Err(TS_EVALUE);
                        }
                    }
                    None => return Err(TS_EBAL),
                }
                vi += 1;
            } else {
                match CODES.get(ci) {
                    Some(w) => {
                        if s.code != *w {
                            return Err(TS_EVALUE);
                        }
                    }
                    None => return Err(TS_EBAL),
                }
                ci += 1;
            }
            i += 1;
        }
        if vi != 8 || ci != 6 {
            return Err(TS_EBAL);
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
            return Err(TS_EBAL);
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
            Err(TS_ESEAL)
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

/// 十四格账本：递归取写 → 争用/递归读 → 跨线程拒/线程号拒 → 读共享 →
/// 读升写拒 → 读者满/非主释放/收工滞留。
pub fn build_ledger() -> Ledger {
    let mut l = Ledger::new();
    let mut g = Gate::new();

    let _ = l.register(word_cell(0, OP_ACQ, g.acquire(1, true)));
    let _ = l.register(word_cell(1, OP_ACQ, g.acquire(1, true)));
    let _ = l.register(word_cell(2, OP_ACQ, g.acquire(1, false)));
    let _ = l.register(word_cell(3, OP_REL, g.release(1)));
    let _ = l.register(word_cell(4, OP_ACQ, g.acquire(2, false)));
    let _ = l.register(word_cell(5, OP_ACQ, g.acquire(0, true)));
    let _ = l.register(word_cell(6, OP_REL, g.release(1)));
    let _ = l.register(word_cell(7, OP_REL, g.release(1)));
    let _ = l.register(word_cell(8, OP_ACQ, g.acquire(2, false)));
    let _ = l.register(word_cell(9, OP_ACQ, g.acquire(2, true)));
    let _ = l.register(word_cell(10, OP_ACQ, g.acquire(3, false)));

    let _ = g.acquire(4, false);
    let _ = g.acquire(5, false);
    let _ = l.register(word_cell(11, OP_ACQ, g.acquire(6, false)));
    let _ = l.register(word_cell(12, OP_REL, g.release(7)));
    let _ = l.register(word_cell(13, OP_FINISH, g.finish()));

    l
}

/// 四路注入。
pub fn inject_four() -> [bool; 4] {
    let mut out = [false; 4];

    let l = build_ledger();
    let rep = l.replay();
    let root = l.seal();
    out[0] = l.len() == SAMPLES && l.audit().is_ok() && rep == Ok(root) && root != 0;

    // 一路：写者递归、放锁归零、读者共享。
    let mut a = Gate::new();
    let r1 = a.acquire(1, true);
    let r2 = a.acquire(1, true);
    let r3 = a.acquire(1, false);
    let x1 = a.release(1);
    let x2 = a.release(1);
    let x3 = a.release(1);
    let r4 = a.acquire(2, false);
    let r5 = a.acquire(3, false);
    out[1] = r1 == Ok(1)
        && r2 == Ok(2)
        && r3 == Ok(3)
        && x1 == Ok(2)
        && x2 == Ok(1)
        && x3 == Ok(0)
        && r4 == Ok(1)
        && r5 == Ok(2)
        && a.writer() == 0
        && a.depth() == 0
        && a.reader_count() == 2
        && a.consistent();

    // 二路：八类失败各自可辨，且失败不改持有关系。
    let mut b = Gate::new();
    let zero = b.acquire(0, true) == Err(TS_EINVAL);
    let idle_wait = b.wait(2) == Err(TS_EORDER);
    let clean_start = b.seal() == Gate::new().seal();
    let w = b.acquire(1, true) == Ok(1);
    let h1 = b.seal();
    let busy = b.acquire(2, true) == Err(TS_EBUSY) && b.seal() == h1;
    let _ = b.release(1);
    let read = b.acquire(3, false) == Ok(1);
    let h2 = b.seal();
    let up = b.acquire(3, true) == Err(TS_EUPGRADE) && b.seal() == h2;
    let _ = b.release(3);
    let w2 = b.acquire(4, true) == Ok(1);
    let h3 = b.seal();
    let mut d = 1u32;
    let mut rec = true;
    while d < DEPTH_MAX {
        if b.acquire(4, true) != Ok(d + 1) {
            rec = false;
        }
        d += 1;
    }
    let recur = b.acquire(4, true) == Err(TS_ERECUR) && b.seal() != h3;
    let h4 = b.seal();
    let mut k = 0u32;
    while k < DEPTH_MAX {
        let _ = b.release(4);
        k += 1;
    }
    let stuck = b.finish() == Ok(0);
    let mut c = Gate::new();
    let _ = c.acquire(9, false);
    let _ = c.wait(9);
    let hc = c.seal();
    let dup = c.wait(9) == Err(TS_EDUP) && c.seal() == hc;
    out[2] = zero
        && idle_wait
        && clean_start
        && w
        && busy
        && read
        && up
        && w2
        && rec
        && recur
        && h4 != 0
        && stuck
        && dup
        && b.consistent()
        && c.consistent();

    // 三路：读者与等待者名额都是硬上限。
    let mut q = Gate::new();
    let mut i = 1u32;
    while (i as usize) <= CAP {
        let _ = q.acquire(i, false);
        i += 1;
    }
    let q1 = q.reader_count() == CAP;
    let q2 = q.acquire((CAP + 1) as u32, false) == Err(TS_EFULL);
    let mut w = 1u32;
    while (w as usize) <= CAP {
        let _ = q.wait(w + 100);
        w += 1;
    }
    let q3 = q.waiter_count() == CAP;
    let q4 = q.wait(999) == Err(TS_EFULL);
    let mut j = 1u32;
    while (j as usize) <= CAP {
        let _ = q.release(j);
        j += 1;
    }
    let q5 = q.finish() == Err(TS_ESTUCK);
    out[3] = q1 && q2 && q3 && q4 && q5 && q.consistent();

    out
}

/// 十三项判据。
pub fn run_unx11372_checks() -> CheckSet {
    let mut s = CheckSet::new("unx11372.dlopen_reentry");
    let l = build_ledger();

    s.add_dyn(
        "TS1-十四格八允六拒",
        l.len() == SAMPLES && l.ok_count() == 8 && l.err_count() == 6 && l.audit().is_ok(),
        &format!("n={} ok={} err={}", l.len(), l.ok_count(), l.err_count()),
    );

    s.add(
        "TS2-同线程递归取写",
        {
            let mut a = Gate::new();
            a.acquire(1, true) == Ok(1)
                && a.acquire(1, true) == Ok(2)
                && a.acquire(1, false) == Ok(3)
                && a.depth() == 3
                && a.writer() == 1
                && a.consistent()
        },
        "static",
    );

    s.add(
        "TS3-写独占跨线程拒",
        {
            let mut a = Gate::new();
            let _ = a.acquire(1, true);
            let no = a.acquire(2, false) == Err(TS_EBUSY);
            let no_w = a.acquire(2, true) == Err(TS_EBUSY);
            no && no_w
                && a.contends() == 2
                && a.reader_count() == 0
                && a.writer() == 1
                && l.sample(4).map(|x| x.code) == Some(TS_EBUSY)
        },
        "static",
    );

    s.add(
        "TS4-读升写显式拒",
        {
            let mut a = Gate::new();
            let _ = a.acquire(1, false);
            a.acquire(1, true) == Err(TS_EUPGRADE)
                && a.depth() == 0
                && a.reader_count() == 1
                && a.consistent()
                && l.sample(9).map(|x| x.code) == Some(TS_EUPGRADE)
        },
        "static",
    );

    s.add(
        "TS5-读者共享并发",
        {
            let mut a = Gate::new();
            let r1 = a.acquire(1, false);
            let r2 = a.acquire(2, false);
            let r3 = a.acquire(2, false);
            r1 == Ok(1)
                && r2 == Ok(2)
                && r3 == Ok(2)
                && a.reader_count() == 2
                && a.release(1) == Ok(1)
                && a.release(2) == Ok(0)
                && a.finish() == Ok(0)
                && a.consistent()
        },
        "static",
    );

    s.add(
        "TS6-递归深度上限",
        {
            let mut a = Gate::new();
            let mut i = 1u32;
            let mut ok = true;
            while i <= DEPTH_MAX {
                if a.acquire(1, true) != Ok(i) {
                    ok = false;
                }
                i += 1;
            }
            let over = a.acquire(1, true) == Err(TS_ERECUR);
            let mut k = DEPTH_MAX;
            while k > 0 {
                let _ = a.release(1);
                k -= 1;
            }
            ok && over && a.depth() == 0 && a.writer() == 0 && a.consistent()
        },
        "static"
    );

    s.add(
        "TS7-非主释放被拒",
        {
            let mut a = Gate::new();
            let _ = a.acquire(1, true);
            let bad = a.release(2) == Err(TS_EORDER);
            let zero = a.release(0) == Err(TS_EINVAL);
            bad && zero
                && a.writer() == 1
                && a.depth() == 1
                && l.sample(12).map(|x| x.code) == Some(TS_EORDER)
        },
        "static",
    );

    s.add(
        "TS8-等待表去重",
        {
            let mut a = Gate::new();
            let early = a.wait(3) == Err(TS_EORDER);
            let _ = a.acquire(1, true);
            let w1 = a.wait(3) == Ok(1);
            let dup = a.wait(3) == Err(TS_EDUP);
            let w2 = a.wait(4) == Ok(2);
            early && w1 && dup && w2 && a.waiter_count() == 2 && a.consistent()
        },
        "static",
    );

    s.add(
        "TS9-读者名额上限",
        {
            let mut a = Gate::new();
            let mut i = 1u32;
            while (i as usize) <= CAP {
                let _ = a.acquire(i, false);
                i += 1;
            }
            a.reader_count() == CAP
                && a.acquire((CAP + 1) as u32, false) == Err(TS_EFULL)
                && a.consistent()
                && l.sample(11).map(|x| x.code) == Some(TS_EFULL)
        },
        "static",
    );

    s.add(
        "TS10-收工滞留被拒",
        {
            let mut a = Gate::new();
            let clean = a.finish() == Ok(0);
            let _ = a.acquire(1, false);
            let stuck = a.finish() == Err(TS_ESTUCK);
            clean && stuck && a.release(1) == Ok(0) && a.finish() == Ok(0)
        },
        "static"
    );

    s.add(
        "TS11-线程号非法被拒",
        {
            let mut a = Gate::new();
            matches!(a.acquire(0, true), Err(TS_EINVAL))
                && a.acquire(TID_MAX + 1, false) == Err(TS_EINVAL)
                && a.release(0) == Err(TS_EINVAL)
                && a.wait(0) == Err(TS_EINVAL)
                && a.writer() == 0
                && a.reader_count() == 0
                && a.consistent()
        },
        "static",
    );

    s.add(
        "TS12-竞争失败不改归属",
        {
            let mut a = Gate::new();
            let _ = a.acquire(1, true);
            let _ = a.acquire(1, true);
            let d = a.depth();
            let _ = a.acquire(2, false);
            let _ = a.acquire(2, true);
            let _ = a.release(2);
            let _ = a.wait(0);
            a.writer() == 1
                && a.reader_count() == 0
                && a.waiter_count() == 0
                && a.depth() == d
                && a.finish() == Err(TS_ESTUCK)
                && a.consistent()
        },
        "static"
    );

    s.add(
        "TS13-冻结重放四注入",
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
        assert_eq!(l.rejects(), 0);
        assert!(l.audit().is_ok());
        let mut i = 0usize;
        while i < l.len() {
            assert_eq!(l.sample(i).map(|x| x.slot as usize), Some(i));
            i += 1;
        }
    }

    #[test]
    fn checks_pass() {
        let s = run_unx11372_checks();
        assert_eq!(s.len(), 13);
        assert!(s.all_passed());
    }

    #[test]
    fn writer_is_exclusive() {
        let mut g = Gate::new();
        assert_eq!(g.acquire(1, true), Ok(1));
        assert_eq!(g.acquire(2, false), Err(TS_EBUSY));
        assert_eq!(g.acquire(2, true), Err(TS_EBUSY));
        assert_eq!(g.release(1), Ok(0));
        assert_eq!(g.acquire(2, false), Ok(1));
        assert_eq!(g.finish(), Err(TS_ESTUCK));
    }

    #[test]
    fn recursive_depth_capped() {
        let mut g = Gate::new();
        let mut i = 1u32;
        while i <= DEPTH_MAX {
            assert_eq!(g.acquire(7, true), Ok(i));
            i += 1;
        }
        assert_eq!(g.acquire(7, true), Err(TS_ERECUR));
        assert_eq!(g.depth(), DEPTH_MAX);
    }

    #[test]
    fn upgrade_needs_single_reader() {
        let mut g = Gate::new();
        assert_eq!(g.acquire(1, false), Ok(1));
        assert_eq!(g.acquire(2, false), Ok(2));
        assert_eq!(g.acquire(1, true), Err(TS_EBUSY));
        assert_eq!(g.release(2), Ok(1));
        assert_eq!(g.acquire(1, true), Err(TS_EUPGRADE));
    }

    #[test]
    fn wait_queue_dedup_and_limit() {
        let mut g = Gate::new();
        assert_eq!(g.wait(1), Err(TS_EORDER));
        assert_eq!(g.acquire(9, false), Ok(1));
        let mut i = 1u32;
        while (i as usize) <= CAP {
            assert_eq!(g.wait(i), Ok(i));
            i += 1;
        }
        assert_eq!(g.wait(1), Err(TS_EDUP));
        assert_eq!(g.wait(99), Err(TS_EFULL));
        assert!(g.consistent());
    }

    #[test]
    fn inject_four_holds() {
        assert_eq!(inject_four(), [true; 4]);
    }
}
