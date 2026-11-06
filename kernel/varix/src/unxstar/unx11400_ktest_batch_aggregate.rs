//! UNX-F11400 B10 批 ktest 断言集聚合（0xef00 段）
//!
//! ktest 生态面要的不是「每条判据各自能跑」，而是**一条命令跑完整批**：
//! 19 条断言一次执行、一次汇总，通过率必须恰好 100% 才算绿。聚合器因此必须
//! 对「一批」有严格定义：
//!
//! - 断言编号从 1 连续到 19，中间不许缺号。缺号意味着批量结果不可复现；
//! - 每条断言登记时声明期望值，执行时回填观测值。**没跑过**与**跑失败**是
//!   两件事：前者是 `ESKIP`（批次不完整），后者是 `EFAIL`（判据红了）；
//! - 分类计数必须与批次定义一致：ls 5 条、cp 7 条、rm 7 条。计数对不上说明
//!   登记表和批次清单已经漂移，此时哪怕全绿也不算数（`ETALLY`）；
//! - 同一条断言可以重跑，但**重跑结论必须一致**。两次结论不同就是抖动
//!   （`EFLAKE`），抖动的绿不能算绿；
//! - 通过率只在「19 条全跑完」之后计算，绝不用分母缩水凑 100%。
//!
//! 错误类为何互不替代：
//!
//! - `ESHAPE`：类别码越界、编号为 0、批次行数不是 19。这是形状问题；
//! - `EORDER`：新断言的编号不是「上一个 + 1」。乱序登记会让缺号检测失效；
//! - `EID`：执行或查询时引用了未登记的编号；
//! - `ESKIP`：批次里仍有未执行的行。补跑即可，不需要改判据；
//! - `EFAIL`：有行执行了但观测值与期望不符。这是真正的红；
//! - `EFLAKE`：同一行两次执行结论不同。要查环境或判据，不能改期望值；
//! - `ETALLY`：分类计数与批次定义不符。要修登记，不是修执行；
//! - `EFULL`：断言行数超过 19。B10 批次是定长的，扩容即换批次。
//!
//! 闸门次序（`aggregate`）：行数形状 → 是否全跑 → 是否全过 → 分类计数 →
//! 通过率。先报 `ESKIP` 再报 `EFAIL`：一份没跑完的批次里出现的红不一定是
//! 判据的红，先补跑才有意义。

use crate::alloc::format;
use crate::checks::CheckSet;

/// 段基址。
pub const SEG_BASE: u32 = 0xef00;

/// 形状错：类别越界、编号为 0、批次行数不是 19。
pub const KA_ESHAPE: u32 = SEG_BASE + 0x01;
/// 登记编号不连续。
pub const KA_EORDER: u32 = SEG_BASE + 0x02;
/// 引用未登记的编号。
pub const KA_EID: u32 = SEG_BASE + 0x03;
/// 批次里仍有未执行的行。
pub const KA_ESKIP: u32 = SEG_BASE + 0x04;
/// 有行执行后与期望不符。
pub const KA_EFAIL: u32 = SEG_BASE + 0x05;
/// 同一行两次执行结论不同。
pub const KA_EFLAKE: u32 = SEG_BASE + 0x06;
/// 分类计数与批次定义不符。
pub const KA_ETALLY: u32 = SEG_BASE + 0x07;
/// 断言行数超过批次容量。
pub const KA_EFULL: u32 = SEG_BASE + 0x08;
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

/// 分类：ls。
pub const LS: u8 = 0;
/// 分类：cp。
pub const CP: u8 = 1;
/// 分类：rm。
pub const RM: u8 = 2;
/// 分类：批级元判据。
pub const META: u8 = 3;
/// 分类数。
pub const NCAT: usize = 4;

/// 断言行容量（即 `CAP`，B10 批次定长 19）。
pub const CAP: usize = 19;
/// 账本格数。
pub const SAMPLES: usize = 14;
/// 目标通过率。
pub const FULL: u32 = 100;

/// 批次分类定义：ls 5 条。
pub const TALLY_LS: u32 = 5;
/// 批次分类定义：cp 7 条。
pub const TALLY_CP: u32 = 7;
/// 批次分类定义：rm 7 条。
pub const TALLY_RM: u32 = 7;

