//! UNX-F11539 校验工具 stdin 档与 - 代位
//!
//! 校验工具读两类东西：校验行和内容。校验行里可以写 `-`，意思是「这一条
//! 的内容来自标准输入」；命令行参数里也可以写 `-`，意思同样是标准输入。
//! 两处 `-` 的语义必须一致：同样的字节流，走参数 `-` 算出的摘要与走文件
//! 算出的摘要必须逐字节相同，且输出里呈现的名字必须是 `-` 本身——既不能
//! 空着，也不能被替换成「standard input」之类的别名，否则下游按名字查表就
//! 对不上了。
//!
//! 这单做的不是哈希，而是**路由**：
//!
//! 1. **参数分类**。`--` 之后一切皆文件名（`-` 仍保留 stdin 语义）；
//!    `--tag`/`--binary`/`--text`/`--check` 以及五个 check 附属开关各自有
//!    重复与从属闸门：重复给 `--tag` 是错，给了 `--binary` 再给 `--text` 是
//!    冲突，没给 `--check` 却给 `--quiet` 也是冲突。
//! 2. **计划**。参数被折叠成有序目标表，`-` 落成 `Stdin` 目标、其余落成
//!    `Path` 目标；名称逐字节校验（可打印、有上限）。计划是纯数据，不碰
//!    任何 I/O，因此可以在内核里被静态判定。
//! 3. **取值与渲染**。`Registry` 是虚拟内容表：文件按名字取字节，stdin 按
//!    独立槽取字节。目标 + 算法 = 一条校验记录（复用 11538 的 `Entry`），
//!    默认渲染 `<hex>  <name>`、`--binary` 渲染 `<hex> *<name>`、`--tag`
//!    渲染 `ALG (<name>) = <hex>`。三种形态都能被 11538 的解析器读回。
//! 4. **核验**。`--check` 时目标是校验文件本身：逐行解析、按行内名字取内容
//!    （行内 `-` 取 stdin）、比对、统计 good/bad/missing/malformed。
//!
//! 闸门次序：参数字面（`SD_EINVAL`）→ 选项重复（`SD_EDUP`）→ 选项冲突
//! （`SD_ECONF`）→ 目标容量（`SD_ESLOT`）→ 名称字节（`SD_EBYTE`）→ 名称过长
//! （`SD_ELEN`）→ 目标为空（`SD_ESHORT`）→ 内容缺失（`SD_ENOENT`）→ 状态
//! （`SD_ESTATE`）。
//!
//! 账本语义：十四格 = 八格成功（样例目标数、stdin 目标数、stdin sha1 首字节、
//! stdin b2b512 首字节、渲染行数、stdin 与文件同摘要、标签行数、二进制星号数）
//! + 六格失败（未知选项、重复标签、二进制文本冲突、缺内容、空计划、容量溢出）。
//! `audit` 认「八值六码」，`seal` 做 FNV-1a 折叠，`replay` 重建比对，
//! `freeze` 后拒绝登记。

use crate::alloc::string::String;
use crate::checks::CheckSet;

use super::unx11538_hash_family::{Algorithm, Entry};

/// 段基址。
pub const SEG_BASE: u32 = 0xf2c0;

/// 参数字面非法。
pub const SD_EINVAL: u32 = SEG_BASE + 0x01;
/// 选项重复。
pub const SD_EDUP: u32 = SEG_BASE + 0x02;
/// 选项冲突。
pub const SD_ECONF: u32 = SEG_BASE + 0x03;
/// 内容缺失。
pub const SD_ENOENT: u32 = SEG_BASE + 0x04;
/// 目标为空。
pub const SD_ESHORT: u32 = SEG_BASE + 0x05;
/// 名称过长。
pub const SD_ELEN: u32 = SEG_BASE + 0x06;
/// 名称字节非法。
pub const SD_EBYTE: u32 = SEG_BASE + 0x07;
/// 状态冲突。
pub const SD_ESTATE: u32 = SEG_BASE + 0x08;
/// 账本失衡。
pub const SD_EBAL: u32 = SEG_BASE + 0x09;
/// 槽位错位或容量溢出。
pub const SD_ESLOT: u32 = SEG_BASE + 0x0a;
/// 值不符。
pub const SD_EVALUE: u32 = SEG_BASE + 0x0b;
/// 指纹不符。
pub const SD_ESEAL: u32 = SEG_BASE + 0x0c;
/// 观测缺失。
pub const SD_ENOBS: u32 = SEG_BASE + 0x0d;

/// 账本格数。
pub const SAMPLES: usize = 14;

/// 目标容量。
pub const TARGET_CAP: usize = 32;

/// 名称容量。
pub const NAME_CAP: usize = 64;

/// 内容容量。
pub const DATA_CAP: usize = 256;

/// 注册表条目数。
pub const REG_CAP: usize = 8;

/// 操作码：参数分类。
pub const OP_ARG: u8 = 0x01;
/// 操作码：目标选择。
pub const OP_SELECT: u8 = 0x02;
/// 操作码：取摘要。
pub const OP_DIGEST: u8 = 0x03;
/// 操作码：渲染。
pub const OP_RENDER: u8 = 0x04;
/// 操作码：比对。
pub const OP_COMPARE: u8 = 0x05;
/// 操作码：标签。
pub const OP_TAG: u8 = 0x06;
/// 操作码：二进制。
pub const OP_BINARY: u8 = 0x07;
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
    let mut k = i;
    while k < 20 {
        if let Some(b) = buf.get(k) {
            s.push(*b as char);
        }
        k += 1;
    }
}

/// 取字节。
fn byte_at(b: &[u8], i: usize) -> Option<u8> {
    match b.get(i) {
        Some(v) => Some(*v),
        None => None,
    }
}

/// 自起找字节。
fn find_from(b: &[u8], start: usize, want: u8) -> Option<usize> {
    let mut i = start;
    while i < b.len() {
        if byte_at(b, i) == Some(want) {
            return Some(i);
        }
        i += 1;
    }
    None
}

