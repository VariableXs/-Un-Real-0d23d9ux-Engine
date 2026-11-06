//! UNX-F11583 busybox <applet> 形式调用等价判据（同 argv 重构）
//!
//! 同一条命令有两种写法：
//!
//! ```text
//! busybox ls -l /tmp      ← 多调形态（applet 是第二参）
//! ls -l /tmp              ← 直呼形态（argv[0] 就是 applet 名）
//! ```
//!
//! 两者必须**逐字节等价**，否则装软链的机器与用 `busybox <applet>` 的机器会
//! 跑出两种行为。等价的判定不是「名字看起来一样」，而是把两种形态各自重构
//! 成**规范 argv**（applet 名占据 argv[0]，其余实参原序保留），再逐项比对：
//!
//! ```text
//! 多调  [busybox, ls, -l, /tmp]  ──丢 1 个前缀、重命名头──▶  [ls, -l, /tmp]
//! 直呼  [/bin/ls, -l, /tmp]      ──剥目录、保留基名──────────▶  [ls, -l, /tmp]
//! ```
//!
//! 关键是**只丢前缀、不重排**：busybox 里 applet 收到的 `argv` 顺序与直呼时
//! 完全一致，`--` 之后的参数、`-l` 这类以横线开头的实参都必须原样留在原位。
//! 任何「顺手去重」「顺手排序」的归一化都会把不等价的两条命令判成等价。
//!
//! 闸门次序：帧容量 → 单参字节 → 头部基名 → 形态判定 → 前缀偏移 → 规范重构 → 逐项比对。
//!
//! 本模块无 std、无 unsafe、无 panic：失败全部落 13 个错误码，账本 14 格
//! （8 成 6 败）在 `audit` 逐格对账，冻结后重放须同指纹。

use crate::alloc::string::String;
use crate::checks::CheckSet;

/// 段基址。
pub const SEG_BASE: u32 = 0xf980;

/// 账本格数。
pub const SAMPLES: usize = 14;

/// 单个实参容量。
pub const ARG_CAP: usize = 64;

/// 实参个数容量。
pub const ARGV_CAP: usize = 8;

/// 多调二进制名。
pub const BUSYBOX: &str = "busybox";

// ---------------------------------------------------------------------------
// 错误码
// ---------------------------------------------------------------------------

/// 参数不足（缺 applet 或缺 argv[0]）。
pub const AF_EFEW: u32 = SEG_BASE + 0x01;
/// 非打印字节。
pub const AF_EBYTE: u32 = SEG_BASE + 0x02;
/// 单参超长。
pub const AF_ELEN: u32 = SEG_BASE + 0x03;
/// 参数个数超容。
pub const AF_ECAP: u32 = SEG_BASE + 0x04;
/// 空参数。
pub const AF_EMPTY: u32 = SEG_BASE + 0x05;
/// 头部不是 applet（含选项形）。
pub const AF_EHEAD: u32 = SEG_BASE + 0x06;
/// 未知 applet。
pub const AF_EUNKNOWN: u32 = SEG_BASE + 0x07;
/// applet 名带斜杠。
pub const AF_ESLASH: u32 = SEG_BASE + 0x08;
/// 账本不平。
pub const AF_EBAL: u32 = SEG_BASE + 0x09;
/// 槽位非法。
pub const AF_ESLOT: u32 = SEG_BASE + 0x0a;
/// 观测值不符。
pub const AF_EVALUE: u32 = SEG_BASE + 0x0b;
/// 指纹不符。
pub const AF_ESEAL: u32 = SEG_BASE + 0x0c;
/// 无观测。
pub const AF_ENOBS: u32 = SEG_BASE + 0x0d;

// ---------------------------------------------------------------------------
// 操作码
// ---------------------------------------------------------------------------

/// argv 帧装载。
pub const OP_FRAME: u8 = 0x01;
/// 基名提取。
pub const OP_BASE: u8 = 0x02;
/// 重构。
pub const OP_REWRITE: u8 = 0x03;
/// 等价比对。
pub const OP_EQUIV: u8 = 0x04;
/// 命令线渲染。
pub const OP_CMDLINE: u8 = 0x05;
/// 偏移与丢弃账。
pub const OP_SHIFT: u8 = 0x06;

// ---------------------------------------------------------------------------
// 哈希
// ---------------------------------------------------------------------------

/// FNV 偏移基。
const FNV_OFF: u64 = 0xcbf2_9ce4_8422_2325;
/// FNV 素数。
const FNV_PRIME: u64 = 0x100_0000_01b3;

/// 折入一个字节（每字节 8 次乘法）。
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

/// 可打印（0x21..=0x7e）。
fn printable(s: &[u8]) -> bool {
    let mut i = 0usize;
    while i < s.len() {
        if let Some(b) = s.get(i) {
            if *b < 0x21 || *b > 0x7e {
                return false;
            }
        }
        i += 1;
    }
    true
}

