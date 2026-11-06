//! UNX-F11340 B07 批 ktest 断言集聚合（0xeb40 段）
//!
//! 内核自检不是一个巨型断言，而是一批小组：每组跑自己的用例，汇报「总数」
//! 与「通过数」。聚合层最容易出的错不是加错数，而是**把没跑完的组当成跑完
//! 的组**：一个组如果没显式关组，它的计数可能还在一半，直接求和会把「尚未
//! 执行」静默算成「已通过」，最终汇总永远绿。
//!
//! 本模块因此把「登记」「关组」「聚合」拆成三个动作，并让聚合显式检查：
//!
//! - 批里至少要有一个组（空批不能聚合，`EEMPTY`）；
//! - 每个组都必须关组（有开着的组就 `EOPEN`，不求和）；
//! - 通过数不得超过总数（超了就是组自己算错，`EINVAL` 在登记时就拒）。
//!
//! 聚合是**一次性的**：聚合成功即封印整批，此后既不能开新组也不能再关组。
//! 这条约束换来一个强性质——`aggregated_total`/`aggregated_passed` 读到的
//! 数永远对应同一份分组快照，不会读到「聚合后又加进来一个组」的混合态。
//!
//! 闸门次序：
//! - `open`：已封印 → 组号为零 → 重号 → 通过数超总数 → 组表满；
//! - `close_group`：已封印 → 找不到组 → 组已关；
//! - `aggregate`：已封印 → 空批 → 尚有未关组。
//!
//! 注意 `open` 里「重号」排在「通过数超总数」之前：同一个组号重复登记时，
//! 即便这次给的计数合法，也不该被接受；先报重号，结论才是稳定的。

use crate::alloc::format;
use crate::checks::CheckSet;

/// 段基址。
pub const SEG_BASE: u32 = 0xeb40;

/// 组号为零。
pub const KT_EID: u32 = SEG_BASE + 0x01;
/// 组号重复。
pub const KT_EDUP: u32 = SEG_BASE + 0x02;
/// 通过数超过总数。
pub const KT_EINVAL: u32 = SEG_BASE + 0x03;
/// 组表已满。
pub const KT_EFULL: u32 = SEG_BASE + 0x04;
/// 找不到组。
pub const KT_ENOENT: u32 = SEG_BASE + 0x05;
/// 组已关闭。
pub const KT_ECLOSED: u32 = SEG_BASE + 0x06;
/// 批已封印。
pub const KT_ESEALED: u32 = SEG_BASE + 0x07;
/// 尚未聚合。
pub const KT_EWARM: u32 = SEG_BASE + 0x08;
/// 空批不能聚合。
pub const KT_EEMPTY: u32 = SEG_BASE + 0x09;
/// 尚有未关组。
pub const KT_EOPEN: u32 = SEG_BASE + 0x0a;
/// 账本失衡。
pub const KT_EBAL: u32 = SEG_BASE + 0x0b;
/// 账本槽位乱序。
pub const KT_EORDER: u32 = SEG_BASE + 0x0c;
/// 观测值与复算不符或重放不一致。
pub const KT_EMISMATCH: u32 = SEG_BASE + 0x0d;

/// 组容量。
pub const GCAP: usize = 4;
/// 账本格数。
pub const SAMPLES: usize = 14;

/// 操作码：开组。
pub const OP_OPEN: u8 = 1;
/// 操作码：关组。
pub const OP_CLOSE: u8 = 2;
/// 操作码：聚合。
pub const OP_AGG: u8 = 3;
/// 操作码：读聚合数。
pub const OP_READ: u8 = 4;

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

/// 一个测试组。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Group {
    /// 组号。
    pub id: u32,
    /// 断言总数。
    pub total: u32,
    /// 通过数。
    pub passed: u32,
    /// 是否已关组。
    pub closed: bool,
}

impl Group {
    /// 空组。
    pub const fn blank() -> Group {
        Group { id: 0, total: 0, passed: 0, closed: false }
    }

