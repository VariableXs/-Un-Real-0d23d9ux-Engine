//! UNX-F11500 B15 批 ktest 断言集聚合（0xf050 段）
//!
//! 一个 B15 批跑完，ktest 手里只剩一张张用例的计数：每张用例给出断言总数、
//! 失败数、跳过数，用例之间没有共享状态。聚合账要把这些散计数收成一张批账，
//! 回答四件事：断言总量多少、失败多少、跳过多少、整批该怎么判。
//!
//! 判据只有三档，次序不可交换：
//! - 只要有一笔失败，整批判负（`V_FAIL`），跳过数量再大也救不回来；
//! - 一笔失败都没有、且断言全被跳过，整批判阻（`V_BLOCK`）——没有一条断言
//!   真正执行过，"通过"是无凭据的；
//! - 其余情形判过（`V_PASS`）。
//!
//! 退出码是判据的投影，不是另一套规则：通过映射 0，失败映射 1，判阻**没有**
//! 退出码（`EBLOCK`）——把一批"没跑"的用例硬塞进 0 或 1 都是在编造结论。
//!
//! 入账闸门次序：已封批 → 表满 → 编号重复 → 断言为零 → 失败超出总数 →
//! 跳过超出"未被判负的余量"（`asserts - failed`）。封批是一次性的：封批后
//! 任何写入返回 `ECLOSED`，空批封批返回 `ENOCASE`。
//!
//! 账本十四格：八格登记成功路径（三张用例、判据、退出码、失败计数、重复编号
//! 的合法首笔、封批），六格登记各类拒绝（封批后写入、编号重复、零断言、失败
//! 越界、跳过越界、表满）。审计逐格复算 VALS/CODES 与槽位；封印把槽位、操作、
//! 判果、错误码、观测值连同拒绝计数折进 FNV-1a；冻结后拒绝登记并计数，重放
//! 重建同名账本比对指纹。

use crate::alloc::format;
use crate::checks::CheckSet;

/// 段基址。
pub const SEG_BASE: u32 = 0xf050;

/// 用例表满。
pub const KA_EFULL: u32 = SEG_BASE + 0x01;
/// 用例编号重复。
pub const KA_EDUP: u32 = SEG_BASE + 0x02;
/// 断言数为零。
pub const KA_EZERO: u32 = SEG_BASE + 0x03;
/// 失败数超出断言总数。
pub const KA_EFAULT: u32 = SEG_BASE + 0x04;
/// 跳过数超出未被判负的余量。
pub const KA_ESKIP: u32 = SEG_BASE + 0x05;
/// 空批不可判。
pub const KA_ENOCASE: u32 = SEG_BASE + 0x06;
/// 判阻没有退出码。
pub const KA_EBLOCK: u32 = SEG_BASE + 0x07;
/// 批已封，拒绝再改。
pub const KA_ECLOSED: u32 = SEG_BASE + 0x08;
/// 账本形状不符。
pub const KA_EBAL: u32 = SEG_BASE + 0x09;
/// 槽位乱序。
pub const KA_ESLOT: u32 = SEG_BASE + 0x0a;
/// 观测值与复算不符。
pub const KA_EVALUE: u32 = SEG_BASE + 0x0b;
/// 冻结后重放不一致。
pub const KA_ESEAL: u32 = SEG_BASE + 0x0c;
/// 观察下标越界。
pub const KA_ENOBS: u32 = SEG_BASE + 0x0d;

/// 用例表容量。
pub const CAP: usize = 12;
/// 账本格数。
pub const SAMPLES: usize = 14;

/// 批判据：通过。
pub const V_PASS: u8 = 0;
/// 批判据：判阻（全跳，无可信结论）。
pub const V_BLOCK: u8 = 1;
/// 批判据：判负。
pub const V_FAIL: u8 = 2;

/// 操作码：登记用例。
pub const OP_ADD: u8 = 1;
/// 操作码：求判据。
pub const OP_VERDICT: u8 = 2;
/// 操作码：求退出码。
pub const OP_EXIT: u8 = 3;
/// 操作码：读失败总量。
pub const OP_FAILED: u8 = 4;
/// 操作码：封批。
pub const OP_CLOSE: u8 = 5;

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

/// 一张用例的三笔计数。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Case {
    /// 用例编号（批内唯一，非零由调用方保证）。
    pub id: u16,
    /// 断言总数。
    pub asserts: u16,
    /// 失败断言数。
    pub failed: u16,
    /// 跳过断言数。
    pub skipped: u16,
}