/// 逐字节相等。
fn eq_bytes(a: &[u8], b: &[u8]) -> bool {
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

/// 推入无符号十进制。
fn push_u64(s: &mut String, mut v: u64) {
    if v == 0 {
        s.push('0');
        return;
    }
    let mut buf = [0u8; 20];
    let mut at = buf.len();
    while v > 0 {
        let d = (v % 10) as u8;
        v /= 10;
        at -= 1;
        if let Some(dst) = buf.get_mut(at) {
            *dst = b'0' + d;
        }
    }
    let mut i = at;
    while i < buf.len() {
        if let Some(b) = buf.get(i) {
            s.push(*b as char);
        }
        i += 1;
    }
}

/// 字节拼 String。
fn text_of(s: &[u8]) -> String {
    let mut out = String::new();
    let mut i = 0usize;
    while i < s.len() {
        if let Some(b) = s.get(i) {
            out.push(*b as char);
        }
        i += 1;
    }
    out
}

/// 取基名（最后一个 `/` 之后）；无斜杠时原样返回。只切不复制。
pub fn base_of(s: &[u8]) -> &[u8] {
    let mut last: Option<usize> = None;
    let mut i = 0usize;
    while i < s.len() {
        if let Some(b) = s.get(i) {
            if *b == b'/' {
                last = Some(i);
            }
        }
        i += 1;
    }
    match last {
        Some(p) => s.get(p.saturating_add(1)..).unwrap_or(&[]),
        None => s,
    }
}

/// 是否含斜杠。
fn has_slash(s: &[u8]) -> bool {
    let mut i = 0usize;
    while i < s.len() {
        if let Some(b) = s.get(i) {
            if *b == b'/' {
                return true;
            }
        }
        i += 1;
    }
    false
}

// ---------------------------------------------------------------------------
// 已知 applet 子集
// ---------------------------------------------------------------------------

/// 等价判定所覆盖的 applet（C 序，16 条）。
pub const KNOWN: [&str; 16] = [
    "awk", "cat", "cp", "echo", "grep", "head", "ls", "mkdir",
    "mv", "printf", "rm", "sed", "sh", "sort", "tail", "wc",
];

/// 是否已知 applet。
pub fn known(name: &[u8]) -> bool {
    let mut i = 0usize;
    while i < KNOWN.len() {
        if let Some(n) = KNOWN.get(i) {
            if eq_bytes(n.as_bytes(), name) {
                return true;
            }
        }
        i += 1;
    }
    false
}

/// 已知表内序号。
pub fn known_id(name: &[u8]) -> Option<usize> {
    let mut i = 0usize;
    while i < KNOWN.len() {
        if let Some(n) = KNOWN.get(i) {
            if eq_bytes(n.as_bytes(), name) {
                return Some(i);
            }
        }
        i += 1;
    }
    None
}

// ---------------------------------------------------------------------------
// 实参与 argv 帧
// ---------------------------------------------------------------------------

/// 单个实参（定容）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Arg {
    bytes: [u8; ARG_CAP],
    len: usize,
}

impl Arg {
    /// 空白。
    pub const fn blank() -> Arg {
        Arg { bytes: [0u8; ARG_CAP], len: 0 }
    }

    /// 构造：空 → `EMPTY`，超长 → `ELEN`，非打印 → `EBYTE`。
    pub fn new(s: &[u8]) -> Result<Arg, u32> {
        if s.is_empty() {
            return Err(AF_EMPTY);
        }
        if s.len() > ARG_CAP {
            return Err(AF_ELEN);
        }
        if !printable(s) {
            return Err(AF_EBYTE);
        }
        let mut a = Arg::blank();
        let mut i = 0usize;
        while i < s.len() {
            match (s.get(i), a.bytes.get_mut(i)) {
                (Some(src), Some(dst)) => *dst = *src,
                _ => return Err(AF_ELEN),
            }
            i += 1;
        }
        a.len = s.len();
        Ok(a)
    }

    /// 字节。
    pub fn bytes(&self) -> &[u8] {
        self.bytes.get(0..self.len).unwrap_or(&[])
    }

    /// 文本。
    pub fn text(&self) -> String {
        text_of(self.bytes())
    }

    /// 长度。
    pub fn len(&self) -> usize {
        self.len
    }

    /// 是否空。
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// 是否同字节。
    pub fn same_as(&self, other: &Arg) -> bool {
        eq_bytes(self.bytes(), other.bytes())
    }

    /// 是否同字面量。
    pub fn named(&self, s: &[u8]) -> bool {
        eq_bytes(self.bytes(), s)
    }
}

/// argv 帧（定容，保序）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Argv {
    args: [Arg; ARGV_CAP],
    len: usize,
}

impl Argv {
    /// 空帧。
    pub const fn new() -> Argv {
        Argv { args: [Arg::blank(); ARGV_CAP], len: 0 }
    }

    /// 追加一参。
    pub fn push(&mut self, s: &[u8]) -> Result<usize, u32> {
        if self.len >= ARGV_CAP {
            return Err(AF_ECAP);
        }
        let a = Arg::new(s)?;
        let idx = self.len;
        if let Some(dst) = self.args.get_mut(idx) {
            *dst = a;
        }
        self.len += 1;
        Ok(idx)
    }

    /// 由借用切片装配。
    pub fn from(args: &[&[u8]]) -> Result<Argv, u32> {
        if args.len() > ARGV_CAP {
            return Err(AF_ECAP);
        }
        let mut v = Argv::new();
        let mut i = 0usize;
        while i < args.len() {
            match args.get(i) {
                Some(a) => {
                    v.push(a)?;
                }
                None => return Err(AF_EFEW),
            }
            i += 1;
        }
        Ok(v)
    }

    /// 帧长。
    pub fn len(&self) -> usize {
        self.len
    }

    /// 是否空。
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// 取参。
    pub fn get(&self, i: usize) -> Option<&Arg> {
        if i >= self.len {
            return None;
        }
        self.args.get(i)
    }

    /// 头参。
    pub fn head(&self) -> Option<&Arg> {
        self.get(0)
    }

    /// 尾参（末位）。
    pub fn tail(&self) -> Option<&Arg> {
        if self.is_empty() {
            None
        } else {
            self.get(self.len.saturating_sub(1))
        }
    }

