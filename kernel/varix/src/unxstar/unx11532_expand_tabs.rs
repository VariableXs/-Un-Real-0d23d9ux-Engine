//! UNX-F11532 expand/unexpand 制表符展开
//!
//! `expand` 与 `unexpand` 是一对互逆的列宽映射：前者把制表符换成空格，后者把
//! 对齐到停位的空格串换回制表符。逆映射并不唯一——「八个空格」既可能是两个制表
//! 符也可能是八个真实空格——因此本模块只在**能被一次制表符精确覆盖**时才收缩，
//! 其余情形保留空格。这一判据保证 `unexpand(expand(x)) == x` 恒成立，而反向的
//! `expand(unexpand(y)) == y` 只在 y 本就对齐成立。
//!
//! 列计数遵循终端语义：
//!
//! - 可打印 ASCII 使列号加一；
//! - `\t` 把列号推到下一个停位；
//! - `\x08` 退格使列号减一，下限为零（不产生负列）；
//!
//! 停位有两种形态。未给表时按固定步长 `tab` 递推；给了表时表内各项就是绝对停位，
//! 表用尽后以「末项减前项」的差继续递推。表必须严格递增，否则停位不再单调，
//! 展开会倒退——这种表一律在构造期拒绝，而不是在渲染期悄悄排序。
//!
//! 非 ASCII 字节（≥0x80）与不可见控制字节一律 `EX_EBYTE`：本模块的列宽模型只对
//! 单字节字形成立，多字节码点在这里会被当成若干个独立列，结果必然错位。与其给出
//! 一个看起来合理却错误的列宽，不如拒绝。
//!
//! 闸门次序：
//!
//! - `Cfg::new` / `Cfg::with_list`：零步长 → 表超长 → 表非递增；
//! - `stop_after`：列号超出上限；
//! - `expand_line` / `unexpand_line`：行长超限 → 非法字节 → 列号溢出 → 输出满；
//! - `Ex::*_at`：行号越界 → 模式非法。

use crate::alloc::string::String;
use crate::checks::CheckSet;

/// 段基址。
pub const SEG_BASE: u32 = 0xf250;

/// 制表宽度为零或超限。
pub const EX_ETAB: u32 = SEG_BASE + 0x01;
/// 停位表非严格递增。
pub const EX_EORDER: u32 = SEG_BASE + 0x02;
/// 输出或停位表容量已满。
pub const EX_EFULL: u32 = SEG_BASE + 0x03;
/// 非法字节（非 ASCII 或不可见控制符）。
pub const EX_EBYTE: u32 = SEG_BASE + 0x04;
/// 行号越界。
pub const EX_ELINE: u32 = SEG_BASE + 0x05;
/// 状态非法（重复扫描等）。
pub const EX_ESTATE: u32 = SEG_BASE + 0x06;
/// 模式非法。
pub const EX_EMODE: u32 = SEG_BASE + 0x07;
/// 列号越界。
pub const EX_ECOL: u32 = SEG_BASE + 0x08;
/// 账本形状不符。
pub const EX_EBAL: u32 = SEG_BASE + 0x09;
/// 槽位乱序。
pub const EX_ESLOT: u32 = SEG_BASE + 0x0a;
/// 观测值与复算不符。
pub const EX_EVALUE: u32 = SEG_BASE + 0x0b;
/// 冻结后重放不一致。
pub const EX_ESEAL: u32 = SEG_BASE + 0x0c;
/// 观察下标越界。
pub const EX_ENOBS: u32 = SEG_BASE + 0x0d;

/// 行表容量。
pub const CAP: usize = 16;
/// 单行源字节上限。
pub const LINEMAX: usize = 64;
/// 列号上限。
pub const COLMAX: u32 = 256;
/// 停位表容量。
pub const LSTMAX: usize = 8;
/// 账本格数。
pub const SAMPLES: usize = 14;

/// 模式：展开。
pub const MODE_EXPAND: u8 = 1;
/// 模式：收缩。
pub const MODE_UNEXPAND: u8 = 2;

