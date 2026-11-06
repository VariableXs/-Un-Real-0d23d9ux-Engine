//! UNX-F11320 B06 批 ktest 断言集聚合（0xea00 段）
//!
//! 一批模块各带十三项判据，散落在各自文件里就是一堆孤立结论；只有把它们
//! 注册进同一个断言面、一次命令全跑，批级质量才有单一数字。聚合面的价值
//! 在于“顺序与唯一性”本身：编号必须落在本批区间内、必须严格递增、不得
//! 重复，否则跑出来的 100% 是假的——漏掉一条与多跑一条在计数上无法区分。
//!
//! 模型把注册表做成三道闸：区间（EUID）、递增（EORDER）、重复（EDUP），
//! 加上容量闸（EFULL）。汇总只产出四元组（总数/通过/失败/百分比），
//! `gate()` 要求失败为零才给出百分比，否则 EGATE——这条规则防止“94% 也
//! 算过”的松弛口径。
//!
//! 账本十四格：八格成功（四次登记、两次汇总、一次放行、一次续登），六格
//! 拒绝（重复编号、逆序编号、区间外编号、容量满、有失败不放行、空面汇总）。

use crate::alloc::format;
use crate::checks::CheckSet;

/// 段基址。
pub const SEG_BASE: u32 = 0xea00;

/// 编号非法。
pub const AG_EID: u32 = SEG_BASE + 0x01;
/// 登记顺序错误。
pub const AG_EORDER: u32 = SEG_BASE + 0x02;
/// 重复登记。
pub const AG_EDUP: u32 = SEG_BASE + 0x03;
/// 已冻结。
pub const AG_EFROZEN: u32 = SEG_BASE + 0x04;
/// 观测不存在的格。
pub const AG_ENOBS: u32 = SEG_BASE + 0x05;
/// 结论不符。
pub const AG_EMISMATCH: u32 = SEG_BASE + 0x06;
/// 账本失衡。
pub const AG_EBAL: u32 = SEG_BASE + 0x07;
/// 编号越出本批区间。
pub const AG_EUID: u32 = SEG_BASE + 0x08;
/// 容量满。
pub const AG_EFULL: u32 = SEG_BASE + 0x09;
/// 存在失败判据。
pub const AG_EGATE: u32 = SEG_BASE + 0x0a;
/// 空面汇总。
pub const AG_EEMPTY: u32 = SEG_BASE + 0x0b;
/// 票号不一致。
pub const AG_ESEAL: u32 = SEG_BASE + 0x0c;

/// 判决：成功。
pub const V_OK: u32 = 0;

/// 操作：登记。
pub const OP_REG: u8 = 1;
/// 操作：汇总。
pub const OP_RUN: u8 = 2;
/// 操作：放行。
pub const OP_GATE: u8 = 3;

/// 编号下界。
pub const LO: u32 = 11301;
/// 编号上界。
pub const HI: u32 = 11319;
/// 批内条数。
pub const WANT: u32 = 19;
/// 注册表容量。
pub const CAP: usize = 24;
/// 样本格数。
pub const SAMPLES: usize = 14;

/// FNV 偏移基。
pub const FNV_OFF: u64 = 0xcbf2_9ce4_8422_2325;
/// FNV 质数。
pub const FNV_PRIME: u64 = 0x100_0000_01b3;

/// 一条判据登记。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Entry {
    pub id: u32,
    pub passed: bool,
}

impl Entry {
    /// 空条。
    pub const fn blank() -> Entry {
        Entry {
            id: 0,
            passed: false,
        }
    }
}

/// 汇总四元组。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Summary {
    pub total: u32,
    pub passed: u32,
    pub failed: u32,
    pub pct: u32,
}

impl Summary {
    /// 空汇总。
    pub const fn blank() -> Summary {
        Summary {
            total: 0,
            passed: 0,
            failed: 0,
            pct: 0,
        }
    }
}

/// 断言聚合面。
#[derive(Clone, Copy, Debug)]
pub struct Registry {
    ents: [Entry; CAP],
    n: usize,
    cap: usize,
    runs: u32,
}

