//! UNX-F11360 B08 批 ktest 断言集聚合（0xec80 段）
//!
//! B08 批的 19 条判据不能各自为政地散落在 19 个可执行体里——那样「全绿」只是
//! 19 次互不相关的口头承诺，「一次命令全跑」也无从谈起。本模块把整批压成一个
//! **断言集**：判据号是主键，期望值是母版里写死的真值，实测值由执行器现场量出，
//! 二者相等才算该条通过。
//!
//! 聚合语义分三层，缺一层都不能判绿：
//!
//! 1. **条数层**：整批必须是 19 条。少一条就说明有判据没进生态面，此时哪怕
//!    已登记的 18 条全过，也不能声称「本批全绿」——覆盖率不是通过率。
//! 2. **裁决层**：每条判据只有三种裁决（通过/失败/跳过）。跳过不是「暂时不算
//!    失败」，它让通过率的分母缺一块，所以整批里只要有一条跳过就整体不绿。
//! 3. **一次性层**：整批聚合是一次性的。封账之后再想单条重跑，得到的不是
//!    新结果而是 `ERUN`——否则「全绿」的封条可以被事后篡改。
//!
//! 单条判据本身允许重复执行（ktest 支持 `--filter` 单跑），重跑会先撤销旧裁决
//! 再落新裁决，三个计数器因此始终等于「当前裁决分布」，不会因为重跑而虚增。
//!
//! 错误分类之所以必须分开，是因为它们的**归责对象不同**：
//!
//! - `EINVAL` / `EDUP` / `EFULL` / `ENOENT` 是**登记面**错误——判据压根没进表，
//!   或进表的方式不合法。这类错误发生在执行之前，与判据内容无关。
//! - `EFAIL` / `ESKIP` 是**裁决面**错误——判据都跑了，但结果不是全通过。
//!   `EFAIL` 说明某条期望与实际不符（真 bug），`ESKIP` 说明某条被主动放弃
//!   （覆盖率缺口），二者的处置方式完全不同，绝不能合并成一个「不绿」。
//! - `EBUDGET` 是**预算面**错误——条数不到 19，或通过数不到 19。它既不是
//!   某条判据失败，也不是某条被跳过，而是整批的资源/规模约束没满足。
//! - `ERUN` 是**状态面**错误——批已封账或该条已有裁决，此刻不允许再写。
//!
//! 闸门次序（文档与实现逐条对齐）：
//!
//! - `add`：零号 → 已封账 → 重号 → 表满；
//! - `run_case`：已封账 → 无此号 → 执行（可重复，先撤旧裁决）；
//! - `skip`：已封账 → 无此号 → 已有裁决 → 落跳过；
//! - `run_all`：已封账 → 逐条补跑未裁决者 → 条数≠19 → 有跳过 → 有失败 →
//!   通过数≠19 → 封账。
//!
//! 注意 `add` 的零号闸门排在最前：表满时插零号得到的是 `EINVAL` 而不是
//! `EFULL`，因为「零号非法」是判据号本身的属性，与表有没有空位无关。这一条
//! 正是闸门次序可被观测的证据。

use crate::alloc::format;
use crate::checks::CheckSet;

/// 段基址。
pub const SEG_BASE: u32 = 0xec80;

/// 判据号非法（零号）。
pub const KT_EINVAL: u32 = SEG_BASE + 0x01;
/// 判据号重复登记。
pub const KT_EDUP: u32 = SEG_BASE + 0x02;
/// 断言表已满。
pub const KT_EFULL: u32 = SEG_BASE + 0x03;
/// 表内无此判据号。
pub const KT_ENOENT: u32 = SEG_BASE + 0x04;
/// 批已封账（或该条已有裁决），此刻不可改写。
pub const KT_ERUN: u32 = SEG_BASE + 0x05;
/// 存在失败判据，整批不得判绿。
pub const KT_EFAIL: u32 = SEG_BASE + 0x06;
/// 存在跳过判据，覆盖率不足百分之百。
pub const KT_ESKIP: u32 = SEG_BASE + 0x07;
/// 判据条数（或通过数）不足 19，整批不可执行。
pub const KT_EBUDGET: u32 = SEG_BASE + 0x08;
/// 账本形状不符（非 14 格 / 非 8 允 6 拒）。
pub const KT_EBAL: u32 = SEG_BASE + 0x09;
/// 账本槽位乱序。
pub const KT_EORDER: u32 = SEG_BASE + 0x0a;
/// 观测值与复算不符。
pub const KT_EMISMATCH: u32 = SEG_BASE + 0x0b;
/// 冻结后重放不一致。
pub const KT_ESEAL: u32 = SEG_BASE + 0x0c;
/// 观察下标越界。
pub const KT_ENOBS: u32 = SEG_BASE + 0x0d;

