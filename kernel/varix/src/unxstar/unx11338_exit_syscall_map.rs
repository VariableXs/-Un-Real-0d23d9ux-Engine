//! UNX-F11338 退出序列 syscall 总图账（0xeb20 段）
//!
//! 一次进程退出并不是一条 `exit_group` 就完事：musl 的收尾会先 `write` 冲
//! 日志/stdio、`close` 掉打开的 fd、`munmap` 掉线程栈、复位信号掩码与
//! robust list，最后才 `exit_group`。这些调用有**次序约束**——`exit_group`
//! 必须是最后一条，之后不可能再有成功的用户态 syscall；也有**总量约束**
//! ——调用条数应当和预期图账一致，多一条说明有重复收尾，少一条说明漏了。
//!
//! 本模块把退出序列压成一张定容总图账：
//!
//! - 每条调用有自带序号（从 1 起，单调 +1，由账本自己发号，不接受外部传入）；
//! - 每条调用归类目（write/close/munmap/exit_group/sigprocmask/robust），
//!   类目计数单独维护，`count_of` 可逐类复核；
//! - `finish(expected)` 做三件事：确认存在 `exit_group`、确认它落在**最后
//!   一位**、确认总条数等于预期。三件事对应三个不同错误码，因为它们指向
//!   三种完全不同的实现缺陷：根本没终结、终结后还有残留、总量漂移。
//!
//! 闸门次序（`push`）：已终结 → 类目非法 → 表满。已终结排最前，是因为
//! 终结之后的任何入账都不该被接受，哪怕类目非法——此时「序列已经关了」是
//! 更根本的结论；而类目非法排在表满之前，是为了让参数错误永远可诊断。
//!
//! `finish` 的次序：已终结 → 无终结调用 → 终结不在尾部 → 总量不符。注意
//! 「无终结」必须排在「不在尾部」前：没有终结调用时去谈尾部位置是无意义的。

use crate::alloc::format;
use crate::checks::CheckSet;

/// 段基址。
pub const SEG_BASE: u32 = 0xeb20;

/// 类目非法。
pub const SM_EKIND: u32 = SEG_BASE + 0x01;
/// 序列已终结。
pub const SM_ESEALED: u32 = SEG_BASE + 0x02;
/// 调用表已满。
pub const SM_EFULL: u32 = SEG_BASE + 0x03;
/// 序列里没有终结调用。
pub const SM_ENOEXIT: u32 = SEG_BASE + 0x04;
/// 终结调用不在尾部。
pub const SM_ETAIL: u32 = SEG_BASE + 0x05;
/// 总条数与预期不符。
pub const SM_ETOTAL: u32 = SEG_BASE + 0x06;
/// 调用下标越界。
pub const SM_ENOBS: u32 = SEG_BASE + 0x07;
/// 账本失衡。
pub const SM_EBAL: u32 = SEG_BASE + 0x08;
/// 账本槽位乱序。
pub const SM_EORDER: u32 = SEG_BASE + 0x09;
/// 观测值与复算不符。
pub const SM_EMISMATCH: u32 = SEG_BASE + 0x0a;
/// 冻结后重放不一致。
pub const SM_ESEAL: u32 = SEG_BASE + 0x0b;
/// 预期图账不合法。
pub const SM_EEXPECT: u32 = SEG_BASE + 0x0c;
/// 尚无调用。
pub const SM_EWARM: u32 = SEG_BASE + 0x0d;

/// 调用表容量。
pub const CAP: usize = 8;
/// 类目数（含 0 号空位）。
pub const KINDS: usize = 7;
/// 账本格数。
pub const SAMPLES: usize = 14;

/// 类目：write。
pub const K_WRITE: u8 = 1;
/// 类目：close。
pub const K_CLOSE: u8 = 2;
/// 类目：munmap。
pub const K_MUNMAP: u8 = 3;
/// 类目：exit_group。
pub const K_EXIT: u8 = 4;
/// 类目：sigprocmask。
pub const K_SIGMASK: u8 = 5;
/// 类目：set_robust_list。
pub const K_ROBUST: u8 = 6;

/// 操作码：入账。
pub const OP_PUSH: u8 = 1;
/// 操作码：终结。
pub const OP_FINISH: u8 = 2;
/// 操作码：按类目计数。
pub const OP_COUNT: u8 = 3;

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

/// 一条退出期调用。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Call {
    /// 序号（从 1 起）。
    pub ord: u32,
    /// 类目。
    pub kind: u8,
    /// 返回值（仅记账，不参与闸门）。
    pub ret: i32,
}

