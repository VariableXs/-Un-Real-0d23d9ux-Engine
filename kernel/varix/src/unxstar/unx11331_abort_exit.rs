//! UNX-F11331 `abort` 与 `exit` 路径交叉（0xeab0 段）
//!
//! 进程有三种收场方式，账必须分得清：
//!
//! - `exit(code)`：跑 `atexit` 处理器（LIFO）→ 刷标准流 → 以退出码收场；
//! - `abort()`：**不跑**任何 `atexit`、**不刷**任何流，直接以 `SIGABRT` 收场，
//!   并按默认动作产生核心转储；
//! - `raise(sig)`：走信号通道收场，信号未被屏蔽时立即终结；被屏蔽时只登记为
//!   待决，进程继续活着——这一格是「信号路径不等于立刻死」的全部含义。
//!
//! 三通道共用一条「已终结」闸：任何一条通道终结进程之后，其余两条都必须被拒，
//! 且 `atexit` 表与流缓冲都必须保持原样。本模块把通道号、信号号、核心标志、
//! 处理器执行日志、冲刷次数分别记账，任何一次「abort 却跑了 atexit」或者
//! 「exit 却报告信号」都会在账面上露馅。
//!
//! 闸门次序：
//! - `atexit`：零号 → 已终结 → 表满 → 重复；
//! - `exit` / `abort` / `raise`：已终结；
//! - `raise` 内部：信号号越界（`ESIG`）→ 被屏蔽（`EBLOCK`）。

use crate::alloc::format;
use crate::checks::CheckSet;

/// 段基址。
pub const SEG_BASE: u32 = 0xeab0;

/// 功能号为零。
pub const AB_EINVAL: u32 = SEG_BASE + 0x01;
/// 进程已终结（或该类操作重复）。
pub const AB_ERUN: u32 = SEG_BASE + 0x02;
/// 处理器表满。
pub const AB_EFULL: u32 = SEG_BASE + 0x03;
/// 同功能号重复。
pub const AB_EDUP: u32 = SEG_BASE + 0x04;
/// 信号被屏蔽。
pub const AB_EBLOCK: u32 = SEG_BASE + 0x05;
/// 信号号越界。
pub const AB_ESIG: u32 = SEG_BASE + 0x06;
/// 账本形状不符。
pub const AB_EBAL: u32 = SEG_BASE + 0x07;
/// 槽位乱序。
pub const AB_EORDER: u32 = SEG_BASE + 0x08;
/// 观测值与复算不符。
pub const AB_EMISMATCH: u32 = SEG_BASE + 0x09;
/// 冻结后重放不一致。
pub const AB_ESEAL: u32 = SEG_BASE + 0x0a;
/// 观察下标越界。
pub const AB_ENOBS: u32 = SEG_BASE + 0x0b;
/// 终结状态尚未确定。
pub const AB_EPEND: u32 = SEG_BASE + 0x0c;
/// 通道与日志不匹配。
pub const AB_ECHAN: u32 = SEG_BASE + 0x0d;

/// 处理器容量。
pub const CAP: usize = 12;
/// 账本格数。
pub const SAMPLES: usize = 14;

/// 未终结。
pub const CH_NONE: u32 = 0;
/// 退出通道。
pub const CH_EXIT: u32 = 1;
/// 信号通道。
pub const CH_SIGNAL: u32 = 2;
/// 中止通道。
pub const CH_ABORT: u32 = 3;

/// `SIGABRT` 编号。
pub const SIGABRT: u32 = 6;

/// 操作码：登记。
pub const OP_REG: u8 = 1;
/// 操作码：退出。
pub const OP_EXIT: u8 = 2;
/// 操作码：中止。
pub const OP_ABORT: u8 = 3;
/// 操作码：读通道面。
pub const OP_OBS: u8 = 4;

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

