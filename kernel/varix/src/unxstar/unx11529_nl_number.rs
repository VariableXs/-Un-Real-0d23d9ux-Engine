//! UNX-F11529 行编号（-b/-n/-d）（0xf220 段）
//!
//! `nl` 的编号不是「第几行就打几」，它由三张正交的开关决定：
//!
//! - **要不要编**（`-b`）：`a` 全部、`t` 仅非空行、`n` 一律不编；
//! - **怎么排版**（`-n`）：`ln` 左对齐、`rn` 右对齐补空格、`rz` 右对齐补零；
//! - **段怎么切**（`-d`）：`\:\:\:` 开新页（计数回到 `-v`）、`\:\:` 回正文、
//!   `\:` 进页脚（不编号）。
//!
//! 三者互不耦合，于是一个「宽度 3、补零、从 7 起、步长 2、仅非空行」的组合
//! 可以独立验证，不必与全文混在一起。分隔符行本身按空行输出，且只有**开新页**
//! 会重置计数器 —— 页脚与正文之间来回切不会回卷，这是本模块刻意保留的语义。
//!
//! 计数器只在**真正打出一个号**时前进；不编号的行输出等宽空白字段，保证所有
//! 行的正文列严格对齐。号码超过宽度时不截断（宁可撑破列，也不丢信息）。
//!
//! 闸门次序：
//!
//! - `Cfg::new`：风格 → 格式 → 宽度 → 步长 → 分隔符；
//! - `Nl::open`：行数上限 → 单行长度；
//! - `run`：正文非 UTF-8。

use crate::alloc::string::String;
use crate::checks::CheckSet;

/// 段基址。
pub const SEG_BASE: u32 = 0xf220;

/// 参数非法（单行过长）。
pub const NL_EINVAL: u32 = SEG_BASE + 0x01;
/// 风格非法。
pub const NL_ESTYLE: u32 = SEG_BASE + 0x02;
/// 号码格式非法。
pub const NL_EFMT: u32 = SEG_BASE + 0x03;
/// 宽度非法。
pub const NL_EWIDTH: u32 = SEG_BASE + 0x04;
/// 步长非法。
pub const NL_ESTEP: u32 = SEG_BASE + 0x05;
/// 分隔符非法。
pub const NL_ESEP: u32 = SEG_BASE + 0x06;
/// 行数超限。
pub const NL_ELINE: u32 = SEG_BASE + 0x07;
/// 正文非 UTF-8。
pub const NL_EBYTE: u32 = SEG_BASE + 0x08;
/// 账本形状不符。
pub const NL_EBAL: u32 = SEG_BASE + 0x09;
/// 槽位乱序。
pub const NL_ESLOT: u32 = SEG_BASE + 0x0a;
/// 观测值与复算不符。
pub const NL_EVALUE: u32 = SEG_BASE + 0x0b;
/// 冻结后重放不一致。
pub const NL_ESEAL: u32 = SEG_BASE + 0x0c;
/// 观察下标越界。
pub const NL_ENOBS: u32 = SEG_BASE + 0x0d;

/// 行数上限。
pub const CAP: usize = 16;
/// 号码宽度上限。
pub const WMAX: u32 = 12;
/// 分隔符缓冲。
pub const SEPMAX: usize = 4;
/// 单行长度上限。
pub const LINEMAX: usize = 72;
/// 账本格数。
pub const SAMPLES: usize = 14;

/// `-b a`：全部行。
pub const STYLE_ALL: u8 = 1;
/// `-b t`：仅非空行。
pub const STYLE_TEXT: u8 = 2;
/// `-b n`：不编号。
pub const STYLE_NONE: u8 = 3;

/// `-n ln`：左对齐。
pub const FMT_LEFT: u8 = 1;
/// `-n rn`：右对齐补空格。
pub const FMT_RIGHT: u8 = 2;
/// `-n rz`：右对齐补零。
pub const FMT_ZERO: u8 = 3;

/// 页眉分隔线。
pub const DELIM_HEAD: &[u8] = b"\\:\\:\\:";
/// 正文分隔线。
pub const DELIM_BODY: &[u8] = b"\\:\\:";
/// 页脚分隔线。
pub const DELIM_FOOT: &[u8] = b"\\:";

