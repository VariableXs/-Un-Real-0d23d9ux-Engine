//! UNX-F11328 `on_exit` 带参处理器（0xea80 段）
//!
//! `atexit(3)` 只能登记无参函数，BSD 的 `on_exit(func, arg)` 允许把一份用户数据随
//! 处理器一起挂上，运行时再以 `func(status, arg)` 的形式回调——退出状态因此可以
//! 被每个处理器各读一遍。这条语义有两个硬约束：参数必须原样抵达，且所有处理器看到
//! 的是**同一个**最终状态，不能各自看到不同的时间点。
//!
//! 本模块把处理器压成 `(功能号, 参数)` 二元组，运行期按 LIFO 展开，并把「它看到的
//! 状态」逐条记进观察表：观察表与调用序一一对应，任何一次「提前取状态」都会在账本里
//! 暴露成 status 不一致。
//!
//! 闸门次序（`register`）：功能号为零 → 已运行 → 容量 → 同二元组重复。运行闸门只有
//! 一条：只此一次。

use crate::alloc::format;
use crate::checks::CheckSet;

/// 段基址。
pub const SEG_BASE: u32 = 0xea80;

/// 功能号为零。
pub const OX_EINVAL: u32 = SEG_BASE + 0x01;
/// 已经运行过。
pub const OX_EUSED: u32 = SEG_BASE + 0x02;
/// 处理器表满。
pub const OX_EFULL: u32 = SEG_BASE + 0x03;
/// 同 (func, arg) 重复。
pub const OX_EDUP: u32 = SEG_BASE + 0x04;
/// 重复运行。
pub const OX_ERUN: u32 = SEG_BASE + 0x05;
/// 观察表下标越界。
pub const OX_ENOBS: u32 = SEG_BASE + 0x06;
/// 账本形状不符。
pub const OX_EBAL: u32 = SEG_BASE + 0x07;
/// 槽位乱序。
pub const OX_EORDER: u32 = SEG_BASE + 0x08;
/// 观测值与复算不符。
pub const OX_EMISMATCH: u32 = SEG_BASE + 0x09;
/// 冻结后重放不一致。
pub const OX_ESEAL: u32 = SEG_BASE + 0x0a;
/// 状态观察不一致。
pub const OX_ESTATUS: u32 = SEG_BASE + 0x0b;
/// 参数表与处理器表错位。
pub const OX_EPAIR: u32 = SEG_BASE + 0x0c;
/// 空表运行。
pub const OX_EMPTY: u32 = SEG_BASE + 0x0d;

/// 处理器容量。
pub const CAP: usize = 16;
/// 账本格数。
pub const SAMPLES: usize = 14;
/// 操作码：登记。
pub const OP_REG: u8 = 1;
/// 操作码：运行。
pub const OP_RUN: u8 = 2;
/// 操作码：读观察表。
pub const OP_OBS: u8 = 3;

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

/// 处理器条目。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Entry {
    /// 功能号。
    pub func: u32,
    /// 随处理器携带的参数。
    pub arg: u32,
}

impl Entry {
    /// 空条目。
    pub const fn blank() -> Entry {
        Entry { func: 0, arg: 0 }
    }
}

/// 带参处理器表。
#[derive(Clone, Copy, Debug)]
pub struct OnExit {
    e: [Entry; CAP],
    n: usize,
    seq: [Entry; CAP],
    seen: [u32; CAP],
    ran: usize,
    runs: u32,
    status: u32,
}

impl OnExit {
    /// 空表。
    pub const fn new() -> OnExit {
        OnExit {
            e: [Entry::blank(); CAP],
            n: 0,
            seq: [Entry::blank(); CAP],
            seen: [0; CAP],
            ran: 0,
            runs: 0,
            status: 0,
        }
    }

    /// 在册条数。
    pub fn len(&self) -> usize {
        self.n
    }

    /// 是否空表。
    pub fn is_empty(&self) -> bool {
        self.n == 0
    }

    /// 运行次数。
    pub fn runs(&self) -> u32 {
        self.runs
    }

    /// 已运行条数。
    pub fn ran(&self) -> usize {
        self.ran
    }

    /// 最近一次运行时的最终状态。
    pub fn status(&self) -> u32 {
        self.status
    }

    /// 在册第 i 条。
    pub fn at(&self, i: usize) -> Option<Entry> {
        if i >= self.n {
            return None;
        }
        self.e.get(i).copied()
    }

    /// 第 i 个被执行的功能号。
    pub fn func_at(&self, i: usize) -> Result<u32, u32> {
        if i >= self.ran {
            return Err(OX_ENOBS);
        }
        match self.seq.get(i) {
            Some(x) => Ok(x.func),
            None => Err(OX_ENOBS),
        }
    }