    /// 字节总量（不含分隔）。
    pub fn byte_len(&self) -> usize {
        let mut n = 0usize;
        let mut i = 0usize;
        while i < self.len {
            if let Some(a) = self.args.get(i) {
                n = n.saturating_add(a.len());
            }
            i += 1;
        }
        n
    }

    /// 空格拼接（人读）。
    pub fn text(&self) -> String {
        let mut s = String::new();
        let mut i = 0usize;
        while i < self.len {
            if let Some(a) = self.args.get(i) {
                if !s.is_empty() {
                    s.push(' ');
                }
                s.push_str(a.text().as_str());
            }
            i += 1;
        }
        s
    }

    /// `/proc/self/cmdline` 形态：每个实参后接 NUL（含末参）。
    pub fn cmdline(&self) -> String {
        let mut s = String::new();
        let mut i = 0usize;
        while i < self.len {
            if let Some(a) = self.args.get(i) {
                let mut k = 0usize;
                while k < a.len() {
                    if let Some(b) = a.bytes().get(k) {
                        s.push(*b as char);
                    }
                    k += 1;
                }
                s.push('\u{0}');
            }
            i += 1;
        }
        s
    }

    /// 逐项相等（长度 + 每参字节）。
    pub fn same_as(&self, other: &Argv) -> bool {
        if self.len != other.len {
            return false;
        }
        let mut i = 0usize;
        while i < self.len {
            match (self.args.get(i), other.args.get(i)) {
                (Some(a), Some(b)) => {
                    if !a.same_as(b) {
                        return false;
                    }
                }
                _ => return false,
            }
            i += 1;
        }
        true
    }

    /// 复制尾部到新帧（`from` 起），不重排。
    pub fn suffix(&self, from: usize) -> Result<Argv, u32> {
        if from > self.len {
            return Err(AF_EFEW);
        }
        let mut out = Argv::new();
        let mut i = from;
        while i < self.len {
            match self.args.get(i) {
                Some(a) => {
                    out.push(a.bytes())?;
                }
                None => return Err(AF_EFEW),
            }
            i += 1;
        }
        Ok(out)
    }
}

// ---------------------------------------------------------------------------
// 形态与重构
// ---------------------------------------------------------------------------

/// 调用形态。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Form {
    /// 直呼：argv[0] 是 applet。
    Direct,
    /// 多调：argv[0] 是 busybox，argv[1] 是 applet。
    Applet,
}

impl Form {
    /// 文本。
    pub fn text(&self) -> &'static str {
        match self {
            Form::Direct => "direct",
            Form::Applet => "applet",
        }
    }

    /// 前缀偏移（直呼 1，多调 2）。
    pub fn skip(&self) -> usize {
        match self {
            Form::Direct => 1,
            Form::Applet => 2,
        }
    }
}

/// 判定形态（只看头部基名，不校验 applet 是否存在）。
pub fn form_of(argv: &Argv) -> Result<Form, u32> {
    let h = match argv.head() {
        Some(v) => v,
        None => return Err(AF_EFEW),
    };
    let b = base_of(h.bytes());
    if b.is_empty() {
        return Err(AF_EMPTY);
    }
    if eq_bytes(b, BUSYBOX.as_bytes()) {
        Ok(Form::Applet)
    } else {
        Ok(Form::Direct)
    }
}

/// 规范重构：把两种形态都折叠为「applet 在 argv[0]，其余原序」。
///
/// 闸门：空帧 → 头部空基名 → 多调缺 applet → 第二参空 → 第二参以 `-` 开头
/// （那是 busybox 自己的选项）→ 第二参含斜杠 → 未知名 → 逐参装帧。
pub fn refactor(argv: &Argv) -> Result<Argv, u32> {
    if argv.is_empty() {
        return Err(AF_EFEW);
    }
    let h = match argv.head() {
        Some(v) => v,
        None => return Err(AF_EFEW),
    };
    let base = base_of(h.bytes());
    if base.is_empty() {
        return Err(AF_EMPTY);
    }
    if eq_bytes(base, BUSYBOX.as_bytes()) {
        if argv.len() < 2 {
            return Err(AF_EFEW);
        }
        let app = match argv.get(1) {
            Some(v) => v,
            None => return Err(AF_EFEW),
        };
        if app.is_empty() {
            return Err(AF_EMPTY);
        }
        if let Some(f) = app.bytes().get(0) {
            if *f == b'-' {
                return Err(AF_EHEAD);
            }
        }
        if has_slash(app.bytes()) {
            return Err(AF_ESLASH);
        }
        if !known(app.bytes()) {
            return Err(AF_EUNKNOWN);
        }
        let mut out = Argv::new();
        out.push(app.bytes())?;
        let mut i = 2usize;
        while i < argv.len() {
            match argv.get(i) {
                Some(a) => {
                    out.push(a.bytes())?;
                }
                None => return Err(AF_EFEW),
            }
            i += 1;
        }
        return Ok(out);
    }
    if !known(base) {
        return Err(AF_EUNKNOWN);
    }
    let mut out = Argv::new();
    out.push(base)?;
    let mut i = 1usize;
    while i < argv.len() {
        match argv.get(i) {
            Some(a) => {
                out.push(a.bytes())?;
            }
            None => return Err(AF_EFEW),
        }
        i += 1;
    }
    Ok(out)
}

/// 两形态是否等价：各自重构后逐项比对。
pub fn equivalent(a: &Argv, b: &Argv) -> Result<bool, u32> {
    let x = refactor(a)?;
    let y = refactor(b)?;
    Ok(x.same_as(&y))
}