impl Call {
    /// 空调用。
    pub const fn blank() -> Call {
        Call { ord: 0, kind: 0, ret: 0 }
    }
}

/// 退出序列总图账。
#[derive(Clone, Copy, Debug)]
pub struct Trace {
    rows: [Call; CAP],
    n: usize,
    counts: [u32; KINDS],
    closed: bool,
}

impl Trace {
    /// 空图账。
    pub const fn new() -> Trace {
        Trace { rows: [Call::blank(); CAP], n: 0, counts: [0; KINDS], closed: false }
    }

    /// 调用条数。
    pub fn len(&self) -> usize {
        self.n
    }

    /// 是否为空。
    pub fn is_empty(&self) -> bool {
        self.n == 0
    }

    /// 是否已终结。
    pub fn is_closed(&self) -> bool {
        self.closed
    }

    /// 某类目出现次数。
    pub fn count_of(&self, kind: u8) -> Result<u32, u32> {
        if kind == 0 || kind as usize >= KINDS {
            return Err(SM_EKIND);
        }
        match self.counts.get(kind as usize) {
            Some(v) => Ok(*v),
            None => Err(SM_EKIND),
        }
    }

    /// 第 i 条调用的序号。
    pub fn ord_at(&self, i: usize) -> Result<u32, u32> {
        if i >= self.n {
            return Err(SM_ENOBS);
        }
        match self.rows.get(i) {
            Some(c) => Ok(c.ord),
            None => Err(SM_ENOBS),
        }
    }

    /// 第 i 条调用的类目。
    pub fn kind_at(&self, i: usize) -> Result<u32, u32> {
        if i >= self.n {
            return Err(SM_ENOBS);
        }
        match self.rows.get(i) {
            Some(c) => Ok(c.kind as u32),
            None => Err(SM_ENOBS),
        }
    }

    /// 第 i 条调用的返回值。
    pub fn ret_at(&self, i: usize) -> Result<i32, u32> {
        if i >= self.n {
            return Err(SM_ENOBS);
        }
        match self.rows.get(i) {
            Some(c) => Ok(c.ret),
            None => Err(SM_ENOBS),
        }
    }

    /// 入账一条调用，返回当前条数（同时也是该条序号）。
    pub fn push(&mut self, kind: u8, ret: i32) -> Result<usize, u32> {
        if self.closed {
            return Err(SM_ESEALED);
        }
        if kind == 0 || kind as usize >= KINDS {
            return Err(SM_EKIND);
        }
        if self.n >= CAP {
            return Err(SM_EFULL);
        }
        let ord = (self.n as u32) + 1;
        if let Some(dst) = self.rows.get_mut(self.n) {
            *dst = Call { ord, kind, ret };
        }
        if let Some(c) = self.counts.get_mut(kind as usize) {
            *c = c.saturating_add(1);
        }
        self.n += 1;
        Ok(self.n)
    }

    /// 终结序列，并核对预期总条数。
    ///
    /// 成功返回条目数；这条序列从此只读，任何 `push` 都是 `ESEALED`。
    pub fn finish(&mut self, expected: usize) -> Result<usize, u32> {
        if self.closed {
            return Err(SM_ESEALED);
        }
        if expected == 0 || expected > CAP {
            return Err(SM_EEXPECT);
        }
        let mut last_exit: Option<usize> = None;
        let mut i = 0usize;
        while i < self.n {
            if let Some(c) = self.rows.get(i) {
                if c.kind == K_EXIT {
                    last_exit = Some(i);
                }
            }
            i += 1;
        }
        let idx = match last_exit {
            Some(v) => v,
            None => return Err(SM_ENOEXIT),
        };
        if idx + 1 != self.n {
            return Err(SM_ETAIL);
        }
        if self.n != expected {
            return Err(SM_ETOTAL);
        }
        self.closed = true;
        Ok(self.n)
    }

    /// 终结调用所在位置的 1 基序号。
    pub fn terminal_ord(&self) -> Result<u32, u32> {
        if self.n == 0 {
            return Err(SM_EWARM);
        }
        let mut i = self.n;
        while i > 0 {
            i -= 1;
            if let Some(c) = self.rows.get(i) {
                if c.kind == K_EXIT {
                    return Ok(c.ord);
                }
            }
        }
        Err(SM_ENOEXIT)
    }

