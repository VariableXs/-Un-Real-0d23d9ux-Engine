//! UNX-F11575 test/expr 字符串比较字节判据
//!
//! 在 C locale 下，`test "a" \< "b"` 与 `expr "a" \< "b"` 的字符串比较
//! **没有排序权重、没有大小写折叠、没有区域规则**：两个操作数被当成无符号
//! 字节串，逐字节比较（前缀相同则短者为小）。本模块把 `test`/`expr` 的
//! 比较面做成一个小型编译器 + 栈机，判据落在三个层面：
//!
//! * 词法层：[`Word`] 定容 32 字节，控制字节（0x00..=0x1F、0x7F）一律拒绝，
//!   0x80..=0xFF 放行——高位字节参与比较时按数值大小定序（例如 0x80 > 0x7F）。
//! * 语法层：[`Compiler`] 用优先级爬升把词元序列编译成扁平 [`Step`] 序列：
//!   `-o` < `-a` < `!` < 比较，括号只做分组；括号不配对、操作数缺失、
//!   多余词元分别在各自的闸门落错误码，不允许「取第一个操作数凑合」。
//! * 语义层：`test` 模式下 `<`/`>` 恒按字节比较；`expr` 模式下若两侧都能
//!   解析成十进制整数则**提升为整数比较**（`10 < 9` 为假），否则退回字节
//!   比较（`abc < abd` 为真）——这两条正是本判据最容易写错的地方。
//! * 执行层：[`Program::run`] 是定容栈机，槽位 12，值域 `Bool/Str/Int`；
//!   操作码与操作数类型不匹配由 [`TE_EOP`] 显性拒绝，绝不静默按 0 处理。
//!
//! 闸门次序：字节 → 词长 → 词元 → 括号 → 操作数 → 提升 → 栈型 → 结果。
//!
//! 全模块无 std、无 unsafe、无 panic：数组访问一律 `get()`，整数一律
//! `checked_*`，递归深度有硬上界；失败全部落 13 个错误码。账本 14 格
//! （8 成 6 败）在 [`Ledger::audit`] 逐格对账，冻结后重放须同指纹。

use crate::alloc::string::String;
use crate::checks::CheckSet;

/// 段基址。
pub const SEG_BASE: u32 = 0xf920;

/// 账本格数。
pub const SAMPLES: usize = 14;

/// 单词字节容量。
pub const WORD_CAP: usize = 32;

/// 词元容量。
pub const TOK_CAP: usize = 24;

/// 原子表容量。
pub const ATOM_CAP: usize = 12;

/// 指令容量。
pub const STEP_CAP: usize = 32;

/// 值栈容量。
pub const STACK_CAP: usize = 12;

/// 括号嵌套上界。
pub const DEPTH_MAX: u8 = 8;

// ---------------------------------------------------------------------------
// 错误码
// ---------------------------------------------------------------------------

/// 单词含控制字节。
pub const TE_EBYTE: u32 = SEG_BASE + 0x01;
/// 词元无法识别（多余词元、未知符号）。
pub const TE_ETOK: u32 = SEG_BASE + 0x02;
/// 操作数缺失或多余。
pub const TE_EARITY: u32 = SEG_BASE + 0x03;
/// 整数解析失败或溢出。
pub const TE_EINT: u32 = SEG_BASE + 0x04;
/// 括号不配对或嵌套过深。
pub const TE_EBRANCH: u32 = SEG_BASE + 0x05;
/// 操作码/操作数类型错配。
pub const TE_EOP: u32 = SEG_BASE + 0x06;
/// 状态冲突（模式与修饰不匹配）。
pub const TE_ESTATE: u32 = SEG_BASE + 0x07;
/// 词元或单词超长。
pub const TE_ELEN: u32 = SEG_BASE + 0x08;
/// 账本不平。
pub const TE_EBAL: u32 = SEG_BASE + 0x09;
/// 槽位非法。
pub const TE_ESLOT: u32 = SEG_BASE + 0x0a;
/// 观测值不符。
pub const TE_EVALUE: u32 = SEG_BASE + 0x0b;
/// 指纹不符。
pub const TE_ESEAL: u32 = SEG_BASE + 0x0c;
/// 无观测。
pub const TE_ENOBS: u32 = SEG_BASE + 0x0d;

// ---------------------------------------------------------------------------
// 操作码（字节码）
// ---------------------------------------------------------------------------

/// 空串判定 `-z`。
pub const O_EMP: u8 = 0x01;
/// 非空判定 `-n`。
pub const O_NEMP: u8 = 0x02;
/// 字节相等 `=`。
pub const O_SEQ: u8 = 0x03;
/// 字节不等 `!=`。
pub const O_SNE: u8 = 0x04;
/// 字节小于 `<`。
pub const O_SLT: u8 = 0x05;
/// 字节大于 `>`。
pub const O_SGT: u8 = 0x06;
/// 整数相等 `-eq`。
pub const O_IEQ: u8 = 0x07;
/// 整数不等 `-ne`。
pub const O_INE: u8 = 0x08;
/// 整数小于 `-lt`。
pub const O_ILT: u8 = 0x09;
/// 整数大于 `-gt`。
pub const O_IGT: u8 = 0x0a;
/// 整数小于等于 `-le`。
pub const O_ILE: u8 = 0x0b;
/// 整数大于等于 `-ge`。
pub const O_IGE: u8 = 0x0c;
/// 逻辑非 `!`。
pub const O_NOT: u8 = 0x0d;
/// 逻辑与 `-a`。
pub const O_AND: u8 = 0x0e;
/// 逻辑或 `-o`。
pub const O_OR: u8 = 0x0f;

// ---------------------------------------------------------------------------
// 操作码（账本口径）
// ---------------------------------------------------------------------------

/// 字节比较。
pub const OP_CMP: u8 = 0x01;
/// 运行一段程序。
pub const OP_RUN: u8 = 0x02;
/// 整数解析。
pub const OP_INT: u8 = 0x03;
/// 词法。
pub const OP_LEX: u8 = 0x04;
/// 编译。
pub const OP_COMPILE: u8 = 0x05;
/// 求值。
pub const OP_EVAL: u8 = 0x06;

// ---------------------------------------------------------------------------
// 哈希
// ---------------------------------------------------------------------------

/// FNV 偏移基。
const FNV_OFF: u64 = 0xcbf2_9ce4_8422_2325;
/// FNV 素数。
const FNV_PRIME: u64 = 0x100_0000_01b3;

/// 折入一个字节（逐位八次乘法）。
fn fold_byte(h: u64, b: u8) -> u64 {
    let mut v = h ^ (b as u64);
    let mut k = 0usize;
    while k < 8 {
        v = v.wrapping_mul(FNV_PRIME);
        k += 1;
    }
    v
}