/// 字节切片相等。
fn slice_eq(a: &[u8], b: &[u8]) -> bool {
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

/// 可打印校验。
fn printable(b: &[u8]) -> bool {
    let mut i = 0usize;
    while i < b.len() {
        let x = match b.get(i) {
            Some(v) => *v,
            None => return false,
        };
        if x < 0x20 || x > 0x7e {
            return false;
        }
        i += 1;
    }
    true
}

/// 字节 → 串。
fn str_of(b: &[u8]) -> String {
    let mut s = String::new();
    let mut i = 0usize;
    while i < b.len() {
        if let Some(c) = b.get(i) {
            s.push(*c as char);
        }
        i += 1;
    }
    s
}

/// 字符出现次数。
fn count_char(s: &str, c: char) -> usize {
    let mut n = 0usize;
    let mut it = s.chars();
    loop {
        match it.next() {
            Some(x) => {
                if x == c {
                    n += 1;
                }
            }
            None => break,
        }
    }
    n
}

// ---------------------------------------------------------------------------
// 参数分类
// ---------------------------------------------------------------------------

/// 参数类别。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Opt {
    /// 普通文件名。
    File,
    /// 标准输入（`-`）。
    Stdin,
    /// 选项结束（`--`）。
    End,
    /// 标签格式。
    Tag,
    /// 二进制内容标记。
    Binary,
    /// 文本内容标记。
    Text,
    /// 核验模式。
    Check,
    /// 只输出失败。
    Quiet,
    /// 只出退出码。
    Status,
    /// 格式错误即失败。
    Strict,
    /// 格式警告。
    Warn,
    /// 忽略缺失文件。
    IgnoreMissing,
}

impl Opt {
    /// 文本名。
    pub fn text(&self) -> &'static str {
        match self {
            Opt::File => "file",
            Opt::Stdin => "stdin",
            Opt::End => "end",
            Opt::Tag => "tag",
            Opt::Binary => "binary",
            Opt::Text => "text",
            Opt::Check => "check",
            Opt::Quiet => "quiet",
            Opt::Status => "status",
            Opt::Strict => "strict",
            Opt::Warn => "warn",
            Opt::IgnoreMissing => "ignore-missing",
        }
    }

    /// 是否选项（非操作数）。
    pub fn is_option(&self) -> bool {
        match self {
            Opt::File | Opt::Stdin => false,
            _ => true,
        }
    }
}

/// 分类一个参数。
pub fn classify(a: &[u8]) -> Result<Opt, u32> {
    if a.is_empty() {
        return Err(SD_ESHORT);
    }
    if slice_eq(a, b"-") {
        return Ok(Opt::Stdin);
    }
    if slice_eq(a, b"--") {
        return Ok(Opt::End);
    }
    if slice_eq(a, b"--tag") {
        return Ok(Opt::Tag);
    }
    if slice_eq(a, b"--binary") || slice_eq(a, b"-b") {
        return Ok(Opt::Binary);
    }
    if slice_eq(a, b"--text") || slice_eq(a, b"-t") {
        return Ok(Opt::Text);
    }
    if slice_eq(a, b"--check") || slice_eq(a, b"-c") {
        return Ok(Opt::Check);
    }
    if slice_eq(a, b"--quiet") || slice_eq(a, b"-q") {
        return Ok(Opt::Quiet);
    }
    if slice_eq(a, b"--status") {
        return Ok(Opt::Status);
    }
    if slice_eq(a, b"--strict") {
        return Ok(Opt::Strict);
    }
    if slice_eq(a, b"--warn") {
        return Ok(Opt::Warn);
    }
    if slice_eq(a, b"--ignore-missing") {
        return Ok(Opt::IgnoreMissing);
    }
    if byte_at(a, 0) == Some(b'-') {
        return Err(SD_EINVAL);
    }
    Ok(Opt::File)
}

// ---------------------------------------------------------------------------
// 目标
// ---------------------------------------------------------------------------

/// 目标种类。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    /// 普通文件。
    Path,
    /// 标准输入。
    Stdin,
}

impl Kind {
    /// 文本名。
    pub fn text(&self) -> &'static str {
        match self {
            Kind::Path => "path",
            Kind::Stdin => "stdin",
        }
    }

    /// 是否 stdin。
    pub fn is_stdin(&self) -> bool {
        match self {
            Kind::Stdin => true,
            Kind::Path => false,
        }
    }
}

/// 一个目标。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Target {
    kind: Kind,
    name: [u8; NAME_CAP],
    len: usize,
}

impl Target {
    /// 空白目标。
    pub const fn blank() -> Target {
        Target { kind: Kind::Path, name: [0u8; NAME_CAP], len: 0 }
    }

    /// 文件目标。
    pub fn path(name: &[u8]) -> Result<Target, u32> {
        if name.is_empty() {
            return Err(SD_ESHORT);
        }
        if name.len() > NAME_CAP {
            return Err(SD_ELEN);
        }
        if !printable(name) {
            return Err(SD_EBYTE);
        }
        let mut buf = [0u8; NAME_CAP];
        let mut i = 0usize;
        while i < name.len() {
            if let (Some(dst), Some(src)) = (buf.get_mut(i), name.get(i)) {
                *dst = *src;
            }
            i += 1;
        }
        Ok(Target { kind: Kind::Path, name: buf, len: name.len() })
    }

    /// stdin 目标；呈现名固定为 `-`。
    pub fn stdin() -> Target {
        let mut buf = [0u8; NAME_CAP];
        if let Some(d) = buf.get_mut(0) {
            *d = b'-';
        }
        Target { kind: Kind::Stdin, name: buf, len: 1 }
    }

    /// 种类。
    pub fn kind(&self) -> Kind {
        self.kind
    }

    /// 呈现名。
    pub fn name(&self) -> &[u8] {
        self.name.get(0..self.len).unwrap_or(&[])
    }

    /// 呈现名串。
    pub fn name_text(&self) -> String {
        str_of(self.name())
    }

    /// 是否 stdin。
    pub fn is_stdin(&self) -> bool {
        self.kind.is_stdin()
    }

    /// 名字与给定字节一致。
    pub fn named(&self, other: &[u8]) -> bool {
        slice_eq(self.name(), other)
    }
}

// ---------------------------------------------------------------------------
// 计划
// ---------------------------------------------------------------------------

/// 参数解析结果。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Plan {
    targets: [Target; TARGET_CAP],
    len: usize,
    stdin_used: bool,
    end_seen: bool,
    tag: bool,
    binary: bool,
    text: bool,
    check: bool,
    quiet: bool,
    status: bool,
    strict: bool,
    warn: bool,
    ignore_missing: bool,
}

impl Plan {
    /// 空计划。
    pub const fn new() -> Plan {
        Plan {
            targets: [Target::blank(); TARGET_CAP],
            len: 0,
            stdin_used: false,
            end_seen: false,
            tag: false,
            binary: false,
            text: false,
            check: false,
            quiet: false,
            status: false,
            strict: false,
            warn: false,
            ignore_missing: false,
        }
    }

