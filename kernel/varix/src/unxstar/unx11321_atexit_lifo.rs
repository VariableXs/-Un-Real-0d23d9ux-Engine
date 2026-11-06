//! UNX-F11321 atexit 注册与 LIFO 语义（0xea10 段）
//!
//! `atexit` 的契约只有两条：容量内返回 0 且登记成功，容量满返回非零且不
//! 登记；退出时按注册的逆序执行（LIFO）。第二条是和 C++ 静态析构、stdio
//! 冲刷、锁释放顺序强耦合的——顺序错了，析构函数可能用到已被清掉的全局。
//!
//! 模型把容量、序号、执行日志三样分开记账：登记返回“注册序号”（第一件
//! 为 1），执行时从尾向头搬进日志，日志长度必须等于执行件数。运行只有
//! 一次：第二次运行必须 ERUN，运行后再登记必须 EUSED——这两条防止“退出
//! 过程中又登记新回调”这类未定义行为被静默接受。
//!
//! 记录顺序的意义在于可重放：日志里第 k 位是哪个函数，是比“执行了 4 个”
//! 更强的证据。模型因此提供 `log_at(k)`，越界与未运行分别记 ENOBS 与 EWARM。
//!
//! 账本十四格：八格成功（四次登记、一次计数、一次运行、一次日志位、一次
//! 日志长度），六格拒绝（容量满、运行后登记、重复运行、非法函数号、日志
//! 越界、未运行取日志）。

use crate::alloc::format;
use crate::checks::CheckSet;

/// 段基址。
pub const SEG_BASE: u32 = 0xea10;

/// 编号非法。
pub const AT_EID: u32 = SEG_BASE + 0x01;
/// 登记顺序错误。
pub const AT_EORDER: u32 = SEG_BASE + 0x02;
/// 重复登记。
pub const AT_EDUP: u32 = SEG_BASE + 0x03;
/// 已冻结。
pub const AT_EFROZEN: u32 = SEG_BASE + 0x04;
/// 观测不存在的格。
pub const AT_ENOBS: u32 = SEG_BASE + 0x05;
/// 结论不符。
pub const AT_EMISMATCH: u32 = SEG_BASE + 0x06;
/// 账本失衡。
pub const AT_EBAL: u32 = SEG_BASE + 0x07;
/// 容量满。
pub const AT_EFULL: u32 = SEG_BASE + 0x08;
/// 运行后又登记。
pub const AT_EUSED: u32 = SEG_BASE + 0x09;
/// 重复运行。
pub const AT_ERUN: u32 = SEG_BASE + 0x0a;
/// 非法函数号。
pub const AT_EINVAL: u32 = SEG_BASE + 0x0b;
/// 未运行取日志。
pub const AT_EWARM: u32 = SEG_BASE + 0x0c;
/// 票号不一致。
pub const AT_ESEAL: u32 = SEG_BASE + 0x0d;

/// 判决：成功。
pub const V_OK: u32 = 0;

/// 操作：登记。
pub const OP_REG: u8 = 1;
/// 操作：运行。
pub const OP_RUN: u8 = 2;
/// 操作：取日志位。
pub const OP_LOG: u8 = 3;
/// 操作：计数。
pub const OP_COUNT: u8 = 4;

/// 回调容量。
pub const CAP: usize = 32;
/// 样本格数。
pub const SAMPLES: usize = 14;

/// FNV 偏移基。
pub const FNV_OFF: u64 = 0xcbf2_9ce4_8422_2325;
/// FNV 质数。
pub const FNV_PRIME: u64 = 0x100_0000_01b3;

/// 退出回调账。
#[derive(Clone, Copy, Debug)]
pub struct Atexit {
    funcs: [u32; CAP],
    log: [u32; CAP],
    n: usize,
    ran: bool,
    log_len: usize,
}

impl Atexit {
    /// 空账。
    pub const fn new() -> Atexit {
        Atexit {
            funcs: [0; CAP],
            log: [0; CAP],
            n: 0,
            ran: false,
            log_len: 0,
        }
    }

