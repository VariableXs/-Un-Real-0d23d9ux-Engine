//! UNX-F11416 cat/mv/chmod 错误文案真值（高频三档）
//!
//! 三件工具最常触发的错误只有三档：**用法**（`usage`）、**目标缺失**
//! （`No such file`）、**权限拒绝**（`Permission denied`）。文案真值账把
//! 「工具 × 档位 → 文案编号 / 占位参数个数 / 退出码」钉成一张表：
//!
//! - 文案编号的高四位是工具号，低四位是档位号，`0x12` 就是 cat 的缺失档；
//! - 用法档不带路径占位（0 个参数），缺失与拒绝档各带 1 个路径占位；
//! - 退出码按档位而非按工具：用法 2，缺失 1，拒绝 1，同档三工具必须同码。
//!
//! 查表时先过工具与档位，再找行；找不到是 `EMISS`，找到但调用方给的占位
//! 个数不符是 `EARGC`——两者不能混为一谈：前者是表缺行，后者是调用错。
//!
//! 闸门次序：
//! - `insert`：工具 → 档位 → 文案编号 → 占位数 → 表满 → 重复；
//! - `lookup`：工具 → 档位 → 查行 → 占位数；
//! - `verify`：行数齐九 → 每格文案编号 = 工具档位合成 → 占位数与档位相符。
//!
//! 账本契约：十四格，八成功六失败；成功格记录插入计数、查表文案与档位退出
//! 码，失败格覆盖工具/档位/文案/占位/重复/缺行六类；表满与版本闸门在判据
//! 与注入里单独验证。

use crate::alloc::format;
use crate::checks::CheckSet;

/// 段基址。
pub const SEG_BASE: u32 = 0xf000;

/// 未知工具号。
pub const TT_ETOOL: u32 = SEG_BASE + 0x01;
/// 未知档位号。
pub const TT_ECLASS: u32 = SEG_BASE + 0x02;
/// 文案编号为零或越界。
pub const TT_ETEXT: u32 = SEG_BASE + 0x03;
/// 占位参数个数不符。
pub const TT_EARGC: u32 = SEG_BASE + 0x04;
/// 同工具同档位重复入表。
pub const TT_EDUP: u32 = SEG_BASE + 0x05;
/// 文案表满。
pub const TT_EFULL: u32 = SEG_BASE + 0x06;
/// 查表无此行。
pub const TT_EMISS: u32 = SEG_BASE + 0x07;
/// 版本为零或与本账不符。
pub const TT_EVER: u32 = SEG_BASE + 0x08;
/// 账本形状不符。
pub const TT_EBAL: u32 = SEG_BASE + 0x09;
/// 槽位乱序。
pub const TT_ESLOT: u32 = SEG_BASE + 0x0a;
/// 观测值与复算不符。
pub const TT_EVALUE: u32 = SEG_BASE + 0x0b;
/// 冻结后重放不一致。
pub const TT_ESEAL: u32 = SEG_BASE + 0x0c;
/// 观察下标越界。
pub const TT_ENOBS: u32 = SEG_BASE + 0x0d;

/// 文案表容量（恰为三工具 × 三档）。
pub const CAP: usize = 9;
/// 账本格数。
pub const SAMPLES: usize = 14;
/// 占位参数上限。
pub const ARG_MAX: u8 = 2;
/// 文案编号上界（不含）。
pub const TEXT_MAX: u16 = 0xff;

/// 工具：cat。
pub const TOOL_CAT: u8 = 1;
/// 工具：mv。
pub const TOOL_MV: u8 = 2;
/// 工具：chmod。
pub const TOOL_CHMOD: u8 = 3;

/// 档位：用法。
pub const CLASS_USAGE: u8 = 1;
/// 档位：目标缺失。
pub const CLASS_NOENT: u8 = 2;
/// 档位：权限拒绝。
pub const CLASS_DENIED: u8 = 3;