/// 规范头名（重构后 argv[0] 的文本）。
pub fn canonical_head(argv: &Argv) -> Result<String, u32> {
    let r = refactor(argv)?;
    match r.head() {
        Some(a) => Ok(a.text()),
        None => Err(AF_EHEAD),
    }
}

/// 等价判定报告。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Report {
    /// 是否等价。
    pub equiv: bool,
    /// 左侧规范帧长。
    pub left_len: usize,
    /// 右侧规范帧长。
    pub right_len: usize,
    /// 左侧规范帧字节量。
    pub left_bytes: usize,
    /// 两侧被丢掉的前缀总数。
    pub dropped: usize,
    /// 左侧规范头名。
    pub head: String,
}

impl Report {
    /// 一行文本：`head=ls equiv=1 len=3 dropped=1`。
    pub fn text(&self) -> String {
        let mut s = String::new();
        s.push_str("head=");
        s.push_str(self.head.as_str());
        s.push_str(" equiv=");
        s.push(if self.equiv { '1' } else { '0' });
        s.push_str(" len=");
        push_u64(&mut s, self.left_len as u64);
        s.push_str(" dropped=");
        push_u64(&mut s, self.dropped as u64);
        s
    }
}

/// 一组对拍：直呼形态 vs 多调形态。
pub fn judge(direct: &Argv, applet: &Argv) -> Result<Report, u32> {
    let l = refactor(direct)?;
    let r = refactor(applet)?;
    let dl = direct.len().saturating_sub(l.len());
    let dr = applet.len().saturating_sub(r.len());
    let head = match l.head() {
        Some(a) => a.text(),
        None => return Err(AF_EHEAD),
    };
    Ok(Report {
        equiv: l.same_as(&r),
        left_len: l.len(),
        right_len: r.len(),
        left_bytes: l.byte_len(),
        dropped: dl.saturating_add(dr),
        head,
    })
}

