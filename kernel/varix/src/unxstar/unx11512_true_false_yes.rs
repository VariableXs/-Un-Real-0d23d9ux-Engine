//! UNX-F11512 true/false/yes 恒值件
//!
//! 三个常被当成「装饰」的工具，恒值性其实是三回事：
//!
//! - `true`：**退出码恒 0**，且**永不产出字节**。POSIX 规定它忽略一切操作数，
//!   连 `--help` 都不解析——`true --help` 不打印帮助，直接 0 退出；
//! - `false`：**退出码恒 1**，同样永不产出字节，同样忽略一切参数；
//! - `yes`：**退出码恒 0**（只会在信号打断时终止），但它是唯一会写字节的一个：
//!   把操作数用单个空格拼接，每行补一个换行，无限重复。
//!
//! 把三者放进同一张表，是为了把「恒值」的两种形态分开记：`true`/`false` 的
//! 恒值性在**退出码**上（与参数无关），`yes` 的恒值性在**行内容**上（与参数
//! 无关地重复同一行）。混为一谈就会写出 `[ -n "$(yes)" ]` 这种把无限流当有限
//! 串用的判据——它不会立刻错，只会在某个管道里把内存吃光。
//!
//! `yes` 的解析有三条真实规则，缺一条都会错：
//!
//! 1. **零操作数默认 `y`**：`yes | head -1` 打的是单个 `y`；
//! 2. **多操作数空格拼接**：`yes a b` 打的是 `a b`，不是 `ab`；
//! 3. **`--` 终止选项**：`yes -- --help` 打的是字面量 `--help`；不带 `--` 时
//!    `--help` 是帮助请求，输出一行帮助并 0 退出。
//!
//! 闸门次序（`emit_to`）：恒值件短路（true/false 直接 0 字节，不看参数）→
//! 帮助/版本优先 → 重复次数（0 与超上限都拒）→ 行宽（含换行）→ 输出预算 →
//! 管道短写。四类数量门分开报，是因为它们对应四种不同的修法：改调用方、
//! 改数据、改上限、查消费者。
//!
//! 账本语义：十四格 = 八格成功（解析两次、取行三次、产出两次、退出码一次）
//! 加六格失败（八类域错里的六类各一格）。`audit` 只认「八值六码」的固定
//! 序列，`seal` 对整账做 FNV-1a 折叠，`replay` 重建一份并比对指纹，
//! `freeze` 之后再登记一律拒绝。

use crate::alloc::format;
use crate::checks::CheckSet;

/// 段基址。
pub const SEG_BASE: u32 = 0xf110;

/// 工具名不认识。
pub const TF_EUNKNOWN: u32 = SEG_BASE + 0x01;
/// 选项不认识。
pub const TF_EOPT: u32 = SEG_BASE + 0x02;
/// 操作数表满。
pub const TF_EFULL: u32 = SEG_BASE + 0x03;
/// 拼接行超宽。
pub const TF_ELONG: u32 = SEG_BASE + 0x04;
/// 重复次数越界。
pub const TF_ECOUNT: u32 = SEG_BASE + 0x05;
/// 输出预算不足。
pub const TF_ECAP: u32 = SEG_BASE + 0x06;
/// 恒值件没有「行」可言。
pub const TF_EIGN: u32 = SEG_BASE + 0x07;
/// 消费者提前关闭管道。
pub const TF_EPIPE: u32 = SEG_BASE + 0x08;
/// 账本形状不符。
pub const TF_EBAL: u32 = SEG_BASE + 0x09;
/// 槽位乱序。
pub const TF_ESLOT: u32 = SEG_BASE + 0x0a;
/// 观测值与复算不符。
pub const TF_EVALUE: u32 = SEG_BASE + 0x0b;
/// 冻结后重放不一致。
pub const TF_ESEAL: u32 = SEG_BASE + 0x0c;
/// 观察下标越界。
pub const TF_ENOBS: u32 = SEG_BASE + 0x0d;

