//! UNX-F11420 B11 批 ktest 断言集聚合
//!
//! B11 批的 ktest 不是一条条孤立断言，而是一张有编号、有次序、有硬软之分的
//! 断言表。每条断言带 `(case, aid)` 二元号：case 是测例号，aid 是测例内的
//! 断言号。收批时先看条数是否齐套，再看有没有硬失败；软失败只留痕，不翻批。
//!
//! 编号规则：case 与 aid 都必须从 1 起；case 不回退（后面的测例号只能更大），
//! 同一 case 内 aid 严格递增。aid 回退记 `EORDER`，aid 相同记 `EDUP`，两者
//! 分开判定，避免「重复」被「乱序」吞掉。
//!
//! 条数规则：开批时声明期望条数 `expect`（1..=CAP）。记录超过期望条数记
//! `EOVER`；表本身装满 CAP 格记 `EFULL`；收批时不足期望条数记 `EMISS`。
//! 表满闸门排在超数闸门之前，因此 `expect == CAP` 时最后一格之后先报 `EFULL`
//! 而不是 `EOVER`，两种越界各有唯一出口。
//!
//! 聚合规则：`finish` 先查齐套，再查硬失败。任何一条 `ok == false && hard`
//! 都会让整批记 `ERED`；`hard == false` 的软失败只进 `soft_count`，批次仍可
//! 收。这样「红」只由硬断言决定，软告警不会污染批结论。
//!
//! 闸门次序（`record`）：期望合法 → 测例号 → 断言号 → 次序 → 重复 → 表满
//! → 超数。账本契约：十四格，八成功六失败；成功格记录入表计数、收批条数与
//! 软失败数，失败格覆盖测例/断言/重复/乱序/超数/硬红六类；缺条与表满在判据
//! 和注入里单独验证。成功值 `[1,2,3,4,5,6,6,1]`，失败码依次为
//! `ECASE/EAID/EDUP/EORDER/EOVER/ERED`。

use crate::alloc::format;
use crate::checks::CheckSet;

/// 段基址。
pub const SEG_BASE: u32 = 0xf040;

/// 测例号为零或越界。
pub const BA_ECASE: u32 = SEG_BASE + 0x01;
/// 断言号为零或越界。
pub const BA_EAID: u32 = SEG_BASE + 0x02;
/// 同测例同断言号重复。
pub const BA_EDUP: u32 = SEG_BASE + 0x03;
/// 测例号或断言号回退。
pub const BA_EORDER: u32 = SEG_BASE + 0x04;
/// 记录超过期望条数。
pub const BA_EOVER: u32 = SEG_BASE + 0x05;
/// 收批时条数不足。
pub const BA_EMISS: u32 = SEG_BASE + 0x06;
/// 存在硬失败，整批翻红。
pub const BA_ERED: u32 = SEG_BASE + 0x07;
/// 断言表满或期望条数越界。
pub const BA_EFULL: u32 = SEG_BASE + 0x08;
/// 账本形状不符。
pub const BA_EBAL: u32 = SEG_BASE + 0x09;
/// 槽位乱序。
pub const BA_ESLOT: u32 = SEG_BASE + 0x0a;
/// 观测值与复算不符。
pub const BA_EVALUE: u32 = SEG_BASE + 0x0b;
/// 冻结后重放不一致。
pub const BA_ESEAL: u32 = SEG_BASE + 0x0c;
/// 观察下标越界。
pub const BA_ENOBS: u32 = SEG_BASE + 0x0d;

/// 断言表容量。
pub const CAP: usize = 8;
/// 测例号上界。
pub const CASE_MAX: u8 = 3;
/// 断言号上界。
pub const AID_MAX: u8 = 6;
/// 账本格数。
pub const SAMPLES: usize = 14;

/// 操作码：开批与期望闸门。
pub const OP_OPEN: u8 = 1;
/// 操作码：断言入表。
pub const OP_REC: u8 = 2;
/// 操作码：收批。
pub const OP_FIN: u8 = 3;
/// 操作码：软失败计数。
pub const OP_SOFT: u8 = 4;

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

