//! UNX-F11518 comm 三列差集/交集
//!
//! `comm` 把两个**已排序**文件按行做三路归并，输出三列：
//!
//! - **第 1 列**：只在甲侧出现的行；
//! - **第 2 列**：只在乙侧出现的行；
//! - **第 3 列**：两侧都有的行（交集）。
//!
//! 三列靠**制表符缩进**区分：第 1 列顶格，第 2 列前一个 `\t`，第 3 列前两个
//! `\t`。所以 `-1`/`-2`/`-3` 不是「不打印这一列」那么简单——被压掉的列连它
//! 那一格缩进也一起消失，后面的列整体左移。本实现在 `leads(col)` 里把这件事
//! 算成「第 col 列之前还有几个**未抑制**的列」，缩进数就等于它，绝不在输出时
//! 数临时状态。
//!
//! 归并是双指针，**不走哈希、不做排序**：两侧一旦逆序，双指针会静默给出错误
//! 的差集。所以先逐行验序（`CM_EORDER`），再验行内字节（`\t` 会把列定位弄乱，
//! NUL 会让 C 字符串语义的消费者截断），再验行长与容量。闸门次序：
//!
//! 掩码（构造期）→ 行数 → 行字节 → 有序。
//!
//! 容量在前：行数越界比「某行多了一个字节」更该先报，因为它决定整张行表放不
//! 放得下。
//!
//! 重复行按 POSIX 的成对消解：甲侧两个 `a`、乙侧一个 `a`，则一个 `a` 进第 3
//! 列、另一个 `a` 进第 1 列；两侧各两个 `a` 则两个都进第 3 列。这不是「集合」
//! 语义，而是「多重集按出现次数取交」。
//!
//! 账本语义：十四格 = 八格成功（交集计数、缺省字节、三种抑制、重复消解、空侧、
//! 同侧）+ 六格失败（掩码两种、乱序、制表、表满、NUL）。`audit` 认「八值六
//! 码」，`seal` 做 FNV-1a 折叠，`replay` 重建比对，`freeze` 后登记一律拒绝。

use crate::alloc::format;
use crate::checks::CheckSet;

/// 段基址。
pub const SEG_BASE: u32 = 0xf170;

/// 输入未按字节序升序。
pub const CM_EORDER: u32 = SEG_BASE + 0x01;
/// 抑制掩码非法。
pub const CM_EMASK: u32 = SEG_BASE + 0x02;
/// 行内含制表符。
pub const CM_ETAB: u32 = SEG_BASE + 0x03;
/// 行含 NUL 字节。
pub const CM_ENUL: u32 = SEG_BASE + 0x04;
/// 单行超过长度上限。
pub const CM_ECAP: u32 = SEG_BASE + 0x05;
/// 一侧行数超过行表。
pub const CM_EFULL: u32 = SEG_BASE + 0x06;
/// 总输出越过字节预算。
pub const CM_EBYTES: u32 = SEG_BASE + 0x07;
/// 列号越界。
pub const CM_ECOL: u32 = SEG_BASE + 0x08;
/// 账本形状不符。
pub const CM_EBAL: u32 = SEG_BASE + 0x09;
/// 槽位乱序。
pub const CM_ESLOT: u32 = SEG_BASE + 0x0a;
/// 观测值与复算不符。
pub const CM_EVALUE: u32 = SEG_BASE + 0x0b;
/// 冻结后重放不一致。
pub const CM_ESEAL: u32 = SEG_BASE + 0x0c;
/// 观察下标越界。
pub const CM_ENOBS: u32 = SEG_BASE + 0x0d;

/// 单侧行数上限。
pub const ROWS: usize = 8;
/// 单行字节上限。
pub const LINE_MAX: u32 = 64;
/// 单次输出总字节上限。
pub const OUT_CAP: u32 = 512;
/// 账本格数。
pub const SAMPLES: usize = 14;

/// 操作码：构造。
pub const OP_BUILD: u8 = 1;
/// 操作码：归并。
pub const OP_RUN: u8 = 2;
/// 操作码：缩进复算。
pub const OP_LEAD: u8 = 3;

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