    /// 独立复算：序号必须严格 1..=n，类目计数之和必须等于 n。
    pub fn recount(&self) -> Result<usize, u32> {
        let mut i = 0usize;
        while i < self.n {
            let c = match self.rows.get(i) {
                Some(v) => *v,
                None => return Err(SM_EBAL),
            };
            if c.ord != (i as u32) + 1 {
                return Err(SM_EORDER);
            }
            i += 1;
        }
        let mut sum = 0u32;
        let mut k = 0usize;
        while k < KINDS {
            if let Some(v) = self.counts.get(k) {
                sum = sum.saturating_add(*v);
            }
            k += 1;
        }
        if sum as usize != self.n {
            return Err(SM_EMISMATCH);
        }
        Ok(self.n)
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
            return Err(SM_EORDER);
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
            return Err(SM_EBAL);
        }
        const VALS: [u32; 8] = [1, 2, 3, 4, 4, 1, 1, 4];
        const CODES: [u32; 6] = [
            SM_EKIND,
            SM_ESEALED,
            SM_EFULL,
            SM_ENOEXIT,
            SM_ETAIL,
            SM_ETOTAL,
        ];
        let mut vi = 0usize;
        let mut ci = 0usize;
        let mut i = 0usize;
        while i < self.len {
            let s = match self.rows.get(i) {
                Some(v) => *v,
                None => return Err(SM_EBAL),
            };
            if s.slot as usize != i {
                return Err(SM_EORDER);
            }
            if s.ok {
                if s.code != 0 {
                    return Err(SM_EMISMATCH);
                }
                match VALS.get(vi) {
                    Some(w) => {
                        if s.value != *w {
                            return Err(SM_EMISMATCH);
                        }
                    }
                    None => return Err(SM_EBAL),
                }
                vi += 1;
            } else {
                match CODES.get(ci) {
                    Some(w) => {
                        if s.code != *w {
                            return Err(SM_EMISMATCH);
                        }
                    }
                    None => return Err(SM_EBAL),
                }
                ci += 1;
            }
            i += 1;
        }
        if vi != 8 || ci != 6 {
            return Err(SM_EBAL);
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
            return Err(SM_EBAL);
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
            Err(SM_ESEAL)
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

/// 十四格账本：四步退出序列 → 非法类目 → 终结对账 → 计数 → 终结后拒绝 → 三类图账错误。
pub fn build_ledger() -> Ledger {
    let mut l = Ledger::new();
    let mut t = Trace::new();

    let _ = l.register(cell(0, OP_PUSH, t.push(K_WRITE, 1)));
    let _ = l.register(cell(1, OP_PUSH, t.push(K_CLOSE, 0)));
    let _ = l.register(cell(2, OP_PUSH, t.push(0, 0)));
    let _ = l.register(cell(3, OP_PUSH, t.push(K_MUNMAP, 0)));
    let _ = l.register(cell(4, OP_PUSH, t.push(K_EXIT, 0)));
    let _ = l.register(cell(5, OP_FINISH, t.finish(4)));
    let _ = l.register(word_cell(6, OP_COUNT, t.count_of(K_WRITE)));
    let _ = l.register(word_cell(7, OP_COUNT, t.count_of(K_EXIT)));
    let _ = l.register(cell(8, OP_PUSH, Ok(t.len())));
    let _ = l.register(cell(9, OP_PUSH, t.push(K_WRITE, 0)));

    let mut f = Trace::new();
    let mut i = 0usize;
    while i < CAP {
        let _ = f.push(K_SIGMASK, 0);
        i += 1;
    }
    let _ = l.register(cell(10, OP_PUSH, f.push(K_ROBUST, 0)));

    let mut e = Trace::new();
    let _ = l.register(cell(11, OP_FINISH, e.finish(1)));

    let mut tail = Trace::new();
    let _ = tail.push(K_WRITE, 1);
    let _ = tail.push(K_EXIT, 0);
    let _ = tail.push(K_WRITE, 1);
    let _ = l.register(cell(12, OP_FINISH, tail.finish(3)));

    let mut g = Trace::new();
    let _ = g.push(K_WRITE, 1);
    let _ = g.push(K_CLOSE, 0);
    let _ = g.push(K_EXIT, 0);
    let _ = l.register(cell(13, OP_FINISH, g.finish(5)));

    l
}

/// 四路注入。
pub fn inject_four() -> [bool; 4] {
    let mut out = [false; 4];

    let l = build_ledger();
    let rep = l.replay();
    let root = l.seal();
    out[0] = l.len() == SAMPLES && l.audit().is_ok() && rep == Ok(root) && root != 0;

    // 一路：教科书式退出序列。
    let mut a = Trace::new();
    let _ = a.push(K_WRITE, 1);
    let _ = a.push(K_CLOSE, 0);
    let _ = a.push(K_MUNMAP, 0);
    let fourth = a.push(K_EXIT, 0);
    let fin = a.finish(4);
    out[1] = fourth == Ok(4)
        && fin == Ok(4)
        && a.is_closed()
        && a.terminal_ord() == Ok(4)
        && a.count_of(K_WRITE) == Ok(1)
        && a.count_of(K_EXIT) == Ok(1)
        && a.count_of(K_CLOSE) == Ok(1)
        && a.ord_at(0) == Ok(1)
        && a.ord_at(3) == Ok(4)
        && a.kind_at(3) == Ok(K_EXIT as u32)
        && a.recount() == Ok(4)
        && a.push(K_WRITE, 0) == Err(SM_ESEALED);

    // 二路：终结位置与终点缺失。
    let mut b = Trace::new();
    let _ = b.push(K_WRITE, 1);
    let _ = b.push(K_EXIT, 0);
    let _ = b.push(K_WRITE, 1);
    let tail = b.finish(3);
    let mut c = Trace::new();
    let _ = c.push(K_SIGMASK, 0);
    let none = c.finish(1);
    let mut d = Trace::new();
    let _ = d.push(K_EXIT, 0);
    let ok = d.finish(1);
    let twice = d.finish(1);
    out[2] = tail == Err(SM_ETAIL)
        && none == Err(SM_ENOEXIT)
        && ok == Ok(1)
        && twice == Err(SM_ESEALED)
        && b.terminal_ord() == Ok(2)
        && c.terminal_ord() == Err(SM_ENOEXIT);

    // 三路：闸门与总量。
    let mut e = Trace::new();
    let g1 = e.push(0, 0) == Err(SM_EKIND);
    let g2 = e.push(KINDS as u8, 0) == Err(SM_EKIND);
    let mut i = 0usize;
    while i < CAP {
        let _ = e.push(K_ROBUST, 0);
        i += 1;
    }
    let g3 = e.push(K_WRITE, 0) == Err(SM_EFULL);
    let g4 = e.count_of(0) == Err(SM_EKIND);
    let g5 = e.ord_at(CAP) == Err(SM_ENOBS);
    let g6 = e.count_of(K_WRITE) == Ok(0);
    let mut h = Trace::new();
    let _ = h.push(K_WRITE, 1);
    let _ = h.push(K_EXIT, 0);
    let g7 = h.finish(3) == Err(SM_ETOTAL);
    let g8 = h.finish(0) == Err(SM_EEXPECT);
    let g9 = h.finish(2) == Ok(2);
    out[3] = g1 && g2 && g3 && g4 && g5 && g6 && g7 && g8 && g9;

    out
}

/// 十三项判据。
pub fn run_unx11338_checks() -> CheckSet {
    let mut s = CheckSet::new("unx11338.exit_syscall_map");
    let l = build_ledger();

    s.add_dyn(
        "SM1-十四格八允六拒",
        l.len() == SAMPLES && l.ok_count() == 8 && l.err_count() == 6 && l.audit().is_ok(),
        &format!("n={} ok={} err={}", l.len(), l.ok_count(), l.err_count()),
    );

    s.add(
        "SM2-序号自增与条数",
        l.sample(0).map(|x| x.value) == Some(1)
            && l.sample(1).map(|x| x.value) == Some(2)
            && l.sample(8).map(|x| x.value) == Some(4),
        "static",
    );

    s.add(
        "SM3-非法类目被拒",
        l.sample(2).map(|x| x.code) == Some(SM_EKIND),
        "static",
    );

    s.add(
        "SM4-终结调用收尾",
        l.sample(3).map(|x| x.value) == Some(3)
            && l.sample(4).map(|x| x.value) == Some(4),
        "static",
    );

    s.add(
        "SM5-总图对账通过",
        l.sample(5).map(|x| x.value) == Some(4),
        "static",
    );

    s.add(
        "SM6-按类目计数",
        l.sample(6).map(|x| x.value) == Some(1)
            && l.sample(7).map(|x| x.value) == Some(1),
        "static",
    );

    s.add(
        "SM7-终结后入账被拒",
        l.sample(9).map(|x| x.code) == Some(SM_ESEALED),
        "static",
    );

    s.add(
        "SM8-表满后拒绝新调用",
        l.sample(10).map(|x| x.code) == Some(SM_EFULL),
        "static",
    );

    s.add(
        "SM9-零调用无终结",
        l.sample(11).map(|x| x.code) == Some(SM_ENOEXIT),
        "static",
    );

    s.add(
        "SM10-尾部残留被拒",
        l.sample(12).map(|x| x.code) == Some(SM_ETAIL),
        "static",
    );

    s.add(
        "SM11-总条数不符被拒",
        l.sample(13).map(|x| x.code) == Some(SM_ETOTAL),
        "static",
    );

    s.add(
        "SM12-类目覆盖独立复算",
        {
            let mut t = Trace::new();
            let _ = t.push(K_WRITE, 1);
            let _ = t.push(K_WRITE, 1);
            let _ = t.push(K_CLOSE, 0);
            let _ = t.push(K_MUNMAP, 0);
            let _ = t.push(K_EXIT, 0);
            t.recount() == Ok(5)
                && t.count_of(K_WRITE) == Ok(2)
                && t.count_of(K_MUNMAP) == Ok(1)
                && t.count_of(K_SIGMASK) == Ok(0)
                && t.terminal_ord() == Ok(5)
                && t.kind_at(4) == Ok(K_EXIT as u32)
                && t.ret_at(0) == Ok(1)
                && t.finish(5) == Ok(5)
                && t.recount() == Ok(5)
                && t.push(K_EXIT, 0) == Err(SM_ESEALED)
        },
        "static",
    );

    s.add(
        "SM13-冻结重放四注入",
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
        let s = run_unx11338_checks();
        assert_eq!(s.len(), 13);
        assert!(s.all_passed());
    }

    #[test]
    fn canonical_sequence_finishes() {
        let mut t = Trace::new();
        assert_eq!(t.push(K_WRITE, 1), Ok(1));
        assert_eq!(t.push(K_CLOSE, 0), Ok(2));
        assert_eq!(t.push(K_MUNMAP, 0), Ok(3));
        assert_eq!(t.push(K_EXIT, 0), Ok(4));
        assert_eq!(t.finish(4), Ok(4));
        assert!(t.is_closed());
        assert_eq!(t.terminal_ord(), Ok(4));
        assert_eq!(t.recount(), Ok(4));
        assert_eq!(t.push(K_WRITE, 0), Err(SM_ESEALED));
    }

    #[test]
    fn tail_is_detected() {
        let mut t = Trace::new();
        let _ = t.push(K_WRITE, 1);
        let _ = t.push(K_EXIT, 0);
        let _ = t.push(K_CLOSE, 0);
        assert_eq!(t.finish(3), Err(SM_ETAIL));
        assert!(!t.is_closed());
        assert_eq!(t.terminal_ord(), Ok(2));
    }

    #[test]
    fn missing_exit_is_detected() {
        let mut t = Trace::new();
        let _ = t.push(K_WRITE, 1);
        assert_eq!(t.finish(1), Err(SM_ENOEXIT));
        assert_eq!(t.terminal_ord(), Err(SM_ENOEXIT));
        let empty = Trace::new();
        assert_eq!(empty.terminal_ord(), Err(SM_EWARM));
    }

    #[test]
    fn total_and_expectation() {
        let mut t = Trace::new();
        let _ = t.push(K_WRITE, 1);
        let _ = t.push(K_EXIT, 0);
        assert_eq!(t.finish(5), Err(SM_ETOTAL));
        assert_eq!(t.finish(0), Err(SM_EEXPECT));
        assert_eq!(t.finish(2), Ok(2));
        assert_eq!(t.finish(2), Err(SM_ESEALED));
    }

    #[test]
    fn kind_gates() {
        let mut t = Trace::new();
        assert_eq!(t.push(0, 0), Err(SM_EKIND));
        assert_eq!(t.push(KINDS as u8, 0), Err(SM_EKIND));
        assert_eq!(t.count_of(0), Err(SM_EKIND));
        assert_eq!(t.ord_at(0), Err(SM_ENOBS));
        assert_eq!(t.kind_at(0), Err(SM_ENOBS));
        assert_eq!(t.push(K_WRITE, 0), Ok(1));
        assert_eq!(t.count_of(K_WRITE), Ok(1));
        assert_eq!(t.count_of(K_EXIT), Ok(0));
    }

    #[test]
    fn freeze_blocks_writes() {
        let mut l = build_ledger();
        assert!(l.freeze().is_ok());
        assert!(l.register(Sample::blank()).is_err());
        assert_eq!(l.rejects(), 1);
    }

    #[test]
    fn inject_paths_agree() {
        assert_eq!(inject_four(), [true; 4]);
    }
}