    /// 推入一个参数。
    pub fn push_arg(&mut self, a: &[u8]) -> Result<(), u32> {
        if self.end_seen {
            if slice_eq(a, b"-") {
                return self.push_target(Target::stdin());
            }
            return self.push_target(Target::path(a)?);
        }
        let opt = classify(a)?;
        match opt {
            Opt::File => self.push_target(Target::path(a)?),
            Opt::Stdin => {
                self.stdin_used = true;
                self.push_target(Target::stdin())
            }
            Opt::End => {
                self.end_seen = true;
                Ok(())
            }
            Opt::Tag => set_once(&mut self.tag),
            Opt::Binary => set_once(&mut self.binary),
            Opt::Text => set_once(&mut self.text),
            Opt::Check => set_once(&mut self.check),
            Opt::Quiet => set_once(&mut self.quiet),
            Opt::Status => set_once(&mut self.status),
            Opt::Strict => set_once(&mut self.strict),
            Opt::Warn => set_once(&mut self.warn),
            Opt::IgnoreMissing => set_once(&mut self.ignore_missing),
        }
    }

    /// 推入目标。
    fn push_target(&mut self, t: Target) -> Result<(), u32> {
        if self.len >= TARGET_CAP {
            return Err(SD_ESLOT);
        }
        if let Some(d) = self.targets.get_mut(self.len) {
            *d = t;
        }
        self.len += 1;
        Ok(())
    }

    /// 收口校验。
    pub fn finish(&mut self) -> Result<(), u32> {
        if self.binary && self.text {
            return Err(SD_ECONF);
        }
        if self.quiet && !self.check {
            return Err(SD_ECONF);
        }
        if self.status && !self.check {
            return Err(SD_ECONF);
        }
        if self.strict && !self.check {
            return Err(SD_ECONF);
        }
        if self.warn && !self.check {
            return Err(SD_ECONF);
        }
        if self.ignore_missing && !self.check {
            return Err(SD_ECONF);
        }
        if self.len == 0 {
            return Err(SD_ESHORT);
        }
        Ok(())
    }

    /// 目标数。
    pub fn len(&self) -> usize {
        self.len
    }

    /// 是否空。
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// 取目标。
    pub fn at(&self, i: usize) -> Option<Target> {
        if i >= self.len {
            return None;
        }
        self.targets.get(i).copied()
    }

    /// 首目标。
    pub fn first(&self) -> Option<Target> {
        self.at(0)
    }

    /// stdin 目标数。
    pub fn stdin_targets(&self) -> usize {
        let mut n = 0usize;
        let mut i = 0usize;
        while i < self.len {
            if let Some(t) = self.targets.get(i) {
                if t.is_stdin() {
                    n += 1;
                }
            }
            i += 1;
        }
        n
    }

    /// 是否用过 stdin。
    pub fn stdin_used(&self) -> bool {
        self.stdin_used
    }

    /// 标签格式。
    pub fn tag(&self) -> bool {
        self.tag
    }

    /// 二进制标记。
    pub fn binary(&self) -> bool {
        self.binary
    }

    /// 文本标记。
    pub fn text(&self) -> bool {
        self.text
    }

    /// 核验模式。
    pub fn check(&self) -> bool {
        self.check
    }

    /// 静默。
    pub fn quiet(&self) -> bool {
        self.quiet
    }

    /// 只出退出码。
    pub fn status(&self) -> bool {
        self.status
    }

    /// 严格。
    pub fn strict(&self) -> bool {
        self.strict
    }

    /// 警告。
    pub fn warn(&self) -> bool {
        self.warn
    }

    /// 忽略缺失。
    pub fn ignore_missing(&self) -> bool {
        self.ignore_missing
    }

    /// 是否见过 `--`。
    pub fn end_seen(&self) -> bool {
        self.end_seen
    }
}

/// 置位一次；重复报 EDUP。
fn set_once(f: &mut bool) -> Result<(), u32> {
    if *f {
        return Err(SD_EDUP);
    }
    *f = true;
    Ok(())
}

/// 由参数表构造计划。
pub fn prepare(args: &[&[u8]]) -> Result<Plan, u32> {
    let mut p = Plan::new();
    let mut i = 0usize;
    while i < args.len() {
        if let Some(a) = args.get(i) {
            p.push_arg(a)?;
        }
        i += 1;
    }
    p.finish()?;
    Ok(p)
}

// ---------------------------------------------------------------------------
// 虚拟内容表
// ---------------------------------------------------------------------------

/// 一条内容。
#[derive(Clone, Copy, Debug)]
struct Slot {
    name: [u8; NAME_CAP],
    name_len: usize,
    data: [u8; DATA_CAP],
    data_len: usize,
    used: bool,
}

impl Slot {
    const fn blank() -> Slot {
        Slot { name: [0u8; NAME_CAP], name_len: 0, data: [0u8; DATA_CAP], data_len: 0, used: false }
    }

    fn name(&self) -> &[u8] {
        self.name.get(0..self.name_len).unwrap_or(&[])
    }

    fn data(&self) -> &[u8] {
        self.data.get(0..self.data_len).unwrap_or(&[])
    }
}

/// 虚拟内容表。
#[derive(Clone, Copy, Debug)]
pub struct Registry {
    rows: [Slot; REG_CAP],
    stdin: [u8; DATA_CAP],
    stdin_len: usize,
}

impl Registry {
    /// 空表。
    pub const fn new() -> Registry {
        Registry { rows: [Slot::blank(); REG_CAP], stdin: [0u8; DATA_CAP], stdin_len: 0 }
    }

    /// 放入文件内容。
    pub fn insert(&mut self, name: &[u8], data: &[u8]) -> Result<(), u32> {
        if name.is_empty() {
            return Err(SD_ESHORT);
        }
        if name.len() > NAME_CAP {
            return Err(SD_ELEN);
        }
        if !printable(name) {
            return Err(SD_EBYTE);
        }
        if data.len() > DATA_CAP {
            return Err(SD_ELEN);
        }
        let mut i = 0usize;
        while i < REG_CAP {
            if let Some(s) = self.rows.get(i) {
                if s.used && slice_eq(s.name(), name) {
                    return Err(SD_EDUP);
                }
            }
            i += 1;
        }
        let mut idx = REG_CAP;
        i = 0;
        while i < REG_CAP {
            if let Some(s) = self.rows.get(i) {
                if !s.used {
                    idx = i;
                    break;
                }
            }
            i += 1;
        }
        if idx >= REG_CAP {
            return Err(SD_ESLOT);
        }
        let mut nbuf = [0u8; NAME_CAP];
        let mut k = 0usize;
        while k < name.len() {
            if let (Some(dst), Some(src)) = (nbuf.get_mut(k), name.get(k)) {
                *dst = *src;
            }
            k += 1;
        }
        let mut dbuf = [0u8; DATA_CAP];
        k = 0;
        while k < data.len() {
            if let (Some(dst), Some(src)) = (dbuf.get_mut(k), data.get(k)) {
                *dst = *src;
            }
            k += 1;
        }
        if let Some(s) = self.rows.get_mut(idx) {
            *s = Slot {
                name: nbuf,
                name_len: name.len(),
                data: dbuf,
                data_len: data.len(),
                used: true,
            };
        }
        Ok(())
    }