/// 折入一个字。
fn fold(h: u64, w: u64) -> u64 {
    let mut v = h;
    let mut sh = 0u32;
    while sh < 64 {
        let b = ((w >> sh) & 0xff) as u8;
        v = fold_byte(v, b);
        sh += 8;
    }
    v
}

// ---------------------------------------------------------------------------
// 字节助手
// ---------------------------------------------------------------------------

/// 逐字节相等。
pub fn eq_bytes(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut i = 0usize;
    while i < a.len() {
        match (a.get(i), b.get(i)) {
            (Some(x), Some(y)) => {
                if x != y {
                    return false;
                }
            }
            _ => return false,
        }
        i += 1;
    }
    true
}

/// 字节序比较：-1 小、0 等、1 大；前缀相同则短者为小。
pub fn cmp_bytes(a: &[u8], b: &[u8]) -> i8 {
    let n = if a.len() < b.len() { a.len() } else { b.len() };
    let mut i = 0usize;
    while i < n {
        match (a.get(i), b.get(i)) {
            (Some(x), Some(y)) => {
                if x < y {
                    return -1;
                }
                if x > y {
                    return 1;
                }
            }
            _ => break,
        }
        i += 1;
    }
    if a.len() < b.len() {
        -1
    } else if a.len() > b.len() {
        1
    } else {
        0
    }
}

/// 三档秩：0 等、1 小、2 大。
pub fn rank(a: &[u8], b: &[u8]) -> u32 {
    match cmp_bytes(a, b) {
        -1 => 1,
        0 => 0,
        _ => 2,
    }
}

// ---------------------------------------------------------------------------
// 单词
// ---------------------------------------------------------------------------

/// 定容单词。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Word {
    bytes: [u8; WORD_CAP],
    len: usize,
}

impl Word {
    /// 空白词。
    pub const fn blank() -> Word {
        Word { bytes: [0u8; WORD_CAP], len: 0 }
    }

    /// 构造：控制字节拒绝，超长拒绝，高位字节放行。
    pub fn new(src: &[u8]) -> Result<Word, u32> {
        if src.len() > WORD_CAP {
            return Err(TE_ELEN);
        }
        let mut i = 0usize;
        while i < src.len() {
            match src.get(i) {
                Some(c) => {
                    if *c < 0x20 || *c == 0x7f {
                        return Err(TE_EBYTE);
                    }
                }
                None => return Err(TE_EBYTE),
            }
            i += 1;
        }
        let mut w = Word::blank();
        let mut k = 0usize;
        while k < src.len() {
            match (src.get(k), w.bytes.get_mut(k)) {
                (Some(s), Some(d)) => *d = *s,
                _ => return Err(TE_ELEN),
            }
            k += 1;
        }
        w.len = src.len();
        Ok(w)
    }

    /// 字节。
    pub fn bytes(&self) -> &[u8] {
        self.bytes.get(0..self.len).unwrap_or(&[])
    }

    /// 长度。
    pub fn len(&self) -> usize {
        self.len
    }

    /// 是否空词。
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// 文本（字节 → char 一一映射）。
    pub fn text(&self) -> String {
        let mut s = String::new();
        let mut i = 0usize;
        while i < self.len {
            if let Some(b) = self.bytes.get(i) {
                s.push(*b as char);
            }
            i += 1;
        }
        s
    }
}

// ---------------------------------------------------------------------------
// 词元
// ---------------------------------------------------------------------------

/// 词元。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tok {
    /// 操作数。
    Word(Word),
    /// `=` / `==`。
    StrEq,
    /// `!=`。
    StrNe,
    /// `<`。
    StrLt,
    /// `>`。
    StrGt,
    /// `-eq`。
    IntEq,
    /// `-ne`。
    IntNe,
    /// `-gt`。
    IntGt,
    /// `-lt`。
    IntLt,
    /// `-ge`。
    IntGe,
    /// `-le`。
    IntLe,
    /// `-a`。
    And,
    /// `-o`。
    Or,
    /// `!`。
    Not,
    /// `-n`。
    UnaryN,
    /// `-z`。
    UnaryZ,
    /// `(`。
    LParen,
    /// `)`。
    RParen,
}

impl Tok {
    /// 空白词元。
    pub const fn blank() -> Tok {
        Tok::Word(Word::blank())
    }

    /// 是否比较类。
    pub fn is_cmp(&self) -> bool {
        matches!(
            self,
            Tok::StrEq
                | Tok::StrNe
                | Tok::StrLt
                | Tok::StrGt
                | Tok::IntEq
                | Tok::IntNe
                | Tok::IntGt
                | Tok::IntLt
                | Tok::IntGe
                | Tok::IntLe
        )
    }

    /// 是否整数专用比较。
    pub fn is_int_cmp(&self) -> bool {
        matches!(
            self,
            Tok::IntEq | Tok::IntNe | Tok::IntGt | Tok::IntLt | Tok::IntGe | Tok::IntLe
        )
    }
}

/// 词元分类。
fn token_of(w: &[u8]) -> Result<Tok, u32> {
    if eq_bytes(w, b"=") || eq_bytes(w, b"==") {
        return Ok(Tok::StrEq);
    }
    if eq_bytes(w, b"!=") {
        return Ok(Tok::StrNe);
    }
    if eq_bytes(w, b"<") {
        return Ok(Tok::StrLt);
    }
    if eq_bytes(w, b">") {
        return Ok(Tok::StrGt);
    }
    if eq_bytes(w, b"-eq") {
        return Ok(Tok::IntEq);
    }
    if eq_bytes(w, b"-ne") {
        return Ok(Tok::IntNe);
    }
    if eq_bytes(w, b"-gt") {
        return Ok(Tok::IntGt);
    }
    if eq_bytes(w, b"-lt") {
        return Ok(Tok::IntLt);
    }
    if eq_bytes(w, b"-ge") {
        return Ok(Tok::IntGe);
    }
    if eq_bytes(w, b"-le") {
        return Ok(Tok::IntLe);
    }
    if eq_bytes(w, b"-a") {
        return Ok(Tok::And);
    }
    if eq_bytes(w, b"-o") {
        return Ok(Tok::Or);
    }
    if eq_bytes(w, b"!") {
        return Ok(Tok::Not);
    }
    if eq_bytes(w, b"-n") {
        return Ok(Tok::UnaryN);
    }
    if eq_bytes(w, b"-z") {
        return Ok(Tok::UnaryZ);
    }
    if eq_bytes(w, b"(") {
        return Ok(Tok::LParen);
    }
    if eq_bytes(w, b")") {
        return Ok(Tok::RParen);
    }
    Ok(Tok::Word(Word::new(w)?))
}

