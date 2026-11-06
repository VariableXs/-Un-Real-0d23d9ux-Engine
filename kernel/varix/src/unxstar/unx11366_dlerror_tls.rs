//! UNX-F11366 dlerror 报文语义：线程局部错误串与一次性消费（0xece0 段）
//!
//! `dlerror` 是动态链接器里唯一「有状态」的查询接口：失败信息被记在调用线程的
//! **线程局部**槽位里，第一次 `dlerror()` 取走它并清空，第二次返回 NULL。
//! 这条一次性的语义是 POSIX 明文要求的，也是无数程序写出如下循环的原因：
//!
//! ```text
//! while ((e = dlerror()) != NULL) { fprintf(stderr, "%s\n", e); }
//! ```
//!
//! 本模块把这条语义拆成可观测的槽位表，并守住三条边界：
//!
//! 1. **一次性**：`take` 成功后槽位转入「已消费」态，再次 `take` 必须报
//!    `ECLEARED`，而不是 `ENOSLOT`。两者都表现为 NULL，但前者是程序重复取用
//!    （常见于把 dlerror 当轮询用），后者是「这个线程从来没出过错」，诊断不同。
//! 2. **线程隔离**：槽位以线程号为主键，A 线程的错误对 B 线程不可见；即便有人
//!    拿着别人的线程号来取，也必须先过 `EISOL` 的跨线程闸门，而不是静默读到
//!    别人的报文。glibc 用 TLS 天然做到这一点，这里把闸门显式化。
//! 3. **覆盖而非累积**：同一线程连续两次失败，第二次覆盖第一次——glibc 不会
//!    排出一条错误队列。因此槽位数恒定，`pending` 只为 0 或 1。
//!
//! 还有一条容易被忽略的细节：错误串里记录的**装载序号**必须单调。若某次报告
//! 携带的序号比槽位里已有的更旧，说明调用方在重放一段过期的装载上下文，必须
//! 以 `ESEQ` 拒收，否则「后发生的错误」会被「先发生的旧错误」覆盖。
//!
//! 闸门次序（报告）：空线程号 → 原因类越界 → 序号倒退 → 表满 → 发布/覆盖。
//! 闸门次序（取用）：空线程号 → 槽位不存在 → 跨线程 → 已消费 → 交付。
//! 之所以把「跨线程」放在「已消费」之前：跨线程访问本身就是协议违规，不应因为
//! 槽位恰好已清空而变成一次看似正常的空读。

use crate::alloc::format;
use crate::checks::CheckSet;

/// 段基址。
pub const SEG_BASE: u32 = 0xece0;

/// 线程号为零。
pub const DLE_EINVAL: u32 = SEG_BASE + 0x01;
/// 原因类不在 1..=7。
pub const DLE_EBADREASON: u32 = SEG_BASE + 0x02;
/// 该线程从未报告过错。
pub const DLE_ENOSLOT: u32 = SEG_BASE + 0x03;
/// 槽位已被消费，报文不再存在。
pub const DLE_ECLEARED: u32 = SEG_BASE + 0x04;
/// 请求者与持有者不是同一线程。
pub const DLE_EISOL: u32 = SEG_BASE + 0x05;
/// 线程局部槽位表已满。
pub const DLE_EFULL: u32 = SEG_BASE + 0x06;
/// 报告序号比槽位里已有的更旧。
pub const DLE_ESEQ: u32 = SEG_BASE + 0x07;
/// 报文下标越界。
pub const DLE_EBADKIND: u32 = SEG_BASE + 0x08;
/// 账本形状不符。
pub const DLE_EBAL: u32 = SEG_BASE + 0x09;
/// 槽位乱序。
pub const DLE_EORDER: u32 = SEG_BASE + 0x0a;
/// 观测值与复算不符。
pub const DLE_EMISMATCH: u32 = SEG_BASE + 0x0b;
/// 冻结后重放不一致。
pub const DLE_ESEAL: u32 = SEG_BASE + 0x0c;
/// 观察下标越界。
pub const DLE_ENOBS: u32 = SEG_BASE + 0x0d;

/// 线程槽位容量。
pub const CAP: usize = 6;
/// 原因类上限（含）。
pub const REASON_MAX: u8 = 7;
/// 账本格数。
pub const SAMPLES: usize = 14;