    /// 设置 stdin 内容。
    pub fn set_stdin(&mut self, data: &[u8]) -> Result<(), u32> {
        if data.len() > DATA_CAP {
            return Err(SD_ELEN);
        }
        let mut k = 0usize;
        while k < data.len() {
            if let (Some(dst), Some(src)) = (self.stdin.get_mut(k), data.get(k)) {
                *dst = *src;
            }
            k += 1;
        }
        self.stdin_len = data.len();
        Ok(())
    }

    /// stdin 内容。
    pub fn stdin_data(&self) -> &[u8] {
        self.stdin.get(0..self.stdin_len).unwrap_or(&[])
    }

    /// 按名查找。
    pub fn find(&self, name: &[u8]) -> Option<&[u8]> {
        let mut i = 0usize;
        while i < REG_CAP {
            if let Some(s) = self.rows.get(i) {
                if s.used && slice_eq(s.name(), name) {
                    return Some(s.data());
                }
            }
            i += 1;
        }
        None
    }

    /// 取目标内容。
    pub fn content_for(&self, t: &Target) -> Result<&[u8], u32> {
        if t.is_stdin() {
            return Ok(self.stdin_data());
        }
        match self.find(t.name()) {
            Some(d) => Ok(d),
            None => Err(SD_ENOENT),
        }
    }

    /// 已登记文件数。
    pub fn len(&self) -> usize {
        let mut n = 0usize;
        let mut i = 0usize;
        while i < REG_CAP {
            if let Some(s) = self.rows.get(i) {
                if s.used {
                    n += 1;
                }
            }
            i += 1;
        }
        n
    }

    /// 是否空。
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

// ---------------------------------------------------------------------------
// 取值与渲染
// ---------------------------------------------------------------------------

/// 由目标取一条校验记录。
pub fn entry_for(t: &Target, alg: Algorithm, reg: &Registry) -> Result<Entry, u32> {
    let content = reg.content_for(t)?;
    Entry::from_content(alg, content, t.name(), false)
}

/// 二进制形态的一行。
pub fn binary_line(e: &Entry) -> String {
    let mut s = String::new();
    s.push_str(e.digest().hex().as_str());
    s.push(' ');
    s.push('*');
    s.push_str(e.name_text().as_str());
    s
}

/// 渲染整个计划。
pub fn render_plan(plan: &Plan, alg: Algorithm, reg: &Registry) -> Result<String, u32> {
    if plan.check() {
        return Err(SD_ESTATE);
    }
    let mut s = String::new();
    let mut i = 0usize;
    while i < plan.len() {
        let t = match plan.at(i) {
            Some(v) => v,
            None => return Err(SD_ENOBS),
        };
        let e = entry_for(&t, alg, reg)?;
        if plan.tag() {
            s.push_str(e.tag_line().as_str());
        } else if plan.binary() {
            s.push_str(binary_line(&e).as_str());
        } else {
            s.push_str(e.render().as_str());
        }
        s.push('\n');
        i += 1;
    }
    Ok(s)
}

/// 两个目标是否同摘要。
pub fn same_digest(a: &Target, b: &Target, alg: Algorithm, reg: &Registry) -> Result<bool, u32> {
    let ea = entry_for(a, alg, reg)?;
    let eb = entry_for(b, alg, reg)?;
    Ok(ea.digest().equals(eb.digest()))
}

// ---------------------------------------------------------------------------
// 核验
// ---------------------------------------------------------------------------

/// 核验统计。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct Report {
    /// 已检查。
    pub checked: usize,
    /// 一致。
    pub good: usize,
    /// 不一致。
    pub bad: usize,
    /// 缺失。
    pub missing: usize,
    /// 格式错误。
    pub malformed: usize,
}

impl Report {
    /// 空白。
    pub const fn new() -> Report {
        Report { checked: 0, good: 0, bad: 0, missing: 0, malformed: 0 }
    }

    /// 是否全好。
    pub fn all_good(&self) -> bool {
        self.bad == 0 && self.missing == 0 && self.malformed == 0
    }

    /// 文本。
    pub fn text(&self) -> String {
        let mut s = String::new();
        s.push_str("checked ");
        push_dec(&mut s, self.checked as u64);
        s.push_str(" good ");
        push_dec(&mut s, self.good as u64);
        s.push_str(" bad ");
        push_dec(&mut s, self.bad as u64);
        s.push_str(" missing ");
        push_dec(&mut s, self.missing as u64);
        s.push_str(" malformed ");
        push_dec(&mut s, self.malformed as u64);
        s
    }
}

/// 取下一行。
fn next_line(c: &[u8], from: usize) -> (Option<&[u8]>, usize) {
    if from >= c.len() {
        return (None, from);
    }
    let end = match find_from(c, from, b'\n') {
        Some(v) => v,
        None => c.len(),
    };
    let line = c.get(from..end).unwrap_or(&[]);
    let next = if end < c.len() { end + 1 } else { c.len() };
    (Some(line), next)
}

/// 空行判定。
fn blank_line(l: &[u8]) -> bool {
    let mut i = 0usize;
    while i < l.len() {
        match l.get(i) {
            Some(b) => {
                if *b != b' ' && *b != b'\r' {
                    return false;
                }
            }
            None => break,
        }
        i += 1;
    }
    true
}

/// 核验计划中的校验文件。
pub fn verify(plan: &Plan, reg: &Registry) -> Result<Report, u32> {
    if !plan.check() {
        return Err(SD_ESTATE);
    }
    let target = match plan.first() {
        Some(t) => t,
        None => return Err(SD_ESHORT),
    };
    let content = reg.content_for(&target)?;
    let mut rep = Report::new();
    let mut at = 0usize;
    loop {
        let (line, next) = next_line(content, at);
        let l = match line {
            Some(v) => v,
            None => break,
        };
        if !blank_line(l) {
            match Entry::parse(l) {
                Ok(e) => {
                    rep.checked += 1;
                    let body = if slice_eq(e.name(), b"-") {
                        Some(reg.stdin_data())
                    } else {
                        reg.find(e.name())
                    };
                    match body {
                        Some(b) => match e.matches(b) {
                            Ok(true) => rep.good += 1,
                            Ok(false) => rep.bad += 1,
                            Err(_) => rep.malformed += 1,
                        },
                        None => {
                            if !plan.ignore_missing() {
                                rep.missing += 1;
                            }
                        }
                    }
                }
                Err(_) => {
                    rep.malformed += 1;
                    if plan.strict() {
                        return Err(SD_EINVAL);
                    }
                }
            }
        }
        at = next;
        if at >= content.len() {
            break;
        }
    }
    Ok(rep)
}

// ---------------------------------------------------------------------------
// 样例与账本
// ---------------------------------------------------------------------------

