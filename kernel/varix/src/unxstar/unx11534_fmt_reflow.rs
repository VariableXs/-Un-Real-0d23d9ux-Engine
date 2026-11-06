//! UNX-F11534 fmt 段落重排
//!
//! `fmt` 把段落内的短行合并成连续的文字流，再按目标宽度重新断行。它的价值不在
//! 「换行」本身，而在三个守恒量：宽度、缩进、词间距。任何一次重排都必须保持词
//! 的顺序不变、词内字节不变，只允许在词与词之间增删空格或插入换行。
//!
//! 段落边界由空行确定。空行之前的内容属于上一段，空行之后另起一段；输出里段落
//! 之间仍然只隔一个换行，不保留原始空行数量。`-s`（split-only）会关闭合并：每一
//! 条非空输入行各自成为一段，只对超宽行做切分，绝不把两行粘在一起——这是 GNU
//! fmt 与朴素折行器最容易混淆的地方。
//!
//! 缩进有两档：段落首行用 first_indent，后续行用 rest_indent。两档都必须小于
//! 宽度，否则一行连缩进都放不下，属于参数错误而不是运行期错误。单词长度也受
//! 宽度约束：一个词如果连「最窄缩进 + 词长」都超过宽度，就没有合法断点，必须
//! 在入口拒绝，而不是悄悄溢出或把词劈开。
//!
//! `-u`（uniform）只改变词间距：句末标点（`.` `?` `!`）之后的词间空隙固定为两个
//! 空格，其余为一个。它不改变断行判定所依据的列宽——判定仍然按「一个空格」的
//! 保守值计算，避免因为多加的一个空格把行顶出宽度。换句话说，uniform 是渲染层
//! 的修饰，不是布局层的输入。
//!
//! 行表是定容的：段数、输入行数、输出字节数都有硬上限。超限一律显性拒绝
//! （`FMT_EFULL`），不做截断，因为截断会静默丢词，破坏「词序守恒」这一根本不变式。
//!
//! 闸门次序：
//!
//! - `Cfg::new`：宽度为零 → 宽度超限 → 缩进越界 → 模式冲突；
//! - `reflow_n`：行表超限 → 非法字节 → 单词超宽 → 输出超限；
//! - `Fmt::open`：行数超限；
//! - `Fmt::scan`：重复扫描；
//! - `Fmt::render`：模式不符。

use crate::alloc::string::String;
use crate::checks::CheckSet;

/// 段基址。
pub const SEG_BASE: u32 = 0xf270;

/// 宽度为零。
pub const FMT_EZERO: u32 = SEG_BASE + 0x01;
/// 宽度超限。
pub const FMT_EWIDE: u32 = SEG_BASE + 0x02;
/// 缩进不小于宽度。
pub const FMT_EINDENT: u32 = SEG_BASE + 0x03;
/// 非法字节。
pub const FMT_EBYTE: u32 = SEG_BASE + 0x04;
/// 单词超宽。
pub const FMT_ELONG: u32 = SEG_BASE + 0x05;
/// 行表或输出已满。
pub const FMT_EFULL: u32 = SEG_BASE + 0x06;
/// 状态非法。
pub const FMT_ESTATE: u32 = SEG_BASE + 0x07;
/// 模式冲突。
pub const FMT_EMODE: u32 = SEG_BASE + 0x08;
/// 账本形状不符。
pub const FMT_EBAL: u32 = SEG_BASE + 0x09;
/// 槽位乱序。
pub const FMT_ESLOT: u32 = SEG_BASE + 0x0a;
/// 观测值与复算不符。
pub const FMT_EVALUE: u32 = SEG_BASE + 0x0b;
/// 冻结后重放不一致。
pub const FMT_ESEAL: u32 = SEG_BASE + 0x0c;
/// 观察下标越界。
pub const FMT_ENOBS: u32 = SEG_BASE + 0x0d;

/// 输入行容量。
pub const CAP: usize = 24;
/// 宽度上限。
pub const MAXW: u32 = 100;
/// 输出字节上限。
pub const OUTMAX: usize = 768;
/// 账本格数。
pub const SAMPLES: usize = 14;
/// 模式：合并段落。
pub const MODE_JOIN: u8 = 1;
/// 模式：仅切分。
pub const MODE_SPLIT: u8 = 2;

