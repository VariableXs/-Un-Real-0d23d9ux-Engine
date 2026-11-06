//! UNX-F11542 nohup 免挂断运行
//!
//! 终端一关，内核给会话首进程发 `SIGHUP`；`nohup` 做两件事让命令活下来：
//! 把 `SIGHUP` 的处置从「缺省终止」改成「忽略」，再在标准输出是终端时把
//! 输出改道到 `nohup.out`（当前目录不可写时退到 `$HOME/nohup.out`）。两件
//! 事都不改命令本身，只改它继承的三张表：信号处置表、描述符表、工作目录。
//!
//! 这单把两件事都做成可判定的纯模型：
//!
//! 1. **信号处置表**。十种常见信号各有一格处置，初始全是缺省；建立计划时
//!    只把 `HUP` 一格改成忽略，其余九格必须原样保留。忽略是单向的：计划
//!    建好之后不允许把 `HUP` 改回缺省，否则「免挂断」这个承诺就被撤销了。
//! 2. **改道判定**。标准输出是终端：`stdout` 与 `stderr` 都追加到输出文件；
//!    标准输出不是终端、标准错误是终端：只把 `stderr` 接到 `stdout`；
//!    两者都不是终端：一律不动。三条分支必须互斥且可复现。
//! 3. **输出文件**。当前目录可写用 `nohup.out`，否则用 `$HOME/nohup.out`；
//!    两者都不可用要报缺失，绝不能悄悄把输出丢掉。文件已有内容时是追加，
//!    空文件或新建时不算追加，状态行必须把这件事说出来。
//!
//! 闸门次序：未知选项（`NH_EOPT`）→ 缺命令（`NH_ECMD`）→ 名称字节
//! （`NH_EBYTE`）→ 命令过多（`NH_ELEN`）→ 输出文件不可用（`NH_ENOENT`）→
//! 处置回退（`NH_ESTATE`）→ 超长路径（`NH_ELEN`）→ 数值不符（`NH_EVALUE`）。
//!
//! 账本语义：十四格 = 八格成功（被忽略信号数、改道条数、输出名长度、追加
//! 标志、命令条数、信号总数、找不到命令的退出码、状态行数）+ 六格失败
//! （缺命令、未知选项、名称字节、命令过多、输出文件不可用、处置回退）。
//! `audit` 认「八值六码」，`seal` 做 FNV-1a 折叠，`replay` 重建比对，
//! `freeze` 后拒绝登记。

use crate::alloc::string::String;
use crate::checks::CheckSet;

/// 段基址。
pub const SEG_BASE: u32 = 0xf2f0;

/// 缺少命令。
pub const NH_ECMD: u32 = SEG_BASE + 0x01;
/// 未知选项。
pub const NH_EOPT: u32 = SEG_BASE + 0x02;
/// 名称字节非法。
pub const NH_EBYTE: u32 = SEG_BASE + 0x03;
/// 名称过长。
pub const NH_ELEN: u32 = SEG_BASE + 0x04;
/// 处置回退。
pub const NH_ESTATE: u32 = SEG_BASE + 0x05;
/// 输出文件不可用。
pub const NH_ENOENT: u32 = SEG_BASE + 0x06;
/// 数值溢出。
pub const NH_ERANGE: u32 = SEG_BASE + 0x07;
/// 指针或描述符非法。
pub const NH_EPTR: u32 = SEG_BASE + 0x08;
/// 账本失衡。
pub const NH_EBAL: u32 = SEG_BASE + 0x09;
/// 槽位错位或容量溢出。
pub const NH_ESLOT: u32 = SEG_BASE + 0x0a;
/// 值不符。
pub const NH_EVALUE: u32 = SEG_BASE + 0x0b;
/// 指纹不符。
pub const NH_ESEAL: u32 = SEG_BASE + 0x0c;
/// 观测缺失。
pub const NH_ENOBS: u32 = SEG_BASE + 0x0d;

/// 账本格数。
pub const SAMPLES: usize = 14;

/// 信号总数。
pub const SIG_COUNT: usize = 10;

/// 命令容量。
pub const CMD_CAP: usize = 4;

/// 名称容量。
pub const NAME_CAP: usize = 48;

/// 缺省输出文件名。
pub const DEFAULT_OUT: &[u8] = b"nohup.out";

/// 找不到命令的退出码。
pub const EXIT_NOT_FOUND: u32 = 127;

/// 操作码：信号表。
pub const OP_SIG: u8 = 0x01;
/// 操作码：改道。
pub const OP_REDIR: u8 = 0x02;
/// 操作码：选文件。
pub const OP_PATH: u8 = 0x03;
/// 操作码：解析命令。
pub const OP_ARGV: u8 = 0x04;
/// 操作码：状态行。
pub const OP_STATUS: u8 = 0x05;
/// 操作码：退出码。
pub const OP_EXIT: u8 = 0x06;
/// 操作码：追加。
pub const OP_APPEND: u8 = 0x07;
/// 操作码：容量。
pub const OP_SLOT: u8 = 0x08;

/// FNV-1a 偏移。
pub const FNV_OFF: u64 = 0xcbf2_9ce4_8422_2325;
/// FNV-1a 质数。
pub const FNV_PRIME: u64 = 0x100_0000_01b3;

/// 折入一个字节。
fn fold(h: u64, b: u64) -> u64 {
    let mut v = h ^ (b & 0xff);
    let mut i = 0u32;
    while i < 8 {
        v = v.wrapping_mul(FNV_PRIME);
        i += 1;
    }
    v
}