/// 样例参数。
pub const SAMPLE_ARGS: [&[u8]; 2] = [b"data.txt", b"-"];

/// 样例内容。
pub const SAMPLE_DATA: &[u8] = b"abc";

/// 样例计划。
pub fn sample_plan() -> Result<Plan, u32> {
    prepare(&SAMPLE_ARGS)
}

/// 样例注册表。
pub fn sample_registry() -> Registry {
    let mut r = Registry::new();
    let _ = r.insert(b"data.txt", SAMPLE_DATA);
    let _ = r.set_stdin(SAMPLE_DATA);
    r
}

/// 样例校验文件内容（stdin 行 + 文件行 + 缺失行）。
pub const SAMPLE_CHECK_FILE: &[u8] = b"a9993e364706816aba3e25717850c26c9cd0d89d  -\na9993e364706816aba3e25717850c26c9cd0d89d  data.txt\n";

/// 首字节观测。
fn first_byte(alg: Algorithm, data: &[u8]) -> Result<u32, u32> {
    match alg.digest(data) {
        Ok(d) => match d.byte(0) {
            Some(b) => Ok(b as u32),
            None => Err(SD_ENOBS),
        },
        Err(c) => Err(c),
    }
}

/// 统计 stdin 目标。
fn sample_stdin_targets() -> Result<usize, u32> {
    let p = sample_plan()?;
    Ok(p.stdin_targets())
}

/// 样例渲染行数。
fn sample_render_lines() -> Result<usize, u32> {
    let p = sample_plan()?;
    let r = sample_registry();
    let s = render_plan(&p, Algorithm::Sha1, &r)?;
    Ok(count_char(s.as_str(), '\n'))
}

/// 样例同摘要。
fn sample_same() -> Result<usize, u32> {
    let r = sample_registry();
    let file = Target::path(b"data.txt")?;
    let stdin = Target::stdin();
    let a = same_digest(&file, &stdin, Algorithm::Sha1, &r)?;
    let b = same_digest(&file, &stdin, Algorithm::Blake2b512, &r)?;
    Ok(if a && b { 1 } else { 0 })
}

/// 标签行数。
fn sample_tag_lines() -> Result<usize, u32> {
    let p = prepare(&[b"--tag", b"-"])?;
    let r = sample_registry();
    let s = render_plan(&p, Algorithm::Sha1, &r)?;
    Ok(count_char(s.as_str(), '\n'))
}

/// 二进制星号数。
fn sample_binary_stars() -> Result<usize, u32> {
    let p = prepare(&[b"--binary", b"data.txt"])?;
    let r = sample_registry();
    let s = render_plan(&p, Algorithm::Sha1, &r)?;
    Ok(count_char(s.as_str(), '*'))
}

/// 缺内容探针。
fn missing_probe() -> Result<u32, u32> {
    let p = prepare(&[b"nope.txt"])?;
    let t = match p.first() {
        Some(v) => v,
        None => return Err(SD_ENOBS),
    };
    let r = Registry::new();
    entry_for(&t, Algorithm::Sha1, &r).map(|_| 0)
}

/// 重复标签探针。
fn dup_probe() -> Result<u32, u32> {
    let mut p = Plan::new();
    p.push_arg(b"--tag")?;
    p.push_arg(b"--tag").map(|_| 0)
}

/// 冲突探针。
fn conf_probe() -> Result<u32, u32> {
    let mut p = Plan::new();
    p.push_arg(b"--binary")?;
    p.push_arg(b"--text")?;
    p.finish().map(|_| 0)
}

/// 容量探针。
fn overflow_probe() -> Result<u32, u32> {
    let mut p = Plan::new();
    let mut i = 0usize;
    while i < TARGET_CAP {
        let mut buf = [b'f', b'0', b'0'];
        if let Some(d) = buf.get_mut(1) {
            *d = b'0' + (i / 10) as u8;
        }
        if let Some(d) = buf.get_mut(2) {
            *d = b'0' + (i % 10) as u8;
        }
        p.push_arg(&buf)?;
        i += 1;
    }
    p.push_arg(b"over").map(|_| 0)
}

/// 构建账本。
pub fn build_ledger() -> Ledger {
    let mut l = Ledger::new();

    let _ = l.register(count_cell(0, OP_SELECT, sample_plan().map(|p| p.len())));

    let _ = l.register(count_cell(1, OP_SELECT, sample_stdin_targets()));

    let _ = l.register(word_cell(2, OP_DIGEST, first_byte(Algorithm::Sha1, SAMPLE_DATA)));

    let _ = l.register(word_cell(3, OP_DIGEST, first_byte(Algorithm::Blake2b512, SAMPLE_DATA)));

    let _ = l.register(count_cell(4, OP_RENDER, sample_render_lines()));

    let _ = l.register(count_cell(5, OP_COMPARE, sample_same()));

    let _ = l.register(count_cell(6, OP_TAG, sample_tag_lines()));

    let _ = l.register(count_cell(7, OP_BINARY, sample_binary_stars()));

    let _ = l.register(word_cell(8, OP_ARG, classify(b"--wat").map(|o| o as u32)));

    let _ = l.register(word_cell(9, OP_ARG, dup_probe()));

    let _ = l.register(word_cell(10, OP_ARG, conf_probe()));

    let _ = l.register(word_cell(11, OP_SELECT, missing_probe()));

    let _ = l.register(word_cell(12, OP_SELECT, prepare(&[]).map(|_| 0)));

    let _ = l.register(word_cell(13, OP_SLOT, overflow_probe()));

    l
}

// ---------------------------------------------------------------------------
// 注入
// ---------------------------------------------------------------------------

/// 分类与计划的字面语义。
fn arg_semantics_ok() -> bool {
    let a = classify(b"-") == Ok(Opt::Stdin);
    let b = classify(b"--") == Ok(Opt::End);
    let c = classify(b"--tag") == Ok(Opt::Tag);
    let d = classify(b"--binary") == Ok(Opt::Binary);
    let e = classify(b"-b") == Ok(Opt::Binary);
    let f = classify(b"--text") == Ok(Opt::Text);
    let g = classify(b"-t") == Ok(Opt::Text);
    let h = classify(b"--check") == Ok(Opt::Check);
    let i = classify(b"-c") == Ok(Opt::Check);
    let j = classify(b"--quiet") == Ok(Opt::Quiet);
    let k = classify(b"--status") == Ok(Opt::Status);
    let l = classify(b"--strict") == Ok(Opt::Strict);
    let m = classify(b"--warn") == Ok(Opt::Warn);
    let n = classify(b"--ignore-missing") == Ok(Opt::IgnoreMissing);
    let o = classify(b"file.txt") == Ok(Opt::File);
    let p = classify(b"--wat") == Err(SD_EINVAL);
    let q = classify(b"-x") == Err(SD_EINVAL);
    let r = classify(b"") == Err(SD_ESHORT);
    a && b && c && d && e && f && g && h && i && j && k && l && m && n && o && p && q && r
}