/// 操作码：重排。
pub const OP_REFLOW: u8 = 1;
/// 操作码：度量。
pub const OP_METRIC: u8 = 2;
/// 操作码：词数。
pub const OP_WORDS: u8 = 3;
/// 操作码：扫描。
pub const OP_SCAN: u8 = 4;

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

/// 重排参数。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Cfg {
    width: u32,
    first: u32,
    rest: u32,
    uniform: bool,
    split: bool,
}

impl Cfg {
    /// 构造并过参数闸门。
    pub fn new(width: u32, first: u32, rest: u32, uniform: bool, split: bool) -> Result<Cfg, u32> {
        if width == 0 {
            return Err(FMT_EZERO);
        }
        if width > MAXW {
            return Err(FMT_EWIDE);
        }
        if first >= width || rest >= width {
            return Err(FMT_EINDENT);
        }
        if uniform && split {
            return Err(FMT_EMODE);
        }
        Ok(Cfg { width, first, rest, uniform, split })
    }

    /// 宽度。
    pub fn width(&self) -> u32 {
        self.width
    }

    /// 首行缩进。
    pub fn first_indent(&self) -> u32 {
        self.first
    }

    /// 续行缩进。
    pub fn rest_indent(&self) -> u32 {
        self.rest
    }

    /// 是否统一词距。
    pub fn uniform(&self) -> bool {
        self.uniform
    }

    /// 是否仅切分。
    pub fn split_only(&self) -> bool {
        self.split
    }

    /// 模式码。
    pub fn mode(&self) -> u8 {
        if self.split {
            MODE_SPLIT
        } else {
            MODE_JOIN
        }
    }
}

/// 字节是否可打印（空格与可见 ASCII）。
fn printable(b: u8) -> bool {
    b == 0x20 || (0x21..=0x7e).contains(&b)
}

/// 句末标点。
fn sentence_end(b: u8) -> bool {
    b == b'.' || b == b'?' || b == b'!'
}

/// 校验一条输入行。
fn check_line(line: &[u8]) -> Result<(), u32> {
    let mut i = 0usize;
    while i < line.len() {
        match line.get(i) {
            Some(b) => {
                if !printable(*b) {
                    return Err(FMT_EBYTE);
                }
            }
            None => return Err(FMT_EBYTE),
        }
        i += 1;
    }
    Ok(())
}

/// 把一条输出行收尾：换行并压入续行缩进。
fn open_line(out: &mut String, indent: u32) {
    out.push('\n');
    let mut k = 0u32;
    while k < indent {
        out.push(' ');
        k += 1;
    }
}

/// 渲染一个词。返回新的列号。
///
/// `col` 是当前行已有列数；`indent` 是本行缩进；`sentence` 记录上一词是否句末。
fn put_word(
    out: &mut String,
    cfg: &Cfg,
    col: u32,
    indent: u32,
    word: &[u8],
    have: bool,
    sentence: bool,
) -> Result<u32, u32> {
    let wlen = word.len() as u32;
    if indent.saturating_add(wlen) > cfg.width {
        return Err(FMT_ELONG);
    }
    let gap = if !have {
        0
    } else if cfg.uniform && sentence {
        2
    } else {
        1
    };
    let need = col.saturating_add(gap).saturating_add(wlen);
    let mut c = col;
    if have && need > cfg.width {
        open_line(out, cfg.rest);
        c = cfg.rest;
        let mut i = 0usize;
        while i < word.len() {
            if let Some(b) = word.get(i) {
                out.push(*b as char);
            }
            i += 1;
        }
        return Ok(c.saturating_add(wlen));
    }
    if !have {
        let mut k = 0u32;
        while k < indent {
            out.push(' ');
            k += 1;
        }
        c = indent;
    } else {
        let mut k = 0u32;
        while k < gap {
            out.push(' ');
            k += 1;
        }
        c = c.saturating_add(gap);
    }
    let mut i = 0usize;
    while i < word.len() {
        if let Some(b) = word.get(i) {
            out.push(*b as char);
        }
        i += 1;
    }
    Ok(c.saturating_add(wlen))
}