impl Registry {
    /// 默认容量。
    pub const fn new() -> Registry {
        Registry {
            ents: [Entry::blank(); CAP],
            n: 0,
            cap: CAP,
            runs: 0,
        }
    }

    /// 指定容量。
    pub const fn with_cap(cap: usize) -> Registry {
        Registry {
            ents: [Entry::blank(); CAP],
            n: 0,
            cap,
            runs: 0,
        }
    }

    /// 条数。
    pub fn len(&self) -> usize {
        self.n
    }

    /// 是否为空。
    pub fn is_empty(&self) -> bool {
        self.n == 0
    }

    /// 汇总次数。
    pub fn runs(&self) -> u32 {
        self.runs
    }

    /// 取条。
    pub fn get(&self, i: usize) -> Option<Entry> {
        if i >= self.n {
            return None;
        }
        self.ents.get(i).copied()
    }

    /// 末尾编号。
    pub fn last_id(&self) -> u32 {
        if self.n == 0 {
            return 0;
        }
        match self.ents.get(self.n - 1) {
            Some(e) => e.id,
            None => 0,
        }
    }

    /// 登记：区间 → 容量 → 递增 → 重复。
    pub fn register(&mut self, id: u32, passed: bool) -> Result<usize, u32> {
        if id < LO || id > HI {
            return Err(AG_EUID);
        }
        if self.n >= self.cap {
            return Err(AG_EFULL);
        }
        if self.n > 0 {
            let last = self.last_id();
            if id == last {
                return Err(AG_EDUP);
            }
            if id < last {
                return Err(AG_EORDER);
            }
        }
        match self.ents.get_mut(self.n) {
            Some(x) => *x = Entry { id, passed },
            None => return Err(AG_EFULL),
        }
        self.n += 1;
        Ok(self.n)
    }

    /// 汇总：空面拒绝。
    pub fn run(&mut self) -> Result<Summary, u32> {
        if self.n == 0 {
            return Err(AG_EEMPTY);
        }
        let mut p = 0u32;
        let mut i = 0;
        while i < self.n {
            match self.ents.get(i) {
                Some(e) => {
                    if e.passed {
                        p += 1;
                    }
                }
                None => return Err(AG_EEMPTY),
            }
            i += 1;
        }
        self.runs += 1;
        let total = self.n as u32;
        Ok(Summary {
            total,
            passed: p,
            failed: total - p,
            pct: p * 100 / total,
        })
    }

    /// 放行：必须全绿。
    pub fn gate(&mut self) -> Result<u32, u32> {
        let s = self.run()?;
        if s.failed != 0 {
            return Err(AG_EGATE);
        }
        Ok(s.pct)
    }

