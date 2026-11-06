//! UNX-F11541 nice 进程谦让值
//!
//! `nice` 只调一个数：进程的谦让值（niceness）。数越小越抢，数越大越让；
//! 合法区间是闭区间 `[-20, 19]`，共四十档。工具本身不改调度器，只把
//! 「当前档 + 调整量」算成新档，超界就钳到端点，然后带着新档去执行命令。
//! 看似一行算式，实际有三处容易做错：
//!
//! 1. **调整量来源**。缺省不带 `-n` 时调整量固定 `+10`；`-n 5`、`-n5`
//!    不认、`--adjustment=5`、`--adjustment 5` 都要认；老写法 `-5`（负号
//!    后全是数字）与 `+5` 也认。`-n` 后面缺值要单独报短，不能把下一个
//!    选项当成值吞掉。
//! 2. **钳制**。`当前 15 + 调整 10` 不是 25，而是 `19`（上钳）；`-15 -10`
//!    不是 -25，而是 `-20`（下钳）。钳制与否要能被观察者看见，否则调用方
//!    无法区分「正好落在端点」与「被截断」。
//! 3. **只查询不执行**。不带命令时 `nice` 是查询：打印当前档与调整后的档，
//!    不构造任何待执行命令。带了命令才是执行态，命令名逐字节校验。
//!
//! 闸门次序：参数分类（`NC_EOPT`）→ 取值缺失（`NC_ESHORT`）→ 数值非法
//! （`NC_EADJ`）→ 数值溢出（`NC_ERANGE`）→ 选项重复（`NC_ESTATE`）→
//! 名称字节（`NC_EBYTE`）→ 命令过长（`NC_ELEN`）→ 档位越界（`NC_ELEVEL`）。
//!
//! 账本语义：十四格 = 八格成功（缺省调整、显式调整、老写法绝对值、上钳档、
//! 下钳档、零调整档、可解析写法数、档位跨度）+ 六格失败（非数字、缺值、
//! 溢出、未知选项、命令过多、名称字节）。`audit` 认「八值六码」，
//! `seal` 做 FNV-1a 折叠，`replay` 重建比对，`freeze` 后拒绝登记。

use crate::alloc::string::String;
use crate::checks::CheckSet;

/// 段基址。
pub const SEG_BASE: u32 = 0xf2e0;

/// 调整量不是十进制整数。
pub const NC_EADJ: u32 = SEG_BASE + 0x01;
/// 调整量溢出。
pub const NC_ERANGE: u32 = SEG_BASE + 0x02;
/// 取值缺失。
pub const NC_ESHORT: u32 = SEG_BASE + 0x03;
/// 未知选项。
pub const NC_EOPT: u32 = SEG_BASE + 0x04;
/// 名称或命令过长。
pub const NC_ELEN: u32 = SEG_BASE + 0x05;
/// 名称字节非法。
pub const NC_EBYTE: u32 = SEG_BASE + 0x06;
/// 档位越界或结果不符。
pub const NC_ELEVEL: u32 = SEG_BASE + 0x07;
/// 状态冲突。
pub const NC_ESTATE: u32 = SEG_BASE + 0x08;
/// 账本失衡。
pub const NC_EBAL: u32 = SEG_BASE + 0x09;
/// 槽位错位或容量溢出。
pub const NC_ESLOT: u32 = SEG_BASE + 0x0a;
/// 值不符。
pub const NC_EVALUE: u32 = SEG_BASE + 0x0b;
/// 指纹不符。
pub const NC_ESEAL: u32 = SEG_BASE + 0x0c;
/// 观测缺失。
pub const NC_ENOBS: u32 = SEG_BASE + 0x0d;

/// 账本格数。
pub const SAMPLES: usize = 14;

/// 最低档（最抢）。
pub const NICE_MIN: i32 = -20;

/// 最高档（最让）。
pub const NICE_MAX: i32 = 19;

/// 档位跨度。
pub const NICE_SPAN: u32 = 40;

/// 缺省调整量。
pub const DEFAULT_ADJ: i32 = 10;

/// 调整量绝对值上限，超过即视为溢出。
pub const ADJ_LIMIT: i64 = 1_000_000;

/// 命令容量。
pub const CMD_CAP: usize = 4;

/// 命令名容量。
pub const CMD_NAME_CAP: usize = 64;

/// 进程容量。
pub const PROC_CAP: usize = 8;

/// 进程名容量。
pub const PROC_NAME_CAP: usize = 40;

/// 操作码：解析调整量。
pub const OP_ADJ: u8 = 0x01;
/// 操作码：解析命令行。
pub const OP_ARGV: u8 = 0x02;
/// 操作码：算新档。
pub const OP_APPLY: u8 = 0x03;
/// 操作码：钳制。
pub const OP_CLAMP: u8 = 0x04;
/// 操作码：查询。
pub const OP_QUERY: u8 = 0x05;
/// 操作码：批处理。
pub const OP_BATCH: u8 = 0x06;
/// 操作码：渲染。
pub const OP_RENDER: u8 = 0x07;
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

/// 十进制拼接（无符号）。
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