/// 批账：定容用例表 + 封批标志。
#[derive(Clone, Copy, Debug)]
pub struct Batch {
    cases: [Case; CAP],
    n: usize,
    adds: u32,
    closed: bool,
}

impl Batch {
    /// 空批。
    pub const fn new() -> Batch {
        Batch {
            cases: [Case { id: 0, asserts: 0, failed: 0, skipped: 0 }; CAP],
            n: 0,
            adds: 0,
            closed: false,
        }
    }

    /// 用例条数。
    pub fn case_count(&self) -> usize {
        self.n
    }

    /// 第 i 张用例。
    pub fn case_at(&self, i: usize) -> Option<Case> {
        if i >= self.n {
            return None;
        }
        self.cases.get(i).copied()
    }

    /// 成功登记次数。
    pub fn add_count(&self) -> u32 {
        self.adds
    }

    /// 是否已封批。
    pub fn is_closed(&self) -> bool {
        self.closed
    }

    /// 断言总量。
    pub fn total_asserts(&self) -> u32 {
        let mut sum = 0u32;
        let mut i = 0usize;
        while i < self.n {
            match self.cases.get(i) {
                Some(c) => sum = sum.saturating_add(c.asserts as u32),
                None => return sum,
            }
            i += 1;
        }
        sum
    }

    /// 失败断言总量。
    pub fn total_failed(&self) -> u32 {
        let mut sum = 0u32;
        let mut i = 0usize;
        while i < self.n {
            match self.cases.get(i) {
                Some(c) => sum = sum.saturating_add(c.failed as u32),
                None => return sum,
            }
            i += 1;
        }
        sum
    }

    /// 跳过断言总量。
    pub fn total_skipped(&self) -> u32 {
        let mut sum = 0u32;
        let mut i = 0usize;
        while i < self.n {
            match self.cases.get(i) {
                Some(c) => sum = sum.saturating_add(c.skipped as u32),
                None => return sum,
            }
            i += 1;
        }
        sum
    }

    /// 登记一张用例，返回累计断言总数。
    pub fn add(&mut self, id: u16, asserts: u16, failed: u16, skipped: u16) -> Result<u32, u32> {
        if self.closed {
            return Err(KA_ECLOSED);
        }
        if self.n >= CAP {
            return Err(KA_EFULL);
        }
        let mut i = 0usize;
        while i < self.n {
            match self.cases.get(i) {
                Some(c) => {
                    if c.id == id {
                        return Err(KA_EDUP);
                    }
                }
                None => return Err(KA_EBAL),
            }
            i += 1;
        }
        if asserts == 0 {
            return Err(KA_EZERO);
        }
        if failed > asserts {
            return Err(KA_EFAULT);
        }
        if skipped > asserts - failed {
            return Err(KA_ESKIP);
        }
        if let Some(dst) = self.cases.get_mut(self.n) {
            *dst = Case { id, asserts, failed, skipped };
        }
        self.n += 1;
        self.adds = self.adds.saturating_add(1);
        Ok(self.total_asserts())
    }

    /// 整批判据；空批不可判。
    pub fn verdict(&self) -> Result<u8, u32> {
        if self.n == 0 {
            return Err(KA_ENOCASE);
        }
        if self.total_failed() > 0 {
            return Ok(V_FAIL);
        }
        if self.total_asserts() == self.total_skipped() {
            return Ok(V_BLOCK);
        }
        Ok(V_PASS)
    }

    /// 退出码；判阻没有退出码。
    pub fn exit_code(&self) -> Result<u32, u32> {
        match self.verdict() {
            Ok(V_PASS) => Ok(0),
            Ok(V_FAIL) => Ok(1),
            Ok(_) => Err(KA_EBLOCK),
            Err(c) => Err(c),
        }
    }

    /// 封批并返回断言总量。
    pub fn close(&mut self) -> Result<u32, u32> {
        if self.closed {
            return Err(KA_ECLOSED);
        }
        if self.n == 0 {
            return Err(KA_ENOCASE);
        }
        self.closed = true;
        Ok(self.total_asserts())
    }