/// 操作码：展开。
pub const OP_EXPAND: u8 = 1;
/// 操作码：收缩。
pub const OP_UNEXPAND: u8 = 2;
/// 操作码：宽度。
pub const OP_WIDTH: u8 = 3;
/// 操作码：停位。
pub const OP_STOP: u8 = 4;

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

/// 生成 `n` 个空格。
pub fn pads(n: u32) -> String {
    let mut s = String::new();
    let mut i = 0u32;
    while i < n {
        s.push(' ');
        i += 1;
    }
    s
}

/// 制表配置。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Cfg {
    tab: u32,
    list: [u32; LSTMAX],
    nlist: usize,
    all: bool,
}

impl Cfg {
    /// 固定步长配置。
    pub fn new(tab: u32, all: bool) -> Result<Cfg, u32> {
        if tab == 0 || tab > COLMAX {
            return Err(EX_ETAB);
        }
        Ok(Cfg { tab, list: [0; LSTMAX], nlist: 0, all })
    }

    /// 显式停位表。
    pub fn with_list(list: &[u32], all: bool) -> Result<Cfg, u32> {
        if list.len() > LSTMAX {
            return Err(EX_EFULL);
        }
        let mut prev = 0u32;
        let mut i = 0usize;
        while i < list.len() {
            let v = match list.get(i) {
                Some(w) => *w,
                None => return Err(EX_EORDER),
            };
            if v == 0 || v > COLMAX || v <= prev {
                return Err(EX_EORDER);
            }
            prev = v;
            i += 1;
        }
        let mut cfg = Cfg { tab: prev.max(1), list: [0; LSTMAX], nlist: list.len(), all };
        let mut k = 0usize;
        while k < list.len() {
            if let (Some(dst), Some(src)) = (cfg.list.get_mut(k), list.get(k)) {
                *dst = *src;
            }
            k += 1;
        }
        Ok(cfg)
    }

    /// 步长。
    pub fn tab(&self) -> u32 {
        self.tab
    }

    /// 停位表长度。
    pub fn nlist(&self) -> usize {
        self.nlist
    }

    /// 停位表第 i 项。
    pub fn stop(&self, i: usize) -> Result<u32, u32> {
        if i >= self.nlist {
            return Err(EX_ELINE);
        }
        match self.list.get(i) {
            Some(v) => Ok(*v),
            None => Err(EX_ELINE),
        }
    }

    /// 是否只收缩行首。
    pub fn leading_only(&self) -> bool {
        !self.all
    }

    /// 列 `col` 之后的下一个停位。
    pub fn stop_after(&self, col: u32) -> Result<u32, u32> {
        if col >= COLMAX {
            return Err(EX_ECOL);
        }
        if self.nlist > 0 {
            let mut i = 0usize;
            while i < self.nlist {
                if let Some(v) = self.list.get(i) {
                    if *v > col {
                        return Ok(*v);
                    }
                }
                i += 1;
            }
            let last = self.list.get(self.nlist - 1).copied().unwrap_or(self.tab);
            let prev = if self.nlist >= 2 {
                self.list.get(self.nlist - 2).copied().unwrap_or(0)
            } else {
                0
            };
            let step = last.saturating_sub(prev).max(1);
            let mut s = last;
            while s <= col {
                s = s.saturating_add(step);
                if s > COLMAX {
                    return Err(EX_ECOL);
                }
            }
            return Ok(s);
        }
        let t = self.tab.max(1);
        let s = (col / t + 1).saturating_mul(t);
        if s > COLMAX {
            return Err(EX_ECOL);
        }
        Ok(s)
    }

    /// 列 `col` 是否恰为停位。
    pub fn at_stop(&self, col: u32) -> bool {
        if col == 0 {
            return true;
        }
        match self.stop_after(col.saturating_sub(1)) {
            Ok(v) => v == col,
            Err(_) => false,
        }
    }

