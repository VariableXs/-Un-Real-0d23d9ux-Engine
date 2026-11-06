//! UNX-F11513 test/[ 条件求值（一元/二元）
//!
//! `test` 与 `[` 是同一个求值器，只差一个收尾的 `]`。它们的语法**不是**运算符
//! 优先级语法，而是**按词数查表**的语法——这是 POSIX 定死的规矩，也是最常被
//! 误解的一处：
//!
//! - 0 词 → 假；
//! - 1 词 → 该词非空即真（所以 `[ ]` 假、`[ "" ]` 假、`[ ! ]` 真）；
//! - 2 词 → `! X`，或一元主项 `-n X` / `-z X` / `-e X` / `-f X` / `-d X`；
//! - 3 词 → 二元主项 `A op B`，或 `! X Y`（对 2 词取反），或 `( X )`（对 1 词求值）；
//! - 4 词 → `! X Y Z`（对 3 词取反），或 `( X Y )`（对 2 词求值）；
//! - 5 词及以上 → 本实现直接拒（`TE_EARITY`）。POSIX 只保证到 4 词，超出的
//!   组合各实现自行其是；与其猜优先级，不如让调用方改成 `&&` 短路。
//!
//! 语法只认 4 词以内，于是「词数 → 分支」就是闸门次序本身：先卡词数，再判
//! `!`、再判 `(`，最后才落到二元主项。顺序颠倒会把 `( x )` 当成一元主项。
//!
//! 三类拒绝必须分开报，因为它们指向完全不同的修法：
//!
//! - `TE_EOP`：`==`、`=~`、`-EQ` 这种**不认识**的运算符。改成 `=` 即可；
//! - `TE_EBOOL`：`-a` / `-o`。它们是被 POSIX 标为过时的布尔连接词，我们不
//!   提供——调用方应当用 shell 的 `&&` / `||`，那还多了短路语义；
//! - `TE_ENEG`：`! ! x`。否定只承认一层，二层否定要靠括号或 shell 展开。
//!
//! 整数比较只在**两端都能解析成整数**时才做：空串、只有符号、混入字母、越过
//! 64 位都报 `TE_EINT`，而不是悄悄退化成字符串比较。这里还有一条明示的不对称：
//! 解析走「正数累加再取反」，因此 `-9223372036854775808`（i64 下界）被判溢出
//! 拒绝——宁可报错，不接受一个静默绕回的边界值。
//!
//! 账本语义：十四格 = 八格成功（各词数分支各一格）+ 六格失败（八类域错里的
//! 六类各一格）。`audit` 认「八值六码」，`seal` 做 FNV-1a 折叠，`replay` 重建
//! 比对，`freeze` 后登记一律拒绝。

use crate::alloc::format;
use crate::checks::CheckSet;

/// 段基址。
pub const SEG_BASE: u32 = 0xf120;

/// 尾括号缺失或多出。
pub const TE_EBRACK: u32 = SEG_BASE + 0x01;
/// 词数超过四。
pub const TE_EARITY: u32 = SEG_BASE + 0x02;
/// 运算符不认识。
pub const TE_EOP: u32 = SEG_BASE + 0x03;
/// 整数非法或溢出。
pub const TE_EINT: u32 = SEG_BASE + 0x04;
/// 括号不配对。
pub const TE_EPAREN: u32 = SEG_BASE + 0x05;
/// 词数与运算符元数不符。
pub const TE_ESHAPE: u32 = SEG_BASE + 0x06;
/// 否定嵌套。
pub const TE_ENEG: u32 = SEG_BASE + 0x07;
/// 布尔连接词被拒。
pub const TE_EBOOL: u32 = SEG_BASE + 0x08;
/// 账本形状不符。
pub const TE_EBAL: u32 = SEG_BASE + 0x09;
/// 槽位乱序。
pub const TE_ESLOT: u32 = SEG_BASE + 0x0a;
/// 观测值与复算不符。
pub const TE_EVALUE: u32 = SEG_BASE + 0x0b;
/// 冻结后重放不一致。
pub const TE_ESEAL: u32 = SEG_BASE + 0x0c;
/// 观察下标越界。
pub const TE_ENOBS: u32 = SEG_BASE + 0x0d;

/// 词的容量（POSIX 保证到 4 词）。
pub const MAXW: usize = 4;
/// 账本格数。
pub const SAMPLES: usize = 14;