    /// 已登记件数。
    pub fn count(&self) -> usize {
        self.n
    }

    /// 是否为空。
    pub fn is_empty(&self) -> bool {
        self.n == 0
    }

    /// 是否已运行。
    pub fn ran(&self) -> bool {
        self.ran
    }

    /// 日志长度。
    pub fn log_len(&self) -> usize {
        self.log_len
    }

    /// 取候选（未运行时）第 i 件。
    pub fn pending(&self, i: usize) -> Option<u32> {
        if i >= self.n {
            return None;
        }
        self.funcs.get(i).copied()
    }

    /// 取日志第 i 位。
    pub fn log_at(&self, i: usize) -> Result<u32, u32> {
        if !self.ran {
            return Err(AT_EWARM);
        }
        if i >= self.log_len {
            return Err(AT_ENOBS);
        }
        match self.log.get(i) {
            Some(v) => Ok(*v),
            None => Err(AT_ENOBS),
        }
    }

    /// 登记：非法号 → 容量 → 已运行 → 重复号。
    pub fn register(&mut self, f: u32) -> Result<usize, u32> {
        if f == 0 {
            return Err(AT_EINVAL);
        }
        if self.n >= CAP {
            return Err(AT_EFULL);
        }
        if self.ran {
            return Err(AT_EUSED);
        }
        let mut i = 0;
        while i < self.n {
            match self.funcs.get(i) {
                Some(v) => {
                    if *v == f {
                        return Err(AT_EDUP);
                    }
                }
                None => return Err(AT_ENOBS),
            }
            i += 1;
        }
        match self.funcs.get_mut(self.n) {
            Some(x) => *x = f,
            None => return Err(AT_EFULL),
        }
        self.n += 1;
        Ok(self.n)
    }

    /// 运行：LIFO 搬进日志，只此一次。
    pub fn run(&mut self) -> Result<usize, u32> {
        if self.ran {
            return Err(AT_ERUN);
        }
        self.ran = true;
        let mut k = 0;
        while k < self.n {
            let src = self.n - 1 - k;
            let f = match self.funcs.get(src) {
                Some(v) => *v,
                None => break,
            };
            if let Some(x) = self.log.get_mut(k) {
                *x = f;
            }
            k += 1;
        }
        self.log_len = k;
        Ok(k)
    }
}