/// 选项闸门。
fn option_gates_ok() -> bool {
    let a = prepare(&[b"--tag", b"--tag", b"-"]).err() == Some(SD_EDUP);
    let b = prepare(&[b"--binary", b"--text", b"-"]).err() == Some(SD_ECONF);
    let c = prepare(&[b"--quiet", b"-"]).err() == Some(SD_ECONF);
    let d = prepare(&[b"--status", b"-"]).err() == Some(SD_ECONF);
    let e = prepare(&[b"--strict", b"-"]).err() == Some(SD_ECONF);
    let f = prepare(&[b"--warn", b"-"]).err() == Some(SD_ECONF);
    let g = prepare(&[b"--ignore-missing", b"-"]).err() == Some(SD_ECONF);
    let h = prepare(&[b"--check", b"--quiet", b"-"]).is_ok();
    let i = prepare(&[]).err() == Some(SD_ESHORT);
    let j = prepare(&[b"--"]).err() == Some(SD_ESHORT);
    a && b && c && d && e && f && g && h && i && j
}

/// 顺序与名称。
fn order_ok() -> bool {
    let p = match prepare(&[b"alpha", b"-", b"beta"]) {
        Ok(v) => v,
        Err(_) => return false,
    };
    let t0 = p.at(0);
    let t1 = p.at(1);
    let t2 = p.at(2);
    let a = match t0 {
        Some(t) => !t.is_stdin() && t.named(b"alpha"),
        None => false,
    };
    let b = match t1 {
        Some(t) => t.is_stdin() && t.named(b"-"),
        None => false,
    };
    let c = match t2 {
        Some(t) => !t.is_stdin() && t.named(b"beta"),
        None => false,
    };
    let d = p.stdin_targets() == 1;
    a && b && c && d
}

/// 结束符。
fn end_marker_ok() -> bool {
    let p = match prepare(&[b"--", b"--tag", b"-"]) {
        Ok(v) => v,
        Err(_) => return false,
    };
    let a = match p.at(0) {
        Some(t) => t.named(b"--tag") && !t.is_stdin(),
        None => false,
    };
    let b = match p.at(1) {
        Some(t) => t.is_stdin(),
        None => false,
    };
    a && b && !p.tag() && p.end_seen()
}

/// 渲染三形态。
fn render_ok() -> bool {
    let r = sample_registry();
    let plain = match prepare(&[b"-"]) {
        Ok(p) => match render_plan(&p, Algorithm::Sha1, &r) {
            Ok(s) => s,
            Err(_) => return false,
        },
        Err(_) => return false,
    };
    let tagged = match prepare(&[b"--tag", b"-"]) {
        Ok(p) => match render_plan(&p, Algorithm::Sha1, &r) {
            Ok(s) => s,
            Err(_) => return false,
        },
        Err(_) => return false,
    };
    let binary = match prepare(&[b"--binary", b"data.txt"]) {
        Ok(p) => match render_plan(&p, Algorithm::Sha1, &r) {
            Ok(s) => s,
            Err(_) => return false,
        },
        Err(_) => return false,
    };
    let a = plain.as_str().ends_with("  -\n");
    let b = tagged.as_str().starts_with("SHA1 (-) = ");
    let c = binary.as_str().contains(" *data.txt\n");
    let d = match Entry::parse(plain.as_str().as_bytes()) {
        Ok(e) => e.name_text().as_str() == "-",
        Err(_) => false,
    };
    let e = match Entry::parse(binary.as_str().as_bytes()) {
        Ok(v) => v.name_text().as_str() == "data.txt",
        Err(_) => false,
    };
    a && b && c && d && e
}

/// 核验样例。
fn verify_ok() -> bool {
    let mut r = sample_registry();
    if r.insert(b"check.txt", SAMPLE_CHECK_FILE).is_err() {
        return false;
    }
    let p = match prepare(&[b"--check", b"check.txt"]) {
        Ok(v) => v,
        Err(_) => return false,
    };
    let rep = match verify(&p, &r) {
        Ok(v) => v,
        Err(_) => return false,
    };
    rep.checked == 2 && rep.good == 2 && rep.bad == 0 && rep.missing == 0
}

/// 账本自洽。
fn ledger_ok() -> bool {
    let l = build_ledger();
    if l.audit().is_err() {
        return false;
    }
    let s1 = l.seal();
    if build_ledger().seal() != s1 {
        return false;
    }
    let mut l3 = build_ledger();
    let r = match l3.freeze() {
        Ok(v) => v,
        Err(_) => return false,
    };
    if r != s1 || !l3.is_frozen() {
        return false;
    }
    let replay = match l3.replay() {
        Ok(v) => v == r,
        Err(_) => false,
    };
    if !replay {
        return false;
    }
    let probe = Sample { slot: 0, op: 0, ok: false, code: 0, value: 0 };
    if l3.register(probe) != Err(SD_ESLOT) {
        return false;
    }
    l3.rejects() == 1
}

/// 四路注入。
pub fn inject_four() -> [bool; 4] {
    let mut out = [false; 4];
    out[0] = arg_semantics_ok() && order_ok() && end_marker_ok();
    out[1] = option_gates_ok() && render_ok();
    out[2] = ledger_ok();
    out[3] = verify_ok();
    out
}