/// 断言表容量。
pub const CAP: usize = 24;
/// 账本格数。
pub const SAMPLES: usize = 14;
/// 整批判据条数（B08 批母版）。
pub const BATCH: usize = 19;

/// 裁决：尚无。
pub const V_NONE: u8 = 0;
/// 裁决：通过。
pub const V_PASS: u8 = 1;
/// 裁决：失败。
pub const V_FAIL: u8 = 2;
/// 裁决：跳过。
pub const V_SKIP: u8 = 3;

/// 操作码：登记判据。
pub const OP_ADD: u8 = 1;
/// 操作码：单条执行。
pub const OP_RUN: u8 = 2;
/// 操作码：整批聚合。
pub const OP_ALL: u8 = 3;
/// 操作码：读通过率。
pub const OP_RATE: u8 = 4;
/// 操作码：跳过判据。
pub const OP_SKIP: u8 = 5;

/// FNV 偏移基。
pub const FNV_OFF: u64 = 0xcbf2_9ce4_8422_2325;
/// FNV 质数。
pub const FNV_PRIME: u64 = 0x100_0000_01b3;

/// 折入一个 64 位字（8 个字节轮）。
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

/// 判据 `id` 的实测值（执行器现场量出的确定性读数）。
///
/// 真实的 ktest 探针读的是被测函数的行为；本模块把「被测行为」压成这个纯函数，
/// 于是「期望值与实测值是否相等」仍然是一条真实的数据流，而不是写死的布尔。
pub fn measured(id: u32) -> u32 {
    id.wrapping_mul(3).wrapping_add(1) % 97
}

/// 一条断言。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Case {
    /// 判据号（主键，非零）。
    pub id: u32,
    /// 母版期望值。
    pub expect: u32,
    /// 现场实测值（未执行时为 0）。
    pub actual: u32,
    /// 三态裁决。
    pub verdict: u8,
}

impl Case {
    /// 空断言。
    pub const fn blank() -> Case {
        Case { id: 0, expect: 0, actual: 0, verdict: V_NONE }
    }

    /// 是否已有裁决。
    pub fn decided(&self) -> bool {
        self.verdict != V_NONE
    }

    /// 是否通过。
    pub fn passed(&self) -> bool {
        self.verdict == V_PASS
    }

    /// 实测是否与期望一致（仅在已执行时有意义）。
    pub fn agrees(&self) -> bool {
        self.actual == self.expect
    }
}

/// 一批断言。
#[derive(Clone, Copy, Debug)]
pub struct Batch {
    c: [Case; CAP],
    n: usize,
    closed: bool,
    runs: u32,
    passes: u32,
    fails: u32,
    skips: u32,
}

impl Batch {
    /// 空批。
    pub const fn new() -> Batch {
        Batch {
            c: [Case::blank(); CAP],
            n: 0,
            closed: false,
            runs: 0,
            passes: 0,
            fails: 0,
            skips: 0,
        }
    }

    /// 已登记条数。
    pub fn count(&self) -> usize {
        self.n
    }

    /// 是否已封账。
    pub fn is_closed(&self) -> bool {
        self.closed
    }

    /// 累计执行次数（含重跑）。
    pub fn run_count(&self) -> u32 {
        self.runs
    }

    /// 通过条数。
    pub fn pass_count(&self) -> u32 {
        self.passes
    }