/// 操作码：入表。
pub const OP_INS: u8 = 1;
/// 操作码：查表。
pub const OP_LOOK: u8 = 2;
/// 操作码：查档位退出码。
pub const OP_EXIT: u8 = 3;
/// 操作码：查占位需求。
pub const OP_NEEDS: u8 = 4;

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

/// 工具号是否合法。
fn tool_ok(tool: u8) -> bool {
    tool == TOOL_CAT || tool == TOOL_MV || tool == TOOL_CHMOD
}

/// 档位号是否合法。
fn class_ok(class: u8) -> bool {
    class == CLASS_USAGE || class == CLASS_NOENT || class == CLASS_DENIED
}

/// 档位 → 退出码。
pub fn exit_of(class: u8) -> Result<u8, u32> {
    match class {
        CLASS_USAGE => Ok(2),
        CLASS_NOENT => Ok(1),
        CLASS_DENIED => Ok(1),
        _ => Err(TT_ECLASS),
    }
}

/// 档位 → 占位参数个数。
pub fn needs_of(class: u8) -> Result<u8, u32> {
    match class {
        CLASS_USAGE => Ok(0),
        CLASS_NOENT => Ok(1),
        CLASS_DENIED => Ok(1),
        _ => Err(TT_ECLASS),
    }
}

/// 工具与档位合成的真值文案编号。
pub fn text_of(tool: u8, class: u8) -> Result<u16, u32> {
    if !tool_ok(tool) {
        return Err(TT_ETOOL);
    }
    if !class_ok(class) {
        return Err(TT_ECLASS);
    }
    Ok(((tool as u16) << 4) | (class as u16))
}

/// 一行文案真值。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Row {
    /// 工具。
    pub tool: u8,
    /// 档位。
    pub class: u8,
    /// 文案编号。
    pub text: u16,
    /// 占位参数个数。
    pub argc: u8,
}

/// 文案真值表。
#[derive(Clone, Copy, Debug)]
pub struct Catalog {
    ver: u32,
    rows: [Row; CAP],
    n: usize,
}

impl Catalog {
    /// 为快照版本开户；版本为零归 `EVER`。
    pub fn open(ver: u32) -> Result<Catalog, u32> {
        if ver == 0 {
            return Err(TT_EVER);
        }
        Ok(Catalog { ver, rows: [Row { tool: 0, class: 0, text: 0, argc: 0 }; CAP], n: 0 })
    }

    /// 版本。
    pub fn ver(&self) -> u32 {
        self.ver
    }

    /// 行数。
    pub fn len(&self) -> usize {
        self.n
    }

    /// 是否空表。
    pub fn is_empty(&self) -> bool {
        self.n == 0
    }

    /// 第 i 行。
    pub fn row_at(&self, i: usize) -> Option<Row> {
        if i >= self.n {
            return None;
        }
        self.rows.get(i).copied()
    }

    /// 找工具与档位对应的行下标。
    fn find(&self, tool: u8, class: u8) -> Option<usize> {
        let mut i = 0usize;
        while i < self.n {
            if let Some(r) = self.rows.get(i) {
                if r.tool == tool && r.class == class {
                    return Some(i);
                }
            }
            i = i.saturating_add(1);
        }
        None
    }

    /// 入表。
    ///
    /// 闸门次序：工具 → 档位 → 文案编号 → 占位数 → 表满 → 重复。
    pub fn insert(&mut self, tool: u8, class: u8, text: u16, argc: u8) -> Result<usize, u32> {
        if !tool_ok(tool) {
            return Err(TT_ETOOL);
        }
        if !class_ok(class) {
            return Err(TT_ECLASS);
        }
        if text == 0 || text >= TEXT_MAX {
            return Err(TT_ETEXT);
        }
        if argc > ARG_MAX {
            return Err(TT_EARGC);
        }
        if self.n >= CAP {
            return Err(TT_EFULL);
        }
        if self.find(tool, class).is_some() {
            return Err(TT_EDUP);
        }
        if let Some(dst) = self.rows.get_mut(self.n) {
            *dst = Row { tool, class, text, argc };
        }
        self.n = self.n.saturating_add(1);
        Ok(self.n)
    }