/// 处理一段（已合并的若干行）。`split_each` 为真时每条源行单独成段。
fn reflow_n(cfg: &Cfg, lines: &[&[u8]], n: usize) -> Result<String, u32> {
    if n > CAP {
        return Err(FMT_EFULL);
    }
    let mut i = 0usize;
    while i < n {
        match lines.get(i) {
            Some(line) => check_line(line)?,
            None => return Err(FMT_EFULL),
        }
        i += 1;
    }
    let mut out = String::new();
    let mut col = 0u32;
    let mut first_line = true;
    let mut have = false;
    let mut sentence = false;
    let mut started = false;
    let mut para_open = false;
    i = 0;
    while i < n {
        let line = match lines.get(i) {
            Some(v) => *v,
            None => return Err(FMT_EFULL),
        };
        if line.is_empty() {
            // 空行：收段。
            if para_open {
                have = false;
                col = 0;
                first_line = true;
                sentence = false;
                para_open = false;
            }
            i += 1;
            continue;
        }
        if !para_open && started {
            out.push('\n');
            have = false;
            col = 0;
            first_line = true;
            sentence = false;
        }
        para_open = true;
        started = true;
        let mut p = 0usize;
        while p < line.len() {
            // 跳过词间空格。
            while p < line.len() {
                match line.get(p) {
                    Some(b) if *b == 0x20 => p += 1,
                    _ => break,
                }
            }
            if p >= line.len() {
                break;
            }
            let start = p;
            while p < line.len() {
                match line.get(p) {
                    Some(b) if *b == 0x20 => break,
                    _ => p += 1,
                }
            }
            let end = p;
            let word = match line.get(start..end) {
                Some(w) => w,
                None => return Err(FMT_EBYTE),
            };
            if word.is_empty() {
                break;
            }
            let ind = if first_line { cfg.first } else { cfg.rest };
            col = put_word(&mut out, cfg, col, ind, word, have, sentence)?;
            if out.len() > OUTMAX {
                return Err(FMT_EFULL);
            }
            have = true;
            first_line = false;
            let last = match word.get(word.len().saturating_sub(1)) {
                Some(b) => *b,
                None => 0,
            };
            sentence = sentence_end(last);
        }
        if cfg.split {
            // split-only：每条源行自成一段。
            have = false;
            col = 0;
            first_line = true;
            sentence = false;
            para_open = false;
        }
        i += 1;
    }
    Ok(out)
}

/// 公共入口：重排。
pub fn reflow(cfg: Cfg, lines: &[&[u8]]) -> Result<String, u32> {
    reflow_n(&cfg, lines, lines.len())
}

/// 词数统计（空白分隔）。
pub fn word_count(lines: &[&[u8]]) -> Result<u32, u32> {
    if lines.len() > CAP {
        return Err(FMT_EFULL);
    }
    let mut n = 0u32;
    let mut i = 0usize;
    while i < lines.len() {
        let line = match lines.get(i) {
            Some(v) => *v,
            None => return Err(FMT_EFULL),
        };
        check_line(line)?;
        let mut p = 0usize;
        let mut in_word = false;
        while p < line.len() {
            let b = match line.get(p) {
                Some(v) => *v,
                None => break,
            };
            if b == 0x20 {
                in_word = false;
            } else if !in_word {
                in_word = true;
                n = n.saturating_add(1);
            }
            p += 1;
        }
        i += 1;
    }
    Ok(n)
}

/// 输出度量：行数与最大列宽。
pub fn metrics(cfg: Cfg, lines: &[&[u8]]) -> Result<(u32, u32), u32> {
    let text = reflow(cfg, lines)?;
    let bytes = text.as_bytes();
    let mut rows = if bytes.is_empty() { 0 } else { 1 };
    let mut max = 0u32;
    let mut cur = 0u32;
    let mut i = 0usize;
    while i < bytes.len() {
        match bytes.get(i) {
            Some(b) if *b == b'\n' => {
                if cur > max {
                    max = cur;
                }
                rows += 1;
                cur = 0;
            }
            Some(_) => cur = cur.saturating_add(1),
            None => break,
        }
        i += 1;
    }
    if cur > max {
        max = cur;
    }
    Ok((rows, max))
}