/// 十进制拼接。
fn push_dec(s: &mut String, mut v: u64) {
    if v == 0 {
        s.push('0');
        return;
    }
    let mut buf = [0u8; 20];
    let mut i = 20usize;
    while v > 0 && i > 0 {
        i -= 1;
        buf[i] = b'0' + (v % 10) as u8;
        v /= 10;
    }
    let mut j = i;
    while j < 20 {
        if let Some(b) = buf.get(j) {
            s.push(*b as char);
        }
        j += 1;
    }
}

/// 字节相等。
fn eq_bytes(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut i = 0usize;
    while i < a.len() {
        let x = match a.get(i) {
            Some(v) => *v,
            None => return false,
        };
        let y = match b.get(i) {
            Some(v) => *v,
            None => return false,
        };
        if x != y {
            return false;
        }
        i += 1;
    }
    true
}

/// 前缀。
fn starts_with(a: &[u8], p: &[u8]) -> bool {
    if a.len() < p.len() {
        return false;
    }
    let head = match a.get(0..p.len()) {
        Some(v) => v,
        None => return false,
    };
    eq_bytes(head, p)
}

/// 名称字节是否可呈现。
fn printable(s: &[u8]) -> bool {
    if s.is_empty() {
        return false;
    }
    let mut i = 0usize;
    while i < s.len() {
        let b = match s.get(i) {
            Some(v) => *v,
            None => return false,
        };
        if b < 0x21 || b > 0x7e {
            return false;
        }
        i += 1;
    }
    true
}

// ---------------------------------------------------------------------------
// 信号
// ---------------------------------------------------------------------------

/// 十种常见信号。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Signal {
    /// 挂断。
    Hup,
    /// 中断。
    Int,
    /// 退出。
    Quit,
    /// 终止。
    Term,
    /// 管道关闭。
    Pipe,
    /// 闹钟。
    Alrm,
    /// 子进程状态变化。
    Chld,
    /// 用户自定义一。
    Usr1,
    /// 用户自定义二。
    Usr2,
    /// 继续。
    Cont,
}

impl Signal {
    /// 全表。
    pub fn all() -> [Signal; SIG_COUNT] {
        [
            Signal::Hup,
            Signal::Int,
            Signal::Quit,
            Signal::Term,
            Signal::Pipe,
            Signal::Alrm,
            Signal::Chld,
            Signal::Usr1,
            Signal::Usr2,
            Signal::Cont,
        ]
    }

    /// 下标。
    pub fn index(&self) -> usize {
        match self {
            Signal::Hup => 0,
            Signal::Int => 1,
            Signal::Quit => 2,
            Signal::Term => 3,
            Signal::Pipe => 4,
            Signal::Alrm => 5,
            Signal::Chld => 6,
            Signal::Usr1 => 7,
            Signal::Usr2 => 8,
            Signal::Cont => 9,
        }
    }

    /// 文本。
    pub fn text(&self) -> &'static str {
        match self {
            Signal::Hup => "HUP",
            Signal::Int => "INT",
            Signal::Quit => "QUIT",
            Signal::Term => "TERM",
            Signal::Pipe => "PIPE",
            Signal::Alrm => "ALRM",
            Signal::Chld => "CHLD",
            Signal::Usr1 => "USR1",
            Signal::Usr2 => "USR2",
            Signal::Cont => "CONT",
        }
    }

    /// 缺省处置。
    pub fn default_disposition(&self) -> Disposition {
        Disposition::Default
    }

    /// 缺省处置是否致命。
    pub fn fatal_by_default(&self) -> bool {
        match self {
            Signal::Chld | Signal::Cont | Signal::Usr1 | Signal::Usr2 => false,
            _ => true,
        }
    }
}

/// 处置。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Disposition {
    /// 缺省。
    Default,
    /// 忽略。
    Ignore,
}

impl Disposition {
    /// 文本。
    pub fn text(&self) -> &'static str {
        match self {
            Disposition::Default => "default",
            Disposition::Ignore => "ignore",
        }
    }
}

/// 信号处置表。
#[derive(Clone, Copy, Debug)]
pub struct SignalTable {
    rows: [Disposition; SIG_COUNT],
}

impl SignalTable {
    /// 全缺省。
    pub const fn new() -> SignalTable {
        SignalTable { rows: [Disposition::Default; SIG_COUNT] }
    }

    /// 取处置。
    pub fn get(&self, sig: Signal) -> Disposition {
        self.rows.get(sig.index()).copied().unwrap_or(Disposition::Default)
    }

    /// 设处置。
    pub fn set(&mut self, sig: Signal, d: Disposition) {
        if let Some(dst) = self.rows.get_mut(sig.index()) {
            *dst = d;
        }
    }

    /// 忽略某信号。
    pub fn ignore(&mut self, sig: Signal) {
        self.set(sig, Disposition::Ignore);
    }

    /// 是否忽略。
    pub fn is_ignored(&self, sig: Signal) -> bool {
        self.get(sig) == Disposition::Ignore
    }

    /// 被忽略的信号数。
    pub fn ignored_count(&self) -> usize {
        let mut n = 0usize;
        let mut i = 0usize;
        while i < SIG_COUNT {
            if let Some(d) = self.rows.get(i) {
                if *d == Disposition::Ignore {
                    n += 1;
                }
            }
            i += 1;
        }
        n
    }