/// 工具：真。
pub const TOOL_TRUE: u8 = 1;
/// 工具：假。
pub const TOOL_FALSE: u8 = 2;
/// 工具：重复输出。
pub const TOOL_YES: u8 = 3;

/// 操作数表容量。
pub const OPS: usize = 8;
/// 单行宽度上限（含换行）。
pub const LINE_MAX: u32 = 64;
/// 单次产出的行数上限。
pub const CAP_LINES: u32 = 64;
/// 单次产出的字节预算。
pub const OUT_CAP: u32 = 64;
/// 账本格数。
pub const SAMPLES: usize = 14;

/// 操作码：解析参数。
pub const OP_PARSE: u8 = 1;
/// 操作码：取行宽。
pub const OP_LINE: u8 = 2;
/// 操作码：产出。
pub const OP_EMIT: u8 = 3;
/// 操作码：查退出码。
pub const OP_STATUS: u8 = 4;

/// 帮助文本（`yes --help`）。
pub const HELP: &str = "Usage: yes [STRING]...\n";
/// 版本文本（`yes --version`）。
pub const VERSION: &str = "yes (varix) 1.0\n";
/// 帮助文本长度。
pub const HELP_LEN: u32 = HELP.len() as u32;
/// 版本文本长度。
pub const VERSION_LEN: u32 = VERSION.len() as u32;

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

/// 工具名 → 编号。
pub fn tool_id(name: &str) -> Result<u8, u32> {
    if name == "true" {
        Ok(TOOL_TRUE)
    } else if name == "false" {
        Ok(TOOL_FALSE)
    } else if name == "yes" {
        Ok(TOOL_YES)
    } else {
        Err(TF_EUNKNOWN)
    }
}

/// 已解析的参数表。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Args {
    tool: u8,
    ops: [&'static str; OPS],
    n: usize,
    help: bool,
    version: bool,
    term: bool,
    ignored: u32,
}

impl Args {
    /// 空白表（仅供测试构造失败的起点）。
    pub const fn blank() -> Args {
        Args {
            tool: TOOL_TRUE,
            ops: [""; OPS],
            n: 0,
            help: false,
            version: false,
            term: false,
            ignored: 0,
        }
    }

    /// 解析命令行；`true`/`false` 一律成功（参数全忽略）。
    pub fn parse(name: &str, argv: &[&'static str]) -> Result<Args, u32> {
        let tool = tool_id(name)?;
        let mut a = Args::blank();
        a.tool = tool;
        if tool != TOOL_YES {
            // POSIX：true/false 忽略一切操作数，连选项都不解析。
            let n = if argv.len() > 255 { 255 } else { argv.len() };
            a.ignored = n as u32;
            return Ok(a);
        }
        let mut i = 0usize;
        while i < argv.len() {
            let arg = match argv.get(i) {
                Some(v) => *v,
                None => break,
            };
            if !a.term {
                if arg == "--" {
                    a.term = true;
                    i += 1;
                    continue;
                }
                if arg == "--help" {
                    a.help = true;
                    i += 1;
                    continue;
                }
                if arg == "--version" {
                    a.version = true;
                    i += 1;
                    continue;
                }
                if arg.starts_with("--") {
                    return Err(TF_EOPT);
                }
                if arg.len() > 1 && arg.starts_with('-') {
                    return Err(TF_EOPT);
                }
            }
            if a.n >= OPS {
                return Err(TF_EFULL);
            }
            if let Some(slot) = a.ops.get_mut(a.n) {
                *slot = arg;
            }
            a.n += 1;
            i += 1;
        }
        Ok(a)
    }

    /// 工具编号。
    pub fn tool(&self) -> u8 {
        self.tool
    }

    /// 操作数个数。
    pub fn operand_count(&self) -> usize {
        self.n
    }

    /// 第 i 个操作数。
    pub fn operand(&self, i: usize) -> Option<&'static str> {
        if i >= self.n {
            return None;
        }
        self.ops.get(i).copied()
    }

    /// 恒值件忽略掉的参数个数。
    pub fn ignored(&self) -> u32 {
        self.ignored
    }

