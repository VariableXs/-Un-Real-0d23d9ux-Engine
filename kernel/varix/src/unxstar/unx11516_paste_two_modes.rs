//! UNX-F11516 paste 并列/串行合并（-d/-s）
//!
//! `paste` 有两种**完全不同的合并语义**，它们的差异不是格式上的，而是行数与
//! 谁跟着谁动：
//!
//! - **并列**（缺省）：从每个输入文件各取一行，用分隔符串起来，输出一行。行数
//!   等于**最长**文件的行数；
//! - **串行**（`-s`）：把**一个**文件的全部行串成一行，每个文件产出一行；
//!   空文件产出一个空行（1 字节换行），而不是零行。
//!
//! 并列模式有一条最容易被漏掉的规则：**短文件补空，但分隔符照占**。三个文件、
//! 第二行只有第一个文件有数据时，输出是 `a2<TAB><TAB>`——两个制表符都在，
//! 只是中间那个字段为空。少了这两个分隔符，下游 `cut -f` 的列号就会整体错位。
//!
//! 分隔符来自 `-d` 的**字符列表**，逐字段循环取用：
//!
//! - `\t` / `\n` / `\\` / `\0`（`\0` 表示「不放分隔符」）四种转义；
//! - 其它反斜杠组合一律 `PS_EDESC`，不猜；
//! - 游标在**并列模式跨行连续**（GNU 的实际行为：两行两文件、`-d'ab'` 时第一行
//!   用 `a`、第二行用 `b`），在**串行模式每次调用复位**（POSIX：分隔符按文件
//!   重新起算）。
//!
//! 闸门次序：构造（转义合法 → 表非空 → 表不超 4 → 模式合法）→ 输入（至少一个
//! 文件 → 文件数不超 4 → 单文件行数不超 8）→ 字节（单行不超 64、总量不超 512）。
//! 先报 `ENOFILE` 再报行数：没有输入时行数无意义。
//!
//! 账本语义：十四格 = 八格成功（并列四种、串行两种、行数、三文件）+ 六格失败。
//! `audit` 认「八值六码」，`seal` 做 FNV-1a 折叠，`replay` 重建比对，`freeze`
//! 后登记一律拒绝。

use crate::alloc::format;
use crate::checks::CheckSet;

/// 段基址。
pub const SEG_BASE: u32 = 0xf150;

/// 没有任何输入文件。
pub const PS_ENOFILE: u32 = SEG_BASE + 0x01;
/// 文件数超过 4。
pub const PS_EFULL: u32 = SEG_BASE + 0x02;
/// 单文件行数超过 8。
pub const PS_EROW: u32 = SEG_BASE + 0x03;
/// 分隔符表为空。
pub const PS_EDELIM: u32 = SEG_BASE + 0x04;
/// 分隔符表超过 4。
pub const PS_EDCAP: u32 = SEG_BASE + 0x05;
/// 转义序列非法。
pub const PS_EDESC: u32 = SEG_BASE + 0x06;
/// 模式非法或与操作不符。
pub const PS_EMODE: u32 = SEG_BASE + 0x07;
/// 输出越过字节预算。
pub const PS_EBYTES: u32 = SEG_BASE + 0x08;
/// 账本形状不符。
pub const PS_EBAL: u32 = SEG_BASE + 0x09;
/// 槽位乱序。
pub const PS_ESLOT: u32 = SEG_BASE + 0x0a;
/// 观测值与复算不符。
pub const PS_EVALUE: u32 = SEG_BASE + 0x0b;
/// 冻结后重放不一致。
pub const PS_ESEAL: u32 = SEG_BASE + 0x0c;
/// 观察下标越界。
pub const PS_ENOBS: u32 = SEG_BASE + 0x0d;

/// 并列模式。
pub const MODE_PAR: u8 = 1;
/// 串行模式。
pub const MODE_SER: u8 = 2;