/// 操作码：渲染。
pub const OP_RENDER: u8 = 1;
/// 操作码：计数。
pub const OP_COUNT: u8 = 2;
/// 操作码：参数。
pub const OP_CFG: u8 = 3;
/// 操作码：观测。
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

/// 十进制数字（不分配），返回 (缓冲, 位数)。
fn digits_of(mut n: u32) -> ([u8; 10], usize) {
    let mut buf = [0u8; 10];
    let mut len = 0usize;
    if n == 0 {
        if let Some(dst) = buf.get_mut(0) {
            *dst = b'0';
        }
        return (buf, 1);
    }
    let mut rev = [0u8; 10];
    while n > 0 && len < 10 {
        if let Some(dst) = rev.get_mut(len) {
            *dst = b'0' + (n % 10) as u8;
        }
        n /= 10;
        len += 1;
    }
    let mut i = 0usize;
    while i < len {
        if let (Some(dst), Some(src)) = (buf.get_mut(i), rev.get(len - 1 - i)) {
            *dst = *src;
        }
        i += 1;
    }
    (buf, len)
}

/// 号码字段。
pub fn number_field(fmt: u8, width: u32, n: u32) -> Result<String, u32> {
    if fmt != FMT_LEFT && fmt != FMT_RIGHT && fmt != FMT_ZERO {
        return Err(NL_EFMT);
    }
    let (buf, len) = digits_of(n);
    let w = width as usize;
    let pad = if w > len { w - len } else { 0 };
    let mut out = String::new();
    if fmt == FMT_LEFT {
        let mut i = 0usize;
        while i < len {
            if let Some(b) = buf.get(i) {
                out.push(*b as char);
            }
            i += 1;
        }
        let mut i = 0usize;
        while i < pad {
            out.push(' ');
            i += 1;
        }
    } else {
        let fill = if fmt == FMT_ZERO { '0' } else { ' ' };
        let mut i = 0usize;
        while i < pad {
            out.push(fill);
            i += 1;
        }
        let mut i = 0usize;
        while i < len {
            if let Some(b) = buf.get(i) {
                out.push(*b as char);
            }
            i += 1;
        }
    }
    Ok(out)
}

/// 编号配置。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Cfg {
    style: u8,
    fmt: u8,
    width: u32,
    start: u32,
    step: u32,
    sep: [u8; SEPMAX],
    slen: u8,
}

impl Cfg {
    /// 建立配置：风格 → 格式 → 宽度 → 步长 → 分隔符。
    pub fn new(
        style: u8,
        fmt: u8,
        width: u32,
        start: u32,
        step: u32,
        sep: &[u8],
    ) -> Result<Cfg, u32> {
        if style != STYLE_ALL && style != STYLE_TEXT && style != STYLE_NONE {
            return Err(NL_ESTYLE);
        }
        if fmt != FMT_LEFT && fmt != FMT_RIGHT && fmt != FMT_ZERO {
            return Err(NL_EFMT);
        }
        if width == 0 || width > WMAX {
            return Err(NL_EWIDTH);
        }
        if step == 0 {
            return Err(NL_ESTEP);
        }
        if sep.is_empty() || sep.len() > SEPMAX {
            return Err(NL_ESEP);
        }
        let mut i = 0usize;
        while i < sep.len() {
            match sep.get(i) {
                Some(b) => {
                    if *b == b'\n' {
                        return Err(NL_ESEP);
                    }
                }
                None => return Err(NL_ESEP),
            }
            i += 1;
        }
        if core::str::from_utf8(sep).is_err() {
            return Err(NL_ESEP);
        }
        let mut c = Cfg {
            style,
            fmt,
            width,
            start,
            step,
            sep: [0; SEPMAX],
            slen: sep.len() as u8,
        };
        i = 0;
        while i < sep.len() {
            if let (Some(dst), Some(src)) = (c.sep.get_mut(i), sep.get(i)) {
                *dst = *src;
            }
            i += 1;
        }
        Ok(c)
    }