    /// 是否请求帮助。
    pub fn wants_help(&self) -> bool {
        self.help
    }

    /// 是否请求版本。
    pub fn wants_version(&self) -> bool {
        self.version
    }

    /// 是否见过 `--`。
    pub fn terminated(&self) -> bool {
        self.term
    }

    /// 拼接前缀长度（不含换行）；零操作数时是默认的 `y`。
    pub fn prefix_len(&self) -> Result<u32, u32> {
        if self.tool != TOOL_YES {
            return Err(TF_EIGN);
        }
        if self.n == 0 {
            return Ok(1);
        }
        let mut n = 0u32;
        let mut i = 0usize;
        while i < self.n {
            match self.ops.get(i) {
                Some(s) => n = n.saturating_add(s.len() as u32),
                None => return Err(TF_EBAL),
            }
            i += 1;
        }
        Ok(n.saturating_add((self.n as u32).saturating_sub(1)))
    }

    /// 单行长度（含换行）。恒值件没有行，调用即类别错。
    pub fn line_len(&self) -> Result<u32, u32> {
        if self.tool != TOOL_YES {
            return Err(TF_EIGN);
        }
        if self.help {
            return Ok(HELP_LEN);
        }
        if self.version {
            return Ok(VERSION_LEN);
        }
        let total = self.prefix_len()?.saturating_add(1);
        if total > LINE_MAX {
            return Err(TF_ELONG);
        }
        Ok(total)
    }

    /// 产出 `want` 行，返回字节数。
    pub fn emit(&self, want: u32) -> Result<u32, u32> {
        self.emit_to(want, want)
    }

    /// 产出 `want` 行，但消费者只收 `accepted` 行。
    pub fn emit_to(&self, want: u32, accepted: u32) -> Result<u32, u32> {
        if self.tool != TOOL_YES {
            // 恒值件：要多少行都只产出 0 字节。
            return Ok(0);
        }
        if self.help || self.version {
            return self.line_len();
        }
        if want == 0 || want > CAP_LINES {
            return Err(TF_ECOUNT);
        }
        let l = self.line_len()?;
        let total = l.saturating_mul(want);
        if total > OUT_CAP {
            return Err(TF_ECAP);
        }
        if accepted < want {
            return Err(TF_EPIPE);
        }
        Ok(total)
    }

    /// 退出码：假恒 1，其余恒 0。
    pub fn status(&self) -> u32 {
        match self.tool {
            TOOL_FALSE => 1,
            _ => 0,
        }
    }