    /// 失败数。
    pub fn failed(&self) -> u32 {
        self.total.saturating_sub(self.passed)
    }
}

/// 一批测试组。
#[derive(Clone, Copy, Debug)]
pub struct Batch {
    g: [Group; GCAP],
    n: usize,
    sealed: bool,
    aggregated: bool,
    agg_total: u32,
    agg_pass: u32,
}

impl Batch {
    /// 空批。
    pub const fn new() -> Batch {
        Batch {
            g: [Group::blank(); GCAP],
            n: 0,
            sealed: false,
            aggregated: false,
            agg_total: 0,
            agg_pass: 0,
        }
    }

    /// 组数。
    pub fn len(&self) -> usize {
        self.n
    }

    /// 是否空批。
    pub fn is_empty(&self) -> bool {
        self.n == 0
    }

    /// 是否已封印。
    pub fn is_sealed(&self) -> bool {
        self.sealed
    }

    /// 是否已聚合。
    pub fn is_aggregated(&self) -> bool {
        self.aggregated
    }

    /// 按组号找下标。
    fn slot(&self, id: u32) -> Option<usize> {
        let mut i = 0usize;
        while i < self.n {
            match self.g.get(i) {
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

    /// 第 i 个组。
    pub fn group_at(&self, i: usize) -> Option<Group> {
        if i >= self.n {
            return None;
        }
        self.g.get(i).copied()
    }

    /// 开一个组，成功返回累计组数。
    pub fn open(&mut self, id: u32, total: u32, passed: u32) -> Result<usize, u32> {
        if self.sealed {
            return Err(KT_ESEALED);
        }
        if id == 0 {
            return Err(KT_EID);
        }
        if self.slot(id).is_some() {
            return Err(KT_EDUP);
        }
        if passed > total {
            return Err(KT_EINVAL);
        }
        if self.n >= GCAP {
            return Err(KT_EFULL);
        }
        if let Some(dst) = self.g.get_mut(self.n) {
            *dst = Group { id, total, passed, closed: false };
        }
        self.n += 1;
        Ok(self.n)
    }

    /// 关一个组，成功返回已关组数。
    pub fn close_group(&mut self, id: u32) -> Result<usize, u32> {
        if self.sealed {
            return Err(KT_ESEALED);
        }
        let i = match self.slot(id) {
            Some(v) => v,
            None => return Err(KT_ENOENT),
        };
        let already = match self.g.get(i) {
            Some(x) => x.closed,
            None => return Err(KT_ENOENT),
        };
        if already {
            return Err(KT_ECLOSED);
        }
        if let Some(dst) = self.g.get_mut(i) {
            dst.closed = true;
        }
        let mut k = 0usize;
        let mut j = 0usize;
        while j < self.n {
            if let Some(x) = self.g.get(j) {
                if x.closed {
                    k += 1;
                }
            }
            j += 1;
        }
        Ok(k)
    }

    /// 聚合全部组，返回断言总数。
    ///
    /// 聚合前逐组检查关组状态：只要有组还开着，就整体拒绝，绝不返回部分和。
    /// 部分和是最危险的返回值——它看起来像一个合法的总数。
    pub fn aggregate(&mut self) -> Result<usize, u32> {
        if self.sealed {
            return Err(KT_ESEALED);
        }
        if self.n == 0 {
            return Err(KT_EEMPTY);
        }
        let mut i = 0usize;
        let mut total = 0u32;
        let mut pass = 0u32;
        while i < self.n {
            let x = match self.g.get(i) {
                Some(v) => *v,
                None => return Err(KT_EEMPTY),
            };
            if !x.closed {
                return Err(KT_EOPEN);
            }
            total = total.saturating_add(x.total);
            pass = pass.saturating_add(x.passed);
            i += 1;
        }
        self.agg_total = total;
        self.agg_pass = pass;
        self.aggregated = true;
        self.sealed = true;
        Ok(total as usize)
    }

    /// 聚合总数。
    pub fn aggregated_total(&self) -> Result<usize, u32> {
        if !self.aggregated {
            return Err(KT_EWARM);
        }
        Ok(self.agg_total as usize)
    }

    /// 聚合通过数。
    pub fn aggregated_passed(&self) -> Result<usize, u32> {
        if !self.aggregated {
            return Err(KT_EWARM);
        }
        Ok(self.agg_pass as usize)
    }

    /// 聚合失败数。
    pub fn aggregated_failed(&self) -> Result<usize, u32> {
        if !self.aggregated {
            return Err(KT_EWARM);
        }
        Ok(self.agg_total.saturating_sub(self.agg_pass) as usize)
    }

    /// 通过率（千分比，向下取整）。空批聚合不可能通过，故分母非零。
    pub fn pass_permille(&self) -> Result<u32, u32> {
        if !self.aggregated {
            return Err(KT_EWARM);
        }
        if self.agg_total == 0 {
            return Ok(0);
        }
        Ok(self.agg_pass.saturating_mul(1000) / self.agg_total)
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

    /// 对账。
    pub fn audit(&self) -> Result<(), u32> {
        if self.len != SAMPLES || self.ok_count() != 8 || self.err_count() != 6 {
            return Err(KT_EBAL);
        }
        const VALS: [u32; 8] = [1, 2, 1, 2, 15, 12, 15, 2];
        const CODES: [u32; 6] = [
            KT_EDUP,
            KT_EINVAL,
            KT_ECLOSED,
            KT_ENOENT,
            KT_EOPEN,
            KT_ESEALED,
        ];
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

    /// 重放。
    pub fn replay(&self) -> Result<u64, u32> {
        let fresh = build_ledger();
        if fresh.seal() == self.seal() {
            Ok(fresh.seal())
        } else {
            Err(KT_EMISMATCH)
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

/// 十四格账本：两组登记 → 重号/超数 → 关组 → 未关组阻断 → 聚合 → 封印后拒绝。
pub fn build_ledger() -> Ledger {
    let mut l = Ledger::new();
    let mut b = Batch::new();

    let _ = l.register(cell(0, OP_OPEN, b.open(1, 10, 7)));
    let _ = l.register(cell(1, OP_OPEN, b.open(2, 5, 5)));
    let _ = l.register(cell(2, OP_OPEN, b.open(1, 1, 1)));
    let _ = l.register(cell(3, OP_OPEN, b.open(3, 4, 6)));
    let _ = l.register(cell(4, OP_CLOSE, b.close_group(1)));
    let _ = l.register(cell(5, OP_CLOSE, b.close_group(1)));
    let _ = l.register(cell(6, OP_CLOSE, b.close_group(9)));
    let _ = l.register(cell(7, OP_AGG, b.aggregate()));
    let _ = l.register(cell(8, OP_CLOSE, b.close_group(2)));
    let _ = l.register(cell(9, OP_AGG, b.aggregate()));
    let _ = l.register(cell(10, OP_READ, b.aggregated_passed()));
    let _ = l.register(cell(11, OP_READ, b.aggregated_total()));
    let _ = l.register(cell(12, OP_CLOSE, b.close_group(9)));
    let _ = l.register(cell(13, OP_OPEN, Ok(b.len())));

    l
}

/// 四路注入。
pub fn inject_four() -> [bool; 4] {
    let mut out = [false; 4];

    let l = build_ledger();
    let rep = l.replay();
    let root = l.seal();
    out[0] = l.len() == SAMPLES && l.audit().is_ok() && rep == Ok(root) && root != 0;

    // 一路：三组求和与通过率。
    let mut a = Batch::new();
    let _ = a.open(1, 10, 7);
    let _ = a.open(2, 5, 5);
    let _ = a.open(3, 2, 1);
    let _ = a.close_group(1);
    let _ = a.close_group(2);
    let _ = a.close_group(3);
    let agg = a.aggregate();
    out[1] = agg == Ok(17)
        && a.is_aggregated()
        && a.is_sealed()
        && a.aggregated_total() == Ok(17)
        && a.aggregated_passed() == Ok(13)
        && a.aggregated_failed() == Ok(4)
        && a.pass_permille() == Ok(764)
        && a.len() == 3
        && a.group_at(2).map(|g| g.failed()) == Some(1)
        && a.group_at(3).is_none()
        && a.close_group(1) == Err(KT_ESEALED)
        && a.open(4, 1, 1) == Err(KT_ESEALED);

    // 二路：开着的组阻断聚合，关掉后才求和。
    let mut b = Batch::new();
    let _ = b.open(5, 3, 2);
    let _ = b.open(6, 4, 1);
    let blocked = b.aggregate();
    let after_close = b.close_group(5);
    let second_close = b.close_group(6);
    let agg = b.aggregate();
    let g2 = b.group_at(1);
    out[2] = blocked == Err(KT_EOPEN)
        && after_close == Ok(1)
        && second_close == Ok(2)
        && agg == Ok(7)
        && g2.map(|x| x.total) == Some(4)
        && b.aggregated_failed() == Ok(4)
        && b.pass_permille() == Ok(428);

    // 三路：闸门（零号 / 重号 / 超数 / 满 / 找不到 / 空批 / 未聚合读取）。
    let mut c = Batch::new();
    let g1 = c.open(0, 1, 0) == Err(KT_EID);
    let g2 = c.open(1, 2, 1) == Ok(1);
    let g3 = c.open(1, 3, 3) == Err(KT_EDUP);
    let g4 = c.open(2, 2, 3) == Err(KT_EINVAL);
    let g5 = c.close_group(9) == Err(KT_ENOENT);
    let g6 = c.close_group(1) == Ok(1);
    let g7 = c.close_group(1) == Err(KT_ECLOSED);
    let mut i = 3u32;
    while (i as usize) <= GCAP + 1 {
        let _ = c.open(i, 1, 1);
        i += 1;
    }
    let g8 = c.open(0x30, 1, 1) == Err(KT_EFULL);
    let g9 = Batch::new().aggregate() == Err(KT_EEMPTY);
    let g10 = Batch::new().aggregated_passed() == Err(KT_EWARM);
    let g11 = Batch::new().aggregated_total() == Err(KT_EWARM);
    let g12 = Batch::new().pass_permille() == Err(KT_EWARM);
    out[3] = g1 && g2 && g3 && g4 && g5 && g6 && g7 && g8 && g9 && g10 && g11 && g12;

    out
}

/// 十三项判据。
pub fn run_unx11340_checks() -> CheckSet {
    let mut s = CheckSet::new("unx11340_b07_assert_agg");
    let l = build_ledger();

    s.add_dyn(
        "KT1-十四格八允六拒",
        l.len() == SAMPLES && l.ok_count() == 8 && l.err_count() == 6 && l.audit().is_ok(),
        &format!("n={} ok={} err={}", l.len(), l.ok_count(), l.err_count()),
    );

    s.add(
        "KT2-分组登记与条数",
        l.sample(0).map(|x| x.value) == Some(1)
            && l.sample(1).map(|x| x.value) == Some(2),
        "static",
    );

    s.add(
        "KT3-重复分组被拒",
        l.sample(2).map(|x| x.code) == Some(KT_EDUP),
        "static",
    );

    s.add(
        "KT4-通过数超总数被拒",
        l.sample(3).map(|x| x.code) == Some(KT_EINVAL),
        "static",
    );

    s.add(
        "KT5-关组计数与重复关组",
        l.sample(4).map(|x| x.value) == Some(1)
            && l.sample(5).map(|x| x.code) == Some(KT_ECLOSED),
        "static",
    );

    s.add(
        "KT6-关不存在的组被拒",
        l.sample(6).map(|x| x.code) == Some(KT_ENOENT),
        "static",
    );

    s.add(
        "KT7-未关组阻断聚合",
        l.sample(7).map(|x| x.code) == Some(KT_EOPEN),
        "static",
    );

    s.add(
        "KT8-全部关组后聚合",
        l.sample(8).map(|x| x.value) == Some(2)
            && l.sample(9).map(|x| x.value) == Some(15),
        "static",
    );

    s.add(
        "KT9-聚合通过数",
        l.sample(10).map(|x| x.value) == Some(12),
        "static",
    );

    s.add(
        "KT10-聚合总数",
        l.sample(11).map(|x| x.value) == Some(15),
        "static",
    );

    s.add(
        "KT11-封印后关组被拒",
        l.sample(12).map(|x| x.code) == Some(KT_ESEALED),
        "static",
    );

    s.add(
        "KT12-失败率与通过率复算",
        {
            let mut b = Batch::new();
            let _ = b.open(1, 8, 6);
            let _ = b.open(2, 4, 2);
            let _ = b.close_group(1);
            let _ = b.close_group(2);
            let agg = b.aggregate();
            agg == Ok(12)
                && b.aggregated_total() == Ok(12)
                && b.aggregated_passed() == Ok(8)
                && b.aggregated_failed() == Ok(4)
                && b.pass_permille() == Ok(666)
                && b.len() == 2
                && l.sample(13).map(|x| x.value) == Some(2)
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
        let s = run_unx11340_checks();
        assert_eq!(s.len(), 13);
        assert!(s.all_passed());
    }

    #[test]
    fn aggregate_only_when_closed() {
        let mut b = Batch::new();
        let _ = b.open(1, 10, 7);
        assert_eq!(b.aggregate(), Err(KT_EOPEN));
        assert_eq!(b.aggregated_total(), Err(KT_EWARM));
        assert_eq!(b.close_group(1), Ok(1));
        assert_eq!(b.aggregate(), Ok(10));
        assert!(b.is_sealed());
        assert_eq!(b.aggregated_passed(), Ok(7));
        assert_eq!(b.aggregated_failed(), Ok(3));
    }

    #[test]
    fn group_gates() {
        let mut b = Batch::new();
        assert_eq!(b.open(0, 1, 1), Err(KT_EID));
        assert_eq!(b.open(1, 2, 1), Ok(1));
        assert_eq!(b.open(1, 2, 2), Err(KT_EDUP));
        assert_eq!(b.open(2, 2, 3), Err(KT_EINVAL));
        assert_eq!(b.close_group(9), Err(KT_ENOENT));
        assert_eq!(b.close_group(1), Ok(1));
        assert_eq!(b.close_group(1), Err(KT_ECLOSED));
    }

    #[test]
    fn empty_batch_cannot_aggregate() {
        let mut b = Batch::new();
        assert_eq!(b.aggregate(), Err(KT_EEMPTY));
        assert_eq!(b.aggregated_passed(), Err(KT_EWARM));
        assert_eq!(b.pass_permille(), Err(KT_EWARM));
    }

    #[test]
    fn sealed_batch_is_read_only() {
        let mut b = Batch::new();
        let _ = b.open(1, 3, 3);
        let _ = b.close_group(1);
        assert_eq!(b.aggregate(), Ok(3));
        assert_eq!(b.open(2, 1, 1), Err(KT_ESEALED));
        assert_eq!(b.close_group(1), Err(KT_ESEALED));
        assert_eq!(b.aggregate(), Err(KT_ESEALED));
        assert_eq!(b.pass_permille(), Ok(1000));
    }

    #[test]
    fn capacity_is_exact() {
        let mut b = Batch::new();
        let mut i = 1u32;
        while (i as usize) <= GCAP {
            assert!(b.open(i, 1, 1).is_ok());
            i += 1;
        }
        assert_eq!(b.open(50, 1, 1), Err(KT_EFULL));
        assert_eq!(b.len(), GCAP);
        assert_eq!(b.group_at(GCAP).is_none(), true);
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