/// 词法：argv → 词元序列。
pub fn tokenize(args: &[&[u8]]) -> Result<([Tok; TOK_CAP], usize), u32> {
    if args.len() > TOK_CAP {
        return Err(TE_ELEN);
    }
    let mut toks = [Tok::blank(); TOK_CAP];
    let mut n = 0usize;
    let mut i = 0usize;
    while i < args.len() {
        let a = match args.get(i) {
            Some(v) => *v,
            None => return Err(TE_ETOK),
        };
        let t = token_of(a)?;
        if let Some(dst) = toks.get_mut(n) {
            *dst = t;
        }
        n += 1;
        i += 1;
    }
    Ok((toks, n))
}

// ---------------------------------------------------------------------------
// 整数
// ---------------------------------------------------------------------------

/// 解析十进制整数；非数字 → [`TE_ETOK`]，溢出 → [`TE_EINT`]。
pub fn parse_int(b: &[u8]) -> Result<i64, u32> {
    if b.is_empty() {
        return Err(TE_ETOK);
    }
    let mut neg = false;
    let mut i = 0usize;
    match b.get(0) {
        Some(b'-') => {
            neg = true;
            i = 1;
        }
        Some(b'+') => {
            i = 1;
        }
        _ => {}
    }
    if i >= b.len() {
        return Err(TE_ETOK);
    }
    let mut v: i64 = 0;
    while i < b.len() {
        let c = match b.get(i) {
            Some(v) => *v,
            None => return Err(TE_ETOK),
        };
        if c < b'0' || c > b'9' {
            return Err(TE_ETOK);
        }
        let d = (c - b'0') as i64;
        v = match v.checked_mul(10).and_then(|x| x.checked_add(d)) {
            Some(x) => x,
            None => return Err(TE_EINT),
        };
        i += 1;
    }
    if neg {
        v.checked_neg().ok_or(TE_EINT)
    } else {
        Ok(v)
    }
}

// ---------------------------------------------------------------------------
// 模式与程序
// ---------------------------------------------------------------------------

/// 求值模式。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    /// `test`：`<`/`>` 恒按字节。
    Test,
    /// `expr`：两侧皆整数则提升为整数比较。
    Expr,
}

impl Mode {
    /// 文本。
    pub fn text(&self) -> &'static str {
        match self {
            Mode::Test => "test",
            Mode::Expr => "expr",
        }
    }
}

/// 一条指令。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Step {
    /// 压入原子下标。
    Str(u16),
    /// 压入整数。
    Int(i64),
    /// 执行操作码。
    Op(u8),
}

/// 值。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Val {
    /// 布尔。
    B(bool),
    /// 字符串（原子下标）。
    S(u16),
    /// 整数。
    I(i64),
}

/// 编译产物：指令流 + 原子表 + 模式。
#[derive(Clone, Copy, Debug)]
pub struct Program {
    steps: [Step; STEP_CAP],
    n: usize,
    atoms: [Word; ATOM_CAP],
    na: usize,
    mode: Mode,
}

impl Program {
    /// 空程序。
    pub const fn blank() -> Program {
        Program {
            steps: [Step::Op(0); STEP_CAP],
            n: 0,
            atoms: [Word::blank(); ATOM_CAP],
            na: 0,
            mode: Mode::Test,
        }
    }

    /// 指令数。
    pub fn len(&self) -> usize {
        self.n
    }

    /// 是否空。
    pub fn is_empty(&self) -> bool {
        self.n == 0
    }

    /// 原子数。
    pub fn atom_count(&self) -> usize {
        self.na
    }

    /// 模式。
    pub fn mode(&self) -> Mode {
        self.mode
    }

    /// 取指令。
    pub fn step_at(&self, i: usize) -> Option<Step> {
        if i >= self.n {
            return None;
        }
        self.steps.get(i).copied()
    }

    /// 取原子字节。
    pub fn atom_bytes(&self, i: u16) -> Option<&[u8]> {
        self.atoms.get(i as usize).map(|w| w.bytes())
    }

    /// 执行。结果为布尔真/假，栈型错误落 [`TE_EOP`]。
    pub fn run(&self) -> Result<bool, u32> {
        let mut st = [Val::B(false); STACK_CAP];
        let mut sp = 0usize;
        let mut i = 0usize;
        while i < self.n {
            let step = match self.steps.get(i) {
                Some(v) => *v,
                None => return Err(TE_EOP),
            };
            match step {
                Step::Str(k) => push_val(&mut st, &mut sp, Val::S(k))?,
                Step::Int(v) => push_val(&mut st, &mut sp, Val::I(v))?,
                Step::Op(op) => match op {
                    O_NOT => {
                        let a = pop_val(&mut st, &mut sp)?;
                        match a {
                            Val::B(v) => push_val(&mut st, &mut sp, Val::B(!v))?,
                            _ => return Err(TE_EOP),
                        }
                    }
                    O_AND | O_OR => {
                        let b = pop_val(&mut st, &mut sp)?;
                        let a = pop_val(&mut st, &mut sp)?;
                        let x = match a {
                            Val::B(v) => v,
                            _ => return Err(TE_EOP),
                        };
                        let y = match b {
                            Val::B(v) => v,
                            _ => return Err(TE_EOP),
                        };
                        let r = if op == O_AND { x && y } else { x || y };
                        push_val(&mut st, &mut sp, Val::B(r))?;
                    }
                    O_EMP | O_NEMP => {
                        let a = pop_val(&mut st, &mut sp)?;
                        let k = match a {
                            Val::S(k) => k,
                            _ => return Err(TE_EOP),
                        };
                        let w = match self.atoms.get(k as usize) {
                            Some(v) => *v,
                            None => return Err(TE_EOP),
                        };
                        let empty = w.is_empty();
                        let r = if op == O_EMP { empty } else { !empty };
                        push_val(&mut st, &mut sp, Val::B(r))?;
                    }
                    _ => {
                        let b = pop_val(&mut st, &mut sp)?;
                        let a = pop_val(&mut st, &mut sp)?;
                        let r = self.compare(a, b, op)?;
                        push_val(&mut st, &mut sp, Val::B(r))?;
                    }
                },
            }
            i += 1;
        }
        if sp != 1 {
            return Err(TE_EARITY);
        }
        match st.get(0) {
            Some(Val::B(v)) => Ok(*v),
            _ => Err(TE_EOP),
        }
    }