/// 定长取字节（越界给 0）。
fn byte_at(s: &[u8], i: usize) -> u8 {
    match s.get(i) {
        Some(v) => *v,
        None => 0,
    }
}

/// 三列缺省掩码：一位一列，置位表示**抑制**。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Opts {
    mask: u8,
}

impl Opts {
    /// 缺省：三列全出。
    pub const fn new() -> Opts {
        Opts { mask: 0 }
    }

    /// `-1` / `-2` / `-3`。
    pub fn sup(self, one: bool, two: bool, three: bool) -> Opts {
        let mut o = self;
        let mut m = 0u8;
        if one {
            m |= 0b001;
        }
        if two {
            m |= 0b010;
        }
        if three {
            m |= 0b100;
        }
        o.mask = m;
        o
    }

    /// 定型并过闸。
    pub fn build(&self) -> Result<Comm, u32> {
        Comm::new(self.mask)
    }

    /// 原始掩码。
    pub fn mask(&self) -> u8 {
        self.mask
    }
}

/// 一次归并的结论。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Report {
    /// 只在甲侧的行数。
    pub only1: u32,
    /// 只在乙侧的行数。
    pub only2: u32,
    /// 两侧都有的行数。
    pub both: u32,
    /// 输出总字节。
    pub bytes: u32,
}

/// 定型后的三路归并器。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Comm {
    mask: u8,
}

impl Comm {
    /// 从原始掩码定型；`0` 合法（全出），`7` 与越界位一律拒绝。
    pub const fn new(mask: u8) -> Result<Comm, u32> {
        if mask & 0b1111_1000 != 0 {
            return Err(CM_EMASK);
        }
        if mask & 0b111 == 0b111 {
            return Err(CM_EMASK);
        }
        Ok(Comm { mask })
    }

    /// 原始掩码。
    pub fn mask(&self) -> u8 {
        self.mask
    }

    /// 第 col 列是否被抑制；`col` 越界按「未抑制」处理，调用方先用 `leads`。
    pub fn suppressed(&self, col: usize) -> bool {
        if col > 2 {
            return false;
        }
        (self.mask >> col) & 1 == 1
    }

    /// 第 col 列前应有的制表符个数。
    pub fn leads(&self, col: usize) -> Result<u32, u32> {
        if col > 2 {
            return Err(CM_ECOL);
        }
        let mut n = 0u32;
        let mut i = 0usize;
        while i < col {
            if !self.suppressed(i) {
                n = n.saturating_add(1);
            }
            i += 1;
        }
        Ok(n)
    }