    /// 渲染，每条一行 `<SIG> <disp>`。
    pub fn render(&self) -> String {
        let mut s = String::new();
        let all = Signal::all();
        let mut i = 0usize;
        while i < all.len() {
            if let Some(sig) = all.get(i) {
                s.push_str(sig.text());
                s.push(' ');
                s.push_str(self.get(*sig).text());
                s.push('\n');
            }
            i += 1;
        }
        s
    }
}

// ---------------------------------------------------------------------------
// 名称
// ---------------------------------------------------------------------------

/// 路径或命令名。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Name {
    bytes: [u8; NAME_CAP],
    len: usize,
}

impl Name {
    /// 空白。
    pub const fn blank() -> Name {
        Name { bytes: [0u8; NAME_CAP], len: 0 }
    }

    /// 构造。
    pub fn new(s: &[u8]) -> Result<Name, u32> {
        if s.is_empty() {
            return Err(NH_EBYTE);
        }
        if s.len() > NAME_CAP {
            return Err(NH_ELEN);
        }
        if !printable(s) {
            return Err(NH_EBYTE);
        }
        let mut n = Name::blank();
        let mut i = 0usize;
        while i < s.len() {
            if let Some(b) = s.get(i) {
                if let Some(dst) = n.bytes.get_mut(i) {
                    *dst = *b;
                }
            }
            i += 1;
        }
        n.len = s.len();
        Ok(n)
    }

    /// 字节。
    pub fn bytes(&self) -> &[u8] {
        self.bytes.get(0..self.len).unwrap_or(&[])
    }

    /// 文本。
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

    /// 长度。
    pub fn len(&self) -> usize {
        self.len
    }

    /// 是否空。
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// 是否同名。
    pub fn named(&self, other: &[u8]) -> bool {
        eq_bytes(self.bytes(), other)
    }
}

// ---------------------------------------------------------------------------
// 改道
// ---------------------------------------------------------------------------

/// 描述符落点。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Target {
    /// 终端。
    Terminal,
    /// 文件。
    File,
    /// 关闭。
    Closed,
}

impl Target {
    /// 文本。
    pub fn text(&self) -> &'static str {
        match self {
            Target::Terminal => "tty",
            Target::File => "file",
            Target::Closed => "closed",
        }
    }
}

/// 一道改道。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Redir {
    /// 不动。
    Unchanged,
    /// 接到标准输出。
    ToStdout,
    /// 追加到文件。
    AppendFile,
}

impl Redir {
    /// 是否真的改道。
    pub fn is_redirect(&self) -> bool {
        match self {
            Redir::Unchanged => false,
            Redir::ToStdout => true,
            Redir::AppendFile => true,
        }
    }

    /// 文本。
    pub fn text(&self) -> &'static str {
        match self {
            Redir::Unchanged => "unchanged",
            Redir::ToStdout => "stdout",
            Redir::AppendFile => "file",
        }
    }
}

/// 选择输出文件：当前目录可写用 `nohup.out`，否则退到 `$HOME/nohup.out`。
pub fn choose_output(cwd_writable: bool, home: &[u8]) -> Result<Name, u32> {
    if cwd_writable {
        return Name::new(DEFAULT_OUT);
    }
    if home.is_empty() {
        return Err(NH_ENOENT);
    }
    if !printable(home) {
        return Err(NH_EBYTE);
    }
    if home.len() + 1 + DEFAULT_OUT.len() > NAME_CAP {
        return Err(NH_ELEN);
    }
    let mut buf = [0u8; NAME_CAP];
    let mut at = 0usize;
    let mut i = 0usize;
    while i < home.len() {
        if let Some(b) = home.get(i) {
            if let Some(dst) = buf.get_mut(at) {
                *dst = *b;
            }
        }
        at += 1;
        i += 1;
    }
    if let Some(dst) = buf.get_mut(at) {
        *dst = b'/';
    }
    at += 1;
    let mut j = 0usize;
    while j < DEFAULT_OUT.len() {
        if let Some(b) = DEFAULT_OUT.get(j) {
            if let Some(dst) = buf.get_mut(at) {
                *dst = *b;
            }
        }
        at += 1;
        j += 1;
    }
    Name::new(buf.get(0..at).unwrap_or(&[]))
}

// ---------------------------------------------------------------------------
// 计划
// ---------------------------------------------------------------------------

/// 一次 `nohup` 的执行计划。
#[derive(Clone, Copy, Debug)]
pub struct Plan {
    table: SignalTable,
    stdout: Redir,
    stderr: Redir,
    path: Name,
    append: bool,
    out_tty: Target,
    err_tty: Target,
    cmds: [Name; CMD_CAP],
    len: usize,
    end: bool,
    argv_seen: bool,
}

impl Plan {
    /// 空白计划。
    pub const fn blank() -> Plan {
        Plan {
            table: SignalTable::new(),
            stdout: Redir::Unchanged,
            stderr: Redir::Unchanged,
            path: Name::blank(),
            append: false,
            out_tty: Target::File,
            err_tty: Target::File,
            cmds: [Name::blank(); CMD_CAP],
            len: 0,
            end: false,
            argv_seen: false,
        }
    }

    /// 信号表。
    pub fn table(&self) -> SignalTable {
        self.table
    }

    /// 标准输出改道。
    pub fn stdout(&self) -> Redir {
        self.stdout
    }

    /// 标准错误改道。
    pub fn stderr(&self) -> Redir {
        self.stderr
    }

    /// 输出文件。
    pub fn path(&self) -> Name {
        self.path
    }

    /// 是否追加。
    pub fn appending(&self) -> bool {
        self.append
    }

    /// 标准输出落点。
    pub fn out_target(&self) -> Target {
        self.out_tty
    }

