//! UNX-F11533 fold 折行
//!
//! `fold` 把过长的行按显示宽度截断并插入换行。有两种断点策略：默认在恰好达到宽度
//! 处硬断；`-s` 则在断点之前回退到最近的空格，避免把单词劈成两半。回退有边界：
//! 若断点前根本没有空格，或空格就落在行首（回退等于没断），仍然按硬断处理——否则
//! 会印出一张空行，或者陷入「断了但宽度没减少」的死循环。
//!
//! 宽度有两种计量：
//!
//! - 字符模式（默认）：制表符推进到 8 的倍数，与终端一致；
//! - 字节模式（`-b`）：每个字节记一列，制表符也只算一列。
//!
//! 单字节模型只对 ASCII 成立。多字节码点在这里会被拆成若干列，折行位置必然落在
//! 码点中间；与其产出一个非法 UTF-8 结果，不如在入口拒绝（`FO_EBYTE`）。
//!
//! 断点选择是「先算列、后搬字节」两步：先把每行的列推进算清楚，再决定断在哪，
//! 最后才拼输出。这样断点前的回退不需要在输出串里反查，列号也不会因为回退而漂移。
//!
//! 闸门次序：
//!
//! - `Cfg::new`：宽度为零 → 宽度超限；
//! - `fold_line`：行长超限 → 非法字节 → 输出满；
//! - `Fold::*_at`：行号越界 → 模式非法；
//! - `Fold::scan`：重复扫描。

use crate::alloc::string::String;
use crate::checks::CheckSet;

/// 段基址。
pub const SEG_BASE: u32 = 0xf260;

/// 宽度为零。
pub const FO_EINVAL: u32 = SEG_BASE + 0x01;
/// 宽度超限。
pub const FO_EWIDTH: u32 = SEG_BASE + 0x02;
/// 非法字节。
pub const FO_EBYTE: u32 = SEG_BASE + 0x03;
/// 行表或输出已满。
pub const FO_EFULL: u32 = SEG_BASE + 0x04;
/// 行号越界。
pub const FO_ELINE: u32 = SEG_BASE + 0x05;
/// 状态非法。
pub const FO_ESTATE: u32 = SEG_BASE + 0x06;
/// 模式非法。
pub const FO_EMODE: u32 = SEG_BASE + 0x07;
/// 停位参数冲突。
pub const FO_ESPACE: u32 = SEG_BASE + 0x08;
/// 账本形状不符。
pub const FO_EBAL: u32 = SEG_BASE + 0x09;
/// 槽位乱序。
pub const FO_ESLOT: u32 = SEG_BASE + 0x0a;
/// 观测值与复算不符。
pub const FO_EVALUE: u32 = SEG_BASE + 0x0b;
/// 冻结后重放不一致。
pub const FO_ESEAL: u32 = SEG_BASE + 0x0c;
/// 观察下标越界。
pub const FO_ENOBS: u32 = SEG_BASE + 0x0d;

/// 行表容量。
pub const CAP: usize = 16;
/// 单行源字节上限。
pub const LINEMAX: usize = 64;
/// 宽度上限。
pub const COLMAX: u32 = 120;
/// 制表步长。
pub const TAB: u32 = 8;
/// 账本格数。
pub const SAMPLES: usize = 14;

/// 模式：字符宽度。
pub const MODE_CHAR: u8 = 1;
/// 模式：字节宽度。
pub const MODE_BYTE: u8 = 2;

/// 操作码：折行。
pub const OP_FOLD: u8 = 1;
/// 操作码：断点数。
pub const OP_BREAK: u8 = 2;
/// 操作码：列宽。
pub const OP_WIDTH: u8 = 3;
/// 操作码：统计。
pub const OP_STAT: u8 = 4;

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

/// 制表配置。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Cfg {
    width: u32,
    bytes: bool,
    spaces: bool,
}