    /// 单侧扫一遍：容量 → 行字节 → 有序。
    fn scan(&self, side: &[&'static str]) -> Result<(), u32> {
        if side.len() > ROWS {
            return Err(CM_EFULL);
        }
        let mut i = 0usize;
        while i < side.len() {
            let l = match side.get(i) {
                Some(v) => *v,
                None => break,
            };
            if l.len() as u32 > LINE_MAX {
                return Err(CM_ECAP);
            }
            let b = l.as_bytes();
            let mut k = 0usize;
            while k < b.len() {
                let c = byte_at(b, k);
                if c == 0 {
                    return Err(CM_ENUL);
                }
                if c == b'\t' {
                    return Err(CM_ETAB);
                }
                k += 1;
            }
            if i > 0 {
                if let Some(p) = side.get(i - 1) {
                    if l < *p {
                        return Err(CM_EORDER);
                    }
                }
            }
            i += 1;
        }
        Ok(())
    }

    /// 一行输出的字节数：缩进 + 内容 + 换行。
    fn line_bytes(&self, col: usize, line: &str) -> Result<u32, u32> {
        let lead = self.leads(col)?;
        let n = lead
            .saturating_add(line.len() as u32)
            .saturating_add(1);
        if n > LINE_MAX.saturating_add(2) {
            return Err(CM_EBYTES);
        }
        Ok(n)
    }

    /// 双指针三路归并。
    pub fn run(&self, a: &[&'static str], b: &[&'static str]) -> Result<Report, u32> {
        self.scan(a)?;
        self.scan(b)?;
        let mut r = Report { only1: 0, only2: 0, both: 0, bytes: 0 };
        let mut i = 0usize;
        let mut j = 0usize;
        let push = |col: usize, line: &str, r: &mut Report| -> Result<(), u32> {
            let n = self.line_bytes(col, line)?;
            r.bytes = r.bytes.saturating_add(n);
            if r.bytes > OUT_CAP {
                return Err(CM_EBYTES);
            }
            Ok(())
        };
        while i < a.len() && j < b.len() {
            let x = match a.get(i) {
                Some(v) => *v,
                None => break,
            };
            let y = match b.get(j) {
                Some(v) => *v,
                None => break,
            };
            if x < y {
                if !self.suppressed(0) {
                    r.only1 = r.only1.saturating_add(1);
                    push(0, x, &mut r)?;
                }
                i += 1;
            } else if y < x {
                if !self.suppressed(1) {
                    r.only2 = r.only2.saturating_add(1);
                    push(1, y, &mut r)?;
                }
                j += 1;
            } else {
                if !self.suppressed(2) {
                    r.both = r.both.saturating_add(1);
                    push(2, x, &mut r)?;
                }
                i += 1;
                j += 1;
            }
        }
        while i < a.len() {
            let x = match a.get(i) {
                Some(v) => *v,
                None => break,
            };
            if !self.suppressed(0) {
                r.only1 = r.only1.saturating_add(1);
                push(0, x, &mut r)?;
            }
            i += 1;
        }
        while j < b.len() {
            let y = match b.get(j) {
                Some(v) => *v,
                None => break,
            };
            if !self.suppressed(1) {
                r.only2 = r.only2.saturating_add(1);
                push(1, y, &mut r)?;
            }
            j += 1;
        }
        Ok(r)
    }

    /// 不变式自检：掩码合法、三列缩进单调不减且末列缩进不超过 2。
    pub fn consistent(&self) -> bool {
        if self.mask & 0b1111_1000 != 0 || self.mask & 0b111 == 0b111 {
            return false;
        }
        let mut prev = 0u32;
        let mut c = 0usize;
        while c < 3 {
            let l = match self.leads(c) {
                Ok(v) => v,
                Err(_) => return false,
            };
            if l < prev || l > 2 {
                return false;
            }
            prev = l;
            c += 1;
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
            self.rejects = self.rejects.saturating_add(1);
            return Err(CM_ESLOT);
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

    /// 对账：八成功值 + 六失败码，且槽位连续。
    pub fn audit(&self) -> Result<(), u32> {
        if self.len != SAMPLES || self.ok_count() != 8 || self.err_count() != 6 {
            return Err(CM_EBAL);
        }
        const VALS: [u32; 8] = [2, 18, 10, 2, 4, 13, 12, 16];
        const CODES: [u32; 6] =
            [CM_EMASK, CM_EMASK, CM_EORDER, CM_ETAB, CM_EFULL, CM_ENUL];
        let mut vi = 0usize;
        let mut ci = 0usize;
        let mut i = 0usize;
        while i < self.len {
            let s = match self.rows.get(i) {
                Some(v) => *v,
                None => return Err(CM_EBAL),
            };
            if s.slot as usize != i {
                return Err(CM_ESLOT);
            }
            if s.ok {
                if s.code != 0 {
                    return Err(CM_EVALUE);
                }
                match VALS.get(vi) {
                    Some(w) => {
                        if s.value != *w {
                            return Err(CM_EVALUE);
                        }
                    }
                    None => return Err(CM_EBAL),
                }
                vi += 1;
            } else {
                match CODES.get(ci) {
                    Some(w) => {
                        if s.code != *w {
                            return Err(CM_EVALUE);
                        }
                    }
                    None => return Err(CM_EBAL),
                }
                ci += 1;
            }
            i += 1;
        }
        if vi != 8 || ci != 6 {
            return Err(CM_EBAL);
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
            self.rejects = self.rejects.saturating_add(1);
            return Err(CM_EBAL);
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
            Err(CM_ESEAL)
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

/// 甲侧：a、c、e、g。
const A: [&str; 4] = ["a", "c", "e", "g"];
/// 乙侧：b、c、f、g。
const B: [&str; 4] = ["b", "c", "f", "g"];
/// 计数字例甲侧。
const DA: [&str; 3] = ["a", "a", "b"];
/// 计数字例乙侧。
const DB: [&str; 3] = ["a", "b", "b"];
/// 乱序甲侧。
const BAD: [&str; 2] = ["b", "a"];

/// 六十四字节长行，用于顶穿总预算。
const L0: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const L1: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
const L2: &str = "cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc";
const L3: &str = "dddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddd";
const L4: &str = "eeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeeee";
const L5: &str = "ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff";
const L6: &str = "gggggggggggggggggggggggggggggggggggggggggggggggggggggggggggggggg";
const L7: &str = "hhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhhh";

/// 起手。
fn mk(o: Opts) -> Result<Comm, u32> {
    o.build()
}

/// 十四格账本：交集 → 缺省字节 → 三种抑制 → 重复 → 空侧 → 同侧。
pub fn build_ledger() -> Ledger {
    let mut l = Ledger::new();
    let base = match mk(Opts::new()) {
        Ok(v) => v,
        Err(_) => return l,
    };

    let _ = l.register(word_cell(0, OP_RUN, base.run(&A, &B).map(|r| r.both)));
    let _ = l.register(word_cell(1, OP_RUN, base.run(&A, &B).map(|r| r.bytes)));
    let _ = l.register(word_cell(
        2,
        OP_RUN,
        mk(Opts::new().sup(true, false, false)).and_then(|c| c.run(&A, &B)).map(|r| r.bytes),
    ));
    let _ = l.register(word_cell(
        3,
        OP_RUN,
        mk(Opts::new().sup(true, true, false)).and_then(|c| c.run(&A, &B)).map(|r| r.both),
    ));
    let _ = l.register(word_cell(
        4,
        OP_RUN,
        mk(Opts::new().sup(true, false, true)).and_then(|c| c.run(&A, &B)).map(|r| r.bytes),
    ));
    let _ = l.register(word_cell(5, OP_RUN, base.run(&DA, &DB).map(|r| r.bytes)));
    let _ = l.register(word_cell(6, OP_RUN, base.run(&[], &B).map(|r| r.bytes)));
    let _ = l.register(word_cell(7, OP_RUN, base.run(&A, &A).map(|r| r.bytes)));

    let _ = l.register(word_cell(8, OP_BUILD, Comm::new(0b1000).map(|c| c.mask() as u32)));
    let _ = l.register(word_cell(9, OP_BUILD, Comm::new(0b111).map(|c| c.mask() as u32)));
    let _ = l.register(word_cell(10, OP_RUN, base.run(&BAD, &B).map(|r| r.only1)));
    let _ = l.register(word_cell(11, OP_RUN, base.run(&["a\tb", "c"], &B).map(|r| r.only1)));
    let _ = l.register(word_cell(
        12,
        OP_RUN,
        base.run(&["a", "b", "c", "d", "e", "f", "g", "h", "i"], &B).map(|r| r.only1),
    ));
    let _ = l.register(word_cell(13, OP_RUN, base.run(&["a\u{0}", "c"], &B).map(|r| r.only1)));

    l
}

/// 四路注入。
pub fn inject_four() -> [bool; 4] {
    let mut out = [false; 4];

    let l = build_ledger();
    let rep = l.replay();
    let root = l.seal();
    out[0] = l.len() == SAMPLES && l.audit().is_ok() && rep == Ok(root) && root != 0;

    // 一路：缺省三列的计数与字节。
    let base = match mk(Opts::new()) {
        Ok(v) => v,
        Err(_) => return out,
    };
    out[1] = base.run(&A, &B)
        == Ok(Report { only1: 2, only2: 2, both: 2, bytes: 18 })
        && base.consistent()
        && base.leads(0) == Ok(0)
        && base.leads(1) == Ok(1)
        && base.leads(2) == Ok(2);

    // 二路：抑制后缩进塌缩。
    let s1 = match mk(Opts::new().sup(true, false, false)) {
        Ok(v) => v,
        Err(_) => return out,
    };
    let s12 = match mk(Opts::new().sup(true, true, false)) {
        Ok(v) => v,
        Err(_) => return out,
    };
    out[2] = s1.leads(1) == Ok(0)
        && s1.leads(2) == Ok(1)
        && s1.run(&A, &B).map(|r| r.bytes) == Ok(10)
        && s12.leads(1) == Ok(0)
        && s12.leads(2) == Ok(0)
        && s12.run(&A, &B).map(|r| r.bytes) == Ok(4)
        && s1.consistent()
        && s12.consistent();

    // 三路：闸门与重复消解。
    let dup = base.run(&DA, &DB);
    out[3] = dup == Ok(Report { only1: 1, only2: 1, both: 2, bytes: 13 })
        && base.run(&BAD, &B) == Err(CM_EORDER)
        && base.run(&A, &BAD) == Err(CM_EORDER)
        && base.run(&["a\tb"], &B) == Err(CM_ETAB)
        && base.run(&["a\u{0}b"], &B) == Err(CM_ENUL)
        && base.run(&[L0; 9], &B) == Err(CM_EFULL)
        && base.leads(3) == Err(CM_ECOL)
        && Comm::new(0b111) == Err(CM_EMASK)
        && Comm::new(0b1000) == Err(CM_EMASK);

    out
}

/// 十三项判据。
pub fn run_unx11518_checks() -> CheckSet {
    let mut s = CheckSet::new("unx11518.comm_three_column");
    let l = build_ledger();

    s.add_dyn(
        "CM1-十四格八允六拒",
        l.len() == SAMPLES && l.ok_count() == 8 && l.err_count() == 6 && l.audit().is_ok(),
        &format!("n={} ok={} err={}", l.len(), l.ok_count(), l.err_count()),
    );

    s.add(
        "CM2-三列差集交集计数",
        {
            let c = match mk(Opts::new()) {
                Ok(v) => v,
                Err(_) => return s,
            };
            matches!(c.run(&A, &B), Ok(ref r) if r.only1 == 2 && r.only2 == 2 && r.both == 2)
                && l.sample(0).map(|x| x.value) == Some(2)
        },
        "static",
    );

    s.add(
        "CM3-抑制列塌缩缩进",
        {
            let a = match mk(Opts::new().sup(true, false, false)) {
                Ok(v) => v,
                Err(_) => return s,
            };
            let b = match mk(Opts::new().sup(true, true, false)) {
                Ok(v) => v,
                Err(_) => return s,
            };
            a.leads(0) == Ok(0)
                && a.leads(1) == Ok(0)
                && a.leads(2) == Ok(1)
                && b.leads(1) == Ok(0)
                && b.leads(2) == Ok(0)
                && a.run(&A, &B).map(|r| r.bytes) == Ok(10)
                && b.run(&A, &B).map(|r| r.both) == Ok(2)
                && b.run(&A, &B).map(|r| r.bytes) == Ok(4)
        },
        "static",
    );

    s.add(
        "CM4-重复行成对消解",
        {
            let c = match mk(Opts::new()) {
                Ok(v) => v,
                Err(_) => return s,
            };
            let two: [&str; 2] = ["a", "a"];
            let one: [&str; 1] = ["a"];
            matches!(c.run(&DA, &DB), Ok(ref r) if r.only1 == 1 && r.only2 == 1 && r.both == 2)
                && matches!(c.run(&two, &one), Ok(ref r) if r.only1 == 1 && r.both == 1)
                && matches!(c.run(&two, &two), Ok(ref r) if r.only1 == 0 && r.both == 2)
        },
        "static",
    );

    s.add(
        "CM5-乱序两侧都拒",
        {
            let c = match mk(Opts::new()) {
                Ok(v) => v,
                Err(_) => return s,
            };
            let mixed: [&str; 4] = ["a", "b", "a", "b"];
            c.run(&BAD, &B) == Err(CM_EORDER)
                && c.run(&A, &BAD) == Err(CM_EORDER)
                && c.run(&mixed, &B) == Err(CM_EORDER)
                && c.run(&[], &[]) == Ok(Report { only1: 0, only2: 0, both: 0, bytes: 0 })
                && l.sample(10).map(|x| x.code) == Some(CM_EORDER)
        },
        "static",
    );

    s.add(
        "CM6-掩码闸门",
        Comm::new(0b111) == Err(CM_EMASK)
            && Comm::new(0b1000) == Err(CM_EMASK)
            && Comm::new(0xff) == Err(CM_EMASK)
            && Comm::new(0).is_ok()
            && Comm::new(0b110).is_ok()
            && matches!(mk(Opts::new().sup(false, false, false)), Ok(ref c) if c.mask() == 0)
            && l.sample(9).map(|x| x.code) == Some(CM_EMASK),
        "static",
    );

    s.add(
        "CM7-制表与NUL先行",
        {
            let c = match mk(Opts::new()) {
                Ok(v) => v,
                Err(_) => return s,
            };
            c.run(&["a\tb"], &B) == Err(CM_ETAB)
                && c.run(&A, &["c\td"]) == Err(CM_ETAB)
                && c.run(&["a\u{0}b"], &B) == Err(CM_ENUL)
                && c.run(&A, &["g\u{0}"]) == Err(CM_ENUL)
                && l.sample(11).map(|x| x.code) == Some(CM_ETAB)
                && l.sample(13).map(|x| x.code) == Some(CM_ENUL)
        },
        "static",
    );

    s.add(
        "CM8-表满与行超长",
        {
            let c = match mk(Opts::new()) {
                Ok(v) => v,
                Err(_) => return s,
            };
            let nine: [&str; 9] =
                ["a", "b", "c", "d", "e", "f", "g", "h", "i"];
            let long: [&str; 1] = ["yyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyy"];
            c.run(&nine, &B) == Err(CM_EFULL)
                && c.run(&A, &nine) == Err(CM_EFULL)
                && c.run(&long, &B) == Err(CM_ECAP)
                && c.run(&[L0], &B).is_ok()
                && l.sample(12).map(|x| x.code) == Some(CM_EFULL)
        },
        "static",
    );

    s.add(
        "CM9-总字节预算",
        {
            let c = match mk(Opts::new()) {
                Ok(v) => v,
                Err(_) => return s,
            };
            let tall: [&str; 8] = [L0, L1, L2, L3, L4, L5, L6, L7];
            let seven: [&str; 7] = [L0, L1, L2, L3, L4, L5, L6];
            let none: [&str; 0] = [];
            matches!(c.run(&seven, &none), Ok(ref r) if r.bytes == 7 * 65)
                && c.run(&tall, &none) == Err(CM_EBYTES)
                && c.run(&tall, &tall) == Err(CM_EBYTES)
        },
        "static",
    );

    s.add(
        "CM10-列号越界",
        {
            let c = match mk(Opts::new()) {
                Ok(v) => v,
                Err(_) => return s,
            };
            c.leads(0) == Ok(0)
                && c.leads(1) == Ok(1)
                && c.leads(2) == Ok(2)
                && c.leads(3) == Err(CM_ECOL)
                && c.leads(usize::MAX) == Err(CM_ECOL)
                && c.suppressed(3) == false
        },
        "static",
    );

    s.add(
        "CM11-空侧与同侧",
        {
            let c = match mk(Opts::new()) {
                Ok(v) => v,
                Err(_) => return s,
            };
            let none: [&str; 0] = [];
            matches!(c.run(&none, &B), Ok(ref r) if r.only2 == 4 && r.both == 0)
                && matches!(c.run(&A, &none), Ok(ref r) if r.only1 == 4 && r.only2 == 0)
                && matches!(c.run(&A, &A), Ok(ref r) if r.both == 4 && r.bytes == 16)
                && l.sample(6).map(|x| x.value) == Some(12)
                && l.sample(7).map(|x| x.value) == Some(16)
        },
        "static",
    );

    s.add(
        "CM12-字节账逐列可加",
        {
            let c = match mk(Opts::new()) {
                Ok(v) => v,
                Err(_) => return s,
            };
            let one = match c.run(&A, &B) {
                Ok(v) => v,
                Err(_) => return s,
            };
            // 2 行顶格 + 2 行一格缩进 + 2 行两格缩进。
            one.bytes == 2 * 2 + 2 * 3 + 2 * 4
                && l.sample(1).map(|x| x.value) == Some(18)
                && l.sample(2).map(|x| x.value) == Some(10)
                && l.sample(3).map(|x| x.value) == Some(2)
                && l.sample(4).map(|x| x.value) == Some(4)
                && l.sample(5).map(|x| x.value) == Some(13)
        },
        "static",
    );

    s.add(
        "CM13-冻结重放四注入",
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
                && inj == [true; 4]
        },
        "static",
    );

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    fn c(o: Opts) -> Comm {
        match o.build() {
            Ok(v) => v,
            Err(_) => panic!("build"),
        }
    }

    #[test]
    fn ledger_shape() {
        let l = build_ledger();
        assert_eq!(l.len(), SAMPLES);
        assert_eq!(l.ok_count(), 8);
        assert_eq!(l.err_count(), 6);
        assert!(l.audit().is_ok());
        assert_eq!(l.rejects(), 0);
    }

    #[test]
    fn checks_pass() {
        let s = run_unx11518_checks();
        assert_eq!(s.len(), 13);
        assert!(s.all_passed());
    }

    #[test]
    fn default_columns() {
        let d = c(Opts::new());
        assert_eq!(
            d.run(&A, &B),
            Ok(Report { only1: 2, only2: 2, both: 2, bytes: 18 })
        );
        assert_eq!(d.leads(2), Ok(2));
    }

    #[test]
    fn suppression_collapses_indent() {
        let s = c(Opts::new().sup(true, false, false));
        assert_eq!(s.leads(1), Ok(0));
        assert_eq!(s.leads(2), Ok(1));
        assert_eq!(s.run(&A, &B).map(|r| r.bytes), Ok(10));
        let t = c(Opts::new().sup(false, true, true));
        assert_eq!(t.run(&A, &B).map(|r| r.bytes), Ok(4));
    }

    #[test]
    fn duplicates_pair_off() {
        let d = c(Opts::new());
        assert_eq!(
            d.run(&DA, &DB),
            Ok(Report { only1: 1, only2: 1, both: 2, bytes: 13 })
        );
    }

    #[test]
    fn order_gate() {
        let d = c(Opts::new());
        assert_eq!(d.run(&BAD, &B), Err(CM_EORDER));
        assert_eq!(d.run(&A, &["g", "c"]), Err(CM_EORDER));
        assert_eq!(d.run(&["a\tb"], &B), Err(CM_ETAB));
        assert_eq!(d.run(&["a\u{0}"], &B), Err(CM_ENUL));
    }

    #[test]
    fn capacity_gates() {
        let d = c(Opts::new());
        let nine: [&str; 9] = ["a", "b", "c", "d", "e", "f", "g", "h", "i"];
        assert_eq!(d.run(&nine, &B), Err(CM_EFULL));
        let long: [&str; 1] = ["yyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyy"];
        assert_eq!(d.run(&long, &B), Err(CM_ECAP));
        let tall: [&str; 8] = [L0, L1, L2, L3, L4, L5, L6, L7];
        assert_eq!(d.run(&tall, &tall), Err(CM_EBYTES));
    }

    #[test]
    fn inject_four_is_all_true() {
        assert_eq!(inject_four(), [true; 4]);
    }
}