/// 路径不存在。
pub const KIND_MISSING: u8 = 0;
/// 普通文件。
pub const KIND_FILE: u8 = 1;
/// 目录。
pub const KIND_DIR: u8 = 2;

/// 操作码：整式求值。
pub const OP_EVAL: u8 = 1;
/// 操作码：带尾括号求值。
pub const OP_BRACKET: u8 = 2;
/// 操作码：一元主项。
pub const OP_UNARY: u8 = 3;
/// 操作码：整数比较。
pub const OP_INT: u8 = 4;

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

/// 真假 → 0/1。
pub fn b2u(b: bool) -> u32 {
    if b {
        1
    } else {
        0
    }
}

/// 取第 i 个词。
fn token(w: &[&'static str], i: usize) -> Result<&'static str, u32> {
    match w.get(i) {
        Some(s) => Ok(*s),
        None => Err(TE_ESHAPE),
    }
}

/// 布尔连接词（POSIX 已标过时，本实现不提供）。
fn bool_op(s: &str) -> bool {
    s == "-a" || s == "-o"
}

/// 名字 → 文件类别（本实现的定长路径表）。
pub fn kind_of(name: &str) -> u8 {
    if name == "a" {
        KIND_FILE
    } else if name == "b" {
        KIND_DIR
    } else {
        KIND_MISSING
    }
}

/// 整数解析：可选符号 + 十进制数字，正数累加后取反。
pub fn int_of(s: &str) -> Result<i64, u32> {
    let b = s.as_bytes();
    if b.is_empty() {
        return Err(TE_EINT);
    }
    let mut i = 0usize;
    let mut neg = false;
    match b.first() {
        Some(&b'-') => {
            neg = true;
            i = 1;
        }
        Some(&b'+') => {
            i = 1;
        }
        _ => {}
    }
    if i >= b.len() {
        return Err(TE_EINT);
    }
    let mut v: i64 = 0;
    while i < b.len() {
        let c = match b.get(i) {
            Some(x) => *x,
            None => return Err(TE_EINT),
        };
        if !c.is_ascii_digit() {
            return Err(TE_EINT);
        }
        v = match v.checked_mul(10) {
            Some(x) => x,
            None => return Err(TE_EINT),
        };
        v = match v.checked_add((c - b'0') as i64) {
            Some(x) => x,
            None => return Err(TE_EINT),
        };
        i += 1;
    }
    if neg {
        v = match v.checked_neg() {
            Some(x) => x,
            None => return Err(TE_EINT),
        };
    }
    Ok(v)
}

/// 一元主项。
pub fn unary(op: &str, arg: &str) -> Result<bool, u32> {
    if bool_op(op) {
        return Err(TE_EBOOL);
    }
    match op {
        "-n" => Ok(!arg.is_empty()),
        "-z" => Ok(arg.is_empty()),
        "-e" => Ok(kind_of(arg) != KIND_MISSING),
        "-f" => Ok(kind_of(arg) == KIND_FILE),
        "-d" => Ok(kind_of(arg) == KIND_DIR),
        "!" => Ok(arg.is_empty()),
        "(" | ")" => Err(TE_EPAREN),
        other => {
            if other.starts_with('-') {
                Err(TE_EOP)
            } else {
                Err(TE_ESHAPE)
            }
        }
    }
}

/// 二元主项。
pub fn binary(a: &str, op: &str, b: &str) -> Result<bool, u32> {
    if bool_op(op) {
        return Err(TE_EBOOL);
    }
    match op {
        "=" => Ok(a == b),
        "!=" => Ok(a != b),
        "-eq" | "-ne" | "-lt" | "-le" | "-gt" | "-ge" => {
            let x = int_of(a)?;
            let y = int_of(b)?;
            Ok(match op {
                "-eq" => x == y,
                "-ne" => x != y,
                "-lt" => x < y,
                "-le" => x <= y,
                "-gt" => x > y,
                _ => x >= y,
            })
        }
        "==" | "=~" | "-EQ" | "-NE" | "-LT" | "-LE" | "-GT" | "-GE" => Err(TE_EOP),
        "(" | ")" => Err(TE_EPAREN),
        other => {
            if other.starts_with('-') {
                Err(TE_EOP)
            } else {
                Err(TE_ESHAPE)
            }
        }
    }
}

/// 按词数分派的求值核心。
fn eval_n(w: &[&'static str]) -> Result<bool, u32> {
    match w.len() {
        0 => Ok(false),
        1 => {
            let a = token(w, 0)?;
            Ok(!a.is_empty())
        }
        2 => {
            let a = token(w, 0)?;
            let b = token(w, 1)?;
            if a == "!" {
                let rest = match w.get(1..) {
                    Some(r) => r,
                    None => return Err(TE_ESHAPE),
                };
                return Ok(!eval_n(rest)?);
            }
            if a == "(" || a == ")" || b == ")" {
                return Err(TE_EPAREN);
            }
            unary(a, b)
        }
        3 => {
            let a = token(w, 0)?;
            let b = token(w, 1)?;
            let c = token(w, 2)?;
            if a == "!" {
                if b == "!" {
                    return Err(TE_ENEG);
                }
                let rest = match w.get(1..) {
                    Some(r) => r,
                    None => return Err(TE_ESHAPE),
                };
                return Ok(!eval_n(rest)?);
            }
            if a == "(" {
                if c != ")" {
                    return Err(TE_EPAREN);
                }
                let one = match w.get(1..2) {
                    Some(r) => r,
                    None => return Err(TE_ESHAPE),
                };
                return eval_n(one);
            }
            if a == ")" || c == "(" {
                return Err(TE_EPAREN);
            }
            binary(a, b, c)
        }
        4 => {
            let a = token(w, 0)?;
            let d = token(w, 3)?;
            if a == "!" {
                let rest = match w.get(1..) {
                    Some(r) => r,
                    None => return Err(TE_ESHAPE),
                };
                return Ok(!eval_n(rest)?);
            }
            if a == "(" {
                if d != ")" {
                    return Err(TE_EPAREN);
                }
                let two = match w.get(1..3) {
                    Some(r) => r,
                    None => return Err(TE_ESHAPE),
                };
                return eval_n(two);
            }
            if d == ")" {
                return Err(TE_EPAREN);
            }
            Err(TE_ESHAPE)
        }
        _ => Err(TE_EARITY),
    }
}

/// `test` 求值。
pub fn eval(words: &[&'static str]) -> Result<bool, u32> {
    if words.len() > MAXW {
        return Err(TE_EARITY);
    }
    let mut i = 0usize;
    while i < words.len() {
        match words.get(i) {
            Some(t) => {
                if bool_op(t) {
                    return Err(TE_EBOOL);
                }
            }
            None => return Err(TE_ESHAPE),
        }
        i += 1;
    }
    eval_n(words)
}

/// `[` 求值：剥掉命令名与尾部 `]`，再走同一条分派。
pub fn eval_br(words: &[&'static str]) -> Result<bool, u32> {
    match words.first() {
        Some(v) => {
            if *v != "[" {
                return Err(TE_EBRACK);
            }
        }
        None => return Err(TE_EBRACK),
    }
    let last = match words.last() {
        Some(v) => *v,
        None => return Err(TE_EBRACK),
    };
    if last != "]" || words.len() < 2 {
        return Err(TE_EBRACK);
    }
    let cut = match words.len().checked_sub(1) {
        Some(v) => v,
        None => return Err(TE_EBRACK),
    };
    let inner = match words.get(1..cut) {
        Some(v) => v,
        None => return Err(TE_EBRACK),
    };
    let mut i = 0usize;
    while i < inner.len() {
        match inner.get(i) {
            Some(t) => {
                if *t == "]" {
                    return Err(TE_EBRACK);
                }
            }
            None => return Err(TE_EBRACK),
        }
        i += 1;
    }
    eval(inner)
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
            return Err(TE_ESLOT);
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
            return Err(TE_EBAL);
        }
        const VALS: [u32; 8] = [0, 0, 1, 0, 1, 1, 1, 1];
        const CODES: [u32; 6] = [TE_EBRACK, TE_EARITY, TE_EOP, TE_EINT, TE_EPAREN, TE_EBOOL];
        let mut vi = 0usize;
        let mut ci = 0usize;
        let mut i = 0usize;
        while i < self.len {
            let s = match self.rows.get(i) {
                Some(v) => *v,
                None => return Err(TE_EBAL),
            };
            if s.slot as usize != i {
                return Err(TE_ESLOT);
            }
            if s.ok {
                if s.code != 0 {
                    return Err(TE_EVALUE);
                }
                match VALS.get(vi) {
                    Some(w) => {
                        if s.value != *w {
                            return Err(TE_EVALUE);
                        }
                    }
                    None => return Err(TE_EBAL),
                }
                vi += 1;
            } else {
                match CODES.get(ci) {
                    Some(w) => {
                        if s.code != *w {
                            return Err(TE_EVALUE);
                        }
                    }
                    None => return Err(TE_EBAL),
                }
                ci += 1;
            }
            i += 1;
        }
        if vi != 8 || ci != 6 {
            return Err(TE_EBAL);
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
            return Err(TE_EBAL);
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
            Err(TE_ESEAL)
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

/// 五词（越过 `MAXW`）。
const FIVE: [&str; 5] = ["x", "=", "x", "-a", "y"];

/// 十四格账本：各词数分支 → 括号 → 文件测试 → 六类拒绝。
pub fn build_ledger() -> Ledger {
    let mut l = Ledger::new();

    let _ = l.register(word_cell(0, OP_EVAL, eval(&[]).map(b2u)));
    let _ = l.register(word_cell(1, OP_EVAL, eval(&[""]).map(b2u)));
    let _ = l.register(word_cell(2, OP_EVAL, eval(&["x"]).map(b2u)));
    let _ = l.register(word_cell(3, OP_EVAL, eval(&["!", "x"]).map(b2u)));
    let _ = l.register(word_cell(4, OP_EVAL, eval(&["a", "=", "a"]).map(b2u)));
    let _ = l.register(word_cell(5, OP_INT, eval(&["3", "-lt", "10"]).map(b2u)));
    let _ = l.register(word_cell(
        6,
        OP_EVAL,
        eval(&["(", "-n", "x", ")"]).map(b2u),
    ));
    let _ = l.register(word_cell(7, OP_BRACKET, eval_br(&["[", "-d", "b", "]"]).map(b2u)));

    let _ = l.register(word_cell(8, OP_BRACKET, eval_br(&["[", "x"]).map(b2u)));
    let _ = l.register(word_cell(9, OP_EVAL, eval(&FIVE).map(b2u)));
    let _ = l.register(word_cell(10, OP_EVAL, eval(&["a", "=~", "b"]).map(b2u)));
    let _ = l.register(word_cell(11, OP_INT, eval(&["x", "-eq", "y"]).map(b2u)));
    let _ = l.register(word_cell(12, OP_EVAL, eval(&["(", "x"]).map(b2u)));
    let _ = l.register(word_cell(13, OP_EVAL, eval(&["a", "-a", "b"]).map(b2u)));

    l
}

/// 四路注入。
pub fn inject_four() -> [bool; 4] {
    let mut out = [false; 4];

    let l = build_ledger();
    let rep = l.replay();
    let root = l.seal();
    out[0] = l.len() == SAMPLES && l.audit().is_ok() && rep == Ok(root) && root != 0;

    // 一路：词数分派。
    out[1] = eval(&[]) == Ok(false)
        && eval(&[""]) == Ok(false)
        && eval(&["x"]) == Ok(true)
        && eval(&["!"]) == Ok(true)
        && eval(&["!", "x"]) == Ok(false)
        && eval(&["!", ""]) == Ok(true)
        && eval(&["(", "x", ")"]) == Ok(true)
        && eval(&["(", "", ")"]) == Ok(false)
        && eval(&["!", "(", "-n", "x", ")"]).is_err();

    // 二路：一元主项与定长路径表。
    out[2] = unary("-n", "x") == Ok(true)
        && unary("-n", "") == Ok(false)
        && unary("-z", "") == Ok(true)
        && unary("-e", "a") == Ok(true)
        && unary("-e", "zz") == Ok(false)
        && unary("-f", "a") == Ok(true)
        && unary("-f", "b") == Ok(false)
        && unary("-d", "b") == Ok(true)
        && unary("-d", "a") == Ok(false)
        && kind_of("b") == KIND_DIR
        && kind_of("zz") == KIND_MISSING;

    // 三路：整数比较六档与边界。
    let hi = "9223372036854775807";
    let over = "9223372036854775808";
    out[3] = binary("3", "-eq", "3") == Ok(true)
        && binary("3", "-ne", "4") == Ok(true)
        && binary("3", "-lt", "4") == Ok(true)
        && binary("3", "-le", "3") == Ok(true)
        && binary("3", "-gt", "2") == Ok(true)
        && binary("3", "-ge", "3") == Ok(true)
        && binary("-3", "-lt", "3") == Ok(true)
        && int_of(hi) == Ok(9223372036854775807)
        && int_of(over) == Err(TE_EINT)
        && int_of("") == Err(TE_EINT)
        && int_of("-") == Err(TE_EINT)
        && int_of("+7") == Ok(7)
        && int_of("--7") == Err(TE_EINT)
        && binary("a", "+", "b") == Err(TE_ESHAPE);

    out
}

/// 十三项判据。
pub fn run_unx11513_checks() -> CheckSet {
    let mut s = CheckSet::new("unx11513.test_bracket_eval");
    let l = build_ledger();

    s.add_dyn(
        "TE1-十四格八允六拒",
        l.len() == SAMPLES && l.ok_count() == 8 && l.err_count() == 6 && l.audit().is_ok(),
        &format!("n={} ok={} err={}", l.len(), l.ok_count(), l.err_count()),
    );

    s.add(
        "TE2-零词为假",
        eval(&[]) == Ok(false) && l.sample(0).map(|x| x.value) == Some(0),
        "static",
    );

    s.add(
        "TE3-一词非空即真",
        eval(&[""]) == Ok(false)
            && eval(&["x"]) == Ok(true)
            && eval(&["!"]) == Ok(true)
            && eval(&[" "]) == Ok(true)
            && l.sample(1).map(|x| x.value) == Some(0)
            && l.sample(2).map(|x| x.value) == Some(1),
    "static",
    );

    s.add(
        "TE4-两词否定与一元",
        eval(&["!", "x"]) == Ok(false)
            && eval(&["!", ""]) == Ok(true)
            && eval(&["-n", "x"]) == Ok(true)
            && eval(&["-z", ""]) == Ok(true)
            && unary("-n", "") == Ok(false)
            && l.sample(3).map(|x| x.value) == Some(0),
        "static",
    );

    s.add(
        "TE5-三词二元等值",
        eval(&["a", "=", "a"]) == Ok(true)
            && eval(&["a", "=", "b"]) == Ok(false)
            && eval(&["a", "!=", "b"]) == Ok(true)
            && eval(&["a", "!=", "a"]) == Ok(false)
            && l.sample(4).map(|x| x.value) == Some(1),
        "static",
    );

    s.add(
        "TE6-整数六档比较",
        {
            let eq = binary("3", "-eq", "3") == Ok(true);
            let ne = binary("3", "-ne", "4") == Ok(true);
            let lt = binary("3", "-lt", "4") == Ok(true);
            let le = binary("3", "-le", "3") == Ok(true);
            let gt = binary("4", "-gt", "3") == Ok(true);
            let ge = binary("3", "-ge", "3") == Ok(true);
            eq && ne && lt && le && gt && ge && l.sample(5).map(|x| x.value) == Some(1)
        },
        "static",
    );

    s.add(
        "TE7-整数边界与符号",
        int_of("9223372036854775807") == Ok(9223372036854775807)
            && int_of("9223372036854775808") == Err(TE_EINT)
            && int_of("-9223372036854775808") == Err(TE_EINT)
            && int_of("+7") == Ok(7)
            && int_of("-7") == Ok(-7)
            && int_of("7x") == Err(TE_EINT)
            && int_of("") == Err(TE_EINT)
            && int_of("-") == Err(TE_EINT)
            && l.sample(11).map(|x| x.code) == Some(TE_EINT),
        "static",
    );

    s.add(
        "TE8-三词括号取一词",
        eval(&["(", "x", ")"]) == Ok(true)
            && eval(&["(", "", ")"]) == Ok(false)
            && eval(&["(", "!", ")"]) == Ok(true)
            && eval(&["(", "-n", "x"]) == Err(TE_EPAREN)
            && eval(&["(", "-n", "x", ")"]) == Ok(true)
            && l.sample(6).map(|x| x.value) == Some(1)
            && l.sample(12).map(|x| x.code) == Some(TE_EPAREN),
        "static",
    );

    s.add(
        "TE9-四词括号与否定",
        eval(&["(", "-z", "x", ")"]) == Ok(false)
            && eval(&["(", "-n", "x", ")"]) == Ok(true)
            && eval(&["!", "-n", "x"]) == Ok(false)
            && eval(&["!", "-z", "x"]) == Ok(true)
            && eval(&["(", "-n", "x", "z"]) == Err(TE_EPAREN)
            && eval(&["(", "-n", "x", "-z"]) == Err(TE_EPAREN),
        "static",
    );

    s.add(
        "TE10-尾括号闸",
        eval_br(&["[", "x", "]"]) == Ok(true)
            && eval_br(&["[", "]"]) == Ok(false)
            && eval_br(&["["]) == Err(TE_EBRACK)
            && eval_br(&["[", "x"]) == Err(TE_EBRACK)
            && eval_br(&["[", "x", "]", "]"]) == Err(TE_EBRACK)
            && eval_br(&[]) == Err(TE_EBRACK)
            && eval_br(&["[", "-d", "b", "]"]).is_ok()
            && l.sample(7).map(|x| x.value) == Some(1)
            && l.sample(8).map(|x| x.code) == Some(TE_EBRACK),
        "static",
    );

    s.add(
        "TE11-运算符白名单",
        eval(&["a", "==", "b"]) == Err(TE_EOP)
            && eval(&["a", "=~", "b"]) == Err(TE_EOP)
            && eval(&["1", "-EQ", "1"]) == Err(TE_EOP)
            && eval(&["x", "-x", "y"]) == Err(TE_EOP)
            && eval(&["a", "-a", "b"]) == Err(TE_EBOOL)
            && eval(&["a", "<", "b"]) == Err(TE_ESHAPE)
            && l.sample(10).map(|x| x.code) == Some(TE_EOP)
            && l.sample(13).map(|x| x.code) == Some(TE_EBOOL),
        "static",
    );

    s.add(
        "TE12-否定嵌套与词数闸",
        eval(&["!", "!", "x"]) == Err(TE_ENEG)
            && eval(&["!", "x", "=", "x"]) == Ok(false)
            && eval(&FIVE) == Err(TE_EARITY)
            && eval(&["x", "=", "x", "y"]) == Err(TE_ESHAPE)
            && l.sample(9).map(|x| x.code) == Some(TE_EARITY),
        "static",
    );

    s.add(
        "TE13-冻结重放四注入",
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
    }

    #[test]
    fn checks_pass() {
        let s = run_unx11513_checks();
        assert_eq!(s.len(), 13);
        assert!(s.all_passed());
    }

    #[test]
    fn arity_dispatch() {
        assert_eq!(eval(&[]), Ok(false));
        assert_eq!(eval(&["x"]), Ok(true));
        assert_eq!(eval(&["!", "x"]), Ok(false));
        assert_eq!(eval(&["a", "=", "a"]), Ok(true));
        assert_eq!(eval(&["(", "-n", "x", ")" ]), Ok(true));
        assert_eq!(eval(&FIVE), Err(TE_EARITY));
    }

    #[test]
    fn integer_semantics() {
        assert_eq!(int_of("-7"), Ok(-7));
        assert_eq!(int_of("+7"), Ok(7));
        assert_eq!(int_of("007"), Ok(7));
        assert_eq!(int_of("9223372036854775808"), Err(TE_EINT));
        assert_eq!(binary("10", "-gt", "9"), Ok(true));
        assert_eq!(binary("10", "-le", "9"), Ok(false));
    }

    #[test]
    fn unary_file_table() {
        assert_eq!(unary("-e", "a"), Ok(true));
        assert_eq!(unary("-f", "b"), Ok(false));
        assert_eq!(unary("-d", "b"), Ok(true));
        assert_eq!(unary("-n", ""), Ok(false));
        assert_eq!(unary("-z", ""), Ok(true));
        assert_eq!(unary("-q", "a"), Err(TE_EOP));
    }

    #[test]
    fn bracket_rules() {
        assert_eq!(eval_br(&["[", "x", "]"]), Ok(true));
        assert_eq!(eval_br(&["[", "x"]), Err(TE_EBRACK));
        assert_eq!(eval_br(&["[", "]", "]"]), Err(TE_EBRACK));
        assert_eq!(eval_br(&["["]), Err(TE_EBRACK));
    }

    #[test]
    fn rejection_codes_differ() {
        assert_eq!(eval(&["!", "!", "x"]), Err(TE_ENEG));
        assert_eq!(eval(&["a", "-a", "b"]), Err(TE_EBOOL));
        assert_eq!(eval(&["a", "==", "a"]), Err(TE_EOP));
        assert_eq!(eval(&["(", "x"]), Err(TE_EPAREN));
        assert_eq!(eval(&["x", "=", "x", "y"]), Err(TE_ESHAPE));
    }

    #[test]
    fn inject_four_is_all_true() {
        assert_eq!(inject_four(), [true; 4]);
    }
}