/// 操作码：登记断言。
pub const OP_PUSH: u8 = 1;
/// 操作码：执行一条。
pub const OP_RUN: u8 = 2;
/// 操作码：读通过数。
pub const OP_PASS: u8 = 3;
/// 操作码：读分类计数。
pub const OP_TALLY: u8 = 4;
/// 操作码：聚合。
pub const OP_AGG: u8 = 5;

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

/// 一条断言。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Ast {
    /// 编号，1..=19。
    pub id: u8,
    /// 分类。
    pub cat: u8,
    /// 期望结论。
    pub expect: bool,
    /// 观测结论；`ran` 为假时无意义。
    pub got: bool,
    /// 是否已执行。
    pub ran: bool,
}

impl Ast {
    /// 空槽。
    pub const fn blank() -> Ast {
        Ast { id: 0, cat: 0, expect: false, got: false, ran: false }
    }

    /// 本条是否通过：跑过且观测等于期望。
    pub fn passed(&self) -> bool {
        self.ran && self.got == self.expect
    }
}

/// 一批断言。
#[derive(Clone, Copy, Debug)]
pub struct Suite {
    a: [Ast; CAP],
    n: usize,
    runs: u32,
}

impl Suite {
    /// 空批。
    pub const fn new() -> Suite {
        Suite { a: [Ast::blank(); CAP], n: 0, runs: 0 }
    }

    /// 行数。
    pub fn len(&self) -> usize {
        self.n
    }

    /// 是否空批。
    pub fn is_empty(&self) -> bool {
        self.n == 0
    }

    /// 累计执行次数。
    pub fn runs(&self) -> u32 {
        self.runs
    }

    /// 第 i 行。
    pub fn row(&self, i: usize) -> Option<Ast> {
        if i >= self.n {
            return None;
        }
        self.a.get(i).copied()
    }

    /// 按编号找行。
    pub fn find(&self, id: u8) -> Option<Ast> {
        let mut i = 0usize;
        while i < self.n {
            if let Some(x) = self.a.get(i) {
                if x.id == id {
                    return Some(*x);
                }
            }
            i += 1;
        }
        None
    }

    /// 登记一条断言。
    ///
    /// 闸门：类别 → 编号形状 → 顺序 → 容量。类别先于编号：类别错了，
    /// 这条断言根本不属于本批，编号连续性也就无从谈起。
    pub fn push(&mut self, id: u8, cat: u8, expect: bool) -> Result<usize, u32> {
        if cat as usize >= NCAT {
            return Err(KA_ESHAPE);
        }
        if id == 0 {
            return Err(KA_ESHAPE);
        }
        if id as usize != self.n + 1 {
            return Err(KA_EORDER);
        }
        if self.n >= CAP {
            return Err(KA_EFULL);
        }
        if let Some(dst) = self.a.get_mut(self.n) {
            *dst = Ast { id, cat, expect, got: false, ran: false };
        }
        self.n += 1;
        Ok(self.n)
    }

    /// 执行一条并回填观测值。
    ///
    /// 重跑同一结论是幂等的；重跑出现不同结论判 `EFLAKE`，且**不改写**已记录
    /// 的观测值——抖动必须留痕。
    pub fn run(&mut self, id: u8, got: bool) -> Result<usize, u32> {
        if id == 0 || id as usize > self.n {
            return Err(KA_EID);
        }
        let idx = (id as usize) - 1;
        let cur = match self.a.get(idx) {
            Some(v) => *v,
            None => return Err(KA_EID),
        };
        if cur.ran && cur.got != got {
            return Err(KA_EFLAKE);
        }
        if let Some(dst) = self.a.get_mut(idx) {
            *dst = Ast { got, ran: true, ..cur };
        }
        self.runs = self.runs.saturating_add(1);
        Ok(self.passed())
    }

    /// 一次命令全跑：按编号顺序执行传入的 19 个结论。
    pub fn run_all(&mut self, verdicts: [bool; CAP]) -> Result<usize, u32> {
        let mut id = 1u8;
        while (id as usize) <= self.n {
            let v = match verdicts.get((id as usize) - 1) {
                Some(x) => *x,
                None => return Err(KA_ESHAPE),
            };
            let _ = self.run(id, v)?;
            id += 1;
        }
        Ok(self.passed())
    }