/// 分隔符表容量。
pub const DCAP: usize = 4;
/// 输入文件数上限。
pub const FILES: usize = 4;
/// 单文件行数上限。
pub const ROWS: usize = 8;
/// 单行字节上限（含换行）。
pub const LINE_MAX: u32 = 64;
/// 单次输出总字节上限。
pub const OUT_CAP: u32 = 512;
/// 账本格数。
pub const SAMPLES: usize = 14;

/// 操作码：构造。
pub const OP_BUILD: u8 = 1;
/// 操作码：并列。
pub const OP_PAR: u8 = 2;
/// 操作码：串行。
pub const OP_SER: u8 = 3;
/// 操作码：行数。
pub const OP_ROWS: u8 = 4;

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

/// 一个 paste 实例（含分隔符游标）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Paste {
    mode: u8,
    delims: [u8; DCAP],
    nd: usize,
    cur: usize,
}

impl Paste {
    /// 空白实例。
    pub const fn blank() -> Paste {
        Paste { mode: 0, delims: [0; DCAP], nd: 0, cur: 0 }
    }

    /// 从 `-d` 原文构造；`\t\n\\` 与 `\0` 走转义。
    pub fn new(mode: u8, spec: &[u8]) -> Result<Paste, u32> {
        if mode != MODE_PAR && mode != MODE_SER {
            return Err(PS_EMODE);
        }
        let mut p = Paste::blank();
        p.mode = mode;
        let mut i = 0usize;
        let mut n = 0usize;
        while i < spec.len() {
            let b = match spec.get(i) {
                Some(v) => *v,
                None => break,
            };
            let mut c = b;
            let mut adv = 1usize;
            if b == b'\\' {
                let e = match spec.get(i + 1) {
                    Some(v) => *v,
                    None => return Err(PS_EDESC),
                };
                c = match e {
                    b't' => b'\t',
                    b'n' => b'\n',
                    b'\\' => b'\\',
                    b'0' => 0,
                    _ => return Err(PS_EDESC),
                };
                adv = 2;
            }
            if n >= DCAP {
                return Err(PS_EDCAP);
            }
            if let Some(dst) = p.delims.get_mut(n) {
                *dst = c;
            }
            n += 1;
            i = i.saturating_add(adv);
        }
        if n == 0 {
            return Err(PS_EDELIM);
        }
        p.nd = n;
        Ok(p)
    }

    /// 模式。
    pub fn mode(&self) -> u8 {
        self.mode
    }

    /// 分隔符个数。
    pub fn delim_count(&self) -> usize {
        self.nd
    }

    /// 第 i 个分隔符字节。
    pub fn delim_at(&self, i: usize) -> Option<u8> {
        if i >= self.nd {
            return None;
        }
        self.delims.get(i).copied()
    }

    /// 游标位置。
    pub fn cursor(&self) -> usize {
        self.cur
    }

    /// 取下一个分隔符；0 表示「不放分隔符」。游标循环前进。
    pub fn next_delim(&mut self) -> u8 {
        if self.nd == 0 {
            return 0;
        }
        let d = match self.delims.get(self.cur) {
            Some(v) => *v,
            None => return 0,
        };
        self.cur = self.cur.saturating_add(1);
        if self.cur >= self.nd {
            self.cur = 0;
        }
        d
    }

    /// 并列模式的行数（= 最长文件的行数）。
    pub fn rows(files: &[&[&[u8]]]) -> Result<u32, u32> {
        if files.is_empty() {
            return Err(PS_ENOFILE);
        }
        if files.len() > FILES {
            return Err(PS_EFULL);
        }
        let mut max = 0usize;
        let mut f = 0usize;
        while f < files.len() {
            if let Some(file) = files.get(f) {
                if file.len() > ROWS {
                    return Err(PS_EROW);
                }
                if file.len() > max {
                    max = file.len();
                }
            }
            f += 1;
        }
        Ok(max as u32)
    }