impl Cfg {
    /// 构造。
    pub fn new(width: u32, bytes: bool, spaces: bool) -> Result<Cfg, u32> {
        if width == 0 {
            return Err(FO_EINVAL);
        }
        if width > COLMAX {
            return Err(FO_EWIDTH);
        }
        Ok(Cfg { width, bytes, spaces })
    }

    /// 宽度。
    pub fn width(&self) -> u32 {
        self.width
    }

    /// 是否按字节计宽。
    pub fn bytes_mode(&self) -> bool {
        self.bytes
    }

    /// 是否在空格处回退。
    pub fn spaces_break(&self) -> bool {
        self.spaces
    }

    /// 模式码。
    pub fn mode(&self) -> u8 {
        if self.bytes {
            MODE_BYTE
        } else {
            MODE_CHAR
        }
    }

    /// 单字节的列推进。
    pub fn advance(&self, col: u32, b: u8) -> u32 {
        if b == 0x09 && !self.bytes {
            ((col / TAB) + 1).saturating_mul(TAB)
        } else {
            col.saturating_add(1)
        }
    }

    /// 校验源字节。
    pub fn check_bytes(&self, line: &[u8]) -> Result<(), u32> {
        let mut i = 0usize;
        while i < line.len() {
            match line.get(i) {
                Some(0x09) | Some(0x20..=0x7e) => {}
                Some(_) => return Err(FO_EBYTE),
                None => break,
            }
            i += 1;
        }
        Ok(())
    }

    /// 折一行。
    pub fn fold_line(&self, line: &[u8]) -> Result<String, u32> {
        if line.len() > LINEMAX {
            return Err(FO_EFULL);
        }
        self.check_bytes(line)?;
        let mut out = String::new();
        let mut start = 0usize;
        let mut col = 0u32;
        let mut j = 0usize;
        while j < line.len() {
            let b = match line.get(j) {
                Some(v) => *v,
                None => break,
            };
            let next = self.advance(col, b);
            if next > self.width {
                let mut brk = j;
                if self.spaces {
                    let mut k = j;
                    while k > start {
                        k -= 1;
                        if let Some(0x20) = line.get(k) {
                            brk = k;
                            break;
                        }
                    }
                    if brk <= start {
                        brk = j;
                    }
                }
                if brk > start {
                    let mut t = start;
                    while t < brk {
                        if let Some(v) = line.get(t) {
                            out.push(*v as char);
                        }
                        t += 1;
                    }
                    out.push('\n');
                    start = if brk < j && matches!(line.get(brk), Some(0x20)) {
                        brk + 1
                    } else {
                        brk
                    };
                    col = 0;
                    let mut u = start;
                    while u < j {
                        if let Some(v) = line.get(u) {
                            col = self.advance(col, *v);
                        }
                        u += 1;
                    }
                    if out.len() > LINEMAX {
                        return Err(FO_EFULL);
                    }
                    continue;
                }
            }
            col = next;
            j += 1;
        }
        let mut t = start;
        while t < line.len() {
            if let Some(v) = line.get(t) {
                out.push(*v as char);
            }
            t += 1;
        }
        if out.len() > LINEMAX {
            return Err(FO_EFULL);
        }
        Ok(out)
    }

    /// 折行后插入的换行个数。
    pub fn breaks(&self, line: &[u8]) -> Result<u32, u32> {
        let folded = self.fold_line(line)?;
        let mut n = 0u32;
        let mut i = 0usize;
        while i < folded.len() {
            if let Some(0x0a) = folded.as_bytes().get(i) {
                n = n.saturating_add(1);
            }
            i += 1;
        }
        Ok(n)
    }

    /// 行宽。
    pub fn width_of(&self, line: &[u8]) -> Result<u32, u32> {
        if line.len() > LINEMAX {
            return Err(FO_EFULL);
        }
        self.check_bytes(line)?;
        let mut col = 0u32;
        let mut i = 0usize;
        while i < line.len() {
            if let Some(v) = line.get(i) {
                col = self.advance(col, *v);
            }
            i += 1;
        }
        Ok(col)
    }
}