/// 一条 ktest 断言。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Assert {
    /// 测例号。
    pub case: u8,
    /// 测例内断言号。
    pub aid: u8,
    /// 断言是否通过。
    pub ok: bool,
    /// 是否硬断言；硬断言失败整批翻红。
    pub hard: bool,
}

/// 一批 ktest 断言。
#[derive(Clone, Copy, Debug)]
pub struct Batch {
    expect: usize,
    rows: [Assert; CAP],
    n: usize,
    last_case: u8,
    last_aid: u8,
}

impl Batch {
    /// 开一批空白断言表。
    pub const fn new(expect: usize) -> Batch {
        Batch {
            expect,
            rows: [Assert { case: 0, aid: 0, ok: false, hard: false }; CAP],
            n: 0,
            last_case: 0,
            last_aid: 0,
        }
    }

    /// 期望条数闸门：1..=CAP 才合法。
    pub fn expect_gate(expect: usize) -> Result<usize, u32> {
        if expect == 0 || expect > CAP {
            Err(BA_EFULL)
        } else {
            Ok(expect)
        }
    }

    /// 期望条数。
    pub fn expect(&self) -> usize {
        self.expect
    }

    /// 已入表条数。
    pub fn len(&self) -> usize {
        self.n
    }

    /// 是否空表。
    pub fn is_empty(&self) -> bool {
        self.n == 0
    }

    /// 第 i 条断言。
    pub fn assert_at(&self, i: usize) -> Option<Assert> {
        if i >= self.n {
            return None;
        }
        self.rows.get(i).copied()
    }

    /// 硬失败数。
    pub fn red_count(&self) -> u8 {
        let mut k = 0u8;
        let mut i = 0usize;
        while i < self.n {
            if let Some(a) = self.rows.get(i) {
                if !a.ok && a.hard {
                    k = k.saturating_add(1);
                }
            }
            i = i.saturating_add(1);
        }
        k
    }

    /// 软失败数。
    pub fn soft_count(&self) -> u8 {
        let mut k = 0u8;
        let mut i = 0usize;
        while i < self.n {
            if let Some(a) = self.rows.get(i) {
                if !a.ok && !a.hard {
                    k = k.saturating_add(1);
                }
            }
            i = i.saturating_add(1);
        }
        k
    }

    /// 失败总数。
    pub fn fail_count(&self) -> u8 {
        let mut k = 0u8;
        let mut i = 0usize;
        while i < self.n {
            if let Some(a) = self.rows.get(i) {
                if !a.ok {
                    k = k.saturating_add(1);
                }
            }
            i = i.saturating_add(1);
        }
        k
    }

    /// 断言入表。
    ///
    /// 闸门次序：期望合法 → 测例号 → 断言号 → 次序 → 重复 → 表满 → 超数。
    pub fn record(&mut self, case: u8, aid: u8, ok: bool, hard: bool) -> Result<usize, u32> {
        match Self::expect_gate(self.expect) {
            Ok(_) => {}
            Err(c) => return Err(c),
        }
        if case == 0 || case > CASE_MAX {
            return Err(BA_ECASE);
        }
        if aid == 0 || aid > AID_MAX {
            return Err(BA_EAID);
        }
        if case < self.last_case {
            return Err(BA_EORDER);
        }
        if case == self.last_case {
            if aid < self.last_aid {
                return Err(BA_EORDER);
            }
            if aid == self.last_aid {
                return Err(BA_EDUP);
            }
        }
        if self.n >= CAP {
            return Err(BA_EFULL);
        }
        if self.n >= self.expect {
            return Err(BA_EOVER);
        }
        if let Some(dst) = self.rows.get_mut(self.n) {
            *dst = Assert { case, aid, ok, hard };
        }
        self.n = self.n.saturating_add(1);
        self.last_case = case;
        self.last_aid = aid;
        Ok(self.n)
    }