    /// 标准错误落点。
    pub fn err_target(&self) -> Target {
        self.err_tty
    }

    /// 命令数。
    pub fn len(&self) -> usize {
        self.len
    }

    /// 是否无命令。
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// 取命令。
    pub fn at(&self, i: usize) -> Option<Name> {
        if i >= self.len {
            return None;
        }
        self.cmds.get(i).copied()
    }

    /// 首命令。
    pub fn first(&self) -> Option<Name> {
        self.at(0)
    }

    /// 命令整行。
    pub fn command_text(&self) -> String {
        let mut s = String::new();
        let mut i = 0usize;
        while i < self.len {
            if let Some(c) = self.cmds.get(i) {
                if i > 0 {
                    s.push(' ');
                }
                s.push_str(c.text().as_str());
            }
            i += 1;
        }
        s
    }

    /// 是否见过 `--`。
    pub fn end_seen(&self) -> bool {
        self.end
    }

    /// 是否收过参数。
    pub fn argv_seen(&self) -> bool {
        self.argv_seen
    }

    /// 改道条数。
    pub fn redirect_count(&self) -> usize {
        let a = if self.stdout.is_redirect() { 1usize } else { 0usize };
        let b = if self.stderr.is_redirect() { 1usize } else { 0usize };
        a + b
    }

    /// 状态行：三行，忽略输入、标准输出去向、标准错误去向。
    pub fn status_text(&self) -> String {
        let mut s = String::new();
        s.push_str("ignoring input\n");
        s.push_str("stdout ");
        match self.stdout {
            Redir::Unchanged => s.push_str("unchanged\n"),
            Redir::AppendFile => {
                if self.append {
                    s.push_str("appending ");
                } else {
                    s.push_str("creating ");
                }
                s.push_str(self.path.text().as_str());
                s.push('\n');
            }
            Redir::ToStdout => s.push_str("stdout\n"),
        }
        s.push_str("stderr ");
        match self.stderr {
            Redir::Unchanged => s.push_str("unchanged\n"),
            Redir::AppendFile => {
                if self.append {
                    s.push_str("appending ");
                } else {
                    s.push_str("creating ");
                }
                s.push_str(self.path.text().as_str());
                s.push('\n');
            }
            Redir::ToStdout => s.push_str("stdout\n"),
        }
        s
    }

    /// 状态行数。
    pub fn status_lines(&self) -> usize {
        let mut n = 0usize;
        let text = self.status_text();
        let bytes = text.as_str().as_bytes();
        let mut i = 0usize;
        while i < bytes.len() {
            if let Some(b) = bytes.get(i) {
                if *b == b'\n' {
                    n += 1;
                }
            }
            i += 1;
        }
        n
    }

    /// 收到某信号后是否存活：被忽略者当然存活，缺省非致命者也存活。
    pub fn survives(&self, sig: Signal) -> bool {
        if self.table.is_ignored(sig) {
            return true;
        }
        !sig.fatal_by_default()
    }

    /// 撤销 `HUP` 忽略。`nohup` 的承诺一旦给出就不收回，因此恒失败。
    pub fn restore_hup(&mut self) -> Result<(), u32> {
        Err(NH_ESTATE)
    }

    /// 退出码：找不到命令是 127；否则被命令自己的结果决定。
    pub fn exit_code(&self, exec_ok: bool) -> u32 {
        if !exec_ok {
            return EXIT_NOT_FOUND;
        }
        0
    }

    /// 解析一条命令名。
    fn push_cmd(&mut self, raw: &[u8]) -> Result<(), u32> {
        if self.len >= CMD_CAP {
            return Err(NH_ELEN);
        }
        let n = Name::new(raw)?;
        if let Some(dst) = self.cmds.get_mut(self.len) {
            *dst = n;
        }
        self.len += 1;
        Ok(())
    }
}

/// 建立计划。
pub fn build_plan(
    args: &[&[u8]],
    stdout: Target,
    stderr: Target,
    cwd_writable: bool,
    home: &[u8],
    existing_len: usize,
) -> Result<Plan, u32> {
    let mut p = Plan::blank();
    p.out_tty = stdout;
    p.err_tty = stderr;
    let mut i = 0usize;
    let mut operands = false;
    while i < args.len() {
        let a = match args.get(i) {
            Some(v) => *v,
            None => return Err(NH_ECMD),
        };
        if !operands {
            if eq_bytes(a, b"--") {
                p.end = true;
                operands = true;
                i += 1;
                continue;
            }
            if starts_with(a, b"--") {
                return Err(NH_EOPT);
            }
            if !a.is_empty() && a.first() == Some(&b'-') {
                return Err(NH_EOPT);
            }
            operands = true;
        }
        p.push_cmd(a)?;
        p.argv_seen = true;
        i += 1;
    }
    if p.len == 0 {
        return Err(NH_ECMD);
    }
    p.table.ignore(Signal::Hup);
    if stdout == Target::Terminal || stderr == Target::Terminal {
        p.path = choose_output(cwd_writable, home)?;
        p.append = existing_len > 0;
    }
    if stdout == Target::Terminal {
        p.stdout = Redir::AppendFile;
        p.stderr = Redir::AppendFile;
    } else if stderr == Target::Terminal {
        p.stdout = Redir::Unchanged;
        p.stderr = Redir::ToStdout;
    } else {
        p.stdout = Redir::Unchanged;
        p.stderr = Redir::Unchanged;
    }
    Ok(p)
}