    /// 不变式自检。
    pub fn consistent(&self) -> bool {
        if self.n > CAP {
            return false;
        }
        if self.adds as usize != self.n {
            return false;
        }
        if self.closed && self.n == 0 {
            return false;
        }
        let mut i = 0usize;
        while i < self.n {
            let c = match self.cases.get(i) {
                Some(v) => *v,
                None => return false,
            };
            if c.asserts == 0 || c.failed > c.asserts {
                return false;
            }
            if c.skipped > c.asserts - c.failed {
                return false;
            }
            i += 1;
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
            return Err(KA_ESLOT);
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
            return Err(KA_EBAL);
        }
        const VALS: [u32; 8] = [24, 32, 48, 2, 1, 9, 4, 48];
        const CODES: [u32; 6] =
            [KA_ECLOSED, KA_EDUP, KA_EZERO, KA_EFAULT, KA_ESKIP, KA_EFULL];
        let mut vi = 0usize;
        let mut ci = 0usize;
        let mut i = 0usize;
        while i < self.len {
            let s = match self.rows.get(i) {
                Some(v) => *v,
                None => return Err(KA_EBAL),
            };
            if s.slot as usize != i {
                return Err(KA_ESLOT);
            }
            if s.op == 0 {
                return Err(KA_EVALUE);
            }
            if s.ok {
                if s.code != 0 {
                    return Err(KA_EVALUE);
                }
                match VALS.get(vi) {
                    Some(w) => {
                        if s.value != *w {
                            return Err(KA_EVALUE);
                        }
                    }
                    None => return Err(KA_EBAL),
                }
                vi += 1;
            } else {
                if s.value != 0 {
                    return Err(KA_EVALUE);
                }
                match CODES.get(ci) {
                    Some(w) => {
                        if s.code != *w {
                            return Err(KA_EVALUE);
                        }
                    }
                    None => return Err(KA_EBAL),
                }
                ci += 1;
            }
            i += 1;
        }
        if vi != 8 || ci != 6 {
            return Err(KA_EBAL);
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
            return Err(KA_EBAL);
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
            Err(KA_ESEAL)
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

/// 十四格账本：三张用例 → 判据 → 退出码 → 失败量 → 合法首笔 → 封批 →
/// 六类拒绝。
pub fn build_ledger() -> Ledger {
    let mut l = Ledger::new();
    let mut b = Batch::new();

    let _ = l.register(word_cell(0, OP_ADD, b.add(1, 24, 0, 2)));
    let _ = l.register(word_cell(1, OP_ADD, b.add(2, 8, 8, 0)));
    let _ = l.register(word_cell(2, OP_ADD, b.add(3, 16, 1, 1)));
    let _ = l.register(word_cell(3, OP_VERDICT, b.verdict().map(|v| v as u32)));
    let _ = l.register(word_cell(4, OP_EXIT, b.exit_code()));
    let _ = l.register(word_cell(5, OP_FAILED, Ok(b.total_failed())));

    let mut d = Batch::new();
    let _ = l.register(word_cell(6, OP_ADD, d.add(7, 4, 0, 0)));
    let _ = l.register(word_cell(7, OP_CLOSE, b.close()));

    let _ = l.register(word_cell(8, OP_ADD, b.add(4, 4, 0, 0)));
    let _ = l.register(word_cell(9, OP_ADD, d.add(7, 4, 0, 0)));
    let _ = l.register(word_cell(10, OP_ADD, d.add(8, 0, 0, 0)));
    let _ = l.register(word_cell(11, OP_ADD, d.add(9, 4, 5, 0)));
    let _ = l.register(word_cell(12, OP_ADD, d.add(10, 4, 3, 2)));

    let mut e = Batch::new();
    let mut i = 1u16;
    while (i as usize) <= CAP {
        let _ = e.add(i, 1, 0, 0);
        i += 1;
    }
    let _ = l.register(word_cell(13, OP_ADD, e.add(99, 1, 0, 0)));

    l
}

/// 四路注入。
pub fn inject_four() -> [bool; 4] {
    let mut out = [false; 4];

    let l = build_ledger();
    let rep = l.replay();
    let root = l.seal();
    out[0] = l.len() == SAMPLES && l.audit().is_ok() && rep == Ok(root) && root != 0;

    // 一路：干净批全绿，判过且退出码为零。
    let mut a = Batch::new();
    let t1 = a.add(1, 10, 0, 0);
    let t2 = a.add(2, 6, 0, 1);
    out[1] = t1 == Ok(10)
        && t2 == Ok(16)
        && a.total_asserts() == 16
        && a.total_failed() == 0
        && a.total_skipped() == 1
        && a.verdict() == Ok(V_PASS)
        && a.exit_code() == Ok(0)
        && a.close() == Ok(16)
        && a.consistent();

    // 二路：判据次序——失败压过跳过，全跳判阻，空批不判。
    let mut f = Batch::new();
    let _ = f.add(1, 4, 2, 0);
    let _ = f.add(2, 4, 0, 1);
    let fail_dom = f.verdict() == Ok(V_FAIL) && f.exit_code() == Ok(1);
    let mut s = Batch::new();
    let _ = s.add(1, 5, 0, 5);
    let blocked = s.verdict() == Ok(V_BLOCK) && s.exit_code() == Err(KA_EBLOCK);
    let mut empty = Batch::new();
    out[2] = fail_dom
        && blocked
        && empty.verdict() == Err(KA_ENOCASE)
        && empty.exit_code() == Err(KA_ENOCASE)
        && empty.close() == Err(KA_ENOCASE)
        && f.consistent()
        && s.consistent();

    // 三路：五道入账闸门各自可辨，拒绝不改账。
    let mut g = Batch::new();
    let _ = g.add(1, 4, 0, 0);
    let dup = g.add(1, 4, 0, 0) == Err(KA_EDUP);
    let zero = g.add(2, 0, 0, 0) == Err(KA_EZERO);
    let over = g.add(2, 4, 5, 0) == Err(KA_EFAULT);
    let skip = g.add(2, 4, 3, 2) == Err(KA_ESKIP);
    let unchanged = g.case_count() == 1 && g.total_asserts() == 4;
    let mut h = Batch::new();
    let mut i = 1u16;
    while (i as usize) <= CAP {
        let _ = h.add(i, 2, 0, 0);
        i += 1;
    }
    let full = h.case_count() == CAP && h.add(30, 1, 0, 0) == Err(KA_EFULL);
    let _ = g.close();
    let closed = g.add(9, 4, 0, 0) == Err(KA_ECLOSED);
    out[3] = dup && zero && over && skip && unchanged && full && closed && g.consistent();

    out
}

/// 十三项判据。
pub fn run_unx11500_checks() -> CheckSet {
    let mut s = CheckSet::new("unx11500.ktest_assert_agg");
    let l = build_ledger();

    s.add_dyn(
        "KA01-十四格八允六拒",
        l.len() == SAMPLES && l.ok_count() == 8 && l.err_count() == 6 && l.audit().is_ok(),
        &format!("n={} ok={} err={}", l.len(), l.ok_count(), l.err_count()),
    );

    s.add(
        "KA02-断言总量逐笔累加",
        l.sample(0).map(|x| x.value) == Some(24)
            && l.sample(1).map(|x| x.value) == Some(32)
            && l.sample(2).map(|x| x.value) == Some(48),
        "static",
    );

    s.add(
        "KA03-失败主导判负",
        {
            let mut b = Batch::new();
            let _ = b.add(1, 20, 1, 4);
            let _ = b.add(2, 4, 0, 0);
            b.verdict() == Ok(V_FAIL) && b.exit_code() == Ok(1) && b.total_failed() == 1
        },
        "static",
    );

    s.add(
        "KA04-通过映射零",
        {
            let mut b = Batch::new();
            let _ = b.add(1, 3, 0, 0);
            b.verdict() == Ok(V_PASS) && b.exit_code() == Ok(0) && l.sample(4).map(|x| x.value) == Some(1)
        },
        "static",
    );

    s.add(
        "KA05-重复编号被拒",
        {
            let mut b = Batch::new();
            let _ = b.add(5, 8, 0, 0);
            let before = b.total_asserts();
            let r = b.add(5, 8, 0, 0);
            r == Err(KA_EDUP)
                && b.case_count() == 1
                && b.total_asserts() == before
                && l.sample(9).map(|x| x.code) == Some(KA_EDUP)
        },
        "static",
    );

    s.add(
        "KA06-零断言被拒",
        {
            let mut b = Batch::new();
            b.add(1, 0, 0, 0) == Err(KA_EZERO)
                && b.case_count() == 0
                && l.sample(10).map(|x| x.code) == Some(KA_EZERO)
        },
        "static",
    );

    s.add(
        "KA07-失败数越界被拒",
        {
            let mut b = Batch::new();
            b.add(1, 4, 5, 0) == Err(KA_EFAULT)
                && b.add(1, 4, 4, 0) == Ok(4)
                && l.sample(11).map(|x| x.code) == Some(KA_EFAULT)
        },
        "static",
    );

    s.add(
        "KA08-跳过数越界被拒",
        {
            let mut b = Batch::new();
            b.add(1, 4, 1, 4) == Err(KA_ESKIP)
                && b.add(1, 4, 1, 3) == Ok(4)
                && l.sample(12).map(|x| x.code) == Some(KA_ESKIP)
        },
        "static",
    );

    s.add(
        "KA09-封批后拒改",
        {
            let mut b = Batch::new();
            let _ = b.add(1, 2, 0, 0);
            let done = b.close();
            let again = b.close();
            done == Ok(2)
                && again == Err(KA_ECLOSED)
                && b.is_closed()
                && b.add(2, 2, 0, 0) == Err(KA_ECLOSED)
                && l.sample(8).map(|x| x.code) == Some(KA_ECLOSED)
        },
        "static",
    );

    s.add(
        "KA10-空批不判",
        {
            let b = Batch::new();
            let mut c = Batch::new();
            b.verdict() == Err(KA_ENOCASE)
                && b.exit_code() == Err(KA_ENOCASE)
                && c.close() == Err(KA_ENOCASE)
                && !c.is_closed()
        },
        "static",
    );

    s.add(
        "KA11-全跳判阻",
        {
            let mut b = Batch::new();
            let _ = b.add(1, 6, 0, 6);
            let _ = b.add(2, 0 + 2, 0, 2);
            b.verdict() == Ok(V_BLOCK)
                && b.exit_code() == Err(KA_EBLOCK)
                && b.total_skipped() == 8
                && b.consistent()
        },
        "static",
    );

    s.add(
        "KA12-表满边界",
        {
            let mut b = Batch::new();
            let mut i = 1u16;
            let mut last = Ok(0);
            while (i as usize) <= CAP {
                last = b.add(i, 3, 0, 0);
                i += 1;
            }
            last == Ok((CAP as u32) * 3)
                && b.case_count() == CAP
                && b.add(50, 3, 0, 0) == Err(KA_EFULL)
                && b.total_asserts() == (CAP as u32) * 3
                && l.sample(13).map(|x| x.code) == Some(KA_EFULL)
        },
        "static",
    );

    s.add(
        "KA13-冻结重放四注入",
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
        let s = run_unx11500_checks();
        assert_eq!(s.len(), 13);
        assert!(s.all_passed());
    }

    #[test]
    fn accumulate_totals() {
        let mut b = Batch::new();
        assert_eq!(b.add(1, 10, 1, 2), Ok(10));
        assert_eq!(b.add(2, 5, 0, 0), Ok(15));
        assert_eq!(b.total_asserts(), 15);
        assert_eq!(b.total_failed(), 1);
        assert_eq!(b.total_skipped(), 2);
        assert_eq!(b.verdict(), Ok(V_FAIL));
        assert_eq!(b.exit_code(), Ok(1));
        assert!(b.consistent());
    }

    #[test]
    fn verdict_order() {
        let mut f = Batch::new();
        assert_eq!(f.add(1, 4, 1, 3), Ok(4));
        assert_eq!(f.verdict(), Ok(V_FAIL));
        let mut s = Batch::new();
        assert_eq!(s.add(1, 4, 0, 4), Ok(4));
        assert_eq!(s.verdict(), Ok(V_BLOCK));
        assert_eq!(s.exit_code(), Err(KA_EBLOCK));
        let mut p = Batch::new();
        assert_eq!(p.add(1, 4, 0, 3), Ok(4));
        assert_eq!(p.verdict(), Ok(V_PASS));
        assert_eq!(p.exit_code(), Ok(0));
    }

    #[test]
    fn gate_order_and_stability() {
        let mut b = Batch::new();
        assert_eq!(b.add(1, 4, 0, 0), Ok(4));
        assert_eq!(b.add(1, 0, 0, 0), Err(KA_EDUP));
        assert_eq!(b.add(2, 0, 0, 0), Err(KA_EZERO));
        assert_eq!(b.add(2, 4, 5, 0), Err(KA_EFAULT));
        assert_eq!(b.add(2, 4, 3, 2), Err(KA_ESKIP));
        assert_eq!(b.case_count(), 1);
        assert_eq!(b.total_asserts(), 4);
        assert!(b.consistent());
    }

    #[test]
    fn capacity_boundary() {
        let mut b = Batch::new();
        let mut i = 1u16;
        while (i as usize) <= CAP {
            assert_eq!(b.add(i, 1, 0, 0), Ok(i as u32));
            i += 1;
        }
        assert_eq!(b.add(99, 1, 0, 0), Err(KA_EFULL));
        assert_eq!(b.case_count(), CAP);
    }

    #[test]
    fn close_empty_and_twice() {
        let mut b = Batch::new();
        assert_eq!(b.close(), Err(KA_ENOCASE));
        let _ = b.add(1, 1, 0, 0);
        assert_eq!(b.close(), Ok(1));
        assert_eq!(b.close(), Err(KA_ECLOSED));
        assert_eq!(b.add(2, 1, 0, 0), Err(KA_ECLOSED));
        assert!(b.is_closed());
    }

    #[test]
    fn inject_four_holds() {
        assert_eq!(inject_four(), [true; 4]);
    }
}