// ---------------------------------------------------------------------------
// 账本结构
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
            return Err(SD_ESLOT);
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
            return Err(SD_EBAL);
        }
        const VALS: [u32; 8] = [2, 1, 169, 186, 2, 1, 1, 1];
        const CODES: [u32; 6] = [SD_EINVAL, SD_EDUP, SD_ECONF, SD_ENOENT, SD_ESHORT, SD_ESLOT];
        let mut vi = 0usize;
        let mut ci = 0usize;
        let mut i = 0usize;
        while i < self.len {
            let s = match self.rows.get(i) {
                Some(v) => *v,
                None => return Err(SD_EBAL),
            };
            if s.slot as usize != i {
                return Err(SD_ESLOT);
            }
            if s.ok {
                if s.code != 0 {
                    return Err(SD_EVALUE);
                }
                match VALS.get(vi) {
                    Some(w) => {
                        if s.value != *w {
                            return Err(SD_EVALUE);
                        }
                    }
                    None => return Err(SD_EBAL),
                }
                vi += 1;
            } else {
                match CODES.get(ci) {
                    Some(w) => {
                        if s.code != *w {
                            return Err(SD_EVALUE);
                        }
                    }
                    None => return Err(SD_EBAL),
                }
                ci += 1;
            }
            i += 1;
        }
        if vi != 8 || ci != 6 {
            return Err(SD_EBAL);
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
            return Err(SD_EBAL);
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
            Err(SD_ESEAL)
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
// 判据
// ---------------------------------------------------------------------------

/// 判据集合。
pub fn run_unx11539_checks() -> CheckSet {
    let mut s = CheckSet::new("unx11539.stdin");

    s.add(
        "SD01-参数分类",
        {
            arg_semantics_ok()
        },
        "static",
    );

    s.add(
        "SD02-选项闸门",
        {
            option_gates_ok()
        },
        "static",
    );

    s.add(
        "SD03-目标顺序",
        {
            order_ok()
        },
        "static",
    );

    s.add(
        "SD04-名称呈现",
        {
            let t = Target::stdin();
            let a = t.named(b"-") && t.is_stdin() && t.name_text().as_str() == "-";
            let p = match Target::path(b"data.txt") {
                Ok(v) => v,
                Err(_) => Target::blank(),
            };
            let b = p.named(b"data.txt") && !p.is_stdin();
            let c = Target::path(b"").err() == Some(SD_ESHORT);
            let d = Target::path(&[0u8; 1]).err() == Some(SD_EBYTE);
            a && b && c && d
        },
        "static",
    );

    s.add(
        "SD05-摘要一致",
        {
            let r = sample_registry();
            let f = Target::path(b"data.txt").unwrap_or(Target::blank());
            let st = Target::stdin();
            let a = same_digest(&f, &st, Algorithm::Sha1, &r) == Ok(true);
            let b = same_digest(&f, &st, Algorithm::Blake2b512, &r) == Ok(true);
            let mut r2 = Registry::new();
            let _ = r2.insert(b"data.txt", b"abc");
            let _ = r2.set_stdin(b"abd");
            let c = same_digest(&f, &st, Algorithm::Sha1, &r2) == Ok(false);
            a && b && c
        },
        "static",
    );

    s.add(
        "SD06-标签格式",
        {
            let r = sample_registry();
            let p = prepare(&[b"--tag", b"-"]).unwrap_or(Plan::new());
            let out = render_plan(&p, Algorithm::Sha1, &r).unwrap_or(String::new());
            let a = out.as_str().starts_with("SHA1 (-) = ");
            let b = match Entry::parse(out.as_str().as_bytes()) {
                Ok(e) => e.tagged() && e.name_text().as_str() == "-" && e.alg() == Algorithm::Sha1,
                Err(_) => false,
            };
            a && b
        },
        "static",
    );

    s.add(
        "SD07-二进制标记",
        {
            let r = sample_registry();
            let p = prepare(&[b"--binary", b"data.txt"]).unwrap_or(Plan::new());
            let out = render_plan(&p, Algorithm::Sha1, &r).unwrap_or(String::new());
            let a = out.as_str().contains(" *data.txt
");
            let b = match Entry::parse(out.as_str().as_bytes()) {
                Ok(e) => !e.tagged() && e.name_text().as_str() == "data.txt",
                Err(_) => false,
            };
            let e0 = entry_for(&Target::stdin(), Algorithm::Sha1, &r).unwrap_or(Entry::blank());
            let c = binary_line(&e0).contains(" *-");
            a && b && c
        },
        "static",
    );

    s.add(
        "SD08-结束符",
        {
            end_marker_ok()
        },
        "static",
    );

    s.add(
        "SD09-缺失内容",
        {
            let q = prepare(&[b"nope.txt"]).unwrap_or(Plan::new());
            let t = q.first().unwrap_or(Target::blank());
            let r = Registry::new();
            let a = entry_for(&t, Algorithm::Sha1, &r).err() == Some(SD_ENOENT);
            let mut r2 = Registry::new();
            let _ = r2.insert(b"check.txt", b"a9993e364706816aba3e25717850c26c9cd0d89d  gone
");
            let p2 = prepare(&[b"--check", b"check.txt"]).unwrap_or(Plan::new());
            let b = match verify(&p2, &r2) {
                Ok(rep) => rep.missing == 1 && rep.good == 0,
                Err(_) => false,
            };
            a && b
        },
        "static",
    );

    s.add(
        "SD10-空计划",
        {
            let a = prepare(&[]).err() == Some(SD_ESHORT);
            let mut r = sample_registry();
            let _ = r.insert(b"check.txt", b"");
            let p0 = Plan::new();
            let b = verify(&p0, &r).err() == Some(SD_ESTATE);
            let p = prepare(&[b"-"]).unwrap_or(Plan::new());
            let c = render_plan(&p, Algorithm::Sha1, &r).is_ok();
            a && b && c
        },
        "static",
    );

    s.add(
        "SD11-账本对账",
        {
            let l = build_ledger();
            let a = l.len() == SAMPLES;
            let b = l.ok_count() == 8;
            let c = l.err_count() == 6;
            let d = l.audit() == Ok(());
            let e = l.sample(0).map(|s| s.value).unwrap_or(0) == 2;
            let f = l.sample(13).map(|s| s.code).unwrap_or(0) == SD_ESLOT;
            let g = {
                let mut m = Ledger::new();
                let s = Sample { slot: 3, op: 0, ok: false, code: 0, value: 0 };
                m.register(s) == Err(SD_ESLOT)
            };
            a && b && c && d && e && f && g
        },
        "static",
    );

    s.add(
        "SD12-冻结重放",
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
            let rejected = l.register(probe) == Err(SD_ESLOT) && l.rejects() == 1;
            let again = l.freeze() == Err(SD_EBAL) && l.rejects() == 2;
            same && frozen && replay && rejected && again
        },
        "static",
    );

    s.add(
        "SD13-冻结重放四注入",
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
    fn classify_table() {
        assert_eq!(classify(b"-"), Ok(Opt::Stdin));
        assert_eq!(classify(b"--"), Ok(Opt::End));
        assert_eq!(classify(b"--tag"), Ok(Opt::Tag));
        assert_eq!(classify(b"-b"), Ok(Opt::Binary));
        assert_eq!(classify(b"-t"), Ok(Opt::Text));
        assert_eq!(classify(b"-c"), Ok(Opt::Check));
        assert_eq!(classify(b"--quiet"), Ok(Opt::Quiet));
        assert_eq!(classify(b"foo"), Ok(Opt::File));
        assert_eq!(classify(b"--wat"), Err(SD_EINVAL));
        assert_eq!(classify(b""), Err(SD_ESHORT));
        assert!(Opt::Tag.is_option());
        assert!(!Opt::File.is_option());
        assert_eq!(Kind::Stdin.text(), "stdin");
    }

    #[test]
    fn plan_builds_targets() {
        let p = prepare(&[b"alpha", b"-", b"beta"]).expect("plan");
        assert_eq!(p.len(), 3);
        assert_eq!(p.stdin_targets(), 1);
        assert!(p.at(1).map(|t| t.is_stdin()).unwrap_or(false));
        assert!(p.at(0).map(|t| t.named(b"alpha")).unwrap_or(false));
        assert!(!p.is_empty());
    }

    #[test]
    fn option_conflicts() {
        assert_eq!(prepare(&[b"--binary", b"--text", b"-"]).err(), Some(SD_ECONF));
        assert_eq!(prepare(&[b"--quiet", b"-"]).err(), Some(SD_ECONF));
        assert!(prepare(&[b"--check", b"--quiet", b"-"]).is_ok());
        assert_eq!(prepare(&[b"--tag", b"--tag", b"-"]).err(), Some(SD_EDUP));
    }

    #[test]
    fn empty_plan_rejected() {
        assert_eq!(prepare(&[]).err(), Some(SD_ESHORT));
        assert_eq!(prepare(&[b"--tag"]).err(), Some(SD_ESHORT));
    }

    #[test]
    fn end_marker_swallows_options() {
        let p = prepare(&[b"--", b"--tag", b"-"]).expect("plan");
        assert!(p.end_seen());
        assert!(!p.tag());
        assert!(p.at(0).map(|t| t.named(b"--tag")).unwrap_or(false));
        assert!(p.at(1).map(|t| t.is_stdin()).unwrap_or(false));
    }

    #[test]
    fn stdin_target_presented_as_dash() {
        let t = Target::stdin();
        assert_eq!(t.name_text().as_str(), "-");
        assert_eq!(t.kind(), Kind::Stdin);
        assert!(t.is_stdin());
    }

    #[test]
    fn registry_roundtrip() {
        let mut r = Registry::new();
        assert!(r.is_empty());
        assert_eq!(r.insert(b"a.txt", b"abc"), Ok(()));
        assert_eq!(r.insert(b"a.txt", b"abc"), Err(SD_EDUP));
        assert_eq!(r.len(), 1);
        assert_eq!(r.find(b"a.txt"), Some(&b"abc"[..]));
        assert_eq!(r.find(b"b.txt"), None);
        assert_eq!(r.set_stdin(b"xyz"), Ok(()));
        assert_eq!(r.stdin_data(), b"xyz");
    }

    #[test]
    fn digest_of_stdin_matches_file() {
        let r = sample_registry();
        let f = Target::path(b"data.txt").expect("path");
        let st = Target::stdin();
        assert_eq!(same_digest(&f, &st, Algorithm::Sha1, &r), Ok(true));
        assert_eq!(same_digest(&f, &st, Algorithm::Blake2b256, &r), Ok(true));
        assert_eq!(same_digest(&f, &st, Algorithm::Blake2b512, &r), Ok(true));
    }

    #[test]
    fn render_three_forms() {
        let r = sample_registry();
        let p = prepare(&[b"-"]).expect("plan");
        let plain = render_plan(&p, Algorithm::Sha1, &r).expect("plain");
        assert!(plain.as_str().ends_with("  -\n"));
        let pt = prepare(&[b"--tag", b"-"]).expect("tag");
        let tagged = render_plan(&pt, Algorithm::Sha1, &r).expect("tagged");
        assert!(tagged.as_str().starts_with("SHA1 (-) = "));
        let pb = prepare(&[b"--binary", b"data.txt"]).expect("bin");
        let bin = render_plan(&pb, Algorithm::Sha1, &r).expect("binary");
        assert!(bin.as_str().contains(" *data.txt\n"));
    }

    #[test]
    fn verify_counts() {
        let mut r = sample_registry();
        r.insert(b"check.txt", SAMPLE_CHECK_FILE).expect("insert");
        let p = prepare(&[b"--check", b"check.txt"]).expect("plan");
        let rep = verify(&p, &r).expect("verify");
        assert_eq!(rep.checked, 2);
        assert_eq!(rep.good, 2);
        assert_eq!(rep.bad, 0);
        assert!(rep.all_good());
        assert!(rep.text().as_str().starts_with("checked 2 good 2"));
    }

    #[test]
    fn verify_bad_line() {
        let mut r = Registry::new();
        r.insert(b"check.txt", b"0000000000000000000000000000000000000000  -\n")
            .expect("insert");
        r.set_stdin(b"abc").expect("stdin");
        let p = prepare(&[b"--check", b"check.txt"]).expect("plan");
        let rep = verify(&p, &r).expect("verify");
        assert_eq!(rep.bad, 1);
        assert!(!rep.all_good());
    }

    #[test]
    fn verify_missing_and_malformed() {
        let mut r = Registry::new();
        r.insert(b"check.txt", b"garbage line\na9993e364706816aba3e25717850c26c9cd0d89d  gone\n")
            .expect("insert");
        let p = prepare(&[b"--check", b"check.txt"]).expect("plan");
        let rep = verify(&p, &r).expect("verify");
        assert_eq!(rep.malformed, 1);
        assert_eq!(rep.missing, 1);
        let ps = prepare(&[b"--check", b"--strict", b"check.txt"]).expect("plan");
        assert_eq!(verify(&ps, &r).err(), Some(SD_EINVAL));
    }

    #[test]
    fn verify_requires_check_mode() {
        let p = prepare(&[b"-"]).expect("plan");
        let r = sample_registry();
        assert_eq!(verify(&p, &r).err(), Some(SD_ESTATE));
        assert_eq!(render_plan(&p, Algorithm::Sha1, &r).is_ok(), true);
        let pc = prepare(&[b"--check", b"-"]).expect("plan");
        assert_eq!(render_plan(&pc, Algorithm::Sha1, &r).err(), Some(SD_ESTATE));
    }

    #[test]
    fn capacity_overflow() {
        assert!(overflow_probe().err() == Some(SD_ESLOT));
    }

    #[test]
    fn ledger_balanced() {
        let l = build_ledger();
        assert_eq!(l.audit(), Ok(()));
        assert_eq!(l.len(), SAMPLES);
        assert_eq!(l.sample(0).map(|s| s.value), Some(2));
        assert_eq!(l.sample(13).map(|s| s.code), Some(SD_ESLOT));
    }

    #[test]
    fn freeze_replay_freeze_again() {
        let mut l = build_ledger();
        let root = l.freeze().expect("freeze");
        assert!(l.is_frozen());
        assert_eq!(l.replay(), Ok(root));
        assert_eq!(l.freeze(), Err(SD_EBAL));
        assert_eq!(l.register(Sample::blank()), Err(SD_ESLOT));
    }

    #[test]
    fn injections_green() {
        let four = inject_four();
        assert!(four[0] && four[1] && four[2] && four[3]);
    }

    #[test]
    fn all_checks_green() {
        let s = run_unx11539_checks();
        assert!(s.all_passed());
    }
}