/// 操作码：报告一次装载失败。
pub const OP_REPORT: u8 = 1;
/// 操作码：取走错误串。
pub const OP_TAKE: u8 = 2;
/// 操作码：查看是否有待取报文（不清）。
pub const OP_PENDING: u8 = 3;
/// 操作码：以指定请求者身份取用。
pub const OP_TAKE_AS: u8 = 4;
/// 操作码：渲染报文原因。
pub const OP_RENDER: u8 = 5;

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

/// 一个线程的报文槽位。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Slot {
    /// 所属线程号。
    pub tid: u16,
    /// 原因类。
    pub reason: u8,
    /// 装载序号。
    pub seq: u32,
    /// 是否还待取。
    pub pending: bool,
}

impl Slot {
    /// 空槽。
    pub const fn empty() -> Slot {
        Slot { tid: 0, reason: 0, seq: 0, pending: false }
    }
}

/// 线程局部槽位表。
#[derive(Clone, Copy, Debug)]
pub struct Tls {
    slots: [Slot; CAP],
    n: usize,
    reports: u32,
    takes: u32,
    cleared: u32,
    isol: u32,
}

impl Tls {
    /// 空表。
    pub const fn new() -> Tls {
        Tls {
            slots: [Slot::empty(); CAP],
            n: 0,
            reports: 0,
            takes: 0,
            cleared: 0,
            isol: 0,
        }
    }

    /// 定位线程槽位。
    fn find(&self, tid: u16) -> Option<usize> {
        let mut i = 0usize;
        while i < self.n {
            if let Some(s) = self.slots.get(i) {
                if s.tid == tid {
                    return Some(i);
                }
            }
            i += 1;
        }
        None
    }

    /// 待取报文数（0 或 1）。
    fn pending_total(&self) -> usize {
        let mut k = 0usize;
        let mut i = 0usize;
        while i < self.n {
            if let Some(s) = self.slots.get(i) {
                if s.pending {
                    k += 1;
                }
            }
            i += 1;
        }
        k
    }

    /// 报告一次失败。
    pub fn report(&mut self, tid: u16, reason: u8, seq: u32) -> Result<usize, u32> {
        if tid == 0 {
            return Err(DLE_EINVAL);
        }
        if reason == 0 || reason > REASON_MAX {
            return Err(DLE_EBADREASON);
        }
        match self.find(tid) {
            Some(i) => {
                let old = match self.slots.get(i) {
                    Some(v) => *v,
                    None => return Err(DLE_ENOSLOT),
                };
                if seq < old.seq {
                    return Err(DLE_ESEQ);
                }
                if let Some(dst) = self.slots.get_mut(i) {
                    dst.reason = reason;
                    dst.seq = seq;
                    dst.pending = true;
                }
            }
            None => {
                if self.n >= CAP {
                    return Err(DLE_EFULL);
                }
                if let Some(dst) = self.slots.get_mut(self.n) {
                    *dst = Slot { tid, reason, seq, pending: true };
                }
                self.n += 1;
            }
        }
        self.reports = self.reports.saturating_add(1);
        Ok(self.pending_total())
    }

    /// 取走本线程的报文。
    pub fn take(&mut self, tid: u16) -> Result<u32, u32> {
        if tid == 0 {
            return Err(DLE_EINVAL);
        }
        let i = match self.find(tid) {
            Some(v) => v,
            None => return Err(DLE_ENOSLOT),
        };
        let s = match self.slots.get(i) {
            Some(v) => *v,
            None => return Err(DLE_ENOSLOT),
        };
        if !s.pending {
            self.cleared = self.cleared.saturating_add(1);
            return Err(DLE_ECLEARED);
        }
        if let Some(dst) = self.slots.get_mut(i) {
            dst.pending = false;
        }
        self.takes = self.takes.saturating_add(1);
        Ok(s.reason as u32)
    }

    /// 以 `who` 的身份取 `owner` 的报文：跨线程直接拒。
    pub fn take_as(&mut self, who: u16, owner: u16) -> Result<u32, u32> {
        if who == 0 || owner == 0 {
            return Err(DLE_EINVAL);
        }
        if who != owner {
            self.isol = self.isol.saturating_add(1);
            return Err(DLE_EISOL);
        }
        self.take(owner)
    }