    /// 查表。
    ///
    /// 闸门次序：工具 → 档位 → 查行 → 占位数。
    pub fn lookup(&self, tool: u8, class: u8, argc: u8) -> Result<Row, u32> {
        if !tool_ok(tool) {
            return Err(TT_ETOOL);
        }
        if !class_ok(class) {
            return Err(TT_ECLASS);
        }
        let i = match self.find(tool, class) {
            Some(v) => v,
            None => return Err(TT_EMISS),
        };
        let row = match self.rows.get(i) {
            Some(v) => *v,
            None => return Err(TT_EMISS),
        };
        if argc != row.argc {
            return Err(TT_EARGC);
        }
        Ok(row)
    }

    /// 齐套校验：九行、文案编号与工具档位合成一致、占位数与档位相符。
    pub fn verify(&self) -> Result<u32, u32> {
        if self.n != CAP {
            return Err(TT_EMISS);
        }
        let mut tool = 1u8;
        while tool <= 3 {
            let mut class = 1u8;
            while class <= 3 {
                let i = match self.find(tool, class) {
                    Some(v) => v,
                    None => return Err(TT_EMISS),
                };
                let row = match self.rows.get(i) {
                    Some(v) => *v,
                    None => return Err(TT_EMISS),
                };
                let want = match text_of(tool, class) {
                    Ok(v) => v,
                    Err(c) => return Err(c),
                };
                if row.text != want {
                    return Err(TT_ETEXT);
                }
                let need = match needs_of(class) {
                    Ok(v) => v,
                    Err(c) => return Err(c),
                };
                if row.argc != need {
                    return Err(TT_EARGC);
                }
                class = class.saturating_add(1);
            }
            tool = tool.saturating_add(1);
        }
        Ok(CAP as u32)
    }