/// 折入。
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
            return Err(AT_EFROZEN);
        }
        if (row.slot as usize) != self.len {
            return Err(AT_EORDER);
        }
        if self.len >= SAMPLES {
            return Err(AT_EID);
        }
        let mut i = 0;
        while i < self.len {
            match self.rows.get(i) {
                Some(x) => {
                    if x.slot == row.slot {
                        return Err(AT_EDUP);
                    }
                }
                None => return Err(AT_ENOBS),
            }
            i += 1;
        }
        match self.rows.get_mut(self.len) {
            Some(x) => *x = row,
            None => return Err(AT_EID),
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
            return Err(AT_EBAL);
        }
        if self.ok_count() != 8 || self.err_count() != 6 {
            return Err(AT_EBAL);
        }
        let mut i = 0;
        while i < self.len {
            match self.rows.get(i) {
                Some(s) => {
                    if s.slot as usize != i {
                        return Err(AT_EORDER);
                    }
                    if s.ok {
                        if s.code != V_OK {
                            return Err(AT_EMISMATCH);
                        }
                    } else if s.code == V_OK || s.value != 0 {
                        return Err(AT_EMISMATCH);
                    }
                }
                None => return Err(AT_ENOBS),
            }
            i += 1;
        }
        let vals = [1u32, 2, 3, 4, 4, 4, 2, 4];
        let mut k = 0;
        while k < vals.len() {
            match (self.sample(k), vals.get(k)) {
                (Some(s), Some(w)) => {
                    if !s.ok || s.value != *w {
                        return Err(AT_EMISMATCH);
                    }
                }
                _ => return Err(AT_EBAL),
            }
            k += 1;
        }
        let codes = [AT_EFULL, AT_EUSED, AT_ERUN, AT_EINVAL, AT_ENOBS, AT_EWARM];
        let mut c = 0;
        let mut j = 8;
        while j < self.len {
            match self.rows.get(j) {
                Some(s) => match codes.get(c) {
                    Some(w) => {
                        if s.code != *w {
                            return Err(AT_EMISMATCH);
                        }
                    }
                    None => return Err(AT_EBAL),
                },
                None => return Err(AT_ENOBS),
            }
            c += 1;
            j += 1;
        }
        if c != codes.len() {
            return Err(AT_EBAL);
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
            return Err(AT_EFROZEN);
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
            return Err(AT_EBAL);
        }
        let h = fresh.seal();
        if self.frozen && h != self.seal {
            return Err(AT_ESEAL);
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

/// 登记结果折算。
pub fn reg_cell(slot: u8, r: Result<usize, u32>) -> Sample {
    match r {
        Ok(v) => Sample {
            slot,
            op: OP_REG,
            ok: true,
            code: V_OK,
            value: v as u32,
        },
        Err(c) => Sample {
            slot,
            op: OP_REG,
            ok: false,
            code: c,
            value: 0,
        },
    }
}

/// 运行结果折算。
pub fn run_cell(slot: u8, r: Result<usize, u32>) -> Sample {
    match r {
        Ok(v) => Sample {
            slot,
            op: OP_RUN,
            ok: true,
            code: V_OK,
            value: v as u32,
        },
        Err(c) => Sample {
            slot,
            op: OP_RUN,
            ok: false,
            code: c,
            value: 0,
        },
    }
}

/// 构建十四格账本。
pub fn build_ledger() -> Ledger {
    let mut l = Ledger::new();
    let mut a = Atexit::new();

    let _ = l.register(reg_cell(0, a.register(1)));
    let _ = l.register(reg_cell(1, a.register(2)));
    let _ = l.register(reg_cell(2, a.register(3)));
    let _ = l.register(reg_cell(3, a.register(4)));
    let _ = l.register(cell(4, OP_COUNT, Ok(a.count() as u32)));
    let _ = l.register(run_cell(5, a.run()));

    let log = match a.log_at(2) {
        Ok(v) => Ok(v),
        Err(c) => Err(c),
    };
    let _ = l.register(cell(6, OP_LOG, log));
    let _ = l.register(cell(7, OP_LOG, Ok(a.log_len() as u32)));

    let mut full = Atexit::new();
    let mut i = 0;
    while i < CAP {
        let _ = full.register(i as u32 + 1);
        i += 1;
    }
    let _ = l.register(reg_cell(8, full.register(999)));

    let _ = l.register(reg_cell(9, a.register(5)));
    let _ = l.register(run_cell(10, a.run()));

    let mut e = Atexit::new();
    let _ = l.register(reg_cell(11, e.register(0)));

    let _ = l.register(cell(12, OP_LOG, match a.log_at(CAP) {
        Ok(v) => Ok(v),
        Err(c) => Err(c),
    }));

    let w = Atexit::new();
    let _ = l.register(cell(13, OP_LOG, match w.log_at(0) {
        Ok(v) => Ok(v),
        Err(c) => Err(c),
    }));

    l
}

/// 四路注入：账本重放、三十二层 LIFO、容量满不登记、闸门齐全。
pub fn inject_four() -> [bool; 4] {
    let mut out = [false; 4];

    let l = build_ledger();
    let base = l.len() == SAMPLES && l.audit().is_ok();
    let rep = l.replay();
    let root = l.seal();
    out[0] = base && rep == Ok(root) && root != 0;

    let mut a = Atexit::new();
    let mut i = 1u32;
    let mut ok = true;
    while (i as usize) <= CAP {
        if a.register(i).is_err() {
            ok = false;
        }
        i += 1;
    }
    let ran = a.run();
    let mut lifo = true;
    if ran == Ok(CAP) {
        let mut k = 0;
        while k < CAP {
            let want = (CAP - k) as u32;
            if a.log_at(k) != Ok(want) {
                lifo = false;
            }
            k += 1;
        }
    } else {
        lifo = false;
    }
    out[1] = ok && a.count() == CAP && ran == Ok(CAP) && lifo && a.log_len() == CAP;

    let mut f = Atexit::new();
    let mut j = 0;
    while j < CAP {
        let _ = f.register(j as u32 + 1);
        j += 1;
    }
    let mut used = Atexit::new();
    let _ = used.register(5);
    let _ = used.run();
    out[2] = f.register(77) == Err(AT_EFULL)
        && f.count() == CAP
        && f.pending(CAP - 1) == Some(CAP as u32)
        && f.run() == Ok(CAP)
        && f.register(78) == Err(AT_EFULL)
        && used.register(6) == Err(AT_EUSED);

    out[3] = gate_probe();

    out
}

/// 六道闸门逐一验证。
pub fn gate_probe() -> bool {
    let mut a = Atexit::new();
    if a.register(0) != Err(AT_EINVAL) {
        return false;
    }
    if a.register(7) != Ok(1) {
        return false;
    }
    if a.register(7) != Err(AT_EDUP) {
        return false;
    }
    if a.register(8) != Ok(2) {
        return false;
    }
    if a.log_at(0) != Err(AT_EWARM) {
        return false;
    }
    if a.run() != Ok(2) {
        return false;
    }
    if a.log_at(0) != Ok(8) || a.log_at(1) != Ok(7) {
        return false;
    }
    if a.log_at(2) != Err(AT_ENOBS) {
        return false;
    }
    if a.run() != Err(AT_ERUN) {
        return false;
    }
    if a.register(9) != Err(AT_EUSED) {
        return false;
    }
    let mut full = Atexit::new();
    let mut i = 0;
    while i < CAP {
        if full.register(i as u32 + 1) != Ok(i + 1) {
            return false;
        }
        i += 1;
    }
    full.register(500) == Err(AT_EFULL)
}

/// 十三项判据。
pub fn run_unx11321_checks() -> CheckSet {
    let mut s = CheckSet::new("unx11321.atexit");
    let l = build_ledger();

    s.add(
        "AT1-十四格齐备",
        l.len() == SAMPLES && !l.is_empty() && l.sample(0).map(|x| x.op) == Some(OP_REG),
        "static",
    );

    s.add_dyn(
        "AT2-八允六拒分布正确",
        l.ok_count() == 8 && l.err_count() == 6 && l.audit().is_ok(),
        &format!("ok={} err={}", l.ok_count(), l.err_count()),
    );

    s.add(
        "AT3-登记序号递增",
        l.sample(0).map(|x| x.value) == Some(1)
            && l.sample(1).map(|x| x.value) == Some(2)
            && l.sample(2).map(|x| x.value) == Some(3)
            && l.sample(3).map(|x| x.value) == Some(4),
        "static",
    );

    s.add(
        "AT4-逆序执行",
        {
            let mut a = Atexit::new();
            let _ = a.register(10);
            let _ = a.register(20);
            let _ = a.register(30);
            a.run() == Ok(3)
                && a.log_at(0) == Ok(30)
                && a.log_at(1) == Ok(20)
                && a.log_at(2) == Ok(10)
        },
        "static",
    );

    s.add(
        "AT5-运行仅一次",
        {
            let mut a = Atexit::new();
            let _ = a.register(1);
            a.run() == Ok(1) && a.run() == Err(AT_ERUN) && a.log_len() == 1
        },
        "static",
    );

    s.add(
        "AT6-容量闸门",
        l.sample(8).map(|x| x.code) == Some(AT_EFULL)
            && {
                let mut a = Atexit::new();
                let mut i = 0;
                while i < CAP {
                    if a.register(i as u32 + 1).is_err() {
                        return s;
                    }
                    i += 1;
                }
                a.register(CAP as u32 + 1) == Err(AT_EFULL) && a.count() == CAP
            },
        "static",
    );

    s.add(
        "AT7-运行后登记闸门",
        l.sample(9).map(|x| x.code) == Some(AT_EUSED)
            && {
                let mut a = Atexit::new();
                let _ = a.register(1);
                let _ = a.run();
                a.register(2) == Err(AT_EUSED) && a.count() == 1
            },
        "static",
    );

    s.add(
        "AT8-重复运行闸门",
        l.sample(10).map(|x| x.code) == Some(AT_ERUN),
        "static",
    );

    s.add(
        "AT9-非法函数号闸门",
        l.sample(11).map(|x| x.code) == Some(AT_EINVAL)
            && {
                let mut a = Atexit::new();
                a.register(0) == Err(AT_EINVAL) && a.is_empty()
            },
        "static",
    );

    s.add(
        "AT10-日志越界闸门",
        l.sample(12).map(|x| x.code) == Some(AT_ENOBS)
            && {
                let mut a = Atexit::new();
                let _ = a.register(1);
                let _ = a.run();
                a.log_at(1) == Err(AT_ENOBS) && a.log_at(CAP) == Err(AT_ENOBS)
            },
        "static",
    );

    s.add(
        "AT11-未运行取日志闸门",
        l.sample(13).map(|x| x.code) == Some(AT_EWARM)
            && {
                let mut a = Atexit::new();
                let _ = a.register(4);
                a.log_at(0) == Err(AT_EWARM)
            },
        "static",
    );

    s.add(
        "AT12-三十二层 LIFO",
        {
            let mut a = Atexit::new();
            let mut i = 1u32;
            let mut ok = true;
            while (i as usize) <= CAP {
                if a.register(i).is_err() {
                    ok = false;
                }
                i += 1;
            }
            let r = a.run();
            let mut k = 0;
            let mut order = true;
            while k < CAP {
                if a.log_at(k) != Ok((CAP - k) as u32) {
                    order = false;
                }
                k += 1;
            }
            ok && r == Ok(CAP) && order && a.count() == CAP
        },
        "static",
    );

    s.add(
        "AT13-冻结重放与四路注入",
        {
            let mut f = build_ledger();
            let rep = f.replay();
            let one = f.freeze();
            let two = f.freeze();
            let inj = inject_four();
            one.is_ok()
                && one == rep
                && two == Err(AT_EFROZEN)
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
        let s = run_unx11321_checks();
        assert_eq!(s.len(), 13);
        assert!(s.all_passed());
    }

    #[test]
    fn lifo_order() {
        let mut a = Atexit::new();
        let _ = a.register(1);
        let _ = a.register(2);
        let _ = a.register(3);
        assert_eq!(a.run(), Ok(3));
        assert_eq!(a.log_at(0), Ok(3));
        assert_eq!(a.log_at(2), Ok(1));
    }

    #[test]
    fn full_capacity_does_not_register() {
        let mut a = Atexit::new();
        let mut i = 0;
        while i < CAP {
            assert!(a.register(i as u32 + 1).is_ok());
            i += 1;
        }
        assert_eq!(a.register(9999), Err(AT_EFULL));
        assert_eq!(a.count(), CAP);
    }

    #[test]
    fn duplicate_func_rejected() {
        let mut a = Atexit::new();
        assert_eq!(a.register(5), Ok(1));
        assert_eq!(a.register(5), Err(AT_EDUP));
        assert_eq!(a.count(), 1);
    }

    #[test]
    fn warm_log_gate() {
        let mut a = Atexit::new();
        let _ = a.register(6);
        assert_eq!(a.log_at(0), Err(AT_EWARM));
        assert_eq!(a.log_len(), 0);
    }

    #[test]
    fn empty_run_is_zero() {
        let mut a = Atexit::new();
        assert_eq!(a.run(), Ok(0));
        assert_eq!(a.log_len(), 0);
        assert_eq!(a.log_at(0), Err(AT_ENOBS));
    }

    #[test]
    fn freeze_then_replay_matches() {
        let mut l = build_ledger();
        let root = l.seal();
        assert_eq!(l.freeze(), Ok(root));
        assert_eq!(l.replay(), Ok(root));
        assert_eq!(l.freeze(), Err(AT_EFROZEN));
    }
}