    /// 查看待取状态（不清）。
    pub fn pending(&self, tid: u16) -> Result<usize, u32> {
        if tid == 0 {
            return Err(DLE_EINVAL);
        }
        let i = match self.find(tid) {
            Some(v) => v,
            None => return Err(DLE_ENOSLOT),
        };
        match self.slots.get(i) {
            Some(s) => Ok(if s.pending { 1 } else { 0 }),
            None => Err(DLE_ENOSLOT),
        }
    }

    /// 渲染某槽位的报文原因（相当于读错误串）。
    pub fn render(&self, idx: usize) -> Result<u32, u32> {
        if idx >= self.n {
            return Err(DLE_EBADKIND);
        }
        match self.slots.get(idx) {
            Some(s) => Ok(s.reason as u32),
            None => Err(DLE_EBADKIND),
        }
    }

    /// 槽位数。
    pub fn len(&self) -> usize {
        self.n
    }

    /// 是否空表。
    pub fn is_empty(&self) -> bool {
        self.n == 0
    }

    /// 报告次数。
    pub fn reports(&self) -> u32 {
        self.reports
    }

    /// 取用次数。
    pub fn takes(&self) -> u32 {
        self.takes
    }

    /// 已消费后再次取用的次数。
    pub fn cleared(&self) -> u32 {
        self.cleared
    }

    /// 跨线程访问次数。
    pub fn isolations(&self) -> u32 {
        self.isol
    }

    /// 线程隔离是否成立：除 `tid` 外无人待取。
    pub fn isolated(&self, tid: u16) -> bool {
        let mut i = 0usize;
        while i < self.n {
            if let Some(s) = self.slots.get(i) {
                if s.tid != tid && s.pending {
                    return false;
                }
            }
            i += 1;
        }
        true
    }