    /// 并列合并：逐行取各文件同序号的一行，短文件补空但仍占分隔符。
    pub fn parallel(&mut self, files: &[&[&[u8]]]) -> Result<u32, u32> {
        if self.mode != MODE_PAR {
            return Err(PS_EMODE);
        }
        let nrows = Paste::rows(files)?;
        let nf = files.len();
        let mut total = 0u32;
        let mut r = 0usize;
        while r < nrows as usize {
            let mut line = 0u32;
            let mut f = 0usize;
            while f < nf {
                if f > 0 {
                    let d = self.next_delim();
                    if d != 0 {
                        line = line.saturating_add(1);
                    }
                }
                if let Some(file) = files.get(f) {
                    if let Some(l) = file.get(r) {
                        line = line.saturating_add(l.len() as u32);
                    }
                }
                f += 1;
            }
            let with_nl = line.saturating_add(1);
            if with_nl > LINE_MAX {
                return Err(PS_EBYTES);
            }
            total = total.saturating_add(with_nl);
            if total > OUT_CAP {
                return Err(PS_EBYTES);
            }
            r += 1;
        }
        Ok(total)
    }

    /// 串行合并：把一个文件的全部行串成一行，游标按文件复位。
    pub fn serial(&mut self, lines: &[&[u8]]) -> Result<u32, u32> {
        if self.mode != MODE_SER {
            return Err(PS_EMODE);
        }
        if lines.len() > ROWS {
            return Err(PS_EROW);
        }
        self.cur = 0;
        let mut b = 0u32;
        let mut i = 0usize;
        while i < lines.len() {
            if i > 0 {
                let d = self.next_delim();
                if d != 0 {
                    b = b.saturating_add(1);
                }
            }
            if let Some(l) = lines.get(i) {
                b = b.saturating_add(l.len() as u32);
            }
            i += 1;
        }
        b = b.saturating_add(1);
        if b > LINE_MAX {
            return Err(PS_EBYTES);
        }
        Ok(b)
    }