    /// 比较：字符串按字节，整数按数值，类型必须匹配。
    fn compare(&self, a: Val, b: Val, op: u8) -> Result<bool, u32> {
        match (a, b) {
            (Val::S(x), Val::S(y)) => {
                let wa = match self.atoms.get(x as usize) {
                    Some(v) => *v,
                    None => return Err(TE_EOP),
                };
                let wb = match self.atoms.get(y as usize) {
                    Some(v) => *v,
                    None => return Err(TE_EOP),
                };
                let c = cmp_bytes(wa.bytes(), wb.bytes());
                match op {
                    O_SEQ => Ok(c == 0),
                    O_SNE => Ok(c != 0),
                    O_SLT => Ok(c < 0),
                    O_SGT => Ok(c > 0),
                    _ => Err(TE_EOP),
                }
            }
            (Val::I(x), Val::I(y)) => match op {
                O_IEQ => Ok(x == y),
                O_INE => Ok(x != y),
                O_ILT => Ok(x < y),
                O_IGT => Ok(x > y),
                O_ILE => Ok(x <= y),
                O_IGE => Ok(x >= y),
                _ => Err(TE_EOP),
            },
            _ => Err(TE_EOP),
        }
    }
}

/// 压栈。
fn push_val(st: &mut [Val; STACK_CAP], sp: &mut usize, v: Val) -> Result<(), u32> {
    if *sp >= STACK_CAP {
        return Err(TE_EARITY);
    }
    match st.get_mut(*sp) {
        Some(dst) => {
            *dst = v;
            *sp += 1;
            Ok(())
        }
        None => Err(TE_EARITY),
    }
}

/// 弹栈。
fn pop_val(st: &mut [Val; STACK_CAP], sp: &mut usize) -> Result<Val, u32> {
    if *sp == 0 {
        return Err(TE_EARITY);
    }
    *sp -= 1;
    st.get(*sp).copied().ok_or(TE_EARITY)
}

// ---------------------------------------------------------------------------
// 编译器
// ---------------------------------------------------------------------------

/// 优先级爬升编译器。
struct Compiler {
    steps: [Step; STEP_CAP],
    n: usize,
    atoms: [Word; ATOM_CAP],
    na: usize,
    toks: [Tok; TOK_CAP],
    nt: usize,
    at: usize,
    mode: Mode,
    depth: u8,
}

impl Compiler {
    /// 建编译器。
    fn new(args: &[&[u8]], mode: Mode) -> Result<Compiler, u32> {
        let (toks, nt) = tokenize(args)?;
        Ok(Compiler {
            steps: [Step::Op(0); STEP_CAP],
            n: 0,
            atoms: [Word::blank(); ATOM_CAP],
            na: 0,
            toks,
            nt,
            at: 0,
            mode,
            depth: 0,
        })
    }

    /// 追加指令。
    fn emit(&mut self, s: Step) -> Result<(), u32> {
        if self.n >= STEP_CAP {
            return Err(TE_ELEN);
        }
        if let Some(dst) = self.steps.get_mut(self.n) {
            *dst = s;
        }
        self.n += 1;
        Ok(())
    }

    /// 追加原子。
    fn add_atom(&mut self, w: Word) -> Result<u16, u32> {
        if self.na >= ATOM_CAP {
            return Err(TE_ELEN);
        }
        let idx = self.na;
        if let Some(dst) = self.atoms.get_mut(idx) {
            *dst = w;
        }
        self.na += 1;
        Ok(idx as u16)
    }

    /// 看下一个词元。
    fn peek(&self) -> Option<Tok> {
        if self.at >= self.nt {
            return None;
        }
        self.toks.get(self.at).copied()
    }

    /// 取原子字节。
    fn atom_bytes(&self, i: u16) -> Option<&[u8]> {
        self.atoms.get(i as usize).map(|w| w.bytes())
    }

    /// 取下一个词元。
    fn bump(&mut self) -> Option<Tok> {
        if self.at >= self.nt {
            return None;
        }
        let t = self.toks.get(self.at).copied();
        self.at += 1;
        t
    }

    /// 或：`and (-o and)*`。
    fn parse_or(&mut self) -> Result<(), u32> {
        self.parse_and()?;
        while matches!(self.peek(), Some(Tok::Or)) {
            self.bump();
            self.parse_and()?;
            self.emit(Step::Op(O_OR))?;
        }
        Ok(())
    }

    /// 与：`not (-a not)*`。
    fn parse_and(&mut self) -> Result<(), u32> {
        self.parse_not()?;
        while matches!(self.peek(), Some(Tok::And)) {
            self.bump();
            self.parse_not()?;
            self.emit(Step::Op(O_AND))?;
        }
        Ok(())
    }

    /// 非：右结合前缀 `!`。
    fn parse_not(&mut self) -> Result<(), u32> {
        if matches!(self.peek(), Some(Tok::Not)) {
            self.bump();
            self.depth = self.depth.saturating_add(1);
            if self.depth > DEPTH_MAX {
                return Err(TE_EBRANCH);
            }
            self.parse_not()?;
            self.depth -= 1;
            self.emit(Step::Op(O_NOT))?;
            return Ok(());
        }
        self.parse_cmp()
    }

    /// 比较：`primary [cmp primary]`（不允许链式）。
    fn parse_cmp(&mut self) -> Result<(), u32> {
        self.parse_primary()?;
        let op = match self.peek() {
            Some(t) => t,
            None => return Ok(()),
        };
        if !op.is_cmp() {
            return Ok(());
        }
        self.bump();
        self.parse_primary()?;
        let code = self.emit_cmp(op)?;
        self.emit(Step::Op(code))
    }

    /// 主表达式：单词、括号、一元 `-n`/`-z`。
    fn parse_primary(&mut self) -> Result<(), u32> {
        let t = match self.bump() {
            Some(v) => v,
            None => return Err(TE_EARITY),
        };
        match t {
            Tok::Word(w) => {
                let idx = self.add_atom(w)?;
                self.emit(Step::Str(idx))
            }
            Tok::UnaryN | Tok::UnaryZ => {
                let word = match self.bump() {
                    Some(Tok::Word(w)) => w,
                    _ => return Err(TE_EARITY),
                };
                let idx = self.add_atom(word)?;
                self.emit(Step::Str(idx))?;
                if t == Tok::UnaryN {
                    self.emit(Step::Op(O_NEMP))
                } else {
                    self.emit(Step::Op(O_EMP))
                }
            }
            Tok::LParen => {
                self.depth = self.depth.saturating_add(1);
                if self.depth > DEPTH_MAX {
                    return Err(TE_EBRANCH);
                }
                self.parse_or()?;
                self.depth -= 1;
                match self.bump() {
                    Some(Tok::RParen) => Ok(()),
                    _ => Err(TE_EBRANCH),
                }
            }
            _ => Err(TE_EARITY),
        }
    }