/// 一个进程的收场账。
#[derive(Clone, Copy, Debug)]
pub struct Proc {
    tab: [u32; CAP],
    n: usize,
    log: [u32; CAP],
    log_len: usize,
    chan: u32,
    sig: u32,
    core: bool,
    status_word: u32,
    flushes: u32,
    blocked: bool,
    pending: u32,
    terminated: bool,
}

impl Proc {
    /// 新进程。
    pub const fn new() -> Proc {
        Proc {
            tab: [0; CAP],
            n: 0,
            log: [0; CAP],
            log_len: 0,
            chan: CH_NONE,
            sig: 0,
            core: false,
            status_word: 0,
            flushes: 0,
            blocked: false,
            pending: 0,
            terminated: false,
        }
    }

    /// 处理器条数。
    pub fn count(&self) -> usize {
        self.n
    }

    /// 是否已终结。
    pub fn terminated(&self) -> bool {
        self.terminated
    }

    /// 收场通道。
    pub fn channel(&self) -> u32 {
        self.chan
    }

    /// 致命信号号（非信号通道为 0）。
    pub fn signal_no(&self) -> u32 {
        self.sig
    }

    /// 是否产生核心转储。
    pub fn core_dumped(&self) -> u32 {
        if self.core {
            1
        } else {
            0
        }
    }

    /// 处理器日志长度。
    pub fn log_len(&self) -> usize {
        self.log_len
    }

    /// 冲刷次数。
    pub fn flush_count(&self) -> u32 {
        self.flushes
    }

    /// 待决信号（未交付时非零）。
    pub fn pending(&self) -> u32 {
        self.pending
    }

    /// 终结状态字；未终结时无定义。
    pub fn status(&self) -> Result<u32, u32> {
        if !self.terminated {
            return Err(AB_EPEND);
        }
        Ok(self.status_word)
    }

    /// 处理器日志第 i 条。
    pub fn log_at(&self, i: usize) -> Result<u32, u32> {
        if self.chan != CH_EXIT && self.chan != CH_NONE {
            return Err(AB_ECHAN);
        }
        if i >= self.log_len {
            return Err(AB_ENOBS);
        }
        match self.log.get(i) {
            Some(v) => Ok(*v),
            None => Err(AB_ENOBS),
        }
    }

    /// 屏蔽/放行信号。
    pub fn set_block(&mut self, on: bool) {
        self.blocked = on;
    }

    /// 登记处理器。
    pub fn atexit(&mut self, f: u32) -> Result<usize, u32> {
        if f == 0 {
            return Err(AB_EINVAL);
        }
        if self.terminated {
            return Err(AB_ERUN);
        }
        if self.n >= CAP {
            return Err(AB_EFULL);
        }
        let mut i = 0usize;
        while i < self.n {
            match self.tab.get(i) {
                Some(v) => {
                    if *v == f {
                        return Err(AB_EDUP);
                    }
                }
                None => return Err(AB_EFULL),
            }
            i += 1;
        }
        if let Some(slot) = self.tab.get_mut(self.n) {
            *slot = f;
        }
        self.n += 1;
        Ok(self.n)
    }

    /// 正常退出：LIFO 跑处理器、刷流、按退出码收场。
    pub fn exit(&mut self, code: u32) -> Result<usize, u32> {
        if self.terminated {
            return Err(AB_ERUN);
        }
        let mut k = 0usize;
        while k < self.n {
            let src = self.n - 1 - k;
            let f = match self.tab.get(src) {
                Some(v) => *v,
                None => break,
            };
            if let Some(slot) = self.log.get_mut(k) {
                *slot = f;
            }
            k += 1;
        }
        self.log_len = k;
        self.flushes = self.flushes.saturating_add(1);
        self.chan = CH_EXIT;
        self.status_word = code & 0xff;
        self.terminated = true;
        Ok(k)
    }

    /// 中止：不跑处理器、不刷流，`SIGABRT` + 核心转储。
    pub fn abort(&mut self) -> Result<usize, u32> {
        if self.terminated {
            return Err(AB_ERUN);
        }
        self.chan = CH_ABORT;
        self.sig = SIGABRT;
        self.core = true;
        self.status_word = SIGABRT | 0x80;
        self.terminated = true;
        Ok(0)
    }