    /// 不变式自检。
    pub fn consistent(&self) -> bool {
        if self.mode != MODE_PAR && self.mode != MODE_SER {
            return false;
        }
        if self.nd == 0 || self.nd > DCAP || self.cur >= self.nd {
            return false;
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
            return Err(PS_ESLOT);
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
            return Err(PS_EBAL);
        }
        const VALS: [u32; 8] = [14, 3, 9, 1, 9, 14, 6, 17];
        const CODES: [u32; 6] =
            [PS_ENOFILE, PS_EFULL, PS_EDELIM, PS_EDCAP, PS_EDESC, PS_EMODE];
        let mut vi = 0usize;
        let mut ci = 0usize;
        let mut i = 0usize;
        while i < self.len {
            let s = match self.rows.get(i) {
                Some(v) => *v,
                None => return Err(PS_EBAL),
            };
            if s.slot as usize != i {
                return Err(PS_ESLOT);
            }
            if s.ok {
                if s.code != 0 {
                    return Err(PS_EVALUE);
                }
                match VALS.get(vi) {
                    Some(w) => {
                        if s.value != *w {
                            return Err(PS_EVALUE);
                        }
                    }
                    None => return Err(PS_EBAL),
                }
                vi += 1;
            } else {
                match CODES.get(ci) {
                    Some(w) => {
                        if s.code != *w {
                            return Err(PS_EVALUE);
                        }
                    }
                    None => return Err(PS_EBAL),
                }
                ci += 1;
            }
            i += 1;
        }
        if vi != 8 || ci != 6 {
            return Err(PS_EBAL);
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
            return Err(PS_EBAL);
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
            Err(PS_ESEAL)
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

/// 三行文件。
const A: [&[u8]; 3] = [b"a1", b"a2", b"a3"];
/// 一行文件。
const B: [&[u8]; 1] = [b"b1"];
/// 空文件。
const C: [&[u8]; 0] = [];
/// 三行短行（串行用）。
const XYZ: [&[u8]; 3] = [b"x", b"y", b"z"];

/// 起手。
fn mk(mode: u8, spec: &[u8]) -> Result<Paste, u32> {
    Paste::new(mode, spec)
}

/// 十四格账本：并列四档 → 串行两档 → 行数 → 三文件 → 六类拒绝。
pub fn build_ledger() -> Ledger {
    let mut l = Ledger::new();

    let two: [&[&[u8]]; 2] = [&A, &B];
    let _ = l.register(word_cell(
        0,
        OP_PAR,
        mk(MODE_PAR, b"\\t").and_then(|mut p| p.parallel(&two)),
    ));
    let _ = l.register(word_cell(1, OP_ROWS, Paste::rows(&two)));

    let _ = l.register(word_cell(
        2,
        OP_SER,
        mk(MODE_SER, b",").and_then(|mut p| p.serial(&A)),
    ));
    let _ = l.register(word_cell(
        3,
        OP_SER,
        mk(MODE_SER, b",").and_then(|mut p| p.serial(&C)),
    ));

    let ac: [&[&[u8]]; 2] = [&A, &C];
    let _ = l.register(word_cell(
        4,
        OP_PAR,
        mk(MODE_PAR, b"\\0").and_then(|mut p| p.parallel(&ac)),
    ));

    let _ = l.register(word_cell(
        5,
        OP_PAR,
        mk(MODE_PAR, b"ab").and_then(|mut p| p.parallel(&two)),
    ));

    let _ = l.register(word_cell(
        6,
        OP_SER,
        mk(MODE_SER, b"-=").and_then(|mut p| p.serial(&XYZ)),
    ));

    let three: [&[&[u8]]; 3] = [&A, &B, &C];
    let _ = l.register(word_cell(
        7,
        OP_PAR,
        mk(MODE_PAR, b"\\t").and_then(|mut p| p.parallel(&three)),
    ));

    let none: [&[&[u8]]; 0] = [];
    let _ = l.register(word_cell(
        8,
        OP_PAR,
        mk(MODE_PAR, b"\\t").and_then(|mut p| p.parallel(&none)),
    ));
    let five: [&[&[u8]]; 5] = [&A, &B, &C, &A, &B];
    let _ = l.register(word_cell(
        9,
        OP_ROWS,
        Paste::rows(&five),
    ));
    let _ = l.register(word_cell(10, OP_BUILD, mk(MODE_PAR, b"").map(|_| 0)));
    let _ = l.register(word_cell(11, OP_BUILD, mk(MODE_PAR, b"abcde").map(|_| 0)));
    let _ = l.register(word_cell(12, OP_BUILD, mk(MODE_PAR, b"\\q").map(|_| 0)));
    let _ = l.register(word_cell(13, OP_BUILD, mk(9, b",").map(|_| 0)));

    l
}

/// 四路注入。
pub fn inject_four() -> [bool; 4] {
    let mut out = [false; 4];

    let l = build_ledger();
    let rep = l.replay();
    let root = l.seal();
    out[0] = l.len() == SAMPLES && l.audit().is_ok() && rep == Ok(root) && root != 0;

    // 一路：并列补空仍占分隔符。
    let two: [&[&[u8]]; 2] = [&A, &B];
    let mut p = match mk(MODE_PAR, b"\\t") {
        Ok(v) => v,
        Err(_) => return out,
    };
    let total = p.parallel(&two);
    let ac: [&[&[u8]]; 2] = [&A, &C];
    let mut q = match mk(MODE_PAR, b"\\t") {
        Ok(v) => v,
        Err(_) => return out,
    };
    let padded = q.parallel(&ac);
    out[1] = total == Ok(14)
        && padded == Ok(12)
        && Paste::rows(&two) == Ok(3)
        && Paste::rows(&ac) == Ok(3)
        && p.consistent()
        && q.consistent();

    // 二路：串行与游标复位。
    let mut s1 = match mk(MODE_SER, b"-=") {
        Ok(v) => v,
        Err(_) => return out,
    };
    let first = s1.serial(&[b"x", b"y"]);
    let after_first = s1.cursor();
    let second = s1.serial(&[b"d", b"e"]);
    let after_second = s1.cursor();
    let empty = match mk(MODE_SER, b",") {
        Ok(mut v) => v.serial(&C),
        Err(_) => return out,
    };
    let triple = match mk(MODE_SER, b"-=") {
        Ok(mut v) => v.serial(&XYZ),
        Err(_) => return out,
    };
    out[2] = first == Ok(4)
        && after_first == 1
        && second == Ok(4)
        && after_second == 1
        && empty == Ok(1)
        && triple == Ok(6)
        && s1.consistent();

    // 三路：并列游标跨行连续（三分隔符两行 → 停在 2，不是 0）。
    let mut cyc = match mk(MODE_PAR, b"ab") {
        Ok(v) => v,
        Err(_) => return out,
    };
    let two_lines: [&[&[u8]]; 2] = [&A, &B];
    let cyc_total = cyc.parallel(&two_lines);
    let mut fresh = match mk(MODE_PAR, b"ab") {
        Ok(v) => v,
        Err(_) => return out,
    };
    let one_row: [&[&[u8]]; 2] = [&B, &B];
    let single = fresh.parallel(&one_row);
    out[3] = cyc_total == Ok(14)
        && cyc.cursor() == 1
        && single == Ok(6)
        && fresh.cursor() == 1
        && cyc.delim_at(1) == Some(b'b');

    out
}

/// 十三项判据。
pub fn run_unx11516_checks() -> CheckSet {
    let mut s = CheckSet::new("unx11516.paste_two_modes");
    let l = build_ledger();

    s.add_dyn(
        "PS1-十四格八允六拒",
        l.len() == SAMPLES && l.ok_count() == 8 && l.err_count() == 6 && l.audit().is_ok(),
        &format!("n={} ok={} err={}", l.len(), l.ok_count(), l.err_count()),
    );

    s.add(
        "PS2-并列行数取最长",
        {
            let two: [&[&[u8]]; 2] = [&A, &B];
            let ac: [&[&[u8]]; 2] = [&A, &C];
            let empty: [&[&[u8]]; 1] = [&C];
            Paste::rows(&two) == Ok(3)
                && Paste::rows(&ac) == Ok(3)
                && Paste::rows(&empty) == Ok(0)
                && l.sample(1).map(|x| x.value) == Some(3)
        },
        "static",
    );

    s.add(
        "PS3-短文件补空占位",
        {
            let two: [&[&[u8]]; 2] = [&A, &B];
            let ac: [&[&[u8]]; 2] = [&A, &C];
            let mut p = match mk(MODE_PAR, b"\\t") {
                Ok(v) => v,
                Err(_) => return s,
            };
            let mut q = match mk(MODE_PAR, b"\\t") {
                Ok(v) => v,
                Err(_) => return s,
            };
            p.parallel(&two) == Ok(14)
                && q.parallel(&ac) == Ok(12)
                && l.sample(0).map(|x| x.value) == Some(14)
                && l.sample(7).map(|x| x.value) == Some(17)
        },
        "static",
    );

    s.add(
        "PS4-串行一文件一行",
        {
            let mut p = match mk(MODE_SER, b",") {
                Ok(v) => v,
                Err(_) => return s,
            };
            let full = p.serial(&XYZ);
            let empty = p.serial(&C);
            full == Ok(6)
                && empty == Ok(1)
                && l.sample(2).map(|x| x.value) == Some(9)
                && l.sample(3).map(|x| x.value) == Some(1)
        },
        "static",
    );

    s.add(
        "PS5-分隔符循环跨行",
        {
            let two: [&[&[u8]]; 2] = [&A, &B];
            let mut p = match mk(MODE_PAR, b"ab") {
                Ok(v) => v,
                Err(_) => return s,
            };
            let bytes = p.parallel(&two);
            let mut z = match mk(MODE_PAR, b"\\0") {
                Ok(v) => v,
                Err(_) => return s,
            };
            bytes == Ok(14)
                && z.parallel(&two) == Ok(11)
                && p.delim_count() == 2
                && p.delim_at(0) == Some(b'a')
                && p.delim_at(2) == None
                && l.sample(5).map(|x| x.value) == Some(14)
        },
        "static",
    );

    s.add(
        "PS6-串行游标复位",
        {
            let mut p = match mk(MODE_SER, b"-=") {
                Ok(v) => v,
                Err(_) => return s,
            };
            let a = p.serial(&[b"x", b"y"]);
            let c1 = p.cursor();
            let b = p.serial(&[b"d", b"e"]);
            let c2 = p.cursor();
            let mut q = match mk(MODE_SER, b"-=") {
                Ok(v) => v,
                Err(_) => return s,
            };
            a == Ok(4)
                && c1 == 1
                && b == Ok(4)
                && c2 == 1
                && q.serial(&XYZ) == Ok(6)
                && l.sample(6).map(|x| x.value) == Some(6)
        },
        "static",
    );

    s.add(
        "PS7-转义四式",
        {
            let tab = match mk(MODE_PAR, b"\\t") {
                Ok(v) => v,
                Err(_) => return s,
            };
            let nl = match mk(MODE_PAR, b"\\n") {
                Ok(v) => v,
                Err(_) => return s,
            };
            let none = match mk(MODE_PAR, b"\\0") {
                Ok(v) => v,
                Err(_) => return s,
            };
            let bs = match mk(MODE_PAR, b"\\\\") {
                Ok(v) => v,
                Err(_) => return s,
            };
            let mixed = match mk(MODE_PAR, b"a\\tb") {
                Ok(v) => v,
                Err(_) => return s,
            };
            tab.delim_at(0) == Some(b'\t')
                && nl.delim_at(0) == Some(b'\n')
                && none.delim_at(0) == Some(0)
                && bs.delim_at(0) == Some(b'\\')
                && mixed.delim_count() == 3
                && mixed.delim_at(1) == Some(b'\t')
                && l.sample(12).map(|x| x.code) == Some(PS_EDESC)
        },
        "static",
    );

    s.add(
        "PS8-分隔符表门",
        mk(MODE_PAR, b"") == Err(PS_EDELIM)
            && mk(MODE_PAR, b"\\") == Err(PS_EDESC)
            && mk(MODE_PAR, b"abcd") == Ok(match mk(MODE_PAR, b"abcd") { Ok(v) => v, Err(_) => return s })
            && mk(MODE_PAR, b"abcde") == Err(PS_EDCAP)
            && mk(MODE_PAR, b"abcd\\t") == Err(PS_EDCAP)
            && mk(MODE_PAR, b"abc\\t").is_ok()
            && l.sample(10).map(|x| x.code) == Some(PS_EDELIM)
            && l.sample(11).map(|x| x.code) == Some(PS_EDCAP),
        "static",
    );

    s.add(
        "PS9-模式门与错配",
        mk(0, b",") == Err(PS_EMODE)
            && mk(3, b",") == Err(PS_EMODE)
            && {
                let two: [&[&[u8]]; 2] = [&A, &B];
                match mk(MODE_SER, b",") {
                    Ok(mut p) => p.parallel(&two) == Err(PS_EMODE),
                    Err(_) => return s,
                }
            }
            && {
                let mut p = match mk(MODE_PAR, b",") {
                    Ok(v) => v,
                    Err(_) => return s,
                };
                p.serial(&XYZ) == Err(PS_EMODE)
            }
            && l.sample(13).map(|x| x.code) == Some(PS_EMODE),
        "static",
    );

    s.add(
        "PS10-文件数两门",
        {
            let none: [&[&[u8]]; 0] = [];
            let five: [&[&[u8]]; 5] = [&A, &B, &C, &A, &B];
            let four: [&[&[u8]]; 4] = [&A, &B, &C, &B];
            Paste::rows(&none) == Err(PS_ENOFILE)
                && Paste::rows(&five) == Err(PS_EFULL)
                && Paste::rows(&four) == Ok(3)
                && l.sample(8).map(|x| x.code) == Some(PS_ENOFILE)
                && l.sample(9).map(|x| x.code) == Some(PS_EFULL)
        },
        "static",
    );

    s.add(
        "PS11-行数门八行",
        {
            let nine: [&[u8]; 9] =
                [b"1", b"2", b"3", b"4", b"5", b"6", b"7", b"8", b"9"];
            let wide: [&[&[u8]]; 1] = [&nine];
            let eight: [&[u8]; 8] = [b"1", b"2", b"3", b"4", b"5", b"6", b"7", b"8"];
            let ok: [&[&[u8]]; 1] = [&eight];
            Paste::rows(&wide) == Err(PS_EROW)
                && Paste::rows(&ok) == Ok(8)
                && {
                    let mut p = match mk(MODE_SER, b",") {
                        Ok(v) => v,
                        Err(_) => return s,
                    };
                    p.serial(&nine) == Err(PS_EROW)
                }
        },
        "static",
    );

    s.add(
        "PS12-字节预算两门",
        {
            let long: [&[u8]; 1] = [b"0123456789012345678901234567890123456789012345678901234567890123"];
            let one: [&[&[u8]]; 1] = [&long];
            let mut p = match mk(MODE_PAR, b",") {
                Ok(v) => v,
                Err(_) => return s,
            };
            let over = p.parallel(&one) == Err(PS_EBYTES);
            let mut q = match mk(MODE_SER, b",") {
                Ok(v) => v,
                Err(_) => return s,
            };
            let ser = q.serial(&[b"012345678901234567890123456789012345678901234567890123456789012"]) == Ok(64);
            over && ser
        },
        "static",
    );

    s.add(
        "PS13-冻结重放四注入",
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

    fn par(spec: &[u8]) -> Paste {
        match Paste::new(MODE_PAR, spec) {
            Ok(v) => v,
            Err(_) => panic!("build"),
        }
    }

    fn ser(spec: &[u8]) -> Paste {
        match Paste::new(MODE_SER, spec) {
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
        let s = run_unx11516_checks();
        assert_eq!(s.len(), 13);
        assert!(s.all_passed());
    }

    #[test]
    fn parallel_pads_but_keeps_delim() {
        let two: [&[&[u8]]; 2] = [&A, &B];
        let mut p = par(b"\\t");
        assert_eq!(Paste::rows(&two), Ok(3));
        assert_eq!(p.parallel(&two), Ok(14));
    }

    #[test]
    fn serial_one_line_per_file() {
        let mut p = ser(b",");
        assert_eq!(p.serial(&A), Ok(9));
        assert_eq!(p.serial(&C), Ok(1));
        assert_eq!(p.serial(&[]), Ok(1));
    }

    #[test]
    fn delimiters_cycle() {
        let two: [&[&[u8]]; 2] = [&A, &B];
        let mut p = par(b"ab");
        assert_eq!(p.parallel(&two), Ok(14));
        let mut q = par(b"\\0");
        // `-d '\0'` 是「空分隔」，列间不落字节：5 + 3 + 3 = 11。
        assert_eq!(q.parallel(&two), Ok(11));
    }

    #[test]
    fn escapes_and_gates() {
        let p = par(b"a\\tb");
        assert_eq!(p.delim_count(), 3);
        assert_eq!(p.delim_at(1), Some(b'\t'));
        assert_eq!(Paste::new(MODE_PAR, b""), Err(PS_EDELIM));
        assert_eq!(Paste::new(MODE_PAR, b"\\q"), Err(PS_EDESC));
        assert_eq!(Paste::new(MODE_PAR, b"abcde"), Err(PS_EDCAP));
        assert_eq!(Paste::new(7, b","), Err(PS_EMODE));
    }

    #[test]
    fn input_gates() {
        let none: [&[&[u8]]; 0] = [];
        let five: [&[&[u8]]; 5] = [&A, &B, &C, &A, &B];
        assert_eq!(Paste::rows(&none), Err(PS_ENOFILE));
        assert_eq!(Paste::rows(&five), Err(PS_EFULL));
        let mut p = ser(b",");
        let nine: [&[u8]; 9] = [b"1", b"2", b"3", b"4", b"5", b"6", b"7", b"8", b"9"];
        assert_eq!(p.serial(&nine), Err(PS_EROW));
    }

    #[test]
    fn inject_four_is_all_true() {
        assert_eq!(inject_four(), [true; 4]);
    }
}