    /// 把栈顶两条 `Str` 指令改写为整数或按字节比较，返回操作码。
    fn emit_cmp(&mut self, op: Tok) -> Result<u8, u32> {
        if self.n < 2 {
            return Err(TE_EARITY);
        }
        let bs = match self.steps.get(self.n - 1) {
            Some(v) => *v,
            None => return Err(TE_EARITY),
        };
        let asx = match self.steps.get(self.n - 2) {
            Some(v) => *v,
            None => return Err(TE_EARITY),
        };
        let (ai, bi) = match (asx, bs) {
            (Step::Str(x), Step::Str(y)) => (x, y),
            _ => return Err(TE_EARITY),
        };
        let mut numeric = op.is_int_cmp();
        if !numeric && self.mode == Mode::Expr {
            let pa = parse_int(self.atom_bytes(ai).unwrap_or(&[]));
            let pb = parse_int(self.atom_bytes(bi).unwrap_or(&[]));
            match (pa, pb) {
                (Ok(_), Ok(_)) => numeric = true,
                (Err(TE_EINT), _) | (_, Err(TE_EINT)) => return Err(TE_EINT),
                _ => {}
            }
        }
        if numeric {
            let va = parse_int(self.atom_bytes(ai).unwrap_or(&[]))?;
            let vb = parse_int(self.atom_bytes(bi).unwrap_or(&[]))?;
            if let Some(dst) = self.steps.get_mut(self.n - 2) {
                *dst = Step::Int(va);
            }
            if let Some(dst) = self.steps.get_mut(self.n - 1) {
                *dst = Step::Int(vb);
            }
            let code = match op {
                Tok::StrEq | Tok::IntEq => O_IEQ,
                Tok::StrNe | Tok::IntNe => O_INE,
                Tok::StrLt | Tok::IntLt => O_ILT,
                Tok::StrGt | Tok::IntGt => O_IGT,
                Tok::IntLe => O_ILE,
                Tok::IntGe => O_IGE,
                _ => return Err(TE_EOP),
            };
            return Ok(code);
        }
        match op {
            Tok::StrEq => Ok(O_SEQ),
            Tok::StrNe => Ok(O_SNE),
            Tok::StrLt => Ok(O_SLT),
            Tok::StrGt => Ok(O_SGT),
            _ => Err(TE_EOP),
        }
    }

    /// 收束为程序。
    fn finish(mut self) -> Result<Program, u32> {
        if self.nt == 0 {
            return Err(TE_EARITY);
        }
        self.parse_or()?;
        if self.at != self.nt {
            // 多余词元：典型是链式比较或两个操作数并排。
            match self.toks.get(self.at).copied() {
                Some(Tok::RParen) => return Err(TE_EBRANCH),
                _ => return Err(TE_ETOK),
            }
        }
        if self.n == 0 {
            return Err(TE_EARITY);
        }
        Ok(Program { steps: self.steps, n: self.n, atoms: self.atoms, na: self.na, mode: self.mode })
    }
}

/// 编译 argv。
pub fn compile_args(args: &[&[u8]], mode: Mode) -> Result<Program, u32> {
    Compiler::new(args, mode)?.finish()
}

/// `test` 求值。
pub fn test(args: &[&[u8]]) -> Result<bool, u32> {
    compile_args(args, Mode::Test)?.run()
}

/// `expr` 求值。
pub fn expr(args: &[&[u8]]) -> Result<bool, u32> {
    compile_args(args, Mode::Expr)?.run()
}

// ---------------------------------------------------------------------------
// 样本与账本
// ---------------------------------------------------------------------------

/// 一格样本。
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