/// 行表。
#[derive(Clone, Copy, Debug)]
pub struct Fold<'a> {
    cfg: Cfg,
    lines: &'a [&'a [u8]],
    scanned: bool,
    wraps: u32,
}

impl<'a> Fold<'a> {
    /// 开户。
    pub fn open(cfg: Cfg, lines: &'a [&'a [u8]]) -> Result<Fold<'a>, u32> {
        if lines.len() > CAP {
            return Err(FO_EFULL);
        }
        Ok(Fold { cfg, lines, scanned: false, wraps: 0 })
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

    /// 累计断点数。
    pub fn wraps(&self) -> u32 {
        self.wraps
    }

    /// 扫描全部行。
    pub fn scan(&mut self) -> Result<u32, u32> {
        if self.scanned {
            return Err(FO_ESTATE);
        }
        let mut sum = 0u32;
        let mut i = 0usize;
        while i < self.lines.len() {
            let line = match self.lines.get(i) {
                Some(v) => *v,
                None => return Err(FO_ELINE),
            };
            sum = sum.saturating_add(self.cfg.breaks(line)?);
            i += 1;
        }
        self.wraps = sum;
        self.scanned = true;
        Ok(sum)
    }

    /// 第 i 行源字节。
    pub fn line(&self, i: usize) -> Result<&'a [u8], u32> {
        if i >= self.lines.len() {
            return Err(FO_ELINE);
        }
        match self.lines.get(i) {
            Some(v) => Ok(*v),
            None => Err(FO_ELINE),
        }
    }

    /// 第 i 行折行。
    pub fn fold_at(&self, i: usize) -> Result<String, u32> {
        self.cfg.fold_line(self.line(i)?)
    }

    /// 第 i 行断点数。
    pub fn breaks_at(&self, i: usize) -> Result<u32, u32> {
        self.cfg.breaks(self.line(i)?)
    }

    /// 第 i 行宽度。
    pub fn width_at(&self, i: usize) -> Result<u32, u32> {
        self.cfg.width_of(self.line(i)?)
    }

    /// 全表折行后以换行拼接。
    pub fn render(&self) -> Result<String, u32> {
        let mut out = String::new();
        let mut i = 0usize;
        while i < self.lines.len() {
            let piece = self.fold_at(i)?;
            out.push_str(&piece);
            if i + 1 < self.lines.len() {
                out.push('\n');
            }
            i += 1;
        }
        Ok(out)
    }

    /// 按模式取整表（字符/字节）。
    pub fn run(&self, mode: u8) -> Result<String, u32> {
        if mode != self.cfg.mode() {
            return Err(FO_EMODE);
        }
        self.render()
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

/// `Result<String, u32>` → 样本。
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
            return Err(FO_ESLOT);
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
            return Err(FO_EBAL);
        }
        const VALS: [u32; 8] = [12, 9, 8, 3, 4, 3, 7, 8];
        const CODES: [u32; 6] =
            [FO_EINVAL, FO_EBYTE, FO_EFULL, FO_ELINE, FO_EMODE, FO_ESTATE];
        let mut vi = 0usize;
        let mut ci = 0usize;
        let mut i = 0usize;
        while i < self.len {
            let s = match self.rows.get(i) {
                Some(v) => *v,
                None => return Err(FO_EBAL),
            };
            if s.slot as usize != i {
                return Err(FO_ESLOT);
            }
            if s.ok {
                if s.code != 0 {
                    return Err(FO_EVALUE);
                }
                match VALS.get(vi) {
                    Some(w) => {
                        if s.value != *w {
                            return Err(FO_EVALUE);
                        }
                    }
                    None => return Err(FO_EBAL),
                }
                vi += 1;
            } else {
                match CODES.get(ci) {
                    Some(w) => {
                        if s.code != *w {
                            return Err(FO_EVALUE);
                        }
                    }
                    None => return Err(FO_EBAL),
                }
                ci += 1;
            }
            i += 1;
        }
        if vi != 8 || ci != 6 {
            return Err(FO_EBAL);
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
            return Err(FO_EBAL);
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
            Err(FO_ESEAL)
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

/// 十四格账本：硬断、空格回退、制表推进与六类拒绝。
pub fn build_ledger() -> Ledger {
    let mut l = Ledger::new();

    let hard = match Cfg::new(4, false, false) {
        Ok(v) => v,
        Err(_) => return l,
    };
    let _ = l.register(text_cell(0, OP_FOLD, hard.fold_line(b"abcdefghij")));

    let gap = match Cfg::new(4, false, false) {
        Ok(v) => v,
        Err(_) => return l,
    };
    let _ = l.register(text_cell(1, OP_FOLD, gap.fold_line(b"ab cd ef")));

    let soft = match Cfg::new(4, false, true) {
        Ok(v) => v,
        Err(_) => return l,
    };
    let _ = l.register(text_cell(2, OP_FOLD, soft.fold_line(b"ab cd ef")));

    let bytew = match Cfg::new(4, true, false) {
        Ok(v) => v,
        Err(_) => return l,
    };
    let _ = l.register(text_cell(3, OP_FOLD, bytew.fold_line(b"\tab")));

    let eight = match Cfg::new(8, false, false) {
        Ok(v) => v,
        Err(_) => return l,
    };
    let _ = l.register(text_cell(4, OP_FOLD, eight.fold_line(b"\tab")));
    let _ = l.register(text_cell(5, OP_FOLD, eight.fold_line(b"abc")));

    let two = match Cfg::new(2, true, false) {
        Ok(v) => v,
        Err(_) => return l,
    };
    let _ = l.register(text_cell(6, OP_FOLD, two.fold_line(b"abcde")));

    let four = match Cfg::new(4, false, false) {
        Ok(v) => v,
        Err(_) => return l,
    };
    let lines: [&[u8]; 2] = [b"abcd", b"efgh"];
    let _ = l.register(word_cell(7, OP_WIDTH, hard.width_of(b"abcdefgh")));

    let _ = l.register(word_cell(8, OP_STAT, Cfg::new(0, false, false).map(|c| c.width())));

    let bad = match Cfg::new(8, false, false) {
        Ok(v) => v,
        Err(_) => return l,
    };
    let _ = l.register(text_cell(9, OP_FOLD, bad.fold_line(b"a\x80b")));

    let long = [b'a'; LINEMAX + 1];
    let _ = l.register(text_cell(10, OP_FOLD, bad.fold_line(&long)));

    let probe = match Fold::open(four, &lines) {
        Ok(v) => v,
        Err(_) => return l,
    };
    let _ = l.register(text_cell(11, OP_FOLD, probe.fold_at(9)));
    let _ = l.register(text_cell(12, OP_FOLD, probe.run(9)));

    let mut twice = match Fold::open(four, &lines) {
        Ok(v) => v,
        Err(_) => return l,
    };
    let _ = twice.scan();
    let _ = l.register(word_cell(13, OP_STAT, twice.scan()));

    l
}

/// 四路注入。
pub fn inject_four() -> [bool; 4] {
    let mut out = [false; 4];

    let l = build_ledger();
    let rep = l.replay();
    let root = l.seal();
    out[0] = l.len() == SAMPLES && l.audit().is_ok() && rep == Ok(root) && root != 0;

    // 一路：硬断与列宽。
    let hard = match Cfg::new(4, false, false) {
        Ok(v) => v,
        Err(_) => return out,
    };
    let body = hard.fold_line(b"abcdefghij") == Ok(String::from("abcd\nefgh\nij"));
    let width = hard.width_of(b"abcdefgh") == Ok(8) && hard.width_of(b"abc") == Ok(3);
    let breaks = hard.breaks(b"abcdefghij") == Ok(2) && hard.breaks(b"abc") == Ok(0);
    out[1] = body && width && breaks;

    // 二路：空格回退与制表推进。
    let soft = match Cfg::new(4, false, true) {
        Ok(v) => v,
        Err(_) => return out,
    };
    let word = soft.fold_line(b"ab cd ef") == Ok(String::from("ab\ncd\nef"));
    let no_gap = soft.fold_line(b"abcdefgh") == Ok(String::from("abcd\nefgh"));

    let eight = match Cfg::new(8, false, false) {
        Ok(v) => v,
        Err(_) => return out,
    };
    let tab_char = eight.fold_line(b"\tab") == Ok(String::from("\t\nab"));
    let bytew = match Cfg::new(8, true, false) {
        Ok(v) => v,
        Err(_) => return out,
    };
    let tab_byte = bytew.fold_line(b"\tab") == Ok(String::from("\tab"));
    out[2] = word && no_gap && tab_char && tab_byte;

    // 三路：拒绝、行表与冻结重放。
    let ring = Cfg::new(4, false, false).ok();
    let zero = matches!(Cfg::new(0, false, false), Err(FO_EINVAL));
    let wide = matches!(Cfg::new(COLMAX + 1, false, false), Err(FO_EWIDTH));
    let c8 = match Cfg::new(8, false, false) {
        Ok(v) => v,
        Err(_) => return out,
    };
    let byte = c8.fold_line(b"\x00") == Err(FO_EBYTE) && c8.fold_line(b"\xff") == Err(FO_EBYTE);
    let long = [b'a'; LINEMAX + 1];
    let full = c8.fold_line(&long) == Err(FO_EFULL);

    let c4 = match Cfg::new(4, false, false) {
        Ok(v) => v,
        Err(_) => return out,
    };
    let lines: [&[u8]; 3] = [b"abcdefgh", b"abcdefgh", b"abcdefgh"];
    let mut fo = match Fold::open(c4, &lines) {
        Ok(v) => v,
        Err(_) => return out,
    };
    let line = fo.fold_at(9) == Err(FO_ELINE);
    let mode = fo.run(9) == Err(FO_EMODE) && fo.run(MODE_CHAR).is_ok();
    let state = fo.scan() == Ok(3) && fo.scan() == Err(FO_ESTATE) && fo.wraps() == 3;
    let empty = Fold::open(c8, &lines[..0]).map(|f| f.is_empty()) == Ok(true);

    let mut led = build_ledger();
    let one = led.freeze();
    let rep = led.replay();
    let two = led.freeze();
    let reg = led.register(Sample::blank());
    out[3] = zero
        && wide
        && byte
        && full
        && line
        && mode
        && state
        && empty
        && ring.is_some()
        && one.is_ok()
        && rep == one
        && two.is_err()
        && reg == Err(FO_ESLOT)
        && led.is_frozen()
        && led.rejects() == 2;

    out
}

/// 十三项判据。
pub fn run_unx11533_checks() -> CheckSet {
    let mut s = CheckSet::new("unx11533.fold_wrap");

    s.add(
        "FO01-硬断",
        {
            let c = match Cfg::new(4, false, false) {
                Ok(v) => v,
                Err(_) => return s,
            };
            c.fold_line(b"abcdefghij") == Ok(String::from("abcd\nefgh\nij"))
                && c.fold_line(b"abcd") == Ok(String::from("abcd"))
                && c.fold_line(b"") == Ok(String::from(""))
        },
        "static",
    );

    s.add(
        "FO02-空格回退",
        {
            let c = match Cfg::new(4, false, true) {
                Ok(v) => v,
                Err(_) => return s,
            };
            c.fold_line(b"ab cd ef") == Ok(String::from("ab\ncd\nef"))
                && c.fold_line(b"abcdefgh") == Ok(String::from("abcd\nefgh"))
        },
        "static",
    );

    s.add(
        "FO03-行首空格不空行",
        {
            let c = match Cfg::new(4, false, true) {
                Ok(v) => v,
                Err(_) => return s,
            };
            c.fold_line(b" abcd") == Ok(String::from(" abc\nd"))
        },
        "static",
    );

    s.add(
        "FO04-制表推进",
        {
            let c = match Cfg::new(8, false, false) {
                Ok(v) => v,
                Err(_) => return s,
            };
            let b = match Cfg::new(8, true, false) {
                Ok(v) => v,
                Err(_) => return s,
            };
            c.fold_line(b"\tab") == Ok(String::from("\t\nab"))
                && b.fold_line(b"\tab") == Ok(String::from("\tab"))
                && c.width_of(b"\tab") == Ok(10)
                && b.width_of(b"\tab") == Ok(3)
        },
        "static",
    );

    s.add(
        "FO05-列宽与断点",
        {
            let c = match Cfg::new(4, false, false) {
                Ok(v) => v,
                Err(_) => return s,
            };
            c.width_of(b"abcdefgh") == Ok(8)
                && c.breaks(b"abcdefghij") == Ok(2)
                && c.breaks(b"abc") == Ok(0)
                && c.breaks(b"abcdefgh") == Ok(1)
        },
        "static",
    );

    s.add(
        "FO06-行表渲染",
        {
            let c = match Cfg::new(4, false, false) {
                Ok(v) => v,
                Err(_) => return s,
            };
            let lines: [&[u8]; 3] = [b"abcd", b"efgh", b"ijkl"];
            let fo = match Fold::open(c, &lines) {
                Ok(v) => v,
                Err(_) => return s,
            };
            fo.len() == 3
                && !fo.is_empty()
                && fo.fold_at(0) == Ok(String::from("abcd"))
                && fo.render() == Ok(String::from("abcd\nefgh\nijkl"))
        },
        "static",
    );

    s.add(
        "FO07-容量上限",
        {
            let c = match Cfg::new(8, false, false) {
                Ok(v) => v,
                Err(_) => return s,
            };
            let long = [b'a'; LINEMAX + 1];
            let tabs = [b'\t'; LINEMAX];
            c.fold_line(&long) == Err(FO_EFULL)
                && c.fold_line(&tabs).is_err()
                && matches!(Cfg::new(COLMAX + 1, false, false), Err(FO_EWIDTH))
        },
        "static",
    );

    s.add(
        "FO08-非法字节拒绝",
        {
            let c = match Cfg::new(8, false, false) {
                Ok(v) => v,
                Err(_) => return s,
            };
            c.fold_line(b"\x00") == Err(FO_EBYTE)
                && c.fold_line(b"\x1f") == Err(FO_EBYTE)
                && c.fold_line(b"\x7f") == Err(FO_EBYTE)
                && c.fold_line(b"\xc3\xa9") == Err(FO_EBYTE)
                && c.width_of(b"\x80") == Err(FO_EBYTE)
        },
        "static",
    );

    s.add(
        "FO09-参数闸门",
        {
            matches!(Cfg::new(0, false, false), Err(FO_EINVAL))
                && matches!(Cfg::new(0, true, true), Err(FO_EINVAL))
                && Cfg::new(COLMAX, false, false).is_ok()
                && matches!(Cfg::new(COLMAX + 1, false, false), Err(FO_EWIDTH))
        },
        "static",
    );

    s.add(
        "FO10-状态与模式闸门",
        {
            let c = match Cfg::new(4, false, false) {
                Ok(v) => v,
                Err(_) => return s,
            };
            let lines: [&[u8]; 2] = [b"abcdefgh", b"abcd"];
            let mut fo = match Fold::open(c, &lines) {
                Ok(v) => v,
                Err(_) => return s,
            };
            let first = fo.scan() == Ok(1);
            let twice = fo.scan() == Err(FO_ESTATE);
            let line = fo.fold_at(4) == Err(FO_ELINE);
            let mode = fo.run(MODE_BYTE) == Err(FO_EMODE);
            first && twice && line && mode && fo.wraps() == 1 && fo.scanned()
        },
        "static",
    );

    s.add(
        "FO11-账本审计",
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
        "FO12-冻结与重放",
        {
            let mut l = build_ledger();
            let a = l.freeze();
            let b = l.freeze();
            let c = l.register(Sample::blank());
            let sealed = l.seal();
            l.is_frozen()
                && a.is_ok()
                && b == Err(FO_EBAL)
                && c == Err(FO_ESLOT)
                && l.rejects() == 2
                && l.replay() == Err(FO_ESEAL)
                && sealed != 0
        },
        "static",
    );

    s.add(
        "FO13-冻结重放四注入",
        inject_four() == [true; 4] && build_ledger().audit().is_ok(),
        "static",
    );

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hard_wrap() {
        let c = Cfg::new(4, false, false).ok().unwrap();
        assert_eq!(c.fold_line(b"abcdefghij"), Ok(String::from("abcd\nefgh\nij")));
        assert_eq!(c.fold_line(b"abcd"), Ok(String::from("abcd")));
        assert_eq!(c.breaks(b"abcdefghij"), Ok(2));
    }

    #[test]
    fn soft_wrap() {
        let c = Cfg::new(4, false, true).ok().unwrap();
        assert_eq!(c.fold_line(b"ab cd ef"), Ok(String::from("ab\ncd\nef")));
        assert_eq!(c.fold_line(b"abcdefgh"), Ok(String::from("abcd\nefgh")));
        assert_eq!(c.fold_line(b" abcd"), Ok(String::from(" abc\nd")));
    }

    #[test]
    fn tab_columns() {
        let c = Cfg::new(8, false, false).ok().unwrap();
        let b = Cfg::new(8, true, false).ok().unwrap();
        assert_eq!(c.width_of(b"\tab"), Ok(10));
        assert_eq!(b.width_of(b"\tab"), Ok(3));
        assert_eq!(c.fold_line(b"\tab"), Ok(String::from("\t\nab")));
        assert_eq!(b.fold_line(b"\tab"), Ok(String::from("\tab")));
    }

    #[test]
    fn rejections() {
        assert_eq!(Cfg::new(0, false, false), Err(FO_EINVAL));
        assert_eq!(Cfg::new(COLMAX + 1, false, false), Err(FO_EWIDTH));
        let c = Cfg::new(8, false, false).ok().unwrap();
        assert_eq!(c.fold_line(b"\xff"), Err(FO_EBYTE));
        let long = [b'a'; LINEMAX + 1];
        assert_eq!(c.fold_line(&long), Err(FO_EFULL));
    }

    #[test]
    fn table_gates() {
        let c = Cfg::new(4, false, false).ok().unwrap();
        let lines: [&[u8]; 2] = [b"abcdefgh", b"abcd"];
        let mut fo = Fold::open(c, &lines).ok().unwrap();
        assert_eq!(fo.scan(), Ok(1));
        assert_eq!(fo.scan(), Err(FO_ESTATE));
        assert_eq!(fo.fold_at(4), Err(FO_ELINE));
        assert_eq!(fo.run(MODE_BYTE), Err(FO_EMODE));
        assert_eq!(fo.render(), Ok(String::from("abcd\nefgh\nabcd")));
    }

    #[test]
    fn ledger_seal() {
        let mut l = build_ledger();
        assert_eq!(l.len(), SAMPLES);
        assert_eq!(l.audit(), Ok(()));
        let r = l.freeze().ok().unwrap();
        assert_eq!(l.root(), r);
        assert_eq!(l.replay(), Ok(r));
        assert_eq!(l.freeze(), Err(FO_EBAL));
        assert_eq!(l.register(Sample::blank()), Err(FO_ESLOT));
        assert_eq!(l.rejects(), 2);
        assert_eq!(l.replay(), Err(FO_ESEAL));
    }

    #[test]
    fn four_routes() {
        assert_eq!(inject_four(), [true; 4]);
    }

}