// ---------------------------------------------------------------------------
// 账本观测
// ---------------------------------------------------------------------------

/// 被忽略信号数探针。
fn ignored_probe() -> Result<usize, u32> {
    let p = build_plan(&[b"sleep", b"5"], Target::File, Target::File, true, b"", 0)?;
    Ok(p.table().ignored_count())
}

/// 改道条数探针。
fn redirect_probe() -> Result<usize, u32> {
    let p = build_plan(&[b"sleep"], Target::Terminal, Target::Terminal, true, b"", 0)?;
    Ok(p.redirect_count())
}

/// 输出名长度探针。
fn path_len_probe() -> Result<usize, u32> {
    let p = build_plan(&[b"sleep"], Target::Terminal, Target::File, true, b"", 0)?;
    Ok(p.path().len())
}

/// 追加标志探针。
fn append_probe() -> Result<u32, u32> {
    let p = build_plan(&[b"sleep"], Target::Terminal, Target::Terminal, true, b"", 64)?;
    if p.appending() {
        Ok(1)
    } else {
        Err(NH_EVALUE)
    }
}

/// 命令数探针。
fn cmd_probe() -> Result<usize, u32> {
    let p = build_plan(&[b"sh", b"-c"], Target::File, Target::File, true, b"", 0)?;
    Ok(p.len())
}

/// 信号数探针。
fn signal_probe() -> Result<usize, u32> {
    Ok(Signal::all().len())
}

/// 退出码探针。
fn exit_probe() -> Result<u32, u32> {
    let p = build_plan(&[b"missing"], Target::File, Target::File, true, b"", 0)?;
    Ok(p.exit_code(false))
}

/// 状态行数探针。
fn status_probe() -> Result<usize, u32> {
    let p = build_plan(&[b"sleep"], Target::Terminal, Target::Terminal, true, b"", 0)?;
    Ok(p.status_lines())
}

/// 缺命令探针。
fn missing_probe() -> Result<u32, u32> {
    build_plan(&[], Target::File, Target::File, true, b"", 0).map(|_| 0)
}

/// 未知选项探针。
fn opt_probe() -> Result<u32, u32> {
    build_plan(&[b"--bogus", b"x"], Target::File, Target::File, true, b"", 0).map(|_| 0)
}

/// 名称字节探针。
fn byte_probe() -> Result<u32, u32> {
    build_plan(&[b"\x01"], Target::File, Target::File, true, b"", 0).map(|_| 0)
}

/// 命令过多探针。
fn len_probe() -> Result<u32, u32> {
    build_plan(&[b"a", b"b", b"c", b"d", b"e"], Target::File, Target::File, true, b"", 0)
        .map(|_| 0)
}

/// 输出文件不可用探针。
fn noent_probe() -> Result<u32, u32> {
    build_plan(&[b"sleep"], Target::Terminal, Target::File, false, b"", 0).map(|_| 0)
}