/// 十进制拼接（有符号）。
fn push_int(s: &mut String, v: i32) {
    if v < 0 {
        s.push('-');
        let mag = (v as i64).unsigned_abs();
        push_dec(s, mag);
    } else {
        push_dec(s, v as u64);
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

/// 全十进制数字且非空。
fn all_digits(s: &[u8]) -> bool {
    if s.is_empty() {
        return false;
    }
    let mut i = 0usize;
    while i < s.len() {
        let b = match s.get(i) {
            Some(v) => *v,
            None => return false,
        };
        if b < b'0' || b > b'9' {
            return false;
        }
        i += 1;
    }
    true
}

/// 名称字节是否可呈现：0x21..=0x7e。
fn name_bytes_ok(s: &[u8]) -> bool {
    if s.is_empty() || s.len() > CMD_NAME_CAP {
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
// 调整量
// ---------------------------------------------------------------------------

/// 解析十进制有符号整数。
pub fn parse_int(raw: &[u8]) -> Result<i32, u32> {
    if raw.is_empty() {
        return Err(NC_EADJ);
    }
    if raw.len() > 12 {
        return Err(NC_ERANGE);
    }
    let (neg, body) = match raw.first() {
        Some(b'-') => (true, raw.get(1..).unwrap_or(&[])),
        Some(b'+') => (false, raw.get(1..).unwrap_or(&[])),
        Some(_) => (false, raw),
        None => return Err(NC_EADJ),
    };
    if !all_digits(body) {
        return Err(NC_EADJ);
    }
    let mut acc = 0i64;
    let mut i = 0usize;
    while i < body.len() {
        let b = match body.get(i) {
            Some(v) => *v,
            None => return Err(NC_EADJ),
        };
        acc = acc.saturating_mul(10);
        acc = acc.saturating_add((b - b'0') as i64);
        if acc > (i32::MAX as i64) + 1 {
            return Err(NC_ERANGE);
        }
        i += 1;
    }
    let signed = if neg { -acc } else { acc };
    if signed < i32::MIN as i64 || signed > i32::MAX as i64 {
        return Err(NC_ERANGE);
    }
    Ok(signed as i32)
}

/// 解析调整量：整数字面 + 绝对值上限。
pub fn parse_adj(raw: &[u8]) -> Result<i32, u32> {
    let v = parse_int(raw)?;
    let mag = (v as i64).unsigned_abs();
    if mag > ADJ_LIMIT as u64 {
        return Err(NC_ERANGE);
    }
    Ok(v)
}

// ---------------------------------------------------------------------------
// 档位
// ---------------------------------------------------------------------------

/// 合法档位。构造即校验，内部值恒在 `[NICE_MIN, NICE_MAX]`。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct NiceLevel {
    value: i32,
}

impl NiceLevel {
    /// 构造。
    pub fn new(v: i32) -> Result<NiceLevel, u32> {
        if v < NICE_MIN || v > NICE_MAX {
            return Err(NC_ELEVEL);
        }
        Ok(NiceLevel { value: v })
    }

    /// 最低档。
    pub const fn min() -> NiceLevel {
        NiceLevel { value: NICE_MIN }
    }

    /// 最高档。
    pub const fn max() -> NiceLevel {
        NiceLevel { value: NICE_MAX }
    }

    /// 缺省档。
    pub const fn zero() -> NiceLevel {
        NiceLevel { value: 0 }
    }

    /// 空白档（最低档），便于 `unwrap_or`。
    pub const fn blank() -> NiceLevel {
        NiceLevel { value: NICE_MIN }
    }

    /// 原值。
    pub fn raw(&self) -> i32 {
        self.value
    }

    /// 是否处于端点。
    pub fn at_edge(&self) -> bool {
        self.value == NICE_MIN || self.value == NICE_MAX
    }

    /// 与最低档的距离（0..=39）。
    pub fn offset(&self) -> u32 {
        (self.value - NICE_MIN) as u32
    }

    /// 文本。
    pub fn text(&self) -> String {
        let mut s = String::new();
        push_int(&mut s, self.value);
        s
    }

    /// 由文本解析，越界给 `NC_ELEVEL`。
    pub fn parse(raw: &[u8]) -> Result<NiceLevel, u32> {
        let v = parse_int(raw)?;
        NiceLevel::new(v)
    }

    /// 加调整量并钳制。
    pub fn add(&self, delta: i32) -> (NiceLevel, bool) {
        let raw = self.value as i64 + delta as i64;
        if raw < NICE_MIN as i64 {
            (NiceLevel { value: NICE_MIN }, true)
        } else if raw > NICE_MAX as i64 {
            (NiceLevel { value: NICE_MAX }, true)
        } else {
            (NiceLevel { value: raw as i32 }, false)
        }
    }
}

/// 一次调整的结果。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Outcome {
    /// 调整前。
    pub base: i32,
    /// 调整量。
    pub delta: i32,
    /// 调整后。
    pub new: i32,
    /// 是否被钳制。
    pub clamped: bool,
}

impl Outcome {
    /// 空白。
    pub const fn blank() -> Outcome {
        Outcome { base: 0, delta: 0, new: 0, clamped: false }
    }

    /// 是否真的改变。
    pub fn changed(&self) -> bool {
        self.new != self.base
    }

    /// 一行文本：`<base> adj <delta> -> <new>`，被钳制时尾部加 ` *`。
    pub fn text(&self) -> String {
        let mut s = String::new();
        push_int(&mut s, self.base);
        s.push_str(" adj ");
        push_int(&mut s, self.delta);
        s.push_str(" -> ");
        push_int(&mut s, self.new);
        if self.clamped {
            s.push_str(" *");
        }
        s
    }
}

/// 对当前档施加调整量。
pub fn apply(base: i32, delta: i32) -> Result<Outcome, u32> {
    let level = NiceLevel::new(base)?;
    let (next, clamped) = level.add(delta);
    Ok(Outcome { base: level.raw(), delta, new: next.raw(), clamped })
}

// ---------------------------------------------------------------------------
// 命令名
// ---------------------------------------------------------------------------

/// 命令名。
#[derive(Clone, Copy, Debug)]
pub struct Cmd {
    name: [u8; CMD_NAME_CAP],
    len: usize,
}

impl Cmd {
    /// 空白。
    pub const fn blank() -> Cmd {
        Cmd { name: [0u8; CMD_NAME_CAP], len: 0 }
    }

    /// 构造。
    pub fn new(name: &[u8]) -> Result<Cmd, u32> {
        if name.is_empty() {
            return Err(NC_EBYTE);
        }
        if name.len() > CMD_NAME_CAP {
            return Err(NC_ELEN);
        }
        if !name_bytes_ok(name) {
            return Err(NC_EBYTE);
        }
        let mut c = Cmd::blank();
        let mut i = 0usize;
        while i < name.len() {
            if let Some(b) = name.get(i) {
                if let Some(dst) = c.name.get_mut(i) {
                    *dst = *b;
                }
            }
            i += 1;
        }
        c.len = name.len();
        Ok(c)
    }

    /// 名字字节。
    pub fn name(&self) -> &[u8] {
        self.name.get(0..self.len).unwrap_or(&[])
    }

    /// 文本。
    pub fn text(&self) -> String {
        let mut s = String::new();
        let mut i = 0usize;
        while i < self.len {
            if let Some(b) = self.name.get(i) {
                s.push(*b as char);
            }
            i += 1;
        }
        s
    }

    /// 名字是否为给定字节。
    pub fn named(&self, other: &[u8]) -> bool {
        eq_bytes(self.name(), other)
    }

    /// 长度。
    pub fn len(&self) -> usize {
        self.len
    }

    /// 是否空。
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }
}

// ---------------------------------------------------------------------------
// 调用
// ---------------------------------------------------------------------------

/// 一次 `nice` 调用。
#[derive(Clone, Copy, Debug)]
pub struct Invocation {
    delta: i32,
    explicit: bool,
    query: bool,
    end: bool,
    cmds: [Cmd; CMD_CAP],
    len: usize,
}

impl Invocation {
    /// 空白。
    pub const fn blank() -> Invocation {
        Invocation {
            delta: 0,
            explicit: false,
            query: true,
            end: false,
            cmds: [Cmd::blank(); CMD_CAP],
            len: 0,
        }
    }

    /// 有效调整量。
    pub fn delta(&self) -> i32 {
        self.delta
    }

    /// 是否显式给过调整量。
    pub fn explicit(&self) -> bool {
        self.explicit
    }

    /// 是否查询态（没有命令）。
    pub fn query(&self) -> bool {
        self.query
    }

    /// 是否见过 `--`。
    pub fn end_seen(&self) -> bool {
        self.end
    }

    /// 命令数。
    pub fn command_len(&self) -> usize {
        self.len
    }

    /// 是否无命令。
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// 取命令。
    pub fn at(&self, i: usize) -> Option<Cmd> {
        if i >= self.len {
            return None;
        }
        self.cmds.get(i).copied()
    }

    /// 首命令。
    pub fn first(&self) -> Option<Cmd> {
        self.at(0)
    }

    /// 命令整行文本（空格分隔）。
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

    /// 预演：把本调用作用在当前档上。
    pub fn preview(&self, base: i32) -> Result<Outcome, u32> {
        apply(base, self.delta)
    }

    /// 查询文本：两行，`current` 与 `adjusted`。
    pub fn query_text(&self, base: i32) -> Result<String, u32> {
        if !self.query {
            return Err(NC_ESTATE);
        }
        let o = self.preview(base)?;
        let mut s = String::new();
        s.push_str("current ");
        push_int(&mut s, o.base);
        s.push('\n');
        s.push_str("adjusted ");
        push_int(&mut s, o.new);
        if o.clamped {
            s.push_str(" *");
        }
        s.push('\n');
        Ok(s)
    }

    /// 推入命令。
    fn push_cmd(&mut self, name: &[u8]) -> Result<(), u32> {
        if self.len >= CMD_CAP {
            return Err(NC_ELEN);
        }
        let c = Cmd::new(name)?;
        if let Some(dst) = self.cmds.get_mut(self.len) {
            *dst = c;
        }
        self.len += 1;
        Ok(())
    }

    /// 记录调整量，第二次给即状态冲突。
    fn set_delta(&mut self, v: i32) -> Result<(), u32> {
        if self.explicit {
            return Err(NC_ESTATE);
        }
        self.delta = v;
        self.explicit = true;
        Ok(())
    }
}

/// 解析命令行。选项必须出现在第一个命令名之前；`--` 之后全是命令名。
pub fn parse_argv(args: &[&[u8]]) -> Result<Invocation, u32> {
    let mut inv = Invocation::blank();
    if args.is_empty() {
        inv.delta = 0;
        inv.query = true;
        return Ok(inv);
    }
    let mut i = 0usize;
    let mut operands = false;
    while i < args.len() {
        let a = match args.get(i) {
            Some(v) => *v,
            None => return Err(NC_ESHORT),
        };
        if !operands {
            if eq_bytes(a, b"--") {
                inv.end = true;
                operands = true;
                i += 1;
                continue;
            }
            if eq_bytes(a, b"-n") {
                let next = match args.get(i + 1) {
                    Some(v) => *v,
                    None => return Err(NC_ESHORT),
                };
                let v = parse_adj(next)?;
                inv.set_delta(v)?;
                i += 2;
                continue;
            }
            if eq_bytes(a, b"--adjustment") {
                let next = match args.get(i + 1) {
                    Some(v) => *v,
                    None => return Err(NC_ESHORT),
                };
                let v = parse_adj(next)?;
                inv.set_delta(v)?;
                i += 2;
                continue;
            }
            if starts_with(a, b"--adjustment=") {
                let rest = a.get(13..).unwrap_or(&[]);
                let v = parse_adj(rest)?;
                inv.set_delta(v)?;
                i += 1;
                continue;
            }
            if starts_with(a, b"--") {
                return Err(NC_EOPT);
            }
            if a.len() > 1 && a.first() == Some(&b'-') {
                let rest = a.get(1..).unwrap_or(&[]);
                if all_digits(rest) {
                    let v = parse_adj(rest)?;
                    inv.set_delta(-v)?;
                    i += 1;
                    continue;
                }
                return Err(NC_EOPT);
            }
            if a.len() > 1 && a.first() == Some(&b'+') {
                let rest = a.get(1..).unwrap_or(&[]);
                if all_digits(rest) {
                    let v = parse_adj(rest)?;
                    inv.set_delta(v)?;
                    i += 1;
                    continue;
                }
                return Err(NC_EADJ);
            }
            operands = true;
        }
        inv.push_cmd(a)?;
        i += 1;
    }
    if !inv.explicit {
        inv.delta = if inv.len == 0 { 0 } else { DEFAULT_ADJ };
    }
    inv.query = inv.len == 0;
    Ok(inv)
}

// ---------------------------------------------------------------------------
// 批处理
// ---------------------------------------------------------------------------

/// 一个进程条目。
#[derive(Clone, Copy, Debug)]
pub struct Proc {
    name: [u8; PROC_NAME_CAP],
    len: usize,
    level: NiceLevel,
}

impl Proc {
    /// 空白。
    pub const fn blank() -> Proc {
        Proc { name: [0u8; PROC_NAME_CAP], len: 0, level: NiceLevel { value: 0 } }
    }

    /// 构造。
    pub fn new(name: &[u8], level: NiceLevel) -> Result<Proc, u32> {
        if name.is_empty() || !name_bytes_ok(name) {
            return Err(NC_EBYTE);
        }
        if name.len() > PROC_NAME_CAP {
            return Err(NC_ELEN);
        }
        let mut p = Proc::blank();
        let mut i = 0usize;
        while i < name.len() {
            if let Some(b) = name.get(i) {
                if let Some(dst) = p.name.get_mut(i) {
                    *dst = *b;
                }
            }
            i += 1;
        }
        p.len = name.len();
        p.level = level;
        Ok(p)
    }

    /// 名字。
    pub fn name(&self) -> &[u8] {
        self.name.get(0..self.len).unwrap_or(&[])
    }

    /// 名字文本。
    pub fn name_text(&self) -> String {
        let mut s = String::new();
        let mut i = 0usize;
        while i < self.len {
            if let Some(b) = self.name.get(i) {
                s.push(*b as char);
            }
            i += 1;
        }
        s
    }

    /// 档位。
    pub fn level(&self) -> NiceLevel {
        self.level
    }

    /// 是否同名。
    pub fn named(&self, other: &[u8]) -> bool {
        eq_bytes(self.name(), other)
    }
}

/// 一批进程。
#[derive(Clone, Copy, Debug)]
pub struct Schedule {
    rows: [Proc; PROC_CAP],
    len: usize,
}

impl Schedule {
    /// 空批。
    pub const fn new() -> Schedule {
        Schedule { rows: [Proc::blank(); PROC_CAP], len: 0 }
    }

    /// 登记。同名给 `NC_EVALUE`，集满给 `NC_ESLOT`。
    pub fn push(&mut self, p: Proc) -> Result<usize, u32> {
        let mut i = 0usize;
        while i < self.len {
            if let Some(x) = self.rows.get(i) {
                if x.named(p.name()) {
                    return Err(NC_EVALUE);
                }
            }
            i += 1;
        }
        if self.len >= PROC_CAP {
            return Err(NC_ESLOT);
        }
        let at = self.len;
        if let Some(dst) = self.rows.get_mut(at) {
            *dst = p;
        }
        self.len += 1;
        Ok(at)
    }

    /// 条数。
    pub fn len(&self) -> usize {
        self.len
    }

    /// 是否空。
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// 取一条。
    pub fn at(&self, i: usize) -> Option<Proc> {
        if i >= self.len {
            return None;
        }
        self.rows.get(i).copied()
    }

    /// 统一施加调整量，返回被改动条数。钳制逐条生效。
    pub fn apply_all(&mut self, delta: i32) -> Result<u32, u32> {
        if self.len == 0 {
            return Err(NC_ESHORT);
        }
        let mut changed = 0u32;
        let mut i = 0usize;
        while i < self.len {
            let mut moved = false;
            if let Some(p) = self.rows.get_mut(i) {
                let (next, _) = p.level.add(delta);
                if next.raw() != p.level.raw() {
                    moved = true;
                }
                p.level = next;
            }
            if moved {
                changed = changed.saturating_add(1);
            }
            i += 1;
        }
        Ok(changed)
    }

    /// 档位区间 `(最低, 最高)`。
    pub fn spread(&self) -> Result<(i32, i32), u32> {
        if self.len == 0 {
            return Err(NC_ESHORT);
        }
        let mut lo = NICE_MAX;
        let mut hi = NICE_MIN;
        let mut i = 0usize;
        while i < self.len {
            if let Some(p) = self.rows.get(i) {
                let v = p.level.raw();
                if v < lo {
                    lo = v;
                }
                if v > hi {
                    hi = v;
                }
            }
            i += 1;
        }
        Ok((lo, hi))
    }

    /// 渲染，每条一行 `<name> <level>`。
    pub fn render(&self) -> String {
        let mut s = String::new();
        let mut i = 0usize;
        while i < self.len {
            if let Some(p) = self.rows.get(i) {
                s.push_str(p.name_text().as_str());
                s.push(' ');
                s.push_str(p.level().text().as_str());
                s.push('\n');
            }
            i += 1;
        }
        s
    }
}

/// 样批：四档各一。
pub fn sample_schedule() -> Schedule {
    let mut sc = Schedule::new();
    let pairs: [(&[u8], i32); 4] = [(b"alpha", 0), (b"beta", 10), (b"gamma", 19), (b"delta", -20)];
    let mut i = 0usize;
    while i < pairs.len() {
        if let Some((name, v)) = pairs.get(i) {
            if let Ok(level) = NiceLevel::new(*v) {
                if let Ok(p) = Proc::new(name, level) {
                    let _ = sc.push(p);
                }
            }
        }
        i += 1;
    }
    sc
}

// ---------------------------------------------------------------------------
// 账本观测
// ---------------------------------------------------------------------------

/// 缺省调整探针。
fn default_delta() -> Result<u32, u32> {
    parse_argv(&[b"sleep"]).map(|v| v.delta() as u32)
}

/// 显式调整探针。
fn explicit_delta() -> Result<u32, u32> {
    parse_argv(&[b"-n", b"5", b"sleep"]).map(|v| v.delta() as u32)
}

/// 老写法绝对值探针。
fn legacy_mag() -> Result<u32, u32> {
    parse_argv(&[b"-7", b"sleep"]).map(|v| {
        -(v.delta() as i64)
    })
    .map(|v| v as u32)
}

/// 观测：档位编码为与最低档的距离。
fn coded(base: i32, delta: i32) -> Result<u32, u32> {
    apply(base, delta).map(|o| (o.new - NICE_MIN) as u32)
}

/// 零调整探针。
fn zero_shift() -> Result<u32, u32> {
    coded(0, 0)
}

/// 可解析写法数。
fn adj_forms() -> Result<usize, u32> {
    let mut n = 0usize;
    let forms: [&[u8]; 5] = [b"5", b"+5", b"-5", b"0", b"123"];
    let mut i = 0usize;
    while i < forms.len() {
        if let Some(f) = forms.get(i) {
            if parse_adj(f).is_ok() {
                n += 1;
            }
        }
        i += 1;
    }
    if n == forms.len() {
        Ok(n)
    } else {
        Err(NC_EADJ)
    }
}

/// 档位跨度。
fn span_probe() -> Result<u32, u32> {
    let lo = NiceLevel::min();
    let hi = NiceLevel::max();
    let span = hi.raw() - lo.raw() + 1;
    if span as u32 == NICE_SPAN {
        Ok(span as u32)
    } else {
        Err(NC_ELEVEL)
    }
}

/// 非数字探针。
fn bad_adj_probe() -> Result<u32, u32> {
    parse_adj(b"abc").map(|_| 0)
}

/// 缺值探针。
fn short_probe() -> Result<u32, u32> {
    parse_argv(&[b"-n"]).map(|_| 0)
}

/// 溢出探针。
fn range_probe() -> Result<u32, u32> {
    parse_adj(b"999999999999").map(|_| 0)
}

/// 未知选项探针。
fn opt_probe() -> Result<u32, u32> {
    parse_argv(&[b"--bogus", b"sleep"]).map(|_| 0)
}

/// 命令过多探针。
fn len_probe() -> Result<u32, u32> {
    parse_argv(&[b"a", b"b", b"c", b"d", b"e"]).map(|_| 0)
}

/// 名称字节探针。
fn byte_probe() -> Result<u32, u32> {
    parse_argv(&[b"\x01"]).map(|_| 0)
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
            return Err(NC_ESLOT);
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
            return Err(NC_EBAL);
        }
        const VALS: [u32; 8] = [10, 5, 7, 39, 0, 20, 5, 40];
        const CODES: [u32; 6] = [NC_EADJ, NC_ESHORT, NC_ERANGE, NC_EOPT, NC_ELEN, NC_EBYTE];
        let mut vi = 0usize;
        let mut ci = 0usize;
        let mut i = 0usize;
        while i < self.len {
            let s = match self.rows.get(i) {
                Some(v) => *v,
                None => return Err(NC_EBAL),
            };
            if s.slot as usize != i {
                return Err(NC_ESLOT);
            }
            if s.ok {
                if s.code != 0 {
                    return Err(NC_EVALUE);
                }
                match VALS.get(vi) {
                    Some(w) => {
                        if s.value != *w {
                            return Err(NC_EVALUE);
                        }
                    }
                    None => return Err(NC_EBAL),
                }
                vi += 1;
            } else {
                match CODES.get(ci) {
                    Some(w) => {
                        if s.code != *w {
                            return Err(NC_EVALUE);
                        }
                    }
                    None => return Err(NC_EBAL),
                }
                ci += 1;
            }
            i += 1;
        }
        if vi != 8 || ci != 6 {
            return Err(NC_EBAL);
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
            return Err(NC_EBAL);
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
            Err(NC_ESEAL)
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

    let _ = l.register(word_cell(0, OP_ARGV, default_delta()));

    let _ = l.register(word_cell(1, OP_ARGV, explicit_delta()));

    let _ = l.register(word_cell(2, OP_ARGV, legacy_mag()));

    let _ = l.register(word_cell(3, OP_APPLY, coded(15, 10)));

    let _ = l.register(word_cell(4, OP_CLAMP, coded(-15, -10)));

    let _ = l.register(word_cell(5, OP_APPLY, zero_shift()));

    let _ = l.register(count_cell(6, OP_ADJ, adj_forms()));

    let _ = l.register(word_cell(7, OP_CLAMP, span_probe()));

    let _ = l.register(word_cell(8, OP_ADJ, bad_adj_probe()));

    let _ = l.register(word_cell(9, OP_ARGV, short_probe()));

    let _ = l.register(word_cell(10, OP_ADJ, range_probe()));

    let _ = l.register(word_cell(11, OP_ARGV, opt_probe()));

    let _ = l.register(word_cell(12, OP_ARGV, len_probe()));

    let _ = l.register(word_cell(13, OP_ARGV, byte_probe()));

    l
}

// ---------------------------------------------------------------------------
// 注入判据
// ---------------------------------------------------------------------------

/// 缺省与显式档。
fn delta_forms_ok() -> bool {
    let a = match parse_argv(&[b"sleep"]) {
        Ok(v) => v.delta() == DEFAULT_ADJ && !v.explicit() && !v.query(),
        Err(_) => false,
    };
    let b = match parse_argv(&[b"-n", b"5", b"sleep"]) {
        Ok(v) => v.delta() == 5 && v.explicit(),
        Err(_) => false,
    };
    let c = match parse_argv(&[b"--adjustment=7", b"sleep"]) {
        Ok(v) => v.delta() == 7,
        Err(_) => false,
    };
    let d = match parse_argv(&[b"--adjustment", b"-3", b"sleep"]) {
        Ok(v) => v.delta() == -3,
        Err(_) => false,
    };
    let e = match parse_argv(&[b"-7", b"sleep"]) {
        Ok(v) => v.delta() == -7,
        Err(_) => false,
    };
    let f = match parse_argv(&[b"+4", b"sleep"]) {
        Ok(v) => v.delta() == 4,
        Err(_) => false,
    };
    a && b && c && d && e && f
}

/// 钳制。
fn clamp_ok() -> bool {
    let a = match apply(15, 10) {
        Ok(o) => o.new == NICE_MAX && o.clamped && o.changed(),
        Err(_) => false,
    };
    let b = match apply(-15, -10) {
        Ok(o) => o.new == NICE_MIN && o.clamped,
        Err(_) => false,
    };
    let c = match apply(0, 0) {
        Ok(o) => o.new == 0 && !o.clamped && !o.changed(),
        Err(_) => false,
    };
    let d = match NiceLevel::new(19).map(|l| l.add(1)) {
        Ok((n, cl)) => n.raw() == 19 && cl,
        Err(_) => false,
    };
    let e = match NiceLevel::new(-20).map(|l| l.add(-1)) {
        Ok((n, cl)) => n.raw() == -20 && cl,
        Err(_) => false,
    };
    a && b && c && d && e
}

/// 命令与选项次序。
fn argv_order_ok() -> bool {
    let a = match parse_argv(&[b"-n", b"5", b"alpha", b"beta"]) {
        Ok(v) => v.command_len() == 2 && v.at(0).map(|c| c.named(b"alpha")).unwrap_or(false),
        Err(_) => false,
    };
    let b = match parse_argv(&[b"--", b"-n"]) {
        Ok(v) => v.end_seen() && v.command_len() == 1 && v.at(0).map(|c| c.named(b"-n")).unwrap_or(false),
        Err(_) => false,
    };
    let c = match parse_argv(&[b"alpha", b"-n", b"5"]) {
        Ok(v) => v.command_len() == 3 && v.delta() == DEFAULT_ADJ,
        Err(_) => false,
    };
    let d = match parse_argv(&[]) {
        Ok(v) => v.query() && v.delta() == 0 && v.command_len() == 0,
        Err(_) => false,
    };
    let e = match parse_argv(&[b"-n", b"0", b"sleeper"]) {
        Ok(v) => v.delta() == 0 && v.explicit(),
        Err(_) => false,
    };
    a && b && c && d && e
}

/// 文本与档位解析。
fn level_text_ok() -> bool {
    let a = NiceLevel::min().text().as_str() == "-20";
    let b = NiceLevel::zero().text().as_str() == "0";
    let c = NiceLevel::max().text().as_str() == "19";
    let d = match NiceLevel::parse(b"-12") {
        Ok(v) => v.raw() == -12,
        Err(_) => false,
    };
    let e = NiceLevel::parse(b"20").err() == Some(NC_ELEVEL);
    let f = NiceLevel::parse(b"-21").err() == Some(NC_ELEVEL);
    let g = NiceLevel::new(-20).map(|v| v.offset()).unwrap_or(9) == 0;
    let h = NiceLevel::new(19).map(|v| v.offset()).unwrap_or(9) == 39;
    a && b && c && d && e && f && g && h
}

/// 错误闸门。
fn error_gates_ok() -> bool {
    let a = parse_adj(b"abc").err() == Some(NC_EADJ);
    let b = parse_argv(&[b"-n"]).err() == Some(NC_ESHORT);
    let c = parse_adj(b"999999999999").err() == Some(NC_ERANGE);
    let d = parse_adj(b"2000000").err() == Some(NC_ERANGE);
    let e = parse_argv(&[b"--bogus"]).err() == Some(NC_EOPT);
    let f = parse_argv(&[b"-x"]).err() == Some(NC_EOPT);
    let g = parse_argv(&[b"a", b"b", b"c", b"d", b"e"]).err() == Some(NC_ELEN);
    let h = parse_argv(&[b"\x01"]).err() == Some(NC_EBYTE);
    let i = parse_argv(&[b"-n", b"1", b"-n", b"2"]).err() == Some(NC_ESTATE);
    let j = apply(20, 0).err() == Some(NC_ELEVEL);
    a && b && c && d && e && f && g && h && i && j
}

/// 批处理。
fn batch_ok() -> bool {
    let mut sc = sample_schedule();
    if sc.len() != 4 {
        return false;
    }
    let changed = match sc.apply_all(10) {
        Ok(v) => v,
        Err(_) => return false,
    };
    let a = changed == 3;
    let b = sc.at(0).map(|p| p.level().raw()).unwrap_or(0) == 10;
    let c = sc.at(1).map(|p| p.level().raw()).unwrap_or(0) == 19;
    let d = sc.at(2).map(|p| p.level().raw()).unwrap_or(0) == 19;
    let e = sc.at(3).map(|p| p.level().raw()).unwrap_or(0) == -10;
    let f = sc.spread() == Ok((-10, 19));
    let g = sc.render().as_str().starts_with("alpha 10\n");
    let h = Schedule::new().apply_all(10).err() == Some(NC_ESHORT);
    let i = {
        let p = match Proc::new(b"alpha", NiceLevel::zero()) {
            Ok(v) => v,
            Err(_) => Proc::blank(),
        };
        sc.push(p).err() == Some(NC_EVALUE)
    };
    a && b && c && d && e && f && g && h && i
}

/// 查询。
fn query_ok() -> bool {
    let bare = match parse_argv(&[]) {
        Ok(v) => v,
        Err(_) => Invocation::blank(),
    };
    let a = bare.query() && bare.query_text(0) == Ok(String::from("current 0\nadjusted 0\n"));
    let with_n = match parse_argv(&[b"-n", b"5"]) {
        Ok(v) => v,
        Err(_) => Invocation::blank(),
    };
    let b = with_n.query() && with_n.delta() == 5;
    let c = with_n.query_text(18).map(|t| t.as_str().contains("adjusted 19 *")).unwrap_or(false);
    let d = match parse_argv(&[b"sleep"]) {
        Ok(v) => v.query_text(0).err() == Some(NC_ESTATE),
        Err(_) => false,
    };
    a && b && c && d
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
    if l2.register(probe) != Err(NC_ESLOT) {
        return false;
    }
    l2.rejects() == 1
}

/// 四路注入。
pub fn inject_four() -> [bool; 4] {
    let mut out = [false; 4];
    out[0] = delta_forms_ok() && clamp_ok();
    out[1] = argv_order_ok() && error_gates_ok();
    out[2] = ledger_ok();
    out[3] = batch_ok() && query_ok() && level_text_ok();
    out
}

// ---------------------------------------------------------------------------
// 判据
// ---------------------------------------------------------------------------

/// 判据集合。
pub fn run_unx11541_checks() -> CheckSet {
    let mut s = CheckSet::new("unx11541.nice");

    s.add(
        "NC01-缺省调整",
        {
            default_delta() == Ok(DEFAULT_ADJ as u32) && delta_forms_ok()
        },
        "static",
    );

    s.add(
        "NC02-显式调整",
        {
            explicit_delta() == Ok(5) && legacy_mag() == Ok(7)
        },
        "static",
    );

    s.add(
        "NC03-超界钳制",
        {
            clamp_ok()
        },
        "static",
    );

    s.add(
        "NC04-参数次序",
        {
            argv_order_ok()
        },
        "static",
    );

    s.add(
        "NC05-档位文本",
        {
            level_text_ok()
        },
        "static",
    );

    s.add(
        "NC06-错误闸门",
        {
            error_gates_ok()
        },
        "static",
    );

    s.add(
        "NC07-批处理",
        {
            batch_ok()
        },
        "static",
    );

    s.add(
        "NC08-查询档",
        {
            query_ok()
        },
        "static",
    );

    s.add(
        "NC09-渲染",
        {
            let o = match apply(15, 10) {
                Ok(v) => v,
                Err(_) => Outcome::blank(),
            };
            let a = o.text().as_str() == "15 adj 10 -> 19 *";
            let b = match apply(0, 10) {
                Ok(v) => v.text().as_str() == "0 adj 10 -> 10",
                Err(_) => false,
            };
            let inv = match parse_argv(&[b"-n", b"2", b"alpha", b"beta"]) {
                Ok(v) => v,
                Err(_) => Invocation::blank(),
            };
            let c = inv.command_text().as_str() == "alpha beta";
            let sc = sample_schedule();
            let d = sc.render().as_str().contains("gamma 19\n")
                && sc.render().as_str().contains("delta -20\n");
            a && b && c && d
        },
        "static",
    );

    s.add(
        "NC10-账本对账",
        {
            let l = build_ledger();
            let a = l.len() == SAMPLES;
            let b = l.ok_count() == 8;
            let c = l.err_count() == 6;
            let d = l.audit() == Ok(());
            let e = l.sample(0).map(|s| s.value).unwrap_or(0) == 10;
            let f = l.sample(13).map(|s| s.code).unwrap_or(0) == NC_EBYTE;
            let g = {
                let mut m = Ledger::new();
                let s = Sample { slot: 3, op: 0, ok: false, code: 0, value: 0 };
                m.register(s) == Err(NC_ESLOT)
            };
            a && b && c && d && e && f && g
        },
        "static",
    );

    s.add(
        "NC11-冻结重放",
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
            let rejected = l.register(probe) == Err(NC_ESLOT) && l.rejects() == 1;
            let again = l.freeze() == Err(NC_EBAL) && l.rejects() == 2;
            same && frozen && replay && rejected && again
        },
        "static",
    );

    s.add(
        "NC12-容量与重名",
        {
            let mut ok = true;
            let mut sc = Schedule::new();
            let mut i = 0usize;
            while i < PROC_CAP {
                let name = [b'p', b'0' + i as u8];
                match Proc::new(&name, NiceLevel::zero()) {
                    Ok(p) => {
                        if sc.push(p).is_err() {
                            ok = false;
                        }
                    }
                    Err(_) => ok = false,
                }
                i += 1;
            }
            let extra = match Proc::new(b"p9", NiceLevel::zero()) {
                Ok(v) => v,
                Err(_) => Proc::blank(),
            };
            ok && sc.len() == PROC_CAP && sc.push(extra).err() == Some(NC_ESLOT)
        },
        "static",
    );

    s.add(
        "NC13-冻结重放四注入",
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
    fn parse_integers() {
        assert_eq!(parse_int(b"0"), Ok(0));
        assert_eq!(parse_int(b"42"), Ok(42));
        assert_eq!(parse_int(b"-7"), Ok(-7));
        assert_eq!(parse_int(b"+9"), Ok(9));
        assert_eq!(parse_int(b"-0"), Ok(0));
        assert_eq!(parse_int(b""), Err(NC_EADJ));
        assert_eq!(parse_int(b"-"), Err(NC_EADJ));
        assert_eq!(parse_int(b"1x"), Err(NC_EADJ));
        assert_eq!(parse_int(b"2147483648"), Err(NC_ERANGE));
        assert_eq!(parse_adj(b"1000000").is_ok(), true);
        assert_eq!(parse_adj(b"1000001"), Err(NC_ERANGE));
    }

    #[test]
    fn nice_range() {
        assert_eq!(NiceLevel::new(-20).map(|v| v.raw()), Ok(-20));
        assert_eq!(NiceLevel::new(19).map(|v| v.raw()), Ok(19));
        assert_eq!(NiceLevel::new(20), Err(NC_ELEVEL));
        assert_eq!(NiceLevel::new(-21), Err(NC_ELEVEL));
        assert!(NiceLevel::min().at_edge());
        assert!(NiceLevel::max().at_edge());
        assert!(!NiceLevel::zero().at_edge());
        assert_eq!(NiceLevel::max().offset(), 39);
    }

    #[test]
    fn defaults() {
        let inv = parse_argv(&[b"sleep"]).expect("inv");
        assert_eq!(inv.delta(), 10);
        assert!(!inv.explicit());
        assert!(!inv.query());
        assert_eq!(inv.command_text().as_str(), "sleep");
        let bare = parse_argv(&[]).expect("bare");
        assert!(bare.query());
        assert_eq!(bare.delta(), 0);
    }

    #[test]
    fn adjustment_forms() {
        assert_eq!(parse_argv(&[b"-n", b"5", b"x"]).map(|v| v.delta()), Ok(5));
        assert_eq!(parse_argv(&[b"--adjustment=7", b"x"]).map(|v| v.delta()), Ok(7));
        assert_eq!(parse_argv(&[b"--adjustment", b"-3", b"x"]).map(|v| v.delta()), Ok(-3));
        assert_eq!(parse_argv(&[b"-7", b"x"]).map(|v| v.delta()), Ok(-7));
        assert_eq!(parse_argv(&[b"+4", b"x"]).map(|v| v.delta()), Ok(4));
        assert_eq!(parse_argv(&[b"-n", b"0", b"x"]).map(|v| v.delta()), Ok(0));
    }

    #[test]
    fn option_gates() {
        assert_eq!(parse_argv(&[b"-n"]).err(), Some(NC_ESHORT));
        assert_eq!(parse_argv(&[b"--adjustment"]).err(), Some(NC_ESHORT));
        assert_eq!(parse_argv(&[b"--adjustment="]).err(), Some(NC_EADJ));
        assert_eq!(parse_argv(&[b"--bogus"]).err(), Some(NC_EOPT));
        assert_eq!(parse_argv(&[b"-x"]).err(), Some(NC_EOPT));
        assert_eq!(parse_argv(&[b"+x"]).err(), Some(NC_EADJ));
        assert_eq!(parse_argv(&[b"-n", b"1", b"-n", b"2"]).err(), Some(NC_ESTATE));
    }

    #[test]
    fn end_marker() {
        let inv = parse_argv(&[b"--", b"-n", b"-5"]).expect("inv");
        assert!(inv.end_seen());
        assert_eq!(inv.delta(), 10);
        assert_eq!(inv.command_len(), 2);
        assert!(inv.at(0).map(|c| c.named(b"-n")).unwrap_or(false));
        assert!(inv.at(1).map(|c| c.named(b"-5")).unwrap_or(false));
    }

    #[test]
    fn command_limits() {
        assert_eq!(parse_argv(&[b"a", b"b", b"c", b"d"]).map(|v| v.command_len()), Ok(4));
        assert_eq!(parse_argv(&[b"a", b"b", b"c", b"d", b"e"]).err(), Some(NC_ELEN));
        assert_eq!(parse_argv(&[b""]).err(), Some(NC_EBYTE));
        assert_eq!(parse_argv(&[b"a b"]).err(), Some(NC_EBYTE));
        let long = [b'x'; 65];
        let slice: &[u8] = &long;
        assert_eq!(parse_argv(&[slice]).err(), Some(NC_ELEN));
    }

    #[test]
    fn clamping() {
        assert_eq!(apply(15, 10).map(|o| (o.new, o.clamped)), Ok((19, true)));
        assert_eq!(apply(-15, -10).map(|o| (o.new, o.clamped)), Ok((-20, true)));
        assert_eq!(apply(0, 10).map(|o| (o.new, o.clamped)), Ok((10, false)));
        assert_eq!(apply(19, -39).map(|o| (o.new, o.clamped)), Ok((-20, false)));
        assert_eq!(apply(19, -40).map(|o| (o.new, o.clamped)), Ok((-20, true)));
        assert_eq!(apply(0, 0).map(|o| o.changed()), Ok(false));
    }

    #[test]
    fn outcome_text() {
        assert_eq!(apply(15, 10).map(|o| o.text()), Ok(String::from("15 adj 10 -> 19 *")));
        assert_eq!(apply(0, 10).map(|o| o.text()), Ok(String::from("0 adj 10 -> 10")));
        assert_eq!(Outcome::blank().text().as_str(), "0 adj 0 -> 0");
    }

    #[test]
    fn schedule_flow() {
        let mut sc = sample_schedule();
        assert_eq!(sc.len(), 4);
        assert_eq!(sc.apply_all(10), Ok(3));
        assert_eq!(sc.spread(), Ok((-10, 19)));
        assert_eq!(sc.at(1).map(|p| p.level().raw()), Some(19));
        assert!(sc.render().as_str().starts_with("alpha 10\n"));
        assert_eq!(sc.render().as_str().matches('\n').count(), 4);
    }

    #[test]
    fn schedule_duplicates() {
        let mut sc = Schedule::new();
        sc.push(Proc::new(b"alpha", NiceLevel::zero()).expect("p")).expect("push");
        assert_eq!(sc.push(Proc::new(b"alpha", NiceLevel::max()).expect("p")), Err(NC_EVALUE));
        assert_eq!(Proc::new(b"bad name", NiceLevel::zero()).err(), Some(NC_EBYTE));
        assert_eq!(sc.push(Proc::new(b"beta", NiceLevel::zero()).expect("p")), Ok(1));
        assert_eq!(Schedule::new().apply_all(1).err(), Some(NC_ESHORT));
        assert_eq!(Schedule::new().spread().err(), Some(NC_ESHORT));
    }

    #[test]
    fn query_texts() {
        let bare = parse_argv(&[]).expect("inv");
        assert_eq!(bare.query_text(0), Ok(String::from("current 0\nadjusted 0\n")));
        let five = parse_argv(&[b"-n", b"5"]).expect("inv");
        assert_eq!(five.query_text(18), Ok(String::from("current 18\nadjusted 19 *\n")));
        let exec = parse_argv(&[b"sleep"]).expect("inv");
        assert_eq!(exec.query_text(0).err(), Some(NC_ESTATE));
    }

    #[test]
    fn ledger_shape() {
        let l = build_ledger();
        assert_eq!(l.audit(), Ok(()));
        assert_eq!(l.len(), SAMPLES);
        assert_eq!(l.sample(0).map(|s| s.value), Some(10));
        assert_eq!(l.sample(2).map(|s| s.value), Some(7));
        assert_eq!(l.sample(13).map(|s| s.code), Some(NC_EBYTE));
    }

    #[test]
    fn ledger_replay() {
        let mut l = build_ledger();
        let root = l.freeze().expect("freeze");
        assert_eq!(l.replay(), Ok(root));
        assert_eq!(l.freeze(), Err(NC_EBAL));
        assert_eq!(l.register(Sample::blank()), Err(NC_ESLOT));
    }

    #[test]
    fn injections() {
        let four = inject_four();
        assert!(four[0] && four[1] && four[2] && four[3]);
    }

    #[test]
    fn all_checks_green() {
        let s = run_unx11541_checks();
        assert!(s.all_passed());
    }
}