    /// 表摘要。
    pub fn digest(&self) -> u64 {
        let mut h = FNV_OFF;
        let mut i = 0usize;
        while i < self.n {
            if let Some(s) = self.slots.get(i) {
                h = fold(h, s.tid as u64);
                h = fold(h, s.reason as u64);
                h = fold(h, s.seq as u64);
                h = fold(h, s.pending as u64);
            }
            i += 1;
        }
        fold(h, self.reports as u64)
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
            return Err(DLE_EORDER);
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
            return Err(DLE_EBAL);
        }
        const VALS: [u32; 8] = [1, 2, 3, 5, 0, 1, 6, 0];
        const CODES: [u32; 6] = [
            DLE_ECLEARED,
            DLE_ENOSLOT,
            DLE_EINVAL,
            DLE_EBADREASON,
            DLE_ESEQ,
            DLE_EFULL,
        ];
        let mut vi = 0usize;
        let mut ci = 0usize;
        let mut i = 0usize;
        while i < self.len {
            let s = match self.rows.get(i) {
                Some(v) => *v,
                None => return Err(DLE_EBAL),
            };
            if s.slot as usize != i {
                return Err(DLE_EORDER);
            }
            if s.ok {
                if s.code != 0 {
                    return Err(DLE_EMISMATCH);
                }
                match VALS.get(vi) {
                    Some(w) => {
                        if s.value != *w {
                            return Err(DLE_EMISMATCH);
                        }
                    }
                    None => return Err(DLE_EBAL),
                }
                vi += 1;
            } else {
                match CODES.get(ci) {
                    Some(w) => {
                        if s.code != *w {
                            return Err(DLE_EMISMATCH);
                        }
                    }
                    None => return Err(DLE_EBAL),
                }
                ci += 1;
            }
            i += 1;
        }
        if vi != 8 || ci != 6 {
            return Err(DLE_EBAL);
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
            return Err(DLE_EBAL);
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
            Err(DLE_ESEAL)
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

/// 十四格账本：一报一取，重复取用、未知线程、跨线程与表满各据其位。
pub fn build_ledger() -> Ledger {
    let mut l = Ledger::new();
    let mut t = Tls::new();

    let _ = l.register(cell(0, OP_REPORT, t.report(1, 3, 10)));
    let _ = l.register(cell(1, OP_REPORT, t.report(2, 5, 11)));
    let _ = l.register(word_cell(2, OP_TAKE, t.take(1)));
    let _ = l.register(word_cell(3, OP_TAKE, t.take(2)));
    let _ = l.register(cell(4, OP_PENDING, t.pending(2)));
    let _ = l.register(cell(5, OP_REPORT, t.report(2, 6, 12)));
    let _ = l.register(word_cell(6, OP_TAKE_AS, t.take_as(2, 2)));
    let _ = l.register(cell(7, OP_PENDING, t.pending(2)));

    let _ = l.register(word_cell(8, OP_TAKE, t.take(1)));

    let _ = l.register(word_cell(9, OP_TAKE, t.take(3)));

    let _ = l.register(cell(10, OP_REPORT, t.report(0, 1, 1)));

    let _ = l.register(cell(11, OP_REPORT, t.report(1, 9, 20)));

    let _ = l.register(cell(12, OP_REPORT, t.report(1, 2, 5)));

    let _ = t.report(10, 1, 30);
    let _ = t.report(11, 2, 31);
    let _ = t.report(12, 3, 32);
    let _ = t.report(13, 4, 33);
    let _ = l.register(cell(13, OP_REPORT, t.report(0x99, 1, 40)));

    l
}

/// 四路注入。
pub fn inject_four() -> [bool; 4] {
    let mut out = [false; 4];

    let l = build_ledger();
    let rep = l.replay();
    let root = l.seal();
    out[0] = l.len() == SAMPLES && l.audit().is_ok() && rep == Ok(root) && root != 0;

    // 一路：报一次、取一次、取空、隔离。
    let mut a = Tls::new();
    let r1 = a.report(4, 2, 1);
    let p1 = a.pending(4);
    let t1 = a.take(4);
    let p2 = a.pending(4);
    let t2 = a.take(4);
    let _ = a.report(5, 3, 2);
    let other = a.pending(4);
    let cross = a.take_as(5, 4);
    out[1] = r1 == Ok(1)
        && p1 == Ok(1)
        && t1 == Ok(2)
        && p2 == Ok(0)
        && t2 == Err(DLE_ECLEARED)
        && other == Ok(0)
        && cross == Err(DLE_EISOL)
        && a.reports() == 2
        && a.takes() == 1
        && a.isolations() == 1
        && a.isolated(5)
        && a.len() == 2;

    // 二路：六类失败互不相同。
    let mut b = Tls::new();
    let _ = b.report(7, 1, 5);
    let _ = b.take(7);
    let e1 = b.take(7);
    let e2 = b.take(8);
    let e3 = b.report(0, 1, 1);
    let e4 = b.report(7, 0, 6);
    let e5 = b.report(7, 2, 4);
    let mut c = Tls::new();
    let mut i = 0u16;
    while (i as usize) < CAP {
        let _ = c.report(20 + i, 1, 1);
        i += 1;
    }
    let e6 = c.report(99, 1, 1);
    let bad = c.render(CAP);
    let distinct = e1.err() != e2.err()
        && e2.err() != e3.err()
        && e3.err() != e4.err()
        && e4.err() != e5.err()
        && e5.err() != e6.err()
        && e1.err() != e6.err();
    out[2] = e1 == Err(DLE_ECLEARED)
        && e2 == Err(DLE_ENOSLOT)
        && e3 == Err(DLE_EINVAL)
        && e4 == Err(DLE_EBADREASON)
        && e5 == Err(DLE_ESEQ)
        && e6 == Err(DLE_EFULL)
        && bad == Err(DLE_EBADKIND)
        && distinct
        && b.cleared() == 1;

    // 三路：闸门次序与容量边界。
    let mut d = Tls::new();
    let g1 = d.report(0, 99, 0);
    let g2 = d.report(3, 99, 0);
    let _ = d.report(3, 1, 9);
    let g3 = d.report(3, 2, 8);
    let g4 = d.report(3, 2, 9);
    let mut e = Tls::new();
    let mut k = 0u16;
    while (k as usize) < CAP {
        let _ = e.report(40 + k, 1, 1);
        k += 1;
    }
    let full = e.report(60, 1, 1);
    let exist = e.report(40, 2, 2);
    let size = e.len();
    let mut f = build_ledger();
    let one = f.freeze();
    let two = f.freeze();
    let after = f.register(Sample::blank());
    out[3] = g1 == Err(DLE_EINVAL)
        && g2 == Err(DLE_EBADREASON)
        && g3 == Err(DLE_ESEQ)
        && g4 == Ok(1)
        && full == Err(DLE_EFULL)
        && exist == Ok(6)
        && size == CAP
        && one.is_ok()
        && two.is_err()
        && after == Err(DLE_EORDER)
        && f.rejects() == 2;

    out
}

/// 运行本卡检查。
pub fn run_unx11366_checks() -> CheckSet {
    let mut s = CheckSet::new("UNX-F11366");
    let l = build_ledger();

    s.add_dyn(
        "DLE1-十四格八允六拒",
        l.len() == SAMPLES && l.ok_count() == 8 && l.err_count() == 6 && l.audit().is_ok(),
        &format!("n={} ok={} err={} audit={:?}", l.len(), l.ok_count(), l.err_count(), l.audit()),
    );

    s.add(
        "DLE2-取后即清",
        {
            let mut t = Tls::new();
            let _ = t.report(1, 4, 1);
            let a = t.take(1);
            let b = t.take(1);
            a == Ok(4) && b == Err(DLE_ECLEARED)
        },
        "static",
    );

    s.add(
        "DLE3-线程互相隔离",
        {
            let mut t = Tls::new();
            let _ = t.report(1, 1, 1);
            let _ = t.report(2, 2, 1);
            let both = t.pending(1) == Ok(1) && t.pending(2) == Ok(1);
            let seen = !t.isolated(1) && !t.isolated(2);
            let _ = t.take(2);
            both && seen && t.isolated(1) && t.pending(2) == Ok(0)
        },
        "static",
    );

    s.add(
        "DLE4-跨线程取用被拒",
        {
            let mut t = Tls::new();
            let _ = t.report(2, 3, 1);
            let x = t.take_as(1, 2);
            let y = t.take(2);
            x == Err(DLE_EISOL) && y == Ok(3) && t.isolations() == 1 && t.takes() == 1
        },
        "static",
    );

    s.add(
        "DLE5-原因类与渲染",
        {
            let mut t = Tls::new();
            let z = t.report(1, 0, 1);
            let big = t.report(1, 8, 1);
            let ok = t.report(1, 7, 1);
            let r = t.render(0);
            let bad = t.render(1);
            z == Err(DLE_EBADREASON)
                && big == Err(DLE_EBADREASON)
                && ok == Ok(1)
                && r == Ok(7)
                && bad == Err(DLE_EBADKIND)
        },
        "static",
    );

    s.add(
        "DLE6-序号只进不退",
        {
            let mut t = Tls::new();
            let _ = t.report(1, 1, 10);
            let back = t.report(1, 2, 9);
            let same = t.report(1, 3, 10);
            let fwd = t.report(1, 4, 11);
            back == Err(DLE_ESEQ)
                && same == Ok(1)
                && fwd == Ok(1)
                && t.take(1) == Ok(4)
                && t.reports() == 3
        },
        "static",
    );

    s.add(
        "DLE7-容量精确",
        {
            let mut t = Tls::new();
            let mut i = 0u16;
            while (i as usize) < CAP {
                let _ = t.report(10 + i, 1, 1);
                i += 1;
            }
            let over = t.report(50, 1, 1);
            let again = t.report(10, 2, 2);
            over == Err(DLE_EFULL) && again == Ok(6) && t.len() == CAP
        },
        "static",
    );

    s.add(
        "DLE8-重复报告覆盖不累积",
        {
            let mut t = Tls::new();
            let _ = t.report(1, 1, 1);
            let _ = t.report(1, 2, 2);
            let n = t.len();
            let p = t.pending(1);
            let v = t.take(1);
            n == 1 && p == Ok(1) && v == Ok(2)
        },
        "static",
    );

    s.add(
        "DLE9-账本槽序不可越",
        {
            let mut x = Ledger::new();
            let a = x.register(cell(1, OP_REPORT, Ok(1)));
            let b = x.register(cell(0, OP_REPORT, Ok(1)));
            let c = x.register(cell(0, OP_REPORT, Ok(1)));
            a == Err(DLE_EORDER)
                && b == Ok(())
                && c == Err(DLE_EORDER)
                && x.len() == 1
                && x.rejects() == 2
        },
        "static",
    );

    s.add(
        "DLE10-审计拒绝错值",
        {
            let mut x = build_ledger();
            let a = x.audit();
            let after = x.register(Sample { slot: 0, op: OP_REPORT, ok: true, code: 0, value: 1 });
            a.is_ok() && after == Err(DLE_EORDER) && x.rejects() == 1 && x.root() == 0
        },
        "static",
    );

    s.add(
        "DLE11-冻结后写拒",
        {
            let mut x = build_ledger();
            let r = x.freeze();
            let again = x.freeze();
            let w = x.register(Sample::blank());
            let frozen = x.is_frozen();
            r.is_ok() && again.is_err() && w == Err(DLE_EORDER) && frozen && x.rejects() == 2
        },
        "static",
    );

    s.add(
        "DLE12-观测面边界",
        {
            let x = build_ledger();
            let a = x.sample(SAMPLES - 1);
            let b = x.sample(SAMPLES);
            let c = x.sample(SAMPLES + 5);
            a.is_some() && b.is_none() && c.is_none() && !x.is_empty()
        },
        "static",
    );

    s.add(
        "DLE13-冻结重放四注入",
        {
            let mut f = build_ledger();
            let rep = f.replay();
            let one = f.freeze();
            let two = f.freeze();
            let inj = inject_four();
            rep.is_ok()
                && one.is_ok()
                && one == rep
                && two.is_err()
                && f.is_frozen()
                && f.rejects() == 1
                && f.root() != 0
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
        let s = run_unx11366_checks();
        assert_eq!(s.len(), 13);
        assert!(s.all_passed());
    }

    #[test]
    fn one_shot_consumption() {
        let mut t = Tls::new();
        assert_eq!(t.report(9, 6, 1), Ok(1));
        assert_eq!(t.take(9), Ok(6));
        assert_eq!(t.take(9), Err(DLE_ECLEARED));
        assert_eq!(t.pending(9), Ok(0));
    }

    #[test]
    fn thread_isolation_holds() {
        let mut t = Tls::new();
        assert_eq!(t.report(1, 1, 1), Ok(1));
        assert_eq!(t.report(2, 2, 1), Ok(2));
        assert_eq!(t.pending(1), Ok(1));
        assert_eq!(t.pending(2), Ok(1));
        assert_eq!(t.take_as(1, 2), Err(DLE_EISOL));
        assert!(t.isolated(1) == false);
    }

    #[test]
    fn gate_order_null_thread_first() {
        let mut t = Tls::new();
        assert_eq!(t.report(0, 99, 0), Err(DLE_EINVAL));
        assert_eq!(t.report(3, 99, 0), Err(DLE_EBADREASON));
        assert_eq!(t.take(0), Err(DLE_EINVAL));
    }

    #[test]
    fn capacity_is_exact() {
        let mut t = Tls::new();
        let mut i = 0u16;
        while (i as usize) < CAP {
            assert_eq!(t.report(10 + i, 1, 1), Ok(i as usize + 1));
            i += 1;
        }
        assert_eq!(t.report(99, 1, 1), Err(DLE_EFULL));
        assert_eq!(t.report(10, 2, 2), Ok(CAP));
        assert_eq!(t.len(), CAP);
    }

    #[test]
    fn sequence_must_not_regress() {
        let mut t = Tls::new();
        assert_eq!(t.report(1, 1, 10), Ok(1));
        assert_eq!(t.report(1, 1, 9), Err(DLE_ESEQ));
        assert_eq!(t.report(1, 1, 10), Ok(1));
        assert_eq!(t.take(1), Ok(1));
    }

    #[test]
    fn overwrite_keeps_single_pending() {
        let mut t = Tls::new();
        assert_eq!(t.report(1, 1, 1), Ok(1));
        assert_eq!(t.report(1, 2, 2), Ok(1));
        assert_eq!(t.report(1, 3, 3), Ok(1));
        assert_eq!(t.len(), 1);
        assert_eq!(t.take(1), Ok(3));
    }
}