    /// 第 i 个被执行处理器随身携带的参数。
    pub fn arg_at(&self, i: usize) -> Result<u32, u32> {
        if i >= self.ran {
            return Err(OX_ENOBS);
        }
        match self.seq.get(i) {
            Some(x) => Ok(x.arg),
            None => Err(OX_ENOBS),
        }
    }

    /// 第 i 个被执行处理器看到的状态。
    pub fn status_at(&self, i: usize) -> Result<u32, u32> {
        if i >= self.ran {
            return Err(OX_ENOBS);
        }
        match self.seen.get(i) {
            Some(v) => Ok(*v),
            None => Err(OX_ENOBS),
        }
    }

    /// 登记一个带参处理器。返回在册条数。
    pub fn register(&mut self, func: u32, arg: u32) -> Result<usize, u32> {
        if func == 0 {
            return Err(OX_EINVAL);
        }
        if self.runs > 0 {
            return Err(OX_EUSED);
        }
        if self.n >= CAP {
            return Err(OX_EFULL);
        }
        let mut i = 0usize;
        while i < self.n {
            if let Some(x) = self.e.get(i) {
                if x.func == func && x.arg == arg {
                    return Err(OX_EDUP);
                }
            }
            i += 1;
        }
        if let Some(dst) = self.e.get_mut(self.n) {
            *dst = Entry { func, arg };
        }
        self.n += 1;
        Ok(self.n)
    }

    /// 以最终状态运行全部处理器一次（LIFO）。返回执行条数。
    pub fn run(&mut self, status: u32) -> Result<usize, u32> {
        if self.runs > 0 {
            return Err(OX_ERUN);
        }
        if self.n == 0 {
            return Err(OX_EMPTY);
        }
        let mut k = 0usize;
        while k < self.n {
            let src = self.n - 1 - k;
            let x = match self.e.get(src) {
                Some(v) => *v,
                None => return Err(OX_EPAIR),
            };
            if let Some(dst) = self.seq.get_mut(k) {
                *dst = x;
            } else {
                return Err(OX_EPAIR);
            }
            if let Some(dst) = self.seen.get_mut(k) {
                *dst = status;
            } else {
                return Err(OX_EPAIR);
            }
            k += 1;
        }
        self.ran = k;
        self.runs += 1;
        self.status = status;
        Ok(k)
    }

    /// 全部观察是否一致（逐条复算）。
    pub fn consistent(&self) -> bool {
        if self.runs == 0 {
            return true;
        }
        let mut i = 0usize;
        while i < self.ran {
            match self.seen.get(i) {
                Some(v) => {
                    if *v != self.status {
                        return false;
                    }
                }
                None => return false,
            }
            i += 1;
        }
        true
    }