    /// 展开一行。
    pub fn expand_line(&self, line: &[u8]) -> Result<String, u32> {
        if line.len() > LINEMAX {
            return Err(EX_EFULL);
        }
        let mut out = String::new();
        let mut col = 0u32;
        let mut i = 0usize;
        while i < line.len() {
            let b = match line.get(i) {
                Some(v) => *v,
                None => break,
            };
            match b {
                0x09 => {
                    let stop = self.stop_after(col)?;
                    let mut k = col;
                    while k < stop {
                        if out.len() >= LINEMAX {
                            return Err(EX_EFULL);
                        }
                        out.push(' ');
                        k += 1;
                    }
                    col = stop;
                }
                0x08 => {
                    if out.len() >= LINEMAX {
                        return Err(EX_EFULL);
                    }
                    out.push('\u{8}');
                    col = col.saturating_sub(1);
                }
                0x20..=0x7e => {
                    if out.len() >= LINEMAX {
                        return Err(EX_EFULL);
                    }
                    out.push(b as char);
                    col = col.saturating_add(1);
                    if col > COLMAX {
                        return Err(EX_ECOL);
                    }
                }
                _ => return Err(EX_EBYTE),
            }
            i += 1;
        }
        Ok(out)
    }

    /// 收缩一行。
    pub fn unexpand_line(&self, line: &[u8]) -> Result<String, u32> {
        if line.len() > LINEMAX {
            return Err(EX_EFULL);
        }
        let mut out = String::new();
        let mut col = 0u32;
        let mut run_start = 0u32;
        let mut run = 0u32;
        let mut i = 0usize;
        while i < line.len() {
            let b = match line.get(i) {
                Some(v) => *v,
                None => break,
            };
            if b == 0x20 {
                if run == 0 {
                    run_start = col;
                }
                run = run.saturating_add(1);
                col = col.saturating_add(1);
                if col > COLMAX {
                    return Err(EX_ECOL);
                }
                i += 1;
                continue;
            }
            if run > 0 {
                let end = run_start.saturating_add(run);
                let collapse = (self.all || run_start == 0)
                    && matches!(self.stop_after(run_start), Ok(v) if v == end);
                if collapse {
                    out.push('\t');
                } else {
                    let mut k = 0u32;
                    while k < run {
                        out.push(' ');
                        k += 1;
                    }
                }
                run = 0;
            }
            match b {
                0x09 => {
                    let stop = self.stop_after(col)?;
                    out.push('\t');
                    col = stop;
                }
                0x08 => {
                    out.push('\u{8}');
                    col = col.saturating_sub(1);
                }
                0x20..=0x7e => {
                    out.push(b as char);
                    col = col.saturating_add(1);
                    if col > COLMAX {
                        return Err(EX_ECOL);
                    }
                }
                _ => return Err(EX_EBYTE),
            }
            i += 1;
        }
        if run > 0 {
            let end = run_start.saturating_add(run);
            let collapse = (self.all || run_start == 0)
                && matches!(self.stop_after(run_start), Ok(v) if v == end);
            if collapse {
                out.push('\t');
            } else {
                let mut k = 0u32;
                while k < run {
                    out.push(' ');
                    k += 1;
                }
            }
        }
        if out.len() > LINEMAX {
            return Err(EX_EFULL);
        }
        Ok(out)
    }

    /// 展开后的列宽。
    pub fn width(&self, line: &[u8]) -> Result<u32, u32> {
        if line.len() > LINEMAX {
            return Err(EX_EFULL);
        }
        let mut col = 0u32;
        let mut i = 0usize;
        while i < line.len() {
            let b = match line.get(i) {
                Some(v) => *v,
                None => break,
            };
            match b {
                0x09 => col = self.stop_after(col)?,
                0x08 => col = col.saturating_sub(1),
                0x20..=0x7e => {
                    col = col.saturating_add(1);
                    if col > COLMAX {
                        return Err(EX_ECOL);
                    }
                }
                _ => return Err(EX_EBYTE),
            }
            i += 1;
        }
        Ok(col)
    }

    /// 一行里制表符个数。
    pub fn tabs(&self, line: &[u8]) -> u32 {
        let mut n = 0u32;
        let mut i = 0usize;
        while i < line.len() {
            if let Some(0x09) = line.get(i) {
                n = n.saturating_add(1);
            }
            i += 1;
        }
        n
    }
}