    /// 不变式自检。
    pub fn consistent(&self) -> bool {
        if self.tool != TOOL_TRUE && self.tool != TOOL_FALSE && self.tool != TOOL_YES {
            return false;
        }
        if self.n > OPS {
            return false;
        }
        if self.tool != TOOL_YES && self.n != 0 {
            return false;
        }
        if self.tool == TOOL_YES && self.ignored != 0 {
            return false;
        }
        if self.term && self.tool != TOOL_YES {
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
            return Err(TF_ESLOT);
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
            return Err(TF_EBAL);
        }
        const VALS: [u32; 8] = [0, 0, 2, 8, 4, 7, 1, 8];
        const CODES: [u32; 6] = [TF_EOPT, TF_EFULL, TF_ELONG, TF_ECOUNT, TF_ECAP, TF_EPIPE];
        let mut vi = 0usize;
        let mut ci = 0usize;
        let mut i = 0usize;
        while i < self.len {
            let s = match self.rows.get(i) {
                Some(v) => *v,
                None => return Err(TF_EBAL),
            };
            if s.slot as usize != i {
                return Err(TF_ESLOT);
            }
            if s.ok {
                if s.code != 0 {
                    return Err(TF_EVALUE);
                }
                match VALS.get(vi) {
                    Some(w) => {
                        if s.value != *w {
                            return Err(TF_EVALUE);
                        }
                    }
                    None => return Err(TF_EBAL),
                }
                vi += 1;
            } else {
                match CODES.get(ci) {
                    Some(w) => {
                        if s.code != *w {
                            return Err(TF_EVALUE);
                        }
                    }
                    None => return Err(TF_EBAL),
                }
                ci += 1;
            }
            i += 1;
        }
        if vi != 8 || ci != 6 {
            return Err(TF_EBAL);
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
            return Err(TF_EBAL);
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
            Err(TF_ESEAL)
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

/// 超宽操作数（70 字节，越过 `LINE_MAX`）。
const LONG: &str =
    "0123456789012345678901234567890123456789012345678901234567890123456789";
/// 九操作数（越过 `OPS`）。
const NINE: [&str; 9] = ["a", "b", "c", "d", "e", "f", "g", "h", "i"];
/// 恒值件的垃圾参数。
const JUNK: [&str; 3] = ["--help", "-x", "junk"];

/// 十四格账本：三档解析 → 行宽 → 产出 → 退出码 → 六类拒绝。
pub fn build_ledger() -> Ledger {
    let mut l = Ledger::new();

    let t = match Args::parse("true", &JUNK) {
        Ok(v) => v,
        Err(_) => return l,
    };
    let _ = l.register(word_cell(0, OP_PARSE, Ok(t.operand_count() as u32)));

    let y0 = match Args::parse("yes", &[]) {
        Ok(v) => v,
        Err(_) => return l,
    };
    let _ = l.register(word_cell(1, OP_PARSE, Ok(y0.operand_count() as u32)));
    let _ = l.register(word_cell(2, OP_LINE, y0.line_len()));
    let _ = l.register(word_cell(3, OP_EMIT, y0.emit(4)));

    let y2 = match Args::parse("yes", &["a", "b"]) {
        Ok(v) => v,
        Err(_) => return l,
    };
    let _ = l.register(word_cell(4, OP_LINE, y2.line_len()));

    let y3 = match Args::parse("yes", &["--", "--help"]) {
        Ok(v) => v,
        Err(_) => return l,
    };
    let _ = l.register(word_cell(5, OP_LINE, y3.line_len()));

    let f = match Args::parse("false", &[]) {
        Ok(v) => v,
        Err(_) => return l,
    };
    let _ = l.register(word_cell(6, OP_STATUS, Ok(f.status())));
    let _ = l.register(word_cell(7, OP_EMIT, y0.emit_to(4, 4)));

    let _ = l.register(word_cell(8, OP_PARSE, Args::parse("yes", &["-z"]).map(|_| 0)));
    let _ = l.register(word_cell(9, OP_PARSE, Args::parse("yes", &NINE).map(|_| 0)));

    let _ = l.register(word_cell(
        10,
        OP_LINE,
        Args::parse("yes", &[LONG]).and_then(|a| a.line_len()),
    ));

    let _ = l.register(word_cell(11, OP_EMIT, y0.emit(0)));

    let _ = l.register(word_cell(
        12,
        OP_EMIT,
        Args::parse("yes", &["yy"]).and_then(|a| a.emit(24)),
    ));

    let _ = l.register(word_cell(13, OP_EMIT, y0.emit_to(4, 2)));

    l
}

/// 四路注入。
pub fn inject_four() -> [bool; 4] {
    let mut out = [false; 4];

    let l = build_ledger();
    let rep = l.replay();
    let root = l.seal();
    out[0] = l.len() == SAMPLES && l.audit().is_ok() && rep == Ok(root) && root != 0;

    // 一路：恒值性——参数再多、再像选项，退出码与字节数都不动。
    let t = match Args::parse("true", &JUNK) {
        Ok(v) => v,
        Err(_) => return out,
    };
    let f = match Args::parse("false", &JUNK) {
        Ok(v) => v,
        Err(_) => return out,
    };
    out[1] = t.ignored() == 3
        && t.operand_count() == 0
        && !t.wants_help()
        && t.status() == 0
        && t.emit(0) == Ok(0)
        && t.emit(CAP_LINES) == Ok(0)
        && t.line_len() == Err(TF_EIGN)
        && f.status() == 1
        && f.emit(8) == Ok(0)
        && t.consistent()
        && f.consistent();

    // 二路：yes 的拼接与终止符。
    let y0 = match Args::parse("yes", &[]) {
        Ok(v) => v,
        Err(_) => return out,
    };
    let y2 = match Args::parse("yes", &["a", "b"]) {
        Ok(v) => v,
        Err(_) => return out,
    };
    let y3 = match Args::parse("yes", &["--", "--help"]) {
        Ok(v) => v,
        Err(_) => return out,
    };
    let yh = match Args::parse("yes", &["--help"]) {
        Ok(v) => v,
        Err(_) => return out,
    };
    let yv = match Args::parse("yes", &["--version"]) {
        Ok(v) => v,
        Err(_) => return out,
    };
    out[2] = y0.line_len() == Ok(2)
        && y0.prefix_len() == Ok(1)
        && y0.emit(6) == Ok(12)
        && y2.line_len() == Ok(4)
        && y2.operand(1) == Some("b")
        && y3.terminated()
        && y3.operand_count() == 1
        && y3.operand(0) == Some("--help")
        && y3.line_len() == Ok(7)
        && yh.wants_help()
        && yh.line_len() == Ok(HELP_LEN)
        && yh.emit(4) == Ok(HELP_LEN)
        && yv.wants_version()
        && yv.line_len() == Ok(VERSION_LEN)
        && y0.consistent()
        && y3.consistent();

    // 三路：八类域的七类可辨（EUNKNOWN 只在名字表上）。
    let unknown = Args::parse("tru", &[]) == Err(TF_EUNKNOWN);
    let opt = Args::parse("yes", &["-z"]) == Err(TF_EOPT)
        && Args::parse("yes", &["--bogus"]) == Err(TF_EOPT);
    let full = Args::parse("yes", &NINE) == Err(TF_EFULL);
    let long = matches!(
        Args::parse("yes", &[LONG]).and_then(|a| a.line_len()),
        Err(TF_ELONG)
    );
    let count = y0.emit(0) == Err(TF_ECOUNT) && y0.emit(CAP_LINES + 1) == Err(TF_ECOUNT);
    let cap = matches!(
        Args::parse("yes", &["yy"]).and_then(|a| a.emit(24)),
        Err(TF_ECAP)
    );
    let pipe = y0.emit_to(4, 3) == Err(TF_EPIPE) && y0.emit_to(4, 4) == Ok(8);
    out[3] = unknown && opt && full && long && count && cap && pipe;

    out
}

/// 十三项判据。
pub fn run_unx11512_checks() -> CheckSet {
    let mut s = CheckSet::new("unx11512.true_false_yes");
    let l = build_ledger();

    s.add_dyn(
        "TF1-十四格八允六拒",
        l.len() == SAMPLES && l.ok_count() == 8 && l.err_count() == 6 && l.audit().is_ok(),
        &format!("n={} ok={} err={}", l.len(), l.ok_count(), l.err_count()),
    );

    s.add(
        "TF2-恒值退出码三档",
        {
            let t = Args::parse("true", &[]).map(|a| a.status());
            let f = Args::parse("false", &[]).map(|a| a.status());
            let y = Args::parse("yes", &[]).map(|a| a.status());
            t == Ok(0) && f == Ok(1) && y == Ok(0) && l.sample(6).map(|x| x.value) == Some(1)
        },
        "static",
    );

    s.add(
        "TF3-true忽略一切参数",
        {
            let t = match Args::parse("true", &JUNK) {
                Ok(v) => v,
                Err(_) => return s,
            };
            t.ignored() == 3
                && t.operand_count() == 0
                && !t.wants_help()
                && t.emit(8) == Ok(0)
                && t.consistent()
                && l.sample(0).map(|x| x.value) == Some(0)
        },
        "static",
    );

    s.add(
        "TF4-yes默认前缀y",
        l.sample(2).map(|x| x.value) == Some(2)
            && {
                let y = match Args::parse("yes", &[]) {
                    Ok(v) => v,
                    Err(_) => return s,
                };
                y.prefix_len() == Ok(1) && y.line_len() == Ok(2) && y.operand_count() == 0
            },
        "static",
    );

    s.add(
        "TF5-多操作数空格拼接",
        {
            let y = match Args::parse("yes", &["a", "b"]) {
                Ok(v) => v,
                Err(_) => return s,
            };
            let z = match Args::parse("yes", &["abc", "d", "e"]) {
                Ok(v) => v,
                Err(_) => return s,
            };
            y.line_len() == Ok(4)
                && y.prefix_len() == Ok(3)
                && z.prefix_len() == Ok(7)
                && l.sample(4).map(|x| x.value) == Some(4)
        },
        "static",
    );

    s.add(
        "TF6-双横线终止选项",
        {
            let y = match Args::parse("yes", &["--", "--help"]) {
                Ok(v) => v,
                Err(_) => return s,
            };
            let bare = match Args::parse("yes", &["--help"]) {
                Ok(v) => v,
                Err(_) => return s,
            };
            y.terminated()
                && y.operand(0) == Some("--help")
                && !y.wants_help()
                && y.line_len() == Ok(7)
                && bare.wants_help()
                && bare.line_len() == Ok(HELP_LEN)
                && l.sample(5).map(|x| x.value) == Some(7)
        },
        "static",
    );

    s.add(
        "TF7-帮助版本优先于重复",
        {
            let h = match Args::parse("yes", &["--help", "x"]) {
                Ok(v) => v,
                Err(_) => return s,
            };
            let v = match Args::parse("yes", &["--version"]) {
                Ok(v) => v,
                Err(_) => return s,
            };
            h.emit(4) == Ok(HELP_LEN)
                && v.emit(4) == Ok(VERSION_LEN)
                && v.status() == 0
                && HELP_LEN != VERSION_LEN
        },
        "static",
    );

    s.add(
        "TF8-未知选项被拒",
        {
            let short = Args::parse("yes", &["-z"]);
            let long = Args::parse("yes", &["--bogus"]);
            let dash = Args::parse("yes", &["-"]);
            let unknown = Args::parse("tru", &[]);
            short == Err(TF_EOPT)
                && long == Err(TF_EOPT)
                && dash.is_ok()
                && unknown == Err(TF_EUNKNOWN)
                && l.sample(8).map(|x| x.code) == Some(TF_EOPT)
        },
        "static",
    );

    s.add(
        "TF9-操作数表满",
        {
            let nine = Args::parse("yes", &NINE);
            let eight = Args::parse("yes", &NINE[0..8]);
            nine == Err(TF_EFULL)
                && matches!(eight, Ok(ref a) if a.operand_count() == 8)
                && l.sample(9).map(|x| x.code) == Some(TF_EFULL)
        },
        "static",
    );

    s.add(
        "TF10-超宽行被拒",
        {
            let long = matches!(
                Args::parse("yes", &[LONG]).and_then(|a| a.line_len()),
                Err(TF_ELONG)
            );
            let ok = match Args::parse("yes", &["012345678901234567890123456789012345678901234567890123456789012"]) {
                Ok(v) => v,
                Err(_) => return s,
            };
            long && ok.line_len() == Ok(LINE_MAX) && l.sample(10).map(|x| x.code) == Some(TF_ELONG)
        },
        "static",
    );

    s.add(
        "TF11-零次与预算两门",
        {
            let y = match Args::parse("yes", &[]) {
                Ok(v) => v,
                Err(_) => return s,
            };
            let wide = match Args::parse("yes", &["yy"]) {
                Ok(v) => v,
                Err(_) => return s,
            };
            y.emit(0) == Err(TF_ECOUNT)
                && y.emit(CAP_LINES + 1) == Err(TF_ECOUNT)
                && y.emit(CAP_LINES / 4) == Ok(2 * (CAP_LINES / 4))
                && wide.emit(24) == Err(TF_ECAP)
                && wide.emit(21) == Ok(63)
                && l.sample(11).map(|x| x.code) == Some(TF_ECOUNT)
                && l.sample(12).map(|x| x.code) == Some(TF_ECAP)
        },
        "static",
    );

    s.add(
        "TF12-管道短写与恒值无行",
        {
            let y = match Args::parse("yes", &[]) {
                Ok(v) => v,
                Err(_) => return s,
            };
            let t = match Args::parse("true", &[]) {
                Ok(v) => v,
                Err(_) => return s,
            };
            y.emit_to(4, 2) == Err(TF_EPIPE)
                && y.emit_to(4, 4) == Ok(8)
                && t.line_len() == Err(TF_EIGN)
                && t.emit(0) == Ok(0)
                && l.sample(13).map(|x| x.code) == Some(TF_EPIPE)
        },
        "static",
    );

    s.add(
        "TF13-冻结重放四注入",
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

    #[test]
    fn ledger_shape() {
        let l = build_ledger();
        assert_eq!(l.len(), SAMPLES);
        assert_eq!(l.ok_count(), 8);
        assert_eq!(l.err_count(), 6);
        assert!(l.audit().is_ok());
        assert_eq!(l.rejects(), 0);
        assert!(!l.is_empty());
    }

    #[test]
    fn checks_pass() {
        let s = run_unx11512_checks();
        assert_eq!(s.len(), 13);
        assert!(s.all_passed());
    }

    #[test]
    fn constants_never_emit() {
        let t = match Args::parse("true", &JUNK) {
            Ok(v) => v,
            Err(_) => panic!("parse"),
        };
        let f = match Args::parse("false", &JUNK) {
            Ok(v) => v,
            Err(_) => panic!("parse"),
        };
        assert_eq!(t.status(), 0);
        assert_eq!(f.status(), 1);
        assert_eq!(t.emit(CAP_LINES), Ok(0));
        assert_eq!(f.emit(CAP_LINES), Ok(0));
        assert_eq!(t.line_len(), Err(TF_EIGN));
        assert_eq!(t.ignored(), 3);
    }

    #[test]
    fn yes_default_prefix() {
        let y = match Args::parse("yes", &[]) {
            Ok(v) => v,
            Err(_) => panic!("parse"),
        };
        assert_eq!(y.prefix_len(), Ok(1));
        assert_eq!(y.line_len(), Ok(2));
        assert_eq!(y.emit(4), Ok(8));
        assert_eq!(y.operand_count(), 0);
    }

    #[test]
    fn yes_joins_with_spaces() {
        let y = match Args::parse("yes", &["a", "b"]) {
            Ok(v) => v,
            Err(_) => panic!("parse"),
        };
        assert_eq!(y.line_len(), Ok(4));
        assert_eq!(y.operand(1), Some("b"));
        assert_eq!(y.operand(2), None);
    }

    #[test]
    fn terminator_is_literal() {
        let y = match Args::parse("yes", &["--", "--help"]) {
            Ok(v) => v,
            Err(_) => panic!("parse"),
        };
        assert!(y.terminated());
        assert!(!y.wants_help());
        assert_eq!(y.operand(0), Some("--help"));
        assert_eq!(y.line_len(), Ok(7));
    }

    #[test]
    fn gates_are_distinct() {
        assert_eq!(Args::parse("yes", &["-z"]), Err(TF_EOPT));
        assert_eq!(Args::parse("yes", &NINE), Err(TF_EFULL));
        assert_eq!(Args::parse("tru", &[]), Err(TF_EUNKNOWN));
        let y = match Args::parse("yes", &[]) {
            Ok(v) => v,
            Err(_) => panic!("parse"),
        };
        assert_eq!(y.emit(0), Err(TF_ECOUNT));
        assert_eq!(y.emit_to(4, 1), Err(TF_EPIPE));
        assert!(matches!(
            Args::parse("yes", &[LONG]).and_then(|a| a.line_len()),
            Err(TF_ELONG)
        ));
    }

    #[test]
    fn inject_four_is_all_true() {
        assert_eq!(inject_four(), [true; 4]);
    }
}