    /// 指纹。
    pub fn seal(&self) -> u64 {
        let mut h = FNV_OFF;
        h = fold(h, self.n as u64);
        let mut i = 0usize;
        while i < self.n {
            if let Some(x) = self.e.get(i) {
                h = fold(h, x.func as u64);
                h = fold(h, x.arg as u64);
            }
            i += 1;
        }
        h = fold(h, self.ran as u64);
        let mut j = 0usize;
        while j < self.ran {
            if let Some(x) = self.seq.get(j) {
                h = fold(h, x.func as u64);
                h = fold(h, x.arg as u64);
            }
            if let Some(v) = self.seen.get(j) {
                h = fold(h, *v as u64);
            }
            j += 1;
        }
        fold(h, self.status as u64)
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
            return Err(OX_EORDER);
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
            return Err(OX_EBAL);
        }
        const VALS: [u32; 8] = [1, 2, 3, 3, 3, 300, 7, 100];
        const CODES: [u32; 6] = [OX_EINVAL, OX_EUSED, OX_EFULL, OX_EDUP, OX_ERUN, OX_ENOBS];
        let mut vi = 0usize;
        let mut ci = 0usize;
        let mut i = 0usize;
        while i < self.len {
            let s = match self.rows.get(i) {
                Some(v) => *v,
                None => return Err(OX_EBAL),
            };
            if s.slot as usize != i {
                return Err(OX_EORDER);
            }
            if s.ok {
                if s.code != 0 {
                    return Err(OX_EMISMATCH);
                }
                match VALS.get(vi) {
                    Some(w) => {
                        if s.value != *w {
                            return Err(OX_EMISMATCH);
                        }
                    }
                    None => return Err(OX_EBAL),
                }
                vi += 1;
            } else {
                match CODES.get(ci) {
                    Some(w) => {
                        if s.code != *w {
                            return Err(OX_EMISMATCH);
                        }
                    }
                    None => return Err(OX_EBAL),
                }
                ci += 1;
            }
            i += 1;
        }
        if vi != 8 || ci != 6 {
            return Err(OX_EBAL);
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
            return Err(OX_EBAL);
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
            Err(OX_ESEAL)
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

/// 十四格账本：三个带参处理器 LIFO 展开，参数与状态逐一核。
pub fn build_ledger() -> Ledger {
    let mut l = Ledger::new();
    let mut o = OnExit::new();

    let _ = l.register(cell(0, OP_REG, o.register(1, 100)));
    let _ = l.register(cell(1, OP_REG, o.register(2, 200)));
    let _ = l.register(cell(2, OP_REG, o.register(3, 300)));
    let _ = l.register(cell(3, OP_RUN, o.run(7)));
    let _ = l.register(word_cell(4, OP_OBS, o.func_at(0)));
    let _ = l.register(word_cell(5, OP_OBS, o.arg_at(0)));
    let _ = l.register(word_cell(6, OP_OBS, o.status_at(0)));
    let _ = l.register(word_cell(7, OP_OBS, o.arg_at(2)));

    let mut w = OnExit::new();
    let _ = l.register(cell(8, OP_REG, w.register(0, 0)));
    let _ = l.register(cell(9, OP_REG, o.register(4, 400)));

    let mut full = OnExit::new();
    let mut i = 1u32;
    while (i as usize) <= CAP {
        let _ = full.register(i, i * 10);
        i += 1;
    }
    let _ = l.register(cell(10, OP_REG, full.register(0x100, 1)));

    let mut dup = OnExit::new();
    let _ = dup.register(5, 50);
    let _ = l.register(cell(11, OP_REG, dup.register(5, 50)));
    let _ = l.register(cell(12, OP_RUN, o.run(8)));
    let _ = l.register(word_cell(13, OP_OBS, o.status_at(o.ran())));
    let _ = w;
    l
}

/// 四路注入。
pub fn inject_four() -> [bool; 4] {
    let mut out = [false; 4];

    let l = build_ledger();
    let rep = l.replay();
    let root = l.seal();
    out[0] = l.len() == SAMPLES && l.audit().is_ok() && rep == Ok(root) && root != 0;

    // 一路：LIFO 展开且参数随条目走。
    let mut a = OnExit::new();
    let _ = a.register(11, 1);
    let _ = a.register(12, 2);
    let _ = a.register(13, 3);
    let n = a.run(9);
    out[1] = n == Ok(3)
        && a.func_at(0) == Ok(13)
        && a.arg_at(0) == Ok(3)
        && a.func_at(2) == Ok(11)
        && a.arg_at(2) == Ok(1)
        && a.consistent();

    // 二路：所有处理器看到同一个最终状态。
    let mut b = OnExit::new();
    let _ = b.register(21, 0);
    let _ = b.register(22, 0);
    let _ = b.register(23, 0);
    let _ = b.run(0x2a);
    out[2] = b.status_at(0) == Ok(0x2a)
        && b.status_at(1) == Ok(0x2a)
        && b.status_at(2) == Ok(0x2a)
        && b.status() == 0x2a
        && b.consistent();

    // 三路：闸门（零号 / 空表 / 重复 / 空运行）。
    let mut g = OnExit::new();
    let g1 = g.register(0, 1) == Err(OX_EINVAL);
    let g2 = g.run(0) == Err(OX_EMPTY);
    let g3 = g.register(1, 1) == Ok(1);
    let g4 = g.register(1, 1) == Err(OX_EDUP);
    let g5 = g.register(1, 2) == Ok(2);
    let g6 = g.run(0) == Ok(2);
    let g7 = g.register(3, 3) == Err(OX_EUSED);
    let g8 = g.run(0) == Err(OX_ERUN);
    out[3] = g1 && g2 && g3 && g4 && g5 && g6 && g7 && g8;

    out
}

/// 十三项判据。
pub fn run_unx11328_checks() -> CheckSet {
    let mut s = CheckSet::new("unx11328.on_exit");
    let l = build_ledger();

    s.add_dyn(
        "OX1-十四格八允六拒",
        l.len() == SAMPLES && l.ok_count() == 8 && l.err_count() == 6 && l.audit().is_ok(),
        &format!("n={} ok={} err={}", l.len(), l.ok_count(), l.err_count()),
    );

    s.add(
        "OX2-登记计数递增",
        l.sample(0).map(|x| x.value) == Some(1)
            && l.sample(1).map(|x| x.value) == Some(2)
            && l.sample(2).map(|x| x.value) == Some(3),
        "static",
    );

    s.add(
        "OX3-运行返回条数",
        l.sample(3).map(|x| x.value) == Some(3),
        "static",
    );

    s.add(
        "OX4-先跑最后登记者",
        l.sample(4).map(|x| x.value) == Some(3) && l.sample(5).map(|x| x.value) == Some(300),
        "static",
    );

    s.add(
        "OX5-状态随运行下传",
        l.sample(6).map(|x| x.value) == Some(7),
        "static",
    );

    s.add(
        "OX6-最老处理器的参数",
        l.sample(7).map(|x| x.value) == Some(100),
        "static",
    );

    s.add(
        "OX7-零号功能被拒",
        l.sample(8).map(|x| x.code) == Some(OX_EINVAL),
        "static",
    );

    s.add(
        "OX8-运行后登记被拒",
        l.sample(9).map(|x| x.code) == Some(OX_EUSED),
        "static",
    );

    s.add(
        "OX9-满员与重复二元组",
        l.sample(10).map(|x| x.code) == Some(OX_EFULL)
            && l.sample(11).map(|x| x.code) == Some(OX_EDUP),
        "static",
    );

    s.add(
        "OX10-重复运行与越界观察",
        l.sample(12).map(|x| x.code) == Some(OX_ERUN)
            && l.sample(13).map(|x| x.code) == Some(OX_ENOBS),
        "static",
    );

    s.add(
        "OX11-同功能号不同参数可共存",
        {
            let mut a = OnExit::new();
            let one = a.register(7, 1);
            let two = a.register(7, 2);
            one == Ok(1) && two == Ok(2) && a.run(0) == Ok(2) && a.arg_at(0) == Ok(2)
        },
        "static",
    );

    s.add(
        "OX12-观察表与执行序对齐",
        {
            let mut a = OnExit::new();
            let _ = a.register(1, 10);
            let _ = a.register(2, 20);
            let _ = a.register(3, 30);
            let _ = a.run(5);
            let mut k = 0usize;
            let mut ok = true;
            while k < 3 {
                let want_f = 3 - k as u32;
                let want_a = 30 - (k as u32) * 10;
                if a.func_at(k) != Ok(want_f) || a.arg_at(k) != Ok(want_a) || a.status_at(k) != Ok(5) {
                    ok = false;
                }
                k += 1;
            }
            ok && a.ran() == 3 && a.consistent()
        },
        "static",
    );

    s.add(
        "OX13-冻结重放四注入",
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
        let s = run_unx11328_checks();
        assert_eq!(s.len(), 13);
        assert!(s.all_passed());
    }

    #[test]
    fn lifo_with_args() {
        let mut a = OnExit::new();
        assert_eq!(a.register(1, 100), Ok(1));
        assert_eq!(a.register(2, 200), Ok(2));
        assert_eq!(a.run(0), Ok(2));
        assert_eq!(a.func_at(0), Ok(2));
        assert_eq!(a.arg_at(0), Ok(200));
        assert_eq!(a.arg_at(1), Ok(100));
    }

    #[test]
    fn same_status_for_all() {
        let mut a = OnExit::new();
        let _ = a.register(1, 0);
        let _ = a.register(2, 0);
        let _ = a.run(0x55);
        assert_eq!(a.status_at(0), Ok(0x55));
        assert_eq!(a.status_at(1), Ok(0x55));
        assert!(a.consistent());
    }

    #[test]
    fn gates() {
        let mut a = OnExit::new();
        assert_eq!(a.register(0, 1), Err(OX_EINVAL));
        assert_eq!(a.run(0), Err(OX_EMPTY));
        assert_eq!(a.register(1, 1), Ok(1));
        assert_eq!(a.register(1, 1), Err(OX_EDUP));
        assert_eq!(a.run(1), Ok(1));
        assert_eq!(a.run(1), Err(OX_ERUN));
        assert_eq!(a.register(2, 2), Err(OX_EUSED));
    }

    #[test]
    fn capacity_is_exact() {
        let mut a = OnExit::new();
        let mut i = 1u32;
        while (i as usize) <= CAP {
            assert!(a.register(i, 0).is_ok());
            i += 1;
        }
        assert_eq!(a.register(0xFFFF, 0), Err(OX_EFULL));
    }

    #[test]
    fn observations_are_bounded() {
        let mut a = OnExit::new();
        let _ = a.register(1, 0);
        let _ = a.run(3);
        assert_eq!(a.func_at(1), Err(OX_ENOBS));
        assert_eq!(a.arg_at(1), Err(OX_ENOBS));
        assert_eq!(a.status_at(1), Err(OX_ENOBS));
    }

    #[test]
    fn freeze_blocks_writes() {
        let mut l = build_ledger();
        assert!(l.freeze().is_ok());
        assert!(l.register(Sample::blank()).is_err());
        assert_eq!(l.rejects(), 1);
    }
}