    /// 覆盖率：登记条数相对批内应有条数。
    pub fn coverage(&self) -> u32 {
        (self.n as u32) * 100 / WANT
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
            return Err(AG_EFROZEN);
        }
        if (row.slot as usize) != self.len {
            return Err(AG_EORDER);
        }
        if self.len >= SAMPLES {
            return Err(AG_EID);
        }
        let mut i = 0;
        while i < self.len {
            match self.rows.get(i) {
                Some(x) => {
                    if x.slot == row.slot {
                        return Err(AG_EDUP);
                    }
                }
                None => return Err(AG_ENOBS),
            }
            i += 1;
        }
        match self.rows.get_mut(self.len) {
            Some(x) => *x = row,
            None => return Err(AG_EID),
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
            return Err(AG_EBAL);
        }
        if self.ok_count() != 8 || self.err_count() != 6 {
            return Err(AG_EBAL);
        }
        let mut i = 0;
        while i < self.len {
            match self.rows.get(i) {
                Some(s) => {
                    if s.slot as usize != i {
                        return Err(AG_EORDER);
                    }
                    if s.ok {
                        if s.code != V_OK {
                            return Err(AG_EMISMATCH);
                        }
                    } else if s.code == V_OK || s.value != 0 {
                        return Err(AG_EMISMATCH);
                    }
                }
                None => return Err(AG_ENOBS),
            }
            i += 1;
        }
        let vals = [1u32, 2, 3, 4, 4, 100, 5, 5];
        let mut k = 0;
        while k < vals.len() {
            match (self.sample(k), vals.get(k)) {
                (Some(s), Some(w)) => {
                    if !s.ok || s.value != *w {
                        return Err(AG_EMISMATCH);
                    }
                }
                _ => return Err(AG_EBAL),
            }
            k += 1;
        }
        let codes = [AG_EDUP, AG_EORDER, AG_EUID, AG_EFULL, AG_EGATE, AG_EEMPTY];
        let mut c = 0;
        let mut j = 8;
        while j < self.len {
            match self.rows.get(j) {
                Some(s) => match codes.get(c) {
                    Some(w) => {
                        if s.code != *w {
                            return Err(AG_EMISMATCH);
                        }
                    }
                    None => return Err(AG_EBAL),
                },
                None => return Err(AG_ENOBS),
            }
            c += 1;
            j += 1;
        }
        if c != codes.len() {
            return Err(AG_EBAL);
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
            return Err(AG_EFROZEN);
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
            return Err(AG_EBAL);
        }
        let h = fresh.seal();
        if self.frozen && h != self.seal {
            return Err(AG_ESEAL);
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

/// 汇总结果取总数。
pub fn run_cell(slot: u8, r: Result<Summary, u32>) -> Sample {
    match r {
        Ok(s) => Sample {
            slot,
            op: OP_RUN,
            ok: true,
            code: V_OK,
            value: s.total,
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
    let mut r = Registry::new();

    let _ = l.register(reg_cell(0, r.register(11307, true)));
    let _ = l.register(reg_cell(1, r.register(11308, true)));
    let _ = l.register(reg_cell(2, r.register(11309, true)));
    let _ = l.register(reg_cell(3, r.register(11310, true)));
    let _ = l.register(run_cell(4, r.run()));
    let _ = l.register(cell(5, OP_GATE, r.gate()));
    let _ = l.register(reg_cell(6, r.register(11311, true)));
    let _ = l.register(cell(7, OP_RUN, match r.run() {
        Ok(s) => Ok(s.passed),
        Err(c) => Err(c),
    }));

    let _ = l.register(reg_cell(8, r.register(11311, true)));
    let _ = l.register(reg_cell(9, r.register(11305, true)));
    let _ = l.register(reg_cell(10, r.register(11200, true)));

    let mut small = Registry::with_cap(8);
    let mut i = 0;
    while i < 8 {
        let _ = small.register(11307 + i as u32, true);
        i += 1;
    }
    let _ = l.register(reg_cell(11, small.register(11315, true)));

    let mut dirty = Registry::new();
    let _ = dirty.register(11301, true);
    let _ = dirty.register(11302, false);
    let _ = l.register(cell(12, OP_GATE, dirty.gate()));

    let mut empty = Registry::new();
    let _ = l.register(run_cell(13, empty.run()));

    l
}

/// 四路注入：账本重放、十九条全覆盖、单失败不放行、闸门齐全。
pub fn inject_four() -> [bool; 4] {
    let mut out = [false; 4];

    let l = build_ledger();
    let base = l.len() == SAMPLES && l.audit().is_ok();
    let rep = l.replay();
    let root = l.seal();
    out[0] = base && rep == Ok(root) && root != 0;

    let mut full = Registry::new();
    let mut i = 0;
    let mut ok = true;
    while i < WANT {
        if full.register(LO + i, true).is_err() {
            ok = false;
        }
        i += 1;
    }
    out[1] = ok
        && full.len() as u32 == WANT
        && full.coverage() == 100
        && matches!(full.run(), Ok(Summary { total: 19, passed: 19, failed: 0, pct: 100 }))
        && full.gate() == Ok(100);

    let mut dirty = Registry::new();
    let mut i2 = 0;
    while i2 < WANT {
        let _ = dirty.register(LO + i2, i2 != 7);
        i2 += 1;
    }
    out[2] = match dirty.run() {
        Ok(s) => {
            s.total == 19 && s.passed == 18 && s.failed == 1 && s.pct == 94 && s.pct < 100
        }
        Err(_) => false,
    } && dirty.gate() == Err(AG_EGATE);

    out[3] = gate_probe();

    out
}

/// 六道闸门逐一验证。
pub fn gate_probe() -> bool {
    let mut r = Registry::new();
    if r.register(11305, true).is_err() {
        return false;
    }
    if r.register(11305, true) != Err(AG_EDUP) {
        return false;
    }
    if r.register(11304, true) != Err(AG_EORDER) {
        return false;
    }
    if r.register(HI + 1, true) != Err(AG_EUID) {
        return false;
    }
    if r.register(LO - 1, true) != Err(AG_EUID) {
        return false;
    }
    if Registry::new().run() != Err(AG_EEMPTY) {
        return false;
    }
    if Registry::new().gate() != Err(AG_EEMPTY) {
        return false;
    }
    let mut small = Registry::with_cap(2);
    if small.register(11301, true).is_err() || small.register(11302, true).is_err() {
        return false;
    }
    if small.register(11303, true) != Err(AG_EFULL) {
        return false;
    }
    let mut one = Registry::new();
    if one.register(11301, false).is_err() {
        return false;
    }
    if one.run() != Ok(Summary {
        total: 1,
        passed: 0,
        failed: 1,
        pct: 0,
    }) {
        return false;
    }
    if one.gate() != Err(AG_EGATE) {
        return false;
    }
    one.register(11302, true) == Ok(2) && one.gate() == Err(AG_EGATE)
}

/// 十三项判据。
pub fn run_unx11320_checks() -> CheckSet {
    let mut s = CheckSet::new("unx11320.ktestagg");
    let l = build_ledger();

    s.add(
        "AG1-十四格齐备",
        l.len() == SAMPLES && !l.is_empty() && l.sample(0).map(|x| x.op) == Some(OP_REG),
        "static",
    );

    s.add_dyn(
        "AG2-八允六拒分布正确",
        l.ok_count() == 8 && l.err_count() == 6 && l.audit().is_ok(),
        &format!("ok={} err={}", l.ok_count(), l.err_count()),
    );

    s.add(
        "AG3-登记计数递增",
        l.sample(0).map(|x| x.value) == Some(1)
            && l.sample(1).map(|x| x.value) == Some(2)
            && l.sample(2).map(|x| x.value) == Some(3)
            && l.sample(3).map(|x| x.value) == Some(4),
        "static",
    );

    s.add(
        "AG4-汇总四元组",
        {
            let mut r = Registry::new();
            let _ = r.register(11301, true);
            let _ = r.register(11302, true);
            let _ = r.register(11303, false);
            r.run()
                == Ok(Summary {
                    total: 3,
                    passed: 2,
                    failed: 1,
                    pct: 66,
                })
        },
        "static",
    );

    s.add(
        "AG5-全绿放行",
        {
            let mut r = Registry::new();
            let _ = r.register(11301, true);
            let _ = r.register(11302, true);
            r.gate() == Ok(100) && r.runs() == 1
        },
        "static",
    );

    s.add(
        "AG6-单失败不放行",
        l.sample(12).map(|x| x.code) == Some(AG_EGATE)
            && {
                let mut r = Registry::new();
                let _ = r.register(11301, true);
                let _ = r.register(11302, false);
                r.gate() == Err(AG_EGATE) && r.len() == 2
            },
        "static",
    );

    s.add(
        "AG7-重复编号闸门",
        l.sample(8).map(|x| x.code) == Some(AG_EDUP)
            && {
                let mut r = Registry::new();
                r.register(11301, true) == Ok(1) && r.register(11301, false) == Err(AG_EDUP)
            },
        "static",
    );

    s.add(
        "AG8-逆序编号闸门",
        l.sample(9).map(|x| x.code) == Some(AG_EORDER)
            && {
                let mut r = Registry::new();
                let _ = r.register(11310, true);
                r.register(11309, true) == Err(AG_EORDER) && r.len() == 1
            },
        "static",
    );

    s.add(
        "AG9-区间闸门",
        l.sample(10).map(|x| x.code) == Some(AG_EUID)
            && {
                let mut r = Registry::new();
                r.register(LO - 1, true) == Err(AG_EUID)
                    && r.register(HI + 1, true) == Err(AG_EUID)
                    && r.register(LO, true) == Ok(1)
                    && r.register(HI, true) == Ok(2)
            },
        "static",
    );

    s.add(
        "AG10-容量闸门",
        l.sample(11).map(|x| x.code) == Some(AG_EFULL)
            && {
                let mut r = Registry::with_cap(3);
                let _ = r.register(11301, true);
                let _ = r.register(11302, true);
                let _ = r.register(11303, true);
                r.register(11304, true) == Err(AG_EFULL) && r.len() == 3
            },
        "static",
    );

    s.add(
        "AG11-空面汇总闸门",
        l.sample(13).map(|x| x.code) == Some(AG_EEMPTY)
            && Registry::new().run() == Err(AG_EEMPTY)
            && Registry::new().gate() == Err(AG_EEMPTY),
        "static",
    );

    s.add(
        "AG12-十九条全覆盖",
        {
            let mut r = Registry::new();
            let mut i = 0;
            let mut ok = true;
            while i < WANT {
                if r.register(LO + i, true).is_err() {
                    ok = false;
                }
                i += 1;
            }
            ok && r.len() as u32 == WANT
                && r.coverage() == 100
                && r.last_id() == HI
                && matches!(r.run(), Ok(Summary { total: 19, passed: 19, failed: 0, pct: 100 }))
        },
        "static",
    );

    s.add(
        "AG13-冻结重放与四路注入",
        {
            let mut f = build_ledger();
            let rep = f.replay();
            let one = f.freeze();
            let two = f.freeze();
            let inj = inject_four();
            one.is_ok()
                && one == rep
                && two == Err(AG_EFROZEN)
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
        let s = run_unx11320_checks();
        assert_eq!(s.len(), 13);
        assert!(s.all_passed());
    }

    #[test]
    fn register_enforces_order() {
        let mut r = Registry::new();
        assert_eq!(r.register(11305, true), Ok(1));
        assert_eq!(r.register(11306, false), Ok(2));
        assert_eq!(r.register(11306, true), Err(AG_EDUP));
        assert_eq!(r.register(11304, true), Err(AG_EORDER));
        assert_eq!(r.len(), 2);
    }

    #[test]
    fn summary_math() {
        let mut r = Registry::new();
        let mut i = 0;
        while i < 3 {
            let _ = r.register(11301 + i, i != 0);
            i += 1;
        }
        let s = r.run().unwrap_or(Summary::blank());
        assert_eq!(s.total, 3);
        assert_eq!(s.passed, 2);
        assert_eq!(s.failed, 1);
        assert_eq!(s.pct, 66);
    }

    #[test]
    fn gate_requires_zero_failures() {
        let mut r = Registry::new();
        let _ = r.register(11301, true);
        assert_eq!(r.gate(), Ok(100));
        let _ = r.register(11302, false);
        assert_eq!(r.gate(), Err(AG_EGATE));
    }

    #[test]
    fn coverage_is_count_over_want() {
        let mut r = Registry::new();
        let _ = r.register(11301, true);
        assert_eq!(r.coverage(), 5);
        assert!(r.coverage() < 100);
    }

    #[test]
    fn range_gate() {
        let mut r = Registry::new();
        assert_eq!(r.register(0, true), Err(AG_EUID));
        assert_eq!(r.register(99999, true), Err(AG_EUID));
        assert_eq!(r.register(HI, true), Ok(1));
    }

    #[test]
    fn freeze_then_replay_matches() {
        let mut l = build_ledger();
        let root = l.seal();
        assert_eq!(l.freeze(), Ok(root));
        assert_eq!(l.replay(), Ok(root));
        assert_eq!(l.freeze(), Err(AG_EFROZEN));
    }
}