    /// 失败条数。
    pub fn fail_count(&self) -> u32 {
        self.fails
    }

    /// 跳过条数。
    pub fn skip_count(&self) -> u32 {
        self.skips
    }

    /// 尚未裁决条数。
    pub fn pending(&self) -> usize {
        let mut k = 0usize;
        let mut i = 0usize;
        while i < self.n {
            match self.c.get(i) {
                Some(x) => {
                    if !x.decided() {
                        k += 1;
                    }
                }
                None => break,
            }
            i += 1;
        }
        k
    }

    /// 第 i 条断言。
    pub fn case_at(&self, i: usize) -> Option<Case> {
        if i >= self.n {
            return None;
        }
        self.c.get(i).copied()
    }

    /// 按号查表内下标。
    fn slot(&self, id: u32) -> Option<usize> {
        let mut i = 0usize;
        while i < self.n {
            match self.c.get(i) {
                Some(x) => {
                    if x.id == id {
                        return Some(i);
                    }
                }
                None => return None,
            }
            i += 1;
        }
        None
    }

    /// 按号查裁决。
    pub fn verdict_of(&self, id: u32) -> Result<u8, u32> {
        match self.slot(id) {
            Some(i) => match self.c.get(i) {
                Some(x) => Ok(x.verdict),
                None => Err(KT_ENOENT),
            },
            None => Err(KT_ENOENT),
        }
    }

    /// 按号查实测值。
    pub fn actual_of(&self, id: u32) -> Result<u32, u32> {
        match self.slot(id) {
            Some(i) => match self.c.get(i) {
                Some(x) => Ok(x.actual),
                None => Err(KT_ENOENT),
            },
            None => Err(KT_ENOENT),
        }
    }

    /// 登记一条判据，返回登记后的条数。
    pub fn add(&mut self, id: u32, expect: u32) -> Result<usize, u32> {
        if id == 0 {
            return Err(KT_EINVAL);
        }
        if self.closed {
            return Err(KT_ERUN);
        }
        if self.slot(id).is_some() {
            return Err(KT_EDUP);
        }
        if self.n >= CAP {
            return Err(KT_EFULL);
        }
        if let Some(dst) = self.c.get_mut(self.n) {
            *dst = Case { id, expect, actual: 0, verdict: V_NONE };
        }
        self.n += 1;
        Ok(self.n)
    }

    /// 落一次裁决：先撤销该条旧裁决，再记新裁决。
    ///
    /// 这是「重跑不虚增计数」的唯一入口——三个裁决计数永远等于当前分布。
    fn apply(&mut self, idx: usize, v: u8) {
        let prev = match self.c.get(idx) {
            Some(x) => x.verdict,
            None => return,
        };
        match prev {
            V_PASS => self.passes = self.passes.saturating_sub(1),
            V_FAIL => self.fails = self.fails.saturating_sub(1),
            V_SKIP => self.skips = self.skips.saturating_sub(1),
            _ => {}
        }
        match v {
            V_PASS => self.passes = self.passes.saturating_add(1),
            V_FAIL => self.fails = self.fails.saturating_add(1),
            V_SKIP => self.skips = self.skips.saturating_add(1),
            _ => {}
        }
        if let Some(x) = self.c.get_mut(idx) {
            x.verdict = v;
        }
    }

    /// 单条执行：实测 → 与期望比对 → 落裁决。返回实测值。
    pub fn run_case(&mut self, id: u32) -> Result<u32, u32> {
        if self.closed {
            return Err(KT_ERUN);
        }
        let idx = match self.slot(id) {
            Some(i) => i,
            None => return Err(KT_ENOENT),
        };
        let expect = match self.c.get(idx) {
            Some(x) => x.expect,
            None => return Err(KT_ENOENT),
        };
        let actual = measured(id);
        if let Some(x) = self.c.get_mut(idx) {
            x.actual = actual;
        }
        let v = if actual == expect { V_PASS } else { V_FAIL };
        self.apply(idx, v);
        self.runs = self.runs.saturating_add(1);
        Ok(actual)
    }