/// 行表。
#[derive(Clone, Copy, Debug)]
pub struct Ex<'a> {
    cfg: Cfg,
    lines: &'a [&'a [u8]],
    scanned: bool,
    width_total: u32,
}

impl<'a> Ex<'a> {
    /// 开户。
    pub fn open(cfg: Cfg, lines: &'a [&'a [u8]]) -> Result<Ex<'a>, u32> {
        if lines.len() > CAP {
            return Err(EX_EFULL);
        }
        Ok(Ex { cfg, lines, scanned: false, width_total: 0 })
    }

    /// 配置。
    pub fn cfg(&self) -> Cfg {
        self.cfg
    }

    /// 行数。
    pub fn len(&self) -> usize {
        self.lines.len()
    }

    /// 是否空表。
    pub fn is_empty(&self) -> bool {
        self.lines.is_empty()
    }

    /// 是否已扫描。
    pub fn scanned(&self) -> bool {
        self.scanned
    }

    /// 累计列宽。
    pub fn width_total(&self) -> u32 {
        self.width_total
    }

    /// 扫描。
    pub fn scan(&mut self) -> Result<u32, u32> {
        if self.scanned {
            return Err(EX_ESTATE);
        }
        let mut sum = 0u32;
        let mut i = 0usize;
        while i < self.lines.len() {
            let line = match self.lines.get(i) {
                Some(v) => *v,
                None => return Err(EX_ELINE),
            };
            sum = sum.saturating_add(self.cfg.width(line)?);
            i += 1;
        }
        self.width_total = sum;
        self.scanned = true;
        Ok(sum)
    }

    /// 第 i 行。
    pub fn line(&self, i: usize) -> Result<&'a [u8], u32> {
        if i >= self.lines.len() {
            return Err(EX_ELINE);
        }
        match self.lines.get(i) {
            Some(v) => Ok(*v),
            None => Err(EX_ELINE),
        }
    }

    /// 第 i 行展开。
    pub fn expand_at(&self, i: usize) -> Result<String, u32> {
        self.cfg.expand_line(self.line(i)?)
    }

    /// 第 i 行收缩。
    pub fn unexpand_at(&self, i: usize) -> Result<String, u32> {
        self.cfg.unexpand_line(self.line(i)?)
    }

    /// 第 i 行列宽。
    pub fn width_at(&self, i: usize) -> Result<u32, u32> {
        self.cfg.width(self.line(i)?)
    }

    /// 按模式取第 i 行。
    pub fn run(&self, i: usize, mode: u8) -> Result<String, u32> {
        let line = self.line(i)?;
        match mode {
            MODE_EXPAND => self.cfg.expand_line(line),
            MODE_UNEXPAND => self.cfg.unexpand_line(line),
            _ => Err(EX_EMODE),
        }
    }

    /// 往返：展开后再收缩。
    pub fn roundtrip(&self, i: usize) -> Result<String, u32> {
        let widened = self.expand_at(i)?;
        self.cfg.unexpand_line(widened.as_bytes())
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

/// `Result<String, u32>` → 样本，观测值取字节数。
fn text_cell(slot: u8, op: u8, r: Result<String, u32>) -> Sample {
    match r {
        Ok(v) => Sample { slot, op, ok: true, code: 0, value: v.len() as u32 },
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
            return Err(EX_ESLOT);
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
            return Err(EX_EBAL);
        }
        const VALS: [u32; 8] = [9, 9, 5, 3, 2, 10, 3, 8];
        const CODES: [u32; 6] = [EX_ETAB, EX_EBYTE, EX_EORDER, EX_EFULL, EX_EMODE, EX_ELINE];
        let mut vi = 0usize;
        let mut ci = 0usize;
        let mut i = 0usize;
        while i < self.len {
            let s = match self.rows.get(i) {
                Some(v) => *v,
                None => return Err(EX_EBAL),
            };
            if s.slot as usize != i {
                return Err(EX_ESLOT);
            }
            if s.ok {
                if s.code != 0 {
                    return Err(EX_EVALUE);
                }
                match VALS.get(vi) {
                    Some(w) => {
                        if s.value != *w {
                            return Err(EX_EVALUE);
                        }
                    }
                    None => return Err(EX_EBAL),
                }
                vi += 1;
            } else {
                match CODES.get(ci) {
                    Some(w) => {
                        if s.code != *w {
                            return Err(EX_EVALUE);
                        }
                    }
                    None => return Err(EX_EBAL),
                }
                ci += 1;
            }
            i += 1;
        }
        if vi != 8 || ci != 6 {
            return Err(EX_EBAL);
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
            return Err(EX_EBAL);
        }
        self.root = self.seal();
        self.frozen = true;
        Ok(self.root)
    }

    /// 重放。
    pub fn replay(&self) -> Result<u64, u32> {
        let fresh = build_ledger();
        let r = fresh.seal();
        if r == self.seal() {
            Ok(r)
        } else {
            Err(EX_ESEAL)
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

/// 十四格账本：展开、收缩、列宽、停位与六类拒绝。
pub fn build_ledger() -> Ledger {
    let mut l = Ledger::new();
    let eight = match Cfg::new(8, true) {
        Ok(v) => v,
        Err(_) => return l,
    };

    let _ = l.register(text_cell(0, OP_EXPAND, eight.expand_line(b"a\tb")));
    let _ = l.register(text_cell(1, OP_EXPAND, eight.expand_line(b"\tx")));

    let four = match Cfg::new(4, true) {
        Ok(v) => v,
        Err(_) => return l,
    };
    let _ = l.register(text_cell(2, OP_EXPAND, four.expand_line(b"ab\tc")));

    let _ = l.register(text_cell(3, OP_UNEXPAND, eight.unexpand_line(b"a       b")));
    let _ = l.register(text_cell(4, OP_UNEXPAND, eight.unexpand_line(b"        x")));
    let _ = l.register(word_cell(5, OP_WIDTH, eight.width(b"a\tbc")));
    let _ = l.register(word_cell(6, OP_WIDTH, eight.width(b"abc")));
    let _ = l.register(word_cell(7, OP_STOP, eight.stop_after(0)));

    let _ = l.register(word_cell(8, OP_STOP, Cfg::new(0, true).map(|c| c.tab())));

    let bad = match Cfg::new(8, true) {
        Ok(v) => v,
        Err(_) => return l,
    };
    let _ = l.register(text_cell(9, OP_EXPAND, bad.expand_line(b"a\x80b")));

    let _ = l.register(word_cell(10, OP_STOP, Cfg::with_list(&[8, 4], true).map(|c| c.tab())));

    let long = [b'a'; LINEMAX + 1];
    let _ = l.register(text_cell(11, OP_EXPAND, eight.expand_line(&long)));

    let lines: [&[u8]; 1] = [b"ab"];
    let ex = match Ex::open(eight, &lines) {
        Ok(v) => v,
        Err(_) => return l,
    };
    let _ = l.register(text_cell(12, OP_EXPAND, ex.run(0, 9)));

    let _ = l.register(text_cell(13, OP_EXPAND, ex.expand_at(7)));

    l
}

/// 四路注入。
pub fn inject_four() -> [bool; 4] {
    let mut out = [false; 4];

    let l = build_ledger();
    let rep = l.replay();
    let root = l.seal();
    out[0] = l.len() == SAMPLES && l.audit().is_ok() && rep == Ok(root) && root != 0;

    // 一路：固定步长展开与退格。
    let eight = match Cfg::new(8, true) {
        Ok(v) => v,
        Err(_) => return out,
    };
    let mut want = String::from("a");
    want.push_str(&pads(7));
    want.push('b');
    let basic = eight.expand_line(b"a\tb") == Ok(want);

    let mut lead = pads(8);
    lead.push('x');
    let zero = eight.expand_line(b"\tx") == Ok(lead.clone());

    let bs = eight.width(b"ab\x08\x08\t") == Ok(8) && eight.width(b"ab\x08\x08") == Ok(0);
    out[1] = basic && zero && bs;

    // 二路：收缩与往返。
    let mut spaced = String::from("a");
    spaced.push_str(&pads(7));
    spaced.push('b');
    let back = eight.unexpand_line(spaced.as_bytes()) == Ok(String::from("a\tb"));
    let lead_back = eight.unexpand_line(lead.as_bytes()) == Ok(String::from("\tx"));

    let lines: [&[u8]; 3] = [b"a\tb", b"ab\tcd", b"\t"];
    let ex = match Ex::open(eight, &lines) {
        Ok(v) => v,
        Err(_) => return out,
    };
    let trip = ex.roundtrip(0) == Ok(String::from("a\tb"))
        && ex.roundtrip(1) == Ok(String::from("ab\tcd"))
        && ex.roundtrip(2) == Ok(String::from("\t"));
    out[2] = back && lead_back && trip;

    // 三路：停位表、列宽与拒绝。
    let stops = match Cfg::with_list(&[4, 8, 16], true) {
        Ok(v) => v,
        Err(_) => return out,
    };
    let list = stops.stop_after(0) == Ok(4)
        && stops.stop_after(4) == Ok(8)
        && stops.stop_after(9) == Ok(16)
        && stops.stop_after(16) == Ok(24);
    let width = stops.width(b"ab\tc") == Ok(5) && eight.width(b"abc") == Ok(3);

    let tab0 = matches!(Cfg::new(0, true), Err(EX_ETAB));
    let order = matches!(Cfg::with_list(&[8, 8], true), Err(EX_EORDER));
    let byte = eight.expand_line(b"\x7f") == Err(EX_EBYTE);
    let long = [b'a'; LINEMAX + 1];
    let full = eight.expand_line(&long) == Err(EX_EFULL);
    let mode = ex.run(0, 5) == Err(EX_EMODE);
    let line = ex.expand_at(9) == Err(EX_ELINE);
    out[3] = list
        && width
        && tab0
        && order
        && byte
        && full
        && mode
        && line
        && Ex::open(eight, &lines[..0]).map(|e| e.is_empty()) == Ok(true);

    out
}

/// 十三项判据。
pub fn run_unx11532_checks() -> CheckSet {
    let mut s = CheckSet::new("unx11532.expand_tabs");

    s.add(
        "EX01-固定步长展开",
        {
            let c = match Cfg::new(8, true) {
                Ok(v) => v,
                Err(_) => return s,
            };
            let mut want = String::from("a");
            want.push_str(&pads(7));
            want.push('b');
            c.expand_line(b"a\tb") == Ok(want)
                && c.expand_line(b"\tx") == Ok({
                    let mut w = pads(8);
                    w.push('x');
                    w
                })
        },
        "static",
    );

    s.add(
        "EX02-步长四与多制表",
        {
            let c = match Cfg::new(4, true) {
                Ok(v) => v,
                Err(_) => return s,
            };
            let mut want = String::from("ab");
            want.push_str(&pads(2));
            want.push('c');
            let mut two = String::from("a");
            two.push_str(&pads(3));
            two.push('b');
            two.push_str(&pads(3));
            two.push('c');
            c.expand_line(b"ab\tc") == Ok(want) && c.expand_line(b"a\tb\tc") == Ok(two)
        },
        "static",
    );

    s.add(
        "EX03-收缩相邻空格",
        {
            let c = match Cfg::new(8, true) {
                Ok(v) => v,
                Err(_) => return s,
            };
            let mut spaced = String::from("a");
            spaced.push_str(&pads(7));
            spaced.push('b');
            let mut two = String::from("ab");
            two.push_str(&pads(6));
            two.push('c');
            c.unexpand_line(spaced.as_bytes()) == Ok(String::from("a\tb"))
                && c.unexpand_line(two.as_bytes()) == Ok(String::from("ab\tc"))
        },
        "static",
    );

    s.add(
        "EX04-未对齐空格不收缩",
        {
            let c = match Cfg::new(8, true) {
                Ok(v) => v,
                Err(_) => return s,
            };
            let mut odd = String::from("a");
            odd.push_str(&pads(3));
            odd.push('b');
            c.unexpand_line(odd.as_bytes()) == Ok(odd.clone())
                && c.unexpand_line(b"ab") == Ok(String::from("ab"))
        },
        "static",
    );

    s.add(
        "EX05-行首限定",
        {
            let c = match Cfg::new(8, false) {
                Ok(v) => v,
                Err(_) => return s,
            };
            let mut inside = String::from("a");
            inside.push_str(&pads(7));
            inside.push('b');
            let mut lead = pads(8);
            lead.push('x');
            c.unexpand_line(inside.as_bytes()) == Ok(inside.clone())
                && c.unexpand_line(lead.as_bytes()) == Ok(String::from("\tx"))
        },
        "static",
    );

    s.add(
        "EX06-显式停位表",
        {
            let c = match Cfg::with_list(&[4, 8, 16], true) {
                Ok(v) => v,
                Err(_) => return s,
            };
            c.nlist() == 3
                && c.stop(0) == Ok(4)
                && c.stop(2) == Ok(16)
                && c.stop(3) == Err(EX_ELINE)
                && c.stop_after(0) == Ok(4)
                && c.stop_after(4) == Ok(8)
                && c.stop_after(9) == Ok(16)
                && c.stop_after(16) == Ok(24)
                && c.width(b"ab\tc") == Ok(5)
        },
        "static",
    );

    s.add(
        "EX07-列宽与退格",
        {
            let c = match Cfg::new(8, true) {
                Ok(v) => v,
                Err(_) => return s,
            };
            c.width(b"a\tbc") == Ok(10)
                && c.width(b"abc") == Ok(3)
                && c.width(b"ab\x08\x08") == Ok(0)
                && c.width(b"ab\x08\x08\x08") == Ok(0)
                && c.tabs(b"a\tb\tc") == 2
        },
        "static",
    );

    s.add(
        "EX08-非法字节拒绝",
        {
            let c = match Cfg::new(8, true) {
                Ok(v) => v,
                Err(_) => return s,
            };
            c.expand_line(b"\x00") == Err(EX_EBYTE)
                && c.expand_line(b"\x7f") == Err(EX_EBYTE)
                && c.expand_line(b"\xff") == Err(EX_EBYTE)
                && c.width(b"\x1f") == Err(EX_EBYTE)
                && c.unexpand_line(b"\x80") == Err(EX_EBYTE)
        },
        "static",
    );

    s.add(
        "EX09-容量与表序",
        {
            let c = match Cfg::new(8, true) {
                Ok(v) => v,
                Err(_) => return s,
            };
            let long = [b'a'; LINEMAX + 1];
            let wide = [b'\t'; LINEMAX];
            c.expand_line(&long) == Err(EX_EFULL)
                && c.expand_line(&wide).is_err()
                && matches!(Cfg::new(0, true), Err(EX_ETAB))
                && matches!(Cfg::with_list(&[8, 8], true), Err(EX_EORDER))
                && matches!(Cfg::with_list(&[0], true), Err(EX_EORDER))
                && matches!(Cfg::with_list(&[1; LSTMAX + 1], true), Err(EX_EFULL))
        },
        "static",
    );

    s.add(
        "EX10-状态与模式闸门",
        {
            let c = match Cfg::new(8, true) {
                Ok(v) => v,
                Err(_) => return s,
            };
            let lines: [&[u8]; 2] = [b"a\tb", b"ab"];
            let mut ex = match Ex::open(c, &lines) {
                Ok(v) => v,
                Err(_) => return s,
            };
            let first = ex.scan() == Ok(11);
            let twice = ex.scan() == Err(EX_ESTATE);
            let mode = ex.run(0, 0) == Err(EX_EMODE);
            let line = ex.run(5, MODE_EXPAND) == Err(EX_ELINE);
            first && twice && mode && line && ex.width_total() == 11 && ex.scanned()
        },
        "static",
    );

    s.add(
        "EX11-账本审计",
        {
            let l = build_ledger();
            l.len() == SAMPLES
                && l.ok_count() == 8
                && l.err_count() == 6
                && l.audit().is_ok()
                && l.rejects() == 0
        },
        "static",
    );

    s.add(
        "EX12-冻结与重放",
        {
            let mut l = build_ledger();
            let a = l.freeze();
            let b = l.freeze();
            let c = l.register(Sample::blank());
            let sealed = l.seal();
            l.is_frozen()
                && a.is_ok()
                && b == Err(EX_EBAL)
                && c == Err(EX_ESLOT)
                && l.rejects() == 2
                && l.replay() == Err(EX_ESEAL)
                && sealed != 0
        },
        "static",
    );

    s.add(
        "EX13-冻结重放四注入",
        inject_four() == [true; 4] && build_ledger().audit().is_ok(),
        "static",
    );

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn expand_basic() {
        let c = Cfg::new(8, true).ok().unwrap();
        let mut want = String::from("a");
        want.push_str(&pads(7));
        want.push('b');
        assert_eq!(c.expand_line(b"a\tb"), Ok(want));
        assert_eq!(c.expand_line(b"abc"), Ok(String::from("abc")));
        assert_eq!(c.expand_line(b""), Ok(String::from("")));
    }

    #[test]
    fn expand_tabs_chain() {
        let c = Cfg::new(4, true).ok().unwrap();
        let mut two = String::from("a");
        two.push_str(&pads(3));
        two.push('b');
        two.push_str(&pads(3));
        two.push('c');
        assert_eq!(c.expand_line(b"a\tb\tc"), Ok(two));
        assert_eq!(c.width(b"a\tb\tc"), Ok(9));
    }

    #[test]
    fn unexpand_roundtrip() {
        let c = Cfg::new(8, true).ok().unwrap();
        let lines: [&[u8]; 3] = [b"a\tb", b"ab\tcd", b"\t"];
        let ex = Ex::open(c, &lines).ok().unwrap();
        assert_eq!(ex.roundtrip(0), Ok(String::from("a\tb")));
        assert_eq!(ex.roundtrip(1), Ok(String::from("ab\tcd")));
        assert_eq!(ex.roundtrip(2), Ok(String::from("\t")));
    }

    #[test]
    fn stops_and_width() {
        let c = Cfg::with_list(&[4, 8, 16], true).ok().unwrap();
        assert_eq!(c.stop_after(9), Ok(16));
        assert_eq!(c.stop_after(16), Ok(24));
        assert!(c.at_stop(8));
        assert!(!c.at_stop(9));
        assert_eq!(c.width(b"ab\tc"), Ok(5));
    }

    #[test]
    fn rejections() {
        assert_eq!(Cfg::new(0, true), Err(EX_ETAB));
        assert_eq!(Cfg::with_list(&[8, 4], true), Err(EX_EORDER));
        let c = Cfg::new(8, true).ok().unwrap();
        assert_eq!(c.expand_line(b"\x00"), Err(EX_EBYTE));
        let long = [b'a'; LINEMAX + 1];
        assert_eq!(c.expand_line(&long), Err(EX_EFULL));
    }

    #[test]
    fn scan_and_gates() {
        let c = Cfg::new(8, true).ok().unwrap();
        let lines: [&[u8]; 2] = [b"a\tb", b"ab"];
        let mut ex = Ex::open(c, &lines).ok().unwrap();
        assert_eq!(ex.scan(), Ok(11));
        assert_eq!(ex.scan(), Err(EX_ESTATE));
        assert_eq!(ex.run(0, 9), Err(EX_EMODE));
        assert_eq!(ex.expand_at(4), Err(EX_ELINE));
        assert_eq!(ex.len(), 2);
        assert!(!ex.is_empty());
    }

    #[test]
    fn ledger_and_freeze() {
        let mut l = build_ledger();
        assert_eq!(l.len(), SAMPLES);
        assert_eq!(l.audit(), Ok(()));
        let r = l.freeze().ok().unwrap();
        assert_eq!(l.root(), r);
        assert_eq!(l.replay(), Ok(r));
        assert_eq!(l.freeze(), Err(EX_EBAL));
        assert_eq!(l.register(Sample::blank()), Err(EX_ESLOT));
        assert_eq!(l.rejects(), 2);
        assert_eq!(l.replay(), Err(EX_ESEAL));
        assert_eq!(l.sample(SAMPLES), None);
    }

    #[test]
    fn four_injections() {
        assert_eq!(inject_four(), [true; 4]);
    }
}