    /// 默认配置（`-b a -n rn -w 6 -v 1 -i 1 -s TAB`）。
    pub fn default_cfg() -> Cfg {
        match Cfg::new(STYLE_ALL, FMT_RIGHT, 6, 1, 1, b"\t") {
            Ok(v) => v,
            Err(_) => Cfg {
                style: STYLE_ALL,
                fmt: FMT_RIGHT,
                width: 6,
                start: 1,
                step: 1,
                sep: [b'\t', 0, 0, 0],
                slen: 1,
            },
        }
    }

    /// 风格。
    pub fn style(&self) -> u8 {
        self.style
    }

    /// 格式。
    pub fn fmt(&self) -> u8 {
        self.fmt
    }

    /// 宽度。
    pub fn width(&self) -> u32 {
        self.width
    }

    /// 起始号。
    pub fn start(&self) -> u32 {
        self.start
    }

    /// 步长。
    pub fn step(&self) -> u32 {
        self.step
    }

    /// 分隔符。
    pub fn sep(&self) -> &[u8] {
        self.sep.get(..self.slen as usize).unwrap_or(&[])
    }

    /// 正文行是否编号。
    fn numbers(&self, text: &[u8]) -> bool {
        match self.style {
            STYLE_ALL => true,
            STYLE_TEXT => has_text(text),
            _ => false,
        }
    }

    /// 号码字段或等宽空白。
    fn field(&self, n: u32) -> Result<String, u32> {
        number_field(self.fmt, self.width, n)
    }

    /// 非编号行的空白字段。
    fn blank_field(&self) -> String {
        let mut out = String::new();
        let mut i = 0u32;
        while i < self.width {
            out.push(' ');
            i += 1;
        }
        out
    }
}

/// 行内是否有非空白字符。
pub fn has_text(line: &[u8]) -> bool {
    let mut i = 0usize;
    while i < line.len() {
        match line.get(i) {
            Some(b) => {
                if *b != b' ' && *b != b'\t' {
                    return true;
                }
            }
            None => break,
        }
        i += 1;
    }
    false
}

/// 是否为分隔线。
pub fn delim_kind(line: &[u8]) -> u8 {
    if line == DELIM_HEAD {
        3
    } else if line == DELIM_BODY {
        2
    } else if line == DELIM_FOOT {
        1
    } else {
        0
    }
}

/// 编号器。
#[derive(Clone, Copy, Debug)]
pub struct Nl<'a> {
    cfg: Cfg,
    lines: &'a [&'a [u8]],
}