/// 处置回退探针。
fn state_probe() -> Result<u32, u32> {
    let mut p = build_plan(&[b"sleep"], Target::File, Target::File, true, b"", 0)?;
    p.restore_hup().map(|_| 0)
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
            return Err(NH_ESLOT);
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

    /// 取一格。
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
            return Err(NH_EBAL);
        }
        const VALS: [u32; 8] = [1, 2, 9, 1, 2, 10, 127, 3];
        const CODES: [u32; 6] = [NH_ECMD, NH_EOPT, NH_EBYTE, NH_ELEN, NH_ENOENT, NH_ESTATE];
        let mut vi = 0usize;
        let mut ci = 0usize;
        let mut i = 0usize;
        while i < self.len {
            let s = match self.rows.get(i) {
                Some(v) => *v,
                None => return Err(NH_EBAL),
            };
            if s.slot as usize != i {
                return Err(NH_ESLOT);
            }
            if s.ok {
                if s.code != 0 {
                    return Err(NH_EVALUE);
                }
                match VALS.get(vi) {
                    Some(w) => {
                        if s.value != *w {
                            return Err(NH_EVALUE);
                        }
                    }
                    None => return Err(NH_EBAL),
                }
                vi += 1;
            } else {
                match CODES.get(ci) {
                    Some(w) => {
                        if s.code != *w {
                            return Err(NH_EVALUE);
                        }
                    }
                    None => return Err(NH_EBAL),
                }
                ci += 1;
            }
            i += 1;
        }
        if vi != 8 || ci != 6 {
            return Err(NH_EBAL);
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
            return Err(NH_EBAL);
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
            Err(NH_ESEAL)
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

/// 建账本。
pub fn build_ledger() -> Ledger {
    let mut l = Ledger::new();

    let _ = l.register(count_cell(0, OP_SIG, ignored_probe()));

    let _ = l.register(count_cell(1, OP_REDIR, redirect_probe()));

    let _ = l.register(count_cell(2, OP_PATH, path_len_probe()));

    let _ = l.register(word_cell(3, OP_APPEND, append_probe()));

    let _ = l.register(count_cell(4, OP_ARGV, cmd_probe()));

    let _ = l.register(count_cell(5, OP_SIG, signal_probe()));

    let _ = l.register(word_cell(6, OP_EXIT, exit_probe()));

    let _ = l.register(count_cell(7, OP_STATUS, status_probe()));

    let _ = l.register(word_cell(8, OP_ARGV, missing_probe()));

    let _ = l.register(word_cell(9, OP_ARGV, opt_probe()));

    let _ = l.register(word_cell(10, OP_ARGV, byte_probe()));

    let _ = l.register(word_cell(11, OP_ARGV, len_probe()));

    let _ = l.register(word_cell(12, OP_PATH, noent_probe()));

    let _ = l.register(word_cell(13, OP_SIG, state_probe()));

    l
}

// ---------------------------------------------------------------------------
// 注入判据
// ---------------------------------------------------------------------------

/// 信号表。
fn signal_ok() -> bool {
    let t = SignalTable::new();
    let a = t.ignored_count() == 0;
    let b = t.get(Signal::Hup) == Disposition::Default;
    let c = Signal::all().len() == SIG_COUNT;
    let p = match build_plan(&[b"sleep"], Target::File, Target::File, true, b"", 0) {
        Ok(v) => v,
        Err(_) => Plan::blank(),
    };
    let d = p.table().ignored_count() == 1;
    let e = p.table().is_ignored(Signal::Hup);
    let f = !p.table().is_ignored(Signal::Int);
    let g = p.survives(Signal::Hup);
    let h = !p.survives(Signal::Term);
    a && b && c && d && e && f && g && h
}

/// 改道三分支。
fn tty_ok() -> bool {
    let both = match build_plan(&[b"x"], Target::Terminal, Target::Terminal, true, b"", 0) {
        Ok(v) => v,
        Err(_) => Plan::blank(),
    };
    let a = both.stdout() == Redir::AppendFile && both.stderr() == Redir::AppendFile;
    let b = both.redirect_count() == 2;
    let only_err = match build_plan(&[b"x"], Target::File, Target::Terminal, true, b"", 0) {
        Ok(v) => v,
        Err(_) => Plan::blank(),
    };
    let c = only_err.stdout() == Redir::Unchanged && only_err.stderr() == Redir::ToStdout;
    let d = only_err.redirect_count() == 1;
    let none = match build_plan(&[b"x"], Target::File, Target::Closed, true, b"", 0) {
        Ok(v) => v,
        Err(_) => Plan::blank(),
    };
    let e = none.stdout() == Redir::Unchanged && none.stderr() == Redir::Unchanged;
    let f = none.redirect_count() == 0;
    a && b && c && d && e && f
}

/// 输出文件选择。
fn path_ok() -> bool {
    let a = choose_output(true, b"") == Ok(Name::new(b"nohup.out").unwrap_or(Name::blank()));
    let b = match choose_output(false, b"/home/u") {
        Ok(n) => n.text().as_str() == "/home/u/nohup.out",
        Err(_) => false,
    };
    let c = choose_output(false, b"").err() == Some(NH_ENOENT);
    let d = choose_output(false, b"bad home").err() == Some(NH_EBYTE);
    let long_home = [b'h'; 48];
    let e = choose_output(false, &long_home).err() == Some(NH_ELEN);
    a && b && c && d && e
}

/// 参数解析。
fn argv_ok() -> bool {
    let a = match build_plan(&[b"sh", b"-c"], Target::File, Target::File, true, b"", 0) {
        Ok(v) => v.len() == 2 && v.command_text().as_str() == "sh -c",
        Err(_) => false,
    };
    let b = match build_plan(&[b"--", b"-x"], Target::File, Target::File, true, b"", 0) {
        Ok(v) => v.end_seen() && v.len() == 1 && v.at(0).map(|n| n.named(b"-x")).unwrap_or(false),
        Err(_) => false,
    };
    let c = build_plan(&[], Target::File, Target::File, true, b"", 0).err() == Some(NH_ECMD);
    let d = build_plan(&[b"--bogus"], Target::File, Target::File, true, b"", 0).err() == Some(NH_EOPT);
    let e = build_plan(&[b"-"], Target::File, Target::File, true, b"", 0).err() == Some(NH_EOPT);
    let f = build_plan(&[b"\x01"], Target::File, Target::File, true, b"", 0).err() == Some(NH_EBYTE);
    let g = build_plan(&[b"a", b"b", b"c", b"d", b"e"], Target::File, Target::File, true, b"", 0).err()
        == Some(NH_ELEN);
    a && b && c && d && e && f && g
}

/// 追加与状态。
fn append_ok() -> bool {
    let fresh = match build_plan(&[b"x"], Target::Terminal, Target::File, true, b"", 0) {
        Ok(v) => v,
        Err(_) => Plan::blank(),
    };
    let a = !fresh.appending();
    let b = fresh.status_text().as_str().contains("stdout creating nohup.out");
    let old = match build_plan(&[b"x"], Target::Terminal, Target::File, true, b"", 128) {
        Ok(v) => v,
        Err(_) => Plan::blank(),
    };
    let c = old.appending();
    let d = old.status_text().as_str().contains("stdout appending nohup.out");
    let e = old.status_lines() == 3;
    let f = old.status_text().as_str().starts_with("ignoring input\n");
    a && b && c && d && e && f
}

/// 退出码。
fn exit_ok() -> bool {
    let p = match build_plan(&[b"x"], Target::File, Target::File, true, b"", 0) {
        Ok(v) => v,
        Err(_) => Plan::blank(),
    };
    p.exit_code(true) == 0 && p.exit_code(false) == EXIT_NOT_FOUND
}

/// 渲染与容量。
fn render_ok() -> bool {
    let mut p = match build_plan(&[b"x"], Target::Terminal, Target::Terminal, true, b"", 8) {
        Ok(v) => v,
        Err(_) => Plan::blank(),
    };
    let a = p.table().render().as_str().starts_with("HUP ignore\n");
    let b = p.table().render().as_str().contains("INT default\n");
    let c = p.table().render().as_str().matches('\n').count() == SIG_COUNT;
    let d = p.stderr().text() == "file";
    let e = p.out_target() == Target::Terminal;
    let f = p.restore_hup().err() == Some(NH_ESTATE);
    let g = {
        let mut q = p;
        q.restore_hup().err() == Some(NH_ESTATE) && q.table().is_ignored(Signal::Hup)
    };
    a && b && c && d && e && f && g
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
    let probe = Sample { slot: 0, op: 0, ok: false, code: 0, value: 0 };
    if l2.register(probe) != Err(NH_ESLOT) {
        return false;
    }
    l2.rejects() == 1
}

/// 四路注入。
pub fn inject_four() -> [bool; 4] {
    let mut out = [false; 4];
    out[0] = signal_ok() && tty_ok();
    out[1] = path_ok() && argv_ok();
    out[2] = ledger_ok();
    out[3] = append_ok() && exit_ok() && render_ok();
    out
}

// ---------------------------------------------------------------------------
// 判据
// ---------------------------------------------------------------------------

/// 判据集合。
pub fn run_unx11542_checks() -> CheckSet {
    let mut s = CheckSet::new("unx11542.nohup");

    s.add(
        "NH01-信号忽略",
        {
            signal_ok()
        },
        "static",
    );

    s.add(
        "NH02-改道三分支",
        {
            tty_ok()
        },
        "static",
    );

    s.add(
        "NH03-输出文件",
        {
            path_ok()
        },
        "static",
    );

    s.add(
        "NH04-参数解析",
        {
            argv_ok()
        },
        "static",
    );

    s.add(
        "NH05-追加语义",
        {
            append_ok()
        },
        "static",
    );

    s.add(
        "NH06-退出码",
        {
            exit_ok()
        },
        "static",
    );

    s.add(
        "NH07-渲染与回退",
        {
            render_ok()
        },
        "static",
    );

    s.add(
        "NH08-账本核对",
        {
            let l = build_ledger();
            let a = l.len() == SAMPLES;
            let b = l.ok_count() == 8;
            let c = l.err_count() == 6;
            let d = l.audit() == Ok(());
            let e = l.sample(0).map(|s| s.value).unwrap_or(0) == 1;
            let f = l.sample(13).map(|s| s.code).unwrap_or(0) == NH_ESTATE;
            let g = {
                let mut m = Ledger::new();
                let s = Sample { slot: 3, op: 0, ok: false, code: 0, value: 0 };
                m.register(s) == Err(NH_ESLOT)
            };
            a && b && c && d && e && f && g
        },
        "static",
    );

    s.add(
        "NH09-冻结重放",
        {
            let mut l = build_ledger();
            let same = l.seal() == build_ledger().seal();
            let root = match l.freeze() {
                Ok(v) => v,
                Err(_) => 0,
            };
            let frozen = l.is_frozen() && root == l.seal();
            let replay = match l.replay() {
                Ok(v) => v == root,
                Err(_) => false,
            };
            let probe = Sample { slot: 0, op: 0, ok: false, code: 0, value: 0 };
            let rejected = l.register(probe) == Err(NH_ESLOT) && l.rejects() == 1;
            let again = l.freeze() == Err(NH_EBAL) && l.rejects() == 2;
            same && frozen && replay && rejected && again
        },
        "static",
    );

    s.add(
        "NH10-状态文本",
        {
            let p = match build_plan(&[b"x"], Target::Terminal, Target::Terminal, true, b"", 4) {
                Ok(v) => v,
                Err(_) => Plan::blank(),
            };
            let text = p.status_text();
            let a = p.status_lines() == 3;
            let b = text.as_str().contains("stdout appending nohup.out");
            let c = text.as_str().contains("stderr appending nohup.out");
            let d = text.as_str().ends_with('\n');
            let e = p.path().text().as_str() == "nohup.out";
            a && b && c && d && e
        },
        "static",
    );

    s.add(
        "NH11-非终端不动",
        {
            let p = match build_plan(&[b"x"], Target::Closed, Target::File, false, b"", 9) {
                Ok(v) => v,
                Err(_) => Plan::blank(),
            };
            p.redirect_count() == 0
                && p.path().is_empty()
                && !p.appending()
                && p.status_text().as_str().contains("stdout unchanged")
                && p.status_text().as_str().contains("stderr unchanged")
                && p.table().is_ignored(Signal::Hup)
        },
        "static",
    );

    s.add(
        "NH12-容量边界",
        {
            let a = build_plan(&[b"a", b"b", b"c", b"d"], Target::File, Target::File, true, b"", 0)
                .map(|p| p.len())
                == Ok(4);
            let b = build_plan(&[b"a", b"b", b"c", b"d", b"e"], Target::File, Target::File, true, b"", 0)
                .err()
                == Some(NH_ELEN);
            let long = [b'x'; 49];
            let c = Name::new(&long).err() == Some(NH_ELEN);
            let d = Name::new(b"").err() == Some(NH_EBYTE);
            let e = Name::new(b"a b").err() == Some(NH_EBYTE);
            a && b && c && d && e
        },
        "static",
    );

    s.add(
        "NH13-冻结重放四注入",
        {
            let four = inject_four();
            four[0] && four[1] && four[2] && four[3]
        },
        "static",
    );

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn signal_table() {
        let mut t = SignalTable::new();
        assert_eq!(t.ignored_count(), 0);
        assert_eq!(t.get(Signal::Hup), Disposition::Default);
        t.ignore(Signal::Hup);
        assert!(t.is_ignored(Signal::Hup));
        assert_eq!(t.ignored_count(), 1);
        assert!(!t.is_ignored(Signal::Term));
        assert_eq!(Signal::all().len(), 10);
        assert!(Signal::Hup.fatal_by_default());
        assert!(!Signal::Chld.fatal_by_default());
    }

    #[test]
    fn table_rendering() {
        let mut t = SignalTable::new();
        t.ignore(Signal::Hup);
        let text = t.render();
        assert!(text.as_str().starts_with("HUP ignore\n"));
        assert!(text.as_str().contains("INT default\n"));
        assert_eq!(text.as_str().matches('\n').count(), SIG_COUNT);
    }

    #[test]
    fn name_limits() {
        assert_eq!(Name::new(b"nohup.out").map(|n| n.len()), Ok(9));
        assert_eq!(Name::new(b"").err(), Some(NH_EBYTE));
        assert_eq!(Name::new(b"a\tb").err(), Some(NH_EBYTE));
        let long = [b'x'; 49];
        assert_eq!(Name::new(&long).err(), Some(NH_ELEN));
        assert!(Name::new(DEFAULT_OUT).map(|n| n.named(b"nohup.out")).unwrap_or(false));
    }

    #[test]
    fn output_fallback() {
        assert_eq!(choose_output(true, b"").map(|n| n.text()), Ok(String::from("nohup.out")));
        assert_eq!(
            choose_output(false, b"/h").map(|n| n.text()),
            Ok(String::from("/h/nohup.out"))
        );
        assert_eq!(choose_output(false, b"").err(), Some(NH_ENOENT));
        assert_eq!(choose_output(false, b"a b").err(), Some(NH_EBYTE));
    }

    #[test]
    fn redirect_branches() {
        let both = build_plan(&[b"x"], Target::Terminal, Target::Terminal, true, b"", 0).expect("p");
        assert_eq!(both.stdout(), Redir::AppendFile);
        assert_eq!(both.stderr(), Redir::AppendFile);
        assert_eq!(both.redirect_count(), 2);
        let err_only = build_plan(&[b"x"], Target::File, Target::Terminal, true, b"", 0).expect("p");
        assert_eq!(err_only.stdout(), Redir::Unchanged);
        assert_eq!(err_only.stderr(), Redir::ToStdout);
        let none = build_plan(&[b"x"], Target::Closed, Target::File, true, b"", 0).expect("p");
        assert_eq!(none.redirect_count(), 0);
        assert!(none.path().is_empty());
    }

    #[test]
    fn append_and_status() {
        let fresh = build_plan(&[b"x"], Target::Terminal, Target::Terminal, true, b"", 0).expect("p");
        assert!(!fresh.appending());
        assert!(fresh.status_text().as_str().contains("creating nohup.out"));
        let old = build_plan(&[b"x"], Target::Terminal, Target::Terminal, true, b"", 99).expect("p");
        assert!(old.appending());
        assert!(old.status_text().as_str().contains("appending nohup.out"));
        assert_eq!(old.status_lines(), 3);
    }

    #[test]
    fn argv_gates() {
        assert_eq!(build_plan(&[], Target::File, Target::File, true, b"", 0).err(), Some(NH_ECMD));
        assert_eq!(
            build_plan(&[b"--bad"], Target::File, Target::File, true, b"", 0).err(),
            Some(NH_EOPT)
        );
        assert_eq!(
            build_plan(&[b"-1"], Target::File, Target::File, true, b"", 0).err(),
            Some(NH_EOPT)
        );
        assert_eq!(
            build_plan(&[b"\x02"], Target::File, Target::File, true, b"", 0).err(),
            Some(NH_EBYTE)
        );
        let p = build_plan(&[b"--", b"-1"], Target::File, Target::File, true, b"", 0).expect("p");
        assert!(p.end_seen());
        assert!(p.at(0).map(|n| n.named(b"-1")).unwrap_or(false));
    }

    #[test]
    fn exit_codes() {
        let p = build_plan(&[b"x"], Target::File, Target::File, true, b"", 0).expect("p");
        assert_eq!(p.exit_code(true), 0);
        assert_eq!(p.exit_code(false), 127);
        assert!(p.survives(Signal::Hup));
        assert!(!p.survives(Signal::Int));
    }

    #[test]
    fn hup_is_one_way() {
        let mut p = build_plan(&[b"x"], Target::File, Target::File, true, b"", 0).expect("p");
        assert_eq!(p.restore_hup(), Err(NH_ESTATE));
        assert!(p.table().is_ignored(Signal::Hup));
    }

    #[test]
    fn ledger_shape() {
        let l = build_ledger();
        assert_eq!(l.audit(), Ok(()));
        assert_eq!(l.len(), SAMPLES);
        assert_eq!(l.sample(2).map(|s| s.value), Some(9));
        assert_eq!(l.sample(13).map(|s| s.code), Some(NH_ESTATE));
    }

    #[test]
    fn ledger_replay() {
        let mut l = build_ledger();
        let root = l.freeze().expect("freeze");
        assert_eq!(l.replay(), Ok(root));
        assert_eq!(l.freeze(), Err(NH_EBAL));
        assert_eq!(l.register(Sample::blank()), Err(NH_ESLOT));
    }

    #[test]
    fn injections() {
        let four = inject_four();
        assert!(four[0] && four[1] && four[2] && four[3]);
    }

    #[test]
    fn all_checks_green() {
        let s = run_unx11542_checks();
        assert!(s.all_passed());
    }
}