    /// 跳过一条尚未裁决的判据，返回剩余未裁决条数。
    pub fn skip(&mut self, id: u32) -> Result<usize, u32> {
        if self.closed {
            return Err(KT_ERUN);
        }
        let idx = match self.slot(id) {
            Some(i) => i,
            None => return Err(KT_ENOENT),
        };
        let decided = match self.c.get(idx) {
            Some(x) => x.decided(),
            None => return Err(KT_ENOENT),
        };
        if decided {
            return Err(KT_ERUN);
        }
        self.apply(idx, V_SKIP);
        Ok(self.pending())
    }

    /// 整批聚合：一次命令跑全部未裁决者，随后按条数/跳过/失败/通过数四闸判绿。
    pub fn run_all(&mut self) -> Result<usize, u32> {
        if self.closed {
            return Err(KT_ERUN);
        }
        let mut i = 0usize;
        while i < self.n {
            let pair = match self.c.get(i) {
                Some(x) => (x.id, x.verdict),
                None => break,
            };
            if pair.1 == V_NONE {
                let _ = self.run_case(pair.0);
            }
            i += 1;
        }
        if self.n != BATCH {
            return Err(KT_EBUDGET);
        }
        if self.skips != 0 {
            return Err(KT_ESKIP);
        }
        if self.fails != 0 {
            return Err(KT_EFAIL);
        }
        if self.passes != BATCH as u32 {
            return Err(KT_EBUDGET);
        }
        self.closed = true;
        Ok(self.passes as usize)
    }

    /// 通过率（百分比，整数向下取整）。分母为已裁决条数。
    pub fn pass_rate(&self) -> Result<u32, u32> {
        let decided =
            self.passes.saturating_add(self.fails).saturating_add(self.skips);
        if decided == 0 {
            return Err(KT_EINVAL);
        }
        Ok(self.passes.saturating_mul(100) / decided)
    }

    /// 覆盖率（百分比）：已裁决条数 / 已登记条数。
    pub fn coverage(&self) -> u32 {
        if self.n == 0 {
            return 0;
        }
        let decided =
            self.passes.saturating_add(self.fails).saturating_add(self.skips);
        decided.saturating_mul(100) / (self.n as u32)
    }

    /// 整批指纹（含裁决分布），用于冻结对账。
    pub fn digest(&self) -> u64 {
        let mut h = FNV_OFF;
        let mut i = 0usize;
        while i < self.n {
            if let Some(x) = self.c.get(i) {
                h = fold(h, x.id as u64);
                h = fold(h, x.expect as u64);
                h = fold(h, x.actual as u64);
                h = fold(h, x.verdict as u64);
            }
            i += 1;
        }
        fold(h, self.passes as u64)
    }
}

/// 造一批 `n` 条全绿母版断言（期望值即实测值）。
pub fn seed_green(n: u32) -> Batch {
    let mut b = Batch::new();
    let mut id = 1u32;
    while id <= n {
        let _ = b.add(id, measured(id));
        id += 1;
    }
    b
}