    /// 收批：条数齐套且无硬失败。
    pub fn finish(&self) -> Result<u32, u32> {
        match Self::expect_gate(self.expect) {
            Ok(_) => {}
            Err(c) => return Err(c),
        }
        if self.n < self.expect {
            return Err(BA_EMISS);
        }
        if self.n > self.expect {
            return Err(BA_EOVER);
        }
        if self.red_count() > 0 {
            return Err(BA_ERED);
        }
        Ok(self.n as u32)
    }

    /// 不变式自检。
    pub fn consistent(&self) -> bool {
        if Self::expect_gate(self.expect).is_err() || self.n > CAP || self.n > self.expect {
            return false;
        }
        let mut i = 0usize;
        while i < self.n {
            let a = match self.rows.get(i) {
                Some(v) => *v,
                None => return false,
            };
            if a.case == 0 || a.case > CASE_MAX || a.aid == 0 || a.aid > AID_MAX {
                return false;
            }
            if i > 0 {
                if let Some(p) = self.rows.get(i.saturating_sub(1)) {
                    if p.case > a.case {
                        return false;
                    }
                    if p.case == a.case && p.aid >= a.aid {
                        return false;
                    }
                }
            }
            i = i.saturating_add(1);
        }
        if self.n > 0 {
            if let Some(last) = self.rows.get(self.n.saturating_sub(1)) {
                if self.last_case != last.case || self.last_aid != last.aid {
                    return false;
                }
            }
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

/// `Result<u8, u32>` → 样本。
fn byte_cell(slot: u8, op: u8, r: Result<u8, u32>) -> Sample {
    match r {
        Ok(v) => Sample { slot, op, ok: true, code: 0, value: v as u32 },
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
            return Err(BA_ESLOT);
        }
        if let Some(dst) = self.rows.get_mut(self.len) {
            *dst = s;
        }
        self.len = self.len.saturating_add(1);
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
            if let Some(v) = self.rows.get(i) {
                if v.ok {
                    n = n.saturating_add(1);
                }
            }
            i = i.saturating_add(1);
        }
        n
    }

    /// 失败格数。
    pub fn err_count(&self) -> usize {
        self.len.saturating_sub(self.ok_count())
    }

    /// 对账。
    pub fn audit(&self) -> Result<(), u32> {
        if self.len != SAMPLES || self.ok_count() != 8 || self.err_count() != 6 {
            return Err(BA_EBAL);
        }
        const VALS: [u32; 8] = [1, 2, 3, 4, 5, 6, 6, 1];
        const CODES: [u32; 6] =
            [BA_ECASE, BA_EAID, BA_EDUP, BA_EORDER, BA_EOVER, BA_ERED];
        let mut vi = 0usize;
        let mut ci = 0usize;
        let mut i = 0usize;
        while i < self.len {
            let v = match self.rows.get(i) {
                Some(x) => *x,
                None => return Err(BA_EBAL),
            };
            if v.slot as usize != i {
                return Err(BA_ESLOT);
            }
            if v.ok {
                if v.code != 0 {
                    return Err(BA_EVALUE);
                }
                match VALS.get(vi) {
                    Some(w) => {
                        if v.value != *w {
                            return Err(BA_EVALUE);
                        }
                    }
                    None => return Err(BA_EBAL),
                }
                vi = vi.saturating_add(1);
            } else {
                match CODES.get(ci) {
                    Some(w) => {
                        if v.code != *w {
                            return Err(BA_EVALUE);
                        }
                    }
                    None => return Err(BA_EBAL),
                }
                ci = ci.saturating_add(1);
            }
            i = i.saturating_add(1);
        }
        if vi != 8 || ci != 6 {
            return Err(BA_EBAL);
        }
        Ok(())
    }

    /// 指纹。
    pub fn seal(&self) -> u64 {
        let mut h = FNV_OFF;
        let mut i = 0usize;
        while i < self.len {
            if let Some(v) = self.rows.get(i) {
                h = fold(h, v.slot as u64);
                h = fold(h, v.op as u64);
                h = fold(h, v.ok as u64);
                h = fold(h, v.code as u64);
                h = fold(h, v.value as u64);
            }
            i = i.saturating_add(1);
        }
        fold(h, self.rejects as u64)
    }

    /// 冻结。
    pub fn freeze(&mut self) -> Result<u64, u32> {
        if self.frozen {
            self.rejects = self.rejects.saturating_add(1);
            return Err(BA_EBAL);
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
            Err(BA_ESEAL)
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

/// 十四格账本：六条断言（含一软失败）→ 收批 → 软计数 → 六类拒绝。
pub fn build_ledger() -> Ledger {
    let mut l = Ledger::new();
    let mut b = Batch::new(6);

    let _ = l.register(cell(0, OP_REC, b.record(1, 1, true, true)));
    let _ = l.register(cell(1, OP_REC, b.record(1, 2, true, true)));
    let _ = l.register(cell(2, OP_REC, b.record(1, 3, true, true)));
    let _ = l.register(cell(3, OP_REC, b.record(2, 1, false, false)));
    let _ = l.register(cell(4, OP_REC, b.record(2, 2, true, true)));
    let _ = l.register(cell(5, OP_REC, b.record(3, 1, true, true)));
    let _ = l.register(word_cell(6, OP_FIN, b.finish()));
    let _ = l.register(byte_cell(7, OP_SOFT, Ok(b.soft_count())));

    let mut c = Batch::new(3);
    let _ = l.register(cell(8, OP_REC, c.record(0, 1, true, true)));

    let mut d = Batch::new(3);
    let _ = l.register(cell(9, OP_REC, d.record(1, 0, true, true)));

    let mut e = Batch::new(4);
    let _ = e.record(1, 1, true, true);
    let _ = l.register(cell(10, OP_REC, e.record(1, 1, true, true)));

    let mut f = Batch::new(4);
    let _ = f.record(1, 2, true, true);
    let _ = l.register(cell(11, OP_REC, f.record(1, 1, true, true)));

    let mut g = Batch::new(2);
    let _ = g.record(1, 1, true, true);
    let _ = g.record(1, 2, true, true);
    let _ = l.register(cell(12, OP_REC, g.record(1, 3, true, true)));

    let mut h = Batch::new(3);
    let _ = h.record(1, 1, true, true);
    let _ = h.record(1, 2, false, true);
    let _ = h.record(1, 3, true, true);
    let _ = l.register(word_cell(13, OP_FIN, h.finish()));

    l
}

/// 四路注入。
pub fn inject_four() -> [bool; 4] {
    let mut out = [false; 4];

    let l = build_ledger();
    let rep = l.replay();
    let root = l.seal();
    out[0] = l.len() == SAMPLES && l.audit().is_ok() && rep == Ok(root) && root != 0;

    // 一路：齐套收批，软失败留痕不翻红。
    let mut a = Batch::new(6);
    let r1 = a.record(1, 1, true, true);
    let r2 = a.record(1, 2, true, true);
    let r3 = a.record(1, 3, true, true);
    let r4 = a.record(2, 1, false, false);
    let r5 = a.record(2, 2, true, true);
    let r6 = a.record(3, 1, true, true);
    out[1] = r1 == Ok(1)
        && r2 == Ok(2)
        && r3 == Ok(3)
        && r4 == Ok(4)
        && r5 == Ok(5)
        && r6 == Ok(6)
        && a.len() == 6
        && a.expect() == 6
        && a.red_count() == 0
        && a.soft_count() == 1
        && a.fail_count() == 1
        && a.finish() == Ok(6)
        && a.assert_at(3) == Some(Assert { case: 2, aid: 1, ok: false, hard: false })
        && a.assert_at(6).is_none()
        && a.consistent();

    // 二路：八类失败各自可辨。
    let mut b = Batch::new(3);
    let case_zero = b.record(0, 1, true, true) == Err(BA_ECASE);
    let case_high = b.record(CASE_MAX + 1, 1, true, true) == Err(BA_ECASE);
    let aid_zero = b.record(1, 0, true, true) == Err(BA_EAID);
    let aid_high = b.record(1, AID_MAX + 1, true, true) == Err(BA_EAID);
    let mut c = Batch::new(4);
    let _ = c.record(1, 1, true, true);
    let dup = c.record(1, 1, true, true) == Err(BA_EDUP);
    let mut d = Batch::new(4);
    let _ = d.record(1, 2, true, true);
    let order_aid = d.record(1, 1, true, true) == Err(BA_EORDER);
    let mut e = Batch::new(4);
    let _ = e.record(2, 1, true, true);
    let order_case = e.record(1, 1, true, true) == Err(BA_EORDER);
    let mut f = Batch::new(2);
    let _ = f.record(1, 1, true, true);
    let _ = f.record(1, 2, true, true);
    let over = f.record(1, 3, true, true) == Err(BA_EOVER);
    let mut g = Batch::new(CAP);
    let mut k = 1u8;
    while k <= 6 {
        let _ = g.record(1, k, true, true);
        k = k.saturating_add(1);
    }
    let _ = g.record(2, 1, true, true);
    let _ = g.record(2, 2, true, true);
    let full = g.record(2, 3, true, true) == Err(BA_EFULL) && g.len() == CAP;
    let mut h = Batch::new(3);
    let _ = h.record(1, 1, true, true);
    let _ = h.record(1, 2, true, true);
    let miss = h.finish() == Err(BA_EMISS);
    let mut i = Batch::new(3);
    let _ = i.record(1, 1, true, true);
    let _ = i.record(1, 2, false, true);
    let _ = i.record(1, 3, true, true);
    let red = i.finish() == Err(BA_ERED);
    let bad_expect = Batch::expect_gate(0) == Err(BA_EFULL)
        && Batch::expect_gate(CAP + 1) == Err(BA_EFULL)
        && Batch::expect_gate(CAP) == Ok(CAP);
    out[2] = case_zero
        && case_high
        && aid_zero
        && aid_high
        && dup
        && order_aid
        && order_case
        && over
        && full
        && miss
        && red
        && bad_expect;

    // 三路：硬软分流与聚合计数。
    let mut p = Batch::new(4);
    let _ = p.record(1, 1, true, true);
    let _ = p.record(1, 2, false, false);
    let _ = p.record(2, 1, true, true);
    let _ = p.record(2, 2, false, true);
    let hard = p.finish() == Err(BA_ERED)
        && p.red_count() == 1
        && p.soft_count() == 1
        && p.fail_count() == 2
        && p.consistent();
    let mut q = Batch::new(4);
    let _ = q.record(1, 1, true, true);
    let _ = q.record(1, 2, false, false);
    let _ = q.record(2, 1, true, false);
    let _ = q.record(2, 2, false, false);
    let soft = q.finish() == Ok(4)
        && q.red_count() == 0
        && q.soft_count() == 2
        && q.fail_count() == 2
        && q.consistent();
    let mut r = Batch::new(3);
    let _ = r.record(1, 1, true, true);
    let _ = r.record(2, 1, true, true);
    let _ = r.record(3, 1, true, true);
    let ordered = r.finish() == Ok(3)
        && r.assert_at(2) == Some(Assert { case: 3, aid: 1, ok: true, hard: true })
        && r.consistent();
    out[3] = hard && soft && ordered;

    out
}

/// 十三项判据。
pub fn run_unx11420_checks() -> CheckSet {
    let mut s = CheckSet::new("unx11420.b11_ktest_assert_aggregate");
    let l = build_ledger();

    s.add_dyn(
        "BA1-十四格八允六拒",
        l.len() == SAMPLES && l.ok_count() == 8 && l.err_count() == 6 && l.audit().is_ok(),
        &format!("n={} ok={} err={}", l.len(), l.ok_count(), l.err_count()),
    );

    s.add(
        "BA2-入表计数递增",
        l.sample(0).map(|x| x.value) == Some(1)
            && l.sample(1).map(|x| x.value) == Some(2)
            && l.sample(2).map(|x| x.value) == Some(3)
            && l.sample(3).map(|x| x.value) == Some(4)
            && l.sample(4).map(|x| x.value) == Some(5)
            && l.sample(5).map(|x| x.value) == Some(6)
            && l.sample(0).map(|x| x.op) == Some(OP_REC)
            && l.sample(5).map(|x| x.op) == Some(OP_REC),
        "static",
    );

    s.add(
        "BA3-收批与软失败计数",
        l.sample(6).map(|x| x.value) == Some(6)
            && l.sample(6).map(|x| x.op) == Some(OP_FIN)
            && l.sample(7).map(|x| x.value) == Some(1)
            && l.sample(7).map(|x| x.op) == Some(OP_SOFT),
        "static",
    );

    s.add(
        "BA4-六类失败归位",
        l.sample(8).map(|x| x.code) == Some(BA_ECASE)
            && l.sample(9).map(|x| x.code) == Some(BA_EAID)
            && l.sample(10).map(|x| x.code) == Some(BA_EDUP)
            && l.sample(11).map(|x| x.code) == Some(BA_EORDER)
            && l.sample(12).map(|x| x.code) == Some(BA_EOVER)
            && l.sample(13).map(|x| x.code) == Some(BA_ERED)
            && l.sample(8).map(|x| x.ok) == Some(false)
            && l.sample(13).map(|x| x.ok) == Some(false),
        "static",
    );

    s.add(
        "BA5-测例与断言号闸门",
        {
            let mut a = Batch::new(3);
            let c1 = a.record(0, 1, true, true) == Err(BA_ECASE);
            let c2 = a.record(CASE_MAX + 1, 1, true, true) == Err(BA_ECASE);
            let a1 = a.record(1, 0, true, true) == Err(BA_EAID);
            let a2 = a.record(1, AID_MAX + 1, true, true) == Err(BA_EAID);
            c1 && c2 && a1 && a2 && a.is_empty() && a.consistent()
        },
        "static",
    );

    s.add(
        "BA6-次序与重复分流",
        {
            let mut a = Batch::new(4);
            let _ = a.record(1, 2, true, true);
            let back_aid = a.record(1, 1, true, true) == Err(BA_EORDER);
            let retry = a.record(1, 3, true, true) == Ok(2);
            let _ = a.record(2, 1, true, true);
            let back_case = a.record(1, 1, true, true) == Err(BA_EORDER);
            let dup = a.record(2, 1, true, true) == Err(BA_EDUP);
            back_aid && retry && back_case && dup && a.len() == 3 && a.consistent()
        },
        "static",
    );

    s.add(
        "BA7-超数与表满分界",
        {
            let mut a = Batch::new(2);
            let _ = a.record(1, 1, true, true);
            let _ = a.record(1, 2, true, true);
            let over = a.record(1, 3, true, true) == Err(BA_EOVER);
            let mut g = Batch::new(CAP);
            let mut k = 1u8;
            while k <= 6 {
                let _ = g.record(1, k, true, true);
                k = k.saturating_add(1);
            }
            let _ = g.record(2, 1, true, true);
            let _ = g.record(2, 2, true, true);
            let full = g.record(2, 3, true, true) == Err(BA_EFULL);
            over && full && a.len() == 2 && g.len() == CAP && g.consistent()
        },
        "static",
    );

    s.add(
        "BA8-缺条与硬红收批",
        {
            let mut a = Batch::new(3);
            let _ = a.record(1, 1, true, true);
            let _ = a.record(1, 2, true, true);
            let miss = a.finish() == Err(BA_EMISS);
            let mut b = Batch::new(3);
            let _ = b.record(1, 1, true, true);
            let _ = b.record(1, 2, false, true);
            let _ = b.record(1, 3, true, true);
            let red = b.finish() == Err(BA_ERED);
            miss && red && a.is_empty() == false && b.red_count() == 1
        },
        "static",
    );

    s.add(
        "BA9-软失败不翻批",
        {
            let mut a = Batch::new(4);
            let _ = a.record(1, 1, true, true);
            let _ = a.record(1, 2, false, false);
            let _ = a.record(2, 1, true, false);
            let _ = a.record(2, 2, false, false);
            a.finish() == Ok(4)
                && a.red_count() == 0
                && a.soft_count() == 2
                && a.fail_count() == 2
                && a.consistent()
        },
        "static",
    );

    s.add(
        "BA10-硬软混合聚合",
        {
            let mut a = Batch::new(4);
            let _ = a.record(1, 1, true, true);
            let _ = a.record(1, 2, false, false);
            let _ = a.record(2, 1, true, true);
            let _ = a.record(2, 2, false, true);
            a.finish() == Err(BA_ERED)
                && a.red_count() == 1
                && a.soft_count() == 1
                && a.fail_count() == 2
                && a.finish() != Ok(4)
        },
        "static",
    );

    s.add(
        "BA11-断言回查与不变式",
        {
            let mut a = Batch::new(3);
            let _ = a.record(1, 1, true, true);
            let _ = a.record(2, 1, false, true);
            let _ = a.record(3, 1, true, false);
            a.assert_at(0) == Some(Assert { case: 1, aid: 1, ok: true, hard: true })
                && a.assert_at(1) == Some(Assert { case: 2, aid: 1, ok: false, hard: true })
                && a.assert_at(2) == Some(Assert { case: 3, aid: 1, ok: true, hard: false })
                && a.assert_at(3).is_none()
                && a.consistent()
                && a.finish() == Err(BA_ERED)
        },
        "static",
    );

    s.add(
        "BA12-段基与错误码布局",
        SEG_BASE == 0xf040
            && BA_ECASE == SEG_BASE + 0x01
            && BA_EAID == SEG_BASE + 0x02
            && BA_EDUP == SEG_BASE + 0x03
            && BA_EORDER == SEG_BASE + 0x04
            && BA_EOVER == SEG_BASE + 0x05
            && BA_EMISS == SEG_BASE + 0x06
            && BA_ERED == SEG_BASE + 0x07
            && BA_EFULL == SEG_BASE + 0x08
            && BA_EBAL == SEG_BASE + 0x09
            && BA_ESLOT == SEG_BASE + 0x0a
            && BA_EVALUE == SEG_BASE + 0x0b
            && BA_ESEAL == SEG_BASE + 0x0c
            && BA_ENOBS == SEG_BASE + 0x0d
            && SAMPLES == 14
            && CAP == 8
            && CASE_MAX == 3
            && AID_MAX == 6,
        "static",
    );

    s.add(
        "BA13-冻结重放四注入",
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

    fn batch() -> Batch {
        let mut b = Batch::new(6);
        let _ = b.record(1, 1, true, true);
        let _ = b.record(1, 2, true, true);
        let _ = b.record(1, 3, true, true);
        let _ = b.record(2, 1, false, false);
        let _ = b.record(2, 2, true, true);
        let _ = b.record(3, 1, true, true);
        b
    }

    #[test]
    fn ledger_shape() {
        let l = build_ledger();
        assert_eq!(l.len(), SAMPLES);
        assert_eq!(l.ok_count(), 8);
        assert_eq!(l.err_count(), 6);
        assert!(l.audit().is_ok());
        assert_eq!(l.sample(0).map(|x| x.value), Some(1));
        assert_eq!(l.sample(5).map(|x| x.value), Some(6));
        assert_eq!(l.sample(7).map(|x| x.value), Some(1));
        assert_eq!(l.sample(13).map(|x| x.code), Some(BA_ERED));
    }

    #[test]
    fn checks_pass() {
        let s = run_unx11420_checks();
        assert_eq!(s.len(), 13);
        assert!(s.all_passed());
    }

    #[test]
    fn expect_gate() {
        assert_eq!(Batch::expect_gate(0), Err(BA_EFULL));
        assert_eq!(Batch::expect_gate(CAP + 1), Err(BA_EFULL));
        assert_eq!(Batch::expect_gate(1), Ok(1));
        let b = Batch::new(3);
        assert_eq!(b.expect(), 3);
        assert!(b.is_empty());
        assert!(b.consistent());
    }

    #[test]
    fn record_gates() {
        let mut b = Batch::new(4);
        assert_eq!(b.record(0, 1, true, true), Err(BA_ECASE));
        assert_eq!(b.record(CASE_MAX + 1, 1, true, true), Err(BA_ECASE));
        assert_eq!(b.record(1, 0, true, true), Err(BA_EAID));
        assert_eq!(b.record(1, AID_MAX + 1, true, true), Err(BA_EAID));
        assert_eq!(b.record(1, 2, true, true), Ok(1));
        assert_eq!(b.record(1, 1, true, true), Err(BA_EORDER));
        assert_eq!(b.record(1, 2, true, true), Err(BA_EDUP));
        assert_eq!(b.record(2, 1, true, true), Ok(2));
    }

    #[test]
    fn over_and_full() {
        let mut a = Batch::new(2);
        assert_eq!(a.record(1, 1, true, true), Ok(1));
        assert_eq!(a.record(1, 2, true, true), Ok(2));
        assert_eq!(a.record(1, 3, true, true), Err(BA_EOVER));
        let mut g = Batch::new(CAP);
        let mut k = 1u8;
        while k <= 6 {
            assert_eq!(g.record(1, k, true, true), Ok(k as usize));
            k = k.saturating_add(1);
        }
        assert_eq!(g.record(2, 1, true, true), Ok(7));
        assert_eq!(g.record(2, 2, true, true), Ok(8));
        assert_eq!(g.record(2, 3, true, true), Err(BA_EFULL));
        assert!(g.consistent());
    }

    #[test]
    fn finish_aggregate() {
        let b = batch();
        assert_eq!(b.finish(), Ok(6));
        assert_eq!(b.red_count(), 0);
        assert_eq!(b.soft_count(), 1);
        assert_eq!(b.fail_count(), 1);
        let mut short = Batch::new(3);
        assert_eq!(short.record(1, 1, true, true), Ok(1));
        assert_eq!(short.finish(), Err(BA_EMISS));
        let mut red = Batch::new(3);
        assert_eq!(red.record(1, 1, true, true), Ok(1));
        assert_eq!(red.record(1, 2, false, true), Ok(2));
        assert_eq!(red.record(1, 3, true, false), Ok(3));
        assert_eq!(red.finish(), Err(BA_ERED));
        assert_eq!(red.red_count(), 1);
        assert_eq!(red.soft_count(), 0);
    }

    #[test]
    fn assert_at_and_flags() {
        let b = batch();
        assert_eq!(b.assert_at(3), Some(Assert { case: 2, aid: 1, ok: false, hard: false }));
        assert_eq!(b.assert_at(5), Some(Assert { case: 3, aid: 1, ok: true, hard: true }));
        assert!(b.assert_at(6).is_none());
        let mut c = Batch::new(4);
        assert_eq!(c.record(1, 1, true, true), Ok(1));
        assert_eq!(c.record(2, 1, false, false), Ok(2));
        assert_eq!(c.record(2, 2, true, true), Ok(3));
        assert_eq!(c.record(3, 1, false, true), Ok(4));
        assert_eq!(c.finish(), Err(BA_ERED));
        assert_eq!(c.soft_count(), 1);
    }

    #[test]
    fn inject_four_holds() {
        assert_eq!(inject_four(), [true; 4]);
    }
}