/// 头部偏移表：把每格的 (形态, 偏移) 记成 `u32`。
pub fn shift_ledger(argv: &Argv) -> Result<u32, u32> {
    let f = form_of(argv)?;
    let skip = f.skip();
    if argv.len() < skip {
        return Err(AF_EFEW);
    }
    Ok(skip as u32)
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

    /// 登记。
    pub fn register(&mut self, s: Sample) -> Result<(), u32> {
        if self.frozen || (s.slot as usize) != self.len || (s.slot as usize) >= SAMPLES {
            self.rejects = self.rejects.saturating_add(1);
            return Err(AF_ESLOT);
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
        self.len.saturating_sub(self.ok_count())
    }

    /// 对账。
    pub fn audit(&self) -> Result<(), u32> {
        if self.len != SAMPLES || self.ok_count() != 8 || self.err_count() != 6 {
            return Err(AF_EBAL);
        }
        const VALS: [u32; 8] = [4, 3, 3, 2, 1, 11, 8, 2];
        const CODES: [u32; 6] = [
            AF_EFEW,
            AF_EBYTE,
            AF_ELEN,
            AF_ECAP,
            AF_EMPTY,
            AF_EUNKNOWN,
        ];
        let mut vi = 0usize;
        let mut ci = 0usize;
        let mut i = 0usize;
        while i < self.len {
            let s = match self.rows.get(i) {
                Some(v) => *v,
                None => return Err(AF_EBAL),
            };
            if s.slot as usize != i {
                return Err(AF_ESLOT);
            }
            if s.ok {
                if s.code != 0 {
                    return Err(AF_EVALUE);
                }
                match VALS.get(vi) {
                    Some(w) => {
                        if s.value != *w {
                            return Err(AF_EVALUE);
                        }
                    }
                    None => return Err(AF_EBAL),
                }
                vi += 1;
            } else {
                match CODES.get(ci) {
                    Some(w) => {
                        if s.code != *w {
                            return Err(AF_EVALUE);
                        }
                    }
                    None => return Err(AF_EBAL),
                }
                ci += 1;
            }
            i += 1;
        }
        if vi != 8 || ci != 6 {
            return Err(AF_EBAL);
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
            return Err(AF_EBAL);
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
            Err(AF_ESEAL)
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

/// `Result<usize, u32>` → 样本。
fn cell(slot: u8, op: u8, r: Result<usize, u32>) -> Sample {
    match r {
        Ok(v) => Sample { slot, op, ok: true, code: 0, value: v as u32 },
        Err(c) => Sample { slot, op, ok: false, code: c, value: 0 },
    }
}

/// `Result<u32, u32>` → 样本。
fn cell32(slot: u8, op: u8, r: Result<u32, u32>) -> Sample {
    match r {
        Ok(v) => Sample { slot, op, ok: true, code: 0, value: v },
        Err(c) => Sample { slot, op, ok: false, code: c, value: 0 },
    }
}

const D_LS: &[u8] = b"ls";
const D_BB: &[u8] = b"busybox";
const D_L: &[u8] = b"-l";
const D_TMP: &[u8] = b"/tmp";
const D_LS_PATH: &[u8] = b"/bin/ls";

/// 账本组装。
pub fn build_ledger() -> Ledger {
    let mut l = Ledger::new();

    let _ = l.register(cell(
        0,
        OP_FRAME,
        Argv::from(&[D_BB, D_LS, D_L, D_TMP]).map(|v| v.len()),
    ));

    let _ = l.register(cell(
        1,
        OP_FRAME,
        Argv::from(&[D_LS, D_L, D_TMP]).map(|v| v.len()),
    ));

    let _ = l.register(cell(
        2,
        OP_REWRITE,
        Argv::from(&[D_BB, D_LS, D_L, D_TMP])
            .and_then(|v| refactor(&v))
            .map(|v| v.len()),
    ));

    let _ = l.register(cell(
        3,
        OP_REWRITE,
        Argv::from(&[D_LS_PATH])
            .and_then(|v| refactor(&v))
            .and_then(|v| match v.head() {
                Some(a) => Ok(a.len()),
                None => Err(AF_EHEAD),
            }),
    ));

    let _ = l.register(cell(
        4,
        OP_EQUIV,
        Argv::from(&[D_BB, D_LS, D_L, D_TMP])
            .and_then(|a| Argv::from(&[D_LS, D_L, D_TMP]).map(|b| (a, b)))
            .and_then(|(a, b)| equivalent(&a, &b))
            .map(|e| if e { 1usize } else { 0usize }),
    ));

    let _ = l.register(cell(
        5,
        OP_CMDLINE,
        Argv::from(&[D_LS, D_L, D_TMP]).map(|v| v.cmdline().as_bytes().len()),
    ));

    let _ = l.register(cell(
        6,
        OP_REWRITE,
        Argv::from(&[D_BB, D_LS, D_L, D_TMP])
            .and_then(|v| refactor(&v))
            .map(|v| v.byte_len()),
    ));

    let _ = l.register(cell32(
        7,
        OP_SHIFT,
        Argv::from(&[D_BB, D_LS, D_L, D_TMP]).and_then(|v| shift_ledger(&v)),
    ));

    let _ = l.register(cell(
        8,
        OP_REWRITE,
        Argv::from(&[D_BB]).and_then(|v| refactor(&v)).map(|v| v.len()),
    ));

    let _ = l.register(cell(
        9,
        OP_FRAME,
        Argv::from(&[&[0x02u8]]).map(|v| v.len()),
    ));

    let _ = l.register(cell(
        10,
        OP_FRAME,
        Argv::from(&[&[b'x'; 65]]).map(|v| v.len()),
    ));

    let _ = l.register(cell(
        11,
        OP_FRAME,
        Argv::from(&[D_LS, D_L, D_TMP, b"a", b"b", b"c", b"d", b"e", b"f"]).map(|v| v.len()),
    ));

    let _ = l.register(cell(
        12,
        OP_REWRITE,
        Argv::from(&[D_BB, b""]).and_then(|v| refactor(&v)).map(|v| v.len()),
    ));

    let _ = l.register(cell(
        13,
        OP_REWRITE,
        Argv::from(&[b"gerp"]).and_then(|v| refactor(&v)).map(|v| v.len()),
    ));

    l
}

// ---------------------------------------------------------------------------
// 判据面
// ---------------------------------------------------------------------------

/// 帧装载。
fn frame_ok() -> bool {
    let a = match Argv::from(&[b"busybox", b"ls", b"-l", b"/tmp"]) {
        Ok(v) => v,
        Err(_) => return false,
    };
    if a.len() != 4 || a.is_empty() {
        return false;
    }
    if a.text().as_str() != "busybox ls -l /tmp" {
        return false;
    }
    if a.byte_len() != 15 {
        return false;
    }
    if a.head().map(|x| x.named(b"busybox")) != Some(true) {
        return false;
    }
    if a.tail().map(|x| x.named(b"/tmp")) != Some(true) {
        return false;
    }
    let s = match a.suffix(2) {
        Ok(v) => v,
        Err(_) => return false,
    };
    if s.len() != 2 || s.text().as_str() != "-l /tmp" {
        return false;
    }
    let copy = match Argv::from(&[b"busybox", b"ls", b"-l", b"/tmp"]) {
        Ok(v) => v,
        Err(_) => return false,
    };
    a.same_as(&copy) && !a.same_as(&s)
}

/// 基名提取。
fn base_ok() -> bool {
    eq_bytes(base_of(b"/bin/ls"), b"ls")
        && eq_bytes(base_of(b"ls"), b"ls")
        && eq_bytes(base_of(b"./ls"), b"ls")
        && eq_bytes(base_of(b"busybox"), b"busybox")
        && eq_bytes(base_of(b"/sbin/busybox"), b"busybox")
        && base_of(b"/bin/").is_empty()
        && base_of(b"").is_empty()
        && eq_bytes(base_of(b"a/b/c"), b"c")
}

/// 字节与容量闸。
fn gate_ok() -> bool {
    let a = Arg::new(b"").err() == Some(AF_EMPTY);
    let b = Arg::new(&[0x7f]).err() == Some(AF_EBYTE);
    let c = Arg::new(&[b'x'; 65]).err() == Some(AF_ELEN);
    let d = Arg::new(&[b'x'; 64]).map(|x| x.len()) == Ok(64);
    let e = Argv::from(&[b"a", b"b", b"c", b"d", b"e", b"f", b"g", b"h", b"i"]).err()
        == Some(AF_ECAP);
    let f = Argv::from(&[b"a", b"b", b"c", b"d", b"e", b"f", b"g", b"h"]).map(|v| v.len())
        == Ok(8);
    let g = Argv::from(&[]).map(|v| v.len()) == Ok(0);
    let h = Argv::from(&[b"ok", b""]).err() == Some(AF_EMPTY);
    a && b && c && d && e && f && g && h
}

/// 多调重构。
fn multi_ok() -> bool {
    let v = match Argv::from(&[b"/bin/busybox", b"ls", b"-l", b"/tmp"]) {
        Ok(x) => x,
        Err(_) => return false,
    };
    if form_of(&v) != Ok(Form::Applet) {
        return false;
    }
    let r = match refactor(&v) {
        Ok(x) => x,
        Err(_) => return false,
    };
    if r.len() != 3 || r.text().as_str() != "ls -l /tmp" {
        return false;
    }
    if r.head().map(|a| a.named(b"ls")) != Some(true) {
        return false;
    }
    if canonical_head(&v).map(|s| s.as_str() == "ls") != Ok(true) {
        return false;
    }
    let bare = match Argv::from(&[b"busybox", b"cat"]) {
        Ok(x) => x,
        Err(_) => return false,
    };
    let br = match refactor(&bare) {
        Ok(x) => x,
        Err(_) => return false,
    };
    br.len() == 1
        && br.head().map(|a| a.named(b"cat")) == Some(true)
        && refactor(&bare).map(|x| x.byte_len()) == Ok(3)
}

/// 直呼归一。
fn direct_ok() -> bool {
    let v = match Argv::from(&[b"/usr/bin/cat", b"a.txt"]) {
        Ok(x) => x,
        Err(_) => return false,
    };
    if form_of(&v) != Ok(Form::Direct) {
        return false;
    }
    let r = match refactor(&v) {
        Ok(x) => x,
        Err(_) => return false,
    };
    if r.text().as_str() != "cat a.txt" || r.len() != 2 {
        return false;
    }
    if r.head().map(|a| a.named(b"cat")) != Some(true) {
        return false;
    }
    let plain = match Argv::from(&[b"ls", b"-l", b"/tmp"]) {
        Ok(x) => x,
        Err(_) => return false,
    };
    let pr = match refactor(&plain) {
        Ok(x) => x,
        Err(_) => return false,
    };
    plain.same_as(&pr) && pr.text().as_str() == "ls -l /tmp"
}

/// 等价判定。
fn equiv_ok() -> bool {
    let a = match Argv::from(&[b"busybox", b"ls", b"-l", b"/tmp"]) {
        Ok(v) => v,
        Err(_) => return false,
    };
    let b = match Argv::from(&[b"ls", b"-l", b"/tmp"]) {
        Ok(v) => v,
        Err(_) => return false,
    };
    if equivalent(&a, &b) != Ok(true) {
        return false;
    }
    let c = match Argv::from(&[b"/bin/ls", b"-l", b"/tmp"]) {
        Ok(v) => v,
        Err(_) => return false,
    };
    if equivalent(&c, &b) != Ok(true) {
        return false;
    }
    if equivalent(&a, &c) != Ok(true) {
        return false;
    }
    let d = match Argv::from(&[b"ls", b"-l", b"/var"]) {
        Ok(v) => v,
        Err(_) => return false,
    };
    if equivalent(&b, &d) != Ok(false) {
        return false;
    }
    let e = match Argv::from(&[b"ls", b"-l"]) {
        Ok(v) => v,
        Err(_) => return false,
    };
    if equivalent(&b, &e) != Ok(false) {
        return false;
    }
    let cat = match Argv::from(&[b"busybox", b"cat", b"-l", b"/tmp"]) {
        Ok(v) => v,
        Err(_) => return false,
    };
    equivalent(&cat, &b) == Ok(false)
}

/// 命令线渲染。
fn cmdline_ok() -> bool {
    let v = match Argv::from(&[b"ls", b"-l", b"/tmp"]) {
        Ok(x) => x,
        Err(_) => return false,
    };
    let c = v.cmdline();
    if c.as_bytes().len() != 11 {
        return false;
    }
    let mut want = String::new();
    want.push_str("ls");
    want.push('\u{0}');
    want.push_str("-l");
    want.push('\u{0}');
    want.push_str("/tmp");
    want.push('\u{0}');
    if c.as_bytes() != want.as_bytes() {
        return false;
    }
    let empty = Argv::new();
    if !empty.cmdline().is_empty() {
        return false;
    }
    let one = match Argv::from(&[b"sh"]) {
        Ok(x) => x,
        Err(_) => return false,
    };
    one.cmdline().as_bytes() == b"sh\x00"
}

/// 偏移账面。
fn shift_ok() -> bool {
    let a = match Argv::from(&[b"ls", b"x"]) {
        Ok(v) => v,
        Err(_) => return false,
    };
    let b = match Argv::from(&[b"busybox", b"ls", b"x"]) {
        Ok(v) => v,
        Err(_) => return false,
    };
    shift_ledger(&a) == Ok(1)
        && shift_ledger(&b) == Ok(2)
        && Form::Direct.skip() == 1
        && Form::Applet.skip() == 2
        && Form::Applet.text() == "applet"
        && form_of(&Argv::new()).err() == Some(AF_EFEW)
}

/// 未知与缺参。
fn unknown_ok() -> bool {
    let a = Argv::from(&[b"gerp"]).and_then(|v| refactor(&v)).err() == Some(AF_EUNKNOWN);
    let b = Argv::from(&[b"busybox", b"gerp"]).and_then(|v| refactor(&v)).err()
        == Some(AF_EUNKNOWN);
    let c = Argv::from(&[b"busybox"]).and_then(|v| refactor(&v)).err() == Some(AF_EFEW);
    let d = refactor(&Argv::new()).err() == Some(AF_EFEW);
    let e = Argv::from(&[b"busybox", b"grep", b"x"]).and_then(|v| refactor(&v)).map(|v| v.len())
        == Ok(2);
    let f = Argv::from(&[b"busybox", b"LS"]).and_then(|v| refactor(&v)).err()
        == Some(AF_EUNKNOWN);
    a && b && c && d && e && f
}

/// 空参与斜杠。
fn shape_ok() -> bool {
    let a = Argv::from(&[b"busybox", b""]).and_then(|v| refactor(&v)).err() == Some(AF_EMPTY);
    let b = Argv::from(&[b"busybox", b"/bin/ls"]).and_then(|v| refactor(&v)).err()
        == Some(AF_ESLASH);
    let c = Argv::from(&[b"busybox", b"--help"]).and_then(|v| refactor(&v)).err()
        == Some(AF_EHEAD);
    let d = Argv::from(&[b"busybox", b"-"]).and_then(|v| refactor(&v)).err() == Some(AF_EHEAD);
    let e = Argv::from(&[b"/bin/"]).and_then(|v| refactor(&v)).err() == Some(AF_EMPTY);
    let f = form_of(&Argv::from(&[b"/bin/"]).unwrap_or(Argv::new())).err() == Some(AF_EMPTY);
    let g = has_slash(b"a/b") && !has_slash(b"ab");
    a && b && c && d && e && f && g
}

/// 等价报告。
fn report_ok() -> bool {
    let direct = match Argv::from(&[b"ls", b"-l", b"/tmp"]) {
        Ok(v) => v,
        Err(_) => return false,
    };
    let applet = match Argv::from(&[b"busybox", b"ls", b"-l", b"/tmp"]) {
        Ok(v) => v,
        Err(_) => return false,
    };
    let r = match judge(&direct, &applet) {
        Ok(v) => v,
        Err(_) => return false,
    };
    if !r.equiv || r.left_len != 3 || r.right_len != 3 || r.left_bytes != 8 || r.dropped != 1 {
        return false;
    }
    if r.head.as_str() != "ls" || r.text().as_str() != "head=ls equiv=1 len=3 dropped=1" {
        return false;
    }
    let other = match Argv::from(&[b"busybox", b"cat", b"-l", b"/tmp"]) {
        Ok(v) => v,
        Err(_) => return false,
    };
    match judge(&direct, &other) {
        Ok(x) => !x.equiv && x.head.as_str() == "ls",
        Err(_) => false,
    }
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
    if l2.replay() != Ok(root) {
        return false;
    }
    if l2.register(Sample::blank()) != Err(AF_ESLOT) {
        return false;
    }
    l2.rejects() == 1
}

/// 四路注入。
pub fn inject_four() -> [bool; 4] {
    let mut out = [false; 4];
    if let Some(dst) = out.get_mut(0) {
        *dst = frame_ok() && base_ok() && gate_ok();
    }
    if let Some(dst) = out.get_mut(1) {
        *dst = multi_ok() && direct_ok();
    }
    if let Some(dst) = out.get_mut(2) {
        *dst = equiv_ok() && cmdline_ok() && shift_ok();
    }
    if let Some(dst) = out.get_mut(3) {
        *dst = unknown_ok() && shape_ok() && report_ok() && ledger_ok();
    }
    out
}

/// 判据集合（13 项）。
pub fn run_unx11583_checks() -> CheckSet {
    let mut s = CheckSet::new("unx11583.applet-form");

    s.add("BF01-直呼帧装载", frame_ok(), "static");
    s.add("BF02-基名提取", base_ok(), "static");
    s.add("BF03-字节与容量闸", gate_ok(), "static");
    s.add("BF04-多调重构", multi_ok(), "static");
    s.add("BF05-直呼归一", direct_ok(), "static");
    s.add("BF06-等价判定", equiv_ok(), "static");
    s.add("BF07-命令线渲染", cmdline_ok(), "static");
    s.add("BF08-偏移账", shift_ok(), "static");
    s.add("BF09-未知与缺参", unknown_ok(), "static");
    s.add("BF10-空参与斜杠", shape_ok(), "static");
    s.add("BF11-等价报告", report_ok(), "static");
    s.add("BF12-账本对账", build_ledger().audit() == Ok(()), "static");
    s.add(
        "BF13-冻结重放四注入",
        {
            let four = inject_four();
            let mut all = true;
            let mut i = 0usize;
            while i < four.len() {
                if let Some(v) = four.get(i) {
                    if !*v {
                        all = false;
                    }
                }
                i += 1;
            }
            all
        },
        "static",
    );

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn argv_frames() {
        let v = Argv::from(&[b"busybox", b"ls", b"-l", b"/tmp"]).expect("v");
        assert_eq!(v.len(), 4);
        assert_eq!(v.byte_len(), 15);
        assert_eq!(v.text().as_str(), "busybox ls -l /tmp");
        assert_eq!(v.head().map(|a| a.text().as_str() == "busybox"), Some(true));
        assert_eq!(v.tail().map(|a| a.text().as_str() == "/tmp"), Some(true));
        assert_eq!(v.get(4), None);
        assert_eq!(v.suffix(2).map(|x| x.len()), Ok(2));
        assert_eq!(v.suffix(5).err(), Some(AF_EFEW));
    }

    #[test]
    fn argv_gates() {
        assert_eq!(Argv::from(&[b"a", b"b", b"c", b"d", b"e", b"f", b"g", b"h", b"i"]).err(), Some(AF_ECAP));
        assert_eq!(Argv::from(&[b"a", b""]).err(), Some(AF_EMPTY));
        assert_eq!(Argv::from(&[&[0x01u8]]).err(), Some(AF_EBYTE));
        assert_eq!(Arg::new(&[b'q'; 65]).err(), Some(AF_ELEN));
        assert_eq!(Arg::new(&[b'q'; 64]).map(|a| a.len()), Ok(64));
    }

    #[test]
    fn basename_split() {
        assert_eq!(base_of(b"/bin/ls"), b"ls");
        assert_eq!(base_of(b"ls"), b"ls");
        assert_eq!(base_of(b"/sbin/busybox"), b"busybox");
        assert!(base_of(b"/bin/").is_empty());
    }

    #[test]
    fn refactor_busybox() {
        let v = Argv::from(&[b"busybox", b"ls", b"-l", b"/tmp"]).expect("v");
        let r = refactor(&v).expect("r");
        assert_eq!(r.text().as_str(), "ls -l /tmp");
        assert_eq!(r.len(), 3);
        assert_eq!(r.byte_len(), 8);
        assert_eq!(form_of(&v), Ok(Form::Applet));
        assert_eq!(shift_ledger(&v), Ok(2));
    }

    #[test]
    fn refactor_direct() {
        let v = Argv::from(&[b"/usr/bin/cat", b"a.txt"]).expect("v");
        let r = refactor(&v).expect("r");
        assert_eq!(r.text().as_str(), "cat a.txt");
        assert_eq!(form_of(&v), Ok(Form::Direct));
        assert_eq!(shift_ledger(&v), Ok(1));
    }

    #[test]
    fn equivalence_true_and_false() {
        let a = Argv::from(&[b"busybox", b"ls", b"-l", b"/tmp"]).expect("a");
        let b = Argv::from(&[b"ls", b"-l", b"/tmp"]).expect("b");
        assert_eq!(equivalent(&a, &b), Ok(true));
        let c = Argv::from(&[b"/bin/ls", b"-l", b"/tmp"]).expect("c");
        assert_eq!(equivalent(&c, &b), Ok(true));
        let d = Argv::from(&[b"ls", b"-l", b"/var"]).expect("d");
        assert_eq!(equivalent(&b, &d), Ok(false));
    }

    #[test]
    fn command_line_rendering() {
        let v = Argv::from(&[b"ls", b"-l", b"/tmp"]).expect("v");
        assert_eq!(v.cmdline().as_bytes().len(), 11);
        assert_eq!(v.cmdline().as_bytes(), b"ls\x00-l\x00/tmp\x00");
        assert_eq!(Argv::from(&[b"sh"]).map(|x| x.cmdline()), Ok(String::from("sh\u{0}")));
    }

    #[test]
    fn errors_are_precise() {
        assert_eq!(
            Argv::from(&[b"busybox"]).and_then(|v| refactor(&v)).err(),
            Some(AF_EFEW)
        );
        assert_eq!(
            Argv::from(&[b"busybox", b""]).and_then(|v| refactor(&v)).err(),
            Some(AF_EMPTY)
        );
        assert_eq!(
            Argv::from(&[b"busybox", b"/bin/ls"]).and_then(|v| refactor(&v)).err(),
            Some(AF_ESLASH)
        );
        assert_eq!(
            Argv::from(&[b"busybox", b"--version"]).and_then(|v| refactor(&v)).err(),
            Some(AF_EHEAD)
        );
        assert_eq!(
            Argv::from(&[b"gerp"]).and_then(|v| refactor(&v)).err(),
            Some(AF_EUNKNOWN)
        );
    }

    #[test]
    fn report_fields() {
        let a = Argv::from(&[b"ls", b"-l", b"/tmp"]).expect("a");
        let b = Argv::from(&[b"busybox", b"ls", b"-l", b"/tmp"]).expect("b");
        let r = judge(&a, &b).expect("r");
        assert!(r.equiv);
        assert_eq!(r.dropped, 1);
        assert_eq!(r.left_bytes, 8);
        assert_eq!(r.text().as_str(), "head=ls equiv=1 len=3 dropped=1");
    }

    #[test]
    fn empty_argument_forms() {
        assert_eq!(Arg::new(b"").err(), Some(AF_EMPTY));
        assert!(Argv::new().is_empty());
        assert_eq!(form_of(&Argv::new()).err(), Some(AF_EFEW));
    }

    #[test]
    fn ledger_shape() {
        let l = build_ledger();
        assert_eq!(l.audit(), Ok(()));
        assert_eq!(l.len(), SAMPLES);
        assert_eq!(l.ok_count(), 8);
        assert_eq!(l.err_count(), 6);
        assert_eq!(l.sample(0).map(|s| s.value), Some(4));
        assert_eq!(l.sample(5).map(|s| s.value), Some(11));
        assert_eq!(l.sample(8).map(|s| s.code), Some(AF_EFEW));
        assert_eq!(l.sample(13).map(|s| s.code), Some(AF_EUNKNOWN));
    }

    #[test]
    fn ledger_freeze_replay() {
        let mut l = build_ledger();
        let root = l.freeze().expect("freeze");
        assert_eq!(l.replay(), Ok(root));
        assert_eq!(l.freeze(), Err(AF_EBAL));
        assert_eq!(l.register(Sample::blank()), Err(AF_ESLOT));
        assert_eq!(l.rejects(), 2);
        assert!(l.is_frozen());
    }

    #[test]
    fn injections_all_true() {
        assert_eq!(inject_four(), [true, true, true, true]);
    }

    #[test]
    fn check_count_is_thirteen() {
        assert_eq!(run_unx11583_checks().len(), 13);
    }

    #[test]
    fn all_checks_green() {
        let s = run_unx11583_checks();
        assert!(s.all_passed(), "{}", s.red_items_text());
    }
}