/// 账本。
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

    /// 是否空。
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// 取格。
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
            return Err(TE_EBAL);
        }
        const VALS: [u32; 8] = [1, 1, 0, 1, 1, 0, 2_147_483_647, 2];
        const CODES: [u32; 6] = [TE_EARITY, TE_EBRANCH, TE_EARITY, TE_EINT, TE_ETOK, TE_EBYTE];
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
        let a = fresh.seal();
        if a == self.seal() {
            Ok(a)
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

// ---------------------------------------------------------------------------
// 账本组装
// ---------------------------------------------------------------------------

/// `Result<bool, u32>` → 样本（真记 1、假记 0）。
fn bool_cell(slot: u8, op: u8, r: Result<bool, u32>) -> Sample {
    match r {
        Ok(v) => Sample { slot, op, ok: true, code: 0, value: if v { 1 } else { 0 } },
        Err(c) => Sample { slot, op, ok: false, code: c, value: 0 },
    }
}

/// `Result<u32, u32>` → 样本。
fn u32_cell(slot: u8, op: u8, r: Result<u32, u32>) -> Sample {
    match r {
        Ok(v) => Sample { slot, op, ok: true, code: 0, value: v },
        Err(c) => Sample { slot, op, ok: false, code: c, value: 0 },
    }
}

/// 账本组装：槽 0..7 成、8..13 败。
pub fn build_ledger() -> Ledger {
    let mut l = Ledger::new();

    let _ = l.register(u32_cell(0, OP_CMP, Ok(rank(b"Z", b"a"))));
    let _ = l.register(u32_cell(1, OP_CMP, Ok(rank(b"abc", b"abd"))));
    let _ = l.register(u32_cell(2, OP_CMP, Ok(rank(b"abc", b"abc"))));
    let _ = l.register(u32_cell(3, OP_CMP, Ok(rank(b"ab", b"abc"))));

    let _ = l.register(bool_cell(4, OP_RUN, test(&[b"x", b"=", b"x"])));

    let _ = l.register(bool_cell(
        5,
        OP_RUN,
        expr(&[b"10", b"<", b"9"]),
    ));

    let _ = l.register(u32_cell(
        6,
        OP_INT,
        parse_int(b"2147483647").map(|v| v as u32),
    ));

    let _ = l.register(u32_cell(7, OP_CMP, Ok(rank(b"a", b"Z"))));

    let _ = l.register(bool_cell(8, OP_RUN, test(&[b"x", b"="])));
    let _ = l.register(bool_cell(9, OP_RUN, test(&[b"(", b"x", b"=", b"y"])));
    let _ = l.register(bool_cell(10, OP_RUN, test(&[b"-eq", b"3"])));
    let _ = l.register(bool_cell(
        11,
        OP_INT,
        parse_int(b"9999999999999999999").map(|_| false),
    ));
    let _ = l.register(bool_cell(12, OP_RUN, test(&[b"x", b"=", b"y", b"z"])));
    let _ = l.register(bool_cell(13, OP_RUN, test(&[&[b'x', 0x01][..]])));

    l
}

// ---------------------------------------------------------------------------
// 判据
// ---------------------------------------------------------------------------

/// 字节比较面。
fn compare_ok() -> bool {
    let a = rank(b"Z", b"a") == 1;
    let b = rank(b"a", b"Z") == 2;
    let c = rank(b"abc", b"abc") == 0;
    let d = rank(b"ab", b"abc") == 1;
    let e = rank(b"abc", b"ab") == 2;
    let f = rank(b"", b"") == 0 && rank(b"", b"a") == 1;
    let g = rank(b"09", b"1") == 1;
    let h = rank(&[0x80], &[0x7f]) == 2;
    let i = cmp_bytes(b"a", b"b") == -1 && cmp_bytes(b"b", b"a") == 1;
    a && b && c && d && e && f && g && h && i
}

/// 词法面。
fn lex_ok() -> bool {
    let a = match tokenize(&[b"x", b"=", b"y"]) {
        Ok((t, n)) => {
            n == 3 && t.get(0) == Some(&Tok::Word(Word::new(b"x").unwrap_or(Word::blank())))
        }
        Err(_) => false,
    };
    let b = match tokenize(&[b"-eq", b"-a", b"-o", b"!", b"-n", b"-z"]) {
        Ok((t, n)) => {
            n == 6
                && t.get(0) == Some(&Tok::IntEq)
                && t.get(1) == Some(&Tok::And)
                && t.get(2) == Some(&Tok::Or)
                && t.get(3) == Some(&Tok::Not)
                && t.get(4) == Some(&Tok::UnaryN)
                && t.get(5) == Some(&Tok::UnaryZ)
        }
        Err(_) => false,
    };
    let c = tokenize(&[b"("]).map(|(t, _)| t.get(0).copied()) == Ok(Some(Tok::LParen));
    let d = tokenize(&[&[0x01][..]]).err() == Some(TE_EBYTE);
    let e = tokenize(&[&[0x7f][..]]).err() == Some(TE_EBYTE);
    let f = tokenize(&[&[b'x'; WORD_CAP + 1][..]]).err() == Some(TE_ELEN);
    let g = tokenize(&[&[0x80][..]]).map(|(_, n)| n) == Ok(1);
    a && b && c && d && e && f && g
}

/// 字节串比较求值面。
fn eval_ok() -> bool {
    let a = test(&[b"x", b"=", b"x"]) == Ok(true);
    let b = test(&[b"x", b"!=", b"y"]) == Ok(true);
    let c = test(&[b"", b"=", b""]) == Ok(true);
    let d = test(&[b"a", b"<", b"b"]) == Ok(true);
    let e = test(&[b"b", b"<", b"a"]) == Ok(false);
    let f = test(&[b"abc", b"<", b"abd"]) == Ok(true);
    let g = test(&[b"abd", b"<", b"abc"]) == Ok(false);
    let h = test(&[b"-n", b"x"]) == Ok(true);
    let i = test(&[b"-z", b""]) == Ok(true);
    let j = test(&[b"-n", b""]) == Ok(false);
    a && b && c && d && e && f && g && h && i && j
}

/// 逻辑与分组面。
fn logic_ok() -> bool {
    let a = test(&[b"a", b"=", b"a", b"-o", b"b", b"=", b"c"]) == Ok(true);
    let b = test(&[b"a", b"=", b"b", b"-o", b"c", b"=", b"c"]) == Ok(true);
    let c = test(&[b"a", b"=", b"b", b"-a", b"c", b"=", b"c"]) == Ok(false);
    let d = test(&[b"(", b"a", b"=", b"b", b"-o", b"c", b"=", b"c", b")", b"-a", b"-n", b"z"])
        == Ok(true);
    let e = test(&[b"!", b"(", b"1", b"=", b"2", b")"]) == Ok(true);
    let f = test(&[b"!", b"!", b"1", b"=", b"1"]) == Ok(true);
    let g = test(&[b"1", b"=", b"1", b"-a", b"2", b"=", b"2", b"-a", b"3", b"=", b"3"]) == Ok(true);
    a && b && c && d && e && f && g
}

/// 整数提升面。
fn numeric_ok() -> bool {
    let a = test(&[b"10", b"<", b"9"]) == Ok(true);
    let b = expr(&[b"10", b"<", b"9"]) == Ok(false);
    let c = test(&[b"010", b"=", b"10"]) == Ok(false);
    let d = expr(&[b"010", b"=", b"10"]) == Ok(true);
    let e = test(&[b"3", b"-gt", b"2"]) == Ok(true);
    let f = test(&[b"3", b"-ge", b"3"]) == Ok(true);
    let g = test(&[b"3", b"-le", b"2"]) == Ok(false);
    let h = test(&[b"-5", b"-lt", b"0"]) == Ok(true);
    let i = expr(&[b"abc", b"<", b"abd"]) == Ok(true);
    let j = expr(&[b"2", b"-ne", b"3"]) == Ok(true);
    a && b && c && d && e && f && g && h && i && j
}

/// 编译形状面。
fn compile_ok() -> bool {
    let a = match compile_args(&[b"x", b"=", b"x"], Mode::Test) {
        Ok(p) => p.len() == 3 && p.atom_count() == 2 && p.mode() == Mode::Test,
        Err(_) => false,
    };
    let b = match compile_args(&[b"-n", b"x"], Mode::Test) {
        Ok(p) => p.len() == 2 && p.atom_count() == 1,
        Err(_) => false,
    };
    let c = match compile_args(&[b"!", b"x", b"=", b"y"], Mode::Test) {
        Ok(p) => p.len() == 4,
        Err(_) => false,
    };
    let d = match compile_args(&[b"10", b"<", b"9"], Mode::Expr) {
        Ok(p) => match p.step_at(0) {
            Some(Step::Int(10)) => true,
            _ => false,
        },
        Err(_) => false,
    };
    let e = match compile_args(&[b"abc", b"<", b"abd"], Mode::Expr) {
        Ok(p) => matches!(p.step_at(0), Some(Step::Str(_))),
        Err(_) => false,
    };
    let f = compile_args(&[], Mode::Test).err() == Some(TE_EARITY);
    a && b && c && d && e && f
}

/// 错误码面。
fn errors_ok() -> bool {
    let a = test(&[b"x", b"="]).err() == Some(TE_EARITY);
    let b = test(&[b"(", b"x", b"=", b"y"]).err() == Some(TE_EBRANCH);
    let c = test(&[b"x", b"=", b"y", b")"]).err() == Some(TE_EBRANCH);
    let d = test(&[b"x", b"=", b"y", b"z"]).err() == Some(TE_ETOK);
    let e = test(&[b"x", b"=", b"y", b"=", b"z"]).err() == Some(TE_ETOK);
    let f = expr(&[b"9999999999999999999", b"<", b"1"]).err() == Some(TE_EINT);
    let g = test(&[b"abc", b"-eq", b"0"]).err() == Some(TE_ETOK);
    let h = test(&[b"-a", b"x"]).err() == Some(TE_EARITY);
    let i = test(&[&[b'x', 0x02][..]]).err() == Some(TE_EBYTE);
    a && b && c && d && e && f && g && h && i
}

/// 账本闭环。
fn ledger_ok() -> bool {
    let l = build_ledger();
    if l.audit() != Ok(()) {
        return false;
    }
    if l.len() != SAMPLES || l.ok_count() != 8 || l.err_count() != 6 {
        return false;
    }
    let s1 = l.seal();
    if s1 != build_ledger().seal() {
        return false;
    }
    let mut l2 = build_ledger();
    let root = match l2.freeze() {
        Ok(v) => v,
        Err(_) => return false,
    };
    if root != s1 || !l2.is_frozen() {
        return false;
    }
    let replay = match l2.replay() {
        Ok(v) => v == root,
        Err(_) => false,
    };
    if !replay {
        return false;
    }
    if l2.register(Sample::blank()) != Err(TE_ESLOT) {
        return false;
    }
    l2.rejects() == 1
}

/// 四路注入。
pub fn inject_four() -> [bool; 4] {
    let mut out = [false; 4];
    out[0] = compare_ok() && lex_ok();
    out[1] = eval_ok() && logic_ok();
    out[2] = numeric_ok() && compile_ok();
    out[3] = errors_ok() && ledger_ok();
    out
}

/// 判据集合。
pub fn run_unx11575_checks() -> CheckSet {
    let mut s = CheckSet::new("unx11575.test_expr");

    s.add("TE01-字节定序", compare_ok(), "static");
    s.add("TE02-词法与词长", lex_ok(), "static");
    s.add("TE03-字节串求值", eval_ok(), "static");
    s.add("TE04-逻辑与分组", logic_ok(), "static");
    s.add("TE05-整数提升", numeric_ok(), "static");
    s.add("TE06-编译形状", compile_ok(), "static");
    s.add("TE07-错误码面", errors_ok(), "static");
    s.add(
        "TE08-前缀与空串",
        {
            let a = rank(b"abc", b"ab") == 2 && rank(b"ab", b"abc") == 1;
            let b = test(&[b"", b"=", b"x"]) == Ok(false);
            let c = test(&[b"", b"<", b"x"]) == Ok(true);
            let d = test(&[b"-z", b""]) == Ok(true) && test(&[b"-n", b""]) == Ok(false);
            a && b && c && d
        },
        "static",
    );
    s.add(
        "TE09-高位字节",
        {
            let hi: &[u8] = &[0x80];
            let lo: &[u8] = b"~";
            let gt: &[u8] = b">";
            let lt: &[u8] = b"<";
            let a = test(&[hi, gt, lo]) == Ok(true);
            let b = test(&[lo, lt, hi]) == Ok(true);
            let c = test(&[&[0xff][..], b"=", &[0xff][..]]) == Ok(true);
            let d = test(&[&[0xc3, 0xa9][..], b">", &[0xc3, 0xa8][..]]) == Ok(true);
            a && b && c && d
        },
        "static",
    );
    s.add(
        "TE10-容量边界",
        {
            let a = tokenize(&[&[b'x'; WORD_CAP][..]]).is_ok();
            let b = tokenize(&[&[b'x'; WORD_CAP + 1][..]]).err() == Some(TE_ELEN);
            let c = match compile_args(&[b"x", b"=", b"x", b"-a", b"y", b"=", b"y"], Mode::Test) {
                Ok(p) => p.len() == 7,
                Err(_) => false,
            };
            let d = match tokenize(&[&b"a"[..]; TOK_CAP + 1]) {
                Err(c) => c == TE_ELEN,
                Ok(_) => false,
            };
            a && b && c && d
        },
        "static",
    );
    s.add("TE11-账本对账", build_ledger().audit() == Ok(()), "static");
    s.add(
        "TE12-冻结指纹",
        {
            let mut l = build_ledger();
            match l.freeze() {
                Ok(r) => r == l.seal() && l.is_frozen() && l.freeze() == Err(TE_EBAL),
                Err(_) => false,
            }
        },
        "static",
    );
    s.add(
        "TE13-冻结重放四注入",
        {
            let four = inject_four();
            four.get(0) == Some(&true)
                && four.get(1) == Some(&true)
                && four.get(2) == Some(&true)
                && four.get(3) == Some(&true)
        },
        "static",
    );

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn byte_order() {
        assert_eq!(cmp_bytes(b"abc", b"abc"), 0);
        assert_eq!(cmp_bytes(b"ab", b"abc"), -1);
        assert_eq!(cmp_bytes(b"abd", b"abc"), 1);
        assert_eq!(cmp_bytes(b"Z", b"a"), -1);
        assert_eq!(cmp_bytes(b"09", b"1"), -1);
        assert_eq!(cmp_bytes(&[0x80], &[0x7f]), 1);
        assert_eq!(cmp_bytes(b"", b""), 0);
        assert_eq!(rank(b"a", b"a"), 0);
    }

    #[test]
    fn words() {
        let w = Word::new(b"abc").expect("w");
        assert_eq!(w.len(), 3);
        assert_eq!(w.bytes(), b"abc");
        assert_eq!(w.text().as_str(), "abc");
        assert!(!w.is_empty());
        assert!(Word::new(b"").expect("e").is_empty());
        assert_eq!(Word::new(&[0x01]).err(), Some(TE_EBYTE));
        assert_eq!(Word::new(&[0x7f]).err(), Some(TE_EBYTE));
        assert!(Word::new(&[0x80]).is_ok());
        assert_eq!(Word::new(&[b'q'; WORD_CAP + 1]).err(), Some(TE_ELEN));
    }

    #[test]
    fn tokens() {
        let (t, n) = tokenize(&[b"=", b"!=", b"<", b">"]).expect("t");
        assert_eq!(n, 4);
        assert_eq!(t[0], Tok::StrEq);
        assert_eq!(t[1], Tok::StrNe);
        assert_eq!(t[2], Tok::StrLt);
        assert_eq!(t[3], Tok::StrGt);
        let (i, _) = tokenize(&[b"-eq", b"-le"]).expect("t");
        assert_eq!(i[0], Tok::IntEq);
        assert_eq!(i[1], Tok::IntLe);
    }

    #[test]
    fn integers() {
        assert_eq!(parse_int(b"0"), Ok(0));
        assert_eq!(parse_int(b"-42"), Ok(-42));
        assert_eq!(parse_int(b"+7"), Ok(7));
        assert_eq!(parse_int(b"2147483647"), Ok(2_147_483_647));
        assert_eq!(parse_int(b"abc").err(), Some(TE_ETOK));
        assert_eq!(parse_int(b"").err(), Some(TE_ETOK));
        assert_eq!(parse_int(b"-").err(), Some(TE_ETOK));
        assert_eq!(parse_int(b"12x").err(), Some(TE_ETOK));
        assert_eq!(parse_int(b"9999999999999999999").err(), Some(TE_EINT));
    }

    #[test]
    fn strings_compare_by_bytes() {
        assert_eq!(test(&[b"x", b"=", b"x"]), Ok(true));
        assert_eq!(test(&[b"x", b"=", b"y"]), Ok(false));
        assert_eq!(test(&[b"Z", b"<", b"a"]), Ok(true));
        assert_eq!(test(&[b"abc", b"<", b"abd"]), Ok(true));
        assert_eq!(test(&[b"ab", b"<", b"abc"]), Ok(true));
        assert_eq!(test(&[b"", b"=", b""]), Ok(true));
    }

    #[test]
    fn unary_tests() {
        assert_eq!(test(&[b"-n", b"x"]), Ok(true));
        assert_eq!(test(&[b"-n", b""]), Ok(false));
        assert_eq!(test(&[b"-z", b""]), Ok(true));
        assert_eq!(test(&[b"-z", b"x"]), Ok(false));
        assert_eq!(test(&[b"-n"]).err(), Some(TE_EARITY));
    }

    #[test]
    fn logic_and_parens() {
        assert_eq!(test(&[b"a", b"=", b"a", b"-o", b"b", b"=", b"c"]), Ok(true));
        assert_eq!(test(&[b"a", b"=", b"b", b"-a", b"c", b"=", b"c"]), Ok(false));
        assert_eq!(test(&[b"!", b"(", b"1", b"=", b"2", b")"]), Ok(true));
        assert_eq!(
            test(&[b"(", b"a", b"=", b"a", b")", b"-a", b"!", b"(", b"b", b"=", b"b", b")"]),
            Ok(false)
        );
    }

    #[test]
    fn expr_promotes_integers() {
        assert_eq!(test(&[b"10", b"<", b"9"]), Ok(true));
        assert_eq!(expr(&[b"10", b"<", b"9"]), Ok(false));
        assert_eq!(test(&[b"010", b"=", b"10"]), Ok(false));
        assert_eq!(expr(&[b"010", b"=", b"10"]), Ok(true));
        assert_eq!(expr(&[b"abc", b"<", b"abd"]), Ok(true));
        assert_eq!(expr(&[b"-5", b"-lt", b"0"]), Ok(true));
    }

    #[test]
    fn int_operators() {
        assert_eq!(test(&[b"3", b"-eq", b"3"]), Ok(true));
        assert_eq!(test(&[b"3", b"-ne", b"3"]), Ok(false));
        assert_eq!(test(&[b"4", b"-gt", b"3"]), Ok(true));
        assert_eq!(test(&[b"4", b"-lt", b"3"]), Ok(false));
        assert_eq!(test(&[b"4", b"-ge", b"4"]), Ok(true));
        assert_eq!(test(&[b"4", b"-le", b"3"]), Ok(false));
        assert_eq!(test(&[b"x", b"-eq", b"0"]).err(), Some(TE_ETOK));
    }

    #[test]
    fn program_shape() {
        let p = compile_args(&[b"x", b"=", b"x"], Mode::Test).expect("p");
        assert_eq!(p.len(), 3);
        assert_eq!(p.atom_count(), 2);
        assert_eq!(p.mode().text(), "test");
        assert_eq!(p.step_at(2), Some(Step::Op(O_SEQ)));
        assert_eq!(p.atom_bytes(0), Some(&b"x"[..]));
        assert!(p.step_at(3).is_none());
    }

    #[test]
    fn compile_errors() {
        assert_eq!(test(&[b"x", b"="]).err(), Some(TE_EARITY));
        assert_eq!(test(&[b"(", b"x"]).err(), Some(TE_EBRANCH));
        assert_eq!(test(&[b"x", b")"]).err(), Some(TE_EBRANCH));
        assert_eq!(test(&[b"x", b"=", b"y", b"z"]).err(), Some(TE_ETOK));
        assert_eq!(test(&[]).err(), Some(TE_EARITY));
        assert_eq!(expr(&[b"9999999999999999999", b"<", b"1"]).err(), Some(TE_EINT));
    }

    #[test]
    fn stack_type_errors() {
        // 手搓坏程序：字符串进整数比较，栈型必须报错而不是按 0 处理。
        let mut p = Program::blank();
        p.steps[0] = Step::Str(0);
        p.steps[1] = Step::Str(0);
        p.steps[2] = Step::Op(O_IEQ);
        p.n = 3;
        p.atoms[0] = Word::new(b"1").expect("w");
        p.na = 1;
        assert_eq!(p.run().err(), Some(TE_EOP));
    }

    #[test]
    fn ledger_shape() {
        let l = build_ledger();
        assert_eq!(l.audit(), Ok(()));
        assert_eq!(l.len(), SAMPLES);
        assert_eq!(l.ok_count(), 8);
        assert_eq!(l.err_count(), 6);
        assert_eq!(l.sample(0).map(|s| s.value), Some(1));
        assert_eq!(l.sample(5).map(|s| s.value), Some(0));
        assert_eq!(l.sample(8).map(|s| s.code), Some(TE_EARITY));
        assert_eq!(l.sample(13).map(|s| s.code), Some(TE_EBYTE));
    }

    #[test]
    fn ledger_freeze_replay() {
        let mut l = build_ledger();
        let root = l.freeze().expect("freeze");
        assert_eq!(l.replay(), Ok(root));
        assert_eq!(l.freeze(), Err(TE_EBAL));
        assert_eq!(l.register(Sample::blank()), Err(TE_ESLOT));
        assert_eq!(l.rejects(), 2);
    }

    #[test]
    fn injections() {
        let four = inject_four();
        assert!(four[0] && four[1] && four[2] && four[3]);
    }

    #[test]
    fn all_checks_green() {
        let s = run_unx11575_checks();
        assert!(s.all_passed(), "{}", s.red_items_text());
        assert_eq!(s.len(), 13);
    }
}