    /// 通过数。
    pub fn passed(&self) -> usize {
        let mut k = 0usize;
        let mut i = 0usize;
        while i < self.n {
            if let Some(x) = self.a.get(i) {
                if x.passed() {
                    k += 1;
                }
            }
            i += 1;
        }
        k
    }

    /// 已执行数。
    pub fn ran(&self) -> usize {
        let mut k = 0usize;
        let mut i = 0usize;
        while i < self.n {
            if let Some(x) = self.a.get(i) {
                if x.ran {
                    k += 1;
                }
            }
            i += 1;
        }
        k
    }

    /// 分类计数。
    pub fn tally(&self, cat: u8) -> Result<u32, u32> {
        if cat as usize >= NCAT {
            return Err(KA_ESHAPE);
        }
        let mut k = 0u32;
        let mut i = 0usize;
        while i < self.n {
            if let Some(x) = self.a.get(i) {
                if x.cat == cat {
                    k += 1;
                }
            }
            i += 1;
        }
        Ok(k)
    }

    /// 分类计数是否与批次定义一致。
    pub fn tally_ok(&self) -> bool {
        self.tally(LS) == Ok(TALLY_LS)
            && self.tally(CP) == Ok(TALLY_CP)
            && self.tally(RM) == Ok(TALLY_RM)
            && self.tally(META) == Ok(0)
    }

    /// 聚合：19 行全跑、全过、计数相符，才给出 100。
    pub fn aggregate(&self) -> Result<u32, u32> {
        if self.n != CAP {
            return Err(KA_ESHAPE);
        }
        if self.ran() != CAP {
            return Err(KA_ESKIP);
        }
        if self.passed() != CAP {
            return Err(KA_EFAIL);
        }
        if !self.tally_ok() {
            return Err(KA_ETALLY);
        }
        Ok(FULL)
    }

    /// 不变式：行数不越容量、编号从 1 连续、分类合法。
    pub fn consistent(&self) -> bool {
        if self.n > CAP {
            return false;
        }
        let mut i = 0usize;
        while i < self.n {
            let x = match self.a.get(i) {
                Some(v) => *v,
                None => return false,
            };
            if x.id as usize != i + 1 || x.cat as usize >= NCAT {
                return false;
            }
            i += 1;
        }
        true
    }
}

/// 全通过的 19 条结论。
pub fn all_true() -> [bool; CAP] {
    [true; CAP]
}

/// 指定下标为假的结论表。
pub fn one_false(at: usize) -> [bool; CAP] {
    let mut v = [true; CAP];
    if let Some(p) = v.get_mut(at) {
        *p = false;
    }
    v
}

/// 19 条断言：ls 1..=5、cp 6..=12、rm 13..=19。
pub fn batch_19() -> Suite {
    let mut s = Suite::new();
    let mut id = 1u8;
    while (id as usize) <= CAP {
        let cat = if id <= 5 {
            LS
        } else if id <= 12 {
            CP
        } else {
            RM
        };
        let _ = s.push(id, cat, true);
        id += 1;
    }
    s
}

/// 全绿批。
pub fn green_batch() -> Suite {
    let mut s = batch_19();
    let _ = s.run_all(all_true());
    s
}