    /// 发信号：屏蔽则登记待决，否则直接终结。
    pub fn raise(&mut self, sig: u32) -> Result<usize, u32> {
        if self.terminated {
            return Err(AB_ERUN);
        }
        if sig == 0 || sig > 64 {
            return Err(AB_ESIG);
        }
        if self.blocked {
            self.pending = sig;
            return Err(AB_EBLOCK);
        }
        self.chan = CH_SIGNAL;
        self.sig = sig;
        self.core = false;
        self.status_word = sig & 0x7f;
        self.terminated = true;
        Ok(1)
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
            return Err(AB_EORDER);
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
            return Err(AB_EBAL);
        }
        const VALS: [u32; 8] = [1, 2, 0, 3, 6, 1, 0, 134];
        const CODES: [u32; 6] =
            [AB_EINVAL, AB_ERUN, AB_ESIG, AB_EBLOCK, AB_EFULL, AB_EDUP];
        let mut vi = 0usize;
        let mut ci = 0usize;
        let mut i = 0usize;
        while i < self.len {
            let s = match self.rows.get(i) {
                Some(v) => *v,
                None => return Err(AB_EBAL),
            };
            if s.slot as usize != i {
                return Err(AB_EORDER);
            }
            if s.ok {
                if s.code != 0 {
                    return Err(AB_EMISMATCH);
                }
                match VALS.get(vi) {
                    Some(w) => {
                        if s.value != *w {
                            return Err(AB_EMISMATCH);
                        }
                    }
                    None => return Err(AB_EBAL),
                }
                vi += 1;
            } else {
                match CODES.get(ci) {
                    Some(w) => {
                        if s.code != *w {
                            return Err(AB_EMISMATCH);
                        }
                    }
                    None => return Err(AB_EBAL),
                }
                ci += 1;
            }
            i += 1;
        }
        if vi != 8 || ci != 6 {
            return Err(AB_EBAL);
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
            return Err(AB_EBAL);
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
            Err(AB_ESEAL)
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

/// 十四格账本：登记两个处理器后中止，逐面核对通道、信号、核心、日志、状态。
pub fn build_ledger() -> Ledger {
    let mut l = Ledger::new();
    let mut p = Proc::new();

    let _ = l.register(cell(0, OP_REG, p.atexit(1)));
    let _ = l.register(cell(1, OP_REG, p.atexit(2)));
    let _ = l.register(cell(2, OP_ABORT, p.abort()));
    let _ = l.register(word_cell(3, OP_OBS, Ok(p.channel())));
    let _ = l.register(word_cell(4, OP_OBS, Ok(p.signal_no())));
    let _ = l.register(word_cell(5, OP_OBS, Ok(p.core_dumped())));
    let _ = l.register(cell(6, OP_OBS, Ok(p.log_len())));
    let _ = l.register(word_cell(7, OP_OBS, p.status()));

    let mut e = Proc::new();
    let _ = l.register(cell(8, OP_REG, e.atexit(0)));
    let _ = l.register(cell(9, OP_REG, p.atexit(3)));

    let mut s = Proc::new();
    let _ = l.register(cell(10, OP_ABORT, s.raise(0)));

    let mut bl = Proc::new();
    bl.set_block(true);
    let _ = l.register(cell(11, OP_ABORT, bl.raise(2)));

    let mut full = Proc::new();
    let mut i = 1u32;
    while (i as usize) <= CAP {
        let _ = full.atexit(i);
        i += 1;
    }
    let _ = l.register(cell(12, OP_REG, full.atexit(0x100)));

    let mut dup = Proc::new();
    let _ = dup.atexit(9);
    let _ = l.register(cell(13, OP_REG, dup.atexit(9)));

    l
}

/// 四路注入。
pub fn inject_four() -> [bool; 4] {
    let mut out = [false; 4];

    let l = build_ledger();
    let rep = l.replay();
    let root = l.seal();
    out[0] = l.len() == SAMPLES && l.audit().is_ok() && rep == Ok(root) && root != 0;

    // 一路：退出通道跑 LIFO 且刷流。
    let mut a = Proc::new();
    let _ = a.atexit(11);
    let _ = a.atexit(12);
    let ex = a.exit(3);
    out[1] = ex == Ok(2)
        && a.log_at(0) == Ok(12)
        && a.log_at(1) == Ok(11)
        && a.channel() == CH_EXIT
        && a.flush_count() == 1
        && a.signal_no() == 0
        && a.status() == Ok(3)
        && a.core_dumped() == 0;

    // 二路：中止通道一条处理器都不跑、一次都不刷。
    let mut b = Proc::new();
    let _ = b.atexit(13);
    let _ = b.atexit(14);
    let ab = b.abort();
    out[2] = ab == Ok(0)
        && b.log_len() == 0
        && b.flush_count() == 0
        && b.channel() == CH_ABORT
        && b.signal_no() == SIGABRT
        && b.core_dumped() == 1
        && b.status() == Ok(134)
        && b.log_at(0) == Err(AB_ECHAN);

    // 三路：信号通道——放行即终结，屏蔽则待决且不终结。
    let mut c = Proc::new();
    let s1 = c.raise(15);
    let mut d = Proc::new();
    d.set_block(true);
    let s2 = d.raise(15);
    out[3] = s1 == Ok(1)
        && c.channel() == CH_SIGNAL
        && c.signal_no() == 15
        && c.status() == Ok(15)
        && c.core_dumped() == 0
        && s2 == Err(AB_EBLOCK)
        && d.pending() == 15
        && !d.terminated()
        && d.channel() == CH_NONE
        && d.status() == Err(AB_EPEND);

    out
}

/// 十三项判据。
pub fn run_unx11331_checks() -> CheckSet {
    let mut s = CheckSet::new("unx11331.abort_exit");
    let l = build_ledger();

    s.add_dyn(
        "AB1-十四格八允六拒",
        l.len() == SAMPLES && l.ok_count() == 8 && l.err_count() == 6 && l.audit().is_ok(),
        &format!("n={} ok={} err={}", l.len(), l.ok_count(), l.err_count()),
    );

    s.add(
        "AB2-处理器计数递增",
        l.sample(0).map(|x| x.value) == Some(1) && l.sample(1).map(|x| x.value) == Some(2),
        "static",
    );

    s.add(
        "AB3-中止不返回执行条数",
        l.sample(2).map(|x| x.value) == Some(0),
        "static",
    );

    s.add(
        "AB4-通道为中止",
        l.sample(3).map(|x| x.value) == Some(CH_ABORT),
        "static",
    );

    s.add(
        "AB5-信号为SIGABRT",
        l.sample(4).map(|x| x.value) == Some(SIGABRT),
        "static",
    );

    s.add(
        "AB6-核心转储置位",
        l.sample(5).map(|x| x.value) == Some(1),
        "static",
    );

    s.add(
        "AB7-处理器一条未跑",
        l.sample(6).map(|x| x.value) == Some(0),
        "static",
    );

    s.add(
        "AB8-状态报告一百三十四",
        l.sample(7).map(|x| x.value) == Some(134),
        "static",
    );

    s.add(
        "AB9-零号登记被拒",
        l.sample(8).map(|x| x.code) == Some(AB_EINVAL),
        "static",
    );

    s.add(
        "AB10-终结后登记被拒",
        l.sample(9).map(|x| x.code) == Some(AB_ERUN),
        "static",
    );

    s.add(
        "AB11-信号号越界",
        l.sample(10).map(|x| x.code) == Some(AB_ESIG),
        "static",
    );

    s.add(
        "AB12-屏蔽信号待决不终结",
        {
            let mut p = Proc::new();
            p.set_block(true);
            let r = p.raise(15);
            r == Err(AB_EBLOCK)
                && p.pending() == 15
                && !p.terminated()
                && p.status() == Err(AB_EPEND)
                && l.sample(11).map(|x| x.code) == Some(AB_EBLOCK)
        },
        "static",
    );

    s.add(
        "AB13-表满重复与冻结重放",
        {
            let mut f = build_ledger();
            let rep = f.replay();
            let one = f.freeze();
            let two = f.freeze();
            let inj = inject_four();
            l.sample(12).map(|x| x.code) == Some(AB_EFULL)
                && l.sample(13).map(|x| x.code) == Some(AB_EDUP)
                && one.is_ok()
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
        let s = run_unx11331_checks();
        assert_eq!(s.len(), 13);
        assert!(s.all_passed());
    }

    #[test]
    fn exit_runs_handlers_in_lifo() {
        let mut p = Proc::new();
        assert_eq!(p.atexit(1), Ok(1));
        assert_eq!(p.atexit(2), Ok(2));
        assert_eq!(p.exit(7), Ok(2));
        assert_eq!(p.log_at(0), Ok(2));
        assert_eq!(p.log_at(1), Ok(1));
        assert_eq!(p.flush_count(), 1);
        assert_eq!(p.status(), Ok(7));
        assert_eq!(p.channel(), CH_EXIT);
    }

    #[test]
    fn abort_skips_handlers_and_flush() {
        let mut p = Proc::new();
        let _ = p.atexit(1);
        let _ = p.atexit(2);
        assert_eq!(p.abort(), Ok(0));
        assert_eq!(p.log_len(), 0);
        assert_eq!(p.flush_count(), 0);
        assert_eq!(p.signal_no(), SIGABRT);
        assert_eq!(p.core_dumped(), 1);
        assert_eq!(p.status(), Ok(134));
    }

    #[test]
    fn raise_paths() {
        let mut p = Proc::new();
        assert_eq!(p.raise(0), Err(AB_ESIG));
        assert_eq!(p.raise(65), Err(AB_ESIG));
        assert_eq!(p.raise(9), Ok(1));
        assert_eq!(p.channel(), CH_SIGNAL);
        assert_eq!(p.status(), Ok(9));
        assert_eq!(p.raise(9), Err(AB_ERUN));

        let mut b = Proc::new();
        b.set_block(true);
        assert_eq!(b.raise(2), Err(AB_EBLOCK));
        assert_eq!(b.pending(), 2);
        assert!(!b.terminated());
    }

    #[test]
    fn gates() {
        let mut p = Proc::new();
        assert_eq!(p.atexit(0), Err(AB_EINVAL));
        assert_eq!(p.atexit(5), Ok(1));
        assert_eq!(p.atexit(5), Err(AB_EDUP));
        assert_eq!(p.status(), Err(AB_EPEND));
        assert_eq!(p.exit(0), Ok(1));
        assert_eq!(p.atexit(6), Err(AB_ERUN));
        assert_eq!(p.abort(), Err(AB_ERUN));
        assert_eq!(p.raise(1), Err(AB_ERUN));
    }

    #[test]
    fn capacity_is_exact() {
        let mut p = Proc::new();
        let mut i = 1u32;
        while (i as usize) <= CAP {
            assert!(p.atexit(i).is_ok());
            i += 1;
        }
        assert_eq!(p.atexit(0xFFFF), Err(AB_EFULL));
    }

    #[test]
    fn log_at_rejects_non_exit_channel() {
        let mut p = Proc::new();
        let _ = p.atexit(1);
        let _ = p.abort();
        assert_eq!(p.log_at(0), Err(AB_ECHAN));
        let mut q = Proc::new();
        let _ = q.exit(0);
        assert_eq!(q.log_at(0), Err(AB_ENOBS));
    }

    #[test]
    fn freeze_blocks_writes() {
        let mut l = build_ledger();
        assert!(l.freeze().is_ok());
        assert!(l.register(Sample::blank()).is_err());
        assert_eq!(l.rejects(), 1);
    }
}