    /// 按真值表构造全表。
    pub fn canonical(ver: u32) -> Result<Catalog, u32> {
        let mut c = Catalog::open(ver)?;
        let mut tool = 1u8;
        while tool <= 3 {
            let mut class = 1u8;
            while class <= 3 {
                let text = match text_of(tool, class) {
                    Ok(v) => v,
                    Err(c) => return Err(c),
                };
                let argc = match needs_of(class) {
                    Ok(v) => v,
                    Err(c) => return Err(c),
                };
                if let Err(e) = c.insert(tool, class, text, argc) {
                    return Err(e);
                }
                class = class.saturating_add(1);
            }
            tool = tool.saturating_add(1);
        }
        Ok(c)
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
            return Err(TT_ESLOT);
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
            return Err(TT_EBAL);
        }
        const VALS: [u32; 8] = [1, 2, 3, 4, 5, 6, 18, 1];
        const CODES: [u32; 6] =
            [TT_ETOOL, TT_ECLASS, TT_ETEXT, TT_EARGC, TT_EDUP, TT_EMISS];
        let mut vi = 0usize;
        let mut ci = 0usize;
        let mut i = 0usize;
        while i < self.len {
            let v = match self.rows.get(i) {
                Some(x) => *x,
                None => return Err(TT_EBAL),
            };
            if v.slot as usize != i {
                return Err(TT_ESLOT);
            }
            if v.ok {
                if v.code != 0 {
                    return Err(TT_EVALUE);
                }
                match VALS.get(vi) {
                    Some(w) => {
                        if v.value != *w {
                            return Err(TT_EVALUE);
                        }
                    }
                    None => return Err(TT_EBAL),
                }
                vi = vi.saturating_add(1);
            } else {
                match CODES.get(ci) {
                    Some(w) => {
                        if v.code != *w {
                            return Err(TT_EVALUE);
                        }
                    }
                    None => return Err(TT_EBAL),
                }
                ci = ci.saturating_add(1);
            }
            i = i.saturating_add(1);
        }
        if vi != 8 || ci != 6 {
            return Err(TT_EBAL);
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
            return Err(TT_EBAL);
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
            Err(TT_ESEAL)
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

/// 十四格账本：六行入表 → 一次查表 → 档位码 → 六类拒绝。
pub fn build_ledger() -> Ledger {
    let mut l = Ledger::new();
    let mut c = match Catalog::open(3) {
        Ok(v) => v,
        Err(_) => return l,
    };

    let _ = l.register(cell(0, OP_INS, c.insert(TOOL_CAT, CLASS_USAGE, 0x11, 0)));
    let _ = l.register(cell(1, OP_INS, c.insert(TOOL_CAT, CLASS_NOENT, 0x12, 1)));
    let _ = l.register(cell(2, OP_INS, c.insert(TOOL_CAT, CLASS_DENIED, 0x13, 1)));
    let _ = l.register(cell(3, OP_INS, c.insert(TOOL_MV, CLASS_USAGE, 0x21, 0)));
    let _ = l.register(cell(4, OP_INS, c.insert(TOOL_MV, CLASS_NOENT, 0x22, 1)));
    let _ = l.register(cell(5, OP_INS, c.insert(TOOL_CHMOD, CLASS_USAGE, 0x31, 0)));
    let _ = l.register(word_cell(6, OP_LOOK, c.lookup(TOOL_CAT, CLASS_NOENT, 1).map(|r| r.text as u32)));
    let _ = l.register(byte_cell(7, OP_EXIT, exit_of(CLASS_NOENT)));

    let _ = l.register(cell(8, OP_INS, c.insert(9, CLASS_USAGE, 0x11, 0)));
    let _ = l.register(cell(9, OP_INS, c.insert(TOOL_CAT, 9, 0x11, 0)));
    let _ = l.register(cell(10, OP_INS, c.insert(TOOL_MV, CLASS_DENIED, 0, 1)));
    let _ = l.register(cell(11, OP_INS, c.insert(TOOL_MV, CLASS_DENIED, 0x23, 3)));
    let _ = l.register(cell(12, OP_INS, c.insert(TOOL_CAT, CLASS_USAGE, 0x11, 0)));
    let _ = l.register(cell(13, OP_LOOK, c.lookup(TOOL_CHMOD, CLASS_DENIED, 1).map(|_| 0usize)));

    l
}

/// 四路注入。
pub fn inject_four() -> [bool; 4] {
    let mut out = [false; 4];

    let l = build_ledger();
    let rep = l.replay();
    let root = l.seal();
    out[0] = l.len() == SAMPLES && l.audit().is_ok() && rep == Ok(root) && root != 0;

    // 一路：真值全表齐九行，文案编号与档位码一致。
    let full = match Catalog::canonical(5) {
        Ok(v) => v,
        Err(_) => return out,
    };
    let mut grid = full.verify() == Ok(CAP as u32);
    let mut tool = 1u8;
    let mut texts = 0u32;
    while tool <= 3 {
        let mut class = 1u8;
        while class <= 3 {
            let text = match text_of(tool, class) {
                Ok(v) => v,
                Err(_) => return out,
            };
            let argc = match needs_of(class) {
                Ok(v) => v,
                Err(_) => return out,
            };
            let row = match full.lookup(tool, class, argc) {
                Ok(v) => v,
                Err(_) => return out,
            };
            let code = match exit_of(class) {
                Ok(v) => v,
                Err(_) => return out,
            };
            if row.text != text || row.argc != argc || code != (if class == CLASS_USAGE { 2 } else { 1 }) {
                grid = false;
            }
            texts = texts.saturating_add(row.text as u32);
            class = class.saturating_add(1);
        }
        tool = tool.saturating_add(1);
    }
    out[1] = grid && full.len() == CAP && texts == 0x11 + 0x12 + 0x13 + 0x21 + 0x22 + 0x23 + 0x31 + 0x32 + 0x33;

    // 二路：八类失败各自可辨。
    let ver_zero = matches!(Catalog::open(0), Err(TT_EVER));
    let mut c = match Catalog::open(2) {
        Ok(v) => v,
        Err(_) => return out,
    };
    let tool = c.insert(9, CLASS_USAGE, 0x11, 0) == Err(TT_ETOOL);
    let class = c.insert(TOOL_CAT, 9, 0x11, 0) == Err(TT_ECLASS);
    let text_zero = c.insert(TOOL_CAT, CLASS_USAGE, 0, 0) == Err(TT_ETEXT);
    let text_high = c.insert(TOOL_CAT, CLASS_USAGE, TEXT_MAX, 0) == Err(TT_ETEXT);
    let argc = c.insert(TOOL_CAT, CLASS_USAGE, 0x11, 3) == Err(TT_EARGC);
    let ok1 = c.insert(TOOL_CAT, CLASS_USAGE, 0x11, 0) == Ok(1);
    let dup = c.insert(TOOL_CAT, CLASS_USAGE, 0x11, 0) == Err(TT_EDUP);
    let miss = c.lookup(TOOL_MV, CLASS_NOENT, 1) == Err(TT_EMISS);
    let bad_arg = c.lookup(TOOL_CAT, CLASS_USAGE, 1) == Err(TT_EARGC);
    let mut f = match Catalog::canonical(4) {
        Ok(v) => v,
        Err(_) => return out,
    };
    let full_err = f.insert(TOOL_CAT, CLASS_USAGE, 0x11, 0) == Err(TT_EFULL);
    out[2] = ver_zero
        && tool
        && class
        && text_zero
        && text_high
        && argc
        && ok1
        && dup
        && miss
        && bad_arg
        && full_err;

    // 三路：同档同码、跨工具文案不重、占位需求分档。
    let mut q = match Catalog::canonical(8) {
        Ok(v) => v,
        Err(_) => return out,
    };
    let same_usage = exit_of(CLASS_USAGE) == Ok(2)
        && exit_of(CLASS_NOENT) == Ok(1)
        && exit_of(CLASS_DENIED) == Ok(1);
    let needs = needs_of(CLASS_USAGE) == Ok(0)
        && needs_of(CLASS_NOENT) == Ok(1)
        && needs_of(CLASS_DENIED) == Ok(1);
    let a = q.lookup(TOOL_CAT, CLASS_DENIED, 1);
    let b = q.lookup(TOOL_MV, CLASS_DENIED, 1);
    let c2 = q.lookup(TOOL_CHMOD, CLASS_DENIED, 1);
    let distinct = match (a, b, c2) {
        (Ok(x), Ok(y), Ok(z)) => x.text != y.text && y.text != z.text && x.text != z.text,
        _ => false,
    };
    out[3] = same_usage
        && needs
        && distinct
        && q.verify() == Ok(9)
        && q.insert(TOOL_CAT, CLASS_USAGE, 0x11, 0) == Err(TT_EFULL)
        && text_of(0, CLASS_USAGE) == Err(TT_ETOOL)
        && text_of(TOOL_CAT, 0) == Err(TT_ECLASS);

    out
}

/// 十三项判据。
pub fn run_unx11416_checks() -> CheckSet {
    let mut s = CheckSet::new("unx11416.cat_mv_chmod_text_truth");
    let l = build_ledger();

    s.add_dyn(
        "TT1-十四格八允六拒",
        l.len() == SAMPLES && l.ok_count() == 8 && l.err_count() == 6 && l.audit().is_ok(),
        &format!("n={} ok={} err={}", l.len(), l.ok_count(), l.err_count()),
    );

    s.add(
        "TT2-真值表齐九行",
        {
            let c = match Catalog::canonical(1) {
                Ok(v) => v,
                Err(_) => return s,
            };
            c.len() == CAP && c.verify() == Ok(9) && c.ver() == 1
        },
        "static",
    );

    s.add(
        "TT3-用法档文案",
        {
            let c = match Catalog::canonical(1) {
                Ok(v) => v,
                Err(_) => return s,
            };
            let cat = c.lookup(TOOL_CAT, CLASS_USAGE, 0);
            let mv = c.lookup(TOOL_MV, CLASS_USAGE, 0);
            let chmod = c.lookup(TOOL_CHMOD, CLASS_USAGE, 0);
            cat == Ok(Row { tool: TOOL_CAT, class: CLASS_USAGE, text: 0x11, argc: 0 })
                && mv == Ok(Row { tool: TOOL_MV, class: CLASS_USAGE, text: 0x21, argc: 0 })
                && chmod == Ok(Row { tool: TOOL_CHMOD, class: CLASS_USAGE, text: 0x31, argc: 0 })
                && exit_of(CLASS_USAGE) == Ok(2)
                && l.sample(5).map(|x| x.value) == Some(6)
        },
        "static",
    );

    s.add(
        "TT4-缺失档文案",
        {
            let c = match Catalog::canonical(1) {
                Ok(v) => v,
                Err(_) => return s,
            };
            c.lookup(TOOL_CAT, CLASS_NOENT, 1)
                == Ok(Row { tool: TOOL_CAT, class: CLASS_NOENT, text: 0x12, argc: 1 })
                && c.lookup(TOOL_MV, CLASS_NOENT, 1)
                    == Ok(Row { tool: TOOL_MV, class: CLASS_NOENT, text: 0x22, argc: 1 })
                && c.lookup(TOOL_CHMOD, CLASS_NOENT, 1)
                    == Ok(Row { tool: TOOL_CHMOD, class: CLASS_NOENT, text: 0x32, argc: 1 })
                && exit_of(CLASS_NOENT) == Ok(1)
                && l.sample(6).map(|x| x.value) == Some(18)
        },
        "static",
    );

    s.add(
        "TT5-拒绝档文案",
        {
            let c = match Catalog::canonical(1) {
                Ok(v) => v,
                Err(_) => return s,
            };
            c.lookup(TOOL_CAT, CLASS_DENIED, 1)
                == Ok(Row { tool: TOOL_CAT, class: CLASS_DENIED, text: 0x13, argc: 1 })
                && c.lookup(TOOL_MV, CLASS_DENIED, 1)
                    == Ok(Row { tool: TOOL_MV, class: CLASS_DENIED, text: 0x23, argc: 1 })
                && c.lookup(TOOL_CHMOD, CLASS_DENIED, 1)
                    == Ok(Row { tool: TOOL_CHMOD, class: CLASS_DENIED, text: 0x33, argc: 1 })
                && exit_of(CLASS_DENIED) == Ok(1)
        },
        "static",
    );

    s.add(
        "TT6-文案编号工具前缀",
        {
            let c = match Catalog::canonical(1) {
                Ok(v) => v,
                Err(_) => return s,
            };
            let mut ok = true;
            let mut i = 0usize;
            while i < c.len() {
                match c.row_at(i) {
                    Some(r) => {
                        if (r.text >> 4) as u8 != r.tool || (r.text & 0x0f) as u8 != r.class {
                            ok = false;
                        }
                    }
                    None => ok = false,
                }
                i = i.saturating_add(1);
            }
            ok && text_of(TOOL_MV, CLASS_DENIED) == Ok(0x23)
                && text_of(TOOL_CHMOD, CLASS_USAGE) == Ok(0x31)
                && text_of(0, 1) == Err(TT_ETOOL)
                && text_of(TOOL_CAT, 0) == Err(TT_ECLASS)
        },
        "static",
    );

    s.add(
        "TT7-查表与占位闸门",
        {
            let c = match Catalog::canonical(1) {
                Ok(v) => v,
                Err(_) => return s,
            };
            c.lookup(TOOL_CAT, CLASS_USAGE, 1) == Err(TT_EARGC)
                && c.lookup(TOOL_MV, CLASS_NOENT, 0) == Err(TT_EARGC)
                && c.lookup(TOOL_CHMOD, 9, 0) == Err(TT_ECLASS)
                && c.lookup(9, CLASS_USAGE, 0) == Err(TT_ETOOL)
                && l.sample(11).map(|x| x.code) == Some(TT_EARGC)
                && l.sample(13).map(|x| x.code) == Some(TT_EMISS)
        },
        "static",
    );

    s.add(
        "TT8-非法工具档位",
        {
            let mut c = match Catalog::open(1) {
                Ok(v) => v,
                Err(_) => return s,
            };
            c.insert(0, CLASS_USAGE, 0x11, 0) == Err(TT_ETOOL)
                && c.insert(TOOL_CAT, 0, 0x11, 0) == Err(TT_ECLASS)
                && c.insert(4, CLASS_USAGE, 0x11, 0) == Err(TT_ETOOL)
                && c.len() == 0
                && l.sample(8).map(|x| x.code) == Some(TT_ETOOL)
                && l.sample(9).map(|x| x.code) == Some(TT_ECLASS)
        },
        "static",
    );

    s.add(
        "TT9-文案编号闸门",
        {
            let mut c = match Catalog::open(1) {
                Ok(v) => v,
                Err(_) => return s,
            };
            c.insert(TOOL_CAT, CLASS_USAGE, 0, 0) == Err(TT_ETEXT)
                && c.insert(TOOL_CAT, CLASS_USAGE, TEXT_MAX, 0) == Err(TT_ETEXT)
                && c.insert(TOOL_CAT, CLASS_USAGE, 0x11, 3) == Err(TT_EARGC)
                && c.insert(TOOL_CAT, CLASS_USAGE, 0x11, 0) == Ok(1)
                && l.sample(10).map(|x| x.code) == Some(TT_ETEXT)
        },
        "static",
    );

    s.add(
        "TT10-重复与表满",
        {
            let mut c = match Catalog::canonical(2) {
                Ok(v) => v,
                Err(_) => return s,
            };
            let full = c.insert(TOOL_CAT, CLASS_USAGE, 0x11, 0) == Err(TT_EFULL);
            let mut d = match Catalog::open(2) {
                Ok(v) => v,
                Err(_) => return s,
            };
            let _ = d.insert(TOOL_CAT, CLASS_USAGE, 0x11, 0);
            let dup = d.insert(TOOL_CAT, CLASS_USAGE, 0x11, 0) == Err(TT_EDUP);
            full && dup && d.len() == 1 && l.sample(12).map(|x| x.code) == Some(TT_EDUP)
        },
        "static",
    );

    s.add(
        "TT11-版本闸门",
        {
            matches!(Catalog::open(0), Err(TT_EVER))
                && Catalog::open(1).is_ok()
                && Catalog::open(0xffff_ffff).is_ok()
                && Catalog::canonical(7).map(|c| c.ver()) == Ok(7)
        },
        "static",
    );

    s.add(
        "TT12-同档同码跨工具",
        {
            let c = match Catalog::canonical(1) {
                Ok(v) => v,
                Err(_) => return s,
            };
            let a = c.lookup(TOOL_CAT, CLASS_USAGE, 0);
            let b = c.lookup(TOOL_MV, CLASS_USAGE, 0);
            let d = c.lookup(TOOL_CHMOD, CLASS_USAGE, 0);
            let dedup = match (a, b, d) {
                (Ok(x), Ok(y), Ok(z)) => x.text != y.text && y.text != z.text && x.text != z.text,
                _ => false,
            };
            dedup
                && needs_of(CLASS_USAGE) == Ok(0)
                && needs_of(CLASS_NOENT) == Ok(1)
                && needs_of(CLASS_DENIED) == Ok(1)
                && exit_of(CLASS_USAGE) == exit_of(CLASS_USAGE)
                && exit_of(CLASS_NOENT) == exit_of(CLASS_DENIED)
        },
        "static",
    );

    s.add(
        "TT13-冻结重放四注入",
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

    fn canon(ver: u32) -> Catalog {
        match Catalog::canonical(ver) {
            Ok(v) => v,
            Err(_) => panic!("canonical"),
        }
    }

    #[test]
    fn ledger_shape() {
        let l = build_ledger();
        assert_eq!(l.len(), SAMPLES);
        assert_eq!(l.ok_count(), 8);
        assert_eq!(l.err_count(), 6);
        assert_eq!(l.rejects(), 0);
        assert!(l.audit().is_ok());
        assert_ne!(l.seal(), 0);
        assert_eq!(l.replay(), Ok(l.seal()));
        let mut i = 0usize;
        while i < l.len() {
            assert_eq!(l.sample(i).map(|x| x.slot as usize), Some(i));
            i += 1;
        }
    }

    #[test]
    fn checks_pass() {
        let s = run_unx11416_checks();
        assert_eq!(s.len(), 13);
        assert!(s.all_passed());
    }

    #[test]
    fn truth_grid_complete() {
        let c = canon(1);
        assert_eq!(c.len(), CAP);
        assert_eq!(c.verify(), Ok(9));
        assert_eq!(
            c.lookup(TOOL_CHMOD, CLASS_DENIED, 1),
            Ok(Row { tool: TOOL_CHMOD, class: CLASS_DENIED, text: 0x33, argc: 1 })
        );
        assert_eq!(exit_of(CLASS_USAGE), Ok(2));
        assert_eq!(exit_of(CLASS_NOENT), Ok(1));
    }

    #[test]
    fn lookup_arity() {
        let c = canon(1);
        assert_eq!(c.lookup(TOOL_CAT, CLASS_USAGE, 1), Err(TT_EARGC));
        assert_eq!(c.lookup(TOOL_CAT, CLASS_NOENT, 0), Err(TT_EARGC));
        assert_eq!(c.lookup(TOOL_CAT, CLASS_DENIED, 1).map(|r| r.text), Ok(0x13));
    }

    #[test]
    fn insert_gates() {
        let mut c = match Catalog::open(1) {
            Ok(v) => v,
            Err(_) => panic!("open"),
        };
        assert_eq!(c.insert(0, CLASS_USAGE, 0x11, 0), Err(TT_ETOOL));
        assert_eq!(c.insert(TOOL_CAT, 0, 0x11, 0), Err(TT_ECLASS));
        assert_eq!(c.insert(TOOL_CAT, CLASS_USAGE, 0, 0), Err(TT_ETEXT));
        assert_eq!(c.insert(TOOL_CAT, CLASS_USAGE, 0x11, 3), Err(TT_EARGC));
        assert_eq!(c.insert(TOOL_CAT, CLASS_USAGE, 0x11, 0), Ok(1));
        assert_eq!(c.insert(TOOL_CAT, CLASS_USAGE, 0x11, 0), Err(TT_EDUP));
    }

    #[test]
    fn missing_row_and_full() {
        let c = canon(2);
        assert_eq!(c.lookup(TOOL_CAT, CLASS_DENIED, 1).map(|r| r.text), Ok(0x13));
        let mut f = canon(2);
        assert_eq!(f.insert(TOOL_CAT, CLASS_USAGE, 0x11, 0), Err(TT_EFULL));
        assert_eq!(Catalog::open(0).err(), Some(TT_EVER));
    }

    #[test]
    fn text_ids_encode_tool_and_class() {
        assert_eq!(text_of(TOOL_CAT, CLASS_USAGE), Ok(0x11));
        assert_eq!(text_of(TOOL_MV, CLASS_NOENT), Ok(0x22));
        assert_eq!(text_of(TOOL_CHMOD, CLASS_DENIED), Ok(0x33));
        assert_eq!(text_of(0, CLASS_USAGE), Err(TT_ETOOL));
        let c = canon(3);
        let mut i = 0usize;
        while i < c.len() {
            let r = match c.row_at(i) {
                Some(v) => v,
                None => panic!("row"),
            };
            assert_eq!((r.text >> 4) as u8, r.tool);
            assert_eq!((r.text & 0x0f) as u8, r.class);
            i += 1;
        }
    }

    #[test]
    fn inject_four_holds() {
        assert_eq!(inject_four(), [true; 4]);
    }
}