/// 一格观测。
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
            self.rejects = self.rejects.saturating_add(1);
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
        const VALS: [u32; 8] = [1, 2, 3, 1, 2, 3, 3, 2];
        const CODES: [u32; 6] = [KA_ESHAPE, KA_EORDER, KA_EID, KA_ESKIP, KA_EFLAKE, KA_EFULL];
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

    /// 指纹（只折内容行）。
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
        h
    }

    /// 冻结并记根。
    pub fn freeze(&mut self) -> Result<u64, u32> {
        if self.frozen {
            self.rejects = self.rejects.saturating_add(1);
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

/// 十四格账本：三条登记 + 三条执行 + 通过数 + 计数 + 六类拒绝。
pub fn build_ledger() -> Ledger {
    let mut l = Ledger::new();
    let mut s = Suite::new();

    let _ = l.register(cell(0, OP_PUSH, s.push(1, LS, true)));
    let _ = l.register(cell(1, OP_PUSH, s.push(2, LS, true)));
    let _ = l.register(cell(2, OP_PUSH, s.push(3, CP, true)));
    let _ = l.register(cell(3, OP_RUN, s.run(1, true)));
    let _ = l.register(cell(4, OP_RUN, s.run(2, true)));
    let _ = l.register(cell(5, OP_RUN, s.run(3, true)));
    let _ = l.register(word_cell(6, OP_PASS, Ok(s.passed() as u32)));
    let _ = l.register(word_cell(7, OP_TALLY, s.tally(LS)));

    let mut a = Suite::new();
    let _ = l.register(cell(8, OP_PUSH, a.push(0, LS, true)));

    let mut b = Suite::new();
    let _ = b.push(1, LS, true);
    let _ = l.register(cell(9, OP_PUSH, b.push(3, LS, true)));

    let mut c = Suite::new();
    let _ = c.push(1, LS, true);
    let _ = l.register(cell(10, OP_RUN, c.run(9, true)));

    let mut d = batch_19();
    let _ = d.push(1, LS, true);
    let _ = l.register(word_cell(11, OP_AGG, d.aggregate()));

    let mut e = Suite::new();
    let _ = e.push(1, LS, true);
    let _ = e.run(1, true);
    let _ = l.register(cell(12, OP_RUN, e.run(1, false)));

    let mut f = batch_19();
    let _ = l.register(cell(13, OP_PUSH, f.push(20, LS, true)));

    l
}

/// 四路注入。
pub fn inject_four() -> [bool; 4] {
    let mut out = [false; 4];

    // 一路：账本形状、对账、重放、指纹。
    let l = build_ledger();
    let rep = l.replay();
    let root = l.seal();
    out[0] = l.len() == SAMPLES
        && l.ok_count() == 8
        && l.err_count() == 6
        && l.audit().is_ok()
        && rep == Ok(root)
        && root != 0;

    // 二路：一条命令跑完 19 条，计数 5/7/7，通过率恰好 100。
    let g = green_batch();
    out[1] = g.len() == CAP
        && g.ran() == CAP
        && g.passed() == CAP
        && g.tally(LS) == Ok(TALLY_LS)
        && g.tally(CP) == Ok(TALLY_CP)
        && g.tally(RM) == Ok(TALLY_RM)
        && g.tally(META) == Ok(0)
        && g.tally_ok()
        && g.aggregate() == Ok(FULL)
        && g.runs() == CAP as u32
        && g.consistent();

    // 三路：八类问题彼此可辨。
    let mut a = Suite::new();
    let shape = a.push(1, 9, true) == Err(KA_ESHAPE);

    let mut b = Suite::new();
    let _ = b.push(1, LS, true);
    let order = b.push(3, LS, true) == Err(KA_EORDER);
    let id = b.run(5, true) == Err(KA_EID);

    let skip = batch_19().aggregate() == Err(KA_ESKIP);

    let mut f = batch_19();
    let _ = f.run_all(one_false(5));
    let fail = f.aggregate() == Err(KA_EFAIL) && f.passed() == CAP - 1;

    let mut k = Suite::new();
    let _ = k.push(1, LS, true);
    let _ = k.run(1, true);
    let flake = k.run(1, false) == Err(KA_EFLAKE) && k.find(1).map(|x| x.got) == Some(true);

    let mut t = Suite::new();
    let mut i = 1u8;
    while (i as usize) <= CAP {
        let _ = t.push(i, LS, true);
        i += 1;
    }
    let _ = t.run_all(all_true());
    let tally = t.aggregate() == Err(KA_ETALLY) && t.passed() == CAP;

    let full = {
        let mut x = batch_19();
        x.push(20, LS, true) == Err(KA_EFULL)
    };

    out[2] = shape && order && id && skip && fail && flake && tally && full;

    // 四路：闸门次序与容量边界。
    let mut q = Suite::new();
    let g1 = q.push(1, 9, true) == Err(KA_ESHAPE);
    let g2 = q.push(5, 9, true) == Err(KA_ESHAPE);
    let g3 = q.push(2, LS, true) == Err(KA_EORDER);
    let g4 = q.push(1, LS, true) == Ok(1);
    let g5 = q.push(2, LS, true) == Ok(2);
    let g6 = q.aggregate() == Err(KA_ESHAPE);
    let g7 = q.run(0, true) == Err(KA_EID);
    let g8 = q.tally(7) == Err(KA_ESHAPE);
    out[3] = g1 && g2 && g3 && g4 && g5 && g6 && g7 && g8 && q.consistent();

    out
}

/// 十三项判据。
pub fn run_unx11400_checks() -> CheckSet {
    let mut s = CheckSet::new("unx11400.ktest_batch_aggregate");
    let l = build_ledger();

    s.add_dyn(
        "KA1-十四格八允六拒",
        l.len() == SAMPLES && l.ok_count() == 8 && l.err_count() == 6 && l.audit().is_ok(),
        &format!("n={} ok={} err={}", l.len(), l.ok_count(), l.err_count()),
    );

    s.add(
        "KA2-十九条连续编号",
        l.sample(0).map(|x| x.value) == Some(1)
            && l.sample(1).map(|x| x.value) == Some(2)
            && l.sample(2).map(|x| x.value) == Some(3)
            && {
                let b = batch_19();
                let mut ok = b.len() == CAP;
                let mut i = 1u8;
                while (i as usize) <= CAP {
                    if b.find(i).map(|x| x.id) != Some(i) {
                        ok = false;
                    }
                    i += 1;
                }
                ok && b.consistent()
            },
        "static",
    );

    s.add(
        "KA3-三档计数五七七",
        l.sample(7).map(|x| x.value) == Some(2)
            && {
                let b = batch_19();
                b.tally(LS) == Ok(TALLY_LS)
                    && b.tally(CP) == Ok(TALLY_CP)
                    && b.tally(RM) == Ok(TALLY_RM)
                    && b.tally(META) == Ok(0)
                    && b.tally_ok()
            },
        "static",
    );

    s.add(
        "KA4-全跑通过率100",
        l.sample(6).map(|x| x.value) == Some(3)
            && {
                let g = green_batch();
                g.aggregate() == Ok(FULL)
                    && g.passed() == CAP
                    && g.ran() == CAP
                    && g.runs() == CAP as u32
            },
        "static",
    );

    s.add(
        "KA5-未跑即ESKIP",
        l.sample(11).map(|x| x.code) == Some(KA_ESKIP)
            && {
                let mut b = batch_19();
                b.aggregate() == Err(KA_ESKIP)
                    && b.run(1, true) == Ok(1)
                    && b.aggregate() == Err(KA_ESKIP)
                    && {
                        let _ = b.run_all(all_true());
                        b.aggregate() == Ok(FULL)
                    }
            },
        "static",
    );

    s.add(
        "KA6-失败即EFAIL",
        {
            let mut b = batch_19();
            let _ = b.run_all(one_false(0));
            let red = b.aggregate() == Err(KA_EFAIL);
            let mut c = batch_19();
            let _ = c.run_all(one_false(18));
            red && c.aggregate() == Err(KA_EFAIL) && c.passed() == CAP - 1
        },
        "static",
    );

    s.add(
        "KA7-计数不符ETALLY",
        {
            let mut t = Suite::new();
            let mut i = 1u8;
            while (i as usize) <= CAP {
                let _ = t.push(i, LS, true);
                i += 1;
            }
            let _ = t.run_all(all_true());
            t.aggregate() == Err(KA_ETALLY)
                && t.passed() == CAP
                && t.tally(LS) == Ok(CAP as u32)
                && !t.tally_ok()
        },
        "static",
    );

    s.add(
        "KA8-类别越界ESHAPE",
        l.sample(8).map(|x| x.code) == Some(KA_ESHAPE)
            && {
                let mut a = Suite::new();
                a.push(1, 9, true) == Err(KA_ESHAPE)
                    && a.push(0, LS, true) == Err(KA_ESHAPE)
                    && a.len() == 0
            },
        "static",
    );

    s.add(
        "KA9-乱序登记EORDER",
        l.sample(9).map(|x| x.code) == Some(KA_EORDER)
            && {
                let mut a = Suite::new();
                let one = a.push(1, LS, true) == Ok(1);
                let three = a.push(3, LS, true) == Err(KA_EORDER);
                let dup = a.push(1, LS, true) == Err(KA_EORDER);
                one && three && dup && a.len() == 1
            },
        "static",
    );

    s.add(
        "KA10-未知编号EID",
        l.sample(10).map(|x| x.code) == Some(KA_EID)
            && {
                let mut a = Suite::new();
                let _ = a.push(1, LS, true);
                a.run(2, true) == Err(KA_EID)
                    && a.run(0, true) == Err(KA_EID)
                    && a.passed() == 0
            },
        "static",
    );

    s.add(
        "KA11-结论抖动EFLAKE",
        l.sample(12).map(|x| x.code) == Some(KA_EFLAKE)
            && {
                let mut a = Suite::new();
                let _ = a.push(1, LS, true);
                let _ = a.run(1, true);
                a.run(1, false) == Err(KA_EFLAKE)
                    && a.find(1).map(|x| x.got) == Some(true)
                    && a.run(1, true) == Ok(1)
                    && a.runs() == 2
            },
        "static",
    );

    s.add(
        "KA12-批次满EFULL",
        l.sample(13).map(|x| x.code) == Some(KA_EFULL)
            && {
                let mut b = batch_19();
                b.len() == CAP
                    && b.push(20, LS, true) == Err(KA_EFULL)
                    && b.push(1, LS, true) == Err(KA_EORDER)
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
                && f.rejects() == 1
                && f.is_frozen()
                && f.root() != 0
                && f.root() == f.seal()
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
        let s = run_unx11400_checks();
        assert_eq!(s.len(), 13);
        assert!(s.all_passed());
    }

    #[test]
    fn batch_is_contiguous() {
        let b = batch_19();
        assert_eq!(b.len(), 19);
        assert!(b.consistent());
        assert_eq!(b.find(1).map(|x| x.cat), Some(LS));
        assert_eq!(b.find(5).map(|x| x.cat), Some(LS));
        assert_eq!(b.find(6).map(|x| x.cat), Some(CP));
        assert_eq!(b.find(12).map(|x| x.cat), Some(CP));
        assert_eq!(b.find(13).map(|x| x.cat), Some(RM));
        assert_eq!(b.find(19).map(|x| x.cat), Some(RM));
    }

    #[test]
    fn green_batch_is_100() {
        let g = green_batch();
        assert_eq!(g.aggregate(), Ok(100));
        assert_eq!(g.passed(), 19);
        assert_eq!(g.ran(), 19);
    }

    #[test]
    fn skipped_batch_is_not_green() {
        let b = batch_19();
        assert_eq!(b.aggregate(), Err(KA_ESKIP));
        let mut c = batch_19();
        let _ = c.run_all(one_false(3));
        assert_eq!(c.aggregate(), Err(KA_EFAIL));
    }

    #[test]
    fn reruns_must_be_stable() {
        let mut a = Suite::new();
        assert_eq!(a.push(1, LS, true), Ok(1));
        assert_eq!(a.run(1, true), Ok(1));
        assert_eq!(a.run(1, false), Err(KA_EFLAKE));
        assert_eq!(a.find(1).map(|x| x.got), Some(true));
        assert_eq!(a.runs(), 1);
    }

    #[test]
    fn order_and_capacity() {
        let mut a = Suite::new();
        assert_eq!(a.push(1, 4, true), Err(KA_ESHAPE));
        assert_eq!(a.push(0, LS, true), Err(KA_ESHAPE));
        assert_eq!(a.push(2, LS, true), Err(KA_EORDER));
        assert_eq!(a.push(1, LS, true), Ok(1));
        let mut b = batch_19();
        assert_eq!(b.push(20, LS, true), Err(KA_EFULL));
    }

    #[test]
    fn inject_four_is_all_true() {
        assert_eq!(inject_four(), [true; 4]);
    }
}