/// 段表状态机。
#[derive(Clone, Copy)]
pub struct Fmt<'a> {
    cfg: Cfg,
    lines: [&'a [u8]; CAP],
    n: usize,
    scanned: bool,
}

impl<'a> Fmt<'a> {
    /// 打开行表。
    pub fn open(cfg: Cfg, lines: &[&'a [u8]]) -> Result<Fmt<'a>, u32> {
        if lines.len() > CAP {
            return Err(FMT_EFULL);
        }
        let mut tab: [&[u8]; CAP] = [&[]; CAP];
        let mut i = 0usize;
        while i < lines.len() {
            if let Some(v) = lines.get(i) {
                if let Some(dst) = tab.get_mut(i) {
                    *dst = *v;
                }
            }
            i += 1;
        }
        Ok(Fmt { cfg, lines: tab, n: lines.len(), scanned: false })
    }

    /// 行数。
    pub fn len(&self) -> usize {
        self.n
    }

    /// 是否空表。
    pub fn is_empty(&self) -> bool {
        self.n == 0
    }

    /// 参数。
    pub fn cfg(&self) -> Cfg {
        self.cfg
    }

    /// 扫描一次（返回输出字节数）。
    pub fn scan(&mut self) -> Result<u32, u32> {
        if self.scanned {
            return Err(FMT_ESTATE);
        }
        let text = reflow_n(&self.cfg, &self.lines, self.n)?;
        self.scanned = true;
        Ok(text.len() as u32)
    }

    /// 按模式渲染。
    pub fn render(&self, mode: u8) -> Result<String, u32> {
        if mode != self.cfg.mode() {
            return Err(FMT_EMODE);
        }
        reflow_n(&self.cfg, &self.lines, self.n)
    }

    /// 是否已扫描。
    pub fn scanned(&self) -> bool {
        self.scanned
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

/// `Result<usize, u32>` → 样本。
fn count_cell(slot: u8, op: u8, r: Result<usize, u32>) -> Sample {
    match r {
        Ok(v) => Sample { slot, op, ok: true, code: 0, value: v as u32 },
        Err(c) => Sample { slot, op, ok: false, code: c, value: 0 },
    }
}

/// `Result<String, u32>` → 样本（值为字节长度）。
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
            return Err(FMT_ESLOT);
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
            return Err(FMT_EBAL);
        }
        const VALS: [u32; 8] = [11, 19, 8, 11, 10, 3, 2, 4];
        const CODES: [u32; 6] = [
            FMT_EZERO,
            FMT_EWIDE,
            FMT_EINDENT,
            FMT_EBYTE,
            FMT_ELONG,
            FMT_EFULL,
        ];
        let mut vi = 0usize;
        let mut ci = 0usize;
        let mut i = 0usize;
        while i < self.len {
            let s = match self.rows.get(i) {
                Some(v) => *v,
                None => return Err(FMT_EBAL),
            };
            if s.slot as usize != i {
                return Err(FMT_ESLOT);
            }
            if s.ok {
                if s.code != 0 {
                    return Err(FMT_EVALUE);
                }
                match VALS.get(vi) {
                    Some(w) => {
                        if s.value != *w {
                            return Err(FMT_EVALUE);
                        }
                    }
                    None => return Err(FMT_EBAL),
                }
                vi += 1;
            } else {
                match CODES.get(ci) {
                    Some(w) => {
                        if s.code != *w {
                            return Err(FMT_EVALUE);
                        }
                    }
                    None => return Err(FMT_EBAL),
                }
                ci += 1;
            }
            i += 1;
        }
        if vi != 8 || ci != 6 {
            return Err(FMT_EBAL);
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
            return Err(FMT_EBAL);
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
            Err(FMT_ESEAL)
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

/// 十四格账本：合并、缩进、split-only、uniform 与六类拒绝。
pub fn build_ledger() -> Ledger {
    let mut l = Ledger::new();

    let c0 = match Cfg::new(10, 0, 0, false, false) {
        Ok(v) => v,
        Err(_) => return l,
    };
    let one: [&[u8]; 1] = [b"aaa bbb ccc"];
    let _ = l.register(text_cell(0, OP_REFLOW, reflow(c0, &one)));

    let c1 = match Cfg::new(12, 2, 1, false, false) {
        Ok(v) => v,
        Err(_) => return l,
    };
    let alpha: [&[u8]; 1] = [b"alpha beta gamma"];
    let _ = l.register(text_cell(1, OP_REFLOW, reflow(c1, &alpha)));

    let c2 = match Cfg::new(8, 0, 0, false, true) {
        Ok(v) => v,
        Err(_) => return l,
    };
    let two: [&[u8]; 2] = [b"aa bb", b"cc"];
    let _ = l.register(text_cell(2, OP_REFLOW, reflow(c2, &two)));

    let c3 = match Cfg::new(10, 0, 0, false, true) {
        Ok(v) => v,
        Err(_) => return l,
    };
    let splitline: [&[u8]; 1] = [b"aa bb cc dd"];
    let _ = l.register(text_cell(3, OP_REFLOW, reflow(c3, &splitline)));

    let c4 = match Cfg::new(20, 0, 0, true, false) {
        Ok(v) => v,
        Err(_) => return l,
    };
    let uni: [&[u8]; 1] = [b"hi. there"];
    let _ = l.register(text_cell(4, OP_REFLOW, reflow(c4, &uni)));

    let words: [&[u8]; 1] = [b"aa bb  cc"];
    let _ = l.register(word_cell(5, OP_WORDS, word_count(&words)));

    let c6 = match Cfg::new(5, 0, 0, false, false) {
        Ok(v) => v,
        Err(_) => return l,
    };
    let five: [&[u8]; 1] = [b"aa bb cc"];
    let _ = l.register(count_cell(6, OP_METRIC, metrics(c6, &five).map(|m| m.0 as usize)));

    let c7 = match Cfg::new(6, 0, 0, false, false) {
        Ok(v) => v,
        Err(_) => return l,
    };
    let six: [&[u8]; 1] = [b"aaaa bbbb"];
    let _ = l.register(count_cell(7, OP_METRIC, metrics(c7, &six).map(|m| m.1 as usize)));

    let _ = l.register(word_cell(8, OP_METRIC, Cfg::new(0, 0, 0, false, false).map(|c| c.width())));

    let _ = l.register(word_cell(9, OP_METRIC, Cfg::new(MAXW + 1, 0, 0, false, false).map(|c| c.width())));

    let _ = l.register(word_cell(10, OP_METRIC, Cfg::new(5, 5, 0, false, false).map(|c| c.width())));

    let c8 = match Cfg::new(8, 0, 0, false, false) {
        Ok(v) => v,
        Err(_) => return l,
    };
    let bad: [&[u8]; 1] = [b"ab\x80cd"];
    let _ = l.register(text_cell(11, OP_REFLOW, reflow(c8, &bad)));

    let c5 = match Cfg::new(5, 0, 0, false, false) {
        Ok(v) => v,
        Err(_) => return l,
    };
    let long: [&[u8]; 1] = [b"abcdefghijkl"];
    let _ = l.register(text_cell(12, OP_REFLOW, reflow(c5, &long)));

    let many: [&[u8]; CAP + 1] = [b"aa"; CAP + 1];
    let _ = l.register(text_cell(13, OP_REFLOW, reflow(c8, &many)));

    l
}

/// 四路注入。
pub fn inject_four() -> [bool; 4] {
    let mut out = [false; 4];

    let l = build_ledger();
    let rep = l.replay();
    let root = l.seal();
    out[0] = l.len() == SAMPLES && l.audit().is_ok() && rep == Ok(root) && root != 0;

    // 一路：基准重排。
    let c = match Cfg::new(10, 0, 0, false, false) {
        Ok(v) => v,
        Err(_) => return out,
    };
    let one: [&[u8]; 1] = [b"aaa bbb ccc"];
    let text = match reflow(c, &one) {
        Ok(v) => v,
        Err(_) => return out,
    };
    out[1] = text.as_bytes() == b"aaa bbb\nccc" && text.len() == 11;

    // 二路：缩进、uniform 与共享度量往返。
    let ci = match Cfg::new(12, 2, 1, false, false) {
        Ok(v) => v,
        Err(_) => return out,
    };
    let alpha: [&[u8]; 1] = [b"alpha beta gamma"];
    let indented = match reflow(ci, &alpha) {
        Ok(v) => v,
        Err(_) => return out,
    };
    let cu = match Cfg::new(20, 0, 0, true, false) {
        Ok(v) => v,
        Err(_) => return out,
    };
    let uni: [&[u8]; 1] = [b"hi. there"];
    let spaced = match reflow(cu, &uni) {
        Ok(v) => v,
        Err(_) => return out,
    };
    let m = metrics(ci, &alpha);
    out[2] = indented.as_bytes() == b"  alpha beta\n gamma"
        && spaced.as_bytes() == b"hi.  there"
        && m == Ok((2, 12))
        && word_count(&alpha) == Ok(3);

    // 三路：六类拒绝与冻结重放。
    let zero = matches!(Cfg::new(0, 0, 0, false, false), Err(FMT_EZERO));
    let wide = matches!(Cfg::new(MAXW + 1, 0, 0, false, false), Err(FMT_EWIDE));
    let indent = matches!(Cfg::new(5, 5, 0, false, false), Err(FMT_EINDENT));
    let mode = matches!(Cfg::new(8, 0, 0, true, true), Err(FMT_EMODE));
    let c8 = match Cfg::new(8, 0, 0, false, false) {
        Ok(v) => v,
        Err(_) => return out,
    };
    let bad: [&[u8]; 1] = [b"a\x01b"];
    let byte = reflow(c8, &bad) == Err(FMT_EBYTE);
    let c5 = match Cfg::new(5, 0, 0, false, false) {
        Ok(v) => v,
        Err(_) => return out,
    };
    let long: [&[u8]; 1] = [b"abcdefghijkl"];
    let overlong = reflow(c5, &long) == Err(FMT_ELONG);
    let many: [&[u8]; CAP + 1] = [b"aa"; CAP + 1];
    let full = reflow(c8, &many) == Err(FMT_EFULL);

    let mut led = build_ledger();
    let one_freeze = led.freeze();
    let rep = led.replay();
    let two_freeze = led.freeze();
    let reg = led.register(Sample::blank());
    out[3] = zero
        && wide
        && indent
        && mode
        && byte
        && overlong
        && full
        && one_freeze.is_ok()
        && rep == one_freeze
        && two_freeze == Err(FMT_EBAL)
        && reg == Err(FMT_ESLOT)
        && led.is_frozen()
        && led.rejects() == 2
        && led.replay() == Err(FMT_ESEAL);

    out
}

/// 十三项判据。
pub fn run_unx11534_checks() -> CheckSet {
    let mut s = CheckSet::new("unx11534.fmt_reflow");

    s.add(
        "FMT01-单段重排",
        {
            let c = match Cfg::new(10, 0, 0, false, false) {
                Ok(v) => v,
                Err(_) => return s,
            };
            let one: [&[u8]; 1] = [b"aaa bbb ccc"];
            match reflow(c, &one) {
                Ok(v) => v.as_bytes() == b"aaa bbb\nccc" && v.len() == 11,
                Err(_) => false,
            }
        },
        "static",
    );

    s.add(
        "FMT02-首续缩进",
        {
            let c = match Cfg::new(12, 2, 1, false, false) {
                Ok(v) => v,
                Err(_) => return s,
            };
            let one: [&[u8]; 1] = [b"alpha beta gamma"];
            match reflow(c, &one) {
                Ok(v) => v.as_bytes() == b"  alpha beta\n gamma",
                Err(_) => false,
            }
        },
        "static",
    );

    s.add(
        "FMT03-仅切分不合并",
        {
            let c = match Cfg::new(8, 0, 0, false, true) {
                Ok(v) => v,
                Err(_) => return s,
            };
            let two: [&[u8]; 2] = [b"aa bb", b"cc"];
            match reflow(c, &two) {
                Ok(v) => v.as_bytes() == b"aa bb\ncc" && v.len() == 8,
                Err(_) => false,
            }
        },
        "static",
    );

    s.add(
        "FMT04-空行分段",
        {
            let c = match Cfg::new(10, 0, 0, false, false) {
                Ok(v) => v,
                Err(_) => return s,
            };
            let three: [&[u8]; 3] = [b"aa", b"", b"bb"];
            match reflow(c, &three) {
                Ok(v) => v.as_bytes() == b"aa\nbb" && word_count(&three) == Ok(2),
                Err(_) => false,
            }
        },
        "static",
    );

    s.add(
        "FMT05-uniform 双空格",
        {
            let c = match Cfg::new(20, 0, 0, true, false) {
                Ok(v) => v,
                Err(_) => return s,
            };
            let one: [&[u8]; 1] = [b"hi. there"];
            let plain = match Cfg::new(20, 0, 0, false, false) {
                Ok(v) => v,
                Err(_) => return s,
            };
            match (reflow(c, &one), reflow(plain, &one)) {
                (Ok(a), Ok(b)) => a.as_bytes() == b"hi.  there" && b.as_bytes() == b"hi. there",
                _ => false,
            }
        },
        "static",
    );

    s.add(
        "FMT06-度量与词数",
        {
            let c = match Cfg::new(5, 0, 0, false, false) {
                Ok(v) => v,
                Err(_) => return s,
            };
            let one: [&[u8]; 1] = [b"aa bb cc"];
            metrics(c, &one) == Ok((2, 5)) && word_count(&one) == Ok(3)
        },
        "static",
    );

    s.add(
        "FMT07-单词超宽",
        {
            let c = match Cfg::new(5, 0, 0, false, false) {
                Ok(v) => v,
                Err(_) => return s,
            };
            let long: [&[u8]; 1] = [b"abcdefghijkl"];
            let exact: [&[u8]; 1] = [b"abcde"];
            reflow(c, &long) == Err(FMT_ELONG) && reflow(c, &exact).is_ok()
        },
        "static",
    );

    s.add(
        "FMT08-非法字节",
        {
            let c = match Cfg::new(8, 0, 0, false, false) {
                Ok(v) => v,
                Err(_) => return s,
            };
            let nul: [&[u8]; 1] = [b"a\x00b"];
            let high: [&[u8]; 1] = [b"a\x80b"];
            let tab: [&[u8]; 1] = [b"a\tb"];
            reflow(c, &nul) == Err(FMT_EBYTE)
                && reflow(c, &high) == Err(FMT_EBYTE)
                && reflow(c, &tab) == Err(FMT_EBYTE)
        },
        "static",
    );

    s.add(
        "FMT09-参数与模式闸门",
        {
            matches!(Cfg::new(0, 0, 0, false, false), Err(FMT_EZERO))
                && matches!(Cfg::new(MAXW + 1, 0, 0, false, false), Err(FMT_EWIDE))
                && matches!(Cfg::new(5, 5, 0, false, false), Err(FMT_EINDENT))
                && matches!(Cfg::new(5, 0, 5, false, false), Err(FMT_EINDENT))
                && matches!(Cfg::new(8, 0, 0, true, true), Err(FMT_EMODE))
        },
        "static",
    );

    s.add(
        "FMT10-容量与状态",
        {
            let c = match Cfg::new(8, 0, 0, false, false) {
                Ok(v) => v,
                Err(_) => return s,
            };
            let many: [&[u8]; CAP + 1] = [b"aa"; CAP + 1];
            let full = reflow(c, &many) == Err(FMT_EFULL);
            let two: [&[u8]; 2] = [b"aa bb", b"cc"];
            let mut f = match Fmt::open(c, &two) {
                Ok(v) => v,
                Err(_) => return s,
            };
            let first = f.scan() == Ok(8);
            let twice = f.scan() == Err(FMT_ESTATE);
            let mode = f.render(MODE_SPLIT) == Err(FMT_EMODE);
            let join = f.render(MODE_JOIN).is_ok();
            full && first && twice && mode && join && f.len() == 2 && !f.is_empty()
        },
        "static",
    );

    s.add(
        "FMT11-账本审计",
        {
            let l = build_ledger();
            l.len() == SAMPLES
                && l.ok_count() == 8
                && l.err_count() == 6
                && l.audit().is_ok()
                && l.rejects() == 0
                && l.sample(0).map(|x| x.ok) == Some(true)
                && l.sample(13).map(|x| x.ok) == Some(false)
        },
        "static",
    );

    s.add(
        "FMT12-冻结与重放",
        {
            let mut l = build_ledger();
            let a = l.freeze();
            let b = l.freeze();
            let c = l.register(Sample::blank());
            let sealed = l.seal();
            l.is_frozen()
                && a.is_ok()
                && b == Err(FMT_EBAL)
                && c == Err(FMT_ESLOT)
                && l.rejects() == 2
                && l.replay() == Err(FMT_ESEAL)
                && sealed != 0
        },
        "static",
    );

    s.add(
        "FMT13-冻结重放四注入",
        inject_four() == [true; 4] && build_ledger().audit().is_ok(),
        "static",
    );

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg(w: u32, f: u32, r: u32) -> Cfg {
        match Cfg::new(w, f, r, false, false) {
            Ok(v) => v,
            Err(_) => Cfg::new(4, 0, 0, false, false).ok().unwrap(),
        }
    }

    #[test]
    fn hard_reflow() {
        let c = cfg(10, 0, 0);
        let one: [&[u8]; 1] = [b"aaa bbb ccc"];
        match reflow(c, &one) {
            Ok(v) => assert!(v.as_bytes() == b"aaa bbb\nccc"),
            Err(_) => assert!(false),
        }
    }

    #[test]
    fn indents() {
        let c = cfg(12, 2, 1);
        let one: [&[u8]; 1] = [b"alpha beta gamma"];
        match reflow(c, &one) {
            Ok(v) => assert!(v.as_bytes() == b"  alpha beta\n gamma"),
            Err(_) => assert!(false),
        }
    }

    #[test]
    fn split_only() {
        let c = match Cfg::new(10, 0, 0, false, true) {
            Ok(v) => v,
            Err(_) => cfg(10, 0, 0),
        };
        let one: [&[u8]; 1] = [b"aa bb cc dd"];
        assert_eq!(reflow(c, &one).map(|v| v.len()), Ok(11));
    }

    #[test]
    fn uniform_spacing() {
        let c = match Cfg::new(20, 0, 0, true, false) {
            Ok(v) => v,
            Err(_) => cfg(20, 0, 0),
        };
        let one: [&[u8]; 1] = [b"hi. there"];
        match reflow(c, &one) {
            Ok(v) => assert!(v.as_bytes() == b"hi.  there"),
            Err(_) => assert!(false),
        }
    }

    #[test]
    fn gates() {
        assert_eq!(Cfg::new(0, 0, 0, false, false), Err(FMT_EZERO));
        assert_eq!(Cfg::new(MAXW + 1, 0, 0, false, false), Err(FMT_EWIDE));
        assert_eq!(Cfg::new(5, 5, 0, false, false), Err(FMT_EINDENT));
        assert_eq!(Cfg::new(8, 0, 0, true, true), Err(FMT_EMODE));
    }

    #[test]
    fn rejects() {
        let c = cfg(5, 0, 0);
        let long: [&[u8]; 1] = [b"abcdefghijkl"];
        let bad: [&[u8]; 1] = [b"a\x80b"];
        let many: [&[u8]; CAP + 1] = [b"aa"; CAP + 1];
        assert_eq!(reflow(c, &long), Err(FMT_ELONG));
        assert_eq!(reflow(c, &bad), Err(FMT_EBYTE));
        assert_eq!(reflow(c, &many), Err(FMT_EFULL));
    }

    #[test]
    fn ledger_cycle() {
        let mut l = build_ledger();
        assert_eq!(l.audit(), Ok(()));
        let r = match l.freeze() {
            Ok(v) => v,
            Err(_) => 0,
        };
        assert!(r != 0);
        assert_eq!(l.replay(), Ok(r));
        assert_eq!(l.freeze(), Err(FMT_EBAL));
        assert_eq!(l.register(Sample::blank()), Err(FMT_ESLOT));
        assert_eq!(l.rejects(), 2);
        assert_eq!(l.replay(), Err(FMT_ESEAL));
        assert_eq!(inject_four(), [true; 4]);
    }
}