impl<'a> Nl<'a> {
    /// 开户：行数上限 → 单行长度。
    pub fn open(cfg: Cfg, lines: &'a [&'a [u8]]) -> Result<Nl<'a>, u32> {
        if lines.len() > CAP {
            return Err(NL_ELINE);
        }
        let mut i = 0usize;
        while i < lines.len() {
            match lines.get(i) {
                Some(l) => {
                    if l.len() > LINEMAX {
                        return Err(NL_EINVAL);
                    }
                }
                None => return Err(NL_EINVAL),
            }
            i += 1;
        }
        Ok(Nl { cfg, lines })
    }

    /// 配置。
    pub fn cfg(&self) -> Cfg {
        self.cfg
    }

    /// 行数。
    pub fn len(&self) -> usize {
        self.lines.len()
    }

    /// 是否空输入。
    pub fn is_empty(&self) -> bool {
        self.lines.is_empty()
    }

    /// 逐行渲染（每行以 `\n` 结尾）。
    pub fn run(&self) -> Result<String, u32> {
        let mut out = String::new();
        let mut next = self.cfg.start();
        let mut body = true;
        let mut i = 0usize;
        while i < self.lines.len() {
            let line = match self.lines.get(i) {
                Some(v) => *v,
                None => break,
            };
            let kind = delim_kind(line);
            if kind != 0 {
                if kind == 3 {
                    next = self.cfg.start();
                    body = true;
                } else if kind == 2 {
                    body = true;
                } else {
                    body = false;
                }
                out.push('\n');
                i += 1;
                continue;
            }
            if body && self.cfg.numbers(line) {
                let f = self.cfg.field(next)?;
                out.push_str(f.as_str());
                let sep = match core::str::from_utf8(self.cfg.sep()) {
                    Ok(v) => v,
                    Err(_) => return Err(NL_ESEP),
                };
                out.push_str(sep);
                let text = match core::str::from_utf8(line) {
                    Ok(v) => v,
                    Err(_) => return Err(NL_EBYTE),
                };
                out.push_str(text);
                out.push('\n');
                next = next.saturating_add(self.cfg.step());
            } else {
                out.push_str(self.cfg.blank_field().as_str());
                let sep = match core::str::from_utf8(self.cfg.sep()) {
                    Ok(v) => v,
                    Err(_) => return Err(NL_ESEP),
                };
                out.push_str(sep);
                let text = match core::str::from_utf8(line) {
                    Ok(v) => v,
                    Err(_) => return Err(NL_EBYTE),
                };
                out.push_str(text);
                out.push('\n');
            }
            i += 1;
        }
        Ok(out)
    }

    /// 将会被编号的行数（不做 IO）。
    pub fn count(&self) -> usize {
        let mut n = 0usize;
        let mut body = true;
        let mut i = 0usize;
        while i < self.lines.len() {
            let line = match self.lines.get(i) {
                Some(v) => *v,
                None => break,
            };
            let kind = delim_kind(line);
            if kind != 0 {
                if kind == 3 {
                    body = true;
                } else if kind == 2 {
                    body = true;
                } else {
                    body = false;
                }
            } else if body && self.cfg.numbers(line) {
                n += 1;
            }
            i += 1;
        }
        n
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

/// `Result<String, u32>` → 样本（观测值为输出长度）。
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
            return Err(NL_ESLOT);
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
            return Err(NL_EBAL);
        }
        const VALS: [u32; 8] = [19, 2, 24, 12, 5, 11, 2, 3];
        const CODES: [u32; 6] =
            [NL_ESTYLE, NL_EFMT, NL_EWIDTH, NL_ESTEP, NL_ESEP, NL_ELINE];
        let mut vi = 0usize;
        let mut ci = 0usize;
        let mut i = 0usize;
        while i < self.len {
            let s = match self.rows.get(i) {
                Some(v) => *v,
                None => return Err(NL_EBAL),
            };
            if s.slot as usize != i {
                return Err(NL_ESLOT);
            }
            if s.ok {
                if s.code != 0 {
                    return Err(NL_EVALUE);
                }
                match VALS.get(vi) {
                    Some(w) => {
                        if s.value != *w {
                            return Err(NL_EVALUE);
                        }
                    }
                    None => return Err(NL_EBAL),
                }
                vi += 1;
            } else {
                match CODES.get(ci) {
                    Some(w) => {
                        if s.code != *w {
                            return Err(NL_EVALUE);
                        }
                    }
                    None => return Err(NL_EBAL),
                }
                ci += 1;
            }
            i += 1;
        }
        if vi != 8 || ci != 6 {
            return Err(NL_EBAL);
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
            return Err(NL_EBAL);
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
            Err(NL_ESEAL)
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

/// 渲染便捷入口。
pub fn render(cfg: Cfg, lines: &[&[u8]]) -> Result<String, u32> {
    match Nl::open(cfg, lines) {
        Ok(n) => n.run(),
        Err(c) => Err(c),
    }
}

/// 十四格账本：三种风格、三种格式、段切换与六类拒绝。
pub fn build_ledger() -> Ledger {
    let mut l = Ledger::new();

    let a = match Cfg::new(STYLE_ALL, FMT_RIGHT, 3, 1, 1, b":") {
        Ok(v) => v,
        Err(_) => return l,
    };
    let la: [&[u8]; 2] = [b"alpha", b"beta"];
    let _ = l.register(text_cell(0, OP_RENDER, render(a, &la)));
    let _ = l.register(word_cell(1, OP_COUNT, Nl::open(a, &la).map(|n| n.count() as u32)));

    let t = match Cfg::new(STYLE_TEXT, FMT_LEFT, 3, 1, 1, b":") {
        Ok(v) => v,
        Err(_) => return l,
    };
    let lb: [&[u8]; 3] = [b"alpha", b"", b"beta"];
    let _ = l.register(text_cell(2, OP_RENDER, render(t, &lb)));

    let z = match Cfg::new(STYLE_ALL, FMT_ZERO, 3, 7, 2, b":") {
        Ok(v) => v,
        Err(_) => return l,
    };
    let lz: [&[u8]; 2] = [b"x", b"y"];
    let _ = l.register(text_cell(3, OP_RENDER, render(z, &lz)));

    let n = match Cfg::new(STYLE_NONE, FMT_RIGHT, 2, 1, 1, b" ") {
        Ok(v) => v,
        Err(_) => return l,
    };
    let ln: [&[u8]; 1] = [b"a"];
    let _ = l.register(text_cell(4, OP_RENDER, render(n, &ln)));

    let _ = l.register(Sample {
        slot: 5,
        op: OP_CFG,
        ok: false,
        code: match Cfg::new(9, FMT_RIGHT, 3, 1, 1, b":") {
            Ok(_) => 0,
            Err(c) => c,
        },
        value: 0,
    });
    let _ = l.register(Sample {
        slot: 6,
        op: OP_CFG,
        ok: false,
        code: match Cfg::new(STYLE_ALL, 9, 3, 1, 1, b":") {
            Ok(_) => 0,
            Err(c) => c,
        },
        value: 0,
    });
    let _ = l.register(Sample {
        slot: 7,
        op: OP_CFG,
        ok: false,
        code: match Cfg::new(STYLE_ALL, FMT_RIGHT, 0, 1, 1, b":") {
            Ok(_) => 0,
            Err(c) => c,
        },
        value: 0,
    });
    let _ = l.register(Sample {
        slot: 8,
        op: OP_CFG,
        ok: false,
        code: match Cfg::new(STYLE_ALL, FMT_RIGHT, 3, 1, 0, b":") {
            Ok(_) => 0,
            Err(c) => c,
        },
        value: 0,
    });
    let _ = l.register(Sample {
        slot: 9,
        op: OP_CFG,
        ok: false,
        code: match Cfg::new(STYLE_ALL, FMT_RIGHT, 3, 1, 1, b"") {
            Ok(_) => 0,
            Err(c) => c,
        },
        value: 0,
    });

    let h = match Cfg::new(STYLE_ALL, FMT_RIGHT, 6, 5, 1, b":") {
        Ok(v) => v,
        Err(_) => return l,
    };
    let lh: [&[u8]; 2] = [DELIM_HEAD, b"n1"];
    let _ = l.register(text_cell(10, OP_RENDER, render(h, &lh)));

    let lv: [&[u8]; 3] = [b"a", b"", b"b"];
    let _ = l.register(word_cell(11, OP_COUNT, Nl::open(t, &lv).map(|x| x.count() as u32)));

    let many: [&[u8]; 17] = [
        b"1", b"2", b"3", b"4", b"5", b"6", b"7", b"8", b"9", b"10", b"11", b"12", b"13", b"14",
        b"15", b"16", b"17",
    ];
    let _ = l.register(word_cell(12, OP_STAT, Nl::open(a, &many).map(|x| x.len() as u32)));

    let lc: [&[u8]; 3] = [b"a", b"b", b"c"];
    let _ = l.register(word_cell(13, OP_COUNT, Nl::open(a, &lc).map(|x| x.count() as u32)));

    l
}

/// 四路注入。
pub fn inject_four() -> [bool; 4] {
    let mut out = [false; 4];

    let l = build_ledger();
    let rep = l.replay();
    let root = l.seal();
    out[0] = l.len() == SAMPLES && l.audit().is_ok() && rep == Ok(root) && root != 0;

    // 一路：右对齐 + 仅非空行 + 空字段对齐。
    let a = match Cfg::new(STYLE_ALL, FMT_RIGHT, 3, 1, 1, b":") {
        Ok(v) => v,
        Err(_) => return out,
    };
    let la: [&[u8]; 2] = [b"alpha", b"beta"];
    let ra = render(a, &la) == Ok(String::from("  1:alpha\n  2:beta\n"));

    let t = match Cfg::new(STYLE_TEXT, FMT_LEFT, 3, 1, 1, b":") {
        Ok(v) => v,
        Err(_) => return out,
    };
    let lb: [&[u8]; 3] = [b"alpha", b"", b"beta"];
    let rb = render(t, &lb) == Ok(String::from("1  :alpha\n   :\n2  :beta\n"));
    out[1] = ra && rb;

    // 二路：补零 + 起始/步长 + 左对齐单字符宽度。
    let z = match Cfg::new(STYLE_ALL, FMT_ZERO, 3, 7, 2, b":") {
        Ok(v) => v,
        Err(_) => return out,
    };
    let lz: [&[u8]; 2] = [b"x", b"y"];
    let rz = render(z, &lz) == Ok(String::from("007:x\n009:y\n"));

    let l = match Cfg::new(STYLE_ALL, FMT_LEFT, 1, 0, 1, b"") {
        Ok(_) => false,
        Err(NL_ESEP) => true,
        Err(_) => false,
    };
    let wide = number_field(FMT_ZERO, 4, 42) == Ok(String::from("0042"))
        && number_field(FMT_RIGHT, 4, 42) == Ok(String::from("  42"))
        && number_field(FMT_LEFT, 4, 42) == Ok(String::from("42  "));
    out[2] = rz && l && wide;

    // 三路：段切换重置、页脚不编号、非 UTF-8 拒绝。
    let h = match Cfg::new(STYLE_ALL, FMT_RIGHT, 6, 5, 1, b":") {
        Ok(v) => v,
        Err(_) => return out,
    };
    let lh: [&[u8]; 5] = [DELIM_HEAD, b"n1", DELIM_FOOT, b"skip", b"\xff"];
    let reset = render(h, &lh[..4])
        == Ok(String::from("\n     5:n1\n\n      :skip\n"));
    let bad = render(h, &lh) == Err(NL_EBYTE);

    let mut long = [b'a'; 80];
    long[0] = b'z';
    let ll: [&[u8]; 1] = [&long];
    let toolong = render(h, &ll) == Err(NL_EINVAL);
    out[3] = reset && bad && toolong;

    out
}

/// 十三项判据。
pub fn run_unx11529_checks() -> CheckSet {
    let mut s = CheckSet::new("unx11529.nl_number");

    s.add(
        "NL01-默认右对齐",
        {
            let a = match Cfg::new(STYLE_ALL, FMT_RIGHT, 6, 1, 1, b"\t") {
                Ok(v) => v,
                Err(_) => return s,
            };
            let l: [&[u8]; 2] = [b"alpha", b"beta"];
            render(a, &l) == Ok(String::from("     1\talpha\n     2\tbeta\n"))
                && Cfg::default_cfg().width() == 6
        },
        "static",
    );

    s.add(
        "NL02-仅非空行",
        {
            let t = match Cfg::new(STYLE_TEXT, FMT_LEFT, 3, 1, 1, b":") {
                Ok(v) => v,
                Err(_) => return s,
            };
            let l: [&[u8]; 3] = [b"alpha", b"", b"beta"];
            render(t, &l) == Ok(String::from("1  :alpha\n   :\n2  :beta\n"))
        },
        "static",
    );

    s.add(
        "NL03-三种格式",
        {
            number_field(FMT_LEFT, 3, 7) == Ok(String::from("7  "))
                && number_field(FMT_RIGHT, 3, 7) == Ok(String::from("  7"))
                && number_field(FMT_ZERO, 3, 7) == Ok(String::from("007"))
                && number_field(FMT_LEFT, 1, 123) == Ok(String::from("123"))
                && number_field(FMT_RIGHT, 0, 1) == Ok(String::from("1"))
                && number_field(9, 3, 1) == Err(NL_EFMT)
        },
        "static",
    );

    s.add(
        "NL04-起始与步长",
        {
            let z = match Cfg::new(STYLE_ALL, FMT_ZERO, 3, 7, 2, b":") {
                Ok(v) => v,
                Err(_) => return s,
            };
            let l: [&[u8]; 3] = [b"x", b"y", b"z"];
            render(z, &l) == Ok(String::from("007:x\n009:y\n011:z\n"))
        },
        "static",
    );

    s.add(
        "NL05-多字节分隔符",
        {
            let c = match Cfg::new(STYLE_ALL, FMT_RIGHT, 1, 1, 1, b"::") {
                Ok(v) => v,
                Err(_) => return s,
            };
            let l: [&[u8]; 1] = [b"x"];
            render(c, &l) == Ok(String::from("1::x\n"))
        },
        "static",
    );

    s.add(
        "NL06-段切换重置",
        {
            let h = match Cfg::new(STYLE_ALL, FMT_RIGHT, 2, 5, 1, b":") {
                Ok(v) => v,
                Err(_) => return s,
            };
            let l: [&[u8]; 6] =
                [b"a", DELIM_HEAD, b"b", DELIM_BODY, b"c", DELIM_FOOT];
            render(h, &l) == Ok(String::from(" 5:a\n\n 5:b\n\n 6:c\n\n"))
                && delim_kind(DELIM_HEAD) == 3
                && delim_kind(DELIM_BODY) == 2
                && delim_kind(DELIM_FOOT) == 1
                && delim_kind(b"x") == 0
        },
        "static",
    );

    s.add(
        "NL07-不编号风格",
        {
            let n = match Cfg::new(STYLE_NONE, FMT_RIGHT, 2, 1, 1, b" ") {
                Ok(v) => v,
                Err(_) => return s,
            };
            let l: [&[u8]; 2] = [b"a", b"b"];
            render(n, &l) == Ok(String::from("   a\n   b\n"))
                && Nl::open(n, &l).map(|x| x.count()) == Ok(0)
        },
        "static",
    );

    s.add(
        "NL08-参数闸门",
        {
            Cfg::new(0, FMT_RIGHT, 3, 1, 1, b":") == Err(NL_ESTYLE)
                && Cfg::new(STYLE_ALL, 0, 3, 1, 1, b":") == Err(NL_EFMT)
                && Cfg::new(STYLE_ALL, FMT_RIGHT, 0, 1, 1, b":") == Err(NL_EWIDTH)
                && Cfg::new(STYLE_ALL, FMT_RIGHT, WMAX + 1, 1, 1, b":") == Err(NL_EWIDTH)
                && Cfg::new(STYLE_ALL, FMT_RIGHT, 3, 1, 0, b":") == Err(NL_ESTEP)
                && Cfg::new(STYLE_ALL, FMT_RIGHT, 3, 1, 1, b"") == Err(NL_ESEP)
                && Cfg::new(STYLE_ALL, FMT_RIGHT, 3, 1, 1, b"abcde") == Err(NL_ESEP)
                && Cfg::new(STYLE_ALL, FMT_RIGHT, 3, 1, 1, b"\n") == Err(NL_ESEP)
        },
        "static",
    );

    s.add(
        "NL09-行数与行宽",
        {
            let a = match Cfg::new(STYLE_ALL, FMT_RIGHT, 1, 1, 1, b":") {
                Ok(v) => v,
                Err(_) => return s,
            };
            let many: [&[u8]; 17] = [
                b"1", b"2", b"3", b"4", b"5", b"6", b"7", b"8", b"9", b"10", b"11", b"12",
                b"13", b"14", b"15", b"16", b"17",
            ];
            let mut long = [b'a'; 80];
            long[0] = b'z';
            let ll: [&[u8]; 1] = [&long];
            render(a, &many) == Err(NL_ELINE)
                && render(a, &ll) == Err(NL_EINVAL)
                && Nl::open(a, &many).is_err()
        },
        "static",
    );

    s.add(
        "NL10-非UTF8拒绝",
        {
            let h = match Cfg::new(STYLE_ALL, FMT_RIGHT, 2, 1, 1, b":") {
                Ok(v) => v,
                Err(_) => return s,
            };
            let l: [&[u8]; 2] = [b"ok", b"\xff"];
            render(h, &l) == Err(NL_EBYTE)
                && render(h, &l[..1]) == Ok(String::from(" 1:ok\n"))
                && has_text(b" \t") == false
                && has_text(b" x") == true
        },
        "static",
    );

    s.add(
        "NL11-账本审计",
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
        "NL12-冻结与重放",
        {
            let mut l = build_ledger();
            let a = l.freeze();
            let b = l.freeze();
            let c = l.register(Sample::blank());
            let sealed = l.seal();
            l.is_frozen()
                && a.is_ok()
                && b == Err(NL_EBAL)
                && c == Err(NL_ESLOT)
                && l.rejects() == 2
                && l.replay() == Err(NL_ESEAL)
                && sealed != 0
        },
        "static",
    );

    s.add(
        "NL13-冻结重放四注入",
        inject_four() == [true; 4] && build_ledger().audit().is_ok(),
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
    fn numbering_styles() {
        let a = Cfg::new(STYLE_ALL, FMT_RIGHT, 3, 1, 1, b":").ok().unwrap();
        let l: [&[u8]; 2] = [b"alpha", b"beta"];
        assert_eq!(render(a, &l), Ok(String::from("  1:alpha\n  2:beta\n")));

        let t = Cfg::new(STYLE_TEXT, FMT_LEFT, 3, 1, 1, b":").ok().unwrap();
        let l2: [&[u8]; 3] = [b"alpha", b"", b"beta"];
        assert_eq!(render(t, &l2), Ok(String::from("1  :alpha\n   :\n2  :beta\n")));
    }

    #[test]
    fn number_formats() {
        assert_eq!(number_field(FMT_LEFT, 3, 7), Ok(String::from("7  ")));
        assert_eq!(number_field(FMT_RIGHT, 3, 7), Ok(String::from("  7")));
        assert_eq!(number_field(FMT_ZERO, 3, 7), Ok(String::from("007")));
        assert_eq!(number_field(FMT_ZERO, 1, 0), Ok(String::from("0")));
        assert_eq!(number_field(0, 1, 0), Err(NL_EFMT));
    }

    #[test]
    fn step_and_start() {
        let z = Cfg::new(STYLE_ALL, FMT_ZERO, 3, 7, 2, b":").ok().unwrap();
        let l: [&[u8]; 3] = [b"x", b"y", b"z"];
        assert_eq!(render(z, &l), Ok(String::from("007:x\n009:y\n011:z\n")));
    }

    #[test]
    fn sections() {
        let h = Cfg::new(STYLE_ALL, FMT_RIGHT, 2, 5, 1, b":").ok().unwrap();
        let l: [&[u8]; 6] = [b"a", DELIM_HEAD, b"b", DELIM_BODY, b"c", DELIM_FOOT];
        assert_eq!(render(h, &l), Ok(String::from(" 5:a\n\n 5:b\n\n 6:c\n\n")));
        let n = Nl::open(h, &l).ok().unwrap();
        assert_eq!(n.count(), 3);
    }

    #[test]
    fn config_gates() {
        assert_eq!(Cfg::new(0, FMT_RIGHT, 3, 1, 1, b":"), Err(NL_ESTYLE));
        assert_eq!(Cfg::new(STYLE_ALL, 0, 3, 1, 1, b":"), Err(NL_EFMT));
        assert_eq!(Cfg::new(STYLE_ALL, FMT_RIGHT, 0, 1, 1, b":"), Err(NL_EWIDTH));
        assert_eq!(Cfg::new(STYLE_ALL, FMT_RIGHT, 3, 1, 0, b":"), Err(NL_ESTEP));
        assert_eq!(Cfg::new(STYLE_ALL, FMT_RIGHT, 3, 1, 1, b""), Err(NL_ESEP));
        assert_eq!(Cfg::new(STYLE_ALL, FMT_RIGHT, 3, 1, 1, b"\n"), Err(NL_ESEP));
    }

    #[test]
    fn freeze_and_replay() {
        let mut l = build_ledger();
        let r = l.freeze().ok().unwrap();
        assert_eq!(l.root(), r);
        assert_eq!(l.replay(), Ok(r));
        assert_eq!(l.freeze(), Err(NL_EBAL));
        assert_eq!(l.register(Sample::blank()), Err(NL_ESLOT));
        assert_eq!(l.rejects(), 2);
        assert_eq!(l.replay(), Err(NL_ESEAL));
    }


}