/// 造一批 `n` 条断言，其中 `bad` 号的期望值被故意写歪一条。
pub fn seed_poisoned(n: u32, bad: u32) -> Batch {
    let mut b = Batch::new();
    let mut id = 1u32;
    while id <= n {
        let e = if id == bad {
            measured(id).wrapping_add(1)
        } else {
            measured(id)
        };
        let _ = b.add(id, e);
        id += 1;
    }
    b
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

    /// 登记一格（只接受严格下一个槽位）。
    pub fn register(&mut self, s: Sample) -> Result<(), u32> {
        if self.frozen || (s.slot as usize) != self.len || (s.slot as usize) >= SAMPLES {
            self.rejects += 1;
            return Err(KT_EORDER);
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

    /// 对账：形状、槽序、八允六拒、逐格真值。
    pub fn audit(&self) -> Result<(), u32> {
        if self.len != SAMPLES || self.ok_count() != 8 || self.err_count() != 6 {
            return Err(KT_EBAL);
        }
        const VALS: [u32; 8] = [1, 2, 3, 7, 4, 10, 19, 100];
        const CODES: [u32; 6] =
            [KT_EINVAL, KT_EDUP, KT_EFULL, KT_ENOENT, KT_ERUN, KT_EBUDGET];
        let mut vi = 0usize;
        let mut ci = 0usize;
        let mut i = 0usize;
        while i < self.len {
            let s = match self.rows.get(i) {
                Some(v) => *v,
                None => return Err(KT_EBAL),
            };
            if s.slot as usize != i {
                return Err(KT_EORDER);
            }
            if s.ok {
                if s.code != 0 {
                    return Err(KT_EMISMATCH);
                }
                match VALS.get(vi) {
                    Some(w) => {
                        if s.value != *w {
                            return Err(KT_EMISMATCH);
                        }
                    }
                    None => return Err(KT_EBAL),
                }
                vi += 1;
            } else {
                match CODES.get(ci) {
                    Some(w) => {
                        if s.code != *w {
                            return Err(KT_EMISMATCH);
                        }
                    }
                    None => return Err(KT_EBAL),
                }
                ci += 1;
            }
            i += 1;
        }
        if vi != 8 || ci != 6 {
            return Err(KT_EBAL);
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
            return Err(KT_EBAL);
        }
        self.root = self.seal();
        self.frozen = true;
        Ok(self.root)
    }

    /// 重放：重建一份账本并比对指纹。
    pub fn replay(&self) -> Result<u64, u32> {
        let fresh = build_ledger();
        if fresh.seal() == self.seal() {
            Ok(fresh.seal())
        } else {
            Err(KT_ESEAL)
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

/// 十四格账本：登记 → 逐条执行 → 整批聚合 → 通过率 → 六类拒绝。
pub fn build_ledger() -> Ledger {
    let mut l = Ledger::new();

    let mut b1 = Batch::new();
    let _ = l.register(cell(0, OP_ADD, b1.add(1, measured(1))));
    let _ = l.register(cell(1, OP_ADD, b1.add(2, measured(2))));
    let _ = l.register(cell(2, OP_ADD, b1.add(3, measured(3))));
    let _ = l.register(word_cell(3, OP_RUN, b1.run_case(2)));
    let _ = l.register(word_cell(4, OP_RUN, b1.run_case(1)));
    let _ = l.register(word_cell(5, OP_RUN, b1.run_case(3)));

    let mut b19 = seed_green(BATCH as u32);
    let _ = l.register(cell(6, OP_ALL, b19.run_all()));
    let _ = l.register(word_cell(7, OP_RATE, b1.pass_rate()));

    let _ = l.register(cell(8, OP_ADD, b1.add(0, 5)));

    let mut dup = Batch::new();
    let _ = dup.add(2, measured(2));
    let _ = l.register(cell(9, OP_ADD, dup.add(2, measured(2))));

    let mut full = Batch::new();
    let mut i = 1u32;
    while (i as usize) <= CAP {
        let _ = full.add(i, measured(i));
        i += 1;
    }
    let _ = l.register(cell(10, OP_ADD, full.add(0x100, measured(0x100))));

    let _ = l.register(word_cell(11, OP_RUN, b1.run_case(99)));

    let _ = l.register(word_cell(12, OP_RUN, b19.run_case(1)));

    let _ = l.register(cell(13, OP_ALL, b1.run_all()));

    l
}

/// 四路注入。
pub fn inject_four() -> [bool; 4] {
    let mut out = [false; 4];

    // 零路：账本自洽、审计通过、重放等值、指纹非零。
    let l = build_ledger();
    let rep = l.replay();
    let root = l.seal();
    out[0] = l.len() == SAMPLES && l.audit().is_ok() && rep == Ok(root) && root != 0;

    // 一路：整批 19 条全绿，计数精确，封账后拒绝重跑。
    let mut a = seed_green(BATCH as u32);
    let all = a.run_all();
    let rate = a.pass_rate();
    let again = a.run_all();
    let rerun = a.run_case(1);
    out[1] = all == Ok(BATCH)
        && rate == Ok(100)
        && again == Err(KT_ERUN)
        && rerun == Err(KT_ERUN)
        && a.is_closed()
        && a.pass_count() == BATCH as u32
        && a.fail_count() == 0
        && a.skip_count() == 0
        && a.coverage() == 100;

    // 二路：四类登记面失败互不相同（零号/重号/无号/表满）。
    let mut g = Batch::new();
    let e1 = g.add(0, 1);
    let _ = g.add(5, measured(5));
    let e2 = g.add(5, measured(5));
    let e3 = g.run_case(77);
    let mut h = Batch::new();
    let mut i = 1u32;
    while (i as usize) <= CAP {
        let _ = h.add(i, measured(i));
        i += 1;
    }
    let e4 = h.add(0x200, 1);
    let codes = [
        match e1 {
            Err(c) => c,
            Ok(_) => 0,
        },
        match e2 {
            Err(c) => c,
            Ok(_) => 0,
        },
        match e3 {
            Err(c) => c,
            Ok(_) => 0,
        },
        match e4 {
            Err(c) => c,
            Ok(_) => 0,
        },
    ];
    let mut distinct = true;
    let mut p = 0usize;
    while p < 4 {
        let mut q = p + 1;
        while q < 4 {
            let a1 = match codes.get(p) {
                Some(v) => *v,
                None => 0,
            };
            let b1 = match codes.get(q) {
                Some(v) => *v,
                None => 0,
            };
            if a1 == 0 || a1 == b1 {
                distinct = false;
            }
            q += 1;
        }
        p += 1;
    }
    out[2] = distinct && e1 == Err(KT_EINVAL) && e2 == Err(KT_EDUP)
        && e3 == Err(KT_ENOENT) && e4 == Err(KT_EFULL);

    // 三路：闸门次序与边界。满表时零号仍报 EINVAL、重号仍报 EDUP、新号才报 EFULL。
    let mut f = Batch::new();
    let mut k = 1u32;
    while (k as usize) <= CAP {
        let _ = f.add(k, measured(k));
        k += 1;
    }
    let g1 = f.add(0, 1) == Err(KT_EINVAL);
    let g2 = f.add(1, 1) == Err(KT_EDUP);
    let g3 = f.add(0x300, 1) == Err(KT_EFULL);
    let g4 = f.pass_rate() == Err(KT_EINVAL);

    let mut s19 = seed_green(BATCH as u32);
    let _ = s19.skip(4);
    let skip_twice = s19.skip(4);
    let all_skip = s19.run_all();
    out[3] = g1
        && g2
        && g3
        && g4
        && skip_twice == Err(KT_ERUN)
        && all_skip == Err(KT_ESKIP)
        && s19.skip_count() == 1
        && s19.pass_count() == (BATCH as u32) - 1;

    out
}

/// 十三项判据。
pub fn run_unx11360_checks() -> CheckSet {
    let mut s = CheckSet::new("unx11360.b08_ktest_agg");
    let l = build_ledger();

    s.add_dyn(
        "KT1-十四格八允六拒",
        l.len() == SAMPLES && l.ok_count() == 8 && l.err_count() == 6 && l.audit().is_ok(),
        &format!("n={} ok={} err={}", l.len(), l.ok_count(), l.err_count()),
    );

    s.add(
        "KT2-登记序号逐条递增",
        l.sample(0).map(|x| x.value) == Some(1)
            && l.sample(1).map(|x| x.value) == Some(2)
            && l.sample(2).map(|x| x.value) == Some(3),
        "static",
    );

    s.add(
        "KT3-逐条执行回报实测值",
        l.sample(3).map(|x| x.value) == Some(measured(2))
            && l.sample(4).map(|x| x.value) == Some(measured(1))
            && l.sample(5).map(|x| x.value) == Some(measured(3)),
        "static",
    );

    s.add(
        "KT4-整批一次聚合返通过数",
        l.sample(6).map(|x| x.value) == Some(BATCH as u32),
        "static",
    );

    s.add(
        "KT5-全绿时通过率为百",
        l.sample(7).map(|x| x.value) == Some(100),
        "static",
    );

    s.add(
        "KT6-零号判据登记被拒",
        l.sample(8).map(|x| x.code) == Some(KT_EINVAL),
        "static",
    );

    s.add(
        "KT7-重号登记被拒",
        l.sample(9).map(|x| x.code) == Some(KT_EDUP),
        "static",
    );

    s.add(
        "KT8-断言表满拒新号",
        l.sample(10).map(|x| x.code) == Some(KT_EFULL),
        "static",
    );

    s.add(
        "KT9-未知判据不可执行",
        l.sample(11).map(|x| x.code) == Some(KT_ENOENT),
        "static",
    );

    s.add(
        "KT10-封账后单条重跑被拒",
        l.sample(12).map(|x| x.code) == Some(KT_ERUN),
        "static",
    );

    s.add(
        "KT11-条数不足整批被拒",
        l.sample(13).map(|x| x.code) == Some(KT_EBUDGET),
        "static",
    );

    s.add(
        "KT12-失败与跳过各自成类",
        {
            let mut bad = seed_poisoned(BATCH as u32, 7);
            let f = bad.run_all();
            let mut sk = seed_green(BATCH as u32);
            let _ = sk.skip(9);
            let k = sk.run_all();
            let mut ok = seed_green(BATCH as u32);
            f == Err(KT_EFAIL)
                && k == Err(KT_ESKIP)
                && f != k
                && bad.fail_count() == 1
                && sk.skip_count() == 1
                && ok.run_all() == Ok(BATCH)
        },
        "static",
    );

    s.add(
        "KT13-冻结重放四注入",
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
        let s = run_unx11360_checks();
        assert_eq!(s.len(), 13);
        assert!(s.all_passed());
    }

    #[test]
    fn green_batch_closes_once() {
        let mut b = seed_green(BATCH as u32);
        assert_eq!(b.run_all(), Ok(BATCH));
        assert!(b.is_closed());
        assert_eq!(b.pass_rate(), Ok(100));
        assert_eq!(b.run_all(), Err(KT_ERUN));
        assert_eq!(b.run_case(3), Err(KT_ERUN));
        assert_eq!(b.add(BATCH as u32 + 1, 1), Err(KT_ERUN));
    }

    #[test]
    fn poisoned_batch_reports_fail() {
        let mut b = seed_poisoned(BATCH as u32, 5);
        assert_eq!(b.run_all(), Err(KT_EFAIL));
        assert_eq!(b.fail_count(), 1);
        assert_eq!(b.verdict_of(5), Ok(V_FAIL));
        assert!(!b.is_closed());
    }

    #[test]
    fn skip_blocks_green() {
        let mut b = seed_green(BATCH as u32);
        assert_eq!(b.skip(2), Ok(BATCH - 1));
        assert_eq!(b.verdict_of(2), Ok(V_SKIP));
        assert_eq!(b.skip(2), Err(KT_ERUN));
        assert_eq!(b.run_all(), Err(KT_ESKIP));
        assert_eq!(b.coverage(), 100);
    }

    #[test]
    fn gate_order_on_full_table() {
        let mut b = Batch::new();
        let mut i = 1u32;
        while (i as usize) <= CAP {
            assert!(b.add(i, measured(i)).is_ok());
            i += 1;
        }
        assert_eq!(b.add(0, 0), Err(KT_EINVAL));
        assert_eq!(b.add(1, 0), Err(KT_EDUP));
        assert_eq!(b.add(0x400, 0), Err(KT_EFULL));
        assert_eq!(b.count(), CAP);
    }

    #[test]
    fn rerun_does_not_inflate() {
        let mut b = Batch::new();
        let _ = b.add(1, measured(1));
        let _ = b.run_case(1);
        let _ = b.run_case(1);
        assert_eq!(b.run_count(), 2);
        assert_eq!(b.pass_count(), 1);
        assert_eq!(b.fail_count(), 0);
        assert_eq!(b.pass_rate(), Ok(100));
    }
}
